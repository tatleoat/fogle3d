// SPDX-License-Identifier: MIT
//! `renutil.c` on the memory image: `si_fatal`, the renderer's way out on an error
//! (finding 0012, section 19.1). The rest of the file's ports are [`crate::renutil`], on
//! values.

use crate::data::text;
use crate::img::{f, g, Outside};
use si_mem::{clib, Mem};
use si_x87::{x, Round};

/// Reports an error and leaves the render [soft 0x08260560 si_fatal]:
/// `si_fatal(text, more, seconds)`.
///
/// 1. The two texts are joined (`sprintf` with `%s%s` into 1024 bytes of the frame: longer
///    texts overrun it in the original, and here the image's stack) and copied to
///    `SI_renMessage`.
/// 2. In the program (`SI_standaloneRenderer` = 0) the text is drawn in red in the middle
///    of the screen: `font(SI_irisf)`, `RGBcolor(255, 0, 0)`, x = trunc(`SI_xmaxscreen` ×
///    0.5 − 0.5 × `strwidth(text)`) (the screen's width loaded as a signed 16-bit word, the
///    difference cut off towards zero), `getdisplaymode()`; above 1 (double buffered)
///    `viewport(0, SI_xmaxscreen, 0, SI_ymaxscreen)` first and `swapbuffers()` last; between
///    them `cmov2i(x, 60 when SI_screenRes is 1, else 80)` and `charstr(text)`. Then
///    `sleep(seconds)`.
/// 3. In the stand-alone renderer: `printf("%s\n", text)`; when the sixteen-bit word at
///    0x08790cda is 9 or 10 also `fprintf(SI_getStatusFile(), "%s\n", text)`,
///    `fflush(SI_getStatusFile())` and `sleep(2)`; **otherwise it does not sleep at all**.
/// 4. `longjmp(SI_ren, 0)`: `SI_image` takes its second return from `_setjmp`.
///
/// Every callee is another file's and is called through `env` by address. Here the routine
/// returns after `longjmp`; its caller asks [`Outside::jumped`]. Class: copy (the product
/// and the difference are exact).
pub fn fatal(m: &mut Mem, env: &mut dyn Outside, first: u32, more: u32, seconds: u32) {
    let sp = m.sp();
    let buffer = m.local(1024);
    let mut joined = m.cstr(first);
    joined.extend_from_slice(&m.cstr(more));
    m.put_cstr(buffer, &joined);
    clib::strcpy(m, g::SI_REN_MESSAGE, buffer);
    if m.i16(g::SI_STANDALONE_RENDERER) == 0 {
        env.call(m, f::FONT, &[m.i16(g::SI_IRISF) as i32 as u32]);
        env.call(m, f::RGBCOLOR, &[0xff, 0, 0]);
        let width = env.call(m, f::STRWIDTH, &[buffer]) as i32;
        let half = x(0.5f64);
        let px = (x(m.i16(g::SI_XMAXSCREEN) as i32) * half - x(width) * half).to_i32(Round::Zero);
        let double = env.call(m, f::GETDISPLAYMODE, &[]) as i32 > 1;
        if double {
            let (xmax, ymax) = (m.i16(g::SI_XMAXSCREEN) as i32 as u32, m.i16(g::SI_YMAXSCREEN) as i32 as u32);
            env.call(m, f::VIEWPORT, &[0, xmax, 0, ymax]);
        }
        let y = if m.i16(g::SI_SCREEN_RES) == 1 { 0x3c } else { 0x50 };
        env.call(m, f::CMOV2I, &[px as u32, y]);
        env.call(m, f::CHARSTR, &[buffer]);
        if double {
            env.call(m, f::SWAPBUFFERS, &[]);
        }
        env.call(m, f::SLEEP, &[seconds]);
    } else {
        env.call(m, f::PRINTF, &[text::FMT_LINE, buffer]);
        if m.u16(g::REN_FA).wrapping_sub(9) <= 1 {
            let file = env.call(m, f::SI_GET_STATUS_FILE, &[]);
            env.call(m, f::FPRINTF, &[file, text::FMT_LINE_STATUS, buffer]);
            let file = env.call(m, f::SI_GET_STATUS_FILE, &[]);
            env.call(m, f::FFLUSH, &[file]);
            env.call(m, f::SLEEP, &[2]);
        }
    }
    env.call(m, f::LONGJMP, &[g::SI_REN, 0]);
    m.set_sp(sp);
}

