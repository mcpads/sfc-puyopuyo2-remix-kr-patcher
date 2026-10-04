use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Cursor,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const CHR_STREAM_PC: usize = 0x13_0F31;
const CHR_COMPRESSED_LEN: usize = 0x2091;
const CHR_DECOMPRESSED_LEN: usize = 0x29C0;
const CHR_STREAM_SHA256: &str = "0f0ac0c8b48521a9c032b349e7655006001c32651dda249c4179354a9bab7911";
const TILEMAP_STREAM_PC: usize = 0x12_FB16;
const TILEMAP_COMPRESSED_LEN: usize = 0x0142;
const TILEMAP_DECOMPRESSED_LEN: usize = 0x1000;
const TILEMAP_STREAM_SHA256: &str =
    "31946709ead947b872646c90a23ba275f50cc429da8d072c14d08c68fe41805d";
const TILE_BYTES: usize = 32;
const TILEMAP_HEIGHT: usize = 32;
const SCREEN_TILE_WIDTH: usize = 32;
const TITLE_X: usize = 4;
const TITLE_Y: usize = 2;
const TITLE_WIDTH: usize = 24;
const TITLE_HEIGHT: usize = 3;
const TITLE_PALETTE: u8 = 2;
const TITLE_FIRST_TILE: usize = 0x050;
const TITLE_LAST_TILE: usize = 0x095;
const RANKING_SHEET_PATH: &str = "assets/ranking_graphics/ranking_lettering_sheet.png";

fn read_ranking_sheet() -> Result<Vec<u8>> {
    std::fs::read(RANKING_SHEET_PATH)
        .with_context(|| format!("ranking lettering sheet {RANKING_SHEET_PATH} is unavailable"))
}

// Every syllable fills one tile-aligned 3x3 cell, so a repeated syllable reuses
// the same eight-by-eight tiles; the two titles fit the 70-tile budget only
// because 뿌 and 요 are drawn once.
const CELL_PIXELS: usize = 24;
const GLYPH_PIXELS: usize = 22;
const GLYPH_COVERAGE: f32 = 0.75;

// Measured from the ranking-screen runtime CGRAM. Index 0 is transparent in the
// editable layer; the remaining values preserve the game's yellow/brown style.
const PALETTE_2_BGR555: [u16; 16] = [
    0x0000, 0x0840, 0x1080, 0x1CE3, 0x35A9, 0x420C, 0x5290, 0x6314, 0x0108, 0x018C, 0x0210, 0x0294,
    0x0318, 0x139C, 0x43FF, 0x7FFF,
];

const KOREAN_TITLES: [(&str, &str); 2] = [
    ("ranking_solo", "혼자서 뿌요뿌요"),
    ("ranking_endless", "무한 뿌요뿌요"),
];

