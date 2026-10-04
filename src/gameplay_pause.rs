use std::{collections::BTreeSet, fs, io::Cursor, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const STREAM_PC: usize = 0x03_C054;
const STREAM_CAPACITY: usize = 6_002;
const DECOMPRESSED_LEN: usize = 8_192;
const EXPECTED_DECODED_SHA256: &str =
    "90776dccc3793e849eba617b0325488288abf64f6925faff98d8d468f2f34211";
const RUNTIME_VRAM_BYTE: usize = 0x8000;
const TILE_LEN: usize = 32;
const WIDTH: usize = 48;
const HEIGHT: usize = 16;
const TEXT: &str = "휴식중";
const TOP_TILES: [usize; 6] = [0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F];
const BOTTOM_TILES: [usize; 6] = [0x7A, 0x7B, 0x7C, 0x7D, 0x7E, 0x7F];
const LEFT_PALETTE: usize = 1;
const RIGHT_PALETTE: usize = 2;
const LEFT_PALETTE_SHA256: &str =
    "09148e3a5be0b301acbd11cd39cb501252cc070f027586637b5e3ca7b9ba8e55";
const RIGHT_PALETTE_SHA256: &str =
    "e763ab3e5876463b00bd9fa2f356571bd3d5d700aa6635693eb0292272a0856c";
const TRANSPARENT_INDEX: u8 = 0;
const SHADOW_INDEX: u8 = 1;
const OUTLINE_INDEX: u8 = 2;
const DARK_BODY_INDEX: u8 = 4;
const BODY_INDEX: u8 = 5;
const LIGHT_BODY_INDEX: u8 = 6;
const SPECULAR_INDEX: u8 = 15;
// Left-edge x of each 16px syllable cell.
const CELL_X: [usize; 3] = [2, 18, 34];
// The BG1 tilemap draws columns 0-23 with palette 1 (red) and 24-47 with
// palette 2 (blue), which splits the middle syllable. That syllable is drawn
// in the near-white index 15 with a black outline, which both palettes share.
const NEUTRAL_CELL: std::ops::Range<usize> = 16..32;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub source_path: String,
    pub output_path: String,
    pub evidence_source: String,
    pub text: String,
    pub glyph_source: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub stream_capacity: usize,
    pub decompressed_len: usize,
    pub decoded_sha256: String,
    pub runtime_vram_range: String,
    pub runtime_vram_matches_source: bool,
    pub bg_mode: u64,
    pub tilemap_reuses_graphic_with_two_palettes: bool,
    pub left_palette: usize,
    pub right_palette: usize,
    pub tile_ids: Vec<String>,
    pub palette_indices: Vec<u8>,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_expected_range: bool,
    pub source_previews: Vec<String>,
    pub korean_previews: Vec<String>,
    pub output_sha256: String,
}

