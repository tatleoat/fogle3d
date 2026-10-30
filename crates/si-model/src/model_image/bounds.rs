// SPDX-License-Identifier: MIT
//! The bounding boxes of a node's geometry, on the image.
//!
//! Four routines of `model.c` walk the points of a definition, whatever its kind, and keep
//! per axis the smallest and the largest value, and two radii: the largest distance from
//! the Y axis and the largest from the origin. They were written from the instruction
//! listing, because what is rounded where is not in any source line:
//!
//! - a coordinate is compared with the box as the unit compares (an unordered comparison
//!   keeps nothing, so a NaN never enters a box) and stored as a `float`;
//! - the two radii are running maxima of **squared** distances. `x² + z²` is formed in the
//!   register, compared, and `y²` added to the same register value; the maxima stay in the
//!   register at 80 bits across the whole walk (one of them is a `float` in memory in
//!   [`compute_bounding_el`]) and are rounded once, after the square root;
//! - a NURBS point is x, y, z each times 1/w, the quotient kept at 80 bits and each product
//!   rounded to `float` before anything else is done with it;
//! - where a point goes through a matrix, each coordinate is
//!   `((x·m0j + y·m1j) + z·m2j) + m3j` in the register. [`hierarchy_bounding`] compares the
//!   first coordinate before it is rounded and the others after; [`compute_bounding_el`]
//!   puts the unrounded result through a second matrix.
//!
//! The geometry records are read by named offsets ([`geo`]): these routines index the point
//! arrays by byte counts of their own, as [`super::points`] does.

use si_math::types::Matrix;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::{x, F80};

use super::{at, mat, mx, tree};

/// The offsets these routines read, by record.
pub(crate) mod geo {
    /// `SI_Mesh`: the vertices (0x14 bytes each, the position first) and their count.
    pub const MESH_VERTICES: u32 = 0x00;
    pub const MESH_NB_VERTICES: u32 = 0x14;
    /// `SI_Patch`: the two 16-bit point counts and the control points (16 bytes each).
    pub const PATCH_NU: u32 = 0x0c;
    pub const PATCH_NV: u32 = 0x0e;
    pub const PATCH_POINTS: u32 = 0x1a4;
    /// `SI_Patch`: the display curves running in U and in V (finding 0020): the points
    /// (12 bytes each) and two 16-bit counts whose product is their number.
    pub const PATCH_CURVES_U: u32 = 0x1b0;
    pub const PATCH_CURVES_U_COUNTS: u32 = 0x1b4;
    pub const PATCH_CURVES_V: u32 = 0x1b8;
    pub const PATCH_CURVES_V_COUNTS: u32 = 0x1bc;
    /// `SI_Face`: the outline (an `SI_Spline` inside the record), the number of holes or of
    /// NURBS curves (16 bits, signed), the holes (`SI_Spline`, 0x74 bytes each), the NURBS
    /// curves (`__SI_NurbsCrv`, 100 bytes each), and the 16-bit word that is zero for a
    /// face of NURBS curves.
    pub const FACE_OUTLINE: u32 = 0x04;
    pub const FACE_NB_HOLES: u32 = 0x78;
    pub const FACE_HOLES: u32 = 0x7c;
    pub const FACE_CURVES: u32 = 0x80;
    pub const FACE_SPLINE: u32 = 0x84;
    /// `SI_Spline`: the type (1: a key carries two more points, before and after its
    /// position), the key count (16 bits) and the keys (0x30 bytes: a point at +0, the
    /// position at +0xc, a point at +0x18).
    pub const SPLINE_TYPE: u32 = 0x0c;
    pub const SPLINE_NB_KEYS: u32 = 0x10;
    pub const SPLINE_KEYS: u32 = 0x14;
    /// The record of a kind-8 node (a metaball): its radius, a double.
    pub const BALL_RADIUS: u32 = 0xcc;
    /// `__SI_NurbsCrv`: the point count and the points (0x24 bytes: x, y, z, w as doubles).
    pub const CRV_NB_POINTS: u32 = 0x04;
    pub const CRV_POINTS: u32 = 0x2c;
    /// `SI_NurbsSrf`: the point count and the points.
    pub const SRF_NB_POINTS: u32 = 0x48;
    pub const SRF_POINTS: u32 = 0x54;
}

