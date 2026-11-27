use super::components::CONTROL_HEIGHT;
use super::fluent::{self, Layout, Palette};
use super::scrollbar::{self, Motion};
use super::{child, wide};
use crate::native::paint::{Label, Painter, rect};
use crate::native::{color_ref, dip};
use std::ptr::null;
use std::time::Instant;
use std::{borrow::Cow, cell::RefCell};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{
        Controls::{
            COMBOBOXINFO, DRAWITEMSTRUCT, GetComboBoxInfo, ODS_COMBOBOXEDIT, ShowScrollBar,
            WM_MOUSELEAVE,
        },
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            GetFocus, IsWindowEnabled, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DOWN,
            VK_ESCAPE, VK_F4, VK_RETURN, VK_SPACE,
        },
        Shell::{DefSubclassProc, GetWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const CHEVRON_DOWN: &[u8] = include_bytes!("../../../assets/icons/tabler/chevron-down.svg");
const CHEVRON_UP: &[u8] = include_bytes!("../../../assets/icons/tabler/chevron-up.svg");
const CHECK: &[u8] = include_bytes!("../../../assets/icons/tabler/check.svg");
// Win32 adds six physical pixels to the selection field height.
const NATIVE_FACE_INSET_PX: i32 = 6;
const OPTION_INSET: f32 = 4.0;
const MAX_VISIBLE: usize = 7;
const FACE_SUBCLASS: usize = 2;
const NO_SAVED_SELECTION: usize = usize::MAX - 1;
const POPUP_SUBCLASS: usize = 1;
const NO_HOVER: usize = usize::MAX;
const MOUSE_LEFT_BUTTON: WPARAM = 0x0001;
const OPEN_TIMER: usize = 21;

struct PopupScroll {
    hwnd: HWND,
    combo: HWND,
    motion: Motion,
    drag: Option<i32>,
    wheel: i32,
    opening: Option<Instant>,
}

thread_local! {
    static POPUP_SCROLL: RefCell<Option<PopupScroll>> = const { RefCell::new(None) };
}

pub(super) fn create(
    parent: HWND,
    id: usize,
    values: impl IntoIterator<Item = impl AsRef<str>>,
    selected: usize,
    area: Layout,
) -> HWND {
    let hwnd = child(
        parent,
        windows_sys::w!("COMBOBOX"),
        "",
        WS_TABSTOP
            | (CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS | CBS_NOINTEGRALHEIGHT)
                as u32,
        id,
        area,
    );
    if hwnd.is_null() {
        return hwnd;
    }
    unsafe {
        for value in values {
            SendMessageW(
                hwnd,
                CB_ADDSTRING,
                0,
                wide(value.as_ref()).as_ptr() as isize,
            );
        }
        SendMessageW(hwnd, CB_SETCURSEL, selected, 0);
        SetWindowSubclass(hwnd, Some(face_proc), FACE_SUBCLASS, NO_SAVED_SELECTION);
    }
    hwnd
}

pub(super) fn is_combo(hwnd: HWND) -> bool {
    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetComboBoxInfo(hwnd, &mut info) != 0 && info.hwndCombo == hwnd }
}

pub(super) fn dropped(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, CB_GETDROPPEDSTATE, 0, 0) != 0 }
}

pub(super) fn sync_metrics(hwnd: HWND, dpi: u32) -> i32 {
    unsafe {
        let row = dip(CONTROL_HEIGHT, dpi);
        for (index, height) in [(usize::MAX, row - NATIVE_FACE_INSET_PX), (0, row)] {
            if SendMessageW(hwnd, CB_GETITEMHEIGHT, index, 0) != height as isize {
                SendMessageW(hwnd, CB_SETITEMHEIGHT, index, height as isize);
            }
        }
        let count = SendMessageW(hwnd, CB_GETCOUNT, 0, 0).clamp(1, MAX_VISIBLE as isize) as i32;
        row * (1 + count) + dip(5, dpi)
    }
}

fn popup_for(hwnd: HWND) -> HWND {
    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetComboBoxInfo(hwnd, &mut info) } != 0 {
        info.hwndList
    } else {
        std::ptr::null_mut()
    }
}

