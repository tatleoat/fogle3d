// SPDX-License-Identifier: MIT
//! The reading routines of the attribute groups an expression or an animation of a loaded scene can reach beyond `MODELS` and
//! `General_Element`: `Materials`, `Texture2D`, `Texture3D`, `Cameras`, `Lights`, `Spot`, `Sun`, `Scenes`, `Waves` and the
//! dummy (`attrib.c`, finding 0013, section 21).
//!
//! All have one shape: the element is looked up in the current element dictionary (it must have status 0 at +0x48), the
//! attribute and its component number are checked against the attribute dictionary, and the chapter's parameter reader answers
//! with the fcurve type of the component. Four of them go through the generic static read (`SI_edElementStaticValueGet`), the
//! others call the chapter's reader directly with no curve. When the attribute is of kind 3 (an enumeration) the answer's kind
//! and value are overwritten with 3 and the attribute's enumeration number. The group's table (`si-dict` rodata) names the
//! routines by address; the host that routes indirect calls answers the addresses listed in [`ROUTINES`] with these.
use super::elem::ed_element_static_value_get;
use super::params;
use super::rec;
use si_dict::globals::SI_ATD;
use si_mem::{Mem, Outside, Ptr32};
use si_types::dict::{AttGrpIndex, AttributeValue, Elem, ElemIdentity};

/// `SI_defaultControl` (0x08781800), the render control `SI_scenesAttrGetValue` reads.
const SI_DEFAULT_CONTROL: u32 = 0x0878_1800;

/// The routines of this file, by address, for a host that answers indirect calls.
pub const ROUTINES: [(u32, &str); 10] = [
    (0x080f_9674, "SI_materialsAttrGetValue"),
    (0x080f_97e0, "SI_texture2DAttrGetValue"),
    (0x080f_994c, "SI_texture3DAttrGetValue"),
    (0x080f_9abc, "SI_camerasAttrGetValue"),
    (0x080f_9cb8, "SI_lightsAttrGetValue"),
    (0x080f_9f4c, "SI_spotAttrGetValue"),
    (0x080f_a0c4, "SI_sunAttrGetValue"),
    (0x080f_a23c, "SI_scenesAttrGetValue"),
    (0x080f_a3a0, "SI_wavesAttrGetValue"),
    (0x080f_abf0, "SI_dummyAttrGetValue"),
];

/// What every routine looks up first: the element (status 0), the attribute, and the value record present. `Err(())` answers 0.
struct Found {
    elem: u32,
    attr: u32,
}

fn lookup(m: &mut Mem, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> Option<Found> {
    let ed = rec::cur_dict(m);
    let (chap, id) = (m.i32(ident.addr()), m.i32(ident.addr().wrapping_add(4)));
    let elem = si_dict::elemdict::get_elem(m, ed, chap, id).addr();
    if elem == 0 || m.i16(elem + 0x48) != 0 {
        return None;
    }
    let attr = si_dict::attdict::attribute_get_by_index(m, SI_ATD, index).addr();
    if attr == 0 || value.addr() == 0 {
        return None;
    }
    Some(Found { elem, attr })
}

/// The component number is inside the attribute's list of fcurve types (+0x56 count, +0x58 the 16-bit types): its type.
fn component(m: &Mem, f: &Found, index: Ptr32<AttGrpIndex>) -> Option<i32> {
    let k = m.i32(index.addr().wrapping_add(0x10));
    if k < 0 || k >= m.i16(f.attr + 0x56) as i32 {
        return None;
    }
    Some(m.i16(m.u32(f.attr + 0x58).wrapping_add(2 * k as u32)) as i32)
}

/// An enumeration attribute (kind +0x2c is 3) answers as kind 3 with the attribute's own enumeration number (+0x30).
fn enum_fixup(m: &mut Mem, f: &Found, v: u32) {
    if m.i32(f.attr + 0x2c) == 3 {
        m.set_u32(v, 3);
        let n = m.i16(f.attr + 0x30) as i32;
        m.set_i32(v + 4, n);
    }
}

/// The routines whose object is the element's (+4) and whose reader takes `(type, object, no curve, value)`: the shape of
/// `Materials`, `Texture2D`, `Texture3D`. The object must exist; the component is checked after it.
fn direct(
    m: &mut Mem,
    ident: Ptr32<ElemIdentity>,
    index: Ptr32<AttGrpIndex>,
    value: Ptr32<AttributeValue>,
    read: &mut dyn FnMut(&mut Mem, i32, u32, u32) -> u32,
) -> i32 {
    let Some(f) = lookup(m, ident, index, value) else { return 0 };
    let obj = m.u32(f.elem + 4);
    if obj == 0 {
        return 0;
    }
    let Some(t) = component(m, &f, index) else { return 0 };
    let r = read(m, t, obj, value.addr()) as i16;
    enum_fixup(m, &f, value.addr());
    r as i32
}

/// [soft 0x080f9674 SI_materialsAttrGetValue]: the material's parameter of the component's type (`SI_materialGetParam`).
pub fn materials_attr_get_value(m: &mut Mem, _o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    direct(m, ident, index, value, &mut |m, t, obj, out| params::material_get_param(m, t, obj, out))
}

/// [soft 0x080f97e0 SI_texture2DAttrGetValue]: the 2D texture's parameter (`SI_text2dGetParam`).
pub fn texture2d_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    direct(m, ident, index, value, &mut |m, t, obj, out| params::text2d_get_param(m, o, t, obj, out))
}

