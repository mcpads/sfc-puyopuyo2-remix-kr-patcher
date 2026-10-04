use std::{collections::BTreeSet, ops::Range};

use anyhow::{Context, Result, bail};
use serde::Serialize;

pub(crate) const STREAM_PC: usize = 0x03_8DF8;
pub(crate) const STREAM_CAPACITY: usize = 5_948;
// The CPU reads this stream through the bank $07 asset table as $87:$8DF8.
const STREAM_SOURCE_BANK: u8 = 0x87;
const STREAM_SOURCE_ADDRESS: u16 = 0x8DF8;
// Remix moved the generic decompressor entry from base-game PC 0x003B91 to
// 0x003D52; its first 32 bytes are identical. The entry reads its source
// pointer from direct page $68-$6A and its first control byte from $6B.
pub(crate) const DECOMPRESS_ENTRY: Range<usize> = 0x00_3D52..0x00_3D57;
const DECOMPRESS_ENTRY_ORIGINAL: [u8; 5] = [0xA0, 0x00, 0x00, 0xA5, 0x6B];
// Free-space candidates assigned to this stage; both are 0xFF in the source ROM.
pub(crate) const RELOCATED_STREAM_SLOT: Range<usize> = 0x15_C000..0x16_0000;
pub(crate) const HOOK_SLOT: Range<usize> = 0x16_6000..0x16_6100;
const DECOMPRESSED_LEN: usize = 8_192;
const TILE_LEN: usize = 32;
const FRAME_LEN: usize = 17;
// The original kanji labels are white ink (index 15) with a dark drop shadow
// (index 1) to the right and below.
const SHADOW_INDEX: u8 = 1;
const INK_INDEX: u8 = 15;
// The Remix frame list keeps the base game's five label frames byte-exact, but
// moved from PC 0x00BF22 to 0x00C054 and appended three more frames.
pub(crate) const FRAME_PCS: [usize; 5] = [0x00_C054, 0x00_C065, 0x00_C076, 0x00_C087, 0x00_C098];
const ORIGINAL_FRAME_TILES: [[u8; 2]; 5] = [[0, 2], [2, 4], [6, 8], [8, 4], [0, 8]];
const KOREAN_FRAME_TILES: [[u8; 2]; 5] = [
    [0x00, 0x02],
    [0x04, 0x06],
    [0x08, 0x5A],
    [0x6E, 0x7A],
    [0x9A, 0x02],
];
// Remix-only frames whose consumer has not been observed. They stay unchanged,
// so the build reports that they now point at Korean glyph slots.
const REMIX_ONLY_FRAME_PCS: [usize; 3] = [0x00_C0A9, 0x00_C0BA, 0x00_C0CB];
const REMIX_ONLY_FRAME_TILES: [[u8; 2]; 3] = [[2, 0], [6, 0], [8, 0]];
const KOREAN_GLYPH_SLOTS: [(char, usize); 9] = [
    ('단', 0),
    ('맛', 2),
    ('순', 4),
    ('함', 6),
    ('보', 8),
    ('통', 0x5A),
    ('매', 0x6E),
    ('움', 0x7A),
    ('불', 0x9A),
];
const EXTRA_GLYPH_BASES: [usize; 3] = [0x6E, 0x7A, 0x9A];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub glyph_source: String,
    pub stream_pc: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub stream_capacity: usize,
    pub relocated: bool,
    pub write_pc: String,
    pub decompressor_hook: Option<String>,
    pub decompressed_len: usize,
    pub glyph_slots: Vec<String>,
    pub frame_sequence: Vec<String>,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub changed_frame_bytes: usize,
    pub unmapped_remix_only_frames: Vec<String>,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_expected_ranges: bool,
}

