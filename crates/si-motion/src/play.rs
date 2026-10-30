// SPDX-License-Identifier: MIT
//! Playback and scene refresh: `motion.c` (finding 0017, "Playback").
//!
//! Playback has no clock of frames: it keeps a **position**, a number of frames with a
//! fraction, in the 80-bit register across the whole run ([`play_back_frames`]). Each turn
//! shows the frame nearest to the position ([`play_back_frame`]) and adds a **step** to it.
//! The step is the frame step of the play control, or, with "adaptive step" (real-time
//! playback), the time the last frame took to show, in frames: frames are then skipped so
//! that the animation keeps its speed. A frame is never a field here: nothing in this file
//! adds a half.

use crate::glob;
use si_mem::{Addr, Mem};
use crate::update::{self, sec_to_frame, UpdateHost};
use si_x87::{Round, F80};

/// Globals of the playback (symbol table of the Linux binary).
pub mod g {
    use si_mem::Addr;
    /// `secPrev`, `secPerFramePrev`: the clock (in ticks) at the last call of
    /// `timeElapsed` on each; `prevFrame`: the frame shown last. Statics of `motion.c`.
    pub const SEC_PREV: Addr = 0x0869_61a0;
    pub const SEC_PER_FRAME_PREV: Addr = 0x0869_61a4;
    pub const PREV_FRAME: Addr = 0x0869_61a8;
    /// `SI_playBoxMode`, one byte: 0 the program's own windows, −1 no playback at all,
    /// anything else the stand-alone viewer ("play box").
    pub const SI_PLAY_BOX_MODE: Addr = 0x086c_ad7e;
    /// `SI_stepAdaptive`, 16 bits: real-time playback.
    pub const SI_STEP_ADAPTIVE: Addr = 0x0878_fb28;
    /// `si_psPrevFlag`, 16 bits: 1 while a PostScript preview is being written.
    pub const SI_PS_PREV_FLAG: Addr = 0x0878_fb2a;
    /// `SI_dialogActive`, 16 bits: a dialogue with curves is open.
    pub const SI_DIALOG_ACTIVE: Addr = 0x0878_1412;
    /// `SI_fastPlayBackActive`: the frames are replayed from the fast-playback cache.
    pub const SI_FAST_PLAY_BACK_ACTIVE: Addr = 0x0867_9324;
    /// `SI_sysid`: the window the refresh draws in.
    pub const SI_SYSID: Addr = 0x0867_940c;
    /// `SI_audioContainer`.
    pub const SI_AUDIO_CONTAINER: Addr = 0x087f_b9c0;
    /// "Interrupt preview", three copies: the mouse button prompts during playback.
    pub const TEXT_INTERRUPT: [Addr; 3] = [0x0869_6252, 0x0869_6240, 0x0869_622e];
}

/// Fields of a window (`__SI_Grafport`) that [`scene_refresh_loop`] reads.
mod port {
    /// 16 bits: 1 for a window that shows a grid.
    pub const HAS_GRID: u32 = 0x0a;
    /// The camera the window looks through.
    pub const CAMERA: u32 = 0x2c;
    /// 16 bits: the window takes part in fast playback.
    pub const FAST: u32 = 0x25c;
}

/// Ticks per second of `times()`: the double 100.0 at 0x0864d540.
const TICKS: f64 = 100.0;

