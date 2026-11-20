mod backdrop;
mod diagnostics;
mod edit_paint;
mod font;
#[cfg(feature = "ocr")]
mod ocr;
mod paint;
mod popup;
mod preferences;
mod screen_shade;
mod search_text;
mod search_window;
mod selection;
#[cfg(feature = "text-render-lab")]
mod text_render_lab;
mod tray;

use self::paint::{Painter, SURFACE};
use self::{
    popup::{Popup, popup_proc},
    search_window::{MAIN_CLASS, main_proc},
    selection::{DOUBLE_ALT, SelectionTasks},
};
use std::{
    cell::{Cell, RefCell},
    io,
    path::PathBuf,
    process::ExitCode,
    ptr::{null, null_mut},
    rc::Rc,
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::{
            Dwm::DwmSetWindowAttribute,
            Gdi::{CreateSolidBrush, HBRUSH, HFONT},
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::*,
        },
    },
    w,
};
use yisou::{
    application::{Application, History, Query},
    icons::IconCache,
    launch::{Launch, LaunchMode, WindowAction},
    platform::{self, HotkeySet, Instance, KeyBindings, WindowsBrowser},
    settings::Settings,
    storage::JsonSettings,
};

type App = Application<JsonSettings, WindowsBrowser>;
const TITLE: *const u16 = w!("易搜 · Rust 原生版");
const REVEAL: u32 = WM_APP + 1;

struct State {
    diagnostics: diagnostics::MemoryTrace,
    app: RefCell<App>,
    config: PathBuf,
    data: PathBuf,
    window: Cell<HWND>,
    screen_shade: Cell<HWND>,
    edit: Cell<HWND>,
    engine_button: Cell<HWND>,
    engine_tooltip: Cell<HWND>,
    displayed_engine: Cell<Option<usize>>,
    engine_hover: Cell<bool>,
    #[cfg(feature = "text-render-lab")]
    text_sample: Cell<Option<text_render_lab::Sample>>,
    #[cfg(feature = "text-render-lab")]
    text_receipt: Cell<usize>,
    layout_dpi: Cell<u32>,
    hint: Cell<&'static str>,
    popup_window: Cell<HWND>,
    settings_window: Cell<HWND>,
    dpi: Cell<u32>,
    composing: Cell<bool>,
    preedit: RefCell<edit_paint::Preedit>,
    capture_dc: Cell<windows_sys::Win32::Graphics::Gdi::HDC>,
    composing_controls: Cell<bool>,
    edit_dirty: Cell<bool>,
    search_text_dirty: Cell<bool>,
    caret_on: Cell<bool>,
    native_caret_hidden: Cell<bool>,
    changing_input: Cell<bool>,
    error: Cell<bool>,
    focus_popup: Cell<bool>,
    launch_focus: Cell<HWND>,
    selection: SelectionTasks,
    #[cfg(feature = "ocr")]
    ocr: ocr::OcrTasks,
    #[cfg(feature = "ocr")]
    ocr_draft: Cell<bool>,
    hotkeys: RefCell<Option<HotkeySet>>,
    taskbar_created: u32,
    tray: Cell<bool>,
    positions: RefCell<std::collections::HashMap<usize, (i32, i32)>>,
    temporary: RefCell<Option<String>>,
    retry: RefCell<Option<Query>>,
    popup: RefCell<Option<Popup>>,
    painter: RefCell<Option<Painter>>,
    backdrop: RefCell<Option<backdrop::Backdrop>>,
    backdrop_unavailable: Cell<bool>,
    images: RefCell<IconCache>,
    font: Cell<HFONT>,
    file_font: RefCell<Option<font::FileFont>>,
    background: HBRUSH,
}

thread_local! { static SESSION: RefCell<Option<Rc<State>>> = const { RefCell::new(None) }; }

