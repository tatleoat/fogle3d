// SPDX-License-Identifier: MIT
//! `mr_main.c`, the part that is not `mi_mr_main`: the parameter block of the external tracer, the message
//! and quit checks of the render loop, the resource check, and the "DONE" wait (package 63, finding 0042).
//! Graphics, the C library's system calls and the status line are called through [`Outside`].

use crate::mi_mr_main::{ext, si_get_mi2_output};
use crate::mr_messages::{SI_MUTEX_LOCK, SI_MUTEX_UNLOCK, SIGNAL};
use si_mem::{clib, Mem, Outside, Routine};

pub const WAIT: Routine = Routine::new("wait", 0x080c_5158);
pub const QTEST: Routine = Routine::new("qtest", 0x0860_e188);
pub const QREAD: Routine = Routine::new("qread", 0x0860_e1bc);
pub const GETBUTTON: Routine = Routine::new("getbutton", 0x0860_e684);
pub const GETRLIMIT: Routine = Routine::new("getrlimit", 0x080c_4f08);
pub const SETRLIMIT: Routine = Routine::new("setrlimit", 0x080c_3c38);
pub const FXSTAT: Routine = Routine::new("__fxstat", 0x080c_43f8);
pub const STRWIDTH: Routine = Routine::new("strwidth", 0x0860_f084);
pub const PUSHVIEWPORT: Routine = Routine::new("pushviewport", 0x0860_fdcc);
pub const POPVIEWPORT: Routine = Routine::new("popviewport", 0x0860_fde4);
pub const VIEWPORT: Routine = Routine::new("viewport", 0x0860_fe90);
pub const FRONTBUFFER: Routine = Routine::new("frontbuffer", 0x0861_1770);
pub const RGBCOLOR: Routine = Routine::new("RGBcolor", 0x0861_11a4);
pub const CMOV2I: Routine = Routine::new("cmov2i", 0x0860_f2ac);
pub const CHARSTR: Routine = Routine::new("charstr", 0x0860_ecc4);
pub const WINTITLE: Routine = Routine::new("wintitle", 0x0861_999c);
pub const GETDISPLAYMODE: Routine = Routine::new("getdisplaymode", 0x0861_81f0);
pub const USLEEP: Routine = Routine::new("usleep", 0x080c_37b8);

/// Globals of the file.
pub mod g {
    pub const SI_PROGRAM_NAME: u32 = 0x0878_2060;
    pub const SI_PROC_DATA: u32 = 0x0867_9314;
    pub const MR_RENDER_ERROR: u32 = 0x086d_2b54; // 16 bits
    pub const RLIMIT_DONE: u32 = 0x086d_2af0; // 16 bits
    pub const RLIMIT_NOFILE: u32 = 0x086d_2af4;
    pub const STR_NOFILEMAX: u32 = 0x086d_2b10; // a pointer
    pub const STR_RESOURCES: u32 = 0x086d_2b50; // a pointer
    pub const STR_DONE: u32 = 0x086d_2a64; // a pointer
    pub const SI_XMAXSCREEN: u32 = 0x0878_1794;
    pub const SI_YMAXSCREEN: u32 = 0x0878_1796;
    pub const SI_SCREENRES: u32 = 0x0878_17ae;
    pub const SI_TABLETBUTTON: u32 = 0x0867_94b4;
}

/// [soft 0x0842b654 SI_initMentalPipeParms]: fills the pipe part of the render parameter block `rp`: signals on (+0x652), the program name
/// (+0x668), the tracer variable's value (+0x664, `SI_MI_TRACER2` in the mi2 output mode), the run mode (+0x654, +0x644 and the five
/// shorts after it: 3 / 0 when the flag at +0x3e4 is set, else 2 / 1 when the one at +0x60e is), and the process data words.
pub fn si_init_mental_pipe_parms(m: &mut Mem, o: &mut dyn Outside, rp: u32) {
    let b = rp + 0x62c;
    let name = m.u32(g::SI_PROGRAM_NAME);
    m.set_u16(rp + 0x652, 1);
    m.set_u32(rp + 0x668, name);
    let var = if si_get_mi2_output(m) as u16 != 0 { 0x086d_2b77 } else { 0x086d_2b85 };
    let v = o.call(m, ext::GETENV, &[var]).eax;
    m.set_u32(rp + 0x664, v);
    let mode = if m.u16(rp + 0x3e4) != 0 {
        Some((3u32, 0u16))
    } else if m.u16(rp + 0x60e) != 0 {
        Some((2, 1))
    } else {
        None
    };
    if let Some((a, c)) = mode {
        m.set_u32(b + 0x28, a);
        m.set_u16(b + 0x18, c);
        m.set_u16(b + 0x1a, 0);
        m.set_u16(b + 0x1c, 0);
        m.set_u16(b + 0x1e, 1);
        m.set_u16(b + 0x20, 1);
        m.set_u16(b + 0x22, 0);
        m.set_u16(b + 0x24, 0);
    }
    let pd = m.u32(g::SI_PROC_DATA);
    m.set_u32(b + 4, 0xffff_ffff);
    m.set_u16(b + 0x14, 0);
    m.set_u16(b + 0x16, 0);
    m.set_u32(b, 0xffff_ffff);
    for i in 0..3 {
        let w = m.u32(pd + 4 * i);
        m.set_u32(b + 8 + 4 * i, w);
    }
}

