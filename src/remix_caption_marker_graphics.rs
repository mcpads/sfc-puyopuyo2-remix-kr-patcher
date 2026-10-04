use std::ops::Range;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

// Easy-course ending captions (G17). Bank $27 keeps the caption CHR and tilemap
// streams back to back with no gap, so each rewritten stream must fit its own
// slot; the asset-table records stay untouched.
const CAPTION_BANK_PC: usize = 0x13_8000;
const CAPTION_REGION: Range<usize> = 0x13_8B0B..0x13_D321;
const CAPTION_FIRST_GROUP: usize = 1;
// Course 1 and 2 CHR and tilemap stream indices into CAPTION_STREAMS.
const CAPTION_REWRITTEN: [usize; 4] = [0, 1, 4, 5];
const CAPTION_STREAMS: [CaptionStream; 7] = [
    CaptionStream {
        pc: 0x13_8B0B,
        len: 5_536,
        sha256: "2830821df5e50e3c3d3eba4acca43c31a68a7638fada9718daa3fb2cda4e4328",
    },
    CaptionStream {
        pc: 0x13_A0AB,
        len: 5_028,
        sha256: "3d118fb9042e201dd6dd522d95108a0ecbcc439bab5484d864c25cb0d168c655",
    },
    CaptionStream {
        pc: 0x13_B44F,
        len: 5_906,
        sha256: "02d0e797b4fcfaef5b1561b1d377bb4a07e3c3b66cdb4855e2f548be8d404c74",
    },
    CaptionStream {
        pc: 0x13_CB61,
        len: 410,
        sha256: "1bf7b733515e7ca1cbb2130842c1bb303889cd8706f40737cba84738c7276f3d",
    },
    CaptionStream {
        pc: 0x13_CCFB,
        len: 539,
        sha256: "d730406e0b8ef5eff69117b5a8a422725e0fa5d2b14f1a4f39bf456ab96f6bcd",
    },
    CaptionStream {
        pc: 0x13_CF16,
        len: 487,
        sha256: "517185168d254a04a7d3b1e86614845363b14d280fe18588751f672da4fe69c6",
    },
    CaptionStream {
        pc: 0x13_D0FD,
        len: 548,
        sha256: "bfe634aa373064ec5df19dda5091531d036ad277a5ecdc9bda71087e7642934c",
    },
];
const CAPTION_STROKE: u8 = 15;
const CAPTION_OUTLINE: u8 = 3;
const CAPTION_FONT_PX: f32 = 12.0;
const CAPTION_RIGHT_EDGE_PX: usize = 28 * 8;
const CAPTION_LINE_ROWS: [usize; 2] = [21, 24];
const MAP_COLUMNS: usize = 32;

// Course 1 hand-lettered つん, reused twice by the illustration tilemap.
const POKE_TILES: [usize; 6] = [0x16, 0x17, 0x18, 0x1A, 0x1B, 0x1C];
const POKE_TEXT: &str = "콕";
const POKE_FILL: u8 = 12;
const POKE_OUTLINE: u8 = 2;

struct CaptionStream {
    pc: usize,
    len: usize,
    sha256: &'static str,
}

