// SPDX-License-Identifier: MIT
//! `mr_messages.c`: the thread that reads the text mental ray writes to its message pipe and hands it, line by line, to a
//! callback (package 63, finding 0042). The thread's parameter block (`SI_mrthreadParms`): +0x00 result, +0x14 pipe, +0x18
//! mutex, +0x1c "mutex held", +0x1e stop flag, +0x20 "catch signals", +0x24 callback, +0x28 buffer, +0x2c capacity, +0x30 fill.

use si_mem::{clib, Mem, Outside, Routine};

pub const SI_PIPEIO_READ: Routine = Routine::new("SI_pipeioRead", 0x0842_fd90);
pub const SI_MUTEX_LOCK: Routine = Routine::new("SI_mutexLock", 0x0842_fc5c);
pub const SI_MUTEX_UNLOCK: Routine = Routine::new("SI_mutexUnlock", 0x0842_fc80);
pub const SI_CALLOC: Routine = Routine::new("SI_calloc", 0x080d_0f50);
pub const SI_MRTHREAD_EXIT: Routine = Routine::new("SI_mrthreadExit", 0x0843_02cc);
pub const SLEEP: Routine = Routine::new("sleep", 0x080c_4468);
pub const ALARM: Routine = Routine::new("alarm", 0x080c_5108);
pub const SETJMP: Routine = Routine::new("_setjmp", 0x080c_4e48);
pub const LONGJMP: Routine = Routine::new("longjmp", 0x080c_4458);
pub const SIGNAL: Routine = Routine::new("signal", 0x080c_3bd8);
/// The jump buffer of this thread (`jmpError`) and the address of `handler`.
const JMP_ERROR: u32 = 0x0881_a520;
const HANDLER: u32 = 0x0842_be70;

/// [soft 0x0842bfd4 processMsgBuffer]: takes the first line out of the buffer `buf` (`*count` bytes, capacity 0x400) into `dst` (at most
/// `size - 1` bytes, `dst` cleared first); a buffer of more than 0x7f bytes without a newline gives its first 0x7f bytes. 1 when a line was taken.
pub fn process_msg_buffer(m: &mut Mem, count: u32, buf: u32, dst: u32, size: i32) -> u32 {
    let nl = clib::strchr(m, buf, b'\n');
    if nl == 0 {
        if m.i32(count) > 0x7f {
            clib::memmove(m, dst, buf, 0x7f);
            m.set_u8(dst + 0x7f, 0);
            clib::memmove(m, buf, buf + 0x7f, 0x381);
            let n = m.i32(count).wrapping_sub(0x7f);
            m.set_u32(count, n as u32);
            m.set_u8(buf.wrapping_add(n as u32), 0);
            return 1;
        }
        return 0;
    }
    m.set_u8(nl, 0);
    clib::memset(m, dst, 0, size as u32);
    let len = clib::strlen(m, buf) as i32;
    let n = if len < size.wrapping_sub(1) { len } else { size.wrapping_sub(1) };
    clib::memmove(m, dst, buf, n as u32);
    let used = nl + 1 - buf;
    clib::memmove(m, buf, nl + 1, 0x400u32.wrapping_sub(used));
    let left = m.i32(count).wrapping_sub(used as i32);
    m.set_u32(count, left as u32);
    m.set_u8(buf.wrapping_add(left as u32), 0);
    1
}

/// [soft 0x0842bf6c processMessages]: every complete line in the thread's buffer goes to the callback (when there is one), under the mutex.
pub fn process_messages(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    let line = m.local(0x80);
    while process_msg_buffer(m, p + 0x30, m.u32(p + 0x28), line, 0x80) as u16 != 0 {
        let cb = m.u32(p + 0x24);
        if cb != 0 {
            o.call(m, SI_MUTEX_LOCK, &[p + 0x18]);
            o.call(m, Routine::new(if cb == 0x083c_ca94 { "SI_statusDisplayMessage" } else { "callback" }, cb), &[line]);
            o.call(m, SI_MUTEX_UNLOCK, &[p + 0x18]);
        }
    }
    m.release(0x80);
}

