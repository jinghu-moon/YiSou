#[cfg(feature = "ocr")]
mod cache;
mod capture;
mod chrome;
mod frame;
mod image_search;
#[cfg(feature = "ocr")]
mod recognition;
mod render;
mod reverse_image;
#[cfg(feature = "ocr")]
mod text_overlay;
mod windows;

use self::capture::{Snapshot, desktop_bounds};
use self::chrome::{Action, Chrome, Handle};
use self::render::Canvas;
use self::windows::{OverlayWindows, Surface};
#[cfg(feature = "ocr")]
use self::{cache::Cache, recognition::Worker, text_overlay::TextOverlay};
use super::diagnostics::ocr as perf;
use super::{State, instance, message, session};
use std::{
    cell::{Cell, RefCell},
    io,
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::DwmFlush, Gdi::*},
        UI::{Controls::WM_MOUSELEAVE, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    w,
};

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.OcrSelection");
#[cfg(feature = "ocr")]
pub(super) const READY: u32 = WM_APP + 5;
#[cfg(feature = "ocr")]
pub(super) const START: u32 = WM_APP + 6;
pub(super) const START_IMAGE_SEARCH: u32 = WM_APP + 7;
const IMAGE_SEARCH_MENU: u32 = WM_APP + 8;
pub(super) const IMAGE_SEARCH_READY: u32 = WM_APP + 9;
#[cfg(feature = "ocr")]
const STATUS_TIMER: usize = 1;
const TOOLTIP_TIMER: usize = 2;
const READY_LABEL: &str = "识别完成";
const LOADING_LABEL: &str = "正在识别";
const RESIZE_LABEL: &str = "调整选区";
#[cfg(feature = "ocr")]
const EMPTY_LABEL: &str = "未识别到文字 · 重新框选";
#[cfg(feature = "ocr")]
const FAILED_LABEL: &str = "识别失败 · 重新框选";

pub(super) fn register() -> io::Result<()> {
    super::register(CLASS, Some(proc))?;
    super::register(windows::CLASS, Some(windows::surface_proc))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[cfg(feature = "ocr")]
    Ocr,
    ImageSearch,
}

#[derive(Default)]
pub(super) struct ScreenshotTasks {
    overlay: Cell<HWND>,
    selection: RefCell<Option<Selection>>,
    image_upload: RefCell<Option<image_search::Upload>>,
    image_retry: RefCell<Option<image_search::Retry>>,
    #[cfg(feature = "ocr")]
    next_id: Cell<usize>,
    #[cfg(feature = "ocr")]
    worker: RefCell<Option<Worker>>,
}

struct Selection {
    mode: Mode,
    english: bool,
    canvas: RefCell<Option<Canvas>>,
    bounds: RECT,
    windows: OverlayWindows,
    snapshot: Option<Snapshot>,
    previous: HWND,
    #[cfg(feature = "ocr")]
    engine: Option<String>,
    start: Option<POINT>,
    area: Option<RECT>,
    #[cfg(feature = "ocr")]
    pending: Option<Pending>,
    status: &'static str,
    uploading: bool,
    spinner: u8,
    #[cfg(feature = "ocr")]
    cache: Cache,
    chrome: Chrome,
    resize: Option<(Handle, POINT, RECT)>,
    hover: Option<Action>,
    pressed: Option<Action>,
    tracking_mouse: HWND,
    tooltip: bool,
    #[cfg(feature = "ocr")]
    text: Option<TextOverlay>,
}

#[cfg(feature = "ocr")]
struct Pending {
    id: usize,
    requested: Option<RECT>,
    matching: Option<RECT>,
}

impl Selection {
    fn dragging(&self) -> bool {
        self.start.is_some() || self.resize.is_some()
    }

