//! Generated Hangul lettering sheets.
//!
//! An image model draws each phrase as solid black ink on white, one phrase per
//! row. The builders sample that ink into small pixel masks and shade the masks
//! with the palette-index roles measured from the original Japanese artwork, so
//! the committed sheet stays the single source of letter shape and the shading
//! stays deterministic.

use std::{io::Cursor, ops::Range};

use anyhow::{Context, Result, bail};

const INK_LUMA_THRESHOLD: u32 = 128;
const MIN_ROW_HEIGHT: usize = 40;
const ROW_MERGE_GAP: usize = 8;
const STROKE_MERGE_GAP: usize = 6;

pub struct InkSheet {
    width: usize,
    height: usize,
    ink: Vec<bool>,
}

struct InkComponent {
    columns: Range<usize>,
    x_sum: usize,
    pixels: usize,
}

/// Which ink a phrase row's syllables sample: each syllable's source columns,
/// the row-relative component index of every sheet pixel, and per syllable
/// whether it keeps each component.
struct SyllableOwnership {
    columns: Vec<Range<usize>>,
    labels: Vec<usize>,
    allowed: Vec<Vec<bool>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub bits: Vec<bool>,
}

#[derive(Clone, Copy, Debug)]
pub struct PhraseStyle {
    pub text_height: usize,
    pub max_width: usize,
    pub syllable_gap: usize,
    pub word_space: usize,
    pub coverage_threshold: f32,
}

pub fn decode_ink_sheet(encoded: &[u8]) -> Result<InkSheet> {
    let mut decoder = png::Decoder::new(Cursor::new(encoded));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .context("read lettering sheet PNG info")?;
    let mut pixels = vec![
        0u8;
        reader
            .output_buffer_size()
            .context("lettering sheet is too large")?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .context("decode lettering sheet PNG")?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => bail!("lettering sheet palette was not expanded"),
    };
    let width = info.width as usize;
    let height = info.height as usize;
    let mut ink = Vec::with_capacity(width * height);
    for pixel in pixels[..info.buffer_size()].chunks_exact(channels) {
        let (luma, alpha) = match channels {
            1 => (u32::from(pixel[0]), 255),
            2 => (u32::from(pixel[0]), pixel[1]),
            3 => (rgb_luma(pixel), 255),
            _ => (rgb_luma(pixel), pixel[3]),
        };
        ink.push(alpha >= 128 && luma < INK_LUMA_THRESHOLD);
    }
    Ok(InkSheet { width, height, ink })
}

fn rgb_luma(pixel: &[u8]) -> u32 {
    (299 * u32::from(pixel[0]) + 587 * u32::from(pixel[1]) + 114 * u32::from(pixel[2])) / 1000
}

impl InkSheet {
    fn ink_at(&self, x: usize, y: usize) -> bool {
        self.ink[y * self.width + x]
    }

    /// Phrase rows separated by blank sheet rows, top to bottom.
    pub fn rows(&self) -> Vec<Range<usize>> {
        let mut rows: Vec<Range<usize>> = Vec::new();
        for y in 0..self.height {
            if !(0..self.width).any(|x| self.ink_at(x, y)) {
                continue;
            }
            match rows.last_mut() {
                Some(row) if y - row.end < ROW_MERGE_GAP => row.end = y + 1,
                _ => rows.push(y..y + 1),
            }
        }
        rows.retain(|row| row.len() > MIN_ROW_HEIGHT);
        rows
    }

