// SPDX-License-Identifier: MIT
//! `lattice.c`: the lattice record, how a lattice moves the points of the geometry under
//! it, and the lattice sets of a model node.
//!
//! A lattice is a box of `nx × ny × nz` control points that rests on a regular grid from
//! `min` to `max`. A point of the geometry is located in the resting grid (per axis the
//! cell number and the fraction inside the cell) and answered with the moved control
//! points blended at that place: per axis either linearly between the two points around
//! it or by a uniform cubic B-spline through four. Outside the box the control points are
//! continued in a straight line. Finding 0028 has the order of operations.
//!
//! Not here: `SI_latticeDraw (0x0843f254)`, which only draws (three nests of line strips
//! through `bgnline (0x08612b58)`, `v3f (0x08612c60)` and `endline (0x08612b6c)`).

use crate::globals::*;
use crate::outside::*;
use crate::records::{geom, Lattice};
use crate::{cur_dict, rd3, wr3, V3};
use si_math::algebra::{bspline_interpolate, bspline_tangent_interpolate, matrix_transpose_inverse};
use si_math::types::Vector;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, x, Round, F80};

/// The double constants of the file's `.rodata` (held against the binary by
/// `re/tools/si-deform/check_data.py`): a box thinner than [`THIN`] along an axis is widened
/// by [`WIDEN`] on both sides; a normal no longer than [`THIN`] is not rescaled.
pub const THIN: f64 = 0.0001;
pub const THIN_AT: [u32; 2] = [0x0865_f8e0, 0x0865_f8f8];
pub const WIDEN: f64 = 0.005;
pub const WIDEN_AT: u32 = 0x0865_f8e8;
pub const HALF: f64 = 0.5;
pub const HALF_AT: u32 = 0x0865_f8f0;

fn points(m: &Mem, lat: Ptr32<Lattice>) -> i32 {
    // three sign-extended 16-bit counts multiplied as 32-bit integers
    (m[lat].nx as i32).wrapping_mul(m[lat].ny as i32).wrapping_mul(m[lat].nz as i32)
}

/// The first lattice of a set (`si-types` keeps the set's element type opaque).
fn first(m: &Mem, set: Ptr32<Ptr32<si_types::model::Lattice>>) -> Ptr32<Lattice> {
    m.read(set).cast()
}

fn vector(v: V3) -> Vector {
    Vector::new(v[0], v[1], v[2])
}

/// Whether a lattice acts on a node's geometry [soft 0x0843d910 SI_isLatticeDeformed].
///
/// A node that depends on itself (`latdep`): its own node lattice or its own branch
/// lattice is active. A node that depends on another: walking from the node up to the
/// father of that other node, some node on the way depends on a node whose branch lattice
/// is active.
pub fn is_lattice_deformed(m: &Mem, model: Ptr32<Model>) -> i32 {
    let dep = m[model].latdep;
    if dep == model {
        if m[model].nb_lattices0 != 0 {
            let l = first(m, m[model].lattices0);
            if m[l].active != 0 {
                return 1;
            }
        }
        let dep = m[model].latdep;
        if m[dep].nb_lattices1 != 0 {
            let l = first(m, m[dep].lattices1);
            if m[l].active != 0 {
                return 1;
            }
        }
        return 0;
    }
    let stop = m[dep].father;
    if model == stop {
        return 0;
    }
    let mut cur = model;
    loop {
        let dep = m[cur].latdep;
        if m[dep].nb_lattices1 != 0 {
            let l = first(m, m[dep].lattices1);
            if m[l].active != 0 {
                return 1;
            }
        }
        cur = m[cur].father;
        if cur == stop {
            return 0;
        }
    }
}

/// A lattice without points [soft 0x0843d988 latticeAllocate]: 0x44 bytes from
/// `SI_malloc`, everything 0 but `active` = 1. Null when there is no memory.
pub fn lattice_allocate(m: &mut Mem) -> Ptr32<Lattice> {
    let lat: Ptr32<Lattice> = Ptr32::new(m.si_malloc(0x44));
    if !lat.is_null() {
        m[lat].unk_40 = 0;
        m[lat].min = [0.0; 3];
        m[lat].max = [0.0; 3];
        m[lat].step = [0.0; 3];
        m[lat].active = 1;
        m[lat].nx = 0;
        m[lat].ny = 0;
        m[lat].nz = 0;
        m[lat].interp = [0; 3];
        m[lat].keys = Ptr32::NULL;
        m[lat].flags = Ptr32::NULL;
    }
    lat
}

/// A new lattice at rest in a box [soft 0x0843da24 SI_latticeInitialize].
///
/// The box: along an axis where |max − min| < 0.0001 it becomes float(min − 0.005) to
/// float(max + 0.005) (the constants are doubles, the sums are made in the register). The
/// step per axis is float((max − min) / (n − 1)), the division by the integer n − 1 in the
/// register. The points are laid out x outermost; each coordinate starts at `min` and has
/// the step added once per point **as a float sum rounded every time**, so the last point
/// of an axis is not exactly `max`. Every flag word is 0. The word at +0x40 is not written.
/// Neither `SI_calloc` is checked for null.
#[allow(clippy::too_many_arguments)]
pub fn lattice_initialize(m: &mut Mem, nx: i16, ny: i16, nz: i16, interp: [i32; 3], min: V3, max: V3) -> Ptr32<Lattice> {
    let lat: Ptr32<Lattice> = Ptr32::new(m.si_malloc(0x44));
    if lat.is_null() {
        return lat;
    }
    m[lat].active = 1;
    m[lat].nx = nx;
    m[lat].ny = ny;
    m[lat].nz = nz;
    m[lat].interp = interp;
    m[lat].min = min;
    m[lat].max = max;
    for a in 0..3 {
        let (lo, hi) = (m[lat].min[a], m[lat].max[a]);
        if (x(hi) - lo).abs().lt(x(THIN)) {
            m[lat].max[a] = (x(WIDEN) + hi).to_f32();
            m[lat].min[a] = (x(lo) - x(WIDEN)).to_f32();
        }
    }
    let mut step = [0.0f32; 3];
    for a in 0..3 {
        let n = [m[lat].nx, m[lat].ny, m[lat].nz][a] as i32 - 1;
        step[a] = ((x(m[lat].max[a]) - m[lat].min[a]) / x(n)).to_f32();
        m[lat].step[a] = step[a];
    }
    let n = points(m, lat);
    let keys = m.si_calloc(n as u32, 0xc);
    m[lat].keys = Ptr32::new(keys);
    let mut p = keys;
    let mut px = fmov(m[lat].min[0]);
    let mut i: i16 = 0;
    while i < nx {
        let mut py = fmov(m[lat].min[1]);
        let mut j: i16 = 0;
        while j < ny {
            let mut pz = fmov(m[lat].min[2]);
            if nz > 0 {
                let mut left = nz as u16;
                loop {
                    wr3(m, p, [px, py, pz]);
                    pz = (x(pz) + step[2]).to_f32();
                    left = left.wrapping_sub(1);
                    p = p.wrapping_add(0xc);
                    if left == 0 {
                        break;
                    }
                }
            }
            py = (x(py) + step[1]).to_f32();
            j = j.wrapping_add(1);
        }
        px = (x(px) + step[0]).to_f32();
        i = i.wrapping_add(1);
    }
    let n = points(m, lat);
    let flags = m.si_calloc(n as u32, 2);
    m[lat].flags = Ptr32::new(flags);
    let mut left = points(m, lat);
    let mut p = flags;
    while left > 0 {
        m.set_u16(p, 0);
        left -= 1;
        p = p.wrapping_add(2);
    }
    lat
}

