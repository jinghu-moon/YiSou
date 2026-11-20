use rapid_structure_rs::{OcrOutput, OcrWord};
use std::ops::{Range, RangeInclusive};

pub type Quad = [[f32; 2]; 4];

#[derive(Clone, Default)]
pub struct TextPage {
    text: String,
    units: Vec<Unit>,
    lines: Vec<Option<Quad>>,
}

#[derive(Clone)]
struct Unit {
    bytes: Range<usize>,
    quad: Quad,
    line: usize,
}

impl TextPage {
    /// Select characters by region ownership, then translate them into the current crop.
    pub fn crop_cached<'a>(
        pages: impl IntoIterator<Item = (&'a Self, [f32; 2], [f32; 4])>,
        size: [f32; 2],
    ) -> Self {
        let mut lines = Vec::new();
        for (page, offset, [left, top, right, bottom]) in pages {
            let translate = |quad: Quad| quad.map(|[x, y]| [x + offset[0], y + offset[1]]);
            for line in page.units.chunk_by(|a, b| a.line == b.line) {
                let visible = |unit: &Unit| {
                    let quad = translate(unit.quad);
                    let x = quad.iter().map(|point| point[0]).sum::<f32>() / 4.0;
                    let y = quad.iter().map(|point| point[1]).sum::<f32>() / 4.0;
                    x >= left
                        && x < right
                        && y >= top
                        && y < bottom
                        && quad.into_iter().all(|[x, y]| {
                            x >= -0.01 && y >= -0.01 && x <= size[0] + 0.01 && y <= size[1] + 0.01
                        })
                };
                for units in line
                    .split(|unit| !visible(unit))
                    .filter(|units| !units.is_empty())
                {
                    let first = &units[0];
                    let last = &units[units.len() - 1];
                    let a = translate(first.quad);
                    let b = translate(last.quad);
                    let quad = [a[0], b[1], b[2], a[3]];
                    // Character boxes are for selection. A complete line can use its
                    // detector box, which accounts for glyphs extending beyond those boxes.
                    let outline = if units.len() == line.len() {
                        page.lines[first.line].map(translate).unwrap_or(quad)
                    } else {
                        quad
                    };
                    lines.push((page, units, quad, offset, outline));
                }
            }
        }
        let vertical_center =
            |quad: Quad| (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
        let same_row = |a: Quad, b: Quad| {
            let height =
                |quad: Quad| ((quad[3][1] - quad[0][1]) + (quad[2][1] - quad[1][1])).abs() / 2.0;
            (vertical_center(a) - vertical_center(b)).abs() <= 0.4 * height(a).min(height(b))
        };
        lines.sort_by(|a, b| vertical_center(a.2).total_cmp(&vertical_center(b.2)));
        let mut first = 0;
        while first < lines.len() {
            let end = first
                + 1
                + lines[first + 1..]
                    .iter()
                    .take_while(|line| same_row(lines[first].2, line.2))
                    .count();
            lines[first..end].sort_by(|a, b| a.2[0][0].total_cmp(&b.2[0][0]));
            first = end;
        }
        let mut result = Self::default();
        let mut previous: Option<Quad> = None;
        for (page, units, quad, offset, outline) in lines {
            if let Some(previous) = previous {
                if !same_row(previous, quad) {
                    result.text.push('\n');
                } else {
                    let gap = ((quad[0][0] + quad[3][0]) - (previous[1][0] + previous[2][0])) / 2.0;
                    let height = (quad[3][1] - quad[0][1]).abs();
                    if gap >= height * 0.12 {
                        result.text.push(' ');
                    }
                }
            }
            previous = Some(quad);
            let start = result.text.len();
            let original = units[0].bytes.start;
            result
                .text
                .push_str(&page.text[original..units[units.len() - 1].bytes.end]);
            let line = result.lines.len();
            result.lines.push(Some(outline));
            result.units.extend(units.iter().map(|unit| Unit {
                bytes: start + unit.bytes.start - original..start + unit.bytes.end - original,
                quad: unit.quad.map(|[x, y]| [x + offset[0], y + offset[1]]),
                line,
            }));
        }
        result
    }

    pub(super) fn from_output(output: OcrOutput) -> Self {
        let mut page = Self {
            text: String::new(),
            units: Vec::new(),
            lines: Vec::new(),
        };
        for index in output.reading_order() {
            let region = &output.regions[index];
            if let Some(recognition) = &region.recognition {
                page.push_line(
                    &recognition.text,
                    region.polygon.map(|p| p.points),
                    recognition.words.as_deref(),
                );
            }
        }
        page
    }

    fn push_line(&mut self, text: &str, quad: Option<Quad>, words: Option<&[OcrWord]>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        let start = self.text.len();
        self.text.push_str(text);
        let line = self.lines.len();
        self.lines.push(quad);
        if let Some(words) = words {
            // CTC boxes can omit spaces. Match against the original string to keep spelling and spacing.
            let mut cursor = 0;
            let mut units = Vec::with_capacity(words.len());
            for word in words {
                if word.text.trim().is_empty() {
                    continue;
                }
                cursor = text.len() - text[cursor..].trim_start().len();
                if word.text.is_empty() || !text[cursor..].starts_with(&word.text) {
                    break;
                }
                let end = cursor + word.text.len();
                units.push(Unit {
                    bytes: start + cursor..start + end,
                    quad: word.polygon.points,
                    line,
                });
                cursor = end;
            }
            if !units.is_empty() && text[cursor..].trim().is_empty() {
                self.units.extend(units);
                return;
            }
        }
        if let Some(quad) = quad {
            self.units.push(Unit {
                bytes: start..self.text.len(),
                quad,
                line,
            });
        }
    }

    pub fn text(&self, selected: Option<&RangeInclusive<usize>>) -> &str {
        selected.map_or(self.text.as_str(), |range| {
            &self.text[self.units[*range.start()].bytes.start..self.units[*range.end()].bytes.end]
        })
    }

    pub fn lines(&self) -> impl Iterator<Item = Quad> + '_ {
        self.lines.iter().flatten().copied()
    }

