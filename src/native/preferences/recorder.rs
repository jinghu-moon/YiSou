use super::{
    components::{CONTROL_HEIGHT, Page, ROW, ROW_WITH_DETAIL},
    fluent::{self, blend},
    *,
};
use crate::native::paint::{Label, Painter, rect};
use std::time::{Duration, Instant};
use yisou::{
    settings::Preferences,
    shortcut::{ALT, CONTROL, SHIFT, WIN},
};

pub(super) const TIMER: usize = 6;
const DANGER: u32 = 0xc75146;
const AMBER: u32 = 0xd29a1a;
const SUCCESS: u32 = 0x37674c;
const MOTION: Duration = Duration::from_millis(200);
const RECORD_INSET: i32 = 4;
const SHAKE: Duration = Duration::from_millis(300);
const CONFIRMED: Duration = Duration::from_millis(700);
const DOT_X: f32 = 12.0;
const DOT_SIZE: f32 = 8.0;
const RESET_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14L4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/></svg>"#;

#[derive(Default)]
pub(super) struct Recorder {
    active: Option<Active>,
    confirmed: Option<(usize, Instant, bool)>,
}

struct Active {
    id: usize,
    phase: Phase,
    started: Instant,
    animate: bool,
}

enum Phase {
    Listening { held: u32, error: Option<Instant> },
    Conflict { key: Shortcut, previous: usize },
}

fn reset_id(id: usize) -> usize {
    if id == 156 { 157 } else { id + 8 }
}

fn status_id(id: usize) -> usize {
    id + 200
}

pub(super) fn is_trigger(id: usize) -> bool {
    matches!(id, 137..=139 | 156 | 248)
}

fn name(id: usize) -> &'static str {
    match id {
        137 => "唤出易搜",
        138 => "划词搜索",
        139 => "剪贴板搜索",
        248 => "截图搜图",
        _ => "OCR 快捷键",
    }
}

fn default_shortcut(id: usize) -> Option<Shortcut> {
    (id == 137).then(Shortcut::default)
}

fn bindings(prefs: &Preferences) -> [(usize, Option<Shortcut>); 5] {
    [
        (137, prefs.main_shortcut),
        (138, prefs.selection.as_ref().map(|b| b.shortcut)),
        (139, prefs.clipboard_shortcut),
        (248, prefs.image_search_shortcut),
        (
            156,
            if cfg!(feature = "ocr") {
                prefs.ocr_shortcut
            } else {
                None
            },
        ),
    ]
}

fn shown(key: Option<Shortcut>) -> String {
    match key {
        None => tr("未设置").into(),
        Some(key) if key == Shortcut::default() => "双击左 Alt".into(),
        Some(key) => {
            let mut names = modifier_names(key.modifiers);
            names.push(key_name(key.key));
            names.join("+")
        }
    }
}

fn modifier_names(mask: u32) -> Vec<String> {
    [
        (CONTROL, "Ctrl"),
        (ALT, "Alt"),
        (SHIFT, "Shift"),
        (WIN, "Win"),
    ]
    .into_iter()
    .filter(|(bit, _)| mask & bit != 0)
    .map(|(_, label)| label.to_owned())
    .collect()
}

fn key_name(key: u32) -> String {
    let label = match key {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0d => "Enter",
        0x20 => "Space",
        0x21 => "PgUp",
        0x22 => "PgDn",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "←",
        0x26 => "↑",
        0x27 => "→",
        0x28 => "↓",
        0x2e => "Del",
        0xba => ";",
        0xbb => "=",
        0xbc => ",",
        0xbd => "-",
        0xbe => ".",
        0xbf => "/",
        0xc0 => "\u{60}",
        0xdb => "[",
        0xdc => "\\",
        0xdd => "]",
        0xde => "'",
        0x30..=0x39 | 0x41..=0x5a => return (key as u8 as char).to_string(),
        0x70..=0x87 => return format!("F{}", key - 0x6f),
        _ => return format!("0x{key:02X}"),
    };
    label.into()
}

