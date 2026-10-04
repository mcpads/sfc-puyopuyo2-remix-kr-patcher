use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::story_codec::StoryToken;

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
pub const TABLE_PC: usize = 0x005610;
const GROUP_COUNT: usize = 4;
const SLOTS_PER_GROUP: usize = 16;
const PLACEHOLDER_ADDRESS: u16 = 0xE134;
const PAGE_PREFIX: &[u8] = &[0xFF, 0x03, 0x84, 0x05, 0x04, 0x1E];
const EXPECTED_ACTIVE_SLOTS: usize = 35;
const EXPECTED_UNIQUE_TARGETS: usize = 33;
const EXPECTED_PLACEHOLDER_SLOTS: usize = 29;
const EXPECTED_PAGE_STARTS: usize = 74;
pub const RELOCATION_PC: usize = 0x006200;
const RELOCATION_END_PC: usize = 0x007000;

const GROUP_IDS: [&str; GROUP_COUNT] = ["option", "io_test", "game_mode", "custom"];

#[derive(Clone, Copy)]
struct KoreanLine {
    source_prefix: &'static [u16],
    text: &'static str,
}

#[derive(Clone, Copy)]
struct KoreanTarget {
    address: u16,
    source_len: usize,
    source_sha256: &'static str,
    table_indices: &'static [usize],
    lines: &'static [KoreanLine],
}

macro_rules! line {
    ($text:literal) => {
        KoreanLine {
            source_prefix: &[],
            text: $text,
        }
    };
    ([$($prefix:expr),+ $(,)?] => $text:literal) => {
        KoreanLine {
            source_prefix: &[$($prefix),+],
            text: $text,
        }
    };
}

macro_rules! target {
    ($address:expr, $len:expr, $sha:literal, [$($index:expr),+ $(,)?], [$($line:expr),+ $(,)?]) => {
        KoreanTarget {
            address: $address,
            source_len: $len,
            source_sha256: $sha,
            table_indices: &[$($index),+],
            lines: &[$($line),+],
        }
    };
}

