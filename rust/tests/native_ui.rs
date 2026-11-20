#![cfg(windows)]

use std::{
    io::Read,
    process::{Child, Command, Stdio},
    ptr::null,
    thread::sleep,
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{InvalidateRect, UpdateWindow},
        UI::{
            Controls::{COMBOBOXINFO, EM_CHARFROMPOS, EM_GETSEL, EM_SETSEL, GetComboBoxInfo},
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
                SetThreadDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::{
                IsWindowEnabled, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_RETURN, VK_TAB,
            },
            WindowsAndMessaging::*,
        },
    },
    w,
};
use yisou::storage::JsonSettings;

struct AppProcess(Child);

impl Drop for AppProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn before_timeout<T>(step: &str, mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "timed out: {step}");
        sleep(Duration::from_millis(20));
    }
}

fn visible_window(class: *const u16, pid: u32) -> Option<HWND> {
    window_for_process(class, pid).filter(|&hwnd| unsafe { IsWindowVisible(hwnd) } != 0)
}

fn window_for_process(class: *const u16, pid: u32) -> Option<HWND> {
    unsafe {
        let window = FindWindowW(class, null());
        let mut process = 0;
        if !window.is_null() {
            GetWindowThreadProcessId(window, &mut process);
        }
        (process == pid).then_some(window)
    }
}

fn height(hwnd: HWND) -> i32 {
    let mut rect = RECT::default();
    assert_ne!(unsafe { GetClientRect(hwnd, &mut rect) }, 0);
    rect.bottom - rect.top
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    assert_ne!(unsafe { GetWindowRect(hwnd, &mut rect) }, 0);
    rect
}

fn send(window: HWND, msg: u32, wp: usize, lp: isize) -> usize {
    assert!(!window.is_null());
    let mut result = 0;
    assert_ne!(
        unsafe { SendMessageTimeoutW(window, msg, wp, lp, SMTO_ABORTIFHUNG, 1500, &mut result) },
        0,
        "window did not process message {msg:#x}"
    );
    result
}

