// SPDX-License-Identifier: MIT
//! `envelop.c`: how a skin follows its skeleton.
//!
//! The scene update calls [`envelop_comp_update`] for a skin whose skeleton moved (finding
//! 0029 has the call tree). In short: every bone keeps the inverse of its matrix in the
//! skeleton's rest position ([`env_update_joint_state`]); a point of the skin, taken from
//! the first stored definition, is brought into the skeleton's space, through that inverse
//! and through the bone's present matrix, and what each bone makes of it is blended by the
//! weights of the bone's list.

use crate::env_globals::*;
use crate::env_records::{DefCrv, EditVert, EnvList, InitSrt};
use crate::env_outside::SI_SKE_CHAIN_ROOT;
use crate::envtools::*;
use crate::globals::SI_IDENTITE;
use crate::outside::{Outside, SI_MESH_COMPUTES_POLYGON_NORMAL, SI_MESH_COMPUTES_VERTEX_NORMAL, SI_NURBS_CRV_MOVE_POINT, SI_NURBS_SRF_MOVE_POINT};
use crate::records::geom;
use si_math::algebra::{matrix_copy, matrix_inverse, matrix_multiply, matrix_xyz_rotation, matrix_z_rotation};
use si_math::types::{Matrix, Vector};
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, fmov64, trig, x, F80};

type Node = Ptr32<Model>;
type MatPtr = Ptr32<[[f32; 4]; 4]>;

/// The double constants of the file's `.rodata` (held against the binary by
/// `re/tools/si-deform/check_env_data.py`).
pub const SMALL: f64 = 0.0001;
pub const SMALL_AT: [u32; 3] = [0x0866_0e68, 0x0866_0f80, 0x0866_0eb0];
pub const HALF: f64 = 0.5;
pub const HALF_AT: [u32; 2] = [0x0866_0e40, 0x0866_0e80];
pub const NINE: f64 = 9.0;
pub const NINE_AT: u32 = 0x0866_0e48;
pub const SPLIT: f64 = 0.6;
pub const SPLIT_AT: u32 = 0x0866_0e50;
pub const REST: f64 = 0.4;
pub const REST_AT: u32 = 0x0866_0e58;
pub const QUARTER_TURN: f64 = 1.5707963267948966;
pub const QUARTER_TURN_AT: [u32; 2] = [0x0866_0e60, 0x0866_0e70];
pub const SIDE: f64 = 0.15;
pub const SIDE_AT: u32 = 0x0866_0e78;
/// A weight is a percentage: times [`PERCENT`] it is a share. A weight below [`PERCENT`]
/// itself is passed over, and one not below [`FULL`] moves the point all the way.
pub const PERCENT: f64 = 0.01;
pub const PERCENT_AT: [u32; 4] = [0x0866_0f10, 0x0866_0f78, 0x0866_0ea8, 0x0866_0fb8];
pub const FULL: f64 = 99.99;
pub const FULL_AT: [u32; 4] = [0x0866_0f18, 0x0866_0f70, 0x0866_0eb8, 0x0866_0fb0];
/// A joint's blend reaches at most half a bone ([`HALF`]; the float [`HALF_F`] is what a
/// larger setting is replaced by).
pub const HALF_F: f32 = 0.5;
pub const HALF_F_AT: u32 = 0x0866_0e88;
pub const TURN: f64 = 6.283185307179586;
pub const TURN_AT: u32 = 0x0866_0e90;
pub const HALF_TURN: f64 = 3.141592653589793;
pub const HALF_TURN_AT: u32 = 0x0866_0e98;
pub const MINUS_HALF_TURN: f64 = -3.141592653589793;
pub const MINUS_HALF_TURN_AT: u32 = 0x0866_0ea0;

/// Offsets in a mesh beyond those of [`geom`]: the polygon count, and a polygon's flag word
/// whose bit 0x400 says "the normal is out of date".
pub const MESH_NB_POLYS: u32 = 0x18;
pub const POLY_FLAGS: u32 = 0x14;
pub const POLY_STALE: u16 = 0x0400;
/// A vertex's flag byte (+0x12): bit 0x20 "the corner normals are out of date".
pub const VERT_STALE: u8 = 0x20;

/// `st0 > v` or the comparison is unordered: the outcome the original's `jae` after
/// `and 0x45, dec, cmp 0x40` takes.
fn above_or_nan(a: F80, b: F80) -> bool {
    !a.le(b)
}

/// `st0 < v` or unordered: the outcome `and 5; jne` takes.
fn below_or_nan(a: F80, b: F80) -> bool {
    !a.ge(b)
}

fn zero() -> F80 {
    x(0.0f32)
}

fn one() -> F80 {
    x(1.0f32)
}

fn srt(m: &Mem, node: Node) -> Ptr32<InitSrt> {
    m[node].init_srt.cast()
}

fn vector(v: [f32; 3]) -> Vector {
    Vector::new(v[0], v[1], v[2])
}

/// A new element for the chain of points edited by hand [soft 0x084b1bc0 editVertAllocate]:
/// 0x4c bytes from `SI_malloc`, two words of 0, the identity matrix, no successor. The
/// block is written without a test for null.
pub fn edit_vert_allocate(m: &mut Mem) -> Ptr32<EditVert> {
    let e: Ptr32<EditVert> = Ptr32::new(m.si_malloc(0x4c));
    m[e].weight = 0.0;
    m[e].index = 0;
    let identity = m[SI_IDENTITE];
    m[e].matrix = matrix_copy(&identity);
    m[e].next = Ptr32::NULL;
    e
}

/// Frees the chain of edited points and clears the file's pointer to it
/// [soft 0x084b1c04 editVertDispose].
pub fn edit_vert_dispose(m: &mut Mem) {
    let mut e = m[EDITVERT];
    if e.is_null() {
        return;
    }
    loop {
        let next = m[e].next;
        m.free(e.addr());
        e = next;
        if e.is_null() {
            break;
        }
    }
    m[EDITVERT] = Ptr32::NULL;
}

/// Makes the table of one 16-bit word per point of a skin, all 0, and leaves it in the
/// file's `damptab` [soft 0x084b1c40 dampTabCreate]. The count is read from the node's
/// first stored definition: patch (kind 1) the product of its two 16-bit counts, mesh (4)
/// its vertex count, spline (6) its 16-bit key count, NURBS curve (9) and surface (10)
/// their point counts. 1 on success; 0 for any other kind and when `SI_malloc` (of twice
/// the count) answers null, which it does for a count of 0.
pub fn damp_tab_create(m: &mut Mem, node: Node) -> i32 {
    let kind = m[node].kind;
    if !matches!(kind, 1 | 4 | 6 | 9 | 10) {
        return 0;
    }
    let def = m[m[node].defs].addr();
    let n: i32 = match kind {
        1 => (m.u16(def.wrapping_add(geom::PATCH_NU)) as i32).wrapping_mul(m.u16(def.wrapping_add(geom::PATCH_NV)) as i32),
        4 => m.i32(def.wrapping_add(geom::MESH_NB_VERTS)),
        6 => m.u16(def.wrapping_add(geom::SPLINE_NB_KEYS)) as i32,
        9 => m.i32(def.wrapping_add(geom::CRV_NB_POINTS)),
        _ => m.i32(def.wrapping_add(geom::SRF_NB_POINTS)),
    };
    let tab = m.si_malloc((n as u32).wrapping_add(n as u32));
    m[DAMPTAB] = Ptr32::new(tab);
    if tab == 0 {
        return 0;
    }
    let mut i = 0;
    while i < n {
        let slot = m[DAMPTAB].add(i);
        m[slot] = 0;
        i += 1;
    }
    1
}

/// Brings a mesh's normals up to date where its points moved
/// [soft 0x084b1f1c envelopMeshCompNormals].
///
/// Every polygon whose flag bit 0x400 is set gets its normal recomputed
/// (`SI_meshComputesPolygonNormal (0x084599f4)`), the bit cleared, and bit 0x20 set in the
/// flag byte of each of its corners' vertices (a corner whose vertex number is −1 is
/// passed over). Then every vertex with that bit gets its corner normals recomputed
/// (`SI_meshComputesVertexNormal (0x08459e8c)`) and the bit cleared. While this runs, bit 3
/// of the mesh's flag word (+0x20) is set, unless bit 1 or 3 was set already.
pub fn envelop_mesh_comp_normals(m: &mut Mem, o: &mut dyn Outside, mesh: u32) {
    let mut set = false;
    let flags = m.u16(mesh.wrapping_add(geom::MESH_FLAGS));
    if flags & 0x0a == 0 {
        m.set_u16(mesh.wrapping_add(geom::MESH_FLAGS), flags | 8);
        set = true;
    }
    let mut poly = m.u32(mesh.wrapping_add(geom::MESH_POLYS));
    let mut i: i32 = 0;
    while i < m.i32(mesh.wrapping_add(MESH_NB_POLYS)) {
        if m.u16(poly.wrapping_add(POLY_FLAGS)) & POLY_STALE != 0 {
            o.call(m, &SI_MESH_COMPUTES_POLYGON_NORMAL, &[mesh, i as u32]);
            let f = m.u16(poly.wrapping_add(POLY_FLAGS));
            m.set_u16(poly.wrapping_add(POLY_FLAGS), f & !POLY_STALE);
            let mut c: u16 = 0;
            while c < m.u16(poly.wrapping_add(geom::POLY_NB_CORNERS)) {
                let corners = m.u32(poly.wrapping_add(geom::POLY_CORNERS));
                let v = m.u32(corners.wrapping_add((c as u32).wrapping_mul(geom::CORNER_SIZE)));
                if v != 0xffff_ffff {
                    let verts = m.u32(mesh.wrapping_add(geom::MESH_VERTS));
                    m.or_u8(verts.wrapping_add(v.wrapping_mul(geom::VERT_SIZE)).wrapping_add(geom::VERT_FLAGS), VERT_STALE);
                }
                c = c.wrapping_add(1);
            }
        }
        i += 1;
        poly = poly.wrapping_add(geom::POLY_SIZE);
    }
    let mut vert = m.u32(mesh.wrapping_add(geom::MESH_VERTS));
    let mut i: i32 = 0;
    while i < m.i32(mesh.wrapping_add(geom::MESH_NB_VERTS)) {
        if m.u8(vert.wrapping_add(geom::VERT_FLAGS)) & VERT_STALE != 0 {
            o.call(m, &SI_MESH_COMPUTES_VERTEX_NORMAL, &[mesh, i as u32]);
            m.and_u8(vert.wrapping_add(geom::VERT_FLAGS), !VERT_STALE);
        }
        i += 1;
        vert = vert.wrapping_add(geom::VERT_SIZE);
    }
    if set {
        m.and_u8(mesh.wrapping_add(geom::MESH_FLAGS), 0xf7);
    }
}

/// A bone's rest matrix and its inverse [soft 0x084b1fec envUpdateJointState].
///
/// `matrix` holds the rest matrix of the bone's father on entry and the bone's on return:
/// the bone's local rest matrix times the father's. The local one is, for a joint or an
/// effector: the rotation of the rest angles (`Model` +0x328, the second vector; the
/// identity when the node's word is 4) with **the node's present translation** in the last
/// row; for any other node: that rotation with each of its first three rows multiplied by
/// the rest scaling of its axis (three products each, rounded to float) and the rest
/// translation in the last row. The inverse of the product goes to the bone's record
/// (+0x08), which stays as it was when there is none.
pub fn env_update_joint_state(m: &mut Mem, node: Node, matrix: MatPtr) {
    let kind = m[node].kind;
    let rest = srt(m, node);
    let mut local: Matrix;
    if kind == 5 || is_effector(m, node) {
        if m[node].envelop == 4 {
            let identity = m[SI_IDENTITE];
            local = matrix_copy(&identity);
        } else {
            local = matrix_xyz_rotation(&vector(m[rest].rot));
        }
        local[3][0] = fmov(m[node].trans.x);
        local[3][1] = fmov(m[node].trans.y);
        local[3][2] = fmov(m[node].trans.z);
    } else {
        local = matrix_xyz_rotation(&vector(m[rest].rot));
        let scale = m[rest].scale;
        for i in 0..3 {
            for j in 0..3 {
                local[i][j] = (x(local[i][j]) * scale[i]).to_f32();
            }
        }
        let trans = m[rest].trans;
        for j in 0..3 {
            local[3][j] = fmov(trans[j]);
        }
    }
    let father = m.read(matrix);
    let product = matrix_multiply(&local, &father);
    m.write(matrix, product);
    if let Some(inverse) = matrix_inverse(&product) {
        let e = info(m, node);
        m[e].matrix = inverse;
    }
}

