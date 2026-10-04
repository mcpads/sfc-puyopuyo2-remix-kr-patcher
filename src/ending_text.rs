use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::story_codec::StoryToken;

const RELOCATION_PC: usize = 0x15_8400;
const RELOCATION_END_PC: usize = 0x15_C000;
const RELOCATION_BANK: u8 = 0x2B;
const RELOCATION_RUNTIME_BANK: u8 = 0xAB;
const GENERIC_LINE_LIMIT: usize = 13;

const NORMAL_FRONT_POINTERS: [u16; 16] = [
    0xACFF, 0xAD01, 0xAD18, 0xAD25, 0xAD2E, 0xAD62, 0xAD70, 0xAD92, 0xAD9E, 0xADB0, 0xADD8, 0xADE5,
    0xADE5, 0xADE5, 0xADE5, 0xADE5,
];
const EASY_POINTERS: [u16; 16] = [
    0x9716, 0x9718, 0x9745, 0x975D, 0x9779, 0x97A6, 0x97AF, 0x97C7, 0x97CF, 0x97F7, 0x982C, 0x9842,
    0x984D, 0x986B, 0x988B, 0x9895,
];
const NORMAL_REVEAL_POINTERS: [u16; 2] = [0xC8F0, 0xC8F2];
const NORMAL_BACK_POINTERS: [u16; 16] = [
    0xC9EF, 0xC9F1, 0xCA96, 0xCAB6, 0xCAD1, 0xCAE7, 0xCB10, 0xCB36, 0xCB42, 0xCB5E, 0xCB70, 0xCB8C,
    0xCB98, 0xCB98, 0xCB98, 0xCB98,
];

// Inherited from the Super Puyo Puyo Tsuu Korean ending text. Three lines are
// reworded so the Remix shared font does not need `겼`, `족`, or `터`.
const NORMAL_FRONT_KO: [Option<&[&str]>; 16] = [
    None,
    Some(&["성공이다!", "내 승리다!"]),
    Some(&["으, 분하다!"]),
    Some(&["어라?"]),
    Some(&[
        "자, 잠깐만!",
        "지면이 움직이고 있어!",
        "무슨 일이야?",
        "사탄!",
    ]),
    Some(&["하하하하하!"]),
    Some(&["이제 둘이서", "우주로 신혼여행을", "떠나는 거다!"]),
    Some(&["말도 안 돼!"]),
    Some(&["내가 이긴 건데!!"]),
    Some(&["뭔지는", "잘 모르겠지만", "살았다…"]),
    Some(&["앗, 사탄."]),
    None,
    None,
    None,
    None,
    None,
];

const EASY_KO: [Option<&[&str]>; 16] = [
    None,
    Some(&["성공이다!", "내 승리다!", "이제 내가", "뿌요 고수다!"]),
    Some(&["후후후… 아직", "멀었다, 아르르."]),
    Some(&["성공이야! 성공이야!", "내가 세계 제일이야!"]),
    Some(&[
        "제법 솜씨가 좋구나.",
        "하지만 정말 이긴",
        "거라고 생각하느냐?",
    ]),
    Some(&["누구지?"]),
    Some(&["어라?", "쓰러진 적이 없어."]),
    Some(&["앗!"]),
    Some(&["이 탑에서 승리해야", "진정한 뿌요 고수가", "되는 것이다."]),
    Some(&[
        "자!",
        "탑으로 올라오너라.",
        "새 도전이",
        "널 기다리고 있다…",
    ]),
    Some(&["좋아! 절대", "지지 않을 거야!!"]),
    Some(&["하지만…"]),
    Some(&["오늘은 여기까지.", "다음에 또 와야지."]),
    Some(&["뭐?", "어이, 기다려!", "잠깐만…"]),
    Some(&["이봐!"]),
    None,
];

// The reveal banner scrolls through four rotating four-cell slots. Every
// Japanese chunk is four characters, so each Korean chunk is packed to four
// characters and trailing chunks clear the ring buffer with spaces.
const MASKED_REVEAL_KO: &[&str] = &[
    "에~거짓",
    "말~!마",
    "스크드사",
    "탄이그잘",
    "생기고다",
    "리긴사탄",
    "이랑같은",
    "사람이라",
    "니~! ",
    "    ",
    "    ",
    "    ",
    "    ",
];

