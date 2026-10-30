// SPDX-License-Identifier: MIT
//! Expression files read into the image (`physicalio.c`, `express_io.c`; finding 0013,
//! sections 7 and 12).
//!
//! The scanner and the parser are not on the image yet. [`elemdict_read_expressions`]
//! reads an expression file with the value form ([`crate::io`]) against the image's own
//! dictionaries ([`ImageDict`]) and lays each tree into the image ([`super::tree`]), into
//! the element of the EXPRESSIONS chapter the block is named after, as the original's
//! `SI_elemdictReadExpressions` (0x0810ff94) does.

// adapter: not in place; the scanner, the parser and the token reader run on values, and
// the tree is copied into the image afterwards

use super::rec::{self, cid};
use crate::env::Dict;
use si_dict::globals::{SI_ATD, SI_EVD, SI_RD};
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::ElemDict;
use std::cell::RefCell;

/// The scanner's and the parser's dictionary questions answered from the image's
/// dictionaries (`SI_EVD`, `SI_ATD`, `SI_RD`) with `si-dict`'s routines. Element look-ups
/// (arguments being validated) are not answered: the file reader does not validate.
pub struct ImageDict<'a> {
    pub m: RefCell<&'a mut Mem>,
}

impl ImageDict<'_> {
    /// A string on the image's stack for the length of one question.
    fn with_cstr<R>(&self, s: &[u8], f: impl FnOnce(&mut Mem, CStr32) -> R) -> R {
        let mut m = self.m.borrow_mut();
        let sp = m.sp();
        let p = m.local(s.len() as u32 + 1);
        m.put_cstr(p, s);
        let r = f(&mut m, Ptr32::new(p));
        m.set_sp(sp);
        r
    }
}

impl Dict for ImageDict<'_> {
    fn enum_val_by_any_code(&self, enum_type: &mut i32, code: &[u8]) -> Option<i32> {
        let t = *enum_type;
        let (v, after) = self.with_cstr(code, |m, c| {
            let sp = m.sp();
            let idx = Ptr32::<i32>::new(m.local(4));
            m.write(idx, t);
            let v = si_dict::enmdict::enum_val_get_by_any_code(m, SI_EVD, idx, c);
            let after = m.read(idx);
            m.set_sp(sp);
            (if v.is_null() { None } else { Some(m.i32(v.addr())) }, after)
        });
        *enum_type = after;
        v
    }

    fn enum_code_by_value(&self, enum_type: i32, id: i32) -> Option<Vec<u8>> {
        let m = self.m.borrow();
        let c = si_dict::enmdict::enum_code_get_by_value(&m, SI_EVD, enum_type, id);
        if c.is_null() {
            None
        } else {
            Some(m.cstr(c.addr()))
        }
    }

    fn rel_id_by_name(&self, name: &[u8]) -> i32 {
        self.with_cstr(name, |m, c| si_dict::reldict::get_rel_id_by_name(m, SI_RD, c))
    }

    fn rel_name(&self, id: i32) -> Vec<u8> {
        let mut m = self.m.borrow_mut();
        let c = si_dict::reldict::get_rel_name(&mut m, SI_RD, id);
        m.cstr(c.addr())
    }

    fn fcurve_from_name(&self, name: &[u8]) -> i32 {
        self.with_cstr(name, |m, c| si_dict::attdict::get_fcurve_from_name(m, SI_ATD, c))
    }

    fn name_from_fcurve(&self, fcv: i32) -> Vec<u8> {
        let mut m = self.m.borrow_mut();
        let c = si_dict::attdict::get_name_from_fcurve(&mut m, SI_ATD, fcv);
        if c.is_null() {
            b"(null)".to_vec()
        } else {
            m.cstr(c.addr())
        }
    }
}

/// What reading a file did: blocks stored into elements, blocks passed over, blocks whose
/// text the parser refused (their element gets a null tree, as in the original).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadCount {
    pub stored: usize,
    pub refused: usize,
}

/// Reads an expression file's bytes into the dictionary `ed`: every block named after an
/// element of the EXPRESSIONS chapter (`SI_elemdictFindElemByFullName`, only elements read
/// from files) is parsed and its tree stored in that element's object (+4), and the
/// chapter's sort table is marked stale; `SI_elemdictReadExpressions (0x0810ff94)` with the
/// file read by the caller. The answer of the original is always 0; here the counts, or
/// the reader's error for a block that ends in the middle (where the original reads empty
/// tokens for ever).
pub fn elemdict_read_expressions(m: &mut Mem, ed: Ptr32<ElemDict>, data: &[u8]) -> Result<ReadCount, crate::io::ReadError> {
    let exp = rec::chap(m, cid::EXP);
    let blocks = {
        let dict = ImageDict { m: RefCell::new(&mut *m) };
        let mut parser = crate::parse::Parser::new();
        // the reader and the parser ask one question at a time, each borrowing the image
        // only while it is asked
        let mut wanted = |name: &[u8]| {
            dict.with_cstr(name, |m, c| si_dict::elemname::find_elem_by_full_name(m, ed, exp, Ptr32::NULL, c, 1)) >= 0
        };
        crate::io::read_expressions(data, &mut parser, &dict, &mut wanted)?
    };
    let mut count = ReadCount::default();
    for (name, block) in blocks {
        let sp = m.sp();
        let p = m.local(name.len() as u32 + 1);
        m.put_cstr(p, &name);
        let i = si_dict::elemname::find_elem_by_full_name(m, ed, exp, Ptr32::NULL, Ptr32::new(p), 1);
        m.set_sp(sp);
        if i < 0 {
            continue;
        }
        let e = si_dict::elemdict::get_elem(m, ed, exp, i);
        let t = match &block.expr {
            Some(x) => {
                count.stored += 1;
                super::tree::put_expression(m, x).addr()
            }
            None => {
                count.refused += 1;
                0
            }
        };
        m.set_u32(e.addr() + 4, t);
        let list = Ptr32::new(m.u32(ed.addr() + 4).wrapping_add((exp as u32).wrapping_mul(0x40)));
        si_dict::elemdict::sort_tbl_set_dirty(m, list);
    }
    Ok(count)
}
