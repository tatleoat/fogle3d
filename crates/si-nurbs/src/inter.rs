// SPDX-License-Identifier: MIT
//! `intersec.c` and `ray.c` (package 16, finding 0044 section "trim1"): the intersections of a ray
//! with a surface's trim curves and the rays themselves, on the memory image.
//!
//! Records (names are ours, read from what the routines do with them):
//! - an intersection, `SI_calloc(1, 0x78)`: +0 the point (3 doubles), +0x18 a direction (3 doubles),
//!   +0x30 u, +0x38 v, +0x40 the ray it lies on, +0x44 the ray parameter t, +0x4c a 16-bit "in curve"
//!   number, +0x50 flags (bits 1/2 u at the low/high edge, 4/8 v at the low/high edge, 0x10..0x80
//!   the same for the link, 0x100/0x200 crossing marks), +0x64 the distance to the linked
//!   intersection, +0x6c a number copied from the ray (+0x58), +0x70 / +0x74 the previous / next
//!   linked intersection;
//! - a ray, `SI_calloc(1, 0x60)`: +0 origin (3 doubles), +0x18 direction, +0x30 t, +0x38 the end
//!   point (3 doubles), +0x50 the number of intersections, +0x54 the list (generic list) of them,
//!   +0x58 and +0x5c two words the copy carries over.
//!
//! The generic list routines are `si-gen`'s (`genList.c`); that crate has no dependency on this one,
//! but adding it here would change the lockfile, so its source file is compiled into this module
//! (`genlist`). Lists the original hands to `SI_genListCopy` / `SI_genListDestroy` with this file's
//! callbacks reach them through the `Outside` (see [`route`]).

use si_mem::{Mem, Outside, Ret, Routine};
use si_x87::F80;

#[path = "../../si-gen/src/list.rs"]
#[allow(dead_code)]
pub(crate) mod genlist;

pub use genlist::{DATA, NEXT, PREV};

/// The entry points that other lists call back (addresses in the original).
pub const INTER_COPY: Routine = Routine::new("si_nurbsInterCopy", 0x0850_d448);
pub const INTER_COMP_T: Routine = Routine::new("si_nurbsInterCompT", 0x0850_d4a4);
pub const INTER_DESTROY: Routine = Routine::new("si_nurbsInterDestroy", 0x0850_cacc);

/// The generic lists call their callbacks by bare address; this gives the three of this file their names on the way
/// out, so that a check (or a run-time that routes by name) can recognise them.
struct Named<'a>(&'a mut dyn Outside);

impl Outside for Named<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        let r = match r.addr {
            0x0850_d448 => INTER_COPY,
            0x0850_d4a4 => INTER_COMP_T,
            0x0850_cacc => INTER_DESTROY,
            _ => r,
        };
        self.0.call(m, r, args)
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        self.0.print(stream, text)
    }
}

const SRF_EPS: f64 = 1e-4; // 0x08662040 / 0x08662048 / 0x08662058
const CLOSE2: f64 = 1e-8; // 0x08662050

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// The two flag words an iteration hands back (`first`, `last`), in a stack frame.
pub(crate) struct Fl(pub(crate) u32);
#[allow(dead_code)]
impl Fl {
    pub(crate) fn new(m: &mut Mem) -> Fl {
        Fl(m.local(4))
    }
    pub(crate) fn first(&self, m: &Mem) -> i16 {
        m.i16(self.0)
    }
    pub(crate) fn init(&self, m: &mut Mem, list: u32) -> u32 {
        genlist::iter_init(m, list, self.0, self.0 + 2)
    }
    pub(crate) fn next(&self, m: &mut Mem, list: u32) -> u32 {
        genlist::iter_next(m, list, self.0, self.0 + 2)
    }
    pub(crate) fn done(self, m: &mut Mem) {
        m.release(4);
    }
}

fn data(m: &Mem, e: u32) -> u32 {
    genlist::elem_get_data(m, e)
}

/// [soft 0x0850ca50 si_nurbsInterCreate]: an intersection from `SI_calloc(1, 0x78)` with +0x64 = -1.0
/// (when the allocation worked).
pub fn inter_create(m: &mut Mem) -> u32 {
    let p = m.si_calloc(1, 0x78);
    if p != 0 {
        m.set_f64(p + 0x64, -1.0);
    }
    p
}

