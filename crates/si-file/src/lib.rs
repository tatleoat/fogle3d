// SPDX-License-Identifier: MIT
//! The file layer under every Softimage|3D reader and writer (finding 0002).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `headers.c`, `byteswap.c`.

/// A file being read, with the part of C stdio's behaviour the original readers depend on:
/// reads past the end are short and set an end-of-file flag, and seeking clears it.
/// The original code tests that flag (`feof`) to decide when to stop, so a faithful reader
/// needs the same flag, not just "no more bytes".
pub struct Stream<'a> {
    data: &'a [u8],
    pos: usize,
    eof: bool,
}

impl<'a> Stream<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Stream { data, pos: 0, eof: false }
    }

    /// `getc`: the next byte, or `None` (C's EOF) with the end-of-file flag set.
    pub fn getc(&mut self) -> Option<u8> {
        match self.data.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                Some(b)
            }
            None => {
                self.eof = true;
                None
            }
        }
    }

    /// `fread(buf, 1, buf.len(), f)`: copies what is there and returns how many bytes that was.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        if n < buf.len() {
            self.eof = true;
        }
        n
    }

    /// `fread(buf, size, 1, f)` as the original uses it for one record: all or nothing in the
    /// return value. A partial record is consumed, as stdio does.
    pub fn read_record(&mut self, buf: &mut [u8]) -> bool {
        self.read(buf) == buf.len()
    }

    pub fn feof(&self) -> bool {
        self.eof
    }

    pub fn tell(&self) -> usize {
        self.pos
    }

    /// `fseek(f, pos, SEEK_SET)`. Seeking past the end is allowed, as in C.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.data.len());
        self.eof = false;
    }

    /// `fseek(f, 0, SEEK_END)`
    pub fn seek_end(&mut self) {
        self.pos = self.data.len();
        self.eof = false;
    }

    pub fn rewind(&mut self) {
        self.seek(0);
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    // Multi-byte fields are big-endian in every file on every platform: the format was defined
    // on SGI machines, and the little-endian builds swap after each read
    // [soft 0x083f9ba0 SI_byteswap_fread] [soft 0x083f9350 SI_swap4byte] [soft 0x083f9330 SI_swap2byte].
    pub fn be_u16(&mut self) -> Option<u16> {
        let mut b = [0u8; 2];
        self.read_record(&mut b).then(|| u16::from_be_bytes(b))
    }

    pub fn be_i16(&mut self) -> Option<i16> {
        self.be_u16().map(|v| v as i16)
    }

    pub fn be_u32(&mut self) -> Option<u32> {
        let mut b = [0u8; 4];
        self.read_record(&mut b).then(|| u32::from_be_bytes(b))
    }

    pub fn be_i32(&mut self) -> Option<i32> {
        self.be_u32().map(|v| v as i32)
    }

    pub fn be_f32(&mut self) -> Option<f32> {
        self.be_u32().map(f32::from_bits)
    }
}

/// Size of the chunk every binary file starts with.
pub const HEADER_SIZE: usize = 0x58;
/// The comment field of the header.
pub const COMMENT_SIZE: usize = 0x50;

pub const MAGIC_GENERAL: u32 = 0x5380_f634;
pub const MAGIC_ANIMATION: u32 = 0x5380_f635;
pub const MAGIC_CAMERA: u32 = 0x5380_f636;
/// Not in the program's table, but accepted by the header reader; cluster files carry it.
pub const MAGIC_CLUSTER: u32 = 0x5380_f637;
pub const MAGIC_LATTICE: u32 = 0x5380_f638;
pub const MAGIC_LIGHT: u32 = 0x5380_f639;
pub const MAGIC_MATERIAL: u32 = 0x5380_f640;
pub const MAGIC_MODEL: u32 = 0x5380_f641;
/// Not in the program's table, but accepted by the header reader; shape files carry it.
pub const MAGIC_SHAPE: u32 = 0x5380_f642;
pub const MAGIC_TEXTURE_2D: u32 = 0x5380_f643;
pub const MAGIC_TEXTURE_3D: u32 = 0x5380_f644;
pub const MAGIC_STRUCT_KEYS: u32 = 0x5380_f646;

