use super::{
    REVEAL, State, TITLE, color_ref,
    edit_paint::CARET_TIMER,
    instance, message,
    paint::{ACCENT, BORDER, HOVER, Label, MUTED, Painter, SELECTED, TEXT, blend, rect},
    popup::PopupKind,
    preferences,
    selection::{DOUBLE_ALT, SELECTION_READY, SELECTION_TIMER},
    session, text_of,
    tray::TRAY,
    wide,
};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    io,
    ptr::{null, null_mut},
};
use windows::{
    Win32::Graphics::{Direct2D::Common::D2D_RECT_F, DirectWrite::*},
    core::w as ww,
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::{
            Dwm::{
                DWMWA_BORDER_COLOR, DWMWA_CLOAK, DWMWA_COLOR_NONE, DWMWA_TRANSITIONS_FORCEDISABLED,
                DwmFlush, DwmSetWindowAttribute,
            },
            Gdi::*,
        },
        UI::{
            Controls::{
                DRAWITEMSTRUCT, EM_GETSEL, EM_REPLACESEL, EM_SETCUEBANNER, EM_SETLIMITTEXT,
                EM_SETMARGINS, EM_SETSEL, HOVER_DEFAULT, ICC_BAR_CLASSES, INITCOMMONCONTROLSEX,
                InitCommonControlsEx, NMHDR, NMTTDISPINFOW, ODS_FOCUS, ODS_SELECTED,
                TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_POP,
                TTN_GETDISPINFOW, TTS_NOPREFIX, TTTOOLINFOW, WM_MOUSELEAVE,
            },
            HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
            Input::{Ime::*, KeyboardAndMouse::*},
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
    w,
};
use yisou::{
    application::{Origin, Query, SearchOutcome},
    engine::{alias_prefix, is_web_url},
    launch::{WindowAction, close_search},
    platform::Hotkey,
    settings::{Font, Preferences, SearchMaterial},
};

pub(super) const MAIN_CLASS: *const u16 = w!("YiSou.Rust.Search");
const ENGINE: usize = 101;
const ENGINE_BUTTON_HEIGHT: i32 = 40;
const MIN_SEARCH_EDIT_WIDTH: i32 = 80;
const ENGINE_NAME_GAP: i32 = 6;
const MAX_ENGINE_NAME_WIDTH: i32 = 200;
pub(super) const CONTENT_GAP: i32 = 8;
const HINT_HEIGHT: i32 = 27;
const GRAPHICS_TIMER: usize = 2;
pub(super) const HISTORY_TIMER: usize = 4;

struct EngineNameFont {
    factory: IDWriteFactory,
    format: IDWriteTextFormat,
    widths: HashMap<String, i32>,
}

thread_local! {
    static ENGINE_NAME_FONT: RefCell<Option<EngineNameFont>> = RefCell::new(None);
}

fn with_engine_name_font<R>(use_font: impl FnOnce(&mut EngineNameFont) -> R) -> Option<R> {
    ENGINE_NAME_FONT.with_borrow_mut(|slot| {
        if slot.is_none() {
            let factory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ok()?;
            let format = Painter::text_format(&factory, ww!("Segoe UI"), None, Label::Body).ok()?;
            *slot = Some(EngineNameFont {
                factory,
                format,
                widths: HashMap::new(),
            });
        }
        slot.as_mut().map(use_font)
    })
}

fn engine_name_width(name: &str) -> i32 {
    with_engine_name_font(|font| {
        if let Some(&width) = font.widths.get(name) {
            return width;
        }
        let units: Vec<_> = name.encode_utf16().collect();
        let width = Painter::measure_text(&font.factory, &font.format, &units)
            .map_or(0, |width| width.ceil() as i32 + 1)
            .min(MAX_ENGINE_NAME_WIDTH);
        if font.widths.len() >= 64 {
            font.widths.clear();
        }
        font.widths.insert(name.to_owned(), width);
        width
    })
    .unwrap_or_default()
}

pub(super) fn engine_button_width(prefs: &Preferences, name: Option<&str>, width: i32) -> i32 {
    let name_width = name
        .filter(|_| prefs.show_search_engine_name)
        .map_or(0, engine_name_width);
    let gap = if name_width > 0 { ENGINE_NAME_GAP } else { 0 };
    let padding = prefs.search_padding;
    let available = width - padding.left - padding.right - CONTENT_GAP - MIN_SEARCH_EDIT_WIDTH;
    (prefs.search_engine_icon_size + gap + name_width)
        .clamp(ENGINE_BUTTON_HEIGHT, available.max(ENGINE_BUTTON_HEIGHT))
}

pub(super) fn search_row_height(font_size: i32) -> i32 {
    (font_size * 5 / 4 + 8).max(ENGINE_BUTTON_HEIGHT)
}

fn monitor_work_area(monitor: HMONITOR) -> Option<RECT> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    (unsafe { GetMonitorInfoW(monitor, &mut info) } != 0).then_some(info.rcWork)
}

pub(super) fn editable_query_text(text: &str) -> Cow<'_, str> {
    if text.contains(['\r', '\n']) {
        Cow::Owned(text.replace("\r\n", " ").replace(['\r', '\n'], " "))
    } else {
        Cow::Borrowed(text)
    }
}

