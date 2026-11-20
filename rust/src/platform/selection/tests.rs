use super::{Backup, open};
use crate::platform::selection::{Foreground, SelectionCopy, clipboard_text};
use std::{
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    thread::{self, sleep},
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*,
            LibraryLoader::{GetModuleHandleW, LoadLibraryW},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
        UI::{Controls::EM_SETSEL, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    w,
};

const ORIGINAL: &str = "clipboard before selection";
const METADATA: &[u8] = b"additional clipboard format";
const COPIED: &str = "selection from clipboard";
static COPY_KEYS: AtomicU8 = AtomicU8::new(0);

fn set_format(format: u32, data: &[u8]) {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, data.len());
        assert!(!memory.is_null());
        let buffer = GlobalLock(memory).cast::<u8>();
        assert!(!buffer.is_null());
        buffer.copy_from_nonoverlapping(data.as_ptr(), data.len());
        GlobalUnlock(memory);
        if SetClipboardData(format, memory).is_null() {
            GlobalFree(memory);
            panic!("SetClipboardData failed");
        }
    }
}

fn set_text(text: &str) {
    let bytes: Vec<_> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    set_format(13, &bytes);
}

unsafe extern "system" fn copy_source(hwnd: HWND, msg: u32, word: WPARAM, long: LPARAM) -> LRESULT {
    if msg == WM_KEYDOWN && unsafe { GetKeyState(i32::from(VK_CONTROL)) } < 0 {
        if word == usize::from(VK_C) {
            COPY_KEYS.fetch_or(1, Ordering::Relaxed);
            return 0;
        }
        if word == usize::from(VK_INSERT) {
            COPY_KEYS.fetch_or(2, Ordering::Relaxed);
            if let Some(_clipboard) = open(hwnd, Instant::now() + Duration::from_millis(100))
                && unsafe { EmptyClipboard() } != 0
            {
                set_text(COPIED);
            }
            return 0;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, word, long) }
}

struct Window(HWND);

impl Drop for Window {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

struct RestoreClipboard<'a>(&'a Window, Backup);

impl Drop for RestoreClipboard<'_> {
    fn drop(&mut self) {
        let _clipboard = open(self.0.0, Instant::now() + Duration::from_millis(500))
            .expect("reopen clipboard after selection test");
        self.1
            .restore()
            .expect("restore original clipboard after selection test");
    }
}

fn pump() {
    unsafe {
        let mut event = MSG::default();
        while PeekMessageW(&mut event, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&event);
            DispatchMessageW(&event);
        }
    }
}

fn read_from(hwnd: HWND, copy: Option<SelectionCopy>) -> String {
    unsafe {
        SetFocus(hwnd);
        assert_eq!(GetFocus(), hwnd);
    }
    pump();
    let foreground = Foreground::capture().unwrap();
    assert_eq!(foreground.focus, hwnd as usize);
    let (send, receive) = mpsc::channel();
    let deadline = Instant::now() + Duration::from_millis(850);
    let reader = thread::spawn(move || {
        let text = foreground.read_selection(deadline, copy, &AtomicBool::new(false));
        let _ = send.send(text);
    });
    let result = loop {
        pump();
        if let Ok(result) = receive.try_recv() {
            break result;
        }
        assert!(
            Instant::now() < deadline + Duration::from_secs(2),
            "selection worker timed out"
        );
        sleep(Duration::from_millis(2));
    };
    reader.join().unwrap();
    result.unwrap()
}

#[test]
#[ignore = "requires an interactive Windows desktop and temporarily uses the clipboard"]
fn reads_native_edit_uia_and_copy_in_order() {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = w!("YiSou.SelectionCopyTest");
        assert_ne!(
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(copy_source),
                hInstance: instance,
                lpszClassName: class,
                ..Default::default()
            }),
            0
        );
        let window = Window(CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("STATIC"),
            w!("YiSou selection regression"),
            WS_POPUP | WS_CAPTION | WS_VISIBLE,
            100,
            100,
            380,
            180,
            null_mut(),
            null_mut(),
            instance,
            null(),
        ));
        assert!(!window.0.is_null());
        SetForegroundWindow(window.0);
        pump();
        assert_eq!(GetForegroundWindow(), window.0);

        let rich_edit = LoadLibraryW(w!("msftedit.dll"));
        assert!(!rich_edit.is_null());
        let child = |kind, text, style, y| {
            let child = Window(CreateWindowExW(
                0,
                kind,
                text,
                WS_CHILD | WS_VISIBLE | style,
                10,
                y,
                340,
                28,
                window.0,
                null_mut(),
                instance,
                null(),
            ));
            assert!(!child.0.is_null());
            child
        };
        let edit = child(w!("Edit"), w!("a 中文 z"), 0, 10);
        let rich = child(w!("RICHEDIT50W"), w!("a UIA z"), 0, 45);
        let source = child(class, null(), 0, 80);
        let password = child(w!("Edit"), w!("test password"), ES_PASSWORD as u32, 115);
        SendMessageW(edit.0, EM_SETSEL, 2, 4);
        SendMessageW(rich.0, EM_SETSEL, 2, 5);
        SendMessageW(password.0, EM_SETSEL, 0, -1);
        assert_eq!(read_from(edit.0, None), "\u{4e2d}\u{6587}");
        assert_eq!(read_from(rich.0, None), "UIA");
        assert_eq!(read_from(source.0, None), "");
        assert_eq!(COPY_KEYS.load(Ordering::Relaxed), 0);

        let _restore = {
            let _clipboard = open(window.0, Instant::now() + Duration::from_millis(500)).unwrap();
            RestoreClipboard(
                &window,
                Backup::capture().expect("snapshot current clipboard"),
            )
        };
        let format = RegisterClipboardFormatW(w!("YiSou.SelectionTestMetadata"));
        assert_ne!(format, 0);
        {
            let _clipboard = open(window.0, Instant::now() + Duration::from_millis(500)).unwrap();
            assert_ne!(EmptyClipboard(), 0);
            set_text(ORIGINAL);
            set_format(format, METADATA);
        }
        let copy = Some(SelectionCopy {
            clipboard_window: window.0 as usize,
            shortcut_key: u32::from(VK_F21),
        });
        assert_eq!(read_from(password.0, copy), "");
        assert_eq!(COPY_KEYS.load(Ordering::Relaxed), 0);
        assert_eq!(read_from(source.0, copy), COPIED);
        assert_eq!(COPY_KEYS.load(Ordering::Relaxed), 3);
        assert_eq!(clipboard_text().unwrap(), ORIGINAL);
        {
            let _clipboard = open(window.0, Instant::now() + Duration::from_millis(500)).unwrap();
            let metadata = GetClipboardData(format);
            assert!(!metadata.is_null());
            let data = GlobalLock(metadata).cast::<u8>();
            assert!(!data.is_null());
            assert_eq!(std::slice::from_raw_parts(data, METADATA.len()), METADATA);
            GlobalUnlock(metadata);
        }
        drop(rich);
        FreeLibrary(rich_edit);
    }
}
