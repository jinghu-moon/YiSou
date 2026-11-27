use super::{
    components::{
        BUTTON_GAP, CARD_GAP, CONTROL_HEIGHT, FIRST_CARD, NumberStepper, Page, ROW,
        ROW_WITH_DETAIL, SETTING_TEXT_X, WIDTH, button_width,
    },
    *,
};
use crate::native::paint::Label;
use crate::native::search_window::{CONTENT_GAP, engine_button_width, search_row_height};
use yisou::settings::{
    HistoryMaterial, MAX_SEARCH_PADDING, MAX_SEARCH_WIDTH, MIN_SEARCH_WIDTH, ScreenBackground,
};

pub(super) const ENGINE_ICON_SIZE: usize = 178;
pub(super) const ICON_DECREASE: usize = 179;
pub(super) const ICON_INCREASE: usize = 180;
pub(super) const ICON_STEPPER: NumberStepper = NumberStepper {
    input: ENGINE_ICON_SIZE,
    decrease: ICON_DECREASE,
    increase: ICON_INCREASE,
    range: 1..=MAX_SEARCH_ENGINE_ICON_SIZE,
    unit: "DIP",
};
pub(super) const SEARCH_FONT_SIZE: usize = 216;
pub(super) const SEARCH_WIDTH: usize = 306;
const WIDTH_STEPPER: NumberStepper = NumberStepper {
    input: SEARCH_WIDTH,
    decrease: SEARCH_WIDTH + 1,
    increase: SEARCH_WIDTH + 2,
    range: MIN_SEARCH_WIDTH..=MAX_SEARCH_WIDTH,
    unit: "DIP",
};
pub(super) const RIGHT_ENGINE_NAME: usize = 263;
const FONT_STEPPER: NumberStepper = NumberStepper {
    input: SEARCH_FONT_SIZE,
    decrease: 217,
    increase: 218,
    range: yisou::settings::MIN_SEARCH_FONT_SIZE..=yisou::settings::MAX_SEARCH_FONT_SIZE,
    unit: "DIP",
};
pub(super) const SEARCH_MATERIAL: usize = 197;
pub(super) const SEARCH_TONE: usize = 198;
pub(super) const SCREEN_BACKGROUND: usize = 199;
pub(super) const BACKGROUND_BLUR_PERCENT: usize = 303;
const BACKGROUND_BLUR_STEPPER: NumberStepper = NumberStepper {
    input: BACKGROUND_BLUR_PERCENT,
    decrease: BACKGROUND_BLUR_PERCENT + 1,
    increase: BACKGROUND_BLUR_PERCENT + 2,
    range: 0..=100,
    unit: "%",
};
pub(super) const ACRYLIC_TINT: usize = 210;
pub(super) const MICA_TINT: usize = 213;
pub(super) const PADDING_TOP: usize = 270;
pub(super) const PADDING_BOTTOM: usize = 273;
pub(super) const PADDING_LEFT: usize = 276;
pub(super) const PADDING_RIGHT: usize = 279;
pub(super) const HISTORY_MATERIAL: usize = 282;
pub(super) const HISTORY_TONE: usize = 283;
pub(super) const HISTORY_ENABLED: usize = 284;
pub(super) const HISTORY_SHORTCUTS: usize = 285;
pub(super) const HISTORY_ACRYLIC_TINT: usize = 286;
pub(super) const HISTORY_MICA_TINT: usize = 289;
pub(super) const SEARCH_RADIUS: usize = 295;
pub(super) const HISTORY_RADIUS: usize = 298;
pub(super) const HISTORY_ENABLED_LABEL: &str = "启用搜索历史浮层";
pub(super) const HISTORY_SHORTCUTS_LABEL: &str = "显示底部快捷键提示";
const PADDING_STEPPERS: [NumberStepper; 4] = [
    padding_stepper(PADDING_TOP),
    padding_stepper(PADDING_BOTTOM),
    padding_stepper(PADDING_LEFT),
    padding_stepper(PADDING_RIGHT),
];