    pub fn line_boxes(&self) -> impl Iterator<Item = (usize, Quad)> + '_ {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(index, quad)| quad.map(|quad| (index, quad)))
    }

    pub fn line_at(&self, point: [f32; 2]) -> Option<usize> {
        self.hit(point).map(|index| self.units[index].line)
    }

    pub fn hit(&self, point: [f32; 2]) -> Option<usize> {
        self.units
            .iter()
            .position(|unit| contains(unit.quad, point))
    }

    pub fn nearest(&self, point: [f32; 2]) -> Option<usize> {
        self.hit(point).or_else(|| {
            let mut start = 0;
            let mut best = None;
            for line in self.units.chunk_by(|a, b| a.line == b.line) {
                let (offset, distance) = line
                    .iter()
                    .map(|unit| distance(unit.quad, point))
                    .enumerate()
                    .min_by(|(_, a), (_, b)| a.total_cmp(b))
                    .unwrap();
                let quad = self.lines[line[0].line].unwrap_or(line[offset].quad);
                let center_y = quad.iter().map(|p| p[1]).sum::<f32>() / 4.0;
                let score = ((center_y - point[1]).abs(), distance);
                if best.is_none_or(|(best_score, _)| score < best_score) {
                    best = Some((score, start + offset));
                }
                start += line.len();
            }
            best.map(|(_, index)| index)
        })
    }

    pub fn word(&self, index: usize) -> RangeInclusive<usize> {
        let adjacent = |left: usize, right: usize| {
            let a = &self.units[left].bytes;
            let b = &self.units[right].bytes;
            a.end == b.start && self.text[a.clone()].is_ascii() && self.text[b.clone()].is_ascii()
        };
        let mut first = index;
        let mut last = index;
        while first > 0 && adjacent(first - 1, first) {
            first -= 1;
        }
        while last + 1 < self.units.len() && adjacent(last, last + 1) {
            last += 1;
        }
        first..=last
    }

    pub fn highlights(&self, range: &RangeInclusive<usize>) -> impl Iterator<Item = (usize, Quad)> {
        self.units[range.clone()]
            .chunk_by(|a, b| a.line == b.line)
            .map(|line| {
                let first = line.first().unwrap().quad;
                let last = line.last().unwrap().quad;
                (line[0].line, [first[0], last[1], last[2], first[3]])
            })
    }
}

