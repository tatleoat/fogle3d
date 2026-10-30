// SPDX-License-Identifier: MIT
//! Picture files and appended icons (finding 0003).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `picture.c`, `geticon.c`.
//!
//! The readers reproduce what the original leaves in memory for any input, including files
//! that are cut short or badly coded: the original keeps what it read, leaves the rest zero
//! and still reports success, and so does this.

use si_file::{read_chunk, write_bin_header, Stream, MAGIC_GENERAL};

mod ci;
mod icon;
mod info;
mod reader;
mod rgba;
mod util;
pub use ci::{crop_ci, enlarge_ci, resize_ci, PictureCi};
pub use icon::{
    add_icon_to_pic, db_chapter_icon_get, db_file_icon_get, file_icon_rgba_read, get_soft_icon, icon_from_rgba, icon_size,
    pict_read_as_icon, pict_read_as_icon_rgba, read_icon_rgba, soft_icon_resource, ICON_MAX,
};
pub use info::{
    construct_picture_comment, fd_write, picture_get_dimensions, picture_get_info, picture_info, rgba_write, write_header, CommentError,
    FdOpenError, GetInfo, GetInfoError,
};
pub use reader::{read_bw_scale, read_rgba, read_rgba_scale, read_scaled_rgba_scale, OpenPicture, ScaleX};
pub use rgba::{alpha_remove, border, crop, enlarge, resize, swap_bits_rgb, swap_bits_rgba, Irect, PictureRgba};
pub use util::{file_size_message, pack_bit, soft_open, split_path, un_pack_bit, SoftOpen};

/// Largest single allocation a reader makes for a size taken from a file: 256 MB.
///
/// The original runs in a 32-bit address space, where its allocator fails for sizes of this
/// order and the reader gives up; this draws the same line lower and at a fixed place.
pub const MAX_ALLOC: usize = 1 << 28;
/// Below this many bytes a reader allocates what the header asks for without looking at the
/// file's length, so that a small file cut short reads exactly as in the original (the part
/// not reached stays zero).
pub const SMALL_ALLOC: usize = 1 << 22;

/// Whether a reader may allocate `bytes` for a picture of `height` rows when `left` bytes of
/// the file remain and a row cannot be coded in fewer than `min_row` bytes.
///
/// Not in the original, which trusts the header (and, in its packed readers, asks
/// `SI_swapSpaceAvailable`). Small requests always pass; larger ones only when the file is
/// long enough to hold that many rows, and never above [`MAX_ALLOC`].
pub fn may_allocate(bytes: usize, height: usize, min_row: usize, left: usize) -> bool {
    bytes <= SMALL_ALLOC || (bytes <= MAX_ALLOC && height.saturating_mul(min_row) <= left)
}

/// Version the picture writer puts in the header (a constant of its own, 0x08673360).
pub const PICTURE_VERSION: f32 = 4.0;
pub(crate) const PICT: u32 = u32::from_be_bytes(*b"PICT");
pub(crate) const ICON: &[u8; 4] = b"ICON";

/// Channel bits of a packet, in the order the planes are stored.
pub const RED: u8 = 0x80;
pub const GREEN: u8 = 0x40;
pub const BLUE: u8 = 0x20;
pub const ALPHA: u8 = 0x10;
pub(crate) const PLANE_BITS: [u8; 4] = [RED, GREEN, BLUE, ALPHA];

/// Packet types (low four bits of the type byte).
pub const TYPE_RAW: u8 = 0;
pub const TYPE_MIXED: u8 = 2;

/// A picture in memory: the original's 28-byte record, with its four plane pointers as
/// options. Planes are `width × height` bytes, first row first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    pub width: i16,
    pub height: i16,
    pub ratio: f32,
    pub fields: i16,
    /// Carries no meaning. The picture writer leaves it uninitialised (see [`Picture::write`]).
    pub pad: i16,
    /// Red, green, blue, alpha.
    pub planes: [Option<Vec<u8>>; 4],
}

