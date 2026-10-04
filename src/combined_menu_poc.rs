use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const CHECKSUM_PC: usize = 0x7FDC;
const PROMPT_STREAM: std::ops::Range<usize> = 0x0D_B322..0x0D_BB75;
const MODE_STREAM: std::ops::Range<usize> = 0x0F_C5FE..0x0F_EED6;
const REMIX_STREAM: std::ops::Range<usize> = 0x13_D321..0x13_F1A2;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CombinedMenuPocReport {
    pub verdict: String,
    pub output_path: String,
    pub output_sha256: String,
    pub label_count: usize,
    pub surface_count: usize,
    pub mode_prompt: crate::remix_mode_prompt::ModePromptPocReport,
    pub inherited_mode_labels: crate::remix_mode_labels::ModeLabelsPocReport,
    pub remix_only_labels: crate::remix_menu_graphics::RemixMenuPocReport,
    pub diff_confined_to_three_streams_and_checksum: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    source: &[u8],
    source_path: String,
    mode_assets_dir: &Path,
    remix_assets_dir: &Path,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_mode_runtime_dump: Option<&Path>,
    patched_remix_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, CombinedMenuPocReport)> {
    build_impl(
        source,
        source_path,
        mode_assets_dir,
        remix_assets_dir,
        mode_select_dump,
        solo_type_dump,
        tokoton_option_dump,
        patched_mode_runtime_dump,
        patched_remix_runtime_dump,
        output_path,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    mode_assets_dir: &Path,
    remix_assets_dir: &Path,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_mode_runtime_dump: Option<&Path>,
    patched_remix_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, CombinedMenuPocReport)> {
    build_impl(
        source,
        source_path,
        mode_assets_dir,
        remix_assets_dir,
        mode_select_dump,
        solo_type_dump,
        tokoton_option_dump,
        patched_mode_runtime_dump,
        patched_remix_runtime_dump,
        output_path,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_impl(
    source: &[u8],
    source_path: String,
    mode_assets_dir: &Path,
    remix_assets_dir: &Path,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_mode_runtime_dump: Option<&Path>,
    patched_remix_runtime_dump: Option<&Path>,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, CombinedMenuPocReport)> {
    // Every stage still verifies the untouched original stream it owns. Only
    // the first entry point additionally requires whole-ROM identity.
    let (remix_patched, remix_only_labels) = if require_original_identity {
        crate::remix_menu_graphics::build_poc(
            source,
            source_path,
            remix_assets_dir,
            patched_remix_runtime_dump,
            "in-process Remix-only menu intermediate; not written".to_owned(),
        )?
    } else {
        crate::remix_menu_graphics::build_after_verified_patch(
            source,
            source_path,
            remix_assets_dir,
            patched_remix_runtime_dump,
            "in-process Remix-only menu intermediate; not written".to_owned(),
        )?
    };
    let (labels_patched, inherited_mode_labels) =
        crate::remix_mode_labels::build_after_verified_patch(
            &remix_patched,
            "verified in-process Remix-only menu derivative".to_owned(),
            mode_assets_dir,
            mode_select_dump,
            solo_type_dump,
            tokoton_option_dump,
            patched_mode_runtime_dump,
            output_path.clone(),
        )?;
    let (combined, mode_prompt) = crate::remix_mode_prompt::build_after_verified_patch(
        &labels_patched,
        "verified in-process twelve-label menu derivative".to_owned(),
        mode_select_dump,
        patched_mode_runtime_dump,
        output_path.clone(),
    )?;
    let diff_confined_to_three_streams_and_checksum =
        source
            .iter()
            .zip(&combined)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || PROMPT_STREAM.contains(&offset)
                    || MODE_STREAM.contains(&offset)
                    || REMIX_STREAM.contains(&offset)
                    || crate::remix_menu_graphics::MENU_MAP_HOOK_RANGE.contains(&offset)
                    || crate::remix_menu_graphics::MENU_MAP_CODE_RANGE.contains(&offset)
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&offset)
            });
    if !diff_confined_to_three_streams_and_checksum {
        bail!(
            "combined menu PoC diff escaped its registered streams, title-map hook, and checksum"
        );
    }
    let output_sha256 = format!("{:x}", Sha256::digest(&combined));
    Ok((
        combined,
        CombinedMenuPocReport {
            verdict: "Korean mode prompt and all twelve generative menu labels inserted into their independently verified Remix streams and tilemap ownership contract"
                .to_owned(),
            output_path,
            output_sha256,
            label_count: 12,
            surface_count: 13,
            mode_prompt,
            inherited_mode_labels,
            remix_only_labels,
            diff_confined_to_three_streams_and_checksum,
        },
    ))
}
