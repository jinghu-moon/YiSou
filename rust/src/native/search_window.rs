use super::{
    REVEAL, State, TITLE, color_ref,
    edit_paint::CARET_TIMER,
    instance, message,
    paint::{ACCENT, BORDER, HOVER, Label, MUTED, Painter, SELECTED, SURFACE, TEXT, rect},
    popup::PopupKind,
    preferences,
    selection::{DOUBLE_ALT, SELECTION_READY, SELECTION_TIMER},
    session, text_of,
    tray::TRAY,
    wide,
};
use std::{
    io,
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Controls::{
                DRAWITEMSTRUCT, EM_SETCUEBANNER, EM_SETLIMITTEXT, EM_SETSEL, HOVER_DEFAULT,
                ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, NMTTDISPINFOW,
                ODS_FOCUS, ODS_SELECTED, TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW,
                TTM_POP, TTN_GETDISPINFOW, TTS_NOPREFIX, TTTOOLINFOW, WM_MOUSELEAVE,
            },
            HiDpi::GetDpiForWindow,
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
    settings::Font,
};

pub(super) const MAIN_CLASS: *const u16 = w!("YiSou.Rust.Search");
const WIDTH: i32 = 580;
const HEIGHT: i32 = 56;
const ENGINE: usize = 101;
const ENGINE_BUTTON_SIZE: i32 = 40;
const INPUT_FONT_SIZE: i32 = 20;
const INPUT_HEIGHT: i32 = 32;
const INPUT_TOP: i32 = 17;
const ENGINE_BUTTON_X: i32 = WIDTH - 12 - ENGINE_BUTTON_SIZE;
const ENGINE_BUTTON_Y: i32 = (HEIGHT - ENGINE_BUTTON_SIZE) / 2;
const INPUT_LEFT: i32 = 20;
const INPUT_RIGHT: i32 = ENGINE_BUTTON_X - 12;
const GRAPHICS_TIMER: usize = 2;

