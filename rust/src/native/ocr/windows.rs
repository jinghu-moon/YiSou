use std::{
    io,
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::DwmFlush, Gdi::*},
        UI::{Controls::WM_MOUSELEAVE, Input::KeyboardAndMouse::SetFocus, WindowsAndMessaging::*},
    },
    w,
};

use super::super::instance;
use super::perf;
use super::{Selection, capture::Snapshot};
use super::{chrome::Chrome, frame::Frame};

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.OcrSurface");
#[derive(Clone, Copy)]
pub(super) enum Surface {
    Preview,
    Frame,
    Badge,
    Toolbar,
    Tooltip,
}

impl Surface {
    fn index(self) -> usize {
        match self {
            Self::Preview => 0,
            Self::Frame => 1,
            Self::Badge => 2,
            Self::Toolbar => 3,
            Self::Tooltip => 4,
        }
    }

    pub(super) fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Preview),
            1 => Some(Self::Frame),
            2 => Some(Self::Badge),
            3 => Some(Self::Toolbar),
            4 => Some(Self::Tooltip),
            _ => None,
        }
    }
}

pub(super) struct OverlayWindows {
    bounds: RECT,
    owner: HWND,
    surfaces: [HWND; 5],
    positions: [Option<RECT>; 5],
    frame: Option<Frame>,
    capture_excluded: bool,
}

impl OverlayWindows {
    pub fn new(owner: HWND, bounds: RECT) -> io::Result<Self> {
        let mut windows = Self {
            bounds,
            owner,
            surfaces: [null_mut(); 5],
            positions: [None; 5],
            frame: None,
            capture_excluded: true,
        };
        unsafe {
            windows.capture_excluded = SetWindowDisplayAffinity(owner, WDA_EXCLUDEFROMCAPTURE) != 0;
            for (index, slot) in windows.surfaces.iter_mut().enumerate() {
                let composition = if index == Surface::Frame.index() {
                    WS_EX_NOREDIRECTIONBITMAP
                } else {
                    0
                };
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | composition,
                    CLASS,
                    null(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    owner,
                    null_mut(),
                    instance(),
                    null(),
                );
                if hwnd.is_null() {
                    return Err(io::Error::last_os_error());
                }
                *slot = hwnd;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (index + 1) as isize);
                #[cfg(feature = "ocr-ui-test")]
                if index == Surface::Frame.index() {
                    // Screen pixel tests need to observe the frame in the desktop DC.
                    windows.capture_excluded = false;
                    continue;
                }
                windows.capture_excluded &=
                    SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) != 0;
            }
            let frame = windows.surfaces[Surface::Frame.index()];
            let preview = windows.surfaces[Surface::Preview.index()];
            if SetWindowPos(
                preview,
                frame,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            windows.frame = Some(Frame::new(frame).map_err(io::Error::other)?);
        }
        Ok(windows)
    }

    pub fn capture(&self, area: RECT, margin: i32) -> io::Result<Snapshot> {
        if self.capture_excluded {
            let wait = perf::start();
            unsafe { DwmFlush() };
            perf::record("capture_dwm_excluded", wait);
            return Snapshot::take(self.bounds, area, margin);
        }
        unsafe {
            ShowOwnedPopups(self.owner, 0);
            SetWindowPos(
                self.owner,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOZORDER | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _span = perf::span("capture_dwm_hidden");
            DwmFlush();
        }
        let result = Snapshot::take(self.bounds, area, margin);
        unsafe {
            ShowWindow(self.owner, SW_SHOWNA);
            ShowOwnedPopups(self.owner, 1);
            SetForegroundWindow(self.owner);
            SetFocus(self.owner);
        }
        result
    }

    pub fn update(
        &mut self,
        area: Option<RECT>,
        mut positions: [Option<RECT>; 5],
        chrome: &Chrome,
        show_grips: bool,
    ) {
        if let Some(frame) = &mut self.frame
            && frame.update(self.bounds, area, chrome, show_grips).is_err()
        {
            let _span = perf::span("compose_frame_failed");
        }
        positions[Surface::Frame.index()] = Some(RECT {
            right: self.bounds.right - self.bounds.left,
            bottom: self.bounds.bottom - self.bounds.top,
            ..RECT::default()
        });
        if self
            .positions
            .into_iter()
            .zip(positions)
            .all(|(old, new)| same_position(old, new))
        {
            return;
        }
        unsafe {
            let _span = perf::span("move_surfaces");
            let mut batch = BeginDeferWindowPos(self.surfaces.len() as i32);
            for ((hwnd, previous), next) in
                self.surfaces.into_iter().zip(self.positions).zip(positions)
            {
                if same_position(previous, next) {
                    continue;
                }
                let (area, flags) = next.map_or(
                    (RECT::default(), SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE),
                    |area| (area, SWP_SHOWWINDOW),
                );
                if !batch.is_null() {
                    batch = DeferWindowPos(
                        batch,
                        hwnd,
                        null_mut(),
                        self.bounds.left + area.left,
                        self.bounds.top + area.top,
                        area.right - area.left,
                        area.bottom - area.top,
                        flags | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_DEFERERASE,
                    );
                }
            }
            if batch.is_null() || EndDeferWindowPos(batch) == 0 {
                for (hwnd, next) in self.surfaces.into_iter().zip(positions) {
                    if let Some(area) = next {
                        SetWindowPos(
                            hwnd,
                            null_mut(),
                            self.bounds.left + area.left,
                            self.bounds.top + area.top,
                            area.right - area.left,
                            area.bottom - area.top,
                            SWP_SHOWWINDOW | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                        );
                    } else {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
        }
        self.positions = positions;
    }

    pub fn invalidate(&self, surface: Surface, dirty: Option<RECT>) {
        let index = surface.index();
        if let Some(area) = self.positions[index] {
            let rect = dirty.map(|mut rect| {
                unsafe { OffsetRect(&mut rect, -area.left, -area.top) };
                rect
            });
            super::capture::invalidate(self.surfaces[index], rect.as_ref());
        }
    }

    pub fn position(&self, surface: Surface) -> Option<RECT> {
        self.positions[surface.index()]
    }
}

impl Drop for OverlayWindows {
    fn drop(&mut self) {
        drop(self.frame.take());
        for hwnd in self.surfaces.into_iter().filter(|hwnd| !hwnd.is_null()) {
            unsafe { DestroyWindow(hwnd) };
        }
    }
}

fn same_position(first: Option<RECT>, second: Option<RECT>) -> bool {
    match (first, second) {
        (Some(a), Some(b)) => super::cache::same_rect(a, b),
        (None, None) => true,
        _ => false,
    }
}

pub(super) unsafe extern "system" fn surface_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let id = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            let Some(surface) = Surface::from_index(id.wrapping_sub(1) as usize) else {
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            };
            if let Surface::Frame = surface {
                unsafe { ValidateRect(hwnd, null()) };
                return 0;
            }
            let Some(state) = super::session() else {
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            };
            let Ok(slot) = state.ocr.selection.try_borrow() else {
                return 0;
            };
            if let Some(selection) = slot.as_ref() {
                Selection::paint_surface(selection, hwnd, surface);
            } else {
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_SETCURSOR | WM_MOUSEMOVE | WM_MOUSELEAVE | WM_LBUTTONDOWN | WM_LBUTTONUP
        | WM_RBUTTONUP => unsafe {
            let owner = GetWindow(hwnd, GW_OWNER);
            SendMessageW(owner, msg, wp, lp)
        },
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
