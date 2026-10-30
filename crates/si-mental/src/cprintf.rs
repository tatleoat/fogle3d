// SPDX-License-Identifier: MIT
//! The C library's formatted output (glibc 2.2's `vfprintf` conversions) for the image: the text a
//! `printf`-family call writes, the arguments being the stack words the caller pushed (a `double` two
//! words, low word first; a `long long` two; a `long double` three). Shared by the translator's text
//! writers (`mi_error.c`'s `sfatal`/`swarning`/`sdebug`, the `sprintf` calls of `mi_mr_main.c`).
//!
//! Nothing here reproduces a function of the original: it is the library's behaviour, written to match
//! glibc (the `printf-check` binary compares it with the host's `snprintf` on generated cases).
//! A missing argument word reads as 0; an unknown conversion is copied through, as glibc does.

use si_mem::Mem;

struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    prec: Option<usize>,
}

struct Args<'a> {
    w: &'a [u32],
    i: usize,
}

impl Args<'_> {
    fn word(&mut self) -> u32 {
        let v = self.w.get(self.i).copied().unwrap_or(0);
        self.i += 1;
        v
    }
    fn quad(&mut self) -> u64 {
        let lo = self.word() as u64;
        let hi = self.word() as u64;
        lo | hi << 32
    }
}

