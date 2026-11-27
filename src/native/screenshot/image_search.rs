use super::reverse_image::{self, Engine};
use super::{IMAGE_SEARCH_MENU, IMAGE_SEARCH_READY, Mode, Snapshot, State, Surface, message};
use std::{
    io,
    ptr::null,
    sync::{Arc, mpsc},
    thread,
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::*,
};
use windows_sys::w;
use yisou::settings::ImageSearchAction;

const ENGINES: [Engine; 2] = [Engine::Yandex, Engine::GoogleLens];

enum Destination {
    Selection(HWND),
    Browser,
    Dismissed,
}

pub(super) struct Retry {
    png: Arc<Vec<u8>>,
    engines: Vec<Engine>,
}

type UploadResult = (Engine, io::Result<String>, Option<Arc<Vec<u8>>>);

pub(super) struct Upload {
    destination: Destination,
    result: mpsc::Receiver<UploadResult>,
    remaining: usize,
    failures: Vec<String>,
    retry: Option<Retry>,
}

impl Upload {
    pub fn dismiss(&mut self, overlay: HWND) {
        if matches!(self.destination, Destination::Selection(owner) if owner == overlay) {
            self.destination = Destination::Dismissed;
        }
    }

    fn record_failure(&mut self, engine: Engine, error: io::Error, png: Option<Arc<Vec<u8>>>) {
        self.failures.push(format!("{} · {error}", engine.name()));
        if let Some(png) = png {
            self.retry
                .get_or_insert_with(|| Retry {
                    png,
                    engines: Vec::new(),
                })
                .engines
                .push(engine);
        }
    }
}

fn configured_engines(action: ImageSearchAction) -> Option<&'static [Engine]> {
    match action {
        ImageSearchAction::Choose => None,
        ImageSearchAction::Both => Some(&ENGINES),
        ImageSearchAction::Yandex => Some(&ENGINES[..1]),
        ImageSearchAction::GoogleLens => Some(&ENGINES[1..]),
    }
}

impl State {
    pub(super) fn confirm_screenshot(&self, hwnd: HWND, choose: bool) {
        let mode = self
            .screenshot
            .selection
            .borrow()
            .as_ref()
            .and_then(|selection| {
                (selection.area.is_some() && !selection.dragging() && !selection.uploading)
                    .then_some(selection.mode)
            });
        match mode {
            Some(Mode::ImageSearch) => unsafe {
                PostMessageW(hwnd, IMAGE_SEARCH_MENU, usize::from(choose), 0);
            },
            #[cfg(feature = "ocr")]
            Some(Mode::Ocr) => self.search_ocr_text(),
            None => (),
        }
    }

    pub(super) fn image_search_menu(&self, hwnd: HWND, choose: bool) {
        if self.screenshot.image_upload.borrow().is_some() {
            return;
        }
        let (button, english, snapshot) = {
            let slot = self.screenshot.selection.borrow();
            let Some(selection) = slot.as_ref().filter(|selection| {
                selection.mode == Mode::ImageSearch && !selection.dragging() && !selection.uploading
            }) else {
                return;
            };
            let Some(area) = selection.area else { return };
            let button = selection.chrome.search_button(area);
            (
                RECT {
                    left: selection.bounds.left + button.left,
                    bottom: selection.bounds.top + button.bottom,
                    ..Default::default()
                },
                selection.english,
                selection.windows.capture(area, 0),
            )
        };
        if let Err(error) = self.submit_image_search(hwnd, button, english, snapshot, choose) {
            show_error(hwnd, english, error);
        }
    }