/// One channel packet of a picture file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packet {
    pub size: u8,
    pub kind: u8,
    pub channel: u8,
}

/// What the header of a picture file says, beyond the picture itself.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureFile {
    pub picture: Picture,
    pub version: f32,
    pub comment: Vec<u8>,
    pub packets: Vec<Packet>,
    /// Offset of the first byte after the scanline data that was read.
    pub end: usize,
}

/// The cases in which the original refuses a file, each with the message it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PictureError {
    /// "Cannot read input file": shorter than a header, a tag or a picture chunk.
    CannotRead,
    /// "The file … is not a SOFTIMAGE file": the magic is not the general one.
    NotSoftimage,
    /// "File … is not a SOFTIMAGE Picture": no `PICT` tag.
    NotPicture,
    /// "Picture … has invalid size": a zero width or height. Negative sizes, which the
    /// original lets through into its allocations, are refused here too.
    InvalidSize,
    /// "image too big, memory insufficient": the packed readers ask `SI_swapSpaceAvailable`
    /// before allocating. Here: a header asking for more than the file could hold (see
    /// [`may_allocate`]).
    TooBig,
    /// No message: an allocation came back empty and the reader returned 0. The program's
    /// allocators return nothing for a count of zero ([soft 0x080d0f50 SI_calloc],
    /// [soft 0x080d0ee8 SI_malloc]), which is how a half-size read of a picture one pixel
    /// wide or high ends.
    NothingAllocated,
}

impl PictureError {
    /// The text the original shows for this refusal (through `SI_statusDisplayMessage`, level
    /// 3), for a file of the given name and, for `InvalidSize`, the size found.
    pub fn message(&self, name: &str, width: i16, height: i16) -> String {
        match self {
            PictureError::CannotRead => format!("Cannot read input file {name}\n"),
            PictureError::NotSoftimage => format!("The file {name} is not a SOFTIMAGE file.\n"),
            PictureError::NotPicture => format!("File {name} is not a SOFTIMAGE Picture file.\n"),
            PictureError::InvalidSize => format!("Picture {name} has invalid size {width} x {height}\n"),
            PictureError::TooBig => "image too big, memory insufficient".to_string(),
            PictureError::NothingAllocated => String::new(),
        }
    }
}

pub(crate) struct Opened {
    pub(crate) picture: Picture,
    pub(crate) version: f32,
    pub(crate) comment: Vec<u8>,
    pub(crate) packets: Vec<Packet>,
}

/// [soft 0x085d2970 SI_pictureOpenFile] [soft 0x083f98a0 SI_fix_SI_PictChunk]
pub(crate) fn open(s: &mut Stream) -> Result<Opened, PictureError> {
    open_with(s, false)
}

/// `negative`: let a negative width or height through, as the original does (it refuses
/// only a size of zero).
pub(crate) fn open_with(s: &mut Stream, negative: bool) -> Result<Opened, PictureError> {
    let h = read_chunk(s).ok_or(PictureError::CannotRead)?;
    if h.magic != MAGIC_GENERAL {
        return Err(PictureError::NotSoftimage);
    }
    let tag = s.be_u32().ok_or(PictureError::CannotRead)?;
    if tag != PICT {
        return Err(PictureError::NotPicture);
    }
    let mut picture = read_pict_chunk(s).ok_or(PictureError::CannotRead)?;
    if picture.width == 0 || picture.height == 0 || (!negative && (picture.width < 0 || picture.height < 0)) {
        return Err(PictureError::InvalidSize);
    }
    if picture.ratio == 0.0 {
        // an integer division in the original: 640 × 480 with no stored ratio gives 1.0
        picture.ratio = (picture.width as i32 / picture.height as i32) as f32;
    }
    // packets are read while the first byte ("chained") is non-zero; a file that ends here
    // leaves the packet as it was, so a chained packet repeats until memory runs out in the
    // original. Here the list ends at the end of the file.
    let mut packets = Vec::new();
    let mut raw = [0u8; 4];
    loop {
        let complete = s.read_record(&mut raw);
        packets.push(Packet { size: raw[1], kind: raw[2], channel: raw[3] });
        if raw[0] == 0 || !complete {
            break;
        }
    }
    // the original copies the comment with strcpy from a field that may have no terminator;
    // we stop at the field's end
    Ok(Opened { picture, version: h.version, comment: h.comment_bytes().to_vec(), packets })
}