/// The text `vsprintf(.., fmt, args)` writes for the format string `fmt` (without its terminator).
pub fn format_bytes(m: &mut Mem, fmt: &[u8], words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut a = Args { w: words, i: 0 };
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let mut s = Spec { left: false, plus: false, space: false, alt: false, zero: false, width: 0, prec: None };
        while i < fmt.len() {
            match fmt[i] {
                b'-' => s.left = true,
                b'+' => s.plus = true,
                b' ' => s.space = true,
                b'#' => s.alt = true,
                b'0' => s.zero = true,
                b'\'' => {}
                _ => break,
            }
            i += 1;
        }
        if i < fmt.len() && fmt[i] == b'*' {
            let v = a.word() as i32;
            if v < 0 {
                s.left = true;
            }
            s.width = v.unsigned_abs() as usize;
            i += 1;
        } else {
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                s.width = s.width.saturating_mul(10).saturating_add((fmt[i] - b'0') as usize);
                i += 1;
            }
        }
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            if i < fmt.len() && fmt[i] == b'*' {
                let v = a.word() as i32;
                s.prec = if v < 0 { None } else { Some(v as usize) };
                i += 1;
            } else {
                let mut p = 0usize;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    p = p.saturating_mul(10).saturating_add((fmt[i] - b'0') as usize);
                    i += 1;
                }
                s.prec = Some(p);
            }
        }
        // length: 0 plain, 1 h, 2 hh, 3 l (32 bits here), 4 ll / q / j (64 bits), 5 L
        let mut len = 0;
        while i < fmt.len() && matches!(fmt[i], b'h' | b'l' | b'L' | b'q' | b'z' | b't' | b'j') {
            len = match (fmt[i], len) {
                (b'h', 1) => 2,
                (b'h', _) => 1,
                (b'l', 3) => 4,
                (b'l', _) => 3,
                (b'q' | b'j', _) => 4,
                (b'L', _) => 5,
                _ => 3,
            };
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            out.extend_from_slice(&fmt[start..]);
            break;
        };
        i += 1;
        match conv {
            b'%' => out.push(b'%'),
            b'd' | b'i' => {
                let v: i64 = match len {
                    1 => a.word() as i16 as i64,
                    2 => a.word() as i8 as i64,
                    4 => a.quad() as i64,
                    _ => a.word() as i32 as i64,
                };
                let digits = v.unsigned_abs().to_string().into_bytes();
                let sign: &[u8] = if v < 0 { b"-" } else if s.plus { b"+" } else if s.space { b" " } else { b"" };
                integer(&mut out, &s, sign, b"", digits, v == 0);
            }
            b'u' | b'o' | b'x' | b'X' => {
                let v: u64 = match len {
                    1 => a.word() as u16 as u64,
                    2 => a.word() as u8 as u64,
                    4 => a.quad(),
                    _ => a.word() as u64,
                };
                let mut digits = match conv {
                    b'u' => v.to_string(),
                    b'o' => format!("{v:o}"),
                    b'x' => format!("{v:x}"),
                    _ => format!("{v:X}"),
                }
                .into_bytes();
                let mut prefix: &[u8] = b"";
                if s.alt && v != 0 {
                    match conv {
                        b'x' => prefix = b"0x",
                        b'X' => prefix = b"0X",
                        _ => {}
                    }
                }
                if s.alt && conv == b'o' && (digits[0] != b'0') && !(v == 0) {
                    // '#' raises the precision so that the first digit is a zero
                    digits.insert(0, b'0');
                }
                if s.alt && conv == b'o' && v == 0 && s.prec == Some(0) {
                    digits = b"0".to_vec();
                    integer(&mut out, &s, b"", b"", digits, false);
                } else {
                    integer(&mut out, &s, b"", prefix, digits, v == 0);
                }
            }
            b'c' => {
                let c = a.word() as u8;
                pad(&mut out, &s, &[c], false);
            }
            b's' => {
                let p = a.word();
                let text: Vec<u8> = if p == 0 {
                    if s.prec.is_none_or(|n| n >= 6) { b"(null)".to_vec() } else { Vec::new() }
                } else {
                    let mut t = Vec::new();
                    let mut q = p;
                    while s.prec.is_none_or(|n| t.len() < n) {
                        let b = m.u8(q);
                        if b == 0 {
                            break;
                        }
                        t.push(b);
                        q = q.wrapping_add(1);
                    }
                    t
                };
                pad(&mut out, &s, &text, false);
            }
            b'p' => {
                let p = a.word();
                let text = if p == 0 { b"(nil)".to_vec() } else { format!("0x{p:x}").into_bytes() };
                pad(&mut out, &s, &text, false);
            }
            b'n' => {
                let p = a.word();
                let n = out.len() as u32;
                match len {
                    1 => m.set_u16(p, n as u16),
                    2 => m.set_u8(p, n as u8),
                    _ => m.set_u32(p, n),
                }
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
                let v: f64 = if len == 5 {
                    let (lo, hi, ex) = (a.word() as u64, a.word() as u64, a.word());
                    si_x87::F80::from_bits(lo | hi << 32, ex as u16).to_f64()
                } else {
                    f64::from_bits(a.quad())
                };
                float(&mut out, &s, conv, v);
            }
            _ => out.extend_from_slice(&fmt[start..i]),
        }
    }
    out
}

/// `format_bytes` of the NUL-terminated format string in the image.
pub fn format(m: &mut Mem, fmt: u32, words: &[u32]) -> Vec<u8> {
    let f = m.cstr(fmt);
    format_bytes(m, &f, words)
}

fn pad(out: &mut Vec<u8>, s: &Spec, body: &[u8], _num: bool) {
    let fill = s.width.saturating_sub(body.len());
    if s.left {
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(body);
    }
}

