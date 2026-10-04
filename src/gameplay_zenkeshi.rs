use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TILE_LEN: usize = 32;
const STREAM_PC: usize = 0x03_8078;
const NEXT_STREAM_PC: usize = 0x03_8DF8;
const DECOMPRESSED_LEN: usize = 6_144;
const VERIFIED_VRAM_BASE: usize = 0xC000;
const OAM_FIRST: usize = 34;
const OAM_COUNT: usize = 10;
const OBJ_PALETTE: usize = 1;
const CANVAS_X: i32 = 0x10;
const CANVAS_Y: i32 = 0x21;
const CANVAS_WIDTH: usize = 80;
const CANVAS_HEIGHT: usize = 40;
const CANDIDATE_BASES: [usize; 4] = [0x8000, 0xA000, 0xC000, 0xE000];
const FRAME_PC: usize = 0x00_B536;
const FRAME_LEN: usize = 2 + OAM_COUNT * 8;
const SPRITE_ROWS_HEIGHT: usize = 32;
const ZENKESHI_TEXT: &str = "싹쓸이!";
// Index roles measured from the original 全消し!: 8 core, 9 rim, 12 outline.
// The original also rims the right and bottom edges with 10/11; lighting only
// the top and left edges keeps the recompressed stream inside its slot.
const CORE_INDEX: u8 = 8;
const RIM_INDEX: u8 = 9;
const OUTLINE_INDEX: u8 = 12;
const KOREAN_BASES: [u8; OAM_COUNT] = [0x98, 0x9A, 0x9C, 0x9E, 0xA0, 0xA2, 0xA4, 0xA6, 0x60, 0x62];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuditReport {
    pub verdict: String,
    pub source_path: String,
    pub runtime_dump: String,
    pub bg_mode: u64,
    pub text: String,
    pub korean_target: String,
    pub oam_entries: Vec<usize>,
    pub obj_palette: usize,
    pub canvas: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub compressed_len: usize,
    pub stream_capacity: usize,
    pub decompressed_len: usize,
    pub verified_vram_base: String,
    pub runtime_tiles_match_source: bool,
    pub blank_unreferenced_groups: Vec<String>,
    pub candidates: Vec<CandidateReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CandidateReport {
    pub vram_base: String,
    pub palette_indices: Vec<u8>,
    pub output: String,
    pub output_sha256: String,
}

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
    pub stream_headroom: usize,
    pub decompressed_len: usize,
    pub runtime_tiles_match_source: bool,
    pub frame_pc: String,
    pub frame_lorom: String,
    pub frame_sprites: usize,
    pub tile_groups: Vec<String>,
    pub palette_indices: Vec<u8>,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub changed_frame_bytes: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_expected_ranges: bool,
    pub preview: String,
    pub output_sha256: String,
}

