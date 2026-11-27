use crate::shortcut::{AltTaps, Shortcut};
use std::cell::RefCell;
use std::io;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    Main,
    Selection,
    Clipboard,
    ImageSearch,
    #[cfg(feature = "ocr")]
    Ocr,
}

#[derive(Clone, Copy, Default)]
pub struct KeyBindings {
    pub main: Option<Shortcut>,
    pub selection: Option<Shortcut>,
    pub clipboard: Option<Shortcut>,
    pub image_search: Option<Shortcut>,
    #[cfg(feature = "ocr")]
    pub ocr: Option<Shortcut>,
}

struct Listener {
    hook: HHOOK,
    window: HWND,
    message: u32,
    taps: AltTaps,
    recording: bool,
}

thread_local! { static LISTENER: RefCell<Option<Listener>> = const { RefCell::new(None) }; }

unsafe extern "system" fn keyboard_proc(code: i32, kind: WPARAM, data: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let key = unsafe { &*(data as *const KBDLLHOOKSTRUCT) };
        if key.flags & LLKHF_INJECTED == 0 {
            let down = matches!(kind as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let other = [VK_SHIFT, VK_CONTROL, VK_LWIN, VK_RWIN, VK_RMENU]
                .into_iter()
                .any(|modifier| unsafe { GetAsyncKeyState(i32::from(modifier)) < 0 });
            let trigger = LISTENER.with_borrow_mut(|listener| {
                let state = listener.as_mut()?;
                (!state.recording && state.taps.key(key.vkCode, down, key.time, other))
                    .then_some((state.window, state.message))
            });
            if let Some((window, message)) = trigger {
                unsafe { PostMessageW(window, message, 0, 0) };
            }
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, kind, data) }
}

pub struct HotkeySet {
    window: HWND,
    double_alt_message: u32,
    combinations: Vec<(i32, Shortcut, Hotkey)>,
    next_id: i32,
    has_alt: bool,
    recording: bool,
}

impl HotkeySet {
    /// # Safety
    /// `window` must be a live window owned by the current thread and outlive the set.
    /// Its procedure must reserve `double_alt_message` for this listener.
    pub unsafe fn new(window: HWND, double_alt_message: u32) -> Self {
        Self {
            window,
            double_alt_message,
            combinations: Vec::new(),
            next_id: 1,
            has_alt: false,
            recording: false,
        }
    }

    pub fn apply(&mut self, bindings: KeyBindings) -> io::Result<()> {
        let use_alt = bindings.main == Some(Shortcut::default());
        let requested: Vec<_> = [
            (bindings.main.filter(|_| !use_alt), Hotkey::Main),
            (bindings.selection, Hotkey::Selection),
            (bindings.clipboard, Hotkey::Clipboard),
            #[cfg(feature = "ocr")]
            (bindings.ocr, Hotkey::Ocr),
            (bindings.image_search, Hotkey::ImageSearch),
        ]
        .into_iter()
        .filter_map(|(key, role)| key.map(|key| (key, role)))
        .collect();
        if requested.iter().enumerate().any(|(index, (key, _))| {
            !key.is_combination()
                || requested[..index]
                    .iter()
                    .any(|(previous, _)| previous == key)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid shortcut bindings",
            ));
        }

        let mut added = Vec::new();
        let mut next = Vec::new();
        for (key, role) in requested {
            if let Some(&(id, _, _)) = self
                .combinations
                .iter()
                .find(|(_, previous, _)| previous == &key)
            {
                next.push((id, key, role));
                continue;
            }
            let id = self.allocate_id();
            if unsafe { RegisterHotKey(self.window, id, key.modifiers | MOD_NOREPEAT, key.key) }
                == 0
            {
                let error = io::Error::last_os_error();
                self.unregister(&added);
                return Err(error);
            }
            added.push(id);
            next.push((id, key, role));
        }
        if use_alt && !self.has_alt {
            let installed = LISTENER.with_borrow_mut(|listener| {
                if listener.is_some() {
                    return Err(io::Error::other("keyboard listener already installed"));
                }
                let hook = unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), null_mut(), 0)
                };
                if hook.is_null() {
                    return Err(io::Error::last_os_error());
                }
                *listener = Some(Listener {
                    hook,
                    window: self.window,
                    message: self.double_alt_message,
                    taps: AltTaps::default(),
                    recording: self.recording,
                });
                Ok(())
            });
            if let Err(error) = installed {
                self.unregister(&added);
                return Err(error);
            }
            self.has_alt = true;
        }
        if !use_alt {
            self.remove_alt();
        }
        for &(id, _, _) in &self.combinations {
            if !next.iter().any(|&(retained, _, _)| retained == id) {
                self.unregister(&[id]);
            }
        }
        self.combinations = next;
        Ok(())
    }

    fn allocate_id(&mut self) -> i32 {
        loop {
            let id = self.next_id;
            self.next_id = if id == 0xbfff { 1 } else { id + 1 };
            if !self
                .combinations
                .iter()
                .any(|&(registered, _, _)| id == registered)
            {
                return id;
            }
        }
    }

    pub fn role(&self, id: WPARAM) -> Option<Hotkey> {
        self.combinations
            .iter()
            .find(|&&(key, _, _)| key as usize == id)
            .map(|(_, _, role)| *role)
    }

    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
        if self.has_alt {
            LISTENER.with_borrow_mut(|listener| {
                if let Some(state) = listener {
                    state.recording = recording;
                    state.taps = AltTaps::default();
                }
            });
        }
    }

    fn unregister(&self, ids: &[i32]) {
        for &id in ids {
            unsafe { UnregisterHotKey(self.window, id) };
        }
    }

    fn remove_alt(&mut self) {
        if self.has_alt {
            if let Some(state) = LISTENER.with_borrow_mut(Option::take) {
                unsafe { UnhookWindowsHookEx(state.hook) };
            }
            self.has_alt = false;
        }
    }
}

impl Drop for HotkeySet {
    fn drop(&mut self) {
        for &(id, _, _) in &self.combinations {
            self.unregister(&[id]);
        }
        self.remove_alt();
    }
}
