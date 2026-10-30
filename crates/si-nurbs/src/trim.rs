// SPDX-License-Identifier: MIT
//! The original's `trim.c` (finding 0044, section trim3): the trim record (a surface, a curve, the lists of rays,
//! orientations, intersections and composite curves of one trimming or projecting operation) and the steps of it.
//!
//! Records (names are ours): a trim is 0x104 bytes: the surface at +0, the surface's ray-trace slot at +4 (the
//! tree root cell handed to the ray tracer), the caller's curve at +8, its transformed copy at +0xc, the ray list
//! at +0x10, the orientation list at +0x14, the intersection / composite lists at +0x18 .. +0x30, the tree at
//! +0x34, the operation, projection and coordinate types at +0x38 / +0x3c / +0x40, the centre (3 doubles) at +0x44,
//! the direction (3 doubles) at +0x5c, the flattened curve at +0x74, two 4 x 4 float matrices at +0x78 / +0xb8,
//! the step at +0xf8 and the surface operation at +0xfc.

use si_mem::{Mem, Outside, Routine};
use si_x87::{Round, F80};

#[path = "../../si-gen/src/list.rs"]
#[allow(dead_code)]
mod genlist;

pub const RAY_DESTROY: Routine = Routine::new("si_nurbsRayDestroy", 0x0850_db00);
pub const ORIENT_DESTROY: Routine = Routine::new("si_nurbsCurveOrientDestroy", 0x0850_a36c);
pub const CRV_DISPOSE: Routine = Routine::new("SI_nurbsCrvDispose", 0x084d_b870);
pub const INTER_DESTROY: Routine = Routine::new("si_nurbsCurveInterDestroy", 0x0850_758c);
pub const COMP_DESTROY: Routine = Routine::new("si_nurbsCompositeCurveDestroy", 0x0850_3878);
pub const CRV_COPY: Routine = Routine::new("SI_nurbsCrvCopy", 0x084d_bbf4);
pub const CRV_APPLY_MATRIX: Routine = Routine::new("SI_nurbsCrvApplyMatrix", 0x084e_9750);
pub const CRV_FLATTEN: Routine = Routine::new("SI_nurbsCrvFlatten", 0x084e_62f4);
pub const TREE_DESTROY: Routine = Routine::new("SI_genTreeDestroy", 0x0851_5848);
pub const ORIENT_ORIENT: Routine = Routine::new("si_nurbsCurveOrientOrient", 0x0850_a424);
pub const SHOW_ACTIVITY: Routine = Routine::new("SI_showActivity", 0x083d_1a60);
pub const RAY_CONNECT_NO_TRANS: Routine = Routine::new("si_nurbsRayConnectNoTrans", 0x0850_e8c4);
pub const RAY_CONNECT_TRANS: Routine = Routine::new("si_nurbsRayConnectTrans", 0x0850_e9a8);
pub const RAY_TRACE_SRF2: Routine = Routine::new("si_nurbsRayTraceSrf2", 0x0851_1240);
pub const STATUS_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
pub const CRV_VALUE: Routine = Routine::new("SI_nurbsCrvValue", 0x084d_3550);

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

fn dbl(w: &[u32; 6], k: usize) -> f64 {
    f64::from_bits(w[2 * k] as u64 | (w[2 * k + 1] as u64) << 32)
}

fn fl(m: &Mem, at: u32) -> F80 {
    F80::from_f32(f32::from_bits(m.u32(at)))
}

/// A 3 x 3 part of one of the trim's matrices applied to a vector, the way `SetDirection` and `SetCenter` do it
/// (all in the register, rounded at the store). `base` is the matrix (+0x78 or +0xb8); `centre` adds the translation row.
/// Returns the three results (x', y', z') as stored at the first, second and third double.
fn transform(m: &Mem, t: u32, v: &[u32; 6], centre: bool) -> [f64; 3] {
    let b = t + if m.u32(t + 0x40) == 1 { 0xb8 } else { 0x78 };
    let (px, py, pz) = (x(dbl(v, 0)), x(dbl(v, 1)), x(dbl(v, 2)));
    // x' = ((y * m[2][0]... ) the order of the sums is the original's (kit x87)
    let a = (py * fl(m, b + 0x10) + px * fl(m, b)) + pz * fl(m, b + 0x20);
    let c = (py * fl(m, b + 0x14) + px * fl(m, b + 4)) + pz * fl(m, b + 0x24);
    let d = pz * fl(m, b + 0x28) + (py * fl(m, b + 0x18) + px * fl(m, b + 8));
    if centre {
        // translation row at +0x30 (x), +0x34 (y), +0x38 (z); the centre is stored x, y, z at +0x44, +0x4c, +0x54
        let a = a + fl(m, b + 0x30);
        let c = c + fl(m, b + 0x34);
        let d = d + fl(m, b + 0x38);
        [a.to_f64(), c.to_f64(), d.to_f64()]
    } else {
        [a.to_f64(), c.to_f64(), d.to_f64()]
    }
}

/// [soft 0x0850f7d8 si_nurbsTrimSetDirection]: `(trim, dvec[3 doubles])`: nothing for a null trim; coordinate type 1 or 2
/// stores the vector rotated by the second or first matrix (3 x 3 part) at +0x5c / +0x64 / +0x6c, any other type copies the
/// 24 bytes.
pub fn set_direction(m: &mut Mem, t: u32, v: [u32; 6]) {
    if t == 0 {
        return;
    }
    let ty = m.u32(t + 0x40);
    if ty == 1 || ty == 2 {
        let r = transform(m, t, &v, false);
        m.set_f64(t + 0x5c, r[0]);
        m.set_f64(t + 0x6c, r[2]);
        m.set_f64(t + 0x64, r[1]);
    } else {
        for (i, w) in v.iter().enumerate() {
            m.set_u32(t + 0x5c + 4 * i as u32, *w);
        }
    }
}

/// [soft 0x0850fa6c si_nurbsTrimSetCoordDirection]: `(trim, type, dvec)`: stores the type at +0x40, then [`set_direction`].
pub fn set_coord_direction(m: &mut Mem, t: u32, ty: u32, v: [u32; 6]) {
    if t != 0 {
        m.set_u32(t + 0x40, ty);
        set_direction(m, t, v);
    }
}

/// [soft 0x0850f7ac si_nurbsTrimSetSurfaceOp]: `(trim, op)`: `op` at +0xfc.
pub fn set_surface_op(m: &mut Mem, t: u32, op: u32) {
    if t != 0 {
        m.set_u32(t + 0xfc, op);
    }
}

/// [soft 0x0850f8fc si_nurbsTrimSetCenter]: `(trim, point[3 doubles])`: as [`set_direction`] but with the matrix's
/// translation row added, stored at +0x44 / +0x4c / +0x54.
pub fn set_center(m: &mut Mem, t: u32, v: [u32; 6]) {
    if t == 0 {
        return;
    }
    let ty = m.u32(t + 0x40);
    if ty == 1 || ty == 2 {
        let r = transform(m, t, &v, true);
        m.set_f64(t + 0x44, r[0]);
        m.set_f64(t + 0x54, r[2]);
        m.set_f64(t + 0x4c, r[1]);
    } else {
        for (i, w) in v.iter().enumerate() {
            m.set_u32(t + 0x44 + 4 * i as u32, *w);
        }
    }
}

