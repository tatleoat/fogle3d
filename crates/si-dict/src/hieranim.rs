// SPDX-License-Identifier: MIT
//! `hierani.c`: the hierarchy-animation record (`SI_HierAnim`, 0x1c bytes), the part not
//! yet on the image elsewhere: disposal and the two look-ups.
//!
//! The record: +0x00 the number of elements, +0x04 their array (0x28 bytes each), +0x08
//! the chapter, +0x0c and +0x14 two action lists (8 bytes each). An element: +0x00 its
//! name (a block of its own), +0x04 its father's index, +0x08 the number of items, and
//! six arrays of one entry per item: +0x0c types (16-bit, 0 a function curve, 1 a static
//! value), +0x10 action relation ids, +0x14 occurrence ids, +0x18 data (the curve, or for
//! a static value a block of its own), +0x1c ones, +0x20 flags. `si-elemio` has the same
//! offsets under `image::off`, with the constructors; this module keeps its own so that
//! `si-dict` stays free of that crate.

use crate::outside::{si_actions_dispose, si_hieranim_init, Outside};
use si_mem::Mem;

const NB_ELEMS: u32 = 0;
const ELEMS: u32 = 4;
const CHAPTER: u32 = 8;
const ACTIONS: u32 = 0xc;
const ACTIONS2: u32 = 0x14;
const ELEM_SIZE: u32 = 0x28;

/// [soft 0x08138b5c SI_hieranimDispose]: frees what the record owns and leaves it empty.
/// Per element: the data block of every item of type 1 (a static value; zeroed), then the
/// arrays data (+0x18), types (+0x0c), ones (+0x1c), flags (+0x20), relation ids (+0x10),
/// occurrence ids (+0x14) and the name, each zeroed after its `free`; then the element
/// array; then both action lists are disposed (`SI_actionsDispose`) and the record is
/// initialised again for the same chapter (`SI_hieranimInit`).
pub fn dispose(m: &mut Mem, o: &mut dyn Outside, anim: u32) {
    let mut k = 0i32;
    while k < m.i32(anim.wrapping_add(NB_ELEMS)) {
        let e = m.u32(anim.wrapping_add(ELEMS)).wrapping_add((k as u32).wrapping_mul(ELEM_SIZE));
        k += 1;
        let nb_items = m.i32(e + 8);
        let mut i = 0i32;
        while i < nb_items {
            if m.i16(m.u32(e + 0xc).wrapping_add((i as u32).wrapping_mul(2))) == 1 {
                let slot = m.u32(e + 0x18).wrapping_add((i as u32).wrapping_mul(4));
                let p = m.u32(slot);
                if p != 0 {
                    m.free(p);
                    m.set_u32(slot, 0);
                }
            }
            i += 1;
        }
        for off in [0x18u32, 0x0c, 0x1c, 0x20, 0x10, 0x14, 0x00] {
            let p = m.u32(e + off);
            if p != 0 {
                m.free(p);
                m.set_u32(e + off, 0);
            }
        }
    }
    let elems = m.u32(anim + ELEMS);
    if elems != 0 {
        m.free(elems);
        m.set_u32(anim + ELEMS, 0);
    }
    si_actions_dispose(o, m, anim + ACTIONS);
    si_actions_dispose(o, m, anim + ACTIONS2);
    let chapter = m.i32(anim + CHAPTER);
    si_hieranim_init(o, m, anim, chapter);
}

/// [soft 0x08138dbc SI_hieranimGetName]: the name of element `index`; null outside the list.
pub fn get_name(m: &mut Mem, anim: u32, index: i32) -> u32 {
    if index < 0 || m.i32(anim) <= index {
        0
    } else {
        m.u32(m.u32(anim + ELEMS).wrapping_add((index as u32).wrapping_mul(ELEM_SIZE)))
    }
}

/// [soft 0x08138de0 SI_hieranimGetType]: for item `index` of an element, the first word of
/// the item's data when its type is 0 or 1. For any other type the original leaves its
/// result register as the caller had `ebx` (the compiler never set it); that value is
/// `caller_ebx`, which the callers of this port have no way to know.
pub fn get_type(m: &mut Mem, elem: u32, index: i32, caller_ebx: u32) -> u32 {
    let t = m.i16(m.u32(elem + 0xc).wrapping_add((index as u32).wrapping_mul(2)));
    if t == 0 || t == 1 {
        m.u32(m.u32(m.u32(elem + 0x18).wrapping_add((index as u32).wrapping_mul(4))))
    } else {
        caller_ebx
    }
}