/// The 12-byte picture chunk: width, height, ratio, fields, padding.
pub(crate) fn read_pict_chunk(s: &mut Stream) -> Option<Picture> {
    let mut b = [0u8; 12];
    if !s.read_record(&mut b) {
        return None;
    }
    Some(Picture {
        width: i16::from_be_bytes([b[0], b[1]]),
        height: i16::from_be_bytes([b[2], b[3]]),
        ratio: f32::from_bits(u32::from_be_bytes([b[4], b[5], b[6], b[7]])),
        fields: i16::from_be_bytes([b[8], b[9]]),
        pad: i16::from_be_bytes([b[10], b[11]]),
        planes: Default::default(),
    })
}

pub(crate) fn write_pict_chunk(out: &mut Vec<u8>, p: &Picture) {
    out.extend_from_slice(&p.width.to_be_bytes());
    out.extend_from_slice(&p.height.to_be_bytes());
    out.extend_from_slice(&p.ratio.to_bits().to_be_bytes());
    out.extend_from_slice(&p.fields.to_be_bytes());
    out.extend_from_slice(&p.pad.to_be_bytes());
}

/// One row of `planes.len()` planes in the mixed coding, written at `at` in each plane
/// [soft 0x085d2f8c si_channelReadMixed] (and, for fixed plane counts,
/// [soft 0x085d30f8 SI_channelRead1mixed] [soft 0x085d320c SI_channelRead3mixed]).
///
/// Count byte `c`: below 0x80, `c + 1` pixels follow; above, one pixel follows and is repeated
/// `c − 0x7f` times; exactly 0x80, a 16-bit count follows and then the pixel.
/// `seek_on_overrun` is the one difference between the general routine and the fixed ones:
/// on a run that passes the end of the row the general one seeks to the end of the file.
/// Returns false as the original returns 0; pixels written before that stay.
pub(crate) fn read_mixed(s: &mut Stream, width: usize, planes: &mut [&mut [u8]], at: usize, seek_on_overrun: bool) -> bool {
    let mut x = 0usize;
    while x < width {
        let Some(c) = s.getc() else { return false };
        let count;
        if c < 0x80 {
            count = c as usize + 1;
            if x + count > width {
                if seek_on_overrun {
                    s.seek_end();
                }
                return false;
            }
            for i in 0..count {
                for p in planes.iter_mut() {
                    let Some(b) = s.getc() else { return false };
                    p[at + x + i] = b;
                }
            }
        } else {
            if c == 0x80 {
                // read with fread, so a short read leaves the count as whatever arrived; the
                // original then tests feof and gives up
                let v = s.be_u16();
                if s.feof() {
                    return false;
                }
                count = v.unwrap_or(0) as usize;
            } else {
                count = c as usize - 0x7f;
            }
            if x + count > width {
                if seek_on_overrun {
                    s.seek_end();
                }
                return false;
            }
            for p in planes.iter_mut() {
                let Some(b) = s.getc() else { return false };
                p[at + x..at + x + count].fill(b);
            }
        }
        x += count;
    }
    true
}

