#![cfg(windows)]

use std::{
    io::Read,
    process::{Child, Command, Stdio},
    ptr::null,
    sync::Mutex,
    thread::sleep,
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::{InvalidateRect, UpdateWindow},
        UI::{
            Controls::{COMBOBOXINFO, EM_CHARFROMPOS, EM_GETSEL, EM_SETSEL, GetComboBoxInfo},
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
                SetThreadDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::{
                IsWindowEnabled, VK_BACK, VK_DOWN, VK_END, VK_ESCAPE, VK_F9, VK_F10, VK_HOME,
                VK_LEFT, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
            },
            WindowsAndMessaging::*,
        },
    },
    w,
};
use yisou::settings::ScreenBackground;
use yisou::storage::JsonSettings;

static DESKTOP: Mutex<()> = Mutex::new(());

fn executable() -> std::ffi::OsString {
    std::env::var_os("YISOU_TEST_EXE").unwrap_or_else(|| env!("CARGO_BIN_EXE_YiSou").into())
}

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

#[test]
fn native_search_alias_suggestions_and_square_engine_button() {
    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let mut settings = yisou::settings::Settings::default();
    let engines = yisou::engine::built_in_engines();
    for index in 0..32 {
        let mut engine = engines[0].clone();
        engine.id = format!("custom-test-{index}");
        engine.name = format!("Test engine {index}");
        engine.alias = format!("test{index}");
        settings.custom_engines.push(engine);
    }
    let engine_count = engines.len() + settings.custom_engines.len();
    settings.normalize().unwrap();
    std::fs::write(&config, serde_json::to_vec(&settings).unwrap()).unwrap();
    let child = AppProcess(
        Command::new(executable())
            .env("LOCALAPPDATA", data.path())
            .arg("--config")
            .arg(config)
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();
    let search_class = w!("YiSou.Rust.Search");
    let popup_class = w!("YiSou.Rust.Engines");
    let search = before_timeout("search window", || visible_window(search_class, pid));
    let edit = unsafe { GetDlgItem(search, 100) };
    let button = unsafe { GetDlgItem(search, 101) };
    let dpi = unsafe { GetDpiForWindow(search) } as i32;
    let px = |dip| (dip * dpi + 48) / 96;
    let bounds = window_rect(button);
    assert_eq!(bounds.right - bounds.left, bounds.bottom - bounds.top);
    assert_eq!(bounds.right - bounds.left, px(40));

    let open_suggestions = || {
        send(edit, WM_SETTEXT, 0, w!("#") as isize);
        send(edit, EM_SETSEL, 1, 1);
        before_timeout("unfiltered alias suggestions", || {
            visible_window(popup_class, pid)
        })
    };
    let suggestions = open_suggestions();
    let search_bounds = window_rect(search);
    let suggestion_bounds = window_rect(suggestions);
    assert_eq!(suggestion_bounds.left, search_bounds.left);
    assert_eq!(suggestion_bounds.right, search_bounds.right);
    let visible_rows = (1..=6)
        .find(|rows| height(suggestions) == px(14 + 38 * *rows as i32))
        .expect("alias panel uses a compact viewport");
    let total_rows = engine_count.div_ceil(2);

    send(edit, WM_KEYDOWN, usize::from(VK_LEFT), 0);
    assert_eq!(send(edit, EM_GETSEL, 0, 0), 0, "left still moves the caret");
    send(edit, WM_KEYDOWN, usize::from(VK_RIGHT), 0);
    assert_eq!(send(edit, EM_GETSEL, 0, 0), 0x0001_0001);
    send(edit, WM_KEYDOWN, usize::from(VK_RIGHT), 0);
    for _ in 1..total_rows {
        send(edit, WM_KEYDOWN, usize::from(VK_DOWN), 0);
    }
    send(edit, WM_KEYDOWN, usize::from(VK_TAB), 0);
    assert_eq!(control_text(edit), "#test31 ");
    assert!(visible_window(popup_class, pid).is_none());

    let suggestions = open_suggestions();
    let popup_bounds = window_rect(suggestions);
    let pointer = ((popup_bounds.top + px(20)) << 16) | ((popup_bounds.left + px(20)) & 0xffff);
    for _ in visible_rows..total_rows {
        send(
            edit,
            WM_MOUSEWHEEL,
            usize::from((-120i16) as u16) << 16,
            pointer as isize,
        );
    }
    let last_row = ((px(20 + 38 * (visible_rows as i32 - 1)) << 16) | px(310)) as isize;
    send(suggestions, WM_LBUTTONUP, 0, last_row);
    assert_eq!(control_text(edit), "#test31 ");
    assert!(visible_window(popup_class, pid).is_none());

    send(edit, WM_SETTEXT, 0, w!("#x") as isize);
    let filtered = before_timeout("filtered alias suggestions", || {
        visible_window(popup_class, pid)
    });
    assert_eq!(height(filtered), px(52));
    send(edit, WM_KEYDOWN, usize::from(VK_TAB), 0);
    assert_eq!(control_text(edit), "#x ");
}

#[test]
fn native_screenshot_image_search_keeps_selection_when_provider_menu_is_cancelled() {
    use std::{path::PathBuf, ptr::null_mut};
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;
    use yisou::{
        settings::{Language, Settings},
        storage::SettingsStore,
    };

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let trace = data.path().join("image-search.csv");
    let mut settings = Settings::default();
    settings.preferences.language = Language::Chinese;
    settings.preferences.ocr_enabled = false;
    settings.preferences.browser =
        Some(PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe"));
    JsonSettings::new(config.clone()).save(&settings).unwrap();
    let mut child = AppProcess(
        Command::new(executable())
            .env("LOCALAPPDATA", data.path())
            .env("YISOU_MEMORY_TRACE", &trace)
            .env("TEMP", data.path())
            .env("TMP", data.path())
            .arg("--config")
            .arg(config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();
    let search = before_timeout("search owner", || {
        visible_window(w!("YiSou.Rust.Search"), pid)
    });

    struct Fixture(HWND, POINT);
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
                SetCursorPos(self.1.x, self.1.y);
            }
        }
    }
    let fixture = unsafe {
        let mut cursor = POINT::default();
        GetCursorPos(&mut cursor);
        // SS_WHITERECT and SS_BLACKRECT give us a known vertical boundary.
        let backdrop = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("STATIC"),
            null(),
            WS_POPUP | 0x06,
            0,
            0,
            420,
            400,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
        );
        assert!(!backdrop.is_null());
        let black = CreateWindowExW(
            0,
            w!("STATIC"),
            null(),
            WS_CHILD | WS_VISIBLE | 0x04,
            120,
            0,
            300,
            400,
            backdrop,
            null_mut(),
            null_mut(),
            null(),
        );
        assert!(!black.is_null());
        ShowWindow(backdrop, SW_SHOWNOACTIVATE);
        UpdateWindow(backdrop);
        UpdateWindow(black);
        DwmFlush();
        Fixture(backdrop, cursor)
    };
    let menu_class = w!("#32768");
    let overlay_class = w!("YiSou.Rust.OcrSelection");
    let open_menu = |target: HWND, msg: u32, wp: usize, lp: isize| {
        assert_ne!(unsafe { PostMessageW(target, msg, wp, lp) }, 0);
        before_timeout("native popup menu", || visible_window(menu_class, pid))
    };
    let key = |menu, key: u16| {
        assert_ne!(
            unsafe { PostMessageW(menu, WM_KEYDOWN, key as usize, 0) },
            0
        );
    };
    let drag = |overlay, (x1, y1), (x2, y2)| {
        unsafe { SetCursorPos(x1, y1) };
        send(overlay, WM_LBUTTONDOWN, 0, 0);
        unsafe { SetCursorPos(x2, y2) };
        send(overlay, WM_MOUSEMOVE, 0, 0);
        send(overlay, WM_LBUTTONUP, 0, 0);
    };
    unsafe {
        SetCursorPos(220, 200);
        AllowSetForegroundWindow(pid);
    }
    let tray = open_menu(search, WM_APP + 2, 1, WM_RBUTTONUP as isize);
    // OCR is disabled in both editions. Screenshot image search follows Open search.
    key(tray, VK_DOWN);
    key(tray, VK_DOWN);
    key(tray, VK_RETURN);
    let overlay = before_timeout("screenshot selection", || {
        visible_window(overlay_class, pid)
    });
    drag(overlay, (70, 80), (190, 145));

    let menu = open_menu(overlay, WM_KEYDOWN, VK_RETURN as usize, 0);
    key(menu, VK_ESCAPE);
    before_timeout("engine menu cancelled", || {
        visible_window(menu_class, pid).is_none().then_some(())
    });
    assert_eq!(visible_window(overlay_class, pid), Some(overlay));

    drag(overlay, (190, 112), (245, 112));
    let menu = open_menu(overlay, WM_KEYDOWN, VK_RETURN as usize, 0);
    if std::env::var_os("YISOU_TEST_IMAGE_SEARCH_NETWORK").is_some() {
        key(menu, VK_DOWN);
        key(menu, VK_RETURN);
        before_timeout("handoff to the browser", || {
            visible_window(overlay_class, pid).is_none().then_some(())
        });
        before_timeout("both image searches reach the browser", || {
            let log = std::fs::read_to_string(&trace).ok()?;
            assert!(!log.contains(",image_search_result_failed,"), "{log}");
            (log.matches(",image_search_result_opened,").count() == 2).then_some(())
        });
    } else {
        key(menu, VK_ESCAPE);
        before_timeout("resized region remains searchable", || {
            visible_window(menu_class, pid).is_none().then_some(())
        });
        send(overlay, WM_KEYDOWN, VK_ESCAPE as usize, 0);
        assert!(visible_window(overlay_class, pid).is_none());
    }
    unsafe { AllowSetForegroundWindow(pid) };
    let tray = open_menu(search, WM_APP + 2, 1, WM_RBUTTONUP as isize);
    key(tray, VK_UP);
    key(tray, VK_RETURN);
    before_timeout("graceful exit", || child.0.try_wait().unwrap());
    assert!(
        !std::fs::read_dir(data.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| { entry.path().extension().is_some_and(|ext| ext == "png") })
    );
    drop(fixture);
}

fn screen_shades(search: HWND) -> Vec<HWND> {
    let mut shades = Vec::new();
    let mut window = unsafe { GetWindow(search, GW_HWNDNEXT) };
    let class: Vec<_> = "YiSou.Rust.ScreenShade".encode_utf16().collect();
    while !window.is_null() {
        let mut buffer = [0; 40];
        let len = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
        if buffer[..len as usize] != class {
            break;
        }
        assert_ne!(unsafe { IsWindowVisible(window) }, 0);
        shades.push(window);
        window = unsafe { GetWindow(window, GW_HWNDNEXT) };
    }
    shades
}

#[test]
fn native_screen_shade_stays_in_place_when_search_moves() {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows_sys::core::BOOL;

    unsafe extern "system" fn collect(_: HMONITOR, _: HDC, bounds: *mut RECT, data: isize) -> BOOL {
        unsafe { (*(data as *mut Vec<RECT>)).push(*bounds) };
        1
    }

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let launch = |args: &[&str]| {
        AppProcess(
            Command::new(executable())
                .env("LOCALAPPDATA", data.path())
                .arg("--config")
                .arg(&config)
                .args(args)
                .spawn()
                .unwrap(),
        )
    };
    let app = launch(&["--startup"]);
    let class = w!("YiSou.Rust.Search");
    let pid = app.0.id();
    let search = before_timeout("resident search window", || window_for_process(class, pid));
    let mut reveal = launch(&[]);
    before_timeout("show search", || visible_window(class, pid));
    before_timeout("reveal process exits", || reveal.0.try_wait().unwrap());
    let shades = before_timeout("all screen shades visible", || {
        let shades = screen_shades(search);
        (!shades.is_empty()).then_some(shades)
    });
    let coordinates = |bounds: RECT| (bounds.left, bounds.top, bounds.right, bounds.bottom);
    let coverage = || {
        let mut bounds: Vec<_> = screen_shades(search)
            .into_iter()
            .map(|hwnd| coordinates(window_rect(hwnd)))
            .collect();
        bounds.sort_unstable();
        bounds
    };
    let mut monitors = Vec::<RECT>::new();
    assert_ne!(
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                null(),
                Some(collect),
                &mut monitors as *mut Vec<RECT> as isize,
            )
        },
        0
    );
    let mut monitor_bounds: Vec<_> = monitors.into_iter().map(coordinates).collect();
    monitor_bounds.sort_unstable();
    assert_eq!(coverage(), monitor_bounds);

    for (left, top, right, bottom) in &monitor_bounds {
        assert_ne!(
            unsafe {
                SetWindowPos(
                    search,
                    std::ptr::null_mut(),
                    left + (right - left) / 5,
                    top + (bottom - top) / 5,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            },
            0,
        );
        assert_eq!(coverage(), monitor_bounds);
    }
    send(shades[0], WM_LBUTTONUP, 0, 0);
    assert!(visible_window(class, pid).is_none());
    assert!(
        shades
            .iter()
            .all(|&hwnd| unsafe { IsWindowVisible(hwnd) } == 0)
    );
}

