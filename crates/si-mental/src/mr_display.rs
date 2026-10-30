// SPDX-License-Identifier: MIT
//! `mr_display.c`: the thread that reads picture tiles from the external mental ray's pipe and shows them (package 62, finding 0029/0042).
//! The thread's parameter block (`SI_mrthreadParms`): +0x00 result, +0x14 pipe, +0x18 mutex, +0x1c "mutex held", +0x1e stop flag, +0x20 "catch
//! signals", +0x28 tile buffer, +0x2c its capacity, +0x34 window, +0x38/+0x3c window origin (u16 each), +0x50 "show tiles", +0x56 "show frame count",
//! +0x58 "ghost frame", +0x5c second buffer, +0x64 frame counter, +0x68 field state, +0x74 frame step.
//! A tile is an `SI_Rect` of four shorts (x0, y0, x1, y1).
//!
//! Not ported (IRIS GL window drawing, no behaviour of the renderer): ghostFrame (0x0842d068), showFrameCount (0x0842ce90),
//! si_mrwinOpen (0x0842d210), si_mrwinClose (0x0842d370); the port calls them through `Outside`.

use crate::mr_messages::{ALARM, LONGJMP, SETJMP, SIGNAL, SI_MRTHREAD_EXIT, SI_MUTEX_LOCK, SI_MUTEX_UNLOCK, SI_PIPEIO_READ, SLEEP};
use crate::mi_mr_main::{si_malloc_blk, ext};
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_MRWIN_OPEN: Routine = Routine::new("si_mrwinOpen", 0x0842_d210);
pub const SHOW_FRAME_COUNT: Routine = Routine::new("showFrameCount", 0x0842_ce90);
pub const GHOST_FRAME: Routine = Routine::new("ghostFrame", 0x0842_d068);
pub const WINSET: Routine = Routine::new("winset", 0x0861_993c);
pub const LRECTWRITE: Routine = Routine::new("lrectwrite", 0x0861_1a50);
pub const GFLUSH: Routine = Routine::new("gflush", 0x0861_3094);
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
/// The jump buffer of this thread (`jmpError`), the address of `handler`, the text `strLostSynch` and the `stderr` pointer.
const JMP_ERROR: u32 = 0x0881_a660;
const HANDLER: u32 = 0x0842_c9c0;
const STR_LOST_SYNCH: u32 = 0x086d_2dd8;
const STDERR: u32 = 0x0878_0c88;

/// [soft 0x0842cd7c swapLongArray]: reverses the bytes of each of `n` words.
pub fn swap_long_array(m: &mut Mem, p: u32, n: i32) {
    let mut i = 0;
    while i < n {
        let a = p.wrapping_add((i as u32).wrapping_mul(4));
        let v = m.u32(a);
        m.set_u32(a, v.swap_bytes());
        i += 1;
    }
}

/// [soft 0x0842cb9c updateFrame]: field state `*state` 0: `*count += step`; 1 becomes 2 (no step); 2 becomes 1 and steps; anything else is left alone.
pub fn update_frame(m: &mut Mem, count: u32, state: u32, step: i32) {
    match m.i32(state) {
        1 => m.set_i32(state, 2),
        2 => {
            m.set_i32(state, 1);
            let v = m.i32(count).wrapping_add(step);
            m.set_i32(count, v);
        }
        0 => {
            let v = m.i32(count).wrapping_add(step);
            m.set_i32(count, v);
        }
        _ => {}
    }
}

/// [soft 0x0842cbd8 pipeReadTileHeader]: reads the 20 byte header (five big-endian words) into a local buffer, a read at a time. 3 when a read fails,
/// 2 when the stop flag (`stop`, u16) is set after a read (after a one second sleep when nothing came), else 0 with `*kind` = word 0 and the tile
/// rectangle at `rect` (x0 = word 1, y0 = word 3, x1 = word 2, y1 = word 4).
pub fn pipe_read_tile_header(m: &mut Mem, o: &mut dyn Outside, pipe: u32, stop: u32, kind: u32, rect: u32) -> u32 {
    let got = m.local(4);
    let hdr = m.local(0x14);
    m.set_u32(got, 0);
    let mut left: i32 = 0x14;
    let mut off: u32 = 0;
    loop {
        let r = o.call(m, SI_PIPEIO_READ, &[pipe, hdr.wrapping_add(off), left as u32, got]).eax;
        if r != 0 {
            m.release(0x18);
            return 3;
        }
        let n = m.i32(got);
        if n == 0 {
            o.call(m, SLEEP, &[1]);
        }
        if m.u16(stop) != 0 {
            m.release(0x18);
            return 2;
        }
        left = left.wrapping_sub(n);
        off = off.wrapping_add(n as u32);
        if left <= 0 {
            break;
        }
    }
    swap_long_array(m, hdr, 5);
    let w: Vec<u32> = (0..5).map(|i| m.u32(hdr + 4 * i)).collect();
    m.set_u32(kind, w[0]);
    m.set_u16(rect, w[1] as u16);
    m.set_u16(rect + 4, w[2] as u16);
    m.set_u16(rect + 2, w[3] as u16);
    m.set_u16(rect + 6, w[4] as u16);
    m.release(0x18);
    0
}

