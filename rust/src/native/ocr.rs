mod cache;
mod capture;
mod chrome;
mod frame;
mod render;
mod text_overlay;
mod windows;

use self::cache::{Cache, Region, intersect, same_rect};
use self::capture::{Snapshot, desktop_bounds};
use self::chrome::{Action, Chrome, Handle};
use self::render::Canvas;
use self::text_overlay::TextOverlay;
use self::windows::{OverlayWindows, Surface};
use super::diagnostics::ocr as perf;
use super::{State, instance, message, session};
use std::{
    cell::{Cell, RefCell},
    io,
    path::PathBuf,
    ptr::{null, null_mut},
    sync::mpsc,
    thread,
    time::Duration,
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::DwmFlush, Gdi::*},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
        UI::{Controls::WM_MOUSELEAVE, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    w,
};
use yisou::{
    ocr::{Model, Pixels, Recognizer, TextPage},
    settings::Preferences,
};

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.OcrSelection");
pub(super) const READY: u32 = WM_APP + 5;
pub(super) const START: u32 = WM_APP + 6;
const STATUS_TIMER: usize = 1;
const TOOLTIP_TIMER: usize = 2;
const READY_LABEL: &str = "识别完成";
const LOADING_LABEL: &str = "正在识别";
const IDLE_LABEL: &str = "框选要识别的区域";
const RESIZE_LABEL: &str = "调整选区";
const EMPTY_LABEL: &str = "未识别到文字 · 重新框选";
const FAILED_LABEL: &str = "识别失败 · 重新框选";
const KEEP_WARM: Duration = Duration::from_secs(10);

pub(super) fn register() -> io::Result<()> {
    super::register(CLASS, Some(proc))?;
    super::register(windows::CLASS, Some(windows::surface_proc))
}

#[derive(Default)]
pub(super) struct OcrTasks {
    overlay: Cell<HWND>,
    selection: RefCell<Option<Selection>>,
    next_id: Cell<usize>,
    worker: RefCell<Option<Worker>>,
}

struct Selection {
    canvas: RefCell<Option<Canvas>>,
    bounds: RECT,
    windows: OverlayWindows,
    snapshot: Option<Snapshot>,
    previous: HWND,
    engine: Option<String>,
    start: Option<POINT>,
    area: Option<RECT>,
    pending: Option<Pending>,
    status: &'static str,
    spinner: u8,
    cache: Cache,
    chrome: Chrome,
    resize: Option<(Handle, POINT, RECT)>,
    hover: Option<Action>,
    pressed: Option<Action>,
    tracking_mouse: HWND,
    tooltip: bool,
    text: Option<TextOverlay>,
}

struct Pending {
    id: usize,
    requested: Option<RECT>,
    matching: Option<RECT>,
}

impl Selection {
    fn dragging(&self) -> bool {
        self.start.is_some() || self.resize.is_some()
    }

