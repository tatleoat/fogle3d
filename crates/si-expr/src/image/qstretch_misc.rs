// SPDX-License-Identifier: MIT
//! `qstretch_misc.c` on the image (finding 0013, section 18): the comparison of two quick-stretch
//! data records and the check that a model carries the four kinematic-state curve triplets.

use si_mem::{outside_calls, Mem, Outside};

outside_calls! {
    /// The callees of `qstretch_misc.c` that are not on this crate's image form (each is ported
    /// elsewhere; the routing is the host's).
    pub trait QsMiscCalls {
        /// `edtools.c`: a model's element in the MODELS chapter; its index goes to `*index`.
        fn ed_get_elem_from_model(model: u32, index: u32) -> u32 = "SI_edGetElemFromModel", 0x080fcf20;
        /// `reldict.c`: a relation's source chapter (negative when unknown), its list in `*group`.
        fn reldict_get_rel_chap_grp(rd: u32, relid: i32, group: u32) -> i32 = "SI_reldictGetRelChapGrp", 0x0813a464;
        /// `motionUtils.c`: an element's three curves of a relation, by kind.
        fn get_fcv_triplet(elem: u32, c1: u32, c2: u32, c3: u32, group: i32, relid: i32) = "SI_getFcvTriplet", 0x0822efcc;
    }
}

/// `SI_RD`, the relation dictionary.
const SI_RD: u32 = 0x087815f0;

/// Whether two quick-stretch data records are equal [soft 0x085a2cd4 SI_QSequal]: twelve
/// groups of 0x18 bytes, each a 16-bit word at +0, +4 and +6, a 32-bit word at +8 and two
/// floats at +0xc and +0x10 (floats compare as numbers: a NaN is never equal, +0 equals −0,
/// so the test is `==` on `f32`, not on the bits), then the 32-bit word at +0x120 and the
/// 16-bit word at +0x124. The original tests in stages (all `+0xc` floats, then all `+0x10`
/// floats, the words at +0, +4, then +6/+8); nothing is written, so the order does not show.
/// Answers 1 when all are equal, else 0.
pub fn qs_equal(m: &Mem, a: u32, b: u32) -> u32 {
    for g in 0..12u32 {
        let o = 0x18 * g;
        if m.f32(a + o + 0xc) != m.f32(b + o + 0xc) || m.f32(a + o + 0x10) != m.f32(b + o + 0x10) {
            return 0;
        }
        if m.u16(a + o) != m.u16(b + o) || m.u16(a + o + 4) != m.u16(b + o + 4) || m.u16(a + o + 6) != m.u16(b + o + 6) || m.u32(a + o + 8) != m.u32(b + o + 8) {
            return 0;
        }
    }
    (m.u32(a + 0x120) == m.u32(b + 0x120) && m.u16(a + 0x124) == m.u16(b + 0x124)) as u32
}

/// Whether the model's element has all three curves of relation `relid`
/// [soft 0x085a3220 fcurveCheckTriplet]: the element (`SI_edGetElemFromModel`), none, 0;
/// the relation's chapter and group (`SI_reldictGetRelChapGrp` on `SI_RD`), negative, 0;
/// else `SI_getFcvTriplet` and 1 when all three curves came back.
pub fn fcurve_check_triplet(m: &mut Mem, o: &mut dyn Outside, model: u32, relid: i32) -> u32 {
    let sp = m.sp();
    let index = m.local(4);
    let elem = o.ed_get_elem_from_model(m, model, index);
    if elem == 0 {
        m.set_sp(sp);
        return 0;
    }
    let group = m.local(4);
    if o.reldict_get_rel_chap_grp(m, SI_RD, relid, group) < 0 {
        m.set_sp(sp);
        return 0;
    }
    let c3 = m.local(4);
    let c2 = m.local(4);
    let c1 = m.local(4);
    let g = m.i32(group);
    o.get_fcv_triplet(m, elem, c1, c2, c3, g, relid);
    let r = m.u32(c1) != 0 && m.u32(c2) != 0 && m.u32(c3) != 0;
    m.set_sp(sp);
    r as u32
}

/// Whether the model has the four triplets of relations 90 … 93 (`LinVel`, `LinAcc`, `AngVel`,
/// `AngAcc`) [soft 0x085a329c si_fcurveKinStateCheckAll]: [`fcurve_check_triplet`] in that
/// order, stopping at the first that fails. Answers 1 or 0.
pub fn fcurve_kin_state_check_all(m: &mut Mem, o: &mut dyn Outside, model: u32) -> u32 {
    for relid in 0x5a..=0x5d {
        if fcurve_check_triplet(m, o, model, relid) == 0 {
            return 0;
        }
    }
    1
}
