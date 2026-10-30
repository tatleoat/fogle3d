// SPDX-License-Identifier: MIT
//! The setters of a node's transformation on the image, and what each of them runs before
//! the update: the compensation that keeps sons, geometry and initial poses in place
//! (finding 0015, "Setting a transformation", "Compensation").
//!
//! These are the image forms of routines the crate has in store form in
//! [`crate::transform`] and [`crate::setters`]; the arithmetic is the same (the order of
//! operations, the floors and where a value is rounded were read from the instruction
//! listing and measured there), the record and the stack blocks are the original's. Copies
//! of a whole value are copies of its words, as in the original (`mov`, not `fld`/`fstp`).
//!
//! Every setter stores the local values, sets its update flag bit in both flag words,
//! calls [`compensate_matrix`] with its last two arguments (a mode, of which only 0 and 1
//! are told apart, and a 16-bit flag `geom`) and then
//! `SI_modelUpdateSonState(father, node, 1)`.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x;

use super::state::{chain_node, point_through, update_son_state};
use super::{at, bounds, callee, g, mx, Outside};
use crate::setters::floor_compensation;
use crate::transform::floor_scaling;

/// Sets bits in the low bytes of both update flag words (+0x0a, +0x3fa), as the original
/// does with two byte-wide `or`s.
fn flag(m: &mut Mem, node: Ptr32<Model>, bits: u8) {
    m.or_u8(at!(node, update_flags), bits);
    m.or_u8(at!(node, update_flags2), bits);
}

/// Three words from `src` to `dst`, one after the other.
fn words3(m: &mut Mem, dst: u32, src: u32) {
    for i in 0..3 {
        let w = m.u32(src + 4 * i);
        m.set_u32(dst + 4 * i, w);
    }
}

/// The end every setter shares: the compensation, then the update of the node, its later
/// brothers and its subtree under its father.
fn finish(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, mode: i32, geom: i16) {
    compensate_matrix(m, o, node, mode, geom);
    let father = m[node].father;
    update_son_state(m, o, father, node, 1);
}

/// Sets a node's local scaling [soft 0x08463a74 SI_modelSetScaling]: the three words,
/// flag bit 0x2, the compensation, the update.
pub fn set_scaling(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, scal: u32, mode: i32, geom: i16) {
    words3(m, at!(node, scal), scal);
    flag(m, node, 2);
    finish(m, o, node, mode, geom);
}

/// Sets a node's local rotation angles [soft 0x08463f54 SI_modelSetRotation]: the three
/// words, their rotation matrix into `mrot` (from the caller's vector), flag bit 0x4, the
/// compensation **unless the node is a joint** (kind 5), the update.
pub fn set_rotation(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, rot: u32, mode: i32, geom: i16) {
    words3(m, at!(node, vrot), rot);
    mx::xyz_rotation(m, rot, at!(node, mrot));
    flag(m, node, 4);
    if m[node].kind != 5 {
        compensate_matrix(m, o, node, mode, geom);
    }
    let father = m[node].father;
    update_son_state(m, o, father, node, 1);
}

/// Sets a node's local translation [soft 0x084640cc SI_modelSetTranslation]: the three
/// words, flag bit 0x8, the compensation, the update.
pub fn set_translation(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, trans: u32, mode: i32, geom: i16) {
    words3(m, at!(node, trans), trans);
    flag(m, node, 8);
    finish(m, o, node, mode, geom);
}

/// Sets all three local values [soft 0x084638a8 SI_modelSetState]: bits 0xe in both flag
/// words (the second as a 16-bit `or`), scaling, angles and translation word by word,
/// then the update. No compensation, and `mrot` is left to the update.
pub fn set_state(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, scal: u32, rot: u32, trans: u32) {
    m.or_u8(at!(node, update_flags), 0xe);
    let f = m[node].update_flags2;
    m[node].update_flags2 = f | 0xe;
    words3(m, at!(node, scal), scal);
    words3(m, at!(node, vrot), rot);
    words3(m, at!(node, trans), trans);
    let father = m[node].father;
    update_son_state(m, o, father, node, 1);
}

