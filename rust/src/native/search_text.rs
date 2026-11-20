use super::{
    State,
    paint::{ACCENT, MUTED, TEXT, color, rect},
};
use windows::{
    Win32::Graphics::{Direct2D::*, DirectWrite::*},
    core::{PCWSTR, Result, w},
};
use windows_numerics::Vector2;
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::*,
    UI::{
        Controls::{EM_CHARFROMPOS, EM_GETRECT, EM_GETSEL, EM_POSFROMCHAR},
        Input::KeyboardAndMouse::GetFocus,
        WindowsAndMessaging::*,
    },
};

pub(super) struct SearchText {
    factory: IDWriteFactory,
    format: IDWriteTextFormat,
    layout: Option<Line>,
    selection: Vec<DWRITE_HIT_TEST_METRICS>,
}

struct Line {
    layout: IDWriteTextLayout,
    len: u32,
    width: f32,
    baseline: f32,
    native_ascent: i32,
}

impl Line {
    fn position(&self, index: u32) -> Result<f32> {
        let (mut x, mut y, mut hit) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
        unsafe {
            self.layout.HitTestTextPosition(
                index.min(self.len.saturating_sub(1)),
                index >= self.len,
                &mut x,
                &mut y,
                &mut hit,
            )?;
        }
        Ok(x)
    }
}