/// One row of raw planes [soft 0x085d33e4 channelReadRaw]: a single plane is read in one
/// piece; several are interleaved pixel by pixel.
pub(crate) fn read_raw(s: &mut Stream, width: usize, planes: &mut [&mut [u8]], at: usize) -> bool {
    if s.feof() {
        return false;
    }
    if planes.len() == 1 {
        s.read(&mut planes[0][at..at + width]);
        if s.feof() {
            return false;
        }
    } else {
        for x in 0..width {
            for p in planes.iter_mut() {
                let Some(b) = s.getc() else { return false };
                p[at + x] = b;
            }
        }
    }
    true
}

impl Picture {
    pub fn plane(&self, bit: u8) -> Option<&[u8]> {
        PLANE_BITS.iter().position(|&b| b == bit).and_then(|i| self.planes[i].as_deref())
    }

    /// Reads a picture file [soft 0x085d2dfc SI_pictureRead] [soft 0x085d2cfc SI_scanlineRead].
    ///
    /// Every packet gets its own zeroed planes; rows are read packet by packet until the
    /// height is reached or the end-of-file flag is set. A file that ends early or holds a bad
    /// run still succeeds, with the unread part zero. Packets of a type other than raw and
    /// mixed contribute no data. If two packets name the same plane, the picture keeps the
    /// later one's (the original leaks the earlier buffer).
    pub fn read(data: &[u8]) -> Result<PictureFile, PictureError> {
        let mut s = Stream::new(data);
        let Opened { mut picture, version, comment, packets } = open(&mut s)?;
        let (w, h) = (picture.width as usize, picture.height as usize);
        // Not in the original: it allocates whatever the header says. A header claiming more
        // than the file could hold is refused before anything is allocated (see MAX_ALLOC).
        let buffers: usize = packets.iter().map(|p| (p.channel >> 4).count_ones() as usize).sum();
        let min_row: usize = packets.iter().filter(|p| matches!(p.kind & 0xf, TYPE_RAW | TYPE_MIXED)).count().max(1);
        if !may_allocate((w * h).saturating_mul(buffers), h, min_row, s.len() - s.tell()) {
            return Err(PictureError::InvalidSize);
        }
        // per packet: (index of the plane in the picture, the packet's own buffer)
        let mut bufs: Vec<Vec<(usize, Vec<u8>)>> = packets
            .iter()
            .map(|p| (0..4).filter(|&i| p.channel & PLANE_BITS[i] != 0).map(|i| (i, vec![0u8; w * h])).collect())
            .collect();
        let mut pos = vec![0usize; packets.len()]; // each packet's write position: it advances only on a row read in full
        'rows: for _ in 0..h {
            for (k, p) in packets.iter().enumerate() {
                let mut planes: Vec<&mut [u8]> = bufs[k].iter_mut().map(|(_, b)| b.as_mut_slice()).collect();
                let ok = match p.kind & 0xf {
                    TYPE_RAW => read_raw(&mut s, w, &mut planes, pos[k]),
                    TYPE_MIXED => read_mixed(&mut s, w, &mut planes, pos[k], true),
                    _ => true,
                };
                if ok && matches!(p.kind & 0xf, TYPE_RAW | TYPE_MIXED) {
                    pos[k] += w;
                }
                if s.feof() {
                    break 'rows;
                }
            }
        }
        for packet in bufs.drain(..) {
            for (i, b) in packet {
                picture.planes[i] = Some(b);
            }
        }
        Ok(PictureFile { picture, version, comment, packets, end: s.tell() })
    }

    /// Writes a picture file [soft 0x085d34a0 SI_pictureWrite].
    ///
    /// To a file: one mixed packet declared for red, green and blue, and one for alpha if the
    /// picture has an alpha plane. To standard output (`to_stdout`, a file name starting with
    /// `-` in the original): one raw packet per plane present, each row plane after plane.
    ///
    /// One deliberate difference: the original never sets the chunk's padding word, so its
    /// files carry two bytes of stale stack there. This writes `self.pad`.
    pub fn write(&self, comment: Option<&[u8]>, to_stdout: bool) -> Vec<u8> {
        let mut out = Vec::new();
        write_bin_header(&mut out, MAGIC_GENERAL, PICTURE_VERSION, comment);
        out.extend_from_slice(&PICT.to_be_bytes());
        write_pict_chunk(&mut out, self);
        let has = |i: usize| self.planes[i].is_some();
        if !to_stdout {
            out.extend_from_slice(&[has(3) as u8, 8, TYPE_MIXED, RED | GREEN | BLUE]);
            if has(3) {
                out.extend_from_slice(&[0, 8, TYPE_MIXED, ALPHA]);
            }
        } else {
            for i in 0..4 {
                if has(i) {
                    let chained = (i + 1..4).any(has) as u8;
                    out.extend_from_slice(&[chained, 8, TYPE_RAW, PLANE_BITS[i]]);
                }
            }
        }
        let w = self.width.max(0) as usize;
        for y in 0..self.height.max(0) as usize {
            let row = |i: usize| self.planes[i].as_ref().map(|p| &p[y * w..(y + 1) * w]);
            if to_stdout {
                for i in 0..4 {
                    if let Some(r) = row(i) {
                        out.extend_from_slice(r);
                    }
                }
            } else {
                write_row(&mut out, [row(0), row(1), row(2), row(3)]);
            }
        }
        out
    }
}

