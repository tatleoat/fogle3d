// SPDX-License-Identifier: MIT
//! The twelve routines of `edtools.c` that `edtools2` left open (finding 0012, "edtools3,
//! 2026-10-29"), on the memory image: the element adders for cameras, lights, clusters, shapes
//! and lattices, the shader replacer, the relation-list text, the joint extra relation, the
//! extra-camera remover and the editable / uneditable marks.
//!
//! As in `edtools2`, the element dictionary's own routines, the name records, the schematic and
//! the animation / expression removers belong to other files and are called through
//! [`si_mem::Outside`] by address. The current dictionary is `SI_elemDicts + 0x70 * *SI_procData`.
//! `SI_RD` (0x087815f0) is the relation dictionary, passed by address.
//!
//! The C library calls are exact: `sprintf` into the shared `workStr` buffer is [`sprintf_work`]
//! (glibc prints a null `%s` as `(null)`), `strdup` and `strcpy` are the image's.

use crate::edtools2::{wrath_unsupported_mdl_type, Ed2Calls};
use si_mem::{Mem, Outside};

si_mem::outside_calls! {
    /// What these twelve routines call that `edtools2` does not already.
    pub trait Ed3Calls {
        /// `SI_elemdictGetNewName (0x0813697c)`: `(dict, chapter, name)`; a name not yet in use.
        fn elemdict_get_new_name(dict: u32, cid: u32, name: u32) -> u32 = "SI_elemdictGetNewName", 0x0813697c;
        /// `SI_elemdictUpdateHierInfo (0x08134cc8)`: `(dict, chapter, 1, relation id)`.
        fn elemdict_update_hier_info(dict: u32, cid: u32, one: u32, id: u32) = "SI_elemdictUpdateHierInfo", 0x08134cc8;
        /// `SI_schemComputeHierarchiesBBox (0x0833bd74)`: `(dict, 0, 0)`.
        fn schem_compute_hierarchies_bbox(dict: u32, a: u32, b: u32) = "SI_schemComputeHierarchiesBBox", 0x0833bd74;
        /// `SI_elemdictSetDirty (0x08132be8)`: `(element, flags as an int)`.
        fn elemdict_set_dirty(elem: u32, flags: u32) = "SI_elemdictSetDirty", 0x08132be8;
        /// `SI_udfAttachArrayElements (0x08545584)`.
        fn udf_attach_array_elements(udf: u32) = "SI_udfAttachArrayElements", 0x08545584;
        /// `SI_shaderFcurveUpdateFromDialog (0x080d75f8)`: `(old, new)`.
        fn shader_fcurve_update_from_dialog(old: u32, new: u32) = "SI_shaderFcurveUpdateFromDialog", 0x080d75f8;
        /// `SI_reldictGetRelAttributes (0x0813a28c)`: `(&SI_RD, id, 1, 0, &a, &b, &c, &d, &e, &f)`; nonzero when found.
        fn reldict_get_rel_attributes(rd: u32, id: u32, one: u32, zero: u32, a: u32, b: u32, c: u32, d: u32, e: u32, f: u32) -> i16 = "SI_reldictGetRelAttributes", 0x0813a28c;
        /// `SI_elemdictFindRelatedElemSerial (0x08135554)`: `(dict, elem, 1, attribute, id, index, &chapter, &number, pass-through)`.
        fn elemdict_find_related_elem_serial(dict: u32, elem: u32, one: u32, attr: u32, id: u32, index: u32, out_a: u32, out_b: u32, p: u32) -> u32 = "SI_elemdictFindRelatedElemSerial", 0x08135554;
        /// `SI_elemdictFindNextRelatedElemSerial (0x08135620)`: the same arguments.
        fn elemdict_find_next_related_elem_serial(dict: u32, elem: u32, one: u32, attr: u32, id: u32, index: u32, out_a: u32, out_b: u32, p: u32) -> u32 = "SI_elemdictFindNextRelatedElemSerial", 0x08135620;
        /// `SI_strAppend (0x083cf280)`: `(&string, text)`.
        fn str_append(string: u32, text: u32) = "SI_strAppend", 0x083cf280;
        /// `SI_elemdictSerialRelCount (0x0813590c)`: `(dict, elem, id, 0, 0)`.
        fn elemdict_serial_rel_count(dict: u32, elem: u32, id: u32, a: u32, b: u32) -> i32 = "SI_elemdictSerialRelCount", 0x0813590c;
        /// `SI_edGetElemFromScene (0x080fd620)`: `(&index)`; the scene's element.
        fn ed_get_elem_from_scene(index: u32) -> u32 = "SI_edGetElemFromScene", 0x080fd620;
        /// `SI_relGetAnimFRelFromChapter (0x0810f540)`.
        fn rel_get_anim_f_rel_from_chapter(cid: u32) -> u32 = "SI_relGetAnimFRelFromChapter", 0x0810f540;
        /// `SI_schematicNoRefresh (0x0832bfd0)`.
        fn schematic_no_refresh(on: u32) = "SI_schematicNoRefresh", 0x0832bfd0;
        /// `SI_edRemoveElemAnim (0x08221190)`: `(dict, chapter, elem, number, 1)`.
        fn ed_remove_elem_anim(dict: u32, cid: u32, elem: u32, number: u32, one: u32) = "SI_edRemoveElemAnim", 0x08221190;
        /// `SI_edRemoveExpressionsFromArg (0x08255ac0)`: `(dict, elem)`.
        fn ed_remove_expressions_from_arg(dict: u32, elem: u32) = "SI_edRemoveExpressionsFromArg", 0x08255ac0;
        /// `SI_edRemoveElemExpressions (0x082559d0)`: `(dict, elem, chapter, number)`.
        fn ed_remove_elem_expressions(dict: u32, elem: u32, cid: u32, number: u32) = "SI_edRemoveElemExpressions", 0x082559d0;
        /// `edGetConstrainingItem (0x08106a70)`: `(dict, model, chapter, list, relation, first, &kind)`; the item or null.
        fn ed_get_constraining_item(dict: u32, model: u32, cid: u32, list: u32, rel: u32, first: u32, kind: u32) -> u32 = "edGetConstrainingItem", 0x08106a70;
    }
}

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const SI_ADD_IN_DICTIONARY: u32 = 0x0867_92b0;
const SI_STANDALONE_RENDERER: u32 = 0x0867_9400;
const SI_GLB_SCHEM: u32 = 0x087f_b960;
/// `workStr`, the shared 'sprintf' buffer.
const WORK_STR: u32 = 0x0878_4f80;
/// `SI_RD`, the relation dictionary, passed by address.
const SI_RD: u32 = 0x0878_15f0;

