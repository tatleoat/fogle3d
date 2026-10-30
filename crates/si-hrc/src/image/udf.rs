// SPDX-License-Identifier: MIT
//! Shader references (`__SI_UDF`) on the image: the routines of `udf_fct.c` that the
//! readers need to make, copy and free one.
//!
//! Every reference alive is also listed in the array `si_udfList` (of `si_numUdf` slots,
//! grown by one when none is free); a reference that is not in the list is not freed.

use super::globals::{SI_NUM_UDF, SI_UDF_LIST, S_DONE};
use super::outside::{self, Outside};
use si_mem::{record, Mem, Ptr32};

record! {
    /// A reference to a shader (`__SI_UDF`): the name and the three numbers a file stores,
    /// and what the custom effects hang on it once the shader is loaded. Offsets read from
    /// `SI_udfInitializeUdf` and `SI_udfDispose`; the meaning of the fields marked `x` was
    /// not determined.
    pub struct Udf = "__SI_UDF", size 0x3c {
        0x00 name: Ptr32<u8>, "the shader's name; a block of its own";
        0x04 x04: u32, "copied with the reference";
        0x08 x08: u32, "copied with the reference";
        0x0c active: i16, "file token 2";
        0x0e x0e: i16, "copied with the reference";
        0x10 internal: i16, "file token 3; a reference with 1 here is not copied";
        0x12 pad12: u16, "never written";
        0x14 order: i32, "file token 4";
        0x18 x18: u32, "copied with the reference";
        0x1c x1c: u32, "not copied";
        0x20 str20: Ptr32<u8>, "a string, a block of its own";
        0x24 shader: u32, "the loaded shader (`SI_CustomShader`), or null";
        0x28 str28: Ptr32<u8>, "a string, a block of its own";
        0x2c values: u32, "a list of parameter values (`custom_value_struct`, linked at +0x44)";
        0x30 values2: u32, "a second such list";
        0x34 str34: Ptr32<u8>, "a string, a block of its own";
        0x38 str38: Ptr32<u8>, "a string, a block of its own";
    }
}

/// [soft 0x0854c2f4 SI_udfAllocate]: a zeroed reference in the first free slot of the
/// list. The list is made with one slot on first use and grown by one slot (`SI_realloc`)
/// when none is free; each time the list moves, the custom effects are told
/// (`SI_udfListPointerUpdate`).
pub fn udf_allocate(m: &mut Mem, o: &mut dyn Outside) -> Ptr32<Udf> {
    if m[SI_NUM_UDF] == 0 {
        let list = m.si_calloc(1, 4);
        m[SI_UDF_LIST] = list;
        if list == 0 {
            return Ptr32::NULL;
        }
        m[SI_NUM_UDF] = 1;
        outside::udf_list_pointer_update(m, o);
    }
    let mut i: i32 = 0;
    if 0 < m[SI_NUM_UDF] {
        let mut p = m.u32(m[SI_UDF_LIST]);
        while p != 0 {
            i += 1;
            if !(i < m[SI_NUM_UDF]) {
                break;
            }
            p = m.u32(m[SI_UDF_LIST].wrapping_add(4 * i as u32));
        }
    }
    if i == m[SI_NUM_UDF] {
        let list = m.si_realloc(m[SI_UDF_LIST], (i * 4 + 4) as u32);
        m[SI_UDF_LIST] = list;
        if list == 0 {
            return Ptr32::NULL;
        }
        let n = m[SI_NUM_UDF];
        m.set_u32(list.wrapping_add(4 * n as u32), 0);
        m[SI_NUM_UDF] = n + 1;
        outside::udf_list_pointer_update(m, o);
    }
    let udf: Ptr32<Udf> = Ptr32::new(m.si_calloc(1, 0x3c));
    let slot = m[SI_UDF_LIST].wrapping_add(4 * i as u32);
    m.set_u32(slot, udf.addr());
    if !udf.is_null() {
        m[udf].active = 0;
    }
    udf
}

/// [soft 0x0854c748 SI_udfIsActive]
pub fn udf_is_active(m: &Mem, udf: Ptr32<Udf>) -> i32 {
    if udf.is_null() {
        0
    } else {
        m[udf].active as i32
    }
}

/// [soft 0x0854c790 SI_udfSetActive]: stores the flag; the loaded shader is told when the
/// flag of a reference that is not internal changes.
pub fn udf_set_active(m: &mut Mem, o: &mut dyn Outside, udf: Ptr32<Udf>, active: i16) {
    if !udf.is_null() {
        if m[udf].internal == 0 && m[udf].active != active {
            let shader = m[udf].shader;
            outside::shader_set_active(m, o, shader, active);
        }
        m[udf].active = active;
    }
}

