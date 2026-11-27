#![windows_subsystem = "windows"]

use std::{cell::RefCell, rc::Rc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use nsh_base::{button, input};
use nsh_core::{
    Color,
    desktop::{WindowAttributes, WindowFrame},
    element::{ImageSrc, desktop::current_window},
    prelude::*,
};
use nsh_platform_windows::{AppOptions, AppSource, Root, run_app};
use yisou::{
    application::{Application, History, Origin, Query, SearchOutcome},
    engine::{self, Engine, built_in_engines},
    platform::WindowsBrowser,
    settings::Settings,
    storage::{SettingsStore, StorageError},
};

const TITLE: &str = "易搜 · NSH 原型";
const WIDTH: i32 = 580;
const CLOSED_HEIGHT: i32 = 64;
const OPEN_HEIGHT: i32 = 258;
const TEXT: Color = Color(0xff1a2029);
const MUTED: Color = Color(0xff536273);
const SURFACE: Color = Color(0xfffcfdff);
const RAIL: Color = Color(0xfff1f3f7);
const BORDER: Color = Color(0xffdde2ea);
const ACCENT: Color = Color(0xff005fb8);
const HOVER: Color = Color(0xffebf0f7);
const SELECTED: Color = Color(0xffe6eef9);

macro_rules! icon {
    ($path:literal) => {
        include_bytes!(concat!("../../../assets/icons/", $path))
    };
}

fn svg(bytes: &[u8]) -> ImageSrc {
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(bytes)).into()
}

fn engine_icon(id: &str) -> ImageSrc {
    svg(match id {
        "amazon" => icon!("amazon.svg"),
        "baidu" => icon!("baidu.svg"),
        "bilibili" => icon!("bilibili.svg"),
        "bing" => icon!("bing.svg"),
        "github" => icon!("github.svg"),
        "google" => icon!("google.svg"),
        "reddit" => icon!("reddit.svg"),
        "stackoverflow" => icon!("stackoverflow.svg"),
        "taobao" => icon!("taobao.svg"),
        "youtube" => icon!("youtube.svg"),
        "zhihu" => icon!("zhihu.svg"),
        _ => icon!("tabler/search.svg"),
    })
}

struct SessionStore;

impl SettingsStore for SessionStore {
    fn save(&mut self, _: &Settings) -> Result<(), StorageError> {
        Ok(())
    }
}

#[derive(Clone)]
struct EngineTile {
    engine: Engine,
    icon: ImageSrc,
}

