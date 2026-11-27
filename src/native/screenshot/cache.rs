use super::capture::{expand, intersect, same_rect};
use std::sync::Arc;
use windows_sys::Win32::Foundation::RECT;
use yisou::ocr::TextPage;

#[derive(Default)]
pub(super) struct Cache(Vec<Entry>);

struct Entry {
    covered: RECT,
    source: RECT,
    page: Arc<TextPage>,
}

#[derive(Clone, Copy)]
pub(super) struct Region {
    pub covered: RECT,
    pub source: RECT,
}

impl Cache {
    pub fn retain_within(&mut self, matching: Option<RECT>) {
        self.0.retain_mut(|entry| {
            if let Some(covered) = matching.and_then(|area| intersect(entry.covered, area)) {
                entry.covered = covered;
                true
            } else {
                false
            }
        });
    }

    pub fn insert(&mut self, region: Region, page: TextPage) {
        self.0 = self
            .0
            .drain(..)
            .flat_map(|entry| {
                subtract(entry.covered, region.covered)
                    .into_iter()
                    .map(move |covered| Entry {
                        covered,
                        source: entry.source,
                        page: Arc::clone(&entry.page),
                    })
            })
            .collect();
        self.0.push(Entry {
            covered: region.covered,
            source: region.source,
            page: Arc::new(page),
        });
    }

    pub fn regions(&self, area: RECT, desktop: RECT, margin: i32) -> Vec<Region> {
        let mut remaining = vec![area];
        for entry in &self.0 {
            remaining = remaining
                .into_iter()
                .flat_map(|rect| subtract(rect, entry.covered))
                .collect();
        }
        let mut regions: Vec<Region> = Vec::new();
        for rect in remaining {
            let mut refresh = vec![expand(rect, margin, area)];
            for region in &regions {
                refresh = refresh
                    .into_iter()
                    .flat_map(|rect| subtract(rect, region.covered))
                    .collect();
            }
            // Context outside the current selection lets OCR locate a complete glyph
            // even when the user has only included part of it.
            regions.extend(refresh.into_iter().map(|covered| Region {
                covered,
                source: expand(covered, margin, desktop),
            }));
        }
        regions
    }

    pub fn page(&self, area: RECT) -> TextPage {
        if let Some(entry) = self
            .0
            .iter()
            .find(|entry| same_rect(entry.covered, area) && same_rect(entry.source, area))
        {
            return (*entry.page).clone();
        }
        TextPage::crop_cached(
            self.0
                .iter()
                .filter(|entry| intersect(entry.covered, area).is_some())
                .map(|entry| {
                    let covered = entry.covered;
                    (
                        entry.page.as_ref(),
                        [
                            (entry.source.left - area.left) as f32,
                            (entry.source.top - area.top) as f32,
                        ],
                        [
                            (covered.left - area.left) as f32,
                            (covered.top - area.top) as f32,
                            (covered.right - area.left) as f32,
                            (covered.bottom - area.top) as f32,
                        ],
                    )
                }),
            [
                (area.right - area.left) as f32,
                (area.bottom - area.top) as f32,
            ],
        )
    }
}

