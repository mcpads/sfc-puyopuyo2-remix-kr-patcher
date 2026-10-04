use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const BASE_SHA256: &str = "5b7ba076d62b0221df270e3a78e2f73ef0efa4dca54354d5207c38283a9dd45c";
const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";

struct SurfaceSpec {
    id: &'static str,
    role: &'static str,
    base_pc: usize,
    target_pc: usize,
    pointer_contract: Option<PointerContract>,
    mode_select_vram_start: Option<usize>,
}

#[derive(Clone, Copy)]
struct PointerContract {
    pointer_pc: usize,
    base_word: u16,
    target_word: u16,
}

const SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec {
        id: "gameplay_zenkeshi_chr",
        role: "gameplay all-clear OBJ graphics",
        base_pc: 0x038078,
        target_pc: 0x038078,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "shared_gameplay_obj_chr",
        role: "chain counter and Tokoton difficulty OBJ graphics",
        base_pc: 0x038DF8,
        target_pc: 0x038DF8,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "gameplay_pause_chr",
        role: "gameplay pause BG graphics",
        base_pc: 0x03C054,
        target_pc: 0x03C054,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "opponent_prompt_chr",
        role: "normal-mode opponent prompt BG3 graphics",
        base_pc: 0x0B6748,
        target_pc: 0x0B6748,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "mode_prompt_chr",
        role: "mode-selection prompt BG graphics",
        base_pc: 0x0DB438,
        target_pc: 0x0DB322,
        pointer_contract: Some(PointerContract {
            pointer_pc: 0x0D8032,
            base_word: 0xB438,
            target_word: 0xB322,
        }),
        mode_select_vram_start: Some(0x3400),
    },
    SurfaceSpec {
        id: "two_player_rules_prompt_chr",
        role: "two-player rule-selection prompt BG graphics",
        base_pc: 0x0E7660,
        target_pc: 0x0E7660,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "mode_labels_chr",
        role: "mode and solo-type lower-label BG graphics",
        base_pc: 0x0FC5E9,
        target_pc: 0x0FC5FE,
        pointer_contract: Some(PointerContract {
            pointer_pc: 0x0F8030,
            base_word: 0xC5E9,
            target_word: 0xC5FE,
        }),
        mode_select_vram_start: Some(0x0000),
    },
    SurfaceSpec {
        id: "auto_demo_title_chr",
        role: "auto-demo handwritten title graphics",
        base_pc: 0x1180D8,
        target_pc: 0x1180D8,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "ranking_title_tilemap",
        role: "solo and endless ranking title tilemap",
        base_pc: 0x12FB16,
        target_pc: 0x12FB16,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
    SurfaceSpec {
        id: "ranking_title_chr",
        role: "solo and endless ranking title BG graphics",
        base_pc: 0x130F31,
        target_pc: 0x130F31,
        pointer_contract: None,
        mode_select_vram_start: None,
    },
];

#[derive(Debug, Serialize)]
pub struct SurfaceAuditReport {
    pub base_path: String,
    pub target_path: String,
    pub base_sha256: String,
    pub target_sha256: String,
    pub surface_count: usize,
    pub byte_exact: usize,
    pub decoded_exact: usize,
    pub changed_compatible: usize,
    pub same_address: usize,
    pub relocated: usize,
    pub unresolved: usize,
    pub runtime_dump: Option<String>,
    pub runtime_surfaces_checked: usize,
    pub runtime_surfaces_matched: usize,
    pub surfaces: Vec<SurfaceReport>,
}

#[derive(Debug, Serialize)]
pub struct SurfaceReport {
    pub id: String,
    pub role: String,
    pub classification: String,
    pub base_pc: String,
    pub base_lorom: String,
    pub target_pc: String,
    pub pointer_contract: Option<String>,
    pub base_compressed_len: usize,
    pub base_decoded_len: usize,
    pub base_raw_sha256: String,
    pub base_decoded_sha256: String,
    pub target_decodes: bool,
    pub target_compressed_len: Option<usize>,
    pub target_decoded_len: Option<usize>,
    pub target_raw_sha256: Option<String>,
    pub target_decoded_sha256: Option<String>,
    pub raw_equal: bool,
    pub decoded_equal: bool,
    pub decoded_len_equal: bool,
    pub base_raw_occurrences_in_target: Vec<String>,
    pub runtime_vram_range: Option<String>,
    pub runtime_vram_matches: Option<bool>,
}

pub fn audit(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
    runtime_dump: Option<&Path>,
) -> Result<SurfaceAuditReport> {
    let base_sha256 = sha256(base);
    let target_sha256 = sha256(target);
    if base_sha256 != BASE_SHA256 {
        bail!("Tsuu base ROM SHA-256 mismatch: expected {BASE_SHA256}, got {base_sha256}");
    }
    if target_sha256 != TARGET_SHA256 {
        bail!("Remix target ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {target_sha256}");
    }

    let runtime_vram = runtime_dump
        .map(|path| {
            fs::read(path.join("vram.bin"))
                .with_context(|| format!("read runtime VRAM from {}", path.display()))
        })
        .transpose()?;
    if runtime_vram
        .as_ref()
        .is_some_and(|vram| vram.len() != 65_536)
    {
        bail!("runtime vram.bin must be exactly 65,536 bytes");
    }
    let surfaces = SURFACES
        .iter()
        .map(|spec| audit_surface(spec, base, target, runtime_vram.as_deref()))
        .collect::<Result<Vec<_>>>()?;
    Ok(SurfaceAuditReport {
        base_path: base_path.display().to_string(),
        target_path: target_path.display().to_string(),
        base_sha256,
        target_sha256,
        surface_count: surfaces.len(),
        byte_exact: count_prefix(&surfaces, "byte_exact_"),
        decoded_exact: count_prefix(&surfaces, "decoded_exact_"),
        changed_compatible: count_prefix(&surfaces, "changed_compatible_"),
        same_address: count_suffix(&surfaces, "_same_address"),
        relocated: count_suffix(&surfaces, "_relocated"),
        unresolved: surfaces
            .iter()
            .filter(|surface| surface.classification == "unresolved")
            .count(),
        runtime_dump: runtime_dump.map(|path| path.display().to_string()),
        runtime_surfaces_checked: surfaces
            .iter()
            .filter(|surface| surface.runtime_vram_matches.is_some())
            .count(),
        runtime_surfaces_matched: surfaces
            .iter()
            .filter(|surface| surface.runtime_vram_matches == Some(true))
            .count(),
        surfaces,
    })
}