/// [soft 0x0850ca74 si_nurbsInterCreateP]: `(u, v, point, direction, t, ray)`: an intersection (no -1.0
/// at +0x64: left 0) with those values; the point and direction are copied as words, u, v and t go
/// through the x87 unit.
pub fn inter_create_p(m: &mut Mem, u: f64, v: f64, pt: [u32; 6], dir: [u32; 6], t: f64, ray: u32) -> u32 {
    let p = m.si_calloc(1, 0x78);
    if p != 0 {
        m.set_f64(p + 0x30, si_x87::fmov64(u));
        m.set_f64(p + 0x38, si_x87::fmov64(v));
        for i in 0..6u32 {
            m.set_u32(p + 4 * i, pt[i as usize]);
            m.set_u32(p + 0x18 + 4 * i, dir[i as usize]);
        }
        m.set_f64(p + 0x44, si_x87::fmov64(t));
        m.set_u32(p + 0x40, ray);
    }
    p
}

/// [soft 0x0850cacc si_nurbsInterDestroy]: `(pp)`: frees `*pp` and clears it, when both are set.
pub fn inter_destroy(m: &mut Mem, pp: u32) {
    if pp != 0 {
        let p = m.u32(pp);
        if p != 0 {
            m.free(p);
            m.set_u32(pp, 0);
        }
    }
}

/// [soft 0x0850cc48 si_nurbsInterComputeRayT]: the ray parameter of the intersection: for each of
/// x, y, z whose ray direction component is longer than 1e-6, (point - origin) / direction; the
/// ones longer than 1e-6 are summed (80 bits) and, when the sum is longer than 1e-6, their mean is
/// stored at +0x44 (rounded). Answers the value at +0x44 (st0; 0 for a null intersection).
pub fn inter_compute_ray_t(m: &mut Mem, inter: u32) -> F80 {
    if inter == 0 {
        return x(0.0);
    }
    let r = m.u32(inter + 0x40);
    let eps = x(1e-6);
    let mut t = [x(0.0); 3];
    for k in 0..3u32 {
        let d = x(m.f64(r + 0x18 + 8 * k));
        if eps.lt(d.abs()) {
            t[k as usize] = (x(m.f64(inter + 8 * k)) - x(m.f64(r + 8 * k))) / d;
        }
    }
    let mut sum = x(0.0);
    let mut n = 0i32;
    for k in 0..3 {
        if eps.lt(t[k].abs()) {
            sum = if k == 2 { t[k] + sum } else { sum + t[k] };
            n += 1;
        }
    }
    if eps.lt(sum.abs()) {
        m.set_f64(inter + 0x44, (sum / F80::from_i32(n)).to_f64());
    }
    x(m.f64(inter + 0x44))
}

/// si_nurbsInterDist2D (0x0850d31c, answers in st0, not compared): routed (unit route3) to [`crate::dist::inter_dist_2d`].
pub fn inter_dist_2d(a: f64, b: f64, c: f64, d: f64) -> F80 {
    crate::dist::inter_dist_2d(a, b, c, d)
}

/// si_nurbsRayDist3D (0x0850dcd8, answers in st0): routed (unit route3) to [`crate::dist::ray_dist_3d`] (x and y first, z added
/// last), on the points at `a` and `b` in the image.
pub fn ray_dist_3d(m: &Mem, a: u32, b: u32) -> F80 {
    crate::dist::ray_dist_3d(
        [m.f64(a), m.f64(a + 8), m.f64(a + 16)],
        [m.f64(b), m.f64(b + 8), m.f64(b + 16)],
    )
}

