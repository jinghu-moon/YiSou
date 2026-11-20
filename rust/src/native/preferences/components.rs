use super::{
    HWND, PANEL, RIGHT, STRETCH, action, caption, edit_at,
    fluent::{FontRole, Layout},
    place,
};

pub(super) const WIDTH: i32 = 536;

#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub top: i32,
    pub bottom: i32,
    pub dividers: &'static [i32],
}

#[derive(Clone, Copy)]
pub(super) struct Page(pub HWND);

impl Page {
    pub fn heading(self, text: &str) {
        caption(
            self.0,
            text,
            place(0, 18, WIDTH, 43, STRETCH),
            FontRole::Header,
        );
    }

    pub fn section(self, text: &str, top: i32, bottom: i32, dividers: &'static [i32]) {
        if !text.is_empty() {
            caption(
                self.0,
                text,
                place(0, top - 28, WIDTH, 23, STRETCH),
                FontRole::Section,
            );
        }
        PANEL.with_borrow_mut(|p| {
            p.view.surfaces.push(Surface {
                top,
                bottom,
                dividers,
            })
        });
    }

    pub fn body(self, text: &str, layout: Layout) -> HWND {
        caption(self.0, text, layout, FontRole::Body)
    }

    pub fn detail(self, text: &str, layout: Layout) -> HWND {
        caption(self.0, text, layout, FontRole::Detail)
    }

    pub fn setting(self, title: &str, detail: &str, top: i32, trailing: i32) {
        if detail.is_empty() {
            self.body(
                title,
                place(14, top + 16, WIDTH - 28 - trailing, 24, STRETCH),
            );
        } else {
            self.body(
                title,
                place(14, top + 7, WIDTH - 28 - trailing, 23, STRETCH),
            );
            self.detail(
                detail,
                place(14, top + 30, WIDTH - 28 - trailing, 21, STRETCH),
            );
        }
    }

    pub fn toggle(self, id: usize, title: &str, enabled: bool, top: i32) -> HWND {
        action(
            self.0,
            id,
            &toggle_label(title, enabled),
            place(478, top + 14, 44, 30, RIGHT),
        )
    }

    pub fn field(
        self,
        id: usize,
        title: &str,
        value: &str,
        x: i32,
        y: i32,
        width: i32,
        sizing: i32,
    ) -> HWND {
        self.body(title, place(x, y, width, 22, sizing));
        edit_at(
            self.0,
            id,
            value,
            place(x + 8, y + 32, width - 16, 22, sizing),
        )
    }

    pub fn finish_at(self, bottom: i32) {
        PANEL.with_borrow_mut(|p| p.view.extent = bottom);
    }
}

pub(super) fn toggle_label(title: &str, enabled: bool) -> String {
    format!(
        "{} ({})",
        super::tr(title),
        super::tr(if enabled { "开" } else { "关" })
    )
}
