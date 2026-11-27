use super::{SelectionCopy, SelectionError, read_open_clipboard};
use std::{
    ptr::null_mut,
    thread::sleep,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, GetLastError, GlobalFree, HANDLE, HWND, SetLastError},
    Graphics::Gdi::{DeleteEnhMetaFile, DeleteMetaFile, DeleteObject},
    System::{
        DataExchange::*,
        Memory::{GMEM_MOVEABLE, GlobalLock, GlobalUnlock},
        Ole::*,
    },
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::GetWindowThreadProcessId},
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

const POLL: Duration = Duration::from_millis(5);
const OPEN_WAIT: Duration = Duration::from_millis(50);

struct Backup(Vec<(u16, HANDLE)>);

impl Backup {
    fn capture() -> Option<Self> {
        let mut backup = Self(Vec::new());
        let mut format = 0;
        loop {
            unsafe { SetLastError(ERROR_SUCCESS) };
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                return (unsafe { GetLastError() } == ERROR_SUCCESS).then_some(backup);
            }
            let id = u16::try_from(format).ok()?;
            if id == CF_OWNERDISPLAY
                || (CF_DSPTEXT..=CF_DSPENHMETAFILE).contains(&id)
                || (CF_PRIVATEFIRST..=CF_GDIOBJLAST).contains(&id)
            {
                return None;
            }
            let source = unsafe { GetClipboardData(format) };
            if source.is_null() {
                return None;
            }
            let duplicate = unsafe { OleDuplicateData(source, id, GMEM_MOVEABLE) };
            if duplicate.is_null() {
                return None;
            }
            backup.0.push((id, duplicate));
        }
    }

    fn restore(&mut self) -> Result<(), SelectionError> {
        if unsafe { EmptyClipboard() } == 0 {
            return Err(SelectionError::ClipboardRestore);
        }
        let mut restored = true;
        for (format, data) in &mut self.0 {
            if unsafe { SetClipboardData(u32::from(*format), *data) }.is_null() {
                restored = false;
            } else {
                *data = null_mut();
            }
        }
        if restored {
            Ok(())
        } else {
            Err(SelectionError::ClipboardRestore)
        }
    }
}

impl Drop for Backup {
    fn drop(&mut self) {
        for &(format, data) in &self.0 {
            if data.is_null() {
                continue;
            }
            unsafe {
                match format {
                    CF_BITMAP | CF_PALETTE => {
                        DeleteObject(data);
                    }
                    CF_ENHMETAFILE => {
                        DeleteEnhMetaFile(data);
                    }
                    CF_METAFILEPICT => {
                        if let Some(picture) = GlobalLock(data).cast::<METAFILEPICT>().as_ref() {
                            DeleteMetaFile(picture.hMF);
                            GlobalUnlock(data);
                        }
                        GlobalFree(data);
                    }
                    _ => {
                        GlobalFree(data);
                    }
                }
            }
        }
    }
}

fn open(window: HWND, deadline: Instant) -> Option<super::Clipboard> {
    let until = deadline.min(Instant::now() + OPEN_WAIT);
    loop {
        if unsafe { OpenClipboard(window) } != 0 {
            return Some(super::Clipboard);
        }
        if Instant::now() >= until {
            return None;
        }
        sleep(POLL);
    }
}

fn clipboard_state() -> (u32, HWND) {
    unsafe { (GetClipboardSequenceNumber(), GetClipboardOwner()) }
}

fn keys_held(shortcut: u32) -> bool {
    [
        VK_MENU, VK_CONTROL, VK_SHIFT, VK_LWIN, VK_RWIN, VK_INSERT, VK_C,
    ]
    .into_iter()
    .map(i32::from)
    .chain([shortcut as i32])
    .any(|key| key != 0 && unsafe { GetAsyncKeyState(key) < 0 })
}

fn send_copy(key: u16) -> bool {
    let input = |key, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let flags = if key == VK_INSERT {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    };
    let events = [
        input(VK_LCONTROL, 0),
        input(key, flags),
        input(key, flags | KEYEVENTF_KEYUP),
        input(VK_LCONTROL, KEYEVENTF_KEYUP),
    ];
    let send = |events: &[INPUT]| unsafe {
        SendInput(
            events.len() as u32,
            events.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    match send(&events) {
        0 => false,
        1 | 3 => {
            send(&events[3..]);
            true
        }
        2 => {
            send(&events[2..]);
            true
        }
        _ => true,
    }
}

pub(super) fn read(
    process: u32,
    terminal: bool,
    deadline: Instant,
    copy: SelectionCopy,
    active: &impl Fn() -> bool,
) -> Result<String, SelectionError> {
    let window = copy.clipboard_window as HWND;
    while active() && keys_held(copy.shortcut_key) {
        sleep(POLL);
    }
    if !active() || window.is_null() {
        return Ok(String::new());
    }
    let (mut backup, before) = {
        let Some(_clipboard) = open(window, deadline) else {
            return Ok(String::new());
        };
        if !active() || keys_held(copy.shortcut_key) {
            return Ok(String::new());
        }
        let Some(backup) = Backup::capture() else {
            return Ok(String::new());
        };
        (backup, clipboard_state())
    };
    let attempts = if terminal {
        &[(VK_INSERT, 220)][..]
    } else {
        &[(VK_C, 150), (VK_INSERT, 220)][..]
    };
    for &(key, wait_ms) in attempts {
        if !active()
            || keys_held(copy.shortcut_key)
            || clipboard_state() != before
            || !send_copy(key)
        {
            break;
        }
        // Finish restoring a copy already sent even if the search gets cancelled.
        let until = Instant::now() + Duration::from_millis(wait_ms);
        loop {
            if clipboard_state() != before {
                let Some(_clipboard) = open(window, Instant::now() + OPEN_WAIT) else {
                    return Err(SelectionError::ClipboardRestore);
                };
                let (sequence, owner) = clipboard_state();
                if (sequence, owner) != before {
                    let mut owner_process = 0;
                    if !owner.is_null() {
                        unsafe { GetWindowThreadProcessId(owner, &mut owner_process) };
                        if owner_process != process {
                            return Ok(String::new());
                        }
                    } else if !terminal || sequence == before.0 || !active() {
                        return Ok(String::new());
                    }
                    let text = read_open_clipboard().unwrap_or_default();
                    if owner.is_null() && (text.is_empty() || !active()) {
                        return Ok(String::new());
                    }
                    backup.restore()?;
                    return Ok(if active() { text } else { String::new() });
                }
            }
            if Instant::now() >= until {
                break;
            }
            sleep(POLL);
        }
    }
    Ok(String::new())
}
