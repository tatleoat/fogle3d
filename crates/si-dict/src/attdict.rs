// SPDX-License-Identifier: MIT
//! `attdict.c`: the dictionary of attributes (`SI_ATD`), on the image.
//!
//! The dictionary is a tree of groups (0x28 bytes each: name, condition, the indices of
//! its attributes, sub-groups, the two functions that read and write its attributes on an
//! element) and one flat array of attributes (100 bytes each). Every array grows by one
//! record per addition: `SI_malloc(size)` for the first, `SI_realloc(array, (n + 1) *
//! size)` after. An attribute is found by a path of group indices (`SI_AttGrpIndex`).
//!
//! An attribute owns one function-curve id per animated component. The index of all of
//! them (`SI_AttributeDict` +0x10: one record of 12 bytes per curve, sorted by id; the
//! count at +0x14) is what the scene loader asks for a curve's group, name and relation.
//!
//! The two functions of a group are code addresses of the binary ([`si_mem::FnPtr`]);
//! the one call through such a pointer (`SI_attdictEvalCond`) goes to
//! [`Outside::call_addr`].
//!
//! Field names are `si-types`': an attribute's `unk_00` is the kind of its owner (0 an
//! element, otherwise an operator: the dump prints `Elem` or `Oper`) and `unk_04` the
//! owner's id (a chapter, or an operator id); `range_low` and `range_high` are each a
//! float and a word that is 1 when the bound exists; the dictionary's `unk_14` is the
//! number of entries of the curve index.

use crate::elemdict;
use crate::globals::{DEFAULT_NAME_347, GLB_ATD, LAST_FCV_ID_328, LAST_SORT_IND_327, SI_ATD, SI_ELEM_DICTS, SI_PROC_DATA, SI_RD};
use crate::libc::{qsort, strchr, strcmp};
use crate::outside::Outside;
use crate::records::{AttribCond, FcvAtdIndex};
use crate::reldict;
use crate::rodata::{ATT_FCTS, S_CAMHID, S_HIDDEN, S_LIGHID, S_T2NORM};
use si_mem::clib::bsearch;
use si_mem::{field, CStr32, Mem, Ptr32};
use si_types::dict::{AttGrpIndex, AttribGroup, Attribute, AttributeDict, AttributeValue, ElemIdentity};
use si_x87::{Round, F80};

/// The parameter id of a custom effect's curve: it belongs to no attribute.
pub const FCV_CUSTOM: i32 = 0x236;

/// `SI_dummyAttrGetValue (0x080fabf0)` and `SI_dummyAttrSetValue (0x080fabfc)`, what a
/// new group reads and writes attributes with.
const SI_DUMMY_ATTR_GET_VALUE: u32 = 0x080f_abf0;
const SI_DUMMY_ATTR_SET_VALUE: u32 = 0x080f_abfc;

fn cond_of(m: &Mem, g: Ptr32<AttribGroup>) -> Ptr32<AttribCond> {
    m[g].cond.cast()
}

fn index_of(m: &Mem, atd: Ptr32<AttributeDict>) -> Ptr32<FcvAtdIndex> {
    m[atd].fcv_index.cast()
}

/// The 16-bit word at a byte offset of an index record, as the original's pointer
/// arithmetic reads it (beyond the record too).
fn short_at(m: &Mem, index: Ptr32<AttGrpIndex>, offset: i32) -> i16 {
    m.i16(index.addr().wrapping_add(offset as u32))
}

/// `free(*slot); *slot = NULL` when the slot holds a pointer.
fn free_slot(m: &mut Mem, slot: u32) {
    let p = m.u32(slot);
    if p != 0 {
        m.free(p);
        m.set_u32(slot, 0);
    }
}

/// [soft 0x0811e7d0 si_attdictInitAttribute]: the initial values of an attribute. Six of
/// the seven path entries are cleared; the last one and the padding are left as they were.
pub fn init_attribute(m: &mut Mem, a: Ptr32<Attribute>) {
    let a = &mut m[a];
    a.unk_00 = 0;
    a.unk_04 = 0;
    a.index.depth = 0;
    for p in &mut a.index.path[..6] {
        *p = 0;
    }
    a.range_low[0] = 0;
    a.range_high[0] = 0;
    a.index.value_index = 0;
    a.id = 0;
    a.name = Ptr32::NULL;
    a.description = Ptr32::NULL;
    a.access = 0;
    a.scope = 0;
    a.value_type = 1;
    a.enum_type = 0;
    a.range_low[1] = 0;
    a.range_low_flag = 1;
    a.range_high[1] = 0;
    a.range_high_flag = 1;
    a.group = 0;
    a.rank = 0;
    a.visible = 0;
    a.nb_values = 0;
    a.fcv_ids = Ptr32::NULL;
    a.fcv_names = Ptr32::NULL;
    a.unk_60 = 0;
    a.incmode = 3;
}

/// [soft 0x0811e890 attdictInitAttribGroup]: the initial values of a group.
pub fn init_attrib_group(m: &mut Mem, g: Ptr32<AttribGroup>) {
    let g = &mut m[g];
    g.nb_attribs = 0;
    g.attribs = Ptr32::NULL;
    g.cond = Ptr32::NULL;
    g.nb_subs = 0;
    g.subs = Ptr32::NULL;
    g.name = Ptr32::NULL;
    g.flag = 1;
    g.rel_type = 0;
    g.get_value = Ptr32::new(SI_DUMMY_ATTR_GET_VALUE);
    g.set_value = Ptr32::new(SI_DUMMY_ATTR_SET_VALUE);
}

/// [soft 0x0811e8e0 SI_attdictInit]
pub fn init(m: &mut Mem, atd: Ptr32<AttributeDict>) {
    let d = &mut m[atd];
    d.groups = Ptr32::NULL;
    d.nb_groups = 0;
    d.attributes = Ptr32::NULL;
    d.nb_attributes = 0;
    d.fcv_index = Ptr32::NULL;
    d.unk_14 = 0;
    d.dirty = 1;
}

/// [soft 0x0811e91c si_attdictAllocateFcurveArray]: gives an attribute `count`
/// components: the two arrays from `SI_calloc`, the ids all -1 and the names all null.
pub fn allocate_fcurve_array(m: &mut Mem, a: Ptr32<Attribute>, count: i32) {
    m[a].nb_values = count as i16;
    m[a].fcv_ids = Ptr32::new(m.si_calloc(count as u32, 2));
    m[a].fcv_names = Ptr32::new(m.si_calloc(count as u32, 4));
    let mut i = 0;
    while i < count {
        let ids = m[a].fcv_ids;
        m[ids.add(i)] = -1;
        let names = m[a].fcv_names;
        m[names.add(i)] = Ptr32::NULL;
        i += 1;
    }
}