pub(super) fn key_down(key: u16) -> bool {
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

pub(super) struct SearchColors {
    pub surface: u32,
    pub text: u32,
    pub muted: u32,
    pub error: u32,
    pub bright_foreground: bool,
    border: u32,
    hover: u32,
    selected: u32,
}

#[derive(Clone, Copy)]
pub(super) enum BrowserFailure {
    Search,
    Link,
}

impl SearchColors {
    pub(super) fn for_dark(dark: bool) -> Self {
        if dark {
            Self {
                surface: 0,
                text: 0xf5f5f5,
                muted: 0xb8bdc5,
                error: 0xffa8a3,
                bright_foreground: true,
                border: 0x42464f,
                hover: 0x252932,
                selected: 0x323b49,
            }
        } else {
            Self {
                surface: 0xffffff,
                text: TEXT,
                muted: MUTED,
                error: 0x9d3026,
                bright_foreground: false,
                border: BORDER,
                hover: HOVER,
                selected: SELECTED,
            }
        }
    }

    pub(super) fn paint_border(
        &self,
        painter: &mut Painter,
        size: (i32, i32),
        dpi: u32,
        focused: bool,
        radius: i32,
    ) {
        let (width, height) = size;
        let pixel = 96.0 / dpi as f32;
        let border = if focused {
            blend(self.border, ACCENT, 0.25)
        } else {
            self.border
        };
        painter.smooth_border(
            width as f32 * pixel,
            height as f32 * pixel,
            radius,
            pixel,
            border,
        );
    }
}

fn set_dwm_attribute(window: HWND, attribute: i32, value: u32) -> bool {
    unsafe {
        DwmSetWindowAttribute(
            window,
            attribute as u32,
            (&value as *const u32).cast(),
            size_of::<u32>() as u32,
        ) >= 0
    }
}

impl State {
    pub(super) fn search_control_focused(&self) -> bool {
        let focus = unsafe { GetFocus() };
        !focus.is_null() && (focus == self.edit.get() || focus == self.engine_button.get())
    }

    pub(super) fn search_is_dark(&self) -> bool {
        self.app
            .borrow()
            .settings()
            .preferences
            .search_tone
            .is_dark(self.system_dark.get())
    }

    pub(super) fn search_colors(&self) -> SearchColors {
        self.search_colors_for(&self.app.borrow().settings().preferences)
    }

    pub(super) fn search_colors_for(&self, prefs: &Preferences) -> SearchColors {
        let mut colors = self.resolve_search_colors(prefs, false);
        colors.text = prefs.search_text_color.unwrap_or(colors.text);
        colors
    }

    pub(super) fn search_preview_colors(&self) -> SearchColors {
        let app = self.app.borrow();
        let prefs = &app.settings().preferences;
        let mut colors = self.resolve_search_colors(prefs, true);
        colors.text = blend(
            colors.surface,
            prefs.search_text_color.unwrap_or(colors.text),
            prefs.search_text_opacity(),
        );
        colors
    }

    pub(super) fn resolve_search_colors(&self, prefs: &Preferences, preview: bool) -> SearchColors {
        let dark = prefs.search_tone.is_dark(self.system_dark.get());
        let surface = prefs.search_tint(dark);
        let default = if dark { 0 } else { 0xffffff };
        if surface == default && !preview {
            return SearchColors::for_dark(dark);
        }
        // Estimate the combined fill using the material's light or dark base.
        // The desktop colors under Acrylic change as the window moves.
        let base = if dark { 0x282828 } else { 0xf0f0f0 };
        let percent = if !preview
            && self
                .backdrop_material
                .get()
                .is_none_or(|material| material == SearchMaterial::Solid)
        {
            100
        } else {
            prefs.search_tint_percent()
        };
        let fill = blend(base, surface, percent as f32 / 100.0);
        let [r, g, b] = [16, 8, 0].map(|shift| (fill >> shift) & 255);
        let linear = |channel: u32| {
            let srgb = channel as f32 / 255.0;
            if srgb <= 0.04045 {
                srgb / 12.92
            } else {
                ((srgb + 0.055) / 1.055).powf(2.4)
            }
        };
        let luminance = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
        let mut colors = SearchColors::for_dark(luminance < 0.18);
        colors.surface = if preview { fill } else { surface };
        colors
    }

    pub(super) fn create_search_window(&self) -> io::Result<HWND> {
        let height = self.search_height();
        let hwnd = unsafe {
            // Keep a redirection surface for native controls when switching to solid.
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                MAIN_CLASS,
                TITLE,
                WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                self.search_width(),
                height,
                null_mut(),
                null_mut(),
                instance(),
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        self.window.set(hwnd);
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        self.dpi.set(if dpi == 0 { 96 } else { dpi });
        self.dwm(hwnd, 2);
        set_dwm_attribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, 1);
        set_dwm_attribute(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
        Ok(hwnd)
    }

    fn create_search_controls(&self) -> io::Result<()> {
        let hwnd = self.window.get();
        unsafe {
            let edit = CreateWindowExW(
                0,
                w!("EDIT"),
                null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                0,
                0,
                0,
                0,
                hwnd,
                100 as HMENU,
                instance(),
                null(),
            );
            let engine = CreateWindowExW(
                0,
                w!("BUTTON"),
                w!("选择搜索引擎"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                0,
                0,
                0,
                hwnd,
                ENGINE as HMENU,
                instance(),
                null(),
            );
            if edit.is_null()
                || engine.is_null()
                || SetWindowSubclass(edit, Some(control_proc), 1, 0) == 0
                || SetWindowSubclass(engine, Some(control_proc), 1, 0) == 0
            {
                let error = io::Error::last_os_error();
                for control in [edit, engine] {
                    if !control.is_null() {
                        DestroyWindow(control);
                    }
                }
                return Err(error);
            }
            self.edit.set(edit);
            self.engine_button.set(engine);
            self.engine_tooltip.set(create_engine_tooltip(hwnd, engine));
            SendMessageW(edit, EM_SETLIMITTEXT, 2048, 0);
            SendMessageW(edit, EM_SETCUEBANNER, 1, w!("搜索网页…") as isize);
        }
        self.apply_font();
        self.diagnostics.sample("search_controls_created");
        Ok(())
    }

    pub(super) fn font_family(&self) -> String {
        #[cfg(feature = "text-render-lab")]
        if let Some(sample) = self.text_sample.get() {
            return sample.family.to_owned();
        }
        match &self.app.borrow().settings().preferences.font {
            Font::System(name) => name.clone(),
            Font::File(_) => self.file_font.borrow().as_ref().map_or_else(
                || "Microsoft YaHei UI".to_owned(),
                |font| font.family.clone(),
            ),
            _ => "Microsoft YaHei UI".to_owned(),
        }
    }

    pub(super) fn reload_file_font(&self) {
        let font = match &self.app.borrow().settings().preferences.font {
            Font::File(path) => super::font::FileFont::load(path).ok(),
            _ => None,
        };
        *self.file_font.borrow_mut() = font;
    }

    pub(super) fn apply_font(&self) {
        if self.edit.get().is_null() {
            return;
        }
        let font = self.create_ui_font(self.dpi.get());
        if !font.is_null() {
            unsafe {
                SendMessageW(self.edit.get(), WM_SETFONT, font as usize, 1);
                SendMessageW(
                    self.edit.get(),
                    EM_SETMARGINS,
                    (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
                    0,
                );
            }
            Self::delete_font(self.font.replace(font));
        }
        self.search_edit_height.set(unsafe {
            let edit = self.edit.get();
            let dc = GetDC(edit);
            if dc.is_null() {
                self.scale(self.input_font_size() + 5)
            } else {
                let previous = SelectObject(dc, self.font.get());
                let mut metrics = TEXTMETRICW::default();
                let height = if GetTextMetricsW(dc, &mut metrics) != 0 {
                    metrics.tmHeight + self.scale(2)
                } else {
                    self.scale(self.input_font_size() + 5)
                };
                SelectObject(dc, previous);
                ReleaseDC(edit, dc);
                height
            }
        });
        *self.painter.borrow_mut() = None;
        self.release_backdrop();
        self.layout_dpi.set(0);
        self.layout();
        unsafe { InvalidateRect(self.window.get(), null(), 0) };
    }

    pub(super) fn create_ui_font(&self, dpi: u32) -> HFONT {
        self.create_font_at(dpi, self.input_font_size())
    }

    pub(super) fn input_font_size(&self) -> i32 {
        #[cfg(feature = "text-render-lab")]
        if let Some(sample) = self.text_sample.get() {
            return sample.size;
        }
        self.app.borrow().settings().preferences.search_font_size
    }

    pub(super) fn create_font_at(&self, dpi: u32, size: i32) -> HFONT {
        super::font::preview_font(&self.font_family(), self.font_face(), size, dpi)
    }

    pub(super) fn search_height(&self) -> i32 {
        let padding = self.app.borrow().settings().preferences.search_padding;
        padding.top + search_row_height(self.input_font_size()) + padding.bottom
    }

    fn search_width(&self) -> i32 {
        self.app.borrow().settings().preferences.search_width
    }

    pub(super) fn search_hint_area(&self, width_px: i32) -> D2D_RECT_F {
        let padding = self.app.borrow().settings().preferences.search_padding;
        rect(
            padding.left as f32,
            (self.search_height() - padding.bottom + 3) as f32,
            (width_px as f32 * 96.0 / self.dpi.get() as f32
                - (padding.left + padding.right) as f32)
                .max(1.0),
            24.0,
        )
    }

    fn delete_font(font: HFONT) {
        if !font.is_null() {
            unsafe { DeleteObject(font) };
        }
    }

    fn display_label(&self, input: &str) -> &'static str {
        if self.browser_failure.get().is_some() {
            let app = self.app.borrow();
            let prefs = &app.settings().preferences;
            return match (
                preferences::english_selected(prefs.language),
                prefs.browser.is_some(),
            ) {
                (true, true) => "Browser failed · Ctrl+Shift+Enter to use Windows default",
                (false, true) => "浏览器启动失败 · Ctrl+Shift+Enter 使用系统默认",
                (true, false) => "Could not open the browser · Try again",
                (false, false) => "无法打开浏览器，请检查设置后重试",
            };
        }
        if is_web_url(input) {
            return "Ctrl+Enter 打开链接";
        }
        ""
    }

    pub(super) fn set_input(&self, value: &str) {
        if !self.ensure_search_controls() {
            return;
        }
        unsafe { KillTimer(self.window.get(), HISTORY_TIMER) };
        if self
            .popup
            .borrow()
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::History)
        {
            self.close_popup();
        }
        self.changing_input.set(true);
        unsafe {
            SetWindowTextW(self.edit.get(), wide(value).as_ptr());
            let end = GetWindowTextLengthW(self.edit.get());
            SendMessageW(self.edit.get(), EM_SETSEL, end as usize, end as isize);
        }
        self.changing_input.set(false);
        self.layout_input(value);
    }

    pub(super) fn layout(&self) {
        if !self.edit.get().is_null() {
            self.layout_input(&text_of(self.edit.get()));
        }
    }

    fn layout_input(&self, input: &str) {
        let hwnd = self.window.get();
        let previous_hint = self.hint.replace(self.display_label(input));
        let dpi_changed = self.layout_dpi.replace(self.dpi.get()) != self.dpi.get();
        let size_changed = dpi_changed || previous_hint.is_empty() != self.hint.get().is_empty();
        unsafe {
            if size_changed {
                let height = self.scale(
                    self.search_height()
                        + if self.hint.get().is_empty() {
                            0
                        } else {
                            HINT_HEIGHT
                        },
                );
                let mut width = self.scale(self.search_width());
                let mut bounds = RECT::default();
                GetWindowRect(hwnd, &mut bounds);
                let mut left = bounds.left;
                if let Some(work) =
                    monitor_work_area(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))
                {
                    width = width.min(work.right - work.left).max(1);
                    if width != bounds.right - bounds.left {
                        left = left.clamp(work.left, work.right - width);
                    }
                }
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    left,
                    bounds.top,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                self.update_search_corners();
            }
            let mut client = RECT::default();
            GetClientRect(hwnd, &mut client);
            let dpi = self.dpi.get() as i32;
            let width = (client.right * 96 + dpi / 2) / dpi;
            let index = self.engine_index(input);
            let engine_changed = self.displayed_engine.replace(index) != index;
            let (padding, button_width) = {
                let app = self.app.borrow();
                let prefs = &app.settings().preferences;
                let name = index
                    .and_then(|index| app.engines().get(index))
                    .map(|engine| engine.name.as_str());
                (
                    prefs.search_padding,
                    engine_button_width(prefs, name, width),
                )
            };
            let width_changed = self.engine_button_width.replace(button_width) != button_width;
            if size_changed || width_changed {
                let row_height = search_row_height(self.input_font_size());
                let row_top = self.scale(padding.top).max(1);
                let row_end = padding.top + row_height;
                let row_bottom = self
                    .scale(row_end)
                    .min(self.scale(row_end + padding.bottom) - 1);
                let row_pixels = row_bottom - row_top;
                let edit_height = self.search_edit_height.get().clamp(1, row_pixels);
                let button_height = self.scale(ENGINE_BUTTON_HEIGHT).min(row_pixels);
                let edit_left = self.scale(padding.left).max(1);
                let button_right = client.right - self.scale(padding.right).max(1);
                let button_pixels = if button_width == ENGINE_BUTTON_HEIGHT {
                    button_height
                } else {
                    self.scale(button_width)
                };
                let button_left = button_right - button_pixels;
                self.search_text_dirty.set(true);
                self.edit_dirty.set(true);
                MoveWindow(
                    self.edit.get(),
                    edit_left,
                    row_top + (row_pixels - edit_height) / 2,
                    (button_left - self.scale(CONTENT_GAP) - edit_left).max(1),
                    edit_height,
                    1,
                );
                MoveWindow(
                    self.engine_button.get(),
                    button_left,
                    row_top + (row_pixels - button_height) / 2,
                    button_pixels,
                    button_height,
                    1,
                );
            }
            if engine_changed || dpi_changed || width_changed {
                self.refresh_engine_button();
            }
            if size_changed || width_changed || previous_hint != self.hint.get() {
                InvalidateRect(hwnd, null(), 0);
            }
        }
    }

    pub(super) fn update_search_corners(&self) {
        let hwnd = self.window.get();
        let mut bounds = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut bounds) } != 0 {
            let radius = self
                .app
                .borrow()
                .settings()
                .preferences
                .search_corner_radius;
            let radius = radius as f32 * self.dpi.get() as f32 / 96.0;
            super::set_window_shape(
                hwnd,
                super::smooth_corner::window_region(
                    bounds.right - bounds.left,
                    bounds.bottom - bounds.top,
                    radius,
                    self.backdrop_material.get().is_some(),
                ),
            );
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    fn ensure_search_controls(&self) -> bool {
        if self.edit.get().is_null()
            && let Err(error) = self.create_search_controls()
        {
            message(self.window.get(), &error.to_string());
            return false;
        }
        true
    }

    pub(super) fn refresh_engine_button(&self) {
        let button = self.engine_button.get();
        if button.is_null() {
            return;
        }
        let name = self
            .displayed_engine
            .get()
            .and_then(|index| {
                let app = self.app.borrow();
                app.engines()
                    .get(index)
                    .map(|e| format!("选择搜索引擎，当前为 {}", e.name))
            })
            .unwrap_or_else(|| "选择搜索引擎".to_owned());
        unsafe {
            SetWindowTextW(button, wide(&name).as_ptr());
        }
        self.repaint_engine_button();
    }

    pub(super) fn pop_engine_tooltip(&self) {
        let tooltip = self.engine_tooltip.get();
        if !tooltip.is_null() {
            unsafe { SendMessageW(tooltip, TTM_POP, 0, 0) };
        }
    }

    pub(super) fn repaint_engine_button(&self) {
        let button = self.engine_button.get();
        if !button.is_null() {
            unsafe { InvalidateRect(button, null(), 0) };
        }
        if self.composing_controls.get() {
            unsafe { InvalidateRect(self.window.get(), null(), 0) };
        }
    }

    pub(super) fn relayout_search_controls(&self) {
        self.layout_dpi.set(0);
        self.search_text_dirty.set(true);
        self.edit_dirty.set(true);
        self.layout();
    }

    pub(super) fn show(&self) {
        self.show_at(None);
    }

    pub(super) fn show_at(&self, anchor: Option<POINT>) {
        self.diagnostics.sample("search_reveal_begin");
        if !self.ensure_search_controls() {
            return;
        }
        let hwnd = self.window.get();
        unsafe {
            KillTimer(hwnd, GRAPHICS_TIMER);
            let active = GetForegroundWindow();
            if active != self.settings_window.get() && active != hwnd {
                self.launch_focus.set(active);
            }
            let revealing = IsWindowVisible(hwnd) == 0;
            if revealing {
                let mut cursor = POINT::default();
                if let Some(anchor) = anchor {
                    cursor = anchor;
                } else {
                    GetCursorPos(&mut cursor);
                }
                let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
                if let Some(work) = monitor_work_area(monitor) {
                    let mut dpi = self.dpi.get();
                    let mut vertical_dpi = dpi;
                    GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut vertical_dpi);
                    let scale = |value| super::dip(value, dpi.max(96));
                    let width = scale(self.search_width()).min(work.right - work.left);
                    let (x, y) = if let Some(anchor) = anchor {
                        (
                            anchor.x,
                            (anchor.y + scale(10)).clamp(
                                work.top,
                                (work.bottom - scale(self.search_height() + HINT_HEIGHT))
                                    .max(work.top),
                            ),
                        )
                    } else {
                        self.positions
                            .borrow()
                            .get(&(monitor as usize))
                            .copied()
                            .unwrap_or((
                                work.left + (work.right - work.left - width) / 2,
                                work.top
                                    + (work.bottom - work.top - scale(self.search_height())) / 3,
                            ))
                    };
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        x.clamp(work.left, work.right - width),
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let mut bounds = RECT::default();
                    if GetWindowRect(hwnd, &mut bounds) != 0 && bounds.right - bounds.left != width
                    {
                        self.layout_dpi.set(0);
                    }
                }
            }
            self.layout();
            self.diagnostics.sample("search_layout_done");
            self.prepare_backdrop();
            self.diagnostics.sample("search_material_done");
            // Paint the search contents before uncloaking the window.
            let cloaked = revealing && set_dwm_attribute(hwnd, DWMWA_CLOAK, 1);
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(hwnd);
            SetFocus(self.edit.get());
            self.diagnostics.sample("search_focus_done");
            self.show_screen_shade();
            if revealing {
                RedrawWindow(
                    hwnd,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW,
                );
            }
            if cloaked {
                DwmFlush();
                set_dwm_attribute(hwnd, DWMWA_CLOAK, 0);
            }
        }
        self.diagnostics.sample("search_shown");
    }

    pub(super) fn hide(&self, restore_focus: bool) {
        self.pop_engine_tooltip();
        self.close_popup();
        self.cancel_selection();
        #[cfg(feature = "ocr")]
        if self.ocr_draft.replace(false) {
            self.retry.borrow_mut().take();
            self.temporary.borrow_mut().take();
            self.set_input("");
        }
        self.diagnostics.sample("before_search_hidden");
        self.engine_hover.set(false);
        unsafe {
            self.hide_screen_shade();
            ShowWindow(self.window.get(), SW_HIDE);
            SetTimer(self.window.get(), GRAPHICS_TIMER, 1500, None);
            let previous = self.launch_focus.replace(null_mut());
            if restore_focus && IsWindow(previous) != 0 {
                SetForegroundWindow(previous);
            }
        }
    }

    fn close_search(&self) {
        if close_search(&self.app.borrow().settings().preferences) == WindowAction::StayInTray
            && self.tray.get()
        {
            self.hide(true);
        } else {
            unsafe { DestroyWindow(self.window.get()) };
        }
    }

    fn engine_index(&self, query: &str) -> Option<usize> {
        let app = self.app.borrow();
        let selected = self.temporary.borrow();
        let alias = self.accepts_alias().then(|| alias_prefix(query)).flatten();
        let aliased = alias.and_then(|(key, _)| {
            app.engines()
                .iter()
                .position(|engine| !key.is_empty() && engine.alias.eq_ignore_ascii_case(key))
        });
        aliased.or_else(|| {
            selected
                .as_deref()
                .and_then(|id| app.engines().iter().position(|engine| engine.id == id))
                .or_else(|| {
                    let default = &app.settings().preferences.default_engine;
                    app.engines()
                        .iter()
                        .position(|engine| &engine.id == default)
                })
        })
    }

    fn accepts_alias(&self) -> bool {
        #[cfg(feature = "ocr")]
        if self.ocr_draft.get() {
            return false;
        }
        self.retry.borrow().is_none()
    }

    fn on_edit(&self) {
        if self.changing_input.get() {
            return;
        }
        self.browser_failure.set(None);
        let input = text_of(self.edit.get());
        if input.is_empty()
            && self
                .retry
                .borrow()
                .as_ref()
                .is_some_and(|q| q.origin == Origin::History)
        {
            self.retry.borrow_mut().take();
            self.temporary.borrow_mut().take();
        }
        self.layout_input(&input);
        unsafe { KillTimer(self.window.get(), HISTORY_TIMER) };
        let suggestions_enabled = !self.composing.get() && self.accepts_alias();
        self.update_suggestions(suggestions_enabled.then_some(input.as_str()));
        if !self.composing.get()
            && !input.trim().is_empty()
            && alias_prefix(&input).is_none()
            && self
                .app
                .borrow()
                .settings()
                .preferences
                .history_popup
                .enabled
        {
            unsafe { SetTimer(self.window.get(), HISTORY_TIMER, 110, None) };
        }
    }

    pub(super) fn restore_query(&self, query: Query) {
        self.close_popup();
        self.browser_failure.set(None);
        #[cfg(feature = "ocr")]
        self.ocr_draft.set(false);
        *self.temporary.borrow_mut() = query.engine.clone();
        let value = editable_query_text(&query.text).into_owned();
        *self.retry.borrow_mut() = Some(query);
        self.set_input(&value);
        unsafe { SetFocus(self.edit.get()) };
    }

    fn submit(&self) {
        self.submit_using(false);
    }

    fn submit_using(&self, system: bool) {
        if self.composing.get() {
            return;
        }
        let value = text_of(self.edit.get());
        let previous = self.retry.borrow().clone();
        let origin = previous.as_ref().map_or(Origin::Typed, |q| q.origin);
        #[cfg(feature = "ocr")]
        let origin = if self.ocr_draft.get() {
            Origin::Ocr
        } else {
            origin
        };
        let text = match previous.as_ref() {
            Some(query) if value == editable_query_text(&query.text) => query.text.clone(),
            _ => value,
        };
        let query = Query {
            text,
            engine: self.temporary.borrow().clone(),
            origin,
            source: previous.map_or_else(String::new, |q| q.source),
        };
        self.search_using(query, false, system);
    }

    pub(super) fn search(&self, query: Query, reveal_failure: bool) {
        self.search_using(query, reveal_failure, false);
    }

    fn search_using(&self, query: Query, reveal_failure: bool, system: bool) {
        let outcome = if system {
            self.app.borrow_mut().search_with_system_browser(query)
        } else {
            self.app.borrow_mut().search(query)
        };
        match outcome {
            SearchOutcome::Opened { history_error } => {
                self.browser_failure.set(None);
                self.retry.borrow_mut().take();
                self.temporary.borrow_mut().take();
                self.set_input("");
                self.hide(false);
                if history_error.is_none()
                    && self.app.borrow().settings().preferences.record_history
                {
                    super::preferences::history_changed(self);
                }
            }
            SearchOutcome::Empty => {}
            SearchOutcome::Failed { query, .. } => {
                self.browser_failure.set(Some(BrowserFailure::Search));
                *self.temporary.borrow_mut() = query.engine.clone();
                let text = editable_query_text(&query.text).into_owned();
                *self.retry.borrow_mut() = Some(query);
                if text_of(self.edit.get()) == text {
                    self.layout();
                } else {
                    self.set_input(&text);
                }
                if reveal_failure {
                    self.show();
                }
            }
        }
    }

    fn open_link(&self, system: bool) {
        let text = text_of(self.edit.get());
        let outcome = if system {
            self.app.borrow_mut().open_system_link(&text)
        } else {
            self.app.borrow_mut().open_link(&text)
        };
        match outcome {
            Ok(true) => {
                self.browser_failure.set(None);
                self.retry.borrow_mut().take();
                self.temporary.borrow_mut().take();
                self.set_input("");
                self.hide(false);
            }
            Ok(false) => {}
            Err(_) => {
                self.browser_failure.set(Some(BrowserFailure::Link));
                self.layout();
            }
        }
    }

    pub(super) fn choose(&self, index: usize, kind: PopupKind) {
        let engine = self
            .app
            .borrow()
            .engines()
            .get(index)
            .map(|e| (e.id.clone(), e.alias.clone()));
        let Some((id, alias)) = engine else {
            return;
        };
        self.close_popup();
        if kind == PopupKind::Suggestions {
            self.set_input(&format!("#{alias} "));
        } else {
            let value = text_of(self.edit.get());
            if self.accepts_alias()
                && let Some((key, rest)) = alias_prefix(&value)
                && self
                    .app
                    .borrow()
                    .engines()
                    .iter()
                    .any(|e| !key.is_empty() && e.alias.eq_ignore_ascii_case(key))
            {
                self.set_input(rest);
            }
            *self.temporary.borrow_mut() = Some(id);
        }
        self.layout();
        unsafe {
            SetFocus(self.edit.get());
        }
    }

    fn paint_search(&self, dc: HDC, bounds: &RECT) {
        if self.paint_backdrop(bounds) {
            // BeginPaint clips out child controls. Clear their old pixels on the glass as well.
            unsafe {
                let window = self.window.get();
                let glass = GetDCEx(window, null_mut(), DCX_CACHE | DCX_CLIPSIBLINGS);
                if glass.is_null() {
                    FillRect(dc, bounds, GetStockObject(BLACK_BRUSH));
                } else {
                    FillRect(glass, bounds, GetStockObject(BLACK_BRUSH));
                    ReleaseDC(window, glass);
                }
            }
            return;
        }
        let hint = self.hint.get();
        let colors = self.search_colors();
        self.paint(dc, bounds, |painter| {
            painter.clear(colors.surface);
            colors.paint_border(
                painter,
                (bounds.right, bounds.bottom),
                self.dpi.get(),
                self.search_control_focused(),
                self.app
                    .borrow()
                    .settings()
                    .preferences
                    .search_corner_radius,
            );
            if !hint.is_empty() {
                painter.text(
                    hint,
                    self.search_hint_area(bounds.right),
                    if self.browser_failure.get().is_some() {
                        colors.error
                    } else {
                        colors.muted
                    },
                    Label::Hint,
                );
            }
        });
    }

    fn paint_engine_button(&self, item: &DRAWITEMSTRUCT) {
        self.paint(item.hDC, &item.rcItem, |painter| {
            painter.clear(self.search_colors().surface);
            self.draw_engine_button(
                painter,
                (0.0, 0.0),
                item.itemState & ODS_FOCUS != 0,
                item.itemState & ODS_SELECTED != 0,
                false,
            );
        });
    }

    pub(super) fn paint_composed_engine(&self, painter: &mut Painter) {
        let button = self.engine_button.get();
        if button.is_null() {
            return;
        }
        unsafe {
            let mut origin = POINT::default();
            MapWindowPoints(button, self.window.get(), &mut origin, 1);
            let dip = 96.0 / self.dpi.get() as f32;
            self.draw_engine_button(
                painter,
                (origin.x as f32 * dip, origin.y as f32 * dip),
                GetFocus() == button,
                SendMessageW(button, BM_GETSTATE, 0, 0) & BST_PUSHED as isize != 0,
                true,
            );
        }
    }

    fn draw_engine_button(
        &self,
        painter: &mut Painter,
        (x, y): (f32, f32),
        focused: bool,
        down: bool,
        composed: bool,
    ) {
        let app = self.app.borrow();
        let engine = self
            .displayed_engine
            .get()
            .and_then(|index| app.engines().get(index));
        let selected = self
            .popup
            .borrow()
            .as_ref()
            .is_some_and(|popup| popup.kind == PopupKind::Engines);
        let hover = self.engine_hover.get();
        let prefs = &app.settings().preferences;
        let colors = self.search_colors_for(prefs);
        let background = if selected || down {
            Some(colors.selected)
        } else if hover {
            Some(colors.hover)
        } else {
            None
        };
        let mut bounds = RECT::default();
        unsafe { GetClientRect(self.engine_button.get(), &mut bounds) };
        let dip = 96.0 / self.dpi.get() as f32;
        let height = bounds.bottom as f32 * dip;
        let width = bounds.right as f32 * dip;
        if let Some(background) = background {
            painter.rounded_alpha(
                rect(x + 1.0, y + 1.0, width - 2.0, height - 2.0),
                8.0,
                background,
                if composed { 0.88 } else { 1.0 },
                false,
            );
        }
        if focused {
            painter.rounded(
                rect(x + 1.5, y + 1.5, width - 3.0, height - 3.0),
                8.0,
                ACCENT,
                true,
            );
        }
        if let Some(engine) = engine {
            self.paint_search_engine_content(
                painter,
                engine,
                rect(x, y, width, height),
                self.dpi.get(),
                prefs,
            );
        }
    }

    pub(super) fn paint_search_engine_content(
        &self,
        painter: &mut Painter,
        engine: &yisou::engine::Engine,
        area: D2D_RECT_F,
        dpi: u32,
        prefs: &Preferences,
    ) {
        let size = prefs.search_engine_icon_size;
        let icon_extent = super::dip(size, dpi).max(1) as f32 * 96.0 / dpi as f32;
        let name_width = if prefs.show_search_engine_name {
            engine_name_width(&engine.name)
        } else {
            0
        };
        let gap = if name_width > 0 {
            ENGINE_NAME_GAP as f32
        } else {
            0.0
        };
        let name_left = (area.right - name_width as f32).max(area.left + icon_extent + gap);
        let icon_left = if name_width > 0 {
            name_left - gap - icon_extent
        } else {
            (area.left + area.right - icon_extent) / 2.0
        };
        self.paint_search_icon(
            painter,
            engine,
            (icon_left, (area.top + area.bottom - icon_extent) / 2.0),
            size,
            dpi,
            prefs,
        );
        if name_width > 0 && name_left < area.right {
            with_engine_name_font(|font| {
                painter.text_with_format(
                    &engine.name,
                    rect(
                        name_left,
                        area.top,
                        area.right - name_left,
                        area.bottom - area.top,
                    ),
                    self.resolve_search_colors(prefs, false).text,
                    &font.format,
                );
            });
        }
    }

    pub(super) fn use_composed_controls(&self, enabled: bool) {
        if self.composing_controls.replace(enabled) == enabled {
            return;
        }
        let edit = self.edit.get();
        let window = self.window.get();
        unsafe {
            if enabled {
                if !edit.is_null() && GetFocus() == edit {
                    self.native_caret_hidden.set(HideCaret(edit) != 0);
                    self.restart_composed_caret();
                }
            } else {
                KillTimer(window, CARET_TIMER);
                self.caret_on.set(false);
                self.edit_dirty.set(true);
                if self.native_caret_hidden.replace(false) && GetFocus() == edit {
                    ShowCaret(edit);
                }
                for child in [edit, self.engine_button.get()] {
                    if !child.is_null() {
                        InvalidateRect(child, null(), 0);
                    }
                }
            }
        }
    }

    fn restart_composed_caret(&self) {
        unsafe {
            let window = self.window.get();
            let interval = GetCaretBlinkTime();
            self.caret_on.set(true);
            KillTimer(window, CARET_TIMER);
            if !self.composing.get() && interval != 0 && interval != u32::MAX {
                SetTimer(window, CARET_TIMER, interval, None);
            }
            InvalidateRect(window, null(), 0);
        }
    }

    fn control_key(&self, hwnd: HWND, key: u16) -> bool {
        if self.composing.get() {
            return false;
        }
        let ctrl = key_down(VK_CONTROL);
        let alt = key_down(VK_MENU);
        let shift = key_down(VK_SHIFT);
        let popup = self.popup.borrow().as_ref().map(|p| p.kind);
        if ctrl && !alt {
            match key {
                VK_RETURN => {
                    if shift {
                        let Some(failure) = self.browser_failure.get() else {
                            return false;
                        };
                        if self.app.borrow().settings().preferences.browser.is_none() {
                            return false;
                        }
                        self.close_popup();
                        match failure {
                            BrowserFailure::Link => self.open_link(true),
                            BrowserFailure::Search => self.submit_using(true),
                        }
                    } else {
                        self.open_link(false);
                    }
                    return true;
                }
                VK_OEM_COMMA => {
                    preferences::show(self, 0);
                    return true;
                }
                _ => {}
            }
        }
        if alt && !ctrl && !shift {
            if key == VK_DOWN {
                self.toggle_engine_popup();
                return true;
            }
            if (b'1' as u16..=b'5' as u16).contains(&key) {
                let index = {
                    let app = self.app.borrow();
                    app.pinned_engine((key - b'1' as u16) as usize)
                        .and_then(|id| app.engines().iter().position(|e| e.id == id))
                };
                if let Some(index) = index {
                    self.choose(index, PopupKind::Engines);
                    return true;
                }
            }
            return false;
        }
        if ctrl || alt {
            return false;
        }
        if popup == Some(PopupKind::Suggestions) && matches!(key, VK_LEFT | VK_RIGHT) {
            if shift {
                return false;
            }
            if !self.focus_popup.get() {
                if key == VK_LEFT {
                    return false;
                }
                let edit = self.edit.get();
                let (selection, end) = unsafe {
                    (
                        SendMessageW(edit, EM_GETSEL, 0, 0),
                        GetWindowTextLengthW(edit) as isize,
                    )
                };
                if selection != (end | (end << 16)) {
                    return false;
                }
            }
        }
        if popup.is_some() && self.navigate_popup(key) {
            return true;
        }
        match key {
            VK_ESCAPE => {
                if popup.is_some() {
                    self.close_popup();
                    unsafe {
                        SetFocus(self.edit.get());
                    }
                } else {
                    self.close_search();
                }
            }
            VK_RETURN if hwnd == self.edit.get() || popup.is_some() => {
                if popup == Some(PopupKind::Engines) || (popup.is_some() && self.focus_popup.get())
                {
                    self.activate_popup(false);
                } else {
                    self.close_popup();
                    self.submit();
                }
            }
            VK_RETURN if hwnd == self.engine_button.get() => self.toggle_engine_popup(),
            VK_SPACE if popup == Some(PopupKind::Engines) => self.activate_popup(false),
            VK_TAB => {
                if matches!(popup, Some(PopupKind::Suggestions | PopupKind::History)) {
                    self.activate_popup(true);
                } else {
                    unsafe {
                        SetFocus(if hwnd == self.edit.get() {
                            self.engine_button.get()
                        } else {
                            self.edit.get()
                        });
                    }
                }
            }
            VK_UP if hwnd == self.edit.get() && text_of(hwnd).is_empty() => {
                let last = self.app.borrow().last_search().cloned();
                let Some(last) = last else {
                    return false;
                };
                self.restore_query(last);
            }
            _ => return false,
        }
        true
    }
}

