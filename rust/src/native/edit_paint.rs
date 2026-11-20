use super::{
    State,
    paint::{TEXT, color, pixel_format, rect},
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

#[derive(Default)]
pub(super) struct Preedit {
    units: Vec<u16>,
    cursor: usize,
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
            SetTextColor(dc, super::color_ref(TEXT));
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
        brush: &ID2D1SolidColorBrush,
    ) -> Result<()> {
        let edit = state.edit.get();
        if edit.is_null() {
            return Ok(());
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
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
            let mut caret = POINT::default();
            if state.caret_on.get() && GetFocus() == edit && GetCaretPos(&mut caret) != 0 {
                let mut line = RECT::default();
                SendMessageW(edit, EM_GETRECT, 0, (&mut line as *mut RECT) as isize);
                let width = state.scale(1).max(1);
                let x = layer
                    .preedit_caret
                    .unwrap_or(caret.x)
                    .clamp(0, (bounds.right - width).max(0));
                let height = (line.bottom - caret.y).max(1).min(state.scale(20));
                brush.SetColor(&color(TEXT, 1.0));
                canvas.FillRectangle(
                    &rect(
                        (origin.x + x) as f32 * dip,
                        (origin.y + caret.y) as f32 * dip,
                        width as f32 * dip,
                        height as f32 * dip,
                    ),
                    brush,
                );
            }
        }
        Ok(())
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
            pixels
                .iter_mut()
                .for_each(|pixel| *pixel = remove_white_matte(*pixel));
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

// The captured Edit was drawn over white. Subtract the shared RGB component
// to form premultiplied BGRA while retaining colored glyphs and selection.
fn remove_white_matte(pixel: u32) -> u32 {
    let [b, g, r, _] = pixel.to_le_bytes();
    let matte = b.min(g).min(r);
    u32::from_le_bytes([b - matte, g - matte, r - matte, 255 - matte])
}

#[cfg(test)]
mod tests {
    use super::remove_white_matte;

    #[test]
    fn edit_capture_removes_white_and_preserves_premultiplied_coverage() {
        assert_eq!(remove_white_matte(0x00ffffff), 0);
        assert_eq!(remove_white_matte(0x00000000), 0xff000000);
        assert_eq!(remove_white_matte(0x00808080), 0x7f000000);
        assert_eq!(remove_white_matte(0xff4080ff), 0xbf0040bf);
    }
}