/// [soft 0x0811e984 si_attdictAddAttribute]: appends a copy of `src` (all 100 bytes, over
/// the initial values) to the dictionary and its index to the group's list.
pub fn add_attribute(m: &mut Mem, atd: Ptr32<AttributeDict>, group: Ptr32<AttribGroup>, src: Ptr32<Attribute>) {
    let n = m[atd].nb_attributes;
    let old = m[atd].attributes;
    let p = if old.is_null() { m.si_malloc(100) } else { m.si_realloc(old.addr(), ((n + 1) * 100) as u32) };
    m[atd].attributes = Ptr32::new(p);
    let dst = m[atd].attributes.add(n);
    init_attribute(m, dst);
    for k in 0..25 {
        let w = m.u32(src.addr() + k * 4);
        m.set_u32(dst.addr() + k * 4, w);
    }
    m[atd].nb_attributes += 1;
    m[atd].dirty = 1;
    let k = m[group].nb_attribs;
    let old = m[group].attribs;
    let p = if old.is_null() { m.si_malloc(4) } else { m.si_realloc(old.addr(), (k * 4 + 4) as u32) };
    m[group].attribs = Ptr32::new(p);
    let slot = m[group].attribs.add(k);
    m[slot] = n;
    m[group].nb_attribs += 1;
}

/// [soft 0x0811ea5c si_attdictAddAttribGroup]: appends a top-level group. The name and
/// the condition are stored as given. The first group of a dictionary gets flag 0.
pub fn add_attrib_group(m: &mut Mem, atd: Ptr32<AttributeDict>, name: CStr32, cond: u32, rel_type: i32) {
    let n = m[atd].nb_groups;
    let old = m[atd].groups;
    let p = if old.is_null() { m.si_malloc(0x28) } else { m.si_realloc(old.addr(), ((n * 5 + 5) * 8) as u32) };
    m[atd].groups = Ptr32::new(p);
    let g = m[atd].groups.add(n);
    init_attrib_group(m, g);
    m[g].name = name;
    m[g].cond = Ptr32::new(cond);
    m[g].rel_type = rel_type;
    if n == 0 {
        m[g].flag = 0;
    }
    m[atd].nb_groups += 1;
    m[atd].dirty = 1;
}

/// [soft 0x0811eae0 si_attdictAddAttribSubGroup]: appends a sub-group and answers it;
/// null for a null group.
pub fn add_attrib_sub_group(m: &mut Mem, group: Ptr32<AttribGroup>, name: CStr32, cond: u32) -> Ptr32<AttribGroup> {
    if group.is_null() {
        return Ptr32::NULL;
    }
    let n = m[group].nb_subs;
    let old = m[group].subs;
    let p = if old.is_null() { m.si_malloc(0x28) } else { m.si_realloc(old.addr(), ((n * 5 + 5) * 8) as u32) };
    m[group].subs = Ptr32::new(p);
    let g = m[group].subs.add(n);
    init_attrib_group(m, g);
    m[g].name = name;
    m[g].cond = Ptr32::new(cond);
    m[group].nb_subs += 1;
    g
}

/// [soft 0x0811eb58 attdictAttribGroupDispose]: frees a group's list of attributes, its
/// name, its condition (the condition's array first) and, after each sub-group, the array
/// of sub-groups. Every pointer is cleared after its `free`.
pub fn attrib_group_dispose(m: &mut Mem, g: Ptr32<AttribGroup>) {
    free_slot(m, field!(g, AttribGroup, attribs).addr());
    free_slot(m, field!(g, AttribGroup, name).addr());
    if !m[g].cond.is_null() {
        let cond = cond_of(m, g);
        free_slot(m, field!(cond, AttribCond, values).addr());
        free_slot(m, field!(g, AttribGroup, cond).addr());
    }
    let mut i = 0;
    while i < m[g].nb_subs {
        let sub = m[g].subs.add(i);
        attrib_group_dispose(m, sub);
        i += 1;
    }
    free_slot(m, field!(g, AttribGroup, subs).addr());
}

/// [soft 0x0811ec1c SI_attdictDispose]: frees every attribute's component names, its two
/// arrays, its name and its description, every group, the curve index and the two
/// arrays, and initialises the dictionary again.
pub fn dispose(m: &mut Mem, atd: Ptr32<AttributeDict>) {
    let mut i = 0;
    while i < m[atd].nb_attributes {
        let a = m[atd].attributes.add(i);
        i += 1;
        let mut k = 0;
        while k < m[a].nb_values as i32 {
            let slot = m[a].fcv_names.add(k).addr();
            free_slot(m, slot);
            k += 1;
        }
        free_slot(m, field!(a, Attribute, fcv_names).addr());
        free_slot(m, field!(a, Attribute, fcv_ids).addr());
        free_slot(m, field!(a, Attribute, name).addr());
        free_slot(m, field!(a, Attribute, description).addr());
    }
    let mut i = 0;
    while i < m[atd].nb_groups {
        let g = m[atd].groups.add(i);
        attrib_group_dispose(m, g);
        i += 1;
    }
    free_slot(m, field!(atd, AttributeDict, fcv_index).addr());
    free_slot(m, field!(atd, AttributeDict, attributes).addr());
    free_slot(m, field!(atd, AttributeDict, groups).addr());
    init(m, atd);
}

/// [soft 0x0811ed88 si_attdictAttribSubGroupGetByName]: the group itself when it has the
/// name, else the first of its sub-groups (searched in depth) that has it; null for a
/// null group or when none has.
pub fn attrib_sub_group_get_by_name(m: &Mem, g: Ptr32<AttribGroup>, name: CStr32) -> Ptr32<AttribGroup> {
    if g.is_null() {
        return Ptr32::NULL;
    }
    if strcmp(m, m[g].name.addr(), name.addr()) == 0 {
        return g;
    }
    let mut i = 0;
    while i < m[g].nb_subs {
        let found = attrib_sub_group_get_by_name(m, m[g].subs.add(i), name);
        if !found.is_null() {
            return found;
        }
        i += 1;
    }
    Ptr32::NULL
}

/// [soft 0x0811edf8 SI_attdictAttribGroupGetByName]: the first group of that name, the
/// top-level groups gone through in order and each searched in depth; null when none.
pub fn attrib_group_get_by_name(m: &Mem, atd: Ptr32<AttributeDict>, name: CStr32) -> Ptr32<AttribGroup> {
    let mut i = 0;
    while i < m[atd].nb_groups {
        let found = attrib_sub_group_get_by_name(m, m[atd].groups.add(i), name);
        if !found.is_null() {
            return found;
        }
        i += 1;
    }
    Ptr32::NULL
}

