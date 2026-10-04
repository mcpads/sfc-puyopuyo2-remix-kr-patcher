use std::{collections::BTreeSet, fs, io::Cursor, path::Path};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const CHECKSUM_PC: usize = 0x7FDC;
const VRAM_LEN: usize = 65_536;
const CGRAM_LEN: usize = 512;
const VRAM_BYTE: usize = 0x5600;
const VRAM_BASE_TILE: usize = VRAM_BYTE / 32;
const TILE_LEN: usize = 32;
const TILEMAP_BYTE: usize = 0xD000;
const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 224;
const ASSET_WIDTH: usize = 48;
const ASSET_HEIGHT: usize = 40;
const GENERATED_X_SHIFT: isize = -1;
const GENERATED_Y_SHIFT: isize = -2;
const TEXT_CORE: &[u8] = &[15];
const TEXT_EDGE: &[u8] = &[1, 2, 5, 6, 7, 10, 11, 12, 14];
/// Palette-2 roles of the mouth signs. The fill is 3 above the lettering and 4
/// below it and as the glow around each Japanese letter; the letters are white
/// (15) with pink (10/11) edges; the outline (1) carries the white fangs.
const MOUTH_COLORS: crate::mouth_sign::SignColors = crate::mouth_sign::SignColors {
    fill: &[3, 4],
    lettering: &[10, 11, 15],
    outline: 1,
    palette: 2,
    // Each fang is about ten white pixels; a letter stroke is far larger.
    max_fang_pixels: 24,
};
/// Chebyshev radius of the glow ring baked around the Japanese lettering. The
/// widest measured glow run is four pixels; restoration votes on the row, so
/// true fill inside the ring comes back unchanged.
const GLOW_RADIUS: usize = 4;
/// The 48x40 canvas crops the mouth: its outline starts about four pixels to
/// the left and two rows above it, and the lettering reaches the canvas edge.
/// Classification therefore looks at the canvas grown by (sides, top, bottom).
const MOUTH_MARGIN: (usize, usize, usize) = (8, 4, 2);

