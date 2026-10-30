// SPDX-License-Identifier: MIT
//! `dbexternal.c`: the scene-file checks and the database delete of the browser.
//! Finding 0007, 2026-10-28 section.

use crate::calls::DbCalls;
use crate::imglobals::{SI_ELEM_DICTS, SI_PROC_DATA};
use si_mem::clib;
use si_mem::{Mem, Outside};

/// [soft 0x0812458c dbfileCheckElemInList]: for the first element (0x84-byte records, `count` at
/// +4, array at +8) that is marked in the list (work flag +0x58 set, +0x48 clear, +0x42 set)
/// and whose name (name record +0x24) equals `name`, clears the work flag and counts it.
pub fn check_elem_in_list(m: &mut Mem, list: u32, name: u32, found: u32) {
    let n = m.i32(list + 4);
    let mut i = 0;
    while i < n {
        let e = m.u32(list + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        if m.u32(e + 0x58) != 0 && m.u16(e + 0x48) == 0 && m.u16(e + 0x42) != 0 {
            let rec = m.u32(e);
            if clib::strcmp(m, m.u32(rec + 0x24), name) == 0 {
                m.set_u32(e + 0x58, 0);
                let c = m.u32(found);
                m.set_u32(found, c.wrapping_add(1));
            }
        }
        i += 1;
    }
}

/// [soft 0x08127168 SI_dbDatabaseDelete]: deletes a database from the directory list (`dir`:
/// count at +0, 0x28-byte entries at +4, changed flag at +8). With `ext` it first deletes the
/// external directory (0x52 when `only_empty` and the database is not empty, 0x51 when the
/// deletion failed); it then resets the current dictionary's origin database, closes the gap
/// in the list and either marks the list changed or rewrites its file (0x5c when that fails).
/// Returns 0x53 (external) or 0x54.
#[allow(clippy::too_many_arguments)]
pub fn database_delete(m: &mut Mem, o: &mut dyn Outside, dir: u32, info: u32, ext: i16, only_empty: i16, write: i16, path: u32) -> u32 {
    if ext != 0 {
        if only_empty != 0 && (o.db_database_is_empty(m, info) as i16) == 0 {
            return 0x52;
        }
        let name = m.u32(info + 0xc);
        if o.db_delete_ext_database(m, name) == 0 {
            return 0x51;
        }
    }
    let idx = m.u32(m.u32(SI_PROC_DATA.0));
    let dict = m.u32(SI_ELEM_DICTS.0).wrapping_add(idx.wrapping_mul(0x70));
    let name = m.u32(info + 0xc);
    o.elemdict_reset_origin_db(m, dict, name);
    let mut at = o.db_dir_get_database_index(m, dir, info);
    if at >= 0 {
        let count = m.i32(dir);
        while at < count - 1 {
            let base = m.u32(dir + 4);
            let to = base.wrapping_add((at as u32).wrapping_mul(0x28));
            for k in 0..10 {
                let v = m.u32(to + 0x28 + k * 4);
                m.set_u32(to + k * 4, v);
            }
            at += 1;
        }
        m.set_u32(dir, (count - 1) as u32);
        if write == 0 {
            m.set_u16(dir + 8, 1);
        } else if (o.db_database_dir_write(m, dir, path, 0, 0) as i16) == 0x5c {
            return 0x5c;
        }
    }
    if ext != 0 { 0x53 } else { 0x54 }
}
