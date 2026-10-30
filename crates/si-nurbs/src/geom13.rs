// SPDX-License-Identifier: MIT
//! nurbs13: the skin conversions of `cross.c` on the memory image (finding 0025, section 33).

use crate::image::{crv_apply_matrix, crv_copy};
use si_hrc::image::node::model_allocate;
use si_hrc::image::nurbs::{knots_allocate, nurbs_crv_allocate, nurbs_crv_dispose, nurbs_points_allocate, nurbs_srf_allocate};
use si_mem::{Mem, Outside, Routine};
use si_x87::{x, F80};

/// `SI_modelDispose (0x08461920)`, answered by the caller's world (it lives in `si-model`, which this crate does not use).
pub const SI_MODEL_DISPOSE: Routine = Routine::new("SI_modelDispose", 0x0846_1920);

/// The double 0.6666 at 0x0864a248.
const C_TWO_THIRDS: f64 = f64::from_bits(0x3fe5_54c9_85f0_6f69);
/// The double 0.3333 at 0x0864a250.
const C_THIRD: f64 = f64::from_bits(0x3fd5_54c9_85f0_6f69);
/// The double 0.333 at 0x0864a258.
const C_THIRD_SHORT: f64 = f64::from_bits(0x3fd5_4fdf_3b64_5a1d);

/// `SI_modelChangeName (0x0846db14)`, answered by the caller's world.
pub const SI_MODEL_CHANGE_NAME: Routine = Routine::new("SI_modelChangeName", 0x0846_db14);

/// The body of `SI_modelAddDefinition (0x08467038)` (the port of `si-hrc` takes the other `Outside` trait of that crate;
/// the steps are the same): append the geometry, set the type with the first one, follow the current geometry, rename.
fn add_definition(m: &mut Mem, o: &mut dyn Outside, model: u32, kind: u32, geometry: u32) {
    let first;
    if m.i16(model + 0x34) == 0 {
        m.set_u32(model + 0x30, kind);
        let array = m.si_malloc(4);
        m.set_u32(model + 0x38, array);
        first = 0;
    } else {
        first = m.u32(m.u32(model + 0x38));
        let array = m.si_realloc(m.u32(model + 0x38), m.u16(model + 0x34) as u32 * 4 + 4);
        m.set_u32(model + 0x38, array);
    }
    let slot = m.u32(model + 0x38).wrapping_add(m.u16(model + 0x34) as u32 * 4);
    m.set_u32(slot, geometry);
    let n = m.i16(model + 0x34).wrapping_add(1);
    m.set_i16(model + 0x34, n);
    if m.u32(model + 0x48) == first {
        let head = m.u32(m.u32(model + 0x38));
        m.set_u32(model + 0x48, head);
    }
    o.call(m, SI_MODEL_CHANGE_NAME, &[model, m.u32(model + 4)]);
}

/// A control point record: nine words copied verbatim (`rep movsd`).
fn copy_point(m: &mut Mem, from: u32, to: u32) {
    for w in 0..9 {
        let v = m.u32(from.wrapping_add(4 * w));
        m.set_u32(to.wrapping_add(4 * w), v);
    }
}

/// Fills point `to` with `a * first + b * second` for the control points `a` and `b` (the register order of the listing: the
/// x products stay at 80 bits and are added there; y, z and w are products stored as doubles and then added; the flag word is
/// cleared).
fn blend_point(m: &mut Mem, a: u32, b: u32, first: f64, second: f64, to: u32) {
    let (f, s) = (x(first), x(second));
    let xa = x(m.f64(a)) * f;
    let xb = x(m.f64(b)) * s;
    let prod = |m: &Mem, p: u32, o: u32, k: F80| (x(m.f64(p.wrapping_add(o))) * k).to_f64();
    let ya = prod(m, a, 8, f);
    let za = prod(m, a, 0x10, f);
    let wa = prod(m, a, 0x18, f);
    let yb = prod(m, b, 8, s);
    let zb = prod(m, b, 0x10, s);
    let wb = prod(m, b, 0x18, s);
    m.set_f64(to, (xb + xa).to_f64());
    m.set_f64(to.wrapping_add(8), (x(ya) + x(yb)).to_f64());
    m.set_f64(to.wrapping_add(0x10), (x(za) + x(zb)).to_f64());
    m.set_f64(to.wrapping_add(0x18), (x(wa) + x(wb)).to_f64());
    m.set_u32(to.wrapping_add(0x20), 0);
}