/// [soft 0x0850f7c4 si_nurbsTrimSetProjection]: `(trim, type)`: `type` at +0x3c.
pub fn set_projection(m: &mut Mem, t: u32, ty: u32) {
    if t != 0 {
        m.set_u32(t + 0x3c, ty);
    }
}

/// [soft 0x0850fa58 si_nurbsTrimSetTrimming]: `(trim, type)`: `type` at +0x38.
pub fn set_trimming(m: &mut Mem, t: u32, ty: u32) {
    if t != 0 {
        m.set_u32(t + 0x38, ty);
    }
}

/// [soft 0x0850faa4 si_nurbsTrimSetCoordCenter]: `(trim, type, point)`: stores the type at +0x40, then [`set_center`].
pub fn set_coord_center(m: &mut Mem, t: u32, ty: u32, v: [u32; 6]) {
    if t != 0 {
        m.set_u32(t + 0x40, ty);
        set_center(m, t, v);
    }
}

/// [soft 0x0850fadc si_nurbsTrimSetStep]: `(trim, step)`: when the trim exists and its step (+0xf8) differs: stores it there
/// and in the trim's curve copy (+0xc, at +0x34), disposes the flattened curve (+0x74) when there is one and makes a new one
/// with `SI_nurbsCrvFlatten` (outside).
pub fn set_step(m: &mut Mem, o: &mut dyn Outside, t: u32, step: u32) {
    if t == 0 || m.u32(t + 0xf8) == step {
        return;
    }
    let c = m.u32(t + 0xc);
    m.set_u32(t + 0xf8, step);
    m.set_u32(c + 0x34, step);
    if m.u32(t + 0x74) != 0 {
        o.call(m, CRV_DISPOSE, &[t + 0x74]);
    }
    let f = o.call(m, CRV_FLATTEN, &[c]).eax;
    m.set_u32(t + 0x74, f);
}

/// The arguments of `SI_nurbsTrimCreateP` after the surface and the curve.
pub struct CreateArgs {
    pub trimming: u32,
    pub projection: u32,
    pub coord: u32,
    pub center: [u32; 6],
    pub dir: [u32; 6],
    pub mat_a: u32,
    pub mat_b: u32,
    pub step: u32,
    pub surface_op: u32,
}

/// [soft 0x0850f580 SI_nurbsTrimCreateP]: `(srf, crv, trimming, projection, coord, center, dir, mat_a, mat_b, step, op)`:
/// a zeroed 0x104 block (0 if that fails); the two matrices (16 floats each) copied to +0x78 and +0xb8, the surface and curve
/// stored at +0 / +8, a copy of the curve at +0xc with the second matrix applied (mask 0), then the setters in order:
/// projection, coord/direction, centre, trimming, step, surface operation.
pub fn create_p(m: &mut Mem, o: &mut dyn Outside, srf: u32, crv: u32, a: &CreateArgs) -> u32 {
    let t = m.si_calloc(1, 0x104);
    if t != 0 {
        for i in 0..16u32 {
            let w = m.u32(a.mat_a + 4 * i);
            m.set_u32(t + 0x78 + 4 * i, w);
        }
        for i in 0..16u32 {
            let w = m.u32(a.mat_b + 4 * i);
            m.set_u32(t + 0xb8 + 4 * i, w);
        }
        m.set_u32(t, srf);
        m.set_u32(t + 8, crv);
        // SI_nurbsCrvCopy / SI_nurbsCrvApplyMatrix live on the image in `image.rs`; they are reached through the outside
        // world here like the other curve routines of the file, so that a run-time can wire either form
        let c = o.call(m, CRV_COPY, &[crv]).eax;
        m.set_u32(t + 0xc, c);
        let c = m.u32(t + 0xc);
        o.call(m, CRV_APPLY_MATRIX, &[c, a.mat_b, 0]);
        set_projection(m, t, a.projection);
        set_coord_direction(m, t, a.coord, a.dir);
        set_center(m, t, a.center);
        set_trimming(m, t, a.trimming);
        set_step(m, o, t, a.step);
        set_surface_op(m, t, a.surface_op);
    }
    t
}