const TARGETS: &[KoreanTarget] = &[
    target!(
        0xD6A2,
        25,
        "d992422ce9ace15f7ed9d5c3803bbb38e72195e8758eb0a99c64c4a9aff9ba6f",
        [0],
        [line!("타이틀로 돌아감.")]
    ),
    target!(
        0xD6BB,
        37,
        "64657fd471a8fd6b294b77444b06b6758aefc0f8ddcb06090931cd62942adc90",
        [1],
        [line!("혼자서 뿌요뿌요의"), line!("난이도.")]
    ),
    target!(
        0xD6E0,
        109,
        "51503fe6195bdd00cd40da8f87a2e65ec4d0f90c94e3d5ebc5ded23250074a30",
        [2],
        [
            line!("대전의 경기 수."),
            line!("앞 수는 둘 모드에서"),
            line!("이기는 수."),
            line!("다음은 모두 모드의"),
            line!("모든 경기 수.")
        ]
    ),
    target!(
        0xD74D,
        241,
        "49051ac9c66a2f44b78b008f889de9d414a4db7b6b9d93b9bfa0b31648d15f69",
        [3],
        [
            line!("무한 모드의 규칙."),
            line!([0x00C0, 0x00C1, 0x00C2, 0x00C3] => "연습은 도움도"),
            line!("방해도 안 나와요."),
            line!([0x00C4, 0x00C5, 0x00C6] => "보통은 도움은 나와요."),
            line!("방해는 안 나와요."),
            line!([0x00C8, 0x00C9, 0x00CA] => "액션은 도움도 나와요."),
            line!("방해도 나와요."),
            line!([0x00C7, 0x00CB] => "야생은 도움은 안 나와요."),
            line!("방해는 나와요."),
            line!("도움은 큰 뿌요와"),
            line!("카방클입니다.")
        ]
    ),
    target!(
        0xD83E,
        41,
        "a537412294187ee945d557de20f286be122fee70d55f8e65465e447ff0e1dc31",
        [4],
        [line!("음악을 스테레오나"), line!("모노로 정함.")]
    ),
    target!(
        0xD867,
        37,
        "5d444017626d20fb41c2d6fab1761e21730d720bd9ee0fa410951a7c4edab82d",
        [5],
        [line!("뿌요 회전 키를"), line!("정함.")]
    ),
    target!(
        0xD88C,
        46,
        "8ed55309018734b6efda0867f8122b0d8127d9423d8cff12a4f10a1907fd8b5d",
        [6],
        [line!("기기와 그림 테스트."), line!("그림 설정도 변경.")]
    ),
    target!(
        0xD8BA,
        71,
        "fb3ee051813160561f6471526faeb51793849c115ae970438a133310a4e2a133",
        [7],
        [
            line!("설정 변경."),
            line!("뭐가 달라지는지는"),
            line!("직접 시험해 보세요.")
        ]
    ),
    target!(
        0xD901,
        68,
        "8a4e0a8353600191c6af2cb77ca2fd4376fb25ae3750a392adde18d69d106325",
        [8],
        [
            line!("별난 설정."),
            line!("할 수 있는 일은"),
            line!("직접 알아보세요.")
        ]
    ),
    target!(
        0xD945,
        34,
        "0be3d348c815fa7b72477bf812ad95d2e5992173259cb2bc8c3172eba20cfe39",
        [16, 32, 48],
        [line!("일반 옵션으로"), line!("돌아감.")]
    ),
    target!(
        0xD967,
        71,
        "bfa7871c87453667f9126591a3bed359f7e29f941069a39dcc8978b7b659a8df",
        [17],
        [
            line!("키 테스트."),
            line!("여러 게 나오지만,"),
            line!("신경 쓰지 마세요.")
        ]
    ),
    target!(
        0xD9AE,
        28,
        "aafe4c71efa0e81090458e0ac7e56218de6c2320d34c3770c02162813511029d",
        [18],
        [line!("소리 테스트.")]
    ),
    target!(
        0xD9CA,
        25,
        "9018f5e9eb74c95218a3c25afc75fcee604489171980ebd79feda832bf1db4b1",
        [19],
        [line!("음악 테스트.")]
    ),
    target!(
        0xD9E3,
        25,
        "447fa9d0803b85f711570bec038326a2208470b7dc6ec95722799e0777801126",
        [20],
        [line!("음성 테스트.")]
    ),
    target!(
        0xD9FC,
        69,
        "b8536d9e2d5c41ed5052d51f6b848753441306697efe23df9959eb653675f5b7",
        [21],
        [
            line!("혼자서 뿌요뿌요의"),
            line!("얼굴 보이기 방식."),
            line!([0x00CC, 0x00CD] => "꺼두면 안 나와요.")
        ]
    ),
    target!(
        0xDA41,
        42,
        "b8fa3fb5dfbc53330492c5cdc4671f140e5bf6826f8016aec3ff47f45d73e5e5",
        [22],
        [line!("혼자서 뿌요뿌요의"), line!("얼굴 보이기 높이.")]
    ),
    target!(
        0xDA6B,
        39,
        "69142c9081149f2a4a6e7bbb88e73800c903aaabb6a535e74505f5e3c6ca5123",
        [23],
        [line!("네 명 대전 배경을"), line!("고정함.")]
    ),
    target!(
        0xDA92,
        45,
        "13887c51e6d8c312cafd9797b7571cf827e07d3e236610b8382a945d27a22fa8",
        [24],
        [
            line!([0x00CC, 0x00CD] => "꺼두면 네 명 대전의"),
            line!("배경 그림을 지움.")
        ]
    ),
    target!(
        0xDABF,
        76,
        "5129cda689f3c0e6c0bf6c7c2256c854f3b07e87e55150a7f8564eddf1b798bf",
        [25],
        [
            line!("네 명 대전 성적."),
            line!("가장자리에 있어"),
            line!("안 보일 수도 있음.")
        ]
    ),
    target!(
        0xDB0B,
        40,
        "937215d6f2cd3d821d891410680a3db046350a585d257416c24465797e536087",
        [33],
        [line!("무한 모드 외의"), line!("제한 시간.")]
    ),
    target!(
        0xDB33,
        89,
        "02f4c68f9488934f3340c835a3a396c3bda8077d3316c672bf5dbb1b87db62cb",
        [34],
        [
            line!("정한 연쇄 수보다"),
            line!("적으면 방해를 못 보내요."),
            line!([0x00CC, 0x00CD] => "끄면 제한 없음."),
            line!("둘, 모두 모드만.")
        ]
    ),
    target!(
        0xDB8C,
        45,
        "259816eddfb1b4ca08e99be8bd306dfe9b7ddf3599f6c1cd3310993861cab51d",
        [35],
        [
            line!([0x00CA] => "켜면 혼자 모드에"),
            line!("여러 방해가 나와요.")
        ]
    ),
    target!(
        0xDBB9,
        40,
        "06cdcda208d396f5f0c4dde2b3e5edc92b6ff127b24fe4e1b8c835ea36ab153a",
        [36],
        [line!("방해 뿌요의 상쇄를"), line!("사용할지 정함.")]
    ),
    target!(
        0xDBE1,
        87,
        "8099583c9f8426e8555f03d8a95a874b80b439ecf33556a190b1c0fabc6fe1a1",
        [37],
        [
            line!("세부 규칙은 뿌요1처럼."),
            line!("설정할 수 있음."),
            line!("방해 상쇄는 바로 위"),
            line!("바로 위에서 정함.")
        ]
    ),
    target!(
        0xDC38,
        110,
        "dce4a1075ffb43283af756f03b8d8b4c7aaf68f54a595276bc2f79bf14576e2f",
        [38],
        [
            line!("정한 연쇄 수보다"),
            line!("적으면 굳은 뿌요를"),
            line!([0x00CC, 0x00CD] => "못 지움. 끄면 제한 없음."),
            line!("둘 정도가 적당."),
            line!("아주 흥미로워요.")
        ]
    ),
    target!(
        0xDCA6,
        105,
        "763fdb500df5acde6a3b62ac07f9e341f3b779ee09ff152ca341ed132c79c62a",
        [49],
        [
            line!("미리 준비"),
            line!("뿌요 등을 놓음."),
            line!([0x00CC, 0x00CD] => "끄면 아무것도 안 놓음."),
            line!("둘, 모두와"),
            line!("무한 모드만.")
        ]
    ),
    target!(
        0xDD0F,
        70,
        "8e517c6a45173a0aa163170809a764af8d0400549dd3204ec2e9ca0ac9b51af9",
        [50],
        [
            line!("이 수만큼"),
            line!("원래 색을 뺌."),
            line!("둘, 모두 모드만.")
        ]
    ),
    target!(
        0xDD55,
        113,
        "46a242cfe1df2f55e31f492ee9ec02ef93f41388eccc46b7fb60e56a44139d17",
        [51],
        [
            line!("보기 모드."),
            line!([0x00CC, 0x00CD] => "끄면 없음."),
            line!("수는 혼자 모드의"),
            line!("경기 위치."),
            line!("나머지는 여러 가지로"),
            line!("직접 시험해 보세요.")
        ]
    ),
    target!(
        0xDDC6,
        224,
        "e2621739f81485f0a41d2299739d4a9fea426d99dddc3998cdb171a347f77101",
        [52],
        [
            line!("둘 모드에서 별난"),
            line!("뿌요 제거 방식을 사용."),
            line!("모두 모드에서는"),
            line!("둘째 상대의"),
            line!("제거 방식을 사용."),
            line!("나중에 둘째를"),
            line!("사람이 움직여도"),
            line!("문제없음."),
            line!("혼자 모드도"),
            line!("변함.")
        ]
    ),
    target!(
        0xDEA6,
        28,
        "2333c165a88d758fec58c63a512fbcc16766dc6d84bc84e1fac8d82286ede5bf",
        [53],
        [line!("공식 대회 모드.")]
    ),
    target!(
        0xDEC2,
        36,
        "092df5ba42d82364d0c480e569dee8e605af8178610773326f2af94f8b71da4d",
        [61],
        [line!("무한 모드의 배경을"), line!("정함.")]
    ),
    target!(
        0xDEE6,
        65,
        "ba7f4f57b2320bcd1fd32b9916078ce2ebe25f375c478dd0ed684f875a029e5f",
        [62],
        [
            line!("뿌요를 놓으면"),
            line!("그림이 움직여요."),
            line!("둘, 무한 모드만.")
        ]
    ),
    target!(
        0xDF27,
        40,
        "9628f47b5b098f30feb260048d17d03573cf65421b18b794940838afc5dd5af3",
        [63],
        [line!("무한 모드에서"), line!("말할지 정함.")]
    ),
];

