use std::{ops::Range, path::Path};

use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const CHECKSUM_PC: usize = 0x7FDC;
const PROMPT_STREAM: Range<usize> = 0x0D_B322..0x0D_BB75;
const MODE_STREAM: Range<usize> = 0x0F_C5FE..0x0F_EED6;
const REMIX_STREAM: Range<usize> = 0x13_D321..0x13_F1A2;
const TOKOTON_CHOICE_STREAM: Range<usize> = 0x0C_1C16..0x0C_2A2F;
const SHARED_OBJ_STREAM: Range<usize> = 0x03_8DF8..0x03_A534;
const ZENKESHI_SOURCE_STREAM: Range<usize> = 0x03_8078..0x03_8DF8;
const PAUSE_STREAM: Range<usize> = 0x03_C054..0x03_D7C6;
const EXTENDED_FONT_REGION: Range<usize> = 0x04_1264..0x04_2BCD;
const SAMPLE_POINTER: Range<usize> = 0x00_53DC..0x00_53DE;
const SAMPLE_RELOCATION: Range<usize> = 0x00_6152..0x00_6192;
const OPPONENT_PROMPT_STREAM: Range<usize> = 0x0B_6748..0x0B_6D42;
const ZENKESHI_FRAME: Range<usize> = 0x00_B536..0x00_B588;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CombinedUiPocReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub output_path: String,
    pub output_sha256: String,
    pub menu_label_count: usize,
    pub menu_surface_count: usize,
    pub gameplay_effect_count: usize,
    pub menu: crate::combined_menu_poc::CombinedMenuPocReport,
    pub tokoton_choices: crate::remix_tokoton_play::BuildReport,
    pub gameplay: crate::remix_gameplay_text::RemixGameplayTextReport,
    pub sample_select: crate::sample_select_text::BuildReport,
    pub opponent_prompt: crate::opponent_prompt::OpponentPromptKrPocReport,
    pub checksum_hex: String,
    pub diff_confined_to_registered_writes: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    source: &[u8],
    source_path: String,
    mode_assets_dir: &Path,
    remix_assets_dir: &Path,
    prompt_ttf_path: String,
    prompt_ttf_data: &[u8],
    opponent_prompt_font_px: f32,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_mode_runtime_dump: Option<&Path>,
    patched_remix_runtime_dump: Option<&Path>,
    tokoton_play_asset: &Path,
    tokoton_demonstration_asset: &Path,
    tokoton_choice_dump: &Path,
    patched_tokoton_runtime_dump: Option<&Path>,
    rensa_ttf_path: String,
    rensa_ttf_data: &[u8],
    sample_font_px: f32,
    output_path: String,
) -> Result<(Vec<u8>, CombinedUiPocReport)> {
    let source_sha256 = sha256(source);
    let (menu_rom, menu) = crate::combined_menu_poc::build(
        source,
        source_path.clone(),
        mode_assets_dir,
        remix_assets_dir,
        mode_select_dump,
        solo_type_dump,
        tokoton_option_dump,
        patched_mode_runtime_dump,
        patched_remix_runtime_dump,
        output_path.clone(),
    )?;
    let (tokoton_rom, tokoton_choices) = crate::remix_tokoton_play::build_after_verified_patch(
        &menu_rom,
        "verified in-process fourteen-label menu derivative".to_owned(),
        tokoton_play_asset,
        tokoton_demonstration_asset,
        tokoton_choice_dump,
        patched_tokoton_runtime_dump,
        output_path.clone(),
    )?;
    let (gameplay_rom, gameplay) = crate::remix_gameplay_text::build_after_verified_patch(
        &tokoton_rom,
        "verified in-process fifteen-surface menu derivative".to_owned(),
        output_path.clone(),
    )?;
    let (sample_rom, sample_select) = crate::sample_select_text::build_after_verified_patch(
        &gameplay_rom,
        "verified in-process menu and gameplay derivative".to_owned(),
        rensa_ttf_path,
        rensa_ttf_data,
        sample_font_px,
        output_path.clone(),
    )?;
    let opponent_inputs = crate::opponent_prompt::OpponentPromptKrPocInputs {
        rom: &sample_rom,
        source_path: "verified in-process UI and sample-select derivative".to_owned(),
        ttf_path: prompt_ttf_path,
        ttf_data: prompt_ttf_data,
        font_px: opponent_prompt_font_px,
        runtime_vram: None,
        runtime_cram: None,
        output_path: output_path.clone(),
    };
    let (mut combined, opponent_prompt) =
        crate::opponent_prompt::build_opponent_prompt_kr_poc(&opponent_inputs)?;
    let checksum = crate::rom::fix_checksum(&mut combined)?;

    let diff_confined_to_registered_writes =
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
                    || TOKOTON_CHOICE_STREAM.contains(&offset)
                    || SHARED_OBJ_STREAM.contains(&offset)
                    || ZENKESHI_SOURCE_STREAM.contains(&offset)
                    || PAUSE_STREAM.contains(&offset)
                    || EXTENDED_FONT_REGION.contains(&offset)
                    || SAMPLE_POINTER.contains(&offset)
                    || SAMPLE_RELOCATION.contains(&offset)
                    || OPPONENT_PROMPT_STREAM.contains(&offset)
                    || ZENKESHI_FRAME.contains(&offset)
                    || crate::tokoton_difficulty::registered_write(offset).is_some()
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&offset)
            });
    if !diff_confined_to_registered_writes {
        bail!("combined Remix UI PoC diff escaped its registered write ranges");
    }

    let output_sha256 = sha256(&combined);
    Ok((
        combined,
        CombinedUiPocReport {
            verdict: "all verified Remix menu labels plus Korean 상대 선택 prompt, 놀기, 시범, 3/4/5연쇄, 끝내기, 연쇄, 상쇄, 싹쓸이!, and 휴식중 inserted from the original ROM"
                .to_owned(),
            source_path,
            source_sha256,
            output_path,
            output_sha256,
            menu_label_count: menu.label_count + 2,
            menu_surface_count: menu.surface_count + 2,
            gameplay_effect_count: gameplay.effect_count,
            menu,
            tokoton_choices,
            gameplay,
            sample_select,
            opponent_prompt,
            checksum_hex: format!("0x{checksum:04X}"),
            diff_confined_to_registered_writes,
        },
    ))
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
