// SPDX-License-Identifier: MIT
//! `zip.c`: the zip of two patches applied (`refreshZip`, below).
//!
//! Checked against the original in the emulator (re/tools/si-model/gen_zip.py, run by
//! `mattxt-emucheck`): 4000 cases, about half with a null dialog (the way `SI_zipUpdate` calls
//! it) and half with a dialog whose three UI callees (`SI_getTexedLong`, `SI_setSliderMinMaxi`,
//! `SI_refreshSelectedUnderDialog`) are recorded stand-ins, `SI_getTexedLong` answering
//! generated numbers (in range, below 0, past the limits). All four (`zipUSup`, `zipUZip`)
//! combinations, both orientations, open and closed patches in both directions, start indices
//! from below 0 to past the point count; 3844 identical, 74 identical in the defined part,
//! 0 different, 82 not compared (the original wrote outside a buffer or stopped); with
//! `EMUCHECK_MUTATE=1` every compared case differs, with `=calls` every case that makes a call.
//! Not reached by any case: 58 bytes, all alignment padding between blocks. Notes on the code:
//! - Branches: the four cases of (`zipUSup`, `zipUZip`) are eight blocks in the listing, which
//!   [`limits_block`] folds into one shape. In the case (1, 1) the second block tests the size
//!   against the sum before testing the closed flag; in the other three the test is inside.
//! - Rounding: none in this routine itself. All arithmetic is 32-bit integer (sizes are
//!   unsigned 16-bit widened, products wrap); the only floating point is
//!   `SI_hvectorMultiplyMatrix` (a value function of `si-math`, itself cited there).
//! - Where `SI_matrixInverse` fails the original leaves its output on the stack as it was
//!   (uninitialised memory, never checked: no case has it); here zeros.
//! - Divide errors (a size of 0) are recorded faults here; the original traps. The routine
//!   reads `ptchZip` first for the `SI_patchTransferPoints` call, then divides, so the
//!   fault comes after the transfer and the two matrix calls, as in the original.
//! - Arguments of the `SI_hvectorMultiplyMatrix` calls: the fourth (count) is 1 in both.
//!
//! With a null dialog (the way `SI_zipUpdate` calls it) none of the three UI calls
//! (`SI_getTexedLong`, `SI_setSliderMinMaxi`, `SI_refreshSelectedUnderDialog`) is made: the
//! four numbers come from the globals `rwDw`, `rwUp`, `ptDw`, `ptUp`. With a dialog they go
//! through [`Outside`].

use si_math::algebra as alg;
use si_math::types::Hvector;
use si_mem::{Access, Mem, Ptr32};
use si_spline::zip_image::{MAX_PT_DW, MAX_PT_UP, MAX_RW_DW, MAX_RW_UP, INDICE_SUP, INDICE_ZIP, ZIP_U_SUP, ZIP_U_ZIP};
use si_types::geom::Patch;

use super::{callee, Callee, Outside};

si_mem::globals! {
    /// `zip.c`: the model of the zip's first patch.
    ZIP_MDL: u32 = 0x087889f0, "zipMdl";
    /// `zip.c`: the model the points go to (an unnamed global in the listing).
    ZIP_MDL2: u32 = 0x087889f4, "DAT_087889f4";
    /// `zip.c`: the scratch copy of the first patch the transfer starts from.
    PTCH_ZIP: u32 = 0x087889e8, "ptchZip";
    /// `zip.c`: the wanted shifts: rows down.
    RW_DW: i32 = 0x08788a14, "rwDw";
    /// `zip.c`: rows up.
    RW_UP: i32 = 0x08788a1c, "rwUp";
    /// `zip.c`: points down.
    PT_DW: i32 = 0x08788a04, "ptDw";
    /// `zip.c`: points up.
    PT_UP: i32 = 0x08788a0c, "ptUp";
    /// `zip.c`: the shifts as `SI_zipUpdate` first set them (never read again here).
    OLD_PT_DW: i32 = 0x08788a08, "oldPtDw";
    OLD_PT_UP: i32 = 0x08788a10, "oldPtUp";
    OLD_RW_DW: i32 = 0x08788a18, "oldRwDw";
    OLD_RW_UP: i32 = 0x08788a20, "oldRwUp";
    /// `zip.c`: the scratch copy of the first NURBS surface (the NURBS branch).
    NRB_SRF_ZIP: u32 = 0x087889ec, "nrbSrfZip";
}

