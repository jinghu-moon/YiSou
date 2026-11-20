use super::super::dip;
use super::{LOADING_LABEL, READY_LABEL, render::Canvas};
use windows_sys::Win32::{
    Foundation::{POINT, RECT, SIZE},
    Graphics::Gdi::*,
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW},
    },
};
use yisou::icons::OcrIcon;

const PANEL: u32 = 0x242933;
const BORDER: u32 = 0x444b58;
const TEXT: u32 = 0xf8f9fb;
const TOOLBAR_WIDTH: i32 = 126;
const TOOLBAR_HEIGHT: i32 = 46;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Search,
    Copy,
    Cancel,
}

pub(super) struct Chrome {
    work: RECT,
    dpi: u32,
    animate: bool,
}

impl Chrome {
    pub fn new(desktop: RECT, point: POINT) -> Self {
        let monitor = unsafe {
            MonitorFromPoint(
                POINT {
                    x: point.x + desktop.left,
                    y: point.y + desktop.top,
                },
                MONITOR_DEFAULTTONEAREST,
            )
        };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut work = if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
            info.rcWork
        } else {
            desktop
        };
        unsafe { OffsetRect(&mut work, -desktop.left, -desktop.top) };
        let mut dpi = 96;
        let mut vertical = 96;
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut vertical) };
        let dpi = dpi.max(96);
        let mut animate = 1i32;
        unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut animate as *mut i32).cast(),
                0,
            )
        };
        Self {
            work,
            dpi,
            animate: animate != 0,
        }
    }

    pub fn animations_enabled(&self) -> bool {
        self.animate
    }

    pub fn ocr_margin(&self) -> i32 {
        self.px(48)
    }

    pub fn px(&self, value: i32) -> i32 {
        dip(value, self.dpi)
    }

    pub fn toolbar(&self, area: RECT) -> RECT {
        let width = self.px(TOOLBAR_WIDTH);
        let height = self.px(TOOLBAR_HEIGHT);
        let gap = self.px(10);
        let bottom = self.work.bottom - self.px(8);
        let top = self.work.top + self.px(8);
        let below = area.bottom + gap;
        let above = area.top - gap - height;
        let y = if below + height <= bottom {
            below
        } else if above >= top {
            above
        } else {
            (area.bottom - gap - height).max(top)
        };
        self.place(area.right - width, y, width, height)
    }

    pub fn action(&self, area: RECT, point: POINT) -> Option<Action> {
        let bar = self.toolbar(area);
        [Action::Copy, Action::Search, Action::Cancel]
            .into_iter()
            .find(|&action| contains(self.button(bar, action), point))
    }

    fn button(&self, bar: RECT, action: Action) -> RECT {
        let offset = match action {
            Action::Copy => 6,
            Action::Search => 44,
            Action::Cancel => 87,
        };
        RECT {
            left: bar.left + self.px(offset),
            right: bar.left + self.px(offset + 34),
            top: bar.top + self.px(6),
            bottom: bar.top + self.px(40),
        }
    }

    fn place(&self, x: i32, y: i32, width: i32, height: i32) -> RECT {
        let left = x.clamp(
            self.work.left,
            (self.work.right - width).max(self.work.left),
        );
        let top = y.clamp(
            self.work.top,
            (self.work.bottom - height).max(self.work.top),
        );
        RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    pub fn badge(&self, area: Option<RECT>, text: &str) -> RECT {
        let width = self.px(39 + label_width(text));
        let height = self.px(29);
        let gap = self.px(10);
        let (x, y) = area.map_or(
            (
                (self.work.left + self.work.right - width) / 2,
                self.work.top + self.px(20),
            ),
            |area| {
                let x = (area.left + area.right - width) / 2;
                let above = area.top - gap - height;
                let mut y = if above >= self.work.top + self.px(8) {
                    above
                } else {
                    area.top + gap
                };
                let toolbar = self.toolbar(area);
                let badge = self.place(x, y, width, height);
                if badge.left < toolbar.right
                    && badge.right > toolbar.left
                    && badge.top < toolbar.bottom + gap
                    && badge.bottom + gap > toolbar.top
                {
                    let higher = toolbar.top - gap - height;
                    y = if higher >= self.work.top + self.px(8) {
                        higher
                    } else {
                        toolbar.bottom + gap
                    };
                }
                (x, y)
            },
        );
        self.place(x, y, width, height)
    }

    pub fn handle_at(&self, area: RECT, point: POINT) -> Option<Handle> {
        let tolerance = self.px(8);
        let near_handle = handles()
            .into_iter()
            .filter(|&handle| handle.visible(area, self.px(24)))
            .map(|handle| {
                let p = handle.position(area);
                ((p.x - point.x).abs().max((p.y - point.y).abs()), handle)
            })
            .filter(|(distance, _)| *distance <= tolerance)
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, handle)| handle);
        near_handle.or_else(|| {
            let edge = self.px(4);
            if point.x < area.left - edge
                || point.x > area.right + edge
                || point.y < area.top - edge
                || point.y > area.bottom + edge
            {
                return None;
            }
            let nearest = |value: i32, start: i32, end: i32| {
                let first = (value - start).abs();
                let last = (value - end).abs();
                if first <= edge && first <= last {
                    -1
                } else if last <= edge {
                    1
                } else {
                    0
                }
            };
            let x = nearest(point.x, area.left, area.right);
            let y = nearest(point.y, area.top, area.bottom);
            (x != 0 || y != 0).then_some(Handle { x, y })
        })
    }

    pub fn frame_width(&self) -> i32 {
        (1.5 * self.dpi as f32 / 96.0).round() as i32
    }

    pub fn grip_centers(&self, area: RECT) -> [Option<POINT>; 8] {
        let diameter = self.px(12);
        let fits = area.right - area.left >= diameter && area.bottom - area.top >= diameter;
        handles().map(|handle| {
            (fits && handle.visible(area, self.px(24))).then(|| handle.position(area))
        })
    }

    pub fn paint_badge(
        &self,
        canvas: &mut Canvas,
        area: Option<RECT>,
        label: &str,
        spinner: Option<u8>,
    ) {
        let badge = self.badge(area, label);
        canvas.rounded(badge, self.px(8), PANEL, BORDER);
        let icon = POINT {
            x: badge.left + self.px(15),
            y: (badge.top + badge.bottom) / 2,
        };
        if label == LOADING_LABEL {
            canvas.icon(
                icon,
                OcrIcon::Loader,
                self.px(OcrIcon::Loader.size()) as u32,
                0xaad5ff,
                spinner.unwrap_or(0),
            );
        } else if label == READY_LABEL || label == "已复制" {
            canvas.icon(
                icon,
                OcrIcon::Check,
                self.px(OcrIcon::Check.size()) as u32,
                0x83d1ac,
                0,
            );
        } else {
            canvas.icon(
                icon,
                OcrIcon::Scan,
                self.px(OcrIcon::Scan.size()) as u32,
                0xaeb9ca,
                0,
            );
        }
        let label_area = RECT {
            left: badge.left + self.px(27),
            right: badge.right - self.px(9),
            ..badge
        };
        canvas.text(label_area, label, self.px(12), TEXT);
    }

    pub fn paint_toolbar(
        &self,
        canvas: &mut Canvas,
        area: RECT,
        enabled: bool,
        hover: Option<Action>,
        pressed: Option<Action>,
    ) {
        let bar = self.toolbar(area);
        canvas.rounded(
            RECT {
                top: bar.top + 2,
                bottom: bar.bottom + 2,
                ..bar
            },
            self.px(8),
            0x20242c,
            0x20242c,
        );
        canvas.rounded(bar, self.px(8), PANEL, BORDER);
        canvas.fill(
            RECT {
                left: bar.left + self.px(82),
                right: bar.left + self.px(82) + 1,
                top: bar.top + self.px(14),
                bottom: bar.bottom - self.px(14),
            },
            BORDER,
            1.0,
        );
        for (action, icon) in [
            (Action::Copy, OcrIcon::Copy),
            (Action::Search, OcrIcon::Search),
            (Action::Cancel, OcrIcon::Close),
        ] {
            let button = self.button(bar, action);
            let active = action == Action::Cancel || enabled;
            let hot = active && hover == Some(action);
            let down = hot && pressed == Some(action);
            let mut color = if active { 0xc7cfdb } else { 0x778191 };
            if active && action == Action::Search {
                let fill = if down {
                    0x659ce8
                } else if hot {
                    0xa2caff
                } else {
                    0x80b5ff
                };
                canvas.rounded(button, self.px(6), fill, fill);
                color = 0x152742;
            } else if hot {
                let fill = if down { 0x3d4657 } else { 0x353d4b };
                canvas.rounded(button, self.px(6), fill, fill);
                color = TEXT;
            }
            canvas.icon(
                POINT {
                    x: (button.left + button.right) / 2,
                    y: (button.top + button.bottom) / 2,
                },
                icon,
                self.px(icon.size()) as u32,
                color,
                0,
            );
        }
    }

    pub fn tooltip(&self, area: RECT, enabled: bool, action: Action) -> (RECT, &'static str) {
        let bar = self.toolbar(area);
        let label = match (action, enabled) {
            (Action::Cancel, _) => "取消选区 · Esc",
            (Action::Copy, true) => "复制所选 · Ctrl+C",
            (Action::Search, true) => "搜索所选 · Enter",
            _ => "请先选择文字",
        };
        let button = self.button(bar, action);
        let width = self.px(label_width(label) + 20);
        let height = self.px(25);
        let below = bar.bottom + self.px(5);
        let y = if below + height <= self.work.bottom {
            below
        } else {
            bar.top - height - self.px(5)
        };
        let tip = self.place((button.left + button.right - width) / 2, y, width, height);
        (tip, label)
    }

    pub fn paint_tooltip(&self, canvas: &mut Canvas, area: RECT, enabled: bool, action: Action) {
        let (tip, label) = self.tooltip(area, enabled, action);
        canvas.rounded(tip, self.px(6), PANEL, BORDER);
        canvas.text(tip, label, self.px(12), TEXT);
    }
}