/// `SI_cid` words (each a 32-bit chapter id in the data segment, read when used).
mod cid {
    pub const ANIMATION: u32 = 0x0878_1660;
    pub const CLUSTERS: u32 = 0x0878_1668;
    pub const CAMERAS: u32 = 0x0878_1664;
    pub const LATTICES: u32 = 0x0878_1674;
    pub const LIGHTS: u32 = 0x0878_1678;
    pub const MATERIALS: u32 = 0x0878_167c;
    pub const MODELS: u32 = 0x0878_1680;
    pub const CID_1684: u32 = 0x0878_1684;
    pub const MATERIAL_SHADERS: u32 = 0x0878_1690;
    pub const TEXTURE2D_SHADERS: u32 = 0x0878_1694;
    pub const TEXTURE3D_SHADERS: u32 = 0x0878_1698;
    pub const VOLUME_SHADERS: u32 = 0x0878_169c;
    pub const SHADOW_SHADERS: u32 = 0x0878_16a0;
    pub const LIGHT_SHADERS: u32 = 0x0878_16a4;
    pub const CAMERA_SHADERS: u32 = 0x0878_16a8;
    pub const OUTPUT_SHADERS: u32 = 0x0878_16ac;
    pub const ENVIRON_SHADERS: u32 = 0x0878_16b0;
    pub const SHAPES: u32 = 0x0878_16b4;
    pub const TEXTURES2D: u32 = 0x0878_16c8;
    pub const TEXTURES3D: u32 = 0x0878_16cc;
    pub const WAVES: u32 = 0x0878_16d0;
    pub const PHOTON_SHADERS: u32 = 0x0878_16e8;
    pub const LIGHT_PHOTON_SHADERS: u32 = 0x0878_16ec;
}

/// The current scene's dictionary: `SI_elemDicts + 0x70 * *SI_procData`.
fn cur_dict(m: &mut Mem) -> u32 {
    let p = m.u32(m.u32(SI_PROC_DATA));
    p.wrapping_mul(7).wrapping_mul(16).wrapping_add(m.u32(SI_ELEM_DICTS))
}

/// One piece of a `sprintf` argument list.
pub enum Arg {
    /// `%s` with the address of a string (null prints `(null)`, as glibc does).
    S(u32),
    /// `%c`.
    C(u8),
}

/// `sprintf(workStr, format, ...)` for formats made of `%s` and `%c` and literal text: the bytes
/// and a terminator are stored at `workStr`. Answers the length.
pub fn sprintf_work(m: &mut Mem, pieces: &[Result<&[u8], Arg>]) -> u32 {
    let mut out: Vec<u8> = Vec::new();
    for p in pieces {
        match p {
            Ok(lit) => out.extend_from_slice(lit),
            Err(Arg::S(a)) => {
                if *a == 0 {
                    out.extend_from_slice(b"(null)");
                } else {
                    let n = si_mem::clib::strlen(m, *a);
                    out.extend_from_slice(m.bytes(*a, n));
                }
            }
            Err(Arg::C(c)) => out.push(*c),
        }
    }
    let n = out.len() as u32;
    out.push(0);
    m.set_bytes(WORK_STR, &out);
    n
}

/// A name record for `text`: `SI_dbListNodeAllocate`, `+0x14` a copy of the text, `+0x24` the full name.
fn name_record(m: &mut Mem, o: &mut dyn Outside, text: u32) -> u32 {
    use Ed2Calls as _;
    let rec = o.db_list_node_allocate(m);
    let copy = m.strdup(text);
    m.set_u32(rec.wrapping_add(0x14), copy);
    let full = o.db_format_full_name(m, rec);
    m.set_u32(rec.wrapping_add(0x24), full);
    rec
}

/// `SI_elemdictAddElem(current dictionary, chapter, node, data, graph node, 0, 0)`.
fn add_elem(m: &mut Mem, o: &mut dyn Outside, cid_value: u32, rec: u32, data: u32, gn: u32) -> u32 {
    use Ed2Calls as _;
    let d = cur_dict(m);
    o.elemdict_add_elem(m, d, cid_value, rec, data, gn, 0, 0)
}

/// `SI_elemdictAddRelation(current dictionary, &SI_RD, from, to, id, 1, 0)`.
fn add_relation(m: &mut Mem, o: &mut dyn Outside, from: u32, to: u32, id: u32) {
    use Ed2Calls as _;
    let d = cur_dict(m);
    o.elemdict_add_relation(m, d, SI_RD, from, to, id, 1, 0);
}