/// [soft 0x0850cd44 si_nurbsInterFindClosest]: `(elem, ray1, ray2, pflag)`: among the intersections of
/// `ray2`'s list, walked in step with `ray1`'s list, the one whose point is nearest the point of
/// `elem`'s intersection (squared distance below the least so far, which starts at 3.4028234663852886e38);
/// answers the element of `ray2`'s list that was current at that step (0 when none), and sets `*pflag`
/// to the 0/1 toggle (flipped at every step) the step had. When nothing was nearer the original stores
/// an uninitialised stack word; the port stores 0 there (marked in the check as not compared).
pub fn inter_find_closest(m: &mut Mem, elem: u32, ray1: u32, ray2: u32, pflag: u32) -> u32 {
    if elem == 0 || ray1 == 0 || ray2 == 0 {
        return 0;
    }
    let l1 = ray_get_inters(m, ray1);
    let l2 = ray_get_inters(m, ray2);
    let me = data(m, elem);
    let mut best = x(f32::MAX as f64); // 3.4028234663852886e+38 at 0x08662030
    let mut found = 0u32;
    let mut flag = m.i16(pflag) as i32;
    let mut out_flag = 0i32;
    let (f1, f2) = (Fl::new(m), Fl::new(m));
    let mut e1 = f1.init(m, l1);
    let mut e2 = f2.init(m, l2);
    while f1.first(m) != 1 {
        let d1 = data(m, e1);
        let d = ray_dist_3d(m, me, d1);
        if d.lt(best) {
            best = x(d.to_f64());
            found = e2;
            out_flag = flag;
        }
        flag = (flag == 0) as i32;
        e1 = f1.next(m, l1);
        e2 = f2.next(m, l2);
    }
    f2.done(m);
    f1.done(m);
    m.set_i16(pflag, out_flag as i16);
    found
}

/// The part of the link routines that sets the distance: |t2 - t1| (80 bits); for a distance of 1e-6 or
/// more and t1 >= t2 the wrap-round distance (d5 - t1) + (t2 - d4) instead. Stored rounded.
fn link_distance(m: &mut Mem, from: u32, t1: F80, t2: F80, d4: f64, d5: f64) {
    let mut dist = (t2 - t1).abs();
    if !t1.lt(t2) && !dist.lt(x(1e-6)) {
        dist = (x(d5) - t1) + (t2 - x(d4));
    }
    m.set_f64(from + 0x64, dist.to_f64());
    let r = m.u32(from + 0x40);
    let w = m.u32(r + 0x58);
    m.set_u32(from + 0x6c, w);
}

/// [soft 0x0850cea0 si_nurbsInterLink]: `(e1, e2, dir, d4, d5)`: links the two intersections (null if
/// either element or datum is null): with `dir` set `e1.next = e2`, `e2.prev = e1`, else `e1.prev = e2`,
/// `e2.next = e1`; and sets the link distance and number on `e1` (dir set) or `e2` (see
/// [`link_distance`]). Answers `e2`.
pub fn inter_link(m: &mut Mem, e1: u32, e2: u32, dir: i16, d4: f64, d5: f64) -> u32 {
    if e1 == 0 || e2 == 0 {
        return 0;
    }
    let (i1, i2) = (data(m, e1), data(m, e2));
    if i1 == 0 || i2 == 0 {
        return 0;
    }
    let (r1, r2) = (m.u32(i1 + 0x40), m.u32(i2 + 0x40));
    if dir != 0 {
        m.set_u32(i1 + 0x74, i2);
        m.set_u32(i2 + 0x70, i1);
        let (t1, t2) = (x(m.f64(r1 + 0x30)), x(m.f64(r2 + 0x30)));
        link_distance(m, i1, t1, t2, d4, d5);
    } else {
        m.set_u32(i1 + 0x70, i2);
        m.set_u32(i2 + 0x74, i1);
        let (t1, t2) = (x(m.f64(r1 + 0x30)), x(m.f64(r2 + 0x30)));
        link_distance(m, i2, t1, t2, d4, d5);
    }
    e2
}

/// [soft 0x0850cfc4 si_nurbsInterUnconn]: `(elem, dir)`: 1 when the intersection has no link in that
/// direction (next for a set `dir`, previous otherwise), 0 otherwise or for a null element / datum.
pub fn inter_unconn(m: &mut Mem, elem: u32, dir: i16) -> i16 {
    if elem == 0 {
        return 0;
    }
    let i = data(m, elem);
    if i == 0 {
        return 0;
    }
    if dir != 0 {
        return (m.u32(i + 0x74) == 0) as i16;
    }
    (m.u32(i + 0x70) == 0) as i16
}

