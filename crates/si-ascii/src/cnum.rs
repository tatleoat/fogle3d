// SPDX-License-Identifier: MIT
//! The C library's number conversions as the ASCII readers and writers use them: `atoi`,
//! `atof`, `sscanf("%f")` and `printf("%.Ng")`.
//!
//! These are not Softimage functions (the program calls glibc), so nothing here carries a
//! citation; the readers that call them do. Behaviour is that of the C locale. Two corners
//! depend on the glibc release the program ran against and are noted where they occur.

/// C's `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `atoi(s)`, which glibc defines as `(int)strtol(s, NULL, 10)`: leading white space, an
/// optional sign, decimal digits. No digits gives 0. On the 32-bit build `long` is 32 bits, so
/// a value out of range is clamped to `i32::MAX` or `i32::MIN`.
pub fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v * 10 + (s[i] - b'0') as i64;
        if v > i32::MAX as i64 + 1 {
            v = i32::MAX as i64 + 1; // saturate; keeps scanning as strtol does
        }
        i += 1;
    }
    if neg {
        (-v).max(i32::MIN as i64) as i32
    } else {
        v.min(i32::MAX as i64) as i32
    }
}

/// What a floating-point scan found at the start of a string.
struct Scanned {
    /// The value, without its sign, as a double and as a single (each rounded once from the
    /// text, as `strtod` and `strtof` do).
    f64: f64,
    f32: f32,
    neg: bool,
}

/// `m × 2^e` rounded once to a double (`sticky`: non-zero bits were dropped below `m`).
fn scale2(m: u64, e: i32, sticky: bool) -> f64 {
    if m == 0 {
        return 0.0;
    }
    let top = 63 - m.leading_zeros() as i32; // position of the leading bit
    let exp = top + e; // value is in [2^exp, 2^(exp+1))
    if exp > 1023 {
        return f64::INFINITY;
    }
    // bits that fit: 53 for a normal number, fewer below 2^-1022
    let keep = if exp >= -1022 { 53 } else { 53 - (-1022 - exp) };
    if keep <= 0 {
        // at or below half the smallest subnormal
        let half = keep == 0 && (m > (1u64 << top) || sticky);
        return if half { f64::from_bits(1) } else { 0.0 };
    }
    let drop = top + 1 - keep;
    let (mut q, round_up) = if drop <= 0 {
        (m << (-drop), false)
    } else {
        let q = m >> drop;
        let rem = m & ((1u64 << drop) - 1);
        let half = 1u64 << (drop - 1);
        (q, rem > half || (rem == half && (sticky || q & 1 == 1)))
    };
    if round_up {
        q += 1;
    }
    // q × 2^(e + drop): q has at most 54 bits, so the conversion and the scaling are exact
    let mut v = q as f64;
    let mut p = e + drop;
    while p > 0 {
        let step = p.min(1000);
        v *= 2f64.powi(step);
        p -= step;
    }
    while p < 0 {
        let step = (-p).min(1000);
        v /= 2f64.powi(step);
        p += step;
    }
    v
}