/// Puts a node at a position given in global terms
/// [soft 0x0846412c SI_modelSetGlobalTranslation]: for a root the three words become the
/// local translation; under a father the position goes through the inverse of the
/// father's `gmat` (`((x·m₀ⱼ + y·m₁ⱼ) + z·m₂ⱼ) + m₃ⱼ`, the three components loaded first).
/// Flag bit 0x8, the compensation, the update.
///
/// The inverse goes into a block of the stack nothing fills first: a father's matrix
/// without one leaves the original computing with what was there (counted,
/// [`crate::env::stale`]).
pub fn set_global_translation(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, pos: u32, mode: i32, geom: i16) {
    let father = m[node].father;
    if father.is_null() {
        words3(m, at!(node, trans), pos);
    } else {
        let inverse = m.local(0x40);
        if !mx::inverse(m, at!(father, gmat), inverse) {
            crate::env::stale::note();
        }
        point_through(m, pos, inverse, at!(node, trans));
        m.release(0x40);
    }
    flag(m, node, 8);
    finish(m, o, node, mode, geom);
}

/// Gives a node an orientation given in global terms
/// [soft 0x08464224 SI_modelSetGlobalRotation]: for a root the three words become the
/// local angles; under a father the angles are taken out of R(angles) × (father's
/// `grot`)ᵀ (the full 4 × 4 product) with the node's present angles as the previous
/// solution. Flag bit 0x4, the compensation (a joint too), the update.
pub fn set_global_rotation(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, rot: u32, mode: i32, geom: i16) {
    let father = m[node].father;
    if father.is_null() {
        words3(m, at!(node, vrot), rot);
    } else {
        let transposed = m.local(0x40);
        let rotation = m.local(0x40);
        let product = m.local(0x40);
        mx::transpose(m, at!(father, grot), transposed);
        mx::xyz_rotation(m, rot, rotation);
        mx::multiply(m, rotation, transposed, product);
        mx::extract_xyz_rotation(m, product, at!(node, vrot));
        m.release(0xc0);
    }
    flag(m, node, 4);
    finish(m, o, node, mode, geom);
}

/// Gives a node a scaling given in global terms [soft 0x084642fc SI_modelSetGlobalScaling].
/// For a root the three words are stored. Under a father, by the classical method
/// (`SI_glbManip` = 1) the row lengths of S(factors) × (father's `gmat`)⁻¹; by the usual
/// one each factor ÷ the node's `hscl`, a divisor smaller than 1e-5 taken as the float
/// nearest ±1e-5 (the three divisors first, then the three quotients, each rounded on its
/// store). Flag bit 0x2, the compensation, the update.
pub fn set_global_scaling(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, scal: u32, mode: i32, geom: i16) {
    let father = m[node].father;
    if father.is_null() {
        words3(m, at!(node, scal), scal);
    } else if m[g::SI_GLB_MANIP].scaling_method == 1 {
        let inverse = m.local(0x40);
        let scaling = m.local(0x40);
        let product = m.local(0x40);
        if !mx::inverse(m, at!(father, gmat), inverse) {
            crate::env::stale::note();
        }
        mx::scaling(m, scal, scaling);
        mx::multiply(m, scaling, inverse, product);
        mx::extract_scaling(m, product, at!(node, scal));
        m.release(0xc0);
    } else {
        let h = at!(node, hscl);
        let d = [floor_scaling(m.f32(h)), floor_scaling(m.f32(h + 4)), floor_scaling(m.f32(h + 8))];
        for (i, d) in d.into_iter().enumerate() {
            let i = i as u32;
            let v = (x(m.f32(scal + 4 * i)) / d).to_f32();
            m.set_f32(at!(node, scal) + 4 * i, v);
        }
    }
    flag(m, node, 2);
    finish(m, o, node, mode, geom);
}

