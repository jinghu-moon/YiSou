use super::{
    EnableWindow, GWLP_ID, GetDlgItem, HWND, PANEL, RIGHT, STRETCH, SetWindowLongPtrW, action,
    caption, edit_at,
    fluent::{FontRole, Layout},
    number_at, place,
};
use crate::native::paint::Label;
use std::ops::RangeInclusive;

pub(super) const WIDTH: i32 = 536;
pub(super) const FIRST_CARD: i32 = 113;
pub(super) const CARD_GAP: i32 = 5;
pub(super) const SECTION_GAP: i32 = 53;
pub(super) const ROW: i32 = 54;
pub(super) const ROW_WITH_DETAIL: i32 = 61;
pub(super) const BUTTON_PADDING: i32 = 12;
pub(super) const CONTROL_HEIGHT: i32 = 32;
pub(super) const BUTTON_GAP: i32 = 8;
pub(super) const BUTTON_ICON_SPACE: i32 = 20;
pub(super) const SETTING_TEXT_X: i32 = 44;
const PAGE_HEADING: isize = 192;

pub(super) fn button_width(text: &str, label: Label) -> i32 {
    super::fluent::text_width(super::tr(text), label) + 2 * BUTTON_PADDING
}

pub(super) struct NumberStepper {
    pub input: usize,
    pub decrease: usize,
    pub increase: usize,
    pub range: RangeInclusive<i32>,
    pub unit: &'static str,
}

impl NumberStepper {
    pub fn create(&self, hwnd: HWND, value: i32, x: i32, top: i32, sizing: i32, labels: [&str; 2]) {
        const EDGE: i32 = CONTROL_HEIGHT;
        let input_width = if *self.range.end() > 999 { 36 } else { 24 };
        let x = x + 24 - input_width;
        action(
            hwnd,
            self.decrease,
            labels[0],
            place(x, top, EDGE, EDGE, sizing),
        );
        number_at(
            hwnd,
            self.input,
            value,
            place(x + 44, top, input_width, CONTROL_HEIGHT, sizing).suffix(self.unit),
        );
        action(
            hwnd,
            self.increase,
            labels[1],
            place(x + 86 + input_width, top, EDGE, EDGE, sizing),
        );
        self.sync(hwnd, value);
    }

    pub fn sync(&self, hwnd: HWND, value: i32) {
        unsafe {
            EnableWindow(
                GetDlgItem(hwnd, self.decrease as i32),
                i32::from(value > *self.range.start()),
            );
            EnableWindow(
                GetDlgItem(hwnd, self.increase as i32),
                i32::from(value < *self.range.end()),
            );
        }
    }

    pub fn stepped(&self, value: i32, button: usize) -> i32 {
        let step = if button == self.decrease { -1 } else { 1 };
        value
            .saturating_add(step)
            .clamp(*self.range.start(), *self.range.end())
    }
}

#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub x: i32,
    pub width: i32,
    pub sizing: i32,
    pub top: i32,
    pub bottom: i32,
    pub dividers: &'static [i32],
}

#[derive(Clone, Copy)]
pub(super) struct Page(pub HWND);

impl Page {
    pub fn heading(self, text: &str) {
        self.heading_with_trailing(text, 0);
    }

    pub fn heading_with_trailing(self, text: &str, trailing: i32) {
        let title = caption(
            self.0,
            text,
            place(0, 24, WIDTH - trailing, 38, STRETCH),
            FontRole::Header,
        );
        unsafe { SetWindowLongPtrW(title, GWLP_ID, PAGE_HEADING) };
    }

    pub fn section_title(self, text: &str, top: i32) {
        caption(
            self.0,
            text,
            place(0, top, WIDTH, 23, STRETCH),
            FontRole::Section,
        );
    }

    pub fn section(self, text: &str, top: i32, bottom: i32, dividers: &'static [i32]) {
        if !text.is_empty() {
            self.section_title(text, top - 31);
        }
        self.card(0, WIDTH, STRETCH, top, bottom, dividers);
    }

