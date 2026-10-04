use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::story_codec::StoryToken;

const CATALOG_TABLE_PC: usize = 0x0000_53D0;
const SETTINGS_TABLE_PC: usize = 0x0000_5690;
const SETTINGS_TERMINATOR: u16 = 0x03FF;
const SHARED_OPTION_SOURCE: u16 = 0xD6E0;
const SHARED_OPTION_TABLE_INDEX: usize = 2;
const RELOCATION_END_PC: usize = 0x0000_7F00;
const LINE_CHARACTER_CAP: usize = 14;

#[derive(Clone, Copy)]
enum Payload {
    Korean(&'static [&'static str]),
    /// Keep the original fall-through chain: the Korean sample-select block is
    /// repeated here while its catalog pointer stays with `sample_select_text`.
    SampleSelectCopy,
    /// The settings entry points at the option-help relocation of the same source.
    SharedOptionHelp,
}

#[derive(Clone, Copy)]
struct Entry {
    table: &'static str,
    index: usize,
    address: u16,
    source_len: usize,
    source_sha256: &'static str,
    retarget_pointer: bool,
    payload: Payload,
}

const PORT_WARNING: &[&str] = &["멀티탭은", "1번 단자에", "연결하지 마세요."];
const MOUSE_WARNING: &[&str] = &["멀티탭에는", "마우스를 연결하지 마세요."];
const MISSING_TAP: &[&str] = &["멀티탭이 없으면,", "이 게임을 할 수 없어요."];
const ALT_MENU: &[&str] = &[
    "뭘 할까요?",
    "게임 시작",
    "규칙 만들기",
    "규칙 선택으로",
    "처음으로 돌아가기",
];
const STAGE_SELECT_HINT: &[&str] = &[
    "좋은 걸 알려 줄게.",
    "2 모드에서",
    "R, L, 스타트를",
    "누르면서 시작하면,",
    "원하는 스테이지를",
    "고를 수 있게 돼.",
];

const SETTINGS_EXIT: &[&str] = &["설정 끝내기."];
const SETTINGS_PUYO_TYPE: &[&str] = &["뿌요의 모양 선택."];
const SETTINGS_NEXT_SIZE: &[&str] = &["다음 뿌요의 크기 선택."];
const SETTINGS_HUMAN: &[&str] = &[
    "왼쪽부터 몇 명을",
    "사람이 조작할지.",
    "남은 자리는",
    "컴퓨터가 맡는다.",
    "컴퓨터가 맡은",
    "자리는",
    "컨트롤러1의 R, L로",
    "게임 오버시킬 수 있다.",
    "그리고 여기를 0이나 1로",
    "하면,",
    "다른 게임에서도 그 자리를",
    "컴퓨터가 맡는다.",
    "무슨 일이 생길지는",
    "직접 해 봐.",
];
const SETTINGS_PLAYER_1: &[&str] = &["플레이어1 자리 종류."];
const SETTINGS_PLAYER_2: &[&str] = &["플레이어2 자리 종류."];
const SETTINGS_PLAYER_3: &[&str] = &["플레이어3 자리 종류."];
const SETTINGS_PLAYER_4: &[&str] = &["플레이어4 자리 종류."];

// Every group is written in its original contiguous order so a text object
// that keeps reading after `FF00` sees the same next block as in the source.
const CATALOG_ENTRIES: [Entry; 6] = [
    Entry {
        table: "catalog",
        index: 2,
        address: 0xD4D5,
        source_len: 43,
        source_sha256: "ec9f28d8678619b959ab5010162180416cb6990f924fa85d1f6a94d59dc1a748",
        retarget_pointer: true,
        payload: Payload::Korean(PORT_WARNING),
    },
    Entry {
        table: "catalog",
        index: 3,
        address: 0xD500,
        source_len: 32,
        source_sha256: "b94117354f3c037a3fff1af23e8706e16b9532422272a309e331ce04371fa883",
        retarget_pointer: true,
        payload: Payload::Korean(MOUSE_WARNING),
    },
    Entry {
        table: "catalog",
        index: 4,
        address: 0xD520,
        source_len: 33,
        source_sha256: "98f13e3e3da400d2dff3d4e0b88832eb819b0f768c7a00ccf673aa8d8cf96641",
        retarget_pointer: true,
        payload: Payload::Korean(MISSING_TAP),
    },
    Entry {
        table: "catalog",
        index: 5,
        address: 0xD541,
        source_len: 62,
        source_sha256: "85ead02ddd303c4295510a837519f4f5835db1ccc3d02434e2d188d395674ccb",
        retarget_pointer: true,
        payload: Payload::Korean(ALT_MENU),
    },
    Entry {
        table: "catalog",
        index: 6,
        address: 0xD57F,
        source_len: 37,
        source_sha256: "40efc13a203851c6578ea9d3e35cc88e691ea9ee2e28258ce058a499feb1851f",
        retarget_pointer: false,
        payload: Payload::SampleSelectCopy,
    },
    Entry {
        table: "catalog",
        index: 7,
        address: 0xD5A4,
        source_len: 108,
        source_sha256: "c8979ccdb2fbee26eb54657474788695e5881ec85ff1d3a98df3fa3d3e83f373",
        retarget_pointer: true,
        payload: Payload::Korean(STAGE_SELECT_HINT),
    },
];

