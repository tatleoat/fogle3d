// SPDX-License-Identifier: MIT
//! Helpers of the picture library: how a picture file name is opened, and the PackBits coding.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source file: `util.c`.

/// What `SI_softOpen` decides to open.
#[derive(Clone, Debug, PartialEq)]
pub enum SoftOpen {
    /// The name starts with `-` and the mode with `r`: standard input.
    Stdin,
    /// The name starts with `-` and the mode with `w`: standard output.
    Stdout,
    /// The name starts with `-` and the mode is neither; the text is shown as an error.
    BadMode(String),
    /// A file to hand to `fopen` with the mode.
    Path(String),
}

/// The name the picture library opens for a file name, an extension and an `fopen` mode
/// [soft 0x085e3164 SI_softOpen].
///
/// - A name starting with `-` means standard input (mode `r…`) or output (`w…`); any other
///   mode is refused with "Cannot use stdin or stdout with mode %s!". The base name reported
///   to the caller is then `-`.
/// - With no extension, or when the name's last dot starts exactly the extension, the name
///   is used as it is.
/// - Otherwise the extension is appended. For a mode that does not start with `w`, the
///   original then tests the longer name with `access` (`exists` here) and falls back to the
///   bare name when it is not there.
///
/// After `fopen` fails the original shows "<name>: <system error text>", when its caller
/// asked for messages. On request it also hands back the file's base name and its directory
/// (see [`split_path`]).
pub fn soft_open(name: &str, extension: Option<&str>, mode: &str, exists: &dyn Fn(&str) -> bool) -> SoftOpen {
    if name.starts_with('-') {
        return match mode.as_bytes().first() {
            Some(b'r') => SoftOpen::Stdin,
            Some(b'w') => SoftOpen::Stdout,
            _ => SoftOpen::BadMode(format!("Cannot use stdin or stdout with mode {mode}!")),
        };
    }
    let Some(ext) = extension else { return SoftOpen::Path(name.to_string()) };
    if name.rfind('.').is_some_and(|dot| &name[dot..] == ext) {
        return SoftOpen::Path(name.to_string());
    }
    let longer = format!("{name}{ext}");
    if !mode.starts_with('w') && !exists(&longer) {
        return SoftOpen::Path(name.to_string());
    }
    SoftOpen::Path(longer)
}

/// The directory part and the base name of a path as `SI_softOpen` reports them
/// [soft 0x085e3164 SI_softOpen]: split at the last `/`. With no `/` the directory is the
/// current one (`None` here); otherwise the original changes into the directory part, asks
/// for the working directory and changes back, so that it reports an absolute path with
/// symbolic links resolved. That part belongs to the file system and is left to the caller.
pub fn split_path(path: &str) -> (Option<&str>, &str) {
    match path.rfind('/') {
        Some(i) => (Some(&path[..i]), &path[i + 1..]),
        None => (None, path),
    }
}

/// The message [soft 0x085e2b20 si_fileSize] shows when it cannot open the file whose length
/// it was asked for (it then returns −1; otherwise it seeks to the end and returns the
/// position).
pub fn file_size_message(name: &str) -> String {
    format!("Cannot open {name}\n")
}

/// PackBits coding of `src` [soft 0x085e2bb0 si_packBit].
///
/// Output units: a count byte `c` from 0 to 0x7f followed by `c + 1` literal bytes, or a
/// count byte from 0x81 to 0xff (as a signed value −127 to −1) followed by one byte repeated
/// `1 − c` times (2 to 128). 0x80 is never written.
///
/// The coder walks the pairs of neighbouring bytes. Its quirks, all reproduced:
/// - Two equal bytes always start a run, so `a b b c` is coded as three units.
/// - A run that reaches 128 is written at once and the walk skips the pair that follows.
///   If the input **ends** one byte later with yet another equal byte (129, 258, … equal
///   bytes at the end), that last byte is not written at all.
/// - With fewer than two bytes the original compares bytes in front of its input. Here an
///   empty input gives nothing and a single byte gives one literal unit.
///
/// The original allocates `1.1 × length` bytes when the caller passes no buffer, which the
/// worst case (all literals, `length × 129 / 128` bytes) fits into for lengths of 13 and up.
pub fn pack_bit(src: &[u8]) -> Vec<u8> {
    let n = src.len();
    let mut out = Vec::with_capacity(n + n / 10 + 2);
    if n == 0 {
        return out;
    }
    let mut count: i16 = 0; // above 0: literals held; below 0: a run of 1 − count bytes
    let mut held = 0usize; // index of the first byte not yet written
    let mut i = 0usize;
    while i + 1 < n {
        if src[i] == src[i + 1] {
            if count > 0 {
                out.push((count - 1) as u8);
                out.extend_from_slice(&src[held..held + count as usize]);
                held += count as usize;
                count = 0;
            }
            count -= 1;
            if count == -0x7f {
                out.push(0x81);
                out.push(src[held]);
                held += 0x80;
                count = 0;
                i += 1;
            }
        } else if count < 0 {
            out.push(count as u8);
            out.push(src[held]);
            held += (1 - count) as usize;
            count = 0;
        } else {
            count += 1;
            if count == 0x80 {
                out.push(0x7f);
                out.extend_from_slice(&src[held..held + 0x80]);
                held += 0x80;
                count = 0;
            }
        }
        i += 1;
    }
    if count < 0 {
        out.push(count as u8);
        out.push(src[held]);
    } else if count > 0 {
        out.push(count as u8);
        out.extend_from_slice(&src[held..held + count as usize + 1]);
    } else if n >= 2 && src[n - 2] == src[n - 1] {
        // nothing held: the last pair closed a full run, or (the flaw) followed one
    } else {
        out.push(0);
        out.push(src[held.min(n - 1)]);
    }
    out
}

