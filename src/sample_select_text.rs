use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::font_gen::StoryFontRasterizer;

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const GALMURI11_SHA256: &str = "2c709890595668f7bdb6df408420fda957dde0288e95b31a1cc17a2ab98b4b4f";
const FONT_BANK: u8 = 0x08;
const BASE_POINTER_PC: usize = 0x04002E;
const EXT_POINTER_PC: usize = 0x040036;
const NEXT_ASSET_POINTER_PC: usize = 0x040040;
const ORIGINAL_BASE_SLOTS: usize = 208;
const ORIGINAL_EXTENDED_SLOTS: usize = 224;
const FINAL_EXTENDED_SLOTS: usize = 224;
const CHECKSUM_PC: usize = 0x007FDC;

pub(crate) const POINTER_PC: usize = 0x0053DC;
pub(crate) const RELOCATION_PC: usize = 0x006152;
pub(crate) const RELOCATION_CAPACITY: usize = 0x40;
const SOURCE_PC: usize = 0x00557F;
const SOURCE_LEN: usize = 37;
const SOURCE_SHA256: &str = "40efc13a203851c6578ea9d3e35cc88e691ea9ee2e28258ce058a499feb1851f";
const ORIGINAL_POINTER: [u8; 2] = [0x7F, 0xD5];
const RELOCATED_POINTER: [u8; 2] = [0x52, 0xE1];

pub(crate) const KOREAN_LABELS: [&str; 4] = ["3연쇄", "4연쇄", "5연쇄", "끝내기"];
const ADDED_CHARACTERS: [char; 5] = ['연', '쇄', '끝', '내', '기'];
const ADDED_CODES: [u16; 5] = [0x01A0, 0x01A1, 0x01DD, 0x01DE, 0x01DF];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub output_path: String,
    pub output_sha256: String,
    pub ttf_path: String,
    pub ttf_sha256: String,
    pub font_px: f32,
    pub labels: Vec<String>,
    pub source_text_pc: String,
    pub source_text_lorom: String,
    pub pointer_pc: String,
    pub pointer_lorom: String,
    pub relocation_pc: String,
    pub relocation_lorom: String,
    pub encoded_text_bytes: usize,
    pub original_extended_slots: usize,
    pub final_extended_slots: usize,
    pub added_glyphs: Vec<String>,
    pub extended_font_compressed_len: usize,
    pub font_region_headroom: usize,
    pub checksum_hex: String,
    pub diff_confined_to_registered_writes: bool,
}

pub fn build_poc(
    source: &[u8],
    source_path: String,
    ttf_path: String,
    ttf_data: &[u8],
    font_px: f32,
    output_path: String,
) -> Result<(Vec<u8>, BuildReport)> {
    build_impl(
        source,
        source_path,
        ttf_path,
        ttf_data,
        font_px,
        output_path,
        true,
    )
}

pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    ttf_path: String,
    ttf_data: &[u8],
    font_px: f32,
    output_path: String,
) -> Result<(Vec<u8>, BuildReport)> {
    build_impl(
        source,
        source_path,
        ttf_path,
        ttf_data,
        font_px,
        output_path,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_impl(
    source: &[u8],
    source_path: String,
    ttf_path: String,
    ttf_data: &[u8],
    font_px: f32,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, BuildReport)> {
    let source_sha256 = sha256(source);
    if require_original_identity && source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    let ttf_sha256 = sha256(ttf_data);
    if ttf_sha256 != GALMURI11_SHA256 {
        bail!("Galmuri11 TTF SHA-256 differs from the Super Puyo Puyo 2 font input");
    }
    verify_text_source(source)?;

    let base_source = crate::story_font::source_pc(source, BASE_POINTER_PC, FONT_BANK)?;
    let extended_source = crate::story_font::source_pc(source, EXT_POINTER_PC, FONT_BANK)?;
    let font_end = crate::story_font::source_pc(source, NEXT_ASSET_POINTER_PC, FONT_BANK)?;
    let base = crate::story_font::decode_at(source, base_source)?;
    let extended = crate::story_font::decode_at(source, extended_source)?;
    if base.bytes.len() != ORIGINAL_BASE_SLOTS * crate::story_font::GLYPH_LEN
        || extended.bytes.len() != ORIGINAL_EXTENDED_SLOTS * crate::story_font::GLYPH_LEN
        || base_source + base.compressed_len != extended_source
        || extended_source + extended.compressed_len != font_end
    {
        bail!("Remix shared-font streams differ from the verified source boundary");
    }

    let rasterizer = StoryFontRasterizer::new(ttf_data, font_px)?;
    let added_glyphs = ADDED_CHARACTERS
        .into_iter()
        .map(|character| {
            rasterizer
                .render(character)
                .with_context(|| format!("render sample-select glyph {character:?}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut extended_bytes = extended.bytes.clone();
    for ((code, glyph), character) in ADDED_CODES
        .into_iter()
        .zip(&added_glyphs)
        .zip(ADDED_CHARACTERS)
    {
        let index = usize::from(code - 0x0100);
        let original = crate::story_font::glyph_tiles(&extended, index)?;
        if code >= 0x01DD && original.iter().any(|byte| *byte != 0) {
            bail!("verified blank shared-font code 0x{code:04X} for {character} is occupied");
        }
        if code <= 0x01A1 && original.iter().all(|byte| *byte == 0) {
            bail!("runtime-confirmed 連/鎖 shared-font code 0x{code:04X} is blank");
        }
        crate::story_font::replace_glyph(&mut extended_bytes, index, glyph)?;
    }
    if extended_bytes.len() != FINAL_EXTENDED_SLOTS * crate::story_font::GLYPH_LEN {
        bail!("sample-select extended font slot count drifted");
    }
    let extended_compressed = crate::snes_lz::compress(&extended_bytes);
    verify_compression(
        &extended_compressed,
        &extended_bytes,
        "sample-select extended font",
    )?;
    let font_region_len = font_end - extended_source;
    if extended_compressed.len() > font_region_len {
        bail!("sample-select font extension exceeds the verified font region");
    }

    let codes = ADDED_CHARACTERS
        .into_iter()
        .zip(ADDED_CODES)
        .collect::<BTreeMap<_, _>>();
    let encoded = encode_labels(|character| match character {
        '3' => Ok(0x00B3),
        '4' => Ok(0x00B4),
        '5' => Ok(0x00B5),
        _ => codes
            .get(&character)
            .copied()
            .with_context(|| format!("sample-select character has no code: {character:?}")),
    })?;
    verify_relocation(source, encoded.len())?;

    let mut patched = source.to_vec();
    let mut allowed = vec![false; source.len()];
    mark_allowed(&mut allowed, extended_source, font_region_len)?;
    patched[extended_source..font_end].fill(0xFF);
    patched[extended_source..extended_source + extended_compressed.len()]
        .copy_from_slice(&extended_compressed);

    mark_allowed(&mut allowed, POINTER_PC, 2)?;
    mark_allowed(&mut allowed, RELOCATION_PC, encoded.len())?;
    write_text_patch(&mut patched, &encoded)?;

    mark_allowed(&mut allowed, CHECKSUM_PC, 4)?;
    let checksum = crate::rom::fix_checksum(&mut patched)?;
    let diff_confined_to_registered_writes = source
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(index, (before, after))| before == after || allowed[index]);
    if !diff_confined_to_registered_writes {
        bail!("sample-select build changed bytes outside Expected Write ranges");
    }

    let (relocation_bank, relocation_address) = crate::rom::pc_to_lorom(RELOCATION_PC);
    let output_sha256 = sha256(&patched);
    Ok((
        patched,
        BuildReport {
            verdict: "Super Puyo Puyo 2 shared-font method inserted 3/4/5연쇄 and 끝내기"
                .to_owned(),
            source_path,
            source_sha256,
            output_path,
            output_sha256,
            ttf_path,
            ttf_sha256,
            font_px,
            labels: KOREAN_LABELS
                .iter()
                .map(|label| (*label).to_owned())
                .collect(),
            source_text_pc: format!("0x{SOURCE_PC:06X}"),
            source_text_lorom: crate::rom::format_lorom_addr(SOURCE_PC),
            pointer_pc: format!("0x{POINTER_PC:06X}"),
            pointer_lorom: crate::rom::format_lorom_addr(POINTER_PC),
            relocation_pc: format!(
                "0x{RELOCATION_PC:06X}-0x{:06X}",
                RELOCATION_PC + encoded.len() - 1
            ),
            relocation_lorom: format!(
                "${relocation_bank:02X}:${relocation_address:04X}-${relocation_bank:02X}:${:04X}",
                relocation_address as usize + encoded.len() - 1
            ),
            encoded_text_bytes: encoded.len(),
            original_extended_slots: ORIGINAL_EXTENDED_SLOTS,
            final_extended_slots: FINAL_EXTENDED_SLOTS,
            added_glyphs: ADDED_CHARACTERS
                .into_iter()
                .zip(ADDED_CODES)
                .map(|(character, code)| format!("0x{code:04X}={character}"))
                .collect(),
            extended_font_compressed_len: extended_compressed.len(),
            font_region_headroom: font_region_len - extended_compressed.len(),
            checksum_hex: format!("0x{checksum:04X}"),
            diff_confined_to_registered_writes,
        },
    ))
}

pub(crate) fn add_required_characters(characters: &mut std::collections::BTreeSet<char>) {
    for label in KOREAN_LABELS {
        characters.extend(label.chars());
    }
}

pub(crate) fn encode_with_story_codes(codes: &BTreeMap<char, u16>) -> Result<Vec<u8>> {
    encode_labels(|character| {
        codes
            .get(&character)
            .copied()
            .with_context(|| format!("story encoding lacks sample-select character {character:?}"))
    })
}

pub(crate) fn verify_text_source(rom: &[u8]) -> Result<()> {
    if rom.get(POINTER_PC..POINTER_PC + 2) != Some(ORIGINAL_POINTER.as_slice()) {
        bail!("sample-select pointer differs from the runtime-confirmed Remix source");
    }
    let source = rom
        .get(SOURCE_PC..SOURCE_PC + SOURCE_LEN)
        .context("sample-select source text is outside ROM")?;
    if sha256(source) != SOURCE_SHA256 {
        bail!("sample-select source text hash differs from the runtime-confirmed block");
    }
    let parsed = crate::story_codec::parse(rom, SOURCE_PC)?;
    if parsed.consumed_len != SOURCE_LEN {
        bail!(
            "sample-select source text boundary differs from the verified block: {} != {SOURCE_LEN}",
            parsed.consumed_len
        );
    }
    Ok(())
}

pub(crate) fn verify_relocation(rom: &[u8], encoded_len: usize) -> Result<()> {
    if encoded_len > RELOCATION_CAPACITY {
        bail!("sample-select text exceeds its Bank $00 relocation budget");
    }
    if rom
        .get(RELOCATION_PC..RELOCATION_PC + encoded_len)
        .context("sample-select relocation range is outside ROM")?
        .iter()
        .any(|byte| *byte != 0xFF)
    {
        bail!("sample-select relocation range is not empty");
    }
    Ok(())
}

pub(crate) fn write_text_patch(rom: &mut [u8], encoded: &[u8]) -> Result<()> {
    rom.get_mut(POINTER_PC..POINTER_PC + 2)
        .context("sample-select pointer is outside ROM")?
        .copy_from_slice(&RELOCATED_POINTER);
    rom.get_mut(RELOCATION_PC..RELOCATION_PC + encoded.len())
        .context("sample-select relocation is outside ROM")?
        .copy_from_slice(encoded);
    Ok(())
}

fn encode_labels(mut code_for: impl FnMut(char) -> Result<u16>) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for (row, label) in (2u8..=5).zip(KOREAN_LABELS) {
        output.extend_from_slice(&[0xFF, 0x02, 0x2A, row]);
        for character in label.chars() {
            let code = code_for(character)?;
            if code < 0x0100 {
                output.push(code as u8);
            } else if code < 0x0200 {
                output.extend_from_slice(&[0xFE, code as u8]);
            } else {
                bail!("sample-select code exceeds the FE-prefixed font space");
            }
        }
    }
    output.extend_from_slice(&[0xFF, 0x00]);
    Ok(output)
}

fn verify_compression(compressed: &[u8], expected: &[u8], label: &str) -> Result<()> {
    let decoded = crate::snes_lz::decompress(compressed, 0)?;
    if decoded.bytes != expected || decoded.compressed_len != compressed.len() {
        bail!("{label} failed compression round-trip");
    }
    Ok(())
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    allowed
        .get_mut(start..start + len)
        .with_context(|| format!("Expected Write 0x{start:06X}+0x{len:X} is outside ROM"))?
        .fill(true);
    Ok(())
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_labels_fit_the_relocated_block() {
        let codes = ADDED_CHARACTERS
            .into_iter()
            .zip(ADDED_CODES)
            .collect::<BTreeMap<_, _>>();
        let encoded = encode_labels(|character| match character {
            '3' => Ok(0x00B3),
            '4' => Ok(0x00B4),
            '5' => Ok(0x00B5),
            _ => Ok(codes[&character]),
        })
        .unwrap();
        assert_eq!(encoded.len(), 39);
        assert_eq!(
            &encoded[..9],
            &[0xFF, 0x02, 0x2A, 0x02, 0xB3, 0xFE, 0xA0, 0xFE, 0xA1]
        );
        assert_eq!(&encoded[37..], &[0xFF, 0x00]);
    }
}
