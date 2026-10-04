//! Separates the baked Japanese lettering inside a mouth-shaped sign from the
//! sign itself.
//!
//! The Remix menu signs draw their lettering inside a red mouth whose frame
//! (outline, lips and the two white fangs) touches the canvas edge, while the
//! lettering floats on the red fill. The lettering also carries a one to three
//! pixel glow in the other fill shade. A palette-index mask cannot tell a fang
//! from a letter because both use the same white, so the classification is
//! topological: the frame is flooded from the edge of the analyzed area through
//! non-fill pixels, and every non-fill pixel it does not reach is lettering.
//!
//! Two rules keep the flood out of the lettering. It moves only between
//! 4-neighbours, because lettering may touch the outline at a corner. It enters
//! a lettering-colored pixel only from the outline color or from another
//! lettering-colored frame pixel: a fang hangs directly from the outline, but a
//! letter reaches the outline at most through its own dark drop shadow. A
//! lettering-colored patch stays a fang only when it is fang-sized, hangs from
//! the upper lip and starts in the upper half; a stroke leaning on the side
//! outline is lettering.
//!
//! Fill pixels near the lettering form the glow ring. Once the lettering is
//! gone the whole mouth interior is repainted as the original two shades: the
//! upper shade above a band of the lower shade along the mouth bottom, whose
//! thickness the clean columns show. The menu dims unselected signs with
//! color-math subtraction, which turns any leftover mix of the two shades into
//! visible scraps.

use anyhow::{Context, Result, bail};

/// Role-less value for pixels drawn with another palette.
const OTHER_PALETTE: u8 = 0xFF;

/// Fill pixels farther than this from the lettering vote on the row background.
const VOTING_RADIUS: usize = 2;

/// Most frequent value; ties go to the value seen first (the nearest).
fn majority(values: &[u8]) -> Option<u8> {
    let mut best: Option<(usize, u8)> = None;
    for &value in values {
        let count = values.iter().filter(|&&other| other == value).count();
        if best.is_none_or(|(best_count, _)| count > best_count) {
            best = Some((count, value));
        }
    }
    best.map(|(_, value)| value)
}

/// Colors of the generated Korean sign lettering after palette normalization.
const LETTER_WHITE: [u8; 3] = [0xE7, 0xE7, 0xE7];
const LETTER_PINK: [u8; 3] = [0xE7, 0x63, 0x84];
const LETTER_LIGHT_PINK: [u8; 3] = [0xE7, 0xA5, 0xC6];
const LETTER_OUTLINE: [u8; 3] = [0x63, 0x00, 0x00];
/// Opaque islands this small are generation specks, not strokes.
const MAX_SPECK_PIXELS: usize = 2;

/// Pixels changed by [`tidy_generated_lettering`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TidyCounts {
    pub light_pink_to_white: usize,
    pub light_pink_to_edge: usize,
    pub stray_outline: usize,
    pub specks: usize,
}