pub(super) fn create(
    page: Page,
    id: usize,
    title: &str,
    detail: &str,
    value: Option<Shortcut>,
    top: i32,
) {
    let hwnd = page.0;
    let icon = (
        "keyboard",
        include_bytes!("../../../assets/icons/tabler/keyboard.svg") as &[u8],
    );
    PANEL.with_borrow_mut(|p| p.view.row_icons.push((icon, 14, top + 19)));
    let text_left = 44;
    let reset_left = 235;
    let text_width = reset_left - text_left - 4;
    page.body(title, place(text_left, top + 7, text_width, 23, STRETCH));
    if !detail.is_empty() {
        let normal = page.detail(detail, place(text_left, top + 30, text_width, 21, STRETCH));
        unsafe { SetWindowLongPtrW(normal, GWLP_ID, (id + 300) as isize) };
    } else {
        let normal = page.detail(
            &format!("{} {}", tr("默认"), shown(default_shortcut(id))),
            place(text_left, top + 30, text_width, 21, STRETCH),
        );
        unsafe { SetWindowLongPtrW(normal, GWLP_ID, (id + 300) as isize) };
    }
    let status = page.detail("", place(text_left, top + 29, text_width, 16, STRETCH));
    let row_height = if detail.is_empty() {
        ROW
    } else {
        ROW_WITH_DETAIL
    };
    let button_top = top + (row_height - CONTROL_HEIGHT) / 2;
    let record = action(
        hwnd,
        id,
        &shown(value),
        place(
            267,
            button_top - RECORD_INSET,
            255,
            CONTROL_HEIGHT + 2 * RECORD_INSET,
            RIGHT,
        ),
    );
    let reset = action(
        hwnd,
        reset_id(id),
        "恢复默认",
        place(reset_left, button_top, 28, CONTROL_HEIGHT, RIGHT),
    );
    for (x, command, label) in [(44, id + 400, "覆盖"), (95, id + 500, "取消")] {
        let link = action(hwnd, command, label, place(x, top + 43, 48, 14, 0));
        unsafe { ShowWindow(link, SW_HIDE) };
    }
    unsafe {
        SetWindowLongPtrW(status, GWLP_ID, status_id(id) as isize);
        ShowWindow(status, SW_HIDE);
        ShowWindow(
            reset,
            if value == default_shortcut(id) {
                SW_HIDE
            } else {
                SW_SHOWNA
            },
        );
        InvalidateRect(record, null(), 0);
    }
}

fn set_listening(state: &State, enabled: bool) {
    if let Some(hotkeys) = state.hotkeys.borrow_mut().as_mut() {
        hotkeys.set_recording(enabled);
    }
}

pub(super) fn reset(state: &State, hwnd: HWND) {
    let active = PANEL.with_borrow_mut(|p| std::mem::take(&mut p.recorder).active.is_some());
    if active {
        set_listening(state, false);
    }
    unsafe { KillTimer(hwnd, TIMER) };
}

fn feedback(hwnd: HWND, id: usize, text: &str, conflict: bool) {
    unsafe {
        for (field, show) in [
            (id + 300, text.is_empty()),
            (status_id(id), !text.is_empty()),
            (id + 400, conflict),
            (id + 500, conflict),
        ] {
            let handle = GetDlgItem(hwnd, field as i32);
            if handle.is_null() {
                continue;
            }
            show_control(hwnd, handle, show);
        }
        set_field(hwnd, status_id(id) as i32, text);
        InvalidateRect(GetDlgItem(hwnd, id as i32), null(), 0);
    }
}

fn show_control(parent: HWND, control: HWND, visible: bool) {
    unsafe {
        if (GetWindowLongW(control, GWL_STYLE) as u32 & WS_VISIBLE != 0) == visible {
            return;
        }
        ShowWindow(control, if visible { SW_SHOWNA } else { SW_HIDE });
        if !visible {
            let mut bounds = RECT::default();
            GetWindowRect(control, &mut bounds);
            MapWindowPoints(null_mut(), parent, (&mut bounds as *mut RECT).cast(), 2);
            InvalidateRect(parent, &bounds, 0);
        }
    }
}

pub(super) fn cancel(state: &State, hwnd: HWND) -> bool {
    let active = PANEL.with_borrow_mut(|p| p.recorder.active.take());
    if let Some(active) = active {
        set_listening(state, false);
        feedback(hwnd, active.id, "", false);
        true
    } else {
        false
    }
}

