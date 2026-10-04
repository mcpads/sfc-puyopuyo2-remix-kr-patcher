use anyhow::{Context, Result, bail};

use crate::story_font::GLYPH_LEN;

const TRAILING_PUNCTUATION: [char; 4] = ['.', ',', '!', '?'];

pub struct StoryFontRasterizer {
    font: fontdue::Font,
    px: f32,
}

impl StoryFontRasterizer {
    pub fn new(ttf_data: &[u8], px: f32) -> Result<Self> {
        if !px.is_finite() || px <= 0.0 {
            bail!("TTF pixel size must be positive, got {px}");
        }
        let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
            .map_err(|error| anyhow::anyhow!("failed to parse TTF: {error}"))?;
        Ok(Self { font, px })
    }

    pub fn render(&self, character: char) -> Result<[u8; GLYPH_LEN]> {
        let bitmap = render_16x16_bitmap(&self.font, character, self.px)?;
        Ok(pack_story_1bpp(&bitmap))
    }
}

fn render_16x16_bitmap(font: &fontdue::Font, character: char, px: f32) -> Result<[bool; 256]> {
    let (metrics, raster) = font.rasterize(character, px);
    if metrics.width == 0 || metrics.height == 0 || raster.iter().all(|coverage| *coverage == 0) {
        bail!("TTF produced an empty glyph for {character:?} at {px}px");
    }
    let ascent = font
        .horizontal_line_metrics(px)
        .context("TTF has no horizontal line metrics")?
        .ascent as i32;
    // Every code occupies a fixed 16px cell, so a centered period or comma
    // reads as a separate word. Trailing punctuation hugs the previous glyph.
    let x_offset = if TRAILING_PUNCTUATION.contains(&character) {
        1
    } else {
        ((16i32 - metrics.width as i32) / 2 - metrics.xmin).max(0) as usize
    };
    let baseline = (16 + ascent) / 2;
    // Descenders such as a comma's tail may pass the cell bottom; lift only
    // those glyphs so the shared baseline stays unchanged for the rest.
    let y_offset = (baseline - metrics.ymin - metrics.height as i32)
        .min(16 - metrics.height as i32)
        .max(0) as usize;
    let mut bitmap = [false; 256];
    for row in 0..metrics.height {
        for column in 0..metrics.width {
            if raster[row * metrics.width + column] < 128 {
                continue;
            }
            let x = x_offset + column;
            let y = y_offset + row;
            if x >= 16 || y >= 16 {
                bail!("TTF glyph for {character:?} at {px}px does not fit the 16x16 cell");
            }
            bitmap[y * 16 + x] = true;
        }
    }
    if bitmap.iter().all(|pixel| !pixel) {
        bail!("TTF glyph for {character:?} rendered no ink at {px}px");
    }
    Ok(bitmap)
}

fn pack_story_1bpp(bitmap: &[bool; 256]) -> [u8; GLYPH_LEN] {
    let mut output = [0; GLYPH_LEN];
    for (tile_index, (x_offset, y_offset)) in [(0usize, 0usize), (8, 0), (0, 8), (8, 8)]
        .into_iter()
        .enumerate()
    {
        for row in 0..8 {
            let mut bits = 0;
            for column in 0..8 {
                if bitmap[(y_offset + row) * 16 + x_offset + column] {
                    bits |= 1 << (7 - column);
                }
            }
            output[tile_index * 8 + row] = bits;
        }
    }
    output
}
