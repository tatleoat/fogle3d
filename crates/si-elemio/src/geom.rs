// SPDX-License-Identifier: MIT
//! The geometry records of the shape and cluster files: spline, face, patch, polygon mesh,
//! NURBS curve and NURBS surface. Finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `binary.c` (the readers), `binary_b.c` (the
//! writers), `byteswap.c` (vectors and doubles) and the allocators of the geometry packages,
//! which give a record the values a file does not state.
//!
//! Field names: the SDK's page on the hierarchy file
//! (`sdk/html/html/fileFormats/hrc7.htm`, "<Hierarchy> Block"), which lists for each of these
//! records its fields by number with a description. A field the page does not have is named
//! by its offset in the original record (`w_52`), and its doc comment says what the code
//! shows.
//!
//! A geometry record has no tokens of its own. Its fields come in the order its *prototype*
//! gives (see [`crate::proto`]); if the file has no prototype for a record, every field is
//! preceded by its 16-bit number and a zero number ends the record. The writers never write
//! that second form, and they do not consult a prototype either: each writes one fixed
//! sequence of fields for the output version, the sequence `SI_prototypeBinaryWrite` announces
//! (with the exceptions noted at [`write_face`], [`write_nurbs_curve`],
//! [`write_nurbs_surface`] and [`write_mesh`]).
//!
//! The readers of the original go on to prepare what they read for the program: they rebuild
//! a mesh's edges and normals, compute spline and patch matrices, convert old Bézier point
//! lists and copy splines and curves into the face that owns them. That is geometry code of
//! other packages and is not reproduced: a record here is what the file said, laid over the
//! values the allocator gave. Each reader lists what the original does next.

use crate::io::{fread_f32, fread_i16, fread_i32, fread_u16, fread_u32, End, Reader, Writer};
use crate::proto::Prototypes;
use si_file::Stream;

// --- what the readers share ------------------------------------------------------------------

/// What the geometry readers and writers use that lies outside the record they handle.
#[derive(Clone, Debug, PartialEq)]
pub struct GeomState {
    /// `count` of `SI_splineAllocate` (0x086d6960), a static short of the program: every
    /// spline allocated anywhere gets the next number in its default name. (It belongs with
    /// the program's other statics; it is kept here because this module is its only user so
    /// far.)
    pub spline_count: i16,
    /// The number of UV domains of the model that owns the geometry (`__SI_Model` +0x424; 1
    /// in a new model, `SI_modelAllocate`). A mesh stores it (field 0x413) and reads and
    /// writes that many pairs of texture coordinates, less one, for every polygon node.
    pub uv_domains: i32,
}

impl Default for GeomState {
    fn default() -> Self {
        GeomState { spline_count: 0, uv_domains: 1 }
    }
}

/// A field loop that the original never leaves: it takes its field numbers from the file,
/// the file has ended, and the number left in its variable is not zero.
struct Eof;

/// Where a record's field numbers come from: its prototype list, or the file.
struct Fields<'a> {
    list: Option<&'a [i16]>,
    next: usize,
}

impl<'a> Fields<'a> {
    fn new(list: Option<&'a [i16]>) -> Self {
        Fields { list, next: 0 }
    }

    /// The next field number into the reader's variable. With a list this is the next entry
    /// (the original indexes the list without a bound: here a list with no closing zero ends
    /// at its end); without one, two bytes of the file, and false if they were not there.
    fn fetch(&mut self, r: &mut Reader, var: &mut i16) -> bool {
        match self.list {
            Some(l) => {
                *var = l.get(self.next).copied().unwrap_or(0);
                self.next += 1;
                true
            }
            None => fread_i16(&mut r.s, var),
        }
    }

    /// One turn of a field loop: fetches the next number and says whether there is a field
    /// to read. A zero ends the record. A number that could not be read leaves the variable
    /// as it was, and the original goes round again with it for ever; that is [`Eof`] here.
    fn step(&mut self, r: &mut Reader, var: &mut i16, who: &'static str) -> Result<bool, Eof> {
        let ok = self.fetch(r, var);
        r.note(who, *var as u16, ok);
        if *var == 0 {
            Ok(false)
        } else if ok {
            Ok(true)
        } else {
            Err(Eof)
        }
    }
}

/// The least number of bytes a field of an unknown number takes when it is skipped
/// ([`Reader::skip_field`]): its size byte, or the four bytes of a 32-bit size.
fn skip_min(field: i16) -> usize {
    match (field as u16) >> 8 {
        0xff => 4,
        n => n as usize,
    }
}

/// The least number of bytes one record of a list takes in the file: with no list, the two
/// bytes of the closing zero; with one, the sum over its fields of `known` (or of the size a
/// skipped field has).
fn min_size(list: Option<&[i16]>, known: fn(i16) -> Option<usize>) -> usize {
    match list {
        None => 2,
        Some(l) => crate::proto::fields(l).map(|f| known(f).unwrap_or_else(|| skip_min(f))).sum(),
    }
}

/// Checks a count read from the file against the bytes that are left: how many records of
/// at least `min` bytes each can be there. The original allocates what the count says and
/// reads on past the end of the file; here a count the file cannot hold is cut to what it
/// can and the reader's `clamped` is set. Returns the number to read and whether it was cut.
///
/// Records that take no bytes at all (a prototype with no field that is stored) are the one
/// case the bytes left cannot bound: the original makes as many empty records as the count
/// says. Here that gives none, with `clamped` set, so that what a file makes the reader
/// allocate stays in proportion to the file's size.
fn room(r: &mut Reader, count: i64, min: usize) -> (usize, bool) {
    let want = count.max(0) as u64;
    if min == 0 {
        r.clamped |= want > 0;
        return (0, false);
    }
    let left = (r.s.len() - r.s.tell()) as u64;
    let have = left / min as u64;
    if want > have {
        r.clamped = true;
    }
    (want.min(have) as usize, want > have)
}

/// After a count was cut: the original's loop would have run through the rest of the file
/// and tried to read beyond it.
fn past_end(r: &mut Reader) {
    r.s.seek_end();
    r.s.getc();
}

/// Twelve bytes into a vector [soft 0x083f9d54 SI_Vector_fread]: one `fread` of the whole
/// item straight into the destination, and the three floats are swapped only if all twelve
/// bytes arrived. A short read leaves the destination's first bytes overwritten, unswapped
/// (its memory image is little-endian on this build).
pub fn fread_vector(s: &mut Stream, v: &mut [f32; 3]) -> bool {
    let mut b = [0u8; 12];
    for (i, c) in v.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&c.to_bits().to_le_bytes());
    }
    let ok = s.read_record(&mut b);
    if ok {
        for c in b.chunks_exact_mut(4) {
            c.reverse();
        }
    }
    for (i, c) in v.iter_mut().enumerate() {
        *c = f32::from_bits(u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]));
    }
    ok
}

/// Doubles, as many as the slice holds, in one `fread` [soft 0x083f9ad0 SI_double_fread]:
/// the items that arrived whole are swapped [soft 0x083f9388 SI_swap8byte]; the bytes of a
/// last, partial item stay in the destination unswapped. Returns the number of whole items.
/// ([soft 0x083f9328 validate_double] is an empty function in this build.)
pub fn fread_doubles(s: &mut Stream, v: &mut [f64]) -> usize {
    let mut b = vec![0u8; 8 * v.len()];
    for (i, c) in v.iter().enumerate() {
        b[8 * i..8 * i + 8].copy_from_slice(&c.to_bits().to_le_bytes());
    }
    let whole = s.read(&mut b) / 8;
    for c in b.chunks_exact_mut(8).take(whole) {
        c.reverse();
    }
    for (i, c) in v.iter_mut().enumerate() {
        let mut x = [0u8; 8];
        x.copy_from_slice(&b[8 * i..8 * i + 8]);
        *c = f64::from_bits(u64::from_le_bytes(x));
    }
    whole
}

/// Three floats [soft 0x083f9df8 SI_Vector_fwrite]: swapped in place, written, swapped back.
/// (Not through `si_fvalue`, unlike [`Writer::vector`].)
pub fn fwrite_vector(w: &mut Writer, v: &[f32; 3]) {
    for &c in v {
        w.put_f32(c);
    }
}

fn put_f64(w: &mut Writer, v: f64) {
    w.out.extend_from_slice(&v.to_bits().to_be_bytes());
}

/// `fldl; fstpl`: a double copied through an x87 register, as the NURBS writers copy each
/// control point to their stack before writing it. The copy is exact except for a signalling
/// NaN, which the load turns into a quiet one.
fn x87_copy(v: f64) -> f64 {
    if v.is_nan() {
        f64::from_bits(v.to_bits() | 0x0008_0000_0000_0000)
    } else {
        v
    }
}

/// `flds a; fmull b; fstpl`: the product of a single and a double, rounded first to the
/// x87's 64-bit significand and then to a double by the store (both to nearest, ties to
/// even). One rounding to a double, as `f64` arithmetic does, differs when the first lands
/// on a tie of the second.
fn x87_mul_to_double(a: f32, b: f64) -> f64 {
    let a = a as f64;
    let plain = a * b;
    // the two-step path below is for normal numbers only
    if !plain.is_finite() || a == 0.0 || b == 0.0 || plain.abs() < 1e-290 || a.abs() < 1e-290 || b.abs() < 1e-290 {
        return plain;
    }
    let split = |x: f64| ((x.to_bits() & 0x000f_ffff_ffff_ffff) | 1 << 52, ((x.to_bits() >> 52) & 0x7ff) as i32 - 1075);
    let (ma, ea) = split(a);
    let (mb, eb) = split(b);
    let round = |m: u128, e: i32, bits: u32| -> (u128, i32) {
        let n = 128 - m.leading_zeros();
        if n <= bits {
            return (m, e);
        }
        let sh = n - bits;
        let (q, rest, half) = (m >> sh, m & ((1u128 << sh) - 1), 1u128 << (sh - 1));
        let q = if rest > half || (rest == half && q & 1 == 1) { q + 1 } else { q };
        (q, e + sh as i32)
    };
    let (q, e) = round(ma as u128 * mb as u128, ea + eb, 64);
    let (q, e) = round(q, e, 53);
    let v = q as f64 * 2f64.powi(e);
    if plain < 0.0 {
        -v
    } else {
        v
    }
}

// --- spline ------------------------------------------------------------------------------------

/// One key of a spline (0x30 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplineKey {
    /// +0x00, key field 1: "Previous tangent"
    pub previous_tangent: [f32; 3],
    /// +0x0c, key field 2: "Position of key"
    pub position: [f32; 3],
    /// +0x18, key field 3: "Next tangent"
    pub next_tangent: [f32; 3],
    /// +0x24: 1.0 in a new key; no file field reaches it
    pub f_24: f32,
    /// +0x28, key field 4: "Type of constraint"
    pub constraint: i16,
    /// +0x2a, key field 5: "Type of segment"
    pub segment: i16,
    /// +0x2c, key field 6: "Flag"
    pub flag: i16,
}

impl Default for SplineKey {
    /// A new key [soft 0x0848f8dc SI_splineKeyAllocate]: everything zero but the float at
    /// +0x24. (The allocator rounds the number of keys up to a multiple of 16 and
    /// initialises as many as were asked for.)
    fn default() -> Self {
        SplineKey { previous_tangent: [0.0; 3], position: [0.0; 3], next_tangent: [0.0; 3], f_24: 1.0, constraint: 0, segment: 0, flag: 0 }
    }
}

/// A spline (`SI_Spline`, 0x74 bytes), as far as a file reaches into it.
///
/// The rest of the record is set by `SI_splineInit` (0x0848a9f0): 0x10 at +0x04, zero at
/// +0x18, the identity matrix at +0x24 (replaced by `SI_splineUpdateMatrix` after a read), 2
/// at +0x64, 1 at +0x66 and zero at +0x68, +0x6c and +0x70.
#[derive(Clone, Debug, PartialEq)]
pub struct Spline {
    /// +0x00, field 8: "Name". A spline is allocated with the name `spl<n>`, which the
    /// name in the file replaces. (The splines the original keeps inside a face have no
    /// name: see [`read_face`].)
    pub name: Vec<u8>,
    /// +0x06, field 6: "Phantom key"
    pub phantom: i16,
    /// +0x08, field 3: "Close"
    pub close: i16,
    /// +0x0c, field 1: "Type": 0 linear, 1 Bézier, 2 B-spline, 3 cardinal, 4 NURBS (16 bits
    /// in the file)
    pub kind: i32,
    /// +0x10, fields 7 and 2: the number of keys as stored. The original allocates by it
    /// as an unsigned number, and counts keys with signed shorts (see the reader and the
    /// writer for what that does to a count of 0x8000 or more).
    pub nb_keys: u16,
    /// +0x14: the keys
    pub keys: Vec<SplineKey>,
    /// +0x1c, field 4: "Tension"
    pub tension: f32,
    /// +0x20, field 5: "Drawing precision"
    pub step: i16,
    /// Not in the record: the keys came from field 2, the old list of positions only. The
    /// original then rebuilds a Bézier spline (type 1) with `SI_bezierShrink`.
    pub old_points: bool,
}

impl Spline {
    /// A spline as `SI_splineInit` (0x0848a9f0) leaves it: no name, no keys, drawing
    /// precision 10. This is the spline inside a new face.
    pub fn init() -> Spline {
        Spline { name: Vec::new(), phantom: 0, close: 0, kind: 0, nb_keys: 0, keys: Vec::new(), tension: 0.0, step: 10, old_points: false }
    }

    /// A new spline of its own (`SI_splineAllocate`, 0x0848a990): initialised, and named
    /// `spl<n>` with the next number of the program's counter (`sprintf` with `spl%1hd`).
    pub fn allocate(st: &mut GeomState) -> Spline {
        st.spline_count = st.spline_count.wrapping_add(1);
        Spline { name: format!("spl{}", st.spline_count).into_bytes(), ..Spline::init() }
    }
}

fn key_size(field: i16) -> Option<usize> {
    match field {
        1..=3 => Some(12),
        4..=6 => Some(2),
        _ => None,
    }
}

/// The least a spline's fields take: the counts without their keys, the name's terminator.
fn spline_size(field: i16) -> Option<usize> {
    match field {
        1..=3 | 5..=7 => Some(2),
        4 => Some(4),
        8 => Some(1),
        _ => None,
    }
}

