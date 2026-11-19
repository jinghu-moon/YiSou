use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Ole::{
    OleFlushClipboard, OleGetClipboard, OleInitialize, OleSetClipboard, OleUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, UIA_TextPatternId,
};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber, OpenClipboard,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Controls::EM_GETSEL;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const LIMIT: usize = 2048;
const UNICODE_TEXT: u32 = 13;

#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("could not restore clipboard data")]
    ClipboardRestore,
}

#[derive(Clone, Copy)]
pub struct Foreground {
    window: usize,
    focus: usize,
    process: u32,
}

impl Foreground {
    pub fn capture() -> Option<Self> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return None;
        }
        let mut process = 0;
        let thread = unsafe { GetWindowThreadProcessId(window, &mut process) };
        if thread == 0 {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(thread, &mut info) };
        Some(Self {
            window: window as usize,
            focus: info.hwndFocus as usize,
            process,
        })
    }

    pub fn id(self) -> usize {
        self.window
    }

    pub fn is_current(self) -> bool {
        let Some(active) = Self::capture() else {
            return false;
        };
        self.window == active.window && self.process == active.process && self.focus == active.focus
    }

    pub fn executable(self) -> Option<PathBuf> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, self.process) };
        if process.is_null() {
            return None;
        }
        let mut text = vec![0u16; 32768];
        let mut size = text.len() as u32;
        let read = unsafe { QueryFullProcessImageNameW(process, 0, text.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(process) };
        (read != 0).then(|| OsString::from_wide(&text[..size as usize]).into())
    }

    pub fn read_selection(
        self,
        deadline: Instant,
        copy_allowed: bool,
        cancelled: &AtomicBool,
    ) -> Result<String, SelectionError> {
        let active =
            || !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline && self.is_current();
        if !active() {
            return Ok(String::new());
        }
        let edit = class(self.focus).eq_ignore_ascii_case("Edit");
        if edit {
            let style = unsafe { GetWindowLongPtrW(self.focus as HWND, GWL_STYLE) };
            if style & ES_PASSWORD as isize != 0 {
                return Ok(String::new());
            }
            if let Some(text) = native_selection(self.focus, deadline) {
                return Ok(if active() { text } else { String::new() });
            }
        }
        let terminal = class(self.window) == "CASCADIA_HOSTING_WINDOW_CLASS";
        let (text, copy_safe) = automation_selection(self.process, terminal, &active);
        if !active() {
            return Ok(String::new());
        }
        if let Some(text) = text {
            return Ok(text);
        }
        if copy_allowed && (copy_safe || edit) && class(self.window) != "ConsoleWindowClass" {
            return copy_selection(self.process, terminal, deadline, &active);
        }
        Ok(String::new())
    }
}

fn class(window: usize) -> String {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(window as HWND, name.as_mut_ptr(), name.len() as i32) };
    String::from_utf16_lossy(&name[..len as usize])
}

fn native_selection(window: usize, deadline: Instant) -> Option<String> {
    let mut start = 0u32;
    let mut end = 0u32;
    let send = |message, word, long, result| {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(80) as u32;
        timeout != 0
            && unsafe {
                SendMessageTimeoutW(
                    window as HWND,
                    message,
                    word,
                    long,
                    SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
                    timeout,
                    result,
                )
            } != 0
    };
    if !send(
        EM_GETSEL,
        (&mut start as *mut u32) as usize,
        (&mut end as *mut u32) as isize,
        null_mut(),
    ) || start >= end
        || start > 1024 * 1024
    {
        return None;
    }
    let start = start as usize;
    let end = (end as usize).min(start + LIMIT);
    let mut text = vec![0u16; end + 1];
    let mut copied = 0;
    if !send(
        WM_GETTEXT,
        text.len(),
        text.as_mut_ptr() as isize,
        &mut copied,
    ) || copied <= start
    {
        return None;
    }
    Some(from_wide(&text[start..end.min(copied)]))
}

fn from_wide(mut text: &[u16]) -> String {
    if text.last().is_some_and(|ch| (0xd800..=0xdbff).contains(ch)) {
        text = &text[..text.len() - 1];
    }
    String::from_utf16_lossy(text)
}

struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn automation_selection(
    process: u32,
    terminal: bool,
    active: &impl Fn() -> bool,
) -> (Option<String>, bool) {
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return (None, false);
    }
    let _com = Com;
    let automation: IUIAutomation =
        match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
            Ok(automation) => automation,
            Err(_) => return (None, false),
        };
    let Ok(mut element) = (unsafe { automation.GetFocusedElement() }) else {
        return (None, false);
    };
    if unsafe { element.CurrentProcessId() }.ok() != Some(process as i32) {
        return (None, false);
    }
    if unsafe { element.CurrentIsPassword() }.map_or(true, |value| value.as_bool()) {
        return (None, false);
    }
    let Ok(walker) = (unsafe { automation.ControlViewWalker() }) else {
        return (None, false);
    };
    for _ in 0..8 {
        if !active() || unsafe { element.CurrentProcessId() }.ok() != Some(process as i32) {
            break;
        }
        if let Some(text) = pattern_text(&element, terminal, active) {
            return (Some(text), true);
        }
        let Ok(parent) = (unsafe { walker.GetParentElement(&element) }) else {
            break;
        };
        element = parent;
    }
    (None, true)
}