/// Puts every point back to rest and clears every flag [soft 0x0843dc70 SI_latticeReset]:
/// the layout loop of `SI_latticeInitialize` on the lattice's own `min`, `step` and counts
/// (the counts are read again at every turn of each loop).
pub fn lattice_reset(m: &mut Mem, lat: Ptr32<Lattice>) {
    let step = m[lat].step.map(fmov);
    let mut p = m[lat].keys.addr();
    let mut px = fmov(m[lat].min[0]);
    let mut i: i16 = 0;
    while i < m[lat].nx {
        let mut py = fmov(m[lat].min[1]);
        let mut j: i16 = 0;
        while j < m[lat].ny {
            let mut pz = fmov(m[lat].min[2]);
            let mut k: i16 = 0;
            while k < m[lat].nz {
                wr3(m, p, [px, py, pz]);
                pz = (x(pz) + step[2]).to_f32();
                k = k.wrapping_add(1);
                p = p.wrapping_add(0xc);
            }
            py = (x(py) + step[1]).to_f32();
            j = j.wrapping_add(1);
        }
        px = (x(px) + step[0]).to_f32();
        i = i.wrapping_add(1);
    }
    let mut p = m[lat].flags.addr();
    let mut left = points(m, lat);
    while left > 0 {
        m.set_u16(p, 0);
        left -= 1;
        p = p.wrapping_add(2);
    }
}

/// A copy of a lattice with points and flags of its own [soft 0x0843dd34 SI_latticeCopy].
/// Neither `SI_calloc` is checked for null.
pub fn lattice_copy(m: &mut Mem, src: Ptr32<Lattice>) -> Ptr32<Lattice> {
    let dst = lattice_allocate(m);
    if dst.is_null() {
        return dst;
    }
    m[dst].active = m[src].active;
    m[dst].nx = m[src].nx;
    m[dst].ny = m[src].ny;
    m[dst].nz = m[src].nz;
    // plain word moves: no float passes through the unit
    for off in [0x08u32, 0x0c, 0x10, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c] {
        let w = m.u32(src.addr() + off);
        m.set_u32(dst.addr() + off, w);
    }
    let n = points(m, src);
    let keys = m.si_calloc(n as u32, 0xc);
    m[dst].keys = Ptr32::new(keys);
    let n = points(m, src);
    let flags = m.si_calloc(n as u32, 2);
    m[dst].flags = Ptr32::new(flags);
    let (mut sk, mut sf) = (m[src].keys.addr(), m[src].flags.addr());
    let (mut dk, mut df) = (keys, flags);
    let mut left = points(m, src);
    while left > 0 {
        for c in [0u32, 4, 8] {
            let w = m.u32(sk.wrapping_add(c));
            m.set_u32(dk.wrapping_add(c), w);
        }
        left -= 1;
        dk = dk.wrapping_add(0xc);
        sk = sk.wrapping_add(0xc);
        let w = m.u16(sf);
        m.set_u16(df, w);
        df = df.wrapping_add(2);
        sf = sf.wrapping_add(2);
    }
    dst
}

/// Gives a copy of a node the lattices of the node it was copied from
/// [soft 0x0843de60 SI_latticeCopyFromSetup].
///
/// With the Duplicate setting at `SI_glbDuplic` + 0x20: every lattice of the source's
/// active set (the branch set if its first lattice is active, else the node set) is copied
/// and added at the same index, both sets get their elements, the word at +0x5c is copied
/// and the lattice element's animation is copied. Without it: only the active lattice is
/// copied, to index 0. Which set is "active" comes from [`model_lattice`]; when the source
/// has no active lattice that routine stores nothing and the original goes on with a
/// 16-bit local nobody set (here: a local of the image's stack, which holds what the
/// stack held).
pub fn lattice_copy_from_setup(m: &mut Mem, o: &mut dyn Outside, src: Ptr32<Model>, dst: Ptr32<Model>) {
    let frame = m.local(0x10);
    let which_at = frame + 0xe;
    let (idx1_at, idx2_at) = (frame + 8, frame + 4);
    let active = model_lattice(m, src, Ptr32::new(which_at));
    if m.i16(SI_GLB_DUPLIC.addr() + GLB_DUPLIC_LATTICE_ANIM) != 0 {
        let chap = m[SI_CID][CID_LATTICES] as u32;
        o.call(m, &SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP, &[chap]);
        let n = if m.i16(which_at) != 0 { m[src].nb_lattices1 } else { m[src].nb_lattices0 } as i32;
        let mut i = 0i32;
        while i < n {
            let set = if m.i16(which_at) != 0 { m[src].lattices1 } else { m[src].lattices0 };
            let one: Ptr32<Lattice> = m.read(set.add(i)).cast();
            let copy = lattice_copy(m, one);
            let which = m.u16(which_at) as u32;
            o.call(m, &SI_MODEL_ADD_I_LATTICE, &[dst.addr(), which, i as u16 as u32, copy.addr()]);
            i += 1;
        }
        if m[dst].nb_lattices0 != 0 {
            o.call(m, &SI_ED_ADD_LATTICE, &[dst.addr(), 0]);
        }
        if m[dst].nb_lattices1 != 0 {
            o.call(m, &SI_ED_ADD_LATTICE, &[dst.addr(), 1]);
        }
        m[dst].unk_5c = m[src].unk_5c;
        let which = m.u16(which_at) as u32;
        let e1 = o.call(m, &SI_ED_GET_ELEM_FROM_LATTICE, &[src.addr(), which, idx1_at]);
        let which = m.u16(which_at) as u32;
        let e2 = o.call(m, &SI_ED_GET_ELEM_FROM_LATTICE, &[dst.addr(), which, idx2_at]);
        if e2 != 0 {
            let chap = m[SI_CID][CID_LATTICES] as u32;
            let (i1, i2) = (m.u32(idx1_at), m.u32(idx2_at));
            let dict = cur_dict(m);
            o.call(m, &SI_ELEMDICT_COPY_ELEM_ANIM, &[dict, e1, e2, chap, i1, i2]);
        }
    } else {
        let copy = lattice_copy(m, active);
        let which = m.u16(which_at) as u32;
        o.call(m, &SI_MODEL_ADD_I_LATTICE, &[dst.addr(), which, 0, copy.addr()]);
        if m[dst].nb_lattices0 != 0 {
            o.call(m, &SI_ED_ADD_LATTICE, &[dst.addr(), 0]);
        }
        if m[dst].nb_lattices1 != 0 {
            o.call(m, &SI_ED_ADD_LATTICE, &[dst.addr(), 1]);
        }
    }
    m.release(0x10);
}