/// One row to a file [soft 0x085d3bc0 SI_channelWrite]: the colour planes that exist as one
/// mixed sequence, then alpha as another. (The original spells out all fifteen combinations
/// of present planes; they reduce to this rule.)
pub(crate) fn write_row(out: &mut Vec<u8>, rows: [Option<&[u8]>; 4]) {
    let colour: Vec<&[u8]> = rows[..3].iter().flatten().copied().collect();
    if !colour.is_empty() {
        write_mixed(out, &colour);
    }
    if let Some(a) = rows[3] {
        write_mixed(out, &[a]);
    }
}

/// One row of a picture to an open picture file [soft 0x085d3bc0 SI_channelWrite], for the
/// routines that write a picture row by row. To a file: the colour planes that exist as one
/// mixed sequence, then alpha as another. **To standard output the planes that exist are
/// written raw**, each in one piece, red first.
pub fn channel_write(out: &mut Vec<u8>, to_stdout: bool, rows: [Option<&[u8]>; 4]) {
    if to_stdout {
        for r in rows.into_iter().flatten() {
            out.extend_from_slice(r);
        }
    } else {
        write_row(out, rows);
    }
}

/// The writer's run coding of one row [soft 0x085d3ddc SI_channelWriteMixed].
///
/// A state machine with no look-ahead. It holds either a run of equal pixels or a list of
/// differing ones:
/// - differing pixels join the list; at 128 the list is written and the state emptied;
/// - a pixel equal to the list's last one ends the list *without that last pixel* and starts
///   a run of two;
/// - a different pixel ends a run; runs of 2 to 127 take the short form, longer ones the
///   16-bit form, and a run that reaches 65,535 is written at once.
pub fn write_mixed(out: &mut Vec<u8>, planes: &[&[u8]]) {
    let k = planes.len();
    let width = planes.first().map_or(0, |p| p.len());
    let pixel = |x: usize| -> [u8; 4] {
        let mut p = [0u8; 4];
        for (i, plane) in planes.iter().enumerate() {
            p[i] = plane[x];
        }
        p
    };
    let put_run = |out: &mut Vec<u8>, count: usize, p: &[u8; 4]| {
        if count < 0x80 {
            out.push((count + 0x7f) as u8);
        } else {
            out.push(0x80);
            out.extend_from_slice(&(count as u16).to_be_bytes());
        }
        out.extend_from_slice(&p[..k]);
    };
    let put_list = |out: &mut Vec<u8>, list: &[[u8; 4]]| {
        out.push((list.len() - 1) as u8);
        for p in list {
            out.extend_from_slice(&p[..k]);
        }
    };
    let mut buf: Vec<[u8; 4]> = Vec::with_capacity(128);
    let mut count = 0usize;
    let mut same = false;
    for x in 0..width {
        let px = pixel(x);
        if count == 0 {
            buf.clear();
            buf.push(px);
            count = 1;
        } else if count == 1 {
            same = px == buf[0];
            if !same {
                buf.push(px);
            }
            count = 2;
        } else {
            let eq = px == if same { buf[0] } else { buf[count - 1] };
            if eq == same {
                if !same {
                    buf.push(px);
                }
                count += 1;
                if count == 0x80 && !same {
                    put_list(out, &buf);
                    count = 0;
                }
                if count == 0xffff && same {
                    put_run(out, count, &buf[0]);
                    count = 0;
                }
            } else if !same {
                put_list(out, &buf[..count - 1]);
                buf.clear();
                buf.push(px);
                count = 2;
                same = true;
            } else {
                put_run(out, count, &buf[0]);
                buf.clear();
                buf.push(px);
                count = 1;
            }
        }
    }
    if count != 0 {
        if count == 1 || !same {
            put_list(out, &buf[..count]);
        } else {
            put_run(out, count, &buf[0]);
        }
    }
}

