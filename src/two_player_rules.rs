use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

macro_rules! hex {
    ($value:literal) => {{
        const LEN: usize = $value.len() / 2;
        const fn nibble(byte: u8) -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("invalid hex"),
            }
        }
        const fn decode(value: &str) -> [u8; LEN] {
            let bytes = value.as_bytes();
            let mut output = [0; LEN];
            let mut index = 0;
            while index < LEN {
                output[index] = nibble(bytes[index * 2]) * 16 + nibble(bytes[index * 2 + 1]);
                index += 1;
            }
            output
        }
        decode($value)
    }};
}

const POINTER_TABLE_PC: usize = 0x0D_4806;
const SOURCE_POINTERS: [u16; 6] = [0xC812, 0xC82A, 0xC843, 0xC85A, 0xC872, 0xC88A];
const SOURCE_BLOCKS: [&[u8]; 6] = [
    &hex!("ff020606fcfc1b1102ff0900201878fe1d784125fcfcff00"),
    &hex!("ff020806fcfcfcfeaffeb04d25ff0900204125fcfcfcfcff00"),
    &hex!("ff020806fcfcfc050f4d25ff0900204125fcfcfcfcff00"),
    &hex!("ff020806fcfcfcb609feaa0bff0900204125fcfcfcfcff00"),
    &hex!("ff020806fcfcfcb209feaa0bff0900204125fcfcfcfcff00"),
    &hex!("ff020606fcfcfc03442f3313ff0900204125fcfcfcff00"),
];
const SOURCE_BANK: u8 = 0x1A;
const RELOCATION_PC: usize = 0x0D_7D80;
const RELOCATION_END_PC: usize = 0x0D_8000;
const SPACE_CODE: u16 = 0x00FC;

const PROMPT_STREAM_PC: usize = 0x0E_7660;
const PROMPT_COMPRESSED_LEN: usize = 2_403;
const PROMPT_DECOMPRESSED_LEN: usize = 3_584;
const PROMPT_DECOMPRESSED_SHA256: &str =
    "b60e6ceb876f344e7b442c89fecb2f0b7b2a46713440c748aabc8fffc14295f1";
const PROMPT_TEXT: &str = "규칙을 선택해주세요";
const PROMPT_TILE_WIDTH: usize = 27;
const PROMPT_TILE_HEIGHT: usize = 3;
const PROMPT_SHEET_PATH: &str = "assets/menu_graphics/prompts/prompt_lettering_sheet.png";
const PROMPT_SHEET_ROW: usize = 1;

fn read_prompt_sheet() -> Result<Vec<u8>> {
    std::fs::read(PROMPT_SHEET_PATH)
        .with_context(|| format!("prompt lettering sheet {PROMPT_SHEET_PATH} is unavailable"))
}
const TILE_LEN: usize = 32;

const RULES: [(&str, &str); 6] = [
    ("보통", " 규칙이야"),
    ("점수 뿌요", "야"),
    ("강한 뿌요", "야"),
    ("여섯 개", " 지우기"),
    ("두 개", " 지우기"),
    ("규칙", " 수정"),
];

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub translated_rules: usize,
    pub rule_texts: Vec<String>,
    pub pointer_table_pc: String,
    pub source_blocks_pc: Vec<String>,
    pub relocation_pc: String,
    pub relocation_bytes_used: usize,
    pub prompt_text: String,
    pub prompt_lettering_sheet: String,
    pub prompt_lettering_sheet_sha256: String,
    pub prompt_stream_pc: String,
    pub prompt_stream_lorom: String,
    pub prompt_original_compressed_len: usize,
    pub prompt_patched_compressed_len: usize,
    pub prompt_changed_tiles: usize,
}

pub fn add_required_characters(characters: &mut BTreeSet<char>) {
    characters.extend(
        RULES
            .iter()
            .flat_map(|(accent, suffix)| accent.chars().chain(suffix.chars())),
    );
}

