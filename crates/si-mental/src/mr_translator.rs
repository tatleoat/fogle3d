// SPDX-License-Identifier: MIT
//! `mr_translator.c`: the thread that runs the translator (`sequence_mental`) for the external mental ray (package 63, finding 0042).
//! It works on the render parameter block (`SI_renderExecuteParms`): +0x08 result, +0x2c and +0x30 buffers, +0x34 descriptor,
//! +0x60e "no thread storage", +0x630 mutex, +0x640 "mutex held", +0x642 stop flag, +0x652 "catch signals", +0x658 counter.

use crate::mr_messages::{ALARM, LONGJMP, SETJMP, SIGNAL, SI_MRTHREAD_EXIT, SI_MUTEX_UNLOCK};
use si_mem::{Mem, Outside, Routine};

pub const SEQUENCE_MENTAL: Routine = Routine::new("sequence_mental", 0x0858_1310);
pub const SI_THREAD_STORAGE_INIT: Routine = Routine::new("SI_threadStorageInit", 0x080d_0f80);
pub const SI_THREAD_STORAGE_FREE: Routine = Routine::new("SI_threadStorageFree", 0x080d_0fa8);
pub const CLOSE: Routine = Routine::new("close", 0x080c_3b08);
pub const UNLINK: Routine = Routine::new("unlink", 0x080c_3c78);
const JMP_ERROR: u32 = 0x0881_a5c0;
const HANDLER: u32 = 0x0842_c27c;
const HANDLER_SIGPIPE: u32 = 0x0842_c294;
const W_8790DEE: u32 = 0x0879_0dee;

/// [soft 0x0842c2ac si_mrtranslatorProc_Protected]: marks the block running (result 1), clears the stop flag and counter, runs `sequence_mental`, clears the result.
pub fn si_mrtranslatorproc_protected(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    if m.u16(p + 0x60e) == 0 {
        o.call(m, SI_THREAD_STORAGE_INIT, &[p + 0x634]);
    }
    m.set_u32(p + 8, 1);
    m.set_u16(p + 0x642, 0);
    m.set_u32(p + 0x658, 0);
    o.call(m, SEQUENCE_MENTAL, &[p]);
    m.set_u32(p + 8, 0);
}

/// [soft 0x0842c110 si_mrtranslatorProc]: the thread's body. A jump back with 1 (SIGBUS/SIGSEGV) leaves result 6, with 2 (SIGPIPE) result 3; the
/// mutex is released if held and the signals are reset. A null block makes the original write to address 8 and fault; here it only returns.
pub fn si_mrtranslatorproc(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    if p == 0 {
        return;
    }
    o.call(m, ALARM, &[0]);
    let r = o.call(m, SETJMP, &[JMP_ERROR]).eax as i32;
    if r == 1 || r == 2 {
        m.set_u32(p + 8, if r == 1 { 6 } else { 3 });
        if m.u16(p + 0x640) != 0 {
            o.call(m, SI_MUTEX_UNLOCK, &[p + 0x630]);
        }
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
        o.call(m, SIGNAL, &[0xd, 0]);
        if m.u16(p + 0x60e) == 0 {
            o.call(m, SI_THREAD_STORAGE_FREE, &[]);
        }
        return;
    }
    if m.u16(p + 0x652) != 0 {
        o.call(m, SIGNAL, &[7, HANDLER]);
        o.call(m, SIGNAL, &[0xb, HANDLER]);
        o.call(m, SIGNAL, &[0xd, HANDLER_SIGPIPE]);
    }
    si_mrtranslatorproc_protected(m, o, p);
    if m.u16(p + 0x652) != 0 {
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
        o.call(m, SIGNAL, &[0xd, 0]);
    }
    if m.u16(p + 0x60e) == 0 {
        o.call(m, SI_THREAD_STORAGE_FREE, &[]);
    }
}

/// [soft 0x0842c27c handler]: signal handler of the translator thread: jumps back to `si_mrtranslatorProc` with 1.
pub fn handler(m: &mut Mem, o: &mut dyn Outside, _sig: u32) {
    o.call(m, LONGJMP, &[JMP_ERROR, 1]);
}

/// [soft 0x0842c294 handler_SIGPIPE]: signal handler for a broken pipe: jumps back with 2.
pub fn handler_sigpipe(m: &mut Mem, o: &mut dyn Outside, _sig: u32) {
    o.call(m, LONGJMP, &[JMP_ERROR, 2]);
}

/// [soft 0x0842c308 si_mrtranslatorProcCleanup]: frees the buffer at +0x2c; with the flag at 0x08790dee set, closes the descriptor at +0x34, removes the file named at +0x30 and frees the name.
pub fn si_mrtranslatorproc_cleanup(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    let b = m.u32(p + 0x2c);
    if b != 0 {
        m.free(b);
        m.set_u32(p + 0x2c, 0);
    }
    if m.u16(W_8790DEE) != 0 {
        let fd = m.u32(p + 0x34);
        o.call(m, CLOSE, &[fd]);
        let name = m.u32(p + 0x30);
        o.call(m, UNLINK, &[name]);
        let name = m.u32(p + 0x30);
        if name != 0 {
            m.free(name);
            m.set_u32(p + 0x30, 0);
        }
    }
}
#[allow(dead_code)]
const _UNUSED: Routine = SI_MRTHREAD_EXIT;
