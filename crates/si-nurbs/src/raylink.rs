// SPDX-License-Identifier: MIT
//! `ray.c`, the rest (finding 0044, section "Trim9"): the search for the cut between two rays (`RayBinarySearch`,
//! `peturbBinarySplit`), the three ways of linking the intersections of two neighbouring rays (`LinkCurLess`, `LinkCurMore`,
//! `ConnectNoTrans`), the sampling of a new ray (`addRayToSampling`) and `ConnectTrans` with the helpers it chooses between.
//! Records as in [`crate::inter`]; lists are the generic lists of `si-gen`.

use crate::inter::{self, genlist, Fl};
use crate::trim::{clear_inter_links, ray_cast, start_at_zero, RayCastArgs, CRV_VALUE};
use si_mem::{Mem, Outside, Ret, Routine};
use si_x87::F80;

/// The callback the generic lists get for a ray (`SI_genListRemove`, `SI_genListElemReplaceData`).
pub const RAY_DESTROY: Routine = Routine::new("si_nurbsRayDestroy", 0x0850_db00);

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// Gives the generic lists' bare-address callback to the port: a call of `si_nurbsRayDestroy` runs [`inter::ray_destroy`].
struct RayCb<'a>(&'a mut dyn Outside);

impl Outside for RayCb<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        if r.addr == RAY_DESTROY.addr {
            inter::ray_destroy(m, &mut *self.0, args[0]);
            return Ret::NONE;
        }
        self.0.call(m, r, args)
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        self.0.print(stream, text)
    }
}

fn data(m: &Mem, e: u32) -> u32 {
    genlist::elem_get_data(m, e)
}

fn w6(m: &Mem, at: u32) -> [u32; 6] {
    let mut w = [0u32; 6];
    for (i, v) in w.iter_mut().enumerate() {
        *v = m.u32(at + 4 * i as u32);
    }
    w
}

/// Destroys the ray whose pointer is in the cell `cell` (the original passes `&local`).
fn destroy_cell(m: &mut Mem, o: &mut dyn Outside, cell: u32) {
    inter::ray_destroy(m, o, cell);
}

/// The ray at `trim`'s curve: casts a ray through the curve's point at `t` ([`CRV_VALUE`], `si_nurbsRayInitRayCast`,
/// [`ray_cast`] with no perturbation) and answers it (0 when it hits nothing); the common middle of both searches.
fn cast_at(m: &mut Mem, o: &mut dyn Outside, trim: u32, t: f64, status: u32) -> u32 {
    let sp = m.sp();
    let (val, b54, b58, b5c) = (m.local(24), m.local(24), m.local(24), m.local(24));
    let crv = m.u32(trim + 0xc);
    let lo = t.to_bits();
    o.call(m, CRV_VALUE, &[val, crv, lo as u32, (lo >> 32) as u32, 0]);
    let (vp, dir, p3) = (w6(m, trim + 0x44), w6(m, trim + 0x5c), w6(m, val));
    let typ = m.i32(trim + 0x3c);
    inter::ray_init_ray_cast(m, vp, dir, p3, typ, b54, b58, b5c);
    let args = RayCastArgs { origin: w6(m, b5c), dir: w6(m, b54), t, end: w6(m, b58), status, perturb: 0 };
    let r = ray_cast(m, o, trim, &args);
    m.set_sp(sp);
    r
}

/// [soft 0x0850e288 peturbBinarySplit]: `(ray1, ray2, t, step, trim)`: from `t` (kept in 80 bits) steps by `step` until a ray cast through
/// the curve's point has the intersection count of the lower (`step` < 0) or upper (`step` > 0) ray; the parameter is wrapped into the
/// curve's range (+0x10 .. +0x18 of the curve at trim +0xc) and the search stops (0) when it leaves the span of the two rays. A step
/// of 0 (or NaN) stops at the first ray found. Answers the ray or 0. (The original's sixth argument, the status, is a scratch slot of its own.)
pub fn peturb_binary_split(m: &mut Mem, o: &mut dyn Outside, ray1: u32, ray2: u32, t: f64, step: f64, trim: u32) -> u32 {
    let sp = m.sp();
    let res = m.local(4);
    let status = m.local(4);
    m.set_u32(res, 0);
    let (mut tt, st) = (x(t), x(step));
    loop {
        if m.u32(res) != 0 {
            destroy_cell(m, o, res);
        }
        tt = tt + st;
        let crv = m.u32(trim + 0xc);
        let (e, s) = (x(m.f64(crv + 0x18)), x(m.f64(crv + 0x10)));
        if e.lt(tt) {
            tt = s + (tt - e);
        } else if s.gt(tt) {
            tt = e - (s - tt);
        }
        if tt.gt(x(m.f64(ray2 + 0x30))) || tt.lt(x(m.f64(ray1 + 0x30))) {
            break;
        }
        let r = cast_at(m, o, trim, tt.to_f64(), status);
        m.set_u32(res, r);
        if r == 0 {
            continue;
        }
        if x(0.0).lt(st) {
            if m.u32(r + 0x50) == m.u32(ray2 + 0x50) {
                break;
            }
        } else if x(0.0).gt(st) {
            if m.u32(r + 0x50) == m.u32(ray1 + 0x50) {
                break;
            }
        } else {
            break;
        }
    }
    let out = m.u32(res);
    m.set_sp(sp);
    out
}

