// SPDX-License-Identifier: MIT
//! The spline primitives of `primitive.c` on `si-mem`'s memory image (finding 0023, section
//! "mesh1"): a circle, a square, an arc and a spiral as splines. A function takes the image and
//! the arguments the original takes, makes the original's allocation calls in its order
//! (`SI_splineAllocate`, one `SI_calloc` of the key block) and writes the keys in place.
//!
//! Floating point follows finding 0008: the original keeps each value in an x87 register
//! and rounds it to `float` where it stores it; the sine and cosine are the C library's
//! ([`si_x87::trig`], answered from the tape in a check).
//!
//! The original leaves some locals unset for a spline type other than 0, 2 and 3 (the
//! arc and the spiral of type 1, or a type above 3): the key count and the start values are
//! then whatever the stack held. The port takes them as 0 (an empty key block); nothing in the
//! program calls these with such a type.

use si_mem::{Mem, Ptr32};
use si_spline::spline_image::{spline_allocate, spline_update_matrix};
use si_types::geom::{Spline, SplineKey, Vector};
use si_x87::{trig, x, F80};

/// 2π as the double 0x08660440 (the circle) and 0x08660490 (the arc).
const TWO_PI: f64 = f64::from_bits(0x4019_21fb_5444_2d18);

/// The key block of `n` keys the way these four routines ask for it: `SI_calloc(n rounded up to a
/// multiple of 16, 0x30)` (the 16-key rounding of `SI_splineKeyAllocate`, without the
/// per-key initialisation: the block is only zeroed).
fn key_block(m: &mut Mem, n: i32) -> Ptr32<SplineKey> {
    let room = n.wrapping_sub(((n.wrapping_sub(1) as u32 & 0xf) as i32).wrapping_sub(0xf));
    Ptr32::new(m.si_calloc(room as u32, SplineKey::SIZE as u32))
}

/// A key at `p` with its two neighbours equal to it (`prev` and `next` are copies of `pos`).
fn put_key(m: &mut Mem, keys: Ptr32<SplineKey>, i: i32, p: Vector) {
    let k = keys.add(i);
    m[k].pos = p;
    m[k].next = p;
    m[k].prev = p;
}

/// A circle as a spline [soft 0x08487d54 SI_splineCircle]: `kind` is the spline type, `radius`
/// the radius and `n` the number of points. Closed (`+8` is 1), `n` keys on the circle starting
/// at angle 0 in steps of `2π / n` (rounded to float); radius `x · cos(angle)`, `y · sin(angle)`
/// computed in the register and rounded when stored; for type 2 the radius is first scaled by
/// `3 / (cos(2π/n) + 2)` (the circle through the control points of a quadratic B-spline), for
/// type 0 the step field (`+0x20`) is 1 and for type 3 the tension (`+0x1c`) is 0.5. The spline
/// update of the matrix ends it. Null when `SI_splineAllocate` is.
pub fn spline_circle(m: &mut Mem, kind: i32, radius: f32, n: i32) -> Ptr32<Spline> {
    let s = spline_allocate(m);
    if s.is_null() {
        return s;
    }
    let keys = key_block(m, n);
    m[s].keys = keys;
    // fild n; fld 2π; fdivrp: stored as float
    let step = (x(TWO_PI) / F80::from_i32(n)).to_f32();
    if kind == 0 {
        m[s].step = 1;
    }
    let mut radius = radius;
    if kind == 2 {
        let c = trig::cos(step as f64);
        radius = ((F80::from_f64(3.0) / (c + 2.0f64)) * (radius as f64)).to_f32();
    }
    if kind == 3 {
        m[s].tension = 0.5;
    }
    let mut angle = 0.0f32;
    for i in 0..n.max(0) {
        let c = trig::cos(angle as f64);
        let px = (c * x(radius)).to_f32();
        let sn = trig::sin(angle as f64);
        let py = (x(radius) * sn).to_f32();
        put_key(m, keys, i, Vector { x: px, y: py, z: 0.0 });
        angle = (x(angle) + step).to_f32();
    }
    m[s].closed = 1;
    m[s].nb_keys = n as i16;
    m[s].kind = kind;
    spline_update_matrix(m, s);
    s
}