/// The rest matrices of a branch of bones [soft 0x084b211c envUpdateMatrix]: for `node`, if
/// its record's level is `level`, [`env_update_joint_state`] on a copy of `matrix` and then
/// the same for its first son one level down with that copy; and, whatever the node's
/// level, the same for its next brother with `matrix` and `level`.
pub fn env_update_matrix(m: &mut Mem, node: Node, matrix: MatPtr, level: i32) {
    if envelop_level(m, node) == level {
        let mine: MatPtr = Ptr32::new(m.local(0x40));
        let father = m.read(matrix);
        m.write(mine, matrix_copy(&father));
        env_update_joint_state(m, node, mine);
        let son = m[node].son;
        if !son.is_null() {
            env_update_matrix(m, son, mine, level.wrapping_add(1));
        }
        m.release(0x40);
    }
    let brother = m[node].brother;
    if !brother.is_null() {
        env_update_matrix(m, brother, matrix, level);
    }
}

/// Parks or restores the rest rotations of a chain under an automatic local envelope
/// [soft 0x084b2198 envAutoSet]. Nothing unless `root` is a chain's root whose local skin
/// ([`env_local_get`]) has the word 8.
///
/// With `set`: from the root's first son down the first sons, until an effector is met,
/// each node's rest rotation is copied (as three words) to its record (+0x54) and cleared.
/// Without: each record's three words are copied back. The first node is tested for being
/// the effector before anything is done only when setting.
pub fn env_auto_set(m: &mut Mem, root: Node, set: i16) {
    if !is_chain_root(m, root) {
        return;
    }
    let local = env_local_get(m, root);
    if local.is_null() || m[local].envelop != 8 {
        return;
    }
    let mut n = m[root].son;
    if set != 0 {
        if is_effector(m, n) {
            return;
        }
        loop {
            let (rest, e) = (srt(m, n), info(m, n));
            let rot = m[rest].rot;
            m[e].saved_rot = rot;
            let rest = srt(m, n);
            m[rest].rot = [0.0; 3];
            n = m[n].son;
            if is_effector(m, n) {
                return;
            }
        }
    }
    while !is_effector(m, n) {
        let (e, rest) = (info(m, n), srt(m, n));
        let rot = m[e].saved_rot;
        m[rest].rot = rot;
        n = m[n].son;
    }
}

/// `x` to the power `y` as the profile wants it [soft 0x084b22a4 expFct]: 0 unless 0 < x
/// (a NaN goes on), 1 unless y > 0 (a NaN goes on), else the library's `pow` on the two
/// widened to double, rounded to float.
pub fn exp_fct(base: f32, exponent: f32) -> f32 {
    if !below_or_nan(zero(), x(base)) {
        return 0.0;
    }
    if !above_or_nan(x(exponent), zero()) {
        return 1.0;
    }
    trig::pow(base as f64, exponent as f64).to_f32()
}

/// The value of a deformation profile at a position [soft 0x084b22fc yDef]; `t` is how
/// strongly the joint is bent sideways, 0 to 1. The answer is left in the register.
///
/// With a table: the first of the first 99 pairs whose position is not below `pos` is
/// looked up (the hundredth when none is), and of it and the pair before it the one nearer
/// to `pos` answers with its value (the earlier one only when strictly nearer). No
/// interpolation.
///
/// Kind 1, with a = float((1 − t) + t × 0.5) and b = float(t × 9 + 1): up to 0.6 the answer
/// is expFct(float(pos / 0.6), a); beyond (or for a NaN) it is
/// float(1 − expFct(float((pos − 0.6) / 0.4), b)).
///
/// Kind 2: float(cos(v × π/2)) with v = 1 − (pos + pos) up to 0.5 and
/// (pos − 0.5) + (pos − 0.5) beyond: the library's cosine, on a double.
///
/// Any other kind: 0.
pub fn y_def(m: &Mem, prm: &DefCrv, pos: f32, t: f32) -> F80 {
    let fx = x(pos);
    let tab = prm.table;
    if !tab.is_null() {
        let mut i: i32 = 0;
        if !fx.le(x(m[tab])) {
            loop {
                i += 1;
                if i > 0x62 || !above_or_nan(fx, x(m[tab.add(2 * i)])) {
                    break;
                }
            }
        }
        if i > 0 {
            let after = x(m[tab.add(2 * i)]) - fx;
            let before = fx - m[tab.add(2 * i - 2)];
            if before.lt(after) {
                i -= 1;
            }
        }
        return x(m[tab.add(2 * i + 1)]);
    }
    match prm.kind {
        1 => {
            let ft = x(t);
            let a = ((one() - ft) + ft * HALF).to_f32();
            let b = (ft * NINE + one()).to_f32();
            let split = x(SPLIT);
            if !below_or_nan(split, fx) {
                let q = (fx / split).to_f32();
                x(exp_fct(q, a))
            } else {
                let q = ((fx - split) / REST).to_f32();
                x((one() - x(exp_fct(q, b))).to_f32())
            }
        }
        2 => {
            let half = x(HALF);
            let v = if !below_or_nan(half, fx) {
                one() - (fx + fx)
            } else {
                let d = fx - half;
                d + d
            };
            x(trig::cos((v * QUARTER_TURN).to_f64()).to_f32())
        }
        _ => zero(),
    }
}

/// How far a point of the skin is pushed out of (or pulled into) the bend of a joint
/// [soft 0x084b2450 compDef]; the answer is left in the register, unrounded.
///
/// `pos` is the point's position along the blend, `bend` and `turn` two of the joint's
/// present angles, `side` and `along` two coordinates of the point in the joint's frame.
/// Zero when both profiles of the joint have an amplitude below 0.0001, when both angles
/// are smaller than 0.0001, and when the profile chosen has neither kind nor table, an
/// amplitude not above 0.0001, or a range that `pos` is outside of.
///
/// When the joint's son is the chain's effector, `bend` takes the place of `turn` and 0 the
/// place of `bend`. The inner profile is chosen when sin(turn) and `side` have the same
/// sign (precisely: sine below 0 and side below 0, or sine not below 0 and side not below
/// 0), else the outer. With p = float(|bend| / (π/2)) and q = float(|turn| / (π/2)), each 1
/// when the angle is π/2 or more, r = float(p × (1 − q) × 0.15 + q), and
/// e = float(|side| / sqrt(along² + side²)) (the root itself when it is not above 0), the
/// answer is yDef(profile, float((pos − lo) / (hi − lo)), r) × e × float(amplitude × r)
/// × scale × side.
pub fn comp_def(m: &Mem, node: Node, pos: f32, bend: f32, turn: f32, side: f32, along: f32) -> F80 {
    let e = info(m, node);
    let small = x(SMALL);
    if x(m[e].inner.amplitude).lt(small) && small.gt(x(m[e].outer.amplitude)) {
        return zero();
    }
    if small.gt(x(bend).abs()) && x(turn).abs().lt(small) {
        return zero();
    }
    let son = m[node].son;
    let (bend, turn) = if is_effector(m, son) { (0.0f32, bend) } else { (bend, turn) };
    let sine = trig::sin(turn as f64);
    let inner = if sine.lt(zero()) { zero().gt(x(side)) } else { !zero().gt(x(side)) };
    let e = info(m, node);
    let prm: DefCrv = if inner { m[e].inner } else { m[e].outer };
    if prm.kind == 0 && prm.table.is_null() {
        return zero();
    }
    if !x(prm.amplitude).gt(small) {
        return zero();
    }
    let (lo, fpos) = (x(prm.lo), x(pos));
    if above_or_nan(lo, fpos) {
        return zero();
    }
    let hi = x(prm.hi);
    if below_or_nan(hi, fpos) {
        return zero();
    }
    let place = (fpos - lo) / (hi - lo);
    let quarter = x(QUARTER_TURN);
    let share = |angle: f32| -> F80 {
        let a = x(angle).abs();
        if above_or_nan(quarter, a) {
            x((a / quarter).to_f32())
        } else {
            one()
        }
    };
    let p = share(bend);
    let q = share(turn);
    let r = ((p * (one() - q)) * SIDE + q).to_f32();
    let reach = (x(along) * x(along) + x(side) * x(side)).sqrt();
    let lean = if reach.gt(zero()) { x((x(side).abs() / reach).to_f32()) } else { reach };
    let gain = (x(prm.amplitude) * x(r)).to_f32();
    let y = y_def(m, &prm, place.to_f32(), r);
    (((y * lean) * gain) * prm.scale) * side
}

/// A matrix local of the original that it may read without having set it (the result of
/// `SI_matrixInverse` when there is no inverse): room on the image's stack.
///
/// The original reads there whatever its earlier calls left on the processor's stack,
/// which cannot be reproduced; in the emulated original those words are addresses of the
/// stack, which read as floats are NaNs, and every point that goes through the matrix
/// becomes that NaN. Here the sixteen floats are NaNs too, with the image's fill in their
/// low bits: a result that depends on the local is thereby not a number, as in the
/// original, and in a check (which runs each case on two fills) it shows as depending on
/// memory nobody wrote.
fn unset_matrix(m: &mut Mem) -> MatPtr {
    let at = m.local(0x40);
    // one payload for the sixteen, so that it does not matter which of them an operation
    // hands on
    let fill = m.fill();
    let low = fill.byte(at) as u32 | (fill.byte(at + 1) as u32) << 8 | ((fill.byte(at + 2) & 0x3f) as u32) << 16;
    for k in 0..16u32 {
        m.set_u32(at + 4 * k, 0x7fc0_0000 | low);
    }
    Ptr32::new(at)
}

/// The list of a skin in a bone's chain of lists (null when the bone has none for it).
fn list_of(m: &Mem, bone: Node, skin: Node) -> Ptr32<EnvList> {
    let e = info(m, bone);
    let mut l = m[e].lists;
    while !l.is_null() && m[l].owner != skin {
        l = m[l].next;
    }
    l
}

/// What a bone does to a point of its skin, as a matrix on the skin's own coordinates:
/// skin to world, world to the bone at rest, the bone's present matrix, and `back` (world
/// to skin). Three calls of `SI_matrixMultiply`.
fn bone_matrix(m: &Mem, skin: Node, bone: Node, back: MatPtr) -> Matrix {
    let e = info(m, bone);
    let a = matrix_multiply(&m[skin].gmat, &m[e].matrix);
    let b = matrix_multiply(&a, &m[bone].gmat);
    matrix_multiply(&b, &m.read(back))
}

fn rd3(m: &Mem, a: u32) -> [f32; 3] {
    [m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8))]
}

fn wr3(m: &mut Mem, a: u32, v: [f32; 3]) {
    m.set_f32(a, v[0]);
    m.set_f32(a.wrapping_add(4), v[1]);
    m.set_f32(a.wrapping_add(8), v[2]);
}

/// A NURBS control point as a position: x, y, z each divided by the weight, in doubles in
/// the register, rounded to float.
fn nurbs_point(m: &Mem, cp: u32) -> [f32; 3] {
    let w = m.f64(cp.wrapping_add(geom::CP_W));
    [(x(m.f64(cp)) / w).to_f32(), (x(m.f64(cp.wrapping_add(8))) / w).to_f32(), (x(m.f64(cp.wrapping_add(16))) / w).to_f32()]
}

/// A point through a matrix as `envelopSkelMdl` writes it out: per coordinate the two
/// first products are added, then the third, then the translation, in the register, with
/// one rounding.
fn through(mm: &Matrix, p: [f32; 3]) -> [f32; 3] {
    let (px, py, pz) = (x(p[0]), x(p[1]), x(p[2]));
    [
        (((py * mm[1][0] + px * mm[0][0]) + pz * mm[2][0]) + mm[3][0]).to_f32(),
        ((pz * mm[2][1] + (px * mm[0][1] + py * mm[1][1])) + mm[3][1]).to_f32(),
        ((pz * mm[2][2] + (py * mm[1][2] + px * mm[0][2])) + mm[3][2]).to_f32(),
    ]
}

/// The share of a move a weight below [`FULL`] takes, added to `base`: per coordinate
/// float(float((moved − rest) × (weight × 0.01)) + base), the difference and the share in
/// the register.
fn share(weight: f32, moved: [f32; 3], rest: [f32; 3], base: [f32; 3]) -> [f32; 3] {
    let f = x(weight) * PERCENT;
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        let step = ((x(moved[c]) - rest[c]) * f).to_f32();
        out[c] = (x(step) + base[c]).to_f32();
    }
    out
}

/// Whether, while the user edits points, a point takes part: its tag (bit 0 of the flag)
/// is set, or `marked` is, or it is the point under the cursor.
fn edited(tagged: bool, marked: bool, current: bool) -> bool {
    tagged || marked || current
}

/// One vertex of a bone's list, ready to be moved.
struct Point {
    kind: i32,
    /// The skin's first stored definition and its working one (0 for a kind without points).
    def0: u32,
    work: u32,
    idx: i32,
    weight: f32,
    /// The point at rest; for a spline of type 1 also the key's two tangent points.
    p: [f32; 3],
    tan_a: [f32; 3],
    tan_b: [f32; 3],
}

