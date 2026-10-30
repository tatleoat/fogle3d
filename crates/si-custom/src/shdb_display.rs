// SPDX-License-Identifier: MIT
//! `shdb_display.c`: the preview window's pipe reader (finding 0041, unit shdb7). The tile reader and the
//! compose step are ported; the drawing routines (`si_shdbDisplayUpdate`, `si_shdbDisplayPopup`,
//! `displayCursor`, `eventHandler`: GL `lrectwrite`, `winpop`, `RGBcolor`) are not, and `handler` ends in a
//! `longjmp`. Not cited as checked: `pipeReadTileData`, `si_shdbDisplayProc_Protected`, `si_shdbDisplayProc` (their data
//! reads go to a buffer that is no local of the routine, which the check kit's `NAME.out` replay cannot deliver).
//!
//! The thread record (`si_shdbThreadParms`): +0 the state word (2 done, 3 pipe error, 6 fault, 7 no memory),
//! +0x14 the read end of the pipe, +0x22 the stop flag (a short), +0x24 the "catch signals" flag (a short),
//! +0x38 the window id, +0x40 / +0x44 the window position, +0x48 / +0x4c the picture size (width, height),
//! +0x50 the tile buffer, +0x54 the picture buffer, +0x58 the last row set.

use crate::shdb_hooks::S_TEMP;
use si_mem::{clib, Mem, Outside, Routine};

pub const ALARM: Routine = Routine::new("plt:alarm", 0x080c_5108);
pub const SIGNAL: Routine = Routine::new("plt:signal", 0x080c_3bd8);
pub const SETJMP: Routine = Routine::new("plt:_setjmp", 0x080c_4e48);
pub const SLEEP: Routine = Routine::new("plt:sleep", 0x080c_4468);
pub const SI_PIPEIO_READ: Routine = Routine::new("SI_pipeioRead", 0x0842_fd90);
pub const SI_UI_OPEN_WINDOW: Routine = Routine::new("SI_uiOpenWindow", 0x0839_2400);
pub const SI_UI_CLOSE_WINDOW: Routine = Routine::new("SI_uiCloseWindow", 0x0839_250c);

/// `scan`: the row buffer of `composeImgBuffer` (a pointer, one row of 4-byte pixels).
pub const SCAN: u32 = 0x086e_550c;
/// `szShaderball`: the pointer to the window's title.
pub const SZ_SHADERBALL: u32 = 0x086e_5508;
/// `jmpError` of this file: the jump buffer the signal handler returns to.
pub const JMP_ERROR: u32 = 0x0882_4760;
/// The signal handler (`handler`, 0x0854e254) and the event handler (0x0854e804) as the addresses the original hands out.
pub const HANDLER: u32 = 0x0854_e254;
pub const EVENT_HANDLER: u32 = 0x0854_e804;

/// [soft 0x0854e3a4 pipeReadTileHeader]: reads the 20-byte tile header from the pipe (a count and four shorts)
/// in as many reads as it takes, sleeping a second after an empty read. Answers 3 on a read error, 2 when the
/// stop flag (a short at `stop`) is set after a read, else 0 with the count stored at `scale` and the four
/// shorts at `rect` (+0, +2, +4, +6).
pub fn pipe_read_tile_header(m: &mut Mem, o: &mut dyn Outside, fd: u32, stop: u32, scale: u32, rect: u32) -> u32 {
    let buf = m.local(20);
    let cnt = m.local(4);
    let mut left: i32 = 0x14;
    let mut off: u32 = 0;
    loop {
        m.set_u32(cnt, 0);
        let r = o.call(m, SI_PIPEIO_READ, &[fd, buf.wrapping_add(off), left as u32, cnt]).eax;
        if r != 0 {
            return 3;
        }
        if m.u32(cnt) == 0 {
            o.call(m, SLEEP, &[1]);
        }
        if m.u16(stop) != 0 {
            return 2;
        }
        let got = m.u32(cnt);
        left = left.wrapping_sub(got as i32);
        off = off.wrapping_add(got);
        if left < 1 {
            let n = m.u32(buf);
            m.set_u32(scale, n);
            for i in 0..4u32 {
                let w = m.u32(buf + 4 + 4 * i);
                m.set_u16(rect.wrapping_add(2 * i), w as u16);
            }
            return 0;
        }
    }
}

