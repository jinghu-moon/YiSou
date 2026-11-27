use super::{
    components::{CONTROL_HEIGHT, Page, WIDTH},
    fluent::{self, RIGHT, STRETCH},
    *,
};
use crate::native::paint::{Label, Painter, color, rect};
use std::cell::Cell;
use windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP;
use windows_sys::Win32::UI::Controls::ODS_FOCUS;

const CLASS: *const u16 = w!("YiSou.Rust.Settings.ColorPicker");
const GRAPH_CLASS: *const u16 = w!("YiSou.Rust.Settings.ColorPicker.Graph");
const STYLE: u32 = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
const EX_STYLE: u32 = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT | WS_EX_COMPOSITED;
pub(super) const INSET: i32 = 20;
const GRAPH_TOP: i32 = 32;
pub(super) const LIGHT: usize = 230;
pub(super) const DARK: usize = 231;
pub(super) const TEXT_COLOR: usize = 258;
pub(super) const ICON_COLOR: usize = 259;
pub(super) const HISTORY_LIGHT: usize = 292;
pub(super) const HISTORY_DARK: usize = 293;
pub(super) const HISTORY_HIGHLIGHT: usize = 294;
const FORMAT: usize = 232;
const VALUE: usize = 233;
const COPY: usize = 235;
const CANCEL: usize = 236;
const APPLY: usize = 237;
const RESET: usize = 238;
const GRAPH: usize = 239;
const FIRST_PRESET: usize = 240;
const FIRST_RECENT: usize = 250;
const PRESETS: [u32; 10] = [
    0xff3b30, 0xff9500, 0xffcc00, 0x34c759, 0x55c5ed, 0x007aff, 0x5856d6, 0xff2d55, 0xffffff,
    0x000000,
];

#[derive(Clone, Copy)]
enum Format {
    Hex,
    Rgb,
    Hsl,
}

#[derive(Clone, Copy)]
enum Drag {
    Plane,
    Hue,
    Transparency,
}

#[derive(Clone, Copy)]
struct Hsv {
    hue: f32,
    saturation: f32,
    value: f32,
}

impl Hsv {
    fn from_rgb(rgb: u32, hue: f32) -> Self {
        let [r, g, b] = [16, 8, 0].map(|shift| ((rgb >> shift) & 255) as f32 / 255.0);
        let value = r.max(g).max(b);
        let delta = value - r.min(g).min(b);
        let hue = if delta == 0.0 {
            hue
        } else {
            let sector = if value == r {
                (g - b) / delta
            } else if value == g {
                (b - r) / delta + 2.0
            } else {
                (r - g) / delta + 4.0
            };
            (sector / 6.0).rem_euclid(1.0)
        };
        Self {
            hue,
            saturation: if value > 0.0 { delta / value } else { 0.0 },
            value,
        }
    }

    fn rgb(self) -> u32 {
        let channel = |offset: f32| {
            let k = (self.hue * 6.0 + offset).rem_euclid(6.0);
            let ratio = 1.0 - self.saturation * k.min(4.0 - k).clamp(0.0, 1.0);
            (ratio * self.value * 255.0).round() as u32
        };
        channel(5.0) << 16 | channel(3.0) << 8 | channel(1.0)
    }

    fn from_hsl(hue: f32, saturation: f32, lightness: f32) -> Self {
        let value = lightness + saturation * lightness.min(1.0 - lightness);
        Self {
            hue,
            saturation: if value > 0.0 {
                2.0 * (1.0 - lightness / value)
            } else {
                0.0
            },
            value,
        }
    }
}

impl Format {
    fn display(self, hsv: Hsv) -> String {
        let rgb = hsv.rgb();
        match self {
            Self::Hex => format!("#{rgb:06X}"),
            Self::Rgb => format!("{}, {}, {}", rgb >> 16, (rgb >> 8) & 255, rgb & 255),
            Self::Hsl => {
                let lightness = hsv.value * (1.0 - hsv.saturation / 2.0);
                let saturation = if (0.0..1.0).contains(&lightness) && lightness > 0.0 {
                    (hsv.value - lightness) / lightness.min(1.0 - lightness)
                } else {
                    0.0
                };
                format!(
                    "{:.1}, {:.1}%, {:.1}%",
                    hsv.hue * 360.0,
                    saturation * 100.0,
                    lightness * 100.0
                )
            }
        }
    }

    fn parse(self, text: &str, hue: f32) -> Option<Hsv> {
        if matches!(self, Self::Hex) {
            let hex = text.trim().trim_start_matches('#');
            return (hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| Hsv::from_rgb(u32::from_str_radix(hex, 16).unwrap(), hue));
        }
        let mut parts = text.split(',');
        let [a, b, c] = [(); 3].map(|_| parts.next().map(str::trim));
        if parts.next().is_some() {
            return None;
        }
        match self {
            Self::Rgb => {
                let [r, g, b] = [a, b, c].map(|part| part?.parse::<u8>().ok());
                let rgb = u32::from(r?) << 16 | u32::from(g?) << 8 | u32::from(b?);
                Some(Hsv::from_rgb(rgb, hue))
            }
            Self::Hsl => {
                let number = |part: Option<&str>, max| {
                    let value = part?.trim_end_matches('%').parse::<f32>().ok()?;
                    (0.0..=max).contains(&value).then_some(value / max)
                };
                Some(Hsv::from_hsl(
                    number(a, 360.0)?,
                    number(b, 100.0)?,
                    number(c, 100.0)?,
                ))
            }
            Self::Hex => unreachable!(),
        }
    }
}

