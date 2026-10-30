// SPDX-License-Identifier: MIT
//! `enmdict.c`: the dictionary of enumerated values (`SI_EVD`), on the image.
//!
//! A dictionary is an array of types (12 bytes each: count, values, name); a type an array
//! of values (12 bytes: number, code, name). Both arrays grow by one record per addition:
//! `SI_malloc(12)` for the first, `SI_realloc(array, (n + 1) * 12)` after.

use crate::libc::strcmp;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{EnumVal, EnumValDict, EnumValType};

/// [soft 0x08138520 si_enmdictAddValue]: appends a value to a type. The two strings are
/// stored as given (the caller's pointers, not copies).
pub fn add_value(m: &mut Mem, ty: Ptr32<EnumValType>, id: i32, code: CStr32, name: CStr32) {
    let n = m[ty].nb_values;
    let values = m[ty].values;
    let p = if values.is_null() { m.si_malloc(0xc) } else { m.si_realloc(values.addr(), ((n * 3 + 3) * 4) as u32) };
    m[ty].values = Ptr32::new(p);
    let v = m[ty].values.add(n);
    m[v].id = id;
    m[v].code = code;
    m[v].name = name;
    m[ty].nb_values += 1;
}

/// [soft 0x08138588 si_enmdictAddEnumType]: appends an empty type and answers it.
pub fn add_enum_type(m: &mut Mem, dict: Ptr32<EnumValDict>, name: CStr32) -> Ptr32<EnumValType> {
    let n = m[dict].nb_types;
    let types = m[dict].types;
    let p = if types.is_null() { m.si_malloc(0xc) } else { m.si_realloc(types.addr(), ((n * 3 + 3) * 4) as u32) };
    m[dict].types = Ptr32::new(p);
    let t = m[dict].types.add(n);
    m[t].name = name;
    m[t].nb_values = 0;
    m[t].values = Ptr32::NULL;
    m[dict].nb_types += 1;
    t
}

/// [soft 0x081385f4 SI_enmdictInit]
pub fn init(m: &mut Mem, dict: Ptr32<EnumValDict>) {
    m[dict].nb_types = 0;
    m[dict].types = Ptr32::NULL;
}

/// [soft 0x0813860c SI_enmdictDispose]: frees every value's two strings, every type's
/// array and name, and the array of types; each pointer is cleared after its `free`.
pub fn dispose(m: &mut Mem, dict: Ptr32<EnumValDict>) {
    let mut i = 0;
    while i < m[dict].nb_types {
        let t = m[dict].types.add(i);
        let mut j = 0;
        while j < m[t].nb_values {
            let v = m[t].values.add(j);
            if !m[v].code.is_null() {
                let p = m[v].code.addr();
                m.free(p);
                m[v].code = Ptr32::NULL;
            }
            if !m[v].name.is_null() {
                let p = m[v].name.addr();
                m.free(p);
                m[v].name = Ptr32::NULL;
            }
            j += 1;
        }
        if !m[t].values.is_null() {
            let p = m[t].values.addr();
            m.free(p);
            m[t].values = Ptr32::NULL;
        }
        if !m[t].name.is_null() {
            let p = m[t].name.addr();
            m.free(p);
            m[t].name = Ptr32::NULL;
        }
        i += 1;
    }
    if !m[dict].types.is_null() {
        let p = m[dict].types.addr();
        m.free(p);
        m[dict].types = Ptr32::NULL;
    }
    init(m, dict);
}

/// [soft 0x081387c0 SI_enmdictEnumValGetByCode]: the value of type `ty` whose code is
/// `code`, or null. A type index outside the dictionary answers null.
pub fn enum_val_get_by_code(m: &Mem, dict: Ptr32<EnumValDict>, ty: i32, code: CStr32) -> Ptr32<EnumVal> {
    if -1 < ty && ty < m[dict].nb_types {
        let t = m[dict].types.add(ty);
        let mut j = 0;
        while j < m[t].nb_values {
            let v = m[t].values.add(j);
            if strcmp(m, m[v].code.addr(), code.addr()) == 0 {
                return v;
            }
            j += 1;
        }
    }
    Ptr32::NULL
}

/// [soft 0x08138834 SI_enmdictEnumCodeGetByValue]: the code of the value numbered `id` in
/// type `ty`, or null.
pub fn enum_code_get_by_value(m: &Mem, dict: Ptr32<EnumValDict>, ty: i32, id: i32) -> CStr32 {
    if -1 < ty && ty < m[dict].nb_types {
        let t = m[dict].types.add(ty);
        let n = m[t].nb_values;
        let values = m[t].values;
        let mut j = 0;
        while j < n {
            if m[values.add(j)].id == id {
                return m[values.add(j)].code;
            }
            j += 1;
        }
    }
    Ptr32::NULL
}

/// [soft 0x08138888 SI_enmdictEnumTypeGetName]: the index of the type called `name`, or −1.
pub fn enum_type_get_name(m: &Mem, dict: Ptr32<EnumValDict>, name: CStr32) -> i32 {
    let mut i = 0;
    while i < m[dict].nb_types {
        let t = m[dict].types.add(i);
        if strcmp(m, m[t].name.addr(), name.addr()) == 0 {
            return i;
        }
        i += 1;
    }
    -1
}

/// [soft 0x081388d8 SI_enmdictEnumValGetByAnyCode]: looks for `code` in every type after
/// `*index` (−1 to start at the first). On a hit `*index` is the type and the value is
/// answered. Without one null is answered and `*index` stays one past where the search
/// started (or untouched, when it was already past the end or `code` is null).
pub fn enum_val_get_by_any_code(m: &mut Mem, dict: Ptr32<EnumValDict>, index: Ptr32<i32>, code: CStr32) -> Ptr32<EnumVal> {
    let mut i = m[index];
    if i < m[dict].nb_types && !code.is_null() {
        if i == -1 {
            m[index] = 0;
        } else {
            m[index] = i + 1;
        }
        i = m[index];
        while i < m[dict].nb_types {
            let t = m[dict].types.add(i);
            let mut j = 0;
            while j < m[t].nb_values {
                let v = m[t].values.add(j);
                if strcmp(m, m[v].code.addr(), code.addr()) == 0 {
                    m[index] = i;
                    return v;
                }
                j += 1;
            }
            i += 1;
        }
    }
    Ptr32::NULL
}

/// [soft 0x081389a4 SI_enmdictEnumValGetNameById]: the long name of the value numbered
/// `id` in type `ty`, or null; a null dictionary answers null.
pub fn enum_val_get_name_by_id(m: &Mem, dict: Ptr32<EnumValDict>, ty: i32, id: i32) -> CStr32 {
    if -1 < ty && !dict.is_null() && ty < m[dict].nb_types {
        let t = m[dict].types.add(ty);
        let n = m[t].nb_values;
        let values = m[t].values;
        let mut j = 0;
        while j < n {
            if m[values.add(j)].id == id {
                return m[values.add(j)].name;
            }
            j += 1;
        }
    }
    Ptr32::NULL
}