fn style_popup(hwnd: HWND) {
    let popup = popup_for(hwnd);
    if popup.is_null() {
        return;
    }
    unsafe {
        POPUP_SCROLL.with_borrow_mut(|state| {
            *state = Some(PopupScroll {
                hwnd: popup,
                combo: hwnd,
                motion: Motion::start(),
                drag: None,
                wheel: 0,
                opening: fluent::animations_enabled().then(Instant::now),
            })
        });
        SetWindowSubclass(popup, Some(popup_proc), POPUP_SUBCLASS, NO_HOVER);
        ShowScrollBar(popup, SB_VERT, 0);
        wake_scrollbar(popup);
        let outline = fluent::palette().border;
        let corner = DWMWCP_ROUNDSMALL as i32;
        DwmSetWindowAttribute(
            popup,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&corner as *const i32).cast(),
            size_of_val(&corner) as u32,
        );
        let border = color_ref(outline);
        DwmSetWindowAttribute(
            popup,
            DWMWA_BORDER_COLOR as u32,
            (&border as *const u32).cast(),
            size_of_val(&border) as u32,
        );
    }
}

struct ScrollMetrics {
    bounds: RECT,
    dpi: u32,
    top: i32,
    maximum: i32,
    page: i32,
}

impl ScrollMetrics {
    fn of(hwnd: HWND) -> Option<Self> {
        unsafe {
            let mut bounds = RECT::default();
            GetClientRect(hwnd, &mut bounds);
            let row = SendMessageW(hwnd, LB_GETITEMHEIGHT, 0, 0) as i32;
            let count = SendMessageW(hwnd, LB_GETCOUNT, 0, 0) as i32;
            if row <= 0 || bounds.bottom < row {
                return None;
            }
            let page = bounds.bottom / row;
            let maximum = count - page;
            (maximum > 0).then(|| Self {
                bounds,
                dpi: GetDpiForWindow(hwnd).max(96),
                maximum,
                page,
                top: (SendMessageW(hwnd, LB_GETTOPINDEX, 0, 0) as i32).clamp(0, maximum),
            })
        }
    }

    fn track(&self) -> RECT {
        let edge = dip(3, self.dpi);
        RECT {
            left: self.bounds.right - dip(scrollbar::HIT, self.dpi),
            right: self.bounds.right,
            top: edge,
            bottom: self.bounds.bottom - edge,
        }
    }

    fn thumb(&self) -> (i32, i32) {
        let track = self.track();
        let (start, length) = scrollbar::thumb(
            self.top,
            self.maximum,
            self.page,
            track.bottom - track.top,
            self.dpi,
        );
        (track.top + start, length)
    }

    fn set_top(&self, hwnd: HWND, position: i32) {
        unsafe {
            SendMessageW(
                hwnd,
                LB_SETTOPINDEX,
                position.clamp(0, self.maximum) as usize,
                0,
            );
        }
    }
}

fn popup_state<T>(hwnd: HWND, action: impl FnOnce(&mut PopupScroll) -> T) -> Option<T> {
    POPUP_SCROLL
        .with_borrow_mut(|slot| slot.as_mut().filter(|state| state.hwnd == hwnd).map(action))
}

fn clear_scrollbar(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    unsafe {
        KillTimer(hwnd, scrollbar::TIMER);
        KillTimer(hwnd, OPEN_TIMER);
        SetWindowRgn(hwnd, std::ptr::null_mut(), 0);
    }
    POPUP_SCROLL.with_borrow_mut(|state| {
        if state.as_ref().is_some_and(|state| state.hwnd == hwnd) {
            *state = None;
        }
    });
}