    /// Column ranges of the `count` syllables in a row. Strokes separated by a
    /// narrow gap belong to one syllable. Brush syllables can overlap in the
    /// column projection, so while too few runs remain the widest run is split
    /// at its least-inked column within the middle half; while too many
    /// remain, the closest neighbors are merged.
    pub fn syllable_columns(&self, row: &Range<usize>, count: usize) -> Result<Vec<Range<usize>>> {
        let column_ink = (0..self.width)
            .map(|x| row.clone().filter(|&y| self.ink_at(x, y)).count())
            .collect::<Vec<_>>();
        let mut runs: Vec<Range<usize>> = Vec::new();
        for (x, &ink) in column_ink.iter().enumerate() {
            if ink == 0 {
                continue;
            }
            match runs.last_mut() {
                Some(run) if x - run.end < STROKE_MERGE_GAP => run.end = x + 1,
                _ => runs.push(x..x + 1),
            }
        }
        if runs.is_empty() {
            bail!("lettering row {}..{} has no ink", row.start, row.end);
        }
        // Syllables are roughly as wide as the row is tall. Pieces of one
        // syllable (such as the circle and bar of 이) sit closer than an
        // eighth of that and together stay narrower than 1.25 times it.
        let height = row.len();
        let mut merged = true;
        while merged && runs.len() > 1 {
            merged = false;
            for index in 0..runs.len() - 1 {
                let gap = runs[index + 1].start - runs[index].end;
                let combined = runs[index + 1].end - runs[index].start;
                if gap * 8 < height && combined * 4 <= height * 5 {
                    runs[index].end = runs[index + 1].end;
                    runs.remove(index + 1);
                    merged = true;
                    break;
                }
            }
        }
        while runs.len() < count {
            let widest = (0..runs.len())
                .max_by_key(|&index| runs[index].len())
                .expect("runs are not empty");
            let run = runs[widest].clone();
            let search = run.start + run.len() / 4..run.end - run.len() / 4;
            let split = search
                .clone()
                .min_by_key(|&x| (column_ink[x], x.abs_diff(run.start + run.len() / 2)))
                .with_context(|| {
                    format!(
                        "lettering row {}..{} cannot be split into {count} syllables",
                        row.start, row.end
                    )
                })?;
            runs[widest] = run.start..split;
            runs.insert(widest + 1, split + 1..run.end);
        }
        while runs.len() > count {
            let closest = (0..runs.len() - 1)
                .min_by_key(|&index| runs[index + 1].start - runs[index].end)
                .expect("at least two runs remain");
            runs[closest].end = runs[closest + 1].end;
            runs.remove(closest + 1);
        }
        Ok(runs)
    }

    fn ink_rows(&self, columns: &Range<usize>, row: &Range<usize>) -> Result<Range<usize>> {
        let inked = row
            .clone()
            .filter(|&y| columns.clone().any(|x| self.ink_at(x, y)))
            .collect::<Vec<_>>();
        let (first, last) = inked
            .first()
            .zip(inked.last())
            .context("lettering syllable has no ink")?;
        Ok(*first..*last + 1)
    }

    /// Area-sample a source rectangle into `width` x `height` pixels. A pixel
    /// is ink when at least `threshold` of its source footprint is ink.
    pub fn sample(
        &self,
        columns: &Range<usize>,
        rows: &Range<usize>,
        width: usize,
        height: usize,
        threshold: f32,
    ) -> Mask {
        self.sample_where(columns, rows, width, height, threshold, |x, y| {
            self.ink_at(x, y)
        })
    }

    /// Eight-connected ink components inside a row band: a per-pixel
    /// component index (`usize::MAX` for no ink, row-relative) and each
    /// component's column extent and centroid.
    fn components(&self, row: &Range<usize>) -> (Vec<usize>, Vec<InkComponent>) {
        let height = row.len();
        let mut labels = vec![usize::MAX; self.width * height];
        let mut components = Vec::new();
        for start_y in 0..height {
            for start_x in 0..self.width {
                if labels[start_y * self.width + start_x] != usize::MAX
                    || !self.ink_at(start_x, row.start + start_y)
                {
                    continue;
                }
                let id = components.len();
                let mut component = InkComponent {
                    columns: start_x..start_x + 1,
                    x_sum: 0,
                    pixels: 0,
                };
                labels[start_y * self.width + start_x] = id;
                let mut stack = vec![(start_x, start_y)];
                while let Some((x, y)) = stack.pop() {
                    component.columns.start = component.columns.start.min(x);
                    component.columns.end = component.columns.end.max(x + 1);
                    component.x_sum += x;
                    component.pixels += 1;
                    for ny in y.saturating_sub(1)..(y + 2).min(height) {
                        for nx in x.saturating_sub(1)..(x + 2).min(self.width) {
                            let label = &mut labels[ny * self.width + nx];
                            if *label == usize::MAX && self.ink_at(nx, row.start + ny) {
                                *label = id;
                                stack.push((nx, ny));
                            }
                        }
                    }
                }
                components.push(component);
            }
        }
        (labels, components)
    }