/// The version this build writes into headers [soft 0x086793f4 SI_versionOutput].
pub const VERSION_OUTPUT: f32 = 4.0;
/// A file whose version is above this triggers the "newer version" hook (a double in the
/// original, 0x0865c590).
pub const VERSION_CHECK_ABOVE: f64 = 4.0001;
/// What the writers put in the comment field when given none.
pub const DEFAULT_COMMENT: &[u8] = b"No comment...";

const fn tag(t: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*t)
}

/// The program's own description of a file type, by magic: all 35 entries of its table
/// [soft 0x083fb328 SI_dbfileMagicType]. The descriptions are the original's own texts and are
/// not in this source: each entry names where the Linux `soft` holds its text (release model,
/// `docs/design/runtime-data.md`), and the text is read from the user's install when asked.
/// `Ok(None)` is a magic the table does not hold; a missing install is an `Err`, not a `None`.
pub fn magic_type(magic: u32) -> Result<Option<&'static str>, si_rt::Error> {
    const TABLE: [(u32, u32); 35] = [
        (tag(b"CLUS"), 0x086ceda6),
        (tag(b"SCHN"), 0x086ced55),
        (tag(b"SDAD"), 0x086cf1c0),
        (tag(b"SDAM"), 0x086cf080),
        (tag(b"SDAT"), 0x086cf000),
        (tag(b"SDIG"), 0x086cf180),
        (tag(b"SDRV"), 0x086ced88),
        (tag(b"SDSC"), 0x086ced20),
        (tag(b"SDT2"), 0x086cf100),
        (tag(b"SDT3"), 0x086cf140),
        (tag(b"SDUT"), 0x086cf0c0),
        (tag(b"SDVL"), 0x086cf040),
        (tag(b"SFLK"), 0x086cedc3),
        (tag(b"SHAD"), 0x086cefc0),
        (tag(b"SHAM"), 0x086cee80),
        (tag(b"SHAT"), 0x086cee00),
        (tag(b"SHIG"), 0x086cef80),
        (tag(b"SHT2"), 0x086cef00),
        (tag(b"SHT3"), 0x086cef40),
        (tag(b"SHUT"), 0x086ceec0),
        (tag(b"SHVL"), 0x086cee40),
        (tag(b"SREL"), 0x086ceddf),
        (tag(b"SSCN"), 0x086ced00),
        (tag(b"SSET"), 0x086ced3f),
        (tag(b"SWAV"), 0x086ced73),
        (MAGIC_GENERAL, 0x086cebc4),
        (MAGIC_ANIMATION, 0x086cebdd),
        (MAGIC_CAMERA, 0x086cebf8),
        (MAGIC_LATTICE, 0x086cec4b),
        (MAGIC_LIGHT, 0x086cec64),
        (MAGIC_MATERIAL, 0x086cec7b),
        (MAGIC_MODEL, 0x086cec95),
        (MAGIC_TEXTURE_2D, 0x086cecc8),
        (MAGIC_TEXTURE_3D, 0x086cece4),
        (MAGIC_STRUCT_KEYS, 0x086cec2e),
    ];
    match TABLE.iter().find(|(m, _)| *m == magic) {
        Some((_, addr)) => si_rt::try_text(*addr).map(Some),
        None => Ok(None),
    }
}

/// The 88-byte chunk at the start of a binary file.
#[derive(Clone, Debug, PartialEq)]
pub struct BinHeader {
    pub magic: u32,
    pub version: f32,
    /// Raw field: zero-padded, and with no terminator when the comment fills all 80 bytes.
    pub comment: [u8; COMMENT_SIZE],
}

impl BinHeader {
    /// The comment up to its first zero byte.
    pub fn comment_bytes(&self) -> &[u8] {
        let n = self.comment.iter().position(|&b| b == 0).unwrap_or(COMMENT_SIZE);
        &self.comment[..n]
    }
}

/// Reads one 88-byte chunk, whatever its magic [soft 0x083faa40 SI_FileChunk_fread]
/// [soft 0x083f9878 SI_fix_SI_FileChunk]. `None` if the file is shorter than a chunk.
pub fn read_chunk(s: &mut Stream) -> Option<BinHeader> {
    let mut b = [0u8; HEADER_SIZE];
    if !s.read_record(&mut b) {
        return None;
    }
    let mut comment = [0u8; COMMENT_SIZE];
    comment.copy_from_slice(&b[8..]);
    Some(BinHeader {
        magic: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        version: f32::from_bits(u32::from_be_bytes([b[4], b[5], b[6], b[7]])),
        comment,
    })
}

