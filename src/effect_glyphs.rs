//! Pixel masks for Korean gameplay effect text.
//!
//! The 16px masks start from Galmuri11 Bold rasterized at 12px and are
//! corrected by hand where stacked strokes merge at that size (`통`, `불`,
//! `쓸`). The 8x8 chain-counter masks are drawn by hand to match the original
//! 7x7 body plus one-pixel drop shadow. Keeping the masks as source text makes
//! every pixel reviewable and removes the TTF dependency from these stages.

use anyhow::{Result, bail};

/// A glyph body: `#` is ink, `.` is empty. Rows may differ in width.
pub type Mask = &'static [&'static str];

const GLYPHS: &[(char, Mask)] = &[
    (
        '단',
        &[
            "######..##.",
            "######..##.",
            "##......###",
            "##......###",
            "#######.##.",
            "#######.##.",
            "........##.",
            ".##........",
            ".##........",
            ".#########.",
            ".#########.",
        ],
    ),
    (
        '맛',
        &[
            "######..##.",
            "######..##.",
            "##..##..###",
            "##..##..###",
            "######..##.",
            "######..##.",
            "........##.",
            "....###....",
            "...#####...",
            ".####.####.",
            ".###...###.",
        ],
    ),
    (
        '순',
        &[
            "....###....",
            "...#####...",
            ".####.####.",
            ".###...###.",
            "...........",
            "###########",
            ".....##....",
            ".##..##....",
            ".##........",
            ".#########.",
            ".#########.",
        ],
    ),
    (
        '함',
        &[
            "...##...##.",
            "##########.",
            "..####..###",
            ".##..##.###",
            ".######.##.",
            "..####..##.",
            "...........",
            ".#########.",
            ".##.....##.",
            ".#########.",
            ".#########.",
        ],
    ),
    (
        '보',
        &[
            ".##.....##.",
            ".#########.",
            ".#########.",
            ".##.....##.",
            ".#########.",
            ".#########.",
            ".....##....",
            ".....##....",
            "###########",
            "###########",
        ],
    ),
    (
        '통',
        &[
            ".#########.",
            ".##........",
            ".#########.",
            ".##........",
            ".#########.",
            ".....##....",
            "###########",
            "...........",
            "..#######..",
            ".##.....##.",
            ".##.....##.",
            "..#######..",
        ],
    ),
    (
        '매',
        &[
            "#####.#.##",
            "#####.#.##",
            "##.##.#.##",
            "##.##.#.##",
            "##.##.####",
            "##.##.####",
            "##.##.#.##",
            "##.##.#.##",
            "#####.#.##",
            "#####.#.##",
            "......#.##",
        ],
    ),
    (
        '움',
        &[
            "..#######..",
            ".##.....##.",
            ".#########.",
            "..#######..",
            "...........",
            "###########",
            ".....##....",
            ".#########.",
            ".##.....##.",
            ".#########.",
            ".#########.",
        ],
    ),
    (
        '불',
        &[
            ".##.....##.",
            ".#########.",
            ".##.....##.",
            ".#########.",
            "...........",
            "###########",
            ".....##....",
            ".#########.",
            ".........##",
            ".#########.",
            ".##........",
            ".#########.",
        ],
    ),
    (
        '상',
        &[
            "..##....##.",
            "..##....##.",
            "..##....###",
            ".####...###",
            "######..##.",
            "##..##..##.",
            "...........",
            "..#######..",
            ".##.....##.",
            ".#########.",
            "..#######..",
        ],
    ),
    (
        '쇄',
        &[
            "..##..#.##",
            "..##..#.##",
            ".####.#.##",
            ".####.#.##",
            "##########",
            "##..######",
            "..##..#.##",
            "..##..#.##",
            "#######.##",
            "#######.##",
            "......#.##",
        ],
    ),
    (
        '휴',
        &[
            ".....##....",
            "###########",
            "..#######..",
            ".##.....##.",
            ".#########.",
            "..#######..",
            "...........",
            "###########",
            "..##...##..",
            "..##...##..",
            "..##...##..",
        ],
    ),
    (
        '식',
        &[
            "..##....##",
            "..##....##",
            "..##....##",
            ".####...##",
            "######..##",
            "##..##..##",
            "..........",
            ".#########",
            ".#########",
            "........##",
            "........##",
        ],
    ),
    (
        '중',
        &[
            ".#########.",
            ".#########.",
            "...##.##...",
            ".###...###.",
            "...........",
            "###########",
            ".....##....",
            "..#######..",
            ".##.....##.",
            ".#########.",
            "..#######..",
        ],
    ),
    (
        '싹',
        &[
            ".##.##..##.",
            ".##.##..##.",
            ".#####..###",
            "#######.###",
            "##.#.##.##.",
            "##.#.##.##.",
            "...........",
            ".#########.",
            ".#########.",
            "........##.",
            "........##.",
        ],
    ),
    (
        '쓸',
        &[
            "..#.....#..",
            ".###...###.",
            "##.##.##.##",
            "...........",
            "###########",
            "...........",
            ".#########.",
            ".........##",
            ".#########.",
            ".##........",
            ".#########.",
        ],
    ),
    (
        '이',
        &[
            ".####...##",
            "######..##",
            "##..##..##",
            "##..##..##",
            "##..##..##",
            "##..##..##",
            "##..##..##",
            "##..##..##",
            "######..##",
            ".####...##",
            "........##",
        ],
    ),
    (
        '!',
        &[
            "##", "##", "##", "##", "##", "##", "##", "##", "..", "##", "##",
        ],
    ),
];

