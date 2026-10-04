use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const POINTER_TABLE_PC: usize = 0x10_16C8;
const BANK_OPERAND_PC: usize = 0x10_169D;
const SOURCE_BANK: u8 = 0x20;
const RELOCATION_BANK: u8 = 0x2C;
const RELOCATION_PC: usize = 0x16_4000;
const RELOCATION_END_PC: usize = 0x16_5000;
const POINTER_SLOTS: usize = 16;
const SOURCE_POINTERS: [u16; POINTER_SLOTS] = [
    0x96E8, 0x96EA, 0x9737, 0x977F, 0x97BB, 0x97E8, 0x980C, 0x9843, 0x9878, 0x989B, 0x98C6, 0x98F5,
    0x9938, 0x9979, 0x99AA, 0x99AA,
];
const SOURCE_LENGTHS: [usize; POINTER_SLOTS] =
    [2, 77, 72, 60, 45, 36, 55, 53, 35, 43, 47, 67, 65, 49, 2, 2];
const SOURCE_POPULATION_SHA256: &str =
    "2f5728ce282a34650af7c2d8681bfec55afdcbd104cd3049a02d8670e488fe1c";

const TITLE_CHR_PC: usize = 0x11_80D8;
const TITLE_CHR_LEN: usize = 8_929;
const TITLE_DECODED_LEN: usize = 16_320;
const TITLE_RAW_SHA256: &str = "820dda23ca2c5d480f7613ba38a8aafbc10ef4c1e2de208bb04a28bce124f6ea";
const TITLE_DECODED_SHA256: &str =
    "7ce6a96f484b2d42ca846b4a8a5412c8ac1193d007ef3a91d017d86a19624e03";
const TITLE_POINTER_TABLE_PC: usize = 0x10_11E4;
const TITLE_MAP_PC: usize = 0x10_11F4;
const TITLE_MAP_LEN: usize = 1_024;
const TITLE_MAP_SHA256: &str = "a7b43b73f04425df2e6f21423c417c3275299f64d55138a757a0720296eaf1f6";
// Galmuri14 v2.40.4: the original titles are 1px-stroke kana about 15px tall.
const TITLE_TTF_SHA256: &str = "6fe6c3fe4369e3837ac348431e8670733d67aa4bd550982baa72cc93c81a1c68";
const TITLE_VARIANTS: usize = 8;
const TITLE_POINTERS: [u16; TITLE_VARIANTS] = [
    0x91F4, 0x9274, 0x92F4, 0x9374, 0x93F4, 0x9474, 0x94F4, 0x9574,
];
const TITLE_WIDTH_TILES: usize = 32;
const TITLE_HEIGHT_TILES: usize = 2;
const TILE_BYTES_4BPP: usize = 32;
const TITLE_PALETTE_WORD: u16 = 0x0800;
const TITLE_BLANK_TILE: u16 = 0x0001;
const TITLE_TEXT_INDEX: u8 = 2;
const TITLE_BACKGROUND_INDEX: u8 = 14;

pub const KOREAN_TITLES: [&str; TITLE_VARIANTS] = [
    "기본 조작의 비밀",
    "게임 오버의 비밀",
    "연쇄 공격의 비밀",
    "방해 뿌요의 비밀",
    "상쇄의 비밀",
    "싹쓸이의 비밀",
    "난입의 비밀",
    "퀵 턴의 비밀",
];

