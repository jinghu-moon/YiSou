mod components;
mod dropdown;
mod engines;
mod fluent;
mod menu;
#[cfg(feature = "ocr")]
mod ocr;
mod pages;

use super::{State, dip, instance, message, register, session, text_of, wide};
use fluent::{Layout, RIGHT, STRETCH};
use std::{
    cell::RefCell,
    ffi::OsString,
    io,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::{
    Win32::{
        Foundation::*,
        Globalization::GetUserDefaultUILanguage,
        Graphics::Gdi::*,
        UI::{
            Controls::{
                DRAWITEMSTRUCT,
                Dialogs::{
                    CC_FULLOPEN, CC_RGBINIT, CF_INITTOLOGFONTSTRUCT, CF_SCREENFONTS, CHOOSECOLORW,
                    CHOOSEFONTW, ChooseColorW, ChooseFontW, GetOpenFileNameW, GetSaveFileNameW,
                    OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
                    OPENFILENAMEW,
                },
                EM_SETCUEBANNER,
            },
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    w,
};
use yisou::{
    application::{Record, UpdateError},
    engine::{Engine, EngineError, Icon},
    icons::{import_bitmap, import_svg},
    platform,
    settings::{Font, Language, SelectionBinding, Settings, SettingsError},
    shortcut::Shortcut,
};

const CLASS: *const u16 = w!("YiSou.Rust.Settings");
const NAV: usize = 1;
const HISTORY_FILTER: usize = 1;
const BACK: u32 = WM_APP + 40;
const HISTORY_ACTION: u32 = WM_APP + 41;
const ENGINE_MENU: u32 = WM_APP + 42;
const ENGINE_FILTER: usize = 2;
const SS_CENTERIMAGE: u32 = 0x0200;
const SS_ENDELLIPSIS: u32 = 0x4000;
const OCR_PAGE: usize = 7;
const PAGES: &[&str] = &[
    "常规",
    "外观",
    "搜索引擎",
    "快捷键",
    "浏览器",
    "搜索历史",
    "配置文件",
    #[cfg(feature = "ocr")]
    "OCR 搜索",
];

fn nav_pages() -> impl Iterator<Item = usize> {
    (0..6)
        .chain((PAGES.len() > OCR_PAGE).then_some(OCR_PAGE))
        .chain([6])
}

fn nav_index(page: usize) -> usize {
    nav_pages().position(|id| id == page).unwrap_or(0)
}

include!(concat!(env!("OUT_DIR"), "/settings_strings.rs"));

fn english_selected(language: Language) -> bool {
    match language {
        Language::English => true,
        Language::Chinese => false,
        Language::System => (unsafe { GetUserDefaultUILanguage() } & 0x03ff) != 0x04,
    }
}

fn tr(text: &str) -> &str {
    if PANEL.with_borrow(|p| p.english) {
        match text {
            "应用后生效" => "Applies immediately",
            "应用所选" => "Use selected",
            "搜索内容" => "Search terms",
            "上一页" => "Previous page",
            "例如 mstsc.exe" => "For example, mstsc.exe",
            "OCR 搜索" => "OCR search",
            "本地模型目录" => "Local model folder",
            "ModelScope 下载 · PP-OCRv6 tiny" => "ModelScope downloads · PP-OCRv6 tiny",
            "检测模型" => "Detector",
            "识别模型" => "Recognizer",
            "字典" => "Dictionary",
            "录制 OCR 快捷键" => "Record OCR shortcut",
            "请选择 OCR 模型目录" => "Choose an OCR model folder",
            "启动与后台" => "Startup and background",
            "当前字体" => "Current font",
            "显示名称" => "Show names",
            "仅显示图标" => "Icons only",
            "筛选名称或 #别名" => "Filter by name or #alias",
            "置顶快捷位" => "Pinned shortcuts",
            "添加引擎" => "Add engine",
            "引擎" => "Engine",
            "别名" => "Alias",
            "编辑别名" => "Edit alias",
            "恢复默认别名" => "Restore default alias",
            "别名须以字母开头，最多 16 位，可用 A–Z、数字、- 和 _。" => {
                "Start with A–Z. Use up to 16 letters, digits, - or _."
            }
            "该别名已被其他引擎使用。" => "Another engine already uses this alias.",
            "状态" => "Status",
            "更多操作" => "More actions",
            "没有符合条件的引擎" => "No matching engines",
            "前往设置" => "Open settings",
            "下一页" => "Next",
            "清除筛选" => "Clear filters",
            "当前配置" => "Current configuration",
            "完整配置" => "Full configuration",
            "搜索引擎包" => "Search engine pack",
            "截图文字搜索" => "Search text from screenshots",
            "启用 OCR 搜索" => "Enable OCR search",
            "本地模型" => "Local model",
            "待配置" => "Setup required",
            "正在检查模型…" => "Checking model…",
            "模型可用" => "Model ready",
            "将检测模型、识别模型和字典放在同一个目录。" => {
                "Keep the detector, recognizer and dictionary in the same folder."
            }
            "启用前需要配置并验证本地 OCR 模型。" => {
                "Configure and verify the local model before enabling."
            }
            "导入会替换当前配置。下次启动时会使用新选定的配置位置。" => {
                "Import replaces current settings. Location changes apply at next launch."
            }
            _ => english(text),
        }
    } else {
        text
    }
}

#[derive(Default)]
struct Panel {
    page: usize,
    children: Vec<HWND>,
    selected: Option<usize>,
    edit_engine: bool,
    engine_filter: String,
    engine_rows: Vec<usize>,
    engine_top: usize,
    engine_draft: Option<[String; 5]>,
    next_config: Option<PathBuf>,
    page_scroll: [i32; 8],
    list_scroll: [usize; 8],
    history_page: usize,
    history_keyword: String,
    history_source: String,
    history: Vec<Record>,
    #[cfg(feature = "ocr")]
    ocr_check: ocr::Check,
    browsers: Vec<platform::InstalledBrowser>,
    preview_font: HFONT,
    view: fluent::View,
    browser_panel: BrowserPanel,
    recording: Option<(usize, Option<Shortcut>)>,
    english: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum BrowserPanel {
    #[default]
    Closed,
    Path,
}

impl Drop for Panel {
    fn drop(&mut self) {
        if !self.preview_font.is_null() {
            unsafe { DeleteObject(self.preview_font) };
        }
    }
}

thread_local! { static PANEL: RefCell<Panel> = RefCell::new(Panel::default()); }

pub(super) fn register_window() -> io::Result<()> {
    register(CLASS, Some(proc))?;
    menu::register_window()
}

pub(super) fn show(state: &State, page: usize) {
    state.close_popup();
    let language = state.app.borrow().settings().preferences.language;
    PANEL.with_borrow_mut(|panel| panel.english = english_selected(language));
    let mut hwnd = state.settings_window.get();
    if hwnd.is_null() {
        hwnd = unsafe {
            let owner = state.window.get();
            let dpi = GetDpiForWindow(owner).max(96);
            let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VSCROLL;
            let mut frame = RECT {
                right: dip(900, dpi),
                bottom: dip(580, dpi),
                ..Default::default()
            };
            let extended = WS_EX_CONTROLPARENT | WS_EX_COMPOSITED;
            AdjustWindowRectExForDpi(&mut frame, style, 0, extended, dpi);
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(
                MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            );
            let work = monitor.rcWork;
            let width = (frame.right - frame.left).min(work.right - work.left);
            let height = (frame.bottom - frame.top).min(work.bottom - work.top);
            CreateWindowExW(
                extended,
                CLASS,
                wide(tr("易搜设置")).as_ptr(),
                style,
                work.left + (work.right - work.left - width) / 2,
                work.top + (work.bottom - work.top - height) / 2,
                width,
                height,
                null_mut(),
                null_mut(),
                instance(),
                null(),
            )
        };
        if hwnd.is_null() {
            return;
        }
        state.settings_window.set(hwnd);
        state.dwm(hwnd, 2);
        let nav = control(
            hwnd,
            w!("LISTBOX"),
            "",
            WS_TABSTOP
                | (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT) as u32,
            NAV,
            (16, 34, 170, 294),
        );
        for page in nav_pages() {
            add_item(nav, tr(PAGES[page]));
        }
        fluent::track_control(nav);
        fluent::apply_palette(hwnd);
    }
    select_page(state, hwnd, page);
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_SHOWNORMAL);
        }
        SetForegroundWindow(hwnd);
    }
}

fn select_page(state: &State, hwnd: HWND, page: usize) {
    if page >= PAGES.len() {
        return;
    }
    let previous = PANEL.with_borrow(|panel| panel.page);
    if previous == page && !PANEL.with_borrow(|panel| panel.children.is_empty()) {
        return;
    }
    if !confirm_engine_exit(hwnd) {
        unsafe {
            SendMessageW(
                GetDlgItem(hwnd, NAV as i32),
                LB_SETCURSEL,
                nav_index(previous),
                0,
            )
        };
        return;
    }
    let list = unsafe { GetDlgItem(hwnd, 10) };
    let top = if list.is_null() {
        0
    } else {
        unsafe { SendMessageW(list, LB_GETTOPINDEX, 0, 0) }.max(0) as usize
    };
    let left_history = PANEL.with_borrow_mut(|panel| {
        let left_history = panel.page == 5 && page != 5;
        if panel.page != page {
            panel.page_scroll[panel.page] = panel.view.scroll;
            panel.list_scroll[panel.page] = top;
            if panel.page == 2 && !panel.edit_engine {
                panel.engine_top = top;
            }
            panel.selected = None;
            panel.edit_engine = false;
            panel.engine_draft = None;
            panel.browser_panel = BrowserPanel::Closed;
            panel.recording = None;
            panel.view.scroll = panel.page_scroll[page];
            panel.view.wheel = 0;
        }
        if left_history {
            panel.history = Vec::new();
        }
        panel.page = page;
        left_history
    });
    if left_history {
        unsafe { KillTimer(hwnd, HISTORY_FILTER) };
        state.app.borrow_mut().history().release_cache();
        state.diagnostics.sample("history_cache_released");
    }
    unsafe {
        SendMessageW(
            GetDlgItem(hwnd, NAV as i32),
            LB_SETCURSEL,
            nav_index(page),
            0,
        );
    }
    display(state, hwnd);
    if matches!(page, 4 | 5) {
        let list = unsafe { GetDlgItem(hwnd, 10) };
        if !list.is_null() {
            unsafe {
                SendMessageW(
                    list,
                    LB_SETTOPINDEX,
                    PANEL.with_borrow(|p| p.list_scroll[page]),
                    0,
                )
            };
        }
    }
    if page == 5 {
        state.diagnostics.sample("history_page_loaded");
    }
}

fn control(
    parent: HWND,
    class: *const u16,
    label: &str,
    style: u32,
    id: usize,
    area: (i32, i32, i32, i32),
) -> HWND {
    let dpi = unsafe { GetDpiForWindow(parent) }.max(96);
    let (x, y, width, height) = area;
    unsafe {
        CreateWindowExW(
            0,
            class,
            wide(label).as_ptr(),
            WS_VISIBLE | WS_CHILD | style,
            dip(x, dpi),
            dip(y, dpi),
            dip(width, dpi),
            dip(height, dpi),
            parent,
            id as HMENU,
            instance(),
            null(),
        )
    }
}

fn child(
    parent: HWND,
    class: *const u16,
    label: &str,
    style: u32,
    id: usize,
    area: Layout,
) -> HWND {
    let handle = control(parent, class, label, style, id, (0, 0, 0, 0));
    if handle.is_null() {
        return handle;
    }
    PANEL.with_borrow_mut(|p| {
        p.children.push(handle);
        p.view.controls.push((handle, area));
    });
    fluent::track_control(handle);
    handle
}

fn caption(hwnd: HWND, text: &str, area: Layout, font: fluent::FontRole) -> HWND {
    child(
        hwnd,
        w!("STATIC"),
        tr(text),
        SS_CENTERIMAGE | SS_ENDELLIPSIS,
        0,
        area.font(font),
    )
}

fn action(hwnd: HWND, id: usize, text: &str, area: Layout) -> HWND {
    child(
        hwnd,
        w!("BUTTON"),
        tr(text),
        WS_TABSTOP | BS_OWNERDRAW as u32,
        id,
        area,
    )
}

fn edit_at(hwnd: HWND, id: usize, text: &str, bounds: Layout) -> HWND {
    child(
        hwnd,
        w!("EDIT"),
        text,
        WS_TABSTOP | ES_AUTOHSCROLL as u32,
        id,
        bounds,
    )
}

fn list_at(hwnd: HWND, items: impl IntoIterator<Item = String>, bounds: Layout) -> HWND {
    let list = child(
        hwnd,
        w!("LISTBOX"),
        "",
        WS_TABSTOP
            | WS_VSCROLL
            | (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT) as u32,
        10,
        bounds,
    );
    for item in items {
        add_item(list, &item);
    }
    unsafe {
        if let Some(selected) = PANEL.with_borrow(|panel| panel.selected) {
            SendMessageW(list, LB_SETCURSEL, selected, 0);
        }
    }
    list
}

fn engine_combo(state: &State, hwnd: HWND, area: Layout) {
    let app = state.app.borrow();
    let prefs = &app.settings().preferences;
    let selected = prefs
        .selection
        .as_ref()
        .map_or(prefs.default_engine.as_str(), |b| b.engine.as_str());
    let engines = app.engines();
    let index = engines
        .iter()
        .position(|engine| engine.id == selected)
        .unwrap_or(0);
    dropdown::create(
        hwnd,
        136,
        engines.iter().map(|engine| {
            if engine.alias.is_empty() {
                engine.name.clone()
            } else {
                format!("{}  #{}", engine.name, engine.alias)
            }
        }),
        index,
        area,
    );
}

fn add_item(list: HWND, text: &str) {
    unsafe {
        SendMessageW(list, LB_ADDSTRING, 0, wide(text).as_ptr() as isize);
    }
}

fn field(hwnd: HWND, id: usize) -> String {
    text_of(unsafe { GetDlgItem(hwnd, id as i32) })
}

fn set_field(hwnd: HWND, id: i32, value: &str) {
    unsafe { SetWindowTextW(GetDlgItem(hwnd, id), wide(value).as_ptr()) };
}

fn set_toggle(hwnd: HWND, id: i32, title: &str, enabled: bool) {
    set_field(hwnd, id, &components::toggle_label(title, enabled));
    fluent::animate_toggle(hwnd, id);
}

fn set_label(hwnd: HWND, id: i32, value: &str) {
    set_field(hwnd, id, tr(value));
}

fn focus_control(hwnd: HWND, id: i32) {
    unsafe {
        let control = GetDlgItem(hwnd, id);
        if !control.is_null() && IsWindowVisible(control) != 0 && IsWindowEnabled(control) != 0 {
            SetFocus(control);
        }
    }
}

fn reload_navigation(hwnd: HWND, page: usize) {
    unsafe {
        SetWindowTextW(hwnd, wide(tr("易搜设置")).as_ptr());
        let nav = GetDlgItem(hwnd, NAV as i32);
        SendMessageW(nav, WM_SETREDRAW, 0, 0);
        SendMessageW(nav, LB_RESETCONTENT, 0, 0);
        for page in nav_pages() {
            add_item(nav, tr(PAGES[page]));
        }
        SendMessageW(nav, LB_SETCURSEL, nav_index(page), 0);
        SendMessageW(nav, WM_SETREDRAW, 1, 0);
        InvalidateRect(nav, null(), 0);
    }
}

fn draft_fields(hwnd: HWND) -> [String; 5] {
    [20, 21, 22, 23, 24].map(|id| field(hwnd, id))
}

fn edited_engine(state: &State) -> Option<Engine> {
    PANEL
        .with_borrow(|p| p.selected)
        .and_then(|index| state.app.borrow().engines().get(index).cloned())
}

fn engine_save_error(error: UpdateError) -> String {
    match error {
        UpdateError::Settings(SettingsError::Engine(EngineError::Alias)) => {
            "别名须以字母开头，最多 16 位，可用 A–Z、数字、- 和 _。".into()
        }
        UpdateError::Settings(SettingsError::Engine(EngineError::DuplicateAlias)) => {
            "该别名已被其他引擎使用。".into()
        }
        _ => error.to_string(),
    }
}

fn set_draft(hwnd: HWND) {
    let fields = draft_fields(hwnd);
    PANEL.with_borrow_mut(|p| p.engine_draft = Some(fields));
}

fn confirm(hwnd: HWND, prompt: &str) -> bool {
    unsafe {
        MessageBoxW(
            hwnd,
            wide(tr(prompt)).as_ptr(),
            wide(tr("易搜设置")).as_ptr(),
            MB_ICONQUESTION | MB_YESNO | MB_DEFBUTTON2,
        ) == IDYES
    }
}

fn confirm_engine_exit(hwnd: HWND) -> bool {
    let initial = PANEL.with_borrow(|p| p.engine_draft.clone());
    initial.is_none_or(|saved| saved == draft_fields(hwnd)) || confirm(hwnd, "放弃未保存的修改？")
}

fn choose_system_font(state: &State, hwnd: HWND) -> Option<String> {
    let mut face = LOGFONTW::default();
    for (slot, unit) in face
        .lfFaceName
        .iter_mut()
        .take(31)
        .zip(state.font_family().encode_utf16())
    {
        *slot = unit;
    }
    face.lfHeight = -dip(16, unsafe { GetDpiForWindow(hwnd) });
    let mut dialog = CHOOSEFONTW {
        lStructSize: size_of::<CHOOSEFONTW>() as u32,
        hwndOwner: hwnd,
        lpLogFont: &mut face,
        Flags: CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT,
        ..Default::default()
    };
    (unsafe { ChooseFontW(&mut dialog) } != 0).then(|| {
        String::from_utf16_lossy(
            &face.lfFaceName[..face
                .lfFaceName
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(face.lfFaceName.len())],
        )
    })
}

fn choose_color(hwnd: HWND) {
    let current = u32::from_str_radix(&field(hwnd, 23), 16).unwrap_or(super::paint::ACCENT);
    let mut palette = [0; 16];
    let mut dialog = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: super::color_ref(current),
        lpCustColors: palette.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    if unsafe { ChooseColorW(&mut dialog) } != 0 {
        set_field(
            hwnd,
            23,
            &format!("{:06X}", super::color_ref(dialog.rgbResult)),
        );
        set_field(hwnd, 24, "");
        PANEL.with_borrow_mut(|panel| panel.view.engine_icon = None);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

fn choose_file(hwnd: HWND, filter: *const u16, save: bool) -> Option<PathBuf> {
    let mut name = vec![0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: filter,
        lpstrFile: name.as_mut_ptr(),
        nMaxFile: name.len() as u32,
        Flags: OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    let selected = unsafe {
        if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        }
    };
    if selected == 0 {
        return None;
    }
    let length = name.iter().position(|unit| *unit == 0)?;
    Some(PathBuf::from(OsString::from_wide(&name[..length])))
}

fn display(state: &State, hwnd: HWND) {
    // Keep the previous frame on screen until the new page has been laid out.
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    if visible {
        unsafe { SendMessageW(hwnd, WM_SETREDRAW, 0, 0) };
    }
    let (children, preview_font) = PANEL.with_borrow_mut(|p| {
        p.view.controls.clear();
        p.view.surfaces.clear();
        p.view.engine_icon = None;
        p.view.extent = 0;
        p.view.hover = null_mut();
        (
            std::mem::take(&mut p.children),
            std::mem::take(&mut p.preview_font),
        )
    });
    for child in children {
        unsafe { DestroyWindow(child) };
    }
    if !preview_font.is_null() {
        unsafe { DeleteObject(preview_font) };
    }
    pages::build(state, hwnd);
    fluent::layout(hwnd);
    unsafe {
        if visible {
            SendMessageW(hwnd, WM_SETREDRAW, 1, 0);
            RedrawWindow(
                hwnd,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME | RDW_NOERASE,
            );
        }
    }
}

const fn place(x: i32, y: i32, width: i32, height: i32, sizing: i32) -> Layout {
    Layout::new(x, y, width, height, sizing)
}

fn cue(field: HWND, text: &str) {
    unsafe { SendMessageW(field, EM_SETCUEBANNER, 1, wide(tr(text)).as_ptr() as isize) };
}

fn model_field(hwnd: HWND, id: usize, text: &str) {
    let field = control(hwnd, w!("EDIT"), text, 0, id, (0, 0, 0, 0));
    unsafe { ShowWindow(field, SW_HIDE) };
    PANEL.with_borrow_mut(|panel| panel.children.push(field));
}

fn combo(hwnd: HWND, id: usize, values: &[&str], selected: usize, area: Layout) {
    dropdown::create(
        hwnd,
        id,
        values.iter().map(|value| tr(value)),
        selected,
        area,
    );
}

fn populate_history(state: &State, hwnd: HWND, reset: bool) {
    let keyword = field(hwnd, 25);
    let source = field(hwnd, 26);
    let mut page = if reset {
        0
    } else {
        PANEL.with_borrow(|p| p.history_page)
    };
    let (records, more) = {
        let mut app = state.app.borrow_mut();
        let history = app.history();
        let mut records = history.page(&keyword, &source, page).unwrap_or_default();
        if records.is_empty() && page > 0 {
            page -= 1;
            records = history.page(&keyword, &source, page).unwrap_or_default();
        }
        let more = records.len() == 24
            && history
                .page(&keyword, &source, page + 1)
                .is_ok_and(|rows| !rows.is_empty());
        (records, more)
    };
    let list = unsafe { GetDlgItem(hwnd, 10) };
    let empty = records.is_empty();
    let filtered = !keyword.trim().is_empty() || !source.trim().is_empty();
    let rows: Vec<_> = records.iter().map(history_row).collect();
    PANEL.with_borrow_mut(|panel| {
        panel.history_page = page;
        panel.history_keyword = keyword;
        panel.history_source = source;
        panel.history = records;
    });
    unsafe {
        let visible = IsWindowVisible(list) != 0;
        if visible {
            SendMessageW(list, WM_SETREDRAW, 0, 0);
        }
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        for row in rows {
            add_item(list, &row);
        }
        let empty_label = if filtered {
            "没有符合筛选条件的记录"
        } else {
            "暂无搜索记录"
        };
        set_label(hwnd, 1560, empty_label);
        ShowWindow(
            GetDlgItem(hwnd, 1560),
            if empty { SW_SHOWNA } else { SW_HIDE },
        );
        ShowWindow(list, if empty { SW_HIDE } else { SW_SHOWNA });
        EnableWindow(GetDlgItem(hwnd, 155), i32::from(filtered));
        EnableWindow(GetDlgItem(hwnd, 153), i32::from(page > 0));
        EnableWindow(GetDlgItem(hwnd, 154), i32::from(more));
        if visible {
            SendMessageW(list, WM_SETREDRAW, 1, 0);
            if empty {
                ShowWindow(list, SW_HIDE);
            }
        }
        InvalidateRect(list, null(), 0);
        InvalidateRect(hwnd, null(), 0);
    }
}

fn history_row(record: &Record) -> String {
    let time = fluent::history_time(record.timestamp_ms);
    let query = record.query.replace(['\r', '\n', '\t'], " ");
    format!(
        "{query}\t{}  ·  {}  ·  {time}",
        record.engine_name, record.source
    )
}

fn format_shortcut(s: Shortcut) -> String {
    if s.key == 0 {
        return "双击左 Alt".into();
    }
    let mut names = Vec::new();
    for (bit, name) in [(1, "Alt"), (2, "Ctrl"), (4, "Shift"), (8, "Win")] {
        if s.modifiers & bit != 0 {
            names.push(name.to_owned());
        }
    }
    names.push(
        if (0x41..=0x5a).contains(&s.key) || (0x30..=0x39).contains(&s.key) {
            ((s.key as u8) as char).to_string()
        } else if (0x70..=0x87).contains(&s.key) {
            format!("F{}", s.key - 0x6f)
        } else {
            format!("0x{:02X}", s.key)
        },
    );
    names.join("+")
}

fn parse_shortcut(text: &str) -> Result<Option<Shortcut>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text == "双击左 Alt" {
        return Ok(Some(Shortcut::default()));
    }
    let mut s = Shortcut::default();
    for token in text.split('+').map(|s| s.trim().to_ascii_lowercase()) {
        let bit = match token.as_str() {
            "alt" => 1,
            "ctrl" | "control" => 2,
            "shift" => 4,
            "win" => 8,
            _ => 0,
        };
        if bit != 0 {
            s.modifiers |= bit;
            continue;
        }
        if s.key != 0 {
            return Err("快捷键格式无效".into());
        }
        s.key = if token.len() == 1 && token.as_bytes()[0].is_ascii_alphanumeric() {
            u32::from(token.as_bytes()[0].to_ascii_uppercase())
        } else if let Some(hex) = token.strip_prefix("0x") {
            u32::from_str_radix(hex, 16).unwrap_or_default()
        } else if let Some(function) = token.strip_prefix('f') {
            function
                .parse::<u32>()
                .ok()
                .filter(|key| (1..=24).contains(key))
                .map_or(0, |key| key + 0x6f)
        } else {
            0
        };
    }
    if s.is_combination() {
        Ok(Some(s))
    } else {
        Err("快捷键格式无效".into())
    }
}

fn change(state: &State, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    state
        .app
        .borrow_mut()
        .change_settings(change)
        .map_err(|error| error.to_string())
}

fn apply_hotkeys(state: &State, settings: Settings) -> Result<(), String> {
    #[cfg(feature = "ocr")]
    {
        let app = state.app.borrow();
        let previous = app.settings().preferences.active_ocr_model_dir();
        let next = settings.preferences.active_ocr_model_dir();
        if previous != next && next.is_some() {
            state
                .validate_ocr_directory(&settings.preferences)
                .map_err(|error| tr(&error).to_owned())?;
        }
    }
    apply_settings_with_checked_model(state, settings)
}

fn apply_settings_with_checked_model(state: &State, settings: Settings) -> Result<(), String> {
    let old = State::bindings(state.app.borrow().settings());
    #[cfg(feature = "ocr")]
    let ocr_changed = state
        .app
        .borrow()
        .settings()
        .preferences
        .active_ocr_model_dir()
        != settings.preferences.active_ocr_model_dir();
    state
        .bind_hotkeys(State::bindings(&settings))
        .map_err(|error| error.to_string())?;
    if let Err(error) = change(state, |target| *target = settings) {
        state
            .bind_hotkeys(old)
            .map_err(|restore| format!("{error}\n{restore}"))?;
        return Err(error);
    }
    #[cfg(feature = "ocr")]
    if ocr_changed {
        state.end_ocr(true);
    }
    Ok(())
}

fn select_index(hwnd: HWND) -> Option<usize> {
    usize::try_from(unsafe { SendMessageW(GetDlgItem(hwnd, 10), LB_GETCURSEL, 0, 0) }).ok()
}

fn start_recording(hwnd: HWND, target: usize) {
    let already = PANEL.with_borrow(|p| p.recording.is_some());
    if already {
        return;
    }
    PANEL.with_borrow_mut(|p| p.recording = Some((target, None)));
    let (y, replace) = match target {
        137 => (110, 145),
        138 => (168, 146),
        139 => (286, 147),
        156 => (433, 157),
        _ => return,
    };
    unsafe {
        ShowWindow(GetDlgItem(hwnd, replace), SW_HIDE);
        SetFocus(GetDlgItem(hwnd, target as i32));
    }
    PANEL.with_borrow_mut(|p| {
        let handle = unsafe { GetDlgItem(hwnd, target as i32) };
        if let Some((_, area)) = p
            .view
            .controls
            .iter_mut()
            .find(|(child, _)| *child == handle)
        {
            *area = place(276, y, 83, 34, RIGHT);
        }
    });
    let save = action(hwnd, 148, "保存", place(447, y, 75, 34, RIGHT));
    action(hwnd, 149, "取消", place(367, y, 72, 34, RIGHT));
    let status = caption(
        hwnd,
        "",
        place(14, y + 34, 508, 18, STRETCH),
        fluent::FontRole::Detail,
    );
    unsafe {
        SetWindowLongPtrW(status, GWLP_ID, pages::RECORD_STATUS as isize);
    }
    set_label(hwnd, target as i32, "按下要使用的快捷键");
    unsafe { EnableWindow(save, 0) };
    fluent::layout(hwnd);
}

pub(super) fn capture_registered(state: &State, shortcut: Shortcut) -> bool {
    let hwnd = state.settings_window.get();
    if hwnd.is_null() || unsafe { GetForegroundWindow() } != hwnd {
        return false;
    }
    record_shortcut(hwnd, shortcut)
}

fn record_shortcut(hwnd: HWND, shortcut: Shortcut) -> bool {
    let recording = PANEL.with_borrow_mut(|p| {
        let (id, draft) = p.recording.as_mut()?;
        if shortcut.is_combination() || (*id == 137 && shortcut == Shortcut::default()) {
            *draft = Some(shortcut);
            Some(*id)
        } else {
            None
        }
    });
    if let Some(id) = recording {
        let duplicate = session()
            .and_then(|state| {
                let app = state.app.borrow();
                let prefs = &app.settings().preferences;
                [
                    (137, Some(prefs.main_shortcut)),
                    (138, prefs.selection.as_ref().map(|b| b.shortcut)),
                    (139, prefs.clipboard_shortcut),
                    (
                        156,
                        if cfg!(feature = "ocr") {
                            prefs.ocr_shortcut
                        } else {
                            None
                        },
                    ),
                ]
                .into_iter()
                .any(|(target, key)| target != id && key == Some(shortcut))
                .then_some(())
            })
            .is_some();
        set_field(hwnd, id as i32, &format_shortcut(shortcut));
        set_label(
            hwnd,
            pages::RECORD_STATUS as i32,
            if duplicate {
                "此组合键已用于其他搜索操作。"
            } else {
                ""
            },
        );
        unsafe { EnableWindow(GetDlgItem(hwnd, 148), i32::from(!duplicate)) };
        true
    } else {
        false
    }
}

pub(super) fn capture_key(hwnd: HWND, msg: &MSG) -> bool {
    if msg.hwnd != hwnd && unsafe { IsChild(hwnd, msg.hwnd) } == 0 {
        return false;
    }
    if msg.message != WM_KEYDOWN && msg.message != WM_SYSKEYDOWN {
        return false;
    }
    if !PANEL.with_borrow(|p| p.recording.is_some()) {
        let id = unsafe { GetDlgCtrlID(msg.hwnd) };
        let key = msg.wParam as u16;
        let page = PANEL.with_borrow(|p| p.page);
        let command = match (page, id, key) {
            (_, _, VK_ESCAPE)
                if unsafe { SendMessageW(msg.hwnd, CB_GETDROPPEDSTATE, 0, 0) } == 0 =>
            {
                unsafe { PostMessageW(hwnd, BACK, 0, 0) };
                return true;
            }
            (2, 10, VK_RETURN | VK_APPS) => engines::MENU_COMMAND,
            (2, 21, VK_RETURN) => 126,
            (3, 23, VK_RETURN) => 134,
            (4, 10, VK_RETURN | VK_SPACE) => 140,
            (4, 20, VK_RETURN) => 141,
            (5, 10, VK_RETURN) => 151,
            (5, 10, VK_DELETE) => 152,
            (5, 25 | 26, VK_RETURN) => {
                unsafe { PostMessageW(hwnd, WM_TIMER, HISTORY_FILTER, 0) };
                return true;
            }
            _ => return false,
        };
        unsafe { PostMessageW(hwnd, WM_COMMAND, command, 0) };
        return true;
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        return false;
    }
    let key = msg.wParam as u32;
    if key == u32::from(VK_ESCAPE) {
        unsafe { PostMessageW(hwnd, BACK, 0, 0) };
        return true;
    }
    if key == u32::from(VK_TAB) || key == u32::from(VK_RETURN) {
        return false;
    }
    let modifiers = [
        (VK_MENU, yisou::shortcut::ALT),
        (VK_CONTROL, yisou::shortcut::CONTROL),
        (VK_SHIFT, yisou::shortcut::SHIFT),
        (VK_LWIN, yisou::shortcut::WIN),
        (VK_RWIN, yisou::shortcut::WIN),
    ]
    .into_iter()
    .fold(0, |mask, (vk, flag)| {
        if unsafe { GetKeyState(i32::from(vk)) } < 0 {
            mask | flag
        } else {
            mask
        }
    });
    record_shortcut(hwnd, Shortcut { key, modifiers });
    true
}

fn execute(state: &State, hwnd: HWND, id: usize) -> Result<(), String> {
    let selected = || {
        engines::selected_index(hwnd)
            .and_then(|index| state.app.borrow().engines().get(index).cloned())
            .ok_or_else(|| "请选择搜索引擎".to_owned())
    };
    let result = |err: yisou::application::UpdateError| err.to_string();
    let update = match id {
        100 => {
            let enabled = !state.app.borrow().settings().preferences.start_with_windows;
            let path = std::env::current_exe().map_err(|e| e.to_string())?;
            platform::set_startup(&path, Some(&state.config), enabled)
                .map_err(|e| e.to_string())?;
            if let Err(error) = change(state, |settings| {
                settings.preferences.start_with_windows = enabled
            }) {
                platform::set_startup(&path, Some(&state.config), !enabled)
                    .map_err(|e| format!("{error}\n{e}"))?;
                return Err(error);
            }
            set_toggle(hwnd, 100, "登录 Windows 时启动", enabled);
            false
        }
        101 => {
            let enabled = !state
                .app
                .borrow()
                .settings()
                .preferences
                .show_on_manual_launch;
            change(state, |s| s.preferences.show_on_manual_launch = enabled)?;
            set_toggle(hwnd, 101, "手动启动时显示搜索框", enabled);
            false
        }
        102 => {
            let enabled = !state.app.borrow().settings().preferences.close_to_tray;
            change(state, |s| s.preferences.close_to_tray = enabled)?;
            set_toggle(hwnd, 102, "关闭搜索框后驻留托盘", enabled);
            false
        }
        104 => {
            let choice = unsafe { SendMessageW(GetDlgItem(hwnd, 103), CB_GETCURSEL, 0, 0) };
            let language = match choice {
                1 => Language::Chinese,
                2 => Language::English,
                _ => Language::System,
            };
            change(state, |s| s.preferences.language = language)?;
            PANEL.with_borrow_mut(|panel| panel.english = english_selected(language));
            reload_navigation(hwnd, 0);
            true
        }
        110 | 114 => {
            change(state, |s| s.preferences.show_engine_names = id == 110)?;
            true
        }
        111 | 112 => {
            let font = if id == 112 {
                Font::Default
            } else {
                let Some(family) = choose_system_font(state, hwnd) else {
                    return Ok(());
                };
                Font::System(family)
            };
            change(state, |s| s.preferences.font = font)?;
            state.reload_file_font();
            state.apply_font();
            true
        }
        113 => {
            let Some(path) = choose_file(hwnd, w!("字体文件\0*.ttf;*.otf;*.ttc;*.otc\0\0"), false)
            else {
                return Ok(());
            };
            let font =
                super::font::FileFont::load(&path).map_err(|_| "无法加载字体文件".to_owned())?;
            change(state, |s| s.preferences.font = Font::File(path))?;
            state.file_font.replace(Some(font));
            state.apply_font();
            true
        }
        120 => {
            let e = selected()?;
            engines::remember_list(hwnd);
            change(state, |s| s.preferences.default_engine = e.id)?;
            true
        }
        121 => {
            let engine = selected()?;
            engines::remember_list(hwnd);
            state
                .app
                .borrow_mut()
                .toggle_pin(&engine.id)
                .map_err(result)?;
            true
        }
        122 | 123 => {
            if id == 123 {
                selected()?;
            }
            engines::remember_list(hwnd);
            let selected = (id == 123).then(|| engines::selected_index(hwnd)).flatten();
            PANEL.with_borrow_mut(|p| {
                p.edit_engine = true;
                p.selected = selected;
                p.view.scroll = 0;
            });
            true
        }
        124 => {
            let engine = selected()?;
            let prompt = if state.app.borrow().settings().preferences.default_engine == engine.id {
                "删除此引擎并将默认引擎恢复为必应？"
            } else if state
                .app
                .borrow()
                .settings()
                .preferences
                .selection
                .as_ref()
                .is_some_and(|binding| binding.engine == engine.id)
            {
                "删除此引擎并清除对应的划词快捷键？"
            } else {
                "删除此搜索引擎？"
            };
            if !confirm(hwnd, prompt) {
                return Ok(());
            }
            engines::remember_list(hwnd);
            state
                .app
                .borrow_mut()
                .delete_engine(&engine.id)
                .map_err(result)?;
            PANEL.with_borrow_mut(|p| p.selected = None);
            true
        }
        125 => {
            let url = if PANEL.with_borrow(|p| p.edit_engine) {
                field(hwnd, 22)
            } else {
                selected()?.url
            };
            state
                .app
                .borrow_mut()
                .test_engine(&url, "易搜")
                .map_err(|e| e.to_string())?;
            false
        }
        126 => {
            let engine = edited_engine(state);
            let alias = field(hwnd, 21);
            let saved = if let Some(engine) = engine.as_ref().filter(|engine| !engine.is_custom()) {
                state
                    .app
                    .borrow_mut()
                    .set_built_in_alias(&engine.id, alias)
                    .map_err(engine_save_error)?;
                engine.id.clone()
            } else {
                let color = u32::from_str_radix(field(hwnd, 23).trim().trim_start_matches('#'), 16)
                    .map_err(|e| e.to_string())?;
                let mut engine = engine.unwrap_or(Engine {
                    id: String::new(),
                    name: String::new(),
                    alias: String::new(),
                    url: String::new(),
                    color,
                    icon: Icon::Default,
                });
                engine.name = field(hwnd, 20);
                engine.alias = alias;
                engine.url = field(hwnd, 22);
                engine.color = color;
                engine.icon = PANEL.with_borrow(|p| {
                    p.view
                        .engine_icon
                        .as_ref()
                        .map_or(Icon::Default, |preview| preview.icon.clone())
                });
                let saved = state
                    .app
                    .borrow_mut()
                    .save_engine(engine)
                    .map_err(engine_save_error)?;
                if let Some(painter) = state.painter.borrow_mut().as_mut() {
                    painter.reset_images();
                }
                state.reset_backdrop_images();
                fluent::reset_images();
                saved
            };
            let selected = state
                .app
                .borrow()
                .engines()
                .iter()
                .position(|engine| engine.id == saved);
            PANEL.with_borrow_mut(|p| {
                p.edit_engine = false;
                p.engine_draft = None;
                p.selected = selected;
            });
            state.close_popup();
            true
        }
        119 => {
            if let Some(engine) = edited_engine(state).and_then(|engine| {
                yisou::engine::built_in_engines()
                    .into_iter()
                    .find(|built_in| built_in.id == engine.id)
            }) {
                set_field(hwnd, 21, &engine.alias);
                focus_control(hwnd, 21);
            }
            false
        }
        127 => {
            if let Some(path) = choose_file(
                hwnd,
                w!("图标文件\0*.svg;*.png;*.jpg;*.jpeg;*.bmp;*.ico\0\0"),
                false,
            ) {
                let icon = engines::load_icon(&path)?;
                PANEL.with_borrow_mut(|p| p.view.engine_icon = Some(engines::preview_icon(icon)));
                fluent::reset_images();
                unsafe {
                    SetWindowTextW(
                        GetDlgItem(hwnd, 24),
                        wide(&path.display().to_string()).as_ptr(),
                    );
                }
                set_field(hwnd, pages::EDIT_STATUS as i32, "");
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            false
        }
        128 => {
            if !confirm_engine_exit(hwnd) {
                return Ok(());
            }
            PANEL.with_borrow_mut(|p| {
                p.edit_engine = false;
                p.engine_draft = None;
                p.view.scroll = 0;
            });
            true
        }
        129 | 130 => {
            let engine = selected()?;
            engines::remember_list(hwnd);
            let direction = if id == 129 { -1 } else { 1 };
            state
                .app
                .borrow_mut()
                .move_engine(&engine.id, direction)
                .map_err(result)?;
            let selected = state
                .app
                .borrow()
                .engines()
                .iter()
                .position(|item| item.id == engine.id);
            PANEL.with_borrow_mut(|p| p.selected = selected);
            true
        }
        131 => {
            choose_color(hwnd);
            set_field(hwnd, pages::EDIT_STATUS as i32, "");
            false
        }
        engines::MENU_COMMAND => {
            engines::show_menu(state, hwnd);
            return Ok(());
        }
        134 | 145 | 146 | 147 | 148 | 157 => {
            if PANEL.with_borrow(|p| p.page == OCR_PAGE) {
                let mut settings = state.app.borrow().settings().clone();
                settings.preferences.ocr_shortcut = if id == 148 {
                    PANEL.with_borrow(|p| p.recording.and_then(|(_, shortcut)| shortcut))
                } else {
                    None
                };
                apply_hotkeys(state, settings)?;
                PANEL.with_borrow_mut(|p| p.recording = None);
                display(state, hwnd);
                focus_control(hwnd, 156);
                return Ok(());
            }
            let draft = PANEL.with_borrow(|p| p.recording);
            let value = |target, field_id, reset_id| -> Result<Option<Shortcut>, String> {
                if id == reset_id {
                    return Ok(None);
                }
                if id == 148
                    && let Some((active, shortcut)) = draft
                    && active == target
                {
                    return Ok(shortcut);
                }
                parse_shortcut(&field(hwnd, field_id))
            };
            let mut settings = state.app.borrow().settings().clone();
            let selection_engine = {
                let choice = unsafe { SendMessageW(GetDlgItem(hwnd, 136), CB_GETCURSEL, 0, 0) };
                let app = state.app.borrow();
                usize::try_from(choice)
                    .ok()
                    .and_then(|index| app.engines().get(index))
                    .map(|engine| engine.id.clone())
                    .unwrap_or_else(|| settings.preferences.default_engine.clone())
            };
            let p = &mut settings.preferences;
            p.main_shortcut = value(137, 20, 145)?.unwrap_or_default();
            p.selection = value(138, 21, 146)?.map(|shortcut| SelectionBinding {
                shortcut,
                engine: selection_engine,
                preview: unsafe { SendMessageW(GetDlgItem(hwnd, 135), CB_GETCURSEL, 0, 0) == 1 },
            });
            p.clipboard_shortcut = value(139, 22, 147)?;
            p.excluded_apps = field(hwnd, 23)
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            apply_hotkeys(state, settings)?;
            PANEL.with_borrow_mut(|p| p.recording = None);
            display(state, hwnd);
            let focus = match id {
                145 => 137,
                146 => 138,
                147 => 139,
                148 => draft.map_or(137, |(target, _)| target as i32),
                _ => 23,
            };
            focus_control(hwnd, focus);
            return Ok(());
        }
        137..=139 | 156 => {
            start_recording(hwnd, id);
            return Ok(());
        }
        149 => {
            let target = PANEL.with_borrow_mut(|p| p.recording.take().map(|(target, _)| target));
            display(state, hwnd);
            if let Some(target) = target {
                focus_control(hwnd, target as i32);
            }
            return Ok(());
        }
        142 | 1440 => {
            PANEL.with_borrow_mut(|p| {
                p.selected = None;
                p.browser_panel = if id == 142 {
                    BrowserPanel::Path
                } else {
                    BrowserPanel::Closed
                };
            });
            true
        }
        143 => {
            if let Some(path) = choose_file(hwnd, w!("应用程序\0*.exe\0\0"), false) {
                set_field(hwnd, 20, &path.display().to_string());
            }
            false
        }
        140 | 141 => {
            let path = match id {
                140 => {
                    let Some(index) = select_index(hwnd) else {
                        return Ok(());
                    };
                    if index == 0 {
                        None
                    } else {
                        Some(
                            PANEL
                                .with_borrow(|p| p.browsers.get(index - 1).map(|b| b.path.clone()))
                                .or_else(|| {
                                    state.app.borrow().settings().preferences.browser.clone()
                                })
                                .ok_or_else(|| "请选择有效的浏览器程序".to_owned())?,
                        )
                    }
                }
                141 => {
                    let text = field(hwnd, 20);
                    (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()))
                }
                _ => unreachable!(),
            };
            if path.as_ref().is_some_and(|path| {
                !path.is_absolute()
                    || !path.is_file()
                    || !path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            }) {
                return Err("请选择有效的浏览器程序".into());
            }
            change(state, |s| s.preferences.browser = path)?;
            PANEL.with_borrow_mut(|p| p.browser_panel = BrowserPanel::Closed);
            true
        }
        150 => {
            let enabled = !state.app.borrow().settings().preferences.record_history;
            change(state, |s| s.preferences.record_history = enabled)?;
            set_toggle(hwnd, 150, "记录搜索历史", enabled);
            false
        }
        151 | 152 => {
            if let Some(record) = select_index(hwnd)
                .and_then(|index| PANEL.with_borrow(|p| p.history.get(index).cloned()))
            {
                if id == 151 {
                    state.search(record.repeat(), true);
                } else {
                    state
                        .app
                        .borrow_mut()
                        .history()
                        .delete(record.id)
                        .map_err(|e| e.to_string())?;
                }
            }
            if id == 152 {
                let previous = select_index(hwnd).unwrap_or(0);
                populate_history(state, hwnd, false);
                let count = PANEL.with_borrow(|p| p.history.len());
                unsafe {
                    let list = GetDlgItem(hwnd, 10);
                    if count > 0 {
                        SendMessageW(list, LB_SETCURSEL, previous.min(count - 1), 0);
                        SetFocus(list);
                    } else {
                        SetFocus(GetDlgItem(hwnd, 25));
                    }
                }
            }
            false
        }
        153 | 154 => {
            PANEL.with_borrow_mut(|p| {
                if id == 153 {
                    p.history_page = p.history_page.saturating_sub(1);
                } else if p.history.len() == 24 {
                    p.history_page += 1;
                }
            });
            populate_history(state, hwnd, false);
            focus_control(hwnd, 10);
            false
        }
        155 => {
            set_field(hwnd, 25, "");
            set_field(hwnd, 26, "");
            populate_history(state, hwnd, true);
            focus_control(hwnd, 25);
            false
        }
        161..=164 => {
            let Some(path) = choose_file(
                hwnd,
                w!("易搜配置\0*.json\0所有文件\0*.*\0\0"),
                id == 162 || id == 164,
            ) else {
                return Ok(());
            };
            match id {
                161 => {
                    let settings =
                        yisou::storage::import_settings(&path).map_err(|e| e.to_string())?;
                    if !confirm(hwnd, "导入将替换当前配置。是否继续？") {
                        return Ok(());
                    }
                    apply_hotkeys(state, settings)?;
                    state.reload_file_font();
                    state.apply_font();
                    let language = state.app.borrow().settings().preferences.language;
                    PANEL.with_borrow_mut(|panel| panel.english = english_selected(language));
                    reload_navigation(hwnd, 6);
                }
                162 => state
                    .app
                    .borrow()
                    .export_settings(&path)
                    .map_err(|e| e.to_string())?,
                163 => state
                    .app
                    .borrow_mut()
                    .import_engine_pack(&path)
                    .map_err(result)?,
                _ => state
                    .app
                    .borrow()
                    .export_engine_pack(&path)
                    .map_err(|e| e.to_string())?,
            }
            if let Some(painter) = state.painter.borrow_mut().as_mut() {
                painter.reset_images();
            }
            state.reset_backdrop_images();
            true
        }
        165 => {
            if let Some(path) = choose_file(hwnd, w!("易搜配置\0*.json\0\0"), true) {
                if !path.is_file() {
                    state
                        .app
                        .borrow()
                        .export_settings(&path)
                        .map_err(|e| e.to_string())?;
                } else {
                    yisou::storage::import_settings(&path).map_err(|e| e.to_string())?;
                }
                platform::remember_settings_path(&state.data, &path).map_err(|e| e.to_string())?;
                PANEL.with_borrow_mut(|p| p.next_config = Some(path));
            }
            true
        }
        #[cfg(feature = "ocr")]
        170..=175 => ocr::execute(state, hwnd, id)?,
        #[cfg(feature = "ocr")]
        176 => {
            select_page(state, hwnd, OCR_PAGE);
            focus_control(hwnd, 156);
            return Ok(());
        }
        _ => false,
    };
    state.layout();
    state.refresh_engine_button();
    if update {
        display(state, hwnd);
        match id {
            122 | 123 => {
                let field = if unsafe { GetDlgItem(hwnd, 20) }.is_null() {
                    21
                } else {
                    20
                };
                focus_control(hwnd, field);
            }
            120 | 121 | 124 | 126 | 128 | 129 | 130 => focus_control(hwnd, 10),
            140 | 141 | 1440 => focus_control(hwnd, 10),
            142 => focus_control(hwnd, 20),
            _ => {}
        }
    }
    Ok(())
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(state) = session() else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            fluent::paint(hwnd, dc, ps.rcPaint);
            EndPaint(hwnd, &ps);
            0
        },
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            fluent::control_color(hwnd, wp as HDC, lp as HWND, msg)
        }
        WM_DRAWITEM if lp != 0 => {
            fluent::draw_item(&state, hwnd, unsafe { &*(lp as *const DRAWITEMSTRUCT) });
            1
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
            fluent::apply_palette(hwnd);
            0
        }
        WM_GETMINMAXINFO if lp != 0 => unsafe {
            let dpi = GetDpiForWindow(hwnd).max(96);
            let limits = &mut *(lp as *mut MINMAXINFO);
            limits.ptMinTrackSize.x = dip(667, dpi);
            limits.ptMinTrackSize.y = dip(345, dpi);
            0
        },
        WM_SIZE if state.settings_window.get() == hwnd && wp != SIZE_MINIMIZED as usize => {
            fluent::layout(hwnd);
            0
        }
        WM_MOUSEWHEEL | WM_VSCROLL => {
            fluent::scroll(hwnd, msg, wp);
            0
        }
        BACK => {
            let cancel = PANEL.with_borrow(|p| {
                if p.recording.is_some() {
                    149
                } else if p.edit_engine {
                    128
                } else if p.browser_panel != BrowserPanel::Closed {
                    1440
                } else {
                    0
                }
            });
            if cancel == 0 {
                unsafe { DestroyWindow(hwnd) };
            } else {
                let _ = execute(&state, hwnd, cancel);
            }
            0
        }
        HISTORY_ACTION => {
            if PANEL.with_borrow(|p| p.page == 5)
                && let Err(error) = execute(&state, hwnd, wp)
            {
                message(hwnd, &error);
            }
            0
        }
        ENGINE_MENU => {
            if PANEL.with_borrow(|p| p.page == 2 && !p.edit_engine) {
                engines::show_menu(&state, hwnd);
            }
            0
        }
        WM_COMMAND if wp & 0xffff == NAV && (wp >> 16) as u32 == LBN_SELCHANGE => {
            let page = unsafe { SendMessageW(GetDlgItem(hwnd, NAV as i32), LB_GETCURSEL, 0, 0) };
            if let Some(page) = usize::try_from(page)
                .ok()
                .and_then(|index| nav_pages().nth(index))
            {
                select_page(&state, hwnd, page);
            }
            0
        }
        WM_COMMAND if wp & 0xffff == 10 && (wp >> 16) as u32 == LBN_SELCHANGE => 0,
        WM_COMMAND if wp & 0xffff == 10 && (wp >> 16) as u32 == LBN_DBLCLK => {
            let page = PANEL.with_borrow(|p| p.page);
            if page == 2 && engines::selected_index(hwnd).is_some() {
                unsafe { PostMessageW(hwnd, WM_COMMAND, 123, 0) };
            } else if page == 4 {
                unsafe { PostMessageW(hwnd, WM_COMMAND, 140, 0) };
            }
            0
        }
        WM_COMMAND if matches!(wp & 0xffff, 103 | 135 | 136) && lp != 0 => unsafe {
            let code = (wp >> 16) as u32;
            dropdown::notice(lp as HWND, code);
            if wp & 0xffff == 103
                && (code == CBN_CLOSEUP || code == CBN_SELCHANGE && !dropdown::dropped(lp as HWND))
            {
                let choice = SendMessageW(lp as HWND, CB_GETCURSEL, 0, 0);
                let saved = match state.app.borrow().settings().preferences.language {
                    Language::System => 0,
                    Language::Chinese => 1,
                    Language::English => 2,
                };
                EnableWindow(GetDlgItem(hwnd, 104), i32::from(choice != saved));
            }
            0
        },
        WM_COMMAND if matches!(wp & 0xffff, 25 | 26) && (wp >> 16) as u32 == EN_CHANGE => unsafe {
            SetTimer(hwnd, HISTORY_FILTER, 180, None);
            0
        },
        WM_COMMAND if wp & 0xffff == 27 && (wp >> 16) as u32 == EN_CHANGE => unsafe {
            SetTimer(hwnd, ENGINE_FILTER, 100, None);
            0
        },
        WM_COMMAND if wp & 0xffff == 20 && (wp >> 16) as u32 == EN_CHANGE => unsafe {
            if PANEL.with_borrow(|p| p.edit_engine) {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        },
        WM_COMMAND if wp & 0xffff == 21 && (wp >> 16) as u32 == EN_CHANGE => {
            if PANEL.with_borrow(|p| p.edit_engine) {
                set_field(hwnd, pages::EDIT_STATUS as i32, "");
            }
            0
        }
        WM_TIMER if wp == ENGINE_FILTER => unsafe {
            KillTimer(hwnd, ENGINE_FILTER);
            if PANEL.with_borrow(|p| p.page == 2 && !p.edit_engine) {
                engines::refresh(&state, hwnd, true);
            }
            0
        },
        #[cfg(feature = "ocr")]
        WM_COMMAND if wp & 0xffff == 40 && (wp >> 16) as u32 == EN_CHANGE => {
            if PANEL.with_borrow(|p| p.page == OCR_PAGE) {
                ocr::change_path(hwnd);
            }
            0
        }
        #[cfg(feature = "ocr")]
        WM_TIMER if wp == ocr::TIMER => {
            ocr::poll(&state, hwnd);
            0
        }
        WM_TIMER if wp == fluent::TOGGLE_TIMER => {
            fluent::tick_toggle(hwnd);
            0
        }
        WM_TIMER if wp == HISTORY_FILTER => unsafe {
            KillTimer(hwnd, HISTORY_FILTER);
            if PANEL.with_borrow(|p| p.page == 5) {
                populate_history(&state, hwnd, true);
            }
            0
        },
        WM_COMMAND if wp & 0xffff >= 100 && (wp >> 16) as u32 == BN_CLICKED => {
            if let Err(error) = execute(&state, hwnd, wp & 0xffff) {
                let id = wp & 0xffff;
                let inline = if matches!(id, 125..=127) {
                    pages::EDIT_STATUS
                } else if id == 148 {
                    pages::RECORD_STATUS
                } else {
                    0
                };
                if inline != 0 && !unsafe { GetDlgItem(hwnd, inline as i32) }.is_null() {
                    set_field(hwnd, inline as i32, tr(&error));
                } else {
                    message(hwnd, tr(&error));
                }
            }
            0
        }
        WM_DPICHANGED if lp != 0 => unsafe {
            let bounds = &*(lp as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let font = if PANEL.with_borrow(|p| p.page == 1) {
                state.create_font_at(wp as u32 & 0xffff, 20)
            } else {
                null_mut()
            };
            let old = PANEL.with_borrow_mut(|p| std::mem::replace(&mut p.preview_font, font));
            fluent::layout(hwnd);
            if !old.is_null() {
                DeleteObject(old);
            }
            0
        },
        WM_CLOSE => unsafe {
            if confirm_engine_exit(hwnd) {
                DestroyWindow(hwnd);
            }
            0
        },
        WM_DESTROY => {
            state.settings_window.set(null_mut());
            PANEL.with_borrow_mut(|p| *p = Panel::default());
            state.app.borrow_mut().history().release_cache();
            state.diagnostics.sample("settings_closed");
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
