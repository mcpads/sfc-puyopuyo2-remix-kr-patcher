//! Small 65816 assembler for generated ROM hook code.
//!
//! This follows the sibling SNES projects' two-pass, label-based `Inst`
//! builder. Only the instruction subset currently needed by this project is
//! implemented; adding a hook instruction requires adding its encoding here
//! and a unit test rather than embedding opaque machine-code byte arrays.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Inst {
    Rep(u8),
    Sep(u8),
    LdaImm8(u8),
    LdaDp(u8),
    StaDp(u8),
    CmpImm8(u8),
    LdxImm16(u16),
    LdyImm16(u16),
    AndImm16(u16),
    OraImm16(u16),
    AdcImm16(u16),
    CpxImm16(u16),
    CmpStackRelative(u8),
    StaAbsY(u16),
    Pea(u16),
    Jsr(&'static str),
    Jsl(u32),
    Bne(&'static str),
    Phb,
    Plb,
    Phx,
    Plx,
    Pha,
    Pla,
    Txa,
    Tya,
    Tay,
    Inx,
    Iny,
    Clc,
    Rts,
    Rtl,
    Nop,
    Label(&'static str),
}

fn inst_size(inst: &Inst) -> usize {
    match inst {
        Inst::Rep(_)
        | Inst::Sep(_)
        | Inst::LdaImm8(_)
        | Inst::LdaDp(_)
        | Inst::StaDp(_)
        | Inst::CmpImm8(_)
        | Inst::CmpStackRelative(_)
        | Inst::Bne(_) => 2,
        Inst::LdxImm16(_)
        | Inst::LdyImm16(_)
        | Inst::AndImm16(_)
        | Inst::OraImm16(_)
        | Inst::AdcImm16(_)
        | Inst::CpxImm16(_)
        | Inst::StaAbsY(_)
        | Inst::Pea(_)
        | Inst::Jsr(_) => 3,
        Inst::Jsl(_) => 4,
        Inst::Phb
        | Inst::Plb
        | Inst::Phx
        | Inst::Plx
        | Inst::Pha
        | Inst::Pla
        | Inst::Txa
        | Inst::Tya
        | Inst::Tay
        | Inst::Inx
        | Inst::Iny
        | Inst::Clc
        | Inst::Rts
        | Inst::Rtl
        | Inst::Nop => 1,
        Inst::Label(_) => 0,
    }
}

/// Assemble at a 16-bit bank-local origin so `JSR label` can be resolved.
pub(crate) fn assemble_at(origin: u16, program: &[Inst]) -> Result<Vec<u8>, String> {
    let mut labels = HashMap::new();
    let mut offset = 0usize;
    for inst in program {
        if let Inst::Label(name) = inst
            && labels.insert(*name, offset).is_some()
        {
            return Err(format!("duplicate label: {name:?}"));
        }
        offset += inst_size(inst);
    }

    if usize::from(origin) + offset > 0x1_0000 {
        return Err(format!(
            "assembled program crosses bank boundary: ${origin:04X} + {offset} bytes"
        ));
    }

    let mut out = Vec::with_capacity(offset);
    let mut pc = 0usize;
    for inst in program {
        match inst {
            Inst::Rep(value) => out.extend_from_slice(&[0xC2, *value]),
            Inst::Sep(value) => out.extend_from_slice(&[0xE2, *value]),
            Inst::LdaImm8(value) => out.extend_from_slice(&[0xA9, *value]),
            Inst::LdaDp(address) => out.extend_from_slice(&[0xA5, *address]),
            Inst::StaDp(address) => out.extend_from_slice(&[0x85, *address]),
            Inst::CmpImm8(value) => out.extend_from_slice(&[0xC9, *value]),
            Inst::LdxImm16(value) => emit_u16(&mut out, 0xA2, *value),
            Inst::LdyImm16(value) => emit_u16(&mut out, 0xA0, *value),
            Inst::AndImm16(value) => emit_u16(&mut out, 0x29, *value),
            Inst::OraImm16(value) => emit_u16(&mut out, 0x09, *value),
            Inst::AdcImm16(value) => emit_u16(&mut out, 0x69, *value),
            Inst::CpxImm16(value) => emit_u16(&mut out, 0xE0, *value),
            Inst::CmpStackRelative(value) => out.extend_from_slice(&[0xC3, *value]),
            Inst::StaAbsY(address) => emit_u16(&mut out, 0x99, *address),
            Inst::Pea(value) => emit_u16(&mut out, 0xF4, *value),
            Inst::Jsr(label) => {
                let target = label_address(origin, &labels, label)?;
                emit_u16(&mut out, 0x20, target);
            }
            Inst::Jsl(address) => out.extend_from_slice(&[
                0x22,
                *address as u8,
                (*address >> 8) as u8,
                (*address >> 16) as u8,
            ]),
            Inst::Bne(label) => {
                let target = *labels
                    .get(label)
                    .ok_or_else(|| format!("undefined label: {label:?}"))?;
                let next_pc = pc + 2;
                let relative = target as isize - next_pc as isize;
                if !(-128..=127).contains(&relative) {
                    return Err(format!(
                        "branch to {label:?} out of range: {relative} (must be -128..127)"
                    ));
                }
                let opcode = match inst {
                    Inst::Bne(_) => 0xD0,
                    _ => unreachable!(),
                };
                out.extend_from_slice(&[opcode, relative as i8 as u8]);
            }
            Inst::Phb => out.push(0x8B),
            Inst::Plb => out.push(0xAB),
            Inst::Phx => out.push(0xDA),
            Inst::Plx => out.push(0xFA),
            Inst::Pha => out.push(0x48),
            Inst::Pla => out.push(0x68),
            Inst::Txa => out.push(0x8A),
            Inst::Tya => out.push(0x98),
            Inst::Tay => out.push(0xA8),
            Inst::Inx => out.push(0xE8),
            Inst::Iny => out.push(0xC8),
            Inst::Clc => out.push(0x18),
            Inst::Rts => out.push(0x60),
            Inst::Rtl => out.push(0x6B),
            Inst::Nop => out.push(0xEA),
            Inst::Label(_) => {}
        }
        pc += inst_size(inst);
    }
    Ok(out)
}

pub(crate) fn assemble(program: &[Inst]) -> Result<Vec<u8>, String> {
    assemble_at(0, program)
}

fn emit_u16(out: &mut Vec<u8>, opcode: u8, value: u16) {
    out.extend_from_slice(&[opcode, value as u8, (value >> 8) as u8]);
}

fn label_address(origin: u16, labels: &HashMap<&str, usize>, label: &str) -> Result<u16, String> {
    let offset = labels
        .get(label)
        .ok_or_else(|| format!("undefined label: {label:?}"))?;
    u16::try_from(usize::from(origin) + offset)
        .map_err(|_| format!("label {label:?} crosses bank boundary"))
}

#[cfg(test)]
mod tests {
    use super::{Inst::*, *};

    #[test]
    fn resolves_relative_and_absolute_labels() {
        let code = assemble_at(
            0xD000,
            &[
                Jsr("sub"),
                Bne("done"),
                Label("sub"),
                Rts,
                Label("done"),
                Rtl,
            ],
        )
        .unwrap();
        assert_eq!(code, [0x20, 0x05, 0xD0, 0xD0, 0x01, 0x60, 0x6B]);
    }

    #[test]
    fn encodes_menu_hook_instruction_subset() {
        let code = assemble(&[
            Sep(0x20),
            Rep(0x20),
            Pea(0x1234),
            LdxImm16(0x5678),
            LdyImm16(0x9ABC),
            CmpStackRelative(5),
            StaAbsY(0x4000),
            Jsl(0xAC_D000),
            Nop,
        ])
        .unwrap();
        assert_eq!(
            code,
            [
                0xE2, 0x20, 0xC2, 0x20, 0xF4, 0x34, 0x12, 0xA2, 0x78, 0x56, 0xA0, 0xBC, 0x9A, 0xC3,
                0x05, 0x99, 0x00, 0x40, 0x22, 0x00, 0xD0, 0xAC, 0xEA,
            ]
        );
    }

    #[test]
    fn encodes_direct_page_and_8bit_compare() {
        let code = assemble(&[LdaDp(0x6A), CmpImm8(0x87), StaDp(0x68)]).unwrap();
        assert_eq!(code, [0xA5, 0x6A, 0xC9, 0x87, 0x85, 0x68]);
    }

    #[test]
    fn rejects_invalid_control_flow() {
        assert!(
            assemble(&[Bne("missing")])
                .unwrap_err()
                .contains("undefined")
        );
        assert!(
            assemble(&[Label("same"), Label("same")])
                .unwrap_err()
                .contains("duplicate")
        );
        let mut too_far = vec![Bne("target")];
        too_far.extend(std::iter::repeat_n(Nop, 128));
        too_far.push(Label("target"));
        assert!(assemble(&too_far).unwrap_err().contains("out of range"));
    }
}
