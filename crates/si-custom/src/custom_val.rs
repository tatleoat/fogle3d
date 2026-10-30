// SPDX-License-Identifier: MIT
//! The custom value lists of `custom_dialog.c` (finding 0041, "eleventh continuation", unit customrt1):
//! allocation, copy and disposal of the 0x50-byte values, the keyed-value walkers, `findCustomValue`
//! and the two `SI_udfGenerate*` wrappers. A value: name +0, type (short) +4, id +8, float +0x10,
//! text +0x18, up to four fcurve cells +0x2c, next +0x44.

use crate::custom_util::SI_FCURVE_ALLOCATE;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_FCURVE_TRANSFER: Routine = Routine::new("SI_fcurveTransfer", 0x081f_f0e4);
pub const SI_SHADER_GENERATE_CCOMP: Routine = Routine::new("SI_shaderGenerateCcomp", 0x0854_30cc);
pub const SI_SHADER_GENERATE_CFLAGS: Routine = Routine::new("SI_shaderGenerateCflags", 0x0854_2e64);

/// [soft 0x08526290 si_customValueAllocate]: a zeroed 0x50-byte block (`SI_calloc(1, 0x50)`).
pub fn si_custom_value_allocate(m: &mut Mem) -> u32 {
    m.si_calloc(1, 0x50)
}

/// [soft 0x08526300 si_customValueInitializeCustVal]: frees the name and text of `dst`, then (both non-null)
/// copies the 0x50 bytes of `src` over it, clears the link, duplicates the name and the text and gives the
/// value copies of its fcurves (four for type 4, one for the others). Always 1.
pub fn si_custom_value_initialize_cust_val(m: &mut Mem, o: &mut dyn Outside, dst: u32, src: u32) -> i32 {
    if dst != 0 {
        let n = m.u32(dst);
        if n != 0 {
            m.free(n);
            m.set_u32(dst, 0);
        }
        let t = m.u32(dst.wrapping_add(0x18));
        if t != 0 {
            m.free(t);
            m.set_u32(dst.wrapping_add(0x18), 0);
        }
    }
    if src != 0 && dst != 0 {
        for k in 0..0x14u32 {
            let w = m.u32(src.wrapping_add(4 * k));
            m.set_u32(dst.wrapping_add(4 * k), w);
        }
        m.set_u32(dst.wrapping_add(0x44), 0);
        m.set_u32(dst, 0);
        m.set_u32(dst.wrapping_add(0x18), 0);
        let n = m.u32(src);
        if n != 0 {
            let d = clib::strdup(m, n);
            m.set_u32(dst, d);
        }
        let t = m.u32(src.wrapping_add(0x18));
        if t != 0 {
            let d = clib::strdup(m, t);
            m.set_u32(dst.wrapping_add(0x18), d);
        }
        for off in [0x38u32, 0x34, 0x30, 0x2c] {
            m.set_u32(dst.wrapping_add(off), 0);
        }
        let count = if m.u16(src.wrapping_add(4)) != 4 { 1 } else { 4 };
        for k in 0..count {
            let off = 0x2c + 4 * k;
            let fc = m.u32(src.wrapping_add(off));
            if fc != 0 {
                let nf = o.call(m, SI_FCURVE_ALLOCATE, &[0, dst]).eax;
                m.set_u32(dst.wrapping_add(off), nf);
                if nf != 0 {
                    o.call(m, SI_FCURVE_TRANSFER, &[fc, nf]);
                }
            }
        }
    }
    1
}

/// [soft 0x08526454 si_customValueCopy]: a new value initialised from `src`; 0 when out of memory.
pub fn si_custom_value_copy(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    let p = si_custom_value_allocate(m);
    if p == 0 {
        return 0;
    }
    si_custom_value_initialize_cust_val(m, o, p, src);
    p
}

/// [soft 0x08526488 si_customValueListCopy]: copies the list value by value; stops at the first value that
/// cannot be allocated (the copies so far are returned); 0 for a null list.
pub fn si_custom_value_list_copy(m: &mut Mem, o: &mut dyn Outside, list: u32) -> u32 {
    let mut head = 0u32;
    if list == 0 {
        return 0;
    }
    let mut slot: Option<u32> = None; // None: the head; Some(a): the cell at a
    let mut src = list;
    loop {
        let c = si_custom_value_copy(m, o, src);
        if c == 0 {
            return head;
        }
        match slot {
            None => head = c,
            Some(a) => m.set_u32(a, c),
        }
        src = m.u32(src.wrapping_add(0x44));
        slot = Some(c.wrapping_add(0x44));
        if src == 0 {
            return head;
        }
    }
}

