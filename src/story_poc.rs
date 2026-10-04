use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::font_gen::StoryFontRasterizer;

pub const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
/// Entry states the story encoder can consume. Distribution eligibility is a
/// separate production-policy decision; the encoder only rejects states that
/// have no reviewable Korean text.
pub const BUILDABLE_STATUSES: [&str; 3] = [
    "needs_review",
    "needs_human_review",
    "distribution_eligible",
];
/// File-level eligibility markers. `reviewed_candidate` is only valid when a
/// production policy has separately checked every entry state.
pub const BUILD_ELIGIBILITIES: [&str; 2] = ["poc_only_needs_review", "reviewed_candidate"];
// Galmuri14 v2.40.4 at 15px: 1px strokes and 14px ink, close to the
// original 13px kana. See assets/fonts/README.md.
pub(crate) const STORY_TTF_SHA256: &str =
    "6fe6c3fe4369e3837ac348431e8670733d67aa4bd550982baa72cc93c81a1c68";
pub(crate) const STORY_TTF_PATH: &str = "assets/fonts/galmuri14.ttf";
pub(crate) const STORY_TTF_PX: f32 = 15.0;
const FONT_BANK: u8 = 0x08;
const BASE_POINTER_PC: usize = 0x4002E;
const EXT_POINTER_PC: usize = 0x40036;
const NEXT_ASSET_POINTER_PC: usize = 0x40040;
const BASE_SOURCE_PC: usize = 0x40136;
const BASE_SLOTS: usize = 208;
const RESERVED_BASE_SLOTS: usize = 2;
const TRANSLATED_BASE_SLOTS: usize = BASE_SLOTS - RESERVED_BASE_SLOTS;
const RESERVED_R_CODE: u16 = 0x00CE;
const RESERVED_L_CODE: u16 = 0x00CF;
const SPACE_CODE: u16 = 0x00FC;
const EXT_MAX_SLOTS: usize = 256;
// Direct codes 0xD0-0xFB address WRAM $7F:1A00-$7F:1FFF through the same
// code-derived glyph lookup; the base stream grows into that range only when
// the FE-prefixed extended space is full.
const DIRECT_OVERFLOW_FIRST: u16 = 0x00D0;
const DIRECT_OVERFLOW_SLOTS: usize = 44;
const BASE_MAX_SLOTS: usize = 256;
const GROUP_GLYPHS: usize = 8;
const STORY_BANK_PC: usize = 0x0B8000;
const STORY_BANK_END_PC: usize = 0x0C0000;
const SOURCE_RUNTIME_BANK: u8 = 0x97;
const PRELUDE_ENTRY_INDEX: usize = 4;
const RELOCATION_PC: usize = 0x160000;
const RELOCATION_END_PC: usize = 0x164000;
const RELOCATION_BANK: u8 = 0x2C;
const RELOCATION_RUNTIME_BANK: u8 = 0xAC;
const CHECKSUM_PC: usize = 0x7FDC;

pub struct StoryPocInputs<'a> {
    pub rom: &'a [u8],
    pub translation_path: &'a Path,
    pub port_map_path: &'a Path,
    pub terms_path: &'a Path,
    pub style_path: &'a Path,
    pub ttf_data: &'a [u8],
    pub ttf_size: f32,
    pub opponent_prompt_ttf_path: &'a str,
    pub opponent_prompt_ttf_data: &'a [u8],
    pub opponent_prompt_ttf_size: f32,
}

