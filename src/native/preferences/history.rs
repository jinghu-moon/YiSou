use super::{
    components::{
        BUTTON_GAP, BUTTON_ICON_SPACE, BUTTON_PADDING, CARD_GAP, CONTROL_HEIGHT, FIRST_CARD, Page,
        ROW_WITH_DETAIL, WIDTH, button_width,
    },
    fluent::{self, ButtonState, ButtonStyle, Palette},
    *,
};
use crate::native::paint::{HistoryText, Label, Painter, rect};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};
use yisou::application::Record;

pub(super) const KEYWORD: usize = 25;
pub(super) const SOURCE: usize = 26;
pub(super) const ENGINE: usize = 29;
const EMPTY: i32 = 1560;
const SUMMARY: i32 = 1561;
const FILTER_TOP: i32 = FIRST_CARD + ROW_WITH_DETAIL + 20;
pub(super) const TOP: i32 = FILTER_TOP + CONTROL_HEIGHT + 18;
const CLASS: *const u16 = w!("YiSou.Rust.Settings.History");
const TEXT_X: f32 = 16.0;
const TEXT_TOP: i32 = 10;
const DETAIL_HEIGHT: i32 = 20;
const DETAIL_GAP: i32 = 4;
const CARD_PADDING_BOTTOM: i32 = 8;
const HEADING_HEIGHT: i32 = 30;
const GROUP_GAP: i32 = 16;
const PAGE_SIZE: usize = 24;
const DELETE_RIGHT: i32 = 8;
const REPEAT_RIGHT: i32 = DELETE_RIGHT + CONTROL_HEIGHT + BUTTON_GAP;
const SEARCH: (&str, &[u8]) = (
    "search",
    include_bytes!("../../../assets/icons/tabler/search.svg"),
);
const TRASH: (&str, &[u8]) = (
    "trash",
    include_bytes!("../../../assets/icons/tabler/trash.svg"),
);

#[derive(Default)]
pub(super) struct View {
    keyword: String,
    engine: Option<String>,
    source: Option<String>,
    engines: Vec<String>,
    sources: Vec<String>,
    rows: Vec<Row>,
    page: usize,
    pages: usize,
    repeat_width: i32,
    layout_width: Option<i32>,
    scroll_anchor: Option<(u64, i32)>,
    selected: Option<u64>,
    pressed: Option<(usize, usize)>,
    options_dirty: bool,
    pub(super) deleted: Option<Record>,
}

struct Row {
    record: Record,
    detail: String,
    heading: Option<(String, String)>,
    top: i32,
    bottom: i32,
    text_height: i32,
    text: Option<HistoryText>,
}

impl Row {
    fn heading_height(&self, index: usize) -> i32 {
        if self.heading.is_none() {
            return 0;
        }
        HEADING_HEIGHT + if index == 0 { 0 } else { GROUP_GAP }
    }
}

impl View {
    pub(super) fn release_rows(&mut self) {
        self.rows = Vec::new();
        self.layout_width = None;
        self.scroll_anchor = None;
        self.pressed = None;
        self.engines = Vec::new();
        self.sources = Vec::new();
    }

    pub(super) fn record(&self, index: usize) -> Option<&Record> {
        self.rows.get(index).map(|row| &row.record)
    }

    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }
}

pub(super) fn register_window() -> io::Result<()> {
    register(CLASS, Some(proc))
}