/// [soft 0x0811ee3c SI_attdictAttribGroupGetByIndex]: the group a path leads to: `path[0]`
/// a top-level group, `path[1]` to `path[depth - 2]` sub-groups (the last entry names an
/// attribute and is not looked at). Null for a depth below 1 or an entry out of range.
/// A depth above 8 reads beyond the path.
pub fn attrib_group_get_by_index(m: &Mem, atd: Ptr32<AttributeDict>, index: Ptr32<AttGrpIndex>) -> Ptr32<AttribGroup> {
    let first = short_at(m, index, 2);
    if m[index].depth < 1 || first < 0 || m[atd].nb_groups <= first as i32 {
        return Ptr32::NULL;
    }
    let mut g = m[atd].groups.add(first as i32);
    let mut level = 1;
    while level < m[index].depth as i32 - 1 {
        let s = short_at(m, index, level * 2 + 2);
        if s < 0 || m[g].nb_subs <= s as i32 {
            return Ptr32::NULL;
        }
        g = m[g].subs.add(s as i32);
        level += 1;
    }
    g
}

/// [soft 0x0811eeb4 SI_attdictAttributeGetByName]: the first attribute of that name, or
/// null.
pub fn attribute_get_by_name(m: &Mem, atd: Ptr32<AttributeDict>, name: CStr32) -> Ptr32<Attribute> {
    let mut i = 0;
    while i < m[atd].nb_attributes {
        let a = m[atd].attributes.add(i);
        if strcmp(m, m[a].name.addr(), name.addr()) == 0 {
            return a;
        }
        i += 1;
    }
    Ptr32::NULL
}

/// [soft 0x0811ef0c SI_attdictAttributeGetByIndex]: the attribute a path names: the group
/// as [`attrib_group_get_by_index`] finds it, then `path[depth - 1]` as a position in
/// that group's list. With a depth of 1 the one entry is used both as group and as
/// position. Null where an entry is out of range.
pub fn attribute_get_by_index(m: &Mem, atd: Ptr32<AttributeDict>, index: Ptr32<AttGrpIndex>) -> Ptr32<Attribute> {
    let depth = m[index].depth as i32;
    let first = short_at(m, index, 2) as i32;
    if 0 < depth && -1 < first && first < m[atd].nb_groups {
        let mut g = m[atd].groups.add(first);
        let mut level = 1;
        while level < depth - 1 {
            let s = short_at(m, index, level * 2 + 2);
            if s < 0 {
                return Ptr32::NULL;
            }
            if m[g].nb_subs <= s as i32 {
                return Ptr32::NULL;
            }
            g = m[g].subs.add(s as i32);
            level += 1;
        }
        let pos = short_at(m, index, depth * 2) as i32;
        if -1 < pos && pos < m[g].nb_attribs {
            let which = m[m[g].attribs.add(pos)];
            return m[atd].attributes.add(which);
        }
    }
    Ptr32::NULL
}

/// [soft 0x0811efb4 SI_attdictAttributeSetAccessMode]: sets the access word of the
/// attribute a path names; nothing when it names none.
pub fn attribute_set_access_mode(m: &mut Mem, atd: Ptr32<AttributeDict>, index: Ptr32<AttGrpIndex>, mode: i16) {
    let a = attribute_get_by_index(m, atd, index);
    if !a.is_null() {
        m[a].access = mode;
    }
}

/// [soft 0x0811efe0 SI_attdictAttributeGetAccessMode]: stores the access word in `*out`
/// and answers 1; 0 for a null `out` or a path that names nothing.
pub fn attribute_get_access_mode(m: &mut Mem, atd: Ptr32<AttributeDict>, index: Ptr32<AttGrpIndex>, out: Ptr32<i16>) -> i32 {
    if !out.is_null() {
        let a = attribute_get_by_index(m, atd, index);
        if !a.is_null() {
            m[out] = m[a].access;
            return 1;
        }
    }
    0
}

/// [soft 0x0811f01c SI_attdictEvalCond]: whether a condition holds for an element. No
/// condition holds always; no element, never. Otherwise the condition's attribute (an
/// index into `SI_ATD`'s array) is read from the element by its group's reading function,
/// and the condition holds when the value is of type 3 (an enumeration) and its number
/// (the value's third word) is one of the condition's.
///
/// The element is looked up in the current dictionary (`SI_elemDicts[*SI_procData]`)
/// first: a slot that is not there gives 0. The reading function is called through the
/// group's pointer with the element's identity, the attribute's path and the address of a
/// local value. When the pointer is null the original tests the two locals without
/// having set them; they are words of the image's stack here too, and keep what the call
/// before left there.
pub fn eval_cond(m: &mut Mem, o: &mut dyn Outside, cond: Ptr32<AttribCond>, ident: Ptr32<ElemIdentity>) -> i32 {
    if cond.is_null() {
        return 1;
    }
    if ident.is_null() {
        return 0;
    }
    let path: Ptr32<AttGrpIndex> = field!(m[SI_ATD].attributes.add(m[cond].attr), Attribute, index);
    let group = attrib_group_get_by_index(m, SI_ATD, path);
    if group.is_null() {
        return 0;
    }
    let ed = m[SI_ELEM_DICTS].add(m[m[SI_PROC_DATA]]);
    if elemdict::get_elem(m, ed, m[ident].chapid, m[ident].elemid).is_null() {
        return 0;
    }
    // the frame's two locals, where the original has them below the stack pointer it was
    // entered with (the reading function is handed the value's address): the answer (16
    // bits, at ebp-0xe) and the value (12 bytes, at ebp-0xc)
    let frame = crate::frame(m, 0x14);
    let (ok, value): (Ptr32<i16>, Ptr32<AttributeValue>) = (Ptr32::new(frame + 2), Ptr32::new(frame + 4));
    let read = m[group].get_value;
    if !read.is_null() {
        let r = o.call_addr(m, read.addr(), &[ident.addr(), path.addr(), value.addr()]);
        m[ok] = r as i16;
    }
    let mut holds = 0;
    if m[ok] != 0 && m[value].value_type == 3 {
        let number = m[value].value2 as i32;
        let (n, values) = (m[cond].nb, m[cond].values);
        let mut i = 0;
        while i < n {
            if number == m[values.add(i)] {
                holds = 1;
                break;
            }
            i += 1;
        }
    }
    m.release(0x14);
    holds
}

/// [soft 0x0811f0ec attdictEvalGroupCond]: a group's condition for an element; 0 for a
/// null group.
pub fn eval_group_cond(m: &mut Mem, o: &mut dyn Outside, g: Ptr32<AttribGroup>, ident: Ptr32<ElemIdentity>) -> i32 {
    if g.is_null() {
        return 0;
    }
    let cond = cond_of(m, g);
    // (the 0x20 bytes of stack this routine's frame and call take: see `eval_cond`)
    crate::frame(m, 0x20);
    let holds = eval_cond(m, o, cond, ident) as i16 as i32;
    m.release(0x20);
    holds
}

