// SPDX-License-Identifier: MIT
//! The SOFTIMAGE picture file as the `Import…` and `Export…` routines of the foreign formats
//! use it: header first, then one row at a time, with the caller's own row buffers.
//!
//! Nothing here reproduces an original function of its own: the picture-file routines are
//! those of crate `si-picture` (`picture.c`); this gives them the shape the format modules
//! share. The one exception is [`si_malloc`].

use si_picture::{OpenPicture, Packet, Picture, PictureError, ALPHA, BLUE, GREEN, RED};

const PLANE_BITS: [u8; 4] = [RED, GREEN, BLUE, ALPHA];

/// A buffer as the program's allocator hands it out [soft 0x080d0ee8 SI_malloc]: a size of 0
/// gives no buffer at all; otherwise the first byte is set to 0x6f and the last to 0x6b (in
/// that order) and the rest is whatever the heap held. The unknown bytes are 0 here.
pub(crate) fn si_malloc(n: i32) -> Option<Vec<u8>> {
    // a negative size is a request for more than 2 GB, which a 32-bit C library refuses
    if n <= 0 {
        return None;
    }
    let mut v = vec![0u8; n as usize];
    v[0] = 0x6f;
    v[n as usize - 1] = 0x6b;
    Some(v)
}

/// The cases in which the picture file is refused, with the sizes the last message prints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PicError {
    /// "Cannot read input file %s\n"
    CannotRead,
    /// "The file %s is not a SOFTIMAGE file.\n"
    NotSoftimage,
    /// "File %s is not a SOFTIMAGE Picture file.\n"
    NotPicture,
    /// "Picture %s has invalid size %d x %d\n"
    InvalidSize(i16, i16),
}

impl PicError {
    pub fn message(&self, name: &str) -> String {
        match *self {
            PicError::CannotRead => PictureError::CannotRead.message(name, 0, 0),
            PicError::NotSoftimage => PictureError::NotSoftimage.message(name, 0, 0),
            PicError::NotPicture => PictureError::NotPicture.message(name, 0, 0),
            PicError::InvalidSize(w, h) => PictureError::InvalidSize.message(name, w, h),
        }
    }
}

/// A picture file open for reading rows (`si_picture::OpenPicture`).
pub struct PicIn<'a> {
    file: OpenPicture<'a>,
    pub width: i16,
    pub height: i16,
    pub ratio: f32,
    pub fields: i16,
    pub pad: i16,
    pub version: f32,
    pub comment: Vec<u8>,
    pub packets: Vec<Packet>,
}

impl<'a> PicIn<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, PicError> {
        let file = OpenPicture::open_signed(data).map_err(|e| match e {
            PictureError::NotSoftimage => PicError::NotSoftimage,
            PictureError::NotPicture => PicError::NotPicture,
            PictureError::CannotRead => PicError::CannotRead,
            _ => {
                let v = |i: usize| i16::from_be_bytes([data.get(i).copied().unwrap_or(0), data.get(i + 1).copied().unwrap_or(0)]);
                PicError::InvalidSize(v(92), v(94))
            }
        })?;
        let p = &file.picture;
        Ok(PicIn {
            width: p.width,
            height: p.height,
            ratio: p.ratio,
            fields: p.fields,
            pad: p.pad,
            version: file.version,
            comment: file.comment.clone(),
            packets: file.packets.clone(),
            file,
        })
    }

    /// `ftell`
    pub fn tell(&self) -> usize {
        self.file.stream.tell()
    }

    /// `fseek` to an offset from the start; clears the end-of-file flag.
    pub fn seek(&mut self, pos: usize) {
        self.file.stream.seek(pos);
    }

    /// One row buffer of `width` bytes for every plane some packet names, as each export
    /// routine allocates them before its row loop (with the allocator's two marker bytes).
    pub fn row_buffers(&self) -> [Option<Vec<u8>>; 4] {
        let mut bufs: [Option<Vec<u8>>; 4] = Default::default();
        for (i, bit) in PLANE_BITS.iter().enumerate() {
            if self.packets.iter().any(|p| p.channel & bit != 0) {
                bufs[i] = si_malloc(self.width as i32);
            }
        }
        bufs
    }

    /// `feof` on the input.
    pub fn at_end(&self) -> bool {
        self.file.stream.feof()
    }

    /// One packet of one row through the mixed decoder, written at `at` in the buffers
    /// (`OpenPicture::packet_read_mixed`).
    pub fn packet_mixed(&mut self, packet: usize, bufs: &mut [Option<Vec<u8>>; 4], at: usize) -> bool {
        self.file.packet_read_mixed(packet, bufs, at)
    }

    /// The next row into the row buffers (`OpenPicture::scanline_read`); the export
    /// routines ignore the result.
    pub fn scanline(&mut self, bufs: &mut [Option<Vec<u8>>; 4]) -> bool {
        self.file.scanline_read(bufs)
    }
}

/// Everything of a picture file before its rows (`si_picture::write_header`), for a picture
/// described by (width, height, ratio, fields) of which the planes `has` exist.
pub fn write_header(out: &mut Vec<u8>, to_stdout: bool, chunk: (i16, i16, f32, i16), has: [bool; 4], comment: Option<&[u8]>) {
    let p = Picture { width: chunk.0, height: chunk.1, ratio: chunk.2, fields: chunk.3, pad: 0, planes: Default::default() };
    out.extend_from_slice(&si_picture::write_header(&p, has, comment, to_stdout));
}

/// One row (`si_picture::channel_write`).
pub fn write_row(out: &mut Vec<u8>, to_stdout: bool, rows: [Option<&[u8]>; 4]) {
    si_picture::channel_write(out, to_stdout, rows);
}