const PAGE_1_KO: &[&str] = &[
    "내려오는",
    "뿌요는 키로",
    "좌우로",
    "움직여요.",
    "아래 키를",
    "쓰면 빨리",
    "내려옵니다.",
];
const PAGE_2_KO: &[&str] = &[
    "회전 키로",
    "뿌요 회전",
    "우 키는",
    "우 회전",
    "좌 키는",
    "좌 회전",
];
const PAGE_3_KO: &[&str] = &[
    "뿌요는",
    "상하좌우로",
    "네 개 이상",
    "모이면 모두",
    "사라져요.",
];
const PAGE_4_KO: &[&str] = &["뿌요가 빨간", "선보다 위로", "올라가면", "게임 오버."];
const PAGE_5_KO: &[&str] = &["뿌요를 지워", "상대 쪽에", "방해가 가요"];
const PAGE_6_KO: &[&str] = &["연쇄로", "뿌요를 지워", "상대 쪽에", "방해를 더", "보내요."];
const PAGE_7_KO: &[&str] = &["방해 뿌요는", "주변 뿌요를", "지우면", "같이", "사라져요."];
const PAGE_8_KO: &[&str] = &["단단 뿌요는", "한 번에는", "못 지워요."];
const PAGE_9_KO: &[&str] = &["이렇게 하면", "단단 뿌요도", "한 번에 싹", "사라져요."];
const PAGE_10_KO: &[&str] = &["점수 뿌요는", "보통처럼", "지우면", "점수 올라요"];
const PAGE_11_KO: &[&str] = &[
    "상대 쪽의",
    "방해 뿌요는",
    "내가 보내는",
    "방해 뿌요로",
    "서로 상쇄할",
    "수 있어요.",
];
const PAGE_12_KO: &[&str] = &[
    "싹쓸이!가",
    "나오면",
    "뿌요를 지워",
    "다음 공격에",
    "방해를 더",
    "보내요.",
];
const PAGE_13_KO: &[&str] = &["이런 경우", "회전 키를", "두 번 쓰면", "상하 변경."];
const TRANSLATIONS: [Option<&[&str]>; POINTER_SLOTS] = [
    None,
    Some(PAGE_1_KO),
    Some(PAGE_2_KO),
    Some(PAGE_3_KO),
    Some(PAGE_4_KO),
    Some(PAGE_5_KO),
    Some(PAGE_6_KO),
    Some(PAGE_7_KO),
    Some(PAGE_8_KO),
    Some(PAGE_9_KO),
    Some(PAGE_10_KO),
    Some(PAGE_11_KO),
    Some(PAGE_12_KO),
    Some(PAGE_13_KO),
    None,
    None,
];

#[derive(Debug, Serialize)]
pub struct DemoBodyReport {
    pub verdict: String,
    pub translated_pages: usize,
    pub pointer_table_pc: String,
    pub bank_operand_pc: String,
    pub relocation_pc: String,
    pub relocation_lorom: String,
    pub relocation_bytes_used: usize,
    pub relocation_headroom: usize,
    pub max_line_characters: usize,
    pub controls_preserved: bool,
    pub source_population_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct DemoTitleReport {
    pub verdict: String,
    pub ttf_path: String,
    pub ttf_sha256: String,
    pub font_px: f32,
    pub titles: Vec<String>,
    pub unique_characters: usize,
    pub title_owned_tiles: usize,
    pub allocated_tiles: usize,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decoded_diff_confined_to_title_tiles: bool,
    pub diff_confined_to_expected_ranges: bool,
    pub output_sha256: String,
}

pub fn add_required_characters(characters: &mut BTreeSet<char>) {
    for lines in TRANSLATIONS.iter().flatten() {
        for line in *lines {
            characters.extend(line.chars());
        }
    }
}

pub fn apply_body(
    source: &[u8],
    patched: &mut [u8],
    codes: &BTreeMap<char, u16>,
    allowed: &mut [bool],
) -> Result<DemoBodyReport> {
    if source.len() != patched.len() || source.len() != allowed.len() {
        bail!("auto-demo body buffers have different ROM lengths");
    }
    if source.get(BANK_OPERAND_PC).copied() != Some(SOURCE_BANK | 0x80) {
        bail!("Remix auto-demo source-bank operand differs from $A0");
    }
    if read_pointer_table(source, POINTER_TABLE_PC)? != SOURCE_POINTERS {
        bail!("Remix auto-demo pointer table differs from the measured 16 slots");
    }

    let mut population = Vec::new();
    let mut source_pages = Vec::with_capacity(POINTER_SLOTS);
    for (index, (&pointer, &expected_len)) in
        SOURCE_POINTERS.iter().zip(&SOURCE_LENGTHS).enumerate()
    {
        let pc = crate::rom::lorom_to_pc(SOURCE_BANK, pointer)
            .context("invalid Remix auto-demo source pointer")?;
        let parsed = crate::story_codec::parse(source, pc)
            .with_context(|| format!("parse Remix auto-demo page {index}"))?;
        if parsed.consumed_len != expected_len {
            bail!(
                "Remix auto-demo page {index} length is {}, expected {expected_len}",
                parsed.consumed_len
            );
        }
        population.extend_from_slice(&source[pc..pc + expected_len]);
        source_pages.push(parsed);
    }
    if sha256(&population) != SOURCE_POPULATION_SHA256 {
        bail!("Remix auto-demo source population hash differs from the verified pages");
    }

    let mut encoded_pages = BTreeMap::new();
    let mut max_line_characters = 0;
    for (index, lines) in TRANSLATIONS.iter().enumerate() {
        let Some(lines) = lines else { continue };
        max_line_characters = max_line_characters.max(
            lines
                .iter()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0),
        );
        encoded_pages.insert(
            index,
            encode_translated_page(&source_pages[index].tokens, lines, codes)?,
        );
    }
    let relocation_bytes_used = 2 + encoded_pages.values().map(Vec::len).sum::<usize>();
    if RELOCATION_PC + relocation_bytes_used > RELOCATION_END_PC {
        bail!("Korean auto-demo body exceeds its reserved Bank $2C range");
    }
    if source[RELOCATION_PC..RELOCATION_PC + relocation_bytes_used]
        .iter()
        .any(|&byte| byte != 0xFF)
    {
        bail!("Korean auto-demo relocation range is not empty in the original ROM");
    }

