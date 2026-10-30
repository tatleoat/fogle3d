// SPDX-License-Identifier: MIT
//! SGI image files (`.sgi`, `.rgb`) (finding 0014, "SGI").
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `sgi.c` (0x085d8370–0x085d9032), and from
//! `sgiimage.c` (0x08615170–0x08616c24, the image library that came with IRIS GL, which the
//! reader and the writer are built on) the routines they reach. `sgiimage.c` belongs to the
//! IRIS GL layer's package, which has not ported it; what is here was needed for `sgi.c`.
//!
//! The library is the well-known one (a 512-byte header, an optional table of row offsets
//! and lengths, rows stored bottom-up, verbatim or run-length coded, one or two bytes a
//! sample). What this build does of its own, all reproduced:
//! - its two byte-swap helpers **return the swapped value and store nothing**, so every
//!   "swap to big-endian before writing" in `img_write` and `iclose` does nothing: the header
//!   and the row tables of a file written here are **little-endian** (a byte-reversed SGI
//!   file, which readers that know the reversed magic number accept, this one included);
//! - the header it writes last is the library's whole record, with the file descriptor and
//!   three heap pointers in bytes 0x6c to 0x97 ([`HEADER_UNDEFINED`]; zero here);
//! - two-byte samples are never brought into the machine's order when read.
//!
//! All state is at the level of bytes in memory: a row buffer is `2 × width` bytes, a 16-bit
//! sample is its two bytes read low byte first, as this machine reads them.

use crate::effect::Fault;
use si_picture::Picture;

/// The magic number, 0x01da.
pub const MAGIC: u16 = 0x1da;
/// Where rows (verbatim) or the row tables (run-length) start.
pub const DATA: u32 = 0x200;
/// Bytes of a written header that hold the writing process's file descriptor and pointers.
pub const HEADER_UNDEFINED: [std::ops::Range<usize>; 3] = [0x6c..0x70, 0x84..0x88, 0x90..0x98];

/// A file as the library sees it through a descriptor: `read`, `write`, `lseek`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fd {
    pub data: Vec<u8>,
    pub pos: usize,
    /// A number no file lies behind: `read` and `write` fail (return −1).
    pub bad: bool,
}

impl Fd {
    /// A descriptor on `data` standing at `pos`.
    pub fn at(data: Vec<u8>, pos: usize) -> Fd {
        Fd { data, pos, bad: false }
    }

    /// A descriptor number that leads nowhere.
    pub fn invalid() -> Fd {
        Fd { bad: true, ..Fd::default() }
    }

    /// `read`: the bytes there are, up to `n`.
    pub(crate) fn read(&mut self, n: usize) -> &[u8] {
        let from = self.pos.min(self.data.len());
        let to = (from + n).min(self.data.len());
        // a descriptor standing beyond the end stays where it is; the bytes there are none
        if self.pos <= self.data.len() {
            self.pos = to;
        }
        &self.data[from..to]
    }

    pub(crate) fn write(&mut self, b: &[u8]) {
        if self.data.len() < self.pos + b.len() {
            self.data.resize(self.pos + b.len(), 0);
        }
        self.data[self.pos..self.pos + b.len()].copy_from_slice(b);
        self.pos += b.len();
    }

    pub(crate) fn lseek(&mut self, to: u32) {
        // an offset above 2 GB is negative to `lseek`, which refuses it
        if to <= i32::MAX as u32 {
            self.pos = to as usize;
        }
    }
}

/// The library's record of an open image: the original's 0x98-byte `IMAGE`.
///
/// | Offset | Field |
/// |---|---|
/// | 0 | `imagic` 0x01da |
/// | 2 | `kind`: high byte 0 verbatim, 1 run-length; low byte bytes a sample |
/// | 4 | `dim`: 1 a row, 2 a plane, 3 planes |
/// | 6, 8, 10 | `xsize`, `ysize`, `zsize` |
/// | 0xc, 0x10 | `min`, `max`: least and greatest sample written |
/// | 0x14 | `wastebytes` |
/// | 0x18 | `name`, 80 bytes |
/// | 0x68 | `colormap` |
/// | 0x6c | the file descriptor |
/// | 0x70 | `flags`: 1 reading, 2 writing |
/// | 0x72 | `dorev`: the file is byte-reversed |
/// | 0x74, 0x76, 0x78 | `x`, `y`, `z`: the row last sought |
/// | 0x7a, 0x7c, 0x80 | count, pointer and base of a pixel buffer that is never made |
/// | 0x84 | `tmpbuf`: (xsize + xsize / 64) × 4 bytes |
/// | 0x88 | `offset`: where the descriptor stands, or −1 after a failed transfer |
/// | 0x8c | `rleend`: where the next run-length row goes |
/// | 0x90, 0x94 | `rowstart`, `rowsize`: ysize × zsize words each |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub imagic: u16,
    pub kind: u16,
    pub dim: u16,
    pub xsize: u16,
    pub ysize: u16,
    pub zsize: u16,
    pub min: u32,
    pub max: u32,
    pub wastebytes: u32,
    pub name: [u8; 80],
    pub colormap: u32,
    pub flags: u16,
    pub dorev: i16,
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub offset: u32,
    pub rleend: u32,
    pub rowstart: Vec<u32>,
    pub rowsize: Vec<i32>,
    /// `tmpbuf` and how much of it has ever been written (the rest is `malloc`'s).
    pub tmpbuf: Vec<u8>,
    pub tmp_set: usize,
    pub file: Fd,
    /// What went to the error handler (`i_errhdlr`), in order.
    pub errors: Vec<String>,
    /// Whether an error handler is installed (`i_errfunc`, set with `i_seterror`). The
    /// program installs its status line at start-up, so this starts as true.
    pub handler: bool,
    /// The pixel buffer of the library's pixel-by-pixel calls (`base` at 0x80) and how far
    /// it is filled, in bytes (`ptr` − `base`). Nothing in `sgi.c` makes one.
    pub base: Option<Vec<u8>>,
    pub ptr: i32,
    /// The count at 0x7a: samples left in the pixel buffer (`getpix`/`putpix` take it down).
    pub cnt: i16,
    /// Not in the original: how many bytes of the caller's buffer the last [`Image::getrow`]
    /// wrote (a run-length row may expand to fewer samples than the image is wide).
    pub row_bytes: usize,
}

