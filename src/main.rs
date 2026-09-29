// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io, path::Path, process::exit};

use serde::Serialize;
use winnow::{
    ascii::{line_ending, space1},
    combinator::{alt, cut_err, eof, opt, preceded, repeat, separated_pair, terminated},
    error::{ContextError, ErrMode},
    stream::AsChar,
    token::{take_till, take_while},
    {ModalResult, Parser},
};

use clap::Parser as ClapParser;

type FarAddress = (u16, u16);
type InstructionBytes = Vec<u8>;
type Disassembly<'s> = &'s str;
type FirstLineRegisters = (u16, u16, u16, u16, u16, u16, u16, u16);
type SecondLineRegisters = (u16, u16, u16, u16, u16);

#[derive(Serialize)]
struct Registers {
    ax: u16,
    bx: u16,
    cx: u16,
    dx: u16,
    sp: u16,
    bp: u16,
    si: u16,
    di: u16,
    ds: u16,
    es: u16,
    ss: u16,
    cs: u16,
    ip: u16,
}

#[derive(Serialize)]
struct Flags {
    overflow: bool,
    direction: bool,
    interrupt: bool,
    sign: bool,
    zero: bool,
    aux_carry: bool,
    parity: bool,
    carry: bool,
}

#[derive(Serialize)]
struct Instruction<'s> {
    address: FarAddress,
    bytes: InstructionBytes,
    disassembly: Disassembly<'s>,
}

#[derive(Serialize)]
struct TraceStep<'s> {
    registers: Registers,
    flags: Flags,
    instruction: Instruction<'s>,
}

#[derive(ClapParser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the DOSBox-X (8086 cpu) trace file
    src: String,
}

#[derive(Debug, Serialize)]
struct AsmLabCsvRow {
    instruction: String,
    disassembly_instruction: String,
    ax: String,
    bx: String,
    cx: String,
    dx: String,
    sp: String,
    ds: String,
    ss: String,
    cs: String,
    ip: String,
    flags_czsopaid: String,
}

impl<'s> From<&TraceStep<'s>> for AsmLabCsvRow {
    fn from(s: &TraceStep) -> Self {
        AsmLabCsvRow {
            instruction: {
                s.instruction
                    .bytes
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect()
            },
            disassembly_instruction: s.instruction.disassembly.to_owned(),
            ax: format!("{:04X}", s.registers.ax),
            bx: format!("{:04X}", s.registers.bx),
            cx: format!("{:04X}", s.registers.cx),
            dx: format!("{:04X}", s.registers.dx),
            sp: format!("{:04X}", s.registers.sp),
            ds: format!("{:04X}", s.registers.ds),
            ss: format!("{:04X}", s.registers.ss),
            cs: format!("{:04X}", s.registers.cs),
            ip: format!("{:04X}", s.registers.ip),
            flags_czsopaid: {
                format!(
                    "{:08b}",
                    [
                        s.flags.carry,
                        s.flags.zero,
                        s.flags.sign,
                        s.flags.overflow,
                        s.flags.parity,
                        s.flags.aux_carry,
                        s.flags.interrupt,
                        s.flags.direction,
                    ]
                    .iter()
                    .rev()
                    .enumerate()
                    .fold(0, |acc, (i, &f)| acc | (f as u8) << i)
                )
            },
        }
    }
}

fn main() {
    let src = std::env::args()
        .nth(1)
        .expect("Please provide path to the trace file");

    let data: Vec<u8> = fs::read(&src).unwrap_or_else(|e| panic!("cannot read {src}: {e}"));
    let data = String::from_utf8_lossy(&data);

    let steps = match trace_steps.parse(data.as_ref()) {
        Ok(steps) => steps,
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    };

    if steps.is_empty() {
        eprintln!("No trace steps found in {}", src);
        exit(1);
    }

    let dst =
        Path::new(Path::new(&src).file_stem().expect("Cannot get file stem")).with_extension("csv");

    let mut wtr = csv::Writer::from_path(&dst).expect("Cannot open dst file");

    steps.iter().for_each(|e| {
        wtr.serialize(AsmLabCsvRow::from(e))
            .expect("Cannot serialize")
    });

    wtr.flush().expect("Cannot write serialized data");
}

fn trace_steps<'s>(input: &mut &'s str) -> ModalResult<Vec<TraceStep<'s>>> {
    repeat(
        0..,
        alt((
            trace_step.map(Some),
            (take_till(0.., '\n'), line_ending).map(|_| None),
            terminated(take_till(1.., '\n'), eof).map(|_| None),
        )),
    )
    .fold(Vec::new, |mut acc, item| {
        if let Some(item) = item {
            acc.push(item);
        }
        acc
    })
    .parse_next(input)
}

fn trace_step<'s>(input: &mut &'s str) -> ModalResult<TraceStep<'s>> {
    (cpu_state, cut_err(instruction))
        .map(|((registers, flags), instruction)| TraceStep {
            registers,
            flags,
            instruction,
        })
        .parse_next(input)
}