#[derive(Debug, Serialize)]
pub struct OptionHelpSlotReport {
    pub id: String,
    pub pointer_pc: String,
    pub target_lorom: String,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct OptionHelpAuditReport {
    pub table_pc: String,
    pub total_slots: usize,
    pub active_slots: usize,
    pub unique_active_targets: usize,
    pub placeholder_slots: usize,
    pub page_starts_in_bank00: usize,
    pub slots: Vec<OptionHelpSlotReport>,
}

#[derive(Debug, Serialize)]
pub struct OptionHelpBuildReport {
    pub translated_slots: usize,
    pub translated_unique_targets: usize,
    pub relocation_pc: String,
    pub relocation_lorom: String,
    pub relocation_bytes_used: usize,
    pub relocation_headroom: usize,
    pub controls_preserved: bool,
}

pub fn audit(rom: &[u8]) -> Result<OptionHelpAuditReport> {
    verify_rom_identity(rom)?;
    let table_len = GROUP_COUNT * SLOTS_PER_GROUP * 2;
    let table = rom
        .get(TABLE_PC..TABLE_PC + table_len)
        .context("option-help pointer table is outside ROM")?;
    let mut slots = Vec::with_capacity(GROUP_COUNT * SLOTS_PER_GROUP);
    let mut active_targets = BTreeSet::new();
    let mut active_slots = 0;
    let mut placeholder_slots = 0;
    for (group_index, group) in GROUP_IDS.iter().enumerate() {
        for slot in 0..SLOTS_PER_GROUP {
            let table_index = group_index * SLOTS_PER_GROUP + slot;
            let target = u16::from_le_bytes([table[table_index * 2], table[table_index * 2 + 1]]);
            let placeholder = target == PLACEHOLDER_ADDRESS;
            if placeholder {
                placeholder_slots += 1;
            } else {
                active_slots += 1;
                active_targets.insert(target);
                let target_pc = crate::rom::lorom_to_pc(0x00, target)
                    .with_context(|| format!("invalid option-help target $00:${target:04X}"))?;
                let parsed = crate::story_codec::parse(rom, target_pc)?;
                if parsed.consumed_len == 0 {
                    bail!("empty option-help target $00:${target:04X}");
                }
            }
            slots.push(OptionHelpSlotReport {
                id: format!("{group}_{slot:02}"),
                pointer_pc: format!("0x{:06X}", TABLE_PC + table_index * 2),
                target_lorom: format!("$00:${target:04X}"),
                state: if placeholder { "placeholder" } else { "active" }.to_owned(),
            });
        }
    }
    let bank00 = rom.get(..0x8000).context("ROM has no complete Bank $00")?;
    let page_starts_in_bank00 = bank00
        .windows(PAGE_PREFIX.len())
        .filter(|window| *window == PAGE_PREFIX)
        .count();
    if active_slots != EXPECTED_ACTIVE_SLOTS
        || active_targets.len() != EXPECTED_UNIQUE_TARGETS
        || placeholder_slots != EXPECTED_PLACEHOLDER_SLOTS
        || page_starts_in_bank00 != EXPECTED_PAGE_STARTS
    {
        bail!(
            "Remix option-help population drifted: active {active_slots}, unique {}, placeholders {placeholder_slots}, pages {page_starts_in_bank00}",
            active_targets.len()
        );
    }
    verify_sources(rom)?;
    Ok(OptionHelpAuditReport {
        table_pc: format!("0x{TABLE_PC:06X}"),
        total_slots: slots.len(),
        active_slots,
        unique_active_targets: active_targets.len(),
        placeholder_slots,
        page_starts_in_bank00,
        slots,
    })
}

pub fn add_required_characters(visible: &mut BTreeSet<char>) {
    visible.extend(
        TARGETS
            .iter()
            .flat_map(|target| target.lines.iter().flat_map(|line| line.text.chars())),
    );
}

pub fn expected_pointer_pcs(rom: &[u8]) -> Result<Vec<usize>> {
    audit(rom)?;
    Ok(TARGETS
        .iter()
        .flat_map(|target| target.table_indices)
        .map(|index| TABLE_PC + index * 2)
        .collect())
}

pub fn encoded_len(rom: &[u8], codes: &BTreeMap<char, u16>) -> Result<usize> {
    prepare_blocks(rom, codes).map(|blocks| blocks.iter().map(Vec::len).sum())
}

pub fn apply(
    source_rom: &[u8],
    patched: &mut [u8],
    codes: &BTreeMap<char, u16>,
) -> Result<OptionHelpBuildReport> {
    audit(source_rom)?;
    let blocks = prepare_blocks(source_rom, codes)?;
    let relocation_bytes_used = blocks.iter().map(Vec::len).sum::<usize>();
    let relocation_end = RELOCATION_PC + relocation_bytes_used;
    if relocation_end > RELOCATION_END_PC {
        bail!("Remix option-help relocation exceeds physical Bank $00");
    }
    for (label, rom) in [("source", source_rom), ("current derivative", &*patched)] {
        if rom[RELOCATION_PC..relocation_end]
            .iter()
            .any(|byte| *byte != 0xFF)
        {
            bail!("option-help relocation is occupied in {label} ROM");
        }
    }

    let mut cursor = RELOCATION_PC;
    for (target, encoded) in TARGETS.iter().zip(&blocks) {
        let (bank, address) = crate::rom::pc_to_lorom(cursor);
        if bank != 0x00 {
            bail!("option-help relocation left physical Bank $00");
        }
        patched[cursor..cursor + encoded.len()].copy_from_slice(encoded);
        for table_index in target.table_indices {
            let pointer_pc = TABLE_PC + table_index * 2;
            patched[pointer_pc..pointer_pc + 2].copy_from_slice(&address.to_le_bytes());
        }
        let reparsed = crate::story_codec::parse(patched, cursor)?;
        if reparsed.consumed_len != encoded.len() {
            bail!("relocated option-help block failed final round-trip");
        }
        cursor += encoded.len();
    }
    if cursor != relocation_end {
        bail!("option-help relocation cursor drifted");
    }
    Ok(OptionHelpBuildReport {
        translated_slots: EXPECTED_ACTIVE_SLOTS,
        translated_unique_targets: TARGETS.len(),
        relocation_pc: format!("0x{RELOCATION_PC:06X}-0x{:06X}", relocation_end - 1),
        relocation_lorom: format!("$00:E200-$00:{:04X}", 0xE200 + relocation_bytes_used - 1),
        relocation_bytes_used,
        relocation_headroom: RELOCATION_END_PC - relocation_end,
        controls_preserved: true,
    })
}

fn verify_rom_identity(rom: &[u8]) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(rom));
    if actual != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {actual}");
    }
    Ok(())
}