/// Reads the point a vertex of a bone's list names from the skin's first stored definition
/// (the first half of the loop bodies of `envelopSkelMdl` and `envelopJoint`, which are the
/// same instruction for instruction). `None`: the vertex is passed over (a weight below
/// 0.01, a number that is no point of the definition, or, while the user edits, a point
/// that takes no part).
fn fetch(m: &Mem, skin: Node, idx: i32, weight: f32, edit: i16) -> Option<Point> {
    if x(weight).lt(x(PERCENT)) {
        return None;
    }
    let kind = m[skin].kind;
    let (mut def0, mut work) = (0u32, 0u32);
    if matches!(kind, 1 | 4 | 6 | 9 | 10) {
        def0 = m[m[skin].defs].addr();
        work = m[skin].definition.addr();
    }
    let current = m[SI_MDL_PNT];
    let mut p = [0.0f32; 3];
    let (mut tan_a, mut tan_b) = ([0.0f32; 3], [0.0f32; 3]);
    let mut take = true;
    match kind {
        1 => {
            let n = (m.u16(def0.wrapping_add(geom::PATCH_NU)) as i32).wrapping_mul(m.u16(def0.wrapping_add(geom::PATCH_NV)) as i32);
            if idx < 0 || idx >= n {
                take = false;
            } else {
                if edit != 0 {
                    let flag = m.u16(m.u32(work.wrapping_add(geom::PATCH_FLAGS)).wrapping_add((idx as u32).wrapping_mul(2)));
                    take = edited(flag & 1 != 0, (flag as i16) < 0, current == idx);
                }
                if take {
                    let at = m.u32(def0.wrapping_add(geom::PATCH_POINTS)).wrapping_add((idx as u32) << 4);
                    p = rd3(m, at).map(fmov);
                }
            }
        }
        4 => {
            if idx < 0 || idx >= m.i32(def0.wrapping_add(geom::MESH_NB_VERTS)) {
                take = false;
            } else {
                let off = (idx as u32).wrapping_mul(geom::VERT_SIZE);
                if edit != 0 {
                    let flag = m.u8(m.u32(work.wrapping_add(geom::MESH_VERTS)).wrapping_add(off).wrapping_add(geom::VERT_FLAGS));
                    take = edited(flag & 1 != 0, flag & VERT_STALE != 0, current == idx);
                }
                if take {
                    p = rd3(m, m.u32(def0.wrapping_add(geom::MESH_VERTS)).wrapping_add(off));
                }
            }
        }
        6 => {
            if idx < 0 || idx >= m.u16(def0.wrapping_add(geom::SPLINE_NB_KEYS)) as i32 {
                take = false;
            } else {
                let off = (idx as u32).wrapping_mul(geom::KEY_SIZE);
                if edit != 0 {
                    let key = m.u32(work.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off);
                    let flag = m.u16(key.wrapping_add(geom::KEY_FLAGS));
                    // the cursor's number counts a key's three points: divided by 3, toward zero
                    take = edited(flag & 1 != 0, (flag as i16) < 0, current.wrapping_div(3) == idx);
                }
                if take {
                    let key = m.u32(def0.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off);
                    p = rd3(m, key.wrapping_add(geom::KEY_POINT));
                    if m.i32(def0.wrapping_add(geom::SPLINE_TYPE)) == 1 {
                        tan_a = rd3(m, key);
                        tan_b = rd3(m, key.wrapping_add(0x18));
                    }
                }
            }
        }
        9 | 10 => {
            let (count, points) = if kind == 9 { (geom::CRV_NB_POINTS, geom::CRV_POINTS) } else { (geom::SRF_NB_POINTS, geom::SRF_POINTS) };
            if idx < 0 || idx >= m.i32(def0.wrapping_add(count)) {
                take = false;
            } else {
                let off = (idx as u32).wrapping_mul(geom::CP_SIZE);
                if edit != 0 {
                    let cp = m.u32(work.wrapping_add(points)).wrapping_add(off);
                    take = edited(m.u8(cp.wrapping_add(geom::CP_FLAGS)) & 1 != 0, m.i32(cp.wrapping_add(geom::CP_FLAGS)) < 0, current == idx);
                }
                if take {
                    p = nurbs_point(m, m.u32(def0.wrapping_add(points)).wrapping_add(off));
                }
            }
        }
        _ => {}
    }
    if !take {
        return None;
    }
    Some(Point { kind, def0, work, idx, weight, p, tan_a, tan_b })
}

/// Moves one point by a bone's matrix `mm` and stores it (the second half of the two loop
/// bodies): the transformation, the share of it the weight takes, the store into the
/// working definition by the skin's kind, or, while the user edits, the gathering of the
/// matrix in `editvert`; and the point's mark in `damptab`.
fn deposit(m: &mut Mem, o: &mut dyn Outside, pt: &Point, mm: &Matrix, edit: i16) {
    let Point { kind, def0, work, idx, weight, p, .. } = *pt;
    let (mut tan_a, mut tan_b) = (pt.tan_a, pt.tan_b);
    let mut spline_tangents = false;
    let moved = through(mm, p);
    if kind == 6 && m.i32(def0.wrapping_add(geom::SPLINE_TYPE)) == 1 {
        spline_tangents = true;
        let (ax, ay, az) = (x(tan_a[0]), x(tan_a[1]), x(tan_a[2]));
        tan_a = [
            (((ay * mm[1][0] + ax * mm[0][0]) + az * mm[2][0]) + mm[3][0]).to_f32(),
            ((az * mm[2][1] + (ay * mm[1][1] + ax * mm[0][1])) + mm[3][1]).to_f32(),
            ((az * mm[2][2] + (ay * mm[1][2] + ax * mm[0][2])) + mm[3][2]).to_f32(),
        ];
        let (bx, by, bz) = (x(tan_b[0]), x(tan_b[1]), x(tan_b[2]));
        tan_b = [
            (((by * mm[1][0] + bx * mm[0][0]) + bz * mm[2][0]) + mm[3][0]).to_f32(),
            (((bx * mm[0][1] + by * mm[1][1]) + bz * mm[2][1]) + mm[3][1]).to_f32(),
            (x(mm[3][2]) + (x(mm[2][2]) * bz + (x(mm[1][2]) * by + bx * mm[0][2]))).to_f32(),
        ];
    }
    let damp = m[DAMPTAB].add(idx);
    if edit != 0 {
        if m[damp] == 0 {
            let ev = edit_vert_allocate(m);
            m[ev].index = idx;
            m[ev].weight = fmov(weight);
            m[ev].matrix = matrix_copy(&mm);
            let f = x(PERCENT) * weight;
            for i in 0..4 {
                for j in 0..4 {
                    m[ev].matrix[i][j] = (f * m[ev].matrix[i][j]).to_f32();
                }
            }
            let head = m[EDITVERT];
            if !head.is_null() {
                m[ev].next = head;
            }
            m[EDITVERT] = ev;
        } else {
            let mut ev = m[EDITVERT];
            while !ev.is_null() && m[ev].index != idx {
                ev = m[ev].next;
            }
            if !ev.is_null() {
                m[ev].weight = (x(weight) + m[ev].weight).to_f32();
                for i in 0..4 {
                    for j in 0..4 {
                        m[ev].matrix[i][j] = (((x(weight) * mm[i][j]) * PERCENT) + m[ev].matrix[i][j]).to_f32();
                    }
                }
            }
        }
    } else {
        let partial = x(weight).lt(x(FULL));
        // a point a bone has moved already in this pass is added to where it is now
        let moved_before = |m: &Mem| m[damp] != 0;
        match kind {
            1 => {
                let off = (idx as u32) << 4;
                let mut out = moved;
                if partial {
                    let rest = rd3(m, m.u32(def0.wrapping_add(geom::PATCH_POINTS)).wrapping_add(off));
                    let from = if moved_before(m) { work } else { def0 };
                    let base = rd3(m, m.u32(from.wrapping_add(geom::PATCH_POINTS)).wrapping_add(off));
                    out = share(weight, moved, rest, base);
                }
                let at = m.u32(work.wrapping_add(geom::PATCH_POINTS)).wrapping_add(off);
                wr3(m, at, out.map(fmov));
                m.set_f32(at.wrapping_add(12), 1.0);
            }
            4 => {
                let off = (idx as u32).wrapping_mul(geom::VERT_SIZE);
                let mut out = moved;
                if partial {
                    let rest = rd3(m, m.u32(def0.wrapping_add(geom::MESH_VERTS)).wrapping_add(off));
                    let from = if moved_before(m) { work } else { def0 };
                    let base = rd3(m, m.u32(from.wrapping_add(geom::MESH_VERTS)).wrapping_add(off));
                    out = share(weight, moved, rest, base);
                }
                let vert = m.u32(work.wrapping_add(geom::MESH_VERTS)).wrapping_add(off);
                wr3(m, vert, out);
                if !moved_before(m) {
                    let mut c: i32 = 0;
                    while c < m.u16(vert.wrapping_add(geom::VERT_NB_STAR)) as i32 {
                        let star = m.u32(vert.wrapping_add(geom::VERT_STAR));
                        let poly = m.u32(star.wrapping_add((c as u32).wrapping_mul(geom::STAR_SIZE)).wrapping_add(geom::STAR_POLY));
                        if poly != 0xffff_ffff {
                            let polys = m.u32(work.wrapping_add(geom::MESH_POLYS));
                            m.or_u8(polys.wrapping_add(poly.wrapping_mul(geom::POLY_SIZE)).wrapping_add(POLY_FLAGS + 1), (POLY_STALE >> 8) as u8);
                        }
                        c += 1;
                    }
                }
            }
            6 => {
                let off = (idx as u32).wrapping_mul(geom::KEY_SIZE);
                let mut out = moved;
                if partial {
                    let rest = rd3(m, m.u32(def0.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off).wrapping_add(geom::KEY_POINT));
                    let from = if moved_before(m) { work } else { def0 };
                    let base = rd3(m, m.u32(from.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off).wrapping_add(geom::KEY_POINT));
                    out = share(weight, moved, rest, base);
                }
                let key = m.u32(work.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off);
                wr3(m, key.wrapping_add(geom::KEY_POINT), out);
                if m.i32(work.wrapping_add(geom::SPLINE_TYPE)) == 1 {
                    // (the original stores its locals here whether or not it set them)
                    let _ = spline_tangents;
                    let key = m.u32(work.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off);
                    wr3(m, key, tan_a);
                    let key = m.u32(work.wrapping_add(geom::SPLINE_KEYS)).wrapping_add(off);
                    wr3(m, key.wrapping_add(0x18), tan_b);
                }
            }
            9 | 10 => {
                let points = if kind == 9 { geom::CRV_POINTS } else { geom::SRF_POINTS };
                let off = (idx as u32).wrapping_mul(geom::CP_SIZE);
                let mut out = moved;
                if partial {
                    let rest = nurbs_point(m, m.u32(def0.wrapping_add(points)).wrapping_add(off));
                    let f = x(weight) * PERCENT;
                    let step = [((x(moved[0]) - rest[0]) * f).to_f32(), ((x(moved[1]) - rest[1]) * f).to_f32(), (f * (x(moved[2]) - rest[2])).to_f32()];
                    let base = if moved_before(m) { nurbs_point(m, m.u32(work.wrapping_add(points)).wrapping_add(off)) } else { rest };
                    out = [(x(base[0]) + step[0]).to_f32(), (x(step[1]) + base[1]).to_f32(), (x(step[2]) + base[2]).to_f32()];
                }
                let d = |v: f32| (v as f64).to_bits();
                let (dx, dy, dz) = (d(out[0]), d(out[1]), d(out[2]));
                let mut args = vec![work, idx as u32, dx as u32, (dx >> 32) as u32, dy as u32, (dy >> 32) as u32, dz as u32, (dz >> 32) as u32];
                if kind == 9 {
                    o.call(m, &SI_NURBS_CRV_MOVE_POINT, &args);
                } else {
                    let w = m.u64(m.u32(work.wrapping_add(points)).wrapping_add(off).wrapping_add(geom::CP_W));
                    args.extend([w as u32, (w >> 32) as u32]);
                    o.call(m, &SI_NURBS_SRF_MOVE_POINT, &args);
                }
            }
            _ => {}
        }
    }
    let damp = m[DAMPTAB].add(idx);
    m[damp] = 1;
}

