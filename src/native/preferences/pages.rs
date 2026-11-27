use super::{
    components::{
        BUTTON_GAP, BUTTON_ICON_SPACE, CARD_GAP, CONTROL_HEIGHT, FIRST_CARD, Page, ROW,
        ROW_WITH_DETAIL, SECTION_GAP, WIDTH, button_width,
    },
    fluent::{LEFT_HALF, RIGHT_HALF},
    *,
};
use crate::native::paint::Label;

pub(super) const ACCENT_HELP: usize = 190;
pub(super) const BROWSER_ROW: i32 = ROW_WITH_DETAIL + CARD_GAP;

macro_rules! tabler {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../../../assets/icons/tabler/", $name, ".svg")) as &[u8],
        )
    };
}

pub(super) fn build(state: &State, hwnd: HWND) {
    if PANEL.with_borrow(|p| p.font_picker.is_some()) {
        fonts::display(hwnd);
        return;
    }
    let page = Page(hwnd);
    let (id, editing, browser_panel) =
        PANEL.with_borrow(|p| (p.page, p.edit_engine, p.browser_panel));
    let engine = editing.then(|| edited_engine(state)).flatten();
    page.heading_with_trailing(
        if editing {
            match engine.as_ref() {
                Some(_) => "编辑搜索引擎",
                None => "添加搜索引擎",
            }
        } else {
            PAGES[id]
        },
        if id == 2 && !editing { 144 } else { 0 },
    );
    let prefs = state.app.borrow().settings().preferences.clone();
    let fixed = |x, y, w, h| place(x, y, w, h, 0);
    let right = |x, y, w, h| place(x, y, w, h, RIGHT);
    let stretch = |x, y, w, h| place(x, y, w, h, STRETCH);
    let right_buttons = |edge, y, items: &[(usize, &str)]| page.buttons(edge, y, RIGHT, items);
    match id {
        0 => {
            let pitch = ROW_WITH_DETAIL + CARD_GAP;
            page.cards("启动与后台", FIRST_CARD, &[ROW_WITH_DETAIL; 3]);
            for (index, (icon, name, subtitle, enabled)) in [
                (
                    tabler!("settings"),
                    "登录 Windows 时启动",
                    "登录后在系统托盘等待。",
                    prefs.start_with_windows,
                ),
                (
                    tabler!("search"),
                    "手动启动时显示搜索框",
                    "关闭后，手动启动也只驻留托盘。",
                    prefs.show_on_manual_launch,
                ),
                (
                    tabler!("browser"),
                    "关闭搜索框后驻留托盘",
                    "关闭后，按 Esc 会退出易搜。",
                    prefs.close_to_tray,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = FIRST_CARD + index as i32 * pitch;
                page.setting_with_icon(icon, name, subtitle, y, 65);
                page.toggle(100 + index, name, enabled, y, ROW_WITH_DETAIL);
            }
            let language_top = FIRST_CARD + pitch * 3 - CARD_GAP + SECTION_GAP;
            page.section("界面语言", language_top, language_top + ROW, &[]);
            let language = match prefs.language {
                Language::System => 0,
                Language::Chinese => 1,
                Language::English => 2,
            };
            page.buttons_after(
                523,
                language_top + 11,
                RIGHT,
                &[(104, "应用")],
                |apply_x| {
                    let language_x = apply_x - BUTTON_GAP - 146;
                    page.setting_with_icon(
                        tabler!("typography"),
                        "界面语言",
                        "",
                        language_top,
                        WIDTH - language_x,
                    );
                    combo(
                        hwnd,
                        103,
                        &["跟随系统", "简体中文", "English"],
                        language,
                        right(language_x, language_top + 11, 146, CONTROL_HEIGHT),
                    );
                },
            );
            unsafe { EnableWindow(GetDlgItem(hwnd, 104), 0) };
            page.finish_at(language_top + ROW + 30);
        }
        1 => super::appearance::build(state, hwnd, &prefs),
        2 if editing => {
            let engine = engine.unwrap_or_else(|| Engine {
                id: String::new(),
                name: String::new(),
                alias: String::new(),
                url: String::new(),
                color: 0x1f75db,
                icon: Icon::Default,
                mono_icon: Icon::Default,
                mono_color: None,
            });
            page.section("", 77, 515, &[258, 331, 404, 462]);
            page.field(20, "名称", &engine.name, 14, 88, 307, STRETCH);
            page.field(21, "别名（可选）", &engine.alias, 335, 88, 187, RIGHT);
            let url_field = page.field(22, "搜索网址", &engine.url, 14, 164, 508, STRETCH);
            cue(url_field, "https://example.com/search?q=%s");
            page.detail("使用 %s 代表要搜索的关键词", stretch(14, 225, 508, 22));
            for (label, pick, clear, y) in
                [("单色图标", 127, 118, 258), ("多色图标", 116, 1320, 331)]
            {
                page.body(label, fixed(66, y + 18, 145, 36));
                right_buttons(522, y + 20, &[(pick, "选择文件"), (clear, "清除")]);
            }
            page.body("单色配色", fixed(14, 415, 160, 36));
            let tint = engine.mono_color;
            let label = mono_color_label(tint);
            action(hwnd, MONO_COLOR, &label, mono_color_layout(&label, tint));
            right_buttons(522, 417, &[(RESET_MONO_COLOR, "恢复默认")]);
            let accent_end = page.buttons(14, 471, 0, &[(131, "选择强调色")]);
            action(
                hwnd,
                ACCENT_HELP,
                "?",
                fixed(accent_end + BUTTON_GAP, 471, CONTROL_HEIGHT, CONTROL_HEIGHT),
            );
            if !engine.id.is_empty() && !engine.is_custom() {
                right_buttons(522, 471, &[(119, "恢复内置配置")]);
            }
            model_field(hwnd, 23, &format!("{:06X}", engine.color));
            PANEL.with_borrow_mut(|p| p.view.engine_icon = Some(engine));
            update_icon_actions(hwnd);
            fluent::reset_images();
            set_draft(hwnd);
            let status = page.detail("", stretch(14, 520, 508, 25));
            unsafe { SetWindowLongPtrW(status, GWLP_ID, EDIT_STATUS as isize) };
            page.buttons(0, 552, 0, &[(125, "测试搜索")]);
            right_buttons(WIDTH, 552, &[(128, "取消"), (126, "保存")]);
            page.finish_at(606);
            update_engine_validation(state, hwnd);
        }
        2 => engines::display(state, hwnd),
        3 => {
            let selection_height = 145;
            let selection_top = FIRST_CARD + ROW_WITH_DETAIL + CARD_GAP;
            let clipboard_top = selection_top + selection_height + CARD_GAP;
            let image_top = clipboard_top + ROW_WITH_DETAIL + CARD_GAP;
            let ocr_top = image_top + 116 + CARD_GAP;
            page.section("全局快捷键", FIRST_CARD, FIRST_CARD + ROW_WITH_DETAIL, &[]);
            page.section("", selection_top, selection_top + selection_height, &[239]);
            page.section("", clipboard_top, clipboard_top + ROW_WITH_DETAIL, &[]);
            recorder::create(
                page,
                137,
                "唤出易搜",
                "有选区时直接搜索，其他时候打开搜索框。",
                prefs.main_shortcut,
                FIRST_CARD,
            );
            recorder::create(
                page,
                138,
                "划词搜索",
                "使用指定的引擎和触发动作。",
                prefs.selection.as_ref().map(|b| b.shortcut),
                selection_top,
            );
            let field_y = selection_top + 73;
            page.body("划词搜索引擎", place(14, field_y, 250, 21, LEFT_HALF));
            page.body("触发动作", place(272, field_y, 250, 21, RIGHT_HALF));
            engine_combo(
                state,
                hwnd,
                place(14, field_y + 24, 250, CONTROL_HEIGHT, LEFT_HALF),
            );
            combo(
                hwnd,
                135,
                &["直接搜索", "打开搜索框"],
                usize::from(prefs.selection.as_ref().is_none_or(|b| b.preview)),
                place(272, field_y + 24, 250, CONTROL_HEIGHT, RIGHT_HALF),
            );
            recorder::create(
                page,
                139,
                "剪贴板搜索",
                "使用默认引擎搜索剪贴板里的文字。",
                prefs.clipboard_shortcut,
                clipboard_top,
            );
            page.section("", image_top, image_top + 116, &[]);
            recorder::create(
                page,
                248,
                "截图搜图",
                "框选图片并搜索来源。",
                prefs.image_search_shortcut,
                image_top,
            );
            page.body(
                "框选后按 Enter",
                place(14, image_top + 76, 235, 24, LEFT_HALF),
            );
            combo(
                hwnd,
                249,
                &["选择搜图引擎", "同时搜图", "Yandex Images", "Google Lens"],
                prefs.image_search_action as usize,
                place(272, image_top + 72, 228, CONTROL_HEIGHT, RIGHT_HALF),
            );
            #[cfg(feature = "ocr")]
            {
                page.section("", ocr_top, ocr_top + ROW, &[]);
                let start = right_buttons(522, ocr_top + 11, &[(176, "前往设置")]);
                page.setting_with_icon(tabler!("scan"), "OCR 快捷键", "", ocr_top, WIDTH - start);
            }
            let last_card_bottom = if cfg!(feature = "ocr") {
                ocr_top + ROW
            } else {
                ocr_top - CARD_GAP
            };
            let excluded = last_card_bottom + SECTION_GAP;
            let values = PANEL
                .with_borrow(|panel| panel.exclusions.clone())
                .unwrap_or(prefs.excluded_apps);
            let chip_pitch = CONTROL_HEIGHT + CARD_GAP;
            let mut x = 14;
            let mut row = 0;
            for (index, app) in values.iter().enumerate() {
                let label = format!("{app} ×");
                let width = button_width(&label, Label::Control).min(375);
                if x + width > 389 {
                    x = 14;
                    row += 1;
                }
                action(
                    hwnd,
                    EXCLUSION_REMOVE + index,
                    &label,
                    fixed(x, excluded + 8 + row * chip_pitch, width, CONTROL_HEIGHT),
                );
                x += width + BUTTON_GAP;
            }
            let field_top = excluded + (row + 1) * chip_pitch + 20;
            page.section("双击 Alt 排除的应用", excluded, field_top + 49, &[]);
            if values.is_empty() {
                page.detail("没有排除应用", stretch(14, excluded + 10, 508, 24));
            }
            page.buttons_after(
                522,
                field_top,
                RIGHT,
                &[(EXCLUSION_ADD, "添加"), (134, "应用")],
                |start| {
                    let field = edit_at(
                        hwnd,
                        23,
                        "",
                        stretch(22, field_top, start - 45, CONTROL_HEIGHT),
                    );
                    cue(field, "例如 mstsc.exe");
                },
            );
            page.finish_at(field_top + 58);
        }
        4 => {
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
            let row_count = 1 + browsers.len() as i32 + i32::from(custom.is_some());
            let end = FIRST_CARD + row_count * BROWSER_ROW;
            page.section_title("打开搜索结果", FIRST_CARD - 31);
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
            let list = list_at(
                hwnd,
                rows,
                stretch(0, FIRST_CARD, WIDTH, row_count * BROWSER_ROW),
            );
            unsafe { SendMessageW(list, LB_SETCURSEL, applied.unwrap_or(0), 0) };
            if browser_panel == BrowserPanel::Path {
                page.section("", end + 7, end + 87, &[]);
                let value = custom.map(|p| p.display().to_string()).unwrap_or_default();
                page.buttons_after(522, end + 40, RIGHT, &[(143, "浏览")], |start| {
                    page.field(20, "浏览器程序", &value, 14, end + 14, start - 28, STRETCH);
                });
                right_buttons(
                    WIDTH,
                    end + 97,
                    &[(1440, "取消"), (1441, "测试打开"), (141, "应用")],
                );
                page.finish_at(end + 151);
            } else {
                action(
                    hwnd,
                    142,
                    "选择其他浏览器",
                    stretch(0, end, WIDTH, CONTROL_HEIGHT),
                );
                right_buttons(
                    WIDTH,
                    end + CONTROL_HEIGHT + 12,
                    &[(1441, "测试打开"), (140, "应用")],
                );
                unsafe { EnableWindow(GetDlgItem(hwnd, 140), 0) };
                page.finish_at(end + CONTROL_HEIGHT + 76);
            }
            PANEL.with_borrow_mut(|p| p.browsers = browsers);
        }
        5 => history::build(state, hwnd, prefs.record_history),
        6 => {
            let location = PANEL
                .with_borrow(|p| p.next_config.clone())
                .map(|path| format!("{} {}", tr("下次启动使用"), path.display()));
            let current_bottom = FIRST_CARD + 85 + if location.is_some() { 24 } else { 0 };
            let import_top = current_bottom + SECTION_GAP;
            let pack_top = import_top + ROW_WITH_DETAIL + CARD_GAP;
            page.section("当前配置", FIRST_CARD, current_bottom, &[]);
            page.buttons_after(
                522,
                FIRST_CARD + 36,
                RIGHT,
                &[(165, "更换位置")],
                |start| {
                    let path = page.field(
                        166,
                        "配置文件路径",
                        &state.config.display().to_string(),
                        14,
                        FIRST_CARD + 10,
                        start - 28,
                        STRETCH,
                    );
                    unsafe {
                        SendMessageW(path, windows_sys::Win32::UI::Controls::EM_SETREADONLY, 1, 0)
                    };
                },
            );
            if let Some(location) = location {
                page.detail(&location, stretch(14, FIRST_CARD + 76, 508, 24));
            }
            page.cards("导入与导出", import_top, &[ROW_WITH_DETAIL; 2]);
            let import_x = right_buttons(
                522,
                import_top + 14,
                &[(161, "导入配置"), (162, "导出配置")],
            );
            let pack_x = right_buttons(522, pack_top + 14, &[(163, "导入引擎"), (164, "导出引擎")]);
            page.setting_with_icon(
                tabler!("file-typography"),
                "完整配置",
                "导入将替换当前配置。",
                import_top,
                WIDTH - import_x - 6,
            );
            page.setting_with_icon(
                tabler!("search"),
                "搜索引擎包",
                "导入会合并自定义引擎与置顶顺序。",
                pack_top,
                WIDTH - pack_x - 6,
            );
            page.finish_at(pack_top + ROW_WITH_DETAIL + 30);
        }
        #[cfg(feature = "ocr")]
        OCR_PAGE => ocr::display(page, &prefs),
        _ => {}
    }
}

fn mono_color_label(tint: Option<u32>) -> String {
    tint.map_or_else(|| "选择颜色".into(), |color| format!("#{color:06X}"))
}

fn mono_color_layout(label: &str, tint: Option<u32>) -> Layout {
    let end = 522 - button_width("恢复默认", Label::Control) - BUTTON_GAP;
    let width =
        button_width(label, Label::Control) + if tint.is_some() { BUTTON_ICON_SPACE } else { 0 };
    place(end - width, 417, width, CONTROL_HEIGHT, RIGHT)
}

pub(super) fn update_icon_actions(hwnd: HWND) {
    let (mono, color, tint) = PANEL.with_borrow(|p| {
        p.view
            .engine_icon
            .as_ref()
            .map_or((false, false, None), |e| {
                (
                    !e.mono_icon.is_default(),
                    !e.icon.is_default(),
                    e.mono_color,
                )
            })
    });
    let label = mono_color_label(tint);
    let layout = mono_color_layout(&label, tint);
    let control = unsafe { GetDlgItem(hwnd, MONO_COLOR as i32) };
    let resized = PANEL.with_borrow_mut(|p| {
        if let Some((_, area)) = p
            .view
            .controls
            .iter_mut()
            .find(|(child, _)| *child == control)
        {
            if *area != layout {
                *area = layout;
                return true;
            }
        }
        false
    });
    set_field(hwnd, MONO_COLOR as i32, tr(&label));
    if resized {
        fluent::layout(hwnd);
    }
    unsafe {
        EnableWindow(GetDlgItem(hwnd, 118), i32::from(mono));
        EnableWindow(GetDlgItem(hwnd, 1320), i32::from(color));
        EnableWindow(
            GetDlgItem(hwnd, RESET_MONO_COLOR as i32),
            i32::from(tint.is_some()),
        );
    }
}

pub(super) const EDIT_STATUS: usize = 133;
pub(super) const MONO_COLOR: usize = 1321;
pub(super) const RESET_MONO_COLOR: usize = 1322;