/// Reads one spline [soft 0x083f00dc splineBinaryRead]. Prototype 3 (list at offset 0):
///
/// | field | data | into |
/// |---|---|---|
/// | 1 | 16-bit | +0x0c type, sign-extended |
/// | 2 | 16-bit count, then per key a vector | +0x10, and each key's position: the old list of positions only |
/// | 3 | 16-bit | +0x08 close |
/// | 4 | 4 bytes | +0x1c tension |
/// | 5 | 16-bit | +0x20 drawing precision |
/// | 6 | 16-bit | +0x06 phantom key |
/// | 7 | 16-bit count, then per key a record of prototype 0x12 | +0x10, +0x14 |
/// | 8 | string | +0x00 name, replacing the allocator's |
/// | other | skipped by its size byte | |
///
/// Prototype 0x12, the key (list at offset 4): fields 1, 2, 3 a vector each (previous
/// tangent, position, next tangent), fields 4, 5, 6 16 bits each (constraint, segment,
/// flag); any other is skipped by its size byte.
///
/// The key loop and the record loop share one variable for the field number, so a key list
/// read without a prototype leaves a zero in it; if the next field number then cannot be
/// read, the record ends there as if a zero had been stored.
///
/// The original counts the keys of field 7 with a signed 16-bit index against the unsigned
/// count: a count of 0x8000 or more is never reached and it writes on beyond the keys it
/// allocated. Here the keys the count names are read and `clamped` is set. Field 2 counts
/// down a signed short, so such a count allocates the keys and reads none.
///
/// The variable field 1 reads through is not initialised in the original; here it starts at
/// zero, as does the field number.
///
/// What the original does next, not reproduced: if field 2 was read and the type is 1 the
/// spline is replaced by `SI_bezierShrink` of it; `SI_splineUpdateMatrix`; and `SI_allDummy`
/// if the phantom field is not zero.
pub fn read_spline(r: &mut Reader, protos: &Prototypes, st: &mut GeomState) -> (Spline, End) {
    let mut s = Spline::allocate(st);
    let end = match spline_fields(r, protos, &mut s) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    (s, end)
}

fn spline_fields(r: &mut Reader, protos: &Prototypes, s: &mut Spline) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut kind = 0i16;
    let mut src = Fields::new(protos.by_offset(0));
    while src.step(r, &mut field, "spline")? {
        match field {
            1 => {
                fread_i16(&mut r.s, &mut kind);
                s.kind = kind as i32;
            }
            2 => {
                s.old_points = true;
                fread_u16(&mut r.s, &mut s.nb_keys);
                let (take, cut) = room(r, s.nb_keys as i64, 12);
                s.keys = vec![SplineKey::default(); take];
                if (s.nb_keys as i16) > 0 {
                    for k in s.keys.iter_mut() {
                        fread_vector(&mut r.s, &mut k.position);
                    }
                    if cut {
                        past_end(r);
                    }
                }
            }
            3 => {
                fread_i16(&mut r.s, &mut s.close);
            }
            4 => {
                fread_f32(&mut r.s, &mut s.tension);
            }
            5 => {
                fread_i16(&mut r.s, &mut s.step);
            }
            6 => {
                fread_i16(&mut r.s, &mut s.phantom);
            }
            7 => {
                fread_u16(&mut r.s, &mut s.nb_keys);
                let list = protos.by_offset(4);
                let (take, cut) = room(r, s.nb_keys as i64, min_size(list, key_size));
                s.keys = vec![SplineKey::default(); take];
                for k in s.keys.iter_mut() {
                    let mut ksrc = Fields::new(list);
                    while ksrc.step(r, &mut field, "splinekey")? {
                        match field {
                            1 => {
                                fread_vector(&mut r.s, &mut k.previous_tangent);
                            }
                            2 => {
                                fread_vector(&mut r.s, &mut k.position);
                            }
                            3 => {
                                fread_vector(&mut r.s, &mut k.next_tangent);
                            }
                            4 => {
                                fread_i16(&mut r.s, &mut k.constraint);
                            }
                            5 => {
                                fread_i16(&mut r.s, &mut k.segment);
                            }
                            6 => {
                                fread_i16(&mut r.s, &mut k.flag);
                            }
                            f => {
                                r.skip_field(f as u16);
                            }
                        }
                    }
                }
                if s.nb_keys >= 0x8000 {
                    // the original's loop does not end here: it overruns its keys
                    r.clamped = true;
                }
                if cut {
                    past_end(r);
                }
            }
            8 => s.name = r.string(),
            f => {
                r.skip_field(f as u16);
            }
        }
    }
    Ok(())
}

/// Writes one spline [soft 0x083f59e0 splineBinaryWrite]: the fields of prototype 3 as this
/// build announces it, 8 1 7 3 4 5 6, each key with fields 1 to 6. No version is consulted.
///
/// The type is written from the low 16 bits of its 32-bit field. The keys are counted down
/// with a signed short, so a count of 0x8000 or more writes the count and no key.
///
/// The original trusts the count and writes that many keys from its array. Here the keys the
/// record holds are written, at most the count.
pub fn write_spline(w: &mut Writer, s: &Spline) {
    w.string(Some(&s.name));
    w.put_u16(s.kind as u16);
    w.put_u16(s.nb_keys);
    for k in s.keys.iter().take((s.nb_keys as i16).max(0) as usize) {
        fwrite_vector(w, &k.previous_tangent);
        fwrite_vector(w, &k.position);
        fwrite_vector(w, &k.next_tangent);
        w.put_u16(k.constraint as u16);
        w.put_u16(k.segment as u16);
        w.put_u16(k.flag as u16);
    }
    w.put_u16(s.close as u16);
    w.put_f32(s.tension);
    w.put_u16(s.step as u16);
    w.put_u16(s.phantom as u16);
}

// --- NURBS curve ---------------------------------------------------------------------------------

/// A control point of a NURBS curve or surface (0x24 bytes)
/// [soft 0x084dc47c SI_nurbsPointsAllocate]: allocated as (0, 0, 0, 1) with flag 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NurbsPoint {
    /// +0x00: "Points x, y, z, w (saved in homogeneous coordinates.)"
    pub xyzw: [f64; 4],
    /// +0x20: "Flag"
    pub flag: i32,
}

impl Default for NurbsPoint {
    fn default() -> Self {
        NurbsPoint { xyzw: [0.0, 0.0, 0.0, 1.0], flag: 0 }
    }
}

/// A NURBS curve (`__SI_NurbsCrv`, 100 bytes), as far as a file reaches into it. Everything
/// else is zero in a new curve (`SI_nurbsCrvAllocate`, 0x084db5a0).
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    /// +0x00, field 1: "Direction order (degree + 1)"
    pub order: i32,
    /// +0x04, field 2: the number of control points
    pub nb_points: i32,
    /// +0x2c: the control points
    pub points: Vec<NurbsPoint>,
    /// +0x20, field 3: "Closed flag"
    pub closed: i16,
    /// +0x24, field 4: "Uniformity"
    pub uniformity: i32,
    /// +0x08, field 4: the number of knots (32 bits, although the page says short)
    pub nb_knots: i32,
    /// +0x0c: the knots [soft 0x084dc500 si_knotsAllocate]
    pub knots: Vec<f64>,
    /// +0x10, field 5: "Start"
    pub start: f64,
    /// +0x18, field 5: "End"
    pub end: f64,
    /// +0x34, field 6: "Display steps"
    pub display_steps: i32,
    /// +0x30, field 7: "Closing information (normal, preserved, linear)"
    pub closing: i32,
    /// +0x4c, fields 8 and 0x208: "Key framed flag"
    pub key_framed: i16,
}

impl Default for NurbsCurve {
    /// A new curve (`SI_nurbsCrvAllocate`, 0x084db5a0): order 4, display steps 10, closing 1.
    fn default() -> Self {
        NurbsCurve {
            order: 4,
            nb_points: 0,
            points: Vec::new(),
            closed: 0,
            uniformity: 0,
            nb_knots: 0,
            knots: Vec::new(),
            start: 0.0,
            end: 0.0,
            display_steps: 10,
            closing: 1,
            key_framed: 0,
        }
    }
}

fn nurbs_curve_field_size(field: i16) -> Option<usize> {
    match field {
        1 | 2 | 6 | 7 => Some(4),
        3 | 8 | 0x208 => Some(2),
        4 => Some(8),
        5 => Some(16),
        _ => None,
    }
}

/// The control points of a curve or a surface: per point four doubles in one read and, if
/// the file's version is 2.6 or more (the float `fileVersion` against the double 2.6 at
/// 0x0865bf30 and 0x0865bf38), a 32-bit flag. The float nearest 2.6 is below that double, so
/// a file whose header says 2.6 is read without flags.
fn read_nurbs_points(r: &mut Reader, count: i32) -> Vec<NurbsPoint> {
    let (take, cut) = room(r, count as i64, 32);
    let mut points = vec![NurbsPoint::default(); take];
    let with_flag = (r.st.file_version as f64) >= 2.6;
    for p in points.iter_mut() {
        fread_doubles(&mut r.s, &mut p.xyzw);
        if with_flag {
            fread_i32(&mut r.s, &mut p.flag);
        }
    }
    if cut {
        past_end(r);
    }
    points
}

/// A knot vector: the doubles in one read. A negative count is passed on as unsigned by the
/// original, to an allocation that fails and a read of the whole rest of the file through
/// its null result; here it gives no knots, and the rest of the file is consumed.
fn read_knots(r: &mut Reader, count: i32) -> Vec<f64> {
    let (take, cut) = room(r, count as i64, 8);
    let mut knots = vec![0.0f64; take];
    fread_doubles(&mut r.s, &mut knots);
    if count < 0 {
        r.clamped = true;
    }
    if cut || count < 0 {
        past_end(r);
    }
    knots
}

/// Reads one NURBS curve [soft 0x083f2b38 nurbsCrvBinaryRead]. Prototype 0x1d (list at
/// offset 0x64):
///
/// | field | data | into |
/// |---|---|---|
/// | 1 | 32-bit | +0x00 order |
/// | 2 | 32-bit count, then the control points | +0x04, +0x2c |
/// | 3 | 16-bit | +0x20 closed |
/// | 4 | 32-bit, 32-bit count, that many doubles | +0x24 uniformity, +0x08, +0x0c knots |
/// | 5 | two doubles | +0x10 start, +0x18 end |
/// | 6 | 32-bit | +0x34 display steps |
/// | 7 | 32-bit | +0x30 closing information |
/// | 8, 0x208 | 16-bit | +0x4c key framed |
/// | other | skipped by its size byte | |
///
/// The field number's variable is not initialised in the original; here it starts at zero.
/// Nothing is done to the curve after it is read.
pub fn read_nurbs_curve(r: &mut Reader, protos: &Prototypes) -> (NurbsCurve, End) {
    let mut c = NurbsCurve::default();
    let end = match nurbs_curve_fields(r, protos, &mut c) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    (c, end)
}

fn nurbs_curve_fields(r: &mut Reader, protos: &Prototypes, c: &mut NurbsCurve) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut src = Fields::new(protos.by_offset(0x64));
    while src.step(r, &mut field, "nurbscurve")? {
        match field {
            1 => {
                fread_i32(&mut r.s, &mut c.order);
            }
            2 => {
                fread_i32(&mut r.s, &mut c.nb_points);
                c.points = read_nurbs_points(r, c.nb_points);
            }
            3 => {
                fread_i16(&mut r.s, &mut c.closed);
            }
            4 => {
                fread_i32(&mut r.s, &mut c.uniformity);
                fread_i32(&mut r.s, &mut c.nb_knots);
                c.knots = read_knots(r, c.nb_knots);
            }
            5 => {
                fread_doubles(&mut r.s, std::slice::from_mut(&mut c.start));
                fread_doubles(&mut r.s, std::slice::from_mut(&mut c.end));
            }
            6 => {
                fread_i32(&mut r.s, &mut c.display_steps);
            }
            7 => {
                fread_i32(&mut r.s, &mut c.closing);
            }
            8 | 0x208 => {
                fread_i16(&mut r.s, &mut c.key_framed);
            }
            f => {
                r.skip_field(f as u16);
            }
        }
    }
    Ok(())
}

/// Writes one NURBS curve [soft 0x083f6d0c nurbsCrvBinaryWrite] (its writing mode): fields
/// 1 2 3 4 5 6 7 0x208 of prototype 0x1d, every point with its flag. No version is
/// consulted, although the prototype this build announces for an output version of 3.0 or
/// less leaves out 0x208, and 6 and 7 for 2.69 or less: a file written for such a version
/// does not match its own prototype.
///
/// Each point is copied to the stack through the x87 before it is written, which makes a
/// signalling NaN quiet.
///
/// The original stops at the first write that fails; writing to memory does not fail. It
/// trusts the two counts; here the points and knots the record holds are written, at most
/// the counts.
pub fn write_nurbs_curve(w: &mut Writer, c: &NurbsCurve) {
    w.put_i32(c.order);
    w.put_i32(c.nb_points);
    write_nurbs_points(w, &c.points, c.nb_points);
    w.put_u16(c.closed as u16);
    w.put_i32(c.uniformity);
    w.put_i32(c.nb_knots);
    for &k in c.knots.iter().take(c.nb_knots.max(0) as usize) {
        put_f64(w, k);
    }
    put_f64(w, c.start);
    put_f64(w, c.end);
    w.put_i32(c.display_steps);
    w.put_i32(c.closing);
    w.put_u16(c.key_framed as u16);
}

fn write_nurbs_points(w: &mut Writer, points: &[NurbsPoint], count: i32) {
    for p in points.iter().take(count.max(0) as usize) {
        for &d in &p.xyzw {
            put_f64(w, x87_copy(d));
        }
        w.put_i32(p.flag);
    }
}

/// The counting mode of [soft 0x083f6d0c nurbsCrvBinaryWrite]: the bytes the curve takes,
/// computed from its counts with 32-bit arithmetic.
pub fn nurbs_curve_size(c: &NurbsCurve) -> i32 {
    let points = c.nb_points.max(0).wrapping_mul(36);
    8i32.wrapping_add(points).wrapping_add(10).wrapping_add(c.nb_knots.wrapping_mul(8)).wrapping_add(26)
}

// --- face ----------------------------------------------------------------------------------------

/// A face (`SI_Face`, 0x88 bytes): a planar region bounded by splines, one for the exterior
/// and one for each hole, or by NURBS curves. A new face (`SI_faceAllocate`, 0x084347d0)
/// is made of splines and has none.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// +0x04, field 1: "Spline", the exterior, a spline record inside the face
    pub exterior: Spline,
    /// +0x78, fields 2 and 4: "Nb holes" or "Nb NURBS curves"
    pub count: i16,
    /// +0x7c, field 2: "Array of holes"
    pub holes: Vec<Spline>,
    /// +0x80, field 4: "Array of nurbs curves"
    pub curves: Vec<NurbsCurve>,
    /// +0x84, field 3: "Is a spline ?" (1 in a new face)
    pub is_spline: i16,
}

impl Default for Face {
    fn default() -> Self {
        Face { exterior: Spline::init(), count: 0, holes: Vec::new(), curves: Vec::new(), is_spline: 1 }
    }
}

