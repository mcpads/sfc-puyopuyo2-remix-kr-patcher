use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::story_codec::StoryToken;

const POINTER_TABLE_PC: usize = 0x0000_53D0;
const ORIGINAL_POINTERS: [u16; 8] = [
    0xD3E0, 0xD41E, 0xD4D5, 0xD500, 0xD520, 0xD541, 0xD57F, 0xD5A4,
];
const MENU_POINTER_INDEX: usize = 0;
const EDITOR_POINTER_INDEX: usize = 1;
const MENU_POINTER_PC: usize = POINTER_TABLE_PC + MENU_POINTER_INDEX * 2;
const EDITOR_POINTER_PC: usize = POINTER_TABLE_PC + EDITOR_POINTER_INDEX * 2;
const MENU_SOURCE_PC: usize = 0x0000_53E0;
const MENU_SOURCE_LEN: usize = 62;
const MENU_SOURCE_SHA256: &str = "e86f21ad2ac2b9c33da897d4c97fb96afb35067c41becb3b7ec01112681c686f";
const EDITOR_SOURCE_PC: usize = 0x0000_541E;
const EDITOR_SOURCE_LEN: usize = 183;
const EDITOR_SOURCE_SHA256: &str =
    "ddc935716ed09d0dbaf7906097017aa8dfe98d529cbbe6c291dacf60b036f319";
const RELOCATION_PC: usize = 0x0000_75A0;
const RELOCATION_END_PC: usize = 0x0000_7F00;

const KOREAN_MENU_LINES: [&str; 5] = [
    "뭘 할까요?",
    "게임 시작",
    "규칙 만들기",
    "규칙 선택으로",
    "타이틀로 돌아가기",
];
const KOREAN_LINES: [&str; 10] = [
    "규칙 만드는 중",
    "끝내기",
    "규칙을 복사한다     에서",
    "지우는 뿌요 수      개",
    "방해 1개 내릴 점수   점",
    "방해를 지운 점수     점",
    "방해 뿌요 종류",
    "연쇄 배율 방식",
    "싹쓸이 보너스       개",
    " 보통 단단  두겹  ",
];

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub catalog_pointer_table_pc: String,
    pub catalog_entries: usize,
    pub menu_pointer_pc: String,
    pub editor_pointer_pc: String,
    pub menu_source_pc: String,
    pub menu_source_sha256: String,
    pub editor_source_pc: String,
    pub editor_source_sha256: String,
    pub korean_menu_lines: Vec<String>,
    pub korean_lines: Vec<String>,
    pub menu_encoded_len: usize,
    pub editor_encoded_len: usize,
    pub menu_relocation_pc: String,
    pub menu_relocation_lorom: String,
    pub editor_relocation_pc: String,
    pub editor_relocation_lorom: String,
    pub contiguous_runtime_handoff: bool,
    pub controls_preserved: bool,
    #[serde(skip)]
    pub relocation_end_pc: usize,
}

pub fn add_required_characters(characters: &mut BTreeSet<char>) {
    characters.extend(KOREAN_MENU_LINES.iter().flat_map(|line| line.chars()));
    characters.extend(KOREAN_LINES.iter().flat_map(|line| line.chars()));
}

