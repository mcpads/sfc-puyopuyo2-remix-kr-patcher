//! Game-over `コンティニューするかい？` line. The line is BG2 (Mode 1) palette 5
//! drawn from its own 39 tiles `0x36-0x5C` of the CHR stream at PC `0x1202D2`
//! through tilemap rows 23-24, columns 6-25 of the stream at PC `0x120B08`.
//! Nothing outside that 20x2 region references those tiles, so the importer
//! may repack the line into them and rewrite only the region's tilemap words.

use std::{io::Cursor, ops::Range, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CHR_STREAM: Range<usize> = 0x12_02D2..0x12_0B08;
pub const MAP_STREAM: Range<usize> = 0x12_0B08..0x12_0BFD;
const CHR_RAW_SHA256: &str = "d1c916752053258269cec78a73b68de8795b696baee79321e5825528278ce966";
const CHR_DECODED_SHA256: &str = "ae5fa169a3ee986973a28d42cc6d0c3904e836e2600774096c619c7f916da664";
const CHR_DECODED_LEN: usize = 2_976;
const MAP_RAW_SHA256: &str = "c7092adfd8e20d54cba39b2cf9ed21f7c4e3b523caed73b02103655b5c7c4aed";
const MAP_DECODED_SHA256: &str = "f891728d2c3b8a39fdba2e89f790152de911c3e14a02201de7b363efde3f75e8";
const MAP_DECODED_LEN: usize = 2_048;
const TILE_LEN: usize = 32;
const LINE_TILES: Range<usize> = 0x36..0x5D;
const REGION_ROWS: Range<usize> = 23..25;
const REGION_COLUMNS: Range<usize> = 6..26;
pub const ASSET_WIDTH: usize = 160;
pub const ASSET_HEIGHT: usize = 16;
/// Palette 5, priority 0, no flip.
const LINE_ATTRIBUTES: u16 = 5 << 10;
/// The line uses index 15 for its strokes and index 1 for the outline.
const STROKE_INDEX: u8 = 15;
const OUTLINE_INDEX: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub text: String,
    pub asset_path: String,
    pub asset_sha256: String,
    pub chr_stream: String,
    pub map_stream: String,
    pub original_chr_compressed_len: usize,
    pub patched_chr_compressed_len: usize,
    pub original_map_compressed_len: usize,
    pub patched_map_compressed_len: usize,
    pub used_line_tiles: usize,
    pub available_line_tiles: usize,
    pub blank_cells: usize,
    pub compression_roundtrip_matches: bool,
}

struct Plan {
    chr: Vec<u8>,
    map: Vec<u8>,
    used_line_tiles: usize,
    blank_cells: usize,
}

/// Rewrites the continue line of an already verified derivative. The caller
/// owns the checksum.
pub fn patch(source: &[u8], text: &str, asset_path: &Path) -> Result<(Vec<u8>, Report)> {
    let (chr, map) = verify_streams(source)?;
    let asset_bytes = std::fs::read(asset_path)
        .with_context(|| format!("read continue line asset {}", asset_path.display()))?;
    let indices = decode_asset(&asset_bytes)?;
    let plan = plan_line(&chr, &map, &indices)?;
    let chr_compressed = compress_within(&plan.chr, CHR_STREAM.len(), "continue CHR")?;
    let map_compressed = compress_within(&plan.map, MAP_STREAM.len(), "continue tilemap")?;
    let mut patched = source.to_vec();
    for (range, compressed) in [(CHR_STREAM, &chr_compressed), (MAP_STREAM, &map_compressed)] {
        patched[range.clone()].fill(0);
        patched[range.start..range.start + compressed.len()].copy_from_slice(compressed);
    }
    Ok((
        patched,
        Report {
            text: text.to_owned(),
            asset_path: asset_path.display().to_string(),
            asset_sha256: sha256(&asset_bytes),
            chr_stream: format!("0x{:06X}", CHR_STREAM.start),
            map_stream: format!("0x{:06X}", MAP_STREAM.start),
            original_chr_compressed_len: CHR_STREAM.len(),
            patched_chr_compressed_len: chr_compressed.len(),
            original_map_compressed_len: MAP_STREAM.len(),
            patched_map_compressed_len: map_compressed.len(),
            used_line_tiles: plan.used_line_tiles,
            available_line_tiles: LINE_TILES.len(),
            blank_cells: plan.blank_cells,
            compression_roundtrip_matches: true,
        },
    ))
}