impl Default for Image {
    fn default() -> Self {
        Image {
            imagic: 0,
            kind: 0,
            dim: 0,
            xsize: 0,
            ysize: 0,
            zsize: 0,
            min: 0,
            max: 0,
            wastebytes: 0,
            name: [0; 80],
            colormap: 0,
            flags: 0,
            dorev: 0,
            x: 0,
            y: 0,
            z: 0,
            offset: 0,
            rleend: 0,
            rowstart: Vec::new(),
            rowsize: Vec::new(),
            tmpbuf: Vec::new(),
            tmp_set: 0,
            file: Fd::default(),
            errors: Vec::new(),
            handler: true,
            base: None,
            ptr: 0,
            cnt: 0,
            row_bytes: 0,
        }
    }
}

/// Reverses the two bytes of each 16-bit word in the first `n` bytes
/// [soft 0x086152ac cvtshorts]. The original counts the words in a 16-bit variable: for
/// more than 32,767 of them it never ends ([`Fault::BadSize`]).
pub fn cvtshorts(buf: &mut [u8], n: i32) -> Result<(), Fault> {
    let words = n >> 1;
    if words > i16::MAX as i32 {
        return Err(Fault::BadSize);
    }
    for i in 0..words.max(0) as usize {
        buf.get_mut(2 * i..2 * i + 2).ok_or(Fault::ShortPlane)?.swap(0, 1);
    }
    Ok(())
}

/// Reverses the four bytes of each 32-bit word in the first `n` bytes
/// [soft 0x086152d4 cvtlongs]; the same 16-bit counter.
pub fn cvtlongs(buf: &mut [u32], n: i32) -> Result<(), Fault> {
    let words = n >> 2;
    if words > i16::MAX as i32 {
        return Err(Fault::BadSize);
    }
    for i in 0..words.max(0) as usize {
        let v = buf.get_mut(i).ok_or(Fault::ShortPlane)?;
        *v = v.swap_bytes();
    }
    Ok(())
}

/// What the two helpers return, and all they do [soft 0x08615170 sgi_byteSwap4]
/// [soft 0x086151a0 sgi_byteSwap2]: the value with its bytes reversed. **Nothing is stored**,
/// and no caller uses the result.
pub fn sgi_byte_swap4(v: u32) -> u32 {
    v.swap_bytes()
}

pub fn sgi_byte_swap2(v: u16) -> u16 {
    v.swap_bytes()
}

/// The size of `tmpbuf` for a row of `xsize` samples [soft 0x08616b84 ibufalloc].
pub fn ibufalloc(xsize: u16) -> usize {
    (xsize as usize + (xsize as usize >> 6)) * 4
}

/// Expands a run-length row [soft 0x08615aac img_rle_expand] of `ibpp`-byte units into
/// `obpp`-byte samples, and returns the output's length in bytes.
///
/// A unit's low seven bits are a count; 0 ends the row. With bit 7 set, that many units
/// follow as they are; otherwise the next unit is repeated that many times. Units and
/// samples of two bytes are in the machine's order. **Nothing is bounded**: the original
/// writes as many samples as the row says and reads until it meets a zero count; here a row
/// longer than `out`, and a read past `input`, are [`Fault`]s.
pub fn img_rle_expand(input: &[u8], ibpp: i32, out: &mut [u8], obpp: i32) -> Result<Result<usize, String>, Fault> {
    if !matches!((ibpp, obpp), (1, 1) | (1, 2) | (2, 1) | (2, 2)) {
        return Ok(Err(format!("rle_expand: bad bpp: {ibpp} {obpp}\n")));
    }
    let (ib, ob) = (ibpp as usize, obpp as usize);
    let unit = |at: usize| -> Result<u16, Fault> {
        let b = input.get(at..at + ib).ok_or(Fault::Uninitialised)?;
        Ok(if ib == 1 { b[0] as u16 } else { u16::from_le_bytes([b[0], b[1]]) })
    };
    let (mut i, mut o) = (0usize, 0usize);
    loop {
        let pixel = unit(i)?;
        let mut count = pixel & 0x7f;
        if count == 0 {
            return Ok(Ok(o));
        }
        i += ib;
        let literal = pixel & 0x80 != 0;
        let value = if literal { 0 } else { unit(i)? };
        if !literal {
            // one byte past a one-byte unit, two past a two-byte one
            i += ib;
        }
        while count != 0 {
            count -= 1;
            let v = if literal {
                let v = unit(i)?;
                i += ib;
                v
            } else {
                value
            };
            let dst = out.get_mut(o..o + ob).ok_or(Fault::ShortPlane)?;
            if ob == 1 {
                dst[0] = v as u8;
            } else {
                dst.copy_from_slice(&v.to_le_bytes());
            }
            o += ob;
        }
    }
}

