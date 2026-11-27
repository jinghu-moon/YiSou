use std::{
    io,
    ptr::{null, null_mut},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::*,
};
#[cfg(feature = "ocr")]
use yisou::ocr::{PixelFormat, Pixels};

pub(super) fn expand(rect: RECT, margin: i32, limit: RECT) -> RECT {
    RECT {
        left: (rect.left - margin).max(limit.left),
        top: (rect.top - margin).max(limit.top),
        right: (rect.right + margin).min(limit.right),
        bottom: (rect.bottom + margin).min(limit.bottom),
    }
}

pub(super) fn same_rect(a: RECT, b: RECT) -> bool {
    (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
}

#[cfg(feature = "ocr")]
pub(super) fn intersect(a: RECT, b: RECT) -> Option<RECT> {
    let rect = RECT {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
}

pub(super) fn desktop_bounds() -> io::Result<RECT> {
    let (x, y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > 64 * 1024 * 1024 {
        return Err(io::Error::other("unsupported desktop size"));
    }
    Ok(RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    })
}

pub(super) struct Snapshot {
    pub area: RECT,
    pub data: Arc<[u8]>,
}

impl Snapshot {
    pub fn take(desktop: RECT, selection: RECT, margin: i32) -> io::Result<Self> {
        let _span = super::perf::span("capture_pixels");
        let extent = RECT {
            right: desktop.right - desktop.left,
            bottom: desktop.bottom - desktop.top,
            ..RECT::default()
        };
        let area = expand(selection, margin, extent);
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        let stride = width as usize * 4;
        let mut pixels = Arc::<[u8]>::new_uninit_slice(stride * height as usize);
        let pixels_out = Arc::get_mut(&mut pixels).unwrap();
        unsafe {
            let screen = GetDC(null_mut());
            if screen.is_null() {
                return Err(io::Error::last_os_error());
            }
            let dib = CropDib::new(screen, width, height);
            let result = dib.and_then(|dib| {
                let bitblt = super::perf::start();
                if BitBlt(
                    dib.dc,
                    0,
                    0,
                    width,
                    height,
                    screen,
                    desktop.left + area.left,
                    desktop.top + area.top,
                    SRCCOPY | CAPTUREBLT,
                ) == 0
                    || GdiFlush() == 0
                {
                    return Err(io::Error::last_os_error());
                }
                super::perf::record("bitblt_and_flush", bitblt);
                let _copy = super::perf::span("copy_pixels");
                let pixels_in = std::slice::from_raw_parts(dib.bits, pixels_out.len());
                for (out, bgra) in pixels_out
                    .chunks_exact_mut(4)
                    .zip(pixels_in.chunks_exact(4))
                {
                    out[0].write(bgra[0]);
                    out[1].write(bgra[1]);
                    out[2].write(bgra[2]);
                    out[3].write(255);
                }
                Ok(())
            });
            ReleaseDC(null_mut(), screen);
            result?;
        }
        Ok(Self {
            area,
            data: unsafe { pixels.assume_init() },
        })
    }

    pub fn width(&self) -> u32 {
        (self.area.right - self.area.left) as u32
    }

    pub fn height(&self) -> u32 {
        (self.area.bottom - self.area.top) as u32
    }

    pub fn stride(&self) -> usize {
        self.width() as usize * 4
    }

    #[cfg(feature = "ocr")]
    fn row(&self, rect: RECT, y: i32) -> &[u8] {
        let offset = (y - self.area.top) as usize * self.stride()
            + (rect.left - self.area.left) as usize * 4;
        &self.data[offset..offset + (rect.right - rect.left) as usize * 4]
    }

    #[cfg(feature = "ocr")]
    pub fn crop(&self, rect: RECT) -> Pixels {
        debug_assert!(intersect(rect, self.area).is_some_and(|overlap| same_rect(rect, overlap)));
        let width = (rect.right - rect.left) as u32;
        let height = (rect.bottom - rect.top) as u32;
        let stride = width as usize * 4;
        let data = if same_rect(rect, self.area) {
            Arc::clone(&self.data)
        } else {
            let mut pixels = Arc::<[u8]>::new_uninit_slice(stride * height as usize);
            for (y, row) in (rect.top..rect.bottom)
                .zip(Arc::get_mut(&mut pixels).unwrap().chunks_exact_mut(stride))
            {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.row(rect, y).as_ptr(),
                        row.as_mut_ptr().cast(),
                        stride,
                    )
                };
            }
            unsafe { pixels.assume_init() }
        };
        Pixels {
            width,
            height,
            stride,
            format: PixelFormat::Bgra8,
            bottom_up: false,
            data,
        }
    }

    #[cfg(feature = "ocr")]
    pub fn matching_area(&self, current: &Self) -> Option<RECT> {
        intersect(self.area, current.area).filter(|&area| {
            (area.top..area.bottom).all(|y| self.row(area, y) == current.row(area, y))
        })
    }

    #[cfg(feature = "ocr")]
    pub fn dark_at(&self, x: i32, y: i32) -> bool {
        let x = x.clamp(self.area.left, self.area.right - 1);
        let y = y.clamp(self.area.top, self.area.bottom - 1);
        let pixel = self.row(
            RECT {
                left: x,
                right: x + 1,
                ..self.area
            },
            y,
        );
        29 * u32::from(pixel[0]) + 150 * u32::from(pixel[1]) + 77 * u32::from(pixel[2]) < 128 * 256
    }
}