#[derive(Debug, Serialize)]
pub struct StoryPocReport {
    pub source_sha256: String,
    pub output_sha256: String,
    pub entries: usize,
    pub relocated_entries: usize,
    pub in_place_prelude_entries: usize,
    pub direct_pointer_references: usize,
    pub sequential_entries: usize,
    pub false_pointer_candidates_excluded: usize,
    pub layout_adjusted_entries: usize,
    pub layout_shifted_entries: usize,
    pub max_line_characters: usize,
    pub translated_bitmap_glyphs: usize,
    pub sample_select_labels: Vec<String>,
    pub sample_select_encoded_bytes: usize,
    pub sample_select_relocation_pc: String,
    pub option_help: crate::option_help::OptionHelpBuildReport,
    pub demo_body: crate::demo_explanation::DemoBodyReport,
    pub two_player_rules: crate::two_player_rules::BuildReport,
    pub rule_editor: crate::rule_editor::BuildReport,
    pub multi_ui_text: crate::multi_ui_text::BuildReport,
    pub ending_text: crate::ending_text::BuildReport,
    pub opponent_prompt: crate::opponent_prompt::OpponentPromptKrPocReport,
    pub reserved_base_glyphs: Vec<String>,
    pub reserved_glyphs_preserved: bool,
    pub base_slots: usize,
    pub direct_overflow_glyphs: Vec<String>,
    pub glyph_capacity: usize,
    pub base_font_compressed_len: usize,
    pub extended_font_compressed_len: usize,
    pub extended_slots: usize,
    pub font_region_headroom: usize,
    pub relocation_pc: String,
    pub relocation_lorom: String,
    pub relocation_bytes_used: usize,
    pub relocation_headroom: usize,
    pub checksum_hex: String,
    pub changed_bytes: usize,
    pub translation_eligibility: String,
}

struct EncodedEntry {
    entry_id: usize,
    source_pc: usize,
    source_address: u16,
    source_raw_len: usize,
    bytes: Vec<u8>,
    direct_references: Vec<usize>,
    sequential: bool,
    prelude: bool,
    layout: crate::story_codec::StoryLayout,
}