crate::host_trait! {
    /// What `motion.c` calls outside this package.
    pub trait PlayHost: UpdateHost {
        /// `times(&buffer)` of the C library: the clock in ticks (100 a second).
        fn times(buffer: Addr) -> i32;
        /// `SI_statusDisplayPrompt(line 1, line 2)`: two C strings.
        fn status_display_prompt(a: Addr, b: Addr);
        /// `SI_statusDisplayMouse(left, middle, right)`: the mouse button prompts.
        fn status_display_mouse(a: Addr, b: Addr, c: Addr);
        /// `SI_sceneUpdate(time)` (motionUpdate.c; ported as [`update::scene_update`]). It is
        /// taken through the host so that a viewer or a test can stand in for the whole
        /// update; a host of the full program forwards to the port.
        fn scene_update(time: f32);
        /// `SI_audioContainerPlayFrame(container, frame)`, `SI_audioPlaySetMode(container, a,
        /// b)`, `SI_audioContainerPlayAbort(container)`.
        fn audio_container_play_frame(container: Addr, frame: i32);
        fn audio_play_set_mode(container: Addr, a: i32, b: i32);
        fn audio_container_play_abort(container: Addr);
        /// The graphics library's `winget()`, `winset(id)`, `getdrawmode()`, `drawmode(mode)`.
        fn winget() -> i32;
        fn winset(id: i32);
        fn getdrawmode() -> i32;
        fn drawmode(mode: i32);
        /// `SI_fastGetDirty(window, frame, 0)`: whether the fast-playback cache lacks the frame.
        fn fast_get_dirty(port: Addr, frame: i32, flag: i32) -> i16;
        /// `SI_windowGetNextVisible(&window)`: steps to the next visible window; 0 at the end.
        fn window_get_next_visible(port: Addr) -> i16;
        /// `SI_drawGrid(window, 0)`.
        fn draw_grid(port: Addr, flag: i32);
        /// `SI_refreshSelectedFcurve(0)`.
        fn refresh_selected_fcurve(flag: i32);
        /// `si_postscriptDumpFrame(frame)` (motionUtils.c).
        fn postscript_dump_frame(frame: i32);
        /// `SI_viewerUpdateScene(frame)`: the stand-alone viewer shows a frame.
        fn viewer_update_scene(frame: i32);
        /// `SI_dialogUpdateFcv()`.
        fn dialog_update_fcv();
        /// `SI_mouseInterrupt()`: non-zero when the user asked to stop.
        fn mouse_interrupt() -> i16;
    }
}

/// C's `%6.2f` of a double, as far as the playback prompts need it.
fn fmt_6_2(v: f64) -> String {
    if v.is_nan() {
        format!("{:>6}", if v.is_sign_negative() { "-nan" } else { "nan" })
    } else if v.is_infinite() {
        format!("{:>6}", if v < 0.0 { "-inf" } else { "inf" })
    } else {
        format!("{v:6.2}")
    }
}

fn put_str(m: &mut Mem, at: Addr, s: &str) {
    let mut b = s.as_bytes().to_vec();
    b.push(0);
    m.set_bytes(at, &b);
}

/// `(float)frame / SI_frameRate` rounded to `float`: the time of a frame.
fn frame_time(m: &Mem, frame: i32) -> f32 {
    (F80::from_f32(F80::from_i32(frame).to_f32()) / glob::frame_rate(m)).to_f32()
}

/// Seconds since the last call on the same slot [soft 0x0820f5f0 timeElapsed]: the clock
/// (`times()`, ticks) replaces the slot's value, and the difference of the two, as a 32-bit
/// integer, is divided by 100.0 in the register.
pub fn time_elapsed<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, slot: Addr) -> F80 {
    let buf = m.local(16);
    let now = h.times(m, buf);
    m.release(16);
    let before = m.i32(slot);
    m.set_i32(slot, now);
    F80::from_i32(now.wrapping_sub(before)) / F80::from_f64(TICKS)
}

/// Shows how long the last frame took [soft 0x0820dcd0 SI_displayFrameRate]: the time since
/// the last call on `secPrev`, rounded to `float`; the prompt lines are "Frame rate: " with
/// 1 / time as `%6.2f` ("MAX" for a time of exactly 0) and "Seconds per frame: " with the
/// time. Returns the time.
///
/// The original formats into a 48-byte local; a rate of more than 44 characters overruns
/// it. Here the buffers are as long as the text.
pub fn display_frame_rate<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H) -> f32 {
    let dt = time_elapsed(m, h, g::SEC_PREV).to_f32();
    let rate = if F80::ZERO.eq(F80::from_f32(dt)) { "MAX".to_string() } else { fmt_6_2((F80::ONE / dt).to_f64()) };
    prompt(m, h, &format!("Frame rate: {rate}"), &format!("Seconds per frame: {}", fmt_6_2(dt as f64)));
    dt
}

fn prompt<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, a: &str, b: &str) {
    let (na, nb) = (a.len() as u32 + 1, b.len() as u32 + 1);
    let pa = m.local(na);
    let pb = m.local(nb);
    put_str(m, pa, a);
    put_str(m, pb, b);
    h.status_display_prompt(m, pa, pb);
    m.release(nb);
    m.release(na);
}