const fn padding_stepper(input: usize) -> NumberStepper {
    NumberStepper {
        input,
        decrease: input + 1,
        increase: input + 2,
        range: 0..=MAX_SEARCH_PADDING,
        unit: "DIP",
    }
}

pub(super) fn is_padding_input(id: usize) -> bool {
    PADDING_STEPPERS.iter().any(|stepper| stepper.input == id)
}

const TINT_STEPPERS: [NumberStepper; 2] = [
    NumberStepper {
        input: ACRYLIC_TINT,
        decrease: 211,
        increase: 212,
        range: 0..=100,
        unit: "%",
    },
    NumberStepper {
        input: MICA_TINT,
        decrease: 214,
        increase: 215,
        range: 0..=100,
        unit: "%",
    },
];

const HISTORY_TINT_STEPPERS: [NumberStepper; 2] = [
    history_tint_stepper(HISTORY_ACRYLIC_TINT),
    history_tint_stepper(HISTORY_MICA_TINT),
];
const RADIUS_STEPPERS: [NumberStepper; 2] = [
    radius_stepper(SEARCH_RADIUS),
    radius_stepper(HISTORY_RADIUS),
];

const fn radius_stepper(input: usize) -> NumberStepper {
    NumberStepper {
        input,
        decrease: input + 1,
        increase: input + 2,
        range: 0..=i32::MAX,
        unit: "DIP",
    }
}

const fn history_tint_stepper(input: usize) -> NumberStepper {
    NumberStepper {
        input,
        decrease: input + 1,
        increase: input + 2,
        range: 0..=100,
        unit: "%",
    }
}

pub(super) fn search_steppers() -> impl Iterator<Item = NumberStepper> {
    [
        ICON_STEPPER,
        FONT_STEPPER,
        WIDTH_STEPPER,
        BACKGROUND_BLUR_STEPPER,
    ]
    .into_iter()
    .chain(TINT_STEPPERS)
    .chain(HISTORY_TINT_STEPPERS)
    .chain(RADIUS_STEPPERS)
    .chain(PADDING_STEPPERS)
}

pub(super) const MATERIALS: [SearchMaterial; 3] = [
    SearchMaterial::Acrylic,
    SearchMaterial::Mica,
    SearchMaterial::Solid,
];
pub(super) const HISTORY_MATERIALS: [HistoryMaterial; 4] = [
    HistoryMaterial::Acrylic,
    HistoryMaterial::Mica,
    HistoryMaterial::Solid,
    HistoryMaterial::FollowSearch,
];
pub(super) const SCREEN_BACKGROUNDS: [ScreenBackground; 3] = [
    ScreenBackground::Dim,
    ScreenBackground::System,
    ScreenBackground::Gaussian,
];
pub(super) const TONES: [SearchTone; 3] = [SearchTone::System, SearchTone::Light, SearchTone::Dark];
pub(super) const SEARCH_PREVIEW_TOP: i32 = FIRST_CARD;
pub(super) const MENU_PREVIEW_WIDTH: i32 = 232;
const ROW_PITCH: i32 = ROW + CARD_GAP;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Background,
    Text,
    Engines,
    History,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Background, Self::Text, Self::Engines, Self::History];

    pub(super) fn from_id(id: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.id() == id)
    }

    pub(super) fn id(self) -> usize {
        match self {
            Self::History => 264,
            _ => 260 + self as usize,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Background => "搜索框背景",
            Self::Text => "搜索框文字",
            Self::Engines => "引擎图标与菜单",
            Self::History => "搜索历史浮层",
        }
    }

    pub(super) fn navigate(self, key: u16) -> Option<Self> {
        let index = self as usize;
        let count = Self::ALL.len();
        let next = match key {
            VK_LEFT => (index + count - 1) % count,
            VK_RIGHT => (index + 1) % count,
            VK_HOME => 0,
            VK_END => count - 1,
            _ => return None,
        };
        Some(Self::ALL[next])
    }
}

