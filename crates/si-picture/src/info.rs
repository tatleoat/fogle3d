// SPDX-License-Identifier: MIT
//! The picture information calls, the header writer and the other entry points of
//! `picture.c` around a picture file.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source files: `picture.c`, `util.c`.

use crate::{
    read_pict_chunk, write_mixed, write_pict_chunk, OpenPicture, Packet, Picture, PictureError, PictureRgba, ALPHA, BLUE, GREEN,
    PICT, PICTURE_VERSION, RED, TYPE_MIXED, TYPE_RAW,
};
use si_file::{read_chunk, write_bin_header, Stream, MAGIC_GENERAL};

// Three functions of `picture.c` are entry points with nothing of their own:
// [soft 0x085d2910 nReadPicture] calls `SI_pictureRead` and returns 1 whatever it said,
// [soft 0x085d2930 nWritePicture] does the same with `SI_pictureWrite`, and
// [soft 0x085d2900 SI_pictDisplayError] stores its argument in the flag `SI_pictureOpenFile`
// passes to `SI_softOpen` (whether a file that cannot be opened is reported). In this crate a
// refusal is a returned value, so there is no flag.

/// The header of a picture file up to and including the channel packets
/// [soft 0x085d4a34 SI_pictureWriteHeader]: file header (version 4.0, the comment or
/// "No comment..."), `PICT`, the picture chunk, then the packets.
///
/// `planes` says which of red, green, blue, alpha exist.
/// - To a file: one mixed packet declared for red, green and blue whatever planes exist,
///   and one for alpha if there is an alpha plane.
/// - To standard output (a name starting with `-`): one raw packet per plane present, each
///   chained to the next.
///
/// As in [`Picture::write`], the chunk's spare word is stale stack in the original and
/// `p.pad` here.
pub fn write_header(p: &Picture, planes: [bool; 4], comment: Option<&[u8]>, to_stdout: bool) -> Vec<u8> {
    let mut out = Vec::new();
    write_bin_header(&mut out, MAGIC_GENERAL, PICTURE_VERSION, comment);
    out.extend_from_slice(&PICT.to_be_bytes());
    write_pict_chunk(&mut out, p);
    if to_stdout {
        for (i, bit) in [RED, GREEN, BLUE, ALPHA].into_iter().enumerate() {
            if planes[i] {
                let chained = planes[i + 1..].iter().any(|&b| b) as u8;
                out.extend_from_slice(&[chained, 8, TYPE_RAW, bit]);
            }
        }
    } else {
        out.extend_from_slice(&[planes[3] as u8, 8, TYPE_MIXED, RED | GREEN | BLUE]);
        if planes[3] {
            out.extend_from_slice(&[0, 8, TYPE_MIXED, ALPHA]);
        }
    }
    out
}

/// Writes a picture to an already open file [soft 0x085d6520 SI_fdpictureWrite]: always the
/// file layout (one mixed packet for the colours, one for alpha if present), whatever the
/// stream is; version 4.0 from a constant of its own (0x086733a8). The bytes are those of
/// [`Picture::write`] to a file.
pub fn fd_write(p: &Picture, comment: Option<&[u8]>) -> Vec<u8> {
    p.write(comment, false)
}

/// Writes a packed picture as a picture file [soft 0x085d5f64 SI_pictureRGBAWrite].
///
/// The header is [`write_header`] for a picture of the record's width, height and ratio with
/// **`fields` = 3** (the record's own value is not used) and an alpha plane; then, from the
/// record's last row to its first (the file is top row first), each row as a mixed sequence
/// of red, green and blue and a mixed sequence of alpha.
///
/// For a name starting with `-` the original builds the packets from three plane pointers
/// it never set; what it writes then depends on stale stack and is not determined: `None`.
pub fn rgba_write(p: &PictureRgba, comment: Option<&[u8]>, to_stdout: bool) -> Option<Vec<u8>> {
    if to_stdout {
        return None;
    }
    let head = Picture { width: p.width, height: p.height, ratio: p.ratio, fields: 3, pad: 0, planes: Default::default() };
    let mut out = write_header(&head, [true; 4], comment, false);
    let (w, h) = (p.width.max(0) as usize, p.height.max(0) as usize);
    let mut rows: [Vec<u8>; 4] = [vec![0; w], vec![0; w], vec![0; w], vec![0; w]];
    for y in (0..h).rev() {
        for x in 0..w {
            for (c, row) in rows.iter_mut().enumerate() {
                row[x] = p.data[4 * (y * w + x) + c];
            }
        }
        write_mixed(&mut out, &[&rows[0], &rows[1], &rows[2]]);
        write_mixed(&mut out, &[&rows[3]]);
    }
    Some(out)
}

