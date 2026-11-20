use super::{NAV, PAGES, PANEL, Panel, components::Surface, nav_pages, tr};
use crate::native::{
    State, color_ref, dip,
    paint::{ACCENT, BORDER, HOVER, Label, Painter, RAIL, SELECTED, rect},
    text_of, wide,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    ptr::{null, null_mut},
    time::Instant,
};
use windows::Win32::{Foundation::RECT as D2dRect, Graphics::Gdi::HDC as D2dDc};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    UI::{
        Controls::*,
        HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi},
        Input::KeyboardAndMouse::{
            GetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE,
        },
        Shell::{DefSubclassProc, ExtractIconExW, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
use yisou::engine::Icon;

const CONTENT_WIDTH: i32 = 536;
const SIDEBAR: i32 = 184;
const GUTTER: i32 = 26;
const NAV_ROW: i32 = 39;
const CARD: u32 = 0xfafbfd;
const TEXT: u32 = 0x1a1e26;
const DETAIL: u32 = 0x545e6f;
const WHITE: u32 = 0xffffff;
pub(super) const TOGGLE_TIMER: usize = 17;
pub(super) const RIGHT: i32 = -1;
pub(super) const STRETCH: i32 = -2;

#[derive(Clone, Copy, Default)]
pub(super) enum FontRole {
    #[default]
    Body,
    Detail,
    Header,
    Section,
    Preview,
}

#[derive(Clone, Copy)]
pub(super) struct Layout {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    sizing: i32,
    bottom: bool,
    font: FontRole,
}

impl Layout {
    pub(super) const fn new(x: i32, y: i32, width: i32, height: i32, sizing: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            sizing,
            bottom: false,
            font: FontRole::Body,
        }
    }

    pub(super) fn font(mut self, font: FontRole) -> Self {
        self.font = font;
        self
    }

    pub(super) fn bottom(mut self) -> Self {
        self.bottom = true;
        self
    }

    fn place(self, viewport: Viewport, scroll: i32) -> RECT {
        let delta = viewport.width - CONTENT_WIDTH;
        let x = viewport.left + self.x + if self.sizing == RIGHT { delta } else { 0 };
        let y = if self.bottom {
            viewport.height + self.y
        } else {
            self.y
        };
        let width = self.width + if self.sizing == STRETCH { delta } else { 0 };
        let height = if self.height < 0 {
            viewport.height + self.height
        } else {
            self.height
        };
        scaled_rect(x, y, width.max(1), height.max(1), viewport.dpi, scroll)
    }
}

#[derive(Default)]
pub(super) struct View {
    pub(super) controls: Vec<(HWND, Layout)>,
    pub(super) surfaces: Vec<Surface>,
    pub(super) extent: i32,
    pub(super) engine_icon: Option<yisou::engine::Engine>,
    toggle_motion: Option<(HWND, Instant)>,
    pub(super) scroll: i32,
    pub(super) hover: HWND,
    pub(super) nav_hover: Option<usize>,
    pub(super) wheel: i32,
    fonts: Option<Fonts>,
    painter: Option<Painter>,
    browser_icons: HashMap<String, BrowserIcon>,
}

struct BrowserIcon(HICON);

pub(super) fn animate_toggle(hwnd: HWND, id: i32) {
    unsafe {
        let mut animate = 0i32;
        if SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut animate as *mut i32).cast(),
            0,
        ) != 0
            && animate != 0
        {
            let toggle = GetDlgItem(hwnd, id);
            PANEL.with_borrow_mut(|p| p.view.toggle_motion = Some((toggle, Instant::now())));
            SetTimer(hwnd, TOGGLE_TIMER, 15, None);
            InvalidateRect(toggle, null(), 0);
        }
    }
}

pub(super) fn tick_toggle(hwnd: HWND) {
    let motion = PANEL.with_borrow_mut(|p| {
        let motion = p.view.toggle_motion?;
        if motion.1.elapsed().as_millis() >= 120 {
            p.view.toggle_motion = None;
        }
        Some((motion.0, p.view.toggle_motion.is_some()))
    });
    unsafe {
        if let Some((toggle, _)) = motion
            && IsWindow(toggle) != 0
        {
            InvalidateRect(toggle, null(), 0);
        }
        if !motion.is_some_and(|(_, active)| active) {
            KillTimer(hwnd, TOGGLE_TIMER);
        }
    }
}

impl Drop for BrowserIcon {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { DestroyIcon(self.0) };
        }
    }
}

fn browser_icon(path: &str) -> HICON {
    if path.is_empty() {
        return null_mut();
    }
    if let Some(icon) = PANEL.with_borrow(|p| p.view.browser_icons.get(path).map(|icon| icon.0)) {
        return icon;
    }
    let mut icon = null_mut();
    unsafe { ExtractIconExW(wide(path).as_ptr(), 0, null_mut(), &mut icon, 1) };
    PANEL.with_borrow_mut(|p| {
        p.view
            .browser_icons
            .insert(path.to_owned(), BrowserIcon(icon))
    });
    icon
}

pub(super) fn history_time(timestamp: u64) -> String {
    let Some(ticks) = timestamp
        .checked_mul(10_000)
        .and_then(|ticks| ticks.checked_add(116_444_736_000_000_000))
    else {
        return String::new();
    };
    let time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&time, &mut utc) } == 0
        || unsafe { SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) } == 0
    {
        return String::new();
    }
    format!(
        "{}/{}/{} {}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
    )
}

