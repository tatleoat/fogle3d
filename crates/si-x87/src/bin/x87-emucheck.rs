// SPDX-License-Identifier: MIT
//! `x87-emucheck <vectors directory>`: is the emulator's x87 unit the unit this crate models?
//!
//! `re/tools/emu/x87check.py` runs instruction sequences of our own (nothing of the original)
//! in the CPU emulator that the emulation harness uses, one file per instruction form, on
//! made-up operands. This binary computes the same with [`F80`] and compares every bit.
//! `F80` is itself compared with this machine's real x87 unit by the crate's tests, so
//! agreement here means the emulator, the model and a real unit give the same results for
//! that instruction form (finding 0011 has the numbers, and the forms where they do not).
use si_x87::vectors::{check_dir, print_report, Case, Check, Verdict};
use si_x87::{fmov, fmov64, Round, F80};
use std::cmp::Ordering;
use std::path::Path;

fn hex(v: F80) -> String {
    let (m, se) = v.to_bits();
    format!("{se:04x}{m:016x}")
}

fn reg(c: &Case, got: F80) -> Verdict {
    Verdict::from(c.output("r").same_f80(got), || format!("model {}", hex(got)))
}

fn num(c: &Case, name: &str, got: u64) -> Verdict {
    let e = c.output(name).values()[0] as u64;
    Verdict::from(e == got, || format!("model {got:x}"))
}

fn a(c: &Case) -> F80 {
    c.input("a").f80()
}

fn b(c: &Case) -> F80 {
    c.input("b").f80()
}

fn m32(c: &Case) -> F80 {
    F80::from_f32(c.input("m").f32())
}

fn m64(c: &Case) -> F80 {
    F80::from_f64(c.input("m").f64())
}

/// C3, C2 and C0 after comparing st0 = `x` with `y`.
fn flags(c: &Case, x: F80, y: F80) -> Verdict {
    let want = match x.compare(y) {
        None => 0x4500,
        Some(Ordering::Less) => 0x0100,
        Some(Ordering::Greater) => 0,
        Some(Ordering::Equal) => 0x4000,
    };
    num(c, "sw", want)
}

fn mode(name: &str) -> Round {
    match name.rsplit('-').next().unwrap() {
        "nearest" => Round::Nearest,
        "down" => Round::Down,
        "up" => Round::Up,
        _ => Round::Zero,
    }
}

fn lookup(name: &str) -> Option<Check> {
    let name = name.strip_prefix("x87.")?;
    Some(match name {
        // st0 = a, st1 = b; the D8 forms compute into st0, the DC and DE forms into st1
        "fadd-d8" | "fadd-dc" | "fadd-de" => |c: &Case| reg(c, a(c) + b(c)),
        "fmul-d8" | "fmul-dc" | "fmul-de" => |c: &Case| reg(c, a(c) * b(c)),
        "fsub-d8" | "fsubr-dc" | "fsubr-de" => |c: &Case| reg(c, a(c) - b(c)),
        "fsubr-d8" | "fsub-dc" | "fsub-de" => |c: &Case| reg(c, b(c) - a(c)),
        "fdiv-d8" | "fdivr-dc" | "fdivr-de" => |c: &Case| reg(c, a(c) / b(c)),
        "fdivr-d8" | "fdiv-dc" | "fdiv-de" => |c: &Case| reg(c, b(c) / a(c)),
        "fadd-m32" => |c: &Case| reg(c, a(c) + m32(c)),
        "fmul-m32" => |c: &Case| reg(c, a(c) * m32(c)),
        "fsub-m32" => |c: &Case| reg(c, a(c) - m32(c)),
        "fsubr-m32" => |c: &Case| reg(c, m32(c) - a(c)),
        "fdiv-m32" => |c: &Case| reg(c, a(c) / m32(c)),
        "fdivr-m32" => |c: &Case| reg(c, m32(c) / a(c)),
        "fadd-m64" => |c: &Case| reg(c, a(c) + m64(c)),
        "fmul-m64" => |c: &Case| reg(c, a(c) * m64(c)),
        "fsub-m64" => |c: &Case| reg(c, a(c) - m64(c)),
        "fsubr-m64" => |c: &Case| reg(c, m64(c) - a(c)),
        "fdiv-m64" => |c: &Case| reg(c, a(c) / m64(c)),
        "fdivr-m64" => |c: &Case| reg(c, m64(c) / a(c)),
        "fsqrt" => |c: &Case| reg(c, a(c).sqrt()),
        "fabs" => |c: &Case| reg(c, a(c).abs()),
        "fchs" => |c: &Case| reg(c, -a(c)),
        "fxch" => |c: &Case| reg(c, b(c)).and(|| Verdict::from(c.output("r2").same_f80(a(c)), || "second register".into())),
        "fld-st1" => |c: &Case| reg(c, b(c)),
        "fldz" => |c: &Case| reg(c, F80::ZERO),
        "fld1" => |c: &Case| reg(c, F80::ONE),
        "fstp-m32" | "fst-m32" => |c: &Case| num(c, "r", a(c).to_f32().to_bits() as u64),
        "fstp-m64" | "fst-m64" => |c: &Case| num(c, "r", a(c).to_f64().to_bits()),
        "fld-m32" => |c: &Case| reg(c, m32(c)),
        "fld-m64" => |c: &Case| reg(c, m64(c)),
        "fild-m16" => |c: &Case| reg(c, F80::from_i64(c.input("m").i16() as i64)),
        "fild-m32" => |c: &Case| reg(c, F80::from_i64(c.input("m").i32() as i64)),
        "fild-m64" => |c: &Case| reg(c, F80::from_i64(c.input("m").u64s()[0] as i64)),
        "fld-fstp-m32" => |c: &Case| num(c, "r", fmov(c.input("m").f32()).to_bits() as u64),
        "fld-fstp-m64" => |c: &Case| num(c, "r", fmov64(c.input("m").f64()).to_bits()),
        "fcom" | "fcomp" | "fcompp" | "fucom" | "fucomp" | "fucompp" => |c: &Case| flags(c, a(c), b(c)),
        "fcom-m32" | "fcomp-m32" => |c: &Case| flags(c, a(c), m32(c)),
        "fcom-m64" | "fcomp-m64" => |c: &Case| flags(c, a(c), m64(c)),
        // the two instructions the harness computes itself
        "fprem-harness" => |c: &Case| {
            let (r, more) = a(c).fprem(b(c));
            reg(c, r).and(|| num(c, "c2", more as u64))
        },
        "fscale-harness" => |c: &Case| reg(c, a(c).fscale(b(c))),
        _ if name.starts_with("frndint-") => |c: &Case| reg(c, a(c).round_int(mode(&c.name))),
        _ if name.starts_with("fistp-m16-") => |c: &Case| num(c, "r", a(c).to_i16(mode(&c.name)) as u16 as u64),
        _ if name.starts_with("fistp-m32-") || name.starts_with("fist-m32-") => {
            |c: &Case| num(c, "r", a(c).to_i32(mode(&c.name)) as u32 as u64)
        }
        _ if name.starts_with("fistp-m64-") => |c: &Case| num(c, "r", a(c).to_i64(mode(&c.name)) as u64),
        _ => return None,
    })
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: x87-emucheck <directory of x87.*.vec files from re/tools/emu/x87check.py>");
        std::process::exit(2);
    };
    match check_dir(Path::new(&dir), &lookup) {
        Ok(rows) => {
            if !print_report(&rows) {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("x87-emucheck: {e}");
            std::process::exit(2);
        }
    }
}
