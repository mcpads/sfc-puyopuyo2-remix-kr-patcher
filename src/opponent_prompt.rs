use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(crate) const STREAM_PC: usize = 0x0B_6748;
pub(crate) const ORIGINAL_COMPRESSED_LEN: usize = 1_530;
const DECOMPRESSED_LEN: usize = 2_048;
const RUNTIME_VRAM_BYTE: usize = 0xA000;
const TILE_LEN: usize = 16;
// Remix keeps the original prompt tile order but relocates its tilemap table.
const TILEMAP_TABLE_PC: usize = 0x0D_56F2;
const PROMPT_KO: [&str; 2] = ["상대를", "골라주세요"];
const FONT_PALETTE: usize = 3;
const EXPECTED_PALETTE_BYTES: [u8; 8] = [0x00, 0x00, 0xDF, 0x00, 0x1B, 0x00, 0x2A, 0x00];
const EXPECTED_DECODED_SHA256: &str =
    "3b24a05cb1c29577d39b03bc55a12a5f92e134b29fb7143328899dc0d1523289";
const BACKGROUND_INDEX: u8 = 0;
// The original prompt fills a 15x15 mask with index 2, lights its left and
// top edges with index 1 and drops a one-pixel index-3 shadow to the right
// and below. Measured on the Remix Japanese prompt tiles $54-$73.
const EDGE_INDEX: u8 = 1;
const FILL_INDEX: u8 = 2;
const SHADOW_INDEX: u8 = 3;
const MASK_SIZE: usize = 15;

// Hand-drawn 15x15 masks. The BMJUA silhouettes were fitted to the original
// glyph box; `를` and `골` were redrawn because their stacked strokes merge
// when a TTF is reduced to this size.
const GLYPH_MASKS: [(char, [&str; MASK_SIZE]); 8] = [
    (
        '상',
        [
            "....##.....###.",
            "....##.....###.",
            "...####....###.",
            "...####....###.",
            "..######...####",
            ".###..###..###.",
            "###....###.###.",
            "##......##.###.",
            "...............",
            "....#########..",
            "..###.....###..",
            "..###.....###..",
            "..###.....###..",
            "...#########...",
            "...............",
        ],
    ),
    (
        '대',
        [
            "..........##.##",
            ".#######..##.##",
            ".#######..##.##",
            ".###......##.##",
            ".###......##.##",
            ".###......##.##",
            ".###......#####",
            ".###......#####",
            ".###......##.##",
            ".###......##.##",
            ".###......##.##",
            ".###......##.##",
            ".#######..##.##",
            ".#######..##.##",
            "...............",
        ],
    ),
    (
        '를',
        [
            ".#############.",
            "...........###.",
            ".#############.",
            ".###...........",
            ".#############.",
            "...............",
            "###############",
            "...............",
            ".#############.",
            "...........###.",
            ".#############.",
            ".###...........",
            ".#############.",
            "...............",
            "...............",
        ],
    ),
    (
        '골',
        [
            "...............",
            ".############..",
            "..........###..",
            "..........###..",
            "......###......",
            "###############",
            "...............",
            ".############..",
            "..........###..",
            "..........###..",
            ".############..",
            ".###...........",
            ".###...........",
            ".############..",
            "...............",
        ],
    ),
    (
        '라',
        [
            "...........##..",
            ".#######...##..",
            ".#######...##..",
            "......###..##..",
            "......###..##..",
            "......###..##..",
            ".#######...####",
            ".#######...####",
            ".###.......##..",
            ".###.......##..",
            ".###.......##..",
            ".#########.##..",
            ".#########.##..",
            "...........##..",
            "...............",
        ],
    ),
    (
        '주',
        [
            ".############..",
            ".############..",
            ".......###.....",
            "......####.....",
            ".....######....",
            "...####..####..",
            "..###......###.",
            "...............",
            "###############",
            "###############",
            "......###......",
            "......###......",
            "......###......",
            "......###......",
            "...............",
        ],
    ),
    (
        '세',
        [
            "...........##.#",
            "....##.....##.#",
            "....##.....##.#",
            "....##.....##.#",
            "....##.....##.#",
            "....##...####.#",
            "...####..####.#",
            "...####....##.#",
            "..######...##.#",
            "..###.##...##.#",
            ".###...##..##.#",
            "###.....##.##.#",
            "##......#..##.#",
            "...........##.#",
            "...............",
        ],
    ),
    (
        '요',
        [
            "....#######....",
            "..###########..",
            ".####.....####.",
            ".###.......###.",
            ".###.......###.",
            ".####.....####.",
            "..###########..",
            "....#######....",
            "....##...##....",
            "....##...##....",
            "....##...##....",
            "###############",
            "###############",
            "...............",
            "...............",
        ],
    ),
];

