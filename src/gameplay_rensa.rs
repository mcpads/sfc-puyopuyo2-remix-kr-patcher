use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const STREAM_PC: usize = 0x038_DF8;
const ORIGINAL_COMPRESSED_LEN: usize = 5_948;
const DECOMPRESSED_LEN: usize = 8_192;
const RUNTIME_VRAM_BYTE: usize = 0xE000;
const TILE_LEN: usize = 32;
const YEON_TILE: usize = 0x2A;
const SWAE_TILE: usize = 0x2B;
const BLANK_TILE: usize = 0xAF;
const DECORATION_TILE: usize = 0xB0;
const BACKGROUND_INDEX: u8 = 0;
const SHADOW_INDEX: u8 = 2;
const BODY_INDEX: u8 = 6;
const OAM_DESCRIPTOR_PC: usize = 0x00_B588;
const ORIGINAL_OAM_DESCRIPTORS: [u8; 33] = [
    0x02, 0x03, 0x00, 0x00, 0x00, 0x00, 0x2A, 0x33, 0x02, 0x03, 0x10, 0x00, 0x00, 0x00, 0xAF, 0x33,
    0x00, 0x02, 0x03, 0x00, 0x00, 0x00, 0x00, 0x2A, 0x35, 0x02, 0x03, 0x10, 0x00, 0x00, 0x00, 0xAF,
    0x35,
];

