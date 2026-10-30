// SPDX-License-Identifier: MIT
//! Truevision Targa files (`.tga`). Original source file `targa.c`.
//!
//! The file is an 18-byte header, an optional identification field, an optional colour map
//! and the pixels. The reader here is the Linux build's, with what that build really does:
//!
//! - **Bytes are signed.** The header is kept in `char` variables, and on this compiler
//!   `char` is signed. Width, height and colour-map length are assembled as
//!   `high × 256 + low` from two signed bytes, so a low byte of 0x80 or more subtracts 256:
//!   a file 640 wide (0x0280) is read as 384 wide, 720 × 486 as 464 × 230. The same sign
//!   extension hits every run-length packet header after the first one: a run packet's
//!   count becomes a number near four thousand million and the run floods the rest of the
//!   picture. Both are reproduced, not repaired.
//! - Colour-mapped pictures (types 0, 1, 9) do not decode to their colours: the map is
//!   indexed as bytes by the raw pixel value and the three planes get the same byte, masked.
//! - Run-length black-and-white (type 11) goes through the true-colour arithmetic, not the
//!   grey copy that type 3 gets.
//! - Past the end of the file every byte reads as −1, which sets all higher bits of the
//!   pixel being assembled; the reader never notices the end.
//!
//! The writer produces 32-bit pictures only, type 10 (run-length) or type 2, bottom row first.
//!
//! Opening is the caller's business (`SI_softOpen`, not ported here). The reader asks whether
//! `name + ".tga"` is readable and opens with the extension `.tga` if so, `.TGA` if not, mode
//! `rb`, error display on ([`read_extension`]); the writer opens with `.tga`, mode `wb`.

use si_file::Stream;
use si_picture::Picture;
use si_x87::{Round, F80};

use crate::io::StreamExt;

/// Size of the fixed header.
pub const HEADER_SIZE: usize = 18;

/// What the C library prints for the program name in the main program's messages: the global
/// `SI_programName` (0x08782060) lies in zero-initialised storage and no instruction of
/// `soft` stores to it by address, so `%s` is given a null pointer.
pub const PROGRAM_NAME: &str = "(null)";

/// A picture this large is refused unless the file holds bytes for it (see
/// [`TargaError::TooLarge`]).
const UNBACKED_PIXELS: usize = 1 << 20;

/// The extension the reader opens with [soft 0x085dd2d8 SI_pictureReadTarga]: `.tga` when
/// `name + ".tga"` passes `access(…, R_OK)`, `.TGA` otherwise.
pub fn read_extension(lower_case_is_readable: bool) -> &'static str {
    if lower_case_is_readable {
        ".tga"
    } else {
        ".TGA"
    }
}

/// The extension the writer opens with (mode `wb`).
pub const WRITE_EXTENSION: &str = ".tga";

/// The 18 header bytes, with the fields as the original computes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header(pub [u8; HEADER_SIZE]);

impl Header {
    fn s(&self, i: usize) -> i32 {
        self.0[i] as i8 as i32
    }

    /// Byte 0: length of the identification field, as a signed byte.
    pub fn id_length(&self) -> i32 {
        self.s(0)
    }

    /// Byte 1: non-zero when a colour map follows.
    pub fn map_type(&self) -> u8 {
        self.0[1]
    }

    /// Byte 2: image type, as a signed byte.
    pub fn image_type(&self) -> i32 {
        self.s(2)
    }

    /// Bytes 5 and 6: colour-map length, signed high byte × 256 + signed low byte.
    /// (Bytes 3 and 4, the map's first index, are never used.)
    pub fn map_length(&self) -> i32 {
        self.s(6) * 256 + self.s(5)
    }

    /// Byte 7: bits per colour-map entry, as a signed byte.
    pub fn map_entry_bits(&self) -> i32 {
        self.s(7)
    }

    /// Bytes 12 and 13, in 16-bit arithmetic: a low byte of 0x80 or more subtracts 256.
    /// (Bytes 8 to 11, the origin, are never used.)
    pub fn width(&self) -> i16 {
        ((self.s(13) << 8) + self.s(12)) as i16
    }

    /// Bytes 14 and 15, the same way.
    pub fn height(&self) -> i16 {
        ((self.s(15) << 8) + self.s(14)) as i16
    }

    /// Byte 16: bits per pixel, as a signed byte.
    pub fn depth(&self) -> i32 {
        self.s(16)
    }

    /// Byte 17: low four bits = attribute (alpha) bits per pixel, 0x10 = columns run right
    /// to left, 0x20 = rows run top to bottom, 0x40 and 0x80 = interleaving.
    pub fn descriptor(&self) -> u8 {
        self.0[17]
    }

    /// Bytes per pixel in the file: `(depth − 1) × 0.125 + 1`, cut to an integer
    /// (float class: model; the operands are small integers, so the result is exact).
    pub fn pixel_size(&self) -> i32 {
        (F80::from_i32(self.depth() - 1) * 0.125f64 + F80::ONE).to_i32(Round::Zero)
    }
}

/// What the reader leaves behind.
#[derive(Clone, Debug, PartialEq)]
pub struct TargaFile {
    /// Width, height, the caller's ratio and `fields` = 3. `pad` is not touched by the
    /// original; it is 0 here.
    pub picture: Picture,
    pub header: Header,
    /// False for types 32 and 33: the original reports success without touching the planes
    /// of the caller's record (here they are `None`).
    pub loaded: bool,
    /// What the original prints on standard error: the header description when asked for,
    /// and "This file format is not supported!" for types 32 and 33 in any case.
    pub stderr: String,
    /// File position when the reader stops.
    pub end: usize,
}

/// Where the original runs outside what C defines; nothing of it can be reproduced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fault {
    /// Identification length of 0x80 or more: a negative size goes to the allocator and to
    /// `fread`, which then copies the rest of the file to a null pointer. (With no byte left
    /// in the file the C library may get away with it; that corner is refused too.)
    NegativeIdLength,
    /// Colour map whose length or entry size came out negative: same mechanism.
    NegativeColourMap,
    /// A colour-mapped type with pixels but no colour map in the file: the masks and the map
    /// pointer are stack slots nobody wrote.
    NoColourMap,
    /// A colour-mapped pixel indexes outside the bytes allocated for the map (always the case
    /// when the map is empty: the pointer is then null). The original reads whatever lies
    /// there.
    OutsideColourMap,
    /// A run-length picture whose width or height is not positive: the packet loop writes
    /// through a null row pointer or outside the row table, or never ends.
    RunLengthWithoutSize,
}