fn subtract(area: RECT, covered: RECT) -> Vec<RECT> {
    let Some(overlap) = intersect(area, covered) else {
        return vec![area];
    };
    [
        RECT {
            bottom: overlap.top,
            ..area
        },
        RECT {
            top: overlap.bottom,
            ..area
        },
        RECT {
            left: area.left,
            right: overlap.left,
            ..overlap
        },
        RECT {
            left: overlap.right,
            right: area.right,
            ..overlap
        },
    ]
    .into_iter()
    .filter(|rect| rect.left < rect.right && rect.top < rect.bottom)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn shrinking_and_reexpanding_reuse_the_interior() {
        let mut cache = Cache::default();
        let old = rect(10, 10, 30, 30);
        cache.insert(
            Region {
                covered: old,
                source: old,
            },
            TextPage::default(),
        );
        assert!(cache.regions(rect(11, 12, 29, 27), old, 2).is_empty());
        let expanded = rect(5, 5, 35, 35);
        let new = cache.regions(expanded, expanded, 2);
        assert_eq!(new.len(), 4);
        assert_eq!(
            new.iter()
                .map(|r| (r.covered.right - r.covered.left) * (r.covered.bottom - r.covered.top))
                .sum::<i32>(),
            644
        );
        for (index, &region) in new.iter().enumerate() {
            assert!(intersect(region.source, old).is_some());
            assert!(intersect(region.source, rect(14, 14, 26, 26)).is_none());
            assert!(
                new[..index]
                    .iter()
                    .all(|earlier| intersect(region.covered, earlier.covered).is_none())
            );
            cache.insert(region, TextPage::default());
        }
        assert!(cache.regions(expanded, expanded, 2).is_empty());
    }

    #[test]
    fn real_ocr_omits_a_cut_letter_and_recovers_it_after_expansion() {
        check_real_ocr_resize(
            include_bytes!("../../../tests/fixtures/ocr-text-at-bottom.png"),
            // x = 202 runs through the 'o' in "docs".
            rect(20, 50, 202, 118),
            rect(20, 50, 265, 118),
            "Read the d",
            "Read the docs",
        );
    }

    #[test]
    fn real_ocr_omits_cut_chinese_characters_on_both_edges() {
        let image = include_bytes!("../../../tests/fixtures/ocr-chinese-windows-error.png");
        let expanded = rect(0, 0, 335, 32);
        let full_text = "系统找不到指定路径。错误码0x80070003";
        // x = 131 runs through 路, and x = 51 runs through 找.
        check_real_ocr_resize(
            image,
            rect(0, 0, 131, 32),
            expanded,
            "系统找不到指定",
            full_text,
        );
        check_real_ocr_resize(
            image,
            rect(51, 0, 335, 32),
            expanded,
            "不到指定路径。错误码0x80070003",
            full_text,
        );
    }

    fn check_real_ocr_resize(
        image: &[u8],
        initial: RECT,
        expanded: RECT,
        partial_text: &str,
        full_text: &str,
    ) {
        use std::path::Path;
        use yisou::ocr::{Model, PixelFormat, Pixels, Recognizer};

        let Some(directory) = std::env::var_os("YISOU_TEST_OCR_MODELS") else {
            return;
        };
        let mut recognizer =
            Recognizer::open_directory(Path::new(&directory), Model::Tiny).unwrap();
        let image = image::load_from_memory(image).unwrap().into_rgb8();
        let desktop = rect(0, 0, image.width() as i32, image.height() as i32);
        let mut cache = Cache::default();
        let mut recognize = |cache: &mut Cache, area: RECT| {
            let regions = cache.regions(area, desktop, 48);
            for region in &regions {
                let src = region.source;
                let width = (src.right - src.left) as u32;
                let height = (src.bottom - src.top) as u32;
                let pixels = image::imageops::crop_imm(
                    &image,
                    src.left as u32,
                    src.top as u32,
                    width,
                    height,
                )
                .to_image()
                .into_raw();
                let page = recognizer
                    .recognize_text_page(Pixels {
                        width,
                        height,
                        stride: width as usize * 3,
                        format: PixelFormat::Rgb8,
                        bottom_up: false,
                        data: pixels.into(),
                    })
                    .unwrap();
                cache.insert(*region, page);
            }
            regions
        };
        assert_eq!(recognize(&mut cache, initial).len(), 1);
        assert_eq!(cache.page(initial).text(None), partial_text);
        let regions = recognize(&mut cache, expanded);
        assert_eq!(regions.len(), 1);
        assert!(
            intersect(regions[0].source, initial)
                .is_some_and(|overlap| !same_rect(overlap, initial))
        );
        assert_eq!(cache.page(expanded).text(None), full_text);
        assert!(recognize(&mut cache, initial).is_empty());
        assert_eq!(cache.page(initial).text(None), partial_text);
        assert!(recognize(&mut cache, expanded).is_empty());
        assert_eq!(cache.page(expanded).text(None), full_text);
    }
}