/// `SI_getTexedLong` (0x0838c204)
const GET_TEXED_LONG: Callee = Callee { name: "SI_getTexedLong", addr: 0x0838c204 };
/// `SI_setSliderMinMaxi` (0x0838cbb0)
const SET_SLIDER_MIN_MAXI: Callee = Callee { name: "SI_setSliderMinMaxi", addr: 0x0838cbb0 };
/// `SI_refreshSelectedUnderDialog` (0x083263e0)
const REFRESH_SELECTED_UNDER_DIALOG: Callee = Callee { name: "SI_refreshSelectedUnderDialog", addr: 0x083263e0 };

/// The signed division (`cdq`, `idiv`); a divisor of 0 is a recorded fault and zeros.
fn idiv(m: &Mem, a: i32, b: i32) -> (i32, i32) {
    if b == 0 || (a == i32::MIN && b == -1) {
        m.record_fault(0, 0, Access::Read);
        return (0, 0);
    }
    (a.wrapping_div(b), a.wrapping_rem(b))
}

fn hvec(m: &Mem, a: u32) -> Hvector {
    Hvector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8), m.f32(a + 12))
}

fn put_hvec(m: &mut Mem, a: u32, v: Hvector) {
    m.set_f32(a, v.x);
    m.set_f32(a + 4, v.y);
    m.set_f32(a + 8, v.z);
    m.set_f32(a + 12, v.w);
}

/// One `SI_hvectorMultiplyMatrix(src, matrix, dst, 1)`, [soft 0x08516c28 SI_hvectorMultiplyMatrix].
fn hmul(m: &mut Mem, src: u32, mat: &[[f32; 4]; 4], dst: u32) {
    let v = alg::hvector_multiply_matrix(&hvec(m, src), mat);
    put_hvec(m, dst, v);
}

fn local_mat(m: &Mem, a: u32) -> [[f32; 4]; 4] {
    super::mat(m, a)
}

/// One block of limits: the patch `ps` (the target) has a closed flag and size `psn` along
/// one direction; `a` and `b` are the two wanted shifts that apply to it. Returns `false`
/// when the original returns 0 (the size is no more than their sum). The two sliders are
/// set (when there is a dialog) to the smaller of the room in `ps` and the room in the
/// other patch `pz` (`pzn` points along its direction, `q` the start's place there).
#[allow(clippy::too_many_arguments)]
fn limits_block(m: &mut Mem, o: &mut dyn Outside, dlg: u32, ps_closed: i16, psn: i32, a: i32, b: i32, pz_closed: i16, pzn: i32, q: i32, s: (u32, u32), check_inside: bool) -> bool {
    if check_inside {
        if ps_closed != 0 && psn <= a.wrapping_add(b) {
            return false;
        }
    } else if psn <= a.wrapping_add(b) {
        return false;
    }
    if ps_closed == 0 {
        return true;
    }
    let (hi1, hi2);
    if pz_closed == 0 {
        let other = pzn.wrapping_sub(q).wrapping_sub(1);
        hi1 = psn.wrapping_sub(b.wrapping_add(1)).min(q);
        hi2 = psn.wrapping_sub(a.wrapping_add(1)).min(other);
    } else {
        hi1 = psn.wrapping_sub(b.wrapping_add(1));
        hi2 = psn.wrapping_sub(a.wrapping_add(1));
    }
    if dlg != 0 {
        o.call(m, &SET_SLIDER_MIN_MAXI, &[dlg, s.0, 0, hi1 as u32]);
        o.call(m, &SET_SLIDER_MIN_MAXI, &[dlg, s.1, 0, hi2 as u32]);
    }
    true
}