    mark_allowed(allowed, BANK_OPERAND_PC, 1)?;
    mark_allowed(allowed, POINTER_TABLE_PC, POINTER_SLOTS * 2)?;
    mark_allowed(allowed, RELOCATION_PC, relocation_bytes_used)?;
    patched[BANK_OPERAND_PC] = RELOCATION_BANK | 0x80;
    patched[RELOCATION_PC..RELOCATION_PC + 2].copy_from_slice(&[0xFF, 0x00]);
    let blank_address = crate::rom::pc_to_lorom(RELOCATION_PC).1;
    let mut pointers = [blank_address; POINTER_SLOTS];
    let mut cursor = RELOCATION_PC + 2;
    for (index, encoded) in encoded_pages {
        pointers[index] = crate::rom::pc_to_lorom(cursor).1;
        patched[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
        cursor += encoded.len();
    }
    for (index, pointer) in pointers.iter().enumerate() {
        let start = POINTER_TABLE_PC + index * 2;
        patched[start..start + 2].copy_from_slice(&pointer.to_le_bytes());
    }

    Ok(DemoBodyReport {
        verdict: "all 13 shared-font game-explanation pages translated".to_owned(),
        translated_pages: 13,
        pointer_table_pc: format!("0x{POINTER_TABLE_PC:06X}"),
        bank_operand_pc: format!("0x{BANK_OPERAND_PC:06X}"),
        relocation_pc: format!("0x{RELOCATION_PC:06X}-0x{:06X}", cursor - 1),
        relocation_lorom: format!("$2C:$C000-$2C:${:04X}", 0xC000 + cursor - RELOCATION_PC - 1),
        relocation_bytes_used,
        relocation_headroom: RELOCATION_END_PC - cursor,
        max_line_characters,
        controls_preserved: true,
        source_population_sha256: SOURCE_POPULATION_SHA256.to_owned(),
    })
}

pub fn build_title_after_verified_patch(
    source: &[u8],
    ttf_path: String,
    ttf_data: &[u8],
    font_px: f32,
) -> Result<(Vec<u8>, DemoTitleReport)> {
    if read_pointer_table(source, TITLE_POINTER_TABLE_PC)? != TITLE_POINTERS {
        bail!("Remix game-explanation title pointer table differs from eight variants");
    }
    let raw = source
        .get(TITLE_CHR_PC..TITLE_CHR_PC + TITLE_CHR_LEN)
        .context("game-explanation title CHR stream is outside ROM")?;
    if sha256(raw) != TITLE_RAW_SHA256 {
        bail!("game-explanation title CHR source hash differs from the verified stream");
    }
    let map_source = source
        .get(TITLE_MAP_PC..TITLE_MAP_PC + TITLE_MAP_LEN)
        .context("game-explanation title tilemaps are outside ROM")?;
    if sha256(map_source) != TITLE_MAP_SHA256 {
        bail!("game-explanation title tilemap hash differs from the verified eight maps");
    }
    let ttf_sha256 = sha256(ttf_data);
    if ttf_sha256 != TITLE_TTF_SHA256 {
        bail!("MapleStory Light TTF differs from the verified game-explanation input");
    }
    let original = crate::snes_lz::decompress(source, TITLE_CHR_PC)?;
    if original.compressed_len != TITLE_CHR_LEN
        || original.bytes.len() != TITLE_DECODED_LEN
        || sha256(&original.bytes) != TITLE_DECODED_SHA256
    {
        bail!("game-explanation title CHR decoded contract differs");
    }
    let owned_tiles = title_owned_tiles(source)?;
    if owned_tiles.len() != 126 {
        bail!("game-explanation title tile pool differs from 126 owned tiles");
    }

    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|error| anyhow::anyhow!("failed to parse game-explanation TTF: {error}"))?;
    let unique_characters = KOREAN_TITLES
        .iter()
        .flat_map(|title| title.chars())
        .filter(|character| *character != ' ')
        .collect::<BTreeSet<_>>();
    let mut available_tiles = owned_tiles.iter().copied();
    let mut glyph_tiles = BTreeMap::<char, [u16; 4]>::new();
    let mut replacements = BTreeMap::<u16, [u8; TILE_BYTES_4BPP]>::new();
    let mut allocated_patterns = BTreeMap::<[u8; TILE_BYTES_4BPP], u16>::new();
    for character in &unique_characters {
        let pixels = render_title_glyph(&font, *character, font_px)?;
        let mut ids = [TITLE_BLANK_TILE; 4];
        for tile_y in 0..2 {
            for tile_x in 0..2 {
                if title_quadrant_is_empty(&pixels, tile_x * 8, tile_y * 8) {
                    continue;
                }
                let encoded = encode_title_tile(&pixels, tile_x * 8, tile_y * 8);
                let (canonical, hflip, vflip) = canonical_title_tile(&encoded);
                let tile_id = if let Some(tile_id) = allocated_patterns.get(&canonical) {
                    *tile_id
                } else {
                    let tile_id = available_tiles
                        .next()
                        .context("Korean game-explanation titles exceed the 126-tile pool")?;
                    allocated_patterns.insert(canonical, tile_id);
                    replacements.insert(tile_id, canonical);
                    tile_id
                };
                ids[tile_y * 2 + tile_x] =
                    tile_id | if hflip { 0x4000 } else { 0 } | if vflip { 0x8000 } else { 0 };
            }
        }
        glyph_tiles.insert(*character, ids);
    }

