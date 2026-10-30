// SPDX-License-Identifier: MIT
//! `mr_cmdline.c`: the command line of the external mental ray renderer (package 64, finding 0042). The list of words is a
//! made array of strings (`char **` and a count) grown by `SI_strListAppend`; rodata strings are passed by their address in
//! the original, as elsewhere in this crate.

use si_mem::{Mem, Outside, Routine};

pub const SI_STR_LIST_APPEND: Routine = Routine::new("SI_strListAppend", 0x083c_f668);
pub const SI_STR_LIST_TO_STRING: Routine = Routine::new("SI_strListToString", 0x083c_f77c);

/// Rodata of the original: "-core" (the word `strCORE` points at), "-imgpipe", " " and "on".
const STR_CORE: u32 = 0x086d_2d80;
const STR_IMGPIPE: u32 = 0x086d_2d8f;
const STR_SPACE: u32 = 0x086d_2d98;
const STR_ON: u32 = 0x086d_2d9a;

/// [soft 0x0842c854 addCoreOn]: appends `-core` and `on` to the list unless a word equal to `-core` is already in it.
pub fn add_core_on(m: &mut Mem, o: &mut dyn Outside, list: u32, count: u32) {
    let n = m.i32(count);
    for i in 0..n.max(0) as u32 {
        let words = m.u32(list);
        let w = m.u32(words.wrapping_add(4 * i));
        if m.cstr(w) == b"-core" {
            return;
        }
    }
    o.call(m, SI_STR_LIST_APPEND, &[STR_CORE, list, count]);
    o.call(m, SI_STR_LIST_APPEND, &[STR_ON, list, count]);
}

/// [soft 0x0842c7d0 si_mrfileBuildCmdline]: `*out` = the words joined by blanks; with `pipe` set the words `-imgpipe` and
/// the decimal `value` come first, and `-core on` is always on the end unless already there.
pub fn si_mrfile_build_cmdline(m: &mut Mem, o: &mut dyn Outside, out: u32, pipe: u32, value: u32, list: u32, count: u32) {
    m.set_u32(out, 0);
    if pipe as u16 != 0 {
        let buf = m.local(40);
        let t = format!("{}", value as i32);
        m.put_cstr(buf, t.as_bytes());
        o.call(m, SI_STR_LIST_APPEND, &[STR_IMGPIPE, list, count]);
        o.call(m, SI_STR_LIST_APPEND, &[buf, list, count]);
        m.release(40);
    }
    add_core_on(m, o, list, count);
    let s = o.call(m, SI_STR_LIST_TO_STRING, &[m.u32(list), m.u32(count), STR_SPACE]).eax;
    m.set_u32(out, s);
}
