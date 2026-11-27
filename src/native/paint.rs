use super::{smooth_corner::CornerShape, wide};
use std::{borrow::Cow, collections::HashMap, mem::ManuallyDrop};
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
    Graphics::Gdi::{DC_BRUSH, FillRect, GetStockObject, HDC as Win32Dc, SetDCBrushColor},
};
use yisou::engine::Engine;

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

pub(super) fn blend(a: u32, b: u32, t: f32) -> u32 {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    [0_u32, 8, 16].into_iter().fold(0, |rgb, shift| {
        let left = (a >> shift & 255) as f32;
        let right = (b >> shift & 255) as f32;
        rgb | (((left + (right - left) * t).round() as u32) << shift)
    })
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
    smooth_outline: Option<(D2D_RECT_F, CornerShape, ID2D1Geometry)>,
}

pub(super) struct HistoryText {
    layout: IDWriteTextLayout,
    matches: Vec<DWRITE_HIT_TEST_METRICS>,
    pub(super) height: i32,
}

impl HistoryText {
    pub(super) fn new(
        painter: &mut Painter,
        text: &str,
        width: f32,
        keyword: &str,
    ) -> Option<Self> {
        let (layout, height) = painter.wrapped_text(text, width)?;
        let mut matches = Vec::new();
        let keyword = keyword.trim().to_lowercase();
        if !keyword.is_empty() {
            let lower = text.to_lowercase();
            // DirectWrite positions are UTF-16 offsets in the original text. A lowercase
            // character can expand, so offsets in the folded string need to be mapped back.
            let mut positions = Vec::with_capacity(text.chars().count() + 1);
            let (mut byte, mut unit) = (0, 0);
            for ch in text.chars() {
                positions.push((byte, unit));
                byte += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                unit += ch.len_utf16() as u32;
            }
            positions.push((byte, unit));
            let unit_at = |offset| {
                let i = positions.partition_point(|&(byte, _)| byte < offset);
                let start = positions[i.saturating_sub(1)].1;
                let &(byte, end) = positions.get(i).unwrap_or(positions.last().unwrap());
                (if byte == offset { end } else { start }, end)
            };
            for (byte, _) in lower.match_indices(&keyword) {
                let (start, _) = unit_at(byte);
                let (_, end) = unit_at(byte + keyword.len());
                let mut count = 0;
                unsafe {
                    let _ = layout.HitTestTextRange(start, end - start, 0.0, 0.0, None, &mut count);
                    if count == 0 {
                        continue;
                    }
                    let tail = matches.len();
                    matches.resize(tail + count as usize, DWRITE_HIT_TEST_METRICS::default());
                    if layout
                        .HitTestTextRange(
                            start,
                            end - start,
                            0.0,
                            0.0,
                            Some(&mut matches[tail..]),
                            &mut count,
                        )
                        .is_err()
                    {
                        matches.truncate(tail);
                    }
                }
            }
        }
        Some(Self {
            layout,
            matches,
            height: height.ceil() as i32,
        })
    }

    pub(super) fn draw(&self, painter: &Painter, x: f32, y: f32, color: u32, highlight: u32) {
        self.draw_with_highlight(painter, (x, y), color, (highlight, 0.19));
    }

