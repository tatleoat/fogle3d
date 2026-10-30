// SPDX-License-Identifier: MIT
//! Lattice files (`.lat`, magic 0x5380f638): finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `bsceneio.c` (the program's reader), `b_structio.c`
//! (the writer, and the reader of the stand-alone converter) and the element dictionary's
//! file (the functions that open the file).
//!
//! Field names: the keywords the vendor's converter `bin2ascii` prints
//! (`SI_latticeAsciiWrite`, read from its disassembly).
//!
//! A lattice file holds the lattices of one model, one after another, each a run of tokens
//! closed by its points; a single zero token ends the file. A model has two lists of
//! lattices, one acting on the node and one on the branch; a file holds one list, and its
//! name says which (`…NLat` or `…BLat`).

use crate::io::{fread_u16, End, Reader, Writer};
use si_file::{write_bin_header, write_db_header, MAGIC_LATTICE};

/// One lattice (`SI_Lattice`, 0x44 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Lattice {
    /// +0x00 `ACTIVE`
    pub active: i16,
    /// +0x02, +0x04, +0x06 `NB_KEYS`: the number of control points along each axis
    pub nb_keys: [i16; 3],
    /// +0x08, +0x0c, +0x10 `INTERPOLATION`, one code per axis (16 bits in the file)
    pub interpolation: [i32; 3],
    /// +0x14 `KEYS`: the control points, `nb_keys[0] × nb_keys[1] × nb_keys[2]` of them
    pub keys: Vec<[f32; 3]>,
    /// +0x18: one 16-bit word per control point, zeroed on reading and not stored
    pub key_flags: Vec<i16>,
    /// +0x1c `MINIMUM`
    pub minimum: [f32; 3],
    /// +0x28 `MAXIMUM`
    pub maximum: [f32; 3],
    /// +0x34 `SCALING`
    pub scaling: [f32; 3],
}

impl Lattice {
    /// The number of control points the three counts ask for, as the reader and writer
    /// compute it: a 32-bit product of the three 16-bit counts.
    pub fn key_count(&self) -> i32 {
        (self.nb_keys[2] as i32).wrapping_mul(self.nb_keys[1] as i32).wrapping_mul(self.nb_keys[0] as i32)
    }
}

pub use crate::camera::Variant;

/// What a lattice token stream gave.
#[derive(Clone, Debug, PartialEq)]
pub struct LatticeRead {
    pub lattices: Vec<Lattice>,
    /// How many point blocks were seen (the index the next lattice would get). Larger than
    /// `lattices.len()` when blocks were read without a model to put them in.
    pub count: i32,
    pub end: End,
}

/// Reads the token stream of a lattice file [soft 0x083f8d20 SI_latticeBinaryRead]
/// (`Variant::Program`) or [soft 0x083ebbb8 b2aLatticeBinaryRead] (`Variant::Converter`).
///
/// | token | field |
/// |---|---|
/// | 0 | end of the file's stream |
/// | 1 | 16-bit: `ACTIVE` |
/// | 2 | three 16-bit: `NB_KEYS` |
/// | 3 | three 16-bit: `INTERPOLATION` |
/// | 4 | `KEYS`: one vector per control point; completes a lattice |
/// | 6 | vector: `MINIMUM` |
/// | 7 | vector: `MAXIMUM` |
/// | 8 | vector: `SCALING` |
/// | other | the program's reader ignores the token and reads nothing; the converter's skips it by its size byte |
///
/// The fields are collected in local variables and copied into a new lattice when token 4
/// arrives; they are not reset afterwards, so a later lattice in the same file inherits what
/// it does not restate. If the three counts multiply to zero or less, token 4 reads nothing
/// and makes no lattice.
///
/// `ACTIVE` and `NB_KEYS` start at zero. The original does not initialise the other
/// variables (interpolation, minimum, maximum, scaling): a lattice whose stream lacks them
/// gets whatever was on the stack. Here they start at zero.
///
/// The program's reader stores each lattice in a model (`SI_modelAddILattice`, not ported:
/// it belongs to the model code) and, for the first one, tells the editor; with no model
/// (`have_model` false) it reads the points and drops them.
/// The converter's reader takes no model and returns the list.
///
/// A point count larger than the rest of the file would make the original read (and
/// allocate for) points that are not there, each a repeat of the last float read. Here the
/// lattice gets the points the file holds and the reader's `clamped` is set.
pub fn read_tokens(r: &mut Reader, variant: Variant, have_model: bool) -> LatticeRead {
    let mut cur = Lattice {
        active: 0,
        nb_keys: [0; 3],
        interpolation: [0; 3],
        keys: Vec::new(),
        key_flags: Vec::new(),
        minimum: [0.0; 3],
        maximum: [0.0; 3],
        scaling: [0.0; 3],
    };
    let mut out = LatticeRead { lattices: Vec::new(), count: 0, end: End::Zero };
    loop {
        let ok = match variant {
            Variant::Program => fread_u16(&mut r.s, &mut r.st.token_bsceneio),
            Variant::Converter => fread_u16(&mut r.s, &mut r.st.token_bstructio),
        };
        let token = match variant {
            Variant::Program => r.st.token_bsceneio,
            Variant::Converter => r.st.token_bstructio,
        };
        r.note("lattice", token, ok);
        if token == 0 {
            out.end = End::Zero;
            return out;
        }
        if !ok {
            out.end = End::Eof;
            return out;
        }
        match token {
            1 => cur.active = r.svalue(),
            2 => {
                cur.nb_keys[0] = r.svalue();
                cur.nb_keys[1] = r.svalue();
                cur.nb_keys[2] = r.svalue();
            }
            3 => {
                cur.interpolation[0] = r.svalue() as i32;
                cur.interpolation[1] = r.svalue() as i32;
                cur.interpolation[2] = r.svalue() as i32;
            }
            4 => {
                let n = cur.key_count();
                if n > 0 {
                    let keep = variant == Variant::Converter || have_model;
                    let have = (r.s.len() - r.s.tell()) / 12;
                    let take = (n as usize).min(have);
                    let mut keys = Vec::with_capacity(if keep { take } else { 0 });
                    for _ in 0..take {
                        let mut v = [0.0f32; 3];
                        r.vector(Some(&mut v));
                        if keep {
                            keys.push(v);
                        }
                    }
                    if take < n as usize {
                        r.clamped = true;
                        r.s.seek_end();
                        r.s.getc();
                    }
                    if keep {
                        let key_flags = vec![0i16; keys.len()];
                        out.lattices.push(Lattice { keys, key_flags, ..cur.clone() });
                    }
                    out.count += 1;
                }
            }
            6 => {
                r.vector(Some(&mut cur.minimum));
            }
            7 => {
                r.vector(Some(&mut cur.maximum));
            }
            8 => {
                r.vector(Some(&mut cur.scaling));
            }
            _ => {
                if variant == Variant::Converter {
                    r.skip_field(token);
                }
            }
        }
    }
}