pub(super) fn blur(hwnd: HWND, id: usize) {
    if PANEL.with_borrow(|p| is_listening(&p.recorder, id))
        && let Some(state) = session()
    {
        cancel(&state, hwnd);
    }
}

pub(super) fn deactivate(state: &State, hwnd: HWND) {
    if PANEL.with_borrow(|p| {
        p.recorder
            .active
            .as_ref()
            .is_some_and(|active| matches!(active.phase, Phase::Listening { .. }))
    }) {
        cancel(state, hwnd);
    }
}

fn is_listening(recorder: &Recorder, id: usize) -> bool {
    recorder
        .active
        .as_ref()
        .is_some_and(|active| active.id == id && matches!(active.phase, Phase::Listening { .. }))
}

fn start(state: &State, hwnd: HWND, id: usize) {
    let same = PANEL.with_borrow(|p| p.recorder.active.as_ref().is_some_and(|a| a.id == id));
    if same {
        if !PANEL.with_borrow(|p| is_listening(&p.recorder, id)) {
            cancel(state, hwnd);
        }
        return;
    }
    cancel(state, hwnd);
    focus_control(hwnd, id as i32);
    let animate = fluent::animations_enabled();
    PANEL.with_borrow_mut(|p| {
        p.recorder.active = Some(Active {
            id,
            phase: Phase::Listening {
                held: 0,
                error: None,
            },
            started: Instant::now(),
            animate,
        });
    });
    set_listening(state, true);
    feedback(
        hwnd,
        id,
        tr("按下快捷键 · Esc 取消 · Backspace 清除"),
        false,
    );
    if animate {
        unsafe { SetTimer(hwnd, TIMER, 15, None) };
    }
}

pub(super) fn tick(hwnd: HWND) {
    let (active, confirmed) = PANEL.with_borrow_mut(|p| {
        let recorder = &mut p.recorder;
        let active = recorder.active.as_ref().filter(|active| active.animate).and_then(|active| {
            let frame = Duration::from_millis(17);
            let moving = active.started.elapsed() <= MOTION + frame
                || matches!(&active.phase, Phase::Listening { error: Some(at), .. } if at.elapsed() <= SHAKE + frame);
            let waiting = matches!(active.phase, Phase::Listening { held: 0, .. });
            (moving || waiting).then_some((active.id, !moving))
        });
        let confirmed = recorder.confirmed.map(|(id, at, animate)| {
            let end = if animate { CONFIRMED + MOTION } else { CONFIRMED };
            if at.elapsed() >= end {
                recorder.confirmed = None;
            }
            id
        });
        (active, confirmed)
    });
    unsafe {
        if let Some((id, dot_only)) = active {
            let child = GetDlgItem(hwnd, id as i32);
            if dot_only {
                invalidate_dot(child);
            } else {
                InvalidateRect(child, null(), 0);
            }
        }
        if let Some(id) = confirmed {
            InvalidateRect(GetDlgItem(hwnd, id as i32), null(), 0);
        }
        if active.is_none() && confirmed.is_none() {
            KillTimer(hwnd, TIMER);
        }
    }
}

fn invalidate_dot(hwnd: HWND) {
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let mut dot = RECT::default();
        GetClientRect(hwnd, &mut dot);
        let length = dip(DOT_SIZE as i32, dpi);
        dot.left = dip(DOT_X as i32, dpi) - 1;
        dot.right = dot.left + length + 3;
        dot.top = (dot.bottom - length) / 2 - 1;
        dot.bottom = dot.top + length + 3;
        InvalidateRect(hwnd, &dot, 0);
    }
}

fn validation(key: Shortcut) -> Result<(), &'static str> {
    let reserved = matches!(
        (key.modifiers, key.key),
        (ALT, 0x73) | (WIN, 0x4c | 0x44) | (3, 0x2e) | (_, 0x7b) | (_, 0x5d)
    );
    if reserved {
        Err("系统保留，无法使用")
    } else if key.modifiers & (CONTROL | ALT | WIN) == 0 && !(0x70..=0x7a).contains(&key.key) {
        Err("需要包含 Ctrl / Alt / Win（F1–F11 除外）")
    } else if !key.is_combination() {
        Err("系统保留，无法使用")
    } else {
        Ok(())
    }
}