fn pattern_text(
    element: &IUIAutomationElement,
    terminal: bool,
    active: &impl Fn() -> bool,
) -> Option<String> {
    let pattern: IUIAutomationTextPattern =
        unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
    let ranges = unsafe { pattern.GetSelection() }.ok()?;
    let mut result = Vec::new();
    for index in 0..unsafe { ranges.Length() }.ok()?.min(128) {
        let remaining = LIMIT.saturating_sub(result.len())
            - usize::from(!result.is_empty() && result.len() < LIMIT);
        if !active() || remaining == 0 {
            break;
        }
        let Ok(range) = (unsafe { ranges.GetElement(index) }) else {
            continue;
        };
        if !terminal
            && unsafe {
                range.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &range,
                    TextPatternRangeEndpoint_End,
                )
            }
            .ok()
                == Some(0)
        {
            continue;
        }
        let Ok(value) = (unsafe { range.GetText(remaining as i32) }) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if !result.is_empty() {
            result.push(b' ' as u16);
        }
        result.extend(value.iter().take(remaining));
    }
    (!result.is_empty()).then(|| from_wide(&result))
}

struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

pub fn clipboard_text() -> io::Result<String> {
    if unsafe { OpenClipboard(null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _clipboard = Clipboard;
    let data = unsafe { GetClipboardData(UNICODE_TEXT) };
    if data.is_null() {
        return Ok(String::new());
    }
    let locked = unsafe { GlobalLock(data) }.cast::<u16>();
    if locked.is_null() {
        return Err(io::Error::last_os_error());
    }
    let length = (unsafe { GlobalSize(data) } / 2).min(LIMIT);
    let text = unsafe { std::slice::from_raw_parts(locked, length) };
    let text = from_wide(&text[..text.iter().position(|&unit| unit == 0).unwrap_or(length)]);
    unsafe { GlobalUnlock(data) };
    Ok(text)
}

struct Ole;
impl Drop for Ole {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

fn copy_selection(
    process: u32,
    terminal: bool,
    deadline: Instant,
    active: &impl Fn() -> bool,
) -> Result<String, SelectionError> {
    if unsafe { OleInitialize(None) }.is_err() {
        return Ok(String::new());
    }
    let _ole = Ole;
    let before = unsafe { GetClipboardSequenceNumber() };
    let Ok(saved) = (unsafe { OleGetClipboard() }) else {
        return Ok(String::new());
    };
    let modifier_held = || {
        [VK_MENU, VK_CONTROL, VK_SHIFT, VK_LWIN, VK_RWIN]
            .into_iter()
            .any(|key| unsafe { GetAsyncKeyState(i32::from(key)) < 0 })
    };
    while active() && modifier_held() {
        std::thread::sleep(Duration::from_millis(5));
    }
    if !active() || unsafe { GetClipboardSequenceNumber() } != before {
        return Ok(String::new());
    }
    let key = if terminal { VK_INSERT } else { VK_C };
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
    let events = [
        input(VK_CONTROL, 0),
        input(key, 0),
        input(key, KEYEVENTF_KEYUP),
        input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            events.len() as u32,
            events.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent == 0 {
        return Ok(String::new());
    }
    if sent < events.len() as u32 {
        let release = [
            input(key, KEYEVENTF_KEYUP),
            input(VK_CONTROL, KEYEVENTF_KEYUP),
        ];
        unsafe {
            SendInput(
                release.len() as u32,
                release.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
    }
    let until = deadline.min(Instant::now() + Duration::from_millis(250));
    loop {
        let sequence = unsafe { GetClipboardSequenceNumber() };
        if sequence != before {
            let mut owner = 0;
            unsafe { GetWindowThreadProcessId(GetClipboardOwner(), &mut owner) };
            if owner != process {
                return Ok(String::new());
            }
            if let Ok(text) = clipboard_text() {
                if unsafe { GetClipboardSequenceNumber() } != sequence {
                    return Ok(String::new());
                }
                unsafe { OleSetClipboard(&saved).and_then(|()| OleFlushClipboard()) }
                    .map_err(|_| SelectionError::ClipboardRestore)?;
                return Ok(if active() { text } else { String::new() });
            }
        }
        if Instant::now() >= until {
            return Ok(String::new());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