/// Replaces the five curry-spiciness labels in the shared gameplay OBJ stream.
/// The stream is rewritten in place; the Remix build has no relocation slot for it.
pub(crate) fn build_kr(rom: &[u8]) -> Result<(Vec<u8>, BuildReport)> {
    let block = crate::snes_lz::decompress(rom, STREAM_PC)?;
    if block.compressed_len > STREAM_CAPACITY || block.bytes.len() != DECOMPRESSED_LEN {
        bail!(
            "Tokoton OBJ stream differs from the verified slot: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    for (pc, tiles) in FRAME_PCS
        .into_iter()
        .zip(ORIGINAL_FRAME_TILES)
        .chain(REMIX_ONLY_FRAME_PCS.into_iter().zip(REMIX_ONLY_FRAME_TILES))
    {
        if rom.get(pc..pc + FRAME_LEN) != Some(frame_bytes(tiles).as_slice()) {
            bail!("Tokoton frame at PC 0x{pc:06X} differs from the verified Remix frame list");
        }
    }

    let mut decoded = block.bytes.clone();
    for base in EXTRA_GLYPH_BASES {
        for tile in [base, base + 1, base + 16, base + 17] {
            let start = tile * TILE_LEN;
            if block.bytes.get(start..start + TILE_LEN) != Some(&[0u8; TILE_LEN]) {
                bail!("Tokoton extra glyph group ${base:02X} is not blank in the source stream");
            }
        }
    }
    for (character, base) in KOREAN_GLYPH_SLOTS {
        let pixels = render_korean_glyph(character)?;
        let tiles = [(0, 0), (8, 0), (0, 8), (8, 8)]
            .map(|(x, y)| crate::effect_glyphs::encode_4bpp(&pixels, 16, x, y));
        replace_large_sprite(&mut decoded, base, &tiles)?;
    }
    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    let target_tiles = KOREAN_GLYPH_SLOTS
        .iter()
        .flat_map(|(_, base)| [*base, *base + 1, *base + 16, *base + 17])
        .collect::<BTreeSet<_>>();
    for (index, (before, after)) in block.bytes.iter().zip(&decoded).enumerate() {
        if before != after && !target_tiles.contains(&(index / TILE_LEN)) {
            bail!("unexplained Tokoton decoded change at 0x{index:04X}");
        }
    }

    let compressed = crate::snes_lz::compress(&decoded);
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == decoded;
    if !compression_roundtrip_matches {
        bail!("Korean Tokoton compression round-trip failed");
    }

    let mut patched = rom.to_vec();
    let relocated = compressed.len() > STREAM_CAPACITY;
    let mut hook_len = 0;
    let write_pc = if relocated {
        if compressed.len() > RELOCATED_STREAM_SLOT.len() {
            bail!(
                "Korean Tokoton stream grew to {} bytes, beyond its {}-byte relocation slot",
                compressed.len(),
                RELOCATED_STREAM_SLOT.len()
            );
        }
        if rom.get(DECOMPRESS_ENTRY.clone()) != Some(DECOMPRESS_ENTRY_ORIGINAL.as_slice()) {
            bail!(
                "Remix decompressor entry at PC 0x{:06X} differs from the verified bytes",
                DECOMPRESS_ENTRY.start
            );
        }
        let hook = decompression_redirect_hook()?;
        hook_len = hook.len();
        for (name, range) in [
            (
                "relocated Tokoton stream",
                RELOCATED_STREAM_SLOT.start..RELOCATED_STREAM_SLOT.start + compressed.len(),
            ),
            (
                "decompressor hook",
                HOOK_SLOT.start..HOOK_SLOT.start + hook.len(),
            ),
        ] {
            if !rom
                .get(range.clone())
                .with_context(|| format!("{name} range is outside ROM"))?
                .iter()
                .all(|byte| *byte == 0xFF)
            {
                bail!(
                    "{name} range 0x{:06X}..0x{:06X} is not 0xFF free space",
                    range.start,
                    range.end
                );
            }
        }
        if hook.len() > HOOK_SLOT.len() {
            bail!(
                "decompressor hook needs {} bytes, beyond its {}-byte slot",
                hook.len(),
                HOOK_SLOT.len()
            );
        }
        patched[DECOMPRESS_ENTRY.clone()].copy_from_slice(&decompress_entry_call()?);
        patched[HOOK_SLOT.start..HOOK_SLOT.start + hook.len()].copy_from_slice(&hook);
        patched[RELOCATED_STREAM_SLOT.start..RELOCATED_STREAM_SLOT.start + compressed.len()]
            .copy_from_slice(&compressed);
        RELOCATED_STREAM_SLOT.start
    } else {
        patched
            .get_mut(STREAM_PC..STREAM_PC + compressed.len())
            .context("Tokoton compressed write is outside ROM")?
            .copy_from_slice(&compressed);
        STREAM_PC
    };
    let installed = crate::snes_lz::decompress(&patched, write_pc)?;
    if installed.bytes != decoded || installed.compressed_len != compressed.len() {
        bail!("installed Korean Tokoton stream does not re-extract from PC 0x{write_pc:06X}");
    }
    for (pc, korean) in FRAME_PCS.into_iter().zip(KOREAN_FRAME_TILES) {
        patched
            .get_mut(pc..pc + FRAME_LEN)
            .with_context(|| format!("Tokoton frame write at PC 0x{pc:06X} is outside ROM"))?
            .copy_from_slice(&frame_bytes(korean));
    }
    let changed_frame_bytes = FRAME_PCS
        .iter()
        .flat_map(|pc| *pc..*pc + FRAME_LEN)
        .filter(|index| rom[*index] != patched[*index])
        .count();
    let diff_confined_to_expected_ranges = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(index, (before, after))| before == after || registered_write(index).is_some());
    if !diff_confined_to_expected_ranges {
        bail!("Tokoton patch changed bytes outside its expected write ranges");
    }

    let report = BuildReport {
        verdict: "Tokoton curry-spiciness labels patched to Korean five-stage labels".to_owned(),
        glyph_source: "src/effect_glyphs.rs 16px masks with a drop shadow".to_owned(),
        stream_pc: format!("0x{STREAM_PC:06X}"),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        stream_capacity: STREAM_CAPACITY,
        relocated,
        write_pc: format!("0x{write_pc:06X}"),
        decompressor_hook: relocated.then(|| {
            format!(
                "entry 0x{:06X} -> JSL hook 0x{:06X} ({hook_len} bytes)",
                DECOMPRESS_ENTRY.start, HOOK_SLOT.start
            )
        }),
        decompressed_len: decoded.len(),
        glyph_slots: KOREAN_GLYPH_SLOTS
            .iter()
            .map(|(character, base)| format!("${base:02X}={character}"))
            .collect(),
        frame_sequence: vec![
            "$00/$02 단맛".to_owned(),
            "$04/$06 순함".to_owned(),
            "$08/$5A 보통".to_owned(),
            "$6E/$7A 매움".to_owned(),
            "$9A/$02 불맛".to_owned(),
        ],
        changed_tiles: target_tiles.len(),
        changed_decompressed_bytes,
        changed_frame_bytes,
        unmapped_remix_only_frames: REMIX_ONLY_FRAME_PCS
            .iter()
            .zip(REMIX_ONLY_FRAME_TILES)
            .map(|(pc, tiles)| format!("0x{pc:06X} tiles ${:02X}/${:02X}", tiles[0], tiles[1]))
            .collect(),
        compression_roundtrip_matches,
        diff_confined_to_expected_ranges,
    };
    Ok((patched, report))
}

/// The five rewritten label frames form one contiguous PC range.
fn frame_range() -> Range<usize> {
    FRAME_PCS[0]..FRAME_PCS[4] + FRAME_LEN
}

/// Every PC range this stage may change, including the relocation reserve.
pub(crate) fn registered_write(offset: usize) -> Option<&'static str> {
    if (STREAM_PC..STREAM_PC + STREAM_CAPACITY).contains(&offset) {
        Some("shared gameplay OBJ stream")
    } else if frame_range().contains(&offset) {
        Some("Tokoton difficulty frame data")
    } else if DECOMPRESS_ENTRY.contains(&offset) {
        Some("decompressor entry call")
    } else if HOOK_SLOT.contains(&offset) {
        Some("decompressor source-redirect hook")
    } else if RELOCATED_STREAM_SLOT.contains(&offset) {
        Some("relocated shared gameplay OBJ stream")
    } else {
        None
    }
}