#[test]
fn native_search_material_and_tone_are_independent_of_screen_blur() {
    use windows_sys::Win32::{
        Graphics::Gdi::{ClientToScreen, GetDC, GetPixel, ReleaseDC},
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    };
    use yisou::settings::{Language, SearchMaterial, SearchTone, Settings};

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let trace = data.path().join("memory.csv");
    let mut defaults = Settings::default();
    defaults.preferences.language = Language::Chinese;
    defaults.preferences.search_tone = SearchTone::Dark;
    std::fs::write(&config, serde_json::to_vec(&defaults).unwrap()).unwrap();
    let launch = |args: &[&str]| {
        AppProcess(
            Command::new(executable())
                .env("LOCALAPPDATA", data.path())
                .env("YISOU_MEMORY_TRACE", &trace)
                .arg("--config")
                .arg(&config)
                .args(args)
                .spawn()
                .unwrap(),
        )
    };
    let app = launch(&["--settings"]);
    let pid = app.0.id();
    let settings = before_timeout("settings window", || {
        visible_window(w!("YiSou.Rust.Settings"), pid)
    });
    let nav = unsafe { GetDlgItem(settings, 1) };
    send(nav, LB_SETCURSEL, 1, 0);
    send(
        settings,
        WM_COMMAND,
        (LBN_SELCHANGE as usize) << 16 | 1,
        nav as isize,
    );
    let combo = |id| unsafe { GetDlgItem(settings, id) };
    assert_eq!(combo_labels(combo(197)), ["亚克力", "云母", "纯色"]);
    assert_eq!(combo_labels(combo(198)), ["跟随系统", "浅色", "深色"]);
    assert_eq!(
        combo_labels(combo(199)),
        ["纯暗遮罩", "系统模糊", "高斯模糊"]
    );
    assert_eq!(send(combo(199), CB_GETCURSEL, 0, 0), 1);
    assert!(combo(303).is_null());
    let input_text = |id| {
        let mut text = [0u16; 16];
        let len = send(
            combo(id),
            WM_GETTEXT,
            text.len(),
            text.as_mut_ptr() as isize,
        );
        String::from_utf16(&text[..len]).unwrap()
    };
    assert_eq!(input_text(210), "32");
    assert!(combo(213).is_null());
    let light_color = window_rect(combo(230));
    let dark_color = window_rect(combo(231));
    assert_eq!(light_color.top, dark_color.top);
    assert!(light_color.right < dark_color.left);

    let mut reveal = launch(&[]);
    let search = before_timeout("search window", || {
        visible_window(w!("YiSou.Rust.Search"), pid)
    });
    assert!(before_timeout("reveal process exits", || reveal.0.try_wait().unwrap()).success());
    let controls = [100, 101].map(|id| unsafe { GetDlgItem(search, id) });
    let bounds = controls.map(window_rect);
    let control_edges = [
        (bounds[0].right, (bounds[0].top + bounds[0].bottom) / 2, -1),
        (bounds[1].left, (bounds[1].top + bounds[1].bottom) / 2, 1),
    ];
    let shades = before_timeout("all screen shades visible", || {
        let shades = screen_shades(search);
        (!shades.is_empty()).then_some(shades)
    });
    let stored = || {
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .preferences
    };
    let select = |id, index| {
        let control = combo(id);
        send(control, CB_SETCURSEL, index, 0);
        send(
            settings,
            WM_COMMAND,
            (CBN_SELCHANGE as usize) << 16 | id as usize,
            control as isize,
        );
    };
    let material = combo(197);
    select(197, 1);
    assert_eq!(combo(197), material);
    assert!(combo(210).is_null());
    assert_eq!(input_text(213), "84");
    select(197, 2);
    assert!(combo(210).is_null() && combo(213).is_null());
    select(197, 0);
    assert_eq!(input_text(210), "32");
    let mut system_light = 1u32;
    let mut size = size_of_val(&system_light) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut system_light as *mut u32).cast(),
            &mut size,
        );
    }
    unsafe {
        InvalidateRect(search, null(), 0);
        UpdateWindow(search);
    }
    let supports_acrylic = std::fs::read_to_string(&trace)
        .unwrap()
        .contains(",backdrop_acrylic,");
    if std::env::var_os("YISOU_TEST_REQUIRE_ACRYLIC").is_some() {
        assert!(supports_acrylic);
    }
    let assert_ready_before_reveal = |events: &str, ready: &str| {
        assert!(
            events.find(&format!(",{ready},")).unwrap() < events.find(",search_shown,").unwrap(),
            "{ready} must occur before revealing the search window"
        );
    };
    assert_ready_before_reveal(
        &std::fs::read_to_string(&trace).unwrap(),
        if supports_acrylic {
            "backdrop_acrylic"
        } else if std::fs::read_to_string(&trace)
            .unwrap()
            .contains(",backdrop_solid_composed,")
        {
            "backdrop_solid_composed"
        } else {
            "first_paint"
        },
    );
    for (index, material) in [
        (2, SearchMaterial::Solid),
        (1, SearchMaterial::Mica),
        (0, SearchMaterial::Acrylic),
    ] {
        select(197, index);
        for (tone_index, tone) in [SearchTone::System, SearchTone::Light, SearchTone::Dark]
            .into_iter()
            .enumerate()
        {
            select(198, tone_index);
            let prefs = stored();
            assert_eq!((prefs.search_material, prefs.search_tone), (material, tone));
            assert_eq!(prefs.screen_background, ScreenBackground::System);
            send(controls[0], WM_SETTEXT, 0, w!("复杂中文 鬱鑿") as isize);
            let dark = tone.is_dark(system_light == 0);
            unsafe {
                InvalidateRect(search, null(), 0);
                UpdateWindow(search);
            }
            let mut sample = POINT {
                x: 10,
                y: height(search) / 2,
            };
            assert_ne!(unsafe { ClientToScreen(search, &mut sample) }, 0);
            let mut edge = POINT {
                x: (bounds[0].right - bounds[0].left) / 2,
                y: 0,
            };
            assert_ne!(unsafe { ClientToScreen(search, &mut edge) }, 0);
            let deadline = Instant::now() + Duration::from_secs(4);
            before_timeout(&format!("{material:?} {tone:?} background"), || unsafe {
                let screen = GetDC(std::ptr::null_mut());
                assert!(!screen.is_null());
                let pixel = GetPixel(screen, sample.x, sample.y);
                let control_pixels = control_edges.map(|(x, y, direction)| {
                    [
                        GetPixel(screen, x + 3 * direction, y),
                        GetPixel(screen, x - 3 * direction, y),
                    ]
                });
                let frame_pixels =
                    [0, 1, 4].map(|offset| GetPixel(screen, edge.x, edge.y + offset));
                ReleaseDC(std::ptr::null_mut(), screen);
                let rgb = pixel.to_le_bytes();
                let matches_tone = if material == SearchMaterial::Solid || !supports_acrylic {
                    pixel == if dark { 0 } else { 0xffffff }
                } else if dark {
                    rgb[..3].iter().all(|&channel| channel < 128)
                } else {
                    rgb[..3].iter().all(|&channel| channel >= 128)
                };
                let blends = control_pixels.iter().all(|&[inside, outside]| {
                    inside.to_le_bytes()[..3]
                        .iter()
                        .zip(&outside.to_le_bytes()[..3])
                        .all(|(inside, outside)| inside.abs_diff(*outside) <= 4)
                });
                let [edge, near, interior] = frame_pixels.map(u32::to_le_bytes);
                let close = |a: [u8; 4], b: [u8; 4]| {
                    a[..3].iter().zip(&b[..3]).all(|(a, b)| a.abs_diff(*b) <= 4)
                };
                let border_colors: [u32; 2] = if dark {
                    [0x4f4642, 0x694c32]
                } else {
                    [0xeae2dd, 0xdec1a6]
                };
                let matches_border = border_colors
                    .into_iter()
                    .any(|color| close(edge, color.to_le_bytes()))
                    && close(near, interior);
                let painted = matches_tone && blends && matches_border;
                assert!(
                    painted || Instant::now() < deadline,
                    "{material:?} {tone:?} background pixel {pixel:#08x}, control edges {control_pixels:06x?}, top border {frame_pixels:06x?}, search visible={} at ({}, {}), trace:\n{}",
                    IsWindowVisible(search),
                    sample.x,
                    sample.y,
                    std::fs::read_to_string(&trace).unwrap()
                );
                painted.then_some(())
            });
            assert_eq!(screen_shades(search), shades);
        }
    }

    let effect_control = combo(199);
    select(199, 0);
    assert_eq!(stored().screen_background, ScreenBackground::Dim);
    let dim = screen_shades(search);
    assert!(!dim.is_empty());
    assert!(dim.iter().all(|&window| unsafe {
        GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_LAYERED as isize != 0
    }));
    assert_eq!(stored().search_material, SearchMaterial::Acrylic);
    assert_eq!(stored().search_tone, SearchTone::Dark);
    let set_transparency = |id, text: *const u16, key| {
        send(combo(id), WM_SETTEXT, 0, text as isize);
        send(combo(id), WM_KEYDOWN, usize::from(key), 0);
    };
    let previous_events = std::fs::read_to_string(&trace).unwrap();
    select(199, 2);
    assert_eq!(combo(199), effect_control);
    assert_eq!(stored().screen_background, ScreenBackground::Gaussian);
    assert_eq!(input_text(303), "50");
    let events = std::fs::read_to_string(&trace).unwrap();
    let gaussian = events[previous_events.len()..].contains(",screen_shade_gaussian,");
    if std::env::var_os("YISOU_TEST_REQUIRE_GAUSSIAN").is_some() {
        assert!(gaussian, "{events}");
    }
    set_transparency(303, w!("75"), VK_RETURN);
    assert_eq!(stored().background_blur_percent, 75);
    if gaussian {
        let events_after = std::fs::read_to_string(&trace).unwrap();
        assert!(events_after[events.len()..].contains(",screen_shade_gaussian,"));
    }
    set_transparency(303, w!("0"), VK_RETURN);
    assert_eq!(input_text(303), "0");
    assert_eq!(unsafe { IsWindowEnabled(combo(304)) }, 0);
    set_transparency(303, w!("125"), VK_RETURN);
    assert_eq!(input_text(303), "100");
    assert_eq!(unsafe { IsWindowEnabled(combo(305)) }, 0);
    select(199, 1);
    assert_eq!(stored().screen_background, ScreenBackground::System);
    assert!(combo(303).is_null());
    select(199, 2);
    assert_eq!(input_text(303), "100");
    select(199, 1);
    set_transparency(210, w!("25"), VK_ESCAPE);
    assert_eq!(input_text(210), "32");
    send(settings, WM_COMMAND, 212, combo(212) as isize);
    assert_eq!(input_text(210), "33");
    assert_eq!(stored().acrylic_tint_percent, 67);
    assert_eq!(stored().mica_tint_percent, 16);
    let mut sample = POINT {
        x: 10,
        y: height(search) / 2,
    };
    assert_ne!(unsafe { ClientToScreen(search, &mut sample) }, 0);
    let sample_fill = || unsafe {
        let screen = GetDC(std::ptr::null_mut());
        assert!(!screen.is_null());
        let pixel = GetPixel(screen, sample.x, sample.y);
        ReleaseDC(std::ptr::null_mut(), screen);
        assert_ne!(pixel, u32::MAX);
        pixel
    };
    for (material_index, id, decrease, increase) in [(0, 210, 211, 212), (1, 213, 214, 215)] {
        select(197, material_index);
        set_transparency(id, w!("125"), VK_RETURN);
        assert_eq!(input_text(id), "100");
        assert_eq!(unsafe { IsWindowEnabled(combo(increase)) }, 0);
        if supports_acrylic {
            before_timeout("system material visible without the dark tint", || {
                (sample_fill() != 0).then_some(())
            });
        }
        set_transparency(id, w!("0"), VK_RETURN);
        assert_eq!(input_text(id), "0");
        assert_eq!(unsafe { IsWindowEnabled(combo(decrease)) }, 0);
        assert_ne!(unsafe { IsWindowVisible(search) }, 0);
        before_timeout("opaque black tint covers the material", || {
            (sample_fill() == 0).then_some(())
        });
    }
    assert_eq!(stored().acrylic_tint_percent, 100);
    assert_eq!(stored().mica_tint_percent, 100);

    let click_settings = |id| send(settings, WM_COMMAND, id, combo(id as i32) as isize);
    let picker = || {
        before_timeout("owned color picker popup", || {
            visible_window(w!("YiSou.Rust.Settings.ColorPicker"), pid)
        })
    };
    send(settings, WM_VSCROLL, SB_BOTTOM as usize, 0);
    let appearance_preview = combo(115);
    let dark_trigger = combo(231);
    let before = window_rect(dark_trigger);
    click_settings(231);
    let popup = picker();
    assert_eq!(unsafe { GetWindow(popup, GW_OWNER) }, settings);
    assert_eq!(unsafe { IsWindowEnabled(settings) }, 0);
    assert_eq!(combo(115), appearance_preview);
    assert!(combo(233).is_null());
    let dialog_item = |id| unsafe { GetDlgItem(popup, id) };
    let dialog_click = |id| send(popup, WM_COMMAND, id as usize, dialog_item(id) as isize);
    assert_eq!(combo_labels(dialog_item(232)), ["HEX", "RGB", "HSL"]);
    assert_eq!(control_text(dialog_item(233)), "#000000 / 0%");
    assert!(dialog_item(234).is_null());
    let row = [232, 233, 235].map(|id| window_rect(dialog_item(id)));
    for pair in row.windows(2) {
        assert!(pair[0].right < pair[1].left);
        assert!((pair[0].top + pair[0].bottom - pair[1].top - pair[1].bottom).abs() <= 2);
    }
    let plane = dialog_item(239);
    let value_field = dialog_item(233);
    let dpi = unsafe { GetDpiForWindow(popup) } as i32;
    let point = |x, y| (x * dpi / 96 | ((y * dpi / 96) << 16)) as isize;
    send(plane, WM_LBUTTONDOWN, 1, point(100, 80));
    send(plane, WM_MOUSEMOVE, 1, point(200, 35));
    send(plane, WM_LBUTTONUP, 0, point(200, 35));
    assert_eq!(dialog_item(233), value_field);
    assert_ne!(control_text(value_field), "#000000 / 0%");
    let mut graph = RECT::default();
    assert_ne!(unsafe { GetClientRect(plane, &mut graph) }, 0);
    let px = |dip| (dip * dpi + 95) / 96;
    let track_left = px(51);
    let track_right = graph.right - 8 * dpi / 96;
    let click_track = |x: i32, y| {
        let point = (x | (px(y) << 16)) as isize;
        send(plane, WM_LBUTTONDOWN, 1, point);
        send(plane, WM_LBUTTONUP, 0, point);
    };
    for y in [187, 213] {
        if y == 213 {
            dialog_click(240);
        }
        for (x, outside) in [
            (track_left, px(42)..track_left - 1),
            (track_right, track_right + 1..graph.right - 1),
        ] {
            click_track(x, y);
            let mut points = vec![(px(35), px(225))];
            points.extend(outside.map(|x| (x, px(y))));
            let pixels = printed_client_pixels(plane, &points);
            assert!(
                pixels.iter().all(|&pixel| pixel == pixels[0]),
                "slider {y} overhangs at x={x}"
            );
            if y == 213 {
                assert_eq!(
                    control_text(value_field),
                    if x == track_left {
                        "#FF3B30 / 0%"
                    } else {
                        "#FF3B30 / 100%"
                    }
                );
                let rgb = printed_client_pixels(plane, &[(px(21), px(200))])[0];
                assert_eq!(rgb == 0x303bff, x == track_left, "opaque red swatch at 0%");
            }
        }
    }
    click_track((track_left + track_right) / 2, 213);
    let [card, top_left, top_right, bottom_left, bottom_right, track] = printed_client_pixels(
        plane,
        &[
            (px(35), px(225)),
            (track_left, px(207)),
            (track_right - 1, px(207)),
            (track_left, 219 * dpi / 96 - 1),
            (track_right - 1, 219 * dpi / 96 - 1),
            (px(56), px(213)),
        ],
    )
    .try_into()
    .unwrap();
    assert_eq!([top_left, top_right, bottom_left, bottom_right], [card; 4]);
    assert_ne!(
        track, card,
        "the transparency track remains visible inside its clip"
    );
    let swatch_width = printed_client_pixels(
        plane,
        &(0..px(46)).map(|x| (x, px(200))).collect::<Vec<_>>(),
    )
    .into_iter()
    .filter(|&pixel| pixel != card)
    .count();
    let swatch_column = printed_client_pixels(
        plane,
        &(px(175)..px(224)).map(|y| (px(21), y)).collect::<Vec<_>>(),
    );
    let first_swatch_pixel = swatch_column
        .iter()
        .position(|&pixel| pixel != card)
        .unwrap() as i32
        + px(175);
    let last_swatch_pixel = swatch_column
        .iter()
        .rposition(|&pixel| pixel != card)
        .unwrap() as i32
        + px(175);
    let swatch_height = (last_swatch_pixel - first_swatch_pixel + 1) as usize;
    assert!((first_swatch_pixel - px(181)).abs() <= 1);
    assert!((last_swatch_pixel + 1 - px(219)).abs() <= 1);
    assert!(swatch_width.abs_diff((px(219) - px(181)) as usize) <= 2);
    assert!(
        swatch_width.abs_diff(swatch_height) <= 2,
        "preview swatch is square"
    );
    dialog_click(240);
    assert_eq!(control_text(value_field), "#FF3B30 / 50%");
    let format = dialog_item(232);
    let set_format = |index| {
        send(format, CB_SETCURSEL, index, 0);
        send(
            popup,
            WM_COMMAND,
            (CBN_SELCHANGE as usize) << 16 | 232,
            format as isize,
        );
    };
    set_format(2);
    assert!(control_text(value_field).ends_with(" / 50%"));
    set_format(1);
    assert_eq!(control_text(value_field), "255, 59, 48 / 50%");
    send(value_field, WM_SETTEXT, 0, w!("43, 72, 131") as isize);
    send(value_field, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(control_text(value_field), "43, 72, 131 / 50%");
    send(
        value_field,
        WM_SETTEXT,
        0,
        w!("255, 0, 0 / invalid") as isize,
    );
    send(value_field, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(control_text(value_field), "43, 72, 131 / 50%");
    send(value_field, WM_SETTEXT, 0, w!("43, 72, 131 / 43%") as isize);
    assert_eq!(stored().search_dark_color, 0);
    dialog_click(237);
    assert_eq!(unsafe { IsWindow(popup) }, 0);
    assert_ne!(unsafe { IsWindowEnabled(settings) }, 0);
    assert_eq!(combo(115), appearance_preview);
    assert_eq!(combo(231), dark_trigger);
    let after = window_rect(dark_trigger);
    assert_eq!((after.left, after.top), (before.left, before.top));
    assert_eq!(input_text(213), "43");
    let prefs = stored();
    assert_eq!(prefs.search_dark_color, 0x2b4883);
    assert_eq!(prefs.search_light_color, 0xffffff);
    assert_eq!(prefs.acrylic_tint_percent, 100);
    assert_eq!(prefs.mica_tint_percent, 57);
    assert_eq!(prefs.recent_search_colors, [0x2b4883]);
    select(197, 0);
    if unsafe { IsWindowVisible(search) } == 0 {
        let mut reveal = launch(&[]);
        assert!(
            before_timeout("show customized search", || reveal.0.try_wait().unwrap()).success()
        );
    }
    before_timeout("custom dark color reaches the acrylic surface", || {
        (sample_fill() == 0x83482b).then_some(())
    });
    click_settings(230);
    let popup = picker();
    let value = unsafe { GetDlgItem(popup, 233) };
    assert_eq!(
        control_text(value),
        format!("#FFFFFF / {}%", input_text(210))
    );
    assert!(
        !unsafe { GetDlgItem(popup, 250) }.is_null(),
        "recent colors appear in the editor"
    );
    send(value, WM_SETTEXT, 0, w!("#FFE8D1") as isize);
    send(popup, WM_CLOSE, 0, 0);
    assert_eq!(unsafe { IsWindow(popup) }, 0);
    assert_eq!(stored().search_light_color, 0xffffff);

    select(197, 2);
    click_settings(230);
    let popup = picker();
    assert_eq!(control_text(unsafe { GetDlgItem(popup, 233) }), "#FFFFFF");
    send(popup, WM_CLOSE, 0, 0);
    if unsafe { IsWindowVisible(search) } == 0 {
        let mut reveal = launch(&[]);
        assert!(before_timeout("show solid search", || reveal.0.try_wait().unwrap()).success());
    }
    before_timeout("solid search is visible", || {
        visible_window(w!("YiSou.Rust.Search"), pid)
    });
    send(search, WM_NULL, 0, 0);
    let trace_len = std::fs::read_to_string(&trace).unwrap().len();
    send(search, WM_CLOSE, 0, 0);
    before_timeout("release graphics while hidden", || {
        let events = std::fs::read_to_string(&trace).unwrap();
        events[trace_len..]
            .contains(",after_idle_release,")
            .then_some(())
    });
    let mut reopen = launch(&[]);
    assert!(before_timeout("reopen process exits", || reopen.0.try_wait().unwrap()).success());
    let reopened = before_timeout("repaint solid search before revealing", || {
        let events = std::fs::read_to_string(&trace).unwrap();
        events[trace_len..]
            .contains(",search_shown,")
            .then_some(events)
    });
    let events = &reopened[trace_len..];
    assert_ready_before_reveal(
        events,
        if events.contains(",backdrop_solid_composed,") {
            "backdrop_solid_composed"
        } else {
            "first_paint"
        },
    );
}

#[test]
fn native_search_foreground_colors_and_font_size() {
    use yisou::settings::{Language, SearchMaterial, SearchTone, Settings};

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let mut defaults = Settings::default();
    let prefs = &mut defaults.preferences;
    prefs.language = Language::Chinese;
    prefs.default_engine = "google".into();
    prefs.search_material = SearchMaterial::Solid;
    prefs.search_tone = SearchTone::Light;
    prefs.screen_background = ScreenBackground::Dim;
    prefs.multicolor_engine_icons = false;
    std::fs::write(&config, serde_json::to_vec(&defaults).unwrap()).unwrap();
    let app = AppProcess(
        Command::new(executable())
            .env("LOCALAPPDATA", data.path())
            .arg("--config")
            .arg(&config)
            .arg("--settings")
            .spawn()
            .unwrap(),
    );
    let pid = app.0.id();
    let settings = before_timeout("settings window", || {
        visible_window(w!("YiSou.Rust.Settings"), pid)
    });
    let item = |id| unsafe { GetDlgItem(settings, id) };
    let nav = item(1);
    send(nav, LB_SETCURSEL, 1, 0);
    send(
        settings,
        WM_COMMAND,
        (LBN_SELCHANGE as usize) << 16 | 1,
        nav as isize,
    );
    let click = |id| send(settings, WM_COMMAND, id as usize, item(id) as isize);
    let open_color = |id| {
        click(id);
        before_timeout("owned foreground color picker", || {
            visible_window(w!("YiSou.Rust.Settings.ColorPicker"), pid)
        })
    };
    let color_item = |popup, id| unsafe { GetDlgItem(popup, id) };
    let color_click = |popup, id| {
        send(
            popup,
            WM_COMMAND,
            id as usize,
            color_item(popup, id) as isize,
        )
    };
    let stored = || {
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .preferences
    };

    let preview = item(115);
    let preview_height = height(preview);
    let window_width = |hwnd| {
        let bounds = window_rect(hwnd);
        bounds.right - bounds.left
    };
    let default_preview_width = window_width(preview);
    let width_input = item(306);
    assert_eq!(control_text(width_input), "580");
    click(308);
    assert_eq!(stored().search_width, 581);
    click(307);
    assert_eq!(stored().search_width, 580);
    let set_width = |text: *const u16| {
        send(width_input, WM_SETTEXT, 0, text as isize);
        send(width_input, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    };
    set_width(w!("360"));
    assert_eq!(stored().search_width, 360);
    assert_eq!(item(115), preview);
    assert!(window_width(preview) < default_preview_width);

    let search_class = w!("YiSou.Rust.Search");
    let popup_class = w!("YiSou.Rust.Engines");
    let search = window_for_process(search_class, pid).unwrap();
    send(search, WM_APP + 1, 0, 0);
    before_timeout("narrow search window", || visible_window(search_class, pid));
    let edit = unsafe { GetDlgItem(search, 100) };
    let icon = unsafe { GetDlgItem(search, 101) };
    let dpi = unsafe { GetDpiForWindow(search) } as i32;
    let px = |dip| (dip * dpi + 48) / 96;
    assert_eq!(window_width(search), px(360));
    assert_eq!(window_width(icon), height(icon));
    let narrow_edit_width = window_width(edit);
    send(edit, WM_SETTEXT, 0, w!("#") as isize);
    let popup = before_timeout("alias panel follows narrow search box", || {
        visible_window(popup_class, pid)
    });
    assert_eq!(window_width(popup), px(360));
    send(edit, WM_SETTEXT, 0, w!("") as isize);
    set_width(w!("2400"));
    assert_eq!(stored().search_width, 1600);
    assert_eq!(control_text(width_input), "1600");
    assert_eq!(unsafe { IsWindowEnabled(item(308)) }, 0);
    let monitor_width = {
        use windows_sys::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        assert_ne!(
            unsafe {
                GetMonitorInfoW(
                    MonitorFromWindow(search, MONITOR_DEFAULTTONEAREST),
                    &mut info,
                )
            },
            0
        );
        info.rcWork.right - info.rcWork.left
    };
    assert_eq!(window_width(search), px(1600).min(monitor_width));
    set_width(w!("580"));
    assert_eq!(window_width(search), px(580));
    assert!(window_width(edit) > narrow_edit_width);
    assert_eq!(window_width(preview), default_preview_width);

    assert!(item(216).is_null());
    click(261);
    let font_size = item(216);
    assert_eq!(item(115), preview);
    assert!(item(197).is_null());
    click(218);
    assert_eq!(stored().search_font_size, 21);
    send(font_size, WM_SETTEXT, 0, w!("48") as isize);
    send(font_size, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(stored().search_font_size, 48);
    assert_eq!(item(216), font_size);
    assert_eq!(item(115), preview);
    assert!(height(preview) > preview_height);
    send(font_size, WM_SETTEXT, 0, w!("120") as isize);
    send(font_size, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(stored().search_font_size, 72);
    assert_eq!(unsafe { IsWindowEnabled(item(218)) }, 0);
    send(font_size, WM_SETTEXT, 0, w!("48") as isize);
    send(font_size, WM_KEYDOWN, usize::from(VK_RETURN), 0);

    let popup = open_color(258);
    assert!(control_text(color_item(popup, 233)).ends_with(" / 0%"));
    color_click(popup, 237);
    assert_eq!(stored().search_text_color, None);
    let popup = open_color(258);
    let format = color_item(popup, 232);
    send(format, CB_SETCURSEL, 2, 0);
    send(
        popup,
        WM_COMMAND,
        (CBN_SELCHANGE as usize) << 16 | 232,
        format as isize,
    );
    color_click(popup, 237);
    assert_eq!(stored().search_text_color, None);
    let popup = open_color(258);
    send(
        color_item(popup, 233),
        WM_SETTEXT,
        0,
        w!("#B000E0 / 30%") as isize,
    );
    color_click(popup, 237);
    assert_eq!(stored().search_text_color, Some(0xb000e0));
    assert_eq!(stored().search_text_transparency, 30);

    send(item(261), WM_KEYDOWN, 0x27, 0);
    before_timeout("right arrow selects the engine tab", || {
        (!item(117).is_null()).then_some(())
    });
    assert_eq!(item(115), preview);
    assert!(item(216).is_null());
    let popup = open_color(259);
    assert!(control_text(color_item(popup, 233)).ends_with(" / 0%"));
    color_click(popup, 243);
    send(
        color_item(popup, 233),
        WM_SETTEXT,
        0,
        w!("#34C759 / 20%") as isize,
    );
    color_click(popup, 237);
    assert_eq!(stored().search_mono_icon_color, Some(0x34c759));
    assert_eq!(stored().search_mono_icon_transparency, 20);

    let search = window_for_process(w!("YiSou.Rust.Search"), pid).unwrap();
    let show = || {
        if unsafe { IsWindowVisible(search) } == 0 {
            send(search, WM_APP + 1, 0, 0);
        }
        before_timeout("search window", || {
            visible_window(w!("YiSou.Rust.Search"), pid)
        });
    };
    show();
    let edit = unsafe { GetDlgItem(search, 100) };
    let icon = unsafe { GetDlgItem(search, 101) };
    let width = |window| {
        let bounds = window_rect(window);
        bounds.right - bounds.left
    };
    let initial_edit_width = width(edit);
    let initial_icon_width = width(icon);
    let search_dpi = unsafe { GetDpiForWindow(search) } as i32;
    let px = |dip| (dip * search_dpi + 48) / 96;
    assert_eq!(initial_icon_width, height(icon));
    assert_eq!(initial_icon_width, px(40));
    assert!((window_rect(icon).left - window_rect(edit).right - px(8)).abs() <= 1);
    let initial_preview_width = width(preview);
    let initial_menu_names = stored().show_engine_names;
    assert!(!stored().show_search_engine_name);
    click(263);
    assert!(stored().show_search_engine_name);
    assert_eq!(stored().show_engine_names, initial_menu_names);
    assert!(width(icon) > initial_icon_width);
    assert!(width(edit) < initial_edit_width);
    assert!(width(preview) < initial_preview_width);
    assert_eq!(item(115), preview);
    let search_bounds = window_rect(search);
    let button = window_rect(icon);
    assert_eq!(search_bounds.right - button.right, px(12));
    let name_pixels: Vec<_> = (button.top + 4..button.bottom - 4)
        .flat_map(|y| {
            (button.left + initial_icon_width..button.right - 4)
                .map(move |x| (x - search_bounds.left, y - search_bounds.top))
        })
        .collect();
    before_timeout("engine name is drawn beside the icon", || {
        printed_pixels(search, &name_pixels, 2)
            .into_iter()
            .any(|rgb| rgb.to_le_bytes()[..3].iter().all(|&channel| channel < 128))
            .then_some(())
    });
    let last_name_pixels: Vec<_> = (button.top + 4..button.bottom - 4)
        .flat_map(|y| {
            (button.right - px(9)..button.right - px(1))
                .map(move |x| (x - search_bounds.left, y - search_bounds.top))
        })
        .collect();
    assert!(
        printed_pixels(search, &last_name_pixels, 2)
            .into_iter()
            .any(|rgb| { rgb.to_le_bytes()[..3].iter().all(|&channel| channel < 128) }),
        "name ends close to the configured right padding"
    );
    send(edit, WM_SETTEXT, 0, w!("#bd query") as isize);
    assert!(control_text(icon).contains("百度"));
    assert!(width(icon) < button.right - button.left);
    assert_eq!(window_rect(icon).right, button.right);
    send(edit, WM_SETTEXT, 0, w!("") as isize);
    click(263);
    assert!(!stored().show_search_engine_name);
    assert_eq!(width(edit), initial_edit_width);
    assert_eq!(width(icon), initial_icon_width);
    assert_eq!(width(preview), initial_preview_width);
    let large_input_height = height(edit);
    send(edit, WM_SETTEXT, 0, w!("鬱鑿齒 Search") as isize);
    let contains_color = |window, rgb: u32| {
        let bounds = window_rect(window);
        let origin = window_rect(search);
        let width = (bounds.right - bounds.left).min(225);
        let height = bounds.bottom - bounds.top;
        let points: Vec<_> = (bounds.top - origin.top..bounds.top - origin.top + height)
            .flat_map(|y| {
                (bounds.left - origin.left..bounds.left - origin.left + width).map(move |x| (x, y))
            })
            .collect();
        let [b, g, r, _] = rgb.to_le_bytes();
        printed_pixels(search, &points, 2).into_iter().any(|pixel| {
            let [actual_r, actual_g, actual_b, _] = pixel.to_le_bytes();
            r.abs_diff(actual_r)
                .max(g.abs_diff(actual_g))
                .max(b.abs_diff(actual_b))
                <= 4
        })
    };
    before_timeout("custom text and monochrome icon are visible", || {
        (contains_color(edit, 0xc84de9) && contains_color(icon, 0x5dd27a)).then_some(())
    });

    click(117);
    assert_eq!(unsafe { IsWindowEnabled(item(259)) }, 0);
    click(259);
    assert!(window_for_process(w!("YiSou.Rust.Settings.ColorPicker"), pid).is_none());
    assert_eq!(stored().search_mono_icon_color, Some(0x34c759));
    assert_eq!(stored().search_mono_icon_transparency, 20);
    show();
    before_timeout("multicolor Google icon retains its original yellow", || {
        (!contains_color(icon, 0x5dd27a) && contains_color(icon, 0xfbbc05)).then_some(())
    });

    click(117);
    assert_ne!(unsafe { IsWindowEnabled(item(259)) }, 0);
    show();
    before_timeout("monochrome icon uses the saved color again", || {
        contains_color(icon, 0x5dd27a).then_some(())
    });
    click(261);
    let popup = open_color(258);
    color_click(popup, 240);
    color_click(popup, 236);
    assert_eq!(stored().search_text_color, Some(0xb000e0));
    let popup = open_color(258);
    color_click(popup, 240);
    send(
        color_item(popup, 233),
        WM_KEYDOWN,
        usize::from(VK_ESCAPE),
        0,
    );
    before_timeout("Escape closes the picker", || {
        (unsafe { IsWindow(popup) } == 0).then_some(())
    });
    assert_ne!(unsafe { IsWindowEnabled(settings) }, 0);
    assert_eq!(stored().search_text_color, Some(0xb000e0));
    for (tab, id) in [(261, 258), (262, 259)] {
        click(tab);
        let popup = open_color(id);
        color_click(popup, 238);
        color_click(popup, 237);
    }
    click(261);
    send(item(216), WM_SETTEXT, 0, w!("20") as isize);
    send(item(216), WM_KEYDOWN, usize::from(VK_RETURN), 0);
    show();
    assert!(height(edit) < large_input_height);
    let prefs = stored();
    assert_eq!(prefs.search_text_color, None);
    assert_eq!(prefs.search_mono_icon_color, None);
    assert_eq!(prefs.search_text_transparency, 0);
    assert_eq!(prefs.search_mono_icon_transparency, 0);
    assert_eq!(prefs.search_font_size, 20);
    assert_eq!(prefs.recent_search_colors, [0x34c759, 0xb000e0]);

    click(260);
    let settings_dpi = unsafe { GetDpiForWindow(settings) } as i32;
    let settings_px = |dip| (dip * settings_dpi + 48) / 96;
    let find_label = |text| {
        let mut child = unsafe { GetWindow(settings, GW_CHILD) };
        while !child.is_null() {
            if control_text(child) == text {
                return child;
            }
            child = unsafe { GetWindow(child, GW_HWNDNEXT) };
        }
        panic!("missing padding label {text}");
    };
    let labels = ["上", "下", "左", "右"].map(find_label);
    let heading = find_label("搜索框内边距");
    let assert_columns = || {
        let card = window_rect(item(192));
        for (label, plus) in [(labels[0], 275), (labels[2], 281)] {
            let right = card.right - window_rect(item(plus)).right;
            assert_eq!(window_rect(label).left, window_rect(heading).left);
            assert!((right - settings_px(13)).abs() <= 1);
        }
        for (label, minus) in labels.into_iter().zip([271, 274, 277, 280]) {
            let gap = window_rect(item(minus)).left - window_rect(label).right;
            assert!((gap - settings_px(16)).abs() <= 1);
        }
    };
    let settings_bounds = window_rect(settings);
    let resize = |width| unsafe {
        assert_ne!(
            SetWindowPos(
                settings,
                std::ptr::null_mut(),
                0,
                0,
                width,
                settings_bounds.bottom - settings_bounds.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            ),
            0
        );
    };
    assert_columns();
    resize(settings_px(680));
    assert_columns();
    resize(settings_bounds.right - settings_bounds.left);
    let set_padding = |id, value: &str| {
        let value: Vec<_> = value.encode_utf16().chain([0]).collect();
        send(item(id), WM_SETTEXT, 0, value.as_ptr() as isize);
        send(item(id), WM_KEYDOWN, usize::from(VK_RETURN), 0);
    };
    set_padding(270, "25");
    set_padding(273, "999");
    set_padding(276, "37");
    set_padding(279, "44");
    click(274);
    assert_eq!(
        stored().search_padding,
        yisou::settings::SearchPadding {
            top: 25,
            bottom: 63,
            left: 37,
            right: 44,
        }
    );
    show();
    let bounds = window_rect(search);
    let icon_bounds = window_rect(icon);
    assert_eq!(window_rect(edit).left - bounds.left, px(37));
    assert!((bounds.right - icon_bounds.right - px(44)).abs() <= 1);
    assert_eq!(icon_bounds.top - bounds.top, px(25));
    assert!((bounds.bottom - icon_bounds.bottom - px(63)).abs() <= 1);
    assert_eq!(item(115), preview);
}

fn control_text(hwnd: HWND) -> String {
    let mut text = vec![0u16; send(hwnd, WM_GETTEXTLENGTH, 0, 0) + 1];
    let len = send(hwnd, WM_GETTEXT, text.len(), text.as_mut_ptr() as isize);
    String::from_utf16(&text[..len]).unwrap()
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

fn combo_labels(combo: HWND) -> Vec<String> {
    (0..send(combo, CB_GETCOUNT, 0, 0))
        .map(|index| {
            let len = send(combo, CB_GETLBTEXTLEN, index, 0);
            let mut text = vec![0u16; len + 1];
            assert_eq!(
                send(combo, CB_GETLBTEXT, index, text.as_mut_ptr() as isize),
                len
            );
            String::from_utf16(&text[..len]).unwrap()
        })
        .collect()
}

fn printed_client_pixels(window: HWND, points: &[(i32, i32)]) -> Vec<u32> {
    printed_pixels(window, points, 1)
}

fn printed_pixels(window: HWND, points: &[(i32, i32)], flags: u32) -> Vec<u32> {
    use windows_sys::Win32::Graphics::Gdi::*;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PrintWindow(hwnd: HWND, dc: HDC, flags: u32) -> i32;
    }

    unsafe {
        let mut bounds = RECT::default();
        assert_ne!(GetClientRect(window, &mut bounds), 0);
        let screen = GetDC(std::ptr::null_mut());
        assert!(!screen.is_null());
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, bounds.right, bounds.bottom);
        ReleaseDC(std::ptr::null_mut(), screen);
        assert!(!dc.is_null() && !bitmap.is_null());
        let previous = SelectObject(dc, bitmap);
        let printed = PrintWindow(window, dc, flags);
        let pixels: Vec<_> = points.iter().map(|&(x, y)| GetPixel(dc, x, y)).collect();
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        assert_ne!(printed, 0);
        assert!(!pixels.contains(&CLR_INVALID));
        pixels
    }
}

#[test]
fn native_search_popup_preferences_and_second_launch() {
    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
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
    let executable = executable();
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
    assert!(screen_shades(search).contains(&shade));
    let edit = unsafe { GetDlgItem(search, 100) };
    let engine = unsafe { GetDlgItem(search, 101) };
    let button = window_rect(engine);
    let input = window_rect(edit);
    let dpi = unsafe { GetDpiForWindow(search) } as i32;
    let px = |dip| (dip * dpi + 48) / 96;
    assert_eq!(button.right - button.left, button.bottom - button.top);
    assert_eq!(button.bottom - button.top, px(40));
    assert_eq!(button.left - input.right, px(8));
    assert_eq!(button.right - input.left, px(568) - px(20));
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
            text_routes(),
            initial_routes,
            "IME uses the same text renderer"
        );
        send(edit, WM_IME_ENDCOMPOSITION, 0, 0);
        paint();
        assert_eq!(text_routes(), initial_routes);
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
    assert_eq!(
        unsafe { GetNextDlgTabItem(settings, language, 0) },
        apply_language
    );
    let settings_dpi = unsafe { GetDpiForWindow(settings) } as i32;
    let settings_pixels = |dip| (dip * settings_dpi + 48) / 96;
    let assert_control_height = |id| {
        let control = item(id);
        let bounds = window_rect(control);
        let actual = bounds.bottom - bounds.top;
        assert!(
            (actual - settings_pixels(32)).abs() <= 1,
            "control {id}: expected 32 DIP at {settings_dpi} DPI, got {actual} px",
        );
    };
    let assert_dropdown_height = |id| {
        assert_control_height(id);
        assert_eq!(
            send(item(id), CB_GETITEMHEIGHT, 0, 0),
            settings_pixels(32) as usize,
            "dropdown {id} option height",
        );
    };
    let assert_edit_frame = |id, button: Option<i32>| {
        let edit = window_rect(item(id));
        // The native text area is inset 5 DIP from the painted frame on each side vertically.
        let inset = settings_pixels(5);
        let frame_top = edit.top - inset;
        let frame_bottom = edit.bottom + inset;
        assert!(
            (frame_bottom - frame_top - settings_pixels(32)).abs() <= 1,
            "input {id}: expected a 32 DIP frame",
        );
        if let Some(button) = button {
            let button = window_rect(item(button));
            assert_eq!((frame_top, frame_bottom), (button.top, button.bottom));
        }
    };

    for (page, buttons) in [
        &[102, 104][..],
        &[260, 261, 262],
        &[122],
        &[134],
        &[140, 142],
        &[150, 153, 154, 155],
        if cfg!(feature = "ocr") {
            &[170, 171, 172, 173, 174, 175]
        } else {
            &[162]
        },
    ]
    .into_iter()
    .chain(cfg!(feature = "ocr").then_some(&[162][..]))
    .enumerate()
    {
        open_page(page);
        for &id in buttons {
            assert_control_height(id);
        }
        match page {
            0 => {
                assert_dropdown_height(103);
                assert_eq!(window_rect(item(103)).top, window_rect(item(104)).top);
            }
            1 => {
                assert_dropdown_height(197);
                send(item(261), BM_CLICK, 0, 0);
                assert_control_height(111);
                assert_edit_frame(216, Some(217));
                send(item(262), BM_CLICK, 0, 0);
                for id in [110, 179, 180, 263] {
                    assert_control_height(id);
                }
                assert_edit_frame(178, Some(179));
            }
            2 => assert_edit_frame(27, None),
            3 => {
                assert_dropdown_height(135);
                assert_dropdown_height(136);
                assert_edit_frame(23, Some(191));
            }
            5 => {
                assert_edit_frame(25, None);
                assert_dropdown_height(26);
                assert_dropdown_height(29);
            }
            6 if cfg!(feature = "ocr") => assert_edit_frame(40, Some(171)),
            _ if !item(166).is_null() => assert_edit_frame(166, Some(165)),
            _ => {}
        }
    }
    open_page(2);
    let list = item(10);
    let control_width = |id| {
        let bounds = window_rect(item(id));
        bounds.right - bounds.left
    };
    let heading = window_rect(item(192));
    let add_engine = window_rect(item(122));
    assert!(heading.right < add_engine.left);
    let scrollbar = item(28);
    let before_scroll = window_rect(item(27));
    let list_rect = window_rect(list);
    assert!(before_scroll.bottom + settings_pixels(6) < list_rect.top);
    assert_eq!(
        send(list, LB_GETITEMHEIGHT, 0, 0),
        settings_pixels(59) as usize
    );
    assert_eq!(
        height(list) as usize,
        send(list, LB_GETCOUNT, 0, 0) * send(list, LB_GETITEMHEIGHT, 0, 0)
    );
    let sidebar = window_rect(nav);
    let track_bottom = ((height(scrollbar) - 2) as usize) << 16;
    assert_ne!(unsafe { IsWindowVisible(scrollbar) }, 0);
    send(scrollbar, WM_LBUTTONDOWN, 1, track_bottom as isize);
    assert!(window_rect(item(27)).top < before_scroll.top);
    assert_eq!(window_rect(nav).top, sidebar.top);
    assert_eq!(send(list, LB_GETTOPINDEX, 0, 0), 0);

    send(settings, WM_VSCROLL, SB_BOTTOM as usize, 0);
    let last = send(list, LB_GETCOUNT, 0, 0) - 1;
    send(list, LB_SETCURSEL, last, 0);
    send(settings, WM_COMMAND, 132, 0);
    let menu_class = w!("YiSou.Rust.Settings.Menu");
    let menu = before_timeout("menu at the bottom of settings", || {
        visible_window(menu_class, pid)
    });
    let mut viewport = RECT::default();
    unsafe {
        use windows_sys::Win32::Graphics::Gdi::MapWindowPoints;
        assert_ne!(GetClientRect(settings, &mut viewport), 0);
        MapWindowPoints(
            settings,
            std::ptr::null_mut(),
            (&mut viewport as *mut RECT).cast(),
            2,
        );
    }
    let popup = window_rect(menu);
    let row_top = window_rect(list).top + (last * send(list, LB_GETITEMHEIGHT, 0, 0)) as i32;
    assert!(popup.left >= viewport.left && popup.right <= viewport.right);
    assert!(popup.top >= viewport.top && popup.bottom <= row_top);
    send(menu, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);

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
    send(item(261), BM_CLICK, 0, 0);
    assert!(!item(113).is_null());
    assert!(window_rect(item(115)).bottom < window_rect(item(111)).top);
    send(item(262), BM_CLICK, 0, 0);
    let names_toggle = item(110);
    assert!(window_rect(names_toggle).top >= window_rect(item(117)).bottom);
    assert!(stored().show_engine_names);
    send(names_toggle, BM_CLICK, 0, 0);
    assert!(!stored().show_engine_names);
    send(names_toggle, BM_CLICK, 0, 0);
    assert!(stored().show_engine_names);
    assert!(
        !JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .preferences
            .multicolor_engine_icons
    );
    send(item(117), BM_CLICK, 0, 0);
    assert!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .preferences
            .multicolor_engine_icons
    );
    let icon_size = item(178);
    assert!(window_rect(item(115)).bottom < window_rect(item(117)).top);
    assert!(window_rect(item(117)).bottom < window_rect(icon_size).top);
    assert!(window_rect(icon_size).bottom < window_rect(names_toggle).top);
    let icon_size_text = |control| {
        let mut buffer = [0u16; 32];
        let length = send(
            control,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
        );
        String::from_utf16(&buffer[..length]).unwrap()
    };
    assert_eq!(icon_size_text(icon_size), "20");
    let decrease = window_rect(item(179));
    let increase = window_rect(item(180));
    for button in [decrease, increase] {
        assert_eq!(button.right - button.left, button.bottom - button.top);
        assert!((button.bottom - button.top - settings_pixels(32)).abs() <= 1);
    }
    assert_eq!(
        (decrease.top, decrease.bottom),
        (increase.top, increase.bottom)
    );
    send(icon_size, EM_SETSEL, 0, -1);
    send(icon_size, WM_CHAR, usize::from(b'1'), 0);
    send(icon_size, WM_CHAR, usize::from(b'6'), 0);
    assert_eq!(icon_size_text(icon_size), "16");
    assert_eq!(stored().search_engine_icon_size, 20);
    send(icon_size, WM_SETTEXT, 0, w!("31") as isize);
    assert_eq!(stored().search_engine_icon_size, 20);
    send(icon_size, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert_eq!(icon_size_text(icon_size), "20");
    send(icon_size, WM_SETTEXT, 0, w!("31") as isize);
    send(icon_size, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(stored().search_engine_icon_size, 31);
    send(icon_size, WM_SETTEXT, 0, w!("0") as isize);
    send(icon_size, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(icon_size_text(icon_size), "31");
    send(icon_size, WM_SETTEXT, 0, w!("100") as isize);
    send(icon_size, WM_KILLFOCUS, 0, 0);
    assert_eq!(stored().search_engine_icon_size, 36);
    assert_eq!(icon_size_text(icon_size), "36");
    open_page(0);
    open_page(1);
    assert_eq!(icon_size_text(item(178)), "36");
    assert_eq!(unsafe { IsWindowEnabled(item(180)) }, 0);
    send(item(179), BM_CLICK, 0, 0);
    assert_eq!(icon_size_text(item(178)), "35");
    assert_ne!(unsafe { IsWindowEnabled(item(180)) }, 0);
    send(item(180), BM_CLICK, 0, 0);
    assert_eq!(stored().search_engine_icon_size, 36);

    let set_language = |selection| {
        open_page(0);
        let language = item(103);
        send(language, CB_SETCURSEL, selection, 0);
        send(
            settings,
            WM_COMMAND,
            (CBN_SELCHANGE as usize) << 16 | 103,
            language as isize,
        );
        send(item(104), BM_CLICK, 0, 0);
        let apply_width = control_width(104);
        open_page(1);
        send(item(261), BM_CLICK, 0, 0);
        apply_width
    };
    let filter_arial = || {
        send(item(186), WM_SETTEXT, 0, w!("Arial") as isize);
        before_timeout("system font filter", || {
            let count = send(item(181), CB_GETCOUNT, 0, 0);
            (count > 0 && count < 10).then_some(())
        });
        assert_eq!(combo_labels(item(181))[0], "Arial");
        send(item(181), CB_SETCURSEL, 0, 0);
        send(
            settings,
            WM_COMMAND,
            (CBN_SELCHANGE as usize) << 16 | 181,
            item(181) as isize,
        );
    };
    let chinese_apply_width = set_language(1);
    send(item(111), BM_CLICK, 0, 0);
    for id in [181, 182, 183] {
        assert_dropdown_height(id);
    }
    assert_eq!(window_rect(item(183)).top, window_rect(item(193)).top);
    assert_edit_frame(184, Some(193));
    assert_edit_frame(186, None);
    assert!(send(item(181), CB_GETCOUNT, 0, 0) > 10);
    assert!(!item(189).is_null(), "Windows font settings link");
    assert!((control_width(188) - chinese_apply_width).abs() <= 1);
    let cancel = window_rect(item(187));
    let apply = window_rect(item(188));
    assert!((apply.left - cancel.right - settings_pixels(8)).abs() <= 1);
    for (button, equivalent) in [(193, decrease), (194, increase)] {
        let bounds = window_rect(item(button));
        let edge = equivalent.right - equivalent.left;
        assert!((control_width(button) - edge).abs() <= 1);
        assert!((bounds.bottom - bounds.top - edge).abs() <= 1);
    }
    send(item(194), BM_CLICK, 0, 0);
    assert_eq!(icon_size_text(item(184)), "21");
    send(item(193), BM_CLICK, 0, 0);
    assert_eq!(icon_size_text(item(184)), "20");
    send(item(184), WM_SETTEXT, 0, w!("8") as isize);
    assert_eq!(unsafe { IsWindowEnabled(item(193)) }, 0);
    send(item(184), WM_SETTEXT, 0, w!("72") as isize);
    assert_eq!(unsafe { IsWindowEnabled(item(194)) }, 0);
    assert_eq!(stored().search_font_size, 20);
    filter_arial();
    let chinese_faces = combo_labels(item(182));
    assert!(chinese_faces.len() >= 2);
    assert!(
        chinese_faces
            .iter()
            .all(|name| name.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)))
    );
    send(item(184), WM_SETTEXT, 0, w!("32") as isize);
    send(item(187), BM_CLICK, 0, 0);
    assert_eq!(stored().search_font_size, 20);
    let english_apply_width = set_language(2);
    assert!(english_apply_width > chinese_apply_width);
    send(item(111), BM_CLICK, 0, 0);
    assert!((control_width(188) - english_apply_width).abs() <= 1);
    filter_arial();
    let english_faces = combo_labels(item(182));
    assert_eq!(chinese_faces.len(), english_faces.len());
    assert!(english_faces.iter().any(|name| name == "Bold"));
    send(item(187), BM_CLICK, 0, 0);
    set_language(0);
    send(item(111), BM_CLICK, 0, 0);
    send(item(184), WM_SETTEXT, 0, w!("32") as isize);
    send(item(188), BM_CLICK, 0, 0);
    assert_eq!(stored().search_font_size, 32);
    assert!(matches!(stored().font, yisou::settings::Font::System(_)));
    send(item(112), BM_CLICK, 0, 0);
    assert_eq!(stored().search_font_size, 20);
    assert_eq!(stored().font, yisou::settings::Font::Default);
    open_page(2);
    assert_eq!(send(item(10), LB_GETCOUNT, 0, 0), 1);
    send(item(10), LB_SETCURSEL, 0, 0);
    let load = || JsonSettings::new(config.clone()).load().unwrap();
    send(item(10), WM_KEYDOWN, 0x20, 0);
    before_timeout("disabled engine persisted", || {
        (!load().engine_enabled("baidu")).then_some(())
    });
    assert_eq!(stored().default_engine, "bing");
    assert_eq!(stored().pins, ["baidu"]);
    send(item(10), WM_KEYDOWN, 0x20, 0);
    before_timeout("enabled engine persisted", || {
        load().engine_enabled("baidu").then_some(())
    });
    send(settings, WM_COMMAND, 120, 0);
    send(item(27), WM_SETTEXT, 0, w!("") as isize);
    before_timeout("unfiltered engines", || {
        (send(item(10), LB_GETCOUNT, 0, 0) > 2).then_some(())
    });
    let list = item(10);
    let row = send(list, LB_GETITEMHEIGHT, 0, 0);
    let grip = (16 * unsafe { GetDpiForWindow(list) } / 96) as usize;
    send(list, WM_LBUTTONDOWN, 1, ((row / 2) << 16 | grip) as isize);
    send(
        list,
        WM_MOUSEMOVE,
        1,
        (((row * 3) / 2) << 16 | grip) as isize,
    );
    send(
        list,
        WM_LBUTTONUP,
        0,
        (((row * 3) / 2) << 16 | grip) as isize,
    );
    assert_eq!(&load().engine_order[..2], &["baidu", "bing"]);
    send(item(27), WM_SETTEXT, 0, w!("#bd") as isize);
    before_timeout("filtered engines after reorder", || {
        (send(item(10), LB_GETCOUNT, 0, 0) == 1).then_some(())
    });
    send(item(10), LB_SETCURSEL, 0, 0);
    let open_engine_menu = || {
        send(settings, WM_COMMAND, 132, 0);
        before_timeout("engine actions menu", || visible_window(menu_class, pid))
    };
    let list = item(10);
    let list_bounds = window_rect(list);
    let list_dpi = unsafe { GetDpiForWindow(list) };
    let x = list_bounds.right - list_bounds.left - (20 * list_dpi / 96) as i32;
    let y = send(list, LB_GETITEMHEIGHT, 0, 0) / 2;
    let click_more = || {
        let point = ((y << 16) | x as usize) as isize;
        send(list, WM_LBUTTONDOWN, 1, point);
        send(list, WM_LBUTTONUP, 0, point);
    };
    click_more();
    before_timeout("more button opens engine menu", || {
        visible_window(menu_class, pid)
    });
    click_more();
    before_timeout("more button closes engine menu", || {
        window_for_process(menu_class, pid).is_none().then_some(())
    });
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
    let alias = before_timeout("built-in engine editor", || {
        let alias = item(21);
        (!alias.is_null()
            && [20, 22, 127, 116, 119]
                .into_iter()
                .all(|id| !item(id).is_null()))
        .then_some(alias)
    });
    send(item(20), WM_SETTEXT, 0, w!("Baidu test") as isize);
    send(
        item(22),
        WM_SETTEXT,
        0,
        w!("https://example.org/?q=%s") as isize,
    );
    send(alias, WM_SETTEXT, 0, w!("#g") as isize);
    assert_eq!(unsafe { IsWindowEnabled(item(126)) }, 0);
    assert!(send(item(133), WM_GETTEXTLENGTH, 0, 0) > 0);
    assert!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .built_in_aliases
            .is_empty()
    );
    send(alias, WM_SETTEXT, 0, w!("#BDTest") as isize);
    assert_ne!(unsafe { IsWindowEnabled(item(126)) }, 0);
    send(item(126), BM_CLICK, 0, 0);
    assert!(item(21).is_null());
    assert_eq!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .built_in_aliases["baidu"],
        "bdtest"
    );
    let saved = JsonSettings::new(config.clone()).load().unwrap();
    assert_eq!(saved.built_in_overrides["baidu"].name, "Baidu test");
    assert_eq!(
        saved.built_in_overrides["baidu"].url,
        "https://example.org/?q=%s"
    );
    assert_eq!(send(item(10), LB_GETCOUNT, 0, 0), 1);
    assert_eq!(send(item(10), LB_SETCURSEL, 0, 0), 0);
    assert_eq!(send(item(10), LB_GETCURSEL, 0, 0), 0);
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
    assert!(
        JsonSettings::new(config.clone())
            .load()
            .unwrap()
            .built_in_overrides
            .is_empty()
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
    let mut client = RECT::default();
    assert_ne!(unsafe { GetClientRect(combo.hwndList, &mut client) }, 0);
    assert_eq!(
        unsafe { GetWindowLongW(combo.hwndList, GWL_STYLE) } as u32 & WS_VSCROLL,
        0
    );
    send(combo.hwndList, LB_SETTOPINDEX, 0, 0);
    let rail = (client.right - px(6)) as isize;
    send(
        combo.hwndList,
        WM_LBUTTONDOWN,
        1,
        (isize::try_from(px(10)).unwrap() << 16) | rail,
    );
    let bottom = ((client.bottom - 2) as isize) << 16 | rail;
    send(combo.hwndList, WM_MOUSEMOVE, 1, bottom);
    send(combo.hwndList, WM_LBUTTONUP, 0, bottom);
    let dragged = send(combo.hwndList, LB_GETTOPINDEX, 0, 0);
    assert!(dragged > 0);
    assert_ne!(send(selection_engine, CB_GETDROPPEDSTATE, 0, 0), 0);
    assert_eq!(send(selection_engine, CB_GETCURSEL, 0, 0), selected);
    send(selection_engine, WM_MOUSEWHEEL, 120 << 16, 0);
    assert!(send(combo.hwndList, LB_GETTOPINDEX, 0, 0) < dragged);
    assert_eq!(send(selection_engine, CB_GETCURSEL, 0, 0), selected);
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
    let post_key = |id, key: u16| {
        assert_ne!(
            unsafe { PostMessageW(item(id), WM_KEYDOWN, usize::from(key), 0) },
            0
        );
    };
    assert_eq!(&load().engine_order[..2], &["baidu", "bing"]);
    send(item(136), CB_SETCURSEL, 1, 0);
    send(item(138), BM_CLICK, 0, 0);
    post_key(138, VK_F10);
    before_timeout("recorded selection shortcut", || stored().selection);
    send(item(134), BM_CLICK, 0, 0);
    assert_eq!(stored().selection.unwrap().engine, "bing");
    send(item(23), WM_SETTEXT, 0, w!("MSTSC.EXE") as isize);
    send(item(191), BM_CLICK, 0, 0);
    assert!(!item(2000).is_null());
    assert!(stored().excluded_apps.is_empty());
    send(item(134), BM_CLICK, 0, 0);
    assert_eq!(stored().excluded_apps, ["mstsc.exe"]);
    send(item(2000), BM_CLICK, 0, 0);
    assert!(item(2000).is_null());
    assert_eq!(stored().excluded_apps, ["mstsc.exe"]);
    send(item(134), BM_CLICK, 0, 0);
    assert!(stored().excluded_apps.is_empty());
    send(item(139), BM_CLICK, 0, 0);
    post_key(139, VK_F10);
    before_timeout("shortcut conflict", || {
        (unsafe { IsWindowVisible(item(539)) } != 0).then_some(())
    });
    assert!(stored().clipboard_shortcut.is_none());
    send(item(639), BM_CLICK, 0, 0);
    assert!(stored().selection.is_some());
    send(item(139), BM_CLICK, 0, 0);
    post_key(139, VK_F10);
    before_timeout("shortcut conflict again", || {
        (unsafe { IsWindowVisible(item(539)) } != 0).then_some(())
    });
    send(item(539), BM_CLICK, 0, 0);
    assert!(stored().selection.is_none());
    assert_eq!(stored().clipboard_shortcut.unwrap().key, u32::from(VK_F10));
    send(item(139), BM_CLICK, 0, 0);
    post_key(139, VK_ESCAPE);
    before_timeout("shortcut recording cancelled", || {
        (unsafe { IsWindowVisible(item(339)) } == 0).then_some(())
    });
    assert_eq!(stored().clipboard_shortcut.unwrap().key, u32::from(VK_F10));
    send(item(137), BM_CLICK, 0, 0);
    post_key(137, VK_BACK);
    before_timeout("main shortcut cleared", || {
        stored().main_shortcut.is_none().then_some(())
    });
    let reset = item(145);
    assert_ne!(unsafe { IsWindowVisible(reset) }, 0);
    let bounds = window_rect(reset);
    for y in [
        bounds.top + settings_pixels(4),
        bounds.bottom - settings_pixels(4),
    ] {
        let mut point = POINT {
            x: (bounds.left + bounds.right) / 2,
            y,
        };
        unsafe {
            windows_sys::Win32::Graphics::Gdi::MapWindowPoints(
                std::ptr::null_mut(),
                settings,
                &mut point,
                1,
            );
            assert_eq!(
                ChildWindowFromPointEx(settings, point, CWP_SKIPINVISIBLE),
                reset,
                "shortcut reset button is covered by another control",
            );
        }
    }
    send(reset, BM_CLICK, 0, 0);
    assert_eq!(
        stored().main_shortcut,
        Some(yisou::shortcut::Shortcut::default())
    );
    assert_eq!(unsafe { IsWindowVisible(reset) }, 0);
    send(item(248), BM_CLICK, 0, 0);
    post_key(248, VK_F9);
    before_timeout("screenshot search shortcut saved", || {
        (stored().image_search_shortcut.map(|key| key.key) == Some(u32::from(VK_F9))).then_some(())
    });
    send(item(256), BM_CLICK, 0, 0);
    assert!(stored().image_search_shortcut.is_none());
    let action = item(249);
    send(action, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    send(action, WM_KEYDOWN, usize::from(VK_DOWN), 0);
    send(action, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    assert_eq!(
        stored().image_search_action,
        yisou::settings::ImageSearchAction::Both
    );
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
            ocr_hotkey = stored().ocr_shortcut;
            assert!(ocr_hotkey.is_some());
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
    let browsers = window_rect(item(10));
    let browse = window_rect(item(142));
    assert_eq!((browsers.left, browsers.right), (browse.left, browse.right));
    assert!((browse.top - browsers.bottom).abs() <= settings_pixels(3));
    assert_eq!(unsafe { IsWindowEnabled(item(140)) }, 0);
    send(item(142), BM_CLICK, 0, 0);
    assert!(!item(10).is_null());
    assert!(!item(20).is_null());
    assert_edit_frame(20, Some(143));
    {
        use std::os::windows::ffi::OsStrExt;
        let test_browser = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/where.exe");
        let path: Vec<_> = test_browser.as_os_str().encode_wide().chain([0]).collect();
        send(item(20), WM_SETTEXT, 0, path.as_ptr() as isize);
        send(item(1441), BM_CLICK, 0, 0);
        assert!(stored().browser.is_none());
    }
    send(item(1440), BM_CLICK, 0, 0);
    assert!(item(20).is_null());
    assert!(stored().browser.is_none());
    open_page(5);
    assert_ne!(unsafe { IsWindowVisible(item(1560)) }, 0);
    for page in [2, 3, 5, 1, 2, 3, 5] {
        open_page(page);
    }
    open_page(2);
    send(item(27), WM_SETTEXT, 0, w!("") as isize);
    before_timeout("unfiltered engines for drag painting", || {
        (send(item(10), LB_GETCOUNT, 0, 0) > 2).then_some(())
    });
    let list = item(10);
    let row = send(list, LB_GETITEMHEIGHT, 0, 0);
    let grip = settings_pixels(16) as usize;
    let pointer = |y| ((y << 16) | grip) as isize;
    let offset = row / 4;
    let list_rect = window_rect(list);
    let x = list_rect.right - list_rect.left - settings_pixels(125);
    let mut origin = windows_sys::Win32::Foundation::POINT::default();
    unsafe { windows_sys::Win32::Graphics::Gdi::MapWindowPoints(list, settings, &mut origin, 1) };
    let point = |y| (origin.x + x, origin.y + y);
    let sample = [
        (
            origin.x + settings_pixels(4),
            origin.y + (offset / 2) as i32,
        ),
        point((offset / 2) as i32 + settings_pixels(4)),
        point((row * 2 + row / 2) as i32),
    ];
    struct RestoreCursor(POINT);
    impl Drop for RestoreCursor {
        fn drop(&mut self) {
            unsafe { SetCursorPos(self.0.x, self.0.y) };
        }
    }
    let mut cursor = RestoreCursor(POINT::default());
    assert_ne!(unsafe { GetCursorPos(&mut cursor.0) }, 0);
    let move_cursor = |y| unsafe {
        assert_ne!(
            SetCursorPos(list_rect.left + grip as i32, list_rect.top + y as i32),
            0
        );
    };
    let order_before_drag = load().engine_order;
    move_cursor(row / 2);
    send(list, WM_LBUTTONDOWN, 1, pointer(row / 2));
    unsafe { UpdateWindow(list) };
    let before = printed_client_pixels(settings, &sample);
    move_cursor(row / 2 + offset);
    send(list, WM_MOUSEMOVE, 1, pointer(row / 2 + offset));
    unsafe { UpdateWindow(list) };
    let moved = printed_client_pixels(settings, &sample);
    assert_ne!(moved[0], before[0], "the floating card follows the cursor");
    assert_eq!(
        moved[1..],
        before[1..],
        "the placeholder and unrelated row stay in place"
    );
    move_cursor(row * 3 / 2);
    send(list, WM_MOUSEMOVE, 1, pointer(row * 3 / 2));
    unsafe { UpdateWindow(list) };
    let crossed = printed_client_pixels(settings, &sample);
    assert_ne!(
        crossed[1], moved[1],
        "the placeholder follows the hovered row"
    );
    send(list, WM_KEYDOWN, usize::from(VK_ESCAPE), 0);
    assert_eq!(load().engine_order, order_before_drag);
    drop(cursor);
    send(settings, WM_CLOSE, 0, 0);
    #[cfg(feature = "ocr")]
    if let Some(shortcut) = ocr_hotkey {
        check_ocr_selection(pid, shortcut, &trace);
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
    if let Some(shortcut) = ocr_hotkey {
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
        press_shortcut(shortcut);
        sleep(Duration::from_millis(150));
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

#[test]
fn native_preferences_history_filters_and_row_actions() {
    use yisou::{
        application::{History, Origin},
        settings::Settings,
        storage::SettingsStore,
    };

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let mut preferences = Settings::default();
    preferences.preferences.record_history = true;
    // The browser process consumes the URL locally.
    preferences.preferences.browser = Some(
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/where.exe"),
    );
    JsonSettings::new(config.clone())
        .save(&preferences)
        .unwrap();
    let history_dir = data.path().join("YiSouRust");
    std::fs::create_dir(&history_dir).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let long_query = format!(
        "{}\n{}\n最后一行",
        "复杂中文搜索词，测试窗口变窄时的自动换行。".repeat(45),
        "A".repeat(500)
    );
    let history = [
        ("Rust compiler", "Terminal", "bing", "Bing", now),
        ("Rust manual", "Browser", "google", "Google", now),
        (
            long_query.as_str(),
            "Terminal",
            "retired",
            "Retired",
            now - 172_800_000,
        ),
    ];
    let history: Vec<_> = history
        .into_iter()
        .enumerate()
        .map(
            |(id, (query, source, engine_id, engine_name, timestamp_ms))| {
                serde_json::json!({
                    "id": id + 1, "timestamp_ms": timestamp_ms, "query": query,
                    "engine_id": engine_id, "engine_name": engine_name,
                    "origin": "typed", "source": source,
                })
            },
        )
        .collect();
    std::fs::write(
        history_dir.join("history.json"),
        serde_json::to_vec(&history).unwrap(),
    )
    .unwrap();
    let child = AppProcess(
        Command::new(executable())
            .env("LOCALAPPDATA", data.path())
            .arg("--config")
            .arg(config)
            .arg("--settings")
            .spawn()
            .unwrap(),
    );
    let settings = before_timeout("history settings", || {
        visible_window(w!("YiSou.Rust.Settings"), child.0.id())
    });
    let item = |id| unsafe { GetDlgItem(settings, id) };
    let dpi = unsafe { GetDpiForWindow(settings) } as i32;
    let settings_pixels = |dip| (dip * dpi + 48) / 96;
    let nav = item(1);
    send(nav, LB_SETCURSEL, 5, 0);
    send(
        settings,
        WM_COMMAND,
        (LBN_SELCHANGE as usize) << 16 | 1,
        nav as isize,
    );
    let rows = || send(item(10), LB_GETCOUNT, 0, 0);
    assert_eq!(rows(), 3);
    let query = window_rect(item(25));
    let engine = window_rect(item(29));
    let source = window_rect(item(26));
    assert!(
        query.right < engine.left && engine.right < source.left,
        "query {}..{}, engine {}..{}, source {}..{}",
        query.left,
        query.right,
        engine.left,
        engine.right,
        source.left,
        source.right,
    );
    assert_eq!((engine.top, engine.bottom), (source.top, source.bottom));
    let engine_labels = combo_labels(item(29));
    let source_labels = combo_labels(item(26));
    assert_eq!(&engine_labels[1..], ["Bing", "Google", "Retired"]);
    assert_eq!(&source_labels[1..], ["Browser", "Terminal"]);
    assert!(source.bottom + settings_pixels(6) < window_rect(item(10)).top);
    let row_height = |index| send(item(10), LB_GETITEMHEIGHT, index, 0);
    assert!(row_height(1) >= settings_pixels(80) as usize);
    assert!(row_height(0) > row_height(1) && row_height(2) > row_height(0));
    let long_height = row_height(2);
    assert!(long_height > 255);
    let list = item(10);
    let window = window_rect(settings);
    let window_width = window.right - window.left;
    let window_height = window.bottom - window.top;
    let resize = |width| unsafe {
        assert_ne!(
            SetWindowPos(
                settings,
                std::ptr::null_mut(),
                0,
                0,
                width,
                window_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
            ),
            0
        );
    };
    resize(settings_pixels(680));
    assert!(row_height(2) > long_height);
    resize(window_width);
    assert_eq!(row_height(2), long_height);
    assert!(height(list) < height(settings));
    send(settings, WM_VSCROLL, SB_BOTTOM as usize, 0);
    let summary = window_rect(item(1561));
    assert!(summary.top >= window_rect(list).bottom);
    assert!(summary.bottom < window.bottom);
    send(settings, WM_VSCROLL, SB_TOP as usize, 0);
    let list_width = window_rect(list).right - window_rect(list).left;
    let repeat_x = list_width - settings_pixels(64);
    let delete_x = list_width - settings_pixels(24);
    let center = settings_pixels(61);
    {
        use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, GetDC, GetPixel, ReleaseDC};
        use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
        struct CursorPosition(POINT);
        impl Drop for CursorPosition {
            fn drop(&mut self) {
                unsafe { SetCursorPos(self.0.x, self.0.y) };
            }
        }
        let mut cursor = CursorPosition(POINT::default());
        assert_ne!(unsafe { GetCursorPos(&mut cursor.0) }, 0);
        let hover = |x, y| {
            let mut point = POINT { x, y };
            assert_ne!(unsafe { ClientToScreen(list, &mut point) }, 0);
            assert_ne!(unsafe { SetCursorPos(point.x, point.y) }, 0);
            send(list, WM_MOUSEMOVE, 0, ((y << 16) | x) as isize);
        };
        // Sample button faces away from the centered text and icons.
        let hover_pixels = [
            (repeat_x, center - settings_pixels(11)),
            (delete_x, center - settings_pixels(11)),
            (
                delete_x,
                center + row_height(0) as i32 - settings_pixels(11),
            ),
        ];
        let snapshot = || unsafe {
            UpdateWindow(list);
            let dc = GetDC(list);
            let pixels = hover_pixels.map(|(x, y)| GetPixel(dc, x, y));
            ReleaseDC(list, dc);
            assert!(!pixels.contains(&u32::MAX));
            pixels
        };
        hover(settings_pixels(80), center);
        send(list, WM_MOUSELEAVE, 0, 0);
        sleep(Duration::from_millis(150));
        let idle = snapshot();
        hover(repeat_x, center);
        before_timeout("repeat button changes color on hover", || {
            let pixels = snapshot();
            (pixels[0] != idle[0] && pixels[1..] == idle[1..]).then_some(())
        });
        hover(delete_x, center);
        before_timeout(
            "hover moves to delete without affecting the next row",
            || {
                let pixels = snapshot();
                (pixels[0] == idle[0] && pixels[1] != idle[1] && pixels[2] == idle[2]).then_some(())
            },
        );
        hover(settings_pixels(80), center);
        before_timeout("delete button returns to its resting color", || {
            (snapshot() == idle).then_some(())
        });
    }
    let choose = |id, selection| {
        let combo = item(id);
        send(combo, WM_KEYDOWN, usize::from(VK_RETURN), 0);
        assert_ne!(send(combo, CB_GETDROPPEDSTATE, 0, 0), 0);
        send(combo, WM_KEYDOWN, usize::from(VK_HOME), 0);
        for _ in 0..selection {
            send(combo, WM_KEYDOWN, usize::from(VK_DOWN), 0);
        }
        send(combo, WM_KEYDOWN, usize::from(VK_RETURN), 0);
        assert_eq!(send(combo, CB_GETCURSEL, 0, 0), selection);
        assert_eq!(send(combo, CB_GETDROPPEDSTATE, 0, 0), 0);
    };
    choose(29, 1);
    assert_eq!(rows(), 1);
    choose(26, 1);
    assert_eq!(rows(), 0);
    choose(26, 2);
    assert_eq!(rows(), 1);
    send(item(25), WM_SETTEXT, 0, w!("missing result") as isize);
    before_timeout("empty filtered history", || (rows() == 0).then_some(()));
    assert_ne!(unsafe { IsWindowVisible(item(1560)) }, 0);
    send(item(25), WM_SETTEXT, 0, w!("rust") as isize);
    before_timeout("keyword and both dropdowns match", || {
        (rows() == 1).then_some(())
    });
    assert!(send(item(1561), WM_GETTEXTLENGTH, 0, 0) > 0);
    send(item(155), BM_CLICK, 0, 0);
    assert_eq!(rows(), 3);
    assert_eq!(send(item(29), CB_GETCURSEL, 0, 0), 0);
    assert_eq!(send(item(26), CB_GETCURSEL, 0, 0), 0);
    choose(29, 3);
    assert_eq!(rows(), 1);
    let list = item(10);
    let bounds = window_rect(list);
    let x = bounds.right - bounds.left - settings_pixels(24);
    let y = settings_pixels(61);
    let point = ((y << 16) | x) as isize;
    send(list, WM_LBUTTONDOWN, 1, point);
    send(list, WM_LBUTTONUP, 0, point);
    before_timeout("deleted the only record for an engine", || {
        (rows() == 2).then_some(())
    });
    assert_eq!(send(item(29), CB_GETCURSEL, 0, 0), 0);
    assert_eq!(&combo_labels(item(29))[1..], ["Bing", "Google"]);
    assert_eq!(send(item(10), LB_GETCOUNT, 0, 0), 2);
    assert!(
        !History::new(history_dir.join("history.json"))
            .records()
            .unwrap()
            .iter()
            .any(|record| record.engine_id == "retired")
    );

    let x = bounds.right - bounds.left - settings_pixels(64);
    let y = settings_pixels(61);
    let point = ((y << 16) | x) as isize;
    send(list, WM_LBUTTONDOWN, 1, point);
    send(list, WM_LBUTTONUP, 0, point);
    before_timeout("repeated search is added to history", || {
        (rows() == 3).then_some(())
    });
    let mut history = History::new(history_dir.join("history.json"));
    let repeated = &history.records().unwrap()[0];
    assert_eq!(repeated.query, "Rust compiler");
    assert_eq!(repeated.engine_id, "bing");
    assert_eq!(repeated.origin, Origin::History);

    choose(26, 2);
    assert_eq!(rows(), 2);
    let search_class = w!("YiSou.Rust.Search");
    let search = window_for_process(search_class, child.0.id()).unwrap();
    let live_search = |text: *const u16| {
        send(search, WM_APP + 1, 0, 0);
        let edit = before_timeout("search input initialized while settings stays open", || {
            let edit = unsafe { GetDlgItem(search, 100) };
            (!edit.is_null()).then_some(edit)
        });
        send(edit, WM_SETTEXT, 0, text as isize);
        send(edit, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    };
    live_search(w!("实时刷新历史，无需重开设置"));
    before_timeout(
        "new search source is present while settings stays open",
        || (combo_labels(item(26)).len() == 4).then_some(()),
    );
    assert_eq!(rows(), 2);
    assert_eq!(
        combo_labels(item(26))[send(item(26), CB_GETCURSEL, 0, 0)],
        "Terminal"
    );
    send(item(155), BM_CLICK, 0, 0);
    assert_eq!(rows(), 4);
    assert_eq!(
        History::new(history_dir.join("history.json"))
            .records()
            .unwrap()[0]
            .query,
        "实时刷新历史，无需重开设置"
    );
    live_search(w!("第二条即时记录"));
    before_timeout("new search card appears without changing page", || {
        (rows() == 5).then_some(())
    });
    assert_ne!(unsafe { IsWindowVisible(item(1551)) }, 0);
    send(item(1551), BM_CLICK, 0, 0);
    assert_eq!(rows(), 6);
    assert_eq!(unsafe { IsWindowVisible(item(1551)) }, 0);
    let restored = History::new(history_dir.join("history.json"))
        .records()
        .unwrap()
        .iter()
        .find(|record| record.engine_id == "retired")
        .unwrap()
        .clone();
    assert_eq!(restored.timestamp_ms, now - 172_800_000);
    assert_eq!(restored.query, long_query);

    send(settings, WM_VSCROLL, SB_BOTTOM as usize, 0);
    let last_row = || {
        let list = item(10);
        let mut rect = RECT::default();
        assert_eq!(
            send(
                list,
                LB_GETITEMRECT,
                rows() - 1,
                (&mut rect as *mut RECT) as isize
            ),
            0
        );
        window_rect(list).top + rect.top
    };
    let anchored = last_row();
    live_search(w!("插入新纪录时保留阅读位置"));
    before_timeout("history insertion keeps the last card anchored", || {
        (rows() == 7).then_some(())
    });
    assert_eq!(last_row(), anchored);
}

#[test]
fn native_history_popup_appearance_and_visibility() {
    use windows_sys::Win32::Graphics::{
        Dwm::DwmFlush,
        Gdi::{
            CreateRectRgn, DeleteObject, GetDC, GetMonitorInfoW, GetPixel, GetWindowRgn,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, PtInRegion, ReleaseDC,
        },
    };
    use yisou::{
        application::{Origin, Record},
        settings::{HistoryMaterial, Language, SearchMaterial, SearchTone, Settings},
        storage::SettingsStore,
    };

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    struct DesktopFixture(HWND);
    impl Drop for DesktopFixture {
        fn drop(&mut self) {
            unsafe { DestroyWindow(self.0) };
        }
    }
    let fixture = unsafe {
        let mut cursor = POINT::default();
        assert_ne!(GetCursorPos(&mut cursor), 0);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        assert_ne!(GetMonitorInfoW(monitor, &mut info), 0);
        let area = info.rcMonitor;
        const SS_BLACKRECT: u32 = 4;
        let backdrop = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            w!("STATIC"),
            null(),
            WS_POPUP | SS_BLACKRECT,
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            null(),
        );
        assert!(!backdrop.is_null());
        DesktopFixture(backdrop)
    };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let history = data.path().join("YiSouRust/history.json");
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let record = Record {
        id: 1,
        timestamp_ms: 1_700_000_000_000,
        query: "着色测试 · native history".into(),
        engine_id: "bing".into(),
        engine_name: "必应".into(),
        origin: Origin::Typed,
        source: String::new(),
    };
    std::fs::write(&history, serde_json::to_vec(&[record]).unwrap()).unwrap();
    let mut defaults = Settings::default();
    let prefs = &mut defaults.preferences;
    prefs.language = Language::Chinese;
    prefs.search_material = SearchMaterial::Solid;
    prefs.search_tone = SearchTone::Light;
    prefs.history_popup.tone = SearchTone::Light;
    prefs.record_history = true;
    prefs.screen_background = ScreenBackground::Dim;
    prefs.browser = Some(
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/where.exe"),
    );
    let mut stored = JsonSettings::new(config.clone());
    stored.save(&defaults).unwrap();
    let app = AppProcess(
        Command::new(executable())
            .env("LOCALAPPDATA", data.path())
            .arg("--config")
            .arg(&config)
            .arg("--settings")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let pid = app.0.id();
    let settings = before_timeout("settings window", || {
        visible_window(w!("YiSou.Rust.Settings"), pid)
    });
    let search = window_for_process(w!("YiSou.Rust.Search"), pid).unwrap();
    let item = |id| unsafe { GetDlgItem(settings, id) };
    let nav = item(1);
    send(nav, LB_SETCURSEL, 1, 0);
    send(
        settings,
        WM_COMMAND,
        (LBN_SELCHANGE as usize) << 16 | 1,
        nav as isize,
    );
    let click = |id| send(settings, WM_COMMAND, id as usize, item(id) as isize);
    click(264);
    let choose = |id, index| {
        send(item(id), CB_SETCURSEL, index, 0);
        send(
            settings,
            WM_COMMAND,
            (CBN_SELCHANGE as usize) << 16 | id as usize,
            item(id) as isize,
        );
    };
    let set_color = |id, text: *const u16| {
        click(id);
        let dialog = before_timeout("history color picker", || {
            visible_window(w!("YiSou.Rust.Settings.ColorPicker"), pid)
        });
        let value = unsafe { GetDlgItem(dialog, 233) };
        send(value, WM_SETTEXT, 0, text as isize);
        send(dialog, WM_COMMAND, 237, unsafe { GetDlgItem(dialog, 237) }
            as isize);
        assert_eq!(unsafe { IsWindow(dialog) }, 0);
    };
    let popup_class = w!("YiSou.Rust.Engines");
    let type_query = |text: *const u16| {
        if unsafe { IsWindowVisible(search) } == 0 {
            send(search, WM_APP + 1, 0, 0);
        }
        let edit = unsafe { GetDlgItem(search, 100) };
        send(edit, WM_SETTEXT, 0, text as isize);
        edit
    };
    let open_popup = || {
        type_query(w!("测试"));
        before_timeout("history popup", || {
            if unsafe { IsWindowVisible(search) } == 0 {
                type_query(w!("测试"));
            }
            visible_window(popup_class, pid)
        })
    };
    let dpi = unsafe { GetDpiForWindow(settings) } as i32;
    let px = |dip| (dip * dpi + 48) / 96;
    for id in [284, 285] {
        let control = item(id);
        let bounds = window_rect(control);
        let saved = || {
            let history = stored.load().unwrap().preferences.history_popup;
            if id == 284 {
                history.enabled
            } else {
                history.show_shortcuts
            }
        };
        let assert_thumb = |on| {
            before_timeout(
                "history switch thumb moves to the selected side",
                || unsafe {
                    UpdateWindow(control);
                    DwmFlush();
                    let dc = GetDC(std::ptr::null_mut());
                    let y = (bounds.top + bounds.bottom) / 2;
                    let at = |x| GetPixel(dc, bounds.left + px(x), y);
                    let track = at(22);
                    let left = at(11);
                    let right = at(33);
                    ReleaseDC(std::ptr::null_mut(), dc);
                    let (thumb, empty) = if on { (right, left) } else { (left, right) };
                    (track != u32::MAX && thumb != u32::MAX && thumb != track && empty == track)
                        .then_some(())
                },
            );
        };
        assert_thumb(true);
        click(id);
        assert!(!saved());
        assert_thumb(false);
        click(id);
        assert!(saved());
        assert_thumb(true);
    }
    let set_number = |id, text: *const u16| {
        send(item(id), WM_SETTEXT, 0, text as isize);
        send(item(id), WM_KEYDOWN, usize::from(VK_RETURN), 0);
    };
    let assert_screen_corners = |window, background, rounded| {
        let bounds = window_rect(window);
        let inset = px(1);
        let points = [
            (bounds.left + inset, bounds.top + inset),
            (bounds.right - 1 - inset, bounds.top + inset),
            (bounds.left + inset, bounds.bottom - 1 - inset),
            (bounds.right - 1 - inset, bounds.bottom - 1 - inset),
        ];
        let interior_x = bounds.left + px(12);
        let interior_y = bounds.top + px(18);
        let deadline = Instant::now() + Duration::from_secs(4);
        before_timeout("composited corners match the window shape", || unsafe {
            DwmFlush();
            let dc = GetDC(std::ptr::null_mut());
            let interior = GetPixel(dc, interior_x, interior_y);
            let corners = points.map(|(x, y)| GetPixel(dc, x, y));
            let outside = GetPixel(dc, bounds.left - px(4), bounds.top + inset);
            ReleaseDC(std::ptr::null_mut(), dc);
            let expected = if rounded { outside } else { background };
            let matches = interior == background
                && corners == [expected; 4]
                && outside != u32::MAX
                && outside != background;
            assert!(
                matches || Instant::now() < deadline,
                "rounded={rounded}: expected background {background:#x}, corners {expected:#x}; got background {interior:#x}, corners {corners:x?}, desktop {outside:#x}"
            );
            matches.then_some(())
        });
    };
    let has_corner_cutout = |window| unsafe {
        let region = CreateRectRgn(0, 0, 0, 0);
        let result = GetWindowRgn(window, region) != 0
            && PtInRegion(region, 0, 0) == 0
            && PtInRegion(region, px(80), px(3)) != 0;
        DeleteObject(region);
        result
    };
    let assert_smooth_corner = |window, radius: f32, fill: u32| {
        let corner = window_rect(window);
        let diagonal = (radius * 0.22) as i32;
        let fill_green = (fill >> 8) & 255;
        let deadline = Instant::now() + Duration::from_secs(4);
        before_timeout(
            "the quartic corner preserves its diagonal and has partial edge coverage",
            || unsafe {
                DwmFlush();
                let dc = GetDC(std::ptr::null_mut());
                let backdrop = GetPixel(dc, corner.left - px(4), corner.top);
                let outside = GetPixel(dc, corner.left, corner.top);
                // A quarter-circle would still be outside at 22% of its radius.
                let inside = GetPixel(dc, corner.left + diagonal, corner.top + diagonal);
                let soft_edge = (0..diagonal).any(|y| {
                    (0..radius.ceil() as i32).any(|x| {
                        let value = GetPixel(dc, corner.left + x, corner.top + y);
                        let green = (value >> 8) & 255;
                        green > ((backdrop >> 8) & 255) + 8 && green + 50 < fill_green
                    })
                });
                ReleaseDC(std::ptr::null_mut(), dc);
                let inside_is_filled = [0, 8, 16]
                    .into_iter()
                    .all(|shift| ((inside >> shift) & 255).abs_diff((fill >> shift) & 255) <= 8);
                let matches = outside == backdrop && inside_is_filled && soft_edge;
                assert!(
                    matches || Instant::now() < deadline,
                    "radius={radius}, diagonal={diagonal}, expected fill={fill:06x}, sampled inside={inside:06x}, outside={outside:06x}, backdrop={backdrop:06x}, edge={soft_edge}"
                );
                matches.then_some(())
            },
        );
    };
    click(260);
    send(settings, WM_VSCROLL, SB_TOP as usize, 0);
    let preview = window_rect(item(115));
    let radius_control = window_rect(item(295));
    assert!((preview.bottom..preview.bottom + px(100)).contains(&radius_control.top));
    let preview_corner = || unsafe {
        UpdateWindow(settings);
        DwmFlush();
        let dc = GetDC(std::ptr::null_mut());
        let left = preview.left - px(20);
        let top = preview.top - px(8);
        let mut pixels = Vec::new();
        for y in top..top + px(4) {
            for x in left..left + px(32) {
                pixels.push(GetPixel(dc, x, y));
            }
        }
        ReleaseDC(std::ptr::null_mut(), dc);
        assert!(!pixels.contains(&u32::MAX));
        pixels
    };
    set_number(295, w!("0"));
    let mut previous = preview_corner();
    for radius in [w!("8"), w!("16")] {
        set_number(295, radius);
        previous = before_timeout("each radius redraws the search preview corner", || {
            let current = preview_corner();
            (current != previous).then_some(current)
        });
    }
    // The default search box is 56 DIP high. At 28 DIP its ends are semicircles.
    for radius in [w!("28"), w!("500")] {
        set_number(295, radius);
        before_timeout("the search preview has semicircular ends", || unsafe {
            UpdateWindow(settings);
            DwmFlush();
            let dc = GetDC(std::ptr::null_mut());
            let left = preview.left - px(20);
            let top = preview.top - px(8);
            let background = GetPixel(dc, left + px(6), top - px(2));
            let outside = GetPixel(dc, left + px(6), top + px(6));
            let inside = GetPixel(dc, left + px(16), top + px(6));
            let tip = GetPixel(dc, left + px(2), top + px(28));
            ReleaseDC(std::ptr::null_mut(), dc);
            (background != u32::MAX
                && outside == background
                && inside == 0xffffff
                && tip == 0xffffff)
                .then_some(())
        });
    }
    click(297);
    assert_eq!(stored.load().unwrap().preferences.search_corner_radius, 501);
    click(296);
    assert_eq!(stored.load().unwrap().preferences.search_corner_radius, 500);
    click(264);
    assert!(item(302).is_null());
    send(settings, WM_VSCROLL, SB_BOTTOM as usize, 0);
    assert_ne!(unsafe { IsWindowVisible(item(298)) }, 0);
    set_number(298, w!("64"));
    assert_eq!(
        stored
            .load()
            .unwrap()
            .preferences
            .history_popup
            .corner_radius,
        64
    );
    set_number(298, w!("0"));
    unsafe {
        ShowWindow(fixture.0, SW_SHOWNOACTIVATE);
        UpdateWindow(fixture.0);
    }
    let full_height = height(open_popup());
    assert!(!has_corner_cutout(open_popup()));
    let bounds = window_rect(search);
    let width = bounds.right - bounds.left;
    let middle = (bounds.bottom - bounds.top) / 2;
    unsafe {
        let region = CreateRectRgn(0, 0, 0, 0);
        assert!(!region.is_null());
        assert_ne!(GetWindowRgn(search, region), 0);
        for x in [px(6), width - 1 - px(6)] {
            assert_eq!(PtInRegion(region, x, px(6)), 0);
        }
        for x in [px(16), width - 1 - px(16)] {
            assert_ne!(PtInRegion(region, x, px(6)), 0);
        }
        for x in [px(2), width - 1 - px(2)] {
            assert_ne!(PtInRegion(region, x, middle), 0);
        }
        DeleteObject(region);
    }
    click(260);
    set_number(295, w!("18"));
    click(264);
    open_popup();
    assert!(has_corner_cutout(search));
    assert_screen_corners(search, 0xffffff, true);
    assert_smooth_corner(search, px(18) as f32, 0xffffff);
    set_number(298, w!("20"));
    assert!(has_corner_cutout(open_popup()));
    let saved = stored.load().unwrap().preferences;
    assert_eq!(saved.search_corner_radius, 18);
    assert_eq!(saved.history_popup.corner_radius, 20);
    click(285);
    assert!((height(open_popup()) - (full_height - px(35))).abs() <= 1);
    set_color(292, w!("#F0DFC0"));
    set_color(293, w!("#182438"));
    set_color(294, w!("#FF21A0 / 0%"));
    let popup = open_popup();
    let match_points: Vec<_> = (px(48)..px(132))
        .flat_map(|x| (px(49)..px(82)).map(move |y| (x, y)))
        .collect();
    before_timeout("solid color and custom match color render", || {
        let base = printed_pixels(popup, &[(px(10), px(19))], 2)[0];
        (base == 0xc0dff0 && printed_pixels(popup, &match_points, 2).contains(&0xa021ff))
            .then_some(())
    });
    assert_smooth_corner(popup, px(20) as f32, 0xc0dff0);
    choose(283, 2);
    let popup = open_popup();
    before_timeout("dark history background is independent", || {
        (printed_pixels(popup, &[(px(10), px(19))], 2)[0] == 0x382418).then_some(())
    });
    choose(283, 1);
    for (index, material, input_id) in [
        (0, SearchMaterial::Acrylic, 286),
        (1, SearchMaterial::Mica, 289),
    ] {
        choose(282, index);
        set_color(292, w!("#F0DFC0 / 0%"));
        assert_eq!(control_text(item(input_id)), "0");
        let popup = open_popup();
        assert_ne!(
            unsafe {
                SetWindowPos(
                    fixture.0,
                    *screen_shades(search).last().unwrap(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            },
            0
        );
        assert_screen_corners(popup, 0xc0dff0, true);
        set_number(298, w!("0"));
        assert_screen_corners(open_popup(), 0xc0dff0, false);
        set_number(298, w!("20"));
        let popup = open_popup();
        let bounds = window_rect(popup);
        before_timeout("material tint is visible on screen", || unsafe {
            let dc = GetDC(std::ptr::null_mut());
            let pixel = GetPixel(dc, bounds.left + px(10), bounds.top + px(19));
            ReleaseDC(std::ptr::null_mut(), dc);
            (pixel == 0xc0dff0).then_some(())
        });
        let saved = stored.load().unwrap().preferences;
        assert_eq!(saved.history_background(false).material, material);
        assert_eq!(saved.history_popup.tint_percent(material), 100);
        assert_eq!(saved.search_material, SearchMaterial::Solid);
        if material == SearchMaterial::Acrylic {
            struct RestoreCursor(POINT);
            impl Drop for RestoreCursor {
                fn drop(&mut self) {
                    unsafe { SetCursorPos(self.0.x, self.0.y) };
                }
            }
            let mut cursor = RestoreCursor(POINT::default());
            assert_ne!(unsafe { GetCursorPos(&mut cursor.0) }, 0);
            click(285);
            set_color(293, w!("#182438 / 0%"));
            for (tone, surface, direction) in [(1, 0xc0dff0, -1), (2, 0x382418, 1)] {
                choose(283, tone);
                let popup = open_popup();
                let bounds = window_rect(popup);
                let footer = bounds.bottom - px(36);
                let x = (bounds.left + bounds.right) / 2;
                let diff = |color: u32| {
                    direction * (((color >> 8) & 255) as i32 - ((surface >> 8) & 255) as i32)
                };
                let mut sample = [0; 3];
                let mut keycap = 0;
                before_timeout(
                    "acrylic history outlines follow the panel tone",
                    || unsafe {
                        DwmFlush();
                        let dc = GetDC(std::ptr::null_mut());
                        let at = |x, y| GetPixel(dc, x, y);
                        sample = [
                            at(x, bounds.top + px(19)),
                            at(x, bounds.top + px(38)),
                            at(x, footer),
                        ];
                        let key_top = footer + px(9);
                        keycap = (key_top - 1..=key_top + 1)
                            .map(|y| diff(at(bounds.left + px(23), y)))
                            .max()
                            .unwrap();
                        ReleaseDC(std::ptr::null_mut(), dc);
                        (sample[0] == surface).then_some(())
                    },
                );
                let [_, header, footer] = sample;
                assert!(diff(header) >= 8, "acrylic header: {sample:x?}");
                assert!(diff(footer) >= 8, "acrylic footer: {sample:x?}");
                assert!(
                    keycap > diff(footer),
                    "keycap contrast {keycap} should exceed the divider: {sample:x?}"
                );
                let sample_row = || unsafe {
                    DwmFlush();
                    let dc = GetDC(std::ptr::null_mut());
                    let color = GetPixel(dc, bounds.left + px(42), bounds.top + px(64));
                    ReleaseDC(std::ptr::null_mut(), dc);
                    color
                };
                let move_cursor = |y| {
                    assert_ne!(
                        unsafe { SetCursorPos(bounds.left + px(42), bounds.top + px(y)) },
                        0
                    );
                    send(popup, WM_MOUSEMOVE, 0, ((px(y) << 16) | px(42)) as isize);
                    unsafe { UpdateWindow(popup) };
                };
                move_cursor(20);
                before_timeout("history row at rest", || {
                    (sample_row() == surface).then_some(())
                });
                move_cursor(64);
                let deadline = Instant::now() + Duration::from_secs(4);
                before_timeout("acrylic hover blends with the surface", || {
                    let color = sample_row();
                    let matches = (3..=25).contains(&diff(color));
                    assert!(
                        matches || Instant::now() < deadline,
                        "tone {tone}: resting {surface:#x}, hover {color:#x}"
                    );
                    matches.then_some(())
                });
                move_cursor(20);
                before_timeout("leaving a history row clears the hover", || {
                    (sample_row() == surface).then_some(())
                });
            }
            choose(283, 1);
            click(285);
        }
        if std::env::var_os("YISOU_TEST_REQUIRE_ACRYLIC").is_some() {
            set_number(input_id, w!("75"));
            let popup = open_popup();
            let bounds = window_rect(popup);
            before_timeout("material is visible through the tint", || unsafe {
                let dc = GetDC(std::ptr::null_mut());
                let pixel = GetPixel(dc, bounds.left + px(10), bounds.top + px(19));
                ReleaseDC(std::ptr::null_mut(), dc);
                (pixel != 0xc0dff0 && pixel != 0 && pixel != u32::MAX).then_some(())
            });
        }
    }
    choose(282, 3);
    assert_eq!(
        stored.load().unwrap().preferences.history_popup.material,
        HistoryMaterial::FollowSearch
    );
    assert!(item(286).is_null() && item(289).is_null());
    assert!(item(283).is_null() && item(292).is_null() && item(293).is_null());
    click(260);
    set_color(230, w!("#D4E6F2"));
    set_color(231, w!("#24354C"));
    for (tone, color) in [(1, 0xf2e6d4), (2, 0x4c3524)] {
        choose(198, tone);
        let popup = open_popup();
        assert_screen_corners(popup, color, true);
    }
    choose(198, 1);
    set_color(230, w!("#FFFFFF"));
    for (index, transparency, material) in [
        (0, 210, SearchMaterial::Acrylic),
        (1, 213, SearchMaterial::Mica),
    ] {
        choose(197, index);
        set_number(transparency, w!("0"));
        assert_eq!(
            stored
                .load()
                .unwrap()
                .preferences
                .history_background(false)
                .material,
            material
        );
        click(264);
        assert_eq!(send(item(282), CB_GETCURSEL, 0, 0), 3);
        assert!(item(286).is_null() && item(289).is_null());
        click(260);
        let popup = open_popup();
        assert_screen_corners(popup, 0xffffff, true);
        assert_screen_corners(search, 0xffffff, true);
        set_number(295, w!("0"));
        open_popup();
        assert_screen_corners(search, 0xffffff, false);
        set_number(295, w!("18"));
    }
    choose(197, 2);
    click(264);
    assert!(item(286).is_null() && item(289).is_null());
    choose(282, 2);
    assert!(!item(283).is_null() && !item(292).is_null() && !item(293).is_null());
    assert_screen_corners(open_popup(), 0xc0dff0, true);
    choose(282, 3);
    click(284);
    assert!(visible_window(popup_class, pid).is_none());
    let edit = type_query(w!("关闭浮层后仍记录"));
    sleep(Duration::from_millis(260));
    assert!(visible_window(popup_class, pid).is_none());
    send(edit, WM_KEYDOWN, usize::from(VK_RETURN), 0);
    let entries: Vec<Record> = serde_json::from_slice(&std::fs::read(&history).unwrap()).unwrap();
    assert_eq!(entries[0].query, "关闭浮层后仍记录");
    assert!(!stored.load().unwrap().preferences.history_popup.enabled);
    click(284);
    open_popup();
}

#[test]
fn native_search_history_and_engine_switch() {
    use yisou::{
        application::{History, Origin, Record},
        settings::{Language, SearchTone, Settings},
        storage::SettingsStore,
    };

    let _desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("settings.json");
    let mut settings = Settings::default();
    settings.preferences.record_history = true;
    settings.preferences.language = Language::Chinese;
    settings.preferences.search_tone = SearchTone::Light;
    // A short-lived executable consumes the URL argument without opening a real browser.
    settings.preferences.browser = Some(
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/where.exe"),
    );
    JsonSettings::new(config.clone()).save(&settings).unwrap();
    let history_path = data.path().join("YiSouRust/history.json");
    std::fs::create_dir_all(history_path.parent().unwrap()).unwrap();
    let long_query = "Unicode 🧪 候选测试与换行验证。".repeat(33);
    let records: Vec<_> = (0..8)
        .map(|index| Record {
            id: index + 1,
            timestamp_ms: 1_700_000_000_000 - index,
            query: if index == 0 {
                long_query.clone()
            } else {
                format!(
                    "Unicode 🧪 候选测试 {index} · {}",
                    "这条搜索记录也需要换行，验证滚动后的定位和回填。".repeat(3)
                )
            },
            engine_id: "bing".into(),
            engine_name: "必应".into(),
            origin: Origin::Typed,
            source: "Terminal".into(),
        })
        .collect();
    std::fs::write(&history_path, serde_json::to_vec(&records).unwrap()).unwrap();
    let child = AppProcess(
        Command::new(executable())
            .env("LOCALAPPDATA", data.path())
            .arg("--config")
            .arg(config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();
    let search_class = w!("YiSou.Rust.Search");
    let popup_class = w!("YiSou.Rust.Engines");
    let search = before_timeout("history search window", || {
        visible_window(search_class, pid)
    });
    let edit = unsafe { GetDlgItem(search, 100) };
    let button = unsafe { GetDlgItem(search, 101) };
    let input = |text: &str| {
        let text: Vec<_> = text.encode_utf16().chain([0]).collect();
        send(edit, WM_SETTEXT, 0, text.as_ptr() as isize);
    };
    let current_text = || {
        let len = send(edit, WM_GETTEXTLENGTH, 0, 0);
        let mut text = vec![0u16; len + 1];
        send(edit, WM_GETTEXT, text.len(), text.as_mut_ptr() as isize);
        String::from_utf16(&text[..len]).unwrap()
    };
    let key = |key| send(edit, WM_KEYDOWN, usize::from(key), 0);
    let last_record = || {
        let mut history = History::new(history_path.clone());
        history.page("", "", 0).unwrap().into_iter().next().unwrap()
    };
    let reopen = || {
        before_timeout("search reopened after the browser process exits", || {
            if visible_window(search_class, pid).is_none() {
                send(search, WM_APP + 1, 0, 0);
            }
            visible_window(search_class, pid)
        });
    };
    let keyword = "Rust 编译器";
    input(&format!("#g {keyword}"));
    key(VK_RETURN);
    assert_eq!(last_record().engine_id, "google");
    reopen();

    key(VK_UP);
    assert_eq!(current_text(), keyword);
    send(button, BM_CLICK, 0, 0);
    key(VK_HOME);
    key(VK_RETURN);
    assert_eq!(current_text(), keyword);
    key(VK_RETURN);
    let repeated = last_record();
    assert_eq!(repeated.engine_id, "bing");
    assert_eq!(repeated.query, keyword);
    reopen();

    let history_popup = |keyword: &str| {
        input(keyword);
        before_timeout("history suggestions", || {
            if visible_window(search_class, pid).is_none() {
                reopen();
                input(keyword);
            }
            visible_window(popup_class, pid)
        })
    };
    let popup = history_popup("🧪 候选");
    let search_bounds = window_rect(search);
    let panel_bounds = window_rect(popup);
    let dpi = unsafe { GetDpiForWindow(search) } as i32;
    let px = |dip| (dip * dpi + 48) / 96;
    assert_eq!(panel_bounds.left, search_bounds.left);
    assert_eq!(panel_bounds.right, search_bounds.right);
    assert!(height(popup) > px(120));
    before_timeout("history surface is painted", || {
        (printed_pixels(popup, &[(px(10), px(19))], 2)[0] == 0xfbf8f7).then_some(())
    });
    let match_points: Vec<_> = (px(50)..px(540))
        .flat_map(|x| (px(50)..px(101)).map(move |y| (x, y)))
        .collect();
    let pixels = printed_pixels(popup, &match_points, 2);
    assert!(
        pixels.iter().any(|pixel| {
            let [red, green, blue, _] = pixel.to_le_bytes();
            (203..=208).contains(&red)
                && (221..=226).contains(&green)
                && (243..=247).contains(&blue)
        }),
        "multilingual keyword highlight is drawn in the native panel"
    );
    for _ in 0..records.len() {
        key(VK_DOWN);
    }
    key(VK_TAB);
    assert_eq!(current_text(), records.last().unwrap().query);
    assert!(visible_window(popup_class, pid).is_none());
    let popup = history_popup("Unicode");
    let popup_bounds = window_rect(popup);
    let pointer = ((popup_bounds.top + px(55)) << 16) | ((popup_bounds.left + px(20)) & 0xffff);
    for _ in 0..24 {
        send(
            edit,
            WM_MOUSEWHEEL,
            usize::from((-120i16) as u16) << 16,
            pointer as isize,
        );
    }
    key(VK_TAB);
    assert_ne!(current_text(), long_query);
    assert!(current_text().starts_with("Unicode 🧪 候选测试"));
    history_popup("unmatched-yi-sou");
    key(VK_DOWN);
    key(VK_TAB);
    assert_eq!(current_text(), "unmatched-yi-sou");
    key(VK_ESCAPE);
    assert!(visible_window(popup_class, pid).is_none());
    history_popup("unmatched-yi-sou");
    key(VK_RETURN);
    assert_eq!(last_record().query, "unmatched-yi-sou");
    reopen();

    let popup = history_popup("候选测试 2");
    let point = ((px(60) << 16) | px(32)) as isize;
    send(popup, WM_MOUSEMOVE, 0, point);
    send(popup, WM_LBUTTONUP, 0, point);
    assert_eq!(last_record().query, records[2].query);
    assert_eq!(last_record().origin, Origin::History);
    reopen();

    history_popup("rust");
    key(VK_DOWN);
    key(VK_DOWN);
    key(VK_TAB);
    assert_eq!(current_text(), keyword);
    assert!(visible_window(popup_class, pid).is_none());
    input("Rust 改过的查询");
    key(VK_RETURN);
    assert_eq!(last_record().engine_id, "google");
    assert_eq!(last_record().query, "Rust 改过的查询");
    reopen();

    history_popup("编译器");
    key(VK_DOWN);
    key(VK_RETURN);
    let from_history = last_record();
    assert_eq!(from_history.engine_id, "bing");
    assert_eq!(from_history.query, keyword);
    assert_eq!(from_history.origin, yisou::application::Origin::History);
}

fn press_shortcut(shortcut: yisou::shortcut::Shortcut) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use yisou::shortcut::{ALT, CONTROL, SHIFT, WIN};

    let modifiers: Vec<_> = [
        (ALT, VK_MENU),
        (CONTROL, VK_CONTROL),
        (SHIFT, VK_SHIFT),
        (WIN, VK_LWIN),
    ]
    .into_iter()
    .filter_map(|(mask, key)| (shortcut.modifiers & mask != 0).then_some(key))
    .collect();
    let key = shortcut.key as u16;
    let keys: Vec<_> = modifiers.iter().copied().chain([key]).collect();
    let down = keys.iter().copied().map(|key| (key, 0));
    let up = keys.iter().rev().copied().map(|key| (key, KEYEVENTF_KEYUP));
    let inputs: Vec<_> = down
        .chain(up)
        .map(|(key, flags)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        })
        .collect();
    assert_eq!(
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        },
        inputs.len() as u32
    );
}

#[cfg(feature = "ocr")]
fn check_ocr_selection(pid: u32, shortcut: yisou::shortcut::Shortcut, trace: &std::path::Path) {
    use windows_sys::Win32::{
        Graphics::Gdi::*,
        UI::{HiDpi::*, Input::KeyboardAndMouse::VK_RETURN},
    };

    let class = w!("YiSou.Rust.OcrSelection");
    let capture = || {
        press_shortcut(shortcut);
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