pub(super) fn build(state: &State, hwnd: HWND, enabled: bool) {
    let page = Page(hwnd);
    page.section("", FIRST_CARD, FIRST_CARD + ROW_WITH_DETAIL, &[]);
    page.setting_with_icon(
        (
            "history",
            include_bytes!("../../../assets/icons/tabler/history.svg"),
        ),
        "记录搜索历史",
        "关闭后仍可查看已有记录。",
        FIRST_CARD,
        70,
    );
    page.toggle(150, "记录搜索历史", enabled, FIRST_CARD, ROW_WITH_DETAIL);

    const ENGINE_WIDTH: i32 = 128;
    const SOURCE_WIDTH: i32 = 156;
    const ENGINE_X: i32 = WIDTH - SOURCE_WIDTH - BUTTON_GAP - ENGINE_WIDTH;
    let keyword = PANEL.with_borrow(|p| p.history.keyword.clone());
    let input = edit_at(
        hwnd,
        KEYWORD,
        &keyword,
        place(
            8,
            FILTER_TOP,
            ENGINE_X - BUTTON_GAP - 38,
            CONTROL_HEIGHT,
            STRETCH,
        )
        .trailing_icon(SEARCH),
    );
    cue(input, "在历史记录中查找");
    combo(
        hwnd,
        ENGINE,
        &["全部引擎"],
        0,
        place(ENGINE_X, FILTER_TOP, ENGINE_WIDTH, CONTROL_HEIGHT, RIGHT),
    );
    combo(
        hwnd,
        SOURCE,
        &["全部来源"],
        0,
        place(
            WIDTH - SOURCE_WIDTH,
            FILTER_TOP,
            SOURCE_WIDTH,
            CONTROL_HEIGHT,
            RIGHT,
        ),
    );
    child(
        hwnd,
        CLASS,
        tr("搜索历史"),
        WS_TABSTOP,
        10,
        place(0, TOP, WIDTH, 1, STRETCH),
    );
    let empty = page.body(
        "暂无搜索记录",
        place(16, TOP + 14, WIDTH - 32, CONTROL_HEIGHT, STRETCH),
    );
    unsafe { SetWindowLongPtrW(empty, GWLP_ID, EMPTY as isize) };
    let summary = page.detail("", place(2, TOP, 150, CONTROL_HEIGHT, 0));
    unsafe { SetWindowLongPtrW(summary, GWLP_ID, SUMMARY as isize) };
    page.buttons(
        WIDTH,
        TOP,
        RIGHT,
        &[
            (155, "清除筛选"),
            (153, "上一页"),
            (154, "下一页"),
            (1551, "撤销删除"),
        ],
    );
    let repeat_width = button_width("再次搜索", Label::Hint) + BUTTON_ICON_SPACE;
    PANEL.with_borrow_mut(|p| p.history.repeat_width = repeat_width);
    refresh_options(state, hwnd);
    refresh(state, hwnd, false);
}

pub(super) fn refresh_options(state: &State, hwnd: HWND) {
    if [ENGINE, SOURCE]
        .into_iter()
        .any(|id| unsafe { dropdown::dropped(GetDlgItem(hwnd, id as i32)) })
    {
        PANEL.with_borrow_mut(|p| p.history.options_dirty = true);
        return;
    }
    let (engines, sources) = {
        let mut app = state.app.borrow_mut();
        let records = app.history().records().unwrap_or_default();
        let mut engines = BTreeMap::new();
        let mut sources = BTreeSet::new();
        for record in records {
            engines
                .entry(record.engine_id.clone())
                .or_insert_with(|| record.engine_name.clone());
            sources.insert(record.source.clone());
        }
        let mut engines: Vec<_> = engines.into_iter().collect();
        engines.sort_by_key(|(_, name)| name.to_lowercase());
        (engines, sources)
    };
    let (engine, source) =
        PANEL.with_borrow(|p| (p.history.engine.clone(), p.history.source.clone()));
    replace_options(
        hwnd,
        ENGINE,
        "全部引擎",
        engines
            .iter()
            .map(|(id, name)| (id.as_str(), name.as_str())),
        engine.as_deref(),
    );
    replace_options(
        hwnd,
        SOURCE,
        "全部来源",
        sources.iter().map(|source| {
            (
                source.as_str(),
                if source.is_empty() {
                    tr("无来源")
                } else {
                    source
                },
            )
        }),
        source.as_deref(),
    );
    PANEL.with_borrow_mut(|p| {
        p.history.options_dirty = false;
        p.history.engines = engines.into_iter().map(|(id, _)| id).collect();
        p.history.sources = sources.into_iter().collect();
    });
    selection_changed(hwnd);
}

fn replace_options<'a>(
    hwnd: HWND,
    id: usize,
    all: &str,
    options: impl Iterator<Item = (&'a str, &'a str)>,
    selected: Option<&str>,
) {
    unsafe {
        let combo = GetDlgItem(hwnd, id as i32);
        SendMessageW(combo, CB_RESETCONTENT, 0, 0);
        SendMessageW(combo, CB_ADDSTRING, 0, wide(tr(all)).as_ptr() as isize);
        let mut index = 0;
        for (key, label) in options {
            let item = SendMessageW(combo, CB_ADDSTRING, 0, wide(label).as_ptr() as isize);
            if Some(key) == selected && item >= 0 {
                index = item as usize;
            }
        }
        SendMessageW(combo, CB_SETCURSEL, index, 0);
    }
}

