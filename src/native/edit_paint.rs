use super::{
    State,
    paint::{TEXT, pixel_format, rect},
};
use std::ptr::{null, null_mut};
use windows::{
    Win32::{
        Foundation::E_FAIL,
        Graphics::Direct2D::{Common::*, *},
    },
    core::{Error, Result},
};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT, SIZE},
    Graphics::Gdi::*,
    UI::{
        Controls::{EM_GETRECT, EM_GETSEL, EM_POSFROMCHAR},
        Input::{Ime::*, KeyboardAndMouse::GetFocus},
        WindowsAndMessaging::*,
    },
};

pub(super) const CARET_TIMER: usize = 3;

pub(super) fn composition_result(edit: HWND) -> Option<Vec<u16>> {
    unsafe {
        let context = ImmGetContext(edit);
        if context.is_null() {
            return None;
        }
        let size = ImmGetCompositionStringW(context, GCS_RESULTSTR, null_mut(), 0);
        let result = if size > 0 {
            let mut units = vec![0; size as usize / 2 + 1];
            let received = ImmGetCompositionStringW(
                context,
                GCS_RESULTSTR,
                units.as_mut_ptr().cast(),
                size as u32,
            );
            if received > 0 {
                units.truncate(received as usize / 2 + 1);
                Some(units)
            } else {
                None
            }
        } else {
            None
        };
        ImmReleaseContext(edit, context);
        result
    }
}

#[derive(Default)]
pub(super) struct Preedit {
    pub(super) units: Vec<u16>,
    pub(super) cursor: usize,
}

impl Preedit {
    pub(super) fn update(&mut self, edit: HWND) {
        self.units.clear();
        unsafe {
            let context = ImmGetContext(edit);
            if context.is_null() {
                return;
            }
            let bytes = ImmGetCompositionStringW(context, GCS_COMPSTR, null_mut(), 0);
            if bytes > 0 {
                self.units.resize(bytes as usize / 2, 0);
                let received = ImmGetCompositionStringW(
                    context,
                    GCS_COMPSTR,
                    self.units.as_mut_ptr().cast(),
                    (self.units.len() * 2) as u32,
                );
                self.units.truncate(received.max(0) as usize / 2);
                let cursor = ImmGetCompositionStringW(context, GCS_CURSORPOS, null_mut(), 0);
                self.cursor = usize::try_from(cursor)
                    .unwrap_or(self.units.len())
                    .min(self.units.len());
            }
            ImmReleaseContext(edit, context);
        }
    }

    pub(super) fn clear(&mut self) {
        self.units.clear();
    }

    fn paint(&self, state: &State, dc: HDC) -> i32 {
        let edit = state.edit.get();
        unsafe {
            let mut text = vec![0; GetWindowTextLengthW(edit).max(0) as usize + 1];
            let length = GetWindowTextW(edit, text.as_mut_ptr(), text.len() as i32);
            text.truncate(length.max(0) as usize);
            let (mut start, mut end) = (0_u32, 0_u32);
            SendMessageW(
                edit,
                EM_GETSEL,
                (&mut start as *mut u32) as usize,
                (&mut end as *mut u32) as isize,
            );
            let start = (start as usize).min(text.len());
            let end = (end as usize).clamp(start, text.len());
            text.splice(start..end, self.units.iter().copied());

            let mut line = RECT::default();
            SendMessageW(edit, EM_GETRECT, 0, (&mut line as *mut RECT) as isize);
            let mut anchor = POINT {
                x: line.left,
                y: line.top,
            };
            GetCaretPos(&mut anchor);
            let position = SendMessageW(edit, EM_POSFROMCHAR, start, 0);
            if position != -1 {
                anchor.x = i32::from(position as i16);
            }

            let font = state.font.get();
            let previous = SelectObject(
                dc,
                if font.is_null() {
                    GetStockObject(DEFAULT_GUI_FONT)
                } else {
                    font
                },
            );
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, super::color_ref(capture_text_color(state)));
            let measure = |count| {
                let mut size = SIZE::default();
                GetTextExtentPoint32W(dc, text.as_ptr(), count as i32, &mut size);
                size.cx
            };
            let first = measure(start);
            let last = measure(start + self.units.len());
            let cursor = measure(start + self.cursor);
            let x = (anchor.x - first).min(line.right - cursor - state.scale(2));
            ExtTextOutW(
                dc,
                x,
                line.top,
                ETO_CLIPPED,
                &line,
                text.as_ptr(),
                text.len() as u32,
                null(),
            );
            let mut metrics = TEXTMETRICW::default();
            if GetTextMetricsW(dc, &mut metrics) != 0 {
                let y = (line.top + metrics.tmAscent + state.scale(2)).min(line.bottom - 1);
                let underline = RECT {
                    left: line.left.max(x + first),
                    top: y,
                    right: line.right.min(x + last),
                    bottom: y + 1,
                };
                FillRect(dc, &underline, GetStockObject(GRAY_BRUSH));
            }
            SelectObject(dc, previous);
            x + cursor
        }
    }
}

