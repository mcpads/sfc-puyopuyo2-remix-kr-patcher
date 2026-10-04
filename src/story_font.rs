use anyhow::{Context, Result, bail};

pub const GLYPH_LEN: usize = 32;
const GROUP_GLYPHS: usize = 8;
const HALF_LEN: usize = 16;
const GROUP_LEN: usize = GROUP_GLYPHS * GLYPH_LEN;

pub struct FontStream {
    pub bytes: Vec<u8>,
    pub compressed_len: usize,
}

pub fn decode_at(rom: &[u8], pc: usize) -> Result<FontStream> {
    let block = crate::snes_lz::decompress(rom, pc)?;
    if !block.bytes.len().is_multiple_of(GLYPH_LEN) {
        bail!("font stream at PC 0x{pc:06X} is not glyph-aligned");
    }
    Ok(FontStream {
        bytes: block.bytes,
        compressed_len: block.compressed_len,
    })
}

pub fn glyph_tiles(stream: &FontStream, glyph_index: usize) -> Result<[u8; GLYPH_LEN]> {
    let (top, bottom) = glyph_half_offsets(glyph_index);
    if bottom + HALF_LEN > stream.bytes.len() {
        bail!("glyph index {glyph_index} is outside the decoded font stream");
    }
    let mut glyph = [0; GLYPH_LEN];
    glyph[..HALF_LEN].copy_from_slice(&stream.bytes[top..top + HALF_LEN]);
    glyph[HALF_LEN..].copy_from_slice(&stream.bytes[bottom..bottom + HALF_LEN]);
    Ok(glyph)
}

pub fn replace_glyph(bytes: &mut [u8], glyph_index: usize, glyph: &[u8; GLYPH_LEN]) -> Result<()> {
    let (top, bottom) = glyph_half_offsets(glyph_index);
    if bottom + HALF_LEN > bytes.len() {
        bail!("glyph index {glyph_index} is outside the decoded font stream");
    }
    bytes[top..top + HALF_LEN].copy_from_slice(&glyph[..HALF_LEN]);
    bytes[bottom..bottom + HALF_LEN].copy_from_slice(&glyph[HALF_LEN..]);
    Ok(())
}

pub fn pack_glyph_stream(glyphs: &[[u8; GLYPH_LEN]], slot_count: usize) -> Result<Vec<u8>> {
    if !slot_count.is_multiple_of(GROUP_GLYPHS) {
        bail!("story font slot count must be group-aligned");
    }
    if glyphs.len() > slot_count {
        bail!(
            "{} glyphs exceed {slot_count} story font slots",
            glyphs.len()
        );
    }
    let mut bytes = vec![0; slot_count * GLYPH_LEN];
    for (glyph_index, glyph) in glyphs.iter().enumerate() {
        let (top, bottom) = glyph_half_offsets(glyph_index);
        bytes[top..top + HALF_LEN].copy_from_slice(&glyph[..HALF_LEN]);
        bytes[bottom..bottom + HALF_LEN].copy_from_slice(&glyph[HALF_LEN..]);
    }
    Ok(bytes)
}

pub fn source_pc(rom: &[u8], pointer_pc: usize, bank: u8) -> Result<usize> {
    let raw = rom
        .get(pointer_pc..pointer_pc + 2)
        .with_context(|| format!("font source pointer at PC 0x{pointer_pc:06X} is outside ROM"))?;
    let address = u16::from_le_bytes([raw[0], raw[1]]);
    crate::rom::lorom_to_pc(bank, address)
        .with_context(|| format!("font source ${bank:02X}:${address:04X} is not mapped"))
}

fn glyph_half_offsets(glyph_index: usize) -> (usize, usize) {
    let group = glyph_index / GROUP_GLYPHS;
    let in_group = glyph_index % GROUP_GLYPHS;
    let top = group * GROUP_LEN + in_group * HALF_LEN;
    (top, top + GROUP_GLYPHS * HALF_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_reads_grouped_glyphs() {
        let mut first = [0; GLYPH_LEN];
        first[0] = 0x80;
        first[16] = 0x40;
        let bytes = pack_glyph_stream(&[first], 8).unwrap();
        let stream = FontStream {
            bytes,
            compressed_len: 0,
        };
        assert_eq!(glyph_tiles(&stream, 0).unwrap(), first);
    }

    #[test]
    fn replaces_one_grouped_glyph_without_touching_its_neighbor() {
        let first = [0x11; GLYPH_LEN];
        let second = [0x22; GLYPH_LEN];
        let mut bytes = pack_glyph_stream(&[first, second], 8).unwrap();
        let replacement = [0x33; GLYPH_LEN];
        replace_glyph(&mut bytes, 0, &replacement).unwrap();
        let stream = FontStream {
            bytes,
            compressed_len: 0,
        };
        assert_eq!(glyph_tiles(&stream, 0).unwrap(), replacement);
        assert_eq!(glyph_tiles(&stream, 1).unwrap(), second);
    }
}
