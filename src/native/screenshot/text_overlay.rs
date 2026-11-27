use std::ops::RangeInclusive;
use windows_sys::Win32::Foundation::{POINT, RECT};
use yisou::ocr::{Quad, TextPage};

use super::{capture::Snapshot, render::Canvas};

pub(super) struct TextOverlay {
    page: TextPage,
    outlines: Vec<TextOutline>,
    hovered: Option<usize>,
    selected: Option<RangeInclusive<usize>>,
    drag_start: Option<(usize, POINT)>,
    dragged: bool,
}

impl TextOverlay {
    pub fn new(page: TextPage, snapshot: &Snapshot, area: RECT) -> Self {
        let points = line_outlines(page.line_boxes().map(|(_, quad)| quad));
        let outlines = page
            .line_boxes()
            .zip(points)
            .map(|((line, _), points)| {
                let left = points[3];
                let right = points[2];
                let dark = snapshot.dark_at(
                    area.left + left.x.midpoint(right.x),
                    area.top + left.y.midpoint(right.y),
                );
                TextOutline { line, points, dark }
            })
            .collect();
        Self {
            page,
            outlines,
            hovered: None,
            selected: None,
            drag_start: None,
            dragged: false,
        }
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selected
            .as_ref()
            .map(|range| self.page.text(Some(range)))
    }

    pub fn text_cursor(&self, area: RECT, point: POINT) -> bool {
        self.drag_start.is_some() || self.page.hit(local(area, point)).is_some()
    }

    pub fn down(&mut self, area: RECT, point: POINT) {
        let hit = self.page.hit(local(area, point));
        self.selected = hit.map(|hit| self.page.word(hit));
        self.drag_start = hit.map(|unit| (unit, point));
        self.dragged = false;
    }

    pub fn drag(&mut self, area: RECT, point: POINT) -> bool {
        let Some((first, anchor)) = self.drag_start else {
            return false;
        };
        self.dragged |= (anchor.x - point.x).abs() > 3 || (anchor.y - point.y).abs() > 3;
        if !self.dragged {
            return false;
        }
        let Some(last) = self.page.nearest(local(area, point)) else {
            return false;
        };
        let next = Some(first.min(last)..=first.max(last));
        if next == self.selected {
            return false;
        }
        self.selected = next;
        true
    }

    pub fn release(&mut self) {
        self.drag_start = None;
    }

    pub fn hover(&mut self, area: RECT, point: Option<POINT>) -> [Option<RECT>; 2] {
        let next = point.and_then(|point| self.page.line_at(local(area, point)));
        let previous = std::mem::replace(&mut self.hovered, next);
        if previous == next {
            return [None, None];
        }
        [previous, next].map(|line| {
            let outline = self
                .outlines
                .iter()
                .find(|outline| Some(outline.line) == line)?;
            Some(RECT {
                left: area.left + outline.points.iter().map(|point| point.x).min()? - 1,
                top: area.top + outline.points.iter().map(|point| point.y).min()? - 1,
                right: area.left + outline.points.iter().map(|point| point.x).max()? + 2,
                bottom: area.top + outline.points.iter().map(|point| point.y).max()? + 2,
            })
        })
    }

    pub fn paint(&self, canvas: &Canvas, area: RECT) {
        canvas.push_clip(area);
        let mut highlights = self
            .selected
            .iter()
            .flat_map(|range| self.page.highlights(range))
            .peekable();
        for outline in &self.outlines {
            let points = outline.points.map(|point| POINT {
                x: area.left + point.x,
                y: area.top + point.y,
            });
            let [a, b, c, d] = points;
            while highlights
                .peek()
                .is_some_and(|(line, _)| *line < outline.line)
            {
                highlights.next();
            }
            let selected = if highlights
                .peek()
                .is_some_and(|(line, _)| *line == outline.line)
            {
                highlights
                    .next()
                    .map(|(_, quad)| outline_points(area, quad, false))
            } else {
                None
            };
            if selected.is_none() && self.hovered == Some(outline.line) && self.drag_start.is_none()
            {
                canvas.line(
                    &[a, b, c, d, a],
                    if outline.dark { 0x90b9f6 } else { 0x618cc9 },
                );
                continue;
            }
            let underline_color = if outline.dark { 0x607999 } else { 0x9faec4 };
            if let Some(mut selected) = selected {
                if a.y == b.y && c.y == d.y {
                    let left = selected[0].x.min(selected[3].x).clamp(a.x, b.x);
                    let right = selected[1].x.max(selected[2].x).clamp(left, b.x);
                    let underline = |left, right| {
                        if left < right {
                            canvas.line(
                                &[POINT { x: left, y: d.y }, POINT { x: right, y: c.y }],
                                underline_color,
                            );
                        }
                    };
                    underline(d.x, left);
                    underline(right, c.x);
                    selected = [
                        POINT { x: left, y: a.y },
                        POINT { x: right, y: b.y },
                        POINT { x: right, y: c.y },
                        POINT { x: left, y: d.y },
                    ];
                }
                let (color, opacity) = if outline.dark {
                    (0x4589f8, 87.0 / 255.0)
                } else {
                    (0x3175e7, 61.0 / 255.0)
                };
                canvas.highlight(selected, color, opacity);
            } else {
                canvas.line(&[d, c], underline_color);
            }
        }
        canvas.pop_clip();
    }
}

