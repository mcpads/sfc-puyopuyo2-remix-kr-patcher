use std::{collections::BTreeMap, fs, io::Cursor, ops::Range, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::generated_lettering::Mask;
use crate::snes_asm::{Inst, assemble, assemble_at};

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const STREAM_PC: usize = 0x13_D321;
const STREAM_BANK_END: usize = 0x14_0000;
const EXPECTED_COMPRESSED_LEN: usize = 7_809;
const EXPECTED_DECOMPRESSED_LEN: usize = 10_240;
const EXPECTED_RAW_SHA256: &str =
    "69404e6847ec02542b4361c7bd547e044da03279b1a172c9ff5af9cbcf3c64f2";
const EXPECTED_DECODED_SHA256: &str =
    "d76129a63a9782f419862f17ecfd171e8e7d155591f0814b1892672be3f64710";
const STREAM_VRAM_BYTE: usize = 0x3800;
const BG1_CHR_BASE_BYTE: usize = 0x2000;
const BG2_CHR_BASE_BYTE: usize = 0x0000;
const TILE_LEN: usize = 32;
const CHECKSUM_PC: usize = 0x7FDC;
pub(crate) const MENU_MAP_HOOK_RANGE: Range<usize> = 0x00_414C..0x00_4151;
pub(crate) const MENU_MAP_CODE_RANGE: Range<usize> = 0x16_5000..0x16_5200;
const MENU_MAP_HOOK_EXPECTED: [u8; 5] = [0xE2, 0x20, 0xA0, 0xF5, 0xC1];
const MENU_MAP_CODE_ORIGIN: u16 = 0xD000;
const MENU_MAP_CODE_LONG: u32 = 0xAC_D000;
const BG2_BACKGROUND_TILE_STARTS: [u16; 8] =
    [0x1C8, 0x1D8, 0x1E8, 0x1F8, 0x208, 0x218, 0x228, 0x238];

fn build_menu_map_hook_site() -> Result<Vec<u8>> {
    assemble(&[Inst::Jsl(MENU_MAP_CODE_LONG), Inst::Nop])
        .map_err(anyhow::Error::msg)
        .context("assemble Remix multi-menu hook site")
}

/// Build the multi-menu tilemap remapper with the same Rust `Inst`/two-pass
/// assembler pattern used by the sibling SNES projects.
fn build_menu_map_code() -> Result<Vec<u8>> {
    use Inst::*;

    let program = [
        Sep(0x20),
        Phb,
        Phx,
        LdaImm8(0x7E),
        Pha,
        Plb,
        Rep(0x20),
        // Clear every original label rectangle before filling the Korean
        // canvases: the 27x4 three- and four-player rows, settings 14x4 and
        // return-title 28x4. The Korean player canvases are one column
        // narrower, and an uncleared original column would keep referencing
        // pool tiles that now hold Korean pixels.
        LdxImm16(0x04C0),
        Pea(0x003A),
        Pea(0x0244),
        LdyImm16(0x0144),
        Jsr("clear_rect"),
        Pla,
        Pla,
        Pea(0x003A),
        Pea(0x0384),
        LdyImm16(0x0284),
        Jsr("clear_rect"),
        Pla,
        Pla,
        Pea(0x0020),
        Pea(0x04C4),
        LdyImm16(0x03C4),
        Jsr("clear_rect"),
        Pla,
        Pla,
        Pea(0x003C),
        Pea(0x0604),
        LdyImm16(0x0504),
        Jsr("clear_rect"),
        Pla,
        Pla,
        // Three-player phrase: the full 26x4 master canvas.
        Pea(0x0038),
        Pea(0x0244),
        LdxImm16(safe_tile(THREE_POOL)),
        LdyImm16(0x0144),
        Jsr("fill_rect"),
        Pla,
        Pla,
        // Four-player phrase: a unique four-column prefix, followed by the
        // byte-identical 22-column suffix from the three-player master.
        Pea(0x000C),
        Pea(0x0384),
        LdxImm16(safe_tile(FOUR_PREFIX_POOL)),
        LdyImm16(0x0284),
        Jsr("fill_rect"),
        Pla,
        Pla,
        Pea(0x0038),
        Pea(0x02C4),
        LdxImm16(safe_tile(THREE_POOL + 4)),
        LdyImm16(0x028C),
        Jsr("fill_rect"),
        Pla,
        Pla,
        Pea(0x0038),
        Pea(0x0304),
        LdxImm16(safe_tile(THREE_POOL + PLAYER_WIDTH_TILES + 4)),
        LdyImm16(0x02CC),
        Jsr("fill_rect"),
        Pla,
        Pla,
        Pea(0x0038),
        Pea(0x0344),
        LdxImm16(safe_tile(THREE_POOL + 2 * PLAYER_WIDTH_TILES + 4)),
        LdyImm16(0x030C),
        Jsr("fill_rect"),
        Pla,
        Pla,
        Pea(0x0038),
        Pea(0x0384),
        LdxImm16(safe_tile(THREE_POOL + 3 * PLAYER_WIDTH_TILES + 4)),
        LdyImm16(0x034C),
        Jsr("fill_rect"),
        Pla,
        Pla,
        // Settings: 8x4 inside the original 14x4 rectangle.
        Pea(0x0014),
        Pea(0x04C4),
        LdxImm16(safe_tile(SETTINGS_POOL)),
        LdyImm16(0x03C4),
        Jsr("fill_rect"),
        Pla,
        Pla,
        // Return title: 25x4 inside the original 28x4 rectangle.
        Pea(0x0036),
        Pea(0x0604),
        LdxImm16(safe_tile(TITLE_POOL)),
        LdyImm16(0x0504),
        Jsr("fill_rect"),
        Pla,
        Pla,
        // Restore replaced SEP/LDY effects and caller state.
        Sep(0x20),
        Plx,
        Plb,
        LdyImm16(0xC1F5),
        Rtl,
        // Stack parameters below the JSR return address:
        //   3,S = first destination after the rectangle
        //   5,S = low-six-bit destination after each row
        Label("clear_rect"),
        Txa,
        StaAbsY(0x4000),
        Iny,
        Iny,
        Tya,
        AndImm16(0x003F),
        CmpStackRelative(5),
        Bne("clear_rect"),
        Tya,
        AndImm16(0xFFC0),
        Clc,
        AdcImm16(0x0044),
        Tay,
        CmpStackRelative(3),
        Bne("clear_rect"),
        Rts,
        Label("fill_rect"),
        Txa,
        OraImm16(0x1800),
        StaAbsY(0x4000),
        Inx,
        // BG2 uses physical CHR IDs 0x1C8..0x23F while BG1 addresses the
        // same bytes through a 0x100-tile base offset. Skip the converted BG1
        // logical IDs, not the BG2 tilemap values.
        CpxImm16(0x00C8),
        Bne("fill_skip_0d8"),
        LdxImm16(0x00D0),
        Label("fill_skip_0d8"),
        CpxImm16(0x00D8),
        Bne("fill_skip_0e8"),
        LdxImm16(0x00E0),
        Label("fill_skip_0e8"),
        CpxImm16(0x00E8),
        Bne("fill_skip_0f8"),
        LdxImm16(0x00F0),
        Label("fill_skip_0f8"),
        CpxImm16(0x00F8),
        Bne("fill_skip_108"),
        LdxImm16(0x0100),
        Label("fill_skip_108"),
        CpxImm16(0x0108),
        Bne("fill_skip_118"),
        LdxImm16(0x0110),
        Label("fill_skip_118"),
        CpxImm16(0x0118),
        Bne("fill_skip_128"),
        LdxImm16(0x0120),
        Label("fill_skip_128"),
        CpxImm16(0x0128),
        Bne("fill_skip_138"),
        LdxImm16(0x0130),
        Label("fill_skip_138"),
        CpxImm16(0x0138),
        Bne("fill_advance_destination"),
        LdxImm16(0x0140),
        Label("fill_advance_destination"),
        Iny,
        Iny,
        Tya,
        AndImm16(0x003F),
        CmpStackRelative(5),
        Bne("fill_rect"),
        Tya,
        AndImm16(0xFFC0),
        Clc,
        AdcImm16(0x0044),
        Tay,
        CmpStackRelative(3),
        Bne("fill_rect"),
        Rts,
    ];

    assemble_at(MENU_MAP_CODE_ORIGIN, &program)
        .map_err(anyhow::Error::msg)
        .context("assemble Remix multi-menu tilemap code")
}

const BRUSH_SHEET_FILE: &str = "minna_brush_sheet.png";
// Label canvases draw only the index roles of the original brush labels, so the
// game's palette-1 (selected) and palette-6 (unselected) swaps keep working.
// PALETTE repeats color B50808 at indices 3 and 8; exact PNG decoding therefore
// resolves colors inside this role set only.
const LABEL_INDICES: [u8; 8] = [1, 8, 9, 10, 11, 12, 13, 15];
const PLAYER_TEXT_HEIGHT: usize = 28;
const TITLE_TEXT_HEIGHT: usize = 26;
const BRUSH_COVERAGE: f32 = 0.65;

// Runtime palette 1 from the verified minna_submenu CGRAM dump. PNG colors are
// an authoring contract; the importer converts them back to these indices.
const PALETTE: [[u8; 3]; 16] = [
    [0x63, 0x63, 0x42],
    [0x00, 0x00, 0x00],
    [0x63, 0x00, 0x21],
    [0xB5, 0x08, 0x08],
    [0xEF, 0x19, 0x19],
    [0xFF, 0x42, 0x42],
    [0xFF, 0x73, 0x73],
    [0xE6, 0xC5, 0xC5],
    [0xB5, 0x08, 0x08],
    [0xC5, 0x3A, 0x08],
    [0xD6, 0x6B, 0x08],
    [0xDE, 0x9C, 0x00],
    [0xEF, 0xCE, 0x00],
    [0xFF, 0xFF, 0x00],
    [0x00, 0x00, 0x42],
    [0xFF, 0xFF, 0xFF],
];

const fn tile_is_background_owned(tile: u16) -> bool {
    let mut index = 0;
    while index < BG2_BACKGROUND_TILE_STARTS.len() {
        let start = bg2_tile_to_bg1_tile(BG2_BACKGROUND_TILE_STARTS[index]);
        if tile >= start && tile < start + 8 {
            return true;
        }
        index += 1;
    }
    false
}

const fn bg2_tile_to_bg1_tile(tile: u16) -> u16 {
    tile - ((BG1_CHR_BASE_BYTE - BG2_CHR_BASE_BYTE) / TILE_LEN) as u16
}

const fn safe_tile(pool_index: usize) -> u16 {
    let mut tile = 0x0C1;
    let mut index = 0;
    loop {
        if !tile_is_background_owned(tile) {
            if index == pool_index {
                return tile;
            }
            index += 1;
        }
        tile += 1;
    }
}

const fn rectangle_tiles<const N: usize>(
    width_tiles: usize,
    active_width: usize,
    active_rows: usize,
    pool_offset: usize,
) -> [u16; N] {
    let mut tiles = [0x0C0; N];
    let mut row = 0;
    let mut used = 0;
    while row < active_rows {
        let mut column = 0;
        while column < active_width {
            tiles[row * width_tiles + column] = safe_tile(pool_offset + used);
            used += 1;
            column += 1;
        }
        row += 1;
    }
    tiles
}

const fn player_tiles(pool_offset: usize) -> [u16; PLAYER_WIDTH_TILES * 4] {
    let mut tiles = [0x0C0; PLAYER_WIDTH_TILES * 4];
    let mut slot = 0;
    while slot < PLAYER_WIDTH_TILES * 4 {
        tiles[slot] = safe_tile(pool_offset + slot);
        slot += 1;
    }
    tiles
}

const fn four_player_tiles() -> [u16; PLAYER_WIDTH_TILES * 4] {
    let mut tiles = [0x0C0; PLAYER_WIDTH_TILES * 4];
    let mut slot = 0;
    while slot < PLAYER_WIDTH_TILES * 4 {
        let column = slot % PLAYER_WIDTH_TILES;
        let row = slot / PLAYER_WIDTH_TILES;
        tiles[slot] = if column < 4 {
            safe_tile(FOUR_PREFIX_POOL + row * 4 + column)
        } else {
            THREE_TILES[slot]
        };
        slot += 1;
    }
    tiles
}

// The four-player label owns only its distinct four-column prefix and shares
// the coherent suffix from the simultaneously generated three-player master.
// Settings and return-title keep their measured outer rectangles, which the
// hook clears to 0x04C0 before filling the active 8x4 and 25x4 canvases. The
// pool holds 255 background-safe tiles from 0x0C1 to 0x1FF; the four canvases
// use 252 of them, which fixes the canvas sizes.
const PLAYER_WIDTH_TILES: usize = 26;
const SETTINGS_WIDTH_TILES: usize = 8;
const TITLE_WIDTH_TILES: usize = 25;
const THREE_POOL: usize = 0;
const FOUR_PREFIX_POOL: usize = THREE_POOL + PLAYER_WIDTH_TILES * 4;
const SETTINGS_POOL: usize = FOUR_PREFIX_POOL + 16;
const TITLE_POOL: usize = SETTINGS_POOL + SETTINGS_WIDTH_TILES * 4;
const POOL_TILES_USED: usize = TITLE_POOL + TITLE_WIDTH_TILES * 4;
const _: () = assert!(safe_tile(POOL_TILES_USED - 1) <= 0x1FF);
// Original label rectangles cleared by the hook: three- and four-player 27x4,
// settings 14x4 and return title 28x4.
const CLEARED_ENTRIES: usize = 2 * 27 * 4 + 14 * 4 + 28 * 4;

const THREE_TILES: [u16; PLAYER_WIDTH_TILES * 4] = player_tiles(THREE_POOL);
const FOUR_TILES: [u16; PLAYER_WIDTH_TILES * 4] = four_player_tiles();
const SETTINGS_TILES: [u16; SETTINGS_WIDTH_TILES * 4] =
    rectangle_tiles(SETTINGS_WIDTH_TILES, SETTINGS_WIDTH_TILES, 4, SETTINGS_POOL);
const TITLE_TILES: [u16; TITLE_WIDTH_TILES * 4] =
    rectangle_tiles(TITLE_WIDTH_TILES, TITLE_WIDTH_TILES, 4, TITLE_POOL);

#[derive(Clone, Copy)]
struct LabelSpec {
    id: &'static str,
    ko: &'static str,
    output: &'static str,
    width_tiles: usize,
    tiles: &'static [u16],
}

const LABELS: [LabelSpec; 4] = [
    LabelSpec {
        id: "three_player",
        ko: "셋이서 뿌요뿌요",
        output: "minna_three_player.png",
        width_tiles: PLAYER_WIDTH_TILES,
        tiles: &THREE_TILES,
    },
    LabelSpec {
        id: "four_player",
        ko: "넷이서 뿌요뿌요",
        output: "minna_four_player.png",
        width_tiles: PLAYER_WIDTH_TILES,
        tiles: &FOUR_TILES,
    },
    LabelSpec {
        id: "settings",
        ko: "설정",
        output: "minna_settings.png",
        width_tiles: SETTINGS_WIDTH_TILES,
        tiles: &SETTINGS_TILES,
    },
    LabelSpec {
        id: "return_title",
        ko: "타이틀로 돌아가기",
        output: "minna_return_title.png",
        width_tiles: TITLE_WIDTH_TILES,
        tiles: &TITLE_TILES,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreparedAssetReport {
    pub id: String,
    pub text: String,
    pub input: String,
    pub input_sha256: String,
    pub dimensions: String,
    pub output: String,
    pub output_sha256: String,
    pub opaque_pixels: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrepareAssetsReport {
    pub verdict: String,
    pub method: String,
    pub shared_tile_patterns: usize,
    pub assets: Vec<PreparedAssetReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemixMenuPocReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub assets_dir: String,
    pub output_path: String,
    pub output_sha256: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub protected_tiles_unchanged: usize,
    pub tilemap_entries_remapped: usize,
    pub tilemap_entries_cleared: usize,
    pub menu_tilemap_hook_pc: String,
    pub menu_tilemap_code_pc: String,
    pub runtime_dump: Option<String>,
    pub runtime_vram_matches_patched_chr: Option<bool>,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_registered_writes: bool,
    pub checksum_hex: String,
    pub asset_sha256: BTreeMap<String, String>,
}

pub fn prepare_assets(source_dir: &Path, out_dir: &Path) -> Result<PrepareAssetsReport> {
    let sheet_path = source_dir.join(BRUSH_SHEET_FILE);
    let sheet_bytes = fs::read(&sheet_path)
        .with_context(|| format!("read generated brush sheet {}", sheet_path.display()))?;
    let normalized = compose_brush_labels(&sheet_bytes)?;

    // Validate the physical CHR contract after composition. The two player
    // phrases share one exact suffix; only their four-column prefixes own
    // distinct tiles.
    let mut desired = BTreeMap::new();
    for (spec, image) in LABELS.iter().zip(&normalized) {
        collect_exact_tiles(&mut desired, spec, image)?;
    }

    fs::create_dir_all(out_dir)
        .with_context(|| format!("create Remix label asset directory {}", out_dir.display()))?;
    let mut reports = Vec::new();
    let input_sha256 = sha256(&sheet_bytes);
    for (spec, indices) in LABELS.iter().zip(normalized) {
        let rgba = indices_to_rgba(&indices);
        let encoded = encode_png_rgba(spec.width_tiles * 8, 32, &rgba)?;
        let output_path = out_dir.join(spec.output);
        fs::write(&output_path, &encoded)
            .with_context(|| format!("write normalized Remix label {}", output_path.display()))?;
        reports.push(PreparedAssetReport {
            id: spec.id.to_owned(),
            text: spec.ko.to_owned(),
            input: sheet_path.display().to_string(),
            input_sha256: input_sha256.clone(),
            dimensions: format!("{}x32", spec.width_tiles * 8),
            output: output_path.display().to_string(),
            output_sha256: sha256(&encoded),
            opaque_pixels: indices.iter().filter(|&&index| index != 0).count(),
        });
    }
    Ok(PrepareAssetsReport {
        verdict: "four Remix-only Korean brush labels shaded with the original label index roles and validated against the runtime tilemap rectangles"
            .to_owned(),
        method: "generated black-ink brush sheet; per-syllable area sampling; shared player-phrase suffix; original outline/ramp/light index roles; binary alpha; BG1-relative background-safe CHR ownership"
            .to_owned(),
        shared_tile_patterns: desired.len(),
        assets: reports,
    })
}

/// Sample the four phrases from the generated sheet and shade them. Rows are,
/// top to bottom: three-player, four-player, settings, return title.
fn compose_brush_labels(sheet_bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    use crate::generated_lettering::{PhraseStyle, decode_ink_sheet, shade_brush};

    let sheet = decode_ink_sheet(sheet_bytes)?;
    let rows = sheet.rows();
    if rows.len() != LABELS.len() {
        bail!(
            "{BRUSH_SHEET_FILE} must hold {} phrase rows, found {}",
            LABELS.len(),
            rows.len()
        );
    }
    let player_style = PhraseStyle {
        text_height: PLAYER_TEXT_HEIGHT,
        max_width: PLAYER_WIDTH_TILES * 8 - 4,
        syllable_gap: 2,
        word_space: 7,
        coverage_threshold: BRUSH_COVERAGE,
    };
    let player_y = (32 - PLAYER_TEXT_HEIGHT) / 2;
    let player_band = player_y..player_y + PLAYER_TEXT_HEIGHT;

    let (three_phrase, three_spans) = sheet.compose_phrase(&rows[0], LABELS[0].ko, player_style)?;
    let (four_phrase, four_spans) = sheet.compose_phrase(&rows[1], LABELS[1].ko, player_style)?;
    let player_width = PLAYER_WIDTH_TILES * 8;
    // The first syllable and its outline sit inside the four prefix columns,
    // right-aligned so its outline ends at x=31; the rest of the phrase starts
    // at x=33 so its outline begins in the shared suffix columns.
    let prefix_end: usize = 4 * 8;
    let suffix_x = prefix_end + 1;
    let place_first = |canvas: &mut Mask, first: &Mask| -> Result<()> {
        let x = (prefix_end - 1)
            .checked_sub(first.width + 1)
            .context("player-phrase first syllable is wider than the prefix columns")?;
        canvas.paste(first, x, player_y);
        Ok(())
    };
    let rest = crop_columns(
        &three_phrase,
        three_spans[1].start..three_spans.last().expect("phrase has syllables").end,
    );
    if suffix_x + rest.width + 1 > player_width {
        bail!("three-player phrase suffix does not fit its shared columns");
    }
    let mut three = Mask::new(player_width, 32);
    place_first(
        &mut three,
        &crop_columns(&three_phrase, three_spans[0].clone()),
    )?;
    three.paste(&rest, suffix_x, player_y);
    let mut four = Mask::new(player_width, 32);
    place_first(
        &mut four,
        &crop_columns(&four_phrase, four_spans[0].clone()),
    )?;
    four.paste(&rest, suffix_x, player_y);

    let three_indices = shade_brush(&three, player_band.clone());
    let mut four_indices = shade_brush(&four, player_band.clone());
    for y in 0..32 {
        for x in prefix_end..player_width {
            four_indices[y * player_width + x] = three_indices[y * player_width + x];
        }
    }

    let settings_width = SETTINGS_WIDTH_TILES * 8;
    let (settings_phrase, _) = sheet.compose_phrase(
        &rows[2],
        LABELS[2].ko,
        PhraseStyle {
            max_width: settings_width - 2,
            ..player_style
        },
    )?;
    let mut settings = Mask::new(settings_width, 32);
    settings.paste(
        &settings_phrase,
        1 + (settings_width - 2 - settings_phrase.width) / 2,
        player_y,
    );

    let title_width = TITLE_WIDTH_TILES * 8;
    let title_y = (32 - TITLE_TEXT_HEIGHT) / 2;
    let (title_phrase, _) = sheet.compose_phrase(
        &rows[3],
        LABELS[3].ko,
        PhraseStyle {
            text_height: TITLE_TEXT_HEIGHT,
            max_width: title_width - 4,
            syllable_gap: 1,
            word_space: 5,
            coverage_threshold: BRUSH_COVERAGE,
        },
    )?;
    let mut title = Mask::new(title_width, 32);
    title.paste(
        &title_phrase,
        2 + (title_width - 4 - title_phrase.width) / 2,
        title_y,
    );

    Ok(vec![
        three_indices,
        four_indices,
        shade_brush(&settings, player_band),
        shade_brush(&title, title_y..title_y + TITLE_TEXT_HEIGHT),
    ])
}

fn crop_columns(mask: &Mask, columns: Range<usize>) -> Mask {
    let mut cropped = Mask::new(columns.len(), mask.height);
    for y in 0..mask.height {
        for x in columns.clone() {
            cropped.bits[y * cropped.width + x - columns.start] = mask.bits[y * mask.width + x];
        }
    }
    cropped
}

pub fn build_poc(
    source: &[u8],
    source_path: String,
    assets_dir: &Path,
    runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, RemixMenuPocReport)> {
    build_impl(
        source,
        source_path,
        assets_dir,
        runtime_dump,
        output_path,
        true,
    )
}

pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    assets_dir: &Path,
    runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, RemixMenuPocReport)> {
    build_impl(
        source,
        source_path,
        assets_dir,
        runtime_dump,
        output_path,
        false,
    )
}

fn build_impl(
    source: &[u8],
    source_path: String,
    assets_dir: &Path,
    runtime_dump: Option<&Path>,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, RemixMenuPocReport)> {
    let source_sha256 = sha256(source);
    if require_original_identity && source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    let block = crate::snes_lz::decompress(source, STREAM_PC)?;
    if block.compressed_len != EXPECTED_COMPRESSED_LEN
        || block.bytes.len() != EXPECTED_DECOMPRESSED_LEN
    {
        bail!(
            "Remix menu CHR stream differs from the verified contract: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let raw = source
        .get(STREAM_PC..STREAM_PC + block.compressed_len)
        .context("verified Remix menu CHR stream is outside ROM")?;
    if sha256(raw) != EXPECTED_RAW_SHA256 || sha256(&block.bytes) != EXPECTED_DECODED_SHA256 {
        bail!("Remix menu CHR stream hash differs from the verified runtime-owned resource");
    }
    let growth = source
        .get(STREAM_PC + block.compressed_len..STREAM_BANK_END)
        .context("Remix menu CHR growth range is outside ROM")?;
    if growth.iter().any(|&byte| byte != 0xFF) {
        bail!("Remix menu CHR growth range is not the verified 0xFF bank tail");
    }

    let original_decoded = block.bytes;
    let mut patched_decoded = original_decoded.clone();
    let mut desired: BTreeMap<u16, [u8; 64]> = BTreeMap::new();
    let mut asset_sha256 = BTreeMap::new();
    for spec in LABELS {
        let path = assets_dir.join(spec.output);
        let bytes = fs::read(&path)
            .with_context(|| format!("read normalized Remix label {}", path.display()))?;
        let image = decode_rgba8_png(&bytes)?;
        if image.width != spec.width_tiles * 8 || image.height != 32 {
            bail!(
                "{} dimensions are {}x{}, expected {}x32",
                path.display(),
                image.width,
                image.height,
                spec.width_tiles * 8
            );
        }
        let indices = exact_asset_indices(&image)
            .with_context(|| format!("validate normalized Remix label {}", path.display()))?;
        collect_exact_tiles(&mut desired, &spec, &indices)?;
        asset_sha256.insert(spec.id.to_owned(), sha256(&bytes));
    }
    if desired
        .get(&0x0C0)
        .is_some_and(|tile| tile.iter().any(|&pixel| pixel != 0))
    {
        bail!("shared transparent tile 0x0C0 is not transparent in all label assets");
    }

    let editable_tiles: Vec<u16> = desired.keys().copied().collect();
    for (&tile_id, pixels) in &desired {
        let stream_tile = stream_tile_index(tile_id)?;
        let start = stream_tile * TILE_LEN;
        encode_4bpp_tile(pixels, &mut patched_decoded[start..start + TILE_LEN]);
    }
    let changed_tiles = original_decoded
        .as_chunks::<TILE_LEN>()
        .0
        .iter()
        .zip(patched_decoded.as_chunks::<TILE_LEN>().0)
        .filter(|(before, after)| before != after)
        .count();
    let changed_decompressed_bytes = original_decoded
        .iter()
        .zip(&patched_decoded)
        .filter(|(before, after)| before != after)
        .count();
    for (tile, (before, after)) in original_decoded
        .as_chunks::<TILE_LEN>()
        .0
        .iter()
        .zip(patched_decoded.as_chunks::<TILE_LEN>().0)
        .enumerate()
    {
        let absolute_tile =
            ((STREAM_VRAM_BYTE + tile * TILE_LEN - BG1_CHR_BASE_BYTE) / TILE_LEN) as u16;
        if before != after && !editable_tiles.contains(&absolute_tile) {
            bail!("protected CHR tile 0x{absolute_tile:03X} changed outside the label tile set");
        }
    }

    let compressed = crate::snes_lz::compress(&patched_decoded);
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.bytes == patched_decoded && roundtrip.compressed_len == compressed.len();
    if !compression_roundtrip_matches {
        bail!("recompressed Remix menu CHR stream failed its round trip");
    }
    let capacity = STREAM_BANK_END - STREAM_PC;
    if compressed.len() > capacity {
        bail!(
            "recompressed Remix menu CHR needs {} bytes, exceeding the verified bank-tail capacity {}",
            compressed.len(),
            capacity
        );
    }

    let runtime_vram_matches_patched_chr = runtime_dump
        .map(|dump| {
            let path = dump.join("vram.bin");
            let vram = fs::read(&path)
                .with_context(|| format!("read patched Remix runtime VRAM {}", path.display()))?;
            if vram.len() != 64 * 1024 {
                bail!("patched Remix runtime VRAM must be 65536 bytes");
            }
            let matches = vram.get(STREAM_VRAM_BYTE..STREAM_VRAM_BYTE + patched_decoded.len())
                == Some(patched_decoded.as_slice());
            if !matches {
                bail!("patched Remix runtime VRAM does not contain the built menu CHR at 0x3800");
            }
            Ok(matches)
        })
        .transpose()?;

    let write_len = block.compressed_len.max(compressed.len());
    let mut patched = source.to_vec();
    apply_menu_tilemap_hook(source, &mut patched)?;
    patched[STREAM_PC..STREAM_PC + write_len].fill(0xFF);
    patched[STREAM_PC..STREAM_PC + compressed.len()].copy_from_slice(&compressed);
    let checksum = crate::rom::fix_checksum(&mut patched)?;
    let diff_confined_to_registered_writes =
        source
            .iter()
            .zip(&patched)
            .enumerate()
            .all(|(index, (before, after))| {
                before == after
                    || (STREAM_PC..STREAM_PC + write_len).contains(&index)
                    || MENU_MAP_HOOK_RANGE.contains(&index)
                    || MENU_MAP_CODE_RANGE.contains(&index)
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&index)
            });
    if !diff_confined_to_registered_writes {
        bail!("PoC ROM contains a change outside the registered stream and checksum writes");
    }

    Ok((
        patched.clone(),
        RemixMenuPocReport {
            verdict: "four Remix-only generated Korean menu labels inserted as coherent phrase canvases while preserving the shared BG2 background CHR"
                .to_owned(),
            source_path,
            source_sha256,
            assets_dir: assets_dir.display().to_string(),
            output_path,
            output_sha256: sha256(&patched),
            stream_pc: format!("0x{STREAM_PC:06X}"),
            stream_lorom: "$27:$D321".to_owned(),
            original_compressed_len: block.compressed_len,
            patched_compressed_len: compressed.len(),
            decompressed_len: patched_decoded.len(),
            changed_tiles,
            changed_decompressed_bytes,
            protected_tiles_unchanged: original_decoded.len() / TILE_LEN - changed_tiles,
            tilemap_entries_remapped: LABELS.iter().map(|spec| spec.tiles.len()).sum(),
            tilemap_entries_cleared: CLEARED_ENTRIES
                - LABELS.iter().map(|spec| spec.tiles.len()).sum::<usize>(),
            menu_tilemap_hook_pc: format!("0x{:06X}", MENU_MAP_HOOK_RANGE.start),
            menu_tilemap_code_pc: format!("0x{:06X}", MENU_MAP_CODE_RANGE.start),
            runtime_dump: runtime_dump.map(|path| path.display().to_string()),
            runtime_vram_matches_patched_chr,
            compression_roundtrip_matches,
            diff_confined_to_registered_writes,
            checksum_hex: format!("0x{checksum:04X}"),
            asset_sha256,
        },
    ))
}

fn apply_menu_tilemap_hook(source: &[u8], patched: &mut [u8]) -> Result<()> {
    let hook_site = build_menu_map_hook_site()?;
    let code = build_menu_map_code()?;
    if hook_site.len() != MENU_MAP_HOOK_RANGE.len() {
        bail!(
            "assembled multi-menu hook site is {} bytes, expected {}",
            hook_site.len(),
            MENU_MAP_HOOK_RANGE.len()
        );
    }
    if code.len() > MENU_MAP_CODE_RANGE.len() {
        bail!(
            "assembled multi-menu code is {} bytes, exceeding verified range capacity {}",
            code.len(),
            MENU_MAP_CODE_RANGE.len()
        );
    }
    if source.get(MENU_MAP_HOOK_RANGE.clone()) != Some(MENU_MAP_HOOK_EXPECTED.as_slice()) {
        bail!(
            "multi-menu tilemap hook differs at PC 0x{:06X}",
            MENU_MAP_HOOK_RANGE.start
        );
    }
    let code_source = source
        .get(MENU_MAP_CODE_RANGE.clone())
        .context("multi-menu tilemap hook code range is outside ROM")?;
    if code_source.iter().any(|&byte| byte != 0xFF) {
        bail!(
            "multi-menu tilemap hook code range is not the verified 0xFF gap at PC 0x{:06X}",
            MENU_MAP_CODE_RANGE.start
        );
    }
    patched[MENU_MAP_HOOK_RANGE.clone()].copy_from_slice(&hook_site);
    patched[MENU_MAP_CODE_RANGE.start..MENU_MAP_CODE_RANGE.start + code.len()]
        .copy_from_slice(&code);
    Ok(())
}

#[derive(Clone)]
struct RgbaImage {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

fn decode_rgba8_png(encoded: &[u8]) -> Result<RgbaImage> {
    let decoder = png::Decoder::new(Cursor::new(encoded));
    let mut reader = decoder.read_info().context("read PNG info")?;
    let mut pixels = vec![
        0u8;
        reader
            .output_buffer_size()
            .context("PNG output is too large")?
    ];
    let info = reader.next_frame(&mut pixels).context("decode PNG frame")?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!(
            "PNG must be 8-bit RGBA, got {:?} {:?}",
            info.color_type,
            info.bit_depth
        );
    }
    pixels.truncate(info.buffer_size());
    Ok(RgbaImage {
        width: info.width as usize,
        height: info.height as usize,
        pixels,
    })
}

fn collect_exact_tiles(
    desired: &mut BTreeMap<u16, [u8; 64]>,
    spec: &LabelSpec,
    image: &[u8],
) -> Result<()> {
    let width = spec.width_tiles * 8;
    for (slot, &tile_id) in spec.tiles.iter().enumerate() {
        let pattern = canvas_tile(
            image,
            width,
            slot % spec.width_tiles,
            slot / spec.width_tiles,
        );
        if let Some(previous) = desired.get(&tile_id) {
            if previous != &pattern {
                bail!(
                    "{} assigns conflicting pixels to shared CHR tile 0x{tile_id:03X}",
                    spec.id
                );
            }
        } else {
            desired.insert(tile_id, pattern);
        }
    }
    Ok(())
}

fn canvas_tile(image: &[u8], width: usize, tile_x: usize, tile_y: usize) -> [u8; 64] {
    let mut tile = [0u8; 64];
    for y in 0..8 {
        for x in 0..8 {
            tile[y * 8 + x] = image[(tile_y * 8 + y) * width + tile_x * 8 + x];
        }
    }
    tile
}

fn indices_to_rgba(indices: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(indices.len() * 4);
    for &index in indices {
        if index == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            rgba.extend_from_slice(&[
                PALETTE[usize::from(index)][0],
                PALETTE[usize::from(index)][1],
                PALETTE[usize::from(index)][2],
                0xFF,
            ]);
        }
    }
    rgba
}

fn exact_asset_indices(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(image.width * image.height);
    for (pixel_number, pixel) in image.pixels.as_chunks::<4>().0.iter().enumerate() {
        match pixel[3] {
            0 => output.push(0),
            0xFF => {
                let rgb = [pixel[0], pixel[1], pixel[2]];
                let index = LABEL_INDICES
                    .into_iter()
                    .find(|&index| PALETTE[usize::from(index)] == rgb)
                    .with_context(|| {
                        format!(
                            "pixel {},{} uses a color outside the label index roles #{:02X}{:02X}{:02X}",
                            pixel_number % image.width,
                            pixel_number / image.width,
                            rgb[0],
                            rgb[1],
                            rgb[2]
                        )
                    })?;
                output.push(index);
            }
            alpha => bail!(
                "pixel {},{} has non-binary alpha {alpha}",
                pixel_number % image.width,
                pixel_number / image.width
            ),
        }
    }
    Ok(output)
}

fn stream_tile_index(tile_id: u16) -> Result<usize> {
    let tile_byte = BG1_CHR_BASE_BYTE + usize::from(tile_id) * TILE_LEN;
    let stream_tile = tile_byte
        .checked_sub(STREAM_VRAM_BYTE)
        .context("label tile is before the Remix CHR stream")?
        / TILE_LEN;
    if stream_tile >= EXPECTED_DECOMPRESSED_LEN / TILE_LEN {
        bail!("label tile 0x{tile_id:03X} is outside the Remix CHR stream");
    }
    Ok(stream_tile)
}

fn encode_4bpp_tile(pixels: &[u8; 64], output: &mut [u8]) {
    debug_assert_eq!(output.len(), TILE_LEN);
    output.fill(0);
    for y in 0..8 {
        for x in 0..8 {
            let value = pixels[y * 8 + x];
            let bit = 7 - x;
            for plane in 0..4 {
                let byte = if plane < 2 {
                    y * 2 + plane
                } else {
                    16 + y * 2 + plane - 2
                };
                output[byte] |= ((value >> plane) & 1) << bit;
            }
        }
    }
}

fn encode_png_rgba(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != width * height * 4 {
        bail!("PNG RGBA buffer length does not match {width}x{height}");
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            &mut encoded,
            u32::try_from(width).context("PNG width exceeds u32")?,
            u32::try_from(height).context("PNG height exceeds u32")?,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write PNG header")?;
        writer.write_image_data(rgba).context("write PNG pixels")?;
    }
    Ok(encoded)
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_maps_are_four_rows_and_inside_stream() {
        for spec in LABELS {
            assert_eq!(spec.tiles.len(), spec.width_tiles * 4);
            for &tile in spec.tiles {
                stream_tile_index(tile).unwrap();
            }
        }
    }

    #[test]
    fn player_phrase_tiles_share_one_background_safe_suffix() {
        assert_eq!(THREE_TILES[0], 0x0C1);
        assert_eq!(bg2_tile_to_bg1_tile(0x1C8), 0x0C8);
        assert!(tile_is_background_owned(0x0C8));
        assert!(tile_is_background_owned(0x13F));
        assert!(!tile_is_background_owned(0x1C8));

        let mut owned = BTreeMap::new();
        for spec in LABELS {
            for &tile in spec.tiles {
                if tile != 0x0C0 {
                    assert!(!tile_is_background_owned(tile));
                    owned.insert(tile, spec.id);
                }
            }
        }
        assert_eq!(owned.len(), POOL_TILES_USED);
        assert!(owned.keys().all(|&tile| tile <= 0x1FF));

        for row in 0..4 {
            for column in 0..PLAYER_WIDTH_TILES {
                let slot = row * PLAYER_WIDTH_TILES + column;
                if column < 4 {
                    assert_ne!(FOUR_TILES[slot], THREE_TILES[slot]);
                    assert!(!THREE_TILES.contains(&FOUR_TILES[slot]));
                } else {
                    assert_eq!(FOUR_TILES[slot], THREE_TILES[slot]);
                }
            }
        }
    }

    #[test]
    fn settings_and_return_title_fill_their_active_canvases() {
        assert!(SETTINGS_TILES.iter().all(|&tile| tile != 0x0C0));
        assert!(TITLE_TILES.iter().all(|&tile| tile != 0x0C0));
        assert_eq!(SETTINGS_TILES[0], safe_tile(SETTINGS_POOL));
        assert_eq!(TITLE_TILES[0], safe_tile(TITLE_POOL));
    }

    #[test]
    fn label_role_colors_resolve_to_role_indices() {
        let rgba = indices_to_rgba(&LABEL_INDICES);
        let image = RgbaImage {
            width: LABEL_INDICES.len(),
            height: 1,
            pixels: rgba,
        };
        assert_eq!(exact_asset_indices(&image).unwrap(), LABEL_INDICES);
    }

    #[test]
    fn menu_map_hook_matches_assembled_code_contract() {
        let hook_site = build_menu_map_hook_site().unwrap();
        let code = build_menu_map_code().unwrap();
        assert_eq!(MENU_MAP_HOOK_RANGE.len(), hook_site.len());
        assert_eq!(code.len(), 337);
        assert!(code.len() <= MENU_MAP_CODE_RANGE.len());
        assert_eq!(MENU_MAP_CODE_RANGE.end, 0x16_5200);
        assert_eq!(hook_site, [0x22, 0x00, 0xD0, 0xAC, 0xEA]);
        assert_eq!(
            sha256(&code),
            "7df2bcbe243f476e8200c10cf86644a3d7c671d29fef423701f003de8fa219cd"
        );
        assert_eq!(
            &code[..10],
            &[0xE2, 0x20, 0x8B, 0xDA, 0xA9, 0x7E, 0x48, 0xAB, 0xC2, 0x20]
        );
        assert!(
            code.windows(8)
                .any(|bytes| bytes == [0xE2, 0x20, 0xFA, 0xAB, 0xA0, 0xF5, 0xC1, 0x6B])
        );
    }

    #[test]
    fn four_bpp_encoder_uses_snes_plane_order() {
        let mut pixels = [0u8; 64];
        pixels[0] = 1;
        pixels[1] = 2;
        pixels[2] = 4;
        pixels[3] = 8;
        let mut tile = [0u8; TILE_LEN];
        encode_4bpp_tile(&pixels, &mut tile);
        assert_eq!(tile[0], 0x80);
        assert_eq!(tile[1], 0x40);
        assert_eq!(tile[16], 0x20);
        assert_eq!(tile[17], 0x10);
    }

    #[test]
    #[ignore = "requires assets/menu_graphics/remix_labels/ brush sheet"]
    fn brush_labels_have_no_detached_ink_specks() {
        let bytes = crate::test_input::read(format!(
            "assets/menu_graphics/remix_labels/{BRUSH_SHEET_FILE}"
        ));
        for (label, indices) in compose_brush_labels(&bytes)
            .expect("compose brush labels")
            .iter()
            .enumerate()
        {
            let width = indices.len() / 32;
            let ink = |x: usize, y: usize| indices[y * width + x] > 1;
            let mut seen = vec![false; indices.len()];
            for start in 0..indices.len() {
                if seen[start] || !ink(start % width, start / width) {
                    continue;
                }
                seen[start] = true;
                let mut stack = vec![(start % width, start / width)];
                let mut pixels = 0;
                while let Some((x, y)) = stack.pop() {
                    pixels += 1;
                    for ny in y.saturating_sub(1)..(y + 2).min(32) {
                        for nx in x.saturating_sub(1)..(x + 2).min(width) {
                            if ink(nx, ny) && !seen[ny * width + nx] {
                                seen[ny * width + nx] = true;
                                stack.push((nx, ny));
                            }
                        }
                    }
                }
                assert!(
                    pixels > 4,
                    "label {label} has a detached {pixels}px ink speck at x={}",
                    start % width
                );
            }
        }
    }
}
