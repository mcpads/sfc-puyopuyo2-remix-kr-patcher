use std::{collections::BTreeSet, fs, io::Cursor, ops::Range, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SHOCK_STREAM_PC: usize = 0x12_1F56;
const EXPECTED_COMPRESSED_LEN: usize = 6_003;
const EXPECTED_DECODED_LEN: usize = 7_872;
const EXPECTED_DECODED_SHA256: &str =
    "b2da6a35b99fdc4a8ef102092b921f02badd0388a9c86117747704565883ea7d";
const TILE_BYTES: usize = 32;
const TILEMAP_COLUMNS: usize = 32;
const TILEMAP_ROWS: usize = 32;
const VISIBLE_ROWS: usize = 28;
const SHOCK_COLOR_INDEX: u8 = 12;
const SHOCK_CANVAS_LEFT: usize = 24;
const SHOCK_CANVAS_TOP: usize = 2;
const SHOCK_CANVAS_COLUMNS: usize = 5;
const SHOCK_CANVAS_ROWS: usize = 8;
const SHOCK_CELLS: &[(usize, usize, usize)] = &[
    (2, 24, 0x02D),
    (2, 25, 0x02E),
    (2, 26, 0x02F),
    (2, 27, 0x030),
    (2, 28, 0x031),
    (3, 25, 0x042),
    (3, 26, 0x043),
    (3, 27, 0x044),
    (3, 28, 0x045),
    (4, 25, 0x054),
    (4, 26, 0x055),
    (5, 25, 0x063),
    (5, 26, 0x064),
    (6, 24, 0x072),
    (6, 25, 0x073),
    (7, 24, 0x083),
    (7, 25, 0x084),
    (8, 24, 0x095),
    (8, 25, 0x096),
    (8, 26, 0x097),
    (9, 24, 0x0A5),
    (9, 25, 0x0A6),
    (9, 26, 0x0A7),
];
const NORMALIZED_WIDTH: usize = SHOCK_CANVAS_COLUMNS * 8;
const NORMALIZED_HEIGHT: usize = SHOCK_CANVAS_ROWS * 8;
const NORMALIZED_INK_RGBA: [u8; 4] = [255, 255, 0, 255];
const NORMALIZED_BACKGROUND_RGBA: [u8; 4] = [0, 0, 0, 0];
const GLYPH_TARGETS: [GlyphTarget; 3] = [
    GlyphTarget {
        label: "쿠",
        x: 23,
        y: 1,
        width: 16,
        height: 14,
    },
    GlyphTarget {
        label: "궁",
        x: 8,
        y: 17,
        width: 16,
        height: 15,
    },
    GlyphTarget {
        label: "!",
        x: 4,
        y: 39,
        width: 8,
        height: 18,
    },
];

const GIANT_EXCLAMATION_MAP_STREAM_PC: usize = 0x12_1EBC;
const GIANT_EXCLAMATION_MAP_COMPRESSED_LEN: usize = 154;
const GIANT_EXCLAMATION_MAP_DECODED_LEN: usize = 4_096;
const GIANT_EXCLAMATION_MAP_DECODED_SHA256: &str =
    "8f3c96831a529a12c490c1ca378519a74aaa2756487dd3da37e0fb78fa6e5ca9";
const GIANT_EXCLAMATION_CHR_STREAM_PC: usize = 0x11_FF01;
const GIANT_EXCLAMATION_CHR_COMPRESSED_LEN: usize = 212;
const GIANT_EXCLAMATION_CHR_DECODED_LEN: usize = 512;
const GIANT_EXCLAMATION_CHR_DECODED_SHA256: &str =
    "1591e0d1abac1bb858dd5e2ec3e405bb7c2b23425c224f300339791156b5b4cf";
const GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET: usize = 0x800;
const GIANT_EXCLAMATION_CANVAS_LEFT: usize = 2;
const GIANT_EXCLAMATION_CANVAS_TOP: usize = 6;
const GIANT_EXCLAMATION_CANVAS_WIDTH: usize = 28;
const GIANT_EXCLAMATION_CANVAS_HEIGHT: usize = 14;
const GIANT_EXCLAMATION_BACKGROUND_WORD: u16 = 0x0C01;
const GIANT_EXCLAMATION_FOREGROUND_WORD: u16 = 0x0C07;
const GIANT_EXCLAMATION_SOURCE_FOREGROUND_CELLS: usize = 89;
const GIANT_EXCLAMATION_TEXT: &str = "앗!";
const GIANT_EXCLAMATION_COVERAGE_THRESHOLD: u8 = 96;

const GIANT_KAAKUN_MAP_STREAM_PC: usize = 0x12_5669;
const GIANT_KAAKUN_MAP_COMPRESSED_LEN: usize = 156;
const GIANT_KAAKUN_MAP_DECODED_LEN: usize = 2_048;
const GIANT_KAAKUN_MAP_DECODED_SHA256: &str =
    "94af7ba073ae960601cd329e16b1632b700b326877e34c99b3b49e638d6a6979";
const GIANT_KAAKUN_CANVAS_WIDTH: usize = 64;
const GIANT_KAAKUN_CANVAS_HEIGHT: usize = 16;
const GIANT_KAAKUN_BACKGROUND_WORD: u16 = 0x2000;
const GIANT_KAAKUN_FOREGROUND_WORD: u16 = 0x2001;
const GIANT_KAAKUN_SOURCE_FOREGROUND_CELLS: usize = 250;
const GIANT_KAAKUN_TEXT: &str = "카~ 군~!";
const GIANT_KAAKUN_COVERAGE_THRESHOLD: u8 = 96;
const GIANT_KAAKUN_PREVIEW_SCALE: usize = 4;
const GIANT_KAAKUN_FONT_SHA256: &str =
    "5265b2f437fe81f0c8095b44c0173dd9a276b58a42552bf983f21c0e69e6e8af";

#[derive(Debug, Clone, Copy)]
struct GlyphTarget {
    label: &'static str,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceGlyphBandReport {
    pub text: String,
    pub pixel_bounds: String,
    pub ink_pixels: usize,
    pub target_bounds: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShockTextAssetReport {
    pub verdict: String,
    pub input_path: String,
    pub input_sha256: String,
    pub input_dimensions: String,
    pub coverage_threshold: f32,
    pub source_ink_pixels: usize,
    pub source_bands: Vec<SourceGlyphBandReport>,
    pub normalized_dimensions: String,
    pub normalized_ink_pixels: usize,
    pub forbidden_ink_pixels: usize,
    pub output_path: String,
    pub output_sha256: String,
    pub preview_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShockTextBuildReport {
    pub verdict: String,
    pub source_path: String,
    pub asset_path: String,
    pub asset_sha256: String,
    pub output_path: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub write_range: String,
    pub normalized_ink_pixels: usize,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub compression_headroom: usize,
    pub decompressed_len: usize,
    pub changed_tiles: usize,
    pub changed_unowned_tiles: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_write_range: bool,
    pub output_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShockTextWrite {
    pub offset: usize,
    pub bytes: Vec<u8>,
    pub normalized_ink_pixels: usize,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub changed_tiles: usize,
    pub changed_unowned_tiles: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GiantExclamationBuildReport {
    pub verdict: String,
    pub source_path: String,
    pub ttf_path: String,
    pub ttf_size: f32,
    pub text: String,
    pub output_path: String,
    pub map_stream_pc: String,
    pub map_stream_lorom: String,
    pub write_range: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub compression_headroom: usize,
    pub decompressed_len: usize,
    pub source_foreground_cells: usize,
    pub korean_foreground_cells: usize,
    pub changed_map_cells: usize,
    pub changed_cells_outside_canvas: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_write_range: bool,
    pub preview_path: Option<String>,
    pub output_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GiantKaakunBuildReport {
    pub verdict: String,
    pub source_path: String,
    pub ttf_path: String,
    pub ttf_size: f32,
    pub coverage_threshold: u8,
    pub text: String,
    pub output_path: String,
    pub map_stream_pc: String,
    pub map_stream_lorom: String,
    pub write_range: String,
    pub source_decoded_sha256: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub compression_headroom: usize,
    pub decompressed_len: usize,
    pub source_foreground_cells: usize,
    pub korean_foreground_cells: usize,
    pub changed_map_cells: usize,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_write_range: bool,
    pub preview_path: Option<String>,
    pub output_sha256: String,
}

#[derive(Debug, Clone, Copy)]
struct TilemapCell {
    tile_id: usize,
    palette: u8,
    horizontal_flip: bool,
    vertical_flip: bool,
}

pub fn prepare_shock_text_asset(
    input_path: &Path,
    output_path: &Path,
    preview_path: &Path,
    coverage_threshold: f32,
) -> Result<ShockTextAssetReport> {
    if !(0.0..=1.0).contains(&coverage_threshold) || coverage_threshold == 0.0 {
        bail!("coverage threshold must be in (0, 1], got {coverage_threshold}");
    }
    let encoded = fs::read(input_path)
        .with_context(|| format!("read generated shock artwork {}", input_path.display()))?;
    let (source_width, source_height, source_rgb) = decode_rgb_or_rgba_png(&encoded)?;
    let source_mask = source_rgb
        .iter()
        .map(|rgb| is_generated_yellow(*rgb))
        .collect::<Vec<_>>();
    let source_ink_pixels = source_mask.iter().filter(|pixel| **pixel).count();
    if source_ink_pixels == 0 {
        bail!("generated shock artwork contains no yellow ink pixels");
    }

    let bands = find_three_glyph_bands(&source_mask, source_width, source_height)?;
    let mut normalized = vec![false; NORMALIZED_WIDTH * NORMALIZED_HEIGHT];
    let mut source_bands = Vec::with_capacity(GLYPH_TARGETS.len());
    for (band, target) in bands.iter().zip(GLYPH_TARGETS) {
        let bounds = ink_bounds_in_rows(&source_mask, source_width, band.clone())?;
        let ink_pixels = count_mask_pixels(&source_mask, source_width, bounds);
        place_scaled_glyph(
            &source_mask,
            source_width,
            bounds,
            &mut normalized,
            target,
            coverage_threshold,
        )?;
        source_bands.push(SourceGlyphBandReport {
            text: target.label.to_owned(),
            pixel_bounds: format_bounds(bounds),
            ink_pixels,
            target_bounds: format!(
                "x={}..{}, y={}..{}",
                target.x,
                target.x + target.width - 1,
                target.y,
                target.y + target.height - 1
            ),
        });
    }

    let forbidden_ink_pixels = normalized
        .iter()
        .enumerate()
        .filter(|(index, pixel)| {
            **pixel && !shock_pixel_is_owned(*index % NORMALIZED_WIDTH, *index / NORMALIZED_WIDTH)
        })
        .count();
    if forbidden_ink_pixels != 0 {
        bail!("normalized shock asset uses {forbidden_ink_pixels} pixels outside owned cells");
    }
    let normalized_ink_pixels = normalized.iter().filter(|pixel| **pixel).count();
    if normalized_ink_pixels == 0 {
        bail!("coverage threshold removed every normalized shock pixel");
    }

    let rgba = normalized
        .iter()
        .flat_map(|pixel| {
            if *pixel {
                NORMALIZED_INK_RGBA
            } else {
                NORMALIZED_BACKGROUND_RGBA
            }
        })
        .collect::<Vec<_>>();
    let output = encode_rgba_png(NORMALIZED_WIDTH, NORMALIZED_HEIGHT, &rgba)?;
    let preview = render_normalized_preview(&normalized, 8);
    let preview_png = encode_rgb_png(NORMALIZED_WIDTH * 8, NORMALIZED_HEIGHT * 8, &preview)?;
    write_asset(output_path, &output)?;
    write_asset(preview_path, &preview_png)?;

    Ok(ShockTextAssetReport {
        verdict: "generated 쿠궁! normalized into verified true-ending shock cells".to_owned(),
        input_path: input_path.display().to_string(),
        input_sha256: format!("{:x}", Sha256::digest(&encoded)),
        input_dimensions: format!("{source_width}x{source_height}"),
        coverage_threshold,
        source_ink_pixels,
        source_bands,
        normalized_dimensions: format!("{NORMALIZED_WIDTH}x{NORMALIZED_HEIGHT} RGBA"),
        normalized_ink_pixels,
        forbidden_ink_pixels,
        output_path: output_path.display().to_string(),
        output_sha256: format!("{:x}", Sha256::digest(&output)),
        preview_path: preview_path.display().to_string(),
    })
}

pub fn build_shock_text_kr(
    rom: &[u8],
    source_path: String,
    asset_path: String,
    asset_bytes: &[u8],
    output_path: String,
) -> Result<(Vec<u8>, ShockTextBuildReport)> {
    let write = prepare_shock_text_write(rom, asset_bytes)?;
    let write_range = write.offset..write.offset + write.bytes.len();
    let mut patched = rom.to_vec();
    patched[write_range.clone()].copy_from_slice(&write.bytes);
    let diff_confined_to_write_range = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(offset, (before, after))| before == after || write_range.contains(&offset));
    if !diff_confined_to_write_range {
        bail!("Korean shock-text ROM diff escaped its declared stream slot");
    }
    verify_installed_shock_text(&patched, &write.bytes)?;
    let (bank, address) = crate::rom::pc_to_lorom(write.offset);
    let report = ShockTextBuildReport {
        verdict: "Korean 쿠궁! inserted into the verified true-ending BG1 stream".to_owned(),
        source_path,
        asset_path,
        asset_sha256: format!("{:x}", Sha256::digest(asset_bytes)),
        output_path,
        stream_pc: format!("0x{:06X}", write.offset),
        stream_lorom: format!("${bank:02X}:${address:04X}"),
        write_range: format!("0x{:06X}..0x{:06X}", write_range.start, write_range.end),
        normalized_ink_pixels: write.normalized_ink_pixels,
        original_compressed_len: write.original_compressed_len,
        patched_compressed_len: write.patched_compressed_len,
        compression_headroom: write.bytes.len() - write.patched_compressed_len,
        decompressed_len: write.decompressed_len,
        changed_tiles: write.changed_tiles,
        changed_unowned_tiles: write.changed_unowned_tiles,
        compression_roundtrip_matches: true,
        diff_confined_to_write_range,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

pub(crate) fn prepare_shock_text_write(rom: &[u8], asset_bytes: &[u8]) -> Result<ShockTextWrite> {
    let block = verified_shock_chr(rom)?;
    let mask = decode_normalized_shock_asset(asset_bytes)?;
    let normalized_ink_pixels = mask.iter().filter(|pixel| **pixel).count();
    let mut patched_chr = block.bytes.clone();
    apply_shock_text_mask(&mut patched_chr, &mask)?;

    let owned_tiles = SHOCK_CELLS
        .iter()
        .map(|(_, _, tile_id)| *tile_id)
        .collect::<BTreeSet<_>>();
    let changed_tile_ids = block
        .bytes
        .as_chunks::<TILE_BYTES>()
        .0
        .iter()
        .zip(patched_chr.as_chunks::<TILE_BYTES>().0.iter())
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(tile_id, _)| tile_id)
        .collect::<BTreeSet<_>>();
    let changed_unowned_tiles = changed_tile_ids.difference(&owned_tiles).count();
    if changed_unowned_tiles != 0 {
        bail!("Korean shock text changed {changed_unowned_tiles} unowned BG1 tiles");
    }

    let compressed = crate::snes_lz::compress(&patched_chr);
    if compressed.len() > EXPECTED_COMPRESSED_LEN {
        bail!(
            "Korean shock stream grew to {} bytes, beyond its {}-byte slot",
            compressed.len(),
            EXPECTED_COMPRESSED_LEN
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == patched_chr;
    if !compression_roundtrip_matches {
        bail!("Korean shock stream compression round-trip failed");
    }

    let mut bytes = vec![0; EXPECTED_COMPRESSED_LEN];
    bytes[..compressed.len()].copy_from_slice(&compressed);
    Ok(ShockTextWrite {
        offset: SHOCK_STREAM_PC,
        bytes,
        normalized_ink_pixels,
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        decompressed_len: patched_chr.len(),
        changed_tiles: changed_tile_ids.len(),
        changed_unowned_tiles,
    })
}

pub(crate) fn verify_installed_shock_text(rom: &[u8], payload: &[u8]) -> Result<()> {
    if payload.len() != EXPECTED_COMPRESSED_LEN {
        bail!(
            "shock-text payload has {} bytes, expected {EXPECTED_COMPRESSED_LEN}",
            payload.len()
        );
    }
    let write_range = SHOCK_STREAM_PC..SHOCK_STREAM_PC + EXPECTED_COMPRESSED_LEN;
    let installed = rom
        .get(write_range)
        .context("shock-text stream slot is outside the ROM")?;
    if installed != payload {
        bail!("installed shock-text stream differs from the prepared payload");
    }
    let rebuilt = crate::snes_lz::decompress(rom, SHOCK_STREAM_PC)?;
    if rebuilt.bytes.len() != EXPECTED_DECODED_LEN {
        bail!(
            "installed shock-text stream decodes to {} bytes, expected {EXPECTED_DECODED_LEN}",
            rebuilt.bytes.len()
        );
    }
    if rebuilt.compressed_len > EXPECTED_COMPRESSED_LEN
        || payload[rebuilt.compressed_len..]
            .iter()
            .any(|byte| *byte != 0)
    {
        bail!("installed shock-text stream does not preserve zero-filled slot padding");
    }
    let recompressed = crate::snes_lz::compress(&rebuilt.bytes);
    if recompressed != payload[..rebuilt.compressed_len] {
        bail!("installed shock-text stream does not reproduce its prepared compression");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn build_giant_exclamation_kr(
    rom: &[u8],
    source_path: String,
    ttf_path: String,
    ttf_data: &[u8],
    font_px: f32,
    runtime_dump: Option<&Path>,
    preview_path: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, GiantExclamationBuildReport)> {
    if !font_px.is_finite() || font_px <= 0.0 {
        bail!("giant-exclamation TTF pixel size must be positive, got {font_px}");
    }
    if runtime_dump.is_some() != preview_path.is_some() {
        bail!("giant-exclamation runtime dump and preview path must both be present or omitted");
    }
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|error| anyhow::anyhow!("failed to parse giant-exclamation TTF: {error}"))?;
    let block = verified_giant_exclamation_map(rom)?;
    let source_map = block.bytes.clone();
    let mut korean_map = source_map.clone();
    let active_map = korean_map
        .get_mut(
            GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET
                ..GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET + TILEMAP_COLUMNS * TILEMAP_ROWS * 2,
        )
        .context("giant-exclamation active tilemap is outside decoded stream")?;
    verify_giant_exclamation_source_map(active_map)?;

    let mask = render_giant_exclamation_mask(&font, font_px)?;
    let korean_foreground_cells = mask.iter().filter(|cell| **cell).count();
    if korean_foreground_cells == 0 {
        bail!("giant-exclamation Korean mask contains no foreground cells");
    }
    for y in 0..GIANT_EXCLAMATION_CANVAS_HEIGHT {
        for x in 0..GIANT_EXCLAMATION_CANVAS_WIDTH {
            let column = GIANT_EXCLAMATION_CANVAS_LEFT + x;
            let row = GIANT_EXCLAMATION_CANVAS_TOP + y;
            let offset = (row * TILEMAP_COLUMNS + column) * 2;
            let source_word = u16::from_le_bytes([active_map[offset], active_map[offset + 1]]);
            if !matches!(
                source_word,
                GIANT_EXCLAMATION_BACKGROUND_WORD | GIANT_EXCLAMATION_FOREGROUND_WORD
            ) {
                bail!(
                    "giant-exclamation canvas cell ({column}, {row}) has protected word 0x{source_word:04X}"
                );
            }
            let replacement = if mask[y * GIANT_EXCLAMATION_CANVAS_WIDTH + x] {
                GIANT_EXCLAMATION_FOREGROUND_WORD
            } else {
                GIANT_EXCLAMATION_BACKGROUND_WORD
            };
            active_map[offset..offset + 2].copy_from_slice(&replacement.to_le_bytes());
        }
    }

    let changed_map_cells = source_map
        .as_chunks::<2>()
        .0
        .iter()
        .zip(korean_map.as_chunks::<2>().0.iter())
        .filter(|(before, after)| before != after)
        .count();
    let changed_cells_outside_canvas = source_map
        .as_chunks::<2>()
        .0
        .iter()
        .zip(korean_map.as_chunks::<2>().0.iter())
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .filter(|(cell, _)| !giant_exclamation_cell_is_owned(*cell))
        .count();
    if changed_cells_outside_canvas != 0 {
        bail!(
            "giant-exclamation patch changed {changed_cells_outside_canvas} cells outside its canvas"
        );
    }

    let compressed = crate::snes_lz::compress(&korean_map);
    if compressed.len() > GIANT_EXCLAMATION_MAP_COMPRESSED_LEN {
        bail!(
            "Korean giant-exclamation map grew to {} bytes, beyond its {}-byte slot",
            compressed.len(),
            GIANT_EXCLAMATION_MAP_COMPRESSED_LEN
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == korean_map;
    if !compression_roundtrip_matches {
        bail!("Korean giant-exclamation compression round-trip failed");
    }

    let write_range = GIANT_EXCLAMATION_MAP_STREAM_PC
        ..GIANT_EXCLAMATION_MAP_STREAM_PC + GIANT_EXCLAMATION_MAP_COMPRESSED_LEN;
    let mut patched = rom.to_vec();
    patched[write_range.start..write_range.start + compressed.len()].copy_from_slice(&compressed);
    patched[write_range.start + compressed.len()..write_range.end].fill(0);
    let diff_confined_to_write_range = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(offset, (before, after))| before == after || write_range.contains(&offset));
    if !diff_confined_to_write_range {
        bail!("Korean giant-exclamation ROM diff escaped its declared map-stream slot");
    }
    let rebuilt = crate::snes_lz::decompress(&patched, GIANT_EXCLAMATION_MAP_STREAM_PC)?;
    if rebuilt.bytes != korean_map || rebuilt.compressed_len != compressed.len() {
        bail!("Korean giant-exclamation map does not re-extract from the patched ROM");
    }

    let rendered_preview =
        if let (Some(runtime_dump), Some(preview_path)) = (runtime_dump, preview_path) {
            let chr = verified_giant_exclamation_chr(rom)?;
            let cram = fs::read(runtime_dump.join("cram.bin")).with_context(|| {
                format!(
                    "read giant-exclamation CGRAM from {}",
                    runtime_dump.display()
                )
            })?;
            let palettes = decode_cgram(&cram)?;
            let active_map = korean_map
                .get(
                    GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET
                        ..GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET + TILEMAP_COLUMNS * TILEMAP_ROWS * 2,
                )
                .context("patched giant-exclamation active tilemap is outside decoded stream")?;
            let cells = active_map
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| parse_tilemap_cell(u16::from_le_bytes([bytes[0], bytes[1]])))
                .collect::<Vec<_>>();
            let rgb = render_tilemap_from_chr(&chr.bytes, &cells, &palettes, VISIBLE_ROWS)?;
            write_asset(
                preview_path,
                &encode_rgb_png(TILEMAP_COLUMNS * 8, VISIBLE_ROWS * 8, &rgb)?,
            )?;
            Some(preview_path.display().to_string())
        } else {
            None
        };

    let (bank, address) = crate::rom::pc_to_lorom(GIANT_EXCLAMATION_MAP_STREAM_PC);
    let report = GiantExclamationBuildReport {
        verdict: "Korean 앗! inserted into the verified true-ending tilemap stream".to_owned(),
        source_path,
        ttf_path,
        ttf_size: font_px,
        text: GIANT_EXCLAMATION_TEXT.to_owned(),
        output_path,
        map_stream_pc: format!("0x{GIANT_EXCLAMATION_MAP_STREAM_PC:06X}"),
        map_stream_lorom: format!("${bank:02X}:${address:04X}"),
        write_range: format!("0x{:06X}..0x{:06X}", write_range.start, write_range.end),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        compression_headroom: GIANT_EXCLAMATION_MAP_COMPRESSED_LEN - compressed.len(),
        decompressed_len: korean_map.len(),
        source_foreground_cells: GIANT_EXCLAMATION_SOURCE_FOREGROUND_CELLS,
        korean_foreground_cells,
        changed_map_cells,
        changed_cells_outside_canvas,
        compression_roundtrip_matches,
        diff_confined_to_write_range,
        preview_path: rendered_preview,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

#[allow(clippy::too_many_arguments)]
pub fn build_giant_kaakun_kr(
    rom: &[u8],
    source_path: String,
    ttf_path: String,
    ttf_data: &[u8],
    font_px: f32,
    preview_path: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, GiantKaakunBuildReport)> {
    if font_px != 12.0 {
        bail!("giant Ka-kun lettering requires fixed Galmuri11 Bold 12px, got {font_px}");
    }
    let font_sha256 = format!("{:x}", Sha256::digest(ttf_data));
    if font_sha256 != GIANT_KAAKUN_FONT_SHA256 {
        bail!(
            "giant Ka-kun font SHA-256 mismatch: expected {GIANT_KAAKUN_FONT_SHA256}, got {font_sha256}"
        );
    }
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|error| anyhow::anyhow!("failed to parse giant Ka-kun TTF: {error}"))?;
    let block = verified_giant_kaakun_map(rom)?;
    let source_surface = decode_giant_kaakun_surface(&block.bytes)?;
    let source_foreground_cells = source_surface.iter().filter(|cell| **cell).count();
    if source_foreground_cells != GIANT_KAAKUN_SOURCE_FOREGROUND_CELLS {
        bail!(
            "giant Ka-kun source has {source_foreground_cells} foreground cells, expected {GIANT_KAAKUN_SOURCE_FOREGROUND_CELLS}"
        );
    }

    let korean_surface = render_giant_kaakun_mask(&font, font_px)?;
    let korean_foreground_cells = korean_surface.iter().filter(|cell| **cell).count();
    if korean_foreground_cells == 0 {
        bail!("giant Ka-kun Korean lettering rendered blank");
    }
    let korean_map = encode_giant_kaakun_surface(&korean_surface)?;
    let changed_map_cells = block
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .zip(korean_map.as_chunks::<2>().0.iter())
        .filter(|(before, after)| before != after)
        .count();

    let compressed = crate::snes_lz::compress(&korean_map);
    if compressed.len() > GIANT_KAAKUN_MAP_COMPRESSED_LEN {
        bail!(
            "Korean giant Ka-kun map grew to {} bytes, beyond its {}-byte slot",
            compressed.len(),
            GIANT_KAAKUN_MAP_COMPRESSED_LEN
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == korean_map;
    if !compression_roundtrip_matches {
        bail!("Korean giant Ka-kun compression round-trip failed");
    }

    let write_range =
        GIANT_KAAKUN_MAP_STREAM_PC..GIANT_KAAKUN_MAP_STREAM_PC + GIANT_KAAKUN_MAP_COMPRESSED_LEN;
    let mut patched = rom.to_vec();
    patched[write_range.start..write_range.start + compressed.len()].copy_from_slice(&compressed);
    patched[write_range.start + compressed.len()..write_range.end].fill(0);
    let diff_confined_to_write_range = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(offset, (before, after))| before == after || write_range.contains(&offset));
    if !diff_confined_to_write_range {
        bail!("Korean giant Ka-kun ROM diff escaped its declared map-stream slot");
    }
    let rebuilt = crate::snes_lz::decompress(&patched, GIANT_KAAKUN_MAP_STREAM_PC)?;
    if rebuilt.bytes != korean_map || rebuilt.compressed_len != compressed.len() {
        bail!("Korean giant Ka-kun map does not re-extract from the patched ROM");
    }

    let rendered_preview = if let Some(preview_path) = preview_path {
        let width = GIANT_KAAKUN_CANVAS_WIDTH * GIANT_KAAKUN_PREVIEW_SCALE;
        let height = GIANT_KAAKUN_CANVAS_HEIGHT * GIANT_KAAKUN_PREVIEW_SCALE;
        let mut rgb = vec![0u8; width * height * 3];
        for y in 0..GIANT_KAAKUN_CANVAS_HEIGHT {
            for x in 0..GIANT_KAAKUN_CANVAS_WIDTH {
                let color = if korean_surface[y * GIANT_KAAKUN_CANVAS_WIDTH + x] {
                    [255, 0, 0]
                } else {
                    [0, 0, 0]
                };
                for dy in 0..GIANT_KAAKUN_PREVIEW_SCALE {
                    for dx in 0..GIANT_KAAKUN_PREVIEW_SCALE {
                        let destination = ((y * GIANT_KAAKUN_PREVIEW_SCALE + dy) * width
                            + x * GIANT_KAAKUN_PREVIEW_SCALE
                            + dx)
                            * 3;
                        rgb[destination..destination + 3].copy_from_slice(&color);
                    }
                }
            }
        }
        write_asset(preview_path, &encode_rgb_png(width, height, &rgb)?)?;
        Some(preview_path.display().to_string())
    } else {
        None
    };

    let (bank, address) = crate::rom::pc_to_lorom(GIANT_KAAKUN_MAP_STREAM_PC);
    let report = GiantKaakunBuildReport {
        verdict: "Korean 카~ 군~! inserted into the verified true-ending BG3 map stream".to_owned(),
        source_path,
        ttf_path,
        ttf_size: font_px,
        coverage_threshold: GIANT_KAAKUN_COVERAGE_THRESHOLD,
        text: GIANT_KAAKUN_TEXT.to_owned(),
        output_path,
        map_stream_pc: format!("0x{GIANT_KAAKUN_MAP_STREAM_PC:06X}"),
        map_stream_lorom: format!("${bank:02X}:${address:04X}"),
        write_range: format!("0x{:06X}..0x{:06X}", write_range.start, write_range.end),
        source_decoded_sha256: GIANT_KAAKUN_MAP_DECODED_SHA256.to_owned(),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        compression_headroom: GIANT_KAAKUN_MAP_COMPRESSED_LEN - compressed.len(),
        decompressed_len: korean_map.len(),
        source_foreground_cells,
        korean_foreground_cells,
        changed_map_cells,
        compression_roundtrip_matches,
        diff_confined_to_write_range,
        preview_path: rendered_preview,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

fn parse_tilemap_cell(word: u16) -> TilemapCell {
    TilemapCell {
        tile_id: usize::from(word & 0x03FF),
        palette: ((word >> 10) & 7) as u8,
        horizontal_flip: word & 0x4000 != 0,
        vertical_flip: word & 0x8000 != 0,
    }
}

fn verified_giant_exclamation_map(rom: &[u8]) -> Result<crate::snes_lz::LzBlock> {
    let block = crate::snes_lz::decompress(rom, GIANT_EXCLAMATION_MAP_STREAM_PC)?;
    if block.compressed_len != GIANT_EXCLAMATION_MAP_COMPRESSED_LEN
        || block.bytes.len() != GIANT_EXCLAMATION_MAP_DECODED_LEN
    {
        bail!(
            "giant-exclamation map differs from Spec: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let decoded_sha256 = format!("{:x}", Sha256::digest(&block.bytes));
    if decoded_sha256 != GIANT_EXCLAMATION_MAP_DECODED_SHA256 {
        bail!("giant-exclamation map SHA-256 differs from Spec: {decoded_sha256}");
    }
    Ok(block)
}

fn verified_giant_exclamation_chr(rom: &[u8]) -> Result<crate::snes_lz::LzBlock> {
    let block = crate::snes_lz::decompress(rom, GIANT_EXCLAMATION_CHR_STREAM_PC)?;
    if block.compressed_len != GIANT_EXCLAMATION_CHR_COMPRESSED_LEN
        || block.bytes.len() != GIANT_EXCLAMATION_CHR_DECODED_LEN
    {
        bail!(
            "giant-exclamation CHR differs from Spec: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let decoded_sha256 = format!("{:x}", Sha256::digest(&block.bytes));
    if decoded_sha256 != GIANT_EXCLAMATION_CHR_DECODED_SHA256 {
        bail!("giant-exclamation CHR SHA-256 differs from Spec: {decoded_sha256}");
    }
    Ok(block)
}

fn verify_giant_exclamation_source_map(active_map: &[u8]) -> Result<()> {
    if active_map.len() != TILEMAP_COLUMNS * TILEMAP_ROWS * 2 {
        bail!(
            "giant-exclamation active map is {} bytes, expected {}",
            active_map.len(),
            TILEMAP_COLUMNS * TILEMAP_ROWS * 2
        );
    }
    let mut foreground_cells = 0usize;
    for (cell, bytes) in active_map.as_chunks::<2>().0.iter().enumerate() {
        let word = u16::from_le_bytes([bytes[0], bytes[1]]);
        if word != GIANT_EXCLAMATION_FOREGROUND_WORD {
            continue;
        }
        foreground_cells += 1;
        let column = cell % TILEMAP_COLUMNS;
        let row = cell / TILEMAP_COLUMNS;
        let in_canvas = (GIANT_EXCLAMATION_CANVAS_LEFT
            ..GIANT_EXCLAMATION_CANVAS_LEFT + GIANT_EXCLAMATION_CANVAS_WIDTH)
            .contains(&column)
            && (GIANT_EXCLAMATION_CANVAS_TOP
                ..GIANT_EXCLAMATION_CANVAS_TOP + GIANT_EXCLAMATION_CANVAS_HEIGHT)
                .contains(&row);
        if !in_canvas {
            bail!(
                "giant-exclamation foreground cell ({column}, {row}) is outside the measured canvas"
            );
        }
    }
    if foreground_cells != GIANT_EXCLAMATION_SOURCE_FOREGROUND_CELLS {
        bail!(
            "giant-exclamation source has {foreground_cells} foreground cells, expected {GIANT_EXCLAMATION_SOURCE_FOREGROUND_CELLS}"
        );
    }
    Ok(())
}

fn giant_exclamation_cell_is_owned(decoded_cell: usize) -> bool {
    if decoded_cell < GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET / 2 {
        return false;
    }
    let active_cell = decoded_cell - GIANT_EXCLAMATION_ACTIVE_MAP_OFFSET / 2;
    let column = active_cell % TILEMAP_COLUMNS;
    let row = active_cell / TILEMAP_COLUMNS;
    (GIANT_EXCLAMATION_CANVAS_LEFT..GIANT_EXCLAMATION_CANVAS_LEFT + GIANT_EXCLAMATION_CANVAS_WIDTH)
        .contains(&column)
        && (GIANT_EXCLAMATION_CANVAS_TOP
            ..GIANT_EXCLAMATION_CANVAS_TOP + GIANT_EXCLAMATION_CANVAS_HEIGHT)
            .contains(&row)
}

fn render_giant_exclamation_mask(font: &fontdue::Font, px: f32) -> Result<Vec<bool>> {
    let characters = GIANT_EXCLAMATION_TEXT.chars().collect::<Vec<_>>();
    if characters.len() != 2 {
        bail!("giant-exclamation text must contain exactly two characters");
    }
    let cell_width = GIANT_EXCLAMATION_CANVAS_WIDTH / characters.len();
    let ascent = font
        .horizontal_line_metrics(px)
        .context("giant-exclamation TTF has no horizontal line metrics")?
        .ascent as i32;
    let baseline = (GIANT_EXCLAMATION_CANVAS_HEIGHT as i32 + ascent) / 2;
    let mut coverage = vec![0u8; GIANT_EXCLAMATION_CANVAS_WIDTH * GIANT_EXCLAMATION_CANVAS_HEIGHT];
    for (cell, character) in characters.into_iter().enumerate() {
        let (metrics, raster) = font.rasterize(character, px);
        if metrics.width == 0 || metrics.height == 0 || raster.iter().all(|value| *value == 0) {
            bail!("TTF produced an empty giant-exclamation glyph for {character:?} at {px}px");
        }
        let cell_x = cell * cell_width;
        let x_offset =
            cell_x as i32 + (cell_width as i32 - metrics.width as i32) / 2 - metrics.xmin;
        let y_offset = baseline - metrics.ymin - metrics.height as i32;
        for row in 0..metrics.height {
            for column in 0..metrics.width {
                let x = x_offset + column as i32;
                let y = y_offset + row as i32;
                if !(0..GIANT_EXCLAMATION_CANVAS_WIDTH as i32).contains(&x)
                    || !(0..GIANT_EXCLAMATION_CANVAS_HEIGHT as i32).contains(&y)
                {
                    continue;
                }
                let destination = y as usize * GIANT_EXCLAMATION_CANVAS_WIDTH + x as usize;
                coverage[destination] =
                    coverage[destination].max(raster[row * metrics.width + column]);
            }
        }
    }
    Ok(coverage
        .into_iter()
        .map(|value| value >= GIANT_EXCLAMATION_COVERAGE_THRESHOLD)
        .collect())
}

fn verified_giant_kaakun_map(rom: &[u8]) -> Result<crate::snes_lz::LzBlock> {
    let block = crate::snes_lz::decompress(rom, GIANT_KAAKUN_MAP_STREAM_PC)?;
    let decoded_sha256 = format!("{:x}", Sha256::digest(&block.bytes));
    if block.compressed_len != GIANT_KAAKUN_MAP_COMPRESSED_LEN
        || block.bytes.len() != GIANT_KAAKUN_MAP_DECODED_LEN
        || decoded_sha256 != GIANT_KAAKUN_MAP_DECODED_SHA256
    {
        bail!(
            "giant Ka-kun map differs from Spec: compressed {} / decoded {} / SHA-256 {}",
            block.compressed_len,
            block.bytes.len(),
            decoded_sha256
        );
    }
    Ok(block)
}

fn decode_giant_kaakun_surface(decoded: &[u8]) -> Result<Vec<bool>> {
    if decoded.len() != GIANT_KAAKUN_MAP_DECODED_LEN {
        bail!(
            "giant Ka-kun decoded map is {} bytes, expected {GIANT_KAAKUN_MAP_DECODED_LEN}",
            decoded.len()
        );
    }
    let words = decoded
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    if let Some(word) = words.iter().find(|word| {
        !matches!(
            **word,
            GIANT_KAAKUN_BACKGROUND_WORD | GIANT_KAAKUN_FOREGROUND_WORD
        )
    }) {
        bail!("giant Ka-kun source uses protected tilemap word 0x{word:04X}");
    }

    let mut surface = vec![false; GIANT_KAAKUN_CANVAS_WIDTH * GIANT_KAAKUN_CANVAS_HEIGHT];
    for y in 0..GIANT_KAAKUN_CANVAS_HEIGHT {
        for x in 0..32 {
            surface[y * GIANT_KAAKUN_CANVAS_WIDTH + x] =
                words[y * 32 + x] == GIANT_KAAKUN_FOREGROUND_WORD;
        }
        for chunk in 0..4 {
            for local_x in 0..8 {
                let source = 32 * 16 + chunk * 8 * 16 + y * 8 + local_x;
                let x = 32 + chunk * 8 + local_x;
                surface[y * GIANT_KAAKUN_CANVAS_WIDTH + x] =
                    words[source] == GIANT_KAAKUN_FOREGROUND_WORD;
            }
        }
    }
    Ok(surface)
}

fn encode_giant_kaakun_surface(surface: &[bool]) -> Result<Vec<u8>> {
    if surface.len() != GIANT_KAAKUN_CANVAS_WIDTH * GIANT_KAAKUN_CANVAS_HEIGHT {
        bail!(
            "giant Ka-kun surface has {} cells, expected {}",
            surface.len(),
            GIANT_KAAKUN_CANVAS_WIDTH * GIANT_KAAKUN_CANVAS_HEIGHT
        );
    }
    let mut decoded = Vec::with_capacity(GIANT_KAAKUN_MAP_DECODED_LEN);
    let mut write_word = |ink: bool| {
        decoded.extend_from_slice(
            &(if ink {
                GIANT_KAAKUN_FOREGROUND_WORD
            } else {
                GIANT_KAAKUN_BACKGROUND_WORD
            })
            .to_le_bytes(),
        );
    };
    for y in 0..GIANT_KAAKUN_CANVAS_HEIGHT {
        for x in 0..32 {
            write_word(surface[y * GIANT_KAAKUN_CANVAS_WIDTH + x]);
        }
    }
    for chunk in 0..4 {
        for y in 0..GIANT_KAAKUN_CANVAS_HEIGHT {
            for local_x in 0..8 {
                let x = 32 + chunk * 8 + local_x;
                write_word(surface[y * GIANT_KAAKUN_CANVAS_WIDTH + x]);
            }
        }
    }
    Ok(decoded)
}

fn render_giant_kaakun_mask(font: &fontdue::Font, px: f32) -> Result<Vec<bool>> {
    struct RasterizedGlyph {
        metrics: fontdue::Metrics,
        coverage: Vec<u8>,
    }

    let characters = GIANT_KAAKUN_TEXT.chars().collect::<Vec<_>>();
    let mut glyphs = Vec::with_capacity(characters.len());
    let mut total_width = characters.len().saturating_sub(1);
    for character in characters {
        if character == ' ' {
            total_width += 4;
            glyphs.push(None);
            continue;
        }
        let (metrics, coverage) = font.rasterize(character, px);
        if metrics.width == 0 || metrics.height == 0 || coverage.iter().all(|value| *value == 0) {
            bail!("TTF produced an empty giant Ka-kun glyph for {character:?} at {px}px");
        }
        total_width += metrics.width;
        glyphs.push(Some(RasterizedGlyph { metrics, coverage }));
    }
    if total_width > GIANT_KAAKUN_CANVAS_WIDTH {
        bail!(
            "giant Ka-kun lettering is {total_width} cells wide, beyond its {GIANT_KAAKUN_CANVAS_WIDTH}-cell canvas"
        );
    }

    let line_metrics = font
        .horizontal_line_metrics(px)
        .context("giant Ka-kun TTF has no horizontal line metrics")?;
    let line_height = (line_metrics.ascent - line_metrics.descent).ceil() as i32;
    if line_height > GIANT_KAAKUN_CANVAS_HEIGHT as i32 {
        bail!(
            "giant Ka-kun font line height {line_height} exceeds its {GIANT_KAAKUN_CANVAS_HEIGHT}-cell canvas"
        );
    }
    let baseline =
        (GIANT_KAAKUN_CANVAS_HEIGHT as i32 - line_height) / 2 + line_metrics.ascent.ceil() as i32;
    let mut mask = vec![false; GIANT_KAAKUN_CANVAS_WIDTH * GIANT_KAAKUN_CANVAS_HEIGHT];
    let mut cursor = (GIANT_KAAKUN_CANVAS_WIDTH - total_width) / 2;
    for (index, glyph) in glyphs.into_iter().enumerate() {
        if let Some(glyph) = glyph {
            let y_offset = baseline - glyph.metrics.ymin - glyph.metrics.height as i32;
            for y in 0..glyph.metrics.height {
                for x in 0..glyph.metrics.width {
                    if glyph.coverage[y * glyph.metrics.width + x]
                        <= GIANT_KAAKUN_COVERAGE_THRESHOLD
                    {
                        continue;
                    }
                    let destination_y = y_offset + y as i32;
                    if !(0..GIANT_KAAKUN_CANVAS_HEIGHT as i32).contains(&destination_y) {
                        bail!(
                            "giant Ka-kun glyph row {destination_y} escaped its {GIANT_KAAKUN_CANVAS_HEIGHT}-cell canvas"
                        );
                    }
                    mask[destination_y as usize * GIANT_KAAKUN_CANVAS_WIDTH + cursor + x] = true;
                }
            }
            cursor += glyph.metrics.width;
        } else {
            cursor += 4;
        }
        if index + 1 < GIANT_KAAKUN_TEXT.chars().count() {
            cursor += 1;
        }
    }
    Ok(mask)
}

fn verified_shock_chr(rom: &[u8]) -> Result<crate::snes_lz::LzBlock> {
    let block = crate::snes_lz::decompress(rom, SHOCK_STREAM_PC)?;
    if block.compressed_len != EXPECTED_COMPRESSED_LEN || block.bytes.len() != EXPECTED_DECODED_LEN
    {
        bail!(
            "true-ending BG1 stream differs from Spec: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let decoded_sha256 = format!("{:x}", Sha256::digest(&block.bytes));
    if decoded_sha256 != EXPECTED_DECODED_SHA256 {
        bail!("true-ending BG1 decoded SHA-256 differs from Spec: {decoded_sha256}");
    }
    Ok(block)
}

fn decode_normalized_shock_asset(encoded: &[u8]) -> Result<Vec<bool>> {
    let decoder = png::Decoder::new(Cursor::new(encoded));
    let mut reader = decoder
        .read_info()
        .context("read normalized shock PNG info")?;
    let mut bytes = vec![
        0u8;
        reader
            .output_buffer_size()
            .context("normalized shock PNG output is too large")?
    ];
    let info = reader
        .next_frame(&mut bytes)
        .context("decode normalized shock PNG")?;
    if info.width as usize != NORMALIZED_WIDTH || info.height as usize != NORMALIZED_HEIGHT {
        bail!(
            "normalized shock PNG is {}x{}, expected {}x{}",
            info.width,
            info.height,
            NORMALIZED_WIDTH,
            NORMALIZED_HEIGHT
        );
    }
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!("normalized shock PNG must be 8-bit RGBA");
    }
    bytes.truncate(info.buffer_size());
    let mut mask = Vec::with_capacity(NORMALIZED_WIDTH * NORMALIZED_HEIGHT);
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let rgba = [pixel[0], pixel[1], pixel[2], pixel[3]];
        let ink = if rgba == NORMALIZED_INK_RGBA {
            true
        } else if rgba == NORMALIZED_BACKGROUND_RGBA {
            false
        } else {
            bail!(
                "normalized shock PNG pixel ({}, {}) is {:?}, expected transparent or yellow",
                index % NORMALIZED_WIDTH,
                index / NORMALIZED_WIDTH,
                rgba
            );
        };
        if ink && !shock_pixel_is_owned(index % NORMALIZED_WIDTH, index / NORMALIZED_WIDTH) {
            bail!(
                "normalized shock PNG pixel ({}, {}) is outside verified cells",
                index % NORMALIZED_WIDTH,
                index / NORMALIZED_WIDTH
            );
        }
        mask.push(ink);
    }
    if !mask.iter().any(|pixel| *pixel) {
        bail!("normalized shock PNG contains no ink");
    }
    Ok(mask)
}

fn apply_shock_text_mask(chr: &mut [u8], mask: &[bool]) -> Result<()> {
    if chr.len() != EXPECTED_DECODED_LEN {
        bail!("true-ending BG1 CHR has {} bytes", chr.len());
    }
    if mask.len() != NORMALIZED_WIDTH * NORMALIZED_HEIGHT {
        bail!("normalized shock mask has {} pixels", mask.len());
    }
    for &(_, _, tile_id) in SHOCK_CELLS {
        let tile = &mut chr[tile_id * TILE_BYTES..(tile_id + 1) * TILE_BYTES];
        for y in 0..8 {
            for x in 0..8 {
                let color = decode_4bpp_pixel(tile, x, y);
                if !matches!(color, 0 | SHOCK_COLOR_INDEX) {
                    bail!("shock tile ${tile_id:03X} contains protected color index {color}");
                }
            }
        }
        tile.fill(0);
    }
    for (index, ink) in mask.iter().enumerate() {
        if !*ink {
            continue;
        }
        let x = index % NORMALIZED_WIDTH;
        let y = index / NORMALIZED_WIDTH;
        let column = SHOCK_CANVAS_LEFT + x / 8;
        let row = SHOCK_CANVAS_TOP + y / 8;
        let tile_id = SHOCK_CELLS
            .iter()
            .find(|(owned_row, owned_column, _)| *owned_row == row && *owned_column == column)
            .map(|(_, _, tile_id)| *tile_id)
            .with_context(|| format!("shock pixel ({x}, {y}) has no owned tile"))?;
        let tile = &mut chr[tile_id * TILE_BYTES..(tile_id + 1) * TILE_BYTES];
        set_4bpp_pixel(tile, x % 8, y % 8, SHOCK_COLOR_INDEX);
    }
    Ok(())
}

fn decode_rgb_or_rgba_png(encoded: &[u8]) -> Result<(usize, usize, Vec<[u8; 3]>)> {
    let decoder = png::Decoder::new(Cursor::new(encoded));
    let mut reader = decoder.read_info().context("read generated PNG info")?;
    let mut bytes = vec![
        0u8;
        reader
            .output_buffer_size()
            .context("generated PNG output is too large")?
    ];
    let info = reader
        .next_frame(&mut bytes)
        .context("decode generated PNG")?;
    if info.bit_depth != png::BitDepth::Eight {
        bail!("generated PNG must use 8-bit channels");
    }
    bytes.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect(),
        png::ColorType::Rgba => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect(),
        other => bail!("generated PNG must be RGB or RGBA, got {other:?}"),
    };
    Ok((info.width as usize, info.height as usize, pixels))
}

fn is_generated_yellow(rgb: [u8; 3]) -> bool {
    let [red, green, blue] = rgb;
    red >= 128 && green >= 96 && red.saturating_sub(blue) >= 80 && green.saturating_sub(blue) >= 64
}

fn find_three_glyph_bands(mask: &[bool], width: usize, height: usize) -> Result<Vec<Range<usize>>> {
    if mask.len() != width * height {
        bail!("generated mask dimensions do not match its pixel count");
    }
    let mut bands = Vec::<Range<usize>>::new();
    let mut start = None;
    for row in 0..height {
        let has_ink = mask[row * width..(row + 1) * width]
            .iter()
            .any(|pixel| *pixel);
        match (start, has_ink) {
            (None, true) => start = Some(row),
            (Some(first), false) => {
                bands.push(first..row);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start {
        bands.push(first..height);
    }
    while bands.len() > GLYPH_TARGETS.len() {
        let merge_index = bands
            .windows(2)
            .enumerate()
            .min_by_key(|(_, adjacent)| adjacent[1].start - adjacent[0].end)
            .map(|(index, _)| index)
            .context("generated glyph bands cannot be merged")?;
        let merged = bands[merge_index].start..bands[merge_index + 1].end;
        bands.splice(merge_index..=merge_index + 1, [merged]);
    }
    if bands.len() != GLYPH_TARGETS.len() {
        bail!(
            "generated artwork resolves to {} vertical glyph bands, expected {} for 쿠궁!",
            bands.len(),
            GLYPH_TARGETS.len()
        );
    }
    Ok(bands)
}

fn ink_bounds_in_rows(
    mask: &[bool],
    width: usize,
    rows: Range<usize>,
) -> Result<(usize, usize, usize, usize)> {
    let mut bounds = None::<(usize, usize, usize, usize)>;
    for y in rows {
        for x in 0..width {
            if !mask[y * width + x] {
                continue;
            }
            bounds = Some(match bounds {
                Some((min_x, min_y, max_x, max_y)) => {
                    (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                }
                None => (x, y, x, y),
            });
        }
    }
    bounds.context("generated glyph band contains no ink")
}

fn count_mask_pixels(mask: &[bool], width: usize, bounds: (usize, usize, usize, usize)) -> usize {
    let (min_x, min_y, max_x, max_y) = bounds;
    (min_y..=max_y)
        .flat_map(|y| (min_x..=max_x).map(move |x| (x, y)))
        .filter(|(x, y)| mask[*y * width + *x])
        .count()
}

fn format_bounds(bounds: (usize, usize, usize, usize)) -> String {
    let (min_x, min_y, max_x, max_y) = bounds;
    format!("x={min_x}..{max_x}, y={min_y}..{max_y}")
}

fn place_scaled_glyph(
    source: &[bool],
    source_stride: usize,
    source_bounds: (usize, usize, usize, usize),
    destination: &mut [bool],
    target: GlyphTarget,
    coverage_threshold: f32,
) -> Result<()> {
    let (min_x, min_y, max_x, max_y) = source_bounds;
    let source_width = max_x - min_x + 1;
    let source_height = max_y - min_y + 1;
    let (scaled_width, scaled_height) =
        if source_width * target.height > source_height * target.width {
            (
                target.width,
                (source_height * target.width / source_width).max(1),
            )
        } else {
            (
                (source_width * target.height / source_height).max(1),
                target.height,
            )
        };
    let left = target.x + (target.width - scaled_width) / 2;
    let top = target.y + (target.height - scaled_height) / 2;
    for target_y in 0..scaled_height {
        let source_y_start = min_y + target_y * source_height / scaled_height;
        let source_y_end = min_y + ((target_y + 1) * source_height).div_ceil(scaled_height);
        for target_x in 0..scaled_width {
            let source_x_start = min_x + target_x * source_width / scaled_width;
            let source_x_end = min_x + ((target_x + 1) * source_width).div_ceil(scaled_width);
            let mut covered = 0usize;
            let mut samples = 0usize;
            for source_y in source_y_start..=source_y_end.min(max_y) {
                for source_x in source_x_start..=source_x_end.min(max_x) {
                    covered += usize::from(source[source_y * source_stride + source_x]);
                    samples += 1;
                }
            }
            if covered as f32 / (samples as f32) < coverage_threshold {
                continue;
            }
            let x = left + target_x;
            let y = top + target_y;
            if !shock_pixel_is_owned(x, y) {
                bail!(
                    "normalized {} pixel ({x}, {y}) falls outside verified shock cells",
                    target.label
                );
            }
            destination[y * NORMALIZED_WIDTH + x] = true;
        }
    }
    Ok(())
}

fn shock_pixel_is_owned(x: usize, y: usize) -> bool {
    if x >= NORMALIZED_WIDTH || y >= NORMALIZED_HEIGHT {
        return false;
    }
    let column = SHOCK_CANVAS_LEFT + x / 8;
    let row = SHOCK_CANVAS_TOP + y / 8;
    SHOCK_CELLS
        .iter()
        .any(|&(owned_row, owned_column, _)| owned_row == row && owned_column == column)
}

fn render_normalized_preview(mask: &[bool], scale: usize) -> Vec<u8> {
    let width = NORMALIZED_WIDTH * scale;
    let height = NORMALIZED_HEIGHT * scale;
    let mut rgb = vec![0u8; width * height * 3];
    for source_y in 0..NORMALIZED_HEIGHT {
        for source_x in 0..NORMALIZED_WIDTH {
            let color = if mask[source_y * NORMALIZED_WIDTH + source_x] {
                [255, 224, 0]
            } else if shock_pixel_is_owned(source_x, source_y) {
                [0, 0, 0]
            } else {
                [32, 32, 32]
            };
            for offset_y in 0..scale {
                for offset_x in 0..scale {
                    let x = source_x * scale + offset_x;
                    let y = source_y * scale + offset_y;
                    let pixel = (y * width + x) * 3;
                    rgb[pixel..pixel + 3].copy_from_slice(&color);
                }
            }
        }
    }
    rgb
}

fn write_asset(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn tilemap_pixel(tile: &[u8], cell: TilemapCell, x: usize, y: usize) -> u8 {
    let source_x = if cell.horizontal_flip { 7 - x } else { x };
    let source_y = if cell.vertical_flip { 7 - y } else { y };
    decode_4bpp_pixel(tile, source_x, source_y)
}

fn decode_4bpp_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn set_4bpp_pixel(tile: &mut [u8], x: usize, y: usize, value: u8) {
    let mask = 1 << (7 - x);
    for (plane, byte_index) in [y * 2, y * 2 + 1, 16 + y * 2, 16 + y * 2 + 1]
        .into_iter()
        .enumerate()
    {
        tile[byte_index] &= !mask;
        if value & (1 << plane) != 0 {
            tile[byte_index] |= mask;
        }
    }
}

fn decode_cgram(cram: &[u8]) -> Result<[[[u8; 3]; 16]; 8]> {
    if cram.len() < 256 {
        bail!("CGRAM is too short for eight BG palettes");
    }
    let mut palettes = [[[0u8; 3]; 16]; 8];
    for (palette_id, palette) in palettes.iter_mut().enumerate() {
        for (color_id, rgb) in palette.iter_mut().enumerate() {
            let offset = (palette_id * 16 + color_id) * 2;
            let color = u16::from_le_bytes([cram[offset], cram[offset + 1]]);
            *rgb = [
                expand_five_bits((color & 0x1F) as u8),
                expand_five_bits(((color >> 5) & 0x1F) as u8),
                expand_five_bits(((color >> 10) & 0x1F) as u8),
            ];
        }
    }
    Ok(palettes)
}

fn expand_five_bits(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

fn render_tilemap_from_chr(
    chr: &[u8],
    tilemap: &[TilemapCell],
    palettes: &[[[u8; 3]; 16]; 8],
    rows: usize,
) -> Result<Vec<u8>> {
    if tilemap.len() != TILEMAP_COLUMNS * TILEMAP_ROWS {
        bail!("tilemap preview has {} cells", tilemap.len());
    }
    if !chr.len().is_multiple_of(TILE_BYTES) {
        bail!("tilemap preview CHR is not an integer number of tiles");
    }
    let width = TILEMAP_COLUMNS * 8;
    let height = rows * 8;
    let mut rgb = vec![0u8; width * height * 3];
    for row in 0..rows {
        for column in 0..TILEMAP_COLUMNS {
            let cell = tilemap[row * TILEMAP_COLUMNS + column];
            let tile = chr
                .get(cell.tile_id * TILE_BYTES..(cell.tile_id + 1) * TILE_BYTES)
                .with_context(|| {
                    format!(
                        "tilemap preview tile ${:03X} is outside {}-tile CHR",
                        cell.tile_id,
                        chr.len() / TILE_BYTES
                    )
                })?;
            for y in 0..8 {
                for x in 0..8 {
                    let color = palettes[usize::from(cell.palette)]
                        [usize::from(tilemap_pixel(tile, cell, x, y))];
                    let pixel = ((row * 8 + y) * width + column * 8 + x) * 3;
                    rgb[pixel..pixel + 3].copy_from_slice(&color);
                }
            }
        }
    }
    Ok(rgb)
}

fn encode_rgb_png(width: usize, height: usize, rgb: &[u8]) -> Result<Vec<u8>> {
    if rgb.len() != width * height * 3 {
        bail!("RGB buffer length does not match {width}x{height}");
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgb)?;
    }
    Ok(output)
}

fn encode_rgba_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != width * height * 4 {
        bail!("RGBA buffer length does not match {width}x{height}");
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
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
    fn tilemap_attributes_select_palette_and_flip_axes() {
        let cell = parse_tilemap_cell(0xDC42);
        assert_eq!(cell.tile_id, 0x42);
        assert_eq!(cell.palette, 7);
        assert!(cell.horizontal_flip);
        assert!(cell.vertical_flip);
    }

    #[test]
    fn four_bitplanes_decode_one_pixel_value() {
        let mut tile = [0u8; TILE_BYTES];
        tile[0] = 0x80;
        tile[1] = 0x80;
        tile[16] = 0x80;
        tile[17] = 0x80;
        assert_eq!(decode_4bpp_pixel(&tile, 0, 0), 15);
        assert_eq!(decode_4bpp_pixel(&tile, 1, 0), 0);
    }

    #[test]
    fn separated_exclamation_parts_merge_into_one_glyph_band() {
        let width = 4;
        let height = 32;
        let mut mask = vec![false; width * height];
        for rows in [1..5, 10..14, 19..23, 25..27] {
            for y in rows {
                mask[y * width + 1] = true;
            }
        }
        assert_eq!(
            find_three_glyph_bands(&mask, width, height).unwrap(),
            [1..5, 10..14, 19..27]
        );
    }

    #[test]
    fn normalized_glyph_targets_use_only_verified_cells() {
        let width = 16;
        let height = 64;
        let mut source = vec![false; width * height];
        for (start, end) in [(0, 16), (24, 40), (48, 64)] {
            for y in start..end {
                for x in 0..width {
                    source[y * width + x] = true;
                }
            }
        }
        let bands = find_three_glyph_bands(&source, width, height).unwrap();
        let mut normalized = vec![false; NORMALIZED_WIDTH * NORMALIZED_HEIGHT];
        for (band, target) in bands.into_iter().zip(GLYPH_TARGETS) {
            let bounds = ink_bounds_in_rows(&source, width, band).unwrap();
            place_scaled_glyph(&source, width, bounds, &mut normalized, target, 0.5).unwrap();
        }
        assert!(normalized.iter().any(|pixel| *pixel));
        assert!(normalized.iter().enumerate().all(|(index, pixel)| {
            !*pixel || shock_pixel_is_owned(index % NORMALIZED_WIDTH, index / NORMALIZED_WIDTH)
        }));
    }

    #[test]
    #[ignore = "requires the Remix JP ROM and assets/ending_graphics/true_ending_shock.png"]
    fn approved_asset_rebuilds_only_the_measured_shock_stream() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let asset_path = Path::new("assets/ending_graphics/true_ending_shock.png");
        let rom = crate::test_input::read(rom_path);
        let asset = crate::test_input::read(asset_path);
        let (patched, report) = build_shock_text_kr(
            &rom,
            rom_path.display().to_string(),
            asset_path.display().to_string(),
            &asset,
            "test-output.sfc".to_owned(),
        )
        .unwrap();
        assert!(report.compression_roundtrip_matches);
        assert!(report.diff_confined_to_write_range);
        assert_eq!(report.changed_unowned_tiles, 0);
        assert!(report.patched_compressed_len <= report.original_compressed_len);
        assert!(
            rom.iter()
                .zip(&patched)
                .enumerate()
                .all(|(offset, (before, after))| before == after
                    || (SHOCK_STREAM_PC..SHOCK_STREAM_PC + EXPECTED_COMPRESSED_LEN)
                        .contains(&offset))
        );
    }

    #[test]
    #[ignore = "requires the Remix JP ROM and assets/fonts/maplestory_bold.ttf"]
    fn korean_giant_exclamation_rebuilds_only_the_measured_map_stream() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let ttf_path = Path::new("assets/fonts/maplestory_bold.ttf");
        let rom = crate::test_input::read(rom_path);
        let ttf = crate::test_input::read(ttf_path);
        let (patched, report) = build_giant_exclamation_kr(
            &rom,
            rom_path.display().to_string(),
            ttf_path.display().to_string(),
            &ttf,
            14.0,
            None,
            None,
            "test-output.sfc".to_owned(),
        )
        .unwrap();
        assert_eq!(report.text, "앗!");
        assert_eq!(report.source_foreground_cells, 89);
        assert!(report.korean_foreground_cells > 0);
        assert_eq!(report.changed_cells_outside_canvas, 0);
        assert!(report.compression_roundtrip_matches);
        assert!(report.diff_confined_to_write_range);
        assert!(report.patched_compressed_len <= report.original_compressed_len);
        assert!(
            rom.iter()
                .zip(&patched)
                .enumerate()
                .all(|(offset, (before, after))| before == after
                    || (GIANT_EXCLAMATION_MAP_STREAM_PC
                        ..GIANT_EXCLAMATION_MAP_STREAM_PC + GIANT_EXCLAMATION_MAP_COMPRESSED_LEN)
                        .contains(&offset))
        );
    }

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn giant_kaakun_surface_layout_round_trips_the_verified_source() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let rom = crate::test_input::read(rom_path);
        let source = verified_giant_kaakun_map(&rom).unwrap();
        let surface = decode_giant_kaakun_surface(&source.bytes).unwrap();
        assert_eq!(
            surface.iter().filter(|cell| **cell).count(),
            GIANT_KAAKUN_SOURCE_FOREGROUND_CELLS
        );
        assert_eq!(encode_giant_kaakun_surface(&surface).unwrap(), source.bytes);
    }

    #[test]
    #[ignore = "requires the Remix JP ROM and assets/fonts/galmuri11_bold.ttf"]
    fn korean_giant_kaakun_rebuilds_only_the_measured_map_stream() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let ttf_path = Path::new("assets/fonts/galmuri11_bold.ttf");
        let rom = crate::test_input::read(rom_path);
        let ttf = crate::test_input::read(ttf_path);
        let (patched, report) = build_giant_kaakun_kr(
            &rom,
            rom_path.display().to_string(),
            ttf_path.display().to_string(),
            &ttf,
            12.0,
            None,
            "test-output.sfc".to_owned(),
        )
        .unwrap();
        assert_eq!(report.text, "카~ 군~!");
        assert_eq!(report.source_foreground_cells, 250);
        assert!(report.korean_foreground_cells > 0);
        assert!(report.compression_roundtrip_matches);
        assert!(report.diff_confined_to_write_range);
        assert!(report.patched_compressed_len <= report.original_compressed_len);
        assert!(
            rom.iter()
                .zip(&patched)
                .enumerate()
                .all(|(offset, (before, after))| before == after
                    || (GIANT_KAAKUN_MAP_STREAM_PC
                        ..GIANT_KAAKUN_MAP_STREAM_PC + GIANT_KAAKUN_MAP_COMPRESSED_LEN)
                        .contains(&offset))
        );
    }
}