pub(super) fn finish_pending_options(state: &State, hwnd: HWND) {
    if PANEL.with_borrow(|p| p.history.options_dirty) {
        refresh_options(state, hwnd);
    }
}

pub(super) fn selection_changed(hwnd: HWND) -> bool {
    let read = |id| unsafe {
        usize::try_from(SendMessageW(GetDlgItem(hwnd, id), CB_GETCURSEL, 0, 0))
            .ok()
            .and_then(|index| index.checked_sub(1))
    };
    let (engine, source) = (read(ENGINE as i32), read(SOURCE as i32));
    PANEL.with_borrow_mut(|p| {
        let view = &mut p.history;
        let engine = engine.and_then(|index| view.engines.get(index)).cloned();
        let source = source.and_then(|index| view.sources.get(index)).cloned();
        let changed = view.engine != engine || view.source != source;
        view.engine = engine;
        view.source = source;
        changed
    })
}

pub(super) fn clear(state: &State, hwnd: HWND) {
    set_field(hwnd, KEYWORD as i32, "");
    unsafe {
        for id in [ENGINE, SOURCE] {
            SendMessageW(GetDlgItem(hwnd, id as i32), CB_SETCURSEL, 0, 0);
        }
        KillTimer(hwnd, HISTORY_FILTER);
    }
    selection_changed(hwnd);
    refresh(state, hwnd, true);
}

pub(super) fn change_page(state: &State, hwnd: HWND, next: bool) {
    PANEL.with_borrow_mut(|p| {
        let view = &mut p.history;
        view.selected = None;
        view.page = if next {
            (view.page + 1).min(view.pages.saturating_sub(1))
        } else {
            view.page.saturating_sub(1)
        };
        p.view.scroll = 0;
    });
    refresh(state, hwnd, false);
}