/// A node's global scaling, angles and position [soft 0x084637dc SI_modelGlobalState].
/// By the classical method: (1, 1, 1), (0, 0, 0) and row 3 of `gmat`. By the usual one:
/// `gscl`; the local angles, then the angles taken out of `grot` with those as the
/// previous solution; `gtrs`. Every store is made in that order, so a caller that passes
/// one block for the scaling and the angles (as `SI_modelKineStateCompute` does) gets the
/// angles in it.
pub fn global_state(m: &mut Mem, node: Ptr32<Model>, scal: u32, rot: u32, trans: u32) {
    if m[g::SI_GLB_MANIP].scaling_method == 1 {
        for i in 0..3 {
            m.set_u32(scal + 4 * i, 0x3f80_0000);
        }
        for i in 0..3 {
            m.set_u32(rot + 4 * i, 0);
        }
        words3(m, trans, at!(node, gmat) + 0x30);
    } else {
        words3(m, scal, at!(node, gscl));
        words3(m, rot, at!(node, vrot));
        mx::extract_xyz_rotation(m, at!(node, grot), rot);
        words3(m, trans, at!(node, gtrs));
    }
}

/// The scaling floor of the compensation: each factor of the node's `gscl`, one smaller
/// than the double 0.0001 replaced by the float nearest ±0.0001 (0x0865fd10, 0x0865fd18,
/// 0x0865fd1c; `compensateInit_srt` has its own copies at 0x0865fd20 …).
fn floors(m: &Mem, node: Ptr32<Model>) -> [f32; 3] {
    let s = at!(node, gscl);
    [floor_compensation(m.f32(s)), floor_compensation(m.f32(s + 4)), floor_compensation(m.f32(s + 8))]
}

/// A son that is an effector (algorithm 3) under a node whose inverse-kinematics record
/// has a kind other than 0 keeps its angles. Read through the son's own `father` link and
/// that node's `inv_kin` pointer without a test, as the original reads them.
fn keeps_angles(m: &Mem, son: Ptr32<Model>) -> bool {
    if m[son].algo_type != 3 {
        return false;
    }
    let ik = m[m[son].father].inv_kin;
    m.i32(ik.addr()) != 0
}

/// Under hierarchy manipulation (`SI_glbManip` + 0x1e): whether the son is selected on
/// screen, by `SI_findGlData(SI_grafLists + 0x24 × (the process's list number), son, 3)`
/// answering a node whose word at +8 is 1.
fn selected_on_screen(m: &mut Mem, o: &mut dyn Outside, son: Ptr32<Model>) -> bool {
    if m[g::SI_GLB_MANIP].hierarchy_manipulation == 0 {
        return false;
    }
    let list = m.i32(m[g::SI_PROC_DATA].addr() + 4);
    let lists = m[g::SI_GRAF_LISTS].wrapping_add((list as u32).wrapping_mul(0x24));
    let gn = o.call(m, &callee::SI_FIND_GL_DATA, &[lists, son.addr(), 3]);
    gn != 0 && m.i32(gn + 8) == 1
}