pub fn audit(
    rom: &[u8],
    source_path: String,
    runtime_dump: &Path,
    out_dir: &Path,
) -> Result<AuditReport> {
    let block = crate::snes_lz::decompress(rom, STREAM_PC)?;
    let stream_capacity = NEXT_STREAM_PC - STREAM_PC;
    if block.compressed_len > stream_capacity || block.bytes.len() != DECOMPRESSED_LEN {
        bail!(
            "zenkeshi OBJ stream differs from the measured slot: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let vram = fs::read(runtime_dump.join("vram.bin"))?;
    let cram = fs::read(runtime_dump.join("cram.bin"))?;
    let oam = fs::read(runtime_dump.join("oam.bin"))?;
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(runtime_dump.join("state.json"))?)?;
    let bg_mode = state
        .get("ppu.bgMode")
        .and_then(serde_json::Value::as_u64)
        .context("zenkeshi runtime state has no ppu.bgMode")?;
    if bg_mode != 2 {
        bail!("zenkeshi screen expected BG Mode 2, got {bg_mode}");
    }
    verify_oam(&oam)?;
    let target_tiles = target_tiles(&oam);
    let runtime_tiles_match_source = target_tiles.iter().all(|tile| {
        let decoded_start = tile * TILE_LEN;
        let runtime_start = VERIFIED_VRAM_BASE + decoded_start;
        block.bytes.get(decoded_start..decoded_start + TILE_LEN)
            == vram.get(runtime_start..runtime_start + TILE_LEN)
    });
    if !runtime_tiles_match_source {
        bail!("zenkeshi tiles differ between ROM source and runtime VRAM");
    }
    let visible_tiles = visible_name_table_zero_tiles(&oam)?;
    let blank_unreferenced_groups = (0usize..=0xAE)
        .step_by(2)
        .filter(|base| {
            [*base, *base + 1, *base + 16, *base + 17]
                .into_iter()
                .all(|tile| {
                    let start = tile * TILE_LEN;
                    block.bytes.get(start..start + TILE_LEN) == Some(&[0u8; TILE_LEN])
                        && !visible_tiles.contains(&tile)
                })
        })
        .map(|base| format!("${base:02X}"))
        .collect::<Vec<_>>();
    let palette = obj_palette(&cram, OBJ_PALETTE)?;
    fs::create_dir_all(out_dir)?;
    let mut candidates = Vec::new();
    for base in CANDIDATE_BASES {
        let (rgba, indices) = render_oam_candidate(&vram, &oam, &palette, base)?;
        let png = encode_png(&rgba)?;
        let output = out_dir.join(format!("obj_base_{base:04X}.png"));
        fs::write(&output, &png)?;
        candidates.push(CandidateReport {
            vram_base: format!("0x{base:04X}"),
            palette_indices: indices.into_iter().collect(),
            output: output.display().to_string(),
            output_sha256: format!("{:x}", Sha256::digest(&png)),
        });
    }
    Ok(AuditReport {
        verdict: "全消し! is a ten-large-OBJ 80x40 graphic".to_owned(),
        source_path,
        runtime_dump: runtime_dump.display().to_string(),
        bg_mode,
        text: "全消し!".to_owned(),
        korean_target: "싹쓸이!".to_owned(),
        oam_entries: (OAM_FIRST..OAM_FIRST + OAM_COUNT).collect(),
        obj_palette: OBJ_PALETTE,
        canvas: "80x40 at screen x=16,y=33".to_owned(),
        stream_pc: format!("0x{STREAM_PC:06X}"),
        stream_lorom: crate::rom::format_lorom_addr(STREAM_PC),
        compressed_len: block.compressed_len,
        stream_capacity,
        decompressed_len: block.bytes.len(),
        verified_vram_base: format!("0x{VERIFIED_VRAM_BASE:04X}"),
        runtime_tiles_match_source,
        blank_unreferenced_groups,
        candidates,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_kr_poc(
    rom: &[u8],
    source_path: String,
    runtime_dump: Option<&Path>,
    preview_path: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, BuildReport)> {
    let evidence_source;
    let runtime_tiles_match_source =
        if let (Some(runtime_dump), Some(preview_path)) = (runtime_dump, preview_path) {
            evidence_source = "runtime evidence".to_owned();
            let baseline_dir = preview_path
                .parent()
                .unwrap_or_else(|| Path::new("out/work/gameplay/zenkeshi"));
            let baseline = audit(rom, source_path.clone(), runtime_dump, baseline_dir)?;
            if !baseline.runtime_tiles_match_source {
                bail!("zenkeshi runtime tiles do not match the ROM source");
            }
            true
        } else if runtime_dump.is_none() && preview_path.is_none() {
            evidence_source = "hardcoded verified ROM stream/OAM Spec".to_owned();
            true
        } else {
            bail!("zenkeshi runtime dump and preview path must both be present or omitted");
        };
    let expected_frame = original_frame_bytes();
    if rom.get(FRAME_PC..FRAME_PC + FRAME_LEN) != Some(expected_frame.as_slice()) {
        bail!("zenkeshi OAM frame differs from the measured ROM Spec");
    }
    let preview_pixels = render_canvas()?;
    if let (Some(runtime_dump), Some(preview_path)) = (runtime_dump, preview_path) {
        let cram = fs::read(runtime_dump.join("cram.bin"))?;
        let palette = obj_palette(&cram, OBJ_PALETTE)?;
        // The preview keeps the 80x40 audit canvas; the sprite grid sits 4 rows down.
        let mut framed = vec![0u8; CANVAS_WIDTH * CANVAS_HEIGHT];
        framed[4 * CANVAS_WIDTH..(4 + SPRITE_ROWS_HEIGHT) * CANVAS_WIDTH]
            .copy_from_slice(&preview_pixels);
        let preview_rgba = palette_pixels_to_rgba(&framed, &palette);
        let preview_png = encode_png(&preview_rgba)?;
        if let Some(parent) = preview_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(preview_path, &preview_png)?;
    }

    let block = crate::snes_lz::decompress(rom, STREAM_PC)?;
    let mut decoded = block.bytes.clone();
    let target_tiles = encode_regular_grid(&mut decoded, &preview_pixels)?;
    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    for (index, (before, after)) in block.bytes.iter().zip(&decoded).enumerate() {
        if before != after && !target_tiles.contains(&(index / TILE_LEN)) {
            bail!("unexplained zenkeshi decoded change at 0x{index:04X}");
        }
    }
    let compressed = crate::snes_lz::compress(&decoded);
    let stream_capacity = NEXT_STREAM_PC - STREAM_PC;
    if compressed.len() > stream_capacity {
        bail!(
            "Korean zenkeshi stream grew to {} bytes, beyond in-place capacity {}; global decompressor relocation is unsafe",
            compressed.len(),
            stream_capacity
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == decoded;
    if !compression_roundtrip_matches {
        bail!("Korean zenkeshi compression round-trip failed");
    }
    let korean_frame = korean_frame_bytes();
    let mut patched = rom.to_vec();
    patched
        .get_mut(STREAM_PC..STREAM_PC + compressed.len())
        .context("zenkeshi compressed write is outside ROM")?
        .copy_from_slice(&compressed);
    patched
        .get_mut(FRAME_PC..FRAME_PC + FRAME_LEN)
        .context("zenkeshi frame write is outside ROM")?
        .copy_from_slice(&korean_frame);
    let changed_frame_bytes = expected_frame
        .iter()
        .zip(korean_frame)
        .filter(|(before, after)| **before != *after)
        .count();
    let write_range = STREAM_PC..STREAM_PC + compressed.len();
    let diff_confined_to_expected_ranges =
        rom.iter()
            .zip(&patched)
            .enumerate()
            .all(|(index, (before, after))| {
                before == after
                    || write_range.contains(&index)
                    || (FRAME_PC..FRAME_PC + FRAME_LEN).contains(&index)
            });
    if !diff_confined_to_expected_ranges {
        bail!("zenkeshi patch changed bytes outside its expected write ranges");
    }
    let (frame_bank, frame_address) = crate::rom::pc_to_lorom(FRAME_PC);
    let report = BuildReport {
        verdict: "ten-OBJ 全消し! graphic rebuilt as palette-exact Korean 싹쓸이!".to_owned(),
        source_path,
        output_path,
        evidence_source,
        glyph_source: "src/effect_glyphs.rs 16px masks at 2x with the original rim and outline"
            .to_owned(),
        stream_pc: format!("0x{STREAM_PC:06X}"),
        stream_lorom: crate::rom::format_lorom_addr(STREAM_PC),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        stream_capacity,
        stream_headroom: stream_capacity - compressed.len(),
        decompressed_len: decoded.len(),
        runtime_tiles_match_source,
        frame_pc: format!("0x{FRAME_PC:06X}"),
        frame_lorom: format!("${frame_bank:02X}:${frame_address:04X}"),
        frame_sprites: OAM_COUNT,
        tile_groups: KOREAN_BASES
            .iter()
            .map(|base| format!("${base:02X}"))
            .collect(),
        palette_indices: vec![0, CORE_INDEX, RIM_INDEX, OUTLINE_INDEX],
        changed_tiles: target_tiles.len(),
        changed_decompressed_bytes,
        changed_frame_bytes,
        compression_roundtrip_matches,
        diff_confined_to_expected_ranges,
        preview: preview_path
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

fn palette_pixels_to_rgba(pixels: &[u8], palette: &[[u8; 3]; 16]) -> Vec<u8> {
    let mut rgba = vec![0; pixels.len() * 4];
    for (index, value) in pixels.iter().copied().enumerate() {
        if value == 0 {
            continue;
        }
        rgba[index * 4..index * 4 + 3].copy_from_slice(&palette[usize::from(value)]);
        rgba[index * 4 + 3] = 255;
    }
    rgba
}

fn encode_regular_grid(decoded: &mut [u8], pixels: &[u8]) -> Result<BTreeSet<usize>> {
    let mut target_tiles = BTreeSet::new();
    for (sprite, base) in KOREAN_BASES.iter().copied().enumerate() {
        let sprite_x = (sprite % 5) * 16;
        let sprite_y = (sprite / 5) * 16;
        for (qx, qy, tile) in [
            (0usize, 0usize, usize::from(base)),
            (8, 0, usize::from(base) + 1),
            (0, 8, usize::from(base) + 16),
            (8, 8, usize::from(base) + 17),
        ] {
            let mut encoded = [0u8; TILE_LEN];
            for y in 0..8 {
                for x in 0..8 {
                    let value = pixels[(sprite_y + qy + y) * CANVAS_WIDTH + sprite_x + qx + x];
                    let bit = 1 << (7 - x);
                    for plane in 0..4 {
                        if value & (1 << plane) != 0 {
                            let offset = if plane < 2 {
                                y * 2 + plane
                            } else {
                                16 + y * 2 + plane - 2
                            };
                            encoded[offset] |= bit;
                        }
                    }
                }
            }
            let start = tile * TILE_LEN;
            decoded
                .get_mut(start..start + TILE_LEN)
                .with_context(|| format!("zenkeshi target tile ${tile:02X} outside stream"))?
                .copy_from_slice(&encoded);
            target_tiles.insert(tile);
        }
    }
    Ok(target_tiles)
}

fn original_frame_bytes() -> [u8; FRAME_LEN] {
    frame_bytes(&[
        (-40, -4, 0xA0),
        (-24, -4, 0xA2),
        (-8, -4, 0xA4),
        (8, -12, 0xA6),
        (24, -12, 0xA8),
        (-40, -20, 0xAA),
        (-24, -20, 0xAC),
        (-8, -20, 0xAE),
        (8, 4, 0x9C),
        (24, 4, 0x9E),
    ])
}

fn korean_frame_bytes() -> [u8; FRAME_LEN] {
    let mut descriptors = [(0i16, 0i16, 0u8); OAM_COUNT];
    for (index, base) in KOREAN_BASES.iter().copied().enumerate() {
        descriptors[index] = (
            -40 + (index % 5) as i16 * 16,
            -20 + (index / 5) as i16 * 16,
            base,
        );
    }
    frame_bytes(&descriptors)
}

fn frame_bytes(descriptors: &[(i16, i16, u8); OAM_COUNT]) -> [u8; FRAME_LEN] {
    let mut frame = [0u8; FRAME_LEN];
    frame[0] = 0x02;
    frame[1] = OAM_COUNT as u8;
    for (index, (x, y, tile)) in descriptors.iter().copied().enumerate() {
        let offset = 2 + index * 8;
        frame[offset] = 0;
        frame[offset + 1..offset + 3].copy_from_slice(&x.to_le_bytes());
        frame[offset + 3..offset + 5].copy_from_slice(&y.to_le_bytes());
        frame[offset + 5] = tile;
        frame[offset + 6] = 0x02;
        frame[offset + 7] = 0x02;
    }
    frame
}

/// Renders `싹쓸이!` into the 80x32 sprite grid in the original 全消し! style:
/// Galmuri-derived masks at 2x, a brown core, a one-pixel rim on the top and
/// left edges and a one-pixel yellow outline.
fn render_canvas() -> Result<Vec<u8>> {
    const SCALE: usize = 2;
    const SPACING: usize = 1;
    let masks = ZENKESHI_TEXT
        .chars()
        .map(crate::effect_glyphs::glyph)
        .collect::<Result<Vec<_>>>()?;
    let text_width = masks
        .iter()
        .map(|mask| crate::effect_glyphs::width(mask) * SCALE)
        .sum::<usize>()
        + SPACING * SCALE * (masks.len() - 1);
    let text_height = masks.iter().map(|mask| mask.len()).max().unwrap_or(0) * SCALE;
    if text_width + 2 > CANVAS_WIDTH || text_height + 2 > SPRITE_ROWS_HEIGHT {
        bail!("zenkeshi text {text_width}x{text_height} does not fit the 80x32 sprite grid");
    }
    let mut body = vec![false; CANVAS_WIDTH * SPRITE_ROWS_HEIGHT];
    let mut x = (CANVAS_WIDTH - text_width) / 2;
    let top = (SPRITE_ROWS_HEIGHT - text_height) / 2;
    for mask in masks {
        let y = top + text_height - mask.len() * SCALE;
        crate::effect_glyphs::stamp(
            &mut body,
            CANVAS_WIDTH,
            SPRITE_ROWS_HEIGHT,
            mask,
            x,
            y,
            SCALE,
        )?;
        x += (crate::effect_glyphs::width(mask) + SPACING) * SCALE;
    }
    let ink = |x: i32, y: i32| {
        (0..CANVAS_WIDTH as i32).contains(&x)
            && (0..SPRITE_ROWS_HEIGHT as i32).contains(&y)
            && body[y as usize * CANVAS_WIDTH + x as usize]
    };
    let mut pixels = vec![0u8; body.len()];
    for y in 0..SPRITE_ROWS_HEIGHT as i32 {
        for x in 0..CANVAS_WIDTH as i32 {
            let index = y as usize * CANVAS_WIDTH + x as usize;
            pixels[index] = if ink(x, y) {
                let lit_edge = !ink(x, y - 1) || !ink(x - 1, y);
                if lit_edge { RIM_INDEX } else { CORE_INDEX }
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| ink(x + dx, y + dy))) {
                OUTLINE_INDEX
            } else {
                0
            };
        }
    }
    Ok(pixels)
}

fn target_tiles(oam: &[u8]) -> BTreeSet<usize> {
    (OAM_FIRST..OAM_FIRST + OAM_COUNT)
        .flat_map(|entry| {
            let base = usize::from(oam[entry * 4 + 2]);
            [base, base + 1, base + 16, base + 17]
        })
        .collect()
}

fn visible_name_table_zero_tiles(oam: &[u8]) -> Result<BTreeSet<usize>> {
    if oam.len() != 544 {
        bail!("zenkeshi runtime OAM is {} bytes, expected 544", oam.len());
    }
    let mut tiles = BTreeSet::new();
    for entry in 0..128usize {
        let offset = entry * 4;
        if oam[offset + 1] >= 224 || oam[offset + 3] & 1 != 0 {
            continue;
        }
        let base = usize::from(oam[offset + 2]);
        let high = (oam[512 + entry / 4] >> ((entry % 4) * 2)) & 3;
        if high & 2 != 0 {
            tiles.extend([base, base + 1, base + 16, base + 17]);
        } else {
            tiles.insert(base);
        }
    }
    Ok(tiles)
}

fn verify_oam(oam: &[u8]) -> Result<()> {
    if oam.len() != 544 {
        bail!("zenkeshi runtime OAM is {} bytes, expected 544", oam.len());
    }
    let expected: [[u8; 4]; OAM_COUNT] = [
        [0x10, 0x31, 0xA0, 0x02],
        [0x20, 0x31, 0xA2, 0x02],
        [0x30, 0x31, 0xA4, 0x02],
        [0x40, 0x29, 0xA6, 0x02],
        [0x50, 0x29, 0xA8, 0x02],
        [0x10, 0x21, 0xAA, 0x02],
        [0x20, 0x21, 0xAC, 0x02],
        [0x30, 0x21, 0xAE, 0x02],
        [0x40, 0x39, 0x9C, 0x02],
        [0x50, 0x39, 0x9E, 0x02],
    ];
    for (entry, bytes) in (OAM_FIRST..OAM_FIRST + OAM_COUNT).zip(expected) {
        let offset = entry * 4;
        if oam.get(offset..offset + 4) != Some(bytes.as_slice()) {
            bail!("zenkeshi OAM entry {entry} differs from the measured frame");
        }
        let high = (oam[512 + entry / 4] >> ((entry % 4) * 2)) & 3;
        if high & 2 == 0 {
            bail!("zenkeshi OAM entry {entry} is not a large sprite");
        }
    }
    Ok(())
}

fn render_oam_candidate(
    vram: &[u8],
    oam: &[u8],
    palette: &[[u8; 3]; 16],
    vram_base: usize,
) -> Result<(Vec<u8>, BTreeSet<u8>)> {
    let mut rgba = vec![0; CANVAS_WIDTH * CANVAS_HEIGHT * 4];
    let mut indices = BTreeSet::new();
    for entry in (OAM_FIRST..OAM_FIRST + OAM_COUNT).rev() {
        let offset = entry * 4;
        let sprite_x = i32::from(oam[offset]) - CANVAS_X;
        let sprite_y = i32::from(oam[offset + 1]) - CANVAS_Y;
        let base_tile = usize::from(oam[offset + 2]);
        for (qx, qy, tile_id) in [
            (0i32, 0i32, base_tile),
            (8, 0, base_tile + 1),
            (0, 8, base_tile + 16),
            (8, 8, base_tile + 17),
        ] {
            let start = vram_base + tile_id * TILE_LEN;
            let tile = vram
                .get(start..start + TILE_LEN)
                .with_context(|| format!("zenkeshi tile ${tile_id:02X} outside VRAM"))?;
            for y in 0..8i32 {
                for x in 0..8i32 {
                    let value = pixel(tile, x as usize, y as usize);
                    indices.insert(value);
                    if value == 0 {
                        continue;
                    }
                    let dx = sprite_x + qx + x;
                    let dy = sprite_y + qy + y;
                    if !(0..CANVAS_WIDTH as i32).contains(&dx)
                        || !(0..CANVAS_HEIGHT as i32).contains(&dy)
                    {
                        continue;
                    }
                    let destination = (dy as usize * CANVAS_WIDTH + dx as usize) * 4;
                    rgba[destination..destination + 3]
                        .copy_from_slice(&palette[usize::from(value)]);
                    rgba[destination + 3] = 255;
                }
            }
        }
    }
    Ok((rgba, indices))
}

fn obj_palette(cram: &[u8], id: usize) -> Result<[[u8; 3]; 16]> {
    let mut out = [[0; 3]; 16];
    for (index, rgb) in out.iter_mut().enumerate() {
        let offset = (128 + id * 16 + index) * 2;
        let bytes = cram
            .get(offset..offset + 2)
            .context("zenkeshi OBJ palette outside CGRAM")?;
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
        let mut encoder = png::Encoder::new(&mut output, CANVAS_WIDTH as u32, CANVAS_HEIGHT as u32);
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
    fn current_korean_graphic_fits_the_original_stream_without_a_global_hook() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let (patched, report) = build_kr_poc(
            &rom,
            "test Remix ROM".to_owned(),
            None,
            None,
            "test output".to_owned(),
        )
        .unwrap();

        assert!(report.patched_compressed_len <= report.stream_capacity);
        assert_eq!(report.stream_capacity, 3_456);
        assert_eq!(&patched[0x00_3D52..0x00_3D57], &rom[0x00_3D52..0x00_3D57]);
        assert_eq!(&patched[0x16_D739..0x16_D75D], &rom[0x16_D739..0x16_D75D]);
        assert!(report.diff_confined_to_expected_ranges);
    }
}
