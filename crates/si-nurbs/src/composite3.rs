// SPDX-License-Identifier: MIT
//! The original's `composite.c`, third part (finding 0044, unit trim7): the containment-tree routines that build the
//! composite trim curves of a surface from the generic tree (`genTree.c`, compiled into this module as `gentree`, since
//! this crate does not depend on `si-gen`; the lists come from `inter::genlist`).

use crate::composite as cp;
use crate::image::Halt;
use si_mem::{Mem, Outside, Ret, Routine};

#[path = "../../si-gen/src/tree.rs"]
#[allow(dead_code)]
pub(crate) mod gentree;
use crate::inter::genlist;
use gentree as tree;

/// `curveInCurve (0x08505150)`: the compare handed to the tree insertions.
pub const CURVE_IN_CURVE: u32 = 0x0850_5150;
/// `si_nurbsCompositeCurveDestroy (0x08503878)`: the destructor handed to tree destruction.
pub const COMP_DESTROY: u32 = 0x0850_3878;
/// `indexDestroy (0x08506088)`.
pub const INDEX_DESTROY: u32 = 0x0850_6088;
/// `getenv (0x080c3bb8)`.
pub const GETENV: Routine = Routine::new("plt:getenv", 0x080c_3bb8);
/// "SI_NO_HIGH_DEGREE_TRIM" in `addCompFromTree`.
const ENV_NO_HD: u32 = 0x086d_b081;
const MSG_HD_FAIL: u32 = 0x086d_b0a0;
const MSG_CUBIC_FAIL: u32 = 0x086d_b100;
/// `si_validTrimCurve (0x084fd0d0)`: `(box (12 words), comp)`, ported in `trimmed.rs` and checked there; outside here.
const VALID_TRIM_CURVE: Routine = Routine::new("si_validTrimCurve", 0x084f_d0d0);
/// `SI_nurbsSrfTrmDispose (0x084db944)`.
const SRF_TRM_DISPOSE: Routine = Routine::new("SI_nurbsSrfTrmDispose", 0x084d_b944);

/// Answers the callbacks that live in this file (`curveInCurve`, the composite and index destructors) with their ports and
/// hands everything else to the outer routing.
pub struct Own<'a>(pub &'a mut dyn Outside);

impl Outside for Own<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        match r.addr {
            CURVE_IN_CURVE => {
                let v = cp::curve_in_curve(m, self.0, args[0], args[1]);
                Ret::eax(v)
            }
            COMP_DESTROY => {
                // the original disposes the curve through SI_nurbsCrvDispose (outside here), frees the block, clears *pp
                let pp = args[0];
                if pp != 0 && m.u32(pp) != 0 {
                    let p = m.u32(pp);
                    self.0.call(m, cp::CRV_DISPOSE, &[p + 0x30]);
                    let p = m.u32(pp);
                    if p != 0 {
                        m.free(p);
                    }
                    m.set_u32(pp, 0);
                }
                Ret::eax(0)
            }
            crate::trimprev::COMP_CURVE_IN_CURVE_EXT => {
                let v = comp_curve_in_curve_ext(m, self.0, args[0], args[1]);
                Ret::eax(v)
            }
            INDEX_DESTROY => {
                cp::index_destroy(m, args[0]);
                Ret::eax(0)
            }
            _ => match self.native(m, r, args) {
                Some(v) => v,
                None => self.0.call(m, r, args),
            },
        }
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        self.0.print(stream, text)
    }
}

impl Own<'_> {
    /// The generic list and tree routines of `si-gen` (`genList.c`, `genTree.c`) answered by their ports, for a caller that
    /// reaches them through [`Outside`]; their callbacks go through this wrapper again.
    fn native(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Option<Ret> {
        let v = match r.addr {
            0x0851_4ef0 => genlist::create(m),
            0x0851_4f08 => {
                genlist::destroy(m, self, a[0], a[1]);
                0
            }
            0x0851_4fb8 => genlist::iter_init(m, a[0], a[1], a[2]),
            0x0851_4ff4 => genlist::iter_next(m, a[0], a[1], a[2]),
            0x0851_5154 => genlist::insert_e(m, a[0], a[1]),
            0x0851_50ec => genlist::insert_b(m, a[0], a[1]),
            0x0851_53d4 => genlist::elem_get_data(m, a[0]),
            0x0851_53e8 => genlist::get_head(m, a[0]),
            0x0851_579c => genlist::elem_of(m, self, a[0], a[1], a[2]),
            0x0851_5830 => tree::create(m),
            0x0851_5b68 => tree::elem_get_data(m, a[0]),
            0x0851_5b7c => tree::get_root(m, a[0]),
            0x0851_5848 => {
                tree::destroy(m, self, a[0], a[1]);
                0
            }
            0x0851_59a0 => tree::elem_destroy(m, self, a[0], a[1]),
            0x0851_5c1c => tree::remove_sub_tree(m, self, a[0], a[1], a[2]),
            0x0851_5cf0 => tree::insert_s(m, self, a[0], a[1], a[2]),
            0x0850_dcd8 => {
                // si_nurbsRayDist3D (ray.c, not ported here): the squared distance of two points, in st0.
                let f = |i: usize| si_x87::F80::from_f64(f64::from_bits(a[i] as u64 | (a[i + 1] as u64) << 32));
                let mut acc = si_x87::F80::ZERO;
                for k in 0..3 {
                    let dd = f(2 * k) - f(6 + 2 * k);
                    acc = acc + dd * dd;
                }
                let (mn, se) = acc.to_bits();
                let mut b = [0u8; 10];
                b[..8].copy_from_slice(&mn.to_le_bytes());
                b[8..].copy_from_slice(&se.to_le_bytes());
                return Some(Ret::st0(b));
            }
            _ => return None,
        };
        Some(Ret::eax(v))
    }
}

