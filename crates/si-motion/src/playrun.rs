// SPDX-License-Identifier: MIT
//! The playback command of `motion.c` (finding 0017, section 9b): play, loop, step, and the
//! key browser, on the range the user has in front of him.
//!
//! Written from the instruction listing in the fifth pass, measured in the sixth: held
//! against the listing again and run against the original in the emulation harness on 2,000
//! made-up cases (finding 0017, sections 9b and 12). The second reading found one fault,
//! in four places: a frame number was rounded to `float` before it was held against a
//! bound, where the original compares the integer ([`below`], [`above`]).

use crate::glob;
use crate::nextkey::{self, BrowseHost, SI_SCNGL};
use crate::path::PathHost;
use crate::play::{self, g};
use si_mem::{Addr, Mem};
use si_x87::{fmov, Round, F80};

/// Globals of the command (symbol table of the Linux binary).
pub mod gl {
    use si_mem::Addr;
    /// `SI_displayKeyF`, `SI_playBackRun`, `SI_reversePlayF`: 16-bit flags raised while the
    /// key browser, playback, backward playback runs.
    pub const SI_DISPLAY_KEY_F: Addr = 0x0869_5e90;
    pub const SI_PLAY_BACK_RUN: Addr = 0x0869_5e92;
    pub const SI_REVERSE_PLAY_F: Addr = 0x0869_5e94;
    /// `SI_tabletButton`: the tablet button held, as a device code.
    pub const SI_TABLET_BUTTON: Addr = 0x0867_94b4;
    /// `SI_dopeSh` (0x0878faa0), the dope sheet's record, as far as playback reads it:
    /// + 8 a region is selected (16 bits), + 0xc its first frame and + 0xe the frame behind
    /// its last (16 bits each); + 0x3c the current action's record (its start and end in
    /// seconds at + 8 and + 0xc of it), + 0x6c the current action's index (negative: none),
    /// + 0x70 a pointer to the actions shown.
    pub const SI_DOPE_SH: Addr = 0x0878_faa0;
    pub const DOPE_REGION: u32 = 0x08;
    pub const DOPE_REGION_START: u32 = 0x0c;
    pub const DOPE_REGION_END: u32 = 0x0e;
    pub const DOPE_ACTION: u32 = 0x3c;
    pub const DOPE_ACTION_START: u32 = 0x44;
    pub const DOPE_ACTION_END: u32 = 0x48;
    pub const DOPE_ACTION_INDEX: u32 = 0x6c;
    pub const DOPE_ACTIONS: u32 = 0x70;
    /// `SI_audioContainer` (0x087fb9c0), as far as playback reads it: + 0x7c a word handed
    /// to the marker look-ups; + 0x80 a region is selected (16 bits), + 0x84 its first frame,
    /// + 0x86 the frame behind its last; + 0x94 markers are active (16 bits).
    pub const AUDIO_MARKERS_ARG: u32 = 0x7c;
    pub const AUDIO_REGION: u32 = 0x80;
    pub const AUDIO_REGION_START: u32 = 0x84;
    pub const AUDIO_REGION_END: u32 = 0x86;
    pub const AUDIO_MARKERS: u32 = 0x94;
    /// An audio marker: its first frame (+ 8) and the frame behind its last (+ 0xa), 16 bits.
    pub const MARKER_START: u32 = 0x08;
    pub const MARKER_END: u32 = 0x0a;
    /// "The frame step is set to 0..." (two copies), "No valid item currently selected.",
    /// and four empty strings (the status line and the three mouse prompts cleared).
    pub const TEXT_STEP_0: [Addr; 2] = [0x0869_6264, 0x0869_6282];
    pub const TEXT_NO_ITEM: Addr = 0x0869_62a0;
    pub const TEXT_EMPTY: Addr = 0x0869_62c2;
    pub const TEXT_MOUSE: [Addr; 3] = [0x0869_62c5, 0x0869_62c4, 0x0869_62c3];
}

/// Fields of a window (`__SI_Grafport`) the command reads: + 6 visible (16 bits), + 0x2c a
/// record whose 16-bit word + 4 is the window's kind (0x7d8: the dope sheet).
mod port {
    pub const VISIBLE: u32 = 0x06;
    pub const INFO: u32 = 0x2c;
    pub const INFO_KIND: u32 = 0x04;
}