#[test]
fn native_search_popup_preferences_and_second_launch() {
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    if !cfg!(feature = "ocr") {
        let mut shared = yisou::settings::Settings::default();
        shared.preferences.ocr_enabled = true;
        shared.preferences.ocr_model_dir = Some("external-models".into());
        std::fs::write(&config, serde_json::to_vec(&shared).unwrap()).unwrap();
    }
    let trace = data.path().join("memory.csv");
    let history_dir = data.path().join("YiSouRust");
    std::fs::create_dir(&history_dir).unwrap();
    let history = [
        ("Rust compiler", "Terminal"),
        ("Rust manual", "Browser"),
        ("Windows manual", "Terminal"),
    ];
    let history: Vec<_> = history
        .into_iter()
        .enumerate()
        .map(|(id, (query, source))| {
            serde_json::json!({
                "id": id + 1, "timestamp_ms": 1, "query": query, "engine_id": "bing",
                "engine_name": "Bing", "origin": "typed", "source": source,
            })
        })
        .collect();
    std::fs::write(
        history_dir.join("history.json"),
        serde_json::to_vec(&history).unwrap(),
    )
    .unwrap();
    let executable =
        std::env::var_os("YISOU_TEST_EXE").unwrap_or_else(|| env!("CARGO_BIN_EXE_YiSou").into());
    let launch = |args: &[&str]| {
        Command::new(&executable)
            .env("LOCALAPPDATA", data.path())
            .env("YISOU_MEMORY_TRACE", &trace)
            .env("RUST_BACKTRACE", "1")
            .stderr(Stdio::piped())
            .arg("--config")
            .arg(&config)
            .args(args)
            .spawn()
            .map(AppProcess)
            .unwrap()
    };
    let mut resident = launch(&["--startup"]);
    let pid = resident.0.id();
    let search_class = w!("YiSou.Rust.Search");
    let popup_class = w!("YiSou.Rust.Engines");
    let shade_class = w!("YiSou.Rust.ScreenShade");
    let search = before_timeout("resident window", || window_for_process(search_class, pid));
    send(search, WM_NULL, 0, 0);
    assert!(visible_window(search_class, pid).is_none());
    assert!(unsafe { GetDlgItem(search, 100) }.is_null());
    let mut first_reveal = launch(&[]);
    assert_eq!(
        before_timeout("first reveal", || visible_window(search_class, pid)),
        search
    );
    assert!(before_timeout("first reveal exits", || first_reveal.0.try_wait().unwrap()).success());
    let shade = before_timeout("screen shade", || visible_window(shade_class, pid));
    let mut shade_bounds = RECT::default();
    assert_ne!(unsafe { GetWindowRect(shade, &mut shade_bounds) }, 0);
    unsafe {
        assert_eq!(shade_bounds.left, GetSystemMetrics(SM_XVIRTUALSCREEN));
        assert_eq!(shade_bounds.top, GetSystemMetrics(SM_YVIRTUALSCREEN));
        assert_eq!(
            shade_bounds.right - shade_bounds.left,
            GetSystemMetrics(SM_CXVIRTUALSCREEN)
        );
        assert_eq!(
            shade_bounds.bottom - shade_bounds.top,
            GetSystemMetrics(SM_CYVIRTUALSCREEN)
        );
    }
    assert_eq!(unsafe { GetWindow(search, GW_HWNDNEXT) }, shade);
    let edit = unsafe { GetDlgItem(search, 100) };
    let engine = unsafe { GetDlgItem(search, 101) };
    let button = window_rect(engine);
    let input = window_rect(edit);
    let dpi = unsafe { GetDpiForWindow(search) } as i32;
    let px = |dip| (dip * dpi + 48) / 96;
    assert_eq!(button.right - button.left, px(40));
    assert_eq!(button.bottom - button.top, px(40));
    assert_eq!(input.right - input.left, px(516) - px(20));
    assert!(input.right < button.left);
    let tooltip = window_for_process(w!("tooltips_class32"), pid);
    assert!(tooltip.is_some_and(|tip| unsafe { GetWindow(tip, GW_OWNER) } == search));

    let default_height = height(search);
    let paint = || unsafe {
        InvalidateRect(search, null(), 0);
        UpdateWindow(search);
    };
    let text_routes = || {
        std::fs::read_to_string(&trace)
            .unwrap()
            .lines()
            .filter_map(|row| row.split(',').nth(2))
            .filter(|event| event.starts_with("search_text_"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    paint();
    let acrylic = std::fs::read_to_string(&trace)
        .unwrap()
        .contains(",backdrop_acrylic,");
    if std::env::var_os("YISOU_TEST_REQUIRE_ACRYLIC").is_some() {
        assert!(acrylic, "test desktop must support Acrylic");
    }
    let initial_routes = text_routes();
    if acrylic {
        assert_eq!(
            initial_routes.last().map(String::as_str),
            Some("search_text_direct")
        );
    }
    send(edit, WM_SETTEXT, 0, w!("https://example.com/") as isize);
    assert!(height(search) > default_height);
    send(edit, WM_SETTEXT, 0, w!("普通文本") as isize);
    assert_eq!(height(search), default_height);
    send(edit, EM_SETSEL, 1, 3);
    assert_eq!(send(edit, EM_GETSEL, 0, 0), 0x0003_0001);
    send(edit, WM_CHAR, usize::from(b'A'), 0);
    assert_eq!(send(edit, WM_GETTEXTLENGTH, 0, 0), 3);
    let long = "鬱證與 Wiii 鑿龜 🧭 ".repeat(16);
    let length = long.encode_utf16().count();
    let long: Vec<_> = long.encode_utf16().chain([0]).collect();
    assert_ne!(send(edit, WM_SETTEXT, 0, long.as_ptr() as isize), 0);
    send(edit, WM_KEYDOWN, VK_END as usize, 0);
    paint();
    assert_eq!(send(edit, EM_GETSEL, 0, 0), length | (length << 16));
    let left_edge = (px(12) as isize) << 16;
    assert!(send(edit, EM_CHARFROMPOS, 0, left_edge) & 0xffff > 0);
    send(edit, WM_KEYDOWN, VK_HOME as usize, 0);
    paint();
    assert_eq!(send(edit, EM_GETSEL, 0, 0), 0);
    assert_eq!(send(edit, EM_CHARFROMPOS, 0, left_edge) & 0xffff, 0);
    send(edit, EM_SETSEL, 6, 1);
    paint();
    assert_eq!(send(edit, EM_GETSEL, 0, 0), 0x0006_0001);
    if acrylic {
        assert_eq!(text_routes(), initial_routes);
        send(edit, WM_IME_STARTCOMPOSITION, 0, 0);
        paint();
        assert_eq!(
            text_routes().last().map(String::as_str),
            Some("search_text_fallback")
        );
        send(edit, WM_IME_ENDCOMPOSITION, 0, 0);
        paint();
        assert_eq!(
            text_routes().last().map(String::as_str),
            Some("search_text_direct")
        );
    }
    send(edit, WM_SETTEXT, 0, w!("") as isize);
    send(engine, BM_CLICK, 0, 0);
    before_timeout("engine popup", || visible_window(popup_class, pid));
    send(edit, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert!(visible_window(popup_class, pid).is_none());
    send(edit, WM_KEYDOWN, usize::from(VK_TAB), 0);
    let mut gui = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let thread = GetWindowThreadProcessId(search, std::ptr::null_mut());
        assert_ne!(GetGUIThreadInfo(thread, &mut gui), 0);
    }
    assert_eq!(gui.hwndFocus, engine);
    send(engine, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    before_timeout("engine popup from keyboard", || {
        visible_window(popup_class, pid)
    });
    send(engine, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert!(visible_window(popup_class, pid).is_none());
    send(edit, WM_SETTEXT, 0, w!("#b") as isize);
    before_timeout("alias suggestions", || visible_window(popup_class, pid));
    send(edit, WM_SETTEXT, 0, w!("") as isize);
    assert!(visible_window(popup_class, pid).is_none());
    send(shade, WM_LBUTTONUP, 0, 0);
    assert!(visible_window(search_class, pid).is_none());
    assert!(visible_window(shade_class, pid).is_none());
    let overlay_class = w!("YiSou.Rust.OcrSelection");
    send(search, WM_HOTKEY, 1, 0);
    assert!(visible_window(overlay_class, pid).is_none());
    let mut ocr_hotkey = None;

    let mut settings_launch = launch(&["--settings"]);
    let settings = before_timeout("settings window", || {
        visible_window(w!("YiSou.Rust.Settings"), pid)
    });
    before_timeout("second launch exits", || {
        settings_launch.0.try_wait().unwrap()
    });

    let nav = unsafe { GetDlgItem(settings, 1) };
    assert_eq!(
        send(nav, LB_GETCOUNT, 0, 0),
        if cfg!(feature = "ocr") { 8 } else { 7 }
    );
    let open_page = |index| {
        send(nav, LB_SETCURSEL, index, 0);
        send(
            settings,
            WM_COMMAND,
            ((LBN_SELCHANGE as usize) << 16) | 1,
            nav as isize,
        );
    };
    let item = |id| unsafe { GetDlgItem(settings, id) };
    let stored = || {
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .preferences
    };
    let language = item(103);
    let apply_language = item(104);
    assert_eq!(send(language, CB_GETCURSEL, 0, 0), 0);
    send(language, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_ne!(send(language, CB_GETDROPPEDSTATE, 0, 0), 0);
    send(language, WM_KEYDOWN, usize::from(VK_DOWN), 0);
    assert_eq!(send(language, CB_GETCURSEL, 0, 0), 1);
    assert_eq!(unsafe { IsWindowEnabled(apply_language) }, 0);
    send(language, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert_eq!(send(language, CB_GETDROPPEDSTATE, 0, 0), 0);
    assert_eq!(send(language, CB_GETCURSEL, 0, 0), 0);
    assert_eq!(unsafe { IsWindowEnabled(apply_language) }, 0);
    send(language, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    send(language, WM_KEYDOWN, usize::from(VK_DOWN), 0);
    send(language, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(send(language, CB_GETDROPPEDSTATE, 0, 0), 0);
    assert_eq!(send(language, CB_GETCURSEL, 0, 0), 1);
    assert_ne!(unsafe { IsWindowEnabled(apply_language) }, 0);

    for (page, control) in [
        102,
        110,
        122,
        134,
        140,
        150,
        if cfg!(feature = "ocr") { 170 } else { 162 },
    ]
    .into_iter()
    .chain(cfg!(feature = "ocr").then_some(162))
    .enumerate()
    {
        open_page(page);
        assert!(!item(control).is_null(), "settings page {page}");
    }
    open_page(2);
    send(item(27), WM_SETTEXT, 0, w!("#bd") as isize);
    before_timeout("engine filter", || {
        (send(item(10), LB_GETCOUNT, 0, 0) == 1).then_some(())
    });
    send(item(10), LB_SETCURSEL, 0, 0);
    send(settings, WM_COMMAND, 120, 0);
    assert_eq!(stored().default_engine, "baidu");
    send(settings, WM_COMMAND, 121, 0);
    assert_eq!(stored().pins, ["baidu"]);
    send(item(122), BM_CLICK, 0, 0);
    assert!(!item(20).is_null());
    send(item(128), BM_CLICK, 0, 0);
    open_page(1);
    assert!(!item(113).is_null());
    open_page(2);
    assert_eq!(send(item(10), LB_GETCOUNT, 0, 0), 1);
    send(item(10), LB_SETCURSEL, 0, 0);
    let menu_class = w!("YiSou.Rust.Settings.Menu");
    let open_engine_menu = || {
        send(settings, WM_COMMAND, 132, 0);
        before_timeout("engine actions menu", || visible_window(menu_class, pid))
    };
    let menu = open_engine_menu();
    let default_action = unsafe { GetDlgItem(menu, 120) };
    assert!(!default_action.is_null());
    assert_eq!(unsafe { IsWindowEnabled(default_action) }, 0);
    send(menu, WM_KEYDOWN, usize::from(VK_DOWN), 0);
    let menu_focus = |menu| {
        let mut gui = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            let thread = GetWindowThreadProcessId(menu, std::ptr::null_mut());
            assert_ne!(GetGUIThreadInfo(thread, &mut gui), 0);
        }
        gui.hwndFocus
    };
    let pin = unsafe { GetDlgItem(menu, 121) };
    assert_eq!(menu_focus(menu), pin);
    send(pin, WM_KEYDOWN, usize::from(VK_END), 0);
    let edit_alias = unsafe { GetDlgItem(menu, 123) };
    assert_eq!(menu_focus(menu), edit_alias);
    send(edit_alias, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert!(window_for_process(menu_class, pid).is_none());
    assert_eq!(menu_focus(settings), item(10));

    send(
        settings,
        WM_COMMAND,
        ((LBN_DBLCLK as usize) << 16) | 10,
        item(10) as isize,
    );
    let alias = before_timeout("built-in alias editor", || {
        let alias = item(21);
        (!alias.is_null()).then_some(alias)
    });
    assert!(item(22).is_null());
    send(alias, WM_SETTEXT, 0, w!("#g") as isize);
    send(item(126), BM_CLICK, 0, 0);
    assert!(send(item(133), WM_GETTEXTLENGTH, 0, 0) > 0);
    assert!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .built_in_aliases
            .is_empty()
    );
    send(alias, WM_SETTEXT, 0, w!("#BDTest") as isize);
    send(item(126), BM_CLICK, 0, 0);
    assert!(item(21).is_null());
    assert_eq!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .built_in_aliases["baidu"],
        "bdtest"
    );
    send(item(10), LB_SETCURSEL, 0, 0);
    send(settings, WM_COMMAND, 123, 0);
    send(item(119), BM_CLICK, 0, 0);
    send(item(126), BM_CLICK, 0, 0);
    assert_eq!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .built_in_aliases["baidu"],
        "bd"
    );

    open_page(3);
    let selection_engine = item(136);
    assert!(send(selection_engine, CB_GETCOUNT, 0, 0) > 7);
    send(selection_engine, CB_SHOWDROPDOWN, 1, 0);
    let mut combo = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    assert_ne!(unsafe { GetComboBoxInfo(selection_engine, &mut combo) }, 0);
    let row_height = send(selection_engine, CB_GETITEMHEIGHT, 0, 0) as i32;
    let popup = window_rect(combo.hwndList);
    assert!((row_height..8 * row_height).contains(&(popup.bottom - popup.top)));
    let selected = send(selection_engine, CB_GETCURSEL, 0, 0);
    for row in 1..=2 {
        let y = (row * row_height + row_height / 2) as isize;
        send(combo.hwndList, WM_MOUSEMOVE, 0, y << 16 | 20);
        assert_eq!(send(selection_engine, CB_GETCURSEL, 0, 0), selected);
    }
    send(selection_engine, CB_SHOWDROPDOWN, 0, 0);

    let window = window_rect(settings);
    unsafe {
        SetWindowPos(
            settings,
            std::ptr::null_mut(),
            0,
            0,
            (667 * dpi + 48) / 96,
            (345 * dpi + 48) / 96,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    let nav_before = window_rect(nav);
    let action_before = window_rect(item(134));
    for _ in 0..8 {
        send(
            settings,
            WM_MOUSEWHEEL,
            usize::from((-120i16) as u16) << 16,
            0,
        );
    }
    let action_after = window_rect(item(134));
    assert!(action_after.top < action_before.top);
    assert_eq!(window_rect(nav).top, nav_before.top);
    for _ in 0..8 {
        send(settings, WM_MOUSEWHEEL, 120 << 16, 0);
    }
    assert_eq!(window_rect(item(134)).top, action_before.top);
    unsafe {
        SetWindowPos(
            settings,
            std::ptr::null_mut(),
            0,
            0,
            window.right - window.left,
            window.bottom - window.top,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    send(item(136), CB_SETCURSEL, 0, 0);
    send(item(21), WM_SETTEXT, 0, w!("Ctrl+Alt+Shift+F21") as isize);
    send(item(134), BM_CLICK, 0, 0);
    assert_eq!(stored().selection.unwrap().engine, "bing");
    assert!(item(170).is_null());
    if cfg!(feature = "ocr") {
        send(item(176), BM_CLICK, 0, 0);
        assert_eq!(send(nav, LB_GETCURSEL, 0, 0), 6);
        for id in 170..=175 {
            assert!(!item(id).is_null(), "missing OCR control {id}");
        }
        assert!(!stored().ocr_enabled);
        if let Some(directory) = std::env::var_os("YISOU_TEST_OCR_MODELS") {
            use std::os::windows::ffi::OsStrExt;
            let directory = std::path::PathBuf::from(directory);
            let wide: Vec<_> = directory.as_os_str().encode_wide().chain([0]).collect();
            send(item(40), WM_SETTEXT, 0, wide.as_ptr() as isize);
            send(settings, WM_COMMAND, 172, item(172) as isize);
            before_timeout("saved validated OCR directory", || {
                (stored().ocr_model_dir.as_deref() == Some(directory.as_path())).then_some(())
            });
            assert!(!stored().ocr_enabled);
            send(settings, WM_COMMAND, 170, item(170) as isize);
            before_timeout("enabled OCR", || stored().ocr_enabled.then_some(()));
            ocr_hotkey = Some(2);
        }
    } else {
        assert!(item(176).is_null());
        assert!(stored().ocr_enabled);
        assert_eq!(
            stored().ocr_model_dir.as_deref(),
            Some(std::path::Path::new("external-models"))
        );
    }
    open_page(4);
    assert!(!item(10).is_null());
    send(item(142), BM_CLICK, 0, 0);
    assert!(!item(20).is_null());
    open_page(5);
    let rows = || send(item(10), LB_GETCOUNT, 0, 0);
    assert_eq!(rows(), 3);
    send(item(25), WM_SETTEXT, 0, w!("rust") as isize);
    send(item(26), WM_SETTEXT, 0, w!("terminal") as isize);
    before_timeout("filtered history", || (rows() == 1).then_some(()));
    send(item(25), WM_SETTEXT, 0, w!("missing result") as isize);
    before_timeout("empty filtered history", || (rows() == 0).then_some(()));
    send(item(155), BM_CLICK, 0, 0);
    assert_eq!(rows(), 3);
    for page in [2, 3, 5, 1, 2, 3, 5] {
        open_page(page);
    }
    send(settings, WM_CLOSE, 0, 0);
    #[cfg(feature = "ocr")]
    if let Some(id) = ocr_hotkey {
        check_ocr_selection(search, pid, id, &trace);
    }
    #[cfg(not(feature = "ocr"))]
    assert!(ocr_hotkey.is_none());
    send(search, WM_CLOSE, 0, 0);
    assert!(visible_window(search_class, pid).is_none());
    before_timeout("idle resources released", || {
        std::fs::read_to_string(&trace)
            .ok()
            .filter(|contents| contents.contains(",after_idle_release,"))
    });
    let mut reveal = launch(&[]);
    assert_eq!(
        before_timeout("resident is revealed again", || visible_window(
            search_class,
            pid
        )),
        search
    );
    before_timeout("reveal process exits", || reveal.0.try_wait().unwrap());
    before_timeout("shade is revealed again", || {
        visible_window(shade_class, pid)
    });

    let mut reopen_settings = launch(&["--settings"]);
    let settings = before_timeout("reopened settings window", || {
        visible_window(w!("YiSou.Rust.Settings"), pid)
    });
    before_timeout("settings command exits", || {
        reopen_settings.0.try_wait().unwrap()
    });
    if let Some(id) = ocr_hotkey {
        let nav = unsafe { GetDlgItem(settings, 1) };
        send(nav, LB_SETCURSEL, 6, 0);
        send(
            settings,
            WM_COMMAND,
            ((LBN_SELCHANGE as usize) << 16) | 1,
            nav as isize,
        );
        send(settings, WM_COMMAND, 170, 0);
        let prefs = JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .preferences;
        assert!(!prefs.ocr_enabled);
        assert!(prefs.ocr_model_dir.is_some());
        send(search, WM_HOTKEY, id, 0);
        assert!(visible_window(overlay_class, pid).is_none());
        send(nav, LB_SETCURSEL, 0, 0);
        send(
            settings,
            WM_COMMAND,
            ((LBN_SELCHANGE as usize) << 16) | 1,
            nav as isize,
        );
    }
    send(unsafe { GetDlgItem(settings, 102) }, BM_CLICK, 0, 0);
    send(settings, WM_CLOSE, 0, 0);
    send(search, WM_CLOSE, 0, 0);
    let status = before_timeout("resident exits after closing", || {
        resident.0.try_wait().unwrap()
    });
    let mut diagnostics = String::new();
    resident
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut diagnostics)
        .unwrap();
    assert!(status.success(), "{status}\n{diagnostics}");
    let csv = std::fs::read_to_string(trace).unwrap();
    assert!(csv.starts_with("pid,elapsed_us,event,working_set_bytes,private_bytes,"));
    let events: Vec<_> = csv
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            assert_eq!(fields.len(), 8);
            assert_eq!(fields[0].parse::<u32>().unwrap(), pid);
            assert!(fields[4].parse::<usize>().unwrap() > 0);
            fields[2]
        })
        .collect();
    for expected in [
        "resident_ready",
        "search_controls_created",
        "engine_popup_shown",
        "history_page_loaded",
        "history_cache_released",
        "message_loop_exit",
    ] {
        assert!(events.contains(&expected), "missing trace event {expected}");
    }
}

#[cfg(feature = "ocr")]
fn check_ocr_selection(search: HWND, pid: u32, id: usize, trace: &std::path::Path) {
    use windows_sys::Win32::{
        Graphics::Gdi::*,
        UI::{HiDpi::*, Input::KeyboardAndMouse::VK_RETURN},
    };

    let class = w!("YiSou.Rust.OcrSelection");
    let capture = || {
        send(search, WM_HOTKEY, id, 0);
        before_timeout("OCR selection overlay", || visible_window(class, pid))
    };
    let overlay = capture();
    let capture_count = || {
        std::fs::read_to_string(trace)
            .unwrap()
            .matches(",ocr_capture_region,")
            .count()
    };
    let job_count = || {
        std::fs::read_to_string(trace)
            .unwrap()
            .matches(",ocr_job_started,")
            .count()
    };
    send(overlay, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert_eq!(
        capture_count(),
        0,
        "opening the overlay does not capture pixels"
    );
    assert!(visible_window(class, pid).is_none());
    assert!(visible_window(w!("YiSou.Rust.Search"), pid).is_none());

    let _dpi = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let mut pointer = windows_sys::Win32::Foundation::POINT::default();
    unsafe { GetCursorPos(&mut pointer) };
    const SS_WHITERECT: u32 = 0x06;
    let backdrop = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("STATIC"),
            null(),
            WS_POPUP | SS_WHITERECT,
            0,
            0,
            760,
            460,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            null(),
        )
    };
    assert!(!backdrop.is_null());
    let fixture = unsafe {
        CreateWindowExW(
            0,
            w!("STATIC"),
            w!("Rust error E0425\r\nRead the docs"),
            WS_CHILD | WS_VISIBLE,
            60,
            60,
            420,
            135,
            backdrop,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            null(),
        )
    };
    assert!(!fixture.is_null());
    let font = unsafe {
        CreateFontW(
            -42,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            w!("Segoe UI"),
        )
    };
    assert!(!font.is_null());
    unsafe {
        SendMessageW(fixture, WM_SETFONT, font as usize, 1);
        ShowWindow(backdrop, SW_SHOWNOACTIVATE);
        UpdateWindow(backdrop);
        UpdateWindow(fixture);
        windows_sys::Win32::Graphics::Dwm::DwmFlush();
    }
    let error_code_x = unsafe {
        let dc = GetDC(fixture);
        let old = SelectObject(dc, font);
        let mut prefix = windows_sys::Win32::Foundation::SIZE::default();
        assert_ne!(
            GetTextExtentPoint32W(dc, w!("Rust error "), 11, &mut prefix),
            0
        );
        SelectObject(dc, old);
        ReleaseDC(fixture, dc);
        60 + prefix.cx + 35
    };
    let wait_for_text = |count| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let label = format!("OCR text available on screenshot ({count})");
        before_timeout(&label, || {
            let log = std::fs::read_to_string(trace).ok()?;
            if log.matches(",ocr_text_ready,").count() == count {
                return Some(());
            }
            assert!(
                Instant::now() + Duration::from_millis(40) < deadline,
                "{label}: {}",
                log.lines()
                    .filter(|line| line.contains(",ocr_"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            None
        });
        assert!(visible_window(w!("YiSou.Rust.Search"), pid).is_none());
    };
    let blank = capture();
    unsafe { SetCursorPos(411, 150) };
    send(blank, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(475, 182) };
    send(blank, WM_MOUSEMOVE, 1, 0);
    assert_eq!(capture_count(), 0, "dragging does not capture pixels");
    send(blank, WM_LBUTTONUP, 0, 0);
    before_timeout("blank crop reported inline", || {
        let log = std::fs::read_to_string(trace).ok()?;
        assert!(
            !log.contains(",ocr_failed,") && !log.contains(",ocr_readback_failed,"),
            "{log}"
        );
        log.contains(",ocr_no_text,").then_some(())
    });
    assert_eq!(visible_window(class, pid), Some(blank));

    let query = || {
        let search = before_timeout("OCR result in search", || {
            visible_window(w!("YiSou.Rust.Search"), pid)
        });
        let edit = unsafe { GetDlgItem(search, 100) };
        let mut text = [0u16; 256];
        let length = send(edit, WM_GETTEXT, text.len(), text.as_mut_ptr() as isize);
        (edit, String::from_utf16_lossy(&text[..length]))
    };
    // Leave room for the detector's character bounds around the first glyph.
    let capture_left = 52;
    let before_capture = job_count();
    unsafe { SetCursorPos(capture_left, 53) };
    send(blank, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(502, 210) };
    send(blank, WM_LBUTTONUP, 0, 0);
    // The completed job should still cover the new crop even if the user starts resizing early.
    unsafe { SetCursorPos(501, 122) };
    send(blank, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(477, 122) };
    send(blank, WM_LBUTTONUP, 0, 0);
    wait_for_text(1);
    assert_eq!(job_count(), before_capture + 1);
    let overlay = blank;
    send(overlay, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(visible_window(class, pid), Some(overlay));
    unsafe { SetCursorPos(69, 82) };
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(370, 127) };
    send(overlay, WM_MOUSEMOVE, 1, 0);
    send(overlay, WM_LBUTTONUP, 0, 0);
    send(overlay, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    let (edit, text) = query();
    assert_eq!(text, "Rust error E0425 Read the docs");
    send(edit, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);

    let overlay = capture();
    #[cfg(feature = "ocr-ui-test")]
    let screen_pixels = |points: &[(i32, i32)]| unsafe {
        windows_sys::Win32::Graphics::Dwm::DwmFlush();
        let screen = GetDC(std::ptr::null_mut());
        assert!(!screen.is_null());
        let pixels: Vec<_> = points
            .iter()
            .map(|&(x, y)| GetPixel(screen, x, y))
            .collect();
        ReleaseDC(std::ptr::null_mut(), screen);
        assert!(pixels.iter().all(|&pixel| pixel != CLR_INVALID));
        pixels
    };
    #[cfg(feature = "ocr-ui-test")]
    const BLUE: u32 = 0xdb751f; // COLORREF for the frame's RGB accent.
    #[cfg(feature = "ocr-ui-test")]
    const WHITE: u32 = 0xffffff;
    #[cfg(feature = "ocr-ui-test")]
    let outside: Vec<_> = (58..105).map(|x| (x, 191)).collect();
    #[cfg(feature = "ocr-ui-test")]
    let masked_pixels = before_timeout("composited shade", || {
        let pixels = screen_pixels(&outside);
        pixels
            .iter()
            .all(|&pixel| pixel < 0xc0c0c0)
            .then_some(pixels)
    });
    unsafe { SetCursorPos(capture_left, 53) };
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(478, 210) };
    send(overlay, WM_LBUTTONUP, 0, 0);
    wait_for_text(2);
    let original_job_count = job_count();
    let frame = unsafe {
        use windows_sys::Win32::Foundation::POINT;
        let point = POINT {
            x: capture_left - 3,
            y: 131,
        };
        SetCursorPos(point.x, point.y);
        let frame = WindowFromPoint(point);
        assert_eq!(GetWindow(frame, GW_OWNER), overlay);
        frame
    };
    send(frame, WM_LBUTTONDOWN, 1, 0);
    let before_drag = capture_count();
    let drag_anchor = capture_left - 3;
    for x in (capture_left + 2..122).step_by(2) {
        unsafe { SetCursorPos(x + drag_anchor - capture_left, 131) };
        send(overlay, WM_MOUSEMOVE, 1, 0);
        #[cfg(feature = "ocr-ui-test")]
        if [76, 98, 120].contains(&x) {
            before_timeout("new frame and grip share the moved edge", || {
                let pixels = screen_pixels(&[
                    (x, 190),
                    (x - 2, 131),
                    (x + 2, 131),
                    (477, 190),
                    (260, 53),
                    (260, 209),
                ]);
                (pixels == [BLUE, WHITE, WHITE, BLUE, BLUE, BLUE]).then_some(())
            });
        }
    }
    #[cfg(feature = "ocr-ui-test")]
    assert_eq!(screen_pixels(&outside), masked_pixels);
    assert_eq!(
        capture_count(),
        before_drag,
        "resizing captures only on release"
    );
    send(overlay, WM_LBUTTONUP, 0, 0);
    wait_for_text(3);
    #[cfg(feature = "ocr-ui-test")]
    before_timeout(
        "frozen preview preserves the frame and centered grip",
        || {
            (screen_pixels(&[(120, 190), (118, 131), (122, 131)]) == [BLUE, WHITE, WHITE])
                .then_some(())
        },
    );
    assert_eq!(
        job_count(),
        original_job_count,
        "shrinking is served from the cache"
    );
    unsafe { SetCursorPos(120, 122) };
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(capture_left, 122) };
    send(overlay, WM_LBUTTONUP, 0, 0);
    wait_for_text(4);
    #[cfg(feature = "ocr-ui-test")]
    before_timeout("expanded frame follows the edge", || {
        (screen_pixels(&[(capture_left, 190)]) == [BLUE]).then_some(())
    });
    assert_eq!(
        job_count(),
        original_job_count,
        "restoring the original crop is cached"
    );
    unsafe { SetCursorPos(477, 122) };
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    unsafe { SetCursorPos(502, 122) };
    send(overlay, WM_LBUTTONUP, 0, 0);
    wait_for_text(5);
    assert_eq!(
        job_count(),
        original_job_count + 1,
        "expansion recognizes the new strip"
    );
    unsafe {
        SetWindowTextW(fixture, w!("Rust error E0425\r\nRead the guide"));
        UpdateWindow(fixture);
        windows_sys::Win32::Graphics::Dwm::DwmFlush();
        SetCursorPos(501, 122);
    }
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    send(overlay, WM_LBUTTONUP, 0, 0);
    wait_for_text(6);
    assert_eq!(
        job_count(),
        original_job_count + 2,
        "changed desktop pixels invalidate the cache"
    );
    unsafe { SetCursorPos(error_code_x, 82) };
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    send(overlay, WM_LBUTTONUP, 0, 0);
    let mut dpi = 96;
    let mut dpi_y = 96;
    unsafe {
        let monitor = MonitorFromPoint(
            windows_sys::Win32::Foundation::POINT { x: 61, y: 61 },
            MONITOR_DEFAULTTONEAREST,
        );
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let px = |dips| (dips * dpi as i32 + 48) / 96;
        SetCursorPos(503 - px(65), 210 + px(33));
    }
    send(overlay, WM_LBUTTONDOWN, 1, 0);
    send(overlay, WM_LBUTTONUP, 0, 0);
    unsafe {
        DestroyWindow(backdrop);
        DeleteObject(font);
        SetCursorPos(pointer.x, pointer.y);
    }
    let (edit, text) = query();
    assert_eq!(text, "E0425");
    send(edit, WM_SETTEXT, 0, w!("#g E0425") as isize);
    assert!(visible_window(w!("YiSou.Rust.Engines"), pid).is_none());
    send(edit, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
}