/// The offset of the icon a database file carries at its end, if any
/// [soft 0x085c3050 SI_fileIconExists]: the last four bytes are `ICON`, the four before them
/// the offset, big-endian.
pub fn icon_offset(data: &[u8]) -> Option<usize> {
    let n = data.len();
    if n < 8 || &data[n - 4..] != ICON {
        return None;
    }
    Some(u32::from_be_bytes([data[n - 8], data[n - 7], data[n - 6], data[n - 5]]) as usize)
}

/// Reads the icon at the end of a database file [soft 0x085c34b4 fileIconReadF]
/// [soft 0x085c3614 SI_fileIconRead].
///
/// At the icon's offset: a picture chunk with no tag and no packets, then per row a mixed
/// sequence for red, green and blue and one for alpha. All four planes always exist. The
/// original ignores the row readers' results, so a damaged icon comes back partly zero.
/// Returns the icon and the offset just past its last row.
pub fn read_icon(data: &[u8]) -> Option<(Picture, usize)> {
    let off = icon_offset(data)?;
    if off > data.len() {
        return None; // fseek beyond the end succeeds in C; the read after it then fails
    }
    let mut s = Stream::new(data);
    s.seek(off);
    let mut icon = read_pict_chunk(&mut s)?;
    icon.pad = 0; // the record comes zeroed from the allocator and the chunk's spare word is not copied
    if icon.width < 0 || icon.height < 0 {
        return None; // the original would hand a negative size to its allocator
    }
    let (w, h) = (icon.width as usize, icon.height as usize);
    let mut planes: Vec<Vec<u8>> = (0..4).map(|_| vec![0u8; w * h]).collect();
    for y in 0..h {
        let (rgb, a) = planes.split_at_mut(3);
        let mut three: Vec<&mut [u8]> = rgb.iter_mut().map(|p| p.as_mut_slice()).collect();
        read_mixed(&mut s, w, &mut three, y * w, false);
        read_mixed(&mut s, w, &mut [a[0].as_mut_slice()], y * w, false);
    }
    let mut it = planes.into_iter();
    icon.planes = [it.next(), it.next(), it.next(), it.next()];
    Some((icon, s.tell()))
}