/// [soft 0x08101b08 SI_edAddCamera]: nothing while `SI_addInDictionary` is 0 or when the camera
/// (the +4 of the first node) is not the scene's current one (`*(*SI_procData + 8)`). With
/// `rename` the camera's name (the text its first word points to) is freed and made a new name from
/// "Camera". Then the camera and its interest ("cam_int" name) get name records and elements in
/// the cameras chapter, related by 0x456 (side 1); the hierarchy info is updated; the interest's
/// element gets (0, 0) at +0xc/+0x10, the camera's element the schematic offset (0, -2.0) or, with
/// `SI_glbSchem` set, (3.5, 0); the schematic bounding box is recomputed.
pub fn ed_add_camera(m: &mut Mem, o: &mut dyn Outside, node: u32, interest: u32, rename: i16) {
    use Ed2Calls as _;
    let c = m.u32(cid::CAMERAS);
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    let cam = m.u32(node + 4);
    if cam != m.u32(m.u32(SI_PROC_DATA) + 8) {
        return;
    }
    if rename != 0 {
        let old = m.u32(cam);
        if old != 0 {
            m.free(old);
            m.set_u32(cam, 0);
        }
        let d = cur_dict(m);
        let name = o.elemdict_get_new_name(m, d, c, 0x0868_0bb2); // 'Camera'
        let copy = m.strdup(name);
        m.set_u32(cam, copy);
    }
    let text = m.u32(cam);
    let rec = name_record(m, o, text);
    let data = m.u32(node + 4);
    let e1 = add_elem(m, o, c, rec, data, node);
    let rec = o.db_list_node_allocate(m);
    let d = cur_dict(m);
    let name = o.elemdict_get_new_name(m, d, c, 0x0868_0bb9); // 'cam_int'
    let copy = m.strdup(name);
    m.set_u32(rec + 0x14, copy);
    let full = o.db_format_full_name(m, rec);
    m.set_u32(rec + 0x24, full);
    let data2 = m.u32(interest + 4);
    let e2 = add_elem(m, o, c, rec, data2, interest);
    add_relation(m, o, e1, e2, 0x456);
    let d = cur_dict(m);
    o.elemdict_update_hier_info(m, d, c, 1, 0x456);
    let d = cur_dict(m);
    let c2 = m.u32(cid::CAMERAS);
    let el = o.elemdict_get_elem(m, d, c2, e2);
    m.set_u32(el + 0xc, 0);
    m.set_u32(el + 0x10, 0);
    let d = cur_dict(m);
    let el = o.elemdict_get_elem(m, d, c2, e1);
    if m.u32(SI_GLB_SCHEM) == 0 {
        m.set_u32(el + 0xc, 0);
        let v = 0xc000_0000; // -2.0 (the float at 0x08649a70)
        m.set_u32(el + 0x10, v);
    } else {
        let v = 0x4060_0000; // 3.5 (the float at 0x08649a74)
        m.set_u32(el + 0xc, v);
        m.set_u32(el + 0x10, 0);
    }
    let d = cur_dict(m);
    o.schem_compute_hierarchies_bbox(m, d, 0, 0);
}

/// [soft 0x08101d70 SI_edAddLight]: nothing while `SI_addInDictionary` is 0. The light (the +4
/// of the first node; its name is the text at its start) gets a new unused name copied back into
/// it, a name record and an element in the lights chapter, whose number goes to the light's +0x170.
/// With an interest node: a record named `<light name>_int` (made unique), its element, and the
/// relation 0x83e (side 1) from the light's element to it; the hierarchy info is updated.
pub fn ed_add_light(m: &mut Mem, o: &mut dyn Outside, node: u32, interest: u32, _rename: i16) {
    use Ed2Calls as _;
    let c = m.u32(cid::LIGHTS);
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    let light = m.u32(node + 4);
    let d = cur_dict(m);
    let name = o.elemdict_get_new_name(m, d, c, light);
    si_mem::clib::strcpy(m, light, name);
    let rec = name_record(m, o, light);
    let e = add_elem(m, o, c, rec, m.u32(node + 4), node);
    m.set_u32(light + 0x170, e);
    if interest != 0 {
        let rec = o.db_list_node_allocate(m);
        sprintf_work(m, &[Err(Arg::S(light)), Ok(b"_int")]);
        let d = cur_dict(m);
        let name = o.elemdict_get_new_name(m, d, c, WORK_STR);
        let copy = m.strdup(name);
        m.set_u32(rec + 0x14, copy);
        let full = o.db_format_full_name(m, rec);
        m.set_u32(rec + 0x24, full);
        let data = m.u32(interest + 4);
        let e2 = add_elem(m, o, c, rec, data, interest);
        add_relation(m, o, e, e2, 0x83e);
        let d = cur_dict(m);
        o.elemdict_update_hier_info(m, d, c, 1, 0x83e);
    }
}

/// [soft 0x08103510 SI_edAddCluster]: nothing while `SI_addInDictionary` is 0 or when the model
/// has no element. Else a name record whose name is `"%s%s"` of the model's name and the
/// cluster's name (`sprintf` into `workStr`, copied), the model's +0x10 text copied to the
/// record's +0x10 when set, the full name; an element in the clusters chapter with the cluster
/// as data; the relation 700 (side 1) from the model's element to it.
pub fn ed_add_cluster(m: &mut Mem, o: &mut dyn Outside, model: u32, cluster: u32) {
    use Ed2Calls as _;
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    let index = m.local(4);
    let e = o.ed_get_elem_from_model(m, model, index);
    if e == 0 {
        return;
    }
    let rec = o.db_list_node_allocate(m);
    let node = m.u32(e);
    let name = m.u32(node + 0x14);
    let cname = m.u32(cluster);
    sprintf_work(m, &[Err(Arg::S(name)), Err(Arg::S(cname))]);
    let copy = m.strdup(WORK_STR);
    m.set_u32(rec + 0x14, copy);
    let node = m.u32(e);
    if m.u32(node + 0x10) != 0 {
        let copy = m.strdup(m.u32(node + 0x10));
        m.set_u32(rec + 0x10, copy);
    }
    let full = o.db_format_full_name(m, rec);
    m.set_u32(rec + 0x24, full);
    let c = m.u32(cid::CLUSTERS);
    let n = add_elem(m, o, c, rec, cluster, 0);
    let from = m.u32(index);
    add_relation(m, o, from, n, 700);
}