/// [soft 0x0850dd04 si_nurbsRayBinarySearch]: `(e1, e2, pp3, pp4, trim)`: between the rays of the list elements `e1` and `e2`
/// (parameter t at +0x30) bisects ([`inter::ray_binary_split_rays`]) and casts rays until the two current rays are within a squared
/// distance of 1e-8 (+0x38 points) or the bisection parameter moves by less than 1e-6. A new ray that is not valid
/// ([`inter::ray_valid_new_ray`]) sends the search to [`peturb_binary_split`] on both sides (+0.1 for the upper, -0.1 for the lower)
/// and ends it with no element; one with the count of the lower (upper) ray replaces it; one with another count is made into an element
/// (answered) and, unless it has no intersections or the list is already flagged, flags the list (bit 0) and drops both current rays.
/// `*pp3` / `*pp4` get the lower / upper current ray; answers the new element, else `e2`.
pub fn ray_binary_search(m: &mut Mem, o: &mut dyn Outside, e1: u32, e2in: u32, pp3: u32, pp4: u32, trim: u32) -> u32 {
    let sp = m.sp();
    let (c1, c2, res, status) = (m.local(4), m.local(4), m.local(4), m.local(4));
    let mut ta = -3.4028234663852886e38f64; // 0x08662060
    let mut tb = 3.4028234663852886e38f64; // 0x08662068
    let o1 = data(m, e1);
    let o2 = data(m, e2in);
    m.set_u32(c1, o1);
    m.set_u32(c2, o2);
    let mut e2 = e2in;
    loop {
        let (r1, r2) = (m.u32(c1), m.u32(c2));
        if !inter::ray_dist_3d(m, r1 + 0x38, r2 + 0x38).gt(x(1e-8)) {
            break;
        }
        if (x(ta) - x(tb)).abs().lt(x(1e-6)) {
            break;
        }
        tb = ta;
        let crv = m.u32(trim + 0xc);
        let (ee, ss) = (m.f64(crv + 0x18), m.f64(crv + 0x10));
        ta = inter::ray_binary_split_rays(m.f64(r2 + 0x30), m.f64(r1 + 0x30), ee, ss).to_f64();
        let r = cast_at(m, o, trim, ta, status);
        m.set_u32(res, r);
        if inter::ray_valid_new_ray(m, r, o1, o2) == 0 {
            if m.u32(res) != 0 {
                destroy_cell(m, o, res);
            }
            let (a, b) = (m.u32(c1), m.u32(c2));
            let p = peturb_binary_split(m, o, a, b, ta, 0.1, trim); // 0x08662080
            m.set_u32(res, p);
            if p != 0 {
                if m.u32(c2) != o2 {
                    destroy_cell(m, o, c2);
                }
                m.set_u32(c2, p);
            }
            let (a, b) = (m.u32(c1), m.u32(c2));
            let p = peturb_binary_split(m, o, a, b, ta, -0.1, trim); // 0x08662088
            m.set_u32(res, p);
            if p != 0 {
                if m.u32(c1) != o1 {
                    destroy_cell(m, o, c1);
                }
                m.set_u32(c1, p);
            }
            e2 = 0;
            break;
        }
        let r = m.u32(res);
        let n = m.u32(r + 0x50);
        let (k1, k2) = (m.u32(c1), m.u32(c2));
        if n == m.u32(k1 + 0x50) {
            if k1 != o1 {
                destroy_cell(m, o, c1);
            }
            m.set_u32(c1, r);
        } else if n == m.u32(k2 + 0x50) {
            if k2 != o2 {
                destroy_cell(m, o, c2);
            }
            m.set_u32(c2, r);
        } else {
            e2 = genlist::elem_create(m);
            genlist::elem_set_data(m, e2, r);
            if inter::ray_get_no_inter(m, r) == 0 {
                let list = m.u32(e1 + 0xc);
                if genlist::get_flags(m, list) & 1 == 0 {
                    let f = genlist::get_flags(m, list) | 1;
                    genlist::set_flags(m, list, f);
                    if k1 != o1 {
                        destroy_cell(m, o, c1);
                    }
                    if k2 != o2 {
                        destroy_cell(m, o, c2);
                    }
                    m.set_u32(c2, 0);
                }
            }
            break;
        }
    }
    let (a, b) = (m.u32(c1), m.u32(c2));
    m.set_u32(pp3, a);
    m.set_u32(pp4, b);
    m.set_sp(sp);
    e2
}