/// A square in the XY plane as a closed linear spline [soft 0x084880a0 SI_splineSquare] of
/// four keys: with `h = size · 0.5` (the product rounded to float) the corners
/// `(-h,-h)`, `(h,-h)`, `(h,h)`, `(-h,h)`, z = 0. The spline type stays what
/// `SI_splineAllocate` gave it.
pub fn spline_square(m: &mut Mem, size: f32) -> Ptr32<Spline> {
    let h = (x(size) * 0.5f64).to_f32();
    let s = spline_allocate(m);
    if s.is_null() {
        return s;
    }
    let keys: Ptr32<SplineKey> = Ptr32::new(m.si_calloc(0x10, SplineKey::SIZE as u32));
    m[s].keys = keys;
    let nh = (-x(h)).to_f32();
    put_key(m, keys, 0, Vector { x: nh, y: nh, z: 0.0 });
    put_key(m, keys, 1, Vector { x: h, y: nh, z: 0.0 });
    put_key(m, keys, 2, Vector { x: h, y: h, z: 0.0 });
    put_key(m, keys, 3, Vector { x: nh, y: h, z: 0.0 });
    m[s].closed = 1;
    m[s].nb_keys = 4;
    spline_update_matrix(m, s);
    s
}

/// An arc as a spline [soft 0x084881a4 SI_splineArc]: `kind` the spline type, `radius`, the start
/// and end angles (radians) and `n` the number of segments. The sweep `end - start` (one
/// register value) gets 2π added when it is negative (compare ordered: a NaN is left); the step
/// is the sweep over `n`, rounded to float. Type 0 (linear) takes `n + 1` keys from the start
/// angle with step field 1; types 2 and 3 take `n + 3`, one step before the start (type 2 has the
/// radius scaled by `3 / (cos(step) + 2)`, type 3 the tension 0.5); an open spline in every
/// case. Type 1 and the others: see the module's top.
pub fn spline_arc(m: &mut Mem, kind: i32, radius: f32, start: f32, end: f32, n: i16) -> Ptr32<Spline> {
    let s = spline_allocate(m);
    if s.is_null() {
        return s;
    }
    let mut sweep = x(end) - x(start);
    if sweep.lt(F80::from_i32(0)) {
        // fadd 2π, fstp float, fld
        sweep = x((sweep + TWO_PI).to_f32());
    }
    let step = (sweep / F80::from_i32(n as i32)).to_f32();
    let mut radius = radius;
    let (mut angle, count) = if kind == 2 {
        let c = trig::cos(step as f64);
        radius = ((F80::from_f64(3.0) / (c + 2.0f64)) * (radius as f64)).to_f32();
        ((x(start) - step).to_f32(), n as i32 + 3)
    } else if kind == 3 {
        m[s].tension = 0.5;
        ((x(start) - step).to_f32(), n as i32 + 3)
    } else if kind == 0 {
        m[s].step = 1;
        (x(start).to_f32(), n as i32 + 1)
    } else {
        (0.0, 0)
    };
    let keys = key_block(m, count);
    m[s].keys = keys;
    for i in 0..count.max(0) {
        let c = trig::cos(angle as f64);
        let px = (c * x(radius)).to_f32();
        let sn = trig::sin(angle as f64);
        let py = (x(radius) * sn).to_f32();
        put_key(m, keys, i, Vector { x: px, y: py, z: 0.0 });
        angle = (x(angle) + step).to_f32();
    }
    m[s].nb_keys = count as i16;
    m[s].kind = kind;
    spline_update_matrix(m, s);
    s
}