/// [soft 0x08144c4c SI_skin2Nurbs]: `(models, count, spline_type, closed, flag)` builds a NURBS surface that skins the
/// `count` curve models (type 9 NURBS curve, or type 2 whose geometry holds the curve at +0x80) given in the pointer array
/// `models`; returns the new surface (0 when `count` is below 2 or the allocation fails). Each curve is copied, moved by its
/// model's matrix (+0x1c4, mask 0) and put in a scratch model, and the scratch models are disposed at the end. The surface
/// takes order, point count, knots, range and closing from the **last** curve; its V direction has `count` curves (4 when
/// `count` is 2 or 3: the two or three curves are spread over four rows, the rows between blended with the weights 0.6666 and
/// 0.3333, or 0.333 and 0.6666 and 0.3333 for three curves), V order 2 for `spline_type` 0, 3 for 1, else the allocator's 4,
/// uniform knots 0, 1, 2, ... (clamped for orders above 2 when not closed). `closed` is the V closing short at +0x44 and
/// `flag` the word at +0x74.
/// [The original does not free the pointer array of scratch models; the port does not either (the image leaks it the same
/// way). A model of neither type keeps the previous curve, as the original does; the first one would copy a null curve.]
pub fn skin2_nurbs(m: &mut Mem, o: &mut dyn Outside, models: u32, count: i32, spline_type: i32, closed: i32, flag: u32) -> u32 {
    let count16 = count as i16 as i32;
    let closed16 = closed as i16;
    if count16 < 2 {
        return 0;
    }
    let n = count16;
    let arr = m.si_malloc((n as u32).wrapping_mul(4));
    let mut cur_crv: u32 = 0;
    let mut copy: u32 = 0;
    let mut i: i32 = 0;
    while i < n {
        let model = m.u32(models.wrapping_add(4 * i as u32));
        let t = m.u32(model.wrapping_add(0x30));
        if t == 9 {
            cur_crv = m.u32(model.wrapping_add(0x48));
        } else if t == 2 {
            cur_crv = m.u32(m.u32(model.wrapping_add(0x48)).wrapping_add(0x80));
        }
        copy = crv_copy(m, cur_crv);
        crv_apply_matrix(m, copy, m.u32(models.wrapping_add(4 * i as u32)).wrapping_add(0x1c4), 0);
        let nm = model_allocate(m, 0).addr();
        m.set_u32(arr.wrapping_add(4 * i as u32), nm);
        add_definition(m, o, nm, 9, copy);
        i += 1;
    }
    let s = nurbs_srf_allocate(m).addr();
    if s != 0 {
        let c = copy;
        m.set_u32(s, m.u32(c));
        let cl = m.i16(c.wrapping_add(0x20));
        m.set_i16(s.wrapping_add(0x20), cl);
        m.set_u32(s.wrapping_add(4), m.u32(c.wrapping_add(4)));
        let nk = if cl == 0 { m.i32(c).wrapping_add(-2).wrapping_add(m.i32(c.wrapping_add(4))) } else { m.i32(c.wrapping_add(4)).wrapping_add(1) };
        m.set_i32(s.wrapping_add(8), nk);
        let k = knots_allocate(m, nk);
        m.set_u32(s.wrapping_add(0xc), k);
        let mut j = 0;
        while j < m.i32(s.wrapping_add(8)) {
            let v = m.u64(m.u32(c.wrapping_add(0xc)).wrapping_add(8 * j as u32));
            m.set_u64(m.u32(s.wrapping_add(0xc)).wrapping_add(8 * j as u32), v);
            j += 1;
        }
        for off in [0x10u32, 0x14, 0x18, 0x1c] {
            let v = m.u32(c.wrapping_add(off));
            m.set_u32(s.wrapping_add(off), v);
        }
        m.set_u32(s.wrapping_add(0x70), m.u32(c.wrapping_add(0x34)));
        m.set_u32(s.wrapping_add(0x74), flag);
        m.set_i16(s.wrapping_add(0x44), closed16);
        let (vo, vn, vk, vks, _vs, _ve, tot, pts) = (0x24u32, 0x28u32, 0x2cu32, 0x30u32, 0x34u32, 0x3cu32, 0x48u32, 0x54u32);
        let cols = |m: &Mem| m.i32(s.wrapping_add(4));
        let row_pt = |m: &Mem, model_idx: i32, col: i32| -> u32 {
            let model = m.u32(arr.wrapping_add(4 * model_idx as u32));
            m.u32(m.u32(model.wrapping_add(0x48)).wrapping_add(0x2c)).wrapping_add(0x24 * col as u32)
        };
        let dst_pt = |m: &Mem, row: i32, col: i32| -> u32 {
            m.u32(s.wrapping_add(pts)).wrapping_add(((col.wrapping_add(cols(m).wrapping_mul(row))) as u32).wrapping_mul(0x24))
        };
        if spline_type == 0 {
            m.set_i32(s.wrapping_add(vo), 2);
            m.set_i32(s.wrapping_add(vn), n);
            let nkv = if closed16 == 0 { n } else { n + 1 };
            m.set_i32(s.wrapping_add(vk), nkv);
            let kv = knots_allocate(m, nkv);
            m.set_u32(s.wrapping_add(vks), kv);
            let mut j = 0;
            while j < m.i32(s.wrapping_add(vk)) {
                m.set_f64(m.u32(s.wrapping_add(vks)).wrapping_add(8 * j as u32), j as f64);
                j += 1;
            }
            v_tail(m, s, c, closed16);
            m.set_i32(s.wrapping_add(tot), m.i32(s.wrapping_add(4)).wrapping_mul(m.i32(s.wrapping_add(vn))));
            let p = nurbs_points_allocate(m, m.i32(s.wrapping_add(4)).wrapping_mul(m.i32(s.wrapping_add(vn))));
            m.set_u32(s.wrapping_add(pts), p);
            copy_rows(m, s, arr, &row_pt, &dst_pt);
        } else {
            if spline_type == 1 {
                m.set_i32(s.wrapping_add(vo), 3);
            }
            m.set_i32(s.wrapping_add(vn), if count16 < 4 { 4 } else { n });
            let vord = m.i32(s.wrapping_add(vo));
            let vnp = m.i32(s.wrapping_add(vn));
            let nkv = if m.i16(s.wrapping_add(0x44)) == 0 { vord.wrapping_sub(2).wrapping_add(vnp) } else { vnp.wrapping_add(1) };
            m.set_i32(s.wrapping_add(vk), nkv);
            let kv = knots_allocate(m, nkv);
            m.set_u32(s.wrapping_add(vks), kv);
            let kb = |m: &Mem| m.u32(s.wrapping_add(vks));
            if vord == 2 || closed16 != 0 {
                let mut j = 0;
                while j < m.i32(s.wrapping_add(vk)) {
                    m.set_f64(kb(m).wrapping_add(8 * j as u32), j as f64);
                    j += 1;
                }
            } else {
                let mut j = 0;
                while j < m.i32(s.wrapping_add(vo)).wrapping_sub(1) {
                    m.set_u64(kb(m).wrapping_add(8 * j as u32), 0);
                    j += 1;
                }
                let mut v = 1;
                while j < m.i32(s.wrapping_add(vk)).wrapping_sub(m.i32(s.wrapping_add(vo))).wrapping_add(1) {
                    m.set_f64(kb(m).wrapping_add(8 * j as u32), v as f64);
                    v += 1;
                    j += 1;
                }
                while j < m.i32(s.wrapping_add(vk)) {
                    let last = m.i32(s.wrapping_add(vn)).wrapping_sub(m.i32(s.wrapping_add(vo))).wrapping_add(1);
                    m.set_f64(kb(m).wrapping_add(8 * j as u32), last as f64);
                    j += 1;
                }
            }
            v_tail(m, s, c, closed16);
            m.set_i32(s.wrapping_add(tot), m.i32(s.wrapping_add(4)).wrapping_mul(m.i32(s.wrapping_add(vn))));
            let p = nurbs_points_allocate(m, m.i32(s.wrapping_add(4)).wrapping_mul(m.i32(s.wrapping_add(vn))));
            m.set_u32(s.wrapping_add(pts), p);
            if count16 < 4 {
                // the first curve is row 0; the last is row 3
                let last_model = if count16 == 2 { 1 } else { 2 };
                let mut j = 0;
                while j < cols(m) {
                    copy_point(m, row_pt(m, 0, j), dst_pt(m, 0, j));
                    j += 1;
                }
                if count16 == 2 {
                    let mut j = 0;
                    while j < cols(m) {
                        blend_point(m, row_pt(m, 0, j), row_pt(m, 1, j), C_TWO_THIRDS, C_THIRD, dst_pt(m, 1, j));
                        j += 1;
                    }
                    let mut j = 0;
                    while j < cols(m) {
                        blend_point(m, row_pt(m, 0, j), row_pt(m, 1, j), C_THIRD, C_TWO_THIRDS, dst_pt(m, 2, j));
                        j += 1;
                    }
                } else {
                    let mut j = 0;
                    while j < cols(m) {
                        blend_point(m, row_pt(m, 0, j), row_pt(m, 1, j), C_THIRD_SHORT, C_TWO_THIRDS, dst_pt(m, 1, j));
                        j += 1;
                    }
                    let mut j = 0;
                    while j < cols(m) {
                        blend_point(m, row_pt(m, 1, j), row_pt(m, 2, j), C_TWO_THIRDS, C_THIRD, dst_pt(m, 2, j));
                        j += 1;
                    }
                }
                let mut j = 0;
                while j < cols(m) {
                    copy_point(m, row_pt(m, last_model, j), dst_pt(m, 3, j));
                    j += 1;
                }
            } else {
                copy_rows(m, s, arr, &row_pt, &dst_pt);
            }
        }
    }
    // the scratch models go; the pointer array stays
    let mut k = n;
    let mut slot = arr;
    while k != 0 {
        o.call(m, SI_MODEL_DISPOSE, &[slot, 0]);
        slot = slot.wrapping_add(4);
        k -= 1;
    }
    s
}

