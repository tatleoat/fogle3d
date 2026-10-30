// SPDX-License-Identifier: MIT
//! The original's trim preview list (finding 0044, unit trim7, step 3): `si_nurbsTrimBuildListFromPreview` and
//! `si_nurbsInterBuildLinearCrvCB`, which turn the intersection chains of the curve-orient records into linear curves.

use crate::composite3::{gentree as tree, Own};
use crate::image::Halt;
use crate::inter::genlist;
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_points_allocate};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// `si_nurbsCompCurveInCurveExt (0x08505344)`: the compare of the second tree of the preview.
pub const COMP_CURVE_IN_CURVE_EXT: u32 = 0x0850_5344;
/// `si_nurbsCurveOrientMarkBCross (0x0850a720)` (curveOrient.c, not ported here): `(orient, trim)`.
pub const MARK_B_CROSS: Routine = Routine::new("si_nurbsCurveOrientMarkBCross", 0x0850_a720);
/// 0x08662058: the distance under which an intersection sits on the border (1e-4).
const NEAR: f64 = 1.0e-4;

fn x(v: f64) -> F80 {
    F80::from_f64(v)
}

/// Whether `|a - b|` (measured in the register) is under the tolerance.
fn near(a: f64, b: f64) -> bool {
    (x(a) - x(b)).abs().lt(x(NEAR))
}

/// [soft 0x0850d5b4 si_nurbsInterBuildLinearCrvCB]: `(first, last, n, trim)`: a linear order 2 curve for the stretch of the
/// intersection chain (links at +0x74) from `first` to `last`, which is `n` nodes long. The ends are moved onto the border
/// their flags (+0x50) name (`MoveInterToBorder`, twice); the curve has `n` points, one less when the start did not move
/// onto the border and `first` has a successor at +0x70, one more when the end moved onto a border. The first and last points are the moved ends (the start's u / v are replaced by the surface's border value when its
/// flags (0x10 / 0x20 for u, 0x40 / 0x80 for v) name one), those between come from the chain (skipping `first`
/// when the start moved). Knots are the intersections' distances (+0x64), the point flag (+0x20) their +0x6c. 0 when the
/// curve cannot be allocated.
pub fn inter_build_linear_crv_cb(m: &mut Mem, p1: u32, p2: u32, n: i32, trim: u32) -> u32 {
    let sp = m.sp();
    let r = build_cb(m, p1, p2, n, trim);
    m.set_sp(sp);
    r
}