/// Run-length codes `n` samples of `ibpp` bytes into units of `obpp` bytes
/// [soft 0x086156d4 img_rle_compact], and returns the number of units, the closing zero
/// included.
///
/// The published coder: samples up to the next three equal in a row go out as they are, in
/// stretches of at most 126 with a count unit that has bit 7 set; then the run of equal
/// samples goes out as count and value, at most 126 at a time; a zero unit closes the row.
/// One thing is this build's: with two-byte input, a run is continued while the next sample
/// equals **the low byte** of the run's value, so samples above 255 never extend a run.
pub fn img_rle_compact(input: &[u8], ibpp: i32, obpp: i32, n: i32) -> Result<Result<Vec<u8>, String>, Fault> {
    if !matches!((ibpp, obpp), (1, 1) | (1, 2) | (2, 1) | (2, 2)) {
        return Ok(Err(format!("rle_compact: bad bpp: {ibpp} {obpp}\n")));
    }
    let (ib, ob) = (ibpp as usize, obpp as usize);
    let n = n.max(0) as usize;
    let at = |k: usize| -> Result<u16, Fault> {
        // with one-byte input the "next three" test reads up to one byte past the samples
        let b = input.get(ib * k..ib * k + ib).ok_or(Fault::ShortPlane)?;
        Ok(if ib == 1 { b[0] as u16 } else { u16::from_le_bytes([b[0], b[1]]) })
    };
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, v: u16| {
        if ob == 1 {
            out.push(v as u8);
        } else {
            out.extend_from_slice(&v.to_le_bytes());
        }
    };
    let mut i = 0usize;
    while i < n {
        let mut p = i + 2;
        while p < n && (at(p - 2)? != at(p - 1)? || at(p - 2)? != at(p)?) {
            p += 1;
        }
        let q = p - 2;
        let mut count = q.wrapping_sub(i) as isize as i32;
        while count != 0 {
            let todo = if count > 126 { 126 } else { count };
            count -= todo;
            put(&mut out, (todo as u8 | 0x80) as u16);
            // the inner counter is 16 bits wide
            let mut k = todo as i16 as u16;
            while k != 0 {
                k -= 1;
                put(&mut out, at(i)?);
                i += 1;
            }
        }
        let cc = at(q)?;
        i = p - 1;
        if i < n {
            let same = |v: u16| if ib == 1 { v == cc } else { v == (cc & 0xff) };
            while same(at(i)?) {
                i += 1;
                if i >= n {
                    break;
                }
            }
        }
        let mut count = (i as isize - q as isize) as i16 as i32;
        while count != 0 {
            let todo = if count > 126 { 126 } else { count };
            put(&mut out, todo as u16);
            put(&mut out, if ob == 1 { cc & 0xff } else { cc });
            count -= todo as i16 as i32;
        }
    }
    put(&mut out, 0);
    Ok(Ok(out))
}

/// Why [`sgiopen`] returns nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The file does not open: on standard output "Error: <system text> , can't open file
    /// <name>".
    CannotOpen,
    /// Shown on the status line: "<program>: error on read of image header" and its kin.
    Status(&'static str),
    /// The magic number is not 0x01da in either byte order: nothing is said.
    NotSgi,
}

/// The name the library opens for `name` [part of soft 0x085d87b8 sgiopen]: the name itself
/// when its last dot starts `.sgi` or `.rgb` to the end, else the name with `.sgi` added.
pub fn sgi_file_name(name: &[u8]) -> Vec<u8> {
    match name.iter().rposition(|&b| b == b'.').map(|d| &name[d..]) {
        Some(b".sgi") | Some(b".rgb") => name.to_vec(),
        _ => [name, b".sgi"].concat(),
    }
}

impl Image {
    pub(crate) fn header(&self, big_endian: bool) -> [u8; 0x98] {
        let mut h = [0u8; 0x98];
        let w16 = |v: u16| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
        let w32 = |v: u32| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
        for (k, v) in [self.imagic, self.kind, self.dim, self.xsize, self.ysize, self.zsize].into_iter().enumerate() {
            h[2 * k..2 * k + 2].copy_from_slice(&w16(v));
        }
        h[0xc..0x10].copy_from_slice(&w32(self.min));
        h[0x10..0x14].copy_from_slice(&w32(self.max));
        h[0x14..0x18].copy_from_slice(&self.wastebytes.to_le_bytes());
        h[0x18..0x68].copy_from_slice(&self.name);
        h[0x68..0x6c].copy_from_slice(&w32(self.colormap));
        h
    }

    /// Reverses the header's fields in the record [soft 0x08615328 cvtimage]: six 16-bit
    /// words, the three 32-bit words after them, and the colour map word.
    pub fn cvtimage(&mut self) {
        for v in [&mut self.imagic, &mut self.kind, &mut self.dim, &mut self.xsize, &mut self.ysize, &mut self.zsize] {
            *v = v.swap_bytes();
        }
        for v in [&mut self.min, &mut self.max, &mut self.wastebytes, &mut self.colormap] {
            *v = v.swap_bytes();
        }
    }

    /// Meant to bring the header to big-endian and back around a write
    /// [soft 0x086151b4 sgi_swapImageHeader]; with helpers that store nothing, it changes
    /// nothing.
    pub fn sgi_swap_image_header(&mut self) {
        for v in [self.imagic, self.kind, self.dim, self.xsize, self.ysize, self.zsize] {
            let _ = sgi_byte_swap2(v);
        }
        for v in [self.min, self.max, self.colormap] {
            let _ = sgi_byte_swap4(v);
        }
    }

    /// Sets the image's name [soft 0x08616704 isetname]: 80 bytes, padded with zeros, not
    /// closed by one if the name is that long.
    pub fn isetname(&mut self, name: &[u8]) {
        self.name = [0; 80];
        let n = name.len().min(80);
        self.name[..n].copy_from_slice(&name[..n]);
    }

    /// Hands a message to the error handler [soft 0x0861523c i_errhdlr]. The program
    /// installs its status line as the handler (`SI_initDefault`); without one the original
    /// prints the message on standard error and **ends the program** ([`Fault::Exit`]; the
    /// message is the last of `errors`).
    pub(crate) fn i_errhdlr(&mut self, text: &str) -> Result<(), Fault> {
        self.errors.push(text.to_string());
        if self.handler {
            Ok(())
        } else {
            Err(Fault::Exit)
        }
    }