fn flag(m: &Mem, e: u32) -> u16 {
    m.u16(tree::elem_get_data(m, e) + 0x34)
}

/// [soft 0x08505090 buildContainHier]: `(list_a, list_b)`: a new tree (0 when it cannot be made) with the data of every
/// element of list A and then of list B inserted by containment (`curveInCurve` as the compare).
pub fn build_contain_hier(m: &mut Mem, o: &mut dyn Outside, list_a: u32, list_b: u32) -> u32 {
    let t = tree::create(m);
    if t == 0 {
        return 0;
    }
    let sp = m.sp();
    let fl = m.local(4);
    for list in [list_a, list_b] {
        let mut e = genlist::iter_init(m, list, fl, fl + 2);
        while (m.i16(fl)) != 1 {
            let d = genlist::elem_get_data(m, e);
            tree::insert_s(m, o, t, d, CURVE_IN_CURVE);
            e = genlist::iter_next(m, list, fl, fl + 2);
        }
    }
    m.set_sp(sp);
    t
}

/// [soft 0x08505ba0 pruneContainHier]: `(elem, op)`: goes over the children of `elem` and, according to the flag of the
/// element's own curve (word at data+0x34) and of the child's, drops sub-trees (`op` 2, or 0 under a flagged parent),
/// single elements (3) or the parent itself (1, then the walk goes on with the parent's replacement), and recurses into the
/// next sibling and first child of the element (of its replacement after op 1).
pub fn prune_contain_hier(m: &mut Mem, o: &mut dyn Outside, elem: u32, op: u32) {
    if elem == 0 {
        return;
    }
    let sp = m.sp();
    let slot = m.local(4);
    prune(m, o, elem, op, slot);
    m.set_sp(sp);
}

fn prune(m: &mut Mem, o: &mut dyn Outside, elem: u32, op: u32, slot: u32) {
    m.set_u32(slot, m.u32(elem + 0x10));
    let mut esi = elem;
    let parent_clear = flag(m, elem) == 0;
    loop {
        let cur = m.u32(slot);
        if cur == 0 {
            break;
        }
        let next = m.u32(cur + 8);
        let child_clear = flag(m, cur) == 0;
        if parent_clear {
            if child_clear {
                match op {
                    2 => {
                        let r = tree::remove_sub_tree(m, o, m.u32(cur + 0x14), slot, 0);
                        m.set_u32(slot, r);
                    }
                    1 => {
                        esi = tree::elem_destroy(m, o, cur + 0xc, 0);
                        break;
                    }
                    3 => {
                        let r = tree::elem_destroy(m, o, slot, 0);
                        m.set_u32(slot, r);
                    }
                    _ => m.set_u32(slot, next),
                }
            } else {
                m.set_u32(slot, next);
            }
        } else if child_clear {
            m.set_u32(slot, next);
        } else {
            match op {
                0 => {
                    let r = tree::remove_sub_tree(m, o, m.u32(cur + 0x14), slot, 0);
                    m.set_u32(slot, r);
                }
                3 => {
                    let r = tree::elem_destroy(m, o, slot, 0);
                    m.set_u32(slot, r);
                }
                _ => m.set_u32(slot, next),
            }
        }
    }
    if esi != 0 {
        let n = m.u32(esi + 8);
        if n != 0 {
            let s2 = m.local(4);
            prune(m, o, n, op, s2);
        }
        let c = m.u32(esi + 0x10);
        if c != 0 {
            let s2 = m.local(4);
            prune(m, o, c, op, s2);
        }
    }
}

