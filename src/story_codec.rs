use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryToken {
    Glyph { code: u16 },
    Control { code: u8, args: Vec<u8> },
}

#[derive(Debug)]
pub struct ParsedStory {
    pub consumed_len: usize,
    pub tokens: Vec<StoryToken>,
}

#[derive(Debug, Clone, Copy)]
pub struct StoryLayout {
    pub max_line_characters: usize,
    pub shifted_columns: usize,
    pub changed: bool,
}

pub fn parse(data: &[u8], start: usize) -> Result<ParsedStory> {
    let mut cursor = start;
    let mut tokens = Vec::new();
    loop {
        let byte = take(data, &mut cursor, start)?;
        match byte {
            0xFE => {
                let low = take(data, &mut cursor, start)?;
                tokens.push(StoryToken::Glyph {
                    code: 0x0100 | u16::from(low),
                });
            }
            0xFF => {
                let code = take(data, &mut cursor, start)?;
                if code == 0 {
                    tokens.push(StoryToken::Control {
                        code,
                        args: Vec::new(),
                    });
                    break;
                }
                let argument_len = match code {
                    0x01 | 0x02 | 0x04..=0x09 => 2,
                    0x03 => 4,
                    0x0A => 0,
                    _ => bail!(
                        "unknown story control FF{code:02X} at PC 0x{:06X}",
                        cursor - 2
                    ),
                };
                let end = cursor
                    .checked_add(argument_len)
                    .context("story control argument length overflow")?;
                let args = data
                    .get(cursor..end)
                    .context("truncated story control arguments")?
                    .to_vec();
                tokens.push(StoryToken::Control { code, args });
                cursor = end;
            }
            code => tokens.push(StoryToken::Glyph {
                code: u16::from(code),
            }),
        }
    }
    Ok(ParsedStory {
        consumed_len: cursor - start,
        tokens,
    })
}

pub fn visible_characters(text: &str) -> Result<Vec<char>> {
    let mut visible = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '{' => {
                let mut closed = false;
                for token_character in characters.by_ref() {
                    if token_character == '}' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    bail!("unterminated story control token");
                }
            }
            '}' => bail!("unmatched closing brace in story text"),
            _ => visible.push(character),
        }
    }
    Ok(visible)
}

pub fn encode_work_text(text: &str, codes: &BTreeMap<char, u16>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut characters = text.chars().peekable();
    let mut ended = false;
    while let Some(character) = characters.next() {
        if ended {
            bail!("story content found after END token");
        }
        if character == '{' {
            let mut token = String::new();
            let mut closed = false;
            for token_character in characters.by_ref() {
                if token_character == '}' {
                    closed = true;
                    break;
                }
                token.push(token_character);
            }
            if !closed {
                bail!("unterminated story control token");
            }
            if token == "END" {
                bytes.extend_from_slice(&[0xFF, 0x00]);
                ended = true;
                continue;
            }
            let (control, arguments) = token
                .split_once(':')
                .with_context(|| format!("invalid story control token {{{token}}}"))?;
            let code = control
                .strip_prefix("FF")
                .filter(|hex| hex.len() == 2)
                .with_context(|| format!("invalid story control token {{{token}}}"))?;
            bytes.extend_from_slice(&[0xFF, u8::from_str_radix(code, 16)?]);
            bytes.extend_from_slice(&decode_hex(arguments)?);
        } else if character == '}' {
            bail!("unmatched closing brace in story text");
        } else {
            let code = codes
                .get(&character)
                .with_context(|| format!("unmapped story character {character:?}"))?;
            match *code {
                0x0000..=0x00FD => bytes.push(*code as u8),
                0x0100..=0x01FF => bytes.extend_from_slice(&[0xFE, *code as u8]),
                _ => bail!("invalid story code 0x{code:04X}"),
            }
        }
    }
    if !ended {
        bail!("story work text has no END token");
    }
    Ok(bytes)
}