/// Why `SI_fdpictureOpenFile` returns 0.
#[derive(Clone, Debug, PartialEq)]
pub enum FdOpenError {
    /// "Cannot read input file \n" (the name in the message is empty): shorter than a
    /// header, a tag or a picture chunk.
    CannotRead,
    /// The magic is not the general one. No message; the version and the comment have
    /// already been handed to the caller.
    NotSoftimage { version: f32, comment: Vec<u8> },
    /// "File  is not a Softimage Picture file.\n"
    NotPicture,
}

impl FdOpenError {
    /// The text shown, if any.
    pub fn message(&self) -> Option<&'static str> {
        match self {
            FdOpenError::CannotRead => Some("Cannot read input file \n"),
            FdOpenError::NotSoftimage { .. } => None,
            FdOpenError::NotPicture => Some("File  is not a Softimage Picture file.\n"),
        }
    }
}

impl<'a> OpenPicture<'a> {
    /// Reads the header of a picture from an already open file
    /// [soft 0x085d6350 SI_fdpictureOpenFile]. Unlike [`OpenPicture::open`] it accepts a zero
    /// width or height and leaves a zero ratio as it is.
    pub fn open_fd(data: &'a [u8]) -> Result<Self, FdOpenError> {
        let mut stream = Stream::new(data);
        let h = read_chunk(&mut stream).ok_or(FdOpenError::CannotRead)?;
        let comment = h.comment_bytes().to_vec();
        if h.magic != MAGIC_GENERAL {
            return Err(FdOpenError::NotSoftimage { version: h.version, comment });
        }
        let tag = stream.be_u32().ok_or(FdOpenError::CannotRead)?;
        if tag != PICT {
            return Err(FdOpenError::NotPicture);
        }
        let picture = read_pict_chunk(&mut stream).ok_or(FdOpenError::CannotRead)?;
        Ok(OpenPicture { packets: read_packets(&mut stream), stream, picture, version: h.version, comment })
    }
}

/// The channel packets: read while the first byte is non-zero. At the end of the file the
/// original's buffer keeps its last content and the loop does not end; here it does.
fn read_packets(s: &mut Stream) -> Vec<Packet> {
    let mut packets = Vec::new();
    let mut raw = [0u8; 4];
    loop {
        let complete = s.read_record(&mut raw);
        packets.push(Packet { size: raw[1], kind: raw[2], channel: raw[3] });
        if raw[0] == 0 || !complete {
            return packets;
        }
    }
}

/// C's `%f` for a float argument (six decimals).
fn percent_f(v: f32) -> String {
    let v = v as f64;
    if v.is_nan() {
        "nan".into()
    } else if v.is_infinite() {
        if v > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        format!("{v:.6}")
    }
}

