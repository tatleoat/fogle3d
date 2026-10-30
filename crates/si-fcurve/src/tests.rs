// SPDX-License-Identifier: MIT
//! Tests with made-up curves. Expected values are worked by hand from the formulas of finding
//! 0009; times and values are chosen dyadic where a result has to be exact.

use crate::edit::fcv_equal;
use crate::editor::{self, Rect};
use crate::fkey;
use crate::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct Log {
    messages: Vec<(String, i16)>,
    printed: Vec<String>,
    dirty: u32,
    history: u32,
    asked: u32,
    answer: i16,
}

struct Recorder(Rc<RefCell<Log>>);

impl Host for Recorder {
    fn fast_set_dirty(&mut self) {
        self.0.borrow_mut().dirty += 1;
    }
    fn status_message(&mut self, text: &str, kind: i16) {
        self.0.borrow_mut().messages.push((text.to_string(), kind));
    }
    fn replace_locked_key(&mut self) -> i16 {
        let mut log = self.0.borrow_mut();
        log.asked += 1;
        log.answer
    }
    fn printf(&mut self, text: &str) {
        self.0.borrow_mut().printed.push(text.to_string());
    }
    fn save_history(&mut self, _curve: &Fcurve) {
        self.0.borrow_mut().history += 1;
    }
}

fn recording() -> (Env, Rc<RefCell<Log>>) {
    let log = Rc::new(RefCell::new(Log::default()));
    let mut env = Env::new();
    env.host = Box::new(Recorder(log.clone()));
    (env, log)
}

/// A curve of the given interpolation with keys at (time, value), automatic slopes.
fn curve(kind: i16, points: &[(f32, f32)], env: &mut Env) -> Fcurve {
    let mut c = Fcurve::allocate(1, 0, env);
    c.interp = kind;
    for &(t, v) in points {
        c.add_key(Key::create(t, v, 0.0, 0, kind), env);
    }
    c
}

fn times(c: &Fcurve) -> Vec<f32> {
    c.k().iter().map(|k| k.time).collect()
}

fn values(c: &Fcurve) -> Vec<f32> {
    c.k().iter().map(|k| k.value).collect()
}

fn eval(c: &mut Fcurve, t: f32, env: &mut Env) -> f32 {
    c.float_value(t, 0, env).unwrap().to_f32()
}

// ---- records ------------------------------------------------------------------------------

#[test]
fn a_new_curve_and_a_new_key() {
    let env = Env::new();
    assert_eq!((env.frame_rate, env.key_tolerance, env.default_interp), (30.0, 0.15, 2));
    let c = Fcurve::allocate(1, 77, &env);
    assert_eq!((c.kind, c.owner, c.nkeys(), c.keys.is_none()), (1, 77, 0, true));
    assert_eq!((c.last_time, c.last_value, c.pre_extrap, c.post_extrap, c.interp), (-100000.0, 0.0, 0, 0, 2));
    assert_eq!((c.active, c.active_copy, c.cache, c.range_start, c.range_end, c.dirty), (1, 1, 0, 0.0, 0.0, 0));
    // the interpolation a type starts with
    for (kind, interp) in [(kind::NODE_VISIBILITY, 0), (kind::BRANCH_VISIBILITY, 0), (kind::RELATION_ACTIVE, 0),
                           (kind::NODE_MATTE, 0), (kind::CAUSTIC_PHOTONS, 0), (kind::WIREFRAME_DISPLAY, 0),
                           (kind::SUN_TIME, 1), (kind::SHAPE_WEIGHT, 3), (kind::CLUSTER_WEIGHT, 3), (1, 2), (621, 2)] {
        assert_eq!(Fcurve::allocate(kind, 0, &env).interp, interp, "type {kind}");
    }
    let mut other = Env::new();
    other.default_interp = interp::LINEAR;
    assert_eq!(Fcurve::allocate(1, 0, &other).interp, 1);
    assert_eq!(Fcurve::allocate(kind::NODE_VISIBILITY, 0, &other).interp, 0);

    let k = Key::new();
    assert_eq!((k.time, k.value, k.left_slope, k.right_slope, k.slope_type, k.tagged, k.flags), (0.0, 0.0, 0.0, 0.0, 0, 0, 0));
    assert_eq!((k.spline_index, k.field_20), (-1, -1));
    let k = Key::create(1.5, 2.0, 0.25, slope::USER, interp::LINEAR);
    assert_eq!((k.time, k.value, k.left_slope, k.right_slope, k.slope_type, k.interp), (1.5, 2.0, 0.25, 0.25, 1, 1));
    assert_eq!(k.copy(), k);
    assert_eq!([capacity(0), capacity(1), capacity(8), capacity(9)], [0, 8, 8, 16]);
}

#[test]
fn frames_and_seconds() {
    let env = Env::new();
    // time × 30, plus or minus a half, truncated toward zero
    for (sec, frame) in [(0.5f32, 15), (0.49, 15), (0.51, 15), (0.52, 16), (1.0 / 30.0, 1), (0.0166, 0), (0.0, 0), (-0.5, -15),
                         (-0.52, -16), (1e9, i32::MIN), (f32::NAN, i32::MIN)] {
        assert_eq!(eval::sec_to_frame(sec, &env), frame, "{sec}");
    }
    assert_eq!(eval::frame_to_sec(15, &env).to_f32(), 0.5);
    assert_eq!(eval::sec_as_frame(0.5, &env).to_f32(), 15.0);
    // 10/30 in the register is not the float 1/3
    assert_eq!(eval::frame_to_sec(10, &env), X87 { sign: false, exp: 0x3ffd, man: 0xaaaa_aaaa_aaaa_aaab });
    assert_eq!(Key::frame_create(15, 2.0, 0.0, 0, 2, &env).time, 0.5);
    assert_eq!(Key::frame_create(10, 2.0, 0.0, 0, 2, &env).time, 10.0f32 / 30.0);
    assert_eq!(Key::frame_create(15, 2.0, 0.0, 0, 2, &env).frame(&env), 15);
}

// ---- evaluation ---------------------------------------------------------------------------

#[test]
fn no_key_and_one_key() {
    let mut env = Env::new();
    let mut c = Fcurve::allocate(1, 0, &env);
    assert_eq!(eval(&mut c, 3.0, &mut env), 0.0);
    assert_eq!(c.integer_value(3.0, 0, 0, &mut env), Ok(0));
    assert_eq!(c.last_time, -100000.0); // not even the time is recorded
    let mut c = curve(interp::HERMITE, &[(2.0, 7.0)], &mut env);
    for pre in 0..3 {
        for post in 0..3 {
            (c.pre_extrap, c.post_extrap) = (pre, post);
            for t in [-5.0, 2.0, 9.0] {
                assert_eq!(eval(&mut c, t, &mut env), 7.0, "{pre} {post} {t}");
            }
        }
    }
    // a gradient after the only key is the one case that does not record the value
    c.post_extrap = extrap::GRADIENT;
    c.last_value = -1.0;
    assert_eq!(eval(&mut c, 9.0, &mut env), 7.0);
    assert_eq!((c.last_time, c.last_value), (9.0, -1.0));
    c.post_extrap = extrap::CONSTANT;
    assert_eq!(eval(&mut c, 9.0, &mut env), 7.0);
    assert_eq!(c.last_value, 7.0);
}

#[test]
fn constant_and_linear_segments() {
    let mut env = Env::new();
    let mut c = curve(interp::CONSTANT, &[(0.0, 4.0), (1.0, 10.0), (2.0, -3.0)], &mut env);
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, 0.999, &mut env), eval(&mut c, 1.0, &mut env), eval(&mut c, 1.5, &mut env)],
               [4.0, 4.0, 10.0, 10.0]);
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0)], &mut env);
    // (10 − 0)·(0.25 − 0)/(1 − 0) + 0
    assert_eq!(eval(&mut c, 0.25, &mut env), 2.5);
    assert_eq!((c.last_time, c.last_value, c.cache), (0.25, 2.5, 0));
    // the interpolation is the left key's, not the curve's
    c.keys.as_mut().unwrap()[0].interp = interp::CONSTANT;
    assert_eq!(eval(&mut c, 0.25, &mut env), 0.0);
    // an interpolation outside 0..=4 gives 0 and records nothing
    c.keys.as_mut().unwrap()[0].interp = 5;
    assert_eq!((eval(&mut c, 0.25, &mut env), c.last_value), (0.0, 0.0));
    c.keys.as_mut().unwrap()[0].interp = -1;
    assert_eq!(eval(&mut c, 0.25, &mut env), 0.0);
}

#[test]
fn linear_values_come_back_unrounded() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (3.0, 1.0)], &mut env);
    // (1·1)/3 + 0 stays in the register: 64 bits of one third
    let r = c.float_value(1.0, 0, &mut env).unwrap();
    assert_eq!(r, X87 { sign: false, exp: 0x3ffd, man: 0xaaaa_aaaa_aaaa_aaab });
    assert_eq!(c.last_value, 1.0f32 / 3.0);
    assert_eq!(c.value_f32(1.0, 0, &mut env), 1.0f32 / 3.0);
    // spline values pass through a float before they are returned
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (3.0, 1.0)], &mut env);
    let r = c.float_value(1.0, 0, &mut env).unwrap();
    assert_eq!(r, X87::from_f32(r.to_f32()));
    // 3·(1/9) − 2·(1/27) = 7/27
    assert_eq!(r.to_f32(), (7.0f64 / 27.0) as f32);
}

#[test]
fn hermite_segment_by_hand() {
    let mut env = Env::new();
    let both = slope::USER | slope::DOUBLE;
    let mut c = Fcurve::allocate(1, 0, &env);
    c.add_key(Key::create(0.0, 1.0, 2.0, both, interp::HERMITE), &mut env);
    c.add_key(Key::create(4.0, 3.0, 0.0, both, interp::HERMITE), &mut env);
    // at x = 1: h = 1/4, d = 1; weights 27/32 and 5/32; (d³h² − 2d²h + d) = 9/16, (d³h² − d²h) = −3/16
    // 27/32·1 + 5/32·3 + 9/16·2 + (−3/16)·0 = 2.4375
    assert_eq!(eval(&mut c, 1.0, &mut env), 2.4375);
    c.keys.as_mut().unwrap()[1].left_slope = -1.0;
    assert_eq!(eval(&mut c, 1.0, &mut env), 2.625);
    // the right slope of the second key and the left slope of the first play no part
    c.keys.as_mut().unwrap()[1].right_slope = 99.0;
    c.keys.as_mut().unwrap()[0].left_slope = 99.0;
    assert_eq!(eval(&mut c, 1.0, &mut env), 2.625);
    // slope-0 and plateau segments are evaluated by the same formula
    for kind in [interp::SLOPE0, interp::PLATEAU] {
        c.keys.as_mut().unwrap()[0].interp = kind;
        assert_eq!(eval(&mut c, 1.0, &mut env), 2.625);
    }
    // midpoint of a segment with zero slopes: the mean
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (2.0, 4.0)], &mut env);
    assert_eq!(eval(&mut c, 1.0, &mut env), 2.0);
}