/// [soft 0x0850588c addCompFromTree]: `(elem, comps, &count, box (12 words), old, op)`: appends to `comps` the composite
/// record (0x18 bytes: +0 whether the curve is hollow, +4 curve count 1, +8 the curve table) of every element of the tree, depth
/// first (children first when the element's curve flag is clear, siblings first otherwise). An element that already has a
/// slot in the old table (index at data+0x3c, not -1) hands that record over (the old slot is cleared); a new one gets the
/// curve: `buildHighDegree` of the composite when its order (clamped to 4) is above 2 and `SI_NO_HIGH_DEGREE_TRIM` is not set
/// (a failure, or a result `checkFitSelfInter` rejects, falls back to `fitFailed`, each with a message), otherwise
/// `fitFailed` directly. `op` is passed through unused here.
pub fn add_comp_from_tree(m: &mut Mem, o: &mut dyn Outside, elem: u32, comps: u32, pcount: u32, bx: [u32; 12], old: u32, op: u32) {
    if elem == 0 {
        return;
    }
    let d = tree::elem_get_data(m, elem);
    let idx = m.i32(d + 0x3c);
    let count = m.i32(pcount);
    let slot = comps.wrapping_add((count as u32).wrapping_mul(4));
    if idx == -1 {
        let p = m.si_calloc(1, 0x18);
        m.set_u32(slot, p);
        if p == 0 {
            return;
        }
        m.set_u16(p, (m.u16(d + 0x34) == 0) as u16);
        m.set_u32(p + 4, 1);
        let arr = m.si_calloc(m.u32(p + 4), 4);
        m.set_u32(p + 8, arr);
        let env = o.call(m, GETENV, &[ENV_NO_HD]).eax;
        if env == 0 {
            let mut order = m.i32(d + 0x38);
            if order > 4 {
                order = 4;
                m.set_i32(d + 0x38, 4);
            }
            if order > 2 {
                let f = |i: usize| f64::from_bits(bx[i] as u64 | (bx[i + 1] as u64) << 32);
                let r = crate::composite2::build_high_degree(m, o, d, [f(0), f(2), f(4), f(6), f(8), f(10)]);
                let arr = m.u32(p + 8);
                m.set_u32(arr, r);
                if r == 0 {
                    o.call(m, cp::SI_STATUS_DISPLAY_MESSAGE, &[MSG_HD_FAIL, 3]);
                    let c = cp::fit_failed(m, o, m.u32(d + 0x30));
                    let arr = m.u32(p + 8);
                    m.set_u32(arr, c);
                } else {
                    let sp = m.sp();
                    let bb = m.local(48);
                    for (i, w) in bx.iter().enumerate() {
                        m.set_u32(bb + 4 * i as u32, *w);
                    }
                    let bad = crate::curveinter2::check_fit_self_inter(m, o, r, bb) != 0;
                    m.set_sp(sp);
                    if bad {
                        o.call(m, cp::CRV_DISPOSE, &[m.u32(p + 8)]);
                        let c = cp::fit_failed(m, o, m.u32(d + 0x30));
                        let arr = m.u32(p + 8);
                        m.set_u32(arr, c);
                        o.call(m, cp::SI_STATUS_DISPLAY_MESSAGE, &[MSG_CUBIC_FAIL, 3]);
                    }
                }
                return add_tail(m, o, elem, comps, pcount, bx, old, op);
            }
        }
        let c = cp::fit_failed(m, o, m.u32(d + 0x30));
        let arr = m.u32(p + 8);
        m.set_u32(arr, c);
    } else {
        let os = old.wrapping_add((idx as u32).wrapping_mul(4));
        m.set_u32(slot, m.u32(os));
        m.set_u32(os, 0);
    }
    add_tail(m, o, elem, comps, pcount, bx, old, op)
}

#[allow(clippy::too_many_arguments)]
fn add_tail(m: &mut Mem, o: &mut dyn Outside, elem: u32, comps: u32, pcount: u32, bx: [u32; 12], old: u32, op: u32) {
    m.set_i32(pcount, m.i32(pcount).wrapping_add(1));
    let d = tree::elem_get_data(m, elem);
    let clear = m.u16(d + 0x34) == 0;
    let c = m.u32(elem + 0x10);
    let n = m.u32(elem + 8);
    if clear {
        if c != 0 {
            add_comp_from_tree(m, o, c, comps, pcount, bx, old, op);
        }
        let n = m.u32(elem + 8);
        if n != 0 {
            add_comp_from_tree(m, o, n, comps, pcount, bx, old, op);
        }
    } else {
        if n != 0 {
            add_comp_from_tree(m, o, n, comps, pcount, bx, old, op);
        }
        let c = m.u32(elem + 0x10);
        if c != 0 {
            add_comp_from_tree(m, o, c, comps, pcount, bx, old, op);
        }
    }
}

/// [soft 0x08504130 si_nurbsCompCurveCreateSrfComps]: `(list_a, list_b, &comps, op, box (12 words), old, trim)`: builds
/// the containment tree of the two lists, prunes it by `op`, drops the top elements whose curve is flagged, and (a single
/// element left whose curve lies on the surface border) that one too; then lays the remaining elements out as composite records
/// ([`add_comp_from_tree`], `op` taken from trim+0xfc) in `*comps` and keeps those `si_validTrimCurve` accepts: a lone valid
/// record is dropped again (count 0), several are packed into a new array. Answers the count (0 on an allocation failure).
#[allow(clippy::too_many_arguments)]
pub fn create_srf_comps(m: &mut Mem, o: &mut dyn Outside, list_a: u32, list_b: u32, pcomps: u32, op: u32, bx: [u32; 12], old: u32, trim: u32) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = create_srf_comps_in(m, o, list_a, list_b, pcomps, op, bx, old, trim);
    m.set_sp(sp);
    r
}