/// Moves the points of a skin that a bone carries, each by the bone's whole matrix
/// weighted with the point's weight [soft 0x084b41fc envelopSkelMdl]: the path for every
/// bone that is not a joint, and for a joint without blend while a sequence plays.
///
/// `back` is the inverse of the skin's global matrix; `edit` is not 0 while the user moves
/// points of the skin by hand. For each vertex of the bone's list for this skin whose
/// weight is not below 0.01 and whose number is a point of the first stored definition:
///
/// 1. The point p is read from **the first stored definition** (a NURBS point divided by
///    its weight) and p′ = p × M is formed with M = skin's matrix × the bone's rest inverse
///    × the bone's matrix × `back` ([`bone_matrix`]; made once before the loop while a
///    sequence plays, else anew for every vertex).
/// 2. With a weight of 99.99 or more the point becomes p′. Otherwise it becomes
///    base + float((p′ − p) × weight/100), where base is the point of the working
///    definition if a bone has moved it already in this pass (`damptab`), else p.
/// 3. The result goes to the working definition: a patch point with fourth word 1.0; a mesh
///    vertex, and when it is the vertex's first move the polygons of its star are marked
///    stale; a spline key's point, and for a spline of type 1 the key's two tangent points
///    transformed by M whole, whatever the weight; a NURBS point through
///    `SI_nurbsCrvMovePoint (0x084e9eec)` or `SI_nurbsSrfMovePoint (0x084e9f2c)` (the latter
///    with the working point's own weight).
/// 4. The point's word in `damptab` becomes 1.
///
/// While the user edits (`edit`): only tagged points, points with their flag's top bit (a
/// mesh vertex: bit 0x20) and the point under the cursor (`SI_mdlPnt`; a spline: that
/// number divided by 3) take part, and instead of step 2 and 3 the matrix is gathered in
/// the chain `editvert`: a new element with M × weight/100 for a point's first bone, added
/// to for the others.
///
/// Where the Rust differs: a spline whose working definition has type 1 while its first
/// stored one has not gets its tangent points from locals the original never set (here:
/// zeros); the same for the point of a skin of a kind that has no points.
pub fn envelop_skel_mdl(m: &mut Mem, o: &mut dyn Outside, skin: Node, bone: Node, back: MatPtr, edit: i16) {
    let list = list_of(m, bone, skin);
    if list.is_null() {
        return;
    }
    let playing = m[SI_GLB_ANIM][GLB_ANIM_PLAYING] != 0;
    let mut mm: Matrix = [[0.0; 4]; 4];
    if playing {
        if m[list].verts.is_null() {
            return;
        }
        mm = bone_matrix(m, skin, bone, back);
    }
    let mut v = m[list].verts;
    while !v.is_null() {
        let (idx, weight) = (m[v].index, m[v].weight);
        if let Some(pt) = fetch(m, skin, idx, weight, edit) {
            if !playing {
                mm = bone_matrix(m, skin, bone, back);
            }
            deposit(m, o, &pt, &mm, edit);
        }
        v = m[v].next;
    }
}

/// Index in [`SI_GLB_ANIM`] of the word at +0x2e, which `envelopJoint` asks before it bends
/// the skin around a chain's first joint.
pub const GLB_ANIM_2E: usize = 0x17;

/// A joint's blend length as `envelopJoint` uses it: 0 for a negative setting, 0.5 for one
/// above 0.5.
fn blend_length(area: f32) -> f32 {
    let a = x(area);
    if a.lt(zero()) {
        0.0
    } else if a.gt(x(HALF)) {
        HALF_F
    } else {
        fmov(area)
    }
}

/// An angle as `envelopJoint` prepares it: the remainder of the division by 2π (the unit's
/// partial remainder, repeated until done; rounded to double, then to float), then moved by
/// one turn into −π … π.
fn wrapped(angle: f32, by_memory: bool) -> f32 {
    let turn = x(TURN);
    let a = x(x(angle).fmod(turn).to_f64()).to_f32();
    let fa = x(a);
    if fa.gt(x(HALF_TURN)) {
        (fa - turn).to_f32()
    } else if fa.lt(x(MINUS_HALF_TURN)) {
        // (the first angle is added to the turn in the register, the second has the turn
        // added from memory: the operands change places)
        if by_memory { (fa + turn).to_f32() } else { (turn + fa).to_f32() }
    } else {
        a
    }
}

/// A point of the skin in a bone's rest frame, as `envelopJoint` writes the product out.
fn bone_frame(t: &Matrix, p: [f32; 3]) -> [f32; 3] {
    let (px, py, pz) = (x(p[0]), x(p[1]), x(p[2]));
    [
        (((py * t[1][0] + px * t[0][0]) + pz * t[2][0]) + t[3][0]).to_f32(),
        (((py * t[1][1] + px * t[0][1]) + pz * t[2][1]) + t[3][1]).to_f32(),
        ((pz * t[2][2] + (py * t[1][2] + px * t[0][2])) + t[3][2]).to_f32(),
    ]
}

/// The bone's matrix turned by `angle` about its z axis through the point's place along the
/// bone (`along`), each of its first three rows then multiplied by `scale`.
fn bent(bone: &Matrix, along: f32, angle: f32, scale: [f32; 3]) -> Matrix {
    let mut b = matrix_copy(bone);
    b[3][0] = (x(b[3][0]) + along).to_f32();
    b = matrix_multiply(&matrix_z_rotation(angle), &b);
    b[3][0] = (x(b[3][0]) - along).to_f32();
    for i in 0..3 {
        for j in 0..3 {
            b[i][j] = (x(b[i][j]) * scale[i]).to_f32();
        }
    }
    b
}

/// Moves the points of a skin that a joint carries, bending the skin around the joint and
/// around the next one [soft 0x084b26cc envelopJoint].
///
/// Before the loop: A and B, the blend lengths of the joint and of its son ([`blend_length`];
/// B is 0 for a son without record). T = skin's matrix × the joint's rest inverse, and T′
/// the same with the son's. a and a′: how far the joint and its son are turned about z from
/// rest (`Model` +0x114 minus the third rest angle; a is 0 for the first joint of a chain
/// when its word is 4, a′ is 0 for an effector or a son without record), each wrapped into
/// −π … π ([`wrapped`]). J and J′: the z rotation by −a (−a′) times the joint's (the son's)
/// global rotation matrix with the global translation in the last row: the bone as it would
/// lie had the joint not turned. (Under classical scaling and for a skin of word 6 the
/// global matrix is used whole.)
///
/// For each vertex (read as in `envelopSkelMdl`), with q = p × T the point in the joint's
/// rest frame and r the vertex's place along the bone (+0x04 of the vertex):
///
/// - **Near the joint** (A above 0.0001 and r not beyond A): t = float(1 − r / A), at most
///   1. The bone's matrix is J turned back by float(a − a × 0.5 × t) about z through q's
///   place along the bone ([`bent`]), its rows scaled by the joint's global scaling (the
///   chain root's `hscl` under classical scaling): at the joint the skin has turned half
///   the joint's angle, at r = A all of it. Not for the first joint of a chain when the
///   word at `SI_glbAnim` +0x2e is 0, the joint's word is 4, or classical scaling is on:
///   there the joint's own matrix is used.
/// - **Near the next joint** (otherwise, B above 0.0001 and r not before 1 − B):
///   t = float(1 − float((1 − r) / B)), at most 1. q is taken anew with T′, and the bone's
///   matrix is J′ turned by float(0.5 × a′ × t), scaled by the son's global scaling: the
///   skin starts into the next joint's turn. When the son is the effector the joint's own
///   matrix is used.
/// - **Between**: the joint's own matrix.
///
/// In every zone, when r lies inside 0 … 1 (in the first: above 0; in the second: below 1),
/// `compDef` answers how far the point is pushed sideways; that is added to the y
/// translation of T (T′). The point's matrix is then T × the bone's matrix × `back`, and
/// the point is moved by it and stored as in `envelopSkelMdl` ([`deposit`]).
pub fn envelop_joint(m: &mut Mem, o: &mut dyn Outside, skin: Node, joint: Node, back: MatPtr, edit: i16) {
    let e = info(m, joint);
    let area = blend_length(m[e].joint_area);
    let to_joint = matrix_multiply(&m[skin].gmat, &m[e].matrix);
    let son = m[joint].son;
    let se = info(m, son);
    let mut son_area = 0.0f32;
    let mut to_son: Matrix = [[0.0; 4]; 4];
    if !se.is_null() {
        son_area = blend_length(m[se].joint_area);
        let se = info(m, son);
        to_son = matrix_multiply(&m[skin].gmat, &m[se].matrix);
    }
    let father = m[joint].father;
    let mut turn = 0.0f32;
    if !(is_chain_root(m, father) && m[joint].envelop == 4) {
        let rest = srt(m, joint);
        turn = (x(m[joint].vrot.z) - m[rest].rot[2]).to_f32();
    }
    let son = m[joint].son;
    let mut son_turn = 0.0f32;
    if !is_effector(m, son) && !info(m, son).is_null() {
        let rest = srt(m, son);
        son_turn = (x(m[son].vrot.z) - m[rest].rot[2]).to_f32();
    }
    let turn = wrapped(turn, false);
    let son_turn = wrapped(son_turn, true);
    let back_turn = matrix_z_rotation(-turn);
    let son_back_turn = matrix_z_rotation(-son_turn);
    let classic = m.i32(SI_GLB_MANIP.addr()) == 1;
    let (straight, son_straight);
    if classic && m[skin].envelop == 6 {
        straight = matrix_multiply(&back_turn, &m[joint].gmat);
        let son = m[joint].son;
        son_straight = matrix_multiply(&son_back_turn, &m[son].gmat);
    } else {
        let mut a = matrix_multiply(&back_turn, &m[joint].grot);
        a[3][0] = fmov(m[joint].gtrs.x);
        a[3][1] = fmov(m[joint].gtrs.y);
        a[3][2] = fmov(m[joint].gtrs.z);
        straight = a;
        let son = m[joint].son;
        let mut b = matrix_multiply(&son_back_turn, &m[son].grot);
        let son = m[joint].son;
        b[3][0] = fmov(m[son].gtrs.x);
        b[3][1] = fmov(m[son].gtrs.y);
        b[3][2] = fmov(m[son].gtrs.z);
        son_straight = b;
    }
    let list = list_of(m, joint, skin);
    if list.is_null() {
        return;
    }
    let mut v = m[list].verts;
    while !v.is_null() {
        let (idx, weight) = (m[v].index, m[v].weight);
        let Some(pt) = fetch(m, skin, idx, weight, edit) else {
            v = m[v].next;
            continue;
        };
        let mut t = matrix_copy(&to_joint);
        let q = bone_frame(&t, pt.p);
        let ratio = m[v].ratio;
        let (fa, r) = (x(area), x(ratio));
        let mut bulge = 0.0f32;
        let mut bone: Option<Matrix> = None;
        let gscl = |m: &Mem, n: Node| [m[n].gscl.x, m[n].gscl.y, m[n].gscl.z];
        let hscl = |m: &mut Mem, o: &mut dyn Outside| {
            let root: Node = Ptr32::new(o.call(m, &SI_SKE_CHAIN_ROOT, &[joint.addr()]));
            [m[root].hscl.x, m[root].hscl.y, m[root].hscl.z]
        };
        if fa.gt(x(SMALL)) && !below_or_nan(fa, r) {
            let mut share = (one() - r / area).to_f32();
            if one().lt(x(share)) {
                share = 1.0;
            }
            let father = m[joint].father;
            let plain = is_chain_root(m, father) && (m[SI_GLB_ANIM][GLB_ANIM_2E] == 0 || m[joint].envelop == 4 || m.i32(SI_GLB_MANIP.addr()) == 1);
            if zero().lt(r) {
                bulge = comp_def(m, joint, ratio, turn, son_turn, q[1], q[2]).to_f32();
            }
            if !plain {
                let angle = (x(turn) - (x(turn) * HALF) * share).to_f32();
                let scale = if m.i32(SI_GLB_MANIP.addr()) == 1 { None } else { Some(gscl(m, joint)) };
                // (the scaling is read after the matrix is made: the chain root is asked last)
                let unscaled = bent(&straight, q[0], angle, [1.0; 3]);
                let scale = match scale {
                    Some(s) => s,
                    None => hscl(m, o),
                };
                let mut b = unscaled;
                for i in 0..3 {
                    for j in 0..3 {
                        b[i][j] = (x(b[i][j]) * scale[i]).to_f32();
                    }
                }
                bone = Some(b);
            }
        } else {
            let fb = x(son_area);
            let mut near_son = false;
            if fb.gt(x(SMALL)) {
                let edge = one() - fb;
                if !above_or_nan(edge, r) {
                    near_son = true;
                    let u = ((one() - r) / fb).to_f32();
                    let mut share = (one() - x(u)).to_f32();
                    if one().lt(x(share)) {
                        share = 1.0;
                    }
                    let son = m[joint].son;
                    if is_effector(m, son) {
                        if one().gt(r) {
                            bulge = comp_def(m, joint, ratio, turn, son_turn, q[1], q[2]).to_f32();
                        }
                    } else {
                        t = matrix_copy(&to_son);
                        let q = bone_frame(&t, pt.p);
                        if r.lt(one()) {
                            bulge = comp_def(m, joint, ratio, turn, son_turn, q[1], q[2]).to_f32();
                        }
                        let angle = ((x(HALF) * son_turn) * share).to_f32();
                        let unscaled = bent(&son_straight, q[0], angle, [1.0; 3]);
                        let scale = if m.i32(SI_GLB_MANIP.addr()) == 1 {
                            hscl(m, o)
                        } else {
                            let son = m[joint].son;
                            gscl(m, son)
                        };
                        let mut b = unscaled;
                        for i in 0..3 {
                            for j in 0..3 {
                                b[i][j] = (x(b[i][j]) * scale[i]).to_f32();
                            }
                        }
                        bone = Some(b);
                    }
                }
            }
            if !near_son && zero().lt(r) && one().gt(r) {
                bulge = comp_def(m, joint, ratio, turn, son_turn, q[1], q[2]).to_f32();
            }
        }
        t[3][1] = (x(bulge) + t[3][1]).to_f32();
        let moved = match bone {
            Some(b) => matrix_multiply(&t, &b),
            None => matrix_multiply(&t, &m[joint].gmat),
        };
        let mm = matrix_multiply(&moved, &m.read(back));
        deposit(m, o, &pt, &mm, edit);
        v = m[v].next;
    }
}