const NORMAL_REVEAL_KO: [Option<&[&str]>; 2] = [None, Some(MASKED_REVEAL_KO)];
const NORMAL_BACK_KO: [Option<&[&str]>; 16] = [
    None,
    Some(MASKED_REVEAL_KO),
    Some(&["후후후!", "속았지!"]),
    Some(&["뭐야?", "역시", "사탄이잖아."]),
    Some(&["카방클,", "돌아갈까?"]),
    Some(&["기다려…", "내가 사탄인", "걸 어떻게", "알았지?"]),
    Some(&["난", "싸우기 전에", "말했잖아.", "그렇지?"]),
    Some(&["말도 안 돼"]),
    Some(&["난 이제", "피곤해.", "잘 있어!"]),
    Some(&["두고", "가지 마~"]),
    Some(&["카방클은", "안 갈", "거지…?"]),
    Some(&["카방클~!"]),
    None,
    None,
    None,
    None,
];

struct TableSpec {
    id: &'static str,
    pointer_pc: usize,
    source_bank: u8,
    bank_operand_pc: usize,
    pointers: &'static [u16],
    translations: &'static [Option<&'static [&'static str]>],
}

// Each table is read by `LDA #bank; STA $10; ... LDA table,Y; ... JSL $80:CFB5`.
// The operand byte sits 0x2B bytes before the table in every Remix routine.
const TABLES: [TableSpec; 4] = [
    TableSpec {
        id: "normal_front",
        pointer_pc: 0x15_2CDF,
        source_bank: 0x2A,
        bank_operand_pc: 0x15_2CB4,
        pointers: &NORMAL_FRONT_POINTERS,
        translations: &NORMAL_FRONT_KO,
    },
    TableSpec {
        id: "easy",
        pointer_pc: 0x10_96F6,
        source_bank: 0x21,
        bank_operand_pc: 0x10_96CB,
        pointers: &EASY_POINTERS,
        translations: &EASY_KO,
    },
    TableSpec {
        id: "normal_reveal",
        pointer_pc: 0x15_48EC,
        source_bank: 0x2A,
        bank_operand_pc: 0x15_48C1,
        pointers: &NORMAL_REVEAL_POINTERS,
        translations: &NORMAL_REVEAL_KO,
    },
    TableSpec {
        id: "normal_back",
        pointer_pc: 0x15_49CF,
        source_bank: 0x2A,
        bank_operand_pc: 0x15_49A4,
        pointers: &NORMAL_BACK_POINTERS,
        translations: &NORMAL_BACK_KO,
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct TableReport {
    pub id: String,
    pub pointer_table_pc: String,
    pub bank_operand_pc: String,
    pub source_bank: String,
    pub pointer_slots: usize,
    pub translated_blocks: usize,
    pub blank_slots: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub tables: Vec<TableReport>,
    pub translated_blocks: usize,
    pub blank_slots: usize,
    pub relocation_pc: String,
    pub relocation_lorom: String,
    pub relocation_bytes_used: usize,
    pub relocation_headroom: usize,
    pub runtime_bank: String,
    pub controls_preserved: bool,
    pub translation_eligibility: String,
}

pub fn add_required_characters(characters: &mut BTreeSet<char>) {
    characters.extend(
        TABLES
            .iter()
            .flat_map(|table| table.translations.iter().flatten())
            .flat_map(|lines| lines.iter())
            .flat_map(|line| line.chars()),
    );
}

pub fn apply(
    source: &[u8],
    patched: &mut [u8],
    codes: &BTreeMap<char, u16>,
    allowed: &mut [bool],
) -> Result<BuildReport> {
    if source.len() != patched.len() || source.len() != allowed.len() {
        bail!("ending text buffers have different ROM lengths");
    }
    let table_reports = audit(source)?;
    for (label, rom) in [("source", source), ("current derivative", &*patched)] {
        if rom[RELOCATION_PC..RELOCATION_END_PC]
            .iter()
            .any(|byte| *byte != 0xFF)
        {
            bail!(
                "ending relocation PC 0x{RELOCATION_PC:06X}..0x{RELOCATION_END_PC:06X} is not empty in {label}"
            );
        }
    }
    for table in &TABLES {
        let table_len = table.pointers.len() * 2;
        if patched.get(table.pointer_pc..table.pointer_pc + table_len)
            != source.get(table.pointer_pc..table.pointer_pc + table_len)
            || patched[table.bank_operand_pc] != source[table.bank_operand_pc]
        {
            bail!(
                "{} ending table was already changed by another owner",
                table.id
            );
        }
    }

    let mut cursor = RELOCATION_PC;
    patched[cursor..cursor + 2].copy_from_slice(&[0xFF, 0x00]);
    let blank_address = crate::rom::pc_to_lorom(cursor).1;
    cursor += 2;
    let mut final_targets = Vec::new();
    for table in &TABLES {
        let mut table_targets = Vec::with_capacity(table.pointers.len());
        for (index, (pointer, translation)) in table
            .pointers
            .iter()
            .copied()
            .zip(table.translations.iter().copied())
            .enumerate()
        {
            let target = if let Some(lines) = translation {
                let source_pc = crate::rom::lorom_to_pc(table.source_bank, pointer)
                    .context("ending pointer is not LoROM")?;
                let parsed = crate::story_codec::parse(source, source_pc)?;
                let encoded =
                    encode_translated_block(&parsed.tokens, lines, codes, table.id, index)
                        .with_context(|| format!("encode {} index {index}", table.id))?;
                if cursor + encoded.len() > RELOCATION_END_PC {
                    bail!("ending text relocation exceeds its assigned Bank $2B range");
                }
                let target_pc = cursor;
                patched[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
                cursor += encoded.len();
                let (bank, address) = crate::rom::pc_to_lorom(target_pc);
                if bank != RELOCATION_BANK {
                    bail!("ending target left physical Bank $2B");
                }
                final_targets.push((target_pc, encoded));
                address
            } else {
                blank_address
            };
            table_targets.push(target);
        }
        mark_allowed(allowed, table.pointer_pc, table_targets.len() * 2)?;
        for (slot, target) in table_targets.into_iter().enumerate() {
            let pc = table.pointer_pc + slot * 2;
            patched[pc..pc + 2].copy_from_slice(&target.to_le_bytes());
        }
        mark_allowed(allowed, table.bank_operand_pc, 1)?;
        patched[table.bank_operand_pc] = RELOCATION_RUNTIME_BANK;
    }
    mark_allowed(allowed, RELOCATION_PC, cursor - RELOCATION_PC)?;

    for (target_pc, encoded) in &final_targets {
        let parsed = crate::story_codec::parse(patched, *target_pc)?;
        if parsed.consumed_len != encoded.len()
            || patched.get(*target_pc..*target_pc + encoded.len()) != Some(encoded.as_slice())
        {
            bail!("relocated ending text at PC 0x{target_pc:06X} failed final round-trip");
        }
    }

    let (_, start_address) = crate::rom::pc_to_lorom(RELOCATION_PC);
    let (_, end_address) = crate::rom::pc_to_lorom(cursor - 1);
    Ok(BuildReport {
        verdict: "easy, normal, and true-ending text relocated to Bank $2B with protected controls"
            .to_owned(),
        translated_blocks: table_reports
            .iter()
            .map(|table| table.translated_blocks)
            .sum(),
        blank_slots: table_reports.iter().map(|table| table.blank_slots).sum(),
        tables: table_reports,
        relocation_pc: format!("0x{RELOCATION_PC:06X}-0x{:06X}", cursor - 1),
        relocation_lorom: format!("$2B:${start_address:04X}-$2B:${end_address:04X}"),
        relocation_bytes_used: cursor - RELOCATION_PC,
        relocation_headroom: RELOCATION_END_PC - cursor,
        runtime_bank: format!("${RELOCATION_RUNTIME_BANK:02X}"),
        controls_preserved: true,
        translation_eligibility: "poc_only_needs_review".to_owned(),
    })
}

fn audit(rom: &[u8]) -> Result<Vec<TableReport>> {
    let mut reports = Vec::with_capacity(TABLES.len());
    for table in &TABLES {
        verify_table_header(rom, table)?;
        let mut translated_blocks = 0usize;
        let mut blank_slots = 0usize;
        for (index, (pointer, translation)) in table
            .pointers
            .iter()
            .copied()
            .zip(table.translations.iter().copied())
            .enumerate()
        {
            let source_pc = crate::rom::lorom_to_pc(table.source_bank, pointer)
                .with_context(|| format!("{} pointer ${pointer:04X} is not LoROM", table.id))?;
            let block = crate::story_codec::parse(rom, source_pc)
                .with_context(|| format!("parse {} index {index}", table.id))?;
            match translation {
                Some(lines) => {
                    let source_lines = count_lines(&block.tokens);
                    if source_lines != lines.len() {
                        bail!(
                            "{} index {index} has {source_lines} source lines but {} Korean lines",
                            table.id,
                            lines.len()
                        );
                    }
                    if let Some(slot) = scroll_banner_slot_width(&block.tokens) {
                        for line in lines.iter() {
                            if line.chars().count() > slot {
                                bail!(
                                    "{} index {index} line {line:?} exceeds the {slot}-cell scrolling banner slot",
                                    table.id
                                );
                            }
                        }
                    }
                    translated_blocks += 1;
                }
                None => {
                    if block.consumed_len != 2 {
                        bail!(
                            "{} index {index} is non-empty but has no Korean translation",
                            table.id
                        );
                    }
                    blank_slots += 1;
                }
            }
        }
        reports.push(TableReport {
            id: table.id.to_owned(),
            pointer_table_pc: format!("0x{:06X}", table.pointer_pc),
            bank_operand_pc: format!("0x{:06X}", table.bank_operand_pc),
            source_bank: format!("${:02X}", table.source_bank),
            pointer_slots: table.pointers.len(),
            translated_blocks,
            blank_slots,
        });
    }
    Ok(reports)
}

fn encode_translated_block(
    tokens: &[StoryToken],
    lines: &[&str],
    codes: &BTreeMap<char, u16>,
    table: &str,
    entry_index: usize,
) -> Result<Vec<u8>> {
    if count_lines(tokens) != lines.len() {
        bail!("Korean line count differs from source control count");
    }
    let mut output = Vec::new();
    let mut expected_controls = Vec::new();
    let mut line_index = 0usize;
    let mut skip_source_glyphs = false;
    for token in tokens {
        match token {
            StoryToken::Control { code, args } => {
                let mut args = args.clone();
                if *code == 0x02 {
                    let columns_left = line_shift_columns_left(table, entry_index, line_index);
                    if columns_left != 0 {
                        if args.len() != 2 {
                            bail!("ending FF02 position is not a word");
                        }
                        let position = u16::from_le_bytes([args[0], args[1]]);
                        let shifted = position
                            .checked_sub(columns_left * 2)
                            .context("ending line shift underflow")?;
                        args.copy_from_slice(&shifted.to_le_bytes());
                    }
                }
                output.extend_from_slice(&[0xFF, *code]);
                output.extend_from_slice(&args);
                skip_source_glyphs = *code == 0x02;
                if *code == 0x02 {
                    let line = lines
                        .get(line_index)
                        .context("ending translation line index overflow")?;
                    if line.chars().count() > GENERIC_LINE_LIMIT {
                        bail!(
                            "ending line {line:?} exceeds the {GENERIC_LINE_LIMIT}-character margin"
                        );
                    }
                    validate_fixed_window_line(table, entry_index, &args, line)?;
                    encode_visible(line, codes, &mut output)?;
                    line_index += 1;
                }
                expected_controls.push((*code, args));
            }
            StoryToken::Glyph { .. } if skip_source_glyphs => {}
            StoryToken::Glyph { .. } => bail!("ending source has a glyph before its first FF02"),
        }
    }
    if line_index != lines.len() {
        bail!("ending translation did not consume every line");
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
        bail!("ending translation changed protected controls or failed round-trip");
    }
    Ok(output)
}

fn encode_visible(text: &str, codes: &BTreeMap<char, u16>, output: &mut Vec<u8>) -> Result<()> {
    for character in text.chars() {
        let code = *codes
            .get(&character)
            .with_context(|| format!("no shared-font code for ending character {character:?}"))?;
        match code {
            0x0000..=0x00FD => output.push(code as u8),
            0x0100..=0x01FF => output.extend_from_slice(&[0xFE, code as u8]),
            _ => bail!("ending character {character:?} has invalid code 0x{code:04X}"),
        }
    }
    Ok(())
}

fn line_shift_columns_left(table: &str, entry_index: usize, line_index: usize) -> u16 {
    // The inherited Easy success lines need one 8x8 tile column of extra margin.
    match (table, entry_index, line_index) {
        ("easy", 3, 0 | 1) => 1,
        _ => 0,
    }
}

fn validate_fixed_window_line(
    table: &str,
    entry_index: usize,
    position_args: &[u8],
    line: &str,
) -> Result<()> {
    if table == "normal_back" && matches!(entry_index, 5..=7) && line.chars().count() > 6 {
        bail!(
            "ending {table} index {entry_index} line {line:?} exceeds the inherited 6-character fixed-window margin"
        );
    }
    // Inherited runtime measurement: column 27 is the exclusive right edge of
    // these Easy ending windows; each 16x16 glyph advances two tilemap columns.
    let Some(right_exclusive_column) = (match (table, entry_index) {
        ("easy", 2 | 3 | 10 | 12) => Some(27),
        _ => None,
    }) else {
        return Ok(());
    };
    if position_args.len() != 2 {
        bail!("ending FF02 position is not a word");
    }
    let position = u16::from_le_bytes([position_args[0], position_args[1]]);
    let start_column = usize::from(position / 2) % 32;
    let end_column = start_column + line.chars().count() * 2;
    if end_column > right_exclusive_column {
        bail!(
            "ending {table} index {entry_index} line {line:?} ends at column {end_column}, beyond fixed-window column {right_exclusive_column}"
        );
    }
    Ok(())
}

fn count_lines(tokens: &[StoryToken]) -> usize {
    tokens
        .iter()
        .filter(|token| matches!(token, StoryToken::Control { code: 0x02, .. }))
        .count()
}

/// Blocks that advance with `FF01` rotate their `FF02` slots, so each Korean
/// chunk must fit its widest Japanese slot instead of the generic line margin.
fn scroll_banner_slot_width(tokens: &[StoryToken]) -> Option<usize> {
    if !tokens
        .iter()
        .any(|token| matches!(token, StoryToken::Control { code: 0x01, .. }))
    {
        return None;
    }
    let mut widths = Vec::new();
    let mut current = 0usize;
    let mut started = false;
    for token in tokens {
        match token {
            StoryToken::Control { code: 0x02, .. } => {
                if started {
                    widths.push(current);
                }
                started = true;
                current = 0;
            }
            StoryToken::Glyph { .. } if started => current += 1,
            _ => {}
        }
    }
    if started {
        widths.push(current);
    }
    widths.into_iter().max()
}

fn verify_table_header(rom: &[u8], table: &TableSpec) -> Result<()> {
    if table.pointers.len() != table.translations.len() {
        bail!("{} pointer/translation slot count differs", table.id);
    }
    for (index, expected) in table.pointers.iter().copied().enumerate() {
        let pc = table.pointer_pc + index * 2;
        if rom.get(pc..pc + 2) != Some(expected.to_le_bytes().as_slice()) {
            bail!(
                "{} pointer index {index} differs from the Remix spec",
                table.id
            );
        }
    }
    let pc = table.bank_operand_pc;
    if rom.get(pc - 1..pc + 3) != Some([0xA9, table.source_bank | 0x80, 0x85, 0x10].as_slice()) {
        bail!("{} source-bank load differs from the Remix spec", table.id);
    }
    // `LDA table,Y` must address this table in the same routine.
    let table_address = crate::rom::pc_to_lorom(table.pointer_pc).1.to_le_bytes();
    let load = [0xB9, table_address[0], table_address[1]];
    if rom.get(table.pointer_pc - 0x0F..table.pointer_pc - 0x0C) != Some(load.as_slice()) {
        bail!("{} table load differs from the Remix spec", table.id);
    }
    Ok(())
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    let end = start
        .checked_add(len)
        .context("ending Expected Write overflow")?;
    allowed
        .get_mut(start..end)
        .with_context(|| format!("ending Expected Write 0x{start:06X}..0x{end:06X} outside ROM"))?
        .fill(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires roms/Super Puyo Puyo Tsuu Remix (Japan).sfc"]
    fn remix_ending_tables_match_the_measured_spec() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let reports = audit(&rom).unwrap();
        assert_eq!(reports.len(), 4);
        let translated: usize = reports.iter().map(|table| table.translated_blocks).sum();
        let blank: usize = reports.iter().map(|table| table.blank_slots).sum();
        assert_eq!((translated, blank), (36, 14));
        assert!(
            rom[RELOCATION_PC..RELOCATION_END_PC]
                .iter()
                .all(|byte| *byte == 0xFF)
        );
    }

    #[test]
    fn easy_success_lines_shift_left_one_tile_column() {
        assert_eq!(line_shift_columns_left("easy", 3, 0), 1);
        assert_eq!(line_shift_columns_left("easy", 3, 1), 1);
        assert_eq!(line_shift_columns_left("easy", 2, 0), 0);
    }

    #[test]
    fn reveal_slots_fully_overwrite_the_scrolling_ring_buffer() {
        assert!(
            MASKED_REVEAL_KO
                .iter()
                .all(|chunk| chunk.chars().count() == 4)
        );
        assert!(MASKED_REVEAL_KO[9..].iter().all(|chunk| *chunk == "    "));
    }
}