pub(super) struct EditLayer {
    dc: HDC,
    dib: HBITMAP,
    bits: *mut u32,
    size: D2D_SIZE_U,
    bitmap: ID2D1Bitmap,
    preedit_caret: Option<i32>,
}

impl EditLayer {
    fn new(canvas: &ID2D1DeviceContext, size: D2D_SIZE_U) -> Result<Self> {
        unsafe {
            let bitmap = ID2D1RenderTarget::CreateBitmap(
                canvas,
                size,
                None,
                0,
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: pixel_format(D2D1_ALPHA_MODE_PREMULTIPLIED),
                    dpiX: 96.0,
                    dpiY: 96.0,
                },
            )?;
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size.width as i32,
                    biHeight: -(size.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = null_mut();
            let dib = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if dib.is_null() {
                return Err(Error::from_thread());
            }
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                DeleteObject(dib);
                return Err(Error::from_thread());
            }
            SelectObject(dc, dib);
            Ok(Self {
                dc,
                dib,
                bits: bits.cast(),
                size,
                bitmap,
                preedit_caret: None,
            })
        }
    }

    pub(super) fn draw(
        layer: &mut Option<Self>,
        state: &State,
        canvas: &ID2D1DeviceContext,
    ) -> Result<Option<D2D_RECT_F>> {
        let edit = state.edit.get();
        if edit.is_null() {
            return Ok(None);
        }
        unsafe {
            let mut bounds = RECT::default();
            if GetClientRect(edit, &mut bounds) == 0 || bounds.right <= 0 || bounds.bottom <= 0 {
                return Err(Error::from_hresult(E_FAIL));
            }
            let size = D2D_SIZE_U {
                width: bounds.right as u32,
                height: bounds.bottom as u32,
            };
            let recreate = layer.as_ref().is_none_or(|layer| layer.size != size);
            if recreate {
                *layer = Some(Self::new(canvas, size)?);
            }
            let layer = layer.as_mut().unwrap();
            if state.edit_dirty.replace(false) || recreate {
                layer.capture(state, edit, &bounds)?;
            }
            let mut origin = POINT::default();
            MapWindowPoints(edit, state.window.get(), &mut origin, 1);
            let dip = 96.0 / state.dpi.get() as f32;
            ID2D1RenderTarget::DrawBitmap(
                canvas,
                &layer.bitmap,
                Some(&rect(
                    origin.x as f32 * dip,
                    origin.y as f32 * dip,
                    size.width as f32 * dip,
                    size.height as f32 * dip,
                )),
                state
                    .app
                    .borrow()
                    .settings()
                    .preferences
                    .search_text_opacity(),
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
            let mut caret = POINT::default();
            if GetFocus() == edit && GetCaretPos(&mut caret) != 0 {
                let mut line = RECT::default();
                SendMessageW(edit, EM_GETRECT, 0, (&mut line as *mut RECT) as isize);
                let width = state.scale(1).max(1);
                let x = layer
                    .preedit_caret
                    .unwrap_or(caret.x)
                    .clamp(0, (bounds.right - width).max(0));
                let height = (line.bottom - caret.y).max(1).min(state.scale(20));
                return Ok(Some(rect(
                    (origin.x + x) as f32 * dip,
                    (origin.y + caret.y) as f32 * dip,
                    width as f32 * dip,
                    height as f32 * dip,
                )));
            }
        }
        Ok(None)
    }

    unsafe fn capture(&mut self, state: &State, edit: HWND, bounds: &RECT) -> Result<()> {
        unsafe {
            FillRect(self.dc, bounds, GetStockObject(WHITE_BRUSH));
            let preedit = state.preedit.borrow();
            self.preedit_caret = if preedit.units.is_empty() {
                state.capture_dc.set(self.dc);
                SendMessageW(
                    edit,
                    WM_PRINTCLIENT,
                    self.dc as usize,
                    (PRF_CLIENT | PRF_ERASEBKGND) as isize,
                );
                state.capture_dc.set(null_mut());
                None
            } else {
                Some(preedit.paint(state, self.dc))
            };
            GdiFlush();
            let pixels = std::slice::from_raw_parts_mut(
                self.bits,
                self.size.width as usize * self.size.height as usize,
            );
            let foreground = state.search_colors().text;
            for pixel in pixels {
                *pixel = captured_pixel(*pixel, foreground);
            }
            self.bitmap
                .CopyFromMemory(None, self.bits.cast(), self.size.width * 4)
        }
    }
}