#[test]
fn automatic_slopes_feed_the_spline() {
    let mut env = Env::new();
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)], &mut env);
    // ends flat; the middle key gets (4 − 0)/(2 − 0) on both sides
    let s: Vec<(f32, f32)> = c.k().iter().map(|k| (k.left_slope, k.right_slope)).collect();
    assert_eq!(s, [(0.0, 0.0), (2.0, 2.0), (0.0, 0.0)]);
    // x = 0.5: 0.5·0 + 0.5·1 + 0.125·0 + (−0.125)·2
    assert_eq!(eval(&mut c, 0.5, &mut env), 0.25);
    // x = 1.5: 0.5·1 + 0.5·4 + 0.125·2 + (−0.125)·0
    assert_eq!(eval(&mut c, 1.5, &mut env), 2.75);
    assert_eq!(c.cache, 1);
}

#[test]
fn a_time_within_1e_5_of_a_key_is_that_key() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)], &mut env);
    assert_eq!(eval(&mut c, 0.999995, &mut env), 10.0);
    assert_eq!(c.cache, 1);
    assert_eq!(eval(&mut c, 1.000005, &mut env), 10.0);
    assert!(eval(&mut c, 0.99998, &mut env) < 10.0);
    // coincident keys: at their time the first one answers, after it the second one starts the segment
    let mut c = Fcurve::allocate(1, 0, &env);
    c.keys = Some(vec![Key::create(1.0, 2.0, 0.0, 0, 1), Key::create(1.0, 5.0, 0.0, 0, 1), Key::create(2.0, 7.0, 0.0, 0, 1)]);
    assert_eq!(eval(&mut c, 1.0, &mut env), 2.0);
    assert_eq!(eval(&mut c, 1.5, &mut env), 6.0);
}

#[test]
fn extrapolation_constant_and_gradient() {
    si_rt::skip_without_install!();
    let mut env = Env::new();
    let mut c = curve(interp::HERMITE, &[(1.0, 5.0), (3.0, 9.0)], &mut env);
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, 3.0, &mut env), eval(&mut c, 10.0, &mut env)], [5.0, 9.0, 9.0]);
    (c.pre_extrap, c.post_extrap) = (extrap::GRADIENT, extrap::GRADIENT);
    // automatic end slopes are zero
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, 10.0, &mut env)], [5.0, 9.0]);
    {
        let keys = c.keys.as_mut().unwrap();
        keys[0].left_slope = 2.0;
        keys[0].right_slope = 50.0;
        keys[1].right_slope = -1.0;
        keys[1].left_slope = 50.0;
    }
    // 5 − 2·(1 − 0); 9 + (−1)·(5 − 3)
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, 5.0, &mut env)], [3.0, 7.0]);
    assert_eq!(c.last_value, 7.0);
    // a linear curve extends its first and last segment instead: slope (9 − 5)/(3 − 1)
    c.interp = interp::LINEAR;
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, -1.0, &mut env), eval(&mut c, 5.0, &mut env)], [3.0, 1.0, 13.0]);
    // unknown modes: a message, and 0
    let (mut env, log) = recording();
    (c.pre_extrap, c.post_extrap) = (3, -1);
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, 5.0, &mut env)], [0.0, 0.0]);
    assert_eq!(log.borrow().printed, [si_rt::text(0x0869_5160), si_rt::text(0x0869_51a0)]);
}

#[test]
fn extrapolation_cycle() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0)], &mut env);
    c.post_extrap = extrap::CYCLE;
    // 2.25 − 2·1 = 0.25
    assert_eq!(eval(&mut c, 2.25, &mut env), 2.5);
    assert_eq!(eval(&mut c, 2.5, &mut env), 5.0);
    // a whole number of periods is the end of a cycle, not its start
    assert_eq!(eval(&mut c, 1.0, &mut env), 10.0);
    assert_eq!(eval(&mut c, 2.0, &mut env), 10.0);
    assert_eq!(eval(&mut c, 2.000001, &mut env), 10.0);
    assert_eq!(c.cache, 1);
    // far away: 1000000.25 is a float
    assert_eq!(eval(&mut c, 1000000.25, &mut env), 2.5);
    // before the first key the mode is still constant
    assert_eq!(eval(&mut c, -0.75, &mut env), 0.0);
    c.pre_extrap = extrap::CYCLE;
    c.post_extrap = extrap::CONSTANT;
    // −0.75 + 1; −1.75 + 2
    assert_eq!(eval(&mut c, -0.75, &mut env), 2.5);
    assert_eq!(eval(&mut c, -1.75, &mut env), 2.5);
    // −1 + 1 lands on the first key: it becomes the last key's time, which the rule for
    // times after the curve then handles (here: constant)
    assert_eq!(eval(&mut c, -1.0, &mut env), 10.0);
    // keys closer than 1e-5 do not cycle
    let mut c = Fcurve::allocate(1, 0, &env);
    c.keys = Some(vec![Key::create(1.0, 2.0, 0.0, 0, 1), Key::create(1.000001, 5.0, 0.0, 0, 1)]);
    (c.pre_extrap, c.post_extrap) = (extrap::CYCLE, extrap::CYCLE);
    assert_eq!([eval(&mut c, 0.0, &mut env), eval(&mut c, 3.0, &mut env)], [2.0, 5.0]);
}

#[test]
fn inputs_without_a_defined_result() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0)], &mut env);
    (c.pre_extrap, c.post_extrap) = (extrap::CYCLE, extrap::CYCLE);
    c.cache = 0;
    assert_eq!(c.float_value(f32::INFINITY, 0, &mut env), Err(Undefined::NeverReturns));
    assert_eq!(c.float_value(f32::NEG_INFINITY, 0, &mut env), Err(Undefined::NeverReturns));
    // the float evaluator's counter is a float: it cannot count past 2^24 periods
    assert_eq!(c.float_value(16777216.0, 0, &mut env).map(X87::to_f32), Ok(10.0));
    assert_eq!(c.float_value(16777220.0, 0, &mut env), Err(Undefined::NeverReturns));
    assert_eq!(c.value_f32(16777220.0, 0, &mut env), 0.0);
    // a NaN time ends the search on the key it starts from
    c.cache = 0;
    assert_eq!(c.float_value(f32::NAN, 0, &mut env), Err(Undefined::BeforeKeys));
    assert_eq!(c.cache, -1);
    assert_eq!(c.float_value(0.5, 0, &mut env), Err(Undefined::BeforeKeys));
    c.cache = 1;
    assert!(c.float_value(f32::NAN, 0, &mut env).unwrap().is_nan());
    assert_eq!(c.cache, 0);
    // the integer evaluator's counter is an int and gets there
    let mut v = Fcurve::allocate(kind::NODE_VISIBILITY, 0, &env);
    v.keys = Some(vec![Key::create(0.0, 1.0, 0.0, 0, 0), Key::create(1.0, 0.0, 0.0, 0, 0), Key::create(2.0, 1.0, 0.0, 0, 0)]);
    v.post_extrap = extrap::CYCLE;
    assert_eq!(v.integer_value(100000000.0, 0, 1, &mut env), Ok(1));
    assert_eq!(v.integer_value(f32::INFINITY, 0, 1, &mut env), Err(Undefined::NeverReturns));
    // less than 1e-5 before the last key, and not on it: the search runs off the array
    v.post_extrap = extrap::CONSTANT;
    assert_eq!(v.integer_value(1.999995, 0, 1, &mut env), Err(Undefined::PastKeys));
    assert_eq!(v.integer_value(2.0, 0, 1, &mut env), Ok(1));
}

#[test]
fn the_cycle_search_matches_the_original_loop() {
    // the original counts periods one by one; the port bisects. Same answers on a spread of
    // times, with a period that is not a round number.
    let mut env = Env::new();
    let (t0, tn) = (0.3f32, 1.7f32);
    let mut c = curve(interp::LINEAR, &[(t0, 0.0), (tn, 10.0)], &mut env);
    (c.pre_extrap, c.post_extrap) = (extrap::CYCLE, extrap::CYCLE);
    let x = X87::from_f32;
    let period = x(tn) - x(t0);
    let mut state = 12345u32;
    for _ in 0..2000 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let time = ((state >> 8) as f32 / 65536.0 - 128.0) * 7.3;
        let mut expect = x(time);
        if x(t0).gt(x(time)) {
            let mut n = 1.0f32;
            loop {
                expect = x(n) * period + x(time);
                n += 1.0;
                if !expect.lt(x(t0)) {
                    break;
                }
            }
        } else if x(time).gt(x(tn)) {
            let mut n = 1.0f32;
            loop {
                expect = x(time) - x(n) * period;
                n += 1.0;
                if !expect.gt(x(tn)) {
                    break;
                }
            }
        }
        // inside the keys the value is linear in the wrapped time
        if expect.gt(x(t0)) && expect.lt(x(tn)) && (expect - x(t0)).abs().gt(X87::from_f64(1e-5)) && (expect - x(tn)).abs().gt(X87::from_f64(1e-5)) {
            let want = ((x(10.0) - x(0.0)) * (expect - x(t0))) / (x(tn) - x(t0)) + x(0.0);
            assert_eq!(c.float_value(time, 0, &mut env), Ok(want), "time {time}");
        }
    }
}

#[test]
fn activity_and_range() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0)], &mut env);
    let at = |c: &mut Fcurve, t: f32, check: i16, env: &mut Env| c.float_value(t, check, env).unwrap().to_f32();
    (c.range_start, c.range_end) = (0.25, 0.5);
    assert_eq!([at(&mut c, 0.75, 0, &mut env), at(&mut c, 0.75, 1, &mut env), at(&mut c, 0.125, 1, &mut env), at(&mut c, 0.375, 1, &mut env)],
               [7.5, 5.0, 2.5, 3.75]);
    assert_eq!(c.last_time, 0.375);
    // a range narrower than 1e-5 is no range
    (c.range_start, c.range_end) = (0.25, 0.250001);
    assert_eq!(at(&mut c, 0.75, 1, &mut env), 7.5);
    // an inactive curve is evaluated at time 0
    c.active = 0;
    assert_eq!([at(&mut c, 0.75, 1, &mut env), at(&mut c, 0.75, 0, &mut env)], [0.0, 7.5]);
    // cycling uses the time as given, not the clamped one
    c.active = 1;
    (c.range_start, c.range_end) = (0.0, 4.0);
    c.post_extrap = extrap::CYCLE;
    assert_eq!(at(&mut c, 9.25, 1, &mut env), 2.5);
}