    /// Column ranges and per-syllable ink ownership. The projection split
    /// from `syllable_columns` stays authoritative, except that a stroke whose
    /// centroid lies in one syllable but whose tip reaches less than a quarter
    /// of its width into a neighbour's columns (a brush flick under the next
    /// letter) stays whole with its own syllable instead of leaving a detached
    /// speck at the neighbour's edge.
    fn syllable_ownership(&self, row: &Range<usize>, count: usize) -> Result<SyllableOwnership> {
        let columns = self.syllable_columns(row, count)?;
        let (labels, components) = self.components(row);
        let owner = components
            .iter()
            .map(|component| {
                let centroid = component.x_sum / component.pixels.max(1);
                (0..columns.len())
                    .min_by_key(|&index| {
                        let column = &columns[index];
                        if column.contains(&centroid) {
                            0
                        } else {
                            column
                                .start
                                .abs_diff(centroid)
                                .min(column.end.abs_diff(centroid))
                        }
                    })
                    .expect("syllable columns are not empty")
            })
            .collect::<Vec<_>>();
        let mut extents = columns.clone();
        let allowed = (0..columns.len())
            .map(|syllable| {
                components
                    .iter()
                    .zip(&owner)
                    .map(|(component, &owner)| {
                        let column = &columns[syllable];
                        let overlap = component
                            .columns
                            .end
                            .min(column.end)
                            .saturating_sub(component.columns.start.max(column.start));
                        owner == syllable || overlap * 4 >= component.columns.len()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        for (id, (component, &owner)) in components.iter().zip(&owner).enumerate() {
            // Widen the owner only over columns no other syllable keeps, so a
            // shared stroke is never drawn twice.
            let shared = (0..columns.len()).any(|syllable| {
                syllable != owner
                    && allowed[syllable][id]
                    && component.columns.start < columns[syllable].end
                    && columns[syllable].start < component.columns.end
            });
            if !shared {
                let extent = &mut extents[owner];
                extent.start = extent.start.min(component.columns.start);
                extent.end = extent.end.max(component.columns.end);
            }
        }
        Ok(SyllableOwnership {
            columns: extents,
            labels,
            allowed,
        })
    }

    /// `sample` restricted to the source pixels `keep` accepts.
    fn sample_where(
        &self,
        columns: &Range<usize>,
        rows: &Range<usize>,
        width: usize,
        height: usize,
        threshold: f32,
        keep: impl Fn(usize, usize) -> bool,
    ) -> Mask {
        let scale_x = width as f32 / columns.len() as f32;
        let scale_y = height as f32 / rows.len() as f32;
        let mut bits = vec![false; width * height];
        for out_y in 0..height {
            let y0 = rows.start + (out_y as f32 / scale_y) as usize;
            let y1 = (rows.start + ((out_y + 1) as f32 / scale_y) as usize).min(rows.end - 1);
            for out_x in 0..width {
                let x0 = columns.start + (out_x as f32 / scale_x) as usize;
                let x1 =
                    (columns.start + ((out_x + 1) as f32 / scale_x) as usize).min(columns.end - 1);
                let mut total = 0usize;
                let mut inked = 0usize;
                for y in y0..=y1.max(y0) {
                    for x in x0..=x1.max(x0) {
                        total += 1;
                        inked += usize::from(keep(x, y));
                    }
                }
                bits[out_y * width + out_x] = inked as f32 >= threshold * total as f32;
            }
        }
        Mask {
            width,
            height,
            bits,
        }
    }

    /// Compose one phrase row at `style.text_height`. Syllables keep the row's
    /// common vertical scale; spaces in `text` become word spaces. When the
    /// natural width exceeds `style.max_width`, syllables are condensed
    /// horizontally by one shared factor. Returns the phrase mask and the
    /// x ranges of every syllable inside it.
    pub fn compose_phrase(
        &self,
        row: &Range<usize>,
        text: &str,
        style: PhraseStyle,
    ) -> Result<(Mask, Vec<Range<usize>>)> {
        let syllables = text.chars().filter(|ch| !ch.is_whitespace()).count();
        let SyllableOwnership {
            columns,
            labels,
            allowed,
        } = self.syllable_ownership(row, syllables)?;
        let mut gaps = Vec::with_capacity(syllables.saturating_sub(1));
        let mut space_pending = false;
        let mut seen = 0;
        for ch in text.chars() {
            if ch.is_whitespace() {
                space_pending = true;
                continue;
            }
            if seen > 0 {
                gaps.push(if space_pending {
                    style.word_space
                } else {
                    style.syllable_gap
                });
            }
            space_pending = false;
            seen += 1;
        }
        let scale_y = style.text_height as f32 / row.len() as f32;
        let natural: usize = columns
            .iter()
            .map(|column| (column.len() as f32 * scale_y).round() as usize)
            .sum();
        let gap_total: usize = gaps.iter().sum();
        let available = style
            .max_width
            .checked_sub(gap_total)
            .context("phrase gaps exceed the target width")?;
        // Per-syllable rounding can overshoot the shared factor by a pixel or
        // two; shrink the factor until the rounded widths fit.
        let mut condense = (available as f32 / natural as f32).min(1.0);
        let widths = loop {
            let scale_x = scale_y * condense;
            let widths = columns
                .iter()
                .map(|column| ((column.len() as f32 * scale_x).round() as usize).max(1))
                .collect::<Vec<_>>();
            if widths.iter().sum::<usize>() <= available {
                break widths;
            }
            if condense < 0.5 {
                bail!(
                    "composed phrase {text:?} cannot fit {}px without condensing below half width",
                    style.max_width
                );
            }
            condense -= 0.005;
        };
        let parts = columns
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(syllable, (column, &width))| {
                self.sample_where(
                    column,
                    row,
                    width,
                    style.text_height,
                    style.coverage_threshold,
                    |x, y| {
                        let label = labels[(y - row.start) * self.width + x];
                        label != usize::MAX && allowed[syllable][label]
                    },
                )
            })
            .collect::<Vec<_>>();
        let width = widths.iter().sum::<usize>() + gap_total;
        let mut phrase = Mask::new(width, style.text_height);
        let mut spans = Vec::with_capacity(parts.len());
        let mut x = 0;
        for (index, part) in parts.iter().enumerate() {
            phrase.paste(part, x, 0);
            spans.push(x..x + part.width);
            x += part.width + gaps.get(index).copied().unwrap_or(0);
        }
        Ok((phrase, spans))
    }

    /// Sample every syllable of `text` in `row` into a square `size` cell,
    /// keeping its aspect ratio and centering it. Repeated syllables reuse the
    /// first occurrence so identical letters produce identical tiles.
    pub fn compose_cells(
        &self,
        row: &Range<usize>,
        text: &str,
        cell: usize,
        glyph: usize,
        threshold: f32,
    ) -> Result<Vec<(char, Mask)>> {
        let chars = text
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<Vec<_>>();
        let columns = self.syllable_columns(row, chars.len())?;
        let mut cells = Vec::with_capacity(chars.len());
        for (ch, columns) in chars.into_iter().zip(columns) {
            let rows = self.ink_rows(&columns, row)?;
            let longest = columns.len().max(rows.len()) as f32;
            let width = ((columns.len() as f32 * glyph as f32 / longest).round() as usize).max(1);
            let height = ((rows.len() as f32 * glyph as f32 / longest).round() as usize).max(1);
            let sampled = self.sample(&columns, &rows, width, height, threshold);
            let mut square = Mask::new(cell, cell);
            square.paste(&sampled, (cell - width) / 2, (cell - height) / 2);
            cells.push((ch, square));
        }
        Ok(cells)
    }
}

impl Mask {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            bits: vec![false; width * height],
        }
    }

    pub fn get(&self, x: isize, y: isize) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.width
            && (y as usize) < self.height
            && self.bits[y as usize * self.width + x as usize]
    }

