use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const STREAM_PC: usize = 0x03_8DF8;
const STREAM_CAPACITY: usize = 5_948;
const DECOMPRESSED_LEN: usize = 8_192;
const VRAM_BYTE: usize = 0xE000;
const TILE_LEN: usize = 32;
const OBJ_PALETTE: usize = 1;
const GLYPHS: [(char, usize); 2] = [('상', 0xC8), ('쇄', 0xCA)];
const BACKGROUND_INDEX: u8 = 0;
// The original 相殺 is dark ink (index 1) on a rounded white plate (index 15)
// spanning both sprites; the Korean pair keeps that plate.
const INK_INDEX: u8 = 1;
const PLATE_INDEX: u8 = 15;
const PAIR_WIDTH: usize = 32;
const PLATE_TOP: usize = 2;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub source_path: String,
    pub output_path: String,
    pub evidence_source: String,
    pub glyph_source: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub stream_capacity: usize,
    pub decompressed_len: usize,
    pub bg_mode: u64,
    pub runtime_tiles_match_source: bool,
    pub runtime_oam_entries: Vec<usize>,
    pub glyph_slots: Vec<String>,
    pub palette_indices: Vec<u8>,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_expected_range: bool,
    pub source_preview: String,
    pub korean_preview: String,
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
    // Remix keeps this OBJ stream at the Tsuu location, but does not use the
    // five-label Tokoton relocation from the sibling patch.  The preceding
    // rensa stage may have recompressed the same stream in process, so accept
    // any valid block that still fits the verified original extent.
    let source_stream_pc = STREAM_PC;
    let stream_capacity = STREAM_CAPACITY;
    let block = crate::snes_lz::decompress(rom, source_stream_pc)?;
    if block.compressed_len > stream_capacity || block.bytes.len() != DECOMPRESSED_LEN {
        bail!(
            "sousai OBJ stream differs from the verified slot: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let target_tiles = GLYPHS
        .iter()
        .flat_map(|(_, base)| [*base, *base + 1, *base + 16, *base + 17])
        .collect::<BTreeSet<_>>();
    let evidence_source;
    let (bg_mode, runtime_tiles_match_source, runtime_oam_entries, palette) =
        if let (Some(runtime_dump), Some(preview_dir)) = (runtime_dump, preview_dir) {
            evidence_source = "runtime evidence".to_owned();
            let vram = fs::read(runtime_dump.join("vram.bin"))?;
            let cram = fs::read(runtime_dump.join("cram.bin"))?;
            let oam = fs::read(runtime_dump.join("oam.bin"))?;
            let state: serde_json::Value =
                serde_json::from_slice(&fs::read(runtime_dump.join("state.json"))?)?;
            let bg_mode = state
                .get("ppu.bgMode")
                .and_then(serde_json::Value::as_u64)
                .context("sousai runtime state has no ppu.bgMode")?;
            if bg_mode != 2 {
                bail!("sousai gameplay screen expected BG Mode 2, got {bg_mode}");
            }
            let runtime_obj = vram
                .get(VRAM_BYTE..VRAM_BYTE + DECOMPRESSED_LEN)
                .context("sousai OBJ range is outside runtime VRAM")?;
            let matches = target_tiles.iter().all(|tile| {
                let start = tile * TILE_LEN;
                block.bytes.get(start..start + TILE_LEN) == runtime_obj.get(start..start + TILE_LEN)
            });
            if !matches {
                bail!("sousai tiles differ between ROM source and runtime OBJ VRAM");
            }
            let entries = find_runtime_pair(&oam)?;
            if entries.is_empty() {
                bail!("runtime OAM has no visible $C8/$CA sousai pair");
            }
            fs::create_dir_all(preview_dir)?;
            (
                bg_mode,
                true,
                entries,
                Some(obj_palette(&cram, OBJ_PALETTE)?),
            )
        } else if runtime_dump.is_none() && preview_dir.is_none() {
            evidence_source = "hardcoded verified ROM stream/palette Spec".to_owned();
            (2, true, Vec::new(), None)
        } else {
            bail!("sousai runtime dump and preview directory must both be present or omitted");
        };
    let source_preview = preview_dir.map(|dir| dir.join("source_相殺.png"));
    if let (Some(path), Some(palette)) = (&source_preview, &palette) {
        fs::write(path, encode_png(&render_pair(&block.bytes, palette)?)?)?;
    }

    let pair = render_plate_pair()?;
    let mut decoded = block.bytes.clone();
    for (sprite, (_, base)) in GLYPHS.into_iter().enumerate() {
        let tiles = [(0, 0), (8, 0), (0, 8), (8, 8)]
            .map(|(x, y)| crate::effect_glyphs::encode_4bpp(&pair, PAIR_WIDTH, sprite * 16 + x, y));
        replace_large_sprite(&mut decoded, base, &tiles)?;
    }
    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    for (index, (before, after)) in block.bytes.iter().zip(&decoded).enumerate() {
        if before != after && !target_tiles.contains(&(index / TILE_LEN)) {
            bail!("unexplained sousai decoded change at 0x{index:04X}");
        }
    }
    let compressed = crate::snes_lz::compress(&decoded);
    if compressed.len() > stream_capacity {
        bail!(
            "Korean sousai stream grew to {} bytes, beyond the {}-byte in-place extent",
            compressed.len(),
            stream_capacity
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == decoded;
    if !compression_roundtrip_matches {
        bail!("Korean sousai compression round-trip failed");
    }
    let mut patched = rom.to_vec();
    patched
        .get_mut(source_stream_pc..source_stream_pc + compressed.len())
        .context("sousai compressed write is outside ROM")?
        .copy_from_slice(&compressed);
    let write_end = source_stream_pc + compressed.len();
    let diff_confined_to_expected_range =
        rom.iter()
            .zip(&patched)
            .enumerate()
            .all(|(index, (before, after))| {
                before == after || (source_stream_pc..write_end).contains(&index)
            });
    if !diff_confined_to_expected_range {
        bail!("sousai patch changed bytes outside the OBJ stream");
    }
    let korean_preview = preview_dir.map(|dir| dir.join("korean_상쇄.png"));
    if let (Some(path), Some(palette)) = (&korean_preview, &palette) {
        let korean_png = encode_png(&render_pair(&decoded, palette)?)?;
        fs::write(path, korean_png)?;
    }

    let (bank, address) = crate::rom::pc_to_lorom(source_stream_pc);
    let report = BuildReport {
        verdict: "gameplay OBJ 16x16 相殺 patched to Korean 상쇄".to_owned(),
        source_path,
        output_path,
        evidence_source,
        glyph_source: "src/effect_glyphs.rs 16px masks on the original white plate".to_owned(),
        stream_pc: format!("0x{source_stream_pc:06X}"),
        stream_lorom: format!("${bank:02X}:${address:04X}"),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        stream_capacity,
        decompressed_len: decoded.len(),
        bg_mode,
        runtime_tiles_match_source,
        runtime_oam_entries,
        glyph_slots: GLYPHS
            .iter()
            .map(|(character, base)| format!("${base:02X}={character}"))
            .collect(),
        palette_indices: vec![BACKGROUND_INDEX, INK_INDEX, PLATE_INDEX],
        changed_tiles: target_tiles.len(),
        changed_decompressed_bytes,
        compression_roundtrip_matches,
        diff_confined_to_expected_range,
        source_preview: source_preview
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        korean_preview: korean_preview
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

fn find_runtime_pair(oam: &[u8]) -> Result<Vec<usize>> {
    if oam.len() != 544 {
        bail!("sousai runtime OAM is {} bytes, expected 544", oam.len());
    }
    let mut entries = Vec::new();
    for index in 0..127usize {
        let left = index * 4;
        let right = left + 4;
        if oam[left + 1] < 224
            && oam[left + 2] == 0xC8
            && oam[left + 3] == 0x33
            && oam[right + 1] == oam[left + 1]
            && oam[right + 2] == 0xCA
            && oam[right + 3] == 0x33
        {
            entries.extend([index, index + 1]);
        }
    }
    Ok(entries)
}

fn obj_palette(cram: &[u8], id: usize) -> Result<[[u8; 3]; 16]> {
    let mut out = [[0; 3]; 16];
    for (index, rgb) in out.iter_mut().enumerate() {
        let offset = (128 + id * 16 + index) * 2;
        let bytes = cram
            .get(offset..offset + 2)
            .context("sousai OBJ palette outside CGRAM")?;
        let color = u16::from_le_bytes([bytes[0], bytes[1]]);
        let scale = |value: u16| (value * 255 / 31) as u8;
        *rgb = [
            scale(color & 31),
            scale((color >> 5) & 31),
            scale((color >> 10) & 31),
        ];
    }
    Ok(out)
}

/// Renders the 32x16 `상쇄` pair: a white plate over rows 2-15 with its four
/// corners cut, and each syllable centred in its 16px sprite as dark ink.
fn render_plate_pair() -> Result<Vec<u8>> {
    let mut pixels = vec![BACKGROUND_INDEX; PAIR_WIDTH * 16];
    for y in PLATE_TOP..16 {
        for x in 0..PAIR_WIDTH {
            let corner = (y == PLATE_TOP || y == 15) && (x == 0 || x == PAIR_WIDTH - 1);
            if !corner {
                pixels[y * PAIR_WIDTH + x] = PLATE_INDEX;
            }
        }
    }
    for (sprite, (character, _)) in GLYPHS.into_iter().enumerate() {
        let mask = crate::effect_glyphs::glyph(character)?;
        let x0 = sprite * 16 + (16 - crate::effect_glyphs::width(mask)) / 2;
        let y0 = PLATE_TOP + 1 + (16 - PLATE_TOP - 1 - mask.len()) / 2;
        let mut ink = vec![false; PAIR_WIDTH * 16];
        crate::effect_glyphs::stamp(&mut ink, PAIR_WIDTH, 16, mask, x0, y0, 1)?;
        for (pixel, inked) in pixels.iter_mut().zip(ink) {
            if inked {
                *pixel = INK_INDEX;
            }
        }
    }
    Ok(pixels)
}

fn replace_large_sprite(
    decoded: &mut [u8],
    base: usize,
    tiles: &[[u8; TILE_LEN]; 4],
) -> Result<()> {
    for (tile, replacement) in [base, base + 1, base + 16, base + 17]
        .into_iter()
        .zip(tiles)
    {
        let start = tile * TILE_LEN;
        decoded
            .get_mut(start..start + TILE_LEN)
            .with_context(|| format!("sousai glyph tile ${tile:02X} is outside stream"))?
            .copy_from_slice(replacement);
    }
    Ok(())
}

fn render_pair(decoded: &[u8], palette: &[[u8; 3]; 16]) -> Result<Vec<u8>> {
    let mut rgba = vec![0; 32 * 16 * 4];
    for (sprite_x, (_, base)) in GLYPHS.into_iter().enumerate() {
        for (qx, qy, tile_id) in [
            (0usize, 0usize, base),
            (8, 0, base + 1),
            (0, 8, base + 16),
            (8, 8, base + 17),
        ] {
            let start = tile_id * TILE_LEN;
            let tile = decoded
                .get(start..start + TILE_LEN)
                .with_context(|| format!("sousai OBJ tile ${tile_id:02X} is outside stream"))?;
            for y in 0..8 {
                for x in 0..8 {
                    let value = pixel(tile, x, y);
                    let dst = ((qy + y) * 32 + sprite_x * 16 + qx + x) * 4;
                    rgba[dst..dst + 3].copy_from_slice(&palette[usize::from(value)]);
                    rgba[dst + 3] = if value == 0 { 0 } else { 255 };
                }
            }
        }
    }
    Ok(rgba)
}

fn pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn encode_png(rgba: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, 32, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(output)
}