#[test]
fn integer_curves() {
    si_rt::skip_without_install!();
    let mut env = Env::new();
    let mut c = Fcurve::allocate(kind::CAUSTIC_PHOTONS, 0, &env);
    for (t, v) in [(0.0, 2.5), (1.0, -2.7), (2.0, 0.3), (3.0, -0.3)] {
        c.add_key(Key::create(t, v, 0.0, 0, interp::CONSTANT), &mut env);
    }
    // as numbers: value + 0.5, truncated toward zero
    let n: Vec<i32> = [0.0, 0.5, 1.0, 2.0, 3.0, 9.0, -4.0].iter().map(|&t| c.integer_value(t, 0, 0, &mut env).unwrap()).collect();
    assert_eq!(n, [3, 3, -2, 0, 0, 0, 3]);
    assert_eq!(c.last_value, 3.0);
    // as flags: above zero or not
    let f: Vec<i32> = [0.0, 1.0, 2.0, 3.0].iter().map(|&t| c.integer_value(t, 0, 1, &mut env).unwrap()).collect();
    assert_eq!(f, [1, 0, 1, 0]);
    // gradient extrapolation is constant for these
    (c.pre_extrap, c.post_extrap) = (extrap::GRADIENT, extrap::GRADIENT);
    assert_eq!([c.integer_value(-4.0, 0, 0, &mut env), c.integer_value(9.0, 0, 0, &mut env)], [Ok(3), Ok(0)]);
    // a key takes over 1e-5 before its time
    assert_eq!([c.integer_value(0.99998, 0, 0, &mut env), c.integer_value(0.999995, 0, 0, &mut env)], [Ok(3), Ok(-2)]);
    // cycling: period 3; 4.5 − 3 = 1.5, 7.25 − 6 = 1.25, −0.5 + 3 = 2.5
    (c.pre_extrap, c.post_extrap) = (extrap::CYCLE, extrap::CYCLE);
    assert_eq!([c.integer_value(4.5, 0, 0, &mut env), c.integer_value(7.25, 0, 0, &mut env), c.integer_value(-0.5, 0, 0, &mut env)],
               [Ok(-2), Ok(-2), Ok(0)]);
    // several keys within 1e-5 and cycling: the end key's value, merely truncated
    let mut d = Fcurve::allocate(kind::CAUSTIC_PHOTONS, 0, &env);
    d.keys = Some(vec![Key::create(1.0, 2.9, 0.0, 0, 0), Key::create(1.000001, 7.9, 0.0, 0, 0)]);
    (d.pre_extrap, d.post_extrap) = (extrap::CYCLE, extrap::CYCLE);
    assert_eq!([d.integer_value(0.0, 0, 1, &mut env), d.integer_value(5.0, 0, 0, &mut env)], [Ok(2), Ok(7)]);
    assert_eq!(d.last_value, 7.9);
    // unknown modes
    let (mut env, log) = recording();
    (c.pre_extrap, c.post_extrap) = (-1, 3);
    assert_eq!([c.integer_value(-4.0, 0, 0, &mut env), c.integer_value(9.0, 0, 0, &mut env)], [Ok(0), Ok(0)]);
    assert_eq!(log.borrow().printed.len(), 2);
}

#[test]
fn the_type_selects_the_evaluator() {
    let mut env = Env::new();
    let make = |kind: i32, v: f32, env: &mut Env| {
        let mut c = Fcurve::allocate(kind, 0, env);
        let interp = c.interp;
        c.add_key(Key::create(0.0, v, 0.0, 0, interp), env);
        c.add_key(Key::create(1.0, v + 0.5, 0.0, 0, interp), env);
        c
    };
    let v = |c: &mut Fcurve, env: &mut Env| c.value(0.5, 1, env).unwrap().to_f32();
    // on/off parameters read as flags
    for kind in [175, 176, 177, 178, 182, 196, 273, 378, 620, 627, 628, 631, 632, 633] {
        assert_eq!(v(&mut make(kind, 0.3, &mut env), &mut env), 1.0, "type {kind}");
        assert_eq!(v(&mut make(kind, -0.3, &mut env), &mut env), 0.0, "type {kind}");
    }
    // node visibility reads as a number while hidden objects are used as mattes
    env.use_hidden_for_matte = 1;
    assert_eq!(v(&mut make(175, 0.3, &mut env), &mut env), 0.0);
    assert_eq!(v(&mut make(175, 1.6, &mut env), &mut env), 2.0);
    assert_eq!(v(&mut make(176, 0.3, &mut env), &mut env), 1.0);
    env.use_hidden_for_matte = 0;
    // photon counts: numbers, not below zero
    assert_eq!(v(&mut make(625, 7.4, &mut env), &mut env), 7.0);
    assert_eq!(v(&mut make(626, -3.0, &mut env), &mut env), 0.0);
    // energies: real numbers, not below zero; other types keep their sign
    assert_eq!(v(&mut make(621, -1.5, &mut env), &mut env), 0.0);
    assert_eq!(v(&mut make(624, 1.5, &mut env), &mut env), 1.75);
    assert_eq!(v(&mut make(1, -1.5, &mut env), &mut env), -1.25);
    assert_eq!(v(&mut make(625, 0.5, &mut env), &mut env), 1.0); // 0.5 + 0.5 truncates to 1 …
    assert_eq!(v(&mut make(625, 0.4, &mut env), &mut env), 0.0); // … 0.4 + 0.5 to 0
    // matte values have constant interpolation but are real numbers
    assert_eq!(v(&mut make(371, 0.25, &mut env), &mut env), 0.25);
    // by frame: 15/30 s
    assert_eq!(make(1, 2.0, &mut env).frame_value(15, &mut env).unwrap().to_f32(), 2.25);
}

// ---- insertion and removal ----------------------------------------------------------------

#[test]
fn keys_go_in_by_time() {
    let (mut env, log) = recording();
    let mut c = Fcurve::allocate(1, 0, &env);
    let key = |t: f32, v: f32| Key::create(t, v, 0.0, 0, interp::HERMITE);
    assert_eq!(c.add_key(key(1.0, 3.0), &mut env), 0);
    assert_eq!(c.add_key(key(0.0, -2.0), &mut env), 0);
    assert_eq!(c.add_key(key(0.5, 1.0), &mut env), 1);
    assert_eq!((times(&c), c.cache), (vec![0.0, 0.5, 1.0], 1));
    // the bounds only ever widen from the 0, 0 they start with
    assert_eq!((c.min_value, c.max_value), (-2.0, 3.0));
    assert_eq!(log.borrow().dirty, 3);
    // 0.12 frame from the key at 0.5: the same key, replaced with a note
    assert_eq!(c.add_key(key(0.504, 9.0), &mut env), 1);
    assert_eq!((times(&c), values(&c)), (vec![0.0, 0.504, 1.0], vec![-2.0, 9.0, 3.0]));
    assert_eq!(log.borrow().messages, [("Overlapping keys, new one replaces old.".to_string(), 0)]);
    // 0.18 frame away: another key
    assert_eq!(c.add_key(key(0.51, 4.0), &mut env), 2);
    assert_eq!(times(&c), [0.0, 0.504, 0.51, 1.0]);
    // without `set_time` a replaced key keeps its time; without `warn` nothing is said
    assert_eq!(c.add_key_after(key(1.003, 5.0), 0, 0, 0, &mut env), 3);
    assert_eq!((times(&c)[3], values(&c)[3], log.borrow().messages.len()), (1.0, 5.0, 1));
    // a scan that starts behind the key on the same frame does not see it
    assert_eq!(c.add_key_after(key(0.0, 6.0), 2, 1, 1, &mut env), 1);
    assert_eq!(times(&c), [0.0, 0.0, 0.504, 0.51, 1.0]);
}

#[test]
fn locked_keys_ask_before_they_are_replaced() {
    let (mut env, log) = recording();
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0)], &mut env);
    c.keys.as_mut().unwrap()[1].flags = keyflag::KEY_FRAME | keyflag::STRUCT;
    let mut key = Key::create(1.0, 8.0, 0.0, 0, interp::HERMITE);
    key.flags = keyflag::BIT_40 | 0x80;
    assert_eq!(c.add_key(key, &mut env), -1);
    assert_eq!((values(&c)[1], log.borrow().asked), (1.0, 1));
    log.borrow_mut().answer = 1;
    assert_eq!(c.add_key(key, &mut env), 1);
    // the new key's flags, made a locked key's: type 1, the three flag bits kept, the rest dropped
    assert_eq!((values(&c)[1], c.k()[1].flags), (8.0, keyflag::BIT_40 | 1));
    // without `warn` a locked key is replaced unasked and takes the new key's flags as they are
    assert_eq!(c.add_key_after(key, 0, 0, 1, &mut env), 1);
    assert_eq!((c.k()[1].flags, log.borrow().asked), (keyflag::BIT_40 | 0x80, 2));
    // propagation mode marks every key written
    env.key_type_propag_mode = 1;
    c.add_key(Key::create(2.0, 0.0, 0.0, 0, 2), &mut env);
    assert_eq!(c.k()[2].flags, keyflag::PROPAG);
}