/// [soft 0x0842cc8c pipeReadTileData]: reads the pixels of the tile `rect` (4 bytes each) into `*buf`, growing the block to `*cap` first (the old block
/// freed; 7 when the allocation fails). 3 when a read fails, 2 when the stop flag is set after a read, else 0. The block is cleared before the read.
pub fn pipe_read_tile_data(m: &mut Mem, o: &mut dyn Outside, pipe: u32, stop: u32, rect: u32, buf: u32, cap: u32) -> u32 {
    let w = (m.i16(rect + 4) as i32).wrapping_sub(m.i16(rect) as i32).wrapping_add(1);
    let h = (m.i16(rect + 6) as i32).wrapping_sub(m.i16(rect + 2) as i32).wrapping_add(1);
    let size = w.wrapping_mul(h).wrapping_mul(4);
    if size > m.i32(cap) {
        let p = si_malloc_blk(m, size as u32);
        if p == 0 {
            return 7;
        }
        let old = m.u32(buf);
        if old != 0 {
            m.free(old);
            m.set_u32(buf, 0);
        }
        m.set_u32(buf, p);
        m.set_i32(cap, size);
    }
    let dst = m.u32(buf);
    clib::memset(m, dst, 0, size as u32);
    let got = m.local(4);
    let mut left = size;
    let mut off: u32 = 0;
    let mut r = 0;
    while left > 0 {
        let rr = o.call(m, SI_PIPEIO_READ, &[pipe, dst.wrapping_add(off), left as u32, got]).eax;
        if rr != 0 {
            r = 3;
            break;
        }
        let n = m.i32(got);
        if n == 0 {
            o.call(m, SLEEP, &[1]);
        }
        if m.u16(stop) != 0 {
            r = 2;
            break;
        }
        left = left.wrapping_sub(n);
        off = off.wrapping_add(n as u32);
    }
    m.release(4);
    r
}

/// [soft 0x0842cdb4 tileDisplay]: draws the tile `rect` (given as the two packed words x0|y0<<16 and x1|y1<<16) moved by the window origin
/// (+0x38, +0x3c), under the mutex, with `lrectwrite` into the window +0x34; the window is reset to 0 afterwards when `si_IsIGLOO`.
pub fn tile_display(m: &mut Mem, o: &mut dyn Outside, p: u32, d0: u32, d1: u32, pix: u32) {
    let ox = m.u16(p + 0x38) as u32;
    let oy = m.u16(p + 0x3c) as u32;
    // the original adds in 32 bits and keeps the low halves of the sums
    let x0 = (d0.wrapping_add(ox) & 0xffff) as u16;
    let y0 = ((d0 as i32 >> 16).wrapping_add(oy as i32)) as u16;
    let x1 = (d1.wrapping_add(ox) & 0xffff) as u16;
    let y1 = ((d1 as i32 >> 16).wrapping_add(oy as i32)) as u16;
    o.call(m, SI_MUTEX_LOCK, &[p + 0x18]);
    m.set_u16(p + 0x1c, 1);
    let win = m.u32(p + 0x34);
    o.call(m, WINSET, &[win]);
    o.call(m, LRECTWRITE, &[x0 as i16 as i32 as u32, y0 as i16 as i32 as u32, x1 as i16 as i32 as u32, y1 as i16 as i32 as u32, pix]);
    o.call(m, GFLUSH, &[]);
    if o.call(m, ext::SI_IS_IGLOO, &[]).eax as u16 != 0 {
        o.call(m, WINSET, &[0]);
    }
    o.call(m, SI_MUTEX_UNLOCK, &[p + 0x18]);
    m.set_u16(p + 0x1c, 0);
}