    /// Opens an image [soft 0x085d87b8 sgiopen]: for reading from `file` (`None`: the file
    /// does not open), or for writing a new one of the given kind and size.
    ///
    /// Reading: the first 0x98 bytes are the header, big-endian; a file whose magic number
    /// reads as 0x01da only when its two bytes are exchanged is taken as byte-reversed
    /// (`dorev`) and its header turned again. For a run-length image the two row tables are
    /// read from 0x200. The fields are not looked at otherwise: a sample width other than 1
    /// or 2, or a `dim` outside 1…3, shows only when a row is read.
    ///
    /// Writing: `dim` 1 gives one row, 2 a plane, 3 planes, and the record's own `dim` is
    /// then set from the sizes (a zsize of 1 makes it 2, with a ysize of 1 as well, 1). A
    /// big-endian header goes out at once: magic, kind, sizes, `min` 10,000,000, `max` 0, the
    /// name `no name`. For a run-length image the tables start empty (offset 0, length −1)
    /// and rows will go behind them.
    #[allow(clippy::too_many_arguments)]
    pub fn sgiopen(write: bool, file: Option<&[u8]>, kind: u32, dim: u32, xsize: u32, ysize: u32, zsize: u32) -> Result<Result<Image, OpenError>, Fault> {
        let mut im = Image::default();
        if write {
            im.imagic = MAGIC;
            im.kind = kind as u16;
            im.xsize = xsize as u16;
            im.ysize = 1;
            im.zsize = 1;
            if dim > 1 {
                im.ysize = ysize as u16;
            }
            if dim > 2 {
                im.zsize = zsize as u16;
            }
            im.dim = if im.zsize == 1 {
                if im.ysize == 1 {
                    1
                } else {
                    2
                }
            } else {
                3
            };
            im.min = 0x0098_9680;
            im.max = 0;
            im.isetname(b"no name");
            im.file.write(&im.header(true));
        } else {
            let Some(data) = file else { return Ok(Err(OpenError::CannotOpen)) };
            im.file.data = data.to_vec();
            let head = im.file.read(0x98).to_vec();
            if head.len() != 0x98 {
                return Ok(Err(OpenError::Status("error on read of image header")));
            }
            let be16 = |k: usize| u16::from_be_bytes([head[k], head[k + 1]]);
            let be32 = |k: usize| u32::from_be_bytes([head[k], head[k + 1], head[k + 2], head[k + 3]]);
            (im.imagic, im.kind, im.dim, im.xsize, im.ysize, im.zsize) = (be16(0), be16(2), be16(4), be16(6), be16(8), be16(10));
            (im.min, im.max, im.colormap) = (be32(0xc), be32(0x10), be32(0x68));
            im.wastebytes = u32::from_le_bytes([head[0x14], head[0x15], head[0x16], head[0x17]]);
            im.name.copy_from_slice(&head[0x18..0x68]);
            if im.imagic.swap_bytes() == MAGIC {
                im.dorev = 1;
                im.cvtimage();
            }
            if im.imagic != MAGIC {
                return Ok(Err(OpenError::NotSgi));
            }
        }
        im.flags = if write { 2 } else { 1 };
        if im.kind >> 8 == 1 {
            let n = im.ysize as usize * im.zsize as usize;
            if n == 0 {
                return Ok(Err(OpenError::Status("error on table alloc")));
            }
            im.rleend = (n as u32) * 8 + DATA;
            if write {
                im.rowstart = vec![0; n];
                im.rowsize = vec![-1; n];
            } else {
                im.file.lseek(DATA);
                for (which, text) in [(0, "error on read of rowstart"), (1, "error on read of rowsize")] {
                    let raw = im.file.read(4 * n).to_vec();
                    if raw.len() != 4 * n {
                        return Ok(Err(OpenError::Status(text)));
                    }
                    let mut t: Vec<u32> = raw.chunks_exact(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect();
                    if im.dorev != 0 {
                        cvtlongs(&mut t, (4 * n) as i32)?;
                    }
                    if which == 0 {
                        im.rowstart = t;
                    } else {
                        im.rowsize = t.into_iter().map(|v| v as i32).collect();
                    }
                }
            }
        }
        im.tmpbuf = vec![0; ibufalloc(im.xsize)];
        im.tmp_set = 0;
        (im.x, im.y, im.z) = (0, 0, 0);
        im.offset = DATA;
        im.file.lseek(DATA);
        Ok(Ok(im))
    }

    /// Is a row outside the image [soft 0x08615404 img_badrow].
    pub fn img_badrow(&self, y: u32, z: u32) -> bool {
        !(y < self.ysize as u32 && z < self.zsize as u32)
    }

    /// Moves the descriptor only if the record does not say it is there already
    /// [soft 0x0861542c img_optseek].
    pub fn img_optseek(&mut self, to: u32) {
        if self.offset != to {
            self.offset = to;
            self.file.lseek(to);
        }
    }

    /// Goes to the start of a row [soft 0x0861545c img_seek] and notes it as the current
    /// one. A row outside the image, a `dim` outside 1…3 and a kind other than verbatim or
    /// run-length are reported and nothing moves; in the latter two the row is noted first.
    pub fn img_seek(&mut self, y: u32, z: u32) -> Result<(), Fault> {
        if self.img_badrow(y, z) {
            self.i_errhdlr("img_seek: row number out of range\n")?;
            return Ok(());
        }
        (self.x, self.y, self.z) = (0, y as i16, z as i16);
        let bpp = (self.kind & 0xff) as u32;
        let (xs, ys) = (self.xsize as u32, self.ysize as u32);
        let to = match self.kind & 0xff00 {
            0 => match self.dim {
                1 => DATA,
                2 => y.wrapping_mul(xs).wrapping_mul(bpp).wrapping_add(DATA),
                3 => z.wrapping_mul(xs).wrapping_mul(ys).wrapping_add(y.wrapping_mul(xs)).wrapping_mul(bpp).wrapping_add(DATA),
                _ => {
                    self.i_errhdlr("img_seek: weird dim\n")?;
                    return Ok(());
                }
            },
            0x100 => {
                let at = match self.dim {
                    1 => 0,
                    2 => y as usize,
                    3 => z as usize * ys as usize + y as usize,
                    _ => {
                        self.i_errhdlr("img_seek: weird dim\n")?;
                        return Ok(());
                    }
                };
                *self.rowstart.get(at).ok_or(Fault::ShortPlane)?
            }
            _ => {
                self.i_errhdlr("img_seek: weird image type\n")?;
                return Ok(());
            }
        };
        self.img_optseek(to);
        Ok(())
    }

    /// The stored length of the current row [soft 0x086155ac img_getrowsize]; 0 for a `dim`
    /// outside 1…3.
    pub fn img_getrowsize(&self) -> Result<i32, Fault> {
        let at = match self.dim {
            1 => 0,
            2 => self.y as isize,
            3 => self.y as isize + self.z as isize * self.ysize as isize,
            _ => return Ok(0),
        };
        usize::try_from(at).ok().and_then(|k| self.rowsize.get(k)).copied().ok_or(Fault::ShortPlane)
    }

    /// Notes where a run-length row of `cnt` bytes goes: at the end of the data
    /// [soft 0x08615618 img_setrowsize]. A row written before leaves its bytes behind as
    /// waste. Nothing for a row outside the image.
    pub fn img_setrowsize(&mut self, cnt: i32, y: i32, z: i32) -> Result<(), Fault> {
        if self.img_badrow(y as u32, z as u32) {
            return Ok(());
        }
        let at = match self.dim {
            1 => 0,
            2 => y as usize,
            3 => y as usize + z as usize * self.ysize as usize,
            // the original goes on with a pointer it never set
            _ => return Err(Fault::Uninitialised),
        };
        *self.rowstart.get_mut(at).ok_or(Fault::ShortPlane)? = self.rleend;
        let size = self.rowsize.get_mut(at).ok_or(Fault::ShortPlane)?;
        if *size != -1 {
            self.wastebytes = self.wastebytes.wrapping_add(*size as u32);
        }
        *size = cnt;
        self.rleend = self.rleend.wrapping_add(cnt as u32);
        Ok(())
    }

    /// Reads `count` bytes into `tmpbuf` (or, with `own`, hands them back)
    /// [soft 0x086153c0 img_read] and returns how many came; the record's offset moves on,
    /// or becomes −1 when fewer came than asked for.
    fn img_read(&mut self, count: i32, own: bool) -> Result<(i32, Vec<u8>), Fault> {
        if count < 0 {
            // the kernel takes the count as huge and fills the buffer and what lies behind it
            return Err(Fault::BadSize);
        }
        let got = self.file.read(count as usize).to_vec();
        let failed = self.file.bad;
        if !own {
            if got.len() > self.tmpbuf.len() {
                return Err(Fault::ShortPlane);
            }
            self.tmpbuf[..got.len()].copy_from_slice(&got);
            self.tmp_set = self.tmp_set.max(got.len());
        }
        if !failed && got.len() == count as usize {
            self.offset = self.offset.wrapping_add(count as u32);
        } else {
            self.offset = u32::MAX;
        }
        Ok((if failed { -1 } else { got.len() as i32 }, got))
    }

    /// Writes bytes at the descriptor [soft 0x08615364 img_write]; the header swaps around
    /// the write do nothing (see the module's note).
    fn img_write(&mut self, bytes: &[u8]) -> i32 {
        self.sgi_swap_image_header();
        if self.file.bad {
            self.sgi_swap_image_header();
            self.offset = u32::MAX;
            return -1;
        }
        self.file.write(bytes);
        self.sgi_swap_image_header();
        self.offset = self.offset.wrapping_add(bytes.len() as u32);
        bytes.len() as i32
    }

    /// Reads row `y` of plane `z` into `buf`, two bytes a sample [soft 0x08616050 getrow],
    /// and returns the row's width, or −1.
    ///
    /// - An image of fewer than three dimensions has only plane 0, one of fewer than two
    ///   only row 0, whatever is asked for.
    /// - **A row that cannot be sought is read all the same**, from where the descriptor
    ///   stands and with the length of the row sought last.
    /// - Verbatim, one byte a sample: each byte widened. Two bytes: read into `buf` as they
    ///   are, the count being `2 × width` **as a 16-bit number**, and turned only for a
    ///   byte-reversed file: on this machine a regular file's samples keep their big-endian
    ///   bytes.
    /// - Run-length: the stored length, **taken as a 16-bit number**, is read into `tmpbuf`
    ///   and expanded ([`img_rle_expand`]); two-byte units are turned for a byte-reversed
    ///   file only.
    pub fn getrow(&mut self, buf: &mut [u8], y: u32, z: u32) -> Result<i32, Fault> {
        self.row_bytes = 0;
        if self.flags & 0x81 == 0 {
            return Ok(-1);
        }
        let z = if self.dim < 3 { 0 } else { z };
        let y = if self.dim < 2 { 0 } else { y };
        self.img_seek(y, z)?;
        let bpp = self.kind & 0xff;
        match self.kind & 0xff00 {
            0 => match bpp {
                1 => {
                    let (n, _) = self.img_read(self.xsize as i32, false)?;
                    if n != self.xsize as i32 {
                        self.i_errhdlr("getrow: error on read of row\n")?;
                        return Ok(-1);
                    }
                    for k in 0..self.xsize as usize {
                        buf.get_mut(2 * k..2 * k + 2).ok_or(Fault::ShortPlane)?.copy_from_slice(&[self.tmpbuf[k], 0]);
                    }
                    self.row_bytes = 2 * self.xsize as usize;
                }
                2 => {
                    let count = (self.xsize.wrapping_mul(2)) as i16 as i32;
                    let (n, got) = self.img_read(count, true)?;
                    buf.get_mut(..got.len()).ok_or(Fault::ShortPlane)?.copy_from_slice(&got);
                    self.row_bytes = got.len();
                    if n != count {
                        self.i_errhdlr("getrow: error on read of row\n")?;
                        return Ok(-1);
                    }
                    if self.dorev != 0 {
                        cvtshorts(buf, n)?;
                    }
                }
                _ => {
                    self.i_errhdlr("getrow: weird bpp\n")?;
                    return Ok(-1);
                }
            },
            0x100 => {
                if bpp != 1 && bpp != 2 {
                    self.i_errhdlr("getrow: weird bpp\n")?;
                    return Ok(-1);
                }
                let cnt = self.img_getrowsize()? as i16;
                if cnt == -1 {
                    return Ok(-1);
                }
                let (n, _) = self.img_read(cnt as i32, false)?;
                if n != cnt as i32 {
                    self.i_errhdlr("getrow: error on read of row\n")?;
                    return Ok(-1);
                }
                if bpp == 2 && self.dorev != 0 {
                    let set = self.tmp_set;
                    cvtshorts(&mut self.tmpbuf[..set], cnt as i32)?;
                }
                let set = self.tmp_set;
                match img_rle_expand(&self.tmpbuf[..set], bpp as i32, buf, 2)? {
                    Ok(n) => self.row_bytes = n,
                    Err(text) => self.i_errhdlr(&text)?,
                }
            }
            _ => {
                self.i_errhdlr("getrow: weird image type\n")?;
                return Ok(-1);
            }
        }
        Ok(self.xsize as i32)
    }

    /// Writes row `y` of plane `z` from `buf`, two bytes a sample [soft 0x08615c5c putrow],
    /// and returns the row's width, or −1. `min` and `max` follow the samples (their low
    /// bytes only, for a verbatim one-byte image). A run-length row goes to the end of the
    /// data and its place is noted in the tables; a verbatim row goes to its fixed place.
    /// For a byte-reversed image two-byte data is turned for the write and turned back.
    pub fn putrow(&mut self, buf: &mut [u8], y: u32, z: u32) -> Result<i32, Fault> {
        if self.flags & 0x82 == 0 {
            return Ok(-1);
        }
        let z = if self.dim < 3 { 0 } else { z };
        let y = if self.dim < 2 { 0 } else { y };
        let xs = self.xsize as usize;
        let bpp = self.kind & 0xff;
        let storage = self.kind & 0xff00;
        if storage != 0 && storage != 0x100 {
            self.i_errhdlr("putrow: weird image type\n")?;
            return Ok(-1);
        }
        if bpp != 1 && bpp != 2 {
            self.i_errhdlr("putrow: weird bpp\n")?;
            return Ok(-1);
        }
        let row = buf.get(..2 * xs).ok_or(Fault::ShortPlane)?;
        for s in row.chunks_exact(2) {
            let v = if storage == 0 && bpp == 1 { s[0] as u32 } else { u16::from_le_bytes([s[0], s[1]]) as u32 };
            self.max = self.max.max(v);
            self.min = self.min.min(v);
        }
        if storage == 0 {
            if bpp == 1 {
                let bytes: Vec<u8> = row.chunks_exact(2).map(|s| s[0]).collect();
                if bytes.len() > self.tmpbuf.len() {
                    return Err(Fault::ShortPlane);
                }
                self.tmpbuf[..bytes.len()].copy_from_slice(&bytes);
                self.tmp_set = self.tmp_set.max(bytes.len());
                self.img_seek(y, z)?;
                if self.img_write(&bytes) != bytes.len() as i32 {
                    self.i_errhdlr("putrow: error on write of row\n")?;
                    return Ok(-1);
                }
            } else {
                self.img_seek(y, z)?;
                let n = (2 * xs) as i32;
                if self.dorev != 0 {
                    cvtshorts(buf, n)?;
                }
                let bytes = buf[..2 * xs].to_vec();
                let wrote = self.img_write(&bytes);
                if self.dorev != 0 {
                    cvtshorts(buf, n)?;
                }
                if wrote != n {
                    self.i_errhdlr("putrow: error on write of row\n")?;
                    return Ok(-1);
                }
            }
        } else {
            let packed = match img_rle_compact(row, 2, bpp as i32, xs as i32)? {
                Ok(p) => p,
                Err(text) => {
                    self.i_errhdlr(&text)?;
                    Vec::new()
                }
            };
            let cnt = packed.len() as i32;
            if packed.len() > self.tmpbuf.len() {
                return Err(Fault::ShortPlane);
            }
            self.tmpbuf[..packed.len()].copy_from_slice(&packed);
            self.tmp_set = self.tmp_set.max(packed.len());
            self.img_setrowsize(cnt, y as i32, z as i32)?;
            self.img_seek(y, z)?;
            if bpp == 2 && self.dorev != 0 {
                cvtshorts(&mut self.tmpbuf, cnt)?;
            }
            let bytes = self.tmpbuf[..packed.len()].to_vec();
            let wrote = self.img_write(&bytes);
            if bpp == 2 && self.dorev != 0 {
                cvtshorts(&mut self.tmpbuf, cnt)?;
            }
            if wrote != bytes.len() as i32 {
                self.i_errhdlr("putrow: error on write of row\n")?;
                return Ok(-1);
            }
        }
        Ok(self.xsize as i32)
    }

    /// Writes out what the pixel buffer holds [soft 0x0861626c iflush]: for an image being
    /// written that has such a buffer with at least one sample in it, the buffer goes out as
    /// the current row; if that fails, flag 0x20 is set and −1 returned. Neither `sgi.c` nor
    /// `sgiopen` ever makes that buffer.
    pub fn iflush(&mut self) -> Result<i32, Fault> {
        if self.flags & 2 != 0 && self.ptr >> 1 > 0 {
            if let Some(mut base) = self.base.take() {
                let n = self.putrow(&mut base, self.y as i32 as u32, self.z as i32 as u32);
                self.base = Some(base);
                if n? != self.xsize as i32 {
                    self.flags |= 0x20;
                    return Ok(-1);
                }
            }
        }
        Ok(0)
    }

    /// Closes the image [soft 0x086162c0 iclose] and returns the file's bytes.
    ///
    /// For an image being written the header goes out again at offset 0, now as the whole
    /// record **in the machine's byte order** (see the module's note), a byte-reversed one
    /// turned for the write; and for a run-length image the two row tables at 0x200, in the
    /// machine's order too.
    pub fn iclose(mut self) -> Result<Vec<u8>, Fault> {
        self.iflush()?;
        self.img_optseek(0);
        if self.flags & 2 != 0 {
            if self.dorev != 0 {
                self.cvtimage();
            }
            let mut h = self.header(false);
            // file descriptor, tmpbuf, rowstart and rowsize pointers: HEADER_UNDEFINED, zero
            h[0x70..0x72].copy_from_slice(&self.flags.to_le_bytes());
            h[0x72..0x74].copy_from_slice(&self.dorev.to_le_bytes());
            h[0x74..0x76].copy_from_slice(&self.x.to_le_bytes());
            h[0x76..0x78].copy_from_slice(&self.y.to_le_bytes());
            h[0x78..0x7a].copy_from_slice(&self.z.to_le_bytes());
            h[0x88..0x8c].copy_from_slice(&self.offset.to_le_bytes());
            h[0x8c..0x90].copy_from_slice(&self.rleend.to_le_bytes());
            self.img_write(&h);
            if self.dorev != 0 {
                self.cvtimage();
            }
            if self.kind >> 8 == 1 {
                self.img_optseek(DATA);
                let n = (self.ysize as i32 * self.zsize as i32) * 4;
                let mut start = self.rowstart.clone();
                let mut size: Vec<u32> = self.rowsize.iter().map(|&v| v as u32).collect();
                if self.dorev != 0 {
                    cvtlongs(&mut start, n)?;
                }
                for &v in &start {
                    let _ = sgi_byte_swap4(v);
                }
                let bytes: Vec<u8> = start.iter().flat_map(|v| v.to_le_bytes()).collect();
                self.img_write(&bytes);
                if self.dorev != 0 {
                    cvtlongs(&mut size, n)?;
                }
                for &v in &size {
                    let _ = sgi_byte_swap4(v);
                }
                let bytes: Vec<u8> = size.iter().flat_map(|v| v.to_le_bytes()).collect();
                self.img_write(&bytes);
            }
        }
        Ok(self.file.data)
    }
}

/// What reading an SGI file gives: the picture, and the messages that went to the error
/// handler on the way (the status line, in the running program).
pub type Read = (Picture, Vec<String>);

/// An SGI file as a picture [soft 0x085d83a8 SI_pictureReadSgi]
/// ([soft 0x085d8370 readSgiPicture] is the same call).
///
/// - Width and height are the header's, ratio 1.0, `fields` 3. There is an alpha plane
///   exactly when the header says four planes.
/// - Rows are read from the last (the top of the picture) to the first; of every row,
///   planes 0, 1 and 2 (and 3). An image of one plane so gives the same row three times: a
///   grey picture. **The low byte in memory of each two-byte sample is taken**, which for a
///   regular two-byte file is its high byte, as it happens.
/// - A row that fails to read leaves its buffer as it was: the picture gets the row read
///   before it once more. Where that buffer was never filled the original copies memory
///   nobody set: [`Fault::Uninitialised`].
/// - The port refuses images of 32,768 or more in width or height (the original computes
///   their plane sizes with signed 16-bit numbers and overruns), of more than 2²⁶ pixels,
///   and of more than a million pixels in a file too small to hold them.
pub fn picture_read_sgi(file: Option<&[u8]>) -> Result<Result<Read, OpenError>, Fault> {
    let mut im = match Image::sgiopen(false, file, 0, 0, 0, 0, 0)? {
        Ok(im) => im,
        Err(e) => return Ok(Err(e)),
    };
    let (w, h) = (im.xsize as usize, im.ysize as usize);
    // before allocating: the header's size against the file. A run-length row packs at most
    // 63 samples into a byte, so a picture of more than a million pixels needs a file of at
    // least a 64th of that
    let held = file.map_or(0, |f| f.len());
    if w >= 0x8000 || h >= 0x8000 || w * h > 1 << 26 || (w * h > 1 << 20 && w * h > 64 * held) {
        return Err(Fault::BadSize);
    }
    let planes = if im.zsize == 4 { 4 } else { 3 };
    let mut out: Vec<Vec<u8>> = (0..planes).map(|_| Vec::with_capacity(w * h)).collect();
    let mut rows = vec![vec![0u8; 2 * w]; planes];
    let mut filled = vec![0usize; planes];
    for y in (0..h).rev() {
        for z in 0..planes {
            im.getrow(&mut rows[z], y as u32, z as u32)?;
            filled[z] = filled[z].max(im.row_bytes);
        }
        if filled.iter().any(|&f| f < 2 * w) {
            return Err(Fault::Uninitialised);
        }
        for z in 0..planes {
            out[z].extend(rows[z].chunks_exact(2).map(|s| s[0]));
        }
    }
    let errors = im.errors.clone();
    let mut planes_out: [Option<Vec<u8>>; 4] = [None, None, None, None];
    if w * h != 0 {
        for (z, p) in out.into_iter().enumerate() {
            planes_out[z] = Some(p);
        }
    }
    Ok(Ok((Picture { width: w as i16, height: h as i16, ratio: 1.0, fields: 3, pad: 0, planes: planes_out }, errors)))
}

/// What [`picture_write_sgi`] leaves on disc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    /// The whole file; the call succeeds.
    File(Vec<u8>),
    /// The image could not be opened (a height of 0: there is no room for a row table, and
    /// the status line says "<program>: error on table alloc"): the file holds the first,
    /// big-endian header and nothing else, and the call fails.
    HeaderOnly(Vec<u8>),
}