/// Writes an icon into a database file's bytes [soft 0x085c343c SI_fileIconWrite]
/// [soft 0x085c3250 fileIconWriteF].
///
/// If the file already has an icon the new one is written over it from the old icon's
/// offset; otherwise at the end. The file is never shortened (it is opened `rb+`), so a
/// smaller icon over a larger one leaves the old tail behind it. An absent plane is written
/// as zeros. After the rows come the icon's offset and `ICON`.
pub fn write_icon(file: &mut Vec<u8>, icon: &Picture) {
    let at = match icon_offset(file) {
        Some(off) => off.min(file.len()),
        None => file.len(),
    };
    let mut out = Vec::new();
    write_pict_chunk(&mut out, icon);
    let w = icon.width.max(0) as usize;
    let zero = vec![0u8; w];
    for y in 0..icon.height.max(0) as usize {
        let row = |i: usize| Some(icon.planes[i].as_ref().map_or(&zero[..], |p| &p[y * w..(y + 1) * w]));
        write_row(&mut out, [row(0), row(1), row(2), row(3)]);
    }
    out.extend_from_slice(&(at as u32).to_be_bytes());
    out.extend_from_slice(ICON);
    let end = at + out.len();
    if file.len() < end {
        file.resize(end, 0);
    }
    file[at..end].copy_from_slice(&out);
}

