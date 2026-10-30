// SPDX-License-Identifier: MIT
//! The dictionary side of `express_io.c` on the image (finding 0013, section 15): an
//! expression added to the EXPRESSIONS chapter, given up again, and the expressions of an
//! element removed.
//!
//! Callees that are not on the image and so go to the outside world (`calls.rs`):
//! `SI_dbListNodeAllocate`, `SI_dbFormatFullName` (`si-scene` has them but depends on this
//! crate), `SI_expressDataDispose` (the image form is not ported), `SI_edElementSetDeleteFlag`.

use super::calls::{DictOutside, ExprCalls};
use super::rec::{self, cid};
use si_mem::{CStr32, Mem, Outside, Ptr32};
use si_types::dict::{Elem, ElemDict, ElemList};

/// `SI_addInDictionary` (16 bits).
const SI_ADD_IN_DICTIONARY: u32 = 0x086792b0;
/// `SI_edSumatraTo3x` (16 bits).
const SI_ED_SUMATRA_TO_3X: u32 = 0x08680e50;
/// The literal the file's own entry is named with, and the base of a new element name.
const S_EXPRESS_FILE: u32 = 0x0869b940;
const S_NEW_BASE: u32 = 0x0869b94d;
/// `SI_RD`.
const SI_RD: u32 = 0x087815f0;

/// A name record named `name` (copied), its file name made and stored.
fn named_node(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    let node = o.db_list_node_allocate(m);
    let copy = m.strdup(name);
    m.set_u32(node + 0x14, copy);
    let full = o.db_format_full_name(m, node);
    m.set_u32(node + 0x24, full);
    node
}

/// An expression put in the EXPRESSIONS chapter under `name` (a new name when it is null or
/// empty) and marked as read from a file; answers the name [soft 0x08255770 SI_edAddExpression].
///
/// The first expression of a chapter that holds none first gets the chapter's entry for the
/// expression file (`express_file`). Does nothing when `SI_addInDictionary` is 0; the
/// original then returns whatever its `esi` held (the caller's), the port returns 0.
pub fn ed_add_expression(m: &mut Mem, o: &mut dyn Outside, name: u32, expr: u32) -> u32 {
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return 0;
    }
    let ed = rec::cur_dict(m);
    let chap = rec::chap(m, cid::EXP);
    let list = m.u32(ed.addr() + 4).wrapping_add((chap as u32).wrapping_mul(0x40));
    if m.i32(list + 4) == 0 {
        let node = named_node(m, o, S_EXPRESS_FILE);
        si_dict::elemdict::add_elem(m, &mut DictOutside(o), ed, chap, Ptr32::new(node), Ptr32::NULL, Ptr32::NULL, 0, 0);
    }
    let name = if name == 0 || m.u8(name) == 0 {
        si_dict::elemname::get_new_name(m, &mut DictOutside(o), ed, chap, CStr32::new(S_NEW_BASE)).addr()
    } else {
        name
    };
    let node = named_node(m, o, name);
    let index = si_dict::elemdict::add_elem(m, &mut DictOutside(o), ed, chap, Ptr32::new(node), Ptr32::new(expr), Ptr32::NULL, 0, 0);
    let e = si_dict::elemdict::elemlist_get_elem(m, Ptr32::<ElemList>::new(list), index);
    m.set_i32(e.addr() + 0x54, 1);
    name
}

/// The chapter's entry for the expression file given up when no expression read from a file
/// is left in the chapter [soft 0x082558bc edCheckDisposeExprFile]. Not while a scene of the
/// newer program is converted (`SI_edSumatraTo3x`).
pub fn ed_check_dispose_expr_file(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) {
    if m.i16(SI_ED_SUMATRA_TO_3X) != 0 {
        return;
    }
    let chap = rec::chap(m, cid::EXP);
    let list = m.u32(ed.addr() + 4).wrapping_add((chap as u32).wrapping_mul(0x40));
    let mut n = m.i32(list + 4);
    let mut e = m.u32(list + 8);
    let mut found = 0;
    while n > 0 {
        if m.i16(e + 0x48) == 0 && m.u32(e + 4) != 0 && m.i32(e + 0x54) != 0 {
            found += 1;
        }
        e = e.wrapping_add(0x84);
        n -= 1;
    }
    if found == 0 {
        si_dict::elemdict::elemlist_dispose(m, &mut DictOutside(o), Ptr32::<ElemList>::new(list), 0);
    }
}