/// A spiral as a spline [soft 0x08488a44 SI_splineSpiral]: `kind`, the radii `r0` and `r1`, the
/// angles `a0` and `a1`, the rise `height` and the number of segments `n`. Per segment the angle
/// grows by `(a1 - a0) / n`, the radius by `(r1 - r0) / n` (types 0 and 3: kept in the register
/// unrounded, added to the radius there) and z by `height / n` (float); the keys are
/// `(r cos a, r sin a, z)`. Type 0: `n + 1` keys from `(a0, r0, 0)`, step field 1. Type 2: both
/// radii scaled by `3 / (|cos(da)| + 2)` (the two products rounded to float), the radius step
/// taken from the scaled radii, `n + 3` keys starting one step back
/// (`a0 - da`, `R0 - dr`, `-height/n`). Type 3: tension 0.5, `n + 3` keys one step back from
/// `(a0, r0, 0)`. Open in every case. Type 1 and the others: see the module's top.
pub fn spline_spiral(m: &mut Mem, kind: i32, r0: f32, r1: f32, a0: f32, a1: f32, height: f32, n: i16) -> Ptr32<Spline> {
    let s = spline_allocate(m);
    if s.is_null() {
        return s;
    }
    let nf = F80::from_i32(n as i32);
    let da = ((x(a1) - x(a0)) / nf).to_f32();
    let mut dr: F80 = (x(r1) - x(r0)) / nf;
    let hn = (x(height) / nf).to_f32();
    let (mut angle, mut rad, mut z, count);
    if kind == 2 {
        // the cosine is taken twice, with the same argument
        let c = trig::cos(da as f64).abs();
        let r0s = ((F80::from_f64(3.0) / (c + 2.0f64)) * r0).to_f32();
        let c = trig::cos(da as f64).abs();
        let r1s = ((F80::from_f64(3.0) / (c + 2.0f64)) * r1).to_f32();
        // fdiv by the float form of n
        dr = (x(r1s) - x(r0s)) / x(nf.to_f32());
        angle = (x(a0) - da).to_f32();
        rad = (x(r0s) - dr).to_f32();
        z = (-x(hn)).to_f32();
        count = n as i32 + 3;
    } else if kind == 3 {
        m[s].tension = 0.5;
        angle = (x(a0) - da).to_f32();
        rad = (x(r0) - dr).to_f32();
        z = (-x(hn)).to_f32();
        count = n as i32 + 3;
    } else if kind == 0 {
        angle = x(a0).to_f32();
        rad = x(r0).to_f32();
        z = 0.0;
        count = n as i32 + 1;
        m[s].step = 1;
    } else {
        angle = 0.0;
        rad = 0.0;
        z = 0.0;
        count = 0;
    }
    let keys = key_block(m, count);
    m[s].keys = keys;
    for i in 0..count.max(0) {
        let c = trig::cos(angle as f64);
        let px = (c * x(rad)).to_f32();
        let sn = trig::sin(angle as f64);
        let py = (x(rad) * sn).to_f32();
        put_key(m, keys, i, Vector { x: px, y: py, z });
        let na = (x(angle) + da).to_f32();
        let nr = (x(rad) + dr).to_f32();
        let nz = (x(z) + hn).to_f32();
        angle = na;
        rad = nr;
        z = nz;
    }
    m[s].nb_keys = count as i16;
    m[s].kind = kind;
    spline_update_matrix(m, s);
    s
}

