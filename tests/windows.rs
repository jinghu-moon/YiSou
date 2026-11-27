#![cfg(windows)]

use std::{ptr::null_mut, sync::mpsc};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WM_APP,
};
use yisou::platform::{
    Hotkey, HotkeySet, Instance, KeyBindings, installed_browsers, remember_settings_path,
    settings_path,
};
use yisou::shortcut::{ALT, CONTROL, SHIFT, Shortcut, WIN};

#[test]
fn instance_and_config_location() {
    let instance = Instance::acquire()
        .unwrap()
        .expect("run with the Rust app closed");
    assert!(Instance::acquire().unwrap().is_none());
    drop(instance);
    assert!(Instance::acquire().unwrap().is_some());

    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let app_data = root.join("app-data");
    let executable = root.join("YiSou.exe");
    assert_eq!(
        settings_path(None, &app_data, &executable).unwrap(),
        app_data.join("settings.json")
    );
    let portable = root.join("settings.json");
    std::fs::write(&portable, "{}").unwrap();
    assert_eq!(
        settings_path(None, &app_data, &executable).unwrap(),
        portable
    );
    let remembered = root.join("remembered.json");
    remember_settings_path(&app_data, &remembered).unwrap();
    assert_eq!(
        settings_path(None, &app_data, &executable).unwrap(),
        remembered
    );
    let explicit = root.join("配置文件.json");
    assert_eq!(
        settings_path(Some(explicit.clone()), &app_data, &executable).unwrap(),
        explicit
    );
}

#[test]
fn win32_hotkey_conflict_keeps_previous_binding() {
    let (send, take) = mpsc::sync_channel(0);
    let (stop, wait) = mpsc::sync_channel(0);
    let holder = std::thread::spawn(move || {
        let modifiers = MOD_SHIFT | MOD_CONTROL | MOD_ALT | MOD_WIN;
        let key = (0x7c..=0x87)
            .find(|&key| unsafe { RegisterHotKey(null_mut(), 1, modifiers, key) } != 0);
        send.send(key).unwrap();
        if key.is_some() {
            let _ = wait.recv();
            unsafe { UnregisterHotKey(null_mut(), 1) };
        }
    });
    let key = take.recv().unwrap().expect("an available test hotkey");
    let name: Vec<_> = "STATIC\0".encode_utf16().collect();
    let test_window = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!test_window.is_null());
    let mut hotkeys = unsafe { HotkeySet::new(test_window, WM_APP + 50) };
    let modifiers = SHIFT | CONTROL | ALT | WIN;
    let installed = (0x7c..=0x87)
        .filter(|candidate| *candidate != key)
        .any(|key| {
            hotkeys
                .apply(KeyBindings {
                    main: Some(Shortcut { modifiers, key }),
                    ..Default::default()
                })
                .is_ok()
        });
    let previous = (1..=12).find(|&id| hotkeys.role(id) == Some(Hotkey::Main));
    let conflicting = hotkeys.apply(KeyBindings {
        main: Some(Shortcut { modifiers, key }),
        ..Default::default()
    });
    let retained = previous.and_then(|id| hotkeys.role(id));
    drop(hotkeys);
    unsafe { DestroyWindow(test_window) };
    stop.send(()).unwrap();
    holder.join().unwrap();
    assert!(installed);
    assert!(conflicting.is_err());
    assert_eq!(retained, Some(Hotkey::Main));
}

#[test]
fn enumerated_browsers_are_existing_executables() {
    for browser in installed_browsers() {
        assert!(browser.path.is_absolute() && browser.path.is_file());
        assert!(
            browser
                .path
                .extension()
                .unwrap()
                .eq_ignore_ascii_case("exe")
        );
    }
}
