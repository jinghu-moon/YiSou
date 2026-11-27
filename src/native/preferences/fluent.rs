use super::{NAV, PAGES, PANEL, components::Surface, nav_pages, tr};
pub(super) use crate::native::paint::blend;
use crate::native::{
    State, color_ref, dip,
    paint::{HOVER, Label, Painter, rect},
    smooth_corner::CornerShape,
    text_of,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::RECT as D2dRect,
    Graphics::{Direct2D::Common::D2D_RECT_F, Gdi::HDC as D2dDc},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    UI::{
        Controls::*,
        HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi},
        Input::KeyboardAndMouse::{
            EnableWindow, GetFocus, IsWindowEnabled, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
            TrackMouseEvent, VK_ESCAPE, VK_RETURN,
        },
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

use super::components::{
    BUTTON_ICON_SPACE, BUTTON_PADDING, CARD_GAP, CONTROL_HEIGHT, WIDTH as CONTENT_WIDTH,
};

const SIDEBAR: i32 = 182;
const SCROLLBAR_GUTTER: i32 = 10;
const EDIT_VERTICAL_INSET: i32 = 5;
const NAV_ROW: i32 = 42;
const CARD: u32 = 0xf9f9fc;
const TEXT: u32 = 0x1a1e26;
const DETAIL: u32 = 0x545e6f;
const WHITE: u32 = 0xffffff;
pub(super) const CONTROL_STATE_TIMER: usize = 17;
pub(super) const HOVER_TIMER: usize = 22;
const CONTROL_STATE_DURATION: Duration = Duration::from_millis(150);
const HOVER_DURATION: Duration = Duration::from_millis(120);
pub(super) const RIGHT: i32 = -1;
pub(super) const STRETCH: i32 = -2;
pub(super) const LEFT_HALF: i32 = -3;
pub(super) const RIGHT_HALF: i32 = -4;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum FontRole {
    #[default]
    Body,
    Detail,
    Header,
    Section,
    Preview,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Layout {
    x: i32,
    pub(super) y: i32,
    width: i32,
    width_limit: i32,
    height: i32,
    sizing: i32,
    font: FontRole,
    edit: bool,
    toggle: bool,
    suffix: Option<&'static str>,
    trailing_icon: Option<(&'static str, &'static [u8])>,
}

impl Layout {
    pub(super) const fn new(x: i32, y: i32, width: i32, height: i32, sizing: i32) -> Self {
        Self {
            x,
            y,
            width,
            width_limit: i32::MAX,
            height,
            sizing,
            font: FontRole::Body,
            edit: false,
            toggle: false,
            suffix: None,
            trailing_icon: None,
        }
    }

    pub(super) fn font(mut self, font: FontRole) -> Self {
        self.font = font;
        self
    }

    pub(super) fn limit_width(mut self, width: i32) -> Self {
        self.width_limit = width;
        self
    }

    pub(super) fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub(super) fn trailing_icon(mut self, icon: (&'static str, &'static [u8])) -> Self {
        self.trailing_icon = Some(icon);
        self
    }

    pub(super) fn edit(mut self) -> Self {
        self.height = CONTROL_HEIGHT;
        self.edit = true;
        self
    }

    pub(super) fn toggle(mut self) -> Self {
        self.toggle = true;
        self
    }

    pub(super) fn move_to_y(&mut self, y: i32) {
        self.y = y;
    }

    fn place(self, viewport: Viewport, scroll: i32) -> RECT {
        let delta = viewport.width - CONTENT_WIDTH;
        let half = delta.div_euclid(2);
        let (offset, extra_width) = match self.sizing {
            RIGHT => (delta, 0),
            STRETCH => (0, delta),
            LEFT_HALF => (0, half),
            RIGHT_HALF => (half, delta - half),
            _ => (0, 0),
        };
        scaled_rect(
            viewport.left + self.x + offset,
            self.y,
            (self.width + extra_width).min(self.width_limit).max(1),
            self.height.max(1),
            viewport.dpi,
            scroll,
        )
    }
}

#[derive(Default)]
pub(super) struct View {
    pub(super) controls: Vec<(HWND, Layout)>,
    pub(super) surfaces: Vec<Surface>,
    pub(super) row_icons: Vec<((&'static str, &'static [u8]), i32, i32)>,
    pub(super) extent: i32,
    pub(super) engine_icon: Option<yisou::engine::Engine>,
    control_state_motions: HashMap<HWND, Transition>,
    hover_motions: HashMap<HoverTarget, Transition>,
    pub(super) scroll: i32,
    pub(super) scrollbar: super::scrollbar::Motion,
    hover: Option<HoverTarget>,
    pub(super) nav_hover: Option<usize>,
    pub(super) wheel: i32,
    fonts: Option<Fonts>,
    painter: Option<Painter>,
    browser_icons: HashMap<(String, u32), Option<Vec<u32>>>,
}

thread_local! { static PICKER_VIEW: std::cell::RefCell<View> = std::cell::RefCell::new(View::default()); }

pub(super) fn with_view<T>(hwnd: HWND, read: impl FnOnce(&View) -> T) -> T {
    if !hwnd.is_null() && hwnd == super::color_picker::window() {
        PICKER_VIEW.with_borrow(read)
    } else {
        PANEL.with_borrow(|panel| read(&panel.view))
    }
}

pub(super) fn with_view_mut<T>(hwnd: HWND, edit: impl FnOnce(&mut View) -> T) -> T {
    if !hwnd.is_null() && hwnd == super::color_picker::window() {
        PICKER_VIEW.with_borrow_mut(edit)
    } else {
        PANEL.with_borrow_mut(|panel| edit(&mut panel.view))
    }
}

struct Transition {
    from: f32,
    to: f32,
    started: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct HoverTarget {
    control: HWND,
    row_action: Option<(usize, usize)>,
}

impl HoverTarget {
    fn control(control: HWND) -> Self {
        Self {
            control,
            row_action: None,
        }
    }

    fn row(control: HWND, row: usize, action: usize) -> Self {
        Self {
            control,
            row_action: Some((row, action)),
        }
    }

    fn invalidate(self) {
        unsafe {
            if let Some((row, _)) = self.row_action {
                let mut bounds = RECT::default();
                if SendMessageW(
                    self.control,
                    LB_GETITEMRECT,
                    row,
                    (&mut bounds as *mut RECT) as isize,
                ) >= 0
                {
                    InvalidateRect(self.control, &bounds, 0);
                }
            } else {
                InvalidateRect(self.control, null(), 0);
            }
        }
    }
}

impl View {
    pub(super) fn settings_painter(&mut self) -> Option<&mut Painter> {
        if self.painter.is_none() {
            self.painter = Painter::new("Segoe UI", None).ok();
        }
        self.painter.as_mut()
    }

    fn change_hover(
        &mut self,
        next: Option<HoverTarget>,
        now: Instant,
        animate: bool,
    ) -> Option<HoverTarget> {
        let previous = std::mem::replace(&mut self.hover, next);
        for (control, target) in [(previous, 0.0), (next, 1.0)] {
            let Some(control) = control else { continue };
            let old = self.hover_motions.remove(&control);
            if animate {
                let mut motion = Transition::reverse(old, target, now, HOVER_DURATION);
                if motion.from != motion.to {
                    motion.started = Some(now);
                    self.hover_motions.insert(control, motion);
                }
            }
        }
        previous
    }

    fn hover_amount(&self, control: HoverTarget, now: Instant) -> f32 {
        self.hover_motions.get(&control).map_or_else(
            || f32::from(u8::from(self.hover == Some(control))),
            |motion| motion.amount(now, HOVER_DURATION),
        )
    }
}

impl Transition {
    fn reverse(previous: Option<Self>, to: f32, now: Instant, duration: Duration) -> Self {
        let from = previous.map_or(1.0 - to, |motion| motion.amount(now, duration));
        Self {
            from,
            to,
            started: None,
        }
    }

    fn elapsed(&self, now: Instant) -> Duration {
        self.started
            .map_or(Duration::ZERO, |start| now.saturating_duration_since(start))
    }

    fn amount(&self, now: Instant, duration: Duration) -> f32 {
        let progress = self.elapsed(now).as_secs_f32() / duration.as_secs_f32();
        self.from + (self.to - self.from) * easing(progress, 0.18, 0.30, 1.0)
    }
}

pub(super) fn animate_control_state(control: HWND, active: bool) {
    let to = f32::from(u8::from(active));
    let animate = animations_enabled();
    PANEL.with_borrow_mut(|p| {
        let motions = &mut p.view.control_state_motions;
        let motion = Transition::reverse(
            motions.remove(&control),
            to,
            Instant::now(),
            CONTROL_STATE_DURATION,
        );
        if animate && motion.from != motion.to {
            motions.insert(control, motion);
        }
    });
}

pub(super) fn animate_selector(previous: HWND, next: HWND) {
    PANEL.with_borrow_mut(|p| p.view.control_state_motions.remove(&previous));
    animate_control_state(next, true);
}

fn control_state_amount(parent: HWND, control: HWND, active: bool) -> f32 {
    let now = Instant::now();
    let motion = PANEL.with_borrow_mut(|p| {
        let motion = p.view.control_state_motions.get_mut(&control)?;
        let first_frame = motion.started.is_none();
        motion.started.get_or_insert(now);
        Some((motion.amount(now, CONTROL_STATE_DURATION), first_frame))
    });
    if let Some((amount, first_frame)) = motion {
        if first_frame {
            unsafe { SetTimer(parent, CONTROL_STATE_TIMER, 15, None) };
        }
        amount
    } else {
        f32::from(u8::from(active))
    }
}

pub(super) fn clear_control_motions(hwnd: HWND) {
    PANEL.with_borrow_mut(|p| {
        p.view.control_state_motions.clear();
        p.view.hover_motions.clear();
        p.view.hover = None;
    });
    unsafe {
        KillTimer(hwnd, CONTROL_STATE_TIMER);
        KillTimer(hwnd, HOVER_TIMER);
    }
}

fn set_control_hover(parent: HWND, next: Option<HoverTarget>) {
    if with_view(parent, |view| view.hover == next) {
        return;
    }
    let animate = animations_enabled();
    let now = Instant::now();
    let (previous, start_timer) = with_view_mut(parent, |view| {
        let was_idle = view.hover_motions.is_empty();
        let previous = view.change_hover(next, now, animate);
        (previous, was_idle && !view.hover_motions.is_empty())
    });
    for target in [previous, next].into_iter().flatten() {
        target.invalidate();
    }
    if start_timer {
        unsafe { SetTimer(parent, HOVER_TIMER, 15, None) };
    }
}

pub(super) fn clear_control_hover(control: HWND) {
    let parent = unsafe { GetParent(control) };
    with_view_mut(parent, |view| {
        view.hover_motions
            .retain(|target, _| target.control != control);
        if view.hover.is_some_and(|target| target.control == control) {
            view.hover = None;
        }
    });
}

pub(super) fn tick_hover(parent: HWND) {
    let now = Instant::now();
    let (targets, active) = with_view_mut(parent, |view| {
        let motions = &mut view.hover_motions;
        let targets: Vec<_> = motions.keys().copied().collect();
        motions.retain(|_, motion| motion.elapsed(now) < HOVER_DURATION);
        (targets, !motions.is_empty())
    });
    for target in targets {
        target.invalidate();
    }
    if !active {
        unsafe { KillTimer(parent, HOVER_TIMER) };
    }
}

fn leave_hover(parent: HWND, control: HWND) {
    if with_view(parent, |view| {
        view.hover.is_some_and(|target| target.control == control)
    }) {
        set_control_hover(parent, None);
    }
}

fn update_hover(parent: HWND, control: HWND, target: Option<HoverTarget>) {
    if let Some(target) = target {
        let entered = with_view(parent, |view| {
            view.hover.is_none_or(|hover| hover.control != control)
        });
        set_control_hover(parent, Some(target));
        if entered {
            track_mouse_leave(control);
        }
    } else {
        leave_hover(parent, control);
    }
}

pub(super) fn button_hover_message(parent: HWND, button: HWND, msg: u32, wp: WPARAM, lp: LPARAM) {
    unsafe {
        match msg {
            WM_MOUSEMOVE if IsWindowEnabled(button) != 0 => {
                let mut bounds = RECT::default();
                GetClientRect(button, &mut bounds);
                let point = POINT {
                    x: i32::from(lp as i16),
                    y: i32::from((lp >> 16) as i16),
                };
                let target = (PtInRect(&bounds, point) != 0).then(|| HoverTarget::control(button));
                update_hover(parent, button, target);
            }
            WM_MOUSELEAVE => leave_hover(parent, button),
            WM_ENABLE | WM_SHOWWINDOW if wp == 0 => {
                clear_control_hover(button);
                InvalidateRect(button, null(), 0);
            }
            WM_NCDESTROY => clear_control_hover(button),
            _ => {}
        }
    }
}

pub(super) fn animations_enabled() -> bool {
    let mut animate = 0i32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut animate as *mut i32).cast(),
            0,
        ) != 0
            && animate != 0
    }
}