/// Keeps what hangs on a node in place while its local values change
/// [soft 0x08465660 SI_modelCompensateMatrix]: called by the setters after the new values
/// are stored and before the update. Nothing unless `mode` is 0 or `geom` is set. Then:
///
/// 1. with `SI_initPoseCompensate`, the sons' initial poses are noted in global terms
///    ([`compute_init_srt_global_transfo`]);
/// 2. the node's old `gmat` is copied, the node alone updated
///    (`SI_modelUpdateSonState(father, node, 0)`), and the new `gmat` inverted;
/// 3. with `geom`: old × new⁻¹ goes to `SI_modelMultiplyDefinition`
///    ([`super::touch::multiply_definition`]) and `SI_clustersApplyTransformation`, and the
///    box is recomputed
///    ([`bounds::compute_bounding`]): the geometry stays where it was;
/// 4. with `mode` 0: every son's 16-bit word at +8 is **set to** 0x2000, then bit 0x2000 is
///    cleared in every node a son follows as its path (+0x2c8); a son selected on screen
///    under hierarchy manipulation is compensated in turn (this routine with `mode`, 0, and
///    `geom`); every other son that still has the bit gets local values that keep its
///    global ones:
///    - classical method: its `gmat` × (the node's `gmat`)⁻¹ taken apart into its scaling,
///      angles (its present ones as the previous solution) and translation; the angles
///      and their `mrot` are stored unless it keeps them (an effector, [`keeps_angles`]),
///      the translation unless under hierarchy manipulation; and for every son not
///      compensated in turn, when neither node nor son belongs to a skeleton chain,
///      `SI_modelCompensateAngle(product, son)` with the block of the last product;
///    - usual method: scaling = its `gscl` ÷ the node's (floored at 0.0001, [`floors`]),
///      `mrot` = its `grot` × (the node's `grot`)⁻¹ and the angles taken out of it (unless
///      it keeps them), translation = its `gtrs` through the inverse of step 2 (unless
///      under hierarchy manipulation);
/// 5. with `SI_initPoseCompensate`, [`compensate_init_srt`].
///
/// The inverses go into blocks of the stack nothing fills first, and the classical
/// method's product block keeps what an earlier product (or step 3) left: a use of a block
/// the original never filled is counted ([`crate::env::stale`]).
pub fn compensate_matrix(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, mode: i32, geom: i16) {
    if mode != 0 && geom == 0 {
        return;
    }
    if m[g::SI_INIT_POSE_COMPENSATE] != 0 {
        let first = m[node].son;
        compute_init_srt_global_transfo(m, o, node, first);
    }
    let sp = m.sp();
    let old = m.local(0x40);
    let inverse = m.local(0x40);
    let product = m.local(0x40);
    let angles = m.local(12);
    let trs = m.local(12);
    mx::copy(m, at!(node, gmat), old);
    let father = m[node].father;
    update_son_state(m, o, father, node, 0);
    let have_inverse = mx::inverse(m, at!(node, gmat), inverse);
    let mut product_written = false;
    if geom != 0 {
        if !have_inverse {
            crate::env::stale::note();
        }
        mx::multiply(m, old, inverse, product);
        product_written = true;
        super::touch::multiply_definition(m, o, node, product);
        o.call(m, &callee::SI_CLUSTERS_APPLY_TRANSFORMATION, &[node.addr(), product]);
        bounds::compute_bounding(m, node);
    }
    if mode == 0 {
        let mut s = m[node].son;
        while !s.is_null() {
            m.set_u16(s.addr() + 8, 0x2000);
            s = m[s].brother;
        }
        let mut s = m[node].son;
        while !s.is_null() {
            let path = m[s].path.addr();
            if path != 0 {
                let w = m.u16(path + 8);
                m.set_u16(path + 8, w & 0xdfff);
            }
            s = m[s].brother;
        }
        let hierarchy = |m: &Mem| m[g::SI_GLB_MANIP].hierarchy_manipulation != 0;
        if m[g::SI_GLB_MANIP].scaling_method == 1 {
            let inverse2 = m.local(0x40);
            let have_inverse2 = mx::inverse(m, at!(node, gmat), inverse2);
            let mut s = m[node].son;
            while !s.is_null() {
                if selected_on_screen(m, o, s) {
                    compensate_matrix(m, o, s, mode, geom);
                } else {
                    if m.u8(s.addr() + 9) & 0x20 != 0 {
                        if !have_inverse2 {
                            crate::env::stale::note();
                        }
                        mx::multiply(m, at!(s, gmat), inverse2, product);
                        product_written = true;
                        words3(m, angles, at!(s, vrot));
                        mx::extract_transformations(m, product, at!(s, scal), angles, trs);
                        if !keeps_angles(m, s) {
                            words3(m, at!(s, vrot), angles);
                            mx::xyz_rotation(m, angles, at!(s, mrot));
                        }
                        if !hierarchy(m) {
                            words3(m, at!(s, trans), trs);
                        }
                    }
                    if !chain_node(m, node) && !chain_node(m, s) {
                        if !product_written {
                            crate::env::stale::note();
                        }
                        o.call(m, &callee::SI_MODEL_COMPENSATE_ANGLE, &[product, s.addr()]);
                    }
                }
                s = m[s].brother;
            }
        } else {
            let inverse_rot = m.local(0x40);
            let have_inverse_rot = mx::inverse(m, at!(node, grot), inverse_rot);
            let mut s = m[node].son;
            while !s.is_null() {
                if selected_on_screen(m, o, s) {
                    compensate_matrix(m, o, s, mode, geom);
                } else if m.u8(s.addr() + 9) & 0x20 != 0 {
                    let d = floors(m, node);
                    for (i, d) in d.into_iter().enumerate() {
                        let i = i as u32;
                        let v = (x(m.f32(at!(s, gscl) + 4 * i)) / d).to_f32();
                        m.set_f32(at!(s, scal) + 4 * i, v);
                    }
                    if !keeps_angles(m, s) {
                        if !have_inverse_rot {
                            crate::env::stale::note();
                        }
                        mx::multiply_sub3(m, at!(s, grot), inverse_rot, at!(s, mrot));
                        mx::extract_xyz_rotation(m, at!(s, mrot), at!(s, vrot));
                    }
                    if !hierarchy(m) {
                        if !have_inverse {
                            crate::env::stale::note();
                        }
                        point_through(m, at!(s, gtrs), inverse, at!(s, trans));
                    }
                }
                s = m[s].brother;
            }
        }
    }
    m.set_sp(sp);
    if m[g::SI_INIT_POSE_COMPENSATE] != 0 {
        let first = m[node].son;
        compensate_init_srt(m, node, first);
    }
}

