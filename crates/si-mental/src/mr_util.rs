// SPDX-License-Identifier: MIT
//! `mr_util.c`: the pipe to mental ray (package 64, finding 0042): the buffered writer behind
//! `SI_mrpipeFPRINTF`/`SI_mrpipeFPUTC`, the pipe set-up, and two small helpers (the view check and
//! the per-frame `.mi` file name).
//!
//! The state is four words of the image: the pipe id (−1 when the text goes to the `out` stream of
//! the translator instead), the address of a termination flag, the address of the slot that holds
//! the 16 KB buffer, and the fill index.

use crate::cprintf;
use crate::mi_mr_main::{ext, si_malloc_blk, sprintf_lit};
use si_mem::{clib, Mem, Outside};

const MRPIPE_ID: u32 = 0x086d_2c00;
const B_TERMINATE: u32 = 0x086d_2c04;
const BUFFER: u32 = 0x086d_2c08;
const BUFF_NDX: u32 = 0x086d_2c0c;
/// The `FILE *` the translator writes to when there is no pipe, and the flag of the stand-alone renderer.
const OUT: u32 = 0x0882_9f08;
const STANDALONE_RENDERER: u32 = 0x0867_9400;
const CAPACITY: i32 = 0x3fff;

/// [soft 0x0842c370 SI_checkHighlightedWindow]: 1 when the render camera's view type (2000 to
/// 2003, 2009) can be rendered; 0 with a message otherwise.
pub fn si_check_highlighted_window(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let cam = o.call(m, ext::SI_GET_RENDER_CAMERA, &[1]).eax;
    if cam == 0 {
        o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2c20, 3]);
        return 0;
    }
    let idx = m.u16(cam + 4).wrapping_sub(2000) as i16 as i32 as u32;
    if idx > 9 {
        o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2d20, 3]);
        return 0;
    }
    match idx {
        0..=3 | 9 => 1,
        4 => {
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2c60, 3]);
            0
        }
        7 => {
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2ca0, 3]);
            0
        }
        8 => {
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2ce0, 3]);
            0
        }
        _ => {
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2d20, 3]);
            0
        }
    }
}

/// [soft 0x0842c40c SI_generateMIPerFrameFileName]: a new string `name[.n[.k]].mi` (the ".mi" of
/// `name` is cut off while it is copied and put back), or null. `flag` is the low half of its second
/// argument; `k` of −1 means "no second number".
pub fn si_generate_mi_per_frame_file_name(m: &mut Mem, o: &mut dyn Outside, name: u32, flag: u32, n: u32, k: u32) -> u32 {
    if name == 0 {
        return 0;
    }
    let len = clib::strlen(m, name);
    let tail = name.wrapping_add(len).wrapping_sub(3);
    let saved = if m.u8(tail) == b'.' && m.u8(tail + 1) == b'm' && m.u8(tail + 2) == b'i' && m.u8(tail + 3) == 0 {
        m.set_u8(tail, 0);
        tail
    } else {
        0
    };
    let len = clib::strlen(m, name);
    let buf = si_malloc_blk(m, len + 0x32);
    if buf == 0 {
        o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[0x086d_2d4d, 3]);
        if saved != 0 {
            m.set_u8(saved, b'.');
        }
        return 0;
    }
    if flag & 0xffff != 0 {
        if k == 0xffff_ffff {
            sprintf_lit(m, buf, b"%s.%d.mi", &[name, n]);
        } else {
            sprintf_lit(m, buf, b"%s.%d.%d.mi", &[name, n, k]);
        }
    } else {
        sprintf_lit(m, buf, b"%s.mi", &[name]);
    }
    if saved != 0 {
        m.set_u8(saved, b'.');
    }
    buf
}

/// [soft 0x0842c578 SI_mrpipeSet]: remember the pipe id, the termination flag's address and the
/// buffer slot; empty the buffer and allocate its 16 KB. 1 when the allocation succeeded.
pub fn si_mrpipe_set(m: &mut Mem, _o: &mut dyn Outside, id: u32, terminate: u32, buffer: u32) -> i32 {
    m.set_u32(MRPIPE_ID, id);
    m.set_u32(B_TERMINATE, terminate);
    m.set_u32(BUFFER, buffer);
    m.set_u32(BUFF_NDX, 0);
    let p = si_malloc_blk(m, 0x4000);
    let slot = m.u32(BUFFER);
    m.set_u32(slot, p);
    (m.u32(slot) != 0) as i32
}