fn verify_sources(rom: &[u8]) -> Result<()> {
    if TARGETS.len() != EXPECTED_UNIQUE_TARGETS {
        bail!("option-help translation target count differs from audited population");
    }
    for target in TARGETS {
        for table_index in target.table_indices {
            let pointer_pc = TABLE_PC + table_index * 2;
            let actual = u16::from_le_bytes([rom[pointer_pc], rom[pointer_pc + 1]]);
            if actual != target.address {
                bail!(
                    "option-help table index {table_index} differs: ${actual:04X} != ${:04X}",
                    target.address
                );
            }
        }
        let source_pc = crate::rom::lorom_to_pc(0x00, target.address)
            .context("invalid option-help source address")?;
        let source = rom
            .get(source_pc..source_pc + target.source_len)
            .context("option-help source is outside ROM")?;
        let sha = format!("{:x}", Sha256::digest(source));
        if sha != target.source_sha256 {
            bail!(
                "option-help source $00:${:04X} differs from Remix JP Spec: {sha}",
                target.address
            );
        }
        let parsed = crate::story_codec::parse(rom, source_pc)?;
        if parsed.consumed_len != target.source_len {
            bail!(
                "option-help source $00:${:04X} boundary drifted",
                target.address
            );
        }
    }
    Ok(())
}