    pub fn cards(self, text: &str, top: i32, heights: &[i32]) {
        self.section(text, top, top + heights[0], &[]);
        let mut y = top + heights[0] + CARD_GAP;
        for &height in &heights[1..] {
            self.section("", y, y + height, &[]);
            y += height + CARD_GAP;
        }
    }

    pub fn card(
        self,
        x: i32,
        width: i32,
        sizing: i32,
        top: i32,
        bottom: i32,
        dividers: &'static [i32],
    ) {
        super::fluent::with_view_mut(self.0, |view| {
            view.surfaces.push(Surface {
                x,
                width,
                sizing,
                top,
                bottom,
                dividers,
            })
        });
    }

    pub fn setting_with_icon(
        self,
        icon: (&'static str, &'static [u8]),
        title: &str,
        detail: &str,
        top: i32,
        trailing: i32,
    ) {
        let x = SETTING_TEXT_X;
        let width = WIDTH - x - 14 - trailing;
        self.body(
            title,
            place(
                x,
                top + if detail.is_empty() { 15 } else { 10 },
                width,
                23,
                STRETCH,
            ),
        );
        if !detail.is_empty() {
            self.detail(detail, place(x, top + 32, width, 20, STRETCH));
        }
        let row_height = if detail.is_empty() {
            ROW
        } else {
            ROW_WITH_DETAIL
        };
        PANEL.with_borrow_mut(|p| {
            p.view
                .row_icons
                .push((icon, 13, top + (row_height - 18) / 2))
        });
    }

    pub fn body(self, text: &str, layout: Layout) -> HWND {
        caption(self.0, text, layout, FontRole::Body)
    }

    pub fn detail(self, text: &str, layout: Layout) -> HWND {
        caption(self.0, text, layout, FontRole::Detail)
    }

    /// Returns the opposite edge of the button row in the page's base layout.
    pub fn buttons(self, anchor: i32, top: i32, sizing: i32, items: &[(usize, &str)]) -> i32 {
        self.buttons_after(anchor, top, sizing, items, |_| {})
    }

    pub fn buttons_after(
        self,
        anchor: i32,
        top: i32,
        sizing: i32,
        items: &[(usize, &str)],
        leading: impl FnOnce(i32),
    ) -> i32 {
        let widths: Vec<_> = items
            .iter()
            .map(|(_, text)| button_width(text, Label::Control))
            .collect();
        let total = widths.iter().sum::<i32>() + BUTTON_GAP * items.len().saturating_sub(1) as i32;
        let start = if sizing == RIGHT {
            anchor - total
        } else {
            anchor
        };
        leading(start);
        let mut x = start;
        for (&(id, label), width) in items.iter().zip(widths) {
            action(
                self.0,
                id,
                label,
                place(x, top, width, CONTROL_HEIGHT, sizing),
            );
            x += width + BUTTON_GAP;
        }
        if sizing == RIGHT {
            start
        } else {
            start + total
        }
    }

    pub fn toggle(self, id: usize, title: &str, enabled: bool, top: i32, row_height: i32) -> HWND {
        action(
            self.0,
            id,
            &toggle_label(title, enabled),
            place(
                479,
                top + (row_height - CONTROL_HEIGHT) / 2,
                44,
                CONTROL_HEIGHT,
                RIGHT,
            )
            .toggle(),
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
            place(x + 8, y + 26, width - 16, CONTROL_HEIGHT, sizing),
        )
    }

    pub fn finish_at(self, bottom: i32) {
        super::fluent::with_view_mut(self.0, |view| view.extent = bottom);
    }
}

pub(super) fn toggle_label(title: &str, enabled: bool) -> String {
    format!(
        "{} ({})",
        super::tr(title),
        super::tr(if enabled { "开" } else { "关" })
    )
}