/// Puts back the points no bone has moved [soft 0x084b1ce4 unassVertCopy]: every point of
/// the skin whose word in `damptab` is 0 is copied from the first stored definition to the
/// working one (a patch point's four words; a mesh vertex's position; a spline key's middle
/// point; a NURBS control point's 0x24 bytes). Other kinds: nothing.
pub fn unass_vert_copy(m: &mut Mem, skin: Node) {
    let kind = m[skin].kind;
    if !matches!(kind, 1 | 4 | 6 | 9 | 10) {
        return;
    }
    let def0 = m[m[skin].defs].addr();
    let work = m[skin].definition.addr();
    let (n, points, stride, at, words): (i32, u32, u32, u32, u32) = match kind {
        1 => ((m.u16(def0.wrapping_add(geom::PATCH_NU)) as i32).wrapping_mul(m.u16(def0.wrapping_add(geom::PATCH_NV)) as i32), geom::PATCH_POINTS, geom::PATCH_POINT_SIZE, 0, 4),
        4 => (m.i32(def0.wrapping_add(geom::MESH_NB_VERTS)), geom::MESH_VERTS, geom::VERT_SIZE, 0, 3),
        6 => (m.u16(def0.wrapping_add(geom::SPLINE_NB_KEYS)) as i32, geom::SPLINE_KEYS, geom::KEY_SIZE, geom::KEY_POINT, 3),
        9 => (m.i32(def0.wrapping_add(geom::CRV_NB_POINTS)), geom::CRV_POINTS, geom::CP_SIZE, 0, 9),
        _ => (m.i32(def0.wrapping_add(geom::SRF_NB_POINTS)), geom::SRF_POINTS, geom::CP_SIZE, 0, 9),
    };
    let mut i: i32 = 0;
    while i < n {
        let damp = m[DAMPTAB].add(i);
        if m[damp] == 0 {
            let off = (i as u32).wrapping_mul(stride).wrapping_add(at);
            let from = m.u32(def0.wrapping_add(points)).wrapping_add(off);
            let to = m.u32(work.wrapping_add(points)).wrapping_add(off);
            for k in 0..words {
                let word = m.u32(from.wrapping_add(4 * k));
                m.set_u32(to.wrapping_add(4 * k), word);
            }
        }
        i += 1;
    }
}

/// Offset in `SI_glbManip` of the 16-bit "keep whole turns" setting the extraction of
/// angles from a matrix reads (`si-math`'s `matrix_extract_xyz_rotation`).
pub const GLB_MANIP_KEEP_TURNS: u32 = 0x20;

fn keep_turns(m: &Mem) -> bool {
    m.i16(SI_GLB_MANIP.addr().wrapping_add(GLB_MANIP_KEEP_TURNS)) != 0
}

/// What one bone asks of a node it carries rigidly (a skin of word 7)
/// [soft 0x084b54b8 envelopJntAxisMap]; the answer is gathered in `curvect`: scaling,
/// rotation and translation, nine floats.
///
/// Nothing unless the bone has a list for the node with a first vertex, and the node has a
/// rest transformation. The node's rest matrix (`SI_matrixScalRotTrans` of its global
/// scaling, or +0x3a0 under classical scaling, and its rest rotation and translation) goes
/// through the bone's rest inverse and the bone's present matrix; scaling, angles (nearest
/// to the rest angles) and translation are taken out of the product again. With the first
/// vertex's weight at 99.99 or more these nine floats replace `curvect`. Otherwise only
/// rotation and translation move, each by (new − rest) × float(weight × 0.01), added to
/// what `curvect` holds: the first coordinate of each with the product kept in the
/// register, the other two with it rounded first.
pub fn envelop_jnt_axis_map(m: &mut Mem, node: Node, bone: Node) {
    let list = list_of(m, bone, node);
    if list.is_null() || m[list].verts.is_null() {
        return;
    }
    let rest = srt(m, node);
    if rest.is_null() {
        return;
    }
    let scale = if m.i32(SI_GLB_MANIP.addr()) == 1 { m[node].cscl } else { m[node].gscl };
    let (rest_rot, rest_trans) = (m[rest].rot, m[rest].trans);
    let own = si_math::algebra::matrix_scal_rot_trans(&Vector::new(scale.x, scale.y, scale.z), &vector(rest_rot), &vector(rest_trans));
    let e = info(m, bone);
    let at_rest = matrix_multiply(&own, &m[e].matrix);
    let now = matrix_multiply(&at_rest, &m[bone].gmat);
    let (scl, rot, trs) = si_math::algebra::matrix_extract_transformations(&now, &vector(rest_rot), keep_turns(m));
    let (scl, rot, trs) = (scl.to_array(), rot.to_array(), trs.to_array());
    let first = m[list].verts;
    let weight = m[first].weight;
    let cv = m[CURVECT];
    if !x(weight).lt(x(FULL)) {
        m[cv] = [scl[0], scl[1], scl[2], rot[0], rot[1], rot[2], trs[0], trs[1], trs[2]];
        return;
    }
    let f = (x(weight) * PERCENT).to_f32();
    let rest = srt(m, node);
    for (base, new, old) in [(3usize, rot, m[rest].rot), (6usize, trs, m[rest].trans)] {
        let d: [F80; 3] = [x(new[0]) - old[0], x(new[1]) - old[1], x(new[2]) - old[2]];
        let first = d[0] * f;
        let second = (d[1] * f).to_f32();
        // (the third product is formed the other way round for the translation: the
        // operands of a multiplication change places, the value does not)
        let third = if base == 3 { (d[2] * f).to_f32() } else { (x(f) * d[2]).to_f32() };
        let cv = m[CURVECT];
        m[cv][base] = (first + m[cv][base]).to_f32();
        m[cv][base + 1] = (x(m[cv][base + 1]) + second).to_f32();
        m[cv][base + 2] = (x(m[cv][base + 2]) + third).to_f32();
    }
}

/// Walks a skeleton and lets every bone of the right level move its points of a skin
/// [soft 0x084b5728 envelopChain]. `level` is the level expected of `node`; the walk goes
/// to the first son one level down and, when `brothers` is set, to the next brother.
///
/// A node of the wrong level is passed over with its sons. For a global skin (word 6 or 7)
/// a node of word 11, and a chain's root whose first son has the word 4, are passed over
/// with their sons too: those carry a local envelope. Otherwise: a skin of word 7 is
/// carried rigidly ([`envelop_jnt_axis_map`]); a joint bends the skin
/// ([`envelop_joint`]), except while a sequence plays (`SI_glbAnim` +0x38) and the joint has
/// no blend to make (its blend length not above 0.0001, its son's, when the son has a
/// record, not above 0.0001 either, and no kind in either profile): then, and for every
/// node that is not a joint and whose word is not 11, the points are moved by the bone's
/// matrix alone ([`envelop_skel_mdl`]).
pub fn envelop_chain(m: &mut Mem, o: &mut dyn Outside, skin: Node, node: Node, back: MatPtr, edit: i16, level: i32, brothers: i16) {
    if envelop_level(m, node) == level {
        let w = m[skin].envelop;
        let global = (w as u32).wrapping_sub(6) <= 1;
        let mut skip = false;
        if global {
            if m[node].envelop == 11 {
                skip = true;
            } else if is_chain_root(m, node) {
                let son = m[node].son;
                skip = m[son].envelop == 4;
            }
        }
        if !skip {
            if w == 7 {
                envelop_jnt_axis_map(m, skin, node);
            } else if m[node].kind == 5 {
                let mut rigid = false;
                if m[SI_GLB_ANIM][GLB_ANIM_PLAYING] != 0 {
                    let e = info(m, node);
                    let small = x(SMALL);
                    if !x(m[e].joint_area).gt(small) {
                        let son = m[node].son;
                        let se = info(m, son);
                        if se.is_null() || !small.lt(x(m[se].joint_area)) {
                            rigid = m[e].inner.kind == 0 && m[e].outer.kind == 0;
                        }
                    }
                }
                if rigid {
                    envelop_skel_mdl(m, o, skin, node, back, edit);
                } else {
                    envelop_joint(m, o, skin, node, back, edit);
                }
            } else if m[node].envelop != 11 {
                envelop_skel_mdl(m, o, skin, node, back, edit);
            }
            let son = m[node].son;
            if !son.is_null() {
                envelop_chain(m, o, skin, son, back, edit, level.wrapping_add(1), 1);
            }
        }
    }
    let brother = m[node].brother;
    if !brother.is_null() && brothers != 0 {
        envelop_chain(m, o, skin, brother, back, edit, level, 1);
    }
}