pub(super) fn easing(progress: f32, x1: f32, x2: f32, y2: f32) -> f32 {
    if progress <= 0.0 || progress >= 1.0 {
        return progress.clamp(0.0, 1.0);
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..10 {
        let t = (low + high) * 0.5;
        let u = 1.0 - t;
        let x = 3.0 * u * t * (u * x1 + t * x2) + t * t * t;
        if x < progress {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) * 0.5;
    t * t * (3.0 * (1.0 - t) * y2 + t)
}

pub(super) fn tick_control_state(hwnd: HWND) {
    let now = Instant::now();
    let active = PANEL.with_borrow_mut(|p| {
        let motions = &mut p.view.control_state_motions;
        motions.retain(|&control, motion| {
            if motion.started.is_none() {
                return true;
            }
            unsafe { InvalidateRect(control, null(), 0) };
            motion.elapsed(now) < CONTROL_STATE_DURATION
        });
        motions.values().any(|motion| motion.started.is_some())
    });
    if !active {
        unsafe { KillTimer(hwnd, CONTROL_STATE_TIMER) };
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn motion_waits_for_the_first_frame_and_reverses_without_a_jump() {
        let click = Instant::now();
        let mut opening = Transition::reverse(None, 1.0, click, CONTROL_STATE_DURATION);
        let first_frame = click + CONTROL_STATE_DURATION;
        assert_eq!(opening.amount(first_frame, CONTROL_STATE_DURATION), 0.0);
        opening.started = Some(first_frame);
        assert_eq!(opening.amount(first_frame, CONTROL_STATE_DURATION), 0.0);
        assert_eq!(
            opening.amount(first_frame + CONTROL_STATE_DURATION, CONTROL_STATE_DURATION),
            1.0
        );

        let reversal = first_frame + CONTROL_STATE_DURATION / 2;
        let amount = opening.amount(reversal, CONTROL_STATE_DURATION);
        assert!(amount > 0.0 && amount < 1.0);
        let mut closing = Transition::reverse(Some(opening), 0.0, reversal, CONTROL_STATE_DURATION);
        closing.started = Some(reversal);
        assert_eq!(closing.amount(reversal, CONTROL_STATE_DURATION), amount);
        assert_eq!(
            closing.amount(reversal + CONTROL_STATE_DURATION, CONTROL_STATE_DURATION),
            0.0
        );
    }

    #[test]
    fn hovering_between_buttons_fades_both_and_reverses_from_the_current_color() {
        let mut view = View::default();
        let list = std::ptr::without_provenance_mut(1);
        let first = HoverTarget::control(std::ptr::without_provenance_mut(2));
        let second = HoverTarget::row(list, 0, 1);
        let begin = Instant::now();
        view.change_hover(Some(first), begin, true);
        let switch = begin + HOVER_DURATION / 2;
        let first_hover = view.hover_amount(first, switch);
        assert!(first_hover > 0.0 && first_hover < 1.0);

        view.change_hover(Some(second), switch, true);
        assert_eq!(view.hover_amount(first, switch), first_hover);
        assert_eq!(view.hover_amount(second, switch), 0.0);
        let back = switch + HOVER_DURATION / 2;
        let first_fading = view.hover_amount(first, back);
        let second_hover = view.hover_amount(second, back);
        assert!(first_fading > 0.0 && first_fading < first_hover);
        assert!(second_hover > 0.0 && second_hover < 1.0);

        view.change_hover(Some(first), back, true);
        assert_eq!(view.hover_amount(first, back), first_fading);
        assert_eq!(view.hover_amount(second, back), second_hover);
        assert_eq!(view.hover_amount(first, back + HOVER_DURATION), 1.0);
        assert_eq!(view.hover_amount(second, back + HOVER_DURATION), 0.0);

        view.change_hover(None, back + HOVER_DURATION, false);
        assert_eq!(view.hover_amount(first, back + HOVER_DURATION), 0.0);
    }
}

fn browser_pixels(path: &str, edge: u32) -> Option<Vec<u32>> {
    let icon = win_icon_extractor::extract_icon_with_size(path, edge).ok()?;
    let mut rgba = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba)?;
    if !rgba.pixels().any(|pixel| pixel[3] != 0) {
        return None;
    }
    for pixel in rgba.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    if rgba.dimensions() != (edge, edge) {
        rgba = image::imageops::resize(&rgba, edge, edge, image::imageops::FilterType::Triangle);
    }
    Some(
        rgba.pixels()
            .map(|&image::Rgba([r, g, b, a])| u32::from_le_bytes([b, g, r, a]))
            .collect(),
    )
}

fn paint_browser_icon(p: &mut Painter, path: &str, row_height: i32, dpi: u32) -> bool {
    let edge = dip(18, dpi) as u32;
    let unit = 96.0 / dpi as f32;
    let size = edge as f32 * unit;
    let area = rect(
        dip(36, dpi) as f32 * unit,
        ((row_height - edge as i32) / 2) as f32 * unit,
        size,
        size,
    );
    PANEL.with_borrow_mut(|panel| {
        let pixels = panel
            .view
            .browser_icons
            .entry((path.to_owned(), edge))
            .or_insert_with(|| browser_pixels(path, edge));
        let Some(pixels) = pixels.as_deref() else {
            return false;
        };
        p.image(&format!("settings-browser-exe-{path}"), area, edge, || {
            Some(Cow::Borrowed(pixels))
        });
        true
    })
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
        let logical = (bounds.right * 96 + dpi as i32 / 2) / dpi as i32;
        if hwnd == super::color_picker::window() {
            return Self {
                sidebar: 0,
                left: super::color_picker::INSET,
                width: (logical - 2 * super::color_picker::INSET).max(1),
                height: bounds.bottom * 96 / dpi as i32,
                dpi,
            };
        }
        let (sidebar, gutter) = if logical < 730 {
            (147, 17)
        } else {
            (SIDEBAR, 32)
        };
        let available = (logical - sidebar - SCROLLBAR_GUTTER).max(0);
        let view_width = available.min(794);
        Self {
            sidebar,
            left: sidebar + gutter + (available - view_width) / 2,
            width: (view_width - 2 * gutter).max(1),
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
    pub(super) background: u32,
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
        rail: 0xedf0f6,
        background: 0xedf0f6,
        card: CARD,
        text: TEXT,
        detail: DETAIL,
        accent: 0x1f75db,
        selection: 0xe4eefb,
        hover: HOVER,
        border: 0xdce0e7,
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

fn scroll_range(view: &View, viewport: Viewport) -> i32 {
    dip(view.extent - viewport.height, viewport.dpi).max(0)
}

pub(super) fn scroll_metrics(hwnd: HWND) -> (i32, i32, i32) {
    let viewport = Viewport::of(hwnd);
    with_view(hwnd, |view| {
        (
            view.scroll,
            scroll_range(view, viewport),
            dip(viewport.height, viewport.dpi),
        )
    })
}

pub(super) fn scroll_to(hwnd: HWND, position: i32) {
    let viewport = Viewport::of(hwnd);
    let changed = with_view_mut(hwnd, |view| {
        let position = position.clamp(0, scroll_range(view, viewport));
        if position == view.scroll {
            return false;
        }
        view.scroll = position;
        true
    });
    if changed {
        super::menu::dismiss();
        position_controls(hwnd, false);
        refresh_list_hover(hwnd);
        super::scrollbar::wake_page(hwnd);
    }
}

pub(super) fn layout(hwnd: HWND) {
    position_controls(hwnd, true);
    refresh_list_hover(hwnd);
}

pub(super) fn update_search_font_size(state: &State, hwnd: HWND, previous_height: i32) {
    let font = state.create_ui_font(unsafe { GetDpiForWindow(hwnd) }.max(96));
    let old_font = PANEL.with_borrow_mut(|p| std::mem::replace(&mut p.preview_font, font));
    update_search_preview(state, hwnd, previous_height);
    unsafe {
        if !old_font.is_null() {
            DeleteObject(old_font);
        }
        let custom =
            super::appearance::custom_search_font(&state.app.borrow().settings().preferences);
        EnableWindow(GetDlgItem(hwnd, 112), i32::from(custom));
    }
}

pub(super) fn update_search_preview(state: &State, hwnd: HWND, previous_height: i32) {
    use super::appearance::{SEARCH_PREVIEW_TOP, preview_caption, search_preview_height};

    let (next_height, caption) = {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        (search_preview_height(prefs), preview_caption(state, prefs))
    };
    let delta = next_height - previous_height;
    let below_preview = SEARCH_PREVIEW_TOP + previous_height;
    let preview = unsafe { GetDlgItem(hwnd, 115) };
    PANEL.with_borrow_mut(|p| {
        for (child, layout) in &mut p.view.controls {
            if *child == preview {
                *layout = caption;
            } else if layout.y >= below_preview {
                layout.y += delta;
            }
        }
        for surface in &mut p.view.surfaces {
            if surface.top >= below_preview {
                surface.top += delta;
                surface.bottom += delta;
            }
        }
        for (_, _, y) in &mut p.view.row_icons {
            if *y >= below_preview {
                *y += delta;
            }
        }
        p.view.extent += delta;
    });
    layout(hwnd);
}

fn position_controls(hwnd: HWND, full_layout: bool) {
    let initial = Viewport::of(hwnd);
    let (page, preview) = if hwnd == super::color_picker::window() {
        (None, null_mut())
    } else {
        PANEL.with_borrow(|p| (Some(p.page), p.preview_font))
    };
    if full_layout && page == Some(5) {
        super::history::layout_rows(initial.width, initial.dpi);
    }
    let scroll = with_view_mut(hwnd, |view| {
        let maximum = scroll_range(view, initial);
        view.scroll = view.scroll.clamp(0, maximum);
        view.scroll
    });
    let viewport = initial;
    let (controls, old_fonts, replacements) = with_view_mut(hwnd, |view| {
        let old = if view.fonts.as_ref().is_none_or(|f| f.dpi != viewport.dpi) {
            view.fonts.replace(Fonts::new(viewport.dpi))
        } else {
            None
        };
        (
            view.controls.clone(),
            old,
            view.fonts.as_ref().unwrap().handles,
        )
    });
    unsafe {
        let nav = GetDlgItem(hwnd, NAV as i32);
        if full_layout && SendMessageW(nav, WM_GETFONT, 0, 0) != replacements[0] as isize {
            SendMessageW(nav, WM_SETFONT, replacements[0] as usize, 0);
        }
        let compact_nav = viewport.height < 375;
        let nav_row = dip(if compact_nav { 34 } else { NAV_ROW }, viewport.dpi);
        if full_layout && SendMessageW(nav, LB_GETITEMHEIGHT, 0, 0) != nav_row as isize {
            SendMessageW(nav, LB_SETITEMHEIGHT, 0, nav_row as isize);
        }
        let mut positions = BeginDeferWindowPos((controls.len() + 1) as i32);
        let mut place = |control, bounds: RECT| {
            let flags = SWP_NOZORDER
                | SWP_NOACTIVATE
                | SWP_NOREDRAW
                | if full_layout || page == Some(5) && GetDlgCtrlID(control) == 10 {
                    0
                } else {
                    SWP_NOSIZE
                };
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
            let top = if compact_nav { 7 } else { 17 };
            let inset = if compact_nav { 8 } else { 10 };
            let mut bounds = scaled_rect(
                inset,
                top,
                viewport.sidebar - 2 * inset,
                viewport.height - top,
                viewport.dpi,
                0,
            );
            bounds.bottom = bounds
                .bottom
                .min(bounds.top + nav_pages().count() as i32 * nav_row as i32);
            place(nav, bounds);
        }
        for (child, layout) in controls {
            let is_list = GetDlgCtrlID(child) == 10;
            let list_row = (is_list && page != Some(5)).then(|| match page {
                Some(2) => super::engines::ROW_HEIGHT,
                _ => super::pages::BROWSER_ROW,
            });
            let mut bounds = layout.place(viewport, scroll);
            if is_list && page == Some(5) {
                let bottom = dip(viewport.height, viewport.dpi);
                bounds.top = bounds.top.clamp(0, bottom);
                bounds.bottom = bounds.bottom.clamp(0, bottom);
            }
            if layout.edit {
                // The outer frame is painted by the parent. The native edit only holds the text.
                let inset = dip(EDIT_VERTICAL_INSET, viewport.dpi);
                bounds.top += inset;
                bounds.bottom -= inset;
            }
            if let Some(row) = list_row {
                let row_px = dip(row, viewport.dpi).min(255);
                // Win32 rounds each list row separately.
                bounds.bottom = bounds.top + (layout.height / row).max(1) * row_px;
            }
            if !full_layout {
                place(child, bounds);
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
            if let Some(height) = list_row {
                let height = dip(height, viewport.dpi).min(255) as isize;
                if SendMessageW(child, LB_GETITEMHEIGHT, 0, 0) != height {
                    SendMessageW(child, LB_SETITEMHEIGHT, 0, height);
                }
            }
            if super::dropdown::is_combo(child) {
                bounds.bottom = bounds.top + super::dropdown::sync_metrics(child, viewport.dpi);
            }
            place(child, bounds);
        }
        if !positions.is_null() {
            EndDeferWindowPos(positions);
        }
        super::scrollbar::layout(hwnd);
        let content = RECT {
            left: dip(viewport.sidebar, viewport.dpi),
            top: 0,
            right: dip(
                viewport.left + viewport.width + SCROLLBAR_GUTTER,
                viewport.dpi,
            ),
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
    let next = with_view_mut(hwnd, |view| {
        let maximum = scroll_range(view, viewport);
        let line = dip(22, viewport.dpi);
        let page = dip(viewport.height, viewport.dpi);
        if message == WM_MOUSEWHEEL {
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
                _ => view.scroll,
            }
        }
    });
    scroll_to(hwnd, next);
}

pub(super) fn ensure_visible(hwnd: HWND, child: HWND) {
    let viewport = Viewport::of(hwnd);
    let row = if unsafe { GetDlgCtrlID(child) } == 10
        && PANEL.with_borrow(|p| matches!(p.page, 2 | 4 | 5) && !p.edit_engine)
    {
        unsafe {
            let selected = SendMessageW(child, LB_GETCURSEL, 0, 0);
            let selected = if selected < 0 {
                SendMessageW(child, LB_GETCARETINDEX, 0, 0)
            } else {
                selected
            };
            let mut row = RECT::default();
            (SendMessageW(
                child,
                LB_GETITEMRECT,
                selected.max(0) as usize,
                (&mut row as *mut RECT) as isize,
            ) >= 0)
                .then_some(row)
        }
    } else {
        None
    };
    let history = row.is_some() && PANEL.with_borrow(|p| p.page == 5);
    let next = with_view(hwnd, |view| {
        let Some((_, area)) = view.controls.iter().find(|(hwnd, _)| *hwnd == child) else {
            return view.scroll;
        };
        let scroll = view.scroll;
        let mut bounds = area.place(viewport, scroll);
        if let Some(row) = row {
            if history {
                bounds.top = bounds.top.max(0);
            }
            bounds.bottom = bounds.top + row.bottom;
            bounds.top += row.top;
        }
        let bottom = dip(viewport.height, viewport.dpi);
        let tall_history_row = history && bounds.bottom - bounds.top > bottom;
        let offset = if bounds.top < 0 || tall_history_row {
            bounds.top
        } else {
            (bounds.bottom - bottom).max(0)
        };
        scroll + offset
    });
    scroll_to(hwnd, next);
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
        if matches!(kind, 1 | 4) {
            button_hover_message(parent, hwnd, msg, wp, lp);
        }
        let page_list = GetDlgCtrlID(hwnd) == 10 && kind == 0;
        let (engine_list, history_list) = if page_list {
            PANEL.with_borrow(|p| (p.page == 2 && !p.edit_engine, p.page == 5))
        } else {
            (false, false)
        };
        let recorder = kind == 1 && super::recorder::is_trigger(GetDlgCtrlID(hwnd) as usize);
        let dragging = engine_list && PANEL.with_borrow(|p| p.engine_drag.is_some());
        let tab = super::appearance::Tab::from_id(GetDlgCtrlID(hwnd) as usize);
        match msg {
            WM_GETDLGCODE if tab.is_some() => {
                return DefSubclassProc(hwnd, msg, wp, lp) | DLGC_WANTARROWS as isize;
            }
            WM_KEYDOWN if let Some(next) = tab.and_then(|tab| tab.navigate(wp as u16)) => {
                PostMessageW(parent, WM_COMMAND, next.id(), 0);
                return 0;
            }
            WM_MOUSEMOVE if dragging => {
                super::engines::drag_to(hwnd, i32::from((lp >> 16) as i16));
                return 0;
            }
            WM_ERASEBKGND if dragging => return 1,
            WM_PAINT | WM_PRINTCLIENT if dragging => {
                if let Some(state) = crate::native::session() {
                    let mut ps = PAINTSTRUCT::default();
                    let mut update = RECT::default();
                    let dc = if msg == WM_PAINT {
                        let dc = BeginPaint(hwnd, &mut ps);
                        update = ps.rcPaint;
                        dc
                    } else {
                        GetClientRect(hwnd, &mut update);
                        wp as HDC
                    };
                    paint_drag(&state, parent, hwnd, dc, update);
                    if msg == WM_PAINT {
                        EndPaint(hwnd, &ps);
                    }
                }
                return 0;
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE if dragging => {
                if let Some(state) = crate::native::session() {
                    if msg == WM_LBUTTONUP {
                        super::engines::drag_to(hwnd, i32::from((lp >> 16) as i16));
                    }
                    super::engines::finish_drag(&state, hwnd, msg == WM_LBUTTONUP);
                }
                return 0;
            }
            WM_KEYDOWN if dragging && wp as u16 == VK_ESCAPE => {
                if let Some(state) = crate::native::session() {
                    super::engines::finish_drag(&state, hwnd, false);
                }
                return 0;
            }
            WM_KEYDOWN if engine_list && wp as u16 == 0x20 => {
                if lp & (1 << 30) == 0 {
                    PostMessageW(parent, super::ENGINE_TOGGLE, 0, 0);
                }
                return 0;
            }
            WM_SYSKEYDOWN if engine_list && matches!(wp as u16, 0x26 | 0x28) => {
                PostMessageW(parent, WM_COMMAND, if wp == 0x26 { 129 } else { 130 }, 0);
                return 0;
            }
            WM_ERASEBKGND if kind == 3 || recorder => return 1,
            WM_PAINT | WM_PRINTCLIENT if kind == 3 || recorder => {
                let mut bounds = RECT::default();
                GetClientRect(hwnd, &mut bounds);
                let mut ps = PAINTSTRUCT::default();
                let dc = if msg == WM_PAINT {
                    BeginPaint(hwnd, &mut ps)
                } else {
                    wp as HDC
                };
                if recorder {
                    super::recorder::paint_window(hwnd, dc, bounds);
                } else {
                    let update = if msg == WM_PAINT { ps.rcPaint } else { bounds };
                    paint_navigation(hwnd, dc, bounds, update);
                }
                if msg == WM_PAINT {
                    EndPaint(hwnd, &ps);
                }
                return 0;
            }
            WM_MOUSEACTIVATE
                if engine_list
                    && super::menu::is_open()
                    && matches!((lp >> 16) as u32, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK) =>
            {
                let mut point = POINT::default();
                if GetCursorPos(&mut point) != 0
                    && ScreenToClient(hwnd, &mut point) != 0
                    && super::engines::more_row(hwnd, point.x, point.y).is_some()
                {
                    // Keep the popup active until the button handles this click.
                    return MA_NOACTIVATE as isize;
                }
            }
            WM_MOUSEMOVE if kind == 3 => {
                let hit = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, lp);
                let row = (hit >> 16 == 0).then_some(hit as usize & 0xffff);
                if set_nav_hover(hwnd, row) && wp & 1 == 0 {
                    track_mouse_leave(hwnd);
                }
                if wp & 1 == 0 {
                    return 0;
                }
            }
            WM_MOUSEMOVE if page_list => {
                update_hover(
                    parent,
                    hwnd,
                    list_hover_target(hwnd, lp, engine_list, history_list),
                );
            }
            WM_MOUSELEAVE if kind == 3 => {
                set_nav_hover(hwnd, None);
                return 0;
            }
            WM_MOUSELEAVE if page_list => leave_hover(parent, hwnd),
            LB_RESETCONTENT | WM_NCDESTROY if page_list => clear_control_hover(hwnd),
            WM_SHOWWINDOW if page_list && wp == 0 => clear_control_hover(hwnd),
            WM_SETFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                if !history_list {
                    ensure_visible(parent, hwnd);
                }
                if kind == 2 {
                    invalidate_edit_frame(parent, hwnd);
                } else if recorder || kind == 3 {
                    InvalidateRect(hwnd, null(), 0);
                }
                return result;
            }
            WM_KILLFOCUS if kind == 2 => {
                invalidate_edit_frame(parent, hwnd);
            }
            WM_KILLFOCUS if kind == 1 => {
                if recorder {
                    super::recorder::blur(parent, GetDlgCtrlID(hwnd) as usize);
                }
                InvalidateRect(hwnd, null(), 0);
            }
            WM_KILLFOCUS | WM_UPDATEUISTATE if kind == 3 || recorder => {
                InvalidateRect(hwnd, null(), 0);
            }
            WM_KEYDOWN
                if kind == 2
                    && super::color_picker::is_input(GetDlgCtrlID(hwnd) as usize)
                    && wp as u16 == VK_RETURN =>
            {
                let _ = super::color_picker::read_input(parent, true);
                return 0;
            }
            WM_KEYDOWN
                if kind == 2
                    && super::search_number_input(GetDlgCtrlID(hwnd) as usize)
                    && matches!(wp as u16, VK_RETURN | VK_ESCAPE) =>
            {
                if let Some(state) = crate::native::session() {
                    super::finish_search_number(&state, parent, hwnd, wp as u16 == VK_RETURN);
                }
                return 0;
            }
            WM_KEYDOWN if wp as u16 == VK_ESCAPE && kind != 4 => {
                PostMessageW(parent, super::BACK, 0, 0);
                return 0;
            }
            WM_MOUSEWHEEL if !matches!(kind, 3 | 4) => {
                return SendMessageW(parent, msg, wp, lp);
            }
            WM_CONTEXTMENU if engine_list => {
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
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK if engine_list => {
                use super::engines::{RowAction, row_action};
                let (x, y) = (i32::from(lp as i16), i32::from((lp >> 16) as i16));
                if let Some(row) = row_action(hwnd, x, y, RowAction::More) {
                    SendMessageW(hwnd, LB_SETCURSEL, row, 0);
                    PostMessageW(parent, super::ENGINE_MENU, 0, 0);
                    return 0;
                }
                if let Some(row) = row_action(hwnd, x, y, RowAction::Toggle) {
                    if msg == WM_LBUTTONDOWN {
                        SetFocus(hwnd);
                        SendMessageW(hwnd, LB_SETCURSEL, row, 0);
                        PostMessageW(parent, super::ENGINE_TOGGLE, 0, 0);
                    }
                    return 0;
                }
                if let Some(row) = row_action(hwnd, x, y, RowAction::Grip) {
                    leave_hover(parent, hwnd);
                    super::engines::begin_drag(hwnd, row, y);
                    return 0;
                }
            }
            _ => {}
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if GetDlgCtrlID(hwnd) == 10 && msg == WM_KEYDOWN {
            ensure_visible(parent, hwnd);
        }
        result
    }
}

fn list_hover_target(
    list: HWND,
    point: LPARAM,
    engine_list: bool,
    history_list: bool,
) -> Option<HoverTarget> {
    if history_list {
        let (row, action) = super::history::action_at(list, point)?;
        return Some(HoverTarget::row(list, row, action));
    }
    let x = i32::from(point as i16);
    let y = i32::from((point >> 16) as i16);
    if engine_list {
        use super::engines::{RowAction, row_action};
        return row_action(list, x, y, RowAction::More)
            .map(|row| HoverTarget::row(list, row, 0))
            .or_else(|| {
                row_action(list, x, y, RowAction::Toggle).map(|row| HoverTarget::row(list, row, 1))
            });
    }
    unsafe {
        let hit = SendMessageW(list, LB_ITEMFROMPOINT, 0, point);
        let row = hit as usize & 0xffff;
        let mut bounds = RECT::default();
        (hit >> 16 == 0
            && SendMessageW(
                list,
                LB_GETITEMRECT,
                row,
                (&mut bounds as *mut RECT) as isize,
            ) >= 0
            && (bounds.top..bounds.bottom - dip(CARD_GAP, GetDpiForWindow(list))).contains(&y))
        .then(|| HoverTarget::row(list, row, 0))
    }
}

fn refresh_list_hover(parent: HWND) {
    let active = PANEL.with_borrow(|p| p.view.hover.filter(|target| target.row_action.is_some()));
    let Some(active) = active else { return };
    unsafe {
        let mut cursor = POINT::default();
        let target = if GetCursorPos(&mut cursor) != 0
            && WindowFromPoint(cursor) == active.control
            && ScreenToClient(active.control, &mut cursor) != 0
        {
            let point = (u32::from(cursor.y as u16) << 16 | u32::from(cursor.x as u16)) as isize;
            let (engine_list, history_list) =
                PANEL.with_borrow(|p| (p.page == 2 && !p.edit_engine, p.page == 5));
            list_hover_target(active.control, point, engine_list, history_list)
        } else {
            None
        };
        set_control_hover(parent, target);
    }
}

fn track_mouse_leave(hwnd: HWND) {
    let mut track = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe { TrackMouseEvent(&mut track) };
}

fn paint_drag(state: &State, parent: HWND, list: HWND, dc: HDC, update: RECT) {
    let Some((drag, count)) = PANEL.with_borrow(|p| Some((p.engine_drag?, p.engine_rows.len())))
    else {
        return;
    };
    unsafe {
        let row = SendMessageW(list, LB_GETITEMHEIGHT, 0, 0).max(1) as i32;
        let mut client = RECT::default();
        GetClientRect(list, &mut client);
        let mut viewport = RECT::default();
        GetClientRect(parent, &mut viewport);
        MapWindowPoints(parent, list, (&mut viewport as *mut RECT).cast(), 2);
        let mut bounds = RECT::default();
        if IntersectRect(&mut bounds, &viewport, &update) == 0 {
            return;
        }
        let dpi = GetDpiForWindow(list).max(96);
        let unit = 96.0 / dpi as f32;
        let height = row as f32 * unit - CARD_GAP as f32;
        let first = bounds.top.max(0) / row;
        let end = ((bounds.bottom + row - 1) / row).min(count as i32);
        let colors = palette();
        // Draw the rows and the moving card into one Direct2D frame.
        with_painter(dc, bounds, dpi, |p| {
            p.clear(colors.background);
            let mut draw = |index, top, floating, placeholder| {
                let area = rect(
                    -bounds.left as f32 * unit,
                    (top - bounds.top) as f32 * unit,
                    client.right as f32 * unit,
                    height,
                );
                paint_engine_row(
                    p,
                    state,
                    colors,
                    dpi,
                    area,
                    index,
                    floating,
                    floating,
                    placeholder,
                    [0.0; 2],
                );
            };
            for index in first..end {
                draw(
                    drag.row(index as usize),
                    index * row,
                    false,
                    drag.target == index as usize,
                );
            }
            if drag.top < bounds.bottom && drag.top + row > bounds.top {
                draw(drag.source, drag.top, true, false);
            }
        });
    }
}

fn set_nav_hover(hwnd: HWND, row: Option<usize>) -> bool {
    let previous = PANEL.with_borrow_mut(|p| std::mem::replace(&mut p.view.nav_hover, row));
    if previous == row {
        return false;
    }
    for index in [previous, row].into_iter().flatten() {
        let mut bounds = RECT::default();
        unsafe {
            if SendMessageW(
                hwnd,
                LB_GETITEMRECT,
                index,
                (&mut bounds as *mut RECT) as isize,
            ) >= 0
            {
                InvalidateRect(hwnd, &bounds, 0);
            }
        }
    }
    previous.is_none() && row.is_some()
}

const NAV_ICONS: [(&str, &[u8]); 8] = [
    (
        "settings",
        include_bytes!("../../../assets/icons/tabler/settings.svg"),
    ),
    (
        "palette",
        include_bytes!("../../../assets/icons/tabler/palette.svg"),
    ),
    (
        "search",
        include_bytes!("../../../assets/icons/tabler/search.svg"),
    ),
    (
        "keyboard",
        include_bytes!("../../../assets/icons/tabler/keyboard.svg"),
    ),
    (
        "browser",
        include_bytes!("../../../assets/icons/tabler/browser.svg"),
    ),
    (
        "history",
        include_bytes!("../../../assets/icons/tabler/history.svg"),
    ),
    (
        "file-typography",
        include_bytes!("../../../assets/icons/tabler/file-typography.svg"),
    ),
    (
        "scan",
        include_bytes!("../../../assets/icons/tabler/scan.svg"),
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
    painter.svg_icon((id, svg), (x, y), size, dpi, color);
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

pub(super) fn text_width(text: &str, label: Label) -> i32 {
    PANEL.with_borrow_mut(|p| {
        p.view
            .settings_painter()
            .and_then(|painter| painter.text_width(text, label))
            .map_or_else(
                || text.chars().count() as i32 * 14,
                |width| width.ceil() as i32,
            )
    })
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
            use super::appearance::{
                MENU_PREVIEW_WIDTH, SEARCH_PREVIEW_TOP, menu_options_top, search_preview_height,
            };
            let search_width = prefs.search_width.min(view.width);
            let width = search_width as f32;
            let search_top = SEARCH_PREVIEW_TOP as f32 - offset;
            let preview_height = search_preview_height(prefs);
            let search_height = preview_height as f32;
            let pixel = 96.0 / view.dpi as f32;
            let inset = pixel / 2.0;
            let area = rect(
                x + inset,
                search_top + inset,
                width - pixel,
                search_height - pixel,
            );
            let search_colors = state.search_preview_colors();
            let corner = CornerShape::new(prefs.search_corner_radius as f32, width, search_height);
            p.smooth_rectangle(
                area,
                corner.inset(inset),
                Some(search_colors.surface),
                colors.border,
                pixel,
            );
            if let Some(engine) = default {
                let padding = prefs.search_padding;
                let button = crate::native::search_window::engine_button_width(
                    prefs,
                    Some(&engine.name),
                    search_width,
                ) as f32;
                state.paint_search_engine_content(
                    p,
                    engine,
                    rect(
                        x + width - padding.right as f32 - button,
                        search_top + padding.top as f32,
                        button,
                        search_height - (padding.top + padding.bottom) as f32,
                    ),
                    view.dpi,
                    prefs,
                );
            }
            if !PANEL.with_borrow(|p| p.appearance_tab == super::appearance::Tab::Engines) {
                return;
            }
            let menu_top = menu_options_top(preview_height) as f32 - offset;
            let preview_width = MENU_PREVIEW_WIDTH as f32;
            p.text(
                super::tr("菜单预览"),
                rect(x, menu_top + 10.0, preview_width, 20.0),
                colors.detail,
                Label::ControlDetail,
            );
            let size = if prefs.show_engine_names { 54.0 } else { 42.0 };
            let tile_height = if prefs.show_engine_names { 58.0 } else { 42.0 };
            let top = menu_top + 66.0 - tile_height / 2.0;
            let gap = 6.0;
            let first = x + (preview_width - (size * 3.0 + gap * 2.0)) / 2.0;
            let icon_size = 20;
            let icon_center_y = if prefs.show_engine_names {
                20.0
            } else {
                tile_height / 2.0
            };
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
                let left = first + index as f32 * (size + gap);
                let area = rect(left, top, size, tile_height);
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
                state.paint_engine_variant(
                    p,
                    engine,
                    (
                        left + (size - icon_size as f32) / 2.0,
                        top + icon_center_y - icon_size as f32 / 2.0,
                    ),
                    icon_size,
                    view.dpi,
                    prefs.multicolor_engine_icons,
                );
                if prefs.show_engine_names {
                    p.text(
                        &engine.name,
                        rect(left + 3.0, top + 34.0, size - 6.0, 18.0),
                        colors.text,
                        Label::ControlDetail,
                    );
                }
            }
        } else {
            let stride = (width - 28.0) / 5.0;
            let pin_top = (super::engines::PIN_TOP + 13) as f32 - offset;
            for index in 0..5 {
                let left = x + 14.0 + index as f32 * stride;
                let area = rect(left, pin_top, stride - 6.0, 30.0);
                p.rounded(area, 5.0, colors.button, false);
                p.rounded(area, 5.0, colors.border, true);
                p.text(
                    &format!("Alt+{}", index + 1),
                    rect(left + 5.0, pin_top, 35.0, 30.0),
                    colors.detail,
                    Label::Hint,
                );
                if let Some(engine) = prefs
                    .pins
                    .get(index)
                    .and_then(|id| engines.iter().find(|e| &e.id == id))
                {
                    state.paint_engine(p, engine, left + 44.0, pin_top + 8.0, 14);
                    if stride > 93.0 {
                        p.text(
                            &engine.name,
                            rect(left + 63.0, pin_top, stride - 69.0, 30.0),
                            colors.text,
                            Label::Hint,
                        );
                    }
                } else {
                    p.text(
                        tr("空"),
                        rect(left + 44.0, pin_top, stride - 52.0, 30.0),
                        colors.detail,
                        Label::Hint,
                    );
                }
            }
        }
    } else if page == 2 && !unsafe { GetDlgItem(hwnd, 23) }.is_null() {
        for (y, multicolor) in [(276.0, false), (349.0, true)] {
            let y = y - offset;
            if y > 96.0 || y < -36.0 {
                continue;
            }
            let preview = PANEL.with_borrow(|p| p.view.engine_icon.clone());
            if let Some(mut engine) = preview {
                engine.color =
                    u32::from_str_radix(&super::field(hwnd, 23), 16).unwrap_or(colors.accent);
                engine.name = super::field(hwnd, 20);
                if engine.name.is_empty() {
                    engine.name = "S".into();
                }
                state.paint_engine_variant(p, &engine, (x + 14.0, y), 36, view.dpi, multicolor);
            }
        }
    }
}

fn surface_horizontal(surface: Surface, width: i32) -> (i32, i32) {
    let delta = width - CONTENT_WIDTH;
    (
        surface.x + if surface.sizing == RIGHT { delta } else { 0 },
        surface.width + if surface.sizing == STRETCH { delta } else { 0 },
    )
}

pub(super) fn hover_amount(hwnd: HWND) -> f32 {
    with_view(unsafe { GetParent(hwnd) }, |view| {
        view.hover_amount(HoverTarget::control(hwnd), Instant::now())
    })
}

pub(super) fn row_hover_amount(list: HWND, row: usize, action: usize) -> f32 {
    PANEL.with_borrow(|p| {
        p.view
            .hover_amount(HoverTarget::row(list, row, action), Instant::now())
    })
}

#[derive(Clone, Copy)]
pub(super) enum ButtonStyle {
    Standard,
    Primary,
    Subtle,
    Card,
}

#[derive(Default)]
pub(super) struct ButtonState {
    pub hover: f32,
    pub focused: bool,
    pub pressed: bool,
    pub disabled: bool,
}

pub(super) fn paint_button_face(
    p: &mut Painter,
    area: D2D_RECT_F,
    colors: Palette,
    style: ButtonStyle,
    state: ButtonState,
) {
    let (base, radius) = match style {
        ButtonStyle::Standard => (colors.button, 6.0),
        ButtonStyle::Primary => (colors.accent, 6.0),
        ButtonStyle::Subtle => (colors.card, 6.0),
        ButtonStyle::Card => (colors.card, 7.0),
    };
    let primary = matches!(style, ButtonStyle::Primary) && !state.disabled;
    let subtle = matches!(style, ButtonStyle::Subtle);
    if state.disabled {
        if !subtle {
            p.rounded(area, radius, colors.card, false);
        }
    } else if primary {
        p.rounded(area, radius, base, false);
        let shade = if state.pressed {
            0.18
        } else {
            0.10 * state.hover
        };
        if shade > 0.0 {
            p.rounded_alpha(area, radius, 0, shade, false);
        }
    } else if state.pressed {
        p.rounded(area, radius, colors.selection, false);
    } else if subtle {
        if state.hover > 0.0 {
            p.rounded_alpha(area, radius, colors.hover, state.hover, false);
        }
    } else {
        p.rounded(area, radius, blend(base, colors.hover, state.hover), false);
    }
    if state.focused || primary {
        p.rounded(area, radius, colors.accent, true);
    } else if !subtle {
        p.rounded(area, radius, colors.border, true);
    }
}

fn paint_switch(
    p: &mut Painter,
    colors: Palette,
    x: f32,
    y: f32,
    amount: f32,
    enabled: bool,
    pressed: bool,
    hover: f32,
) {
    let track = rect(x, y, 42.0, 18.0);
    let tint = if enabled { amount } else { 0.0 };
    let hover = if enabled { hover } else { 0.0 };
    let off = blend(
        colors.button,
        colors.hover,
        if pressed { 1.0 } else { hover },
    );
    let on = blend(colors.accent, 0, 0.10 * hover);
    let fill = blend(off, on, tint);
    p.rounded(track, 9.0, fill, false);
    p.rounded(track, 9.0, blend(colors.border, colors.accent, tint), true);
    let width = if pressed { 14.0 } else { 10.0 };
    p.rounded(
        rect(x + 5.0 + (32.0 - width) * amount, y + 4.0, width, 10.0),
        5.0,
        blend(colors.detail, colors.primary_text, tint),
        false,
    );
}

pub(super) fn control_surface(hwnd: HWND, child: HWND, colors: Palette) -> u32 {
    let viewport = Viewport::of(hwnd);
    let inside = with_view(hwnd, |view| {
        let Some((_, area)) = view.controls.iter().find(|(handle, _)| *handle == child) else {
            return false;
        };
        let y = area.y;
        view.surfaces.iter().any(|&surface| {
            let delta = viewport.width - CONTENT_WIDTH;
            let x = area.x + if area.sizing == RIGHT { delta } else { 0 };
            let (left, width) = surface_horizontal(surface, viewport.width);
            (left..left + width).contains(&x) && (surface.top..surface.bottom).contains(&y)
        })
    });
    if inside {
        colors.card
    } else {
        colors.background
    }
}

fn edit_frame(bounds: RECT, dpi: u32, layout: Layout) -> RECT {
    RECT {
        left: bounds.left - dip(8, dpi),
        right: bounds.right
            + dip(
                if layout.suffix.is_some() {
                    38
                } else if layout.trailing_icon.is_some() {
                    30
                } else {
                    8
                },
                dpi,
            ),
        ..bounds
    }
}

fn invalidate_edit_frame(parent: HWND, child: HWND) {
    let viewport = Viewport::of(parent);
    let frame = with_view(parent, |view| {
        let (_, layout) = view.controls.iter().find(|(hwnd, _)| *hwnd == child)?;
        let area = layout.place(viewport, view.scroll);
        Some(edit_frame(area, viewport.dpi, *layout))
    });
    if let Some(mut frame) = frame {
        unsafe {
            // Include the stroke centered on the frame edge.
            let padding = dip(1, viewport.dpi);
            InflateRect(&mut frame, padding, padding);
            InvalidateRect(parent, &frame, 0);
        }
    }
}

pub(super) fn paint(hwnd: HWND, dc: HDC, invalid: RECT) {
    let viewport = Viewport::of(hwnd);
    let colors = palette();
    let dialog = hwnd == super::color_picker::window();
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
        if !dialog {
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
    }
    let (page, editing, font_picker) =
        PANEL.with_borrow(|p| (p.page, p.edit_engine, p.font_picker.is_some()));
    let show_preview = !dialog && !font_picker;
    let (scroll, surfaces, fields, row_icons) = with_view(hwnd, |view| {
        let fields = view
            .controls
            .iter()
            .filter_map(|(child, area)| {
                area.edit.then(|| {
                    let bounds = area.place(viewport, view.scroll);
                    (
                        *child,
                        edit_frame(bounds, viewport.dpi, *area),
                        area.suffix,
                        area.trailing_icon,
                    )
                })
            })
            .collect::<Vec<_>>();
        (
            view.scroll,
            view.surfaces.clone(),
            fields,
            view.row_icons.clone(),
        )
    });
    let left = dip(viewport.left, viewport.dpi).max(invalid.left);
    let right = dip(viewport.left + viewport.width, viewport.dpi).min(invalid.right);
    let unit = 96.0 / viewport.dpi as f32;
    let layer_x = viewport.left as f32 - left as f32 * unit;
    let state = crate::native::session();
    let search_preview_bottom = state
        .as_ref()
        .filter(|_| page == 1 && show_preview)
        .map(|state| {
            super::appearance::SEARCH_PREVIEW_TOP
                + super::appearance::search_preview_height(
                    &state.app.borrow().settings().preferences,
                )
        });
    for top in (invalid.top..invalid.bottom).step_by(96) {
        let stripe = RECT {
            left,
            top,
            right,
            bottom: (top + 96).min(invalid.bottom),
        };
        let appearance = search_preview_bottom.is_some_and(|bottom| {
            dip(super::appearance::SEARCH_PREVIEW_TOP, viewport.dpi) - scroll < stripe.bottom
                && dip(bottom, viewport.dpi) - scroll > stripe.top
        });
        if !appearance
            && !surfaces.iter().any(|surface| {
                dip(surface.top, viewport.dpi) - scroll < stripe.bottom
                    && dip(surface.bottom, viewport.dpi) - scroll > stripe.top
            })
            && !fields.iter().any(|(_, frame, _, _)| {
                frame.top - 1 < stripe.bottom && frame.bottom + 1 > stripe.top
            })
        {
            continue;
        }
        with_painter(dc, stripe, viewport.dpi, |painter| {
            painter.clear(colors.background);
            let offset = (scroll + top) as f32 * unit;
            for &surface in &surfaces {
                let y = surface.top;
                let end = surface.bottom;
                if end <= y {
                    continue;
                }
                let (x, width) = surface_horizontal(surface, viewport.width);
                let area = rect(
                    layer_x + x as f32 + 0.5,
                    y as f32 - offset + 0.5,
                    width as f32 - 1.0,
                    (end - y) as f32 - 1.0,
                );
                painter.rounded(area, 7.0, colors.card, false);
                painter.rounded(area, 7.0, colors.border, true);
                for &line in surface.dividers {
                    if line < end {
                        painter.rounded(
                            rect(
                                layer_x + x as f32 + 1.0,
                                line as f32 - offset,
                                width as f32 - 2.0,
                                1.0,
                            ),
                            0.0,
                            colors.border,
                            false,
                        );
                    }
                }
            }
            for &(source, x, y) in &row_icons {
                let top = dip(y, viewport.dpi) - scroll;
                if top < stripe.bottom && top + dip(19, viewport.dpi) > stripe.top {
                    icon(
                        painter,
                        source,
                        layer_x + x as f32,
                        y as f32 - offset,
                        18.0,
                        viewport.dpi,
                        colors.detail,
                    );
                }
            }
            #[cfg(feature = "ocr")]
            if !dialog
                && page == super::OCR_PAGE
                && let Some(angle) = super::ocr::progress_angle()
            {
                let y = (super::ocr::STATUS_TOP + 11) as f32 - offset;
                if y + 10.0 > 0.0 && y - 10.0 < (stripe.bottom - stripe.top) as f32 * unit {
                    for index in 0..8 {
                        let turn = angle + index as f32 * std::f32::consts::FRAC_PI_4;
                        let dot = rect(
                            layer_x + 22.0 + turn.cos() * 6.0,
                            y + turn.sin() * 6.0,
                            2.5,
                            2.5,
                        );
                        painter.rounded_alpha(
                            dot,
                            1.25,
                            colors.accent,
                            (index + 2) as f32 / 9.0,
                            false,
                        );
                    }
                }
            }
            for &(child, frame, suffix, trailing_icon) in &fields {
                let area = rect(
                    (frame.left - left) as f32 * unit,
                    (frame.top - top) as f32 * unit,
                    (frame.right - frame.left) as f32 * unit,
                    (frame.bottom - frame.top) as f32 * unit,
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
                if let Some(suffix) = suffix {
                    painter.text(
                        suffix,
                        rect(area.right - 31.0, area.top, 29.0, area.bottom - area.top),
                        colors.detail,
                        Label::ControlDetail,
                    );
                }
                if let Some(asset) = trailing_icon {
                    icon(
                        painter,
                        asset,
                        area.right - 24.0,
                        area.top + (area.bottom - area.top - 14.0) / 2.0,
                        14.0,
                        viewport.dpi,
                        colors.detail,
                    );
                }
            }
            if show_preview && let Some(state) = &state {
                paint_preview(
                    state, hwnd, painter, colors, viewport, page, editing, layer_x, offset,
                );
            }
        });
    }
}

#[cfg(feature = "ocr")]
pub(super) fn invalidate_model_indicator(hwnd: HWND) {
    let viewport = Viewport::of(hwnd);
    let scroll = PANEL.with_borrow(|p| p.view.scroll);
    let area = scaled_rect(
        viewport.left + 12,
        super::ocr::STATUS_TOP,
        23,
        22,
        viewport.dpi,
        scroll,
    );
    unsafe { InvalidateRect(hwnd, &area, 0) };
}

pub(super) fn invalidate_menu_preview(hwnd: HWND, prefs: &yisou::settings::Preferences) {
    use super::appearance::{MENU_PREVIEW_WIDTH, menu_options_top, search_preview_height};

    let viewport = Viewport::of(hwnd);
    let scroll = PANEL.with_borrow(|p| p.view.scroll);
    let height = search_preview_height(prefs);
    let menu = scaled_rect(
        viewport.left,
        menu_options_top(height),
        MENU_PREVIEW_WIDTH,
        super::components::ROW * 2 + CARD_GAP,
        viewport.dpi,
        scroll,
    );
    unsafe { InvalidateRect(hwnd, &menu, 0) };
}

pub(super) fn invalidate_search_preview(hwnd: HWND, prefs: &yisou::settings::Preferences) {
    let viewport = Viewport::of(hwnd);
    let scroll = PANEL.with_borrow(|p| p.view.scroll);
    let preview = scaled_rect(
        viewport.left,
        super::appearance::SEARCH_PREVIEW_TOP,
        viewport.width,
        super::appearance::search_preview_height(prefs),
        viewport.dpi,
        scroll,
    );
    unsafe {
        RedrawWindow(
            hwnd,
            &preview,
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
        );
    }
}

pub(super) fn control_color(hwnd: HWND, dc: HDC, child: HWND, message: u32) -> LRESULT {
    let colors = palette();
    if control_id(child) == 115
        && let Some(state) = crate::native::session()
    {
        let search = state.search_preview_colors();
        unsafe {
            SetTextColor(dc, color_ref(search.text));
            SetBkColor(dc, color_ref(search.surface));
            SetDCBrushColor(dc, color_ref(search.surface));
            return GetStockObject(DC_BRUSH) as isize;
        }
    }
    let rgb = with_view(hwnd, |view| {
        if message == WM_CTLCOLOREDIT {
            return (colors.button, colors.text);
        }
        if message == WM_CTLCOLORLISTBOX {
            return (
                if control_id(child) == NAV as i32 {
                    colors.rail
                } else {
                    colors.background
                },
                colors.text,
            );
        }
        let Some((_, area)) = view.controls.iter().find(|(h, _)| *h == child) else {
            return (colors.rail, colors.text);
        };
        (
            if area.edit || control_id(child) == 115 {
                colors.button
            } else {
                0
            },
            if let Some(color) = super::recorder::status_color(control_id(child)) {
                if crate::native::backdrop::high_contrast() {
                    colors.text
                } else {
                    color
                }
            } else if control_id(child) == super::pages::EDIT_STATUS as i32
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
    let rgb = (
        if rgb.0 == 0 {
            control_surface(hwnd, child, colors)
        } else {
            rgb.0
        },
        rgb.1,
    );
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

fn paint_engine_row(
    p: &mut Painter,
    state: &State,
    colors: Palette,
    dpi: u32,
    area: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F,
    index: usize,
    selected: bool,
    focused: bool,
    placeholder: bool,
    hover: [f32; 2],
) {
    let width = area.right - area.left;
    let height = area.bottom - area.top;
    let box_at = |x, y, w, h| rect(area.left + x, area.top + y, w, h);
    let card = box_at(0.5, 0.5, width - 1.0, height - 1.0);
    p.rounded(
        card,
        7.0,
        if selected {
            colors.selection
        } else {
            colors.card
        },
        false,
    );
    p.rounded(
        card,
        7.0,
        if focused {
            colors.accent
        } else {
            colors.border
        },
        true,
    );
    if placeholder {
        p.rounded(
            box_at(7.0, 3.0, width - 14.0, height - 6.0),
            6.0,
            colors.selection,
            false,
        );
        for x in (13..width as i32 - 12).step_by(8) {
            for y in [4.0, height - 5.0] {
                p.rounded(box_at(x as f32, y, 4.0, 1.0), 0.5, colors.accent, false);
            }
        }
        return;
    }
    let app = state.app.borrow();
    let index = PANEL.with_borrow(|panel| panel.engine_rows.get(index).copied());
    let Some(engine) = index.and_then(|index| app.all_engines().get(index)) else {
        p.text(
            tr("没有符合条件的引擎"),
            box_at(15.0, 0.0, width - 30.0, height),
            colors.detail,
            Label::Body,
        );
        return;
    };
    let prefs = &app.settings().preferences;
    let default = engine.id == prefs.default_engine;
    let enabled = app.settings().engine_enabled(&engine.id);
    let movable = app.engines().len() > 1 || !enabled;
    for column in 0..2 {
        for dot in 0..3 {
            p.rounded(
                box_at(
                    15.0 + column as f32 * 4.0,
                    height / 2.0 - 5.0 + dot as f32 * 4.0,
                    2.0,
                    2.0,
                ),
                1.0,
                colors.detail,
                false,
            );
        }
    }
    state.paint_engine_variant(
        p,
        engine,
        (area.left + 35.0, area.top + (height - 22.0) / 2.0),
        22,
        dpi,
        prefs.multicolor_engine_icons,
    );
    p.text(
        &engine.name,
        box_at(64.0, 0.0, (width - 374.0).max(1.0), height),
        if enabled { colors.text } else { colors.detail },
        Label::Body,
    );
    let alias = if engine.alias.is_empty() {
        "—".into()
    } else {
        format!("#{}", engine.alias)
    };
    p.text(
        &alias,
        box_at(width - 297.0, 0.0, 104.0, height),
        colors.detail,
        Label::Hint,
    );
    let pinned = enabled
        .then(|| prefs.pins.iter().position(|id| id == &engine.id))
        .flatten();
    let status = match (default, pinned) {
        (true, Some(index)) if index < 5 => format!("{} · Alt+{}", tr("默认"), index + 1),
        (true, _) => tr("默认").into(),
        (false, Some(index)) if index < 5 => format!("Alt+{}", index + 1),
        (false, Some(_)) => tr("已置顶").into(),
        _ => String::new(),
    };
    p.text(
        &status,
        box_at(width - 187.0, 0.0, 90.0, height),
        colors.accent,
        Label::Hint,
    );
    paint_switch(
        p,
        colors,
        area.right - 92.0,
        area.top + (height - 18.0) / 2.0,
        if enabled { 1.0 } else { 0.0 },
        movable,
        false,
        hover[1],
    );
    paint_button_face(
        p,
        box_at(
            width - 39.0,
            (height - CONTROL_HEIGHT as f32) / 2.0,
            CONTROL_HEIGHT as f32,
            CONTROL_HEIGHT as f32,
        ),
        colors,
        ButtonStyle::Subtle,
        ButtonState {
            hover: hover[0],
            ..ButtonState::default()
        },
    );
    for dot in 0..3 {
        p.rounded(
            box_at(
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
}

fn paint_navigation(hwnd: HWND, dc: HDC, bounds: RECT, update: RECT) {
    if update.right <= update.left || update.bottom <= update.top {
        return;
    }
    let (page, hover) = PANEL.with_borrow(|p| (p.page, p.view.nav_hover));
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let unit = 96.0 / dpi as f32;
    let colors = palette();
    let width = bounds.right as f32 * unit;
    let focus = unsafe {
        (GetFocus() == hwnd
            && SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS as isize == 0)
            .then(|| SendMessageW(hwnd, LB_GETCARETINDEX, 0, 0) as usize)
    };
    with_painter(dc, bounds, dpi, |p| {
        p.clear(colors.rail);
        for (index, page_id) in nav_pages().enumerate() {
            let mut row = RECT::default();
            let result = unsafe {
                SendMessageW(
                    hwnd,
                    LB_GETITEMRECT,
                    index,
                    (&mut row as *mut RECT) as isize,
                )
            };
            if result < 0 || row.bottom <= update.top || row.top >= update.bottom {
                continue;
            }
            let y = row.top as f32 * unit;
            let height = (row.bottom - row.top) as f32 * unit;
            let selected = page_id == page;
            if selected || hover == Some(index) {
                let fill = if selected {
                    colors.selection
                } else {
                    colors.hover
                };
                p.rounded(rect(0.0, y, width, height - 3.0), 5.0, fill, false);
            }
            if selected {
                p.rounded(
                    rect(0.0, y + (height - 20.0) / 2.0, 3.0, 17.0),
                    1.5,
                    colors.accent,
                    false,
                );
            }
            if focus == Some(index) {
                p.rounded(
                    rect(2.0, y + 3.0, width - 5.0, height - 6.0),
                    4.0,
                    colors.accent,
                    true,
                );
            }
            icon(
                p,
                NAV_ICONS[page_id],
                11.0,
                y + (height - 21.0) / 2.0,
                18.0,
                dpi,
                if selected {
                    colors.accent
                } else {
                    colors.detail
                },
            );
            p.text(
                tr(PAGES[page_id]),
                rect(39.0, y, width - 45.0, height - 3.0),
                colors.text,
                Label::Body,
            );
        }
    });
}

pub(super) fn draw_item(state: &State, hwnd: HWND, item: &DRAWITEMSTRUCT) {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let unit = 96.0 / dpi as f32;
    let b = item.rcItem;
    let width = (b.right - b.left) as f32 * unit;
    let height = (b.bottom - b.top) as f32 * unit;
    let colors = palette();
    if item.CtlType == ODT_COMBOBOX {
        super::dropdown::draw_option(item);
        return;
    }
    let page = PANEL.with_borrow(|p| p.page);
    let id = item.CtlID as usize;
    let index = item.itemID as usize;
    let focused = item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT) == ODS_FOCUS;
    let selected = item.itemState & ODS_SELECTED != 0;
    if id == NAV || super::recorder::is_trigger(id as usize) {
        unsafe { InvalidateRect(item.hwndItem, &b, 0) };
        return;
    }
    if id == 10 && page == 2 {
        if PANEL.with_borrow(|panel| panel.engine_drag.is_some()) {
            unsafe { InvalidateRect(item.hwndItem, &b, 0) };
        } else {
            with_painter(item.hDC, b, dpi, |p| {
                p.clear(colors.background);
                paint_engine_row(
                    p,
                    state,
                    colors,
                    dpi,
                    rect(0.0, 0.0, width, height - CARD_GAP as f32),
                    index,
                    selected,
                    focused,
                    false,
                    [
                        row_hover_amount(item.hwndItem, index, 0),
                        row_hover_amount(item.hwndItem, index, 1),
                    ],
                );
            });
        }
        return;
    }
    if id == 10 {
        let text = item_text(item);
        let height = height - CARD_GAP as f32;
        let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
        with_painter(item.hDC, b, dpi, |p| {
            p.clear(colors.background);
            paint_button_face(
                p,
                area,
                colors,
                ButtonStyle::Card,
                ButtonState {
                    hover: row_hover_amount(item.hwndItem, index, 0),
                    focused,
                    pressed: selected,
                    disabled: false,
                },
            );
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
            if index == 0
                || detail.is_empty()
                || !paint_browser_icon(p, detail, b.bottom - b.top - dip(CARD_GAP, dpi), dpi)
            {
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
                rect(
                    64.0,
                    center - if detail.is_empty() { 11.5 } else { 21.0 },
                    width - 77.0,
                    23.0,
                ),
                colors.text,
                Label::Body,
            );
            p.text(
                detail,
                rect(64.0, center + 2.0, width - 77.0, 18.0),
                colors.detail,
                Label::Hint,
            );
        });
    } else {
        let disabled = item.itemState & ODS_DISABLED != 0;
        let hover = if disabled {
            0.0
        } else {
            hover_amount(item.hwndItem)
        };
        let label = text_of(item.hwndItem);
        let toggle = with_view(hwnd, |view| {
            view.controls
                .iter()
                .any(|(child, layout)| *child == item.hwndItem && layout.toggle)
        });
        let primary = !disabled && matches!(id, 122 | 126 | 141);
        with_painter(item.hDC, b, dpi, |p| {
            if let Some(tab) = super::appearance::Tab::from_id(id) {
                let active = PANEL.with_borrow(|p| tab == p.appearance_tab);
                p.clear(colors.background);
                if focused {
                    p.rounded(
                        rect(2.5, 2.5, width - 5.0, height - 7.0),
                        4.0,
                        colors.accent,
                        true,
                    );
                }
                p.text(
                    &label,
                    rect(0.0, 0.0, width, height - 4.0),
                    blend(
                        colors.text,
                        colors.detail,
                        if selected { 1.0 } else { hover },
                    ),
                    Label::Control,
                );
                if active {
                    let amount = control_state_amount(hwnd, item.hwndItem, active);
                    let marker = 4.0 + 12.0 * amount;
                    p.rounded_alpha(
                        rect((width - marker) / 2.0, height - 3.0, marker, 3.0),
                        1.5,
                        colors.accent,
                        amount,
                        false,
                    );
                }
                return;
            }
            p.clear(control_surface(hwnd, item.hwndItem, colors));
            if page == 4 && id == 142 {
                let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
                paint_button_face(
                    p,
                    area,
                    colors,
                    ButtonStyle::Card,
                    ButtonState {
                        hover,
                        focused,
                        pressed: selected,
                        disabled,
                    },
                );
                icon(
                    p,
                    NAV_ICONS[4],
                    36.0,
                    (height - 18.0) / 2.0,
                    18.0,
                    dpi,
                    colors.detail,
                );
                p.text(
                    &label,
                    rect(64.0, 0.0, width - 77.0, height),
                    colors.text,
                    Label::Body,
                );
                return;
            }
            if matches!(id, 145..=147 | 157) {
                super::recorder::paint_reset(p, width, height, dpi, colors, focused, hover);
                return;
            }
            if matches!(id, 537..=539 | 556 | 637..=639 | 656) {
                super::recorder::paint_link(
                    p,
                    rect(0.0, 0.0, width, height),
                    &label,
                    colors,
                    focused,
                    hover,
                );
                return;
            }
            if toggle {
                let on = label.ends_with(&format!(" ({})", tr("开")));
                let amount = control_state_amount(hwnd, item.hwndItem, on);
                paint_switch(
                    p,
                    colors,
                    (width - 42.0) / 2.0,
                    (height - 18.0) / 2.0,
                    amount,
                    !disabled,
                    selected,
                    hover,
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
            let style = if primary {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Standard
            };
            paint_button_face(
                p,
                area,
                colors,
                style,
                ButtonState {
                    hover,
                    focused,
                    pressed: selected,
                    disabled,
                },
            );
            let text_color = if disabled {
                colors.detail
            } else if primary {
                colors.primary_text
            } else {
                colors.text
            };
            if super::color_picker::paint_button(p, id, item, width, height, dpi) {
                return;
            }
            let step_icon = match id {
                id if id == super::fonts::DECREASE
                    || super::appearance::search_steppers()
                        .any(|stepper| id == stepper.decrease) =>
                {
                    Some((
                        "minus",
                        include_bytes!("../../../assets/icons/tabler/minus.svg").as_slice(),
                    ))
                }
                id if id == super::fonts::INCREASE
                    || super::appearance::search_steppers()
                        .any(|stepper| id == stepper.increase) =>
                {
                    Some((
                        "plus",
                        include_bytes!("../../../assets/icons/tabler/plus.svg").as_slice(),
                    ))
                }
                _ => None,
            };
            if let Some(source) = step_icon {
                let size = 16.0;
                icon(
                    p,
                    source,
                    (width - size) / 2.0,
                    (height - size) / 2.0,
                    size,
                    dpi,
                    text_color,
                );
                return;
            }
            let tint = if id == super::pages::MONO_COLOR {
                PANEL.with_borrow(|p| p.view.engine_icon.as_ref().and_then(|e| e.mono_color))
            } else {
                None
            };
            let padding = if id == super::pages::ACCENT_HELP {
                4
            } else {
                BUTTON_PADDING
            };
            if let Some(rgb) = tint {
                let swatch = rect(padding as f32, (height - 14.0) / 2.0, 14.0, 14.0);
                p.rounded(swatch, 3.0, rgb, false);
                p.rounded(swatch, 3.0, colors.detail, true);
                p.text(
                    &label,
                    rect(
                        (padding + BUTTON_ICON_SPACE) as f32,
                        0.0,
                        width - (2 * padding + BUTTON_ICON_SPACE) as f32,
                        height,
                    ),
                    text_color,
                    Label::Control,
                );
            } else {
                p.text(
                    &label,
                    rect(padding as f32, 0.0, width - (2 * padding) as f32, height),
                    text_color,
                    Label::Control,
                );
            }
        });
    }
}
