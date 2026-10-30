// SPDX-License-Identifier: MIT
//! The non-dialog helpers of `express_dial.c` on the image (finding 0013, section 18): whether an
//! expression's left-hand side is a custom parameter, and the scan for where a name starts.
//! The dialog itself (`SI_expressDialogBox` and the items it sets) stays unported.

use super::calls::ExprCalls;
use super::rec;
use si_mem::{Mem, Outside, Ptr32};

/// `exclusionSet.264` of `express_dial.c`, the characters that end a name: `+*/,()!=<> `.
pub const EXCLUSION_SET: u32 = 0x0869b2da;

/// Whether the expression's left-hand side is a custom parameter [soft 0x082522a4 lhsIsCustom]:
/// all of `*custom`, `*fcv` and the expression are non-null, the expression has a root, the
/// root a left child, that child a leaf record whose kind is 0x17 (a field leaf); the leaf's
/// data names a chapter (+8) and an element (+0xc) of the current dictionary
/// (`SI_elemdictGetElem`); with that element, `*custom` is the custom value
/// `SI_cusFetchCustomValue(element's data, the name at +0x20, fcv)` and the answer 1. Else 0
/// (the outputs untouched).
pub fn lhs_is_custom(m: &mut Mem, o: &mut dyn Outside, expr: u32, custom: u32, fcv: u32) -> u32 {
    if custom == 0 || fcv == 0 || expr == 0 {
        return 0;
    }
    let root = m.u32(expr);
    if root == 0 {
        return 0;
    }
    let left = m.u32(root + 4);
    if left == 0 {
        return 0;
    }
    let nop = m.u32(left + 4);
    if nop == 0 || m.u32(nop) != 0x17 {
        return 0;
    }
    let data = m.u32(nop + 4);
    let ed = rec::cur_dict(m);
    let e = si_dict::elemdict::get_elem(m, ed, m.i32(data + 8), m.i32(data + 0xc));
    if e.is_null() {
        return 0;
    }
    let model = m.u32(e.addr() + 4);
    let name = m.u32(data + 0x20);
    let v = o.cus_fetch_custom_value(m, Ptr32::new(model), Ptr32::new(name), Ptr32::new(fcv));
    m.set_u32(custom, v);
    1
}

/// Where the name that ends at `from` starts, scanning backward [soft 0x08252334 checkStartName]:
/// from `from` down while `limit` is lower, stop at the first character that is one of the
/// exclusion set's (`+*/,()!=<> `, read from the image; an empty set never stops). Found: the
/// address after it, or `from` itself when it is `from`'s own character; not found, `limit`.
/// The loop body runs once even when it ends at the first step.
pub fn check_start_name(m: &Mem, from: u32, limit: u32) -> u32 {
    let set = m.cstr(EXCLUSION_SET);
    let mut p = from;
    let mut found = false;
    if limit < from {
        loop {
            if !set.is_empty() && set.contains(&m.u8(p)) {
                found = true;
            } else {
                p = p.wrapping_sub(1);
            }
            if !(limit < p && !found) {
                break;
            }
        }
    }
    if found {
        if p != from {
            p + 1
        } else {
            p
        }
    } else {
        limit
    }
}
