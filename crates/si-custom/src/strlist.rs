// SPDX-License-Identifier: MIT
//! `SI_strListAppend` (the string-list helper the shader database and the shader-file writers
//! use; it lies in the C library of the program's own `si_str` sources).

use si_mem::{clib, Mem};

/// [soft 0x083cf668 SI_strListAppend]: appends a copy of `text` (null stays null) to the array of
/// strings `*list`, whose length is `*count`: a new array one longer, the old entries moved, the
/// old array freed. Answers 1; 0 for a null `list` or `count`, a negative count, or no memory.
pub fn si_str_list_append(m: &mut Mem, text: u32, list: u32, count: u32) -> i32 {
    if list == 0 || count == 0 || m.i32(count) < 0 {
        return 0;
    }
    let copy = if text == 0 {
        0
    } else {
        let c = clib::strdup(m, text);
        if c == 0 {
            return 0;
        }
        c
    };
    let n = m.u32(count);
    let dest = m.si_calloc(n.wrapping_add(1), 4);
    if dest == 0 {
        return 0;
    }
    let old = m.u32(list);
    if old != 0 {
        clib::memmove(m, dest, old, n.wrapping_shl(2));
        m.free(old);
        m.set_u32(list, 0);
    }
    m.set_u32(dest.wrapping_add(n.wrapping_shl(2)), copy);
    m.set_u32(list, dest);
    m.set_u32(count, n.wrapping_add(1));
    1
}

/// [soft 0x083cf280 SI_strAppend]: appends `text` to the string `*dest` (a new block of both lengths
/// plus one from `SI_malloc`, the old one freed); a null `*dest` takes a copy. Answers 1; 0 for a
/// null argument or no memory.
pub fn si_str_append(m: &mut Mem, dest: u32, text: u32) -> i32 {
    if text == 0 || dest == 0 {
        return 0;
    }
    let old = m.u32(dest);
    if old == 0 {
        let c = clib::strdup(m, text);
        m.set_u32(dest, c);
    } else {
        let n = clib::strlen(m, old).wrapping_add(clib::strlen(m, text)).wrapping_add(1);
        let buf = m.si_malloc(n);
        if buf == 0 {
            return 0;
        }
        clib::strcpy(m, buf, old);
        clib::strcat(m, buf, text);
        let cur = m.u32(dest);
        if cur != 0 {
            m.free(cur);
            m.set_u32(dest, 0);
        }
        m.set_u32(dest, buf);
    }
    (m.u32(dest) != 0) as i32
}