/// [soft 0x0811f118 SI_attdictValidateAttr]: whether the conditions of every group on a
/// path of `SI_ATD` hold for an element: 0 for a null argument, for a sub-group entry out
/// of range, and as soon as one condition fails. The first entry is not tested against
/// the number of groups.
pub fn validate_attr(m: &mut Mem, o: &mut dyn Outside, ident: Ptr32<ElemIdentity>, index: Ptr32<AttGrpIndex>) -> i32 {
    if ident.is_null() || index.is_null() || index.addr().wrapping_add(2) == 0 {
        return 0;
    }
    let mut g = m[SI_ATD].groups.add(short_at(m, index, 2) as i32);
    // (the 0x30 bytes of stack this routine's frame and calls take: see `eval_cond`)
    crate::frame(m, 0x30);
    let mut level = 1;
    let mut holds = 1;
    while level < m[index].depth as i32 - 1 {
        let s = short_at(m, index, level * 2 + 2);
        if s < 0 || m[g].nb_subs <= s as i32 {
            holds = 0;
            break;
        }
        if eval_group_cond(m, o, g, ident) as i16 == 0 {
            holds = 0;
            break;
        }
        g = m[g].subs.add(short_at(m, index, level * 2 + 2) as i32);
        level += 1;
    }
    if holds != 0 {
        holds = eval_group_cond(m, o, g, ident) as i16 as i32;
    }
    m.release(0x30);
    holds
}

/// [soft 0x0811f1c0 SI_attdictRangeAdjust]: brings a value inside an attribute's range.
/// Answers 1, or 0 for a value type the routine has no case for (when the attribute has
/// a bound at all).
///
/// A bound exists when its word is 1 (`range_low[1]`, `range_high[1]`); the bound itself
/// is a float. A float value (type 1) and a double (type 6) are compared with the bound
/// in the x87 unit and replaced by it when beyond; nothing is done when the comparison
/// is unordered. An integer (type 4) and a 16-bit integer (type 8) are compared with the
/// bound cut to an integer (toward zero; a bound that does not fit 32 bits gives
/// 0x80000000, and for type 8 its low 16 bits are taken).
pub fn range_adjust(m: &mut Mem, a: Ptr32<Attribute>, v: Ptr32<AttributeValue>) -> i32 {
    use core::cmp::Ordering::{Greater, Less};
    let (low_kind, high_kind) = (m[a].range_low[1], m[a].range_high[1]);
    if low_kind == 0 && high_kind == 0 {
        return 1;
    }
    let low = F80::from_f32(f32::from_bits(m[a].range_low[0]));
    let high = F80::from_f32(f32::from_bits(m[a].range_high[0]));
    let at = field!(v, AttributeValue, value).addr();
    match m[v].value_type {
        1 => {
            if low_kind == 1 && low.compare(F80::from_f32(m.f32(at))) == Some(Greater) {
                m.set_f32(at, low.to_f32());
            }
            if high_kind == 1 && high.compare(F80::from_f32(m.f32(at))) == Some(Less) {
                m.set_f32(at, high.to_f32());
            }
            1
        }
        6 => {
            if low_kind == 1 && low.compare(F80::from_f64(m.f64(at))) == Some(Greater) {
                m.set_f64(at, low.to_f64());
            }
            if high_kind == 1 && high.compare(F80::from_f64(m.f64(at))) == Some(Less) {
                m.set_f64(at, high.to_f64());
            }
            1
        }
        4 => {
            if low_kind == 1 {
                let bound = low.to_i32(Round::Zero);
                if m.i32(at) < bound {
                    m.set_i32(at, bound);
                }
            }
            if high_kind == 1 {
                let bound = high.to_i32(Round::Zero);
                if m.i32(at) > bound {
                    m.set_i32(at, bound);
                }
            }
            1
        }
        8 => {
            if low_kind == 1 {
                let bound = low.to_i32(Round::Zero) as i16;
                if m.i16(at) < bound {
                    m.set_i16(at, bound);
                }
            }
            if high_kind == 1 {
                let bound = high.to_i32(Round::Zero) as i16;
                if m.i16(at) > bound {
                    m.set_i16(at, bound);
                }
            }
            1
        }
        _ => 0,
    }
}

/// [soft 0x0811f370 SI_attdictSetFctsPointers]: installs the reading and writing
/// functions of the 28 groups the program has code for ([`ATT_FCTS`]: the group's name,
/// then the two code addresses; `BoundingBox` has no writing function). A group that is
/// not in the dictionary is passed over.
pub fn set_fcts_pointers(m: &mut Mem, atd: Ptr32<AttributeDict>) {
    for &(name, _, get, _, set, _) in ATT_FCTS {
        let g = attrib_group_get_by_name(m, atd, Ptr32::new(name));
        if !g.is_null() {
            m[g].get_value = Ptr32::new(get);
            m[g].set_value = Ptr32::new(set);
        }
    }
}

/// [soft 0x0811f754 SI_attdictAttGrpGetAttrib]: the attribute of a group that follows
/// `after`, or the group's first for a null `after`; null for a null or empty group and
/// after the last. The successor is found through a copy of `after`'s own path with its
/// last entry raised by one, so `after` need not be of this group at all. (The entry
/// raised is the 16-bit word number `depth` of the copy, wherever that lies.)
pub fn att_grp_get_attrib(m: &mut Mem, atd: Ptr32<AttributeDict>, g: Ptr32<AttribGroup>, after: Ptr32<Attribute>) -> Ptr32<Attribute> {
    if g.is_null() || m[g].nb_attribs < 1 {
        return Ptr32::NULL;
    }
    if after.is_null() {
        let first = m[m[g].attribs];
        return m[atd].attributes.add(first);
    }
    let local: Ptr32<AttGrpIndex> = Ptr32::new(m.local(0x14));
    let path = m.read(field!(after, Attribute, index));
    m.write(local, path);
    let at = local.addr().wrapping_add((m[local].depth as i32 * 2) as u32);
    let raised = m.i16(at).wrapping_add(1);
    m.set_i16(at, raised);
    let found = attribute_get_by_index(m, atd, local);
    m.release(0x14);
    found
}

/// [soft 0x0811f7b4 SI_attdictAttGrpGetNbAttributes]
pub fn att_grp_get_nb_attributes(m: &Mem, g: Ptr32<AttribGroup>) -> i32 {
    if g.is_null() {
        0
    } else {
        m[g].nb_attribs
    }
}

