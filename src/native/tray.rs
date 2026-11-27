use super::{State, app_icon, preferences};
use std::ptr::null;
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
        tray.hIcon = app_icon(true);
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

    pub(super) fn notify_image_search_failure(&self, detail: &str, retry: bool) {
        let english =
            preferences::english_selected(self.app.borrow().settings().preferences.language);
        let title = match (english, retry) {
            (true, true) => "Image search failed · Click to retry",
            (false, true) => "搜图失败 · 点击重试",
            (true, false) => "Image search failed",
            (false, false) => "截图搜图失败",
        };
        let mut tray = self.tray_data();
        tray.uFlags = NIF_INFO;
        for (to, from) in tray
            .szInfoTitle
            .iter_mut()
            .take(63)
            .zip(title.encode_utf16())
        {
            *to = from;
        }
        for (to, from) in tray.szInfo.iter_mut().take(255).zip(detail.encode_utf16()) {
            *to = from;
        }
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &tray) };
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
            let english =
                preferences::english_selected(self.app.borrow().settings().preferences.language);
            AppendMenuW(
                menu,
                MF_STRING,
                5,
                if english {
                    w!("Screenshot image search")
                } else {
                    w!("截图搜图")
                },
            );
            if self.can_retry_image_search() {
                AppendMenuW(
                    menu,
                    MF_STRING,
                    6,
                    if english {
                        w!("Retry failed image search")
                    } else {
                        w!("重试失败的搜图")
                    },
                );
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
                5 => {
                    PostMessageW(hwnd, super::screenshot::START_IMAGE_SEARCH, 0, 0);
                }
                6 => self.retry_image_search(),
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
