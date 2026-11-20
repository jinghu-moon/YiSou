use super::{components::Page, *};

pub(super) fn build(state: &State, hwnd: HWND) {
    let page = Page(hwnd);
    let (id, editing, browser_panel) =
        PANEL.with_borrow(|p| (p.page, p.edit_engine, p.browser_panel));
    let engine = editing.then(|| edited_engine(state)).flatten();
    page.heading(if editing {
        match engine.as_ref() {
            Some(engine) if !engine.is_custom() => "编辑别名",
            Some(_) => "编辑搜索引擎",
            None => "添加搜索引擎",
        }
    } else {
        PAGES[id]
    });
    let prefs = state.app.borrow().settings().preferences.clone();
    let fixed = |x, y, w, h| place(x, y, w, h, 0);
    let right = |x, y, w, h| place(x, y, w, h, RIGHT);
    let stretch = |x, y, w, h| place(x, y, w, h, STRETCH);
    let button = |id, text: &str, area| action(hwnd, id, text, area);
    match id {
        0 => {
            page.section("启动与后台", 98, 272, &[156, 214]);
            for (index, (name, subtitle, enabled)) in [
                (
                    "登录 Windows 时启动",
                    "登录后在系统托盘等待。",
                    prefs.start_with_windows,
                ),
                (
                    "手动启动时显示搜索框",
                    "关闭后，手动启动也只驻留托盘。",
                    prefs.show_on_manual_launch,
                ),
                (
                    "关闭搜索框后驻留托盘",
                    "关闭后，按 Esc 会退出易搜。",
                    prefs.close_to_tray,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 98 + index as i32 * 58;
                page.setting(name, subtitle, y, 65);
                page.toggle(100 + index, name, enabled, y);
            }
            page.section("界面语言", 316, 373, &[]);
            page.setting("界面语言", "", 316, 251);
            let language = match prefs.language {
                Language::System => 0,
                Language::Chinese => 1,
                Language::English => 2,
            };
            combo(
                hwnd,
                103,
                &["跟随系统", "简体中文", "English"],
                language,
                right(283, 327, 146, 34),
            );
            unsafe { EnableWindow(button(104, "应用", right(440, 328, 82, 32)), 0) };
            page.finish_at(393);
        }
        1 => {
            page.section("搜索框字体", 98, 279, &[159]);
            let current = if let Font::File(path) = &prefs.font {
                format!("{} · {}", state.font_family(), path.display())
            } else {
                state.font_family()
            };
            page.setting("当前字体", &current, 98, 143);
            button(112, "恢复默认", right(408, 111, 114, 32));
            unsafe {
                EnableWindow(
                    GetDlgItem(hwnd, 112),
                    i32::from(prefs.font != Font::Default),
                )
            };
            button(111, "选择系统字体", fixed(14, 172, 140, 32));
            button(113, "打开字体文件…", fixed(162, 172, 134, 32));
            page.detail("20 DIP 搜索框预览", stretch(14, 211, 502, 19));
            let preview = caption(
                hwnd,
                "复杂中文 鬱鑿齒 · Search 123",
                stretch(25, 235, 444, 31),
                fluent::FontRole::Preview,
            );
            unsafe { SetWindowLongPtrW(preview, GWLP_ID, 115) };
            let font = state.create_font_at(unsafe { GetDpiForWindow(hwnd) }.max(96), 20);
            PANEL.with_borrow_mut(|p| p.preview_font = font);
            page.section("搜索引擎菜单", 323, 473, &[378]);
            page.setting("磁贴样式", "", 323, 251);
            button(110, "显示名称", right(284, 335, 114, 32));
            button(114, "仅显示图标", right(398, 335, 124, 32));
            page.finish_at(493);
        }
        2 if editing => {
            if let Some(engine) = engine.as_ref().filter(|engine| !engine.is_custom()) {
                page.section("", 77, 240, &[129]);
                page.body(&engine.name, stretch(14, 89, 508, 29));
                page.field(21, "别名（可选）", &engine.alias, 14, 141, 348, STRETCH);
                button(119, "恢复默认别名", right(378, 169, 144, 30));
                let status = page.detail("", stretch(14, 205, 508, 24));
                unsafe { SetWindowLongPtrW(status, GWLP_ID, EDIT_STATUS as isize) };
                button(128, "取消", right(323, 258, 96, 34));
                button(126, "保存", right(428, 258, 108, 34));
                set_draft(hwnd);
                page.finish_at(311);
                return;
            }
            let (name, alias, url, color) = engine.as_ref().map_or_else(
                || (String::new(), String::new(), String::new(), "005FB8".into()),
                |e| {
                    (
                        e.name.clone(),
                        e.alias.clone(),
                        e.url.clone(),
                        format!("{:06X}", e.color),
                    )
                },
            );
            page.section("", 77, 381, &[258]);
            page.field(20, "名称", &name, 14, 88, 307, STRETCH);
            page.field(21, "别名（可选）", &alias, 335, 88, 187, RIGHT);
            let url_field = page.field(22, "搜索网址", &url, 14, 164, 508, STRETCH);
            cue(url_field, "https://example.com/search?q=%s");
            page.detail("使用 %s 代表要搜索的关键词", stretch(14, 225, 508, 22));
            page.body("图标", fixed(14, 267, 118, 20));
            button(131, "选择纯色", fixed(67, 290, 110, 32));
            button(127, "导入本地图标", fixed(185, 290, 136, 32));
            model_field(hwnd, 23, &color);
            model_field(hwnd, 24, "");
            if let Some(engine) = engine
                .as_ref()
                .filter(|engine| !matches!(engine.icon, Icon::Default))
            {
                PANEL.with_borrow_mut(|p| {
                    p.view.engine_icon = Some(engines::preview_icon(engine.icon.clone()))
                });
                fluent::reset_images();
            }
            set_draft(hwnd);
            let status = page.detail("", stretch(14, 342, 508, 25));
            unsafe { SetWindowLongPtrW(status, GWLP_ID, EDIT_STATUS as isize) };
            button(125, "测试搜索", fixed(0, 399, 115, 34));
            button(128, "取消", right(323, 399, 96, 34));
            button(126, "保存", right(428, 399, 108, 34));
            page.finish_at(453);
        }
        2 => engines::display(state, hwnd),
        3 => {
            page.section(
                "全局快捷键",
                98,
                if cfg!(feature = "ocr") { 390 } else { 332 },
                &[156, 274, 332],
            );
            shortcut(
                page,
                137,
                145,
                20,
                "唤出易搜",
                "有选区时直接搜索，其他时候打开搜索框。",
                &format_shortcut(prefs.main_shortcut),
                98,
            );
            let selection = prefs
                .selection
                .as_ref()
                .map(|b| format_shortcut(b.shortcut))
                .unwrap_or_default();
            shortcut(page, 138, 146, 21, "划词搜索", "", &selection, 156);
            engine_combo(state, hwnd, stretch(14, 224, 351, 34));
            combo(
                hwnd,
                135,
                &["直接搜索", "打开搜索框"],
                usize::from(prefs.selection.as_ref().is_none_or(|b| b.preview)),
                right(376, 224, 146, 34),
            );
            let clipboard = prefs
                .clipboard_shortcut
                .map(format_shortcut)
                .unwrap_or_default();
            shortcut(
                page,
                139,
                147,
                22,
                "剪贴板搜索",
                "使用默认引擎搜索剪贴板里的文字。",
                &clipboard,
                274,
            );
            #[cfg(feature = "ocr")]
            {
                page.setting("OCR 快捷键", "", 332, 135);
                button(176, "前往设置", right(400, 344, 122, 32));
            }
            let excluded = if cfg!(feature = "ocr") { 434 } else { 376 };
            page.section("双击 Alt 排除的应用", excluded, excluded + 64, &[]);
            let field = edit_at(
                hwnd,
                23,
                &prefs.excluded_apps.join(", "),
                stretch(22, excluded + 20, 389, 22),
            );
            cue(field, "例如 mstsc.exe");
            button(134, "应用", right(431, excluded + 14, 91, 34));
            page.finish_at(excluded + 90);
        }
        4 => {
            if browser_panel == BrowserPanel::Path {
                page.section("搜索结果浏览器", 98, 224, &[174]);
                let value = prefs
                    .browser
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                page.field(20, "浏览器程序", &value, 14, 107, 383, STRETCH);
                button(143, "浏览…", right(415, 132, 107, 34));
                button(1440, "取消", right(327, 185, 92, 32));
                button(141, "应用", right(428, 185, 94, 32));
            } else {
                let browsers = platform::installed_browsers();
                let default = platform::default_browser();
                let default_option = format!(
                    "{}\t{}",
                    tr("跟随 Windows 默认浏览器"),
                    default
                        .as_ref()
                        .map_or_else(String::new, |p| p.display().to_string())
                );
                let mut applied = prefs.browser.as_ref().and_then(|path| {
                    browsers
                        .iter()
                        .position(|b| &b.path == path)
                        .map(|index| index + 1)
                });
                let custom = prefs.browser.as_deref().filter(|_| applied.is_none());
                if custom.is_some() {
                    applied = Some(browsers.len() + 1);
                }
                page.section("搜索结果浏览器", 98, 0, &[]);
                let rows = std::iter::once(default_option)
                    .chain(
                        browsers
                            .iter()
                            .map(|b| format!("{}\t{}", b.name, b.path.display())),
                    )
                    .chain(custom.map(|path| {
                        format!(
                            "{}\t{}",
                            path.file_stem().unwrap_or_default().to_string_lossy(),
                            path.display()
                        )
                    }));
                let list = list_at(hwnd, rows, stretch(1, 99, 534, -155));
                unsafe { SendMessageW(list, LB_SETCURSEL, applied.unwrap_or(0), 0) };
                button(142, "选择其他浏览器…", fixed(0, -45, 186, 32).bottom());
                button(140, "应用", right(428, -45, 108, 32).bottom());
                PANEL.with_borrow_mut(|p| p.browsers = browsers);
            }
        }
        5 => {
            page.section("本机搜索记录", 98, 156, &[]);
            page.setting("记录搜索历史", "关闭后仍可查看已有记录。", 98, 70);
            page.toggle(150, "记录搜索历史", prefs.record_history, 98);
            let (keyword, source) =
                PANEL.with_borrow(|p| (p.history_keyword.clone(), p.history_source.clone()));
            page.field(25, "搜索内容", &keyword, 0, 167, 274, STRETCH);
            page.field(26, "来源应用", &source, 288, 167, 248, RIGHT);
            list_at(hwnd, std::iter::empty(), stretch(0, 233, 536, -277));
            button(155, "清除筛选", fixed(0, -38, 112, 30).bottom());
            button(153, "上一页", right(324, -38, 102, 30).bottom());
            button(154, "下一页", right(434, -38, 102, 30).bottom());
            populate_history(state, hwnd, false);
        }
        6 => {
            page.section("当前配置", 98, 218, &[]);
            let path = page.field(
                166,
                "配置文件路径",
                &state.config.display().to_string(),
                14,
                104,
                508,
                STRETCH,
            );
            unsafe { SendMessageW(path, windows_sys::Win32::UI::Controls::EM_SETREADONLY, 1, 0) };
            button(165, "更换位置…", fixed(14, 173, 140, 32));
            let location = PANEL
                .with_borrow(|p| p.next_config.clone())
                .map(|path| format!("{} {}", tr("下次启动使用"), path.display()));
            if let Some(location) = location {
                page.detail(&location, stretch(163, 177, 356, 24));
            }
            page.section("完整配置", 262, 326, &[]);
            button(161, "导入配置…", fixed(14, 278, 131, 32));
            button(162, "导出配置…", fixed(153, 278, 131, 32));
            page.section("搜索引擎包", 370, 434, &[]);
            button(163, "导入引擎…", fixed(14, 386, 131, 32));
            button(164, "导出引擎…", fixed(153, 386, 131, 32));
            page.finish_at(454);
        }
        #[cfg(feature = "ocr")]
        OCR_PAGE => ocr::display(page, &prefs),
        _ => {}
    }
}

pub(super) const RECORD_STATUS: usize = 158;
pub(super) const EDIT_STATUS: usize = 133;

pub(super) fn shortcut(
    page: Page,
    target: usize,
    reset: usize,
    value_id: usize,
    title: &str,
    detail: &str,
    value: &str,
    top: i32,
) {
    let hwnd = page.0;
    page.setting(title, detail, top, 271);
    model_field(hwnd, value_id, value);
    let record = action(
        hwnd,
        target,
        if value.is_empty() { "未设置" } else { value },
        place(280, top + 12, 153, 34, RIGHT),
    );
    let reset_label = if target == 137 {
        "恢复默认"
    } else {
        "清除"
    };
    let reset_button = action(
        hwnd,
        reset,
        reset_label,
        place(441, top + 12, 81, 34, RIGHT),
    );
    unsafe {
        EnableWindow(
            reset_button,
            i32::from(!value.is_empty() && (target != 137 || value != "双击左 Alt")),
        )
    };
    let _ = record;
}