/// An expression element given up: its trees freed, its pointer cleared, the element flagged
/// deleted (both ways) and the file entry checked; whether the element exists
/// [soft 0x0825591c SI_edDisposeExpression].
pub fn ed_dispose_expression(m: &mut Mem, o: &mut dyn Outside, index: i32) -> bool {
    let ed = rec::cur_dict(m);
    let chap = rec::chap(m, cid::EXP);
    let e = si_dict::elemdict::get_elem(m, ed, chap, index);
    if e.is_null() {
        return false;
    }
    let tree = m.u32(e.addr() + 4);
    o.express_data_dispose(m, tree);
    m.set_u32(e.addr() + 4, 0);
    o.ed_element_set_delete_flag(m, e.addr(), chap, index, 1, 0, 0, 0);
    o.ed_element_set_delete_flag(m, e.addr(), chap, index, 1, 0, 0, 1);
    ed_check_dispose_expr_file(m, o, rec::cur_dict(m));
    true
}

/// The expressions `e` drives removed: for each of the six forward relation groups but the
/// fifth, every relation 20010 (`0x4e2a`) is made current by its serial number, removed from
/// the chapter `chap`'s element `index`, and its expression element given up
/// [soft 0x082559d0 SI_edRemoveElemExpressions].
pub fn ed_remove_elem_expressions(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, e: Ptr32<Elem>, chap: i32, index: i32) {
    let sp = m.sp();
    let oc = Ptr32::<i32>::new(m.local(4));
    let oi = Ptr32::<i32>::new(m.local(4));
    let mut g = 0;
    while g < 6 {
        if g != 4 {
            let mut r = si_dict::elemrel::find_related_elem(m, ed, e, 1, g, 0x4e2a, -1, oc, oi);
            while !r.is_null() {
                let rel = si_dict::elemrel::get_cur_relation(m, ed, e, 1, g).addr();
                let serial = m.i32(rel + 0xc);
                si_dict::elemrel::set_rel_to_serialid(m, ed, e, 0x4e2a, serial, 1);
                si_dict::elemrel::remove_serial_rel_id(m, &mut DictOutside(o), ed, Ptr32::new(SI_RD), chap, index, 0x4e2a, serial);
                let target = m.read(oi);
                ed_dispose_expression(m, o, target);
                r = si_dict::elemrel::find_related_elem(m, ed, e, 1, g, 0x4e2a, -1, oc, oi);
            }
        }
        g += 1;
    }
    m.set_sp(sp);
}

/// The expressions that read `e` (the backward lists) removed the same way; the relation
/// removed is the one of the expression's own element, whose index is the word at +4 of the
/// relation `SI_elemdictSetRelToSerialid` answers when it is made current on the expression
/// element [soft 0x08255ac0 SI_edRemoveExpressionsFromArg].
pub fn ed_remove_expressions_from_arg(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, e: Ptr32<Elem>) {
    let sp = m.sp();
    let oc = Ptr32::<i32>::new(m.local(4));
    let oi = Ptr32::<i32>::new(m.local(4));
    let mut g = 0;
    while g < 6 {
        if g != 4 {
            let mut r = si_dict::elemrel::find_related_elem(m, ed, e, 0, g, 0x4e2a, -1, oc, oi);
            while !r.is_null() {
                let rel = si_dict::elemrel::get_cur_relation(m, ed, e, 0, g).addr();
                let serial = m.i32(rel + 0xc);
                let made = si_dict::elemrel::set_rel_to_serialid(m, ed, Ptr32::<Elem>::new(r.addr()), 0x4e2a, serial, 1);
                let index = m.i32(made.addr().wrapping_add(4));
                let (c, i) = (m.read(oc), m.read(oi));
                si_dict::elemrel::remove_serial_rel_id(m, &mut DictOutside(o), ed, Ptr32::new(SI_RD), c, i, 0x4e2a, serial);
                ed_dispose_expression(m, o, index);
                r = si_dict::elemrel::find_related_elem(m, ed, e, 0, g, 0x4e2a, -1, oc, oi);
            }
        }
        g += 1;
    }
    m.set_sp(sp);
}