fn popup_top(anchor: RECT, work: RECT, height: i32, gap: i32) -> i32 {
    let below = anchor.bottom + gap;
    let above = anchor.top - height - gap;
    let last = (work.bottom - height).max(work.top);
    if below <= last {
        below
    } else {
        above.clamp(work.top, last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_prefers_below_then_flips_and_clamps() {
        let work = RECT {
            top: -600,
            bottom: 400,
            ..Default::default()
        };
        let anchor = |top| RECT {
            top,
            bottom: top + 34,
            ..Default::default()
        };
        assert_eq!(popup_top(anchor(-300), work, 240, 4), -262);
        assert_eq!(popup_top(anchor(250), work, 240, 4), 6);
        assert_eq!(popup_top(anchor(250), work, 980, 4), -600);
    }
}

unsafe fn position_popup(hwnd: HWND, bounds: &mut WINDOWPOS) {
    unsafe {
        let Some(combo) = popup_state(hwnd, |state| state.combo) else {
            return;
        };
        let mut anchor = RECT::default();
        if GetWindowRect(combo, &mut anchor) == 0 {
            return;
        }
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(
            MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        ) == 0
        {
            return;
        }
        let mut popup = RECT::default();
        GetWindowRect(hwnd, &mut popup);
        let width = if bounds.flags & SWP_NOSIZE == 0 {
            bounds.cx
        } else {
            popup.right - popup.left
        };
        let height = if bounds.flags & SWP_NOSIZE == 0 {
            bounds.cy
        } else {
            popup.bottom - popup.top
        };
        let work = monitor.rcWork;
        bounds.x = anchor
            .left
            .clamp(work.left, (work.right - width).max(work.left));
        bounds.y = popup_top(anchor, work, height, dip(4, GetDpiForWindow(combo)));
        bounds.flags &= !SWP_NOMOVE;
    }
}

fn reveal_popup(hwnd: HWND) {
    let Some((combo, time)) =
        popup_state(hwnd, |state| state.opening.map(|time| (state.combo, time))).flatten()
    else {
        return;
    };
    unsafe {
        let progress = (time.elapsed().as_secs_f32() / 0.2).min(1.0);
        if progress >= 1.0 {
            popup_state(hwnd, |state| state.opening = None);
            SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
            KillTimer(hwnd, OPEN_TIMER);
            return;
        }
        let mut popup = RECT::default();
        let mut anchor = RECT::default();
        GetWindowRect(hwnd, &mut popup);
        GetWindowRect(combo, &mut anchor);
        let height = popup.bottom - popup.top;
        let width = popup.right - popup.left;
        let reveal =
            ((0.35 + 0.65 * fluent::easing(progress, 0.0, 0.38, 0.9)) * height as f32) as i32;
        let (top, bottom) = if popup.top < anchor.top {
            (height - reveal, height)
        } else {
            (0, reveal)
        };
        let clip = CreateRectRgn(0, top, width, bottom);
        if !clip.is_null() && SetWindowRgn(hwnd, clip, 1) == 0 {
            DeleteObject(clip);
        }
    }
}

fn wake_scrollbar(hwnd: HWND) {
    popup_state(hwnd, |state| state.motion.wake(hwnd));
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

fn paint_popup(hwnd: HWND, dc: HDC, update: RECT) {
    let Some(combo) = popup_state(hwnd, |state| state.combo) else {
        return;
    };
    unsafe {
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let unit = 96.0 / dpi as f32;
        let colors = fluent::palette();
        let metrics = ScrollMetrics::of(hwnd);
        let width = bounds.right as f32 * unit;
        let scrollbar_space = if metrics.is_some() {
            scrollbar::HIT as f32
        } else {
            0.0
        };
        let preview = SendMessageW(combo, CB_GETCURSEL, 0, 0) as usize;
        let mut selection = NO_SAVED_SELECTION;
        let mut hover = NO_HOVER;
        GetWindowSubclass(combo, Some(face_proc), FACE_SUBCLASS, &mut selection);
        GetWindowSubclass(hwnd, Some(popup_proc), POPUP_SUBCLASS, &mut hover);
        let count = SendMessageW(hwnd, LB_GETCOUNT, 0, 0);
        let first = SendMessageW(hwnd, LB_GETTOPINDEX, 0, 0).max(0);
        fluent::with_painter(dc, bounds, dpi, |p| {
            p.clear(colors.card);
            for index in first as usize..count.max(0) as usize {
                let mut row = RECT::default();
                if SendMessageW(
                    hwnd,
                    LB_GETITEMRECT,
                    index,
                    (&mut row as *mut RECT) as isize,
                ) < 0
                    || row.top >= update.bottom
                {
                    break;
                }
                if row.bottom <= update.top {
                    continue;
                }
                paint_option(
                    p,
                    colors,
                    combo,
                    index,
                    rect(
                        0.0,
                        row.top as f32 * unit,
                        width,
                        (row.bottom - row.top) as f32 * unit,
                    ),
                    index == hover,
                    index
                        == if selection == NO_SAVED_SELECTION {
                            preview
                        } else {
                            selection
                        },
                    index == preview,
                    scrollbar_space,
                    dpi,
                    IsWindowEnabled(combo) == 0,
                );
            }
            if let Some(metrics) = metrics {
                let (start, length) = metrics.thumb();
                popup_state(hwnd, |state| {
                    state.motion.draw(
                        p,
                        width - OPTION_INSET,
                        start as f32 * unit,
                        length as f32 * unit,
                    )
                });
            }
        });
    }
}

fn scroll_wheel(hwnd: HWND, wp: WPARAM) {
    let Some(metrics) = ScrollMetrics::of(hwnd) else {
        return;
    };
    let notches = popup_state(hwnd, |state| {
        state.wheel += i32::from((wp >> 16) as i16);
        let whole = state.wheel / 120;
        state.wheel %= 120;
        whole
    })
    .unwrap_or(0);
    let mut lines = 3u32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETWHEELSCROLLLINES,
            0,
            (&mut lines as *mut u32).cast(),
            0,
        )
    };
    let step = if lines == u32::MAX {
        metrics.page
    } else {
        lines.min(100) as i32
    };
    let top = (metrics.top - notches * step).clamp(0, metrics.maximum);
    if metrics.top != top {
        metrics.set_top(hwnd, top);
    } else {
        wake_scrollbar(hwnd);
    }
}

fn rail_input(hwnd: HWND, message: u32, point: POINT, track_edge: bool) -> bool {
    let Some(metrics) = ScrollMetrics::of(hwnd) else {
        return false;
    };
    let track = metrics.track();
    let inside = unsafe { PtInRect(&track, point) } != 0;
    let edge = track_edge
        && point.x >= metrics.bounds.right - dip(20, metrics.dpi)
        && point.x < metrics.bounds.right
        && (0..metrics.bounds.bottom).contains(&point.y);
    let (start, length) = metrics.thumb();
    let Some((handled, position, changed)) = popup_state(hwnd, |state| {
        let mut position = None;
        let mut handled = inside || state.drag.is_some();
        match message {
            WM_LBUTTONDOWN if inside => {
                if (start..start + length).contains(&point.y) {
                    state.drag = Some(point.y - start);
                } else {
                    let direction = if point.y < start { -1 } else { 1 };
                    position = Some(metrics.top + direction * ((metrics.page * 9 + 5) / 10).max(1));
                    // Suppress the matching button-up even if it lands on a row.
                    state.drag = Some(-1);
                }
            }
            WM_MOUSEMOVE if let Some(grab) = state.drag.filter(|grab| *grab >= 0) => {
                let travel = (track.bottom - track.top - length).max(1);
                position = Some(scrollbar::project(
                    (point.y - track.top - grab).clamp(0, travel),
                    travel,
                    metrics.maximum,
                ));
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE => {
                handled |= state.drag.take().is_some();
            }
            _ => {}
        }
        let changed = state.motion.update(
            hwnd,
            edge,
            inside,
            state.drag.is_some_and(|grab| grab >= 0),
            handled && matches!(message, WM_LBUTTONDOWN | WM_LBUTTONUP),
        );
        (handled, position, changed)
    }) else {
        return false;
    };
    if let Some(top) = position {
        metrics.set_top(hwnd, top);
    }
    if changed {
        unsafe { InvalidateRect(hwnd, &track, 0) };
    }
    handled
}

fn mouse_point(lp: LPARAM) -> POINT {
    POINT {
        x: i32::from(lp as i16),
        y: i32::from((lp >> 16) as i16),
    }
}

fn watch_leave(hwnd: HWND) {
    let mut track = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe { TrackMouseEvent(&mut track) };
}

fn option_text(hwnd: HWND, index: usize) -> String {
    unsafe {
        let count = SendMessageW(hwnd, CB_GETLBTEXTLEN, index, 0);
        if count <= 0 {
            return String::new();
        }
        let mut units = vec![0u16; count as usize + 1];
        let read = SendMessageW(hwnd, CB_GETLBTEXT, index, units.as_mut_ptr() as isize);
        String::from_utf16_lossy(&units[..read.max(0) as usize])
    }
}

fn paint_value(p: &mut Painter, combo: HWND, index: usize, area: D2D_RECT_F, color: u32) {
    let value = option_text(combo, index);
    if unsafe { GetDlgCtrlID(combo) } == super::fonts::FAMILY as i32 {
        super::fonts::paint_family(p, index, &value, area, color);
    } else {
        p.text(&value, area, color, Label::Body);
    }
}

fn svg(
    painter: &mut Painter,
    name: &str,
    source: &[u8],
    x: f32,
    y: f32,
    length: f32,
    dpi: u32,
    color: u32,
) {
    let px = (length * dpi as f32 / 96.0).round() as u32;
    let key = format!("combo-{name}-{color:06x}");
    painter.image(&key, rect(x, y, length, length), px, || {
        yisou_svg::render(source, px, Some(color)).map(Cow::Owned)
    });
}

fn paint_face(hwnd: HWND, dc: HDC) {
    unsafe {
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let scale = 96.0 / dpi as f32;
        let width = bounds.right as f32 * scale;
        let height = bounds.bottom as f32 * scale;
        let colors = fluent::palette();
        let enabled = IsWindowEnabled(hwnd) != 0;
        let focused = GetFocus() == hwnd;
        let open = dropped(hwnd);
        let hover = fluent::hover_amount(hwnd);
        let index = SendMessageW(hwnd, CB_GETCURSEL, 0, 0);
        fluent::with_painter(dc, bounds, dpi, |p| {
            p.clear(fluent::control_surface(GetParent(hwnd), hwnd, colors));
            let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
            fluent::paint_button_face(
                p,
                area,
                colors,
                fluent::ButtonStyle::Standard,
                fluent::ButtonState {
                    hover: if open { 1.0 } else { hover },
                    disabled: !enabled,
                    ..fluent::ButtonState::default()
                },
            );
            if focused || open {
                p.rounded(
                    rect(2.5, height - 2.0, width - 5.0, 1.5),
                    0.7,
                    colors.accent,
                    false,
                );
            }
            paint_value(
                p,
                hwnd,
                index as usize,
                rect(11.0, 0.0, (width - 41.0).max(0.0), height),
                if enabled { colors.text } else { colors.detail },
            );
            let (name, arrow) = if open {
                ("open", CHEVRON_UP)
            } else {
                ("closed", CHEVRON_DOWN)
            };
            svg(
                p,
                name,
                arrow,
                width - 25.0,
                (height - 14.0) / 2.0,
                14.0,
                dpi,
                colors.detail,
            );
        });
    }
}

pub(super) fn draw_option(item: &DRAWITEMSTRUCT) {
    if item.itemState & ODS_COMBOBOXEDIT == 0 {
        let popup = popup_for(item.hwndItem);
        if !popup.is_null() {
            unsafe { InvalidateRect(popup, &item.rcItem, 0) };
        }
    }
}

fn paint_option(
    p: &mut Painter,
    colors: Palette,
    combo: HWND,
    index: usize,
    area: D2D_RECT_F,
    hovered: bool,
    current: bool,
    preview: bool,
    scrollbar_space: f32,
    dpi: u32,
    disabled: bool,
) {
    let y = area.top;
    let width = area.right;
    let content_width = width - scrollbar_space;
    let height = area.bottom - y;
    if hovered || current {
        let fill = if current {
            colors.selection
        } else {
            colors.hover
        };
        p.rounded(
            rect(
                OPTION_INSET,
                y + 2.0,
                width - 2.0 * OPTION_INSET,
                height - 4.0,
            ),
            4.0,
            fill,
            false,
        );
    }
    if preview && !current {
        p.rounded(
            rect(
                OPTION_INSET + 0.5,
                y + 2.5,
                width - 2.0 * OPTION_INSET - 1.0,
                height - 5.0,
            ),
            4.0,
            colors.border,
            true,
        );
    }
    if current {
        p.rounded(
            rect(OPTION_INSET, y + (height - 14.0) / 2.0, 2.5, 14.0),
            1.0,
            colors.accent,
            false,
        );
        svg(
            p,
            "check",
            CHECK,
            content_width - 25.0,
            y + (height - 14.0) / 2.0,
            14.0,
            dpi,
            colors.accent,
        );
    }
    paint_value(
        p,
        combo,
        index,
        rect(12.0, y, (content_width - 42.0).max(0.0), height),
        if disabled { colors.detail } else { colors.text },
    );
}

fn popup_row(hwnd: HWND, point: POINT) -> usize {
    let mut client = RECT::default();
    let mut row = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut client);
        if PtInRect(&client, point) != 0 {
            let position = ((point.y as u16 as u32) << 16 | point.x as u16 as u32) as LPARAM;
            let hit = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, position);
            let index = (hit & 0xffff) as usize;
            if hit >> 16 == 0
                && SendMessageW(
                    hwnd,
                    LB_GETITEMRECT,
                    index,
                    (&mut row as *mut RECT) as LPARAM,
                ) != LB_ERR as isize
                && PtInRect(&row, point) != 0
            {
                return index;
            }
        }
    }
    NO_HOVER
}

