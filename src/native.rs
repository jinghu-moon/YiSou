mod backdrop;
mod composition_queue;
mod diagnostics;
mod edit_paint;
mod font;
mod paint;
mod popup;
mod preferences;
mod screen_shade;
mod screenshot;
mod search_text;
mod search_window;
mod selection;
mod smooth_corner;
#[cfg(feature = "text-render-lab")]
mod text_render_lab;
mod tray;

use self::paint::Painter;
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
    sync::OnceLock,
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::DwmSetWindowAttribute, Gdi::HFONT},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
                GetSystemMetricsForDpi, SetProcessDpiAwarenessContext,
            },
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
const TITLE: *const u16 = w!("易搜");
const APP_ICON: *const u16 = 101 as *const u16; // Icon resource ID in app.rc.
const REVEAL: u32 = WM_APP + 1;

struct State {
    diagnostics: diagnostics::MemoryTrace,
    app: RefCell<App>,
    config: PathBuf,
    data: PathBuf,
    window: Cell<HWND>,
    screen_shade: RefCell<Option<screen_shade::ScreenShade>>,
    system_dark: Cell<bool>,
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
    engine_button_width: Cell<i32>,
    search_edit_height: Cell<i32>,
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
    browser_failure: Cell<Option<search_window::BrowserFailure>>,
    focus_popup: Cell<bool>,
    launch_focus: Cell<HWND>,
    selection: SelectionTasks,
    screenshot: screenshot::ScreenshotTasks,
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
    composition_device: RefCell<Option<backdrop::CompositionDevice>>,
    backdrop_material: Cell<Option<yisou::settings::SearchMaterial>>,
    images: RefCell<IconCache>,
    font: Cell<HFONT>,
    file_font: RefCell<Option<font::FileFont>>,
}

thread_local! { static SESSION: RefCell<Option<Rc<State>>> = const { RefCell::new(None) }; }

fn session() -> Option<Rc<State>> {
    SESSION.with_borrow(Clone::clone)
}

fn personalization_flag(name: *const u16, default: bool) -> bool {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    };

    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            name,
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    if result == ERROR_SUCCESS {
        value != 0
    } else {
        default
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn write_clipboard(hwnd: HWND, text: &str) -> bool {
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
    };

    let text = wide(text);
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, text.len() * size_of::<u16>());
        if memory.is_null() {
            return false;
        }
        let target = GlobalLock(memory).cast::<u16>();
        let mut copied = false;
        if !target.is_null() {
            std::ptr::copy_nonoverlapping(text.as_ptr(), target, text.len());
            GlobalUnlock(memory);
            if OpenClipboard(hwnd) != 0 {
                copied = EmptyClipboard() != 0
                    && !SetClipboardData(u32::from(CF_UNICODETEXT), memory).is_null();
                CloseClipboard();
            }
        }
        if !copied {
            GlobalFree(memory);
        }
        copied
    }
}

fn dip(value: i32, dpi: u32) -> i32 {
    (value * dpi as i32 + 48) / 96
}

fn set_window_shape(hwnd: HWND, region: windows_sys::Win32::Graphics::Gdi::HRGN) {
    use windows_sys::Win32::Graphics::{
        Dwm::{DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE},
        Gdi::{DeleteObject, SetWindowRgn},
    };
    let do_not_round = 1u32;
    unsafe {
        for (attribute, value) in [
            (DWMWA_WINDOW_CORNER_PREFERENCE, do_not_round),
            (DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE),
        ] {
            DwmSetWindowAttribute(hwnd, attribute as u32, (&value as *const u32).cast(), 4);
        }
        if SetWindowRgn(hwnd, region, 1) == 0 && !region.is_null() {
            DeleteObject(region);
        }
    }
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

fn app_icon(small: bool) -> HICON {
    static SMALL: OnceLock<usize> = OnceLock::new();
    static LARGE: OnceLock<usize> = OnceLock::new();
    let (icon, width, height) = if small {
        (&SMALL, SM_CXSMICON, SM_CYSMICON)
    } else {
        (&LARGE, SM_CXICON, SM_CYICON)
    };
    // Keep both sizes alive for the registered window classes.
    *icon.get_or_init(|| unsafe {
        let dpi = GetDpiForSystem();
        LoadImageW(
            instance(),
            APP_ICON,
            IMAGE_ICON,
            GetSystemMetricsForDpi(width, dpi),
            GetSystemMetricsForDpi(height, dpi),
            0,
        ) as usize
    }) as HICON
}

fn register(class: *const u16, proc: WNDPROC) -> io::Result<()> {
    let definition = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: proc,
        lpszClassName: class,
        hInstance: instance(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hIcon: app_icon(false),
        hIconSm: app_icon(true),
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&definition) } == 0 {
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
    screenshot::register()?;
    preferences::register_window()?;
    let state = Rc::new(State {
        diagnostics,
        app: RefCell::new(app),
        config,
        data,
        window: Cell::default(),
        screen_shade: RefCell::default(),
        system_dark: Cell::new(!personalization_flag(w!("AppsUseLightTheme"), true)),
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
        engine_button_width: Cell::default(),
        search_edit_height: Cell::default(),
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
        browser_failure: Cell::new(None),
        focus_popup: Cell::new(false),
        launch_focus: Cell::default(),
        selection: SelectionTasks::default(),
        screenshot: screenshot::ScreenshotTasks::default(),
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
        composition_device: RefCell::default(),
        backdrop_material: Cell::default(),
        images: RefCell::default(),
        font: Cell::default(),
        file_font: RefCell::default(),
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
            image_search: prefs.image_search_shortcut,
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