/// pipeReadTileData (0x0854e450): reads one row of the tile, `(x1 - x0) * 4 + 4` bytes (shorts at +0 and +4 of
/// `rect`), into `dst` in as many reads as it takes; the same answers as the header reader (3, 2, 0).
pub fn pipe_read_tile_data(m: &mut Mem, o: &mut dyn Outside, fd: u32, stop: u32, rect: u32, dst: u32) -> u32 {
    let cnt = m.local(4);
    let x0 = m.i16(rect) as i32;
    let x1 = m.i16(rect.wrapping_add(4)) as i32;
    let mut left: i32 = x1.wrapping_sub(x0).wrapping_mul(4).wrapping_add(4);
    let mut off: u32 = 0;
    while left > 0 {
        m.set_u32(cnt, 0);
        let r = o.call(m, SI_PIPEIO_READ, &[fd, dst.wrapping_add(off), left as u32, cnt]).eax;
        if r != 0 {
            return 3;
        }
        if m.u32(cnt) == 0 {
            o.call(m, SLEEP, &[1]);
        }
        if m.u16(stop) != 0 {
            return 2;
        }
        let got = m.u32(cnt);
        left = left.wrapping_sub(got as i32);
        off = off.wrapping_add(got);
    }
    0
}

/// [soft 0x0854e4dc composeImgBuffer]: copies a tile into the picture. `r0` holds x0 (low short) and y0 (high), `r1`
/// x1 and y1 (the rectangle by value, two words). For each row y0 ..= y1 + scale: clears the row buffer (`scan`, `4 * width`
/// bytes, allocated on first use; nothing happens when that fails), writes for each x of x0 ..= x1 `scale` copies of the
/// tile word x - x0, and moves the row buffer to the picture's row y (`dst + 4 * (y * width)`). The row buffer
/// overruns when the copies exceed `width` words, as in the original.
pub fn compose_img_buffer(m: &mut Mem, dst: u32, src: u32, width: u32, r0: u32, r1: u32, scale: i32) {
    let n = width.wrapping_shl(2);
    if m.u32(SCAN) == 0 {
        let p = m.si_malloc(n);
        m.set_u32(SCAN, p);
        if p == 0 {
            return;
        }
    }
    let mut y = (r0 as i32) >> 16;
    let y_last = ((r1 as i32) >> 16).wrapping_add(scale);
    if y > y_last {
        return;
    }
    let x0 = r0 as u16 as i16 as i32;
    let x1 = r1 as u16 as i16 as i32;
    loop {
        let scan = m.u32(SCAN);
        clib::memset(m, scan, 0, n);
        let row = (y as u32).wrapping_mul(width);
        y = y.wrapping_add(1);
        let mut p = scan;
        let mut x = x0;
        while x <= x1 {
            let mut k = scale;
            while k > 0 {
                let w = m.u32(src.wrapping_add((x.wrapping_sub(x0) as u32).wrapping_shl(2)));
                m.set_u32(p, w);
                p = p.wrapping_add(4);
                k -= 1;
            }
            x += 1;
        }
        let scan = m.u32(SCAN);
        clib::memmove(m, dst.wrapping_add(row.wrapping_shl(2)), scan, n);
        if y > y_last {
            break;
        }
    }
}

/// si_shdbDisplayProc_Protected (0x0854e26c): the reader thread's body. Frees the row buffer, sizes the two
/// picture buffers `max(4 * (w + 1) * (h + 1), 0x4000)` (a `SI_calloc` each when the record has none; state 7 when
/// one fails), then reads tiles until the stop flag is set: header, row, `composeImgBuffer`, sets the third short of
/// `s_temp` to 1 and records the tile's top row at +0x58. A read error ends with its code in the state word, the stop
/// flag with state 2.
pub fn si_shdb_display_proc_protected(m: &mut Mem, o: &mut dyn Outside, t: u32) {
    o.call(m, ALARM, &[0]);
    let scan = m.u32(SCAN);
    if scan != 0 {
        m.free(scan);
        m.set_u32(SCAN, 0);
    }
    let w = m.u32(t + 0x48).wrapping_add(1);
    let h = m.u32(t + 0x4c).wrapping_add(1);
    let mut size = w.wrapping_mul(h).wrapping_shl(2);
    if (size as i32) < 0x4000 {
        size = 0x4000;
    }
    if m.u32(t + 0x50) == 0 {
        let p = m.si_calloc(size, 1);
        m.set_u32(t + 0x50, p);
        if p == 0 {
            m.set_u32(t, 7);
            return;
        }
    }
    if m.u32(t + 0x54) == 0 {
        let p = m.si_calloc(size, 1);
        m.set_u32(t + 0x54, p);
        if p == 0 {
            m.set_u32(t, 7);
            return;
        }
    }
    m.set_u32(t + 0x58, 0);
    let loc = m.local(0x10);
    let (scale, rect) = (loc, loc + 4);
    loop {
        let fd = m.u32(t + 0x14);
        let r = pipe_read_tile_header(m, o, fd, t + 0x22, scale, rect);
        m.set_u32(t, r);
        if r != 0 {
            return;
        }
        if m.u16(t + 0x22) != 0 {
            break;
        }
        let (fd, dst) = (m.u32(t + 0x14), m.u32(t + 0x50));
        let r = pipe_read_tile_data(m, o, fd, t + 0x22, rect, dst);
        m.set_u32(t, r);
        if r != 0 {
            return;
        }
        if m.u16(t + 0x22) != 0 {
            break;
        }
        let (dst, src, width) = (m.u32(t + 0x54), m.u32(t + 0x50), m.u32(t + 0x48));
        let (r0, r1, sc) = (m.u32(rect), m.u32(rect + 4), m.u32(scale));
        compose_img_buffer(m, dst, src, width, r0, r1, sc as i32);
        let tmp = m.u32(S_TEMP);
        m.set_u16(tmp.wrapping_add(4), 1);
        let y0 = m.i16(rect + 2) as i32;
        m.set_u32(t + 0x58, y0 as u32);
        if m.u16(t + 0x22) != 0 {
            break;
        }
    }
    m.set_u32(t, 2);
}