fn error(hwnd: HWND, id: usize, text: String) {
    PANEL.with_borrow_mut(|p| {
        if let Some(Active {
            phase: Phase::Listening { error, .. },
            ..
        }) = p.recorder.active.as_mut()
        {
            *error = Some(Instant::now());
        }
    });
    feedback(hwnd, id, &text, false);
    unsafe { SetTimer(hwnd, TIMER, 15, None) };
}

fn try_key(state: &State, hwnd: HWND, id: usize, key: Shortcut) {
    if let Err(message) = validation(key) {
        error(hwnd, id, tr(message).to_owned());
        return;
    }
    let duplicate = bindings(&state.app.borrow().settings().preferences)
        .into_iter()
        .find(|(other, value)| *other != id && *value == Some(key));
    if let Some((previous, _)) = duplicate {
        PANEL.with_borrow_mut(|p| {
            if let Some(active) = p.recorder.active.as_mut() {
                active.phase = Phase::Conflict { key, previous };
                active.started = Instant::now();
            }
        });
        set_listening(state, false);
        feedback(
            hwnd,
            id,
            &tr("与「{}」冲突").replace("{}", tr(name(previous))),
            true,
        );
        unsafe { SetTimer(hwnd, TIMER, 15, None) };
    } else if let Err(message) = save(state, hwnd, id, Some(key), None) {
        error(hwnd, id, message);
    }
}

fn listening_id(hwnd: HWND) -> Option<usize> {
    let id = unsafe { GetDlgCtrlID(GetFocus()) } as usize;
    (unsafe { GetForegroundWindow() } == hwnd
        && PANEL.with_borrow(|p| is_listening(&p.recorder, id)))
    .then_some(id)
}

pub(super) fn registered(state: &State, hwnd: HWND, key: Shortcut) -> bool {
    if let Some(id) = listening_id(hwnd) {
        if key != Shortcut::default() {
            try_key(state, hwnd, id, key);
        }
        true
    } else {
        false
    }
}

fn key_modifiers() -> u32 {
    [
        (VK_CONTROL, CONTROL),
        (VK_MENU, ALT),
        (VK_SHIFT, SHIFT),
        (VK_LWIN, WIN),
        (VK_RWIN, WIN),
    ]
    .into_iter()
    .filter(|(vk, _)| unsafe { GetKeyState(i32::from(*vk)) } < 0)
    .fold(0, |mask, (_, flag)| mask | flag)
}

pub(super) fn key(hwnd: HWND, msg: &MSG) -> bool {
    if !matches!(
        msg.message,
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP
    ) {
        return false;
    }
    let Some(id) = listening_id(hwnd) else {
        return false;
    };
    let Some(state) = session() else { return false };
    let key = msg.wParam as u16;
    let held = key_modifiers();
    // Tab and Shift+Tab keep the dialog's focus navigation.
    if key == VK_TAB && held & !SHIFT == 0 {
        return false;
    }
    if matches!(msg.message, WM_KEYUP | WM_SYSKEYUP)
        || matches!(
            key,
            VK_CONTROL
                | VK_MENU
                | VK_SHIFT
                | VK_LWIN
                | VK_RWIN
                | VK_LCONTROL
                | VK_RCONTROL
                | VK_LSHIFT
                | VK_RSHIFT
                | VK_LMENU
                | VK_RMENU
        )
    {
        let animate = PANEL.with_borrow_mut(|p| {
            let Some(Active {
                phase: Phase::Listening { held: previous, .. },
                animate,
                ..
            }) = p.recorder.active.as_mut()
            else {
                return false;
            };
            *previous = held;
            *animate
        });
        unsafe {
            InvalidateRect(GetDlgItem(hwnd, id as i32), null(), 0);
            if animate && held == 0 {
                SetTimer(hwnd, TIMER, 15, None);
            }
        }
        return true;
    }
    if msg.lParam & (1 << 30) != 0 {
        return true;
    }
    match (held, key) {
        (0, VK_ESCAPE) => {
            cancel(&state, hwnd);
        }
        (0, VK_BACK) => {
            if let Err(message) = save(&state, hwnd, id, None, None) {
                error(hwnd, id, message);
            }
        }
        _ => try_key(
            &state,
            hwnd,
            id,
            Shortcut {
                key: u32::from(key),
                modifiers: held,
            },
        ),
    }
    true
}

