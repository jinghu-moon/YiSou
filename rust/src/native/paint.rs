use super::wide;
use std::{borrow::Cow, collections::HashMap};
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC,
        },
    },
    core::{Interface, PCWSTR, Result, w},
};
use windows_sys::Win32::{
    Foundation::RECT as Win32Rect,
    Graphics::Gdi::{FillRect, HDC as Win32Dc},
};
use yisou::engine::{Engine, Icon};

pub const SURFACE: u32 = 0xfcfdff;
pub const RAIL: u32 = 0xf1f3f7;
pub const BORDER: u32 = 0xdde2ea;
pub const TEXT: u32 = 0x1a2029;
pub const MUTED: u32 = 0x536273;
pub const ACCENT: u32 = 0x005fb8;
pub const HOVER: u32 = 0xebf0f7;
pub const SELECTED: u32 = 0xe6eef9;

pub(super) fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.0,
        g: ((rgb >> 8) & 255) as f32 / 255.0,
        b: (rgb & 255) as f32 / 255.0,
        a: alpha,
    }
}

pub fn rect(left: f32, top: f32, width: f32, height: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

pub(super) fn pixel_format(alpha: D2D1_ALPHA_MODE) -> D2D1_PIXEL_FORMAT {
    D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: alpha,
    }
}

#[derive(Clone, Copy)]
pub enum Label {
    Body,
    Tile,
    Alias,
    Hint,
    Control,
    ControlDetail,
}

pub struct Painter {
    target: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
    factory: IDWriteFactory,
    family: Vec<u16>,
    collection: Option<IDWriteFontCollection>,
    formats: [Option<IDWriteTextFormat>; 6],
    text_units: Vec<u16>,
    images: HashMap<(String, u32), ID2D1Bitmap>,
}

impl Painter {
    pub fn new(family: &str, collection: Option<IDWriteFontCollection>) -> Result<Self> {
        // The device and formats belong to the GUI thread.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: pixel_format(D2D1_ALPHA_MODE_IGNORE),
                ..Default::default()
            })?;
            Self::for_target(target.into(), family, collection)
        }
    }

    pub(super) fn for_target(
        target: ID2D1RenderTarget,
        family: &str,
        collection: Option<IDWriteFontCollection>,
    ) -> Result<Self> {
        unsafe {
            let brush = target.CreateSolidColorBrush(&color(TEXT, 1.0), None)?;
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            Ok(Self {
                target,
                brush,
                factory,
                family: wide(family),
                collection,
                formats: Default::default(),
                text_units: Vec::new(),
                images: HashMap::new(),
            })
        }
    }

    fn text_format(&self, label: Label) -> Result<IDWriteTextFormat> {
        let (size, weight, alignment) = match label {
            Label::Body => (
                14.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            ),
            Label::Tile => (
                13.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            ),
            Label::Alias => (
                11.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            ),
            Label::Hint => (
                12.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            ),
            Label::Control => (
                14.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            ),
            Label::ControlDetail => (
                12.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            ),
        };
        unsafe {
            let font = self.factory.CreateTextFormat(
                PCWSTR(self.family.as_ptr()),
                self.collection.as_ref(),
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )?;
            font.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            font.SetTextAlignment(alignment)?;
            font.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let ellipsis = self.factory.CreateEllipsisTrimmingSign(&font)?;
            font.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    ..Default::default()
                },
                &ellipsis,
            )?;
            Ok(font)
        }
    }

    pub fn begin(&self, dc: HDC, bounds: &RECT, dpi: u32) -> Result<()> {
        unsafe {
            self.target
                .cast::<ID2D1DCRenderTarget>()?
                .BindDC(dc, bounds)?;
            self.target.SetDpi(dpi as f32, dpi as f32);
            self.target.BeginDraw();
            Ok(())
        }
    }

    pub fn end(&self) -> Result<()> {
        unsafe { self.target.EndDraw(None, None) }
    }

    pub fn clear(&self, rgb: u32) {
        unsafe { self.target.Clear(Some(&color(rgb, 1.0))) };
    }

    pub fn rounded(&self, area: D2D_RECT_F, radius: f32, rgb: u32, stroke: bool) {
        self.rounded_alpha(area, radius, rgb, 1.0, stroke);
    }

    pub(super) fn rounded_alpha(
        &self,
        area: D2D_RECT_F,
        radius: f32,
        rgb: u32,
        alpha: f32,
        stroke: bool,
    ) {
        let rounded = D2D1_ROUNDED_RECT {
            rect: area,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            if stroke {
                self.target
                    .DrawRoundedRectangle(&rounded, &self.brush, 1.0, None);
            } else {
                self.target.FillRoundedRectangle(&rounded, &self.brush);
            }
        }
    }

    pub fn text(&mut self, text: &str, area: D2D_RECT_F, rgb: u32, label: Label) {
        let index = label as usize;
        if self.formats[index].is_none() {
            self.formats[index] = self.text_format(label).ok();
        }
        let Some(format) = self.formats[index].as_ref() else {
            return;
        };
        self.text_units.clear();
        self.text_units.extend(text.encode_utf16());
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            self.target.DrawText(
                &self.text_units,
                format,
                &area,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    pub fn image<'a>(
        &mut self,
        id: &str,
        area: D2D_RECT_F,
        edge: u32,
        pixels: impl FnOnce() -> Option<Cow<'a, [u32]>>,
    ) {
        let key = (id.to_owned(), edge);
        if let Some(bitmap) = self.images.get(&key) {
            unsafe {
                self.target.DrawBitmap(
                    bitmap,
                    Some(&area),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                )
            };
            return;
        }
        let Some(pixels) = pixels() else { return };
        let bitmap = unsafe {
            self.target.CreateBitmap(
                D2D_SIZE_U {
                    width: edge,
                    height: edge,
                },
                Some(pixels.as_ptr().cast()),
                edge * 4,
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: pixel_format(D2D1_ALPHA_MODE_PREMULTIPLIED),
                    dpiX: 96.0,
                    dpiY: 96.0,
                },
            )
        };
        if let Ok(bitmap) = bitmap {
            unsafe {
                self.target.DrawBitmap(
                    &bitmap,
                    Some(&area),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                )
            };
            if self.images.len() >= 128 {
                self.images.clear();
            }
            self.images.insert(key, bitmap);
        }
    }

    pub fn reset_images(&mut self) {
        self.images.clear();
    }
}