/// [soft 0x0850f678 SI_nurbsTrimDestroy]: `(pp)`: for a non-null `*pp`: clears the surface's preview pointer (+0xd0),
/// destroys the lists at +0x10 (rays), +0x14 (orientations), +0x20 (curves), +0x18, +0x1c (intersection lists), +0x24, +0x28
/// (composite curves), +0x30, +0x2c (intersection lists), the tree at +0x34 (outside), disposes the curves at +0xc and +0x74,
/// frees the trim and clears `*pp`.
pub fn destroy(m: &mut Mem, o: &mut dyn Outside, pp: u32) {
    if pp == 0 || m.u32(pp) == 0 {
        return;
    }
    let t = m.u32(pp);
    let srf = m.u32(t);
    m.set_u32(srf + 0xd0, 0);
    let steps: [(u32, Routine); 9] = [
        (0x10, RAY_DESTROY),
        (0x14, ORIENT_DESTROY),
        (0x20, CRV_DISPOSE),
        (0x18, INTER_DESTROY),
        (0x1c, INTER_DESTROY),
        (0x24, COMP_DESTROY),
        (0x28, COMP_DESTROY),
        (0x30, INTER_DESTROY),
        (0x2c, INTER_DESTROY),
    ];
    for (off, cb) in steps {
        let t = m.u32(pp);
        genlist::destroy(m, o, t + off, cb.addr);
    }
    let t = m.u32(pp);
    o.call(m, TREE_DESTROY, &[t + 0x34, 0]);
    // the two curve disposals go through the outside world like the list callback does (the routine lives in si-hrc)
    let t = m.u32(pp);
    o.call(m, CRV_DISPOSE, &[t + 0xc]);
    let t = m.u32(pp);
    o.call(m, CRV_DISPOSE, &[t + 0x74]);
    let t = m.u32(pp);
    if t != 0 {
        m.free(t);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x0850fb2c si_nurbsTrimShowPreview]: `(trim, show)`: the surface's preview pointer (+0xd0) becomes 0 for `show == 0`
/// (a 16-bit test), else the trim's orientation list (+0x14).
pub fn show_preview(m: &mut Mem, t: u32, show: u16) {
    if t != 0 {
        let srf = m.u32(t);
        let v = if show == 0 { 0 } else { m.u32(t + 0x14) };
        m.set_u32(srf + 0xd0, v);
    }
}

/// [soft 0x0850ff1c si_nurbsTrimGetRayList]: the ray list (+0x10); 0 for a null trim.
pub fn get_ray_list(m: &Mem, t: u32) -> u32 {
    if t == 0 { 0 } else { m.u32(t + 0x10) }
}

/// [soft 0x0850ff38 si_nurbsTrimGetOrientList]: the orientation list (+0x14); 0 for a null trim.
pub fn get_orient_list(m: &Mem, t: u32) -> u32 {
    if t == 0 { 0 } else { m.u32(t + 0x14) }
}

/// [soft 0x0850ff50 si_nurbsTrimSetRayList]: stores the list at +0x10 and answers it; 0 for a null trim.
pub fn set_ray_list(m: &mut Mem, t: u32, l: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    m.set_u32(t + 0x10, l);
    l
}

/// [soft 0x0850ff68 si_nurbsTrimSetOrientList]: stores the list at +0x14 and answers it; 0 for a null trim.
pub fn set_orient_list(m: &mut Mem, t: u32, l: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    m.set_u32(t + 0x14, l);
    l
}

/// [soft 0x08510328 si_nurbsTrimStartAtZero]: `(list)` of rays: for a list with a head different from its tail
/// (+0 / +4) and flag bit 0 set, when the head ray has intersections (`si_nurbsRayGetNoInter`): the first ray in iteration
/// order without any becomes the head (the tail takes that element's previous link). Answers the list.
pub fn start_at_zero(m: &mut Mem, list: u32) -> u32 {
    if list == 0 {
        return 0;
    }
    if m.u32(list) == 0 || m.u32(list) == m.u32(list + 4) {
        return list;
    }
    let flags = genlist::get_flags(m, list) as u8;
    if (flags ^ 1) & 1 != 0 {
        return list;
    }
    let head = m.u32(list);
    let d = genlist::elem_get_data(m, head);
    if crate::inter::ray_get_no_inter(m, d) == 0 {
        return list;
    }
    let f = m.local(4);
    let mut e = genlist::iter_init(m, list, f, f + 2);
    while m.i16(f) != 1 {
        let d = genlist::elem_get_data(m, e);
        if crate::inter::ray_get_no_inter(m, d) == 0 {
            break;
        }
        e = genlist::iter_next(m, list, f, f + 2);
    }
    m.release(4);
    if e != 0 {
        let p = m.u32(e + 4);
        m.set_u32(list + 4, p);
        m.set_u32(list, e);
    }
    list
}

/// [soft 0x085103e8 si_nurbsTrimClearInterLinks]: `(rays)`: every intersection of every ray gets flags (+0x50), previous (+0x70)
/// and next (+0x74) cleared. Answers the list (0 for null).
pub fn clear_inter_links(m: &mut Mem, rays: u32) -> u32 {
    if rays == 0 {
        return 0;
    }
    let f = m.local(8);
    let mut e = genlist::iter_init(m, rays, f, f + 2);
    if m.i16(f) != 1 {
        loop {
            let ray = genlist::elem_get_data(m, e);
            let il = crate::inter::ray_get_inters(m, ray);
            let mut ie = genlist::iter_init(m, il, f + 4, f + 6);
            while m.i16(f + 4) != 1 {
                let i = genlist::elem_get_data(m, ie);
                m.set_u32(i + 0x50, 0);
                m.set_u32(i + 0x74, 0);
                m.set_u32(i + 0x70, 0);
                ie = genlist::iter_next(m, il, f + 4, f + 6);
            }
            e = genlist::iter_next(m, rays, f, f + 2);
            if m.i16(f) == 1 {
                break;
            }
        }
    }
    m.release(8);
    rays
}

/// [soft 0x08510aac si_nurbsTrimComputeLineInter]: `(a, b, c, d, out_pt, out_s, out_t)` with the four points as three doubles each
/// (only x, y are used): the intersection of the lines a-b and c-d. With `det = (c.x - d.x) * (b.y - a.y) - (c.y - d.y) * (b.x - a.x)`
/// in the register: answers 0 when |det| < 1e-6 (0x086620b8), else 1 with `*out_s = ((c.y - d.y) * (a.x - c.x) - (c.x - d.x) *
/// (a.y - c.y)) / det`, `*out_t = (round64((a.y - c.y) * round64(b.x - a.x)) - (a.x - c.x) * (b.y - a.y)) / det` and
/// `out_pt = c + t * (d - c)` (x, y; the differences taken in doubles). The 80-bit sums keep their register width except where
/// the original stores and reloads (the rounded copies of `b.x - a.x`, of the product and of d - c).
pub fn compute_line_inter(m: &mut Mem, a: [u32; 6], b: [u32; 6], c: [u32; 6], d: [u32; 6], out_pt: u32, out_s: u32, out_t: u32) -> u32 {
    let (ax, ay) = (x(dbl(&a, 0)), x(dbl(&a, 1)));
    let (bx, by) = (x(dbl(&b, 0)), x(dbl(&b, 1)));
    let (cx, cy) = (x(dbl(&c, 0)), x(dbl(&c, 1)));
    let (dx, dy) = (x(dbl(&d, 0)), x(dbl(&d, 1)));
    let t1 = bx - ax;
    let t2 = by - ay;
    let t3 = cx - dx;
    let t4 = cy - dy;
    let t5 = ax - cx;
    let t6 = ay - cy;
    let l50 = x(t1.to_f64());
    let l50 = x((t6 * l50).to_f64());
    let l50 = x((l50 - t5 * t2).to_f64());
    let t7 = t3 * t2 - t4 * t1;
    if t7.abs().lt(x(1.0e-6)) {
        return 0;
    }
    m.set_f64(out_s, ((t4 * t5 - t3 * t6) / t7).to_f64());
    let t8 = l50 / t7;
    m.set_f64(out_t, t8.to_f64());
    let l60 = x((dx - cx).to_f64());
    let l70 = x((dy - cy).to_f64());
    let t9 = t8 * l60 + cx;
    let t10 = t8 * l70 + cy;
    m.set_f64(out_pt, t9.to_f64());
    m.set_f64(out_pt + 8, t10.to_f64());
    1
}

/// [soft 0x08510c08 initInterSeg]: `(p1, p2, p3, p4, inter, srf)` with four output points (3 doubles each, z words zeroed):
/// p1-p2 is the border segment of the surface's (u0, u1, v0, v1) (+0x10 / +0x18 / +0x34 / +0x3c) the intersection's flags (+0x50)
/// name (bit 0 or 4 the first side, ..., left unwritten when no listed bit is set), p3-p4 the segment from the
/// intersection to its neighbour (the previous, else the next, link), or, with a wrap bit in the second flag byte (bits 10 / 11),
/// to the next link shifted by the whole number of spans of the surface (the difference truncated to an int in the register).
pub fn init_inter_seg(m: &mut Mem, p1: u32, p2: u32, p3: u32, p4: u32, inter: u32, srf: u32) {
    let mv = |m: &mut Mem, dst: u32, src: u32| {
        let v = si_x87::fmov64(m.f64(src));
        m.set_f64(dst, v);
    };
    let zero = |m: &mut Mem, p: u32| {
        m.set_u32(p + 0x10, 0);
        m.set_u32(p + 0x14, 0);
    };
    let f = m.u32(inter + 0x50);
    let (u0, u1, v0, v1) = (srf + 0x10, srf + 0x18, srf + 0x34, srf + 0x3c);
    if f & 0x11 != 0 {
        mv(m, p1, u0);
        mv(m, p1 + 8, v0);
        zero(m, p1);
        mv(m, p2, u0);
        mv(m, p2 + 8, v1);
        zero(m, p2);
    } else if f & 0x44 != 0 {
        mv(m, p1, u0);
        mv(m, p1 + 8, v0);
        zero(m, p1);
        mv(m, p2, u1);
        mv(m, p2 + 8, v0);
        zero(m, p2);
    } else if f & 0x22 != 0 {
        mv(m, p1, u1);
        mv(m, p1 + 8, v0);
        zero(m, p1);
        mv(m, p2, u1);
        mv(m, p2 + 8, v1);
        zero(m, p2);
    } else if f & 0x88 != 0 {
        mv(m, p1, u0);
        mv(m, p1 + 8, v1);
        zero(m, p1);
        mv(m, p2, u1);
        mv(m, p2 + 8, v1);
        zero(m, p2);
    }
    if (f >> 8) & 0xc == 0 {
        let prev = m.u32(inter + 0x70);
        let nb = if prev == 0 { m.u32(inter + 0x74) } else { prev };
        mv(m, p3, nb + 0x30);
        mv(m, p3 + 8, nb + 0x38);
        zero(m, p3);
        mv(m, p4, inter + 0x30);
        mv(m, p4 + 8, inter + 0x38);
        zero(m, p4);
        return;
    }
    let du = x(m.f64(u1)) - x(m.f64(u0));
    let dv = x(m.f64(v1)) - x(m.f64(v0));
    mv(m, p3, inter + 0x30);
    mv(m, p3 + 8, inter + 0x38);
    zero(m, p3);
    let nx = m.u32(inter + 0x74);
    mv(m, p4, nx + 0x30);
    mv(m, p4 + 8, nx + 0x38);
    let f = m.u32(inter + 0x50);
    if f & 0x10 != 0 {
        let k = F80::from_i32(du.to_i32(Round::Zero));
        let nx = m.u32(inter + 0x74);
        let v = x(m.f64(nx + 0x30)) - k;
        m.set_f64(p4, v.to_f64());
    } else if f & 0x20 != 0 {
        let k = F80::from_i32(du.to_i32(Round::Zero));
        let nx = m.u32(inter + 0x74);
        let v = k + x(m.f64(nx + 0x30));
        m.set_f64(p4, v.to_f64());
    }
    let f = m.u32(inter + 0x50);
    let y = if f & 0x40 != 0 {
        let k = F80::from_i32(dv.to_i32(Round::Zero));
        let nx = m.u32(inter + 0x74);
        Some(x(m.f64(nx + 0x38)) - k)
    } else if f & 0x80 != 0 {
        let k = F80::from_i32(dv.to_i32(Round::Zero));
        let nx = m.u32(inter + 0x74);
        Some(k + x(m.f64(nx + 0x38)))
    } else {
        None
    };
    if let Some(y) = y {
        m.set_f64(p4 + 8, y.to_f64());
    }
    zero(m, p4);
}

fn words6(m: &Mem, at: u32) -> [u32; 6] {
    let mut w = [0u32; 6];
    for (i, v) in w.iter_mut().enumerate() {
        *v = m.u32(at + 4 * i as u32);
    }
    w
}

/// [soft 0x085104b4 si_nurbsTrimOrientOrientList]: `(trim)`: every orientation of the trim's list (+0x14) is oriented
/// (`si_nurbsCurveOrientOrient(orient, trim)`, outside); one that answers 0 gets bit 0 of its byte at +0x24 set. Answers
/// the list (0 for a null trim).
pub fn orient_orient_list(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    let list = m.u32(t + 0x14);
    let f = m.local(4);
    let mut e = genlist::iter_init(m, list, f, f + 2);
    while m.i16(f) != 1 {
        let d = genlist::elem_get_data(m, e);
        if o.call(m, ORIENT_ORIENT, &[d, t]).eax == 0 {
            let b = m.u8(d + 0x24);
            m.set_u8(d + 0x24, b | 1);
        }
        e = genlist::iter_next(m, list, f, f + 2);
    }
    m.release(4);
    list
}

/// [soft 0x0850ff88 si_nurbsTrimConnectRayInter]: `(trim)`: walks the ray list (+0x10, first made to start at a ray
/// without intersections) in neighbouring pairs, showing activity for each; a pair with the same number of intersections is
/// joined by `si_nurbsRayConnectNoTrans(ray, next, flag, trim->curve copy)`, any other by `si_nurbsRayConnectTrans` (both
/// outside; the second may move the walk through the locals handed to it); a 16-bit flag (1 to begin with) carries their
/// answers. The walk ends when the iteration comes round to the start, unless the trim's curve has the 16-bit field at +0x20
/// set, which allows one more pair (the `done` local becomes 1); a nonzero `done` set by the connection ends it too.
/// Answers the list (0 for a null trim).
pub fn connect_ray_inter(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    let list = m.u32(t + 0x10);
    start_at_zero(m, list);
    // frame: the next element (+0), the iteration flags (+4, +6), `done` (+8)
    let fr = m.local(12);
    m.set_i16(fr + 8, 0);
    let mut flag: i16 = 1;
    let mut cur = genlist::iter_init(m, list, fr + 4, fr + 6);
    let nx = genlist::iter_next(m, list, fr + 4, fr + 6);
    m.set_u32(fr, nx);
    while m.u32(fr) != 0 {
        o.call(m, SHOW_ACTIVITY, &[]);
        let r1 = genlist::elem_get_data(m, cur);
        let nxt = m.u32(fr);
        let r2 = genlist::elem_get_data(m, nxt);
        let (n1, n2) = (crate::inter::ray_get_no_inter(m, r1), crate::inter::ray_get_no_inter(m, r2));
        if n1 == n2 {
            let c = m.u32(t + 0xc);
            flag = o.call(m, RAY_CONNECT_NO_TRANS, &[r1, r2, flag as i32 as u32, c]).eax as i16;
        } else {
            let args = [t, cur, fr, r1, r2, list, fr + 4, fr + 6, fr + 8, flag as i32 as u32];
            flag = o.call(m, RAY_CONNECT_TRANS, &args).eax as i16;
        }
        cur = m.u32(fr);
        if m.i16(fr + 8) != 0 {
            m.set_u32(fr, 0);
        } else {
            let nx = genlist::iter_next(m, list, fr + 4, fr + 6);
            m.set_u32(fr, nx);
            if m.i16(fr + 4) == 1 {
                let c = m.u32(t + 8);
                if m.i16(c + 0x20) != 0 {
                    m.set_i16(fr + 8, 1);
                } else {
                    m.set_u32(fr, 0);
                }
            }
        }
    }
    m.release(12);
    list
}

/// [soft 0x08511100 parallelRayPerturb]: `(ray, trim, t, status)`: a message (`SI_statusDisplayMessage`, level 2), the ray
/// destroyed, `t` moved by 1e-4 and, when that is not below the curve copy's end parameter (+0x18 of the curve at trim+0xc),
/// reduced by it; the curve's point at `t` (`SI_nurbsCrvValue`, outside) is the new end of a ray cast the way
/// `si_nurbsRayInitRayCast` sets it up from the trim's centre and direction; that ray is traced (`*status` takes the answer,
/// 0 when no ray could be made). Answers the new ray.
pub fn parallel_ray_perturb(m: &mut Mem, o: &mut dyn Outside, ray: u32, t: u32, tv: f64, status: u32) -> u32 {
    let sp = m.sp();
    let cell = m.local(4);
    m.set_u32(cell, ray);
    o.call(m, STATUS_MESSAGE, &[0x086d_bb60, 2]);
    crate::inter::ray_destroy(m, o, cell);
    let crv = m.u32(t + 0xc);
    let t1 = x(tv) + x(f64::from_bits(0x3f1a_36e2_eb1c_432d));
    let c = x(m.f64(crv + 0x18));
    let tn = if t1.lt(c) { t1.to_f64() } else { (t1 - c).to_f64() };
    let val = m.local(24);
    let bits = tn.to_bits();
    o.call(m, CRV_VALUE, &[val, crv, bits as u32, (bits >> 32) as u32, 0]);
    let (b54, b58, b5c) = (m.local(24), m.local(24), m.local(24));
    let p3 = words6(m, val);
    crate::inter::ray_init_ray_cast(m, words6(m, t + 0x44), words6(m, t + 0x5c), p3, m.u32(t + 0x3c) as i32, b54, b58, b5c);
    let r = crate::inter::ray_create_p(m, words6(m, b5c), words6(m, b54), tn, words6(m, b58));
    let out = if r == 0 {
        m.set_u32(status, 0);
        0
    } else {
        let (srf, slot) = (m.u32(t), m.u32(t + 4));
        let a = o.call(m, RAY_TRACE_SRF2, &[srf, r, slot, t + 0x78]).eax;
        m.set_u32(status, a);
        r
    };
    m.set_sp(sp);
    out
}

/// The arguments of [`ray_cast`] after the trim.
pub struct RayCastArgs {
    pub origin: [u32; 6],
    pub dir: [u32; 6],
    pub t: f64,
    pub end: [u32; 6],
    pub status: u32,
    pub perturb: u16,
}

/// [soft 0x08510538 si_nurbsTrimRayCast]: `(trim, direction, origin, t, end, *status, perturb)` (the original takes the direction first; the ray gets origin and direction from the second and first group): a ray made from the values
/// (`si_nurbsRayCreateP`), traced against the trim's surface (`si_nurbsRayTraceSrf2(surface, ray, slot (+4), matrix +0x78)`,
/// outside), `*status` set to the answer (0 when no ray could be made). An answer of 5 (parallel) with a nonzero 16-bit
/// `perturb` goes through [`parallel_ray_perturb`] (which replaces the ray). The ray is kept when the status is 1, else
/// destroyed; answers it or 0.
pub fn ray_cast(m: &mut Mem, o: &mut dyn Outside, t: u32, a: &RayCastArgs) -> u32 {
    let sp = m.sp();
    let cell = m.local(4);
    let mut ray = crate::inter::ray_create_p(m, a.origin, a.dir, a.t, a.end);
    m.set_u32(cell, ray);
    let out;
    if ray == 0 {
        m.set_u32(a.status, 0);
        out = 0;
    } else {
        let (srf, slot) = (m.u32(t), m.u32(t + 4));
        let r = o.call(m, RAY_TRACE_SRF2, &[srf, ray, slot, t + 0x78]).eax;
        m.set_u32(a.status, r);
        if r == 5 && a.perturb != 0 {
            ray = parallel_ray_perturb(m, o, ray, t, a.t, a.status);
            m.set_u32(cell, ray);
        }
        if m.u32(a.status) == 1 {
            out = ray;
        } else {
            crate::inter::ray_destroy(m, o, cell);
            out = 0;
        }
    }
    m.set_sp(sp);
    out
}

pub const COMP_BUILD: Routine = Routine::new("si_nurbsCompositeCurveBuild", 0x0850_3978);
pub const CREATE_SRF_COMPS: Routine = Routine::new("si_nurbsCompCurveCreateSrfComps", 0x0850_4130);
pub const CREATE_SRF_COMPS_PROJ: Routine = Routine::new("si_nurbsCompCurveCreateSrfCompsProj", 0x0850_43c8);
pub const SRF_TRM_DISPOSE: Routine = Routine::new("SI_nurbsSrfTrmDispose", 0x084d_b944);
pub const SRF_PROJ_DISPOSE: Routine = Routine::new("SI_nurbsSrfProjDispose", 0x084d_b9d0);
pub const NEW_INTER_LIST: Routine = Routine::new("si_nurbsCurveInterBuildNewInterList", 0x0850_79f0);
pub const OLD_INTER_LIST: Routine = Routine::new("si_nurbsCurveInterBuildOldInterList", 0x0850_82c8);
pub const INTERSECT_LISTS: Routine = Routine::new("si_nurbsCurveInterIntersectLists", 0x0850_9de0);
pub const ORIENT_CREATE_P: Routine = Routine::new("si_nurbsCurveOrientCreateP", 0x0850_a330);

/// Three doubles at `at` as the six words a copy through the x87 stack gives (a signalling NaN comes out quiet).
fn copy3(m: &Mem, at: u32) -> [u32; 6] {
    let mut w = [0u32; 6];
    for k in 0..3u32 {
        let b = si_x87::fmov64(m.f64(at + 8 * k)).to_bits();
        w[2 * k as usize] = b as u32;
        w[2 * k as usize + 1] = (b >> 32) as u32;
    }
    w
}

/// The words of the bounding box the original hands to the curve routines: the surface's U start (+0x10), V start (+0x34),
/// U end (+0x18) and V end (+0x3c) in a 48-byte block (two doubles of it never written).
fn srf_box(m: &Mem, srf: u32) -> [u32; 12] {
    let mut w = [0u32; 12];
    for (i, off) in [(0usize, 0x10u32), (2, 0x34), (6, 0x18), (8, 0x3c)] {
        let b = si_x87::fmov64(m.f64(srf + off)).to_bits();
        w[i] = b as u32;
        w[i + 1] = (b >> 32) as u32;
    }
    w
}

/// [soft 0x08510e4c si_nurbsTrimBuildIntersectionLists]: `(trim)` (a null trim is not looked at before the surface is read):
/// builds the new intersection list at +0x18 from the curve list (+0x20) and the copied curve's two header words (+0x00,
/// +0x34) with the surface box (`si_nurbsCurveInterBuildNewInterList`, outside; with the +0x2c list as extra output when the
/// trim has a trimming type and the surface's +0x60 count is positive, the +0x30 list for the old list), then the old list
/// at +0x1c (`...BuildOldInterList(surface, &+0x1c, +0x30 or 0)`); with an +0x2c list of positive count (+0x10) the two
/// are intersected (`...IntersectLists(+0x2c, +0x30, 1)`), made into composite curves in a list of their own
/// (`si_nurbsCompositeCurveBuild`, outside), which then adds to the +0x18 list, and is destroyed.
pub fn build_intersection_lists(m: &mut Mem, o: &mut dyn Outside, t: u32) {
    let sp = m.sp();
    let fr = m.local(0x3c);
    let srf = m.u32(t);
    let bx = srf_box(m, srf);
    for (i, w) in bx.iter().enumerate() {
        m.set_u32(fr + 4 * i as u32, if matches!(i, 4 | 5 | 10 | 11) { m.u32(fr + 4 * i as u32) } else { *w });
    }
    m.set_u32(fr + 0x38, 0);
    m.set_u32(fr + 0x34, 0);
    let mut first_out = 0u32;
    if t != 0 {
        if m.u32(t + 0x38) != 0 && m.i32(srf + 0x60) > 0 {
            first_out = t + 0x2c;
            m.set_u32(fr + 0x38, t + 0x30);
        }
        let crv = m.u32(t + 0xc);
        let (h0, h1) = (m.u32(crv), m.u32(crv + 0x34));
        let l20 = m.u32(t + 0x20);
        o.call(m, NEW_INTER_LIST, &[l20, h0, h1, t + 0x18, first_out, fr]);
        let (s, l1c, x) = (m.u32(t), t + 0x1c, m.u32(fr + 0x38));
        o.call(m, OLD_INTER_LIST, &[s, l1c, x]);
        let l = m.u32(t + 0x2c);
        if l != 0 && m.i32(l + 0x10) > 0 {
            let nl = genlist::create(m);
            m.set_u32(fr + 0x34, nl);
            if nl != 0 {
                let l30 = m.u32(t + 0x30);
                o.call(m, INTERSECT_LISTS, &[l, l30, 1]);
                let a = o.call(m, COMP_BUILD, &[m.u32(t + 0x2c), 0, 0, nl]).eax as u16;
                if a != 0 {
                    let b = o.call(m, COMP_BUILD, &[m.u32(t + 0x30), 0, 0, nl]).eax as u16;
                    if b != 0 {
                        let crv = m.u32(t + 0xc);
                        let (h0, h1) = (m.u32(crv), m.u32(crv + 0x34));
                        o.call(m, NEW_INTER_LIST, &[nl, h0, h1, t + 0x18, 0, fr]);
                        genlist::destroy(m, o, fr + 0x34, CRV_DISPOSE.addr);
                    }
                }
            }
        }
    }
    m.set_sp(sp);
}

/// [soft 0x08510f6c si_nurbsTrimBuildCompositeCurves]: `(trim)`: two new lists at +0x24 and +0x28 (0 when either cannot be
/// made); the +0x1c and +0x18 intersection lists are made into composite curves in them (`si_nurbsCompositeCurveBuild`,
/// outside; either answering 0 destroys a list of its own (an empty local one) and gives 0); then
/// `si_nurbsCompCurveCreateSrfComps(+0x24, +0x28, &local, trim+0x38, box, surface+0x64, trim)` (outside) fills the surface:
/// its trims disposed of (`SI_nurbsSrfTrmDispose`), +0x5c cleared, bits 0, 5 and 6 of the byte at +0x5b set, +0x60 the
/// answer and +0x64 the local; answers 1.
pub fn build_composite_curves(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    let sp = m.sp();
    let fr = m.local(8);
    m.set_u32(fr, 0);
    m.set_u32(fr + 4, 0);
    let l24 = genlist::create(m);
    m.set_u32(t + 0x24, l24);
    let mut out = 0;
    if l24 != 0 {
        let l28 = genlist::create(m);
        m.set_u32(t + 0x28, l28);
        if l28 != 0 {
            let srf = m.u32(t);
            let bx = srf_box(m, srf);
            let a = o.call(m, COMP_BUILD, &[m.u32(t + 0x1c), m.u32(t + 0x24), l28, 0]).eax as u16;
            let mut ok = false;
            if a != 0 {
                let b = o.call(m, COMP_BUILD, &[m.u32(t + 0x18), m.u32(t + 0x24), m.u32(t + 0x28), 0]).eax as u16;
                ok = b != 0;
            }
            if ok {
                let srf = m.u32(t);
                let mut args = vec![m.u32(t + 0x24), m.u32(t + 0x28), fr, m.u32(t + 0x38)];
                args.extend_from_slice(&bx);
                args.push(m.u32(srf + 0x64));
                args.push(t);
                let v = o.call(m, CREATE_SRF_COMPS, &args).eax;
                o.call(m, SRF_TRM_DISPOSE, &[t]);
                let srf = m.u32(t);
                m.set_u16(srf + 0x5c, 0);
                for bit in [1u8, 0x20, 0x40] {
                    let b = m.u8(srf + 0x5b);
                    m.set_u8(srf + 0x5b, b | bit);
                }
                m.set_u32(srf + 0x60, v);
                m.set_u32(srf + 0x64, m.u32(fr));
                out = 1;
            } else {
                genlist::destroy(m, o, fr + 4, CRV_DISPOSE.addr);
            }
        }
    }
    m.set_sp(sp);
    out
}

/// [soft 0x08511094 si_nurbsTrimBuildCompositeCurvesProj]: `(trim)`: `si_nurbsCompCurveCreateSrfCompsProj(&local, surface+0x6c,
/// trim)` (outside), the surface's projections disposed of (`SI_nurbsSrfProjDispose`), bit 5 of its byte at +0x5b set, +0x68 the
/// answer and +0x6c the local; answers 1.
pub fn build_composite_curves_proj(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    let sp = m.sp();
    let fr = m.local(4);
    m.set_u32(fr, 0);
    let srf = m.u32(t);
    let v = o.call(m, CREATE_SRF_COMPS_PROJ, &[fr, m.u32(srf + 0x6c), t]).eax;
    o.call(m, SRF_PROJ_DISPOSE, &[t]);
    let srf = m.u32(t);
    let b = m.u8(srf + 0x5b);
    m.set_u8(srf + 0x5b, b | 0x20);
    m.set_u32(srf + 0x68, v);
    m.set_u32(srf + 0x6c, m.u32(fr));
    m.set_sp(sp);
    1
}

/// [soft 0x085100f0 si_nurbsTrimBuildOrientList]: `(rays)`: a list of orientations (one per chain of linked intersections)
/// for a ray list: for each intersection of each ray whose in-curve mark (`si_nurbsInterGetInCurve`) is 0, the chain of
/// `next` links (+0x74) from it is walked, marking each 1 and counting, until it comes back to the start (closed, flag 1),
/// ends (open, flag 0) or meets a mark 1 (a loop); an open chain is then extended backwards along `prev` (+0x70) from the
/// start, marking and counting, a mark 1 there being a loop too. A loop destroys the whole list and shows "loop detected"
/// at level 3 (answer 0). An orientation is made over the ends (`si_nurbsCurveOrientCreateP(first, last, closed, count)`,
/// outside); one with a count (+8) of 2 or less is dropped with a message at level 2 (`si_nurbsCurveOrientDestroy`, outside),
/// any other is added to the list. Answers the list (0 for a null ray list or when it could not be made).
pub fn build_orient_list(m: &mut Mem, o: &mut dyn Outside, rays: u32) -> u32 {
    use crate::inter::{inter_get_in_curve as gic, inter_set_in_curve as sic, ray_get_inters};
    if rays == 0 {
        return 0;
    }
    let sp = m.sp();
    // list +0, orientation +4, outer flags +8 / +10, inner flags +12 / +14
    let fr = m.local(0x10);
    let list = genlist::create(m);
    m.set_u32(fr, list);
    if list == 0 {
        m.set_sp(sp);
        return 0;
    }
    let mut bad = false;
    let mut e = genlist::iter_init(m, rays, fr + 8, fr + 10);
    if m.i16(fr + 8) != 1 {
        'outer: loop {
            let ray = genlist::elem_get_data(m, e);
            let inters = ray_get_inters(m, ray);
            let mut ie = genlist::iter_init(m, inters, fr + 12, fr + 14);
            while m.i16(fr + 12) != 1 {
                let inter = genlist::elem_get_data(m, ie);
                if gic(m, inter) as i16 == 0 {
                    let mut count: i32 = 0;
                    let mut last = inter;
                    let mut start = inter;
                    let mut nx = inter;
                    let mut found = false;
                    let mut ended = false;
                    loop {
                        if nx == 0 {
                            ended = true;
                            break;
                        }
                        last = nx;
                        nx = m.u32(nx + 0x74);
                        if gic(m, last) as i16 == 1 {
                            found = true;
                            break;
                        }
                        sic(m, last, 1);
                        count += 1;
                        if nx == inter {
                            break;
                        }
                    }
                    let closed: i16 = if !ended || found {
                        1
                    } else {
                        let mut b = if inter != 0 { m.u32(inter + 0x70) } else { 0 };
                        while b != 0 {
                            start = b;
                            if gic(m, start) as i16 == 1 {
                                bad = true;
                                break 'outer;
                            }
                            sic(m, start, 1);
                            count += 1;
                            b = m.u32(start + 0x70);
                        }
                        0
                    };
                    if found {
                        bad = true;
                        break 'outer;
                    }
                    let orient = o.call(m, ORIENT_CREATE_P, &[start, last, closed as i32 as u32, count as u32]).eax;
                    m.set_u32(fr + 4, orient);
                    if m.i32(orient + 8) > 2 {
                        genlist::insert_e(m, list, orient);
                    } else {
                        o.call(m, STATUS_MESSAGE, &[0x086d_baa0, 2]);
                        o.call(m, ORIENT_DESTROY, &[fr + 4]);
                    }
                }
                ie = genlist::iter_next(m, inters, fr + 12, fr + 14);
            }
            e = genlist::iter_next(m, rays, fr + 8, fr + 10);
            if m.i16(fr + 8) == 1 {
                break;
            }
        }
    }
    if bad {
        genlist::destroy(m, o, fr, ORIENT_DESTROY.addr);
        o.call(m, STATUS_MESSAGE, &[0x086d_bae0, 3]);
    }
    let r = m.u32(fr);
    m.set_sp(sp);
    r
}

