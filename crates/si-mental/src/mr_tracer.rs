// SPDX-License-Identifier: MIT
//! `SI_mentalRunTracer` (package 63, finding 0042): starts the external mental ray, the threads that read its messages, tile the picture and
//! run the translator, then waits for them and cleans up. Written from the decompiled flow; everything it calls goes through [`Outside`] except the
//! ported siblings of `mr_main.c`, `mr_messages.c`, `mr_translator.c` and `mr_cmdline.c`, which are called directly as the original does.
//! Compared against the emulated original with `re/tools/si-mental/gen_mrtracer.py` and `mrtracer-checkkit` (finding 0042, round 11).

use crate::mi_mr_main::{ext, si_get_mi2_output};
use crate::mr_main::{check_resources, check_user_quit, compute_window_size_pos, wait_after_done, USLEEP};
use crate::{mr_cmdline, mr_messages, mr_translator};
use si_mem::{Mem, Outside, Routine};

pub const SI_STR_STRIP_STRING: Routine = Routine::new("SI_strStripString", 0x083c_f57c);
pub const SI_STR_LIST_APPEND: Routine = Routine::new("SI_strListAppend", 0x083c_f668);
pub const SI_STR_LIST_DESTROY: Routine = Routine::new("SI_strListDestroy", 0x083c_f7fc);
pub const SI_MRWIN_OPEN: Routine = Routine::new("si_mrwinOpen", 0x0842_d210);
pub const SI_MRWIN_CLOSE: Routine = Routine::new("si_mrwinClose", 0x0842_d370);
pub const SI_MRTILE_PROC_CLEANUP: Routine = Routine::new("si_mrtileProcCleanup", 0x0842_d520);
pub const SI_MRFILE_GET_EXEC_FULLPATH: Routine = Routine::new("SI_mrfileGetExecFullpath", 0x0842_f930);
pub const SI_MUTEX_CREATE: Routine = Routine::new("SI_mutexCreate", 0x0842_fbd0);
pub const SI_MUTEX_DESTROY: Routine = Routine::new("SI_mutexDestroy", 0x0842_fc24);
pub const SI_PIPEIO_CREATE: Routine = Routine::new("SI_pipeioCreate", 0x0842_fcb0);
pub const SI_PIPEIO_DESTROY: Routine = Routine::new("SI_pipeioDestroy", 0x0842_fcfc);
pub const SI_PIPEIO_RESET_INHERITANCE: Routine = Routine::new("SI_pipeioResetInheritance", 0x0842_fd30);
pub const SI_MRPROCESS_CREATE: Routine = Routine::new("SI_mrprocessCreate", 0x0842_ffbc);
pub const SI_MRPROCESS_DESTROY: Routine = Routine::new("SI_mrprocessDestroy", 0x0843_0138);
pub const SI_MRPROCESS_IS_ALIVE: Routine = Routine::new("SI_mrprocessIsAlive", 0x0843_0154);
pub const SI_MRTHREAD_CREATE: Routine = Routine::new("SI_mrthreadCreate", 0x0843_01b8);
pub const SI_MRTHREAD_IS_ALIVE: Routine = Routine::new("SI_mrthreadIsAlive", 0x0843_028c);
pub const GFLUSH: Routine = Routine::new("gflush", 0x0861_3094);
pub const WINGET: Routine = Routine::new("winget", 0x0861_9918);
pub const WINSET: Routine = Routine::new("winset", 0x0861_993c);
pub const SI_MENUWINDOW: Routine = Routine::new("SI_menuwindow", 0x0829_1028);