/// An integer: sign, prefix, digits widened to the precision, then the width (zeros when `0` and no precision).
fn integer(out: &mut Vec<u8>, s: &Spec, sign: &[u8], prefix: &[u8], mut digits: Vec<u8>, is_zero: bool) {
    if let Some(p) = s.prec {
        if p == 0 && is_zero {
            digits.clear();
        }
        while digits.len() < p {
            digits.insert(0, b'0');
        }
    }
    let used = sign.len() + prefix.len() + digits.len();
    let fill = s.width.saturating_sub(used);
    if s.left {
        out.extend_from_slice(sign);
        out.extend_from_slice(prefix);
        out.extend_from_slice(&digits);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else if s.zero && s.prec.is_none() {
        out.extend_from_slice(sign);
        out.extend_from_slice(prefix);
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(&digits);
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(sign);
        out.extend_from_slice(prefix);
        out.extend_from_slice(&digits);
    }
}

/// `%e` digits: `d.ddde+XX` (at least two exponent digits).
fn exp_form(v: f64, prec: usize, upper: bool, alt: bool) -> String {
    let t = format!("{v:.prec$e}");
    let (mant, ex) = t.split_once('e').unwrap();
    let ex: i32 = ex.parse().unwrap();
    let mut mant = mant.to_string();
    if alt && prec == 0 {
        mant.push('.');
    }
    let r = format!("{mant}e{}{:02}", if ex < 0 { '-' } else { '+' }, ex.abs());
    if upper { r.to_uppercase() } else { r }
}

fn float(out: &mut Vec<u8>, s: &Spec, conv: u8, v: f64) {
    let upper = conv.is_ascii_uppercase();
    let neg = v.is_sign_negative();
    let sign: &[u8] = if neg { b"-" } else if s.plus { b"+" } else if s.space { b" " } else { b"" };
    let a = v.abs();
    let body: String = if !v.is_finite() {
        let t = if v.is_nan() { "nan" } else { "inf" };
        let t = if upper { t.to_uppercase() } else { t.to_string() };
        let mut o = sign.to_vec();
        o.extend_from_slice(t.as_bytes());
        pad(out, &Spec { zero: false, ..*s }, &o, true);
        return;
    } else {
        let prec = s.prec.unwrap_or(6);
        match conv.to_ascii_lowercase() {
            b'f' => {
                let mut t = format!("{a:.prec$}");
                if s.alt && prec == 0 {
                    t.push('.');
                }
                t
            }
            b'e' => exp_form(a, prec, upper, s.alt),
            b'a' => hex_float(a, s.prec, upper),
            _ => {
                let p = if prec == 0 { 1 } else { prec };
                // the exponent the %e conversion with P-1 digits would show decides the style
                let x: i32 = {
                    let t = format!("{a:.*e}", p - 1);
                    t.split_once('e').unwrap().1.parse().unwrap()
                };
                let mut t = if x >= -4 && (x as i64) < p as i64 {
                    format!("{a:.*}", (p as i64 - 1 - x as i64) as usize)
                } else {
                    exp_form(a, p - 1, false, false)
                };
                if !s.alt {
                    let (m, e) = match t.find('e') {
                        Some(k) => (t[..k].to_string(), t[k..].to_string()),
                        None => (t.clone(), String::new()),
                    };
                    let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.').to_string() } else { m };
                    t = m + &e;
                } else if !t.contains('.') {
                    match t.find('e') {
                        Some(k) => t.insert(k, '.'),
                        None => t.push('.'),
                    }
                }
                if upper { t.to_uppercase() } else { t }
            }
        }
    };
    let used = sign.len() + body.len();
    let fill = s.width.saturating_sub(used);
    if s.left {
        out.extend_from_slice(sign);
        out.extend_from_slice(body.as_bytes());
        out.extend(std::iter::repeat_n(b' ', fill));
    } else if s.zero {
        out.extend_from_slice(sign);
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(body.as_bytes());
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(sign);
        out.extend_from_slice(body.as_bytes());
    }
}

/// `%a` for a finite non-negative double (glibc: `0x1.8p+1`, `0x0p+0` for zero).
fn hex_float(a: f64, prec: Option<usize>, upper: bool) -> String {
    let bits = a.to_bits();
    let ex = ((bits >> 52) & 0x7ff) as i32;
    let mut frac = bits & ((1u64 << 52) - 1);
    let (lead, e) = if ex == 0 { if frac == 0 { (0u64, 0) } else { (0, -1022) } } else { (1, ex - 1023) };
    let mut lead = lead;
    let mut digits = 13usize;
    let mut f = frac;
    if let Some(p) = prec {
        if p < 13 {
            let shift = 4 * (13 - p) as u32;
            let rem = frac & ((1u64 << shift) - 1);
            frac >>= shift;
            let half = 1u64 << (shift - 1);
            if rem > half || (rem == half && (frac & 1 == 1 || (p == 0 && lead & 1 == 1))) {
                frac += 1;
                if p == 0 || frac >> (4 * p as u32) != 0 {
                    if p == 0 {
                        lead += 1;
                        frac = 0;
                    } else {
                        frac = 0;
                        lead += 1;
                    }
                }
            }
            f = frac;
            digits = p;
        }
    }
    let mut hex = if digits == 0 { String::new() } else { format!("{f:0digits$x}") };
    if prec.is_none() {
        hex = hex.trim_end_matches('0').to_string();
    } else if let Some(p) = prec {
        while hex.len() < p {
            hex.push('0');
        }
    }
    let dot = if hex.is_empty() { "" } else { "." };
    let t = format!("0x{lead}{dot}{hex}p{}{}", if e < 0 { '-' } else { '+' }, e.abs());
    if upper { t.to_uppercase() } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, w: &[u32]) -> String {
        let mut m = Mem::new();
        String::from_utf8(format_bytes(&mut m, fmt.as_bytes(), w)).unwrap()
    }
    fn dw(v: f64) -> [u32; 2] {
        let b = v.to_bits();
        [b as u32, (b >> 32) as u32]
    }

    #[test]
    fn integers() {
        assert_eq!(f("%d|%5d|%-5d|%05d|%+d|% d", &[5, 5, 5, 5, 5, 5]), "5|    5|5    |00005|+5| 5");
        assert_eq!(f("%x %X %#x %o %#o %u", &[255, 255, 255, 8, 8, 0xffff_ffff]), "ff FF 0xff 10 010 4294967295");
        assert_eq!(f("%.3d|%.0d|%hd|%hhd", &[7, 0, 0x1_0005, 0x1_0105]), "007||5|5");
        assert_eq!(f("%ld %lld", &[(-3i32) as u32, 0, 1, 1]), "-3 4294967296");
    }

    #[test]
    fn strings_and_chars() {
        let mut m = Mem::new();
        let p = m.malloc(8);
        m.set_bytes(p, b"abc\0");
        let mut o = |fmt: &str, w: &[u32]| String::from_utf8(format_bytes(&mut m, fmt.as_bytes(), w)).unwrap();
        assert_eq!(o("%s|%5s|%-5s|%.2s|%c", &[p, p, p, p, 65]), "abc|  abc|abc  |ab|A");
        assert_eq!(o("%s|%.3s|%.0s", &[0, 0, 0]), "(null)||");
        assert_eq!(o("%p %p", &[0, 0x1234]), "(nil) 0x1234");
    }

    #[test]
    fn doubles() {
        let w = |v: f64| dw(v);
        assert_eq!(f("%f", &w(1.5)), "1.500000");
        assert_eq!(f("%g|%g|%g|%g", &[w(100000.0), w(1e6), w(0.0001), w(0.00001)].concat()), "100000|1e+06|0.0001|1e-05");
        assert_eq!(f("%.3g|%e|%.2e|%10.3f|%-10.1f|", &[w(3.14159), w(12345.678), w(0.5), w(2.0), w(2.25)].concat()), "3.14|1.234568e+04|5.00e-01|     2.000|2.2       |");
        assert_eq!(f("%f %f %g", &[w(f64::INFINITY), w(f64::NAN), w(-0.0)].concat()), "inf nan -0");
        assert_eq!(f("%08.3f|%+.1f", &[w(-3.14159), w(2.0)].concat()), "-003.142|+2.0");
        assert_eq!(f("%a", &w(1.5)), "0x1.8p+0");
    }
}