struct CropDib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *const u8,
}

impl CropDib {
    fn new(screen: HDC, width: i32, height: i32) -> io::Result<Self> {
        unsafe {
            let mut dib = Self {
                dc: CreateCompatibleDC(screen),
                bitmap: null_mut(),
                previous: null_mut(),
                bits: null(),
            };
            if dib.dc.is_null() {
                return Err(io::Error::last_os_error());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = null_mut();
            dib.bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if dib.bitmap.is_null() || bits.is_null() {
                return Err(io::Error::last_os_error());
            }
            let old = SelectObject(dib.dc, dib.bitmap);
            if old.is_null() || old as isize == -1 {
                return Err(io::Error::last_os_error());
            }
            dib.previous = old;
            dib.bits = bits.cast();
            Ok(dib)
        }
    }
}

impl Drop for CropDib {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_null() {
                SelectObject(self.dc, self.previous);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
        }
    }
}

pub(super) fn invalidate(hwnd: HWND, rect: Option<&RECT>) {
    unsafe { InvalidateRect(hwnd, rect.map_or(null(), std::ptr::from_ref), 0) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::screenshot::render::Canvas;

    #[test]
    fn large_preview_preserves_pixels_and_highlights_across_strips() {
        const WIDTH: i32 = 1600;
        const HEIGHT: i32 = 900;
        let mut data = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                data.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
            }
        }
        let snapshot = Snapshot {
            area: RECT {
                left: 10,
                top: 20,
                right: WIDTH + 10,
                bottom: HEIGHT + 20,
            },
            data: data.into(),
        };
        let area = RECT {
            left: 18,
            top: 28,
            right: 1610,
            bottom: 920,
        };
        let dirty = RECT {
            left: 5,
            top: 7,
            right: 1580,
            bottom: 880,
        };
        let highlight = RECT {
            left: 60,
            top: 580,
            right: 80,
            bottom: 800,
        };
        let output = CropDib::new(null_mut(), WIDTH, HEIGHT).unwrap();
        Canvas::new()
            .unwrap()
            .draw_preview(output.dc, area, dirty, &snapshot, |canvas| {
                canvas.fill(highlight, 0x2468ac, 1.0);
            })
            .unwrap();
        assert_ne!(unsafe { GdiFlush() }, 0);
        let output =
            unsafe { std::slice::from_raw_parts(output.bits, (WIDTH * HEIGHT * 4) as usize) };
        for y in dirty.top..dirty.bottom {
            for x in dirty.left..dirty.right {
                let source_x = area.left + x - snapshot.area.left;
                let source_y = area.top + y - snapshot.area.top;
                let blue_green_red = if (highlight.left..highlight.right).contains(&(area.left + x))
                    && (highlight.top..highlight.bottom).contains(&(area.top + y))
                {
                    [0xac, 0x68, 0x24]
                } else {
                    [source_x as u8, source_y as u8, (source_x + source_y) as u8]
                };
                let offset = ((y * WIDTH + x) * 4) as usize;
                assert_eq!(
                    &output[offset..offset + 3],
                    &blue_green_red,
                    "pixel ({x}, {y})"
                );
            }
        }
    }
}
