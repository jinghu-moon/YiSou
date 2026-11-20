use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use windows::{
    Win32::{
        Foundation::RECT as WinRect,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Gdi::HDC as WinDc,
        },
    },
    core::{Result, w},
};
use windows_numerics::{Matrix3x2, Vector2};
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::HDC,
};
use yisou::icons::OcrIcon;

use super::super::paint::{color, pixel_format, rect};
use super::capture::Snapshot;

const PREVIEW_BUFFER_BYTES: usize = 4 * 1024 * 1024;

fn bounds(r: RECT) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.left as f32,
        top: r.top as f32,
        right: r.right as f32,
        bottom: r.bottom as f32,
    }
}

fn point(p: POINT, offset: f32) -> Vector2 {
    Vector2 {
        X: p.x as f32 + offset,
        Y: p.y as f32 + offset,
    }
}

pub(super) struct Canvas {
    context: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    factory: ID2D1Factory,
    write: IDWriteFactory,
    formats: HashMap<i32, IDWriteTextFormat>,
    icons: HashMap<(OcrIcon, u32, u32, u8), ID2D1Bitmap>,
    preview: Option<(Weak<[u8]>, ID2D1Bitmap)>,
    preview_strip: Option<ID2D1Bitmap>,
}

impl Canvas {
    pub fn new() -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let context = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: pixel_format(D2D1_ALPHA_MODE_IGNORE),
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            })?;
            context.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Self {
                brush: context.CreateSolidColorBrush(&color(0, 1.0), None)?,
                factory,
                write: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                context,
                formats: HashMap::new(),
                icons: HashMap::new(),
                preview: None,
                preview_strip: None,
            })
        }
    }

    pub fn draw(
        &mut self,
        dc: HDC,
        area: RECT,
        dirty: RECT,
        paint: impl FnOnce(&mut Self),
    ) -> Result<()> {
        unsafe {
            self.context.BindDC(
                WinDc(dc),
                &WinRect {
                    left: dirty.left,
                    top: dirty.top,
                    right: dirty.right,
                    bottom: dirty.bottom,
                },
            )?;
            self.context.SetTransform(&Matrix3x2 {
                M11: 1.0,
                M22: 1.0,
                M31: -(area.left + dirty.left) as f32,
                M32: -(area.top + dirty.top) as f32,
                ..Default::default()
            });
            self.context.BeginDraw();
        }
        paint(self);
        unsafe { self.context.EndDraw(None, None) }
    }

    pub fn clear(&self, rgb: u32) {
        unsafe { self.context.Clear(Some(&color(rgb, 1.0))) };
    }

    pub fn draw_preview(
        &mut self,
        dc: HDC,
        area: RECT,
        dirty: RECT,
        snapshot: &Snapshot,
        mut paint_overlays: impl FnMut(&Self),
    ) -> Result<()> {
        if snapshot.pixels.data.len() <= PREVIEW_BUFFER_BYTES {
            self.preview_strip = None;
            let mut base = Ok(());
            self.draw(dc, area, dirty, |canvas| {
                base = canvas.preview(snapshot, area);
                paint_overlays(canvas);
            })?;
            return base;
        }
        self.preview = None;
        let width = (dirty.right - dirty.left) as usize;
        let rows = (PREVIEW_BUFFER_BYTES / (width * 4)).max(1) as i32;
        let strip_rows = rows.min(dirty.bottom - dirty.top);
        for top in (dirty.top..dirty.bottom).step_by(strip_rows as usize) {
            let strip = RECT {
                top,
                bottom: (top + strip_rows).min(dirty.bottom),
                ..dirty
            };
            let mut source = strip;
            unsafe {
                windows_sys::Win32::Graphics::Gdi::OffsetRect(&mut source, area.left, area.top)
            };
            // Upload before BeginDraw so each strip can reuse the same bitmap.
            self.upload_preview_strip(snapshot, source, strip_rows as u32)?;
            self.draw(dc, area, strip, |canvas| {
                unsafe {
                    canvas.context.DrawBitmap(
                        canvas.preview_strip.as_ref().unwrap(),
                        Some(&bounds(source)),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                        Some(&rect(
                            0.0,
                            0.0,
                            width as f32,
                            (strip.bottom - strip.top) as f32,
                        )),
                    );
                }
                paint_overlays(canvas);
            })?;
        }
        Ok(())
    }

    fn upload_preview_strip(&mut self, snapshot: &Snapshot, source: RECT, rows: u32) -> Result<()> {
        let pixels = &snapshot.pixels;
        let width = (source.right - source.left) as u32;
        let height = (source.bottom - source.top) as u32;
        let offset = (source.top - snapshot.area.top) as usize * pixels.stride
            + (source.left - snapshot.area.left) as usize * 4;
        let data = pixels.data[offset..].as_ptr().cast();
        unsafe {
            if self.preview_strip.as_ref().is_none_or(|bitmap| {
                let size = bitmap.GetPixelSize();
                size.width != width || size.height != rows
            }) {
                self.preview_strip = None;
                self.preview_strip = Some(self.context.CreateBitmap(
                    D2D_SIZE_U {
                        width,
                        height: rows,
                    },
                    None,
                    0,
                    &D2D1_BITMAP_PROPERTIES {
                        pixelFormat: pixel_format(D2D1_ALPHA_MODE_IGNORE),
                        dpiX: 96.0,
                        dpiY: 96.0,
                    },
                )?);
            }
            self.preview_strip.as_ref().unwrap().CopyFromMemory(
                Some(&D2D_RECT_U {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                }),
                data,
                pixels.stride as u32,
            )
        }
    }

    fn preview(&mut self, snapshot: &Snapshot, area: RECT) -> Result<()> {
        let pixels = &snapshot.pixels;
        if self.preview.as_ref().is_none_or(|(cached, _)| {
            !std::ptr::addr_eq(cached.as_ptr(), Arc::as_ptr(&pixels.data))
        }) {
            let bitmap = unsafe {
                self.context.CreateBitmap(
                    D2D_SIZE_U {
                        width: pixels.width,
                        height: pixels.height,
                    },
                    Some(pixels.data.as_ptr().cast()),
                    pixels.stride as u32,
                    &D2D1_BITMAP_PROPERTIES {
                        pixelFormat: pixel_format(D2D1_ALPHA_MODE_IGNORE),
                        dpiX: 96.0,
                        dpiY: 96.0,
                    },
                )?
            };
            self.preview = Some((Arc::downgrade(&pixels.data), bitmap));
        }
        let source = rect(
            (area.left - snapshot.area.left) as f32,
            (area.top - snapshot.area.top) as f32,
            (area.right - area.left) as f32,
            (area.bottom - area.top) as f32,
        );
        unsafe {
            self.context.DrawBitmap(
                &self.preview.as_ref().unwrap().1,
                Some(&bounds(area)),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                Some(&source),
            )
        };
        Ok(())
    }

    pub fn fill(&self, area: RECT, rgb: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.context.FillRectangle(&bounds(area), &self.brush);
        }
    }

    pub fn rounded(&self, area: RECT, radius: i32, fill: u32, stroke: u32) {
        let mut shape = D2D1_ROUNDED_RECT {
            rect: bounds(area),
            radiusX: radius as f32,
            radiusY: radius as f32,
        };
        unsafe {
            self.brush.SetColor(&color(fill, 1.0));
            self.context.FillRoundedRectangle(&shape, &self.brush);
            if stroke != fill {
                shape.rect.left += 0.5;
                shape.rect.top += 0.5;
                shape.rect.right -= 0.5;
                shape.rect.bottom -= 0.5;
                self.brush.SetColor(&color(stroke, 1.0));
                self.context
                    .DrawRoundedRectangle(&shape, &self.brush, 1.0, None);
            }
        }
    }

    pub fn push_clip(&self, area: RECT) {
        unsafe {
            self.context
                .PushAxisAlignedClip(&bounds(area), D2D1_ANTIALIAS_MODE_ALIASED)
        };
    }

    pub fn pop_clip(&self) {
        unsafe { self.context.PopAxisAlignedClip() };
    }

    pub fn line(&self, points: &[POINT], rgb: u32) {
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            for edge in points.windows(2) {
                self.context.DrawLine(
                    point(edge[0], 0.5),
                    point(edge[1], 0.5),
                    &self.brush,
                    1.0,
                    None,
                );
            }
        }
    }

    pub fn highlight(&self, points: [POINT; 4], rgb: u32, alpha: f32) {
        let [a, b, c, d] = points;
        if a.y == b.y && c.y == d.y && a.x == d.x && b.x == c.x {
            self.fill(
                RECT {
                    left: a.x,
                    top: a.y,
                    right: c.x,
                    bottom: c.y,
                },
                rgb,
                alpha,
            );
            return;
        }
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(point(a, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLines(&[point(b, 0.0), point(c, 0.0), point(d, 0.0)]);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_ok() {
                self.brush.SetColor(&color(rgb, alpha));
                self.context.FillGeometry(&path, &self.brush, None);
            }
        }
    }

    pub fn text(&mut self, area: RECT, label: &str, size: i32, rgb: u32) {
        unsafe {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.formats.entry(size) {
                let Ok(format) = self.write.CreateTextFormat(
                    w!("Microsoft YaHei UI"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size as f32,
                    w!(""),
                ) else {
                    return;
                };
                let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                entry.insert(format);
            }
            self.brush.SetColor(&color(rgb, 1.0));
            let text: Vec<_> = label.encode_utf16().collect();
            self.context.DrawText(
                &text,
                &self.formats[&size],
                &bounds(area),
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    pub fn icon(&mut self, center: POINT, icon: OcrIcon, edge: u32, rgb: u32, rotation: u8) {
        let key = (icon, edge, rgb, rotation);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.icons.entry(key) {
            let Some(pixels) = icon.raster(edge) else {
                return;
            };
            // Right-angle animation frames reuse the supersampled coverage without interpolation.
            let last = edge - 1;
            let tinted: Vec<u32> = (0..edge * edge)
                .map(|index| {
                    let (x, y) = (index % edge, index / edge);
                    let (x, y) = match rotation {
                        1 => (y, last - x),
                        2 => (last - x, last - y),
                        3 => (last - y, x),
                        _ => (x, y),
                    };
                    let alpha = pixels[(y * edge + x) as usize] >> 24;
                    let channel =
                        |shift: u32| ((((rgb >> shift) & 255u32) * alpha + 127) / 255) << shift;
                    alpha << 24 | channel(16) | channel(8) | channel(0)
                })
                .collect();
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: pixel_format(D2D1_ALPHA_MODE_PREMULTIPLIED),
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let Ok(bitmap) = (unsafe {
                ID2D1RenderTarget::CreateBitmap(
                    &self.context,
                    D2D_SIZE_U {
                        width: edge,
                        height: edge,
                    },
                    Some(tinted.as_ptr().cast()),
                    edge * 4,
                    &props,
                )
            }) else {
                return;
            };
            entry.insert(bitmap);
        }
        let half = (edge / 2) as i32;
        self.bitmap(
            &self.icons[&key],
            rect(
                (center.x - half) as f32,
                (center.y - half) as f32,
                edge as f32,
                edge as f32,
            ),
        );
    }

    fn bitmap(&self, bitmap: &ID2D1Bitmap, area: D2D_RECT_F) {
        unsafe {
            ID2D1RenderTarget::DrawBitmap(
                &self.context,
                bitmap,
                Some(&area),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
        }
    }
}