#[allow(clippy::too_many_arguments)]
fn create_srf_comps_in(m: &mut Mem, o: &mut dyn Outside, list_a: u32, list_b: u32, pcomps: u32, op: u32, bx: [u32; 12], old: u32, trim: u32) -> Result<u32, Halt> {
    let ptree = m.local(4);
    let cur = m.local(4);
    let n = m.local(4);
    let t = build_contain_hier(m, o, list_a, list_b);
    m.set_u32(ptree, t);
    if t == 0 {
        return Ok(0);
    }
    prune_contain_hier(m, o, m.u32(t), op);
    m.set_u32(cur, m.u32(t));
    while m.u32(cur) != 0 {
        let c = m.u32(cur);
        if flag(m, c) == 0 {
            m.set_u32(cur, m.u32(c + 8));
        } else {
            let r = tree::elem_destroy(m, o, cur, 0);
            m.set_u32(cur, r);
        }
    }
    if m.i32(t + 0xc) == 1 {
        let root = m.u32(t);
        let d = tree::elem_get_data(m, root);
        if cp::srf_crv_on_border(m, m.u32(trim), m.u32(d + 0x30)) as u16 != 0 {
            tree::elem_destroy(m, o, t, 0); // the tree's first word is the root slot
        }
    }
    let t = m.u32(ptree);
    let comps = m.si_calloc(m.u32(t + 0xc), 4);
    m.set_u32(pcomps, comps);
    if comps == 0 {
        return Ok(0);
    }
    m.set_i32(n, 0);
    let opx = m.u32(trim + 0xfc);
    add_comp_from_tree(m, o, m.u32(t), comps, n, bx, old, opx);
    let cnt = m.i32(n);
    let mut nvalid = 0i32;
    let mut i = 0i32;
    while i < cnt {
        let s = comps.wrapping_add((i as u32).wrapping_mul(4));
        let c = m.u32(s);
        if c != 0 {
            let mut va = [0u32; 13];
            va[..12].copy_from_slice(&bx);
            va[12] = c;
            if o.call(m, VALID_TRIM_CURVE, &va).eax as u16 != 0 {
                nvalid += 1;
            } else {
                m.free(c);
                m.set_u32(s, 0);
            }
        }
        i += 1;
    }
    let mut ret = cnt;
    if nvalid != cnt {
        if nvalid > 1 {
            let nw = m.si_calloc(nvalid as u32, 4);
            if nw == 0 {
                return Ok(0);
            }
            let mut j = 0u32;
            for i in 0..cnt as u32 {
                let c = m.u32(comps + 4 * i);
                if c != 0 {
                    m.set_u32(nw + 4 * j, c);
                    j += 1;
                }
            }
            m.free(comps);
            m.set_u32(pcomps, nw);
            ret = nvalid;
        } else if nvalid == 1 {
            for i in 0..cnt as u32 {
                let c = m.u32(comps + 4 * i);
                if c != 0 {
                    m.free(c);
                    m.set_u32(comps + 4 * i, 0);
                    break;
                }
            }
            ret = 0;
        }
    }
    tree::destroy(m, o, ptree, 0);
    Ok(ret as u32)
}

/// [soft 0x08506a84 rebuildComp]: `(srf, tree)`: replaces the trim composites of the surface by the ones the tree describes
/// (the old table, srf+0x64, handed to [`add_comp_from_tree`] with op 1; the box is the surface's parameter range): the old
/// table is disposed with `SI_nurbsSrfTrmDispose`, then the count (+0x60) and the new table (+0x64) are stored, the flag bytes
/// at +0x5b get 0x20 and 0x40 and the word at +0x5c is cleared.
pub fn rebuild_comp(m: &mut Mem, o: &mut dyn Outside, srf: u32, t: u32) {
    let sp = m.sp();
    let pcnt = m.local(4);
    let pslot = m.local(4);
    let bxw: [f64; 6] = [m.f64(srf + 0x10), m.f64(srf + 0x34), 0.0, m.f64(srf + 0x18), m.f64(srf + 0x3c), 0.0];
    let mut bx = [0u32; 12];
    for (i, v) in bxw.iter().enumerate() {
        bx[2 * i] = v.to_bits() as u32;
        bx[2 * i + 1] = (v.to_bits() >> 32) as u32;
    }
    m.set_i32(pcnt, 0);
    let comps = m.si_calloc(m.u32(t + 0xc), 4);
    if comps != 0 {
        add_comp_from_tree(m, o, m.u32(t), comps, pcnt, bx, m.u32(srf + 0x64), 1);
    }
    m.set_u32(pslot, srf);
    o.call(m, SRF_TRM_DISPOSE, &[pslot]);
    let srf = m.u32(pslot);
    m.set_u16(srf + 0x5c, 0);
    m.set_u8(srf + 0x5b, m.u8(srf + 0x5b) | 0x20);
    m.set_u8(srf + 0x5b, m.u8(srf + 0x5b) | 0x40);
    m.set_u32(srf + 0x60, m.u32(pcnt));
    m.set_u32(srf + 0x64, comps);
    m.set_sp(sp);
}