pub(super) fn refresh(state: &State, hwnd: HWND, reset: bool) {
    let keyword = field(hwnd, KEYWORD);
    let (engine, source, page, english) = PANEL.with_borrow(|p| {
        (
            p.history.engine.clone(),
            p.history.source.clone(),
            if reset { 0 } else { p.history.page },
            p.english,
        )
    });
    let needle = keyword.trim().to_lowercase();
    let filtered = !needle.is_empty() || engine.is_some() || source.is_some();
    let (records, count, page, pages) = {
        let mut app = state.app.borrow_mut();
        let mut records: Vec<_> = app
            .history()
            .records()
            .unwrap_or_default()
            .iter()
            .filter(|record| {
                engine.as_ref().is_none_or(|id| record.engine_id == *id)
                    && source.as_ref().is_none_or(|name| record.source == *name)
                    && (needle.is_empty() || record.query.to_lowercase().contains(&needle))
            })
            .collect();
        records.sort_by_key(|record| std::cmp::Reverse(record.timestamp_ms));
        let count = records.len();
        let pages = count.div_ceil(PAGE_SIZE);
        let page = page.min(pages.saturating_sub(1));
        let records = records
            .into_iter()
            .skip(page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .cloned()
            .collect();
        (records, count, page, pages)
    };
    let rows = group_rows(records, english);
    let empty = rows.is_empty();
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    PANEL.with_borrow_mut(|p| {
        let view = &mut p.history;
        let scroll = p.view.scroll;
        view.scroll_anchor = if !reset && page == view.page && scroll >= dip(TOP, dpi) {
            view.rows
                .iter()
                .find(|row| dip(row.bottom, dpi) > scroll)
                .map(|row| (row.record.id, scroll - dip(row.top, dpi)))
        } else {
            None
        };
        view.keyword = keyword;
        view.page = page;
        view.pages = pages;
        view.rows = rows;
        view.layout_width = None;
        view.pressed = None;
        if reset
            || !view
                .rows
                .iter()
                .any(|row| Some(row.record.id) == view.selected)
        {
            view.selected = None;
        }
        if reset {
            p.view.scroll = 0;
        }
    });
    let summary = if english {
        format!("{count} {}", if count == 1 { "entry" } else { "entries" })
    } else {
        format!("共 {count} 条")
    };
    let summary = if pages > 1 {
        format!("{summary}  ·  {} / {pages}", page + 1)
    } else {
        summary
    };
    set_label(hwnd, SUMMARY, &summary);
    set_label(
        hwnd,
        EMPTY,
        if filtered {
            "没有符合筛选条件的记录"
        } else {
            "暂无搜索记录"
        },
    );
    unsafe {
        let list = GetDlgItem(hwnd, 10);
        fluent::clear_control_hover(list);
        for (id, show) in [
            (10, !empty),
            (EMPTY, empty),
            (155, filtered),
            (153, pages > 1),
            (154, pages > 1),
            (1551, PANEL.with_borrow(|p| p.history.deleted.is_some())),
        ] {
            ShowWindow(GetDlgItem(hwnd, id), if show { SW_SHOWNA } else { SW_HIDE });
        }
        EnableWindow(GetDlgItem(hwnd, 153), i32::from(page > 0));
        EnableWindow(GetDlgItem(hwnd, 154), i32::from(page + 1 < pages));
        if IsWindowVisible(hwnd) != 0 {
            fluent::layout(hwnd);
        }
    }
}

pub(super) fn layout_rows(width: i32, dpi: u32) {
    PANEL.with_borrow_mut(|p| {
        if p.history.layout_width == Some(width) {
            return;
        }
        p.history.layout_width = Some(width);
        let mut painter = p.view.settings_painter();
        let keyword = &p.history.keyword;
        let minimum = painter
            .as_deref_mut()
            .and_then(|painter| painter.wrapped_text("国\n国", 64.0))
            .map_or(38, |(_, height)| height.ceil() as i32);
        let repeat_left = width - REPEAT_RIGHT - p.history.repeat_width;
        let text_width = (repeat_left as f32 - TEXT_X - 12.0).max(1.0);
        let mut top = TOP;
        for (index, row) in p.history.rows.iter_mut().enumerate() {
            let wrapped = painter.as_deref_mut().and_then(|painter| {
                HistoryText::new(painter, &row.record.query, text_width, keyword)
            });
            row.text_height = wrapped
                .as_ref()
                .map_or(minimum, |text| text.height.max(minimum));
            row.text = wrapped;
            row.top = top;
            top = top + row.heading_height(index) + card_height(row.text_height) + CARD_GAP;
            row.bottom = top;
        }
        if let Some((id, offset)) = p.history.scroll_anchor.take()
            && let Some(row) = p.history.rows.iter().find(|row| row.record.id == id)
        {
            p.view.scroll = dip(row.top, dpi) + offset.min(dip(row.bottom - row.top, dpi) - 1);
        }
        let height = (top - TOP).max(ROW_WITH_DETAIL);
        let footer = TOP + height + 10;
        p.view.extent = footer + CONTROL_HEIGHT + 20;
        for (child, area) in &mut p.view.controls {
            match unsafe { GetDlgCtrlID(*child) } {
                10 => *area = place(0, TOP, WIDTH, height, STRETCH),
                153..=155 | 1551 | SUMMARY => area.move_to_y(footer),
                _ => {}
            }
        }
    });
}

fn card_height(text_height: i32) -> i32 {
    TEXT_TOP + text_height + DETAIL_GAP + DETAIL_HEIGHT + CARD_PADDING_BOTTOM
}

fn local_time(timestamp: u64) -> Option<SYSTEMTIME> {
    let ticks = timestamp
        .checked_mul(10_000)?
        .checked_add(116_444_736_000_000_000)?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    (unsafe { FileTimeToSystemTime(&file, &mut utc) } != 0
        && unsafe { SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) } != 0)
        .then_some(local)
}

fn calendar_day(time: SYSTEMTIME) -> Option<u64> {
    let mut file = FILETIME::default();
    (unsafe { SystemTimeToFileTime(&time, &mut file) } != 0).then(|| {
        ((u64::from(file.dwHighDateTime) << 32) | u64::from(file.dwLowDateTime)) / 864_000_000_000
    })
}