/// [soft 0x0811f7cc SI_attdictAttGrpGetAttribute]: the `i`-th attribute of a group, or
/// null.
pub fn att_grp_get_attribute(m: &Mem, atd: Ptr32<AttributeDict>, g: Ptr32<AttribGroup>, i: i32) -> Ptr32<Attribute> {
    if g.is_null() || i < 0 || m[g].nb_attribs <= i {
        return Ptr32::NULL;
    }
    m[atd].attributes.add(m[m[g].attribs.add(i)])
}

/// A `%s` argument as the C library prints it.
fn text(m: &Mem, s: CStr32) -> String {
    if s.is_null() {
        "(null)".to_string()
    } else {
        String::from_utf8_lossy(&m.cstr(s.addr())).into_owned()
    }
}

/// [soft 0x0811f7fc attdictAttribGroupDump]: prints a group on the standard error: its
/// name, position and depth, its condition, then each attribute (its path, owner, id and
/// name, and each component's curve id and name), then its sub-groups one level deeper.
pub fn attrib_group_dump(m: &Mem, o: &mut dyn Outside, atd: Ptr32<AttributeDict>, g: Ptr32<AttribGroup>, position: i32, level: i32) {
    o.stderr(format!("Attribute Group [{}] index({}) lev({})\n", text(m, m[g].name), position, level).as_bytes());
    let cond = cond_of(m, g);
    if !cond.is_null() {
        o.stderr(format!("Condition ( attrib {} = ", m[cond].attr).as_bytes());
        let mut i = 0;
        while i < m[cond].nb {
            o.stderr(format!("{},", m[m[cond].values.add(i)]).as_bytes());
            i += 1;
        }
        o.stderr(b" )\n");
    }
    o.stderr(b"-------------------------\n");
    let n = att_grp_get_nb_attributes(m, g);
    let mut i = 0;
    while i < n {
        let a = att_grp_get_attribute(m, atd, g, i);
        o.stderr(b"[");
        let mut k: i16 = 0;
        while k < m[a].index.depth {
            let entry = m.i16(field!(a, Attribute, index).addr() + 2 + (k as i32 * 2) as u32);
            o.stderr(format!("{entry} ").as_bytes());
            k = k.wrapping_add(1);
        }
        o.stderr(b"]\n");
        let owner = if m[a].unk_00 == 0 { "Elem" } else { "Oper" };
        o.stderr(format!("{}) ATTRIB [{}] owner_id({})  att_id({}) name[{}]\n", i, owner, m[a].unk_04, m[a].id, text(m, m[a].name)).as_bytes());
        let mut c = 0;
        while c < m[a].nb_values as i32 {
            let id = m[m[a].fcv_ids.add(c)];
            let name = m[m[a].fcv_names.add(c)];
            o.stderr(format!("\t\t{}) fcv id({}) name[{}]\n", c, id, text(m, name)).as_bytes());
            c += 1;
        }
        i += 1;
    }
    let mut s = 0;
    while s < m[g].nb_subs {
        attrib_group_dump(m, o, atd, m[g].subs.add(s), s, level + 1);
        s += 1;
    }
}

/// [soft 0x0811fa00 SI_attdictDump]: prints the whole dictionary on the standard error.
pub fn dump(m: &Mem, o: &mut dyn Outside, atd: Ptr32<AttributeDict>) {
    o.stderr(b" Dictionary of Attributes :\n");
    o.stderr(b"====================================\n");
    let mut i = 0;
    while i < m[atd].nb_groups {
        attrib_group_dump(m, o, atd, m[atd].groups.add(i), i, 0);
        i += 1;
    }
    o.stderr(b"====================================\n");
}

/// [soft 0x0811fa78 attdictAttribGroupUpdateIndexes]: writes every attribute of a group
/// its own path: the `level` entries of `path` that lead to the group, then its position
/// in the group, with a depth of `level + 1`; then the same for each sub-group with the
/// sub-group's position as entry `level` of `path`.
pub fn attrib_group_update_indexes(m: &mut Mem, atd: Ptr32<AttributeDict>, g: Ptr32<AttribGroup>, level: i32, path: Ptr32<AttGrpIndex>) {
    let mut i = 0;
    while i < m[g].nb_attribs {
        let a = m[atd].attributes.add(m[m[g].attribs.add(i)]);
        let own = field!(a, Attribute, index).addr();
        let mut k = 0;
        while k < level {
            let entry = m.i16(path.addr().wrapping_add((2 + k * 2) as u32));
            m.set_i16(own.wrapping_add((2 + k * 2) as u32), entry);
            k += 1;
        }
        m.set_i16(own, (level as i16).wrapping_add(1));
        m.set_i16(own.wrapping_add((2 + level * 2) as u32), i as i16);
        i += 1;
    }
    let mut s = 0;
    while s < m[g].nb_subs {
        m.set_i16(path.addr().wrapping_add((2 + level * 2) as u32), s as i16);
        let sub = m[g].subs.add(s);
        attrib_group_update_indexes(m, atd, sub, level + 1, path);
        s += 1;
    }
}

/// [soft 0x0811fb3c SI_attdictUpdateIndexes]: writes every attribute of the dictionary
/// its own path, through a path kept in a local.
pub fn update_indexes(m: &mut Mem, atd: Ptr32<AttributeDict>) {
    let local: Ptr32<AttGrpIndex> = Ptr32::new(m.local(0x14));
    let mut i = 0;
    while i < m[atd].nb_groups {
        m.set_i16(local.addr() + 2, i as i16);
        let g = m[atd].groups.add(i);
        attrib_group_update_indexes(m, atd, g, 1, local);
        i += 1;
    }
    m.release(0x14);
}

/// [soft 0x0811fb84 SI_attdictCountAnimatedParams]: the sum of the attributes' numbers of
/// components, also stored as the length of the curve index (+0x14).
pub fn count_animated_params(m: &mut Mem, atd: Ptr32<AttributeDict>) -> i32 {
    let mut sum = 0i32;
    let n = m[atd].nb_attributes;
    let attrs = m[atd].attributes;
    let mut i = 0;
    while i < n {
        sum = sum.wrapping_add(m[attrs.add(i)].nb_values as i32);
        i += 1;
    }
    m[atd].unk_14 = sum;
    sum
}

/// The curve id an entry of the index stands for, read through `glbATD` as the two
/// comparison functions read it: `glbATD->attributes[attr].fcv_ids[component]`.
fn id_of(m: &Mem, entry: Ptr32<FcvAtdIndex>) -> i16 {
    let a = m[m[GLB_ATD]].attributes.add(m[entry].attr);
    m[m[a].fcv_ids.add(m[entry].component)]
}