/// The start values of a box before any point: `.rodata` 0x0865fed0 and 0x0865fed4 (each
/// routine has its own copy of the pair).
const BOX_MIN_START: f32 = 1.0e10;
const BOX_MAX_START: f32 = -1.0e10;

/// `if (v < *dst) *dst = v`: the unit's comparison of the register value with the float
/// in memory (nothing is kept when it is unordered), the store rounds to `float`.
fn keep_min(m: &mut Mem, dst: u32, v: F80) {
    if v.lt(x(m.f32(dst))) {
        m.set_f32(dst, v.to_f32());
    }
}

/// `if (v > *dst) *dst = v`, likewise.
fn keep_max(m: &mut Mem, dst: u32, v: F80) {
    if v.gt(x(m.f32(dst))) {
        m.set_f32(dst, v.to_f32());
    }
}

/// The two running maxima of squared distances.
struct Radii {
    /// The largest `x² + z²` so far (the distance from the Y axis, squared), at 80 bits.
    axis: F80,
    /// The largest `(x² + z²) + y²` so far, at 80 bits.
    origin: F80,
}

impl Radii {
    fn new() -> Radii {
        Radii { axis: F80::ZERO, origin: F80::ZERO }
    }

    /// One point: `x² + z²` against the first maximum, then the same register value plus
    /// `y²` against the second.
    fn point(&mut self, px: f32, pz: f32, py: f32) {
        let t = x(px) * x(px) + x(pz) * x(pz);
        if t.gt(self.axis) {
            self.axis = t;
        }
        let d = t + x(py) * x(py);
        if d.gt(self.origin) {
            self.origin = d;
        }
    }
}

/// Where a box is kept: the addresses of its minimum and of its maximum (three floats
/// each).
#[derive(Clone, Copy)]
struct BoxAt {
    min: u32,
    max: u32,
}

impl BoxAt {
    /// The box before any point: the minimum three times 1e10, then the maximum three
    /// times −1e10.
    fn start(self, m: &mut Mem) {
        for i in 0..3 {
            m.set_f32(self.min + 4 * i, BOX_MIN_START);
        }
        for i in 0..3 {
            m.set_f32(self.max + 4 * i, BOX_MAX_START);
        }
    }

    /// The box of no points: six zeros.
    fn zero(self, m: &mut Mem) {
        for i in (0..3).rev() {
            m.set_u32(self.min + 4 * i, 0);
        }
        for i in (0..3).rev() {
            m.set_u32(self.max + 4 * i, 0);
        }
    }

    /// One point into the box and the radii, as [`compute_bounding`] and
    /// [`compute_bounding_cp`] do it: per axis the minimum and then the maximum, then the
    /// radii. A point in memory has each coordinate read again for every comparison, and
    /// its address asked again (the original reads the array's pointer again before most
    /// of the loads).
    fn point(self, m: &mut Mem, r: &mut Radii, pt: Pt) {
        match pt {
            Pt::Memory(at) => {
                for i in 0..3u32 {
                    let v = m.f32(at(m) + 4 * i);
                    keep_min(m, self.min + 4 * i, x(v));
                    let v = m.f32(at(m) + 4 * i);
                    keep_max(m, self.max + 4 * i, x(v));
                }
                let p = at(m);
                r.point(m.f32(p), m.f32(p + 8), m.f32(p + 4));
            }
            Pt::Value(p) => {
                for i in 0..3 {
                    keep_min(m, self.min + 4 * i as u32, x(p[i]));
                    keep_max(m, self.max + 4 * i as u32, x(p[i]));
                }
                r.point(p[0], p[2], p[1]);
            }
        }
    }
}