pub fn adjust_layout(bytes: &mut [u8], source_tokens: &[StoryToken]) -> Result<StoryLayout> {
    const RIGHT_EDGE_LIMIT: usize = 30;
    let parsed = parse(bytes, 0)?;
    let (line_count, max_line_characters) = text_dimensions(&parsed.tokens);
    if line_count == 0 || max_line_characters == 0 {
        bail!("story layout has no visible text line");
    }
    let desired_height = 2usize
        .checked_mul(line_count + 1)
        .context("story layout height overflow")?;
    let desired_width = 2usize
        .checked_mul(max_line_characters + 1)
        .context("story layout width overflow")?;
    if desired_height > u8::MAX as usize || desired_width > RIGHT_EDGE_LIMIT {
        bail!("story layout exceeds the verified 14-character screen limit");
    }

    let (source_lines, source_max) = text_dimensions(source_tokens);
    let source_layout_args = source_tokens
        .iter()
        .filter_map(|token| match token {
            StoryToken::Control { code: 0x03, args } => Some(args),
            _ => None,
        })
        .collect::<Vec<_>>();
    if source_layout_args.len() != 1
        || source_layout_args[0].len() != 4
        || usize::from(source_layout_args[0][2]) != 2 * (source_lines + 1)
        || usize::from(source_layout_args[0][3]) != 2 * (source_max + 1)
    {
        bail!("source FF03 dimensions do not match source text geometry");
    }

    let mut cursor = 0;
    let mut layout_offset = None;
    let mut line_offsets = Vec::new();
    for token in &parsed.tokens {
        match token {
            StoryToken::Control { code: 0x02, .. } => line_offsets.push(cursor),
            StoryToken::Control { code: 0x03, .. } if layout_offset.replace(cursor).is_some() => {
                bail!("encoded story contains multiple FF03 controls");
            }
            _ => {}
        }
        cursor += token_len(token);
    }
    let layout_offset = layout_offset.context("encoded story has no FF03 control")?;
    let original_args = bytes[layout_offset + 2..layout_offset + 6].to_vec();
    let mut display_offset = u16::from_le_bytes([original_args[0], original_args[1]]) as usize;
    if !display_offset.is_multiple_of(2) {
        bail!("story display offset is not word-aligned");
    }
    let column = (display_offset / 2) % 32;
    let shifted_columns = (column + desired_width).saturating_sub(RIGHT_EDGE_LIMIT);
    if shifted_columns > column {
        bail!("story box cannot shift far enough left");
    }
    display_offset -= shifted_columns * 2;
    let [low, high] = (display_offset as u16).to_le_bytes();
    let adjusted_args = [low, high, desired_height as u8, desired_width as u8];
    bytes[layout_offset + 2..layout_offset + 6].copy_from_slice(&adjusted_args);

    let line_shift = shifted_columns * 2;
    for offset in line_offsets {
        let args = &mut bytes[offset + 2..offset + 4];
        let position = u16::from_le_bytes([args[0], args[1]]) as usize;
        if position < line_shift {
            bail!("story line cannot shift far enough left");
        }
        args.copy_from_slice(&((position - line_shift) as u16).to_le_bytes());
    }
    Ok(StoryLayout {
        max_line_characters,
        shifted_columns,
        changed: original_args != adjusted_args,
    })
}

pub fn controls_match(source: &[StoryToken], encoded: &[StoryToken]) -> bool {
    let source = source
        .iter()
        .filter_map(|token| match token {
            StoryToken::Control { code, args } => Some((*code, args)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let encoded = encoded
        .iter()
        .filter_map(|token| match token {
            StoryToken::Control { code, args } => Some((*code, args)),
            _ => None,
        })
        .collect::<Vec<_>>();
    source.len() == encoded.len()
        && source.iter().zip(encoded).all(
            |((source_code, source_args), (encoded_code, encoded_args))| {
                source_code == &encoded_code
                    && (matches!(*source_code, 0x02 | 0x03) || *source_args == encoded_args)
            },
        )
}

fn text_dimensions(tokens: &[StoryToken]) -> (usize, usize) {
    let mut lines = 0;
    let mut width = 0;
    let mut max_width = 0;
    for token in tokens {
        match token {
            StoryToken::Control { code: 0x02, .. } => {
                if lines != 0 {
                    max_width = max_width.max(width);
                }
                lines += 1;
                width = 0;
            }
            StoryToken::Glyph { .. } => width += 1,
            _ => {}
        }
    }
    (lines, max_width.max(width))
}

fn token_len(token: &StoryToken) -> usize {
    match token {
        StoryToken::Glyph { code } if *code <= 0x00FD => 1,
        StoryToken::Glyph { .. } => 2,
        StoryToken::Control { args, .. } => 2 + args.len(),
    }
}

fn take(data: &[u8], cursor: &mut usize, start: usize) -> Result<u8> {
    let byte = *data
        .get(*cursor)
        .with_context(|| format!("unterminated story block at PC 0x{start:06X}"))?;
    *cursor += 1;
    Ok(byte)
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        bail!("hex byte string has odd length");
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(Into::into))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_text_round_trips_controls_and_extended_glyphs() {
        let codes = BTreeMap::from([('가', 0x0100), (' ', 0x00FC)]);
        let bytes = encode_work_text("{FF02:8603}가 가{END}", &codes).unwrap();
        let parsed = parse(&bytes, 0).unwrap();
        assert_eq!(parsed.consumed_len, bytes.len());
        assert!(matches!(
            parsed.tokens[1],
            StoryToken::Glyph { code: 0x0100 }
        ));
    }
}
