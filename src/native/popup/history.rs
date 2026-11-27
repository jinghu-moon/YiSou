use super::{Item, Popup};
use crate::native::{
    State,
    backdrop::{Composition, high_contrast, set_material, set_material_frame},
    paint::{HistoryText, Label, Painter, blend, rect},
};
use std::{
    cell::RefCell,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BLACK_BRUSH, COLOR_HIGHLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, FillRect, GetStockObject,
    GetSysColor, HDC,
};
use yisou::settings::{DEFAULT_HISTORY_HIGHLIGHT, PopupBackground, SearchMaterial};

pub(super) const MAX_HEIGHT: i32 = 342;
const HEADER: i32 = 38;
const FOOTER: i32 = 36;
const INSET: i32 = 6;
const GAP: i32 = 2;
const MIN_ROW: i32 = 44;
const TEXT_X: f32 = 49.0;
const HISTORY: (&str, &[u8]) = (
    "history",
    include_bytes!("../../../assets/icons/tabler/history.svg"),
);

pub(super) struct HistoryPanel {
    rows: Vec<Row>,
    content_height: i32,
    viewport_height: i32,
    scroll: i32,
    footer_height: i32,
    composition: RefCell<Option<(Composition, SearchMaterial)>>,
}

struct Row {
    top: i32,
    height: i32,
    text: Option<HistoryText>,
    time: String,
    time_width: f32,
}

impl HistoryPanel {
    pub(super) fn new(
        items: &[Item],
        width: i32,
        max_height: i32,
        keyword: &str,
        english: bool,
        show_shortcuts: bool,
        mut painter: Option<&mut Painter>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        let mut top = INSET;
        let rows = items
            .iter()
            .map(|item| {
                let Item::History(record) = item else {
                    unreachable!()
                };
                let time = age(now.saturating_sub(record.timestamp_ms) / 60_000, english);
                let time_width = painter
                    .as_deref_mut()
                    .and_then(|p| p.text_width(&time, Label::Hint))
                    .unwrap_or(72.0)
                    .ceil()
                    + 4.0;
                let text = painter.as_deref_mut().and_then(|p| {
                    let available = width as f32 - TEXT_X - time_width - 31.0;
                    HistoryText::new(p, &record.query, available.max(1.0), keyword)
                });
                let height = text
                    .as_ref()
                    .map_or(MIN_ROW, |text| (text.height + 16).max(MIN_ROW));
                let row = Row {
                    top,
                    height,
                    text,
                    time,
                    time_width,
                };
                top += height + GAP;
                row
            })
            .collect::<Vec<_>>();
        let content_height = if rows.is_empty() {
            60
        } else {
            top + INSET - GAP
        };
        let footer_height = if show_shortcuts { FOOTER } else { 1 };
        Self {
            rows,
            content_height,
            viewport_height: content_height.min(
                (max_height - HEADER - footer_height).clamp(1, MAX_HEIGHT + FOOTER - footer_height),
            ),
            scroll: 0,
            footer_height,
            composition: RefCell::default(),
        }
    }

    pub(super) fn height(&self) -> i32 {
        HEADER + self.viewport_height + self.footer_height
    }

    pub(super) fn apply_material(&self, state: &State, hwnd: HWND) -> bool {
        let (material, dark, radius) = {
            let app = state.app.borrow();
            let preferences = &app.settings().preferences;
            let background = preferences.history_background(state.system_dark.get());
            (
                background.material,
                background.dark,
                preferences.history_popup.corner_radius,
            )
        };
        if material == SearchMaterial::Solid && radius == 0 {
            return false;
        }
        let material = composed_material(material);
        if set_material_frame(hwnd, material, dark, true) {
            let mut composition = self.composition.borrow_mut();
            if composition.is_none() {
                *composition = Composition::new(state, hwnd, material)
                    .ok()
                    .map(|composition| (composition, material));
            }
            if composition.is_some() {
                return true;
            }
        }
        self.composition.borrow_mut().take();
        set_material(hwnd, SearchMaterial::Solid, dark);
        false
    }

