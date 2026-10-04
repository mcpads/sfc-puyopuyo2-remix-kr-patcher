use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const STREAM_PC: usize = 0x0D_B322;
const EXPECTED_COMPRESSED_LEN: usize = 2_131;
const EXPECTED_DECOMPRESSED_LEN: usize = 3_072;
const EXPECTED_RAW_SHA256: &str =
    "47a4a1306e589a1c55be63a688ba0f1e4671a4992d4c704cd189b06fb053f2a5";
const EXPECTED_DECODED_SHA256: &str =
    "c87e5d7d149d3de18a72e79d955ba1386b8f9e874b37271c6ac28c0b606690f8";
const CHECKSUM_PC: usize = 0x7FDC;

const VRAM_LEN: usize = 64 * 1024;
const CGRAM_LEN: usize = 512;
const TILE_LEN: usize = 32;
const TILEMAP_BASE_BYTE: usize = 0xD000;
const TILEMAP_WIDTH: usize = 64;
const TILEMAP_HEIGHT: usize = 32;
const FIRST_TILE: usize = 0x1A0;
const VRAM_BYTE: usize = FIRST_TILE * TILE_LEN;
const TILE_X: usize = 2;
const TILE_Y: usize = 1;
const TILE_WIDTH: usize = 27;
const TILE_HEIGHT: usize = 3;
const TEXT: &str = "모드를 선택해주세요";
const PROMPT_SHEET_PATH: &str = "assets/menu_graphics/prompts/prompt_lettering_sheet.png";
const PROMPT_SHEET_ROW: usize = 0;

fn read_prompt_sheet() -> Result<Vec<u8>> {
    fs::read(PROMPT_SHEET_PATH)
        .with_context(|| format!("prompt lettering sheet {PROMPT_SHEET_PATH} is unavailable"))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModePromptPocReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub lettering_sheet: String,
    pub lettering_sheet_sha256: String,
    pub output_path: String,
    pub runtime_dump: String,
    pub text: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub write_range: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub palette_indices: Vec<u8>,
    pub runtime_vram_matches_original: bool,
    pub patched_runtime_dump: Option<String>,
    pub patched_runtime_vram_matches: Option<bool>,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_registered_writes: bool,
    pub checksum_hex: String,
    pub output_sha256: String,
}