/// [soft 0x0842b734 handler]: SIGCHLD handler: reaps a child and sets itself again.
pub fn handler(m: &mut Mem, o: &mut dyn Outside, _sig: u32) {
    let st = m.local(4);
    o.call(m, WAIT, &[st]);
    m.release(4);
    o.call(m, SIGNAL, &[0x11, 0x0842_b734]);
}

/// Sets the render-error flag from the text: 1 when it contains the word at `error` or the one at `failed` ("error", "failed").
fn set_error_flag(m: &mut Mem, text: u32, error: u32, failed: u32) {
    let e = clib::strstr(m, text, error) != 0 || clib::strstr(m, text, failed) != 0;
    m.set_u16(g::MR_RENDER_ERROR, e as u16);
}

/// [soft 0x0842b75c displayMessages]: shows a message from mental ray on the status line (to the file only without a window) and notes whether it reports an error.
pub fn display_messages(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    if text != 0 {
        set_error_flag(m, text, 0x086d_2b92, 0x086d_2b98);
    }
    if o.call(m, ext::SI_IS_IGLOO, &[]).eax as u16 == 0 {
        let sys = m.u32(ext_sysid());
        o.call(m, ext::WINSET, &[sys]);
        o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[text, 1]);
        o.call(m, ext::GFLUSH, &[]);
    } else {
        o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY, &[text, 1]);
    }
}

fn ext_sysid() -> u32 {
    crate::mi_mr_main::g::SI_SYSID
}

/// [soft 0x0842b7ec detectMessages]: the same without the window handling.
pub fn detect_messages(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    if text == 0 {
        return;
    }
    o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[text, 1]);
    set_error_flag(m, text, 0x086d_2b9f, 0x086d_2ba5);
}

/// [soft 0x0842b9bc checkUserQuit]: under the mutex, true when ESC (0x66) is the next queued event or the mouse button 7 is down; drains the event queue.
pub fn check_user_quit(m: &mut Mem, o: &mut dyn Outside, mutex: u32) -> u32 {
    o.call(m, SI_MUTEX_LOCK, &[mutex]);
    let sys = m.u32(ext_sysid());
    o.call(m, ext::WINSET, &[sys]);
    let t = o.call(m, QTEST, &[]).eax;
    let r = if t == 0x66 { 1 } else { (o.call(m, GETBUTTON, &[7]).eax != 0) as u32 };
    let ev = m.local(2);
    while o.call(m, QTEST, &[]).eax != 0 {
        o.call(m, QREAD, &[ev]);
    }
    m.release(2);
    o.call(m, ext::GFLUSH, &[]);
    o.call(m, SI_MUTEX_UNLOCK, &[mutex]);
    r
}

/// [soft 0x0842bbc8 checkResources]: raises the soft limit on open files (hard limit, at most 1000, or what the variable named at `strNOFILEMAX` says, between that limit and the first value)
/// and counts the open descriptors; 1 when at least five of the allowed ones are free, otherwise shows the resources message and gives 0.
pub fn check_resources(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let rl = m.local(8);
    let st = m.local(0x58);
    if m.u16(g::RLIMIT_DONE) == 0 {
        m.set_u16(g::RLIMIT_DONE, 1);
        o.call(m, GETRLIMIT, &[7, rl]);
        let mx = m.u32(rl + 4);
        m.set_u32(g::RLIMIT_NOFILE, if mx > 1000 { 1000 } else { mx });
        let name = m.u32(g::STR_NOFILEMAX);
        let env = o.call(m, ext::GETENV, &[name]).eax;
        if env != 0 {
            let v = clib::strtol(m, env, 0, 10) as u32;
            let mut a = if m.u32(rl + 4) < v { m.u32(rl + 4) } else { v };
            let n = m.u32(g::RLIMIT_NOFILE);
            if a < n {
                a = n;
            }
            m.set_u32(g::RLIMIT_NOFILE, a);
        }
        let n = m.u32(g::RLIMIT_NOFILE);
        m.set_u32(rl, n);
        o.call(m, SETRLIMIT, &[7, rl]);
    }
    let n = m.i32(g::RLIMIT_NOFILE);
    let mut count = 0i32;
    let mut fd = 0i32;
    while fd < n {
        if (o.call(m, FXSTAT, &[3, fd as u32, st]).eax as i32) >= 0 {
            count += 1;
        }
        fd += 1;
    }
    m.release(0x58);
    m.release(8);
    if count < n.wrapping_sub(5) {
        1
    } else {
        let s = m.u32(g::STR_RESOURCES);
        o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[s, 3]);
        0
    }
}

