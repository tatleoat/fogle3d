// SPDX-License-Identifier: MIT
//! The C library's string and memory routines on the image.
//!
//! Nothing here reproduces a function of the original program: these are the library
//! routines the program calls, written once for every crate that works on the image. Each
//! behaves as the emulation harness's stand-in of the same name does
//! (`re/tools/emu/standins.py`), which is the C standard's behaviour with the choices a
//! standard leaves open made one way:
//!
//! - the comparisons (`strcmp`, `strncmp`, `strcasecmp`, `strncasecmp`, `memcmp`) answer
//!   −1, 0 or 1;
//! - `memcpy` copies as `memmove` does (the ranges may overlap), and so does `strcpy`;
//! - `strtok` and `strtok_r` leave a null pointer as the saved position when the string
//!   is used up;
//! - `qsort` is the merge sort of the C library of the time, which asks its comparison
//!   function about elements *in the array itself* in a fixed order; `bsearch` compares
//!   the key with the middle element of what is left.
//!
//! A string or a buffer at an address that is not memory is a recorded fault of the image
//! ([`crate::Access::Read`], [`crate::Access::Write`]): a string there reads as empty,
//! nothing is stored. No routine reads further than the C routine has to: a comparison
//! stops at the first difference, `strncpy` at `n` bytes.
//!
//! An address is a `u32` here, as in the vector files; a typed pointer goes in as
//! `p.addr()`. The functions `strcmp`, `strncmp`, `strcasecmp`, `strcpy`, `strcat`,
//! `strchr`, `strrchr`, `qsort`, `strtol10` and `decimal` have the names and signatures
//! of `si-dict`'s own (`crates/si-dict/src/libc.rs`), which they replace.
//!
//! Not here: `printf` and `scanf` and their families (a formatter on the image is a piece
//! of work of its own), the streams, `getenv`: those are not memory.

use crate::Mem;

fn sign(a: u8, b: u8) -> i32 {
    (a > b) as i32 - (a < b) as i32
}

/// The characters `isspace` knows in the C locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// At most `n` bytes of the string at `s`, without reading past its end.
fn strn(m: &Mem, s: u32, n: u32) -> Vec<u8> {
    let mut out = Vec::new();
    while (out.len() as u32) < n {
        let b = m.u8(s.wrapping_add(out.len() as u32));
        if b == 0 {
            break;
        }
        out.push(b);
    }
    out
}

// ---- strings

/// `strlen(s)`.
pub fn strlen(m: &Mem, s: u32) -> u32 {
    m.strlen(s)
}

/// `strcpy(dst, src)`: answers `dst`.
pub fn strcpy(m: &mut Mem, dst: u32, src: u32) -> u32 {
    let s = m.cstr(src);
    m.put_cstr(dst, &s);
    dst
}

/// `strncpy(dst, src, n)`: at most `n` bytes of the string, and zeros up to `n` bytes when
/// it is shorter (so no terminator when it is not).
pub fn strncpy(m: &mut Mem, dst: u32, src: u32, n: u32) -> u32 {
    let s = strn(m, src, n);
    m.set_bytes(dst, &s);
    m.set_fill_bytes(dst.wrapping_add(s.len() as u32), 0, n - s.len() as u32);
    dst
}

/// `strcat(dst, src)`.
pub fn strcat(m: &mut Mem, dst: u32, src: u32) -> u32 {
    let at = dst.wrapping_add(m.strlen(dst));
    strcpy(m, at, src);
    dst
}

/// `strncat(dst, src, n)`: at most `n` bytes of `src` and always a terminator.
pub fn strncat(m: &mut Mem, dst: u32, src: u32, n: u32) -> u32 {
    let at = dst.wrapping_add(m.strlen(dst));
    let s = strn(m, src, n);
    m.put_cstr(at, &s);
    dst
}

/// `strcmp(a, b)`: −1, 0 or 1.
pub fn strcmp(m: &Mem, a: u32, b: u32) -> i32 {
    let mut i = 0u32;
    loop {
        let (x, y) = (m.u8(a.wrapping_add(i)), m.u8(b.wrapping_add(i)));
        // a byte that is not memory reads as the terminator, so this ends
        if x != y || x == 0 {
            return sign(x, y);
        }
        i = i.wrapping_add(1);
    }
}