/// One Remix CHR stream whose 48x40 mouth signs are loaded at VRAM `0x5600`
/// and drawn by BG1 Mode 1 palette 2 through tilemap `0xD000`.
#[derive(Debug, Clone, Copy)]
pub struct SignStream {
    pub name: &'static str,
    pub stream_pc: usize,
    pub stream_lorom: &'static str,
    pub compressed_len: usize,
    pub decompressed_len: usize,
    pub raw_sha256: &'static str,
    pub decoded_sha256: &'static str,
    pub slots: &'static [SignSlot],
    /// Decoded tiles from this index on have no verified consumer on the sign
    /// screen and must stay byte-identical.
    pub protected_first_tile: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct SignSlot {
    pub x: usize,
    pub y: usize,
    pub source_text: &'static str,
}

pub const TOKOTON_CHOICES: SignStream = SignStream {
    name: "tokoton_choices",
    stream_pc: 0x0C_1C16,
    stream_lorom: "$18:$9C16",
    compressed_len: 3_609,
    decompressed_len: 4_608,
    raw_sha256: "bb5515b48277db06795ce1899c7af874ea58740b323e44a60affdcb47187829c",
    decoded_sha256: "b887fa445a69c4ae0eb8a8f3bfb3c13fde1e07c97541dbb018e6c593d1c5edd9",
    slots: &[
        SignSlot {
            x: 40,
            y: 144,
            source_text: "あそぶ",
        },
        SignSlot {
            x: 168,
            y: 144,
            source_text: "おてほん",
        },
    ],
    protected_first_tile: 60,
};

/// Easy-mode course signs. The course select screen draws only the first 90
/// tiles; the remaining 126 tiles have no verified consumer.
pub const EASY_COURSES: SignStream = SignStream {
    name: "easy_courses",
    stream_pc: 0x0C_0669,
    stream_lorom: "$18:$8669",
    compressed_len: 5_549,
    decompressed_len: 6_912,
    raw_sha256: "07667dd47a4e1eb085a29ca7d17096d87753d054ff4e88893b19d97148ecde76",
    decoded_sha256: "84acbc641fe8abd92cdef764a65e7c2eb9370c694d9051ac967b1623149bfcac",
    slots: &[
        SignSlot {
            x: 32,
            y: 144,
            source_text: "はじめて",
        },
        SignSlot {
            x: 112,
            y: 144,
            source_text: "なれた",
        },
        SignSlot {
            x: 192,
            y: 144,
            source_text: "そつぎょう",
        },
    ],
    protected_first_tile: 90,
};

pub fn sign_stream(name: &str) -> Result<SignStream> {
    [TOKOTON_CHOICES, EASY_COURSES]
        .into_iter()
        .find(|stream| stream.name == name)
        .with_context(|| {
            format!("unknown sign stream {name}; expected tokoton_choices or easy_courses")
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetReport {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildReport {
    pub verdict: String,
    pub source_path: String,
    pub source_sha256: String,
    pub play_asset_path: String,
    pub play_asset_sha256: String,
    pub demonstration_asset_path: String,
    pub demonstration_asset_sha256: String,
    pub runtime_dump: String,
    pub output_path: String,
    pub output_sha256: String,
    pub texts: Vec<String>,
    pub surfaces: Vec<String>,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub runtime_vram_range: String,
    pub runtime_vram_matches_source: bool,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub erased_jp_pixels: usize,
    pub preserved_frame_pixels: usize,
    pub dropped_frame_overlap_pixels: usize,
    pub blocked_unowned_pixels: usize,
    pub compression_roundtrip_matches: bool,
    pub patched_runtime_dump: Option<String>,
    pub patched_runtime_vram_matches: Option<bool>,
    pub diff_confined_to_registered_writes: bool,
    pub checksum_hex: String,
}

pub fn prepare_asset(
    source_path: &Path,
    text: &str,
    runtime_dump: &Path,
    output_path: &Path,
) -> Result<AssetReport> {
    const CONTENT_WIDTH: usize = 42;
    const CONTENT_HEIGHT: usize = 30;

    let source_bytes = fs::read(source_path).with_context(|| {
        format!(
            "read generated Tokoton play label {}",
            source_path.display()
        )
    })?;
    let (source_width, source_height, source_pixels) = decode_rgba8_png_any(&source_bytes)?;
    let (min_x, min_y, max_x, max_y) = opaque_bounds(&source_pixels, source_width, source_height)?;
    let content_width = max_x - min_x + 1;
    let content_height = max_y - min_y + 1;
    let scale = (CONTENT_WIDTH as f64 / content_width as f64)
        .min(CONTENT_HEIGHT as f64 / content_height as f64);
    let rendered_width = ((content_width as f64 * scale).round() as usize).max(1);
    let rendered_height = ((content_height as f64 * scale).round() as usize).max(1);
    let destination_x = (ASSET_WIDTH - rendered_width) / 2;
    let destination_y = (ASSET_HEIGHT - rendered_height) / 2;

    let cgram_path = runtime_dump.join("cram.bin");
    let palette =
        decode_cgram(&fs::read(&cgram_path).with_context(|| {
            format!("read Tokoton play runtime CGRAM {}", cgram_path.display())
        })?)?;
    let allowed = TEXT_CORE
        .iter()
        .chain(TEXT_EDGE)
        .copied()
        .collect::<Vec<_>>();
    let mut output = vec![0u8; ASSET_WIDTH * ASSET_HEIGHT * 4];
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
            let index = allowed
                .iter()
                .copied()
                .min_by_key(|index| color_distance(rgb, palette[2 * 16 + usize::from(*index)]))
                .expect("Tokoton play authoring palette is non-empty");
            let destination = ((destination_y + y) * ASSET_WIDTH + destination_x + x) * 4;
            output[destination..destination + 3]
                .copy_from_slice(&palette[2 * 16 + usize::from(index)]);
            output[destination + 3] = 0xFF;
        }
    }
    let opaque_pixels = output
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 0xFF)
        .count();
    if opaque_pixels == 0 {
        bail!("normalized Tokoton play label contains no opaque pixels");
    }
    let encoded = encode_png_rgba(ASSET_WIDTH, ASSET_HEIGHT, &output)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, &encoded)?;
    Ok(AssetReport {
        verdict: format!(
            "generated {text} label normalized to the verified 48x40 Tokoton mouth and runtime palette-2 contract"
        ),
        text: text.to_owned(),
        source_path: source_path.display().to_string(),
        source_sha256: sha256(&source_bytes),
        source_dimensions: format!("{source_width}x{source_height}"),
        source_content_bounds: format!("{min_x},{min_y} {}x{}", content_width, content_height),
        runtime_dump: runtime_dump.display().to_string(),
        output_path: output_path.display().to_string(),
        output_sha256: sha256(&encoded),
        output_dimensions: format!("{ASSET_WIDTH}x{ASSET_HEIGHT}"),
        opaque_pixels,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_poc(
    source: &[u8],
    source_path: String,
    play_asset_path: &Path,
    demonstration_asset_path: &Path,
    runtime_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, BuildReport)> {
    build_impl(
        source,
        source_path,
        play_asset_path,
        demonstration_asset_path,
        runtime_dump,
        patched_runtime_dump,
        output_path,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_after_verified_patch(
    source: &[u8],
    source_path: String,
    play_asset_path: &Path,
    demonstration_asset_path: &Path,
    runtime_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
) -> Result<(Vec<u8>, BuildReport)> {
    build_impl(
        source,
        source_path,
        play_asset_path,
        demonstration_asset_path,
        runtime_dump,
        patched_runtime_dump,
        output_path,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_impl(
    source: &[u8],
    source_path: String,
    play_asset_path: &Path,
    demonstration_asset_path: &Path,
    runtime_dump: &Path,
    patched_runtime_dump: Option<&Path>,
    output_path: String,
    require_original_identity: bool,
) -> Result<(Vec<u8>, BuildReport)> {
    let stream = TOKOTON_CHOICES;
    let source_sha256 = sha256(source);
    if require_original_identity && source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    let block = verify_stream(&stream, source)?;
    let runtime = read_runtime_dump(runtime_dump, &block)?;
    let play_asset_bytes = fs::read(play_asset_path)
        .with_context(|| format!("read Tokoton play asset {}", play_asset_path.display()))?;
    let play_asset = decode_rgba8_png(&play_asset_bytes, ASSET_WIDTH, ASSET_HEIGHT)?;
    let demonstration_asset_bytes = fs::read(demonstration_asset_path).with_context(|| {
        format!(
            "read Tokoton demonstration asset {}",
            demonstration_asset_path.display()
        )
    })?;
    let demonstration_asset =
        decode_rgba8_png(&demonstration_asset_bytes, ASSET_WIDTH, ASSET_HEIGHT)?;

    let plan = plan_signs(
        &stream,
        &block,
        &runtime,
        &[
            Some(play_asset.as_slice()),
            Some(demonstration_asset.as_slice()),
        ],
    )?;
    let compressed = compress_in_place(&stream, &plan.decoded)?;

    let patched_runtime_vram_matches = patched_runtime_dump
        .map(|dump| {
            let vram = fs::read(dump.join("vram.bin"))?;
            let matches = vram.get(VRAM_BYTE..VRAM_BYTE + stream.decompressed_len)
                == Some(plan.decoded.as_slice());
            if !matches {
                bail!("patched runtime VRAM does not contain the built Tokoton play stream");
            }
            Ok(matches)
        })
        .transpose()?;

    let stream_range = stream.stream_pc..stream.stream_pc + stream.compressed_len;
    let mut patched = source.to_vec();
    patched[stream_range.clone()].fill(0);
    patched[stream.stream_pc..stream.stream_pc + compressed.len()].copy_from_slice(&compressed);
    let checksum = crate::rom::fix_checksum(&mut patched)?;
    let diff_confined_to_registered_writes =
        source
            .iter()
            .zip(&patched)
            .enumerate()
            .all(|(offset, (before, after))| {
                before == after
                    || stream_range.contains(&offset)
                    || (CHECKSUM_PC..CHECKSUM_PC + 4).contains(&offset)
            });
    if !diff_confined_to_registered_writes {
        bail!("Tokoton play PoC diff escaped its registered stream and checksum");
    }

    Ok((
        patched.clone(),
        BuildReport {
            verdict: "Remix-only あそぶ and おてほん surfaces rebuilt from separate generated Korean 놀기 and 시범 labels; fangs and mouth frame preserved".to_owned(),
            source_path,
            source_sha256,
            play_asset_path: play_asset_path.display().to_string(),
            play_asset_sha256: sha256(&play_asset_bytes),
            demonstration_asset_path: demonstration_asset_path.display().to_string(),
            demonstration_asset_sha256: sha256(&demonstration_asset_bytes),
            runtime_dump: runtime_dump.display().to_string(),
            output_path,
            output_sha256: sha256(&patched),
            texts: vec!["놀기".to_owned(), "시범".to_owned()],
            surfaces: vec![
                "あそぶ selected/unselected -> 놀기".to_owned(),
                "おてほん selected/unselected -> 시범".to_owned(),
            ],
            stream_pc: format!("0x{:06X}", stream.stream_pc),
            stream_lorom: stream.stream_lorom.to_owned(),
            original_compressed_len: stream.compressed_len,
            patched_compressed_len: compressed.len(),
            decompressed_len: stream.decompressed_len,
            runtime_vram_range: format!(
                "0x{VRAM_BYTE:04X}-0x{:04X}",
                VRAM_BYTE + stream.decompressed_len - 1
            ),
            runtime_vram_matches_source: true,
            changed_tiles: plan.changed_tiles,
            changed_decompressed_bytes: plan.changed_decompressed_bytes,
            erased_jp_pixels: plan.erased_jp_pixels,
            preserved_frame_pixels: plan.preserved_frame_pixels,
            dropped_frame_overlap_pixels: plan.dropped_frame_overlap_pixels,
            blocked_unowned_pixels: 0,
            compression_roundtrip_matches: true,
            patched_runtime_dump: patched_runtime_dump.map(|path| path.display().to_string()),
            patched_runtime_vram_matches,
            diff_confined_to_registered_writes,
            checksum_hex: format!("0x{checksum:04X}"),
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignStreamReport {
    pub stream: String,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub texts: Vec<String>,
    pub asset_paths: Vec<String>,
    pub asset_sha256: Vec<String>,
    pub runtime_dump: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub erased_jp_pixels: usize,
    pub preserved_frame_pixels: usize,
    pub dropped_frame_overlap_pixels: usize,
    pub compression_roundtrip_matches: bool,
}

/// Rewrites one mouth-sign stream of an already verified derivative with one
/// generated 48x40 label per slot. The stream bytes must still be the original
/// ones; the recompressed stream is written in place and the rest of its
/// original range is zero-filled. The caller owns the checksum.
pub(crate) fn patch_sign_stream(
    source: &[u8],
    stream: &SignStream,
    texts: &[&str],
    asset_paths: &[&Path],
    runtime_dump: &Path,
) -> Result<(Vec<u8>, SignStreamReport)> {
    if asset_paths.len() != stream.slots.len() || texts.len() != stream.slots.len() {
        bail!(
            "{} needs {} labels and texts in slot order",
            stream.name,
            stream.slots.len()
        );
    }
    let block = verify_stream(stream, source)?;
    let runtime = read_runtime_dump(runtime_dump, &block)?;
    let asset_bytes = asset_paths
        .iter()
        .map(|path| fs::read(path).with_context(|| format!("read sign asset {}", path.display())))
        .collect::<Result<Vec<_>>>()?;
    let assets = asset_bytes
        .iter()
        .map(|bytes| decode_rgba8_png(bytes, ASSET_WIDTH, ASSET_HEIGHT))
        .collect::<Result<Vec<_>>>()?;
    let slots = assets
        .iter()
        .map(|asset| Some(asset.as_slice()))
        .collect::<Vec<_>>();
    let plan = plan_signs(stream, &block, &runtime, &slots)?;
    let compressed = compress_in_place(stream, &plan.decoded)?;
    let mut patched = source.to_vec();
    patched[stream.stream_pc..stream.stream_pc + stream.compressed_len].fill(0);
    patched[stream.stream_pc..stream.stream_pc + compressed.len()].copy_from_slice(&compressed);
    Ok((
        patched,
        SignStreamReport {
            stream: stream.name.to_owned(),
            stream_pc: format!("0x{:06X}", stream.stream_pc),
            stream_lorom: stream.stream_lorom.to_owned(),
            texts: texts.iter().map(|text| (*text).to_owned()).collect(),
            asset_paths: asset_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            asset_sha256: asset_bytes.iter().map(|bytes| sha256(bytes)).collect(),
            runtime_dump: runtime_dump.display().to_string(),
            original_compressed_len: stream.compressed_len,
            patched_compressed_len: compressed.len(),
            decompressed_len: stream.decompressed_len,
            changed_tiles: plan.changed_tiles,
            changed_decompressed_bytes: plan.changed_decompressed_bytes,
            erased_jp_pixels: plan.erased_jp_pixels,
            preserved_frame_pixels: plan.preserved_frame_pixels,
            dropped_frame_overlap_pixels: plan.dropped_frame_overlap_pixels,
            compression_roundtrip_matches: true,
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignSlotReport {
    pub x: usize,
    pub y: usize,
    pub source_text: String,
    pub text_pixels: usize,
    pub halo_pixels: usize,
    pub frame_pixels: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignPreviewReport {
    pub verdict: String,
    pub stream: String,
    pub source_sha256: String,
    pub runtime_dump: String,
    pub assets: Vec<String>,
    pub stream_pc: String,
    pub stream_lorom: String,
    pub original_compressed_len: usize,
    pub patched_compressed_len: usize,
    pub decompressed_len: usize,
    pub changed_tiles: usize,
    pub changed_decompressed_bytes: usize,
    pub erased_jp_pixels: usize,
    pub preserved_frame_pixels: usize,
    pub dropped_frame_overlap_pixels: usize,
    pub protected_tiles_unchanged: bool,
    pub compression_roundtrip_matches: bool,
    pub slots: Vec<SignSlotReport>,
    pub preview_path: String,
    pub preview_sha256: String,
}

/// Plans a mouth-sign stream and renders each original sign next to its
/// result. With no assets it only erases the Japanese lettering, which proves
/// the erase-and-preserve half of an importer before a generated Korean label
/// exists; with one 48x40 asset per slot it previews the composited label. It
/// applies every stream check of a build but never writes a ROM.
pub fn sign_preview(
    stream_name: &str,
    source: &[u8],
    runtime_dump: &Path,
    asset_paths: &[std::path::PathBuf],
    preview_path: &Path,
) -> Result<SignPreviewReport> {
    let stream = sign_stream(stream_name)?;
    let source_sha256 = sha256(source);
    if source_sha256 != TARGET_SHA256 {
        bail!("Remix ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {source_sha256}");
    }
    if !asset_paths.is_empty() && asset_paths.len() != stream.slots.len() {
        bail!(
            "{} needs {} assets in slot order, got {}",
            stream.name,
            stream.slots.len(),
            asset_paths.len()
        );
    }
    let block = verify_stream(&stream, source)?;
    let runtime = read_runtime_dump(runtime_dump, &block)?;
    let assets = asset_paths
        .iter()
        .map(|path| {
            let bytes =
                fs::read(path).with_context(|| format!("read sign asset {}", path.display()))?;
            decode_rgba8_png(&bytes, ASSET_WIDTH, ASSET_HEIGHT)
        })
        .collect::<Result<Vec<_>>>()?;
    let slots = if assets.is_empty() {
        vec![None; stream.slots.len()]
    } else {
        assets.iter().map(|asset| Some(asset.as_slice())).collect()
    };
    let plan = plan_signs(&stream, &block, &runtime, &slots)?;
    let compressed = compress_in_place(&stream, &plan.decoded)?;

    let mut patched_vram = runtime.vram.clone();
    patched_vram[VRAM_BYTE..VRAM_BYTE + stream.decompressed_len].copy_from_slice(&plan.decoded);
    let palette = decode_cgram(&runtime.cgram)?;
    let before = render_index_layer(&runtime.vram)?;
    let after = render_index_layer(&patched_vram)?;
    let preview = side_by_side_preview(&stream, &before, &after, &palette)?;
    if let Some(parent) = preview_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(preview_path, &preview)?;

    let verdict = if assets.is_empty() {
        format!(
            "{} Japanese lettering erased with frame and fangs preserved; no replacement drawn",
            stream.name
        )
    } else {
        format!(
            "{} generated labels composited over the erased signs with frame and fangs preserved",
            stream.name
        )
    };
    Ok(SignPreviewReport {
        verdict,
        stream: stream.name.to_owned(),
        source_sha256,
        runtime_dump: runtime_dump.display().to_string(),
        assets: asset_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        stream_pc: format!("0x{:06X}", stream.stream_pc),
        stream_lorom: stream.stream_lorom.to_owned(),
        original_compressed_len: stream.compressed_len,
        patched_compressed_len: compressed.len(),
        decompressed_len: stream.decompressed_len,
        changed_tiles: plan.changed_tiles,
        changed_decompressed_bytes: plan.changed_decompressed_bytes,
        erased_jp_pixels: plan.erased_jp_pixels,
        preserved_frame_pixels: plan.preserved_frame_pixels,
        dropped_frame_overlap_pixels: plan.dropped_frame_overlap_pixels,
        protected_tiles_unchanged: true,
        compression_roundtrip_matches: true,
        slots: plan.slots,
        preview_path: preview_path.display().to_string(),
        preview_sha256: sha256(&preview),
    })
}

fn verify_stream(stream: &SignStream, source: &[u8]) -> Result<Vec<u8>> {
    let block = crate::snes_lz::decompress(source, stream.stream_pc)?;
    if block.compressed_len != stream.compressed_len || block.bytes.len() != stream.decompressed_len
    {
        bail!(
            "Remix {} stream differs from the verified contract: compressed {} / decoded {}",
            stream.name,
            block.compressed_len,
            block.bytes.len()
        );
    }
    let raw = source
        .get(stream.stream_pc..stream.stream_pc + stream.compressed_len)
        .with_context(|| format!("{} source stream is outside ROM", stream.name))?;
    if sha256(raw) != stream.raw_sha256 || sha256(&block.bytes) != stream.decoded_sha256 {
        bail!(
            "Remix {} stream hashes differ from the runtime-owned resource",
            stream.name
        );
    }
    Ok(block.bytes)
}

struct SignPlan {
    decoded: Vec<u8>,
    changed_tiles: usize,
    changed_decompressed_bytes: usize,
    erased_jp_pixels: usize,
    preserved_frame_pixels: usize,
    dropped_frame_overlap_pixels: usize,
    slots: Vec<SignSlotReport>,
}

/// Plans the decoded CHR for every sign of `stream`. A slot with an asset
/// draws that generated label; a slot without one only erases the Japanese
/// lettering. Frame pixels (outline, lips, fangs) are never repainted.
fn plan_signs(
    stream: &SignStream,
    source_decoded: &[u8],
    runtime: &RuntimeDump,
    assets: &[Option<&[u8]>],
) -> Result<SignPlan> {
    if assets.len() != stream.slots.len() {
        bail!(
            "{} expects {} sign assets, got {}",
            stream.name,
            stream.slots.len(),
            assets.len()
        );
    }
    let indices = render_index_layer(&runtime.vram)?;
    let palette = decode_cgram(&runtime.cgram)?;
    let mut decoded = source_decoded.to_vec();
    let mut changed_keys = BTreeSet::new();
    let mut erased_jp_pixels = 0usize;
    let mut preserved_frame_pixels = 0usize;
    let mut dropped_frame_overlap_pixels = 0usize;
    let mut slots = Vec::with_capacity(stream.slots.len());
    for (slot, asset) in stream.slots.iter().zip(assets) {
        let canvas = crate::mouth_sign::Region {
            x: slot.x,
            y: slot.y,
            width: ASSET_WIDTH,
            height: ASSET_HEIGHT,
        };
        let mask = crate::mouth_sign::analyze_around(
            &indices,
            SCREEN_WIDTH,
            canvas,
            MOUTH_MARGIN,
            MOUTH_COLORS,
            GLOW_RADIUS,
        )
        .with_context(|| format!("classify {} sign {}", stream.name, slot.source_text))?;
        let (text_pixels, halo_pixels, frame_pixels) = mask.counts_within(canvas);
        slots.push(SignSlotReport {
            x: slot.x,
            y: slot.y,
            source_text: slot.source_text.to_owned(),
            text_pixels,
            halo_pixels,
            frame_pixels,
        });
        for local_y in 0..ASSET_HEIGHT {
            for local_x in 0..ASSET_WIDTH {
                let x = slot.x + local_x;
                let y = slot.y + local_y;
                let original_index = indices[y * SCREEN_WIDTH + x] & 0x0F;
                let class = mask.class_at(x, y);
                let restored = mask.restored_at(x, y);
                let candidate_x = local_x + GENERATED_X_SHIFT.unsigned_abs();
                let candidate_y = local_y + GENERATED_Y_SHIFT.unsigned_abs();
                let generated = match asset {
                    Some(asset) if candidate_x < ASSET_WIDTH && candidate_y < ASSET_HEIGHT => {
                        let offset = (candidate_y * ASSET_WIDTH + candidate_x) * 4;
                        (asset[offset + 3] == 0xFF)
                            .then(|| {
                                map_rgb_to_palette_index(
                                    [asset[offset], asset[offset + 1], asset[offset + 2]],
                                    &palette,
                                )
                            })
                            .transpose()?
                    }
                    _ => None,
                };
                // The dark-maroon outline and drop shadow (2) are drawn like the
                // shadow under the original lettering; near-black (1) and orange
                // (5) never belong to a normalized sign label.
                let generated = generated.filter(|index| !matches!(index, 1 | 5));
                let desired = match class {
                    crate::mouth_sign::PixelClass::Frame => {
                        preserved_frame_pixels += 1;
                        if generated.is_some() {
                            dropped_frame_overlap_pixels += 1;
                        }
                        original_index
                    }
                    crate::mouth_sign::PixelClass::Text | crate::mouth_sign::PixelClass::Halo => {
                        erased_jp_pixels += 1;
                        generated.unwrap_or(restored)
                    }
                    // Fill keeps its original shade unless the restoration
                    // moved the mouth's shade boundary through it.
                    crate::mouth_sign::PixelClass::Fill => generated.unwrap_or(restored),
                };
                let entry = tilemap_entry_at(&runtime.vram, x / 8, y / 8)?;
                if entry.palette != 2 || entry.hflip || entry.vflip {
                    bail!(
                        "{} sign pixel {x},{y} has an unsupported tilemap state",
                        stream.name
                    );
                }
                let tile = entry.tile.checked_sub(VRAM_BASE_TILE).with_context(|| {
                    format!("{} sign tile precedes its verified VRAM block", stream.name)
                })?;
                if tile * TILE_LEN >= stream.decompressed_len {
                    bail!(
                        "{} sign tile 0x{:03X} is outside its source stream",
                        stream.name,
                        entry.tile
                    );
                }
                let key = (tile, x % 8, y % 8);
                let tile_bytes = tile * TILE_LEN..(tile + 1) * TILE_LEN;
                let before = decode_4bpp_pixel(&source_decoded[tile_bytes.clone()], x % 8, y % 8);
                if desired != before {
                    changed_keys.insert(key);
                    set_4bpp_pixel(&mut decoded[tile_bytes], x % 8, y % 8, desired);
                }
            }
        }
    }

    let unowned = collect_unowned_consumer_keys(stream, &runtime.vram)?;
    let blocked_unowned_pixels = changed_keys.intersection(&unowned).count();
    if blocked_unowned_pixels != 0 {
        bail!(
            "{blocked_unowned_pixels} {} CHR pixels have unowned screen consumers",
            stream.name
        );
    }
    for (index, (before, after)) in source_decoded.iter().zip(&decoded).enumerate() {
        if before != after && index / TILE_LEN >= stream.protected_first_tile {
            bail!(
                "{} patch changed protected decoded tile 0x{:02X}",
                stream.name,
                index / TILE_LEN
            );
        }
    }
    let changed_decompressed_bytes = source_decoded
        .iter()
        .zip(&decoded)
        .filter(|(before, after)| before != after)
        .count();
    let changed_tiles = source_decoded
        .as_chunks::<TILE_LEN>()
        .0
        .iter()
        .zip(decoded.as_chunks::<TILE_LEN>().0)
        .filter(|(before, after)| before != after)
        .count();
    Ok(SignPlan {
        decoded,
        changed_tiles,
        changed_decompressed_bytes,
        erased_jp_pixels,
        preserved_frame_pixels,
        dropped_frame_overlap_pixels,
        slots,
    })
}

fn compress_in_place(stream: &SignStream, decoded: &[u8]) -> Result<Vec<u8>> {
    let compressed = crate::snes_lz::compress(decoded);
    if compressed.len() > stream.compressed_len {
        bail!(
            "Korean {} stream grew from {} to {} bytes",
            stream.name,
            stream.compressed_len,
            compressed.len()
        );
    }
    let roundtrip = crate::snes_lz::decompress(&compressed, 0)?;
    if roundtrip.compressed_len != compressed.len() || roundtrip.bytes != decoded {
        bail!(
            "Korean {} stream failed compression round-trip",
            stream.name
        );
    }
    Ok(compressed)
}

fn side_by_side_preview(
    stream: &SignStream,
    before: &[u8],
    after: &[u8],
    palette: &[[u8; 3]],
) -> Result<Vec<u8>> {
    // One row per sign: the original mouth, then the erased mouth, each
    // cropped to the canvas grown by the classification margin.
    const SCALE: usize = 4;
    const GAP: usize = 2;
    let (side, top_margin, bottom_margin) = MOUTH_MARGIN;
    let crop_width = ASSET_WIDTH + side * 2;
    let crop_height = ASSET_HEIGHT + top_margin + bottom_margin;
    let width = (crop_width * 2 + GAP) * SCALE;
    let height = ((crop_height + GAP) * stream.slots.len()) * SCALE;
    let mut pixels = vec![0u8; width * height * 4];
    for (row, slot) in stream.slots.iter().enumerate() {
        let left = slot.x.saturating_sub(side);
        let top = slot.y.saturating_sub(top_margin);
        for (panel, layer) in [before, after].into_iter().enumerate() {
            let panel_x = panel * (crop_width + GAP);
            let panel_y = row * (crop_height + GAP);
            for y in 0..crop_height {
                for x in 0..crop_width {
                    let (sx, sy) = (left + x, top + y);
                    if sx >= SCREEN_WIDTH || sy >= SCREEN_HEIGHT {
                        continue;
                    }
                    let color = palette[usize::from(layer[sy * SCREEN_WIDTH + sx])];
                    for dy in 0..SCALE {
                        for dx in 0..SCALE {
                            let offset = ((((panel_y + y) * SCALE + dy) * width)
                                + (panel_x + x) * SCALE
                                + dx)
                                * 4;
                            pixels[offset..offset + 3].copy_from_slice(&color);
                            pixels[offset + 3] = 0xFF;
                        }
                    }
                }
            }
        }
    }
    encode_png_rgba(width, height, &pixels)
}

struct RuntimeDump {
    vram: Vec<u8>,
    cgram: Vec<u8>,
}

fn read_runtime_dump(path: &Path, decoded: &[u8]) -> Result<RuntimeDump> {
    let vram = fs::read(path.join("vram.bin"))?;
    let cgram = fs::read(path.join("cram.bin"))?;
    if vram.len() != VRAM_LEN || cgram.len() != CGRAM_LEN {
        bail!("Tokoton play runtime dump has invalid VRAM/CGRAM sizes");
    }
    if vram.get(VRAM_BYTE..VRAM_BYTE + decoded.len()) != Some(decoded) {
        bail!("sign runtime VRAM does not contain the source stream at 0x5600");
    }
    let state: serde_json::Value = serde_json::from_slice(&fs::read(path.join("state.json"))?)?;
    let number = |key: &str| {
        state
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .with_context(|| format!("Tokoton play runtime state lacks {key}"))
    };
    let boolean = |key: &str| {
        state
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .with_context(|| format!("Tokoton play runtime state lacks {key}"))
    };
    if number("ppu.bgMode")? != 1
        || number("ppu.layers[0].chrAddress")? != 0
        || number("ppu.layers[0].tilemapAddress")?.checked_mul(2) != Some(TILEMAP_BYTE as u64)
        || boolean("ppu.layers[0].doubleWidth")?
        || boolean("ppu.layers[0].doubleHeight")?
    {
        bail!("Tokoton play runtime BG1 geometry differs from the verified Mode 1 contract");
    }
    Ok(RuntimeDump { vram, cgram })
}

fn render_index_layer(vram: &[u8]) -> Result<Vec<u8>> {
    let mut output = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
    for tile_y in 0..SCREEN_HEIGHT / 8 {
        for tile_x in 0..SCREEN_WIDTH / 8 {
            let entry = tilemap_entry_at(vram, tile_x, tile_y)?;
            let tile = vram
                .get(entry.tile * TILE_LEN..(entry.tile + 1) * TILE_LEN)
                .context("Tokoton play BG1 tile is outside VRAM")?;
            for y in 0..8 {
                for x in 0..8 {
                    output[(tile_y * 8 + y) * SCREEN_WIDTH + tile_x * 8 + x] =
                        entry.palette * 16 + decode_4bpp_pixel(tile, x, y);
                }
            }
        }
    }
    Ok(output)
}

#[derive(Clone, Copy)]
struct TilemapEntry {
    tile: usize,
    palette: u8,
    hflip: bool,
    vflip: bool,
}

fn tilemap_entry_at(vram: &[u8], x: usize, y: usize) -> Result<TilemapEntry> {
    if x >= 32 || y >= 32 {
        bail!("Tokoton play tilemap coordinate is outside 32x32");
    }
    let offset = TILEMAP_BYTE + (y * 32 + x) * 2;
    let bytes = vram
        .get(offset..offset + 2)
        .context("Tokoton play tilemap outside VRAM")?;
    let raw = u16::from_le_bytes([bytes[0], bytes[1]]);
    Ok(TilemapEntry {
        tile: usize::from(raw & 0x03FF),
        palette: ((raw >> 10) & 7) as u8,
        hflip: raw & 0x4000 != 0,
        vflip: raw & 0x8000 != 0,
    })
}

fn collect_unowned_consumer_keys(
    stream: &SignStream,
    vram: &[u8],
) -> Result<BTreeSet<(usize, usize, usize)>> {
    let mut keys = BTreeSet::new();
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let owned = stream.slots.iter().any(|slot| {
                (slot.x..slot.x + ASSET_WIDTH).contains(&x)
                    && (slot.y..slot.y + ASSET_HEIGHT).contains(&y)
            });
            if owned {
                continue;
            }
            let entry = tilemap_entry_at(vram, x / 8, y / 8)?;
            let Some(tile) = entry.tile.checked_sub(VRAM_BASE_TILE) else {
                continue;
            };
            if tile * TILE_LEN >= stream.decompressed_len {
                continue;
            }
            let source_x = if entry.hflip { 7 - x % 8 } else { x % 8 };
            let source_y = if entry.vflip { 7 - y % 8 } else { y % 8 };
            keys.insert((tile, source_x, source_y));
        }
    }
    Ok(keys)
}

fn map_rgb_to_palette_index(rgb: [u8; 3], palette: &[[u8; 3]]) -> Result<u8> {
    let matches = (1u8..16)
        .filter(|index| palette[2 * 16 + usize::from(*index)] == rgb)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        bail!("Tokoton play asset color is absent from runtime palette 2");
    }
    Ok(matches
        .iter()
        .copied()
        .find(|index| TEXT_CORE.contains(index))
        .or_else(|| {
            matches
                .iter()
                .copied()
                .find(|index| TEXT_EDGE.contains(index))
        })
        .unwrap_or(matches[0]))
}

fn opaque_bounds(
    pixels: &[u8],
    width: usize,
    height: usize,
) -> Result<(usize, usize, usize, usize)> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    for y in 0..height {
        for x in 0..width {
            if pixels[(y * width + x) * 4 + 3] >= 0x80 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x > max_x || min_y > max_y {
        bail!("generated Tokoton play image contains no opaque pixels");
    }
    Ok((min_x, min_y, max_x, max_y))
}

fn color_distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| u32::from(a.abs_diff(b)))
        .sum()
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
                expand5((color & 31) as u8),
                expand5(((color >> 5) & 31) as u8),
                expand5(((color >> 10) & 31) as u8),
            ]
        })
        .collect())
}

fn expand5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

/// Decodes a generated sign label and tidies its generation leftovers.
fn decode_rgba8_png(encoded: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let (actual_width, actual_height, mut pixels) = decode_rgba8_png_any(encoded)?;
    if actual_width != width || actual_height != height {
        bail!("Tokoton play PNG is {actual_width}x{actual_height}, expected {width}x{height}");
    }
    crate::mouth_sign::tidy_generated_lettering(&mut pixels, width, height);
    Ok(pixels)
}

fn decode_rgba8_png_any(encoded: &[u8]) -> Result<(usize, usize, Vec<u8>)> {
    let decoder = png::Decoder::new(Cursor::new(encoded));
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![0u8; reader.output_buffer_size().context("PNG is too large")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!("Tokoton play PNG must be 8-bit RGBA");
    }
    pixels.truncate(info.buffer_size());
    Ok((info.width as usize, info.height as usize, pixels))
}

fn encode_png_rgba(width: usize, height: usize, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(pixels)?;
    }
    Ok(output)
}

fn decode_4bpp_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let bit = 7 - x;
    ((tile[y * 2] >> bit) & 1)
        | (((tile[y * 2 + 1] >> bit) & 1) << 1)
        | (((tile[16 + y * 2] >> bit) & 1) << 2)
        | (((tile[16 + y * 2 + 1] >> bit) & 1) << 3)
}

fn set_4bpp_pixel(tile: &mut [u8], x: usize, y: usize, value: u8) {
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
    fn verified_target_tiles_are_two_disjoint_six_by_five_surfaces() {
        assert_eq!(VRAM_BASE_TILE, 0x2B0);
        assert_eq!(TOKOTON_CHOICES.protected_first_tile * TILE_LEN, 1_920);
        assert_eq!(TOKOTON_CHOICES.decompressed_len / TILE_LEN, 144);
    }

    #[test]
    fn every_sign_stream_owns_thirty_tiles_per_slot_before_its_protected_tail() {
        for stream in [TOKOTON_CHOICES, EASY_COURSES] {
            let tiles_per_slot = (ASSET_WIDTH / 8) * (ASSET_HEIGHT / 8);
            assert_eq!(tiles_per_slot, 30);
            assert_eq!(
                stream.protected_first_tile,
                stream.slots.len() * tiles_per_slot,
                "{}",
                stream.name
            );
            assert!(stream.protected_first_tile * TILE_LEN <= stream.decompressed_len);
            assert_eq!(sign_stream(stream.name).expect("known").name, stream.name);
        }
        assert!(sign_stream("mode_labels").is_err());
    }

    /// Needs the local Remix ROM and the two original sign dumps.
    #[test]
    #[ignore = "requires the Remix JP ROM and original sign VRAM dumps under out/evidence/menu/"]
    fn erasing_keeps_every_fang_and_leaves_no_lettering() {
        let rom = std::fs::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc").expect("Remix ROM");
        for (stream, dump) in [
            (
                TOKOTON_CHOICES,
                "out/evidence/menu/remix_tokoton_play_select_original",
            ),
            (
                EASY_COURSES,
                "out/evidence/menu/remix_easy_course_select_original",
            ),
        ] {
            let block = verify_stream(&stream, &rom).expect("verified stream");
            let runtime = read_runtime_dump(Path::new(dump), &block).expect("runtime dump");
            let blanks = vec![None; stream.slots.len()];
            let plan = plan_signs(&stream, &block, &runtime, &blanks).expect("plan");
            let mut erased_vram = runtime.vram.clone();
            erased_vram[VRAM_BYTE..VRAM_BYTE + stream.decompressed_len]
                .copy_from_slice(&plan.decoded);
            let before = render_index_layer(&runtime.vram).expect("before");
            let after = render_index_layer(&erased_vram).expect("after");
            for slot in stream.slots {
                let white = |layer: &[u8], rows: std::ops::Range<usize>| {
                    rows.flat_map(|y| (slot.x..slot.x + ASSET_WIDTH).map(move |x| (x, y)))
                        .filter(|&(x, y)| layer[y * SCREEN_WIDTH + x] == 2 * 16 + 15)
                        .count()
                };
                // The fangs hang in the top three rows; the tallest letter
                // (お) starts on the fourth.
                let fang_rows = slot.y..slot.y + 3;
                assert!(
                    white(&before, fang_rows.clone()) > 0,
                    "{}",
                    slot.source_text
                );
                assert_eq!(
                    white(&before, fang_rows.clone()),
                    white(&after, fang_rows),
                    "{} fangs",
                    slot.source_text
                );
                let letter_rows = slot.y + 6..slot.y + ASSET_HEIGHT;
                assert!(
                    white(&before, letter_rows.clone()) > 100,
                    "{}",
                    slot.source_text
                );
                assert_eq!(
                    white(&after, letter_rows),
                    0,
                    "{} lettering",
                    slot.source_text
                );
            }
        }
    }

    #[test]
    fn sign_slots_do_not_overlap_and_stay_on_screen() {
        for stream in [TOKOTON_CHOICES, EASY_COURSES] {
            for (index, slot) in stream.slots.iter().enumerate() {
                assert!(slot.x + ASSET_WIDTH <= SCREEN_WIDTH);
                assert!(slot.y + ASSET_HEIGHT <= SCREEN_HEIGHT);
                for other in &stream.slots[index + 1..] {
                    assert!(slot.x + ASSET_WIDTH <= other.x || other.x + ASSET_WIDTH <= slot.x);
                }
            }
        }
    }
}