/// [soft 0x080f994c SI_texture3DAttrGetValue]: the 3D texture's parameter (`SI_text3dGetParam`).
pub fn texture3d_attr_get_value(m: &mut Mem, _o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    direct(m, ident, index, value, &mut |m, t, obj, out| params::text3d_get_param(m, t, obj, out))
}

/// Type 0xb1 of a camera, 0xb0 of a light: the second pointer of the element (+8) when set gives a kind-2 answer, the boolean
/// "its word at +0xc is 2" in the 16-bit word after the kind.
fn interest_flag(m: &mut Mem, elem: u32, v: u32) -> bool {
    let p = m.u32(elem + 8);
    if p == 0 {
        return false;
    }
    m.set_u32(v, 2);
    let b = m.i32(p + 0xc) == 2;
    m.set_u16(v + 4, b as u16);
    true
}

/// [soft 0x080f9abc SI_camerasAttrGetValue]: type 0xb1 reads the element's second pointer (+8; boolean "word +0xc is 2", answer 1,
/// no enumeration fix-up); any other type is `SI_camGetParam` without a curve.
pub fn cameras_attr_get_value(m: &mut Mem, _o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    let Some(f) = lookup(m, ident, index, value) else { return 0 };
    let cam = m.u32(f.elem + 4);
    if cam == 0 {
        return 0;
    }
    let Some(t) = component(m, &f, index) else { return 0 };
    if t == 0xb1 && interest_flag(m, f.elem, value.addr()) {
        return 1;
    }
    let r = params::cam_get_param(m, t, cam, 0, value.addr()) as i16;
    enum_fixup(m, &f, value.addr());
    r as i32
}

/// The access record the generic read is given: its first word is 1 (the other two stay as the stack had them).
fn access_record(m: &mut Mem) -> u32 {
    let a = m.local(12);
    m.set_u32(a, 1);
    a
}

/// Shared tail of the groups that read through `SI_edElementStaticValueGet` (the element, its chapter, the type, the access
/// record, the value, no custom name) and fix an enumeration up when `fix` is set.
fn via_static(m: &mut Mem, o: &mut dyn Outside, f: &Found, ident: Ptr32<ElemIdentity>, t: i32, value: Ptr32<AttributeValue>, fix: bool) -> i32 {
    let access = access_record(m);
    let chap = m.i32(ident.addr());
    let r = ed_element_static_value_get(m, o, Ptr32::<Elem>::new(f.elem), chap, t, Ptr32::new(access), value, 0) as i16;
    m.release(12);
    if fix {
        enum_fixup(m, f, value.addr());
    }
    r as i32
}

/// [soft 0x080f9cb8 SI_lightsAttrGetValue]: the answer starts as the attribute's kind (+0x2c). Type 0xb0 is the interest flag as
/// the camera's 0xb1 (answer 1 only when the second pointer is set); 0x171 answers the attribute's kind and enumeration number
/// and the light's 16-bit word at +0x1e; any other type is the generic static read, then the enumeration fix-up.
pub fn lights_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    let Some(f) = lookup(m, ident, index, value) else { return 0 };
    let light = m.u32(f.elem + 4);
    if light == 0 {
        return 0;
    }
    // the bounds test of this routine reads the index twice and compares the full word, like the others
    let Some(t) = component(m, &f, index) else { return 0 };
    let v = value.addr();
    let kind = m.i32(f.attr + 0x2c);
    m.set_i32(v, kind);
    match t {
        0xb0 => interest_flag(m, f.elem, v) as i32,
        0x171 => {
            m.set_i32(v, kind);
            let n = m.i16(f.attr + 0x30) as i32;
            m.set_i32(v + 4, n);
            let w = m.i16(light + 0x1e) as i32;
            m.set_i32(v + 8, w);
            1
        }
        _ => via_static(m, o, &f, ident, t, value, true),
    }
}

fn static_group(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>, fix: bool) -> i32 {
    let Some(f) = lookup(m, ident, index, value) else { return 0 };
    let Some(t) = component(m, &f, index) else { return 0 };
    via_static(m, o, &f, ident, t, value, fix)
}

/// [soft 0x080f9f4c SI_spotAttrGetValue]: the generic static read, then the enumeration fix-up.
pub fn spot_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    static_group(m, o, ident, index, value, true)
}

/// [soft 0x080fa0c4 SI_sunAttrGetValue]: as the spot's.
pub fn sun_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    static_group(m, o, ident, index, value, true)
}

/// [soft 0x080fa3a0 SI_wavesAttrGetValue]: the generic static read, with no enumeration fix-up.
pub fn waves_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    static_group(m, o, ident, index, value, false)
}

/// [soft 0x080fa23c SI_scenesAttrGetValue]: the scene parameter of the component's type read from the default render control
/// (`SI_sceneGetParam`), then the enumeration fix-up. Needs no object of the element.
pub fn scenes_attr_get_value(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>, value: Ptr32<AttributeValue>) -> i32 {
    let Some(f) = lookup(m, ident, index, value) else { return 0 };
    let Some(t) = component(m, &f, index) else { return 0 };
    let r = params::scene_get_param(m, o, t, SI_DEFAULT_CONTROL, value.addr()) as i16;
    enum_fixup(m, &f, value.addr());
    r as i32
}

/// [soft 0x080fabf0 SI_dummyAttrGetValue]: the reader a new group has until it is given its own; answers 0.
pub fn dummy_attr_get_value(_m: &mut Mem, _o: &mut dyn Outside, _ident: Ptr32<ElemIdentity>, _index: Ptr32<AttGrpIndex>, _value: Ptr32<AttributeValue>) -> i32 {
    0
}

