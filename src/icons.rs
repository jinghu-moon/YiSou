use crate::engine::{Engine, Icon};
use image::{ImageReader, imageops::FilterType};
use std::{collections::VecDeque, io::Cursor};

#[path = "icon_sources.rs"]
mod icon_sources;

include!(concat!(env!("OUT_DIR"), "/icons_index.rs"));
const RASTER_ICONS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icons.bin"));

fn precomputed_icon(id: &str, edge: u32) -> Option<Vec<u32>> {
    let compressed = RASTER_ICONS.get(icon_range(id, edge)?)?;
    let size = (edge * edge * 4) as usize;
    let bytes = miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, size).ok()?;
    (bytes.len() == size).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes)
            .collect()
    })
}

pub fn chevron_icon(edge: u32, color: u32) -> Option<Vec<u32>> {
    (color == icon_sources::CHEVRON_COLOR)
        .then(|| precomputed_icon("chevron-down", edge))
        .flatten()
        .or_else(|| {
            yisou_svg::render(
                include_bytes!("../assets/icons/tabler/chevron-down.svg"),
                edge,
                Some(color),
            )
        })
}

macro_rules! embedded_svg {
    ($($name:literal),+ $(,)?) => {
        fn embedded_svg(id: &str) -> Option<&'static [u8]> {
            match id {
                $($name => Some(include_bytes!(concat!("../assets/icons/", $name, ".svg"))),)+
                _ => None,
            }
        }
    };
}

icon_sources::engine_icons!(embedded_svg);

macro_rules! embedded_color_svg {
    ($($name:literal),+ $(,)?) => {
        fn embedded_color_svg(id: &str) -> Option<(&'static str, &'static [u8])> {
            match id {
                $($name => Some((concat!("color:", $name),
                    include_bytes!(concat!("../assets/icons/color/", $name, ".svg")))),)+
                _ => None,
            }
        }
    };
}

icon_sources::color_engine_icons!(embedded_color_svg);

macro_rules! ocr_icon {
    ($($variant:ident, $name:literal, $size:literal;)+) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub enum OcrIcon {
            $($variant,)+
        }

        impl OcrIcon {
            pub fn size(self) -> i32 {
                match self {
                    $(Self::$variant => $size,)+
                }
            }

            fn source(self) -> (&'static str, &'static [u8]) {
                match self {
                    $(Self::$variant => (concat!("ocr:", $name), include_bytes!(concat!("../assets/icons/tabler/", $name, ".svg"))),)+
                }
            }

            pub fn raster(self, edge: u32) -> Option<Vec<u32>> {
                let (id, source) = self.source();
                precomputed_icon(id, edge).or_else(|| yisou_svg::render(source, edge, None))
            }
        }
    };
}

icon_sources::ocr_icons!(ocr_icon);

pub fn import_svg(bytes: Vec<u8>) -> Option<Icon> {
    let svg = String::from_utf8(bytes).ok()?;
    yisou_svg::render(svg.as_bytes(), 32, None)?;
    Some(Icon::Svg(svg))
}

pub fn import_bitmap(bytes: &[u8]) -> Option<Icon> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    let pixels = image.pixels().map(|pixel| {
        let [r, g, b, a] = pixel.0;
        let premultiply = |value| ((u32::from(value) * u32::from(a) + 127) / 255) as u8;
        image::Rgba([premultiply(r), premultiply(g), premultiply(b), a])
    });
    let premultiplied = image::RgbaImage::from_vec(
        image.width(),
        image.height(),
        pixels.flat_map(|pixel| pixel.0).collect(),
    )?;
    let fitted = image::DynamicImage::ImageRgba8(premultiplied)
        .resize(32, 32, FilterType::Triangle)
        .into_rgba8();
    let mut output = vec![0; 32 * 32];
    let left = (32 - fitted.width()) / 2;
    let top = (32 - fitted.height()) / 2;
    for (x, y, pixel) in fitted.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        output[((top + y) * 32 + left + x) as usize] = u32::from_le_bytes([b, g, r, a]);
    }
    Some(Icon::Bgra32(output))
}

struct CachedIcon {
    id: String,
    icon: Icon,
    color: u32,
    multicolor: bool,
    tint: Option<u32>,
    edge: u32,
    pixels: Vec<u32>,
}

#[derive(Default)]
pub struct IconCache {
    entries: VecDeque<CachedIcon>,
    bytes: usize,
}

impl IconCache {
    pub fn get(&mut self, engine: &Engine, edge: u32) -> Option<&[u32]> {
        self.get_variant(engine, edge, false)
    }