/// The cases in which no picture comes back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargaError {
    /// "Cannot read": the file is shorter than the 18-byte header.
    CannotRead,
    /// "Format … is unknown": an image type other than 0, 1, 2, 3, 9, 10, 11, 32, 33
    /// (the signed type byte is carried for the message).
    UnknownFormat(i32),
    /// Not a case of the original: see [`Fault`].
    Fault(Fault),
    /// Not a case of the original: the header asks for more than 2^20 pixels and more than
    /// 128 for each byte left in the file. The original allocates whatever the header says
    /// and fills what the file lacks with 0xff.
    TooLarge,
    /// Not a case of the original (writer): the picture lacks a red, green or blue plane, or
    /// a plane is shorter than `width × height`. The original reads through the pointer.
    BadPlanes,
}

impl TargaError {
    /// The text the original passes to `SI_statusDisplayMessage` (level 3), with
    /// [`PROGRAM_NAME`]; `name` is the full name the opener returned. Empty for the cases
    /// that are not the original's.
    pub fn message(&self, name: &str) -> String {
        self.message_as(PROGRAM_NAME, name)
    }

    /// The same with the program name given (the converter programs share this code and may
    /// set theirs).
    pub fn message_as(&self, program: &str, name: &str) -> String {
        match self {
            TargaError::CannotRead => format!("{program}: Cannot read {name}"),
            // the file name is not part of this one
            TargaError::UnknownFormat(t) => format!("{program}: Format {t} is unknown"),
            _ => String::new(),
        }
    }
}

/// The text shown when the writer's header cannot be written (disk full, closed pipe); the
/// writer then returns failure. Nothing in [`write`] can produce it.
pub fn cannot_write_message(program: &str, name: &str) -> String {
    format!("{program}: Cannot write {name}")
}

/// Reads a Targa file [soft 0x085dd2d8 SI_pictureReadTarga].
///
/// - `verbose`: the original's third parameter; the header is described on standard error.
/// - `opaque_alpha`: its fourth; every non-zero alpha value becomes 0xff.
/// - `ratio`: its fifth, stored in the picture as a `float`.
///
/// The import dialog and [soft 0x085dd290 readTgaPicture] pass 0, 0 and 1.0
/// ([`read_tga_picture`]).
///
/// The header is read as five pieces (1, 1, 1, 5 and 10 bytes); a short read of any gives
/// "Cannot read". There is no signature test: any file of 18 bytes whose third byte is a
/// known type is a Targa file. Width and height are taken as described on [`Header`], then
/// the identification field is read and dropped, then the type decides:
///
/// | type | read as |
/// |---|---|
/// | 0, 1 | colour-mapped, not packed |
/// | 2, 3 | true colour, not packed |
/// | 9 | colour-mapped, run-length |
/// | 10, 11 | true colour, run-length |
/// | 32, 33 | nothing; "not supported" on standard error, success returned |
/// | other | failure, "Format … is unknown" |
///
/// With `verbose` and an unknown type the header lines were already printed when the
/// original fails; the error here does not carry them.
pub fn read(data: &[u8], verbose: bool, opaque_alpha: bool, ratio: f64) -> Result<TargaFile, TargaError> {
    let mut s = Stream::new(data);
    let mut raw = [0u8; HEADER_SIZE];
    for (at, len) in [(0usize, 1usize), (1, 1), (2, 1), (3, 5), (8, 10)] {
        if !s.read_record(&mut raw[at..at + len]) {
            return Err(TargaError::CannotRead);
        }
    }
    let header = Header(raw);
    let mut picture = Picture {
        width: header.width(),
        height: header.height(),
        // fld m64, fstp m32 (float class: single)
        ratio: F80::from_f64(ratio).to_f32(),
        fields: 3,
        pad: 0,
        planes: Default::default(),
    };
    let pixel_size = header.pixel_size();
    match header.id_length() {
        0 => {}
        n if n < 0 => return Err(TargaError::Fault(Fault::NegativeIdLength)),
        n => {
            // read into a buffer that is freed at once; a short read is not noticed
            let mut id = vec![0u8; n as usize];
            s.read(&mut id);
        }
    }
    let mut stderr = String::new();
    let kind = header.image_type();
    if verbose {
        stderr += &format!("width = {}\theight = {}\n", picture.width, picture.height);
        stderr += &format!("pixel size = {pixel_size} byte(s)\n");
        stderr += if header.map_type() != 0 {
            "Space is reserved in the file for a colour map.\n"
        } else {
            "There is no colour map in the file.\n"
        };
        stderr += &format!("Image type = {kind}\n");
    }
    // (run-length, true colour, the line printed afterwards when verbose)
    let (rle, true_colour, line) = match kind {
        0 => (false, false, "No image data included.\n"),
        1 => (false, false, "Uncompressed, colour-mapped image.\n"),
        2 => (false, true, "Uncompressed, true-colour image.\n"),
        3 => (false, true, "Uncompressed, black and white image.\n"),
        9 => (true, false, "Run-length encoded colour-mapped image.\n"),
        10 => (true, true, "Run-length encoded true-colour image.\n"),
        11 => (true, true, "Run-length encoded black and white image.\n"),
        32 | 33 => {
            stderr += "This file format is not supported!\n";
            return Ok(TargaFile { picture, header, loaded: false, stderr, end: s.tell() });
        }
        other => return Err(TargaError::UnknownFormat(other)),
    };
    data_read(&mut s, &header, &mut picture, pixel_size, rle, true_colour, opaque_alpha)?;
    if verbose {
        stderr += line;
    }
    Ok(TargaFile { picture, header, loaded: true, stderr, end: s.tell() })
}

