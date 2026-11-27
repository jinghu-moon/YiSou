mod history;

use super::{
    State, instance,
    paint::{ACCENT, BORDER, HOVER, Label, MUTED, RAIL, SURFACE, TEXT, blend, rect},
    preferences,
    search_window::key_down,
    session,
};
use history::HistoryPanel;
use std::ptr::{null, null_mut};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Controls::{HOVER_DEFAULT, WM_MOUSELEAVE},
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    w,
};
use yisou::application::Record;

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.Engines");
const MAX_ENGINE_GRID_ROWS: usize = 3;
const MAX_SUGGESTION_ROWS: usize = 6;
const SUGGESTION_ROW_HEIGHT: i32 = 32;
const MIN_SUGGESTION_COLUMN_WIDTH: i32 = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PopupKind {
    Engines,
    Suggestions,
    History,
}

#[derive(Clone)]
enum Item {
    Engine(usize),
    AddEngine,
    History(Record),
}

pub(super) struct Popup {
    pub(super) kind: PopupKind,
    items: Vec<Item>,
    focus: usize,
    hover: Option<usize>,
    first: usize,
    columns: usize,
    rows: usize,
    width: i32,
    height: i32,
    tile_width: i32,
    tile_height: i32,
    names: bool,
    history: Option<HistoryPanel>,
}

const GAP: i32 = 6;
const INSET: i32 = 10;

impl State {
    pub(super) fn invalidate_following_history_background(&self) {
        let follows = self
            .app
            .borrow()
            .settings()
            .preferences
            .history_popup
            .material
            == yisou::settings::HistoryMaterial::FollowSearch;
        if follows
            && self
                .popup
                .borrow()
                .as_ref()
                .is_some_and(|popup| popup.kind == PopupKind::History)
        {
            unsafe { InvalidateRect(self.popup_window.get(), null(), 0) };
        }
    }

    pub(super) fn dismiss_history_popup(&self) {
        unsafe { KillTimer(self.window.get(), super::search_window::HISTORY_TIMER) };
        if self
            .popup
            .borrow()
            .as_ref()
            .is_some_and(|popup| popup.kind == PopupKind::History)
        {
            self.close_popup();
        }
    }