#[test]
fn slopes_by_rule() {
    let mut env = Env::new();
    // the line through the neighbours: (2 − 0)/(3 − 0)
    let c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (3.0, 2.0)], &mut env);
    assert_eq!((c.k()[1].left_slope, c.k()[1].right_slope), ((2.0f64 / 3.0) as f32, (2.0f64 / 3.0) as f32));
    assert_eq!(edit::calculate_slope(0.0, 0.0, 3.0, 2.0), X87::from_f64(2.0) / X87::from_f64(3.0));
    // "double": the two chords, (1 − 0)/(1 − 0) and (2 − 1)/(3 − 1)
    let mut d = c.clone();
    d.keys.as_mut().unwrap()[1].slope_type = slope::DOUBLE;
    assert_eq!(d.manage_slope(1), 1);
    assert_eq!((d.k()[1].left_slope, d.k()[1].right_slope), (1.0, 0.5));
    // user slope: the left follows the right; user and double: both stay
    let mut u = c.clone();
    {
        let k = &mut u.keys.as_mut().unwrap()[1];
        (k.slope_type, k.left_slope, k.right_slope) = (slope::USER, 7.0, 3.0);
    }
    u.manage_slope(1);
    assert_eq!((u.k()[1].left_slope, u.k()[1].right_slope), (3.0, 3.0));
    {
        let k = &mut u.keys.as_mut().unwrap()[1];
        (k.slope_type, k.left_slope, k.right_slope) = (slope::USER | slope::DOUBLE, 7.0, 3.0);
    }
    u.manage_slope(1);
    assert_eq!((u.k()[1].left_slope, u.k()[1].right_slope), (7.0, 3.0));
    // a user slope survives at the ends too, where automatic slopes are zero
    let mut e = c.clone();
    e.keys.as_mut().unwrap()[0].slope_type = slope::USER;
    e.keys.as_mut().unwrap()[0].right_slope = 4.0;
    e.default_slope(&mut env);
    assert_eq!((e.k()[0].left_slope, e.k()[0].right_slope, e.k()[2].right_slope), (4.0, 4.0, 0.0));
    assert_eq!([c.clone().manage_slope(-1), c.clone().manage_slope(3)], [0, 0]);
    // slope-0 keys are flat
    let z = curve(interp::SLOPE0, &[(0.0, 0.0), (1.0, 1.0), (3.0, 2.0)], &mut env);
    assert_eq!((z.k()[1].left_slope, z.k()[1].right_slope), (0.0, 0.0));
    // plateau: flat next to a value within 1% …
    let p = curve(interp::PLATEAU, &[(0.0, 0.0), (1.0, 5.0), (2.0, 5.02), (3.0, 9.0)], &mut env);
    assert_eq!([p.k()[1].right_slope, p.k()[2].right_slope], [0.0, 0.0]);
    // … flat at an extremum …
    let p = curve(interp::PLATEAU, &[(0.0, 0.0), (1.0, 5.0), (2.0, 0.0)], &mut env);
    assert_eq!(p.k()[1].right_slope, 0.0);
    let p = curve(interp::PLATEAU, &[(0.0, 5.0), (1.0, -5.0), (2.0, 5.0)], &mut env);
    assert_eq!(p.k()[1].right_slope, 0.0);
    // … and like a Hermite key where the curve runs through: (10 − 0)/(2 − 0)
    let p = curve(interp::PLATEAU, &[(0.0, 0.0), (1.0, 5.0), (2.0, 10.0)], &mut env);
    assert_eq!((p.k()[1].left_slope, p.k()[1].right_slope), (5.0, 5.0));
    // changing the interpolation changes every key and the slopes
    let mut s = c.clone();
    s.set_interp(interp::SLOPE0, &mut env);
    assert_eq!((s.interp, s.k()[1].interp, s.k()[1].right_slope), (3, 3, 0.0));
}

#[test]
fn removal() {
    let (mut env, log) = recording();
    let five = |env: &mut Env| curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 9.0), (4.0, 16.0)], env);
    let mut c = five(&mut env);
    assert!(!c.remove_key_index(0, &mut env) && !c.remove_key_index(6, &mut env));
    assert!(c.remove_key_index(3, &mut env));
    assert_eq!(times(&c), [0.0, 1.0, 3.0, 4.0]);
    // the keys beside the gap get new slopes: (9 − 0)/(3 − 0) and (16 − 1)/(4 − 1)
    assert_eq!([c.k()[1].right_slope, c.k()[2].right_slope], [3.0, 5.0]);
    assert_eq!([c.key_index(3), c.key_index(4)], [4, 0]);
    c.remove_key(9, &mut env);
    assert_eq!(c.nkeys(), 4);
    // between times: strictly inside
    let mut c = five(&mut env);
    assert!(c.remove_key_between_times(0, 1.0, 3.0, 0, &mut env));
    assert_eq!(times(&c), [0.0, 1.0, 3.0, 4.0]);
    let mut c = five(&mut env);
    c.remove_key_between_times(0, 0.5, 3.5, 0, &mut env);
    assert_eq!(times(&c), [0.0, 4.0]);
    assert!(!c.remove_key_between_times(0, 3.0, 1.0, 0, &mut env));
    // by index: bounds clamp, an empty or outside range is refused
    let mut c = five(&mut env);
    assert!(c.remove_key_between_index(-3, 2, 0, &mut env));
    assert_eq!(times(&c), [2.0, 3.0, 4.0]);
    assert!(!c.remove_key_between_index(4, 9, 0, &mut env) && !c.remove_key_between_index(3, 2, 0, &mut env) && !c.remove_key_between_index(1, 0, 0, &mut env));
    // by value, stepping on after each removal: of the neighbours 1 and 4 only the first goes
    let mut c = five(&mut env);
    c.remove_key_between(1.0, 9.0, &mut env);
    assert_eq!(values(&c), [0.0, 4.0, 9.0, 16.0]);
    // around the current frame (frame 60 = 2 s)
    let mut c = five(&mut env);
    env.frame_current = 60;
    assert!(c.remove_key_round(0.5, &mut env));
    assert_eq!(times(&c), [0.0, 1.0, 3.0, 4.0]);
    // raw keys
    let mut c = five(&mut env);
    c.keys.as_mut().unwrap()[1].flags = keyflag::RAW;
    c.keys.as_mut().unwrap()[2].flags = keyflag::RAW | keyflag::STRUCT;
    assert!(c.check_for_raw_keys());
    c.remove_raw_keys();
    assert_eq!((times(&c), c.check_for_raw_keys()), (vec![0.0, 3.0, 4.0], false));
    // all keys: the array goes, nothing else is reset
    let before = log.borrow().dirty;
    c.cache = 2;
    c.remove_all_keys(&mut env);
    assert_eq!((c.keys.is_none(), c.cache, c.max_value, log.borrow().dirty), (true, 2, 16.0, before + 1));
    // emptied key by key, the array is still there
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0)], &mut env);
    c.remove_key_index(1, &mut env);
    assert_eq!((c.nkeys(), c.keys.is_some()), (0, true));
    assert_eq!(c.set_keys_on_frames(0.0, 1.0, &mut env), 1);
    c.remove_all_keys(&mut env);
    assert_eq!(c.set_keys_on_frames(0.0, 1.0, &mut env), 0);
}

#[test]
fn removing_struct_keys_only() {
    let mut env = Env::new();
    let five = |marks: &[usize], env: &mut Env| {
        let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)], env);
        for &m in marks {
            c.keys.as_mut().unwrap()[m].flags |= keyflag::STRUCT;
        }
        c
    };
    // a run of struct keys is removed cleanly
    let mut c = five(&[1, 2, 3], &mut env);
    c.remove_key_between_index(1, 5, 1, &mut env);
    assert_eq!(times(&c), [0.0, 4.0]);
    let mut c = five(&[3, 4], &mut env);
    c.remove_key_between_times(0, -1.0, 9.0, 1, &mut env);
    assert_eq!(times(&c), [0.0, 1.0, 2.0]);
    // struct keys with others between them: the original's compaction duplicates and loses keys
    let mut c = five(&[1, 3], &mut env);
    c.remove_key_between_index(1, 5, 1, &mut env);
    assert_eq!(times(&c), [0.0, 2.0, 2.0]);
    // the struct-key queries
    let mut c = five(&[1], &mut env);
    assert!(c.check_struct_key(1.0, 1.0) && c.check_struct_key(0.5, 3.0) && !c.check_struct_key(1.5, 3.0));
    assert_eq!(c.set_struct_key_state(2.0, 3.0, 1), 1);
    let f: Vec<u16> = c.k().iter().map(|k| k.flags).collect();
    assert_eq!(f, [0, 0x10, 0x10, 0x10, 0]);
    c.keys.as_mut().unwrap()[2].flags = 0x71;
    assert_eq!(c.set_struct_key_state(1.5, 2.5, 0), 1);
    assert_eq!(c.k()[2].flags, 1); // clearing takes the other flag bits along
    assert_eq!(c.set_struct_key_state(7.0, 9.0, 1), 0);
}

// ---- moving and scaling -------------------------------------------------------------------

#[test]
fn sorting_has_a_tolerance() {
    let mut env = Env::new();
    let mut c = Fcurve::allocate(1, 0, &env);
    let key = |t: f32, v: f32| Key::create(t, v, 0.0, 0, interp::HERMITE);
    c.keys = Some(vec![key(0.0, 0.0), key(2.0, 2.0), key(1.0, 1.0), key(1.000005, 3.0), key(1.0, 4.0), key(-1.0, 5.0)]);
    c.sort_keys(&mut env);
    // 1.000005 and the 1.0 behind it are within 1e-5: left in the order they were in
    assert_eq!(values(&c), [5.0, 0.0, 1.0, 3.0, 4.0, 2.0]);
    // and the slopes are recomputed: for the key valued 0, (1 − 5)/(1 − (−1))
    assert_eq!(c.k()[1].right_slope, -2.0);
}

#[test]
fn moving_keys_in_time() {
    let mut env = Env::new();
    let four = |env: &mut Env| curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)], env);
    let mut c = four(&mut env);
    assert_eq!(c.move_key_time(1.0, 2.0, 0.5, 0, &mut env), 1);
    assert_eq!(times(&c), [0.0, 1.5, 2.5, 3.0]);
    // the keys are re-sorted afterwards
    let mut c = four(&mut env);
    c.move_key_time(1.0, 2.0, 1.5, 0, &mut env);
    assert_eq!((times(&c), values(&c)), (vec![0.0, 2.5, 3.0, 3.5], vec![0.0, 1.0, 3.0, 2.0]));
    let mut c = four(&mut env);
    c.keys.as_mut().unwrap()[2].flags = keyflag::STRUCT;
    assert_eq!(c.move_key_time(0.0, 9.0, 0.25, 1, &mut env), 1);
    assert_eq!(times(&c), [0.0, 1.0, 2.25, 3.0]);
    assert_eq!(c.move_key_time(5.0, 9.0, 0.25, 0, &mut env), 0);
    // offset: all but the locked keys
    let mut c = four(&mut env);
    c.keys.as_mut().unwrap()[1].flags = keyflag::KEY_FRAME;
    assert!(c.offset(0.25, &mut env));
    assert_eq!(times(&c), [0.25, 1.0, 2.25, 3.25]);
    // spread: key i moves by 3·i/3
    let mut c = four(&mut env);
    assert!(c.spread(3.0, &mut env));
    assert_eq!(times(&c), [0.0, 2.0, 4.0, 6.0]);
    assert!(!curve(interp::LINEAR, &[(0.0, 0.0)], &mut env).spread(3.0, &mut env));
}