/// Frees a lattice with its points and flags and clears the pointer to it
/// [soft 0x0843dff0 SI_latticeDispose].
pub fn lattice_dispose(m: &mut Mem, slot: Ptr32<Ptr32<Lattice>>) {
    if m.read(slot).is_null() {
        return;
    }
    let lat = m.read(slot);
    let keys = m[lat].keys.addr();
    if keys != 0 {
        m.free(keys);
        let lat = m.read(slot);
        m[lat].keys = Ptr32::NULL;
    }
    let lat = m.read(slot);
    let flags = m[lat].flags.addr();
    if flags != 0 {
        m.free(flags);
        let lat = m.read(slot);
        m[lat].flags = Ptr32::NULL;
    }
    let lat = m.read(slot);
    if !lat.is_null() {
        m.free(lat.addr());
        m.write(slot, Ptr32::NULL);
    }
    m.write(slot, Ptr32::NULL);
}

/// Where a coordinate lies along one axis of the resting grid: the quotient
/// (v − min) / step in the register, and what `fistl` stores of it with the rounding
/// control set to "toward zero" (0x80000000 for a NaN or a value that does not fit).
fn locate(m: &Mem, lat: Ptr32<Lattice>, v: f32, axis: usize) -> (F80, i32) {
    let t = (x(v) - m[lat].min[axis]) / m[lat].step[axis];
    (t, t.to_i32(Round::Zero))
}

/// float(a + (b − a) × f): the difference, the product and the sum in the register.
fn lerp(a: V3, b: V3, f: f32) -> V3 {
    core::array::from_fn(|c| (x(a[c]) + (x(b[c]) - a[c]) * f).to_f32())
}

/// float(b − a).
fn delta(a: V3, b: V3) -> V3 {
    core::array::from_fn(|c| (x(b[c]) - a[c]).to_f32())
}

fn bspline(m: &mut Mem, out: u32, f: f32, p: [u32; 4]) {
    let v = bspline_interpolate(f, &vector(rd3(m, p[0])), &vector(rd3(m, p[1])), &vector(rd3(m, p[2])), &vector(rd3(m, p[3])));
    wr3(m, out, v.to_array());
}

fn bspline_tangent(m: &mut Mem, out: u32, f: f32, p: [u32; 4]) {
    let v = bspline_tangent_interpolate(f, &vector(rd3(m, p[0])), &vector(rd3(m, p[1])), &vector(rd3(m, p[2])), &vector(rd3(m, p[3])));
    wr3(m, out, v.to_array());
}

