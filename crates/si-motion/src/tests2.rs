// SPDX-License-Identifier: MIT
//! Unit tests of the path, playback, lattice and utility routines on made-up memory.
//! Expected values are worked by hand from the doc comments (finding 0017).

use crate::fcv;
use crate::glob;
use crate::host::{Arg, Ret, Trace};
use crate::lattice;
use si_mem::{Addr, Mem};
use crate::off::{fcurve as fo, key as ko, spline};
use crate::path;
use crate::play;
use si_x87::F80;

fn scene() -> Mem {
    let mut m = Mem::new();
    glob::init_defaults(&mut m);
    m.set_i32(glob::SI_FRAME_START, 1);
    m.set_i32(glob::SI_FRAME_END, 100);
    m
}

fn key(m: &Mem, curve: Addr, i: u32) -> Addr {
    m.ptr(curve + fo::KEYS) + i * ko::SIZE
}

#[test]
fn path_relations_and_timing() {
    let mut m = scene();
    assert_eq!([2, 4, 5, 6, 7, 0xb, 9].map(path::rel_get_path_rel_from_graf_type), [112, 1112, 1114, 2112, 2114, 612, -1]);
    // keys at 1 s and 2.5 s, 30 frames a second
    let c = fcv::make(&mut m, 7, 1, &[(1.0, 0.0), (2.5, 100.0)]);
    assert_eq!(path::get_path_timing(&m, c), (30, 75));
    let one = fcv::make(&mut m, 7, 1, &[(1.0, 0.0)]);
    assert_eq!(path::get_path_timing(&m, one), (1, 100));
    assert_eq!(path::get_path_timing(&m, 0), (1, 100));
}

/// A spline of `n` points whose length table has `step` samples a segment.
fn spline_with(m: &mut Mem, n: u16, step: u16, lengths: &[f32]) -> Addr {
    let s = m.si_calloc(1, 0x74);
    m.set_u16(s + spline::NB_POINTS, n);
    m.set_u16(s + spline::LENGTH_STEP, step);
    let t = m.si_calloc(lengths.len() as u32, 4);
    for (i, v) in lengths.iter().enumerate() {
        m.set_f32(t + 4 * i as u32, *v);
    }
    m.set_ptr(s + spline::LENGTHS, t);
    s
}

#[test]
fn path_keys_follow_their_points() {
    let mut m = scene();
    // three points, two samples a segment: the points lie at 0, 1 and 4 of a length of 4
    let s = spline_with(&mut m, 3, 2, &[0.0, 0.5, 1.0, 2.0, 4.0]);
    let c = fcv::make(&mut m, 7, 1, &[(0.0, 0.0), (1.0, 50.0), (2.0, 60.0), (3.0, 100.0)]);
    let mut t = Trace::with(|_, name, _| if name == "spline_nb_segment" { Ret::int(2) } else { Ret::default() });
    // four keys on three points: keys 0 and 1 take the points 0 and 1, key 2 loses its
    // point, the last key takes the last point
    assert_eq!(path::key_spline_to_fcurve(&mut m, &mut t, c, s), 1);
    let idx: Vec<i16> = (0..4).map(|i| m.i16(key(&m, c, i) + ko::SPLINE_INDEX)).collect();
    let typ: Vec<u16> = (0..4).map(|i| m.u16(key(&m, c, i) + ko::FLAGS) & 0xf).collect();
    assert_eq!(idx, [0, 1, -1, 2]);
    assert_eq!(typ, [2, 2, 0, 2]);
    assert_eq!(m.u16(s + spline::KEYFRAMED), 1);
    // values: 100 × length to the point / whole length; the free key keeps its 60
    let val: Vec<f32> = (0..4).map(|i| m.f32(key(&m, c, i) + ko::VALUE)).collect();
    assert_eq!(val, [0.0, 25.0, 60.0, 100.0]);
    assert_eq!(t.names(), ["spline_nb_segment"]);
    // a point inserted before point 1, then point 0 deleted
    assert_eq!(path::add_fcurve_path_key(&mut m, c, 1), 1);
    let idx: Vec<i16> = (0..4).map(|i| m.i16(key(&m, c, i) + ko::SPLINE_INDEX)).collect();
    assert_eq!(idx, [0, 2, -1, 3]);
    assert_eq!(path::delete_fcurve_path_key(&mut m, &mut t, c, 0), 1);
    assert_eq!(m.i32(c + fo::NB_KEYS), 3);
    let idx: Vec<i16> = (0..3).map(|i| m.i16(key(&m, c, i) + ko::SPLINE_INDEX)).collect();
    assert_eq!(idx, [1, -1, 2]);
    assert!(m.fault().is_none());
}