/// [soft 0x0811fbb8 attdictCompareIds]: `qsort`'s comparison of two entries of the curve
/// index by their curve ids (16 bits, signed): -1, 0 or 1.
pub fn compare_ids(m: &Mem, a: Ptr32<FcvAtdIndex>, b: Ptr32<FcvAtdIndex>) -> i32 {
    let (x, y) = (id_of(m, a), id_of(m, b));
    if x < y {
        -1
    } else if y < x {
        1
    } else {
        0
    }
}

/// [soft 0x0811fc14 attdictInitFcvIndexes]: fills the index in the attributes' order: one
/// entry per component (attribute, component, flag 0; the two bytes after the flag are
/// not written).
pub fn init_fcv_indexes(m: &mut Mem, atd: Ptr32<AttributeDict>, base: Ptr32<FcvAtdIndex>) {
    let mut at = 0;
    let mut i = 0;
    while i < m[atd].nb_attributes {
        let a = m[atd].attributes.add(i);
        let mut k = 0;
        while k < m[a].nb_values as i32 {
            let e = base.add(at);
            m[e].attr = i;
            m[e].component = k;
            m[e].flag = 0;
            at += 1;
            k += 1;
        }
        i += 1;
    }
}

/// [soft 0x0811fc80 SI_attdictSortAttribFromFcurveIds]: builds the curve index: an array
/// from `SI_calloc`, filled in the attributes' order and sorted by curve id with `qsort`
/// (which keeps entries of one id in their first order). The old index is freed first;
/// the dictionary is marked clean. Nothing for a null dictionary or one without groups.
pub fn sort_attrib_from_fcurve_ids(m: &mut Mem, atd: Ptr32<AttributeDict>) {
    if atd.is_null() || m[atd].nb_groups == 0 || m[atd].groups.is_null() {
        return;
    }
    free_slot(m, field!(atd, AttributeDict, fcv_index).addr());
    m[GLB_ATD] = atd;
    let n = count_animated_params(m, atd);
    let base: Ptr32<FcvAtdIndex> = Ptr32::new(m.si_calloc(n as u32, 0xc));
    init_fcv_indexes(m, atd, base);
    if 1 < n {
        qsort(m, base.addr(), n as u32, 0xc, &mut |m, a, b| compare_ids(m, Ptr32::new(a), Ptr32::new(b)));
    }
    m[atd].fcv_index = base.cast();
    m[atd].dirty = 0;
}

/// [soft 0x0811fd10 attdictBSearchCompareIds]: `bsearch`'s comparison: an entry whose
/// attribute is -1 is a key and stands for the number in its second word; any other
/// stands for its curve id.
pub fn bsearch_compare_ids(m: &Mem, a: Ptr32<FcvAtdIndex>, b: Ptr32<FcvAtdIndex>) -> i32 {
    let value = |e: Ptr32<FcvAtdIndex>| if m[e].attr == -1 { m[e].component } else { id_of(m, e) as i32 };
    let (x, y) = (value(a), value(b));
    if x < y {
        -1
    } else if y < x {
        1
    } else {
        0
    }
}

/// [soft 0x0811fd8c SI_attdictGetGrpIndexFromFcurve]: stores in `*out` the path of the
/// attribute that owns a curve id, with the curve's component as the path's component,
/// and answers 1; 0 (and nothing stored) for the custom-effect id, without attributes or
/// index, and for an id no attribute owns.
pub fn get_grp_index_from_fcurve(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32, out: Ptr32<AttGrpIndex>) -> i32 {
    if id != FCV_CUSTOM && !atd.is_null() && !m[atd].attributes.is_null() && !m[atd].fcv_index.is_null() {
        let at = find_fcurve(m, atd, id);
        if at != -1 {
            let e = index_of(m, atd).add(at);
            let path = m.read(field!(m[atd].attributes.add(m[e].attr), Attribute, index));
            m.write(out, path);
            m[out].value_index = m[e].component;
            return 1;
        }
    }
    0
}

/// [soft 0x0811fdfc SI_attdictFindFcurve]: the position in the curve index of a curve id,
/// or -1 (also without attributes or index).
///
/// The last answer is remembered in two statics: the same id answers at once, and the id
/// one above it answers with the next entry when that entry is the next component of the
/// same attribute and that component has this id. Otherwise the index is searched with
/// `bsearch`, so of several entries with one id the one the bisection meets first
/// answers. The remembered position is used only while it is below the index's length.
pub fn find_fcurve(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32) -> i32 {
    if atd.is_null() {
        return -1;
    }
    let attrs = m[atd].attributes;
    if attrs.is_null() {
        return -1;
    }
    let index = index_of(m, atd);
    if index.is_null() {
        return -1;
    }
    let last = m[LAST_SORT_IND_327];
    if last != -1 && last < m[atd].unk_14 {
        let e = index.add(last);
        let (attr, component) = (m[e].attr, m[e].component);
        let last_id = m[LAST_FCV_ID_328];
        if id == last_id {
            return last;
        }
        if last_id.wrapping_add(1) == id {
            let a = attrs.add(attr);
            let next = component.wrapping_add(1);
            if next < m[a].nb_values as i32
                && m[m[a].fcv_ids.add(next)] as i32 == id
                && last + 1 < m[atd].unk_14
                && m[index.add(last + 1)].component == next
            {
                m[LAST_SORT_IND_327] = last + 1;
                m[LAST_FCV_ID_328] = id;
                return last + 1;
            }
        }
    }
    // the key: an entry whose attribute is -1 and whose second word is the id
    let key: Ptr32<FcvAtdIndex> = Ptr32::new(m.local(0xc));
    m[key].attr = -1;
    m[key].component = id;
    m[GLB_ATD] = atd;
    let (base, n) = (m[atd].fcv_index.addr(), m[atd].unk_14 as u32);
    let found = bsearch(m, key.addr(), base, n, 0xc, &mut |m, a, b| bsearch_compare_ids(m, Ptr32::new(a), Ptr32::new(b)));
    m.release(0xc);
    if found == 0 {
        return -1;
    }
    m[LAST_FCV_ID_328] = id;
    let at = (found.wrapping_sub(m[atd].fcv_index.addr()) as i32).wrapping_mul(0xaaaa_aaabu32 as i32) >> 2;
    m[LAST_SORT_IND_327] = at;
    at
}

/// [soft 0x0811ff18 SI_attdictSetFcurveFlag]: sets the 16-bit flag of a curve id's entry
/// and answers 1; 0 when the id is not in the index.
pub fn set_fcurve_flag(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32, flag: i32) -> i32 {
    if !atd.is_null() && !m[atd].attributes.is_null() && !m[atd].fcv_index.is_null() {
        let at = find_fcurve(m, atd, id);
        if at != -1 {
            let e = index_of(m, atd).add(at);
            m[e].flag = flag as i16;
            return 1;
        }
    }
    0
}