/// A deformed point and the three tangents of the deformation there
/// [soft 0x0843e05c latticeValueTangent].
///
/// `axis` 2 is the entry: the routine calls itself for axis 1 at two (linear) or four
/// (B-spline) z cells, that for axis 0 at two or four y cells, and axis 0 takes two or
/// four points of [`lattice_virtual_key`] along x. `j` and `k` are the y and z cell
/// numbers found so far. Per level: the cell is trunc((in − min) / step) as a 32-bit
/// integer, the fraction float(quotient − cell).
///
/// `cs` gets three vectors, the derivatives of the answer by the x, y and z cell
/// coordinate. On the level of an axis the vector of that axis is differentiated (linear:
/// float(second − first); B-spline: `SI_bsplineTangentInterpolate`) and the other two and
/// the point are blended like the point. Level 0 starts the y and z vectors as copies of
/// the point, which is why differentiating them later works.
#[allow(clippy::too_many_arguments)]
pub fn lattice_value_tangent(m: &mut Mem, lat: Ptr32<Lattice>, j: i32, k: i32, inp: Ptr32<V3>, out: Ptr32<V3>, cs: Ptr32<[V3; 3]>, axis: i16) {
    if !(0..=2).contains(&axis) {
        return;
    }
    let a = axis as usize;
    // the callee's temporaries: four points and four coordinate systems
    let frame = m.local(4 * 0xc + 4 * 0x24);
    let pt = |n: usize| frame + 0xc * n as u32;
    let sys = |n: usize| frame + 0x30 + 0x24 * n as u32;
    let (t, cell) = locate(m, lat, m.f32(inp.addr() + 4 * a as u32), a);
    let f = (t - x(cell)).to_f32();
    let (j, k) = match a {
        2 => (j, cell),
        1 => (cell, k),
        _ => (j, k),
    };
    let linear = m[lat].interp[a] == 0;
    // linear: the cells 0 and +1 in slots 1 and 2; B-spline: −1, 0, +1, +2 in slots 0 to 3
    let slots: &[(usize, i32)] = if linear { &[(1, 0), (2, 1)] } else { &[(0, -1), (1, 0), (2, 1), (3, 2)] };
    for &(slot, d) in slots {
        match a {
            2 => lattice_value_tangent(m, lat, j, k.wrapping_add(d), inp, Ptr32::new(pt(slot)), Ptr32::new(sys(slot)), 1),
            1 => lattice_value_tangent(m, lat, j.wrapping_add(d), k, inp, Ptr32::new(pt(slot)), Ptr32::new(sys(slot)), 0),
            _ => lattice_virtual_key(m, lat, cell.wrapping_add(d), j, k, Ptr32::new(pt(slot))),
        }
    }
    let (o, c) = (out.addr(), cs.addr());
    if a == 0 {
        if linear {
            let (p0, p1) = (rd3(m, pt(1)), rd3(m, pt(2)));
            wr3(m, o, lerp(p0, p1, f));
            wr3(m, c, delta(p0, p1));
        } else {
            let p = [pt(0), pt(1), pt(2), pt(3)];
            bspline(m, o, f, p);
            bspline_tangent(m, c, f, p);
        }
        // the y and z vectors start as the point: six word moves out of `out`
        for part in [0x0cu32, 0x18] {
            for w in [0u32, 4, 8] {
                let v = m.u32(o + w);
                m.set_u32(c + part + w, v);
            }
        }
    } else if linear {
        let (p0, p1) = (rd3(m, pt(1)), rd3(m, pt(2)));
        wr3(m, o, lerp(p0, p1, f));
        // in the original's order of stores: the x vector, the y vector, the z vector
        let part = |m: &Mem, n: usize, v: usize| rd3(m, sys(n) + 0xc * v as u32);
        let mut res = [[0.0f32; 3]; 3];
        for (v, r) in res.iter_mut().enumerate() {
            let (s0, s1) = (part(m, 1, v), part(m, 2, v));
            *r = if v == a { delta(s0, s1) } else { lerp(s0, s1, f) };
        }
        for (v, r) in res.iter().enumerate() {
            wr3(m, c + 0xc * v as u32, *r);
        }
    } else {
        bspline(m, o, f, [pt(0), pt(1), pt(2), pt(3)]);
        for v in 0..3usize {
            let p = [0usize, 1, 2, 3].map(|n| sys(n) + 0xc * v as u32);
            if v == a {
                bspline_tangent(m, c + 0xc * v as u32, f, p);
            } else {
                bspline(m, c + 0xc * v as u32, f, p);
            }
        }
    }
    m.release(4 * 0xc + 4 * 0x24);
}

/// A point moved by the lattice [soft 0x0843e87c SI_latticeValue].
///
/// `axis` 2 is the entry (with `j` = `k` = 0, which are then not read): the routine calls
/// itself for axis 1 at two or four z cells, that for axis 0 at two or four y cells, and
/// axis 0 takes two or four points of [`lattice_virtual_key`] along x. Per level: the
/// quotient (in − min) / step in the register, truncated toward zero to a 32-bit integer
/// of which **the low 16 bits** are the cell; the fraction is float(quotient − cell).
/// Linear (interpolation code 0): float(first + (second − first) × fraction) per
/// coordinate. Otherwise `SI_bsplineInterpolate` through the cells −1, 0, +1, +2. Cell
/// numbers are 16-bit throughout and wrap as such. Any other `axis`: nothing is written.
pub fn lattice_value(m: &mut Mem, lat: Ptr32<Lattice>, j: i16, k: i16, inp: Ptr32<V3>, out: Ptr32<V3>, axis: i16) {
    if !(0..=2).contains(&axis) {
        return;
    }
    let a = axis as usize;
    let frame = m.local(4 * 0xc);
    let pt = |n: usize| frame + 0xc * n as u32;
    let (t, whole) = locate(m, lat, m.f32(inp.addr() + 4 * a as u32), a);
    let cell = whole as i16;
    let f = (t - x(cell as i32)).to_f32();
    let (j, k) = match a {
        2 => (j, cell),
        1 => (cell, k),
        _ => (j, k),
    };
    let linear = m[lat].interp[a] == 0;
    let slots: &[(usize, i16)] = if linear { &[(1, 0), (2, 1)] } else { &[(0, -1), (1, 0), (2, 1), (3, 2)] };
    for &(slot, d) in slots {
        match a {
            2 => lattice_value(m, lat, j, k.wrapping_add(d), inp, Ptr32::new(pt(slot)), 1),
            1 => lattice_value(m, lat, j.wrapping_add(d), k, inp, Ptr32::new(pt(slot)), 0),
            // along x the neighbours are 32-bit: the sign-extended cell plus the offset
            _ => lattice_virtual_key(m, lat, (cell as i32).wrapping_add(d as i32), j as i32, k as i32, Ptr32::new(pt(slot))),
        }
    }
    if linear {
        let (p0, p1) = (rd3(m, pt(1)), rd3(m, pt(2)));
        wr3(m, out.addr(), lerp(p0, p1, f));
    } else {
        bspline(m, out.addr(), f, [pt(0), pt(1), pt(2), pt(3)]);
    }
    m.release(4 * 0xc);
}

