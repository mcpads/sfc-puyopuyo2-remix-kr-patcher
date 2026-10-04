use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Cursor,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const STREAM_PC: usize = 0x0F_C5FE;
const EXPECTED_COMPRESSED_LEN: usize = 10_456;
const EXPECTED_DECOMPRESSED_LEN: usize = 12_800;
const EXPECTED_RAW_SHA256: &str =
    "1b762fa0349f90c412d326cbdc46e3775e510bcebcf4b1f6d360fc20862e8be9";
const EXPECTED_DECODED_SHA256: &str =
    "6bc1ce0fbeed6351789ee21e2bdb5ab5e28bd4acd02336d580db5a2a9b7fc5dc";
const CHECKSUM_PC: usize = 0x7FDC;

const VRAM_LEN: usize = 64 * 1024;
const CGRAM_LEN: usize = 512;
const TILE_LEN: usize = 32;
const TILEMAP_HEIGHT: usize = 32;
const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 224;
const GENERATED_LABEL_X_SHIFT: isize = -1;
const GENERATED_LABEL_Y_SHIFT: isize = -2;

#[derive(Debug, Clone, Copy)]
struct Bounds {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[derive(Debug, Clone, Copy)]
struct Layer {
    tilemap_base_byte: usize,
    tilemap_width: usize,
}

const MODE_LAYER: Layer = Layer {
    tilemap_base_byte: 0xD000,
    tilemap_width: 64,
};
const TOKOTON_RIGHT_LAYER: Layer = Layer {
    tilemap_base_byte: 0xD800,
    tilemap_width: 32,
};

#[derive(Debug, Clone, Copy)]
enum Screen {
    ModeSelect,
    SoloType,
    TokotonOption,
}

#[derive(Debug, Clone, Copy)]
struct LabelSpec {
    id: &'static str,
    text: &'static str,
    screen: Screen,
    bounds: Bounds,
    asset_width: usize,
    owned_width: usize,
    core_palette_indices: &'static [u8],
    edge_palette_indices: &'static [u8],
}

const COMMON_CORE: &[u8] = &[9, 10, 11];
const COMMON_EDGE: &[u8] = &[1, 2, 5, 7, 12, 14];
const OPTION_CORE: &[u8] = &[15];
const OPTION_EDGE: &[u8] = &[1, 2, 5, 6, 7, 10, 11, 12, 14];
/// The label canvas crops the mouth and the Japanese lettering; classification
/// looks at the canvas grown by (sides, top, bottom) so the mouth outline, not
/// the canvas edge, bounds the frame.
const SIGN_MARGIN: (usize, usize, usize) = (8, 4, 2);
/// Chebyshev radius of the glow baked around the Japanese lettering.
const SIGN_GLOW_RADIUS: usize = 4;

/// Palette-2 roles of a sign: red fill 3/4, lettering in the label's core
/// colors plus the pink 10/11 edges, and the dark outline 1 carrying the fangs.
fn sign_colors(spec: LabelSpec) -> crate::mouth_sign::SignColors<'static> {
    crate::mouth_sign::SignColors {
        fill: &[3, 4],
        lettering: if spec.core_palette_indices == OPTION_CORE {
            &[10, 11, 15]
        } else {
            &[9, 10, 11]
        },
        outline: 1,
        palette: 2,
        // Each fang is about ten pixels; a letter stroke is far larger.
        max_fang_pixels: 24,
    }
}

