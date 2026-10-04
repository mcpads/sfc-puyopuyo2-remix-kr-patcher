use anyhow::{Context, Result, bail};

const MAX_LITERAL_LEN: usize = 127;
const MAX_BACKREF_LEN: usize = 130;
const MAX_BACKREF_OFFSET: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LzBlock {
    pub bytes: Vec<u8>,
    pub compressed_len: usize,
}

#[derive(Copy, Clone, Debug)]
enum Choice {
    End,
    Literal(usize),
    Backref { len: usize, offset: usize },
}

pub fn decompress(data: &[u8], start: usize) -> Result<LzBlock> {
    let mut out = Vec::new();
    let mut cursor = start;
    loop {
        let control = *data
            .get(cursor)
            .with_context(|| format!("compressed stream at 0x{start:06X} has no terminator"))?;
        cursor += 1;
        if control == 0 {
            return Ok(LzBlock {
                bytes: out,
                compressed_len: cursor - start,
            });
        }
        if control < 0x80 {
            let len = usize::from(control);
            let end = cursor + len;
            if end > data.len() {
                bail!("literal run at 0x{cursor:06X} extends past end of input");
            }
            out.extend_from_slice(&data[cursor..end]);
            cursor = end;
            continue;
        }
        let len = usize::from(control & 0x7F) + 3;
        let offset = usize::from(
            *data
                .get(cursor)
                .with_context(|| format!("backref at 0x{cursor:06X} is missing offset byte"))?,
        ) + 1;
        cursor += 1;
        if offset > out.len() {
            bail!(
                "backref offset {offset} is larger than output length {}",
                out.len()
            );
        }
        for _ in 0..len {
            let source = out.len() - offset;
            out.push(out[source]);
        }
    }
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut best = vec![usize::MAX / 4; n + 1];
    let mut choices = vec![Choice::End; n + 1];
    best[n] = 1;
    for pos in (0..n).rev() {
        for len in 1..=MAX_LITERAL_LEN.min(n - pos) {
            let cost = 1 + len + best[pos + len];
            if cost < best[pos] {
                best[pos] = cost;
                choices[pos] = Choice::Literal(len);
            }
        }
        for offset in 1..=MAX_BACKREF_OFFSET.min(pos) {
            let mut len = 0;
            while len < MAX_BACKREF_LEN
                && pos + len < n
                && data[pos + len] == data[pos + len - offset]
            {
                len += 1;
            }
            for use_len in 3..=len {
                let cost = 2 + best[pos + use_len];
                if cost < best[pos] {
                    best[pos] = cost;
                    choices[pos] = Choice::Backref {
                        len: use_len,
                        offset,
                    };
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < n {
        match choices[pos] {
            Choice::Literal(len) => {
                out.push(len as u8);
                out.extend_from_slice(&data[pos..pos + len]);
                pos += len;
            }
            Choice::Backref { len, offset } => {
                out.push(0x80 | ((len - 3) as u8));
                out.push((offset - 1) as u8);
                pos += len;
            }
            Choice::End => unreachable!("non-terminal position without compression choice"),
        }
    }
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_round_trips_literals_and_backrefs() {
        let data = b"PUYO-PUYO-PUYO-REMIX";
        let compressed = compress(data);
        assert_eq!(decompress(&compressed, 0).unwrap().bytes, data);
    }
}