/// Removes anti-aliasing leftovers from a generated RGBA sign label without
/// changing its strokes.
///
/// The labels draw white strokes with a pink lower edge and a maroon outline
/// one or two pixels thick. At the small label size a one-pixel transparent
/// hole or pink dot inside the white is a letter counter or the gap between
/// two strokes (the two halves of `ㅃ`), so neither is touched. Image
/// generation also left light-pink blend pixels in the white, maroon pixels
/// detached from the outline and tiny islands. The menu darkens unselected
/// signs by color-math subtraction, where those leftovers read as specks.
///
/// A light-pink pixel standing on the outline or the canvas is the lower
/// stroke edge and becomes the edge pink. One beside a pink pixel belongs to a
/// pink edge run, such as the edge that separates two stacked strokes, and
/// stays. Any other light-pink pixel is a blend inside a stroke and becomes
/// white. An outline pixel stays when one of its four neighbours is a stroke
/// pixel, or is such an outline pixel (an outline corner or the second
/// outline row); any other outline pixel, including one that meets a stroke
/// only at a corner, becomes transparent. Finally any opaque island of
/// at most two pixels becomes transparent.
pub fn tidy_generated_lettering(pixels: &mut [u8], width: usize, height: usize) -> TidyCounts {
    let rgb = |pixels: &[u8], x: usize, y: usize| {
        let offset = (y * width + x) * 4;
        (pixels[offset + 3] == 0xFF)
            .then(|| [pixels[offset], pixels[offset + 1], pixels[offset + 2]])
    };
    let set = |pixels: &mut [u8], x: usize, y: usize, color: Option<[u8; 3]>| {
        let offset = (y * width + x) * 4;
        match color {
            Some(color) => {
                pixels[offset..offset + 3].copy_from_slice(&color);
                pixels[offset + 3] = 0xFF;
            }
            None => pixels[offset..offset + 4].fill(0),
        }
    };
    let is_stroke = |color: Option<[u8; 3]>| {
        matches!(color, Some(LETTER_WHITE | LETTER_PINK | LETTER_LIGHT_PINK))
    };
    let neighbours = |x: usize, y: usize, diagonal: bool| {
        let mut out = Vec::with_capacity(8);
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                if (dx, dy) != (0, 0)
                    && (diagonal || dx == 0 || dy == 0)
                    && nx >= 0
                    && ny >= 0
                    && (nx as usize) < width
                    && (ny as usize) < height
                {
                    out.push((nx as usize, ny as usize));
                }
            }
        }
        out
    };
    let mut counts = TidyCounts::default();

    let source = pixels.to_vec();
    for y in 0..height {
        for x in 0..width {
            if rgb(&source, x, y) != Some(LETTER_LIGHT_PINK) {
                continue;
            }
            let below = (y + 1 < height).then(|| rgb(&source, x, y + 1)).flatten();
            let beside_pink = [x.checked_sub(1), Some(x + 1)]
                .into_iter()
                .flatten()
                .any(|nx| nx < width && rgb(&source, nx, y) == Some(LETTER_PINK));
            if matches!(below, None | Some(LETTER_OUTLINE)) {
                set(pixels, x, y, Some(LETTER_PINK));
                counts.light_pink_to_edge += 1;
            } else if !beside_pink {
                set(pixels, x, y, Some(LETTER_WHITE));
                counts.light_pink_to_white += 1;
            }
        }
    }

    let source = pixels.to_vec();
    let is_outline = |x: usize, y: usize| rgb(&source, x, y) == Some(LETTER_OUTLINE);
    let anchored = |x: usize, y: usize| {
        is_outline(x, y)
            && neighbours(x, y, false)
                .into_iter()
                .any(|(nx, ny)| is_stroke(rgb(&source, nx, ny)))
    };
    for y in 0..height {
        for x in 0..width {
            if is_outline(x, y)
                && !anchored(x, y)
                && !neighbours(x, y, false)
                    .into_iter()
                    .any(|(nx, ny)| anchored(nx, ny))
            {
                set(pixels, x, y, None);
                counts.stray_outline += 1;
            }
        }
    }

    let mut seen = vec![false; width * height];
    for start in 0..width * height {
        if seen[start] || rgb(pixels, start % width, start / width).is_none() {
            continue;
        }
        let mut island = vec![start];
        seen[start] = true;
        let mut cursor = 0;
        while cursor < island.len() {
            let (x, y) = (island[cursor] % width, island[cursor] / width);
            cursor += 1;
            for (nx, ny) in neighbours(x, y, true) {
                let offset = ny * width + nx;
                if !seen[offset] && rgb(pixels, nx, ny).is_some() {
                    seen[offset] = true;
                    island.push(offset);
                }
            }
        }
        if island.len() <= MAX_SPECK_PIXELS {
            for offset in island {
                set(pixels, offset % width, offset / width, None);
                counts.specks += 1;
            }
        }
    }
    counts
}

/// Palette roles (low nibble) of one sign.
#[derive(Debug, Clone, Copy)]
pub struct SignColors<'a> {
    /// Mouth fill shades: the upper shade first, then the lower shade that
    /// forms the band along the mouth bottom.
    pub fill: &'a [u8],
    /// Colors of the Japanese letter bodies. Fangs share some of them.
    pub lettering: &'a [u8],
    /// Mouth outline color from which the fangs hang.
    pub outline: u8,
    /// CGRAM palette of the sign. Pixels of any other palette (the scenery
    /// around the mouth) never take a sign role.
    pub palette: u8,
    /// Largest lettering-colored patch that still counts as a fang. A letter
    /// stroke can touch the outline too, but it is far larger than a fang.
    pub max_fang_pixels: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelClass {
    /// Outline, lips and fangs: never repainted.
    Frame,
    /// Red fill away from the lettering: kept as is.
    Fill,
    /// Japanese lettering: erased to restored fill.
    Text,
    /// Glow ring around the lettering: erased to restored fill.
    Halo,
}

#[derive(Debug, Clone)]
pub struct SignMask {
    pub region: Region,
    classes: Vec<PixelClass>,
    restored: Vec<u8>,
}

impl SignMask {
    pub fn class(&self, local_x: usize, local_y: usize) -> PixelClass {
        self.classes[local_y * self.region.width + local_x]
    }

    /// Palette index for the pixel once the Japanese lettering is removed.
    pub fn restored(&self, local_x: usize, local_y: usize) -> u8 {
        self.restored[local_y * self.region.width + local_x]
    }

    /// Class of a screen pixel inside the analyzed region.
    pub fn class_at(&self, screen_x: usize, screen_y: usize) -> PixelClass {
        self.class(screen_x - self.region.x, screen_y - self.region.y)
    }

    /// Restored index of a screen pixel inside the analyzed region.
    pub fn restored_at(&self, screen_x: usize, screen_y: usize) -> u8 {
        self.restored(screen_x - self.region.x, screen_y - self.region.y)
    }