/// A point of a definition as the walk hands it on: three floats in memory, at the
/// address the closure answers when it is asked, or three floats of the routine's own (a
/// NURBS point after its division by w).
enum Pt<'a> {
    Memory(&'a dyn Fn(&Mem) -> u32),
    Value([f32; 3]),
}

impl Pt<'_> {
    /// The three coordinates, each loaded once.
    fn load(&self, m: &Mem) -> [f32; 3] {
        match self {
            Pt::Memory(at) => {
                let a = at(m);
                [m.f32(a), m.f32(a + 4), m.f32(a + 8)]
            }
            Pt::Value(p) => *p,
        }
    }
}

/// A NURBS point as three floats: 1/w is formed first and kept at 80 bits; x, y and z are
/// each multiplied by it and rounded to `float`. `points` answers the address of the
/// array (read again before each coordinate), `offset` is the point's.
fn nurbs_point(m: &Mem, points: &dyn Fn(&Mem) -> u32, offset: u32) -> [f32; 3] {
    let p = points(m).wrapping_add(offset);
    let r = x(1.0f32) / x(m.f64(p + 0x18));
    let px = (x(m.f64(p)) * r).to_f32();
    let p = points(m).wrapping_add(offset);
    let py = (x(m.f64(p + 8)) * r).to_f32();
    let p = points(m).wrapping_add(offset);
    let pz = (r * x(m.f64(p + 0x10))).to_f32();
    [px, py, pz]
}

/// How one of the four routines walks a definition's points.
#[derive(Clone, Copy)]
struct Walk {
    /// A patch gives its display curves (those running in U, then those running in V)
    /// instead of its control points.
    patch_curves: bool,
    /// A key of a type-1 spline gives the point before it, its position and the point
    /// after it, in that order; otherwise the position comes first.
    in_key_order: bool,
    /// The spline's type is read again at every key instead of once before the keys.
    type_per_key: bool,
}

type Each<'a> = &'a mut dyn FnMut(&mut Mem, Pt);

/// The keys of a spline that has at least one.
fn spline_keys(m: &mut Mem, w: Walk, spline: u32, each: Each) {
    let wide = m.i32(spline + geo::SPLINE_TYPE) == 1;
    let mut key = m.u32(spline + geo::SPLINE_KEYS);
    let mut i = 0;
    loop {
        let position = key.wrapping_add(0xc);
        let (before, after) = (key, key.wrapping_add(0x18));
        if w.in_key_order {
            if wide {
                each(m, Pt::Memory(&|_| before));
                each(m, Pt::Memory(&|_| position));
                each(m, Pt::Memory(&|_| after));
            } else {
                each(m, Pt::Memory(&|_| position));
            }
        } else {
            each(m, Pt::Memory(&|_| position));
            // (read after the position went in, where it is read at every key)
            let wide = if w.type_per_key { m.i32(spline + geo::SPLINE_TYPE) == 1 } else { wide };
            if wide {
                each(m, Pt::Memory(&|_| before));
                each(m, Pt::Memory(&|_| after));
            }
        }
        i += 1;
        key = key.wrapping_add(0x30);
        if i >= m.u16(spline + geo::SPLINE_NB_KEYS) as i32 {
            break;
        }
    }
}

/// The NURBS points of an array with a count, from the first while the index is below
/// the count (read again after every point); the caller has seen to it that there is one.
fn nurbs_points(m: &mut Mem, record: u32, count_at: u32, points_at: u32, each: Each) {
    let mut i = 0;
    loop {
        let p = nurbs_point(m, &|m| m.u32(record + points_at), (i as u32).wrapping_mul(0x24));
        each(m, Pt::Value(p));
        i += 1;
        if i >= m.i32(record + count_at) {
            break;
        }
    }
}