pub fn apply(
    source: &[u8],
    patched: &mut [u8],
    codes: &BTreeMap<char, u16>,
    allowed: &mut [bool],
) -> Result<BuildReport> {
    if source.len() != patched.len() || source.len() != allowed.len() {
        bail!("rule-editor buffers have different ROM lengths");
    }
    verify_sources(source)?;
    for (label, pointer_pc, pointer_index) in [
        ("menu", MENU_POINTER_PC, MENU_POINTER_INDEX),
        ("rule-editor", EDITOR_POINTER_PC, EDITOR_POINTER_INDEX),
    ] {
        if patched.get(pointer_pc..pointer_pc + 2)
            != Some(ORIGINAL_POINTERS[pointer_index].to_le_bytes().as_slice())
        {
            bail!("{label} pointer was already changed by another owner");
        }
    }

    let menu_source = crate::story_codec::parse(source, MENU_SOURCE_PC)?;
    let editor_source = crate::story_codec::parse(source, EDITOR_SOURCE_PC)?;
    let menu_encoded = encode_block(&menu_source.tokens, &KOREAN_MENU_LINES, codes)?;
    let editor_encoded = encode_block(&editor_source.tokens, &KOREAN_LINES, codes)?;
    let editor_relocation_pc = RELOCATION_PC + menu_encoded.len();
    let relocation_end = editor_relocation_pc + editor_encoded.len();
    if relocation_end > RELOCATION_END_PC {
        bail!(
            "Korean multi-player menu and rule editor exceed the measured Bank $00 relocation range"
        );
    }
    for (label, rom) in [("source", source), ("current derivative", &*patched)] {
        if rom[RELOCATION_PC..relocation_end]
            .iter()
            .any(|byte| *byte != 0xFF)
        {
            bail!("rule-editor relocation is not empty in {label}");
        }
    }

    let (menu_bank, menu_address) = crate::rom::pc_to_lorom(RELOCATION_PC);
    let (editor_bank, editor_address) = crate::rom::pc_to_lorom(editor_relocation_pc);
    if menu_bank != 0x00 || editor_bank != 0x00 {
        bail!("rule-editor relocation left physical Bank $00");
    }
    mark_allowed(allowed, MENU_POINTER_PC, 2)?;
    mark_allowed(allowed, EDITOR_POINTER_PC, 2)?;
    mark_allowed(allowed, RELOCATION_PC, relocation_end - RELOCATION_PC)?;
    patched[RELOCATION_PC..editor_relocation_pc].copy_from_slice(&menu_encoded);
    patched[editor_relocation_pc..relocation_end].copy_from_slice(&editor_encoded);
    patched[MENU_POINTER_PC..MENU_POINTER_PC + 2].copy_from_slice(&menu_address.to_le_bytes());
    patched[EDITOR_POINTER_PC..EDITOR_POINTER_PC + 2]
        .copy_from_slice(&editor_address.to_le_bytes());

    let reparsed_menu = crate::story_codec::parse(patched, RELOCATION_PC)?;
    let reparsed_editor = crate::story_codec::parse(patched, editor_relocation_pc)?;
    if reparsed_menu.consumed_len != menu_encoded.len()
        || reparsed_editor.consumed_len != editor_encoded.len()
        || patched.get(RELOCATION_PC..editor_relocation_pc) != Some(menu_encoded.as_slice())
        || patched.get(editor_relocation_pc..relocation_end) != Some(editor_encoded.as_slice())
    {
        bail!("Korean multi-player menu or rule editor failed final grammar round-trip");
    }

    Ok(BuildReport {
        verdict: "Korean multi-player menu and rule-editor heading plus all nine settings inserted with their runtime-contiguous handoff preserved".to_owned(),
        catalog_pointer_table_pc: format!("0x{POINTER_TABLE_PC:06X}"),
        catalog_entries: ORIGINAL_POINTERS.len(),
        menu_pointer_pc: format!("0x{MENU_POINTER_PC:06X}"),
        editor_pointer_pc: format!("0x{EDITOR_POINTER_PC:06X}"),
        menu_source_pc: format!("0x{MENU_SOURCE_PC:06X}"),
        menu_source_sha256: MENU_SOURCE_SHA256.to_owned(),
        editor_source_pc: format!("0x{EDITOR_SOURCE_PC:06X}"),
        editor_source_sha256: EDITOR_SOURCE_SHA256.to_owned(),
        korean_menu_lines: KOREAN_MENU_LINES
            .iter()
            .map(|line| (*line).to_owned())
            .collect(),
        korean_lines: KOREAN_LINES.iter().map(|line| (*line).to_owned()).collect(),
        menu_encoded_len: menu_encoded.len(),
        editor_encoded_len: editor_encoded.len(),
        menu_relocation_pc: format!("0x{RELOCATION_PC:06X}-0x{:06X}", editor_relocation_pc - 1),
        menu_relocation_lorom: format!(
            "$00:${menu_address:04X}-$00:${:04X}",
            menu_address as usize + menu_encoded.len() - 1
        ),
        editor_relocation_pc: format!(
            "0x{editor_relocation_pc:06X}-0x{:06X}",
            relocation_end - 1
        ),
        editor_relocation_lorom: format!(
            "$00:${editor_address:04X}-$00:${:04X}",
            editor_address as usize + editor_encoded.len() - 1
        ),
        contiguous_runtime_handoff: true,
        controls_preserved: true,
        relocation_end_pc: relocation_end,
    })
}

