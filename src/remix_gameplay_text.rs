use std::ops::Range;

use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const CHECKSUM_PC: usize = 0x7FDC;
const SHARED_OBJ_STREAM: Range<usize> = 0x03_8DF8..0x03_A534;
const ZENKESHI_SOURCE_STREAM: Range<usize> = 0x03_8078..0x03_8DF8;
const PAUSE_STREAM: Range<usize> = 0x03_C054..0x03_D7C6;
const ZENKESHI_FRAME: Range<usize> = 0x00_B536..0x00_B588;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemixGameplayTextReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub output_path: String,
    pub output_sha256: String,
    pub effect_count: usize,
    pub texts: Vec<String>,
    pub rensa: crate::gameplay_rensa::GameplayRensaKrPocReport,
    pub tokoton_difficulty: crate::tokoton_difficulty::BuildReport,
    pub sousai: crate::gameplay_sousai::BuildReport,
    pub zenkeshi: crate::gameplay_zenkeshi::BuildReport,
    pub pause: crate::gameplay_pause::BuildReport,
    pub checksum_hex: String,
    pub diff_confined_to_registered_writes: bool,
}

pub fn build_poc(
    source: &[u8],
    source_path: String,
    output_path: String,
) -> Result<(Vec<u8>, RemixGameplayTextReport)> {
    build_impl(source, source_path, output_path, true)
}

pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    output_path: String,
) -> Result<(Vec<u8>, RemixGameplayTextReport)> {
    build_impl(source, source_path, output_path, false)
}

fn build_impl(
    source: &[u8],
    source_path: String,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, RemixGameplayTextReport)> {
    let source_sha256 = sha256(source);
    if require_original_identity && source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }

    let (rensa_rom, rensa) = crate::gameplay_rensa::build_gameplay_rensa_kr_poc(
        source,
        source_path.clone(),
        None,
        "in-process Remix rensa intermediate; not written".to_owned(),
    )?;
    let (sousai_rom, sousai) = crate::gameplay_sousai::build_kr_poc(
        &rensa_rom,
        "verified in-process Remix rensa derivative".to_owned(),
        None,
        None,
        "in-process Remix sousai intermediate; not written".to_owned(),
    )?;
    // The difficulty labels come last in the shared OBJ stream because the
    // grown stream may move to the relocation slot, which sousai cannot read.
    let (tokoton_rom, tokoton_difficulty) = crate::tokoton_difficulty::build_kr(&sousai_rom)?;
    let (zenkeshi_rom, zenkeshi) = crate::gameplay_zenkeshi::build_kr_poc(
        &tokoton_rom,
        "verified in-process Remix sousai and Tokoton-difficulty derivative".to_owned(),
        None,
        None,
        "in-process Remix zenkeshi intermediate; not written".to_owned(),
    )?;
    let (mut patched, pause) = crate::gameplay_pause::build_kr_poc(
        &zenkeshi_rom,
        "verified in-process Remix zenkeshi derivative".to_owned(),
        None,
        None,
        output_path.clone(),
    )?;
    let checksum = crate::rom::fix_checksum(&mut patched)?;

    let diff_confined_to_registered_writes =
        source
            .iter()
            .zip(&patched)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || SHARED_OBJ_STREAM.contains(&offset)
                    || ZENKESHI_SOURCE_STREAM.contains(&offset)
                    || PAUSE_STREAM.contains(&offset)
                    || ZENKESHI_FRAME.contains(&offset)
                    || crate::tokoton_difficulty::registered_write(offset).is_some()
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&offset)
            });
    if !diff_confined_to_registered_writes {
        bail!("Remix gameplay-text PoC diff escaped its registered write ranges");
    }

    let output_sha256 = sha256(&patched);
    Ok((
        patched,
        RemixGameplayTextReport {
            verdict:
                "Remix gameplay graphics rebuilt as Korean 연쇄, Tokoton difficulty labels, 상쇄, 싹쓸이!, and 휴식중"
                    .to_owned(),
            source_path,
            source_sha256,
            output_path,
            output_sha256,
            effect_count: 5,
            texts: vec![
                "연쇄".to_owned(),
                "단맛/순함/보통/매움/불맛".to_owned(),
                "상쇄".to_owned(),
                "싹쓸이!".to_owned(),
                "휴식중".to_owned(),
            ],
            rensa,
            tokoton_difficulty,
            sousai,
            zenkeshi,
            pause,
            checksum_hex: format!("0x{checksum:04X}"),
            diff_confined_to_registered_writes,
        },
    ))
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