/// Every point of the definition `def` of `kind` (1 a patch, 2 a face, 4 a mesh, 6 a
/// spline, 9 a NURBS curve, 10 a NURBS surface), in the original's order, handed to
/// `each`. The answer is true when the definition has no points: a mesh whose vertex
/// count is 0 or less, a patch whose count (a product of two 16-bit words, taken as a
/// signed number) is 0 or less, a face whose contours are all empty or whose 16-bit
/// number of holes is negative, a spline without keys, a NURBS record with a count of 0
/// or less. Any other kind has no points and answers false.
fn walk(m: &mut Mem, w: Walk, kind: i32, def: u32, each: Each) -> bool {
    match kind {
        4 => {
            if m.i32(def + geo::MESH_NB_VERTICES) <= 0 {
                return true;
            }
            let mut i = 0;
            loop {
                let off = (i as u32).wrapping_mul(0x14);
                each(m, Pt::Memory(&|m| m.u32(def + geo::MESH_VERTICES).wrapping_add(off)));
                i += 1;
                if i >= m.i32(def + geo::MESH_NB_VERTICES) {
                    break;
                }
            }
            false
        }
        1 if w.patch_curves => {
            let count = |m: &Mem, at: u32| (m.u16(def + at) as u32).wrapping_mul(m.u16(def + at + 2) as u32) as i32;
            if count(m, geo::PATCH_CURVES_U_COUNTS) <= 0 && count(m, geo::PATCH_CURVES_V_COUNTS) <= 0 {
                return true;
            }
            for (points, counts) in [(geo::PATCH_CURVES_U, geo::PATCH_CURVES_U_COUNTS), (geo::PATCH_CURVES_V, geo::PATCH_CURVES_V_COUNTS)] {
                let mut i = 0;
                while i < count(m, counts) {
                    let off = (i as u32).wrapping_mul(0xc);
                    each(m, Pt::Memory(&|m| m.u32(def + points).wrapping_add(off)));
                    i += 1;
                }
            }
            false
        }
        1 => {
            // the count is taken once, and the array's address once
            let count = (m.u16(def + geo::PATCH_NU) as u32).wrapping_mul(m.u16(def + geo::PATCH_NV) as u32) as i32;
            if count <= 0 {
                return true;
            }
            let mut point = m.u32(def + geo::PATCH_POINTS);
            for _ in 0..count {
                each(m, Pt::Memory(&|_| point));
                point = point.wrapping_add(0x10);
            }
            false
        }
        2 => {
            let mut any = false;
            if m.i16(def + geo::FACE_SPLINE) != 0 {
                // the outline (an `SI_Spline` inside the record), then each hole; nothing
                // at all when the 16-bit number of holes is negative
                if m.u8(def + geo::FACE_NB_HOLES + 1) & 0x80 == 0 {
                    let mut hole: i32 = -1;
                    loop {
                        let contour = if hole == -1 { def + geo::FACE_OUTLINE } else { (hole as u32).wrapping_mul(0x74).wrapping_add(m.u32(def + geo::FACE_HOLES)) };
                        hole += 1;
                        if m.u16(contour + geo::SPLINE_NB_KEYS) != 0 {
                            any = true;
                            spline_keys(m, w, contour, each);
                        }
                        if hole >= m.i16(def + geo::FACE_NB_HOLES) as i32 {
                            break;
                        }
                    }
                }
            } else if m.i16(def + geo::FACE_NB_HOLES) > 0 {
                let mut curve: i32 = 0;
                loop {
                    let at = (curve as u32).wrapping_mul(100).wrapping_add(m.u32(def + geo::FACE_CURVES));
                    curve += 1;
                    if m.i32(at + geo::CRV_NB_POINTS) > 0 {
                        any = true;
                        nurbs_points(m, at, geo::CRV_NB_POINTS, geo::CRV_POINTS, each);
                    }
                    if curve >= m.i16(def + geo::FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            }
            !any
        }
        6 => {
            if m.u16(def + geo::SPLINE_NB_KEYS) == 0 {
                return true;
            }
            spline_keys(m, w, def, each);
            false
        }
        9 | 10 => {
            let (count, points) = if kind == 9 { (geo::CRV_NB_POINTS, geo::CRV_POINTS) } else { (geo::SRF_NB_POINTS, geo::SRF_POINTS) };
            if m.i32(def + count) <= 0 {
                return true;
            }
            nurbs_points(m, def, count, points, each);
            false
        }
        _ => false,
    }
}

/// The square roots of the two maxima into the node (+0x28: the distance from the Y axis,
/// +0x2c: from the origin), each rounded to `float` on its store, and bit 0 of the flag
/// byte at +8.
fn store_radii(m: &mut Mem, node: Ptr32<Model>, r: &Radii) {
    let n = node.addr();
    m.set_f32(n + 0x28, r.axis.sqrt().to_f32());
    let flags = m.u8(n + 8);
    m.set_u8(n + 8, flags | 1);
    m.set_f32(n + 0x2c, r.origin.sqrt().to_f32());
}

/// The node's current stored definition: `defs[cur_def]`, neither tested.
fn stored_definition(m: &Mem, node: Ptr32<Model>) -> u32 {
    let n = node.addr();
    m.u32(m.u32(n + 0x38).wrapping_add(4 * m.u16(n + 0x36) as u32))
}

/// The box and the two radii of a node's **working** definition, in the node's own space,
/// stored in the node [soft 0x08467e70 SI_modelComputeBounding]: the box at +0x10 and
/// +0x1c, the radii at +0x28 and +0x2c, bit 0 of the flag byte at +8 set.
///
/// By kind: a mesh's vertices; a patch's display curves, those running in U and then
/// those running in V (not its control points); a face's outline and holes, or its NURBS
/// curves; a spline's keys (with the two points beside each key for a type-1 spline); a
/// metaball (kind 8): the box is ± its radius and both radii are the radius (the square
/// rounded to `float`, then the root); a NURBS curve's or surface's points.
///
/// **A definition without points gives a box of six zeros** (see [`walk`]), and so does
/// kind 0. Kinds 3 and 7 and any kind outside 0 ..= 10 leave the start values in the box
/// (1e10 and −1e10) and radii of 0. **Kind 5 (a joint) is left alone altogether**: no
/// field of the node is written. The definition pointer is not tested.
pub fn compute_bounding(m: &mut Mem, node: Ptr32<Model>) {
    let n = node.addr();
    let b = BoxAt { min: n + 0x10, max: n + 0x1c };
    let mut r = Radii::new();
    let kind = m[node].kind;
    if kind == 5 {
        return;
    }
    b.start(m);
    let def = m.u32(n + 0x48);
    let empty = match kind {
        0 => true,
        8 => {
            // each of the six is its own load of the double, the minimum negated in the
            // register; the two maxima start from the square rounded to float
            let radius = |m: &Mem| x(m.f64(def + geo::BALL_RADIUS));
            for i in 0..3 {
                let v = (-radius(m)).to_f32();
                m.set_f32(b.min + 4 * i, v);
            }
            for i in 0..3 {
                let v = radius(m).to_f32();
                m.set_f32(b.max + 4 * i, v);
            }
            let square = x((radius(m) * radius(m)).to_f32());
            r = Radii { axis: square, origin: square };
            false
        }
        _ => {
            let w = Walk { patch_curves: true, in_key_order: false, type_per_key: true };
            walk(m, w, kind, def, &mut |m, pt| b.point(m, &mut r, pt))
        }
    };
    if empty {
        b.zero(m);
    }
    store_radii(m, node, &r);
}

/// The box of a node's **control points**: the points of its current stored definition
/// (`defs[cur_def]`), with the box at `min` and `max` (three floats each) and the two
/// radii in the node as [`compute_bounding`] leaves them (+0x28, +0x2c, bit 0 of the flag
/// byte at +8) [soft 0x08468b54 SI_modelComputeBoundingCP].
///
/// As [`compute_bounding`] but for: the definition; a patch gives its control points;
/// a metaball (kind 8) gives nothing (start values in the box, radii of 0). Kind 5 is
/// left alone altogether, the caller's box included.
pub fn compute_bounding_cp(m: &mut Mem, node: Ptr32<Model>, min: u32, max: u32) {
    let b = BoxAt { min, max };
    let mut r = Radii::new();
    let kind = m[node].kind;
    if kind == 5 {
        return;
    }
    b.start(m);
    let empty = match kind {
        0 => true,
        1 | 2 | 4 | 6 | 9 | 10 => {
            let def = stored_definition(m, node);
            let w = Walk { patch_curves: false, in_key_order: false, type_per_key: false };
            walk(m, w, kind, def, &mut |m, pt| b.point(m, &mut r, pt))
        }
        _ => false,
    };
    if empty {
        b.zero(m);
    }
    store_radii(m, node, &r);
}

/// A point through a matrix, in the register: `((x·m0j + y·m1j) + z·m2j) + m3j` for
/// j = 0, 1, 2, nothing rounded.
fn through(p: [F80; 3], mm: &Matrix) -> [F80; 3] {
    let mut out = [F80::ZERO; 3];
    for (j, o) in out.iter_mut().enumerate() {
        *o = ((p[0] * mm[0][j] + p[1] * mm[1][j]) + p[2] * mm[2][j]) + mm[3][j];
    }
    out
}

/// The box of a whole tree in the space of its top node, at `min` and `max` (three floats
/// each) [soft 0x08469878 SI_modelHierarchyBounding]. Nothing is stored in any node.
///
/// The top node's global matrix is inverted once; then, for the top node and every node
/// below it (`SI_modelNextSon`), the points of the node's **working** definition go
/// through `gmat` of the node its geometry depends on (`latdep`) times that inverse. A
/// patch gives its control points, a type-1 spline its keys' three points in the key's
/// own order; a null (kind 0) and a joint (kind 5) give the origin; kinds 3, 7, 8 and
/// any other give nothing. There are no radii.
///
/// Of a point through the matrix, the first coordinate is compared with the box's
/// minimum **before** it is rounded to `float` (and stored rounded); every other
/// comparison is of the rounded coordinate. The three minima are kept first, then the
/// three maxima.
///
/// **A node whose definition has no points zeroes the whole box**, also what earlier
/// nodes put there; the walk goes on from the zeros. A top node whose matrix has no
/// inverse leaves the inverse as the stack had it (counted, [`crate::env::stale`]).
pub fn hierarchy_bounding(m: &mut Mem, root: Ptr32<Model>, min: u32, max: u32) {
    let b = BoxAt { min, max };
    b.start(m);
    let inverse = m.local(0x40);
    let local = m.local(0x40);
    if !mx::inverse(m, at!(root, gmat), inverse) {
        crate::env::stale::note();
    }
    let mut node = root;
    loop {
        let latdep = m[node].latdep;
        mx::multiply(m, at!(latdep, gmat), inverse, local);
        let kind = m[node].kind;
        let mut put = |m: &mut Mem, pt: Pt| {
            let p = pt.load(m);
            let v = through([x(p[0]), x(p[1]), x(p[2])], &mat(m, local));
            keep_min(m, b.min, v[0]);
            let v = [v[0].to_f32(), v[1].to_f32(), v[2].to_f32()];
            keep_min(m, b.min + 4, x(v[1]));
            keep_min(m, b.min + 8, x(v[2]));
            for i in 0..3 {
                keep_max(m, b.max + 4 * i as u32, x(v[i]));
            }
        };
        let empty = match kind {
            0 | 5 => {
                put(m, Pt::Value([0.0; 3]));
                false
            }
            1 | 2 | 4 | 6 | 9 | 10 => {
                let def = m.u32(node.addr() + 0x48);
                let w = Walk { patch_curves: false, in_key_order: true, type_per_key: false };
                walk(m, w, kind, def, &mut put)
            }
            _ => false,
        };
        if empty {
            b.zero(m);
        }
        if node == root {
            node = Ptr32::NULL;
        }
        node = tree::next_son(m, root, node);
        if node.is_null() {
            break;
        }
    }
    m.release(0x80);
}

/// The box of a node's control points as seen through a matrix, at `min` and `max`, and
/// the two radii in the node (+0x28, +0x2c) [soft 0x0846ad84 SI_modelComputeBoundingEL];
/// the answer is 0, or −1 for a kind without points (3, 7, 8, anything outside 0 ..= 10),
/// which stores the start values in the box and nothing in the node.
///
/// Every point of the node's current stored definition (`defs[cur_def]`; a patch's
/// control points) goes through the node's `gmat` and, **unrounded**, through `matrix`.
/// The first coordinate is compared with the box's minimum before it is rounded; the
/// radii are taken from the rounded coordinates: the largest `x² + z²` stays in the
/// register, the largest `(x² + z²) + y²` is a `float` in memory, rounded at every new
/// maximum. The flag byte at +8 is not touched.
///
/// A null (kind 0) gives two points, its global position −1 and +1 on every axis (each
/// rounded to `float` first); a joint (kind 5) its global position and that of its first
/// son, or its own +1 without a son. **For both, `gtrs` goes through `gmat`** (the
/// position is put through the matrix that already holds it), radii of 0 are stored, and
/// **the node's own box (+0x10 to +0x24) is zeroed**. A definition without points zeroes
/// the caller's box.
pub fn compute_bounding_el(m: &mut Mem, node: Ptr32<Model>, min: u32, max: u32, matrix: u32) -> i32 {
    let n = node.addr();
    let b = BoxAt { min, max };
    let mut axis = F80::ZERO;
    let mut origin = 0.0f32;
    b.start(m);
    let kind = m[node].kind;
    // a point through both matrices into the box; the answer is the rounded result
    let boxed = |m: &mut Mem, p: [f32; 3]| -> [f32; 3] {
        let w = through([x(p[0]), x(p[1]), x(p[2])], &mat(m, at!(node, gmat)));
        let v = through(w, &mat(m, matrix));
        keep_min(m, b.min, v[0]);
        let v = [v[0].to_f32(), v[1].to_f32(), v[2].to_f32()];
        keep_max(m, b.max, x(v[0]));
        for i in 1..3 {
            keep_min(m, b.min + 4 * i as u32, x(v[i]));
            keep_max(m, b.max + 4 * i as u32, x(v[i]));
        }
        v
    };
    let position = |m: &Mem, of: u32| [m.f32(of + 0x1b8), m.f32(of + 0x1bc), m.f32(of + 0x1c0)];
    let plus_one = |m: &Mem| position(m, n).map(|t| (x(1.0f32) + x(t)).to_f32());
    match kind {
        0 | 5 => {
            if kind == 0 {
                let p = position(m, n).map(|t| (x(t) - x(1.0f32)).to_f32());
                boxed(m, p);
                let p = plus_one(m);
                boxed(m, p);
            } else {
                let p = position(m, n);
                boxed(m, p);
                let son = m[node].son;
                let p = if son.is_null() { plus_one(m) } else { position(m, son.addr()) };
                boxed(m, p);
            }
            BoxAt { min: n + 0x10, max: n + 0x1c }.zero(m);
        }
        1 | 2 | 4 | 6 | 9 | 10 => {
            let def = stored_definition(m, node);
            let w = Walk { patch_curves: false, in_key_order: false, type_per_key: false };
            let empty = walk(m, w, kind, def, &mut |m, pt| {
                let p = pt.load(m);
                let v = boxed(m, p);
                let t = x(v[0]) * x(v[0]) + x(v[2]) * x(v[2]);
                if t.gt(axis) {
                    axis = t;
                }
                let d = t + x(v[1]) * x(v[1]);
                if d.gt(x(origin)) {
                    origin = d.to_f32();
                }
            });
            if empty {
                b.zero(m);
            }
        }
        _ => return -1,
    }
    m.set_f32(n + 0x28, axis.sqrt().to_f32());
    m.set_f32(n + 0x2c, x(origin).sqrt().to_f32());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_node(m: &mut Mem, points: &[[f32; 3]]) -> Ptr32<Model> {
        let node: Ptr32<Model> = Ptr32::new(m.place(0x430, 8));
        let (def, vertices) = (m.place(0x60, 8), m.place(0x14 * points.len() as u32, 8));
        for (i, p) in points.iter().enumerate() {
            for (k, c) in p.iter().enumerate() {
                m.set_f32(vertices + 0x14 * i as u32 + 4 * k as u32, *c);
            }
        }
        m.set_u32(def + geo::MESH_VERTICES, vertices);
        m.set_u32(def + geo::MESH_NB_VERTICES, points.len() as u32);
        m[node].kind = 4;
        m[node].definition = Ptr32::new(def);
        m[node].latdep = node;
        m[node].gmat = si_math::types::IDENTITY;
        node
    }

    #[test]
    fn a_mesh_s_box_and_radii_and_what_is_left_alone() {
        let mut m = Mem::new();
        let node = mesh_node(&mut m, &[[3.0, 4.0, 0.0], [-1.0, 0.5, 2.0], [f32::NAN, 9.0, -7.0]]);
        compute_bounding(&mut m, node);
        // a NaN enters no comparison: the third point gives y and z only
        assert_eq!((m[node].bbox_min.x, m[node].bbox_min.y, m[node].bbox_min.z), (-1.0, 0.5, -7.0));
        assert_eq!((m[node].bbox_max.x, m[node].bbox_max.y, m[node].bbox_max.z), (3.0, 9.0, 2.0));
        // the distance from the Y axis and from the origin (3-4-0 gives 3 and 5)
        assert_eq!(m[node].unk_28, [3.0, 5.0]);
        assert_eq!(m[node].flags_08 & 1, 1);
        // without vertices: six zeros; a joint: nothing at all
        let def = m[node].definition.addr();
        m.set_u32(def + geo::MESH_NB_VERTICES, 0);
        compute_bounding(&mut m, node);
        assert_eq!((m[node].bbox_min.x, m[node].bbox_max.z, m[node].unk_28), (0.0, 0.0, [0.0, 0.0]));
        m[node].kind = 5;
        m[node].bbox_min.x = 7.0;
        compute_bounding(&mut m, node);
        assert_eq!(m[node].bbox_min.x, 7.0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn a_tree_s_box_is_in_the_top_node_s_space() {
        let mut m = Mem::new();
        let top = mesh_node(&mut m, &[[1.0, 1.0, 1.0]]);
        let son = mesh_node(&mut m, &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
        m[top].son = son;
        m[son].father = top;
        // the top node stands at (10, 0, 0), the son at (13, 0, 0)
        m[top].gmat[3][0] = 10.0;
        m[son].gmat[3][0] = 13.0;
        let (min, max) = (m.place(12, 8), m.place(12, 8));
        hierarchy_bounding(&mut m, top, min, max);
        assert_eq!([m.f32(min), m.f32(min + 4), m.f32(min + 8)], [1.0, 0.0, 0.0]);
        assert_eq!([m.f32(max), m.f32(max + 4), m.f32(max + 8)], [5.0, 1.0, 1.0]);
        // through the element's routine: the same points seen from the world
        let world = m.place(0x40, 8);
        super::super::put_mat(&mut m, world, &si_math::types::IDENTITY);
        let array = m.place(4, 8);
        m.set_u32(array, m[son].definition.addr());
        m[son].defs = Ptr32::new(array);
        assert_eq!(compute_bounding_el(&mut m, son, min, max, world), 0);
        assert_eq!((m.f32(min), m.f32(max)), (13.0, 15.0));
        assert_eq!(m[son].unk_28, [15.0, 15.0]);
        m[son].kind = 8;
        assert_eq!(compute_bounding_el(&mut m, son, min, max, world), -1);
        assert!(m.fault().is_none());
    }
}