#[test]
fn scaling() {
    let mut env = Env::new();
    let four = |env: &mut Env| curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)], env);
    // about a pivot, a stretch of time: base (1 − 0)·2 + 0 = 2, then 2 + (t − 1)·2
    let mut c = four(&mut env);
    assert_eq!(c.scale_key_time(1.0, 0.0, 3.0, 2.0, 0, 0, &mut env), 1);
    assert_eq!(times(&c), [0.0, 2.0, 4.0, 6.0]);
    // locked and path keys stay unless asked for
    let mut c = four(&mut env);
    c.keys.as_mut().unwrap()[1].flags = keyflag::KEY_FRAME;
    c.keys.as_mut().unwrap()[2].flags = keyflag::KEY_PATH;
    c.scale_key_time(1.0, 0.0, 3.0, 2.0, 0, 0, &mut env);
    assert_eq!(times(&c), [0.0, 1.0, 2.0, 6.0]);
    c.scale_key_time(1.0, 1.0, 2.0, 0.5, 0, 1, &mut env);
    assert_eq!(times(&c), [0.0, 1.0, 1.5, 6.0]);
    // whole-curve scale: times ×2, user slopes of spline keys ÷2; locked keys untouched
    let mut c = four(&mut env);
    {
        let keys = c.keys.as_mut().unwrap();
        (keys[1].interp, keys[1].slope_type, keys[1].right_slope) = (interp::HERMITE, slope::USER | slope::DOUBLE, 3.0);
        (keys[2].slope_type, keys[2].right_slope) = (slope::USER | slope::DOUBLE, 3.0); // linear: slopes stay
        keys[3].flags = keyflag::KEY_FRAME;
    }
    assert!(c.scale(2.0, &mut env));
    // the locked key stayed at 3 and is now third; the linear key with its user slope is last
    assert_eq!((times(&c), values(&c)), (vec![0.0, 2.0, 3.0, 4.0], vec![0.0, 1.0, 3.0, 2.0]));
    assert_eq!((c.k()[1].right_slope, c.k()[3].right_slope), (1.5, 3.0));
    // the sort recomputed the automatic slopes: for the locked key, (2 − 1)/(4 − 2)
    assert_eq!(c.k()[2].right_slope, 0.5);
    // nearly 1: nothing happens
    let mut d = c.clone();
    assert!(d.scale(1.000001, &mut env));
    assert_eq!(d, c);
    // zero: the slopes become 100000
    c.scale(0.0, &mut env);
    assert_eq!(c.k().iter().filter(|k| k.right_slope == 100000.0).count(), 1);
    // values about a pivot: 1 + (v − 1)·2
    let mut c = four(&mut env);
    c.keys.as_mut().unwrap()[3].flags = keyflag::KEY_PATH;
    c.val_scale(2.0, 1.0, &mut env);
    assert_eq!(values(&c), [-1.0, 1.0, 3.0, 3.0]);
    // times about a pivot: (t − 1)·2 + 1
    let mut c = four(&mut env);
    c.scale_origin(2.0, 1.0, &mut env);
    assert_eq!(times(&c), [-1.0, 1.0, 3.0, 5.0]);
}

#[test]
fn offset_and_scale_keeps_the_outside_attached() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)], &mut env);
    // stretch [1, 3) to twice its length about 1, shift by 0.5:
    // new start 1, new end 5; 0 → 0 + ((1 − 1) + 0.5); 2 → ((2 − 1)·2 + 1) + 0.5; 3 → 3 + ((5 − 3) + 0.5)
    c.offset_and_scale(1.0, 1.0, 3.0, 0.5, 2.0, &mut env);
    assert_eq!(times(&c), [0.5, 1.5, 3.5, 5.5, 6.5]);
}

#[test]
fn keys_onto_frames() {
    let (mut env, log) = recording();
    let mut c = curve(interp::LINEAR, &[(0.2, 0.0), (0.49, 1.0), (0.52, 2.0), (1.01, 3.0)], &mut env);
    assert_eq!(c.set_keys_on_frames(0.4, 2.0, &mut env), 1);
    // 0.49 s → frame 15, 0.52 s → frame 16, 1.01 s → frame 30; 0.2 s is outside
    assert_eq!(times(&c), [0.2, 0.5, 16.0f32 / 30.0, 1.0]);
    assert!(log.borrow().messages.is_empty());
    let mut c = curve(interp::LINEAR, &[(0.49, 1.0), (0.51, 2.0)], &mut env);
    c.set_keys_on_frames(0.0, 2.0, &mut env);
    assert_eq!(times(&c), [0.5, 0.5]);
    assert_eq!(log.borrow().messages, [("Keys set to same frame".to_string(), 2)]);
    // key types by frame
    c.set_time_key_type(keyflag::KEY_FRAME as i32, 0.51, &env);
    assert_eq!([c.k()[0].flags, c.k()[1].flags], [1, 1]);
    c.set_key_type(0x12);
    assert_eq!(c.k()[0].flags, 0x12);
}

#[test]
fn frame_stretch_opens_and_closes_gaps() {
    let mut env = Env::new();
    env.frame_rate = 32.0;
    let frames = |c: &Fcurve| -> Vec<f32> { c.k().iter().map(|k| k.time * 32.0).collect() };
    let at = |f: &[i32], env: &mut Env| curve(interp::LINEAR, &f.iter().map(|&f| (f as f32 / 32.0, f as f32)).collect::<Vec<_>>(), env);
    // open 2 frames at every 4th frame from 0 to 8: steps at frames 0, 6, 12
    let mut c = at(&[0, 2, 4, 6, 8], &mut env);
    assert_eq!(c.frame_stretch(0, 8, 2, 4, 1, &mut env), 1);
    // frame 0: everything +2 → 2 4 6 8 10; frame 6: keys from 6 on +2 → 2 4 8 10 12; frame 12: 2 4 8 10 14
    assert_eq!(frames(&c), [2.0, 4.0, 8.0, 10.0, 14.0]);
    // close 2 frames at frame 0: the keys of frames 0 and 1 go (the cut is from 1e-5 s before
    // frame 0 to 1e-5 s before frame 2), the rest come 2 frames closer
    let mut c = at(&[0, 1, 2, 4, 8], &mut env);
    assert_eq!(c.frame_stretch(0, 1, 2, 4, 0, &mut env), 1);
    assert_eq!(frames(&c), [0.0, 2.0, 6.0]);
    assert_eq!([c.frame_stretch(4, 4, 2, 4, 1, &mut env), c.frame_stretch(0, 8, 2, 0, 1, &mut env)], [0, 0]);
}

#[test]
fn cycling_a_block_of_keys() {
    let mut env = Env::new();
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 1.0), (1.5, 7.0), (5.0, 9.0)], &mut env);
    c.cycle_keys_between_times(0.0, 1.0, 3, &mut env);
    // two copies of the block, shifted by 1 and 2; the key at 1.5 lay in their way; the key at 5 moves up
    assert_eq!(times(&c), [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 5.0]);
    assert_eq!(values(&c), [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 9.0]);
    let types: Vec<u16> = c.k().iter().map(|k| k.key_type()).collect();
    assert_eq!(types, [0, 0, 1, 1, 1, 1, 0]);
    // fewer copies than keys in the way: the count shrinks but nothing moves down
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 1.0), (1.2, 7.0), (1.4, 7.0), (1.6, 7.0), (5.0, 9.0)], &mut env);
    c.cycle_keys_between_times(0.0, 1.0, 2, &mut env);
    assert_eq!(times(&c), [0.0, 1.0, 1.0, 2.0, 1.6]);
    // one cycle is what is there already
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 1.0), (5.0, 9.0)], &mut env);
    c.cycle_keys_between_times(0.0, 1.0, 1, &mut env);
    assert_eq!(times(&c), [0.0, 1.0, 5.0]);
}

#[test]
fn copying_between_curves() {
    let mut env = Env::new();
    let src = curve(interp::LINEAR, &[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)], &mut env);
    assert_eq!([src.cnt_keys_between_times(1.0, 2.0), src.cnt_keys_between_times(1.5, 9.0), src.cnt_keys_between_times(4.0, 9.0)], [2, 2, 0]);
    let mut dst = curve(interp::LINEAR, &[(0.0, 0.0), (10.0, 10.0)], &mut env);
    // (2 − 1.5)·2 + 5 and (3 − 1.5)·2 + 5, before the first key later than 5
    dst.copy_keys_between_times(&src, 5.0, 1.5, 3.0, 2.0, &mut env);
    assert_eq!((times(&dst), values(&dst)), (vec![0.0, 6.0, 8.0, 10.0], vec![0.0, 2.0, 3.0, 10.0]));
    dst.copy_keys_between_times(&src, 5.0, 7.0, 9.0, 2.0, &mut env);
    assert_eq!(dst.nkeys(), 4);
    let mut empty = Fcurve::allocate(1, 0, &env);
    // times are taken relative to the start of the copied interval, not to the first key in it
    empty.copy_keys_between_times(&src, 0.0, 0.0, 9.0, 1.0, &mut env);
    assert_eq!(times(&empty), [1.0, 2.0, 3.0]);
    empty.copy_keys_between_times(&src, 0.5, 3.0, 9.0, 1.0, &mut env);
    assert_eq!(times(&empty), [0.5, 1.0, 2.0, 3.0]);

    // paste: the source's length is its range
    let mut src = curve(interp::LINEAR, &[(0.0, 1.0), (1.0, 2.0)], &mut env);
    (src.range_start, src.range_end) = (0.0, 2.0);
    let four = |env: &mut Env| curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0), (2.0, 20.0), (3.0, 30.0)], env);
    // insert at 1: the keys from 1 on move back by 2, the source goes in at 1 and 2
    let mut dst = four(&mut env);
    assert_eq!(src.simple_paste_insert(&mut dst, 1.0, 1.0, &mut env), 1);
    assert_eq!((times(&dst), values(&dst)), (vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0], vec![0.0, 1.0, 2.0, 10.0, 20.0, 30.0]));
    // replace [1, 3]: scale (3 − 1)/2; the key strictly inside goes, the key at 1 is replaced
    let mut dst = four(&mut env);
    src.simple_paste_insert(&mut dst, 1.0, 3.0, &mut env);
    assert_eq!((times(&dst), values(&dst)), (vec![0.0, 1.0, 2.0, 3.0], vec![0.0, 1.0, 2.0, 30.0]));
}