fn crv_ends(m: &Mem, crv: u32) -> (f64, f64) {
    (si_x87::fmov64(m.f64(crv + 0x10)), si_x87::fmov64(m.f64(crv + 0x18)))
}

fn toggle(d: i16) -> i16 {
    (d == 0) as i16
}

/// [soft 0x0850e44c si_nurbsRayLinkCurLess]: `(r1, r2, e3, e4, dir, crv)`: the ray `r1` has fewer intersections than `r2`. Walks the
/// intersections of `r1` with the nearest ([`inter::inter_find_closest`]) of those of the ray of `e4`, linking
/// ([`inter::inter_link`], the direction toggling at each step) them to the intersections of the ray of `e3` in step; then through the
/// intersections of the ray of `e4` linking each one that is not on the boundary and not yet connected in the direction (the
/// direction flipping at every one passed) to the next. Answers 1 when [`inter::inter_update_next_con`] of the first nearest one is 0.
pub fn link_cur_less(m: &mut Mem, r1: u32, r2: u32, e3: u32, e4: u32, dir: i16, crv: u32) -> i16 {
    let sp = m.sp();
    let flag = m.local(4);
    m.set_i16(flag, 0);
    let ray3 = data(m, e3);
    let l3 = inter::ray_get_inters(m, ray3);
    let ray4 = data(m, e4);
    let l4 = inter::ray_get_inters(m, ray4);
    let l1 = inter::ray_get_inters(m, r1);
    let (f1, f3, f4) = (Fl::new(m), Fl::new(m), Fl::new(m));
    let mut it1 = f1.init(m, l1);
    let e = inter::inter_find_closest(m, it1, r2, ray4, flag);
    let u1 = inter::inter_update_next_con(m, e, (dir == 0) as i16);
    let mut it3 = f3.init(m, l3);
    let mut d = dir;
    let (d4, d5) = crv_ends(m, crv);
    while f1.first(m) != 1 {
        let e = inter::inter_find_closest(m, it1, r2, ray4, flag);
        inter::inter_link(m, it3, e, d, d4, d5);
        it1 = f1.next(m, l1);
        it3 = f3.next(m, l3);
        d = toggle(d);
    }
    let mut eb = f4.init(m, l4);
    let mut d = u1;
    while f4.first(m) != 1 {
        if inter::inter_cross_bndry(m, eb) == 0 && inter::inter_unconn(m, eb, d) != 0 {
            let prev = eb;
            eb = f4.next(m, l4);
            if f4.first(m) != 1 {
                inter::inter_link(m, prev, eb, d, d4, d5);
            }
        } else {
            d = toggle(d);
        }
        if f4.first(m) == 1 {
            break;
        }
        eb = f4.next(m, l4);
    }
    m.set_sp(sp);
    (u1 == 0) as i16
}