/// One turn of playback: the frame at a position is shown
/// [soft 0x0820dd70 playBackFrame]. Returns non-zero when the user interrupted (or 1 at
/// once in play-box mode −1).
///
/// 1. `SI_frameCurrent` = the position rounded to the nearest frame, halves away from zero:
///    `(int)(position + 0.5)` for a position above 0, `(int)(position − 0.5)` otherwise,
///    truncated.
/// 2. **The same frame as last time** (`prevFrame`): nothing is drawn; only the clock is
///    read, and with adaptive step the step becomes `|elapsed × rate| × direction`.
/// 3. **A new frame**: it is a *jump* when it is more than one frame from the last one and
///    not the wrap from the last frame to the first or back. The time since the last new
///    frame goes to the prompt ("Frame rate: 12.50", with a `*` in place of the blank after
///    a jump: frames were skipped; "MAX" for no measurable time).
/// 4. Then, `SI_framePause` + 1 times (not at all for a negative pause), or until the user
///    interrupts: the scene is refreshed at the time of the position (in the program:
///    [`scene_refresh_loop`] at `(float)position` rounded **towards zero to a whole frame**
///    and divided by the rate; in the play box: `SI_viewerUpdateScene(frame)` and the frame
///    slider); the clock is read; an open curve dialogue is updated; with adaptive step the
///    step becomes `|elapsed × rate| × direction` (elapsed: the 80-bit quotient of
///    [`time_elapsed`], unrounded); the mouse is polled.
///
/// Note 4: the frame number shown comes from step 1 (nearest), the time of the update from
/// the truncated position. At position 3.6, frame 4 is announced and the scene is updated
/// at frame 3; [`scene_refresh_loop`] then sets `SI_frameCurrent` to 3 again.
pub fn play_back_frame<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, pos: f64, step: Addr, dir: f64) -> i32 {
    let mode = m.u8(g::SI_PLAY_BOX_MODE);
    if mode == 0xff {
        return 1;
    }
    // SI_add(0, 0, 20) (0x0851a0c4) returns the sum of its first two arguments: 0
    let base = 0i32;
    let p = F80::from_f64(pos);
    let half = if F80::ZERO.lt(p) { 0.5f64 } else { -0.5f64 };
    let frame = base.wrapping_add((p + half).to_i32(Round::Zero));
    m.set_i32(glob::SI_FRAME_CURRENT, frame);
    let prev = m.i32(g::PREV_FRAME);
    let adaptive = |m: &mut Mem, t: F80| {
        if m.i16(g::SI_STEP_ADAPTIVE) != 0 {
            let v = ((t * glob::frame_rate(m)).abs() * dir).to_f64();
            m.set_f64(step, v);
        }
    };
    if frame == prev {
        let t = time_elapsed(m, h, g::SEC_PREV);
        adaptive(m, t);
        return 0;
    }
    let mut jump = false;
    if frame > prev.wrapping_add(1) || frame < prev.wrapping_sub(1) {
        let (end, start) = (m.i32(glob::SI_FRAME_END), m.i32(glob::SI_FRAME_START));
        if !(prev == end && frame == start) && !(prev == start && frame == end) {
            jump = true;
        }
    }
    let now = m.i32(glob::SI_FRAME_CURRENT);
    m.set_i32(g::PREV_FRAME, now);
    let dt = time_elapsed(m, h, g::SEC_PER_FRAME_PREV).to_f64();
    let rate = if F80::from_f64(dt).eq(F80::ZERO) { "MAX".to_string() } else { fmt_6_2((F80::ONE / dt).to_f64()) };
    let line = if jump { format!("Frame rate:*{rate}") } else { format!("Frame rate: {rate}") };
    prompt(m, h, &line, &format!("Seconds per frame: {}", fmt_6_2(dt)));
    let mut stop = 0i32;
    let mut i = 0;
    if i > m.i32(glob::SI_FRAME_PAUSE) {
        return stop;
    }
    loop {
        if m.u8(g::SI_PLAY_BOX_MODE) == 0 {
            // the position as a float, then to a whole number towards zero
            let x = F80::from_f32(p.to_f32());
            let whole = if F80::ZERO.gt(p) { x.round_int(Round::Up) } else { x.round_int(Round::Down) };
            let t = (F80::from_f32(whole.to_f32()) / glob::frame_rate(m)).to_f32();
            scene_refresh_loop(m, h, t);
        } else {
            let f = m.i32(glob::SI_FRAME_CURRENT);
            h.viewer_update_scene(m, f);
            let f = m.i32(glob::SI_FRAME_CURRENT);
            h.fslider_draw(m, f);
        }
        let t = time_elapsed(m, h, g::SEC_PREV);
        if m.u8(g::SI_PLAY_BOX_MODE) == 0 && m.i16(g::SI_DIALOG_ACTIVE) != 0 {
            h.dialog_update_fcv(m);
        }
        adaptive(m, t);
        stop = h.mouse_interrupt(m) as i32;
        i += 1;
        if i > m.i32(glob::SI_FRAME_PAUSE) || stop != 0 {
            break;
        }
    }
    stop
}

