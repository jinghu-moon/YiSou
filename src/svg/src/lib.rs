use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{roxmltree, ImageHrefResolver, Options, Tree};
use std::{borrow::Cow, panic::catch_unwind, slice, str};

const SAMPLES: usize = 16;
const SVG_NS: &str = "http://www.w3.org/2000/svg";

fn with_viewport<'a>(
    source: &'a str,
    root: roxmltree::Node<'_, '_>,
    edge: u32,
    color: Option<u32>,
) -> Cow<'a, str> {
    let viewport = root.has_attribute("viewBox");
    if !viewport && color.is_none() {
        return Cow::Borrowed(source);
    }

    let mut attributes = Vec::new();
    if viewport {
        attributes.push(("width", edge.to_string()));
        attributes.push(("height", edge.to_string()));
    }
    if let Some(rgb) = color {
        attributes.push(("color", format!("#{rgb:06x}")));
    }

    // Update only the root attributes. Preserve CSS selectors, namespaces and element IDs.
    let insert = root.range().start
        + source[root.range().start..]
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .expect("parsed root element has an end");
    let mut edits = Vec::new();
    let mut appended = String::new();
    for (name, value) in attributes {
        let attribute = format!("{name}=\"{value}\"");
        if let Some(original) = root.attribute_node(name) {
            edits.push((original.range(), attribute));
        } else {
            appended.push(' ');
            appended.push_str(&attribute);
        }
    }
    let mut normalized = source.to_owned();
    edits.sort_unstable_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, replacement) in edits {
        normalized.replace_range(range, &replacement);
    }
    normalized.insert_str(insert, &appended);
    Cow::Owned(normalized)
}

fn paint(svg: &[u8], size: usize, color: Option<u32>, output: &mut [u32]) -> Option<()> {
    let source = str::from_utf8(svg).ok()?;
    let document = roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    if !root.has_tag_name((SVG_NS, "svg")) {
        return None;
    }

    let edge = (size * SAMPLES) as u32;
    let options = Options {
        image_href_resolver: ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..Default::default()
        },
        ..Default::default()
    };
    let tree = match with_viewport(source, root, size as u32, color) {
        Cow::Borrowed(_) => Tree::from_xmltree(&document, &options).ok()?,
        Cow::Owned(normalized) => Tree::from_str(&normalized, &options).ok()?,
    };
    if tree.root().children().is_empty() {
        return None;
    }
    let native = tree.size();
    let scale = edge as f32 / native.width().max(native.height());
    let x = (edge as f32 - native.width() * scale) / 2.0;
    let y = (edge as f32 - native.height() * scale) / 2.0;
    let viewport = Transform::from_scale(scale, scale).post_translate(x, y);
    let mut canvas = Pixmap::new(edge, edge)?;
    resvg::render(&tree, viewport, &mut canvas.as_mut());
    average(canvas.data(), output, size);
    Some(())
}

fn average(rgba: &[u8], output: &mut [u32], size: usize) {
    let stride = size * SAMPLES * 4;
    let divisor = (SAMPLES * SAMPLES) as u32;
    for (target_row, source_rows) in output
        .chunks_exact_mut(size)
        .zip(rgba.chunks_exact(stride * SAMPLES))
    {
        for (x, target) in target_row.iter_mut().enumerate() {
            let offset = x * SAMPLES * 4;
            let mut sum = [0u32; 4];
            for row in source_rows.chunks_exact(stride) {
                for pixel in row[offset..offset + SAMPLES * 4].chunks_exact(4) {
                    for (total, value) in sum.iter_mut().zip(pixel) {
                        *total += u32::from(*value);
                    }
                }
            }
            let [r, g, b, a] = sum.map(|channel| (channel + divisor / 2) / divisor);
            *target = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }
}

/// Rasterize self-contained UTF-8 SVG to top-down premultiplied BGRA.
/// The caller owns both buffers and guarantees they are readable or writable
/// for the declared lengths and do not overlap.
#[no_mangle]
pub unsafe extern "C" fn yisou_svg_render(
    svg: *const u8,
    length: usize,
    size: u32,
    rgb: u32,
    output: *mut u32,
    count: usize,
) -> u32 {
    let size = size as usize;
    if svg.is_null()
        || output.is_null()
        || length == 0
        || length > 64 * 1024
        || !(1..=256).contains(&size)
        || count != size * size
    {
        return 0;
    }
    let color = (rgb <= 0x00ff_ffff).then_some(rgb);
    let render = || {
        let svg = unsafe { slice::from_raw_parts(svg, length) };
        let output = unsafe { slice::from_raw_parts_mut(output, count) };
        paint(svg, size, color, output)
    };
    u32::from(matches!(catch_unwind(render), Ok(Some(()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_premultiplied_channels_then_reorders_to_bgra() {
        let mut pixels = vec![0; SAMPLES * SAMPLES * 4];
        for row in pixels.chunks_exact_mut(SAMPLES * 4) {
            for pixel in row[..SAMPLES * 2].chunks_exact_mut(4) {
                pixel.copy_from_slice(&[240, 120, 16, 240]);
            }
        }
        let mut output = [0];
        average(&pixels, &mut output, 1);
        assert_eq!(output, [0x78783c08]);
    }

    #[test]
    fn rounds_half_up_and_handles_multiple_rows_and_columns() {
        let size = 2;
        let mut pixels = Vec::new();
        for y in 0..2 * SAMPLES {
            for x in 0..2 * SAMPLES {
                let alpha = 40 + 4 * (y / SAMPLES) + 2 * (x / SAMPLES) + (x % 2);
                pixels.extend_from_slice(&[0, 0, 0, alpha as u8]);
            }
        }
        let mut output = [0; 4];
        average(&pixels, &mut output, size);
        assert_eq!(output, [0x29000000, 0x2b000000, 0x2d000000, 0x2f000000]);
    }
}