/// [soft 0x0850e6a0 si_nurbsRayLinkCurMore]: `(r1, r2, e3, e4, dir, crv)`: the ray `r1` has more intersections than `r2`: the mirror
/// of [`link_cur_less`]: walks the intersections of `r2` linking each to the nearest of the ray of `e3` in step with those of the ray
/// of `e4`, the direction as the nearest search leaves it, then links the connectable intersections of the ray of `e3` among themselves.
/// Answers the direction the first search left.
pub fn link_cur_more(m: &mut Mem, r1: u32, r2: u32, e3: u32, e4: u32, dir: i16, crv: u32) -> i16 {
    let sp = m.sp();
    let flag = m.local(4);
    m.set_i16(flag, 0);
    let ray3 = data(m, e3);
    let l3 = inter::ray_get_inters(m, ray3);
    let ray4 = data(m, e4);
    let l4 = inter::ray_get_inters(m, ray4);
    let l2 = inter::ray_get_inters(m, r2);
    let (f2, f3, f4) = (Fl::new(m), Fl::new(m), Fl::new(m));
    let mut it2 = f2.init(m, l2);
    let mut it4 = f4.init(m, l4);
    let (d4, d5) = crv_ends(m, crv);
    let mut first = true;
    let mut save = dir;
    while f2.first(m) != 1 {
        m.set_i16(flag, dir);
        let e = inter::inter_find_closest(m, it2, r1, ray3, flag);
        let fl = m.i16(flag);
        if first {
            first = false;
            save = fl;
        }
        inter::inter_link(m, e, it4, fl, d4, d5);
        it2 = f2.next(m, l2);
        it4 = f4.next(m, l4);
    }
    let mut eb = f3.init(m, l3);
    m.set_i16(flag, dir);
    while f3.first(m) != 1 {
        let fl = m.i16(flag);
        if inter::inter_cross_bndry(m, eb) == 0 && inter::inter_unconn(m, eb, fl) != 0 {
            let n = f3.next(m, l3);
            inter::inter_link(m, eb, n, fl, d4, d5);
        } else {
            m.set_i16(flag, toggle(fl));
        }
        eb = f3.next(m, l3);
    }
    m.set_sp(sp);
    save
}

/// [soft 0x0850e8c4 si_nurbsRayConnectNoTrans]: `(r1, r2, dir, crv)`: the two rays have the same count: links the intersections of `r1`
/// to those of `r2` in step (the direction toggling at each); answers `dir`.
pub fn connect_no_trans(m: &mut Mem, r1: u32, r2: u32, dir: i16, crv: u32) -> i16 {
    let sp = m.sp();
    let (l1, l2) = (inter::ray_get_inters(m, r1), inter::ray_get_inters(m, r2));
    let (f1, f2) = (Fl::new(m), Fl::new(m));
    let mut e1 = f1.init(m, l1);
    let mut e2 = f2.init(m, l2);
    let (d4, d5) = crv_ends(m, crv);
    let mut d = dir;
    while f1.first(m) != 1 {
        inter::inter_link(m, e1, e2, d, d4, d5);
        d = toggle(d);
        e1 = f1.next(m, l1);
        e2 = f2.next(m, l2);
    }
    m.set_sp(sp);
    dir
}

/// The arguments of [`add_ray_to_sampling`] that point into the caller's frame and the list under work.
#[derive(Clone, Copy)]
pub struct Sampling {
    /// Cell holding the lower element.
    pub pe3: u32,
    /// Cell holding the upper element.
    pub pe4: u32,
    pub list: u32,
    /// The two flag words of the list's iteration (`first`, `last`).
    pub pf: u32,
    pub pl: u32,
    /// Cell (16 bits) that gets 1 when the iteration ran off the end.
    pub pflag: u32,
}

/// [soft 0x0850f348 addRayToSampling]: `(p1, p2, p3, p4, trim, dir, sampling)`: `p1` / `p2` are the rays `p3` / `p4` as they are to be
/// sampled (a copy is made when they are the same ray); both are classified against the boundary ([`inter::inter_classify_bndry`]) and
/// their flags copied. When `p3` has an even and `p4` an odd count and `p2` is not on the boundary, the upper element is removed from
/// the list and the iteration moves on (setting `*pflag` at the end of the list) before `p2` is made again from the new upper ray. Then
/// by comparing the counts of `p3` and `p4`: fewer is [`link_cur_less`], more [`link_cur_more`], equal [`connect_no_trans`] (with the
/// curve at `trim` + 0xc); the copies are destroyed. Answers the link routine's answer.
pub fn add_ray_to_sampling(m: &mut Mem, o: &mut dyn Outside, p1in: u32, p2in: u32, p3: u32, p4in: u32, trim: u32, dir: i32, s: &Sampling) -> i16 {
    let sp = m.sp();
    let (c1, c2) = (m.local(4), m.local(4));
    let mut p4 = p4in;
    let dir16 = dir as i16;
    let mut p2 = p2in;
    if p2 == p4 {
        p2 = inter::ray_copy(m, o, p4);
    }
    let mut p1 = p1in;
    if p1 == p3 {
        p1 = inter::ray_copy(m, o, p3);
    }
    inter::inter_classify_bndry(m, p1, trim);
    inter::inter_copy_flags(m, o, p1, p3);
    let sflag = inter::inter_classify_bndry(m, p2, trim);
    inter::inter_copy_flags(m, o, p2, p4);
    m.set_u32(c1, p1);
    m.set_u32(c2, p2);
    if m.u8(p3 + 0x50) & 1 == 0 && m.u8(p4 + 0x50) & 1 != 0 && sflag == 0 {
        if p2 != p4 {
            destroy_cell(m, o, c2);
        }
        let e4 = m.u32(s.pe4);
        let e = genlist::remove(m, &mut RayCb(&mut *o), s.list, e4, RAY_DESTROY.addr);
        m.set_u32(s.pe3, e);
        genlist::iter_set(m, s.list, e, s.pf, s.pl);
        let n = genlist::iter_next(m, s.list, s.pf, s.pl);
        m.set_u32(s.pe4, n);
        if m.i16(s.pf) == 1 {
            m.set_i16(s.pflag, 1);
        }
        p4 = data(m, n);
        let c = inter::ray_copy(m, o, p4);
        m.set_u32(c2, c);
        inter::inter_classify_bndry(m, c, trim);
        inter::inter_copy_flags(m, o, c, p4);
    }
    let (p1, p2) = (m.u32(c1), m.u32(c2));
    let crv = m.u32(trim + 0xc);
    let (n3, n4) = (m.i32(p3 + 0x50), m.i32(p4 + 0x50));
    let (e3, e4) = (m.u32(s.pe3), m.u32(s.pe4));
    let r = if n3 < n4 {
        link_cur_less(m, p1, p2, e3, e4, dir16, crv)
    } else if n3 > n4 {
        link_cur_more(m, p1, p2, e3, e4, dir16, crv)
    } else {
        connect_no_trans(m, p3, p4, dir16, crv)
    };
    if m.u32(c2) != p4 {
        destroy_cell(m, o, c2);
    }
    if m.u32(c1) != p3 {
        destroy_cell(m, o, c1);
    }
    m.set_sp(sp);
    r
}