fn label_width(text: &str) -> i32 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 7 } else { 13 })
        .sum()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Handle {
    x: i32,
    y: i32,
}

fn handles() -> [Handle; 8] {
    [
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
        (0, -1),
        (0, 1),
        (-1, 0),
        (1, 0),
    ]
    .map(|(x, y)| Handle { x, y })
}

impl Handle {
    fn visible(self, area: RECT, minimum: i32) -> bool {
        (self.x != 0 || area.right - area.left >= minimum)
            && (self.y != 0 || area.bottom - area.top >= minimum)
    }

    fn position(self, area: RECT) -> POINT {
        let at = |edge, first, last| match edge {
            -1 => first,
            1 => last,
            _ => (first + last) / 2,
        };
        POINT {
            x: at(self.x, area.left, area.right),
            y: at(self.y, area.top, area.bottom),
        }
    }

    pub fn resize(self, area: RECT, delta: SIZE, bounds: RECT) -> RECT {
        let mut next = area;
        if self.x < 0 {
            next.left = (area.left + delta.cx).clamp(0, area.right - 1);
        }
        if self.x > 0 {
            next.right = (area.right + delta.cx).clamp(area.left + 1, bounds.right - bounds.left);
        }
        if self.y < 0 {
            next.top = (area.top + delta.cy).clamp(0, area.bottom - 1);
        }
        if self.y > 0 {
            next.bottom = (area.bottom + delta.cy).clamp(area.top + 1, bounds.bottom - bounds.top);
        }
        next
    }

    pub fn cursor(self) -> *const u16 {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        match (self.x, self.y) {
            (0, _) => IDC_SIZENS,
            (_, 0) => IDC_SIZEWE,
            (x, y) if x == y => IDC_SIZENWSE,
            _ => IDC_SIZENESW,
        }
    }
}

pub(super) fn contains(rect: RECT, point: POINT) -> bool {
    rect.left <= point.x && point.x < rect.right && rect.top <= point.y && point.y < rect.bottom
}