fn session() -> Option<Rc<State>> {
    SESSION.with_borrow(Clone::clone)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn dip(value: i32, dpi: u32) -> i32 {
    (value * dpi as i32 + 48) / 96
}

fn color_ref(rgb: u32) -> COLORREF {
    ((rgb >> 16) & 255) | (rgb & 0x00ff00) | ((rgb & 255) << 16)
}

fn text_of(hwnd: HWND) -> String {
    unsafe {
        let mut text = vec![0; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&text[..length])
    }
}

fn message(hwnd: HWND, text: &str) {
    unsafe { MessageBoxW(hwnd, wide(text).as_ptr(), TITLE, MB_OK | MB_ICONWARNING) };
}

fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

fn register(class: *const u16, proc: WNDPROC) -> io::Result<()> {
    let definition = WNDCLASSW {
        lpfnWndProc: proc,
        lpszClassName: class,
        hInstance: instance(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hIcon: unsafe { LoadIconW(null_mut(), IDI_APPLICATION) },
        ..Default::default()
    };
    if unsafe { RegisterClassW(&definition) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn run() -> ExitCode {
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            message(null_mut(), &error.to_string());
            ExitCode::FAILURE
        }
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    let launch = Launch::parse(std::env::args_os().skip(1))?;
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let Some(_instance) = Instance::acquire()? else {
        let target = unsafe { FindWindowW(MAIN_CLASS, null()) };
        let action = match launch.mode {
            LaunchMode::Startup => return Ok(()),
            LaunchMode::Settings => 1,
            LaunchMode::Configuration => 2,
            LaunchMode::IconPreview => 3,
            LaunchMode::Manual => 0,
        };
        if !target.is_null() {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(target, &mut pid);
                AllowSetForegroundWindow(pid);
                PostMessageW(target, REVEAL, action, 0);
            }
        }
        return Ok(());
    };
    let diagnostics = diagnostics::MemoryTrace::open();
    diagnostics.sample("startup");
    let data = platform::data_directory()?;
    let config = platform::settings_path(launch.config.clone(), &data, &std::env::current_exe()?)?;
    let store = JsonSettings::new(config.clone());
    let settings = store.load()?;
    let action = launch.action(&settings.preferences, false);
    let app = Application::new(
        settings,
        store,
        WindowsBrowser,
        History::new(data.join("history.json")),
    )?;
    register(MAIN_CLASS, Some(main_proc))?;
    register(screen_shade::CLASS, Some(screen_shade::proc))?;
    register(popup::CLASS, Some(popup_proc))?;
    #[cfg(feature = "ocr")]
    ocr::register()?;
    preferences::register_window()?;
    let state = Rc::new(State {
        diagnostics,
        app: RefCell::new(app),
        config,
        data,
        window: Cell::default(),
        screen_shade: Cell::default(),
        edit: Cell::default(),
        engine_button: Cell::default(),
        engine_tooltip: Cell::default(),
        displayed_engine: Cell::default(),
        engine_hover: Cell::default(),
        #[cfg(feature = "text-render-lab")]
        text_sample: Cell::default(),
        #[cfg(feature = "text-render-lab")]
        text_receipt: Cell::default(),
        layout_dpi: Cell::default(),
        hint: Cell::default(),
        popup_window: Cell::default(),
        settings_window: Cell::default(),
        dpi: Cell::new(96),
        composing: Cell::new(false),
        preedit: RefCell::default(),
        capture_dc: Cell::default(),
        composing_controls: Cell::default(),
        edit_dirty: Cell::new(true),
        search_text_dirty: Cell::new(true),
        caret_on: Cell::default(),
        native_caret_hidden: Cell::default(),
        changing_input: Cell::new(false),
        error: Cell::new(false),
        focus_popup: Cell::new(false),
        launch_focus: Cell::default(),
        selection: SelectionTasks::default(),
        #[cfg(feature = "ocr")]
        ocr: ocr::OcrTasks::default(),
        #[cfg(feature = "ocr")]
        ocr_draft: Cell::default(),
        hotkeys: RefCell::default(),
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        tray: Cell::new(false),
        positions: RefCell::default(),
        temporary: RefCell::default(),
        retry: RefCell::default(),
        popup: RefCell::default(),
        painter: RefCell::default(),
        backdrop: RefCell::default(),
        backdrop_unavailable: Cell::default(),
        images: RefCell::default(),
        font: Cell::default(),
        file_font: RefCell::default(),
        background: unsafe { CreateSolidBrush(color_ref(SURFACE)) },
    });
    SESSION.with_borrow_mut(|slot| *slot = Some(state.clone()));
    let result = run_windows(&state, action);
    SESSION.with_borrow_mut(Option::take);
    result
}

fn run_windows(state: &State, action: WindowAction) -> Result<(), Box<dyn std::error::Error>> {
    let hwnd = state.create_search_window()?;
    state.reload_file_font();
    state.diagnostics.sample("resident_window_created");
    state.add_tray();
    *state.hotkeys.borrow_mut() = Some(unsafe { HotkeySet::new(hwnd, DOUBLE_ALT) });
    let bindings = State::bindings(state.app.borrow().settings());
    if let Err(error) = state.bind_hotkeys(bindings) {
        #[cfg(feature = "ocr")]
        let error = if bindings.ocr.is_some() {
            state
                .bind_hotkeys(KeyBindings {
                    ocr: None,
                    ..bindings
                })
                .err()
        } else {
            Some(error)
        };
        #[cfg(not(feature = "ocr"))]
        let error = Some(error);
        if let Some(error) = error {
            message(hwnd, &format!("无法注册快捷键\n{error}"));
        }
    }
    state.diagnostics.sample("resident_ready");
    match action {
        WindowAction::ShowSettings => preferences::show(state, 0),
        WindowAction::ShowConfiguration => preferences::show(state, 6),
        WindowAction::ShowIconPreview => {
            state.show();
            state.toggle_engine_popup();
        }
        WindowAction::ShowSearch => state.show(),
        WindowAction::StayInTray if !state.tray.get() => state.show(),
        _ => {}
    }
    state.message_loop()?;
    state.diagnostics.sample("message_loop_exit");
    Ok(())
}

impl State {
    fn bindings(settings: &Settings) -> KeyBindings {
        let prefs = &settings.preferences;
        KeyBindings {
            main: prefs.main_shortcut,
            selection: prefs.selection.as_ref().map(|s| s.shortcut),
            clipboard: prefs.clipboard_shortcut,
            #[cfg(feature = "ocr")]
            ocr: prefs.active_ocr_model_dir().and(prefs.ocr_shortcut),
        }
    }

    fn bind_hotkeys(&self, bindings: KeyBindings) -> io::Result<()> {
        self.hotkeys
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| io::Error::other("hotkeys unavailable"))?
            .apply(bindings)
    }

    fn message_loop(&self) -> io::Result<()> {
        let mut msg = MSG::default();
        loop {
            match unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } {
                -1 => return Err(io::Error::last_os_error()),
                0 => return Ok(()),
                _ => {
                    let preferences = self.settings_window.get();
                    if !preferences.is_null() && self::preferences::capture_key(preferences, &msg) {
                        continue;
                    }
                    if !preferences.is_null() && unsafe { IsDialogMessageW(preferences, &msg) } != 0
                    {
                        continue;
                    }
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
    }

    fn dwm(&self, hwnd: HWND, corner: i32) {
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                33,
                (&corner as *const i32).cast(),
                size_of::<i32>() as u32,
            )
        };
    }

    fn scale(&self, value: i32) -> i32 {
        dip(value, self.dpi.get())
    }
}
