// SPDX-License-Identifier: MIT
//! `linInOut.c`: the header of a line file (`.lin`, what the hidden-line renderer writes;
//! finding 0014, "Line drawings to PICT", sixth pass).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `linInOut.c` (0x085c48c0–0x085c4e16), outside the
//! picture library; ported here because the PICT writer of line drawings
//! ([`crate::pict::lin_write_pict`]) stands on it. Not ported: `windowStoreInPicture`
//! (0x085c4a40, a picture read back from the screen through the GL layer) and
//! `si_linReadData` (0x085c4d0c, reads the line records into a parameter block for the line
//! programs' display).
//!
//! The file (big-endian, as written on IRIX): the 88-byte header of every database file (magic
//! 0x5380f634); a line chunk of 8 bytes (`LINE`, a byte for the data's width in bits (32), a
//! byte for its type (0x20: float), and a 16-bit word); the bounds, four floats: least x,
//! greatest x, least y, greatest y; then the records: a count byte `n` and, for `n` above 0,
//! `n + 1` points of two floats (x, y); a count of 0 ends the file.

use si_file::Stream;

/// The chunk of 8 bytes behind the header, as `si_fix_LineChunk` (0x083f98f0) leaves it:
/// the magic and the word at 6 turned to this machine's order, the two bytes at 4 as read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineChunk {
    pub magic: u32,
    pub bits: u8,
    pub kind: u8,
    pub word: u16,
}

/// The magic of the line chunk: `LINE`.
pub const LINE_MAGIC: u32 = 0x4c49_4e45;

/// What [`read_header`] answers: the stream after the header (112 bytes in), and the
/// fields read.
#[derive(Clone, Debug, PartialEq)]
pub struct LineHeader {
    pub file: si_file::BinHeader,
    pub chunk: LineChunk,
    /// Least x, greatest x, least y, greatest y.
    pub bounds: [f32; 4],
}

/// Why [`read_header`] refuses a file; [`LinError::message`] gives the text the original
/// shows on the status line (level 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinError {
    /// Fewer than 88 bytes, fewer than 8 behind them, or fewer than 16 for the bounds.
    BadHeader,
    WrongMagic,
    NotLine,
    Not32Bit,
    NotFloat,
}

impl LinError {
    /// The message, with `name` the full name `SI_softOpen` found (the three "bad header"
    /// cases name it). The original formats it into 256 bytes of stack.
    pub fn message(&self, name: &[u8]) -> Vec<u8> {
        match self {
            LinError::BadHeader => {
                let mut v = b"File ".to_vec();
                v.extend_from_slice(name);
                v.extend_from_slice(b" has a bad header!\n");
                v
            }
            LinError::WrongMagic => b"The file magic number is wrong!\n".to_vec(),
            LinError::NotLine => b"This is not a LINE file!\n".to_vec(),
            LinError::Not32Bit => b"Only 32 bit wide data is supported for now.\n".to_vec(),
            LinError::NotFloat => b"Only the float data type is supported at the moment.\n".to_vec(),
        }
    }
}

/// [soft 0x083fabe8 SI_LineChunk_fread] for one chunk, with [soft 0x083f98f0 si_fix_LineChunk].
fn line_chunk_fread(s: &mut Stream) -> Option<LineChunk> {
    let mut b = [0u8; 8];
    if !s.read_record(&mut b) {
        return None;
    }
    Some(LineChunk { magic: u32::from_be_bytes([b[0], b[1], b[2], b[3]]), bits: b[4], kind: b[5], word: u16::from_be_bytes([b[6], b[7]]) })
}

/// [soft 0x083fa5b4 SI_FbndChunk_fread] for one chunk: four big-endian floats (each passed to
/// `validate_float` (0x083f9320), which does nothing).
fn fbnd_chunk_fread(s: &mut Stream) -> Option<[f32; 4]> {
    let mut b = [0u8; 16];
    if !s.read_record(&mut b) {
        return None;
    }
    Some([0, 4, 8, 12].map(|i| f32::from_bits(u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]))))
}

/// [soft 0x083fa92c SI_Fcoor2D_fread]: up to `n` points of two big-endian floats into `buf`
/// (two entries a point). `fread` copies whole points only as counted, but the bytes of a
/// last partial point land in the buffer too, unturned; the floats of such a point are those
/// bytes read in this machine's order. Answers the number of whole points.
pub fn fcoor2d_fread(s: &mut Stream, buf: &mut [Option<f32>], n: usize) -> usize {
    let mut whole = 0;
    for p in 0..n {
        let mut b = [0u8; 8];
        let got = s.read(&mut b);
        if got == 8 {
            buf[2 * p] = Some(f32::from_bits(u32::from_be_bytes([b[0], b[1], b[2], b[3]])));
            buf[2 * p + 1] = Some(f32::from_bits(u32::from_be_bytes([b[4], b[5], b[6], b[7]])));
            whole += 1;
            continue;
        }
        // a partial point: its bytes over what the float slots held
        for k in 0..2 {
            let have = got.saturating_sub(4 * k).min(4);
            if have == 4 {
                buf[2 * p + k] = Some(f32::from_bits(u32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]])));
            } else if have > 0 {
                // part of a float: the rest is what the slot held
                buf[2 * p + k] = buf[2 * p + k].map(|old| {
                    let mut o = old.to_bits().to_le_bytes();
                    o[..have].copy_from_slice(&b[4 * k..4 * k + have]);
                    f32::from_bits(u32::from_le_bytes(o))
                });
            }
        }
        break;
    }
    whole
}

/// [soft 0x085c48c0 SI_linReadHeader]: the header of a line file, the stream left behind it.
/// The original opens the file with `SI_softOpen` (extension `.lin`, mode `rb`); here the
/// stream is given. Every refusal closes the file and shows its message (level 3).
pub fn read_header(s: &mut Stream) -> Result<LineHeader, LinError> {
    let file = si_file::read_chunk(s).ok_or(LinError::BadHeader)?;
    if file.magic != si_file::MAGIC_GENERAL {
        return Err(LinError::WrongMagic);
    }
    let chunk = line_chunk_fread(s).ok_or(LinError::BadHeader)?;
    if chunk.magic != LINE_MAGIC {
        return Err(LinError::NotLine);
    }
    if chunk.bits != 0x20 {
        return Err(LinError::Not32Bit);
    }
    if chunk.kind != 0x20 {
        return Err(LinError::NotFloat);
    }
    let bounds = fbnd_chunk_fread(s).ok_or(LinError::BadHeader)?;
    Ok(LineHeader { file, chunk, bounds })
}