/// The header of a binary database file [soft 0x083fb6fc SI_dbfileReadBinHeader].
///
/// Any magic from 0x5380f634 to 0x5380f646 is accepted (one unsigned range test in the
/// original, so the two values its type table does not name pass too). For anything else the
/// original rewinds the file and reports −1 with a version of −1.0: here that is `None`, with
/// the stream rewound. The original also stores the version in the global `SI_versionInput`
/// (0x086793f0) and, above [`VERSION_CHECK_ABOVE`], calls an installable hook; both are left
/// to the caller, who has the returned version.
pub fn read_bin_header(s: &mut Stream) -> Option<BinHeader> {
    match read_chunk(s) {
        Some(h) if h.magic.wrapping_sub(MAGIC_GENERAL) < 0x13 => Some(h),
        _ => {
            s.rewind();
            None
        }
    }
}

/// Writes a header chunk [soft 0x083fb88c si_newheaderBinaryWrite] [soft 0x083faaa0
/// SI_FileChunk_fwrite]. The comment is copied as `strncpy(field, comment, 80)` does: cut at
/// 80 bytes and at its first zero byte, the rest of the field zero.
///
/// The original builds the chunk on the stack without clearing it and `strncpy` pads with
/// zeros, so the field is fully defined; nothing here differs from the original's bytes.
pub fn write_bin_header(out: &mut Vec<u8>, magic: u32, version: f32, comment: Option<&[u8]>) {
    let comment = comment.unwrap_or(DEFAULT_COMMENT);
    let n = comment.iter().position(|&b| b == 0).unwrap_or(comment.len()).min(COMMENT_SIZE);
    out.extend_from_slice(&magic.to_be_bytes());
    out.extend_from_slice(&version.to_bits().to_be_bytes());
    out.extend_from_slice(&comment[..n]);
    out.resize(out.len() + COMMENT_SIZE - n, 0);
}

/// A general-magic header at this build's version [soft 0x083fb83c SI_headerBinaryWrite].
pub fn write_general_header(out: &mut Vec<u8>, comment: Option<&[u8]>) {
    write_bin_header(out, MAGIC_GENERAL, VERSION_OUTPUT, comment);
}

/// [soft 0x083fb8dc SI_dbfileWriteBinHeader]: a header with the default comment.
pub fn write_db_header(out: &mut Vec<u8>, version: f32, magic: u32) {
    write_bin_header(out, magic, version, None);
}

/// The seven tags the ASCII header reader accepts [soft 0x083fb788 SI_dbfileReadAscHeader].
/// After the tag the version follows as text; reading it needs the ASCII token reader, which
/// belongs with the ASCII file readers.
pub fn is_ascii_header_tag(t: u32) -> bool {
    [b"SFLK", b"SDRV", b"SCHN", b"SDSC", b"SSET", b"SREL", b"SWAV"].iter().any(|x| tag(x) == t)
}

/// The start of an ASCII file: the tag, then the version printed with C's `"%g\n"`
/// [soft 0x083fb900 SI_dbfileWriteAscHeader].
pub fn write_asc_header(out: &mut Vec<u8>, version: f32, tag: u32) {
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(format_g(version as f64).as_bytes());
    out.push(b'\n');
}