/// `strncmp(a, b, n)`.
pub fn strncmp(m: &Mem, a: u32, b: u32, n: u32) -> i32 {
    for i in 0..n {
        let (x, y) = (m.u8(a.wrapping_add(i)), m.u8(b.wrapping_add(i)));
        if x != y || x == 0 {
            return sign(x, y);
        }
    }
    0
}

/// `strcasecmp(a, b)` in the C locale.
pub fn strcasecmp(m: &Mem, a: u32, b: u32) -> i32 {
    let mut i = 0u32;
    loop {
        let (x, y) = (m.u8(a.wrapping_add(i)).to_ascii_lowercase(), m.u8(b.wrapping_add(i)).to_ascii_lowercase());
        if x != y || x == 0 {
            return sign(x, y);
        }
        i = i.wrapping_add(1);
    }
}

/// `strncasecmp(a, b, n)` in the C locale.
pub fn strncasecmp(m: &Mem, a: u32, b: u32, n: u32) -> i32 {
    for i in 0..n {
        let (x, y) = (m.u8(a.wrapping_add(i)).to_ascii_lowercase(), m.u8(b.wrapping_add(i)).to_ascii_lowercase());
        if x != y || x == 0 {
            return sign(x, y);
        }
    }
    0
}

/// `strchr(s, c)`: the address of the first `c` in the string, or null; for `c` 0 the
/// address of the terminator.
pub fn strchr(m: &Mem, s: u32, c: u8) -> u32 {
    let text = m.cstr(s);
    if c == 0 {
        return s.wrapping_add(text.len() as u32);
    }
    text.iter().position(|&b| b == c).map_or(0, |i| s.wrapping_add(i as u32))
}

/// `strrchr(s, c)`: the address of the last `c` in the string, or null.
pub fn strrchr(m: &Mem, s: u32, c: u8) -> u32 {
    let text = m.cstr(s);
    if c == 0 {
        return s.wrapping_add(text.len() as u32);
    }
    text.iter().rposition(|&b| b == c).map_or(0, |i| s.wrapping_add(i as u32))
}

/// `strstr(s, find)`: the address of the first place `find` occurs in `s`, or null; `s`
/// itself for an empty `find`.
pub fn strstr(m: &Mem, s: u32, find: u32) -> u32 {
    let (text, find) = (m.cstr(s), m.cstr(find));
    if find.is_empty() {
        return s;
    }
    text.windows(find.len()).position(|w| w == find).map_or(0, |i| s.wrapping_add(i as u32))
}

/// `strspn(s, set)`: how many bytes at the start of `s` are in `set`.
pub fn strspn(m: &Mem, s: u32, set: u32) -> u32 {
    let (text, set) = (m.cstr(s), m.cstr(set));
    text.iter().take_while(|b| set.contains(b)).count() as u32
}

/// `strcspn(s, set)`: how many bytes at the start of `s` are not in `set`.
pub fn strcspn(m: &Mem, s: u32, set: u32) -> u32 {
    let (text, set) = (m.cstr(s), m.cstr(set));
    text.iter().take_while(|b| !set.contains(b)).count() as u32
}

/// `strdup(s)`: a copy in a new block of the heap ([`Mem::strdup`]).
pub fn strdup(m: &mut Mem, s: u32) -> u32 {
    m.strdup(s)
}

/// `strtok(s, delim)`. The C library keeps the position between calls in a variable of
/// its own, which is not in the image: the caller holds it in `saved` (0 at the start).
pub fn strtok(m: &mut Mem, s: u32, delim: u32, saved: &mut u32) -> u32 {
    let (token, next) = strtok_at(m, if s == 0 { *saved } else { s }, delim);
    *saved = next;
    token
}

