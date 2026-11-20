use super::{State, instance, session};
use std::ptr::{null, null_mut};
use windows_sys::{
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
    w,
};

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.ScreenShade");
const OPACITY: u8 = 96;

impl State {
    pub(super) fn show_screen_shade(&self) {
        unsafe {
            let mut shade = self.screen_shade.get();
            if shade.is_null() {
                shade = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                    CLASS,
                    null(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    instance(),
                    null(),
                );
                if shade.is_null() {
                    return;
                }
                if SetLayeredWindowAttributes(shade, 0, OPACITY, LWA_ALPHA) == 0 {
                    DestroyWindow(shade);
                    return;
                }
                self.screen_shade.set(shade);
            }

            // Inserting immediately after Search keeps the screen shade below
            // both the search card and any popup opened above it.
            SetWindowPos(
                shade,
                self.window.get(),
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    pub(super) fn hide_screen_shade(&self) {
        let shade = self.screen_shade.get();
        if !shade.is_null() {
            unsafe { ShowWindow(shade, SW_HIDE) };
        }
    }

    pub(super) fn release_screen_shade(&self) {
        let shade = self.screen_shade.replace(null_mut());
        if !shade.is_null() {
            unsafe { DestroyWindow(shade) };
        }
    }
}

pub(super) unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            FillRect(dc, &paint.rcPaint, GetStockObject(BLACK_BRUSH));
            EndPaint(hwnd, &paint);
            0
        },
        WM_ERASEBKGND => 1,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_LBUTTONUP => {
            if let Some(state) = session() {
                state.hide(true);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