/// What the import dialog and the format table's entry do
/// [soft 0x085dd290 readTgaPicture]: not verbose, alpha as stored, ratio 1.0.
pub fn read_tga_picture(data: &[u8]) -> Result<TargaFile, TargaError> {
    read(data, false, false, 1.0)
}

/// A buffer as the original's allocator hands it out: the first byte 0x6f and the last 0x6b
/// (it marks both ends), the rest not initialised: zero here.
fn marked(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    if n > 0 {
        v[0] = 0x6f;
        v[n - 1] = 0x6b;
    }
    v
}

/// `count` one-bits from the bottom, built by doubling as the original does (so 32 or more
/// give all ones, none or fewer give 0).
fn low_bits(count: i32) -> u32 {
    let mut m = 0u32;
    for _ in 0..count.max(0) {
        m = m.wrapping_mul(2) | 1;
    }
    m
}

/// One pixel of `pixel_size` bytes, low byte first, OR-ed into `into`. Each byte is what
/// `getc` returns, shifted by 8 × its index with the processor's 5-bit shift count; at the
/// end of the file that is −1, which sets every bit from its position up.
fn or_pixel(s: &mut Stream, pixel_size: i32, into: &mut u32) {
    for k in 0..pixel_size.max(0) {
        let c = s.getc().map_or(u32::MAX, u32::from);
        *into |= c.wrapping_shl(8 * k as u32);
    }
}