fn group_rows(records: Vec<Record>, english: bool) -> Vec<Row> {
    let today = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| local_time(time.as_millis() as u64));
    let today_day = today.and_then(calendar_day);
    let dates: Vec<_> = records
        .iter()
        .map(|record| local_time(record.timestamp_ms))
        .collect();
    let mut index = 0;
    let mut rows = Vec::with_capacity(records.len());
    for record in records {
        let date = dates[index];
        let day = date.and_then(calendar_day);
        let start = index == 0 || dates[index - 1].and_then(calendar_day) != day;
        let heading = start.then(|| {
            let count = dates[index..]
                .iter()
                .take_while(|date| date.and_then(calendar_day) == day)
                .count();
            let title = match date {
                Some(_) if day == today_day && day.is_some() => tr("今天").to_owned(),
                Some(_) if day.is_some() && day == today_day.and_then(|day| day.checked_sub(1)) => {
                    tr("昨天").to_owned()
                }
                Some(date) if today.is_some_and(|now| now.wYear == date.wYear) && !english => {
                    format!("{}月{}日", date.wMonth, date.wDay)
                }
                Some(date) if today.is_some_and(|now| now.wYear == date.wYear) => {
                    format!("{}/{}", date.wMonth, date.wDay)
                }
                Some(date) => format!("{:04}-{:02}-{:02}", date.wYear, date.wMonth, date.wDay),
                None => tr("日期未知").to_owned(),
            };
            let count = if english {
                format!("{count} {}", if count == 1 { "entry" } else { "entries" })
            } else {
                format!("{count} 条")
            };
            (title, count)
        });
        let mut detail = record.source.clone();
        if let Some(time) = date {
            if !detail.is_empty() {
                detail.push_str("  ·  ");
            }
            detail.push_str(&format!("{:02}:{:02}", time.wHour, time.wMinute));
        }
        rows.push(Row {
            record,
            detail,
            heading,
            top: 0,
            bottom: 0,
            text_height: 0,
            text: None,
        });
        index += 1;
    }
    rows
}

fn action_bounds(
    width: f32,
    row_top: f32,
    repeat_width: i32,
) -> [windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F; 2] {
    let top = row_top + 15.0;
    let height = CONTROL_HEIGHT as f32;
    [
        rect(
            width - (REPEAT_RIGHT + repeat_width) as f32,
            top,
            repeat_width as f32,
            height,
        ),
        rect(
            width - (DELETE_RIGHT + CONTROL_HEIGHT) as f32,
            top,
            height,
            height,
        ),
    ]
}

fn list_origin(dpi: u32, scroll: i32) -> i32 {
    dip(TOP, dpi).max(scroll)
}

fn row_at(list: HWND, point: LPARAM) -> Option<usize> {
    let mut bounds = RECT::default();
    let cursor = POINT {
        x: i32::from(point as i16),
        y: i32::from((point >> 16) as i16),
    };
    unsafe {
        GetClientRect(list, &mut bounds);
        if PtInRect(&bounds, cursor) == 0 {
            return None;
        }
    }
    let dpi = unsafe { GetDpiForWindow(list) }.max(96);
    PANEL.with_borrow(|p| {
        let y = cursor.y + list_origin(dpi, p.view.scroll);
        let row = p
            .history
            .rows
            .partition_point(|row| dip(row.bottom, dpi) <= y);
        (row < p.history.rows.len()).then_some(row)
    })
}

pub(super) fn action_at(list: HWND, point: LPARAM) -> Option<(usize, usize)> {
    let index = row_at(list, point)?;
    let mut bounds = RECT::default();
    let dpi = unsafe {
        GetClientRect(list, &mut bounds);
        GetDpiForWindow(list).max(96)
    };
    let unit = 96.0 / dpi as f32;
    let x = i32::from(point as i16) as f32 * unit;
    PANEL.with_borrow(|p| {
        let row = &p.history.rows[index];
        let top = dip(row.top, dpi) - list_origin(dpi, p.view.scroll);
        let y = (i32::from((point >> 16) as i16) - top) as f32 * unit;
        action_bounds(
            bounds.right as f32 * unit,
            row.heading_height(index) as f32,
            p.history.repeat_width,
        )
        .iter()
        .position(|area| {
            (area.left..area.right).contains(&x) && (area.top..area.bottom).contains(&y)
        })
        .map(|action| (index, action))
    })
}

fn selected_index(view: &View) -> Option<usize> {
    view.rows
        .iter()
        .position(|row| Some(row.record.id) == view.selected)
}

fn select(list: HWND, index: usize) -> isize {
    let selected = PANEL.with_borrow_mut(|p| {
        let selected = p.history.rows.get(index).map(|row| row.record.id);
        p.history.selected = selected;
        selected
    });
    unsafe { InvalidateRect(list, null(), 0) };
    selected.map_or(LB_ERR as isize, |_| index as isize)
}