/// [soft 0x0850fb60 si_nurbsTrimPreview]: `(trim)`: rebuilds the ray list (+0x10) and the orientation list (+0x14): both
/// destroyed, a new ray list made (0 when that fails). For each sample of the flattened curve (+0x74: count at +4, 36-byte
/// points at the array +0x2c, parameters at the array +0xc): activity shown, a ray cast setup from the point
/// (`si_nurbsRayInitRayCast` with the trim's centre, direction and projection type), the ray cast ([`ray_cast`], perturbing
/// allowed). A ray with status 1 is added to the list, or, when the previous ray's count (+0x50) is above that of the one
/// before it and of the new one (a "peak"), replaces the previous element's ray; the ray before gets the sample's previous
/// record's word at +0x20 in its +0x58; a ray without intersections sets bit 0 of the list's flags. Any other status shows
/// "Error projecting Ray" at level 3. Last, the final ray gets its +0x58, `si_nurbsTrimConnectRayInter` runs and the orientation
/// list ([`build_orient_list`] over the rays) is kept at +0x14 and in the surface's +0xd0. Answers 1 when it got one.
pub fn preview(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    genlist::destroy(m, o, t + 0x10, RAY_DESTROY.addr);
    genlist::destroy(m, o, t + 0x14, ORIENT_DESTROY.addr);
    let list = genlist::create(m);
    m.set_u32(t + 0x10, list);
    if list == 0 {
        return 0;
    }
    let sp = m.sp();
    let st = m.local(4);
    let (b54, b58, b5c) = (m.local(24), m.local(24), m.local(24));
    let (mut prev, mut prev2, mut elem) = (0u32, 0u32, 0u32);
    let mut i: i32 = 0;
    let mut off: u32 = 0;
    let flat = m.u32(t + 0x74);
    if m.i32(flat + 4) > 0 {
        loop {
            o.call(m, SHOW_ACTIVITY, &[]);
            let flat = m.u32(t + 0x74);
            let pts = m.u32(flat + 0x2c);
            let p3 = copy3(m, pts.wrapping_add(off));
            let typ = m.u32(t + 0x3c) as i32;
            crate::inter::ray_init_ray_cast(m, words6(m, t + 0x44), words6(m, t + 0x5c), p3, typ, b54, b58, b5c);
            let tv = si_x87::fmov64(m.f64(m.u32(flat + 0xc).wrapping_add(8 * i as u32)));
            let a = RayCastArgs { origin: words6(m, b5c), dir: words6(m, b54), t: tv, end: words6(m, b58), status: st, perturb: 1 };
            let ray = ray_cast(m, o, t, &a);
            if m.i32(st) == 1 {
                let peak = !(prev2 == 0 || prev == 0 || m.i32(prev + 0x50) <= m.i32(prev2 + 0x50) || m.i32(prev + 0x50) <= m.i32(ray + 0x50));
                if !peak {
                    elem = genlist::insert_e(m, m.u32(t + 0x10), ray);
                    prev2 = prev;
                } else {
                    genlist::elem_replace_data(m, o, elem, RAY_DESTROY.addr, ray);
                }
                prev = ray;
                if prev2 != 0 {
                    let w = m.u32(m.u32(m.u32(t + 0x74) + 0x2c).wrapping_add(off).wrapping_sub(4));
                    m.set_u32(prev2 + 0x58, w);
                }
                if crate::inter::ray_get_no_inter(m, ray) == 0 {
                    let l = m.u32(t + 0x10);
                    let f = genlist::get_flags(m, l);
                    genlist::set_flags(m, l, f | 1);
                }
            } else {
                o.call(m, STATUS_MESSAGE, &[0x086d_ba80, 3]);
            }
            off = off.wrapping_add(0x24);
            i += 1;
            let flat = m.u32(t + 0x74);
            if i >= m.i32(flat + 4) {
                break;
            }
        }
    }
    if prev != 0 {
        let flat = m.u32(t + 0x74);
        let w = m.u32(m.u32(flat + 0x2c).wrapping_add((i as u32).wrapping_mul(0x24)).wrapping_sub(4));
        m.set_u32(prev + 0x58, w);
    }
    let mut out = 0;
    if connect_ray_inter(m, o, t) != 0 {
        let rl = get_ray_list(m, t);
        let ol = build_orient_list(m, o, rl);
        m.set_u32(t + 0x14, ol);
        let srf = m.u32(t);
        m.set_u32(srf + 0xd0, ol);
        if ol != 0 {
            out = 1;
        }
    }
    m.set_sp(sp);
    out
}