    pub(super) fn paint_window(
        &self,
        state: &State,
        hwnd: HWND,
        dc: HDC,
        bounds: &RECT,
        popup: &Popup,
        english: bool,
    ) {
        let (background, radius) = {
            let app = state.app.borrow();
            let prefs = &app.settings().preferences;
            (
                prefs.history_background(state.system_dark.get()),
                prefs.history_popup.corner_radius,
            )
        };
        let mut composition = self.composition.borrow_mut();
        if let Some((composed, material)) = composition.as_mut() {
            let solid = *material == SearchMaterial::Solid;
            if composed
                .paint(
                    (bounds.right as u32, bounds.bottom as u32),
                    state.dpi.get(),
                    radius,
                    background.dark,
                    |painter, canvas| {
                        let colors = Colors::new(background, *material);
                        let (tint, opacity) = if solid {
                            (colors.surface, 1.0)
                        } else {
                            (background.color, background.tint_percent as f32 / 100.0)
                        };
                        unsafe { canvas.Clear(Some(&crate::native::paint::color(tint, opacity))) };
                        self.paint(state, painter, popup, english, colors);
                        Ok(())
                    },
                )
                .is_ok()
            {
                unsafe { FillRect(dc, bounds, GetStockObject(BLACK_BRUSH)) };
                return;
            }
            *composition = None;
            set_material(hwnd, SearchMaterial::Solid, background.dark);
        }
        state.paint(dc, bounds, |painter| {
            let colors = Colors::new(background, SearchMaterial::Solid);
            painter.clear(colors.surface);
            self.paint(state, painter, popup, english, colors);
        });
    }

    fn scroll_to(&mut self, value: i32) {
        self.scroll = value.clamp(0, self.content_height - self.viewport_height);
    }

    pub(super) fn scroll_wheel(&mut self, steps: i32) {
        self.scroll_to(self.scroll - steps * MIN_ROW);
    }