#[allow(clippy::too_many_arguments)]
pub fn build_poc(
    source: &[u8],
    source_path: String,
    runtime_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, ModePromptPocReport)> {
    build_impl(
        source,
        source_path,
        runtime_dump,
        patched_runtime_dump,
        output_path,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    runtime_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, ModePromptPocReport)> {
    build_impl(
        source,
        source_path,
        runtime_dump,
        patched_runtime_dump,
        output_path,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_impl(
    source: &[u8],
    source_path: String,
    runtime_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, ModePromptPocReport)> {
    let source_sha256 = sha256(source);
    if require_original_identity && source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    let block = crate::snes_lz::decompress(source, STREAM_PC)?;
    if block.compressed_len != EXPECTED_COMPRESSED_LEN
        || block.bytes.len() != EXPECTED_DECOMPRESSED_LEN
    {
        bail!(
            "Remix mode-prompt stream differs from the verified contract: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let raw = source
        .get(STREAM_PC..STREAM_PC + block.compressed_len)
        .context("verified Remix mode-prompt stream is outside ROM")?;
    if sha256(raw) != EXPECTED_RAW_SHA256 || sha256(&block.bytes) != EXPECTED_DECODED_SHA256 {
        bail!("Remix mode-prompt stream hashes differ from the verified relocated resource");
    }

    let runtime = read_runtime_dump(runtime_dump)?;
    let runtime_vram_matches_original =
        runtime.vram.get(VRAM_BYTE..VRAM_BYTE + block.bytes.len()) == Some(block.bytes.as_slice());
    if !runtime_vram_matches_original {
        bail!("runtime VRAM does not contain the verified mode-prompt CHR at 0x{VRAM_BYTE:04X}");
    }
    let prompt_sheet = read_prompt_sheet()?;
    let canvas = crate::generated_lettering::prompt_canvas(&prompt_sheet, PROMPT_SHEET_ROW, TEXT)?;
    let palette_indices = canvas
        .iter()
        .copied()
        .filter(|&index| index != 0)
        .collect::<BTreeSet<_>>();
    for &index in &palette_indices {
        let color = cgram_color(&runtime.cgram, 3 * 16 + usize::from(index))?;
        if color == cgram_color(&runtime.cgram, 3 * 16)? {
            bail!("mode-prompt palette index {index} equals transparent color 0");
        }
    }

    let target_x = TILE_X..TILE_X + TILE_WIDTH;
    let target_y = TILE_Y..TILE_Y + TILE_HEIGHT;
    let mut referenced_tiles = BTreeSet::new();
    for tile_y in 0..TILEMAP_HEIGHT {
        for tile_x in 0..TILEMAP_WIDTH {
            let entry = tilemap_entry_at(&runtime.vram, tile_x, tile_y)?;
            if (FIRST_TILE..FIRST_TILE + EXPECTED_DECOMPRESSED_LEN / TILE_LEN).contains(&entry.tile)
            {
                if !target_x.contains(&tile_x) || !target_y.contains(&tile_y) {
                    bail!(
                        "mode-prompt tile 0x{:03X} is referenced outside the verified 27x3 target at {tile_x},{tile_y}",
                        entry.tile
                    );
                }
                referenced_tiles.insert(entry.tile);
            }
        }
    }
    if referenced_tiles.len() != TILE_WIDTH * TILE_HEIGHT {
        bail!(
            "mode-prompt tilemap uses {} unique tiles, expected {}",
            referenced_tiles.len(),
            TILE_WIDTH * TILE_HEIGHT
        );
    }

    let canvas_width = TILE_WIDTH * 8;
    let canvas_height = TILE_HEIGHT * 8;
    let mut patched_decoded = vec![0u8; block.bytes.len()];
    for local_y in 0..canvas_height {
        for local_x in 0..canvas_width {
            let tile_x = TILE_X + local_x / 8;
            let tile_y = TILE_Y + local_y / 8;
            let entry = tilemap_entry_at(&runtime.vram, tile_x, tile_y)?;
            if entry.palette != 3 {
                bail!(
                    "mode-prompt tilemap {tile_x},{tile_y} uses palette {}, expected 3",
                    entry.palette
                );
            }
            let relative_tile = entry
                .tile
                .checked_sub(FIRST_TILE)
                .context("mode-prompt tile precedes its CHR stream")?;
            if relative_tile * TILE_LEN >= patched_decoded.len() {
                bail!(
                    "mode-prompt tile 0x{:03X} is outside its CHR stream",
                    entry.tile
                );
            }
            let source_x = if entry.hflip {
                7 - local_x % 8
            } else {
                local_x % 8
            };
            let source_y = if entry.vflip {
                7 - local_y % 8
            } else {
                local_y % 8
            };
            let pixel = local_y * canvas_width + local_x;
            let value = canvas[pixel];
            let tile_start = relative_tile * TILE_LEN;
            set_4bpp_pixel(
                &mut patched_decoded[tile_start..tile_start + TILE_LEN],
                source_x,
                source_y,
                value,
            );
        }
    }

    let changed_tiles = block
        .bytes
        .as_chunks::<TILE_LEN>()
        .0
        .iter()
        .zip(patched_decoded.as_chunks::<TILE_LEN>().0)
        .filter(|(before, after)| before != after)
        .count();
    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&patched_decoded)
        .filter(|(before, after)| before != after)
        .count();
    let compressed = crate::snes_lz::compress(&patched_decoded);
    if compressed.len() > block.compressed_len {
        bail!(
            "Korean mode-prompt CHR grew from {} to {} bytes; refusing in-place overflow",
            block.compressed_len,
            compressed.len()
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.bytes == patched_decoded && roundtrip.compressed_len == compressed.len();
    if !compression_roundtrip_matches {
        bail!("recompressed Remix mode-prompt CHR failed its round trip");
    }

    let patched_runtime_vram_matches = patched_runtime_dump
        .map(|dump| {
            let path = dump.join("vram.bin");
            let vram = fs::read(&path)
                .with_context(|| format!("read patched mode-prompt VRAM {}", path.display()))?;
            if vram.len() != VRAM_LEN {
                bail!("patched mode-prompt VRAM must be {VRAM_LEN} bytes");
            }
            let matches = vram.get(VRAM_BYTE..VRAM_BYTE + patched_decoded.len())
                == Some(patched_decoded.as_slice());
            if !matches {
                bail!("patched runtime VRAM does not contain built mode-prompt CHR at 0x{VRAM_BYTE:04X}");
            }
            Ok(matches)
        })
        .transpose()?;

    let write_start = STREAM_PC;
    let write_end = write_start + block.compressed_len;
    let mut patched = source.to_vec();
    patched[write_start..write_end].fill(0);
    patched[write_start..write_start + compressed.len()].copy_from_slice(&compressed);
    let checksum = crate::rom::fix_checksum(&mut patched)?;
    let diff_confined_to_registered_writes =
        source
            .iter()
            .zip(&patched)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || (write_start..write_end).contains(&offset)
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&offset)
            });
    if !diff_confined_to_registered_writes {
        bail!("mode-prompt PoC diff escaped its registered stream and checksum writes");
    }
    let output_sha256 = sha256(&patched);
    Ok((
        patched,
        ModePromptPocReport {
            verdict: "generated Korean prompt lettering inserted into the verified relocated Remix CHR stream"
                .to_owned(),
            source_path,
            source_sha256,
            lettering_sheet: PROMPT_SHEET_PATH.to_owned(),
            lettering_sheet_sha256: sha256(&prompt_sheet),
            output_path,
            runtime_dump: runtime_dump.display().to_string(),
            text: TEXT.to_owned(),
            stream_pc: format!("0x{STREAM_PC:06X}"),
            stream_lorom: "$1B:$B322".to_owned(),
            write_range: format!("0x{write_start:06X}..0x{write_end:06X}"),
            original_compressed_len: block.compressed_len,
            patched_compressed_len: compressed.len(),
            decompressed_len: patched_decoded.len(),
            changed_tiles,
            changed_decompressed_bytes,
            palette_indices: palette_indices.into_iter().collect(),
            runtime_vram_matches_original,
            patched_runtime_dump: patched_runtime_dump.map(|path| path.display().to_string()),
            patched_runtime_vram_matches,
            compression_roundtrip_matches,
            diff_confined_to_registered_writes,
            checksum_hex: format!("0x{checksum:04X}"),
            output_sha256,
        },
    ))
}

struct RuntimeDump {
    vram: Vec<u8>,
    cgram: Vec<u8>,
}

fn read_runtime_dump(path: &Path) -> Result<RuntimeDump> {
    let vram_path = path.join("vram.bin");
    let cgram_path = path.join("cram.bin");
    let state_path = path.join("state.json");
    let vram = fs::read(&vram_path)
        .with_context(|| format!("read mode-prompt runtime VRAM {}", vram_path.display()))?;
    let cgram = fs::read(&cgram_path)
        .with_context(|| format!("read mode-prompt runtime CGRAM {}", cgram_path.display()))?;
    if vram.len() != VRAM_LEN || cgram.len() != CGRAM_LEN {
        bail!(
            "mode-prompt runtime dump sizes differ from SNES VRAM/CGRAM: {} / {}",
            vram.len(),
            cgram.len()
        );
    }
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).with_context(|| {
            format!("read mode-prompt runtime state {}", state_path.display())
        })?)?;
    if state_u64(&state, "ppu.bgMode")? != 1
        || state_u64(&state, "ppu.layers[0].chrAddress")? != 0
        || state_u64(&state, "ppu.layers[0].tilemapAddress")?.checked_mul(2)
            != Some(TILEMAP_BASE_BYTE as u64)
        || !state_bool(&state, "ppu.layers[0].doubleWidth")?
        || state_bool(&state, "ppu.layers[0].doubleHeight")?
    {
        bail!("mode-prompt BG1 geometry differs from verified Mode 1 / 64x32 layout");
    }
    Ok(RuntimeDump { vram, cgram })
}

fn state_u64(state: &serde_json::Value, key: &str) -> Result<u64> {
    state
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .with_context(|| format!("runtime state is missing numeric {key}"))
}

fn state_bool(state: &serde_json::Value, key: &str) -> Result<bool> {
    state
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .with_context(|| format!("runtime state is missing boolean {key}"))
}

#[derive(Clone, Copy)]
struct TilemapEntry {
    tile: usize,
    palette: u8,
    hflip: bool,
    vflip: bool,
}

fn tilemap_entry_at(vram: &[u8], x: usize, y: usize) -> Result<TilemapEntry> {
    if x >= TILEMAP_WIDTH || y >= TILEMAP_HEIGHT {
        bail!("BG1 tilemap coordinate {x},{y} is outside {TILEMAP_WIDTH}x{TILEMAP_HEIGHT}");
    }
    let screen_offset = (x / 32) * 0x800;
    let within_screen = (y * 32 + x % 32) * 2;
    let offset = TILEMAP_BASE_BYTE + screen_offset + within_screen;
    let bytes = vram
        .get(offset..offset + 2)
        .with_context(|| format!("BG1 tilemap entry {x},{y} is outside VRAM"))?;
    let raw = u16::from_le_bytes([bytes[0], bytes[1]]);
    Ok(TilemapEntry {
        tile: usize::from(raw & 0x03FF),
        palette: ((raw >> 10) & 0x07) as u8,
        hflip: raw & 0x4000 != 0,
        vflip: raw & 0x8000 != 0,
    })
}

fn cgram_color(cgram: &[u8], index: usize) -> Result<u16> {
    let offset = index * 2;
    let bytes = cgram
        .get(offset..offset + 2)
        .with_context(|| format!("CGRAM color {index} is outside the dump"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn set_4bpp_pixel(tile: &mut [u8], x: usize, y: usize, value: u8) {
    debug_assert_eq!(tile.len(), TILE_LEN);
    debug_assert!(x < 8 && y < 8 && value < 16);
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

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_owns_exactly_one_prompt_canvas() {
        assert_eq!(EXPECTED_DECOMPRESSED_LEN / TILE_LEN, 96);
        assert_eq!(TILE_WIDTH * TILE_HEIGHT, 81);
        assert_eq!(VRAM_BYTE, 0x3400);
    }

    #[test]
    #[ignore = "requires assets/menu_graphics/prompts/prompt_lettering_sheet.png"]
    fn generated_prompt_uses_the_original_prompt_roles() {
        let canvas = crate::generated_lettering::prompt_canvas(
            &read_prompt_sheet().unwrap(),
            PROMPT_SHEET_ROW,
            TEXT,
        )
        .unwrap();
        assert_eq!(canvas.len(), TILE_WIDTH * 8 * TILE_HEIGHT * 8);
        let used = canvas
            .iter()
            .copied()
            .filter(|&index| index != 0)
            .collect::<BTreeSet<_>>();
        assert!(used.is_subset(&BTreeSet::from([8, 9, 10, 11, 12, 14, 15])));
        assert!(used.contains(&10) && used.contains(&14));
    }
}