pub(super) fn selection_options(
    state: &State,
    hwnd: HWND,
    settings: &Settings,
) -> Option<SelectionBinding> {
    let mut selection = settings.preferences.selection.clone()?;
    let choice = unsafe { SendMessageW(GetDlgItem(hwnd, 136), CB_GETCURSEL, 0, 0) };
    if let Some(engine) = usize::try_from(choice)
        .ok()
        .and_then(|i| state.app.borrow().engines().get(i).map(|e| e.id.clone()))
    {
        selection.engine = engine;
    }
    selection.preview = unsafe { SendMessageW(GetDlgItem(hwnd, 135), CB_GETCURSEL, 0, 0) == 1 };
    Some(selection)
}

fn assign(prefs: &mut Preferences, id: usize, value: Option<Shortcut>) {
    match id {
        137 => prefs.main_shortcut = value,
        138 => {
            prefs.selection = value.map(|shortcut| SelectionBinding {
                shortcut,
                engine: prefs
                    .selection
                    .as_ref()
                    .map_or_else(|| prefs.default_engine.clone(), |b| b.engine.clone()),
                preview: prefs.selection.as_ref().is_none_or(|b| b.preview),
            });
        }
        139 => prefs.clipboard_shortcut = value,
        248 => prefs.image_search_shortcut = value,
        156 => prefs.ocr_shortcut = value,
        _ => unreachable!(),
    }
}

fn save(
    state: &State,
    hwnd: HWND,
    id: usize,
    value: Option<Shortcut>,
    previous: Option<usize>,
) -> Result<(), String> {
    let mut settings = state.app.borrow().settings().clone();
    if let Some(previous) = previous {
        assign(&mut settings.preferences, previous, None);
    }
    assign(&mut settings.preferences, id, value);
    if id == 138
        && let Some(selection) = selection_options(state, hwnd, &settings)
    {
        settings.preferences.selection = Some(selection);
    }
    apply_hotkeys(state, settings)?;
    cancel(state, hwnd);
    let values = bindings(&state.app.borrow().settings().preferences);
    for (target, key) in values {
        unsafe {
            let record = GetDlgItem(hwnd, target as i32);
            if record.is_null() {
                continue;
            }
            set_field(hwnd, target as i32, &shown(key));
            show_control(
                hwnd,
                GetDlgItem(hwnd, reset_id(target) as i32),
                key != default_shortcut(target),
            );
            InvalidateRect(record, null(), 0);
        }
    }
    focus_control(hwnd, id as i32);
    PANEL.with_borrow_mut(|p| {
        p.recorder.confirmed = Some((id, Instant::now(), fluent::animations_enabled()))
    });
    unsafe { SetTimer(hwnd, TIMER, 15, None) };
    Ok(())
}