/// Deforms one skin by its skeleton [soft 0x084b5894 envelopSkel]. `edit` is not 0 while
/// the user moves points of the skin by hand: the working definition is then left alone
/// and the bones' matrices are gathered in `editvert` instead.
///
/// Nothing for a skin whose word at +0x0c is not 0. A skin of word 7 (a node carried
/// rigidly) must not be the main node of a global envelope and must have a rest
/// transformation; `curvect` then gets nine floats from `SI_calloc`: the node's global
/// scaling (+0x3a0 under classical scaling) and its rest rotation and translation. Any
/// other skin gets `damptab` (unless editing; the routine gives up when the table cannot be
/// made) and a working definition of its own (`SI_envelopCopyDef`).
///
/// Then the skeleton is walked ([`envelop_chain`]) with the inverse of the skin's global
/// matrix (a local of the original that keeps what the stack held when there is no
/// inverse), level 0, and its brothers too for a global skin (word 6 or 7).
///
/// Afterwards, for word 7: when the node has a father, its local rotation matrix becomes
/// the rotation of `curvect`'s angles times the inverse of the father's global rotation
/// matrix, its angles are taken from that, and its translation is `curvect`'s translation
/// through the inverse of the father's global matrix; `curvect` is freed. For any other
/// skin, unless editing: the points no bone moved are put back ([`unass_vert_copy`]); a
/// patch is tessellated (`SI_patchComputeSurface (0x0847c1d0)`, with bit 0 of its byte at
/// +0x1e4 set first while the user drags and `SI_fastIntenv` is on), a mesh gets its
/// normals ([`envelop_mesh_comp_normals`]), a NURBS curve or surface the bits 0x60 in its
/// flag byte; `SI_modelTouchDefinition (0x0846d530)`; `damptab` is freed.
pub fn envelop_skel(m: &mut Mem, o: &mut dyn Outside, skin: Node, root: Node, edit: i16) {
    if skin.is_null() || root.is_null() || m[skin].bbox_valid != 0 {
        return;
    }
    if m[skin].envelop == 7 {
        if main_glob_env(m, skin) as i16 != 0 {
            return;
        }
        let rest = srt(m, skin);
        if rest.is_null() {
            return;
        }
        let cv: Ptr32<[f32; 9]> = Ptr32::new(m.si_calloc(1, 0x24));
        m[CURVECT] = cv;
        let scale = if m.i32(SI_GLB_MANIP.addr()) == 1 { m[skin].cscl } else { m[skin].gscl };
        // (copied as words)
        let (rot, trans) = (m[rest].rot, m[rest].trans);
        m[cv][0] = scale.x;
        m[cv][1] = scale.y;
        m[cv][2] = scale.z;
        let cv = m[CURVECT];
        for k in 0..3 {
            m[cv][3 + k] = rot[k];
            m[cv][6 + k] = trans[k];
        }
    } else {
        if edit == 0 && damp_tab_create(m, skin) as i16 == 0 {
            return;
        }
        envelop_copy_def(m, o, skin, 0);
    }
    let back = unset_matrix(m);
    let own = m[skin].gmat;
    if let Some(inverse) = matrix_inverse(&own) {
        m.write(back, inverse);
    }
    let global = (m[skin].envelop as u32).wrapping_sub(6) <= 1;
    envelop_chain(m, o, skin, root, back, edit, 0, global as i16);
    if m[skin].envelop == 7 {
        let father = m[skin].father;
        if !father.is_null() {
            let cv = m[CURVECT];
            let c = m[cv];
            let turned = matrix_xyz_rotation(&Vector::new(c[3], c[4], c[5]));
            // the inverse of the father's rotation, then of its matrix, in one local: each
            // stays as it was when there is no inverse (at first: what the stack held)
            let up_at = unset_matrix(m);
            let grot = m[father].grot;
            if let Some(inverse) = matrix_inverse(&grot) {
                m.write(up_at, inverse);
            }
            let up = m.read(up_at);
            m[skin].mrot = si_math::algebra::matrix_multiply_sub3(&turned, &up);
            let (mrot, previous) = (m[skin].mrot, m[skin].vrot);
            let angles = si_math::algebra::matrix_extract_xyz_rotation(&mrot, &Vector::new(previous.x, previous.y, previous.z), keep_turns(m));
            m[skin].vrot.x = angles.x;
            m[skin].vrot.y = angles.y;
            m[skin].vrot.z = angles.z;
            let gmat = m[father].gmat;
            if let Some(inverse) = matrix_inverse(&gmat) {
                m.write(up_at, inverse);
            }
            let up = m.read(up_at);
            let cv = m[CURVECT];
            let c = m[cv];
            let (tx, ty, tz) = (x(c[6]), x(c[7]), x(c[8]));
            m[skin].trans.x = (((ty * up[1][0] + tx * up[0][0]) + tz * up[2][0]) + up[3][0]).to_f32();
            m[skin].trans.y = (((ty * up[1][1] + tx * up[0][1]) + tz * up[2][1]) + up[3][1]).to_f32();
            m[skin].trans.z = (((ty * up[1][2] + tx * up[0][2]) + tz * up[2][2]) + up[3][2]).to_f32();
            m.release(0x40);
        }
        let cv = m[CURVECT];
        if !cv.is_null() {
            m.free(cv.addr());
            m[CURVECT] = Ptr32::NULL;
        }
        m.release(0x40);
        return;
    }
    m.release(0x40);
    if edit != 0 {
        return;
    }
    unass_vert_copy(m, skin);
    let work = m[skin].definition.addr();
    match m[skin].kind {
        1 => {
            if m[SI_INTERACTIVE] != 0 && m[SI_FAST_INTENV] != 0 {
                m.or_u8(work.wrapping_add(PATCH_FLAGS_1E4), 1);
            }
            o.call(m, &crate::outside::SI_PATCH_COMPUTE_SURFACE, &[work, 0]);
        }
        4 => envelop_mesh_comp_normals(m, o, work),
        9 => m.or_u8(work.wrapping_add(geom::CRV_FLAGS), geom::NURBS_MOVED),
        10 => m.or_u8(work.wrapping_add(geom::SRF_FLAGS), geom::NURBS_MOVED),
        _ => {}
    }
    o.call(m, &crate::env_outside::SI_MODEL_TOUCH_DEFINITION, &[skin.addr()]);
    let tab = m[DAMPTAB];
    if !tab.is_null() {
        m.free(tab.addr());
        m[DAMPTAB] = Ptr32::NULL;
    }
}

/// Brings the rest matrices of a skeleton up to date [soft 0x084b5bf4 SI_envelopSkelUpdate]:
/// every bone's record gets the inverse of the bone's matrix in the rest position, placed
/// under the skeleton's father as that father stands now.
///
/// Nothing for a node without record or without skeleton root ([`skel_root_get`]). While a
/// sequence plays and the scene update is inside an animation step (`SI_glbAnim` +0x38 and
/// +0x06), bit 2 of the root's flag byte (+0x08) is set: "done for this frame".
///
/// - **A chain whose first joint has the word 4** (the skeleton of a local envelope): the
///   chain is solved (`SI_modelSpecialUpdateSonState (0x084ad480)`), the first joint's sons
///   are updated with bit 1 of `SI_viewerUpdOpt` set for the duration
///   (`SI_modelUpdateSonState (0x084662f8)`), an automatic envelope's rest rotations are
///   parked ([`env_auto_set`]), and the rest matrices are made from the first joint down
///   with the first joint's own global matrix as the father's ([`env_update_matrix`]).
/// - **A root of word 11**: the rest matrices from its first son down, under the root's
///   global matrix. Nothing when the first son is missing or has no record.
/// - **A root of word 4 or 10** (the skeleton of a global envelope; when the root is not its
///   father's first son, the walk starts at the skeleton root found from the father): for
///   the root and each of its brothers of level 0, the root's rest matrix is made here:
///   the rotation of its rest angles times the father's global rotation matrix
///   (`SI_matrixMultiplySub3`), its rows scaled by rest scaling × the father's global
///   scaling (+0x3a0 under classical scaling), and in the last row the rest translation
///   through the father's global matrix. For a father of word 7 that has a rest
///   transformation, the father's *rest* rotation, scaling and translation take the place
///   of its present ones. The inverse goes to the root's record and the sons follow
///   ([`env_update_matrix`]) when the first son has a record.
pub fn envelop_skel_update(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    if envelop_level(m, node) < 0 {
        return;
    }
    let mut root = skel_root_get(m, node);
    if root.is_null() {
        return;
    }
    if m[SI_GLB_ANIM][GLB_ANIM_PLAYING] != 0 && m[SI_GLB_ANIM][GLB_ANIM_06] != 0 {
        m.or_u8(root.addr().wrapping_add(8), 4);
    }
    let first = m[root].son;
    if is_chain_root(m, root) && m[first].envelop == 4 {
        let saved = m[SI_VIEWER_UPD_OPT];
        env_local_get(m, root);
        let first = m[root].son;
        o.call(m, &crate::env_outside::SI_MODEL_SPECIAL_UPDATE_SON_STATE, &[0, first.addr()]);
        let first = m[root].son;
        m.or_u8(SI_VIEWER_UPD_OPT.addr(), 2);
        let mut s = m[first].son;
        while !s.is_null() {
            let father = m[s].father;
            o.call(m, &crate::env_outside::SI_MODEL_UPDATE_SON_STATE, &[father.addr(), s.addr(), 1]);
            s = m[s].brother;
        }
        m[SI_VIEWER_UPD_OPT] = saved;
        env_auto_set(m, root, 1);
        let first = m[root].son;
        let level = m[info(m, first)].level;
        let first = m[root].son;
        env_update_matrix(m, first, si_mem::field!(first, Model, gmat), level);
        return;
    }
    let w = m[root].envelop;
    if w == 11 {
        env_local_get(m, root);
        let first = m[root].son;
        if first.is_null() {
            return;
        }
        let e = info(m, first);
        if e.is_null() {
            return;
        }
        let level = m[e].level;
        let first = m[root].son;
        env_update_matrix(m, first, si_mem::field!(root, Model, gmat), level);
        return;
    }
    if w != 4 && w != 10 {
        return;
    }
    let father = m[root].father;
    if m[father].son != root {
        root = envelop_skel_root(m, father);
        if root.is_null() {
            return;
        }
    }
    while !root.is_null() {
        if envelop_level(m, root) == 0 {
            let rest = srt(m, root);
            let up = srt(m, father);
            let scale: [f32; 3];
            let mut mine: Matrix;
            let place: [f32; 3];
            if m[father].envelop == 7 && !up.is_null() {
                let (us, ms) = (m[up].scale, m[rest].scale);
                scale = [(x(us[0]) * ms[0]).to_f32(), (x(us[1]) * ms[1]).to_f32(), (x(us[2]) * ms[2]).to_f32()];
                let up_rot = matrix_xyz_rotation(&vector(m[up].rot));
                let my_rot = matrix_xyz_rotation(&vector(m[rest].rot));
                mine = si_math::algebra::matrix_multiply_sub3(&my_rot, &up_rot);
                // the father's rest matrix, row by row: only its first element is used
                // before it is rounded
                let us = m[up].scale;
                let corner = x(up_rot[0][0]) * us[0];
                let r = |i: usize, j: usize| (x(up_rot[i][j]) * us[i]).to_f32();
                let (ut, t) = (m[up].trans, m[rest].trans);
                let (tx, ty, tz) = (x(t[0]), x(t[1]), x(t[2]));
                place = [
                    (x(ut[0]) + ((ty * r(1, 0) + corner * tx) + tz * r(2, 0))).to_f32(),
                    (x(ut[1]) + (tz * r(2, 1) + (ty * r(1, 1) + tx * r(0, 1)))).to_f32(),
                    (x(ut[2]) + ((ty * r(1, 2) + tx * r(0, 2)) + tz * r(2, 2))).to_f32(),
                ];
            } else {
                let fs = if m.i32(SI_GLB_MANIP.addr()) == 1 { m[father].cscl } else { m[father].gscl };
                let ms = m[rest].scale;
                scale = [(x(fs.x) * ms[0]).to_f32(), (x(fs.y) * ms[1]).to_f32(), (x(fs.z) * ms[2]).to_f32()];
                let my_rot = matrix_xyz_rotation(&vector(m[rest].rot));
                mine = si_math::algebra::matrix_multiply_sub3(&my_rot, &m[father].grot);
                let g = m[father].gmat;
                let t = m[rest].trans;
                let (tx, ty, tz) = (x(t[0]), x(t[1]), x(t[2]));
                place = [
                    (((ty * g[1][0] + tx * g[0][0]) + tz * g[2][0]) + g[3][0]).to_f32(),
                    (((ty * g[1][1] + tx * g[0][1]) + tz * g[2][1]) + g[3][1]).to_f32(),
                    (((ty * g[1][2] + tx * g[0][2]) + tz * g[2][2]) + g[3][2]).to_f32(),
                ];
            }
            for i in 0..3 {
                for j in 0..3 {
                    mine[i][j] = (x(mine[i][j]) * scale[i]).to_f32();
                }
                mine[3][i] = fmov(place[i]);
            }
            let at: MatPtr = Ptr32::new(m.local(0x40));
            m.write(at, mine);
            if let Some(inverse) = matrix_inverse(&mine) {
                let e = info(m, root);
                m[e].matrix = inverse;
            }
            let first = m[root].son;
            if !first.is_null() {
                let e = info(m, first);
                if !e.is_null() {
                    let level = m[e].level;
                    let first = m[root].son;
                    env_update_matrix(m, first, at, level);
                }
            }
            m.release(0x40);
        }
        root = m[root].brother;
    }
}

/// Deforms a skin [soft 0x084b6210 SI_envelopCompUpdate]: what the scene update calls for a
/// node whose envelope must follow its skeleton at this frame.
///
/// Nothing for a node without skeleton ([`envelop_skel_root`]). The skeleton's rest
/// matrices are brought up to date first ([`envelop_skel_update`]), except while a sequence
/// plays (`SI_glbAnim` +0x38) and the root is marked "done for this frame" (bit 2 of its
/// byte at +0x08); a local skin (word 5 or 8) whose root's word is not 11 always updates.
/// Then the skin is deformed ([`envelop_skel`]). Afterwards a node carried rigidly (word 7)
/// has its matrices rebuilt (`SI_modelUpdateSonState (0x084662f8)` from its father), and
/// for a local skin whose root's word is not 11 the parked rest rotations are put back
/// ([`env_auto_set`]) and the root's matrices rebuilt.
pub fn envelop_comp_update(m: &mut Mem, o: &mut dyn Outside, skin: Node) {
    let root = envelop_skel_root(m, skin);
    if root.is_null() {
        return;
    }
    let local = |m: &Mem| matches!(m[skin].envelop, 5 | 8) && m[root].envelop != 11;
    let done = m[SI_GLB_ANIM][GLB_ANIM_PLAYING] != 0 && !local(m) && m.u8(root.addr().wrapping_add(8)) & 4 != 0;
    if !done {
        envelop_skel_update(m, o, root);
    }
    envelop_skel(m, o, skin, root, 0);
    let w = m[skin].envelop;
    if w == 7 {
        let father = m[skin].father;
        o.call(m, &crate::env_outside::SI_MODEL_UPDATE_SON_STATE, &[father.addr(), skin.addr(), 1]);
    } else if local(m) {
        env_auto_set(m, root, 0);
        let father = m[root].father;
        o.call(m, &crate::env_outside::SI_MODEL_UPDATE_SON_STATE, &[father.addr(), root.addr(), 1]);
    }
}

