use super::{
    EMPTY_LABEL, FAILED_LABEL, Mode, Pending, READY, READY_LABEL, STATUS_TIMER, Selection, State,
    Surface, TextOverlay,
    cache::{Cache, Region},
    capture::{intersect, same_rect},
    perf,
};
use std::{io, path::PathBuf, sync::mpsc, thread, time::Duration};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::WindowsAndMessaging::*,
};
use yisou::{
    ocr::{Model, Pixels, Recognizer, TextPage},
    settings::Preferences,
};

const KEEP_WARM: Duration = Duration::from_secs(10);

struct Job {
    id: usize,
    directory: PathBuf,
    regions: Vec<(Region, Pixels)>,
}

pub(super) struct Worker {
    sender: mpsc::Sender<Job>,
    results: mpsc::Receiver<(usize, Vec<(Region, TextPage)>, RecognitionStatus)>,
}

#[derive(Clone, Copy)]
enum RecognitionStatus {
    Complete,
    RegionFailed,
    ModelFailed,
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
                let status = (|| -> Result<RecognitionStatus, yisou::ocr::Error> {
                    if session
                        .as_ref()
                        .is_none_or(|(path, _)| *path != job.directory)
                    {
                        session = None;
                        let recognizer = Recognizer::open_directory(&job.directory, Model::Tiny)?;
                        session = Some((job.directory, recognizer));
                    }
                    let recognizer = &mut session.as_mut().unwrap().1;
                    let mut status = RecognitionStatus::Complete;
                    for (rect, pixels) in job.regions {
                        match recognizer.recognize_text_page(pixels) {
                            Ok(page) => pages.push((rect, page)),
                            Err(_) => status = RecognitionStatus::RegionFailed,
                        }
                    }
                    Ok(status)
                })()
                .unwrap_or(RecognitionStatus::ModelFailed);
                if output.send((job.id, pages, status)).is_err() {
                    break;
                }
                unsafe { PostMessageW(window as HWND, READY, 0, 0) };
            }
        })?;
        Ok(Self { sender, results })
    }
}

impl State {
    pub(in crate::native) fn ocr_directory(&self, path: &std::path::Path) -> PathBuf {
        self.config
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .join(path)
    }

    pub(in crate::native) fn validate_ocr_directory(
        &self,
        prefs: &Preferences,
    ) -> Result<(), String> {
        let path = prefs
            .ocr_model_dir
            .as_deref()
            .ok_or("请选择 OCR 模型目录")?;
        Model::Tiny
            .validate_directory(&self.ocr_directory(path))
            .map_err(|error| error.to_string())
    }

    pub(in crate::native) fn end_ocr_if_active(&self) {
        if self
            .screenshot
            .selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.mode == Mode::Ocr)
        {
            self.end_screenshot(true);
        }
    }

    pub(in crate::native) fn start_ocr(&self) {
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
        self.start_screenshot(Mode::Ocr);
    }

    pub(super) fn recognize_area(&self, capture: bool) {
        if self.send_ocr(capture).is_err()
            && let Some(selection) = self.screenshot.selection.borrow_mut().as_mut()
        {
            self.diagnostics.sample("ocr_readback_failed");
            unsafe { KillTimer(self.screenshot.overlay.get(), STATUS_TIMER) };
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
        let mut selection = self.screenshot.selection.borrow_mut();
        let Some(selection) = selection.as_mut() else {
            return Ok(());
        };
        let Some(area) = selection.area else {
            return Ok(());
        };
        let overlay = self.screenshot.overlay.get();
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
            self.update_ocr_view(selection, overlay, RecognitionStatus::Complete);
            self.diagnostics.sample("ocr_cache_hit");
            return Ok(());
        }
        let id = self.screenshot.next_id.get().wrapping_add(1);
        self.screenshot.next_id.set(id);
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
        let mut worker = self.screenshot.worker.borrow_mut();
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

    pub(in crate::native) fn finish_ocr(&self) {
        let result = self
            .screenshot
            .worker
            .borrow()
            .as_ref()
            .and_then(|worker| worker.results.try_recv().ok());
        let Some((id, pages, status)) = result else {
            return;
        };
        let overlay = self.screenshot.overlay.get();
        let mut slot = self.screenshot.selection.borrow_mut();
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
                self.update_ocr_view(selection, overlay, status);
            } else {
                drop(slot);
                self.recognize_area(false);
            }
        }
    }

    fn update_ocr_view(&self, selection: &mut Selection, overlay: HWND, status: RecognitionStatus) {
        let Some(area) = selection.area else { return };
        let Some(snapshot) = selection.snapshot.as_ref() else {
            return;
        };
        unsafe { KillTimer(overlay, STATUS_TIMER) };
        let page = selection.cache.page(area);
        let empty = page.text(None).is_empty();
        selection.status = match (empty, status) {
            (_, RecognitionStatus::ModelFailed) if selection.english => {
                "Could not load OCR models · Check Settings"
            }
            (_, RecognitionStatus::ModelFailed) => "模型加载失败 · 请检查 OCR 设置",
            (true, RecognitionStatus::RegionFailed) => FAILED_LABEL,
            (true, RecognitionStatus::Complete) => EMPTY_LABEL,
            (false, RecognitionStatus::RegionFailed) => "部分区域识别失败",
            (false, RecognitionStatus::Complete) => READY_LABEL,
        };
        selection.text = (!empty).then(|| TextOverlay::new(page, snapshot, area));
        selection.sync_windows();
        selection.windows.invalidate(Surface::Preview, None);
        selection.windows.invalidate(Surface::Toolbar, None);
        selection.windows.invalidate(Surface::Badge, None);
        self.diagnostics.sample(match (empty, status) {
            (_, RecognitionStatus::ModelFailed) => "ocr_model_load_failed",
            (true, RecognitionStatus::RegionFailed) => "ocr_failed",
            (true, RecognitionStatus::Complete) => "ocr_no_text",
            (false, _) => "ocr_text_ready",
        });
    }

    pub(super) fn copy_ocr_text(&self, hwnd: HWND) {
        let mut slot = self.screenshot.selection.borrow_mut();
        let Some(selection) = slot.as_mut() else {
            return;
        };
        let Some(text) = selection.text.as_ref().and_then(TextOverlay::selected_text) else {
            return;
        };
        selection.status = if super::super::write_clipboard(hwnd, &text.replace('\n', "\r\n")) {
            "已复制"
        } else {
            "复制失败"
        };
        selection.sync_windows();
        selection.windows.invalidate(Surface::Badge, None);
    }

    pub(super) fn search_ocr_text(&self) {
        let mut slot = self.screenshot.selection.borrow_mut();
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
        self.end_screenshot(true);
        self.retry.borrow_mut().take();
        *self.temporary.borrow_mut() = engine;
        self.ocr_draft.set(true);
        self.set_input(&text);
        self.show_at(Some(anchor));
        self.launch_focus.set(previous);
        self.diagnostics.sample("ocr_finished");
    }
}