/// Pixels, colour map and conversion to planes [soft 0x085dd8a8 dataReadType2].
///
/// 1. With a colour map in the file: its `length × entry bytes` bytes are read. Bits per
///    component are `entry bits / 3` cut to an integer when that is below 8, else 8; the
///    three masks are that many low bits, shifted up once and twice.
/// 2. Alpha mask: the descriptor's attribute count as low bits.
/// 3. True colour: bits per component are 8 for a 32-bit pixel, else
///    `(depth − attribute bits) / 3` cut to an integer; masks as above, except that types 3
///    and 11 use the lowest mask for all three; the alpha mask moves up by three components.
///    An alpha plane exists exactly when the alpha mask is not 0.
/// 4. Pixels become one 32-bit word each, row by row in file order: plain, or run-length
///    packets (a header below 0x80: that many + 1 pixels follow; otherwise one pixel
///    follows and is repeated header − 0x7f times, packets running across rows). Only the
///    very first packet header is used as read; later ones are sign-extended.
/// 5. Rows are permuted for the interleave bits (0x40: two passes, 0x80: four), flipped
///    top to bottom unless bit 0x20 is set, mirrored when bit 0x10 is set. With both 0x20
///    and 0x10 the interleave permutation is not applied.
/// 6. Each word becomes blue, green, red (and alpha): type 3 copies its low byte to all
///    three and sets alpha to 0xff; other true-colour types scale each masked component by
///    `255 / lowest mask` and add 0.5; colour-mapped types look the word up as a byte offset
///    into the map.
///
/// Float class: model. The scale is rounded to `float` when stored; component × scale + 0.5
/// stays in the register and is cut to an integer whose low byte is kept.
fn data_read(
    s: &mut Stream,
    header: &Header,
    pic: &mut Picture,
    pixel_size: i32,
    rle: bool,
    true_colour: bool,
    opaque_alpha: bool,
) -> Result<(), TargaError> {
    let (w, h) = (pic.width as i32, pic.height as i32);
    let image_size = w * h;
    let kind = header.image_type();

    // 1. the colour map. `None`: there is none and the original's pointer and masks are
    // unset; an empty vector: the original's pointer is null.
    let mut map: Option<Vec<u8>> = None;
    let mut masks: Option<[u32; 3]> = None;
    if header.map_type() != 0 {
        let entry = (F80::from_i32(header.map_entry_bits() - 1) * 0.125f64 + F80::ONE).to_i32(Round::Zero);
        let count = header.map_length();
        if count == 0 || entry == 0 {
            // nothing allocated, nothing read
            map = Some(Vec::new());
        } else if count < 0 || entry < 0 {
            return Err(TargaError::Fault(Fault::NegativeColourMap));
        } else {
            // at most 32,639 entries of 16 bytes: no check against the file needed
            let mut m = marked((count * entry) as usize);
            s.read(&mut m);
            map = Some(m);
        }
        let third = F80::from_i32(header.map_entry_bits()) / 3.0f64;
        let bits = if third.lt(F80::from_f64(8.0)) { third.to_i32(Round::Zero) } else { 8 };
        let m0 = low_bits(bits);
        let m1 = m0.wrapping_shl(bits as u32);
        masks = Some([m0, m1, m1.wrapping_shl(bits as u32)]);
    }

    // 2. and 3.
    let attr = (header.descriptor() & 0xf) as i32;
    let mut alpha_mask: u32 = 0x7fff >> (15 - attr);
    let mut bits = 0i32;
    if true_colour {
        if header.0[16] == 0x20 {
            bits = 8;
        } else {
            bits = (F80::from_i32(header.depth() - attr) / 3.0f64).to_i32(Round::Zero);
        }
        let m0 = low_bits(bits);
        masks = Some(if kind == 3 || kind == 11 {
            [m0, m0, m0]
        } else {
            let m1 = m0.wrapping_shl(bits as u32);
            [m0, m1, m1.wrapping_shl(bits as u32)]
        });
        alpha_mask = alpha_mask.wrapping_shl((3 * bits) as u32);
    }
    let has_alpha = alpha_mask != 0 && image_size != 0;

    // the planes the original allocates here, `width × height` bytes each
    if w <= 0 || h <= 0 {
        if rle {
            // the one size the packet loop survives: a single row of no pixels. It reads the
            // first packet header and, for a run, its pixel, then finds the row full.
            if !(w == 0 && h == 1) {
                return Err(TargaError::Fault(Fault::RunLengthWithoutSize));
            }
            if s.getc().map_or(true, |c| c > 0x7f) {
                or_pixel(s, pixel_size, &mut 0);
            }
        }
        // No row is read and no pixel converted. With both sizes negative the product is
        // positive and the planes are allocated and returned unwritten.
        if image_size > 0 {
            let n = image_size as usize;
            if n > UNBACKED_PIXELS && n / 128 > s.left() {
                return Err(TargaError::TooLarge);
            }
            let count = if has_alpha { 4 } else { 3 };
            for plane in pic.planes.iter_mut().take(count) {
                *plane = Some(marked(n));
            }
        }
        return Ok(());
    }
    if !true_colour && masks.is_none() {
        // the original gets as far as the conversion before it uses them
        return Err(TargaError::Fault(Fault::NoColourMap));
    }
    let (wu, hu) = (w as usize, h as usize);
    let n = wu * hu;
    if n > UNBACKED_PIXELS && n / 128 > s.left() {
        return Err(TargaError::TooLarge);
    }

    // 4. the words, rows in file order (the original allocates each row zeroed)
    let mut words = vec![0u32; n];
    if !rle {
        for word in words.iter_mut() {
            or_pixel(s, pixel_size, word);
        }
    } else {
        // the first header is the value `getc` returned: 0 to 255, or −1
        let mut packet = s.getc().map_or(u32::MAX, u32::from);
        let (mut x, mut y) = (0usize, 0usize);
        while y < hu {
            let run = packet > 0x7f;
            let mut pixel = 0u32;
            if run {
                or_pixel(s, pixel_size, &mut pixel);
            }
            let mut count: u32 = if run { 0x80 } else { 0 };
            let mut full = false;
            while count <= packet {
                if x == wu {
                    x = 0;
                    if y >= hu - 1 {
                        full = true;
                        break;
                    }
                    y += 1;
                }
                if run {
                    words[y * wu + x] = pixel;
                } else {
                    or_pixel(s, pixel_size, &mut words[y * wu + x]);
                }
                x += 1;
                // a sign-extended run header makes this loop as good as endless; it ends
                // because the picture fills
                count = count.wrapping_add(1);
            }
            if full {
                y += 1;
            }
            if y + 1 < hu || x < wu {
                // every later header goes through a signed byte
                packet = s.getc().map_or(u32::MAX, |c| c as i8 as i32 as u32);
            }
        }
    }

    // 5. the row table
    let descriptor = header.descriptor();
    let step = if descriptor & 0x40 != 0 {
        2
    } else if descriptor & 0x80 != 0 {
        4
    } else {
        1
    };
    let mut passes = Vec::with_capacity(hu);
    for start in 0..step {
        let mut i = start;
        while i < hu {
            passes.push(i);
            i += step;
        }
    }
    let origin = descriptor & 0x30;
    let rows: Vec<usize> = match origin {
        0x00 | 0x10 => (0..hu).map(|i| passes[hu - 1 - i]).collect(),
        0x20 => passes,
        _ => (0..hu).collect(),
    };
    if origin & 0x10 != 0 {
        for row in words.chunks_exact_mut(wu) {
            row.reverse();
        }
    }

    // 6. the planes, written in the order of the row table
    let [m0, m1, m2] = masks.unwrap_or([0; 3]);
    let mut planes: [Vec<u8>; 4] = Default::default();
    for plane in planes.iter_mut().take(if has_alpha { 4 } else { 3 }) {
        plane.reserve_exact(n);
    }
    if true_colour {
        // 255 / lowest mask, stored as a float; a mask of 0 gives infinity, and then every
        // product is a NaN or an infinity, which the conversion turns into 0x80000000
        let scale = (F80::from_f64(255.0) / F80::from_i64(m0 as i64)).to_f32();
        let convert = |v: u32| -> u8 { (F80::from_i64(v as i64) * scale + 0.5f64).to_i32(Round::Zero) as u8 };
        let table: Vec<u8> = (0..256).map(convert).collect();
        let scaled = |v: u32| -> u8 { if v < 256 { table[v as usize] } else { convert(v) } };
        let (sh1, sh2, sh3) = (bits as u32, (2 * bits) as u32, (3 * bits) as u32);
        for &row in &rows {
            for &v in &words[row * wu..(row + 1) * wu] {
                if kind == 3 {
                    for plane in planes.iter_mut().take(3) {
                        plane.push(v as u8);
                    }
                    if has_alpha {
                        planes[3].push(0xff);
                    }
                } else {
                    planes[2].push(scaled(v & m0));
                    planes[1].push(scaled((v & m1).wrapping_shr(sh1)));
                    planes[0].push(scaled((v & m2).wrapping_shr(sh2)));
                    if has_alpha {
                        let a = scaled((v & alpha_mask).wrapping_shr(sh3));
                        planes[3].push(if opaque_alpha && a != 0 { 0xff } else { a });
                    }
                }
            }
        }
    } else {
        let map = map.unwrap_or_default();
        let at = |i: u32| map.get(i as usize).copied().ok_or(TargaError::Fault(Fault::OutsideColourMap));
        for &row in &rows {
            for &v in &words[row * wu..(row + 1) * wu] {
                let c = at(v)?;
                planes[2].push(c & m0 as u8);
                planes[1].push(c & m1 as u8);
                planes[0].push(c & m2 as u8);
                if has_alpha {
                    let a = at(v & alpha_mask)?;
                    planes[3].push(if opaque_alpha && a != 0 { 0xff } else { a });
                }
            }
        }
    }
    let [r, g, b, a] = planes;
    pic.planes = [Some(r), Some(g), Some(b), has_alpha.then_some(a)];
    Ok(())
}