/// `refreshZip(dialog)` [soft 0x081a0180 refreshZip]: the second model's patch gets, through the two
/// matrices, the first patch's points that the zip moves; answers 1 when it ran and 0 when
/// the wanted shifts were out of range.
pub fn refresh_zip(m: &mut Mem, o: &mut dyn Outside, dlg: u32) -> u32 {
    let zip_mdl = m[ZIP_MDL];
    let mdl2 = m[ZIP_MDL2];
    let pz = m.u32(zip_mdl + 0x48); // iVar15: the first model's patch
    let ps = m.u32(mdl2 + 0x48); // pSVar1: the second model's patch
    // SI_patchTransferPoints(ptchZip, ps) [soft 0x084810cc]: the image form of si-spline
    si_spline::patch_image::patch_transfer_points(m, Ptr32::<Patch>::new(m[PTCH_ZIP]), Ptr32::<Patch>::new(ps));
    // SI_matrixCopy(zipMdl + 0x1c4, local_44); SI_matrixInverse(mdl2 + 0x1c4, local_84)
    let local_44 = alg::matrix_copy(&local_mat(m, zip_mdl + 0x1c4));
    // without an inverse the original stores nothing and the stack array keeps what it held
    // (uninitialised); here zeros
    let local_84 = alg::matrix_inverse(&local_mat(m, mdl2 + 0x1c4)).unwrap_or([[0f32; 4]; 4]);
    let iv2 = m.u32(pz + 0x1a4); // first patch's points
    let iv3 = m.u32(ps + 0x1a4); // second patch's points
    let u4 = m.u16(pz + 0xc) as i32;
    let u10 = m.u16(pz + 0xe) as i32;
    let u5 = m.u16(ps + 0xc) as i32;
    let u11 = m.u16(ps + 0xe) as i32;
    let ind_sup = m[INDICE_SUP];
    let ind_zip = m[INDICE_ZIP];
    let (iv6, iv12) = idiv(m, ind_sup, u10);
    let (iv7, iv13) = idiv(m, ind_zip, u11);
    let (mut iv6, iv12) = (iv6, iv12);

    // the four wanted numbers
    let (l98, l9c, la0, mut lc8);
    if dlg == 0 {
        l98 = m[RW_DW];
        if l98 < 0 || m[MAX_RW_DW] < l98 {
            return 0;
        }
        l9c = m[RW_UP];
        if l9c < 0 || m[MAX_RW_UP] < l9c {
            return 0;
        }
        la0 = m[PT_DW];
        if la0 < 0 || m[MAX_PT_DW] < la0 {
            return 0;
        }
        lc8 = m[PT_UP];
        if lc8 < 0 || m[MAX_PT_UP] < lc8 {
            return 0;
        }
    } else {
        let ask = |m: &mut Mem, o: &mut dyn Outside, id: u32| o.call(m, &GET_TEXED_LONG, &[dlg, id]) as i32;
        l98 = ask(m, o, 3);
        if l98 < 0 || m[MAX_RW_DW] < l98 {
            return 0;
        }
        l9c = ask(m, o, 6);
        if l9c < 0 || m[MAX_RW_UP] < l9c {
            return 0;
        }
        la0 = ask(m, o, 9);
        if la0 < 0 || m[MAX_PT_DW] < la0 {
            return 0;
        }
        lc8 = ask(m, o, 0xc);
        if lc8 < 0 || m[MAX_PT_UP] < lc8 {
            return 0;
        }
    }

    let z_sup = m[ZIP_U_SUP];
    let z_zip = m[ZIP_U_ZIP];
    let (ps_uc, ps_vc) = (m.i16(ps + 0x10), m.i16(ps + 0x12));
    let (pz_uc, pz_vc) = (m.i16(pz + 0x10), m.i16(pz + 0x12));
    let (c4, c8);
    let (a98, a9c, aa0, ac8) = (l98, l9c, la0, lc8);
    if z_sup == 0 {
        let ok = if z_zip == 0 {
            limits_block(m, o, dlg, ps_uc, u5, a98, a9c, pz_uc, u4, iv6, (3, 6), true)
                && limits_block(m, o, dlg, ps_vc, u11, aa0, ac8, pz_vc, u10, iv12, (9, 0xc), true)
        } else {
            limits_block(m, o, dlg, ps_uc, u5, aa0, ac8, pz_vc, u10, iv12, (9, 0xc), true)
                && limits_block(m, o, dlg, ps_vc, u11, a98, a9c, pz_uc, u4, iv6, (3, 6), true)
        };
        if !ok {
            return 0;
        }
        if pz_uc == 0 || iv6.wrapping_sub(l98) >= 0 {
            iv6 = ind_sup.wrapping_sub(l98.wrapping_mul(u10));
        } else {
            let (_, r) = idiv(m, l98, u4);
            if iv6 < r {
                iv6 = ind_sup.wrapping_add(u4.wrapping_sub(r).wrapping_mul(u10));
            } else {
                iv6 = ind_sup.wrapping_sub(r.wrapping_mul(u10));
            }
        }
        if pz_vc == 0 || iv12.wrapping_sub(la0) >= 0 {
            iv6 = iv6.wrapping_sub(la0);
        } else {
            let (_, r) = idiv(m, la0, u10);
            if r != 0 {
                iv6 = iv6.wrapping_add(u10.wrapping_sub(r));
            }
        }
        c4 = l98.wrapping_add(l9c);
        lc8 = lc8.wrapping_add(la0);
        c8 = lc8;
    } else {
        let ok = if z_zip == 0 {
            limits_block(m, o, dlg, ps_uc, u5, a98, a9c, pz_vc, u10, iv12, (3, 6), true)
                && limits_block(m, o, dlg, ps_vc, u11, aa0, ac8, pz_uc, u4, iv6, (9, 0xc), true)
        } else {
            // (1, 1): the second block checks the size against the sum before the flag
            limits_block(m, o, dlg, ps_uc, u5, aa0, ac8, pz_uc, u4, iv6, (9, 0xc), true)
                && limits_block(m, o, dlg, ps_vc, u11, a98, a9c, pz_vc, u10, iv12, (3, 6), false)
        };
        if !ok {
            return 0;
        }
        let e0 = lc8.wrapping_add(la0);
        if pz_uc == 0 || iv6.wrapping_sub(la0) >= 0 {
            iv6 = ind_sup.wrapping_sub(la0.wrapping_mul(u10));
        } else {
            let (_, r) = idiv(m, la0, u4);
            if iv6 < r {
                iv6 = ind_sup.wrapping_add(u4.wrapping_sub(r).wrapping_mul(u10));
            } else {
                iv6 = ind_sup.wrapping_sub(r.wrapping_mul(u10));
            }
        }
        if pz_vc == 0 || iv12.wrapping_sub(l98) >= 0 {
            iv6 = iv6.wrapping_sub(l98);
        } else {
            let (_, r) = idiv(m, l98, u10);
            if r != 0 {
                iv6 = iv6.wrapping_add(u10.wrapping_sub(r));
            }
        }
        c4 = e0;
        c8 = l98.wrapping_add(l9c);
    }

    // where the second patch's run starts
    let (first, second) = if z_zip == 0 { (l98, la0) } else { (la0, l98) };
    let t = if ps_uc == 0 || iv7.wrapping_sub(first) >= 0 {
        first.wrapping_mul(u11).wrapping_neg()
    } else {
        u5.wrapping_sub(first).wrapping_mul(u11)
    };
    let mut bc = if ps_vc == 0 || iv13.wrapping_sub(second) >= 0 {
        t.wrapping_add(ind_zip).wrapping_sub(second)
    } else {
        t.wrapping_add(ind_zip).wrapping_add(u11.wrapping_sub(second))
    };

    let step = |m: &mut Mem, from: i32, to: i32, mat1: &[[f32; 4]; 4], mat2: &[[f32; 4]; 4]| {
        let d = iv3.wrapping_add((to as u32).wrapping_mul(16));
        hmul(m, iv2.wrapping_add((from as u32).wrapping_mul(16)), mat1, d);
        hmul(m, d, mat2, d);
    };
    // advance an index one place, wrapping within a row of `n`
    let next = |m: &Mem, i: i32, n: i32| -> i32 {
        let (_, r) = idiv(m, i, n);
        if r < n.wrapping_sub(1) { i.wrapping_add(1) } else { i.wrapping_add(1).wrapping_sub(n) }
    };

    let mut count = 0i32;
    if c4 >= 0 {
        if z_sup == z_zip {
            loop {
                count += 1;
                let mut i13 = iv6.wrapping_add(u10);
                let i12 = bc.wrapping_add(u11);
                let mut k = c8;
                while k >= 0 {
                    step(m, iv6, bc, &local_44, &local_84);
                    iv6 = next(m, iv6, u10);
                    bc = next(m, bc, u11);
                    k -= 1;
                }
                if (u4.wrapping_mul(u10)) <= i13 {
                    i13 = idiv(m, i13, u10).1;
                }
                bc = i12;
                if (u5.wrapping_mul(u11)) <= i12 {
                    bc = idiv(m, i12, u11).1;
                }
                iv6 = i13;
                if count > c4 || m.fault().is_some() {
                    break;
                }
            }
        } else {
            loop {
                let mut c0 = c8;
                let mut i12 = iv6.wrapping_add(u10);
                let i7 = bc.wrapping_add(1);
                count += 1;
                if c8 >= 0 {
                    let mut i13 = bc;
                    loop {
                        step(m, iv6, i13, &local_44, &local_84);
                        iv6 = next(m, iv6, u10);
                        i13 = i13.wrapping_add(u11);
                        if (u5.wrapping_mul(u11)) <= i13 {
                            i13 = idiv(m, i13, u11).1;
                        }
                        c0 -= 1;
                        if c0 < 0 || m.fault().is_some() {
                            break;
                        }
                    }
                }
                if (u4.wrapping_mul(u10)) <= i12 {
                    i12 = idiv(m, i12, u10).1;
                }
                iv6 = idiv(m, bc, u11).1;
                bc = i7;
                if u11.wrapping_sub(1) <= iv6 {
                    bc = i7.wrapping_sub(u11);
                }
                iv6 = i12;
                if count > c4 || m.fault().is_some() {
                    break;
                }
            }
        }
    }

    // SI_modelTouchGeometry(mdl2); SI_patchComputeSurface(ps, 0)
    o.call(m, &callee::SI_MODEL_TOUCH_GEOMETRY, &[mdl2]);
    si_spline::patch_image::patch_compute_surface(m, Ptr32::<Patch>::new(ps), 0);
    if dlg != 0 {
        o.call(m, &REFRESH_SELECTED_UNDER_DIALOG, &[0, 1]);
    }
    1
}