/// The tail both V-direction branches share: range of the V knots, the curve's uniformity word (+0x24 into +0x4c) and short
/// (+0x28 into +0x50), the zeroed +0x5c and +0x60.
fn v_tail(m: &mut Mem, s: u32, c: u32, closed16: i16) {
    let n_for_end = if m.i16(s.wrapping_add(0x44)) == 0 { m.i32(s.wrapping_add(0x28)) } else { m.i32(s.wrapping_add(0x2c)) };
    let kv = m.u32(s.wrapping_add(0x30));
    let from = if closed16 == 0 { kv.wrapping_add((m.i32(s.wrapping_add(0x24)).wrapping_sub(2) as u32).wrapping_mul(8)) } else { kv };
    let (lo, hi) = (m.u64(from), m.u64(kv.wrapping_add((n_for_end.wrapping_sub(1) as u32).wrapping_mul(8))));
    m.set_u64(s.wrapping_add(0x34), lo);
    m.set_u64(s.wrapping_add(0x3c), hi);
    m.set_u32(s.wrapping_add(0x4c), m.u32(c.wrapping_add(0x24)));
    m.set_i16(s.wrapping_add(0x50), m.i16(c.wrapping_add(0x28)));
    m.set_i16(s.wrapping_add(0x5c), 0);
    m.set_u32(s.wrapping_add(0x60), 0);
}

