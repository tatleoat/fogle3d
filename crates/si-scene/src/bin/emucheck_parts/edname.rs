// SPDX-License-Identifier: MIT
//! The outside world of `si_scene::edname::ed_element_change_name` (vectors of
//! `re/tools/chgname1/emu_gen_edname.py`): the dictionary's name routines and
//! `SI_strAppend` ran as they are in the emulator; the dialog and the two renames of what
//! hangs on an element are recorded stand-ins whose text arguments the generator logs as
//! the CRC-32 of the string they point to (the stack buffers of the original and the port
//! do not share addresses).

use super::{Outside, Recorded, Routine};
use si_mem::Mem;
use si_scene::edname as n;

static R_DIALOG: Routine = Routine { name: "SI_dialogRequest", addr: n::SI_DIALOG_REQUEST };
static R_ANIM_NAME: Routine = Routine { name: "si_edAnimElemChangeName", addr: n::SI_ED_ANIM_ELEM_CHANGE_NAME };
static R_DOPE_NAME: Routine = Routine { name: "SI_dopeItemChangeName", addr: n::SI_DOPE_ITEM_CHANGE_NAME };

/// CRC-32 (zlib's) of the C string at `p`; 0 for a null pointer.
fn crc_of(m: &Mem, p: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    let mut c = 0xffff_ffffu32;
    for b in m.cstr(p) {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// `SI_strAppend`, as the binary has it (the same blocks come and go).
fn str_append(m: &mut Mem, dest: u32, text: u32) -> u32 {
    if text == 0 || dest == 0 {
        return 0;
    }
    let old = m.u32(dest);
    if old == 0 {
        let c = m.strdup(text);
        m.set_u32(dest, c);
    } else {
        let len = (m.cstr(old).len() + m.cstr(text).len() + 1) as u32;
        let buf = m.si_malloc(len);
        if buf == 0 {
            return 0;
        }
        let mut v = m.cstr(old);
        v.extend(m.cstr(text));
        m.put_cstr(buf, &v);
        let cur = m.u32(dest);
        if cur != 0 {
            m.free(cur);
            m.set_u32(dest, 0);
        }
        m.set_u32(dest, buf);
    }
    (m.u32(dest) != 0) as u32
}

/// Answers a call of this file's routines, or `None`.
pub fn call(rec: &mut Recorded, m: &mut Mem, f: u32, args: &[u32]) -> Option<u32> {
    let a = |k: usize| args.get(k).copied().unwrap_or(0);
    let recorded = |rec: &Recorded, name: &str| rec.case.calls.iter().any(|c| c.name == name);
    match f {
        n::SI_STR_APPEND if !recorded(rec, "SI_strAppend") => Some(str_append(m, a(0), a(1))),
        n::SI_ELEMDICT_GET_CHAP_NAME if !recorded(rec, "SI_elemdictGetChapName") => {
            Some(si_dict::elemdict::get_chap_name(m, si_mem::Ptr32::new(a(0)), a(1) as i32).addr())
        }
        n::SI_DIALOG_REQUEST if recorded(rec, "SI_dialogRequest") => {
            let h: Vec<u32> = (0..4).map(|k| crc_of(m, a(k))).collect();
            Some(Outside::call(rec, m, &R_DIALOG, &h))
        }
        n::SI_ED_ANIM_ELEM_CHANGE_NAME if recorded(rec, "si_edAnimElemChangeName") => {
            let h = [a(0), a(1), crc_of(m, a(2)), crc_of(m, a(3)), crc_of(m, a(4))];
            Some(Outside::call(rec, m, &R_ANIM_NAME, &h))
        }
        n::SI_DOPE_ITEM_CHANGE_NAME if recorded(rec, "SI_dopeItemChangeName") => Some(Outside::call(rec, m, &R_DOPE_NAME, args)),
        _ => None,
    }
}