    pub fn paste(&mut self, source: &Mask, x: usize, y: usize) {
        for source_y in 0..source.height {
            for source_x in 0..source.width {
                if source.bits[source_y * source.width + source_x] {
                    self.bits[(y + source_y) * self.width + x + source_x] = true;
                }
            }
        }
    }

    fn touches(&self, x: usize, y: usize) -> bool {
        (-1isize..=1).any(|dy| (-1isize..=1).any(|dx| self.get(x as isize + dx, y as isize + dy)))
    }
}

/// Multi-player menu brush lettering, palette-1/palette-6 index roles of the
/// original `3人でぷよぷよ` labels: 1 outline, 8 corner shade, 9-13 a red to
/// yellow vertical ramp, 15 the top-left light. `band` is the vertical extent
/// of the text line so every label shades on the same ramp.
pub fn shade_brush(mask: &Mask, band: Range<usize>) -> Vec<u8> {
    let span = band.len().saturating_sub(1).max(1) as f32;
    let mut indices = vec![0u8; mask.width * mask.height];
    for y in 0..mask.height {
        for x in 0..mask.width {
            let (xi, yi) = (x as isize, y as isize);
            let value = if mask.get(xi, yi) {
                let t = (y as f32 - band.start as f32) / span;
                let base = if t < 0.55 {
                    13
                } else if t < 0.68 {
                    12
                } else if t < 0.8 {
                    11
                } else if t < 0.9 {
                    10
                } else {
                    9
                };
                let up = mask.get(xi, yi - 1);
                let down = mask.get(xi, yi + 1);
                let left = mask.get(xi - 1, yi);
                let right = mask.get(xi + 1, yi);
                let lit = !up || !left || !mask.get(xi, yi - 2) || !mask.get(xi - 2, yi);
                let mut value = if lit && t < 0.8 { 15 } else { base };
                if value != 15 && (!down || !right) {
                    value = (base - 1).max(9);
                }
                if (!up || !down) && (!left || !right) {
                    value = 8;
                }
                value
            } else if mask.touches(x, y) {
                1
            } else {
                0
            };
            indices[y * mask.width + x] = value;
        }
    }
    indices
}