/// Plays a range [soft 0x0820e04c playBackFrames].
///
/// The position starts at `first` and lives in the 80-bit register: every turn hands it to
/// [`play_back_frame`] rounded to `double` and then adds the step (a `double`, `step` at
/// first, which adaptive playback rewrites every frame) **unrounded**. `mode`:
///
/// - **0** once forward: while position ≤ `last`.
/// - **3** once backward: while position ≥ `last` (the direction handed on is −1).
/// - **1** forward in a loop: turns while `loop_to` ≤ position ≤ `last`; then, without
///   `pingpong`, the position returns to `loop_to`; with it, the step and the direction
///   change sign and the position goes two (new) steps on, so that it turns round on the
///   frame before the one that left the range. Until the user interrupts.
/// - **4** backward in a loop: the same between `last` (below) and `loop_to` (above),
///   starting with direction −1.
/// - Any other mode plays nothing and leaves position 0.
///
/// The mouse is polled before the first frame, after every pass of a loop, and by every
/// turn; an interrupt stops the audio (`SI_audioContainerPlayAbort`). At the end
/// `SI_frameCurrent` is the position rounded to the nearest frame, halves away from zero.
/// Nothing happens in play-box mode −1.
pub fn play_back_frames<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, first: f32, last: f32, step: f32, mode: i32, pingpong: i32,
                                             loop_to: i32) {
    let step_at = m.local(8);
    m.set_f64(step_at, step as f64);
    if m.u8(g::SI_PLAY_BOX_MODE) == 0xff {
        m.release(8);
        return;
    }
    let mut stop = h.mouse_interrupt(m) as i32;
    h.status_display_mouse(m, g::TEXT_INTERRUPT[0], g::TEXT_INTERRUPT[1], g::TEXT_INTERRUPT[2]);
    let now = m.i32(glob::SI_FRAME_CURRENT);
    m.set_i32(g::PREV_FRAME, now);
    let mut pos = F80::ZERO;
    let last80 = F80::from_f32(last);
    let to = F80::from_i32(loop_to);
    let turn = |m: &mut Mem, h: &mut H, pos: &mut F80, dir: f64| -> i32 {
        let r = play_back_frame(m, h, pos.to_f64(), step_at, dir);
        *pos = *pos + m.f64(step_at);
        r
    };
    match mode {
        0 => {
            pos = F80::from_f32(first);
            if pos.le(last80) && stop == 0 {
                loop {
                    stop = turn(m, h, &mut pos, 1.0);
                    if !pos.le(last80) || stop != 0 {
                        break;
                    }
                }
            }
        }
        3 => {
            pos = F80::from_f32(first);
            if pos.ge(last80) && stop == 0 {
                loop {
                    stop = turn(m, h, &mut pos, -1.0);
                    if !pos.ge(last80) || stop != 0 {
                        break;
                    }
                }
            }
        }
        1 | 4 => {
            let forward = mode == 1;
            let mut dir = if forward { 1.0f64 } else { -1.0f64 };
            pos = F80::from_f32(first);
            let inside = |pos: F80| if forward { pos.le(last80) && to.le(pos) } else { pos.ge(last80) && to.ge(pos) };
            while stop == 0 {
                if inside(pos) {
                    loop {
                        stop = turn(m, h, &mut pos, dir);
                        if !inside(pos) || stop != 0 {
                            break;
                        }
                    }
                }
                if stop != 0 {
                    break;
                }
                if pingpong == 0 {
                    pos = to;
                } else {
                    let s = -m.f64(step_at);
                    m.set_f64(step_at, s);
                    dir = -dir;
                    pos = F80::from_f64(s) + (pos + s);
                }
                stop = h.mouse_interrupt(m) as i32;
            }
        }
        _ => {}
    }
    if stop != 0 {
        h.audio_container_play_abort(m, g::SI_AUDIO_CONTAINER);
    }
    let half = if F80::ZERO.lt(pos) { 0.5f64 } else { -0.5f64 };
    m.set_i32(glob::SI_FRAME_CURRENT, (pos + half).to_i32(Round::Zero));
    m.release(8);
}

