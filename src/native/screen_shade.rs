use super::{State, instance, personalization_flag, session};
use std::{
    ffi::c_void,
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
        UI::WindowsAndMessaging::*,
    },
    core::BOOL,
    s, w,
};
use yisou::settings::ScreenBackground;

pub(super) const CLASS: *const u16 = w!("YiSou.Rust.ScreenShade");
const OPACITY: u8 = 96;
const MAX_GAUSSIAN_DEVIATION: f32 = 24.0;

mod gaussian;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Background {
    Dim,
    System,
    Gaussian(i32),
}

#[derive(Clone, Copy)]
enum Effect {
    Dim,
    System(SetCompositionAttribute),
    Gaussian(f32),
}

impl Effect {
    fn event(self) -> &'static str {
        match self {
            Self::Dim => "screen_shade_dim",
            Self::System(_) => "screen_shade_blur",
            Self::Gaussian(_) => "screen_shade_gaussian",
        }
    }
}

pub(super) struct ScreenShade {
    windows: Vec<HWND>,
    background: Background,
    effect: Effect,
    gaussian: Option<gaussian::Backdrop>,
}

impl ScreenShade {
    fn new(background: Background, diagnostics: &super::diagnostics::MemoryTrace) -> Option<Self> {
        let mut monitors = Vec::<RECT>::new();
        unsafe {
            EnumDisplayMonitors(
                null_mut(),
                null(),
                Some(collect_monitor),
                &mut monitors as *mut Vec<RECT> as LPARAM,
            );
        }
        if let Background::Gaussian(percent) = background {
            let amount = percent as f32 / 100.0 * MAX_GAUSSIAN_DEVIATION;
            if let Some(shade) = Self::create(&monitors, background, Effect::Gaussian(amount)) {
                return Some(shade);
            }
            diagnostics.sample("screen_shade_gaussian_failed");
        }
        if background != Background::Dim
            && let Some(apply) = blur_api()
            && let Some(shade) = Self::create(&monitors, background, Effect::System(apply))
        {
            return Some(shade);
        }
        Self::create(&monitors, background, Effect::Dim)
    }

    fn create(monitors: &[RECT], background: Background, effect: Effect) -> Option<Self> {
        if monitors.is_empty() {
            return None;
        }
        let mut shade = Self {
            windows: Vec::with_capacity(monitors.len()),
            background,
            effect,
            gaussian: match effect {
                Effect::Gaussian(amount) => Some(gaussian::Backdrop::new(amount).ok()?),
                _ => None,
            },
        };
        let surface = if matches!(effect, Effect::Dim) {
            WS_EX_LAYERED
        } else {
            WS_EX_NOREDIRECTIONBITMAP
        };
        for bounds in monitors {
            unsafe {
                let window = CreateWindowExW(
                    surface | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                    CLASS,
                    null(),
                    WS_POPUP,
                    bounds.left,
                    bounds.top,
                    bounds.right - bounds.left,
                    bounds.bottom - bounds.top,
                    null_mut(),
                    null_mut(),
                    instance(),
                    null(),
                );
                if window.is_null() {
                    return None;
                }
                shade.windows.push(window);
                let ready = match effect {
                    Effect::System(apply) => enable_blur(window, apply),
                    Effect::Dim => SetLayeredWindowAttributes(window, 0, OPACITY, LWA_ALPHA) != 0,
                    Effect::Gaussian(_) => shade.gaussian.as_mut()?.attach(window).is_ok(),
                };
                if !ready {
                    return None;
                }
            }
        }
        Some(shade)
    }
}

impl Drop for ScreenShade {
    fn drop(&mut self) {
        for &window in &self.windows {
            unsafe { DestroyWindow(window) };
        }
    }
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    unsafe {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            (*(data as *mut Vec<RECT>)).push(info.rcMonitor);
        }
    }
    1
}

pub(super) fn transparency_enabled() -> bool {
    personalization_flag(w!("EnableTransparency"), true)
}

#[repr(C)]
struct AccentPolicy {
    state: i32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionAttribute {
    attribute: i32,
    data: *const c_void,
    size: usize,
}

type SetCompositionAttribute = unsafe extern "system" fn(HWND, *const CompositionAttribute) -> BOOL;

fn blur_api() -> Option<SetCompositionAttribute> {
    // The accent policy is an undocumented compositor interface.
    unsafe {
        let user32 = GetModuleHandleW(w!("user32.dll"));
        let address = GetProcAddress(user32, s!("SetWindowCompositionAttribute"))?;
        Some(std::mem::transmute::<_, SetCompositionAttribute>(address))
    }
}

fn enable_blur(window: HWND, apply: SetCompositionAttribute) -> bool {
    const WCA_ACCENT_POLICY: i32 = 19;
    const ACCENT_ENABLE_BLURBEHIND: i32 = 3;
    let policy = AccentPolicy {
        state: ACCENT_ENABLE_BLURBEHIND,
        flags: 0,
        color: 0x10000000,
        animation: 0,
    };
    let attribute = CompositionAttribute {
        attribute: WCA_ACCENT_POLICY,
        data: (&policy as *const AccentPolicy).cast(),
        size: size_of::<AccentPolicy>(),
    };
    unsafe { apply(window, &attribute) != 0 }
}

impl State {
    pub(super) fn refresh_screen_shade(&self) {
        if unsafe { IsWindowVisible(self.window.get()) } != 0 {
            self.show_screen_shade();
        }
    }

    pub(super) fn show_screen_shade(&self) {
        let background = {
            let app = self.app.borrow();
            let prefs = &app.settings().preferences;
            match prefs.screen_background {
                ScreenBackground::Dim => Background::Dim,
                _ if super::backdrop::high_contrast() || !transparency_enabled() => Background::Dim,
                ScreenBackground::System => Background::System,
                ScreenBackground::Gaussian => Background::Gaussian(prefs.background_blur_percent),
            }
        };
        let mut current = self.screen_shade.borrow_mut();
        let previous = if current
            .as_ref()
            .is_none_or(|shade| shade.background != background)
        {
            self.diagnostics.sample("screen_shade_create_begin");
            if let Some(shade) = ScreenShade::new(background, &self.diagnostics) {
                self.diagnostics.sample(shade.effect.event());
                current.replace(shade)
            } else {
                None
            }
        } else {
            None
        };
        let Some(shade) = current.as_ref() else {
            return;
        };
        // Stack the shades immediately under Search and its owned popups.
        let mut after = self.window.get();
        for &window in &shade.windows {
            unsafe {
                SetWindowPos(
                    window,
                    after,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
            }
            after = window;
        }
        drop(previous);
    }

    pub(super) fn hide_screen_shade(&self) {
        if let Some(shade) = self.screen_shade.borrow().as_ref() {
            for &window in &shade.windows {
                unsafe { ShowWindow(window, SW_HIDE) };
            }
        }
    }

    pub(super) fn release_screen_shade(&self) {
        self.screen_shade.take();
    }
}

pub(super) unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_LAYERED as isize != 0 {
                FillRect(dc, &paint.rcPaint, GetStockObject(BLACK_BRUSH));
            }
            EndPaint(hwnd, &paint);
            0
        },
        WM_ERASEBKGND => 1,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_LBUTTONUP => {
            if let Some(state) = session() {
                state.hide(true);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