/// Width and height of the 27x3-tile prompt canvases shared by the mode-select
/// and two-player rules prompts.
pub const PROMPT_CANVAS_WIDTH: usize = 27 * 8;
pub const PROMPT_CANVAS_HEIGHT: usize = 3 * 8;
const PROMPT_FACE_HEIGHT: usize = 20;
const PROMPT_COVERAGE: f32 = 0.8;

/// Compose and shade one prompt row of a generated prompt sheet into a
/// `PROMPT_CANVAS_WIDTH` x `PROMPT_CANVAS_HEIGHT` palette-index canvas. The
/// face sits one pixel below the top so the outline, the two-pixel extrusion
/// and the bottom outline fill the 24-pixel height like the original.
pub fn prompt_canvas(sheet_png: &[u8], row_index: usize, text: &str) -> Result<Vec<u8>> {
    let sheet = decode_ink_sheet(sheet_png)?;
    let rows = sheet.rows();
    let row = rows
        .get(row_index)
        .with_context(|| format!("prompt sheet has no row {row_index}"))?;
    let max_width = PROMPT_CANVAS_WIDTH - 5;
    let (face, _) = sheet.compose_phrase(
        row,
        text,
        PhraseStyle {
            text_height: PROMPT_FACE_HEIGHT,
            max_width,
            syllable_gap: 3,
            word_space: 7,
            coverage_threshold: PROMPT_COVERAGE,
        },
    )?;
    let mut canvas = Mask::new(PROMPT_CANVAS_WIDTH, PROMPT_CANVAS_HEIGHT);
    canvas.paste(&face, 1 + (max_width + 1 - face.width) / 2, 1);
    Ok(shade_pop_prompt(&canvas))
}