struct KoreanTitleCanvas {
    indices: Vec<u8>,
    ink_width: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RankingGraphicsKrPocReport {
    pub verdict: String,
    pub source_path: String,
    pub output_path: String,
    pub lettering_sheet: String,
    pub lettering_sheet_sha256: String,
    pub texts: Vec<String>,
    pub rendered_dimensions: Vec<String>,
    pub preview_files: Vec<String>,
    pub unique_tiles_used: usize,
    pub available_title_tiles: usize,
    pub chr_write_range: String,
    pub tilemap_write_range: String,
    pub chr_original_compressed_len: usize,
    pub chr_patched_compressed_len: usize,
    pub tilemap_original_compressed_len: usize,
    pub tilemap_patched_compressed_len: usize,
    pub compression_roundtrip_matches: bool,
    pub decoded_diffs_confined: bool,
    pub rom_diff_confined: bool,
    pub output_sha256: String,
}

fn decode_verified_stream(
    rom: &[u8],
    start: usize,
    compressed_len: usize,
    decompressed_len: usize,
    name: &str,
) -> Result<Vec<u8>> {
    let block = crate::snes_lz::decompress(rom, start)?;
    if block.compressed_len != compressed_len || block.bytes.len() != decompressed_len {
        bail!(
            "{name} stream differs from the verified JP ROM Spec: compressed 0x{:X}/0x{compressed_len:X}, decoded 0x{:X}/0x{decompressed_len:X}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    Ok(block.bytes)
}

fn verify_stream_bytes(
    rom: &[u8],
    start: usize,
    len: usize,
    expected_sha256: &str,
    name: &str,
) -> Result<()> {
    let bytes = rom
        .get(start..start + len)
        .with_context(|| format!("{name} stream range 0x{start:06X}..0x{:06X}", start + len))?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected_sha256 {
        bail!(
            "{name} compressed stream SHA-256 mismatch: expected {expected_sha256}, got {actual}"
        );
    }
    Ok(())
}

fn tilemap_word(tilemap: &[u8], screen: usize, x: usize, y: usize) -> Result<u16> {
    // A 64x32 SNES tilemap stores its two 32x32 screens consecutively, not as
    // one linear 64-word row.
    let index = screen * SCREEN_TILE_WIDTH * TILEMAP_HEIGHT + y * SCREEN_TILE_WIDTH + x;
    let offset = index * 2;
    let bytes = tilemap
        .get(offset..offset + 2)
        .with_context(|| format!("ranking tilemap word screen={screen}, x={x}, y={y}"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn decode_tile_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn bgr555_to_rgb(value: u16) -> [u8; 3] {
    [
        ((value & 0x1F) * 255 / 31) as u8,
        (((value >> 5) & 0x1F) * 255 / 31) as u8,
        (((value >> 10) & 0x1F) * 255 / 31) as u8,
    ]
}

fn set_4bpp_pixel(tile: &mut [u8; TILE_BYTES], x: usize, y: usize, value: u8) {
    let mask = 1 << (7 - x);
    for plane in 0..4 {
        let offset = if plane < 2 {
            y * 2 + plane
        } else {
            16 + y * 2 + plane - 2
        };
        if value & (1 << plane) != 0 {
            tile[offset] |= mask;
        } else {
            tile[offset] &= !mask;
        }
    }
}

/// Sample each title row of the generated sheet into shaded 24x24 syllable
/// cells and center the cell run on the 24x3-tile canvas at a tile boundary.
fn render_korean_titles() -> Result<Vec<KoreanTitleCanvas>> {
    use crate::generated_lettering::{decode_ink_sheet, shade_embossed};

    let sheet = decode_ink_sheet(&read_ranking_sheet()?)?;
    let rows = sheet.rows();
    if rows.len() != KOREAN_TITLES.len() {
        bail!(
            "{RANKING_SHEET_PATH} must hold {} title rows, found {}",
            KOREAN_TITLES.len(),
            rows.len()
        );
    }
    let mut shaded_cells = BTreeMap::<char, Vec<u8>>::new();
    let mut layouts = Vec::with_capacity(KOREAN_TITLES.len());
    for ((_, text), row) in KOREAN_TITLES.iter().zip(&rows) {
        let cells = sheet.compose_cells(row, text, CELL_PIXELS, GLYPH_PIXELS, GLYPH_COVERAGE)?;
        let mut layout = Vec::with_capacity(cells.len());
        for (ch, cell) in cells {
            shaded_cells
                .entry(ch)
                .or_insert_with(|| shade_embossed(&cell));
            layout.push(ch);
        }
        layouts.push(layout);
    }
    let canvas_width = TITLE_WIDTH * 8;
    let cell_tiles = CELL_PIXELS / 8;
    let mut canvases = Vec::with_capacity(layouts.len());
    for layout in layouts {
        let used_tiles = layout.len() * cell_tiles;
        if used_tiles > TITLE_WIDTH {
            bail!(
                "ranking title of {} syllables exceeds {TITLE_WIDTH} tile columns",
                layout.len()
            );
        }
        let start_x = (TITLE_WIDTH - used_tiles) / 2 * 8;
        let mut indices = vec![0u8; canvas_width * TITLE_HEIGHT * 8];
        for (position, ch) in layout.iter().enumerate() {
            let cell = &shaded_cells[ch];
            let cell_x = start_x + position * CELL_PIXELS;
            for y in 0..CELL_PIXELS {
                for x in 0..CELL_PIXELS {
                    indices[y * canvas_width + cell_x + x] = cell[y * CELL_PIXELS + x];
                }
            }
        }
        canvases.push(KoreanTitleCanvas {
            indices,
            ink_width: used_tiles * 8,
        });
    }
    Ok(canvases)
}

fn korean_title_tile(rendered: &KoreanTitleCanvas, tile_x: usize, tile_y: usize) -> [u8; 32] {
    let canvas_width = TITLE_WIDTH * 8;
    let mut tile = [0u8; TILE_BYTES];
    for y in 0..8 {
        for x in 0..8 {
            let pixel = (tile_y * 8 + y) * canvas_width + tile_x * 8 + x;
            set_4bpp_pixel(&mut tile, x, y, rendered.indices[pixel]);
        }
    }
    tile
}

fn flip_4bpp_tile(tile: &[u8; TILE_BYTES], hflip: bool, vflip: bool) -> [u8; TILE_BYTES] {
    let mut flipped = [0u8; TILE_BYTES];
    for y in 0..8 {
        for x in 0..8 {
            let source_x = if hflip { 7 - x } else { x };
            let source_y = if vflip { 7 - y } else { y };
            let value = decode_tile_pixel(tile, source_x, source_y);
            set_4bpp_pixel(&mut flipped, x, y, value);
        }
    }
    flipped
}

fn canonical_tile(tile: &[u8; TILE_BYTES]) -> ([u8; TILE_BYTES], bool, bool) {
    let mut best = (*tile, false, false);
    for (hflip, vflip) in [(true, false), (false, true), (true, true)] {
        let candidate = flip_4bpp_tile(tile, hflip, vflip);
        if candidate < best.0 {
            best = (candidate, hflip, vflip);
        }
    }
    best
}

fn preview_rgba(rendered: &KoreanTitleCanvas) -> Vec<u8> {
    let width = TITLE_WIDTH * 8;
    let height = TITLE_HEIGHT * 8;
    let mut rgba = vec![0u8; width * height * 4];
    for pixel in 0..width * height {
        let index = rendered.indices[pixel];
        if index != 0 {
            let rgb = bgr555_to_rgb(PALETTE_2_BGR555[usize::from(index)]);
            rgba[pixel * 4..pixel * 4 + 3].copy_from_slice(&rgb);
            rgba[pixel * 4 + 3] = 0xFF;
        }
    }
    rgba
}

fn encode_png_rgba(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut output), width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
        writer.finish()?;
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub fn build_ranking_graphics_kr_poc(
    base: &[u8],
    source_path: String,
    preview_dir: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, RankingGraphicsKrPocReport)> {
    verify_stream_bytes(
        base,
        CHR_STREAM_PC,
        CHR_COMPRESSED_LEN,
        CHR_STREAM_SHA256,
        "ranking CHR",
    )?;
    verify_stream_bytes(
        base,
        TILEMAP_STREAM_PC,
        TILEMAP_COMPRESSED_LEN,
        TILEMAP_STREAM_SHA256,
        "ranking tilemap",
    )?;
    let original_chr = decode_verified_stream(
        base,
        CHR_STREAM_PC,
        CHR_COMPRESSED_LEN,
        CHR_DECOMPRESSED_LEN,
        "ranking CHR",
    )?;
    let original_tilemap = decode_verified_stream(
        base,
        TILEMAP_STREAM_PC,
        TILEMAP_COMPRESSED_LEN,
        TILEMAP_DECOMPRESSED_LEN,
        "ranking tilemap",
    )?;
    if original_chr[..TILE_BYTES].iter().any(|byte| *byte != 0) {
        bail!("ranking tile 0 is not the verified transparent blank tile");
    }

    let target_coordinates = (0..2)
        .flat_map(|screen| {
            (TITLE_Y..TITLE_Y + TITLE_HEIGHT)
                .flat_map(move |y| (TITLE_X..TITLE_X + TITLE_WIDTH).map(move |x| (screen, x, y)))
        })
        .collect::<BTreeSet<_>>();
    let mut original_title_tiles = BTreeSet::new();
    for screen in 0..2 {
        for y in 0..TILEMAP_HEIGHT {
            for x in 0..SCREEN_TILE_WIDTH {
                let word = tilemap_word(&original_tilemap, screen, x, y)?;
                let tile = usize::from(word & 0x03FF);
                let in_title = target_coordinates.contains(&(screen, x, y));
                if (TITLE_FIRST_TILE..=TITLE_LAST_TILE).contains(&tile) {
                    if !in_title {
                        bail!(
                            "ranking title tile 0x{tile:03X} is referenced outside the verified title region at screen={screen}, x={x}, y={y}"
                        );
                    }
                    original_title_tiles.insert(tile);
                }
                if in_title {
                    if (word & 0xFC00) != u16::from(TITLE_PALETTE) << 10 {
                        bail!(
                            "ranking title word at screen={screen}, x={x}, y={y} has unexpected attributes 0x{word:04X}"
                        );
                    }
                    if tile != 0 && !(TITLE_FIRST_TILE..=TITLE_LAST_TILE).contains(&tile) {
                        bail!(
                            "ranking title word at screen={screen}, x={x}, y={y} uses non-title tile 0x{tile:03X}"
                        );
                    }
                }
            }
        }
    }
    let available_title_tiles = TITLE_LAST_TILE - TITLE_FIRST_TILE + 1;
    if original_title_tiles.len() != available_title_tiles {
        bail!(
            "ranking title map owns {} unique tiles, expected {available_title_tiles}",
            original_title_tiles.len()
        );
    }

    let rendered_titles = render_korean_titles()?;

    let mut tile_patterns = BTreeMap::<[u8; TILE_BYTES], usize>::new();
    let mut next_tile = TITLE_FIRST_TILE;
    let mut patched_tilemap = original_tilemap.clone();
    let mut patched_chr = original_chr.clone();
    for (screen, rendered) in rendered_titles.iter().enumerate() {
        for local_y in 0..TITLE_HEIGHT {
            for local_x in 0..TITLE_WIDTH {
                let tile = korean_title_tile(rendered, local_x, local_y);
                let word = if tile.iter().all(|byte| *byte == 0) {
                    u16::from(TITLE_PALETTE) << 10
                } else {
                    let (canonical, hflip, vflip) = canonical_tile(&tile);
                    let tile_id = if let Some(tile_id) = tile_patterns.get(&canonical) {
                        *tile_id
                    } else {
                        if next_tile > TITLE_LAST_TILE {
                            bail!(
                                "Korean ranking titles need more than {available_title_tiles} unique 8x8 tiles"
                            );
                        }
                        let tile_id = next_tile;
                        next_tile += 1;
                        tile_patterns.insert(canonical, tile_id);
                        patched_chr[tile_id * TILE_BYTES..(tile_id + 1) * TILE_BYTES]
                            .copy_from_slice(&canonical);
                        tile_id
                    };
                    u16::try_from(tile_id)?
                        | (u16::from(TITLE_PALETTE) << 10)
                        | if hflip { 0x4000 } else { 0 }
                        | if vflip { 0x8000 } else { 0 }
                };
                let x = TITLE_X + local_x;
                let y = TITLE_Y + local_y;
                let index = screen * SCREEN_TILE_WIDTH * TILEMAP_HEIGHT + y * SCREEN_TILE_WIDTH + x;
                patched_tilemap[index * 2..index * 2 + 2].copy_from_slice(&word.to_le_bytes());
            }
        }
    }

    let chr_changed_only_in_title_tiles =
        original_chr
            .iter()
            .zip(&patched_chr)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || (TITLE_FIRST_TILE * TILE_BYTES..(TITLE_LAST_TILE + 1) * TILE_BYTES)
                        .contains(&offset)
            });
    let tilemap_changed_only_in_title_words = original_tilemap
        .as_chunks::<2>()
        .0
        .iter()
        .zip(patched_tilemap.as_chunks::<2>().0.iter())
        .enumerate()
        .all(|(index, (before, after))| {
            if before == after {
                return true;
            }
            let screen = index / (SCREEN_TILE_WIDTH * TILEMAP_HEIGHT);
            let within_screen = index % (SCREEN_TILE_WIDTH * TILEMAP_HEIGHT);
            let y = within_screen / SCREEN_TILE_WIDTH;
            let x = within_screen % SCREEN_TILE_WIDTH;
            target_coordinates.contains(&(screen, x, y))
        });
    let decoded_diffs_confined =
        chr_changed_only_in_title_tiles && tilemap_changed_only_in_title_words;
    if !decoded_diffs_confined {
        bail!("Korean ranking decoded diff escaped the declared title ranges");
    }

    let compressed_chr = crate::snes_lz::compress(&patched_chr);
    let compressed_tilemap = crate::snes_lz::compress(&patched_tilemap);
    if compressed_chr.len() > CHR_COMPRESSED_LEN {
        bail!(
            "Korean ranking CHR grew from {CHR_COMPRESSED_LEN} to {} bytes",
            compressed_chr.len()
        );
    }
    if compressed_tilemap.len() > TILEMAP_COMPRESSED_LEN {
        bail!(
            "Korean ranking tilemap grew from {TILEMAP_COMPRESSED_LEN} to {} bytes",
            compressed_tilemap.len()
        );
    }
    let chr_roundtrip = crate::snes_lz::decompress(&compressed_chr, 0)?.bytes == patched_chr;
    let tilemap_roundtrip =
        crate::snes_lz::decompress(&compressed_tilemap, 0)?.bytes == patched_tilemap;
    let compression_roundtrip_matches = chr_roundtrip && tilemap_roundtrip;
    if !compression_roundtrip_matches {
        bail!("Korean ranking compression round-trip did not reproduce intended assets");
    }

    let chr_write_range = CHR_STREAM_PC..CHR_STREAM_PC + CHR_COMPRESSED_LEN;
    let tilemap_write_range = TILEMAP_STREAM_PC..TILEMAP_STREAM_PC + TILEMAP_COMPRESSED_LEN;
    let mut patched = base.to_vec();
    patched[chr_write_range.clone()].fill(0);
    patched[CHR_STREAM_PC..CHR_STREAM_PC + compressed_chr.len()].copy_from_slice(&compressed_chr);
    patched[tilemap_write_range.clone()].fill(0);
    patched[TILEMAP_STREAM_PC..TILEMAP_STREAM_PC + compressed_tilemap.len()]
        .copy_from_slice(&compressed_tilemap);
    let rom_diff_confined =
        base.iter()
            .zip(&patched)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || chr_write_range.contains(&offset)
                    || tilemap_write_range.contains(&offset)
            });
    if !rom_diff_confined {
        bail!("Korean ranking ROM diff escaped the declared compressed stream ranges");
    }

    let mut preview_files = Vec::new();
    if let Some(preview_dir) = preview_dir {
        fs::create_dir_all(preview_dir).with_context(|| {
            format!(
                "create ranking Korean preview dir {}",
                preview_dir.display()
            )
        })?;
        for ((id, _), rendered) in KOREAN_TITLES.iter().zip(&rendered_titles) {
            let filename = format!("{id}_ko.png");
            fs::write(
                preview_dir.join(&filename),
                encode_png_rgba(TITLE_WIDTH * 8, TITLE_HEIGHT * 8, &preview_rgba(rendered))?,
            )?;
            preview_files.push(preview_dir.join(filename).display().to_string());
        }
    }

    let report = RankingGraphicsKrPocReport {
        verdict: "generated embossed Korean solo/endless ranking titles inserted into verified BG1 CHR and two-screen tilemap streams"
            .to_owned(),
        source_path,
        output_path,
        lettering_sheet: RANKING_SHEET_PATH.to_owned(),
        lettering_sheet_sha256: format!("{:x}", Sha256::digest(read_ranking_sheet()?)),
        texts: KOREAN_TITLES
            .iter()
            .map(|(_, text)| (*text).to_owned())
            .collect(),
        rendered_dimensions: rendered_titles
            .iter()
            .map(|rendered| format!("{}x{}", rendered.ink_width, TITLE_HEIGHT * 8))
            .collect(),
        preview_files,
        unique_tiles_used: tile_patterns.len(),
        available_title_tiles,
        chr_write_range: format!(
            "0x{CHR_STREAM_PC:06X}..0x{:06X}",
            CHR_STREAM_PC + CHR_COMPRESSED_LEN
        ),
        tilemap_write_range: format!(
            "0x{TILEMAP_STREAM_PC:06X}..0x{:06X}",
            TILEMAP_STREAM_PC + TILEMAP_COMPRESSED_LEN
        ),
        chr_original_compressed_len: CHR_COMPRESSED_LEN,
        chr_patched_compressed_len: compressed_chr.len(),
        tilemap_original_compressed_len: TILEMAP_COMPRESSED_LEN,
        tilemap_patched_compressed_len: compressed_tilemap.len(),
        compression_roundtrip_matches,
        decoded_diffs_confined,
        rom_diff_confined,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires assets/ranking_graphics/ranking_lettering_sheet.png"]
    fn repeated_syllables_share_cells_within_the_tile_budget() {
        let titles = render_korean_titles().unwrap();
        let mut tiles = BTreeSet::new();
        for title in &titles {
            for tile_y in 0..TITLE_HEIGHT {
                for tile_x in 0..TITLE_WIDTH {
                    let tile = korean_title_tile(title, tile_x, tile_y);
                    if tile.iter().any(|&byte| byte != 0) {
                        tiles.insert(canonical_tile(&tile).0);
                    }
                }
            }
        }
        assert!(tiles.len() <= TITLE_LAST_TILE - TITLE_FIRST_TILE + 1);
    }

    #[test]
    #[ignore = "requires the Remix JP ROM and assets/ranking_graphics/ranking_lettering_sheet.png"]
    fn builds_generated_korean_ranking_titles() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let (patched, report) = build_ranking_graphics_kr_poc(
            &rom,
            "test:remix".to_owned(),
            None,
            "test:ranking".to_owned(),
        )
        .unwrap();

        assert_eq!(patched.len(), rom.len());
        assert_eq!(report.texts, ["혼자서 뿌요뿌요", "무한 뿌요뿌요"]);
        assert!(report.unique_tiles_used <= report.available_title_tiles);
        assert!(report.compression_roundtrip_matches);
        assert!(report.decoded_diffs_confined);
        assert!(report.rom_diff_confined);
    }
}