/// PackBits decoding [soft 0x085e2d50 si_unPackBit] of the first `length` bytes of `src`.
///
/// A count byte from 0 to 0x7f copies the next `count + 1` bytes; one from 0x81 to 0xff
/// repeats the next byte `1 − count` times. Quirks reproduced:
/// - **0x80 is copied to the output as a data byte**, and it is not counted against
///   `length`, so each one makes the decoder read one byte further than it was told.
/// - The loop runs while the remaining length is above 0 and subtracts whole units, so a
///   unit that starts inside the length is decoded in full.
/// - The size of the destination is a parameter the original never looks at. Here the output
///   simply grows, and reading stops at the end of `src` (the original reads on).
pub fn un_pack_bit(src: &[u8], length: i64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut left = length;
    let mut at = 0usize;
    while left > 0 {
        let Some(&c) = src.get(at) else { break };
        if c == 0x80 {
            out.push(0x80);
            at += 1;
        } else if c >= 0x80 {
            let n = 1 - (c as i8 as i32);
            let Some(&v) = src.get(at + 1) else { break };
            out.resize(out.len() + n as usize, v);
            at += 2;
            left -= 2;
        } else {
            let n = c as usize + 1;
            let end = (at + 1 + n).min(src.len());
            out.extend_from_slice(&src[at + 1..end]);
            if end < at + 1 + n {
                break;
            }
            at += 1 + n;
            left -= 1 + n as i64;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rules() {
        let no = |_: &str| false;
        let yes = |_: &str| true;
        assert_eq!(soft_open("-", Some(".pic"), "rb", &no), SoftOpen::Stdin);
        assert_eq!(soft_open("-x", Some(".pic"), "wb", &no), SoftOpen::Stdout);
        assert_eq!(soft_open("-", None, "ab", &no), SoftOpen::BadMode("Cannot use stdin or stdout with mode ab!".into()));
        assert_eq!(soft_open("a.pic", Some(".pic"), "rb", &no), SoftOpen::Path("a.pic".into()));
        assert_eq!(soft_open("a", Some(".pic"), "rb", &yes), SoftOpen::Path("a.pic".into()));
        assert_eq!(soft_open("a", Some(".pic"), "rb", &no), SoftOpen::Path("a".into())); // falls back
        assert_eq!(soft_open("a", Some(".pic"), "wb", &no), SoftOpen::Path("a.pic".into())); // writing: no test
        assert_eq!(soft_open("a.1", Some(".pic"), "wb", &no), SoftOpen::Path("a.1.pic".into()));
        assert_eq!(soft_open("a.1", None, "rb", &no), SoftOpen::Path("a.1".into()));
        assert_eq!(split_path("/x/y/z.pic"), (Some("/x/y"), "z.pic"));
        assert_eq!(split_path("z.pic"), (None, "z.pic"));
    }

    #[test]
    fn pack_bits_forms() {
        assert_eq!(pack_bit(&[1, 2, 3]), [2, 1, 2, 3]);
        assert_eq!(pack_bit(&[5, 5, 5]), [0xfe, 5]);
        assert_eq!(pack_bit(&[1, 2, 2, 3]), [0, 1, 0xff, 2, 0, 3]);
        assert_eq!(pack_bit(&[9]), [0, 9]);
        assert_eq!(pack_bit(&[]), [] as [u8; 0]);
        assert_eq!(pack_bit(&[7; 128]), [0x81, 7]);
        assert_eq!(pack_bit(&[7; 130]), [0x81, 7, 0xff, 7]);
        // the flaw: 129 equal bytes at the end lose the last one
        assert_eq!(pack_bit(&[7; 129]), [0x81, 7]);
        let ramp: Vec<u8> = (0..130).map(|i| i as u8).collect();
        let e = pack_bit(&ramp);
        assert_eq!((e[0], e.len()), (0x7f, 129 + 3));
        assert_eq!(&e[129..], [1, 128, 129]);
    }

    #[test]
    fn pack_bits_round_trip() {
        let mut state = 12345u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 16
        };
        for _ in 0..300 {
            let mut src = Vec::new();
            let len = 2 + next() as usize % 400;
            while src.len() < len {
                let run = match next() % 4 { 0 => 1, 1 => 2, 2 => 1 + next() as usize % 9, _ => 1 + next() as usize % 200 };
                // 0x80 as data cannot round-trip through this decoder's count handling of runs,
                // but as a literal or run value it does
                let v = (next() % 5) as u8;
                for _ in 0..run.min(len - src.len()) {
                    src.push(v);
                }
            }
            let packed = pack_bit(&src);
            let back = un_pack_bit(&packed, packed.len() as i64);
            // equal, but for the one byte the coder drops after a full run at the end
            if back != src {
                assert_eq!(back.len() + 1, src.len());
                assert_eq!(&back[..], &src[..back.len()]);
                let tail = src.iter().rev().take_while(|&&b| b == src[src.len() - 1]).count();
                assert_eq!(tail % 128, 1);
            }
        }
    }

    #[test]
    fn unpack_quirks() {
        assert_eq!(un_pack_bit(&[0x80, 0, 5], 2), [0x80, 5]); // 0x80 passes through and is not counted
        assert_eq!(un_pack_bit(&[0xfd, 4, 1, 8, 9], 1), [4, 4, 4, 4]); // a unit begun is finished
        assert_eq!(un_pack_bit(&[3, 1, 2], 9), [1, 2]); // cut short: what was there
    }
}