    let mut decoded = original.bytes.clone();
    for tile_id in &owned_tiles {
        let start = usize::from(*tile_id) * TILE_BYTES_4BPP;
        decoded[start..start + TILE_BYTES_4BPP].fill(0);
    }
    for (tile_id, tile) in &replacements {
        let start = usize::from(*tile_id) * TILE_BYTES_4BPP;
        decoded[start..start + TILE_BYTES_4BPP].copy_from_slice(tile);
    }
    let decoded_diff_confined_to_title_tiles = original
        .bytes
        .as_chunks::<TILE_BYTES_4BPP>()
        .0
        .iter()
        .zip(decoded.as_chunks::<TILE_BYTES_4BPP>().0)
        .enumerate()
        .all(|(tile, (before, after))| before == after || owned_tiles.contains(&(tile as u16)));
    if !decoded_diff_confined_to_title_tiles {
        bail!("game-explanation title CHR changed outside the verified tile pool");
    }

    let tilemaps = KOREAN_TITLES
        .iter()
        .map(|title| compose_korean_title_map(title, &glyph_tiles))
        .collect::<Result<Vec<_>>>()?;
    let compressed = crate::snes_lz::compress(&decoded);
    verify_compression(&compressed, &decoded, "game-explanation title CHR")?;
    if compressed.len() > TITLE_CHR_LEN {
        bail!(
            "Korean game-explanation title CHR needs {} bytes, exceeding the {TITLE_CHR_LEN}-byte slot",
            compressed.len()
        );
    }