fn trunc_i32(v: f64) -> i32 {
    if v.is_nan() || v >= 2147483648.0 || v <= -2147483649.0 {
        i32::MIN
    } else {
        v as i32
    }
}

/// [soft 0x0842ba44 computeWindowSizePos]: size and position of the render window of thread record `thr` from the render control `rc` (a crop window when the flag at +0x224 is set),
/// centred on the screen unless the control gives a position (+0x5b0).
pub fn compute_window_size_pos(m: &mut Mem, rc: u32, thr: u32) {
    let crop = m.u32(rc + 0x224) != 0;
    let w = m.i16(rc + 0x59a) as f64;
    let h = m.i16(rc + 0x59c) as f64;
    let (mut x0, mut y0) = (0i32, 0i32);
    if crop {
        x0 = trunc_i32(m.f32(rc + 0x22c) as f64 * w + 0.5);
        y0 = trunc_i32(m.f32(rc + 0x238) as f64 * h + 0.5);
        let a = trunc_i32(w * (1.0 - m.f32(rc + 0x230) as f64) + 0.5);
        let b = trunc_i32(h * (1.0 - m.f32(rc + 0x234) as f64) + 0.5);
        m.set_i32(thr + 0x48, a.wrapping_sub(x0));
        m.set_i32(thr + 0x4c, b.wrapping_sub(y0));
    } else {
        m.set_i32(thr + 0x48, w as i32);
        m.set_i32(thr + 0x4c, h as i32);
    }
    let cx = (m.i16(g::SI_XMAXSCREEN) as i32).wrapping_sub(m.i32(thr + 0x48)) / 2;
    let cy = (m.i16(g::SI_YMAXSCREEN) as i32).wrapping_sub(m.i32(thr + 0x4c)) / 2;
    m.set_i32(thr + 0x40, cx);
    m.set_i32(thr + 0x44, cy);
    m.set_i32(thr + 0x38, cx);
    m.set_i32(thr + 0x3c, cy);
    if m.i16(rc + 0x5b0) != 0 {
        m.set_i32(thr + 0x40, m.i16(rc + 0x5b2) as i32);
        m.set_i32(thr + 0x44, m.i16(rc + 0x5b4) as i32);
        m.set_i32(thr + 0x38, 0);
        m.set_i32(thr + 0x3c, 0);
    }
    if crop {
        m.set_i32(thr + 0x38, m.i32(thr + 0x38).wrapping_add(1).wrapping_sub(x0));
        m.set_i32(thr + 0x3c, m.i32(thr + 0x3c).wrapping_add(1).wrapping_sub(y0));
    }
}

/// [soft 0x0842b844 WaitAfterDone]: shows "DONE" (in the window title when `titled` is non-zero, else in red on the front buffer) and waits for ESC or the middle mouse button, then clears the text.
pub fn wait_after_done(m: &mut Mem, o: &mut dyn Outside, titled: u32) {
    let done = m.u32(g::STR_DONE);
    let wd = o.call(m, STRWIDTH, &[done]).eax as i32;
    let xmax = m.i16(g::SI_XMAXSCREEN) as f64;
    let x = trunc_i32(xmax * 0.5 - wd as f64 * 0.5) as u32;
    let y = if m.u16(g::SI_SCREENRES) == 1 { 0x3c } else { 0x50 };
    let deep = (o.call(m, GETDISPLAYMODE, &[]).eax as i32) > 1;
    if deep {
        o.call(m, PUSHVIEWPORT, &[]);
        let ym = m.i16(g::SI_YMAXSCREEN) as i32 as u32;
        let xm = m.i16(g::SI_XMAXSCREEN) as i32 as u32;
        o.call(m, VIEWPORT, &[0, xm, 0, ym]);
        o.call(m, FRONTBUFFER, &[1]);
    }
    if titled as u16 != 0 {
        o.call(m, WINTITLE, &[0x086d_2bc0]);
    } else {
        o.call(m, RGBCOLOR, &[0xff, 0, 0]);
        o.call(m, CMOV2I, &[x, y]);
        let d = m.u32(g::STR_DONE);
        o.call(m, CHARSTR, &[d]);
    }
    loop {
        o.call(m, USLEEP, &[200000]);
        if o.call(m, GETBUTTON, &[0x66]).eax != 0 || m.u32(g::SI_TABLETBUTTON) == 0x66 {
            break;
        }
        if o.call(m, GETBUTTON, &[7]).eax != 0 {
            break;
        }
    }
    o.call(m, RGBCOLOR, &[0, 0, 1]);
    o.call(m, CMOV2I, &[x, y]);
    let d = m.u32(g::STR_DONE);
    o.call(m, CHARSTR, &[d]);
    if deep {
        o.call(m, FRONTBUFFER, &[0]);
        o.call(m, POPVIEWPORT, &[]);
    }
}

#[allow(dead_code)]
const _U: (Routine, Routine) = (SI_MUTEX_LOCK, SI_MUTEX_UNLOCK);