impl SearchText {
    pub fn new(state: &State) -> Result<Self> {
        let (family, collection) = state.drawing_font();
        let family = super::wide(&family);
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = factory.CreateTextFormat(
                PCWSTR(family.as_ptr()),
                collection.as_ref(),
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                state.scale(state.input_font_size()) as f32 * 96.0 / state.dpi.get() as f32,
                w!("zh-CN"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(Self {
                factory,
                format,
                layout: None,
                selection: Vec::new(),
            })
        }
    }

    fn line(&self, state: &State, area: RECT, dpi: f32) -> Result<Line> {
        let input = super::text_of(state.edit.get());
        let len = input.encode_utf16().count() as u32;
        let text: Vec<_> = if len == 0 { "搜索网页…" } else { &input }
            .encode_utf16()
            .collect();
        unsafe {
            let layout = self.factory.CreateTextLayout(
                &text,
                &self.format,
                (area.right - area.left) as f32 / dpi,
                (area.bottom - area.top) as f32 / dpi,
            )?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            let mut line = [DWRITE_LINE_METRICS::default()];
            layout.GetMetrics(&mut metrics)?;
            layout.GetLineMetrics(Some(&mut line), &mut 0)?;
            let edit = state.edit.get();
            let dc = GetDC(edit);
            let old = SelectObject(dc, state.font.get());
            let mut native = TEXTMETRICW::default();
            let measured = GetTextMetricsW(dc, &mut native) != 0;
            SelectObject(dc, old);
            ReleaseDC(edit, dc);
            Ok(Line {
                layout,
                len,
                width: metrics.widthIncludingTrailingWhitespace,
                baseline: line[0].baseline,
                native_ascent: if measured {
                    native.tmAscent
                } else {
                    (line[0].baseline * dpi).round() as i32
                },
            })
        }
    }

    pub fn draw(
        &mut self,
        state: &State,
        canvas: &ID2D1DeviceContext,
        brush: &ID2D1SolidColorBrush,
    ) -> Result<()> {
        let edit = state.edit.get();
        if edit.is_null() {
            return Ok(());
        }
        let mut area = RECT::default();
        let mut origin = POINT::default();
        let (mut start, mut end) = (0u32, 0u32);
        let dpi = state.dpi.get() as f32 / 96.0;
        unsafe {
            SendMessageW(edit, EM_GETRECT, 0, &mut area as *mut RECT as isize);
            if area.left >= area.right || area.top >= area.bottom {
                return Ok(());
            }
            MapWindowPoints(edit, state.window.get(), &mut origin, 1);
            SendMessageW(
                edit,
                EM_GETSEL,
                &mut start as *mut u32 as usize,
                &mut end as *mut u32 as isize,
            );
        }
        if self.layout.is_none() || state.search_text_dirty.get() {
            self.layout = Some(self.line(state, area, dpi)?);
            state.search_text_dirty.set(false);
        }
        let line = self.layout.as_ref().unwrap();
        let focused = unsafe { GetFocus() == edit };
        let width = (area.right - area.left) as f32 / dpi;
        let left = (origin.x + area.left) as f32 / dpi;
        let top = (origin.y + area.top) as f32 / dpi;
        let height = (area.bottom - area.top) as f32 / dpi;
        let scroll = if line.len > 0 && line.width > width {
            let first = native_character(edit, area.left, area.top).min(line.len - 1);
            let anchor = native_position(edit, first).unwrap_or(area.left);
            let offset = line.position(first)? - (anchor - area.left) as f32 / dpi;
            let limit = (line.width - width + 2.0 / dpi).max(0.0);
            // Keep the native viewport's anchor and make the trailing caret visible at End.
            if start == line.len && end == line.len {
                limit
            } else {
                offset.clamp(0.0, limit)
            }
        } else {
            0.0
        };
        let point = Vector2 {
            X: ((left - scroll) * dpi).round() / dpi,
            Y: top + line.native_ascent as f32 / dpi - line.baseline,
        };
        let selection = if focused && start < end && end <= line.len {
            let count = (end - start) as usize;
            self.selection
                .resize(count, DWRITE_HIT_TEST_METRICS::default());
            let mut actual = 0;
            unsafe {
                line.layout.HitTestTextRange(
                    start,
                    end - start,
                    point.X,
                    point.Y,
                    Some(&mut self.selection),
                    &mut actual,
                )?;
            }
            &self.selection[..actual as usize]
        } else {
            &[]
        };
        let caret = if focused && state.caret_on.get() {
            let mut caret = POINT::default();
            if unsafe { GetCaretPos(&mut caret) } == 0 {
                None
            } else {
                let index = if start < end && native_position(edit, start) == Some(caret.x) {
                    start
                } else {
                    end
                };
                let offset = if line.len == 0 {
                    0.0
                } else {
                    line.position(index)?
                };
                let pixel = 1.0 / dpi;
                let x = ((point.X + offset) * dpi).round() / dpi;
                Some(rect(
                    x.clamp(left, left + width - pixel),
                    top,
                    pixel,
                    height,
                ))
            }
        } else {
            None
        };
        unsafe {
            canvas
                .PushAxisAlignedClip(&rect(left, top, width, height), D2D1_ANTIALIAS_MODE_ALIASED);
            let draw = |rgb| {
                brush.SetColor(&color(rgb, 1.0));
                ID2D1RenderTarget::DrawTextLayout(
                    canvas,
                    point,
                    &line.layout,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                );
            };
            draw(if line.len == 0 { MUTED } else { TEXT });
            for hit in selection {
                let bounds = rect(hit.left, top, hit.width, height);
                brush.SetColor(&color(ACCENT, 1.0));
                canvas.FillRectangle(&bounds, brush);
                canvas.PushAxisAlignedClip(&bounds, D2D1_ANTIALIAS_MODE_ALIASED);
                draw(0xffffff);
                canvas.PopAxisAlignedClip();
            }
            if let Some(caret) = caret {
                brush.SetColor(&color(TEXT, 1.0));
                canvas.FillRectangle(&caret, brush);
            }
            canvas.PopAxisAlignedClip();
        }
        Ok(())
    }
}

fn native_character(edit: HWND, x: i32, y: i32) -> u32 {
    let packed = ((y as u16 as u32) << 16) | x as u16 as u32;
    unsafe { SendMessageW(edit, EM_CHARFROMPOS, 0, packed as isize) as u16 as u32 }
}

fn native_position(edit: HWND, index: u32) -> Option<i32> {
    let location = unsafe { SendMessageW(edit, EM_POSFROMCHAR, index as usize, 0) };
    (location != -1).then_some(i32::from(location as i16))
}
