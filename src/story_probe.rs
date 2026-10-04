use std::{collections::BTreeMap, path::Path};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BANK_SIZE: usize = 0x8000;
const MAX_BLOCK_LEN: usize = 0x200;

#[derive(Debug, Serialize)]
pub struct StoryScanReport {
    pub path: String,
    pub method: String,
    pub banks: Vec<BankStoryReport>,
}

#[derive(Debug, Serialize)]
pub struct BankStoryReport {
    pub bank: usize,
    pub bank_hex: String,
    pub pc_start: usize,
    pub pc_end_exclusive: usize,
    pub structural_candidates: usize,
    pub accepted_blocks: usize,
    pub rejected_candidates: usize,
    pub long_pointer_references: usize,
    pub unknown_controls: BTreeMap<String, usize>,
    pub sample_block_starts: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StoryCompareReport {
    pub base_path: String,
    pub target_path: String,
    pub bank: u8,
    pub bank_hex: String,
    pub base_blocks: usize,
    pub target_blocks: usize,
    pub same_index_exact_blocks: usize,
    pub all_blocks_same_order_and_bytes: bool,
    pub target_blocks_with_unique_raw_match: usize,
    pub target_blocks_with_ambiguous_raw_match: usize,
    pub target_blocks_without_raw_match: usize,
    pub base_pointer_candidates: PointerCandidateSummary,
    pub target_pointer_candidates: PointerCandidateSummary,
    pub sample_address_mappings: Vec<StoryAddressMapping>,
}

#[derive(Debug, Serialize)]
pub struct StoryAddressMapping {
    pub index: usize,
    pub base: String,
    pub target: String,
    pub raw_len: usize,
}

#[derive(Debug, Serialize)]
pub struct PointerCandidateSummary {
    pub entries_with_long_pointer: usize,
    pub raw_long_pointer_occurrences: usize,
    pub entries_without_long_pointer: usize,
    pub unpointed_contiguous_after_previous: usize,
    pub unpointed_noncontiguous_indices: Vec<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StoryPortMap {
    pub schema_version: u32,
    pub table_id: String,
    pub base_rom_sha256: String,
    pub target_rom_sha256: String,
    pub bank: String,
    pub matching_rule: String,
    pub entries: Vec<StoryPortMapEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StoryPortMapEntry {
    pub entry_id: usize,
    pub logical_id: String,
    pub base_stable_id: String,
    pub target_stable_id: String,
    pub base_file_offset: String,
    pub target_file_offset: String,
    pub base_lorom_address: String,
    pub target_lorom_address: String,
    pub raw_len: usize,
    pub raw_sha256: String,
}

#[derive(Debug)]
struct CandidateBlock {
    pc: usize,
    raw: Vec<u8>,
}

#[derive(Debug)]
enum ParseFailure {
    UnknownControl(u8),
    Invalid,
}

pub fn scan(path: &Path, rom: &[u8], bank_filter: Option<u8>) -> Result<StoryScanReport> {
    if rom.is_empty() || !rom.len().is_multiple_of(BANK_SIZE) {
        bail!("story candidate scan requires a non-empty headerless 32 KiB-bank ROM");
    }
    let bank_count = rom.len() / BANK_SIZE;
    if bank_filter.is_some_and(|bank| usize::from(bank) >= bank_count) {
        bail!("requested bank is outside the ROM");
    }
    let banks = (0..bank_count)
        .filter(|bank| bank_filter.is_none_or(|selected| *bank == usize::from(selected)))
        .map(|bank| scan_bank(rom, bank))
        .filter(|report| report.structural_candidates > 0)
        .collect();
    Ok(StoryScanReport {
        path: path.display().to_string(),
        method: "candidate_only_ff02_word_ff03_with_tsuu_control_widths".to_owned(),
        banks,
    })
}

pub fn compare(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
    bank: u8,
) -> Result<StoryCompareReport> {
    let base_blocks = candidate_blocks(base, bank)?;
    let target_blocks = candidate_blocks(target, bank)?;
    let same_index_exact_blocks = base_blocks
        .iter()
        .zip(&target_blocks)
        .filter(|(left, right)| left.raw == right.raw)
        .count();

    let mut base_raw_counts = BTreeMap::<Vec<u8>, usize>::new();
    for block in &base_blocks {
        *base_raw_counts.entry(block.raw.clone()).or_default() += 1;
    }
    let target_blocks_with_unique_raw_match = target_blocks
        .iter()
        .filter(|block| base_raw_counts.get(&block.raw) == Some(&1))
        .count();
    let target_blocks_with_ambiguous_raw_match = target_blocks
        .iter()
        .filter(|block| {
            base_raw_counts
                .get(&block.raw)
                .is_some_and(|count| *count > 1)
        })
        .count();
    let target_blocks_without_raw_match = target_blocks
        .iter()
        .filter(|block| !base_raw_counts.contains_key(&block.raw))
        .count();
    let sample_address_mappings = base_blocks
        .iter()
        .zip(&target_blocks)
        .enumerate()
        .take(24)
        .map(|(index, (left, right))| StoryAddressMapping {
            index,
            base: format_pc(left.pc),
            target: format_pc(right.pc),
            raw_len: left.raw.len(),
        })
        .collect();
    let all_blocks_same_order_and_bytes =
        base_blocks.len() == target_blocks.len() && same_index_exact_blocks == base_blocks.len();
    let base_pointer_candidates = pointer_candidates(base, &base_blocks, bank);
    let target_pointer_candidates = pointer_candidates(target, &target_blocks, bank);

    Ok(StoryCompareReport {
        base_path: base_path.display().to_string(),
        target_path: target_path.display().to_string(),
        bank,
        bank_hex: format!("0x{bank:02X}"),
        base_blocks: base_blocks.len(),
        target_blocks: target_blocks.len(),
        same_index_exact_blocks,
        all_blocks_same_order_and_bytes,
        target_blocks_with_unique_raw_match,
        target_blocks_with_ambiguous_raw_match,
        target_blocks_without_raw_match,
        base_pointer_candidates,
        target_pointer_candidates,
        sample_address_mappings,
    })
}

pub fn port_map(base: &[u8], target: &[u8], bank: u8) -> Result<StoryPortMap> {
    let base_blocks = candidate_blocks(base, bank)?;
    let target_blocks = candidate_blocks(target, bank)?;
    if base_blocks.len() != target_blocks.len() {
        bail!(
            "story block counts differ: base {} target {}",
            base_blocks.len(),
            target_blocks.len()
        );
    }
    let entries = base_blocks
        .iter()
        .zip(&target_blocks)
        .enumerate()
        .map(|(entry_id, (base_block, target_block))| {
            if base_block.raw != target_block.raw {
                bail!("story block {entry_id} is not byte-exact between base and target");
            }
            let (base_bank, base_address) = crate::rom::pc_to_lorom(base_block.pc);
            let (target_bank, target_address) = crate::rom::pc_to_lorom(target_block.pc);
            Ok(StoryPortMapEntry {
                entry_id,
                logical_id: format!("story_{entry_id:03}"),
                base_stable_id: format!("story_{base_bank:02X}_{base_address:04X}"),
                target_stable_id: format!("story_{target_bank:02X}_{target_address:04X}"),
                base_file_offset: format!("0x{:06X}", base_block.pc),
                target_file_offset: format!("0x{:06X}", target_block.pc),
                base_lorom_address: format!("${base_bank:02X}:${base_address:04X}"),
                target_lorom_address: format!("${target_bank:02X}:${target_address:04X}"),
                raw_len: base_block.raw.len(),
                raw_sha256: format!("{:x}", Sha256::digest(&base_block.raw)),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(StoryPortMap {
        schema_version: 1,
        table_id: "story_bank_17".to_owned(),
        base_rom_sha256: format!("{:x}", Sha256::digest(base)),
        target_rom_sha256: format!("{:x}", Sha256::digest(target)),
        bank: format!("0x{bank:02X}"),
        matching_rule: "same_entry_order_and_byte_exact_raw".to_owned(),
        entries,
    })
}

fn pointer_candidates(rom: &[u8], blocks: &[CandidateBlock], bank: u8) -> PointerCandidateSummary {
    let runtime_bank = bank | 0x80;
    let reference_counts = blocks
        .iter()
        .map(|block| {
            let (_, address) = crate::rom::pc_to_lorom(block.pc);
            let pattern = [address as u8, (address >> 8) as u8, runtime_bank];
            rom.windows(3).filter(|window| *window == pattern).count()
        })
        .collect::<Vec<_>>();
    let unpointed_contiguous_after_previous = blocks
        .iter()
        .enumerate()
        .filter(|(index, _)| reference_counts[*index] == 0)
        .filter(|(index, block)| {
            *index > 0 && blocks[*index - 1].pc + blocks[*index - 1].raw.len() == block.pc
        })
        .count();
    let unpointed_noncontiguous_indices = blocks
        .iter()
        .enumerate()
        .filter(|(index, _)| reference_counts[*index] == 0)
        .filter(|(index, block)| {
            *index == 0 || blocks[*index - 1].pc + blocks[*index - 1].raw.len() != block.pc
        })
        .map(|(index, _)| index)
        .collect();
    PointerCandidateSummary {
        entries_with_long_pointer: reference_counts.iter().filter(|count| **count > 0).count(),
        raw_long_pointer_occurrences: reference_counts.iter().sum(),
        entries_without_long_pointer: reference_counts.iter().filter(|count| **count == 0).count(),
        unpointed_contiguous_after_previous,
        unpointed_noncontiguous_indices,
    }
}

fn candidate_blocks(rom: &[u8], bank: u8) -> Result<Vec<CandidateBlock>> {
    if rom.is_empty() || !rom.len().is_multiple_of(BANK_SIZE) {
        bail!("story candidate scan requires a non-empty headerless 32 KiB-bank ROM");
    }
    let bank = usize::from(bank);
    let pc_start = bank
        .checked_mul(BANK_SIZE)
        .filter(|start| *start < rom.len())
        .ok_or_else(|| anyhow::anyhow!("requested bank is outside the ROM"))?;
    let pc_end_exclusive = (pc_start + BANK_SIZE).min(rom.len());
    let mut blocks = Vec::new();
    for pc in pc_start..pc_end_exclusive.saturating_sub(5) {
        if rom[pc] != 0xFF || rom[pc + 1] != 0x02 || rom[pc + 4] != 0xFF || rom[pc + 5] != 0x03 {
            continue;
        }
        let Ok(len) = parse_block(rom, pc, pc_end_exclusive) else {
            continue;
        };
        blocks.push(CandidateBlock {
            pc,
            raw: rom[pc..pc + len].to_vec(),
        });
    }
    Ok(blocks)
}

fn format_pc(pc: usize) -> String {
    let (bank, address) = crate::rom::pc_to_lorom(pc);
    format!("${bank:02X}:${address:04X}")
}

fn scan_bank(rom: &[u8], bank: usize) -> BankStoryReport {
    let pc_start = bank * BANK_SIZE;
    let pc_end_exclusive = (pc_start + BANK_SIZE).min(rom.len());
    let mut structural_candidates = 0;
    let mut accepted_starts = Vec::new();
    let mut unknown_controls = BTreeMap::new();

    for pc in pc_start..pc_end_exclusive.saturating_sub(5) {
        if rom[pc] != 0xFF || rom[pc + 1] != 0x02 || rom[pc + 4] != 0xFF || rom[pc + 5] != 0x03 {
            continue;
        }
        structural_candidates += 1;
        match parse_block(rom, pc, pc_end_exclusive) {
            Ok(_) => accepted_starts.push(pc),
            Err(ParseFailure::UnknownControl(code)) => {
                *unknown_controls.entry(format!("0x{code:02X}")).or_default() += 1;
            }
            Err(ParseFailure::Invalid) => {}
        }
    }

    let runtime_bank = bank as u8 | 0x80;
    let long_pointer_references = accepted_starts
        .iter()
        .map(|pc| {
            let (_, address) = crate::rom::pc_to_lorom(*pc);
            let pattern = [address as u8, (address >> 8) as u8, runtime_bank];
            rom.windows(3).filter(|window| *window == pattern).count()
        })
        .sum();
    let sample_block_starts = accepted_starts
        .iter()
        .take(24)
        .map(|pc| {
            let (bank, address) = crate::rom::pc_to_lorom(*pc);
            format!("${bank:02X}:${address:04X}")
        })
        .collect();
    BankStoryReport {
        bank,
        bank_hex: format!("0x{bank:02X}"),
        pc_start,
        pc_end_exclusive,
        structural_candidates,
        accepted_blocks: accepted_starts.len(),
        rejected_candidates: structural_candidates - accepted_starts.len(),
        long_pointer_references,
        unknown_controls,
        sample_block_starts,
    }
}

fn parse_block(
    rom: &[u8],
    start: usize,
    bank_end: usize,
) -> std::result::Result<usize, ParseFailure> {
    let hard_end = (start + MAX_BLOCK_LEN).min(bank_end);
    let mut cursor = start;
    while cursor < hard_end {
        let byte = *rom.get(cursor).ok_or(ParseFailure::Invalid)?;
        cursor += 1;
        match byte {
            0xFE => {
                cursor = cursor
                    .checked_add(1)
                    .filter(|end| *end <= hard_end)
                    .ok_or(ParseFailure::Invalid)?
            }
            0xFF => {
                let code = *rom.get(cursor).ok_or(ParseFailure::Invalid)?;
                cursor += 1;
                if code == 0x00 {
                    return Ok(cursor - start);
                }
                let arg_len = match code {
                    0x01 | 0x02 | 0x04..=0x09 => 2,
                    0x03 => 4,
                    0x0A => 0,
                    _ => return Err(ParseFailure::UnknownControl(code)),
                };
                cursor = cursor
                    .checked_add(arg_len)
                    .filter(|end| *end <= hard_end)
                    .ok_or(ParseFailure::Invalid)?;
            }
            _ => {}
        }
    }
    Err(ParseFailure::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_round_trippable_tsuu_grammar_candidate() {
        let block = [
            0xFF, 0x02, 0x06, 0x03, 0xFF, 0x03, 0xC4, 0x02, 0x06, 0x10, 0x3E, 0xFE, 0x17, 0xFF,
            0x00,
        ];
        assert_eq!(parse_block(&block, 0, block.len()).unwrap(), block.len());
    }

    #[test]
    fn reports_unknown_control_without_guessing_width() {
        let block = [
            0xFF, 0x02, 0, 0, 0xFF, 0x03, 0, 0, 0, 0, 0xFF, 0x0E, 0xFF, 0x00,
        ];
        assert!(matches!(
            parse_block(&block, 0, block.len()),
            Err(ParseFailure::UnknownControl(0x0E))
        ));
    }

    #[test]
    fn compares_equal_blocks_after_relocation() {
        let block = [
            0xFF, 0x02, 0x06, 0x03, 0xFF, 0x03, 0xC4, 0x02, 0x06, 0x10, 0x3E, 0xFF, 0x00,
        ];
        let mut base = vec![0; BANK_SIZE];
        let mut target = vec![0; BANK_SIZE];
        base[0x100..0x100 + block.len()].copy_from_slice(&block);
        target[0x200..0x200 + block.len()].copy_from_slice(&block);
        let report = compare(Path::new("base"), &base, Path::new("target"), &target, 0).unwrap();
        assert!(report.all_blocks_same_order_and_bytes);
        assert_eq!(report.same_index_exact_blocks, 1);
        assert_eq!(report.sample_address_mappings[0].base, "$00:$8100");
        assert_eq!(report.sample_address_mappings[0].target, "$00:$8200");
    }

    #[test]
    fn port_map_refuses_nonidentical_raw_blocks() {
        let block = [
            0xFF, 0x02, 0x06, 0x03, 0xFF, 0x03, 0xC4, 0x02, 0x06, 0x10, 0x3E, 0xFF, 0x00,
        ];
        let mut base = vec![0; BANK_SIZE];
        let mut target = vec![0; BANK_SIZE];
        base[0x100..0x100 + block.len()].copy_from_slice(&block);
        target[0x200..0x200 + block.len()].copy_from_slice(&block);
        target[0x20A] = 0x3F;
        assert!(port_map(&base, &target, 0).is_err());
    }
}