/// [soft 0x0850d018 si_nurbsInterUpdateNextCon]: `(elem, flag)`: for a non-null element, walks back along
/// the previous links from `elem.prev` until the element is its list's tail, turning `flag` over
/// (0 <-> 1) at every step; answers the flag.
pub fn inter_update_next_con(m: &Mem, elem: u32, mut flag: i16) -> i16 {
    if elem != 0 {
        let mut e = m.u32(elem + PREV);
        loop {
            let list = m.u32(e + 0xc);
            if e == m.u32(list + 4) {
                break;
            }
            e = m.u32(e + PREV);
            flag = (flag == 0) as i16;
        }
    }
    flag
}

/// [soft 0x0850d04c si_nurbsInterCrossBndry]: whether the intersection's flags (+0x50, byte at +0x51) show a
/// crossing (bits 0x100 / 0x200).
pub fn inter_cross_bndry(m: &Mem, elem: u32) -> i16 {
    let i = data(m, elem);
    (m.u8(i + 0x51) & 3 != 0) as i16
}

/// [soft 0x0850d2e8 si_nurbsInterGetInCurve]: the 16-bit number at +0x4c (0 for null).
pub fn inter_get_in_curve(m: &Mem, i: u32) -> i32 {
    if i == 0 {
        0
    } else {
        m.i16(i + 0x4c) as i32
    }
}

/// [soft 0x0850d300 si_nurbsInterSetInCurve]: sets the number at +0x4c; answers it (0 for null).
pub fn inter_set_in_curve(m: &mut Mem, i: u32, n: i16) -> i32 {
    if i == 0 {
        return 0;
    }
    m.set_i16(i + 0x4c, n);
    n as i32
}

/// [soft 0x0850d4a4 si_nurbsInterCompT]: 1 when `b.t < a.t` (both non-null), else 0: the order of the ray's list.
pub fn inter_comp_t(m: &Mem, a: u32, b: u32) -> u32 {
    if a != 0 && b != 0 {
        let (tb, ta) = (x(m.f64(b + 0x44)), x(m.f64(a + 0x44)));
        if tb.lt(ta) {
            return 1;
        }
    }
    0
}

/// [soft 0x0850d448 si_nurbsInterCopy]: a new intersection (by [`inter_create_p`]) with the same point, direction,
/// u, v and t, no ray and no flags; 0 for null.
pub fn inter_copy(m: &mut Mem, p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let mut pt = [0u32; 6];
    let mut dir = [0u32; 6];
    for i in 0..6u32 {
        pt[i as usize] = m.u32(p + 4 * i);
        dir[i as usize] = m.u32(p + 0x18 + 4 * i);
    }
    let (u, v, t) = (m.f64(p + 0x30), m.f64(p + 0x38), m.f64(p + 0x44));
    inter_create_p(m, u, v, pt, dir, t, 0)
}

/// [soft 0x0850d074 si_nurbsInterClassifyBndry]: `(ray, trim)`: for each intersection of the ray, with the
/// surface `*trim`'s range (u: +0x10..+0x18, v: +0x34..+0x3c; tolerance 1e-4): flags (+0x50) = 1 when u is below
/// the low end + tol, else 2 when above the high end - tol; plus 4 / 8 the same for v; with the surface's
/// u (+0x20) or v (+0x44) closing flag clear and an edge hit in that direction, 0x100 / 0x200 are added
/// and the answer is 1. Answers 0 otherwise.
pub fn inter_classify_bndry(m: &mut Mem, ray: u32, trim: u32) -> i16 {
    let mut out = 0i16;
    if ray == 0 || trim == 0 || m.u32(trim) == 0 {
        return 0;
    }
    let list = ray_get_inters(m, ray);
    let srf = m.u32(trim);
    let (lo_u, hi_u, lo_v, hi_v) = (m.f64(srf + 0x10), m.f64(srf + 0x18), m.f64(srf + 0x34), m.f64(srf + 0x3c));
    let eps = x(SRF_EPS);
    let fl = Fl::new(m);
    let mut e = fl.init(m, list);
    while fl.first(m) != 1 {
        let i = data(m, e);
        m.set_u32(i + 0x50, 0);
        let u = x(m.f64(i + 0x30));
        if u.lt(x(lo_u) + eps) {
            m.set_u32(i + 0x50, 1);
        } else if (x(hi_u) - eps).lt(u) {
            m.set_u32(i + 0x50, 2);
        }
        let v = x(m.f64(i + 0x38));
        if v.lt(x(lo_v) + eps) {
            let f = m.u8(i + 0x50) | 4;
            m.set_u8(i + 0x50, f);
        } else if (x(hi_v) - eps).lt(v) {
            let f = m.u8(i + 0x50) | 8;
            m.set_u8(i + 0x50, f);
        }
        if m.i16(srf + 0x20) == 0 {
            let f = m.u32(i + 0x50);
            if f & 1 != 0 || f & 2 != 0 {
                m.set_u32(i + 0x50, f | 0x100);
                out = 1;
            }
        }
        if m.i16(srf + 0x44) == 0 {
            let f = m.u32(i + 0x50);
            if f & 0xc != 0 {
                m.set_u32(i + 0x50, f | 0x200);
                out = 1;
            }
        }
        e = fl.next(m, list);
    }
    fl.done(m);
    out
}