pub const BUILD_LIST_FROM_PREVIEW: Routine = Routine::new("si_nurbsTrimBuildListFromPreview", 0x0851_0608);
pub const TRAVERSE_NESTING: Routine = Routine::new("si_nurbsOrientTraverseNestingTree", 0x0850_c930);

/// [soft 0x0850fecc si_nurbsTrimProject]: `(trim)`: 0 for a null trim; the preview is made (`Preview`) when the trim has no
/// orientation list yet, the list is built from it (`si_nurbsTrimBuildListFromPreview`, outside) and, when that answers
/// nonzero, the projected composite curves are built (16-bit answer of `BuildCompositeCurvesProj`); answers 1 when both
/// succeeded, else 0.
pub fn project(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    if m.u32(t + 0x14) == 0 {
        preview(m, o, t);
    }
    if o.call(m, BUILD_LIST_FROM_PREVIEW, &[t]).eax == 0 {
        return 0;
    }
    if build_composite_curves_proj(m, o, t) as u16 == 0 {
        return 0;
    }
    1
}

/// [soft 0x0850fe04 SI_nurbsTrimTrim]: `(trim)`: 0 for a null trim. With the byte-sized field at +0xfc zero the answer is
/// that of `Project` (16-bit, sign-extended). Otherwise: the preview is made when there is no orientation list, activity is
/// shown, the orientations oriented (`OrientOrientList`), activity shown, the list built from the preview (outside, 0
/// stops with 0), the nesting tree walked from the first element of the tree at +0x34 with 0 (`si_nurbsOrientTraverseNestingTree`,
/// outside), activity shown, `BuildIntersectionLists`, activity shown, `si_nurbsCurveInterIntersectLists(+0x18, +0x1c, +0x38)`
/// (outside), activity shown and the composite curves built; answers 1 when that answered nonzero (16-bit), else 0.
pub fn trim(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    if t == 0 {
        return 0;
    }
    if m.u32(t + 0xfc) == 0 {
        return project(m, o, t) as u16 as i16 as i32 as u32;
    }
    if m.u32(t + 0x14) == 0 {
        preview(m, o, t);
    }
    o.call(m, SHOW_ACTIVITY, &[]);
    orient_orient_list(m, o, t);
    o.call(m, SHOW_ACTIVITY, &[]);
    if o.call(m, BUILD_LIST_FROM_PREVIEW, &[t]).eax == 0 {
        return 0;
    }
    let tree = m.u32(t + 0x34);
    let first = m.u32(tree);
    o.call(m, TRAVERSE_NESTING, &[first, 0]);
    o.call(m, SHOW_ACTIVITY, &[]);
    build_intersection_lists(m, o, t);
    o.call(m, SHOW_ACTIVITY, &[]);
    o.call(m, INTERSECT_LISTS, &[m.u32(t + 0x18), m.u32(t + 0x1c), m.u32(t + 0x38)]);
    o.call(m, SHOW_ACTIVITY, &[]);
    if build_composite_curves(m, o, t) as u16 == 0 {
        return 0;
    }
    1
}