/// Shows a warning [soft 0x0826072c SI_warning]: `SI_warning(text, more, seconds)`.
///
/// 1. The two texts joined (`sprintf` with `%s%s` into 1024 bytes of the frame).
/// 2. In the stand-alone renderer: `printf("%s\n", text)`, and nothing else.
/// 3. In the program: y = 60 when `SI_screenRes` is 1, else 80; when the sequence word at
///    0x08790c7e is 1 the text is copied to `SI_renMessage`; x = trunc(`SI_xmaxscreen` ×
///    0.5 − 0.5 × `strwidth(text)`) as in [`fatal`] (no `font` call before); then with
///    `getdisplaymode()` above 1: `pushviewport()`, `viewport(0, SI_xmaxscreen, 0,
///    SI_ymaxscreen)`, `frontbuffer(1)`, the text drawn in red (`RGBcolor(255, 0, 0)`,
///    `cmov2i(x, y)`, `charstr`), `sleep(seconds)`, drawn again in blue (`RGBcolor(0, 0,
///    1)`), `frontbuffer(0)`, `popviewport()`; otherwise the same drawing without the four
///    viewport and buffer calls.
///
/// Every callee is another file's (the C library's `sprintf` and `strcpy` are done on the
/// image). Class: copy.
pub fn warning(m: &mut Mem, env: &mut dyn Outside, first: u32, more: u32, seconds: u32) {
    let sp = m.sp();
    let buffer = m.local(1024);
    let mut joined = m.cstr(first);
    joined.extend_from_slice(&m.cstr(more));
    m.put_cstr(buffer, &joined);
    if m.i16(g::SI_STANDALONE_RENDERER) != 0 {
        env.call(m, f::PRINTF, &[text::FMT_LINE_WARNING, buffer]);
        m.set_sp(sp);
        return;
    }
    let y = if m.i16(g::SI_SCREEN_RES) == 1 { 0x3c } else { 0x50 };
    if m.i16(g::SEQUENCE) == 1 {
        clib::strcpy(m, g::SI_REN_MESSAGE, buffer);
    }
    let width = env.call(m, f::STRWIDTH, &[buffer]) as i32;
    let half = x(0.5f64);
    let px = (x(m.i16(g::SI_XMAXSCREEN) as i32) * half - x(width) * half).to_i32(Round::Zero) as u32;
    let double = env.call(m, f::GETDISPLAYMODE, &[]) as i32 > 1;
    if double {
        env.call(m, f::PUSHVIEWPORT, &[]);
        let (xmax, ymax) = (m.i16(g::SI_XMAXSCREEN) as i32 as u32, m.i16(g::SI_YMAXSCREEN) as i32 as u32);
        env.call(m, f::VIEWPORT, &[0, xmax, 0, ymax]);
        env.call(m, f::FRONTBUFFER, &[1]);
    }
    env.call(m, f::RGBCOLOR, &[0xff, 0, 0]);
    env.call(m, f::CMOV2I, &[px, y]);
    env.call(m, f::CHARSTR, &[buffer]);
    env.call(m, f::SLEEP, &[seconds]);
    env.call(m, f::RGBCOLOR, &[0, 0, 1]);
    env.call(m, f::CMOV2I, &[px, y]);
    env.call(m, f::CHARSTR, &[buffer]);
    if double {
        env.call(m, f::FRONTBUFFER, &[0]);
        env.call(m, f::POPVIEWPORT, &[]);
    }
    m.set_sp(sp);
}