#[component]
fn SearchWindow() -> impl IntoElement {
    let window = current_window();
    let mut attributes = WindowAttributes::default();
    attributes.resizable = Some(false);
    window.set(attributes);
    let engines: Rc<[EngineTile]> = built_in_engines()
        .into_iter()
        .map(|engine| EngineTile {
            icon: engine_icon(&engine.id),
            engine,
        })
        .collect();
    let app = Rc::new(RefCell::new(
        Application::new(
            Settings::default(),
            SessionStore,
            WindowsBrowser,
            History::new(std::env::temp_dir().join("YiSou.NshPrototype.History.json")),
        )
        .expect("built-in settings are valid"),
    ));
    let query = signal(String::new());
    let selected = signal(0usize);
    let expanded = signal(false);
    let browser_error = signal(false);
    let duration = if reduce_motion() { 0.0 } else { 120.0 };
    let field = element_ref();
    let set_expanded = {
        let window = window.clone();
        move |open| {
            expanded.set(open);
            window.set_bounds(WindowFrame {
                height: Some(px(if open { OPEN_HEIGHT } else { CLOSED_HEIGHT } as f32)),
                ..Default::default()
            });
        }
    };
    let submit = {
        let app = app.clone();
        let engines = engines.clone();
        let collapse = set_expanded.clone();
        move |value: &str| {
            let result = app.borrow_mut().search(Query {
                text: value.to_owned(),
                engine: Some(engines[selected.get_untracked()].engine.id.clone()),
                origin: Origin::Typed,
                source: String::new(),
            });
            browser_error.set(matches!(&result, SearchOutcome::Failed { .. }));
            if matches!(result, SearchOutcome::Opened { .. }) {
                query.set(String::new());
                collapse(false);
            }
        }
    };
    let select = {
        let engines = engines.clone();
        let collapse = set_expanded.clone();
        move |index| {
            selected.set(index);
            let value = query.get_untracked();
            if let Some((alias, rest)) = engine::alias_prefix(&value)
                && !alias.is_empty()
                && engines
                    .iter()
                    .any(|tile| tile.engine.alias.eq_ignore_ascii_case(alias))
            {
                query.set(rest.to_string());
            }
            collapse(false);
            field.focus();
        }
    };
    let toggle = {
        let set_expanded = set_expanded.clone();
        move || set_expanded(!expanded.get_untracked())
    };
    let close = window.clone();
    let close_menu = set_expanded.clone();
    on_key("escape", move |_| {
        if expanded.get_untracked() {
            close_menu(false);
            field.focus();
        } else {
            close.close();
        }
    });
    let show_menu = set_expanded.clone();
    on_key("alt+down", move |_| show_menu(true));
    on_key("up", move |_| {
        if query.get_untracked().is_empty() && !expanded.get_untracked() {
            query.set(app.borrow().last_query().to_owned());
        }
    });

    let button_icons: Vec<_> = engines.iter().map(|tile| tile.icon.clone()).collect();
    let button_labels: Vec<_> = engines
        .iter()
        .map(|tile| tile.engine.name.clone())
        .collect();
    let menu_engines = engines.clone();
    let menu_button = button()
        .accessibility_label("选择搜索引擎")
        .h(px(34.0))
        .w(px(134.0))
        .rounded(px(6.0))
        .bg(move || if expanded.get() { SELECTED } else { RAIL })
        .transition()
        .duration(duration)
        .hover(|style| style.bg(HOVER))
        .focus_visible(|style| style.border_1().border_color(ACCENT))
        .active(|style| style.bg(SELECTED))
        .on_press(toggle)
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .child(
            image(move || button_icons[selected.get()].clone())
                .size(px(20.0))
                .flex_shrink_0(),
        )
        .child(
            text(move || button_labels[selected.get()].clone())
                .text_size(px(13.0))
                .number_of_lines(1)
                .text_color(TEXT)
                .flex_1(),
        )
        .child(
            image(svg(icon!("tabler/chevron-down.svg")))
                .size(px(16.0))
                .flex_shrink_0(),
        );

    view()
        .flex_1()
        .p_1()
        .gap_1()
        .child(
            view()
                .h(px(56.0))
                .flex_shrink_0()
                .rounded(px(8.0))
                .border_1()
                .border_color(BORDER)
                .bg(SURFACE)
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .window_drag(true)
                .child(
                    button()
                        .accessibility_label("搜索")
                        .size(px(30.0))
                        .rounded(px(6.0))
                        .hover(|style| style.bg(HOVER))
                        .focus_visible(|style| style.border_1().border_color(ACCENT))
                        .items_center()
                        .justify_center()
                        .on_press({
                            let submit = submit.clone();
                            move || submit(&query.get_untracked())
                        })
                        .child(image(svg(icon!("tabler/search.svg"))).size(px(18.0))),
                )
                .child(
                    input()
                        .label("搜索内容")
                        .element_ref(&field)
                        .auto_focus(true)
                        .value(move || query.get())
                        .on_change(move |value: &str| {
                            query.set(value.to_string());
                            browser_error.set(false);
                        })
                        .on_submit(submit)
                        .placeholder(move || {
                            if browser_error.get() {
                                "浏览器打开失败"
                            } else {
                                "搜索网页…"
                            }
                        })
                        .flex_1()
                        .h(px(32.0))
                        .text_size(px(16.0))
                        .text_color(move || {
                            if browser_error.get() {
                                Color(0xff9d3026)
                            } else {
                                TEXT
                            }
                        }),
                )
                .child(menu_button)
                .child(
                    button()
                        .accessibility_label("关闭")
                        .size(px(28.0))
                        .rounded(px(6.0))
                        .hover(|style| style.bg(HOVER))
                        .focus_visible(|style| style.border_1().border_color(ACCENT))
                        .items_center()
                        .justify_center()
                        .on_press(move || window.close())
                        .child(text("×").text_size(px(18.0)).text_color(MUTED)),
                ),
        )
        .child(when(
            move || expanded.get(),
            move || {
                let mut menu = view()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(BORDER)
                    .bg(SURFACE)
                    .gap_1()
                    .p_2();
                for (row, tiles) in menu_engines.chunks(6).enumerate() {
                    let mut line = view().flex_row().gap_1();
                    for (column, tile) in tiles.iter().enumerate() {
                        let index = row * 6 + column;
                        let choose = select.clone();
                        line = line.child(
                            button()
                                .accessibility_label(tile.engine.name.clone())
                                .w(px(88.0))
                                .h(px(82.0))
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .rounded(px(6.0))
                                .bg(move || {
                                    if selected.get() == index {
                                        SELECTED
                                    } else {
                                        SURFACE
                                    }
                                })
                                .transition()
                                .duration(duration)
                                .hover(|style| style.bg(HOVER))
                                .focus_visible(|style| style.border_1().border_color(ACCENT))
                                .on_press(move || choose(index))
                                .child(
                                    text(format!("#{}", tile.engine.alias))
                                        .text_color(MUTED)
                                        .text_size(px(11.0)),
                                )
                                .child(image(tile.icon.clone()).size(px(28.0)))
                                .child(
                                    text(tile.engine.name.clone())
                                        .number_of_lines(1)
                                        .text_color(TEXT)
                                        .text_size(px(12.0)),
                                ),
                        );
                    }
                    menu = menu.child(line);
                }
                menu
            },
        ))
}

fn main() -> std::process::ExitCode {
    run_app(
        AppSource::Rust(Root::of::<SearchWindow>()),
        AppOptions {
            title: TITLE.into(),
            app_id: "cn.yisou.nsh-prototype".into(),
            size: (WIDTH, CLOSED_HEIGHT),
            transparent: true,
            single_instance: true,
            ..Default::default()
        },
    )
}