impl super::State {
    pub(super) fn paint(&self, dc: Win32Dc, bounds: &Win32Rect, draw: impl FnOnce(&mut Painter)) {
        if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
            return;
        }
        let mut painter = self.painter.borrow_mut();
        let creating = painter.is_none();
        if creating {
            let (family, collection) = self.drawing_font();
            *painter = Painter::new(&family, collection).ok();
        }
        let Some(target) = painter.as_mut() else {
            unsafe { FillRect(dc, bounds, self.background) };
            return;
        };
        let bounds = windows::Win32::Foundation::RECT {
            left: bounds.left,
            right: bounds.right,
            top: bounds.top,
            bottom: bounds.bottom,
        };
        if target
            .begin(
                windows::Win32::Graphics::Gdi::HDC(dc),
                &bounds,
                self.dpi.get(),
            )
            .is_err()
        {
            *painter = None;
            return;
        }
        draw(target);
        if target.end().is_err() {
            *painter = None;
        }
        if creating {
            self.diagnostics.sample("first_paint");
        }
    }

    pub(super) fn paint_engine(
        &self,
        painter: &mut Painter,
        engine: &Engine,
        x: f32,
        y: f32,
        size: i32,
    ) {
        let dpi = self.dpi.get() as f32;
        let align = |v: f32| (v * dpi / 96.0).round() * 96.0 / dpi;
        let edge = self.scale(size).max(1) as u32;
        let extent = edge as f32 * 96.0 / dpi;
        let area = rect(align(x), align(y), extent, extent);
        if engine.is_custom() && matches!(engine.icon, Icon::Default) {
            painter.rounded(area, 5.0, engine.color, false);
            painter.text(
                &engine.name.chars().take(1).collect::<String>(),
                area,
                0xffffff,
                Label::Tile,
            );
            return;
        }
        let mut images = self.images.borrow_mut();
        painter.image(&engine.id, area, edge, || {
            images.get(engine, edge).map(Cow::Borrowed)
        });
    }
}
