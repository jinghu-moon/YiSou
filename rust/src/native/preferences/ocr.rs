use super::{components::Page, pages::shortcut, *};
use std::sync::mpsc::{self, Receiver};
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND as WinHwnd},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
            SIGDN_FILESYSPATH,
        },
    },
    core::{HRESULT, PCWSTR},
};
use yisou::{
    ocr::{Model, TINY_DOWNLOADS},
    settings::Preferences,
};

const DIRECTORY: usize = 40;
pub(super) const STATUS: i32 = 177;
pub(super) const TIMER: usize = 16;
pub(super) const TOGGLE: usize = 170;

#[derive(Default)]
pub(super) struct Check {
    pending: Option<(PathBuf, bool, Receiver<Result<(), String>>)>,
    draft: Option<String>,
    status: String,
    invalid: bool,
}

pub(super) fn display(page: Page, prefs: &Preferences) {
    let hwnd = page.0;
    page.section("截图文字搜索", 98, 156, &[]);
    page.setting(
        "启用 OCR 搜索",
        "启用前需要配置并验证本地 OCR 模型。",
        98,
        70,
    );
    page.toggle(TOGGLE, "启用 OCR 搜索", prefs.ocr_enabled, 98);
    page.section("本地模型", 200, 377, &[292]);
    let (path, label) = PANEL.with_borrow(|p| {
        (
            p.ocr_check.draft.clone().unwrap_or_else(|| {
                prefs
                    .ocr_model_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            }),
            if p.ocr_check.status.is_empty() {
                tr_for_status(prefs)
            } else {
                p.ocr_check.status.clone()
            },
        )
    });
    caption(
        hwnd,
        "模型文件夹",
        place(14, 208, 508, 22, STRETCH),
        fluent::FontRole::Body,
    );
    let edit = edit_at(hwnd, DIRECTORY, &path, place(22, 242, 302, 22, STRETCH));
    cue(edit, "本地模型目录");
    action(hwnd, 171, "浏览…", place(340, 235, 85, 34, RIGHT));
    action(hwnd, 172, "应用", place(435, 235, 87, 34, RIGHT));
    let status = page.detail(&label, place(14, 269, 508, 22, STRETCH));
    unsafe { SetWindowLongPtrW(status, GWLP_ID, STATUS as isize) };
    page.body(
        "ModelScope 下载 · PP-OCRv6 tiny",
        place(14, 299, 508, 22, STRETCH),
    );
    page.detail(
        "将检测模型、识别模型和字典放在同一个目录。",
        place(14, 321, 508, 19, STRETCH),
    );
    for (id, name, x) in [
        (173, "检测模型", 14),
        (174, "识别模型", 121),
        (175, "字典", 228),
    ] {
        action(hwnd, id, name, place(x, 344, 100, 27, 0));
    }
    page.section("快捷键", 421, 479, &[]);
    shortcut(
        page,
        156,
        157,
        39,
        "OCR 快捷键",
        "",
        &prefs.ocr_shortcut.map(format_shortcut).unwrap_or_default(),
        421,
    );
    page.finish_at(505);
    sync_state(hwnd);
}

fn tr_for_status(prefs: &Preferences) -> String {
    if prefs.ocr_enabled {
        "模型可用"
    } else if prefs.ocr_model_dir.is_some() {
        "待检查"
    } else {
        "待配置"
    }
    .to_owned()
}

pub(super) fn is_error() -> bool {
    PANEL.with_borrow(|p| p.ocr_check.invalid)
}

fn sync_state(hwnd: HWND) {
    let pending = PANEL.with_borrow(|p| p.ocr_check.pending.is_some());
    unsafe {
        for id in [TOGGLE as i32, DIRECTORY as i32, 171, 172] {
            EnableWindow(GetDlgItem(hwnd, id), i32::from(!pending));
        }
        InvalidateRect(GetDlgItem(hwnd, STATUS), null(), 0);
    }
}

pub(super) fn change_path(hwnd: HWND) {
    let value = field(hwnd, DIRECTORY);
    let changed = PANEL.with_borrow_mut(|p| {
        if p.ocr_check.draft.as_ref() == Some(&value) {
            return false;
        }
        p.ocr_check.draft = Some(value);
        p.ocr_check.status = "待检查".into();
        p.ocr_check.invalid = false;
        true
    });
    if changed {
        set_label(hwnd, STATUS, "待检查");
    }
}