pub(super) fn search_preview_height(prefs: &Preferences) -> i32 {
    prefs.search_padding.top
        + search_row_height(prefs.search_font_size)
        + prefs.search_padding.bottom
}

fn tabs_top(preview_height: i32) -> i32 {
    SEARCH_PREVIEW_TOP + preview_height + 12
}

fn content_top(preview_height: i32) -> i32 {
    tabs_top(preview_height) + CONTROL_HEIGHT + 10
}

pub(super) fn menu_options_top(preview_height: i32) -> i32 {
    content_top(preview_height) + 4 * ROW_PITCH
}

pub(super) fn preview_caption(state: &State, prefs: &Preferences) -> Layout {
    let app = state.app.borrow();
    let name = app
        .engines()
        .iter()
        .find(|engine| engine.id == prefs.default_engine)
        .map(|engine| engine.name.as_str());
    let padding = prefs.search_padding;
    let reserved = padding.left
        + padding.right
        + engine_button_width(prefs, name, prefs.search_width)
        + CONTENT_GAP;
    place(
        padding.left,
        SEARCH_PREVIEW_TOP + padding.top,
        WIDTH - reserved,
        search_row_height(prefs.search_font_size),
        STRETCH,
    )
    .font(fluent::FontRole::Preview)
    .limit_width(prefs.search_width - reserved)
}

pub(super) fn custom_search_font(prefs: &Preferences) -> bool {
    prefs.font != Font::Default
        || prefs.search_font_size != yisou::settings::DEFAULT_SEARCH_FONT_SIZE
        || prefs.search_font_face != Default::default()
        || prefs.font_preview_script != Default::default()
}

macro_rules! tabler {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../../../assets/icons/tabler/", $name, ".svg")) as &[u8],
        )
    };
}

pub(super) fn build(state: &State, hwnd: HWND, prefs: &Preferences) {
    let page = Page(hwnd);
    let height = search_preview_height(prefs);
    page.section_title("搜索框", SEARCH_PREVIEW_TOP - 31);
    let preview = caption(
        hwnd,
        "复杂中文 鬱鑿齒 · Search 123",
        preview_caption(state, prefs),
        fluent::FontRole::Preview,
    );
    unsafe { SetWindowLongPtrW(preview, GWLP_ID, 115) };
    let font = state.create_ui_font(unsafe { GetDpiForWindow(hwnd) }.max(96));
    let tab = PANEL.with_borrow_mut(|p| {
        p.preview_font = font;
        p.appearance_tab
    });

    let y = tabs_top(height);
    let mut x = 0;
    for tab in Tab::ALL {
        let label = tab.label();
        let width = button_width(label, Label::Control);
        action(hwnd, tab.id(), label, place(x, y, width, CONTROL_HEIGHT, 0));
        x += width;
    }
    update_tab_buttons(hwnd, tab);
    build_content(state, page, prefs, tab, content_top(height));
}

fn update_tab_buttons(hwnd: HWND, active: Tab) {
    for tab in Tab::ALL {
        unsafe {
            let button = GetDlgItem(hwnd, tab.id() as i32);
            let style = GetWindowLongPtrW(button, GWL_STYLE) & !(WS_TABSTOP as isize);
            SetWindowLongPtrW(
                button,
                GWL_STYLE,
                style
                    | if tab == active {
                        WS_TABSTOP as isize
                    } else {
                        0
                    },
            );
            InvalidateRect(button, null(), 0);
        }
    }
}

pub(super) fn select_tab(state: &State, hwnd: HWND, tab: Tab) {
    if PANEL.with_borrow(|p| p.page != 1 || p.font_picker.is_some() || p.appearance_tab == tab) {
        return;
    }
    focus_control(hwnd, tab.id() as i32);
    let previous = PANEL.with_borrow_mut(|p| std::mem::replace(&mut p.appearance_tab, tab));
    unsafe {
        fluent::animate_selector(
            GetDlgItem(hwnd, previous.id() as i32),
            GetDlgItem(hwnd, tab.id() as i32),
        );
    }
    let prefs = state.app.borrow().settings().preferences.clone();
    let top = content_top(search_preview_height(&prefs));
    replace_rows(hwnd, top, |page| {
        update_tab_buttons(hwnd, tab);
        build_content(state, page, &prefs, tab, top);
    });
}