/// [soft 0x0850687c SI_nurbsCompositeCurveRebuild]: `(srf, curves)`: makes a composite record for every non-border linear trim
/// curve (flag: the curve's composite is hollow), inserts them into a containment tree, adds the one border curve (if any) as the
/// outermost element, prunes the tree (op 3) and [`rebuild_comp`]s the surface. Answers 1 (0 when a record cannot be made).
pub fn composite_curve_rebuild(m: &mut Mem, o: &mut dyn Outside, srf: u32, crvs: u32) -> u32 {
    let sp = m.sp();
    let r = rebuild_in(m, o, srf, crvs);
    m.set_sp(sp);
    r
}

fn rebuild_in(m: &mut Mem, o: &mut dyn Outside, srf: u32, crvs: u32) -> u32 {
    let ptree = m.local(4);
    let pe = m.local(4);
    let mut border: i32 = -1;
    let t = tree::create(m);
    m.set_u32(ptree, t);
    if t == 0 {
        return 0;
    }
    let boxw = [0u32; 12];
    let hollow = |m: &Mem, i: i32| (m.u16(m.u32(m.u32(srf + 0x64).wrapping_add((i as u32).wrapping_mul(4)))) == 0) as u32;
    let mut i = 0i32;
    while i < m.i32(srf + 0x60) {
        let slot = crvs.wrapping_add((i as u32).wrapping_mul(4));
        let c = m.u32(slot);
        if c != 0 {
            if cp::srf_crv_on_border(m, srf, c) as u16 == 0 {
                let comp = cp::composite_curve_create(m, c, boxw, hollow(m, i));
                if comp == 0 {
                    tree::destroy(m, o, ptree, COMP_DESTROY);
                    return 0;
                }
                m.set_u32(comp + 0x38, m.u32(c));
                m.set_u32(comp + 0x3c, m.u32(c + 0x38));
                tree::insert_s(m, o, m.u32(ptree), comp, CURVE_IN_CURVE);
                m.set_u32(slot, 0);
            } else {
                border = i;
            }
        }
        i += 1;
    }
    let t = m.u32(ptree);
    if m.u32(t) != 0 {
        if border >= 0 {
            let slot = crvs.wrapping_add((border as u32).wrapping_mul(4));
            let c = m.u32(slot);
            let comp = cp::composite_curve_create(m, c, boxw, hollow(m, border));
            if comp == 0 {
                tree::destroy(m, o, ptree, COMP_DESTROY);
                return 0;
            }
            m.set_u32(comp + 0x38, m.u32(c));
            m.set_u32(comp + 0x3c, m.u32(c + 0x38));
            let e = tree::insert_s(m, o, t, comp, CURVE_IN_CURVE);
            m.set_u32(pe, e);
            if m.u32(m.u32(t) + 8) != 0 {
                tree::elem_destroy(m, o, pe, 0);
            }
            m.set_u32(slot, 0);
        }
        prune_contain_hier(m, o, m.u32(t), 3);
    }
    rebuild_comp(m, o, srf, m.u32(ptree));
    tree::destroy(m, o, ptree, COMP_DESTROY);
    1
}

/// [soft 0x0850679c SI_nurbsNoBorderTrimDelete]: `(srf, curves, index)`: deletes trim curve number `index` from the surface:
/// a curve that is a piece of another (index at +0x50 below 0) is marked out of the linearised trim table (`ptsMarkForDelete`,
/// `linearTrimCrvsAdjust`), otherwise the table entry it names is disposed; the surface is rebuilt from the table
/// ([`composite_curve_rebuild`]) and the table disposed. Answers the rebuild's 16-bit result sign-extended.
pub fn no_border_trim_delete(m: &mut Mem, o: &mut dyn Outside, srf: u32, crvs: u32, idx: u16) -> u32 {
    let sp = m.sp();
    let r = nbtd(m, o, srf, crvs, idx);
    m.set_sp(sp);
    r
}