const FIRST_LINE_TOP: [usize; 6] = [0x54, 0x55, 0x56, 0x57, 0x58, 0x59];
const FIRST_LINE_BOTTOM: [usize; 6] = [0x60, 0x61, 0x62, 0x63, 0x64, 0x65];
const SECOND_LINE_TOP: [usize; 10] = [0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x70, 0x71, 0x72, 0x73];
const SECOND_LINE_BOTTOM: [usize; 10] =
    [0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F];

const EXPECTED_TILEMAP_TABLE: [u8; 64] = [
    0x54, 0x2C, 0x55, 0x2C, 0x56, 0x2C, 0x57, 0x2C, 0x58, 0x2C, 0x59, 0x2C, 0x5A, 0x2C, 0x5B, 0x2C,
    0x5C, 0x2C, 0x5D, 0x2C, 0x5E, 0x2C, 0x5F, 0x2C, 0x70, 0x2C, 0x71, 0x2C, 0x72, 0x2C, 0x73, 0x2C,
    0x60, 0x2C, 0x61, 0x2C, 0x62, 0x2C, 0x63, 0x2C, 0x64, 0x2C, 0x65, 0x2C, 0x66, 0x2C, 0x67, 0x2C,
    0x68, 0x2C, 0x69, 0x2C, 0x6A, 0x2C, 0x6B, 0x2C, 0x6C, 0x2C, 0x6D, 0x2C, 0x6E, 0x2C, 0x6F, 0x2C,
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpponentPromptKrPocReport {
    pub verdict: String,
    pub source_path: String,
    pub ttf_path: String,
    pub ttf_sha256: String,
    pub output_path: String,
    pub layout_source: String,
    pub text: Vec<String>,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub runtime_vram_range: String,
    pub runtime_vram_matches_original: bool,
    pub tilemap_table_pc: String,
    pub tilemap_table_lorom: String,
    pub tilemap_unchanged: bool,
    pub line_widths_px: Vec<usize>,
    pub tile_ids: Vec<String>,
    pub palette: usize,
    pub palette_bytes_hex: String,
    pub font_px: f32,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_write_range: bool,
    pub output_sha256: String,
}

pub struct OpponentPromptKrPocInputs<'a> {
    pub rom: &'a [u8],
    pub source_path: String,
    pub ttf_path: String,
    pub ttf_data: &'a [u8],
    pub font_px: f32,
    pub runtime_vram: Option<&'a [u8]>,
    pub runtime_cram: Option<&'a [u8]>,
    pub output_path: String,
}