fn instruction<'s>(input: &mut &'s str) -> ModalResult<Instruction<'s>> {
    let ((address, mut bytes), disassembly) = first_instruction_line.parse_next(input)?;

    let disassembly = match disassembly {
        Some(disassembly) => disassembly,
        None => {
            let extended_instr = repeat(0.., next_instruction_line)
                .fold(Vec::new, |mut acc: Vec<_>, item| {
                    acc.extend(item);
                    acc
                })
                .parse_next(input)?;
            bytes.extend(extended_instr);

            let (last_bytes, disassembly) = cut_err(last_instruction_line).parse_next(input)?;
            bytes.extend(last_bytes);

            disassembly
        }
    };

    if bytes.len() > 15 {
        return Err(ErrMode::Cut(ContextError::new()));
    }

    Ok(Instruction {
        address,
        bytes,
        disassembly,
    })
}

fn last_instruction_line<'s>(
    input: &mut &'s str,
) -> ModalResult<(InstructionBytes, Disassembly<'s>)> {
    terminated(
        separated_pair(preceded(space1, instruction_bytes), space1, disassembly),
        eol,
    )
    .parse_next(input)
}

fn next_instruction_line(input: &mut &str) -> ModalResult<InstructionBytes> {
    terminated(preceded(space1, instruction_bytes), eol).parse_next(input)
}

fn first_instruction_line<'s>(
    input: &mut &'s str,
) -> ModalResult<((FarAddress, InstructionBytes), Option<Disassembly<'s>>)> {
    terminated(
        (
            separated_pair(far_address, space1, instruction_bytes),
            opt(preceded(space1, disassembly)),
        ),
        eol,
    )
    .parse_next(input)
}

fn cpu_state(input: &mut &str) -> ModalResult<(Registers, Flags)> {
    (register_line, flag_and_register_line)
        .map(
            |((ax, bx, cx, dx, sp, bp, si, di), ((ds, es, ss, cs, ip), flags))| {
                (
                    Registers {
                        ax,
                        bx,
                        cx,
                        dx,
                        sp,
                        bp,
                        si,
                        di,
                        ds,
                        es,
                        ss,
                        cs,
                        ip,
                    },
                    flags,
                )
            },
        )
        .parse_next(input)
}

fn flag_and_register_line(input: &mut &str) -> ModalResult<(SecondLineRegisters, Flags)> {
    terminated(
        separated_pair(
            (
                register("DS"),
                preceded(space1, register("ES")),
                preceded(space1, register("SS")),
                preceded(space1, register("CS")),
                preceded(space1, register("IP")),
            )
                .map(|(ds, es, ss, cs, ip)| (ds, es, ss, cs, ip)),
            space1,
            flags,
        ),
        eol,
    )
    .parse_next(input)
}

fn disassembly<'s>(input: &mut &'s str) -> ModalResult<Disassembly<'s>> {
    take_till(1.., ('\r', '\n'))
        .map(str::trim_end)
        .parse_next(input)
}

fn instruction_bytes(input: &mut &str) -> ModalResult<Vec<u8>> {
    take_while(1..=16, AsChar::is_hex_digit)
        .verify(|code: &str| code.len().is_multiple_of(2))
        .try_map(|code: &str| {
            (0..code.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&code[i..i + 2], 16))
                .collect::<Result<Vec<u8>, std::num::ParseIntError>>()
        })
        .parse_next(input)
}

fn eol(input: &mut &str) -> ModalResult<()> {
    preceded(opt(space1), alt((eof, line_ending))).parse_next(input)?;
    Ok(())
}

fn flags(input: &mut &str) -> ModalResult<Flags> {
    (
        flag("NV", "OV"),
        preceded(space1, flag("UP", "DN")),
        preceded(space1, flag("DI", "EI")),
        preceded(space1, flag("PL", "NG")),
        preceded(space1, flag("NZ", "ZR")),
        preceded(space1, flag("NA", "AC")),
        preceded(space1, flag("PO", "PE")),
        preceded(space1, flag("NC", "CY")),
    )
        .map(
            |(overflow, direction, interrupt, sign, zero, aux_carry, parity, carry)| Flags {
                overflow,
                direction,
                interrupt,
                sign,
                zero,
                aux_carry,
                parity,
                carry,
            },
        )
        .parse_next(input)
}

fn flag<'s>(
    off: &'static str,
    on: &'static str,
) -> impl Parser<&'s str, bool, ErrMode<ContextError>> {
    alt((off.value(false), on.value(true)))
}

fn register_line(input: &mut &str) -> ModalResult<FirstLineRegisters> {
    terminated(
        (
            register("AX"),
            preceded(space1, register("BX")),
            preceded(space1, register("CX")),
            preceded(space1, register("DX")),
            preceded(space1, register("SP")),
            preceded(space1, register("BP")),
            preceded(space1, register("SI")),
            preceded(space1, register("DI")),
        ),
        eol,
    )
    .map(|(ax, bx, cx, dx, sp, bp, si, di)| (ax, bx, cx, dx, sp, bp, si, di))
    .parse_next(input)
}

fn register<'s>(name: &'static str) -> impl Parser<&'s str, u16, ErrMode<ContextError>> {
    preceded((name, "="), hex_word)
}

fn hex_word(input: &mut &str) -> ModalResult<u16> {
    take_while(4, AsChar::is_hex_digit)
        .try_map(|s| u16::from_str_radix(s, 16))
        .parse_next(input)
}

fn far_address(input: &mut &str) -> ModalResult<FarAddress> {
    separated_pair(hex_word, ":", hex_word).parse_next(input)
}
