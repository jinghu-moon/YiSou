use super::fluent::{self, Layout, Palette};
use super::{child, wide};
use crate::native::paint::{Label, Painter, rect};
use crate::native::{color_ref, dip};
use std::borrow::Cow;
use std::ptr::null;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{
        Controls::{
            COMBOBOXINFO, DRAWITEMSTRUCT, GetComboBoxInfo, ODS_COMBOBOXEDIT, ODS_DISABLED,
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

const CHEVRON_DOWN: &[u8] = include_bytes!("../../../../assets/icons/tabler/chevron-down.svg");
const CHEVRON_UP: &[u8] = include_bytes!("../../../../assets/icons/tabler/chevron-up.svg");
const CHECK: &[u8] = include_bytes!("../../../../assets/icons/tabler/check.svg");
const ROW_DIP: i32 = 34;
const FACE_DIP: i32 = 34;
const MAX_VISIBLE: usize = 7;
const FACE_SUBCLASS: usize = 2;
const NO_SAVED_SELECTION: usize = usize::MAX - 1;
const POPUP_SUBCLASS: usize = 1;
const NO_HOVER: usize = usize::MAX;
const MOUSE_LEFT_BUTTON: WPARAM = 0x0001;

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
            | WS_VSCROLL
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
        for (index, height) in [(usize::MAX, FACE_DIP - 6), (0, ROW_DIP)] {
            let height = dip(height, dpi);
            if SendMessageW(hwnd, CB_GETITEMHEIGHT, index, 0) != height as isize {
                SendMessageW(hwnd, CB_SETITEMHEIGHT, index, height as isize);
            }
        }
        let count = SendMessageW(hwnd, CB_GETCOUNT, 0, 0).clamp(1, MAX_VISIBLE as isize) as i32;
        dip(FACE_DIP + ROW_DIP * count + 5, dpi)
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
        SetWindowSubclass(popup, Some(popup_proc), POPUP_SUBCLASS, NO_HOVER);
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
        let hover = fluent::hovered(hwnd);
        let index = SendMessageW(hwnd, CB_GETCURSEL, 0, 0);
        let value = option_text(hwnd, index as usize);
        fluent::with_painter(dc, bounds, dpi, |p| {
            p.clear(fluent::control_surface(GetParent(hwnd), hwnd, colors));
            let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
            let fill = if !enabled {
                colors.card
            } else if open || hover {
                colors.hover
            } else {
                colors.button
            };
            p.rounded(area, 5.0, fill, false);
            p.rounded(area, 5.0, colors.border, true);
            if focused || open {
                p.rounded(
                    rect(2.5, height - 2.0, width - 5.0, 1.5),
                    0.7,
                    colors.accent,
                    false,
                );
            }
            p.text(
                &value,
                rect(11.0, 0.0, (width - 41.0).max(0.0), height),
                if enabled { colors.text } else { colors.detail },
                Label::Body,
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

pub(super) fn draw_option(item: &DRAWITEMSTRUCT, colors: Palette) {
    let bounds = item.rcItem;
    let dpi = unsafe { GetDpiForWindow(item.hwndItem) }.max(96);
    let scale = 96.0 / dpi as f32;
    let width = (bounds.right - bounds.left) as f32 * scale;
    let height = (bounds.bottom - bounds.top) as f32 * scale;
    let index = item.itemID as usize;
    fluent::with_painter(item.hDC, bounds, dpi, |p| {
        p.clear(colors.card);
        if item.itemID == u32::MAX || item.itemState & ODS_COMBOBOXEDIT != 0 {
            return;
        }
        let label = option_text(item.hwndItem, index);
        let preview = unsafe { SendMessageW(item.hwndItem, CB_GETCURSEL, 0, 0) } as usize;
        let mut selection = NO_SAVED_SELECTION;
        let mut hover = NO_HOVER;
        unsafe {
            GetWindowSubclass(
                item.hwndItem,
                Some(face_proc),
                FACE_SUBCLASS,
                &mut selection,
            );
            let popup = popup_for(item.hwndItem);
            if !popup.is_null() {
                GetWindowSubclass(popup, Some(popup_proc), POPUP_SUBCLASS, &mut hover);
            }
        }
        let current = index == selection || selection == NO_SAVED_SELECTION && index == preview;
        if index == hover || current {
            p.rounded(
                rect(4.0, 2.0, width - 8.0, height - 4.0),
                4.0,
                if current {
                    colors.selection
                } else {
                    colors.hover
                },
                false,
            );
        }
        if index == preview && !current {
            p.rounded(
                rect(4.5, 2.5, width - 9.0, height - 5.0),
                4.0,
                colors.border,
                true,
            );
        }
        if current {
            p.rounded(
                rect(4.0, (height - 14.0) / 2.0, 2.5, 14.0),
                1.0,
                colors.accent,
                false,
            );
            svg(
                p,
                "check",
                CHECK,
                width - 25.0,
                (height - 14.0) / 2.0,
                14.0,
                dpi,
                colors.accent,
            );
        }
        p.text(
            &label,
            rect(12.0, 0.0, (width - 42.0).max(0.0), height),
            if item.itemState & ODS_DISABLED != 0 {
                colors.detail
            } else {
                colors.text
            },
            Label::Body,
        );
    });
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
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut track);
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
            WM_MOUSEMOVE => {
                let point = POINT {
                    x: i32::from(lp as i16),
                    y: i32::from((lp >> 16) as i16),
                };
                update_hover(hwnd, popup_row(hwnd, point));
                // Native click and drag still handle selection. Idle motion is visual only.
                if wp & MOUSE_LEFT_BUTTON == 0 {
                    return 0;
                }
            }
            WM_MOUSELEAVE | WM_MOUSEWHEEL | WM_VSCROLL => update_hover(hwnd, NO_HOVER),
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
                }
                return DefSubclassProc(hwnd, msg, wp, lp);
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
                    InvalidateRect(popup, null(), 0);
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