    fn idle_label(&self) -> &'static str {
        match self.mode {
            #[cfg(feature = "ocr")]
            Mode::Ocr => "框选要识别的区域",
            Mode::ImageSearch if self.english => "Select an area to search",
            Mode::ImageSearch => "框选要搜索的图片区域",
        }
    }

    fn toolbar_visible(&self) -> bool {
        !self.dragging()
            && !self.uploading
            && (self.mode == Mode::ImageSearch || self.snapshot.is_some())
    }

    fn action_enabled(&self, action: Action) -> bool {
        action == Action::Cancel || self.mode == Mode::ImageSearch || self.has_selected_text()
    }

    fn sync_windows(&mut self) {
        let mut surfaces = [None; 5];
        if let Some(area) = self.area {
            if self.toolbar_visible() {
                surfaces[0] = self.snapshot.as_ref().map(|_| area);
                let mut bar = self.chrome.toolbar(area);
                bar.bottom += 2;
                surfaces[3] = Some(bar);
                if self.tooltip
                    && let Some(action) = self.hover
                {
                    surfaces[4] = Some(
                        self.chrome
                            .tooltip(area, self.action_enabled(action), action)
                            .0,
                    );
                }
            }
        }
        if !self.dragging() {
            surfaces[2] = Some(self.chrome.badge(self.area, self.status));
        }
        self.windows
            .update(self.area, surfaces, &self.chrome, self.start.is_none());
    }

    fn paint_surface(&self, hwnd: HWND, surface: Surface) {
        let event = match surface {
            Surface::Preview => "paint_preview",
            Surface::Badge => "paint_badge",
            Surface::Toolbar => "paint_toolbar",
            Surface::Tooltip => "paint_tooltip",
            Surface::Frame => return,
        };
        let _span = perf::span(event);
        let mut paint = PAINTSTRUCT::default();
        let dc = unsafe { BeginPaint(hwnd, &mut paint) };
        if let Some(bounds) = self.windows.position(surface)
            && !dc.is_null()
            && paint.rcPaint.left < paint.rcPaint.right
            && paint.rcPaint.top < paint.rcPaint.bottom
        {
            let mut canvas = self.canvas.borrow_mut();
            if canvas.is_none() {
                *canvas = Canvas::new().ok();
            }
            if let Some(renderer) = canvas.as_mut() {
                let result = if let (Surface::Preview, Some(snapshot)) = (surface, &self.snapshot) {
                    renderer.draw_preview(dc, bounds, paint.rcPaint, snapshot, |_canvas| {
                        #[cfg(feature = "ocr")]
                        if let Some(text) = &self.text {
                            text.paint(_canvas, bounds);
                        }
                    })
                } else {
                    renderer.draw(dc, bounds, paint.rcPaint, |canvas| match surface {
                        Surface::Badge => {
                            canvas.clear(0x242933);
                            self.chrome.paint_badge(
                                canvas,
                                self.area,
                                self.status,
                                (self.status == LOADING_LABEL).then_some(self.spinner),
                            );
                        }
                        Surface::Toolbar => {
                            canvas.clear(0x242933);
                            if let Some(area) = self.area {
                                self.chrome.paint_toolbar(
                                    canvas,
                                    area,
                                    self.action_enabled(Action::Search),
                                    self.hover,
                                    self.pressed,
                                );
                            }
                        }
                        Surface::Tooltip => {
                            canvas.clear(0x242933);
                            if let (Some(area), Some(action)) = (self.area, self.hover) {
                                self.chrome.paint_tooltip(
                                    canvas,
                                    area,
                                    self.action_enabled(action),
                                    action,
                                );
                            }
                        }
                        Surface::Preview | Surface::Frame => {}
                    })
                };
                if result.is_err() {
                    *canvas = None;
                }
            }
        }
        unsafe { EndPaint(hwnd, &paint) };
    }

    #[cfg(feature = "ocr")]
    fn show_loading(&mut self, hwnd: HWND) {
        self.status = LOADING_LABEL;
        self.spinner = 0;
        if self.chrome.animations_enabled() {
            unsafe { SetTimer(hwnd, STATUS_TIMER, 80, None) };
        }
        self.sync_windows();
        self.windows.invalidate(Surface::Badge, None);
    }

    fn has_selected_text(&self) -> bool {
        #[cfg(feature = "ocr")]
        {
            self.text
                .as_ref()
                .and_then(TextOverlay::selected_text)
                .is_some()
        }
        #[cfg(not(feature = "ocr"))]
        false
    }

    fn set_toolbar_hover(&mut self, hwnd: HWND, hover: Option<Action>) {
        if self.hover == hover {
            return;
        }
        self.hover = hover;
        self.tooltip = false;
        unsafe {
            KillTimer(hwnd, TOOLTIP_TIMER);
            if hover.is_some() {
                SetTimer(hwnd, TOOLTIP_TIMER, 450, None);
            }
        }
        self.sync_windows();
        self.windows.invalidate(Surface::Toolbar, None);
    }
}