/// The text `si_nurbsInterCopyFlags` prints when the two rays differ in their counts.
pub const COPY_FLAGS_MSG: u32 = 0x086d_b880;
/// The `stdout` variable of the C library as the program links it.
const STDOUT: u32 = 0x0878_0c84;
const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);

/// [soft 0x0850d1dc si_nurbsInterCopyFlags]: `(r1, r2)`: with both non-null and different, when they have the same
/// intersection count (+0x50), the flags (+0x50) of r1's intersections are copied to r2's in list order; else a
/// message goes to stdout (`fprintf` to `stdout`, through the Outside) and the answer is 0. Answers r2 otherwise (r2 itself when
/// either is null or they are the same ray).
pub fn inter_copy_flags(m: &mut Mem, o: &mut dyn Outside, r1: u32, r2: u32) -> u32 {
    let mut ret = r2;
    if r1 != 0 && r2 != 0 && r1 != r2 {
        if m.u32(r1 + 0x50) == m.u32(r2 + 0x50) {
            let (l1, l2) = (ray_get_inters(m, r1), ray_get_inters(m, r2));
            let (f1, f2) = (Fl::new(m), Fl::new(m));
            let mut e1 = f1.init(m, l1);
            let mut e2 = f2.init(m, l2);
            while f1.first(m) != 1 {
                let (a, b) = (data(m, e1), data(m, e2));
                let w = m.u32(a + 0x50);
                m.set_u32(b + 0x50, w);
                e1 = f1.next(m, l1);
                e2 = f2.next(m, l2);
            }
            f2.done(m);
            f1.done(m);
        } else {
            let stdout = m.u32(STDOUT);
            o.call(m, FPRINTF, &[stdout, COPY_FLAGS_MSG]);
            ret = 0;
        }
    }
    ret
}

/// [soft 0x0850d340 si_nurbsInterAlreadyThere]: `(list, inter, srf)`: with a closing flag set on the surface (+0x20 for u,
/// +0x44 for v), an intersection whose u (v) lies within 1e-4 of the surface's far end (+0x18 / +0x3c) is moved to the
/// near end (+0x10 / +0x34); then 1 when some intersection of a non-empty list lies within a squared distance of 1e-8
/// in (u, v), else 0 (0 for a null list or intersection).
pub fn inter_already_there(m: &mut Mem, list: u32, inter: u32, srf: u32) -> i16 {
    if list == 0 || inter == 0 {
        return 0;
    }
    let eps = x(SRF_EPS);
    if m.i16(srf + 0x20) != 0 && (x(m.f64(inter + 0x30)) - x(m.f64(srf + 0x18))).abs().lt(eps) {
        let v = si_x87::fmov64(m.f64(srf + 0x10));
        m.set_f64(inter + 0x30, v);
    }
    if m.i16(srf + 0x44) != 0 && (x(m.f64(inter + 0x38)) - x(m.f64(srf + 0x3c))).abs().lt(eps) {
        let v = si_x87::fmov64(m.f64(srf + 0x34));
        m.set_f64(inter + 0x38, v);
    }
    if m.u32(list + 4) == 0 {
        return 0;
    }
    let fl = Fl::new(m);
    let mut e = fl.init(m, list);
    let mut out = 0i16;
    while fl.first(m) != 1 {
        let d = data(m, e);
        let dist = inter_dist_2d(m.f64(d + 0x30), m.f64(d + 0x38), m.f64(inter + 0x30), m.f64(inter + 0x38));
        if dist.lt(x(CLOSE2)) {
            out = 1;
            break;
        }
        e = fl.next(m, list);
    }
    fl.done(m);
    out
}