/// Why the writer refused (`SI_saveElemErrorCode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// Code 4: no list.
    NoList,
    /// Code 7: an empty list.
    Empty,
}

/// Writes the lattices of one list [soft 0x083eb950 SI_latticeBinaryWrite]: per lattice the
/// tokens 1, 2, 3, 6, 7, 8 and, if the three counts multiply to more than zero, token 4 with
/// the points; one zero token after the last lattice. `INTERPOLATION` is written from the
/// low 16 bits of each 32-bit field. An empty list writes nothing and fails.
///
/// The original trusts the counts: it writes that many points from the lattice's array.
/// Here a lattice with fewer points than its counts say is padded with zero points.
pub fn write_tokens(w: &mut Writer, lattices: &[Lattice]) -> Result<(), WriteError> {
    if lattices.is_empty() {
        return Err(WriteError::Empty);
    }
    for l in lattices {
        w.token_bstructio(1);
        w.svalue(l.active);
        w.token_bstructio(2);
        w.svalue(l.nb_keys[0]);
        w.svalue(l.nb_keys[1]);
        w.svalue(l.nb_keys[2]);
        w.token_bstructio(3);
        w.svalue(l.interpolation[0] as i16);
        w.svalue(l.interpolation[1] as i16);
        w.svalue(l.interpolation[2] as i16);
        w.token_bstructio(6);
        w.vector(&l.minimum);
        w.token_bstructio(7);
        w.vector(&l.maximum);
        w.token_bstructio(8);
        w.vector(&l.scaling);
        let n = l.key_count();
        if n > 0 {
            w.token_bstructio(4);
            for i in 0..n as usize {
                w.vector(l.keys.get(i).unwrap_or(&[0.0; 3]));
            }
        }
    }
    w.token_bstructio(0);
    Ok(())
}

/// What reading a lattice file gives besides the lattices.
#[derive(Clone, Debug, PartialEq)]
pub struct LatticeFile {
    /// The header's version, −1.0 if the header was not accepted.
    pub version: f32,
    pub comment: Vec<u8>,
    pub read: LatticeRead,
    /// Offset of the first byte not read.
    pub pos: usize,
    pub clamped: bool,
}

/// Reads a lattice file the way the program loads one
/// [soft 0x08110200 SI_elemdictReadLattice]: the header, then the token stream into the
/// model's node or branch list.
pub fn read_file(r: &mut Reader, have_model: bool) -> LatticeFile {
    let (version, comment) = crate::io::read_header(r);
    let read = read_tokens(r, Variant::Program, have_model);
    LatticeFile { version, comment, read, pos: r.s.tell(), clamped: r.clamped }
}

/// Reads a lattice file the way the converter's entry point does
/// [soft 0x083ebf60 SI_latticeReadBinaryFile]: the header, four bytes that are read and not
/// looked at (the tag its writer puts there), then the token stream.
///
/// The files of a database are written by the other writer and have no such tag: this entry
/// point misreads them by four bytes.
pub fn read_file_converter(r: &mut Reader) -> LatticeFile {
    let (version, comment) = crate::io::read_header(r);
    let mut tag = 0u32;
    crate::io::fread_u32(&mut r.s, &mut tag);
    let read = read_tokens(r, Variant::Converter, false);
    LatticeFile { version, comment, read, pos: r.s.tell(), clamped: r.clamped }
}

