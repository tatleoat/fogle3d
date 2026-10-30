// SPDX-License-Identifier: MIT
//! `shdb_hooks.c`: the callbacks the preview window hands to the mental ray shaders it loads
//! (finding 0041). The dynamic link itself (`si_shdbHookDynalink`) and the window glue are not
//! ported; the small callbacks are, and the texture lookup of the preview.

use crate::custom_misc::GETENV;
use crate::STDERR;
use si_mem::{Mem, Outside, Routine};

/// `fputs`, the binary's stub.
pub const FPUTS: Routine = Routine::new("plt:fputs", 0x080c_4408);
/// `s_temp`: the pointer to three shorts (the render-interrupt flags), and `bStopRendering`.
pub const S_TEMP: u32 = 0x0882_4804;
pub const B_STOP_RENDERING: u32 = 0x0882_4808;

/// [soft 0x0854ef50 si_interrupt_checkflag]: the shader's interrupt protocol. Mode 0 (stop) clears
/// the first two flags and sets `bStopRendering`; mode 1 (start) sets flag 1 only and clears the stop;
/// mode 2 (query) sets all three flags and answers `bStopRendering` (sign-extended); mode 3 (done)
/// sets flag 0 and clears flag 1 and the stop. Any other mode only answers 0.
pub fn si_interrupt_checkflag(m: &mut Mem, mode: i32) -> i32 {
    let t = m.u32(S_TEMP);
    match mode {
        0 => {
            m.set_u16(t.wrapping_add(2), 0);
            m.set_u16(m.u32(S_TEMP), 0);
            m.set_u16(B_STOP_RENDERING, 1);
        }
        1 => {
            m.set_u16(t, 0);
            m.set_u16(m.u32(S_TEMP).wrapping_add(2), 1);
            m.set_u16(m.u32(S_TEMP).wrapping_add(4), 0);
            m.set_u16(B_STOP_RENDERING, 0);
        }
        2 => {
            m.set_u16(t, 1);
            m.set_u16(m.u32(S_TEMP).wrapping_add(2), 1);
            m.set_u16(m.u32(S_TEMP).wrapping_add(4), 1);
            return i32::from(m.i16(B_STOP_RENDERING));
        }
        3 => {
            m.set_u16(t, 1);
            m.set_u16(m.u32(S_TEMP).wrapping_add(2), 0);
            m.set_u16(B_STOP_RENDERING, 0);
        }
        _ => {}
    }
    0
}

/// [soft 0x0854f004 si_print_errormessage]: `fputs(text, stderr)`.
pub fn si_print_errormessage(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    let fp = m.u32(STDERR);
    o.call(m, FPUTS, &[text, fp]);
}

/// [soft 0x0854f020 si_begin_gl] and [soft 0x0854f028 si_end_gl]: empty.
pub fn si_begin_gl() {}
pub fn si_end_gl() {}

/// `SI_mrfileGetExecFullpath (0x0842f930)`: (name, &path); a short, 0 when the file is not found.
pub const SI_MRFILE_GET_EXEC_FULLPATH: Routine = Routine::new("SI_mrfileGetExecFullpath", 0x0842_f930);
/// `SI_dynlibLoad (0x08599560)`, `SI_dynlibSym (0x085995c8)`, `SI_dynlibFree (0x0859959c)`.
pub const SI_DYNLIB_LOAD: Routine = Routine::new("SI_dynlibLoad", 0x0859_9560);
pub const SI_DYNLIB_SYM: Routine = Routine::new("SI_dynlibSym", 0x0859_95c8);
pub const SI_DYNLIB_FREE: Routine = Routine::new("SI_dynlibFree", 0x0859_959c);
/// `szSI_MI_PREVIEW`: the pointer to the name of the environment variable that names the preview library.
pub const SZ_SI_MI_PREVIEW: u32 = 0x086e_5520;
/// The symbols looked up in the library and the names the callbacks are registered under.
const S_MI_START_PREVIEW: u32 = 0x086e_5524;
const S_MI_PREVIEW_RENDER_ABORT: u32 = 0x086e_5535;
const S_MI_REGISTER_PREVIEW_FUNCTION: u32 = 0x086e_554d;
const REGISTERED: [(u32, u32); 5] = [
    (0x086e_556a, 0x0854_f004), // si_print_errormessage
    (0x086e_5580, 0x0854_f020), // si_begin_gl
    (0x086e_558c, 0x0854_f028), // si_end_gl
    (0x086e_5596, 0x0854_ea9c), // si_lookup_preview_texture
    (0x086e_55b0, 0x0854_ef50), // si_interrupt_checkflag
];

/// [soft 0x0854e900 si_shdbHookDynalink]: loads the library the environment variable `szSI_MI_PREVIEW` names (`*handle` set
/// to -1 first). Answers 8 when the variable is not set, 9 when the file is not found, 10 when it does not load, 11 when
/// `mi_start_preview`, `mi_preview_render_abort` or `mi_register_preview_function` is missing (the library is freed again),
/// else registers the five callbacks of this file with `mi_register_preview_function` (name, address) and answers 0. The
/// start and abort entry points go to `*start` and `*abort`.
pub fn si_shdb_hook_dynalink(m: &mut Mem, o: &mut dyn Outside, handle: u32, start: u32, abort: u32) -> u32 {
    m.set_u32(handle, 0xffff_ffff);
    let name = m.u32(SZ_SI_MI_PREVIEW);
    let env = o.call(m, GETENV, &[name]).eax;
    if env == 0 {
        return 8;
    }
    let path = m.local(4);
    m.set_u32(path, 0);
    let found = o.call(m, SI_MRFILE_GET_EXEC_FULLPATH, &[env, path]).eax as u16;
    if found == 0 {
        let p = m.u32(path);
        if p != 0 {
            m.free(p);
            m.set_u32(path, 0);
        }
        return 9;
    }
    let p = m.u32(path);
    let loaded = o.call(m, SI_DYNLIB_LOAD, &[handle, p]).eax as u16;
    let p = m.u32(path);
    if p != 0 {
        m.free(p);
        m.set_u32(path, 0);
    }
    if loaded == 0 {
        return 10;
    }
    let f = o.call(m, SI_DYNLIB_SYM, &[handle, S_MI_START_PREVIEW]).eax;
    m.set_u32(start, f);
    let f = o.call(m, SI_DYNLIB_SYM, &[handle, S_MI_PREVIEW_RENDER_ABORT]).eax;
    m.set_u32(abort, f);
    let reg = o.call(m, SI_DYNLIB_SYM, &[handle, S_MI_REGISTER_PREVIEW_FUNCTION]).eax;
    if m.u32(start) == 0 || m.u32(abort) == 0 || reg == 0 {
        o.call(m, SI_DYNLIB_FREE, &[handle]);
        return 11;
    }
    for (name, addr) in REGISTERED {
        o.call(m, Routine::at(reg), &[name, addr]);
    }
    0
}

/// [soft 0x0854ea70 si_shdbUnhookDynalink]: frees the library unless the handle is still -1 (then 0), sets the handle to -1
/// and answers what `SI_dynlibFree` answered (a short, sign-extended).
pub fn si_shdb_unhook_dynalink(m: &mut Mem, o: &mut dyn Outside, handle: u32) -> i32 {
    if m.u32(handle) == 0xffff_ffff {
        return 0;
    }
    let r = o.call(m, SI_DYNLIB_FREE, &[handle]).eax as u16 as i16 as i32;
    m.set_u32(handle, 0xffff_ffff);
    r
}