pub fn apply(
    source: &[u8],
    patched: &mut [u8],
    codes: &BTreeMap<char, u16>,
    allowed: &mut [bool],
) -> Result<BuildReport> {
    if source.len() != patched.len() || source.len() != allowed.len() {
        bail!("two-player rule buffers have different ROM lengths");
    }
    verify_sources(source)?;

    let blocks = RULES
        .iter()
        .map(|(accent, suffix)| encode_rule(accent, suffix, codes))
        .collect::<Result<Vec<_>>>()?;
    let relocation_bytes_used = blocks.iter().map(Vec::len).sum::<usize>();
    let relocation_end = RELOCATION_PC + relocation_bytes_used;
    if relocation_end > RELOCATION_END_PC {
        bail!("two-player rule text exceeds the measured Bank $1A tail");
    }
    for (label, rom) in [("source", source), ("current derivative", &*patched)] {
        if rom[RELOCATION_PC..relocation_end]
            .iter()
            .any(|byte| *byte != 0xFF)
        {
            bail!("two-player rule relocation is not empty in {label}");
        }
    }

    mark_allowed(allowed, POINTER_TABLE_PC, SOURCE_POINTERS.len() * 2)?;
    mark_allowed(allowed, RELOCATION_PC, relocation_bytes_used)?;
    let mut cursor = RELOCATION_PC;
    for (index, block) in blocks.iter().enumerate() {
        let (bank, address) = crate::rom::pc_to_lorom(cursor);
        if bank != SOURCE_BANK {
            bail!("two-player rule relocation left physical Bank $1A");
        }
        patched[cursor..cursor + block.len()].copy_from_slice(block);
        patched[POINTER_TABLE_PC + index * 2..POINTER_TABLE_PC + index * 2 + 2]
            .copy_from_slice(&address.to_le_bytes());
        cursor += block.len();
    }

    if source[PROMPT_STREAM_PC..PROMPT_STREAM_PC + PROMPT_COMPRESSED_LEN]
        != patched[PROMPT_STREAM_PC..PROMPT_STREAM_PC + PROMPT_COMPRESSED_LEN]
    {
        bail!("two-player prompt stream was already changed by another owner");
    }
    let prompt_block = crate::snes_lz::decompress(source, PROMPT_STREAM_PC)?;
    let original_prompt = prompt_block.bytes;
    let prompt_sheet = read_prompt_sheet()?;
    let canvas =
        crate::generated_lettering::prompt_canvas(&prompt_sheet, PROMPT_SHEET_ROW, PROMPT_TEXT)?;
    let patched_prompt = patch_prompt_tiles(&original_prompt, &canvas)?;
    let prompt_compressed = crate::snes_lz::compress(&patched_prompt);
    if prompt_compressed.len() > PROMPT_COMPRESSED_LEN {
        bail!(
            "Korean rules prompt compressed to {} bytes, exceeding its {}-byte slot",
            prompt_compressed.len(),
            PROMPT_COMPRESSED_LEN
        );
    }
    verify_compression(
        &prompt_compressed,
        &patched_prompt,
        "two-player rules prompt",
    )?;
    mark_allowed(allowed, PROMPT_STREAM_PC, PROMPT_COMPRESSED_LEN)?;
    patched[PROMPT_STREAM_PC..PROMPT_STREAM_PC + PROMPT_COMPRESSED_LEN].fill(0);
    patched[PROMPT_STREAM_PC..PROMPT_STREAM_PC + prompt_compressed.len()]
        .copy_from_slice(&prompt_compressed);

    let prompt_changed_tiles = original_prompt
        .as_chunks::<TILE_LEN>()
        .0
        .iter()
        .zip(patched_prompt.as_chunks::<TILE_LEN>().0)
        .filter(|(before, after)| before != after)
        .count();
    Ok(BuildReport {
        verdict: "Korean two-player rules prompt and six rule descriptions inserted".to_owned(),
        translated_rules: RULES.len(),
        rule_texts: RULES
            .iter()
            .map(|(accent, suffix)| format!("{accent}{suffix}"))
            .collect(),
        pointer_table_pc: format!("0x{POINTER_TABLE_PC:06X}"),
        source_blocks_pc: SOURCE_POINTERS
            .iter()
            .map(|pointer| {
                let pc = crate::rom::lorom_to_pc(SOURCE_BANK, *pointer)
                    .expect("verified two-player rule source pointer");
                format!("0x{pc:06X}")
            })
            .collect(),
        relocation_pc: format!("0x{RELOCATION_PC:06X}..0x{cursor:06X}"),
        relocation_bytes_used,
        prompt_text: PROMPT_TEXT.to_owned(),
        prompt_lettering_sheet: PROMPT_SHEET_PATH.to_owned(),
        prompt_lettering_sheet_sha256: format!("{:x}", Sha256::digest(&prompt_sheet)),
        prompt_stream_pc: format!("0x{PROMPT_STREAM_PC:06X}"),
        prompt_stream_lorom: crate::rom::format_lorom_addr(PROMPT_STREAM_PC),
        prompt_original_compressed_len: PROMPT_COMPRESSED_LEN,
        prompt_patched_compressed_len: prompt_compressed.len(),
        prompt_changed_tiles,
    })
}

fn verify_sources(rom: &[u8]) -> Result<()> {
    for (index, pointer) in SOURCE_POINTERS.iter().copied().enumerate() {
        let table_pc = POINTER_TABLE_PC + index * 2;
        if rom.get(table_pc..table_pc + 2) != Some(pointer.to_le_bytes().as_slice()) {
            bail!("Remix two-player rule pointer {index} differs from the measured spec");
        }
        let source_pc = crate::rom::lorom_to_pc(SOURCE_BANK, pointer)
            .context("invalid Remix two-player rule source pointer")?;
        let expected = SOURCE_BLOCKS[index];
        if rom.get(source_pc..source_pc + expected.len()) != Some(expected) {
            bail!("Remix two-player rule source block {index} differs from the measured spec");
        }
    }
    if rom[RELOCATION_PC..RELOCATION_END_PC]
        .iter()
        .any(|byte| *byte != 0xFF)
    {
        bail!("Remix Bank $1A tail is not the measured empty relocation range");
    }
    let prompt = crate::snes_lz::decompress(rom, PROMPT_STREAM_PC)?;
    if prompt.compressed_len != PROMPT_COMPRESSED_LEN
        || prompt.bytes.len() != PROMPT_DECOMPRESSED_LEN
        || format!("{:x}", Sha256::digest(&prompt.bytes)) != PROMPT_DECOMPRESSED_SHA256
    {
        bail!("Remix two-player rules prompt differs from the measured spec");
    }
    Ok(())
}

