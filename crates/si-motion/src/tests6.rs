// SPDX-License-Identifier: MIT
//! Unit tests of the sixth pass on made-up memory: the four things the pass changed in the
//! code. Expected values are worked by hand from the doc comments (finding 0017, sections
//! 9b, 9f and 11); the measurements against the original are section 12's.

use crate::assoc;
use crate::fcv;
use crate::glob;
use crate::host::{Arg, Ret, Trace};
use crate::off::{fcurve as fo, key as ko};
use crate::playrun;
use si_mem::{Addr, Mem};

fn key_time(m: &Mem, curve: Addr, i: u32) -> f32 {
    m.f32(m.ptr(curve + fo::KEYS) + i * ko::SIZE + ko::TIME)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

/// Closing gaps of two frames every ten frames from frame 57 to 82, at 25 frames a second,
/// on keys at the frames 59, 64, 67 and 75. Turn 0 (frame 57) moves all four back by two
/// frames: 57, 62, 65, 73. Turn 1 (frame 65) removes the key now on 65 and moves the last
/// one to 71. Turn 2 (frame 73) finds nothing. The record behind the three keys left holds
/// the last key as it stood when the removal copied it down: frame 73, not the 75 the curve
/// began with.
#[test]
fn frame_stretch_keeps_what_each_removal_left() {
    let mut m = Mem::new();
    glob::init_defaults(&mut m);
    m.set_f32(glob::SI_FRAME_RATE, 25.0);
    let f = |frame: i32| frame as f32 / 25.0;
    let c = fcv::make(&mut m, 1, 1, &[(f(59), 1.0), (f(64), 2.0), (f(67), 3.0), (f(75), 4.0)]);
    let mut h = Trace::new();
    assert_eq!(fcv::frame_stretch(&mut m, &mut h, c, 57, 82, 2, 10, 0), 1);
    assert_eq!(fcv::nb_keys(&m, c), 3);
    for (i, frame) in [57, 62, 71].into_iter().enumerate() {
        assert!(close(key_time(&m, c, i as u32), f(frame)), "key {i}: {}", key_time(&m, c, i as u32));
    }
    assert!(close(key_time(&m, c, 3), f(73)), "the record behind the keys: {}", key_time(&m, c, 3));
    // a step of zero is where the original divides
    assert_eq!(fcv::frame_stretch(&mut m, &mut h, c, 57, 82, 2, 0, 1), 0);
    assert!(m.fault().is_some());
}

/// An inactive curve is evaluated at time 0. With cycle extrapolation after keys that all
/// lie before 0, the wrapped time lands in front of the first key and the original takes
/// the record in front of the array for a segment's start: no defined value. Before that it
/// has stored the time it used and the search index −1, and so does the bridge.
#[test]
fn an_undefined_evaluation_keeps_the_stores_before_it() {
    let mut m = Mem::new();
    glob::init_defaults(&mut m);
    let c = fcv::make(&mut m, 0x102, 1, &[(-0.136186, 0.0), (-0.102853, 4.2), (-0.0695194, 6.3)]);
    m.set_i16(c + fo::ACTIVE, 0);
    m.set_i16(c + fo::POST_EXTRAP, 2);
    m.set_f32(c + fo::LAST_TIME, -100000.0);
    m.set_f32(c + fo::LAST_VALUE, 7.5);
    let v = fcv::value(&mut m, c, -0.5853, 1);
    assert_eq!(v.to_f32(), 0.0);
    assert_eq!(m.f32(c + fo::LAST_TIME), 0.0);
    assert_eq!(m.i32(c + fo::CACHE), -1);
    assert_eq!(m.f32(c + fo::LAST_VALUE), 7.5);
    assert!(m.fault().is_none());
}

/// One step forward from frame 16,777,220 by 1, the play range ending at 16,777,221: the
/// range's end as a `float` is 16,777,220 (the tie goes to the even neighbour), the new
/// frame 16,777,221 lies above it and wraps to the first frame. A comparison of the frame
/// rounded to `float` would see 16,777,220 against 16,777,220 and stay.
#[test]
fn a_step_holds_the_frame_against_the_range_unrounded() {
    let mut m = Mem::new();
    glob::init_defaults(&mut m);
    m.set_i32(glob::SI_FRAME_START, 3);
    m.set_i32(glob::SI_FRAME_END, 16_777_221);
    m.set_i32(glob::SI_FRAME_CURRENT, 16_777_220);
    m.set_i32(glob::SI_FRAME_STEP, 1);
    let mut h = Trace::new();
    playrun::playback_run(&mut m, &mut h, 2, 1, 0);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 3);
    // and back: from the first frame, one step back goes to the range's end as a float
    playrun::playback_run(&mut m, &mut h, 5, 1, 0);
    assert_eq!(m.i32(glob::SI_FRAME_CURRENT), 16_777_220);
    assert!(m.fault().is_none());
}

/// The PostScript name is the preview name, a dot, the frame and ".ps", in a block of the
/// name's length + 15 bytes: room for ten characters of frame number and the zero. With
/// eleven the name fills the block to its last byte and its zero has no room.
#[test]
fn the_postscript_name_ends_with_its_block() {
    let mut m = Mem::new();
    glob::init_defaults(&mut m);
    let base = m.malloc(2);
    m.put_cstr(base, b"a");
    m.set_ptr(assoc::SI_PS_PREV_FILE, base);
    let names = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let n = names.clone();
    let mut h = Trace::with(move |m, name, args| {
        match (name, args) {
            ("window_get_hilite", [Arg::I(slot)]) => m.set_u32(*slot, 0x7b00_0100),
            ("postscript_refresh", [_, Arg::I(text)]) => n.borrow_mut().push(*text),
            _ => {}
        }
        Ret::default()
    });
    assoc::postscript_dump_frame(&mut m, &mut h, 7);
    assoc::postscript_dump_frame(&mut m, &mut h, -1_000_000_000);
    assert!(m.fault().is_none());
    let names = names.borrow();
    assert_eq!(m.cstr(names[0]), b"a.7.ps");
    assert_eq!(m.bytes(names[1], 16), b"a.-1000000000.ps");
    // the blocks: the copy of the name, then for each call the name (1 + 15 bytes) and the number (10)
    let sizes: Vec<u32> = m.heap_blocks().iter().map(|b| b.1).collect();
    assert_eq!(sizes, [2, 16, 10, 16, 10]);
    assert_eq!(m.bytes(m.heap_blocks()[4].0, 10), b".-10000000");
}
