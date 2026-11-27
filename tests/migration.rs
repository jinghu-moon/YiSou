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
use yisou::settings::{ScreenBackground, SelectionBinding, Settings};
use yisou::shortcut::{ALT, AltTaps, LEFT_ALT, Shortcut};
use yisou::storage::{JsonSettings, SettingsStore, StorageError};

#[test]
fn desktop_background_preserves_existing_choices_and_saves_gaussian_strength() {
    for (enabled, mode) in [
        (false, ScreenBackground::Dim),
        (true, ScreenBackground::System),
    ] {
        let preferences: yisou::settings::Preferences = serde_json::from_value(serde_json::json!({
            "screen_blur": enabled
        }))
        .unwrap();
        assert_eq!(preferences.screen_background, mode);
    }

    let data = tempfile::tempdir().unwrap();
    let mut store = JsonSettings::new(data.path().join("settings.json"));
    let mut settings = Settings::default();
    settings.preferences.screen_background = ScreenBackground::Gaussian;
    settings.preferences.background_blur_percent = 75;
    store.save(&settings).unwrap();
    assert_eq!(store.load().unwrap().preferences, settings.preferences);
}

#[derive(Clone, Default)]
struct TestBrowser {
    calls: Rc<RefCell<Vec<String>>>,
    paths: Rc<RefCell<Vec<Option<std::path::PathBuf>>>>,
    fail: Rc<Cell<bool>>,
}

impl Browser for TestBrowser {
    fn open(&mut self, url: &str, path: Option<&Path>) -> io::Result<()> {
        self.calls.borrow_mut().push(url.to_owned());
        self.paths.borrow_mut().push(path.map(Path::to_path_buf));
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
        mono_icon: Icon::Default,
        mono_color: None,
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
fn search_engine_icon_size_defaults_and_clamps_to_the_button_limit() {
    let preferences: yisou::settings::Preferences =
        serde_json::from_str(r#"{"engine_menu_icon_size":28}"#).unwrap();
    assert_eq!(preferences.search_engine_icon_size, 20);

    let mut settings = Settings::default();
    for (input, expected) in [(100, 36), (31, 31), (1, 1), (0, 20), (-5, 20)] {
        settings.preferences.search_engine_icon_size = input;
        settings.normalize().unwrap();
        assert_eq!(settings.preferences.search_engine_icon_size, expected);
    }
}

#[test]
fn search_width_defaults_clamps_and_survives_reload() {
    let mut settings = Settings::default();
    settings.preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.preferences.search_width, 580);

    for (requested, expected) in [(100, 320), (760, 760), (5000, 1600)] {
        settings.preferences.search_width = requested;
        settings.normalize().unwrap();
        let reloaded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(reloaded.preferences.search_width, expected);
    }
}

#[test]
fn material_tint_defaults_and_round_trips_as_percentages() {
    let mut settings = Settings::default();
    settings.preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.preferences.acrylic_tint_percent, 68);
    assert_eq!(settings.preferences.mica_tint_percent, 16);

    settings.preferences.acrylic_tint_percent = -5;
    settings.preferences.mica_tint_percent = 120;
    settings.normalize().unwrap();
    let reloaded: Settings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(reloaded.preferences.acrylic_tint_percent, 0);
    assert_eq!(reloaded.preferences.mica_tint_percent, 100);
}

#[test]
fn history_can_follow_the_entire_search_background() {
    use yisou::settings::{HistoryMaterial, Preferences, SearchMaterial, SearchTone};

    let mut prefs = Preferences::default();
    prefs.search_light_color = 0xd4e6f2;
    prefs.search_dark_color = 0x24354c;
    prefs.acrylic_tint_percent = 46;
    prefs.mica_tint_percent = 23;
    prefs.history_popup.material = HistoryMaterial::FollowSearch;
    prefs.history_popup.tone = SearchTone::Light;
    for (material, tint_percent) in [
        (SearchMaterial::Acrylic, 46),
        (SearchMaterial::Mica, 23),
        (SearchMaterial::Solid, 100),
    ] {
        prefs.search_material = material;
        for (tone, system_dark, dark) in [
            (SearchTone::System, false, false),
            (SearchTone::System, true, true),
            (SearchTone::Light, true, false),
            (SearchTone::Dark, false, true),
        ] {
            prefs.search_tone = tone;
            let popup = prefs.history_background(system_dark);
            assert_eq!(popup.material, material);
            assert_eq!(popup.dark, dark);
            assert_eq!(popup.color, if dark { 0x24354c } else { 0xd4e6f2 });
            assert_eq!(popup.tint_percent, tint_percent);
        }
    }
    let mut prefs: Preferences =
        serde_json::from_value(serde_json::to_value(prefs).unwrap()).unwrap();
    assert_eq!(prefs.history_popup.material, HistoryMaterial::FollowSearch);
    prefs.history_popup.material = HistoryMaterial::Acrylic;
    let popup = prefs.history_background(true);
    assert_eq!(popup.material, SearchMaterial::Acrylic);
    assert!(!popup.dark);
    assert_eq!(popup.color, prefs.history_popup.light_color);
    assert_eq!(popup.tint_percent, prefs.history_popup.acrylic_tint_percent);
}

#[test]
fn engine_enablement_reaches_default_alias_selection_and_pinned_slots() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonSettings::new(dir.path().join("settings.json"));
    let mut settings = Settings::default();
    settings.preferences.default_engine = "baidu".into();
    settings.preferences.pins = vec!["baidu".into(), "bing".into()];
    settings.preferences.selection = Some(SelectionBinding {
        shortcut: Shortcut {
            modifiers: ALT,
            key: b'S' as u32,
        },
        engine: "baidu".into(),
        preview: true,
    });
    let browser = TestBrowser::default();
    let mut app = Application::new(
        settings,
        store,
        browser.clone(),
        History::new(dir.path().join("history.json")),
    )
    .unwrap();
    app.set_engine_enabled("baidu", false).unwrap();
    assert_eq!(app.settings().preferences.default_engine, "bing");
    assert_eq!(
        app.settings()
            .preferences
            .selection
            .as_ref()
            .unwrap()
            .engine,
        "bing"
    );
    assert_eq!(app.pinned_engine(0), None);
    assert_eq!(app.pinned_engine(1), Some("bing"));
    assert!(
        app.suggestions("#bd")
            .iter()
            .all(|engine| engine.id != "baidu")
    );
    assert!(app.all_engines().iter().any(|engine| engine.id == "baidu"));
    let _ = app.search(typed("#bd example"));
    assert!(browser.calls.borrow()[0].contains("bing.com/search?q=%23bd%20example"));