#[test]
fn equality_within_a_ten_thousandth() {
    let mut env = Env::new();
    let a = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)], &mut env);
    let mut b = a.clone();
    assert_eq!(fcv_equal(Some(&a), Some(&b)), 1);
    b.keys.as_mut().unwrap()[1].value += 0.00005;
    assert_eq!(fcv_equal(Some(&a), Some(&b)), 1);
    b.keys.as_mut().unwrap()[1].left_slope += 0.001;
    assert_eq!(fcv_equal(Some(&a), Some(&b)), 0);
    b.keys.as_mut().unwrap()[1].left_slope = f32::NAN; // a difference that is not a number is none
    assert_eq!(fcv_equal(Some(&a), Some(&b)), 1);
    let empty = Fcurve::allocate(1, 0, &env);
    assert_eq!([fcv_equal(None, None), fcv_equal(Some(&empty), None), fcv_equal(Some(&a), None), fcv_equal(None, Some(&a)),
                fcv_equal(Some(&a), Some(&empty))], [1, 1, 0, 0, 0]);
    let c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0)], &mut env);
    assert_eq!(fcv_equal(Some(&a), Some(&c)), 0);
}

#[test]
fn queries() {
    let (mut env, log) = recording();
    let mut c = curve(interp::HERMITE, &[(0.0, 5.0), (0.5, -1.0), (1.0, 2.0)], &mut env);
    // one frame more than the span: 1 s + 1/30 s; in frames 30 + 1 + 0.5 truncated
    assert_eq!(c.range(&env).to_f32(), 1.0 + 1.0f32 / 30.0);
    assert_eq!(c.frame_range(&env), 31);
    assert_eq!(Fcurve::allocate(1, 0, &env).frame_range(&env), 1);
    // finding a key: within 0.15 frame, else the last one passed at or before the time
    assert_eq!([c.find_key(0, 0.5, &env), c.find_key(0, 0.504, &env), c.find_key(0, 0.51, &env), c.find_key(0, 0.7, &env),
                c.find_key(2, 0.7, &env), c.find_key(0, -1.0, &env), c.find_key(9, 1.0, &env)], [1, 1, 1, 1, -1, -1, 2]);
    assert_eq!([c.key_time_greater_than(0, 0.25), c.key_time_greater_than(0, 0.5), c.key_time_greater_than(0, 1.0),
                c.key_time_greater_than(2, 0.25), c.key_time_greater_than(0, -3.0)], [1, 2, 3, 2, 0]);
    assert_eq!([c.get_key_frame(0.504, &env), c.get_key_frame(0.51, &env), c.get_key_frame(0.25, &env)], [Some(1), None, None]);
    assert_eq!([c.next_key(0, &env), c.next_key(15, &env), c.next_key(30, &env)], [Some(15), Some(30), None]);
    assert_eq!([c.previous_key(30, &env), c.previous_key(15, &env), c.previous_key(0, &env)], [Some(15), Some(0), None]);
    c.keys.as_mut().unwrap()[1].flags = keyflag::RAW;
    assert_eq!([c.next_key(0, &env), c.previous_key(30, &env)], [Some(30), Some(0)]);
    assert_eq!((c.last_key(), c.key(1, &mut env), c.key(3, &mut env), c.key(4, &mut env), c.key(0, &mut env)), (Some(2), Some(0), Some(2), None, None));
    assert_eq!(log.borrow().printed, ["Fatal Error: fcurveKey, index out of range 4 (1, 3)\n", "Fatal Error: fcurveKey, index out of range 0 (1, 3)\n"]);
    // bounds: recomputed on request only
    assert_eq!((c.min_value, c.max_value), (-1.0, 5.0));
    c.keys.as_mut().unwrap()[0].value = 3.0;
    c.min_max_value();
    assert_eq!((c.min_value, c.max_value), (-1.0, 3.0));
    c.remove_all_keys(&mut env);
    c.min_max_value();
    assert_eq!((c.min_value, c.max_value, c.last_key()), (0.0, 0.0, None));
    // tags
    let mut c = curve(interp::HERMITE, &[(0.0, 5.0), (0.5, -1.0), (1.0, 2.0), (2.0, 2.0)], &mut env);
    assert_eq!(c.check_tag_index(), (false, -1, -1));
    c.keys.as_mut().unwrap()[1].tagged = 1;
    assert_eq!(c.check_tag_index(), (true, 1, -1));
    c.keys.as_mut().unwrap()[2].tagged = 1;
    c.keys.as_mut().unwrap()[3].tagged = 1;
    assert_eq!(c.check_tag_index(), (true, 1, 3));
    // the listing
    let mut c = curve(interp::HERMITE, &[(0.5, 2.0), (1.0, -0.25)], &mut env);
    c.keys.as_mut().unwrap()[1].left_slope = 1e-7;
    assert_eq!(c.print_keys(0x1f, &env), "fcurve ptr 1f, size 2\n  0 - t:0.5(15) val:2  left:0  right: 0\n  1 - t:1(30) val:-0.25  left:1e-07  right: 0\n");
}

#[test]
fn value_increments() {
    let mut env = Env::new();
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 9.0)], &mut env);
    c.inc_value_after(1, 10.0, &mut env);
    assert_eq!(values(&c), [0.0, 1.0, 14.0, 19.0]);
    // slopes follow: for key 1, (14 − 0)/(2 − 0)
    assert_eq!(c.k()[1].right_slope, 7.0);
    c.inc_value_above(1.0, -10.0, &mut env);
    assert_eq!(values(&c), [0.0, 1.0, 4.0, 9.0]);
    // a position that is no key's: every key
    c.inc_value_after(99, 1.0, &mut env);
    assert_eq!(values(&c), [1.0, 2.0, 5.0, 10.0]);
}

#[test]
fn an_activation_curve() {
    let mut env = Env::new();
    let mut c = Fcurve::allocate(kind::RELATION_ACTIVE, 0, &env);
    // on from 1 to 3
    c.active_range_set(1.0, 3.0, 1, &mut env).unwrap();
    assert_eq!((times(&c), values(&c)), (vec![1.0, 3.0], vec![1.0, 0.0]));
    assert_eq!([c.value_f32(0.5, 0, &mut env), c.value_f32(1.0, 0, &mut env), c.value_f32(2.9, 0, &mut env), c.value_f32(3.0, 0, &mut env)],
               [1.0, 1.0, 1.0, 0.0]);
    // before its first key a curve reads its first value: the range starts "on" from the left.
    // off from 1.5 to 2 inside it
    c.active_range_set(1.5, 2.0, 0, &mut env).unwrap();
    assert_eq!((times(&c), values(&c)), (vec![1.0, 1.5, 2.0, 3.0], vec![1.0, 0.0, 1.0, 0.0]));
    // on again over the hole: its keys go, and nothing needs adding
    c.active_range_set(1.5, 2.0, 1, &mut env).unwrap();
    assert_eq!((times(&c), values(&c)), (vec![1.0, 3.0], vec![1.0, 0.0]));
    // a curve that already reads "on" before and at the end of the range gets no key; a middle
    // key equal to both neighbours is dropped on the way out
    let mut d = Fcurve::allocate(kind::RELATION_ACTIVE, 0, &env);
    for (t, v) in [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (5.0, 1.0)] {
        d.add_key(Key::create(t, v, 0.0, 0, 0), &mut env);
    }
    d.active_range_set(8.0, 9.0, 1, &mut env).unwrap();
    assert_eq!((times(&d), values(&d)), (vec![0.0, 2.0, 5.0], vec![0.0, 0.0, 1.0]));
    // off from 8 to 9 on the same curve: off at 8, on again at 9
    d.active_range_set(8.0, 9.0, 0, &mut env).unwrap();
    assert_eq!((times(&d), values(&d)), (vec![0.0, 2.0, 5.0, 8.0, 9.0], vec![0.0, 0.0, 1.0, 0.0, 1.0]));
}

// ---- curves as a whole --------------------------------------------------------------------

#[test]
fn snapshot_swap_and_dispose() {
    let mut env = Env::new();
    env.cus_val_num_callback = 1;
    let mut c = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0)], &mut env);
    c.name[..4].copy_from_slice(b"rot\0");
    (c.pre_extrap, c.range_end) = (extrap::CYCLE, 9.0);
    assert_eq!((c.dirty, env.fcurve_dirty), (1, 1));
    c.snapshot(&mut env);
    let s = c.aux.as_ref().unwrap();
    assert_eq!((times(s), s.pre_extrap, s.interp, &s.name[..4], s.range_end, s.dirty, s.cache), (vec![0.0, 1.0], 2, 1, &b"rot\0"[..], 0.0, 0, 0));
    c.add_key(Key::create(2.0, 5.0, 0.0, 0, 1), &mut env);
    c.swap_with_aux();
    assert_eq!((c.nkeys(), c.aux.as_ref().unwrap().nkeys(), c.dirty, c.aux.as_ref().unwrap().dirty), (2, 3, 0, 1));
    assert!(c.aux.as_ref().unwrap().aux.is_none());
    c.swap_with_aux();
    assert_eq!((c.nkeys(), c.dirty), (3, 1));
    // an empty curve cannot be snapshot: the old snapshot stays
    let mut e = Fcurve::allocate(1, 0, &env);
    e.aux = Some(Box::new(c.clone()));
    e.aux.as_mut().unwrap().aux = None;
    e.snapshot(&mut env);
    assert_eq!(e.aux.as_ref().unwrap().nkeys(), 3);
    // freezing drops the snapshot; disposing of a dirty curve takes it out of the count
    c.freeze(&mut env);
    assert!(c.aux.is_none());
    assert_eq!(env.fcurve_dirty, 1);
    let mut slot = Some(Box::new(c));
    Fcurve::remove(&mut slot, &mut env);
    assert!(slot.is_none());
    assert_eq!(env.fcurve_dirty, 0);
    Fcurve::dispose(&mut slot, &mut env);
    // custom-effect curves have a counter of their own
    let mut k = Fcurve::allocate(kind::CUSTOM, 0, &env);
    k.add_key(Key::create(0.0, 0.0, 0.0, 0, 2), &mut env);
    assert_eq!((env.fcurve_dirty, env.cus_fcurve_dirty), (0, 1));
}

