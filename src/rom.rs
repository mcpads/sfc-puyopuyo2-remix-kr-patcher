use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use crc32fast::Hasher as Crc32;
use encoding_rs::SHIFT_JIS;
use md5::Md5;
use serde::Serialize;
use sha1::{Digest, Sha1};
use sha2::Sha256;

const LOROM_BANK_SIZE: usize = 0x8000;

#[derive(Debug, Serialize)]
pub struct RomInfo {
    pub path: String,
    pub size: usize,
    pub copier_header: bool,
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
    pub headers: Vec<SnesHeader>,
}

#[derive(Debug, Serialize)]
pub struct SnesHeader {
    pub kind: String,
    pub pc_offset: String,
    pub title: String,
    pub title_hex: String,
    pub map_mode_hex: String,
    pub map_mode_name: String,
    pub rom_size_code: u8,
    pub checksum_hex: String,
    pub complement_hex: String,
    pub checksum_pair_valid: bool,
    pub plausible: bool,
}

#[derive(Debug, Serialize)]
pub struct CompareReport {
    pub base_path: String,
    pub target_path: String,
    pub size: usize,
    pub different_bytes: usize,
    pub changed_ranges: usize,
    pub identical_banks: Vec<String>,
    pub banks: Vec<BankComparison>,
}

#[derive(Debug, Serialize)]
pub struct BankComparison {
    pub bank: usize,
    pub bank_hex: String,
    pub size: usize,
    pub equal_bytes: usize,
}

pub fn load(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("failed to read {}", path.display()))
}

pub fn inspect(path: &Path, data: &[u8]) -> RomInfo {
    let mut crc = Crc32::new();
    crc.update(data);
    RomInfo {
        path: path.display().to_string(),
        size: data.len(),
        copier_header: data.len() % LOROM_BANK_SIZE == 512,
        crc32: format!("{:08x}", crc.finalize()),
        md5: format!("{:x}", Md5::digest(data)),
        sha1: format!("{:x}", Sha1::digest(data)),
        sha256: format!("{:x}", Sha256::digest(data)),
        headers: [("LoROM", 0x7FC0), ("HiROM", 0xFFC0)]
            .into_iter()
            .filter_map(|(kind, offset)| parse_header(kind, offset, data))
            .collect(),
    }
}

pub fn compare(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
) -> Result<CompareReport> {
    if base.len() != target.len() {
        bail!(
            "ROM sizes differ: {} and {} bytes",
            base.len(),
            target.len()
        );
    }
    let different_bytes = base.iter().zip(target).filter(|(a, b)| a != b).count();
    let mut changed_ranges = 0;
    let mut inside_change = false;
    for (a, b) in base.iter().zip(target) {
        if a != b && !inside_change {
            changed_ranges += 1;
        }
        inside_change = a != b;
    }
    let banks = base
        .chunks(LOROM_BANK_SIZE)
        .zip(target.chunks(LOROM_BANK_SIZE))
        .enumerate()
        .map(|(bank, (left, right))| BankComparison {
            bank,
            bank_hex: format!("0x{bank:02X}"),
            size: left.len(),
            equal_bytes: left.iter().zip(right).filter(|(a, b)| a == b).count(),
        })
        .collect::<Vec<_>>();
    let identical_banks = banks
        .iter()
        .filter(|bank| bank.equal_bytes == bank.size)
        .map(|bank| bank.bank_hex.clone())
        .collect();
    Ok(CompareReport {
        base_path: base_path.display().to_string(),
        target_path: target_path.display().to_string(),
        size: base.len(),
        different_bytes,
        changed_ranges,
        identical_banks,
        banks,
    })
}

pub fn pc_to_lorom(pc: usize) -> (u8, u16) {
    let bank = (pc / LOROM_BANK_SIZE) as u8;
    let address = 0x8000 | (pc % LOROM_BANK_SIZE) as u16;
    (bank, address)
}

pub fn format_lorom_addr(pc: usize) -> String {
    let (bank, address) = pc_to_lorom(pc);
    format!("${bank:02X}:${address:04X}")
}

pub fn lorom_to_pc(bank: u8, address: u16) -> Option<usize> {
    (address >= 0x8000)
        .then(|| usize::from(bank & 0x7F) * LOROM_BANK_SIZE + usize::from(address - 0x8000))
}

pub fn fix_checksum(rom: &mut [u8]) -> Result<u16> {
    const CHECKSUM_PC: usize = 0x7FDC;
    let fields = rom
        .get_mut(CHECKSUM_PC..CHECKSUM_PC + 4)
        .context("ROM is too small for the LoROM checksum fields")?;
    fields.copy_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
    let checksum = rom
        .iter()
        .fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte))) as u16;
    let complement = checksum ^ 0xFFFF;
    rom[CHECKSUM_PC..CHECKSUM_PC + 2].copy_from_slice(&complement.to_le_bytes());
    rom[CHECKSUM_PC + 2..CHECKSUM_PC + 4].copy_from_slice(&checksum.to_le_bytes());
    Ok(checksum)
}

fn parse_header(kind: &str, offset: usize, data: &[u8]) -> Option<SnesHeader> {
    let header = data.get(offset..offset + 0x20)?;
    let title_bytes = &header[..21];
    let (title, _, _) = SHIFT_JIS.decode(title_bytes);
    let map_mode = header[0x15];
    let complement = u16::from_le_bytes([header[0x1C], header[0x1D]]);
    let checksum = u16::from_le_bytes([header[0x1E], header[0x1F]]);
    let checksum_pair_valid = complement.wrapping_add(checksum) == 0xFFFF && complement != 0;
    let plausible = match kind {
        "LoROM" => matches!(map_mode, 0x20 | 0x30 | 0x32 | 0x35),
        "HiROM" => matches!(map_mode, 0x21 | 0x31 | 0x25 | 0x35),
        _ => false,
    } && checksum_pair_valid;
    Some(SnesHeader {
        kind: kind.to_owned(),
        pc_offset: format!("0x{offset:06X}"),
        title: title.trim_end().to_owned(),
        title_hex: title_bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
        map_mode_hex: format!("0x{map_mode:02X}"),
        map_mode_name: match map_mode {
            0x20 => "LoROM SlowROM",
            0x30 => "LoROM FastROM",
            0x21 => "HiROM SlowROM",
            0x31 => "HiROM FastROM",
            _ => "unknown",
        }
        .to_owned(),
        rom_size_code: header[0x17],
        checksum_hex: format!("0x{checksum:04X}"),
        complement_hex: format!("0x{complement:04X}"),
        checksum_pair_valid,
        plausible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_headerless_lorom_offsets() {
        assert_eq!(pc_to_lorom(0), (0x00, 0x8000));
        assert_eq!(pc_to_lorom(0x0B9E17), (0x17, 0x9E17));
        assert_eq!(lorom_to_pc(0x97, 0x9E17), Some(0x0B9E17));
    }
}