pub fn build_opponent_prompt_kr_poc(
    inputs: &OpponentPromptKrPocInputs<'_>,
) -> Result<(Vec<u8>, OpponentPromptKrPocReport)> {
    let OpponentPromptKrPocInputs {
        rom,
        source_path,
        ttf_path,
        ttf_data,
        font_px,
        runtime_vram,
        runtime_cram,
        output_path,
    } = inputs;
    if !font_px.is_finite() || *font_px <= 0.0 {
        bail!("BMJUA pixel size must be positive, got {font_px}");
    }

    let block = crate::snes_lz::decompress(rom, STREAM_PC)?;
    if block.compressed_len != ORIGINAL_COMPRESSED_LEN || block.bytes.len() != DECOMPRESSED_LEN {
        bail!(
            "opponent prompt font stream differs from the verified ROM Spec: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let decoded_sha256 = format!("{:x}", Sha256::digest(&block.bytes));
    if decoded_sha256 != EXPECTED_DECODED_SHA256 {
        bail!(
            "opponent prompt decoded font SHA-256 differs from the verified ROM Spec: {decoded_sha256}"
        );
    }

    let table_end = TILEMAP_TABLE_PC + EXPECTED_TILEMAP_TABLE.len();
    if rom.get(TILEMAP_TABLE_PC..table_end) != Some(EXPECTED_TILEMAP_TABLE.as_slice()) {
        bail!("opponent prompt tilemap table differs from the verified ROM Spec");
    }

    let layout_source;
    let runtime_vram_matches_original =
        if let (Some(runtime_vram), Some(runtime_cram)) = (*runtime_vram, *runtime_cram) {
            layout_source = "runtime evidence".to_owned();
            let runtime_end = RUNTIME_VRAM_BYTE + DECOMPRESSED_LEN;
            let matches =
                runtime_vram.get(RUNTIME_VRAM_BYTE..runtime_end) == Some(block.bytes.as_slice());
            if !matches {
                bail!("opponent prompt source does not match runtime BG3 VRAM $A000-$A7FF");
            }
            let palette_start = FONT_PALETTE * 8;
            if runtime_cram.get(palette_start..palette_start + 8) != Some(&EXPECTED_PALETTE_BYTES) {
                bail!("opponent prompt runtime BG3 palette 3 differs from the verified colors");
            }
            true
        } else if runtime_vram.is_none() && runtime_cram.is_none() {
            layout_source = "hardcoded verified ROM tilemap/palette Spec".to_owned();
            true
        } else {
            bail!("opponent prompt runtime VRAM and CGRAM must both be present or omitted");
        };

    let mut decoded = block.bytes.clone();
    replace_line(
        &mut decoded,
        PROMPT_KO[0],
        &FIRST_LINE_TOP,
        &FIRST_LINE_BOTTOM,
    )?;
    replace_line(
        &mut decoded,
        PROMPT_KO[1],
        &SECOND_LINE_TOP,
        &SECOND_LINE_BOTTOM,
    )?;

    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    let target_tiles = target_tiles();
    for (index, (before, after)) in block.bytes.iter().zip(&decoded).enumerate() {
        if before != after && !target_tiles.contains(&(index / TILE_LEN)) {
            bail!("unexplained opponent prompt decoded change at 0x{index:04X}");
        }
    }

    let compressed = crate::snes_lz::compress(&decoded);
    if compressed.len() > ORIGINAL_COMPRESSED_LEN {
        bail!(
            "Korean opponent prompt stream grew to {} bytes, beyond the {}-byte in-place extent",
            compressed.len(),
            ORIGINAL_COMPRESSED_LEN
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == decoded;
    if !compression_roundtrip_matches {
        bail!("Korean opponent prompt compression round-trip failed");
    }

    let mut patched = rom.to_vec();
    let write_end = STREAM_PC + compressed.len();
    patched
        .get_mut(STREAM_PC..write_end)
        .context("opponent prompt write range is outside ROM")?
        .copy_from_slice(&compressed);
    let diff_confined_to_write_range = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(index, (before, after))| before == after || (STREAM_PC..write_end).contains(&index));
    if !diff_confined_to_write_range {
        bail!("opponent prompt patch changed bytes outside its compressed stream write range");
    }
    if patched.get(TILEMAP_TABLE_PC..table_end) != Some(EXPECTED_TILEMAP_TABLE.as_slice()) {
        bail!("opponent prompt tilemap table changed unexpectedly");
    }

    let (bank, address) = crate::rom::pc_to_lorom(STREAM_PC);
    let (table_bank, table_address) = crate::rom::pc_to_lorom(TILEMAP_TABLE_PC);
    let report = OpponentPromptKrPocReport {
        verdict: "normal-mode Japanese opponent prompt replaced by hand-drawn Korean masks with the measured edge/fill/shadow rule".to_owned(),
        source_path: source_path.clone(),
        ttf_path: ttf_path.clone(),
        ttf_sha256: format!("{:x}", Sha256::digest(ttf_data)),
        output_path: output_path.clone(),
        layout_source,
        text: PROMPT_KO.iter().map(|line| (*line).to_owned()).collect(),
        stream_pc: format!("0x{STREAM_PC:06X}"),
        stream_lorom: format!("${bank:02X}:${address:04X}"),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        decompressed_len: decoded.len(),
        runtime_vram_range: "0xA000-0xA7FF".to_owned(),
        runtime_vram_matches_original,
        tilemap_table_pc: format!("0x{TILEMAP_TABLE_PC:06X}"),
        tilemap_table_lorom: format!("${table_bank:02X}:${table_address:04X}"),
        tilemap_unchanged: true,
        line_widths_px: PROMPT_KO
            .iter()
            .map(|line| line.chars().count() * 16)
            .collect(),
        tile_ids: target_tiles
            .iter()
            .map(|tile| format!("${tile:02X}"))
            .collect(),
        palette: FONT_PALETTE,
        palette_bytes_hex: bytes_hex(&EXPECTED_PALETTE_BYTES),
        font_px: *font_px,
        changed_tiles: target_tiles.len(),
        changed_decompressed_bytes,
        compression_roundtrip_matches,
        diff_confined_to_write_range,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

fn replace_line(
    decoded: &mut [u8],
    line: &str,
    top_tiles: &[usize],
    bottom_tiles: &[usize],
) -> Result<()> {
    let characters: Vec<char> = line.chars().collect();
    if top_tiles.len() != characters.len() * 2 || bottom_tiles.len() != characters.len() * 2 {
        bail!("opponent prompt line {line:?} does not match its measured tile budget");
    }
    for (index, character) in characters.into_iter().enumerate() {
        let pixels = shade_glyph(glyph_mask(character)?);
        let tiles = encode_2bpp_quadrants(&pixels);
        replace_tile(decoded, top_tiles[index * 2], &tiles[0])?;
        replace_tile(decoded, top_tiles[index * 2 + 1], &tiles[1])?;
        replace_tile(decoded, bottom_tiles[index * 2], &tiles[2])?;
        replace_tile(decoded, bottom_tiles[index * 2 + 1], &tiles[3])?;
    }
    Ok(())
}

fn glyph_mask(character: char) -> Result<[[bool; 16]; 16]> {
    let (_, rows) = GLYPH_MASKS
        .iter()
        .find(|(glyph, _)| *glyph == character)
        .with_context(|| format!("no opponent prompt mask for {character:?}"))?;
    let mut mask = [[false; 16]; 16];
    for (y, row) in rows.iter().enumerate() {
        if row.len() != MASK_SIZE {
            bail!("opponent prompt mask row for {character:?} is not {MASK_SIZE} pixels");
        }
        for (x, cell) in row.bytes().enumerate() {
            mask[y][x] = match cell {
                b'#' => true,
                b'.' => false,
                other => bail!("invalid mask cell {:?} for {character:?}", other as char),
            };
        }
    }
    Ok(mask)
}

fn shade_glyph(mask: [[bool; 16]; 16]) -> [u8; 256] {
    let inside = |x: i32, y: i32| {
        (0..16).contains(&x) && (0..16).contains(&y) && mask[y as usize][x as usize]
    };
    let mut pixels = [BACKGROUND_INDEX; 256];
    for y in 0..16i32 {
        for x in 0..16i32 {
            pixels[y as usize * 16 + x as usize] = if inside(x, y) {
                if inside(x - 1, y) && inside(x, y - 1) {
                    FILL_INDEX
                } else {
                    EDGE_INDEX
                }
            } else if inside(x - 1, y) || inside(x, y - 1) || inside(x - 1, y - 1) {
                SHADOW_INDEX
            } else {
                BACKGROUND_INDEX
            };
        }
    }
    pixels
}

fn encode_2bpp_quadrants(pixels: &[u8; 256]) -> [[u8; TILE_LEN]; 4] {
    let mut tiles = [[0u8; TILE_LEN]; 4];
    for (tile_index, (x_offset, y_offset)) in [(0usize, 0usize), (8, 0), (0, 8), (8, 8)]
        .into_iter()
        .enumerate()
    {
        for row in 0..8 {
            for column in 0..8 {
                let palette_index = pixels[(y_offset + row) * 16 + x_offset + column];
                let bit = 1 << (7 - column);
                if palette_index & 1 != 0 {
                    tiles[tile_index][row * 2] |= bit;
                }
                if palette_index & 2 != 0 {
                    tiles[tile_index][row * 2 + 1] |= bit;
                }
            }
        }
    }
    tiles
}

fn replace_tile(decoded: &mut [u8], tile_index: usize, tile: &[u8; TILE_LEN]) -> Result<()> {
    let start = tile_index * TILE_LEN;
    decoded
        .get_mut(start..start + TILE_LEN)
        .with_context(|| {
            format!("opponent prompt tile ${tile_index:02X} is outside decoded block")
        })?
        .copy_from_slice(tile);
    Ok(())
}

fn target_tiles() -> Vec<usize> {
    (0x54..=0x73).collect()
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn every_prompt_character_has_a_mask_that_leaves_room_for_the_shadow() {
        for character in PROMPT_KO.iter().flat_map(|line| line.chars()) {
            let mask = glyph_mask(character).unwrap();
            assert!(mask.iter().flatten().any(|cell| *cell));
            assert!(mask[15].iter().all(|cell| !cell));
            assert!(mask.iter().all(|row| !row[15]));
        }
    }

    #[test]
    fn shading_lights_top_left_edges_and_drops_shadow_right_and_down() {
        let mut mask = [[false; 16]; 16];
        for row in mask.iter_mut().take(3) {
            row[..3].fill(true);
        }
        let pixels = shade_glyph(mask);
        assert_eq!(pixels[0], EDGE_INDEX);
        assert_eq!(pixels[16 + 1], FILL_INDEX);
        assert_eq!(pixels[3], SHADOW_INDEX);
        assert_eq!(pixels[3 * 16], SHADOW_INDEX);
        assert_eq!(pixels[3 * 16 + 3], SHADOW_INDEX);
        assert_eq!(pixels[4 * 16 + 4], BACKGROUND_INDEX);
    }

    #[test]
    fn measured_layout_has_exact_korean_pixel_widths() {
        assert_eq!(PROMPT_KO[0].chars().count() * 16, FIRST_LINE_TOP.len() * 8);
        assert_eq!(PROMPT_KO[1].chars().count() * 16, SECOND_LINE_TOP.len() * 8);
        assert_eq!(target_tiles(), (0x54..=0x73).collect::<Vec<_>>());
    }

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn current_remix_rom_matches_verified_prompt_assets() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let rom = crate::test_input::read(rom_path);
        let block = crate::snes_lz::decompress(&rom, STREAM_PC).unwrap();
        assert_eq!(block.compressed_len, ORIGINAL_COMPRESSED_LEN);
        assert_eq!(block.bytes.len(), DECOMPRESSED_LEN);
        assert_eq!(
            format!("{:x}", Sha256::digest(&block.bytes)),
            EXPECTED_DECODED_SHA256
        );
        assert_eq!(
            &rom[TILEMAP_TABLE_PC..TILEMAP_TABLE_PC + EXPECTED_TILEMAP_TABLE.len()],
            EXPECTED_TILEMAP_TABLE
        );
    }

    #[test]
    #[ignore = "requires the Remix JP ROM and assets/fonts/bmjua.ttf"]
    fn builds_bmjua_normal_mode_opponent_prompt() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let ttf_path = Path::new("assets/fonts/bmjua.ttf");
        let rom = crate::test_input::read(rom_path);
        let ttf = crate::test_input::read(ttf_path);
        let inputs = OpponentPromptKrPocInputs {
            rom: &rom,
            source_path: rom_path.display().to_string(),
            ttf_path: ttf_path.display().to_string(),
            ttf_data: &ttf,
            font_px: 15.0,
            runtime_vram: None,
            runtime_cram: None,
            output_path: "out/test.sfc".to_owned(),
        };
        let (patched, report) = build_opponent_prompt_kr_poc(&inputs).unwrap();
        assert_eq!(report.text, ["상대를", "골라주세요"]);
        assert_eq!(report.line_widths_px, [48, 80]);
        assert_eq!(report.changed_tiles, 32);
        assert!(report.patched_compressed_len <= report.original_compressed_len);
        assert!(report.compression_roundtrip_matches);
        assert!(report.diff_confined_to_write_range);
        assert_eq!(
            &patched[TILEMAP_TABLE_PC..TILEMAP_TABLE_PC + EXPECTED_TILEMAP_TABLE.len()],
            EXPECTED_TILEMAP_TABLE
        );
    }
}