fn update_hover(hwnd: HWND, hover: usize) {
    unsafe {
        let mut previous = NO_HOVER;
        if GetWindowSubclass(hwnd, Some(popup_proc), POPUP_SUBCLASS, &mut previous) == 0
            || previous == hover
        {
            return;
        }
        SetWindowSubclass(hwnd, Some(popup_proc), POPUP_SUBCLASS, hover);
        if hover != NO_HOVER && previous == NO_HOVER {
            watch_leave(hwnd);
        }
        for index in [previous, hover] {
            let mut row = RECT::default();
            if index != NO_HOVER
                && SendMessageW(
                    hwnd,
                    LB_GETITEMRECT,
                    index,
                    (&mut row as *mut RECT) as LPARAM,
                ) != LB_ERR as isize
            {
                InvalidateRect(hwnd, &row, 0);
            }
        }
    }
}

unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_WINDOWPOSCHANGING
                if lp != 0 && (*(lp as *const WINDOWPOS)).flags & SWP_SHOWWINDOW != 0 =>
            {
                position_popup(hwnd, &mut *(lp as *mut WINDOWPOS));
            }
            WM_NCDESTROY => clear_scrollbar(hwnd),
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                paint_popup(hwnd, dc, ps.rcPaint);
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_PRINTCLIENT => {
                let mut bounds = RECT::default();
                GetClientRect(hwnd, &mut bounds);
                paint_popup(hwnd, wp as HDC, bounds);
                return 0;
            }
            WM_SHOWWINDOW if wp != 0 => {
                wake_scrollbar(hwnd);
                if popup_state(hwnd, |state| state.opening.is_some()) == Some(true) {
                    SetTimer(hwnd, OPEN_TIMER, 15, None);
                }
            }
            WM_TIMER if wp == OPEN_TIMER => {
                reveal_popup(hwnd);
                return 0;
            }
            WM_TIMER if wp == scrollbar::TIMER => {
                if popup_state(hwnd, |state| state.motion.tick(hwnd)) == Some(true)
                    && let Some(metrics) = ScrollMetrics::of(hwnd)
                {
                    InvalidateRect(hwnd, &metrics.track(), 0);
                }
                return 0;
            }
            WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP => {
                let point = mouse_point(lp);
                let rail = rail_input(hwnd, msg, point, true);
                update_hover(
                    hwnd,
                    if rail {
                        NO_HOVER
                    } else {
                        popup_row(hwnd, point)
                    },
                );
                if rail {
                    watch_leave(hwnd);
                    return 0;
                }
                // Native click and drag still handle selection. Idle motion is visual only.
                if msg == WM_MOUSEMOVE && wp & MOUSE_LEFT_BUTTON == 0 {
                    return 0;
                }
            }
            WM_MOUSELEAVE | WM_CAPTURECHANGED | WM_CANCELMODE => {
                rail_input(hwnd, msg, POINT { x: -1, y: -1 }, false);
                update_hover(hwnd, NO_HOVER);
            }
            WM_MOUSEWHEEL => {
                update_hover(hwnd, NO_HOVER);
                scroll_wheel(hwnd, wp);
                return 0;
            }
            LB_SETTOPINDEX | WM_VSCROLL => {
                // The native list otherwise scrolls the pixels of the old frame, including the thumb.
                let visible = IsWindowVisible(hwnd) != 0;
                if visible {
                    DefSubclassProc(hwnd, WM_SETREDRAW, 0, 0);
                }
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                if visible {
                    DefSubclassProc(hwnd, WM_SETREDRAW, 1, 0);
                }
                update_hover(hwnd, NO_HOVER);
                wake_scrollbar(hwnd);
                return result;
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

pub(super) fn notice(hwnd: HWND, code: u32) {
    unsafe {
        match code {
            CBN_DROPDOWN => {
                let selection = SendMessageW(hwnd, CB_GETCURSEL, 0, 0) as usize;
                SetWindowSubclass(hwnd, Some(face_proc), FACE_SUBCLASS, selection);
                style_popup(hwnd);
            }
            CBN_SELENDCANCEL => {
                let mut selection = NO_SAVED_SELECTION;
                if GetWindowSubclass(hwnd, Some(face_proc), FACE_SUBCLASS, &mut selection) != 0
                    && selection != NO_SAVED_SELECTION
                {
                    SendMessageW(hwnd, CB_SETCURSEL, selection, 0);
                }
            }
            CBN_CLOSEUP => {
                clear_scrollbar(popup_for(hwnd));
                SetWindowSubclass(hwnd, Some(face_proc), FACE_SUBCLASS, NO_SAVED_SELECTION);
            }
            _ => {}
        }
    }
    if matches!(
        code,
        CBN_DROPDOWN | CBN_CLOSEUP | CBN_SELCHANGE | CBN_SELENDOK | CBN_SELENDCANCEL
    ) {
        unsafe {
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

unsafe extern "system" fn face_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                paint_face(hwnd, dc);
                EndPaint(hwnd, &paint);
                return 0;
            }
            WM_PRINTCLIENT => {
                paint_face(hwnd, wp as HDC);
                return 0;
            }
            WM_GETDLGCODE => {
                let key = if lp != 0 {
                    (*(lp as *const MSG)).wParam as u16
                } else {
                    wp as u16
                };
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                if matches!(key, VK_RETURN | VK_SPACE | VK_F4) || dropped(hwnd) && key == VK_ESCAPE
                {
                    return result | DLGC_WANTMESSAGE as isize;
                }
                return result;
            }
            WM_KEYDOWN if matches!(wp as u16, VK_RETURN | VK_SPACE) && !dropped(hwnd) => {
                SendMessageW(hwnd, CB_SHOWDROPDOWN, 1, 0);
                return 0;
            }
            WM_SYSKEYDOWN if wp as u16 == VK_DOWN && !dropped(hwnd) => {
                SendMessageW(hwnd, CB_SHOWDROPDOWN, 1, 0);
                return 0;
            }
            WM_MOUSEWHEEL if !dropped(hwnd) => return SendMessageW(GetParent(hwnd), msg, wp, lp),
            WM_MOUSEWHEEL => {
                let popup = popup_for(hwnd);
                if !popup.is_null() {
                    update_hover(popup, NO_HOVER);
                    scroll_wheel(popup, wp);
                }
                return 0;
            }
            WM_KEYDOWN | WM_CHAR if dropped(hwnd) => {
                let previous = SendMessageW(hwnd, CB_GETCURSEL, 0, 0);
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                let preview = SendMessageW(hwnd, CB_GETCURSEL, 0, 0);
                if preview >= 0 && preview != previous && dropped(hwnd) {
                    let popup = popup_for(hwnd);
                    let mut client = RECT::default();
                    let mut row = RECT::default();
                    GetClientRect(popup, &mut client);
                    if SendMessageW(
                        popup,
                        LB_GETITEMRECT,
                        preview as usize,
                        (&mut row as *mut RECT) as LPARAM,
                    ) != LB_ERR as isize
                        && (row.top < client.top || row.bottom > client.bottom)
                    {
                        SendMessageW(popup, LB_SETTOPINDEX, preview as usize, 0);
                    }
                    update_hover(popup, NO_HOVER);
                    wake_scrollbar(popup);
                    InvalidateRect(hwnd, null(), 0);
                }
                return result;
            }
            _ => {}
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if matches!(
            msg,
            WM_SETFOCUS
                | WM_KILLFOCUS
                | WM_ENABLE
                | CB_SETCURSEL
                | CB_SHOWDROPDOWN
                | WM_KEYDOWN
                | WM_SYSKEYDOWN
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
        ) {
            InvalidateRect(hwnd, null(), 0);
        }
        result
    }
}