/// [soft 0x081036a8 SI_edAddShape]: as the cluster adder, with the name `"%sShp"` of the model's
/// name, the model itself as the element's data, in the shapes chapter, related by 200.
pub fn ed_add_shape(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    use Ed2Calls as _;
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    let index = m.local(4);
    let e = o.ed_get_elem_from_model(m, model, index);
    if e == 0 {
        return;
    }
    let rec = o.db_list_node_allocate(m);
    let node = m.u32(e);
    let name = m.u32(node + 0x14);
    sprintf_work(m, &[Err(Arg::S(name)), Ok(b"Shp")]);
    let copy = m.strdup(WORK_STR);
    m.set_u32(rec + 0x14, copy);
    let node = m.u32(e);
    if m.u32(node + 0x10) != 0 {
        let copy = m.strdup(m.u32(node + 0x10));
        m.set_u32(rec + 0x10, copy);
    }
    let full = o.db_format_full_name(m, rec);
    m.set_u32(rec + 0x24, full);
    let c = m.u32(cid::SHAPES);
    let n = add_elem(m, o, c, rec, model, 0);
    let from = m.u32(index);
    add_relation(m, o, from, n, 200);
}

/// [soft 0x0810383c SI_edAddLattice]: as above with the name `"%s%cLat"` (`N` for the model's
/// first lattice set, `B` when `which` is not 0), the lattice record's first word (the pointer at
/// +0x50, or at +0x58 for `B`, read once) as data, in the lattices chapter, related by 0xd2 (0xd7).
pub fn ed_add_lattice(m: &mut Mem, o: &mut dyn Outside, model: u32, which: i16) {
    use Ed2Calls as _;
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return;
    }
    let index = m.local(4);
    let e = o.ed_get_elem_from_model(m, model, index);
    if e == 0 {
        return;
    }
    let rec = o.db_list_node_allocate(m);
    let ch = if which != 0 { 0x42 } else { 0x4e };
    let node = m.u32(e);
    let name = m.u32(node + 0x14);
    sprintf_work(m, &[Err(Arg::S(name)), Err(Arg::C(ch)), Ok(b"Lat")]);
    let copy = m.strdup(WORK_STR);
    m.set_u32(rec + 0x14, copy);
    let node = m.u32(e);
    if m.u32(node + 0x10) != 0 {
        let copy = m.strdup(m.u32(node + 0x10));
        m.set_u32(rec + 0x10, copy);
    }
    let full = o.db_format_full_name(m, rec);
    m.set_u32(rec + 0x24, full);
    let slot = if which == 0 { m.u32(model + 0x50) } else { m.u32(model + 0x58) };
    let data = m.u32(slot);
    let c = m.u32(cid::LATTICES);
    let n = add_elem(m, o, c, rec, data, 0);
    let id = if which != 0 { 0xd7 } else { 0xd2 };
    let from = m.u32(index);
    add_relation(m, o, from, n, id);
}

/// [soft 0x08103d44 SI_edReplaceUDF]: nothing unless both shaders are given and the old one has
/// an element in the chapter. For the shader chapters (material, photon, texture 2D and 3D, volume,
/// camera, output, shadow, environment, light, light photon) the element's data becomes the new
/// shader, its record's name and full name are freed and made from the new shader's name (first
/// word), the new shader's flags (16 bits at +0xe) get the old one's ORed in, the element is marked
/// dirty with them (sign-extended), the new shader's array elements are attached and the shader curves
/// are updated from the old to the new.
pub fn ed_replace_udf(m: &mut Mem, o: &mut dyn Outside, old: u32, new: u32, chapter: i32) {
    use Ed2Calls as _;
    use Ed3Calls as _;
    if old == 0 || new == 0 {
        return;
    }
    let d = cur_dict(m);
    let n = o.elemdict_find_elem_by_data(m, d, chapter as u32, old);
    if (n as i32) <= -1 {
        return;
    }
    let d = cur_dict(m);
    let elem = o.elemdict_get_elem(m, d, chapter as u32, n);
    let shader_chapters = [
        cid::MATERIAL_SHADERS,
        cid::PHOTON_SHADERS,
        cid::TEXTURE2D_SHADERS,
        cid::TEXTURE3D_SHADERS,
        cid::VOLUME_SHADERS,
        cid::CAMERA_SHADERS,
        cid::OUTPUT_SHADERS,
        cid::SHADOW_SHADERS,
        cid::ENVIRON_SHADERS,
        cid::LIGHT_SHADERS,
        cid::LIGHT_PHOTON_SHADERS,
    ];
    if shader_chapters.iter().any(|&a| chapter as u32 == m.u32(a)) {
        let node = m.u32(elem);
        m.set_u32(elem + 4, new);
        let p = m.u32(node + 0x14);
        if p != 0 {
            m.free(p);
            m.set_u32(node + 0x14, 0);
        }
        let p = m.u32(node + 0x24);
        if p != 0 {
            m.free(p);
            m.set_u32(node + 0x24, 0);
        }
        let text = m.u32(new);
        let copy = m.strdup(text);
        m.set_u32(node + 0x14, copy);
        let full = o.db_format_full_name(m, node);
        m.set_u32(node + 0x24, full);
        let flags = m.u16(new + 0xe) | m.u16(old + 0xe);
        m.set_u16(new + 0xe, flags);
        o.elemdict_set_dirty(m, elem, flags as i16 as i32 as u32);
        o.udf_attach_array_elements(m, new);
        o.shader_fcurve_update_from_dialog(m, old, new);
    }
}