/// A lattice file as the program saves it [soft 0x08110988 SI_elemdictWriteLattice]: the
/// header, then one of the model's two lists. The header is written even if the list is then
/// refused.
pub fn write_file(w: &mut Writer, lattices: &[Lattice]) -> Result<(), WriteError> {
    write_db_header(&mut w.out, w.version_output, MAGIC_LATTICE);
    write_tokens(w, lattices)
}

/// A lattice file as the converter's entry point writes it
/// [soft 0x083ec014 SI_latticeWriteBinaryFile]: the header with the caller's version, the
/// four characters `NLAT`, then the list.
pub fn write_file_converter(w: &mut Writer, version: f32, lattices: &[Lattice]) -> Result<(), WriteError> {
    write_bin_header(&mut w.out, MAGIC_LATTICE, version, None);
    w.out.extend_from_slice(b"NLAT");
    write_tokens(w, lattices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: [i16; 3], seed: f32) -> Lattice {
        let count = n.iter().map(|&x| x as usize).product::<usize>();
        Lattice {
            active: 1,
            nb_keys: n,
            interpolation: [1, 0, 2],
            keys: (0..count).map(|i| [i as f32 + seed, -(i as f32), 0.5]).collect(),
            key_flags: vec![0; count],
            minimum: [-1.0, -2.0, -3.0],
            maximum: [1.0, 2.0, 3.0],
            scaling: [1.0, 1.0, seed],
        }
    }

    #[test]
    fn round_trip_of_a_list() {
        let list = vec![sample([2, 3, 2], 0.25), sample([1, 1, 4], 7.0)];
        let mut w = Writer::new();
        write_file(&mut w, &list).unwrap();
        // per lattice 4 + 8 + 8 + 3 × 14 + 2 + 12 n; the zero token
        assert_eq!(w.out.len(), 88 + (64 + 144) + (64 + 48) + 2);
        let f = read_file(&mut Reader::new(&w.out), true);
        assert_eq!((f.read.end, f.pos, f.read.count), (End::Zero, w.out.len(), 2));
        assert_eq!(f.read.lattices, list);
        // with no model the points are read and dropped
        let f = read_file(&mut Reader::new(&w.out), false);
        assert_eq!((f.read.lattices.len(), f.read.count, f.pos), (0, 2, w.out.len()));
        // the converter's pair agrees with itself, tag included
        let mut w = Writer::new();
        write_file_converter(&mut w, 3.5, &list).unwrap();
        assert_eq!(&w.out[88..92], b"NLAT");
        let f = read_file_converter(&mut Reader::new(&w.out));
        assert_eq!((f.version, f.read.lattices), (3.5, list));
    }

    #[test]
    fn fields_carry_over_and_empty_lattices_vanish() {
        // one lattice in full, then only new counts and points: the rest is inherited
        let first = sample([1, 2, 1], 3.0);
        let mut w = Writer::new();
        write_tokens(&mut w, std::slice::from_ref(&first)).unwrap();
        w.out.truncate(w.out.len() - 2);
        w.out.extend_from_slice(&[0, 2, 0, 1, 0, 1, 0, 1, 0, 4]);
        w.out.extend_from_slice(&[0x3f, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // a lattice with a zero count: token 4 reads nothing
        w.out.extend_from_slice(&[0, 2, 0, 0, 0, 5, 0, 5, 0, 4, 0, 0]);
        let got = read_tokens(&mut Reader::new(&w.out), Variant::Program, true);
        assert_eq!((got.end, got.lattices.len()), (End::Zero, 2));
        assert_eq!(got.lattices[1].keys, vec![[1.0, 0.0, 0.0]]);
        assert_eq!((got.lattices[1].minimum, got.lattices[1].interpolation), (first.minimum, first.interpolation));
        assert_eq!(write_tokens(&mut Writer::new(), &[]), Err(WriteError::Empty));
    }

    #[test]
    fn unknown_tokens_differ_between_the_readers() {
        // token 0x0205 with two bytes of field, then ACTIVE
        let b = [2, 5, 0, 0, 0, 1, 0, 1, 0, 0];
        let mut r = Reader::new(&b);
        // the program's reader does not skip: it takes the field's zero bytes as the end
        assert_eq!(read_tokens(&mut r, Variant::Program, true).end, End::Zero);
        assert_eq!(r.s.tell(), 4);
        let mut r = Reader::new(&b);
        assert_eq!(read_tokens(&mut r, Variant::Converter, true).end, End::Zero);
        assert_eq!(r.s.tell(), 10);
    }

    #[test]
    fn a_count_beyond_the_file_is_cut() {
        let mut b = vec![0, 2, 0x7f, 0xff, 0x7f, 0xff, 0x7f, 0xff, 0, 4];
        b.extend_from_slice(&[0; 30]);
        let mut r = Reader::new(&b);
        let got = read_tokens(&mut r, Variant::Program, true);
        assert_eq!(got.lattices[0].keys.len(), 2);
        assert!(r.clamped);
        assert_eq!(got.end, End::Eof);
    }
}