    fn submit_image_search(
        &self,
        hwnd: HWND,
        button: RECT,
        english: bool,
        snapshot: io::Result<Snapshot>,
        choose: bool,
    ) -> io::Result<()> {
        let snapshot = snapshot?;
        let action = self.app.borrow().settings().preferences.image_search_action;
        let engines = match (!choose).then(|| configured_engines(action)).flatten() {
            Some(engines) => engines,
            None => {
                let Some(engines) = choose_engines(hwnd, button, english)? else {
                    return Ok(());
                };
                engines
            }
        };
        if self.screenshot.overlay.get() != hwnd {
            return Ok(());
        }
        self.begin_image_upload(Destination::Selection(hwnd), engines.to_vec(), move || {
            reverse_image::encode_png(snapshot).map(Arc::new)
        })?;
        self.screenshot.image_retry.borrow_mut().take();
        if let Some(selection) = self.screenshot.selection.borrow_mut().as_mut() {
            selection.uploading = true;
            selection.hover = None;
            selection.tooltip = false;
            selection.status = match (engines.len() > 1, english) {
                (true, true) => "Searching both engines…",
                (true, false) => "正在使用两个引擎搜索…",
                (false, true) => "Uploading screenshot…",
                (false, false) => "正在上传截图…",
            };
            selection.sync_windows();
            selection.windows.invalidate(Surface::Badge, None);
        }
        Ok(())
    }

    pub(in crate::native) fn can_retry_image_search(&self) -> bool {
        self.screenshot.image_retry.borrow().is_some()
            && self.screenshot.image_upload.borrow().is_none()
    }

    pub(in crate::native) fn retry_image_search(&self) {
        if !self.can_retry_image_search() {
            return;
        }
        let mut slot = self.screenshot.image_retry.borrow_mut();
        let retry = slot.as_ref().unwrap();
        let png = Arc::clone(&retry.png);
        match self.begin_image_upload(Destination::Browser, retry.engines.clone(), move || Ok(png))
        {
            Ok(()) => {
                slot.take();
            }
            Err(error) => {
                drop(slot);
                let english = super::super::preferences::english_selected(
                    self.app.borrow().settings().preferences.language,
                );
                show_error(self.window.get(), english, error);
            }
        }
    }

    fn begin_image_upload(
        &self,
        destination: Destination,
        engines: Vec<Engine>,
        encode: impl FnOnce() -> io::Result<Arc<Vec<u8>>> + Send + 'static,
    ) -> io::Result<()> {
        let mut slot = self.screenshot.image_upload.borrow_mut();
        if slot.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "An image upload is already in progress.",
            ));
        }
        let count = engines.len();
        let (sender, result) = mpsc::channel();
        let target = self.window.get() as usize;
        thread::Builder::new()
            .name("image-search".into())
            .spawn(move || {
                let report = |engine, result, png| {
                    if sender.send((engine, result, png)).is_ok() {
                        unsafe { PostMessageW(target as HWND, IMAGE_SEARCH_READY, 0, 0) };
                    }
                };
                match encode() {
                    Ok(png) => {
                        reverse_image::search_png(&engines, &png, |engine, result| {
                            let retry = (png.len() <= 20 * 1024 * 1024).then(|| Arc::clone(&png));
                            report(engine, result, retry);
                        });
                    }
                    Err(error) => {
                        for engine in engines {
                            report(engine, Err(io::Error::other(error.to_string())), None);
                        }
                    }
                }
            })?;
        *slot = Some(Upload {
            destination,
            result,
            remaining: count,
            failures: Vec::new(),
            retry: None,
        });
        Ok(())
    }

    pub(in crate::native) fn finish_image_search(&self) {
        let Some(mut upload) = self.screenshot.image_upload.borrow_mut().take() else {
            return;
        };
        while let Ok((engine, result, png)) = upload.result.try_recv() {
            upload.remaining -= 1;
            if matches!(upload.destination, Destination::Dismissed) {
                continue;
            }
            let result = result.and_then(|url| {
                if self.app.borrow_mut().open_link(&url)? {
                    Ok(())
                } else {
                    Err(io::Error::other("Invalid image search result URL"))
                }
            });
            match result {
                Ok(()) => {
                    self.diagnostics.sample("image_search_result_opened");
                    if let Destination::Selection(owner) = upload.destination {
                        upload.destination = Destination::Browser;
                        if self.screenshot.overlay.get() == owner {
                            self.end_screenshot(false);
                        }
                    }
                }
                Err(error) => {
                    self.diagnostics.sample("image_search_result_failed");
                    upload.record_failure(engine, error, png);
                }
            }
        }
        if upload.remaining != 0 {
            *self.screenshot.image_upload.borrow_mut() = Some(upload);
        } else if let Destination::Selection(owner) = upload.destination
            && self.screenshot.overlay.get() == owner
        {
            let english = {
                let mut slot = self.screenshot.selection.borrow_mut();
                let Some(selection) = slot.as_mut() else {
                    return;
                };
                selection.uploading = false;
                selection.status = if selection.english {
                    "Search failed · Enter to retry"
                } else {
                    "搜图失败 · Enter 重试"
                };
                selection.sync_windows();
                selection.windows.invalidate(Surface::Badge, None);
                selection.english
            };
            show_error(owner, english, io::Error::other(upload.failures.join("\n")));
        } else if matches!(upload.destination, Destination::Browser) && !upload.failures.is_empty()
        {
            *self.screenshot.image_retry.borrow_mut() = upload.retry;
            self.notify_image_search_failure(
                &upload.failures.join("\n"),
                self.can_retry_image_search(),
            );
        }
    }
}