fn fastrom_long_address(pc: usize) -> u32 {
    let (bank, address) = crate::rom::pc_to_lorom(pc);
    (u32::from(bank | 0x80) << 16) | u32::from(address)
}

fn decompress_entry_call() -> Result<Vec<u8>> {
    use crate::snes_asm::Inst;
    crate::snes_asm::assemble(&[Inst::Jsl(fastrom_long_address(HOOK_SLOT.start)), Inst::Nop])
        .map_err(anyhow::Error::msg)
}

/// Redirects only the $87:$8DF8 source pointer and then replays the two
/// instructions the entry call displaced. Runs with M=8 and X=16 like the entry.
fn decompression_redirect_hook() -> Result<Vec<u8>> {
    use crate::snes_asm::Inst::*;
    let replacement = fastrom_long_address(RELOCATED_STREAM_SLOT.start);
    let [source_low, source_high] = STREAM_SOURCE_ADDRESS.to_le_bytes();
    let (_, origin) = crate::rom::pc_to_lorom(HOOK_SLOT.start);
    crate::snes_asm::assemble_at(
        origin,
        &[
            LdyImm16(0),
            LdaDp(0x6A),
            CmpImm8(STREAM_SOURCE_BANK),
            Bne("resume"),
            LdaDp(0x69),
            CmpImm8(source_high),
            Bne("resume"),
            LdaDp(0x68),
            CmpImm8(source_low),
            Bne("resume"),
            LdaImm8(replacement as u8),
            StaDp(0x68),
            LdaImm8((replacement >> 8) as u8),
            StaDp(0x69),
            LdaImm8((replacement >> 16) as u8),
            StaDp(0x6A),
            Label("resume"),
            LdaDp(0x6B),
            Rtl,
        ],
    )
    .map_err(anyhow::Error::msg)
}

