use std::cell::{Cell, RefCell};
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use yisou::application::{
    Application, Browser, History, Origin, Query, SearchOutcome, SelectionAction, SelectionMode,
    SelectionRequests, clipboard_query, limit_text,
};
use yisou::engine::{
    Engine, Icon, built_in_engines, is_search_template, is_web_url, search_url, trim,
};
use yisou::icons::{IconCache, import_bitmap, import_svg};
use yisou::launch::{Launch, WindowAction, close_search};
use yisou::settings::{SelectionBinding, Settings};
use yisou::shortcut::{ALT, AltTaps, LEFT_ALT, Shortcut};
use yisou::storage::{JsonSettings, SettingsStore, StorageError};

#[derive(Clone, Default)]
struct TestBrowser {
    calls: Rc<RefCell<Vec<String>>>,
    fail: Rc<Cell<bool>>,
}

impl Browser for TestBrowser {
    fn open(&mut self, url: &str, _: Option<&Path>) -> io::Result<()> {
        self.calls.borrow_mut().push(url.to_owned());
        if self.fail.get() {
            Err(io::Error::other("browser failure"))
        } else {
            Ok(())
        }
    }
}

fn custom(alias: &str) -> Engine {
    Engine {
        id: String::new(),
        name: alias.into(),
        alias: alias.into(),
        url: "https://example.org/search?q=%s&copy=%s".into(),
        color: 0x005fb8,
        icon: Icon::Default,
    }
}

fn typed(text: &str) -> Query {
    Query {
        text: text.into(),
        engine: None,
        origin: Origin::Typed,
        source: String::new(),
    }
}

#[test]
fn launch_and_close_route_owned_preferences() {
    let mut prefs = Settings::default().preferences;
    let parse = |args: &[&str]| Launch::parse(args.iter().map(|arg| (*arg).into()));
    assert_eq!(
        parse(&[]).unwrap().action(&prefs, false),
        WindowAction::ShowSearch
    );
    prefs.show_on_manual_launch = false;
    assert_eq!(
        parse(&[]).unwrap().action(&prefs, false),
        WindowAction::StayInTray
    );
    assert_eq!(
        parse(&[]).unwrap().action(&prefs, true),
        WindowAction::ShowSearch
    );
    assert_eq!(
        parse(&["--startup"]).unwrap().action(&prefs, false),
        WindowAction::StayInTray
    );
    let configuration = parse(&["--configuration", "--config", "配置 路径.json"]).unwrap();
    assert_eq!(
        configuration.action(&prefs, false),
        WindowAction::ShowConfiguration
    );
    assert_eq!(
        configuration.config.as_deref(),
        Some(Path::new("配置 路径.json"))
    );
    assert!(parse(&["--config"]).is_err());
    assert_eq!(close_search(&prefs), WindowAction::StayInTray);
    prefs.close_to_tray = false;
    assert_eq!(close_search(&prefs), WindowAction::Exit);
}

#[test]
fn cpp_search_cases_and_url_boundaries() {
    // Keep the inputs and expected URLs in sync with tests/search_cases.h.
    let engines = built_in_engines();
    let cases = [
        (0, " \t\r\n", ""),
        (
            0,
            " \talpha beta\r\n",
            "https://www.bing.com/search?q=alpha%20beta",
        ),
        (
            1,
            "中文 + &/%#?",
            "https://www.baidu.com/s?wd=%E4%B8%AD%E6%96%87%20%2B%20%26%2F%25%23%3F",
        ),
        (2, "🔍", "https://www.google.com/search?q=%F0%9F%94%8D"),
        (0, "AZaz09-._~", "https://www.bing.com/search?q=AZaz09-._~"),
    ];
    for (index, query, expected) in cases {
        let query = trim(query);
        let actual = if query.is_empty() {
            String::new()
        } else {
            search_url(&engines[index].url, query)
        };
        assert_eq!(actual, expected);
    }
    for invalid in [
        "file:///C:/",
        "https://",
        "https://user@site.org",
        "https://exa%mple.org",
        "https://example.org/\nx",
        "https://example.org/\"",
        "https://example.org\\abc",
    ] {
        assert!(!is_web_url(invalid), "{invalid}");
    }
    assert!(is_web_url(" HTTPS://localhost:443?q=a=b "));
    assert!(is_search_template("https://example.org/?a=%s&b=%s"));
    assert!(!is_search_template("https://%s.example.org/"));
    assert_eq!(
        search_url("https://example.org/?a=%s&b=%s", "a\nb"),
        "https://example.org/?a=a%0Ab&b=a%0Ab"
    );
}