unsafe extern "system" fn proc(list: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT | WM_PRINTCLIENT => {
                let mut paint = PAINTSTRUCT::default();
                let mut bounds = RECT::default();
                GetClientRect(list, &mut bounds);
                let dc = if msg == WM_PAINT {
                    let dc = BeginPaint(list, &mut paint);
                    bounds.top = paint.rcPaint.top;
                    bounds.bottom = paint.rcPaint.bottom;
                    dc
                } else {
                    wp as HDC
                };
                if let Some(state) = crate::native::session() {
                    paint_list(&state, list, dc, bounds);
                }
                if msg == WM_PAINT {
                    EndPaint(list, &paint);
                }
                0
            }
            WM_GETDLGCODE => DLGC_WANTARROWS as isize,
            WM_SETFOCUS | WM_KILLFOCUS | WM_UPDATEUISTATE => {
                InvalidateRect(list, null(), 0);
                DefWindowProcW(list, msg, wp, lp)
            }
            WM_KEYDOWN
                if matches!(
                    wp as u16,
                    VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT
                ) =>
            {
                let target = PANEL.with_borrow(|p| {
                    let view = &p.history;
                    let last = view.rows.len().checked_sub(1)?;
                    let selected = selected_index(view);
                    let index = match wp as u16 {
                        VK_HOME => 0,
                        VK_END => last,
                        VK_UP | VK_PRIOR => selected.map_or(0, |index| index.saturating_sub(1)),
                        _ => selected.map_or(0, |index| (index + 1).min(last)),
                    };
                    Some(index)
                });
                if let Some(target) = target {
                    select(list, target);
                }
                0
            }
            WM_LBUTTONDOWN => {
                SetFocus(list);
                if let Some(index) = row_at(list, lp) {
                    select(list, index);
                }
                if let Some(action) = action_at(list, lp) {
                    PANEL.with_borrow_mut(|p| p.history.pressed = Some(action));
                    SetCapture(list);
                }
                0
            }
            WM_LBUTTONUP => {
                let pressed = PANEL.with_borrow_mut(|p| p.history.pressed.take());
                if GetCapture() == list {
                    ReleaseCapture();
                }
                if let Some((index, action)) =
                    pressed.filter(|pressed| Some(*pressed) == action_at(list, lp))
                {
                    select(list, index);
                    SendMessageW(GetParent(list), HISTORY_ACTION, 151 + action, 0);
                }
                0
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                PANEL.with_borrow_mut(|p| p.history.pressed = None);
                0
            }
            // Match the list messages the settings window uses for selection and row geometry.
            LB_GETCOUNT => PANEL.with_borrow(|p| p.history.rows.len() as isize),
            LB_GETCURSEL | LB_GETCARETINDEX => PANEL.with_borrow(|p| {
                selected_index(&p.history).map_or(LB_ERR as isize, |index| index as isize)
            }),
            LB_SETCURSEL => select(list, wp),
            LB_GETITEMRECT | LB_GETITEMHEIGHT => {
                let dpi = GetDpiForWindow(list).max(96);
                PANEL.with_borrow(|p| {
                    let Some(row) = p.history.rows.get(wp) else {
                        return LB_ERR as isize;
                    };
                    let top = dip(row.top, dpi);
                    let bottom = dip(row.bottom, dpi);
                    if msg == LB_GETITEMHEIGHT {
                        return (bottom - top) as isize;
                    }
                    if lp == 0 {
                        return LB_ERR as isize;
                    }
                    let origin = list_origin(dpi, p.view.scroll);
                    let bounds = &mut *(lp as *mut RECT);
                    GetClientRect(list, bounds);
                    bounds.top = top - origin;
                    bounds.bottom = bottom - origin;
                    0
                })
            }
            _ => DefWindowProcW(list, msg, wp, lp),
        }
    }
}