/// `strtok_r(s, delim, save)`: `save` is the address of the caller's `char *`.
pub fn strtok_r(m: &mut Mem, s: u32, delim: u32, save: u32) -> u32 {
    let from = if s == 0 { m.u32(save) } else { s };
    let (token, next) = strtok_at(m, from, delim);
    m.set_u32(save, next);
    token
}

/// One step of `strtok`: the token and the position to go on from (null when the string
/// is used up).
fn strtok_at(m: &mut Mem, s: u32, delim: u32) -> (u32, u32) {
    if s == 0 {
        return (0, 0);
    }
    let (text, delim) = (m.cstr(s), m.cstr(delim));
    let Some(i) = text.iter().position(|b| !delim.contains(b)) else {
        return (0, 0);
    };
    match text[i..].iter().position(|b| delim.contains(b)) {
        Some(n) => {
            let end = s.wrapping_add((i + n) as u32);
            m.set_u8(end, 0);
            (s.wrapping_add(i as u32), end.wrapping_add(1))
        }
        None => (s.wrapping_add(i as u32), 0),
    }
}

// ---- memory

/// `memcpy(dst, src, n)`: answers `dst`. The ranges may overlap, as for [`memmove`].
pub fn memcpy(m: &mut Mem, dst: u32, src: u32, n: u32) -> u32 {
    m.copy(dst, src, n);
    dst
}

/// `memmove(dst, src, n)`.
pub fn memmove(m: &mut Mem, dst: u32, src: u32, n: u32) -> u32 {
    m.copy(dst, src, n);
    dst
}

/// `bcopy(src, dst, n)`: the source comes first.
pub fn bcopy(m: &mut Mem, src: u32, dst: u32, n: u32) {
    m.copy(dst, src, n);
}

/// `memset(dst, c, n)`: answers `dst`.
pub fn memset(m: &mut Mem, dst: u32, c: u8, n: u32) -> u32 {
    m.set_fill_bytes(dst, c, n);
    dst
}

/// `bzero(dst, n)`.
pub fn bzero(m: &mut Mem, dst: u32, n: u32) {
    m.set_fill_bytes(dst, 0, n);
}

/// `memcmp(a, b, n)`: −1, 0 or 1.
pub fn memcmp(m: &Mem, a: u32, b: u32, n: u32) -> i32 {
    match m.bytes(a, n).cmp(m.bytes(b, n)) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }
}

// ---- characters and numbers

/// `toupper(c)` in the C locale.
pub fn toupper(c: i32) -> i32 {
    if (0x61..=0x7a).contains(&c) {
        c - 32
    } else {
        c
    }
}