/// [soft 0x0850edc0 upperIsNull]: `(e, pp, g, list, pf, pl, pflag, dir)`: `g` goes after `e`, the list starts again at zero
/// (`start_at_zero`), `*pp` is the first element of a new iteration, the intersection links of the list are cleared and
/// `*pflag` is 0. Answers `dir`.
pub fn upper_is_null(m: &mut Mem, e: u32, pp: u32, g: u32, list: u32, pf: u32, pl: u32, pflag: u32, dir: i16) -> i16 {
    genlist::insert_elem_after(m, e, g);
    start_at_zero(m, list);
    let n = genlist::iter_init(m, list, pf, pl);
    m.set_u32(pp, n);
    clear_inter_links(m, list);
    m.set_i16(pflag, 0);
    dir
}

/// [soft 0x0850ee20 addNewRay]: `(e, pp, g, r4, r5, pL8, pL4, list, pf, pl, pflag, dir)`: drops the current lower / upper ray (`*pL8`,
/// `*pL4`) unless it is the interval's own (`r4`, `r5`), puts `g` after `e`, makes `*pp` the element `e`, restarts the iteration
/// there (`iter_set`), `*pflag` = 0; answers `dir`.
pub fn add_new_ray(m: &mut Mem, o: &mut dyn Outside, e: u32, pp: u32, g: u32, r4: u32, r5: u32, pl8: u32, pl4: u32, list: u32, pf: u32, pl: u32, pflag: u32, dir: i16) -> i16 {
    if m.u32(pl8) != r4 {
        destroy_cell(m, o, pl8);
    }
    if m.u32(pl4) != r5 {
        destroy_cell(m, o, pl4);
    }
    genlist::insert_elem_after(m, e, g);
    m.set_u32(pp, e);
    genlist::iter_set(m, list, e, pf, pl);
    m.set_i16(pflag, 0);
    dir
}

fn no_inter(m: &Mem, r: u32) -> i32 {
    inter::ray_get_no_inter(m, r) as i32
}

/// [soft 0x0850f008 nextIsUpper]: `(pe1, ray2, ppa, ppb, ppc, pflag)`: when the ray at `*ppb` has more intersections than the one at
/// `*ppc` and `*pflag` (16 bits) is 0, the element `*pe1` takes `*ppb` as its datum (the old one destroyed) and `*ppb` becomes a
/// copy of `ray2`; then `*ppa` is the element's datum and `*ppc` a copy of it.
pub fn next_is_upper(m: &mut Mem, o: &mut dyn Outside, pe1: u32, ray2: u32, ppa: u32, ppb: u32, ppc: u32, pflag: u32) {
    let (b, c) = (m.u32(ppb), m.u32(ppc));
    if no_inter(m, b) > no_inter(m, c) && m.i16(pflag) == 0 {
        let e = m.u32(pe1);
        genlist::elem_replace_data(m, &mut RayCb(&mut *o), e, RAY_DESTROY.addr, b);
        let cp = inter::ray_copy(m, o, ray2);
        m.set_u32(ppb, cp);
    }
    let e = m.u32(pe1);
    let a = data(m, e);
    m.set_u32(ppa, a);
    let cp = inter::ray_copy(m, o, a);
    m.set_u32(ppc, cp);
}