const LABELS: [LabelSpec; 8] = [
    LabelSpec {
        id: "mode_solo",
        text: "혼자서 / 뿌요뿌요",
        screen: Screen::ModeSelect,
        bounds: Bounds {
            x: 32,
            y: 144,
            width: 48,
            height: 40,
        },
        asset_width: 48,
        owned_width: 48,
        core_palette_indices: COMMON_CORE,
        edge_palette_indices: COMMON_EDGE,
    },
    LabelSpec {
        id: "mode_two_player",
        text: "둘이서 / 뿌요뿌요",
        screen: Screen::ModeSelect,
        bounds: Bounds {
            x: 111,
            y: 144,
            width: 49,
            height: 40,
        },
        asset_width: 49,
        owned_width: 49,
        core_palette_indices: COMMON_CORE,
        edge_palette_indices: COMMON_EDGE,
    },
    LabelSpec {
        id: "mode_multi",
        text: "다함께 / 뿌요뿌요",
        screen: Screen::ModeSelect,
        bounds: Bounds {
            x: 191,
            y: 144,
            width: 49,
            height: 40,
        },
        asset_width: 49,
        owned_width: 49,
        core_palette_indices: COMMON_CORE,
        edge_palette_indices: COMMON_EDGE,
    },
    LabelSpec {
        id: "mode_tokoton",
        text: "무한 / 뿌요뿌요",
        screen: Screen::TokotonOption,
        bounds: Bounds {
            x: 16,
            y: 144,
            width: 48,
            height: 40,
        },
        asset_width: 48,
        owned_width: 48,
        core_palette_indices: COMMON_CORE,
        edge_palette_indices: COMMON_EDGE,
    },
    LabelSpec {
        id: "mode_option",
        text: "옵션",
        screen: Screen::TokotonOption,
        bounds: Bounds {
            x: 96,
            y: 148,
            width: 48,
            height: 36,
        },
        asset_width: 48,
        owned_width: 48,
        core_palette_indices: OPTION_CORE,
        edge_palette_indices: OPTION_EDGE,
    },
    LabelSpec {
        id: "solo_easy",
        text: "쉬운 / 뿌요뿌요",
        screen: Screen::SoloType,
        bounds: Bounds {
            // Same contract as solo_normal: a 49px generated canvas with one
            // shared left gutter followed by six private 8px columns. Remix's
            // easy label begins one tile earlier than the inherited Tsuu crop
            // had assumed.
            x: 31,
            y: 144,
            width: 49,
            height: 40,
        },
        asset_width: 49,
        owned_width: 49,
        core_palette_indices: COMMON_CORE,
        edge_palette_indices: COMMON_EDGE,
    },
    LabelSpec {
        id: "solo_normal",
        text: "보통 / 뿌요뿌요",
        screen: Screen::SoloType,
        bounds: Bounds {
            // Remix adds a third difficulty sign and moves normal from the
            // Tsuu two-sign position to the measured center slot.
            x: 111,
            y: 144,
            width: 49,
            height: 40,
        },
        asset_width: 49,
        owned_width: 49,
        core_palette_indices: COMMON_CORE,
        edge_palette_indices: COMMON_EDGE,
    },
    LabelSpec {
        id: "solo_tsuu",
        text: "2 모드",
        screen: Screen::SoloType,
        bounds: Bounds {
            // Remix-only all-character route. It uses the same right-hand 49px
            // canvas contract as the mode-select third sign.
            x: 191,
            y: 144,
            width: 49,
            height: 40,
        },
        asset_width: 49,
        owned_width: 49,
        core_palette_indices: OPTION_CORE,
        edge_palette_indices: OPTION_EDGE,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModeLabelAssetReport {
    pub id: String,
    pub text: String,
    pub input: String,
    pub dimensions: String,
    pub sha256: String,
    pub opaque_pixels: usize,
    pub erased_jp_pixels: usize,
    pub dropped_shared_gutter_pixels: usize,
    pub dropped_dark_outline_pixels: usize,
    pub dropped_generated_shadow_pixels: usize,
    pub dropped_frame_overlap_pixels: usize,
    pub clipped_left_pixels: usize,
    pub clipped_top_pixels: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModeLabelsPocReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub assets_dir: String,
    pub output_path: String,
    pub output_sha256: String,
    pub layout_dumps: BTreeMap<String, String>,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub write_range: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub mapped_screen_pixels: usize,
    pub mapped_chr_pixels: usize,
    pub blocked_shared_chr_pixels: usize,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub protected_tiles_unchanged: usize,
    pub generated_label_x_shift: isize,
    pub generated_label_y_shift: isize,
    pub patched_runtime_dump: Option<String>,
    pub patched_runtime_vram_matches: Option<bool>,
    pub compression_roundtrip_matches: bool,
    pub diff_confined_to_registered_writes: bool,
    pub checksum_hex: String,
    pub assets: Vec<ModeLabelAssetReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SoloTsuuAssetReport {
    pub verdict: String,
    pub text: String,
    pub source_path: String,
    pub source_sha256: String,
    pub source_dimensions: String,
    pub source_content_bounds: String,
    pub runtime_dump: String,
    pub output_path: String,
    pub output_sha256: String,
    pub output_dimensions: String,
    pub opaque_pixels: usize,
}

pub fn prepare_solo_tsuu_asset(
    source_path: &Path,
    runtime_dump: &Path,
    output_path: &Path,
) -> Result<SoloTsuuAssetReport> {
    const OUTPUT_WIDTH: usize = 49;
    const OUTPUT_HEIGHT: usize = 40;
    const CONTENT_WIDTH: usize = 44;
    const CONTENT_HEIGHT: usize = 34;

    let source_bytes = fs::read(source_path)
        .with_context(|| format!("read generated 2-mode label {}", source_path.display()))?;
    let (source_width, source_height, source_pixels) = decode_rgba8_png_any(&source_bytes)
        .with_context(|| format!("decode generated 2-mode label {}", source_path.display()))?;
    let mut min_x = source_width;
    let mut min_y = source_height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    for y in 0..source_height {
        for x in 0..source_width {
            if source_pixels[(y * source_width + x) * 4 + 3] >= 0x80 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x > max_x || min_y > max_y {
        bail!("generated 2-mode label contains no opaque pixels");
    }
    let content_width = max_x - min_x + 1;
    let content_height = max_y - min_y + 1;
    let scale_x = CONTENT_WIDTH as f64 / content_width as f64;
    let scale_y = CONTENT_HEIGHT as f64 / content_height as f64;
    let scale = scale_x.min(scale_y);
    let rendered_width = ((content_width as f64 * scale).round() as usize).max(1);
    let rendered_height = ((content_height as f64 * scale).round() as usize).max(1);
    let destination_x = (OUTPUT_WIDTH - rendered_width) / 2;
    let destination_y = (OUTPUT_HEIGHT - rendered_height) / 2;

    let cgram_path = runtime_dump.join("cram.bin");
    let cgram = fs::read(&cgram_path)
        .with_context(|| format!("read solo-type runtime CGRAM {}", cgram_path.display()))?;
    let palette = decode_cgram(&cgram)?;
    let allowed_indices: Vec<u8> = OPTION_CORE.iter().chain(OPTION_EDGE).copied().collect();
    let mut output = vec![0u8; OUTPUT_WIDTH * OUTPUT_HEIGHT * 4];
    for y in 0..rendered_height {
        let source_y = min_y + y * content_height / rendered_height;
        for x in 0..rendered_width {
            let source_x = min_x + x * content_width / rendered_width;
            let source_offset = (source_y * source_width + source_x) * 4;
            if source_pixels[source_offset + 3] < 0x80 {
                continue;
            }
            let rgb = [
                source_pixels[source_offset],
                source_pixels[source_offset + 1],
                source_pixels[source_offset + 2],
            ];
            let index = allowed_indices
                .iter()
                .copied()
                .min_by_key(|index| {
                    let candidate = palette[2 * 16 + usize::from(*index)];
                    rgb.into_iter()
                        .zip(candidate)
                        .map(|(a, b)| a.abs_diff(b) as u32)
                        .sum::<u32>()
                })
                .expect("2-mode authoring palette is non-empty");
            let destination_offset = ((destination_y + y) * OUTPUT_WIDTH + destination_x + x) * 4;
            output[destination_offset..destination_offset + 3]
                .copy_from_slice(&palette[2 * 16 + usize::from(index)]);
            output[destination_offset + 3] = 0xFF;
        }
    }
    let opaque_pixels = output
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 0xFF)
        .count();
    if opaque_pixels == 0 {
        bail!("normalized 2-mode label contains no opaque pixels");
    }
    let encoded = encode_png_rgba(OUTPUT_WIDTH, OUTPUT_HEIGHT, &output)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create 2-mode asset directory {}", parent.display()))?;
    }
    fs::write(output_path, &encoded)
        .with_context(|| format!("write normalized 2-mode label {}", output_path.display()))?;

    Ok(SoloTsuuAssetReport {
        verdict: "generated 2-mode label normalized to the verified 49x40 solo sign and runtime palette-2 contract"
            .to_owned(),
        text: "2 모드".to_owned(),
        source_path: source_path.display().to_string(),
        source_sha256: sha256(&source_bytes),
        source_dimensions: format!("{source_width}x{source_height}"),
        source_content_bounds: format!("{min_x},{min_y} {}x{}", content_width, content_height),
        runtime_dump: runtime_dump.display().to_string(),
        output_path: output_path.display().to_string(),
        output_sha256: sha256(&encoded),
        output_dimensions: format!("{OUTPUT_WIDTH}x{OUTPUT_HEIGHT}"),
        opaque_pixels,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_poc(
    source: &[u8],
    source_path: String,
    assets_dir: &Path,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, ModeLabelsPocReport)> {
    build_poc_impl(
        source,
        source_path,
        assets_dir,
        mode_select_dump,
        solo_type_dump,
        tokoton_option_dump,
        patched_runtime_dump,
        output_path,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    assets_dir: &Path,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, ModeLabelsPocReport)> {
    build_poc_impl(
        source,
        source_path,
        assets_dir,
        mode_select_dump,
        solo_type_dump,
        tokoton_option_dump,
        patched_runtime_dump,
        output_path,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_poc_impl(
    source: &[u8],
    source_path: String,
    assets_dir: &Path,
    mode_select_dump: &Path,
    solo_type_dump: &Path,
    tokoton_option_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, ModeLabelsPocReport)> {
    let source_sha256 = sha256(source);
    if require_original_identity && source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    let block = crate::snes_lz::decompress(source, STREAM_PC)?;
    if block.compressed_len != EXPECTED_COMPRESSED_LEN
        || block.bytes.len() != EXPECTED_DECOMPRESSED_LEN
    {
        bail!(
            "Remix mode-label stream differs from the verified contract: compressed {} / decoded {}",
            block.compressed_len,
            block.bytes.len()
        );
    }
    let raw = source
        .get(STREAM_PC..STREAM_PC + block.compressed_len)
        .context("verified Remix mode-label stream is outside ROM")?;
    if sha256(raw) != EXPECTED_RAW_SHA256 || sha256(&block.bytes) != EXPECTED_DECODED_SHA256 {
        bail!("Remix mode-label stream hashes differ from the verified runtime-owned resource");
    }

    let mode = read_runtime_dump(mode_select_dump, true, &block.bytes, "mode_select")?;
    let solo = read_runtime_dump(solo_type_dump, false, &block.bytes, "solo_type")?;
    let tokoton = read_runtime_dump(tokoton_option_dump, true, &block.bytes, "tokoton_option")?;
    let mut layout_dumps = BTreeMap::new();
    layout_dumps.insert(
        "mode_select".to_owned(),
        mode_select_dump.display().to_string(),
    );
    layout_dumps.insert("solo_type".to_owned(), solo_type_dump.display().to_string());
    layout_dumps.insert(
        "tokoton_option".to_owned(),
        tokoton_option_dump.display().to_string(),
    );

    let mode_indices = render_index_layer(&mode.vram, MODE_LAYER)?;
    let solo_indices = render_index_layer(&solo.vram, MODE_LAYER)?;
    let tokoton_indices = render_index_layer(&tokoton.vram, TOKOTON_RIGHT_LAYER)?;
    let mode_palette = decode_cgram(&mode.cgram)?;
    let solo_palette = decode_cgram(&solo.cgram)?;
    let tokoton_palette = decode_cgram(&tokoton.cgram)?;

    // A single source CHR pixel can have several tilemap consumers. Register the
    // complete desired canvas so conflicting generated labels fail closed.
    let mut desired: BTreeMap<(usize, usize, usize), (u8, String, bool)> = BTreeMap::new();
    let mut asset_reports = Vec::new();
    let mut mapped_screen_pixels = 0usize;
    for spec in LABELS {
        let (dump, indices, palette, layer) = match spec.screen {
            Screen::ModeSelect => (&mode, &mode_indices, &mode_palette, MODE_LAYER),
            Screen::SoloType => (&solo, &solo_indices, &solo_palette, MODE_LAYER),
            Screen::TokotonOption => (
                &tokoton,
                &tokoton_indices,
                &tokoton_palette,
                TOKOTON_RIGHT_LAYER,
            ),
        };
        let input_path = assets_dir.join(format!("{}.png", spec.id));
        let input_bytes = fs::read(&input_path)
            .with_context(|| format!("read inherited Korean label {}", input_path.display()))?;
        let image = decode_rgba8_png(&input_bytes, spec.asset_width, spec.bounds.height)
            .with_context(|| format!("decode inherited Korean label {}", input_path.display()))?;
        let opaque_pixels = image
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] == 0xFF)
            .count();
        for (pixel_number, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
            if !matches!(pixel[3], 0 | 0xFF) {
                bail!(
                    "{} has non-binary alpha {} at {},{}",
                    input_path.display(),
                    pixel[3],
                    pixel_number % spec.asset_width,
                    pixel_number / spec.bounds.width
                );
            }
        }

        let shift_left = GENERATED_LABEL_X_SHIFT.unsigned_abs();
        let shift_up = GENERATED_LABEL_Y_SHIFT.unsigned_abs();
        let clipped_left_pixels = image
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(pixel_number, pixel)| {
                pixel_number % spec.asset_width < shift_left && pixel[3] == 0xFF
            })
            .count();
        let clipped_top_pixels = image
            .as_chunks::<4>()
            .0
            .iter()
            .take(shift_up * spec.asset_width)
            .filter(|pixel| pixel[3] == 0xFF)
            .count();
        let mut erased_jp_pixels = 0usize;
        let mut dropped_shared_gutter_pixels = 0usize;
        let mut dropped_dark_outline_pixels = 0usize;
        let mut dropped_generated_shadow_pixels = 0usize;
        let mut dropped_frame_overlap_pixels = 0usize;
        let mask = crate::mouth_sign::analyze_around(
            indices,
            SCREEN_WIDTH,
            crate::mouth_sign::Region {
                x: spec.bounds.x,
                y: spec.bounds.y,
                width: spec.bounds.width,
                height: spec.bounds.height,
            },
            SIGN_MARGIN,
            sign_colors(spec),
            SIGN_GLOW_RADIUS,
        )
        .with_context(|| format!("classify the Japanese lettering of {}", spec.id))?;

        for local_y in 0..spec.bounds.height {
            for local_x in 0..spec.bounds.width {
                if local_x >= spec.owned_width {
                    continue;
                }
                let x = spec.bounds.x + local_x;
                let y = spec.bounds.y + local_y;
                let original_index = indices[y * SCREEN_WIDTH + x] & 0x0F;
                let class = mask.class_at(x, y);
                let is_frame = class == crate::mouth_sign::PixelClass::Frame;
                let is_jp_text = matches!(
                    class,
                    crate::mouth_sign::PixelClass::Text | crate::mouth_sign::PixelClass::Halo
                );
                let erased_index = mask.restored_at(x, y);
                let candidate_x = if spec.asset_width == spec.bounds.width {
                    local_x + shift_left
                } else {
                    local_x
                };
                let candidate_y = local_y + shift_up;
                let candidate_offset = (candidate_x < spec.asset_width
                    && candidate_y < spec.bounds.height)
                    .then_some((candidate_y * spec.asset_width + candidate_x) * 4);
                let alpha = candidate_offset.map_or(0, |offset| image[offset + 3]);
                // The 49px assets expose one column from CHR tiles shared by three
                // labels. Clear that gutter to verified sign fill and own 48px.
                let shared_gutter = spec.bounds.width == 49 && local_x == 0;
                let desired_index = if shared_gutter {
                    if alpha == 0xFF {
                        dropped_shared_gutter_pixels += 1;
                    }
                    if is_jp_text {
                        erased_jp_pixels += 1;
                    }
                    if is_frame { original_index } else { 3 }
                } else if is_frame {
                    // Outline, lips and fangs are never repainted.
                    if alpha == 0xFF {
                        dropped_frame_overlap_pixels += 1;
                    }
                    original_index
                } else if alpha == 0xFF {
                    let offset = candidate_offset.expect("opaque candidate has an offset");
                    let rgb = [image[offset], image[offset + 1], image[offset + 2]];
                    let mapped =
                        map_rgb_to_palette_index(rgb, palette, spec).with_context(|| {
                            format!(
                                "map {} pixel {},{} color #{:02X}{:02X}{:02X}",
                                input_path.display(),
                                candidate_x,
                                candidate_y,
                                rgb[0],
                                rgb[1],
                                rgb[2]
                            )
                        })?;
                    // The regenerated labels draw their 1px outline and drop
                    // shadow in dark maroon (2), the shade of the original
                    // Japanese letter shadow. The near-black outline (1) and the
                    // orange shadow (5) of the inherited sheets stay dropped.
                    if mapped == 1 {
                        dropped_dark_outline_pixels += 1;
                    }
                    if mapped == 5 {
                        dropped_generated_shadow_pixels += 1;
                    }
                    if matches!(mapped, 1 | 5) {
                        if is_jp_text {
                            erased_jp_pixels += 1;
                        }
                        erased_index
                    } else {
                        mapped
                    }
                } else if is_jp_text {
                    erased_jp_pixels += 1;
                    erased_index
                } else {
                    // Fill keeps its original shade unless the restoration
                    // moved the mouth's shade boundary through it.
                    erased_index
                };

                let entry = tilemap_entry_at(&dump.vram, layer, x / 8, y / 8)?;
                if entry.palette != 2 {
                    bail!(
                        "{} pixel {},{} uses palette {}, expected 2",
                        spec.id,
                        x,
                        y,
                        entry.palette
                    );
                }
                if entry.tile * TILE_LEN >= EXPECTED_DECOMPRESSED_LEN {
                    bail!(
                        "{} uses tile 0x{:03X} outside the verified CHR stream",
                        spec.id,
                        entry.tile
                    );
                }
                let source_x = if entry.hflip { 7 - x % 8 } else { x % 8 };
                let source_y = if entry.vflip { 7 - y % 8 } else { y % 8 };
                let key = (entry.tile, source_x, source_y);
                if let Some((previous, previous_asset, previous_gutter)) = desired.get(&key) {
                    if *previous != desired_index {
                        let tile_start = entry.tile * TILE_LEN;
                        let original_source_index = decode_4bpp_pixel(
                            &block.bytes[tile_start..tile_start + TILE_LEN],
                            source_x,
                            source_y,
                        );
                        // A Remix tilemap adds one more cross-label reference than
                        // the Tsuu workset. A synthetic 49px gutter must never paint
                        // over a real consumer; retain that consumer's original pixel.
                        if *previous_gutter && desired_index == original_source_index {
                            desired.insert(key, (desired_index, spec.id.to_owned(), shared_gutter));
                        } else if !(shared_gutter && *previous == original_source_index) {
                            bail!(
                                "shared CHR pixel conflict at tile 0x{:03X} {},{}: {} needs {}, {} needs {}",
                                entry.tile,
                                source_x,
                                source_y,
                                previous_asset,
                                previous,
                                spec.id,
                                desired_index
                            );
                        }
                    }
                } else {
                    desired.insert(key, (desired_index, spec.id.to_owned(), shared_gutter));
                }
                mapped_screen_pixels += 1;
            }
        }
        asset_reports.push(ModeLabelAssetReport {
            id: spec.id.to_owned(),
            text: spec.text.to_owned(),
            input: input_path.display().to_string(),
            dimensions: format!("{}x{}", spec.asset_width, spec.bounds.height),
            sha256: sha256(&input_bytes),
            opaque_pixels,
            erased_jp_pixels,
            dropped_shared_gutter_pixels,
            dropped_dark_outline_pixels,
            dropped_generated_shadow_pixels,
            dropped_frame_overlap_pixels,
            clipped_left_pixels,
            clipped_top_pixels,
        });
    }

    let mapped_chr_pixels = desired.len();
    // The same source tile can also draw a rabbit edge, mouth border, or other
    // background pixel outside every declared label canvas.  A wider JP-text
    // cleanup mask is safe only when no such unowned consumer exists.
    let mut unowned_consumers = BTreeSet::new();
    for (screen, dump, layer) in [
        (Screen::ModeSelect, &mode, MODE_LAYER),
        (Screen::SoloType, &solo, MODE_LAYER),
        (Screen::TokotonOption, &tokoton, TOKOTON_RIGHT_LAYER),
    ] {
        collect_unowned_consumer_keys(&mut unowned_consumers, screen, dump, layer)?;
    }
    let mut blocked_shared_chr_pixels = 0usize;
    for (&(tile, x, y), (value, _, _)) in &mut desired {
        let tile_start = tile * TILE_LEN;
        let original = decode_4bpp_pixel(&block.bytes[tile_start..tile_start + TILE_LEN], x, y);
        if *value != original && unowned_consumers.contains(&(tile, x, y)) {
            *value = original;
            blocked_shared_chr_pixels += 1;
        }
    }
    let mut patched_decoded = block.bytes.clone();
    for ((tile, x, y), (value, _, _)) in &desired {
        let tile_start = tile * TILE_LEN;
        set_4bpp_pixel(
            &mut patched_decoded[tile_start..tile_start + TILE_LEN],
            *x,
            *y,
            *value,
        );
    }
    let changed_tiles = block
        .bytes
        .as_chunks::<TILE_LEN>()
        .0
        .iter()
        .zip(patched_decoded.as_chunks::<TILE_LEN>().0)
        .filter(|(before, after)| before != after)
        .count();
    let changed_decompressed_bytes = block
        .bytes
        .iter()
        .zip(&patched_decoded)
        .filter(|(before, after)| before != after)
        .count();
    let protected_tiles_unchanged = block.bytes.len() / TILE_LEN
        - block
            .bytes
            .as_chunks::<TILE_LEN>()
            .0
            .iter()
            .zip(patched_decoded.as_chunks::<TILE_LEN>().0)
            .enumerate()
            .filter(|(tile, (before, after))| {
                before == after || desired.keys().any(|(mapped, _, _)| mapped == tile)
            })
            .count();
    if protected_tiles_unchanged != 0 {
        bail!("{protected_tiles_unchanged} protected mode-label CHR tiles changed");
    }
    let untouched_tile_count = (0..block.bytes.len() / TILE_LEN)
        .filter(|tile| !desired.keys().any(|(mapped, _, _)| mapped == tile))
        .count();

    let compressed = crate::snes_lz::compress(&patched_decoded);
    if compressed.len() > block.compressed_len {
        bail!(
            "Korean mode-label stream grew from {} to {} bytes; refusing in-place overflow",
            block.compressed_len,
            compressed.len()
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    let compression_roundtrip_matches =
        roundtrip.bytes == patched_decoded && roundtrip.compressed_len == compressed.len();
    if !compression_roundtrip_matches {
        bail!("recompressed Remix mode-label CHR failed its round trip");
    }

    let patched_runtime_vram_matches = patched_runtime_dump
        .map(|dump| {
            let path = dump.join("vram.bin");
            let vram = fs::read(&path).with_context(|| {
                format!("read patched mode-label runtime VRAM {}", path.display())
            })?;
            if vram.len() != VRAM_LEN {
                bail!("patched mode-label runtime VRAM must be {VRAM_LEN} bytes");
            }
            let matches = vram.get(..patched_decoded.len()) == Some(patched_decoded.as_slice());
            if !matches {
                bail!("patched runtime VRAM does not contain built mode-label CHR at 0x0000");
            }
            Ok(matches)
        })
        .transpose()?;

    let write_start = STREAM_PC;
    let write_end = STREAM_PC + block.compressed_len;
    let mut patched = source.to_vec();
    patched[write_start..write_end].fill(0);
    patched[write_start..write_start + compressed.len()].copy_from_slice(&compressed);
    let checksum = crate::rom::fix_checksum(&mut patched)?;
    let diff_confined_to_registered_writes =
        source
            .iter()
            .zip(&patched)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || (write_start..write_end).contains(&offset)
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&offset)
            });
    if !diff_confined_to_registered_writes {
        bail!("mode-label PoC diff escaped its registered stream and checksum writes");
    }

    Ok((
        patched.clone(),
        ModeLabelsPocReport {
            verdict: "seven inherited plus one Remix-only solo Korean label remapped onto runtime tilemaps; non-consumer CHR tiles preserved"
                .to_owned(),
            source_path,
            source_sha256,
            assets_dir: assets_dir.display().to_string(),
            output_path,
            output_sha256: sha256(&patched),
            layout_dumps,
            stream_pc: format!("0x{STREAM_PC:06X}"),
            stream_lorom: "$1F:$C5FE".to_owned(),
            write_range: format!("0x{write_start:06X}..0x{write_end:06X}"),
            original_compressed_len: block.compressed_len,
            patched_compressed_len: compressed.len(),
            decompressed_len: patched_decoded.len(),
            mapped_screen_pixels,
            mapped_chr_pixels,
            blocked_shared_chr_pixels,
            changed_tiles,
            changed_decompressed_bytes,
            protected_tiles_unchanged: untouched_tile_count,
            generated_label_x_shift: GENERATED_LABEL_X_SHIFT,
            generated_label_y_shift: GENERATED_LABEL_Y_SHIFT,
            patched_runtime_dump: patched_runtime_dump.map(|path| path.display().to_string()),
            patched_runtime_vram_matches,
            compression_roundtrip_matches,
            diff_confined_to_registered_writes,
            checksum_hex: format!("0x{checksum:04X}"),
            assets: asset_reports,
        },
    ))
}

struct RuntimeDump {
    vram: Vec<u8>,
    cgram: Vec<u8>,
}

fn read_runtime_dump(
    path: &Path,
    expected_double_width: bool,
    decoded: &[u8],
    name: &str,
) -> Result<RuntimeDump> {
    let vram_path = path.join("vram.bin");
    let cgram_path = path.join("cram.bin");
    let state_path = path.join("state.json");
    let vram = fs::read(&vram_path)
        .with_context(|| format!("read {name} runtime VRAM {}", vram_path.display()))?;
    let cgram = fs::read(&cgram_path)
        .with_context(|| format!("read {name} runtime CGRAM {}", cgram_path.display()))?;
    if vram.len() != VRAM_LEN || cgram.len() != CGRAM_LEN {
        bail!(
            "{name} runtime dump sizes differ from SNES VRAM/CGRAM: {} / {}",
            vram.len(),
            cgram.len()
        );
    }
    if vram.get(..decoded.len()) != Some(decoded) {
        bail!("{name} runtime VRAM does not contain the verified Remix stream at 0x0000");
    }
    let state: serde_json::Value = serde_json::from_slice(
        &fs::read(&state_path)
            .with_context(|| format!("read {name} runtime state {}", state_path.display()))?,
    )
    .with_context(|| format!("parse {name} runtime state {}", state_path.display()))?;
    let bg_mode = state_u64(&state, "ppu.bgMode")?;
    let chr_words = state_u64(&state, "ppu.layers[0].chrAddress")?;
    let map_words = state_u64(&state, "ppu.layers[0].tilemapAddress")?;
    let double_width = state_bool(&state, "ppu.layers[0].doubleWidth")?;
    let double_height = state_bool(&state, "ppu.layers[0].doubleHeight")?;
    if bg_mode != 1
        || chr_words != 0
        || map_words.checked_mul(2) != Some(0xD000)
        || double_width != expected_double_width
        || double_height
    {
        bail!(
            "{name} BG1 geometry differs from verified Mode 1 / CHR 0x0000 / map 0xD000 / width {}",
            if expected_double_width { 64 } else { 32 }
        );
    }
    Ok(RuntimeDump { vram, cgram })
}

fn state_u64(state: &serde_json::Value, key: &str) -> Result<u64> {
    state
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .with_context(|| format!("runtime state is missing numeric {key}"))
}

fn state_bool(state: &serde_json::Value, key: &str) -> Result<bool> {
    state
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .with_context(|| format!("runtime state is missing boolean {key}"))
}

/// Decodes a generated sign label and tidies its generation leftovers.
fn decode_rgba8_png(encoded: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let (actual_width, actual_height, mut pixels) = decode_rgba8_png_any(encoded)?;
    if actual_width != width || actual_height != height {
        bail!(
            "PNG dimensions are {}x{}, expected {}x{}",
            actual_width,
            actual_height,
            width,
            height
        );
    }
    crate::mouth_sign::tidy_generated_lettering(&mut pixels, width, height);
    Ok(pixels)
}

fn decode_rgba8_png_any(encoded: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
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
    Ok((info.width as usize, info.height as usize, pixels))
}

fn encode_png_rgba(width: usize, height: usize, pixels: &[u8]) -> Result<Vec<u8>> {
    if pixels.len() != width * height * 4 {
        bail!("RGBA output size does not match {width}x{height}");
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write PNG header")?;
        writer.write_image_data(pixels).context("write PNG data")?;
    }
    Ok(output)
}

fn render_index_layer(vram: &[u8], layer: Layer) -> Result<Vec<u8>> {
    let mut output = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
    for tile_y in 0..SCREEN_HEIGHT / 8 {
        for tile_x in 0..SCREEN_WIDTH / 8 {
            let entry = tilemap_entry_at(vram, layer, tile_x, tile_y)?;
            let tile = vram
                .get(entry.tile * TILE_LEN..(entry.tile + 1) * TILE_LEN)
                .with_context(|| format!("BG1 tile 0x{:03X} is outside VRAM", entry.tile))?;
            for y in 0..8 {
                for x in 0..8 {
                    let source_x = if entry.hflip { 7 - x } else { x };
                    let source_y = if entry.vflip { 7 - y } else { y };
                    output[(tile_y * 8 + y) * SCREEN_WIDTH + tile_x * 8 + x] =
                        entry.palette * 16 + decode_4bpp_pixel(tile, source_x, source_y);
                }
            }
        }
    }
    Ok(output)
}

fn collect_unowned_consumer_keys(
    output: &mut BTreeSet<(usize, usize, usize)>,
    screen: Screen,
    dump: &RuntimeDump,
    layer: Layer,
) -> Result<()> {
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            if LABELS.iter().any(|spec| {
                std::mem::discriminant(&spec.screen) == std::mem::discriminant(&screen)
                    && x >= spec.bounds.x
                    && x < spec.bounds.x + spec.owned_width
                    && y >= spec.bounds.y
                    && y < spec.bounds.y + spec.bounds.height
            }) {
                continue;
            }
            let entry = tilemap_entry_at(&dump.vram, layer, x / 8, y / 8)?;
            if entry.tile * TILE_LEN >= EXPECTED_DECOMPRESSED_LEN {
                continue;
            }
            let source_x = if entry.hflip { 7 - x % 8 } else { x % 8 };
            let source_y = if entry.vflip { 7 - y % 8 } else { y % 8 };
            output.insert((entry.tile, source_x, source_y));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct TilemapEntry {
    tile: usize,
    palette: u8,
    hflip: bool,
    vflip: bool,
}

fn tilemap_entry_at(vram: &[u8], layer: Layer, x: usize, y: usize) -> Result<TilemapEntry> {
    if !matches!(layer.tilemap_width, 32 | 64) || x >= layer.tilemap_width || y >= TILEMAP_HEIGHT {
        bail!(
            "BG1 tilemap coordinate {x},{y} is outside {}x{TILEMAP_HEIGHT}",
            layer.tilemap_width
        );
    }
    let screen_offset = (x / 32) * 0x800;
    let within_screen = (y * 32 + x % 32) * 2;
    let offset = layer.tilemap_base_byte + screen_offset + within_screen;
    let bytes = vram
        .get(offset..offset + 2)
        .with_context(|| format!("BG1 tilemap entry {x},{y} is outside VRAM"))?;
    let raw = u16::from_le_bytes([bytes[0], bytes[1]]);
    Ok(TilemapEntry {
        tile: usize::from(raw & 0x03FF),
        palette: ((raw >> 10) & 0x07) as u8,
        hflip: raw & 0x4000 != 0,
        vflip: raw & 0x8000 != 0,
    })
}

fn map_rgb_to_palette_index(rgb: [u8; 3], palette: &[[u8; 3]], spec: LabelSpec) -> Result<u8> {
    let matches: Vec<u8> = (1u8..16)
        .filter(|index| palette[2 * 16 + usize::from(*index)] == rgb)
        .collect();
    if matches.is_empty() {
        bail!("color is not present in BG palette 2");
    }
    Ok(matches
        .iter()
        .copied()
        .find(|index| spec.core_palette_indices.contains(index))
        .or_else(|| {
            matches
                .iter()
                .copied()
                .find(|index| spec.edge_palette_indices.contains(index))
        })
        .unwrap_or(matches[0]))
}

fn decode_cgram(cgram: &[u8]) -> Result<Vec<[u8; 3]>> {
    if cgram.len() != CGRAM_LEN {
        bail!("CGRAM must be {CGRAM_LEN} bytes");
    }
    Ok(cgram
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| {
            let color = u16::from_le_bytes([bytes[0], bytes[1]]);
            [
                expand5((color & 0x1F) as u8),
                expand5(((color >> 5) & 0x1F) as u8),
                expand5(((color >> 10) & 0x1F) as u8),
            ]
        })
        .collect())
}

fn expand5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

fn decode_4bpp_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn set_4bpp_pixel(tile: &mut [u8], x: usize, y: usize, value: u8) {
    debug_assert_eq!(tile.len(), TILE_LEN);
    debug_assert!(x < 8 && y < 8 && value < 16);
    let bit = 7 - x;
    let mask = !(1 << bit);
    for plane in 0..4 {
        let byte = if plane < 2 {
            y * 2 + plane
        } else {
            16 + y * 2 + plane - 2
        };
        tile[byte] = (tile[byte] & mask) | (((value >> plane) & 1) << bit);
    }
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_bounds_fit_visible_screens() {
        for spec in LABELS {
            assert!(spec.bounds.x + spec.bounds.width <= SCREEN_WIDTH);
            assert!(spec.bounds.y + spec.bounds.height <= SCREEN_HEIGHT);
            assert!(spec.owned_width <= spec.bounds.width);
            assert!(spec.asset_width <= spec.bounds.width);
        }
    }

    #[test]
    fn solo_easy_uses_the_same_canvas_contract_as_solo_normal() {
        let easy = LABELS
            .iter()
            .find(|spec| spec.id == "solo_easy")
            .expect("solo_easy spec");
        let normal = LABELS
            .iter()
            .find(|spec| spec.id == "solo_normal")
            .expect("solo_normal spec");

        assert_eq!(easy.bounds.width, normal.bounds.width);
        assert_eq!(easy.bounds.height, normal.bounds.height);
        assert_eq!(easy.asset_width, normal.asset_width);
        assert_eq!(easy.owned_width, normal.owned_width);
    }

    #[test]
    fn solo_tsuu_uses_the_same_canvas_contract_as_the_other_49px_solo_labels() {
        let tsuu = LABELS
            .iter()
            .find(|spec| spec.id == "solo_tsuu")
            .expect("solo_tsuu spec");
        for id in ["solo_easy", "solo_normal"] {
            let peer = LABELS
                .iter()
                .find(|spec| spec.id == id)
                .expect("solo peer spec");
            assert_eq!(tsuu.bounds.width, peer.bounds.width);
            assert_eq!(tsuu.bounds.height, peer.bounds.height);
            assert_eq!(tsuu.asset_width, peer.asset_width);
            assert_eq!(tsuu.owned_width, peer.owned_width);
        }
    }

    #[test]
    fn four_bpp_pixel_roundtrip() {
        let mut tile = [0u8; TILE_LEN];
        for y in 0..8 {
            for x in 0..8 {
                set_4bpp_pixel(&mut tile, x, y, ((y * 8 + x) & 0x0F) as u8);
            }
        }
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(decode_4bpp_pixel(&tile, x, y), ((y * 8 + x) & 0x0F) as u8);
            }
        }
    }
}