    pub(super) fn scroll_to_focus(&mut self, index: usize) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        if row.top < self.scroll || row.height >= self.viewport_height {
            self.scroll_to(row.top - INSET);
        } else if row.top + row.height > self.scroll + self.viewport_height {
            self.scroll_to(row.top + row.height + INSET - self.viewport_height);
        }
    }

    pub(super) fn index(&self, x: i32, y: i32, width: i32) -> Option<usize> {
        let y = y - HEADER;
        if x < INSET || x >= width - INSET || !(0..self.viewport_height).contains(&y) {
            return None;
        }
        let y = y + self.scroll;
        self.rows
            .iter()
            .position(|row| (row.top..row.top + row.height).contains(&y))
    }

    pub(super) fn first_visible(&self) -> usize {
        self.rows
            .iter()
            .position(|row| row.top + row.height > self.scroll)
            .unwrap_or(0)
    }

    fn paint(
        &self,
        state: &State,
        painter: &mut Painter,
        popup: &Popup,
        english: bool,
        colors: Colors,
    ) {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        let history = &prefs.history_popup;
        let highlight = (
            history.highlight_color.unwrap_or(if high_contrast() {
                colors.accent
            } else {
                DEFAULT_HISTORY_HIGHLIGHT
            }),
            (100 - history.highlight_transparency) as f32 / 100.0,
        );
        let width = popup.width as f32;
        let height = self.height() as f32;
        let footer = (HEADER + self.viewport_height) as f32;
        let pixel = 96.0 / state.dpi.get() as f32;
        let (stroke, divider_opacity, keycap_opacity) = if colors.acrylic {
            (colors.text, 0.16, 0.34)
        } else {
            (colors.border, 1.0, 1.0)
        };
        painter.smooth_border(width, height, history.corner_radius, pixel, colors.border);
        painter.svg_icon(HISTORY, (15.0, 11.0), 16.0, state.dpi.get(), colors.muted);
        let title = if english {
            "Search history"
        } else {
            "搜索历史"
        };
        painter.text(
            title,
            rect(39.0, 0.0, width - 124.0, HEADER as f32),
            colors.text,
            Label::Body,
        );
        let count = match (popup.items.len(), english) {
            (1, true) => "1 match".into(),
            (count, true) => format!("{count} matches"),
            (count, false) => format!("{count} 条"),
        };
        painter.text(
            &count,
            rect(width - 83.0, 0.0, 67.0, HEADER as f32),
            colors.muted,
            Label::Alias,
        );
        painter.rounded_alpha(
            rect(1.0, HEADER as f32, width - 2.0, 1.0),
            0.0,
            stroke,
            divider_opacity,
            false,
        );
        let area = rect(
            1.0,
            HEADER as f32 + 1.0,
            width - 2.0,
            (self.viewport_height - 1).max(0) as f32,
        );
        painter.with_clip(area, |painter| {
            if self.rows.is_empty() {
                let text = if english {
                    "No matching history. Press Enter to search."
                } else {
                    "没有匹配的搜索历史。按 Enter 搜索当前关键词。"
                };
                painter.text(
                    text,
                    rect(16.0, HEADER as f32, width - 32.0, 60.0),
                    colors.muted,
                    Label::Body,
                );
            }
            for (index, row) in self.rows.iter().enumerate() {
                let top = row.top - self.scroll;
                if top + row.height <= 0 || top >= self.viewport_height {
                    continue;
                }
                let y = (HEADER + top) as f32;
                let h = row.height as f32;
                let focused = state.focus_popup.get() && popup.focus == index;
                let hovered = popup.hover == Some(index);
                if focused || hovered {
                    let area = rect(6.0, y, width - 12.0, h);
                    if colors.acrylic {
                        let (fill, opacity) = if focused {
                            (colors.accent, 0.16)
                        } else {
                            (colors.text, 0.08)
                        };
                        painter.rounded_alpha(area, 6.0, fill, opacity, false);
                    } else {
                        let opacity = if focused { 0.13 } else { 0.07 };
                        painter.rounded(
                            area,
                            6.0,
                            blend(colors.surface, colors.accent, opacity),
                            false,
                        );
                    }
                }
                if focused {
                    let (border, opacity) = if colors.acrylic {
                        (colors.accent, 0.42)
                    } else {
                        (blend(colors.surface, colors.accent, 0.36), 1.0)
                    };
                    painter.rounded_alpha(
                        rect(6.5, y + 0.5, width - 13.0, h - 1.0),
                        6.0,
                        border,
                        opacity,
                        true,
                    );
                    painter.rounded(
                        rect(7.0, y + 10.0, 3.0, h - 20.0),
                        1.5,
                        colors.accent,
                        false,
                    );
                }
                let Item::History(record) = &popup.items[index] else {
                    unreachable!()
                };
                let image_y = y + (h - 18.0) / 2.0;
                let engine = app.all_engines().iter().find(|e| e.id == record.engine_id);
                if let Some(engine) = engine {
                    // Use the same raster cache as the engine picker and choose a
                    // legible automatic tint against this panel's own surface.
                    let mut tinted;
                    let engine = if colors.dark
                        && !prefs.multicolor_engine_icons
                        && engine.mono_color.is_none()
                        && !(engine.is_custom() && engine.icon_for(false).is_default())
                    {
                        tinted = engine.clone();
                        tinted.mono_color = Some(colors.text);
                        &tinted
                    } else {
                        engine
                    };
                    state.paint_engine(painter, engine, 18.0, image_y, 18);
                } else {
                    painter.svg_icon(
                        HISTORY,
                        (18.0, image_y),
                        18.0,
                        state.dpi.get(),
                        colors.muted,
                    );
                }
                if let Some(text) = &row.text {
                    text.draw_with_highlight(
                        painter,
                        (TEXT_X, y + (h - text.height as f32) / 2.0),
                        colors.text,
                        highlight,
                    );
                } else {
                    painter.text(
                        &record.query,
                        rect(
                            TEXT_X,
                            y,
                            (width - TEXT_X - row.time_width - 31.0).max(1.0),
                            h,
                        ),
                        colors.text,
                        Label::Body,
                    );
                }
                painter.text(
                    &row.time,
                    rect(width - row.time_width - 19.0, y, row.time_width, h),
                    colors.muted,
                    Label::Alias,
                );
            }
            let max = self.content_height - self.viewport_height;
            if max > 0 {
                let track = self.viewport_height as f32 - 8.0;
                if track > 0.0 {
                    let thumb = (track * self.viewport_height as f32 / self.content_height as f32)
                        .max(18.0)
                        .min(track);
                    let offset = (track - thumb) * self.scroll as f32 / max as f32;
                    painter.rounded(
                        rect(width - 5.0, HEADER as f32 + 4.0 + offset, 2.0, thumb),
                        1.0,
                        colors.muted,
                        false,
                    );
                }
            }
        });
        if self.footer_height != FOOTER {
            return;
        }
        painter.rounded_alpha(
            rect(1.0, footer, width - 2.0, 1.0),
            0.0,
            stroke,
            divider_opacity,
            false,
        );
        let mut x = 14.0;
        let hints = if english {
            [
                ("↑ ↓", "Select"),
                ("Enter", "Search"),
                ("Tab", "Edit"),
                ("Esc", "Close"),
            ]
        } else {
            [
                ("↑ ↓", "选择"),
                ("Enter", "搜索"),
                ("Tab", "回填编辑"),
                ("Esc", "关闭"),
            ]
        };
        for (key, action) in hints {
            if self.rows.is_empty() && matches!(key, "↑ ↓" | "Tab") {
                continue;
            }
            let key_width = painter
                .text_width(key, Label::ControlDetail)
                .unwrap_or(24.0)
                + 12.0;
            let action_width = painter.text_width(action, Label::Hint).unwrap_or(50.0) + 1.0;
            let key_area = rect(x, footer + 9.0, key_width, 19.0);
            painter.rounded_alpha(key_area, 4.0, stroke, keycap_opacity, true);
            painter.text(key, key_area, colors.text, Label::ControlDetail);
            x += key_width + 5.0;
            painter.text(
                action,
                rect(x, footer + 7.0, action_width, 22.0),
                colors.muted,
                Label::Hint,
            );
            x += action_width + 13.0;
        }
    }
}