const CHAIN_GLYPHS: &[(char, Mask)] = &[
    (
        '연',
        &[
            ".#...#.", "#.#.##.", ".#...#.", "....##.", ".....#.", "#......", "#####..",
        ],
    ),
    (
        '쇄',
        &[
            ".#..#.#", ".#..#.#", "#.#.###", "....#.#", ".#..#.#", "###.#.#", "....#.#",
        ],
    ),
];

/// Mask for a 16px effect glyph.
pub fn glyph(character: char) -> Result<Mask> {
    lookup(GLYPHS, character, "effect")
}

/// Mask for an 8x8 chain-counter glyph (7x7 body).
pub fn chain_glyph(character: char) -> Result<Mask> {
    lookup(CHAIN_GLYPHS, character, "chain-counter")
}

fn lookup(table: &[(char, Mask)], character: char, kind: &str) -> Result<Mask> {
    match table.iter().find(|(candidate, _)| *candidate == character) {
        Some((_, mask)) => Ok(mask),
        None => bail!("no {kind} pixel mask for {character:?}"),
    }
}

/// Width of the widest row.
pub fn width(mask: Mask) -> usize {
    mask.iter().map(|row| row.len()).max().unwrap_or(0)
}

/// Stamps `mask` into a `width` x `height` body canvas at (`x0`, `y0`),
/// scaling each mask pixel to `scale` x `scale` canvas pixels.
pub fn stamp(
    canvas: &mut [bool],
    width: usize,
    height: usize,
    mask: Mask,
    x0: usize,
    y0: usize,
    scale: usize,
) -> Result<()> {
    for (y, row) in mask.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            if cell != b'#' {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let cx = x0 + x * scale + dx;
                    let cy = y0 + y * scale + dy;
                    if cx >= width || cy >= height {
                        bail!(
                            "effect glyph pixel ({cx}, {cy}) falls outside the {width}x{height} canvas"
                        );
                    }
                    canvas[cy * width + cx] = true;
                }
            }
        }
    }
    Ok(())
}

/// Non-ink cells connected to the canvas border through non-ink cells.
/// Enclosed counters are excluded so outlines never fill them.
pub fn exterior(body: &[bool], width: usize, height: usize) -> Vec<bool> {
    let mut outside = vec![false; body.len()];
    let mut stack = Vec::new();
    for x in 0..width {
        stack.push((x, 0));
        stack.push((x, height - 1));
    }
    for y in 0..height {
        stack.push((0, y));
        stack.push((width - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        let index = y * width + x;
        if outside[index] || body[index] {
            continue;
        }
        outside[index] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < width {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < height {
            stack.push((x, y + 1));
        }
    }
    outside
}

/// Palette indices for a white-on-shadow glyph in the original effect style:
/// ink uses `body`, and the empty cells right of, below and diagonally below
/// ink use `shadow`.
pub fn drop_shadow(body: &[bool], width: usize, height: usize, ink: u8, shadow: u8) -> Vec<u8> {
    let mut pixels = vec![0u8; body.len()];
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            if body[index] {
                pixels[index] = ink;
            } else if [(1, 0), (0, 1), (1, 1)]
                .iter()
                .any(|(dx, dy)| x >= *dx && y >= *dy && body[(y - dy) * width + x - dx])
            {
                pixels[index] = shadow;
            }
        }
    }
    pixels
}

/// Encodes an 8x8 region of an indexed canvas as one SNES 4bpp tile.
pub fn encode_4bpp(pixels: &[u8], width: usize, x0: usize, y0: usize) -> [u8; 32] {
    let mut tile = [0u8; 32];
    for y in 0..8usize {
        for x in 0..8usize {
            let value = pixels[(y0 + y) * width + x0 + x];
            let bit = 1 << (7 - x);
            for plane in 0..4usize {
                if value & (1 << plane) != 0 {
                    let offset = if plane < 2 {
                        y * 2 + plane
                    } else {
                        16 + y * 2 + plane - 2
                    };
                    tile[offset] |= bit;
                }
            }
        }
    }
    tile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mask_fits_its_cell() {
        for (character, mask) in GLYPHS {
            assert!(
                mask.len() <= 13 && width(mask) <= 13,
                "{character} mask too large"
            );
        }
        for (character, mask) in CHAIN_GLYPHS {
            assert!(
                mask.len() <= 7 && width(mask) <= 7,
                "{character} chain mask too large"
            );
        }
    }

    #[test]
    fn exterior_excludes_enclosed_counters() {
        // A 5x5 ring leaves its centre enclosed.
        let mut body = vec![false; 7 * 7];
        for y in 1..6 {
            for x in 1..6 {
                body[y * 7 + x] = x == 1 || x == 5 || y == 1 || y == 5;
            }
        }
        let outside = exterior(&body, 7, 7);
        assert!(outside[0]);
        assert!(!outside[3 * 7 + 3]);
    }
}
