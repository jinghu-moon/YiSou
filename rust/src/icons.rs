use crate::engine::{Engine, Icon};
use image::{ImageReader, imageops::FilterType};
use std::{collections::VecDeque, io::Cursor};

macro_rules! embedded {
    ($name:literal) => {
        include_bytes!(concat!("../../assets/icons/", $name, ".svg"))
    };
}

fn embedded_svg(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "amazon" => embedded!("amazon"),
        "baidu" => embedded!("baidu"),
        "bilibili" => embedded!("bilibili"),
        "bing" => embedded!("bing"),
        "github" => embedded!("github"),
        "google" => embedded!("google"),
        "reddit" => embedded!("reddit"),
        "stackoverflow" => embedded!("stackoverflow"),
        "taobao" => embedded!("taobao"),
        "youtube" => embedded!("youtube"),
        "zhihu" => embedded!("zhihu"),
        _ => return None,
    })
}

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
                Icon::Default => yisou_svg::render(embedded_svg(&engine.id)?, edge, None)?,
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