fn contains(quad: Quad, point: [f32; 2]) -> bool {
    let mut positive = false;
    let mut negative = false;
    for (a, b) in quad.into_iter().zip(quad.into_iter().cycle().skip(1)) {
        let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        positive |= cross > 0.0;
        negative |= cross < 0.0;
    }
    positive ^ negative
}

fn distance(quad: Quad, point: [f32; 2]) -> f32 {
    let mut total = [0.0; 2];
    for [x, y] in quad {
        total[0] += x;
        total[1] += y;
    }
    (total[0] * 0.25 - point[0]).powi(2) + (total[1] * 0.25 - point[1]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapid_structure_rs::{Polygon, WordKind};

    fn quad(x: f32, y: f32) -> Quad {
        [[x, y], [x + 8.0, y], [x + 8.0, y + 8.0], [x, y + 8.0]]
    }

    #[test]
    fn clicked_words_and_cross_line_ranges_preserve_original_text() {
        let mut page = TextPage {
            text: String::new(),
            units: Vec::new(),
            lines: Vec::new(),
        };
        let chars = |text: &str, y| {
            text.chars()
                .enumerate()
                .filter(|(_, c)| !c.is_whitespace())
                .map(|(i, c)| OcrWord {
                    text: c.to_string(),
                    score: 1.0,
                    kind: WordKind::Char,
                    polygon: Polygon {
                        points: quad(i as f32 * 10.0, y),
                    },
                })
                .collect::<Vec<_>>()
        };
        page.push_line(
            "报错 E0425",
            Some(quad(0.0, 0.0)),
            Some(&chars("报错 E0425", 0.0)),
        );
        page.push_line(
            "src/main.rs",
            Some(quad(0.0, 20.0)),
            Some(&chars("src/main.rs", 20.0)),
        );
        let index = page.hit([42.0, 3.0]).unwrap();
        assert_eq!(page.text(Some(&page.word(index))), "E0425");
        assert_eq!(page.text(Some(&(1..=8))), "错 E0425\nsr");
        assert_eq!(page.highlights(&(1..=9)).count(), 2);
        assert!(page.hit([28.0, 3.0]).is_none());
        assert_eq!(page.nearest([3.0, 22.0]), Some(7));
        let end = page.nearest([150.0, 3.0]).unwrap();
        assert_eq!(page.text(Some(&(0..=end))), "报错 E0425");
    }

    #[test]
    fn a_line_without_matching_character_boxes_can_still_be_selected() {
        let mut page = TextPage {
            text: String::new(),
            units: Vec::new(),
            lines: Vec::new(),
        };
        page.push_line("文档", Some(quad(0.0, 0.0)), None);
        assert_eq!(
            page.text(Some(&page.word(page.hit([2.0, 2.0]).unwrap()))),
            "文档"
        );
    }

    #[test]
    fn partially_visible_character_is_excluded_before_and_after_expansion() {
        let mut page = TextPage::default();
        let words = ["甲", "乙"].map(|text| OcrWord {
            text: text.into(),
            score: 1.0,
            kind: WordKind::Char,
            polygon: Polygon {
                points: quad(if text == "甲" { 10.0 } else { 20.0 }, 10.0),
            },
        });
        let detected = [[8.0, 8.0], [30.0, 8.0], [30.0, 20.0], [8.0, 20.0]];
        page.push_line("甲乙", Some(detected), Some(&words));
        let crop = |width, height| {
            TextPage::crop_cached(
                [(&page, [0.0, 0.0], [0.0, 0.0, width, height])],
                [width, height],
            )
        };
        let before = crop(25.0, 30.0);
        assert_eq!(before.text(None), "甲");
        assert!(before.lines().eq([quad(10.0, 10.0)]));
        assert!(before.hit([23.0, 14.0]).is_none());
        let expanded = crop(30.0, 30.0);
        assert_eq!(expanded.text(None), "甲乙");
        assert!(expanded.lines().eq([detected]));
        let hit = expanded.hit([23.0, 14.0]).unwrap();
        assert_eq!(expanded.text(Some(&expanded.word(hit))), "乙");
        assert_eq!(crop(25.0, 30.0).text(None), "甲");
        assert_eq!(crop(30.0, 15.0).text(None), "");
    }
}