/// [soft 0x0850d4d4 si_nurbsInterBuildLinearCrv]: `(inter, n, closed)`: a linear curve of order 2 with `n` points (`n + 1`
/// knots, closing flag +0x20 set, when `closed`) taken from the chain of intersections starting at `inter` (following the
/// next links, +0x74): point = (u, v) at x, y, the knot = +0x64 distance, the point's flag word (+0x20) = +0x6c; the curve's
/// range (+0x10, +0x18) the first and last knot. A chain shorter than `n` leaves the rest of the points / knots as allocated.
pub fn inter_build_linear_crv(m: &mut Mem, inter: u32, n: i32, closed: i16) -> u32 {
    use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_points_allocate};
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return 0;
    }
    if closed == 0 {
        m.set_i32(c + 8, n);
        let k = knots_allocate(m, n);
        m.set_u32(c + 0xc, k);
    } else {
        m.set_i32(c + 8, n.wrapping_add(1));
        let k = knots_allocate(m, n.wrapping_add(1));
        m.set_u32(c + 0xc, k);
        m.set_i16(c + 0x20, 1);
    }
    let p = nurbs_points_allocate(m, n);
    m.set_u32(c + 0x2c, p);
    m.set_i32(c + 4, n);
    m.set_i32(c, 2);
    m.set_i32(c + 0x34, 1);
    let mut cur = inter;
    let mut i = 0i32;
    if cur != 0 && 0 < n {
        let mut off = 0u32;
        loop {
            let pts = m.u32(c + 0x2c);
            let (u, v, d) = (m.u64(cur + 0x30), m.u64(cur + 0x38), m.u64(cur + 0x64));
            m.set_u64(pts.wrapping_add(off), u);
            m.set_u64(pts.wrapping_add(8).wrapping_add(off), v);
            let ks = m.u32(c + 0xc);
            m.set_u64(ks.wrapping_add((i as u32).wrapping_mul(8)), d);
            let f = m.u32(cur + 0x6c);
            m.set_u32(pts.wrapping_add(0x20).wrapping_add(off), f);
            cur = m.u32(cur + 0x74);
            off = off.wrapping_add(0x24);
            i += 1;
            if cur == 0 || i >= n {
                break;
            }
        }
    }
    let ks = m.u32(c + 0xc);
    let first = m.u64(ks);
    m.set_u64(c + 0x10, first);
    let cnt = m.i32(c + 8);
    let last = m.u64(ks.wrapping_add((cnt.wrapping_sub(1) as u32).wrapping_mul(8)));
    m.set_u64(c + 0x18, last);
    c
}

// ------------------------------------------------------------------------------------------ ray.c

/// [soft 0x0850dacc si_nurbsRayGetInters]: the ray's intersection list (+0x54); 0 for null.
pub fn ray_get_inters(m: &Mem, r: u32) -> u32 {
    if r == 0 {
        0
    } else {
        m.u32(r + 0x54)
    }
}

/// [soft 0x0850dae8 si_nurbsRayGetNoInter]: the ray's intersection count (+0x50); 0 for null.
pub fn ray_get_no_inter(m: &Mem, r: u32) -> u32 {
    if r == 0 {
        0
    } else {
        m.u32(r + 0x50)
    }
}

/// [soft 0x0850d980 si_nurbsRayCreate]: `SI_calloc(1, 0x60)`.
pub fn ray_create(m: &mut Mem) -> u32 {
    m.si_calloc(1, 0x60)
}

/// [soft 0x0850d998 si_nurbsRayCreateP]: `(origin, direction, t, end)`: a ray with those values (the points as words, t through
/// the unit).
pub fn ray_create_p(m: &mut Mem, org: [u32; 6], dir: [u32; 6], t: f64, end: [u32; 6]) -> u32 {
    let p = m.si_calloc(1, 0x60);
    if p != 0 {
        for i in 0..6u32 {
            m.set_u32(p + 4 * i, org[i as usize]);
            m.set_u32(p + 0x18 + 4 * i, dir[i as usize]);
            m.set_u32(p + 0x38 + 4 * i, end[i as usize]);
        }
        m.set_f64(p + 0x30, si_x87::fmov64(t));
    }
    p
}