struct Fonts {
    dpi: u32,
    handles: [HFONT; 4],
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        unsafe {
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
                0,
                dpi,
            );
        }
        let handles = [
            (14, FW_NORMAL),
            (12, FW_NORMAL),
            (28, FW_SEMIBOLD),
            (15, FW_SEMIBOLD),
        ]
        .map(|(size, weight)| {
            let mut font = metrics.lfMessageFont;
            font.lfHeight = -dip(size, dpi);
            font.lfWidth = 0;
            font.lfWeight = weight as i32;
            unsafe { CreateFontIndirectW(&font) }
        });
        Self { dpi, handles }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for font in self.handles {
            if !font.is_null() {
                unsafe { DeleteObject(font) };
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Viewport {
    sidebar: i32,
    left: i32,
    width: i32,
    height: i32,
    dpi: u32,
}

impl Viewport {
    fn of(hwnd: HWND) -> Self {
        let mut bounds = RECT::default();
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        unsafe { GetClientRect(hwnd, &mut bounds) };
        let logical = bounds.right * 96 / dpi as i32;
        let sidebar = if logical < 730 { 148 } else { SIDEBAR };
        let available = logical - sidebar - 2 * GUTTER;
        let width = available.clamp(0, 740);
        Self {
            sidebar,
            left: sidebar + GUTTER + (available - width) / 2,
            width,
            height: bounds.bottom * 96 / dpi as i32,
            dpi,
        }
    }
}

fn scaled_rect(x: i32, y: i32, width: i32, height: i32, dpi: u32, scroll: i32) -> RECT {
    RECT {
        left: dip(x, dpi),
        top: dip(y, dpi) - scroll,
        right: dip(x + width, dpi),
        bottom: dip(y + height, dpi) - scroll,
    }
}

#[derive(Clone, Copy)]
pub(super) struct Palette {
    rail: u32,
    background: u32,
    pub(super) card: u32,
    pub(super) text: u32,
    pub(super) detail: u32,
    pub(super) accent: u32,
    pub(super) selection: u32,
    pub(super) hover: u32,
    pub(super) border: u32,
    pub(super) button: u32,
    primary_text: u32,
}

pub(super) fn palette() -> Palette {
    unsafe {
        if crate::native::backdrop::high_contrast() {
            let sys = |index| color_ref(GetSysColor(index));
            return Palette {
                rail: sys(COLOR_WINDOW),
                background: sys(COLOR_WINDOW),
                card: sys(COLOR_WINDOW),
                text: sys(COLOR_WINDOWTEXT),
                detail: sys(COLOR_WINDOWTEXT),
                accent: sys(COLOR_HIGHLIGHT),
                selection: sys(COLOR_WINDOW),
                hover: sys(COLOR_WINDOW),
                border: sys(COLOR_WINDOWTEXT),
                button: sys(COLOR_BTNFACE),
                primary_text: sys(COLOR_HIGHLIGHTTEXT),
            };
        }
    }
    Palette {
        rail: RAIL,
        background: 0xf7f8fa,
        card: CARD,
        text: TEXT,
        detail: DETAIL,
        accent: ACCENT,
        selection: SELECTED,
        hover: HOVER,
        border: BORDER,
        button: WHITE,
        primary_text: WHITE,
    }
}

pub(super) fn apply_palette(hwnd: HWND) {
    let colors = palette();
    unsafe {
        for (attr, rgb) in [
            (DWMWA_CAPTION_COLOR, colors.rail),
            (DWMWA_TEXT_COLOR, colors.text),
        ] {
            let color = color_ref(rgb);
            DwmSetWindowAttribute(hwnd, attr as u32, (&color as *const u32).cast(), 4);
        }
        InvalidateRect(hwnd, null(), 0);
    }
}

fn scroll_range(panel: &Panel, viewport: Viewport) -> i32 {
    dip(panel.view.extent - viewport.height, viewport.dpi).max(0)
}

pub(super) fn layout(hwnd: HWND) {
    position_controls(hwnd, true);
}

fn position_controls(hwnd: HWND, full_layout: bool) {
    let initial = Viewport::of(hwnd);
    let (scroll, maximum) = PANEL.with_borrow_mut(|panel| {
        let maximum = scroll_range(panel, initial);
        panel.view.scroll = panel.view.scroll.clamp(0, maximum);
        (panel.view.scroll, maximum)
    });
    let page = dip(initial.height, initial.dpi);
    let scroll_info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMax: maximum + page - 1,
        nPage: page.max(0) as u32,
        nPos: scroll,
        ..Default::default()
    };
    unsafe { SetScrollInfo(hwnd, SB_VERT, &scroll_info, 1) };
    let viewport = Viewport::of(hwnd);
    let (controls, old_fonts, replacements, preview, page) = PANEL.with_borrow_mut(|panel| {
        let old = if panel
            .view
            .fonts
            .as_ref()
            .is_none_or(|f| f.dpi != viewport.dpi)
        {
            panel.view.fonts.replace(Fonts::new(viewport.dpi))
        } else {
            None
        };
        (
            panel.view.controls.clone(),
            old,
            panel.view.fonts.as_ref().unwrap().handles,
            panel.preview_font,
            panel.page,
        )
    });
    unsafe {
        let nav = GetDlgItem(hwnd, NAV as i32);
        if full_layout && SendMessageW(nav, WM_GETFONT, 0, 0) != replacements[0] as isize {
            SendMessageW(nav, WM_SETFONT, replacements[0] as usize, 0);
        }
        let nav_row = dip(
            if viewport.height < 375 { 34 } else { NAV_ROW },
            viewport.dpi,
        );
        if full_layout && SendMessageW(nav, LB_GETITEMHEIGHT, 0, 0) != nav_row as isize {
            SendMessageW(nav, LB_SETITEMHEIGHT, 0, nav_row as isize);
        }
        let mut positions = BeginDeferWindowPos((controls.len() + 1) as i32);
        let mut place = |control, bounds: RECT| {
            let flags = SWP_NOZORDER
                | SWP_NOACTIVATE
                | SWP_NOREDRAW
                | if full_layout { 0 } else { SWP_NOSIZE };
            let width = bounds.right - bounds.left;
            let height = bounds.bottom - bounds.top;
            if !positions.is_null() {
                positions = DeferWindowPos(
                    positions,
                    control,
                    null_mut(),
                    bounds.left,
                    bounds.top,
                    width,
                    height,
                    flags,
                );
            }
            if positions.is_null() {
                SetWindowPos(
                    control,
                    null_mut(),
                    bounds.left,
                    bounds.top,
                    width,
                    height,
                    flags,
                );
            }
        };
        if full_layout && !nav.is_null() {
            place(
                nav,
                scaled_rect(
                    11,
                    8,
                    viewport.sidebar - 23,
                    (viewport.height - 12).clamp(1, PAGES.len() as i32 * NAV_ROW),
                    viewport.dpi,
                    0,
                ),
            );
        }
        for (child, layout) in controls {
            if !full_layout {
                place(child, layout.place(viewport, scroll));
                continue;
            }
            let font = match layout.font {
                FontRole::Header => replacements[2],
                FontRole::Section => replacements[3],
                FontRole::Detail => replacements[1],
                FontRole::Preview if !preview.is_null() => preview,
                _ => replacements[0],
            };
            if SendMessageW(child, WM_GETFONT, 0, 0) != font as isize {
                SendMessageW(child, WM_SETFONT, font as usize, 0);
            }
            if GetDlgCtrlID(child) == 10 {
                let height = match page {
                    2 => 48,
                    4 => 52,
                    _ => 62,
                };
                let height = dip(height, viewport.dpi).min(255) as isize;
                if SendMessageW(child, LB_GETITEMHEIGHT, 0, 0) != height {
                    SendMessageW(child, LB_SETITEMHEIGHT, 0, height);
                }
            }
            let mut bounds = layout.place(viewport, scroll);
            if super::dropdown::is_combo(child) {
                bounds.bottom = bounds.top + super::dropdown::sync_metrics(child, viewport.dpi);
            }
            place(child, bounds);
        }
        if !positions.is_null() {
            EndDeferWindowPos(positions);
        }
        let content = RECT {
            left: dip(viewport.sidebar, viewport.dpi),
            top: 0,
            right: dip(viewport.left + viewport.width + GUTTER, viewport.dpi),
            bottom: dip(viewport.height, viewport.dpi),
        };
        RedrawWindow(
            hwnd,
            if full_layout { null() } else { &content },
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
        );
    }
    drop(old_fonts);
}

pub(super) fn scroll(hwnd: HWND, message: u32, word: WPARAM) {
    let viewport = Viewport::of(hwnd);
    let changed = PANEL.with_borrow_mut(|panel| {
        let maximum = scroll_range(panel, viewport);
        let view = &mut panel.view;
        let line = dip(22, viewport.dpi);
        let page = dip(viewport.height, viewport.dpi);
        let next = if message == WM_MOUSEWHEEL {
            view.wheel += i32::from((word >> 16) as i16);
            let notches = view.wheel / 120;
            view.wheel %= 120;
            let mut lines = 3_u32;
            unsafe {
                SystemParametersInfoW(
                    SPI_GETWHEELSCROLLLINES,
                    0,
                    (&mut lines as *mut u32).cast(),
                    0,
                )
            };
            let step = if lines == u32::MAX {
                page
            } else {
                line * lines.min(100) as i32
            };
            view.scroll - notches * step
        } else {
            match (word & 0xffff) as i32 {
                SB_LINEUP => view.scroll - line,
                SB_LINEDOWN => view.scroll + line,
                SB_PAGEUP => view.scroll - page,
                SB_PAGEDOWN => view.scroll + page,
                SB_TOP => 0,
                SB_BOTTOM => maximum,
                SB_THUMBTRACK | SB_THUMBPOSITION => {
                    let mut info = SCROLLINFO {
                        cbSize: size_of::<SCROLLINFO>() as u32,
                        fMask: SIF_TRACKPOS,
                        ..Default::default()
                    };
                    unsafe { GetScrollInfo(hwnd, SB_VERT, &mut info) };
                    info.nTrackPos
                }
                _ => view.scroll,
            }
        }
        .clamp(0, maximum);
        let changed = next != view.scroll;
        view.scroll = next;
        changed
    });
    if changed {
        position_controls(hwnd, false);
    }
}

pub(super) fn ensure_visible(hwnd: HWND, child: HWND) {
    let viewport = Viewport::of(hwnd);
    let changed = PANEL.with_borrow_mut(|panel| {
        let maximum = scroll_range(panel, viewport);
        let Some((_, area)) = panel.view.controls.iter().find(|(hwnd, _)| *hwnd == child) else {
            return false;
        };
        let scroll = panel.view.scroll;
        let bounds = area.place(viewport, scroll);
        let bottom = dip(viewport.height, viewport.dpi);
        let offset = if bounds.top < 0 {
            bounds.top
        } else {
            (bounds.bottom - bottom).max(0)
        };
        panel.view.scroll = (scroll + offset).clamp(0, maximum);
        panel.view.scroll != scroll
    });
    if changed {
        position_controls(hwnd, false);
    }
}

fn has_class(hwnd: HWND, name: &str) -> bool {
    let mut units = [0; 16];
    let len = unsafe { GetClassNameW(hwnd, units.as_mut_ptr(), units.len() as i32) };
    len as usize == name.len()
        && units[..len as usize]
            .iter()
            .zip(name.bytes())
            .all(|(&unit, byte)| (unit as u8).eq_ignore_ascii_case(&byte) && unit <= 127)
}

pub(super) fn track_control(hwnd: HWND) {
    let kind = unsafe {
        if has_class(hwnd, "BUTTON") {
            1
        } else if has_class(hwnd, "EDIT") {
            2
        } else if GetDlgCtrlID(hwnd) as usize == NAV {
            3
        } else if super::dropdown::is_combo(hwnd) {
            4
        } else {
            0
        }
    };
    unsafe { SetWindowSubclass(hwnd, Some(control_proc), 1, kind) };
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    kind: usize,
) -> LRESULT {
    unsafe {
        let parent = GetParent(hwnd);
        match msg {
            WM_MOUSEMOVE if matches!(kind, 1 | 3 | 4) => {
                let row = (kind == 3)
                    .then(|| {
                        let hit = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, lp);
                        (hit >> 16 == 0).then_some(hit as usize & 0xffff)
                    })
                    .flatten();
                let changed = PANEL.with_borrow_mut(|p| {
                    if kind == 3 {
                        let previous = p.view.nav_hover;
                        p.view.nav_hover = row;
                        previous != row
                    } else {
                        let changed = p.view.hover != hwnd;
                        p.view.hover = hwnd;
                        changed
                    }
                });
                if changed {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_MOUSELEAVE => {
                PANEL.with_borrow_mut(|p| {
                    if kind == 3 {
                        p.view.nav_hover = None;
                    }
                    if p.view.hover == hwnd {
                        p.view.hover = null_mut();
                    }
                });
                InvalidateRect(hwnd, null(), 0);
            }
            WM_SETFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                ensure_visible(parent, hwnd);
                if kind == 2 {
                    InvalidateRect(parent, null(), 0);
                }
                return result;
            }
            WM_KILLFOCUS if kind == 2 => {
                InvalidateRect(parent, null(), 0);
            }
            WM_KEYDOWN if wp as u16 == VK_ESCAPE && kind != 4 => {
                PostMessageW(parent, super::BACK, 0, 0);
                return 0;
            }
            WM_MOUSEWHEEL if !matches!(kind, 3 | 4) && GetDlgCtrlID(hwnd) != 10 => {
                return SendMessageW(parent, msg, wp, lp);
            }
            WM_CONTEXTMENU if GetDlgCtrlID(hwnd) == 10 && PANEL.with_borrow(|p| p.page == 2) => {
                if lp != -1 {
                    let mut point = POINT {
                        x: i32::from(lp as i16),
                        y: i32::from((lp >> 16) as i16),
                    };
                    ScreenToClient(hwnd, &mut point);
                    let hit = SendMessageW(
                        hwnd,
                        LB_ITEMFROMPOINT,
                        0,
                        ((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize,
                    );
                    if hit >> 16 != 0 {
                        return 0;
                    }
                    SendMessageW(hwnd, LB_SETCURSEL, hit as usize & 0xffff, 0);
                }
                PostMessageW(parent, super::ENGINE_MENU, 0, 0);
                return 0;
            }
            WM_LBUTTONUP if GetDlgCtrlID(hwnd) == 10 && PANEL.with_borrow(|p| p.page == 2) => {
                let mut bounds = RECT::default();
                GetClientRect(hwnd, &mut bounds);
                let x = i32::from(lp as i16);
                let hit = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, lp);
                if hit >> 16 == 0 && x >= bounds.right - dip(35, GetDpiForWindow(hwnd)) {
                    let result = DefSubclassProc(hwnd, msg, wp, lp);
                    SendMessageW(hwnd, LB_SETCURSEL, hit as usize & 0xffff, 0);
                    PostMessageW(parent, super::ENGINE_MENU, 0, 0);
                    return result;
                }
            }
            WM_LBUTTONUP if GetDlgCtrlID(hwnd) == 10 && PANEL.with_borrow(|p| p.page == 5) => {
                let hit = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, lp);
                let index = hit as usize & 0xffff;
                if hit >> 16 == 0 && PANEL.with_borrow(|p| index < p.history.len()) {
                    let mut row = RECT::default();
                    if SendMessageW(
                        hwnd,
                        LB_GETITEMRECT,
                        index,
                        (&mut row as *mut RECT) as isize,
                    ) >= 0
                    {
                        let dpi = GetDpiForWindow(hwnd);
                        let x = i32::from(lp as i16);
                        let y = i32::from((lp >> 16) as i16);
                        let right = row.right - x;
                        if (row.top + dip(15, dpi)..row.bottom - dip(15, dpi)).contains(&y) {
                            let action = if (dip(15, dpi)..dip(69, dpi)).contains(&right) {
                                152
                            } else if (dip(81, dpi)..dip(165, dpi)).contains(&right) {
                                151
                            } else {
                                0
                            };
                            if action != 0 {
                                SendMessageW(hwnd, LB_SETCURSEL, index, 0);
                                PostMessageW(parent, super::HISTORY_ACTION, action, 0);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

const NAV_ICONS: [(&str, &[u8]); 8] = [
    (
        "settings",
        include_bytes!("../../../../assets/icons/tabler/settings.svg"),
    ),
    (
        "palette",
        include_bytes!("../../../../assets/icons/tabler/palette.svg"),
    ),
    (
        "search",
        include_bytes!("../../../../assets/icons/tabler/search.svg"),
    ),
    (
        "keyboard",
        include_bytes!("../../../../assets/icons/tabler/keyboard.svg"),
    ),
    (
        "browser",
        include_bytes!("../../../../assets/icons/tabler/browser.svg"),
    ),
    (
        "history",
        include_bytes!("../../../../assets/icons/tabler/history.svg"),
    ),
    (
        "file-typography",
        include_bytes!("../../../../assets/icons/tabler/file-typography.svg"),
    ),
    (
        "scan",
        include_bytes!("../../../../assets/icons/tabler/scan.svg"),
    ),
];

pub(super) fn icon(
    painter: &mut Painter,
    (id, svg): (&str, &[u8]),
    x: f32,
    y: f32,
    size: f32,
    dpi: u32,
    color: u32,
) {
    let edge = (size * dpi as f32 / 96.0).round().max(1.0) as u32;
    let key = format!("settings-{id}-{color:06x}");
    painter.image(&key, rect(x, y, size, size), edge, || {
        yisou_svg::render(svg, edge, Some(color)).map(Cow::Owned)
    });
}

pub(super) fn with_painter(dc: HDC, bounds: RECT, dpi: u32, draw: impl FnOnce(&mut Painter)) {
    if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
        return;
    }
    let mut painter = PANEL
        .with_borrow_mut(|p| p.view.painter.take())
        .or_else(|| Painter::new("Segoe UI", None).ok());
    let target = D2dRect {
        left: bounds.left,
        top: bounds.top,
        right: bounds.right,
        bottom: bounds.bottom,
    };
    if let Some(p) = painter.as_mut()
        && p.begin(D2dDc(dc), &target, dpi).is_ok()
    {
        draw(p);
        if p.end().is_err() {
            painter = None;
        }
    }
    PANEL.with_borrow_mut(|p| p.view.painter = painter);
}

pub(super) fn reset_images() {
    PANEL.with_borrow_mut(|p| {
        if let Some(painter) = p.view.painter.as_mut() {
            painter.reset_images();
        }
    });
}

fn paint_preview(
    state: &State,
    hwnd: HWND,
    p: &mut Painter,
    colors: Palette,
    view: Viewport,
    page: usize,
    editing: bool,
    x: f32,
    offset: f32,
) {
    let width = view.width as f32;
    if page == 1 || (page == 2 && !editing) {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        let engines = app.engines();
        let default = engines
            .iter()
            .find(|engine| engine.id == prefs.default_engine);
        if page == 1 {
            let area = rect(x + 14.0, 229.0 - offset, width - 28.0, 44.0);
            p.rounded(area, 6.0, colors.button, false);
            p.rounded(area, 6.0, colors.border, true);
            if let Some(engine) = default {
                state.paint_engine(p, engine, x + width - 48.0, 241.0 - offset, 20);
            }
            let size = if prefs.show_engine_names { 76.0 } else { 47.0 };
            for (index, engine) in default
                .into_iter()
                .chain(
                    engines
                        .iter()
                        .filter(|engine| engine.id != prefs.default_engine),
                )
                .take(3)
                .enumerate()
            {
                let left = x + 14.0 + index as f32 * (size + 8.0);
                let area = rect(
                    left,
                    392.0 - offset,
                    size,
                    if prefs.show_engine_names { 63.0 } else { 48.0 },
                );
                p.rounded(
                    area,
                    6.0,
                    if index == 0 {
                        colors.selection
                    } else {
                        colors.button
                    },
                    false,
                );
                p.rounded(area, 6.0, colors.border, true);
                state.paint_engine(p, engine, left + (size - 20.0) / 2.0, 404.0 - offset, 20);
                if prefs.show_engine_names {
                    p.text(
                        &engine.name,
                        rect(left + 3.0, 429.0 - offset, size - 6.0, 19.0),
                        colors.text,
                        Label::ControlDetail,
                    );
                }
            }
        } else {
            let stride = (width - 28.0) / 5.0;
            for index in 0..5 {
                let left = x + 14.0 + index as f32 * stride;
                let area = rect(left, 144.0 - offset, stride - 6.0, 30.0);
                p.rounded(area, 5.0, colors.button, false);
                p.rounded(area, 5.0, colors.border, true);
                p.text(
                    &format!("Alt+{}", index + 1),
                    rect(left + 5.0, 144.0 - offset, 35.0, 30.0),
                    colors.detail,
                    Label::Hint,
                );
                if let Some(engine) = prefs
                    .pins
                    .get(index)
                    .and_then(|id| engines.iter().find(|e| &e.id == id))
                {
                    state.paint_engine(p, engine, left + 44.0, 152.0 - offset, 14);
                    if stride > 93.0 {
                        p.text(
                            &engine.name,
                            rect(left + 63.0, 144.0 - offset, stride - 69.0, 30.0),
                            colors.text,
                            Label::Hint,
                        );
                    }
                } else {
                    p.text(
                        tr("空"),
                        rect(left + 44.0, 144.0 - offset, stride - 52.0, 30.0),
                        colors.detail,
                        Label::Hint,
                    );
                }
            }
        }
    } else if page == 2 && !unsafe { GetDlgItem(hwnd, 23) }.is_null() {
        let color = u32::from_str_radix(&super::field(hwnd, 23), 16).unwrap_or(colors.accent);
        let area = rect(x + 14.0, 288.0 - offset, 37.0, 37.0);
        let preview = PANEL.with_borrow(|p| p.view.engine_icon.clone());
        if let Some(engine) = preview {
            state.paint_engine(p, &engine, x + 14.0, 288.0 - offset, 37);
        } else {
            p.rounded(area, 6.0, color, false);
            let name: String = super::field(hwnd, 20).chars().take(1).collect();
            p.text(
                if name.is_empty() { "S" } else { &name },
                area,
                WHITE,
                Label::Tile,
            );
        }
    }
}

fn surface_bottom(surface: Surface, height: i32) -> i32 {
    if surface.bottom <= 0 {
        height + surface.bottom
    } else {
        surface.bottom
    }
}

pub(super) fn hovered(hwnd: HWND) -> bool {
    PANEL.with_borrow(|panel| panel.view.hover == hwnd)
}

pub(super) fn control_surface(hwnd: HWND, child: HWND, colors: Palette) -> u32 {
    let viewport = Viewport::of(hwnd);
    let inside = PANEL.with_borrow(|panel| {
        let Some((_, area)) = panel
            .view
            .controls
            .iter()
            .find(|(handle, _)| *handle == child)
        else {
            return false;
        };
        let y = if area.bottom {
            viewport.height + area.y
        } else {
            area.y
        };
        panel
            .view
            .surfaces
            .iter()
            .any(|&surface| y >= surface.top && y < surface_bottom(surface, viewport.height))
    });
    if inside {
        colors.card
    } else {
        colors.background
    }
}

pub(super) fn paint(hwnd: HWND, dc: HDC, invalid: RECT) {
    let viewport = Viewport::of(hwnd);
    let colors = palette();
    unsafe {
        SetDCBrushColor(dc, color_ref(colors.rail));
        FillRect(dc, &invalid, GetStockObject(DC_BRUSH));
        let split = dip(viewport.sidebar, viewport.dpi);
        let background = RECT {
            left: invalid.left.max(split),
            ..invalid
        };
        SetDCBrushColor(dc, color_ref(colors.background));
        FillRect(dc, &background, GetStockObject(DC_BRUSH));
        SetDCBrushColor(dc, color_ref(colors.border));
        FillRect(
            dc,
            &RECT {
                left: split,
                right: split + 1,
                ..invalid
            },
            GetStockObject(DC_BRUSH),
        );
    }
    let (page, editing, scroll, surfaces, fields) = PANEL.with_borrow(|panel| {
        let fields = panel
            .view
            .controls
            .iter()
            .filter_map(|(child, area)| {
                let is_edit = has_class(*child, "EDIT");
                is_edit.then(|| (*child, area.place(viewport, panel.view.scroll)))
            })
            .collect::<Vec<_>>();
        (
            panel.page,
            panel.edit_engine,
            panel.view.scroll,
            panel.view.surfaces.clone(),
            fields,
        )
    });
    let left = dip(viewport.left, viewport.dpi).max(invalid.left);
    let right = dip(viewport.left + viewport.width, viewport.dpi).min(invalid.right);
    let unit = 96.0 / viewport.dpi as f32;
    let layer_x = viewport.left as f32 - left as f32 * unit;
    for top in (invalid.top..invalid.bottom).step_by(96) {
        let stripe = RECT {
            left,
            top,
            right,
            bottom: (top + 96).min(invalid.bottom),
        };
        if !surfaces.iter().any(|surface| {
            dip(surface.top, viewport.dpi) - scroll < stripe.bottom
                && dip(surface_bottom(*surface, viewport.height), viewport.dpi) - scroll
                    > stripe.top
        }) && !fields
            .iter()
            .any(|(_, b)| b.top - 7 < stripe.bottom && b.bottom + 7 > stripe.top)
        {
            continue;
        }
        with_painter(dc, stripe, viewport.dpi, |painter| {
            painter.clear(colors.background);
            let offset = (scroll + top) as f32 * unit;
            for &surface in &surfaces {
                let y = surface.top;
                let end = surface_bottom(surface, viewport.height);
                if end <= y {
                    continue;
                }
                let area = rect(
                    layer_x + 0.5,
                    y as f32 - offset + 0.5,
                    viewport.width as f32 - 1.0,
                    (end - y) as f32 - 1.0,
                );
                painter.rounded(area, 8.0, colors.card, false);
                painter.rounded(area, 8.0, colors.border, true);
                for &line in surface.dividers {
                    if line < end {
                        painter.rounded(
                            rect(
                                layer_x + 1.0,
                                line as f32 - offset,
                                viewport.width as f32 - 2.0,
                                1.0,
                            ),
                            0.0,
                            colors.border,
                            false,
                        );
                    }
                }
            }
            for &(child, b) in &fields {
                let area = rect(
                    (b.left - left) as f32 * unit - 8.0,
                    (b.top - top) as f32 * unit - 6.0,
                    (b.right - b.left) as f32 * unit + 16.0,
                    (b.bottom - b.top) as f32 * unit + 12.0,
                );
                painter.rounded(area, 6.0, colors.button, false);
                painter.rounded(
                    area,
                    6.0,
                    if unsafe { GetFocus() } == child {
                        colors.accent
                    } else {
                        colors.border
                    },
                    true,
                );
            }
            if let Some(state) = crate::native::session() {
                paint_preview(
                    &state, hwnd, painter, colors, viewport, page, editing, layer_x, offset,
                );
            }
        });
    }
}

pub(super) fn control_color(hwnd: HWND, dc: HDC, child: HWND, message: u32) -> LRESULT {
    let viewport = Viewport::of(hwnd);
    let colors = palette();
    let rgb = PANEL.with_borrow(|panel| {
        if message == WM_CTLCOLOREDIT {
            return (colors.button, colors.text);
        }
        if message == WM_CTLCOLORLISTBOX {
            return (
                if control_id(child) == NAV as i32 {
                    colors.rail
                } else {
                    colors.card
                },
                colors.text,
            );
        }
        let Some((_, area)) = panel.view.controls.iter().find(|(h, _)| *h == child) else {
            return (colors.rail, colors.text);
        };
        let inside = panel.view.surfaces.iter().any(|&surface| {
            area.y >= surface.top && area.y < surface_bottom(surface, viewport.height)
        });
        (
            if control_id(child) == 115 {
                colors.button
            } else if inside {
                colors.card
            } else {
                colors.background
            },
            if control_id(child) == super::pages::RECORD_STATUS as i32
                || control_id(child) == super::pages::EDIT_STATUS as i32
                || cfg!(feature = "ocr") && ocr_error_label(control_id(child))
            {
                if crate::native::backdrop::high_contrast() {
                    colors.text
                } else {
                    0x9f2e2a
                }
            } else if matches!(area.font, FontRole::Detail) {
                colors.detail
            } else {
                colors.text
            },
        )
    });
    unsafe {
        SetTextColor(dc, color_ref(rgb.1));
        SetBkColor(dc, color_ref(rgb.0));
        SetBkMode(dc, TRANSPARENT as i32);
        SetDCBrushColor(dc, color_ref(rgb.0));
        GetStockObject(DC_BRUSH) as isize
    }
}

fn ocr_error_label(id: i32) -> bool {
    #[cfg(feature = "ocr")]
    {
        id == super::ocr::STATUS && super::ocr::is_error()
    }
    #[cfg(not(feature = "ocr"))]
    {
        let _ = id;
        false
    }
}

fn control_id(hwnd: HWND) -> i32 {
    unsafe { GetDlgCtrlID(hwnd) }
}

fn item_text(item: &DRAWITEMSTRUCT) -> String {
    let length = unsafe { SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID as usize, 0) };
    if length < 0 {
        return String::new();
    }
    let mut text = vec![0_u16; length as usize + 1];
    unsafe {
        SendMessageW(
            item.hwndItem,
            LB_GETTEXT,
            item.itemID as usize,
            text.as_mut_ptr() as isize,
        )
    };
    String::from_utf16_lossy(&text[..length as usize])
}

pub(super) fn draw_item(state: &State, hwnd: HWND, item: &DRAWITEMSTRUCT) {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let unit = 96.0 / dpi as f32;
    let b = item.rcItem;
    let width = (b.right - b.left) as f32 * unit;
    let height = (b.bottom - b.top) as f32 * unit;
    let colors = palette();
    if item.CtlType == ODT_COMBOBOX {
        super::dropdown::draw_option(item, colors);
        return;
    }
    let (page, nav_hover, hovered) =
        PANEL.with_borrow(|p| (p.page, p.view.nav_hover, p.view.hover == item.hwndItem));
    let id = item.CtlID as usize;
    let index = item.itemID as usize;
    let focused = item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT) == ODS_FOCUS;
    let selected = item.itemState & ODS_SELECTED != 0;
    if id == NAV {
        with_painter(item.hDC, b, dpi, |p| {
            p.clear(colors.rail);
            if let Some(page_id) = nav_pages().nth(index) {
                let label = PAGES[page_id];
                if page_id == page || nav_hover == Some(index) {
                    p.rounded(
                        rect(0.0, 2.0, width - 1.0, height - 4.0),
                        5.0,
                        if page_id == page {
                            colors.selection
                        } else {
                            colors.hover
                        },
                        false,
                    );
                }
                if page_id == page {
                    p.rounded(
                        rect(0.0, 13.0, 3.0, height - 26.0),
                        1.5,
                        colors.accent,
                        false,
                    );
                }
                if focused {
                    p.rounded(
                        rect(2.0, 3.0, width - 5.0, height - 6.0),
                        4.0,
                        colors.accent,
                        true,
                    );
                }
                icon(
                    p,
                    NAV_ICONS[page_id],
                    12.0,
                    (height - 18.0) / 2.0,
                    18.0,
                    dpi,
                    if page_id == page {
                        colors.accent
                    } else {
                        colors.detail
                    },
                );
                p.text(
                    tr(label),
                    rect(40.0, 0.0, width - 46.0, height),
                    colors.text,
                    Label::Body,
                );
            }
        });
    } else if id == 10 {
        let text = item_text(item);
        let browser_handle = if page == 4 && index != 0 {
            text.split_once('\t')
                .map_or(null_mut(), |(_, path)| browser_icon(path))
        } else {
            null_mut()
        };
        with_painter(item.hDC, b, dpi, |p| {
            p.clear(if page == 5 { colors.rail } else { colors.card });
            if page == 5 || selected {
                let inset = if page == 5 { 0.0 } else { 1.0 };
                let area = rect(
                    inset + 0.5,
                    0.5,
                    width - 2.0 * inset - 1.0,
                    height - if page == 5 { 8.0 } else { 1.0 },
                );
                p.rounded(
                    area,
                    6.0,
                    if selected {
                        colors.selection
                    } else {
                        colors.card
                    },
                    false,
                );
                if page == 5 || focused {
                    p.rounded(
                        area,
                        6.0,
                        if focused {
                            colors.accent
                        } else {
                            colors.border
                        },
                        true,
                    );
                }
            }
            match page {
                2 => {
                    let app = state.app.borrow();
                    let index = PANEL.with_borrow(|p| p.engine_rows.get(index).copied());
                    if let Some(engine) = index.and_then(|index| app.engines().get(index)) {
                        let default = engine.id == app.settings().preferences.default_engine;
                        let row = rect(1.0, 1.0, width - 3.0, height - 3.0);
                        if default {
                            p.rounded(row, 6.0, colors.selection, false);
                        }
                        if selected || focused {
                            p.rounded(row, 6.0, colors.accent, true);
                        } else if !default {
                            p.rounded(
                                rect(48.0, height - 1.0, width - 60.0, 1.0),
                                0.0,
                                colors.border,
                                false,
                            );
                        }
                        let edge = dip(22, dpi).max(1) as u32;
                        let area = rect(13.0, 13.0, 22.0, 22.0);
                        if engine.is_custom() && matches!(engine.icon, Icon::Default) {
                            p.rounded(area, 5.0, engine.color, false);
                            p.text(
                                &engine.name.chars().take(1).collect::<String>(),
                                area,
                                WHITE,
                                Label::Tile,
                            );
                        } else {
                            let mut cache = state.images.borrow_mut();
                            p.image(&engine.id, area, edge, || {
                                cache.get(engine, edge).map(Cow::Borrowed)
                            });
                        }
                        p.text(
                            &engine.name,
                            rect(45.0, 0.0, width * 0.39 - 46.0, height),
                            colors.text,
                            Label::Body,
                        );
                        let alias = if engine.alias.is_empty() {
                            "—".into()
                        } else {
                            format!("#{}", engine.alias)
                        };
                        p.text(
                            &alias,
                            rect(width - 536.0 + 221.0 + 17.0, 0.0, 82.0, height),
                            colors.detail,
                            Label::Hint,
                        );
                        let prefs = &app.settings().preferences;
                        let pinned = prefs.pins.iter().position(|id| id == &engine.id);
                        let status = match (default, pinned) {
                            (true, Some(index)) if index < 5 => {
                                format!("{} · Alt+{}", tr("默认"), index + 1)
                            }
                            (true, _) => tr("默认").into(),
                            (false, Some(index)) if index < 5 => format!("Alt+{}", index + 1),
                            (false, Some(_)) => tr("已置顶").into(),
                            _ => String::new(),
                        };
                        p.text(
                            &status,
                            rect(width - 128.0, 0.0, 91.0, height),
                            colors.accent,
                            Label::Hint,
                        );
                        for dot in 0..3 {
                            p.rounded(
                                rect(
                                    width - 25.0 + dot as f32 * 4.0,
                                    height * 0.5 - 1.0,
                                    2.0,
                                    2.0,
                                ),
                                1.0,
                                colors.text,
                                false,
                            );
                        }
                    } else {
                        p.text(
                            &text,
                            rect(15.0, 0.0, width - 30.0, height),
                            colors.detail,
                            Label::Body,
                        );
                    }
                }
                5 => {
                    let (query, detail) = text.split_once('\t').unwrap_or((&text, ""));
                    p.text(
                        query,
                        rect(16.0, 9.0, width - 187.0, 23.0),
                        colors.text,
                        Label::Body,
                    );
                    p.text(
                        detail,
                        rect(16.0, 32.0, width - 187.0, 19.0),
                        colors.detail,
                        Label::Hint,
                    );
                    p.text(
                        tr("再次搜索"),
                        rect(width - 165.0, 18.0, 84.0, 25.0),
                        colors.accent,
                        Label::ControlDetail,
                    );
                    p.text(
                        tr("删除"),
                        rect(width - 69.0, 18.0, 54.0, 25.0),
                        colors.detail,
                        Label::ControlDetail,
                    );
                }
                _ => {
                    let (name, detail) = text.split_once('\t').unwrap_or((&text, ""));
                    let center = height / 2.0;
                    let radio = rect(13.0, center - 7.0, 14.0, 14.0);
                    p.rounded(
                        radio,
                        7.0,
                        if selected {
                            colors.accent
                        } else {
                            colors.detail
                        },
                        true,
                    );
                    if selected {
                        p.rounded(
                            rect(17.0, center - 3.0, 6.0, 6.0),
                            3.0,
                            colors.accent,
                            false,
                        );
                    }
                    if browser_handle.is_null() {
                        icon(
                            p,
                            NAV_ICONS[4],
                            36.0,
                            center - 9.0,
                            18.0,
                            dpi,
                            colors.detail,
                        );
                    }
                    p.text(
                        name,
                        rect(64.0, 4.0, width - 77.0, 23.0),
                        colors.text,
                        Label::Body,
                    );
                    p.text(
                        detail,
                        rect(64.0, 27.0, width - 77.0, 18.0),
                        colors.detail,
                        Label::Hint,
                    );
                }
            }
        });
        if !browser_handle.is_null() {
            let size = dip(18, dpi);
            unsafe {
                DrawIconEx(
                    item.hDC,
                    b.left + dip(36, dpi),
                    (b.top + b.bottom - size) / 2,
                    browser_handle,
                    size,
                    size,
                    0,
                    null_mut(),
                    DI_NORMAL,
                );
            }
        }
    } else {
        let disabled = item.itemState & ODS_DISABLED != 0;
        let label = text_of(item.hwndItem);
        let toggle = (page == 0 && matches!(id, 100..=102)) || id == 150 || id == 170;
        let on = label.ends_with(&format!(" ({})", tr("开")));
        let selected_mode = [110, 114].contains(&id)
            && state.app.borrow().settings().preferences.show_engine_names == (id == 110);
        let primary = !disabled && (matches!(id, 122 | 126 | 141 | 148) || selected_mode);
        with_painter(item.hDC, b, dpi, |p| {
            p.clear(control_surface(hwnd, item.hwndItem, colors));
            if toggle {
                let bounds = rect(1.0, (height - 22.0) / 2.0, width - 2.0, 22.0);
                let fill = if on {
                    colors.accent
                } else if hovered {
                    colors.hover
                } else {
                    colors.button
                };
                p.rounded(bounds, 11.0, fill, false);
                p.rounded(
                    bounds,
                    11.0,
                    if focused || on {
                        colors.accent
                    } else {
                        colors.detail
                    },
                    true,
                );
                let progress = PANEL.with_borrow(|p| {
                    p.view
                        .toggle_motion
                        .filter(|(hwnd, _)| *hwnd == item.hwndItem)
                        .map_or(1.0, |(_, start)| {
                            (start.elapsed().as_secs_f32() / 0.12).min(1.0)
                        })
                });
                let eased = progress * (2.0 - progress);
                let amount = if on { eased } else { 1.0 - eased };
                let handle = 6.0 + (width - 23.0) * amount;
                p.rounded(
                    rect(handle, (height - 10.0) / 2.0, 10.0, 10.0),
                    5.0,
                    if on {
                        colors.primary_text
                    } else {
                        colors.detail
                    },
                    false,
                );
                if focused {
                    p.rounded(
                        rect(0.5, 0.5, width - 1.0, height - 1.0),
                        6.0,
                        colors.accent,
                        true,
                    );
                }
                return;
            }
            let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
            let fill = if disabled {
                colors.card
            } else if primary {
                colors.accent
            } else if selected {
                0xdee7f5
            } else if hovered {
                colors.hover
            } else {
                colors.button
            };
            p.rounded(area, 6.0, fill, false);
            p.rounded(
                area,
                6.0,
                if focused || primary {
                    colors.accent
                } else {
                    colors.border
                },
                true,
            );
            if primary && (selected || hovered) {
                p.rounded_alpha(area, 6.0, 0, if selected { 0.18 } else { 0.10 }, false);
            }
            let text_color = if disabled {
                colors.detail
            } else if primary {
                colors.primary_text
            } else {
                colors.text
            };
            if matches!(id, 137..=139 | 156)
                && (label == "双击左 Alt" || label.contains('+') || label.starts_with("0x"))
            {
                let recording =
                    PANEL.with_borrow(|p| p.recording.is_some_and(|(target, _)| target == id));
                let caps: Vec<_> = if label == "双击左 Alt" {
                    vec![tr("左 Alt"), "× 2"]
                } else {
                    label.split('+').collect()
                };
                let available = width - if recording { 12.0 } else { 61.0 };
                let stride = 63.0_f32.min(available / caps.len() as f32);
                let start = 6.0 + (available - stride * caps.len() as f32) / 2.0;
                for (i, cap) in caps.into_iter().enumerate() {
                    let area = rect(
                        start + i as f32 * stride + 2.0,
                        5.0,
                        stride - 4.0,
                        height - 10.0,
                    );
                    p.rounded(area, 4.0, colors.card, false);
                    p.rounded(area, 4.0, colors.border, true);
                    p.text(cap, area, colors.text, Label::ControlDetail);
                }
                if !recording {
                    p.text(
                        tr("更改"),
                        rect(width - 49.0, 0.0, 44.0, height),
                        colors.accent,
                        Label::Control,
                    );
                }
            } else {
                p.text(
                    &label,
                    rect(4.0, 0.0, width - 8.0, height),
                    text_color,
                    Label::Control,
                );
            }
        });
    }
}