/// The scanline bytes the file writer produces for a picture, without header or packets.
/// Used to compare a decoded file with its own bytes.
pub fn encode_rows(p: &Picture) -> Vec<u8> {
    let mut out = Vec::new();
    let w = p.width.max(0) as usize;
    for y in 0..p.height.max(0) as usize {
        let row = |i: usize| p.planes[i].as_ref().map(|pl| &pl[y * w..(y + 1) * w]);
        write_row(&mut out, [row(0), row(1), row(2), row(3)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_row(bytes: &[u8], width: usize, k: usize) -> Option<Vec<Vec<u8>>> {
        let mut planes = vec![vec![0u8; width]; k];
        let mut s = Stream::new(bytes);
        let mut refs: Vec<&mut [u8]> = planes.iter_mut().map(|p| p.as_mut_slice()).collect();
        let ok = read_mixed(&mut s, width, &mut refs, 0, true);
        (ok && s.tell() == bytes.len()).then_some(planes)
    }

    // a small deterministic generator: rows with runs of every length class
    fn rows(seed: u32, width: usize, k: usize) -> Vec<Vec<u8>> {
        let mut state = seed;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 16
        };
        let mut planes = vec![Vec::with_capacity(width); k];
        while planes[0].len() < width {
            let run = match next() % 6 { 0 => 1, 1 => 2, 2 => 3, 3 => 127 + next() as usize % 3, 4 => 1 + next() as usize % 40, _ => 1 };
            let px: Vec<u8> = (0..k).map(|_| (next() % 3) as u8).collect();
            for _ in 0..run.min(width - planes[0].len()) {
                for (p, v) in planes.iter_mut().zip(&px) {
                    p.push(*v);
                }
            }
        }
        planes
    }

    #[test]
    fn mixed_round_trips() {
        for seed in 0..400 {
            for k in [1usize, 3] {
                let width = 1 + (seed as usize * 37) % 700;
                let planes = rows(seed, width, k);
                let refs: Vec<&[u8]> = planes.iter().map(|p| p.as_slice()).collect();
                let mut out = Vec::new();
                write_mixed(&mut out, &refs);
                assert_eq!(decode_row(&out, width, k).as_ref(), Some(&planes), "seed {seed} k {k}");
            }
        }
    }

    #[test]
    fn writer_forms() {
        let enc = |row: &[u8]| {
            let mut out = Vec::new();
            write_mixed(&mut out, &[row]);
            out
        };
        assert_eq!(enc(&[7]), [0, 7]);
        assert_eq!(enc(&[7, 7]), [0x81, 7]); // two equal pixels are always a run
        assert_eq!(enc(&[1, 2, 2, 3]), [0, 1, 0x81, 2, 0, 3]);
        assert_eq!(enc(&[5; 127]), [0xfe, 5]); // longest short-form run
        assert_eq!(enc(&[5; 128]), [0x80, 0, 128, 5]); // 0xff is never written
        let ramp: Vec<u8> = (0..129).map(|i| i as u8).collect();
        let e = enc(&ramp);
        assert_eq!((e[0], e.len()), (0x7f, 1 + 128 + 2)); // a full list of 128, then the last pixel
        assert_eq!(&e[129..], [0, 128]);
        // the reader accepts 0xff as a run of 128 although the writer never produces it
        assert_eq!(decode_row(&[0xff, 9], 128, 1), Some(vec![vec![9u8; 128]]));
    }

    fn sample() -> Picture {
        let (w, h) = (9usize, 4usize);
        let plane = |m: usize| Some((0..w * h).map(|i| ((i / 3) * m % 5) as u8).collect::<Vec<u8>>());
        Picture { width: w as i16, height: h as i16, ratio: 1.0, fields: 3, pad: 0, planes: [plane(1), plane(2), plane(3), plane(4)] }
    }

    #[test]
    fn file_round_trip_both_layouts() {
        let p = sample();
        for to_stdout in [false, true] {
            let bytes = p.write(Some(b"test"), to_stdout);
            let f = Picture::read(&bytes).unwrap();
            assert_eq!(f.picture, p);
            assert_eq!((f.version, f.comment.as_slice(), f.end), (4.0, &b"test"[..], bytes.len()));
            assert_eq!(f.packets.len(), if to_stdout { 4 } else { 2 });
        }
        let mut rgb = sample();
        rgb.planes[3] = None;
        let bytes = rgb.write(None, false);
        assert_eq!(&bytes[104..108], &[0, 8, 2, 0xe0]);
        assert_eq!(Picture::read(&bytes).unwrap().picture, rgb);
    }

    #[test]
    fn refusals_and_truncation() {
        let p = sample();
        let bytes = p.write(None, false);
        assert_eq!(Picture::read(&bytes[..50]).unwrap_err(), PictureError::CannotRead);
        let mut b = bytes.clone();
        b[3] = 0x35;
        assert_eq!(Picture::read(&b).unwrap_err(), PictureError::NotSoftimage);
        let mut b = bytes.clone();
        b[88] = b'X';
        assert_eq!(Picture::read(&b).unwrap_err(), PictureError::NotPicture);
        let mut b = bytes.clone();
        b[92..94].fill(0);
        assert_eq!(Picture::read(&b).unwrap_err(), PictureError::InvalidSize);
        // cut inside the scanlines: still a picture, the unread rows zero
        let cut = Picture::read(&bytes[..bytes.len() - 6]).unwrap().picture;
        let full = p.planes[0].as_ref().unwrap();
        let got = cut.planes[0].as_ref().unwrap();
        assert_eq!(&got[..9], &full[..9]);
        assert_eq!(got.len(), full.len());
    }

    #[test]
    fn zero_ratio_is_an_integer_division() {
        let mut p = sample();
        p.ratio = 0.0;
        p.width = 9;
        p.height = 4;
        assert_eq!(Picture::read(&p.write(None, false)).unwrap().picture.ratio, 2.0);
    }

    #[test]
    fn icons() {
        let icon = sample();
        let mut file = sample().write(None, false);
        let body = file.len();
        assert_eq!(read_icon(&file), None);
        write_icon(&mut file, &icon);
        assert_eq!(icon_offset(&file), Some(body));
        let (got, end) = read_icon(&file).unwrap();
        assert_eq!(got, icon);
        assert_eq!(end, file.len() - 8);
        // a smaller icon over it starts at the same offset and leaves the file's length alone
        let len = file.len();
        let mut small = sample();
        small.height = 1;
        for p in small.planes.iter_mut() {
            p.as_mut().unwrap().truncate(9);
        }
        write_icon(&mut file, &small);
        assert_eq!(file.len(), len);
        assert_eq!(icon_offset(&file), Some(body)); // the old trailer still ends the file and still points here
        assert_eq!(read_icon(&file).unwrap().0, small);
        // the picture in front of the icon is untouched
        assert_eq!(Picture::read(&file).unwrap().end, body);
    }
}