    /// Pixel counts of each class inside `inner`, a part of the analyzed region.
    pub fn counts_within(&self, inner: Region) -> (usize, usize, usize) {
        let mut text = 0;
        let mut halo = 0;
        let mut frame = 0;
        for y in inner.y..inner.y + inner.height {
            for x in inner.x..inner.x + inner.width {
                match self.class_at(x, y) {
                    PixelClass::Text => text += 1,
                    PixelClass::Halo => halo += 1,
                    PixelClass::Frame => frame += 1,
                    PixelClass::Fill => {}
                }
            }
        }
        (text, halo, frame)
    }
}

/// Classifies the mouth around `canvas`, which may crop the lettering or the
/// mouth outline. The analysis grows the canvas by `margin` pixels on each side
/// (left/right, top, bottom), clamped to the layer, so that the frame is found
/// from the whole mouth rather than from the canvas edge.
pub fn analyze_around(
    indices: &[u8],
    stride: usize,
    canvas: Region,
    margin: (usize, usize, usize),
    colors: SignColors,
    halo_radius: usize,
) -> Result<SignMask> {
    let rows = indices.len() / stride;
    let (side, top, bottom) = margin;
    let x = canvas.x.saturating_sub(side);
    let y = canvas.y.saturating_sub(top);
    let right = (canvas.x + canvas.width + side).min(stride);
    let lower = (canvas.y + canvas.height + bottom).min(rows);
    analyze(
        indices,
        stride,
        Region {
            x,
            y,
            width: right - x,
            height: lower - y,
        },
        colors,
        halo_radius,
    )
}