const PID_TRACER: u32 = 0x086d_2b58;
const OLD_WINDOW_ID: u32 = 0x086d_2960;
const MR_RENDER_ERROR: u32 = 0x086d_2b54; // 16 bits
const SI_BUSY_CURSOR: u32 = 0x0867_92b2; // 16 bits
const SI_CUS_STANDALONE: u32 = 0x086e_49f0; // 16 bits
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_PROGRAM_NAME: u32 = 0x0878_2060;
const STR_ERR_SEM: u32 = 0x086d_2aec;
const STR_STRIP_WHITE: u32 = 0x086d_296c;
const STR_ERR_TRACER2: u32 = 0x086d_29a8;
const STR_ERR_TRACER: u32 = 0x086d_2988;
const STR_ERR_EXEC2: u32 = 0x086d_2a2c;
const STR_ERR_EXEC: u32 = 0x086d_29ec;
const ENV_TRACER2: u32 = 0x086d_2b5c;
const ENV_TRACER: u32 = 0x086d_2b6a;
const MSGPROC: u32 = 0x0842_bd70;
const TRANSLATORPROC: u32 = 0x0842_c110;
const TILEPROC: u32 = 0x0842_c8c0;
const DISPLAY_MESSAGES: u32 = 0x0842_b75c;
const DETECT_MESSAGES: u32 = 0x0842_b7ec;

fn s16(m: &Mem, a: u32) -> i32 {
    m.u16(a) as i16 as i32
}

/// `while n < 300 && alive() { sleep(1); n += 1 }` of the waits (`alive` is one of the two liveness routines).
fn wait_dead(m: &mut Mem, o: &mut dyn Outside, alive: Routine, id: u32) {
    let mut n = 0;
    while n < 300 && o.call(m, alive, &[id]).eax as u16 != 0 {
        o.call(m, mr_messages::SLEEP, &[1]);
        n += 1;
    }
}

/// [soft 0x0842a8a0 SI_mentalRunTracer]: runs the translator and the external tracer for the parameter block `p`; `redo` (the second argument) selects
/// the pipe-only set-up. Answers 0 when it went well, 0x7f on failure, 0x83 when the user quit.
pub fn si_mental_run_tracer(m: &mut Mem, o: &mut dyn Outside, p: u32, mode: u32) -> u32 {
    let mode = mode as u16;
    const FRAME: u32 = 0x1c0;
    let fr = m.local(FRAME);
    let l = |n: u32| fr + FRAME - n; // the address of the original's local at ebp-n
    let r = run(m, o, p, mode, &l);
    m.release(FRAME);
    r
}