fn prepare_blocks(rom: &[u8], codes: &BTreeMap<char, u16>) -> Result<Vec<Vec<u8>>> {
    verify_sources(rom)?;
    TARGETS
        .iter()
        .map(|target| {
            let source_pc = crate::rom::lorom_to_pc(0x00, target.address)
                .context("invalid option-help source address")?;
            let parsed = crate::story_codec::parse(rom, source_pc)?;
            encode_block(&parsed.tokens, target.lines, codes)
                .with_context(|| format!("encode option-help $00:${:04X}", target.address))
        })
        .collect()
}

fn encode_block(
    source_tokens: &[StoryToken],
    lines: &[KoreanLine],
    codes: &BTreeMap<char, u16>,
) -> Result<Vec<u8>> {
    let source_lines = source_line_codes(source_tokens);
    if source_lines.len() != lines.len() {
        bail!(
            "Korean line count {} differs from source {}",
            lines.len(),
            source_lines.len()
        );
    }
    for (index, (source, line)) in source_lines.iter().zip(lines).enumerate() {
        if !line.source_prefix.is_empty() && !source.starts_with(line.source_prefix) {
            bail!("line {index} special-glyph prefix differs from Remix JP Spec");
        }
    }

    let mut output = Vec::new();
    let mut line_index = 0;
    let mut suppress_source_glyphs = false;
    for token in source_tokens {
        match token {
            StoryToken::Control { code, args } => {
                output.extend_from_slice(&[0xFF, *code]);
                output.extend_from_slice(args);
                suppress_source_glyphs = *code == 0x02;
                if *code == 0x02 {
                    let line = lines.get(line_index).context("option-help line overflow")?;
                    validate_line_width(args, line.text)?;
                    encode_visible(line.text, codes, &mut output)?;
                    line_index += 1;
                }
            }
            StoryToken::Glyph { .. } if suppress_source_glyphs => {}
            StoryToken::Glyph { .. } => {
                bail!("option-help source glyph appears before the first FF02 line")
            }
        }
    }
    if line_index != lines.len() {
        bail!("option-help translation did not consume every line");
    }
    let parsed = crate::story_codec::parse(&output, 0)?;
    if !crate::story_codec::controls_match(source_tokens, &parsed.tokens) {
        bail!("option-help translation changed protected controls");
    }
    Ok(output)
}