fn frame_bytes(tiles: [u8; 2]) -> [u8; FRAME_LEN] {
    [
        0x02, 0x02, 0xF0, 0xFF, 0xF8, 0xFF, tiles[0], 0x33, 0x02, 0x02, 0x00, 0x00, 0xF8, 0xFF,
        tiles[1], 0x33, 0x02,
    ]
}

/// Centres a 16px effect mask in its 16x16 sprite, leaving the last row and
/// column for the drop shadow.
fn render_korean_glyph(character: char) -> Result<Vec<u8>> {
    let mask = crate::effect_glyphs::glyph(character)?;
    let mut body = vec![false; 256];
    let x0 = (15 - crate::effect_glyphs::width(mask)) / 2;
    let y0 = (15 - mask.len()) / 2;
    crate::effect_glyphs::stamp(&mut body, 16, 16, mask, x0, y0, 1)?;
    Ok(crate::effect_glyphs::drop_shadow(
        &body,
        16,
        16,
        INK_INDEX,
        SHADOW_INDEX,
    ))
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
            .with_context(|| format!("Tokoton glyph tile ${tile:02X} is outside stream"))?
            .copy_from_slice(replacement);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean_large_sprite_slots_do_not_overlap() {
        let mut owned = BTreeSet::new();
        for (_, base) in KOREAN_GLYPH_SLOTS {
            for tile in [base, base + 1, base + 16, base + 17] {
                assert!(owned.insert(tile), "large-sprite tile ${tile:02X} overlaps");
            }
        }
    }

    #[test]
    fn korean_slots_avoid_chain_and_offset_glyphs() {
        // 연쇄 uses $2A/$2B/$AF/$B0 and 상쇄 uses the $C8/$CA large sprites.
        let foreign = [
            0x2A, 0x2B, 0xAF, 0xB0, 0xC8, 0xC9, 0xD8, 0xD9, 0xCA, 0xCB, 0xDA, 0xDB,
        ];
        for (_, base) in KOREAN_GLYPH_SLOTS {
            for tile in [base, base + 1, base + 16, base + 17] {
                assert!(
                    !foreign.contains(&tile),
                    "tile ${tile:02X} is owned elsewhere"
                );
            }
        }
    }

    #[test]
    fn redirect_hook_encodes_the_measured_source_pointer() {
        let hook = decompression_redirect_hook().unwrap();
        assert_eq!(&hook[..3], &[0xA0, 0x00, 0x00]);
        assert_eq!(&hook[3..7], &[0xA5, 0x6A, 0xC9, 0x87]);
        assert_eq!(&hook[hook.len() - 3..], &[0xA5, 0x6B, 0x6B]);
        assert!(hook.len() <= HOOK_SLOT.len());
        assert_eq!(
            decompress_entry_call().unwrap(),
            [0x22, 0x00, 0xE0, 0xAC, 0xEA]
        );
    }

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn remix_source_builds_korean_labels() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let (patched, report) = build_kr(&rom).unwrap();
        assert_eq!(patched.len(), rom.len());
        assert!(report.diff_confined_to_expected_ranges);
        let installed = crate::snes_lz::decompress(
            &patched,
            if report.relocated {
                RELOCATED_STREAM_SLOT.start
            } else {
                STREAM_PC
            },
        )
        .unwrap();
        assert_eq!(installed.bytes.len(), DECOMPRESSED_LEN);
        assert_eq!(report.unmapped_remix_only_frames.len(), 3);
    }
}