    let mut patched = source.to_vec();
    patched[TITLE_CHR_PC..TITLE_CHR_PC + TITLE_CHR_LEN].fill(0xFF);
    patched[TITLE_CHR_PC..TITLE_CHR_PC + compressed.len()].copy_from_slice(&compressed);
    for (index, tilemap) in tilemaps.iter().enumerate() {
        let pc = crate::rom::lorom_to_pc(SOURCE_BANK, TITLE_POINTERS[index])
            .context("invalid game-explanation title map pointer")?;
        patched[pc..pc + tilemap.len()].copy_from_slice(tilemap);
    }
    let diff_confined_to_expected_ranges =
        source
            .iter()
            .zip(&patched)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || title_chr_range().contains(&offset)
                    || title_map_range().contains(&offset)
            });
    if !diff_confined_to_expected_ranges {
        bail!("game-explanation title diff escaped its CHR and tilemap ranges");
    }

    let report = DemoTitleReport {
        verdict: "all eight game-explanation titles rebuilt with Galmuri14".to_owned(),
        ttf_path,
        ttf_sha256,
        font_px,
        titles: KOREAN_TITLES
            .iter()
            .map(|title| (*title).to_owned())
            .collect(),
        unique_characters: unique_characters.len(),
        title_owned_tiles: owned_tiles.len(),
        allocated_tiles: replacements.len(),
        original_compressed_len: original.compressed_len,
        patched_compressed_len: compressed.len(),
        decoded_diff_confined_to_title_tiles,
        diff_confined_to_expected_ranges,
        output_sha256: sha256(&patched),
    };
    Ok((patched, report))
}

pub fn title_chr_range() -> Range<usize> {
    TITLE_CHR_PC..TITLE_CHR_PC + TITLE_CHR_LEN
}

pub fn title_map_range() -> Range<usize> {
    TITLE_MAP_PC..TITLE_MAP_PC + TITLE_MAP_LEN
}

fn encode_translated_page(
    source: &[crate::story_codec::StoryToken],
    lines: &[&str],
    codes: &BTreeMap<char, u16>,
) -> Result<Vec<u8>> {
    let source_lines = source
        .iter()
        .filter(|token| {
            matches!(
                token,
                crate::story_codec::StoryToken::Control { code: 0x02, .. }
            )
        })
        .count();
    if source_lines != lines.len() {
        bail!(
            "game-explanation Korean line count {} differs from source count {source_lines}",
            lines.len()
        );
    }
    let mut output = Vec::new();
    let mut line_index = 0;
    let mut suppress_source_glyphs = false;
    for token in source {
        match token {
            crate::story_codec::StoryToken::Control { code, args } => {
                output.extend_from_slice(&[0xFF, *code]);
                output.extend_from_slice(args);
                suppress_source_glyphs = *code == 0x02;
                if *code == 0x02 {
                    let line = lines[line_index];
                    if line.chars().count() > 6 {
                        bail!("game-explanation line {line:?} exceeds six glyphs");
                    }
                    encode_visible(line, codes, &mut output)?;
                    line_index += 1;
                }
            }
            crate::story_codec::StoryToken::Glyph { .. } if suppress_source_glyphs => {}
            crate::story_codec::StoryToken::Glyph { .. } => {
                bail!("game-explanation source has a glyph before its first FF02 control")
            }
        }
    }
    let parsed = crate::story_codec::parse(&output, 0)?;
    if parsed.consumed_len != output.len() || !controls_match_exact(source, &parsed.tokens) {
        bail!("Korean game-explanation page failed control-preserving round-trip");
    }
    Ok(output)
}

fn encode_visible(text: &str, codes: &BTreeMap<char, u16>, output: &mut Vec<u8>) -> Result<()> {
    for character in text.chars() {
        let code = codes
            .get(&character)
            .with_context(|| format!("unmapped game-explanation character {character:?}"))?;
        match *code {
            0x0000..=0x00FD => output.push(*code as u8),
            0x0100..=0x01FF => output.extend_from_slice(&[0xFE, *code as u8]),
            _ => bail!("invalid game-explanation code 0x{code:04X}"),
        }
    }
    Ok(())
}

fn controls_match_exact(
    source: &[crate::story_codec::StoryToken],
    encoded: &[crate::story_codec::StoryToken],
) -> bool {
    let source = source.iter().filter_map(|token| match token {
        crate::story_codec::StoryToken::Control { code, args } => Some((code, args)),
        _ => None,
    });
    let encoded = encoded.iter().filter_map(|token| match token {
        crate::story_codec::StoryToken::Control { code, args } => Some((code, args)),
        _ => None,
    });
    source.eq(encoded)
}