/// `lastcall` (0x0869c2d4) and `last_utime` (0x0869c2d8) of `SI_dtime`.
pub const DTIME_LASTCALL: u32 = 0x0869_c2d4;
pub const DTIME_LAST_UTIME: u32 = 0x0869_c2d8;

/// Times a step of the frame [soft 0x08260b78 SI_dtime]: `SI_dtime(text)`.
///
/// `time(&now)` and `times(&tms)` into locals; with a text: `fprintf(statistics file
/// (0x08790dd0), "frame %d, %s: real %ld sec  CPU %1.2f sec\n", frame (0x08790dda, as a
/// signed 16-bit word), text, now − lastcall, (tms_utime − last_utime) / 100.0)` (the
/// difference loaded as an integer and divided as a double by 100) and
/// `fflush(statistics file)`. Then `lastcall` = now and `last_utime` = tms_utime, with a
/// text or without. Class: copy (one division).
pub fn dtime(m: &mut Mem, env: &mut dyn Outside, text_at: u32) {
    let sp = m.sp();
    let frame = m.local(0x24);
    // (the frame's locals: now at -0x18, the tms record at -0x10, its first word tms_utime)
    let (now_at, tms_at) = (frame + 0x24 - 0x18, frame + 0x24 - 0x10);
    env.call(m, f::TIME, &[now_at]);
    env.call(m, f::TIMES, &[tms_at]);
    let (now, utime) = (m.i32(now_at), m.i32(tms_at));
    if text_at != 0 {
        let cpu = (x(utime.wrapping_sub(m.i32(DTIME_LAST_UTIME))) / x(100.0f64)).to_f64().to_bits();
        let file = m.u32(g::STATS_FILE);
        let frame_no = m.i16(g::FRAME_NOW) as i32 as u32;
        env.call(m, f::FPRINTF, &[file, text::FMT_DTIME, frame_no, text_at, now.wrapping_sub(m.i32(DTIME_LASTCALL)) as u32, cpu as u32, (cpu >> 32) as u32]);
        let file = m.u32(g::STATS_FILE);
        env.call(m, f::FFLUSH, &[file]);
    }
    m.set_i32(DTIME_LASTCALL, now);
    m.set_i32(DTIME_LAST_UTIME, utime);
    m.set_sp(sp);
}

/// The renderer's answer to a segmentation fault [soft 0x08260f40 SI_segvHandler]:
/// `SI_segvHandler()`: `si_fatal(" ", "malloc failed, reduce memory demands", 2)` when
/// `SI_renChunk` is a record whose 16-bit word at +4 (the busy word) is set, else
/// `si_fatal(" ", "softimage internal error", 2)` (each text with a blank of its own copy).
/// Class: copy.
pub fn segv_handler(m: &mut Mem, env: &mut dyn Outside) {
    let chunk = m.u32(g::SI_REN_CHUNK);
    if chunk != 0 && m.i16(chunk + 4) != 0 {
        env.call(m, f::SI_FATAL, &[text::BLANK_MALLOC, text::MALLOC_FAILED, 2]);
    } else {
        env.call(m, f::SI_FATAL, &[text::BLANK_INTERNAL, text::INTERNAL_ERROR, 2]);
    }
}

/// 0x0869c200 / 0x0869c240 / 0x0869c2a0: the three copies of "<middle mouse> to exit, <left
/// mouse> to continue" (status line; red text; blue text that paints it out); 0x0869c271:
/// "Voluntary exit..." and 0x0869c283 the text after it (an empty string: the status line is cleared).
const ASK_STATUS: u32 = 0x0869_c200;
const ASK_RED: u32 = 0x0869_c240;
const ASK_BLUE: u32 = 0x0869_c2a0;
const VOLUNTARY_EXIT_TEXT: u32 = 0x0869_c271;
const VOLUNTARY_EXIT_CLEAR: u32 = 0x0869_c283;