/// Reads one face [soft 0x083f043c faceBinaryRead]. Prototype 7 (list at offset 0x10):
///
/// | field | data | condition |
/// |---|---|---|
/// | 1 | a spline: the exterior | only if +0x84 is not zero; otherwise nothing is read |
/// | 2 | 16-bit count into +0x78 and that many splines: the holes | likewise |
/// | 3 | 16-bit into +0x84: made of splines or not | |
/// | 4 | 16-bit count into +0x78 and that many NURBS curves | only if +0x84 is zero |
/// | other | skipped by its size byte | |
///
/// Fields 1, 2 and 4 look at +0x84 as it stands when they come, so field 3 has to precede
/// them (it does in the prototype this build writes: 3 1 2 4).
///
/// What the original does with each spline and curve, not reproduced: it reads the spline
/// into a record of its own (with all that [`read_spline`] lists), copies that into the
/// face with `SI_splineTransfer` and frees it; a curve likewise with `SI_nurbsCrvInit`.
/// Neither copy is complete: `SI_splineTransfer` does not take the name and
/// `SI_nurbsCrvInit` does not take the closing information (+0x30). The face the original
/// holds therefore has splines without names and curves with closing information 0, and
/// that is what it would write back; the record here keeps both as the file gave them.
///
/// A hole count of zero or less leaves the list of holes as it was. The original allocates
/// the list of curves by the count sign-extended to 32 bits; a negative count gives no
/// curves here.
pub fn read_face(r: &mut Reader, protos: &Prototypes, st: &mut GeomState) -> (Face, End) {
    let mut f = Face::default();
    let end = match face_fields(r, protos, st, &mut f) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    (f, end)
}

fn face_fields(r: &mut Reader, protos: &Prototypes, st: &mut GeomState, f: &mut Face) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut src = Fields::new(protos.by_offset(0x10));
    while src.step(r, &mut field, "face")? {
        match field {
            1 => {
                if f.is_spline != 0 {
                    let (s, end) = read_spline(r, protos, st);
                    f.exterior = s;
                    if end == End::Eof {
                        return Err(Eof);
                    }
                }
            }
            2 => {
                if f.is_spline != 0 {
                    fread_i16(&mut r.s, &mut f.count);
                    if f.count > 0 {
                        f.holes = Vec::new();
                    }
                    let (take, cut) = room(r, f.count as i64, min_size(protos.by_offset(0), spline_size));
                    for _ in 0..take {
                        let (s, end) = read_spline(r, protos, st);
                        f.holes.push(s);
                        if end == End::Eof {
                            return Err(Eof);
                        }
                    }
                    if cut {
                        past_end(r);
                    }
                }
            }
            3 => {
                fread_i16(&mut r.s, &mut f.is_spline);
            }
            4 => {
                if f.is_spline == 0 {
                    fread_i16(&mut r.s, &mut f.count);
                    f.curves = Vec::new();
                    let (take, cut) = room(r, f.count as i64, min_size(protos.by_offset(0x64), nurbs_curve_field_size));
                    for _ in 0..take {
                        let (c, end) = read_nurbs_curve(r, protos);
                        f.curves.push(c);
                        if end == End::Eof {
                            return Err(Eof);
                        }
                    }
                    if cut {
                        past_end(r);
                    }
                }
            }
            x => {
                r.skip_field(x as u16);
            }
        }
    }
    Ok(())
}

/// Writes one face [soft 0x083f58fc faceBinaryWrite]: field 3; then, for a face of splines,
/// the exterior (field 1), the count and the holes (field 2); for a face of curves, the
/// count and the curves (field 4). No version is consulted, although the prototype this
/// build announces for an output version of 2.99903 or less is 1 2, without field 3: a file
/// written for such a version does not match its own prototype.
///
/// The original trusts the count; here the holes or curves the record holds are written, at
/// most the count.
pub fn write_face(w: &mut Writer, f: &Face) {
    w.put_u16(f.is_spline as u16);
    if f.is_spline == 0 {
        w.put_u16(f.count as u16);
        for c in f.curves.iter().take(f.count.max(0) as usize) {
            write_nurbs_curve(w, c);
        }
    } else {
        write_spline(w, &f.exterior);
        w.put_u16(f.count as u16);
        for s in f.holes.iter().take(f.count.max(0) as usize) {
            write_spline(w, s);
        }
    }
}

// --- patch -----------------------------------------------------------------------------------------

/// A patch (`SI_Patch`, 0x204 bytes), as far as a file reaches into it. The rest of a new
/// record (`SI_patchAllocate`, 0x0847a0d0) is zero, but for four identity matrices at
/// +0xa4, +0xe4, +0x124 and +0x164.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    /// +0x04, field 1: "Type in U direction": 0 linear, 1 Bézier, 2 B-spline, 3 cardinal,
    /// 4 NURBS (16 bits in the file)
    pub type_u: i32,
    /// +0x08, field 2: "Type in V direction"
    pub type_v: i32,
    /// +0x0c, field 3: "Nb U points"
    pub nb_u: u16,
    /// +0x0e, field 3: "Nb V points"
    pub nb_v: u16,
    /// +0x1a4, field 3: "Array of points", 16 bytes each in memory: the three floats of the
    /// file and a fourth the reader sets to 1.0
    pub points: Vec<[f32; 4]>,
    /// +0x1a8, field 25: "Array of tagged points", one 16-bit word per point, zeroed when
    /// the points are read
    pub tags: Vec<i16>,
    /// +0x10, field 4: "Close in U" (anything not zero is stored as 1)
    pub close_u: i16,
    /// +0x12, field 5: "Close in V" (likewise)
    pub close_v: i16,
    /// +0x14, field 6: "Tension in U"
    pub tension_u: f32,
    /// +0x18, field 7: "Tension in V"
    pub tension_v: f32,
    /// +0x1c, field 8: "Drawing step in U"
    pub step_u: i16,
    /// +0x1e, field 9: "Drawing step in V"
    pub step_v: i16,
    /// +0x20, field 10: "U curves"
    pub curves_u: i16,
    /// +0x22, field 11: "V curves"
    pub curves_v: i16,
    /// +0x1cc, field 12: "U bottom"
    pub u_bottom: i16,
    /// +0x1ce, field 13: "U top"
    pub u_top: i16,
    /// +0x1d0, field 14: "V bottom"
    pub v_bottom: i16,
    /// +0x1d2, field 15: "V top"
    pub v_top: i16,
    /// +0x1d4, field 16: "Faceted"
    pub faceted: i16,
    /// +0x200, field 17: "Mental Ray Approx type"
    pub approx_type: i16,
    /// +0x1e8, field 18: "Mental Ray View Dependencies"
    pub view_dependent: i16,
    /// +0x1ec, field 19: "Mental Ray max. edge length"
    pub max_edge_length: f32,
    /// +0x1f0, field 20: "Mental Ray U curvature"
    pub curvature_u: f32,
    /// +0x1f4, field 21: "Mental Ray V curvature"
    pub curvature_v: f32,
    /// +0x1f8, field 22: "Mental Ray min recursivity"
    pub min_recursion: i16,
    /// +0x1fa, field 23: "Mental Ray max recursivity"
    pub max_recursion: i16,
    /// +0x1fc, field 24: "Mental Ray recursion"
    pub recursion: i16,
}

impl Default for Patch {
    /// A new patch (`SI_patchAllocate`, 0x0847a0d0): drawing steps 3, one curve each way, and the
    /// approximation's defaults (type 1, 5.0, 0.1, 45.0, recursion 1 to 10).
    fn default() -> Self {
        Patch {
            type_u: 0,
            type_v: 0,
            nb_u: 0,
            nb_v: 0,
            points: Vec::new(),
            tags: Vec::new(),
            close_u: 0,
            close_v: 0,
            tension_u: 0.0,
            tension_v: 0.0,
            step_u: 3,
            step_v: 3,
            curves_u: 1,
            curves_v: 1,
            u_bottom: 0,
            u_top: 0,
            v_bottom: 0,
            v_top: 0,
            faceted: 0,
            approx_type: 1,
            view_dependent: 0,
            max_edge_length: 5.0,
            curvature_u: 0.1,
            curvature_v: 45.0,
            min_recursion: 1,
            max_recursion: 10,
            recursion: 0,
        }
    }
}

impl Patch {
    /// The number of points the two counts ask for, as the reader and writer compute it:
    /// the product of the two unsigned 16-bit counts taken as a signed 32-bit number, so
    /// that a product of 2^31 or more counts as none.
    pub fn point_count(&self) -> i32 {
        (self.nb_u as u32).wrapping_mul(self.nb_v as u32) as i32
    }
}

/// Reads one patch [soft 0x083f0690 patchBinaryRead]. Prototype 6 (list at offset 0x0c):
///
/// | field | data | into |
/// |---|---|---|
/// | 1, 2 | 16-bit | +0x04, +0x08 type in U and V, sign-extended |
/// | 3 | two 16-bit counts, then a vector per point | +0x0c, +0x0e, +0x1a4; each point's fourth float set to 1.0 and its tag (+0x1a8) to 0 |
/// | 4, 5 | 16-bit | +0x10, +0x12 close in U and V; a value that is not zero is replaced by 1 |
/// | 6, 7 | 4 bytes | +0x14, +0x18 tension |
/// | 8, 9 | 16-bit | +0x1c, +0x1e drawing step |
/// | 10, 11 | 16-bit | +0x20, +0x22 curves |
/// | 12 to 15 | 16-bit | +0x1cc, +0x1ce, +0x1d0, +0x1d2: U bottom, U top, V bottom, V top |
/// | 16 | 16-bit | +0x1d4 faceted |
/// | 17 | 16-bit | +0x200 approximation type |
/// | 18 | 16-bit | +0x1e8 view dependency |
/// | 19, 20, 21 | float | +0x1ec, +0x1f0, +0x1f4 |
/// | 22, 23, 24 | 16-bit | +0x1f8, +0x1fa, +0x1fc |
/// | 25 | a 16-bit word per point | +0x1a8, the tags of the points read so far |
/// | other | skipped as a field with a 32-bit size, whatever its size byte says (the reader passes −1 to the skipping routine, not the field number) | |
///
/// The variable fields 1 and 2 read through is not initialised in the original, nor is the
/// field number; here both start at zero.
///
/// Field 25 stores into the tag array that field 3 allocated. If the counts were cut to the
/// file's length the words beyond the array are read and dropped (the original would have
/// overrun the file long before).
///
/// What the original does next, not reproduced: `SI_patchUpdateMatrix`; and the shape reader
/// calls `SI_patchUpdateSurface` on the definition it makes current.
pub fn read_patch(r: &mut Reader, protos: &Prototypes) -> (Patch, End) {
    let mut p = Patch::default();
    let end = match patch_fields(r, protos, &mut p) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    (p, end)
}

fn patch_fields(r: &mut Reader, protos: &Prototypes, p: &mut Patch) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut short = 0i16;
    let mut src = Fields::new(protos.by_offset(0x0c));
    while src.step(r, &mut field, "patch")? {
        let s = &mut r.s;
        match field {
            1 => {
                fread_i16(s, &mut short);
                p.type_u = short as i32;
            }
            2 => {
                fread_i16(s, &mut short);
                p.type_v = short as i32;
            }
            3 => {
                fread_u16(s, &mut p.nb_u);
                fread_u16(s, &mut p.nb_v);
                let (take, cut) = room(r, p.point_count() as i64, 12);
                p.points = vec![[0.0; 4]; take];
                p.tags = vec![0; take];
                for q in p.points.iter_mut() {
                    let mut v = [0.0f32; 3];
                    fread_vector(&mut r.s, &mut v);
                    *q = [v[0], v[1], v[2], 1.0];
                }
                if cut {
                    past_end(r);
                }
            }
            4 => {
                fread_i16(s, &mut p.close_u);
                if p.close_u != 0 {
                    p.close_u = 1;
                }
            }
            5 => {
                fread_i16(s, &mut p.close_v);
                if p.close_v != 0 {
                    p.close_v = 1;
                }
            }
            6 => {
                fread_f32(s, &mut p.tension_u);
            }
            7 => {
                fread_f32(s, &mut p.tension_v);
            }
            8 => {
                fread_i16(s, &mut p.step_u);
            }
            9 => {
                fread_i16(s, &mut p.step_v);
            }
            10 => {
                fread_i16(s, &mut p.curves_u);
            }
            11 => {
                fread_i16(s, &mut p.curves_v);
            }
            12 => {
                fread_i16(s, &mut p.u_bottom);
            }
            13 => {
                fread_i16(s, &mut p.u_top);
            }
            14 => {
                fread_i16(s, &mut p.v_bottom);
            }
            15 => {
                fread_i16(s, &mut p.v_top);
            }
            16 => {
                fread_i16(s, &mut p.faceted);
            }
            17 => {
                fread_i16(s, &mut p.approx_type);
            }
            18 => {
                fread_i16(s, &mut p.view_dependent);
            }
            19 => {
                fread_f32(s, &mut p.max_edge_length);
            }
            20 => {
                fread_f32(s, &mut p.curvature_u);
            }
            21 => {
                fread_f32(s, &mut p.curvature_v);
            }
            22 => {
                fread_i16(s, &mut p.min_recursion);
            }
            23 => {
                fread_i16(s, &mut p.max_recursion);
            }
            24 => {
                fread_i16(s, &mut p.recursion);
            }
            25 => {
                let (take, cut) = room(r, p.point_count() as i64, 2);
                for i in 0..take {
                    let mut dropped = 0i16;
                    fread_i16(&mut r.s, p.tags.get_mut(i).unwrap_or(&mut dropped));
                }
                if cut {
                    past_end(r);
                }
            }
            _ => {
                r.skip_field(0xffff);
            }
        }
    }
    Ok(())
}

/// Writes one patch [soft 0x083f5b00 patchBinaryWrite]: fields 1 to 16 of prototype 6 and,
/// unless the output version is 2.65 or less (the float against the double 2.65 at
/// 0x0865c408), fields 17 to 25. The types are written from the low 16 bits of their 32-bit
/// fields, the points as three floats each.
///
/// The original trusts the counts; here the points and tags the record holds are written,
/// at most the product of the counts.
pub fn write_patch(w: &mut Writer, p: &Patch) {
    let count = p.point_count().max(0) as usize;
    w.put_u16(p.type_u as u16);
    w.put_u16(p.type_v as u16);
    w.put_u16(p.nb_u);
    w.put_u16(p.nb_v);
    for q in p.points.iter().take(count) {
        fwrite_vector(w, &[q[0], q[1], q[2]]);
    }
    w.put_u16(p.close_u as u16);
    w.put_u16(p.close_v as u16);
    w.put_f32(p.tension_u);
    w.put_f32(p.tension_v);
    for v in [p.step_u, p.step_v, p.curves_u, p.curves_v, p.u_bottom, p.u_top, p.v_bottom, p.v_top, p.faceted] {
        w.put_u16(v as u16);
    }
    // "not (version <= 2.65)", so that a version that is not a number writes them too
    let old = (w.version_output as f64) <= 2.65;
    if !old {
        w.put_u16(p.approx_type as u16);
        w.put_u16(p.view_dependent as u16);
        w.put_f32(p.max_edge_length);
        w.put_f32(p.curvature_u);
        w.put_f32(p.curvature_v);
        w.put_u16(p.min_recursion as u16);
        w.put_u16(p.max_recursion as u16);
        w.put_u16(p.recursion as u16);
        for &t in p.tags.iter().take(count) {
            w.put_u16(t as u16);
        }
    }
}