/// Notes where the initial poses of a node's sons lie in global terms
/// [soft 0x08465c10 computeInit_srtGlobalTransfo]. The sons from `first` on that have an
/// initial transformation (+0x328, nine floats: scaling, angles, translation) are counted;
/// with none nothing happens (and `glbInit_srt` keeps what it held). Otherwise a block of
/// 0x58 bytes a son (`SI_malloc`) becomes `glbInit_srt`, and each such son in turn is put
/// into its initial pose (the six words of scaling and angles, `mrot` from the angles,
/// the translation), updated under `father` alone, its `gscl`, `grot` and `gtrs` written
/// to the next entry, and then put back (words, and `mrot` from a copy) and updated again.
pub fn compute_init_srt_global_transfo(m: &mut Mem, o: &mut dyn Outside, father: Ptr32<Model>, first: Ptr32<Model>) {
    let mut count = 0i32;
    let mut s = first;
    while !s.is_null() {
        if !m[s].init_srt.is_null() {
            count += 1;
        }
        s = m[s].brother;
    }
    if count <= 0 {
        return;
    }
    let mut entry = m.si_malloc((count as u32).wrapping_mul(0x58));
    m[g::GLB_INIT_SRT] = entry;
    let saved_mrot = m.local(0x40);
    let saved = m.local(24);
    let mut s = first;
    while !s.is_null() {
        if !m[s].init_srt.is_null() {
            words3(m, saved, at!(s, scal));
            words3(m, saved + 12, at!(s, vrot));
            mx::copy(m, at!(s, mrot), saved_mrot);
            let t = [m.u32(at!(s, trans)), m.u32(at!(s, trans) + 4), m.u32(at!(s, trans) + 8)];
            let p = m[s].init_srt.addr();
            words3(m, at!(s, scal), p);
            words3(m, at!(s, vrot), p + 12);
            let p = m[s].init_srt.addr();
            mx::xyz_rotation(m, p + 12, at!(s, mrot));
            let p = m[s].init_srt.addr();
            words3(m, at!(s, trans), p + 24);
            update_son_state(m, o, father, s, 0);
            words3(m, entry, at!(s, gscl));
            mx::copy(m, at!(s, grot), entry + 12);
            words3(m, entry + 0x4c, at!(s, gtrs));
            words3(m, at!(s, scal), saved);
            words3(m, at!(s, vrot), saved + 12);
            mx::copy(m, saved_mrot, at!(s, mrot));
            for (i, w) in t.into_iter().enumerate() {
                m.set_u32(at!(s, trans) + 4 * i as u32, w);
            }
            update_son_state(m, o, father, s, 0);
            entry = entry.wrapping_add(0x58);
        }
        s = m[s].brother;
    }
    m.release(24);
    m.release(0x40);
}

