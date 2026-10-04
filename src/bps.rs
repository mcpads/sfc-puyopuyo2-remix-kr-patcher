//! Deterministic BPS generation and an in-process apply verifier.
//!
//! The generator intentionally emits only `SourceRead` and `TargetRead`
//! actions.  This is less compact than a copy-searching encoder, but keeps the
//! production build deterministic and easy to audit.

const MAGIC: &[u8; 4] = b"BPS1";

fn encode_vli(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte | 0x80);
            return;
        }
        out.push(byte);
        value -= 1;
    }
}

fn decode_vli(input: &[u8]) -> Result<(u64, usize), String> {
    let mut value = 0u64;
    let mut shift = 1u64;
    for (index, byte) in input.iter().copied().enumerate() {
        value = value
            .checked_add(
                u64::from(byte & 0x7f)
                    .checked_mul(shift)
                    .ok_or("BPS VLI overflow")?,
            )
            .ok_or("BPS VLI overflow")?;
        if byte & 0x80 != 0 {
            return Ok((value, index + 1));
        }
        shift = shift.checked_mul(128).ok_or("BPS VLI overflow")?;
        value = value.checked_add(shift).ok_or("BPS VLI overflow")?;
    }
    Err("truncated BPS VLI".to_owned())
}

pub fn generate(source: &[u8], target: &[u8]) -> Vec<u8> {
    let mut patch = Vec::new();
    patch.extend_from_slice(MAGIC);
    encode_vli(&mut patch, source.len() as u64);
    encode_vli(&mut patch, target.len() as u64);
    encode_vli(&mut patch, 0);

    let mut offset = 0usize;
    while offset < target.len() {
        if offset < source.len() && source[offset] == target[offset] {
            let start = offset;
            while offset < target.len() && offset < source.len() && source[offset] == target[offset]
            {
                offset += 1;
            }
            encode_vli(&mut patch, ((offset - start - 1) as u64) << 2);
        } else {
            let start = offset;
            while offset < target.len()
                && (offset >= source.len() || source[offset] != target[offset])
            {
                offset += 1;
            }
            encode_vli(&mut patch, (((offset - start - 1) as u64) << 2) | 1);
            patch.extend_from_slice(&target[start..offset]);
        }
    }

    patch.extend_from_slice(&crc32fast::hash(source).to_le_bytes());
    patch.extend_from_slice(&crc32fast::hash(target).to_le_bytes());
    let patch_crc = crc32fast::hash(&patch);
    patch.extend_from_slice(&patch_crc.to_le_bytes());
    patch
}

pub fn generate_verified(source: &[u8], target: &[u8]) -> Result<Vec<u8>, String> {
    let patch = generate(source, target);
    if apply(source, &patch)? != target {
        return Err("BPS apply round-trip did not reproduce the target".to_owned());
    }
    Ok(patch)
}

/// Apply a BPS patch.  All four standard action kinds are supported so this
/// verifier is not coupled to the two action kinds emitted by `generate`.
pub fn apply(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < 16 || patch.get(..4) != Some(MAGIC) {
        return Err("invalid BPS1 patch".to_owned());
    }
    let footer = patch.len() - 12;
    let stored_patch_crc = u32::from_le_bytes(patch[patch.len() - 4..].try_into().unwrap());
    if crc32fast::hash(&patch[..patch.len() - 4]) != stored_patch_crc {
        return Err("BPS patch CRC mismatch".to_owned());
    }

    let mut cursor = 4usize;
    let (source_len, used) = decode_vli(&patch[cursor..footer])?;
    cursor += used;
    let (target_len, used) = decode_vli(&patch[cursor..footer])?;
    cursor += used;
    let (metadata_len, used) = decode_vli(&patch[cursor..footer])?;
    cursor += used;
    cursor = cursor
        .checked_add(usize::try_from(metadata_len).map_err(|_| "metadata too large")?)
        .ok_or("metadata overflow")?;
    if cursor > footer || source_len != source.len() as u64 {
        return Err("BPS source size or metadata mismatch".to_owned());
    }
    let stored_source_crc = u32::from_le_bytes(patch[footer..footer + 4].try_into().unwrap());
    if crc32fast::hash(source) != stored_source_crc {
        return Err("BPS source CRC mismatch".to_owned());
    }

    let target_len = usize::try_from(target_len).map_err(|_| "target too large")?;
    let mut target = vec![0u8; target_len];
    let mut output = 0usize;
    let mut source_relative = 0i64;
    let mut target_relative = 0i64;

    while cursor < footer {
        let (header, used) = decode_vli(&patch[cursor..footer])?;
        cursor += used;
        let kind = header & 3;
        let len = usize::try_from((header >> 2) + 1).map_err(|_| "action too large")?;
        let end = output.checked_add(len).ok_or("output overflow")?;
        if end > target.len() {
            return Err("BPS action exceeds target".to_owned());
        }

        match kind {
            0 => {
                if end > source.len() {
                    return Err("BPS SourceRead exceeds source".to_owned());
                }
                target[output..end].copy_from_slice(&source[output..end]);
                output = end;
            }
            1 => {
                let patch_end = cursor.checked_add(len).ok_or("patch overflow")?;
                if patch_end > footer {
                    return Err("BPS TargetRead exceeds patch".to_owned());
                }
                target[output..end].copy_from_slice(&patch[cursor..patch_end]);
                cursor = patch_end;
                output = end;
            }
            2 | 3 => {
                let (encoded, used) = decode_vli(&patch[cursor..footer])?;
                cursor += used;
                let magnitude = i64::try_from(encoded >> 1).map_err(|_| "offset too large")?;
                let delta = if encoded & 1 == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                let relative = if kind == 2 {
                    source_relative = source_relative
                        .checked_add(delta)
                        .ok_or("offset overflow")?;
                    &mut source_relative
                } else {
                    target_relative = target_relative
                        .checked_add(delta)
                        .ok_or("offset overflow")?;
                    &mut target_relative
                };
                for _ in 0..len {
                    let index = usize::try_from(*relative).map_err(|_| "negative copy offset")?;
                    let value = if kind == 2 {
                        *source.get(index).ok_or("SourceCopy exceeds source")?
                    } else {
                        if index >= output {
                            return Err("TargetCopy references unwritten output".to_owned());
                        }
                        target[index]
                    };
                    target[output] = value;
                    output += 1;
                    *relative = relative.checked_add(1).ok_or("offset overflow")?;
                }
            }
            _ => unreachable!(),
        }
    }

    if output != target.len() {
        return Err("BPS output size mismatch".to_owned());
    }
    let stored_target_crc = u32::from_le_bytes(patch[footer + 4..footer + 8].try_into().unwrap());
    if crc32fast::hash(&target) != stored_target_crc {
        return Err("BPS target CRC mismatch".to_owned());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_patch_round_trips_and_checks_crcs() {
        let source = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let target = b"0123KOREAN89abcxyzghijklmnopqrstuvwxyz!";
        let patch = generate_verified(source, target).unwrap();
        assert_eq!(apply(source, &patch).unwrap(), target);

        let mut corrupt = patch;
        corrupt[8] ^= 1;
        assert!(apply(source, &corrupt).is_err());
    }
}