impl State {
    pub(super) fn start_image_search(&self) {
        self.start_screenshot(Mode::ImageSearch);
    }

    fn start_screenshot(&self, mode: Mode) {
        self.end_screenshot(true);
        let _span = perf::span("open_overlay");
        let hwnd = self.window.get();
        let active = unsafe { GetForegroundWindow() };
        let previous = if active == hwnd {
            self.launch_focus.get()
        } else {
            active
        };
        #[cfg(feature = "ocr")]
        let engine = self.temporary.borrow().clone();
        let english =
            super::preferences::english_selected(self.app.borrow().settings().preferences.language);
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            self.hide(false);
            unsafe { DwmFlush() };
        }
        self.cancel_selection();
        let bounds = match desktop_bounds() {
            Ok(bounds) => bounds,
            Err(error) => {
                message(hwnd, &format!("无法取得桌面范围\n{error}"));
                return;
            }
        };
        let chrome = Chrome::new(bounds, point(&bounds), mode, english);
        let overlay = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
                CLASS,
                if mode == Mode::ImageSearch {
                    w!("YiSou · Image Search")
                } else {
                    w!("易搜 · 框选识别")
                },
                WS_POPUP,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                null_mut(),
                null_mut(),
                instance(),
                null(),
            )
        };
        if overlay.is_null() {
            message(
                hwnd,
                &format!("无法显示选区\n{}", io::Error::last_os_error()),
            );
            return;
        }
        let windows = match OverlayWindows::new(overlay, bounds) {
            Ok(windows) => windows,
            Err(error) => {
                unsafe { DestroyWindow(overlay) };
                message(hwnd, &format!("无法初始化截图绘制\n{error}"));
                return;
            }
        };
        let mut selection = Selection {
            mode,
            english,
            canvas: RefCell::new(None),
            bounds,
            windows,
            snapshot: None,
            previous,
            #[cfg(feature = "ocr")]
            engine,
            start: None,
            area: None,
            #[cfg(feature = "ocr")]
            pending: None,
            status: "",
            uploading: false,
            spinner: 0,
            #[cfg(feature = "ocr")]
            cache: Cache::default(),
            chrome,
            resize: None,
            hover: None,
            pressed: None,
            tracking_mouse: null_mut(),
            tooltip: false,
            #[cfg(feature = "ocr")]
            text: None,
        };
        selection.status = selection.idle_label();
        selection.sync_windows();
        *self.screenshot.selection.borrow_mut() = Some(selection);
        self.screenshot.overlay.set(overlay);
        unsafe {
            ShowWindow(overlay, SW_SHOW);
            SetForegroundWindow(overlay);
            SetFocus(overlay);
        }
        self.diagnostics.sample("ocr_overlay_shown");
    }

    pub(super) fn end_screenshot(&self, restore: bool) {
        let overlay = self.screenshot.overlay.replace(null_mut());
        let selection = self.screenshot.selection.borrow_mut().take();
        if let Some(upload) = self.screenshot.image_upload.borrow_mut().as_mut() {
            upload.dismiss(overlay);
        }
        let previous = selection.as_ref().map(|selection| selection.previous);
        drop(selection);
        if !overlay.is_null() {
            unsafe { DestroyWindow(overlay) };
            perf::flush();
        }
        if restore && let Some(previous) = previous {
            unsafe {
                if IsWindow(previous) != 0 {
                    SetForegroundWindow(previous);
                }
            }
        }
    }
}