    let ids = app
        .engines()
        .iter()
        .map(|engine| engine.id.clone())
        .collect::<Vec<_>>();
    for id in ids.iter().filter(|id| id.as_str() != "bing") {
        app.set_engine_enabled(id, false).unwrap();
    }
    let before = app.settings().clone();
    assert!(app.set_engine_enabled("bing", false).is_err());
    assert_eq!(app.settings(), &before);
    assert_eq!(
        JsonSettings::new(dir.path().join("settings.json"))
            .load()
            .unwrap(),
        before
    );
    app.set_engine_enabled("baidu", true).unwrap();
    assert_eq!(app.pinned_engine(0), Some("baidu"));
    let _ = app.search(typed("#bd example"));
    assert!(browser.calls.borrow()[1].contains("baidu.com"));
}

#[test]
fn filtered_engine_order_and_font_preferences_survive_export_and_reload() {
    use yisou::settings::{Font, FontFace, FontPreviewScript};
    let dir = tempfile::tempdir().unwrap();
    let make = |name: &str| {
        Application::new(
            Settings::default(),
            JsonSettings::new(dir.path().join(name)),
            TestBrowser::default(),
            History::new(dir.path().join("history.json")),
        )
        .unwrap()
    };
    let mut app = make("settings.json");
    let custom = app.save_engine(custom("unique")).unwrap();
    let ids = |engines: &[Engine]| engines.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
    let mut expected = ids(app.all_engines());
    let custom_index = expected.iter().position(|id| id == &custom).unwrap();
    expected.swap(0, custom_index);
    app.reorder_engines(&[custom.clone(), "bing".into()])
        .unwrap();
    assert_eq!(ids(app.all_engines()), expected);
    assert!(
        app.reorder_engines(&[custom.clone(), custom.clone()])
            .is_err()
    );
    app.set_engine_enabled("bing", false).unwrap();
    app.change_settings(|settings| {
        let prefs = &mut settings.preferences;
        prefs.font = Font::System("Arial".into());
        prefs.search_font_size = 32;
        prefs.search_font_face = FontFace {
            weight: 700,
            stretch: 5,
            style: 2,
        };
        prefs.font_preview_script = FontPreviewScript::Latin;
    })
    .unwrap();
    let full = dir.path().join("full.json");
    let pack = dir.path().join("engines.json");
    app.export_settings(&full).unwrap();
    app.export_engine_pack(&pack).unwrap();
    let mut imported = make("imported.json");
    imported.import_settings(&full).unwrap();
    assert_eq!(imported.settings(), app.settings());
    let mut merged = make("merged.json");
    merged.import_engine_pack(&pack).unwrap();
    assert_eq!(ids(merged.all_engines()), expected);
    assert!(!merged.settings().engine_enabled("bing"));
    assert_eq!(merged.settings().preferences.search_font_size, 20);
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
    // Preserve the C++ baseline's inputs and expected URLs.
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
    let restored = app.last_search().unwrap();
    assert_eq!(restored.engine.as_deref(), Some("google"));
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
    let mut restored = app.last_search().unwrap().clone();
    restored.engine = Some("baidu".into());
    assert!(matches!(app.search(restored), SearchOutcome::Opened { .. }));
    assert_eq!(
        browser.calls.borrow().last().unwrap(),
        "https://www.baidu.com/s?wd=%23g%20test"
    );
    let suggestions = app.history_suggestions("TEST").unwrap();
    assert_eq!(suggestions.len(), 2);
    assert_eq!(suggestions[0].engine_id, "baidu");
    assert_eq!(suggestions[1].engine_id, "bing");
    assert!(matches!(
        app.search(suggestions[0].clone().repeat()),
        SearchOutcome::Opened { .. }
    ));
    assert_eq!(app.history_suggestions("test").unwrap().len(), 2);
    assert!(app.open_link(" https://example.org/?q=a=b ").unwrap());
    assert!(!app.open_link("file:///C:/").unwrap());
    let history = app.history().page("test", "terminal", 0).unwrap();
    assert_eq!(history.len(), 3);
    let clipboard = history.last().unwrap();
    assert_eq!(clipboard.origin, Origin::Clipboard);
    app.history().release_cache();
    assert_eq!(app.history().page("test", "terminal", 0).unwrap(), history);
    for record in &history {
        assert!(app.history().delete(record.id).unwrap());
    }
    app.history().release_cache();
    assert!(
        app.history()
            .page("test", "terminal", 0)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        app.search(clipboard.clone().repeat()),
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert_eq!(
        app.history().page("test", "terminal", 0).unwrap()[0].origin,
        Origin::History
    );
    app.change_settings(|settings| settings.preferences.record_history = false)
        .unwrap();
    assert_eq!(app.history_suggestions("test").unwrap().len(), 1);
    app.change_settings(|settings| settings.preferences.history_popup.enabled = false)
        .unwrap();
    assert!(app.history_suggestions("test").unwrap().is_empty());
    assert_eq!(app.history().count("test", "").unwrap(), 1);
}

#[test]
fn system_browser_retries_a_single_operation_and_preserves_settings() {
    let directory = tempfile::tempdir().unwrap();
    let browser = TestBrowser::default();
    let store = JsonSettings::new(directory.path().join("settings.json"));
    let custom_browser = directory.path().join("browser.exe");
    let mut settings = Settings::default();
    settings.preferences.browser = Some(custom_browser.clone());
    settings.preferences.record_history = true;
    let mut app = Application::new(
        settings,
        store,
        browser.clone(),
        History::new(directory.path().join("history.json")),
    )
    .unwrap();

    browser.fail.set(true);
    let query = clipboard_query("#g 中文", "Terminal".into());
    let SearchOutcome::Failed { query, .. } = app.search(query) else {
        panic!("custom browser should fail");
    };
    assert!(app.open_link("https://example.org/").is_err());
    assert!(app.history().records().unwrap().is_empty());

    browser.fail.set(false);
    assert!(matches!(
        app.search_with_system_browser(query),
        SearchOutcome::Opened {
            history_error: None
        }
    ));
    assert!(app.open_system_link("https://example.org/").unwrap());
    assert!(app.open_link("https://example.org/").unwrap());
    assert_eq!(
        browser.paths.borrow().as_slice(),
        &[
            Some(custom_browser.clone()),
            Some(custom_browser.clone()),
            None,
            None,
            Some(custom_browser.clone())
        ]
    );
    assert_eq!(app.settings().preferences.browser, Some(custom_browser));
    let history = app.history().records().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].query, "#g 中文");
    assert_eq!(history[0].origin, Origin::Clipboard);
    assert_eq!(history[0].source, "Terminal");
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
    let SelectionAction::Preview(query) =
        requests.finish(empty, 1, now, "", String::new(), &settings)
    else {
        panic!("an empty dedicated selection must let the user enter a query");
    };
    assert!(query.text.is_empty());
    assert_eq!(query.engine.as_deref(), Some("google"));
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
        for (edge, color) in [20, 28, 36, 42, 72]
            .into_iter()
            .flat_map(|edge| [(edge, false), (edge, true)])
        {
            let pixels = cache.get_variant(&engine, edge, color).unwrap();
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
    assert_eq!(&initial, cache.get_variant(&engine, 24, true).unwrap());
    engine.color = 0xff0000;
    assert_ne!(&initial, cache.get(&engine, 24).unwrap());
    engine.mono_icon = import_bitmap(include_bytes!("../assets/icons/youtube.png")).unwrap();
    let mono = cache.get_variant(&engine, 24, false).unwrap().to_vec();
    let color = cache.get_variant(&engine, 24, true).unwrap().to_vec();
    assert_ne!(mono, color);
    engine.icon = Icon::Default;
    assert_eq!(mono, cache.get_variant(&engine, 24, true).unwrap());
    assert!(
        matches!(import_bitmap(include_bytes!("../assets/icons/youtube.png")), Some(Icon::Bgra32(pixels)) if pixels.len() == 1024)
    );
}