/// The longest prefix of `s` that `strtod` converts, after white space: sign, then a decimal
/// number, a hexadecimal one (`0x…`), `inf`, `infinity` or `nan` (with an optional
/// parenthesised tail). `None`: no conversion.
fn scan(s: &[u8]) -> Option<Scanned> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let rest = &s[i..];
    let lower = |n: usize| -> Vec<u8> { rest.iter().take(n).map(|b| b.to_ascii_lowercase()).collect() };
    if lower(3) == b"inf" {
        return Some(Scanned { f64: f64::INFINITY, f32: f32::INFINITY, neg });
    }
    if lower(3) == b"nan" {
        return Some(Scanned { f64: f64::NAN, f32: f32::NAN, neg });
    }
    // hexadecimal: 0x, then hex digits with an optional point, then an optional binary exponent
    if rest.len() >= 2 && rest[0] == b'0' && (rest[1] == b'x' || rest[1] == b'X') {
        let h = &rest[2..];
        let mut j = 0;
        let (mut m, mut e, mut sticky, mut digits, mut seen_dot) = (0u64, 0i32, false, 0, false);
        while j < h.len() {
            let d = match h[j] {
                b'0'..=b'9' => h[j] - b'0',
                b'a'..=b'f' => h[j] - b'a' + 10,
                b'A'..=b'F' => h[j] - b'A' + 10,
                b'.' if !seen_dot => {
                    seen_dot = true;
                    j += 1;
                    continue;
                }
                _ => break,
            };
            digits += 1;
            if m >> 60 == 0 {
                m = m << 4 | d as u64;
                if seen_dot {
                    e -= 4;
                }
            } else {
                sticky |= d != 0;
                if !seen_dot {
                    e += 4;
                }
            }
            j += 1;
        }
        if digits > 0 {
            if j < h.len() && (h[j] == b'p' || h[j] == b'P') {
                let mut k = j + 1;
                let mut eneg = false;
                if k < h.len() && (h[k] == b'+' || h[k] == b'-') {
                    eneg = h[k] == b'-';
                    k += 1;
                }
                if k < h.len() && h[k].is_ascii_digit() {
                    let mut x: i32 = 0;
                    while k < h.len() && h[k].is_ascii_digit() {
                        x = x.saturating_mul(10).saturating_add((h[k] - b'0') as i32).min(100_000);
                        k += 1;
                    }
                    e = e.saturating_add(if eneg { -x } else { x });
                }
            }
            let v = scale2(m, e, sticky);
            // the single goes through the double here: a second rounding that strtof does not
            // make. Hexadecimal numbers do not occur in the files.
            return Some(Scanned { f64: v, f32: v as f32, neg });
        }
        // "0x" with no hexadecimal digit: strtod converts the "0"
        return Some(Scanned { f64: 0.0, f32: 0.0, neg });
    }
    // decimal: digits, optional point, digits; at least one digit; then an optional exponent
    let mut j = 0;
    let mut digits = 0;
    while j < rest.len() && rest[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if j < rest.len() && rest[j] == b'.' {
        let mut k = j + 1;
        let mut frac = 0;
        while k < rest.len() && rest[k].is_ascii_digit() {
            k += 1;
            frac += 1;
        }
        if digits + frac > 0 {
            j = k;
            digits += frac;
        }
    }
    if digits == 0 {
        return None;
    }
    if j < rest.len() && (rest[j] == b'e' || rest[j] == b'E') {
        let mut k = j + 1;
        if k < rest.len() && (rest[k] == b'+' || rest[k] == b'-') {
            k += 1;
        }
        if k < rest.len() && rest[k].is_ascii_digit() {
            while k < rest.len() && rest[k].is_ascii_digit() {
                k += 1;
            }
            j = k;
        }
    }
    // Rust's parsers round correctly, as glibc's strtod and strtof do; they accept exactly
    // this syntax ("1.", ".5" and "1e5" included)
    let text = std::str::from_utf8(&rest[..j]).ok()?;
    Some(Scanned { f64: text.parse().ok()?, f32: text.parse().ok()?, neg })
}

/// `atof(s)`, that is `strtod(s, NULL)`: 0.0 when nothing converts.
pub fn atof(s: &[u8]) -> f64 {
    match scan(s) {
        Some(x) => {
            if x.neg {
                -x.f64
            } else {
                x.f64
            }
        }
        None => 0.0,
    }
}

/// `strtoul(s, NULL, 10)` on a 32-bit build: blanks, a sign, digits. A value above
/// `ULONG_MAX` gives `ULONG_MAX` (the sign is then not applied); otherwise a minus sign
/// negates modulo 2³². 0 when there is no digit.
pub fn strtoul(s: &[u8]) -> u32 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let (mut v, mut over) = (0u64, false);
    while i < s.len() && s[i].is_ascii_digit() {
        v = v * 10 + (s[i] - b'0') as u64;
        if v > u32::MAX as u64 {
            over = true;
            v = u32::MAX as u64;
        }
        i += 1;
    }
    if over {
        u32::MAX
    } else if neg {
        (v as u32).wrapping_neg()
    } else {
        v as u32
    }
}

/// `sscanf(s, "%f", &v)`: `None` when nothing is stored.
///
/// glibc gathers the characters that can belong to a number and converts them with `strtof`,
/// so the value is the text rounded once to a single (unlike `(float)atof`). The gathering
/// differs from `strtod` in corners that cannot occur in a version number: glibc 2.2 rejects
/// a hexadecimal number without a `p` exponent and anything after a sign that is not a digit;
/// this function follows `strtod`'s syntax there. Those corners are from the glibc sources as
/// remembered, not from the binary (libc is linked dynamically).
pub fn scan_f(s: &[u8]) -> Option<f32> {
    scan(s).map(|x| if x.neg { -x.f32 } else { x.f32 })
}

/// `printf("%.{precision}g", v)`: `precision` significant digits (0 counts as 1), trailing
/// zeros removed, exponent form when the decimal exponent is below −4 or not below the
/// precision. The writers pass singles, which C widens to double first.
pub fn fmt_g(v: f64, precision: usize) -> String {
    CLib::Glibc.fmt_g(v, precision)
}

/// Whose `printf` formats a number. The program prints through the C library of the platform
/// it was built for, and the libraries disagree on one thing that shows in the files: the
/// exponent of `%g` has at least two digits on Linux and IRIX and always three with the
/// Visual C++ 6 runtime of the NT build (`1e-09` against `1e-009`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CLib {
    /// glibc (the Linux build, the reference) and IRIX's libc.
    Glibc,
    /// The Visual C++ 6 runtime (the NT build).
    Msvc,
}

impl CLib {
    /// `printf("%.{precision}g", v)` as this library prints it.
    pub fn fmt_g(self, v: f64, precision: usize) -> String {
        fmt_g_digits(v, precision, if self == CLib::Msvc { 3 } else { 2 })
    }
}