fn title_owned_tiles(rom: &[u8]) -> Result<BTreeSet<u16>> {
    let mut tiles = BTreeSet::new();
    let map_len = TITLE_WIDTH_TILES * TITLE_HEIGHT_TILES * 2;
    for pointer in TITLE_POINTERS {
        let pc = crate::rom::lorom_to_pc(SOURCE_BANK, pointer)
            .context("invalid game-explanation title pointer")?;
        let tilemap = rom
            .get(pc..pc + map_len)
            .context("game-explanation title tilemap is outside ROM")?;
        for pair in tilemap.as_chunks::<2>().0 {
            let word = u16::from_le_bytes([pair[0], pair[1]]);
            if (word >> 10) & 7 != 2 {
                bail!("game-explanation title tilemap uses an unexpected palette");
            }
            let tile = word & 0x03FF;
            if tile != TITLE_BLANK_TILE {
                tiles.insert(tile);
            }
        }
    }
    Ok(tiles)
}

fn render_title_glyph(
    font: &fontdue::Font,
    character: char,
    font_px: f32,
) -> Result<[u8; 16 * 16]> {
    if !font_px.is_finite() || font_px <= 0.0 {
        bail!("game-explanation title font size must be positive");
    }
    let (metrics, raster) = font.rasterize(character, font_px);
    if metrics.width == 0 || metrics.height == 0 || raster.iter().all(|&value| value == 0) {
        bail!("game-explanation TTF produced an empty glyph for {character:?}");
    }
    let ascent = font
        .horizontal_line_metrics(font_px)
        .context("game-explanation TTF has no horizontal line metrics")?
        .ascent as i32;
    let x_offset = (16 - metrics.width as i32) / 2 - metrics.xmin;
    let baseline = (16 + ascent) / 2;
    let y_offset = baseline - metrics.ymin - metrics.height as i32;
    let mut pixels = [0u8; 16 * 16];
    for row in 0..metrics.height {
        for column in 0..metrics.width {
            let x = x_offset + column as i32;
            let y = y_offset + row as i32;
            if !(0..16).contains(&x) || !(0..16).contains(&y) {
                continue;
            }
            if raster[row * metrics.width + column] >= 32 {
                pixels[y as usize * 16 + x as usize] = TITLE_TEXT_INDEX;
            }
        }
    }
    if pixels.iter().all(|&value| value == 0) {
        bail!("game-explanation title glyph {character:?} vanished after quantization");
    }
    Ok(embolden_horizontally(&pixels))
}

/// The original title kana use mostly 2px vertical strokes; widening each
/// 1px Galmuri14 stroke to the right restores that weight.
fn embolden_horizontally(pixels: &[u8; 16 * 16]) -> [u8; 16 * 16] {
    let mut bold = *pixels;
    for y in 0..16 {
        for x in 1..16 {
            if pixels[y * 16 + x - 1] != 0 {
                bold[y * 16 + x] = TITLE_TEXT_INDEX;
            }
        }
    }
    bold
}

fn encode_title_tile(
    pixels: &[u8; 16 * 16],
    x_offset: usize,
    y_offset: usize,
) -> [u8; TILE_BYTES_4BPP] {
    let mut tile = [0u8; TILE_BYTES_4BPP];
    for y in 0..8 {
        for x in 0..8 {
            let glyph = pixels[(y_offset + y) * 16 + x_offset + x];
            let value = if glyph == 0 {
                TITLE_BACKGROUND_INDEX
            } else {
                glyph
            };
            set_4bpp_pixel(&mut tile, x, y, value);
        }
    }
    tile
}

fn title_quadrant_is_empty(pixels: &[u8; 16 * 16], x_offset: usize, y_offset: usize) -> bool {
    (0..8).all(|y| (0..8).all(|x| pixels[(y_offset + y) * 16 + x_offset + x] == 0))
}

fn canonical_title_tile(tile: &[u8; TILE_BYTES_4BPP]) -> ([u8; TILE_BYTES_4BPP], bool, bool) {
    let mut best = (*tile, false, false);
    for (hflip, vflip) in [(true, false), (false, true), (true, true)] {
        let candidate = flip_title_tile(tile, hflip, vflip);
        if candidate < best.0 {
            best = (candidate, hflip, vflip);
        }
    }
    best
}