    pub fn get_variant(&mut self, engine: &Engine, edge: u32, multicolor: bool) -> Option<&[u32]> {
        const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;
        if !(1..=256).contains(&edge) {
            return None;
        }
        let icon = engine.icon_for(multicolor);
        let tint = engine.mono_tint(multicolor);
        let hit = self.entries.iter().position(|entry| {
            entry.edge == edge
                && entry.id == engine.id
                && entry.color == engine.color
                && entry.multicolor == multicolor
                && entry.tint == tint
                && entry.icon == *icon
        });
        if let Some(index) = hit {
            let entry = self.entries.remove(index)?;
            self.entries.push_front(entry);
        } else {
            let mut pixels = match icon {
                Icon::Default => {
                    let (key, source) = multicolor
                        .then(|| embedded_color_svg(&engine.id))
                        .flatten()
                        .or_else(|| Some((engine.id.as_str(), embedded_svg(&engine.id)?)))?;
                    precomputed_icon(key, edge)
                        .or_else(|| yisou_svg::render_engine_icon(source, edge))?
                }
                Icon::Svg(svg) => yisou_svg::render(svg.as_bytes(), edge, Some(engine.color))?,
                Icon::Bgra32(pixels) => {
                    let bitmap = image::RgbaImage::from_vec(
                        32,
                        32,
                        pixels.iter().flat_map(|p| p.to_le_bytes()).collect(),
                    )?;
                    image::imageops::resize(&bitmap, edge, edge, FilterType::Triangle)
                        .pixels()
                        .map(|pixel| u32::from_le_bytes(pixel.0))
                        .collect()
                }
            };
            if let Some(rgb) = tint {
                let [b, g, r, _] = rgb.to_le_bytes();
                for pixel in &mut pixels {
                    let alpha = (*pixel >> 24) as u8;
                    let premultiply =
                        |channel| ((u32::from(channel) * u32::from(alpha) + 127) / 255) as u8;
                    *pixel =
                        u32::from_le_bytes([premultiply(b), premultiply(g), premultiply(r), alpha]);
                }
            }
            let bytes = pixels.len() * size_of::<u32>()
                + match icon {
                    Icon::Svg(svg) => svg.len(),
                    Icon::Bgra32(pixels) => pixels.len() * size_of::<u32>(),
                    Icon::Default => 0,
                };
            while self.bytes + bytes > MAX_CACHE_BYTES {
                let entry = self.entries.pop_back()?;
                self.bytes -= entry.pixels.len() * size_of::<u32>()
                    + match entry.icon {
                        Icon::Svg(svg) => svg.len(),
                        Icon::Bgra32(pixels) => pixels.len() * size_of::<u32>(),
                        Icon::Default => 0,
                    };
            }
            self.bytes += bytes;
            self.entries.push_front(CachedIcon {
                id: engine.id.clone(),
                icon: icon.clone(),
                color: engine.color,
                multicolor,
                tint,
                edge,
                pixels,
            });
        }
        Some(&self.entries.front()?.pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible_long_edge(pixels: &[u32], edge: usize) -> usize {
        let (mut left, mut top, mut right, mut bottom) = (edge, edge, 0, 0);
        for (index, pixel) in pixels.iter().enumerate() {
            if pixel >> 24 == 0 {
                continue;
            }
            let (x, y) = (index % edge, index / edge);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
        right.saturating_sub(left).max(bottom.saturating_sub(top))
    }

    #[test]
    fn bundled_icons_match_runtime_rasterization() {
        for dpi in [96, 120, 144, 168, 192] {
            for engine in crate::engine::built_in_engines() {
                let mono = (engine.id.as_str(), embedded_svg(&engine.id).unwrap());
                for (id, source) in std::iter::once(mono).chain(embedded_color_svg(&engine.id)) {
                    for size in [20, 24, 28, 36] {
                        let edge = ((size * dpi + 48) / 96) as u32;
                        let pixels = precomputed_icon(id, edge).unwrap();
                        assert_eq!(
                            pixels,
                            yisou_svg::render_engine_icon(source, edge).unwrap(),
                            "{id} at {edge}px"
                        );
                        let long_edge = visible_long_edge(&pixels, edge as usize);
                        assert!(
                            long_edge >= edge as usize - 1,
                            "{id} at {edge}px: {long_edge}"
                        );
                    }
                }
            }
            let edge = ((16 * dpi + 48) / 96) as u32;
            assert_eq!(
                precomputed_icon("chevron-down", edge).unwrap(),
                yisou_svg::render(
                    include_bytes!("../assets/icons/tabler/chevron-down.svg"),
                    edge,
                    Some(icon_sources::CHEVRON_COLOR),
                )
                .unwrap()
            );
        }
    }

    #[test]
    fn bundled_ocr_icons_match_runtime_rasterization() {
        for icon in [
            OcrIcon::Copy,
            OcrIcon::Search,
            OcrIcon::Close,
            OcrIcon::Check,
            OcrIcon::Scan,
            OcrIcon::Loader,
        ] {
            let (id, source) = icon.source();
            for dpi in [96, 120, 144, 168, 192, 240] {
                let edge = ((icon.size() * dpi + 48) / 96) as u32;
                let reference = yisou_svg::render(source, edge, None).unwrap();
                assert!(
                    reference
                        .iter()
                        .any(|pixel| (1..255).contains(&(pixel >> 24)))
                );
                if dpi <= 192 {
                    assert_eq!(
                        precomputed_icon(id, edge).as_deref(),
                        Some(reference.as_slice())
                    );
                }
                assert_eq!(icon.raster(edge), Some(reference), "{id} at {dpi} DPI");
            }
        }
    }
}