/// si_shdbDisplayProc (0x0854e190): the thread entry. A null record writes 8 to address 0 (the original means
/// the record's state word; it faults, so no case reaches it). Otherwise `alarm(0)`, `_setjmp` on `jmpError`: the first
/// return installs the fault handler for signals 7 and 11 when the record's flag at +0x24 is set, runs the body and
/// removes the handlers; a second return (a fault) sets state 6 and removes them.
pub fn si_shdb_display_proc(m: &mut Mem, o: &mut dyn Outside, t: u32) {
    if t == 0 {
        // the original stores 8 at address 0 (a fault); the Rust writes there too and the image records it
        m.set_u32(0, 8);
        return;
    }
    o.call(m, ALARM, &[0]);
    let r = o.call(m, SETJMP, &[JMP_ERROR]).eax;
    if r == 0 {
        if m.u16(t + 0x24) != 0 {
            o.call(m, SIGNAL, &[7, HANDLER]);
            o.call(m, SIGNAL, &[0xb, HANDLER]);
        }
        si_shdb_display_proc_protected(m, o, t);
        if m.u16(t + 0x24) != 0 {
            o.call(m, SIGNAL, &[7, 0]);
            o.call(m, SIGNAL, &[0xb, 0]);
        }
    } else {
        m.set_u32(t, 6);
        o.call(m, SIGNAL, &[7, 0]);
        o.call(m, SIGNAL, &[0xb, 0]);
    }
}

/// [soft 0x0854e5dc si_shdbDisplayCreate]: opens the preview window (title `szShaderball`, position +0x40 / +0x44, the size
/// fields +0x48 / +0x4c by address, the record and the event handler as callback and user data) and keeps its id at +0x38.
/// Answers 1.
pub fn si_shdb_display_create(m: &mut Mem, o: &mut dyn Outside, t: u32) -> u32 {
    let title = m.u32(SZ_SHADERBALL);
    let (x, y) = (m.u32(t + 0x40), m.u32(t + 0x44));
    let id = o.call(m, SI_UI_OPEN_WINDOW, &[title, x, y, t + 0x48, t + 0x4c, t, EVENT_HANDLER]).eax;
    m.set_u32(t + 0x38, id);
    1
}

/// [soft 0x0854e61c si_shdbDisplayClose]: closes the window when the id at +0x38 is not negative and sets it to -1.
pub fn si_shdb_display_close(m: &mut Mem, o: &mut dyn Outside, t: u32) {
    let id = m.u32(t + 0x38);
    if (id as i32) >= 0 {
        o.call(m, SI_UI_CLOSE_WINDOW, &[id]);
        m.set_u32(t + 0x38, 0xffff_ffff);
    }
}

/// [soft 0x0854e838 si_shdbDisplayProcCleanup]: frees the tile buffer, the picture buffer and the row buffer.
pub fn si_shdb_display_proc_cleanup(m: &mut Mem, t: u32) {
    for off in [0x50u32, 0x54] {
        let p = m.u32(t + off);
        if p != 0 {
            m.free(p);
            m.set_u32(t + off, 0);
        }
    }
    let scan = m.u32(SCAN);
    if scan != 0 {
        m.free(scan);
        m.set_u32(SCAN, 0);
    }
}