/// The control point at a cell, also for cells outside the box
/// [soft 0x0843ecf4 latticeVirtualKey].
///
/// Each number is clamped into 0 … n − 1 and how far it lay outside is kept (as a positive
/// integer `e` per axis). The answer starts as the clamped point P. Then, for each axis
/// whose number was below 0 and after those for each axis whose number was n or more, in
/// the order x, y, z: answer = float((P − Q) × e + answer), where Q is P's neighbour one
/// step **inward** along that axis, and the difference and product stay in the register.
/// So the points are continued beyond each face in a straight line through the last two.
/// The neighbour's address is computed from the clamped index without a check: a lattice
/// with one point along an axis reads a point that is not the axis's.
pub fn lattice_virtual_key(m: &mut Mem, lat: Ptr32<Lattice>, i: i32, j: i32, k: i32, out: Ptr32<V3>) {
    let (mut i, mut j, mut k) = (i, j, k);
    // how far outside, per axis; read only where a test below set it
    let mut e = [0i32; 3];
    let below = [i < 0, j < 0, k < 0];
    if below[0] {
        e[0] = i.wrapping_neg();
        i = 0;
    }
    if below[1] {
        e[1] = j.wrapping_neg();
        j = 0;
    }
    if below[2] {
        e[2] = k.wrapping_neg();
        k = 0;
    }
    let (nx, ny, nz) = (m[lat].nx as i32, m[lat].ny as i32, m[lat].nz as i32);
    let above = [i >= nx, j >= ny, k >= nz];
    if above[0] {
        e[0] = i.wrapping_sub(nx).wrapping_add(1);
        i = nx.wrapping_sub(1);
    }
    if above[1] {
        e[1] = j.wrapping_sub(ny).wrapping_add(1);
        j = ny.wrapping_sub(1);
    }
    if above[2] {
        e[2] = k.wrapping_sub(nz).wrapping_add(1);
        k = nz.wrapping_sub(1);
    }
    let index = j.wrapping_add(i.wrapping_mul(ny)).wrapping_mul(nz).wrapping_add(k);
    let at = m[lat].keys.addr().wrapping_add((index as u32).wrapping_mul(0xc));
    // P is copied by word moves, to a local and to the answer
    let words = [m.u32(at), m.u32(at.wrapping_add(4)), m.u32(at.wrapping_add(8))];
    let p = words.map(f32::from_bits);
    let o = out.addr();
    for (c, w) in words.iter().enumerate() {
        m.set_u32(o + 4 * c as u32, *w);
    }
    // one step along x, y, z in bytes
    let stride = [(ny.wrapping_mul(nz) as u32).wrapping_mul(0xc), (nz as u32).wrapping_mul(0xc), 0xc];
    for (outside, inward) in [(below, 1i32), (above, -1)] {
        for axis in 0..3 {
            if !outside[axis] {
                continue;
            }
            let keys = m[lat].keys.addr().wrapping_add((index as u32).wrapping_mul(0xc));
            let q_at = if inward > 0 { keys.wrapping_add(stride[axis]) } else { keys.wrapping_sub(stride[axis]) };
            let q = rd3(m, q_at);
            let cur = rd3(m, o);
            let res: V3 = core::array::from_fn(|c| ((x(p[c]) - q[c]) * x(e[axis]) + cur[c]).to_f32());
            wr3(m, o, res);
        }
    }
}

/// Moves control points at random [soft 0x0843f0f8 SI_latticeRandomize]: every point, or
/// with `tagged_only` those whose flag word has bit 0, `times` times over (a 16-bit count;
/// nothing for 0 or less) gets float((drand48() − 0.5) × amount + coordinate) per
/// coordinate, three random numbers a round in the order x, y, z. The amount is read
/// through its pointer at every use.
pub fn lattice_randomize(m: &mut Mem, o: &mut dyn Outside, lat: Ptr32<Lattice>, amount: Ptr32<V3>, times: i16, tagged_only: i16) {
    let n = points(m, lat);
    let mut p = m[lat].keys.addr();
    let mut i = 0i32;
    while i < n {
        let tagged = if tagged_only != 0 {
            let flags = m[lat].flags.addr();
            m.u8(flags.wrapping_add((i as u32).wrapping_mul(2))) & 1 != 0
        } else {
            true
        };
        if tagged {
            let mut left = times as i32;
            while left > 0 {
                for c in [0u32, 4, 8] {
                    let r = o.drand48();
                    let v = ((x(r) - x(HALF)) * m.f32(amount.addr() + c) + m.f32(p.wrapping_add(c))).to_f32();
                    m.set_f32(p.wrapping_add(c), v);
                }
                left -= 1;
            }
        }
        i += 1;
        p = p.wrapping_add(0xc);
    }
}

/// The points of one lattice between those of two others
/// [soft 0x0843f518 SI_latticeInterpolate]: float(a + (b − a) × t) per coordinate, for
/// lattices with the same three counts; else 0 and nothing is written. The count that
/// ends the loop is tested for 0 only: a negative product would run through memory.
pub fn lattice_interpolate(m: &mut Mem, dst: Ptr32<Lattice>, t: f32, a: Ptr32<Lattice>, b: Ptr32<Lattice>) -> i32 {
    let same = |m: &Mem, off: u32| {
        let v = m.u16(dst.addr() + off);
        v == m.u16(a.addr() + off) && v == m.u16(b.addr() + off)
    };
    if !(same(m, 2) && same(m, 4) && same(m, 6)) {
        return 0;
    }
    let (mut dk, mut ak) = (m[dst].keys.addr(), m[a].keys.addr());
    let mut left = points(m, dst);
    let mut bk = m[b].keys.addr();
    if left != 0 {
        loop {
            for c in [0u32, 4, 8] {
                let (va, vb) = (m.f32(ak.wrapping_add(c)), m.f32(bk.wrapping_add(c)));
                m.set_f32(dk.wrapping_add(c), (x(va) + (x(vb) - va) * t).to_f32());
            }
            ak = ak.wrapping_add(0xc);
            bk = bk.wrapping_add(0xc);
            dk = dk.wrapping_add(0xc);
            left = left.wrapping_sub(1);
            if left == 0 {
                break;
            }
        }
    }
    1
}

