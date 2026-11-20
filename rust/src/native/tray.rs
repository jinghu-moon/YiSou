use super::{State, preferences};
use std::ptr::{null, null_mut};
use windows_sys::{
    Win32::{
        Foundation::POINT,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    w,
};

pub(super) const TRAY: u32 = WM_APP + 2;
const NOTIFY_ID: u32 = 1;

impl State {
    pub(super) fn add_tray(&self) {
        let mut tray = self.tray_data();
        tray.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        tray.uCallbackMessage = TRAY;
        tray.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        for (to, from) in tray.szTip.iter_mut().zip("易搜".encode_utf16()) {
            *to = from;
        }
        self.tray
            .set(unsafe { Shell_NotifyIconW(NIM_ADD, &tray) } != 0);
    }

    fn tray_data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window.get(),
            uID: NOTIFY_ID,
            ..Default::default()
        }
    }

    pub(super) fn tray_menu(&self) {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            AppendMenuW(menu, MF_STRING, 1, w!("打开搜索框"));
            #[cfg(feature = "ocr")]
            if self
                .app
                .borrow()
                .settings()
                .preferences
                .active_ocr_model_dir()
                .is_some()
            {
                AppendMenuW(menu, MF_STRING, 4, w!("框选识别"));
            }
            AppendMenuW(menu, MF_STRING, 2, w!("设置"));
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, 3, w!("退出"));
            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor);
            let hwnd = self.window.get();
            SetForegroundWindow(hwnd);
            let choice = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                cursor.x,
                cursor.y,
                0,
                hwnd,
                null(),
            );
            DestroyMenu(menu);
            PostMessageW(hwnd, WM_NULL, 0, 0);
            match choice {
                1 => self.show(),
                2 => preferences::show(self, 0),
                #[cfg(feature = "ocr")]
                4 => self.start_ocr(),
                3 => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
        }
    }

    pub(super) fn remove_tray(&self) {
        if self.tray.replace(false) {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &self.tray_data());
            }
        }
    }
}