// --- polygon mesh ------------------------------------------------------------------------------------

/// A vertex of a mesh (0x14 bytes; all zero when new, `SI_vertexAllocate`, 0x08459d48). The record also has
/// a list of the edges at the vertex (+0x0c, count at +0x10), which `SI_meshUpdateNodes`
/// builds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    /// +0x00, vertex field 1: "Position"
    pub position: [f32; 3],
    /// +0x12, vertex field 4: "Flag"
    pub flag: i16,
}

/// One corner of a polygon (0x24 bytes; allocated by `SI_polygonNodeAllocate`, 0x08459da8). The word
/// at +0x04 is not stored (zero in a new node).
///
/// Two nodes are equal if their bits are: the texture coordinates of a mesh that was never
/// given any are whatever its memory held when it was saved, NaNs among them.
#[derive(Clone, Debug)]
pub struct PolyNode {
    /// +0x00, node field 3 (32 bits) or the old field 1 (16 bits): "Vertex index"; −1 in a
    /// new node
    pub vertex: i32,
    /// +0x08, node field 2: "Vertex normal"
    pub normal: [f32; 3],
    /// +0x14, +0x18, node field 4: "U texture coordinate", "V texture coordinate" (the page
    /// calls them int; they are four bytes each and hold floats in every file of the corpus)
    pub uv: [f32; 2],
    /// +0x1c, node field 0x405: "Vertex color"; 0xffb2b2b2 in a new node. As the 32-bit
    /// number the file stores big-endian; its top byte is the one the reader forces to 0xff
    /// in files older than 3.71.
    pub colour: u32,
    /// +0x20, node field 0xff06: the coordinates in the model's further UV domains, one
    /// pair for each domain after the first. Empty in a node the field was never read for
    /// (a null pointer in the original) and in a model with one domain.
    pub more_uv: Vec<[f32; 2]>,
}

impl Default for PolyNode {
    fn default() -> Self {
        PolyNode { vertex: -1, normal: [0.0; 3], uv: [0.0; 2], colour: 0xffb2_b2b2, more_uv: Vec::new() }
    }
}

impl PartialEq for PolyNode {
    fn eq(&self, o: &PolyNode) -> bool {
        let bits = |a: &[f32], b: &[f32]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
        self.vertex == o.vertex
            && self.colour == o.colour
            && bits(&self.normal, &o.normal)
            && bits(&self.uv, &o.uv)
            && self.more_uv.len() == o.more_uv.len()
            && self.more_uv.iter().zip(&o.more_uv).all(|(a, b)| bits(a, b))
    }
}

/// A polygon (0x18 bytes) [soft 0x08459e20 SI_polygonAllocate].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    /// +0x00, polygon field 1: "Polygon normal" (a field this build does not write)
    pub normal: [f32; 3],
    /// +0x10, polygon field 2: "Nb nodes", unsigned
    pub nb_nodes: u16,
    /// +0x0c: "Polygon nodes"
    pub nodes: Vec<PolyNode>,
    /// +0x12, polygon field 3: "Material index"
    pub material: i16,
    /// +0x14, polygon field 4: "Flag"
    pub flag: i16,
}

/// An edge as a file lists it (0x14 bytes; allocated by `SI_edgeAllocate`, 0x08441fb8). The record
/// also has two words at +0x08 and +0x0c, −1 in a new edge (the page: "Polygons", not
/// saved), and a zero word at +0x10.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// +0x00, +0x04, edge field 4 (32 bits each) or the old field 1 (16 bits each):
    /// "Vertex start", "Vertex end"; −1 in a new edge
    pub vertex: [i32; 2],
    /// +0x12, edge field 3: "Flag"
    pub flag: i16,
}

impl Default for Edge {
    fn default() -> Self {
        Edge { vertex: [-1, -1], flag: 0 }
    }
}

/// A polygon mesh (`SI_Mesh`, 0x54 bytes), as far as a file reaches into it. Five words
/// at +0x24 to +0x34 are zero in a new mesh (`SI_meshAllocate`, 0x08458c20).
///
/// Fields 0x20a to 0x212 are not on the SDK's page (it describes version 3.5; they are
/// written from 3.8109 on). Seven of them have the defaults, the types and the order of a
/// patch's approximation fields 18 to 24 (view dependency, edge length, two curvatures,
/// recursion minimum, maximum and flag), which suggests the same meaning; that is a guess
/// and they are named by their offsets.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    /// +0x20, field 6: "Flag" (0x804 in a new mesh). The old field 1, "Faceted", sets bit 1.
    pub flag: u16,
    /// +0x0c, field 2: "Discontinuity angle (degrees)" (60 in a new mesh)
    pub angle: f32,
    /// +0x10: the cosine of the angle, which the reader computes when it reads field 2
    pub cos_angle: f32,
    /// +0x14, fields 7 and 3: "Nb vertices"
    pub nb_vertices: i32,
    /// +0x00: "Array of vertices"
    pub vertices: Vec<Vertex>,
    /// +0x18, fields 8 and 4: "Nb polygons"
    pub nb_polygons: i32,
    /// +0x04: "Array of polygons"
    pub polygons: Vec<Polygon>,
    /// Fields 9 and 5: "Nb edges", which the reader keeps in a variable of its own. (The
    /// mesh's count at +0x1c and its edges at +0x08 are set by `SI_meshUpdateNodes`.)
    pub nb_edges: i32,
    /// "Array of edges" as the file lists them: read into a scratch table and freed.
    pub edges: Vec<Edge>,
    /// Not in the record: an edge field was read (the original then has its scratch table,
    /// which is the condition for the normals to be computed).
    pub edge_field: bool,
    /// Not in the record: a node normal (node field 2) was read, so the original does not
    /// compute the normals.
    pub normals_read: bool,
    /// +0x52, field 0x20a (0 in a new mesh)
    pub w_52: i16,
    /// +0x50, field 0x20b (2 in a new mesh)
    pub w_50: i16,
    /// +0x38, field 0x20c (0)
    pub w_38: i16,
    /// +0x3c, field 0x40d (5.0)
    pub f_3c: f32,
    /// +0x40, field 0x40e (0.1)
    pub f_40: f32,
    /// +0x44, field 0x40f (45.0)
    pub f_44: f32,
    /// +0x48, field 0x210 (1)
    pub w_48: i16,
    /// +0x4a, field 0x211 (5)
    pub w_4a: i16,
    /// +0x4c, field 0x212 (0)
    pub w_4c: i16,
    /// Not in the record: the owning model's number of UV domains (`__SI_Model` +0x424) as
    /// it stood when the mesh had been read. Field 0x413 stores it in the model, and every
    /// node's further coordinates are counted by it; [`write_geometry`] writes the mesh with
    /// this number.
    pub uv_domains: i32,
}

impl Default for Mesh {
    /// A new mesh (`SI_meshAllocate`, 0x08458c20). The cosine is that of the double 1.04719755
    /// (0x0865faf8) stored as a single, which is 0.5.
    fn default() -> Self {
        Mesh {
            flag: 0x804,
            angle: 60.0,
            cos_angle: 0.5,
            nb_vertices: 0,
            vertices: Vec::new(),
            nb_polygons: 0,
            polygons: Vec::new(),
            nb_edges: 0,
            edges: Vec::new(),
            edge_field: false,
            normals_read: false,
            w_52: 0,
            w_50: 2,
            w_38: 0,
            f_3c: 5.0,
            f_40: 0.1,
            f_44: 45.0,
            w_48: 1,
            w_4a: 5,
            w_4c: 0,
            uv_domains: 1,
        }
    }
}

fn vertex_size(field: i16) -> Option<usize> {
    match field {
        1 => Some(12),
        4 => Some(2),
        _ => None,
    }
}

fn polygon_size(field: i16) -> Option<usize> {
    match field {
        1 => Some(12),
        2..=4 => Some(2),
        _ => None,
    }
}

fn node_size(field: i16) -> Option<usize> {
    match field as u16 {
        1 => Some(2),
        2 => Some(12),
        3 | 0x405 | 0xff06 => Some(4),
        4 => Some(8),
        _ => None,
    }
}

fn edge_size(field: i16) -> Option<usize> {
    match field {
        1 => Some(4),
        2 | 5 => Some(0),
        3 => Some(2),
        4 => Some(8),
        _ => None,
    }
}

/// Reads one polygon mesh [soft 0x083ef514 meshBinaryRead] (called with no table for the
/// edge numbers, as the shape reader calls it). Prototype 8 (list at offset 0x14):
///
/// | field | data | into |
/// |---|---|---|
/// | 1 | 16-bit | if not zero, bit 1 of the flag at +0x20 is set ("Faceted") |
/// | 2 | 4 bytes | +0x0c angle; +0x10 becomes its cosine |
/// | 3, 7 | count (16-bit signed for 3, 32-bit for 7), then the vertices | +0x14, +0x00 |
/// | 4, 8 | count (likewise), then the polygons | +0x18, +0x04 |
/// | 5, 9 | count (likewise), then the edges | a scratch table |
/// | 6 | 16-bit | +0x20 flag |
/// | 0x20a, 0x20b, 0x20c | 16-bit | +0x52, +0x50, +0x38 |
/// | 0x40d, 0x40e, 0x40f | float | +0x3c, +0x40, +0x44 |
/// | 0x210, 0x211, 0x212 | 16-bit | +0x48, +0x4a, +0x4c |
/// | 0x413 | 32-bit | the *model's* number of UV domains (+0x424) |
/// | other | skipped by its size byte | |
///
/// Prototype 0x0c, the vertex (offset 0x24): 1 a vector (position), 4 16-bit (flag).
///
/// Prototype 0x0a, the polygon (offset 0x1c): 1 a vector (normal); 2 a 16-bit count, taken
/// as unsigned, and that many nodes; 3 16-bit (material index); 4 16-bit (flag).
///
/// Prototype 0x0b, the node (offset 0x20): 1 16-bit (vertex index, sign-extended); 2 a
/// vector (normal); 3 32-bit (vertex index); 4 two 32-bit (U, V); 0x405 32-bit (colour),
/// and if `fileVersion` is below the single 3.71 (0x0865bcc0) the colour's top byte is set
/// to 0xff; 0xff06 a 32-bit size that is not looked at, then for each of the model's UV
/// domains after the first two 32-bit (U, V).
///
/// Prototype 9, the edge (offset 0x18): 1 two 16-bit (the vertices, sign-extended); 3
/// 16-bit (flag); 4 two 32-bit (the vertices); 2 and 5 are known and take nothing.
///
/// In all four, any other field is skipped by its size byte.
///
/// All five loops share one variable for the field number and one for the 16-bit values
/// they read on the way to a wider field; neither is initialised in the original, and both
/// start at zero here. A list read without a prototype leaves a zero in the first, so that
/// a field number that cannot be read after it ends the record.
///
/// The cosine: the angle times the double 0.0174532925 (0x0865bcb8) on the x87, stored as a
/// double, given to the C library's `cos`, stored as a single. The product is rounded as the
/// x87 rounds it; the cosine is this platform's, which may differ from the original
/// library's in the last bit of the single in rare cases. The field reaches no file.
///
/// A negative vertex or polygon count makes the original's allocator run through memory;
/// a negative edge count gives an empty table. All three give an empty list here.
///
/// What the original does next, not reproduced: `SI_meshUpdateNodes` (under
/// `SI_setCurrentModel`), which builds the mesh's own edges from the polygons; then, if an
/// edge field was read, every listed edge is looked up among them by its two vertices and
/// gives its flag to the edge found, the scratch table is freed, and unless a node normal
/// was read the normals are computed (`SI_meshComputesPolygonNormal`,
/// `SI_meshComputeHardEdge`, `SI_meshComputesVertexNormal`). The listed edges themselves are
/// not kept by the original: see [`write_mesh`].
pub fn read_mesh(r: &mut Reader, protos: &Prototypes, st: &mut GeomState) -> (Mesh, End) {
    let mut m = Mesh::default();
    let end = match mesh_fields(r, protos, st, &mut m) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    m.uv_domains = st.uv_domains;
    (m, end)
}