fn build_cb(m: &mut Mem, p1: u32, p2: u32, n: i32, trim: u32) -> u32 {
    let mut b12 = true;
    let mut b3 = true;
    let c = nurbs_crv_allocate(m).addr();
    if c == 0 {
        return 0;
    }
    let out = m.local(24);
    crate::trim::move_inter_to_border(m, out, trim, p1);
    let (mut lu, mut lv) = (m.f64(out), m.f64(out + 8));
    crate::trim::move_inter_to_border(m, out, trim, p2);
    let (eu, ev) = (m.f64(out), m.f64(out + 8));
    let srf = m.u32(trim);
    let f1 = m.u32(p1 + 0x50);
    let n1 = m.u32(p1 + 0x74);
    if f1 & 0x10 == 0 {
        if f1 & 0x20 != 0 {
            lu = m.f64(srf + 0x10);
            b12 = !near(m.f64(n1 + 0x30), lu);
        }
    } else {
        lu = m.f64(srf + 0x18);
        b12 = !near(m.f64(n1 + 0x30), lu);
    }
    if f1 & 0x40 == 0 {
        if f1 & 0x80 != 0 {
            lv = m.f64(srf + 0x34);
            if near(m.f64(n1 + 0x38), lv) {
                b12 = false;
            }
        } else if f1 & 1 != 0 {
            if near(m.f64(p1 + 0x30), m.f64(srf + 0x10)) {
                b12 = false;
            }
        } else if f1 & 2 != 0 && near(m.f64(p1 + 0x30), m.f64(srf + 0x18)) {
            b12 = false;
        }
    } else {
        lv = m.f64(srf + 0x3c);
        if near(m.f64(n1 + 0x38), lv) {
            b12 = false;
        }
    }
    if f1 & 4 != 0 {
        if near(m.f64(p1 + 0x38), m.f64(srf + 0x34)) {
            b12 = false;
        }
    } else if f1 & 8 != 0 && near(m.f64(p1 + 0x38), m.f64(srf + 0x3c)) {
        b12 = false;
    }
    let f2 = m.u32(p2 + 0x50);
    if f2 & 0x11 != 0 {
        if near(m.f64(p2 + 0x30), m.f64(srf + 0x10)) {
            b3 = false;
        }
    } else if f2 & 0x22 != 0 && near(m.f64(p2 + 0x30), m.f64(srf + 0x18)) {
        b3 = false;
    }
    if f2 & 0x44 != 0 {
        if near(m.f64(p2 + 0x38), m.f64(srf + 0x34)) {
            b3 = false;
        }
    } else if f2 & 0x88 != 0 && near(m.f64(p2 + 0x38), m.f64(srf + 0x3c)) {
        b3 = false;
    }
    let mut cnt = n;
    if !b12 && m.u32(p1 + 0x70) != 0 {
        cnt = cnt.wrapping_sub(1);
    }
    if b3 {
        cnt = cnt.wrapping_add(1);
    }
    m.set_i32(c + 8, cnt);
    let k = knots_allocate(m, cnt);
    m.set_u32(c + 0xc, k);
    let first_k = m.u64(k);
    m.set_u64(c + 0x10, first_k);
    let last_k = m.u64(k.wrapping_add((m.i32(c + 8).wrapping_sub(1) as u32).wrapping_mul(8)));
    m.set_u64(c + 0x18, last_k);
    let pts = nurbs_points_allocate(m, cnt);
    m.set_u32(c + 0x2c, pts);
    m.set_i32(c + 4, cnt);
    m.set_i32(c, 2);
    m.set_i32(c + 0x34, 1);
    let mut p = p1;
    if b12 {
        if m.u32(p1 + 0x70) != 0 {
            p = n1;
        }
    } else {
        if m.u32(p1 + 0x70) != 0 {
            p = n1;
        }
        p = m.u32(p + 0x74);
    }
    let mut off = 0u32;
    let mut i = 0i32;
    while i < cnt {
        let pts = m.u32(c + 0x2c);
        let ks = m.u32(c + 0xc);
        if i == 0 {
            m.set_f64(pts, lu);
            m.set_f64(pts + 8, lv);
            let kv = m.u64(p1 + 0x64);
            m.set_u64(ks, kv);
            let fl = m.u32(p1 + 0x6c);
            m.set_u32(pts + 0x20, fl);
        } else if i == cnt - 1 {
            m.set_f64(pts.wrapping_add(off), eu);
            m.set_f64(pts.wrapping_add(8).wrapping_add(off), ev);
            let kv = m.u64(p2 + 0x64);
            m.set_u64(ks.wrapping_add((i as u32).wrapping_mul(8)), kv);
            let fl = m.u32(p2 + 0x6c);
            m.set_u32(pts.wrapping_add(0x20).wrapping_add(off), fl);
        } else {
            let (u, v, kv, fl) = (m.u64(p + 0x30), m.u64(p + 0x38), m.u64(p + 0x64), m.u32(p + 0x6c));
            m.set_u64(pts.wrapping_add(off), u);
            m.set_u64(pts.wrapping_add(8).wrapping_add(off), v);
            m.set_u64(ks.wrapping_add((i as u32).wrapping_mul(8)), kv);
            m.set_u32(pts.wrapping_add(0x20).wrapping_add(off), fl);
            p = m.u32(p + 0x74);
        }
        off = off.wrapping_add(0x24);
        i += 1;
    }
    let ks = m.u32(c + 0xc);
    let first_k = m.u64(ks);
    m.set_u64(c + 0x10, first_k);
    let last_k = m.u64(ks.wrapping_add((m.i32(c + 8).wrapping_sub(1) as u32).wrapping_mul(8)));
    m.set_u64(c + 0x18, last_k);
    c
}