/// [soft 0x0811ff64 SI_attdictGetFcurveFlag]: that flag; 0 when the id is not in the index.
pub fn get_fcurve_flag(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32) -> i32 {
    if !atd.is_null() && !m[atd].attributes.is_null() && !m[atd].fcv_index.is_null() {
        let at = find_fcurve(m, atd, id);
        if at != -1 {
            return m[index_of(m, atd).add(at)].flag as i32;
        }
    }
    0
}

/// The compiler's inlined comparison of a name with a literal of six characters: seven
/// bytes, read no further than the first difference.
fn same7(m: &Mem, name: CStr32, literal: CStr32) -> bool {
    (0..7).all(|k| m.u8(name.addr().wrapping_add(k)) == m.u8(literal.addr() + k))
}

/// [soft 0x0811ffa8 SI_attdictGetFcurveFromName]: the curve id of a component's name, -1
/// when there is none. The index is gone through in its order (by id) and the first
/// component of that name answers; then four names of older versions: `hidden` 0xaf,
/// `camhid` 0xb1, `lighid` 0xb0, `t2norm` 0x2b. -1 without attributes or index.
pub fn get_fcurve_from_name(m: &Mem, atd: Ptr32<AttributeDict>, name: CStr32) -> i32 {
    if atd.is_null() || m[atd].attributes.is_null() || m[atd].fcv_index.is_null() {
        return -1;
    }
    let mut i = 0;
    while i < m[atd].unk_14 {
        let e = index_of(m, atd).add(i);
        let a = m[atd].attributes.add(m[e].attr);
        let own = m[m[a].fcv_names.add(m[e].component)];
        if strcmp(m, own.addr(), name.addr()) == 0 {
            let a = m[atd].attributes.add(m[e].attr);
            return m[m[a].fcv_ids.add(m[e].component)] as i32;
        }
        i += 1;
    }
    if same7(m, name, S_HIDDEN) {
        return 0xaf;
    }
    if same7(m, name, S_CAMHID) {
        return 0xb1;
    }
    if same7(m, name, S_LIGHID) {
        return 0xb0;
    }
    if same7(m, name, S_T2NORM) {
        return 0x2b;
    }
    -1
}

/// [soft 0x081200b4 SI_attdictGetGroupCustomFcurve]: the group custom-effect curves
/// belong to.
pub fn get_group_custom_fcurve() -> i32 {
    5
}

/// [soft 0x081200c0 SI_attdictGetGroupFromFcurve]: the group of the attribute that owns a
/// curve id; 5 ("other") for the custom-effect id, without attributes or index, and for
/// an id no attribute owns.
pub fn get_group_from_fcurve(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32) -> i32 {
    if id == FCV_CUSTOM {
        return get_group_custom_fcurve();
    }
    if !atd.is_null() && !m[atd].attributes.is_null() && !m[atd].fcv_index.is_null() {
        let at = find_fcurve(m, atd, id);
        if at != -1 {
            let a = m[atd].attributes.add(m[index_of(m, atd).add(at)].attr);
            if !a.is_null() {
                return m[a].group as i32;
            }
        }
    }
    5
}

/// [soft 0x0812012c SI_attdictGetNameFromFcurve]: the name of a curve id's component, or
/// what the static `default_name` points at (`notfound`) for the custom-effect id,
/// without attributes or index, and for an id no attribute owns.
pub fn get_name_from_fcurve(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32) -> CStr32 {
    if id != FCV_CUSTOM && !atd.is_null() && !m[atd].attributes.is_null() && !m[atd].fcv_index.is_null() {
        let at = find_fcurve(m, atd, id);
        if at == -1 {
            return m[DEFAULT_NAME_347];
        }
        let e = index_of(m, atd).add(at);
        let a = m[atd].attributes.add(m[e].attr);
        if !a.is_null() {
            return m[m[a].fcv_names.add(m[e].component)];
        }
    }
    m[DEFAULT_NAME_347]
}

/// [soft 0x08120194 SI_attdictGetChapidFromFcurve]: the chapter of the elements a curve
/// id animates: 0 without attributes or index; -1 for an id no attribute owns; the
/// owner's id for an element's attribute; for an operator's the source chapter of the
/// relation whose id is the attribute's (`SI_reldictGetRelChapGrp` on `SI_RD`).
pub fn get_chapid_from_fcurve(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32) -> i32 {
    if atd.is_null() || m[atd].attributes.is_null() || m[atd].fcv_index.is_null() {
        return 0;
    }
    let at = find_fcurve(m, atd, id);
    if at == -1 {
        return -1;
    }
    let a = m[atd].attributes.add(m[index_of(m, atd).add(at)].attr);
    if a.is_null() {
        return -1;
    }
    if m[a].unk_00 == 0 {
        return m[a].unk_04 as i32;
    }
    let group: Ptr32<i32> = Ptr32::new(m.local(4));
    let chap = reldict::get_rel_chap_grp(m, SI_RD, m[a].id, group);
    m.release(4);
    chap
}

/// [soft 0x08120214 getOldSubdef]: stores in `*out` the rule number older versions gave
/// the relation of a curve id: 3 for 7, 9, 10, 0x1f, 0x20, 0xac and 0x6b to 0x8a, 2 for
/// 0xe9 to 0x100, 1 for every other id.
pub fn get_old_subdef(m: &mut Mem, id: i32, out: Ptr32<i32>) {
    m[out] = match id {
        7 | 9 | 10 | 0x1f | 0x20 | 0xac | 0x6b..=0x8a => 3,
        0xe9..=0x100 => 2,
        _ => 1,
    };
}

/// [soft 0x08120264 SI_attdictGetInfoFromFcurve]: what ties a curve to its element, in
/// four words of the caller: the relation id (the id of the attribute that owns the
/// curve), the attribute's group, the rule number ([`get_old_subdef`]) and the count
/// limit handed to the relation adder: 0 for an attribute with a single component, else
/// the component's position plus one. Defaults, stored first: relation 0, group 5, rule
/// 1, limit 1.
pub fn get_info_from_fcurve(m: &mut Mem, atd: Ptr32<AttributeDict>, id: i32, relid: Ptr32<i32>, group: Ptr32<i32>, rule: Ptr32<i32>, limit: Ptr32<i32>) {
    m[relid] = 0;
    m[limit] = 1;
    m[rule] = 1;
    m[group] = 5;
    if id != FCV_CUSTOM && !atd.is_null() && !m[atd].attributes.is_null() && !m[atd].fcv_index.is_null() {
        let at = find_fcurve(m, atd, id);
        if at != -1 {
            let e = index_of(m, atd).add(at);
            let a = m[atd].attributes.add(m[e].attr);
            if !a.is_null() {
                m[relid] = m[a].id;
                m[group] = m[a].group as i32;
                if m[a].nb_values == 1 {
                    m[limit] = 0;
                } else {
                    m[limit] = m[e].component.wrapping_add(1);
                }
                get_old_subdef(m, id, rule);
            }
        }
    }
}