/// The text `infopic` prints about a picture file [soft 0x085d4638 SI_pictureInfo].
///
/// `Ok` is the text written to the output file when the call returns 1. `Err` carries the
/// text written before the failure and the message: a file that is not a Softimage file gets
/// its line in the output itself (with the file's base name, `base`), the other failures go
/// to the status line with the name as given (`name`).
///
/// The names it prints give the meaning of the header fields:
/// - `fields`: 0 "NO FIELD", 1 "ODD FIELD", 2 "EVEN FIELD", 3 "FULL FRAME"; any other value
///   prints nothing after " fields = ".
/// - a packet's type byte, high four bits: 0x00 unsigned integer, 0x10 signed integer, 0x20
///   float; low four bits: 0 uncompressed, 1 pure run-length, 2 mixed run-length. Other
///   values print no line.
pub fn picture_info(data: &[u8], name: &str, base: &str) -> Result<String, (String, String)> {
    let mut s = Stream::new(data);
    let mut out = String::new();
    let cannot = |out: String| Err((out, format!("Cannot read input file {name}\n")));
    let Some(h) = read_chunk(&mut s) else { return cannot(out) };
    if h.magic != MAGIC_GENERAL {
        out.push_str(&format!("The file {base} is not a SOFTIMAGE file\n"));
        return Err((out, String::new()));
    }
    out.push_str(&format!("done with version {}\n", percent_f(h.version)));
    out.push_str(&format!("{}\n", String::from_utf8_lossy(h.comment_bytes())));
    let Some(tag) = s.be_u32() else { return cannot(out) };
    if tag != PICT {
        return Err((out, format!("File {name} is not a Softimage Picture file.\n")));
    }
    let Some(p) = read_pict_chunk(&mut s) else { return cannot(out) };
    out.push_str(&format!(" width  = {}\n height = {}\n ratio  = {}\n fields = ", p.width, p.height, percent_f(p.ratio)));
    out.push_str(match p.fields {
        0 => "NO FIELD\n",
        1 => "ODD FIELD\n",
        2 => "EVEN FIELD\n",
        3 => "FULL FRAME\n",
        _ => "",
    });
    let mut raw = [0u8; 4];
    loop {
        let complete = s.read_record(&mut raw);
        out.push_str(&format!("  - nb bits: {}\n", raw[1] as i8));
        out.push_str(match raw[2] & 0xf0 {
            0x00 => "    unsigned integer\n",
            0x10 => "    signed integer\n",
            0x20 => "    float\n",
            _ => "",
        });
        out.push_str(match raw[2] & 0x0f {
            0 => "    uncompressed\n",
            1 => "    pure run-length\n",
            2 => "    mixed run-length\n",
            _ => "",
        });
        out.push_str("    { ");
        for (bit, text) in [(RED, "RED "), (GREEN, "GREEN "), (BLUE, "BLUE "), (ALPHA, "ALPHA ")] {
            if raw[3] & bit != 0 {
                out.push_str(text);
            }
        }
        out.push_str("}\n");
        // the original loops for ever on a chained packet at the end of the file
        if raw[0] == 0 || !complete {
            out.push('\n');
            return Ok(out);
        }
    }
}

/// What `SI_pictureGetInfo` hands back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GetInfo {
    pub width: i32,
    pub height: i32,
    pub ratio: f32,
    /// 1 when any packet names alpha.
    pub has_alpha: i16,
}

/// Why `SI_pictureGetInfo` returns 0, with its message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GetInfoError {
    /// "The file %s is not a Softimage file\n" (with the file's base name)
    NotSoftimage,
    /// "%s is not a Picture file\n"
    NotPicture,
    /// The file ends inside the tag. No message.
    NoTag,
}

impl GetInfoError {
    pub fn message(&self, base: &str) -> Option<String> {
        match self {
            GetInfoError::NotSoftimage => Some(format!("The file {base} is not a Softimage file\n")),
            GetInfoError::NotPicture => Some(format!("{base} is not a Picture file\n")),
            GetInfoError::NoTag => None,
        }
    }
}