/// Deforms every skin of a skeleton [soft 0x084b62d0 SI_envelopHierUpdate]: for a bone
/// whose skeleton root has lists, the rest matrices are brought up to date
/// ([`envelop_skel_update`]) and each skin of the root's lists is deformed
/// ([`envelop_skel`]), from the last list to the first; a skin of word 7 then has its
/// matrices rebuilt. For a chain whose first joint has the word 4 the parked rest rotations
/// are put back afterwards ([`env_auto_set`]) and the root's matrices rebuilt.
pub fn envelop_hier_update(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    if envelop_level(m, node) < 0 {
        return;
    }
    let root = skel_root_get(m, node);
    if root.is_null() {
        return;
    }
    let e = info(m, root);
    if e.is_null() || m[e].lists.is_null() {
        return;
    }
    envelop_skel_update(m, o, root);
    let e = info(m, root);
    let mut l = m[e].lists;
    while !m[l].next.is_null() {
        l = m[l].next;
    }
    while !l.is_null() {
        let skin = m[l].owner;
        if !skin.is_null() {
            envelop_skel(m, o, skin, root, 0);
            let skin = m[l].owner;
            if m[skin].envelop == 7 {
                let skin = m[l].owner;
                let father = m[skin].father;
                o.call(m, &crate::env_outside::SI_MODEL_UPDATE_SON_STATE, &[father.addr(), skin.addr(), 1]);
            }
        }
        l = m[l].prev;
    }
    let first = m[root].son;
    if is_chain_root(m, root) && m[first].envelop == 4 {
        env_auto_set(m, root, 0);
        let father = m[root].father;
        o.call(m, &crate::env_outside::SI_MODEL_UPDATE_SON_STATE, &[father.addr(), root.addr(), 1]);
    }
}

// ------------------------------------------------ third pass, deform2: the earlier envelope format

/// `Model` +0x34c, for a skin read from a file below version 2.6: the block the earlier
/// format's matrices hang from. Its word at +0xc is an array of pointers, one per list of
/// the skeleton's first node, each to 0x40-byte matrices, one per point of that list (finding
/// 0029, section 18). Addressed by offset: `si-types` calls the field `user_data`.
const OLD_BLOCK: u32 = 0x34c;
const OLD_MATS: u32 = 0xc;

/// A point multiplied by a matrix the way `oldenvelopJoint` does, on the unit: for each
/// coordinate `((x m0 + y m1) + z m2) + m3` (products of floats are exact in 80 bits, each
/// sum rounds to 64 significand bits, the result is stored as a float).
fn old_xform(p: [f32; 3], t: &Matrix) -> [f32; 3] {
    let mut r = [0.0f32; 3];
    for c in 0..3 {
        r[c] = (((x(p[0]) * t[0][c] + x(p[1]) * t[1][c]) + x(p[2]) * t[2][c]) + x(t[3][c])).to_f32();
    }
    r
}

/// Brings the points of a skin of the earlier envelope format into place
/// [soft 0x084b63d4 oldenvelopJoint]: for each list of the node's envelope record (the
/// node is the skeleton's first), each point of the list (counted from 0 in a 16-bit
/// counter, which also selects the point's matrix from the list's array) that exists in the
/// skin's first stored definition is multiplied by its matrix; a spline of type 1 has its
/// two tangent points multiplied too, translation included. The skin kinds are the
/// original's 1 (patch), 4 (mesh), 6 (spline); a point number past the definition's count
/// leaves the point alone (its matrix is still counted). Afterwards the array of matrix
/// pointers and the block that held it are freed (the matrices themselves are not) and the
/// node's pointer to the block is cleared. Nothing is done when the node has no block or
/// the block no array.
pub fn oldenvelop_joint(m: &mut Mem, node: Node) {
    let block = m.u32(node.addr() + OLD_BLOCK);
    if block == 0 {
        return;
    }
    let arrays = m.u32(block + OLD_MATS);
    if arrays == 0 {
        return;
    }
    let mut list = m[info(m, node)].lists;
    let mut idx: u32 = 0;
    while !list.is_null() {
        let mats = m.u32(arrays.wrapping_add(idx.wrapping_mul(4)));
        idx = idx.wrapping_add(1);
        let skin = m[list].owner;
        let mut v = m[list].verts;
        if mats != 0 && !skin.is_null() && !v.is_null() {
            let mut k: i16 = 0;
            while !v.is_null() {
                old_point(m, skin, m[v].index, mats, k);
                k = k.wrapping_add(1);
                v = m[v].next;
            }
        }
        list = m[list].next;
    }
    let block = m.u32(node.addr() + OLD_BLOCK);
    let arrays = m.u32(block + OLD_MATS);
    if arrays != 0 {
        m.free(arrays);
        let block = m.u32(node.addr() + OLD_BLOCK);
        m.set_u32(block + OLD_MATS, 0);
    }
    let block = m.u32(node.addr() + OLD_BLOCK);
    if block != 0 {
        m.free(block);
        m.set_u32(node.addr() + OLD_BLOCK, 0);
    }
}

/// One point of `oldenvelopJoint`'s walk: read, multiplied, written back.
fn old_point(m: &mut Mem, skin: Node, index: i32, mats: u32, k: i16) {
    let kind = m[skin].kind;
    let geom_of = |m: &Mem| m.u32(m.u32(skin.addr() + 0x38));
    let rd = |m: &Mem, a: u32| [m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8))];
    let mut p = [0.0f32; 3];
    let (mut t1, mut t2) = ([0.0f32; 3], [0.0f32; 3]);
    let (mut two, mut at) = (false, 0u32);
    match kind {
        1 => {
            // patch
            let g = geom_of(m);
            let n = (m.u16(g.wrapping_add(0xc)) as i32).wrapping_mul(m.u16(g.wrapping_add(0xe)) as i32);
            if index >= n {
                return;
            }
            at = m.u32(g.wrapping_add(0x1a4)).wrapping_add((index as u32) << 4);
            let q = rd(m, at);
            p = [fmov(q[0]), fmov(q[1]), fmov(q[2])];
        }
        4 => {
            // mesh
            let g = geom_of(m);
            if index >= m.i32(g.wrapping_add(0x14)) {
                return;
            }
            at = m.u32(g).wrapping_add((index as u32).wrapping_mul(0x14));
            p = rd(m, at);
        }
        6 => {
            // spline
            let g = geom_of(m);
            if index >= m.u16(g.wrapping_add(0x10)) as i32 {
                return;
            }
            at = m.u32(g.wrapping_add(0x14)).wrapping_add((index as u32).wrapping_mul(0x30));
            p = rd(m, at.wrapping_add(0xc));
            if m.i32(g.wrapping_add(0xc)) == 1 {
                two = true;
                t1 = rd(m, at);
                t2 = rd(m, at.wrapping_add(0x18));
            }
        }
        _ => {}
    }
    let mut src = mats.wrapping_add(((k as i32) << 6) as u32);
    if src == 0 {
        src = SI_IDENTITE.addr();
    }
    let t = matrix_copy(&m.read(Ptr32::<Matrix>::new(src)));
    let q = old_xform(p, &t);
    match kind {
        1 => {
            wr3(m, at, q);
            m.set_f32(at.wrapping_add(12), 1.0);
        }
        4 => wr3(m, at, q),
        6 => {
            wr3(m, at.wrapping_add(0xc), q);
            if two {
                wr3(m, at, old_xform(t1, &t));
                wr3(m, at.wrapping_add(0x18), old_xform(t2, &t));
            }
        }
        _ => {}
    }
}

/// The earlier format's matrices applied along a chain [soft 0x084b6958 oldenvelopChain]:
/// `oldenvelopJoint` for every node under the root down to (not including) the effector
/// (a null whose algorithm is 3 or 4), then for each son of the effector whose envelope level
/// is `level + 1` the same, recursively (the second argument is the level the recursion
/// found, which is that number). The walk reads through null where a chain has no effector.
pub fn oldenvelop_chain(m: &mut Mem, chain: Node, level: i32) {
    let mut n = m[chain].son;
    while !is_effector(m, n) {
        oldenvelop_joint(m, n);
        n = m[n].son;
    }
    n = m[n].son;
    if !n.is_null() {
        let want = level.wrapping_add(1);
        loop {
            let l = envelop_level(m, n);
            if l == want {
                oldenvelop_chain(m, n, l);
            }
            n = m[n].brother;
            if n.is_null() {
                break;
            }
        }
    }
}

/// Resets a skin of the earlier envelope format [soft 0x084b69e4 si_oldenvelopReset]: from
/// the node, up through `SI_skeChainRoot (0x081d20c0)` of each father until a node of level
/// 0 is found (none: nothing); when that node's first son has the word 4 or it has it
/// itself, the chain's matrices are applied ([`oldenvelop_chain`] with level 0) and every
/// skin in the first son's lists loses its working definition and is given a fresh copy
/// ([`envelop_disp_def`], [`envelop_copy_def`] with 0).
pub fn oldenvelop_reset(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    let mut n = node;
    if envelop_level(m, n) <= -1 {
        return;
    }
    while envelop_level(m, n) != 0 {
        let father = m[n].father;
        n = Ptr32::new(o.call(m, &SI_SKE_CHAIN_ROOT, &[father.addr()]));
        if n.is_null() {
            return;
        }
    }
    if m[m[n].son].envelop != crate::env_records::word::CHAIN_FIRST && m[n].envelop != crate::env_records::word::CHAIN_FIRST {
        return;
    }
    oldenvelop_chain(m, n, 0);
    let mut l = m[info(m, m[n].son)].lists;
    while !l.is_null() {
        let skin = m[l].owner;
        if !skin.is_null() {
            envelop_disp_def(m, o, skin);
            envelop_copy_def(m, o, skin, 0);
        }
        l = m[l].next;
    }
}

/// Clears the "edited" mark of every point of a skin's working definition
/// [soft 0x084b6aa4 envVrtResetEditFlag]: bit 15 of a patch point's 16-bit flag, bit 5 of a
/// mesh vertex's flag byte, bit 15 of a spline key's flag word, bit 7 of the flag byte of a
/// NURBS curve's or surface's control point. Other kinds, and no node, do nothing.
pub fn env_vrt_reset_edit_flag(m: &mut Mem, node: Node) {
    if node.is_null() {
        return;
    }
    let d = m[node].definition.addr();
    match m[node].kind {
        1 => {
            let n = (m.u16(d.wrapping_add(0xc)) as i32).wrapping_mul(m.u16(d.wrapping_add(0xe)) as i32);
            let mut i = 0i32;
            while i < n {
                let a = m.u32(d.wrapping_add(0x1a8)).wrapping_add((i as u32).wrapping_mul(2));
                let v = m.u16(a) & 0x7fff;
                m.set_u16(a, v);
                i += 1;
            }
        }
        4 => {
            let n = m.i32(d.wrapping_add(0x14));
            let mut i = 0i32;
            while i < n {
                let a = m.u32(d).wrapping_add((i as u32).wrapping_mul(0x14)).wrapping_add(0x12);
                m.and_u8(a, 0xdf);
                i += 1;
            }
        }
        6 => {
            let n = m.u16(d.wrapping_add(0x10));
            for i in 0..n as u32 {
                let a = m.u32(d.wrapping_add(0x14)).wrapping_add(i.wrapping_mul(0x30)).wrapping_add(0x2c);
                let v = m.u16(a) & 0x7fff;
                m.set_u16(a, v);
            }
        }
        9 => {
            let n = m.i32(d.wrapping_add(4));
            let mut i = 0i32;
            while i < n {
                let a = m.u32(d.wrapping_add(0x2c)).wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_add(0x23);
                m.and_u8(a, 0x7f);
                i += 1;
            }
        }
        10 => {
            let n = m.i32(d.wrapping_add(0x48));
            let mut i = 0i32;
            while i < n {
                let a = m.u32(d.wrapping_add(0x54)).wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_add(0x23);
                m.and_u8(a, 0x7f);
                i += 1;
            }
        }
        _ => {}
    }
}

/// The element of `editvert` for point `i` with its matrix made ready for use
/// [soft 0x084b6b9c SI_envelopUpdateInitDef]: a total weight below 99.99 adds
/// `1 - 0.01 * weight` to each of the four diagonal elements (in the element, which keeps it),
/// then the matrix is inverted into `inv` (when it has no inverse `inv` is left as it was).
/// The two constants are the copies [`FULL`] and [`PERCENT`] have at 0x08660fb0 and 0x08660fb8.
/// `false` when no element is there.
fn init_def_element(m: &mut Mem, i: i32, inv: &mut Matrix) -> bool {
    let mut ev = m[EDITVERT];
    if ev.is_null() {
        return false;
    }
    while m[ev].index != i {
        ev = m[ev].next;
        if ev.is_null() {
            return false;
        }
    }
    if x(m[ev].weight).lt(x(FULL)) {
        for k in 0..4 {
            let share = F80::ONE - x(PERCENT) * m[ev].weight;
            m[ev].matrix[k][k] = (share + m[ev].matrix[k][k]).to_f32();
        }
    }
    let mat = m[ev].matrix;
    if let Some(r) = matrix_inverse(&mat) {
        *inv = r;
    }
    true
}

