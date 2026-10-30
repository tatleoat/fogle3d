// SPDX-License-Identifier: MIT
//! `mi_defparms.c`: the default parameter declarations of the built-in shaders (package 63,
//! finding 0042): the `.mi` text of a shader's default struct, either from a table of the program
//! or (for `.mi2`) from the declaration in a library file.
//!
//! Tables in the original's read-only data are not in the image: the pointers of `defparms`
//! (the default struct text of each of the eight shader kinds) and of `softShadeNames` (their
//! shader names) are kept here as addresses, the names also as text for the comparison.

use crate::mi_mr_main::{ext, MRPIPE_FPRINTF};
use si_mem::{clib, Mem, Outside, Routine};

const MI2OUTPUT: u32 = 0x086e_f9c0;
/// `defparms`: the text each kind prints as its default struct.
const DEFPARMS: [u32; 8] = [0x086f_3a40, 0x086f_3e60, 0x086f_3fa0, 0x086f_4000, 0x086f_40c0, 0x086f_4160, 0x086f_4320, 0x086f_4520];
/// `softShadeNames`: the address and text of each kind's shader name.
const SHADE_NAMES: [(u32, &[u8]); 8] = [
    (0x086f_39a0, b"soft_material"),
    (0x086f_39ae, b"soft_light"),
    (0x086f_39b9, b"lens_depth_of_field"),
    (0x086f_39cd, b"soft_fog"),
    (0x086f_39d6, b"soft_env_sphere"),
    (0x086f_39e6, b"soft_color"),
    (0x086f_39f1, b"soft_color_3d"),
    (0x086f_39ff, b"soft_displace"),
];
const REWIND: Routine = Routine::new("rewind", 0x080c_3e78);
const FMT_S: u32 = 0x086f_4624;
const FMT_STRUCT: u32 = 0x086f_463f;
const FMT_CLOSE: u32 = 0x086f_465c;

/// [soft 0x08595420 SI_mentalPrintDefaultDeclare]: the default struct of shader kind `idx` (0 to 7):
/// from the library file `file` when the `.mi2` output is on, else the program's own text.
pub fn si_mental_print_default_declare(m: &mut Mem, o: &mut dyn Outside, idx: u32, file: u32) {
    if m.u16(MI2OUTPUT) != 0 {
        mi2_mental_print_default_declare(m, o, idx, file);
    } else {
        o.call(m, MRPIPE_FPRINTF, &[FMT_S, DEFPARMS[idx as usize]]);
    }
}

/// [soft 0x0859545c mi2_mentalPrintDefaultDeclare]: scans the file for `declare shader*`, whose next
/// line names the shader (the second word, between spaces and quotes); when that is the shader
/// of kind `idx`, prints the struct header and then the file's lines up to the one with a `)`.
pub fn mi2_mental_print_default_declare(m: &mut Mem, o: &mut dyn Outside, idx: u32, file: u32) {
    if file == 0 {
        return;
    }
    o.call(m, REWIND, &[file]);
    let buf = m.local(0x1f4);
    // the delimiter and keyword literals of the original's read-only data, made on the stack
    let lit = m.local(16);
    m.set_bytes(lit, b" \0\0\0 \"\0\0shader\0");
    let (sp, spq, shader) = (lit, lit + 4, lit + 8);
    let mut saved = 0u32;
    let (name_addr, name) = SHADE_NAMES[idx as usize];
    while o.call(m, ext::FGETS, &[buf, 0x1f4, file]).eax != 0 {
        let t = clib::strtok(m, buf, sp, &mut saved);
        if t == 0 || m.cstr(t) != b"declare" {
            continue;
        }
        let t = clib::strtok(m, 0, sp, &mut saved);
        if t == 0 || clib::strncmp(m, t, shader, 6) != 0 {
            continue;
        }
        o.call(m, ext::FGETS, &[buf, 0x1f4, file]);
        if clib::strtok(m, buf, sp, &mut saved) == 0 {
            continue;
        }
        let t = clib::strtok(m, 0, spq, &mut saved);
        if t == 0 || m.cstr(t) != name {
            continue;
        }
        o.call(m, MRPIPE_FPRINTF, &[FMT_STRUCT, name_addr]);
        loop {
            o.call(m, ext::FGETS, &[buf, 0x1f4, file]);
            if clib::strchr(m, buf, b')') != 0 {
                o.call(m, MRPIPE_FPRINTF, &[FMT_CLOSE]);
                break;
            }
            o.call(m, MRPIPE_FPRINTF, &[buf]);
        }
    }
    m.release(16);
    m.release(0x1f4);
}
