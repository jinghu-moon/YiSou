mod copy;

use std::ffi::OsString;
use std::io;
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::{
    Win32::{
        Foundation::{HWND as ComWindow, VARIANT_BOOL},
        System::Variant::{
            VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_BSTR, VT_I4, VariantClear,
        },
        UI::Accessibility::*,
    },
    core::{BSTR, Interface},
};
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Controls::EM_GETSEL;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const LIMIT: usize = 2048;
const UNICODE_TEXT: u32 = 13;
const COPY_BUDGET: Duration = Duration::from_millis(420);

#[derive(Clone, Copy)]
pub struct SelectionCopy {
    pub clipboard_window: usize,
    pub shortcut_key: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    Other,
    Chromium,
    Terminal,
    Console,
}

impl WindowKind {
    fn terminal(self) -> bool {
        matches!(self, Self::Terminal | Self::Console)
    }
}

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

    pub fn is_windows_terminal(self) -> bool {
        self.kind() == WindowKind::Terminal
    }

    pub fn is_current(self) -> bool {
        self.is_current_for_selection(self.kind())
    }

    fn is_current_for_selection(self, kind: WindowKind) -> bool {
        if kind == WindowKind::Terminal {
            // Alt can move focus within Terminal while its UIA selection remains.
            let window = unsafe { GetForegroundWindow() };
            let mut process = 0;
            return window as usize == self.window
                && unsafe { GetWindowThreadProcessId(window, &mut process) } != 0
                && process == self.process;
        }
        self.is_current_focus()
    }

    fn is_current_focus(self) -> bool {
        let Some(active) = Self::capture() else {
            return false;
        };
        self.window == active.window && self.process == active.process && self.focus == active.focus
    }

    fn kind(self) -> WindowKind {
        match class(self.window).as_str() {
            "Chrome_WidgetWin_1" => WindowKind::Chromium,
            "CASCADIA_HOSTING_WINDOW_CLASS" => WindowKind::Terminal,
            "ConsoleWindowClass" => WindowKind::Console,
            _ => WindowKind::Other,
        }
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
        copy: Option<SelectionCopy>,
        cancelled: &AtomicBool,
    ) -> Result<String, SelectionError> {
        let kind = self.kind();
        let within_budget = || !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline;
        let active = || within_budget() && self.is_current_for_selection(kind);
        let focused = || within_budget() && self.is_current_focus();
        if !active() {
            return Ok(String::new());
        }
        let edit = class(self.focus).eq_ignore_ascii_case("Edit");
        if edit && focused() {
            let style = unsafe { GetWindowLongPtrW(self.focus as HWND, GWL_STYLE) };
            if style & ES_PASSWORD as isize != 0 {
                return Ok(String::new());
            }
            if let Some(text) = native_selection(self.focus, deadline) {
                return Ok(if focused() { text } else { String::new() });
            }
        }
        let uia_deadline = if copy.is_some() {
            deadline.checked_sub(COPY_BUDGET).unwrap_or(deadline)
        } else {
            deadline
        };
        let (text, password) = self.automation_selection(kind, copy.is_some(), &|| {
            Instant::now() < uia_deadline && active()
        });
        if !active() {
            return Ok(String::new());
        }
        if let Some(text) = text {
            return Ok(text);
        }
        if let Some(copy) = copy.filter(|_| !password && kind != WindowKind::Console) {
            return copy::read(
                self.process,
                kind == WindowKind::Terminal,
                deadline,
                copy,
                &focused,
            );
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

impl Foreground {
    fn automation_selection(
        self,
        kind: WindowKind,
        copy_allowed: bool,
        active: &impl Fn() -> bool,
    ) -> (Option<String>, bool) {
        if !active() || unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
            return (None, false);
        }
        let _com = Com;
        let automation: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) } {
                Ok(automation) => automation,
                Err(_) => return (None, false),
            };
        if let Ok(timeouts) = automation.cast::<IUIAutomation2>() {
            let timeout = if copy_allowed { 150 } else { 250 };
            unsafe {
                let _ = timeouts.SetConnectionTimeout(timeout);
                let _ = timeouts.SetTransactionTimeout(timeout);
            }
        }
        let terminal = kind.terminal();
        let walker = unsafe { automation.RawViewWalker() }.ok();
        let mut focused = unsafe { automation.GetFocusedElement() }.ok();
        for _ in 0..8 {
            let Some(element) = focused else { break };
            if !active() || unsafe { element.CurrentProcessId() }.ok() != Some(self.process as i32)
            {
                break;
            }
            match unsafe { element.CurrentIsPassword() } {
                Ok(value) if value.as_bool() => return (None, true),
                Err(_) if !terminal => return (None, true),
                Err(_) => break,
                _ => {}
            }
            if let Some(text) = pattern_text(&element, terminal, active) {
                return (Some(text), false);
            }
            let control = unsafe { element.CurrentControlType() }.ok();
            if control == Some(UIA_EditControlTypeId) || control == Some(UIA_DocumentControlTypeId)
            {
                if !terminal {
                    return (None, false);
                }
                break;
            }
            if control == Some(UIA_WindowControlTypeId) {
                break;
            }
            focused = walker
                .as_ref()
                .and_then(|walker| unsafe { walker.GetParentElement(&element) }.ok());
        }
        if !active() || (copy_allowed && kind == WindowKind::Other) {
            return (None, false);
        }
        (self.window_selection(&automation, kind, active), false)
    }

    fn window_selection(
        self,
        automation: &IUIAutomation,
        kind: WindowKind,
        active: &impl Fn() -> bool,
    ) -> Option<String> {
        unsafe {
            let window = automation
                .ElementFromHandle(ComWindow(self.window as HWND))
                .ok()?;
            if kind == WindowKind::Console {
                return pattern_text(&window, true, active);
            }
            let visible = automation
                .CreatePropertyCondition(
                    UIA_IsOffscreenPropertyId,
                    &uia_value(
                        VT_BOOL,
                        VARIANT_0_0_0 {
                            boolVal: VARIANT_BOOL(0),
                        },
                    ),
                )
                .ok()?;
            if kind == WindowKind::Terminal {
                let mut name = uia_value(
                    VT_BSTR,
                    VARIANT_0_0_0 {
                        bstrVal: ManuallyDrop::new(BSTR::from("TermControl")),
                    },
                );
                let terminal = automation.CreatePropertyCondition(UIA_ClassNamePropertyId, &name);
                let _ = VariantClear(&mut name);
                let condition = automation
                    .CreateAndCondition(&visible, &terminal.ok()?)
                    .ok()?;
                let panes = window.FindAll(TreeScope_Descendants, &condition).ok()?;
                let count = panes.Length().ok()?;
                for index in 0..count {
                    if !active() {
                        break;
                    }
                    let Ok(pane) = panes.GetElement(index) else {
                        continue;
                    };
                    if count == 1 || pane.CurrentHasKeyboardFocus().is_ok_and(|v| v.as_bool()) {
                        return pattern_text(&pane, true, active);
                    }
                }
                return None;
            }
            let document = automation
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &uia_value(
                        VT_I4,
                        VARIANT_0_0_0 {
                            lVal: UIA_DocumentControlTypeId.0,
                        },
                    ),
                )
                .ok()?;
            let has_text = automation
                .CreatePropertyCondition(
                    UIA_IsTextPatternAvailablePropertyId,
                    &uia_value(
                        VT_BOOL,
                        VARIANT_0_0_0 {
                            boolVal: VARIANT_BOOL(-1),
                        },
                    ),
                )
                .ok()?;
            let visible_document = automation.CreateAndCondition(&document, &visible).ok()?;
            let condition = automation
                .CreateAndCondition(&visible_document, &has_text)
                .ok()?;
            let attempts = if kind == WindowKind::Chromium { 4 } else { 1 };
            for attempt in 0..attempts {
                if !active() {
                    break;
                }
                if let Ok(document) = window.FindFirst(TreeScope_Descendants, &condition) {
                    return pattern_text(&document, false, active);
                }
                if attempt + 1 < attempts && active() {
                    std::thread::sleep(Duration::from_millis(35));
                }
            }
            None
        }
    }
}

fn uia_value(tag: windows::Win32::System::Variant::VARENUM, value: VARIANT_0_0_0) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: tag,
                Anonymous: value,
                ..Default::default()
            }),
        },
    }
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
    read_open_clipboard()
}

fn read_open_clipboard() -> io::Result<String> {
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