/// [soft 0x0854c5e4 SI_udfInitializeUdf]: makes `dst` a copy of `src`: the five strings
/// duplicated, the loaded shader and the two lists of values copied by their own routines,
/// the numbers taken over. The internal flag (+0x10) and the word at +0x1c are not copied.
/// Answers 0, and does nothing, for a null reference or an internal source.
pub fn udf_initialize_udf(m: &mut Mem, o: &mut dyn Outside, dst: Ptr32<Udf>, src: Ptr32<Udf>) -> i32 {
    if src.is_null() || dst.is_null() || m[src].internal == 1 {
        return 0;
    }
    if !m[src].name.is_null() {
        let s = m.strdup(m[src].name.addr());
        m[dst].name = Ptr32::new(s);
    }
    if !m[src].str20.is_null() {
        let s = m.strdup(m[src].str20.addr());
        m[dst].str20 = Ptr32::new(s);
    }
    m[dst].x18 = m[src].x18;
    m[dst].order = m[src].order;
    m[dst].x08 = m[src].x08;
    if !m[src].str28.is_null() {
        let s = m.strdup(m[src].str28.addr());
        m[dst].str28 = Ptr32::new(s);
    }
    if m[src].shader != 0 {
        let shader = m[src].shader;
        m[dst].shader = outside::shader_copy(m, o, shader);
    }
    let values = m[src].values;
    m[dst].values = outside::custom_value_list_copy(m, o, values);
    let values2 = m[src].values2;
    m[dst].values2 = outside::custom_value_list_copy(m, o, values2);
    outside::shader_fcurve_update_owners(m, o, dst.addr());
    if !m[src].str34.is_null() {
        let s = m.strdup(m[src].str34.addr());
        m[dst].str34 = Ptr32::new(s);
    }
    if !m[src].str38.is_null() {
        let s = m.strdup(m[src].str38.addr());
        m[dst].str38 = Ptr32::new(s);
    }
    let active = udf_is_active(m, src) as i16;
    udf_set_active(m, o, dst, active);
    m[dst].x0e = m[src].x0e;
    m[dst].x04 = m[src].x04;
    outside::cus_trace(m, o, S_DONE);
    1
}

/// [soft 0x0854c70c SI_udfCopy]: a new reference that is a copy of `udf`; null for null and
/// for an internal one.
pub fn udf_copy(m: &mut Mem, o: &mut dyn Outside, udf: Ptr32<Udf>) -> Ptr32<Udf> {
    if udf.is_null() || m[udf].internal == 1 {
        return Ptr32::NULL;
    }
    let new = udf_allocate(m, o);
    if !new.is_null() {
        udf_initialize_udf(m, o, new, udf);
    }
    new
}

/// [soft 0x0854c7d8 SI_udfSetName]: the reference's name (when it has one) is freed and a
/// `strdup` of `name` takes its place; 1 goes to +0xe (the flag `SI_edAddUDF` hands to
/// `SI_elemdictSetDirty`). Answers 1. The new name is copied after the old one is freed:
/// a `name` that is the old name is read from the freed block, as in the original.
pub fn udf_set_name(m: &mut Mem, udf: Ptr32<Udf>, name: u32) -> i32 {
    let old = m[udf].name;
    if !old.is_null() {
        m.free(old.addr());
    }
    let copy = m.strdup(name);
    m[udf].name = Ptr32::new(copy);
    m[udf].x0e = 1;
    1
}

/// [soft 0x0854c3c0 SI_udfDispose]: takes the reference out of the list and frees it with
/// its strings; the loaded shader, the curves in the editor and the parameter values go
/// through their own routines. A reference the list does not hold is left alone (answer
/// 0); null answers 1.
pub fn udf_dispose(m: &mut Mem, o: &mut dyn Outside, udf: Ptr32<Udf>) -> i32 {
    if !udf.is_null() {
        let mut i: i32 = 0;
        if 0 < m[SI_NUM_UDF] {
            let mut p = m.u32(m[SI_UDF_LIST]);
            while udf.addr() != p {
                i += 1;
                if !(i < m[SI_NUM_UDF]) {
                    break;
                }
                p = m.u32(m[SI_UDF_LIST].wrapping_add(4 * i as u32));
            }
        }
        if i == m[SI_NUM_UDF] {
            return 0;
        }
        let slot = m[SI_UDF_LIST].wrapping_add(4 * i as u32);
        m.set_u32(slot, 0);
        if !m[udf].name.is_null() {
            m.free(m[udf].name.addr());
            m[udf].name = Ptr32::NULL;
        }
        m[udf].name = Ptr32::NULL;
        if !m[udf].str20.is_null() {
            m.free(m[udf].str20.addr());
            m[udf].str20 = Ptr32::NULL;
        }
        m[udf].str20 = Ptr32::NULL;
        udf_set_active(m, o, udf, 0);
        if !m[udf].str28.is_null() {
            m.free(m[udf].str28.addr());
            m[udf].str28 = Ptr32::NULL;
        }
        if m[udf].shader != 0 {
            let shader = m[udf].shader;
            outside::shader_dispose(m, o, shader);
        }
        outside::shader_fcurve_remove_from_editor(m, o, udf.addr());
        // the two lists of values, one after the other; a value's link is read before
        // the value is handed over
        let mut v = m[udf].values;
        while v != 0 {
            let next = m.u32(v.wrapping_add(0x44));
            outside::custom_value_dispose(m, o, v);
            v = next;
        }
        let mut v = m[udf].values2;
        while v != 0 {
            let next = m.u32(v.wrapping_add(0x44));
            outside::custom_value_dispose(m, o, v);
            v = next;
        }
        m[udf].values = 0;
        m[udf].values2 = 0;
        if !m[udf].str34.is_null() {
            m.free(m[udf].str34.addr());
            m[udf].str34 = Ptr32::NULL;
        }
        if !m[udf].str38.is_null() {
            m.free(m[udf].str38.addr());
            m[udf].str38 = Ptr32::NULL;
        }
        m.free(udf.addr());
    }
    1
}
