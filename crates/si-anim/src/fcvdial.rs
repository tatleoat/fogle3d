// SPDX-License-Identifier: MIT
//! `fcv_dial.c` (finding 0030, section 18): the function-curve lists of the channel and
//! curve dialogs, the data routines only (the lists of curve types per kind of object, a
//! row's accessors, the name of an item's curve).
//!
//! The ten tables of curve types (`fcvMdl`, `fcvCam`, ...) are data of the binary, lifted by
//! `re/tools/si-anim/lift_fcvlists.py` names by address and length in the program and reads at run time from
//! the user's install into [`lists`] (`si-rt`); the routine hands out their addresses,
//! and a routine here that reads one reads that copy. A text list ("SI_Textlist") has its row
//! array at +4, 0x14 bytes a row: +0xc a record whose 16-bit word at +0x50 is the curve's
//! sub-number.
//!
//! Not ported: `SI_fcvBuildList (0x08209968)` (3.3 KB): it checks a licence out of the licence
//! manager for the model case (`lc_checkout`, after `lmdecrypt`) and the check cannot enter
//! that code.

use crate::outside::{Outside, Routine};
use si_mem::{clib, Mem};

mod lists {
    include!("fcvlists_data.rs");

    /// A list's address and ids, read once from the user's install.
    pub fn get(l: &'static std::sync::LazyLock<(u32, Vec<i32>)>) -> (u32, &'static [i32]) {
        (l.0, &l.1[..])
    }
}

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this module names.
        pub const ROUTINES: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    /// `SI_fcvGetNameFromType (0x0810f640)`: the name of a curve type (a text of the program).
    SI_FCV_GET_NAME_FROM_TYPE = "SI_fcvGetNameFromType", 0x0810f640;
}

/// The table `fcvGetFcvList` chooses for a graph-node type: its address and its ids.
fn table(graf: u32) -> Option<(u32, &'static [i32])> {
    Some(match graf {
        2 | 3 => lists::get(&lists::FCV_MDL),
        4 | 5 => lists::get(&lists::FCV_CAM),
        6 | 7 => lists::get(&lists::FCV_LIGSPOT),
        8 => lists::get(&lists::FCV_LIGBLB),
        0x13 => lists::get(&lists::FCV_LIGINF),
        0x14 => lists::get(&lists::FCV_LIGSUN),
        0xe => lists::get(&lists::FCV_MAT),
        0xf => lists::get(&lists::FCV_T2D),
        0x10 => lists::get(&lists::FCV_T3D),
        0xb => lists::get(&lists::FCV_WAV),
        _ => return None,
    })
}

/// [soft 0x08209830 fcvGetFcvList]
///
/// The table of curve types a kind of graph node has, through two pointers: its address and its
/// length. The kinds: 2 and 3 model, 4 and 5 camera, 6 and 7 spot light, 8 bulb, 0x13 infinite,
/// 0x14 sun, 0xb wave, 0xe material, 0xf and 0x10 the two kinds of texture. Any other kind
/// prints `fcvGetFcvList() : Unknown type` on stderr (no state of the program; not reproduced)
/// and gives a null table of length 0. The fourth argument is unused.
pub fn fcv_get_fcv_list(m: &mut Mem, graf: u32, list: u32, count: u32) {
    match table(graf) {
        Some((addr, ids)) => {
            m.set_u32(list, addr);
            m.set_u32(count, ids.len() as u32);
        }
        None => {
            m.set_u32(list, 0);
            m.set_u32(count, 0);
        }
    }
}

