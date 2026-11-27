use super::{control, dip, fluent, register, w};
use crate::native::paint::{Painter, rect};
use std::{
    io,
    ptr::null,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{
        Controls::WM_MOUSELEAVE,
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            GetCapture, ReleaseCapture, SetCapture, TME_LEAVE, TME_QUERY, TRACKMOUSEEVENT,
            TrackMouseEvent,
        },
        WindowsAndMessaging::*,
    },
};

pub(super) const ID: i32 = 28;
const CLASS: *const u16 = w!("YiSou.Rust.Settings.Scrollbar");
pub(super) const HIT: i32 = 14;
pub(super) const TIMER: usize = 31;
const IDLE: Duration = Duration::from_millis(800);
const MOTION: Duration = Duration::from_millis(140);

#[derive(Clone, Copy, Default)]
struct Appearance {
    width: f32,
    opacity: f32,
    pressed: f32,
}

pub(super) struct Motion {
    edge: bool,
    hover: bool,
    pressed: bool,
    awake: Option<Instant>,
    transition: Option<(Instant, Appearance)>,
    target: Appearance,
    animate: bool,
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
    }
}

impl Motion {
    const fn new() -> Self {
        Self {
            edge: false,
            hover: false,
            pressed: false,
            awake: None,
            transition: None,
            target: Appearance {
                width: 0.0,
                opacity: 0.0,
                pressed: 0.0,
            },
            animate: false,
        }
    }

    pub(super) fn start() -> Self {
        Self {
            animate: fluent::animations_enabled(),
            ..Self::new()
        }
    }

    fn appearance(&self, now: Instant) -> Appearance {
        let Some((at, from)) = self.transition else {
            return self.target;
        };
        let progress = now.duration_since(at).as_secs_f32() / MOTION.as_secs_f32();
        let eased = fluent::easing(progress, 0.2, 0.2, 1.0);
        let mix = |a, b| a + (b - a) * eased;
        Appearance {
            width: mix(from.width, self.target.width),
            opacity: mix(from.opacity, self.target.opacity),
            pressed: mix(from.pressed, self.target.pressed),
        }
    }

    pub(super) fn update(
        &mut self,
        hwnd: HWND,
        edge: bool,
        hover: bool,
        pressed: bool,
        wake: bool,
    ) -> bool {
        self.edge = edge;
        self.hover = hover;
        self.pressed = pressed;
        if wake {
            self.awake = Some(Instant::now() + IDLE);
        }
        self.tick(hwnd) || wake
    }

    pub(super) fn wake(&mut self, hwnd: HWND) {
        self.update(hwnd, self.edge, self.hover, self.pressed, true);
    }

    pub(super) fn tick(&mut self, hwnd: HWND) -> bool {
        let now = Instant::now();
        self.awake = self.awake.filter(|until| *until > now);
        let target = Appearance {
            width: if self.hover || self.pressed { 1.0 } else { 0.0 },
            opacity: if self.edge || self.pressed || self.awake.is_some() {
                1.0
            } else {
                0.0
            },
            pressed: if self.pressed { 1.0 } else { 0.0 },
        };
        let changing = target.width != self.target.width
            || target.opacity != self.target.opacity
            || target.pressed != self.target.pressed;
        let was_animating = self.transition.is_some();
        if changing {
            self.transition = self.animate.then(|| (now, self.appearance(now)));
            self.target = target;
        } else {
            self.transition = self
                .transition
                .filter(|(at, _)| now.duration_since(*at) < MOTION);
        }
        unsafe {
            if self.transition.is_some() {
                SetTimer(hwnd, TIMER, 15, None);
            } else if let Some(until) = self.awake {
                SetTimer(
                    hwnd,
                    TIMER,
                    (until.duration_since(now).as_millis() as u32).max(10),
                    None,
                );
            } else {
                KillTimer(hwnd, TIMER);
            }
        }
        changing || was_animating
    }

    pub(super) fn draw(&self, p: &mut Painter, right: f32, y: f32, length: f32) {
        let state = self.appearance(Instant::now());
        if state.opacity <= 0.0 || length <= 0.0 {
            return;
        }
        let width = 4.0 + state.width * 4.0;
        let shade = (138.0 - state.width * 34.0 - state.pressed * 27.0).round() as u32;
        let colors = fluent::palette();
        let rgb = if crate::native::backdrop::high_contrast() {
            colors.text
        } else {
            (shade << 16) | (shade << 8) | (shade - 2)
        };
        p.rounded_alpha(
            rect(right - width, y, width, length),
            width * 0.5,
            rgb,
            state.opacity,
            false,
        );
    }
}

pub(super) fn register_window() -> io::Result<()> {
    register(CLASS, Some(proc))
}

pub(super) fn create(parent: HWND) {
    fluent::with_view_mut(parent, |view| view.scrollbar = Motion::start());
    control(parent, CLASS, "", 0, ID as usize, (0, 0, 0, 0));
}

pub(super) fn layout(parent: HWND) {
    unsafe {
        let bar = GetDlgItem(parent, ID);
        if bar.is_null() {
            return;
        }
        let mut bounds = RECT::default();
        GetClientRect(parent, &mut bounds);
        let dpi = GetDpiForWindow(parent);
        let width = dip(HIT, dpi);
        let inset = dip(3, dpi);
        SetWindowPos(
            bar,
            std::ptr::null_mut(),
            bounds.right - width - inset,
            inset,
            width,
            (bounds.bottom - 2 * inset).max(1),
            SWP_NOACTIVATE
                | if Metrics::of(parent).is_some() {
                    SWP_SHOWWINDOW
                } else {
                    SWP_HIDEWINDOW
                },
        );
        InvalidateRect(bar, null(), 0);
    }
}