/// One row per scratch model: the model's control points copied into row `i`.
fn copy_rows(m: &mut Mem, s: u32, _arr: u32, row_pt: &dyn Fn(&Mem, i32, i32) -> u32, dst_pt: &dyn Fn(&Mem, i32, i32) -> u32) {
    let rows = m.i32(s.wrapping_add(0x28));
    let mut i = 0;
    while i < rows {
        let mut j = 0;
        while j < m.i32(s.wrapping_add(4)) {
            copy_point(m, row_pt(m, i, j), dst_pt(m, i, j));
            j += 1;
        }
        i += 1;
    }
}

/// `sleep` (the import at 0x080c4468), answered by the caller's world. [The emulator image has no symbol for the import, so the
/// vectors label it by its address; the name here is the same.]
pub const SLEEP: Routine = Routine::new("0x080c4468", 0x080c_4468);
/// The text at 0x08687480 ("Cannot skin such a set of curves. Please retry").
const MSG_CANNOT_SKIN: u32 = 0x0868_7480;
/// The double 0.0001 at 0x0864a270: the smallest mean span of the chord-length knots.
const C_MIN_SPAN: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);

fn alloc_rows(m: &mut Mem, n: u32) -> u32 {
    let arr = m.si_calloc(n, 4);
    for i in 0..n {
        let r = m.si_calloc(n, 8);
        m.set_u32(arr.wrapping_add(4 * i), r);
    }
    arr
}

fn free_rows(m: &mut Mem, arr: u32, n: i32) {
    for i in 0..n.max(0) {
        let slot = arr.wrapping_add(4 * i as u32);
        if m.u32(slot) != 0 {
            m.free(m.u32(slot));
            m.set_u32(slot, 0);
        }
    }
    if arr != 0 {
        m.free(arr);
    }
}

fn free_some(m: &mut Mem, p: u32) {
    if p != 0 {
        m.free(p);
    }
}

fn unit_vectors(m: &mut Mem, n: u32) -> u32 {
    let v = m.si_calloc(n, 0x10);
    for i in 0..n {
        let p = v.wrapping_add(16 * i);
        m.set_u32(p, 0);
        m.set_u32(p + 4, 0);
        m.set_u32(p + 8, 0);
        m.set_u32(p + 12, 0x3f80_0000);
    }
    v
}

/// The point `j` of a curve (`crv + 0x2c`, stride 0x24) as the three homogeneous quotients `x/w, y/w, z/w`, each divided in
/// the register and rounded to float.
fn quot3(m: &Mem, geom: u32, j: i32) -> [F80; 3] {
    let p = m.u32(geom.wrapping_add(0x2c)).wrapping_add((j as u32).wrapping_mul(0x24));
    let w = x(m.f64(p.wrapping_add(0x18)));
    [0u32, 8, 0x10].map(|o| x((x(m.f64(p.wrapping_add(o))) / w).to_f32()))
}

/// The distance of two such points: `sqrt((dx² + dy²) + dz²)` in the register (not rounded to float).
fn dist3(a: [F80; 3], b: [F80; 3]) -> F80 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    ((dx * dx + dy * dy) + dz * dz).sqrt()
}