/// Size, ratio and presence of alpha of a picture file [soft 0x085e2de4 SI_pictureGetInfo].
///
/// The original builds the name itself: the name as given when the frame number is
/// negative, otherwise `"<name>.<frame>"`, opened with the extension `.pic`.
/// Odd cases reproduced: a file shorter than a header **succeeds with everything zero**, and
/// so does one that ends inside the picture chunk.
pub fn picture_get_info(data: &[u8]) -> Result<GetInfo, GetInfoError> {
    let mut s = Stream::new(data);
    let mut info = GetInfo::default();
    let Some(h) = read_chunk(&mut s) else { return Ok(info) };
    if h.magic != MAGIC_GENERAL {
        return Err(GetInfoError::NotSoftimage);
    }
    let tag = s.be_u32().ok_or(GetInfoError::NoTag)?;
    if tag != PICT {
        return Err(GetInfoError::NotPicture);
    }
    if let Some(p) = read_pict_chunk(&mut s) {
        info = GetInfo { width: p.width as i32, height: p.height as i32, ratio: p.ratio, has_alpha: 0 };
        if read_packets(&mut s).iter().any(|p| p.channel & ALPHA != 0) {
            info.has_alpha = 1;
        }
    }
    Ok(info)
}

/// Width and height of a picture file [soft 0x085e30c4 SI_pictureGetDimensions]: the header
/// as [`OpenPicture::open`] reads it. The original returns 0 for success and 4 for any
/// failure (no name, or the file refused).
pub fn picture_get_dimensions(data: &[u8]) -> Result<(i16, i16), PictureError> {
    let f = OpenPicture::open(data)?;
    Ok((f.picture.width, f.picture.height))
}

/// The cases in which `SI_constructPictureComment` prints to standard error and **ends the
/// program** with status −1.
#[derive(Clone, Debug, PartialEq)]
pub enum CommentError {
    /// "constructPictureComment: Can't read comment.\n": the file is shorter than a header.
    CannotRead,
    /// "constructPictureComment: Command inconnu: %d"
    UnknownCommand(i32),
}