/// Deforms a geometry [soft 0x0843f5c4 SI_latticeTransform]: every point of `src` (a
/// definition of the kind `kind`) is put through the lattice and stored in `dst`, the
/// working definition of the same shape.
///
/// - Mesh (4), bit 4 of its flag byte clear: [`lattice_value`] per vertex, then
///   `SI_meshComputesPolygonNormal` and `SI_meshComputesVertexNormal` for the whole mesh.
/// - Mesh with that bit set ("normals are carried"): [`lattice_value_tangent`] per vertex.
///   Its three tangents, each divided by the lattice's step of its axis, are the upper 3 × 3
///   of a matrix; every corner normal at that vertex (found through the vertex's star) is
///   multiplied by the transposed inverse of the matrix and scaled back to the length it
///   had, unless its new length is 0.0001 or less. Then `SI_meshComputesPolygonNormal`.
/// - Patch (1): [`lattice_value`] per control point, then `SI_patchComputeSurface`.
/// - Spline (6) and a face (2) made of splines, its outline and each hole: per key the
///   point, and for a spline of type 1 the two tangent points as well.
/// - NURBS curve (9), surface (10) and a face made of NURBS curves: per control point the
///   coordinates divided by the weight (doubles, rounded to float) go through the lattice
///   and the result is handed to `SI_nurbsCrvMovePoint` or `SI_nurbsSrfMovePoint` (with
///   the destination point's weight); two bits are set in the destination's flag byte.
/// - Any other kind: nothing.
pub fn lattice_transform(m: &mut Mem, o: &mut dyn Outside, lat: Ptr32<Lattice>, kind: i32, src: u32, dst: u32) {
    use geom::*;
    let value = |m: &mut Mem, s: u32, d: u32| lattice_value(m, lat, 0, 0, Ptr32::new(s), Ptr32::new(d), 2);
    let spline = |m: &mut Mem, s: u32, d: u32| {
        let (mut sk, mut dk) = (m.u32(s + SPLINE_KEYS), m.u32(d + SPLINE_KEYS));
        let whole = m.i32(s + SPLINE_TYPE) == 1;
        let mut b = 0i32;
        if m.u16(s + SPLINE_NB_KEYS) != 0 {
            loop {
                if whole {
                    value(m, sk, dk);
                }
                value(m, sk.wrapping_add(0xc), dk.wrapping_add(0xc));
                if whole {
                    value(m, sk.wrapping_add(0x18), dk.wrapping_add(0x18));
                }
                b += 1;
                sk = sk.wrapping_add(KEY_SIZE);
                dk = dk.wrapping_add(KEY_SIZE);
                if b >= m.u16(s + SPLINE_NB_KEYS) as i32 {
                    break;
                }
            }
        }
    };
    // a control point of a NURBS curve or surface through the lattice: the moved x, y, z
    // as the six words of three doubles
    let rational = |m: &mut Mem, cp: u32| -> [u32; 6] {
        let frame = m.local(0x18);
        let w = m.f64(cp.wrapping_add(CP_W));
        for c in 0..3u32 {
            let v = (x(m.f64(cp.wrapping_add(8 * c))) / w).to_f32();
            m.set_f32(frame + 0xc + 4 * c, v);
        }
        value(m, frame + 0xc, frame);
        let mut words = [0u32; 6];
        for c in 0..3usize {
            let d = x(m.f32(frame + 4 * c as u32)).to_f64().to_bits();
            words[2 * c] = d as u32;
            words[2 * c + 1] = (d >> 32) as u32;
        }
        m.release(0x18);
        words
    };
    match kind {
        4 => {
            if m.u8(src + MESH_FLAGS) & MESH_CARRY_NORMALS == 0 {
                let (mut sv, mut dv) = (m.u32(src + MESH_VERTS), m.u32(dst + MESH_VERTS));
                let mut i = 0i32;
                while i < m.i32(src + MESH_NB_VERTS) {
                    value(m, sv, dv);
                    i += 1;
                    sv = sv.wrapping_add(VERT_SIZE);
                    dv = dv.wrapping_add(VERT_SIZE);
                }
                o.call(m, &SI_MESH_COMPUTES_POLYGON_NORMAL, &[dst, u32::MAX]);
                o.call(m, &SI_MESH_COMPUTES_VERTEX_NORMAL, &[dst, u32::MAX]);
                return;
            }
            let identity = m[SI_IDENTITE];
            let mut mat = identity.map(|row| row.map(fmov));
            let cs_at = m.local(0x24);
            let (mut sv, mut dv) = (m.u32(src + MESH_VERTS), m.u32(dst + MESH_VERTS));
            let mut i = 0i32;
            while i < m.i32(src + MESH_NB_VERTS) {
                lattice_value_tangent(m, lat, 0, 0, Ptr32::new(sv), Ptr32::new(dv), Ptr32::new(cs_at), 2);
                for r in 0..3 {
                    let step = m[lat].step[r];
                    for c in 0..3 {
                        mat[r][c] = (x(m.f32(cs_at + 0xc * r as u32 + 4 * c as u32)) / step).to_f32();
                    }
                }
                let (n, _) = matrix_transpose_inverse(&mat);
                if m.u16(sv + VERT_NB_STAR) != 0 {
                    let mut s = 0i32;
                    loop {
                        let star = m.u32(sv + VERT_STAR);
                        let poly = m.i32(star.wrapping_add((s as u32).wrapping_mul(STAR_SIZE)).wrapping_add(STAR_POLY));
                        s += 1;
                        if poly != -1 {
                            let off = (poly as u32).wrapping_mul(POLY_SIZE);
                            let sp = m.u32(src + MESH_POLYS).wrapping_add(off);
                            let dp = m.u32(dst + MESH_POLYS).wrapping_add(off);
                            if m.u16(sp + POLY_NB_CORNERS) != 0 {
                                let (mut c, mut at) = (0i32, 0u32);
                                loop {
                                    let sc = m.u32(sp + POLY_CORNERS).wrapping_add(at);
                                    if m.i32(sc + CORNER_VERT) == i {
                                        carry_normal(m, sp, dp, at, &n);
                                    }
                                    at = at.wrapping_add(CORNER_SIZE);
                                    c += 1;
                                    if c >= m.u16(sp + POLY_NB_CORNERS) as i32 {
                                        break;
                                    }
                                }
                            }
                        }
                        if s >= m.u16(sv + VERT_NB_STAR) as i32 {
                            break;
                        }
                    }
                }
                i += 1;
                sv = sv.wrapping_add(VERT_SIZE);
                dv = dv.wrapping_add(VERT_SIZE);
            }
            m.release(0x24);
            o.call(m, &SI_MESH_COMPUTES_POLYGON_NORMAL, &[dst, u32::MAX]);
        }
        1 => {
            let mut left = (m.u16(src + PATCH_NU) as i32).wrapping_mul(m.u16(src + PATCH_NV) as i32);
            let (mut sp, mut dp) = (m.u32(src + PATCH_POINTS), m.u32(dst + PATCH_POINTS));
            while left > 0 {
                value(m, sp, dp);
                left -= 1;
                sp = sp.wrapping_add(PATCH_POINT_SIZE);
                dp = dp.wrapping_add(PATCH_POINT_SIZE);
            }
            o.call(m, &SI_PATCH_COMPUTE_SURFACE, &[dst, 0]);
        }
        2 => {
            if m.i16(src + FACE_IS_SPLINE) != 0 {
                // a negative number of holes: nothing, not even the outline
                if m.u8(src + FACE_NB_HOLES + 1) & 0x80 != 0 {
                    return;
                }
                let mut hole = -1i32;
                loop {
                    let (s, d) = if hole == -1 {
                        (src + FACE_OUTLINE, dst + FACE_OUTLINE)
                    } else {
                        let off = (hole as u32).wrapping_mul(FACE_SPLINE_SIZE);
                        (m.u32(src + FACE_HOLES).wrapping_add(off), m.u32(dst + FACE_HOLES).wrapping_add(off))
                    };
                    spline(m, s, d);
                    hole += 1;
                    if hole >= m.i16(src + FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            } else {
                let mut curve = 0i32;
                if m.i16(src + FACE_NB_HOLES) <= 0 {
                    return;
                }
                loop {
                    let off = (curve as u32).wrapping_mul(FACE_NURBS_SIZE);
                    let s = m.u32(src + FACE_NURBS).wrapping_add(off);
                    let d = m.u32(dst + FACE_NURBS).wrapping_add(off);
                    let mut b = 0i32;
                    while b < m.i32(s + CRV_NB_POINTS) {
                        let cp = m.u32(s + CRV_POINTS).wrapping_add((b as u32).wrapping_mul(CP_SIZE));
                        let w = rational(m, cp);
                        o.call(m, &SI_NURBS_CRV_MOVE_POINT, &[d, b as u32, w[0], w[1], w[2], w[3], w[4], w[5]]);
                        b += 1;
                    }
                    curve += 1;
                    if curve >= m.i16(src + FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            }
        }
        6 => spline(m, src, dst),
        9 => {
            let mut i = 0i32;
            while i < m.i32(src + CRV_NB_POINTS) {
                let cp = m.u32(src + CRV_POINTS).wrapping_add((i as u32).wrapping_mul(CP_SIZE));
                let w = rational(m, cp);
                o.call(m, &SI_NURBS_CRV_MOVE_POINT, &[dst, i as u32, w[0], w[1], w[2], w[3], w[4], w[5]]);
                i += 1;
            }
            let b = m.u8(dst + CRV_FLAGS);
            m.set_u8(dst + CRV_FLAGS, b | NURBS_MOVED);
        }
        10 => {
            let mut i = 0i32;
            while i < m.i32(src + SRF_NB_POINTS) {
                let off = (i as u32).wrapping_mul(CP_SIZE);
                let cp = m.u32(src + SRF_POINTS).wrapping_add(off);
                let w = rational(m, cp);
                // the weight handed on is the destination point's
                let dw = m.u64(m.u32(dst + SRF_POINTS).wrapping_add(off).wrapping_add(CP_W));
                o.call(m, &SI_NURBS_SRF_MOVE_POINT, &[dst, i as u32, w[0], w[1], w[2], w[3], w[4], w[5], dw as u32, (dw >> 32) as u32]);
                i += 1;
            }
            let b = m.u8(dst + SRF_FLAGS);
            m.set_u8(dst + SRF_FLAGS, b | NURBS_MOVED);
        }
        _ => {}
    }
}

/// One corner normal of a mesh whose normals are carried (part of `SI_latticeTransform`):
/// the source corner's normal times `n` (as a row vector, the fourth row added) goes to
/// the destination corner and is scaled to the source normal's length. Lengths are
/// float(sqrt((x² + y²) + z²)); each transformed coordinate is
/// float(((y·n[1][c] + x·n[0][c]) + z·n[2][c]) + n[3][c]).
fn carry_normal(m: &mut Mem, sp: u32, dp: u32, at: u32, n: &[[f32; 4]; 4]) {
    use geom::*;
    let length = |v: V3| ((x(v[0]) * v[0] + x(v[1]) * v[1]) + x(v[2]) * v[2]).sqrt().to_f32();
    let sn = m.u32(sp + POLY_CORNERS).wrapping_add(at).wrapping_add(CORNER_NORMAL);
    let old = length(rd3(m, sn));
    let sn = m.u32(sp + POLY_CORNERS).wrapping_add(at).wrapping_add(CORNER_NORMAL);
    let v = rd3(m, sn);
    for c in 0..3usize {
        let dn = m.u32(dp + POLY_CORNERS).wrapping_add(at).wrapping_add(CORNER_NORMAL);
        let t = (((x(v[1]) * n[1][c] + x(v[0]) * n[0][c]) + x(v[2]) * n[2][c]) + n[3][c]).to_f32();
        m.set_f32(dn + 4 * c as u32, t);
    }
    let dn = m.u32(dp + POLY_CORNERS).wrapping_add(at).wrapping_add(CORNER_NORMAL);
    let new = length(rd3(m, dn));
    if x(new).abs().gt(x(THIN)) {
        let ratio = x(old) / new;
        for c in [0u32, 4, 8] {
            let dn = m.u32(dp + POLY_CORNERS).wrapping_add(at).wrapping_add(CORNER_NORMAL);
            let v = (x(m.f32(dn + c)) * ratio).to_f32();
            m.set_f32(dn + c, v);
        }
    }
}

/// A node's active lattice and which set it is of [soft 0x0843ff1c SI_modelLattice]: the
/// first lattice of the branch set if it is active (`*which` = 1), else the first of the
/// node set if that is active (`*which` = 0), else null **with `*which` left alone**.
pub fn model_lattice(m: &mut Mem, model: Ptr32<Model>, which: Ptr32<i16>) -> Ptr32<Lattice> {
    if m[model].nb_lattices1 != 0 {
        let l = first(m, m[model].lattices1);
        if m[l].active != 0 {
            m.write(which, 1);
            return first(m, m[model].lattices1);
        }
    }
    if m[model].nb_lattices0 != 0 {
        let l = first(m, m[model].lattices0);
        if m[l].active != 0 {
            m.write(which, 0);
            return first(m, m[model].lattices0);
        }
    }
    Ptr32::NULL
}

/// The first lattice of the node set if it is active [soft 0x0843ff68 SI_modelNodeLattice].
pub fn model_node_lattice(m: &Mem, model: Ptr32<Model>) -> Ptr32<Lattice> {
    if m[model].nb_lattices0 != 0 {
        let l = first(m, m[model].lattices0);
        if m[l].active != 0 {
            return l;
        }
    }
    Ptr32::NULL
}

/// The first lattice of the branch set if it is active [soft 0x0843ff88 SI_modelTreeLattice].
pub fn model_tree_lattice(m: &Mem, model: Ptr32<Model>) -> Ptr32<Lattice> {
    if m[model].nb_lattices1 != 0 {
        let l = first(m, m[model].lattices1);
        if m[l].active != 0 {
            return l;
        }
    }
    Ptr32::NULL
}

/// Removes the animation of a lattice set and with it every lattice of the set but the
/// first [soft 0x0843ffa8 SI_modelDeleteLatticeAnimation]. Nothing happens when the set
/// has no element.
pub fn model_delete_lattice_animation(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, which: i16) {
    let idx_at = m.local(4);
    let elem = o.call(m, &SI_ED_GET_ELEM_FROM_LATTICE, &[model.addr(), which as u16 as u32, idx_at]);
    if elem != 0 {
        let chap = m[SI_CID][CID_LATTICES] as u32;
        o.call(m, &SI_SET_FCV_TABLE_ENTRIES_FROM_CHAP, &[chap]);
        let chap = m[SI_CID][CID_LATTICES] as u32;
        let (dict, idx) = (cur_dict(m), m.u32(idx_at));
        o.call(m, &SI_ELEMDICT_RESET_ELEM_ANIM, &[dict, elem, chap, idx, 1]);
        let count_at = model.addr() + if which != 0 { 0x54 } else { 0x4c };
        let set_at = count_at + 4;
        if m.u16(count_at) > 1 {
            let mut i: i16 = 1;
            if m.u16(count_at) as i32 > 1 {
                loop {
                    let slot = m.u32(set_at).wrapping_add((i as i32 as u32).wrapping_mul(4));
                    lattice_dispose(m, Ptr32::new(slot));
                    i = i.wrapping_add(1);
                    if i as i32 >= m.u16(count_at) as i32 {
                        break;
                    }
                }
            }
            m.set_u16(count_at, 1);
        }
    }
    m.release(4);
}

/// Takes the lattice at an index out of a set and frees it
/// [soft 0x084400ac SI_modelDeleteILattice]: 0 when the set has no array or the index is
/// outside it, else 1. The count is lowered before the lattice is freed and the pointers
/// behind it move down one place; the array keeps its size.
pub fn model_delete_i_lattice(m: &mut Mem, model: Ptr32<Model>, which: i16, index: i16) -> i32 {
    let count_at = model.addr() + if which != 0 { 0x54 } else { 0x4c };
    let set_at = count_at + 4;
    let set = m.u32(set_at);
    if set == 0 || index < 0 {
        return 0;
    }
    let at = index as i32;
    if at >= m.u16(count_at) as i32 {
        return 0;
    }
    let n = m.u16(count_at).wrapping_sub(1);
    m.set_u16(count_at, n);
    lattice_dispose(m, Ptr32::new(set.wrapping_add((at as u32).wrapping_mul(4))));
    if at >= m.u16(count_at) as i32 {
        return 1;
    }
    let mut i = index;
    loop {
        let set = m.u32(set_at);
        let c = (i as i32 as u32).wrapping_mul(4);
        i = i.wrapping_add(1);
        let next = m.u32(set.wrapping_add(c).wrapping_add(4));
        m.set_u32(set.wrapping_add(c), next);
        if i as i32 >= m.u16(count_at) as i32 {
            break;
        }
    }
    1
}

/// Exchanges a whole lattice set with an array of as many lattices
/// [soft 0x08440174 SI_modelSwapLattice] (undo and redo): 0 when `count` is not the set's
/// length. The set's element is pointed at the first lattice coming in (its word at +4),
/// then the pointers are exchanged one by one and `SI_latticeTouch` is told.
pub fn model_swap_lattice(m: &mut Mem, o: &mut dyn Outside, model: Ptr32<Model>, which: i16, count: i16, other: Ptr32<u32>) -> i32 {
    let count_at = model.addr() + if which != 0 { 0x54 } else { 0x4c };
    let set_at = count_at + 4;
    let n = count as i32;
    if n != m.u16(count_at) as i32 {
        return 0;
    }
    let set = m.u32(set_at);
    if set != 0 {
        let first = m.u32(set);
        let (dict, chap) = (cur_dict(m), m[SI_CID][CID_LATTICES] as u32);
        let id = o.call(m, &SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dict, chap, first]);
        let (dict, chap) = (cur_dict(m), m[SI_CID][CID_LATTICES] as u32);
        let elem = o.call(m, &SI_ELEMDICT_GET_ELEM, &[dict, chap, id]);
        if elem != 0 {
            let incoming = m.read(other);
            m.set_u32(elem.wrapping_add(4), incoming);
        }
    }
    let mut i = 0i32;
    while i < n {
        let set = m.u32(set_at);
        let off = (i as u32).wrapping_mul(4);
        let (mine, theirs) = (m.u32(set.wrapping_add(off)), m.u32(other.addr().wrapping_add(off)));
        m.set_u32(set.wrapping_add(off), theirs);
        m.set_u32(other.addr().wrapping_add(off), mine);
        i += 1;
    }
    o.call(m, &SI_LATTICE_TOUCH, &[model.addr(), which as i32 as u32]);
    1
}

/// Copies the points and flags of one lattice into another of the same counts
/// [soft 0x08440304 SI_latticeTransfer]; nothing when the counts differ (x as 16 bits, y
/// and z as one 32-bit word).
pub fn lattice_transfer(m: &mut Mem, from: Ptr32<Lattice>, to: Ptr32<Lattice>) {
    if m.u16(from.addr() + 2) != m.u16(to.addr() + 2) || m.u32(from.addr() + 4) != m.u32(to.addr() + 4) {
        return;
    }
    let (mut tk, mut tf) = (m[to].keys.addr(), m[to].flags.addr());
    let (mut fk, mut ff) = (m[from].keys.addr(), m[from].flags.addr());
    let mut left = points(m, to);
    while left > 0 {
        for c in [0u32, 4, 8] {
            let w = m.u32(fk.wrapping_add(c));
            m.set_u32(tk.wrapping_add(c), w);
        }
        let w = m.u16(ff);
        m.set_u16(tf, w);
        left -= 1;
        tk = tk.wrapping_add(0xc);
        fk = fk.wrapping_add(0xc);
        tf = tf.wrapping_add(2);
        ff = ff.wrapping_add(2);
    }
}
