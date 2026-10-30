// SPDX-License-Identifier: MIT
//! Function curves seen from the database side (`dbvsfcv.c`), on the memory image: which
//! relation ties the animation of a chapter's elements to them, and the attribute
//! dictionary's answers about a curve id under the names the dictionary code calls them
//! by. The attribute dictionary is `SI_ATD`; its routines are `si-dict`'s
//! (`si_dict::attdict`).

use si_dict::attdict;
use si_dict::globals::{SI_ATD, SI_CID};
use si_mem::{CStr32, Mem, Ptr32};

/// [soft 0x0810f540 SI_relGetAnimFRelFromChapter]: the relation between an animation
/// file's element and the element it animates, by the animated element's chapter:
/// MODELS 15000, WAVES 15001, CLUSTERS 15002, MATERIALS 15003, SCENES 15004, TEXTURES2D
/// 15005, TEXTURES3D 15006, CAMERAS 15007, LIGHTS 15008, LATTICES 15009, SHAPES 15010;
/// -1 for any other chapter. The chapter is compared with the words of `SI_cid` in the
/// order MODELS, WAVES, SHAPES, CLUSTERS, MATERIALS, TEXTURES2D, TEXTURES3D, LIGHTS,
/// LATTICES, CAMERAS, SCENES: the first match wins, which matters only when two words
/// of `SI_cid` are equal.
pub fn rel_get_anim_f_rel_from_chapter(m: &Mem, chap: i32) -> i32 {
    let cid = &m[SI_CID];
    let table = [
        (cid.model, 15000),
        (cid.wav, 15001),
        (cid.shp, 15010),
        (cid.cls, 15002),
        (cid.mat, 15003),
        (cid.t2d, 15005),
        (cid.t3d, 15006),
        (cid.lig, 15008),
        (cid.lat, 15009),
        (cid.cam, 15007),
        (cid.dsc, 15004),
    ];
    table.iter().find(|(c, _)| *c == chap).map_or(-1, |&(_, r)| r)
}

/// [soft 0x0810f608 SI_fcvGetTypeFromName]: the curve id of a component's name.
pub fn fcv_get_type_from_name(m: &Mem, name: CStr32) -> i32 {
    attdict::get_fcurve_from_name(m, SI_ATD, name)
}

/// [soft 0x0810f624 SI_fcvGetRelGrpFromType]: the relation list (group) of a curve id.
pub fn fcv_get_rel_grp_from_type(m: &mut Mem, id: i32) -> i32 {
    attdict::get_group_from_fcurve(m, SI_ATD, id)
}

/// [soft 0x0810f640 SI_fcvGetNameFromType]: the name of a curve id.
pub fn fcv_get_name_from_type(m: &mut Mem, id: i32) -> CStr32 {
    attdict::get_name_from_fcurve(m, SI_ATD, id)
}

/// [soft 0x0810f65c SI_fcvGetChapidFromType]: the chapter a curve id animates.
pub fn fcv_get_chapid_from_type(m: &mut Mem, id: i32) -> i32 {
    attdict::get_chapid_from_fcurve(m, SI_ATD, id)
}

/// [soft 0x0810f678 SI_fcvGetName]: the name of a curve, by its parameter id (the
/// curve's first word; `curve` is the curve's address).
pub fn fcv_get_name(m: &mut Mem, curve: u32) -> CStr32 {
    let id = m.i32(curve);
    attdict::get_name_from_fcurve(m, SI_ATD, id)
}

/// [soft 0x0810f698 SI_fcvGetRelInfo]: relation id, rule number and count limit of a
/// curve id, in three words of the caller (the group the attribute dictionary also
/// answers goes into a local and is dropped).
pub fn fcv_get_rel_info(m: &mut Mem, id: i32, relid: Ptr32<i32>, rule: Ptr32<i32>, limit: Ptr32<i32>) {
    let group: Ptr32<i32> = Ptr32::new(m.local(4));
    attdict::get_info_from_fcurve(m, SI_ATD, id, relid, group, rule, limit);
    m.release(4);
}

/// [soft 0x0810f6c8 SI_relGetFcvType]: the first curve id of the attribute (relation) id.
pub fn rel_get_fcv_type(m: &Mem, relid: i32) -> i32 {
    attdict::get_fcurve_from_att_id(m, SI_ATD, relid)
}

/// [soft 0x0810f6e4 SI_fcvCheckTypeRange]: 1 when a curve's parameter id (its first
/// word) lies in a range, both ends included; else 0.
pub fn fcv_check_type_range(m: &Mem, curve: u32, lo: i32, hi: i32) -> i32 {
    let id = m.i32(curve);
    (lo <= id && id <= hi) as i32
}

/// The first and last of the relation ids [`rel_get_anim_f_rel_from_chapter`] answers.
pub const REL_ANIM_FIRST: i32 = 15000;
pub const REL_ANIM_LAST: i32 = 15010;