fn mesh_fields(r: &mut Reader, protos: &Prototypes, st: &mut GeomState, m: &mut Mesh) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut short = 0i16;
    let mut src = Fields::new(protos.by_offset(0x14));
    while src.step(r, &mut field, "mesh")? {
        match field as u16 {
            1 => {
                fread_i16(&mut r.s, &mut short);
                if short != 0 {
                    m.flag |= 2;
                }
            }
            2 => {
                fread_f32(&mut r.s, &mut m.angle);
                m.cos_angle = x87_mul_to_double(m.angle, 0.0174532925).cos() as f32;
            }
            3 | 7 => {
                if field == 3 {
                    fread_i16(&mut r.s, &mut short);
                    m.nb_vertices = short as i32;
                } else {
                    fread_i32(&mut r.s, &mut m.nb_vertices);
                }
                let list = protos.by_offset(0x24);
                let (take, cut) = room(r, m.nb_vertices as i64, min_size(list, vertex_size));
                m.vertices = vec![Vertex::default(); take];
                for v in m.vertices.iter_mut() {
                    let mut vsrc = Fields::new(list);
                    while vsrc.step(r, &mut field, "vertex")? {
                        match field {
                            1 => {
                                fread_vector(&mut r.s, &mut v.position);
                            }
                            4 => {
                                fread_i16(&mut r.s, &mut v.flag);
                            }
                            f => {
                                r.skip_field(f as u16);
                            }
                        }
                    }
                }
                if cut {
                    past_end(r);
                }
            }
            4 | 8 => {
                if field == 4 {
                    fread_i16(&mut r.s, &mut short);
                    m.nb_polygons = short as i32;
                } else {
                    fread_i32(&mut r.s, &mut m.nb_polygons);
                }
                let list = protos.by_offset(0x1c);
                let (take, cut) = room(r, m.nb_polygons as i64, min_size(list, polygon_size));
                m.polygons = vec![Polygon::default(); take];
                for p in m.polygons.iter_mut() {
                    polygon_fields(r, protos, st, list, p, &mut field, &mut short, &mut m.normals_read)?;
                }
                if cut {
                    past_end(r);
                }
            }
            5 | 9 => {
                if field == 5 {
                    fread_i16(&mut r.s, &mut short);
                    m.nb_edges = short as i32;
                } else {
                    fread_i32(&mut r.s, &mut m.nb_edges);
                }
                m.edge_field = true;
                let list = protos.by_offset(0x18);
                let (take, cut) = room(r, m.nb_edges as i64, min_size(list, edge_size));
                m.edges = vec![Edge::default(); take];
                for e in m.edges.iter_mut() {
                    let mut esrc = Fields::new(list);
                    while esrc.step(r, &mut field, "edge")? {
                        match field {
                            1 => {
                                fread_i16(&mut r.s, &mut short);
                                e.vertex[0] = short as i32;
                                fread_i16(&mut r.s, &mut short);
                                e.vertex[1] = short as i32;
                            }
                            2 | 5 => {}
                            3 => {
                                fread_i16(&mut r.s, &mut e.flag);
                            }
                            4 => {
                                fread_i32(&mut r.s, &mut e.vertex[0]);
                                fread_i32(&mut r.s, &mut e.vertex[1]);
                            }
                            f => {
                                r.skip_field(f as u16);
                            }
                        }
                    }
                }
                if cut {
                    past_end(r);
                }
            }
            6 => {
                fread_u16(&mut r.s, &mut m.flag);
            }
            0x20a => {
                fread_i16(&mut r.s, &mut m.w_52);
            }
            0x20b => {
                fread_i16(&mut r.s, &mut m.w_50);
            }
            0x20c => {
                fread_i16(&mut r.s, &mut m.w_38);
            }
            0x40d => {
                fread_f32(&mut r.s, &mut m.f_3c);
            }
            0x40e => {
                fread_f32(&mut r.s, &mut m.f_40);
            }
            0x40f => {
                fread_f32(&mut r.s, &mut m.f_44);
            }
            0x210 => {
                fread_i16(&mut r.s, &mut m.w_48);
            }
            0x211 => {
                fread_i16(&mut r.s, &mut m.w_4a);
            }
            0x212 => {
                fread_i16(&mut r.s, &mut m.w_4c);
            }
            0x413 => {
                fread_i32(&mut r.s, &mut st.uv_domains);
            }
            f => {
                r.skip_field(f);
            }
        }
    }
    Ok(())
}

/// One polygon with its nodes (part of [soft 0x083ef514 meshBinaryRead]).
#[allow(clippy::too_many_arguments)]
fn polygon_fields(
    r: &mut Reader,
    protos: &Prototypes,
    st: &GeomState,
    list: Option<&[i16]>,
    p: &mut Polygon,
    field: &mut i16,
    short: &mut i16,
    normals_read: &mut bool,
) -> Result<(), Eof> {
    let mut src = Fields::new(list);
    while src.step(r, field, "polygon")? {
        match *field {
            1 => {
                fread_vector(&mut r.s, &mut p.normal);
            }
            2 => {
                fread_i16(&mut r.s, short);
                p.nb_nodes = *short as u16;
                let nlist = protos.by_offset(0x20);
                let (take, cut) = room(r, p.nb_nodes as i64, min_size(nlist, node_size));
                p.nodes = vec![PolyNode::default(); take];
                for n in p.nodes.iter_mut() {
                    let mut nsrc = Fields::new(nlist);
                    while nsrc.step(r, field, "polynode")? {
                        match *field as u16 {
                            1 => {
                                fread_i16(&mut r.s, short);
                                n.vertex = *short as i32;
                            }
                            2 => {
                                *normals_read = true;
                                fread_vector(&mut r.s, &mut n.normal);
                            }
                            3 => {
                                fread_i32(&mut r.s, &mut n.vertex);
                            }
                            4 => {
                                fread_f32(&mut r.s, &mut n.uv[0]);
                                fread_f32(&mut r.s, &mut n.uv[1]);
                            }
                            0x405 => {
                                fread_u32(&mut r.s, &mut n.colour);
                                // two singles compared on the x87
                                if r.st.file_version < 3.71f32 {
                                    n.colour |= 0xff00_0000;
                                }
                            }
                            0xff06 => {
                                let mut size = 0u32;
                                fread_u32(&mut r.s, &mut size);
                                // (domains − 1) × 2 words are allocated, and for a count
                                // below 1 nothing is read
                                let (take, cut) = room(r, st.uv_domains as i64 - 1, 8);
                                let mut more = vec![[0.0f32; 2]; take];
                                for uv in more.iter_mut() {
                                    fread_f32(&mut r.s, &mut uv[0]);
                                    fread_f32(&mut r.s, &mut uv[1]);
                                }
                                n.more_uv = more;
                                if cut {
                                    past_end(r);
                                }
                            }
                            f => {
                                r.skip_field(f);
                            }
                        }
                    }
                }
                if cut {
                    past_end(r);
                }
            }
            3 => {
                fread_i16(&mut r.s, &mut p.material);
            }
            4 => {
                fread_i16(&mut r.s, &mut p.flag);
            }
            f => {
                r.skip_field(f as u16);
            }
        }
    }
    Ok(())
}

/// Writes one polygon mesh [soft 0x083f54f4 meshBinaryWrite] for a model with `uv_domains`
/// UV domains (`__SI_Model` +0x424). The output version is compared as a float with double
/// constants:
///
/// | written | condition |
/// |---|---|
/// | field 6 (flag), field 2 (angle) | always |
/// | field 0x413: the model's number of UV domains | version ≥ 3.93 (0x0865c3f0) |
/// | field 7: 32-bit count; per vertex fields 1 and 4 | always |
/// | field 8: 32-bit count; per polygon field 2 (16-bit count and the nodes), field 3 | always |
/// | per node fields 3, 2, 4 and 0x405 | always |
/// | per node field 0xff06: the byte size (domains − 1) × 8, then that many pairs less one | version ≥ 3.93 |
/// | per polygon field 4 (flag) | version > 2.65 (0x0865c3f8) |
/// | field 9: 32-bit count; per edge fields 4 and 3 | always |
/// | fields 0x20a to 0x212 | version ≥ 3.8109 (0x0865c400) |
///
/// The node fields 4 and 0x405 are written for every version, although the prototype of an
/// old file has neither.
///
/// The edges the original writes are the mesh's own (+0x08, count at +0x1c), the ones
/// `SI_meshUpdateNodes` built when the mesh was read or edited, in its order; the edge list
/// of the file a mesh came from is gone by then. That routine is not reproduced, so this
/// writes the list the record has: for a mesh that was read, the file's.
///
/// A node whose further coordinates were never read has a null pointer there, which the
/// original reads through when the model has more than one domain; here the missing pairs
/// are written as zeros. The original trusts every count; here the vertices, polygons,
/// nodes and edges the record holds are written, at most the counts.
pub fn write_mesh(w: &mut Writer, m: &Mesh, uv_domains: i32) {
    let version = w.version_output as f64;
    w.put_u16(m.flag);
    w.put_f32(m.angle);
    if version >= 3.93 {
        w.put_i32(uv_domains);
    }
    w.put_i32(m.nb_vertices);
    for v in m.vertices.iter().take(m.nb_vertices.max(0) as usize) {
        fwrite_vector(w, &v.position);
        w.put_u16(v.flag as u16);
    }
    w.put_i32(m.nb_polygons);
    for p in m.polygons.iter().take(m.nb_polygons.max(0) as usize) {
        w.put_u16(p.nb_nodes);
        for n in p.nodes.iter().take(p.nb_nodes as usize) {
            w.put_i32(n.vertex);
            fwrite_vector(w, &n.normal);
            w.put_f32(n.uv[0]);
            w.put_f32(n.uv[1]);
            w.put_i32(n.colour as i32);
            if version >= 3.93 {
                w.put_i32(uv_domains.wrapping_sub(1).wrapping_mul(8));
                for i in 0..uv_domains.wrapping_sub(1).max(0) as usize {
                    let uv = n.more_uv.get(i).copied().unwrap_or([0.0; 2]);
                    w.put_f32(uv[0]);
                    w.put_f32(uv[1]);
                }
            }
        }
        w.put_u16(p.material as u16);
        if version > 2.65 {
            w.put_u16(p.flag as u16);
        }
    }
    w.put_i32(m.nb_edges);
    for e in m.edges.iter().take(m.nb_edges.max(0) as usize) {
        w.put_i32(e.vertex[0]);
        w.put_i32(e.vertex[1]);
        w.put_u16(e.flag as u16);
    }
    if version >= 3.8109 {
        w.put_u16(m.w_52 as u16);
        w.put_u16(m.w_50 as u16);
        w.put_u16(m.w_38 as u16);
        w.put_f32(m.f_3c);
        w.put_f32(m.f_40);
        w.put_f32(m.f_44);
        w.put_u16(m.w_48 as u16);
        w.put_u16(m.w_4a as u16);
        w.put_u16(m.w_4c as u16);
    }
}

// --- NURBS surface -------------------------------------------------------------------------------------

/// A composite curve on a NURBS surface (`SI_NurbsCompCrv`, 0x18 bytes): a trimming or a
/// projected curve made of several curves. The three words after the list are zero.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NurbsComposite {
    /// +0x00, field 1: "Is boundary flag"
    pub is_boundary: i16,
    /// +0x04, field 2: "Nb curves"
    pub nb_curves: i32,
    /// +0x08: "Array of <nurbs curves>"
    pub curves: Vec<NurbsCurve>,
}

/// A NURBS surface (`SI_NurbsSrf`, 0xdc bytes), as far as a file reaches into it; the rest
/// is zero in a new surface (`SI_nurbsSrfAllocate`, 0x084db66c).
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsSurface {
    /// +0x00, +0x24, field 1: "Order in U", "Order in V"
    pub order: [i32; 2],
    /// +0x04, +0x28, field 2: "Nb points in U", "Nb points in V"
    pub nb_points: [i32; 2],
    /// +0x48: the product of the two, which the reader stores and the writer counts by
    pub point_count: i32,
    /// +0x54: the control points
    pub points: Vec<NurbsPoint>,
    /// +0x20, +0x44, field 3: "Closed flag in U", "Closed flag in V"
    pub closed: [i16; 2],
    /// +0x4c, field 4: "Uniformity": 0 uniform, 1 open uniform, 2 non uniform,
    /// 3 chord-length, 4 centripetal
    pub uniformity: i32,
    /// +0x08, +0x2c, field 4: the numbers of knots in U and V
    pub nb_knots: [i32; 2],
    /// +0x0c, +0x30: "Knot list in U", "Knot list in V"
    pub knots: [Vec<f64>; 2],
    /// +0x10, +0x18, +0x34, +0x3c, field 5: "Start in U", "End in U", "Start in V", "End in V"
    pub range: [f64; 4],
    /// +0x5c, field 6: "Border flag": true if the boundary is the entire domain, and the
    /// trim curves are then holes
    pub border: i16,
    /// +0x60, field 7: the number of trim curves
    pub nb_trims: i32,
    /// +0x64: "Trim curves list"
    pub trims: Vec<NurbsComposite>,
    /// +0x68, field 0xff1a (or 0x1a): the number of projected curves
    pub nb_projected: i32,
    /// +0x6c: "Projected curves"
    pub projected: Vec<NurbsComposite>,
    /// +0x70, +0x74, fields 8, 9: "Display steps in U", "Display steps in V"
    pub display_steps: [i32; 2],
    /// +0x78, +0x7c, fields 10, 11: "Nb wireframes curves in U", "… in V"
    pub wire_curves: [i32; 2],
    /// +0x88, +0x8a, +0x8c, +0x8e, fields 12 to 15: "Bottom capping flag in U", "Top … in
    /// U", "Bottom … in V", "Top … in V"
    pub capping: [i16; 4],
    /// +0x90, +0x94, fields 16, 17: "Closing information in U", "… in V"
    pub closing: [i32; 2],
    /// +0x98, field 18: "Approximation method"
    pub approx_method: i16,
    /// +0x9c, field 19: "View dependency: (0 = camera space, 1 = pixel)"
    pub view_dependency: i16,
    /// +0xa0, field 20: "Spatial (max edge length)"
    pub max_edge_length: f32,
    /// +0xa4, +0xa8, fields 21, 22: "Curvature in U", "Curvature in V"
    pub curvature: [f32; 2],
    /// +0xac, field 23: "Recursivity min"
    pub min_recursion: i16,
    /// +0xae, field 24: "Recursivity max"
    pub max_recursion: i16,
    /// +0xb0, field 25: "Recursion flag"
    pub recursion: i16,
}

impl Default for NurbsSurface {
    /// A new surface (`SI_nurbsSrfAllocate`, 0x084db66c): order 4 both ways, display steps 3, one
    /// wireframe curve, closing 1, and the approximation's defaults (method 1, the three
    /// floats at 0x08661740: 5.0, 0.1, 45.0; recursion 1 to 10).
    fn default() -> Self {
        NurbsSurface {
            order: [4, 4],
            nb_points: [0, 0],
            point_count: 0,
            points: Vec::new(),
            closed: [0, 0],
            uniformity: 0,
            nb_knots: [0, 0],
            knots: [Vec::new(), Vec::new()],
            range: [0.0; 4],
            border: 0,
            nb_trims: 0,
            trims: Vec::new(),
            nb_projected: 0,
            projected: Vec::new(),
            display_steps: [3, 3],
            wire_curves: [1, 1],
            capping: [0; 4],
            closing: [1, 1],
            approx_method: 1,
            view_dependency: 0,
            max_edge_length: 5.0,
            curvature: [0.1, 45.0],
            min_recursion: 1,
            max_recursion: 10,
            recursion: 0,
        }
    }
}

/// The least a composite curve's fields take; an unknown one is skipped as a long field.
fn nurbs_composite_field_size(field: i16) -> Option<usize> {
    Some(if field == 1 { 2 } else { 4 })
}

/// Reads one composite curve [soft 0x083f32e4 nurbsCompBinaryRead]. Prototype 0x1f (list at
/// offset 0x6c): field 1 16-bit (boundary flag); field 2 a 32-bit count and that many NURBS
/// curves; any other is skipped as a field with a 32-bit size (the reader passes −1, as the
/// patch reader does). The field number's variable is not initialised in the original; here
/// it starts at zero.
pub fn read_nurbs_composite(r: &mut Reader, protos: &Prototypes) -> (NurbsComposite, End) {
    let mut c = NurbsComposite::default();
    let end = match nurbs_composite_fields(r, protos, &mut c) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    (c, end)
}