#[derive(Clone, Copy)]
enum Target {
    Background {
        dark: bool,
        material: SearchMaterial,
        history: bool,
    },
    Text,
    Icon,
    HistoryHighlight,
}

impl Target {
    fn title(self) -> &'static str {
        match self {
            Self::Background { dark: false, .. } => "浅色模式颜色",
            Self::Background { dark: true, .. } => "深色模式颜色",
            Self::Text => "搜索框文字颜色",
            Self::Icon => "搜索框单色图标颜色",
            Self::HistoryHighlight => "匹配文本高亮背景色",
        }
    }

    fn id(self) -> usize {
        match self {
            Self::Background {
                dark: false,
                history: false,
                ..
            } => LIGHT,
            Self::Background {
                dark: true,
                history: false,
                ..
            } => DARK,
            Self::Background {
                dark: false,
                history: true,
                ..
            } => HISTORY_LIGHT,
            Self::Background {
                dark: true,
                history: true,
                ..
            } => HISTORY_DARK,
            Self::Text => TEXT_COLOR,
            Self::Icon => ICON_COLOR,
            Self::HistoryHighlight => HISTORY_HIGHLIGHT,
        }
    }

    fn color(self, prefs: &Preferences) -> Option<u32> {
        match self {
            Self::Background { dark, history, .. } => Some(if history {
                prefs.history_popup.tint(dark)
            } else {
                prefs.search_tint(dark)
            }),
            Self::Text => prefs.search_text_color,
            Self::Icon => prefs.search_mono_icon_color,
            Self::HistoryHighlight => prefs.history_popup.highlight_color,
        }
    }

    fn default_color(self, state: &State, prefs: &Preferences) -> u32 {
        match self {
            Self::Background {
                dark,
                history: true,
                ..
            } => yisou::settings::HistoryPopup::default().tint(dark),
            Self::Background { dark, .. } => {
                if dark {
                    0
                } else {
                    0xffffff
                }
            }
            Self::Text => state.resolve_search_colors(prefs, true).text,
            Self::HistoryHighlight => yisou::settings::DEFAULT_HISTORY_HIGHLIGHT,
            Self::Icon => {
                let app = state.app.borrow();
                let engine = app
                    .engines()
                    .iter()
                    .find(|engine| engine.id == prefs.default_engine);
                let colors = state.resolve_search_colors(prefs, true);
                engine
                    .and_then(|engine| engine.mono_color)
                    .unwrap_or(colors.text)
            }
        }
    }

    fn has_transparency(self) -> bool {
        !matches!(
            self,
            Self::Background {
                material: SearchMaterial::Solid,
                ..
            }
        )
    }

    fn transparency(self, prefs: &Preferences) -> i32 {
        match self {
            Self::Background {
                history, material, ..
            } => {
                100 - if history {
                    prefs.history_popup.tint_percent(material)
                } else {
                    prefs.search_tint_percent()
                }
            }
            Self::Text => prefs.search_text_transparency,
            Self::Icon => prefs.search_mono_icon_transparency,
            Self::HistoryHighlight => prefs.history_popup.highlight_transparency,
        }
    }
}

struct Picker {
    target: Target,
    hsv: Hsv,
    transparency: i32,
    automatic: bool,
    default_color: u32,
    format: Format,
    drag: Option<Drag>,
    recents: Vec<u32>,
}

thread_local! {
    static WINDOW: Cell<HWND> = const { Cell::new(null_mut()) };
    static PICKER: RefCell<Option<Picker>> = const { RefCell::new(None) };
}

pub(super) fn window() -> HWND {
    WINDOW.get()
}

impl Picker {
    fn new(state: &State, prefs: &Preferences, target: Target) -> Self {
        let rgb = target.color(prefs);
        let default_color = target.default_color(state, prefs);
        Self {
            target,
            hsv: Hsv::from_rgb(rgb.unwrap_or(default_color), 0.0),
            transparency: target.transparency(prefs),
            automatic: rgb.is_none(),
            default_color,
            format: Format::Hex,
            drag: None,
            recents: prefs.recent_search_colors.clone(),
        }
    }

    fn value_text(&self) -> String {
        let color = self.format.display(self.hsv);
        if self.target.has_transparency() {
            format!("{color} / {}%", self.transparency)
        } else {
            color
        }
    }