/// [soft 0x08103eb4 SI_getRelationList]: the text listing the elements related to an element by
/// the serial relation 0x227. Null (0) unless `rec` is given with its number (+8) not negative,
/// `index` in 0..`*dict` (the dictionary's count), the record's element exists in the current
/// dictionary and the relation's attributes are found. Then for each related element of
/// `SI_elemdictFindRelatedElemSerial` / `…Next…` (index `index`): found in `dict`, else the loop
/// ends; the text gets a space before every name but the first, the element's +0x10 text and "-"
/// when set, then its name (+0x14 of its record), all through `SI_strAppend`. Answers the text.
pub fn get_relation_list(m: &mut Mem, o: &mut dyn Outside, dict: u32, rec: u32, index: i32, pass: u32) -> u32 {
    use Ed2Calls as _;
    use Ed3Calls as _;
    let text = m.local(4); // local_14
    let cid_out = m.local(4); // [ebp-0x10]
    let num_out = m.local(4); // [ebp-0xc]
    let attr = m.local(4); // [ebp-8]
    let scratch = m.local(4); // [ebp-4]
    m.set_u32(text, 0);
    if rec == 0 || m.i32(rec + 8) < 0 || index < 0 || index >= m.i32(dict) {
        return 0;
    }
    let d = cur_dict(m);
    let elem = o.elemdict_get_elem(m, d, m.u32(rec + 4), m.u32(rec + 8));
    if elem == 0 {
        return 0;
    }
    let found = o.reldict_get_rel_attributes(m, SI_RD, 0x227, 1, 0, scratch, scratch, attr, scratch, scratch, scratch);
    if found == 0 {
        return 0;
    }
    let d = cur_dict(m);
    let a = m.u32(attr);
    let mut r = o.elemdict_find_related_elem_serial(m, d, elem, 1, a, 0x227, index as u32, cid_out, num_out, pass);
    // (the argument order above puts the two outputs after the index; see the call's definition)
    loop {
        if r == 0 {
            return m.u32(text);
        }
        let (c, n) = (m.u32(cid_out), m.u32(num_out));
        let e = o.elemdict_get_elem(m, dict, c, n);
        if e == 0 {
            return m.u32(text);
        }
        if m.u32(text) != 0 {
            o.str_append(m, text, 0x0868_0be7); // " "
        }
        let node = m.u32(e);
        if m.u32(node + 0x10) != 0 {
            o.str_append(m, text, m.u32(node + 0x10));
            o.str_append(m, text, 0x0868_0be9); // "-"
        }
        let node = m.u32(e);
        o.str_append(m, text, m.u32(node + 0x14));
        let d = cur_dict(m);
        let a = m.u32(attr);
        r = o.elemdict_find_next_related_elem_serial(m, d, elem, 1, a, 0x227, index as u32, cid_out, num_out, pass);
    }
}

/// [soft 0x08104df8 edCnsSetJntMdlExtraRel]: for each item `edGetConstrainingItem(dict, model,
/// models chapter, list, relation, ...)` finds (first, then next) whose kind is 2 or 3 and whose
/// node kind (+0x30) is 5 (a joint): walks down the joint's +0x2c4 chain to the first node with kind
/// 0 and a second kind (+0x2ec) of 3 or 4, takes its element, and relates it to `from` by 0x79
/// (side 1) when `on` is not 0, else removes that relation pair.
pub fn ed_cns_set_jnt_mdl_extra_rel(m: &mut Mem, o: &mut dyn Outside, dict: u32, model: u32, from: u32, list: u32, rel: u32, on: i16) {
    use Ed2Calls as _;
    use Ed3Calls as _;
    let kind = m.local(4);
    let idx = m.local(4);
    let mut first = 1;
    loop {
        let c = m.u32(cid::MODELS);
        let item = o.ed_get_constraining_item(m, dict, model, c, list, rel, first, kind);
        first = 0;
        if item == 0 {
            break;
        }
        if (m.u32(kind).wrapping_sub(2)) <= 1 && m.i32(item + 0x30) == 5 {
            let mut q = m.u32(item + 0x2c4);
            while m.i32(q + 0x30) != 0 || m.u32(q + 0x2ec).wrapping_sub(3) > 1 {
                q = m.u32(q + 0x2c4);
            }
            o.ed_get_elem_from_model(m, q, idx);
            let to = m.u32(idx);
            let d = cur_dict(m);
            if on != 0 {
                o.elemdict_add_relation(m, d, SI_RD, from, to, 0x79, 1, 0);
            } else {
                o.elemdict_remove_relation_pair(m, d, SI_RD, from, to, 0x79, 1, 0);
            }
        }
    }
}