#[test]
fn transfer_and_angles() {
    let mut env = Env::new();
    let src = curve(interp::LINEAR, &[(0.0, 0.0), (1.0, 10.0)], &mut env);
    let mut dst = curve(interp::HERMITE, &[(5.0, 5.0)], &mut env);
    dst.aux = Some(Box::new(src.clone()));
    env.cus_val_num_callback = 1;
    dst.transfer_from(&src, &mut env);
    assert_eq!((times(&dst), dst.interp, dst.aux.is_none(), dst.dirty, env.fcurve_dirty), (vec![0.0, 1.0], 1, true, 1, 1));
    let empty = Fcurve::allocate(7, 3, &env);
    dst.transfer_from(&empty, &mut env);
    assert_eq!((dst.kind, dst.owner, dst.keys.is_none(), dst.dirty, env.fcurve_dirty), (7, 3, true, 1, 1));

    // spotlight cone angle, type 25: radians to degrees
    let mut a = Fcurve::allocate(25, 0, &env);
    a.keys = Some(vec![Key::create(0.0, 1.0, 0.5, 3, 2)]);
    a.convert_angle_unit_to_degree();
    assert_eq!((a.k()[0].value, a.k()[0].left_slope, a.k()[0].right_slope), (57.29578, 28.64789, 28.64789));
    let mut b = Fcurve::allocate(1, 0, &env);
    b.keys = Some(vec![Key::create(0.0, 1.0, 0.5, 3, 2)]);
    b.convert_angle_unit_to_degree();
    assert_eq!(b.k()[0].value, 1.0);
}

// ---- single keys and clipboards -------------------------------------------------------------

#[test]
fn keys_in_a_curve_set_through_their_address() {
    let (mut env, log) = recording();
    let mut c = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)], &mut env);
    c.key_set_value(1, 3.0, &mut env);
    assert_eq!((values(&c), c.k()[1].right_slope), (vec![0.0, 3.0, 4.0], 2.0));
    // the last key moves as intended
    c.key_set_time(2, 5.0, &mut env);
    assert_eq!((times(&c), values(&c)), (vec![0.0, 1.0, 5.0], vec![0.0, 3.0, 4.0]));
    // any other key is lost, and the key behind it gets the time
    c.key_set_time(0, 9.0, &mut env);
    assert_eq!((times(&c), values(&c)), (vec![9.0, 5.0], vec![3.0, 4.0]));
    assert_eq!(log.borrow().messages.len(), 1);
    let mut k = Key::new();
    k.set_vector_frame(15.0, 2.0, &env);
    assert_eq!((k.time, k.value), (0.5, 2.0));
    k.set_vector(3.0, 4.0);
    assert_eq!((k.time, k.value), (3.0, 4.0));
}

#[test]
fn the_key_clipboard() {
    let mut env = Env::new();
    let mut c = curve(interp::HERMITE, &[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)], &mut env);
    assert_eq!(c.copy_keys_to_buffer(1, &mut env), 0);
    assert!(env.fkey_buffer.is_none());
    {
        let keys = c.keys.as_mut().unwrap();
        (keys[1].tagged, keys[1].flags, keys[1].spline_index) = (1, keyflag::KEY_FRAME, 4);
        keys[2].tagged = 1;
    }
    assert_eq!(c.copy_keys_to_buffer(1, &mut env), 2);
    let b = env.fkey_buffer.clone().unwrap();
    assert_eq!((b[0].time, b[1].time, b[0].tagged, b[0].flags, b[0].spline_index, b[0].value), (0.0, 1.0, 0, 0, -1, 2.0));
    let mut d = curve(interp::HERMITE, &[(0.0, 0.0), (11.0, 9.0)], &mut env);
    assert_eq!(d.paste_keys_from_buffer(10.0, &mut env), 2);
    assert_eq!((times(&d), values(&d)), (vec![0.0, 10.0, 11.0], vec![0.0, 2.0, 3.0]));
    let tags: Vec<i16> = d.k().iter().map(|k| k.tagged).collect();
    assert_eq!(tags, [0, 1, 1]);
    assert_eq!(c.copy_keys_to_buffer(0, &mut env), 3);
    assert_eq!(Fcurve::allocate(1, 0, &env).copy_keys_to_buffer(0, &mut env), 0);
    assert_eq!(env.fkey_buffer.as_ref().unwrap().len(), 3);
}

struct Element(Vec<(i32, i32, Fcurve)>, [bool; 2], Vec<String>);

impl ElementCurves for Element {
    fn find(&mut self, group: i32, id: i32) -> Option<&mut Fcurve> {
        self.0.iter_mut().find(|(g, i, _)| *g == group && *i == id).map(|(_, _, c)| c)
    }
    fn each(&mut self, group: i32, id: i32, f: &mut dyn FnMut(&mut Fcurve)) {
        self.0.iter_mut().filter(|(g, i, _)| *g == group && *i == id).for_each(|(_, _, c)| f(c));
    }
    fn each_in_range(&mut self, group: i32, first: i32, last: i32, f: &mut dyn FnMut(&mut Fcurve)) {
        self.0.iter_mut().filter(|(g, i, _)| *g == group && (first..=last).contains(i)).for_each(|(_, _, c)| f(c));
    }
    fn has_path(&self, which: usize) -> bool {
        self.1[which]
    }
    fn key_spline_to_fcurve(&mut self, id: i32, which: usize) {
        self.2.push(format!("key {id:#x} {which}"));
    }
    fn check_spline_to_fcurve(&mut self, id: i32, which: usize) {
        self.2.push(format!("check {id:#x} {which}"));
    }
}

#[test]
fn the_per_type_clipboard() {
    let mut env = Env::new();
    let typed = |kind: i32, points: &[(f32, f32)], env: &mut Env| {
        let mut c = curve(interp::HERMITE, points, env);
        c.kind = kind;
        c
    };
    let mut a = typed(5, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)], &mut env);
    // within a hundredth of a second
    assert_eq!(fkey::copy_time_key_to_buffer(Some(&mut a), 5, 1.5, 0, &mut env), 0);
    assert_eq!(fkey::copy_time_key_to_buffer(Some(&mut a), 5, 1.004, 1, &mut env), 1);
    assert_eq!((times(&a), env.fkey_time_buffer[5].unwrap().value), (vec![0.0, 2.0], 1.0));
    assert_eq!([fkey::copy_time_key_to_buffer(Some(&mut a), 0x2a7, 0.0, 0, &mut env), fkey::copy_time_key_to_buffer(Some(&mut a), -1, 0.0, 0, &mut env)], [0, 0]);
    let mut b = typed(5, &[(0.0, 0.0)], &mut env);
    assert_eq!(b.paste_time_key_from_buffer(3.0, &mut env), 1);
    assert_eq!((times(&b), values(&b)), (vec![0.0, 3.0], vec![0.0, 1.0]));
    assert_eq!(typed(6, &[(0.0, 0.0)], &mut env).paste_time_key_from_buffer(3.0, &mut env), 0);
    // a failed copy empties the slot
    assert_eq!(fkey::copy_time_key_to_buffer(None, 5, 0.0, 0, &mut env), 0);
    assert!(env.fkey_time_buffer[5].is_none());

    // a model: every curve of groups 0, 1, 2 that has a key at the time
    let mut model = Element(vec![(0, 10, typed(10, &[(1.0, 1.0)], &mut env)), (1, 100, typed(1, &[(0.0, 0.0), (1.0, 5.0)], &mut env)),
                                 (1, 101, typed(4, &[(0.0, 0.0)], &mut env)), (2, 7, typed(300, &[(1.0, 2.0)], &mut env)),
                                 (3, 7, typed(301, &[(1.0, 2.0)], &mut env))], [false; 2], vec![]);
    assert_eq!(fkey::copy_model_key_to_buffer(Some(&mut model), 1.0, 0, &mut env), 3);
    assert_eq!(fkey::copy_model_key_to_buffer(None, 1.0, 0, &mut env), 0);
    assert_eq!(fkey::paste_model_key_from_buffer(Some(&mut model), 2.0, &mut env), 3);
    assert_eq!((times(&model.0[1].2), times(&model.0[2].2), times(&model.0[4].2)), (vec![0.0, 1.0, 2.0], vec![0.0], vec![1.0]));
    // cutting takes the keys out
    assert_eq!(fkey::copy_model_key_to_buffer(Some(&mut model), 2.0, 1, &mut env), 3);
    assert_eq!(times(&model.0[1].2), [0.0, 1.0]);
    // a light: curve 0x834 of group 0 and 0x83e..=0x858 of group 1
    let mut light = Element(vec![(0, 0x834, typed(176, &[(1.0, 1.0)], &mut env)), (1, 0x83e, typed(20, &[(1.0, 1.0)], &mut env)),
                                 (1, 0x859, typed(21, &[(1.0, 1.0)], &mut env)), (0, 0x835, typed(22, &[(1.0, 1.0)], &mut env))], [false; 2], vec![]);
    assert_eq!(fkey::copy_light_key_to_buffer(Some(&mut light), 1.0, 0, &mut env), 2);
    assert_eq!(fkey::paste_light_key_from_buffer(Some(&mut light), 4.0, &mut env), 2);
    assert_eq!((light.0[0].2.nkeys(), light.0[2].2.nkeys()), (2, 1));
    // a camera: 0x44c of group 0 and 0x456..=0x478 of group 1
    let mut camera = Element(vec![(0, 0x44c, typed(177, &[(1.0, 1.0)], &mut env)), (1, 0x478, typed(30, &[(1.0, 1.0)], &mut env))], [false; 2], vec![]);
    assert_eq!(fkey::copy_camera_key_to_buffer(Some(&mut camera), 1.0, 0, &mut env), 2);
    assert_eq!(fkey::paste_camera_key_from_buffer(Some(&mut camera), 4.0, &mut env), 2);
}

// ---- the curve editor ---------------------------------------------------------------------