/// Asks whether the render is to stop [soft 0x08260918 SI_voluntaryExit]:
/// `SI_voluntaryExit(code, status)`; answers 0 (go on) or 1 (stop, `code` is 9 or 10); for any
/// other code a stop leaves through `longjmp(SI_ren, 0)` (here the routine returns 0 after the
/// jump and the caller asks [`Outside::jumped`]).
///
/// 1. `qtest()` / `qread` until the event queue is empty.
/// 2. y = 60 when `SI_screenRes` is 1 else 80; x = trunc(`SI_xmaxscreen` × 0.5 − 200.0) (the
///    screen's width loaded as a signed 16-bit word, the difference cut off towards zero, then
///    taken as a signed 16-bit word for `cmov2i`).
/// 3. When the sequence word at 0x08790c7e is 1 there is no question: a stop at once, by the
///    rule at the end.
/// 4. Else, with `status` 0: `getdisplaymode()` above 1 (double buffered) does `pushviewport`,
///    `viewport(0, xmax, 0, ymax)`, `frontbuffer(1)`; then `RGBcolor(255, 0, 0)`,
///    `cmov2i(x, y)`, `charstr(red text)`. With `status` set the text goes to
///    `SI_statusDisplayMessage(text, 2)` instead.
/// 5. With the tablet on, `SI_tabletGetData()` until `qtest()` answers; then `qread`. Then wait
///    until `getbutton(0x66)`, the tablet's last button 0x66, `getbutton(0x67)` or the tablet's
///    last button 0x67 (the tablet on: `SI_tabletGetButtonUpEvent()` each round).
/// 6. 0x66 (either source) means stop: "Voluntary exit..." (18 bytes) is copied to
///    `SI_renMessage`; the code 9 or 10 answers 1, any other code leaves through `longjmp`.
/// 7. Otherwise `qread`, the text is taken back (`status`: `SI_statusDisplayMessage("", 0)`; else
///    `RGBcolor(0, 0, 1)`, `cmov2i`, `charstr(blue text)`, and a double-buffered display goes
///    `frontbuffer(0)`, `popviewport()`), and the answer is 0.
///
/// Note the sequence-word case (3) does not copy the message. Class: copy.
pub fn voluntary_exit(m: &mut Mem, env: &mut dyn Outside, code: i32, status: i16) -> u32 {
    let sp = m.sp();
    let answer = voluntary_exit_body(m, env, code, status);
    m.set_sp(sp);
    answer
}