/// [soft 0x0842c76c writePipe]: write `n` bytes from `buf` with `SI_pipeioWrite (0x0842fd68)`,
/// repeating after a short write; 1 when all went out, 0 on an error or when the termination flag
/// (a short at the stored address) is set after a write.
pub fn write_pipe(m: &mut Mem, o: &mut dyn Outside, buf: u32, n: i32) -> i32 {
    let mut n = n;
    let mut off = 0u32;
    let written = m.local(4);
    let r = loop {
        if n <= 0 {
            break 1;
        }
        let id = m.u32(MRPIPE_ID);
        let rc = o.call(m, ext::SI_PIPEIO_WRITE, &[id, buf.wrapping_add(off), n as u32, written]).eax;
        if rc != 0 {
            break 0;
        }
        let w = m.u32(written);
        n = n.wrapping_sub(w as i32);
        off = off.wrapping_add(w);
        let term = m.u32(B_TERMINATE);
        if m.u16(term) != 0 {
            break 0;
        }
    };
    m.release(4);
    r
}

/// [soft 0x0842c5d8 SI_mrpipeFlush]: write out the buffer; 1 when it was empty or went out.
pub fn si_mrpipe_flush(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let ndx = m.u32(BUFF_NDX);
    if ndx == 0 {
        return 1;
    }
    let slot = m.u32(BUFFER);
    let buf = m.u32(slot);
    let r = write_pipe(m, o, buf, ndx as i32);
    m.set_u32(BUFF_NDX, 0);
    r as i16 as i32
}

/// The failed flush of the two writers: leave the program when it is the stand-alone renderer, else the thread.
fn flush_failed(m: &mut Mem, o: &mut dyn Outside) {
    if m.u16(STANDALONE_RENDERER) != 0 {
        o.call(m, ext::EXIT, &[0]);
    }
    o.call(m, ext::SI_MRTHREAD_EXIT, &[0]);
}

/// [soft 0x0842c610 SI_mrpipeFPUTC]: one byte to the pipe buffer (or to `out` without a pipe).
pub fn si_mrpipe_fputc(m: &mut Mem, o: &mut dyn Outside, c: u32) {
    if m.u32(MRPIPE_ID) == 0xffff_ffff {
        let out = m.u32(OUT);
        if out != 0 {
            o.call(m, ext::FPUTC, &[c, out]);
        }
        return;
    }
    if (m.u32(BUFF_NDX) as i32).wrapping_add(1) > CAPACITY && si_mrpipe_flush(m, o) as i16 == 0 {
        flush_failed(m, o);
    }
    let slot = m.u32(BUFFER);
    let base = m.u32(slot);
    let ndx = m.u32(BUFF_NDX);
    m.set_u8(base.wrapping_add(ndx), c as u8);
    m.set_u32(BUFF_NDX, ndx.wrapping_add(1));
}

/// [soft 0x0842c68c SI_mrpipeFPRINTF]: format `fmt` with the argument words `va` into a 8 KB local
/// and send the text as `SI_mrpipeFPUTC` does one byte.
pub fn si_mrpipe_fprintf(m: &mut Mem, o: &mut dyn Outside, fmt: u32, va: &[u32]) {
    let buf = m.local(0x2000);
    let text = cprintf::format(m, fmt, va);
    m.put_cstr(buf, &text);
    if m.u32(MRPIPE_ID) == 0xffff_ffff {
        let out = m.u32(OUT);
        if out != 0 {
            o.call(m, ext::FPUTS, &[buf, out]);
        }
    } else {
        let n = clib::strlen(m, buf);
        if (n as i32).wrapping_add(m.u32(BUFF_NDX) as i32) > CAPACITY && si_mrpipe_flush(m, o) as i16 == 0 {
            flush_failed(m, o);
        }
        let slot = m.u32(BUFFER);
        let base = m.u32(slot);
        let ndx = m.u32(BUFF_NDX);
        clib::memmove(m, base.wrapping_add(ndx), buf, n);
        m.set_u32(BUFF_NDX, ndx.wrapping_add(n));
    }
    m.release(0x2000);
}