pub(super) fn refresh_material(state: &State, hwnd: HWND) {
    let prefs = state.app.borrow().settings().preferences.clone();
    let top = content_top(search_preview_height(&prefs)) + 3 * ROW_PITCH;
    replace_rows(hwnd, top, |page| {
        build_background_options(page, &prefs, top)
    });
}

pub(super) fn refresh_screen_background(state: &State, hwnd: HWND) {
    let prefs = state.app.borrow().settings().preferences.clone();
    let control = unsafe { GetDlgItem(hwnd, SCREEN_BACKGROUND as i32) };
    let Some(top) = fluent::with_view(hwnd, |view| {
        view.controls
            .iter()
            .find(|(child, _)| *child == control)
            .map(|(_, area)| area.y + (ROW_WITH_DETAIL + CONTROL_HEIGHT + 1) / 2 + CARD_GAP)
    }) else {
        return;
    };
    replace_rows(hwnd, top, |page| build_blur_and_padding(page, &prefs, top));
}

pub(super) fn refresh_history_material(state: &State, hwnd: HWND) {
    let prefs = state.app.borrow().settings().preferences.clone();
    let top = content_top(search_preview_height(&prefs)) + 3 * ROW_PITCH;
    replace_rows(hwnd, top, |page| build_history_colors(page, &prefs, top));
}

fn replace_rows(hwnd: HWND, top: i32, build: impl FnOnce(Page)) {
    unsafe { SendMessageW(hwnd, WM_SETREDRAW, 0, 0) };
    let removed = PANEL.with_borrow_mut(|p| {
        let mut removed = Vec::new();
        p.view.controls.retain(|(child, area)| {
            if area.y >= top {
                removed.push(*child);
                false
            } else {
                true
            }
        });
        p.view.surfaces.retain(|area| area.top < top);
        p.view.row_icons.retain(|(_, _, y)| *y < top);
        p.children.retain(|child| !removed.contains(child));
        removed
    });
    for child in removed {
        unsafe { DestroyWindow(child) };
    }
    build(Page(hwnd));
    fluent::layout(hwnd);
    unsafe {
        SendMessageW(hwnd, WM_SETREDRAW, 1, 0);
        RedrawWindow(
            hwnd,
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
        );
    }
}

fn build_content(state: &State, page: Page, prefs: &Preferences, tab: Tab, top: i32) {
    match tab {
        Tab::Background => {
            stepper_card(
                page,
                &RADIUS_STEPPERS[0],
                "搜索框圆角",
                prefs.search_corner_radius,
                top,
                ["减小圆角", "增大圆角"],
            );
            stepper_card(
                page,
                &WIDTH_STEPPER,
                "搜索框宽度",
                prefs.search_width,
                top + ROW_PITCH,
                ["减小搜索框宽度", "增大搜索框宽度"],
            );
            choice_card(
                page,
                SEARCH_MATERIAL,
                "搜索框材质",
                &["亚克力", "云母", "纯色"],
                MATERIALS
                    .iter()
                    .position(|&material| material == prefs.search_material),
                top + 2 * ROW_PITCH,
            );
            build_background_options(page, prefs, top + 3 * ROW_PITCH);
        }
        Tab::Text => build_text(state, page, prefs, top),
        Tab::Engines => build_engines(page, prefs, top),
        Tab::History => build_history(page, prefs, top),
    }
}