    fn read_value(&mut self, text: &str) -> Result<(), &'static str> {
        let mut color = text;
        let mut transparency = self.transparency;
        if self.target.has_transparency()
            && let Some((rgb, percent)) = text.split_once('/')
        {
            color = rgb;
            transparency = percent
                .trim()
                .trim_end_matches('%')
                .trim_end()
                .parse::<i32>()
                .map_err(|_| "请输入有效的透明度")?
                .clamp(0, 100);
        }
        let color = color.trim();
        if color != self.format.display(self.hsv) {
            self.hsv = self
                .format
                .parse(color, self.hsv.hue)
                .ok_or("请输入有效的颜色值")?;
            self.automatic = false;
        }
        self.transparency = transparency;
        Ok(())
    }
}

pub(super) fn open(state: &State, owner: HWND, id: usize) -> Result<(), String> {
    if !window().is_null() {
        unsafe { SetForegroundWindow(window()) };
        return Ok(());
    }
    let app = state.app.borrow();
    let prefs = &app.settings().preferences;
    let target = match id {
        LIGHT | DARK => Target::Background {
            dark: id == DARK,
            material: prefs.search_material,
            history: false,
        },
        HISTORY_LIGHT | HISTORY_DARK => Target::Background {
            dark: id == HISTORY_DARK,
            material: prefs.history_background(state.system_dark.get()).material,
            history: true,
        },
        HISTORY_HIGHLIGHT => Target::HistoryHighlight,
        TEXT_COLOR => Target::Text,
        ICON_COLOR if !prefs.multicolor_engine_icons => Target::Icon,
        _ => return Ok(()),
    };
    let picker = Picker::new(state, prefs, target);
    drop(app);
    let height = preset_top(target, !picker.recents.is_empty()) + 98;
    let hwnd = unsafe {
        let mut anchor = RECT::default();
        GetWindowRect(owner, &mut anchor);
        let dpi = GetDpiForWindow(owner).max(96);
        let bounds = window_bounds(anchor, dpi, height, true);
        CreateWindowExW(
            EX_STYLE,
            CLASS,
            wide(tr(target.title())).as_ptr(),
            STYLE,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            owner,
            null_mut(),
            instance(),
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(io::Error::last_os_error().to_string());
    }
    WINDOW.set(hwnd);
    PICKER.with_borrow_mut(|current| *current = Some(picker));
    state.dwm(hwnd, 2);
    fluent::apply_palette(hwnd);
    scrollbar::create(hwnd);
    display(hwnd);
    fluent::layout(hwnd);
    unsafe {
        EnableWindow(owner, 0);
        ShowWindow(hwnd, SW_SHOW);
    }
    focus_control(hwnd, VALUE as i32);
    Ok(())
}

fn window_bounds(anchor: RECT, dpi: u32, height: i32, center: bool) -> RECT {
    let mut frame = RECT {
        right: dip(WIDTH + 2 * INSET, dpi),
        bottom: dip(height, dpi),
        ..Default::default()
    };
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        AdjustWindowRectExForDpi(&mut frame, STYLE, 0, EX_STYLE, dpi);
        GetMonitorInfoW(
            MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        );
    }
    let work = monitor.rcWork;
    let width = (frame.right - frame.left).min(work.right - work.left);
    let height = (frame.bottom - frame.top).min(work.bottom - work.top);
    let (x, y) = if center {
        (
            (anchor.left + anchor.right - width) / 2,
            (anchor.top + anchor.bottom - height) / 2,
        )
    } else {
        (anchor.left, anchor.top)
    };
    let left = x.clamp(work.left, work.right - width);
    let top = y.clamp(work.top, work.bottom - height);
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

pub(super) fn capture_key(msg: &MSG) -> bool {
    let hwnd = window();
    if hwnd.is_null() || msg.hwnd != hwnd && unsafe { IsChild(hwnd, msg.hwnd) } == 0 {
        return false;
    }
    unsafe {
        if msg.message == WM_KEYDOWN {
            match msg.wParam as u16 {
                VK_ESCAPE if !dropdown::dropped(GetDlgItem(hwnd, FORMAT as i32)) => {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    return true;
                }
                VK_RETURN if is_input(GetDlgCtrlID(msg.hwnd) as usize) => {
                    SendMessageW(msg.hwnd, msg.message, msg.wParam, msg.lParam);
                    return true;
                }
                _ => {}
            }
        }
        IsDialogMessageW(hwnd, msg) != 0
    }
}

pub(super) fn sync_icon_availability(hwnd: HWND, multicolor: bool) {
    let control = unsafe { GetDlgItem(hwnd, ICON_COLOR as i32) };
    if !control.is_null() {
        unsafe {
            EnableWindow(control, i32::from(!multicolor));
            InvalidateRect(control, null(), 0);
        }
    }
}

pub(super) fn register_window() -> io::Result<()> {
    register(CLASS, Some(dialog_proc))?;
    register(GRAPH_CLASS, Some(graph_proc))
}

fn graph_height(target: Target) -> i32 {
    if target.has_transparency() { 228 } else { 222 }
}

fn value_row_top(target: Target) -> i32 {
    GRAPH_TOP + graph_height(target) + 9
}

fn preset_top(target: Target, has_recents: bool) -> i32 {
    value_row_top(target) + CONTROL_HEIGHT + 12 + if has_recents { 43 } else { 0 }
}

fn display(hwnd: HWND) {
    let (target, recents) = PICKER.with_borrow(|picker| {
        let picker = picker.as_ref().unwrap();
        (picker.target, picker.recents.clone())
    });
    let page = Page(hwnd);
    let background = matches!(target, Target::Background { .. });
    let preset_top = preset_top(target, !recents.is_empty());
    let bottom = preset_top + 42;
    page.section("", GRAPH_TOP - 16, bottom, &[]);
    child(
        hwnd,
        GRAPH_CLASS,
        "",
        0,
        GRAPH,
        place(16, GRAPH_TOP, 504, graph_height(target), STRETCH),
    );
    let row_top = value_row_top(target);
    let copy_x = WIDTH - 16 - CONTROL_HEIGHT;
    combo(
        hwnd,
        FORMAT,
        &["HEX", "RGB", "HSL"],
        0,
        place(16, row_top, 80, CONTROL_HEIGHT, 0),
    );
    const VALUE_X: i32 = 112;
    let value = edit_at(
        hwnd,
        VALUE,
        "",
        place(
            VALUE_X,
            row_top,
            copy_x - VALUE_X - 16,
            CONTROL_HEIGHT,
            STRETCH,
        ),
    );
    cue(
        value,
        if target.has_transparency() {
            "颜色值 / 透明度"
        } else {
            "颜色值"
        },
    );
    action(
        hwnd,
        COPY,
        "复制颜色值",
        place(copy_x, row_top, CONTROL_HEIGHT, CONTROL_HEIGHT, RIGHT),
    );
    if !recents.is_empty() {
        let recent_top = row_top + CONTROL_HEIGHT + 12;
        page.body("最近使用颜色", place(17, recent_top + 3, 148, 22, 0));
        swatches(hwnd, FIRST_RECENT, &recents, recent_top);
    }
    page.body("预设颜色", place(17, preset_top + 3, 148, 22, 0));
    swatches(hwnd, FIRST_PRESET, &PRESETS, preset_top);
    let reset = if background {
        "恢复默认"
    } else {
        "使用自动颜色"
    };
    page.buttons(0, bottom + 12, 0, &[(RESET, reset)]);
    page.buttons(
        WIDTH,
        bottom + 12,
        RIGHT,
        &[(CANCEL, "取消"), (APPLY, "应用")],
    );
    page.finish_at(bottom + 56);
    sync(hwnd);
}

fn swatches(hwnd: HWND, first: usize, colors: &[u32], y: i32) {
    for (index, rgb) in colors.iter().enumerate() {
        action(
            hwnd,
            first + index,
            &format!("#{rgb:06X}"),
            place(171 + index as i32 * 33, y, 28, 28, RIGHT),
        );
    }
}

fn swatch(id: usize) -> Option<u32> {
    let index = id.checked_sub(FIRST_PRESET)?;
    if index < PRESETS.len() {
        PRESETS.get(index).copied()
    } else if (FIRST_RECENT..FIRST_RECENT + 8).contains(&id) {
        PICKER.with_borrow(|current| current.as_ref()?.recents.get(id - FIRST_RECENT).copied())
    } else {
        None
    }
}

pub(super) fn paint_button(
    painter: &mut Painter,
    id: usize,
    item: &DRAWITEMSTRUCT,
    width: f32,
    height: f32,
    dpi: u32,
) -> bool {
    if !matches!(
        id,
        LIGHT
            | DARK
            | TEXT_COLOR
            | ICON_COLOR
            | HISTORY_LIGHT
            | HISTORY_DARK
            | HISTORY_HIGHLIGHT
            | COPY
    ) && !(FIRST_PRESET..FIRST_RECENT + 8).contains(&id)
    {
        return false;
    }
    let colors = fluent::palette();
    if id == COPY {
        let icon = (
            "copy",
            include_bytes!("../../../assets/icons/tabler/copy.svg").as_slice(),
        );
        fluent::icon(
            painter,
            icon,
            (width - 16.0) / 2.0,
            (height - 16.0) / 2.0,
            16.0,
            dpi,
            colors.text,
        );
        return true;
    }
    let rgb = match id {
        LIGHT | DARK | TEXT_COLOR | ICON_COLOR | HISTORY_LIGHT | HISTORY_DARK
        | HISTORY_HIGHLIGHT => {
            let Some(state) = session() else { return false };
            let app = state.app.borrow();
            let prefs = &app.settings().preferences;
            let rgb = match id {
                TEXT_COLOR => prefs.search_text_color,
                ICON_COLOR if prefs.multicolor_engine_icons => {
                    painter.text(
                        tr("多色图标已启用"),
                        rect(10.0, 0.0, width - 20.0, height),
                        colors.detail,
                        Label::Control,
                    );
                    return true;
                }
                ICON_COLOR => prefs.search_mono_icon_color,
                HISTORY_LIGHT | HISTORY_DARK => Some(prefs.history_popup.tint(id == HISTORY_DARK)),
                HISTORY_HIGHLIGHT => Some(
                    prefs
                        .history_popup
                        .highlight_color
                        .unwrap_or(yisou::settings::DEFAULT_HISTORY_HIGHLIGHT),
                ),
                _ => Some(prefs.search_tint(id == DARK)),
            };
            let Some(rgb) = rgb else {
                painter.text(
                    tr("自动颜色"),
                    rect(10.0, 0.0, width - 20.0, height),
                    colors.text,
                    Label::Control,
                );
                return true;
            };
            let top = (height - 16.0) / 2.0;
            let spot = rect(10.0, top, 16.0, 16.0);
            painter.rounded(spot, 4.0, rgb, false);
            painter.rounded(spot, 4.0, colors.border, true);
            let label = match id {
                LIGHT | HISTORY_LIGHT => format!("{}  #{rgb:06X}", tr("浅色")),
                DARK | HISTORY_DARK => format!("{}  #{rgb:06X}", tr("深色")),
                _ => format!("#{rgb:06X}"),
            };
            painter.text(
                &label,
                rect(35.0, 0.0, width - 45.0, height),
                colors.text,
                Label::Control,
            );
            return true;
        }
        _ => {
            let Some(rgb) = swatch(id) else { return false };
            rgb
        }
    };
    let selected = PICKER.with_borrow(|current| {
        current
            .as_ref()
            .is_some_and(|picker| picker.hsv.rgb() == rgb)
    });
    let spot = rect(4.0, 4.0, width - 8.0, height - 8.0);
    painter.rounded(spot, 4.0, rgb, false);
    painter.rounded(spot, 4.0, colors.border, true);
    if selected || item.itemState & ODS_FOCUS != 0 {
        painter.rounded(
            rect(1.0, 1.0, width - 2.0, height - 2.0),
            5.0,
            colors.accent,
            true,
        );
    }
    true
}

fn sync(hwnd: HWND) {
    let value = PICKER.with_borrow(|current| current.as_ref().map(Picker::value_text));
    if let Some(value) = value {
        if field(hwnd, VALUE) != value {
            set_field(hwnd, VALUE as i32, &value);
        }
        repaint(hwnd);
    }
}

fn repaint(hwnd: HWND) {
    unsafe {
        InvalidateRect(GetDlgItem(hwnd, GRAPH as i32), null(), 0);
        for id in FIRST_PRESET..FIRST_RECENT + 8 {
            let button = GetDlgItem(hwnd, id as i32);
            if !button.is_null() {
                InvalidateRect(button, null(), 0);
            }
        }
    }
}

pub(super) fn is_input(id: usize) -> bool {
    id == VALUE
}

pub(super) fn read_input(hwnd: HWND, restore: bool) -> Result<(), &'static str> {
    let text = field(hwnd, VALUE);
    let result = PICKER.with_borrow_mut(|current| {
        current
            .as_mut()
            .map_or(Ok(()), |picker| picker.read_value(&text))
    });
    if result.is_ok() || restore {
        sync(hwnd);
    }
    result
}

