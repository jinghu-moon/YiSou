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
                include_bytes!("../../assets/icons/tabler/chevron-down.svg"),
                edge,
                Some(color),
            )
        })
}

macro_rules! embedded_svg {
    ($($name:literal),+ $(,)?) => {
        fn embedded_svg(id: &str) -> Option<&'static [u8]> {
            match id {
                $($name => Some(include_bytes!(concat!("../../assets/icons/", $name, ".svg"))),)+
                _ => None,
            }
        }
    };
}

icon_sources::engine_icons!(embedded_svg);

#[cfg(feature = "ocr")]
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
                    $(Self::$variant => ($name, include_bytes!(concat!("../../assets/icons/tabler/", $name, ".svg"))),)+
                }
            }

            pub fn raster(self, edge: u32) -> Option<Vec<u32>> {
                let (id, source) = self.source();
                precomputed_icon(id, edge).or_else(|| yisou_svg::render(source, edge, None))
            }
        }
    };
}

#[cfg(feature = "ocr")]
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
    engine: Engine,
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
        const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;
        if !(1..=256).contains(&edge) {
            return None;
        }
        let hit = self.entries.iter().position(|entry| {
            entry.edge == edge
                && entry.engine.id == engine.id
                && entry.engine.color == engine.color
                && entry.engine.icon == engine.icon
        });
        if let Some(index) = hit {
            let entry = self.entries.remove(index)?;
            self.entries.push_front(entry);
        } else {
            let pixels = match &engine.icon {
                Icon::Default => precomputed_icon(&engine.id, edge)
                    .or_else(|| yisou_svg::render(embedded_svg(&engine.id)?, edge, None))?,
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
            let bytes = pixels.len() * size_of::<u32>()
                + match &engine.icon {
                    Icon::Svg(svg) => svg.len(),
                    Icon::Bgra32(pixels) => pixels.len() * size_of::<u32>(),
                    Icon::Default => 0,
                };
            while self.bytes + bytes > MAX_CACHE_BYTES {
                let entry = self.entries.pop_back()?;
                self.bytes -= entry.pixels.len() * size_of::<u32>()
                    + match entry.engine.icon {
                        Icon::Svg(svg) => svg.len(),
                        Icon::Bgra32(pixels) => pixels.len() * size_of::<u32>(),
                        Icon::Default => 0,
                    };
            }
            self.bytes += bytes;
            self.entries.push_front(CachedIcon {
                engine: engine.clone(),
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

    #[test]
    fn bundled_icons_match_runtime_rasterization() {
        for dpi in [96, 120, 144, 168, 192] {
            for engine in crate::engine::built_in_engines() {
                let source = embedded_svg(&engine.id).unwrap();
                for size in [20, 24, 28, 36] {
                    let edge = (size * dpi + 48) / 96;
                    assert_eq!(
                        precomputed_icon(&engine.id, edge).unwrap(),
                        yisou_svg::render(source, edge, None).unwrap(),
                        "{} at {edge}px",
                        engine.id
                    );
                }
            }
            let edge = (16 * dpi + 48) / 96;
            assert_eq!(
                precomputed_icon("chevron-down", edge).unwrap(),
                yisou_svg::render(
                    include_bytes!("../../assets/icons/tabler/chevron-down.svg"),
                    edge,
                    Some(icon_sources::CHEVRON_COLOR),
                )
                .unwrap()
            );
        }
    }

    #[cfg(feature = "ocr")]
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
