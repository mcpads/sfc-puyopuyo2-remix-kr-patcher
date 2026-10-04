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
const ZENKESHI_FRAME: Range<usize> = 0x00_B536..0x00_B588;

pub struct IntegratedPocInputs<'a> {
    pub source_path: String,
    pub story: crate::story_poc::StoryPocInputs<'a>,
    pub demo_title_ttf_path: String,
    pub demo_title_ttf_data: &'a [u8],
    pub demo_title_font_px: f32,
    pub mode_assets_dir: &'a Path,
    pub remix_assets_dir: &'a Path,
    pub mode_select_dump: &'a Path,
    pub solo_type_dump: &'a Path,
    pub tokoton_option_dump: &'a Path,
    pub patched_mode_runtime_dump: Option<&'a Path>,
    pub patched_remix_runtime_dump: Option<&'a Path>,
    pub tokoton_play_asset: &'a Path,
    pub tokoton_demonstration_asset: &'a Path,
    pub tokoton_choice_dump: &'a Path,
    pub patched_tokoton_runtime_dump: Option<&'a Path>,
    pub ranking_ending: crate::remix_ranking_ending_graphics::Inputs<'a>,
    pub course_continue: crate::remix_course_continue_graphics::Inputs<'a>,
    pub caption_marker: crate::remix_caption_marker_graphics::Inputs<'a>,
    pub output_path: String,
}

#[derive(Debug, Serialize)]
pub struct IntegratedPocReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub output_path: String,
    pub output_sha256: String,
    pub story: crate::story_poc::StoryPocReport,
    pub demo_titles: crate::demo_explanation::DemoTitleReport,
    pub menu: crate::combined_menu_poc::CombinedMenuPocReport,
    pub tokoton_choices: crate::remix_tokoton_play::BuildReport,
    pub gameplay: crate::remix_gameplay_text::RemixGameplayTextReport,
    pub ranking_ending: crate::remix_ranking_ending_graphics::Report,
    pub course_continue: crate::remix_course_continue_graphics::Report,
    pub caption_marker: crate::remix_caption_marker_graphics::Report,
    pub story_owned_changed_bytes: usize,
    pub ui_owned_changed_bytes: usize,
    pub changed_bytes: usize,
    pub write_conflicts: usize,
    pub checksum_hex: String,
    pub diff_confined_to_story_and_registered_ui_writes: bool,
}