pub(super) fn command(state: &State, hwnd: HWND, id: usize) -> Result<(), String> {
    match id {
        id if is_trigger(id) => start(state, hwnd, id),
        145..=147 | 157 | 256 => {
            let target = if id == 157 { 156 } else { id - 8 };
            save(state, hwnd, target, default_shortcut(target), None)?;
        }
        537..=539 | 556 | 648 => {
            let target = id - 400;
            let pending = PANEL.with_borrow(|p| {
                let active = p.recorder.active.as_ref().filter(|a| a.id == target)?;
                match active.phase {
                    Phase::Conflict { key, previous } => Some((key, previous)),
                    _ => None,
                }
            });
            if let Some((key, previous)) = pending {
                save(state, hwnd, target, Some(key), Some(previous))?;
            }
        }
        637..=639 | 656 | 748 => {
            let active = PANEL.with_borrow(|p| p.recorder.active.as_ref().map(|a| a.id));
            if cancel(state, hwnd)
                && let Some(target) = active
            {
                focus_control(hwnd, target as i32);
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn status_color(id: i32) -> Option<u32> {
    PANEL.with_borrow(|p| {
        let active = p
            .recorder
            .active
            .as_ref()
            .filter(|a| status_id(a.id) as i32 == id)?;
        match &active.phase {
            Phase::Conflict { .. } => Some(0x75471b),
            Phase::Listening { error: Some(_), .. } => Some(DANGER),
            _ => None,
        }
    })
}

pub(super) fn paint_window(hwnd: HWND, dc: HDC, bounds: RECT) {
    unsafe {
        let parent = GetParent(hwnd);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let unit = 96.0 / dpi as f32;
        let colors = fluent::palette();
        let focused = GetFocus() == hwnd
            && SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS as isize == 0;
        fluent::with_painter(dc, bounds, dpi, |p| {
            p.clear(fluent::control_surface(parent, hwnd, colors));
            paint(
                p,
                colors,
                GetDlgCtrlID(hwnd) as usize,
                &text_of(hwnd),
                bounds.right as f32 * unit,
                bounds.bottom as f32 * unit,
                focused,
                fluent::hover_amount(hwnd),
            );
        });
    }
}

fn paint(
    p: &mut Painter,
    colors: fluent::Palette,
    id: usize,
    label: &str,
    width: f32,
    height: f32,
    focused: bool,
    hover: f32,
) {
    const OUTLINE: u32 = 0xb3b3b3;
    PANEL.with_borrow(|panel| {
        let recorder = &panel.recorder;
        let active = recorder.active.as_ref().filter(|a| a.id == id);
        let success = recorder.confirmed.filter(|(target, _, _)| *target == id);
        let mut x = 4.0;
        let mut border = blend(OUTLINE, colors.detail, hover);
        let mut glow = None;
        let mut live = None;
        let mut pending = None;
        let mut fill = blend(colors.button, colors.hover, hover);
        let mut pulse = 1.0;
        if let Some(active) = active {
            let seconds = active.started.elapsed().as_secs_f32();
            let progress = if active.animate {
                ease(seconds / MOTION.as_secs_f32())
            } else {
                1.0
            };
            match &active.phase {
                Phase::Listening { held, error } => {
                    live = Some(*held);
                    border = blend(border, colors.accent, progress);
                    glow = Some((colors.accent, 0.16 * progress));
                    if active.animate {
                        pulse = 1.0 - 0.7 * (0.5 - 0.5 * (seconds * std::f32::consts::TAU).cos());
                    }
                    if let Some(at) = error {
                        let t = at.elapsed().as_secs_f32() / SHAKE.as_secs_f32();
                        if !active.animate || t < 1.0 {
                            border = DANGER;
                        }
                        if active.animate && t < 1.0 {
                            x += shake(t);
                        }
                    }
                }
                Phase::Conflict { key, .. } => {
                    pending = Some(shown(Some(*key)));
                    border = blend(colors.accent, AMBER, progress);
                    glow = Some((AMBER, 0.18 * progress));
                }
            }
        } else if let Some((_, at, animate)) = success {
            let fade = if animate {
                ease(at.elapsed().saturating_sub(CONFIRMED).as_secs_f32() / MOTION.as_secs_f32())
            } else {
                0.0
            };
            border = blend(SUCCESS, border, fade);
            fill = blend(0xe0f2e7, fill, fade);
        }
        let high_contrast = crate::native::backdrop::high_contrast();
        if high_contrast {
            fill = blend(colors.button, colors.hover, hover);
            border = if focused || active.is_some() {
                colors.accent
            } else {
                colors.border
            };
        }
        if let Some((color, alpha)) = glow.filter(|_| !high_contrast) {
            p.rounded_alpha(
                rect(x - 3.0, 1.0, width - 2.0, height - 2.0),
                11.0,
                color,
                alpha,
                false,
            );
        } else if focused && hover < 1.0 {
            p.rounded_alpha(
                rect(x - 2.0, 2.0, width - 4.0, height - 4.0),
                10.0,
                colors.accent,
                1.0 - hover,
                true,
            );
        }
        let inset = RECORD_INSET as f32;
        let face = rect(x, inset, width - 2.0 * inset, height - 2.0 * inset);
        p.rounded(face, 8.0, fill, false);
        p.rounded(face, 8.0, border, true);
        let left = x + DOT_X - 4.0;
        if live == Some(0) {
            p.rounded_alpha(
                rect(left, (height - DOT_SIZE) / 2.0, DOT_SIZE, DOT_SIZE),
                DOT_SIZE * 0.5,
                if high_contrast { colors.text } else { DANGER },
                pulse,
                false,
            );
            p.text(
                tr("按下快捷键…"),
                rect(left + 16.0, 0.0, width - 35.0, height),
                colors.detail,
                Label::Body,
            );
        } else if label == tr("未设置") && live.is_none() && pending.is_none() {
            p.text(
                label,
                rect(left, 0.0, width - 24.0, height),
                colors.detail,
                Label::Body,
            );
        } else {
            let value = pending.as_deref().unwrap_or(label);
            let caps = if let Some(held) = live {
                modifier_names(held)
            } else if value == "双击左 Alt" {
                vec![tr("左 Alt").to_owned(), "× 2".into()]
            } else {
                value.split('+').map(str::to_owned).collect()
            };
            paint_caps(p, &caps, left, height, width - 24.0, live.is_some(), colors);
        }
    });
}

fn paint_caps(
    p: &mut Painter,
    caps: &[String],
    mut x: f32,
    height: f32,
    width: f32,
    pressed: bool,
    colors: fluent::Palette,
) {
    let natural = |cap: &str| (cap.chars().count() as f32 * 6.0 + 9.0).max(20.0);
    let gap = 14.0;
    let available = width - gap * caps.len().saturating_sub(1) as f32;
    let scale = (available / caps.iter().map(|cap| natural(cap)).sum::<f32>()).min(1.0);
    let y = (height - 20.0) / 2.0 + if pressed { 1.0 } else { 0.0 };
    let outline = if pressed {
        colors.accent
    } else if crate::native::backdrop::high_contrast() {
        colors.border
    } else {
        0xa3a3a3
    };
    let fill = if pressed {
        colors.selection
    } else {
        colors.button
    };
    let bottom = if pressed { 1.0 } else { 2.0 };
    for (index, cap) in caps.iter().enumerate() {
        if index != 0 {
            p.text(
                "+",
                rect(x, 0.0, gap, height),
                colors.detail,
                Label::ControlDetail,
            );
            x += gap;
        }
        let size = natural(cap) * scale;
        let face = rect(x + 1.0, y + 1.0, size - 2.0, 19.0 - bottom);
        p.rounded(rect(x, y, size, 20.0), 4.0, outline, false);
        p.rounded(face, 3.0, fill, false);
        p.text(
            cap,
            face,
            if pressed { colors.accent } else { colors.text },
            Label::ControlDetail,
        );
        x += size;
    }
}

fn ease(time: f32) -> f32 {
    fluent::easing(time, 0.2, 0.38, 0.9)
}

fn shake(t: f32) -> f32 {
    if t < 0.25 {
        -12.0 * t
    } else if t < 0.75 {
        -3.0 + 12.0 * (t - 0.25)
    } else {
        12.0 * (1.0 - t)
    }
}

pub(super) fn paint_link(
    p: &mut Painter,
    area: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F,
    label: &str,
    colors: fluent::Palette,
    focused: bool,
    hover: f32,
) {
    fluent::paint_button_face(
        p,
        rect(0.5, 0.5, area.right - 1.0, area.bottom - 1.0),
        colors,
        fluent::ButtonStyle::Subtle,
        fluent::ButtonState {
            hover,
            focused,
            ..fluent::ButtonState::default()
        },
    );
    p.text(label, area, colors.accent, Label::ControlDetail);
    let width = if label.is_ascii() {
        label.len() as f32 * 5.5
    } else {
        23.0
    };
    p.rounded(
        rect(
            (area.right - width) / 2.0,
            area.bottom / 2.0 + 7.0,
            width,
            1.0,
        ),
        0.0,
        colors.accent,
        false,
    );
}

pub(super) fn paint_reset(
    p: &mut Painter,
    width: f32,
    height: f32,
    dpi: u32,
    colors: fluent::Palette,
    focused: bool,
    hover: f32,
) {
    let area = rect(0.5, 0.5, width - 1.0, height - 1.0);
    fluent::paint_button_face(
        p,
        area,
        colors,
        fluent::ButtonStyle::Subtle,
        fluent::ButtonState {
            hover,
            focused,
            ..fluent::ButtonState::default()
        },
    );
    fluent::icon(
        p,
        ("shortcut-reset", RESET_ICON),
        (width - 16.0) / 2.0,
        (height - 16.0) / 2.0,
        16.0,
        dpi,
        colors.detail,
    );
}