fn point(bounds: &RECT) -> POINT {
    let mut position = POINT::default();
    unsafe { GetCursorPos(&mut position) };
    POINT {
        x: (position.x - bounds.left).clamp(0, bounds.right - bounds.left),
        y: (position.y - bounds.top).clamp(0, bounds.bottom - bounds.top),
    }
}

fn area(a: POINT, b: POINT) -> Option<RECT> {
    (a.x != b.x && a.y != b.y).then_some(RECT {
        left: a.x.min(b.x),
        right: a.x.max(b.x),
        top: a.y.min(b.y),
        bottom: a.y.max(b.y),
    })
}

fn drag(selection: &mut Selection) {
    let _span = perf::span("drag");
    let bounds = selection.bounds;
    let cursor = point(&bounds);
    let next = if let Some((handle, anchor, original)) = selection.resize {
        Some(handle.resize(
            original,
            SIZE {
                cx: cursor.x - anchor.x,
                cy: cursor.y - anchor.y,
            },
            bounds,
        ))
    } else if let Some(start) = selection.start {
        area(start, cursor)
    } else {
        return;
    };
    selection.area = next;
    selection.sync_windows();
}

fn point_cursor(selection: &Selection) -> *const u16 {
    let point = point(&selection.bounds);
    if let Some((handle, _, _)) = selection.resize {
        return handle.cursor();
    }
    if let Some(area) = selection.area {
        if selection.toolbar_visible() && chrome::contains(selection.chrome.toolbar(area), point) {
            return match selection.chrome.action(area, point) {
                Some(action) if selection.action_enabled(action) => IDC_HAND,
                _ => IDC_ARROW,
            };
        }
        if let Some(handle) = selection.chrome.handle_at(area, point) {
            return handle.cursor();
        }
        #[cfg(feature = "ocr")]
        if selection
            .text
            .as_ref()
            .is_some_and(|text| text.text_cursor(area, point))
        {
            return IDC_IBEAM;
        }
        if chrome::contains(area, point) && !selection.dragging() {
            return IDC_ARROW;
        }
    }
    IDC_CROSS
}