fn nbtd(m: &mut Mem, o: &mut dyn Outside, srf: u32, crvs: u32, idx: u16) -> u32 {
    let plist = m.local(4);
    let bb = m.local(48);
    let arr = cp::nurbs_srf_linearize_trim_crvs(m, o, srf);
    let c = m.u32(crvs.wrapping_add((idx as i16 as i32 as u32).wrapping_mul(4)));
    let k = m.i32(c + 0x50);
    if k < 0 {
        for (off, from) in [(0u32, 0x10u32), (8, 0x34), (0x18, 0x18), (0x20, 0x3c)] {
            m.set_f64(bb + off, m.f64(srf + from));
        }
        let list = crate::composite2::pts_mark_for_delete(m, o, arr, c, bb);
        m.set_u32(plist, list);
        cp::linear_trim_crvs_adjust(m, o, arr, list, bb);
        genlist::destroy(m, o, plist, INDEX_DESTROY);
    } else {
        o.call(m, cp::CRV_DISPOSE, &[arr.wrapping_add((k as u32).wrapping_mul(4))]);
    }
    let r = composite_curve_rebuild(m, o, srf, arr) as u16;
    let mut i = 0i32;
    while i < m.i32(srf + 0x60) {
        o.call(m, cp::CRV_DISPOSE, &[arr.wrapping_add((i as u32).wrapping_mul(4))]);
        i += 1;
    }
    if arr != 0 {
        m.free(arr);
    }
    r as i16 as i32 as u32
}

const ENV_NO_HD_PROJ: u32 = 0x086d_af2f;
const MSG_NO_AREA: u32 = 0x086d_af60;
const MSG_PROJ_FAIL: u32 = 0x086d_afa0;

fn pair(m: &mut Mem, o: u32, v: f64) {
    m.set_f64(o, v);
}

/// [soft 0x085043c8 si_nurbsCompCurveCreateSrfCompsProj]: `(&comps, old, item)`: the composite records of a projected curve
/// item (`item`: +0 the surface, whose box is at +0x10 / +0x34 / +0x18 / +0x3c and whose count of existing records is at +0x68;
/// +8 a record with the order at +0 and a word at +0x34 handed to every curve; +0x20 the list of the curves). The curves are
/// walked in groups, a group ending at a curve with bit 0x10; a group whose last curve has bits 0x18 gets the box of all its
/// points, and when that box has no area (under 1e-6) and both its corners lie on the surface's box the group is dropped with a
/// message. Every other group becomes a record (+4 curve count, +8 the curve table: each curve fitted at an order of at most 4
/// when that is above 2 and `SI_NO_HIGH_DEGREE_TRIM` is unset, else copied; the first curve of a single-curve group whose first
/// point is off the box is marked closed in its +0x20), and the records go on a list. The answer table (`*comps`) gets the
/// old records first (the old table is emptied) and the new ones after; answers its length, -1 when a record or the list
/// cannot be made, 0 when the table cannot.
pub fn create_srf_comps_proj(m: &mut Mem, o: &mut dyn Outside, pout: u32, old: u32, item: u32) -> u32 {
    let sp = m.sp();
    let r = proj_in(m, o, pout, old, item);
    m.set_sp(sp);
    r
}

