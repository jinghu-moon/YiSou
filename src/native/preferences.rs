mod appearance;
mod color_picker;
mod components;
mod dropdown;
mod engines;
mod fluent;
mod fonts;
mod history;
mod menu;
#[cfg(feature = "ocr")]
mod ocr;
mod pages;
mod recorder;
mod scrollbar;

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
                    CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW, GetOpenFileNameW,
                    GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
                    OFN_OVERWRITEPROMPT, OPENFILENAMEW,
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
    application::UpdateError,
    engine::{Engine, EngineError, Icon},
    icons::{import_bitmap, import_svg},
    platform,
    settings::{
        Font, Language, MAX_SEARCH_ENGINE_ICON_SIZE, Preferences, SearchMaterial, SearchTone,
        SelectionBinding, Settings, SettingsError,
    },
    shortcut::Shortcut,
};

const CLASS: *const u16 = w!("YiSou.Rust.Settings");
const NAV: usize = 1;
const HISTORY_FILTER: usize = 1;
const BACK: u32 = WM_APP + 40;
const HISTORY_ACTION: u32 = WM_APP + 41;
const ENGINE_MENU: u32 = WM_APP + 42;
const ENGINE_TOGGLE: u32 = WM_APP + 43;
const HISTORY_CHANGED: u32 = WM_APP + 44;
const ENGINE_FILTER: usize = 2;
const EXCLUSION_ADD: usize = 191;
const EXCLUSION_REMOVE: usize = 2000;
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

pub(super) fn english_selected(language: Language) -> bool {
    match language {
        Language::English => true,
        Language::Chinese => false,
        Language::System => (unsafe { GetUserDefaultUILanguage() } & 0x03ff) != 0x04,
    }
}