    pub(super) fn update_suggestions(&self, input: Option<&str>) {
        let items = input
            .map(|input| {
                let app = self.app.borrow();
                app.suggestions(input)
                    .into_iter()
                    .filter(|engine| !engine.alias.is_empty())
                    .filter_map(|engine| {
                        app.engines()
                            .iter()
                            .position(|item| item.id == engine.id)
                            .map(Item::Engine)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if items.is_empty() {
            let suggestions_open = self
                .popup
                .borrow()
                .as_ref()
                .is_some_and(|p| p.kind != PopupKind::Engines);
            if suggestions_open {
                self.close_popup();
            }
        } else {
            self.show_popup(PopupKind::Suggestions, items);
        }
    }

    pub(super) fn show_history_suggestions(&self, input: &str) {
        let (enabled, recording) = {
            let app = self.app.borrow();
            let prefs = &app.settings().preferences;
            (prefs.history_popup.enabled, prefs.record_history)
        };
        if !enabled || input.trim().is_empty() || self.popup.borrow().is_some() {
            return;
        }
        let records = self
            .app
            .borrow_mut()
            .history_suggestions(input)
            .unwrap_or_default();
        if recording || !records.is_empty() {
            self.open_popup(
                PopupKind::History,
                records.into_iter().map(Item::History).collect(),
                input,
            );
        }
    }

    pub(super) fn toggle_engine_popup(&self) {
        if self.edit.get().is_null() {
            return;
        }
        if self
            .popup
            .borrow()
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::Engines)
        {
            self.close_popup();
            return;
        }
        let count = self.app.borrow().engines().len();
        self.show_popup(
            PopupKind::Engines,
            (0..count)
                .map(Item::Engine)
                .chain([Item::AddEngine])
                .collect(),
        );
    }

    fn show_popup(&self, kind: PopupKind, items: Vec<Item>) {
        self.open_popup(kind, items, "");
    }

    fn open_popup(&self, kind: PopupKind, items: Vec<Item>, keyword: &str) {
        let is_history = kind == PopupKind::History;
        let is_suggestions = kind == PopupKind::Suggestions;
        if self
            .popup
            .borrow()
            .as_ref()
            .is_some_and(|popup| (popup.kind == PopupKind::History) != is_history)
        {
            self.close_popup();
        }
        let app = self.app.borrow();
        let prefs = &app.settings().preferences;
        let names = prefs.show_engine_names;
        let english = preferences::english_selected(prefs.language);
        let show_shortcuts = prefs.history_popup.show_shortcuts;
        let corner_radius = prefs.history_popup.corner_radius;
        let glass_frame = is_history
            && (corner_radius > 0
                || prefs.history_background(self.system_dark.get()).material
                    != yisou::settings::SearchMaterial::Solid);
        drop(app);
        let (mut tile_width, tile_height) = match (kind, names) {
            (PopupKind::Suggestions, _) => (0, SUGGESTION_ROW_HEIGHT),
            (PopupKind::Engines, true) => (86, 90),
            _ => (64, 64),
        };
        let hwnd = self.window.get();
        let mut bounds = RECT::default();
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetWindowRect(hwnd, &mut bounds);
            GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
        }
        let work = info.rcWork;
        let dpi = self.dpi.get() as i32;
        let anchor_pixels = (bounds.right - bounds.left).min(work.right - work.left);
        let anchor_width = (anchor_pixels * 96 + dpi / 2) / dpi;
        let columns = match kind {
            PopupKind::Suggestions
                if items.len() > 1
                    && anchor_width >= 2 * (INSET + MIN_SUGGESTION_COLUMN_WIDTH) + GAP =>
            {
                2
            }
            PopupKind::Suggestions | PopupKind::History => 1,
            PopupKind::Engines => ((work.right - work.left - self.scale(INSET * 2)
                + self.scale(GAP))
                / self.scale(tile_width + GAP))
            .clamp(1, 6) as usize,
        };
        if is_suggestions {
            tile_width =
                ((anchor_width - INSET * 2 - GAP * (columns as i32 - 1)) / columns as i32).max(1);
        }
        let gap = self.scale(if is_history { 8 } else { GAP });
        let space = (work.bottom - bounds.bottom).max(bounds.top - work.top) - gap;
        let max_rows = if is_suggestions {
            let space_dip = space * 96 / dpi;
            ((space_dip - 2 * INSET + GAP) / (tile_height + GAP))
                .clamp(1, MAX_SUGGESTION_ROWS as i32) as usize
        } else {
            MAX_ENGINE_GRID_ROWS
        };
        let rows = items.len().div_ceil(columns).min(max_rows);
        let mut width = tile_width * columns as i32 + GAP * (columns as i32 - 1) + INSET * 2;
        let mut height = tile_height * rows as i32 + GAP * (rows as i32 - 1) + INSET * 2;
        if is_suggestions {
            width = anchor_width;
        }
        let history = (kind == PopupKind::History).then(|| {
            let dips = |pixels| (pixels * 96 + dpi / 2) / dpi;
            width = anchor_width.max(1);
            let mut painter = self.painter.borrow_mut();
            if painter.is_none() {
                let (family, collection) = self.drawing_font();
                *painter = super::paint::Painter::new(&family, collection).ok();
            }
            let panel = HistoryPanel::new(
                &items,
                width,
                dips(space),
                keyword,
                english,
                show_shortcuts,
                painter.as_mut(),
            );
            height = panel.height();
            panel
        });
        let focus = if kind == PopupKind::Engines {
            self.displayed_engine.get().unwrap_or(0)
        } else {
            0
        };
        let first = (focus / columns).saturating_sub(rows.saturating_sub(1)) * columns;
        *self.popup.borrow_mut() = Some(Popup {
            kind,
            items,
            first,
            focus,
            hover: None,
            columns,
            rows,
            width,
            height,
            tile_width,
            tile_height,
            names,
            history,
        });
        self.focus_popup.set(false);
        let mut popup = self.popup_window.get();
        if popup.is_null() {
            popup = unsafe {
                CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                    CLASS,
                    w!("搜索引擎"),
                    WS_POPUP | if glass_frame { WS_THICKFRAME } else { 0 },
                    0,
                    0,
                    0,
                    0,
                    hwnd,
                    null_mut(),
                    instance(),
                    null(),
                )
            };
            if popup.is_null() {
                self.close_popup();
                return;
            }
            self.dwm(popup, 2);
            self.popup_window.set(popup);
        }
        let composed = self
            .popup
            .borrow()
            .as_ref()
            .and_then(|popup| popup.history.as_ref())
            .is_some_and(|history| history.apply_material(self, popup));
        let pixels_w = if is_suggestions {
            anchor_pixels.max(1)
        } else {
            self.scale(width)
        };
        let pixels_h = self.scale(height);
        let left = match kind {
            PopupKind::Engines => bounds.right - self.scale(12) - pixels_w,
            PopupKind::Suggestions | PopupKind::History => bounds.left,
        };
        let top = bounds.bottom + gap;
        let x = left.clamp(work.left, (work.right - pixels_w).max(work.left));
        let y = if top + pixels_h <= work.bottom {
            top
        } else {
            (bounds.top - pixels_h - gap).max(work.top)
        };
        unsafe {
            let title = match (kind, english) {
                (PopupKind::History, true) => w!("Search history"),
                (PopupKind::History, false) => w!("搜索历史"),
                (_, true) => w!("Search engines"),
                _ => w!("搜索引擎"),
            };
            SetWindowTextW(popup, title);
            SetWindowPos(
                popup,
                HWND_TOPMOST,
                x,
                y,
                pixels_w,
                pixels_h,
                SWP_NOACTIVATE | if is_history { 0 } else { SWP_SHOWWINDOW },
            );
            if is_history {
                let radius = corner_radius as f32 * self.dpi.get() as f32 / 96.0;
                super::set_window_shape(
                    popup,
                    super::smooth_corner::window_region(pixels_w, pixels_h, radius, composed),
                );
                ShowWindow(popup, SW_SHOWNOACTIVATE);
            }
            InvalidateRect(popup, null(), 0);
        }
        if kind == PopupKind::Engines {
            self.pop_engine_tooltip();
            self.repaint_engine_button();
            self.diagnostics.sample("engine_popup_shown");
        }
    }

    pub(super) fn close_popup(&self) {
        unsafe { KillTimer(self.window.get(), super::search_window::HISTORY_TIMER) };
        let was_engine_popup = self
            .popup
            .borrow_mut()
            .take()
            .is_some_and(|popup| popup.kind == PopupKind::Engines);
        self.focus_popup.set(false);
        let hwnd = self.popup_window.replace(null_mut());
        if !hwnd.is_null() {
            unsafe { DestroyWindow(hwnd) };
            self.diagnostics.sample("popup_closed");
        }
        if was_engine_popup {
            self.repaint_engine_button();
        }
    }

    pub(super) fn navigate_popup(&self, key: u16) -> bool {
        let mut popup = self.popup.borrow_mut();
        let Some(popup) = popup.as_mut() else {
            return false;
        };
        let accepts_key = match popup.kind {
            PopupKind::Engines => true,
            PopupKind::History => matches!(key, VK_UP | VK_DOWN),
            PopupKind::Suggestions => {
                matches!(key, VK_UP | VK_DOWN)
                    || (popup.columns > 1 && matches!(key, VK_LEFT | VK_RIGHT))
            }
        };
        if !accepts_key {
            return false;
        }
        let Some(end) = popup.items.len().checked_sub(1) else {
            return false;
        };
        let focus = match key {
            VK_UP | VK_DOWN if popup.kind == PopupKind::History && !self.focus_popup.get() => {
                popup.first_visible()
            }
            VK_UP => popup.focus.saturating_sub(popup.columns),
            VK_DOWN => (popup.focus + popup.columns).min(end),
            VK_LEFT => popup.focus.saturating_sub(1),
            VK_RIGHT => (popup.focus + 1).min(end),
            VK_HOME => 0,
            VK_END => end,
            VK_TAB if popup.kind == PopupKind::Engines => {
                if key_down(VK_SHIFT) {
                    popup.focus.checked_sub(1).unwrap_or(end)
                } else {
                    (popup.focus + 1) % popup.items.len()
                }
            }
            _ => return false,
        };
        popup.focus = focus;
        popup.scroll_to_focus();
        self.focus_popup.set(true);
        unsafe { InvalidateRect(self.popup_window.get(), null(), 0) };
        true
    }

    pub(super) fn activate_popup(&self, edit_history: bool) {
        let selected = self.popup.borrow().as_ref().and_then(|p| {
            let index = if p.kind == PopupKind::History && !self.focus_popup.get() {
                p.first_visible()
            } else {
                p.focus
            };
            Some((p.items.get(index)?.clone(), p.kind))
        });
        if let Some((item, kind)) = selected {
            self.activate_tile(item, kind, edit_history);
        }
    }

    fn activate_tile(&self, item: Item, kind: PopupKind, edit_history: bool) {
        match item {
            Item::Engine(index) => self.choose(index, kind),
            Item::AddEngine => {
                self.close_popup();
                preferences::show(self, 2);
            }
            Item::History(record) => {
                let query = record.repeat();
                if edit_history {
                    self.restore_query(query);
                } else {
                    self.close_popup();
                    self.search(query, false);
                }
            }
        }
    }

    fn paint_popup(&self, hwnd: HWND, dc: HDC, bounds: &RECT) {
        let data = self.popup.borrow();
        let Some(popup) = data.as_ref() else {
            return;
        };
        if let Some(history) = popup.history.as_ref() {
            let english =
                preferences::english_selected(self.app.borrow().settings().preferences.language);
            history.paint_window(self, hwnd, dc, bounds, popup, english);
            return;
        }
        let app = self.app.borrow();
        let current = self.temporary.borrow();
        let selected = current
            .as_deref()
            .unwrap_or(&app.settings().preferences.default_engine);
        let suggestions = popup.kind == PopupKind::Suggestions;
        self.paint(dc, bounds, |painter| {
            painter.clear(if suggestions { SURFACE } else { RAIL });
            painter.rounded(
                rect(
                    0.5,
                    0.5,
                    popup.width as f32 - 1.0,
                    popup.height as f32 - 1.0,
                ),
                8.0,
                BORDER,
                true,
            );
            for (slot, item) in popup
                .items
                .iter()
                .skip(popup.first)
                .take(popup.columns * popup.rows)
                .enumerate()
            {
                let index = slot + popup.first;
                let column = slot % popup.columns;
                let row = slot / popup.columns;
                let x = (INSET + column as i32 * (popup.tile_width + GAP)) as f32;
                let y = (INSET + row as i32 * (popup.tile_height + GAP)) as f32;
                let w = popup.tile_width as f32;
                let h = popup.tile_height as f32;
                let engine = match item {
                    Item::Engine(index) => app.engines().get(*index),
                    _ => None,
                };
                let is_current =
                    engine.is_some_and(|e| e.id == selected) && popup.kind == PopupKind::Engines;
                let focused = index == popup.focus && (suggestions || self.focus_popup.get());
                let hovered = popup.hover == Some(index);
                if suggestions && (focused || hovered) {
                    painter.rounded(
                        rect(x, y, w, h),
                        6.0,
                        if focused {
                            blend(SURFACE, ACCENT, 0.10)
                        } else {
                            HOVER
                        },
                        false,
                    );
                    if focused {
                        painter.rounded(rect(x + 3.0, y + 9.0, 2.0, h - 18.0), 1.0, ACCENT, false);
                    }
                } else if !suggestions && (focused || hovered || is_current) {
                    painter.rounded(
                        rect(x, y, w, h),
                        8.0,
                        if is_current { SURFACE } else { HOVER },
                        false,
                    );
                    if focused || is_current {
                        painter.rounded(
                            rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0),
                            8.0,
                            if focused { ACCENT } else { BORDER },
                            true,
                        );
                    }
                }
                let Some(engine) = engine else {
                    painter.text("+", rect(x, y, w, h), ACCENT, Label::Tile);
                    continue;
                };
                if suggestions {
                    let icon = 20;
                    self.paint_engine(painter, engine, x + 10.0, y + (h - icon as f32) / 2.0, icon);
                    let alias_width = ((engine.alias.len() as f32 + 1.0) * 9.0)
                        .max(40.0)
                        .min(w * 0.5);
                    painter.text(
                        &engine.name,
                        rect(x + 40.0, y, (w - 58.0 - alias_width).max(0.0), h),
                        TEXT,
                        Label::Body,
                    );
                    painter.text(
                        &format!("#{}", engine.alias),
                        rect(x + w - 10.0 - alias_width, y, alias_width, h),
                        MUTED,
                        Label::Alias,
                    );
                } else {
                    let size = if popup.names { 28 } else { 36 };
                    self.paint_engine(
                        painter,
                        engine,
                        x + (w - size as f32) / 2.0,
                        y + if popup.names { 23.0 } else { 14.0 },
                        size,
                    );
                    if popup.names {
                        if !engine.alias.is_empty() {
                            painter.text(
                                &format!("#{}", engine.alias),
                                rect(x + 4.0, y + 4.0, w - 12.0, 16.0),
                                MUTED,
                                Label::Alias,
                            );
                        }
                        painter.text(
                            &engine.name,
                            rect(x + 4.0, y + 55.0, w - 8.0, 20.0),
                            TEXT,
                            Label::Tile,
                        );
                    }
                    if is_current {
                        painter.rounded(
                            rect(x + w / 2.0 - 8.0, y + h - 10.0, 16.0, 3.0),
                            1.5,
                            ACCENT,
                            false,
                        );
                    }
                }
            }
            let total_rows = popup.items.len().div_ceil(popup.columns);
            if suggestions && total_rows > popup.rows {
                let track = (popup.height - 2 * INSET) as f32;
                let thumb = (track * popup.rows as f32 / total_rows as f32).max(12.0);
                let progress =
                    (popup.first / popup.columns) as f32 / (total_rows - popup.rows) as f32;
                let x = (popup.width - 6) as f32;
                painter.rounded(rect(x, INSET as f32, 3.0, track), 1.5, BORDER, false);
                painter.rounded(
                    rect(x, INSET as f32 + progress * (track - thumb), 3.0, thumb),
                    1.5,
                    MUTED,
                    false,
                );
            }
        });
    }
}