/// [soft 0x0850f090 curIsLower]: `(pe1, pe2, pa3, pa4, pa5, pa6, list, pf, pl, pflag)`: when the upper current ray (`*pa6`) has more
/// intersections than the lower (`*pa5`) the element `*pe1` takes it as datum and, if the element has a previous one (not the list's
/// head element), the iteration moves back to it, `*pe2` is the next one (`*pflag` set at the end of the list) and `*pa3` / `*pa4` /
/// `*pa6` are redone from the two elements (`*pflag` 0); finally `*pa5` is a copy of `*pa3`.
pub fn cur_is_lower(m: &mut Mem, o: &mut dyn Outside, pe1: u32, pe2: u32, pa3: u32, pa4: u32, pa5: u32, pa6: u32, list: u32, pf: u32, pl: u32, pflag: u32) {
    let (a5, a6) = (m.u32(pa5), m.u32(pa6));
    if no_inter(m, a6) > no_inter(m, a5) {
        let e = m.u32(pe1);
        genlist::elem_replace_data(m, &mut RayCb(&mut *o), e, RAY_DESTROY.addr, a6);
        let e = m.u32(pe1);
        let head = genlist::get_head(m, list);
        if m.u32(e + genlist::PREV) != head {
            let p = m.u32(e + genlist::PREV);
            m.set_u32(pe1, p);
            genlist::iter_set(m, list, p, pf, pl);
            let n = genlist::iter_next(m, list, pf, pl);
            m.set_u32(pe2, n);
            if m.i16(pf) == 1 {
                m.set_i16(pflag, 1);
            }
            let d1 = data(m, p);
            m.set_u32(pa3, d1);
            let d2 = data(m, n);
            m.set_u32(pa4, d2);
            let cp = inter::ray_copy(m, o, d2);
            m.set_u32(pa6, cp);
            m.set_i16(pflag, 0);
        }
    }
    let a3 = m.u32(pa3);
    let cp = inter::ray_copy(m, o, a3);
    m.set_u32(pa5, cp);
}

/// [soft 0x0850ee94 curIsLowerUpperIsNext]: same arguments as [`cur_is_lower`]: when the current rays' points (+0x38) are nearer than
/// 2.5e-7 (squared) the one with fewer intersections is removed from the list (`SI_genListRemove` with the ray destructor; when
/// the upper has more, the element `*pe1` itself), the iteration restarts at `*pe1`, `*pe2` is the next element and `*pa3` / `*pa4` /
/// `*pa6` / `*pa5` are redone (`*pflag` set at the end of the list); else just `*pa6` and `*pa5` are copies of `*pa4` / `*pa3`.
pub fn cur_is_lower_upper_is_next(m: &mut Mem, o: &mut dyn Outside, pe1: u32, pe2: u32, pa3: u32, pa4: u32, pa5: u32, pa6: u32, list: u32, pf: u32, pl: u32, pflag: u32) {
    let (a5, a6) = (m.u32(pa5), m.u32(pa6));
    if inter::ray_dist_3d(m, a5 + 0x38, a6 + 0x38).lt(x(2.5000000000000004e-07)) {
        if no_inter(m, a6) > no_inter(m, a5) {
            let e1 = m.u32(pe1);
            let r = genlist::remove(m, &mut RayCb(&mut *o), list, e1, RAY_DESTROY.addr);
            m.set_u32(pe1, r);
        } else {
            let e2 = m.u32(pe2);
            genlist::remove(m, &mut RayCb(&mut *o), list, e2, RAY_DESTROY.addr);
        }
        let e1 = m.u32(pe1);
        genlist::iter_set(m, list, e1, pf, pl);
        let n = genlist::iter_next(m, list, pf, pl);
        m.set_u32(pe2, n);
        let e1 = m.u32(pe1);
        let d1 = data(m, e1);
        m.set_u32(pa3, d1);
        let e2 = m.u32(pe2);
        let d2 = data(m, e2);
        m.set_u32(pa4, d2);
        let cp = inter::ray_copy(m, o, d2);
        m.set_u32(pa6, cp);
        let cp = inter::ray_copy(m, o, d1);
        m.set_u32(pa5, cp);
        if m.i16(pf) == 1 {
            m.set_i16(pflag, 1);
        }
    } else {
        let a4 = m.u32(pa4);
        let cp = inter::ray_copy(m, o, a4);
        m.set_u32(pa6, cp);
        let a3 = m.u32(pa3);
        let cp = inter::ray_copy(m, o, a3);
        m.set_u32(pa5, cp);
    }
}