fn fmt_g_digits(v: f64, precision: usize, exp_digits: usize) -> String {
    let p = precision.max(1);
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // round to p significant digits; the exponent to test is the rounded value's
    let e = format!("{:.*e}", p - 1, v);
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if exp < -4 || exp >= p as i32 {
        format!("{}e{}{:0w$}", strip(mantissa), if exp < 0 { '-' } else { '+' }, exp.abs(), w = exp_digits)
    } else {
        strip(&format!("{:.*}", (p as i32 - 1 - exp) as usize, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoi_like_strtol() {
        for (s, v) in [("12", 12), ("  -7x", -7), ("+3", 3), ("", 0), ("abc", 0), ("12.9", 12), ("-", 0),
                       ("2147483647", i32::MAX), ("2147483648", i32::MAX), ("99999999999", i32::MAX),
                       ("-2147483648", i32::MIN), ("-2147483649", i32::MIN), ("\t\r 5", 5), ("0x10", 0), ("1e3", 1)] {
            assert_eq!(atoi(s.as_bytes()), v, "{s:?}");
        }
    }

    #[test]
    fn atof_like_strtod() {
        for (s, v) in [("1.5", 1.5), ("-2", -2.0), ("  .25abc", 0.25), ("1.", 1.0), ("1e3", 1000.0), ("1e", 1.0),
                       ("1e+", 1.0), ("x", 0.0), ("", 0.0), (".", 0.0), ("-.5e1", -5.0), ("0x10", 16.0),
                       ("0x.8p1", 1.0), ("0x1p-2", 0.25), ("0x", 0.0), ("1e400", f64::INFINITY), ("3.71", 3.71),
                       ("+7", 7.0), ("1e-400", 0.0), ("0x1p-1074", 5e-324), ("0x1p-1075", 0.0), ("0x1.8p-1074", 1e-323)] {
            assert_eq!(atof(s.as_bytes()), v, "{s:?}");
        }
        assert_eq!(atof(b"INFinity"), f64::INFINITY);
        assert_eq!(atof(b"-inf"), f64::NEG_INFINITY);
        assert!(atof(b"nan").is_nan());
    }

    #[test]
    fn strtoul_saturates_and_negates() {
        assert_eq!(strtoul(b"65535"), 65535);
        assert_eq!(strtoul(b" -1"), u32::MAX);
        assert_eq!(strtoul(b"-70000") as u16, 0xee90);
        assert_eq!(strtoul(b"4294967296"), u32::MAX);
        assert_eq!(strtoul(b"-4294967296"), u32::MAX);
        assert_eq!(strtoul(b"x"), 0);
    }

    #[test]
    fn scan_f_rounds_once() {
        assert_eq!(scan_f(b"3.71"), Some(3.71f32));
        assert_eq!(scan_f(b"2.64933"), Some(2.64933f32));
        assert_eq!(scan_f(b""), None);
        assert_eq!(scan_f(b"ELEMENTS"), None);
        assert_eq!(scan_f(b"3.5x"), Some(3.5));
        // a value where rounding the text straight to a single and going through a double
        // differ: 1 + 2^-24 + a little is above the midpoint of two singles, but the nearest
        // double is the midpoint itself, which then rounds to even
        let s = b"1.00000005960464477539062500001";
        assert_eq!(scan_f(s), Some(1.000_000_1));
        assert_eq!(atof(s) as f32, 1.0);
    }

    #[test]
    fn percent_g() {
        for (v, p, s) in [(4.0, 6, "4"), (3.71f32 as f64, 6, "3.71"), (0.0001, 6, "0.0001"), (0.00001, 6, "1e-05"),
                          (1234567.0, 6, "1.23457e+06"), (1234567.0, 7, "1234567"), (12345678.0, 7, "1.234568e+07"),
                          (0.1f32 as f64, 7, "0.1"), (0.3f32 as f64, 7, "0.3"), (1.0 / 3.0, 7, "0.3333333"),
                          (-2.5, 7, "-2.5"), (0.0, 7, "0"), (-0.0, 7, "-0"), (9999999.5, 7, "1e+07"),
                          (123.456f32 as f64, 7, "123.456"), (1e-5f32 as f64, 7, "1e-05"), (29.97f32 as f64, 7, "29.97"),
                          (0.5, 0, "0.5"), (1e10, 7, "1e+10"), (100.0, 7, "100")] {
            assert_eq!(fmt_g(v, p), s, "{v} {p}");
        }
        assert_eq!(CLib::Msvc.fmt_g(1.490116e-9, 7), "1.490116e-009");
        assert_eq!(CLib::Msvc.fmt_g(1e10, 7), "1e+010");
        assert_eq!(CLib::Msvc.fmt_g(2.5, 7), "2.5");
        assert_eq!(CLib::Glibc.fmt_g(1e-100, 7), "1e-100");
    }
}