/// C's `printf("%g", v)` for finite values: six significant digits, trailing zeros removed,
/// exponent form when the decimal exponent is below −4 or at least 6.
pub fn format_g(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !v.is_finite() {
        return if v.is_nan() { "nan".into() } else if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    // round to six significant digits first; the exponent is that of the rounded value
    let e = format!("{:.5e}", v);
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if !(-4..6).contains(&exp) {
        format!("{}e{}{:02}", strip(mantissa), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        strip(&format!("{:.*}", (5 - exp) as usize, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_flags_follow_stdio() {
        let mut s = Stream::new(&[1, 2, 3]);
        assert_eq!(s.be_u16(), Some(0x0102));
        assert!(!s.feof());
        assert_eq!(s.be_u16(), None); // one byte left: consumed, flag set
        assert!(s.feof());
        assert_eq!(s.tell(), 3);
        s.seek_end();
        assert!(!s.feof()); // fseek clears the flag
        assert_eq!(s.getc(), None);
        assert!(s.feof());
    }

    #[test]
    fn header_round_trip_and_range() {
        let mut out = Vec::new();
        write_general_header(&mut out, None);
        assert_eq!(out.len(), HEADER_SIZE);
        assert_eq!(&out[..8], &[0x53, 0x80, 0xf6, 0x34, 0x40, 0x80, 0, 0]);
        let h = read_bin_header(&mut Stream::new(&out)).unwrap();
        assert_eq!((h.magic, h.version, h.comment_bytes()), (MAGIC_GENERAL, 4.0, DEFAULT_COMMENT));
        // both ends of the accepted range, and one past each
        for (magic, ok) in [(0x5380_f634u32, true), (0x5380_f646, true), (0x5380_f633, false), (0x5380_f647, false)] {
            let mut b = out.clone();
            b[..4].copy_from_slice(&magic.to_be_bytes());
            let mut s = Stream::new(&b);
            assert_eq!(read_bin_header(&mut s).is_some(), ok);
            assert_eq!(s.tell(), if ok { HEADER_SIZE } else { 0 });
        }
    }

    #[test]
    fn comment_is_copied_like_strncpy() {
        let mut out = Vec::new();
        write_bin_header(&mut out, MAGIC_CAMERA, 3.5, Some(&[b'x'; 100]));
        assert_eq!(out.len(), HEADER_SIZE);
        assert!(out[8..].iter().all(|&b| b == b'x')); // 80 bytes, no terminator
        let mut out = Vec::new();
        write_bin_header(&mut out, MAGIC_CAMERA, 3.5, Some(b"ab\0cd"));
        assert_eq!(&out[8..12], b"ab\0\0");
    }

    #[test]
    fn table_has_35_entries_and_skips_two_magics() {
        si_rt::skip_without_install!();
        // the 33 magics the table holds read as text (our checksum of the 35 descriptions in
        // table order, each followed by a zero), the two it skips as none
        let mut all = Vec::new();
        let mut n = 0;
        for m in [MAGIC_GENERAL, MAGIC_ANIMATION, MAGIC_CAMERA, MAGIC_LATTICE, MAGIC_LIGHT, MAGIC_MATERIAL, MAGIC_MODEL,
                  MAGIC_TEXTURE_2D, MAGIC_TEXTURE_3D, MAGIC_STRUCT_KEYS] {
            all.extend_from_slice(magic_type(m).unwrap().expect("binary magic in the table").as_bytes());
            all.push(0);
            n += 1;
        }
        for t in [b"CLUS", b"SCHN", b"SDAD", b"SDAM", b"SDAT", b"SDIG", b"SDRV", b"SDSC", b"SDT2", b"SDT3", b"SDUT", b"SDVL",
                  b"SFLK", b"SHAD", b"SHAM", b"SHAT", b"SHIG", b"SHT2", b"SHT3", b"SHUT", b"SHVL", b"SREL", b"SSCN", b"SSET", b"SWAV"] {
            all.extend_from_slice(magic_type(u32::from_be_bytes(*t)).unwrap().expect("ascii tag in the table").as_bytes());
            all.push(0);
            n += 1;
        }
        assert_eq!(n, 35);
        assert_eq!((all.len(), si_rt::fnv1a64(&all)), (TEXTS_LEN, TEXTS_SUM));
        assert_eq!(magic_type(MAGIC_CLUSTER).unwrap(), None);
        assert_eq!(magic_type(MAGIC_SHAPE).unwrap(), None);
    }

    // measured on the literals this file held before (binary entries first, then the ascii tags in table order)
    const TEXTS_LEN: usize = 1139;
    const TEXTS_SUM: u64 = 0x17d9_ff54_aeda_b033;

    #[test]
    fn percent_g() {
        for (v, s) in [(4.0, "4"), (3.71, "3.71"), (3.4909, "3.4909"), (2.64933, "2.64933"), (0.0001, "0.0001"),
                       (0.00001, "1e-05"), (1234567.0, "1.23457e+06"), (999999.5, "1e+06"), (100000.0, "100000"),
                       (-2.5, "-2.5"), (0.0, "0"), (3.704, "3.704"), (123456.0, "123456")] {
            assert_eq!(format_g(v), s, "{v}");
        }
        // the header writer formats the float widened to double, as C's varargs do
        let mut out = Vec::new();
        write_asc_header(&mut out, 3.71, u32::from_be_bytes(*b"SDSC"));
        assert_eq!(out, b"SDSC3.71\n");
    }
}