/// A circle as a NURBS curve [soft 0x08487ee0 SI_nurbsCircle]: `radius`, `order` (2, 3 or 4)
/// and `n` points. The curve gets order and point count as asked, `n + 1` knots, closed (+0x20)
/// 1, uniformity (+0x24) 0 and for order 2 the closing field (+0x34) 1. Step `2π / n` is a
/// double; the radius is scaled for order 4 by `3 / (cos(step) + 2)` and for order 3 by
/// `1 / cos(step / 2)` (the register result rounded to float). The angle runs in the register
/// from 0 downwards by the step; each point gets `x = cos(a)·r`, `y = sin(a)·r` (rounded to
/// double), z 0, and a coordinate below 1e-6 in magnitude is set to 0. The knots then come from
/// `SI_knotsCrvReparametrization`. Null when the curve allocation is.
pub fn nurbs_circle(m: &mut Mem, radius: f32, order: i32, n: i32) -> Result<u32, si_nurbs::image::Halt> {
    use si_hrc::image::nurbs::{crv, nurbs_crv_allocate, nurbs_points_allocate};
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return Ok(0);
    }
    m.set_i32(c, order);
    m.set_i32(c + 4, n);
    m.set_i32(c + 8, n.wrapping_add(1));
    m.set_u16(c + 0x20, 1);
    m.set_u32(c + 0x24, 0);
    if order == 2 {
        m.set_u32(c + 0x34, 1);
    }
    let pts = nurbs_points_allocate(m, m.i32(c + 4));
    m.set_u32(c + crv::POINTS, pts);
    let step: f64 = (x(TWO_PI) / F80::from_i32(n)).to_f64();
    let mut radius = radius;
    if order == 4 {
        let cs = trig::cos(step);
        radius = ((F80::from_f64(3.0) / (cs + 2.0f64)) * (radius as f64)).to_f32();
    } else if order == 3 {
        let cs = trig::cos((x(step) * 0.5f64).to_f64());
        radius = (x(radius as f64) / cs).to_f32();
    }
    let mut angle = F80::from_i32(0);
    let mut i = 0;
    while i < m.i32(c + 4) {
        let p = m.u32(c + crv::POINTS) + 0x24 * i as u32;
        let a = angle.to_f64();
        let px = (trig::cos(a) * x(radius)).to_f64();
        m.set_f64(p, px);
        let a = angle.to_f64();
        let py = (trig::sin(a) * x(radius)).to_f64();
        m.set_f64(p + 8, py);
        for off in [0u32, 8, 0x10] {
            if x(m.f64(p + off)).abs().lt(F80::from_f64(1e-6)) {
                m.set_u32(p + off, 0);
                m.set_u32(p + off + 4, 0);
            }
        }
        angle = angle - step;
        i += 1;
    }
    si_nurbs::image::knots_crv_reparametrization(m, c)?;
    Ok(c)
}




/// The profile of a sphere or torus, on the spline `s` with `count` keys, and the revolution of
/// it into a patch with the spline given back afterwards: the tail the two patch primitives
/// share.
#[allow(clippy::too_many_arguments)]
fn revolve_and_dispose(m: &mut Mem, o: &mut dyn si_spline::spline_image::Outside, s: Ptr32<Spline>, rev_kind: i32, segs: i16, def: i32) -> u32 {
    let patch = si_spline::revolution_image::spline_revolution_to_patch(m, o, s, rev_kind, segs, 6.2831855f32, 1, 0, 1, def);
    // SI_splineDispose(&spline) through a stack slot.
    let sp = m.sp();
    let slot = m.local(4);
    m.set_u32(slot, s.addr());
    si_spline::intersect_image::spline_dispose(m, slot);
    m.set_sp(sp);
    patch.addr()
}