/// Notes the global scaling, rotation and position of a node and its following brothers as
/// they are, for re-parenting [soft 0x0846613c computeInit_srtParenting]: with at least one
/// of them having an initial transformation, a block of 0x58 bytes for each **such** node
/// (`SI_malloc`) becomes `glbInit_srt`, and then an entry is written for **every** node of
/// the chain, `gscl`, `grot` (`SI_matrixCopy`) and `gtrs`: past the end of the block when
/// some have none, as in the original (the image's heap holds whatever lies behind).
pub fn compute_init_srt_parenting(m: &mut Mem, first: Ptr32<Model>) {
    let mut count = 0i32;
    let mut s = first;
    while !s.is_null() {
        if !m[s].init_srt.is_null() {
            count += 1;
        }
        s = m[s].brother;
    }
    if count <= 0 {
        return;
    }
    let mut entry = m.si_malloc((count as u32).wrapping_mul(0x58));
    m[g::GLB_INIT_SRT] = entry;
    let mut s = first;
    while !s.is_null() {
        words3(m, entry, at!(s, gscl));
        mx::copy(m, at!(s, grot), entry + 12);
        words3(m, entry + 0x4c, at!(s, gtrs));
        entry = entry.wrapping_add(0x58);
        s = m[s].brother;
    }
}

/// Rewrites the initial transformations of a node's sons so that the poses noted in
/// `glbInit_srt` hold under the node as it is now [soft 0x08465e3c compensateInit_srt],
/// then frees the list (when there is one) and clears the global.
///
/// The node's `grot` and `gmat` are inverted first. The entries are taken in order, one
/// for each son from `first` on that has an initial transformation (the list is read
/// without a test: a son with a pose and no list reads through null, as in the original):
///
/// - classical method: the angles of the entry's rotation, taken out with the angles of
///   the son before as the previous solution (for the first son a block of the stack
///   nobody wrote: counted), give S·R·T of the entry, which × (`gmat`)⁻¹ is taken apart
///   into the son's initial scaling, angles and translation;
/// - usual method: scaling = the entry's ÷ the node's `gscl` (floored at 0.0001: the
///   constants at 0x0865fd20 …), the angles of the entry's rotation × (`grot`)⁻¹ with the
///   initial angles as the previous solution, the position through (`gmat`)⁻¹.
pub fn compensate_init_srt(m: &mut Mem, node: Ptr32<Model>, first: Ptr32<Model>) {
    let sp = m.sp();
    let inverse_rot = m.local(0x40);
    let inverse = m.local(0x40);
    let angles = m.local(12);
    let srt = m.local(0x40);
    let product = m.local(0x40);
    let rotation = m.local(0x40);
    let have_inverse_rot = mx::inverse(m, at!(node, grot), inverse_rot);
    let have_inverse = mx::inverse(m, at!(node, gmat), inverse);
    let mut entry = m[g::GLB_INIT_SRT];
    let mut first_angles = true;
    let mut s = first;
    while !s.is_null() {
        if !m[s].init_srt.is_null() {
            if m[g::SI_GLB_MANIP].scaling_method == 1 {
                if first_angles {
                    // the previous solution: a block of the stack nobody wrote
                    crate::env::stale::note();
                    first_angles = false;
                }
                mx::extract_xyz_rotation(m, entry + 12, angles);
                mx::scal_rot_trans(m, entry, angles, entry + 0x4c, srt);
                if !have_inverse {
                    crate::env::stale::note();
                }
                mx::multiply(m, srt, inverse, product);
                let p = m[s].init_srt.addr();
                mx::extract_transformations(m, product, p, p + 12, p + 24);
            } else {
                let d = floors(m, node);
                for (i, d) in d.into_iter().enumerate() {
                    let i = i as u32;
                    let v = (x(m.f32(entry + 4 * i)) / d).to_f32();
                    let p = m[s].init_srt.addr();
                    m.set_f32(p + 4 * i, v);
                }
                if !have_inverse_rot || !have_inverse {
                    crate::env::stale::note();
                }
                mx::multiply_sub3(m, entry + 12, inverse_rot, rotation);
                let p = m[s].init_srt.addr();
                mx::extract_xyz_rotation(m, rotation, p + 12);
                let p = m[s].init_srt.addr();
                point_through(m, entry + 0x4c, inverse, p + 24);
            }
            entry = entry.wrapping_add(0x58);
        }
        s = m[s].brother;
    }
    m.set_sp(sp);
    let list = m[g::GLB_INIT_SRT];
    if list != 0 {
        m.free(list);
        m[g::GLB_INIT_SRT] = 0;
    }
}