fn composed_material(material: SearchMaterial) -> SearchMaterial {
    if high_contrast() || !super::super::screen_shade::transparency_enabled() {
        SearchMaterial::Solid
    } else {
        material
    }
}

struct Colors {
    dark: bool,
    acrylic: bool,
    surface: u32,
    text: u32,
    muted: u32,
    accent: u32,
    border: u32,
}

impl Colors {
    fn new(background: PopupBackground, material: SearchMaterial) -> Self {
        let surface = if material != SearchMaterial::Solid {
            blend(
                if background.dark { 0x282828 } else { 0xf0f0f0 },
                background.color,
                background.tint_percent as f32 / 100.0,
            )
        } else {
            background.color
        };
        let [_, r, g, b] = surface.to_be_bytes();
        let dark = 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) < 128_000;
        if crate::native::backdrop::high_contrast() {
            let sys = |index| crate::native::color_ref(unsafe { GetSysColor(index) });
            let text = sys(COLOR_WINDOWTEXT);
            return Self {
                dark,
                acrylic: false,
                surface: sys(COLOR_WINDOW),
                text,
                muted: text,
                accent: sys(COLOR_HIGHLIGHT),
                border: text,
            };
        }
        let acrylic = material == SearchMaterial::Acrylic;
        if dark {
            Self {
                dark,
                acrylic,
                surface,
                text: 0xf2f5fb,
                muted: 0xaeb9ca,
                accent: 0x75adf6,
                border: 0x3b4554,
            }
        } else {
            Self {
                dark,
                acrylic,
                surface,
                text: 0x202632,
                muted: 0x596576,
                accent: 0x1f75db,
                border: 0xdde2eb,
            }
        }
    }
}

fn age(minutes: u64, english: bool) -> String {
    match (minutes, english) {
        (0, true) => "Just now".into(),
        (0, false) => "刚刚".into(),
        (1..=59, true) => format!("{minutes} min ago"),
        (1..=59, false) => format!("{minutes} 分钟前"),
        (60..=1439, true) => format!("{} hr ago", minutes / 60),
        (60..=1439, false) => format!("{} 小时前", minutes / 60),
        (_, true) => format!("{} d ago", minutes / 1440),
        (_, false) => format!("{} 天前", minutes / 1440),
    }
}