/// `SI_nurbsSrfCopy (0x084dc1a8)`, `initNrbSrfMaxValue (0x0819fffc)`, `refreshNrbSrfZip
/// (0x081a1a34)` and `SI_nurbsSrfDispose (0x084dba5c)`: the NURBS branch of
/// [`zip_update`], not ported, reached through [`Outside`] (the first answers the copy, the
/// last gets the address of `nrbSrfZip`).
const NURBS_SRF_COPY: Callee = Callee { name: "SI_nurbsSrfCopy", addr: 0x084dc1a8 };
const INIT_NRB_SRF_MAX_VALUE: Callee = Callee { name: "initNrbSrfMaxValue", addr: 0x0819fffc };
const REFRESH_NRB_SRF_ZIP: Callee = Callee { name: "refreshNrbSrfZip", addr: 0x081a1a34 };
const NURBS_SRF_DISPOSE: Callee = Callee { name: "SI_nurbsSrfDispose", addr: 0x084dba5c };
/// The text "Your relation has been deleted." (0x0868e0c0).
const RELATION_DELETED: u32 = 0x0868_e0c0;

/// The update routine of a zip relation [soft 0x081a3c34 SI_zipUpdate] (the float argument
/// is not used). `node` is the update node: +8 the model that holds the zip's result (the
/// second patch's), +0x1c the relation record (+0xc a short that is 0 when the relation is
/// gone, +8 the model of the first patch), +0x24 the zip record (words: start index of the
/// first patch, of the second, two shorts `zipUSup` and `zipUZip`, then `ptDw`, `ptUp`,
/// `rwDw`, `rwUp`). Does nothing when any of those is missing.
///
/// - Both models of kind 1 (patch) or both of kind 10 (NURBS surface): the globals of the
///   zip are set (`zipMdl` the relation's model, the one after it the node's), a copy of the
///   node's surface made (`ptchZip` / `nrbSrfZip`), the limits set (`initMaxValue`
///   / `initNrbSrfMaxValue`), [`refresh_zip`] run with no dialog (or `refreshNrbSrfZip`), the
///   copy disposed, and the node's model touched, found in the scene's list and updated.
///   The second kind test reads the global again, as the original does. The NURBS routines
///   are hooks (not ported).
/// - Otherwise: a message that the relation is deleted, the relation's model relaxed
///   (`SI_edRelaxRelFromDelModel(model, 0xff)`), `sleep(1)`.
pub fn zip_update(m: &mut Mem, o: &mut dyn Outside, node: u32) {
    let rel = m.u32(node + 0x1c);
    if m.u16(rel + 0xc) == 0 {
        return;
    }
    let model = m.u32(node + 8);
    if model == 0 {
        return;
    }
    let other = m.u32(rel + 8);
    if other == 0 {
        return;
    }
    let rec = m.u32(node + 0x24);
    if rec == 0 {
        return;
    }
    let (k1, k2) = (m.i32(model + 0x30), m.i32(other + 0x30));
    if !((k1 == 1 && k2 == 1) || (k1 == 10 && k2 == 10)) {
        o.call(m, &super::tree::STATUS_DISPLAY_MESSAGE, &[RELATION_DELETED, 2]);
        o.call(m, &super::tree::ED_RELAX_REL_FROM_DEL_MODEL, &[other, 0xff]);
        o.call(m, &super::tree::SLEEP, &[1]);
        return;
    }
    m[ZIP_MDL] = other;
    m[ZIP_MDL2] = model;
    m[INDICE_SUP] = m.i32(rec);
    m[INDICE_ZIP] = m.i32(rec + 4);
    m[ZIP_U_SUP] = m.i16(rec + 8);
    m[ZIP_U_ZIP] = m.i16(rec + 10);
    let (pt_dw, pt_up, rw_dw, rw_up) = (m.i32(rec + 0xc), m.i32(rec + 0x10), m.i32(rec + 0x14), m.i32(rec + 0x18));
    m[PT_DW] = pt_dw;
    m[PT_UP] = pt_up;
    m[RW_DW] = rw_dw;
    m[RW_UP] = rw_up;
    m[OLD_PT_DW] = pt_dw;
    m[OLD_PT_UP] = pt_up;
    m[OLD_RW_DW] = rw_dw;
    m[OLD_RW_UP] = rw_up;
    if k1 == 1 {
        let copy = si_spline::patch_image::patch_copy(m, Ptr32::<Patch>::new(m.u32(model + 0x48)));
        m[PTCH_ZIP] = copy.addr();
        let (a, b) = (m.u32(m[ZIP_MDL] + 0x48), m.u32(m[ZIP_MDL2] + 0x48));
        si_spline::zip_image::init_max_value(m, Ptr32::<Patch>::new(a), Ptr32::<Patch>::new(b));
    } else {
        let copy = o.call(m, &NURBS_SRF_COPY, &[m.u32(model + 0x48)]);
        m[NRB_SRF_ZIP] = copy;
        let (a, b) = (m.u32(m[ZIP_MDL] + 0x48), m.u32(m[ZIP_MDL2] + 0x48));
        o.call(m, &INIT_NRB_SRF_MAX_VALUE, &[a, b]);
    }
    if m.i32(m[ZIP_MDL2] + 0x30) == 1 {
        refresh_zip(m, o, 0);
        si_spline::patch_image::patch_dispose(m, PTCH_ZIP.addr());
    } else {
        o.call(m, &REFRESH_NRB_SRF_ZIP, &[0]);
        o.call(m, &NURBS_SRF_DISPOSE, &[NRB_SRF_ZIP.addr()]);
    }
    super::tree::revolution_refresh(m, o, model);
}