fn verify_sources(rom: &[u8]) -> Result<()> {
    let expected_table = ORIGINAL_POINTERS
        .iter()
        .flat_map(|pointer| pointer.to_le_bytes())
        .collect::<Vec<_>>();
    if rom.get(POINTER_TABLE_PC..POINTER_TABLE_PC + expected_table.len())
        != Some(expected_table.as_slice())
    {
        bail!("Remix shared-font UI pointer catalog differs from the measured spec");
    }
    for (label, source_pc, source_len, expected_sha256) in [
        (
            "multi-player menu",
            MENU_SOURCE_PC,
            MENU_SOURCE_LEN,
            MENU_SOURCE_SHA256,
        ),
        (
            "rule-editor",
            EDITOR_SOURCE_PC,
            EDITOR_SOURCE_LEN,
            EDITOR_SOURCE_SHA256,
        ),
    ] {
        let source = rom
            .get(source_pc..source_pc + source_len)
            .with_context(|| format!("Remix {label} source block is outside ROM"))?;
        let actual_sha256 = format!("{:x}", Sha256::digest(source));
        if actual_sha256 != expected_sha256 {
            bail!("Remix {label} source block differs from the measured spec: {actual_sha256}");
        }
        let parsed = crate::story_codec::parse(rom, source_pc)?;
        if parsed.consumed_len != source_len {
            bail!("Remix {label} source boundary differs from the measured spec");
        }
    }
    if rom[RELOCATION_PC..RELOCATION_END_PC]
        .iter()
        .any(|byte| *byte != 0xFF)
    {
        bail!("Remix Bank $00 rule-editor relocation range is not empty");
    }
    Ok(())
}

pub(crate) fn encode_block(
    tokens: &[StoryToken],
    lines: &[&str],
    codes: &BTreeMap<char, u16>,
) -> Result<Vec<u8>> {
    let line_count = tokens
        .iter()
        .filter(|token| matches!(token, StoryToken::Control { code: 0x02, .. }))
        .count();
    if line_count != lines.len() {
        bail!("shared-font source line population differs from the Korean translation");
    }

    let mut output = Vec::new();
    let mut line_index = 0usize;
    let mut saw_line = false;
    let mut expected_controls = Vec::new();
    for token in tokens {
        match token {
            StoryToken::Control { code, args } => {
                output.extend_from_slice(&[0xFF, *code]);
                output.extend_from_slice(args);
                expected_controls.push((*code, args.clone()));
                if *code == 0x02 {
                    let line = lines
                        .get(line_index)
                        .context("shared-font translation line index overflow")?;
                    validate_line_width(args, line)?;
                    encode_visible(line, codes, &mut output)?;
                    line_index += 1;
                    saw_line = true;
                }
            }
            StoryToken::Glyph { .. } if saw_line => {}
            StoryToken::Glyph { .. } => {
                bail!("rule-editor source glyph appears before the first line control")
            }
        }
    }
    if line_index != lines.len() {
        bail!("shared-font translation did not consume every line");
    }

    let parsed = crate::story_codec::parse(&output, 0)?;
    let actual_controls = parsed
        .tokens
        .iter()
        .filter_map(|token| match token {
            StoryToken::Control { code, args } => Some((*code, args.clone())),
            StoryToken::Glyph { .. } => None,
        })
        .collect::<Vec<_>>();
    if parsed.consumed_len != output.len() || actual_controls != expected_controls {
        bail!("rule-editor translation changed protected controls");
    }
    Ok(output)
}

fn validate_line_width(position_args: &[u8], line: &str) -> Result<()> {
    if position_args.len() != 2 {
        bail!("rule-editor FF02 position is not a word");
    }
    let position = u16::from_le_bytes([position_args[0], position_args[1]]);
    let start_column = usize::from(position / 2) % 32;
    let max_characters = (32 - start_column) / 2;
    let characters = line.chars().count();
    if characters > max_characters {
        bail!(
            "rule-editor line {line:?} has {characters} characters, maximum at column {start_column} is {max_characters}"
        );
    }
    Ok(())
}

fn encode_visible(text: &str, codes: &BTreeMap<char, u16>, output: &mut Vec<u8>) -> Result<()> {
    for character in text.chars() {
        let code = *codes.get(&character).with_context(|| {
            format!("no shared-font code for rule-editor character {character:?}")
        })?;
        match code {
            0x0000..=0x00FD => output.push(code as u8),
            0x0100..=0x01FF => output.extend_from_slice(&[0xFE, code as u8]),
            _ => bail!("rule-editor character {character:?} has invalid code 0x{code:04X}"),
        }
    }
    Ok(())
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    let end = start
        .checked_add(len)
        .context("rule-editor Expected Write overflow")?;
    allowed
        .get_mut(start..end)
        .with_context(|| {
            format!("rule-editor Expected Write 0x{start:06X}..0x{end:06X} outside ROM")
        })?
        .fill(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn remix_rule_editor_matches_the_measured_spec() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        verify_sources(&rom).unwrap();
    }
}