fn nurbs_composite_fields(r: &mut Reader, protos: &Prototypes, c: &mut NurbsComposite) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut src = Fields::new(protos.by_offset(0x6c));
    while src.step(r, &mut field, "nurbscomp")? {
        match field {
            1 => {
                fread_i16(&mut r.s, &mut c.is_boundary);
            }
            2 => {
                fread_i32(&mut r.s, &mut c.nb_curves);
                c.curves = Vec::new();
                let (take, cut) = room(r, c.nb_curves as i64, min_size(protos.by_offset(0x64), nurbs_curve_field_size));
                for _ in 0..take {
                    let (curve, end) = read_nurbs_curve(r, protos);
                    c.curves.push(curve);
                    if end == End::Eof {
                        return Err(Eof);
                    }
                }
                if cut {
                    past_end(r);
                }
            }
            _ => {
                r.skip_field(0xffff);
            }
        }
    }
    Ok(())
}

/// A list of composite curves: a 32-bit count and, if it is above zero, that many records
/// in a new list (a count of zero or less leaves the list as it was).
fn read_composites(r: &mut Reader, protos: &Prototypes, count: &mut i32, list: &mut Vec<NurbsComposite>) -> Result<(), Eof> {
    fread_i32(&mut r.s, count);
    if *count > 0 {
        *list = Vec::new();
        let (take, cut) = room(r, *count as i64, min_size(protos.by_offset(0x6c), nurbs_composite_field_size));
        for _ in 0..take {
            let (c, end) = read_nurbs_composite(r, protos);
            list.push(c);
            if end == End::Eof {
                return Err(Eof);
            }
        }
        if cut {
            past_end(r);
        }
    }
    Ok(())
}

/// Reads one NURBS surface [soft 0x083f2d90 nurbsSrfBinaryRead]. Prototype 0x1e (list at
/// offset 0x68):
///
/// | field | data | into |
/// |---|---|---|
/// | 1 | two 32-bit | +0x00, +0x24 order in U and V |
/// | 2 | two 32-bit counts, then the control points | +0x04, +0x28; their product at +0x48; +0x54 |
/// | 3 | two 16-bit | +0x20, +0x44 closed in U and V |
/// | 4 | 32-bit; 32-bit count and doubles; 32-bit count and doubles | +0x4c uniformity; +0x08, +0x0c knots in U; +0x2c, +0x30 knots in V |
/// | 5 | four doubles | +0x10, +0x18, +0x34, +0x3c |
/// | 6 | 16-bit | +0x5c border flag |
/// | 7 | 32-bit count, then composite curves | +0x60, +0x64 trim curves |
/// | 8, 9 | 32-bit | +0x70, +0x74 display steps |
/// | 10, 11 | 32-bit | +0x78, +0x7c wireframe curves |
/// | 12 to 15 | 16-bit | +0x88, +0x8a, +0x8c, +0x8e capping flags |
/// | 16, 17 | 32-bit | +0x90, +0x94 closing information |
/// | 18, 19 | 16-bit | +0x98, +0x9c |
/// | 20, 21, 22 | float | +0xa0, +0xa4, +0xa8 |
/// | 23, 24, 25 | 16-bit | +0xac, +0xae, +0xb0 |
/// | 0x1a | 32-bit count, then composite curves | +0x68, +0x6c projected curves |
/// | 0xff1a | a 32-bit size that is not looked at, then as 0x1a | |
/// | other | skipped by its size byte | |
///
/// The field number's variable is not initialised in the original; here it starts at zero.
///
/// What the original does next, not reproduced: `SI_nurbsSrfForceCapping` for each capping
/// flag that is set on a side whose direction is not closed.
pub fn read_nurbs_surface(r: &mut Reader, protos: &Prototypes) -> (NurbsSurface, End) {
    let mut s = NurbsSurface::default();
    let end = match nurbs_surface_fields(r, protos, &mut s) {
        Ok(()) => End::Zero,
        Err(Eof) => End::Eof,
    };
    (s, end)
}

fn nurbs_surface_fields(r: &mut Reader, protos: &Prototypes, s: &mut NurbsSurface) -> Result<(), Eof> {
    let mut field = 0i16;
    let mut src = Fields::new(protos.by_offset(0x68));
    while src.step(r, &mut field, "nurbssurf")? {
        match field as u16 {
            1 => {
                fread_i32(&mut r.s, &mut s.order[0]);
                fread_i32(&mut r.s, &mut s.order[1]);
            }
            2 => {
                fread_i32(&mut r.s, &mut s.nb_points[0]);
                fread_i32(&mut r.s, &mut s.nb_points[1]);
                s.point_count = s.nb_points[0].wrapping_mul(s.nb_points[1]);
                s.points = read_nurbs_points(r, s.point_count);
            }
            3 => {
                fread_i16(&mut r.s, &mut s.closed[0]);
                fread_i16(&mut r.s, &mut s.closed[1]);
            }
            4 => {
                fread_i32(&mut r.s, &mut s.uniformity);
                fread_i32(&mut r.s, &mut s.nb_knots[0]);
                s.knots[0] = read_knots(r, s.nb_knots[0]);
                fread_i32(&mut r.s, &mut s.nb_knots[1]);
                s.knots[1] = read_knots(r, s.nb_knots[1]);
            }
            5 => {
                for d in s.range.iter_mut() {
                    fread_doubles(&mut r.s, std::slice::from_mut(d));
                }
            }
            6 => {
                fread_i16(&mut r.s, &mut s.border);
            }
            7 => read_composites(r, protos, &mut s.nb_trims, &mut s.trims)?,
            8 | 9 => {
                fread_i32(&mut r.s, &mut s.display_steps[field as usize - 8]);
            }
            10 | 11 => {
                fread_i32(&mut r.s, &mut s.wire_curves[field as usize - 10]);
            }
            12..=15 => {
                fread_i16(&mut r.s, &mut s.capping[field as usize - 12]);
            }
            16 | 17 => {
                fread_i32(&mut r.s, &mut s.closing[field as usize - 16]);
            }
            18 => {
                fread_i16(&mut r.s, &mut s.approx_method);
            }
            19 => {
                fread_i16(&mut r.s, &mut s.view_dependency);
            }
            20 => {
                fread_f32(&mut r.s, &mut s.max_edge_length);
            }
            21 | 22 => {
                fread_f32(&mut r.s, &mut s.curvature[field as usize - 21]);
            }
            23 => {
                fread_i16(&mut r.s, &mut s.min_recursion);
            }
            24 => {
                fread_i16(&mut r.s, &mut s.max_recursion);
            }
            25 => {
                fread_i16(&mut r.s, &mut s.recursion);
            }
            0x1a | 0xff1a => {
                if field as u16 == 0xff1a {
                    let mut size = 0u32;
                    fread_u32(&mut r.s, &mut size);
                }
                read_composites(r, protos, &mut s.nb_projected, &mut s.projected)?;
            }
            f => {
                r.skip_field(f);
            }
        }
    }
    Ok(())
}

/// Writes one composite curve [soft 0x083f76c0 nurbsCompBinaryWrite] (its writing mode): the
/// boundary flag, the count and the curves.
pub fn write_nurbs_composite(w: &mut Writer, c: &NurbsComposite) {
    w.put_u16(c.is_boundary as u16);
    w.put_i32(c.nb_curves);
    for curve in c.curves.iter().take(c.nb_curves.max(0) as usize) {
        write_nurbs_curve(w, curve);
    }
}

/// The counting mode of [soft 0x083f76c0 nurbsCompBinaryWrite]: six bytes and the curves'.
pub fn nurbs_composite_size(c: &NurbsComposite) -> i32 {
    c.curves.iter().take(c.nb_curves.max(0) as usize).fold(6i32, |n, curve| n.wrapping_add(nurbs_curve_size(curve)))
}

/// Writes one NURBS surface [soft 0x083f7038 nurbsSrfBinaryWrite]: fields 1 to 25 of
/// prototype 0x1e in order, then field 0xff1a: the byte size of what follows it (4 and the
/// projected curves, from a counting pass over them), the count and the projected curves.
/// No version is consulted, although the prototype this build announces lacks 0xff1a for an
/// output version of 3.0905 or less and fields 8 to 25 for 2.69 or less.
///
/// The points are counted by the product stored at +0x48, not by the two counts. Each is
/// copied through the x87 as in [`write_nurbs_curve`].
///
/// The original stops at the first write that fails; writing to memory does not fail. A
/// null entry in either list of composite curves writes nothing; here an entry the lists do
/// not hold is likewise left out.
pub fn write_nurbs_surface(w: &mut Writer, s: &NurbsSurface) {
    w.put_i32(s.order[0]);
    w.put_i32(s.order[1]);
    w.put_i32(s.nb_points[0]);
    w.put_i32(s.nb_points[1]);
    write_nurbs_points(w, &s.points, s.point_count);
    w.put_u16(s.closed[0] as u16);
    w.put_u16(s.closed[1] as u16);
    w.put_i32(s.uniformity);
    for d in 0..2 {
        w.put_i32(s.nb_knots[d]);
        for &k in s.knots[d].iter().take(s.nb_knots[d].max(0) as usize) {
            put_f64(w, k);
        }
    }
    for &d in &s.range {
        put_f64(w, d);
    }
    w.put_u16(s.border as u16);
    w.put_i32(s.nb_trims);
    for c in s.trims.iter().take(s.nb_trims.max(0) as usize) {
        write_nurbs_composite(w, c);
    }
    w.put_i32(s.display_steps[0]);
    w.put_i32(s.display_steps[1]);
    w.put_i32(s.wire_curves[0]);
    w.put_i32(s.wire_curves[1]);
    for &c in &s.capping {
        w.put_u16(c as u16);
    }
    w.put_i32(s.closing[0]);
    w.put_i32(s.closing[1]);
    w.put_u16(s.approx_method as u16);
    w.put_u16(s.view_dependency as u16);
    w.put_f32(s.max_edge_length);
    w.put_f32(s.curvature[0]);
    w.put_f32(s.curvature[1]);
    w.put_u16(s.min_recursion as u16);
    w.put_u16(s.max_recursion as u16);
    w.put_u16(s.recursion as u16);
    let projected = s.projected.iter().take(s.nb_projected.max(0) as usize);
    let size = projected.clone().fold(4i32, |n, c| n.wrapping_add(nurbs_composite_size(c)));
    w.put_i32(size);
    w.put_i32(s.nb_projected);
    for c in projected {
        write_nurbs_composite(w, c);
    }
}

// --- one definition of a model's kind ----------------------------------------------------------------

/// A definition of a model's geometry, by the model's type (`SI_ModelType`, `__SI_Model`
/// +0x30): 1 patch, 2 face, 4 mesh, 6 spline, 9 NURBS curve, 10 NURBS surface.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Patch(Patch),
    Face(Face),
    Mesh(Mesh),
    Spline(Spline),
    NurbsCurve(NurbsCurve),
    NurbsSurface(NurbsSurface),
}

impl Geometry {
    /// The model type this definition belongs to.
    pub fn model_type(&self) -> i32 {
        match self {
            Geometry::Patch(_) => 1,
            Geometry::Face(_) => 2,
            Geometry::Mesh(_) => 4,
            Geometry::Spline(_) => 6,
            Geometry::NurbsCurve(_) => 9,
            Geometry::NurbsSurface(_) => 10,
        }
    }

    /// A new, empty definition of a model type, as the type's allocator gives it; `None`
    /// for a type that has no geometry of these kinds.
    pub fn allocate(model_type: i32, st: &mut GeomState) -> Option<Geometry> {
        Some(match model_type {
            1 => Geometry::Patch(Patch::default()),
            2 => Geometry::Face(Face::default()),
            4 => Geometry::Mesh(Mesh { uv_domains: st.uv_domains, ..Mesh::default() }),
            6 => Geometry::Spline(Spline::allocate(st)),
            9 => Geometry::NurbsCurve(NurbsCurve::default()),
            10 => Geometry::NurbsSurface(NurbsSurface::default()),
            _ => return None,
        })
    }
}

/// The least number of bytes one definition of a model type takes in a file with these
/// prototypes; zero for a type that has no definitions and for a prototype none of whose
/// fields is stored. For a reader to check a count of definitions against the bytes left.
pub fn definition_min_size(protos: &Prototypes, model_type: i32) -> usize {
    fn patch(field: i16) -> Option<usize> {
        Some(match field {
            3 | 6 | 7 | 19..=21 => 4,
            1 | 2 | 4 | 5 | 8..=18 | 22..=24 => 2,
            25 => 0,
            // anything else is skipped as a long field
            _ => 4,
        })
    }
    fn face(field: i16) -> Option<usize> {
        match field {
            // the splines and curves are read or not as field 3 says
            1 | 2 | 4 => Some(0),
            3 => Some(2),
            _ => None,
        }
    }
    fn mesh(field: i16) -> Option<usize> {
        match field as u16 {
            1 | 3..=6 | 0x20a..=0x20c | 0x210..=0x212 => Some(2),
            2 | 7..=9 | 0x40d..=0x40f | 0x413 => Some(4),
            _ => None,
        }
    }
    fn surface(field: i16) -> Option<usize> {
        match field as u16 {
            1 | 2 => Some(8),
            3 | 7..=11 | 16 | 17 | 20..=22 | 0x1a => Some(4),
            4 => Some(12),
            5 => Some(32),
            6 | 12..=15 | 18 | 19 | 23..=25 => Some(2),
            0xff1a => Some(8),
            _ => None,
        }
    }
    match model_type {
        1 => min_size(protos.by_offset(0x0c), patch),
        2 => min_size(protos.by_offset(0x10), face),
        4 => min_size(protos.by_offset(0x14), mesh),
        6 => min_size(protos.by_offset(0), spline_size),
        9 => min_size(protos.by_offset(0x64), nurbs_curve_field_size),
        10 => min_size(protos.by_offset(0x68), surface),
        _ => 0,
    }
}

/// One definition of the model's kind, as the shape reader reads it (the first `switch` of
/// [soft 0x083f1798 SI_shapeBinaryRead], on the model's type less one against a table of
/// ten): types 1, 2, 4, 6, 9 and 10 call their record's reader; for any other type (3, 5,
/// 7, 8, anything below 1 or above 10) the original skips one field with a 32-bit size
/// (`si_fileSkipField` with −1) and there is no definition: `None`.
///
/// This form keeps no state between calls: the spline counter starts at zero and the model
/// has one UV domain. [`read_geometry_in`] is the same with the state in the caller's hands.
pub fn read_geometry(r: &mut Reader, protos: &Prototypes, model_type: i32) -> (Option<Geometry>, End) {
    read_geometry_in(r, protos, model_type, &mut GeomState::default())
}