fn create_engine_tooltip(owner: HWND, button: HWND) -> HWND {
    unsafe {
        let classes = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        if InitCommonControlsEx(&classes) == 0 {
            return null_mut();
        }
        let tooltip = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            TOOLTIPS_CLASSW,
            null(),
            WS_POPUP | TTS_NOPREFIX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            owner,
            null_mut(),
            instance(),
            null(),
        );
        if tooltip.is_null() {
            return tooltip;
        }
        let tool = TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: owner,
            uId: button as usize,
            lpszText: -1isize as *mut u16, // LPSTR_TEXTCALLBACKW
            ..Default::default()
        };
        if SendMessageW(
            tooltip,
            TTM_ADDTOOLW,
            0,
            &tool as *const TTTOOLINFOW as isize,
        ) != 0
        {
            tooltip
        } else {
            DestroyWindow(tooltip);
            null_mut()
        }
    }
}

pub(super) unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if let Some(state) = session() {
        let edit = hwnd == state.edit.get();
        let composed = state.composing_controls.get();
        match msg {
            WM_SETTEXT if edit => state.search_text_dirty.set(true),
            WM_PAINT if composed => unsafe {
                ValidateRect(hwnd, null());
                if edit {
                    state.edit_dirty.set(true);
                }
                InvalidateRect(state.window.get(), null(), 0);
                return 0;
            },
            WM_ERASEBKGND if composed => return 1,
            WM_IME_SETCONTEXT if edit && composed => unsafe {
                return DefWindowProcW(hwnd, msg, wp, lp & !(ISC_SHOWUICOMPOSITIONWINDOW as isize));
            },
            WM_IME_STARTCOMPOSITION if edit => {
                state.composing.set(true);
                state.close_popup();
                if composed {
                    state.search_text_dirty.set(true);
                    state.edit_dirty.set(true);
                    state.restart_composed_caret();
                }
            }
            WM_IME_COMPOSITION if edit && composed => {
                let result_flags =
                    GCS_RESULTSTR | GCS_RESULTCLAUSE | GCS_RESULTREADSTR | GCS_RESULTREADCLAUSE;
                let result = if lp & GCS_RESULTSTR as isize != 0 {
                    if let Some(units) = super::edit_paint::composition_result(hwnd) {
                        // Commit as one edit before IME teardown can reset the selection.
                        unsafe { SendMessageW(hwnd, EM_REPLACESEL, 1, units.as_ptr() as isize) };
                        0
                    } else {
                        unsafe { DefSubclassProc(hwnd, msg, wp, lp & result_flags as isize) }
                    }
                } else {
                    0
                };
                state.preedit.borrow_mut().update(hwnd);
                state.search_text_dirty.set(true);
                state.edit_dirty.set(true);
                state.restart_composed_caret();
                return result;
            }
            WM_IME_ENDCOMPOSITION if edit => {
                let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
                state.composing.set(false);
                state.preedit.borrow_mut().clear();
                if composed {
                    state.search_text_dirty.set(true);
                    state.edit_dirty.set(true);
                    state.restart_composed_caret();
                }
                state.on_edit();
                return result;
            }
            WM_KILLFOCUS if edit => {
                state.composing.set(false);
                state.preedit.borrow_mut().clear();
                state.search_text_dirty.set(true);
            }
            WM_KEYDOWN | WM_SYSKEYDOWN if state.control_key(hwnd, wp as u16) => {
                return 0;
            }
            WM_CHAR
                if !state.composing.get()
                    && matches!(wp as u16, VK_RETURN | VK_TAB | VK_ESCAPE) =>
            {
                return 0;
            }
            WM_MOUSEMOVE
                if hwnd == state.engine_button.get() && !state.engine_hover.replace(true) =>
            unsafe {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: HOVER_DEFAULT,
                };
                TrackMouseEvent(&mut track);
                state.repaint_engine_button();
            },
            WM_MOUSELEAVE if hwnd == state.engine_button.get() => {
                state.engine_hover.set(false);
                state.repaint_engine_button();
            }
            _ => {}
        }
        let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
        if !composed && matches!(msg, WM_SETFOCUS | WM_KILLFOCUS) {
            unsafe { InvalidateRect(state.window.get(), null(), 0) };
        }
        #[cfg(feature = "text-render-lab")]
        if msg == WM_PAINT && edit && !composed && super::text_render_lab::solid_window() {
            super::text_render_lab::record_draw(&state, super::text_render_lab::Mode::Solid);
        }
        if composed {
            unsafe {
                match msg {
                    WM_SETFOCUS if edit => {
                        state.native_caret_hidden.set(HideCaret(hwnd) != 0);
                        state.edit_dirty.set(true);
                        state.restart_composed_caret();
                    }
                    WM_KILLFOCUS if edit => {
                        state.native_caret_hidden.set(false);
                        state.caret_on.set(false);
                        KillTimer(state.window.get(), CARET_TIMER);
                        state.show_composed_caret(false);
                        state.edit_dirty.set(true);
                        InvalidateRect(state.window.get(), null(), 0);
                    }
                    WM_MOUSEMOVE if edit && GetCapture() != hwnd => {}
                    WM_KEYDOWN | WM_CHAR | WM_SETTEXT | EM_SETSEL | WM_LBUTTONDOWN
                    | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_MOUSEMOVE | WM_CUT | WM_PASTE
                    | WM_CLEAR | WM_UNDO
                        if edit =>
                    {
                        state.edit_dirty.set(true);
                        if GetFocus() == hwnd {
                            state.restart_composed_caret();
                        } else {
                            InvalidateRect(state.window.get(), null(), 0);
                        }
                    }
                    WM_SETFOCUS | WM_KILLFOCUS | WM_LBUTTONDOWN | WM_LBUTTONUP | BM_SETSTATE
                    | WM_CAPTURECHANGED
                        if !edit =>
                    {
                        InvalidateRect(state.window.get(), null(), 0);
                    }
                    _ => {}
                }
            }
        }
        return result;
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