/// A point of four coordinates times the inverse, as the NURBS branches of
/// `SI_envelopUpdateInitDef` form it: the four sums stay in the register (x, y, z, w).
fn init_def_project(p: [f64; 4], v: &Matrix) -> [F80; 4] {
    let (p0, p1, p2, p3) = (x(p[0]), x(p[1]), x(p[2]), x(p[3]));
    [
        (p3 * v[3][0]) + ((p2 * v[2][0]) + ((p1 * v[1][0]) + (p0 * v[0][0]))),
        (p3 * v[3][1]) + (((p0 * v[0][1]) + (p1 * v[1][1])) + (p2 * v[2][1])),
        ((p2 * v[2][2]) + ((p0 * v[0][2]) + (p1 * v[1][2]))) + (p3 * v[3][2]),
        (p3 * v[3][3]) + ((p2 * v[2][3]) + ((p1 * v[1][3]) + (p0 * v[0][3]))),
    ]
}

fn dbl_args(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// Puts the points of a skin back to where its first stored definition holds them, with the
/// present shape of the skin as the starting point of the bones' work
/// [soft 0x084b6b9c SI_envelopUpdateInitDef]: the deformation init for dragging a point or
/// editing a weight, called with the skin the user is working on.
///
/// Nothing for no node, a node of word 7, or a node with no skeleton root. Otherwise the
/// skeleton's rest matrices are brought up to date ([`envelop_skel_update`]) and, twice at
/// most (each round only while [`damp_tab_create`] succeeds), the skeleton is walked in edit
/// mode ([`envelop_skel`] with 1), which gathers in `editvert` the weighted matrices of the
/// bones that move each point and marks the points in `damptab`; then for each point of the
/// skin's present definition (the node's +0x48) that is marked (patch: bit 0 or the sign of
/// its 16-bit flag; mesh: bit 0 or bit 5; spline key: bit 0 or the sign; NURBS point: bit 0
/// or the sign of its word) or is the point under the cursor (`SI_mdlPnt`; for a spline
/// the key `SI_mdlPnt / 3`):
///
/// - no bone moved it (`damptab` 0): its coordinates are copied into the first stored
///   definition (a patch point gets 1.0 in its fourth word; a NURBS point's coordinates are
///   divided by its weight and set through `SI_nurbsCrvMovePoint (0x084e9eec)` or
///   `SI_nurbsSrfMovePoint (0x084e9f2c)`, the surface with the weight itself);
/// - a bone moved it: the point is multiplied by the inverse of the matrix `editvert` holds
///   for it (made ready by [`init_def_element`]) and stored. A patch point takes four
///   coordinates, a mesh vertex or spline key three plus the translation row, a NURBS point
///   four, divided by the fourth, the surface's weight being that fourth coordinate. Without
///   an element for the point nothing is done.
///
/// `damptab` is freed after each round. Then for a local skin (word 5 or 8) whose root's
/// word is not 11 the root's parked rest rotations are put back ([`env_auto_set`]) and its
/// matrices rebuilt; the edited marks are cleared ([`env_vrt_reset_edit_flag`]) and the
/// chain of `editvert` freed.
///
/// The matrix of the inverse is a local of the original that keeps its value from point to
/// point (it is only set when the matrix has an inverse): before the first it is whatever the
/// stack held ([`unset_matrix`]'s NaNs here).
pub fn envelop_update_init_def(m: &mut Mem, o: &mut dyn Outside, node: Node) {
    if node.is_null() || m[node].envelop == 7 {
        return;
    }
    let root = envelop_skel_root(m, node);
    if root.is_null() {
        return;
    }
    envelop_skel_update(m, o, root);
    let unset = unset_matrix(m);
    let mut inv: Matrix = m.read(unset);
    let mut left = 2;
    while left != 0 && damp_tab_create(m, node) as i16 != 0 {
        envelop_skel(m, o, node, root, 1);
        left -= 1;
        let kind = m[node].kind;
        let d0 = m[m[node].defs].addr();
        let src = m[node].definition.addr();
        let cursor = m[SI_MDL_PNT];
        let marked = |m: &Mem, i: i32| m[m[DAMPTAB].add(i)] != 0;
        match kind {
            1 => {
                let count = |m: &Mem| (m.u16(d0.wrapping_add(0xc)) as i32).wrapping_mul(m.u16(d0.wrapping_add(0xe)) as i32);
                let mut i = 0i32;
                while i < count(m) {
                    let flag = m.u16(m.u32(src.wrapping_add(0x1a8)).wrapping_add((i as u32).wrapping_mul(2)));
                    if flag & 1 != 0 || (flag as i16) < 0 || cursor == i {
                        let off = (i as u32) << 4;
                        if !marked(m, i) {
                            for k in 0..3u32 {
                                let v = m.f32(m.u32(src.wrapping_add(0x1a4)).wrapping_add(off).wrapping_add(4 * k));
                                m.set_f32(m.u32(d0.wrapping_add(0x1a4)).wrapping_add(off).wrapping_add(4 * k), fmov(v));
                            }
                            m.set_u32(m.u32(d0.wrapping_add(0x1a4)).wrapping_add(off).wrapping_add(0xc), 0x3f80_0000);
                        } else if init_def_element(m, i, &mut inv) {
                            let at = m.u32(src.wrapping_add(0x1a4)).wrapping_add(off);
                            let (p0, p1, p2, p3) = (m.f32(at), m.f32(at.wrapping_add(4)), m.f32(at.wrapping_add(8)), m.f32(at.wrapping_add(12)));
                            let v = &inv;
                            let out = [
                                ((x(p2) * v[2][0]) + ((x(p1) * v[1][0]) + (x(p0) * v[0][0]))) + (x(p3) * v[3][0]),
                                (((x(p1) * v[1][1]) + (x(p0) * v[0][1])) + (x(p2) * v[2][1])) + (x(p3) * v[3][1]),
                                (((x(p1) * v[1][2]) + (x(p0) * v[0][2])) + (x(p2) * v[2][2])) + (x(p3) * v[3][2]),
                                ((x(p2) * v[2][3]) + ((x(p1) * v[1][3]) + (x(p0) * v[0][3]))) + (x(p3) * v[3][3]),
                            ];
                            for k in 0..4u32 {
                                m.set_f32(m.u32(d0.wrapping_add(0x1a4)).wrapping_add(off).wrapping_add(4 * k), out[k as usize].to_f32());
                            }
                        }
                    }
                    i += 1;
                }
            }
            4 => {
                let mut i = 0i32;
                while i < m.i32(d0.wrapping_add(0x14)) {
                    let off = (i as u32).wrapping_mul(0x14);
                    let flag = m.u16(m.u32(src).wrapping_add(0x12).wrapping_add(off));
                    if flag & 1 != 0 || flag & 0x20 != 0 || cursor == i {
                        if !marked(m, i) {
                            for k in 0..3u32 {
                                let v = m.f32(m.u32(src).wrapping_add(off).wrapping_add(4 * k));
                                m.set_f32(m.u32(d0).wrapping_add(off).wrapping_add(4 * k), fmov(v));
                            }
                        } else if init_def_element(m, i, &mut inv) {
                            let at = m.u32(src).wrapping_add(off);
                            let (p0, p1, p2) = (m.f32(at), m.f32(at.wrapping_add(4)), m.f32(at.wrapping_add(8)));
                            let out = init_def_vec3(p0, p1, p2, &inv);
                            for k in 0..3u32 {
                                m.set_f32(m.u32(d0).wrapping_add(off).wrapping_add(4 * k), out[k as usize]);
                            }
                        }
                    }
                    i += 1;
                }
            }
            6 => {
                let mut i = 0i32;
                while i < m.u16(d0.wrapping_add(0x10)) as i32 {
                    let off = (i as u32).wrapping_mul(0x30);
                    let flag = m.u16(m.u32(src.wrapping_add(0x14)).wrapping_add(off).wrapping_add(0x2c));
                    if flag & 1 != 0 || (flag as i16) < 0 || cursor.wrapping_div(3) == i {
                        if !marked(m, i) {
                            for k in 0..3u32 {
                                let v = m.f32(m.u32(src.wrapping_add(0x14)).wrapping_add(off).wrapping_add(0xc + 4 * k));
                                m.set_f32(m.u32(d0.wrapping_add(0x14)).wrapping_add(off).wrapping_add(0xc + 4 * k), fmov(v));
                            }
                        } else if init_def_element(m, i, &mut inv) {
                            let at = m.u32(src.wrapping_add(0x14)).wrapping_add(off).wrapping_add(0xc);
                            let (p0, p1, p2) = (m.f32(at), m.f32(at.wrapping_add(4)), m.f32(at.wrapping_add(8)));
                            let out = init_def_vec3(p0, p1, p2, &inv);
                            for k in 0..3u32 {
                                m.set_f32(m.u32(d0.wrapping_add(0x14)).wrapping_add(off).wrapping_add(0xc + 4 * k), out[k as usize]);
                            }
                        }
                    }
                    i += 1;
                }
            }
            9 | 10 => {
                let (count_at, pts_at) = if kind == 9 { (4u32, 0x2cu32) } else { (0x48, 0x54) };
                let mut i = 0i32;
                while i < m.i32(d0.wrapping_add(count_at)) {
                    let off = (i as u32).wrapping_mul(0x24);
                    let flag = m.u32(m.u32(src.wrapping_add(pts_at)).wrapping_add(off).wrapping_add(0x20));
                    if flag & 1 != 0 || (flag as i32) < 0 || cursor == i {
                        let mut args: Vec<u32> = vec![d0, i as u32];
                        let mut go = false;
                        if !marked(m, i) {
                            let at = m.u32(src.wrapping_add(pts_at)).wrapping_add(off);
                            let (px, py, pz, pw) = (m.f64(at), m.f64(at.wrapping_add(8)), m.f64(at.wrapping_add(0x10)), m.f64(at.wrapping_add(0x18)));
                            let s = F80::ONE / x(pw);
                            args.extend(dbl_args((x(px) * s).to_f64()));
                            args.extend(dbl_args((x(py) * s).to_f64()));
                            args.extend(dbl_args((x(pz) * s).to_f64()));
                            if kind == 10 {
                                args.extend(dbl_args(fmov64(pw)));
                            }
                            go = true;
                        } else if init_def_element(m, i, &mut inv) {
                            let at = m.u32(src.wrapping_add(pts_at)).wrapping_add(off);
                            let q = init_def_project([m.f64(at), m.f64(at.wrapping_add(8)), m.f64(at.wrapping_add(0x10)), m.f64(at.wrapping_add(0x18))], &inv);
                            let s = F80::ONE / q[3];
                            args.extend(dbl_args((q[0] * s).to_f64()));
                            args.extend(dbl_args((q[1] * s).to_f64()));
                            args.extend(dbl_args((q[2] * s).to_f64()));
                            if kind == 10 {
                                args.extend(dbl_args(q[3].to_f64()));
                            }
                            go = true;
                        }
                        if go {
                            if kind == 9 {
                                o.call(m, &SI_NURBS_CRV_MOVE_POINT, &args);
                            } else {
                                o.call(m, &SI_NURBS_SRF_MOVE_POINT, &args);
                            }
                        }
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        let tab = m[DAMPTAB];
        if !tab.is_null() {
            m.free(tab.addr());
            m[DAMPTAB] = Ptr32::NULL;
        }
    }
    let w = m[node].envelop;
    if (w == 5 || w == 8) && m[root].envelop != 0xb {
        env_auto_set(m, root, 0);
        let father = m[root].father;
        o.call(m, &crate::env_outside::SI_MODEL_UPDATE_SON_STATE, &[father.addr(), root.addr(), 1]);
    }
    env_vrt_reset_edit_flag(m, node);
    edit_vert_dispose(m);
}

/// A point of three coordinates through the inverse, with the translation row added, as the
/// mesh and spline branches of `SI_envelopUpdateInitDef` form it.
fn init_def_vec3(p0: f32, p1: f32, p2: f32, v: &Matrix) -> [f32; 3] {
    let (a, b, c) = (x(p0), x(p1), x(p2));
    [
        ((((b * v[1][0]) + (a * v[0][0])) + (c * v[2][0])) + v[3][0]).to_f32(),
        ((((b * v[1][1]) + (a * v[0][1])) + (c * v[2][1])) + v[3][1]).to_f32(),
        ((((b * v[1][2]) + (a * v[0][2])) + (c * v[2][2])) + v[3][2]).to_f32(),
    ]
}