fn show_error(hwnd: HWND, english: bool, error: io::Error) {
    let title = if english {
        "Could not search this screenshot"
    } else {
        "截图搜图失败"
    };
    message(hwnd, &format!("{title}\n{error}"));
}

fn choose_engines(
    hwnd: HWND,
    button: RECT,
    english: bool,
) -> io::Result<Option<&'static [Engine]>> {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return Err(io::Error::last_os_error());
        }
        let all = if english {
            w!("Search both engines")
        } else {
            w!("同时搜图")
        };
        let append = |id, label| AppendMenuW(menu, MF_STRING, id, label) != 0;
        if !append(1, all)
            || AppendMenuW(menu, MF_SEPARATOR, 0, null()) == 0
            || !ENGINES.iter().enumerate().all(|(index, engine)| {
                append(index + 2, super::super::wide(engine.name()).as_ptr())
            })
        {
            let error = io::Error::last_os_error();
            DestroyMenu(menu);
            return Err(error);
        }
        let choice = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            button.left,
            button.bottom,
            0,
            hwnd,
            null(),
        );
        DestroyMenu(menu);
        Ok(match choice {
            1 => Some(&ENGINES),
            _ => ENGINES
                .get(choice.wrapping_sub(2) as usize)
                .map(std::slice::from_ref),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_routes_to_the_configured_image_search_engines() {
        assert!(configured_engines(ImageSearchAction::Choose).is_none());
        assert!(matches!(
            configured_engines(ImageSearchAction::Both),
            Some([Engine::Yandex, Engine::GoogleLens])
        ));
        assert!(matches!(
            configured_engines(ImageSearchAction::Yandex),
            Some([Engine::Yandex])
        ));
        assert!(matches!(
            configured_engines(ImageSearchAction::GoogleLens),
            Some([Engine::GoogleLens])
        ));
    }

    #[test]
    fn partial_failure_reuses_the_image_for_only_the_failed_provider() {
        let mut upload = Upload {
            destination: Destination::Browser,
            result: mpsc::channel().1,
            remaining: 0,
            failures: Vec::new(),
            retry: None,
        };
        let png = Arc::new(vec![1, 2, 3]);
        upload.record_failure(
            Engine::GoogleLens,
            io::Error::other("timeout"),
            Some(png.clone()),
        );
        let retry = upload.retry.unwrap();
        assert!(matches!(retry.engines.as_slice(), [Engine::GoogleLens]));
        assert!(Arc::ptr_eq(&retry.png, &png));
        assert_eq!(upload.failures, ["Google Lens · timeout"]);
    }
}