impl Drop for EditLayer {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.dc);
            DeleteObject(self.dib);
        }
    }
}

pub(super) fn capture_text_color(state: &State) -> u32 {
    if state.search_colors().text == TEXT {
        TEXT
    } else {
        0
    }
}

fn captured_pixel(pixel: u32, foreground: u32) -> u32 {
    let matte_removed = remove_white_matte(pixel);
    let [b, g, r, alpha] = matte_removed.to_le_bytes();
    // The capture uses black ink for custom colors so grayscale coverage can
    // be tinted without changing colored glyphs and native selection fills.
    if foreground == TEXT || b.max(g).max(r) - b.min(g).min(r) >= 8 {
        return matte_removed;
    }
    let [b, g, r] = [0, 8, 16]
        .map(|shift| ((((foreground >> shift) & 255) * u32::from(alpha) + 127) / 255) as u8);
    u32::from_le_bytes([b, g, r, alpha])
}

// The captured Edit was drawn over white. Subtract the shared RGB component
// to form premultiplied BGRA while retaining colored glyphs and selection.
fn remove_white_matte(pixel: u32) -> u32 {
    let [b, g, r, _] = pixel.to_le_bytes();
    let matte = b.min(g).min(r);
    u32::from_le_bytes([b - matte, g - matte, r - matte, 255 - matte])
}

#[cfg(test)]
mod tests {
    use super::{captured_pixel, remove_white_matte};

    #[test]
    fn edit_capture_removes_white_and_preserves_premultiplied_coverage() {
        assert_eq!(remove_white_matte(0x00ffffff), 0);
        assert_eq!(remove_white_matte(0x00000000), 0xff000000);
        assert_eq!(remove_white_matte(0x00808080), 0x7f000000);
        assert_eq!(remove_white_matte(0xff4080ff), 0xbf0040bf);
    }

    #[test]
    fn edit_capture_tints_black_ink_and_keeps_colored_pixels() {
        assert_eq!(captured_pixel(0, 0x20a0f0), 0xff20a0f0);
        assert_eq!(captured_pixel(0x00808080, 0x20a0f0), 0x7f105078);
        assert_eq!(captured_pixel(0x00ffffff, 0x20a0f0), 0);
        assert_eq!(captured_pixel(0xff4080ff, 0x20a0f0), 0xbf0040bf);
    }
}