#[test]
fn a_playback_turn_shows_the_nearest_frame_and_updates_at_the_truncated_one() {
    let mut m = scene();
    let step = m.si_calloc(1, 8);
    m.set_f64(step, 1.0);
    m.set_i32(play::g::PREV_FRAME, 2);
    let mut t = Trace::with(|_, name, _| if name == "times" { Ret::int(250) } else { Ret::default() });
    assert_eq!(play::play_back_frame(&mut m, &mut t, 3.5, step, 1.0), 0);
    // position 3.5: frame 4 is announced (halves away from zero) …
    assert_eq!(m.i32(play::g::PREV_FRAME), 4);
    // … and the scene is updated at frame 3, time 3/30, which is where the frame counter ends
    let upd = t.calls.iter().find(|c| c.name == "scene_update").unwrap();
    assert_eq!(upd.args, [Arg::f((F80::from_f32(3.0) / 30.0f32).to_f32())]);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 3);
    // adaptive step off: the step is untouched
    assert_eq!(m.f64(step), 1.0);
    // the same frame again: only the clock is read; with adaptive step, elapsed × rate
    m.set_i16(play::g::SI_STEP_ADAPTIVE, 1);
    m.set_i32(play::g::PREV_FRAME, 4);
    m.set_i32(play::g::SEC_PREV, 240);
    let mut t = Trace::with(|_, name, _| if name == "times" { Ret::int(250) } else { Ret::default() });
    assert_eq!(play::play_back_frame(&mut m, &mut t, 3.5, step, -1.0), 0);
    assert_eq!(t.names(), ["times"]);
    // 10 ticks = 0.1 s = 3 frames, backwards
    assert_eq!(m.f64(step), ((F80::from_f64(0.1) * 30.0f32).abs() * -1.0f64).to_f64());
    assert!(m.fault().is_none());
}

#[test]
fn playing_a_range_once_ends_one_step_past_it() {
    let mut m = scene();
    let mut t = Trace::new();
    play::play_back_frames(&mut m, &mut t, 1.0, 3.0, 1.0, 0, 0, -1);
    // turns at 1, 2, 3; the position ends at 4
    assert_eq!(t.calls.iter().filter(|c| c.name == "scene_update").count(), 3);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 4);
    // a mode that plays nothing leaves frame 0
    play::play_back_frames(&mut m, &mut t, 1.0, 3.0, 1.0, 2, 0, -1);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 0);
}

#[test]
fn frame_buttons_wrap() {
    let mut m = scene();
    let mut t = Trace::new();
    m.set_i32(glob::SI_FRAME_STEP, 0);
    m.set_i32(glob::SI_FRAME_CURRENT, 100);
    play::scene_step_frame(&mut m, &mut t, 1, 0, 0, 0);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 1);
    play::scene_step_frame(&mut m, &mut t, 0, 1, 0, 0);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 100);
    m.set_i32(glob::SI_FRAME_STEP, 7);
    play::scene_step_frame(&mut m, &mut t, 0, 1, 0, 0);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 93);
    play::scene_step_frame(&mut m, &mut t, 0, 0, 1, 0);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 1);
    play::scene_step_frame(&mut m, &mut t, 0, 0, 0, 1);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 100);
}

#[test]
fn lattice_keys_are_walked_in_a_ring() {
    let mut m = scene();
    let mdl = m.si_calloc(1, 0x430);
    let arr = m.si_calloc(4, 4);
    for i in 0..4u32 {
        let l = m.si_calloc(1, 0x1c);
        m.set_i16(l + lattice::lat::NX, 1);
        m.set_i16(l + lattice::lat::NY, 1);
        m.set_i16(l + lattice::lat::NZ, 1);
        let p = m.si_calloc(3, 4);
        m.set_f32(p, i as f32);
        let f = m.si_calloc(1, 2);
        m.set_ptr(l + lattice::lat::POINTS, p);
        m.set_ptr(l + lattice::lat::FLAGS, f);
        m.set_ptr(arr + 4 * i, l);
    }
    m.set_u16(mdl + lattice::mo::NODE_COUNT, 4);
    m.set_ptr(mdl + lattice::mo::NODE_LATTICES, arr);
    let mut t = Trace::new();
    let x = |m: &Mem| m.f32(m.ptr(m.ptr(arr) + lattice::lat::POINTS));
    // no current key: next is key 1; then 2, 3, and round to 1
    let mut seen = Vec::new();
    for _ in 0..4 {
        seen.push(lattice::model_set_lattice(&mut m, &mut t, mdl, 1, 0));
    }
    assert_eq!(seen, [0, 1, 2, 0]);
    assert_eq!(x(&m), 1.0);
    // previous from key 1 is the last key
    assert_eq!(lattice::model_set_lattice(&mut m, &mut t, mdl, 0, 0), 2);
    assert_eq!(x(&m), 3.0);
    assert!(m.fault().is_none());
}