fn paint_list(state: &State, list: HWND, dc: HDC, bounds: RECT) {
    let (dpi, focused) = unsafe {
        (
            GetDpiForWindow(list).max(96),
            GetFocus() == list
                && SendMessageW(list, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS as isize == 0,
        )
    };
    let unit = 96.0 / dpi as f32;
    let width = bounds.right as f32 * unit;
    let colors = fluent::palette();
    fluent::with_painter(dc, bounds, dpi, |painter| {
        painter.clear(colors.background);
        PANEL.with_borrow(|p| {
            let origin = list_origin(dpi, p.view.scroll) + bounds.top;
            let history = &p.history;
            let first = history
                .rows
                .partition_point(|row| dip(row.bottom, dpi) <= origin);
            for (index, row) in history.rows.iter().enumerate().skip(first) {
                let row_top = dip(row.top, dpi) - origin;
                if row_top >= bounds.bottom - bounds.top {
                    break;
                }
                let selected = history.selected == Some(row.record.id);
                let hover = [
                    fluent::row_hover_amount(list, index, 0),
                    fluent::row_hover_amount(list, index, 1),
                ];
                paint_row(
                    painter,
                    state,
                    colors,
                    dpi,
                    width,
                    row_top as f32 * unit,
                    row,
                    index,
                    history.repeat_width,
                    selected,
                    focused && selected,
                    hover,
                );
            }
        });
    });
}

fn paint_row(
    p: &mut Painter,
    state: &State,
    colors: Palette,
    dpi: u32,
    width: f32,
    row_top: f32,
    row: &Row,
    index: usize,
    repeat_width: i32,
    selected: bool,
    focused: bool,
    hover: [f32; 2],
) {
    let app = state.app.borrow();
    let top = row_top + row.heading_height(index) as f32;
    if let Some((date, count)) = &row.heading {
        let y = top - HEADING_HEIGHT as f32;
        let date_width = p.text_width(date, Label::Body).unwrap_or(68.0);
        p.text(
            date,
            rect(2.0, y, date_width + 2.0, 24.0),
            colors.text,
            Label::Body,
        );
        let left = date_width + 12.0;
        p.text(
            count,
            rect(left, y, (width - left).max(1.0), 24.0),
            colors.detail,
            Label::Hint,
        );
    }
    let card = rect(
        0.5,
        top + 0.5,
        width - 1.0,
        card_height(row.text_height) as f32 - 1.0,
    );
    p.rounded(
        card,
        5.0,
        if selected {
            colors.selection
        } else {
            colors.card
        },
        false,
    );
    p.rounded(
        card,
        5.0,
        if focused {
            colors.accent
        } else {
            colors.border
        },
        true,
    );
    let [repeat, delete] = action_bounds(width, top, repeat_width);
    if let Some(layout) = &row.text {
        layout.draw(p, TEXT_X, top + TEXT_TOP as f32, colors.text, colors.accent);
    }
    let detail_top = top + (TEXT_TOP + row.text_height + DETAIL_GAP) as f32;
    let engine = app
        .all_engines()
        .iter()
        .find(|engine| engine.id == row.record.engine_id);
    if let Some(engine) = engine {
        state.paint_engine_variant(
            p,
            engine,
            (16.0, detail_top + 4.0),
            12,
            dpi,
            app.settings().preferences.multicolor_engine_icons,
        );
    } else {
        fluent::icon(p, SEARCH, 16.0, detail_top + 4.0, 12.0, dpi, colors.detail);
    }
    p.text(
        &row.detail,
        rect(34.0, detail_top, (repeat.left - 46.0).max(1.0), 20.0),
        colors.detail,
        Label::Hint,
    );
    for (area, hover) in [repeat, delete].into_iter().zip(hover) {
        fluent::paint_button_face(
            p,
            area,
            colors,
            ButtonStyle::Standard,
            ButtonState {
                hover,
                ..ButtonState::default()
            },
        );
    }
    let icon_y = repeat.top + (CONTROL_HEIGHT as f32 - 14.0) / 2.0;
    fluent::icon(
        p,
        SEARCH,
        repeat.left + BUTTON_PADDING as f32,
        icon_y,
        14.0,
        dpi,
        colors.detail,
    );
    let caption = BUTTON_PADDING + BUTTON_ICON_SPACE;
    p.text(
        tr("再次搜索"),
        rect(
            repeat.left + caption as f32,
            repeat.top,
            (repeat_width - caption - BUTTON_PADDING) as f32,
            CONTROL_HEIGHT as f32,
        ),
        colors.text,
        Label::Hint,
    );
    fluent::icon(
        p,
        TRASH,
        delete.left + (CONTROL_HEIGHT as f32 - 14.0) / 2.0,
        icon_y,
        14.0,
        dpi,
        colors.detail,
    );
}