/// [soft 0x08510808 si_nurbsTrimMoveInterToBorder]: `(out, trim, inter)`: three doubles are stored at `out` (the first two
/// the surface parameters of the intersection moved onto the border its flags (+0x50) name; the third is never written and
/// holds whatever the caller's stack held: this port stores 0) and `out` is answered. Nothing is done (zeros) for a null trim
/// or intersection. The surface is the first word of the trim; its u0, u1, v0, v1 are at +0x10, +0x18, +0x34, +0x3c. With bit 0
/// (u0) or 1 (u1: 0x22) of the flag byte's low-side pair and bit 2 (v0: 0x44) or 3 (v1: 0x88) both named, the corner is
/// taken as is. With one side only named the parameter of the intersection (u at +0x30, v at +0x38) is kept when it lies
/// within 1e-6 (measured in the register) of that border and snapped to it, else the segment of the border and the one from the
/// intersection to its neighbour (`initInterSeg`) are intersected (`ComputeLineInter`, which writes u and v when the lines
/// cross) and the named sides then overwrite u and v.
pub fn move_inter_to_border(m: &mut Mem, out: u32, trim: u32, inter: u32) -> u32 {
    let sp = m.sp();
    let fr = m.local(24);
    m.set_f64(fr, 0.0);
    m.set_f64(fr + 8, 0.0);
    m.set_f64(fr + 16, 0.0);
    'done: {
        if inter == 0 || trim == 0 {
            break 'done;
        }
        let f = m.u32(inter + 0x50);
        let srf = m.u32(trim);
        let (u0, u1, v0, v1) = (srf + 0x10, srf + 0x18, srf + 0x34, srf + 0x3c);
        let side_v = |m: &Mem| if f & 0x44 != 0 { Some(v0) } else if (m.u8(inter + 0x50) & 0x88) != 0 { Some(v1) } else { None };
        if f & 0x11 != 0 {
            if let Some(v) = side_v(m) {
                let a = m.f64(u0);
                m.set_f64(fr, a);
                let b = m.f64(v);
                m.set_f64(fr + 8, b);
                break 'done;
            }
        }
        if f & 0x22 != 0 {
            if let Some(v) = side_v(m) {
                let a = m.f64(u1);
                m.set_f64(fr, a);
                let b = m.f64(v);
                m.set_f64(fr + 8, b);
                break 'done;
            }
        }
        let near = |a: f64, b: f64| (x(a) - x(b)).abs().lt(x(1.0e-6));
        let (iu, iv) = (m.f64(inter + 0x30), m.f64(inter + 0x38));
        if f & 0x11 != 0 {
            if near(iu, m.f64(u0)) {
                let a = m.f64(u0);
                m.set_f64(fr, a);
                m.set_f64(fr + 8, iv);
                break 'done;
            }
        } else if f & 0x44 != 0 {
            if near(iv, m.f64(v0)) {
                let b = m.f64(v0);
                m.set_f64(fr + 8, b);
                m.set_f64(fr, iu);
                break 'done;
            }
        } else if f & 0x22 != 0 {
            if near(iu, m.f64(u1)) {
                let a = m.f64(u1);
                m.set_f64(fr, a);
                m.set_f64(fr + 8, iv);
                break 'done;
            }
        } else if f & 0x88 != 0 && near(iv, m.f64(v1)) {
            let b = m.f64(v1);
            m.set_f64(fr + 8, b);
            m.set_f64(fr, iu);
            break 'done;
        }
        let ps = m.local(96);
        let (p1, p2, p3, p4) = (ps, ps + 24, ps + 48, ps + 72);
        let (os, ot) = (m.local(8), m.local(8));
        init_inter_seg(m, p1, p2, p3, p4, inter, srf);
        let rd = |m: &Mem, p: u32| {
            let mut w = [0u32; 6];
            for (k, e) in w.iter_mut().enumerate() {
                *e = m.u32(p + 4 * k as u32);
            }
            w
        };
        let (a, b, c, d) = (rd(m, p1), rd(m, p2), rd(m, p3), rd(m, p4));
        compute_line_inter(m, a, b, c, d, fr, os, ot);
        let f = m.u32(inter + 0x50);
        if f & 0x11 != 0 {
            let a = m.f64(u0);
            m.set_f64(fr, a);
        } else if f & 0x22 != 0 {
            let a = m.f64(u1);
            m.set_f64(fr, a);
        }
        if m.u8(inter + 0x50) & 0x44 != 0 {
            let b = m.f64(v0);
            m.set_f64(fr + 8, b);
        } else if m.u8(inter + 0x50) & 0x88 != 0 {
            let b = m.f64(v1);
            m.set_f64(fr + 8, b);
        }
    }
    for k in 0..6 {
        let w = m.u32(fr + 4 * k);
        m.set_u32(out + 4 * k, w);
    }
    m.set_sp(sp);
    out
}