/// [soft 0x08146210 SI_skin2NurbsFarin]: `(models, count, skin, closed)` skins the `count` curve models like
/// [`skin2_nurbs`] but interpolates them: the surface passes through every curve. `skin` is the skinning record (short +0x14
/// the knot rule: 0 uniform, 2 to 4 chord length with the square root taken once (2, 3) or twice (4); 1 or anything else
/// leaves all V knots 0; short +0x16 the degree, which makes V order `degree + 2` (the system sizes below are right for degree
/// 2); int +0x18 stored at +0x74). The V knots come from the rule (an open surface repeats the end knots). A mean span below
/// 0.0001 of a pair of neighbouring curves (the points taken as floats of `x/w`, `y/w`, `z/w`) fails the skin: the message
/// "Cannot skin such a set of curves. Please retry" (`SI_statusDisplayMessage`, level 3), `sleep(1)`, scratch models and
/// the half-built surface disposed, 0 returned. Otherwise, for every column of control points the `count` points (as floats)
/// are fitted with a curve: open, the Bessel end conditions (tridiagonal system of the knots, [`setup_open_system`] and
/// friends) and the ends copied in; closed, the cyclic system ([`setup_closed_system`], [`lu_decomp`], [`solve_lu`]); the
/// curve's control points go into the column and `SI_knotsSrfReparametrization` finishes the surface.
/// [The scratch pointer array is not freed, as in [`skin2_nurbs`].]
pub fn skin2_nurbs_farin(m: &mut Mem, o: &mut dyn Outside, models: u32, count: i32, skin: u32, closed: i32) -> u32 {
    use crate::geom13sys as sys;
    let count16 = count as i16 as i32;
    let closed16 = closed as i16;
    let n = count16;
    if n < 2 {
        return 0;
    }
    let arr = m.si_malloc((n as u32).wrapping_mul(4));
    let mut cur_crv: u32 = 0;
    let mut copy: u32 = 0;
    for i in 0..n {
        let model = m.u32(models.wrapping_add(4 * i as u32));
        let t = m.u32(model.wrapping_add(0x30));
        if t == 9 {
            cur_crv = m.u32(model.wrapping_add(0x48));
        } else if t == 2 {
            cur_crv = m.u32(m.u32(model.wrapping_add(0x48)).wrapping_add(0x80));
        }
        copy = crv_copy(m, cur_crv);
        crv_apply_matrix(m, copy, model.wrapping_add(0x1c4), 0);
        let nm = model_allocate(m, 0).addr();
        m.set_u32(arr.wrapping_add(4 * i as u32), nm);
        add_definition(m, o, nm, 9, copy);
    }
    let s = nurbs_srf_allocate(m).addr();
    if s == 0 {
        return 0;
    }
    let c = copy;
    m.set_u32(s, m.u32(c));
    let cl = m.i16(c.wrapping_add(0x20));
    m.set_i16(s.wrapping_add(0x20), cl);
    m.set_u32(s.wrapping_add(4), m.u32(c.wrapping_add(4)));
    let nku = if cl == 0 { m.i32(c).wrapping_add(-2).wrapping_add(m.i32(c.wrapping_add(4))) } else { m.i32(c.wrapping_add(4)).wrapping_add(1) };
    m.set_i32(s.wrapping_add(8), nku);
    let k = knots_allocate(m, nku);
    m.set_u32(s.wrapping_add(0xc), k);
    let mut j = 0;
    while j < m.i32(s.wrapping_add(8)) {
        let v = m.u64(m.u32(c.wrapping_add(0xc)).wrapping_add(8 * j as u32));
        m.set_u64(m.u32(s.wrapping_add(0xc)).wrapping_add(8 * j as u32), v);
        j += 1;
    }
    for off in [0x10u32, 0x14, 0x18, 0x1c] {
        let v = m.u32(c.wrapping_add(off));
        m.set_u32(s.wrapping_add(off), v);
    }
    m.set_u32(s.wrapping_add(0x70), m.u32(c.wrapping_add(0x34)));
    m.set_u32(s.wrapping_add(0x74), m.u32(skin.wrapping_add(0x18)));
    m.set_i16(s.wrapping_add(0x44), closed16);
    let degree = m.i16(skin.wrapping_add(0x16)) as i32;
    m.set_i32(s.wrapping_add(0x24), degree + 2);
    let vn = if closed16 == 0 { degree + n } else { n };
    m.set_i32(s.wrapping_add(0x28), vn);
    let nk = if m.i16(s.wrapping_add(0x44)) == 0 { m.i32(s.wrapping_add(0x24)).wrapping_sub(2).wrapping_add(vn) } else { vn.wrapping_add(1) };
    m.set_i32(s.wrapping_add(0x2c), nk);
    let kv = knots_allocate(m, nk);
    m.set_u32(s.wrapping_add(0x30), kv);
    let ty = m.i16(skin.wrapping_add(0x14));
    m.set_i32(s.wrapping_add(0x4c), ty as i32);
    m.set_i16(s.wrapping_add(0x50), m.i16(c.wrapping_add(0x28)));
    m.set_i16(s.wrapping_add(0x5c), 0);
    m.set_u32(s.wrapping_add(0x60), 0);
    let total = m.i32(s.wrapping_add(4)).wrapping_mul(m.i32(s.wrapping_add(0x28)));
    m.set_i32(s.wrapping_add(0x48), total);
    let p = nurbs_points_allocate(m, total);
    m.set_u32(s.wrapping_add(0x54), p);

    let kk = m.si_calloc(nk as u32, 8);
    let kat = |i: i32| kk.wrapping_add((i as u32).wrapping_mul(8));
    let mut fail = false;
    let geom = |m: &Mem, i: i32| m.u32(m.u32(arr.wrapping_add(4 * i as u32)).wrapping_add(0x48));
    if ty == 0 {
        let mut i = 1;
        while i < nk {
            if closed16 == 0 && (i < 3 || nk.wrapping_sub(3) < i) {
                let v = m.u64(kat(i - 1));
                m.set_u64(kat(i), v);
            } else {
                let v = (x(m.f64(kat(i - 1))) + x(1.0f64)).to_f64();
                m.set_f64(kat(i), v);
            }
            i += 1;
        }
    } else if (2..=4).contains(&ty) {
        let spans = m.i32(cur_crv.wrapping_add(4));
        let mut acc = 0.0f64;
        let mut acc_close = 0.0f64;
        let mut i = 0;
        while i < n - 1 {
            let (ga, gb) = (geom(m, i), geom(m, i + 1));
            let ring = closed16 != 0 && i == n - 2;
            let gc = if ring { geom(m, 0) } else { 0 };
            let mut j = 0;
            while j < spans {
                let (a, b) = (quot3(m, ga, j), quot3(m, gb, j));
                let mut sd = dist3(a, b);
                if ring {
                    let mut s2 = dist3(a, quot3(m, gc, j));
                    if ty == 4 {
                        s2 = s2.sqrt();
                    }
                    acc_close = (s2 + x(acc_close)).to_f64();
                }
                if ty == 4 {
                    sd = sd.sqrt();
                }
                acc = (sd + x(acc)).to_f64();
                j += 1;
            }
            let avg = x(acc) / F80::from_i32(count16);
            if avg.lt(x(C_MIN_SPAN)) {
                fail = true;
            }
            if closed16 == 0 {
                let v = (avg + x(m.f64(kat(i + 2)))).to_f64();
                m.set_f64(kat(i + 3), v);
            } else {
                let v = (avg + x(m.f64(kat(i)))).to_f64();
                m.set_f64(kat(i + 1), v);
            }
            acc = 0.0;
            i += 1;
        }
        if closed16 == 0 {
            let mut i = n + 2;
            while i < nk {
                let v = m.u64(kat(i - 1));
                m.set_u64(kat(i), v);
                i += 1;
            }
        } else {
            let avg2 = x(acc_close) / F80::from_i32(count16);
            let v = (avg2 + x(m.f64(kat(nk - 2)))).to_f64();
            m.set_f64(kat(nk - 1), v);
        }
    }
    if fail {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_CANNOT_SKIN, 3]);
        o.call(m, SLEEP, &[1]);
        for i in 0..n {
            o.call(m, SI_MODEL_DISPOSE, &[arr.wrapping_add(4 * i as u32), 0]);
        }
        free_some(m, kk);
        let sp0 = m.sp();
        let slot = m.local(4);
        m.set_u32(slot, s);
        crate::image::srf_dispose(m, o, slot);
        m.set_sp(sp0);
        return 0;
    }
    let (mut a5c, mut a60, mut a64) = (0u32, 0u32, 0u32);
    let nsys;
    let (r68, r6c);
    if closed16 == 0 {
        nsys = nk.wrapping_sub(4);
        a5c = m.si_calloc(nsys as u32, 8);
        a60 = m.si_calloc(nsys as u32, 8);
        a64 = m.si_calloc(nsys as u32, 8);
        r68 = alloc_rows(m, nsys as u32);
        r6c = alloc_rows(m, nsys as u32);
        sys::setup_open_system(m, kk, nsys, a5c, a60, a64);
        sys::lu_decomp_tridiagonal(m, a5c, a60, a64, nsys, r6c, r68);
    } else {
        nsys = nk.wrapping_sub(1);
        r68 = alloc_rows(m, nsys as u32);
        r6c = alloc_rows(m, nsys as u32);
        let mat = alloc_rows(m, nsys as u32);
        sys::setup_closed_system(m, kk, n, mat);
        sys::lu_decomp(m, mat, nsys, r6c, r68);
        free_rows(m, mat, nsys);
    }
    let v1 = unit_vectors(m, nk as u32);
    let v2 = unit_vectors(m, nk as u32);
    let sp0 = m.sp();
    let crv_slot = m.local(4);
    let fit = nurbs_crv_allocate(m).addr();
    m.set_u32(crv_slot, fit);
    m.set_i32(fit.wrapping_add(4), if closed16 == 0 { n + 2 } else { n });
    let fp = nurbs_points_allocate(m, if closed16 == 0 { n + 2 } else { n });
    m.set_u32(fit.wrapping_add(0x2c), fp);
    let mut last = geom(m, 0);
    let mut col = 0;
    while col < m.i32(last.wrapping_add(4)) {
        for r in 0..n {
            last = geom(m, r);
            let pt = m.u32(last.wrapping_add(0x2c)).wrapping_add((col as u32).wrapping_mul(0x24));
            for k in 0..4u32 {
                let v = m.f64(pt.wrapping_add(8 * k)) as f32;
                m.set_f32(v2.wrapping_add(16 * r as u32 + 4 * k), v);
            }
        }
        if closed16 == 0 {
            sys::setup_open_input(m, kk, v2, v1, n);
            sys::solve_tridiagonal_lu(m, r68, r6c, v1, fit, n);
            let fpts = m.u32(fit.wrapping_add(0x2c));
            for k in 0..4u32 {
                let a = m.f32(v2.wrapping_add(4 * k)) as f64;
                m.set_f64(fpts.wrapping_add(8 * k), a);
                let b = m.f32(v2.wrapping_add(16 * (n - 1) as u32 + 4 * k)) as f64;
                m.set_f64(fpts.wrapping_add((n as u32 + 1).wrapping_mul(0x24)).wrapping_add(8 * k), b);
            }
        } else {
            sys::setup_closed_input(m, kk, v2, v1, n);
            sys::solve_lu(m, r68, r6c, v1, fit, n);
        }
        let mut row = 0;
        while row < m.i32(fit.wrapping_add(4)) {
            let src = m.u32(fit.wrapping_add(0x2c)).wrapping_add((row as u32).wrapping_mul(0x24));
            let dst = m.u32(s.wrapping_add(0x54)).wrapping_add(((row.wrapping_mul(m.i32(last.wrapping_add(4))).wrapping_add(col)) as u32).wrapping_mul(0x24));
            for k in 0..4u32 {
                let v = m.u64(src.wrapping_add(8 * k));
                m.set_u64(dst.wrapping_add(8 * k), v);
            }
            row += 1;
        }
        col += 1;
    }
    let mut i = 0;
    while i < m.i32(s.wrapping_add(0x2c)) {
        let v = m.u64(kat(i));
        m.set_u64(m.u32(s.wrapping_add(0x30)).wrapping_add(8 * i as u32), v);
        i += 1;
    }
    let end;
    if closed16 == 0 {
        let from = m.u32(s.wrapping_add(0x30)).wrapping_add((m.i32(s.wrapping_add(0x24)) as u32).wrapping_mul(8)).wrapping_sub(0x10);
        let v = m.u64(from);
        m.set_u64(s.wrapping_add(0x34), v);
        end = m.i32(s.wrapping_add(0x28));
    } else {
        m.set_u32(s.wrapping_add(0x34), 0);
        m.set_u32(s.wrapping_add(0x38), 0);
        end = m.i32(s.wrapping_add(0x2c));
    }
    let v = m.u64(m.u32(s.wrapping_add(0x30)).wrapping_add((end.wrapping_sub(1) as u32).wrapping_mul(8)));
    m.set_u64(s.wrapping_add(0x3c), v);
    for i in 0..n {
        o.call(m, SI_MODEL_DISPOSE, &[arr.wrapping_add(4 * i as u32), 0]);
    }
    free_some(m, kk);
    free_some(m, a5c);
    free_some(m, a60);
    free_some(m, a64);
    for i in 0..nsys.max(0) {
        for r in [r68, r6c] {
            let slot = r.wrapping_add(4 * i as u32);
            if m.u32(slot) != 0 {
                m.free(m.u32(slot));
                m.set_u32(slot, 0);
            }
        }
    }
    free_some(m, r68);
    free_some(m, r6c);
    free_some(m, v1);
    free_some(m, v2);
    nurbs_crv_dispose(m, crv_slot);
    m.set_sp(sp0);
    crate::geom3::knots_srf_reparametrization(m, o, s);
    s
}