/// Pink prompt lettering with the original `モードを選んでください` roles:
/// 10 face, 8/9 top-left light, 11 a two-pixel extrusion toward the lower
/// right, 14 outline around face and extrusion. The original also scatters 15
/// glints and a second extrusion shade 12; Hangul strokes carry more edges, and
/// with those two roles the rules-prompt stream no longer fits its slot.
pub fn shade_pop_prompt(mask: &Mask) -> Vec<u8> {
    let mut indices = vec![0u8; mask.width * mask.height];
    for y in 0..mask.height {
        for x in 0..mask.width {
            let (xi, yi) = (x as isize, y as isize);
            indices[y * mask.width + x] = if mask.get(xi, yi) {
                let up = mask.get(xi, yi - 1);
                let left = mask.get(xi - 1, yi);
                match (up, left) {
                    (false, _) => 8,
                    (true, false) => 9,
                    (true, true) => 10,
                }
            } else if mask.get(xi - 1, yi - 1)
                || (mask.get(xi, yi - 1) && mask.get(xi - 1, yi))
                || mask.get(xi - 2, yi - 2)
                || mask.get(xi - 1, yi - 2)
                || mask.get(xi - 2, yi - 1)
            {
                11
            } else {
                0
            };
        }
    }
    let solid = Mask {
        width: mask.width,
        height: mask.height,
        bits: indices.iter().map(|&index| index != 0).collect(),
    };
    for y in 0..mask.height {
        for x in 0..mask.width {
            if indices[y * mask.width + x] == 0 && solid.touches(x, y) {
                indices[y * mask.width + x] = 14;
            }
        }
    }
    indices
}

/// Embossed ranking-title lettering with the original yellow roles:
/// 8 dark rim, 14/15 top-left bevel, 9/10 bottom-right bevel, 11-13 body.
pub fn shade_embossed(mask: &Mask) -> Vec<u8> {
    let mut indices = vec![0u8; mask.width * mask.height];
    let middle = mask.height / 2;
    for y in 0..mask.height {
        for x in 0..mask.width {
            let (xi, yi) = (x as isize, y as isize);
            indices[y * mask.width + x] = if mask.get(xi, yi) {
                let up = mask.get(xi, yi - 1);
                let left = mask.get(xi - 1, yi);
                let lit_edge = !up || !left;
                let shade_edge = !mask.get(xi, yi + 1) || !mask.get(xi + 1, yi);
                let lit_inner =
                    !mask.get(xi, yi - 2) || !mask.get(xi - 2, yi) || !mask.get(xi - 1, yi - 1);
                let shade_inner =
                    !mask.get(xi, yi + 2) || !mask.get(xi + 2, yi) || !mask.get(xi + 1, yi + 1);
                if lit_edge && !shade_edge {
                    if !up && !left { 15 } else { 14 }
                } else if shade_edge && !lit_edge {
                    9
                } else if lit_edge || (lit_inner && !shade_inner) {
                    13
                } else if shade_inner {
                    10
                } else if y < middle {
                    12
                } else {
                    11
                }
            } else if mask.touches(x, y) {
                8
            } else {
                0
            };
        }
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet_from_rows(rows: &[&str]) -> InkSheet {
        let width = rows[0].len();
        InkSheet {
            width,
            height: rows.len(),
            ink: rows
                .iter()
                .flat_map(|row| row.bytes().map(|byte| byte == b'#'))
                .collect(),
        }
    }

    #[test]
    fn syllable_columns_merge_closest_runs_to_the_requested_count() {
        let row = "##..##.........###..........###";
        let sheet = sheet_from_rows(&vec![row; 50]);
        let rows = sheet.rows();
        assert_eq!(rows, vec![0..50]);
        let columns = sheet.syllable_columns(&rows[0], 2).unwrap();
        assert_eq!(columns, vec![0..18, 28..31]);
    }

    #[test]
    fn shading_outlines_every_face_pixel() {
        let mut mask = Mask::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                mask.bits[y * 8 + x] = true;
            }
        }
        for indices in [shade_brush(&mask, 2..6), shade_embossed(&mask)] {
            for y in 0..8 {
                for x in 0..8 {
                    let face = mask.get(x as isize, y as isize);
                    assert_eq!(indices[y * 8 + x] != 0, face || mask.touches(x, y));
                }
            }
        }
        let prompt = shade_pop_prompt(&mask);
        assert_eq!(prompt[2 * 8 + 2], 8);
        assert_eq!(prompt[3 * 8 + 2], 9);
        assert_eq!(prompt[3 * 8 + 3], 10);
        assert_eq!(prompt[6 * 8 + 6], 11);
        assert_eq!(prompt[7 * 8 + 7], 11);
    }
}