fn set_status(hwnd: HWND, status: String, invalid: bool) {
    PANEL.with_borrow_mut(|p| {
        p.ocr_check.status = status.clone();
        p.ocr_check.invalid = invalid;
    });
    if PANEL.with_borrow(|p| p.page == OCR_PAGE) {
        set_label(hwnd, STATUS, &status);
        sync_state(hwnd);
    }
}

pub(super) fn execute(state: &State, hwnd: HWND, id: usize) -> Result<bool, String> {
    if let 173..=175 = id {
        state
            .app
            .borrow_mut()
            .open_link(TINY_DOWNLOADS[id - 173])
            .map_err(|error| error.to_string())?;
        return Ok(false);
    }
    if PANEL.with_borrow(|p| p.ocr_check.pending.is_some()) {
        return Ok(false);
    }
    if id == 171 {
        if let Some(path) = choose_folder(hwnd).map_err(|error| error.to_string())? {
            set_field(hwnd, DIRECTORY as i32, &path.display().to_string());
            focus_control(hwnd, 172);
        }
        return Ok(false);
    }

    let mut settings = state.app.borrow().settings().clone();
    let enabled = settings.preferences.ocr_enabled;
    if id == TOGGLE && enabled {
        settings.preferences.ocr_enabled = false;
        apply_hotkeys(state, settings)?;
        set_field(
            hwnd,
            TOGGLE as i32,
            &components::toggle_label("启用 OCR 搜索", false),
        );
        return Ok(false);
    }
    let draft = field(hwnd, DIRECTORY);
    if draft.trim().is_empty() {
        if id == TOGGLE {
            set_status(hwnd, "请选择 OCR 模型目录".into(), true);
            focus_control(hwnd, DIRECTORY as i32);
        } else {
            settings.preferences.ocr_model_dir = None;
            settings.preferences.ocr_enabled = false;
            apply_hotkeys(state, settings)?;
            set_field(
                hwnd,
                TOGGLE as i32,
                &components::toggle_label("启用 OCR 搜索", false),
            );
            set_status(hwnd, "待配置".into(), false);
        }
        return Ok(false);
    }

    let path = PathBuf::from(draft.trim());
    let directory = state.ocr_directory(&path);
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("ocr-model-check".into())
        .spawn(move || {
            let _ = sender.send(
                Model::Tiny
                    .validate_directory(&directory)
                    .map_err(|error| error.to_string()),
            );
        })
        .map_err(|error| error.to_string())?;
    PANEL.with_borrow_mut(|p| p.ocr_check.pending = Some((path, id == TOGGLE, receiver)));
    set_status(hwnd, "正在检查模型…".into(), false);
    unsafe {
        SetTimer(hwnd, TIMER, 45, None);
        SetFocus(GetDlgItem(hwnd, NAV as i32));
    }
    Ok(false)
}

pub(super) fn poll(state: &State, hwnd: HWND) {
    use std::sync::mpsc::TryRecvError;
    let done = PANEL.with_borrow_mut(|p| {
        let pending = p.ocr_check.pending.as_ref()?;
        let result = match pending.2.try_recv() {
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("OCR 模型检查中断".into()),
            Ok(result) => result,
        };
        let (path, enable, _) = p.ocr_check.pending.take().unwrap();
        Some((path, enable, result))
    });
    let Some((path, enable, result)) = done else {
        return;
    };
    unsafe {
        KillTimer(hwnd, TIMER);
    }
    let result = result.and_then(|()| {
        let mut settings = state.app.borrow().settings().clone();
        settings.preferences.ocr_model_dir = Some(path);
        settings.preferences.ocr_enabled |= enable;
        apply_settings_with_checked_model(state, settings)
    });
    match result {
        Ok(()) => set_status(hwnd, "模型可用".into(), false),
        Err(error) => {
            let app = state.app.borrow();
            let active = app.settings().preferences.active_ocr_model_dir();
            set_status(
                hwnd,
                active.map_or_else(
                    || error.clone(),
                    |path| format!("{error} · 当前使用 {}", path.display()),
                ),
                true,
            );
        }
    }
    if PANEL.with_borrow(|p| p.page == OCR_PAGE) {
        let enabled = state.app.borrow().settings().preferences.ocr_enabled;
        set_field(
            hwnd,
            TOGGLE as i32,
            &components::toggle_label("启用 OCR 搜索", enabled),
        );
    }
}

fn choose_folder(hwnd: HWND) -> windows::core::Result<Option<PathBuf>> {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _apartment = Apartment;
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(PCWSTR(wide(tr("请选择 OCR 模型目录")).as_ptr()))?;
        if let Err(error) = dialog.Show(Some(WinHwnd(hwnd))) {
            return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0.cast()));
        Ok(Some(value.into()))
    }
}