fn audit_surface(
    spec: &SurfaceSpec,
    base: &[u8],
    target: &[u8],
    runtime_vram: Option<&[u8]>,
) -> Result<SurfaceReport> {
    if let Some(contract) = spec.pointer_contract {
        let base_word = read_word(base, contract.pointer_pc)?;
        let target_word = read_word(target, contract.pointer_pc)?;
        if base_word != contract.base_word || target_word != contract.target_word {
            bail!(
                "{} asset pointer contract at PC 0x{:06X} differs: base 0x{base_word:04X}, target 0x{target_word:04X}",
                spec.id,
                contract.pointer_pc
            );
        }
    }

    let base_block = crate::snes_lz::decompress(base, spec.base_pc)
        .with_context(|| format!("decode base surface {}", spec.id))?;
    let base_raw = base
        .get(spec.base_pc..spec.base_pc + base_block.compressed_len)
        .context("base compressed surface is outside ROM")?;
    let raw_occurrences = find_pattern_offsets(target, base_raw);
    let target_block = crate::snes_lz::decompress(target, spec.target_pc).ok();
    let target_raw = target_block.as_ref().and_then(|block| {
        target.get(spec.target_pc..spec.target_pc.checked_add(block.compressed_len)?)
    });
    let raw_equal = target_raw == Some(base_raw);
    let decoded_equal = target_block
        .as_ref()
        .is_some_and(|block| block.bytes == base_block.bytes);
    let decoded_len_equal = target_block
        .as_ref()
        .is_some_and(|block| block.bytes.len() == base_block.bytes.len());
    let location = if spec.target_pc == spec.base_pc {
        "same_address"
    } else {
        "relocated"
    };
    let classification = if raw_equal {
        format!("byte_exact_{location}")
    } else if decoded_equal {
        format!("decoded_exact_{location}")
    } else if decoded_len_equal {
        format!("changed_compatible_{location}")
    } else {
        "unresolved".to_owned()
    };
    let (bank, address) = crate::rom::pc_to_lorom(spec.base_pc);
    let runtime_range = spec.mode_select_vram_start.and_then(|start| {
        target_block
            .as_ref()
            .map(|block| (start, start + block.bytes.len()))
    });
    let runtime_vram_matches = runtime_range.and_then(|(start, end)| {
        runtime_vram.map(|vram| {
            target_block
                .as_ref()
                .is_some_and(|block| vram.get(start..end) == Some(block.bytes.as_slice()))
        })
    });

    Ok(SurfaceReport {
        id: spec.id.to_owned(),
        role: spec.role.to_owned(),
        classification,
        base_pc: format!("0x{:06X}", spec.base_pc),
        base_lorom: format!("${bank:02X}:${address:04X}"),
        target_pc: format!("0x{:06X}", spec.target_pc),
        pointer_contract: spec.pointer_contract.map(|contract| {
            format!(
                "PC 0x{:06X}: 0x{:04X}->0x{:04X}",
                contract.pointer_pc, contract.base_word, contract.target_word
            )
        }),
        base_compressed_len: base_block.compressed_len,
        base_decoded_len: base_block.bytes.len(),
        base_raw_sha256: sha256(base_raw),
        base_decoded_sha256: sha256(&base_block.bytes),
        target_decodes: target_block.is_some(),
        target_compressed_len: target_block.as_ref().map(|block| block.compressed_len),
        target_decoded_len: target_block.as_ref().map(|block| block.bytes.len()),
        target_raw_sha256: target_raw.map(sha256),
        target_decoded_sha256: target_block.as_ref().map(|block| sha256(&block.bytes)),
        raw_equal,
        decoded_equal,
        decoded_len_equal,
        base_raw_occurrences_in_target: raw_occurrences
            .into_iter()
            .map(|offset| format!("0x{offset:06X}"))
            .collect(),
        runtime_vram_range: runtime_range
            .map(|(start, end)| format!("0x{start:04X}-0x{:04X}", end - 1)),
        runtime_vram_matches,
    })
}

fn count_prefix(surfaces: &[SurfaceReport], prefix: &str) -> usize {
    surfaces
        .iter()
        .filter(|surface| surface.classification.starts_with(prefix))
        .count()
}

fn count_suffix(surfaces: &[SurfaceReport], suffix: &str) -> usize {
    surfaces
        .iter()
        .filter(|surface| surface.classification.ends_with(suffix))
        .count()
}

fn find_pattern_offsets(data: &[u8], pattern: &[u8]) -> Vec<usize> {
    data.windows(pattern.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == pattern).then_some(offset))
        .collect()
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn read_word(data: &[u8], pc: usize) -> Result<u16> {
    let bytes = data
        .get(pc..pc + 2)
        .with_context(|| format!("word at PC 0x{pc:06X} is outside ROM"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}