/// [soft 0x0842c9d8 si_mrtileProc_Protected]: reads tiles until the stop flag is set or a read fails (the result word is the last read's answer, 2 when
/// stopped, 4 for an unknown message). Message 2 is a tile (opens the window on the first, shows it), 4 a new frame (counted), 3 and 5 are skipped.
pub fn si_mrtileproc_protected(m: &mut Mem, o: &mut dyn Outside, p: u32) {
    o.call(m, ALARM, &[0]);
    let kind = m.local(4);
    let rect = m.local(8);
    let mut new_frame = false;
    let mut opened = false;
    let r = 'run: loop {
        let r = pipe_read_tile_header(m, o, m.u32(p + 0x14), p + 0x1e, kind, rect);
        m.set_u32(p, r);
        if m.u16(p + 0x1e) != 0 {
            break 'run 2;
        }
        if r != 0 {
            break 'run r;
        }
        match m.i32(kind) {
            2 => {
                let r = pipe_read_tile_data(m, o, m.u32(p + 0x14), p + 0x1e, rect, p + 0x28, p + 0x2c);
                m.set_u32(p, r);
                if m.u16(p + 0x1e) != 0 {
                    break 'run 2;
                }
                if r != 0 {
                    break 'run r;
                }
                if !opened && m.u16(p + 0x50) != 0 {
                    opened = true;
                    if o.call(m, ext::SI_IS_IGLOO, &[]).eax as u16 == 0 {
                        o.call(m, SI_MRWIN_OPEN, &[p]);
                    }
                    if m.u16(p + 0x56) != 0 {
                        o.call(m, SHOW_FRAME_COUNT, &[p]);
                    }
                }
                if new_frame {
                    new_frame = false;
                    if m.u16(p + 0x58) != 0 && m.u16(p + 0x50) != 0 {
                        o.call(m, GHOST_FRAME, &[p]);
                    }
                    if m.u16(p + 0x56) != 0 && m.u16(p + 0x50) != 0 {
                        o.call(m, SHOW_FRAME_COUNT, &[p]);
                    }
                }
                let buf = m.u32(p + 0x28);
                if m.u16(p + 0x50) != 0 {
                    let (d0, d1) = (m.u32(rect), m.u32(rect + 4));
                    tile_display(m, o, p, d0, d1, buf);
                }
            }
            3 | 5 => {}
            4 => {
                update_frame(m, p + 0x64, p + 0x68, m.i32(p + 0x74));
                new_frame = true;
            }
            k => {
                let a = [m.i16(rect) as i32 as u32, m.i16(rect + 4) as i32 as u32, m.i16(rect + 6) as i32 as u32, m.i16(rect + 2) as i32 as u32];
                let err = m.u32(STDERR);
                o.call(m, FPRINTF, &[err, m.u32(STR_LOST_SYNCH), k as u32, a[0], a[1], a[2], a[3]]);
                m.set_u32(p, 4);
                break 'run 4;
            }
        }
        if m.u16(p + 0x1e) != 0 {
            break 'run 2;
        }
    };
    m.set_u32(p, r);
    m.release(12);
}

/// [soft 0x0842c8c0 si_mrtileProc]: the thread's body, as `si_mrmsgProc` (result 6 after a signal). A null block makes the original write to
/// address 8 and fault; here it only leaves the thread.
pub fn si_mrtileproc(m: &mut Mem, o: &mut dyn Outside, p: u32) {
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
        o.call(m, SI_MRTHREAD_EXIT, &[0]);
    }
    if m.u16(p + 0x20) != 0 {
        o.call(m, SIGNAL, &[7, HANDLER]);
        o.call(m, SIGNAL, &[0xb, HANDLER]);
    }
    si_mrtileproc_protected(m, o, p);
    if m.u16(p + 0x20) != 0 {
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
    }
    o.call(m, SI_MRTHREAD_EXIT, &[0]);
}

/// [soft 0x0842c9c0 handler]: the signal handler of the tile thread: jumps back to `si_mrtileProc` with 1.
pub fn handler(m: &mut Mem, o: &mut dyn Outside, _sig: u32) {
    o.call(m, LONGJMP, &[JMP_ERROR, 1]);
}

/// [soft 0x0842d520 si_mrtileProcCleanup]: frees the blocks at +0x5c and +0x28.
pub fn si_mrtileproc_cleanup(m: &mut Mem, p: u32) {
    for off in [0x5c, 0x28] {
        let b = m.u32(p + off);
        if b != 0 {
            m.free(b);
            m.set_u32(p + off, 0);
        }
    }
}