pub fn build(inputs: &IntegratedPocInputs<'_>) -> Result<(Vec<u8>, IntegratedPocReport, String)> {
    let source = inputs.story.rom;
    let source_sha256 = sha256(source);
    let (story_rom, story, encoding) = crate::story_poc::build(&inputs.story)?;
    if story_rom.len() != source.len() {
        bail!("story PoC changed the ROM length before UI composition");
    }

    let story_changed = source
        .iter()
        .zip(&story_rom)
        .map(|(before, after)| before != after)
        .collect::<Vec<_>>();
    if let Some((offset, owner)) =
        story_changed
            .iter()
            .enumerate()
            .find_map(|(offset, &changed)| {
                (changed && !checksum_range().contains(&offset))
                    .then(|| registered_ui_write(offset).map(|owner| (offset, owner)))
                    .flatten()
            })
    {
        bail!("story/option stage conflicts with registered UI write {owner} at PC 0x{offset:06X}");
    }
    let story_owned_changed_bytes = story_changed
        .iter()
        .enumerate()
        .filter(|(offset, changed)| **changed && !checksum_range().contains(offset))
        .count();

    let (demo_title_rom, demo_titles) = crate::demo_explanation::build_title_after_verified_patch(
        &story_rom,
        inputs.demo_title_ttf_path.clone(),
        inputs.demo_title_ttf_data,
        inputs.demo_title_font_px,
    )?;

    let (menu_rom, menu) = crate::combined_menu_poc::build_after_verified_patch(
        &demo_title_rom,
        "verified in-process story, option, game-explanation, sample-select, and opponent-prompt derivative".to_owned(),
        inputs.mode_assets_dir,
        inputs.remix_assets_dir,
        inputs.mode_select_dump,
        inputs.solo_type_dump,
        inputs.tokoton_option_dump,
        inputs.patched_mode_runtime_dump,
        inputs.patched_remix_runtime_dump,
        "in-process integrated menu intermediate; not written".to_owned(),
    )?;
    let (tokoton_rom, tokoton_choices) = crate::remix_tokoton_play::build_after_verified_patch(
        &menu_rom,
        "verified in-process story and Korean menu derivative".to_owned(),
        inputs.tokoton_play_asset,
        inputs.tokoton_demonstration_asset,
        inputs.tokoton_choice_dump,
        inputs.patched_tokoton_runtime_dump,
        "in-process integrated Tokoton intermediate; not written".to_owned(),
    )?;
    let (gameplay_rom, gameplay) = crate::remix_gameplay_text::build_after_verified_patch(
        &tokoton_rom,
        "verified in-process story, menu, and Tokoton derivative".to_owned(),
        inputs.output_path.clone(),
    )?;
    let (ranking_rom, ranking_ending) =
        crate::remix_ranking_ending_graphics::build_after_verified_patch(
            &gameplay_rom,
            &inputs.ranking_ending,
        )?;
    let (course_rom, course_continue) =
        crate::remix_course_continue_graphics::build_after_verified_patch(
            &ranking_rom,
            &inputs.course_continue,
        )?;
    let (mut combined, caption_marker) =
        crate::remix_caption_marker_graphics::build_after_verified_patch(
            &course_rom,
            &inputs.caption_marker,
        )?;
    let checksum = crate::rom::fix_checksum(&mut combined)?;

    let ui_owned_changed_bytes = story_rom
        .iter()
        .zip(&combined)
        .enumerate()
        .filter(|(offset, (before, after))| before != after && !checksum_range().contains(offset))
        .count();
    let changed_bytes = source
        .iter()
        .zip(&combined)
        .filter(|(before, after)| before != after)
        .count();
    let diff_confined_to_story_and_registered_ui_writes = source
        .iter()
        .zip(&combined)
        .enumerate()
        .all(|(offset, (before, after))| {
            before == after
                || story_changed[offset]
                || registered_ui_write(offset).is_some()
                || checksum_range().contains(&offset)
        });
    if !diff_confined_to_story_and_registered_ui_writes {
        bail!("integrated Remix PoC diff escaped story ownership and registered UI writes");
    }

    let output_sha256 = sha256(&combined);
    Ok((
        combined,
        IntegratedPocReport {
            verdict: "273-entry Korean story, 13-page game explanation with eight titles, option help, sample/opponent prompts, two-player rules and rule editor, all menu labels, Tokoton choices and difficulty labels, gameplay effects, ranking titles, true-ending banners, easy course signs, the game-over continue line, easy-course captions, and Nomi markers composed without shared-font rewrites or write conflicts"
                .to_owned(),
            source_path: inputs.source_path.clone(),
            source_sha256,
            output_path: inputs.output_path.clone(),
            output_sha256,
            story,
            demo_titles,
            menu,
            tokoton_choices,
            gameplay,
            ranking_ending,
            course_continue,
            caption_marker,
            story_owned_changed_bytes,
            ui_owned_changed_bytes,
            changed_bytes,
            write_conflicts: 0,
            checksum_hex: format!("0x{checksum:04X}"),
            diff_confined_to_story_and_registered_ui_writes,
        },
        encoding,
    ))
}

fn registered_ui_write(offset: usize) -> Option<&'static str> {
    if PROMPT_STREAM.contains(&offset) {
        Some("mode prompt stream")
    } else if MODE_STREAM.contains(&offset) {
        Some("inherited mode-label stream")
    } else if REMIX_STREAM.contains(&offset) {
        Some("Remix-only menu-label stream")
    } else if crate::remix_menu_graphics::MENU_MAP_HOOK_RANGE.contains(&offset) {
        Some("multi-menu tilemap hook")
    } else if crate::remix_menu_graphics::MENU_MAP_CODE_RANGE.contains(&offset) {
        Some("multi-menu tilemap hook code")
    } else if TOKOTON_CHOICE_STREAM.contains(&offset) {
        Some("Tokoton choice stream")
    } else if SHARED_OBJ_STREAM.contains(&offset) {
        Some("shared gameplay OBJ stream")
    } else if ZENKESHI_SOURCE_STREAM.contains(&offset) {
        Some("all-clear source stream")
    } else if PAUSE_STREAM.contains(&offset) {
        Some("pause stream")
    } else if ZENKESHI_FRAME.contains(&offset) {
        Some("all-clear frame data")
    } else if let Some(owner) = crate::tokoton_difficulty::registered_write(offset) {
        Some(owner)
    } else if let Some(owner) = crate::remix_ranking_ending_graphics::registered_write(offset) {
        Some(owner)
    } else if let Some(owner) = crate::remix_course_continue_graphics::registered_write(offset) {
        Some(owner)
    } else if let Some(owner) = crate::remix_caption_marker_graphics::registered_write(offset) {
        Some(owner)
    } else if crate::demo_explanation::title_chr_range().contains(&offset) {
        Some("game-explanation title CHR")
    } else if crate::demo_explanation::title_map_range().contains(&offset) {
        Some("game-explanation title tilemaps")
    } else {
        None
    }
}

fn checksum_range() -> Range<usize> {
    CHECKSUM_PC..CHECKSUM_PC + 4
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