fn flip_title_tile(
    tile: &[u8; TILE_BYTES_4BPP],
    hflip: bool,
    vflip: bool,
) -> [u8; TILE_BYTES_4BPP] {
    let mut flipped = [0u8; TILE_BYTES_4BPP];
    for y in 0..8 {
        for x in 0..8 {
            let source_x = if hflip { 7 - x } else { x };
            let source_y = if vflip { 7 - y } else { y };
            set_4bpp_pixel(
                &mut flipped,
                x,
                y,
                decode_4bpp_pixel(tile, source_x, source_y),
            );
        }
    }
    flipped
}

fn decode_4bpp_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let mask = 0x80 >> x;
    let mut value = 0;
    for plane in 0..4 {
        let offset = if plane < 2 {
            y * 2 + plane
        } else {
            16 + y * 2 + plane - 2
        };
        if tile[offset] & mask != 0 {
            value |= 1 << plane;
        }
    }
    value
}

fn set_4bpp_pixel(tile: &mut [u8; TILE_BYTES_4BPP], x: usize, y: usize, value: u8) {
    let mask = 0x80 >> x;
    for plane in 0..4 {
        let offset = if plane < 2 {
            y * 2 + plane
        } else {
            16 + y * 2 + plane - 2
        };
        if value & (1 << plane) != 0 {
            tile[offset] |= mask;
        } else {
            tile[offset] &= !mask;
        }
    }
}

fn compose_korean_title_map(
    title: &str,
    glyph_tiles: &BTreeMap<char, [u16; 4]>,
) -> Result<Vec<u8>> {
    let width_tiles = title
        .chars()
        .map(|character| if character == ' ' { 1 } else { 2 })
        .sum::<usize>();
    if width_tiles > TITLE_WIDTH_TILES {
        bail!("Korean game-explanation title {title:?} exceeds 256 pixels");
    }
    let mut words = [TITLE_PALETTE_WORD | TITLE_BLANK_TILE; 64];
    let mut column = (TITLE_WIDTH_TILES - width_tiles) / 2;
    for character in title.chars() {
        if character == ' ' {
            column += 1;
            continue;
        }
        let tiles = glyph_tiles
            .get(&character)
            .with_context(|| format!("no game-explanation title glyph for {character:?}"))?;
        words[column] = TITLE_PALETTE_WORD | tiles[0];
        words[column + 1] = TITLE_PALETTE_WORD | tiles[1];
        words[TITLE_WIDTH_TILES + column] = TITLE_PALETTE_WORD | tiles[2];
        words[TITLE_WIDTH_TILES + column + 1] = TITLE_PALETTE_WORD | tiles[3];
        column += 2;
    }
    let mut bytes = Vec::with_capacity(words.len() * 2);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}

fn read_pointer_table<const N: usize>(rom: &[u8], pc: usize) -> Result<[u16; N]> {
    let bytes = rom
        .get(pc..pc + N * 2)
        .with_context(|| format!("pointer table at PC 0x{pc:06X} is outside ROM"))?;
    let mut pointers = [0u16; N];
    for (pointer, pair) in pointers.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *pointer = u16::from_le_bytes([pair[0], pair[1]]);
    }
    Ok(pointers)
}

fn verify_compression(compressed: &[u8], expected: &[u8], label: &str) -> Result<()> {
    let decoded = crate::snes_lz::decompress(compressed, 0)?;
    if decoded.bytes != expected || decoded.compressed_len != compressed.len() {
        bail!("{label} failed compression round-trip");
    }
    Ok(())
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    let end = start.checked_add(len).context("Expected Write overflow")?;
    allowed
        .get_mut(start..end)
        .with_context(|| format!("Expected Write range 0x{start:06X}..0x{end:06X} is outside ROM"))?
        .fill(true);
    Ok(())
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_translation_matches_thirteen_pages_and_six_glyph_width() {
        let pages = TRANSLATIONS.iter().flatten().count();
        assert_eq!(pages, 13);
        assert!(
            TRANSLATIONS
                .iter()
                .flatten()
                .all(|lines| { lines.iter().all(|line| line.chars().count() <= 6) })
        );
    }

    #[test]
    fn all_korean_titles_fit_the_measured_tilemap_width() {
        assert!(KOREAN_TITLES.iter().all(|title| {
            title
                .chars()
                .map(|character| if character == ' ' { 1 } else { 2 })
                .sum::<usize>()
                <= TITLE_WIDTH_TILES
        }));
    }
}