#[allow(clippy::too_many_arguments)]
pub fn build_kr_poc(
    rom: &[u8],
    source_path: String,
    runtime_dump: Option<&Path>,
    preview_dir: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, BuildReport)> {
    let block = crate::snes_lz::decompress(rom, STREAM_PC)?;
    if block.compressed_len != STREAM_CAPACITY || block.bytes.len() != DECOMPRESSED_LEN {
        bail!(
            "pause BG1 stream differs from the verified ROM Spec: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let decoded_sha256 = format!("{:x}", Sha256::digest(&block.bytes));
    if decoded_sha256 != EXPECTED_DECODED_SHA256 {
        bail!("pause BG1 decoded SHA-256 differs from the verified ROM Spec: {decoded_sha256}");
    }

    let target_tiles = target_tiles();
    let evidence_source;
    let mut source_previews = Vec::new();
    let mut korean_previews = Vec::new();
    let (runtime_vram_matches_source, bg_mode, palettes) =
        if let (Some(runtime_dump), Some(preview_dir)) = (runtime_dump, preview_dir) {
            evidence_source = "runtime BG1 evidence".to_owned();
            let vram = fs::read(runtime_dump.join("vram.bin"))?;
            let cram = fs::read(runtime_dump.join("cram.bin"))?;
            let state: serde_json::Value =
                serde_json::from_slice(&fs::read(runtime_dump.join("state.json"))?)?;
            let bg_mode = state
                .get("ppu.bgMode")
                .and_then(serde_json::Value::as_u64)
                .context("pause runtime state has no ppu.bgMode")?;
            if bg_mode != 2 {
                bail!("pause gameplay screen expected BG Mode 2, got {bg_mode}");
            }
            let runtime_end = RUNTIME_VRAM_BYTE + DECOMPRESSED_LEN;
            if vram.get(RUNTIME_VRAM_BYTE..runtime_end) != Some(block.bytes.as_slice()) {
                bail!("pause source does not match runtime BG1 VRAM $8000-$9FFF");
            }
            verify_runtime_tilemap(&vram)?;
            let left = bg_palette(&cram, LEFT_PALETTE)?;
            let right = bg_palette(&cram, RIGHT_PALETTE)?;
            fs::create_dir_all(preview_dir)?;
            for (name, palette) in [("1p_red", left), ("2p_blue", right)] {
                let source_path = preview_dir.join(format!("source_きゅうけい_{name}.png"));
                fs::write(
                    &source_path,
                    encode_png_rgba(&render_tiles(&block.bytes, &palette)?)?,
                )?;
                source_previews.push(source_path.display().to_string());
            }
            (true, bg_mode, Some([left, right]))
        } else if runtime_dump.is_none() && preview_dir.is_none() {
            evidence_source = "hardcoded verified ROM/BG1 Spec".to_owned();
            (true, 2, None)
        } else {
            bail!("pause runtime dump and preview directory must both be present or omitted");
        };

    let indices = render_text_indices()?;
    let mut decoded = block.bytes.clone();
    replace_graphic(&mut decoded, &indices)?;
    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    for (index, (before, after)) in block.bytes.iter().zip(&decoded).enumerate() {
        if before != after && !target_tiles.contains(&(index / TILE_LEN)) {
            bail!("unexplained pause decoded change at 0x{index:04X}");
        }
    }
    if let (Some(preview_dir), Some(palettes)) = (preview_dir, palettes) {
        for (name, palette) in [("1p_red", palettes[0]), ("2p_blue", palettes[1])] {
            let path = preview_dir.join(format!("korean_휴식중_{name}.png"));
            fs::write(&path, encode_png_rgba(&render_tiles(&decoded, &palette)?)?)?;
            korean_previews.push(path.display().to_string());
        }
    }

    let compressed = crate::snes_lz::compress(&decoded);
    if compressed.len() > STREAM_CAPACITY {
        bail!(
            "Korean pause stream grew to {} bytes, beyond the {}-byte in-place extent",
            compressed.len(),
            STREAM_CAPACITY
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == decoded;
    if !compression_roundtrip_matches {
        bail!("Korean pause compression round-trip failed");
    }

    let mut patched = rom.to_vec();
    let write_end = STREAM_PC + compressed.len();
    patched
        .get_mut(STREAM_PC..write_end)
        .context("pause compressed write is outside ROM")?
        .copy_from_slice(&compressed);
    let diff_confined_to_expected_range = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(index, (before, after))| before == after || (STREAM_PC..write_end).contains(&index));
    if !diff_confined_to_expected_range {
        bail!("pause patch changed bytes outside the BG1 stream");
    }

    let report = BuildReport {
        verdict: "gameplay BG1 pause graphic patched to Korean 휴식중".to_owned(),
        source_path,
        output_path,
        evidence_source,
        text: TEXT.to_owned(),
        glyph_source: "src/effect_glyphs.rs 16px masks with the original shading roles".to_owned(),
        stream_pc: format!("0x{STREAM_PC:06X}"),
        stream_lorom: crate::rom::format_lorom_addr(STREAM_PC),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        stream_capacity: STREAM_CAPACITY,
        decompressed_len: decoded.len(),
        decoded_sha256,
        runtime_vram_range: "0x8000-0x9FFF".to_owned(),
        runtime_vram_matches_source,
        bg_mode,
        tilemap_reuses_graphic_with_two_palettes: true,
        left_palette: LEFT_PALETTE,
        right_palette: RIGHT_PALETTE,
        tile_ids: target_tiles
            .iter()
            .map(|tile| format!("${tile:02X}"))
            .collect(),
        palette_indices: vec![
            TRANSPARENT_INDEX,
            SHADOW_INDEX,
            OUTLINE_INDEX,
            DARK_BODY_INDEX,
            BODY_INDEX,
            LIGHT_BODY_INDEX,
            SPECULAR_INDEX,
        ],
        changed_tiles: target_tiles.len(),
        changed_decompressed_bytes,
        compression_roundtrip_matches,
        diff_confined_to_expected_range,
        source_previews,
        korean_previews,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

fn target_tiles() -> BTreeSet<usize> {
    TOP_TILES
        .into_iter()
        .chain(BOTTOM_TILES)
        .collect::<BTreeSet<_>>()
}

fn verify_runtime_tilemap(vram: &[u8]) -> Result<()> {
    for (x_start, palette) in [(4usize, LEFT_PALETTE), (22, RIGHT_PALETTE)] {
        for row in 0..2usize {
            for column in 0..6usize {
                let offset = 0xA000 + ((11 + row) * 32 + x_start + column) * 2;
                let raw = u16::from_le_bytes(
                    vram.get(offset..offset + 2)
                        .context("pause BG1 tilemap entry outside VRAM")?
                        .try_into()
                        .expect("two-byte tilemap entry"),
                );
                let expected_tile = if row == 0 {
                    0x36A + column
                } else {
                    0x37A + column
                };
                if usize::from(raw & 0x03FF) != expected_tile
                    || usize::from((raw >> 10) & 7) != palette
                    || raw & 0x2000 == 0
                    || raw & 0xC000 != 0
                {
                    bail!(
                        "pause BG1 tilemap differs at ({}, {}): 0x{raw:04X}",
                        x_start + column,
                        11 + row
                    );
                }
            }
        }
    }
    Ok(())
}

fn bg_palette(cram: &[u8], id: usize) -> Result<[[u8; 3]; 16]> {
    let start = id * 32;
    let bytes = cram
        .get(start..start + 32)
        .with_context(|| format!("pause BG palette {id} outside CGRAM"))?;
    let expected_sha = if id == LEFT_PALETTE {
        LEFT_PALETTE_SHA256
    } else if id == RIGHT_PALETTE {
        RIGHT_PALETTE_SHA256
    } else {
        bail!("unsupported pause palette {id}");
    };
    let actual_sha = format!("{:x}", Sha256::digest(bytes));
    if actual_sha != expected_sha {
        bail!("pause BG palette {id} differs from the verified runtime colors: {actual_sha}");
    }
    let mut palette = [[0; 3]; 16];
    for (index, rgb) in palette.iter_mut().enumerate() {
        let color = u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]);
        let scale = |value: u16| (value * 255 / 31) as u8;
        *rgb = [
            scale(color & 31),
            scale((color >> 5) & 31),
            scale((color >> 10) & 31),
        ];
    }
    Ok(palette)
}

fn render_text_indices() -> Result<[u8; WIDTH * HEIGHT]> {
    let mut body = vec![false; WIDTH * HEIGHT];
    for (character, x0) in TEXT.chars().zip(CELL_X) {
        let mask = crate::effect_glyphs::glyph(character)?;
        let y0 = (HEIGHT - 2 - mask.len()) / 2;
        crate::effect_glyphs::stamp(&mut body, WIDTH, HEIGHT, mask, x0, y0, 1)?;
    }
    let outside = crate::effect_glyphs::exterior(&body, WIDTH, HEIGHT);
    let body_at = |x: i32, y: i32| {
        (0..WIDTH as i32).contains(&x)
            && (0..HEIGHT as i32).contains(&y)
            && body[y as usize * WIDTH + x as usize]
    };
    let mut pixels = [TRANSPARENT_INDEX; WIDTH * HEIGHT];
    for y in 0..HEIGHT as i32 {
        for x in 0..WIDTH as i32 {
            let index = y as usize * WIDTH + x as usize;
            let neutral = NEUTRAL_CELL.contains(&(x as usize));
            pixels[index] = if body_at(x, y) {
                if neutral || (!body_at(x - 1, y) && !body_at(x, y - 1)) {
                    SPECULAR_INDEX
                } else if !body_at(x, y - 1) {
                    LIGHT_BODY_INDEX
                } else if !body_at(x, y + 1) {
                    DARK_BODY_INDEX
                } else {
                    BODY_INDEX
                }
            } else if !outside[index] {
                TRANSPARENT_INDEX
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| body_at(x + dx, y + dy))) {
                if neutral { SHADOW_INDEX } else { OUTLINE_INDEX }
            } else if body_at(x - 1, y - 1) || body_at(x - 2, y - 2) {
                SHADOW_INDEX
            } else {
                TRANSPARENT_INDEX
            };
        }
    }
    Ok(pixels)
}

fn replace_graphic(decoded: &mut [u8], pixels: &[u8; WIDTH * HEIGHT]) -> Result<()> {
    for row in 0..2usize {
        for column in 0..6usize {
            let tile_id = if row == 0 {
                TOP_TILES[column]
            } else {
                BOTTOM_TILES[column]
            };
            let encoded = encode_tile(pixels, column * 8, row * 8);
            let start = tile_id * TILE_LEN;
            decoded
                .get_mut(start..start + TILE_LEN)
                .with_context(|| format!("pause tile ${tile_id:02X} outside decoded stream"))?
                .copy_from_slice(&encoded);
        }
    }
    Ok(())
}

fn encode_tile(pixels: &[u8; WIDTH * HEIGHT], x_offset: usize, y_offset: usize) -> [u8; 32] {
    let mut tile = [0u8; 32];
    for y in 0..8usize {
        for x in 0..8usize {
            let value = pixels[(y_offset + y) * WIDTH + x_offset + x];
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

fn render_tiles(decoded: &[u8], palette: &[[u8; 3]; 16]) -> Result<Vec<u8>> {
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];
    for row in 0..2usize {
        for column in 0..6usize {
            let tile_id = if row == 0 {
                TOP_TILES[column]
            } else {
                BOTTOM_TILES[column]
            };
            let start = tile_id * TILE_LEN;
            let tile = decoded
                .get(start..start + TILE_LEN)
                .with_context(|| format!("pause tile ${tile_id:02X} outside decoded stream"))?;
            for y in 0..8usize {
                for x in 0..8usize {
                    let value = decode_pixel(tile, x, y);
                    let dst = (((row * 8 + y) * WIDTH) + column * 8 + x) * 4;
                    rgba[dst..dst + 3].copy_from_slice(&palette[usize::from(value)]);
                    rgba[dst + 3] = if value == TRANSPARENT_INDEX { 0 } else { 255 };
                }
            }
        }
    }
    Ok(rgba)
}

fn decode_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn encode_png_rgba(rgba: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut output), WIDTH as u32, HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn builds_korean_pause_graphic() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let rom = crate::test_input::read(rom_path);
        let (patched, report) = build_kr_poc(
            &rom,
            rom_path.display().to_string(),
            None,
            None,
            "test.sfc".to_owned(),
        )
        .unwrap();
        assert_eq!(patched.len(), rom.len());
        assert_eq!(report.text, "휴식중");
        assert_eq!(report.changed_tiles, 12);
        assert!(report.patched_compressed_len <= STREAM_CAPACITY);
        assert!(report.compression_roundtrip_matches);
        assert!(report.diff_confined_to_expected_range);
    }
}