struct TextOutline {
    line: usize,
    points: [POINT; 4],
    dark: bool,
}

fn local(area: RECT, point: POINT) -> [f32; 2] {
    [(point.x - area.left) as f32, (point.y - area.top) as f32]
}

fn line_outlines(lines: impl Iterator<Item = Quad>) -> Vec<[POINT; 4]> {
    let mut outlines = Vec::new();
    let mut horizontal = Vec::new();
    for line in lines {
        let points = outline_points(RECT::default(), line, true);
        let [a, b, c, d] = points;
        if a.y == b.y && c.y == d.y && a.x == d.x && b.x == c.x {
            horizontal.push((outlines.len(), a, c));
        }
        outlines.push(points);
    }
    horizontal.sort_unstable_by_key(|&(_, top, bottom)| (top.y + bottom.y) / 2);

    // OCR boxes for one visual row often differ by a few pixels because ascenders,
    // descenders, punctuation, and anti-aliasing affect the detector polygon. Use the
    // median height and center of each row so the displayed outlines form a stable grid.
    let mut rows: Vec<Vec<(usize, POINT, POINT)>> = Vec::new();
    for item in horizontal {
        let center = (item.1.y + item.2.y) / 2;
        let height = item.2.y - item.1.y;
        if rows.last().is_some_and(|row| {
            let row_center = (row[0].1.y + row[0].2.y) / 2;
            let row_height = row[0].2.y - row[0].1.y;
            (center - row_center).abs() <= (height.min(row_height) * 2 / 5).max(2)
        }) {
            rows.last_mut().unwrap().push(item);
        } else {
            rows.push(vec![item]);
        }
    }

    let mut row_bounds = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut heights: Vec<_> = row
            .iter()
            .map(|&(_, top, bottom)| bottom.y - top.y)
            .collect();
        heights.sort_unstable();
        let target_height = heights[heights.len() / 2].max(2);
        let mut centers: Vec<_> = row
            .iter()
            .map(|&(_, top, bottom)| (top.y + bottom.y) / 2)
            .collect();
        centers.sort_unstable();
        let center = centers[centers.len() / 2];
        let top = center - target_height / 2;
        let bottom = top + target_height;
        row_bounds.push((top, bottom, center));
        for &(index, _, _) in row {
            outlines[index][0].y = top;
            outlines[index][1].y = top;
            outlines[index][2].y = bottom;
            outlines[index][3].y = bottom;
        }
    }

    // Keep neighboring rows visibly separate. The horizontal span is deliberately
    // ignored here, so two columns at the same y stay independent while every row
    // receives the same vertical treatment.
    for index in 0..row_bounds.len().saturating_sub(1) {
        let (_, upper_bottom, upper_center) = row_bounds[index];
        let (lower_top, _, lower_center) = row_bounds[index + 1];
        if lower_top <= upper_bottom + 1 {
            let divider = (upper_center + lower_center) / 2;
            for &(line, _, _) in &rows[index] {
                outlines[line][2].y = upper_bottom.min(divider - 1);
                outlines[line][3].y = outlines[line][2].y;
            }
            for &(line, _, _) in &rows[index + 1] {
                outlines[line][0].y = lower_top.max(divider + 1);
                outlines[line][1].y = outlines[line][0].y;
            }
        }
    }
    outlines
}

