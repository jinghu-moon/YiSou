use super::{components::Page, menu::Item, *};

const FILTER: usize = 27;
pub(super) const MENU_COMMAND: usize = 132;

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

pub(super) fn preview_icon(icon: Icon) -> Engine {
    Engine {
        id: "custom-settings-preview".into(),
        name: String::new(),
        alias: String::new(),
        url: String::new(),
        color: 0x005fb8,
        icon,
    }
}

pub(super) fn display(state: &State, hwnd: HWND) {
    let filter = PANEL.with_borrow(|panel| panel.engine_filter.clone());
    let search = edit_at(hwnd, FILTER, &filter, place(10, 73, 316, 22, STRETCH));
    cue(search, "筛选名称或 #别名");
    action(hwnd, 122, "添加引擎", place(408, 66, 128, 34, RIGHT));
    Page(hwnd).section("置顶快捷位", 137, 209, &[]);
    Page(hwnd).section("", 216, -15, &[246]);
    caption(
        hwnd,
        "仅在搜索框中生效。",
        place(14, 182, 506, 20, STRETCH),
        fluent::FontRole::Detail,
    );
    caption(
        hwnd,
        "引擎",
        place(14, 222, 100, 22, 0),
        fluent::FontRole::Detail,
    );
    caption(
        hwnd,
        "别名",
        place(222, 222, 80, 22, RIGHT),
        fluent::FontRole::Detail,
    );
    caption(
        hwnd,
        "状态",
        place(389, 222, 76, 22, RIGHT),
        fluent::FontRole::Detail,
    );
    list_at(hwnd, std::iter::empty(), place(1, 247, 534, -265, STRETCH));
    refresh(state, hwnd, false);
}

pub(super) fn refresh(state: &State, hwnd: HWND, filter_changed: bool) {
    let list = unsafe { GetDlgItem(hwnd, 10) };
    if list.is_null() {
        return;
    }
    let filter = if filter_changed {
        field(hwnd, FILTER)
    } else {
        PANEL.with_borrow(|p| p.engine_filter.clone())
    };
    let needle = filter.trim().trim_start_matches('#').to_lowercase();
    let rows: Vec<_> = state
        .app
        .borrow()
        .engines()
        .iter()
        .enumerate()
        .filter_map(|(index, engine)| {
            (engine.name.to_lowercase().contains(&needle)
                || engine.alias.to_lowercase().contains(&needle))
            .then(|| (index, format!("{}\t{}", engine.name, engine.alias)))
        })
        .collect();
    let (selected, top) = PANEL.with_borrow_mut(|panel| {
        panel.engine_filter = filter;
        panel.engine_rows = rows.iter().map(|row| row.0).collect();
        if filter_changed {
            panel.engine_top = 0;
        }
        (
            panel.selected.and_then(|selected| {
                panel
                    .engine_rows
                    .iter()
                    .position(|&index| index == selected)
            }),
            panel.engine_top,
        )
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
        if let Some(selected) = selected {
            SendMessageW(list, LB_SETTOPINDEX, top, 0);
            SendMessageW(list, LB_SETCURSEL, selected, 0);
        } else {
            SendMessageW(list, LB_SETTOPINDEX, top, 0);
        }
        if visible {
            SendMessageW(list, WM_SETREDRAW, 1, 0);
            InvalidateRect(list, null(), 0);
        }
    }
}

pub(super) fn selected_index(hwnd: HWND) -> Option<usize> {
    let row = select_index(hwnd)?;
    PANEL.with_borrow(|p| p.engine_rows.get(row).copied())
}

pub(super) fn remember_list(hwnd: HWND) {
    let list = unsafe { GetDlgItem(hwnd, 10) };
    if !list.is_null() {
        let top = unsafe { SendMessageW(list, LB_GETTOPINDEX, 0, 0) }.max(0) as usize;
        let selected = selected_index(hwnd);
        PANEL.with_borrow_mut(|p| {
            p.engine_top = top;
            p.selected = selected;
        });
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
    let (default, pinned, custom, up, down) = {
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        let Some(engine) = app.engines().get(index) else {
            return;
        };
        let custom_index = app
            .settings()
            .custom_engines
            .iter()
            .position(|item| item.id == engine.id);
        (
            prefs.default_engine == engine.id,
            prefs.pins.contains(&engine.id),
            engine.is_custom(),
            custom_index.is_some_and(|i| i > 0),
            custom_index.is_some_and(|i| i + 1 < app.settings().custom_engines.len()),
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
            Item::new(120, "设为默认", !default),
            Item::pin(pinned),
            Item::new(123, if custom { "编辑" } else { "编辑别名" }, true),
            Item::new(129, "上移", up),
            Item::new(130, "下移", down),
            Item::new(124, "删除", true),
        ];
        super::menu::show(hwnd, rect, if custom { &items } else { &items[..3] }, 2);
    }
}