/// Builds the comment for a picture derived from another
/// [soft 0x085d6130 SI_constructPictureComment]. `source` is the bytes of the picture file
/// the comment is taken from, `comment` the caller's 80-byte comment buffer.
///
/// - Command 0: the source's comment replaces `comment`.
/// - Command 1: if the source's comment starts with `SUBREGION`, `Subregion` or `subregion`,
///   its first line is put in front: `comment` becomes "<first line>\n<comment>", cut to 79
///   bytes. Otherwise `comment` is left alone. This is how the sub-region of a partial
///   render survives the tools that process the picture.
///
/// The magic of the source is not looked at.
pub fn construct_picture_comment(source: &[u8], command: i32, comment: &mut Vec<u8>) -> Result<(), CommentError> {
    let h = read_chunk(&mut Stream::new(source)).ok_or(CommentError::CannotRead)?;
    let theirs = h.comment_bytes();
    match command {
        0 => *comment = theirs.to_vec(),
        1 => {
            if [&b"SUBREGION"[..], b"Subregion", b"subregion"].iter().any(|t| theirs.starts_with(t)) {
                let line = theirs.split(|&b| b == b'\n').next().unwrap_or(&[]);
                let mut joined = line.to_vec();
                joined.push(b'\n');
                joined.extend_from_slice(comment);
                joined.truncate(79);
                *comment = joined;
            }
        }
        other => return Err(CommentError::UnknownCommand(other)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Picture {
        let (w, h) = (5usize, 3usize);
        let plane = |k: usize| Some((0..w * h).map(|i| (i * 3 + k) as u8).collect::<Vec<u8>>());
        Picture { width: 5, height: 3, ratio: 1.5, fields: 3, pad: 0, planes: [plane(0), plane(1), plane(2), plane(3)] }
    }

    #[test]
    fn header_matches_the_picture_writer() {
        let p = sample();
        for to_stdout in [false, true] {
            let full = p.write(Some(b"c"), to_stdout);
            let head = write_header(&p, [true; 4], Some(b"c"), to_stdout);
            assert_eq!(&full[..head.len()], &head[..]);
            assert_eq!(head.len(), 104 + if to_stdout { 16 } else { 8 });
        }
        assert_eq!(&write_header(&p, [true, false, false, false], None, false)[104..], &[0, 8, 2, 0xe0]);
        assert_eq!(&write_header(&p, [false, true, false, true], None, true)[104..], &[1, 8, 0, 0x40, 0, 8, 0, 0x10]);
        assert_eq!(fd_write(&p, None), p.write(None, false));
    }

    #[test]
    fn packed_write_reads_back() {
        let p = sample();
        let mut q = PictureRgba::from_planes(&p).unwrap();
        for i in 0..15 {
            q.data[4 * i + 3] = 200 + i as u8;
        }
        q.fields = 1;
        let bytes = rgba_write(&q, None, false).unwrap();
        let f = Picture::read(&bytes).unwrap();
        assert_eq!(f.picture.fields, 3); // always written as a full frame
        assert_eq!(f.picture.planes[0], p.planes[0]);
        assert_eq!(f.picture.planes[3].as_ref().unwrap()[0], 210); // the file's first row is the record's last
        assert_eq!(crate::read_rgba(&bytes).unwrap().data, q.data);
        assert_eq!(rgba_write(&q, None, true), None);
    }

    #[test]
    fn info_text() {
        let p = sample();
        let text = picture_info(&p.write(Some(b"hello"), false), "a", "a.pic").unwrap();
        assert_eq!(
            text,
            "done with version 4.000000\nhello\n width  = 5\n height = 3\n ratio  = 1.500000\n fields = FULL FRAME\n\
             \x20 - nb bits: 8\n    unsigned integer\n    mixed run-length\n    { RED GREEN BLUE }\n\
             \x20 - nb bits: 8\n    unsigned integer\n    mixed run-length\n    { ALPHA }\n\n"
        );
        let mut bad = p.write(None, false);
        bad[3] = 0;
        assert_eq!(picture_info(&bad, "a", "a.pic").unwrap_err(), ("The file a.pic is not a SOFTIMAGE file\n".into(), String::new()));
        let short = &p.write(None, false)[..90];
        assert_eq!(picture_info(short, "a", "a.pic").unwrap_err().1, "Cannot read input file a\n");
    }

    #[test]
    fn get_info_and_dimensions() {
        let p = sample();
        let bytes = p.write(None, false);
        assert_eq!(picture_get_info(&bytes), Ok(GetInfo { width: 5, height: 3, ratio: 1.5, has_alpha: 1 }));
        assert_eq!(picture_get_info(&bytes[..40]), Ok(GetInfo::default())); // too short: success, all zero
        assert_eq!(picture_get_info(&bytes[..90]), Err(GetInfoError::NoTag));
        assert_eq!(picture_get_info(&bytes[..95]), Ok(GetInfo::default()));
        let mut b = bytes.clone();
        b[88] = b'Q';
        assert_eq!(picture_get_info(&b), Err(GetInfoError::NotPicture));
        assert_eq!(picture_get_dimensions(&bytes), Ok((5, 3)));
        let fd = OpenPicture::open_fd(&bytes).unwrap();
        assert_eq!((fd.picture.width, fd.packets.len(), fd.stream.tell()), (5, 2, 112));
        b[88] = b'P';
        b[3] = 0x35;
        assert!(matches!(OpenPicture::open_fd(&b), Err(FdOpenError::NotSoftimage { version, .. }) if version == 4.0));
    }

    #[test]
    fn subregion_comment_is_carried_over() {
        let p = sample();
        let src = p.write(Some(b"SUBREGION 10 20 IN 640 480\nmore"), false);
        let mut c = b"mine".to_vec();
        construct_picture_comment(&src, 1, &mut c).unwrap();
        assert_eq!(c, b"SUBREGION 10 20 IN 640 480\nmine");
        let mut c = vec![b'x'; 70];
        construct_picture_comment(&src, 1, &mut c).unwrap();
        assert_eq!(c.len(), 79);
        let other = p.write(Some(b"plain"), false);
        let mut c = b"mine".to_vec();
        construct_picture_comment(&other, 1, &mut c).unwrap();
        assert_eq!(c, b"mine");
        construct_picture_comment(&other, 0, &mut c).unwrap();
        assert_eq!(c, b"plain");
        assert_eq!(construct_picture_comment(&other, 2, &mut c), Err(CommentError::UnknownCommand(2)));
    }
}
