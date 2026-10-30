// SPDX-License-Identifier: MIT
//! The database layer on disk (finding 0007): how the program names, finds, versions and
//! lists files.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `dbexternal.c` (nearly all of it), `filetoken.c`
//! (the token reader of the resource files), `init_dict.c` (where the database list is
//! looked for) and, from `elemdict.c`, only the pieces the database layer calls: the file
//! name to list node routine, the list of chapters and the comparison that sorts versions.
//!
//! The original works on C strings, so names here are byte strings without a zero byte.
//! Everything that touches a disk goes through [`FileSystem`] / [`FileSystemMut`], so each
//! function runs unchanged on a directory tree held in memory ([`MemFs`]) or on a real one
//! ([`RealFs`]).

pub mod calls;
mod chapters;
mod dbmessages;
mod create;
pub mod dbicons; pub mod dbmanager; pub mod dbfile; pub mod dictlist;
pub mod imglobals;
mod fs;
mod misc;
mod name;
mod ops;
mod path;
mod rsrc;
mod scan;
mod token;

pub use chapters::*;
pub use create::*;
pub use fs::*;
pub use misc::*;
pub use name::*;
pub use ops::*;
pub use path::*;
pub use rsrc::*;
pub use scan::*;
pub use token::*;

/// A C string held by the program: bytes, no zero byte inside.
pub type Bytes = Vec<u8>;

/// The part of a buffer a C routine sees as the string: up to the first zero byte.
pub fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

/// What glibc's `printf` family writes for a null pointer under `%s`. Several routines here
/// format a field that may be null; the original prints this text and so do we.
pub const NULL_TEXT: &[u8] = b"(null)";

/// `%s` of a possibly null string.
pub(crate) fn or_null(s: &Option<Bytes>) -> &[u8] {
    s.as_deref().unwrap_or(NULL_TEXT)
}

/// `sscanf(s, "%d", &v)`: leading white space is skipped, then an optional sign and at
/// least one digit. Returns the value and how many bytes were consumed, or `None` when
/// nothing converts (the original's variable then keeps whatever it held). A number too
/// large for 32 bits is clamped, as glibc's conversion through `strtol` does.
pub fn scan_int(s: &[u8]) -> Option<(i32, usize)> {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let first = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    if i == first {
        return None;
    }
    let v = if neg { -v } else { v };
    Some((v.clamp(i32::MIN as i64, i32::MAX as i64) as i32, i))
}

/// `strtol(s, NULL, 10)` as the resource readers use it: 0 when nothing converts.
pub fn strtol10(s: &[u8]) -> i32 {
    scan_int(s).map_or(0, |(v, _)| v)
}

/// `sprintf("%d", v)`.
pub(crate) fn dec(v: i32) -> Bytes {
    v.to_string().into_bytes()
}

pub(crate) fn concat(parts: &[&[u8]]) -> Bytes {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_int_follows_sscanf() {
        assert_eq!(scan_int(b"12"), Some((12, 2)));
        assert_eq!(scan_int(b"  -7x"), Some((-7, 4)));
        assert_eq!(scan_int(b"+3"), Some((3, 2)));
        assert_eq!(scan_int(b"-"), None);
        assert_eq!(scan_int(b"x1"), None);
        assert_eq!(scan_int(b""), None);
        assert_eq!(scan_int(b"99999999999"), Some((i32::MAX, 11)));
        assert_eq!(scan_int(b"-99999999999"), Some((i32::MIN, 12)));
        assert_eq!(strtol10(b"4"), 4);
        assert_eq!(strtol10(b"four"), 0);
    }

    #[test]
    fn c_string_ends_at_zero() {
        assert_eq!(cstr(b"ab\0cd"), b"ab");
        assert_eq!(cstr(b"ab"), b"ab");
    }
}
