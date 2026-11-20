use super::{
    State, instance,
    paint::{ACCENT, BORDER, HOVER, Label, MUTED, RAIL, SURFACE, TEXT, rect},
    preferences,
    search_window::key_down,
    session,
};
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

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.Engines");
const MAX_VISIBLE_ROWS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PopupKind {
    Engines,
    Suggestions,
}

pub(super) struct Popup {
    pub(super) kind: PopupKind,
    items: Vec<Option<usize>>,
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
}

const GAP: i32 = 6;
const INSET: i32 = 10;

impl State {
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
                            .map(Some)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if items.is_empty() {
            let suggestions_open = self
                .popup
                .borrow()
                .as_ref()
                .is_some_and(|p| p.kind == PopupKind::Suggestions);
            if suggestions_open {
                self.close_popup();
            }
        } else {
            self.show_popup(PopupKind::Suggestions, items);
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
            (0..count).map(Some).chain([None]).collect(),
        );
    }

    fn show_popup(&self, kind: PopupKind, items: Vec<Option<usize>>) {
        let names = self.app.borrow().settings().preferences.show_engine_names;
        let (tile_width, tile_height) = match (kind, names) {
            (PopupKind::Suggestions, _) => (332, 34),
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
        let columns = match kind {
            PopupKind::Suggestions => 1,
            PopupKind::Engines => ((work.right - work.left - self.scale(INSET * 2)
                + self.scale(GAP))
                / self.scale(tile_width + GAP))
            .clamp(1, 6) as usize,
        };
        let rows = items.len().div_ceil(columns).min(MAX_VISIBLE_ROWS);
        let width = tile_width * columns as i32 + GAP * (columns as i32 - 1) + INSET * 2;
        let height = tile_height * rows as i32 + GAP * (rows as i32 - 1) + INSET * 2;
        let focus = if kind == PopupKind::Engines {
            self.displayed_engine.get().unwrap_or(0)
        } else {
            0
        };
        let first = (focus / columns).saturating_sub(rows - 1) * columns;
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
        });
        self.focus_popup.set(false);
        let mut popup = self.popup_window.get();
        if popup.is_null() {
            popup = unsafe {
                CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                    CLASS,
                    w!("搜索引擎"),
                    WS_POPUP,
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
        let pixels_w = self.scale(width);
        let pixels_h = self.scale(height);
        let left = match kind {
            PopupKind::Engines => bounds.right - self.scale(12) - pixels_w,
            PopupKind::Suggestions => bounds.left + self.scale(20),
        };
        let top = bounds.bottom + self.scale(6);
        let x = left.clamp(work.left, (work.right - pixels_w).max(work.left));
        let y = if top + pixels_h <= work.bottom {
            top
        } else {
            (bounds.top - pixels_h - self.scale(6)).max(work.top)
        };
        unsafe {
            SetWindowPos(
                popup,
                HWND_TOPMOST,
                x,
                y,
                pixels_w,
                pixels_h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(popup, null(), 0);
        }
        if kind == PopupKind::Engines {
            self.pop_engine_tooltip();
            self.repaint_engine_button();
            self.diagnostics.sample("engine_popup_shown");
        }
    }

    pub(super) fn close_popup(&self) {
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
        if popup.kind == PopupKind::Suggestions && !matches!(key, VK_UP | VK_DOWN | VK_TAB) {
            return false;
        }
        let end = popup.items.len() - 1;
        let focus = match key {
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

    pub(super) fn activate_popup(&self) {
        let selected = self
            .popup
            .borrow()
            .as_ref()
            .map(|p| (p.items[p.focus], p.kind));
        if let Some((item, kind)) = selected {
            self.activate_tile(item, kind);
        }
    }

    fn activate_tile(&self, item: Option<usize>, kind: PopupKind) {
        if let Some(index) = item {
            self.choose(index, kind);
        } else {
            self.close_popup();
            preferences::show(self, 2);
        }
    }

    fn paint_popup(&self, dc: HDC, bounds: &RECT) {
        let data = self.popup.borrow();
        let Some(popup) = data.as_ref() else {
            return;
        };
        let app = self.app.borrow();
        let current = self.temporary.borrow();
        let selected = current
            .as_deref()
            .unwrap_or(&app.settings().preferences.default_engine);
        self.paint(dc, bounds, |painter| {
            painter.clear(RAIL);
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
                let engine = item.and_then(|index| app.engines().get(index));
                let is_current =
                    engine.is_some_and(|e| e.id == selected) && popup.kind == PopupKind::Engines;
                let focused = index == popup.focus
                    && (popup.kind == PopupKind::Suggestions || self.focus_popup.get());
                if focused || popup.hover == Some(index) || is_current {
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
                if popup.kind == PopupKind::Suggestions {
                    self.paint_engine(painter, engine, x + 7.0, y + 5.0, 24);
                    painter.text(
                        &engine.name,
                        rect(x + 44.0, y, w - 125.0, h),
                        TEXT,
                        Label::Body,
                    );
                    painter.text(
                        &format!("#{}", engine.alias),
                        rect(x + w - 80.0, y, 70.0, h),
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
        });
    }
}

impl Popup {
    fn scroll_to_focus(&mut self) {
        if self.focus < self.first {
            self.first = self.focus / self.columns * self.columns;
        }
        if self.focus >= self.first + self.columns * self.rows {
            self.first = (self.focus / self.columns + 1 - self.rows) * self.columns;
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
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
            state.paint_popup(dc, &rect);
            EndPaint(hwnd, &paint);
            0
        },
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
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
                    index.map(|i| (popup.items[i], popup.kind))
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
                state.activate_tile(item, kind);
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
                let down = ((wp >> 16) as i16) < 0;
                let row = popup.first / popup.columns;
                let end = popup
                    .items
                    .len()
                    .div_ceil(popup.columns)
                    .saturating_sub(popup.rows);
                popup.first = (if down {
                    (row + 1).min(end)
                } else {
                    row.saturating_sub(1)
                }) * popup.columns;
            }
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