/// [soft 0x08510608 si_nurbsTrimBuildListFromPreview]: `(trim)`: the list (made at trim+0x20) of linear curves of the
/// curve-orient records of the preview list (trim+0x14); 0 when there is no list or a list / tree cannot be made.
/// With trim+0xfc = 1 a tree is made at trim+0x34 first; with 0, `MarkBCross` is run on each record that is not
/// finished (bit 0 of its +0x24). A record with bits 0xc at +0x25 is cut into one curve per stretch between the chain
/// nodes with a border bit (+0x51 & 0xf), each flagged (+0x38) 2 (trim+0x38 clear) or 4, the first 8; a chain that ends
/// before the next border node flags the last curve built 0x10 (that register is the caller's when none was built:
/// the port gives up then). Any other record becomes one curve (the plain `inter_build_linear_crv` of the chain, entered
/// in the tree under `CompCurveInCurveExt`, when its +0xc is non-zero), flagged 2 / 4 and 0x18.
pub fn build_list_from_preview(m: &mut Mem, o: &mut dyn Outside, trim: u32) -> Result<u32, Halt> {
    let sp = m.sp();
    let r = build_list_in(m, o, trim);
    m.set_sp(sp);
    r
}

fn build_list_in(m: &mut Mem, o: &mut dyn Outside, trim: u32) -> Result<u32, Halt> {
    let src = m.u32(trim + 0x14);
    if src == 0 {
        return Ok(0);
    }
    let fl = m.local(4);
    let (first, last) = (fl, fl + 2);
    let out = genlist::create(m);
    m.set_u32(trim + 0x20, out);
    if out == 0 {
        return Ok(0);
    }
    if m.u32(trim + 0xfc) == 1 {
        let t = tree::create(m);
        m.set_u32(trim + 0x34, t);
        if t == 0 {
            return Ok(0);
        }
    }
    let mut edi: u32 = 0;
    let mut e = genlist::iter_init(m, src, first, last);
    let mark = |m: &mut Mem, c: u32, f: u32| {
        let v = m.u8(c + 0x38) | f as u8;
        m.set_u8(c + 0x38, v);
    };
    while m.i16(first) != 1 {
        let mut b2 = true;
        let co = genlist::elem_get_data(m, e);
        if (m.u8(co + 0x24) ^ 1) & 1 != 0 {
            if m.u32(trim + 0xfc) == 0 {
                o.call(m, MARK_B_CROSS, &[co, trim]);
            }
            if m.u8(co + 0x25) & 0xc != 0 {
                let mut cnt = m.i32(co + 8);
                let mut p8 = m.u32(co);
                if m.i16(co + 0xc) != 0 {
                    loop {
                        if m.u8(p8 + 0x51) & 0xc != 0 {
                            break;
                        }
                        p8 = m.u32(p8 + 0x74);
                        if p8 == m.u32(co) {
                            break;
                        }
                    }
                }
                'chain: loop {
                    let mut n = 1i32;
                    let mut p9 = p8;
                    loop {
                        if p9 == 0 || cnt < 1 {
                            break;
                        }
                        p9 = m.u32(p9 + 0x74);
                        if p9 == 0 {
                            break;
                        }
                        cnt -= 1;
                        n += 1;
                        if m.u8(p9 + 0x51) & 0xf != 0 {
                            edi = inter_build_linear_crv_cb(m, p8, p9, n, trim);
                            p8 = p9;
                            if edi != 0 {
                                mark(m, edi, if m.u32(trim + 0x38) == 0 { 2 } else { 4 });
                                if b2 {
                                    b2 = false;
                                    mark(m, edi, 8);
                                }
                                genlist::insert_e(m, out, edi);
                            }
                            continue 'chain;
                        }
                    }
                    // the chain ran out: flag the curve built last
                    if edi == 0 {
                        return Err(Halt::Refused("BuildListFromPreview: no curve built yet"));
                    }
                    mark(m, edi, 0x10);
                    break 'chain;
                }
            } else {
                if m.i16(co + 0xc) == 0 {
                    edi = inter_build_linear_crv_cb(m, m.u32(co), m.u32(co + 4), m.i32(co + 8), trim);
                } else {
                    edi = crate::inter::inter_build_linear_crv(m, m.u32(co), m.i32(co + 8), m.i16(co + 0xc));
                    m.set_u32(co + 0x20, edi);
                    let t = m.u32(trim + 0x34);
                    tree::insert_s(m, &mut Own(o), t, co, COMP_CURVE_IN_CURVE_EXT);
                }
                if edi != 0 {
                    mark(m, edi, if m.u32(trim + 0x38) == 0 { 2 } else { 4 });
                    genlist::insert_e(m, out, edi);
                    mark(m, edi, 0x18);
                }
            }
        }
        e = genlist::iter_next(m, src, first, last);
    }
    let _ = e;
    Ok(out)
}
