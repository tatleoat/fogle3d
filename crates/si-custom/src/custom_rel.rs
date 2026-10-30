// SPDX-License-Identifier: MIT
//! `cusStatusCreateRelations` (custom_mn.c, unit cusrel1, finding 0041, 2026-10-29): the replay of the script a process-based
//! custom effect saved in its icon's user data, and `sscanf` as the C library of the original's time reads it.
//!
//! The first half of this file is [`scan`] / [`sscanf_mem`]: an exact model of glibc 2.x `sscanf` for 32-bit `long` (the
//! original's), the way `edtools3::sprintf_work` models `sprintf`. The second half is the routine, which calls `SI_cusCmd`
//! (0x0853b284, not ported: it forks and execs the effect process) through a hook.

use crate::custom_io::{cus_icon_get_saved_script, si_cus_model_search_user_data};
use crate::custom_udf::PLT_FOPEN;
use crate::{fprintf, through_x87, STDERR};
use si_mem::{Mem, Outside, Routine};

// ------------------------------------------------------------------------------------------------------------------
// sscanf
// ------------------------------------------------------------------------------------------------------------------

/// One value `sscanf` stored through one of its pointer arguments, in the order of the conversions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanStore {
    /// `%s`, `%[..]`: the characters and the closing NUL (the bytes are stored together).
    Str(Vec<u8>),
    /// `%c`: exactly the characters read (no NUL).
    Chars(Vec<u8>),
    /// `%d %i %u %o %x %X`: `bytes` (1, 2, 4 or 8) low bytes of `val`.
    Int { bytes: u8, val: u64 },
    /// `%f %e %g`: the float's bits.
    F32(u32),
    /// `%lf`: the double's bits.
    F64(u64),
    /// `%n`: the count of characters consumed so far.
    Count(u32),
}

/// What [`scan`] did: the return value (`EOF` is -1) and the stores made before it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOut {
    pub ret: i32,
    pub stores: Vec<ScanStore>,
}

const WS: &[u8] = b" \t\n\x0b\x0c\r";

fn is_space(c: i32) -> bool {
    c >= 0 && WS.contains(&(c as u8))
}

struct Src<'a> {
    inp: &'a [u8],
    pos: usize,
}

impl Src<'_> {
    /// `inchar`: the next character, -1 (EOF) at the end of the text.
    fn next(&mut self) -> i32 {
        match self.inp.get(self.pos) {
            Some(&c) => {
                self.pos += 1;
                c as i32
            }
            None => -1,
        }
    }
    /// `ungetc` of the last character read (nothing for EOF).
    fn back(&mut self, c: i32) {
        if c >= 0 {
            self.pos -= 1;
        }
    }
}

/// How a conversion ended: the two jumps of the C library's loop (`input_error` and `conv_error`).
enum End {
    /// `input_error`: EOF where a character was needed; the result is EOF when nothing converted before.
    Input,
    /// `conv_error`: the text does not fit; the result is the count so far.
    Conv,
}

fn digit_val(c: i32) -> Option<u32> {
    match c {
        0x30..=0x39 => Some(c as u32 - 0x30),
        0x61..=0x66 => Some(c as u32 - 0x61 + 10),
        0x41..=0x46 => Some(c as u32 - 0x41 + 10),
        _ => None,
    }
}

