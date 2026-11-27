use super::{
    PANEL,
    components::CONTROL_HEIGHT,
    dip, fluent,
    fluent::{ButtonState, ButtonStyle},
    instance, register, text_of, tr, wide,
};
use crate::native::{
    color_ref,
    paint::{Label, rect},
};
use std::{
    io,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{
        Controls::{DRAWITEMSTRUCT, ODS_SELECTED},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
use windows_sys::w;

const CLASS: *const u16 = w!("YiSou.Rust.Settings.Menu");
const INSET: i32 = 4;
const WIDTH: i32 = 184;
const ROW: i32 = CONTROL_HEIGHT;
const SEPARATOR: i32 = 9;

pub(super) struct Item {
    command: usize,
    text: &'static str,
    enabled: bool,
    pinned: bool,
}

impl Item {
    pub(super) fn new(command: usize, text: &'static str, enabled: bool) -> Self {
        Self {
            command,
            text,
            enabled,
            pinned: false,
        }
    }

    pub(super) fn pin(pinned: bool, enabled: bool) -> Self {
        Self {
            pinned,
            ..Self::new(121, if pinned { "取消置顶" } else { "置顶" }, enabled)
        }
    }
}

pub(super) fn register_window() -> io::Result<()> {
    register(CLASS, Some(proc))
}

pub(super) fn is_open() -> bool {
    PANEL.with_borrow(|p| p.engine_menu.is_some())
}

pub(super) fn dismiss() {
    if let Some((hwnd, _)) = PANEL.with_borrow(|p| p.engine_menu) {
        close(hwnd, false);
    }
}

pub(super) fn toggle_existing(engine: usize) -> bool {
    let Some((hwnd, current)) = PANEL.with_borrow(|p| p.engine_menu) else {
        return false;
    };
    close(hwnd, current == engine);
    current == engine
}

pub(super) fn show(owner: HWND, anchor: RECT, engine: usize, items: &[Item], separator: usize) {
    unsafe {
        let dpi = GetDpiForWindow(owner);
        let gap = dip(4, dpi);
        let width = dip(WIDTH, dpi);
        let height = dip(2 * INSET + items.len() as i32 * ROW + SEPARATOR, dpi);
        let style = WS_POPUP | WS_CLIPCHILDREN | WS_BORDER;
        let extended = WS_EX_TOOLWINDOW;
        let mut frame = RECT {
            right: width,
            bottom: height,
            ..Default::default()
        };
        if AdjustWindowRectExForDpi(&mut frame, style, 0, extended, dpi) == 0 {
            return;
        }
        let outer_width = frame.right - frame.left;
        let outer_height = frame.bottom - frame.top;
        let mut client = RECT::default();
        if GetClientRect(owner, &mut client) == 0 {
            return;
        }
        MapWindowPoints(owner, null_mut(), (&mut client as *mut RECT).cast(), 2);
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
        let bounds = RECT {
            left: client.left.max(monitor.rcWork.left) + gap,
            top: client.top.max(monitor.rcWork.top) + gap,
            right: client.right.min(monitor.rcWork.right) - gap,
            bottom: client.bottom.min(monitor.rcWork.bottom) - gap,
        };
        let last_left = bounds.right - outer_width;
        let last_top = bounds.bottom - outer_height;
        if last_left < bounds.left || last_top < bounds.top {
            return;
        }
        let left = (anchor.right - outer_width).clamp(bounds.left, last_left);
        let below = anchor.bottom + gap;
        let above = anchor.top - gap - outer_height;
        let top = if below <= last_top { below } else { above }.clamp(bounds.top, last_top);
        let hwnd = CreateWindowExW(
            extended,
            CLASS,
            wide(tr("更多操作")).as_ptr(),
            style,
            left,
            top,
            outer_width,
            outer_height,
            owner,
            null_mut(),
            instance(),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let corner = DWMWCP_ROUND as i32;
        let border = color_ref(fluent::palette().border);
        for (attribute, value) in [
            (DWMWA_WINDOW_CORNER_PREFERENCE, corner as u32),
            (DWMWA_BORDER_COLOR, border),
        ] {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                (&value as *const u32).cast(),
                size_of_val(&value) as u32,
            );
        }
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (INSET + separator as i32 * ROW) as isize,
        );
        for (index, item) in items.iter().enumerate() {
            let y = INSET + index as i32 * ROW + if index >= separator { SEPARATOR } else { 0 };
            let button = CreateWindowExW(
                0,
                w!("BUTTON"),
                wide(tr(item.text)).as_ptr(),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | BS_OWNERDRAW as u32
                    | if item.enabled { 0 } else { WS_DISABLED },
                dip(INSET, dpi),
                dip(y, dpi),
                width - 2 * dip(INSET, dpi),
                dip(y + ROW, dpi) - dip(y, dpi),
                hwnd,
                item.command as HMENU,
                instance(),
                null(),
            );
            if button.is_null() {
                DestroyWindow(hwnd);
                return;
            }
            SetWindowLongPtrW(button, GWLP_USERDATA, isize::from(item.pinned));
            SetWindowSubclass(button, Some(button_proc), 1, 0);
        }
        PANEL.with_borrow_mut(|p| p.engine_menu = Some((hwnd, engine)));
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(hwnd);
    }
}

fn close(hwnd: HWND, restore_focus: bool) {
    unsafe {
        if GetWindowLongPtrW(hwnd, GWLP_USERDATA) == 0 {
            return;
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let owner = GetWindow(hwnd, GW_OWNER);
        DestroyWindow(hwnd);
        if restore_focus && !owner.is_null() && IsWindowVisible(owner) != 0 {
            SetActiveWindow(owner);
            let list = GetDlgItem(owner, 10);
            if !list.is_null() {
                SetFocus(list);
            }
        }
    }
}

fn key(hwnd: HWND, button: HWND, key: u16) -> bool {
    unsafe {
        let (from, previous) = match key {
            VK_ESCAPE => {
                close(hwnd, true);
                return true;
            }
            VK_RETURN | VK_SPACE if !button.is_null() => {
                SendMessageW(button, BM_CLICK, 0, 0);
                return true;
            }
            VK_HOME => (null_mut(), false),
            VK_END => (null_mut(), true),
            VK_UP => (button, true),
            VK_DOWN | VK_RETURN | VK_SPACE => (button, false),
            VK_TAB => (button, GetKeyState(i32::from(VK_SHIFT)) < 0),
            _ => return false,
        };
        let first = GetNextDlgTabItem(hwnd, null_mut(), 0);
        let from = if from.is_null() && previous {
            first
        } else {
            from
        };
        let next = GetNextDlgTabItem(hwnd, from, i32::from(previous));
        if !next.is_null() {
            SetFocus(next);
        }
        true
    }
}

fn paint_surface(hwnd: HWND, dc: HDC) {
    unsafe {
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds);
        let dpi = GetDpiForWindow(hwnd);
        let colors = fluent::palette();
        let width = bounds.right as f32 * 96.0 / dpi as f32;
        let separator = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as f32;
        fluent::with_painter(dc, bounds, dpi, |painter| {
            painter.clear(colors.card);
            painter.rounded(
                rect(9.0, separator + 4.0, width - 18.0, 1.0),
                0.0,
                colors.border,
                false,
            );
        });
    }
}

fn paint_item(item: &DRAWITEMSTRUCT) {
    let colors = fluent::palette();
    let button = item.hwndItem;
    let bounds = item.rcItem;
    let dpi = unsafe { GetDpiForWindow(button) };
    let unit = 96.0 / dpi as f32;
    let width = (bounds.right - bounds.left) as f32 * unit;
    let height = (bounds.bottom - bounds.top) as f32 * unit;
    let (enabled, focused, pinned) = unsafe {
        (
            IsWindowEnabled(button) != 0,
            GetFocus() == button,
            GetWindowLongPtrW(button, GWLP_USERDATA) != 0,
        )
    };
    let color = if !enabled { colors.detail } else { colors.text };
    let icon = match item.CtlID {
        120 => (
            "star",
            include_bytes!("../../../assets/icons/tabler/star.svg").as_slice(),
        ),
        121 if pinned => (
            "pinned-off",
            include_bytes!("../../../assets/icons/tabler/pinned-off.svg").as_slice(),
        ),
        121 => (
            "pinned",
            include_bytes!("../../../assets/icons/tabler/pinned.svg").as_slice(),
        ),
        123 => (
            "pencil",
            include_bytes!("../../../assets/icons/tabler/pencil.svg").as_slice(),
        ),
        129 => (
            "arrow-up",
            include_bytes!("../../../assets/icons/tabler/arrow-up.svg").as_slice(),
        ),
        130 => (
            "arrow-down",
            include_bytes!("../../../assets/icons/tabler/arrow-down.svg").as_slice(),
        ),
        124 => (
            "trash",
            include_bytes!("../../../assets/icons/tabler/trash.svg").as_slice(),
        ),
        _ => return,
    };
    let label = text_of(button);
    fluent::with_painter(item.hDC, bounds, dpi, |painter| {
        painter.clear(colors.card);
        let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
        fluent::paint_button_face(
            painter,
            area,
            colors,
            ButtonStyle::Subtle,
            ButtonState {
                hover: fluent::hover_amount(button),
                focused: enabled && focused,
                pressed: item.itemState & ODS_SELECTED != 0,
                disabled: !enabled,
            },
        );
        fluent::icon(painter, icon, 8.0, (height - 16.0) / 2.0, 16.0, dpi, color);
        painter.text(
            &label,
            rect(32.0, 0.0, width - 38.0, height),
            color,
            Label::Body,
        );
    });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                paint_surface(hwnd, BeginPaint(hwnd, &mut paint));
                EndPaint(hwnd, &paint);
                0
            }
            WM_DRAWITEM if lp != 0 => {
                paint_item(&*(lp as *const DRAWITEMSTRUCT));
                1
            }
            WM_KEYDOWN if key(hwnd, null_mut(), wp as u16) => 0,
            WM_COMMAND if lp != 0 && (wp >> 16) as u32 == BN_CLICKED => {
                let owner = GetWindow(hwnd, GW_OWNER);
                close(hwnd, true);
                PostMessageW(owner, WM_COMMAND, wp & 0xffff, 0);
                0
            }
            WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
                close(hwnd, false);
                0
            }
            WM_CANCELMODE | WM_CLOSE => {
                close(hwnd, true);
                0
            }
            WM_NCDESTROY => {
                PANEL.with_borrow_mut(|p| {
                    if p.engine_menu.is_some_and(|(menu, _)| menu == hwnd) {
                        p.engine_menu = None;
                    }
                });
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        let menu = GetParent(hwnd);
        if msg == WM_KEYDOWN && key(menu, hwnd, wp as u16) {
            return 0;
        }
        fluent::button_hover_message(GetWindow(menu, GW_OWNER), hwnd, msg, wp, lp);
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