fn select_format(hwnd: HWND, control: HWND) {
    let format = match unsafe { SendMessageW(control, CB_GETCURSEL, 0, 0) } {
        1 => Format::Rgb,
        2 => Format::Hsl,
        _ => Format::Hex,
    };
    PICKER.with_borrow_mut(|current| {
        if let Some(picker) = current.as_mut() {
            picker.format = format;
        }
    });
    sync(hwnd);
}

fn command(state: &State, hwnd: HWND, id: usize) -> Result<(), String> {
    match id {
        APPLY => apply(state, hwnd),
        CANCEL => {
            close(hwnd);
            Ok(())
        }
        COPY => {
            read_input(hwnd, false).map_err(|message| tr(message).to_owned())?;
            let text = PICKER.with_borrow(|current| {
                let picker = current.as_ref().unwrap();
                picker.format.display(picker.hsv)
            });
            if super::super::write_clipboard(hwnd, &text) {
                Ok(())
            } else {
                Err(tr("无法复制颜色值").into())
            }
        }
        RESET => {
            PICKER.with_borrow_mut(|current| {
                let picker = current.as_mut().unwrap();
                picker.hsv = Hsv::from_rgb(picker.default_color, 0.0);
                picker.automatic = !matches!(picker.target, Target::Background { .. });
                if picker.automatic {
                    picker.transparency = if matches!(picker.target, Target::HistoryHighlight) {
                        yisou::settings::HistoryPopup::default().highlight_transparency
                    } else {
                        0
                    };
                }
            });
            sync(hwnd);
            Ok(())
        }
        _ => {
            let Some(rgb) = swatch(id) else { return Ok(()) };
            PICKER.with_borrow_mut(|current| {
                let picker = current.as_mut().unwrap();
                picker.hsv = Hsv::from_rgb(rgb, picker.hsv.hue);
                picker.automatic = false;
            });
            sync(hwnd);
            Ok(())
        }
    }
}