/// [soft 0x0850f194 curNotLowerUpperNotNext]: same arguments as [`cur_is_lower`]: when the upper current ray has more intersections
/// than the lower and its point (+0x38) is more than 2.5e-7 (squared) from the interval's upper ray `*pa4`, a new element after
/// `*pe1` takes `*pa6` as datum (`*pe2` = it, `*pflag` 0, `*pa4` = the ray, the iteration set there, `*pa6` a copy); otherwise (not more) when the lower
/// current ray is farther than 2.5e-7 from the interval's lower ray `*pa3` the same with `*pa5` (`*pa5` then a copy of `*pa3`, the
/// old `*pa6` destroyed, `*pa6` a copy of `*pa4`).
pub fn cur_not_lower_upper_not_next(m: &mut Mem, o: &mut dyn Outside, pe1: u32, pe2: u32, pa3: u32, pa4: u32, pa5: u32, pa6: u32, list: u32, pf: u32, pl: u32, pflag: u32) {
    let far = x(2.5000000000000004e-07); // 0x086620a8
    let (a5, a6) = (m.u32(pa5), m.u32(pa6));
    if no_inter(m, a6) > no_inter(m, a5) {
        let a4 = m.u32(pa4);
        if inter::ray_dist_3d(m, a4 + 0x38, a6 + 0x38).gt(far) {
            let e = genlist::elem_create(m);
            m.set_u32(pe2, e);
            m.set_i16(pflag, 0);
            genlist::elem_set_data(m, e, a6);
            let e1 = m.u32(pe1);
            genlist::insert_elem_after(m, e1, e);
            m.set_u32(pa4, a6);
            genlist::iter_set(m, list, e, pf, pl);
            let cp = inter::ray_copy(m, o, a6);
            m.set_u32(pa6, cp);
        }
    } else {
        let a3 = m.u32(pa3);
        if inter::ray_dist_3d(m, a5 + 0x38, a3 + 0x38).gt(far) {
            let e = genlist::elem_create(m);
            m.set_u32(pe2, e);
            m.set_i16(pflag, 0);
            genlist::elem_set_data(m, e, a5);
            let e1 = m.u32(pe1);
            genlist::insert_elem_after(m, e1, e);
            m.set_u32(pa4, a5);
            genlist::iter_set(m, list, e, pf, pl);
            let cp = inter::ray_copy(m, o, a3);
            m.set_u32(pa5, cp);
            destroy_cell(m, o, pa6);
            let a4 = m.u32(pa4);
            let cp = inter::ray_copy(m, o, a4);
            m.set_u32(pa6, cp);
        }
    }
}

/// [soft 0x0850eb94 bSearchIsNull]: the search found no new element: `(elem, pp, r3, r4, r5, r6, list, pf, pl, pflag, dir, trim)`, all by
/// value (the frame cells of `elem` and `r3` are made here). `r6` / `r5` are the lower / upper rays the search ended with
/// (`r3` / `r4` the interval's own). When `r6` is not `r3` it becomes a new element after `elem` and is sampled against `r3`
/// ([`add_ray_to_sampling`]); when `r5` is not `r4` the same for it against the last; finally the last element's ray is sampled against
/// the next (or `r4` when nothing was added). Answers the last sampling's answer.
pub fn b_search_is_null(m: &mut Mem, o: &mut dyn Outside, elem: u32, pp: u32, r3in: u32, r4: u32, r5: u32, r6: u32, s: &Sampling, dir: i16, trim: u32) -> i16 {
    let sp = m.sp();
    let (c1, c3) = (m.local(4), m.local(4));
    m.set_u32(c1, elem);
    m.set_u32(c3, r3in);
    let ss = Sampling { pe3: c1, pe4: pp, ..*s };
    let same = r5 == r4;
    let mut first = true;
    let mut d = dir;
    let mut esi = r4;
    if r6 != r3in {
        let e = genlist::elem_create(m);
        m.set_u32(pp, e);
        genlist::elem_set_data(m, e, r6);
        genlist::insert_elem_after(m, elem, e);
        esi = r6;
        let edi = inter::ray_copy(m, o, r3in);
        let ebx = inter::ray_copy(m, o, esi);
        genlist::iter_set(m, s.list, e, s.pf, s.pl);
        d = add_ray_to_sampling(m, o, edi, ebx, r3in, esi, trim, d as i32, &ss);
        let e = m.u32(pp);
        m.set_u32(c1, e);
        first = false;
    }
    if !same {
        let c = m.u32(c1);
        let r3 = data(m, c);
        m.set_u32(c3, r3);
        let edi = inter::ray_copy(m, o, r3);
        let e = genlist::elem_create(m);
        m.set_u32(pp, e);
        genlist::elem_set_data(m, e, r5);
        genlist::insert_elem_after(m, c, e);
        esi = r5;
        let ebx = inter::ray_copy(m, o, esi);
        genlist::iter_set(m, s.list, e, s.pf, s.pl);
        d = add_ray_to_sampling(m, o, edi, ebx, r3, esi, trim, d as i32, &ss);
        let e = m.u32(pp);
        m.set_u32(c1, e);
    }
    let c = m.u32(c1);
    let r3 = data(m, c);
    m.set_u32(c3, r3);
    let edi = inter::ray_copy(m, o, r3);
    if !same || !first {
        let n = genlist::iter_next(m, s.list, s.pf, s.pl);
        m.set_u32(pp, n);
        esi = data(m, n);
    }
    let ebx = inter::ray_copy(m, o, esi);
    let out = add_ray_to_sampling(m, o, edi, ebx, r3, esi, trim, d as i32, &ss);
    m.set_sp(sp);
    out
}