struct CaptionCourse {
    name: &'static str,
    chr_stream: usize,
    map_stream: usize,
    start_column: usize,
    lines: [&'static str; 2],
    poke: bool,
}

const COURSES: [CaptionCourse; 2] = [
    CaptionCourse {
        name: "easy course 1",
        chr_stream: 0,
        map_stream: 4,
        start_column: 6,
        lines: ["이겨 버렸다. 나,", "조금 기쁠지도…"],
        poke: true,
    },
    CaptionCourse {
        name: "easy course 2",
        chr_stream: 1,
        map_stream: 5,
        start_column: 5,
        lines: ["해냈다! 다음은 졸업 시험!", "불끈~~~~!"],
        poke: false,
    },
];

// Nomi's "ここにいる" markers (G19). Each stream is rewritten in place.
const FLOOR_STREAM: InPlaceStream = InPlaceStream {
    name: "Nomi floor marker",
    pc: 0x08_8C68,
    len: 4_365,
    decoded_len: 6_656,
    sha256: "8b735641281efad80bb04505bcaeea52f3873617dcda558180cdfc46af340f47",
};
const ICON_STREAM: InPlaceStream = InPlaceStream {
    name: "opponent icon",
    pc: 0x0B_3BB8,
    len: 11_152,
    decoded_len: 13_824,
    sha256: "70d694ecab3356d02b184aad8887ca24ec0d3e36fd41aef72fe61eefa5832a7c",
};
const PORTRAIT_STREAM: InPlaceStream = InPlaceStream {
    name: "Nomi portrait tile pool",
    pc: 0x05_97D1,
    len: 1_452,
    decoded_len: 2_784,
    sha256: "891364cc5142d34033e6d3fdceb6b91d484c68a00671a550d2e725c95683f2e9",
};
const FLOOR_TILES: [[usize; 4]; 3] = [
    [0x0C, 0x0D, 0x0E, 0x0F],
    [0x1C, 0x1D, 0x1E, 0x1F],
    [0x2C, 0x2D, 0x2E, 0x2F],
];
const FLOOR_LINES: [&str; 2] = ["여기", "있어"];
const FLOOR_FONT_PX: f32 = 10.0;
const FLOOR_OUTLINE: u8 = 1;
// Top-to-bottom shading of the original orange lettering in OBJ palette 7.
const FLOOR_SHADES: [u8; 5] = [11, 10, 8, 5, 3];
const ICON_TILES: [[usize; 3]; 3] = [[0x09, 0x0A, 0x0B], [0x19, 0x1A, 0x1B], [0x29, 0x2A, 0x2B]];
const ICON_TEXT: &str = "여기";
const ICON_OUTLINE: u8 = 1;
const ICON_SHADES: [u8; 3] = [13, 10, 4];
// Lettering occupies rows 2-17 inside the frame. The arrow starts on row 18,
// where only columns 10-12 belong to it.
const ICON_TEXT_ROWS: Range<usize> = 2..18;
const ICON_TEXT_COLUMNS: Range<usize> = 2..22;
const ICON_ARROW_ROW: usize = 18;
const ICON_ARROW_COLUMNS: Range<usize> = 10..13;
const PORTRAIT_TILES: [usize; 6] = [3, 4, 5, 6, 7, 8];
const PORTRAIT_TEXT: &str = "여기야";
const PORTRAIT_STROKE: u8 = 15;
const PORTRAIT_TOP: usize = 1;

// Galmuri11 is pixel-exact only at 12px, so the 7-pixel lettering of the icon
// and portrait uses hand-drawn syllables.
const SMALL_GLYPHS: [(char, [&str; 7]); 3] = [
    (
        '여',
        [
            "......#", ".#....#", "#.#.###", "#.#...#", "#.#.###", ".#....#", "......#",
        ],
    ),
    (
        '기',
        [
            "####..#", "...#..#", "...#..#", "..#...#", ".#....#", "#.....#", "......#",
        ],
    ),
    (
        '야',
        [
            "....#..", ".#..#..", "#.#.###", "#.#.#..", "#.#.###", ".#..#..", "....#..",
        ],
    ),
];

const GALMURI_SHA256: &str = "2c709890595668f7bdb6df408420fda957dde0288e95b31a1cc17a2ab98b4b4f";
const GALMURI_BOLD_SHA256: &str =
    "5265b2f437fe81f0c8095b44c0173dd9a276b58a42552bf983f21c0e69e6e8af";

struct InPlaceStream {
    name: &'static str,
    pc: usize,
    len: usize,
    decoded_len: usize,
    sha256: &'static str,
}

impl InPlaceStream {
    fn range(&self) -> Range<usize> {
        self.pc..self.pc + self.len
    }
}

pub struct Inputs<'a> {
    pub galmuri_ttf_data: &'a [u8],
    pub galmuri_bold_ttf_data: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub verdict: String,
    pub captions: Vec<CaptionReport>,
    pub caption_streams: Vec<RewrittenStream>,
    pub markers: Vec<MarkerReport>,
    pub diff_confined_to_registered_writes: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaptionReport {
    pub course: String,
    pub lines: Vec<String>,
    pub poke_text: Option<String>,
    pub caption_tiles_before: usize,
    pub caption_tiles_after: usize,
    pub appended_tiles: usize,
    pub chr_tiles_before: usize,
    pub chr_tiles_after: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RewrittenStream {
    pub group: usize,
    pub pc: String,
    pub original_len: usize,
    pub rewritten_len: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MarkerReport {
    pub surface: String,
    pub text: String,
    pub stream_pc: String,
    pub original_len: usize,
    pub rewritten_len: usize,
    pub changed_tiles: usize,
}

pub fn registered_write(offset: usize) -> Option<&'static str> {
    if CAPTION_REWRITTEN
        .iter()
        .any(|index| caption_stream_range(*index).contains(&offset))
    {
        Some("easy-course caption streams")
    } else if FLOOR_STREAM.range().contains(&offset) {
        Some("Nomi floor marker stream")
    } else if ICON_STREAM.range().contains(&offset) {
        Some("opponent icon stream")
    } else if PORTRAIT_STREAM.range().contains(&offset) {
        Some("Nomi portrait tile pool")
    } else {
        None
    }
}

fn caption_stream_range(index: usize) -> Range<usize> {
    let stream = &CAPTION_STREAMS[index];
    stream.pc..stream.pc + stream.len
}

// Each group list is one 8-byte record followed by a 0xFFFF terminator.
fn caption_record_pc(group: usize) -> usize {
    CAPTION_BANK_PC + 0x12 + group * 10
}

/// Localizes the easy-course captions and Nomi's markers on an already verified
/// derivative. The caller recomputes the checksum.
pub fn build_after_verified_patch(source: &[u8], inputs: &Inputs<'_>) -> Result<(Vec<u8>, Report)> {
    verify_font(inputs.galmuri_ttf_data, GALMURI_SHA256, "Galmuri11")?;
    verify_font(
        inputs.galmuri_bold_ttf_data,
        GALMURI_BOLD_SHA256,
        "Galmuri11 Bold",
    )?;
    let galmuri = parse_font(inputs.galmuri_ttf_data)?;
    let galmuri_bold = parse_font(inputs.galmuri_bold_ttf_data)?;

    let mut patched = source.to_vec();
    let (captions, caption_streams) = write_captions(source, &mut patched, &galmuri)?;
    let markers = vec![
        write_floor_marker(source, &mut patched, &galmuri_bold)?,
        write_icon_marker(source, &mut patched)?,
        write_portrait_marker(source, &mut patched)?,
    ];

    let diff_confined_to_registered_writes = source
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(offset, (before, after))| before == after || registered_write(offset).is_some());
    if !diff_confined_to_registered_writes {
        bail!("caption and marker writes escaped their registered ranges");
    }
    Ok((
        patched,
        Report {
            verdict: "easy-course 1/2 captions, course-1 poke lettering, and Nomi floor, icon, and portrait markers localized"
                .to_owned(),
            captions,
            caption_streams,
            markers,
            diff_confined_to_registered_writes,
        },
    ))
}

fn write_captions(
    source: &[u8],
    patched: &mut [u8],
    font: &fontdue::Font,
) -> Result<(Vec<CaptionReport>, Vec<RewrittenStream>)> {
    for (index, stream) in CAPTION_STREAMS.iter().enumerate() {
        let raw = source
            .get(stream.pc..stream.pc + stream.len)
            .context("caption stream outside ROM")?;
        if sha256(raw) != stream.sha256 {
            bail!(
                "caption stream at PC 0x{:06X} differs from Remix",
                stream.pc
            );
        }
        let record = caption_record_pc(CAPTION_FIRST_GROUP + index);
        let (_, address) = crate::rom::pc_to_lorom(stream.pc);
        if read_word(source, record + 6)? != address || read_word(source, record + 8)? != 0xFFFF {
            bail!("caption record at PC 0x{record:06X} no longer points at its stream");
        }
    }
    if CAPTION_STREAMS[0].pc != CAPTION_REGION.start
        || CAPTION_STREAMS
            .windows(2)
            .any(|pair| pair[0].pc + pair[0].len != pair[1].pc)
        || CAPTION_STREAMS[6].pc + CAPTION_STREAMS[6].len != CAPTION_REGION.end
    {
        bail!("caption streams are not contiguous inside the caption region");
    }

    let mut rewritten: Vec<Option<Vec<u8>>> = vec![None; CAPTION_STREAMS.len()];
    let mut reports = Vec::new();
    for course in &COURSES {
        let chr_stream = &CAPTION_STREAMS[course.chr_stream];
        let map_stream = &CAPTION_STREAMS[course.map_stream];
        let chr = crate::snes_lz::decompress(source, chr_stream.pc)?.bytes;
        let map = crate::snes_lz::decompress(source, map_stream.pc)?.bytes;
        let (new_chr, new_map, report) = localize_caption(course, &chr, &map, font)?;
        rewritten[course.chr_stream] = Some(compress_verified(&new_chr)?);
        rewritten[course.map_stream] = Some(compress_verified(&new_map)?);
        reports.push(report);
    }

    let mut streams = Vec::new();
    for (index, stream) in CAPTION_STREAMS.iter().enumerate() {
        let Some(bytes) = &rewritten[index] else {
            continue;
        };
        if bytes.len() > stream.len {
            bail!(
                "caption group {} recompressed to {} bytes, over its {}-byte slot",
                CAPTION_FIRST_GROUP + index,
                bytes.len(),
                stream.len
            );
        }
        let slot = &mut patched[stream.pc..stream.pc + stream.len];
        slot.fill(0xFF);
        slot[..bytes.len()].copy_from_slice(bytes);
        if crate::snes_lz::decompress(patched, stream.pc)?.bytes
            != crate::snes_lz::decompress(bytes, 0)?.bytes
        {
            bail!(
                "rewritten caption group {} does not decode back",
                CAPTION_FIRST_GROUP + index
            );
        }
        streams.push(RewrittenStream {
            group: CAPTION_FIRST_GROUP + index,
            pc: format!("0x{:06X}", stream.pc),
            original_len: stream.len,
            rewritten_len: bytes.len(),
        });
    }
    Ok((reports, streams))
}

fn localize_caption(
    course: &CaptionCourse,
    chr: &[u8],
    map: &[u8],
    font: &fontdue::Font,
) -> Result<(Vec<u8>, Vec<u8>, CaptionReport)> {
    if !chr.len().is_multiple_of(32) || !map.len().is_multiple_of(MAP_COLUMNS * 2) {
        bail!("{} caption streams have unexpected sizes", course.name);
    }
    let mut tiles = chr
        .as_chunks::<32>()
        .0
        .iter()
        .map(decode_tile)
        .collect::<Vec<_>>();
    let mut words = map
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect::<Vec<_>>();
    if tiles[0] != [0; 64] {
        bail!("{} tile 0 is not the blank tile", course.name);
    }
    let rows = words.len() / MAP_COLUMNS;
    let caption_rows = CAPTION_LINE_ROWS
        .iter()
        .flat_map(|row| [*row, row + 1])
        .collect::<Vec<_>>();
    if caption_rows.iter().any(|row| *row >= rows) {
        bail!("{} tilemap is shorter than the caption rows", course.name);
    }

    let mut caption_tiles = std::collections::BTreeSet::new();
    let mut other_tiles = std::collections::BTreeSet::new();
    for (index, word) in words.iter().enumerate() {
        let tile = usize::from(word & 0x03FF);
        if tile == 0 {
            continue;
        }
        // Caption cells may mirror a kana tile; every caption cell is rewritten.
        if caption_rows.contains(&(index / MAP_COLUMNS)) {
            caption_tiles.insert(tile);
        } else {
            other_tiles.insert(tile);
        }
    }
    if caption_tiles.iter().any(|tile| other_tiles.contains(tile)) {
        bail!(
            "{} caption tiles are shared with the illustration",
            course.name
        );
    }
    if course.poke && POKE_TILES.iter().any(|tile| caption_tiles.contains(tile)) {
        bail!("{} poke lettering overlaps the caption tiles", course.name);
    }

    for row in &caption_rows {
        for column in 0..MAP_COLUMNS {
            words[row * MAP_COLUMNS + column] = 0;
        }
    }
    let mut cells = Vec::new();
    for (line, row) in course.lines.iter().zip(CAPTION_LINE_ROWS) {
        let mask = render_text(font, line, CAPTION_FONT_PX)?;
        let strip_width = mask.width + 2;
        let x0 = course.start_column * 8;
        if x0 + strip_width > CAPTION_RIGHT_EDGE_PX || mask.height + 2 > 16 {
            bail!(
                "{} caption line {line:?} ({}x{}) does not fit",
                course.name,
                mask.width,
                mask.height
            );
        }
        let columns = (x0 + strip_width).div_ceil(8) - course.start_column;
        let mut strip = vec![0u8; columns * 8 * 16];
        let stride = columns * 8;
        let top = (16 - (mask.height + 2)) / 2;
        paint_outlined(
            &mut strip,
            stride,
            1,
            top + 1,
            &mask,
            |_| CAPTION_STROKE,
            CAPTION_OUTLINE,
        );
        for cell_row in 0..2 {
            for cell_column in 0..columns {
                let mut pixels = [0u8; 64];
                for y in 0..8 {
                    for x in 0..8 {
                        pixels[y * 8 + x] =
                            strip[(cell_row * 8 + y) * stride + cell_column * 8 + x];
                    }
                }
                if pixels != [0; 64] {
                    cells.push((
                        (row + cell_row) * MAP_COLUMNS + course.start_column + cell_column,
                        pixels,
                    ));
                }
            }
        }
    }

    let mut free = caption_tiles.iter().copied().collect::<Vec<_>>();
    free.reverse();
    let mut assigned: Vec<([u8; 64], usize)> = Vec::new();
    let mut appended_tiles = 0;
    for (cell, pixels) in &cells {
        let tile = if let Some((_, tile)) = assigned.iter().find(|(existing, _)| existing == pixels)
        {
            *tile
        } else {
            let tile = if let Some(tile) = free.pop() {
                tiles[tile] = *pixels;
                tile
            } else {
                tiles.push(*pixels);
                appended_tiles += 1;
                tiles.len() - 1
            };
            assigned.push((*pixels, tile));
            tile
        };
        words[*cell] = u16::try_from(tile).context("caption tile index overflow")?;
    }
    for tile in free {
        tiles[tile] = [0; 64];
    }
    if tiles.len() > 0x3FF {
        bail!(
            "{} caption CHR exceeds the tilemap index range",
            course.name
        );
    }

    let mut poke_text = None;
    if course.poke {
        let mask = render_text(font, POKE_TEXT, CAPTION_FONT_PX)?;
        if mask.width + 2 > 24 || mask.height + 2 > 16 {
            bail!("poke lettering does not fit its 24x16 block");
        }
        let mut block = vec![0u8; 24 * 16];
        paint_outlined(
            &mut block,
            24,
            (24 - (mask.width + 2)) / 2 + 1,
            (16 - (mask.height + 2)) / 2 + 1,
            &mask,
            |_| POKE_FILL,
            POKE_OUTLINE,
        );
        for (position, tile) in POKE_TILES.iter().enumerate() {
            let (block_row, block_column) = (position / 3, position % 3);
            let mut pixels = [0u8; 64];
            for y in 0..8 {
                for x in 0..8 {
                    pixels[y * 8 + x] = block[(block_row * 8 + y) * 24 + block_column * 8 + x];
                }
            }
            tiles[*tile] = pixels;
        }
        poke_text = Some(POKE_TEXT.to_owned());
    }

    let new_chr = tiles.iter().flat_map(encode_tile).collect::<Vec<_>>();
    let new_map = words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    Ok((
        new_chr,
        new_map,
        CaptionReport {
            course: course.name.to_owned(),
            lines: course.lines.iter().map(|line| (*line).to_owned()).collect(),
            poke_text,
            caption_tiles_before: caption_tiles.len(),
            caption_tiles_after: assigned.len(),
            appended_tiles,
            chr_tiles_before: chr.len() / 32,
            chr_tiles_after: tiles.len(),
        },
    ))
}

fn write_floor_marker(
    source: &[u8],
    patched: &mut [u8],
    font: &fontdue::Font,
) -> Result<MarkerReport> {
    let mut tiles = read_in_place_tiles(source, &FLOOR_STREAM)?;
    let (width, height) = (32, 24);
    let mut canvas = vec![0u8; width * height];
    for (line, text) in FLOOR_LINES.iter().enumerate() {
        let mask = render_text(font, text, FLOOR_FONT_PX)?;
        if mask.width + 2 > width || mask.height + 2 > 12 {
            bail!("floor marker line {text:?} does not fit 32x12");
        }
        let height = mask.height;
        paint_outlined(
            &mut canvas,
            width,
            (width - (mask.width + 2)) / 2 + 1,
            line * 12 + (12 - (mask.height + 2)) / 2 + 1,
            &mask,
            |y| shade(&FLOOR_SHADES, y, height),
            FLOOR_OUTLINE,
        );
    }
    let changed = blit_tiles(
        &mut tiles,
        &FLOOR_TILES.map(|row| row.to_vec()),
        &canvas,
        width,
    )?;
    write_in_place(patched, &FLOOR_STREAM, &tiles, "여기 / 있어", changed)
}

fn write_icon_marker(source: &[u8], patched: &mut [u8]) -> Result<MarkerReport> {
    let mut tiles = read_in_place_tiles(source, &ICON_STREAM)?;
    let layout = ICON_TILES.map(|row| row.to_vec());
    let width = 24;
    let mut canvas = gather_tiles(&tiles, &layout, width);
    for y in ICON_TEXT_ROWS {
        for x in ICON_TEXT_COLUMNS {
            canvas[y * width + x] = 0;
        }
    }
    for x in ICON_TEXT_COLUMNS {
        if !ICON_ARROW_COLUMNS.contains(&x) {
            canvas[ICON_ARROW_ROW * width + x] = 0;
        }
    }
    let mask = small_text(ICON_TEXT)?;
    if mask.width + 2 > ICON_TEXT_COLUMNS.len() || mask.height + 2 > ICON_TEXT_ROWS.len() {
        bail!("icon lettering does not fit the icon frame");
    }
    let height = mask.height;
    paint_outlined(
        &mut canvas,
        width,
        ICON_TEXT_COLUMNS.start + (ICON_TEXT_COLUMNS.len() - (mask.width + 2)) / 2 + 1,
        ICON_TEXT_ROWS.start + (ICON_TEXT_ROWS.len() - (mask.height + 2)) / 2 + 1,
        &mask,
        |y| shade(&ICON_SHADES, y, height),
        ICON_OUTLINE,
    );
    let changed = blit_tiles(&mut tiles, &layout, &canvas, width)?;
    write_in_place(patched, &ICON_STREAM, &tiles, ICON_TEXT, changed)
}

fn write_portrait_marker(source: &[u8], patched: &mut [u8]) -> Result<MarkerReport> {
    let mut tiles = read_in_place_tiles(source, &PORTRAIT_STREAM)?;
    let width = PORTRAIT_TILES.len() * 8;
    let mut canvas = vec![0u8; width * 8];
    let mask = small_text(PORTRAIT_TEXT)?;
    if mask.width > width || PORTRAIT_TOP + mask.height > 8 {
        bail!("portrait lettering does not fit 48x8");
    }
    let x0 = (width - mask.width) / 2;
    for y in 0..mask.height {
        for x in 0..mask.width {
            if mask.pixels[y * mask.width + x] {
                canvas[(PORTRAIT_TOP + y) * width + x0 + x] = PORTRAIT_STROKE;
            }
        }
    }
    let changed = blit_tiles(&mut tiles, &[PORTRAIT_TILES.to_vec()], &canvas, width)?;
    write_in_place(patched, &PORTRAIT_STREAM, &tiles, PORTRAIT_TEXT, changed)
}

fn read_in_place_tiles(source: &[u8], stream: &InPlaceStream) -> Result<Vec<[u8; 64]>> {
    let raw = source
        .get(stream.range())
        .with_context(|| format!("{} stream outside ROM", stream.name))?;
    if sha256(raw) != stream.sha256 {
        bail!("{} stream differs from Remix", stream.name);
    }
    let block = crate::snes_lz::decompress(source, stream.pc)?;
    if block.compressed_len != stream.len || block.bytes.len() != stream.decoded_len {
        bail!("{} stream boundary differs", stream.name);
    }
    Ok(block
        .bytes
        .as_chunks::<32>()
        .0
        .iter()
        .map(decode_tile)
        .collect())
}

fn write_in_place(
    patched: &mut [u8],
    stream: &InPlaceStream,
    tiles: &[[u8; 64]],
    text: &str,
    changed_tiles: usize,
) -> Result<MarkerReport> {
    let decoded = tiles.iter().flat_map(encode_tile).collect::<Vec<_>>();
    if decoded.len() != stream.decoded_len {
        bail!("{} tile count changed", stream.name);
    }
    let compressed = compress_verified(&decoded)?;
    if compressed.len() > stream.len {
        bail!(
            "{} recompressed to {} bytes, over its {}-byte slot",
            stream.name,
            compressed.len(),
            stream.len
        );
    }
    let slot = &mut patched[stream.range()];
    slot.fill(0xFF);
    slot[..compressed.len()].copy_from_slice(&compressed);
    Ok(MarkerReport {
        surface: stream.name.to_owned(),
        text: text.to_owned(),
        stream_pc: format!("0x{:06X}", stream.pc),
        original_len: stream.len,
        rewritten_len: compressed.len(),
        changed_tiles,
    })
}

fn gather_tiles(tiles: &[[u8; 64]], layout: &[Vec<usize>], width: usize) -> Vec<u8> {
    let mut canvas = vec![0u8; width * layout.len() * 8];
    for (row, tile_row) in layout.iter().enumerate() {
        for (column, tile) in tile_row.iter().enumerate() {
            for y in 0..8 {
                for x in 0..8 {
                    canvas[(row * 8 + y) * width + column * 8 + x] = tiles[*tile][y * 8 + x];
                }
            }
        }
    }
    canvas
}

fn blit_tiles(
    tiles: &mut [[u8; 64]],
    layout: &[Vec<usize>],
    canvas: &[u8],
    width: usize,
) -> Result<usize> {
    let mut changed = 0;
    for (row, tile_row) in layout.iter().enumerate() {
        for (column, tile) in tile_row.iter().enumerate() {
            let target = tiles
                .get_mut(*tile)
                .with_context(|| format!("tile {tile} outside stream"))?;
            let mut pixels = [0u8; 64];
            for y in 0..8 {
                for x in 0..8 {
                    pixels[y * 8 + x] = canvas[(row * 8 + y) * width + column * 8 + x];
                }
            }
            if *target != pixels {
                changed += 1;
                *target = pixels;
            }
        }
    }
    Ok(changed)
}

struct Mask {
    width: usize,
    height: usize,
    pixels: Vec<bool>,
}

/// Renders one line at the font's pixel grid and crops it to its ink box.
fn render_text(font: &fontdue::Font, text: &str, px: f32) -> Result<Mask> {
    let ascent = font
        .horizontal_line_metrics(px)
        .context("TTF has no horizontal line metrics")?
        .ascent
        .round() as i32;
    let height = (px * 2.0).ceil() as usize + 4;
    let mut pen = 0i32;
    let mut glyphs = Vec::new();
    for character in text.chars() {
        let (metrics, raster) = font.rasterize(character, px);
        if character != ' ' && !raster.iter().any(|coverage| *coverage >= 128) {
            bail!("TTF produced an empty glyph for {character:?} at {px}px");
        }
        glyphs.push((pen + metrics.xmin, metrics, raster));
        pen += metrics.advance_width.round() as i32;
    }
    let width = (pen.max(1) as usize) + 8;
    let mut canvas = vec![false; width * height];
    for (x0, metrics, raster) in &glyphs {
        let top = ascent - metrics.ymin - metrics.height as i32 + 2;
        for row in 0..metrics.height {
            for column in 0..metrics.width {
                if raster[row * metrics.width + column] < 128 {
                    continue;
                }
                let x = x0 + column as i32 + 4;
                let y = top + row as i32;
                if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                    bail!("glyph ink escaped the line canvas for {text:?}");
                }
                canvas[y as usize * width + x as usize] = true;
            }
        }
    }
    let ink = |x: usize, y: usize| canvas[y * width + x];
    let columns = (0..width)
        .filter(|x| (0..height).any(|y| ink(*x, y)))
        .collect::<Vec<_>>();
    let rows = (0..height)
        .filter(|y| (0..width).any(|x| ink(x, *y)))
        .collect::<Vec<_>>();
    let (Some(&left), Some(&right), Some(&top), Some(&bottom)) =
        (columns.first(), columns.last(), rows.first(), rows.last())
    else {
        bail!("text {text:?} rendered no ink");
    };
    let (mask_width, mask_height) = (right - left + 1, bottom - top + 1);
    let mut pixels = vec![false; mask_width * mask_height];
    for y in 0..mask_height {
        for x in 0..mask_width {
            pixels[y * mask_width + x] = ink(left + x, top + y);
        }
    }
    Ok(Mask {
        width: mask_width,
        height: mask_height,
        pixels,
    })
}

