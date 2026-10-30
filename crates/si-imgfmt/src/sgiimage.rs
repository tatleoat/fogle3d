// SPDX-License-Identifier: MIT
//! The rest of `sgiimage.c`, the image library that came with IRIS GL (finding 0014, "SGI";
//! imgfmt3 round): the opening routines (`imgopen`, `iopen`, `fiopen`), the pixel-by-pixel
//! reader and writer (`getpix`, `putpix` with `ifilbuf`, `iflsbuf`), `isetcolormap` and
//! `i_seterror`. The other 22 routines of the file are in [`crate::sgi`].
//!
//! Descriptors are modelled as the data they lead to ([`Fd`]); the host's files as a
//! [`Source`]. Messages go to [`Image::errors`] (or, before an image exists, to the list the
//! open routines return).

use crate::effect::Fault;
use crate::sgi::{cvtlongs, ibufalloc, Fd, Image, DATA, MAGIC};

/// What `imgopen` is handed to work on: the name of a file to `open` or `creat`, or a
/// descriptor the caller already has (`fiopen`; also `iopen` with a null name).
#[derive(Clone, Debug)]
pub enum Source {
    /// A name. `existing` is what `open` finds (`None`: no such file); `creatable` whether
    /// `creat` succeeds (it empties a file that is there); `fd` the number either returns.
    Path { name: Vec<u8>, fd: i32, existing: Option<Vec<u8>>, creatable: bool },
    /// A descriptor: its number, and what it leads to (`None`: a number no file has, which
    /// `read` and `write` refuse, or a negative one).
    Fd { fd: i32, file: Option<Fd> },
}

/// The answer of the open routines: the image (`None`: the null pointer) and what the error
/// handler was told, in order.
#[derive(Clone, Debug, Default)]
pub struct Opened {
    pub image: Option<Image>,
    pub errors: Vec<String>,
}

/// Hands a message to the error handler of the library [soft 0x0861523c i_errhdlr] for the
/// open routines, which have no image record to keep it in.
fn say(o: &mut Opened, handler: bool, text: &str) -> Result<(), Fault> {
    o.errors.push(text.to_string());
    if handler {
        Ok(())
    } else {
        Err(Fault::Exit)
    }
}

