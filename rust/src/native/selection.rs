use super::State;
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
use yisou::{
    application::{
        SelectionAction, SelectionMode, SelectionRequest, SelectionRequests, clipboard_query,
    },
    platform::{self, Foreground, Hotkey},
};

pub(super) const DOUBLE_ALT: u32 = WM_APP + 3;
pub(super) const SELECTION_READY: u32 = WM_APP + 4;
pub(super) const SELECTION_TIMER: usize = 1;

#[derive(Default)]
pub(super) struct SelectionTasks {
    requests: RefCell<SelectionRequests>,
    pending: RefCell<Option<PendingSelection>>,
    worker_busy: Arc<AtomicBool>,
}

struct PendingSelection {
    foreground: Foreground,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    source: String,
    request: SelectionRequest,
    receiver: mpsc::Receiver<String>,
}

impl State {
    pub(super) fn cancel_selection(&self) {
        if let Some(request) = self.selection.pending.borrow_mut().take() {
            request.cancelled.store(true, Ordering::Relaxed);
        }
        self.selection.requests.borrow_mut().cancel();
        unsafe { KillTimer(self.window.get(), SELECTION_TIMER) };
    }

    pub(super) fn hotkey(&self, role: Hotkey, double_alt: bool) {
        #[cfg(feature = "ocr")]
        if role == Hotkey::Ocr {
            // WinRT capture can call into COM outside the synchronous input message.
            unsafe { PostMessageW(self.window.get(), super::ocr::START, 0, 0) };
            return;
        } else {
            self.end_ocr(true);
        }
        if role == Hotkey::Clipboard {
            if let Ok(text) = platform::clipboard_text() {
                self.search(
                    clipboard_query(&text, self.source_label(Foreground::capture())),
                    true,
                );
            }
            return;
        }
        self.cancel_selection();
        let Some(foreground) = Foreground::capture() else {
            self.show();
            return;
        };
        if foreground.id() == self.window.get() as usize {
            self.show();
            return;
        }
        let process = foreground.executable();
        if double_alt
            && process
                .as_ref()
                .and_then(|p| p.file_name())
                .is_some_and(|exe| {
                    self.app
                        .borrow()
                        .settings()
                        .preferences
                        .excluded_apps
                        .iter()
                        .any(|item| exe.eq_ignore_ascii_case(item))
                })
        {
            return;
        }
        let mode = if role == Hotkey::Selection {
            SelectionMode::Dedicated
        } else {
            SelectionMode::Main
        };
        let copy = (!double_alt).then(|| {
            let app = self.app.borrow();
            let prefs = &app.settings().preferences;
            platform::SelectionCopy {
                clipboard_window: self.window.get() as usize,
                shortcut_key: if mode == SelectionMode::Dedicated {
                    prefs
                        .selection
                        .as_ref()
                        .map_or(0, |binding| binding.shortcut.key)
                } else {
                    prefs.main_shortcut.key
                },
            }
        });
        let request = self.selection.requests.borrow_mut().begin(
            foreground.id(),
            mode,
            copy.is_some(),
            Instant::now(),
        );
        let deadline = request.deadline();
        let source = process
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cancelled = Arc::new(AtomicBool::new(false));
        if self.selection.worker_busy.swap(true, Ordering::AcqRel) {
            self.finish_selection(foreground, request, "", source);
            return;
        }
        let worker = self.selection.worker_busy.clone();
        let flag = cancelled.clone();
        let window = self.window.get() as usize;
        let (send, receive) = mpsc::sync_channel(1);
        self.selection.pending.replace(Some(PendingSelection {
            foreground,
            deadline,
            cancelled,
            source,
            request,
            receiver: receive,
        }));
        let spawned = std::thread::Builder::new().spawn(move || {
            let result = foreground
                .read_selection(deadline, copy, &flag)
                .unwrap_or_default();
            worker.store(false, Ordering::Release);
            if send.send(result).is_ok() {
                unsafe {
                    PostMessageW(window as HWND, SELECTION_READY, 0, 0);
                }
            }
        });
        if spawned.is_err() {
            self.selection.worker_busy.store(false, Ordering::Release);
            self.poll_selection(true);
            return;
        }
        let delay = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1) as u32;
        unsafe { SetTimer(self.window.get(), SELECTION_TIMER, delay, None) };
    }

    fn source_label(&self, foreground: Option<Foreground>) -> String {
        foreground
            .and_then(Foreground::executable)
            .and_then(|path| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    }

    pub(super) fn poll_selection(&self, timer: bool) {
        let mut pending = self.selection.pending.borrow_mut();
        let Some(request) = pending.as_ref() else {
            return;
        };
        let answer = request.receiver.try_recv().ok();
        let timed_out = Instant::now() >= request.deadline;
        if answer.is_none()
            && (!timer || (!timed_out && self.selection.worker_busy.load(Ordering::Acquire)))
        {
            return;
        }
        let request = pending.take();
        drop(pending);
        if let Some(pending) = request {
            pending.cancelled.store(true, Ordering::Relaxed);
            unsafe { KillTimer(self.window.get(), SELECTION_TIMER) };
            self.finish_selection(
                pending.foreground,
                pending.request,
                &answer.unwrap_or_default(),
                pending.source,
            );
        }
    }

    fn finish_selection(
        &self,
        foreground: Foreground,
        request: SelectionRequest,
        text: &str,
        source: String,
    ) {
        let action = self.selection.requests.borrow_mut().finish(
            request,
            if foreground.is_current() {
                foreground.id()
            } else {
                0
            },
            Instant::now(),
            text,
            source,
            self.app.borrow().settings(),
        );
        match action {
            SelectionAction::Ignore => {}
            SelectionAction::ShowSearch => self.show(),
            SelectionAction::Search(query) => self.search(query, true),
            SelectionAction::Preview(query) => {
                *self.temporary.borrow_mut() = query.engine.clone();
                self.set_input(&query.text);
                *self.retry.borrow_mut() = Some(query);
                self.show();
            }
        }
    }
}