/// [soft 0x0850d9e8 si_nurbsRayCopy]: a copy of the ray: record words copied (t through the unit), the count and +0x5c, and
/// the intersection list copied by `SI_genListCopy` with [`INTER_COPY`] / [`INTER_COMP_T`]; every copied intersection's ray
/// (+0x40) is the new ray. 0 for null.
pub fn ray_copy(m: &mut Mem, o: &mut dyn Outside, r: u32) -> u32 {
    let o = &mut Named(o);
    if r == 0 {
        return 0;
    }
    let n = m.si_calloc(1, 0x60);
    if n == 0 {
        return 0;
    }
    for i in 0..6u32 {
        for base in [0u32, 0x18, 0x38] {
            let w = m.u32(r + base + 4 * i);
            m.set_u32(n + base + 4 * i, w);
        }
    }
    let t = si_x87::fmov64(m.f64(r + 0x30));
    m.set_f64(n + 0x30, t);
    let w = m.u32(r + 0x5c);
    m.set_u32(n + 0x5c, w);
    let w = m.u32(r + 0x50);
    m.set_u32(n + 0x50, w);
    let src = m.u32(r + 0x54);
    let l = genlist::copy(m, o, src, INTER_COPY.addr, INTER_COMP_T.addr);
    m.set_u32(n + 0x54, l);
    let fl = Fl::new(m);
    let mut e = fl.init(m, l);
    while fl.first(m) != 1 {
        let d = data(m, e);
        m.set_u32(d + 0x40, n);
        let l = m.u32(n + 0x54);
        e = fl.next(m, l);
    }
    fl.done(m);
    n
}