/// A sphere as a patch [soft 0x08486910 SI_patchSphere]: the profile is a half circle of `n`
/// segments in the XY plane (a spline of type `spline_type`: `n + 3` keys from one step before
/// the south pole at `-π/2`, `n + 1` from the pole for type 0), turned once around the Y axis
/// into `segs` columns by `SI_splineRevolutionToPatch` (kind `rev_kind`; `def` 1 when
/// `rev_kind` is 0 and 3 otherwise) and the profile freed. The step is `π / n` rounded to float;
/// type 2 scales the radius by `3 / (cos(step) + 2)`, type 3 sets the tension to 0.5, type 0 the
/// step field to 1 (3 otherwise). The keys carry the position only (`neighbours` stay zero) and
/// the spline matrix is not updated. Returns the patch address.
pub fn patch_sphere(m: &mut Mem, o: &mut dyn si_spline::spline_image::Outside, radius: f32, n: i16, segs: i16, rev_kind: i32, spline_type: i32) -> u32 {
    let def = if rev_kind == 0 { 1 } else { 3 };
    let s = spline_allocate(m);
    if s.is_null() {
        return 0;
    }
    let step = (x(std::f64::consts::PI) / F80::from_i32(n as i32)).to_f32();
    let mut start = (x(-std::f64::consts::FRAC_PI_2) - step).to_f32();
    let mut count = n as i32 + 3;
    let mut radius = radius;
    if spline_type == 2 {
        let c = trig::cos(step as f64);
        radius = ((F80::from_f64(3.0) / (c + 2.0f64)) * (radius as f64)).to_f32();
    } else if spline_type == 0 {
        start = -1.5707963705062866f32;
        count = n as i32 + 1;
    } else if spline_type == 3 {
        m[s].tension = 0.5;
    }
    m[s].kind = spline_type;
    m[s].nb_keys = count as i16;
    m[s].step = if spline_type == 0 { 1 } else { 3 };
    let keys = key_block(m, count);
    m[s].keys = keys;
    let mut a = start;
    for i in 0..count.max(0) {
        let c = trig::cos(a as f64);
        let px = (c * x(radius)).to_f32();
        let sn = trig::sin(a as f64);
        let py = (x(radius) * sn).to_f32();
        m[keys.add(i)].pos = Vector { x: px, y: py, z: 0.0 };
        a = (x(a) + step).to_f32();
    }
    revolve_and_dispose(m, o, s, rev_kind, segs, def)
}

/// A torus as a patch [soft 0x08486b7c SI_patchTorus]: the profile is a closed circle of `n`
/// keys at distance `offset` from the axis (key x `cos(a)·r + offset`, y `r·sin(a)`, rounded to
/// float when stored), from angle 0 in steps of `2π / n`; the rest as [`patch_sphere`] (type 2
/// scaling of the radius, tension 0.5 for type 3, the step field 1 for type 0 and 3 otherwise,
/// closed 1, `nb_keys` `n`, the same revolution and disposal, no matrix update).
pub fn patch_torus(m: &mut Mem, o: &mut dyn si_spline::spline_image::Outside, radius: f32, offset: f32, n: i16, segs: i16, rev_kind: i32, spline_type: i32) -> u32 {
    let def = if rev_kind == 0 { 1 } else { 3 };
    let s = spline_allocate(m);
    if s.is_null() {
        return 0;
    }
    let step = (x(TWO_PI) / F80::from_i32(n as i32)).to_f32();
    let mut radius = radius;
    if spline_type == 2 {
        let c = trig::cos(step as f64);
        radius = ((F80::from_f64(3.0) / (c + 2.0f64)) * (radius as f64)).to_f32();
    } else if spline_type == 3 {
        m[s].tension = 0.5;
    }
    m[s].kind = spline_type;
    m[s].closed = 1;
    m[s].nb_keys = n;
    m[s].step = if spline_type == 0 { 1 } else { 3 };
    let count = n as i32;
    let keys = key_block(m, count);
    m[s].keys = keys;
    let mut a = 0.0f32;
    for i in 0..count.max(0) {
        let c = trig::cos(a as f64);
        let px = (c * x(radius) + x(offset)).to_f32();
        let sn = trig::sin(a as f64);
        let py = (x(radius) * sn).to_f32();
        m[keys.add(i)].pos = Vector { x: px, y: py, z: 0.0 };
        a = (x(a) + step).to_f32();
    }
    revolve_and_dispose(m, o, s, rev_kind, segs, def)
}