pub(super) fn key_down(key: u16) -> bool {
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

impl State {
    pub(super) fn create_search_window(&self) -> io::Result<HWND> {
        let styles = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP;
        #[cfg(feature = "text-render-lab")]
        let styles = if super::text_render_lab::solid_window() {
            styles & !WS_EX_NOREDIRECTIONBITMAP
        } else {
            styles
        };
        let hwnd = unsafe {
            CreateWindowExW(
                // Native Edit may also paint synchronously outside WM_PAINT.
                styles,
                MAIN_CLASS,
                TITLE,
                WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                WIDTH,
                HEIGHT,
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
            unsafe { SendMessageW(self.edit.get(), WM_SETFONT, font as usize, 1) };
            Self::delete_font(self.font.replace(font));
        }
        *self.painter.borrow_mut() = None;
        self.release_backdrop();
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
        INPUT_FONT_SIZE
    }

    pub(super) fn create_font_at(&self, dpi: u32, size: i32) -> HFONT {
        let family = self.font_family();
        unsafe {
            CreateFontW(
                -super::dip(size, dpi),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide(&family).as_ptr(),
            )
        }
    }

    fn delete_font(font: HFONT) {
        if !font.is_null() {
            unsafe { DeleteObject(font) };
        }
    }

    fn display_label(&self, input: &str) -> &'static str {
        if self.error.get() {
            return "无法打开浏览器，请检查设置后重试";
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
        let index = self.engine_index(input);
        let engine_changed = self.displayed_engine.replace(index) != index;
        unsafe {
            if size_changed {
                let height = self.scale(if self.hint.get().is_empty() {
                    HEIGHT
                } else {
                    83
                });
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    0,
                    0,
                    self.scale(WIDTH),
                    height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            if dpi_changed {
                MoveWindow(
                    self.edit.get(),
                    self.scale(INPUT_LEFT),
                    self.scale(INPUT_TOP + (INPUT_FONT_SIZE - self.input_font_size()) / 2),
                    self.scale(INPUT_RIGHT) - self.scale(INPUT_LEFT),
                    self.scale(INPUT_HEIGHT),
                    1,
                );
                MoveWindow(
                    self.engine_button.get(),
                    self.scale(ENGINE_BUTTON_X),
                    self.scale(ENGINE_BUTTON_Y),
                    self.scale(ENGINE_BUTTON_SIZE),
                    self.scale(ENGINE_BUTTON_SIZE),
                    1,
                );
            }
            if engine_changed || dpi_changed {
                self.refresh_engine_button();
            }
            if size_changed || previous_hint != self.hint.get() {
                InvalidateRect(hwnd, null(), 0);
            }
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

    pub(super) fn show(&self) {
        self.show_at(None);
    }

    pub(super) fn show_at(&self, anchor: Option<POINT>) {
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
            if IsWindowVisible(hwnd) == 0 {
                let mut cursor = POINT::default();
                if let Some(anchor) = anchor {
                    cursor = anchor;
                } else {
                    GetCursorPos(&mut cursor);
                }
                let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut info) != 0 {
                    let work = info.rcWork;
                    let (x, y) = if let Some(anchor) = anchor {
                        (
                            anchor
                                .x
                                .clamp(work.left, (work.right - self.scale(WIDTH)).max(work.left)),
                            (anchor.y + self.scale(10))
                                .clamp(work.top, (work.bottom - self.scale(83)).max(work.top)),
                        )
                    } else {
                        self.positions
                            .borrow()
                            .get(&(monitor as usize))
                            .copied()
                            .unwrap_or((
                                work.left + (work.right - work.left - self.scale(WIDTH)) / 2,
                                work.top + (work.bottom - work.top - self.scale(HEIGHT)) / 3,
                            ))
                    };
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        x,
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            self.layout();
            self.prepare_backdrop();
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(hwnd);
            SetFocus(self.edit.get());
            self.show_screen_shade();
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
            let id = selected
                .as_deref()
                .unwrap_or(&app.settings().preferences.default_engine);
            app.engines().iter().position(|engine| engine.id == id)
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
        self.error.set(false);
        let input = text_of(self.edit.get());
        self.layout_input(&input);
        let suggestions_enabled = !self.composing.get() && self.accepts_alias();
        self.update_suggestions(suggestions_enabled.then_some(input.as_str()));
    }

    fn submit(&self) {
        let value = text_of(self.edit.get());
        let previous = self.retry.borrow().clone();
        let origin = previous.as_ref().map_or(Origin::Typed, |q| q.origin);
        #[cfg(feature = "ocr")]
        let origin = if self.ocr_draft.get() {
            Origin::Ocr
        } else {
            origin
        };
        let query = Query {
            text: value,
            engine: self.temporary.borrow().clone(),
            origin,
            source: previous.map_or_else(String::new, |q| q.source),
        };
        self.search(query, false);
    }

    pub(super) fn search(&self, query: Query, reveal_failure: bool) {
        let outcome = self.app.borrow_mut().search(query);
        match outcome {
            SearchOutcome::Opened { .. } => {
                self.error.set(false);
                self.retry.borrow_mut().take();
                self.temporary.borrow_mut().take();
                self.set_input("");
                self.hide(false);
            }
            SearchOutcome::Empty => {}
            SearchOutcome::Failed { query, .. } => {
                self.error.set(true);
                *self.temporary.borrow_mut() = query.engine.clone();
                let text = query.text.clone();
                *self.retry.borrow_mut() = Some(query);
                self.set_input(&text);
                if reveal_failure {
                    self.show();
                }
            }
        }
    }

    fn open_link(&self) {
        let outcome = self.app.borrow_mut().open_link(&text_of(self.edit.get()));
        match outcome {
            Ok(true) => {
                self.error.set(false);
                self.retry.borrow_mut().take();
                self.temporary.borrow_mut().take();
                self.set_input("");
                self.hide(false);
            }
            Ok(false) => {}
            Err(_) => {
                self.error.set(true);
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
            unsafe { FillRect(dc, bounds, GetStockObject(BLACK_BRUSH)) };
            return;
        }
        let hint = self.hint.get();
        let dpi = self.dpi.get() as f32;
        let width = bounds.right as f32 * 96.0 / dpi;
        let height = bounds.bottom as f32 * 96.0 / dpi;
        self.paint(dc, bounds, |painter| {
            painter.clear(SURFACE);
            painter.rounded(rect(0.5, 0.5, width - 1.0, height - 1.0), 8.0, BORDER, true);
            if !hint.is_empty() {
                painter.text(
                    hint,
                    rect(20.0, 52.0, 540.0, 24.0),
                    if self.error.get() { 0x9d3026 } else { MUTED },
                    Label::Hint,
                );
            }
        });
    }

    fn paint_engine_button(&self, item: &DRAWITEMSTRUCT) {
        self.paint(item.hDC, &item.rcItem, |painter| {
            painter.clear(SURFACE);
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
        let background = if selected || down {
            Some(SELECTED)
        } else if hover {
            Some(HOVER)
        } else {
            None
        };
        let dip = 96.0 / self.dpi.get() as f32;
        let extent = self.scale(ENGINE_BUTTON_SIZE) as f32 * dip;
        if let Some(background) = background {
            painter.rounded_alpha(
                rect(x + 1.0, y + 1.0, extent - 2.0, extent - 2.0),
                8.0,
                background,
                if composed { 0.88 } else { 1.0 },
                false,
            );
        }
        if focused {
            painter.rounded(
                rect(x + 1.5, y + 1.5, extent - 3.0, extent - 3.0),
                8.0,
                ACCENT,
                true,
            );
        }
        if let Some(engine) = engine {
            let size = self.input_font_size();
            let inset = (self.scale(ENGINE_BUTTON_SIZE) - self.scale(size)) / 2;
            let inset = inset as f32 * dip;
            self.paint_engine(painter, engine, x + inset, y + inset, size);
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
                self.preedit.borrow_mut().clear();
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
            if interval != 0 && interval != u32::MAX {
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
                    self.open_link();
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
                if popup == Some(PopupKind::Engines)
                    || (popup == Some(PopupKind::Suggestions) && self.focus_popup.get())
                {
                    self.activate_popup();
                } else {
                    self.submit();
                }
            }
            VK_RETURN if hwnd == self.engine_button.get() => self.toggle_engine_popup(),
            VK_SPACE if popup == Some(PopupKind::Engines) => self.activate_popup(),
            VK_TAB => {
                if popup == Some(PopupKind::Suggestions) {
                    self.activate_popup();
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
                let last = self.app.borrow().last_query().to_owned();
                if last.is_empty() {
                    return false;
                }
                self.retry.borrow_mut().take();
                self.set_input(&last);
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
            WM_IME_STARTCOMPOSITION => {
                state.composing.set(true);
                state.close_popup();
                if composed {
                    state.edit_dirty.set(true);
                    state.restart_composed_caret();
                }
            }
            WM_IME_COMPOSITION if edit && composed => {
                let result_flags =
                    GCS_RESULTSTR | GCS_RESULTCLAUSE | GCS_RESULTREADSTR | GCS_RESULTREADCLAUSE;
                let result = if lp & GCS_RESULTSTR as isize != 0 {
                    unsafe { DefSubclassProc(hwnd, msg, wp, lp & result_flags as isize) }
                } else {
                    0
                };
                state.preedit.borrow_mut().update(hwnd);
                state.edit_dirty.set(true);
                state.restart_composed_caret();
                return result;
            }
            WM_IME_ENDCOMPOSITION => {
                let result = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
                state.composing.set(false);
                state.preedit.borrow_mut().clear();
                if composed {
                    state.edit_dirty.set(true);
                    state.restart_composed_caret();
                }
                state.on_edit();
                return result;
            }
            WM_KILLFOCUS if edit => {
                state.composing.set(false);
                state.preedit.borrow_mut().clear();
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
            SetTextColor(dc, color_ref(TEXT));
            if state.capture_dc.get() == dc {
                SetBkColor(dc, color_ref(0xffffff));
                return GetStockObject(WHITE_BRUSH) as isize;
            }
            SetBkColor(dc, color_ref(SURFACE));
            state.background as isize
        },
        WM_ERASEBKGND => 1,
        WM_NCCALCSIZE => 0,
        WM_DWMCOMPOSITIONCHANGED | WM_SETTINGCHANGE => {
            state.release_backdrop();
            state.backdrop_unavailable.set(false);
            unsafe { InvalidateRect(hwnd, null(), 0) };
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
        WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                state.hide(false);
            }
            0
        }
        WM_DISPLAYCHANGE => {
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
            state.apply_font();
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
            state.layout();
            0
        },
        TRAY => {
            match lp as u32 {
                WM_LBUTTONUP => state.show(),
                WM_RBUTTONUP | WM_CONTEXTMENU => state.tray_menu(),
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
                        Hotkey::Main => Some(prefs.main_shortcut),
                        Hotkey::Selection => prefs.selection.as_ref().map(|b| b.shortcut),
                        Hotkey::Clipboard => prefs.clipboard_shortcut,
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
        #[cfg(feature = "ocr")]
        super::ocr::START => {
            state.start_ocr();
            0
        }
        #[cfg(feature = "ocr")]
        super::ocr::READY => {
            state.finish_ocr();
            0
        }
        WM_TIMER if wp == SELECTION_TIMER => {
            state.poll_selection(true);
            0
        }
        WM_TIMER if wp == CARET_TIMER => unsafe {
            state.caret_on.set(!state.caret_on.get());
            InvalidateRect(hwnd, null(), 0);
            0
        },
        WM_TIMER if wp == GRAPHICS_TIMER => unsafe {
            KillTimer(hwnd, GRAPHICS_TIMER);
            if IsWindowVisible(hwnd) == 0 {
                state.diagnostics.sample("before_idle_release");
                state.painter.borrow_mut().take();
                state.release_backdrop();
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
            #[cfg(feature = "ocr")]
            state.end_ocr(false);
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
            DeleteObject(state.background);
            DefWindowProcW(hwnd, msg, wp, lp)
        },
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