/// [soft 0x085262a8 si_customValueDispose]: frees the name, the text and the value itself (the fcurves are
/// left alone). Always 1.
pub fn si_custom_value_dispose(m: &mut Mem, v: u32) -> i32 {
    if v != 0 {
        let n = m.u32(v);
        if n != 0 {
            m.free(n);
            m.set_u32(v, 0);
        }
        let t = m.u32(v.wrapping_add(0x18));
        if t != 0 {
            m.free(t);
            m.set_u32(v.wrapping_add(0x18), 0);
        }
        m.free(v);
    }
    1
}

/// [soft 0x085264d4 si_customValueListDispose]: disposes every value of the list.
pub fn si_custom_value_list_dispose(m: &mut Mem, list: u32) {
    let mut p = list;
    while p != 0 {
        let next = m.u32(p.wrapping_add(0x44));
        si_custom_value_dispose(m, p);
        p = next;
    }
}

fn keyed_cell(m: &Mem, v: u32, off: u32) -> bool {
    let fc = m.u32(v.wrapping_add(off));
    fc != 0 && (m.u32(fc.wrapping_add(0xc)) as i32) > 0
}

/// [soft 0x08527ac8 cusvalIsKeyedValue]: 1 when the value (not null) has an fcurve with keys: the first cell
/// for types 1 and 4, any of the other three cells.
pub fn cusval_is_keyed_value(m: &mut Mem, v: u32) -> i32 {
    if v == 0 {
        return 0;
    }
    let t = m.u16(v.wrapping_add(4));
    let hit = ((t == 1 || t == 4) && keyed_cell(m, v, 0x2c)) || keyed_cell(m, v, 0x30) || keyed_cell(m, v, 0x34) || keyed_cell(m, v, 0x38);
    hit as i32
}

/// The scroll test of the two walkers: the entry `id * 0x28` bytes into the scroll array has its short at
/// -4 equal to 1 and its short at -0x14 equal to 0.
fn scroll_ok(m: &Mem, scroll: u32, id: u32) -> bool {
    let e = scroll.wrapping_add(id.wrapping_mul(0x28));
    m.u16(e.wrapping_sub(4)) == 1 && m.u16(e.wrapping_sub(0x14)) == 0
}

/// [soft 0x08527a64 cusvalGetNextKeyedValue]: the next value after `v` that has keys (and, with a scroll
/// array, whose entry passes the scroll test); 0 when none.
pub fn cusval_get_next_keyed_value(m: &mut Mem, v: u32, scroll: u32) -> u32 {
    let mut p = v;
    if p == 0 {
        return 0;
    }
    loop {
        p = m.u32(p.wrapping_add(0x44));
        if p == 0 {
            return 0;
        }
        if cusval_is_keyed_value(m, p) as u16 != 0 {
            if scroll == 0 {
                return p;
            }
            let id = m.u32(p.wrapping_add(8));
            if scroll_ok(m, scroll, id) {
                return p;
            }
        }
    }
}

/// [soft 0x08527a10 cusvalGetNextKeyableValue]: the next value after `v` of type 1 or 4 (and passing the
/// scroll test when a scroll array is given); 0 when none.
pub fn cusval_get_next_keyable_value(m: &mut Mem, v: u32, scroll: u32) -> u32 {
    let mut p = v;
    if p == 0 {
        return 0;
    }
    loop {
        p = m.u32(p.wrapping_add(0x44));
        if p == 0 {
            return 0;
        }
        let t = m.u16(p.wrapping_add(4));
        if t == 1 || t == 4 {
            if scroll == 0 {
                return p;
            }
            let id = m.u32(p.wrapping_add(8));
            if scroll_ok(m, scroll, id) {
                return p;
            }
        }
    }
}

/// [soft 0x08527020 findCustomValue]: the value of the dialog's list (+0x54) whose id (+8) is `id`; 0 when none.
pub fn find_custom_value(m: &mut Mem, dlg: u32, id: i32) -> u32 {
    if dlg == 0 {
        return 0;
    }
    let mut p = m.u32(dlg.wrapping_add(0x54));
    while p != 0 {
        if m.u32(p.wrapping_add(8)) as i32 == id {
            return p;
        }
        p = m.u32(p.wrapping_add(0x44));
    }
    0
}

/// [soft 0x08546048 SI_udfGenerateCcomp]: `SI_shaderGenerateCcomp(a, b)` (a short, sign-extended).
pub fn si_udf_generate_ccomp(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    o.call(m, SI_SHADER_GENERATE_CCOMP, &[a, b]).eax as u16 as i16 as i32
}

/// [soft 0x08546064 SI_udfGenerateCflags]: `SI_shaderGenerateCflags(a, b)` (a short, sign-extended).
pub fn si_udf_generate_cflags(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32) -> i32 {
    o.call(m, SI_SHADER_GENERATE_CFLAGS, &[a, b]).eax as u16 as i16 as i32
}