fn apply(state: &State, hwnd: HWND) -> Result<(), String> {
    read_input(hwnd, false).map_err(|message| tr(message).to_owned())?;
    let (target, rgb, automatic, transparency) = PICKER.with_borrow(|current| {
        let picker = current.as_ref().unwrap();
        (
            picker.target,
            picker.hsv.rgb(),
            picker.automatic,
            picker.transparency,
        )
    });
    let tint = 100 - transparency;
    change(state, |settings| {
        let prefs = &mut settings.preferences;
        match target {
            Target::Background {
                dark,
                material,
                history: true,
            } => {
                let history = &mut prefs.history_popup;
                if dark {
                    history.dark_color = rgb;
                } else {
                    history.light_color = rgb;
                }
                match material {
                    SearchMaterial::Acrylic => history.acrylic_tint_percent = tint,
                    SearchMaterial::Mica => history.mica_tint_percent = tint,
                    SearchMaterial::Solid => {}
                }
            }
            Target::Background {
                dark,
                material,
                history: false,
            } => {
                if dark {
                    prefs.search_dark_color = rgb;
                } else {
                    prefs.search_light_color = rgb;
                }
                match material {
                    SearchMaterial::Acrylic => prefs.acrylic_tint_percent = tint,
                    SearchMaterial::Mica => prefs.mica_tint_percent = tint,
                    SearchMaterial::Solid => {}
                }
            }
            Target::Text => {
                prefs.search_text_color = (!automatic).then_some(rgb);
                prefs.search_text_transparency = transparency;
            }
            Target::Icon => {
                prefs.search_mono_icon_color = (!automatic).then_some(rgb);
                prefs.search_mono_icon_transparency = transparency;
            }
            Target::HistoryHighlight => {
                prefs.history_popup.highlight_color = (!automatic).then_some(rgb);
                prefs.history_popup.highlight_transparency = transparency;
            }
        }
        if !automatic {
            prefs.recent_search_colors.retain(|&recent| recent != rgb);
            prefs.recent_search_colors.insert(0, rgb);
        }
    })?;
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
    if let Target::Background {
        material, history, ..
    } = target
    {
        let input = match (material, history) {
            (SearchMaterial::Acrylic, false) => Some(appearance::ACRYLIC_TINT),
            (SearchMaterial::Mica, false) => Some(appearance::MICA_TINT),
            (SearchMaterial::Acrylic, true) => Some(appearance::HISTORY_ACRYLIC_TINT),
            (SearchMaterial::Mica, true) => Some(appearance::HISTORY_MICA_TINT),
            (SearchMaterial::Solid, _) => None,
        };
        if let Some(input) = input
            && let Some(stepper) =
                appearance::search_steppers().find(|stepper| stepper.input == input)
        {
            set_field(owner, input as i32, &transparency.to_string());
            stepper.sync(owner, transparency);
        }
    }
    if matches!(
        target,
        Target::HistoryHighlight | Target::Background { history: true, .. }
    ) {
        state.dismiss_history_popup();
    } else if matches!(target, Target::Background { history: false, .. }) {
        state.invalidate_following_history_background();
    }
    state.edit_dirty.set(true);
    unsafe {
        RedrawWindow(
            state.window.get(),
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN,
        );
        RedrawWindow(
            owner,
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
        );
    }
    close(hwnd);
    Ok(())
}