/// [soft 0x0842bef4 pipeReadMessages]: reads what the pipe has into the buffer after the fill (a full buffer reads nothing). 0 when all went well,
/// 3 when the read failed, 2 when the stop flag is set after the read (after a one second sleep when nothing came).
pub fn pipe_read_messages(m: &mut Mem, o: &mut dyn Outside, p: u32) -> u32 {
    let mut fill = m.i32(p + 0x30);
    let buf = m.u32(p + 0x28);
    if fill <= 0x3ff {
        let got = m.local(4);
        let pipe = m.u32(p + 0x14);
        let r = o.call(m, SI_PIPEIO_READ, &[pipe, (fill as u32).wrapping_add(buf), (0x3ffi32.wrapping_sub(fill)) as u32, got]).eax;
        let n = m.i32(got);
        m.release(4);
        if r != 0 {
            return 3;
        }
        if n == 0 {
            o.call(m, SLEEP, &[1]);
        }
        if m.u16(p + 0x1e) != 0 {
            return 2;
        }
        fill = fill.wrapping_add(n);
        m.set_u8((fill as u32).wrapping_add(buf), 0);
    }
    m.set_u32(p + 0x30, fill as u32);
    0
}

/// [soft 0x0842be88 si_mrmsgProc_Protected]: allocates the 0x400 byte buffer (result 7 when that fails) and reads and handles messages until the stop flag is set or a read fails.
pub fn si_mrmsgproc_protected(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    m.set_u32(p + 0x30, 0);
    m.set_u32(p + 0x2c, 0x400);
    let b = o.call(m, SI_CALLOC, &[0x400, 1]).eax;
    m.set_u32(p + 0x28, b);
    if b == 0 {
        m.set_u32(p, 7);
        return;
    }
    loop {
        let r = pipe_read_messages(m, o, p);
        if m.u16(p + 0x1e) != 0 || r != 0 {
            return;
        }
        process_messages(m, o, p);
        if m.u16(p + 0x1e) != 0 {
            return;
        }
    }
}

/// [soft 0x0842bd70 si_mrmsgProc]: the thread's body: runs the message loop, with SIGBUS/SIGSEGV (7, 11) caught when the parameter block asks for it;
/// a signal ends in `handler`'s jump back here (result 6, mutex released). A null block makes the original write to address 8 of the null page
/// and fault; here it only leaves the thread.
pub fn si_mrmsgproc(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    if p == 0 {
        o.call(m, SI_MRTHREAD_EXIT, &[0]);
        return;
    }
    o.call(m, ALARM, &[0]);
    if o.call(m, SETJMP, &[JMP_ERROR]).eax != 0 {
        m.set_u32(p, 6);
        if m.u16(p + 0x1c) != 0 {
            o.call(m, SI_MUTEX_UNLOCK, &[p + 0x18]);
        }
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
        // the original goes on into the code below when the exit call returns (it does not in the program: the thread ends)
        o.call(m, SI_MRTHREAD_EXIT, &[0]);
    }
    if m.u16(p + 0x20) != 0 {
        o.call(m, SIGNAL, &[7, HANDLER]);
        o.call(m, SIGNAL, &[0xb, HANDLER]);
    }
    si_mrmsgproc_protected(m, o, p);
    if m.u16(p + 0x20) != 0 {
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
    }
    o.call(m, SI_MRTHREAD_EXIT, &[0]);
}

/// [soft 0x0842be70 handler]: the signal handler of the message thread: jumps back to `si_mrmsgProc` with 1.
pub fn handler(m: &mut Mem, o: &mut dyn Outside, _sig: u32) {
    o.call(m, LONGJMP, &[JMP_ERROR, 1]);
}

/// [soft 0x0842c0e4 si_mrmsgProcCleanup]: frees the thread's buffer.
pub fn si_mrmsgproc_cleanup(m: &mut Mem, p: u32) {
    let b = m.u32(p + 0x28);
    if b != 0 {
        m.free(b);
        m.set_u32(p + 0x28, 0);
    }
}