struct Metrics {
    parent: HWND,
    page: i32,
    maximum: i32,
    top: i32,
}

impl Metrics {
    fn of(parent: HWND) -> Option<Self> {
        let (top, maximum, page) = fluent::scroll_metrics(parent);
        (maximum > 0).then_some(Self {
            parent,
            page,
            maximum,
            top,
        })
    }

    fn thumb(&self, height: i32, dpi: u32) -> (i32, i32) {
        thumb(self.top, self.maximum, self.page, height, dpi)
    }

    fn scroll_to(&self, top: i32) {
        fluent::scroll_to(self.parent, top);
    }
}

pub(super) fn wake_page(parent: HWND) {
    unsafe {
        let bar = GetDlgItem(parent, ID);
        if !bar.is_null() {
            fluent::with_view_mut(parent, |view| view.scrollbar.wake(bar));
            InvalidateRect(bar, null(), 0);
        }
    }
}

pub(super) fn thumb(top: i32, maximum: i32, page: i32, height: i32, dpi: u32) -> (i32, i32) {
    let length = project(page, maximum + page, height).clamp(dip(32, dpi).min(height), height);
    (project(top, maximum, height - length), length)
}

pub(super) fn project(position: i32, range: i32, target: i32) -> i32 {
    ((i64::from(position) * i64::from(target) + i64::from(range) / 2) / i64::from(range)) as i32
}

fn paint(hwnd: HWND, dc: HDC, bounds: RECT, dpi: u32) {
    let colors = fluent::palette();
    let unit = 96.0 / dpi as f32;
    let width = bounds.right as f32 * unit;
    let parent = unsafe { GetParent(hwnd) };
    let metrics = Metrics::of(parent);
    fluent::with_painter(dc, bounds, dpi, |p| {
        p.clear(colors.background);
        if let Some(metrics) = metrics {
            let (offset, length) = metrics.thumb(bounds.bottom, dpi);
            fluent::with_view(parent, |view| {
                view.scrollbar
                    .draw(p, width, offset as f32 * unit, length as f32 * unit)
            });
        }
    });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let parent = GetParent(hwnd);
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let mut bounds = RECT::default();
                GetClientRect(hwnd, &mut bounds);
                paint(
                    hwnd,
                    BeginPaint(hwnd, &mut ps),
                    bounds,
                    GetDpiForWindow(hwnd).max(96),
                );
                EndPaint(hwnd, &ps);
                0
            }
            WM_LBUTTONDOWN | WM_MOUSEMOVE => {
                let drag = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as i32 - 1;
                if msg == WM_MOUSEMOVE && drag < 0 {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_QUERY,
                        ..Default::default()
                    };
                    TrackMouseEvent(&mut track);
                    if track.hwndTrack != hwnd {
                        track.dwFlags = TME_LEAVE;
                        track.hwndTrack = hwnd;
                        TrackMouseEvent(&mut track);
                    }
                    if fluent::with_view_mut(parent, |view| {
                        view.scrollbar.update(hwnd, true, true, false, false)
                    }) {
                        InvalidateRect(hwnd, null(), 0);
                    }
                    return 0;
                }
                if let Some(metrics) = Metrics::of(parent) {
                    let mut bounds = RECT::default();
                    GetClientRect(hwnd, &mut bounds);
                    let y = i32::from((lp >> 16) as i16);
                    let (start, length) = metrics.thumb(bounds.bottom, GetDpiForWindow(hwnd));
                    if msg == WM_LBUTTONDOWN {
                        if (start..start + length).contains(&y) {
                            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (y - start + 1) as isize);
                            SetCapture(hwnd);
                            fluent::with_view_mut(parent, |view| {
                                view.scrollbar.update(hwnd, true, true, true, true)
                            });
                            InvalidateRect(hwnd, null(), 0);
                        } else {
                            let page = if y < start {
                                -metrics.page
                            } else {
                                metrics.page
                            };
                            metrics.scroll_to(metrics.top + page);
                        }
                    } else {
                        let travel = (bounds.bottom - length).max(1);
                        let position = (y - drag).clamp(0, travel);
                        metrics.scroll_to(project(position, travel, metrics.maximum));
                    }
                }
                0
            }
            WM_LBUTTONUP | WM_CANCELMODE if GetCapture() == hwnd => {
                ReleaseCapture();
                0
            }
            WM_CAPTURECHANGED | WM_MOUSELEAVE => {
                if msg == WM_CAPTURECHANGED {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                let mut cursor = POINT::default();
                let over = msg != WM_MOUSELEAVE
                    && GetCursorPos(&mut cursor) != 0
                    && WindowFromPoint(cursor) == hwnd;
                fluent::with_view_mut(parent, |view| {
                    view.scrollbar.update(
                        hwnd,
                        over,
                        over,
                        GetCapture() == hwnd,
                        msg == WM_CAPTURECHANGED,
                    )
                });
                InvalidateRect(hwnd, null(), 0);
                0
            }
            WM_TIMER if wp == TIMER => {
                if fluent::with_view_mut(parent, |view| view.scrollbar.tick(hwnd)) {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_MOUSEWHEEL => {
                fluent::scroll(parent, WM_MOUSEWHEEL, wp);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