/// Opens an image [soft 0x08616734 imgopen] on a file name or a descriptor.
///
/// - A second mode character `+` is refused ("read/write mode not supported"). A first
///   character `w` creates, any other opens for reading; the record is flagged for writing
///   unless that character is `r` (so an `x` reads a file and then writes into it).
/// - Writing: the record is made as the library's open would (`dim` 1 a row, 2 a plane, 3
///   planes; `min` 10,000,000, `max` 0, name `no name`, `wastebytes` and `dorev` 0) and its
///   **0x98 bytes go out as they stand** in the machine's order (the two swap helpers store
///   nothing); bytes 0x68 to 0x72 and 0x74 to 0x98 of them are whatever the heap held.
///   Here they are zero.
/// - Reading: the first 0x98 bytes of the file are the record, whole, taken in the machine's
///   order. If its first two bytes, high first, read 0x01da the file is big-endian: `dorev`
///   becomes 1 and the header is turned ([`cvtimage`]); else `dorev` is 0. A magic number
///   other than 0x01da after that: "bad magic in image file %x". A file that does not open
///   gives the null image with no message.
/// - A run-length image gets its two row tables of ysize × zsize words: after the data they
///   hold for writing (0 and −1 each), else read from 0x200 as they stand and turned when
///   `dorev` says so. `rleend` is set only for run-length images (for a verbatim one it
///   holds the file's own bytes 0x8c to 0x90 when reading).
/// - Then the pixel buffer fields are cleared, `tmpbuf` made, the row last sought set to
///   (0, 0, 0), the offset to 0x200 and the descriptor moved there.
///
/// An allocation that fails is not modelled, except the tables of a count above a
/// gigabyte, which the harness's allocator refuses (a [`Fault`]); a count of zero allocates.
#[allow(clippy::too_many_arguments)]
pub fn imgopen(src: Source, mode: &[u8], kind: u16, dim: u32, xsize: u16, ysize: u16, zsize: u16, handler: bool) -> Result<Opened, Fault> {
    let mut o = Opened::default();
    let m = |k: usize| mode.get(k).copied().unwrap_or(0);
    if m(1) == b'+' {
        say(&mut o, handler, "iopen: read/write mode not supported\n")?;
        return Ok(o);
    }
    let writing_mode = m(0) == b'w';
    let shown_name = match &src {
        Source::Path { name, .. } => String::from_utf8_lossy(name).into_owned(),
        Source::Fd { .. } => "(null)".to_string(),
    };
    let mut im = Image::default();
    let fd: i32;
    let mut file: Option<Fd>;
    if writing_mode {
        match src {
            Source::Path { fd: n, creatable, .. } => {
                fd = if creatable { n } else { -1 };
                file = creatable.then(Fd::default);
            }
            Source::Fd { fd: n, file: f } => {
                fd = n;
                file = f;
            }
        }
        if fd < 0 {
            say(&mut o, handler, &format!("iopen: can't open output file {shown_name}\n"))?;
            return Ok(o);
        }
        im.imagic = MAGIC;
        im.kind = kind;
        im.xsize = xsize;
        im.ysize = 1;
        im.zsize = 1;
        if dim > 1 {
            im.ysize = ysize;
        }
        if dim > 2 {
            im.zsize = zsize;
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
        im.min = 10_000_000;
        im.max = 0;
        im.isetname(b"no name");
        im.wastebytes = 0;
        im.dorev = 0;
        let head = im.header(false);
        match file.as_mut() {
            Some(f) => f.write(&head),
            None => {
                say(&mut o, handler, "iopen: error on write of image header\n")?;
                return Ok(o);
            }
        }
    } else {
        match src {
            Source::Path { fd: n, existing, .. } => {
                fd = if existing.is_some() { n } else { -1 };
                file = existing.map(|d| Fd::at(d, 0));
            }
            Source::Fd { fd: n, file: f } => {
                fd = n;
                file = f;
            }
        }
        if fd < 0 {
            return Ok(o);
        }
        let head = match file.as_mut() {
            Some(f) => f.read(0x98).to_vec(),
            None => Vec::new(),
        };
        if head.len() != 0x98 {
            say(&mut o, handler, "iopen: error on read of image header\n")?;
            return Ok(o);
        }
        let le16 = |k: usize| u16::from_le_bytes([head[k], head[k + 1]]);
        let le32 = |k: usize| u32::from_le_bytes([head[k], head[k + 1], head[k + 2], head[k + 3]]);
        (im.imagic, im.kind, im.dim, im.xsize, im.ysize, im.zsize) = (le16(0), le16(2), le16(4), le16(6), le16(8), le16(10));
        (im.min, im.max, im.wastebytes) = (le32(0xc), le32(0x10), le32(0x14));
        im.name.copy_from_slice(&head[0x18..0x68]);
        im.colormap = le32(0x68);
        im.rleend = le32(0x8c);
        if (head[0] as u16) << 8 | head[1] as u16 == MAGIC {
            im.dorev = 1;
            im.cvtimage();
        } else {
            im.dorev = 0;
        }
        if im.imagic != MAGIC {
            say(&mut o, handler, &format!("iopen: bad magic in image file {:x}\n", im.imagic))?;
            return Ok(o);
        }
    }
    im.flags = if m(0) == b'r' { 1 } else { 2 };
    let mut file = file.expect("a file here");
    if im.kind >> 8 == 1 {
        let n = im.ysize as u32 * im.zsize as u32;
        let bytes = n.wrapping_mul(4);
        if bytes > 1 << 30 {
            return Err(Fault::BadSize);
        }
        im.rleend = n.wrapping_mul(8).wrapping_add(DATA);
        if writing_mode {
            im.rowstart = vec![0; n as usize];
            im.rowsize = vec![-1; n as usize];
        } else {
            file.lseek(DATA);
            for (which, text) in [(0, "iopen: error on read of rowstart\n"), (1, "iopen: error on read of rowsize\n")] {
                let raw = file.read(bytes as usize).to_vec();
                if raw.len() != bytes as usize {
                    say(&mut o, handler, text)?;
                    return Ok(o);
                }
                let mut t: Vec<u32> = raw.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                if im.dorev != 0 {
                    cvtlongs(&mut t, bytes as i32)?;
                }
                if which == 0 {
                    im.rowstart = t;
                } else {
                    im.rowsize = t.into_iter().map(|v| v as i32).collect();
                }
            }
        }
    }
    im.cnt = 0;
    im.ptr = 0;
    im.base = None;
    im.tmpbuf = vec![0; ibufalloc(im.xsize)];
    im.tmp_set = 0;
    (im.x, im.y, im.z) = (0, 0, 0);
    im.offset = DATA;
    file.lseek(DATA);
    im.file = file;
    im.handler = handler;
    o.image = Some(im);
    Ok(o)
}

/// Opens an image on a file name [soft 0x08616b14 iopen]: `imgopen` with no descriptor.
#[allow(clippy::too_many_arguments)]
pub fn iopen(src: Source, mode: &[u8], kind: u16, dim: u32, xsize: u16, ysize: u16, zsize: u16, handler: bool) -> Result<Opened, Fault> {
    imgopen(src, mode, kind, dim, xsize, ysize, zsize, handler)
}

/// Opens an image on a descriptor [soft 0x08616b4c fiopen]: `imgopen` with no name.
#[allow(clippy::too_many_arguments)]
pub fn fiopen(src: Source, mode: &[u8], kind: u16, dim: u32, xsize: u16, ysize: u16, zsize: u16, handler: bool) -> Result<Opened, Fault> {
    imgopen(src, mode, kind, dim, xsize, ysize, zsize, handler)
}

/// Sets the error handler [soft 0x0861529c i_seterror]: `slot` is the library's one global
/// (`i_errfunc`, 0x08833f48).
pub fn i_seterror(slot: &mut u32, handler: u32) {
    *slot = handler;
}

impl Image {
    /// Sets the colour map pointer [soft 0x08616724 isetcolormap] (the word at 0x68).
    pub fn isetcolormap(&mut self, map: u32) {
        self.colormap = map;
    }

    /// The pixel buffer's sample at the pointer, as the original reads it: two bytes, low
    /// first; a pointer that is not inside the buffer is a [`Fault`].
    fn buffer_sample(&self) -> Result<u32, Fault> {
        let base = self.base.as_ref().ok_or(Fault::NullPlane)?;
        let at = usize::try_from(self.ptr).map_err(|_| Fault::ShortPlane)?;
        let b = base.get(at..at + 2).ok_or(Fault::ShortPlane)?;
        Ok(u16::from_le_bytes([b[0], b[1]]) as u32)
    }

    /// Refills the pixel buffer with the next row [soft 0x08616520 ifilbuf] and gives its
    /// first sample, or `0xffffffff`.
    ///
    /// Not for an image that is not open for reading (flag 1). The buffer is made on first
    /// use. The row read is (y, z) = the record's 0x76 and 0x78 and the count becomes its
    /// width less one, as a 16-bit number. A row of width 0 sets "end" (0x10) and clears
    /// flag 1 if the low byte of the flags has bit 7; any other failure sets "error"
    /// (0x20); both leave the count at 0. After a row, y goes up by one, then z; past the
    /// last plane z stays on it, "end" is set and −1 is the answer (with the buffer filled).
    pub fn ifilbuf(&mut self) -> Result<u32, Fault> {
        if self.flags & 1 == 0 {
            return Ok(u32::MAX);
        }
        if self.base.is_none() {
            // an allocation that fails (and the message "can't alloc image buffer") is not modelled
            self.base = Some(vec![0; ibufalloc(self.xsize)]);
        }
        let mut buf = self.base.take().expect("made above");
        let got = self.getrow(&mut buf, self.y as i32 as u32, self.z as i32 as u32);
        self.base = Some(buf);
        let got = got?;
        self.ptr = 0;
        self.cnt = got.wrapping_sub(1) as i16;
        if self.cnt < 0 {
            if self.cnt == -1 {
                self.flags |= 0x10;
                if self.flags & 0x80 != 0 {
                    self.flags &= 0xfffe;
                }
            } else {
                self.flags |= 0x20;
            }
            self.cnt = 0;
            return Ok(u32::MAX);
        }
        self.y = self.y.wrapping_add(1);
        if self.y as i32 >= self.ysize as i32 {
            self.y = 0;
            self.z = self.z.wrapping_add(1);
            if self.z as i32 >= self.zsize as i32 {
                self.z = (self.zsize as i32 - 1) as i16;
                self.flags |= 0x10;
                return Ok(u32::MAX);
            }
        }
        let v = self.buffer_sample()?;
        self.ptr += 2;
        Ok(v)
    }

    /// Flushes the pixel buffer and stores one sample [soft 0x08616624 iflsbuf]; the sample
    /// is the answer, or `0xffffffff`.
    ///
    /// Only for an image open for writing (flag 2). The buffer is made on first use; else if
    /// it holds a sample or more, it goes out as the row (y, z), y goes up by one and z
    /// after it, and past the last plane "end" (0x10) is set and the answer is −1. Then the
    /// count becomes the width less one, the sample goes to the start of the buffer and the
    /// pointer behind it. **The row written is the whole width**, the buffer past the
    /// samples stored being whatever it held (zero here); if the row written is not as long
    /// as the samples counted, "error" (0x20) is set and the answer is −1.
    pub fn iflsbuf(&mut self, c: u32) -> Result<u32, Fault> {
        if self.flags & 2 == 0 {
            return Ok(u32::MAX);
        }
        let (held, wrote);
        match self.base.take() {
            None => {
                // an allocation that fails (and "flsbuf: error on buf alloc") is not modelled
                self.base = Some(vec![0; ibufalloc(self.xsize)]);
                held = 0;
                wrote = 0;
            }
            Some(mut buf) => {
                held = self.ptr >> 1;
                let mut n = held;
                if held > 0 {
                    let r = self.putrow(&mut buf, self.y as i32 as u32, self.z as i32 as u32);
                    self.base = Some(buf);
                    n = r?;
                    self.y = self.y.wrapping_add(1);
                    if self.y as i32 >= self.ysize as i32 {
                        self.y = 0;
                        self.z = self.z.wrapping_add(1);
                        if self.z as i32 >= self.zsize as i32 {
                            self.z = (self.zsize as i32 - 1) as i16;
                            self.flags |= 0x10;
                            return Ok(u32::MAX);
                        }
                    }
                } else {
                    self.base = Some(buf);
                }
                wrote = n;
            }
        }
        self.cnt = (self.xsize as i32 - 1) as i16;
        let base = self.base.as_mut().ok_or(Fault::NullPlane)?;
        let first = base.get_mut(..2).ok_or(Fault::ShortPlane)?;
        first.copy_from_slice(&(c as u16).to_le_bytes());
        self.ptr = 2;
        if held == wrote {
            Ok(c)
        } else {
            self.flags |= 0x20;
            Ok(u32::MAX)
        }
    }

    /// The next sample of the pixel buffer [soft 0x08616bb0 getpix]: the count goes down by
    /// one first; below zero the buffer is refilled ([`Image::ifilbuf`], which leaves the
    /// count it sets); else the sample at the pointer, which moves on.
    pub fn getpix(&mut self) -> Result<u32, Fault> {
        self.cnt = self.cnt.wrapping_sub(1);
        if self.cnt < 0 {
            return self.ifilbuf();
        }
        let v = self.buffer_sample()?;
        self.ptr += 2;
        Ok(v)
    }

    /// Stores a sample in the pixel buffer [soft 0x08616be4 putpix]: the count goes down by
    /// one first; below zero [`Image::iflsbuf`] decides; else the low 16 bits of `c` go to
    /// the pointer, which moves on, and the answer is those 16 bits.
    pub fn putpix(&mut self, c: u32) -> Result<u32, Fault> {
        self.cnt = self.cnt.wrapping_sub(1);
        if self.cnt < 0 {
            return self.iflsbuf(c);
        }
        let base = self.base.as_mut().ok_or(Fault::NullPlane)?;
        let at = usize::try_from(self.ptr).map_err(|_| Fault::ShortPlane)?;
        base.get_mut(at..at + 2).ok_or(Fault::ShortPlane)?.copy_from_slice(&(c as u16).to_le_bytes());
        self.ptr += 2;
        Ok(c & 0xffff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_image() -> Image {
        let src = Source::Path { name: b"/t/o.sgi".to_vec(), fd: 9, existing: None, creatable: true };
        imgopen(src, b"w", 0x0102, 2, 4, 3, 1, true).unwrap().image.unwrap()
    }

    #[test]
    fn writing_gives_header_then_data_offset() {
        let im = write_image();
        assert_eq!(im.offset, 0x200);
        assert_eq!(im.file.pos, 0x200);
        assert_eq!(&im.file.data[..2], &0x01dau16.to_le_bytes());
    }

    #[test]
    fn plus_mode_is_refused() {
        let src = Source::Path { name: b"x".to_vec(), fd: 9, existing: None, creatable: true };
        let o = imgopen(src, b"r+", 0x0102, 2, 4, 3, 1, true).unwrap();
        assert!(o.image.is_none());
        assert_eq!(o.errors, vec!["iopen: read/write mode not supported\n".to_string()]);
    }

    #[test]
    fn pixel_count_goes_down_and_putpix_stores() {
        let mut im = write_image();
        assert_eq!(im.putpix(0x1234).unwrap(), 0x1234);
        assert_eq!(im.cnt, 3);
        assert_eq!(im.putpix(0x00ff_0007).unwrap(), 7);
    }

    #[test]
    fn write_to_nowhere_says_so() {
        let mut im = write_image();
        im.file = Fd::invalid();
        im.putpix(1).unwrap();
        for _ in 0..3 {
            im.putpix(2).unwrap();
        }
        assert_eq!(im.putpix(3).unwrap(), u32::MAX);
        assert_eq!(im.errors.last().map(String::as_str), Some("putrow: error on write of row\n"));
    }

    #[test]
    fn colormap_and_handler_words() {
        let mut im = write_image();
        im.isetcolormap(0xdead);
        assert_eq!(im.colormap, 0xdead);
        let mut slot = 0;
        i_seterror(&mut slot, 5);
        assert_eq!(slot, 5);
    }
}