fn voluntary_exit_body(m: &mut Mem, env: &mut dyn Outside, code: i32, status: i16) -> u32 {
    let scratch = m.local(8);
    let stop = |m: &mut Mem, env: &mut dyn Outside| -> u32 {
        if (code as u32).wrapping_sub(9) <= 1 {
            1
        } else {
            env.call(m, f::LONGJMP, &[g::SI_REN, 0]);
            0
        }
    };
    while env.call(m, f::QTEST, &[]) != 0 {
        env.call(m, f::QREAD, &[scratch]);
    }
    let px = (x(m.i16(g::SI_XMAXSCREEN) as i32) * x(0.5f64) - x(200.0f64)).to_i32(Round::Zero) as i16;
    let py: u32 = if m.i16(g::SI_SCREEN_RES) == 1 { 0x3c } else { 0x50 };
    let at = px as i32 as u32;
    if m.i16(g::SEQUENCE) == 1 {
        return stop(m, env);
    }
    let mut double = false;
    if status == 0 {
        double = env.call(m, f::GETDISPLAYMODE, &[]) as i32 > 1;
        if double {
            env.call(m, f::PUSHVIEWPORT, &[]);
            let (xmax, ymax) = (m.i16(g::SI_XMAXSCREEN) as i32 as u32, m.i16(g::SI_YMAXSCREEN) as i32 as u32);
            env.call(m, f::VIEWPORT, &[0, xmax, 0, ymax]);
            env.call(m, f::FRONTBUFFER, &[1]);
        }
        env.call(m, f::RGBCOLOR, &[0xff, 0, 0]);
        env.call(m, f::CMOV2I, &[at, py]);
        env.call(m, f::CHARSTR, &[ASK_RED]);
    } else {
        env.call(m, f::SI_STATUS_DISPLAY_MESSAGE, &[ASK_STATUS, 2]);
    }
    if m.i32(g::SI_TABLET_ACTIVE) != 0 {
        while env.call(m, f::QTEST, &[]) == 0 {
            env.call(m, f::SI_TABLET_GET_DATA, &[]);
        }
    }
    env.call(m, f::QREAD, &[scratch]);
    loop {
        let b66 = env.call(m, f::GETBUTTON, &[0x66]) != 0 || m.i32(g::SI_TABLET_BUTTON) == 0x66;
        if b66 || env.call(m, f::GETBUTTON, &[0x67]) != 0 || m.i32(g::SI_TABLET_BUTTON) == 0x67 {
            break;
        }
        if m.i32(g::SI_TABLET_ACTIVE) != 0 {
            env.call(m, f::SI_TABLET_GET_BUTTON_UP_EVENT, &[]);
        }
    }
    if env.call(m, f::GETBUTTON, &[0x66]) != 0 || m.i32(g::SI_TABLET_BUTTON) == 0x66 {
        let text = m.bytes(VOLUNTARY_EXIT_TEXT, 18).to_vec();
        m.set_bytes(g::SI_REN_MESSAGE, &text);
        return stop(m, env);
    }
    env.call(m, f::QREAD, &[scratch]);
    if status != 0 {
        env.call(m, f::SI_STATUS_DISPLAY_MESSAGE, &[VOLUNTARY_EXIT_CLEAR, 0]);
    } else {
        env.call(m, f::RGBCOLOR, &[0, 0, 1]);
        env.call(m, f::CMOV2I, &[at, py]);
        env.call(m, f::CHARSTR, &[ASK_BLUE]);
        if double {
            env.call(m, f::FRONTBUFFER, &[0]);
            env.call(m, f::POPVIEWPORT, &[]);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Calls(Vec<(u32, Vec<u32>)>, Vec<u8>);
    impl Outside for Calls {
        fn call(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
            self.0.push((f, a.to_vec()));
            match f {
                f::STRWIDTH => {
                    self.1 = m.cstr(a[0]);
                    101
                }
                f::GETDISPLAYMODE => 5,
                _ => 0,
            }
        }
    }

    #[test]
    fn the_message_in_the_middle_of_the_screen_then_the_jump() {
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        crate::data::init(&mut m);
        m.set_i16(g::SI_XMAXSCREEN, 1279);
        m.set_i16(g::SI_YMAXSCREEN, 1023);
        m.set_i16(g::SI_IRISF, 3);
        let (a, b) = (m.place_cstr(b"Cannot open output file : "), m.place_cstr(b"/p/x.1.pic"));
        let mut env = Calls(Vec::new(), Vec::new());
        fatal(&mut m, &mut env, a, b, 1);
        assert_eq!(m.cstr(g::SI_REN_MESSAGE), b"Cannot open output file : /p/x.1.pic");
        assert_eq!(env.1, b"Cannot open output file : /p/x.1.pic");
        let names: Vec<u32> = env.0.iter().map(|c| c.0).collect();
        assert_eq!(names, [f::FONT, f::RGBCOLOR, f::STRWIDTH, f::GETDISPLAYMODE, f::VIEWPORT, f::CMOV2I, f::CHARSTR, f::SWAPBUFFERS, f::SLEEP, f::LONGJMP]);
        // (1279 − 101) / 2 = 589; 80 lines up unless the screen is the small one
        assert_eq!(env.0[5].1, [589, 0x50]);
        assert_eq!(env.0[4].1, [0, 1279, 0, 1023]);
        assert_eq!(env.0[9].1, [g::SI_REN, 0]);
        // stand-alone: a line on standard output, and no sleep
        m.set_i16(g::SI_STANDALONE_RENDERER, 1);
        let mut env = Calls(Vec::new(), Vec::new());
        fatal(&mut m, &mut env, a, b, 1);
        assert_eq!(env.0.iter().map(|c| c.0).collect::<Vec<_>>(), [f::PRINTF, f::LONGJMP]);
        m.set_u16(g::REN_FA, 10);
        let mut env = Calls(Vec::new(), Vec::new());
        fatal(&mut m, &mut env, a, b, 7);
        assert_eq!(env.0.iter().map(|c| c.0).collect::<Vec<_>>(), [f::PRINTF, f::SI_GET_STATUS_FILE, f::FPRINTF, f::SI_GET_STATUS_FILE, f::FFLUSH, f::SLEEP, f::LONGJMP]);
        assert_eq!(env.0[5].1, [2]);
        assert!(m.fault().is_none());
    }
    struct Script {
        calls: Vec<(u32, Vec<u32>)>,
        queue: u32,
        buttons: Vec<(u32, u32)>,
        jumped: bool,
    }
    impl Outside for Script {
        fn call(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
            let _ = m;
            self.calls.push((f, a.to_vec()));
            match f {
                f::QTEST => {
                    let q = self.queue;
                    self.queue = self.queue.saturating_sub(1);
                    q
                }
                f::GETDISPLAYMODE => 2,
                f::GETBUTTON => self.buttons.iter().find(|b| b.0 == a[0]).map_or(0, |b| b.1),
                f::LONGJMP => {
                    self.jumped = true;
                    0
                }
                _ => 0,
            }
        }
        fn jumped(&mut self) -> bool {
            self.jumped
        }
    }

    #[test]
    fn voluntary_exit_asks_then_takes_the_answer() {
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        crate::data::init(&mut m);
        m.set_i16(g::SI_XMAXSCREEN, 1279);
        m.set_i16(g::SI_YMAXSCREEN, 1023);
        m.set_bytes(VOLUNTARY_EXIT_TEXT, b"Voluntary exit...\0");
        // left button: go on, text painted out
        let mut env = Script { calls: vec![], queue: 2, buttons: vec![(0x67, 1)], jumped: false };
        assert_eq!(voluntary_exit(&mut m, &mut env, 4, 0), 0);
        let c = &env.calls;
        assert_eq!(c[0].0, f::QTEST);
        let cm: Vec<_> = c.iter().filter(|c| c.0 == f::CMOV2I).collect();
        assert_eq!(cm[0].1, [439, 0x50]);
        assert_eq!(cm.len(), 2);
        assert_eq!(c.last().unwrap().0, f::POPVIEWPORT);
        assert!(!env.jumped);
        // the middle button: a stop; code 4 leaves by longjmp, code 9 answers 1
        let mut env = Script { calls: vec![], queue: 0, buttons: vec![(0x66, 1)], jumped: false };
        voluntary_exit(&mut m, &mut env, 4, 1);
        assert!(env.jumped);
        assert_eq!(m.cstr(g::SI_REN_MESSAGE), b"Voluntary exit...");
        assert_eq!(env.calls[1], (f::SI_STATUS_DISPLAY_MESSAGE, vec![ASK_STATUS, 2]));
        let mut env = Script { calls: vec![], queue: 0, buttons: vec![(0x66, 1)], jumped: false };
        assert_eq!(voluntary_exit(&mut m, &mut env, 9, 1), 1);
        assert!(!env.jumped);
        // the sequence word 1: no question
        m.set_i16(g::SEQUENCE, 1);
        let mut env = Script { calls: vec![], queue: 0, buttons: vec![], jumped: false };
        assert_eq!(voluntary_exit(&mut m, &mut env, 10, 0), 1);
        assert_eq!(env.calls.len(), 1);
        assert!(m.fault().is_none());
    }
}