fn close(hwnd: HWND) {
    let id = PICKER.with_borrow(|current| current.as_ref().unwrap().target.id());
    unsafe {
        let owner = GetWindow(hwnd, GW_OWNER);
        EnableWindow(owner, 1);
        DestroyWindow(hwnd);
        if GetActiveWindow() == owner {
            focus_control(owner, id as i32);
        }
    }
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if hwnd != window() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                fluent::paint(hwnd, dc, ps.rcPaint);
                EndPaint(hwnd, &ps);
                0
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                fluent::control_color(hwnd, wp as HDC, lp as HWND, msg)
            }
            WM_DRAWITEM if lp != 0 => {
                if let Some(state) = session() {
                    fluent::draw_item(&state, hwnd, &*(lp as *const DRAWITEMSTRUCT));
                }
                1
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                fluent::apply_palette(hwnd);
                0
            }
            WM_SIZE if wp != SIZE_MINIMIZED as usize => {
                fluent::layout(hwnd);
                0
            }
            WM_DPICHANGED if lp != 0 => {
                let height = fluent::with_view(hwnd, |view| view.extent);
                let bounds = window_bounds(*(lp as *const RECT), wp as u32 & 0xffff, height, false);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    bounds.left,
                    bounds.top,
                    bounds.right - bounds.left,
                    bounds.bottom - bounds.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                0
            }
            WM_MOUSEWHEEL | WM_VSCROLL => {
                fluent::scroll(hwnd, msg, wp);
                0
            }
            WM_TIMER if wp == fluent::HOVER_TIMER => {
                fluent::tick_hover(hwnd);
                0
            }
            WM_COMMAND => {
                let id = wp & 0xffff;
                let code = (wp >> 16) as u32;
                let child = lp as HWND;
                match id {
                    FORMAT if !child.is_null() => {
                        dropdown::notice(child, code);
                        if code == CBN_CLOSEUP || code == CBN_SELCHANGE && !dropdown::dropped(child)
                        {
                            select_format(hwnd, child);
                        }
                    }
                    VALUE if code == EN_KILLFOCUS => {
                        let _ = read_input(hwnd, false);
                    }
                    _ if code == BN_CLICKED => {
                        if let Some(state) = session()
                            && let Err(error) = command(&state, hwnd, id)
                        {
                            message(hwnd, &error);
                        }
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE | BACK => {
                close(hwnd);
                0
            }
            WM_NCDESTROY => {
                fluent::with_view_mut(hwnd, |view| *view = fluent::View::default());
                WINDOW.set(null_mut());
                PICKER.with_borrow_mut(|current| *current = None);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

const PLANE_HEIGHT: f32 = 170.0;
const HUE_Y: f32 = 181.0;
const TRANSPARENCY_Y: f32 = 207.0;
const TRACK_HEIGHT: f32 = 12.0;
const SWATCH_X: f32 = 2.0;
const SWATCH_SIZE: f32 = TRANSPARENCY_Y + TRACK_HEIGHT - HUE_Y;
const TRACK_X: f32 = SWATCH_X + SWATCH_SIZE + 11.0;
const TRACK_RIGHT_INSET: f32 = 8.0;
const THUMB_RADIUS: f32 = 7.0;

fn track_centers(width: f32) -> (f32, f32) {
    let inset = THUMB_RADIUS + 0.5;
    let first = TRACK_X + inset;
    let last = width - TRACK_RIGHT_INSET - inset;
    (first, last.max(first + 1.0))
}

fn gradient_stop(position: f32, rgb: u32, alpha: f32) -> D2D1_GRADIENT_STOP {
    D2D1_GRADIENT_STOP {
        position,
        color: color(rgb, alpha),
    }
}

fn checker(p: &Painter, x: f32, y: f32, width: f32, height: f32) {
    const TILE: f32 = 6.0;
    p.rounded(rect(x, y, width, height), 0.0, 0xe9edf2, false);
    for row in 0..(height / TILE).ceil() as usize {
        for col in (row % 2..(width / TILE).ceil() as usize).step_by(2) {
            let left = col as f32 * TILE;
            let top = row as f32 * TILE;
            p.rounded(
                rect(
                    x + left,
                    y + top,
                    TILE.min(width - left),
                    TILE.min(height - top),
                ),
                0.0,
                0xcdd4df,
                false,
            );
        }
    }
}

fn thumb(p: &Painter, x: f32, y: f32) {
    p.rounded(
        rect(
            x - THUMB_RADIUS,
            y - THUMB_RADIUS,
            2.0 * THUMB_RADIUS,
            2.0 * THUMB_RADIUS,
        ),
        THUMB_RADIUS,
        0x23272e,
        true,
    );
    p.rounded(rect(x - 6.0, y - 6.0, 12.0, 12.0), 6.0, 0xffffff, true);
}

fn paint(hwnd: HWND, dc: HDC) {
    let parent = unsafe { GetParent(hwnd) };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut bounds = RECT::default();
    unsafe { GetClientRect(hwnd, &mut bounds) };
    let width = bounds.right as f32 * 96.0 / dpi as f32;
    let colors = fluent::control_surface(parent, hwnd, fluent::palette());
    let Some((hsv, transparency, has_transparency)) = PICKER.with_borrow(|current| {
        let picker = current.as_ref()?;
        Some((
            picker.hsv,
            picker.transparency,
            picker.target.has_transparency(),
        ))
    }) else {
        return;
    };
    fluent::with_painter(dc, bounds, dpi, |p| {
        p.clear(colors);
        let plane = rect(1.0, 1.0, width - 2.0, PLANE_HEIGHT - 2.0);
        let hue = Hsv {
            hue: hsv.hue,
            saturation: 1.0,
            value: 1.0,
        }
        .rgb();
        p.gradient(
            plane,
            9.0,
            false,
            &[
                gradient_stop(0.0, 0xffffff, 1.0),
                gradient_stop(1.0, hue, 1.0),
            ],
        );
        p.gradient(
            plane,
            9.0,
            true,
            &[gradient_stop(0.0, 0, 0.0), gradient_stop(1.0, 0, 1.0)],
        );
        thumb(
            p,
            8.0 + hsv.saturation * (width - 16.0),
            8.0 + (1.0 - hsv.value) * (PLANE_HEIGHT - 16.0),
        );
        let rgb = hsv.rgb();
        let swatch = rect(SWATCH_X, HUE_Y, SWATCH_SIZE, SWATCH_SIZE);
        if has_transparency {
            p.with_rounded_clip(swatch, 4.0, || {
                checker(p, swatch.left, swatch.top, SWATCH_SIZE, SWATCH_SIZE);
                p.rounded_alpha(swatch, 0.0, rgb, (100 - transparency) as f32 / 100.0, false);
            });
        } else {
            p.rounded(swatch, 4.0, rgb, false);
        }
        p.rounded(
            rect(
                swatch.left + 0.5,
                swatch.top + 0.5,
                SWATCH_SIZE - 1.0,
                SWATCH_SIZE - 1.0,
            ),
            3.5,
            fluent::palette().border,
            true,
        );

        let track_width = width - TRACK_X - TRACK_RIGHT_INSET;
        let radius = TRACK_HEIGHT / 2.0;
        let hue_y = if has_transparency {
            HUE_Y
        } else {
            HUE_Y + (SWATCH_SIZE - TRACK_HEIGHT) / 2.0
        };
        let (first, last) = track_centers(width);
        let thumb_x = |progress: f32| first + progress * (last - first);
        let stop_x = |progress: f32| (thumb_x(progress) - TRACK_X) / track_width;
        let hues = [
            0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
        ];
        let stops: [_; 7] =
            std::array::from_fn(|i| gradient_stop(stop_x(i as f32 / 6.0), hues[i], 1.0));
        p.gradient(
            rect(TRACK_X, hue_y, track_width, TRACK_HEIGHT),
            radius,
            false,
            &stops,
        );
        thumb(p, thumb_x(hsv.hue), hue_y + radius);
        if has_transparency {
            let track = rect(TRACK_X, TRANSPARENCY_Y, track_width, TRACK_HEIGHT);
            p.with_rounded_clip(track, radius, || {
                checker(p, TRACK_X, TRANSPARENCY_Y, track_width, TRACK_HEIGHT);
                p.gradient(
                    track,
                    0.0,
                    false,
                    &[
                        gradient_stop(stop_x(0.0), rgb, 1.0),
                        gradient_stop(stop_x(1.0), rgb, 0.0),
                    ],
                );
            });
            thumb(
                p,
                thumb_x(transparency as f32 / 100.0),
                TRANSPARENCY_Y + radius,
            );
        }
    });
}

fn drag(hwnd: HWND, lp: LPARAM, start: bool) -> bool {
    let scale = 96.0 / unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
    let x = f32::from(lp as i16) * scale;
    let y = f32::from((lp >> 16) as i16) * scale;
    let mut bounds = RECT::default();
    unsafe { GetClientRect(hwnd, &mut bounds) };
    let width = bounds.right as f32 * scale;
    let (first, last) = track_centers(width);
    let changed = PICKER.with_borrow_mut(|current| {
        let Some(picker) = current.as_mut() else {
            return false;
        };
        if start {
            picker.drag = if y <= PLANE_HEIGHT {
                Some(Drag::Plane)
            } else if x < TRACK_X - 8.0 {
                None
            } else if !picker.target.has_transparency() || y < TRANSPARENCY_Y - 4.0 {
                Some(Drag::Hue)
            } else {
                Some(Drag::Transparency)
            };
        }
        let position =
            |value: f32, start: f32, end: f32| ((value - start) / (end - start)).clamp(0.0, 1.0);
        match picker.drag {
            Some(Drag::Plane) => {
                picker.hsv.saturation = position(x, 8.0, width - 8.0);
                picker.hsv.value = 1.0 - position(y, 8.0, PLANE_HEIGHT - 8.0);
                picker.automatic = false;
            }
            Some(Drag::Hue) => {
                picker.hsv.hue = position(x, first, last);
                picker.automatic = false;
            }
            Some(Drag::Transparency) => {
                picker.transparency = (position(x, first, last) * 100.0).round() as i32
            }
            None => return false,
        }
        true
    });
    if changed {
        sync(unsafe { GetParent(hwnd) });
    }
    changed
}

unsafe extern "system" fn graph_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            paint(hwnd, dc);
            EndPaint(hwnd, &ps);
            0
        },
        WM_PRINTCLIENT => {
            paint(hwnd, wp as HDC);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEWHEEL => unsafe { SendMessageW(GetParent(hwnd), msg, wp, lp) },
        WM_LBUTTONDOWN => unsafe {
            let _ = read_input(GetParent(hwnd), false);
            if drag(hwnd, lp, true) {
                SetCapture(hwnd);
            }
            0
        },
        WM_MOUSEMOVE => {
            drag(hwnd, lp, false);
            0
        }
        WM_LBUTTONUP => unsafe {
            if GetCapture() == hwnd {
                drag(hwnd, lp, false);
                ReleaseCapture();
            }
            0
        },
        WM_CAPTURECHANGED => {
            PICKER.with_borrow_mut(|current| {
                if let Some(picker) = current.as_mut() {
                    picker.drag = None;
                }
            });
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_formats_round_trip_rgb() {
        for rgb in [0, 0xffffff, 0xff0000, 0x2b4883, 0x00ffdf, 0x005500] {
            let hsv = Hsv::from_rgb(rgb, 0.0);
            for format in [Format::Hex, Format::Rgb, Format::Hsl] {
                assert_eq!(format.parse(&format.display(hsv), 0.0).unwrap().rgb(), rgb);
            }
        }
        assert!(Format::Hex.parse("#AABBCCFF", 0.0).is_none());
        assert!(Format::Rgb.parse("256, 0, 0", 0.0).is_none());
        assert!(Format::Hsl.parse("20, NaN, 100", 0.0).is_none());
    }
}