fn proj_in(m: &mut Mem, o: &mut dyn Outside, pout: u32, old: u32, item: u32) -> u32 {
    use crate::composite::{composite_curve_point_on_border as on_border, SI_NURBS_CURVE_INTER_UPDATE_BBOX as UPD};
    use si_x87::F80;
    let plist = m.local(4);
    let fl = m.local(4);
    let bx = m.local(48);
    let list = genlist::create(m);
    m.set_u32(plist, list);
    if list == 0 {
        return 0xffff_ffff;
    }
    let srf = m.u32(item);
    for (off, from) in [(0u32, 0x10u32), (8, 0x34), (0x18, 0x18), (0x20, 0x3c)] {
        pair(m, bx + off, m.f64(srf + from));
    }
    let b6 = |m: &Mem| [m.f64(bx), m.f64(bx + 8), m.f64(bx + 0x10), m.f64(bx + 0x18), m.f64(bx + 0x20), m.f64(bx + 0x28)];
    let rec0 = m.u32(item + 8);
    let mut order = m.i32(rec0);
    if order > 4 {
        order = 4;
    }
    let env = o.call(m, GETENV, &[ENV_NO_HD_PROJ]).eax;
    let src = m.u32(item + 0x20);
    let mut e = genlist::iter_init(m, src, fl, fl + 2);
    if m.i16(fl) != 1 {
        loop {
            let mut d = genlist::elem_get_data(m, e);
            m.set_u32(d + 0x34, m.u32(rec0 + 0x34));
            let mut count = 1i32;
            let saved = e;
            let mut ok = true;
            if m.u32(d + 0x38) & 8 != 0 {
                if m.u32(d + 0x38) & 0x10 == 0 && m.i16(fl) != 1 {
                    loop {
                        count += 1;
                        e = genlist::iter_next(m, src, fl, fl + 2);
                        d = genlist::elem_get_data(m, e);
                        m.set_u32(d + 0x34, m.u32(rec0 + 0x34));
                        if m.u32(d + 0x38) & 0x10 != 0 || m.i16(fl) == 1 {
                            break;
                        }
                    }
                }
                if m.u32(d + 0x38) & 0x18 == 0x18 {
                    let sp2 = m.sp();
                    let bb = m.local(48);
                    let out = m.local(48);
                    let pt = m.local(24);
                    for (i, v) in [f32::MAX as f64, f32::MAX as f64].iter().enumerate() {
                        pair(m, bb + 8 * i as u32, *v);
                    }
                    for (i, v) in [-(f32::MAX as f64), -(f32::MAX as f64)].iter().enumerate() {
                        pair(m, bb + 0x18 + 8 * i as u32, *v);
                    }
                    let n = m.i32(d + 4);
                    let mut i = 0i32;
                    let mut at = 0u32;
                    while i < n {
                        let p = m.u32(d + 0x2c);
                        let (x, y) = (m.f64(p.wrapping_add(at)), m.f64(p.wrapping_add(at).wrapping_add(8)));
                        pair(m, pt, x);
                        pair(m, pt + 8, y);
                        let mut args = [0u32; 19];
                        args[0] = out;
                        for w in 0..12 {
                            args[1 + w] = m.u32(bb + 4 * w as u32);
                        }
                        for w in 0..6 {
                            args[13 + w] = m.u32(pt + 4 * w as u32);
                        }
                        o.call(m, UPD, &args);
                        for w in 0..12 {
                            let v = m.u32(out + 4 * w as u32);
                            m.set_u32(bb + 4 * w as u32, v);
                        }
                        i += 1;
                        at = at.wrapping_add(0x24);
                    }
                    let dx = (F80::from_f64(m.f64(bb + 0x18)) - F80::from_f64(m.f64(bb))).abs();
                    let dy = (F80::from_f64(m.f64(bb + 0x20)) - F80::from_f64(m.f64(bb + 8))).abs();
                    if (dx * dy).lt(F80::from_f64(1.0e-6)) {
                        let b = b6(m);
                        let off = on_border(m.f64(bb), m.f64(bb + 8), b) as u16 == 0 || on_border(m.f64(bb + 0x18), m.f64(bb + 0x20), b) as u16 == 0;
                        if off {
                            ok = false;
                            o.call(m, cp::SI_STATUS_DISPLAY_MESSAGE, &[MSG_NO_AREA, 2]);
                        }
                    }
                    m.set_sp(sp2);
                }
            }
            if ok {
                let rec = m.si_calloc(1, 0x18);
                if rec == 0 {
                    return 0xffff_ffff;
                }
                m.set_i32(rec + 4, count);
                let arr = m.si_calloc(m.u32(rec + 4), 4);
                m.set_u32(rec + 8, arr);
                genlist::iter_set(m, src, saved, fl, fl + 2);
                e = saved;
                let mut idx = 0u32;
                let out = m.local(0x24);
                loop {
                    let d = genlist::elem_get_data(m, e);
                    let p = m.u32(d + 0x2c);
                    let (x, y) = (m.f64(p), m.f64(p + 8));
                    if m.i32(rec + 4) == 1 && on_border(x, y, b6(m)) as u16 == 0 {
                        m.set_u16(d + 0x20, 1);
                    }
                    cp::rebuild_knot_vector(m, o, d, out, order);
                    let slot = m.u32(rec + 8).wrapping_add(idx.wrapping_mul(4));
                    if env == 0 && order > 2 {
                        let angle = m.i32(crate::composite2::NURBS_ANGLE_DISCONTINUITY) as f64;
                        let tol = (F80::from_f32(m.f32(crate::composite2::NURBS_ERROR_TOLERANCE)) / F80::from_f64(10.0)).to_f64();
                        let args = [d, order as u32, tol.to_bits() as u32, (tol.to_bits() >> 32) as u32, angle.to_bits() as u32, (angle.to_bits() >> 32) as u32];
                        let fit = o.call(m, crate::composite2::SI_FIT_NURBS_CURVE, &args).eax;
                        m.set_u32(slot, fit);
                        if fit == 0 {
                            o.call(m, cp::SI_STATUS_DISPLAY_MESSAGE, &[MSG_PROJ_FAIL, 3]);
                            let c = o.call(m, cp::SI_NURBS_CRV_COPY, &[d]).eax;
                            let slot = m.u32(rec + 8).wrapping_add(idx.wrapping_mul(4));
                            m.set_u32(slot, c);
                        }
                    } else {
                        let c = o.call(m, cp::SI_NURBS_CRV_COPY, &[d]).eax;
                        let slot = m.u32(rec + 8).wrapping_add(idx.wrapping_mul(4));
                        m.set_u32(slot, c);
                    }
                    if m.u32(d + 0x38) & 0x10 != 0 {
                        break;
                    }
                    e = genlist::iter_next(m, src, fl, fl + 2);
                    idx += 1;
                    let kp = m.u32(out + 0xc);
                    if kp != 0 {
                        m.free(kp);
                        m.set_u32(out + 0xc, 0);
                    }
                    if m.i16(fl) == 1 {
                        break;
                    }
                }
                genlist::insert_e(m, m.u32(plist), rec);
            }
            e = genlist::iter_next(m, src, fl, fl + 2);
            if m.i16(fl) == 1 {
                break;
            }
        }
    }
    let list = m.u32(plist);
    let total = m.i32(m.u32(item) + 0x68).wrapping_add(m.i32(list + 0x10));
    let arr = m.si_calloc(total as u32, 4);
    m.set_u32(pout, arr);
    if arr == 0 {
        return 0;
    }
    let mut b = 0i32;
    while b < m.i32(m.u32(item) + 0x68) {
        let s = old.wrapping_add((b as u32).wrapping_mul(4));
        m.set_u32(arr.wrapping_add((b as u32).wrapping_mul(4)), m.u32(s));
        m.set_u32(s, 0);
        b += 1;
    }
    let mut e = genlist::iter_init(m, list, fl, fl + 2);
    if m.i16(fl) != 1 {
        loop {
            let d = genlist::elem_get_data(m, e);
            m.set_u32(m.u32(pout).wrapping_add((b as u32).wrapping_mul(4)), d);
            e = genlist::iter_next(m, list, fl, fl + 2);
            b += 1;
            if m.i16(fl) == 1 {
                break;
            }
        }
    }
    genlist::destroy(m, o, plist, 0);
    b as u32
}