crate::host_trait! {
    /// What the playback command calls outside this package.
    pub trait RunHost: BrowseHost, PathHost {
        /// `SI_windowGetNextActive(&window)` (0x083d1ca4): steps to the next active window;
        /// 0 at the end.
        fn window_get_next_active(port: Addr) -> i16;
        /// `SI_actionsCountSelected(actions)` (0x081e4678).
        fn actions_count_selected(actions: Addr) -> i32;
        /// `SI_actionsGetMinStart(actions, 1, &seconds)` (0x081e0ea0),
        /// `SI_actionsGetMaxEnd(actions, 1, &seconds)` (0x081e0f24): of the selected actions.
        fn actions_get_min_start(actions: Addr, selected: i32, out: Addr);
        fn actions_get_max_end(actions: Addr, selected: i32, out: Addr);
        /// `SI_actionsElemGetLength(action)` (0x081e55f0).
        fn actions_elem_get_length(action: Addr) -> i32;
        /// `SI_audioMarkerMngFindFirstActive(container, word)` (0x08374ccc),
        /// `SI_audioMarkerMngFindLastActive(container, word)` (0x08374cfc): a marker record.
        fn audio_marker_mng_find_first_active(container: Addr, word: i32) -> Addr;
        fn audio_marker_mng_find_last_active(container: Addr, word: i32) -> Addr;
        /// `SI_fastPlayClearAllView()` (0x0832a7c8).
        fn fast_play_clear_all_view();
    }
}

/// `(float)frame / SI_frameRate`, rounded to `float`.
fn frame_time(m: &Mem, frame: i32) -> f32 {
    (F80::from_f32(F80::from_i32(frame).to_f32()) / glob::frame_rate(m)).to_f32()
}

/// A frame number held as a `float`, truncated towards zero.
fn whole(v: f32) -> i32 {
    F80::from_f32(v).to_i32(Round::Zero)
}

/// An integer frame against a `float` bound as the unit compares them (`fild`, `fcomp`):
/// the integer is not rounded to `float` first, so beyond 2²⁴ a frame can lie strictly
/// below or above a bound it would round onto.
fn below(frame: i32, bound: f32) -> bool {
    F80::from_i32(frame).lt(F80::from_f32(bound))
}

fn above(frame: i32, bound: f32) -> bool {
    F80::from_i32(frame).gt(F80::from_f32(bound))
}