/// [soft 0x08209918 SI_fcvAccessListInfo]
///
/// Row `row` of a text list: the record behind the row (through `rec` when it is not null) and
/// its sub-number (through `num`: the 16-bit word at +0x50 of the record, sign-extended; -1
/// without a record, and the record's pointer 0 then).
pub fn fcv_access_list_info(m: &mut Mem, textlist: u32, row: u32, rec: u32, num: u32) {
    let rows = m.u32(textlist.wrapping_add(4));
    let r = m.u32(rows.wrapping_add(row.wrapping_mul(0x14)).wrapping_add(0xc));
    if r == 0 {
        if rec != 0 {
            m.set_u32(rec, 0);
        }
        if num != 0 {
            m.set_u32(num, 0xffff_ffff);
        }
    } else {
        if rec != 0 {
            m.set_u32(rec, r);
        }
        if num != 0 {
            let v = m.i16(r.wrapping_add(0x50)) as i32 as u32;
            m.set_u32(num, v);
        }
    }
}

/// [soft 0x0820a6a0 SI_grafnodeTypeString]
///
/// The name of a graph-node type: the row of the table `graftypeStr` (0x16 bytes a row).
pub fn grafnode_type_string(ty: u32) -> u32 {
    ty.wrapping_mul(0x16).wrapping_add(0x0869_5680)
}

/// [soft 0x0820a654 SI_isValidFcv]
///
/// 1 when `fcv` is one of the curve types listed for the graph-node type `graf`, else 0 (an
/// unknown kind has an empty list; its message is not reproduced).
pub fn is_valid_fcv(graf: u32, fcv: i32) -> u32 {
    match table(graf) {
        Some((_, ids)) => ids.contains(&fcv) as u32,
        None => 0,
    }
}

/// [soft 0x0820a6b0 SI_itemFcvName]
///
/// Makes the display name of a curve in `out` and answers `out`. Type 0 gives `default`; 0x9a
/// the type's name, `_` and `idx`; 0xb7 the type's name, `_` and as much of `name` as fits in
/// `maxlen - 3` (not below 0) characters; 0xb8 the type's name, `_`, as much of `name` as fits in
/// `maxlen - 6` characters, then `_` and `idx`; any other type its name alone. The name comes
/// from `SI_fcvGetNameFromType`, which the original asks before it writes.
pub fn item_fcv_name(m: &mut Mem, o: &mut dyn Outside, ty: i32, name: u32, idx: i32, maxlen: i32, out: u32) -> u32 {
    let tname = |m: &mut Mem, o: &mut dyn Outside| o.call(m, &SI_FCV_GET_NAME_FROM_TYPE, &[ty as u32]);
    match ty {
        0x9a => {
            let t = tname(m, o);
            let mut s = m.cstr(t);
            s.push(b'_');
            s.extend(clib::decimal(idx));
            m.put_cstr(out, &s);
        }
        0 => m.put_cstr(out, b"default"),
        0xb7 | 0xb8 => {
            let skip = if ty == 0xb7 { 3 } else { 6 };
            let n = maxlen.wrapping_sub(skip).max(0) as u32;
            let t = tname(m, o);
            let mut s = m.cstr(t);
            s.push(b'_');
            m.put_cstr(out, &s);
            clib::strncat(m, out, name, n);
            if ty == 0xb8 {
                let at = out.wrapping_add(m.strlen(out));
                let mut tail = vec![b'_'];
                tail.extend(clib::decimal(idx));
                m.put_cstr(at, &tail);
            }
        }
        _ => {
            let t = tname(m, o);
            let s = m.cstr(t);
            m.put_cstr(out, &s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With an install: the ten lists load (each checked against our checksum) with the lengths
    /// the original's loop bounds have.
    #[test]
    fn lists_load() {
        si_rt::skip_without_install!();
        let lens: Vec<usize> = [2, 4, 6, 8, 0x13, 0x14, 0xe, 0xf, 0x10, 0xb].iter().map(|&g| table(g).unwrap().1.len()).collect();
        assert_eq!(lens, [14, 33, 23, 17, 9, 4, 15, 22, 43, 10]);
        assert_eq!(table(2).unwrap().0, 0x0869_5be0);
        assert!(table(1).is_none());
    }
}
