// SPDX-License-Identifier: MIT
//! `sprd_io.c`: writing a spreadsheet setup as a text file (finding 0043). The reading
//! routines (0x085a7c88, 0x085a825c) are not ported yet.

use crate::{g, glob};
use si_mem::{Mem, Outside, Routine};

pub mod io_hook {
    use super::Routine;
    pub const SET_INDENT: Routine = Routine::new("SI_setIndent", 0x083d_c75c);
    pub const RELDICT_GET_REL_NAME: Routine = Routine::new("SI_reldictGetRelName", 0x0813_a0f4);
    pub const FOPEN: Routine = Routine::new("fopen", 0x080c_4e18);
    pub const FCLOSE: Routine = Routine::new("fclose", 0x080c_4958);
}
use io_hook::*;

/// Globals of the file routines.
pub mod io_glob {
    pub const SI_INDENT_STRING: u32 = 0x086c_eba0;
    pub const SI_PIPED_FILES: u32 = 0x086c_eba8;
    pub const SI_PIPE: u32 = 0x086c_ebac;
    pub const SI_SAVE_ELEM_ERROR_CODE: u32 = 0x086c_ebb0;
    pub const SI_RD: u32 = 0x0878_15f0;
}
use io_glob::*;

fn pf(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt: u32, args: &[u32]) {
    let mut a = vec![file, fmt];
    a.extend_from_slice(args);
    o.call(m, crate::hook::FPRINTF, &a);
}

fn word(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt_label: u32, fmt_word: u32, word: u32) {
    let ind = m.u32(SI_INDENT_STRING);
    pf(m, o, file, fmt_label, &[ind]);
    pf(m, o, file, fmt_word, &[word]);
}

fn indent(m: &mut Mem, o: &mut dyn Outside, n: i32) {
    o.call(m, SET_INDENT, &[n as u32]);
}

/// One group of a setup and its sub groups written as text
/// [soft 0x085a7710 sprdSetupGroupWrite].
pub fn setup_group_write(m: &mut Mem, o: &mut dyn Outside, file: u32, atd: u32, grp: u32) {
    pf(m, o, file, 0x086f_7040, &[]);
    word(m, o, file, 0x086f_7042, 0x086f_7052, 0x086f_7045);
    indent(m, o, 1);
    word(m, o, file, 0x086f_7056, 0x086f_7064, 0x086f_7059);
    pf(m, o, file, 0x086f_7068, &[m.u32(grp)]);
    if (m.i32(grp + 4)) > -1 {
        word(m, o, file, 0x086f_706c, 0x086f_707e, 0x086f_706f);
        let rel = o.call(m, RELDICT_GET_REL_NAME, &[SI_RD, m.u32(grp + 4)]).eax;
        pf(m, o, file, 0x086f_7082, &[rel]);
        if m.i16(grp + 8) == 0 {
            pf(m, o, file, 0x086f_709b, &[0x086f_7092]);
        } else {
            pf(m, o, file, 0x086f_708e, &[0x086f_7086]);
        }
    }
    word(m, o, file, 0x086f_709f, 0x086f_70ad, 0x086f_70a2);
    indent(m, o, 1);
    let ind = m.u32(SI_INDENT_STRING);
    pf(m, o, file, 0x086f_70b1, &[ind]);
    pf(m, o, file, 0x086f_70b4, &[m.u32(grp + 0x14)]);
    indent(m, o, -1);
    word(m, o, file, 0x086f_70b8, 0x086f_70cb, 0x086f_70bb);
    word(m, o, file, 0x086f_70cf, 0x086f_70e0, 0x086f_70d2);
    indent(m, o, 1);
    let mut i = 0i32;
    while i < m.i32(grp + 0x34) {
        let item = m.u32(grp + 0x38).wrapping_add((i as u32).wrapping_mul(0x14));
        let rec = o.call(m, crate::hook::ATTDICT_ATTRIBUTE_GET_BY_INDEX, &[crate::glob::SI_ATD, item]).eax;
        let ind = m.u32(SI_INDENT_STRING);
        pf(m, o, file, 0x086f_70e4, &[ind]);
        let fcv = m.u32(m.u32(rec + 0x5c).wrapping_add(m.u32(item + 0x10).wrapping_mul(4)));
        pf(m, o, file, 0x086f_70e7, &[fcv]);
        let dir = m.i16(m.u32(grp + 0x3c).wrapping_add((i as u32).wrapping_mul(2))) as i32 as u32;
        pf(m, o, file, 0x086f_70eb, &[dir]);
        pf(m, o, file, 0x086f_70f1, &[0x086f_70ef]);
        i += 1;
    }
    indent(m, o, -1);
    word(m, o, file, 0x086f_70f5, 0x086f_7104, 0x086f_70f8);
    word(m, o, file, 0x086f_7108, 0x086f_7117, 0x086f_710b);
    indent(m, o, 1);
    let mut i = 0i32;
    while i < m.i32(grp + 0x28) {
        let item = m.u32(grp + 0x2c).wrapping_add((i as u32).wrapping_mul(0x14));
        let rec = o.call(m, crate::hook::ATTDICT_ATTRIBUTE_GET_BY_INDEX, &[crate::glob::SI_ATD, item]).eax;
        let ind = m.u32(SI_INDENT_STRING);
        pf(m, o, file, 0x086f_711b, &[ind]);
        let fcv = m.u32(m.u32(rec + 0x5c).wrapping_add(m.u32(item + 0x10).wrapping_mul(4)));
        pf(m, o, file, 0x086f_711e, &[fcv]);
        if m.i16(m.u32(grp + 0x30).wrapping_add((i as u32).wrapping_mul(2))) == 0 {
            pf(m, o, file, 0x086f_713b, &[0x086f_7130]);
        } else {
            pf(m, o, file, 0x086f_712c, &[0x086f_7122]);
        }
        pf(m, o, file, 0x086f_7141, &[0x086f_713f]);
        i += 1;
    }
    indent(m, o, -1);
    word(m, o, file, 0x086f_7145, 0x086f_7152, 0x086f_7148);
    indent(m, o, 1);
    let mut i = 0i32;
    while i < m.i16(grp + g::NSUB) as i32 {
        let sub = m.u32(grp + g::SUB).wrapping_add((i as u32).wrapping_mul(0x50));
        setup_group_write(m, o, file, atd, sub);
        i += 1;
    }
    indent(m, o, -2);
    word(m, o, file, 0x086f_7156, 0x086f_7164, 0x086f_7159);
}

/// A setup written to a file (or the pipe) [soft 0x085a7be8 SI_sprdSetupWrite]; 1 on success.
pub fn setup_write(m: &mut Mem, o: &mut dyn Outside, path: u32, atd: u32, s: u32, quiet: i16) -> u32 {
    if quiet != 0 && m.i16(s + 0x6e) != 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 9);
        return 0;
    }
    let file = if m.i16(SI_PIPED_FILES) == 0 { o.call(m, FOPEN, &[path, 0x086f_7168]).eax } else { m.u32(SI_PIPE) };
    if file == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 1);
        return 0;
    }
    indent(m, o, 0);
    setup_group_write(m, o, file, atd, s + 8);
    o.call(m, FCLOSE, &[file]);
    1
}

#[allow(dead_code)]
fn _g() -> u32 {
    glob::SI_ATD
}