fn encode_rule(accent: &str, suffix: &str, codes: &BTreeMap<char, u16>) -> Result<Vec<u8>> {
    let visible_len = accent.chars().count() + suffix.chars().count();
    if visible_len > 13 {
        bail!("two-player rule line exceeds 13 glyph cells");
    }
    let left_pad = (13 - visible_len) / 2;
    let right_pad = 13 - visible_len - left_pad;
    let mut output = vec![0xFF, 0x02, 0x06, 0x06];
    output.extend(std::iter::repeat_n(SPACE_CODE as u8, left_pad));
    encode_text(&mut output, accent, codes)?;
    output.extend_from_slice(&[0xFF, 0x09, 0x00, 0x20]);
    encode_text(&mut output, suffix, codes)?;
    output.extend(std::iter::repeat_n(SPACE_CODE as u8, right_pad));
    output.extend_from_slice(&[0xFF, 0x00]);
    Ok(output)
}

fn encode_text(output: &mut Vec<u8>, text: &str, codes: &BTreeMap<char, u16>) -> Result<()> {
    for character in text.chars() {
        let code = *codes
            .get(&character)
            .with_context(|| format!("no shared-font code for {character:?}"))?;
        if code < 0x0100 {
            output.push(code as u8);
        } else if code < 0x0200 {
            output.extend_from_slice(&[0xFE, (code - 0x0100) as u8]);
        } else {
            bail!("shared-font code 0x{code:04X} is outside story encoding");
        }
    }
    Ok(())
}

fn prompt_tile(local_x: usize, local_y: usize) -> usize {
    if local_x < 12 {
        0x04 + local_y * 0x10 + local_x
    } else {
        0x40 + local_y * 0x10 + local_x - 12
    }
}

fn patch_prompt_tiles(original: &[u8], canvas: &[u8]) -> Result<Vec<u8>> {
    if original.len() != PROMPT_DECOMPRESSED_LEN {
        bail!("unexpected two-player rules prompt decoded length");
    }
    let mut patched = original.to_vec();
    for tile_y in 0..PROMPT_TILE_HEIGHT {
        for tile_x in 0..PROMPT_TILE_WIDTH {
            let tile = prompt_tile(tile_x, tile_y);
            patched[tile * TILE_LEN..(tile + 1) * TILE_LEN].fill(0);
        }
    }
    let canvas_width = PROMPT_TILE_WIDTH * 8;
    for y in 0..PROMPT_TILE_HEIGHT * 8 {
        for x in 0..canvas_width {
            let value = canvas[y * canvas_width + x];
            let tile = prompt_tile(x / 8, y / 8);
            set_4bpp_pixel(
                &mut patched[tile * TILE_LEN..(tile + 1) * TILE_LEN],
                x % 8,
                y % 8,
                value,
            );
        }
    }
    Ok(patched)
}

fn set_4bpp_pixel(tile: &mut [u8], x: usize, y: usize, value: u8) {
    let bit = 7 - x;
    let mask = !(1 << bit);
    for plane in 0..4 {
        let byte_offset = if plane < 2 {
            y * 2 + plane
        } else {
            16 + y * 2 + plane - 2
        };
        tile[byte_offset] = (tile[byte_offset] & mask) | (((value >> plane) & 1) << bit);
    }
}

fn verify_compression(compressed: &[u8], expected: &[u8], label: &str) -> Result<()> {
    let decoded = crate::snes_lz::decompress(compressed, 0)?;
    if decoded.bytes != expected || decoded.compressed_len != compressed.len() {
        bail!("{label} failed compression round-trip");
    }
    Ok(())
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    let end = start.checked_add(len).context("Expected Write overflow")?;
    allowed
        .get_mut(start..end)
        .with_context(|| format!("Expected Write 0x{start:06X}..0x{end:06X} outside ROM"))?
        .fill(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn remix_sources_match_the_measured_spec() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        verify_sources(&rom).unwrap();
    }

    #[test]
    fn prompt_tile_contract_owns_81_unique_tiles() {
        let tiles = (0..PROMPT_TILE_HEIGHT)
            .flat_map(|y| (0..PROMPT_TILE_WIDTH).map(move |x| prompt_tile(x, y)))
            .collect::<BTreeSet<_>>();
        assert_eq!(tiles.len(), 81);
        assert_eq!(tiles.iter().copied().min(), Some(0x04));
        assert_eq!(tiles.iter().copied().max(), Some(0x6E));
    }
}