/// `tolower(c)` in the C locale.
pub fn tolower(c: i32) -> i32 {
    if (0x41..=0x5a).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// What `strtol` reads at the start of `text` in `base` (0, or 2 to 36): the value, held
/// to ±2^100 (far beyond what any caller keeps), and the bytes used; (0, 0) when there is
/// no digit.
fn strtol_text(text: &[u8], base: i32) -> (i128, u32) {
    let mut i = text.iter().take_while(|&&c| is_space(c)).count();
    let negative = text.get(i) == Some(&b'-');
    if matches!(text.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let mut base = base;
    let hex = text.get(i) == Some(&b'0') && matches!(text.get(i + 1), Some(b'x' | b'X')) && text.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit());
    if (base == 0 || base == 16) && hex {
        i += 2;
        base = 16;
    } else if base == 0 {
        base = if text.get(i) == Some(&b'0') { 8 } else { 10 };
    }
    let (mut v, start) = (0i128, i);
    while let Some(d) = text.get(i).and_then(|&c| (c as char).to_digit(36)) {
        if d as i32 >= base {
            break;
        }
        v = (v * base as i128 + d as i128).min(1 << 100);
        i += 1;
    }
    if i == start {
        return (0, 0);
    }
    (if negative { -v } else { v }, i as u32)
}

/// `strtol(s, end, base)`: blanks, an optional sign, digits of the base (0 for the C
/// rule: `0x` hexadecimal, a leading 0 octal, else decimal). A value out of range is the
/// nearest of `LONG_MIN` and `LONG_MAX`. Where `end` is not null the address after the
/// last byte used is stored there (`s` itself when there was no number).
pub fn strtol(m: &mut Mem, s: u32, end: u32, base: i32) -> i32 {
    let (v, used) = strtol_text(&m.cstr(s), base);
    if end != 0 {
        m.set_u32(end, s.wrapping_add(used));
    }
    v.clamp(i32::MIN as i128, i32::MAX as i128) as i32
}

/// `strtoul(s, end, base)`: the same reading; a value beyond 32 bits is `ULONG_MAX`, a
/// negative one its two's complement.
pub fn strtoul(m: &mut Mem, s: u32, end: u32, base: i32) -> u32 {
    let (v, used) = strtol_text(&m.cstr(s), base);
    if end != 0 {
        m.set_u32(end, s.wrapping_add(used));
    }
    if v.abs() > u32::MAX as i128 {
        u32::MAX
    } else {
        v as u32
    }
}

/// `strtol(s, NULL, 10)`.
pub fn strtol10(m: &Mem, s: u32) -> i32 {
    strtol_text(&m.cstr(s), 10).0.clamp(i32::MIN as i128, i32::MAX as i128) as i32
}

/// `atoi(s)`.
pub fn atoi(m: &Mem, s: u32) -> i32 {
    strtol10(m, s)
}

/// What `printf("%d", v)` prints.
pub fn decimal(v: i32) -> Vec<u8> {
    v.to_string().into_bytes()
}

// ---- sorting and searching

/// `qsort(base, n, size, compare)` as the C library of the time has it and the harness's
/// stand-in reproduces it: a merge sort through a temporary array, whose comparison is
/// called with addresses of elements *in the array itself* and in a fixed order, the
/// merged run written back when each merge is done. A comparison that is not consistent,
/// or that reads other memory, therefore gets the same questions in the same order as the
/// original's. An array that is not memory is a fault and nothing is sorted.
pub fn qsort(m: &mut Mem, base: u32, n: u32, size: u32, compare: &mut dyn FnMut(&mut Mem, u32, u32) -> i32) {
    fn msort(m: &mut Mem, b: u32, count: u32, size: u32, compare: &mut dyn FnMut(&mut Mem, u32, u32) -> i32) {
        if count <= 1 {
            return;
        }
        let mut n1 = count / 2;
        let mut n2 = count - n1;
        let (mut b1, mut b2) = (b, b + n1 * size);
        msort(m, b1, n1, size, compare);
        msort(m, b2, n2, size, compare);
        let mut out: Vec<u8> = Vec::with_capacity((count * size) as usize);
        while n1 > 0 && n2 > 0 {
            if compare(m, b1, b2) <= 0 {
                out.extend_from_slice(m.bytes(b1, size));
                b1 += size;
                n1 -= 1;
            } else {
                out.extend_from_slice(m.bytes(b2, size));
                b2 += size;
                n2 -= 1;
            }
        }
        if n1 > 0 {
            out.extend_from_slice(m.bytes(b1, n1 * size));
        }
        m.set_bytes(b, &out);
    }
    if n > 1 && size != 0 {
        // the whole array has to be memory before anything is allocated for it
        let total = n as u64 * size as u64;
        if total > u32::MAX as u64 || !m.is_mapped(base, total as u32) {
            m.record_fault(base, total.min(u32::MAX as u64) as u32, crate::Access::Read);
            return;
        }
        msort(m, base, n, size, compare);
    }
}

/// `bsearch(key, base, n, size, compare)`: the address of an element the comparison calls
/// equal to the key, or null. The comparison is asked about (the key, the middle element
/// of what is left).
pub fn bsearch(m: &mut Mem, key: u32, base: u32, n: u32, size: u32, compare: &mut dyn FnMut(&mut Mem, u32, u32) -> i32) -> u32 {
    let (mut lo, mut hi) = (0u32, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let p = base.wrapping_add(mid.wrapping_mul(size));
        match compare(m, key, p) {
            r if r < 0 => hi = mid,
            r if r > 0 => lo = mid + 1,
            _ => return p,
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Access, ARENA, BSS};

    fn text(m: &mut Mem, s: &[u8]) -> u32 {
        m.place_cstr(s)
    }

    #[test]
    fn comparisons_answer_minus_one_zero_or_one() {
        let mut m = Mem::new();
        let (abc, abd, ab, upper, empty) = (text(&mut m, b"abc"), text(&mut m, b"abd"), text(&mut m, b"ab"), text(&mut m, b"ABC"), text(&mut m, b""));
        let high = text(&mut m, b"ab\xe9");
        assert_eq!((strcmp(&m, abc, abc), strcmp(&m, abc, abd), strcmp(&m, abd, abc)), (0, -1, 1));
        assert_eq!((strcmp(&m, ab, abc), strcmp(&m, abc, ab), strcmp(&m, empty, empty), strcmp(&m, empty, ab)), (-1, 1, 0, -1));
        assert_eq!(strcmp(&m, high, abc), 1); // bytes compare unsigned
        assert_eq!((strncmp(&m, abc, abd, 2), strncmp(&m, abc, abd, 3), strncmp(&m, abc, ab, 9), strncmp(&m, abc, abd, 0)), (0, -1, 1, 0));
        assert_eq!((strcasecmp(&m, abc, upper), strcasecmp(&m, upper, abd), strcmp(&m, upper, abc)), (0, -1, -1));
        assert_eq!((strncasecmp(&m, upper, abd, 2), strncasecmp(&m, upper, abd, 3), strncasecmp(&m, ab, upper, 3)), (0, -1, -1));
        assert_eq!((memcmp(&m, abc, abd, 2), memcmp(&m, abc, abd, 3), memcmp(&m, abd, abc, 3), memcmp(&m, 0, 0, 0)), (0, -1, 1, 0));
        // a comparison reads no further than the first difference: the second string here
        // runs into the end of the region without a terminator
        let end = BSS.0 + BSS.1;
        m.set_bytes(end - 3, b"abd");
        assert_eq!((strcmp(&m, abc, end - 3), strncmp(&m, end - 3, abd, 3)), (-1, 0));
        assert!(m.fault().is_none());
        // equal as far as memory goes: the read past the end is the fault, and ends it
        // (what is not memory reads as the terminator)
        m.set_bytes(end - 3, b"abc");
        assert_eq!(strcmp(&m, end - 3, abc), 0);
        assert_eq!(m.fault().map(|f| (f.addr, f.access)), Some((end, Access::Read)));
        m.clear_fault();
        // a null string reads as empty, with a fault
        assert_eq!(strcmp(&m, 0, 0), 0);
        assert_eq!(m.fault_count(), 2);
    }

    #[test]
    fn copies_and_searches() {
        let mut m = Mem::new();
        let src = text(&mut m, b"Camera1.1-0.cam");
        let buf = m.place(64, 8);
        m.set_fill_bytes(buf, 0xee, 64);
        assert_eq!(strcpy(&mut m, buf, src), buf);
        assert_eq!((m.cstr(buf), strlen(&m, buf), m.u8(buf + 16)), (b"Camera1.1-0.cam".to_vec(), 15, 0xee));
        // strncpy: no terminator when the string is as long as the count, zeros when shorter
        m.set_fill_bytes(buf, 0xee, 64);
        strncpy(&mut m, buf, src, 6);
        assert_eq!(m.bytes(buf, 8), b"Camera\xee\xee");
        strncpy(&mut m, buf + 8, src, 20);
        assert_eq!(m.bytes(buf + 8, 21), &b"Camera1.1-0.cam\0\0\0\0\0\xee"[..]);
        assert_eq!(strncpy(&mut m, buf, src, 0), buf);
        // strcat, strncat
        m.put_cstr(buf, b"ab");
        strcat(&mut m, buf, src);
        strncat(&mut m, buf, src, 3);
        strncat(&mut m, buf, src, 0);
        assert_eq!(m.cstr(buf), b"abCamera1.1-0.camCam");
        // searches
        assert_eq!((strchr(&m, src, b'.'), strrchr(&m, src, b'.'), strchr(&m, src, b'z'), strrchr(&m, src, b'z')), (src + 7, src + 11, 0, 0));
        assert_eq!((strchr(&m, src, 0), strrchr(&m, src, 0)), (src + 15, src + 15));
        let (dot, none, empty) = (text(&mut m, b"1."), text(&mut m, b"1.2"), text(&mut m, b""));
        assert_eq!((strstr(&m, src, dot), strstr(&m, src, none), strstr(&m, src, empty), strstr(&m, empty, dot)), (src + 6, 0, src, 0));
        let (set, digits) = (text(&mut m, b"aCemr"), text(&mut m, b"0123456789"));
        assert_eq!((strspn(&m, src, set), strcspn(&m, src, digits), strspn(&m, src, empty), strcspn(&m, src, empty)), (6, 6, 0, 15));
        let copy = strdup(&mut m, src);
        assert_eq!((m.cstr(copy), m.block_size(copy)), (b"Camera1.1-0.cam".to_vec(), Some(16)));
        // memory
        assert_eq!(memset(&mut m, buf, 0x41, 8), buf);
        assert_eq!(memcpy(&mut m, buf + 8, buf, 8), buf + 8);
        m.set_bytes(buf, b"01234567");
        assert_eq!(memmove(&mut m, buf + 2, buf, 6), buf + 2); // overlapping
        assert_eq!(m.bytes(buf, 16), b"01012345AAAAAAAA");
        bcopy(&mut m, buf + 8, buf, 4);
        bzero(&mut m, buf + 4, 4);
        assert_eq!(m.bytes(buf, 9), b"AAAA\0\0\0\0A");
        memset(&mut m, 0, 1, 0);
        memcpy(&mut m, 0, 0, 0);
        assert!(m.fault().is_none());
        // into what is not memory: a fault, nothing stored
        strcpy(&mut m, 8, src);
        assert_eq!(m.fault().map(|f| (f.addr, f.access)), Some((8, Access::Write)));
        m.clear_fault();
        memcpy(&mut m, buf, 4, 4);
        assert_eq!(m.fault().map(|f| (f.addr, f.access)), Some((4, Access::Read)));
        m.clear_fault();
        // a count that is nonsense allocates nothing and faults where the zeros would go
        strncpy(&mut m, buf, src, 0xffff_fff0);
        assert_eq!(m.fault().map(|f| (f.addr, f.access)), Some((buf + 15, Access::Write)));
        assert_eq!(m.bytes(buf, 15), b"Camera1.1-0.cam");
    }

    #[test]
    fn tokens() {
        let mut m = Mem::new();
        let line = text(&mut m, b"  SCENE Camera1,,1.1 ");
        let delim = text(&mut m, b" ,");
        let save = ARENA.0 + 0x7000;
        let mut got = Vec::new();
        let mut t = strtok_r(&mut m, line, delim, save);
        while t != 0 {
            got.push(m.cstr(t));
            t = strtok_r(&mut m, 0, delim, save);
        }
        assert_eq!(got, [&b"SCENE"[..], b"Camera1", b"1.1"]);
        // the terminators are in the string, and the saved position is null at the end
        assert_eq!((m.cstr(line), m.u32(save)), (b"  SCENE".to_vec(), 0));
        assert_eq!(strtok_r(&mut m, 0, delim, save), 0);
        // the variant with the library's own variable
        let line = text(&mut m, b"a,b");
        let mut saved = 0;
        assert_eq!((strtok(&mut m, line, delim, &mut saved), saved), (line, line + 2));
        assert_eq!((strtok(&mut m, 0, delim, &mut saved), saved), (line + 2, 0));
        assert_eq!(strtok(&mut m, 0, delim, &mut saved), 0);
        let blank = text(&mut m, b" , ");
        assert_eq!(strtok(&mut m, blank, delim, &mut saved), 0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn numbers() {
        let mut m = Mem::new();
        let end = ARENA.0 + 0x7000;
        let read = |m: &mut Mem, s: &[u8], base: i32| {
            let a = text(m, s);
            let v = strtol(m, a, end, base);
            (v, m.u32(end) - a)
        };
        assert_eq!(read(&mut m, b"  -42abc", 10), (-42, 5));
        assert_eq!(read(&mut m, b"+7", 10), (7, 2));
        assert_eq!(read(&mut m, b"cube", 10), (0, 0));
        assert_eq!(read(&mut m, b"-", 10), (0, 0));
        assert_eq!(read(&mut m, b"0x1F,", 16), (31, 4));
        assert_eq!(read(&mut m, b"0x1F,", 0), (31, 4));
        assert_eq!(read(&mut m, b"1F", 16), (31, 2));
        assert_eq!(read(&mut m, b"0xg", 16), (0, 1)); // only the 0 is a number
        assert_eq!(read(&mut m, b"017", 0), (15, 3));
        assert_eq!(read(&mut m, b"17", 0), (17, 2));
        assert_eq!(read(&mut m, b"zz", 36), (35 * 36 + 35, 2));
        assert_eq!(read(&mut m, b"99999999999", 10), (i32::MAX, 11));
        assert_eq!(read(&mut m, b"-99999999999999999999999999999999999999999999", 10), (i32::MIN, 45));
        let a = text(&mut m, b"12 cubes");
        assert_eq!((strtol(&mut m, a, 0, 10), strtol10(&m, a), atoi(&m, a)), (12, 12, 12));
        let (big, neg, over) = (text(&mut m, b"4294967295"), text(&mut m, b"-1"), text(&mut m, b"4294967296"));
        assert_eq!((strtoul(&mut m, big, 0, 10), strtoul(&mut m, neg, 0, 10), strtoul(&mut m, over, end, 10)), (u32::MAX, u32::MAX, u32::MAX));
        assert_eq!((toupper(b'a' as i32), toupper(b'A' as i32), toupper(b'{' as i32), tolower(b'Z' as i32), tolower(-1)), (65, 65, 123, 122, -1));
        assert_eq!((decimal(-12), decimal(0)), (b"-12".to_vec(), b"0".to_vec()));
        assert!(m.fault().is_none());
    }

    #[test]
    fn sorting_asks_in_the_librarys_order() {
        let mut m = Mem::new();
        let a = m.place(4 * 5, 8);
        for (i, v) in [30i32, 10, 20, 10, 5].into_iter().enumerate() {
            m.set_i32(a + 4 * i as u32, v);
        }
        let mut asked = Vec::new();
        qsort(&mut m, a, 5, 4, &mut |m, x, y| {
            asked.push(((x - a) / 4, (y - a) / 4));
            m.i32(x).cmp(&m.i32(y)) as i32
        });
        assert_eq!((0..5).map(|i| m.i32(a + 4 * i)).collect::<Vec<_>>(), [5, 10, 10, 20, 30]);
        // the halves (2 and 3 elements) are sorted in place before they are merged, and
        // every question is about two places of the array itself
        assert_eq!(asked, [(0, 1), (3, 4), (2, 3), (2, 4), (0, 2), (0, 3), (1, 3), (1, 4)]);
        // found, not found, an empty array
        let key = m.place_record(20i32).addr();
        let mut cmp = |m: &mut Mem, k: u32, e: u32| m.i32(k).cmp(&m.i32(e)) as i32;
        assert_eq!(bsearch(&mut m, key, a, 5, 4, &mut cmp), a + 12);
        m.set_i32(key, 7);
        assert_eq!((bsearch(&mut m, key, a, 5, 4, &mut cmp), bsearch(&mut m, key, a, 0, 4, &mut cmp)), (0, 0));
        // nothing to do, and an array that is not memory
        qsort(&mut m, a, 1, 4, &mut |_, _, _| panic!("one element is not compared"));
        qsort(&mut m, a, 5, 0, &mut |_, _, _| panic!("no size, no sort"));
        assert!(m.fault().is_none());
        qsort(&mut m, 0x1000, 0x4000_0000, 16, &mut |_, _, _| 0);
        assert_eq!(m.fault().map(|f| f.access), Some(Access::Read));
    }
}