/// A float copied through the x87 stack (`fld` then `fstp` of 32 bits): a signalling NaN comes
/// out quiet.
fn x87_copy(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// [soft 0x08106588 si_elemdictRemoveExtraCam]: with more than two cameras in use in the cameras
/// chapter of `dict` (elements of 0x84 bytes, state at +0x48 = 0), the extra ones are removed. First
/// each camera in use gets a flag (has an animation relation to the animation chapter, or at least
/// one serial relation 0x4e2a), and two summary flags are formed from the type (+0x46): `a` for a
/// type 1 element, `b` for any other, with a flag set. Then each camera in use is deleted when
/// `a` and its type is not 1, or (not `a`) `b` and its type is 1, or (neither) its type is not 1:
/// (if not the standalone renderer, not an all-zero scene and not a type 1) the node
/// positions (+0xc, +0x10) are remembered by node kind and `*seen_model` is set when any of its
/// relation groups (the tables at +0x64 and +0x6c, six groups) names the models chapter;
/// then its animation (when flagged) and expressions are removed with `SI_addInDictionary` forced
/// to 1 and restored, and the element is marked deleted. At the end the remembered positions are
/// written to the remaining type 1 elements with a node. The remembered floats which were not
/// set stay as the stack held them (the original reads them uninitialised; so does this).
pub fn elemdict_remove_extra_cam(m: &mut Mem, o: &mut dyn Outside, dict: u32, seen_model: u32) {
    use Ed2Calls as _;
    use Ed3Calls as _;
    let mut a = false;
    let mut b = false;
    let mut moved = false;
    let scene_index = m.local(4);
    let out_b = m.local(4);
    let saved = m.local(16); // local_38, local_34 (node kind 4), local_40, local_3c (others)
    let scene = o.ed_get_elem_from_scene(m, scene_index);
    let mut zero_scene = false;
    if scene != 0 {
        zero_scene = (0x2c..=0x38).step_by(4).all(|off| m.f32(scene + off) == 0.0);
    }
    let cams = m.u32(cid::CAMERAS).wrapping_mul(0x40).wrapping_add(m.u32(dict + 4));
    let mut in_use = 0i32;
    {
        let mut n = m.i32(cams + 4);
        if n > 0 {
            let mut p = m.u32(cams + 8).wrapping_add(0x48);
            while n != 0 {
                if m.i16(p) == 0 {
                    in_use += 1;
                }
                p = p.wrapping_add(0x84);
                n -= 1;
            }
        }
    }
    if in_use <= 2 {
        return;
    }
    let rel = o.rel_get_anim_f_rel_from_chapter(m, m.u32(cid::CAMERAS));
    // SI_secureLicAlloc (0x08679660) is licence bookkeeping: the original increments it here and
    // SI_calloc clears it; the licence global is deliberately not written (the check allows that write).
    let count0 = m.u32(cams + 4);
    let flags = m.si_calloc(count0, 2);
    let mut i = 0i32;
    while i < m.i32(cams + 4) {
        let e = m.u32(cams + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        if m.i16(e + 0x48) == 0 {
            let r = o.elemdict_find_related_elem(m, dict, e, 1, 0, rel, m.u32(cid::ANIMATION), scene_index, out_b);
            let has = if r != 0 { true } else { o.elemdict_serial_rel_count(m, dict, e, 0x4e2a, 0, 0) >= 1 };
            m.set_u16(flags.wrapping_add((i as u32).wrapping_mul(2)), has as u16);
            let f = has;
            if m.i16(e + 0x46) == 1 {
                a = a || f;
            } else {
                b = b || f;
            }
        }
        i += 1;
    }
    o.schematic_no_refresh(m, 1);
    let mut i = 0i32;
    while i < m.i32(cams + 4) {
        let e = m.u32(cams + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        i += 1;
        if m.i16(e + 0x48) != 0 {
            continue;
        }
        let ty = m.i16(e + 0x46);
        let delete = if a { ty != 1 } else if b { ty == 1 } else { ty != 1 };
        if !delete {
            continue;
        }
        if m.i16(SI_STANDALONE_RENDERER) == 0 && !zero_scene && ty != 1 {
            moved = true;
            let g = m.u32(e + 8);
            if g != 0 {
                let (x, y) = (m.u32(e + 0xc), m.u32(e + 0x10));
                if m.i32(g) == 4 {
                    m.set_u32(saved, x_copy(x));
                    m.set_u32(saved + 4, x_copy(y));
                } else {
                    m.set_u32(saved + 8, x_copy(x));
                    m.set_u32(saved + 12, x_copy(y));
                }
            }
        }
        if m.i16(seen_model) == 0 && ty != 1 {
            let models = m.u32(cid::MODELS);
            let mut k = 0u32;
            loop {
                let g = m.u32(e + 0x64).wrapping_add(k.wrapping_mul(0x18));
                let mut j = 0i32;
                while j < m.i32(g) {
                    let rec = m.u32(g + 0xc).wrapping_add((j as u32).wrapping_mul(0x20));
                    if m.i16(rec + 0x10) as i32 as u32 == models {
                        m.set_u16(seen_model, 1);
                        break;
                    }
                    j += 1;
                }
                let before = m.i16(seen_model);
                let mut j = 0i32;
                loop {
                    let g2 = m.u32(e + 0x6c).wrapping_add(k.wrapping_mul(0x18));
                    if before != 0 || j >= m.i32(g2) {
                        break;
                    }
                    let rec = m.u32(g2 + 0xc).wrapping_add((j as u32).wrapping_mul(0x20));
                    if m.i16(rec + 0x10) as i32 as u32 == models {
                        m.set_u16(seen_model, 1);
                        break;
                    }
                    j += 1;
                }
                k += 1;
                if m.i16(seen_model) != 0 || k > 5 {
                    break;
                }
            }
        }
        let keep = m.i16(SI_ADD_IN_DICTIONARY);
        let c = m.u32(cid::CAMERAS);
        let idx = (i - 1) as u32;
        if m.i16(flags.wrapping_add(idx.wrapping_mul(2))) != 0 {
            o.ed_remove_elem_anim(m, dict, c, e, idx, 1);
            m.set_i16(SI_ADD_IN_DICTIONARY, 1);
            o.ed_remove_expressions_from_arg(m, dict, e);
            let c = m.u32(cid::CAMERAS);
            o.ed_remove_elem_expressions(m, dict, e, c, idx);
        } else {
            m.set_i16(SI_ADD_IN_DICTIONARY, 1);
            o.ed_remove_expressions_from_arg(m, dict, e);
        }
        m.set_i16(SI_ADD_IN_DICTIONARY, keep);
        let c = m.u32(cid::CAMERAS);
        o.elemdict_set_delete_elem(m, dict, c, e, idx, u32::MAX, 0, 1);
    }
    if moved {
        let mut i = 0i32;
        while i < m.i32(cams + 4) {
            let e = m.u32(cams + 8).wrapping_add((i as u32).wrapping_mul(0x84));
            if m.i16(e + 0x48) == 0 && m.i16(e + 0x46) == 1 {
                let g = m.u32(e + 8);
                if g != 0 {
                    let (x, y) = if m.i32(g) == 4 { (m.u32(saved), m.u32(saved + 4)) } else { (m.u32(saved + 8), m.u32(saved + 12)) };
                    m.set_u32(e + 0xc, x_copy(x));
                    m.set_u32(e + 0x10, x_copy(y));
                }
            }
            i += 1;
        }
    }
    if flags != 0 {
        m.free(flags);
    }
    o.schematic_no_refresh(m, 0);
}

fn x_copy(bits: u32) -> u32 {
    x87_copy(bits)
}

/// The chapters whose related elements the editable marks follow (the eight of
/// `SI_edIsNseByDependence`, without the dependence-only filters).
fn follows_chapter(m: &Mem, chapter: u32) -> bool {
    [cid::ANIMATION, cid::CID_1684, cid::MATERIALS, cid::TEXTURES2D, cid::TEXTURES3D, cid::LIGHTS, cid::WAVES, cid::MODELS]
        .iter()
        .any(|&a| chapter == m.u32(a))
}

/// [soft 0x081074c0 SI_edSetUneditable]: nothing for a null element. With a graph node (+8): none
/// if bit 0x1000 of its 16-bit word at +0x16 is set already; else the word becomes
/// `(word & 0x1fff) | 0x1000` with 0x2000, 0x4000 or 0x8000 added for the node's mode (+8) 1, 3 or
/// 2 (that mode is cleared), the node is moved from the current graph list's slot +8 to +0x20
/// when it is there, and a node of kind 0xc ends it. Then every relation of the element's group table
/// (+0x64, +0x60 groups) other than 0xee, 800 and 0x321 to 0x327, in one of eight chapters, whose
/// element is not in state 2, is made uneditable too, and so are the elements related by 0x4e22 and
/// by 20000 (side 4) in the models chapter.
pub fn ed_set_uneditable(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) {
    use Ed2Calls as _;
    if elem == 0 {
        return;
    }
    let node = m.u32(elem + 8);
    if node != 0 {
        let w = m.u16(node + 0x16);
        if w & 0x1000 != 0 {
            return;
        }
        let mode = m.i32(node + 8);
        m.set_u16(node + 0x16, w | 0x1000);
        let lists = m.u32(SI_GRAF_LISTS);
        let which = m.u32(m.u32(SI_PROC_DATA) + 4);
        let base = lists.wrapping_add(which.wrapping_mul(0x24));
        let w = w & 0x1fff;
        let mut nw = w | 0x1000;
        if mode == 1 {
            nw = w | 0x3000;
        } else if mode == 3 {
            nw = w | 0x5000;
        } else if mode == 2 {
            nw = w | 0x9000;
        }
        m.set_u16(node + 0x16, nw);
        m.set_i32(node + 8, 0);
        if m.u32(base + 8) == node {
            m.set_u32(base + 0x20, node);
            m.set_u32(base + 8, 0);
        }
        if m.i32(node) == 0xc {
            return;
        }
    }
    if m.u32(elem + 100) != 0 {
        let mut g = 0i32;
        if 0 < m.i32(elem + 0x60) {
            loop {
                let next = g + 1;
                let group = m.u32(elem + 100).wrapping_add((g as u32).wrapping_mul(0x18));
                let mut k = 0i32;
                if 0 < m.i32(group) {
                    loop {
                        let rel = m.u32(group + 12).wrapping_add((k as u32).wrapping_mul(0x20));
                        let id = m.i32(rel);
                        if id != 0xee && id != 800 && !(0x321..=0x327).contains(&id) {
                            let chapter = m.i16(rel + 0x10) as i32 as u32;
                            if follows_chapter(m, chapter) {
                                let n = m.u32(rel + 4);
                                let other = o.elemdict_get_elem(m, dict, chapter, n);
                                if other != 0 && m.i16(other + 0x48) != 2 {
                                    ed_set_uneditable(m, o, dict, other);
                                }
                            }
                        }
                        k += 1;
                        if k >= m.i32(group) {
                            break;
                        }
                    }
                }
                g = next;
                if g >= m.i32(elem + 0x60) {
                    break;
                }
            }
        }
        let l = m.local(4);
        for (id, use_cur) in [(0x4e22u32, false), (20000, true)] {
            let d = if use_cur { cur_dict(m) } else { dict };
            let mc = m.u32(cid::MODELS);
            let mut p = o.elemdict_find_related_elem(m, d, elem, 0, 4, id, mc, l, l);
            while p != 0 {
                ed_set_uneditable(m, o, dict, p);
                let mc = m.u32(cid::MODELS);
                p = o.elemdict_find_next_related_elem(m, dict, elem, 0, 4, id, mc, l, l);
            }
        }
    }
}

/// [soft 0x08107a44 SI_edCheckForUneditable]: 0 unless the chapter is the models chapter and the
/// element (not in state 2) has a graph node and a model (+4). By the model's kind (+0x2ec): 0xd
/// is uneditable when the element related by 20000 (side 4) has a graph node with bit 0x10 of
/// its byte at +0x17 set, and otherwise as the default; 0xf is uneditable when the element related
/// by 0x4e22 has such a node (otherwise as the default); kinds 1, 3 to 0xb, 0xe and 0x10 to 0x1d are
/// uneditable; the default is the model's +0x31c being 4 or 9 to 11, or
/// `SI_wrathUnsupportedMdlType`.
pub fn ed_check_for_uneditable(m: &mut Mem, o: &mut dyn Outside, elem: u32, chapter: i32) -> i16 {
    use Ed2Calls as _;
    let mut r: i16 = 0;
    if chapter as u32 != m.u32(cid::MODELS) || elem == 0 || m.i16(elem + 0x48) == 2 || m.u32(elem + 8) == 0 {
        return 0;
    }
    let model = m.u32(elem + 4);
    if model == 0 {
        return 0;
    }
    let l = m.local(4);
    let default = |m: &mut Mem| -> i16 {
        let k = m.i32(model + 0x31c);
        let mut r = 0i16;
        if k == 4 || (3 < k && k < 0xc && 8 < k) {
            r = 1;
        }
        if r == 0 {
            r = wrath_unsupported_mdl_type(m, model) as i16;
        }
        r
    };
    let kind = m.u32(model + 0x2ec);
    match kind {
        0xd => {
            let d = cur_dict(m);
            let mc = m.u32(cid::MODELS);
            let f = o.elemdict_find_related_elem(m, d, elem, 1, 4, 20000, mc, l, l);
            let n = if f != 0 { m.u32(f + 8) } else { 0 };
            if n != 0 && (m.u8(n + 0x17) & 0x10) != 0 {
                r = 1;
            }
            if r == 0 {
                r = default(m);
            }
        }
        0xf => {
            let d = cur_dict(m);
            let mc = m.u32(cid::MODELS);
            let f = o.elemdict_find_related_elem(m, d, elem, 1, 4, 0x4e22, mc, l, l);
            let n = if f != 0 { m.u32(f + 8) } else { 0 };
            if n == 0 || (m.u8(n + 0x17) & 0x10) == 0 {
                r = default(m);
            } else {
                r = 1;
            }
        }
        1 | 3..=0xb | 0xe | 0x10..=0x1d => r = 1,
        _ => r = default(m),
    }
    r
}

/// [soft 0x08107764 SI_edSetEditable]: 0 for a null element or one without a graph node; 1 when the
/// node's byte at +0x17 has not bit 0x10 (nothing to do). Else, unless `SI_edIsNseByDependence`
/// says the element depends on an uneditable one or `SI_edCheckForUneditable(elem, chapter)`
/// says it must stay so (answer 0): the node is moved back from the current graph list's slot
/// +0x20 to +8 when it is there, its mode (+8) is restored from the bits 0x2000, 0x4000, 0x8000 of
/// the word at +0x16 (1, 3, 2, else 0) and that word is masked with 0xfff; then the same is done for
/// the related elements of the group table (eight chapters; element in state other than 2 with a
/// node; the answer is that of the last one done) and for those related by 0x4e22 and 20000 (side
/// 4) in the models chapter. Answers 1 (or the last related element's answer).
pub fn ed_set_editable(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, chapter: i32) -> i32 {
    use Ed2Calls as _;
    if elem == 0 {
        return 0;
    }
    let node = m.u32(elem + 8);
    if node == 0 {
        return 0;
    }
    if m.u8(node + 0x17) & 0x10 == 0 {
        return 1;
    }
    let dep = crate::edtools2::ed_is_nse_by_dependence(m, o, dict, elem);
    let mut ok: u16 = (dep == 0) as u16;
    let mut stay = true;
    if ok != 0 {
        let u = ed_check_for_uneditable(m, o, elem, chapter);
        stay = u != 0;
    }
    if !stay {
        let lists = m.u32(SI_GRAF_LISTS);
        let which = m.u32(m.u32(SI_PROC_DATA) + 4);
        let base = lists.wrapping_add(which.wrapping_mul(0x24));
        if m.u32(base + 0x20) == node {
            m.set_u32(base + 8, node);
            m.set_u32(base + 0x20, 0);
        }
        let w = m.u16(node + 0x16);
        let mode = if w & 0x2000 != 0 {
            1
        } else if w & 0x4000 != 0 {
            3
        } else {
            ((w as i16) < 0) as i32 * 2
        };
        m.set_i32(node + 8, mode);
        m.set_u16(node + 0x16, m.u16(node + 0x16) & 0xfff);
        if m.u32(elem + 100) != 0 && 0 < m.i32(elem + 0x60) {
            let mut g = 0i32;
            loop {
                let next = g + 1;
                let group = m.u32(elem + 100).wrapping_add((g as u32).wrapping_mul(0x18));
                if 0 < m.i32(group) {
                    let mut k = 0i32;
                    loop {
                        let rel = (k as u32).wrapping_mul(0x20).wrapping_add(m.u32(group + 12));
                        let ch = m.i16(rel + 0x10) as i32;
                        if [cid::ANIMATION, cid::CID_1684, cid::MATERIALS, cid::TEXTURES2D, cid::TEXTURES3D, cid::LIGHTS, cid::WAVES, cid::MODELS]
                            .iter()
                            .any(|&a| ch as u32 == m.u32(a))
                        {
                            let n = m.u32(rel + 4);
                            let other = o.elemdict_get_elem(m, dict, ch as u32, n);
                            if other != 0 && m.i16(other + 0x48) != 2 && m.u32(other + 8) != 0 {
                                ok = ed_set_editable(m, o, dict, other, ch) as u16;
                            }
                        }
                        k += 1;
                        if k >= m.i32(group) {
                            break;
                        }
                    }
                }
                g = next;
                if g >= m.i32(elem + 0x60) {
                    break;
                }
            }
        }
        let lc = m.local(4);
        let ld = m.local(4);
        for (id, use_cur) in [(0x4e22u32, false), (20000, true)] {
            let d = if use_cur { cur_dict(m) } else { dict };
            let mc = m.u32(cid::MODELS);
            let mut p = o.elemdict_find_related_elem(m, d, elem, 0, 4, id, mc, lc, ld);
            while p != 0 {
                let c = m.i32(lc);
                ed_set_editable(m, o, dict, p, c);
                let mc = m.u32(cid::MODELS);
                p = o.elemdict_find_next_related_elem(m, dict, elem, 0, 4, id, mc, lc, ld);
            }
        }
    } else {
        ok = 0;
    }
    ok as i16 as i32
}