#[test]
fn tags_and_locks_from_the_editor() {
    let (mut env, log) = recording();
    let mut a = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0)], &mut env);
    let mut b = curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0)], &mut env);
    let mut empty = Fcurve::allocate(1, 0, &env);
    let tags = |c: &Fcurve| -> Vec<i16> { c.k().iter().map(|k| k.tagged).collect() };
    let flags = |c: &Fcurve| -> Vec<u16> { c.k().iter().map(|k| k.flags).collect() };

    editor::set_all_key_tags(&mut [&mut a, &mut empty, &mut b], 1, 0, &mut env);
    assert_eq!((tags(&a), tags(&b), log.borrow().history), (vec![1, 1, 1], vec![1, 1], 2));
    a.keys.as_mut().unwrap()[0].tagged = 0;
    editor::set_all_key_tags(&mut [&mut a], 5, 1, &mut env);
    assert_eq!(tags(&a), [1, 0, 0]);

    // the pointer: point 3 is key 1; in the second curve nothing is near
    editor::set_key_tags(&mut [&mut a, &mut b], &mut |i, _| if i == 0 { (3, 2) } else { (0, 0x7fff_ffff) }, 1, 0, &mut env);
    assert_eq!((tags(&a), tags(&b)), (vec![1, 1, 0], vec![1, 1]));
    // a slope handle ends the walk before the second curve
    editor::set_key_tags(&mut [&mut a, &mut b], &mut |i, _| if i == 0 { (4, 2) } else { (0, 2) }, 0, 0, &mut env);
    assert_eq!((tags(&a), tags(&b)), (vec![1, 1, 0], vec![1, 1]));
    editor::set_key_tags(&mut [&mut a, &mut b], &mut |i, _| if i == 0 { (-1, 2) } else { (0, 2) }, 0, 1, &mut env);
    assert_eq!(tags(&b), [0, 1]);

    // the rectangle: three points per key, the key's own first
    let rect = Rect { x0: 10, x1: 20, y0: 0, y1: 5 };
    let mut pts = |_: usize, c: &Fcurve| -> Vec<(i32, i32)> { (0..c.nkeys() * 3).map(|p| (p / 3 * 10, if p % 3 == 0 { 3 } else { 99 })).collect() };
    editor::rect_set_key_tags(&mut [&mut a], rect, &mut pts, 0x66, &mut env);
    assert_eq!(tags(&a), [1, 0, 0]);
    editor::rect_set_key_tags(&mut [&mut a], rect, &mut pts, 0x65, &mut env);
    assert_eq!(tags(&a), [1, 1, 1]);
    editor::rect_set_key_tags(&mut [&mut a], rect, &mut pts, 0x67, &mut env);
    assert_eq!(tags(&a), [1, 1, 1]);

    // locks keep the flag bits and replace the type
    a.keys.as_mut().unwrap()[0].flags = keyflag::STRUCT | keyflag::KEY_PATH;
    a.keys.as_mut().unwrap()[1].flags = 0x80 | keyflag::PROPAG;
    editor::set_all_key_locks(&mut [&mut a], 1, 0);
    assert_eq!(flags(&a), [0x11, 0x21, 1]);
    a.keys.as_mut().unwrap()[2].flags = keyflag::RAW;
    editor::set_all_key_locks(&mut [&mut a], 0, 0); // unlocking leaves the raw key alone
    assert_eq!(flags(&a), [0x10, 0x20, 3]);
    editor::set_all_key_locks(&mut [&mut a], 0, 1); // toggling locks whatever is not locked
    assert_eq!(flags(&a), [0x11, 0x21, 1]);
    editor::set_all_key_locks(&mut [&mut a], 0, 1);
    assert_eq!(flags(&a), [0x10, 0x20, 0]);
    editor::set_key_locks(&mut [&mut a], &mut |_, _| (6, 1), 3, 0);
    assert_eq!(flags(&a), [0x10, 0x20, 3]);
    editor::set_key_locks(&mut [&mut a], &mut |_, _| (6, 1), 0, 1); // a toggle frees any type
    editor::set_key_locks(&mut [&mut a], &mut |_, _| (0, 1), 0, 1);
    assert_eq!(flags(&a), [0x11, 0x20, 0]);
    editor::rect_set_key_locks(&mut [&mut a], rect, &mut pts, 0x65);
    assert_eq!(flags(&a), [0x11, 0x21, 1]);
    editor::rect_set_key_locks(&mut [&mut a], rect, &mut pts, 0x66);
    assert_eq!(flags(&a), [0x11, 0x20, 0]);
    editor::rect_set_key_locks(&mut [&mut a], rect, &mut pts, 0x67);
    assert_eq!(flags(&a), [0x11, 0x21, 1]);
    a.set_interval_key_locks(0.0, 1.0, 0);
    assert_eq!(flags(&a), [0x10, 0x20, 1]);
    a.set_interval_key_locks(1.0, 2.0, 1);
    assert_eq!(flags(&a), [0x10, 0x21, 1]);
}

#[test]
fn key_types_of_an_element() {
    let mut env = Env::new();
    let typed = |env: &mut Env| curve(interp::HERMITE, &[(0.0, 0.0), (1.0, 1.0)], env);
    let flags = |e: &Element| -> Vec<Vec<u16>> { e.0.iter().map(|(_, _, c)| c.k().iter().map(|k| k.flags).collect()).collect() };
    let model = |env: &mut Env, path: bool| {
        Element(vec![(1, 0x64, typed(env)), (1, 0x70, typed(env)), (1, 0x66, typed(env)), (0, 0x64, typed(env))], [path, false], vec![])
    };
    let mut m = model(&mut env, false);
    editor::set_model_anim_fcurve_key_type(Some(&mut m), 1, 0, 0.0, &env);
    assert_eq!(flags(&m), [vec![1, 1], vec![1, 1], vec![0, 0], vec![0, 0]]);
    editor::set_model_anim_fcurve_key_type(Some(&mut m), 0, 1, 1.0, &env); // only the keys on that frame
    assert_eq!(flags(&m)[0], [1, 0]);
    // with a path: path keys are made by the spline code; any other change is checked against it
    let mut m = model(&mut env, true);
    editor::set_model_anim_fcurve_key_type(Some(&mut m), 2, 0, 0.0, &env);
    assert_eq!((flags(&m)[0].clone(), m.2.clone()), (vec![0, 0], vec!["key 0x70 0".to_string()]));
    editor::set_model_anim_fcurve_key_type(Some(&mut m), 1, 0, 0.0, &env);
    assert_eq!(m.2[1], "check 0x70 0");
    // a camera: curves 0x458..=0x45b; here any non-zero selector means "at the time"
    let mut c = Element(vec![(1, 0x458, typed(&mut env)), (1, 0x45b, typed(&mut env)), (1, 0x45c, typed(&mut env))], [false, true], vec![]);
    editor::set_camera_anim_fcurve_key_type(Some(&mut c), 1, 2, 1.0, &env);
    assert_eq!((flags(&c), c.2.clone()), (vec![vec![0, 1], vec![0, 1], vec![0, 0]], vec!["check 0x45a 1".to_string()]));
    editor::set_camera_anim_fcurve_key_type(Some(&mut c), 2, 0, 0.0, &env);
    assert_eq!(c.2[1], "key 0x45a 1");
    let mut l = Element(vec![(1, 0x840, typed(&mut env)), (1, 0x844, typed(&mut env))], [true, true], vec![]);
    editor::set_light_anim_fcurve_key_type(Some(&mut l), 3, 0, 0.0, &env);
    assert_eq!((flags(&l), l.2.clone()), (vec![vec![3, 3], vec![0, 0]], vec!["check 0x840 0".to_string(), "check 0x842 1".to_string()]));
}

// ---- scene refresh --------------------------------------------------------------------------

#[derive(Default)]
struct Scene(Vec<String>, i16);

impl update::SceneUpdate for Scene {
    fn custom_curve_element(&mut self, curve: &Fcurve) -> Option<(i32, i32)> {
        (curve.owner != 0).then_some((9, curve.owner as i32))
    }
    fn chapter_of_type(&mut self, curve_type: i32) -> i32 {
        if curve_type < 100 { 1 } else if curve_type < 200 { 2 } else { -1 }
    }
    fn find_elem_by_data(&mut self, _chapter: i32, owner: u32) -> i32 {
        owner as i32 - 1
    }
    fn topo_sort_by_fcv(&mut self, list: &update::UpdateList) -> i16 {
        self.0.push(format!("sort {}", list.len()));
        self.1
    }
    fn upd_list_update(&mut self, time: f32) {
        self.0.push(format!("list {time}"));
    }
    fn item_update(&mut self, chapter: i32, index: i32, time: f32) {
        self.0.push(format!("item {chapter} {index} {time}"));
    }
    fn refresh_system(&mut self, code: i32) {
        self.0.push(format!("refresh {code:#x}"));
    }
}

#[test]
fn the_scene_follows_the_edited_curves() {
    let (mut env, log) = recording();
    let make = |kind: i32, owner: u32, env: &Env| Fcurve::allocate(kind, owner, env);
    // types below 100 are chapter 1, below 200 chapter 2; the element index is owner − 1
    let curves = [make(5, 4, &env), make(150, 8, &env), make(6, 4, &env), make(7, 2, &env), make(250, 3, &env),
                  make(8, 0, &env), make(kind::CUSTOM, 6, &env), make(kind::CUSTOM, 0, &env), make(151, 8, &env)];
    let refs: Vec<&Fcurve> = curves.iter().collect();
    let mut scene = Scene(vec![], 1);
    let list = update::create_update_list(&refs, &mut scene);
    // chapters in order of appearance, each element once; unknown types and elements are left out
    assert_eq!(list, vec![(1, vec![3, 1]), (2, vec![7]), (9, vec![6])]);
    env.frame_current = 15;
    update::update_objects_by_fcurves(&list, &mut scene, &mut env);
    assert_eq!(scene.0, ["sort 3", "list 0.5", "refresh 0x37", "refresh 0x17"]);
    assert!(log.borrow().messages.is_empty());
    scene = Scene(vec![], 0);
    update::update_objects_by_fcurves(&list, &mut scene, &mut env);
    assert_eq!(log.borrow().messages.len(), 1);
    assert_eq!(log.borrow().messages[0].1, 3);
    // one by one
    scene = Scene(vec![], 1);
    env.update_option = 2;
    update::update_objects_by_fcurves(&list, &mut scene, &mut env);
    assert_eq!(scene.0, ["item 1 3 0.5", "item 1 1 0.5", "item 2 7 0.5", "item 9 6 0.5", "refresh 0x37", "refresh 0x17"]);
    assert_eq!(env.update_option, 2);
    // only when the option asks for it
    scene = Scene(vec![], 1);
    update::call_refresh_element_by_fcurve(&refs, &mut scene, &mut env);
    assert!(scene.0.is_empty());
    env.refresh_elem_fcv_option = 1;
    update::call_refresh_element_by_fcurve(&refs, &mut scene, &mut env);
    assert_eq!(scene.0.len(), 6);
}