/// The frame buttons of the play control [soft 0x0820f2d8 SI_sceneStepFrame].
///
/// - `next`: current + step (a step of 0 counts as 1); past the last frame it becomes the
///   first.
/// - else `previous`: current − step; before the first frame it becomes the last.
/// - else `first`: the first frame; else `last`: the last frame.
/// - none of the four: the frame stays, and the scene is refreshed with scene refresh
///   forced on (`refreshObj` set to 1 for the call and restored).
///
/// The audio is cued at the frame (`SI_audioPlaySetMode(container, 1, 1)`,
/// `SI_audioContainerPlayFrame`), the scene refreshed ([`scene_refresh_selection`]), and,
/// after a button, the selected curves redrawn (`SI_refreshSelectedFcurve(0)`).
pub fn scene_step_frame<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, next: i32, previous: i32, first: i32, last: i32) {
    let cur = m.i32(glob::SI_FRAME_CURRENT);
    let step = m.i32(glob::SI_FRAME_STEP);
    let mut wrap = Some(m.i32(glob::SI_FRAME_START));
    if next == 1 {
        let f = cur.wrapping_add(if step == 0 { 1 } else { step });
        m.set_i32(glob::SI_FRAME_CURRENT, f);
        if f <= m.i32(glob::SI_FRAME_END) {
            wrap = None;
        }
    } else if previous == 1 {
        let f = if step == 0 { cur.wrapping_sub(1) } else { cur.wrapping_sub(step) };
        m.set_i32(glob::SI_FRAME_CURRENT, f);
        wrap = Some(m.i32(glob::SI_FRAME_END));
        if m.i32(glob::SI_FRAME_START) <= f {
            wrap = None;
        }
    } else if first != 1 {
        wrap = Some(m.i32(glob::SI_FRAME_END));
        if last != 1 {
            wrap = None;
        }
    }
    if let Some(f) = wrap {
        m.set_i32(glob::SI_FRAME_CURRENT, f);
    }
    h.audio_play_set_mode(m, g::SI_AUDIO_CONTAINER, 1, 1);
    let f = m.i32(glob::SI_FRAME_CURRENT);
    h.audio_container_play_frame(m, g::SI_AUDIO_CONTAINER, f);
    let none = next == 0 && previous == 0 && first == 0 && last == 0;
    let mut saved = 0;
    if none {
        saved = update::get_refresh_obj(m);
        update::set_refresh_obj(m, 1);
    }
    scene_refresh_selection(m, h);
    if none {
        update::set_refresh_obj(m, saved);
    } else {
        h.refresh_selected_fcurve(m, 0);
    }
}

/// [soft 0x0820f3f8 SI_sceneFrameRefreshLoop]: [`scene_refresh_loop`] at the time of a
/// frame, `(float)frame / rate`.
pub fn scene_frame_refresh_loop<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, frame: i32) {
    let t = frame_time(m, frame);
    scene_refresh_loop(m, h, t);
}

/// What the three refresh routines share: the frame the PostScript preview is told, the
/// time converted back to a frame (nearest, halves away from zero).
fn postscript<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, frame: i32) {
    if m.i16(g::SI_PS_PREV_FLAG) == 1 {
        h.postscript_dump_frame(m, frame);
    }
}

/// Brings the scene and the screen to the current frame [soft 0x0820f470 SI_sceneRefresh].
///
/// The time is `(float)SI_frameCurrent / rate`. In the interactive program the drawing
/// window and mode are saved and set (`winset(SI_sysid)`, `drawmode(0x10)`); if fast
/// playback has the frame cached (`SI_fastGetDirty(NULL, frame, 0)` is 0) only the frame
/// slider is drawn, otherwise the scene is updated at the time. Then everything is redrawn
/// (`SI_refreshSystem(NULL, NULL, 0x37)`, then `0x17`), a PostScript preview gets the frame
/// (the time converted back: nearest frame), and the window and mode are restored.
pub fn scene_refresh<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H) {
    let t = frame_time(m, m.i32(glob::SI_FRAME_CURRENT));
    let interactive = m.i16(glob::SI_STANDALONE_RENDERER) == 0;
    let (mut win, mut mode) = (0, 0);
    let mut cached = false;
    if interactive {
        win = h.winget(m);
        mode = h.getdrawmode(m);
        let id = m.i32(g::SI_SYSID);
        h.winset(m, id);
        h.drawmode(m, 0x10);
        if m.i16(g::SI_FAST_PLAY_BACK_ACTIVE) != 0 {
            let f = m.i32(glob::SI_FRAME_CURRENT);
            if h.fast_get_dirty(m, 0, f, 0) == 0 {
                let f = m.i32(glob::SI_FRAME_CURRENT);
                h.fslider_draw(m, f);
                cached = true;
            }
        }
    }
    if !cached {
        h.scene_update(m, t);
    }
    h.refresh_system(m, 0, 0, 0x37);
    h.refresh_system(m, 0, 0, 0x17);
    let f = sec_to_frame(m, t);
    postscript(m, h, f);
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 {
        h.winset(m, win);
        h.drawmode(m, mode);
    }
}