pub fn registered_write(offset: usize) -> Option<&'static str> {
    if CHR_STREAM.contains(&offset) {
        Some("game-over continue CHR stream")
    } else if MAP_STREAM.contains(&offset) {
        Some("game-over continue tilemap stream")
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewReport {
    pub report: Report,
    pub preview_path: String,
    pub preview_sha256: String,
}

/// Renders the original line above the rebuilt one with a fixed display
/// palette (stroke white, outline black, index 13 grey); never writes a ROM.
pub fn preview(
    source: &[u8],
    text: &str,
    asset_path: &Path,
    preview_path: &Path,
) -> Result<PreviewReport> {
    let (chr, map) = verify_streams(source)?;
    let (patched, report) = patch(source, text, asset_path)?;
    let patched_chr = crate::snes_lz::decompress(&patched, CHR_STREAM.start)?.bytes;
    let patched_map = crate::snes_lz::decompress(&patched, MAP_STREAM.start)?.bytes;
    const SCALE: usize = 4;
    const GAP: usize = 4;
    let width = ASSET_WIDTH * SCALE;
    let height = (ASSET_HEIGHT * 2 + GAP) * SCALE;
    let mut pixels = vec![0u8; width * height * 4];
    for (panel, (chr, map)) in [(&chr, &map), (&patched_chr, &patched_map)]
        .into_iter()
        .enumerate()
    {
        let layer = render_region(chr, map)?;
        for y in 0..ASSET_HEIGHT {
            for x in 0..ASSET_WIDTH {
                let color = match layer[y * ASSET_WIDTH + x] {
                    0 => [48, 24, 72],
                    OUTLINE_INDEX => [0, 0, 0],
                    STROKE_INDEX => [255, 255, 255],
                    _ => [160, 160, 160],
                };
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        let row = (panel * (ASSET_HEIGHT + GAP) + y) * SCALE + dy;
                        let offset = (row * width + x * SCALE + dx) * 4;
                        pixels[offset..offset + 3].copy_from_slice(&color);
                        pixels[offset + 3] = 0xFF;
                    }
                }
            }
        }
    }
    let encoded = encode_png_rgba(width, height, &pixels)?;
    if let Some(parent) = preview_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(preview_path, &encoded)?;
    Ok(PreviewReport {
        report,
        preview_path: preview_path.display().to_string(),
        preview_sha256: sha256(&encoded),
    })
}

fn verify_streams(source: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut decoded = Vec::new();
    for (range, raw_sha256, decoded_sha256, decoded_len, name) in [
        (
            CHR_STREAM,
            CHR_RAW_SHA256,
            CHR_DECODED_SHA256,
            CHR_DECODED_LEN,
            "continue CHR",
        ),
        (
            MAP_STREAM,
            MAP_RAW_SHA256,
            MAP_DECODED_SHA256,
            MAP_DECODED_LEN,
            "continue tilemap",
        ),
    ] {
        let raw = source
            .get(range.clone())
            .with_context(|| format!("{name} stream is outside ROM"))?;
        let block = crate::snes_lz::decompress(source, range.start)?;
        if block.compressed_len != range.len()
            || block.bytes.len() != decoded_len
            || sha256(raw) != raw_sha256
            || sha256(&block.bytes) != decoded_sha256
        {
            bail!("{name} stream differs from the verified Remix contract");
        }
        decoded.push(block.bytes);
    }
    let map = decoded.pop().expect("two streams");
    let chr = decoded.pop().expect("two streams");
    if chr[..TILE_LEN].iter().any(|byte| *byte != 0) {
        bail!("continue CHR tile 0 is not blank");
    }
    for row in 0..32 {
        for column in 0..32 {
            let entry = map_entry(&map, column, row);
            let tile = usize::from(entry & 0x03FF);
            let in_region = REGION_ROWS.contains(&row) && REGION_COLUMNS.contains(&column);
            if in_region && (entry >> 10) & 7 != 5 {
                bail!("continue line cell {column},{row} is not palette 5");
            }
            if !in_region && LINE_TILES.contains(&tile) {
                bail!("continue line tile 0x{tile:02X} is also drawn at {column},{row}");
            }
        }
    }
    Ok((chr, map))
}

fn plan_line(chr: &[u8], map: &[u8], indices: &[u8]) -> Result<Plan> {
    let mut chr = chr.to_vec();
    let mut map = map.to_vec();
    for tile in LINE_TILES {
        chr[tile * TILE_LEN..(tile + 1) * TILE_LEN].fill(0);
    }
    let mut next_tile = LINE_TILES.start;
    let mut blank_cells = 0;
    for (cell_row, row) in REGION_ROWS.enumerate() {
        for (cell_column, column) in REGION_COLUMNS.enumerate() {
            let mut tile_bytes = [0u8; TILE_LEN];
            let mut blank = true;
            for y in 0..8 {
                for x in 0..8 {
                    let index = indices[(cell_row * 8 + y) * ASSET_WIDTH + cell_column * 8 + x];
                    if index != 0 {
                        blank = false;
                        set_4bpp_pixel(&mut tile_bytes, x, y, index);
                    }
                }
            }
            let tile = if blank {
                blank_cells += 1;
                0
            } else {
                if !LINE_TILES.contains(&next_tile) {
                    bail!("continue line needs more than {} tiles", LINE_TILES.len());
                }
                chr[next_tile * TILE_LEN..(next_tile + 1) * TILE_LEN].copy_from_slice(&tile_bytes);
                next_tile += 1;
                next_tile - 1
            };
            let offset = (row * 32 + column) * 2;
            map[offset..offset + 2].copy_from_slice(&(LINE_ATTRIBUTES | tile as u16).to_le_bytes());
        }
    }
    Ok(Plan {
        chr,
        map,
        used_line_tiles: next_tile - LINE_TILES.start,
        blank_cells,
    })
}