fn lower(c: i32) -> i32 {
    if (0x41..=0x5a).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// `strtol`/`strtoul` on the collected text of the 32-bit C library: the magnitude clamps (signed to the `int` range,
/// unsigned to `0xffffffff`), a minus sign negates an unsigned result modulo 2^32.
fn int_value(buf: &[u8], base: u32, signed: bool) -> u64 {
    let (neg, digits) = match buf.first() {
        Some(b'-') => (true, &buf[1..]),
        Some(b'+') => (false, &buf[1..]),
        _ => (false, buf),
    };
    let mut mag: u128 = 0;
    for &d in digits {
        let v = digit_val(d as i32).unwrap_or(0) as u128;
        mag = mag.saturating_mul(base as u128).saturating_add(v);
        if mag > u64::MAX as u128 {
            mag = u64::MAX as u128;
        }
    }
    if signed {
        let v = if neg {
            if mag > 0x8000_0000 { i32::MIN as i64 } else { -(mag as i64) }
        } else if mag > 0x7fff_ffff {
            i32::MAX as i64
        } else {
            mag as i64
        };
        v as i32 as u32 as u64
    } else {
        let v = if mag > 0xffff_ffff { 0xffff_ffffu32 } else { mag as u32 };
        (if neg { v.wrapping_neg() } else { v }) as u64
    }
}

/// Rounds `mant * 2^e2` (plus a sticky bit for dropped nonzero digits) to the binary format with `mbits` significant bits
/// and the smallest normal exponent `emin`, to nearest even; the bits of the result with the sign. `ebits` is the exponent
/// field width.
fn round_binary(neg: bool, mant: u128, sticky: bool, e2: i32, mbits: u32, ebits: u32) -> u64 {
    let sign = (neg as u64) << (mbits - 1 + ebits);
    if mant == 0 {
        return sign;
    }
    let bias = (1i32 << (ebits - 1)) - 1;
    let emin = 1 - bias;
    let len = 128 - mant.leading_zeros() as i32;
    let e = len - 1 + e2; // exponent of the leading bit
    let keep = if e >= emin { mbits as i32 } else { mbits as i32 - (emin - e) };
    // the number of low bits to drop
    let drop = len - keep;
    let (mut q, round_up): (u128, bool);
    if keep <= 0 {
        // everything is below the smallest denormal's half or at it
        if keep < 0 {
            return sign;
        }
        // keep == 0: the value is in [2^(emin-mbits), 2^(emin-mbits+1)) scaled below the smallest denormal; rounds up only
        // when above the half (the half is 2^(len-1) of mant)
        let half = 1u128 << (len - 1);
        let above = mant > half || (mant == half && sticky);
        return sign | above as u64;
    }
    if drop <= 0 {
        q = mant << (-drop);
        round_up = false;
    } else {
        q = mant >> drop;
        let rem = mant & ((1u128 << drop) - 1);
        let half = 1u128 << (drop - 1);
        round_up = rem > half || (rem == half && (sticky || q & 1 == 1)) || (rem == half && sticky);
        // rem == half with no sticky rounds to even (handled above through q & 1)
    }
    if round_up {
        q += 1;
    }
    // q has `keep` bits (or keep+1 after a carry)
    let mut exp_field;
    if e >= emin {
        let mut ee = e;
        if q >> mbits != 0 {
            q >>= 1;
            ee += 1;
        }
        if ee > bias {
            return sign | (((1u64 << ebits) - 1) << (mbits - 1));
        }
        exp_field = (ee + bias) as u64;
        q &= (1u128 << (mbits - 1)) - 1;
    } else {
        // denormal: q < 2^(mbits-1) normally; a carry into the lowest normal is the exponent field 1
        exp_field = 0;
        if q >> (mbits - 1) != 0 {
            exp_field = 1;
            q &= (1u128 << (mbits - 1)) - 1;
        }
    }
    sign | (exp_field << (mbits - 1)) | q as u64
}

/// `strtof` (`double == false`) or `strtod` on the characters `scanf` collected: the bits of the value and whether any
/// text was used. The longest prefix that is a number is converted ("1e" is 1, "0x" alone is 0).
fn strtod_prefix(buf: &[u8], double: bool) -> Option<u64> {
    let (mbits, ebits) = if double { (53u32, 11u32) } else { (24u32, 8u32) };
    let mut i = 0;
    let mut neg = false;
    if buf.first() == Some(&b'-') || buf.first() == Some(&b'+') {
        neg = buf[0] == b'-';
        i = 1;
    }
    let rest = &buf[i..];
    let low: Vec<u8> = rest.iter().map(|c| c.to_ascii_lowercase()).collect();
    let signbit = (neg as u64) << (mbits - 1 + ebits);
    let inf = (((1u64 << ebits) - 1) << (mbits - 1)) | signbit;
    if low.starts_with(b"inf") {
        return Some(inf);
    }
    if low.starts_with(b"nan") {
        return Some(inf | (1u64 << (mbits - 2)));
    }
    if low.starts_with(b"0x") {
        // hexadecimal: digits, one point, `p` exponent
        let mut j = 2;
        let mut mant: u128 = 0;
        let mut sticky = false;
        let mut e2: i32 = 0;
        let mut dot = false;
        let mut cut = false;
        while j < low.len() {
            let c = low[j];
            if c == b'.' && !dot {
                dot = true;
            } else if let Some(v) = digit_val(c as i32).filter(|_| c.is_ascii_hexdigit()) {
                if mant >> 120 == 0 && !cut {
                    mant = (mant << 4) | v as u128;
                    if dot {
                        e2 -= 4;
                    }
                } else {
                    cut = true;
                    sticky |= v != 0;
                    if !dot {
                        e2 += 4;
                    }
                }
            } else {
                break;
            }
            j += 1;
        }
        // glibc 2.2.5's conversion of hexadecimal text needs the binary exponent letter: without it nothing converts
        if j >= low.len() || low[j] != b'p' {
            return None;
        }
        {
            let mut k = j + 1;
            let mut eneg = false;
            if k < low.len() && (low[k] == b'-' || low[k] == b'+') {
                eneg = low[k] == b'-';
                k += 1;
            }
            let st = k;
            let mut ev: i64 = 0;
            while k < low.len() && low[k].is_ascii_digit() {
                ev = (ev * 10 + (low[k] - b'0') as i64).min(100_000);
                k += 1;
            }
            if k > st {
                e2 = e2.saturating_add(if eneg { -ev } else { ev } as i32);
            }
        }
        return Some(round_binary(neg, mant, sticky, e2, mbits, ebits));
    }
    // decimal: digits [. digits] [e [sign] digits]
    let mut j = 0;
    let mut nd = 0;
    while j < low.len() && low[j].is_ascii_digit() {
        j += 1;
        nd += 1;
    }
    if j < low.len() && low[j] == b'.' {
        j += 1;
        while j < low.len() && low[j].is_ascii_digit() {
            j += 1;
            nd += 1;
        }
    }
    if nd == 0 {
        return None;
    }
    let mut end = j;
    if j < low.len() && low[j] == b'e' {
        let mut k = j + 1;
        if k < low.len() && (low[k] == b'-' || low[k] == b'+') {
            k += 1;
        }
        let st = k;
        while k < low.len() && low[k].is_ascii_digit() {
            k += 1;
        }
        if k > st {
            end = k;
        }
    }
    let text = std::str::from_utf8(&low[..end]).unwrap_or("0");
    let bits = if double {
        text.parse::<f64>().map(|v| v.to_bits()).unwrap_or(0)
    } else {
        text.parse::<f32>().map(|v| v.to_bits() as u64).unwrap_or(0)
    };
    Some(bits | signbit)
}

/// The text `sscanf(text, fmt, ...)` stores and returns for the conversions `%d %i %u %o %x %X %s %c %[set] %f %e %g %E %G
/// %n %%` with `*`, a width and the length modifiers `hh h l ll` (glibc 2.x, 32-bit `long`; `L` and `p` are not modelled and
/// stop with a conversion error). `text` ends at its first NUL or its end.
pub fn scan(text: &[u8], fmt: &[u8]) -> ScanOut {
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    let mut s = Src { inp: &text[..end], pos: 0 };
    let mut stores = Vec::new();
    let mut done: i32 = 0;
    let r = run(&mut s, fmt, &mut stores, &mut done);
    let ret = match r {
        Ok(()) => done,
        Err(End::Conv) => done,
        Err(End::Input) => {
            if done == 0 {
                -1
            } else {
                done
            }
        }
    };
    ScanOut { ret, stores }
}

fn run(s: &mut Src, fmt: &[u8], stores: &mut Vec<ScanStore>, done: &mut i32) -> Result<(), End> {
    let mut fi = 0;
    let mut skip_space = false;
    while fi < fmt.len() && fmt[fi] != 0 {
        let fc = fmt[fi];
        fi += 1;
        if fc != b'%' {
            if is_space(fc as i32) {
                // glibc: "remember to skip spaces" before the next conversion
                skip_space = true;
                continue;
            }
            let c = s.next();
            if c < 0 {
                return Err(End::Input);
            }
            if c != fc as i32 {
                s.back(c);
                return Err(End::Conv);
            }
            continue;
        }
        // flags, width, length
        let mut suppress = false;
        if fmt.get(fi) == Some(&b'*') {
            suppress = true;
            fi += 1;
        }
        let mut width: i64 = -1;
        if fmt.get(fi).is_some_and(|c| c.is_ascii_digit()) {
            width = 0;
            while let Some(&c) = fmt.get(fi).filter(|c| c.is_ascii_digit()) {
                width = (width * 10 + (c - b'0') as i64).min(1 << 30);
                fi += 1;
            }
        }
        let (mut hs, mut ls) = (0, 0);
        while let Some(&c) = fmt.get(fi) {
            if c == b'h' {
                hs += 1;
            } else if c == b'l' {
                ls += 1;
            } else {
                break;
            }
            fi += 1;
        }
        let Some(&conv) = fmt.get(fi) else { return Err(End::Conv) };
        fi += 1;
        let eats = skip_space || !matches!(conv, b'[' | b'c' | b'n');
        if eats {
            let mut c = s.next();
            while is_space(c) {
                c = s.next();
            }
            s.back(c);
            skip_space = false;
        }
        match conv {
            b'%' => {
                let c = s.next();
                if c < 0 {
                    return Err(End::Input);
                }
                if c != b'%' as i32 {
                    s.back(c);
                    return Err(End::Conv);
                }
            }
            b'n' => {
                if !suppress {
                    stores.push(ScanStore::Count(s.pos as u32));
                }
            }
            b'c' => {
                let n = if width < 0 { 1 } else { width as usize };
                let mut buf = Vec::new();
                for k in 0..n {
                    let c = s.next();
                    if c < 0 {
                        // glibc 2.2.5 keeps the characters read before the end (the first one must be there)
                        if k == 0 {
                            return Err(End::Input);
                        }
                        break;
                    }
                    buf.push(c as u8);
                }
                if !suppress {
                    stores.push(ScanStore::Chars(buf));
                    *done += 1;
                }
            }
            b's' => {
                let mut c = s.next();
                if c < 0 {
                    return Err(End::Input);
                }
                let mut buf = Vec::new();
                while c >= 0 && !is_space(c) && width != 0 {
                    buf.push(c as u8);
                    if width > 0 {
                        width -= 1;
                    }
                    c = s.next();
                }
                s.back(c);
                buf.push(0);
                if !suppress {
                    stores.push(ScanStore::Str(buf));
                    *done += 1;
                }
            }
            b'[' => {
                let mut neg = false;
                if fmt.get(fi) == Some(&b'^') {
                    neg = true;
                    fi += 1;
                }
                let mut set = [false; 256];
                let mut first = true;
                loop {
                    let Some(&c) = fmt.get(fi) else { return Err(End::Conv) };
                    fi += 1;
                    if c == b']' && !first {
                        break;
                    }
                    first = false;
                    if fmt.get(fi) == Some(&b'-') && fmt.get(fi + 1).is_some_and(|&e| e != b']') {
                        let e = fmt[fi + 1];
                        fi += 2;
                        if c <= e {
                            for k in c..=e {
                                set[k as usize] = true;
                            }
                        }
                    } else {
                        set[c as usize] = true;
                    }
                }
                let mut c = s.next();
                if c < 0 {
                    return Err(End::Input);
                }
                let mut buf = Vec::new();
                while c >= 0 && set[c as usize] != neg && width != 0 {
                    buf.push(c as u8);
                    if width > 0 {
                        width -= 1;
                    }
                    c = s.next();
                }
                s.back(c);
                if buf.is_empty() {
                    return Err(End::Conv);
                }
                buf.push(0);
                if !suppress {
                    stores.push(ScanStore::Str(buf));
                    *done += 1;
                }
            }
            b'd' | b'i' | b'u' | b'o' | b'x' | b'X' => {
                let mut base: u32 = match conv {
                    b'd' | b'u' => 10,
                    b'o' => 8,
                    b'x' | b'X' => 16,
                    _ => 0,
                };
                let signed = matches!(conv, b'd' | b'i');
                let mut c = s.next();
                if c < 0 {
                    return Err(End::Input);
                }
                let mut buf: Vec<u8> = Vec::new();
                if c == b'-' as i32 || c == b'+' as i32 {
                    buf.push(c as u8);
                    if width > 0 {
                        width -= 1;
                    }
                    c = s.next();
                }
                if width != 0 && c == b'0' as i32 {
                    if width > 0 {
                        width -= 1;
                    }
                    buf.push(c as u8);
                    c = s.next();
                    if width != 0 && lower(c) == b'x' as i32 {
                        if base == 0 {
                            base = 16;
                        }
                        if base == 16 {
                            if width > 0 {
                                width -= 1;
                            }
                            c = s.next();
                        }
                    } else if base == 0 {
                        base = 8;
                    }
                }
                if base == 0 {
                    base = 10;
                }
                while c >= 0 && width != 0 {
                    let ok = if base == 16 { digit_val(c).is_some() } else { digit_val(c).is_some_and(|v| v < base && c <= 0x39) };
                    if !ok {
                        break;
                    }
                    buf.push(c as u8);
                    if width > 0 {
                        width -= 1;
                    }
                    c = s.next();
                }
                s.back(c);
                if buf.is_empty() || (buf.len() == 1 && (buf[0] == b'+' || buf[0] == b'-')) {
                    return Err(End::Conv);
                }
                let v = int_value(&buf, base, signed);
                if !suppress {
                    let bytes = match (hs, ls) {
                        (h, _) if h >= 2 => 1,
                        (1, _) => 2,
                        (_, l) if l >= 2 => 8,
                        _ => 4,
                    };
                    // ll reads with strtoll: the 64-bit range, not the 32-bit clamp
                    let val = if bytes == 8 { int_value64(&buf, base, signed) } else { v };
                    stores.push(ScanStore::Int { bytes, val });
                    *done += 1;
                }
            }
            b'e' | b'f' | b'g' | b'E' | b'G' | b'a' | b'A' => float_conv(s, width, ls >= 1, suppress, stores, done)?,
            _ => return Err(End::Conv),
        }
    }
    Ok(())
}

/// 64-bit `strtoll`/`strtoull` for `%lld`.
fn int_value64(buf: &[u8], base: u32, signed: bool) -> u64 {
    let (neg, digits) = match buf.first() {
        Some(b'-') => (true, &buf[1..]),
        Some(b'+') => (false, &buf[1..]),
        _ => (false, buf),
    };
    let mut mag: u128 = 0;
    for &d in digits {
        mag = mag.saturating_mul(base as u128).saturating_add(digit_val(d as i32).unwrap_or(0) as u128).min(u64::MAX as u128);
    }
    if signed {
        if neg {
            if mag > 1 << 63 { i64::MIN as u64 } else { (mag as u64).wrapping_neg() }
        } else if mag > i64::MAX as u128 {
            i64::MAX as u64
        } else {
            mag as u64
        }
    } else {
        let v = mag as u64;
        if neg { v.wrapping_neg() } else { v }
    }
}

/// The float conversions: glibc collects the characters of a number (`nan`, `inf`, `infinity`, hexadecimal with `0x`, or decimal
/// with a point and an exponent) and hands what it collected to `strtof` / `strtod`, which uses the longest prefix that is a number;
/// text it collected but could not use is gone.
fn float_conv(s: &mut Src, mut width: i64, double: bool, suppress: bool, stores: &mut Vec<ScanStore>, done: &mut i32) -> Result<(), End> {
    let mut c = s.next();
    if c < 0 {
        return Err(End::Input);
    }
    let mut buf: Vec<u8> = Vec::new();
    let dec = |w: &mut i64| {
        if *w > 0 {
            *w -= 1;
        }
    };
    if c == b'-' as i32 || c == b'+' as i32 {
        buf.push(c as u8);
        dec(&mut width);
        c = s.next();
    }
    let mut hexa = false;
    let mut got_digit = false;
    let special = if width != 0 && lower(c) == b'n' as i32 {
        for want in [b'n', b'a', b'n'] {
            if width == 0 || lower(c) != want as i32 {
                return Err(End::Conv);
            }
            buf.push(c as u8);
            dec(&mut width);
            c = s.next();
        }
        true
    } else if width != 0 && lower(c) == b'i' as i32 {
        for want in [b'i', b'n', b'f'] {
            if width == 0 || lower(c) != want as i32 {
                return Err(End::Conv);
            }
            buf.push(c as u8);
            dec(&mut width);
            c = s.next();
        }
        if width != 0 && lower(c) == b'i' as i32 {
            for want in [b'i', b'n', b'i', b't', b'y'] {
                if width == 0 || lower(c) != want as i32 {
                    return Err(End::Conv);
                }
                buf.push(c as u8);
                dec(&mut width);
                c = s.next();
            }
        }
        true
    } else {
        false
    };
    if !special {
        if width != 0 && c == b'0' as i32 {
            // glibc 2.2.5: this first zero is not counted in the width
            buf.push(c as u8);
            c = s.next();
            if width != 0 && lower(c) == b'x' as i32 {
                buf.push(c as u8);
                hexa = true;
                c = s.next();
                dec(&mut width);
            } else {
                got_digit = true;
            }
        }
        let (mut got_dot, mut got_e) = (false, false);
        while c >= 0 && width != 0 {
            let last = buf.last().copied().unwrap_or(0);
            if (b'0' as i32..=b'9' as i32).contains(&c) {
                buf.push(c as u8);
                got_digit = true;
            } else if hexa && !got_e && (c as u8).is_ascii_hexdigit() {
                buf.push(c as u8);
                got_digit = true;
            } else if got_e && matches!(last, b'e' | b'E' | b'p' | b'P') && (c == b'-' as i32 || c == b'+' as i32) {
                buf.push(c as u8);
            } else if got_digit && !got_e && lower(c) == if hexa { b'p' as i32 } else { b'e' as i32 } {
                buf.push(c as u8);
                got_e = true;
                got_dot = true;
            } else if !got_dot && c == b'.' as i32 {
                buf.push(c as u8);
                got_dot = true;
            } else {
                break;
            }
            dec(&mut width);
            c = s.next();
        }
    }
    s.back(c);
    let signed = matches!(buf.first(), Some(b'+') | Some(b'-'));
    if buf.is_empty() || (hexa && buf.len() == if signed { 3 } else { 2 }) {
        return Err(End::Conv);
    }
    match strtod_prefix(&buf, double) {
        None => Err(End::Conv),
        Some(bits) => {
            if !suppress {
                stores.push(if double { ScanStore::F64(bits) } else { ScanStore::F32(bits as u32) });
                *done += 1;
            }
            Ok(())
        }
    }
}

/// The bytes a store writes through its pointer (little endian).
pub fn store_bytes(st: &ScanStore) -> Vec<u8> {
    match st {
        ScanStore::Str(b) | ScanStore::Chars(b) => b.clone(),
        ScanStore::Int { bytes, val } => val.to_le_bytes()[..*bytes as usize].to_vec(),
        ScanStore::F32(b) => b.to_le_bytes().to_vec(),
        ScanStore::F64(b) => b.to_le_bytes().to_vec(),
        ScanStore::Count(n) => n.to_le_bytes().to_vec(),
    }
}

/// `sscanf(text, fmt, args...)` on the image: the C string at `text`, the format `fmt`, the conversions' pointer
/// arguments in order (`args` is the list of addresses); the stores are made through them. The result is the C library's.
pub fn sscanf_mem(m: &mut Mem, text: u32, fmt: &[u8], args: &[u32]) -> i32 {
    let t = m.cstr(text);
    let out = scan(&t, fmt);
    for (st, &a) in out.stores.iter().zip(args.iter()) {
        m.set_bytes(a, &store_bytes(st));
    }
    out.ret
}

// ------------------------------------------------------------------------------------------------------------------
// cusStatusCreateRelations
// ------------------------------------------------------------------------------------------------------------------

/// `SI_fdmodelBinaryRead(fp, 0, 0)` (0x083ec740): not ported here.
pub const SI_FDMODEL_BINARY_READ: Routine = Routine::new("SI_fdmodelBinaryRead", 0x083e_c740);
/// `si_cusFindModelByName(name)` (0x0854a7c0): ported (custom_util), but it walks the scene, so it is a hook here.
pub const SI_CUS_FIND_MODEL_BY_NAME: Routine = Routine::new("si_cusFindModelByName", 0x0854_a7c0);
/// `SI_modelDispose(&model, 1)` (0x08461920): not ported here.
pub const SI_MODEL_DISPOSE: Routine = Routine::new("SI_modelDispose", 0x0846_1920);
/// `SI_byteswap_fwrite(buf, size, count, fp)` (0x083f9c40): not ported here.
pub const SI_BYTESWAP_FWRITE: Routine = Routine::new("SI_byteswap_fwrite", 0x083f_9c40);
/// `SI_cusCmd(status, effect, command, time, 0, &flag, 0)` (0x0853b284): forks, pipes and execs the effect process, so it stays a
/// hook (finding 0041, cusrel1 section).
pub use crate::custom_mn::SI_CUS_CMD_M as SI_CUS_CMD_HOOK;
/// `SI_addInDictionary`, a short the model reader looks at.
const SI_ADD_IN_DICTIONARY: u32 = 0x0867_92b0;
const SI_FRAME_CURRENT_CELL: u32 = 0x0878_2040;
const SI_FRAME_RATE_CELL: u32 = 0x0878_204c;
/// The word `SI_refreshAll` is held back by.
const REFRESH_HOLD: u32 = 0x087f_b99c;

// Strings of the original's data.
const S_SCRIPT_SIZES: u32 = 0x086e_0380; // "Hey! Inconsistent script sizes in user data!\n"
const S_FMT_DN: &[u8] = b" %d %n"; // 0x086e03ae
const S_FMT_SF: &[u8] = b"%s %f"; // 0x086e03b5
const S_FMT_SD: &[u8] = b"%s %d"; // 0x086e03bb, 0x086e041f
const S_FMT_SHD: &[u8] = b"%s %hd"; // 0x086e03c1
const S_FMT_SS: &[u8] = b"%s %s"; // 0x086e03c8
const S_BAD_SCRIPT: u32 = 0x086e_03e0; // "Invalid script in custom icon data.\n"
const S_NO_MODEL: u32 = 0x086e_0405; // "Model %s does not exist!\n"
const S_MODE_W: u32 = 0x086e_0425; // "w"
const S_NO_TAG_FILE: u32 = 0x086e_0427; // "Cannot write custom tag file."
const S_CUSC: u32 = 0x086e_0445; // "CUSC"
const S_FCURVE_SIZES: u32 = 0x086e_0460; // "Hey! Inconsistent fcurve sizes in user data!\n"
const S_STRANGE: u32 = 0x086e_048e; // "Whoa! That's VERY strange.\n"

/// Commands that are handed to `SI_cusCmd` with no script data of their own.
fn plain_command(kind: u16) -> bool {
    matches!(kind, 0xc | 0xe | 0xf | 0x10 | 0x11 | 0x14..=0x1b | 0x1d | 0x23..=0x25 | 0x2d..=0x30 | 0x37..=0x46 | 0x49 | 0x4b..=0x4d)
}

/// Reads the model file a command's pipe holds (kinds 0x12 and 0x1e), and throws the model away.
fn read_and_dispose_model(m: &mut Mem, o: &mut dyn Outside, cmd: u32) {
    let pipe = m.u32(cmd.wrapping_add(0x2c));
    if pipe == 0 {
        return;
    }
    let fp = if (m.u32(pipe.wrapping_add(0x18)) as i32) >= 0 { m.u32(pipe.wrapping_add(0x10)) } else { 0 };
    if fp == 0 {
        return;
    }
    m.set_u16(SI_ADD_IN_DICTIONARY, 1);
    let cell = m.local(4);
    let model = o.call(m, SI_FDMODEL_BINARY_READ, &[fp, 0, 0]).eax;
    m.set_u32(cell, model);
    if model != 0 {
        o.call(m, SI_MODEL_DISPOSE, &[cell, 1]);
    }
}

/// [soft 0x08536914 cusStatusCreateRelations]: replays the script a process-based custom effect saved in the user data of its icon
/// `model`: for every command of the effect (`eff + 0xf8`) it reads the command's values (and model names) from the script,
/// hands the command to `SI_cusCmd` (a hook), and afterwards makes the fcurves of the float values from the "CUSC" user data.
/// `hdr` is the effect's header (script size at +0x30, curve size at +0x34, motion at +4), `models` / `n` are the models the caller
/// has ready (null for none). Answers 1, or 0xffffffff after a complaint on `stderr`.
///
/// The `short` that `SI_cusCmd` reports a change in is never set by the original before the first command; here it starts as 0.
/// (There is no licence code on this path.)
pub fn cus_status_create_relations(m: &mut Mem, o: &mut dyn Outside, status: u32, eff: u32, hdr: u32, model: u32, n: i32, models: u32) -> u32 {
    let fail = 0xffff_ffffu32;
    if m.u16(eff.wrapping_add(8)) == 0 {
        o.call(m, crate::custom_io::SI_CUS_LOAD_EFFECT, &[status, eff]);
    }
    if eff != 0 {
        let t = m.u32(eff.wrapping_add(0x104));
        if t != 0 && m.u32(t) != 0 {
            return 1;
        }
    }
    let err = m.u32(STDERR);
    let node = m.local(4);
    let base = cus_icon_get_saved_script(m, o, model, node);
    if base == 0 {
        return fail;
    }
    let node_p = m.u32(node);
    if m.u32(node_p.wrapping_add(8)) != m.u32(hdr.wrapping_add(0x30)) {
        fprintf(m, o, err, S_SCRIPT_SIZES, &[]);
        return fail;
    }
    let mut off: i32 = 0;
    let mut nflt: i32 = 0;
    let mut mi: i32 = 0;
    let time = (si_x87::x(m.u32(SI_FRAME_CURRENT_CELL) as i32) / si_x87::x(m.f32(SI_FRAME_RATE_CELL))).to_f32().to_bits();
    let flag = m.local(2);
    m.set_u16(flag, 0);
    let tag = m.local(40);
    let name = m.local(160);
    let lenc = m.local(4);
    let cons = m.local(4);
    let mut cmd = m.u32(eff.wrapping_add(0xf8));
    while cmd != 0 {
        let kind = m.u16(cmd);
        let mut s = base.wrapping_add(off as u32);
        let idx = kind.wrapping_sub(2) as i16;
        if (idx as u32) > 0x4b {
            cmd = m.u32(cmd.wrapping_add(0x30));
            continue;
        }
        match kind {
            2 => {
                let mut v = m.u32(eff.wrapping_add(0x64));
                while v != 0 {
                    // the invariant `s = base + off` holds at the start of every value (the string forms reset it at their end)
                    s = base.wrapping_add(off as u32);
                    match m.u16(v.wrapping_add(4)) as i16 {
                        1 => {
                            nflt += 1;
                            let d = m.u32(v);
                            sscanf_mem(m, s, S_FMT_SF, &[d, v.wrapping_add(0x10)]);
                            off = off.wrapping_add(0x28);
                            s = base.wrapping_add(off as u32);
                        }
                        2 => {
                            let d = m.u32(v);
                            sscanf_mem(m, s, S_FMT_SD, &[d, v.wrapping_add(0x14)]);
                            off = off.wrapping_add(0x28);
                            s = base.wrapping_add(off as u32);
                        }
                        3 => {
                            let d = m.u32(v);
                            sscanf_mem(m, s, S_FMT_SHD, &[d, v.wrapping_add(0xc)]);
                            off = off.wrapping_add(0x28);
                            s = base.wrapping_add(off as u32);
                        }
                        0 => {
                            if m.u8(s) == b' ' {
                                let start = s;
                                let mut p = s.wrapping_add(1);
                                while m.u8(p) != b' ' {
                                    p = p.wrapping_add(1);
                                }
                                sscanf_mem(m, p, S_FMT_DN, &[lenc, cons]);
                                let len = m.u32(lenc);
                                p = p.wrapping_add(m.u32(cons));
                                let old = m.u32(v.wrapping_add(0x18));
                                if old != 0 {
                                    m.free(old);
                                    m.set_u32(v.wrapping_add(0x18), 0);
                                }
                                let mut dst = m.si_malloc(len.wrapping_add(1));
                                m.set_u32(v.wrapping_add(0x18), dst);
                                while m.u8(p) != 0 {
                                    let c = m.u8(p);
                                    m.set_u8(dst, c);
                                    dst = dst.wrapping_add(1);
                                    p = p.wrapping_add(1);
                                }
                                m.set_u8(dst, 0);
                                let used = p.wrapping_sub(start) as i32;
                                off = off.wrapping_add((used / 0x28 * 5 + 5) * 8);
                            } else {
                                let mut dst = m.u32(v);
                                off = off.wrapping_add(0x28);
                                loop {
                                    let c = m.u8(s);
                                    m.set_u8(dst, c);
                                    dst = dst.wrapping_add(1);
                                    s = s.wrapping_add(1);
                                    if m.u8(s) == b' ' {
                                        break;
                                    }
                                }
                                m.set_u8(dst, 0);
                                while m.u8(s) == b' ' {
                                    s = s.wrapping_add(1);
                                }
                                let mut dst = m.u32(v.wrapping_add(0x18));
                                while m.u8(s) != 0 {
                                    let c = m.u8(s);
                                    m.set_u8(dst, c);
                                    dst = dst.wrapping_add(1);
                                    s = s.wrapping_add(1);
                                }
                                m.set_u8(dst, 0);
                            }
                        }
                        _ => {}
                    }
                    v = m.u32(v.wrapping_add(0x44));
                }
                let r = o.call(m, SI_CUS_CMD_HOOK, &[status, eff, cmd, time, 0, flag, 0]).eax;
                if (r as i32) < 0 {
                    return fail;
                }
            }
            4..=10 | 0x12 => {
                if models != 0 && mi < n {
                    let mdl = m.u32(models.wrapping_add((mi as u32).wrapping_mul(4)));
                    m.set_u32(cmd.wrapping_add(0x1c), mdl);
                } else {
                    let got = sscanf_mem(m, s, S_FMT_SS, &[tag, name]);
                    if got != 2 {
                        fprintf(m, o, err, S_BAD_SCRIPT, &[]);
                        return fail;
                    }
                    let found = o.call(m, SI_CUS_FIND_MODEL_BY_NAME, &[name]).eax;
                    if found == 0 {
                        fprintf(m, o, err, S_NO_MODEL, &[name]);
                        return fail;
                    }
                    m.set_u32(cmd.wrapping_add(0x1c), found);
                }
                mi += 1;
                off = off.wrapping_add(0xa0);
                if kind == 0x12 {
                    read_and_dispose_model(m, o, cmd);
                }
            }
            0x1e => {
                m.set_u32(eff.wrapping_add(0xc), model);
                m.set_u32(cmd.wrapping_add(0x1c), model);
                off = off.wrapping_add(0xa0);
                read_and_dispose_model(m, o, cmd);
            }
            0xd => {
                sscanf_mem(m, s, S_FMT_SD, &[tag, lenc]);
                off = off.wrapping_add(0x28);
                let len = m.u32(lenc);
                m.set_u32(cmd.wrapping_add(0x20), len);
                s = base.wrapping_add(off as u32);
                if (len as i32) > 0 {
                    let p = m.malloc(len);
                    m.set_u32(cmd.wrapping_add(0x24), p);
                    for i in 0..len {
                        let c = m.u8(s.wrapping_add(i));
                        m.set_u8(p.wrapping_add(i), c);
                    }
                    off = off.wrapping_add(len as i32);
                }
                let pipe = m.u32(cmd.wrapping_add(0x2c));
                let fp = if pipe == 0 {
                    o.call(m, PLT_FOPEN, &[m.u32(cmd.wrapping_add(8)), S_MODE_W]).eax
                } else if (m.u32(pipe.wrapping_add(0x18)) as i32) < 0 {
                    0
                } else {
                    m.u32(pipe.wrapping_add(0x14))
                };
                if fp == 0 {
                    fprintf(m, o, err, S_NO_TAG_FILE, &[]);
                    return fail;
                }
                o.call(m, SI_BYTESWAP_FWRITE, &[lenc, 4, 1, fp]);
                if (m.u32(cmd.wrapping_add(0x20)) as i32) > 0 {
                    let buf = m.u32(cmd.wrapping_add(0x24));
                    o.call(m, SI_BYTESWAP_FWRITE, &[buf, 1, m.u32(lenc), fp]);
                }
                o.call(m, crate::custom_enc::PLT_FFLUSH, &[fp]);
                if m.u32(cmd.wrapping_add(0x2c)) == 0 {
                    o.call(m, crate::custom_udf::PLT_FCLOSE, &[fp]);
                }
            }
            k if plain_command(k) => {
                let r = o.call(m, SI_CUS_CMD_HOOK, &[status, eff, cmd, time, 0, flag, 0]).eax;
                if (r as i32) < 0 {
                    return fail;
                }
            }
            _ => {}
        }
        cmd = m.u32(cmd.wrapping_add(0x30));
    }
    if m.u32(eff.wrapping_add(0xc)) != 0 {
        let at = m.u32(hdr.wrapping_add(4));
        o.call(m, crate::custom_mn::SI_CUS_ADD_MOTION_EFFECT, &[status, eff, at]);
    }
    if m.u32(eff.wrapping_add(0xc)) == 0 {
        o.call(m, crate::custom_mn::SI_CUS_CLOSE_PIPES, &[eff]);
    }
    let un = si_cus_model_search_user_data(m, model, S_CUSC);
    if un != 0 {
        let data = m.u32(un.wrapping_add(0xc));
        let size = m.u32(un.wrapping_add(8));
        let mut complaint = 0;
        if nflt == 0 || size != m.u32(hdr.wrapping_add(0x34)) {
            complaint = S_FCURVE_SIZES;
        } else {
            let count = size / (nflt as u32).wrapping_mul(4).wrapping_add(4);
            let mut e = m.u32(m.u32(status.wrapping_add(0x14c)).wrapping_add(0xfc));
            while e != 0 && m.u32(e.wrapping_add(0xc)) != model {
                e = m.u32(e.wrapping_add(0xfc));
            }
            if e == 0 {
                complaint = S_STRANGE;
            } else {
                let mut vals = data.wrapping_add(count.wrapping_mul(4));
                let mut v = m.u32(e.wrapping_add(0x64));
                while v != 0 {
                    if m.u16(v.wrapping_add(4)) == 1 {
                        let fc = o.call(m, crate::custom_util::SI_FCURVE_ALLOCATE, &[0, v]).eax;
                        m.set_u32(v.wrapping_add(0x2c), fc);
                        m.set_u16(fc.wrapping_add(0x1e), 2);
                        for k in 0..count {
                            let a = through_x87(m.u32(data.wrapping_add(k.wrapping_mul(4))));
                            let b = through_x87(m.u32(vals.wrapping_add(k.wrapping_mul(4))));
                            let key = o.call(m, crate::custom_parse::SI_FKEY_CREATE, &[a, b, 0, 0, 2]).eax;
                            let cur = m.u32(v.wrapping_add(0x2c));
                            o.call(m, crate::custom_util::SI_FCURVE_ADD_KEY, &[cur, key]);
                        }
                        vals = vals.wrapping_add(count.wrapping_mul(4));
                    }
                    v = m.u32(v.wrapping_add(0x44));
                }
            }
        }
        if complaint != 0 {
            fprintf(m, o, err, complaint, &[]);
        }
    }
    if m.u16(flag) != 0 && m.u16(REFRESH_HOLD) == 0 {
        o.call(m, crate::custom_mn::SI_REFRESH_ALL, &[1]);
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(rel: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
    }

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    /// The model against glibc 2.2.5 (the C library of the oracle's sandbox, run by `re/tools/cusrel1/scanref.sh`): the return
    /// value and every destination buffer, after each of the made-up cases. Skipped when the reference files are absent.
    #[test]
    fn same_as_glibc_2_2_5() {
        let (Ok(fm), Ok(cases), Ok(out)) = (
            std::fs::read_to_string(dir("re/tools/cusrel1/formats.txt")),
            std::fs::read_to_string(dir("re/out/cusrel1/scanref_cases.txt")),
            std::fs::read_to_string(dir("re/out/cusrel1/scanref_out.txt")),
        ) else {
            eprintln!("custom_rel: no scanref files, skipped");
            return;
        };
        let formats: Vec<(String, Vec<usize>)> = fm
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (f, s) = l.split_once('\t').unwrap();
                (f.to_string(), s.split(',').map(|x| x.parse().unwrap()).collect())
            })
            .collect();
        let (mut same, mut bad, mut skipped) = (0, Vec::new(), 0);
        for (c, o) in cases.lines().zip(out.lines()) {
            let (idx, hex) = c.split_once(' ').unwrap();
            let (fmt, sizes) = &formats[idx.parse::<usize>().unwrap()];
            // Known difference: a field width on a float conversion (`%2f`) is not fitted to glibc 2.2.5. The original's formats
            // never put a width on `%f`, so those formats are left out of the comparison (see the finding).
            if fmt.as_bytes().windows(2).any(|w| w[0].is_ascii_digit() && w[1] == b'f') {
                skipped += 1;
                continue;
            }
            let text = unhex(hex);
            let r = scan(&text, fmt.as_bytes());
            let mut line = format!("{}", r.ret);
            for (i, &n) in sizes.iter().enumerate() {
                let mut b = vec![0xAAu8; n];
                if let Some(st) = r.stores.get(i) {
                    let sb = store_bytes(st);
                    b[..sb.len().min(n)].copy_from_slice(&sb[..sb.len().min(n)]);
                }
                line.push('|');
                for x in b {
                    line.push_str(&format!("{:02x}", x));
                }
            }
            if line == o {
                same += 1;
            } else if bad.len() < 15 {
                bad.push(format!("{:?} {:?}\n   model {}\n   glibc {}", fmt, String::from_utf8_lossy(&text), line, o));
            }
        }
        eprintln!("custom_rel: {} identical, {} float-width cases not compared, of {}", same, skipped, cases.lines().count());
        assert!(bad.is_empty(), "{} identical; first differences:\n{}", same, bad.join("\n"));
    }

    /// The original's `long` has 32 bits: `%d` of a larger number is the `int` limit (strtol), `%hd` of it is the low 16 bits of
    /// that, `%u` of a negative number wraps.
    #[test]
    fn clamps_like_a_32_bit_long() {
        let o = scan(b"x 99999999999", b"%s %d");
        assert_eq!(o.stores[1], ScanStore::Int { bytes: 4, val: 0x7fff_ffff });
        let o = scan(b"x -99999999999", b"%s %d");
        assert_eq!(o.stores[1], ScanStore::Int { bytes: 4, val: 0x8000_0000 });
        let o = scan(b"x 99999999999", b"%s %hd");
        assert_eq!(store_bytes(&o.stores[1]), vec![0xff, 0xff]);
        let o = scan(b"-5", b"%u");
        assert_eq!(o.stores[0], ScanStore::Int { bytes: 4, val: 0xffff_fffb });
        let o = scan(b"99999999999", b"%u");
        assert_eq!(o.stores[0], ScanStore::Int { bytes: 4, val: 0xffff_ffff });
    }
}
