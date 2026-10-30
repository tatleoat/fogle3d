// SPDX-License-Identifier: MIT
//! The routines of other source files `sys_camera.c` calls, as typed calls on
//! [`si_mem::Outside`] (one line each: arguments, result, name, address).
//!
//! Nothing here reproduces a function of the original. Mathematics on values is not here:
//! it is called directly.

use si_mem::outside_calls;

outside_calls! {
    /// What `SI_camMat` calls outside its file.
    pub trait CamCalls {
        /// `edtools.c`: the element of a camera record (or of its interest) in the current
        /// dictionary; its index goes to `*index`.
        fn ed_get_elem_from_camera(camera: u32, index: u32) -> u32 = "SI_edGetElemFromCamera", 0x080fcfb0;
        /// `elemdict.c`: the first element related to `elem` by `relid` in its list `list`
        /// (`dir` 1: forward), of chapter `chap` (−1: any); its chapter and index go to
        /// `*out_chap`, `*out_index`.
        fn elemdict_find_related_elem(ed: u32, elem: u32, dir: i16, list: i32, relid: i32, chap: i32, out_chap: u32, out_index: u32) -> u32 = "SI_elemdictFindRelatedElem", 0x0813508c;
        /// `elemdict.c`: the relation entry the last search stopped at.
        fn elemdict_get_cur_relation(ed: u32, elem: u32, dir: i16, list: i32) -> u32 = "SI_elemdictGetCurRelation", 0x08134db0;
        /// `edanim.c`: the activation curve of a relation entry, or null.
        fn ed_get_fcv_from_relation(rel: u32) -> u32 = "SI_edGetFcvFromRelation", 0x080ee6f0;
        /// `fcurve.c`: a curve's value at a time as an integer (`check`, `as_flag`).
        fn fcurve_integer_value(curve: u32, time: f32, check: i16, as_flag: i16) -> i32 = "SI_fcurveIntegerValue", 0x08200404;
    }
}