fn render_region(chr: &[u8], map: &[u8]) -> Result<Vec<u8>> {
    let mut layer = vec![0u8; ASSET_WIDTH * ASSET_HEIGHT];
    for (cell_row, row) in REGION_ROWS.enumerate() {
        for (cell_column, column) in REGION_COLUMNS.enumerate() {
            let entry = map_entry(map, column, row);
            let tile = usize::from(entry & 0x03FF);
            let bytes = chr
                .get(tile * TILE_LEN..(tile + 1) * TILE_LEN)
                .context("continue line tile is outside its CHR stream")?;
            for y in 0..8 {
                for x in 0..8 {
                    let source_x = if entry & 0x4000 != 0 { 7 - x } else { x };
                    let source_y = if entry & 0x8000 != 0 { 7 - y } else { y };
                    layer[(cell_row * 8 + y) * ASSET_WIDTH + cell_column * 8 + x] =
                        decode_4bpp_pixel(bytes, source_x, source_y);
                }
            }
        }
    }
    Ok(layer)
}

fn map_entry(map: &[u8], column: usize, row: usize) -> u16 {
    let offset = (row * 32 + column) * 2;
    u16::from_le_bytes([map[offset], map[offset + 1]])
}

/// Transparent pixels are background (0), white is the stroke and black the
/// outline; any other color fails.
fn decode_asset(encoded: &[u8]) -> Result<Vec<u8>> {
    let decoder = png::Decoder::new(Cursor::new(encoded));
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![0u8; reader.output_buffer_size().context("PNG is too large")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!("continue line PNG must be 8-bit RGBA");
    }
    if info.width as usize != ASSET_WIDTH || info.height as usize != ASSET_HEIGHT {
        bail!(
            "continue line PNG is {}x{}, expected {ASSET_WIDTH}x{ASSET_HEIGHT}",
            info.width,
            info.height
        );
    }
    pixels.truncate(info.buffer_size());
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(index, pixel)| match *pixel {
            [_, _, _, 0] => Ok(0),
            [0xFF, 0xFF, 0xFF, 0xFF] => Ok(STROKE_INDEX),
            [0, 0, 0, 0xFF] => Ok(OUTLINE_INDEX),
            _ => bail!(
                "continue line pixel {},{} is neither transparent, white nor black",
                index % ASSET_WIDTH,
                index / ASSET_WIDTH
            ),
        })
        .collect()
}

fn compress_within(decoded: &[u8], limit: usize, name: &str) -> Result<Vec<u8>> {
    let compressed = crate::snes_lz::compress(decoded);
    if compressed.len() > limit {
        bail!(
            "Korean {name} stream grew from {limit} to {} bytes",
            compressed.len()
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    if roundtrip.compressed_len != compressed.len() || roundtrip.bytes != decoded {
        bail!("Korean {name} stream failed compression round-trip");
    }
    Ok(compressed)
}

fn decode_4bpp_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn set_4bpp_pixel(tile: &mut [u8], x: usize, y: usize, value: u8) {
    let bit = 7 - x;
    let mask = !(1 << bit);
    for plane in 0..4 {
        let byte = if plane < 2 {
            y * 2 + plane
        } else {
            16 + y * 2 + plane - 2
        };
        tile[byte] = (tile[byte] & mask) | (((value >> plane) & 1) << bit);
    }
}

fn encode_png_rgba(width: usize, height: usize, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(pixels)?;
    }
    Ok(output)
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_region_and_tile_pool_fit_the_measured_contract() {
        assert_eq!(REGION_COLUMNS.len() * 8, ASSET_WIDTH);
        assert_eq!(REGION_ROWS.len() * 8, ASSET_HEIGHT);
        assert_eq!(LINE_TILES.len(), 39);
        const { assert!(LINE_TILES.end * TILE_LEN <= CHR_DECODED_LEN) };
        assert_eq!(CHR_STREAM.end, MAP_STREAM.start);
        assert_eq!(CHR_STREAM.len(), 2_102);
        assert_eq!(MAP_STREAM.len(), 245);
    }

    #[test]
    fn plan_packs_non_blank_cells_into_the_line_tiles_only() {
        let chr = vec![0u8; CHR_DECODED_LEN];
        let map = vec![0u8; MAP_DECODED_LEN];
        let mut indices = vec![0u8; ASSET_WIDTH * ASSET_HEIGHT];
        indices[0] = STROKE_INDEX;
        indices[ASSET_WIDTH * 9 + 17] = OUTLINE_INDEX;
        let plan = plan_line(&chr, &map, &indices).expect("plan");
        assert_eq!(plan.used_line_tiles, 2);
        assert_eq!(plan.blank_cells, 38);
        let layer = render_region(&plan.chr, &plan.map).expect("render");
        assert_eq!(layer, indices);
        for (index, (before, after)) in chr.iter().zip(&plan.chr).enumerate() {
            if before != after {
                assert!(LINE_TILES.contains(&(index / TILE_LEN)));
            }
        }
    }
}