/// Builds a mask from the hand-drawn 7x7 syllables with one blank column between them.
fn small_text(text: &str) -> Result<Mask> {
    let glyphs = text
        .chars()
        .map(|character| {
            SMALL_GLYPHS
                .iter()
                .find(|(glyph, _)| *glyph == character)
                .map(|(_, rows)| rows)
                .with_context(|| format!("no hand-drawn glyph for {character:?}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let width = glyphs.len() * 8 - 1;
    let mut pixels = vec![false; width * 7];
    for (index, rows) in glyphs.iter().enumerate() {
        for (y, row) in rows.iter().enumerate() {
            for (x, cell) in row.bytes().enumerate() {
                pixels[y * width + index * 8 + x] = cell == b'#';
            }
        }
    }
    Ok(Mask {
        width,
        height: 7,
        pixels,
    })
}

/// Draws the mask at (x0, y0) with `fill(row)` and an eight-neighbour outline.
fn paint_outlined(
    canvas: &mut [u8],
    stride: usize,
    x0: usize,
    y0: usize,
    mask: &Mask,
    fill: impl Fn(usize) -> u8,
    outline: u8,
) {
    for y in 0..mask.height {
        for x in 0..mask.width {
            if !mask.pixels[y * mask.width + x] {
                continue;
            }
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let cx = (x0 + x) as i32 + dx;
                    let cy = (y0 + y) as i32 + dy;
                    let index = cy as usize * stride + cx as usize;
                    let inside = dx == 0 && dy == 0;
                    let mx = x as i32 + dx;
                    let my = y as i32 + dy;
                    let covered = mx >= 0
                        && my >= 0
                        && (mx as usize) < mask.width
                        && (my as usize) < mask.height
                        && mask.pixels[my as usize * mask.width + mx as usize];
                    if inside {
                        canvas[index] = fill(y);
                    } else if !covered && canvas[index] == 0 {
                        canvas[index] = outline;
                    }
                }
            }
        }
    }
}

fn shade(shades: &[u8], row: usize, height: usize) -> u8 {
    let index = row * shades.len() / height.max(1);
    shades[index.min(shades.len() - 1)]
}

fn decode_tile(tile: &[u8; 32]) -> [u8; 64] {
    let mut pixels = [0u8; 64];
    for y in 0..8 {
        let planes = [
            tile[2 * y],
            tile[2 * y + 1],
            tile[16 + 2 * y],
            tile[17 + 2 * y],
        ];
        for x in 0..8 {
            let bit = 7 - x;
            pixels[y * 8 + x] = planes
                .iter()
                .enumerate()
                .map(|(plane, byte)| ((byte >> bit) & 1) << plane)
                .sum();
        }
    }
    pixels
}

fn encode_tile(pixels: &[u8; 64]) -> [u8; 32] {
    let mut tile = [0u8; 32];
    for y in 0..8 {
        for x in 0..8 {
            let value = pixels[y * 8 + x];
            let bit = 7 - x;
            tile[2 * y] |= (value & 1) << bit;
            tile[2 * y + 1] |= ((value >> 1) & 1) << bit;
            tile[16 + 2 * y] |= ((value >> 2) & 1) << bit;
            tile[17 + 2 * y] |= ((value >> 3) & 1) << bit;
        }
    }
    tile
}

fn compress_verified(decoded: &[u8]) -> Result<Vec<u8>> {
    let compressed = crate::snes_lz::compress(decoded);
    let block = crate::snes_lz::decompress(&compressed, 0)?;
    if block.bytes != decoded || block.compressed_len != compressed.len() {
        bail!("recompressed stream does not round-trip");
    }
    Ok(compressed)
}

fn verify_font(data: &[u8], expected: &str, name: &str) -> Result<()> {
    if sha256(data) != expected {
        bail!("{name} TTF differs from the committed font contract");
    }
    Ok(())
}

fn parse_font(data: &[u8]) -> Result<fontdue::Font> {
    fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
        .map_err(|error| anyhow::anyhow!("failed to parse TTF: {error}"))
}

fn read_word(data: &[u8], pc: usize) -> Result<u16> {
    let bytes = data.get(pc..pc + 2).context("word outside ROM")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_codec_round_trips_every_index() {
        let mut pixels = [0u8; 64];
        for (index, pixel) in pixels.iter_mut().enumerate() {
            *pixel = (index % 16) as u8;
        }
        assert_eq!(decode_tile(&encode_tile(&pixels)), pixels);
    }

    #[test]
    fn outline_surrounds_the_stroke_without_covering_it() {
        let mask = Mask {
            width: 1,
            height: 1,
            pixels: vec![true],
        };
        let mut canvas = vec![0u8; 9];
        paint_outlined(&mut canvas, 3, 1, 1, &mask, |_| 15, 3);
        assert_eq!(canvas, vec![3, 3, 3, 3, 15, 3, 3, 3, 3]);
    }

    #[test]
    fn caption_records_follow_the_one_record_group_layout() {
        assert_eq!(caption_record_pc(1), 0x13_801C);
        assert_eq!(caption_record_pc(7), 0x13_8058);
    }
}