/// The playback command [soft 0x0820e39c SI_playbackRun]. `mode`: 0 play, 1 loop, 2 one
/// step forward, 3 play backward, 4 loop backward, 5 one step back, 6 next key, 7 previous
/// key, 8 and 9 the same going round at the ends; `pingpong` for the loops; `from_current`
/// to start at the current frame. Nothing in play-box mode −1.
///
/// 1. The two clocks are read (`timeElapsed` on `secPrev` and `secPerFramePrev`). In the
///    program the curve editor's list and the current graf list are told
///    (`SI_refreshSystem(0, list, 0x47)`) and `SI_playBackRun` is raised.
/// 2. **The range** is first frame to last frame, as `float`s. With `from_current` its
///    start (modes 0, 1, 2, 6, 8) or its end (the others) becomes the current frame.
/// 3. **Narrowing**, only when a visible window of kind 0x7d8 (the dope sheet) is among the
///    active windows, only in the program (play-box mode 0), and by the first of these
///    that applies: the dope sheet's region when it is more than one frame long; the
///    selected actions (from the least start to the greatest end, as frames); the current
///    action when it has a length; the audio region; the active audio markers (from the
///    first one's start to the last one's end). The narrowed start is never below the first
///    frame and the narrowed end never above the last; a region's end is the frame before
///    the one stored. With `from_current` the start is then raised to the current frame
///    (forward modes) or the end lowered to it (backward modes), which here, unlike in 2,
///    never widens the range.
/// 4. The step is `(float)SI_frameStep`; under adaptive step the original leaves its local
///    unset (0 here: every turn of playback rewrites the step before using it, except with
///    a negative `SI_framePause`, which draws nothing).
/// 5. In the program the fast-playback views are cleared; in the stand-alone viewer the
///    command waits until the three mouse buttons (devices 0x67, 0x66, 0x65; also as tablet
///    buttons) are up. The audio is put in play mode (`SI_audioPlaySetMode(…, 0, 1)`).
/// 6. By mode: 0 and 1 [`play::play_back_frames`] from the start to the end, a loop
///    returning to the **first frame of the narrowed range**; 3 and 4 the same from the end
///    to the start with the step negated, a loop returning to the last frame of the range
///    (3 raises `SI_reversePlayF`). 2 and 5 move the current frame by the frame step ("The
///    frame step is set to 0..." for a step of 0), wrapping to the range's start when it
///    passes the end and the other way round, and show it: with a refresh (`pingpong` 0)
///    by [`play::scene_frame_refresh_loop`] (viewer: `SI_viewerUpdateScene` and the
///    slider), without one by the key marks (when key colouring is on), the slider and an
///    open curve dialogue (viewer: the slider). 6 to 9 raise `SI_displayKeyF` and run the
///    key browser ([`nextkey::next_key_display`]); when it says nothing was selected:
///    "No valid item currently selected.".
/// 7. `SI_playBackRun` is lowered (viewer: the status line cleared). The current frame is
///    brought back into first frame … last frame. With fast playback active the scene is
///    updated at it (`SI_sceneUpdate`). Except for the two step modes, and only when a
///    refresh is wanted (no `pingpong`, or a play or loop mode), the scene is refreshed
///    ([`play::scene_refresh_selection`]; viewer: `SI_viewerUpdateScene` and the slider).
///    The two flags are lowered, the mouse prompts cleared, the audio mode restored
///    (`SI_audioPlaySetMode(…, 1, 0)`).
pub fn playback_run<H: RunHost + ?Sized>(m: &mut Mem, h: &mut H, mode: i32, pingpong: i16, from_current: i16) {
    let mut valid: i16 = 1;
    let refresh: i16 = (pingpong == 0 || mode == 1 || mode == 4 || mode == 0 || mode == 3) as i16;
    let playbox = |m: &Mem| m.u8(g::SI_PLAY_BOX_MODE);
    if playbox(m) == 0xff {
        return;
    }
    play::time_elapsed(m, h, g::SEC_PREV);
    play::time_elapsed(m, h, g::SEC_PER_FRAME_PREV);
    if playbox(m) == 0 {
        let scngl = m.ptr(SI_SCNGL);
        h.refresh_system(m, 0, scngl, 0x47);
        let list = glob::cur_graf_list(m);
        h.refresh_system(m, 0, list, 0x47);
        m.set_i16(gl::SI_PLAY_BACK_RUN, 1);
    }
    // the play range (lo, hi) and the range played (a, b), all as floats
    let mut lo = m.i32(glob::SI_FRAME_START) as f32;
    let mut a = lo;
    let mut hi = m.i32(glob::SI_FRAME_END) as f32;
    let mut b = hi;
    let forward = (mode as u32) <= 2 || mode == 6 || mode == 8;
    if from_current != 0 {
        let cur = m.i32(glob::SI_FRAME_CURRENT) as f32;
        if forward {
            a = cur;
        } else {
            b = cur;
        }
    }
    let win = m.local(4);
    m.set_ptr(win, 0);
    let mut dope_open = false;
    while h.window_get_next_active(m, win) != 0 {
        let w = m.ptr(win);
        if m.i16(w + port::VISIBLE) != 0 && m.i16(m.ptr(w + port::INFO) + port::INFO_KIND) == 0x7d8 {
            dope_open = true;
            break;
        }
    }
    m.release(4);
    if dope_open {
        let dope = gl::SI_DOPE_SH;
        let audio = g::SI_AUDIO_CONTAINER;
        // a region: its first frame, and the frame behind its last (read twice, as there)
        let region = |m: &Mem, lo: f32, hi: f32, start: Addr, end: Addr| -> (f32, f32) {
            let mut a = m.i16(start) as f32;
            if lo > a {
                a = lo;
            }
            let b = if ((m.i16(end) as i32 - 1) as f32) > hi { hi } else { (m.i16(end) as i32 - 1) as f32 };
            (a, b)
        };
        // two times in seconds: each is converted to a frame, and again when it is taken.
        // The frame is held against the bound as the integer it is; only the one taken is
        // rounded to `float`.
        let span = |m: &mut Mem, h: &mut H, lo: f32, hi: f32, start: f32, end: f32| -> (f32, f32) {
            let a = if below(h.actions_sec_to_start_frm(m, start), lo) { lo } else { h.actions_sec_to_start_frm(m, start) as f32 };
            let b = if above(h.actions_sec_to_end_frm(m, end), hi) { hi } else { h.actions_sec_to_end_frm(m, end) as f32 };
            (a, b)
        };
        let narrowed = if playbox(m) == 0
                          && m.i16(dope + gl::DOPE_REGION) != 0
                          && m.i16(dope + gl::DOPE_REGION_END) as i32 - m.i16(dope + gl::DOPE_REGION_START) as i32 > 1
        {
            Some(region(m, lo, hi, dope + gl::DOPE_REGION_START, dope + gl::DOPE_REGION_END))
        } else if playbox(m) == 0 && {
            let actions = m.ptr(dope + gl::DOPE_ACTIONS);
            h.actions_count_selected(m, actions) > 0
        } {
            let out = m.local(8);
            let actions = m.ptr(dope + gl::DOPE_ACTIONS);
            h.actions_get_min_start(m, actions, 1, out + 4);
            let actions = m.ptr(dope + gl::DOPE_ACTIONS);
            h.actions_get_max_end(m, actions, 1, out);
            let (start, end) = (fmov(m.f32(out + 4)), fmov(m.f32(out)));
            m.release(8);
            Some(span(m, h, lo, hi, start, end))
        } else if playbox(m) == 0 && m.i32(dope + gl::DOPE_ACTION_INDEX) >= 0 && h.actions_elem_get_length(m, dope + gl::DOPE_ACTION) > 0 {
            let (start, end) = (fmov(m.f32(dope + gl::DOPE_ACTION_START)), fmov(m.f32(dope + gl::DOPE_ACTION_END)));
            Some(span(m, h, lo, hi, start, end))
        } else if playbox(m) == 0 && m.i16(audio + gl::AUDIO_REGION) != 0 {
            Some(region(m, lo, hi, audio + gl::AUDIO_REGION_START, audio + gl::AUDIO_REGION_END))
        } else if playbox(m) == 0 && m.i16(audio + gl::AUDIO_MARKERS) != 0 {
            let first = h.audio_marker_mng_find_first_active(m, audio, m.i32(audio + gl::AUDIO_MARKERS_ARG));
            let last = h.audio_marker_mng_find_last_active(m, audio, m.i32(audio + gl::AUDIO_MARKERS_ARG));
            let a = if (m.i16(first + gl::MARKER_START) as f32) < lo { lo } else { m.i16(first + gl::MARKER_START) as f32 };
            let b = if ((m.i16(last + gl::MARKER_END) as i32 - 1) as f32) > hi { hi } else { (m.i16(last + gl::MARKER_END) as i32 - 1) as f32 };
            Some((a, b))
        } else {
            None
        };
        if let Some((na, nb)) = narrowed {
            a = na;
            lo = na;
            b = nb;
            hi = nb;
            if from_current != 0 {
                let cur = m.i32(glob::SI_FRAME_CURRENT);
                if forward {
                    if above(cur, a) {
                        a = cur as f32;
                    }
                } else if below(cur, b) {
                    b = cur as f32;
                }
            }
        }
    }
    // the original's local is not set under adaptive step
    let mut step: f32 = 0.0;
    if m.i16(g::SI_STEP_ADAPTIVE) == 0 {
        step = m.i32(glob::SI_FRAME_STEP) as f32;
    }
    if playbox(m) == 0 {
        h.fast_play_clear_all_view(m);
    } else {
        // until the three buttons are up
        loop {
            if [0x67, 0x66, 0x65].iter().all(|&button| h.getbutton(m, button) == 0 && m.i32(gl::SI_TABLET_BUTTON) != button) {
                break;
            }
        }
    }
    h.audio_play_set_mode(m, g::SI_AUDIO_CONTAINER, 0, 1);
    match mode {
        0 => play::play_back_frames(m, h, a, b, step, mode, pingpong as i32, -1),
        1 => play::play_back_frames(m, h, a, b, step, mode, pingpong as i32, whole(lo)),
        3 => {
            play::play_back_frames(m, h, b, a, -step, mode, pingpong as i32, -1);
            m.set_i16(gl::SI_REVERSE_PLAY_F, 1);
        }
        4 => play::play_back_frames(m, h, b, a, -step, mode, pingpong as i32, whole(hi)),
        2 | 5 => {
            if m.i32(glob::SI_FRAME_STEP) == 0 {
                crate::key::KeyHost::status_display_message(h, m, gl::TEXT_STEP_0[(mode == 5) as usize], 2);
            }
            if mode == 2 {
                let next = m.i32(glob::SI_FRAME_STEP).wrapping_add(m.i32(glob::SI_FRAME_CURRENT));
                m.set_i32(glob::SI_FRAME_CURRENT, next);
                if above(next, b) {
                    m.set_i32(glob::SI_FRAME_CURRENT, whole(a));
                }
            } else {
                let next = m.i32(glob::SI_FRAME_CURRENT).wrapping_sub(m.i32(glob::SI_FRAME_STEP));
                m.set_i32(glob::SI_FRAME_CURRENT, next);
                if below(next, a) {
                    m.set_i32(glob::SI_FRAME_CURRENT, whole(b));
                }
            }
            let cur = m.i32(glob::SI_FRAME_CURRENT);
            if refresh != 0 {
                if playbox(m) == 0 {
                    play::scene_frame_refresh_loop(m, h, cur);
                } else {
                    h.viewer_update_scene(m, cur);
                    let cur = m.i32(glob::SI_FRAME_CURRENT);
                    h.fslider_draw(m, cur);
                }
            } else if playbox(m) != 0 {
                h.fslider_draw(m, cur);
            } else {
                if m.i16(glob::SI_GLB_ANIM + glob::anim::KEY_COLOR) != 0 {
                    let t = frame_time(m, cur);
                    h.scene_key_set_color(m, t, 1, 0);
                    h.refresh_selected_both_buffers(m, 0);
                }
                let cur = m.i32(glob::SI_FRAME_CURRENT);
                h.fslider_draw(m, cur);
                if m.i16(g::SI_DIALOG_ACTIVE) != 0 {
                    h.dialog_update_fcv(m);
                }
            }
        }
        6..=9 => {
            m.set_i16(gl::SI_DISPLAY_KEY_F, 1);
            let (forward, wrap) = ((mode == 6 || mode == 8) as i16, (mode >= 8) as i16);
            valid = nextkey::next_key_display(m, h, forward, wrap, refresh);
        }
        _ => {}
    }
    if valid == 0 {
        crate::key::KeyHost::status_display_message(h, m, gl::TEXT_NO_ITEM, 3);
    }
    if playbox(m) == 0 {
        m.set_i16(gl::SI_PLAY_BACK_RUN, 0);
    } else {
        crate::key::KeyHost::status_display_message(h, m, gl::TEXT_EMPTY, 0);
    }
    let cur = m.i32(glob::SI_FRAME_CURRENT);
    if cur < m.i32(glob::SI_FRAME_START) {
        let v = m.i32(glob::SI_FRAME_START);
        m.set_i32(glob::SI_FRAME_CURRENT, v);
    } else if cur > m.i32(glob::SI_FRAME_END) {
        let v = m.i32(glob::SI_FRAME_END);
        m.set_i32(glob::SI_FRAME_CURRENT, v);
    }
    if playbox(m) == 0 && m.i16(g::SI_FAST_PLAY_BACK_ACTIVE) != 0 {
        let t = frame_time(m, m.i32(glob::SI_FRAME_CURRENT));
        h.scene_update(m, t);
    }
    if mode != 2 && mode != 5 && refresh != 0 {
        if playbox(m) == 0 {
            play::scene_refresh_selection(m, h);
        } else {
            let cur = m.i32(glob::SI_FRAME_CURRENT);
            h.viewer_update_scene(m, cur);
            let cur = m.i32(glob::SI_FRAME_CURRENT);
            h.fslider_draw(m, cur);
        }
    }
    m.set_i16(gl::SI_REVERSE_PLAY_F, 0);
    m.set_i16(gl::SI_DISPLAY_KEY_F, 0);
    h.status_display_mouse(m, gl::TEXT_MOUSE[0], gl::TEXT_MOUSE[1], gl::TEXT_MOUSE[2]);
    h.audio_play_set_mode(m, g::SI_AUDIO_CONTAINER, 1, 0);
}