fn build_history(page: Page, prefs: &Preferences, top: i32) {
    let history = &prefs.history_popup;
    for (index, (id, title, enabled)) in [
        (HISTORY_ENABLED, HISTORY_ENABLED_LABEL, history.enabled),
        (
            HISTORY_SHORTCUTS,
            HISTORY_SHORTCUTS_LABEL,
            history.show_shortcuts,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = top + index as i32 * ROW_PITCH;
        page.section("", y, y + ROW, &[]);
        page.setting_with_icon(tabler!("eye"), title, "", y, 87);
        page.toggle(id, title, enabled, y, ROW);
    }
    choice_card(
        page,
        HISTORY_MATERIAL,
        "浮层背景",
        &["亚克力", "云母", "纯色", "跟随搜索框"],
        HISTORY_MATERIALS
            .iter()
            .position(|&material| material == history.material),
        top + 2 * ROW_PITCH,
    );
    build_history_colors(page, prefs, top + 3 * ROW_PITCH);
}

fn build_history_colors(page: Page, prefs: &Preferences, mut top: i32) {
    let history = &prefs.history_popup;
    if history.material != HistoryMaterial::FollowSearch {
        top = build_independent_history_background(page, prefs, top);
    }
    color_card(
        page,
        color_picker::HISTORY_HIGHLIGHT,
        "匹配文本高亮背景色",
        top,
    );
    top += ROW_PITCH;
    stepper_card(
        page,
        &RADIUS_STEPPERS[1],
        "浮层圆角",
        history.corner_radius,
        top,
        ["减小圆角", "增大圆角"],
    );
    page.finish_at(top + ROW + 30);
}

fn build_independent_history_background(page: Page, prefs: &Preferences, mut top: i32) -> i32 {
    let history = &prefs.history_popup;
    let background = prefs.history_background(false);
    let transparency = match background.material {
        SearchMaterial::Acrylic => Some((&HISTORY_TINT_STEPPERS[0], "亚克力透明度")),
        SearchMaterial::Mica => Some((&HISTORY_TINT_STEPPERS[1], "云母透明度")),
        SearchMaterial::Solid => None,
    };
    if let Some((stepper, title)) = transparency {
        stepper_card(
            page,
            stepper,
            title,
            100 - background.tint_percent,
            top,
            ["降低透明度", "提高透明度"],
        );
        top += ROW_PITCH;
    }
    choice_card(
        page,
        HISTORY_TONE,
        "浮层色调",
        &["跟随系统", "浅色", "深色"],
        TONES.iter().position(|&tone| tone == history.tone),
        top,
    );
    top += ROW_PITCH;
    background_color_card(
        page,
        [color_picker::HISTORY_LIGHT, color_picker::HISTORY_DARK],
        top,
    );
    top + ROW_PITCH
}

fn choice_card(
    page: Page,
    id: usize,
    title: &str,
    labels: &[&str],
    selected: Option<usize>,
    top: i32,
) {
    page.section("", top, top + ROW, &[]);
    page.setting_with_icon(tabler!("palette"), title, "", top, 172);
    combo(
        page.0,
        id,
        labels,
        selected.unwrap_or_default(),
        place(
            364,
            top + (ROW - CONTROL_HEIGHT) / 2,
            159,
            CONTROL_HEIGHT,
            RIGHT,
        ),
    );
}

fn build_background_options(page: Page, prefs: &Preferences, mut top: i32) {
    let transparency = match prefs.search_material {
        SearchMaterial::Acrylic => Some((
            &TINT_STEPPERS[0],
            "亚克力透明度",
            100 - prefs.acrylic_tint_percent,
        )),
        SearchMaterial::Mica => Some((
            &TINT_STEPPERS[1],
            "云母透明度",
            100 - prefs.mica_tint_percent,
        )),
        SearchMaterial::Solid => None,
    };
    if let Some((stepper, title, value)) = transparency {
        stepper_card(
            page,
            stepper,
            title,
            value,
            top,
            ["降低透明度", "提高透明度"],
        );
        top += ROW_PITCH;
    }
    choice_card(
        page,
        SEARCH_TONE,
        "搜索框色调",
        &["跟随系统", "浅色", "深色"],
        TONES.iter().position(|&tone| tone == prefs.search_tone),
        top,
    );
    top += ROW_PITCH;

    background_color_card(page, [color_picker::LIGHT, color_picker::DARK], top);
    top += ROW_PITCH;
    build_screen_and_padding(page, prefs, top);
}

fn background_color_card(page: Page, [light, dark]: [usize; 2], top: i32) {
    const COLOR_BUTTON_WIDTH: i32 = 146;
    const RIGHT_EDGE: i32 = 523;
    let dark_x = RIGHT_EDGE - COLOR_BUTTON_WIDTH;
    let light_x = dark_x - BUTTON_GAP - COLOR_BUTTON_WIDTH;
    page.section("", top, top + ROW, &[]);
    page.setting_with_icon(tabler!("palette"), "背景配色", "", top, WIDTH - light_x + 8);
    for (id, title, x) in [
        (light, "浅色模式颜色", light_x),
        (dark, "深色模式颜色", dark_x),
    ] {
        action(
            page.0,
            id,
            title,
            place(
                x,
                top + (ROW - CONTROL_HEIGHT) / 2,
                COLOR_BUTTON_WIDTH,
                CONTROL_HEIGHT,
                RIGHT,
            ),
        );
    }
}

fn build_screen_and_padding(page: Page, prefs: &Preferences, top: i32) {
    page.section("", top, top + ROW_WITH_DETAIL, &[]);
    page.setting_with_icon(
        tabler!("eye"),
        "全屏背景效果",
        "应用于搜索框以外的屏幕区域",
        top,
        172,
    );
    combo(
        page.0,
        SCREEN_BACKGROUND,
        &["纯暗遮罩", "系统模糊", "高斯模糊"],
        SCREEN_BACKGROUNDS
            .iter()
            .position(|&mode| mode == prefs.screen_background)
            .unwrap_or_default(),
        place(
            364,
            top + (ROW_WITH_DETAIL - CONTROL_HEIGHT) / 2,
            159,
            CONTROL_HEIGHT,
            RIGHT,
        ),
    );
    build_blur_and_padding(page, prefs, top + ROW_WITH_DETAIL + CARD_GAP);
}

fn build_blur_and_padding(page: Page, prefs: &Preferences, mut top: i32) {
    if prefs.screen_background == ScreenBackground::Gaussian {
        stepper_card(
            page,
            &BACKGROUND_BLUR_STEPPER,
            "背景模糊强度",
            prefs.background_blur_percent,
            top,
            ["降低模糊强度", "提高模糊强度"],
        );
        top += ROW_PITCH;
    }
    let bottom = top + 124;
    page.section("", top, bottom, &[]);
    page.setting_with_icon(tabler!("settings"), "搜索框内边距", "", top, 0);
    const LABEL_GAP: i32 = 16;
    let p = prefs.search_padding;
    let sides = [
        (
            &PADDING_STEPPERS[0],
            "上",
            p.top,
            ["减小上内边距", "增大上内边距"],
        ),
        (
            &PADDING_STEPPERS[1],
            "下",
            p.bottom,
            ["减小下内边距", "增大下内边距"],
        ),
        (
            &PADDING_STEPPERS[2],
            "左",
            p.left,
            ["减小左内边距", "增大左内边距"],
        ),
        (
            &PADDING_STEPPERS[3],
            "右",
            p.right,
            ["减小右内边距", "增大右内边距"],
        ),
    ];
    let label_widths = [0, 1].map(|column| {
        fluent::text_width(tr(sides[column].1), Label::Body)
            .max(fluent::text_width(tr(sides[column + 2].1), Label::Body))
            + 4
    });
    for (index, (stepper, label, value, labels)) in sides.into_iter().enumerate() {
        let y = top + 45 + 38 * (index / 2) as i32;
        let label_width = label_widths[index % 2];
        let (label_x, sizing) = if index % 2 == 0 {
            (SETTING_TEXT_X, 0)
        } else {
            (381 - LABEL_GAP - label_width, RIGHT)
        };
        page.body(
            label,
            place(label_x, y, label_width, CONTROL_HEIGHT, sizing),
        );
        stepper.create(
            page.0,
            value,
            label_x + label_width + LABEL_GAP,
            y,
            sizing,
            labels,
        );
    }
    page.finish_at(bottom + 30);
}

fn build_text(state: &State, page: Page, prefs: &Preferences, top: i32) {
    let current = if let Font::File(path) = &prefs.font {
        path.display().to_string()
    } else {
        state.font_family()
    };
    page.section("", top, top + ROW_WITH_DETAIL, &[]);
    let font_x = page.buttons(
        523,
        top + 14,
        RIGHT,
        &[
            (111, "选择系统字体"),
            (113, "打开字体文件"),
            (112, "恢复默认"),
        ],
    );
    page.setting_with_icon(
        tabler!("typography"),
        "当前字体",
        &current,
        top,
        WIDTH - font_x - 6,
    );
    unsafe {
        EnableWindow(
            GetDlgItem(page.0, 112),
            i32::from(custom_search_font(prefs)),
        )
    };
    let top = top + ROW_WITH_DETAIL + CARD_GAP;
    stepper_card(
        page,
        &FONT_STEPPER,
        "输入字号",
        prefs.search_font_size,
        top,
        ["减小字号", "增大字号"],
    );
    let top = top + ROW_PITCH;
    color_card(page, color_picker::TEXT_COLOR, "文字颜色", top);
    page.finish_at(top + ROW + 30);
}

fn build_engines(page: Page, prefs: &Preferences, top: i32) {
    page.section("", top, top + ROW, &[]);
    page.setting_with_icon(tabler!("palette"), "使用多色图标", "", top, 87);
    page.toggle(117, "使用多色图标", prefs.multicolor_engine_icons, top, ROW);
    color_card(
        page,
        color_picker::ICON_COLOR,
        "右侧单色图标颜色",
        top + ROW_PITCH,
    );
    color_picker::sync_icon_availability(page.0, prefs.multicolor_engine_icons);
    stepper_card(
        page,
        &ICON_STEPPER,
        "右侧图标大小",
        prefs.search_engine_icon_size,
        top + 2 * ROW_PITCH,
        ["减小图标", "增大图标"],
    );

    let top = top + 3 * ROW_PITCH;
    page.section("", top, top + ROW, &[]);
    page.setting_with_icon(tabler!("typography"), "右侧图标显示名称", "", top, 87);
    page.toggle(
        RIGHT_ENGINE_NAME,
        "右侧图标显示名称",
        prefs.show_search_engine_name,
        top,
        ROW,
    );

    let top = top + ROW_PITCH;
    let height = ROW * 2 + CARD_GAP;
    let x = MENU_PREVIEW_WIDTH + CARD_GAP;
    page.section("", top, top + height, &[]);
    page.body(
        "菜单显示名称",
        place(x + 14, top + (height - 23) / 2, WIDTH - x - 89, 23, STRETCH),
    );
    page.toggle(110, "菜单显示名称", prefs.show_engine_names, top, height);
    page.finish_at(top + height + 30);
}

fn stepper_card(
    page: Page,
    stepper: &NumberStepper,
    title: &str,
    value: i32,
    top: i32,
    labels: [&str; 2],
) {
    page.section("", top, top + ROW, &[]);
    let icon = match stepper.input {
        SEARCH_FONT_SIZE => tabler!("typography"),
        SEARCH_WIDTH => tabler!("settings"),
        _ => tabler!("palette"),
    };
    page.setting_with_icon(icon, title, "", top, 155);
    stepper.create(
        page.0,
        value,
        381,
        top + (ROW - CONTROL_HEIGHT) / 2,
        RIGHT,
        labels,
    );
}

fn color_card(page: Page, id: usize, title: &str, top: i32) {
    page.section("", top, top + ROW, &[]);
    page.setting_with_icon(tabler!("palette"), title, "", top, 156);
    action(
        page.0,
        id,
        title,
        place(
            387,
            top + (ROW - CONTROL_HEIGHT) / 2,
            136,
            CONTROL_HEIGHT,
            RIGHT,
        ),
    );
}