/// The attribute with a given id as the two routines below look for it: the first whose
/// id it is, or the record behind the last when none has (`found` then false). With a
/// count below 1 the first record is answered unlooked at, as found unless the count is 0.
fn by_att_id(m: &Mem, atd: Ptr32<AttributeDict>, att_id: i32) -> (Ptr32<Attribute>, bool) {
    let n = m[atd].nb_attributes;
    let mut a = m[atd].attributes;
    let mut i = 0;
    if 0 < n {
        while m[a].id != att_id {
            i += 1;
            a = a.add(1);
            if n <= i {
                break;
            }
        }
    }
    (a, i != n)
}

/// [soft 0x08120320 SI_attdictGetFcurveFromAttId]: the first curve id of the attribute
/// with a given id; -1 when no attribute has that id or it has no component.
pub fn get_fcurve_from_att_id(m: &Mem, atd: Ptr32<AttributeDict>, att_id: i32) -> i32 {
    if !atd.is_null() && !m[atd].attributes.is_null() {
        let (a, found) = by_att_id(m, atd, att_id);
        if found && m[a].nb_values != 0 {
            return m[m[a].fcv_ids] as i32;
        }
    }
    -1
}

/// [soft 0x0812037c SI_attdictGetNbFcurvesForAttId]: how many components that attribute
/// has; -1 when it is not there or has none.
pub fn get_nb_fcurves_for_att_id(m: &Mem, atd: Ptr32<AttributeDict>, att_id: i32) -> i32 {
    if !atd.is_null() && !m[atd].attributes.is_null() {
        let (a, found) = by_att_id(m, atd, att_id);
        if found && m[a].nb_values != 0 {
            return m[a].nb_values as i32;
        }
    }
    -1
}

/// [soft 0x081203d0 SI_attdictAttribGetDefWidth]: the default column width for an
/// attribute, by its value type: 7, 3, 5, 6, 15 for the types 1 to 5; 10 when the path
/// names nothing or the type is another.
pub fn attrib_get_def_width(m: &Mem, atd: Ptr32<AttributeDict>, index: Ptr32<AttGrpIndex>) -> i32 {
    let a = attribute_get_by_index(m, atd, index);
    if a.is_null() {
        return 10;
    }
    match m[a].value_type {
        1 => 7,
        2 => 3,
        3 => 5,
        4 => 6,
        5 => 15,
        _ => 10,
    }
}

/// The step the two path look-ups share: among `n` groups at `groups`, the one whose
/// name is the path's first part; then, when the path goes on behind a `/`, the rest is
/// looked up among that group's sub-groups. The path is copied with `strdup` and cut at
/// its first `/`; the copy is freed only when a group was found.
fn by_path(m: &mut Mem, n_at: Ptr32<i32>, groups_at: Ptr32<Ptr32<AttribGroup>>, path: CStr32) -> Ptr32<AttribGroup> {
    let copy = m.strdup(path.addr());
    if copy == 0 {
        return Ptr32::NULL;
    }
    let slash = strchr(m, copy, b'/');
    if slash != 0 {
        m.set_u8(slash, 0);
    }
    let mut found = Ptr32::NULL;
    let mut i = 0;
    while i < m[n_at] {
        let g = m[groups_at].add(i);
        if strcmp(m, m[g].name.addr(), copy) == 0 {
            found = g;
            break;
        }
        i += 1;
    }
    if found.is_null() {
        return Ptr32::NULL;
    }
    if slash != 0 {
        found = get_att_sub_grp_by_path(m, found, Ptr32::new(slash + 1));
    }
    m.free(copy);
    found
}

/// [soft 0x08120430 SI_attdictGetAttGrpByPath]: the group a path of names separated by
/// `/` leads to, from the top-level groups; null for a null argument and when a part
/// names nothing (the copy of the path is then left allocated).
pub fn get_att_grp_by_path(m: &mut Mem, atd: Ptr32<AttributeDict>, path: CStr32) -> Ptr32<AttribGroup> {
    if atd.is_null() || path.is_null() {
        return Ptr32::NULL;
    }
    by_path(m, field!(atd, AttributeDict, nb_groups), field!(atd, AttributeDict, groups), path)
}

/// [soft 0x081204f4 SI_attdictGetAttSubGrpByPath]: the same from a group's sub-groups.
pub fn get_att_sub_grp_by_path(m: &mut Mem, g: Ptr32<AttribGroup>, path: CStr32) -> Ptr32<AttribGroup> {
    if g.is_null() || path.is_null() {
        return Ptr32::NULL;
    }
    by_path(m, field!(g, AttribGroup, nb_subs), field!(g, AttribGroup, subs), path)
}

/// [soft 0x081205b4 SI_attdictAttGrpGetNbAnimatableAttribs]: how many attributes of a
/// group have the animate bit (4) in their access word; 0 for a null group.
pub fn att_grp_get_nb_animatable_attribs(m: &Mem, atd: Ptr32<AttributeDict>, g: Ptr32<AttribGroup>) -> i32 {
    if g.is_null() {
        return 0;
    }
    let mut count = 0;
    let mut i = 0;
    while i < m[g].nb_attribs {
        let a = m[atd].attributes.add(m[m[g].attribs.add(i)]);
        if m.u8(field!(a, Attribute, access).addr()) & 4 != 0 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// [soft 0x081205fc SI_attdictAttributeIsAnimatable]: the animate bit of an attribute's
/// access word, not normalised (0 or 4).
pub fn attribute_is_animatable(m: &Mem, a: Ptr32<Attribute>) -> u16 {
    m[a].access as u16 & 4
}

/// [soft 0x08120610 SI_attdictCompareGrpIndex]: 1 when two paths have the same depth, the
/// same component and the same `depth` entries; 0 otherwise and for a null pointer. A
/// depth above 7 compares beyond the path.
pub fn compare_grp_index(m: &Mem, a: Ptr32<AttGrpIndex>, b: Ptr32<AttGrpIndex>) -> i32 {
    if a.is_null() || b.is_null() || m[a].depth != m[b].depth || m[a].value_index != m[b].value_index {
        return 0;
    }
    let mut i = 0;
    while i < m[a].depth as i32 {
        if short_at(m, a, 2 + i * 2) != short_at(m, b, 2 + i * 2) {
            return 0;
        }
        i += 1;
    }
    1
}