/// [soft 0x08505344 si_nurbsCompCurveInCurveExt]: `(a, b)` (records with the polyline at +0x20, its point count at +4 and points
/// at +0x2c, 0x24 bytes each; and four reference records at +0x10, +0x14, +0x18, +0x1c with a pair of doubles at +0x30 / +0x38):
/// 0 when either is missing or has no points. `b`'s polyline goes to a `calloc(count, 0x18)` array of (x, y, 0) points; for each
/// reference in turn the point `A` is the first point of `a`'s polyline that is not within 1e-4 in x and y of the reference
/// `B` = (x, y, 0) (the last one when none is; z is always 0), and `si_nurbsCurveOrientPointInCurve(array, count, B, A, (0, 0, 1))`
/// (outside) decides. The first three answers (16-bit) must be non-zero, else the array is freed and 0 answered; the fourth
/// is the result, sign-extended.
pub fn comp_curve_in_curve_ext(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> u32 {
    use crate::composite::SI_NURBS_CURVE_ORIENT_POINT_IN_CURVE as ORIENT;
    use si_x87::F80;
    if a == 0 {
        return 0;
    }
    let ac = m.u32(a + 0x20);
    if m.i32(ac + 4) <= 0 || b == 0 {
        return 0;
    }
    let bc = m.u32(b + 0x20);
    if m.i32(bc + 4) == 0 {
        return 0;
    }
    let p0 = m.u32(ac + 0x2c);
    let mut pa = [m.f64(p0), m.f64(p0 + 8), 0.0];
    let n = m.i32(bc + 4);
    let arr = m.si_calloc(n as u32, 0x18);
    if arr == 0 {
        return 0;
    }
    let mut k = 0u32;
    while (k as i32) < n {
        let src = m.u32(m.u32(b + 0x20) + 0x2c).wrapping_add(k.wrapping_mul(0x24));
        let dst = arr.wrapping_add(k.wrapping_mul(0x18));
        m.set_f64(dst, m.f64(src));
        m.set_f64(dst + 8, m.f64(src + 8));
        m.set_u32(dst + 0x10, 0);
        m.set_u32(dst + 0x14, 0);
        k += 1;
    }
    let eps = F80::from_f64(1.0e-4);
    let mut r = 0u32;
    for which in 0..4u32 {
        let rf = m.u32(a + 0x10 + 4 * which);
        let (bxv, byv) = (m.f64(rf + 0x30), m.f64(rf + 0x38));
        let acv = m.u32(a + 0x20);
        let mut i = 0i32;
        let mut at = 0u32;
        while i < m.i32(acv + 4) {
            let q = m.u32(acv + 0x2c).wrapping_add(at);
            let (px, py) = (m.f64(q), m.f64(q + 8));
            pa = [px, py, 0.0];
            if !(F80::from_f64(px) - F80::from_f64(bxv)).abs().lt(eps) || !(F80::from_f64(py) - F80::from_f64(byv)).abs().lt(eps) {
                break;
            }
            at = at.wrapping_add(0x24);
            i += 1;
        }
        let mut args = vec![arr, n as u32];
        for p in [[bxv, byv, 0.0], pa, [0.0, 0.0, 1.0]] {
            for v in p {
                args.push(v.to_bits() as u32);
                args.push((v.to_bits() >> 32) as u32);
            }
        }
        r = o.call(m, ORIENT, &args).eax;
        if which < 3 && r as u16 == 0 {
            m.free(arr);
            return 0;
        }
    }
    m.free(arr);
    r as u16 as i16 as i32 as u32
}
