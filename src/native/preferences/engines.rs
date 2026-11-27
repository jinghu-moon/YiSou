use super::components::{CARD_GAP, CONTROL_HEIGHT, FIRST_CARD, ROW, WIDTH};
use super::{components::Page, menu::Item, *};

const FILTER: usize = 27;
pub(super) const PIN_TOP: i32 = FIRST_CARD;
const LIST_TOP: i32 = 303;
pub(super) const ROW_HEIGHT: i32 = ROW + CARD_GAP;
pub(super) const MENU_COMMAND: usize = 132;
pub(super) const DRAG_TIMER: usize = 18;

#[derive(Clone, Copy)]
pub(super) struct Drag {
    pub source: usize,
    pub target: usize,
    pub top: i32,
    offset: i32,
}

impl Drag {
    pub fn row(self, index: usize) -> usize {
        if (self.source..self.target).contains(&index) {
            index + 1
        } else if (self.target + 1..=self.source).contains(&index) {
            index - 1
        } else {
            index
        }
    }
}

pub(super) fn load_icon(path: &Path) -> Result<Icon, String> {
    if std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len()
        > 32 * 1024 * 1024
    {
        return Err("图标文件太大".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        import_svg(bytes)
    } else {
        import_bitmap(&bytes)
    }
    .ok_or_else(|| "无法读取图标".into())
}

pub(super) fn display(state: &State, hwnd: HWND) {
    let filter = PANEL.with_borrow(|panel| panel.engine_filter.clone());
    let search = edit_at(
        hwnd,
        FILTER,
        &filter,
        place(8, 222, WIDTH - 16, CONTROL_HEIGHT, STRETCH),
    );
    cue(search, "筛选名称或 #别名");
    let page = Page(hwnd);
    page.buttons(WIDTH, 23, RIGHT, &[(122, "添加引擎")]);
    page.section("置顶快捷位", PIN_TOP, PIN_TOP + 56, &[]);
    page.section_title("搜索引擎列表", 191);
    caption(
        hwnd,
        "引擎",
        place(14, LIST_TOP - 29, 100, 22, 0),
        fluent::FontRole::Detail,
    );
    caption(
        hwnd,
        "别名",
        place(239, LIST_TOP - 29, 80, 22, RIGHT),
        fluent::FontRole::Detail,
    );
    caption(
        hwnd,
        "状态",
        place(349, LIST_TOP - 29, 76, 22, RIGHT),
        fluent::FontRole::Detail,
    );
    caption(
        hwnd,
        "启用",
        place(444, LIST_TOP - 29, 46, 22, RIGHT),
        fluent::FontRole::Detail,
    );
    list_control(hwnd, place(0, LIST_TOP, WIDTH, ROW_HEIGHT, STRETCH), 0);
    refresh(state, hwnd, false);
}

pub(super) fn refresh(state: &State, hwnd: HWND, filter_changed: bool) {
    let list = unsafe { GetDlgItem(hwnd, 10) };
    if list.is_null() {
        return;
    }
    let filter = if filter_changed {
        let filter = field(hwnd, FILTER);
        if PANEL.with_borrow(|p| p.engine_filter == filter) {
            return;
        }
        menu::dismiss();
        filter
    } else {
        PANEL.with_borrow(|p| p.engine_filter.clone())
    };
    let needle = filter.trim().trim_start_matches('#').to_lowercase();
    if filter_changed {
        finish_drag(state, list, false);
    }
    let app = state.app.borrow();
    let rows: Vec<_> = app
        .all_engines()
        .iter()
        .enumerate()
        .filter_map(|(index, engine)| {
            (engine.name.to_lowercase().contains(&needle)
                || engine.alias.to_lowercase().contains(&needle))
            .then(|| {
                (
                    index,
                    format!(
                        "{}\t{}\t{}",
                        engine.name,
                        engine.alias,
                        tr(if app.settings().engine_enabled(&engine.id) {
                            "开"
                        } else {
                            "关"
                        })
                    ),
                )
            })
        })
        .collect();
    drop(app);
    PANEL.with_borrow_mut(|panel| {
        panel.engine_filter = filter;
        panel.engine_rows = rows.iter().map(|row| row.0).collect();
        let height = rows.len().max(1) as i32 * ROW_HEIGHT;
        if let Some((_, area)) = panel
            .view
            .controls
            .iter_mut()
            .find(|(child, _)| *child == list)
        {
            *area = place(0, LIST_TOP, WIDTH, height, STRETCH);
        }
        panel.view.extent = LIST_TOP + height + 16;
        if filter_changed {
            panel.view.scroll = 0;
        }
    });
    unsafe {
        let visible = IsWindowVisible(list) != 0;
        if visible {
            SendMessageW(list, WM_SETREDRAW, 0, 0);
        }
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        if rows.is_empty() {
            add_item(list, tr("没有符合条件的引擎"));
        }
        for (_, label) in rows {
            add_item(list, &label);
        }
        if visible {
            SendMessageW(list, WM_SETREDRAW, 1, 0);
            InvalidateRect(list, null(), 0);
        }
    }
    if filter_changed {
        fluent::layout(hwnd);
    }
}

pub(super) fn selected_index(hwnd: HWND) -> Option<usize> {
    let row = select_index(hwnd)?;
    PANEL.with_borrow(|p| p.engine_rows.get(row).copied())
}

pub(super) fn remember_list(hwnd: HWND) {
    let list = unsafe { GetDlgItem(hwnd, 10) };
    if !list.is_null() {
        let selected = selected_index(hwnd);
        PANEL.with_borrow_mut(|p| p.selected = selected);
    }
}

pub(super) fn restore_selection(hwnd: HWND) {
    let selected = PANEL.with_borrow(|p| {
        p.selected
            .and_then(|selected| p.engine_rows.iter().position(|&i| i == selected))
    });
    if let Some(row) = selected {
        unsafe { SendMessageW(GetDlgItem(hwnd, 10), LB_SETCURSEL, row, 0) };
    }
}

pub(super) fn show_menu(state: &State, hwnd: HWND) {
    let list = unsafe { GetDlgItem(hwnd, 10) };
    let Some(row) = select_index(hwnd) else {
        return;
    };
    let Some(index) = selected_index(hwnd) else {
        return;
    };
    if menu::toggle_existing(index) {
        return;
    }
    let (default, pinned, custom, enabled, down) = {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        let Some(engine) = app.all_engines().get(index) else {
            return;
        };
        (
            prefs.default_engine == engine.id,
            prefs.pins.contains(&engine.id),
            engine.is_custom(),
            app.settings().engine_enabled(&engine.id),
            PANEL.with_borrow(|p| row + 1 < p.engine_rows.len()),
        )
    };
    unsafe {
        let mut rect = RECT::default();
        if SendMessageW(list, LB_GETITEMRECT, row, &mut rect as *mut RECT as isize)
            == LB_ERR as isize
        {
            return;
        }
        MapWindowPoints(list, null_mut(), (&mut rect as *mut RECT).cast(), 2);
        rect.right -= dip(8, GetDpiForWindow(hwnd));
        let items = [
            Item::new(120, "设为默认", enabled && !default),
            Item::pin(pinned, enabled),
            Item::new(123, "编辑", true),
            Item::new(129, "上移", row > 0),
            Item::new(130, "下移", down),
            Item::new(124, "删除", true),
        ];
        menu::show(
            hwnd,
            rect,
            index,
            if custom { &items } else { &items[..5] },
            2,
        );
    }
}

pub(super) fn more_row(list: HWND, x: i32, y: i32) -> Option<usize> {
    row_action(list, x, y, RowAction::More)
}

pub(super) enum RowAction {
    More,
    Toggle,
    Grip,
}

pub(super) fn row_action(list: HWND, x: i32, y: i32, action: RowAction) -> Option<usize> {
    unsafe {
        let dpi = GetDpiForWindow(list);
        let mut bounds = RECT::default();
        GetClientRect(list, &mut bounds);
        let hit = match action {
            RowAction::More => bounds.right - dip(43, dpi)..bounds.right - dip(4, dpi),
            RowAction::Toggle => bounds.right - dip(101, dpi)..bounds.right - dip(48, dpi),
            RowAction::Grip => dip(5, dpi)..dip(34, dpi),
        };
        if !hit.contains(&x) || !(0..bounds.bottom).contains(&y) {
            return None;
        }
        let point = (u32::from(y as u16) << 16 | u32::from(x as u16)) as isize;
        let hit = SendMessageW(list, LB_ITEMFROMPOINT, 0, point);
        let row = hit as usize & 0xffff;
        if hit >> 16 != 0 || !PANEL.with_borrow(|p| row < p.engine_rows.len()) {
            return None;
        }
        if SendMessageW(
            list,
            LB_GETITEMRECT,
            row,
            (&mut bounds as *mut RECT) as isize,
        ) == LB_ERR as isize
        {
            return None;
        }
        (bounds.top..bounds.bottom - dip(CARD_GAP, dpi))
            .contains(&y)
            .then_some(row)
    }
}

pub(super) fn toggle(state: &State, hwnd: HWND) -> Result<(), String> {
    let Some(index) = selected_index(hwnd) else {
        return Ok(());
    };
    let (id, enabled) = {
        let app = state.app.borrow();
        let Some(engine) = app.all_engines().get(index) else {
            return Ok(());
        };
        (engine.id.clone(), app.settings().engine_enabled(&engine.id))
    };
    state
        .app
        .borrow_mut()
        .set_engine_enabled(&id, !enabled)
        .map_err(|error| match error {
            UpdateError::Settings(SettingsError::NoEnabledEngine) => {
                "至少保留一个已启用的搜索引擎。".into()
            }
            _ => error.to_string(),
        })?;
    remember_list(hwnd);
    refresh(state, hwnd, false);
    restore_selection(hwnd);
    state.close_popup();
    state.repaint_engine_button();
    unsafe { InvalidateRect(hwnd, null(), 0) };
    Ok(())
}

pub(super) fn reorder(
    state: &State,
    hwnd: HWND,
    source: usize,
    target: usize,
) -> Result<(), String> {
    let mut order = PANEL.with_borrow(|panel| {
        let app = state.app.borrow();
        panel
            .engine_rows
            .iter()
            .map(|&i| app.all_engines()[i].id.clone())
            .collect::<Vec<_>>()
    });
    if source == target || source >= order.len() || target >= order.len() {
        return Ok(());
    }
    let id = order.remove(source);
    order.insert(target, id.clone());
    state
        .app
        .borrow_mut()
        .reorder_engines(&order)
        .map_err(|error| error.to_string())?;
    let selected = state
        .app
        .borrow()
        .all_engines()
        .iter()
        .position(|e| e.id == id);
    PANEL.with_borrow_mut(|p| p.selected = selected);
    refresh(state, hwnd, false);
    restore_selection(hwnd);
    state.close_popup();
    state.repaint_engine_button();
    Ok(())
}

pub(super) fn begin_drag(list: HWND, source: usize, pointer_y: i32) {
    menu::dismiss();
    unsafe {
        SetFocus(list);
        SetCapture(list);
        let row = SendMessageW(list, LB_GETITEMHEIGHT, 0, 0).max(1) as i32;
        let top = source as i32 * row;
        PANEL.with_borrow_mut(|p| {
            p.engine_drag = Some(Drag {
                source,
                target: source,
                top,
                offset: pointer_y - top,
            })
        });
        SetTimer(GetParent(list), DRAG_TIMER, 30, None);
        InvalidateRect(list, null(), 0);
    }
}

pub(super) fn drag_to(list: HWND, y: i32) {
    let count = PANEL.with_borrow(|p| p.engine_rows.len());
    if count == 0 {
        return;
    }
    let row_height = unsafe { SendMessageW(list, LB_GETITEMHEIGHT, 0, 0) }.max(1) as i32;
    let changed = PANEL.with_borrow_mut(|p| {
        let drag = p.engine_drag.as_mut()?;
        let top = (y - drag.offset).clamp(0, (count - 1) as i32 * row_height);
        if top == drag.top {
            return None;
        }
        let previous = *drag;
        drag.top = top;
        drag.target = ((top + row_height / 2) / row_height) as usize;
        Some((previous, *drag))
    });
    if let Some((previous, drag)) = changed {
        unsafe {
            let mut dirty = RECT::default();
            GetClientRect(list, &mut dirty);
            dirty.top = previous.top.min(drag.top);
            dirty.bottom = previous.top.max(drag.top) + row_height;
            if previous.target != drag.target {
                dirty.top = dirty
                    .top
                    .min(previous.target.min(drag.target) as i32 * row_height);
                dirty.bottom = dirty
                    .bottom
                    .max((previous.target.max(drag.target) + 1) as i32 * row_height);
            }
            InvalidateRect(list, &dirty, 0);
        }
    }
}

pub(super) fn drag_scroll(hwnd: HWND) {
    if !PANEL.with_borrow(|p| p.engine_drag.is_some()) {
        return;
    }
    unsafe {
        let list = GetDlgItem(hwnd, 10);
        let mut cursor = POINT::default();
        if list.is_null() || GetCursorPos(&mut cursor) == 0 {
            return;
        }
        ScreenToClient(hwnd, &mut cursor);
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds);
        let margin = dip(38, GetDpiForWindow(hwnd));
        let (position, maximum, _) = fluent::scroll_metrics(hwnd);
        if (0..bounds.right).contains(&cursor.x)
            && (cursor.y < margin || cursor.y > bounds.bottom - margin)
        {
            let next = (position + if cursor.y < margin { -14 } else { 14 }).clamp(0, maximum);
            if next == position {
                return;
            }
            fluent::scroll_to(hwnd, next);
            MapWindowPoints(hwnd, list, &mut cursor, 1);
            drag_to(list, cursor.y);
        }
    }
}

pub(super) fn finish_drag(state: &State, list: HWND, commit: bool) {
    let Some(drag) = PANEL.with_borrow_mut(|p| p.engine_drag.take()) else {
        return;
    };
    unsafe {
        let hwnd = GetParent(list);
        KillTimer(hwnd, DRAG_TIMER);
        if GetCapture() == list {
            ReleaseCapture();
        }
        if commit && let Err(error) = reorder(state, hwnd, drag.source, drag.target) {
            message(hwnd, &error);
        }
        InvalidateRect(list, null(), 0);
    }
}