/// `meshSkinFromNurbs (0x08143ae4)`: the polygon-mesh skin; not ported, answered by the caller's world.
pub const MESH_SKIN_FROM_NURBS: Routine = Routine::new("meshSkinFromNurbs", 0x0814_3ae4);
/// `skin2NurbsQuad (0x08145870)`: the quadratic skin; not ported, answered by the caller's world.
pub const SKIN2_NURBS_QUAD: Routine = Routine::new("skin2NurbsQuad", 0x0814_5870);
/// `SI_nurbsSrfClose (0x084ed69c)`: ported in [`crate::close::srf_close`], which is called directly; the constant stays for the routine list.
pub const SI_NURBS_SRF_CLOSE: Routine = Routine::new("SI_nurbsSrfClose", 0x084e_d69c);
/// `SI_modelSubstituteIDefinition (0x08467158)`: ported in `si-model`, answered by the caller's world.
pub const SI_MODEL_SUBSTITUTE_IDEFINITION: Routine = Routine::new("SI_modelSubstituteIDefinition", 0x0846_7158);
/// The text at 0x08687255: the name of a model made by the skin ("skin").
const NAME_SKIN: u32 = 0x0868_7255;

/// [soft 0x08142ff0 SI_createSkinFromNurbs] `(models, model_slot, skin)` makes the skin of the curve models `models`
/// described by the skinning record `skin` (shorts: +0 closed, +4 curve count, +8 and +0xc the two mesh counts, +0xe the
/// kind (0 a polygon mesh, otherwise a NURBS surface), +0x16 the degree) and puts it into the model `*model_slot`: a new
/// model named "skin" (`SI_modelAllocate`, the skin added as its definition) when the slot is null, otherwise the first
/// definition of the existing model is replaced (`SI_modelSubstituteIDefinition`); the model's type becomes 4 (mesh) or 10
/// (NURBS surface) and the slot is set. Returns 1, or 0 when no skin came out (the model is made all the same). A NURBS
/// skin is [`skin2_nurbs`] for fewer than three curves or any degree but 1 and 2; with degree 2 [`skin2_nurbs_farin`]
/// (closed only when the first curve is open and the record says so; otherwise a closed record asks for the surface to be
/// closed afterwards: its V knots are freed when its uniformity (+0x4c) is 2, then `SI_nurbsSrfClose(srf, 1)`); with degree 1
/// `skin2NurbsQuad`, closed afterwards by `SI_nurbsSrfClose(srf, 0)` for a closed record.
/// [`meshSkinFromNurbs` and `skin2NurbsQuad` are not ported: the world answers them; `SI_nurbsSrfClose` is called directly.]
pub fn create_skin_from_nurbs(m: &mut Mem, o: &mut dyn Outside, models: u32, model_slot: u32, skin: u32) -> u16 {
    let count = m.i16(skin.wrapping_add(4)) as i32;
    let s8 = m.i16(skin.wrapping_add(8)) as i32;
    let closed = m.i16(skin) as i32;
    let s12 = m.i16(skin.wrapping_add(0xc)) as i32;
    let mut result: u16 = 1;
    let mut model = m.u32(model_slot);
    let fresh = model == 0;
    if fresh {
        model = model_allocate(m, NAME_SKIN).addr();
    }
    let put = |m: &mut Mem, o: &mut dyn Outside, geometry: u32, kind: u32| {
        if fresh {
            add_definition(m, o, model, kind, geometry);
        } else {
            o.call(m, SI_MODEL_SUBSTITUTE_IDEFINITION, &[model, 0, geometry]);
        }
    };
    if m.i16(skin.wrapping_add(0xe)) == 0 {
        let mesh = o.call(m, MESH_SKIN_FROM_NURBS, &[models, s8 as u32, count as u32, s12 as u32, closed as u32]).eax;
        if mesh == 0 {
            result = 0;
        } else {
            put(m, o, mesh, 4);
        }
        m.set_u32(model.wrapping_add(0x30), 4);
        m.set_u32(model_slot, model);
        return result;
    }
    let degree = m.i16(skin.wrapping_add(0x16)) as i32;
    let srf;
    let plain = |m: &mut Mem, o: &mut dyn Outside| skin2_nurbs(m, o, models, count, degree, closed, m.u32(skin.wrapping_add(0x18)));
    if count < 3 {
        srf = plain(m, o);
    } else if degree == 2 {
        let first_geometry = m.u32(m.u32(models).wrapping_add(0x48));
        if m.i16(first_geometry.wrapping_add(0x20)) == 0 {
            srf = skin2_nurbs_farin(m, o, models, count, skin, closed);
        } else if closed as i16 == 0 {
            srf = skin2_nurbs_farin(m, o, models, count, skin, 0);
        } else {
            srf = skin2_nurbs_farin(m, o, models, count, skin, 0);
            if m.i32(srf.wrapping_add(0x4c)) == 2 && m.u32(srf.wrapping_add(0x30)) != 0 {
                m.free(m.u32(srf.wrapping_add(0x30)));
                m.set_u32(srf.wrapping_add(0x30), 0);
            }
            let _ = crate::close::srf_close(m, o, srf, 1);
        }
    } else if degree != 1 {
        srf = plain(m, o);
    } else if closed as i16 == 0 {
        srf = o.call(m, SKIN2_NURBS_QUAD, &[models, count as u32, skin, 0]).eax;
    } else {
        srf = o.call(m, SKIN2_NURBS_QUAD, &[models, count as u32, skin, 0]).eax;
        let _ = crate::close::srf_close(m, o, srf, 0);
    }
    if srf == 0 {
        result = 0;
    } else {
        put(m, o, srf, 10);
    }
    m.set_u32(model.wrapping_add(0x30), 10);
    m.set_u32(model_slot, model);
    result
}

/// `SI_statusDisplayMessage (0x083cca94)`, answered by the caller's world (a local name: `trim` is not built in a test build).
const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