#[test]
fn search_retry_history_and_alias_sources() {
    let directory = tempfile::tempdir().unwrap();
    let browser = TestBrowser::default();
    let store = JsonSettings::new(directory.path().join("settings.json"));
    let mut app = Application::new(
        store.load().unwrap(),
        store,
        browser.clone(),
        History::new(directory.path().join("history.json")),
    )
    .unwrap();
    assert!(matches!(app.search(typed("#g")), SearchOutcome::Empty));
    assert!(matches!(
        app.search(typed("#missing test")),
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert!(app.history().page("", "", 0).unwrap().is_empty());
    app.change_settings(|settings| settings.preferences.record_history = true)
        .unwrap();
    app.toggle_pin("baidu").unwrap();
    assert_eq!(app.pinned_engine(0), Some("baidu"));
    assert_eq!(app.suggestions("#BD")[0].id, "baidu");
    assert!(app.suggestions("#BD ").is_empty());
    let mut query = typed("#G 测试");
    query.engine = Some("baidu".into());
    browser.fail.set(true);
    let SearchOutcome::Failed { query: retry, .. } = app.search(query.clone()) else {
        panic!("expected a failed search")
    };
    assert_eq!(query, retry);
    assert!(app.history().page("", "", 0).unwrap().is_empty());
    browser.fail.set(false);
    assert!(matches!(
        app.search(retry),
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert_eq!(
        browser.calls.borrow().last().unwrap(),
        "https://www.google.com/search?q=%E6%B5%8B%E8%AF%95"
    );
    assert_eq!(app.last_query(), "测试");
    assert!(matches!(
        app.search(clipboard_query("#g test", "Terminal".into())),
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert!(
        browser
            .calls
            .borrow()
            .last()
            .unwrap()
            .contains("bing.com/search?q=%23g%20test")
    );
    assert!(app.open_link(" https://example.org/?q=a=b ").unwrap());
    assert!(!app.open_link("file:///C:/").unwrap());
    let history = app.history().page("test", "terminal", 0).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].origin, Origin::Clipboard);
    app.history().release_cache();
    assert_eq!(app.history().page("test", "terminal", 0).unwrap(), history);
    assert!(app.history().delete(history[0].id).unwrap());
    app.history().release_cache();
    assert!(
        app.history()
            .page("test", "terminal", 0)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        app.search(history[0].clone().repeat()),
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert_eq!(
        app.history().page("test", "terminal", 0).unwrap()[0].origin,
        Origin::History
    );
}

#[test]
fn ocr_search_preserves_alias_like_text_and_records_origin() {
    let directory = tempfile::tempdir().unwrap();
    let browser = TestBrowser::default();
    let store = JsonSettings::new(directory.path().join("settings.json"));
    let mut app = Application::new(
        store.load().unwrap(),
        store,
        browser.clone(),
        History::new(directory.path().join("history.json")),
    )
    .unwrap();
    app.change_settings(|settings| settings.preferences.record_history = true)
        .unwrap();
    let text = "#g cargo error E0425";
    let result = app.search(Query {
        text: text.into(),
        engine: Some("bing".into()),
        origin: Origin::Ocr,
        source: String::new(),
    });
    assert!(matches!(
        result,
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert!(browser.calls.borrow()[0].contains("bing.com/search?q=%23g%20cargo%20error%20E0425"));
    let history = app.history().page("", "", 0).unwrap();
    assert_eq!(history[0].origin, Origin::Ocr);
    assert_eq!(history[0].query, text);
}

#[test]
fn config_and_engines_round_trip_and_failed_commit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let store = JsonSettings::new(path.clone());
    let mut app = Application::new(
        Settings::default(),
        store,
        TestBrowser::default(),
        History::new(directory.path().join("history.json")),
    )
    .unwrap();
    let first = app.save_engine(custom("first")).unwrap();
    let second = app.save_engine(custom("second")).unwrap();
    assert!(app.save_engine(custom("#FIRST")).is_err());
    assert!(app.delete_engine("bing").is_err());
    assert!(app.move_engine(&first, -1).is_err());
    app.move_engine(&first, 1).unwrap();
    app.toggle_pin(&first).unwrap();
    let pack = directory.path().join("engines.json");
    let all = directory.path().join("export.json");
    app.export_engine_pack(&pack).unwrap();
    app.export_settings(&all).unwrap();
    app.delete_engine(&first).unwrap();
    assert_eq!(app.pinned_engine(0), None);
    app.import_engine_pack(&pack).unwrap();
    assert_eq!(app.settings().custom_engines[0].id, second);
    assert_eq!(app.settings().custom_engines[1].id, first);
    assert_eq!(app.pinned_engine(0), Some(first.as_str()));
    app.import_settings(&all).unwrap();
    let expected = app.settings().clone();
    assert_eq!(JsonSettings::new(path.clone()).load().unwrap(), expected);
    std::fs::write(&all, "{bad json}").unwrap();
    assert!(app.import_settings(&all).is_err());
    assert_eq!(app.settings(), &expected);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(path).unwrap();
    assert!(
        app.change_settings(|settings| settings.preferences.record_history = true)
            .is_err()
    );
    assert_eq!(app.settings(), &expected);
}

#[test]
fn built_in_aliases_override_search_and_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let store = JsonSettings::new(path.clone());
    let browser = TestBrowser::default();
    let mut app = Application::new(
        Settings::default(),
        JsonSettings::new(path),
        browser.clone(),
        History::new(directory.path().join("history.json")),
    )
    .unwrap();
    let original = app
        .engines()
        .iter()
        .find(|e| e.id == "baidu")
        .unwrap()
        .clone();
    app.toggle_pin("baidu").unwrap();
    app.save_engine(custom("custom-alias")).unwrap();
    app.set_built_in_alias("baidu", " #BAIDU2 ".into()).unwrap();
    let expected = app.settings().clone();
    for alias in ["g", "#CUSTOM-ALIAS", "1invalid"] {
        assert!(app.set_built_in_alias("baidu", alias.into()).is_err());
    }
    assert!(app.set_built_in_alias("missing", "new".into()).is_err());
    assert!(app.save_engine(custom("BAIDU2")).is_err());
    assert_eq!(store.load().unwrap(), expected);
    assert_eq!(app.pinned_engine(0), Some("baidu"));
    assert_eq!(app.suggestions("#baidu2")[0].id, "baidu");
    assert!(app.suggestions("#bd").iter().all(|e| e.id != "baidu"));
    assert!(matches!(
        app.search(typed("#BAIDU2 中文")),
        SearchOutcome::Opened { .. }
    ));
    assert_eq!(browser.calls.borrow()[0], search_url(&original.url, "中文"));

    let pack = directory.path().join("engines.json");
    app.export_engine_pack(&pack).unwrap();
    app.set_built_in_alias("baidu", String::new()).unwrap();
    assert!(
        app.engines()
            .iter()
            .find(|e| e.id == "baidu")
            .unwrap()
            .alias
            .is_empty()
    );
    app.import_engine_pack(&pack).unwrap();
    assert_eq!(store.load().unwrap(), expected);
    app.set_built_in_alias("baidu", original.alias.clone())
        .unwrap();
    assert_eq!(
        app.engines().iter().find(|e| e.id == "baidu"),
        Some(&original)
    );
}

#[test]
fn settings_store_receives_validated_changes_only() {
    struct Store(Rc<Cell<u32>>);
    impl SettingsStore for Store {
        fn save(&mut self, _: &Settings) -> Result<(), StorageError> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let calls = Rc::new(Cell::new(0));
    let mut app = Application::new(
        Settings::default(),
        Store(calls.clone()),
        TestBrowser::default(),
        History::new(directory.path().join("history.json")),
    )
    .unwrap();
    assert!(
        app.change_settings(|settings| settings.preferences.default_engine = "unknown".into())
            .is_err()
    );
    assert_eq!(calls.get(), 0);
    app.change_settings(|settings| settings.preferences.default_engine = "google".into())
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(app.settings().preferences.default_engine, "google");
}

#[test]
fn selection_ignores_cancelled_foreground_and_expired_results() {
    let mut settings = Settings::default();
    settings.preferences.selection = Some(SelectionBinding {
        shortcut: Shortcut {
            modifiers: ALT,
            key: 65,
        },
        engine: "google".into(),
        preview: true,
    });
    let now = Instant::now();
    let mut requests = SelectionRequests::default();
    let stale = requests.begin(1, SelectionMode::Main, false, now);
    let another = requests.begin(1, SelectionMode::Main, false, now);
    assert!(matches!(
        requests.finish(stale, 1, now, "old", String::new(), &settings),
        SelectionAction::Ignore
    ));
    assert!(matches!(
        requests.finish(another, 2, now, "other window", String::new(), &settings),
        SelectionAction::Ignore
    ));
    let expired = requests.begin(1, SelectionMode::Main, false, now);
    assert!(matches!(
        requests.finish(
            expired,
            1,
            now + Duration::from_millis(350),
            "expired",
            String::new(),
            &settings
        ),
        SelectionAction::ShowSearch
    ));
    let preview = requests.begin(1, SelectionMode::Dedicated, true, now);
    let SelectionAction::Preview(query) =
        requests.finish(preview, 1, now, "  #g\n  test ", "Editor".into(), &settings)
    else {
        panic!("expected preview")
    };
    assert_eq!(query.text, "#g test");
    assert_eq!(query.engine.as_deref(), Some("google"));
    assert_eq!(query.origin, Origin::Selection);
    assert_eq!(limit_text(&("x".repeat(2047) + "🔍")).len(), 2047);
    settings.preferences.selection.as_mut().unwrap().preview = false;
    let empty = requests.begin(1, SelectionMode::Dedicated, true, now);
    assert!(matches!(
        requests.finish(empty, 1, now, "", String::new(), &settings),
        SelectionAction::Ignore
    ));
}

#[test]
fn double_alt_taps_and_wrapping_clock() {
    let mut taps = AltTaps::default();
    assert!(!taps.key(LEFT_ALT, true, u32::MAX - 50, false));
    assert!(!taps.key(LEFT_ALT, false, u32::MAX - 30, false));
    assert!(!taps.key(LEFT_ALT, true, 20, false));
    assert!(taps.key(LEFT_ALT, false, 40, false));
    assert!(!taps.key(LEFT_ALT, true, 80, false));
    assert!(!taps.key(LEFT_ALT, false, 100, false));
    assert!(!taps.key(LEFT_ALT, true, 130, false));
    assert!(!taps.key(65, true, 145, false));
    assert!(!taps.key(LEFT_ALT, false, 160, false));
    assert!(!taps.key(LEFT_ALT, true, 200, false));
    assert!(!taps.key(LEFT_ALT, false, 600, false));
}

#[test]
fn icon_resources_and_premultiplied_pixels() {
    let mut cache = IconCache::default();
    for engine in built_in_engines() {
        for edge in [20, 28, 36, 42, 72] {
            let pixels = cache.get(&engine, edge).unwrap();
            assert_eq!(pixels.len(), (edge * edge) as usize);
            assert!(pixels.iter().any(|pixel| pixel >> 24 != 0));
            assert!(pixels.iter().all(|pixel| {
                let [b, g, r, a] = pixel.to_le_bytes();
                b <= a && g <= a && r <= a
            }));
        }
    }
    assert!(import_svg(b"not svg".to_vec()).is_none());
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="currentColor" d="M2 10Q9 1 12 3T22 9C16 19 18 23 10 19Z"/></svg>"#;
    let mut engine = custom("curve");
    engine.icon = import_svg(svg.to_vec()).unwrap();
    let initial = cache.get(&engine, 24).unwrap().to_vec();
    assert_eq!(&initial, cache.get(&engine, 24).unwrap());
    engine.color = 0xff0000;
    assert_ne!(&initial, cache.get(&engine, 24).unwrap());
    assert!(
        matches!(import_bitmap(include_bytes!("../../assets/icons/youtube.png")), Some(Icon::Bgra32(pixels)) if pixels.len() == 1024)
    );
}