const SETTINGS_ENTRIES: [Entry; 9] = [
    Entry {
        table: "settings",
        index: 0,
        address: 0xDF4F,
        source_len: 25,
        source_sha256: "8a4a5b5c909ae305395e60b269bc2013e1fe6ee5b44d0c3405f92ccd03b22439",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_EXIT),
    },
    Entry {
        table: "settings",
        index: 1,
        address: SHARED_OPTION_SOURCE,
        source_len: 109,
        source_sha256: "51503fe6195bdd00cd40da8f87a2e65ec4d0f90c94e3d5ebc5ded23250074a30",
        retarget_pointer: true,
        payload: Payload::SharedOptionHelp,
    },
    Entry {
        table: "settings",
        index: 2,
        address: 0xDF68,
        source_len: 29,
        source_sha256: "a7249d04eceeaf6a423f4b36f58be21e3600db1a9dfd1f53ecef05f28e388050",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_PUYO_TYPE),
    },
    Entry {
        table: "settings",
        index: 3,
        address: 0xDF85,
        source_len: 32,
        source_sha256: "29aafda799b16683459571e7fcf0c5923422a5dee66a8e05d0af3fd82afb8d46",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_NEXT_SIZE),
    },
    Entry {
        table: "settings",
        index: 4,
        address: 0xDFA5,
        source_len: 275,
        source_sha256: "7e4135a8ff00aa127808b39076f88a201df4bff43384b62553c38a209ac3da1d",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_HUMAN),
    },
    Entry {
        table: "settings",
        index: 5,
        address: 0xE0B8,
        source_len: 31,
        source_sha256: "318159260ee2461383df709cb42a4fbb42ed930f68cc7c0632e2f82d70a0f63d",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_PLAYER_1),
    },
    Entry {
        table: "settings",
        index: 6,
        address: 0xE0D7,
        source_len: 31,
        source_sha256: "6fecc233d8489bccea1d39f109593990c75b71ac470a1a5f581efeb62cb3fe29",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_PLAYER_2),
    },
    Entry {
        table: "settings",
        index: 7,
        address: 0xE0F6,
        source_len: 31,
        source_sha256: "17bdf9ab33fb1ea48f1eda039a1c8503075db6afa34687d32a19b31499293b64",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_PLAYER_3),
    },
    Entry {
        table: "settings",
        index: 8,
        address: 0xE115,
        source_len: 31,
        source_sha256: "61a9fd151fa436bf96103beb95eb082e3fa6f5591fe58650b1e45e5c22cddb54",
        retarget_pointer: true,
        payload: Payload::Korean(SETTINGS_PLAYER_4),
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct EntryReport {
    pub table: String,
    pub index: usize,
    pub source_lorom: String,
    pub korean_lines: Vec<String>,
    pub encoded_len: usize,
    pub target_lorom: String,
    pub pointer_owner: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub catalog_table_pc: String,
    pub settings_table_pc: String,
    pub entries: Vec<EntryReport>,
    pub relocation_pc: String,
    pub relocation_bytes_used: usize,
    pub relocation_headroom: usize,
    pub contiguous_after_rule_editor: bool,
    pub controls_preserved: bool,
    pub translation_eligibility: String,
}

pub fn add_required_characters(characters: &mut BTreeSet<char>) {
    for entry in CATALOG_ENTRIES.iter().chain(&SETTINGS_ENTRIES) {
        if let Payload::Korean(lines) = entry.payload {
            characters.extend(lines.iter().flat_map(|line| line.chars()));
        }
    }
}

/// Writes the remaining shared-font catalog entries and the Remix settings
/// ticker directly after the rule-editor relocation that ends at `start_pc`.
pub fn apply(
    source: &[u8],
    patched: &mut [u8],
    codes: &BTreeMap<char, u16>,
    allowed: &mut [bool],
    start_pc: usize,
) -> Result<BuildReport> {
    if source.len() != patched.len() || source.len() != allowed.len() {
        bail!("multi UI text buffers have different ROM lengths");
    }
    verify_sources(source)?;
    let shared_option_target = shared_option_help_target(source, patched)?;

    let mut encoded = Vec::new();
    for entry in CATALOG_ENTRIES.iter().chain(&SETTINGS_ENTRIES) {
        let bytes = match entry.payload {
            Payload::Korean(lines) => {
                let source_pc = entry_pc(entry)?;
                let parsed = crate::story_codec::parse(source, source_pc)?;
                validate_lines(&parsed.tokens, lines, entry)?;
                Some(crate::rule_editor::encode_block(
                    &parsed.tokens,
                    lines,
                    codes,
                )?)
            }
            Payload::SampleSelectCopy => {
                Some(crate::sample_select_text::encode_with_story_codes(codes)?)
            }
            Payload::SharedOptionHelp => None,
        };
        encoded.push((entry, bytes));
    }
    let relocation_len: usize = encoded
        .iter()
        .filter_map(|(_, bytes)| bytes.as_ref().map(Vec::len))
        .sum();
    let end_pc = start_pc + relocation_len;
    if end_pc > RELOCATION_END_PC {
        bail!("multi UI text exceeds the measured Bank $00 relocation range");
    }
    for (label, rom) in [("source", source), ("current derivative", &*patched)] {
        if rom[start_pc..end_pc].iter().any(|byte| *byte != 0xFF) {
            bail!("multi UI relocation is not empty in {label}");
        }
    }

    mark_allowed(allowed, start_pc, relocation_len)?;
    let mut cursor = start_pc;
    let mut entries = Vec::new();
    for (entry, bytes) in encoded {
        let target_address = match &bytes {
            Some(bytes) => {
                let (bank, address) = crate::rom::pc_to_lorom(cursor);
                if bank != 0x00 {
                    bail!("multi UI relocation left physical Bank $00");
                }
                patched[cursor..cursor + bytes.len()].copy_from_slice(bytes);
                let reparsed = crate::story_codec::parse(patched, cursor)?;
                if reparsed.consumed_len != bytes.len() {
                    bail!(
                        "{} index {} failed final grammar round-trip",
                        entry.table,
                        entry.index
                    );
                }
                cursor += bytes.len();
                address
            }
            None => shared_option_target,
        };
        let pointer_owner = if entry.retarget_pointer {
            let pointer_pc = pointer_pc(entry);
            if patched.get(pointer_pc..pointer_pc + 2)
                != Some(entry.address.to_le_bytes().as_slice())
            {
                bail!(
                    "{} pointer index {} was already changed by another owner",
                    entry.table,
                    entry.index
                );
            }
            mark_allowed(allowed, pointer_pc, 2)?;
            patched[pointer_pc..pointer_pc + 2].copy_from_slice(&target_address.to_le_bytes());
            "multi_ui_text"
        } else {
            "sample_select_text"
        };
        entries.push(EntryReport {
            table: entry.table.to_owned(),
            index: entry.index,
            source_lorom: format!("$00:${:04X}", entry.address),
            korean_lines: match entry.payload {
                Payload::Korean(lines) => lines.iter().map(|line| (*line).to_owned()).collect(),
                Payload::SampleSelectCopy => crate::sample_select_text::KOREAN_LABELS
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect(),
                Payload::SharedOptionHelp => vec!["option-help slot 2".to_owned()],
            },
            encoded_len: bytes.as_ref().map_or(0, Vec::len),
            target_lorom: format!("$00:${target_address:04X}"),
            pointer_owner: pointer_owner.to_owned(),
        });
    }
    if cursor != end_pc {
        bail!("multi UI relocation cursor drifted");
    }

    Ok(BuildReport {
        verdict: "multitap warnings, Remix alternate menu and stage-select hint, and the 3/4-player settings ticker relocated after the rule editor".to_owned(),
        catalog_table_pc: format!("0x{CATALOG_TABLE_PC:06X}"),
        settings_table_pc: format!("0x{SETTINGS_TABLE_PC:06X}"),
        entries,
        relocation_pc: format!("0x{start_pc:06X}-0x{:06X}", end_pc - 1),
        relocation_bytes_used: relocation_len,
        relocation_headroom: RELOCATION_END_PC - end_pc,
        contiguous_after_rule_editor: true,
        controls_preserved: true,
        translation_eligibility: "poc_only_needs_review".to_owned(),
    })
}

fn verify_sources(rom: &[u8]) -> Result<()> {
    for entry in CATALOG_ENTRIES.iter().chain(&SETTINGS_ENTRIES) {
        let pointer_pc = pointer_pc(entry);
        if rom.get(pointer_pc..pointer_pc + 2) != Some(entry.address.to_le_bytes().as_slice()) {
            bail!(
                "Remix {} pointer index {} differs from the measured spec",
                entry.table,
                entry.index
            );
        }
        let source_pc = entry_pc(entry)?;
        let parsed = crate::story_codec::parse(rom, source_pc)?;
        if parsed.consumed_len != entry.source_len {
            bail!(
                "Remix {} index {} boundary differs from the measured spec",
                entry.table,
                entry.index
            );
        }
        let actual = format!(
            "{:x}",
            Sha256::digest(&rom[source_pc..source_pc + entry.source_len])
        );
        if actual != entry.source_sha256 {
            bail!(
                "Remix {} index {} source differs from the measured spec: {actual}",
                entry.table,
                entry.index
            );
        }
    }
    let terminator_pc = SETTINGS_TABLE_PC + SETTINGS_ENTRIES.len() * 2;
    if rom.get(terminator_pc..terminator_pc + 2)
        != Some(SETTINGS_TERMINATOR.to_le_bytes().as_slice())
    {
        bail!("Remix settings ticker table does not end where measured");
    }
    // The settings group is one contiguous source run apart from the shared option block.
    let mut expected_next = None;
    for entry in SETTINGS_ENTRIES
        .iter()
        .filter(|entry| !matches!(entry.payload, Payload::SharedOptionHelp))
    {
        let start = entry_pc(entry)?;
        if let Some(expected) = expected_next
            && expected != start
        {
            bail!(
                "Remix settings ticker source run is not contiguous at $00:${:04X}",
                entry.address
            );
        }
        expected_next = Some(start + entry.source_len);
    }
    Ok(())
}

fn shared_option_help_target(source: &[u8], patched: &[u8]) -> Result<u16> {
    let pointer_pc = crate::option_help::TABLE_PC + SHARED_OPTION_TABLE_INDEX * 2;
    let before = u16::from_le_bytes([source[pointer_pc], source[pointer_pc + 1]]);
    let after = u16::from_le_bytes([patched[pointer_pc], patched[pointer_pc + 1]]);
    if before != SHARED_OPTION_SOURCE || after == SHARED_OPTION_SOURCE {
        bail!("option-help slot 2 must be relocated before the settings ticker shares it");
    }
    let target_pc =
        crate::rom::lorom_to_pc(0x00, after).context("option-help slot 2 target is not LoROM")?;
    let parsed = crate::story_codec::parse(patched, target_pc)?;
    let source_pc =
        crate::rom::lorom_to_pc(0x00, before).context("option-help slot 2 source is not LoROM")?;
    let source_parsed = crate::story_codec::parse(source, source_pc)?;
    if controls(&parsed.tokens) != controls(&source_parsed.tokens) {
        bail!("relocated option-help slot 2 no longer carries the shared settings controls");
    }
    Ok(after)
}

fn validate_lines(tokens: &[StoryToken], lines: &[&str], entry: &Entry) -> Result<()> {
    let positions = tokens
        .iter()
        .filter_map(|token| match token {
            StoryToken::Control { code: 0x02, args } if args.len() == 2 => {
                Some(u16::from_le_bytes([args[0], args[1]]))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if positions.len() != lines.len() {
        bail!(
            "{} index {} has {} source lines but {} Korean lines",
            entry.table,
            entry.index,
            positions.len(),
            lines.len()
        );
    }
    for (position, line) in positions.into_iter().zip(lines) {
        let start_column = usize::from(position / 2) % 32;
        let limit = ((32 - start_column) / 2).min(LINE_CHARACTER_CAP);
        if line.chars().count() > limit {
            bail!(
                "{} index {} line {line:?} exceeds {limit} characters at column {start_column}",
                entry.table,
                entry.index
            );
        }
    }
    Ok(())
}

fn controls(tokens: &[StoryToken]) -> Vec<(u8, Vec<u8>)> {
    tokens
        .iter()
        .filter_map(|token| match token {
            StoryToken::Control { code, args } => Some((*code, args.clone())),
            StoryToken::Glyph { .. } => None,
        })
        .collect()
}

fn entry_pc(entry: &Entry) -> Result<usize> {
    crate::rom::lorom_to_pc(0x00, entry.address)
        .with_context(|| format!("invalid {} source ${:04X}", entry.table, entry.address))
}

fn pointer_pc(entry: &Entry) -> usize {
    let table = if entry.table == "catalog" {
        CATALOG_TABLE_PC
    } else {
        SETTINGS_TABLE_PC
    };
    table + entry.index * 2
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    let end = start
        .checked_add(len)
        .context("multi UI Expected Write overflow")?;
    allowed
        .get_mut(start..end)
        .with_context(|| format!("multi UI Expected Write 0x{start:06X}..0x{end:06X} outside ROM"))?
        .fill(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn remix_multi_ui_sources_match_the_measured_spec() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        verify_sources(&rom).unwrap();
    }
}