    fn sync_windows(&mut self) {
        let mut surfaces = [None; 5];
        if let Some(area) = self.area {
            if !self.dragging() && self.snapshot.is_some() {
                surfaces[0] = Some(area);
                let mut bar = self.chrome.toolbar(area);
                bar.bottom += 2;
                surfaces[3] = Some(bar);
                if self.tooltip
                    && let Some(action) = self.hover
                {
                    surfaces[4] = Some(
                        self.chrome
                            .tooltip(area, self.has_selected_text(), action)
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
                    renderer.draw_preview(dc, bounds, paint.rcPaint, snapshot, |canvas| {
                        if let Some(text) = &self.text {
                            text.paint(canvas, bounds);
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
                                    self.has_selected_text(),
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
                                    self.has_selected_text(),
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
        self.text
            .as_ref()
            .and_then(TextOverlay::selected_text)
            .is_some()
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

struct Job {
    id: usize,
    directory: PathBuf,
    regions: Vec<(Region, Pixels)>,
}

struct Worker {
    sender: mpsc::Sender<Job>,
    results: mpsc::Receiver<(usize, Vec<(Region, TextPage)>, bool)>,
}

impl Worker {
    fn new(hwnd: HWND) -> io::Result<Self> {
        let (sender, requests) = mpsc::channel::<Job>();
        let (output, results) = mpsc::channel();
        let window = hwnd as usize;
        thread::Builder::new().name("ocr".into()).spawn(move || {
            let mut session: Option<(PathBuf, Recognizer)> = None;
            loop {
                let mut job = if session.is_some() {
                    match requests.recv_timeout(KEEP_WARM) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            session = None;
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    let Ok(job) = requests.recv() else { break };
                    job
                };
                while let Ok(latest) = requests.try_recv() {
                    job = latest;
                }
                let mut pages = Vec::new();
                let failed = (|| -> Result<bool, yisou::ocr::Error> {
                    if session
                        .as_ref()
                        .is_none_or(|(path, _)| *path != job.directory)
                    {
                        session = None;
                        let recognizer = Recognizer::open_directory(&job.directory, Model::Tiny)?;
                        session = Some((job.directory, recognizer));
                    }
                    let recognizer = &mut session.as_mut().unwrap().1;
                    let mut failed = false;
                    for (rect, pixels) in job.regions {
                        match recognizer.recognize_text_page(pixels) {
                            Ok(page) => pages.push((rect, page)),
                            Err(_) => failed = true,
                        }
                    }
                    Ok(failed)
                })()
                .unwrap_or(true);
                if output.send((job.id, pages, failed)).is_err() {
                    break;
                }
                unsafe { PostMessageW(window as HWND, READY, 0, 0) };
            }
        })?;
        Ok(Self { sender, results })
    }
}

impl State {
    pub(super) fn ocr_directory(&self, path: &std::path::Path) -> PathBuf {
        self.config
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .join(path)
    }

    pub(super) fn validate_ocr_directory(&self, prefs: &Preferences) -> Result<(), String> {
        let path = prefs
            .ocr_model_dir
            .as_deref()
            .ok_or("请选择 OCR 模型目录")?;
        Model::Tiny
            .validate_directory(&self.ocr_directory(path))
            .map_err(|error| error.to_string())
    }

    pub(super) fn start_ocr(&self) {
        if self
            .app
            .borrow()
            .settings()
            .preferences
            .active_ocr_model_dir()
            .is_none()
        {
            return;
        }
        self.end_ocr(true);
        let _span = perf::span("open_overlay");
        let hwnd = self.window.get();
        let active = unsafe { GetForegroundWindow() };
        let previous = if active == hwnd {
            self.launch_focus.get()
        } else {
            active
        };
        let engine = self.temporary.borrow().clone();
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
        let chrome = Chrome::new(bounds, point(&bounds));
        let overlay = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
                CLASS,
                w!("易搜 · 框选识别"),
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
            canvas: RefCell::new(None),
            bounds,
            windows,
            snapshot: None,
            previous,
            engine,
            start: None,
            area: None,
            pending: None,
            status: IDLE_LABEL,
            spinner: 0,
            cache: Cache::default(),
            chrome,
            resize: None,
            hover: None,
            pressed: None,
            tracking_mouse: null_mut(),
            tooltip: false,
            text: None,
        };
        selection.sync_windows();
        *self.ocr.selection.borrow_mut() = Some(selection);
        self.ocr.overlay.set(overlay);
        unsafe {
            ShowWindow(overlay, SW_SHOW);
            SetForegroundWindow(overlay);
            SetFocus(overlay);
        }
        self.diagnostics.sample("ocr_overlay_shown");
    }

    pub(super) fn end_ocr(&self, restore: bool) {
        let overlay = self.ocr.overlay.replace(null_mut());
        let selection = self.ocr.selection.borrow_mut().take();
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

    fn recognize_area(&self, capture: bool) {
        if self.send_ocr(capture).is_err()
            && let Some(selection) = self.ocr.selection.borrow_mut().as_mut()
        {
            self.diagnostics.sample("ocr_readback_failed");
            unsafe { KillTimer(self.ocr.overlay.get(), STATUS_TIMER) };
            selection.status = FAILED_LABEL;
            selection.sync_windows();
            selection.windows.invalidate(Surface::Badge, None);
        }
    }

    fn send_ocr(&self, capture: bool) -> io::Result<()> {
        let _span = perf::span("submit_ocr");
        let directory = self
            .app
            .borrow()
            .settings()
            .preferences
            .active_ocr_model_dir()
            .map(|path| self.ocr_directory(path))
            .ok_or_else(|| io::Error::other("OCR is disabled"))?;
        let mut selection = self.ocr.selection.borrow_mut();
        let Some(selection) = selection.as_mut() else {
            return Ok(());
        };
        let Some(area) = selection.area else {
            return Ok(());
        };
        let overlay = self.ocr.overlay.get();
        let margin = selection.chrome.ocr_margin();
        if capture {
            let current = match selection.windows.capture(area, margin) {
                Ok(current) => current,
                Err(error) => {
                    selection.snapshot = None;
                    selection.cache = Cache::default();
                    if let Some(pending) = &mut selection.pending {
                        pending.requested = None;
                        pending.matching = None;
                    }
                    return Err(error);
                }
            };
            self.diagnostics.sample("ocr_capture_region");
            let compare = perf::start();
            let matching = selection
                .snapshot
                .as_ref()
                .and_then(|previous| previous.matching_area(&current));
            perf::record("compare_pixels", compare);
            if !matching.is_some_and(|matching| same_rect(matching, current.area)) {
                selection.cache.retain_within(matching);
                if let Some(pending) = &mut selection.pending {
                    pending.matching = pending
                        .matching
                        .zip(matching)
                        .and_then(|(previous, current)| intersect(previous, current));
                    pending.requested = None;
                }
                selection.snapshot = Some(current);
            }
            selection.sync_windows();
            selection.windows.invalidate(Surface::Preview, None);
        }
        if selection.pending.is_some() {
            selection.show_loading(overlay);
            return Ok(());
        }
        let desktop = RECT {
            right: selection.bounds.right - selection.bounds.left,
            bottom: selection.bounds.bottom - selection.bounds.top,
            ..RECT::default()
        };
        let uncovered = selection.cache.regions(area, desktop, margin);
        if uncovered.is_empty() {
            self.update_ocr_view(selection, overlay, false);
            self.diagnostics.sample("ocr_cache_hit");
            return Ok(());
        }
        let id = self.ocr.next_id.get().wrapping_add(1);
        self.ocr.next_id.set(id);
        let Some(snapshot) = selection.snapshot.as_ref() else {
            return Ok(());
        };
        let crop = perf::start();
        let job = Job {
            id,
            directory,
            regions: uncovered
                .into_iter()
                .map(|region| (region, snapshot.crop(region.source)))
                .collect(),
        };
        perf::record("prepare_regions", crop);
        let mut worker = self.ocr.worker.borrow_mut();
        let job = if let Some(worker) = worker.as_ref() {
            match worker.sender.send(job) {
                Ok(()) => None,
                Err(error) => Some(error.0),
            }
        } else {
            Some(job)
        };
        if let Some(job) = job {
            let new_worker = Worker::new(self.window.get())?;
            new_worker.sender.send(job).map_err(io::Error::other)?;
            *worker = Some(new_worker);
        }
        selection.pending = Some(Pending {
            id,
            requested: Some(area),
            matching: Some(snapshot.area),
        });
        selection.show_loading(overlay);
        self.diagnostics.sample("ocr_job_started");
        Ok(())
    }

    pub(super) fn finish_ocr(&self) {
        let result = self
            .ocr
            .worker
            .borrow()
            .as_ref()
            .and_then(|worker| worker.results.try_recv().ok());
        let Some((id, pages, failed)) = result else {
            return;
        };
        let overlay = self.ocr.overlay.get();
        let mut slot = self.ocr.selection.borrow_mut();
        let Some(selection) = slot.as_mut() else {
            return;
        };
        if selection
            .pending
            .as_ref()
            .is_none_or(|pending| pending.id != id)
        {
            return;
        }
        let pending = selection.pending.take().unwrap();
        unsafe { KillTimer(overlay, STATUS_TIMER) };
        for (mut region, page) in pages {
            if let Some(covered) = pending
                .matching
                .and_then(|area| intersect(region.covered, area))
            {
                region.covered = covered;
                selection.cache.insert(region, page);
            }
        }
        if selection.dragging() {
            return;
        }
        if let Some(area) = selection.area {
            if pending
                .requested
                .is_some_and(|requested| same_rect(requested, area))
            {
                self.update_ocr_view(selection, overlay, failed);
            } else {
                drop(slot);
                self.recognize_area(false);
            }
        }
    }

    fn update_ocr_view(&self, selection: &mut Selection, overlay: HWND, failed: bool) {
        let Some(area) = selection.area else { return };
        let Some(snapshot) = selection.snapshot.as_ref() else {
            return;
        };
        unsafe { KillTimer(overlay, STATUS_TIMER) };
        let page = selection.cache.page(area);
        let empty = page.text(None).is_empty();
        selection.status = match (empty, failed) {
            (true, true) => FAILED_LABEL,
            (true, false) => EMPTY_LABEL,
            (false, true) => "部分区域识别失败",
            (false, false) => READY_LABEL,
        };
        selection.text = (!empty).then(|| TextOverlay::new(page, snapshot, area));
        selection.sync_windows();
        selection.windows.invalidate(Surface::Preview, None);
        selection.windows.invalidate(Surface::Toolbar, None);
        selection.windows.invalidate(Surface::Badge, None);
        self.diagnostics.sample(if empty {
            if failed { "ocr_failed" } else { "ocr_no_text" }
        } else {
            "ocr_text_ready"
        });
    }

    fn copy_ocr_text(&self, hwnd: HWND) {
        let mut slot = self.ocr.selection.borrow_mut();
        let Some(selection) = slot.as_mut() else {
            return;
        };
        let Some(text) = selection.text.as_ref().and_then(TextOverlay::selected_text) else {
            return;
        };
        selection.status = if write_clipboard(hwnd, text) {
            "已复制"
        } else {
            "复制失败"
        };
        selection.sync_windows();
        selection.windows.invalidate(Surface::Badge, None);
    }

    fn search_ocr_text(&self) {
        let mut slot = self.ocr.selection.borrow_mut();
        let Some(selection) = slot.as_mut() else {
            return;
        };
        let (Some(area), Some(text)) = (
            selection.area,
            selection.text.as_ref().and_then(TextOverlay::selected_text),
        ) else {
            return;
        };
        let text = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let anchor = POINT {
            x: selection.bounds.left + area.left,
            y: selection.bounds.top + area.bottom,
        };
        let previous = selection.previous;
        let engine = selection.engine.take();
        drop(slot);
        self.end_ocr(true);
        self.retry.borrow_mut().take();
        *self.temporary.borrow_mut() = engine;
        self.ocr_draft.set(true);
        self.set_input(&text);
        self.show_at(Some(anchor));
        self.launch_focus.set(previous);
        self.diagnostics.sample("ocr_finished");
    }
}

fn write_clipboard(hwnd: HWND, text: &str) -> bool {
    let text = text.replace('\n', "\r\n");
    let text = super::wide(&text);
    unsafe {
        let mut memory = GlobalAlloc(GMEM_MOVEABLE, text.len() * size_of::<u16>());
        if memory.is_null() {
            return false;
        }
        let target = GlobalLock(memory).cast::<u16>();
        if !target.is_null() {
            std::ptr::copy_nonoverlapping(text.as_ptr(), target, text.len());
            GlobalUnlock(memory);
            if OpenClipboard(hwnd) != 0 {
                if EmptyClipboard() != 0
                    && !SetClipboardData(u32::from(CF_UNICODETEXT), memory).is_null()
                {
                    memory = null_mut();
                }
                CloseClipboard();
            }
        }
        if memory.is_null() {
            true
        } else {
            GlobalFree(memory);
            false
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
        if selection.snapshot.is_some() && chrome::contains(selection.chrome.toolbar(area), point) {
            return match selection.chrome.action(area, point) {
                Some(Action::Cancel) => IDC_HAND,
                Some(_) if selection.has_selected_text() => IDC_HAND,
                _ => IDC_ARROW,
            };
        }
        if let Some(handle) = selection.chrome.handle_at(area, point) {
            return handle.cursor();
        }
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
                .ocr
                .selection
                .try_borrow()
                .ok()
                .and_then(|slot| slot.as_ref().map(|selection| point_cursor(selection)))
                .unwrap_or(IDC_CROSS);
            SetCursor(LoadCursorW(null_mut(), cursor));
            1
        },
        WM_LBUTTONDOWN => {
            let capture = {
                let mut slot = state.ocr.selection.borrow_mut();
                if let Some(selection) = slot.as_mut() {
                    let point = point(&selection.bounds);
                    let mut start = true;
                    if let Some(area) = selection.area {
                        if chrome::contains(selection.chrome.toolbar(area), point)
                            && selection.snapshot.is_some()
                        {
                            let hover = selection.chrome.action(area, point);
                            selection.pressed = hover.filter(|&action| {
                                action == Action::Cancel || selection.has_selected_text()
                            });
                            selection.set_toolbar_hover(hwnd, hover);
                            selection.windows.invalidate(Surface::Toolbar, None);
                            start = false;
                        } else if let Some(handle) = selection.chrome.handle_at(area, point) {
                            selection.resize = Some((handle, point, area));
                            start = false;
                        } else if chrome::contains(area, point) {
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
                            KillTimer(hwnd, STATUS_TIMER);
                            KillTimer(hwnd, TOOLTIP_TIMER);
                        }
                        selection.text = None;
                        selection.hover = None;
                        selection.tooltip = false;
                        selection.pressed = None;
                        if start {
                            selection.chrome = Chrome::new(selection.bounds, point);
                            selection.start = Some(point);
                        }
                        selection.status = RESIZE_LABEL;
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
            if let Some(selection) = state.ocr.selection.borrow_mut().as_mut() {
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
                    && selection.snapshot.is_some()
                {
                    let cursor = point(&selection.bounds);
                    let over_toolbar = chrome::contains(selection.chrome.toolbar(area), cursor);
                    let hover = selection.chrome.action(area, cursor);
                    selection.set_toolbar_hover(hwnd, hover);
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
            if let Some(selection) = state.ocr.selection.borrow_mut().as_mut() {
                selection.tracking_mouse = null_mut();
                selection.set_toolbar_hover(hwnd, None);
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
            let (submit, action) = {
                let mut slot = state.ocr.selection.borrow_mut();
                match slot.as_mut() {
                    Some(selection) if selection.dragging() => {
                        drag(selection);
                        selection.start = None;
                        selection.resize = None;
                        if selection.area.is_none() {
                            selection.status = IDLE_LABEL;
                            selection.sync_windows();
                            selection.windows.invalidate(Surface::Badge, None);
                        }
                        (selection.area.is_some(), None)
                    }
                    Some(selection) => {
                        let action = if let Some(area) = selection.area {
                            let point = point(&selection.bounds);
                            let action = selection.pressed.take().filter(|pressed| {
                                selection.chrome.action(area, point) == Some(*pressed)
                            });
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
            if submit {
                state.recognize_area(true);
            }
            match action {
                Some(Action::Search) => state.search_ocr_text(),
                Some(Action::Copy) => state.copy_ocr_text(hwnd),
                Some(Action::Cancel) => state.end_ocr(true),
                None => (),
            }
            0
        }
        WM_CAPTURECHANGED => {
            if let Ok(mut slot) = state.ocr.selection.try_borrow_mut()
                && let Some(selection) = slot.as_mut()
            {
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
                    selection.status = IDLE_LABEL;
                    selection.sync_windows();
                    selection.windows.invalidate(Surface::Badge, None);
                }
            }
            0
        }
        WM_TIMER if wp == TOOLTIP_TIMER => {
            unsafe { KillTimer(hwnd, TOOLTIP_TIMER) };
            if let Some(selection) = state.ocr.selection.borrow_mut().as_mut()
                && selection.area.is_some()
                && selection.hover.is_some()
                && selection.pressed.is_none()
            {
                selection.tooltip = true;
                selection.sync_windows();
            }
            0
        }
        WM_TIMER if wp == STATUS_TIMER => {
            if let Some(selection) = state.ocr.selection.borrow_mut().as_mut()
                && selection.status == LOADING_LABEL
            {
                selection.spinner = (selection.spinner + 1) % 4;
                selection.windows.invalidate(Surface::Badge, None);
            }
            0
        }
        WM_KEYDOWN if wp == VK_ESCAPE as usize => {
            state.end_ocr(true);
            0
        }
        WM_KEYDOWN if wp == VK_RETURN as usize => {
            state.search_ocr_text();
            0
        }
        WM_KEYDOWN if wp == VK_C as usize && unsafe { GetKeyState(i32::from(VK_CONTROL)) } < 0 => {
            state.copy_ocr_text(hwnd);
            0
        }
        WM_CLOSE | WM_RBUTTONUP | WM_DISPLAYCHANGE => {
            state.end_ocr(true);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