pub(super) unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    let Some(state) = session() else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    if msg == state.taskbar_created && msg != 0 {
        state.add_tray();
        return 0;
    }
    match msg {
        #[cfg(feature = "text-render-lab")]
        super::text_render_lab::SET_SAMPLE => super::text_render_lab::set_sample(&state, wp, lp),
        #[cfg(feature = "text-render-lab")]
        super::text_render_lab::ACTIVE_RENDERER => super::text_render_lab::active_renderer(&state),
        #[cfg(feature = "text-render-lab")]
        super::text_render_lab::VERIFY_FONT => super::text_render_lab::verify_font(&state),
        WM_PAINT => unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut bounds = RECT::default();
            GetClientRect(hwnd, &mut bounds);
            state.paint_search(dc, &bounds);
            EndPaint(hwnd, &paint);
            0
        },
        WM_DRAWITEM if wp == ENGINE && lp != 0 => {
            if !state.composing_controls.get() {
                state.paint_engine_button(unsafe { &*(lp as *const DRAWITEMSTRUCT) });
            }
            1
        }
        WM_CTLCOLOREDIT => unsafe {
            let dc = wp as HDC;
            if state.capture_dc.get() == dc {
                SetTextColor(dc, color_ref(super::edit_paint::capture_text_color(&state)));
                SetBkColor(dc, color_ref(0xffffff));
                return GetStockObject(WHITE_BRUSH) as isize;
            }
            let colors = state.search_colors();
            let opacity = state
                .app
                .borrow()
                .settings()
                .preferences
                .search_text_opacity();
            SetTextColor(dc, color_ref(blend(colors.surface, colors.text, opacity)));
            if state.composing_controls.get() {
                SetBkMode(dc, TRANSPARENT as i32);
                return GetStockObject(NULL_BRUSH) as isize;
            }
            SetBkColor(dc, color_ref(colors.surface));
            SetDCBrushColor(dc, color_ref(colors.surface));
            GetStockObject(DC_BRUSH) as isize
        },
        WM_ERASEBKGND => 1,
        WM_NCCALCSIZE => 0,
        WM_DWMCOMPOSITIONCHANGED | WM_SETTINGCHANGE => {
            if msg == WM_DWMCOMPOSITIONCHANGED {
                state.release_screen_shade();
            } else {
                let dark = !super::personalization_flag(w!("AppsUseLightTheme"), true);
                if state.system_dark.replace(dark) != dark {
                    let settings = state.settings_window.get();
                    if !settings.is_null() {
                        unsafe { InvalidateRect(settings, null(), 0) };
                    }
                }
            }
            state.refresh_search_appearance();
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                state.show_screen_shade();
            }
            0
        }
        WM_NOTIFY if lp != 0 => unsafe {
            let notice = &*(lp as *const NMHDR);
            if notice.hwndFrom == state.engine_tooltip.get() && notice.code == TTN_GETDISPINFOW {
                let info = &mut *(lp as *mut NMTTDISPINFOW);
                let menu_open = state
                    .popup
                    .borrow()
                    .as_ref()
                    .is_some_and(|popup| popup.kind == PopupKind::Engines);
                if !menu_open {
                    let name = text_of(state.engine_button.get());
                    let mut offset = 0;
                    for ch in name.chars() {
                        let end = offset + ch.len_utf16();
                        if end >= info.szText.len() {
                            break;
                        }
                        ch.encode_utf16(&mut info.szText[offset..end]);
                        offset = end;
                    }
                    info.szText[offset] = 0;
                    info.lpszText = info.szText.as_mut_ptr();
                }
                0
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        },
        WM_COMMAND => match wp & 0xffff {
            ENGINE => {
                state.toggle_engine_popup();
                unsafe { InvalidateRect(state.engine_button.get(), null(), 0) };
                0
            }
            100 if (wp >> 16) as u32 == EN_CHANGE => {
                state.search_text_dirty.set(true);
                state.edit_dirty.set(true);
                if state.composing_controls.get() {
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                state.on_edit();
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        },
        WM_NCHITTEST => HTCAPTION as isize,
        WM_MOUSEWHEEL if !state.popup_window.get().is_null() => {
            let popup = state.popup_window.get();
            let mut bounds = RECT::default();
            let x = lp as i16 as i32;
            let y = (lp >> 16) as i16 as i32;
            unsafe {
                if GetWindowRect(popup, &mut bounds) != 0
                    && (bounds.left..bounds.right).contains(&x)
                    && (bounds.top..bounds.bottom).contains(&y)
                {
                    return SendMessageW(popup, msg, wp, lp);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                state.hide(false);
            }
            0
        }
        WM_DISPLAYCHANGE => {
            state.release_screen_shade();
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                state.show_screen_shade();
            }
            0
        }
        WM_ENTERSIZEMOVE => {
            state.close_popup();
            0
        }
        WM_EXITSIZEMOVE => unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) as usize;
            let mut bounds = RECT::default();
            if GetWindowRect(hwnd, &mut bounds) != 0 {
                state
                    .positions
                    .borrow_mut()
                    .insert(monitor, (bounds.left, bounds.top));
            }
            0
        },
        WM_DPICHANGED if lp != 0 => unsafe {
            state.close_popup();
            state.dpi.set((wp & 0xffff) as u32);
            let bounds = &*(lp as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            state.apply_font();
            state.update_search_corners();
            0
        },
        TRAY => {
            match lp as u32 {
                WM_LBUTTONUP => state.show(),
                WM_RBUTTONUP | WM_CONTEXTMENU => state.tray_menu(),
                windows_sys::Win32::UI::Shell::NIN_BALLOONUSERCLICK => state.retry_image_search(),
                _ => {}
            }
            0
        }
        WM_HOTKEY | DOUBLE_ALT => {
            let role = if msg == DOUBLE_ALT {
                Some(Hotkey::Main)
            } else {
                state
                    .hotkeys
                    .borrow()
                    .as_ref()
                    .and_then(|hotkeys| hotkeys.role(wp))
            };
            if let Some(role) = role {
                let shortcut = {
                    let app = state.app.borrow();
                    let prefs = &app.settings().preferences;
                    match role {
                        Hotkey::Main => prefs.main_shortcut,
                        Hotkey::Selection => prefs.selection.as_ref().map(|b| b.shortcut),
                        Hotkey::Clipboard => prefs.clipboard_shortcut,
                        Hotkey::ImageSearch => prefs.image_search_shortcut,
                        #[cfg(feature = "ocr")]
                        Hotkey::Ocr => prefs.ocr_shortcut,
                    }
                };
                if !shortcut.is_some_and(|s| preferences::capture_registered(&state, s)) {
                    state.hotkey(role, msg == DOUBLE_ALT);
                }
            }
            0
        }
        REVEAL => {
            match wp {
                1 => preferences::show(&state, 0),
                2 => preferences::show(&state, 6),
                3 => {
                    state.show();
                    state.toggle_engine_popup();
                }
                _ => state.show(),
            }
            0
        }
        SELECTION_READY => {
            state.poll_selection(false);
            0
        }
        super::screenshot::START_IMAGE_SEARCH => {
            state.start_image_search();
            0
        }
        super::screenshot::IMAGE_SEARCH_READY => {
            state.finish_image_search();
            0
        }
        #[cfg(feature = "ocr")]
        super::screenshot::START => {
            state.start_ocr();
            0
        }
        #[cfg(feature = "ocr")]
        super::screenshot::READY => {
            state.finish_ocr();
            0
        }
        WM_TIMER if wp == SELECTION_TIMER => {
            state.poll_selection(true);
            0
        }
        WM_TIMER if wp == CARET_TIMER => {
            if !state.composing.get() && unsafe { GetFocus() } == state.edit.get() {
                let visible = !state.caret_on.get();
                state.caret_on.set(visible);
                state.show_composed_caret(visible);
            }
            0
        }
        WM_TIMER if wp == HISTORY_TIMER => unsafe {
            KillTimer(hwnd, HISTORY_TIMER);
            if !state.composing.get()
                && IsWindowVisible(hwnd) != 0
                && GetFocus() == state.edit.get()
            {
                let input = text_of(state.edit.get());
                if alias_prefix(&input).is_none() {
                    state.show_history_suggestions(&input);
                }
            }
            0
        },
        WM_TIMER if wp == GRAPHICS_TIMER => unsafe {
            KillTimer(hwnd, GRAPHICS_TIMER);
            if IsWindowVisible(hwnd) == 0 {
                state.diagnostics.sample("before_idle_release");
                state.painter.borrow_mut().take();
                state.release_backdrop();
                state.composition_device.borrow_mut().take();
                state.release_screen_shade();
                if state.settings_window.get().is_null() {
                    state.app.borrow_mut().history().release_cache();
                }
                state.diagnostics.sample("after_idle_release");
            }
            0
        },
        WM_CLOSE => {
            state.close_search();
            0
        }
        WM_DESTROY => unsafe {
            state.end_screenshot(false);
            state.release_screen_shade();
            state.release_backdrop();
            state.close_popup();
            let settings = state.settings_window.get();
            if !settings.is_null() {
                DestroyWindow(settings);
            }
            state.cancel_selection();
            state.hotkeys.borrow_mut().take();
            state.remove_tray();
            PostQuitMessage(0);
            0
        },
        WM_NCDESTROY => unsafe {
            State::delete_font(state.font.replace(null_mut()));
            DefWindowProcW(hwnd, msg, wp, lp)
        },
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