/// [`read_geometry`] with the state the readers use and change: the program's spline
/// counter and the model's number of UV domains.
pub fn read_geometry_in(r: &mut Reader, protos: &Prototypes, model_type: i32, st: &mut GeomState) -> (Option<Geometry>, End) {
    match model_type {
        1 => {
            let (p, end) = read_patch(r, protos);
            (Some(Geometry::Patch(p)), end)
        }
        2 => {
            let (f, end) = read_face(r, protos, st);
            (Some(Geometry::Face(f)), end)
        }
        4 => {
            let (m, end) = read_mesh(r, protos, st);
            (Some(Geometry::Mesh(m)), end)
        }
        6 => {
            let (s, end) = read_spline(r, protos, st);
            (Some(Geometry::Spline(s)), end)
        }
        9 => {
            let (c, end) = read_nurbs_curve(r, protos);
            (Some(Geometry::NurbsCurve(c)), end)
        }
        10 => {
            let (s, end) = read_nurbs_surface(r, protos);
            (Some(Geometry::NurbsSurface(s)), end)
        }
        _ => {
            r.skip_field(0xffff);
            (None, End::Zero)
        }
    }
}

/// Writes one definition with its record's writer, as the shape writer does for the model's
/// type. A mesh is written for the number of UV domains it was read with
/// ([`Mesh::uv_domains`]); [`write_geometry_in`] takes the model's number instead.
pub fn write_geometry(w: &mut Writer, g: &Geometry) {
    let uv_domains = match g {
        Geometry::Mesh(m) => m.uv_domains,
        _ => 1,
    };
    write_geometry_in(w, g, uv_domains);
}