fn source_line_codes(tokens: &[StoryToken]) -> Vec<Vec<u16>> {
    let mut lines = Vec::new();
    let mut active = None;
    for token in tokens {
        match token {
            StoryToken::Control { code: 0x02, .. } => {
                lines.push(Vec::new());
                active = Some(lines.len() - 1);
            }
            StoryToken::Control { .. } => {}
            StoryToken::Glyph { code } => {
                if let Some(index) = active {
                    lines[index].push(*code);
                }
            }
        }
    }
    lines
}

fn validate_line_width(position_args: &[u8], text: &str) -> Result<()> {
    if position_args.len() != 2 {
        bail!("option-help FF02 position is not a word");
    }
    let position = u16::from_le_bytes([position_args[0], position_args[1]]);
    let start_column = usize::from(position / 2) % 32;
    let maximum = (32 - start_column) / 2;
    let characters = text.chars().count();
    if characters > maximum {
        bail!("option-help line {text:?} uses {characters} cells, maximum is {maximum}");
    }
    Ok(())
}

fn encode_visible(text: &str, codes: &BTreeMap<char, u16>, output: &mut Vec<u8>) -> Result<()> {
    for character in text.chars() {
        let code = codes
            .get(&character)
            .with_context(|| format!("unmapped option-help character {character:?}"))?;
        match *code {
            0x0000..=0x00FD => output.push(*code as u8),
            0x0100..=0x01FF => output.extend_from_slice(&[0xFE, *code as u8]),
            _ => bail!("option-help code 0x{code:04X} is not encodable"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_population_covers_every_active_slot_once() {
        let indices = TARGETS
            .iter()
            .flat_map(|target| target.table_indices.iter().copied())
            .collect::<BTreeSet<_>>();
        assert_eq!(indices.len(), EXPECTED_ACTIVE_SLOTS);
        assert_eq!(TARGETS.len(), EXPECTED_UNIQUE_TARGETS);
        assert!(
            indices
                .iter()
                .all(|index| *index < GROUP_COUNT * SLOTS_PER_GROUP)
        );
    }

    #[test]
    fn every_korean_line_fits_the_common_fifteen_cell_budget() {
        for line in TARGETS.iter().flat_map(|target| target.lines) {
            assert!(line.text.chars().count() <= 15, "{:?}", line.text);
        }
    }
}