/// [soft 0x0850db00 si_nurbsRayDestroy]: `(pp)`: for a non-null `*pp`: destroys the intersection list at `*pp + 0x54`
/// (`SI_genListDestroy` with [`INTER_DESTROY`]), frees the ray and clears `*pp`.
pub fn ray_destroy(m: &mut Mem, o: &mut dyn Outside, pp: u32) {
    let o = &mut Named(o);
    if pp == 0 || m.u32(pp) == 0 {
        return;
    }
    let r = m.u32(pp);
    genlist::destroy(m, o, r + 0x54, INTER_DESTROY.addr);
    let r = m.u32(pp);
    if r != 0 {
        m.free(r);
        m.set_u32(pp, 0);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x0850dc64 si_nurbsRayAddInter]: `(ray, inter, srf)`: creates the ray's list when it has none (0 if that fails);
/// when [`inter_already_there`] answers 0 the count goes up and the intersection is inserted in order of t
/// (`SI_genListInsertS` with [`INTER_COMP_T`]), answering the element; otherwise the intersection is destroyed and 0 answered.
pub fn ray_add_inter(m: &mut Mem, o: &mut dyn Outside, ray: u32, inter: u32, srf: u32) -> u32 {
    let o = &mut Named(o);
    if ray == 0 || inter == 0 {
        return 0;
    }
    if m.u32(ray + 0x54) == 0 {
        let l = genlist::create(m);
        m.set_u32(ray + 0x54, l);
        if l == 0 {
            return 0;
        }
    }
    let l = m.u32(ray + 0x54);
    if inter_already_there(m, l, inter, srf) == 0 {
        let c = m.u32(ray + 0x50).wrapping_add(1);
        m.set_u32(ray + 0x50, c);
        let l = m.u32(ray + 0x54);
        return genlist::insert_s(m, o, l, inter, INTER_COMP_T.addr);
    }
    let f = m.local(4);
    m.set_u32(f, inter);
    inter_destroy(m, f);
    m.release(4);
    0
}

/// [soft 0x0850e0e0 si_nurbsRayBinarySplitRays] (answers in st0, checked with `nurbs-trimcheck`): `(a, b, c, d)`: m = (a + b) * 0.5; when a < b: m += (c - d) * 0.5 and,
/// when c < m, m -= (c - d). 80-bit; answers m.
pub fn ray_binary_split_rays(a: f64, b: f64, c: f64, d: f64) -> F80 {
    let half = x(0.5); // 0x08662090
    let mut mid = (x(a) + x(b)) * half;
    if x(a).lt(x(b)) {
        let diff = x(c) - x(d);
        mid = mid + diff * half;
        if x(c).lt(mid) {
            mid = mid - diff;
        }
    }
    mid
}

/// [soft 0x0850e130 si_nurbsRayInitRayCast]: `(p1, dir, p3, type, out54, out58, out5c)`, the three points of 3 doubles: `*out58 = p3`;
/// for `type == 1`: `*out54 = p3 - p1` (each stored rounded) and `*out5c = p1`; otherwise `*out54 = dir` and `*out5c = p3`.
pub fn ray_init_ray_cast(m: &mut Mem, p1: [u32; 6], dir: [u32; 6], p3: [u32; 6], typ: i32, out54: u32, out58: u32, out5c: u32) {
    let put = |m: &mut Mem, at: u32, w: &[u32; 6]| {
        for i in 0..6u32 {
            m.set_u32(at + 4 * i, w[i as usize]);
        }
    };
    let f = |w: &[u32; 6], k: usize| f64::from_bits(w[2 * k] as u64 | (w[2 * k + 1] as u64) << 32);
    if typ == 1 {
        put(m, out58, &p3);
        for k in 0..3 {
            let v = (x(f(&p3, k)) - x(f(&p1, k))).to_f64();
            m.set_f64(out54 + 8 * k as u32, v);
            put(m, out58, &p3);
        }
        put(m, out5c, &p1);
    } else {
        put(m, out54, &dir);
        put(m, out5c, &p3);
        let w: Vec<u32> = (0..6u32).map(|i| m.u32(out5c + 4 * i)).collect();
        for i in 0..6u32 {
            m.set_u32(out58 + 4 * i, w[i as usize]);
        }
    }
}

/// [soft 0x0850e1c4 nurbsRayValidNewRay]: `(ray, a, b)`: 0 for a null ray or when both `a` and `b` have fewer intersections
/// than `ray` (+0x50); otherwise 1, unless some intersection's direction has |dot with the ray's direction| below 0.01 (0x08662098), then 0.
pub fn ray_valid_new_ray(m: &mut Mem, ray: u32, a: u32, b: u32) -> i16 {
    if ray == 0 || (m.i32(a + 0x50) < m.i32(ray + 0x50) && m.i32(b + 0x50) < m.i32(ray + 0x50)) {
        return 0;
    }
    let list = m.u32(ray + 0x54);
    let fl = Fl::new(m);
    let mut e = fl.init(m, list);
    let mut res = 1i16;
    while fl.first(m) != 1 {
        let i = data(m, e);
        let dot = x(m.f64(i + 0x18)) * x(m.f64(ray + 0x18)) + x(m.f64(i + 0x20)) * x(m.f64(ray + 0x20)) + x(m.f64(i + 0x28)) * x(m.f64(ray + 0x28));
        if dot.abs().lt(x(0.01)) {
            res = 0;
            break;
        }
        e = fl.next(m, list);
    }
    fl.done(m);
    res
}

/// The callbacks the generic lists call by address: `si_nurbsInterCopy`, `si_nurbsInterCompT` and `si_nurbsInterDestroy`.
/// Returns `None` for any other routine.
pub fn route(m: &mut Mem, r: Routine, args: &[u32]) -> Option<Ret> {
    match r.addr {
        0x0850_d448 => Some(Ret::eax(inter_copy(m, args[0]))),
        0x0850_d4a4 => Some(Ret::eax(inter_comp_t(m, args[0], args[1]))),
        0x0850_cacc => {
            inter_destroy(m, args[0]);
            Some(Ret::eax(0))
        }
        // [soft 0x084e62f4 SI_nurbsCrvFlatten] (flatten.rs): the display polyline of a curve, for the trim checkers and the product.
        // A refusal (the original would not end its span search) answers 0, as the stand-ins of the earlier vectors did.
        0x084e_62f4 => {
            let r = crate::flatten::crv_flatten(m, args[0]);
            Some(Ret::eax(r.unwrap_or(0)))
        }
        _ => None,
    }
}