/// [soft 0x0850e9a8 si_nurbsRayConnectTrans]: `(trim, elem, pp, r4, r5, list, pf, pl, pflag, dir)`: the intersections between the interval's rays
/// `r4` / `r5` (the elements `elem` and `*pp`): searches ([`ray_binary_search`]) for a cut between them and then, by what it found,
/// [`b_search_is_null`] (no element), [`upper_is_null`] (no upper ray), [`add_new_ray`] (an element other than `*pp`) or the helpers
/// [`cur_is_lower_upper_is_next`] / [`next_is_upper`] / [`cur_is_lower`] / [`cur_not_lower_upper_not_next`] (the element is `*pp`,
/// by which of the current rays still are the interval's own), and samples ([`add_ray_to_sampling`]) the current rays. Answers the
/// sampling's answer (the helper's, in the first three cases).
pub fn connect_trans(m: &mut Mem, o: &mut dyn Outside, trim: u32, elem_in: u32, pp: u32, r4_in: u32, r5_in: u32, list: u32, pf: u32, pl: u32, pflag: u32, dir: i16) -> i16 {
    let sp = m.sp();
    let (elem, r4, r5) = (m.local(4), m.local(4), m.local(4));
    m.set_u32(elem, elem_in);
    m.set_u32(r4, r4_in);
    m.set_u32(r5, r5_in);
    let (l8, l4) = (m.local(4), m.local(4));
    let e2 = m.u32(pp);
    let g = ray_binary_search(m, o, elem_in, e2, l8, l4, trim);
    let s = Sampling { pe3: elem, pe4: pp, list, pf, pl, pflag };
    let out;
    if g == 0 {
        let (up, lo) = (m.u32(l4), m.u32(l8));
        out = b_search_is_null(m, o, elem_in, pp, r4_in, r5_in, up, lo, &s, dir, trim);
    } else if m.u32(l4) == 0 {
        out = upper_is_null(m, elem_in, pp, g, list, pf, pl, pflag, dir);
    } else if g != m.u32(pp) {
        out = add_new_ray(m, o, elem_in, pp, g, r4_in, r5_in, l8, l4, list, pf, pl, pflag, dir);
    } else {
        let (a5, a6) = (m.u32(l8), m.u32(l4));
        if r5_in == a6 && r4_in == a5 {
            cur_is_lower_upper_is_next(m, o, elem, pp, r4, r5, l8, l4, list, pf, pl, pflag);
        } else if r5_in == a6 {
            next_is_upper(m, o, pp, r4_in, r5, l8, l4, pflag);
        } else if r4_in == a5 {
            cur_is_lower(m, o, elem, pp, r4, r5, l8, l4, list, pf, pl, pflag);
        } else {
            cur_not_lower_upper_not_next(m, o, elem, pp, r4, r5, l8, l4, list, pf, pl, pflag);
        }
        let (lo, up, rr4, rr5) = (m.u32(l8), m.u32(l4), m.u32(r4), m.u32(r5));
        out = add_ray_to_sampling(m, o, lo, up, rr4, rr5, trim, dir as i32, &s);
    }
    m.set_sp(sp);
    out
}