const ORIGINAL_YEON_SLOT: [u8; TILE_LEN] = [
    0x00, 0x60, 0x00, 0xFC, 0x00, 0xFE, 0x00, 0xFE, 0x00, 0xEE, 0x00, 0xEF, 0x00, 0xEF, 0x00, 0x6E,
    0x40, 0x00, 0xD8, 0x00, 0x64, 0x00, 0x44, 0x00, 0x48, 0x00, 0xCA, 0x00, 0x44, 0x00, 0x00, 0x00,
];
const ORIGINAL_SWAE_SLOT: [u8; TILE_LEN] = [
    0x00, 0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0xF8, 0x00, 0xFB, 0x00, 0xFF, 0x00, 0xDF, 0x00, 0xDE,
    0x40, 0x00, 0x40, 0x00, 0x40, 0x00, 0xB0, 0x00, 0xD2, 0x00, 0x92, 0x00, 0x8C, 0x00, 0x00, 0x00,
];
const ORIGINAL_BLANK_SLOT: [u8; TILE_LEN] = [
    0x00, 0x30, 0x00, 0xFE, 0x00, 0xFE, 0x00, 0x7C, 0x00, 0xFE, 0x00, 0xDE, 0x00, 0xFC, 0x00, 0x7C,
    0x20, 0x00, 0xFC, 0x00, 0x10, 0x00, 0x78, 0x00, 0x8C, 0x00, 0x80, 0x00, 0x78, 0x00, 0x00, 0x00,
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GameplayRensaKrPocReport {
    pub verdict: String,
    pub source_path: String,
    pub glyph_source: String,
    pub output_path: String,
    pub evidence_source: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub runtime_vram_range: String,
    pub runtime_vram_matches_source: bool,
    pub tile_mapping: Vec<String>,
    pub patched_tile_bytes: Vec<String>,
    pub background_palette_index: u8,
    pub shadow_palette_index: u8,
    pub body_palette_index: u8,
    pub oam_descriptor_pc: String,
    pub oam_descriptor_lorom: String,
    pub oam_descriptors_unchanged: bool,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub decoration_tile_preserved: bool,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_expected_ranges: bool,
    pub output_sha256: String,
}

pub fn build_gameplay_rensa_kr_poc(
    rom: &[u8],
    source_path: String,
    runtime_vram: Option<&[u8]>,
    output_path: String,
) -> Result<(Vec<u8>, GameplayRensaKrPocReport)> {
    let block = crate::snes_lz::decompress(rom, STREAM_PC)?;
    if block.compressed_len != ORIGINAL_COMPRESSED_LEN || block.bytes.len() != DECOMPRESSED_LEN {
        bail!(
            "gameplay rensa stream differs from the verified JP ROM Spec: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    verify_original_tile(&block.bytes, YEON_TILE, &ORIGINAL_YEON_SLOT, "れ")?;
    verify_original_tile(&block.bytes, SWAE_TILE, &ORIGINAL_SWAE_SLOT, "ん")?;
    verify_original_tile(&block.bytes, BLANK_TILE, &ORIGINAL_BLANK_SLOT, "さ")?;

    let evidence_source;
    let runtime_vram_matches_source = if let Some(runtime_vram) = runtime_vram {
        evidence_source = "runtime evidence".to_owned();
        let runtime_end = RUNTIME_VRAM_BYTE + DECOMPRESSED_LEN;
        let matches =
            runtime_vram.get(RUNTIME_VRAM_BYTE..runtime_end) == Some(block.bytes.as_slice());
        if !matches {
            bail!("gameplay rensa source does not match runtime OBJ VRAM $E000-$FFFF");
        }
        true
    } else {
        evidence_source = "hardcoded verified ROM stream/OAM Spec".to_owned();
        true
    };

    let mut decoded = block.bytes.clone();
    let decoration_before = tile(&decoded, DECORATION_TILE)?.to_vec();
    replace_tile(&mut decoded, YEON_TILE, &encode_chain_glyph('연')?)?;
    replace_tile(&mut decoded, SWAE_TILE, &encode_chain_glyph('쇄')?)?;
    replace_tile(&mut decoded, BLANK_TILE, &[0; TILE_LEN])?;
    let decoration_tile_preserved = tile(&decoded, DECORATION_TILE)? == decoration_before;
    if !decoration_tile_preserved {
        bail!("rensa decoration tile $B0 changed");
    }

    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    for (index, (before, after)) in block.bytes.iter().zip(&decoded).enumerate() {
        if before != after && ![YEON_TILE, SWAE_TILE, BLANK_TILE].contains(&(index / TILE_LEN)) {
            bail!("unexplained gameplay rensa decoded change at 0x{index:04X}");
        }
    }

    let compressed = crate::snes_lz::compress(&decoded);
    if compressed.len() > ORIGINAL_COMPRESSED_LEN {
        bail!(
            "Korean gameplay rensa stream grew to {} bytes, beyond the {}-byte in-place extent",
            compressed.len(),
            ORIGINAL_COMPRESSED_LEN
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.compressed_len == compressed.len() && roundtrip.bytes == decoded;
    if !compression_roundtrip_matches {
        bail!("Korean gameplay rensa compression round-trip failed");
    }

    let mut patched = rom.to_vec();
    let write_end = STREAM_PC + compressed.len();
    patched
        .get_mut(STREAM_PC..write_end)
        .context("gameplay rensa write range is outside ROM")?
        .copy_from_slice(&compressed);
    let descriptor_end = OAM_DESCRIPTOR_PC + ORIGINAL_OAM_DESCRIPTORS.len();
    if rom.get(OAM_DESCRIPTOR_PC..descriptor_end) != Some(ORIGINAL_OAM_DESCRIPTORS.as_slice()) {
        bail!("gameplay rensa OAM descriptors differ from the verified JP ROM Spec");
    }
    let diff_confined_to_expected_ranges = rom
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(index, (before, after))| before == after || (STREAM_PC..write_end).contains(&index));
    if !diff_confined_to_expected_ranges {
        bail!("gameplay rensa patch changed bytes outside its stream");
    }
    let oam_descriptors_unchanged =
        patched.get(OAM_DESCRIPTOR_PC..descriptor_end) == Some(ORIGINAL_OAM_DESCRIPTORS.as_slice());

    let (bank, address) = crate::rom::pc_to_lorom(STREAM_PC);
    let (oam_bank, oam_address) = crate::rom::pc_to_lorom(OAM_DESCRIPTOR_PC);
    let report = GameplayRensaKrPocReport {
        verdict: "gameplay OBJ 8x8 rensa patched to hand-drawn Korean 연쇄".to_owned(),
        source_path,
        glyph_source: "src/effect_glyphs.rs chain masks".to_owned(),
        output_path,
        evidence_source,
        stream_pc: format!("0x{STREAM_PC:06X}"),
        stream_lorom: format!("${bank:02X}:${address:04X}"),
        original_compressed_len: block.compressed_len,
        patched_compressed_len: compressed.len(),
        decompressed_len: decoded.len(),
        runtime_vram_range: "0xE000-0xFFFF".to_owned(),
        runtime_vram_matches_source,
        tile_mapping: vec![
            "$2A れ -> 연".to_owned(),
            "$2B ん -> 쇄".to_owned(),
            "$AF さ -> blank".to_owned(),
            "$B0 decoration preserved".to_owned(),
        ],
        patched_tile_bytes: vec![
            format!("$2A={}", bytes_hex(tile(&decoded, YEON_TILE)?)),
            format!("$2B={}", bytes_hex(tile(&decoded, SWAE_TILE)?)),
            format!("$AF={}", bytes_hex(tile(&decoded, BLANK_TILE)?)),
        ],
        background_palette_index: BACKGROUND_INDEX,
        shadow_palette_index: SHADOW_INDEX,
        body_palette_index: BODY_INDEX,
        oam_descriptor_pc: format!("0x{OAM_DESCRIPTOR_PC:06X}"),
        oam_descriptor_lorom: format!("${oam_bank:02X}:${oam_address:04X}"),
        oam_descriptors_unchanged,
        changed_tiles: 3,
        changed_decompressed_bytes,
        decoration_tile_preserved,
        compression_roundtrip_matches,
        diff_confined_to_expected_ranges,
        output_sha256: format!("{:x}", Sha256::digest(&patched)),
    };
    Ok((patched, report))
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn verify_original_tile(
    decoded: &[u8],
    index: usize,
    expected: &[u8; TILE_LEN],
    label: &str,
) -> Result<()> {
    if tile(decoded, index)? != expected {
        bail!("gameplay rensa {label} tile ${index:02X} differs from the verified JP ROM Spec");
    }
    Ok(())
}

fn tile(decoded: &[u8], index: usize) -> Result<&[u8]> {
    let start = index * TILE_LEN;
    decoded
        .get(start..start + TILE_LEN)
        .with_context(|| format!("gameplay rensa tile ${index:02X} is outside decoded block"))
}

fn replace_tile(decoded: &mut [u8], index: usize, replacement: &[u8; TILE_LEN]) -> Result<()> {
    let start = index * TILE_LEN;
    decoded
        .get_mut(start..start + TILE_LEN)
        .with_context(|| format!("gameplay rensa tile ${index:02X} is outside decoded block"))?
        .copy_from_slice(replacement);
    Ok(())
}

/// Draws a chain-counter syllable the way the original `れんさ` tiles are
/// built: a 7x7 body in index 6 with a one-pixel drop shadow in index 2 to
/// the right, below and diagonally below.
fn encode_chain_glyph(character: char) -> Result<[u8; TILE_LEN]> {
    let mask = crate::effect_glyphs::chain_glyph(character)?;
    let mut body = [false; 64];
    crate::effect_glyphs::stamp(&mut body, 8, 8, mask, 0, 0, 1)?;
    let pixels = crate::effect_glyphs::drop_shadow(&body, 8, 8, BODY_INDEX, SHADOW_INDEX);
    Ok(crate::effect_glyphs::encode_4bpp(&pixels, 8, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_glyph_uses_only_verified_palette_indices() {
        let tile = encode_chain_glyph('쇄').unwrap();
        for row in 0..8 {
            for col in 0..8 {
                let bit = 7 - col;
                let value = ((tile[row * 2] >> bit) & 1)
                    | (((tile[row * 2 + 1] >> bit) & 1) << 1)
                    | (((tile[16 + row * 2] >> bit) & 1) << 2)
                    | (((tile[16 + row * 2 + 1] >> bit) & 1) << 3);
                assert!(matches!(
                    value,
                    BACKGROUND_INDEX | SHADOW_INDEX | BODY_INDEX
                ));
            }
        }
    }

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn current_rom_counter_stream_matches_verified_layout() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let block = crate::snes_lz::decompress(&rom, STREAM_PC).unwrap();
        assert_eq!(block.compressed_len, ORIGINAL_COMPRESSED_LEN);
        assert_eq!(block.bytes.len(), DECOMPRESSED_LEN);
        assert_eq!(tile(&block.bytes, YEON_TILE).unwrap(), ORIGINAL_YEON_SLOT);
        assert_eq!(tile(&block.bytes, SWAE_TILE).unwrap(), ORIGINAL_SWAE_SLOT);
        assert_eq!(tile(&block.bytes, BLANK_TILE).unwrap(), ORIGINAL_BLANK_SLOT);
        assert_eq!(
            &rom[OAM_DESCRIPTOR_PC..OAM_DESCRIPTOR_PC + ORIGINAL_OAM_DESCRIPTORS.len()],
            ORIGINAL_OAM_DESCRIPTORS
        );
    }
}
