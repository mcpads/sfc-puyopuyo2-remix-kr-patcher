use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct FontCompareReport {
    pub base_path: String,
    pub target_path: String,
    pub base_font: StreamReport,
    pub target_font: StreamReport,
    pub base_font_decoded_different_bytes: usize,
    pub base_font_changed_glyph_codes: Vec<String>,
    pub base_extended_font: StreamReport,
    pub target_extended_font: StreamReport,
    pub extended_raw_equal: bool,
    pub extended_decoded_equal: bool,
}

#[derive(Debug, Serialize)]
pub struct StreamReport {
    pub pc: usize,
    pub pc_hex: String,
    pub lorom: String,
    pub compressed_len: usize,
    pub decompressed_len: usize,
    pub recompress_round_trip: bool,
}

pub fn compare(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
    base_font_pc: usize,
) -> Result<FontCompareReport> {
    let base_font = crate::snes_lz::decompress(base, base_font_pc)
        .context("base story font candidate did not decompress")?;
    let target_font = crate::snes_lz::decompress(target, base_font_pc)
        .context("target story font candidate did not decompress at the same entry address")?;
    let base_extended_pc = base_font_pc + base_font.compressed_len;
    let target_extended_pc = base_font_pc + target_font.compressed_len;
    let base_extended = crate::snes_lz::decompress(base, base_extended_pc)
        .context("base extended story font candidate did not decompress")?;
    let target_extended = crate::snes_lz::decompress(target, target_extended_pc)
        .context("target extended story font candidate did not decompress")?;
    let base_extended_raw = base
        .get(base_extended_pc..base_extended_pc + base_extended.compressed_len)
        .context("base extended story font raw range is outside ROM")?;
    let target_extended_raw = target
        .get(target_extended_pc..target_extended_pc + target_extended.compressed_len)
        .context("target extended story font raw range is outside ROM")?;

    Ok(FontCompareReport {
        base_path: base_path.display().to_string(),
        target_path: target_path.display().to_string(),
        base_font_decoded_different_bytes: different_bytes(&base_font.bytes, &target_font.bytes),
        base_font_changed_glyph_codes: changed_glyph_codes(&base_font.bytes, &target_font.bytes),
        base_font: stream_report(base_font_pc, &base_font),
        target_font: stream_report(base_font_pc, &target_font),
        base_extended_font: stream_report(base_extended_pc, &base_extended),
        target_extended_font: stream_report(target_extended_pc, &target_extended),
        extended_raw_equal: base_extended_raw == target_extended_raw,
        extended_decoded_equal: base_extended.bytes == target_extended.bytes,
    })
}

fn changed_glyph_codes(left: &[u8], right: &[u8]) -> Vec<String> {
    const GLYPH_LEN: usize = 32;
    const GROUP_GLYPHS: usize = 8;
    const GROUP_LEN: usize = GLYPH_LEN * GROUP_GLYPHS;
    const HALF_LEN: usize = 16;
    let glyph_count = left.len().min(right.len()) / GLYPH_LEN;
    (0..glyph_count)
        .filter(|glyph| {
            let group = glyph / GROUP_GLYPHS;
            let in_group = glyph % GROUP_GLYPHS;
            let top = group * GROUP_LEN + in_group * HALF_LEN;
            let bottom = top + GROUP_GLYPHS * HALF_LEN;
            left[top..top + HALF_LEN] != right[top..top + HALF_LEN]
                || left[bottom..bottom + HALF_LEN] != right[bottom..bottom + HALF_LEN]
        })
        .map(|glyph| format!("0x{glyph:04X}"))
        .collect()
}

fn stream_report(pc: usize, block: &crate::snes_lz::LzBlock) -> StreamReport {
    let recompressed = crate::snes_lz::compress(&block.bytes);
    let recompress_round_trip = crate::snes_lz::decompress(&recompressed, 0)
        .is_ok_and(|decoded| decoded.bytes == block.bytes);
    let (bank, address) = crate::rom::pc_to_lorom(pc);
    StreamReport {
        pc,
        pc_hex: format!("0x{pc:06X}"),
        lorom: format!("${bank:02X}:${address:04X}"),
        compressed_len: block.compressed_len,
        decompressed_len: block.bytes.len(),
        recompress_round_trip,
    }
}

fn different_bytes(left: &[u8], right: &[u8]) -> usize {
    left.iter().zip(right).filter(|(a, b)| a != b).count() + left.len().abs_diff(right.len())
}