/// A picture as an SGI file [soft 0x085d85cc SI_pictureWriteSgi]
/// ([soft 0x085d838c writeSgiPicture] is the same call): run-length coded, one byte a
/// sample, three planes or four with alpha, rows bottom-up.
///
/// The red, green and blue planes are read without a test.
pub fn picture_write_sgi(p: &Picture) -> Result<Written, Fault> {
    let planes = if p.planes[3].is_some() { 4 } else { 3 };
    let im = Image::sgiopen(true, None, 0x101, 3, p.width as i32 as u32, p.height as i32 as u32, planes as u32)?;
    let mut im = match im {
        Ok(im) => im,
        Err(_) => {
            let mut first = Image::default();
            (first.imagic, first.kind, first.dim, first.xsize, first.ysize, first.zsize) = (MAGIC, 0x101, 3, p.width as u16, p.height as u16, planes as u16);
            first.min = 0x0098_9680;
            first.isetname(b"no name");
            return Ok(Written::HeaderOnly(first.header(true).to_vec()));
        }
    };
    let (w, h) = (im.xsize as usize, im.ysize as usize);
    let mut at = 0usize;
    let mut row = vec![0u8; 2 * w];
    for y in (0..h).rev() {
        for z in 0..planes {
            let plane = p.planes[z].as_deref().ok_or(Fault::NullPlane)?;
            let src = plane.get(at..at + w).ok_or(Fault::ShortPlane)?;
            for (d, &s) in row.chunks_exact_mut(2).zip(src) {
                d.copy_from_slice(&[s, 0]);
            }
            im.putrow(&mut row, y as u32, z as u32)?;
        }
        at += w;
    }
    Ok(Written::File(im.iclose()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_stretches() {
        let row: Vec<u8> = [1u8, 2, 3, 3, 3, 3, 9].iter().flat_map(|&v| [v, 0]).collect();
        let packed = img_rle_compact(&row, 2, 1, 7).unwrap().unwrap();
        assert_eq!(packed, vec![0x82, 1, 2, 4, 3, 1, 9, 0]); // a single last sample is a run of one
        let mut out = vec![0u8; 14];
        assert_eq!(img_rle_expand(&packed, 1, &mut out, 2).unwrap().unwrap(), 14);
        assert_eq!(out, row);
        assert_eq!(img_rle_expand(&packed, 1, &mut out[..4], 2), Err(Fault::ShortPlane));
        assert!(img_rle_compact(&row, 3, 1, 7).unwrap().is_err());
    }

    #[test]
    fn a_written_file_is_little_endian_and_reads_back() {
        let p = Picture { width: 3, height: 2, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![1, 1, 1, 2, 3, 4]), Some(vec![5; 6]), Some(vec![0, 255, 0, 255, 0, 255]), None] };
        let Written::File(file) = picture_write_sgi(&p).unwrap() else { panic!() };
        assert_eq!(&file[..12], &[0xda, 0x01, 0x01, 0x01, 3, 0, 3, 0, 2, 0, 3, 0]);
        assert_eq!(&file[0xc..0x14], &[0, 0, 0, 0, 255, 0, 0, 0]); // least and greatest sample
        assert_eq!(&file[0x18..0x20], b"no name\0");
        // the tables are little-endian too; row 1 (the top) was written first, right behind them
        assert_eq!(u32::from_le_bytes(file[0x204..0x208].try_into().unwrap()), 0x200 + 48);
        let (back, said) = picture_read_sgi(Some(&file)).unwrap().unwrap();
        assert!(said.is_empty());
        assert_eq!((back.width, back.height, &back.planes), (3, 2, &p.planes));
        assert_eq!(sgi_file_name(b"a.b"), b"a.b.sgi".to_vec());
        assert_eq!(sgi_file_name(b"x.rgb"), b"x.rgb".to_vec());
    }

    /// A small generator of bytes: no test depends on its values, only on nothing panicking.
    fn noise(seed: &mut u32) -> u8 {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (*seed >> 24) as u8
    }

    #[test]
    fn damaged_files_are_refused_or_read_but_never_panic() {
        let p = Picture { width: 9, height: 4, ratio: 1.0, fields: 3, pad: 0, planes: [Some((0..36).collect()), Some(vec![7; 36]), Some((0..36).rev().collect()), Some(vec![255; 36])] };
        let Written::File(good) = picture_write_sgi(&p).unwrap() else { panic!() };
        let mut seed = 1;
        for cut in 0..good.len() {
            let _ = picture_read_sgi(Some(&good[..cut]));
        }
        for _ in 0..4000 {
            let mut bad = good.clone();
            for _ in 0..1 + noise(&mut seed) % 4 {
                let at = (noise(&mut seed) as usize * 256 + noise(&mut seed) as usize) % bad.len();
                bad[at] = noise(&mut seed);
            }
            let _ = picture_read_sgi(Some(&bad));
        }
        // sizes the header claims and the file does not hold
        let mut huge = good.clone();
        huge[6..10].copy_from_slice(&[0xff, 0x7f, 0xff, 0x7f]);
        assert!(picture_read_sgi(Some(&huge)).is_err() || picture_read_sgi(Some(&huge)).unwrap().is_err());
    }

    #[test]
    fn what_does_not_open() {
        assert_eq!(picture_read_sgi(None).unwrap().unwrap_err(), OpenError::CannotOpen);
        assert_eq!(picture_read_sgi(Some(&[0; 20])).unwrap().unwrap_err(), OpenError::Status("error on read of image header"));
        assert_eq!(picture_read_sgi(Some(&[0; 0x98])).unwrap().unwrap_err(), OpenError::NotSgi);
        let empty = Picture { width: 4, height: 0, ..Picture::default() };
        let Written::HeaderOnly(first) = picture_write_sgi(&empty).unwrap() else { panic!() };
        assert_eq!(&first[..4], &[0x01u8, 0xda, 0x01, 0x01][..]);
    }
}
