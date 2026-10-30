// SPDX-License-Identifier: MIT
//! Small pieces of the C library the shader-database sources use, written for the image:
//! `sprintf` for `%s`, `%d`, `%f` and literal text, and `strtod` on decimal text. Nothing
//! here reproduces a function of the original.

use si_mem::{clib, Mem};

/// The text `sprintf(.., fmt, ..)` writes for the conversions `%s` `%d` `%i` `%u` `%x` `%c`
/// `%f` `%%` (with a width, `-`, `0` and `.precision`), the arguments being the stack words
/// (a double is two words).
pub fn format(m: &Mem, fmt: u32, args: &[u32]) -> Vec<u8> {
    let f = m.cstr(fmt);
    let mut out = Vec::new();
    let mut ai = 0usize;
    let next = |ai: &mut usize| -> u32 {
        let w = args.get(*ai).copied().unwrap_or(0);
        *ai += 1;
        w
    };
    let mut i = 0;
    while i < f.len() {
        if f[i] != b'%' {
            out.push(f[i]);
            i += 1;
            continue;
        }
        i += 1;
        let (mut left, mut zero) = (false, false);
        while i < f.len() && matches!(f[i], b'-' | b'0') {
            if f[i] == b'-' {
                left = true
            } else {
                zero = true
            }
            i += 1;
        }
        let mut width = 0usize;
        while i < f.len() && f[i].is_ascii_digit() {
            width = width * 10 + (f[i] - b'0') as usize;
            i += 1;
        }
        let mut prec: Option<usize> = None;
        if i < f.len() && f[i] == b'.' {
            i += 1;
            let mut p = 0;
            while i < f.len() && f[i].is_ascii_digit() {
                p = p * 10 + (f[i] - b'0') as usize;
                i += 1;
            }
            prec = Some(p);
        }
        while i < f.len() && matches!(f[i], b'l' | b'h') {
            i += 1;
        }
        let Some(&conv) = f.get(i) else { break };
        i += 1;
        let body: Vec<u8> = match conv {
            b'%' => b"%".to_vec(),
            b'd' | b'i' => (next(&mut ai) as i32).to_string().into_bytes(),
            b'u' => next(&mut ai).to_string().into_bytes(),
            b'x' => format!("{:x}", next(&mut ai)).into_bytes(),
            b'X' => format!("{:X}", next(&mut ai)).into_bytes(),
            b'c' => vec![next(&mut ai) as u8],
            b's' => {
                let a = next(&mut ai);
                let mut s = if a == 0 { b"(null)".to_vec() } else { m.cstr(a) };
                if let Some(p) = prec {
                    s.truncate(p);
                }
                s
            }
            b'f' => {
                let lo = next(&mut ai) as u64;
                let hi = next(&mut ai) as u64;
                let v = f64::from_bits(lo | hi << 32);
                if v.is_nan() {
                    (if v.is_sign_negative() { "-nan" } else { "nan" }).to_string().into_bytes()
                } else if v.is_infinite() {
                    (if v < 0.0 { "-inf" } else { "inf" }).to_string().into_bytes()
                } else {
                    format!("{:.*}", prec.unwrap_or(6), v).into_bytes()
                }
            }
            _ => Vec::new(),
        };
        let pad = width.saturating_sub(body.len());
        if left {
            out.extend_from_slice(&body);
            out.extend(std::iter::repeat(b' ').take(pad));
        } else if zero && conv != b's' {
            let (sign, digits) = if body.first() == Some(&b'-') { (&body[..1], &body[1..]) } else { (&body[..0], &body[..]) };
            out.extend_from_slice(sign);
            out.extend(std::iter::repeat(b'0').take(pad));
            out.extend_from_slice(digits);
        } else {
            out.extend(std::iter::repeat(b' ').take(pad));
            out.extend_from_slice(&body);
        }
    }
    out
}

/// `sprintf(dst, fmt, ..)` on the image: writes the text and its NUL, answers its length.
pub fn sprintf(m: &mut Mem, dst: u32, fmt: u32, args: &[u32]) -> u32 {
    let t = format(m, fmt, args);
    m.put_cstr(dst, &t);
    t.len() as u32
}

/// `strtod(s, 0)` for the decimal text the scanner hands it (blanks, a sign, digits, a
/// point, an exponent); no hexadecimal, `inf` or `nan` forms.
pub fn strtod(m: &Mem, s: u32) -> f64 {
    let b = m.cstr(s);
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let start = i;
    if i < b.len() && matches!(b[i], b'+' | b'-') {
        i += 1;
    }
    let digits_at = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut ndig = i - digits_at;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        ndig += i - f;
    }
    if ndig == 0 {
        return 0.0;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < b.len() && matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        let e = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > e {
            i = j;
        }
    }
    let text = std::str::from_utf8(&b[start..i]).unwrap_or("0");
    let t = if text.starts_with('+') { &text[1..] } else { text };
    t.parse::<f64>().unwrap_or(0.0)
}

/// `strlen` for callers that hold only the image.
pub fn len(m: &Mem, s: u32) -> u32 {
    clib::strlen(m, s)
}