fn tr(text: &str) -> &str {
    if PANEL.with_borrow(|p| p.english) {
        match text {
            "按下快捷键…" => "Press a shortcut…",
            "按下快捷键 · Esc 取消 · Backspace 清除" => {
                "Press a shortcut · Esc to cancel · Backspace to clear"
            }
            "需要包含 Ctrl / Alt / Win（F1–F11 除外）" => {
                "Use Ctrl, Alt or Win, or a function key (F1–F11)"
            }
            "系统保留，无法使用" => "Reserved by Windows",
            "与「{}」冲突" => "Conflicts with “{}”",
            "覆盖" => "Replace",
            "未设置" => "Not set",
            "默认" => "Default",
            "左 Alt" => "Left Alt",
            "应用后生效" => "Applies immediately",
            "应用所选" => "Use selected",
            "搜索内容" => "Search terms",
            "上一页" => "Previous page",
            "例如 mstsc.exe" => "For example, mstsc.exe",
            "没有排除应用" => "No excluded apps",
            "请输入 .exe 文件名，例如 mstsc.exe。" => {
                "Enter an .exe file name, such as mstsc.exe."
            }
            "OCR 搜索" => "OCR search",
            "本地模型目录" => "Local model folder",
            "ModelScope 下载 · PP-OCRv6 tiny" => "ModelScope downloads · PP-OCRv6 tiny",
            "检测模型" => "Detector",
            "识别模型" => "Recognizer",
            "字典" => "Dictionary",
            "录制 OCR 快捷键" => "Record OCR shortcut",
            "截图搜图" => "Screenshot image search",
            "测试打开" => "Test browser",
            "撤销删除" => "Undo delete",
            "请输入包含 %s 的 HTTP 或 HTTPS 搜索网址。" => {
                "Enter an HTTP or HTTPS search URL containing %s."
            }
            "请输入有效的引擎名称。" => "Enter a valid engine name.",
            "应用首选项" => "App preferences",
            "搜索引擎与排序" => "Search engines and order",
            "导入将替换当前配置。是否继续？" => {
                "Replace the current settings with the imported settings?"
            }
            "没有配置差异" => "No settings differences",
            "框选图片并搜索来源。" => "Select an image and search for its source.",
            "框选后按 Enter" => "Enter after selecting an area",
            "选择搜图引擎" => "Choose an image search engine",
            "同时搜图" => "Search both engines",
            "请选择 OCR 模型目录" => "Choose an OCR model folder",
            "启动与后台" => "Startup and background",
            "当前字体" => "Current font",
            "字体" => "Font",
            "筛选字体" => "Filter fonts",
            "搜索字体名称" => "Search font names",
            "字体家族" => "Font family",
            "字形" => "Font style",
            "字号" => "Font size",
            "预览" => "Preview",
            "拉丁文" => "Latin",
            "日语" => "Japanese",
            "韩语" => "Korean",
            "显示更多字体" => "Display more fonts",
            "字号必须在 8 至 72 DIP 之间。" => "Enter a font size from 8 to 72 DIP.",
            "无法打开 Windows 字体设置。" => "Couldn't open Windows font settings.",
            "没有可用的系统字体" => "No system fonts available",
            "菜单显示名称" => "Show names in menu",
            "右侧图标显示名称" => "Show name beside search icon",
            "搜索框" => "Search bar",
            "搜索框背景" => "Search background",
            "搜索框文字" => "Search text",
            "引擎图标与菜单" => "Icons and menu",
            "搜索历史浮层" => "History popup",
            "启用搜索历史浮层" => "Enable history popup",
            "显示底部快捷键提示" => "Show keyboard shortcuts",
            "浮层背景" => "Popup background",
            "浮层色调" => "Popup tone",
            "匹配文本高亮背景色" => "Match highlight color",
            "浮层圆角" => "Popup corner radius",
            "搜索框圆角" => "Search box corner radius",
            "搜索框宽度" => "Search box width",
            "减小搜索框宽度" => "Decrease search box width",
            "增大搜索框宽度" => "Increase search box width",
            "减小圆角" => "Decrease corner radius",
            "增大圆角" => "Increase corner radius",
            "背景配色" => "Background colors",
            "输入字号" => "Input font size",
            "搜索框内边距" => "Search box padding",
            "上" => "Top",
            "下" => "Bottom",
            "左" => "Left",
            "右" => "Right",
            "减小上内边距" => "Decrease top padding",
            "增大上内边距" => "Increase top padding",
            "减小下内边距" => "Decrease bottom padding",
            "增大下内边距" => "Increase bottom padding",
            "减小左内边距" => "Decrease left padding",
            "增大左内边距" => "Increase left padding",
            "减小右内边距" => "Decrease right padding",
            "增大右内边距" => "Increase right padding",
            "文字颜色" => "Text color",
            "右侧单色图标颜色" => "Monochrome icon color",
            "应用于搜索框以外的屏幕区域" => {
                "Applies to the screen outside the search bar"
            }
            "右侧图标大小" => "Search engine icon size",
            "菜单预览" => "Menu preview",
            "筛选名称或 #别名" => "Filter by name or #alias",
            "置顶快捷位" => "Pinned shortcuts",
            "添加引擎" => "Add engine",
            "搜索引擎列表" => "Search engines",
            "启用" => "Enabled",
            "至少保留一个已启用的搜索引擎。" => {
                "Keep at least one search engine enabled."
            }
            "引擎" => "Engine",
            "别名" => "Alias",
            "编辑别名" => "Edit alias",
            "单色图标" => "Monochrome icon",
            "多色图标" => "Color icon",
            "使用多色图标" => "Use color icons",
            "搜索框材质" => "Search box material",
            "搜索框色调" => "Search box tone",
            "搜索框输入字号" => "Search input font size",
            "搜索框文字颜色" => "Search text color",
            "搜索框单色图标颜色" => "Monochrome search icon color",
            "使用自动颜色" => "Use automatic color",
            "自动颜色" => "Automatic",
            "多色图标已启用" => "Color icons on",
            "浅色模式颜色" => "Light mode color",
            "深色模式颜色" => "Dark mode color",
            "颜色值" => "Color value",
            "颜色值 / 透明度" => "Color value / Transparency",
            "复制颜色值" => "Copy color value",
            "透明度" => "Transparency",
            "最近使用颜色" => "Recent colors",
            "预设颜色" => "Preset colors",
            "请输入有效的颜色值" => "Enter a valid color value.",
            "请输入有效的透明度" => "Enter a valid transparency value.",
            "无法复制颜色值" => "Could not copy the color value.",
            "亚克力透明度" => "Acrylic transparency",
            "云母透明度" => "Mica transparency",
            "降低透明度" => "Decrease transparency",
            "提高透明度" => "Increase transparency",
            "全屏背景效果" => "Desktop background effect",
            "纯暗遮罩" => "Dim overlay",
            "系统模糊" => "System blur",
            "高斯模糊" => "Gaussian blur",
            "背景模糊强度" => "Background blur strength",
            "降低模糊强度" => "Decrease blur strength",
            "提高模糊强度" => "Increase blur strength",
            "亚克力" => "Acrylic",
            "跟随搜索框" => "Follow search box",
            "云母" => "Mica",
            "纯色" => "Solid",
            "跟随系统" => "Follow system",
            "浅色" => "Light",
            "深色" => "Dark",
            "选择文件" => "Choose file",
            "清除" => "Clear",
            "选择强调色" => "Choose accent color",
            "强调色用于无图标时的首字母底色，也会替换导入 SVG 的 currentColor。单色图标的配色可以单独设置。" => {
                "The accent color fills the background of the fallback initial and supplies currentColor for imported SVGs. Set the monochrome icon color separately."
            }
            "单色配色" => "Monochrome color",
            "选择颜色" => "Choose color",
            "恢复内置配置" => "Restore built-in defaults",
            "恢复默认别名" => "Restore default alias",
            "别名须以字母开头，最多 16 位，可用 A–Z、数字、- 和 _。" => {
                "Start with A–Z. Use up to 16 letters, digits, - or _."
            }
            "该别名已被其他引擎使用。" => "Another engine already uses this alias.",
            "状态" => "Status",
            "更多操作" => "More actions",
            "没有符合条件的引擎" => "No matching engines",
            "前往设置" => "Open settings",
            "使用指定的引擎和触发动作。" => "Use the selected engine and action.",
            "划词搜索引擎" => "Selection search engine",
            "触发动作" => "Action",
            "打开搜索结果" => "Open search results",
            "暂无搜索记录" => "No search history yet",
            "没有符合筛选条件的记录" => "No matching history",
            "在历史记录中查找" => "Search history",
            "全部引擎" => "All engines",
            "全部来源" => "All sources",
            "无来源" => "No source",
            "今天" => "Today",
            "昨天" => "Yesterday",
            "日期未知" => "Unknown date",
            "下一页" => "Next",
            "清除筛选" => "Clear filters",
            "当前配置" => "Current configuration",
            "导入与导出" => "Import and export",
            "导入将替换当前配置。" => "Importing replaces the current settings.",
            "导入会合并自定义引擎与置顶顺序。" => {
                "Importing merges custom engines and pinned shortcuts."
            }
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
    appearance_tab: appearance::Tab,
    children: Vec<HWND>,
    selected: Option<usize>,
    edit_engine: bool,
    engine_filter: String,
    engine_rows: Vec<usize>,
    engine_drag: Option<engines::Drag>,
    engine_scroll_before_edit: i32,
    engine_menu: Option<(HWND, usize)>,
    engine_draft: Option<EngineDraft>,
    exclusions: Option<Vec<String>>,
    next_config: Option<PathBuf>,
    page_scroll: [i32; 8],
    history: history::View,
    #[cfg(feature = "ocr")]
    ocr_check: ocr::Check,
    browsers: Vec<platform::InstalledBrowser>,
    preview_font: HFONT,
    font_picker: Option<fonts::Picker>,
    view: fluent::View,
    browser_panel: BrowserPanel,
    recorder: recorder::Recorder,
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
    color_picker::register_window()?;
    scrollbar::register_window()?;
    history::register_window()?;
    menu::register_window()
}

pub(super) fn history_changed(state: &State) {
    let hwnd = state.settings_window.get();
    if !hwnd.is_null() && PANEL.with_borrow(|p| p.page == 5) {
        unsafe { PostMessageW(hwnd, HISTORY_CHANGED, 0, 0) };
    }
}

pub(super) fn show(state: &State, page: usize) {
    state.close_popup();
    let picker = color_picker::window();
    if !picker.is_null() {
        unsafe { SetForegroundWindow(picker) };
        return;
    }
    let language = state.app.borrow().settings().preferences.language;
    PANEL.with_borrow_mut(|panel| panel.english = english_selected(language));
    let mut hwnd = state.settings_window.get();
    if hwnd.is_null() {
        hwnd = unsafe {
            let owner = state.window.get();
            let dpi = GetDpiForWindow(owner).max(96);
            let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
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
        scrollbar::create(hwnd);
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
    let left_history = PANEL.with_borrow_mut(|panel| {
        let left_history = panel.page == 5 && page != 5;
        if panel.page != page {
            panel.page_scroll[panel.page] = panel.view.scroll;
            if panel.page == 2 && panel.edit_engine {
                panel.page_scroll[2] = panel.engine_scroll_before_edit;
            }
            panel.selected = None;
            panel.edit_engine = false;
            panel.engine_draft = None;
            panel.browser_panel = BrowserPanel::Closed;
            if let Some(picker) = panel.font_picker.take() {
                panel.page_scroll[previous] = picker.previous_scroll;
            }
            panel.view.scroll = panel.page_scroll[page];
            panel.view.wheel = 0;
        }
        if left_history {
            panel.history.release_rows();
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
    if parent != color_picker::window() {
        PANEL.with_borrow_mut(|p| p.children.push(handle));
    }
    fluent::with_view_mut(parent, |view| view.controls.push((handle, area)));
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
        bounds.edit(),
    )
}

fn number_at(hwnd: HWND, id: usize, value: i32, bounds: Layout) -> HWND {
    child(
        hwnd,
        w!("EDIT"),
        &value.to_string(),
        WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_NUMBER as u32 | ES_RIGHT as u32,
        id,
        bounds.edit(),
    )
}

fn list_at(hwnd: HWND, items: impl IntoIterator<Item = String>, bounds: Layout) -> HWND {
    let list = list_control(hwnd, bounds, 0);
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

fn list_control(hwnd: HWND, bounds: Layout, style: u32) -> HWND {
    child(
        hwnd,
        w!("LISTBOX"),
        "",
        WS_TABSTOP
            | style
            | (LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT) as u32,
        10,
        bounds,
    )
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
    let toggle = unsafe { GetDlgItem(hwnd, id) };
    let label = components::toggle_label(title, enabled);
    if !toggle.is_null() && text_of(toggle) != label {
        fluent::animate_control_state(toggle, enabled);
        unsafe {
            SetWindowTextW(toggle, wide(&label).as_ptr());
            InvalidateRect(toggle, null(), 0);
        }
    }
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

#[derive(PartialEq)]
struct EngineDraft {
    fields: [String; 4],
    icons: [Icon; 2],
    mono_color: Option<u32>,
}

fn draft_fields(hwnd: HWND) -> EngineDraft {
    let (icons, mono_color) = PANEL.with_borrow(|p| {
        p.view
            .engine_icon
            .as_ref()
            .map_or_else(Default::default, |e| {
                ([e.mono_icon.clone(), e.icon.clone()], e.mono_color)
            })
    });
    EngineDraft {
        fields: [20, 21, 22, 23].map(|id| field(hwnd, id)),
        icons,
        mono_color,
    }
}

fn edited_engine(state: &State) -> Option<Engine> {
    PANEL
        .with_borrow(|p| p.selected)
        .and_then(|index| state.app.borrow().all_engines().get(index).cloned())
}

fn update_engine_validation(state: &State, hwnd: HWND) {
    let Some(mut engine) = PANEL.with_borrow(|p| p.view.engine_icon.clone()) else {
        return;
    };
    if unsafe { GetDlgItem(hwnd, 22).is_null() || GetDlgItem(hwnd, 126).is_null() } {
        return;
    }
    engine.name = field(hwnd, 20);
    engine.alias = field(hwnd, 21);
    engine.url = field(hwnd, 22);
    let validation = state.app.borrow().validate_engine(&mut engine);
    let label = match &validation {
        Ok(()) => yisou::engine::search_url(&engine.url, "易搜"),
        Err(EngineError::Url) => tr("请输入包含 %s 的 HTTP 或 HTTPS 搜索网址。").into(),
        Err(EngineError::Name) => tr("请输入有效的引擎名称。").into(),
        Err(EngineError::Alias) => {
            tr("别名须以字母开头，最多 16 位，可用 A–Z、数字、- 和 _。").into()
        }
        Err(EngineError::DuplicateAlias) => tr("该别名已被其他引擎使用。").into(),
        Err(error) => error.to_string(),
    };
    set_field(hwnd, pages::EDIT_STATUS as i32, &label);
    unsafe {
        EnableWindow(GetDlgItem(hwnd, 126), i32::from(validation.is_ok()));
    }
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
    if PANEL.with_borrow(|p| p.engine_draft.is_none()) {
        return true;
    }
    let current = draft_fields(hwnd);
    PANEL.with_borrow(|p| p.engine_draft.as_ref() == Some(&current))
        || confirm(hwnd, "放弃未保存的修改？")
}

fn choose_color(hwnd: HWND, current: u32) -> Option<u32> {
    let mut palette = [0; 16];
    let mut dialog = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: super::color_ref(current),
        lpCustColors: palette.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    (unsafe { ChooseColorW(&mut dialog) } != 0).then(|| super::color_ref(dialog.rgbResult))
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
    fluent::clear_control_motions(hwnd);
    recorder::reset(state, hwnd);
    menu::dismiss();
    // Keep the previous frame on screen until the new page has been laid out.
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    unsafe { KillTimer(hwnd, ENGINE_FILTER) };
    unsafe { KillTimer(hwnd, fonts::FILTER_TIMER) };
    let list = unsafe { GetDlgItem(hwnd, 10) };
    if !list.is_null() {
        engines::finish_drag(state, list, false);
    }
    if visible {
        unsafe { SendMessageW(hwnd, WM_SETREDRAW, 0, 0) };
    }
    let (children, preview_font) = PANEL.with_borrow_mut(|p| {
        p.view.controls.clear();
        p.view.surfaces.clear();
        p.view.row_icons.clear();
        p.view.engine_icon = None;
        p.view.extent = 0;
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
    if PANEL.with_borrow(|p| p.page == 2 && !p.edit_engine) {
        engines::restore_selection(hwnd);
    }
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

fn change(state: &State, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    state
        .app
        .borrow_mut()
        .change_settings(change)
        .map_err(|error| error.to_string())
}

fn set_search_appearance(
    state: &State,
    hwnd: HWND,
    id: usize,
    control: HWND,
) -> Result<(), String> {
    let index = unsafe { SendMessageW(control, CB_GETCURSEL, 0, 0) } as usize;
    if id == appearance::SCREEN_BACKGROUND {
        let current = state.app.borrow().settings().preferences.screen_background;
        if let Some(&next) = appearance::SCREEN_BACKGROUNDS.get(index)
            && current != next
        {
            if let Err(error) = change(state, |s| s.preferences.screen_background = next) {
                let saved = appearance::SCREEN_BACKGROUNDS
                    .iter()
                    .position(|&mode| mode == current);
                unsafe { SendMessageW(control, CB_SETCURSEL, saved.unwrap_or_default(), 0) };
                return Err(error);
            }
            state.refresh_screen_shade();
            appearance::refresh_screen_background(state, hwnd);
        }
        return Ok(());
    }
    let history = matches!(id, appearance::HISTORY_MATERIAL | appearance::HISTORY_TONE);
    let material_control = matches!(
        id,
        appearance::SEARCH_MATERIAL | appearance::HISTORY_MATERIAL
    );
    let (search_material, history_material, tone) = {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        (
            prefs.search_material,
            prefs.history_popup.material,
            if history {
                prefs.history_popup.tone
            } else {
                prefs.search_tone
            },
        )
    };
    let mut selected_search = search_material;
    let mut selected_history = history_material;
    let mut selected_tone = tone;
    match id {
        appearance::SEARCH_MATERIAL => {
            selected_search = appearance::MATERIALS
                .get(index)
                .copied()
                .unwrap_or(search_material);
        }
        appearance::HISTORY_MATERIAL => {
            selected_history = appearance::HISTORY_MATERIALS
                .get(index)
                .copied()
                .unwrap_or(history_material);
        }
        _ => selected_tone = appearance::TONES.get(index).copied().unwrap_or(tone),
    };
    if (search_material, history_material, tone)
        == (selected_search, selected_history, selected_tone)
    {
        return Ok(());
    }
    if let Err(error) = change(state, |settings| {
        let prefs = &mut settings.preferences;
        if history {
            prefs.history_popup.material = selected_history;
            prefs.history_popup.tone = selected_tone;
        } else {
            prefs.search_material = selected_search;
            prefs.search_tone = selected_tone;
        }
    }) {
        let saved = if material_control && history {
            appearance::HISTORY_MATERIALS
                .iter()
                .position(|&item| item == history_material)
        } else if material_control {
            appearance::MATERIALS
                .iter()
                .position(|&item| item == search_material)
        } else {
            appearance::TONES.iter().position(|&item| item == tone)
        };
        unsafe { SendMessageW(control, CB_SETCURSEL, saved.unwrap_or_default(), 0) };
        return Err(error);
    }
    if history {
        state.dismiss_history_popup();
        if history_material != selected_history {
            appearance::refresh_history_material(state, hwnd);
        }
        return Ok(());
    }
    state.refresh_search_appearance();
    if history_material == yisou::settings::HistoryMaterial::FollowSearch {
        state.dismiss_history_popup();
    }
    if search_material != selected_search {
        appearance::refresh_material(state, hwnd);
    } else {
        fluent::invalidate_search_preview(hwnd, &state.app.borrow().settings().preferences);
    }
    Ok(())
}

fn search_setting_value(preferences: &Preferences, id: usize) -> i32 {
    match id {
        appearance::SEARCH_FONT_SIZE => preferences.search_font_size,
        appearance::SEARCH_WIDTH => preferences.search_width,
        appearance::BACKGROUND_BLUR_PERCENT => preferences.background_blur_percent,
        appearance::ACRYLIC_TINT => 100 - preferences.acrylic_tint_percent,
        appearance::MICA_TINT => 100 - preferences.mica_tint_percent,
        appearance::HISTORY_ACRYLIC_TINT => 100 - preferences.history_popup.acrylic_tint_percent,
        appearance::HISTORY_MICA_TINT => 100 - preferences.history_popup.mica_tint_percent,
        appearance::SEARCH_RADIUS => preferences.search_corner_radius,
        appearance::HISTORY_RADIUS => preferences.history_popup.corner_radius,
        appearance::PADDING_TOP => preferences.search_padding.top,
        appearance::PADDING_BOTTOM => preferences.search_padding.bottom,
        appearance::PADDING_LEFT => preferences.search_padding.left,
        appearance::PADDING_RIGHT => preferences.search_padding.right,
        _ => preferences.search_engine_icon_size,
    }
}

fn search_number_input(id: usize) -> bool {
    matches!(
        id,
        appearance::ENGINE_ICON_SIZE
            | appearance::SEARCH_FONT_SIZE
            | appearance::SEARCH_WIDTH
            | appearance::BACKGROUND_BLUR_PERCENT
            | appearance::ACRYLIC_TINT
            | appearance::MICA_TINT
            | appearance::HISTORY_ACRYLIC_TINT
            | appearance::HISTORY_MICA_TINT
            | appearance::SEARCH_RADIUS
            | appearance::HISTORY_RADIUS
    ) || appearance::is_padding_input(id)
}

fn finish_search_number(state: &State, hwnd: HWND, input: HWND, commit: bool) {
    let id = unsafe { GetDlgCtrlID(input) } as usize;
    let Some(stepper) = appearance::search_steppers().find(|stepper| stepper.input == id) else {
        return;
    };
    let (previous, previous_height) = {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        (
            search_setting_value(prefs, id),
            appearance::search_preview_height(prefs),
        )
    };
    let text = text_of(input);
    let mut value = previous;
    let mut error = None;
    let digits = text.trim();
    if commit && !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        let max = *stepper.range.end();
        let next = digits.parse::<i32>().unwrap_or(max).min(max);
        if stepper.range.contains(&next) && next != previous {
            match change(state, |settings| match id {
                appearance::SEARCH_FONT_SIZE => settings.preferences.search_font_size = next,
                appearance::SEARCH_WIDTH => settings.preferences.search_width = next,
                appearance::BACKGROUND_BLUR_PERCENT => {
                    settings.preferences.background_blur_percent = next
                }
                appearance::ACRYLIC_TINT => settings.preferences.acrylic_tint_percent = 100 - next,
                appearance::MICA_TINT => settings.preferences.mica_tint_percent = 100 - next,
                appearance::HISTORY_ACRYLIC_TINT => {
                    settings.preferences.history_popup.acrylic_tint_percent = 100 - next
                }
                appearance::HISTORY_MICA_TINT => {
                    settings.preferences.history_popup.mica_tint_percent = 100 - next
                }
                appearance::SEARCH_RADIUS => settings.preferences.search_corner_radius = next,
                appearance::HISTORY_RADIUS => {
                    settings.preferences.history_popup.corner_radius = next
                }
                appearance::PADDING_TOP => settings.preferences.search_padding.top = next,
                appearance::PADDING_BOTTOM => settings.preferences.search_padding.bottom = next,
                appearance::PADDING_LEFT => settings.preferences.search_padding.left = next,
                appearance::PADDING_RIGHT => settings.preferences.search_padding.right = next,
                _ => settings.preferences.search_engine_icon_size = next,
            }) {
                Ok(()) => {
                    value = next;
                    if id == appearance::SEARCH_FONT_SIZE {
                        state.apply_font();
                        fluent::update_search_font_size(state, hwnd, previous_height);
                    } else if id == appearance::BACKGROUND_BLUR_PERCENT {
                        state.refresh_screen_shade();
                    } else if matches!(id, appearance::ENGINE_ICON_SIZE | appearance::SEARCH_WIDTH)
                        || appearance::is_padding_input(id)
                    {
                        state.close_popup();
                        state.relayout_search_controls();
                        fluent::update_search_preview(state, hwnd, previous_height);
                    } else if matches!(
                        id,
                        appearance::HISTORY_ACRYLIC_TINT
                            | appearance::HISTORY_MICA_TINT
                            | appearance::HISTORY_RADIUS
                    ) {
                        state.dismiss_history_popup();
                    } else if id == appearance::SEARCH_RADIUS {
                        state.refresh_search_appearance();
                        state.update_search_corners();
                        fluent::invalidate_search_preview(
                            hwnd,
                            &state.app.borrow().settings().preferences,
                        );
                    } else {
                        // The search box reads the tint when repainting.
                        state.edit_dirty.set(true);
                        state.invalidate_following_history_background();
                        fluent::invalidate_search_preview(
                            hwnd,
                            &state.app.borrow().settings().preferences,
                        );
                        unsafe { InvalidateRect(state.window.get(), null(), 0) };
                    }
                }
                Err(failure) => error = Some(failure),
            }
        }
    }
    let saved = value.to_string();
    if saved != text {
        unsafe { SetWindowTextW(input, wide(&saved).as_ptr()) };
    }
    if let Some(error) = error {
        // The input has been restored before the modal dialog changes focus.
        message(hwnd, &error);
    }
    stepper.sync(hwnd, value);
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
    let (old, old_startup) = {
        let app = state.app.borrow();
        let current = app.settings();
        (
            State::bindings(current),
            current.preferences.start_with_windows,
        )
    };
    let startup = settings.preferences.start_with_windows;
    let executable = (old_startup != startup)
        .then(std::env::current_exe)
        .transpose()
        .map_err(|error| error.to_string())?;
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
    if let Some(path) = &executable
        && let Err(error) = platform::set_startup(path, Some(&state.config), startup)
    {
        return Err(match state.bind_hotkeys(old) {
            Ok(()) => error.to_string(),
            Err(restore) => format!("{error}\n{restore}"),
        });
    }
    if let Err(error) = change(state, |target| *target = settings) {
        let mut error = error;
        if let Err(restore) = state.bind_hotkeys(old) {
            error.push_str(&format!("\n{restore}"));
        }
        if let Some(path) = &executable
            && let Err(restore) = platform::set_startup(path, Some(&state.config), old_startup)
        {
            error.push_str(&format!("\n{restore}"));
        }
        return Err(error);
    }
    #[cfg(feature = "ocr")]
    if ocr_changed {
        state.end_ocr_if_active();
    }
    Ok(())
}

fn select_index(hwnd: HWND) -> Option<usize> {
    usize::try_from(unsafe { SendMessageW(GetDlgItem(hwnd, 10), LB_GETCURSEL, 0, 0) }).ok()
}

pub(super) fn capture_registered(state: &State, shortcut: Shortcut) -> bool {
    let hwnd = state.settings_window.get();
    !hwnd.is_null() && recorder::registered(state, hwnd, shortcut)
}

pub(super) fn capture_key(hwnd: HWND, msg: &MSG) -> bool {
    if color_picker::capture_key(msg) {
        return true;
    }
    if msg.hwnd != hwnd && unsafe { IsChild(hwnd, msg.hwnd) } == 0 {
        return false;
    }
    if recorder::key(hwnd, msg) {
        return true;
    }
    if matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let id = unsafe { GetDlgCtrlID(msg.hwnd) };
        let key = msg.wParam as u16;
        if search_number_input(id as usize) && matches!(key, VK_RETURN | VK_ESCAPE) {
            unsafe { SendMessageW(msg.hwnd, WM_KEYDOWN, msg.wParam, msg.lParam) };
            return true;
        }
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
            (5, id, VK_RETURN) if id as usize == history::KEYWORD => {
                unsafe { PostMessageW(hwnd, WM_TIMER, HISTORY_FILTER, 0) };
                return true;
            }
            _ => return false,
        };
        unsafe { PostMessageW(hwnd, WM_COMMAND, command, 0) };
        true
    } else {
        false
    }
}

fn execute(state: &State, hwnd: HWND, id: usize) -> Result<(), String> {
    let selected = || {
        engines::selected_index(hwnd)
            .and_then(|index| state.app.borrow().all_engines().get(index).cloned())
            .ok_or_else(|| "请选择搜索引擎".to_owned())
    };
    let result = |err: yisou::application::UpdateError| err.to_string();
    let update = match id {
        100 => {
            let mut settings = state.app.borrow().settings().clone();
            let enabled = !settings.preferences.start_with_windows;
            settings.preferences.start_with_windows = enabled;
            apply_settings_with_checked_model(state, settings)?;
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
        id if let Some(tab) = appearance::Tab::from_id(id) => {
            appearance::select_tab(state, hwnd, tab);
            false
        }
        110 => {
            let enabled = !state.app.borrow().settings().preferences.show_engine_names;
            change(state, |s| s.preferences.show_engine_names = enabled)?;
            set_toggle(hwnd, 110, "菜单显示名称", enabled);
            state.close_popup();
            fluent::invalidate_menu_preview(hwnd, &state.app.borrow().settings().preferences);
            false
        }
        appearance::RIGHT_ENGINE_NAME => {
            let enabled = !state
                .app
                .borrow()
                .settings()
                .preferences
                .show_search_engine_name;
            change(state, |settings| {
                settings.preferences.show_search_engine_name = enabled
            })?;
            set_toggle(
                hwnd,
                appearance::RIGHT_ENGINE_NAME as i32,
                "右侧图标显示名称",
                enabled,
            );
            state.close_popup();
            state.relayout_search_controls();
            let height =
                appearance::search_preview_height(&state.app.borrow().settings().preferences);
            fluent::update_search_preview(state, hwnd, height);
            false
        }
        color_picker::LIGHT
        | color_picker::DARK
        | color_picker::TEXT_COLOR
        | color_picker::ICON_COLOR
        | color_picker::HISTORY_LIGHT
        | color_picker::HISTORY_DARK
        | color_picker::HISTORY_HIGHLIGHT => {
            color_picker::open(state, hwnd, id)?;
            false
        }
        appearance::HISTORY_ENABLED | appearance::HISTORY_SHORTCUTS => {
            let enable_popup = id == appearance::HISTORY_ENABLED;
            let enabled = {
                let app = state.app.borrow();
                let history = &app.settings().preferences.history_popup;
                if enable_popup {
                    !history.enabled
                } else {
                    !history.show_shortcuts
                }
            };
            change(state, |settings| {
                let history = &mut settings.preferences.history_popup;
                if enable_popup {
                    history.enabled = enabled;
                } else {
                    history.show_shortcuts = enabled;
                }
            })?;
            let label = if enable_popup {
                appearance::HISTORY_ENABLED_LABEL
            } else {
                appearance::HISTORY_SHORTCUTS_LABEL
            };
            set_toggle(hwnd, id as i32, label, enabled);
            state.dismiss_history_popup();
            false
        }
        117 => {
            let enabled = !state
                .app
                .borrow()
                .settings()
                .preferences
                .multicolor_engine_icons;
            change(state, |s| s.preferences.multicolor_engine_icons = enabled)?;
            set_toggle(hwnd, 117, "使用多色图标", enabled);
            color_picker::sync_icon_availability(hwnd, enabled);
            state.close_popup();
            state.repaint_engine_button();
            let app = state.app.borrow();
            fluent::invalidate_menu_preview(hwnd, &app.settings().preferences);
            fluent::invalidate_search_preview(hwnd, &app.settings().preferences);
            false
        }
        111 => {
            let scroll = PANEL.with_borrow(|p| p.view.scroll);
            let picker = fonts::Picker::new(state, scroll)?;
            PANEL.with_borrow_mut(|p| {
                p.font_picker = Some(picker);
                p.view.scroll = 0;
            });
            display(state, hwnd);
            focus_control(hwnd, fonts::FILTER as i32);
            return Ok(());
        }
        112 => {
            change(state, |s| {
                s.preferences.font = Font::Default;
                s.preferences.search_font_size = yisou::settings::DEFAULT_SEARCH_FONT_SIZE;
                s.preferences.search_font_face = Default::default();
                s.preferences.font_preview_script = Default::default();
            })?;
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
            change(state, |s| {
                s.preferences.font = Font::File(path);
                s.preferences.search_font_face = Default::default();
            })?;
            state.file_font.replace(Some(font));
            state.apply_font();
            true
        }
        fonts::APPLY | fonts::CANCEL => {
            if id == fonts::APPLY {
                fonts::apply(state, hwnd)?;
            }
            PANEL.with_borrow_mut(|p| {
                if let Some(picker) = p.font_picker.take() {
                    p.view.scroll = picker.previous_scroll;
                }
            });
            display(state, hwnd);
            focus_control(hwnd, 111);
            return Ok(());
        }
        fonts::MORE => {
            fonts::more(hwnd)?;
            false
        }
        fonts::DECREASE | fonts::INCREASE => {
            fonts::step(hwnd, id);
            false
        }
        id if let Some(stepper) = appearance::search_steppers()
            .find(|stepper| id == stepper.decrease || id == stepper.increase) =>
        {
            let input = unsafe { GetDlgItem(hwnd, stepper.input as i32) };
            let value =
                search_setting_value(&state.app.borrow().settings().preferences, stepper.input);
            set_field(
                hwnd,
                stepper.input as i32,
                &stepper.stepped(value, id).to_string(),
            );
            finish_search_number(state, hwnd, input, true);
            false
        }
        pages::ACCENT_HELP => {
            message(
                hwnd,
                tr(
                    "强调色用于无图标时的首字母底色，也会替换导入 SVG 的 currentColor。单色图标的配色可以单独设置。",
                ),
            );
            false
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
                p.engine_scroll_before_edit = p.view.scroll;
                p.view.scroll = 0;
            });
            true
        }
        124 => {
            let engine = selected()?;
            let prompt = if state.app.borrow().settings().preferences.default_engine == engine.id {
                "删除此引擎并重新选择默认引擎？"
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
            let Some(mut engine) = PANEL.with_borrow(|p| p.view.engine_icon.clone()) else {
                return Ok(());
            };
            engine.name = field(hwnd, 20);
            engine.alias = field(hwnd, 21);
            engine.url = field(hwnd, 22);
            engine.color = u32::from_str_radix(field(hwnd, 23).trim().trim_start_matches('#'), 16)
                .map_err(|e| e.to_string())?;
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
            let selected = state
                .app
                .borrow()
                .all_engines()
                .iter()
                .position(|engine| engine.id == saved);
            PANEL.with_borrow_mut(|p| {
                p.edit_engine = false;
                p.engine_draft = None;
                p.selected = selected;
                p.view.scroll = p.engine_scroll_before_edit;
            });
            state.close_popup();
            state.repaint_engine_button();
            true
        }
        119 => {
            if let Some(engine) = edited_engine(state).and_then(|engine| {
                yisou::engine::built_in_engines()
                    .into_iter()
                    .find(|built_in| built_in.id == engine.id)
            }) {
                set_field(hwnd, 20, &engine.name);
                set_field(hwnd, 21, &engine.alias);
                set_field(hwnd, 22, &engine.url);
                set_field(hwnd, 23, &format!("{:06X}", engine.color));
                PANEL.with_borrow_mut(|p| p.view.engine_icon = Some(engine));
                pages::update_icon_actions(hwnd);
                fluent::reset_images();
                unsafe { InvalidateRect(hwnd, null(), 0) };
                focus_control(hwnd, 21);
            }
            false
        }
        127 | 116 | 118 | 1320 => {
            let icon = if matches!(id, 127 | 116) {
                let Some(path) = choose_file(
                    hwnd,
                    w!("图标文件\0*.svg;*.png;*.jpg;*.jpeg;*.bmp;*.ico\0\0"),
                    false,
                ) else {
                    return Ok(());
                };
                engines::load_icon(&path)?
            } else {
                Icon::Default
            };
            PANEL.with_borrow_mut(|p| {
                if let Some(engine) = p.view.engine_icon.as_mut() {
                    if matches!(id, 127 | 118) {
                        engine.mono_icon = icon;
                    } else {
                        engine.icon = icon;
                    }
                }
            });
            pages::update_icon_actions(hwnd);
            fluent::reset_images();
            update_engine_validation(state, hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
            false
        }
        128 => {
            if !confirm_engine_exit(hwnd) {
                return Ok(());
            }
            PANEL.with_borrow_mut(|p| {
                p.edit_engine = false;
                p.engine_draft = None;
                p.view.scroll = p.engine_scroll_before_edit;
            });
            true
        }
        129 | 130 => {
            let Some(row) = select_index(hwnd) else {
                return Ok(());
            };
            let direction = if id == 129 { -1 } else { 1 };
            if let Some(target) = row.checked_add_signed(direction) {
                engines::reorder(state, hwnd, row, target)?;
            }
            false
        }
        131 => {
            let current = u32::from_str_radix(&field(hwnd, 23), 16).unwrap_or(super::paint::ACCENT);
            if let Some(color) = choose_color(hwnd, current) {
                set_field(hwnd, 23, &format!("{color:06X}"));
                fluent::reset_images();
                update_engine_validation(state, hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            false
        }
        pages::MONO_COLOR | pages::RESET_MONO_COLOR => {
            let color = if id == pages::MONO_COLOR {
                let current =
                    PANEL.with_borrow(|p| p.view.engine_icon.as_ref().and_then(|e| e.mono_color));
                let Some(color) = choose_color(hwnd, current.unwrap_or(0)) else {
                    return Ok(());
                };
                Some(color)
            } else {
                None
            };
            PANEL.with_borrow_mut(|p| {
                if let Some(engine) = p.view.engine_icon.as_mut() {
                    engine.mono_color = color;
                }
            });
            pages::update_icon_actions(hwnd);
            fluent::reset_images();
            update_engine_validation(state, hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
            false
        }
        engines::MENU_COMMAND => {
            engines::show_menu(state, hwnd);
            return Ok(());
        }
        134 => {
            let mut settings = state.app.borrow().settings().clone();
            if let Some(selection) = recorder::selection_options(state, hwnd, &settings) {
                settings.preferences.selection = Some(selection);
            }
            if let Some(draft) = PANEL.with_borrow(|p| p.exclusions.clone()) {
                settings.preferences.excluded_apps = draft;
            }
            apply_hotkeys(state, settings)?;
            PANEL.with_borrow_mut(|p| p.exclusions = None);
            return Ok(());
        }
        EXCLUSION_ADD => {
            let value = field(hwnd, 23).trim().to_ascii_lowercase();
            let mut settings = state.app.borrow().settings().clone();
            if let Some(draft) = PANEL.with_borrow(|p| p.exclusions.clone()) {
                settings.preferences.excluded_apps = draft;
            }
            settings.preferences.excluded_apps.push(value);
            settings
                .normalize()
                .map_err(|_| tr("请输入 .exe 文件名，例如 mstsc.exe。").to_owned())?;
            PANEL.with_borrow_mut(|p| p.exclusions = Some(settings.preferences.excluded_apps));
            display(state, hwnd);
            focus_control(hwnd, 23);
            return Ok(());
        }
        id if id >= EXCLUSION_REMOVE && PANEL.with_borrow(|p| p.page == 3) => {
            let current = || {
                state
                    .app
                    .borrow()
                    .settings()
                    .preferences
                    .excluded_apps
                    .clone()
            };
            PANEL.with_borrow_mut(|p| {
                let draft = p.exclusions.get_or_insert_with(current);
                let index = id - EXCLUSION_REMOVE;
                if index < draft.len() {
                    draft.remove(index);
                }
            });
            display(state, hwnd);
            focus_control(hwnd, 23);
            return Ok(());
        }
        137..=139
        | 145..=147
        | 156..=157
        | 248
        | 256
        | 537..=539
        | 556
        | 648
        | 637..=639
        | 656
        | 748 => {
            return recorder::command(state, hwnd, id);
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
            if id == 142 {
                display(state, hwnd);
                focus_control(hwnd, 20);
                return Ok(());
            }
            true
        }
        143 => {
            if let Some(path) = choose_file(hwnd, w!("应用程序\0*.exe\0\0"), false) {
                set_field(hwnd, 20, &path.display().to_string());
            }
            false
        }
        140 | 141 | 1441 => {
            let path = match if id == 1441 {
                if PANEL.with_borrow(|p| p.browser_panel == BrowserPanel::Path) {
                    141
                } else {
                    140
                }
            } else {
                id
            } {
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
            if id == 1441 {
                use yisou::application::Browser;
                platform::WindowsBrowser
                    .open("https://example.com/", path.as_deref())
                    .map_err(|error| error.to_string())?;
                return Ok(());
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
                .and_then(|index| PANEL.with_borrow(|p| p.history.record(index).cloned()))
            {
                if id == 151 {
                    state.search(record.repeat(), true);
                } else {
                    if state
                        .app
                        .borrow_mut()
                        .history()
                        .delete(record.id)
                        .map_err(|e| e.to_string())?
                    {
                        PANEL.with_borrow_mut(|p| p.history.deleted = Some(record));
                    }
                }
            }
            if id == 152 {
                let previous = select_index(hwnd).unwrap_or(0);
                history::refresh_options(state, hwnd);
                history::refresh(state, hwnd, false);
                let count = PANEL.with_borrow(|p| p.history.len());
                unsafe {
                    let list = GetDlgItem(hwnd, 10);
                    if count > 0 {
                        SendMessageW(list, LB_SETCURSEL, previous.min(count - 1), 0);
                        SetFocus(list);
                    } else {
                        SetFocus(GetDlgItem(hwnd, history::KEYWORD as i32));
                    }
                }
            }
            false
        }
        153 | 154 => {
            history::change_page(state, hwnd, id == 154);
            focus_control(hwnd, 10);
            false
        }
        1551 => {
            let record = PANEL.with_borrow(|p| p.history.deleted.clone());
            if let Some(record) = record {
                state
                    .app
                    .borrow_mut()
                    .history()
                    .restore(record)
                    .map_err(|error| error.to_string())?;
                PANEL.with_borrow_mut(|p| p.history.deleted = None);
                history::refresh_options(state, hwnd);
                history::refresh(state, hwnd, false);
            }
            false
        }
        155 => {
            history::clear(state, hwnd);
            focus_control(hwnd, history::KEYWORD as i32);
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
                    let current = state.app.borrow();
                    let before = current.settings();
                    let mut affected = Vec::new();
                    if settings.preferences != before.preferences {
                        affected.push(tr("应用首选项"));
                    }
                    if settings.engine_pack() != before.engine_pack() {
                        affected.push(tr("搜索引擎与排序"));
                    }
                    drop(current);
                    let prompt = format!(
                        "{}\n\n{}",
                        tr("导入将替换当前配置。是否继续？"),
                        if affected.is_empty() {
                            tr("没有配置差异").into()
                        } else {
                            affected.join("\n")
                        }
                    );
                    if !confirm(hwnd, &prompt) {
                        return Ok(());
                    }
                    apply_hotkeys(state, settings)?;
                    PANEL.with_borrow_mut(|panel| panel.exclusions = None);
                    state.reload_file_font();
                    state.apply_font();
                    state.refresh_search_appearance();
                    state.refresh_screen_shade();
                    let language = state.app.borrow().settings().preferences.language;
                    PANEL.with_borrow_mut(|panel| panel.english = english_selected(language));
                    reload_navigation(hwnd, 6);
                }
                162 => state
                    .app
                    .borrow()
                    .export_settings(&path)
                    .map_err(|e| e.to_string())?,
                163 => {
                    let pack =
                        yisou::storage::import_engines(&path).map_err(|error| error.to_string())?;
                    if !confirm(hwnd, "导入会合并自定义引擎与置顶顺序。是否继续？")
                    {
                        return Ok(());
                    }
                    state
                        .app
                        .borrow_mut()
                        .merge_engine_pack(pack)
                        .map_err(result)?;
                }
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
            scrollbar::wake_page(hwnd);
            0
        }
        WM_GETMINMAXINFO if lp != 0 => unsafe {
            let dpi = GetDpiForWindow(hwnd).max(96);
            let limits = &mut *(lp as *mut MINMAXINFO);
            limits.ptMinTrackSize.x = dip(667, dpi);
            limits.ptMinTrackSize.y = dip(345, dpi);
            0
        },
        WM_SIZE if state.settings_window.get() == hwnd => {
            menu::dismiss();
            if wp != SIZE_MINIMIZED as usize {
                fluent::layout(hwnd);
            }
            0
        }
        WM_MOVE if state.settings_window.get() == hwnd => {
            menu::dismiss();
            0
        }
        WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
            recorder::deactivate(&state, hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_MOUSEWHEEL | WM_VSCROLL => {
            fluent::scroll(hwnd, msg, wp);
            0
        }
        BACK => {
            if recorder::cancel(&state, hwnd) {
                return 0;
            }
            let cancel = PANEL.with_borrow(|p| {
                if p.font_picker.is_some() {
                    fonts::CANCEL
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
        HISTORY_CHANGED => {
            if PANEL.with_borrow(|p| p.page == 5) {
                history::refresh_options(&state, hwnd);
                history::refresh(&state, hwnd, false);
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
        ENGINE_TOGGLE => {
            if PANEL.with_borrow(|p| p.page == 2 && !p.edit_engine)
                && let Err(error) = engines::toggle(&state, hwnd)
            {
                message(hwnd, tr(&error));
            }
            0
        }
        WM_TIMER if wp == engines::DRAG_TIMER => {
            engines::drag_scroll(hwnd);
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
        WM_COMMAND if wp & 0xffff == 10 && (wp >> 16) as u32 == LBN_SELCHANGE => {
            if PANEL.with_borrow(|p| p.page == 4) {
                let applied = state.app.borrow().settings().preferences.browser.clone();
                let changed = select_index(hwnd).is_some_and(|index| {
                    if index == 0 {
                        applied.is_some()
                    } else {
                        PANEL.with_borrow(|p| {
                            p.browsers
                                .get(index - 1)
                                .is_some_and(|b| Some(&b.path) != applied.as_ref())
                        })
                    }
                });
                if changed && PANEL.with_borrow(|p| p.browser_panel == BrowserPanel::Path) {
                    let selected = select_index(hwnd).unwrap();
                    PANEL.with_borrow_mut(|p| p.browser_panel = BrowserPanel::Closed);
                    display(&state, hwnd);
                    unsafe { SendMessageW(GetDlgItem(hwnd, 10), LB_SETCURSEL, selected, 0) };
                    focus_control(hwnd, 10);
                }
                unsafe { EnableWindow(GetDlgItem(hwnd, 140), i32::from(changed)) };
            }
            0
        }
        WM_COMMAND if wp & 0xffff == 10 && (wp >> 16) as u32 == LBN_DBLCLK => {
            let page = PANEL.with_borrow(|p| p.page);
            if page == 2 && engines::selected_index(hwnd).is_some() {
                unsafe { PostMessageW(hwnd, WM_COMMAND, 123, 0) };
            } else if page == 4 {
                unsafe { PostMessageW(hwnd, WM_COMMAND, 140, 0) };
            }
            0
        }
        WM_COMMAND
            if matches!(
                wp & 0xffff,
                103 | 135
                    | 249
                    | 136
                    | fonts::FAMILY
                    | fonts::FACE
                    | fonts::SCRIPT
                    | history::ENGINE
                    | history::SOURCE
                    | appearance::SEARCH_MATERIAL
                    | appearance::SEARCH_TONE
                    | appearance::SCREEN_BACKGROUND
                    | appearance::HISTORY_MATERIAL
                    | appearance::HISTORY_TONE
            ) && lp != 0 =>
        unsafe {
            let code = (wp >> 16) as u32;
            dropdown::notice(lp as HWND, code);
            let id = wp & 0xffff;
            if matches!(
                id,
                appearance::SEARCH_MATERIAL
                    | appearance::SEARCH_TONE
                    | appearance::SCREEN_BACKGROUND
                    | appearance::HISTORY_MATERIAL
                    | appearance::HISTORY_TONE
            ) && (code == CBN_CLOSEUP || code == CBN_SELCHANGE && !dropdown::dropped(lp as HWND))
                && let Err(error) = set_search_appearance(&state, hwnd, id, lp as HWND)
            {
                message(hwnd, tr(&error));
            }
            if matches!(id, fonts::FAMILY | fonts::FACE | fonts::SCRIPT)
                && (code == CBN_CLOSEUP || code == CBN_SELCHANGE && !dropdown::dropped(lp as HWND))
            {
                fonts::selection(hwnd, id);
            }
            if matches!(id, history::ENGINE | history::SOURCE)
                && (code == CBN_CLOSEUP || code == CBN_SELCHANGE && !dropdown::dropped(lp as HWND))
            {
                let changed = history::selection_changed(hwnd);
                if code == CBN_CLOSEUP {
                    history::finish_pending_options(&state, hwnd);
                }
                if changed {
                    KillTimer(hwnd, HISTORY_FILTER);
                    history::refresh(&state, hwnd, true);
                }
            }
            if id == 249
                && (code == CBN_CLOSEUP || code == CBN_SELCHANGE && !dropdown::dropped(lp as HWND))
            {
                use yisou::settings::ImageSearchAction::*;
                let index = SendMessageW(lp as HWND, CB_GETCURSEL, 0, 0) as usize;
                if let Some(&action) = [Choose, Both, Yandex, GoogleLens].get(index)
                    && state
                        .app
                        .borrow()
                        .settings()
                        .preferences
                        .image_search_action
                        != action
                    && let Err(error) = change(&state, |settings| {
                        settings.preferences.image_search_action = action
                    })
                {
                    message(hwnd, tr(&error));
                }
            }
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
        WM_COMMAND if wp & 0xffff == fonts::FILTER && (wp >> 16) as u32 == EN_CHANGE => unsafe {
            SetTimer(hwnd, fonts::FILTER_TIMER, 100, None);
            0
        },
        WM_COMMAND if wp & 0xffff == fonts::SIZE && (wp >> 16) as u32 == EN_CHANGE => {
            fonts::update_preview(hwnd);
            0
        }
        WM_TIMER if wp == fonts::FILTER_TIMER => unsafe {
            KillTimer(hwnd, fonts::FILTER_TIMER);
            fonts::filter(hwnd);
            0
        },
        WM_COMMAND
            if search_number_input(wp & 0xffff) && (wp >> 16) as u32 == EN_KILLFOCUS && lp != 0 =>
        {
            finish_search_number(&state, hwnd, lp as HWND, true);
            0
        }
        WM_COMMAND if wp & 0xffff == history::KEYWORD && (wp >> 16) as u32 == EN_CHANGE => unsafe {
            SetTimer(hwnd, HISTORY_FILTER, 180, None);
            0
        },
        WM_COMMAND if wp & 0xffff == 27 && (wp >> 16) as u32 == EN_CHANGE => unsafe {
            SetTimer(hwnd, ENGINE_FILTER, 100, None);
            0
        },
        WM_COMMAND if wp & 0xffff == 20 && (wp >> 16) as u32 == EN_CHANGE => {
            if PANEL.with_borrow(|p| p.edit_engine) {
                update_engine_validation(&state, hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            0
        }
        WM_COMMAND if matches!(wp & 0xffff, 21 | 22) && (wp >> 16) as u32 == EN_CHANGE => {
            if PANEL.with_borrow(|p| p.edit_engine) {
                update_engine_validation(&state, hwnd);
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
        WM_TIMER if wp == recorder::TIMER => {
            recorder::tick(hwnd);
            0
        }
        WM_TIMER if wp == fluent::CONTROL_STATE_TIMER => {
            fluent::tick_control_state(hwnd);
            0
        }
        WM_TIMER if wp == fluent::HOVER_TIMER => {
            fluent::tick_hover(hwnd);
            0
        }
        WM_TIMER if wp == HISTORY_FILTER => unsafe {
            KillTimer(hwnd, HISTORY_FILTER);
            if PANEL.with_borrow(|p| p.page == 5) {
                history::refresh(&state, hwnd, true);
            }
            0
        },
        WM_COMMAND if wp & 0xffff >= 100 && (wp >> 16) as u32 == BN_CLICKED => {
            if let Err(error) = execute(&state, hwnd, wp & 0xffff) {
                let id = wp & 0xffff;
                let inline = if matches!(id, 116 | 125..=127) {
                    pages::EDIT_STATUS
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
            let font_picker = PANEL.with_borrow(|p| p.font_picker.is_some());
            let font = if !font_picker && PANEL.with_borrow(|p| p.page == 1) {
                state.create_ui_font(wp as u32 & 0xffff)
            } else {
                null_mut()
            };
            let old = PANEL.with_borrow_mut(|p| std::mem::replace(&mut p.preview_font, font));
            if font_picker {
                fonts::update_preview(hwnd);
            }
            fluent::layout(hwnd);
            if !old.is_null() {
                DeleteObject(old);
            }
            0
        },
        WM_CLOSE => unsafe {
            let picker = color_picker::window();
            if !picker.is_null() {
                SendMessageW(picker, WM_CLOSE, 0, 0);
            } else if confirm_engine_exit(hwnd) {
                DestroyWindow(hwnd);
            }
            0
        },
        WM_DESTROY => {
            fluent::clear_control_motions(hwnd);
            recorder::reset(&state, hwnd);
            state.settings_window.set(null_mut());
            PANEL.with_borrow_mut(|p| *p = Panel::default());
            state.app.borrow_mut().history().release_cache();
            state.diagnostics.sample("settings_closed");
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