    pub(super) fn draw_with_highlight(
        &self,
        painter: &Painter,
        (x, y): (f32, f32),
        color: u32,
        (highlight, opacity): (u32, f32),
    ) {
        for hit in &self.matches {
            painter.rounded_alpha(
                rect(
                    x + hit.left,
                    y + hit.top + 1.0,
                    hit.width,
                    (hit.height - 2.0).max(0.0),
                ),
                2.0,
                highlight,
                opacity,
                false,
            );
        }
        painter.draw_text_layout(x, y, &self.layout, color);
    }
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
                smooth_outline: None,
            })
        }
    }

    pub(super) fn family_text_format(
        &self,
        family: &str,
        collection: &IDWriteFontCollection,
    ) -> Result<IDWriteTextFormat> {
        let family = wide(family);
        Self::text_format(
            &self.factory,
            PCWSTR(family.as_ptr()),
            Some(collection),
            Label::Body,
        )
    }

    pub(super) fn text_format(
        factory: &IDWriteFactory,
        family: PCWSTR,
        collection: Option<&IDWriteFontCollection>,
        label: Label,
    ) -> Result<IDWriteTextFormat> {
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
            let font = factory.CreateTextFormat(
                family,
                collection,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )?;
            font.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            font.SetTextAlignment(alignment)?;
            font.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let ellipsis = factory.CreateEllipsisTrimmingSign(&font)?;
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

    pub(super) fn with_clip(&mut self, area: D2D_RECT_F, draw: impl FnOnce(&mut Self)) {
        unsafe {
            self.target
                .PushAxisAlignedClip(&area, D2D1_ANTIALIAS_MODE_ALIASED)
        };
        draw(self);
        unsafe { self.target.PopAxisAlignedClip() };
    }

    pub(super) fn svg_icon(
        &mut self,
        (id, svg): (&str, &[u8]),
        (x, y): (f32, f32),
        size: f32,
        dpi: u32,
        rgb: u32,
    ) {
        let edge = (size * dpi as f32 / 96.0).round().max(1.0) as u32;
        let key = format!("ui-{id}-{rgb:06x}");
        self.image(&key, rect(x, y, size, size), edge, || {
            yisou_svg::render(svg, edge, Some(rgb)).map(Cow::Owned)
        });
    }

    pub fn rounded(&self, area: D2D_RECT_F, radius: f32, rgb: u32, stroke: bool) {
        self.rounded_alpha(area, radius, rgb, 1.0, stroke);
    }

    pub(super) fn smooth_border(
        &mut self,
        width: f32,
        height: f32,
        radius: i32,
        pixel: f32,
        color: u32,
    ) {
        let inset = pixel / 2.0;
        let corner = CornerShape::new(radius as f32, width, height);
        self.smooth_rectangle(
            rect(
                inset,
                inset,
                (width - pixel).max(0.0),
                (height - pixel).max(0.0),
            ),
            corner.inset(inset),
            None,
            color,
            pixel,
        );
    }

    pub(super) fn smooth_rectangle(
        &mut self,
        area: D2D_RECT_F,
        corner: CornerShape,
        fill: Option<u32>,
        border: u32,
        width: f32,
    ) {
        unsafe {
            if self
                .smooth_outline
                .as_ref()
                .is_none_or(|(bounds, shape, _)| *bounds != area || *shape != corner)
            {
                let Ok(geometry) = self
                    .target
                    .GetFactory()
                    .and_then(|factory| super::smooth_corner::geometry(&factory, area, corner))
                else {
                    return;
                };
                self.smooth_outline = Some((area, corner, geometry));
            }
            let (_, _, geometry) = self.smooth_outline.as_ref().unwrap();
            if let Some(fill) = fill {
                self.brush.SetColor(&color(fill, 1.0));
                self.target
                    .FillGeometry(geometry, &self.brush, None::<&ID2D1Brush>);
            }
            self.brush.SetColor(&color(border, 1.0));
            self.target.DrawGeometry(geometry, &self.brush, width, None);
        }
    }

    pub(super) fn with_rounded_clip(&self, area: D2D_RECT_F, radius: f32, draw: impl FnOnce()) {
        unsafe {
            let Ok(mask) = self.target.GetFactory().and_then(|factory| {
                factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                    rect: area,
                    radiusX: radius,
                    radiusY: radius,
                })
            }) else {
                return;
            };
            let parameters = D2D1_LAYER_PARAMETERS {
                contentBounds: area,
                geometricMask: ManuallyDrop::new(Some(mask.into())),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: windows_numerics::Matrix3x2::identity(),
                opacity: 1.0,
                ..Default::default()
            };
            self.target.PushLayer(&parameters, None);
            draw();
            self.target.PopLayer();
            drop(ManuallyDrop::into_inner(parameters.geometricMask));
        }
    }

    pub(super) fn gradient(
        &self,
        area: D2D_RECT_F,
        radius: f32,
        vertical: bool,
        stops: &[D2D1_GRADIENT_STOP],
    ) {
        let start = windows_numerics::Vector2 {
            X: area.left,
            Y: area.top,
        };
        let end = if vertical {
            windows_numerics::Vector2 {
                X: area.left,
                Y: area.bottom,
            }
        } else {
            windows_numerics::Vector2 {
                X: area.right,
                Y: area.top,
            }
        };
        unsafe {
            let Ok(stops) = self.target.CreateGradientStopCollection(
                stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            ) else {
                return;
            };
            if let Ok(brush) = self.target.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: start,
                    endPoint: end,
                },
                None,
                &stops,
            ) {
                self.target.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: area,
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &brush,
                );
            }
        }
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

    fn prepare_format(&mut self, label: Label) {
        let index = label as usize;
        if self.formats[index].is_none() {
            self.formats[index] = Self::text_format(
                &self.factory,
                PCWSTR(self.family.as_ptr()),
                self.collection.as_ref(),
                label,
            )
            .ok();
        }
    }

    pub(super) fn text_width(&mut self, text: &str, label: Label) -> Option<f32> {
        self.prepare_format(label);
        let format = self.formats[label as usize].as_ref()?;
        self.text_units.clear();
        self.text_units.extend(text.encode_utf16());
        Self::measure_text(&self.factory, format, &self.text_units).ok()
    }

    pub(super) fn measure_text(
        factory: &IDWriteFactory,
        format: &IDWriteTextFormat,
        units: &[u16],
    ) -> Result<f32> {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe {
            factory
                .CreateTextLayout(units, format, 32_768.0, 32.0)?
                .GetMetrics(&mut metrics)?;
        }
        Ok(metrics.widthIncludingTrailingWhitespace)
    }

    pub(super) fn wrapped_text(
        &mut self,
        text: &str,
        width: f32,
    ) -> Option<(IDWriteTextLayout, f32)> {
        self.prepare_format(Label::Body);
        let format = self.formats[Label::Body as usize].as_ref()?;
        self.text_units.clear();
        self.text_units.extend(text.encode_utf16());
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(&self.text_units, format, width, 0.0)
                .ok()?;
            layout
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)
                .ok()?;
            layout
                .SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK)
                .ok()?;
            layout
                .SetTrimming(&DWRITE_TRIMMING::default(), None::<&IDWriteInlineObject>)
                .ok()?;
            layout.GetMetrics(&mut metrics).ok()?;
            layout.SetMaxHeight(metrics.height).ok()?;
            Some((layout, metrics.height))
        }
    }

    pub(super) fn draw_text_layout(&self, x: f32, y: f32, layout: &IDWriteTextLayout, rgb: u32) {
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            self.target.DrawTextLayout(
                windows_numerics::Vector2 { X: x, Y: y },
                layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    pub fn text(&mut self, text: &str, area: D2D_RECT_F, rgb: u32, label: Label) {
        self.text_alpha(text, area, rgb, 1.0, label);
    }

    fn text_alpha(&mut self, text: &str, area: D2D_RECT_F, rgb: u32, opacity: f32, label: Label) {
        self.prepare_format(label);
        let Some(format) = self.formats[label as usize].as_ref() else {
            return;
        };
        Self::draw_text(
            &self.target,
            &self.brush,
            &mut self.text_units,
            text,
            area,
            color(rgb, opacity),
            format,
        );
    }

    pub(super) fn text_with_format(
        &mut self,
        text: &str,
        area: D2D_RECT_F,
        rgb: u32,
        format: &IDWriteTextFormat,
    ) {
        Self::draw_text(
            &self.target,
            &self.brush,
            &mut self.text_units,
            text,
            area,
            color(rgb, 1.0),
            format,
        );
    }

    fn draw_text(
        target: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        units: &mut Vec<u16>,
        text: &str,
        area: D2D_RECT_F,
        fill: D2D1_COLOR_F,
        format: &IDWriteTextFormat,
    ) {
        units.clear();
        units.extend(text.encode_utf16());
        unsafe {
            brush.SetColor(&fill);
            target.DrawText(
                units,
                format,
                &area,
                brush,
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
        self.image_alpha(id, area, edge, 1.0, pixels);
    }

    fn image_alpha<'a>(
        &mut self,
        id: &str,
        area: D2D_RECT_F,
        edge: u32,
        opacity: f32,
        pixels: impl FnOnce() -> Option<Cow<'a, [u32]>>,
    ) {
        let key = (id.to_owned(), edge);
        if let Some(bitmap) = self.images.get(&key) {
            unsafe {
                self.target.DrawBitmap(
                    bitmap,
                    Some(&area),
                    opacity,
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
                    opacity,
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
            unsafe {
                SetDCBrushColor(dc, super::color_ref(SURFACE));
                FillRect(dc, bounds, GetStockObject(DC_BRUSH));
            }
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
        let multicolor = self
            .app
            .borrow()
            .settings()
            .preferences
            .multicolor_engine_icons;
        self.paint_engine_variant(painter, engine, (x, y), size, self.dpi.get(), multicolor);
    }

    pub(super) fn paint_engine_variant(
        &self,
        painter: &mut Painter,
        engine: &Engine,
        (x, y): (f32, f32),
        size: i32,
        dpi: u32,
        multicolor: bool,
    ) {
        self.paint_engine_variant_alpha(painter, engine, (x, y), size, dpi, multicolor, 1.0);
    }

    fn paint_engine_variant_alpha(
        &self,
        painter: &mut Painter,
        engine: &Engine,
        (x, y): (f32, f32),
        size: i32,
        dpi: u32,
        multicolor: bool,
        opacity: f32,
    ) {
        let edge = super::dip(size, dpi).max(1) as u32;
        let dpi = dpi as f32;
        let align = |v: f32| (v * dpi / 96.0).round() * 96.0 / dpi;
        let extent = edge as f32 * 96.0 / dpi;
        let area = rect(align(x), align(y), extent, extent);
        if (engine.is_custom() || engine.id.is_empty()) && engine.icon_for(multicolor).is_default()
        {
            painter.rounded_alpha(
                area,
                5.0,
                engine.mono_tint(multicolor).unwrap_or(engine.color),
                opacity,
                false,
            );
            painter.text_alpha(
                &engine.name.chars().take(1).collect::<String>(),
                area,
                0xffffff,
                opacity,
                Label::Tile,
            );
            return;
        }
        let mut images = self.images.borrow_mut();
        let key = format!(
            "{}:{}:{:?}",
            if multicolor { "color" } else { "mono" },
            engine.id,
            engine.mono_tint(multicolor),
        );
        painter.image_alpha(&key, area, edge, opacity, || {
            images
                .get_variant(engine, edge, multicolor)
                .map(Cow::Borrowed)
        });
    }

    pub(super) fn paint_search_icon(
        &self,
        painter: &mut Painter,
        engine: &Engine,
        position: (f32, f32),
        size: i32,
        dpi: u32,
        prefs: &yisou::settings::Preferences,
    ) {
        if prefs.multicolor_engine_icons {
            self.paint_engine_variant(painter, engine, position, size, dpi, true);
            return;
        }
        let opacity = prefs.search_mono_icon_opacity();
        if opacity == 0.0 {
            return;
        }
        let tint = prefs.search_mono_icon_color.or_else(|| {
            (self.resolve_search_colors(prefs, false).bright_foreground
                && engine.mono_color.is_none()
                && !(engine.is_custom() && engine.icon_for(false).is_default()))
            .then_some(0xf5f5f5)
        });
        if let Some(tint) = tint {
            let mut tinted_engine = engine.clone();
            tinted_engine.mono_color = Some(tint);
            self.paint_engine_variant_alpha(
                painter,
                &tinted_engine,
                position,
                size,
                dpi,
                false,
                opacity,
            );
        } else {
            self.paint_engine_variant_alpha(painter, engine, position, size, dpi, false, opacity);
        }
    }
}