/// Classifies `region` of a rendered palette-index layer.
///
/// `indices` holds one byte per screen pixel with `stride` pixels per row; only
/// the low nibble is used. `halo_radius` is the Chebyshev radius of the glow
/// ring around the lettering.
pub fn analyze(
    indices: &[u8],
    stride: usize,
    region: Region,
    colors: SignColors,
    halo_radius: usize,
) -> Result<SignMask> {
    let SignColors {
        fill,
        lettering,
        outline,
        palette,
        max_fang_pixels,
    } = colors;
    let Region {
        x: x0,
        y: y0,
        width,
        height,
    } = region;
    if width == 0 || height == 0 || x0 + width > stride || (y0 + height) * stride > indices.len() {
        bail!("sign region {x0},{y0} {width}x{height} is outside the rendered layer");
    }
    // `indices` stores palette * 16 + color. A pixel of another palette reads
    // as OTHER_PALETTE, which matches no sign role.
    let local = |x: usize, y: usize| {
        let value = indices[(y0 + y) * stride + x0 + x];
        if value >> 4 == palette {
            value & 0x0F
        } else {
            OTHER_PALETTE
        }
    };
    let is_fill = |x: usize, y: usize| fill.contains(&local(x, y));

    let neighbours = |x: usize, y: usize| {
        let mut out = Vec::with_capacity(8);
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height {
                    out.push((nx as usize, ny as usize));
                }
            }
        }
        out
    };

    // Floods the frame from the region edge through non-fill pixels, never
    // entering a `blocked` pixel.
    let flood = |blocked: &[bool]| {
        let mut frame = vec![false; width * height];
        let mut queue = std::collections::VecDeque::new();
        for y in 0..height {
            for x in 0..width {
                let edge = x == 0 || y == 0 || x + 1 == width || y + 1 == height;
                if edge && !is_fill(x, y) && !blocked[y * width + x] {
                    frame[y * width + x] = true;
                    queue.push_back((x, y));
                }
            }
        }
        while let Some((x, y)) = queue.pop_front() {
            let sides = [
                x.checked_sub(1).map(|nx| (nx, y)),
                (x + 1 < width).then_some((x + 1, y)),
                y.checked_sub(1).map(|ny| (x, ny)),
                (y + 1 < height).then_some((x, y + 1)),
            ];
            let here = local(x, y);
            let may_enter_lettering = here == outline || lettering.contains(&here);
            for (nx, ny) in sides.into_iter().flatten() {
                let offset = ny * width + nx;
                let next = local(nx, ny);
                if frame[offset] || blocked[offset] || is_fill(nx, ny) {
                    continue;
                }
                if lettering.contains(&next) && !may_enter_lettering {
                    continue;
                }
                frame[offset] = true;
                queue.push_back((nx, ny));
            }
        }
        frame
    };
    let mut frame = flood(&vec![false; width * height]);

    // A lettering-colored frame patch larger than a fang is a letter stroke
    // that happens to touch the outline; return it to the lettering.
    let mut visited = vec![false; width * height];
    let mut strokes = vec![false; width * height];
    for start in 0..width * height {
        let (sx, sy) = (start % width, start / width);
        if visited[start] || !frame[start] || !lettering.contains(&local(sx, sy)) {
            continue;
        }
        let mut patch = vec![start];
        visited[start] = true;
        let mut cursor = 0;
        while cursor < patch.len() {
            let (x, y) = (patch[cursor] % width, patch[cursor] / width);
            cursor += 1;
            let sides = [
                x.checked_sub(1).map(|nx| (nx, y)),
                (x + 1 < width).then_some((x + 1, y)),
                y.checked_sub(1).map(|ny| (x, ny)),
                (y + 1 < height).then_some((x, y + 1)),
            ];
            for (nx, ny) in sides.into_iter().flatten() {
                let offset = ny * width + nx;
                if !visited[offset] && frame[offset] && lettering.contains(&local(nx, ny)) {
                    visited[offset] = true;
                    patch.push(offset);
                }
            }
        }
        // A fang hangs from the upper lip: some pixel sits right under the
        // outline, and the patch starts in the upper half of the mouth. A
        // letter stroke that only leans on the side outline is lettering.
        let hangs_from_lip = patch.iter().any(|&offset| {
            let (x, y) = (offset % width, offset / width);
            y > 0 && local(x, y - 1) == outline
        });
        let top = patch
            .iter()
            .map(|&offset| offset / width)
            .min()
            .unwrap_or(0);
        if patch.len() > max_fang_pixels || !hangs_from_lip || top >= height / 2 {
            for offset in patch {
                strokes[offset] = true;
            }
        }
    }
    // Japanese shadow pixels hang off such a stroke and were reached through
    // it. Flooding again without entering the strokes leaves them with the
    // lettering, so none survives the erase as a stray dot in the mouth.
    if strokes.contains(&true) {
        frame = flood(&strokes);
    }
    // The drop shadow hugging such a stroke was reached through the outline
    // too; the pixels touching the letter body belong to the letter.
    let body = |x: usize, y: usize| !frame[y * width + x] && lettering.contains(&local(x, y));
    let shadow = (0..width * height)
        .filter(|&offset| {
            let (x, y) = (offset % width, offset / width);
            let sides = [
                x.checked_sub(1).map(|nx| (nx, y)),
                (x + 1 < width).then_some((x + 1, y)),
                y.checked_sub(1).map(|ny| (x, ny)),
                (y + 1 < height).then_some((x, y + 1)),
            ];
            frame[offset]
                && local(x, y) != outline
                && !lettering.contains(&local(x, y))
                && sides.into_iter().flatten().any(|(nx, ny)| body(nx, ny))
        })
        .collect::<Vec<_>>();
    for offset in shadow {
        frame[offset] = false;
    }

    let unreached = usize::MAX;
    let mut distance = vec![unreached; width * height];
    let mut queue = std::collections::VecDeque::new();
    for y in 0..height {
        for x in 0..width {
            if !is_fill(x, y) && !frame[y * width + x] {
                distance[y * width + x] = 0;
                queue.push_back((x, y));
            }
        }
    }
    if queue.is_empty() {
        bail!("sign region {x0},{y0} contains no lettering detached from its frame");
    }
    while let Some((x, y)) = queue.pop_front() {
        let next = distance[y * width + x] + 1;
        if next > halo_radius + 1 {
            continue;
        }
        for (nx, ny) in neighbours(x, y) {
            let offset = ny * width + nx;
            if distance[offset] == unreached && is_fill(nx, ny) {
                distance[offset] = next;
                queue.push_back((nx, ny));
            }
        }
    }

    let mut classes = vec![PixelClass::Fill; width * height];
    for offset in 0..width * height {
        classes[offset] = if frame[offset] {
            PixelClass::Frame
        } else if distance[offset] == 0 {
            PixelClass::Text
        } else if distance[offset] <= halo_radius {
            PixelClass::Halo
        } else {
            PixelClass::Fill
        };
    }
    if let Some(restored) = restore_two_zone_mouth(&classes, width, height, &local, fill) {
        return Ok(SignMask {
            region,
            classes,
            restored,
        });
    }

    // The row background is voted by fill pixels well clear of the lettering.
    // Glow pixels three or more pixels out still sit inside the erased ring,
    // so a narrower voting radius keeps enough voters in dense rows while the
    // whole-row vote outweighs the few glow pixels among them.
    let voting_radius = halo_radius.min(VOTING_RADIUS);
    let voters = |y: usize| {
        (0..width)
            .filter(|&x| {
                let offset = y * width + x;
                is_fill(x, y) && !frame[offset] && distance[offset] > voting_radius
            })
            .map(|x| local(x, y))
            .collect::<Vec<_>>()
    };
    let voted = (0..height)
        .map(|y| majority(&voters(y)))
        .collect::<Vec<_>>();
    let mut background = (0..height)
        .map(|y| {
            (0..height)
                .filter_map(|other| voted[other].map(|value| (other.abs_diff(y), value)))
                .min_by_key(|(gap, _)| *gap)
                .map(|(_, value)| value)
        })
        .collect::<Option<Vec<_>>>()
        .with_context(|| {
            format!("sign region {x0},{y0} has no clean fill to restore its lettering")
        })?;
    // The shade boundary is a straight band; a lone row voted the other way
    // is a mixed boundary row and would draw a one-pixel line.
    for y in 1..height.saturating_sub(1) {
        if background[y - 1] == background[y + 1] && background[y] != background[y - 1] {
            background[y] = background[y - 1];
        }
    }

    // The mouth fill is an upper and a lower shade whose boundary the Japanese
    // lettering used to hide: in the original the lower shade shows only below
    // the letters, as a thin band at the mouth bottom. Glow rows vote for the
    // lower shade too early, so a row vote alone exposes a wide bright band
    // under the shorter Korean labels. Vertical runs of erased pixels whose
    // clean ends differ in shade show which shades meet there.
    let erased = |offset: usize| matches!(classes[offset], PixelClass::Text | PixelClass::Halo);
    let clean = |x: usize, y: usize| classes[y * width + x] == PixelClass::Fill && is_fill(x, y);
    let mut runs = Vec::new();
    for x in 0..width {
        let mut y = 0;
        while y < height {
            if !erased(y * width + x) {
                y += 1;
                continue;
            }
            let top = y;
            while y < height && erased(y * width + x) {
                y += 1;
            }
            let above = (0..top)
                .rev()
                .find(|&ay| clean(x, ay))
                .map(|ay| local(x, ay));
            let below = (y..height)
                .find(|&by| clean(x, by))
                .map(|by| (by, local(x, by)));
            runs.push((above, below));
        }
    }
    let crossings = runs
        .iter()
        .filter_map(|&(above, below)| match (above, below) {
            (Some(upper), Some((row, lower))) if upper != lower => Some((upper, lower, row)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut boundary = None;
    if let (Some(upper), Some(lower)) = (
        majority(&crossings.iter().map(|c| c.0).collect::<Vec<_>>()),
        majority(&crossings.iter().map(|c| c.1).collect::<Vec<_>>()),
    ) {
        // The mouth interior is what the erased lettering reaches without
        // crossing the frame; fill-colored scenery outside the mouth must not
        // vote on the band.
        let mut interior = vec![false; width * height];
        let mut queue = (0..width * height)
            .filter(|&offset| erased(offset))
            .collect::<std::collections::VecDeque<_>>();
        for &offset in &queue {
            interior[offset] = true;
        }
        while let Some(offset) = queue.pop_front() {
            let (x, y) = (offset % width, offset / width);
            let sides = [
                x.checked_sub(1).map(|nx| (nx, y)),
                (x + 1 < width).then_some((x + 1, y)),
                y.checked_sub(1).map(|ny| (x, ny)),
                (y + 1 < height).then_some((x, y + 1)),
            ];
            for (nx, ny) in sides.into_iter().flatten() {
                let next = ny * width + nx;
                if !interior[next] && classes[next] != PixelClass::Frame {
                    interior[next] = true;
                    queue.push_back(next);
                }
            }
        }
        let inside = |x: usize, y: usize| interior[y * width + x] && clean(x, y);
        // In the original the lower shade shows as a thin band along the mouth
        // bottom. Each interior column whose bottom pixel is clean reports
        // where its lower-shade run from the bottom starts; the median column
        // sets the band edge.
        let mut tops = (0..width)
            .filter_map(|x| {
                let bottom = (0..height).rev().find(|&y| interior[y * width + x])?;
                if !inside(x, bottom) || local(x, bottom) != lower {
                    return None;
                }
                let mut top = bottom;
                while top > 0 && inside(x, top - 1) && local(x, top - 1) == lower {
                    top -= 1;
                }
                Some(top)
            })
            .collect::<Vec<_>>();
        if tops.is_empty() {
            tops = crossings.iter().map(|c| c.2).collect();
        }
        tops.sort_unstable();
        let row = tops[tops.len() / 2];
        for (y, shade) in background.iter_mut().enumerate() {
            if y >= row && *shade == upper {
                *shade = lower;
            } else if y < row && *shade == lower {
                *shade = upper;
            }
        }
        // Lower-shade fill above the band that the lettering left visible and
        // that hangs from the band would stand out as a block once the letters
        // are gone; flood it from the band so it joins the upper shade.
        let mut lifted = vec![false; width * height];
        let mut queue = (0..width)
            .filter(|&x| row < height && inside(x, row) && local(x, row) == lower)
            .map(|x| (x, row))
            .collect::<std::collections::VecDeque<_>>();
        while let Some((x, y)) = queue.pop_front() {
            let sides = [
                x.checked_sub(1).map(|nx| (nx, y)),
                (x + 1 < width).then_some((x + 1, y)),
                y.checked_sub(1).map(|ny| (x, ny)),
            ];
            for (nx, ny) in sides.into_iter().flatten() {
                let offset = ny * width + nx;
                if ny < row && !lifted[offset] && inside(nx, ny) && local(nx, ny) == lower {
                    lifted[offset] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
        boundary = Some((upper, lifted));
    }

    let mut restored = (0..width * height)
        .map(|offset| {
            let (x, y) = (offset % width, offset / width);
            match classes[offset] {
                PixelClass::Frame | PixelClass::Fill => local(x, y),
                PixelClass::Text | PixelClass::Halo => background[y],
            }
        })
        .collect::<Vec<_>>();
    // Lower-shade fill that the lettering left visible above the boundary
    // would stand out as a block once the letters are gone; it joins the
    // upper shade so the band edge runs straight under the new label.
    if let Some((upper, lifted)) = boundary {
        for (offset, lift) in lifted.into_iter().enumerate() {
            if lift {
                restored[offset] = upper;
            }
        }
    }

    Ok(SignMask {
        region,
        classes,
        restored,
    })
}

/// Columns on each side whose boundaries smooth one column's boundary.
const BOUNDARY_SMOOTHING: usize = 2;
/// Rows a measured column boundary may stray from the mouth-wide level.
const BOUNDARY_DEVIATION: usize = 3;

/// Repaints the whole mouth interior as the original two shades: the darker
/// shade above, the lighter shade below a boundary that each column reports
/// where the lettering does not hide it.
///
/// The Japanese lettering hides the boundary in many columns, and its glow ring
/// mixes both shades around the letters. Restoring pixel by pixel keeps those
/// scraps, which stand out once the screen darkens the unselected signs. The
/// boundary is measured in the columns where a clean upper-shade pixel sits on
/// a clean lower-shade run, interpolated across the hidden columns, and smoothed;
/// every interior pixel then takes the shade of its side.
fn restore_two_zone_mouth(
    classes: &[PixelClass],
    width: usize,
    height: usize,
    local: &impl Fn(usize, usize) -> u8,
    fill: &[u8],
) -> Option<Vec<u8>> {
    let erased = |offset: usize| matches!(classes[offset], PixelClass::Text | PixelClass::Halo);
    let mut interior = vec![false; width * height];
    let mut queue = (0..width * height)
        .filter(|&offset| erased(offset))
        .collect::<std::collections::VecDeque<_>>();
    for &offset in &queue {
        interior[offset] = true;
    }
    while let Some(offset) = queue.pop_front() {
        let (x, y) = (offset % width, offset / width);
        let sides = [
            x.checked_sub(1).map(|nx| (nx, y)),
            (x + 1 < width).then_some((x + 1, y)),
            y.checked_sub(1).map(|ny| (x, ny)),
            (y + 1 < height).then_some((x, y + 1)),
        ];
        for (nx, ny) in sides.into_iter().flatten() {
            let next = ny * width + nx;
            if !interior[next] && classes[next] != PixelClass::Frame {
                interior[next] = true;
                queue.push_back(next);
            }
        }
    }
    // A clean pixel shows the shade the mouth had there before the lettering.
    let known = |x: usize, y: usize| {
        let offset = y * width + x;
        (interior[offset] && classes[offset] == PixelClass::Fill && fill.contains(&local(x, y)))
            .then(|| local(x, y))
    };
    let bottoms = (0..width)
        .map(|x| (0..height).rev().find(|&y| interior[y * width + x]))
        .collect::<Vec<_>>();
    // `fill` lists the upper shade first and the lower shade second. Voting
    // on the bottom row instead would flip them where the curved bottom
    // corners show the upper shade.
    let (&upper, &lower) = (fill.first()?, fill.get(1)?);

    // The lower shade is a band along the mouth bottom. A column whose band
    // ends on a clean upper pixel shows its thickness; columns where the
    // lettering covered the band take the mouth's typical thickness.
    let mut measured: Vec<Option<usize>> = vec![None; width];
    let mut partial: Vec<usize> = Vec::new();
    for x in 0..width {
        let Some(bottom) = bottoms[x] else { continue };
        let mut thickness = 0;
        let mut ended_on_upper = false;
        for y in (0..=bottom).rev() {
            if !interior[y * width + x] {
                break;
            }
            match known(x, y) {
                Some(shade) if shade == lower => thickness += 1,
                Some(_) => {
                    ended_on_upper = true;
                    break;
                }
                None => break,
            }
        }
        if ended_on_upper {
            measured[x] = Some(thickness);
        } else {
            partial.push(thickness);
        }
    }
    let mut exact = measured.iter().flatten().copied().collect::<Vec<_>>();
    exact.sort_unstable();
    partial.sort_unstable();
    let typical = if let Some(&value) = exact.get(exact.len() / 2) {
        value
    } else {
        *partial.get(partial.len() / 2)?
    };
    let thickness = measured
        .iter()
        .map(|value| match value {
            Some(value) if value.abs_diff(typical) <= BOUNDARY_DEVIATION => *value,
            _ => typical,
        })
        .collect::<Vec<_>>();
    let smoothed = (0..width)
        .map(|x| {
            let Some(bottom) = bottoms[x] else {
                return height;
            };
            let mut window = (x.saturating_sub(BOUNDARY_SMOOTHING)
                ..=(x + BOUNDARY_SMOOTHING).min(width - 1))
                .filter(|&other| bottoms[other].is_some())
                .map(|other| thickness[other])
                .collect::<Vec<_>>();
            window.sort_unstable();
            (bottom + 1).saturating_sub(window[window.len() / 2])
        })
        .collect::<Vec<_>>();

    Some(
        (0..width * height)
            .map(|offset| {
                let (x, y) = (offset % width, offset / width);
                if !interior[offset] || classes[offset] == PixelClass::Frame {
                    local(x, y)
                } else if y >= smoothed[x] {
                    lower
                } else {
                    upper
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLORS: SignColors = SignColors {
        fill: &[3, 4],
        lettering: &[0x0A, 0x0F],
        outline: 1,
        palette: 0,
        max_fang_pixels: 3,
    };

    #[test]
    fn a_letter_stroke_touching_the_outline_is_larger_than_a_fang() {
        // A two-pixel fang hangs from the top; a five-pixel stroke touches the
        // right outline.
        let (indices, width) = grid(&[
            "11111111", "1f333331", "1f333331", "13fffff1", "13333331", "11111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 6,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.class(1, 1), PixelClass::Frame);
        assert_eq!(mask.class(6, 3), PixelClass::Text);
        assert_eq!(mask.class(2, 3), PixelClass::Text);
        assert_eq!(mask.restored(6, 3), 3);
    }

    fn grid(rows: &[&str]) -> (Vec<u8>, usize) {
        let width = rows[0].len();
        let bytes = rows
            .iter()
            .flat_map(|row| {
                row.chars()
                    .map(|c| c.to_digit(16).expect("hex digit") as u8)
                    .collect::<Vec<_>>()
            })
            .collect();
        (bytes, width)
    }

    #[test]
    fn fangs_touching_the_frame_survive_while_floating_letters_are_erased() {
        // Frame of 1 with a white fang (f) hanging from the top edge, a white
        // letter floating on fill 3 with a one-pixel glow of 4.
        let (indices, width) = grid(&[
            "1111111111",
            "1f33333331",
            "1f33333331",
            "1334443331",
            "1334f43331",
            "1334443331",
            "1333333331",
            "1111111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 8,
        };
        let mask = analyze(&indices, width, region, COLORS, 1).expect("analyze");
        assert_eq!(mask.class(1, 1), PixelClass::Frame);
        assert_eq!(mask.class(1, 2), PixelClass::Frame);
        assert_eq!(mask.class(4, 4), PixelClass::Text);
        assert_eq!(mask.class(3, 3), PixelClass::Halo);
        assert_eq!(mask.restored(1, 1), 0x0F, "fang keeps its white");
        assert_eq!(mask.restored(4, 4), 3, "letter becomes clean fill");
        assert_eq!(mask.restored(3, 4), 3, "glow becomes clean fill");
        assert_eq!(mask.counts_within(region), (1, 8, 34));
    }

    #[test]
    fn the_mouth_is_repainted_as_upper_shade_over_a_bottom_band() {
        // Clean columns show a two-row lower-shade band at the bottom. The
        // letter (f) hides the band in its own columns, and a stray glow pixel
        // (4) floats near the top. Every interior pixel takes the upper shade
        // except a two-row band along the bottom, so no glow scrap survives.
        let (indices, width) = grid(&[
            "111111111",
            "134333331",
            "133fff331",
            "133fff331",
            "133fff331",
            "144fff441",
            "144fff441",
            "111111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 8,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.restored(2, 1), 3, "stray glow joins the upper shade");
        assert_eq!(mask.restored(4, 3), 3, "erased pixel above the band");
        assert_eq!(mask.restored(4, 5), 4, "erased pixel inside the band");
        assert_eq!(mask.restored(4, 6), 4, "erased pixel inside the band");
        assert_eq!(mask.restored(7, 4), 3, "clean column above its band");
    }

    #[test]
    fn tidying_keeps_strokes_and_drops_generation_leftovers() {
        // W white stroke, P pink lower edge, L light pink, O maroon outline,
        // . transparent. Two strokes are stacked; the pink edge of the upper
        // one separates them. The upper stroke holds a counter hole and a
        // pink gap dot that are part of the letter, plus a light-pink blend.
        // The outline has a second row at the top, a maroon spur hangs
        // diagonally off the outline corner, a maroon dot floats away from
        // it, another meets a small stroke only at its corner, and a
        // two-pixel island sits apart.
        let rows = [
            ".O..........",
            "OOOOOOO.....",
            "OWWWWWO...WW",
            "OWLW.WO.O...",
            "OWWWPWO.....",
            "OWWWWWO....O",
            "OPLPPPO.WWW.",
            "OWWWWWO.....",
            "OPPLPPO.....",
            "OOOOOOO.....",
            ".......O....",
        ];
        let width = rows[0].len();
        let height = rows.len();
        let mut pixels = Vec::new();
        for row in rows {
            for c in row.chars() {
                let (color, alpha) = match c {
                    'W' => (LETTER_WHITE, 0xFF),
                    'P' => (LETTER_PINK, 0xFF),
                    'L' => (LETTER_LIGHT_PINK, 0xFF),
                    'O' => (LETTER_OUTLINE, 0xFF),
                    _ => ([0; 3], 0),
                };
                pixels.extend_from_slice(&color);
                pixels.push(alpha);
            }
        }
        let counts = tidy_generated_lettering(&mut pixels, width, height);
        let at = |x: usize, y: usize| {
            let offset = (y * width + x) * 4;
            (pixels[offset + 3] == 0xFF)
                .then(|| [pixels[offset], pixels[offset + 1], pixels[offset + 2]])
        };
        assert_eq!(at(4, 3), None, "counter hole stays");
        assert_eq!(at(4, 4), Some(LETTER_PINK), "gap dot stays");
        assert_eq!(at(2, 3), Some(LETTER_WHITE), "light-pink blend");
        assert_eq!(at(1, 6), Some(LETTER_PINK), "separating edge stays");
        assert_eq!(at(2, 6), Some(LETTER_LIGHT_PINK), "separating edge stays");
        assert_eq!(at(3, 8), Some(LETTER_PINK), "light pink on the outline");
        assert_eq!(at(0, 2), Some(LETTER_OUTLINE), "outline next to the stroke");
        assert_eq!(at(1, 0), Some(LETTER_OUTLINE), "second outline row");
        assert_eq!(at(6, 9), Some(LETTER_OUTLINE), "outline corner");
        assert_eq!(at(7, 10), None, "diagonal outline spur");
        assert_eq!(at(11, 5), None, "outline meeting a stroke at a corner");
        assert_eq!(at(9, 6), Some(LETTER_WHITE), "three-pixel stroke stays");
        assert_eq!(at(8, 3), None, "detached outline dot");
        assert_eq!(at(10, 2), None, "two-pixel island");
        assert_eq!(
            counts,
            TidyCounts {
                light_pink_to_white: 1,
                light_pink_to_edge: 1,
                stray_outline: 3,
                specks: 2,
            }
        );
    }

    #[test]
    fn a_stroke_leaning_on_the_side_outline_is_not_a_fang() {
        // A two-pixel fang hangs from the upper lip at (1,1)-(1,2). A small
        // letter stroke (f) in the lower half leans on the left outline; it is
        // as small as a fang but never hangs from the lip.
        let (indices, width) = grid(&[
            "111111111",
            "1f3333331",
            "1f3333331",
            "133333331",
            "133333331",
            "1ff333331",
            "1ff333331",
            "111111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 8,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.class(1, 1), PixelClass::Frame, "fang stays");
        assert_eq!(mask.class(1, 5), PixelClass::Text, "stroke is lettering");
        assert_eq!(mask.class(2, 6), PixelClass::Text, "stroke is lettering");
    }

    #[test]
    fn shadow_hanging_off_a_stroke_on_the_outline_is_lettering() {
        // A stroke (f) leans on the left outline. Its shadow 5 touches the
        // stroke, and the shadow 2 touches only that 5, never the stroke.
        let (indices, width) = grid(&[
            "111111111",
            "1ff333331",
            "1ff523331",
            "1ff333331",
            "133333331",
            "111111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 6,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.class(1, 2), PixelClass::Text, "stroke is lettering");
        assert_eq!(mask.class(3, 2), PixelClass::Text, "shadow is lettering");
        assert_eq!(
            mask.class(4, 2),
            PixelClass::Text,
            "outer shadow is lettering"
        );
        assert_eq!(mask.class(0, 2), PixelClass::Frame, "outline stays");
    }

    #[test]
    fn restoration_follows_the_row_shade_of_a_vertical_gradient() {
        let (indices, width) = grid(&[
            "11111111", "13333331", "133f3331", "14444441", "144f4441", "14444441", "11111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 7,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.restored(3, 2), 3);
        assert_eq!(mask.restored(3, 4), 4);
    }

    #[test]
    fn lettering_touching_the_outline_only_at_a_corner_stays_lettering() {
        // The outline bump at (1,4) touches the letter at (2,3) only diagonally.
        let (indices, width) = grid(&[
            "1111111", "1333331", "1333331", "13a3331", "1233331", "1111111",
        ]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 6,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.class(1, 4), PixelClass::Frame);
        assert_eq!(mask.class(2, 3), PixelClass::Text);
        assert_eq!(mask.restored(2, 3), 3);
    }

    #[test]
    fn a_letter_whose_drop_shadow_touches_the_outline_stays_lettering() {
        // The letter's dark shadow (2) at (5,2) touches the outline column (1),
        // yet the white body behind it is not a fang: it never touches the
        // outline itself.
        let (indices, width) = grid(&["1111111", "1333331", "133ff21", "1333331", "1111111"]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 5,
        };
        let mask = analyze(&indices, width, region, COLORS, 0).expect("analyze");
        assert_eq!(mask.class(3, 2), PixelClass::Text);
        assert_eq!(mask.class(4, 2), PixelClass::Text);
        assert_eq!(
            mask.class(5, 2),
            PixelClass::Text,
            "shadow joins its letter"
        );
        assert_eq!(mask.class(6, 2), PixelClass::Frame, "outline stays");
        assert_eq!(mask.restored(4, 2), 3);
        assert_eq!(mask.restored(5, 2), 3);
    }

    #[test]
    fn a_region_without_detached_lettering_is_rejected() {
        let (indices, width) = grid(&["1111", "1331", "1111"]);
        let region = Region {
            x: 0,
            y: 0,
            width,
            height: 3,
        };
        assert!(analyze(&indices, width, region, COLORS, 1).is_err());
    }
}