fn run(m: &mut Mem, o: &mut dyn Outside, p: u32, mode: u16, l: &dyn Fn(u32) -> u32) -> u32 {
    let mut result = 0u32; // local_1a4
    let mut quit = 0u16; // local_19e
    m.set_u32(PID_TRACER, u32::MAX);
    m.set_u16(MR_RENDER_ERROR, 0);
    if o.call(m, ext::SI_IS_IGLOO, &[]).eax as u16 != 0 && check_resources(m, o) as u16 == 0 {
        return 0x7f;
    }
    // the three thread parameter blocks (0x78 bytes each): messages at -0x7c, tiles at -0x16c, translator at -0xf4
    let (msg, tile, trn) = (l(0x7c), l(0x16c), l(0xf4));
    for b in [msg, tile, trn] {
        for i in 0..0x78 {
            m.set_u8(b + i, 0);
        }
    }
    let (mutex, pipe_a, pipe_b, pipe_c, pipe_d, pipe_e, pipe_f) = (l(0x188), l(0x184), l(0x180), l(0x17c), l(0x178), l(0x174), l(0x170));
    let (list, count, exec) = (l(0x190), l(0x18c), l(0x194));
    let prog = m.u32(SI_PROGRAM_NAME);
    m.set_u32(mutex, u32::MAX);
    m.set_u32(OLD_WINDOW_ID, u32::MAX);
    for a in [pipe_e, pipe_f, pipe_c, pipe_d, pipe_a, pipe_b] {
        m.set_u32(a, u32::MAX);
    }
    let tr = p + 0x62c;
    m.set_u16(p + 0x652, 1);
    m.set_u32(p + 0x668, prog);
    let env = if si_get_mi2_output(m) as u16 == 0 { ENV_TRACER } else { ENV_TRACER2 };
    let v = o.call(m, ext::GETENV, &[env]).eax;
    m.set_u32(p + 0x664, v);
    let flags: [(u32, u32); 7] = if mode == 0 {
        if m.u16(p + 0x184) == 0 {
            if m.u16(p + 0x3e4) == 0 {
                let last = if m.u16(p + 0x60e) == 0 { 1 } else { 0 };
                m.set_u16(p + 0x650, last);
                [(0x654, 2), (0x644, 1), (0x646, 0), (0x648, 0), (0x64a, 1), (0x64c, 1), (0x64e, 0)]
            } else {
                m.set_u16(p + 0x650, 0);
                [(0x654, 3), (0x644, 0), (0x646, 0), (0x648, 0), (0x64a, 1), (0x64c, 1), (0x64e, 0)]
            }
        } else {
            let w = m.u16(p + 0x5a4) as u32;
            m.set_u16(p + 0x650, 1);
            [(0x654, 1), (0x644, 1), (0x646, w), (0x648, 1), (0x64a, 1), (0x64c, 1), (0x64e, 0)]
        }
    } else {
        let w = m.u16(p + 0x5a4) as u32;
        m.set_u16(p + 0x650, 0);
        [(0x654, 0), (0x644, 1), (0x646, w), (0x648, 0), (0x64a, 0), (0x64c, 0), (0x64e, 1)]
    };
    for (off, v) in flags {
        if off == 0x654 {
            m.set_u32(p + off, v);
        } else {
            m.set_u16(p + off, v as u16);
        }
    }
    let busy = m.u16(SI_BUSY_CURSOR);
    m.set_u16(SI_BUSY_CURSOR, 0);
    let threaded = m.u16(p + 0x60e) == 0;
    if threaded {
        o.call(m, GFLUSH, &[]);
        let w = o.call(m, WINGET, &[]).eax;
        m.set_u32(OLD_WINDOW_ID, w);
        o.call(m, SI_MUTEX_CREATE, &[mutex]);
        if m.i32(mutex) < 0 {
            let t = m.u32(STR_ERR_SEM);
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[t, 3]);
            return 0x7f;
        }
        o.call(m, mr_messages::SI_MUTEX_LOCK, &[mutex]);
    }
    if m.u16(p + 0x644) != 0 {
        // the external tracer: its command line, the pipes and the threads that serve it
        m.set_u32(list, 0);
        m.set_u32(count, 0);
        let (str_, strip) = (m.u32(p + 0x664), m.u32(STR_STRIP_WHITE));
        if o.call(m, SI_STR_STRIP_STRING, &[str_, strip, list, count]).eax as u16 == 0 {
            let t = if si_get_mi2_output(m) as u16 != 0 { m.u32(STR_ERR_TRACER2) } else { m.u32(STR_ERR_TRACER) };
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[t, 3]);
            if m.u16(p + 0x60e) == 0 {
                o.call(m, SI_MUTEX_DESTROY, &[mutex]);
            }
            return 0x7f;
        }
        m.set_u32(exec, 0);
        let first = m.u32(m.u32(list));
        if o.call(m, SI_MRFILE_GET_EXEC_FULLPATH, &[first, exec]).eax as u16 == 0 {
            let t = if si_get_mi2_output(m) as u16 != 0 { m.u32(STR_ERR_EXEC2) } else { m.u32(STR_ERR_EXEC) };
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[t, 3]);
            let (lst, n) = (m.u32(list), m.u32(count));
            o.call(m, SI_STR_LIST_DESTROY, &[lst, n]);
            let e = m.u32(exec);
            if e != 0 {
                m.free(e);
                m.set_u32(exec, 0);
            }
            if m.u16(p + 0x60e) == 0 {
                o.call(m, SI_MUTEX_DESTROY, &[mutex]);
            }
            return 0x7f;
        }
        let arr = m.u32(list);
        let f = m.u32(arr);
        if f != 0 {
            m.free(f);
            m.set_u32(arr, 0);
        }
        let e = m.u32(exec);
        m.set_u32(arr, e);
        m.set_u32(exec, 0);
        o.call(m, SI_PIPEIO_CREATE, &[pipe_a, pipe_b]);
        o.call(m, SI_PIPEIO_RESET_INHERITANCE, &[pipe_b]);
        let standalone = m.u16(SI_CUS_STANDALONE) != 0;
        if m.u16(p + 0x60e) == 0 || standalone {
            o.call(m, SI_PIPEIO_CREATE, &[pipe_e, pipe_f]);
            o.call(m, SI_PIPEIO_RESET_INHERITANCE, &[pipe_e]);
            m.set_u32(msg + 0x14, m.u32(pipe_e));
            m.set_u32(msg + 0x18, m.u32(mutex));
            m.set_u32(msg + 0x24, if m.u16(p + 0x60e) != 0 { DETECT_MESSAGES } else { DISPLAY_MESSAGES });
            m.set_u32(msg, 0);
            m.set_u16(msg + 0x20, 1);
            let pd = m.u32(SI_PROC_DATA);
            for k in 0..3 {
                m.set_u32(msg + 8 + 4 * k, m.u32(pd + 4 * k));
            }
            let id = o.call(m, SI_MRTHREAD_CREATE, &[MSGPROC, msg, u32::MAX]).eax;
            m.set_u32(msg + 4, id);
        }
        if m.u16(p + 0x646) != 0 {
            o.call(m, SI_PIPEIO_CREATE, &[pipe_c, pipe_d]);
            o.call(m, SI_PIPEIO_RESET_INHERITANCE, &[pipe_c]);
            compute_window_size_pos(m, p + 0xc, tile);
            m.set_u32(tile + 0x14, m.u32(pipe_c));
            m.set_u32(tile + 0x18, m.u32(mutex));
            m.set_u32(tile, 0);
            m.set_u32(tile + 0x34, u32::MAX);
            m.set_u16(tile + 0x50, 1);
            m.set_u16(tile + 0x52, m.u16(p + 0x5bc));
            m.set_u16(tile + 0x56, m.u16(p + 0x64c));
            m.set_u16(tile + 0x58, m.u16(p + 0x648));
            let pd = m.u32(SI_PROC_DATA);
            for k in 0..3 {
                m.set_u32(tile + 8 + 4 * k, m.u32(pd + 4 * k));
            }
            m.set_u16(tile + 0x20, 1);
            m.set_u32(tile + 0x6c, s16(m, p + 0x200) as u32);
            m.set_u32(tile + 0x70, s16(m, p + 0x202) as u32);
            m.set_u32(tile + 0x74, s16(m, p + 0x204) as u32);
            m.set_u32(tile + 0x64, s16(m, p + 0x200) as u32);
            if m.u16(p + 0x148) != 0 {
                m.set_u32(tile + 0x68, (s16(m, p + 0x14a) + 1) as u32);
            }
            if o.call(m, ext::SI_IS_IGLOO, &[]).eax as u16 != 0 {
                o.call(m, SI_MRWIN_OPEN, &[tile]);
            }
            let id = o.call(m, SI_MRTHREAD_CREATE, &[TILEPROC, tile, u32::MAX]).eax;
            m.set_u32(tile + 4, id);
        }
        mr_cmdline::si_mrfile_build_cmdline(m, o, p + 0x670, s16(m, p + 0x646) as u32, m.u32(pipe_d), list, count);
        o.call(m, SI_STR_LIST_APPEND, &[0, list, count]);
        let (lst, n, a, b, c) = (m.u32(list), m.u32(count), m.u32(pipe_a), m.u32(pipe_f), m.u32(pipe_d));
        let pid = o.call(m, SI_MRPROCESS_CREATE, &[lst, n, a, b, c]).eax;
        m.set_u32(PID_TRACER, pid);
        if (pid as i32) < 0 {
            if m.u16(p + 0x60e) == 0 {
                o.call(m, SI_MUTEX_DESTROY, &[mutex]);
            }
            return 0x7f;
        }
        let (lst, n) = (m.u32(list), m.u32(count));
        o.call(m, SI_STR_LIST_DESTROY, &[lst, n]);
        o.call(m, SI_PIPEIO_DESTROY, &[pipe_a]);
        if m.u16(p + 0x60e) == 0 || standalone {
            o.call(m, SI_PIPEIO_DESTROY, &[pipe_f]);
        }
        if m.u16(p + 0x646) != 0 {
            o.call(m, SI_PIPEIO_DESTROY, &[pipe_d]);
        }
    }
    // LAB_0842b08a: hand the pipes and the process data to the translator
    m.set_u32(p + 0x630, m.u32(mutex));
    m.set_u16(p + 0x640, 0);
    m.set_u16(p + 0x642, 0);
    m.set_u32(tr, m.u32(pipe_b));
    let pd = m.u32(SI_PROC_DATA);
    for k in 0..3 {
        m.set_u32(p + 0x634 + 4 * k, m.u32(pd + 4 * k));
    }
    if threaded {
        m.set_u32(trn + 0x14, m.u32(pipe_b));
        for k in 0..3 {
            m.set_u32(trn + 8 + 4 * k, m.u32(pd + 4 * k));
        }
        m.set_u32(trn + 0x18, m.u32(mutex));
        m.set_u32(trn, 0);
        m.set_u16(trn + 0x20, 1);
        let id = o.call(m, SI_MRTHREAD_CREATE, &[TRANSLATORPROC, p, u32::MAX]).eax;
        m.set_u32(trn + 4, id);
        o.call(m, mr_messages::SI_MUTEX_UNLOCK, &[mutex]);
        check_user_quit(m, o, mutex);
        loop {
            let tracer = m.u32(PID_TRACER);
            let (tid, mid, trid) = (m.u32(tile + 4), m.u32(msg + 4), m.u32(trn + 4));
            let outcome: Option<u32> = 'chk: {
                if m.u16(p + 0x644) != 0 && o.call(m, SI_MRPROCESS_IS_ALIVE, &[tracer]).eax as u16 == 0 {
                    break 'chk Some(0);
                }
                if m.u16(p + 0x646) != 0 && o.call(m, SI_MRTHREAD_IS_ALIVE, &[tid]).eax as u16 == 0 {
                    break 'chk Some(0x7f);
                }
                if (mid as i32) > 0 && m.u16(p + 0x60e) == 0 && o.call(m, SI_MRTHREAD_IS_ALIVE, &[mid]).eax as u16 == 0 {
                    break 'chk Some(0x7f);
                }
                if o.call(m, SI_MRTHREAD_IS_ALIVE, &[trid]).eax as u16 == 0 {
                    if m.u16(p + 0x644) == 0 {
                        break 'chk Some(0);
                    }
                    if m.u32(p + 8) != 0 {
                        break 'chk Some(0x7f);
                    }
                }
                quit = check_user_quit(m, o, mutex) as u16;
                if quit != 0 {
                    break 'chk Some(0x83);
                }
                None
            };
            if let Some(v) = outcome {
                result = v;
                break;
            }
            o.call(m, USLEEP, &[200000]);
        }
        // LAB_0842b3e5: stop everything and wait for it
        m.set_u16(p + 0x642, 1);
        m.set_u16(tile + 0x1e, 1);
        m.set_u16(msg + 0x1e, 1);
        m.set_u16(trn + 0x1e, 1);
        let (tid, mid, trid) = (m.u32(tile + 4), m.u32(msg + 4), m.u32(trn + 4));
        if m.u16(p + 0x644) != 0 {
            o.call(m, SI_MRPROCESS_DESTROY, &[PID_TRACER]);
            if m.u16(p + 0x644) != 0 {
                let mut n = 0;
                while n < 300 && o.call(m, SI_MRPROCESS_IS_ALIVE, &[m.u32(PID_TRACER)]).eax as u16 != 0 {
                    o.call(m, mr_messages::SLEEP, &[1]);
                    n += 1;
                }
            }
        }
        if m.u16(p + 0x646) != 0 {
            wait_dead(m, o, SI_MRTHREAD_IS_ALIVE, tid);
        }
        if m.u16(p + 0x644) != 0 && m.u16(p + 0x60e) == 0 {
            wait_dead(m, o, SI_MRTHREAD_IS_ALIVE, mid);
        }
        wait_dead(m, o, SI_MRTHREAD_IS_ALIVE, trid);
        o.call(m, SI_PIPEIO_DESTROY, &[tr]);
        o.call(m, SI_PIPEIO_DESTROY, &[pipe_c]);
        o.call(m, SI_PIPEIO_DESTROY, &[pipe_e]);
        o.call(m, SI_MRTILE_PROC_CLEANUP, &[tile]);
        mr_messages::si_mrmsgproc_cleanup(m, msg);
        mr_translator::si_mrtranslatorproc_cleanup(m, o, tr);
        let b = m.u32(p + 0x670);
        if b != 0 {
            m.free(b);
            m.set_u32(p + 0x670, 0);
        }
        if quit == 0 && m.u16(p + 0x64e) != 0 && s16(m, tile + 0x34) > 0 {
            let w = m.u32(tile + 0x34);
            o.call(m, WINSET, &[w]);
            wait_after_done(m, o, m.u16(p + 0x5bc) as u32);
        }
        if m.u16(MR_RENDER_ERROR) != 0 {
            result = 0x7f;
            m.set_u16(MR_RENDER_ERROR, 0);
        }
        if m.u16(p + 0x646) == 1 {
            o.call(m, SI_MRWIN_CLOSE, &[p, tile]);
        }
        o.call(m, SI_MUTEX_DESTROY, &[mutex]);
        m.set_u16(SI_BUSY_CURSOR, busy);
        o.call(m, SI_MENUWINDOW, &[0]);
        return result;
    }
    // no thread storage: the translator runs here
    mr_translator::si_mrtranslatorproc(m, o, p);
    if m.u16(p + 0x644) != 0 {
        let b = m.u32(p + 0x670);
        if b != 0 {
            m.free(b);
            m.set_u32(p + 0x670, 0);
        }
        o.call(m, SI_PIPEIO_DESTROY, &[tr]);
        while o.call(m, SI_MRPROCESS_IS_ALIVE, &[m.u32(PID_TRACER)]).eax as u16 != 0 {
            let mid = m.u32(msg + 4);
            if m.u16(SI_CUS_STANDALONE) != 0 && (mid as i32) > 0 && o.call(m, SI_MRTHREAD_IS_ALIVE, &[mid]).eax as u16 == 0 {
                result = 0x7f;
                break;
            }
            o.call(m, mr_messages::SLEEP, &[1]);
        }
    }
    if m.u16(SI_CUS_STANDALONE) != 0 {
        m.set_u16(p + 0x642, 1);
        m.set_u16(msg + 0x1e, 1);
        if m.u16(p + 0x644) != 0 {
            wait_dead_process(m, o);
            if m.u16(p + 0x644) != 0 {
                let mid = m.u32(msg + 4);
                wait_dead(m, o, SI_MRTHREAD_IS_ALIVE, mid);
            }
        }
        mr_messages::si_mrmsgproc_cleanup(m, msg);
    }
    mr_translator::si_mrtranslatorproc_cleanup(m, o, tr);
    if m.u16(SI_CUS_STANDALONE) != 0 {
        if m.u16(MR_RENDER_ERROR) != 0 {
            result = 0x7f;
            m.set_u16(MR_RENDER_ERROR, 0);
        }
        if result != 0 && m.u32(p + 8) == 0 {
            m.set_u32(p + 8, 8);
        }
    }
    if m.u32(p + 8) != 0 {
        0x7f
    } else {
        0
    }
}

fn wait_dead_process(m: &mut Mem, o: &mut dyn Outside) {
    let mut n = 0;
    while n < 300 && o.call(m, SI_MRPROCESS_IS_ALIVE, &[m.u32(PID_TRACER)]).eax as u16 != 0 {
        o.call(m, mr_messages::SLEEP, &[1]);
        n += 1;
    }
}