/// Writes a Targa file [soft 0x085de350 SI_pictureWriteTarga] (wrapped unchanged by
/// [soft 0x085dd2b8 writeTgaPicture]).
///
/// Header: no identification, no colour map, type 10 (run-length), or 2 when
/// `uncompressed` (the original's third parameter) is set; origin 0, 0; width and height
/// as the two bytes each of the record (low byte first on this build); 32 bits per pixel;
/// descriptor 8 with an alpha plane and 0 without. Pixels are always four bytes, blue,
/// green, red, alpha, with alpha 0xff when the picture has no alpha plane; rows go last
/// row first.
///
/// Run-length rows are coded one at a time by the same kind of state machine as the picture
/// files' (finding 0003): differing pixels collect in a list, a pixel equal to the list's
/// last one ends the list without that last pixel and starts a run of two, a different
/// pixel ends a run; a list or a run that reaches 128 is written at once. A raw packet
/// starts with count − 1, a run packet with count + 0x7f.
pub fn write(pic: &Picture, uncompressed: bool) -> Result<Vec<u8>, TargaError> {
    let (w, h) = (pic.width.max(0) as usize, pic.height.max(0) as usize);
    let need = w * h;
    let plane = |i: usize| pic.planes[i].as_deref().filter(|p| p.len() >= need);
    let (Some(r), Some(g), Some(b)) = (plane(0), plane(1), plane(2)) else {
        return Err(TargaError::BadPlanes);
    };
    let a = match &pic.planes[3] {
        Some(_) => Some(plane(3).ok_or(TargaError::BadPlanes)?),
        None => None,
    };
    let mut out = Vec::with_capacity(HEADER_SIZE + 4 * need);
    out.extend_from_slice(&[0, 0, if uncompressed { 2 } else { 10 }, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&pic.width.to_le_bytes());
    out.extend_from_slice(&pic.height.to_le_bytes());
    out.extend_from_slice(&[0x20, if a.is_some() { 8 } else { 0 }]);
    let pixel = |i: usize| -> [u8; 4] { [b[i], g[i], r[i], a.map_or(0xff, |a| a[i])] };
    // carried from row to row in the original; it is always set again before it is used
    let mut same = false;
    for y in (0..h).rev() {
        if uncompressed {
            for x in 0..w {
                out.extend_from_slice(&pixel(y * w + x));
            }
            continue;
        }
        let mut buf: Vec<[u8; 4]> = Vec::with_capacity(128);
        for x in 0..w {
            let px = pixel(y * w + x);
            match buf.len() {
                0 => buf.push(px),
                1 => {
                    same = px == buf[0];
                    buf.push(px);
                }
                count => {
                    if (px == buf[count - 1]) == same {
                        buf.push(px);
                        if buf.len() == 0x80 {
                            if same {
                                out.push(0xff);
                                out.extend_from_slice(&buf[0]);
                            } else {
                                out.push(0x7f);
                                buf.iter().for_each(|p| out.extend_from_slice(p));
                            }
                            buf.clear();
                        }
                    } else if !same {
                        out.push((count - 2) as u8);
                        buf[..count - 1].iter().for_each(|p| out.extend_from_slice(p));
                        buf.clear();
                        buf.extend_from_slice(&[px, px]);
                        same = true;
                    } else {
                        out.push((count + 0x7f) as u8);
                        out.extend_from_slice(&buf[0]);
                        buf.clear();
                        buf.push(px);
                    }
                }
            }
        }
        match buf.len() {
            0 => {}
            count if count == 1 || !same => {
                out.push((count - 1) as u8);
                buf.iter().for_each(|p| out.extend_from_slice(p));
            }
            count => {
                out.push((count + 0x7f) as u8);
                out.extend_from_slice(&buf[0]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng(seed: u32) -> impl FnMut() -> u32 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 16
        }
    }

    /// A picture with runs of every length class.
    fn sample(w: usize, h: usize, alpha: bool, seed: u32) -> Picture {
        let mut next = rng(seed);
        let mut planes: [Vec<u8>; 4] = Default::default();
        while planes[0].len() < w * h {
            let run = match next() % 5 {
                0 => 1,
                1 => 2,
                2 => 126 + next() as usize % 5,
                3 => 1 + next() as usize % 300,
                _ => 1,
            };
            let px = [next() as u8 % 3, next() as u8 % 2, next() as u8 % 2, next() as u8];
            for _ in 0..run.min(w * h - planes[0].len()) {
                for (p, v) in planes.iter_mut().zip(px) {
                    p.push(v);
                }
            }
        }
        let [r, g, b, a] = planes;
        Picture { width: w as i16, height: h as i16, ratio: 1.0, fields: 3, pad: 0, planes: [Some(r), Some(g), Some(b), alpha.then_some(a)] }
    }

    fn header(kind: u8, w: u16, h: u16, depth: u8, descriptor: u8) -> Vec<u8> {
        let mut v = vec![0, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(&[depth, descriptor]);
        v
    }

    /// A run-length decoder as the format is meant (no sign extension), for checking the
    /// writer on its own.
    fn decode_packets(data: &[u8], pixels: usize) -> Option<Vec<[u8; 4]>> {
        let mut out = Vec::new();
        let mut i = 0;
        while out.len() < pixels {
            let c = *data.get(i)? as usize;
            i += 1;
            if c < 0x80 {
                for _ in 0..=c {
                    out.push(data.get(i..i + 4)?.try_into().ok()?);
                    i += 4;
                }
            } else {
                let px: [u8; 4] = data.get(i..i + 4)?.try_into().ok()?;
                i += 4;
                out.extend(std::iter::repeat(px).take(c - 0x7f));
            }
        }
        (i == data.len() && out.len() == pixels).then_some(out)
    }

    #[test]
    fn uncompressed_round_trip() {
        for (seed, alpha) in [(1, true), (2, false), (3, true)] {
            let p = sample(37, 11, alpha, seed);
            let bytes = write(&p, true).unwrap();
            assert_eq!(bytes.len(), 18 + 37 * 11 * 4);
            assert_eq!(&bytes[..18], &[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 37, 0, 11, 0, 32, if alpha { 8 } else { 0 }]);
            let f = read_tga_picture(&bytes).unwrap();
            assert_eq!(f.picture, p);
            assert_eq!((f.end, f.loaded, f.stderr.as_str()), (bytes.len(), true, ""));
        }
    }

    #[test]
    fn writer_packets_decode_to_the_picture() {
        for seed in 0..60 {
            let (w, h) = (1 + (seed as usize * 53) % 400, 1 + seed as usize % 4);
            let p = sample(w, h, seed % 2 == 0, seed);
            let bytes = write(&p, false).unwrap();
            assert_eq!(bytes[2], 10);
            let px = decode_packets(&bytes[18..], w * h).expect("packets");
            for y in 0..h {
                for x in 0..w {
                    let i = (h - 1 - y) * w + x; // the file's rows run bottom-up
                    let want = [
                        p.planes[2].as_ref().unwrap()[i],
                        p.planes[1].as_ref().unwrap()[i],
                        p.planes[0].as_ref().unwrap()[i],
                        p.planes[3].as_ref().map_or(0xff, |a| a[i]),
                    ];
                    assert_eq!(px[y * w + x], want, "seed {seed} x {x} y {y}");
                }
            }
        }
    }

    #[test]
    fn writer_forms() {
        let enc = |row: &[u8]| {
            let p = Picture {
                width: row.len() as i16,
                height: 1,
                ratio: 1.0,
                fields: 3,
                pad: 0,
                planes: [Some(row.to_vec()), Some(vec![0; row.len()]), Some(vec![0; row.len()]), None],
            };
            write(&p, false).unwrap()[18..].to_vec()
        };
        assert_eq!(enc(&[7]), [0, 0, 0, 7, 0xff]);
        assert_eq!(enc(&[7, 7]), [0x81, 0, 0, 7, 0xff]);
        assert_eq!(enc(&[1, 2, 2, 3]), [0, 0, 0, 1, 0xff, 0x81, 0, 0, 2, 0xff, 0, 0, 0, 3, 0xff]);
        assert_eq!(enc(&[5; 127]), [0xfe, 0, 0, 5, 0xff]);
        assert_eq!(enc(&[5; 128]), [0xff, 0, 0, 5, 0xff]); // a full run is written at once
        assert_eq!(enc(&[5; 129]), [0xff, 0, 0, 5, 0xff, 0, 0, 0, 5, 0xff]);
        let ramp: Vec<u8> = (0..129).map(|i| i as u8).collect();
        let e = enc(&ramp);
        assert_eq!((e[0], e.len()), (0x7f, 1 + 128 * 4 + 5));
        assert_eq!(&e[1 + 128 * 4..], [0, 0, 0, 128, 0xff]);
        // each row starts afresh, and an empty picture is a header alone
        let two = Picture { width: 2, height: 2, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![9; 4]), Some(vec![9; 4]), Some(vec![9; 4]), Some(vec![1; 4])] };
        assert_eq!(&write(&two, false).unwrap()[18..], [0x81, 9, 9, 9, 1, 0x81, 9, 9, 9, 1]);
        let none = Picture { width: 0, height: 5, ratio: 1.0, fields: 3, pad: 0, planes: [Some(vec![]), Some(vec![]), Some(vec![]), None] };
        assert_eq!(write(&none, false).unwrap().len(), 18);
        let mut bad = two.clone();
        bad.planes[1] = None;
        assert_eq!(write(&bad, true), Err(TargaError::BadPlanes));
        bad = two.clone();
        bad.planes[3] = Some(vec![1; 3]);
        assert_eq!(write(&bad, true), Err(TargaError::BadPlanes));
    }

    #[test]
    fn run_length_reader_floods_after_the_first_packet() {
        // one row of 8: a run of 2, a run of 3, a raw packet of 3
        let mut d = header(10, 8, 1, 32, 8);
        d.extend_from_slice(&[0x81, 1, 2, 3, 4]);
        d.extend_from_slice(&[0x82, 5, 6, 7, 8]);
        d.extend_from_slice(&[0x02, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9]);
        let f = read_tga_picture(&d).unwrap();
        // the first run is right; the second one's header is sign-extended and fills the row
        assert_eq!(f.picture.planes[2].as_deref(), Some(&[1, 1, 5, 5, 5, 5, 5, 5][..]));
        assert_eq!(f.picture.planes[0].as_deref(), Some(&[3, 3, 7, 7, 7, 7, 7, 7][..]));
        assert_eq!(f.picture.planes[3].as_deref(), Some(&[4, 4, 8, 8, 8, 8, 8, 8][..]));
        // a picture of raw packets and at most a leading run reads back as written
        let p = sample(40, 3, true, 9);
        let mut d = header(10, 40, 3, 32, 8);
        let px = |i: usize| [p.planes[2].as_ref().unwrap()[i], p.planes[1].as_ref().unwrap()[i], p.planes[0].as_ref().unwrap()[i], p.planes[3].as_ref().unwrap()[i]];
        for y in (0..3).rev() {
            d.push(39);
            for x in 0..40 {
                d.extend_from_slice(&px(y * 40 + x));
            }
        }
        assert_eq!(read_tga_picture(&d).unwrap().picture, p);
        // packets run across row ends: 3 × 2 from one raw packet of six
        let mut d = header(10, 3, 2, 32, 0x28);
        d.push(5);
        for i in 0..6u8 {
            d.extend_from_slice(&[i, 0, 0, 0xff]);
        }
        let f = read_tga_picture(&d).unwrap();
        assert_eq!(f.picture.planes[2].as_deref(), Some(&[0, 1, 2, 3, 4, 5][..]));
        assert_eq!(f.end, d.len());
    }

    #[test]
    fn signed_header_bytes() {
        // 640 × 480 is read as 384 × 224
        let h = Header(header(2, 640, 480, 24, 0).try_into().unwrap());
        assert_eq!((h.width(), h.height()), (384, 224));
        let h = Header(header(2, 720, 486, 24, 0).try_into().unwrap());
        assert_eq!((h.width(), h.height()), (464, 230));
        let h = Header(header(2, 200, 100, 24, 0).try_into().unwrap());
        assert_eq!((h.width(), h.height()), (-56, 100));
        for (depth, size) in [(0u8, 0), (1, 1), (8, 1), (9, 2), (15, 2), (16, 2), (24, 3), (32, 4), (33, 5), (127, 16), (0x80, -15), (0xff, 0)] {
            let h = Header(header(2, 1, 1, depth, 0).try_into().unwrap());
            assert_eq!(h.pixel_size(), size, "depth {depth}");
        }
        // a negative width: nothing is read, success, no planes
        let f = read_tga_picture(&header(2, 200, 100, 24, 0)).unwrap();
        assert_eq!((f.picture.width, f.picture.height, f.picture.fields), (-56, 100, 3));
        assert_eq!(f.picture.planes, [None, None, None, None]);
        // both negative: planes of the positive product, never written
        let f = read_tga_picture(&header(2, 0xff, 0xfe, 24, 0)).unwrap();
        assert_eq!((f.picture.width, f.picture.height), (-1, -2));
        assert_eq!(f.picture.planes[0].as_deref(), Some(&[0x6f, 0x6b][..]));
        assert_eq!(f.picture.planes[3], None);
        // run-length with no size
        assert_eq!(read_tga_picture(&header(10, 200, 1, 32, 0)), Err(TargaError::Fault(Fault::RunLengthWithoutSize)));
        assert_eq!(read_tga_picture(&header(10, 0, 2, 32, 0)), Err(TargaError::Fault(Fault::RunLengthWithoutSize)));
        let f = read_tga_picture(&header(10, 0, 1, 32, 0)).unwrap();
        assert_eq!((f.picture.planes.clone(), f.end), ([None, None, None, None], 18));
    }

    #[test]
    fn types_and_messages() {
        for kind in 0..=255u8 {
            let mut d = header(kind, 1, 1, 24, 0x20);
            d.extend_from_slice(&[1, 2, 3]);
            let got = read_tga_picture(&d);
            match kind {
                2 | 3 | 10 | 11 => assert!(got.unwrap().loaded, "{kind}"),
                0 | 1 | 9 => assert_eq!(got, Err(TargaError::Fault(Fault::NoColourMap)), "{kind}"),
                32 | 33 => {
                    let f = got.unwrap();
                    assert!(!f.loaded);
                    assert_eq!(f.stderr, "This file format is not supported!\n");
                    assert_eq!((f.picture.width, f.picture.fields, f.end), (1, 3, 18));
                }
                _ => {
                    let e = got.unwrap_err();
                    assert_eq!(e, TargaError::UnknownFormat(kind as i8 as i32));
                    assert_eq!(e.message_as("soft", "x.tga"), format!("soft: Format {} is unknown", kind as i8));
                }
            }
        }
        assert_eq!(TargaError::CannotRead.message("a.tga"), "(null): Cannot read a.tga");
        assert_eq!(TargaError::TooLarge.message("a.tga"), "");
        assert_eq!(cannot_write_message("soft", "a.tga"), "soft: Cannot write a.tga");
        assert_eq!((read_extension(true), read_extension(false)), (".tga", ".TGA"));
        let mut d = header(2, 1, 1, 24, 0x20);
        d.extend_from_slice(&[1, 2, 3]);
        let f = read(&d, true, false, 1.5).unwrap();
        assert_eq!(f.picture.ratio, 1.5);
        assert_eq!(
            f.stderr,
            "width = 1\theight = 1\npixel size = 3 byte(s)\nThere is no colour map in the file.\nImage type = 2\nUncompressed, true-colour image.\n"
        );
    }

    #[test]
    fn depths_and_alpha() {
        // 24 bits: three bytes blue, green, red; no alpha plane
        let mut d = header(2, 2, 1, 24, 0x20);
        d.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
        let p = read_tga_picture(&d).unwrap().picture;
        assert_eq!(p.planes, [Some(vec![30, 60]), Some(vec![20, 50]), Some(vec![10, 40]), None]);
        // 16 bits with one attribute bit: five bits a component, scaled by float(255 / 31)
        let mut d = header(2, 3, 1, 16, 0x21);
        for v in [0x7fffu16, 0x8000, 0x0421] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        let p = read_tga_picture(&d).unwrap().picture;
        assert_eq!(p.planes[0], Some(vec![255, 0, 8]));
        assert_eq!(p.planes[1], Some(vec![255, 0, 8]));
        assert_eq!(p.planes[2], Some(vec![255, 0, 8]));
        assert_eq!(p.planes[3], Some(vec![0, 8, 0])); // one bit of alpha, scaled like a colour
        let p = read(&d, false, true, 1.0).unwrap().picture;
        assert_eq!(p.planes[3], Some(vec![0, 255, 0]));
        // 32 bits without attribute bits: no alpha plane, the fourth byte is dropped
        let mut d = header(2, 1, 1, 32, 0x20);
        d.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(read_tga_picture(&d).unwrap().picture.planes, [Some(vec![3]), Some(vec![2]), Some(vec![1]), None]);
        // type 3 copies the byte; type 11 treats it as two bits a component, all in blue
        let mut d = header(3, 2, 1, 8, 0x28);
        d.extend_from_slice(&[0x55, 0xfe]);
        let p = read_tga_picture(&d).unwrap().picture;
        assert_eq!(p.planes, [Some(vec![0x55, 0xfe]), Some(vec![0x55, 0xfe]), Some(vec![0x55, 0xfe]), Some(vec![0xff, 0xff])]);
        let mut d = header(11, 2, 1, 8, 0x20);
        d.extend_from_slice(&[1, 0x55, 0xfe]);
        let p = read_tga_picture(&d).unwrap().picture;
        assert_eq!(p.planes, [Some(vec![0, 0]), Some(vec![0, 0]), Some(vec![85, 170]), None]);
        // a depth that leaves no bits: the scale is infinite and every component is 0
        let mut d = header(2, 2, 1, 2, 0x20);
        d.extend_from_slice(&[0xff, 0x01]);
        assert_eq!(read_tga_picture(&d).unwrap().picture.planes, [Some(vec![0, 0]), Some(vec![0, 0]), Some(vec![0, 0]), None]);
    }

    #[test]
    fn origin_and_interleave() {
        let rows = |descriptor: u8| {
            let mut d = header(2, 2, 5, 24, descriptor);
            for y in 0..5u8 {
                d.extend_from_slice(&[y * 2, 0, 0, y * 2 + 1, 0, 0]);
            }
            read_tga_picture(&d).unwrap().picture.planes[2].clone().unwrap()
        };
        assert_eq!(rows(0x20), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(rows(0x00), [8, 9, 6, 7, 4, 5, 2, 3, 0, 1]);
        assert_eq!(rows(0x10), [9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(rows(0x30), [1, 0, 3, 2, 5, 4, 7, 6, 9, 8]);
        // two passes: file rows 0, 2, 4 and then 1, 3
        assert_eq!(rows(0x60), [0, 1, 4, 5, 8, 9, 2, 3, 6, 7]);
        // four passes: 0, 4, then 1, 2, 3; 0x40 wins over 0x80
        assert_eq!(rows(0xa0), [0, 1, 8, 9, 2, 3, 4, 5, 6, 7]);
        assert_eq!(rows(0xe0), rows(0x60));
        // with both origin bits the passes are not applied
        assert_eq!(rows(0x70), rows(0x30));
        assert_eq!(rows(0x40), [6, 7, 2, 3, 8, 9, 4, 5, 0, 1]);
    }

    #[test]
    fn colour_maps() {
        // a true-colour file with a colour map: the map is skipped, 4 entries of 3 bytes
        let mut d = header(2, 1, 1, 24, 0x20);
        d[1] = 1;
        d[5] = 4;
        d[7] = 24;
        d.extend_from_slice(&[0xaa; 12]);
        d.extend_from_slice(&[1, 2, 3]);
        assert_eq!(read_tga_picture(&d).unwrap().picture.planes[2], Some(vec![1]));
        // colour-mapped: the pixel is a byte offset into the map; 24-bit entries leave only
        // blue (the other masks have no low byte)
        let mut d = header(1, 3, 1, 8, 0x20);
        d[1] = 1;
        d[5] = 2;
        d[7] = 24;
        d.extend_from_slice(&[10, 11, 12, 13, 14, 15]);
        d.extend_from_slice(&[0, 4, 5]);
        let p = read_tga_picture(&d).unwrap().picture;
        assert_eq!(p.planes, [Some(vec![0, 0, 0]), Some(vec![0, 0, 0]), Some(vec![10, 14, 15]), None]);
        // 15-bit entries: five bits a component, masks 0x1f, 0x3e0 (low byte 0xe0), 0x7c00
        d[7] = 15;
        d.truncate(18);
        d.extend_from_slice(&[0xff, 0x7f, 0x25, 0x00]);
        d.extend_from_slice(&[0, 1, 2]);
        let p = read_tga_picture(&d).unwrap().picture;
        assert_eq!(p.planes, [Some(vec![0, 0, 0]), Some(vec![0xe0, 0x60, 0x20]), Some(vec![0x1f, 0x1f, 0x05]), None]);
        // an index past the map, and a file cut inside the pixels (the pixel is then −1)
        d[22] = 9;
        assert_eq!(read_tga_picture(&d), Err(TargaError::Fault(Fault::OutsideColourMap)));
        d[22] = 0;
        d.truncate(24);
        assert_eq!(read_tga_picture(&d), Err(TargaError::Fault(Fault::OutsideColourMap)));
        // a map length whose low byte is 0x80 or more is negative; so is an entry size
        let mut d = header(1, 1, 1, 8, 0x20);
        d[1] = 1;
        d[5] = 0xff;
        d[7] = 24;
        assert_eq!(read_tga_picture(&d), Err(TargaError::Fault(Fault::NegativeColourMap)));
        // a map the file does not hold at all keeps the allocator's marks: with a depth of 0
        // no pixel byte is read, every pixel is offset 0, and that byte is the start mark
        let mut d = header(1, 2, 1, 0, 0x20);
        d[1] = 1;
        d[5] = 2;
        d[7] = 24;
        assert_eq!(read_tga_picture(&d).unwrap().picture.planes[2], Some(vec![0x6f, 0x6f]));
        // an empty map is a null pointer in the original
        d[5] = 0;
        assert_eq!(read_tga_picture(&d), Err(TargaError::Fault(Fault::OutsideColourMap)));
        // identification field
        let mut d = header(2, 1, 1, 24, 0x20);
        d[0] = 3;
        d.extend_from_slice(b"abc");
        d.extend_from_slice(&[1, 2, 3]);
        assert_eq!(read_tga_picture(&d).unwrap().picture.planes[0], Some(vec![3]));
        d[0] = 0x80;
        assert_eq!(read_tga_picture(&d), Err(TargaError::Fault(Fault::NegativeIdLength)));
    }

    #[test]
    fn truncation_at_every_byte() {
        // uncompressed 32-bit with alpha: missing bytes read as 0xff, and a pixel cut in the
        // middle has every byte above the cut 0xff
        let p = sample(5, 3, true, 4);
        let bytes = write(&p, true).unwrap();
        for cut in 0..bytes.len() {
            let got = read_tga_picture(&bytes[..cut]);
            if cut < 18 {
                assert_eq!(got, Err(TargaError::CannotRead), "cut {cut}");
                continue;
            }
            let f = got.unwrap();
            assert_eq!(f.end, cut);
            let have = cut - 18;
            for i in 0..15 {
                // pixel i of the file is row 2 − i / 5 of the picture
                let at = (2 - i / 5) * 5 + i % 5;
                for (plane, byte) in [(2usize, 0usize), (1, 1), (0, 2), (3, 3)] {
                    let want = if i * 4 + byte < have { p.planes[plane].as_ref().unwrap()[at] } else { 0xff };
                    assert_eq!(f.picture.planes[plane].as_ref().unwrap()[at], want, "cut {cut} pixel {i} byte {byte}");
                }
            }
        }
        // run-length: any cut still gives a whole picture (the end of the file is a run of −1)
        let bytes = write(&p, false).unwrap();
        for cut in 18..bytes.len() {
            let f = read_tga_picture(&bytes[..cut]).unwrap();
            assert_eq!(f.picture.planes[0].as_ref().unwrap().len(), 15, "cut {cut}");
        }
        let f = read_tga_picture(&bytes[..18]).unwrap();
        assert_eq!(f.picture.planes[3], Some(vec![0xff; 15]));
        // a huge picture with no bytes behind it is refused before anything is allocated
        assert_eq!(read_tga_picture(&header(2, 0x7f7f, 0x7f7f, 32, 0)), Err(TargaError::TooLarge));
        assert_eq!(read_tga_picture(&header(10, 0x7f7f, 0x7f7f, 32, 0)), Err(TargaError::TooLarge));
        assert_eq!(read_tga_picture(&header(2, 0x8180, 0x8180, 32, 0)), Err(TargaError::TooLarge));
    }
}