#[test]
fn x_search_and_existing_aliases() {
    let mut settings = Settings::default();
    let engines = settings.engines();
    let x = engines.iter().find(|engine| engine.id == "x").unwrap();
    assert_eq!(x.alias, "x");
    assert_eq!(
        search_url(&x.url, "hello world"),
        "https://x.com/search?q=hello%20world"
    );
    let mut existing = custom("X");
    existing.id = "custom-1".into();
    settings.custom_engines.push(existing);
    settings.normalize().unwrap();
    let engines = settings.engines();
    assert_eq!(
        engines
            .iter()
            .find(|engine| engine.alias == "x")
            .unwrap()
            .id,
        "custom-1"
    );
}

#[test]
fn monochrome_tint_preserves_alpha_and_color_mode() {
    let mut cache = IconCache::default();
    let builtin = built_in_engines()
        .into_iter()
        .find(|e| e.id == "google")
        .unwrap();
    let mut svg = custom("svg");
    svg.mono_icon = import_svg(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="#0044dd"/></svg>"##
            .to_vec(),
    ).unwrap();
    let mut bitmap = custom("bitmap");
    bitmap.icon = import_bitmap(include_bytes!("../assets/icons/youtube.png")).unwrap();

    for mut engine in [builtin, svg, bitmap] {
        let original = cache.get_variant(&engine, 24, false).unwrap().to_vec();
        let multicolor = cache.get_variant(&engine, 24, true).unwrap().to_vec();
        engine.mono_color = Some(0xff0000);
        let tinted = cache.get_variant(&engine, 24, false).unwrap();
        assert_ne!(tinted, original);
        assert!(tinted.iter().zip(&original).all(|(&pixel, &original)| {
            let alpha = original >> 24;
            pixel == (alpha << 24 | alpha << 16)
        }));
        assert_eq!(cache.get_variant(&engine, 24, true).unwrap(), multicolor);
        let json = serde_json::to_string(&engine).unwrap();
        assert_eq!(serde_json::from_str::<Engine>(&json).unwrap(), engine);
        engine.mono_color = None;
        assert_eq!(cache.get_variant(&engine, 24, false).unwrap(), original);
    }
}