fn outline_points(area: RECT, quad: Quad, padded: bool) -> [POINT; 4] {
    // Short screen text can produce slightly rotated OCR boxes. Snap near-horizontal
    // edges for display, keeping the original character geometry for selection.
    const MAX_HORIZONTAL_SLOPE: f32 = 0.1763; // tan(10 degrees)
    let near_horizontal = |[x1, y1]: [f32; 2], [x2, y2]: [f32; 2]| {
        x2 > x1 && (y2 - y1).abs() <= (x2 - x1) * MAX_HORIZONTAL_SLOPE
    };
    let [a, b, c, d] = quad;
    let quad = if near_horizontal(a, b) && near_horizontal(d, c) {
        let mut left = a[0].min(d[0]);
        let mut right = b[0].max(c[0]);
        let mut top = a[1].min(b[1]);
        let mut bottom = c[1].max(d[1]);
        if padded {
            let height = bottom - top;
            let horizontal = (height * 0.125).clamp(2.0, 8.0);
            let vertical = (height * 0.05).clamp(1.0, 3.0);
            left = (left - horizontal).floor();
            right = (right + horizontal).ceil();
            top = (top - vertical).floor();
            bottom = (bottom + vertical).ceil();
        }
        [[left, top], [right, top], [right, bottom], [left, bottom]]
    } else {
        quad
    };
    quad.map(|[x, y]| POINT {
        x: area.left + x.round() as i32,
        y: area.top + y.round() as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_rows_leave_a_gap_without_joining_columns() {
        let line =
            |left, top, right, bottom| [[left, top], [right, top], [right, bottom], [left, bottom]];
        let short = line(0.0, 20.0, 35.0, 38.0);
        let right = line(120.0, 19.0, 190.0, 39.0);
        let lower = line(8.0, 36.0, 90.0, 56.0);
        let bottom = line(25.0, 54.0, 80.0, 74.0);
        let outlines = line_outlines([lower, right, bottom, short].into_iter());
        assert_eq!(outlines[0][0].y - outlines[3][2].y, 2);
        assert_eq!(outlines[2][0].y - outlines[0][2].y, 2);
        assert_eq!(
            outlines[1][2].y - outlines[1][0].y,
            outlines[3][2].y - outlines[3][0].y
        );
        assert_eq!((outlines[3][0].x, outlines[3][2].x), (-3, 38));
    }

    #[test]
    fn rows_use_a_common_height_within_each_visual_row() {
        let line =
            |left, top, right, bottom| [[left, top], [right, top], [right, bottom], [left, bottom]];
        let outlines = line_outlines(
            [
                line(0.0, 10.0, 40.0, 25.0),
                line(80.0, 11.0, 120.0, 28.0),
                line(5.0, 40.0, 70.0, 61.0),
            ]
            .into_iter(),
        );
        assert_eq!(
            outlines[0][2].y - outlines[0][0].y,
            outlines[1][2].y - outlines[1][0].y
        );
        assert!(outlines[2][0].y - outlines[0][2].y >= 2);
    }

    #[test]
    fn text_outlines_snap_small_slopes_and_preserve_rotated_text() {
        let area = RECT {
            left: 100,
            top: 200,
            right: 400,
            bottom: 500,
        };
        let points = |quad| outline_points(area, quad, false).map(|point| (point.x, point.y));
        assert_eq!(
            points([[11.0, 20.0], [40.0, 22.0], [39.0, 36.0], [10.0, 34.0]]),
            [(110, 220), (140, 220), (140, 236), (110, 236)]
        );
        assert_eq!(
            points([[10.0, 22.0], [39.0, 20.0], [40.0, 34.0], [11.0, 36.0]]),
            [(110, 220), (140, 220), (140, 236), (110, 236)]
        );
        assert_eq!(
            points([[15.0, 20.0], [40.0, 32.0], [35.0, 44.0], [10.0, 32.0]]),
            [(115, 220), (140, 232), (135, 244), (110, 232)]
        );
        let padded = outline_points(
            area,
            [[11.5, 20.5], [40.5, 22.5], [39.5, 36.5], [10.5, 34.5]],
            true,
        );
        assert_eq!(
            padded.map(|point| (point.x, point.y)),
            [(108, 219), (143, 219), (143, 238), (108, 238)]
        );
    }
}