/// The refresh of one playback turn [soft 0x0820f620 sceneRefreshLoop].
///
/// `SI_frameCurrent` becomes the frame of the time (nearest, halves away from zero) and
/// the audio plays that frame. In the interactive program: with the frame in the
/// fast-playback cache nothing more is computed; otherwise the scene is updated at the
/// time, the first visible window that looks through the current camera has its grid
/// redrawn (when it shows one, and fast playback is off or wants the frame for that
/// window), and the selected curves are redrawn. Then the screen is refreshed
/// (`SI_refreshSystem(NULL, current graf list, 0x67)`, 0x65 when only the selection is
/// updated) and a PostScript preview gets the frame.
pub fn scene_refresh_loop<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H, time: f32) {
    let frame = sec_to_frame(m, time);
    m.set_i32(glob::SI_FRAME_CURRENT, frame);
    h.audio_container_play_frame(m, g::SI_AUDIO_CONTAINER, frame);
    let mut win = 0;
    let mut cached = false;
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 {
        win = h.winget(m);
        let id = m.i32(g::SI_SYSID);
        h.winset(m, id);
        if m.i16(g::SI_FAST_PLAY_BACK_ACTIVE) != 0 && h.fast_get_dirty(m, 0, frame, 0) == 0 {
            cached = true;
        }
    }
    if !cached {
        h.scene_update(m, time);
        let slot = m.local(4);
        let mut found = false;
        while !found {
            if h.window_get_next_visible(m, slot) == 0 {
                break;
            }
            let p = m.ptr(slot);
            if m.ptr(p + port::CAMERA) == m.ptr(m.ptr(glob::SI_PROC_DATA) + 8) {
                found = true;
            }
        }
        if found {
            let p = m.ptr(slot);
            if m.i16(p + port::HAS_GRID) == 1
                && (m.i16(g::SI_FAST_PLAY_BACK_ACTIVE) == 0 || (m.i16(p + port::FAST) != 0 && h.fast_get_dirty(m, p, frame, 0) != 0))
            {
                h.draw_grid(m, p, 0);
            }
        }
        m.release(4);
        h.refresh_selected_fcurve(m, 0);
    }
    let code = if m.i32(glob::SI_UPDATE_OPTION) != 1 { 0x67 } else { 0x65 };
    let gl = glob::cur_graf_list(m);
    h.refresh_system(m, 0, gl, code);
    postscript(m, h, frame);
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 {
        h.winset(m, win);
    }
}

/// The refresh after a frame button [soft 0x0820f7e0 SI_sceneRefreshSelection]: as
/// [`scene_refresh`] without the drawing mode, and with the redraw codes 0x35 and 0x15 in
/// place of 0x37 and 0x17 when only the selection is updated (`SI_updateOption` = 1).
pub fn scene_refresh_selection<H: PlayHost + ?Sized>(m: &mut Mem, h: &mut H) {
    let t = frame_time(m, m.i32(glob::SI_FRAME_CURRENT));
    let mut win = 0;
    let mut cached = false;
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 {
        win = h.winget(m);
        let id = m.i32(g::SI_SYSID);
        h.winset(m, id);
        if m.i16(g::SI_FAST_PLAY_BACK_ACTIVE) != 0 {
            let f = m.i32(glob::SI_FRAME_CURRENT);
            if h.fast_get_dirty(m, 0, f, 0) == 0 {
                cached = true;
            }
        }
    }
    if !cached {
        h.scene_update(m, t);
    }
    let all = m.i32(glob::SI_UPDATE_OPTION) != 1;
    h.refresh_system(m, 0, 0, if all { 0x37 } else { 0x35 });
    let all = m.i32(glob::SI_UPDATE_OPTION) != 1;
    h.refresh_system(m, 0, 0, if all { 0x17 } else { 0x15 });
    let f = sec_to_frame(m, t);
    postscript(m, h, f);
    if m.i16(glob::SI_STANDALONE_RENDERER) == 0 {
        h.winset(m, win);
    }
}