impl Popup {
    fn first_visible(&self) -> usize {
        self.history
            .as_ref()
            .map_or(self.first, HistoryPanel::first_visible)
    }

    fn scroll_to_focus(&mut self) {
        if let Some(history) = self.history.as_mut() {
            history.scroll_to_focus(self.focus);
            return;
        }
        if self.focus < self.first {
            self.first = self.focus / self.columns * self.columns;
        }
        if self.focus >= self.first + self.columns * self.rows {
            self.first = (self.focus / self.columns + 1 - self.rows) * self.columns;
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if let Some(history) = self.history.as_ref() {
            return history.index(x, y, self.width);
        }
        let (x, y) = (x - INSET, y - INSET);
        if x < 0
            || y < 0
            || x % (self.tile_width + GAP) >= self.tile_width
            || y % (self.tile_height + GAP) >= self.tile_height
        {
            return None;
        }
        let col = (x / (self.tile_width + GAP)) as usize;
        let row = (y / (self.tile_height + GAP)) as usize;
        let index = self.first + row * self.columns + col;
        (col < self.columns && row < self.rows && index < self.items.len()).then_some(index)
    }
}

pub(super) unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    let Some(state) = session() else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    match msg {
        WM_PAINT => unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            if rect.right > 0 && rect.bottom > 0 {
                state.paint_popup(hwnd, dc, &rect);
            }
            EndPaint(hwnd, &paint);
            0
        },
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_NCCALCSIZE => 0,
        WM_NCHITTEST => HTCLIENT as isize,
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE | WM_LBUTTONUP => {
            let hit = {
                let mut popup = state.popup.borrow_mut();
                popup.as_mut().and_then(|popup| {
                    let x = (lp as i16 as i32) * 96 / state.dpi.get() as i32;
                    let y = ((lp >> 16) as i16 as i32) * 96 / state.dpi.get() as i32;
                    let index = popup.index(x, y);
                    if index != popup.hover {
                        popup.hover = index;
                        unsafe {
                            InvalidateRect(hwnd, null(), 0);
                        }
                    }
                    let index = index.filter(|_| msg == WM_LBUTTONUP)?;
                    Some((popup.items[index].clone(), popup.kind))
                })
            };
            if msg == WM_MOUSEMOVE {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: HOVER_DEFAULT,
                };
                unsafe {
                    TrackMouseEvent(&mut track);
                }
            } else if let Some((item, kind)) = hit {
                state.activate_tile(item, kind, false);
            }
            0
        }
        WM_MOUSELEAVE => {
            if let Some(popup) = state.popup.borrow_mut().as_mut() {
                popup.hover = None;
            }
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_MOUSEWHEEL => {
            if let Some(popup) = state.popup.borrow_mut().as_mut() {
                let delta = i32::from((wp >> 16) as i16);
                if delta == 0 {
                    return 0;
                }
                let steps = (delta.abs() / 120).max(1);
                if let Some(history) = popup.history.as_mut() {
                    history.scroll_wheel(delta.signum() * steps);
                    popup.hover = None;
                    state.focus_popup.set(false);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return 0;
                }
                let steps = steps as usize;
                let row = popup.first / popup.columns;
                let end = popup
                    .items
                    .len()
                    .div_ceil(popup.columns)
                    .saturating_sub(popup.rows);
                popup.first = (if delta < 0 {
                    (row + steps).min(end)
                } else {
                    row.saturating_sub(steps)
                }) * popup.columns;
                popup.hover = None;
                if popup.kind == PopupKind::Suggestions {
                    let last =
                        (popup.first + popup.rows * popup.columns).min(popup.items.len()) - 1;
                    popup.focus = popup.focus.clamp(popup.first, last);
                }
            }
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