pub(super) unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(state) = session() else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    match msg {
        WM_PAINT => {
            unsafe { ValidateRect(hwnd, null()) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => unsafe {
            let cursor = state
                .screenshot
                .selection
                .try_borrow()
                .ok()
                .and_then(|slot| {
                    slot.as_ref().map(|selection| {
                        if selection.uploading {
                            IDC_WAIT
                        } else {
                            point_cursor(selection)
                        }
                    })
                })
                .unwrap_or(IDC_CROSS);
            SetCursor(LoadCursorW(null_mut(), cursor));
            1
        },
        WM_LBUTTONDOWN => {
            let capture = {
                let mut slot = state.screenshot.selection.borrow_mut();
                if let Some(selection) = slot.as_mut().filter(|selection| !selection.uploading) {
                    let point = point(&selection.bounds);
                    let mut start = true;
                    if let Some(area) = selection.area {
                        if chrome::contains(selection.chrome.toolbar(area), point)
                            && selection.toolbar_visible()
                        {
                            let hover = selection.chrome.action(area, point);
                            selection.pressed =
                                hover.filter(|&action| selection.action_enabled(action));
                            selection.set_toolbar_hover(hwnd, hover);
                            selection.windows.invalidate(Surface::Toolbar, None);
                            start = false;
                        } else if let Some(handle) = selection.chrome.handle_at(area, point) {
                            selection.resize = Some((handle, point, area));
                            start = false;
                        } else if chrome::contains(area, point) {
                            #[cfg(feature = "ocr")]
                            if let Some(text) = &mut selection.text {
                                text.down(area, point);
                                selection.windows.invalidate(Surface::Preview, None);
                                selection.windows.invalidate(Surface::Toolbar, None);
                            }
                            start = false;
                        }
                    }
                    if start || selection.resize.is_some() {
                        unsafe {
                            #[cfg(feature = "ocr")]
                            KillTimer(hwnd, STATUS_TIMER);
                            KillTimer(hwnd, TOOLTIP_TIMER);
                        }
                        #[cfg(feature = "ocr")]
                        {
                            selection.text = None;
                        }
                        selection.hover = None;
                        selection.tooltip = false;
                        selection.pressed = None;
                        if start {
                            selection.chrome = Chrome::new(
                                selection.bounds,
                                point,
                                selection.mode,
                                selection.english,
                            );
                            selection.start = Some(point);
                        }
                        selection.status =
                            if selection.mode == Mode::ImageSearch && selection.english {
                                "Adjust selection"
                            } else {
                                RESIZE_LABEL
                            };
                        drag(selection);
                        selection.windows.invalidate(Surface::Badge, None);
                    }
                    true
                } else {
                    false
                }
            };
            if capture {
                unsafe { SetCapture(hwnd) };
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(selection) = state.screenshot.selection.borrow_mut().as_mut() {
                let target = if unsafe { GetCapture() } == hwnd {
                    hwnd
                } else {
                    let mut cursor = POINT::default();
                    unsafe { GetCursorPos(&mut cursor) };
                    unsafe { WindowFromPoint(cursor) }
                };
                if target != selection.tracking_mouse
                    && (target == hwnd || unsafe { GetWindow(target, GW_OWNER) } == hwnd)
                {
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: target,
                        dwHoverTime: 0,
                    };
                    if unsafe { TrackMouseEvent(&mut tracking) } != 0 {
                        selection.tracking_mouse = target;
                    }
                }
                if selection.dragging() {
                    drag(selection);
                } else if let Some(area) = selection.area
                    && selection.toolbar_visible()
                {
                    let cursor = point(&selection.bounds);
                    #[cfg(feature = "ocr")]
                    let over_toolbar = chrome::contains(selection.chrome.toolbar(area), cursor);
                    let hover = selection.chrome.action(area, cursor);
                    selection.set_toolbar_hover(hwnd, hover);
                    #[cfg(feature = "ocr")]
                    if let Some(text) = &mut selection.text {
                        let changed = text.drag(area, cursor);
                        let over_text = !over_toolbar
                            && selection.chrome.handle_at(area, cursor).is_none()
                            && chrome::contains(area, cursor);
                        let hover_bounds = text.hover(area, over_text.then_some(cursor));
                        if changed {
                            selection.windows.invalidate(Surface::Preview, None);
                            selection.windows.invalidate(Surface::Toolbar, None);
                        } else {
                            for bounds in hover_bounds.into_iter().flatten() {
                                selection.windows.invalidate(Surface::Preview, Some(bounds));
                            }
                        }
                    }
                }
            }
            0
        }
        WM_MOUSELEAVE => {
            if let Some(selection) = state.screenshot.selection.borrow_mut().as_mut() {
                selection.tracking_mouse = null_mut();
                selection.set_toolbar_hover(hwnd, None);
                #[cfg(feature = "ocr")]
                if let Some(area) = selection.area {
                    if let Some(text) = &mut selection.text {
                        for bounds in text.hover(area, None).into_iter().flatten() {
                            selection.windows.invalidate(Surface::Preview, Some(bounds));
                        }
                    }
                }
            }
            0
        }
        WM_LBUTTONUP => {
            let (_submit, action) = {
                let mut slot = state.screenshot.selection.borrow_mut();
                match slot.as_mut() {
                    Some(selection) if selection.dragging() => {
                        drag(selection);
                        selection.start = None;
                        selection.resize = None;
                        if selection.area.is_none() || selection.mode == Mode::ImageSearch {
                            selection.status = match (selection.area.is_some(), selection.english) {
                                (true, true) => "Choose an image search engine · Enter",
                                (true, false) => "选择搜图引擎 · Enter",
                                (false, _) => selection.idle_label(),
                            };
                            selection.sync_windows();
                            selection.windows.invalidate(Surface::Toolbar, None);
                            selection.windows.invalidate(Surface::Badge, None);
                        }
                        (
                            selection.area.is_some() && selection.mode != Mode::ImageSearch,
                            None,
                        )
                    }
                    Some(selection) => {
                        let action = if let Some(area) = selection.area {
                            let point = point(&selection.bounds);
                            let action = selection.pressed.take().filter(|pressed| {
                                selection.chrome.action(area, point) == Some(*pressed)
                            });
                            #[cfg(feature = "ocr")]
                            if let Some(text) = &mut selection.text {
                                text.drag(area, point);
                                text.release();
                                selection.windows.invalidate(Surface::Preview, None);
                            }
                            selection.windows.invalidate(Surface::Toolbar, None);
                            action
                        } else {
                            None
                        };
                        (false, action)
                    }
                    None => (false, None),
                }
            };
            if unsafe { GetCapture() } == hwnd {
                unsafe { ReleaseCapture() };
            }
            #[cfg(feature = "ocr")]
            if _submit {
                state.recognize_area(true);
            }
            match action {
                Some(Action::Search) => state.confirm_screenshot(hwnd, true),
                Some(Action::Copy) => {
                    #[cfg(feature = "ocr")]
                    state.copy_ocr_text(hwnd);
                }
                Some(Action::Cancel) => state.end_screenshot(true),
                None => (),
            }
            0
        }
        WM_CAPTURECHANGED => {
            if let Ok(mut slot) = state.screenshot.selection.try_borrow_mut()
                && let Some(selection) = slot.as_mut()
            {
                #[cfg(feature = "ocr")]
                if let Some(text) = selection.text.as_mut() {
                    text.release();
                }
                if selection.pressed.take().is_some() {
                    selection.windows.invalidate(Surface::Toolbar, None);
                }
                if selection.dragging() {
                    selection.start = None;
                    selection.resize = None;
                    selection.area = None;
                    selection.status = selection.idle_label();
                    selection.sync_windows();
                    selection.windows.invalidate(Surface::Badge, None);
                }
            }
            0
        }
        WM_TIMER if wp == TOOLTIP_TIMER => {
            unsafe { KillTimer(hwnd, TOOLTIP_TIMER) };
            if let Some(selection) = state.screenshot.selection.borrow_mut().as_mut()
                && selection.area.is_some()
                && selection.hover.is_some()
                && selection.pressed.is_none()
            {
                selection.tooltip = true;
                selection.sync_windows();
            }
            0
        }
        #[cfg(feature = "ocr")]
        WM_TIMER if wp == STATUS_TIMER => {
            if let Some(selection) = state.screenshot.selection.borrow_mut().as_mut()
                && selection.status == LOADING_LABEL
            {
                selection.spinner = (selection.spinner + 1) % 4;
                selection.windows.invalidate(Surface::Badge, None);
            }
            0
        }
        WM_KEYDOWN if wp == VK_ESCAPE as usize => {
            state.end_screenshot(true);
            0
        }
        WM_KEYDOWN if wp == VK_RETURN as usize => {
            state.confirm_screenshot(hwnd, false);
            0
        }
        #[cfg(feature = "ocr")]
        WM_KEYDOWN if wp == VK_C as usize && unsafe { GetKeyState(i32::from(VK_CONTROL)) } < 0 => {
            state.copy_ocr_text(hwnd);
            0
        }
        IMAGE_SEARCH_MENU => {
            state.image_search_menu(hwnd, wp != 0);
            0
        }
        WM_CLOSE | WM_RBUTTONUP | WM_DISPLAYCHANGE => {
            state.end_screenshot(true);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