/// [`write_geometry`] for a model with `uv_domains` UV domains (`__SI_Model` +0x424), which
/// only a mesh uses.
pub fn write_geometry_in(w: &mut Writer, g: &Geometry, uv_domains: i32) {
    match g {
        Geometry::Patch(p) => write_patch(w, p),
        Geometry::Face(f) => write_face(w, f),
        Geometry::Mesh(m) => write_mesh(w, m, uv_domains),
        Geometry::Spline(s) => write_spline(w, s),
        Geometry::NurbsCurve(c) => write_nurbs_curve(w, c),
        Geometry::NurbsSurface(s) => write_nurbs_surface(w, s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{output_lists, read_prototypes, write_prototypes};

    /// The prototypes this build writes for an output version, read back.
    fn protos(version: f32) -> Prototypes {
        let mut w = Writer::with_version(version);
        write_prototypes(&mut w);
        let mut p = Prototypes::new();
        read_prototypes(&mut Reader::new(&w.out[4..]), &mut p);
        p
    }

    fn be(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    fn floats(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_bits().to_be_bytes()).collect()
    }

    fn key(i: usize) -> SplineKey {
        SplineKey { previous_tangent: [i as f32, 1.0, 2.0], position: [0.5, i as f32, -2.0], next_tangent: [3.0, 4.0, i as f32], f_24: 1.0, constraint: 2, segment: 1, flag: i as i16 }
    }

    fn spline(name: &[u8], n: usize) -> Spline {
        Spline { name: name.to_vec(), phantom: 1, close: 1, kind: 2, nb_keys: n as u16, keys: (0..n).map(key).collect(), tension: 0.25, step: 12, old_points: false }
    }

    fn curve(n: usize) -> NurbsCurve {
        NurbsCurve {
            order: 3,
            nb_points: n as i32,
            points: (0..n).map(|i| NurbsPoint { xyzw: [i as f64, 0.5, -1.0, 1.0], flag: 0x6000_0000 + i as i32 }).collect(),
            closed: 1,
            uniformity: 2,
            nb_knots: n as i32 + 3,
            knots: (0..n + 3).map(|i| i as f64 * 0.5).collect(),
            start: 0.0,
            end: 2.5,
            display_steps: 7,
            closing: 2,
            key_framed: 1,
        }
    }

    fn mesh(uv_domains: i32) -> Mesh {
        let node = |v: i32| PolyNode {
            vertex: v,
            normal: [0.0, 0.0, 1.0],
            uv: [v as f32, 0.5],
            colour: 0xff10_2030,
            more_uv: (1..uv_domains).map(|d| [d as f32, v as f32]).collect(),
        };
        Mesh {
            flag: 0x80c,
            angle: 30.0,
            cos_angle: (30f32.to_radians()).cos(),
            nb_vertices: 4,
            vertices: (0..4).map(|i| Vertex { position: [i as f32, 1.0, 0.0], flag: (i % 2) as i16 }).collect(),
            nb_polygons: 2,
            polygons: vec![
                Polygon { normal: [0.0; 3], nb_nodes: 3, nodes: vec![node(0), node(1), node(2)], material: 1, flag: 8 },
                Polygon { normal: [0.0; 3], nb_nodes: 3, nodes: vec![node(2), node(1), node(3)], material: 0, flag: 0 },
            ],
            nb_edges: 5,
            edges: [[0, 1], [1, 2], [2, 0], [1, 3], [3, 2]].iter().map(|&vertex| Edge { vertex, flag: -32752 }).collect(),
            edge_field: true,
            normals_read: true,
            w_52: 1,
            w_50: 3,
            w_38: 1,
            f_3c: 2.5,
            f_40: 0.5,
            f_44: 30.0,
            w_48: 2,
            w_4a: 6,
            w_4c: 1,
            uv_domains,
        }
    }

    #[test]
    fn a_short_read_leaves_the_first_bytes_unswapped() {
        let mut v = [1.0f32, 2.0, 3.0];
        let whole = floats(&[4.0, 5.0, 6.0]);
        assert!(fread_vector(&mut Stream::new(&whole), &mut v));
        assert_eq!(v, [4.0, 5.0, 6.0]);
        // five bytes: they land on the first float and the second's low byte, as they came
        let mut v = [1.0f32, 2.0, 3.0];
        let mut s = Stream::new(&whole[..5]);
        assert!(!fread_vector(&mut s, &mut v));
        assert!(s.feof());
        assert_eq!(v[0].to_bits(), u32::from_le_bytes([0x40, 0x80, 0, 0]));
        assert_eq!(v[1].to_bits(), (2.0f32.to_bits() & 0xffff_ff00) | 0x40);
        assert_eq!(v[2], 3.0);
        // doubles: the whole items are swapped, the partial one is not
        let mut b: Vec<u8> = 1.5f64.to_bits().to_be_bytes().to_vec();
        b.extend_from_slice(&[0x40, 0x04, 0, 0]);
        let mut d = [9.0f64, 0.0, 7.0];
        assert_eq!(fread_doubles(&mut Stream::new(&b), &mut d), 1);
        assert_eq!((d[0], d[1].to_bits(), d[2]), (1.5, 0x0440, 7.0));
    }

    #[test]
    fn the_x87_rounds_a_product_twice() {
        // an exact product just above a tie of the double, which the 64-bit rounding puts on it
        let a = f32::from_bits(0x3f80_0001);
        let b = f64::from_bits(0x3ff0_0000_00c0_0001);
        assert_eq!((a as f64 * b).to_bits(), 0x3ff0_0000_20c0_0003);
        assert_eq!(x87_mul_to_double(a, b).to_bits(), 0x3ff0_0000_20c0_0002);
        assert_eq!(x87_mul_to_double(-a, b).to_bits(), 0xbff0_0000_20c0_0002);
        // the usual case is the plain product
        assert_eq!(x87_mul_to_double(60.0, 0.0174532925), 60.0 * 0.0174532925);
        assert_eq!(x87_mul_to_double(0.0, 3.0), 0.0);
        assert_eq!(x87_mul_to_double(60.0, 0.0174532925).cos() as f32, Mesh::default().cos_angle);
        // a signalling NaN comes out of an x87 register quiet
        assert_eq!(x87_copy(f64::from_bits(0x7ff0_0000_0000_0001)).to_bits(), 0x7ff8_0000_0000_0001);
        assert_eq!(x87_copy(-2.5), -2.5);
    }

    #[test]
    fn spline_round_trip_and_the_default_name() {
        let s = spline(b"curve", 3);
        let mut w = Writer::new();
        write_spline(&mut w, &s);
        // name, type, count, three keys of 42 bytes, close, tension, step, phantom
        assert_eq!(w.out.len(), 6 + 2 + 2 + 3 * 42 + 2 + 4 + 2 + 2);
        let p = protos(4.0);
        let mut st = GeomState::default();
        let mut r = Reader::new(&w.out);
        let (got, end) = read_spline(&mut r, &p, &mut st);
        assert_eq!((end, r.s.tell(), st.spline_count), (End::Zero, w.out.len(), 1));
        assert_eq!(got, s);
        // a prototype without the name: the allocator's name stays, numbered by the counter
        let mut p = Prototypes::new();
        p.slots[0] = Some(vec![1, 0]);
        let (got, _) = read_spline(&mut Reader::new(&[0, 4]), &p, &mut st);
        assert_eq!((got.name.as_slice(), got.kind, got.step, st.spline_count), (&b"spl2"[..], 4, 10, 2));
        assert_eq!(Spline::init().name, b"");
        // the type is written from its low 16 bits and a count of 0x8000 or more writes no key
        let mut odd = spline(b"", 2);
        odd.kind = 0x1_0003;
        odd.nb_keys = 0x8002;
        let mut w = Writer::new();
        write_spline(&mut w, &odd);
        assert_eq!(w.out, [&[0u8, 0, 3, 0x80, 2, 0, 1][..], &[0x3e, 0x80, 0, 0, 0, 12, 0, 1]].concat());
    }

    #[test]
    fn spline_fields_from_the_file() {
        // no prototypes: every field is preceded by its number. Type, an unknown field of
        // three bytes, one key with its position and flag, then the file ends
        let mut b = be(&[1, 3, 0x0309]);
        b.extend_from_slice(&[7, 7, 7]);
        b.extend(be(&[7, 1, 2]));
        b.extend(floats(&[1.0, 2.0, 3.0]));
        b.extend(be(&[6, 5, 0]));
        let p = Prototypes::new();
        let mut r = Reader::new(&b);
        let (s, end) = read_spline(&mut r, &p, &mut GeomState::default());
        // the key loop left a zero in the shared variable, so the missing closing zero of
        // the record is not noticed
        assert_eq!((end, r.s.tell()), (End::Zero, b.len()));
        assert_eq!((s.kind, s.nb_keys, s.keys.len()), (3, 1, 1));
        assert_eq!((s.keys[0].position, s.keys[0].flag), ([1.0, 2.0, 3.0], 5));
        // ending after a plain field leaves that field's number: the original never returns
        let b = be(&[3, 1]);
        let (s, end) = read_spline(&mut Reader::new(&b), &p, &mut GeomState::default());
        assert_eq!((end, s.close), (End::Eof, 1));
        // field 2, the old list of positions
        let mut b = be(&[2, 2]);
        b.extend(floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
        b.extend(be(&[0]));
        let mut r = Reader::new(&b);
        let (s, end) = read_spline(&mut r, &p, &mut GeomState::default());
        assert_eq!((end, r.s.tell(), s.old_points, s.keys[1].position), (End::Zero, b.len(), true, [4.0, 5.0, 6.0]));
        assert_eq!(s.keys[1].previous_tangent, [0.0; 3]);
        // a count that is negative as a short allocates and reads nothing
        let mut b = be(&[2, 0xffff]);
        b.extend(floats(&[1.0, 2.0, 3.0]));
        let mut r = Reader::new(&b);
        let (s, _) = read_spline(&mut r, &p, &mut GeomState::default());
        assert_eq!((s.nb_keys, s.keys.len(), s.keys[0].position, r.clamped), (0xffff, 1, [0.0; 3], true));
    }

    #[test]
    fn face_of_splines_and_face_of_curves() {
        let p = protos(4.0);
        let splines = Face { exterior: spline(b"out", 4), count: 2, holes: vec![spline(b"h1", 2), spline(b"h2", 3)], curves: Vec::new(), is_spline: 1 };
        let mut w = Writer::new();
        write_face(&mut w, &splines);
        assert_eq!(&w.out[..2], &[0, 1]);
        let mut st = GeomState::default();
        let mut r = Reader::new(&w.out);
        let (got, end) = read_face(&mut r, &p, &mut st);
        assert_eq!((end, r.s.tell(), st.spline_count), (End::Zero, w.out.len(), 3));
        assert_eq!(got, splines);
        let curves = Face { exterior: Spline::init(), count: 2, holes: Vec::new(), curves: vec![curve(4), curve(2)], is_spline: 0 };
        let mut w = Writer::new();
        write_face(&mut w, &curves);
        assert_eq!(w.out.len(), 4 + nurbs_curve_size(&curves.curves[0]) as usize + nurbs_curve_size(&curves.curves[1]) as usize);
        let mut r = Reader::new(&w.out);
        r.st.file_version = 4.0;
        let (got, end) = read_face(&mut r, &p, &mut st);
        assert_eq!((end, r.s.tell(), st.spline_count), (End::Zero, w.out.len(), 3));
        assert_eq!(got, curves);
        // the writer does not look at the version: for 2.9 it still starts with field 3,
        // which that version's prototype does not have
        let mut old = Writer::with_version(2.9);
        write_face(&mut old, &curves);
        assert_eq!(old.out, w.out);
        assert_eq!(output_lists(2.9).iter().find(|(id, _)| *id == 7).unwrap().1, [1, 2, 0]);
        // fields 1 and 2 read nothing in a face of curves, field 4 nothing in one of splines
        let mut q = Prototypes::new();
        q.slots[4] = Some(vec![4, 3, 1, 2, 0]);
        let b = be(&[0]);
        let mut r = Reader::new(&b);
        let (got, _) = read_face(&mut r, &q, &mut st);
        assert_eq!((got.is_spline, got.count, r.s.tell()), (0, 0, 2));
    }

    #[test]
    fn patch_round_trip_and_its_oddities() {
        let mut x = Patch {
            type_u: 2,
            type_v: 1,
            nb_u: 3,
            nb_v: 2,
            points: (0..6).map(|i| [i as f32, 0.5, -1.0, 1.0]).collect(),
            tags: vec![0, 1, 0, 0, 1, 1],
            close_u: 1,
            close_v: 0,
            tension_u: 0.5,
            tension_v: 0.25,
            step_u: 4,
            step_v: 5,
            curves_u: 2,
            curves_v: 3,
            u_bottom: 1,
            u_top: 0,
            v_bottom: 1,
            v_top: 1,
            faceted: 1,
            approx_type: 2,
            view_dependent: 1,
            max_edge_length: 2.0,
            curvature_u: 0.2,
            curvature_v: 30.0,
            min_recursion: 2,
            max_recursion: 5,
            recursion: 1,
        };
        let mut w = Writer::new();
        write_patch(&mut w, &x);
        assert_eq!(w.out.len(), 8 + 6 * 12 + 4 + 8 + 18 + 22 + 6 * 2);
        let mut r = Reader::new(&w.out);
        let (got, end) = read_patch(&mut r, &protos(4.0));
        assert_eq!((end, r.s.tell()), (End::Zero, w.out.len()));
        assert_eq!(got, x);
        // 2.65 as a float is above the double 2.65: the approximation and the tags are
        // written; for 2.64 they are not, and that version's prototype ends with field 16
        let len = |v: f32| {
            let mut w = Writer::with_version(v);
            write_patch(&mut w, &x);
            w.out.len()
        };
        assert_eq!((len(2.65), len(2.64)), (w.out.len(), w.out.len() - 34));
        let mut old = Writer::with_version(2.64);
        write_patch(&mut old, &x);
        let mut r = Reader::new(&old.out);
        let (got, _) = read_patch(&mut r, &protos(2.64));
        assert_eq!((r.s.tell(), got.faceted, got.approx_type, &got.tags[..]), (old.out.len(), 1, 1, &[0i16; 6][..]));
        // a close flag that is not zero is stored as 1; an unknown field is skipped as one
        // with a 32-bit size, although its size byte says two bytes
        let mut b = be(&[4, 7, 0x0263, 0, 3]);
        b.extend_from_slice(&[9, 9, 9]);
        b.extend(be(&[2, 4, 0]));
        let mut r = Reader::new(&b);
        let (got, end) = read_patch(&mut r, &Prototypes::new());
        assert_eq!((end, r.s.tell(), got.close_u, got.type_v), (End::Zero, b.len(), 1, 4));
        // a product of the counts of 2^31 or more counts as no point
        x.nb_u = 0xffff;
        x.nb_v = 0xffff;
        assert!(x.point_count() < 0);
        let mut w = Writer::new();
        write_patch(&mut w, &x);
        assert_eq!(w.out.len(), 8 + 4 + 8 + 18 + 22);
    }

    #[test]
    fn mesh_round_trip_and_version_branches() {
        for uv in [1, 3] {
            let m = mesh(uv);
            let mut w = Writer::new();
            write_mesh(&mut w, &m, uv);
            let nodes = 6;
            let base = 2 + 4 + 4 + 4 * 14 + 4 + 2 * (2 + 2) + nodes * 28 + 4 + 5 * 10;
            assert_eq!(w.out.len(), base + 4 + nodes * (4 + 8 * (uv as usize - 1)) + 2 * 2 + 24);
            let mut st = GeomState::default();
            let mut r = Reader::new(&w.out);
            r.st.file_version = 4.0;
            let (got, end) = read_mesh(&mut r, &protos(4.0), &mut st);
            assert_eq!((end, r.s.tell(), st.uv_domains), (End::Zero, w.out.len(), uv));
            // the cosine is the reader's own
            assert!((got.cos_angle - m.cos_angle).abs() < 1e-6);
            assert_eq!(Mesh { cos_angle: m.cos_angle, ..got }, m);
            // the same through the two entry points
            let mut w2 = Writer::new();
            write_geometry(&mut w2, &Geometry::Mesh(m.clone()));
            assert_eq!(w2.out, w.out);
            let len = |v: f32| {
                let mut w = Writer::with_version(v);
                write_mesh(&mut w, &m, uv);
                w.out.len()
            };
            assert_eq!(len(3.93), w.out.len());
            assert_eq!(len(3.9), base + 2 * 2 + 24);
            assert_eq!(len(3.8), base + 2 * 2);
            assert_eq!(len(2.65), base + 2 * 2);
            assert_eq!(len(2.64), base);
            // each of those reads back with the prototypes written for the same version
            for v in [3.9f32, 3.8, 2.64] {
                let mut w = Writer::with_version(v);
                write_mesh(&mut w, &m, uv);
                let mut r = Reader::new(&w.out);
                r.st.file_version = v;
                let (got, end) = read_mesh(&mut r, &protos(v), &mut GeomState::default());
                assert_eq!((end, r.s.tell()), (End::Zero, w.out.len()), "version {v}");
                assert_eq!(got.polygons[1].nodes[2].vertex, 3);
            }
        }
        // a node with fewer pairs than the model has domains is filled up with zeros
        let mut m = mesh(1);
        m.nb_polygons = 0;
        m.nb_vertices = 0;
        m.nb_edges = 0;
        let mut w = Writer::new();
        write_mesh(&mut w, &m, 2);
        assert_eq!(&w.out[..10], &[0x08, 0x0c, 0x41, 0xf0, 0, 0, 0, 0, 0, 2]);
    }

    #[test]
    fn mesh_old_fields_from_the_file() {
        // faceted; one vertex under the 16-bit count; one polygon with one node whose vertex
        // index is a short; one edge with 16-bit vertices and the two fields that hold nothing
        let mut b = be(&[1, 1, 3, 1, 1]);
        b.extend(floats(&[1.0, 2.0, 3.0]));
        b.extend(be(&[4, 9, 0]));
        b.extend(be(&[4, 1, 2, 1, 1, 0xfffe, 0, 3, 7, 0]));
        b.extend(be(&[5, 1, 1, 3, 0xffff, 2, 5, 3, 0x8010, 0]));
        b.extend(be(&[0]));
        let mut r = Reader::new(&b);
        let (m, end) = read_mesh(&mut r, &Prototypes::new(), &mut GeomState::default());
        assert_eq!((end, r.s.tell()), (End::Zero, b.len()));
        assert_eq!((m.flag, m.nb_vertices, m.nb_polygons, m.nb_edges), (0x806, 1, 1, 1));
        assert_eq!(m.vertices, [Vertex { position: [1.0, 2.0, 3.0], flag: 9 }]);
        assert_eq!((m.polygons[0].nb_nodes, m.polygons[0].nodes[0].vertex, m.polygons[0].material), (1, -2, 7));
        assert_eq!(m.polygons[0].nodes[0].colour, 0xffb2_b2b2);
        assert_eq!(m.edges, [Edge { vertex: [3, -1], flag: -32752 }]);
        assert!(m.edge_field && !m.normals_read);
        // the angle's cosine
        let mut b = be(&[2]);
        b.extend(floats(&[90.0]));
        b.extend(be(&[0]));
        let (m, _) = read_mesh(&mut Reader::new(&b), &Prototypes::new(), &mut GeomState::default());
        assert!((m.cos_angle - 1.7948966e-9).abs() < 1e-14, "{}", m.cos_angle);
        // after a list the shared variable is zero: the end of the file ends the record;
        // after a plain field it does not
        let mut b = be(&[7, 0, 1, 1]);
        b.extend(floats(&[1.0, 2.0, 3.0]));
        b.extend(be(&[0]));
        let (m, end) = read_mesh(&mut Reader::new(&b), &Prototypes::new(), &mut GeomState::default());
        assert_eq!((end, m.vertices.len()), (End::Zero, 1));
        let (m, end) = read_mesh(&mut Reader::new(&be(&[6, 4])), &Prototypes::new(), &mut GeomState::default());
        assert_eq!((end, m.flag), (End::Eof, 4));
        // counts beyond the file
        let mut r = Reader::new(&[0, 7, 0x7f, 0xff, 0xff, 0xff, 0, 0]);
        let (m, _) = read_mesh(&mut r, &Prototypes::new(), &mut GeomState::default());
        assert_eq!((m.nb_vertices, m.vertices.len(), r.clamped), (0x7fff_ffff, 1, true));
    }

    #[test]
    fn node_colours_and_uv_domains() {
        // one polygon, one node: the colour, then two further UV pairs after the model's
        // count was set to three by field 0x413
        let mut b = be(&[0x413, 0, 3, 8, 0, 1, 2, 1, 0x405, 0x0011, 0x2233, 0xff06, 0, 16]);
        b.extend(floats(&[1.0, 2.0, 3.0, 4.0]));
        b.extend(be(&[0, 0, 0]));
        let read = |version: f32| {
            let mut r = Reader::new(&b);
            r.st.file_version = version;
            let mut st = GeomState::default();
            let (m, end) = read_mesh(&mut r, &Prototypes::new(), &mut st);
            assert_eq!((end, r.s.tell(), st.uv_domains, m.uv_domains), (End::Zero, b.len(), 3, 3));
            m.polygons[0].nodes[0].clone()
        };
        let n = read(3.71);
        assert_eq!((n.colour, &n.more_uv[..]), (0x0011_2233, &[[1.0, 2.0], [3.0, 4.0]][..]));
        // a file older than 3.71 gets the colour's top byte set
        assert_eq!(read(3.704).colour, 0xff11_2233);
        // nodes are equal by their bits
        let nan = PolyNode { uv: [f32::NAN, 0.0], ..PolyNode::default() };
        assert_eq!(nan, nan.clone());
        assert_ne!(nan, PolyNode { uv: [f32::from_bits(0x7fc0_0001), 0.0], ..PolyNode::default() });
    }

    #[test]
    fn nurbs_curve_and_the_point_flags() {
        let c = curve(3);
        let mut w = Writer::new();
        write_nurbs_curve(&mut w, &c);
        assert_eq!(w.out.len(), nurbs_curve_size(&c) as usize);
        assert_eq!(w.out.len(), 44 + 3 * 36 + 6 * 8);
        let p = protos(4.0);
        let mut r = Reader::new(&w.out);
        r.st.file_version = 2.61;
        let (got, end) = read_nurbs_curve(&mut r, &p);
        assert_eq!((end, r.s.tell()), (End::Zero, w.out.len()));
        assert_eq!(got, c);
        // a file below 2.6 has no flags: the same bytes without them. The float 2.6 is
        // below the double 2.6, so a file marked 2.6 is such a file.
        let mut b = w.out[..8].to_vec();
        for i in 0..3 {
            b.extend_from_slice(&w.out[8 + 36 * i..8 + 36 * i + 32]);
        }
        b.extend_from_slice(&w.out[8 + 3 * 36..]);
        for version in [2.59f32, 2.6] {
            let mut r = Reader::new(&b);
            r.st.file_version = version;
            let (got, end) = read_nurbs_curve(&mut r, &p);
            assert_eq!((end, r.s.tell()), (End::Zero, b.len()));
            assert_eq!((got.points[2].xyzw, got.points[2].flag, got.key_framed), (c.points[2].xyzw, 0, 1));
        }
        // field 8 is the key framed flag too, and a negative knot count takes the rest
        let mut q = Prototypes::new();
        q.slots[25] = Some(vec![8, 4, 1, 0]);
        let mut r = Reader::new(&[0, 1, 0, 0, 0, 2, 0xff, 0xff, 0xff, 0xff, 1, 2, 3]);
        let (got, _) = read_nurbs_curve(&mut r, &q);
        assert_eq!((got.key_framed, got.uniformity, got.nb_knots, got.knots.len(), got.order), (1, 2, -1, 0, 4));
        assert!(r.clamped && r.s.feof());
        // a signalling NaN is written quiet
        let mut odd = curve(1);
        odd.points[0].xyzw[1] = f64::from_bits(0x7ff0_0000_0000_0001);
        let mut w = Writer::new();
        write_nurbs_curve(&mut w, &odd);
        assert_eq!(&w.out[16..24], &0x7ff8_0000_0000_0001u64.to_be_bytes());
    }

    #[test]
    fn nurbs_surface_with_trim_and_projected_curves() {
        let comp = |n: usize| NurbsComposite { is_boundary: 1, nb_curves: n as i32, curves: (0..n).map(|i| curve(i + 2)).collect() };
        let s = NurbsSurface {
            order: [4, 3],
            nb_points: [3, 2],
            point_count: 6,
            points: (0..6).map(|i| NurbsPoint { xyzw: [i as f64, 1.0, 2.0, 1.0], flag: i }).collect(),
            closed: [0, 1],
            uniformity: 2,
            nb_knots: [7, 5],
            knots: [(0..7).map(|i| i as f64).collect(), (0..5).map(|i| i as f64 * 0.25).collect()],
            range: [0.0, 4.0, 0.0, 1.0],
            border: 1,
            nb_trims: 2,
            trims: vec![comp(2), comp(1)],
            nb_projected: 1,
            projected: vec![comp(2)],
            display_steps: [5, 6],
            wire_curves: [2, 3],
            capping: [1, 0, 1, 0],
            closing: [2, 0],
            approx_method: 2,
            view_dependency: 1,
            max_edge_length: 1.5,
            curvature: [0.5, 20.0],
            min_recursion: 2,
            max_recursion: 4,
            recursion: 1,
        };
        let mut w = Writer::new();
        write_nurbs_surface(&mut w, &s);
        // the size in front of the projected curves counts itself... not: it counts the
        // count word and the curves
        let projected = nurbs_composite_size(&s.projected[0]) as usize;
        let at = w.out.len() - projected - 8;
        assert_eq!(&w.out[at..at + 8], &[&(projected as u32 + 4).to_be_bytes()[..], &[0, 0, 0, 1]].concat()[..]);
        let p = protos(4.0);
        let mut r = Reader::new(&w.out);
        r.st.file_version = 4.0;
        let (got, end) = read_nurbs_surface(&mut r, &p);
        assert_eq!((end, r.s.tell()), (End::Zero, w.out.len()));
        assert_eq!(got, s);
        // under the plain number 0x1a the projected curves have no size in front
        let mut q = Prototypes::new();
        q.slots[26] = Some(vec![0x1a, 0]);
        q.slots[27] = p.slots[27].clone();
        q.slots[25] = p.slots[25].clone();
        let mut r = Reader::new(&w.out[at + 4..]);
        r.st.file_version = 4.0;
        let (got, _) = read_nurbs_surface(&mut r, &q);
        assert_eq!((r.s.tell(), &got.projected), (projected + 4, &s.projected));
        // a composite curve skips an unknown field as a long one
        let b = be(&[0x0105, 0, 2, 0x0909, 1, 1]);
        let mut r = Reader::new(&b);
        let (c, end) = read_nurbs_composite(&mut r, &Prototypes::new());
        assert_eq!((c.is_boundary, end), (1, End::Eof));
        // the dispatch by model type
        for (t, g) in [(10, Geometry::NurbsSurface(s.clone())), (9, Geometry::NurbsCurve(curve(2))), (6, Geometry::Spline(spline(b"s", 2)))] {
            let mut w = Writer::new();
            write_geometry(&mut w, &g);
            let mut r = Reader::new(&w.out);
            r.st.file_version = 4.0;
            let (got, end) = read_geometry(&mut r, &p, t);
            assert_eq!((got.as_ref(), end, r.s.tell(), g.model_type()), (Some(&g), End::Zero, w.out.len(), t));
        }
        // a type with no geometry skips one long field
        let mut r = Reader::new(&[0, 0, 0, 2, 1, 2, 3]);
        assert_eq!(read_geometry(&mut r, &p, 5), (None, End::Zero));
        assert_eq!(r.s.tell(), 6);
        assert!(Geometry::allocate(5, &mut GeomState::default()).is_none());
        assert_eq!(Geometry::allocate(4, &mut GeomState::default()), Some(Geometry::Mesh(Mesh::default())));
    }
}