pub fn build(inputs: &StoryPocInputs<'_>) -> Result<(Vec<u8>, StoryPocReport, String)> {
    let source_sha256 = format!("{:x}", Sha256::digest(inputs.rom));
    if source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    let port_map: crate::story_probe::StoryPortMap = read_json(inputs.port_map_path)?;
    let translation = crate::translation_port::read_ported(inputs.translation_path)?;
    validate_asset_headers(inputs, &port_map, &translation)?;

    let mut visible = BTreeSet::new();
    for entry in &translation.entries {
        visible.extend(crate::story_codec::visible_characters(&entry.ko)?);
    }
    crate::sample_select_text::add_required_characters(&mut visible);
    crate::option_help::add_required_characters(&mut visible);
    crate::demo_explanation::add_required_characters(&mut visible);
    crate::two_player_rules::add_required_characters(&mut visible);
    crate::rule_editor::add_required_characters(&mut visible);
    crate::multi_ui_text::add_required_characters(&mut visible);
    crate::ending_text::add_required_characters(&mut visible);
    if !visible.remove(&' ') {
        bail!("story translation has no mapped space character");
    }
    visible.remove(&'R');
    visible.remove(&'L');
    let translated_characters = visible.into_iter().collect::<Vec<_>>();
    if translated_characters.len() <= TRANSLATED_BASE_SLOTS {
        bail!("story translation does not exercise the extended font path");
    }
    let glyph_capacity = TRANSLATED_BASE_SLOTS + EXT_MAX_SLOTS + DIRECT_OVERFLOW_SLOTS;
    if translated_characters.len() > glyph_capacity {
        bail!(
            "Korean text needs {} bitmap glyphs but the shared font holds {glyph_capacity}",
            translated_characters.len()
        );
    }
    let extended_used = (translated_characters.len() - TRANSLATED_BASE_SLOTS).min(EXT_MAX_SLOTS);
    let extended_slots = extended_used.div_ceil(GROUP_GLYPHS) * GROUP_GLYPHS;
    let overflow_start = TRANSLATED_BASE_SLOTS + extended_used;
    let overflow_used = translated_characters.len() - overflow_start;
    let base_slots = if overflow_used == 0 {
        BASE_SLOTS
    } else {
        (usize::from(DIRECT_OVERFLOW_FIRST) + overflow_used).div_ceil(GROUP_GLYPHS) * GROUP_GLYPHS
    };
    if base_slots > BASE_MAX_SLOTS {
        bail!("direct overflow glyphs exceed the $7F:0000-$7F:1FFF base font range");
    }

    let mut codes = BTreeMap::new();
    for (index, character) in translated_characters.iter().copied().enumerate() {
        let code = if index < TRANSLATED_BASE_SLOTS {
            index as u16
        } else if index < overflow_start {
            0x0100 + (index - TRANSLATED_BASE_SLOTS) as u16
        } else {
            DIRECT_OVERFLOW_FIRST + (index - overflow_start) as u16
        };
        codes.insert(character, code);
    }
    codes.insert('R', RESERVED_R_CODE);
    codes.insert('L', RESERVED_L_CODE);
    codes.insert(' ', SPACE_CODE);

    let mut encoded_entries = Vec::with_capacity(translation.entries.len());
    let mut previous_source_end = None;
    let mut false_pointer_candidates_excluded = 0;
    for ((mapping, translated), entry_id) in port_map
        .entries
        .iter()
        .zip(&translation.entries)
        .zip(0usize..)
    {
        if mapping.entry_id != entry_id
            || translated.entry_id != entry_id
            || translated.logical_id != mapping.logical_id
            || translated.target_stable_id != mapping.target_stable_id
            || translated.raw_sha256 != mapping.raw_sha256
            || !BUILDABLE_STATUSES.contains(&translated.status.as_str())
        {
            bail!("translation protection fields differ at logical entry {entry_id}");
        }
        let source_pc = parse_pc(&mapping.target_file_offset)?;
        let raw = inputs
            .rom
            .get(source_pc..source_pc + mapping.raw_len)
            .with_context(|| format!("story source {} is outside ROM", mapping.logical_id))?;
        if format!("{:x}", Sha256::digest(raw)) != mapping.raw_sha256 {
            bail!("story raw bytes differ for {}", mapping.logical_id);
        }
        let source = crate::story_codec::parse(raw, 0)?;
        if source.consumed_len != raw.len() {
            bail!("story raw boundary differs for {}", mapping.logical_id);
        }
        let (_, source_address) = crate::rom::pc_to_lorom(source_pc);
        let pattern = [
            source_address as u8,
            (source_address >> 8) as u8,
            SOURCE_RUNTIME_BANK,
        ];
        let direct_references =
            find_pattern_offsets(&inputs.rom[STORY_BANK_PC..STORY_BANK_END_PC], &pattern)
                .into_iter()
                .map(|offset| STORY_BANK_PC + offset)
                .collect::<Vec<_>>();
        let all_references = find_pattern_offsets(inputs.rom, &pattern);
        false_pointer_candidates_excluded += all_references.len() - direct_references.len();
        let sequential = direct_references.is_empty()
            && previous_source_end.is_some_and(|end| end == source_pc)
            && entry_id != PRELUDE_ENTRY_INDEX;
        let prelude = entry_id == PRELUDE_ENTRY_INDEX;

        let mut bytes = crate::story_codec::encode_work_text(&translated.ko, &codes)
            .with_context(|| format!("encode {}", mapping.logical_id))?;
        let layout = crate::story_codec::adjust_layout(&mut bytes, &source.tokens)
            .with_context(|| format!("adjust layout for {}", mapping.logical_id))?;
        let encoded = crate::story_codec::parse(&bytes, 0)?;
        if encoded.consumed_len != bytes.len()
            || !crate::story_codec::controls_match(&source.tokens, &encoded.tokens)
        {
            bail!("encoded controls differ for {}", mapping.logical_id);
        }
        encoded_entries.push(EncodedEntry {
            entry_id,
            source_pc,
            source_address,
            source_raw_len: raw.len(),
            bytes,
            direct_references,
            sequential,
            prelude,
            layout,
        });
        previous_source_end = Some(source_pc + raw.len());
    }
    validate_entry_relations(
        inputs.rom,
        &encoded_entries,
        false_pointer_candidates_excluded,
    )?;

    let base_source = crate::story_font::source_pc(inputs.rom, BASE_POINTER_PC, FONT_BANK)?;
    let ext_source = crate::story_font::source_pc(inputs.rom, EXT_POINTER_PC, FONT_BANK)?;
    let font_end = crate::story_font::source_pc(inputs.rom, NEXT_ASSET_POINTER_PC, FONT_BANK)?;
    let original_base = crate::story_font::decode_at(inputs.rom, base_source)?;
    let original_extended = crate::story_font::decode_at(inputs.rom, ext_source)?;
    if base_source != BASE_SOURCE_PC
        || base_source + original_base.compressed_len != ext_source
        || ext_source + original_extended.compressed_len != font_end
        || original_base.bytes.len() != BASE_SLOTS * crate::story_font::GLYPH_LEN
        || original_extended.bytes.len() != 224 * crate::story_font::GLYPH_LEN
    {
        bail!("Remix font asset table differs from the verified static boundary");
    }
    let reserved_r = crate::story_font::glyph_tiles(&original_base, usize::from(RESERVED_R_CODE))?;
    let reserved_l = crate::story_font::glyph_tiles(&original_base, usize::from(RESERVED_L_CODE))?;
    if reserved_r.iter().all(|byte| *byte == 0) || reserved_l.iter().all(|byte| *byte == 0) {
        bail!("Remix reserved R/L glyphs are unexpectedly blank");
    }

    let rasterizer = StoryFontRasterizer::new(inputs.ttf_data, inputs.ttf_size)?;
    let rendered = translated_characters
        .iter()
        .map(|character| {
            rasterizer
                .render(*character)
                .with_context(|| format!("render story glyph {character:?}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut base_glyphs = rendered[..TRANSLATED_BASE_SLOTS].to_vec();
    base_glyphs.extend([reserved_r, reserved_l]);
    base_glyphs.extend_from_slice(&rendered[overflow_start..]);
    let base_bytes = crate::story_font::pack_glyph_stream(&base_glyphs, base_slots)?;
    let extended_bytes = crate::story_font::pack_glyph_stream(
        &rendered[TRANSLATED_BASE_SLOTS..overflow_start],
        extended_slots,
    )?;
    let base_compressed = crate::snes_lz::compress(&base_bytes);
    let extended_compressed = crate::snes_lz::compress(&extended_bytes);
    verify_compression(&base_compressed, &base_bytes, "base Korean font")?;
    verify_compression(
        &extended_compressed,
        &extended_bytes,
        "extended Korean font",
    )?;
    let font_region_len = font_end - base_source;
    let font_used = base_compressed.len() + extended_compressed.len();
    if font_used > font_region_len {
        bail!("Korean font streams exceed the Remix font asset region");
    }
    let new_ext_source = base_source + base_compressed.len();
    let (new_ext_bank, new_ext_address) = crate::rom::pc_to_lorom(new_ext_source);
    if new_ext_bank != FONT_BANK {
        bail!("new extended font source leaves physical Bank $08");
    }

    let relocation_bytes: usize = encoded_entries
        .iter()
        .filter(|entry| !entry.prelude)
        .map(|entry| entry.bytes.len())
        .sum();
    if RELOCATION_PC + relocation_bytes > RELOCATION_END_PC {
        bail!("Korean story text exceeds physical Bank $2C");
    }
    if inputs.rom[RELOCATION_PC..RELOCATION_PC + relocation_bytes]
        .iter()
        .any(|byte| *byte != 0xFF)
    {
        bail!("planned Bank $2C story relocation is not empty");
    }

    let mut patched = inputs.rom.to_vec();
    let mut allowed = vec![false; patched.len()];
    mark_allowed(&mut allowed, base_source, font_region_len)?;
    mark_allowed(&mut allowed, EXT_POINTER_PC, 2)?;
    patched[base_source..font_end].fill(0xFF);
    patched[base_source..base_source + base_compressed.len()].copy_from_slice(&base_compressed);
    patched[new_ext_source..new_ext_source + extended_compressed.len()]
        .copy_from_slice(&extended_compressed);
    patched[EXT_POINTER_PC..EXT_POINTER_PC + 2].copy_from_slice(&new_ext_address.to_le_bytes());

    mark_allowed(&mut allowed, RELOCATION_PC, relocation_bytes)?;
    let mut actual_pc_by_entry = BTreeMap::new();
    let mut cursor = RELOCATION_PC;
    for entry in &encoded_entries {
        if entry.prelude {
            if entry.bytes.len() > entry.source_raw_len {
                bail!("prelude story entry does not fit in place");
            }
            mark_allowed(&mut allowed, entry.source_pc, entry.source_raw_len)?;
            patched[entry.source_pc..entry.source_pc + entry.source_raw_len].fill(0xFF);
            patched[entry.source_pc..entry.source_pc + entry.bytes.len()]
                .copy_from_slice(&entry.bytes);
            actual_pc_by_entry.insert(entry.entry_id, entry.source_pc);
        } else {
            patched[cursor..cursor + entry.bytes.len()].copy_from_slice(&entry.bytes);
            actual_pc_by_entry.insert(entry.entry_id, cursor);
            cursor += entry.bytes.len();
        }
    }
    if cursor != RELOCATION_PC + relocation_bytes {
        bail!("story relocation cursor drifted");
    }

    let mut direct_pointer_references = 0;
    for entry in &encoded_entries {
        if entry.prelude || entry.sequential {
            continue;
        }
        let new_pc = actual_pc_by_entry[&entry.entry_id];
        let (new_bank, new_address) = crate::rom::pc_to_lorom(new_pc);
        if new_bank != RELOCATION_BANK {
            bail!("relocated story entry left physical Bank $2C");
        }
        for reference in &entry.direct_references {
            let expected = [
                entry.source_address as u8,
                (entry.source_address >> 8) as u8,
                SOURCE_RUNTIME_BANK,
            ];
            if inputs.rom.get(*reference..*reference + 3) != Some(&expected) {
                bail!("story pointer at PC 0x{reference:06X} differs from expected bytes");
            }
            mark_allowed(&mut allowed, *reference, 3)?;
            patched[*reference..*reference + 3].copy_from_slice(&[
                new_address as u8,
                (new_address >> 8) as u8,
                RELOCATION_RUNTIME_BANK,
            ]);
            direct_pointer_references += 1;
        }
    }
    if direct_pointer_references != 264 {
        bail!("direct story pointer total drifted from 264");
    }

    for entry in &encoded_entries {
        let actual_pc = actual_pc_by_entry[&entry.entry_id];
        let parsed = crate::story_codec::parse(&patched, actual_pc)?;
        if parsed.consumed_len != entry.bytes.len()
            || patched.get(actual_pc..actual_pc + entry.bytes.len()) != Some(&entry.bytes)
        {
            bail!("final story round-trip failed for entry {}", entry.entry_id);
        }
    }

    let patched_base = crate::story_font::decode_at(&patched, base_source)?;
    let reserved_glyphs_preserved =
        crate::story_font::glyph_tiles(&patched_base, usize::from(RESERVED_R_CODE))? == reserved_r
            && crate::story_font::glyph_tiles(&patched_base, usize::from(RESERVED_L_CODE))?
                == reserved_l;
    if !reserved_glyphs_preserved {
        bail!("Remix R/L glyph preservation check failed");
    }

    crate::sample_select_text::verify_text_source(inputs.rom)?;
    let sample_select_encoded = crate::sample_select_text::encode_with_story_codes(&codes)?;
    crate::sample_select_text::verify_relocation(inputs.rom, sample_select_encoded.len())?;
    mark_allowed(&mut allowed, crate::sample_select_text::POINTER_PC, 2)?;
    mark_allowed(
        &mut allowed,
        crate::sample_select_text::RELOCATION_PC,
        sample_select_encoded.len(),
    )?;
    crate::sample_select_text::write_text_patch(&mut patched, &sample_select_encoded)?;

    let option_help_encoded_len = crate::option_help::encoded_len(inputs.rom, &codes)?;
    for pointer_pc in crate::option_help::expected_pointer_pcs(inputs.rom)? {
        mark_allowed(&mut allowed, pointer_pc, 2)?;
    }
    mark_allowed(
        &mut allowed,
        crate::option_help::RELOCATION_PC,
        option_help_encoded_len,
    )?;
    let option_help = crate::option_help::apply(inputs.rom, &mut patched, &codes)?;

    let demo_body =
        crate::demo_explanation::apply_body(inputs.rom, &mut patched, &codes, &mut allowed)?;

    let two_player_rules =
        crate::two_player_rules::apply(inputs.rom, &mut patched, &codes, &mut allowed)?;

    let rule_editor = crate::rule_editor::apply(inputs.rom, &mut patched, &codes, &mut allowed)?;
    let multi_ui_text = crate::multi_ui_text::apply(
        inputs.rom,
        &mut patched,
        &codes,
        &mut allowed,
        rule_editor.relocation_end_pc,
    )?;
    let ending_text = crate::ending_text::apply(inputs.rom, &mut patched, &codes, &mut allowed)?;

    let opponent_inputs = crate::opponent_prompt::OpponentPromptKrPocInputs {
        rom: &patched,
        source_path: "verified in-process Korean story derivative".to_owned(),
        ttf_path: inputs.opponent_prompt_ttf_path.to_owned(),
        ttf_data: inputs.opponent_prompt_ttf_data,
        font_px: inputs.opponent_prompt_ttf_size,
        runtime_vram: None,
        runtime_cram: None,
        output_path: "story-kr-poc".to_owned(),
    };
    mark_allowed(
        &mut allowed,
        crate::opponent_prompt::STREAM_PC,
        crate::opponent_prompt::ORIGINAL_COMPRESSED_LEN,
    )?;
    let (opponent_patched, opponent_prompt) =
        crate::opponent_prompt::build_opponent_prompt_kr_poc(&opponent_inputs)?;
    patched = opponent_patched;

    mark_allowed(&mut allowed, CHECKSUM_PC, 4)?;
    let checksum = crate::rom::fix_checksum(&mut patched)?;
    let changed_bytes = inputs
        .rom
        .iter()
        .zip(&patched)
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .try_fold(0usize, |count, index| {
            if !allowed[index] {
                bail!("unexplained changed byte at PC 0x{index:06X}");
            }
            Ok(count + 1)
        })?;

    let layout_adjusted_entries = encoded_entries
        .iter()
        .filter(|entry| entry.layout.changed)
        .count();
    let layout_shifted_entries = encoded_entries
        .iter()
        .filter(|entry| entry.layout.shifted_columns != 0)
        .count();
    let max_line_characters = encoded_entries
        .iter()
        .map(|entry| entry.layout.max_line_characters)
        .max()
        .unwrap_or(0);
    let output_sha256 = format!("{:x}", Sha256::digest(&patched));
    let report = StoryPocReport {
        source_sha256,
        output_sha256,
        entries: encoded_entries.len(),
        relocated_entries: encoded_entries.len() - 1,
        in_place_prelude_entries: 1,
        direct_pointer_references,
        sequential_entries: encoded_entries
            .iter()
            .filter(|entry| entry.sequential)
            .count(),
        false_pointer_candidates_excluded,
        layout_adjusted_entries,
        layout_shifted_entries,
        max_line_characters,
        translated_bitmap_glyphs: translated_characters.len(),
        sample_select_labels: crate::sample_select_text::KOREAN_LABELS
            .iter()
            .map(|label| (*label).to_owned())
            .collect(),
        sample_select_encoded_bytes: sample_select_encoded.len(),
        sample_select_relocation_pc: format!(
            "0x{:06X}-0x{:06X}",
            crate::sample_select_text::RELOCATION_PC,
            crate::sample_select_text::RELOCATION_PC + sample_select_encoded.len() - 1
        ),
        option_help,
        demo_body,
        two_player_rules,
        rule_editor,
        multi_ui_text,
        ending_text,
        opponent_prompt,
        reserved_base_glyphs: vec!["0x00CE=R".to_owned(), "0x00CF=L".to_owned()],
        reserved_glyphs_preserved,
        base_slots,
        direct_overflow_glyphs: translated_characters[overflow_start..]
            .iter()
            .enumerate()
            .map(|(offset, character)| {
                format!(
                    "0x{:04X}={character}",
                    usize::from(DIRECT_OVERFLOW_FIRST) + offset
                )
            })
            .collect(),
        glyph_capacity,
        base_font_compressed_len: base_compressed.len(),
        extended_font_compressed_len: extended_compressed.len(),
        extended_slots,
        font_region_headroom: font_region_len - font_used,
        relocation_pc: format!("0x{RELOCATION_PC:06X}-0x{:06X}", cursor - 1),
        relocation_lorom: format!(
            "${RELOCATION_BANK:02X}:$8000-${RELOCATION_BANK:02X}:${:04X}",
            0x8000 + relocation_bytes - 1
        ),
        relocation_bytes_used: relocation_bytes,
        relocation_headroom: RELOCATION_END_PC - cursor,
        checksum_hex: format!("0x{checksum:04X}"),
        changed_bytes,
        translation_eligibility: translation.build_eligibility,
    };
    Ok((patched, report, encoding_tsv(&codes)))
}

fn validate_asset_headers(
    inputs: &StoryPocInputs<'_>,
    port_map: &crate::story_probe::StoryPortMap,
    translation: &crate::translation_port::PortedStoryTranslation,
) -> Result<()> {
    if port_map.schema_version != 1
        || port_map.entries.len() != 273
        || port_map.target_rom_sha256 != TARGET_SHA256
        || translation.schema_version != 1
        || translation.table_id != port_map.table_id
        || translation.target_rom_sha256 != TARGET_SHA256
        || translation.source_rom_sha256 != port_map.base_rom_sha256
        || translation.entries.len() != port_map.entries.len()
        || !BUILD_ELIGIBILITIES.contains(&translation.build_eligibility.as_str())
    {
        bail!("story port map and translation headers are incompatible");
    }
    if file_sha256(inputs.terms_path)? != translation.source_terms_sha256
        || file_sha256(inputs.style_path)? != translation.source_style_sha256
    {
        bail!("story terms/style baselines differ from the ported translation");
    }
    if format!("{:x}", Sha256::digest(inputs.ttf_data)) != STORY_TTF_SHA256 {
        bail!("Galmuri14 TTF SHA-256 differs from the verified build input");
    }
    Ok(())
}

fn validate_entry_relations(
    rom: &[u8],
    entries: &[EncodedEntry],
    false_pointer_candidates_excluded: usize,
) -> Result<()> {
    let direct_entries = entries
        .iter()
        .filter(|entry| !entry.direct_references.is_empty())
        .count();
    let direct_references: usize = entries
        .iter()
        .map(|entry| entry.direct_references.len())
        .sum();
    let sequential_entries = entries.iter().filter(|entry| entry.sequential).count();
    let preludes = entries.iter().filter(|entry| entry.prelude).count();
    if direct_entries != 264
        || direct_references != 264
        || sequential_entries != 8
        || preludes != 1
        || false_pointer_candidates_excluded != 1
    {
        bail!("Remix story entry relation population drifted");
    }
    if entries
        .iter()
        .any(|entry| !entry.prelude && !entry.sequential && entry.direct_references.len() != 1)
    {
        bail!("a direct Remix story entry lacks exactly one Bank $17 pointer");
    }

    let prelude_entry = &entries[PRELUDE_ENTRY_INDEX];
    if !prelude_entry.prelude || !prelude_entry.direct_references.is_empty() {
        bail!("Remix story prelude relation is not at logical index 4");
    }
    let prelude_pc = prelude_entry
        .source_pc
        .checked_sub(6)
        .context("story prelude underflow")?;
    let (_, prelude_address) = crate::rom::pc_to_lorom(prelude_pc);
    let expected = [
        0x09,
        0x01,
        0x00,
        0xFF,
        prelude_address as u8,
        (prelude_address >> 8) as u8,
    ];
    if rom.get(prelude_pc..prelude_pc + 6) != Some(&expected) {
        bail!("Remix story prelude raw bytes differ");
    }
    let pointer = [
        prelude_address as u8,
        (prelude_address >> 8) as u8,
        SOURCE_RUNTIME_BANK,
    ];
    let references = find_pattern_offsets(&rom[STORY_BANK_PC..STORY_BANK_END_PC], &pointer);
    if references.len() != 1 {
        bail!("Remix story prelude pointer population drifted");
    }
    Ok(())
}

fn find_pattern_offsets(data: &[u8], pattern: &[u8]) -> Vec<usize> {
    data.windows(pattern.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == pattern).then_some(offset))
        .collect()
}

fn verify_compression(compressed: &[u8], expected: &[u8], label: &str) -> Result<()> {
    let decoded = crate::snes_lz::decompress(compressed, 0)?;
    if decoded.bytes != expected || decoded.compressed_len != compressed.len() {
        bail!("{label} failed compression round-trip");
    }
    Ok(())
}

fn mark_allowed(allowed: &mut [bool], start: usize, len: usize) -> Result<()> {
    let end = start.checked_add(len).context("allowed range overflow")?;
    allowed
        .get_mut(start..end)
        .with_context(|| format!("allowed range 0x{start:06X}..0x{end:06X} is outside ROM"))?
        .fill(true);
    Ok(())
}

fn parse_pc(value: &str) -> Result<usize> {
    usize::from_str_radix(
        value
            .strip_prefix("0x")
            .with_context(|| format!("PC offset is not 0x-prefixed: {value}"))?,
        16,
    )
    .with_context(|| format!("invalid PC offset: {value}"))
}

fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn encoding_tsv(codes: &BTreeMap<char, u16>) -> String {
    let mut output = String::from("code\tcharacter\tunicode\tkind\n");
    for (character, code) in codes {
        let kind = match *character {
            ' ' => "space",
            'R' | 'L' => "reserved_remix",
            _ => "bitmap",
        };
        output.push_str(&format!(
            "0x{code:04X}\t{character}\tU+{:04X}\t{kind}\n",
            *character as u32
        ));
    }
    output
}