/// A spiral as a NURBS curve [soft 0x08488cbc SI_nurbsSpiral]: radii `r0`, `r1`, angles `a0`,
/// `a1`, `height`, the curve order (2, 3 or 4) and `n` segments. Order 2 is the linear
/// spiral spline ([`spline_spiral`] type 0) converted by `SI_spline2NurbsCrvKeepCurve`; order 4
/// the type 2 spiral spline with step field 10, converted the same way (the spline is not freed
/// in either); both then have uniformity (+0x24) 0. Order 3 builds the curve itself: `n + 1`
/// points, `n + 2` knots, the angle step `da = (a1 - a0)/n` (the difference first rounded to
/// float), the two radii scaled by `3 / (|cos da| + 2)`, the radius step from the scaled radii,
/// the keys starting one step back with z kept in the register; point x, y, z are doubles.
/// All end in `SI_knotsCrvReparametrization` (order 3) and `SI_nurbsCrvInverse`. An order
/// other than 2, 3 and 4 hands the original a null curve (not made).
#[allow(clippy::too_many_arguments)]
pub fn nurbs_spiral(m: &mut Mem, o: &mut dyn si_mem::Outside, r0: f32, r1: f32, a0: f32, a1: f32, height: f32, order: i32, n: i16) -> Result<u32, si_nurbs::image::Halt> {
    use si_hrc::image::nurbs::{crv, nurbs_crv_allocate, nurbs_points_allocate};
    let c;
    if order == 2 || order == 4 {
        let s = spline_spiral(m, if order == 2 { 0 } else { 2 }, r0, r1, a0, a1, height, n);
        if order == 4 && !s.is_null() {
            m[s].step = 10;
        }
        c = si_nurbs::conv6::spline_to_crv_keep_curve(m, o, s.addr())?;
        if c != 0 {
            m.set_u32(c + 0x24, 0);
        }
    } else if order == 3 {
        let nf = x((n as i32) as f32 as f64);
        let da = (x((x(a1) - x(a0)).to_f32()) / nf).to_f32();
        let hn = (x(height) / nf).to_f32();
        c = nurbs_crv_allocate(m).addr();
        if c == 0 {
            return Ok(0);
        }
        m.set_i32(c, 3);
        m.set_i32(c + 4, n as i32 + 1);
        m.set_i32(c + 8, n as i32 + 2);
        m.set_u32(c + 0x24, 0);
        let pts = nurbs_points_allocate(m, n as i32 + 1);
        m.set_u32(c + crv::POINTS, pts);
        let cs = trig::cos(da as f64).abs();
        let r0s = ((F80::from_f64(3.0) / (cs + 2.0f64)) * r0).to_f32();
        let cs = trig::cos(da as f64).abs();
        let r1s = ((F80::from_f64(3.0) / (cs + 2.0f64)) * r1).to_f32();
        let dr = (x(r1s) - x(r0s)) / nf;
        let mut angle = (x(a0) - da).to_f32();
        let dr32 = dr.to_f32();
        let mut rad = (x(r0s) - dr).to_f32();
        let mut z: F80 = -x(hn);
        let mut i = 0;
        while i < m.i32(c + 4) {
            let p = m.u32(c + crv::POINTS) + 0x24 * i as u32;
            let cv = trig::cos(angle as f64);
            m.set_f64(p, (cv * x(rad)).to_f64());
            let sv = trig::sin(angle as f64);
            m.set_f64(p + 8, (sv * x(rad)).to_f64());
            m.set_f64(p + 0x10, z.to_f64());
            let na = (x(angle) + da).to_f32();
            let nr = (x(rad) + dr32).to_f32();
            z = z + hn;
            angle = na;
            rad = nr;
            i += 1;
        }
        si_nurbs::image::knots_crv_reparametrization(m, c)?;
    } else {
        return Ok(0);
    }
    si_nurbs::image::crv_inverse(m, o, c);
    Ok(c)
}
