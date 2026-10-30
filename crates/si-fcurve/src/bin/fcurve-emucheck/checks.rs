// SPDX-License-Identifier: MIT
//! One check per routine of the original. Vectors: `re/tools/si-fcurve/emu_gen.py`.
use crate::record::*;
use si_fcurve::{edit, eval, Env, Fcurve, Key, X87};
use si_x87::vectors::{Case, Check, Verdict, Word};

fn st0(c: &Case, got: X87) -> Result<(), String> {
    // a result that depends on memory nobody wrote has no value to compare with
    let Some(v) = c.output("st0").written().first().copied().flatten() else { return Ok(()) };
    let mut want = [0u8; 10];
    want.copy_from_slice(&v.to_le_bytes()[..10]);
    let mut b = got.to_bytes();
    b[0] ^= mutate() as u8;
    if b == want {
        Ok(())
    } else {
        Err(format!("st0: port {:02x?}", b.iter().rev().collect::<Vec<_>>()))
    }
}

fn eax(c: &Case, got: i32) -> Result<(), String> {
    let Some(e) = c.output("eax").written()[0].map(|v| v as u32 as i32) else { return Ok(()) };
    if e == got ^ mutate() as i32 {
        Ok(())
    } else {
        Err(format!("eax: port {got:#x}, original {e:#x}"))
    }
}

fn f(c: &Case, name: &str) -> f32 {
    c.input(name).f32()
}

fn i(c: &Case, name: &str) -> i32 {
    c.input(name).values()[0] as u32 as i32
}

fn s(c: &Case, name: &str) -> i16 {
    c.input(name).values()[0] as u16 as i16
}

/// The frame of a check on one curve: build the curve and the environment from the case,
/// run the port, then compare what the port says the routine returns (`ret`), the curve,
/// the globals and the calls into the program.
fn on_curve(c: &Case, run: impl FnOnce(&mut Fcurve, &mut Env) -> Result<(), String>) -> Verdict {
    // A case marked UNINIT is one in which the original read memory nobody wrote; the port
    // cannot follow it there (it answers `Undefined`, or does what its comment says), so the
    // case is not compared. One family is the exception, see `state_before_undefined`.
    let strict = c.name.ends_with("#inactive-cycle");
    if c.has_note("UNINIT") && !strict {
        return Verdict::Skip;
    }
    let mut curve = curve_from(c, "curve", "keys");
    let (mut env, log) = env_from(c);
    let ran = run(&mut curve, &mut env);
    if ran.as_ref().err().is_some_and(|e| e == UNDEFINED) {
        if strict {
            return state_before_undefined(c, &curve);
        }
        UNDEFINED_BUT_STABLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Verdict::Skip;
    }
    let r = ran
        .and_then(|()| curve_same(c, "curve", "k", &curve))
        .and_then(|()| env_same(c, &env))
        .and_then(|()| host_same(c, &log.borrow()));
    match r {
        Err(e) if e == NEGATIVE_COUNT => {
            NEGATIVE_COUNTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Verdict::Skip
        }
        r => verdict(r),
    }
}

/// An inactive curve whose keys all lie before time 0, cycle extrapolation after the last
/// key, and a time before the first key: the evaluator wraps the time as passed in, lands in
/// front of the first key and takes the key record in front of the array as the start of a
/// segment. Its result is undefined, but before that read it has stored the time it used
/// (0, at +0x10) and the search index −1 (at +0x50), and the port stores the same two before
/// it answers `Undefined`. Those two are compared here; the count is printed by `main`.
fn state_before_undefined(c: &Case, curve: &Fcurve) -> Verdict {
    let w = c.after("curve").words();
    let time = w[4].value().map(|v| v as u32);
    let index = w[49].value().map(|v| v as u32 as i32);
    if time == Some(curve.last_time.to_bits() ^ mutate() as u32) && index == Some(curve.cache) {
        STATE_BEFORE_UNDEFINED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Verdict::Skip
    } else {
        Verdict::Different(format!("before the undefined read: port time {:08x} index {}, original {time:x?} {index:?}", curve.last_time.to_bits(), curve.cache))
    }
}

/// Cases of the family above in which the two stores agree.
pub static STATE_BEFORE_UNDEFINED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Cases in which the original leaves a key count below zero (`SI_fcvCycleKeysBetweenTimes`
/// with fewer than one cycle only lowers the count). A `Vec` cannot hold that; the port ends
/// with no keys. Not compared; `main` prints the count.
pub static NEGATIVE_COUNTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const UNDEFINED: &str = "undefined";

fn empty_array(cv: &Fcurve) -> Result<(), String> {
    if cv.keys.is_some() && cv.nkeys() == 0 {
        Err(UNDEFINED.to_string())
    } else {
        Ok(())
    }
}

/// Cases the port refuses as undefined (the original reads a key record outside the array)
/// although the emulated original gave the same result on two different memory contents.
/// They are not compared; `main` prints the count.
pub static UNDEFINED_BUT_STABLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn undefined<T>(r: Result<T, eval::Undefined>) -> Result<T, String> {
    r.map_err(|_| UNDEFINED.to_string())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        // ---- evaluation
        "fcurveFloatValue" => |c: &Case| on_curve(c, |cv, env| st0(c, undefined(cv.float_value(f(c, "time"), s(c, "check"), env))?)),
        "SI_fcurveIntegerValue" => {
            |c: &Case| on_curve(c, |cv, env| eax(c, undefined(cv.integer_value(f(c, "time"), s(c, "check"), s(c, "flag"), env))?))
        }
        "SI_fcurveValue" => |c: &Case| on_curve(c, |cv, env| st0(c, undefined(cv.value(f(c, "time"), s(c, "check"), env))?)),
        "SI_fcurveFrameValue" => |c: &Case| on_curve(c, |cv, env| st0(c, undefined(cv.frame_value(i(c, "fr"), env))?)),
        "SI_frameToSec" => |c: &Case| verdict(st0(c, eval::frame_to_sec(i(c, "f"), &env_from(c).0))),
        "SI_secToFrame" => |c: &Case| verdict(eax(c, eval::sec_to_frame(f(c, "t"), &env_from(c).0))),
        "SI_secAsFrame" => |c: &Case| verdict(st0(c, eval::sec_as_frame(f(c, "t"), &env_from(c).0))),
        "calculateSlope" => |c: &Case| verdict(st0(c, edit::calculate_slope(f(c, "t0"), f(c, "v0"), f(c, "t1"), f(c, "v1")))),
        "SI_fcnManageSlope" => |c: &Case| on_curve(c, |cv, _| eax(c, cv.manage_slope(i(c, "i")))),
        "SI_fcurveDefaultSlope" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.default_slope(env);
                Ok(())
            })
        },

        // ---- one curve and scalars
        // an array that is present but empty: the original reads the key record in front
        // of it, the port answers one frame (said at `Fcurve::range`); not compared
        "fcurveFrameRange" => |c: &Case| on_curve(c, |cv, env| empty_array(cv).and_then(|()| eax(c, cv.frame_range(env)))),
        "fcurveRange" => |c: &Case| on_curve(c, |cv, env| empty_array(cv).and_then(|()| st0(c, cv.range(env)))),
        "SI_fcurveFindKey" => |c: &Case| on_curve(c, |cv, env| eax(c, cv.find_key(i(c, "start"), f(c, "time"), env))),
        "SI_fcvKeyTimeGreaterThan" => |c: &Case| on_curve(c, |cv, _| eax(c, cv.key_time_greater_than(i(c, "start"), f(c, "time")))),
        "SI_fcurveRemoveAllKeys" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.remove_all_keys(env);
                Ok(())
            })
        },
        "SI_fcurveRemoveKeyIndex" => |c: &Case| on_curve(c, |cv, env| truth(c, cv.remove_key_index(i(c, "index"), env))),
        "fcurveRemoveKeyBetweenIndex" => {
            |c: &Case| on_curve(c, |cv, env| truth(c, cv.remove_key_between_index(i(c, "first"), i(c, "last"), s(c, "so"), env)))
        }
        "si_fcvCheckStructKey" => |c: &Case| on_curve(c, |cv, _| ax(c, cv.check_struct_key(f(c, "t0"), f(c, "t1")) as i16)),
        "si_fcvSetStructKeyState" => |c: &Case| on_curve(c, |cv, _| ax(c, cv.set_struct_key_state(f(c, "t0"), f(c, "t1"), s(c, "on")))),
        "SI_fcvMoveKeyTime" => |c: &Case| on_curve(c, |cv, env| ax(c, cv.move_key_time(f(c, "t0"), f(c, "t1"), f(c, "dt"), s(c, "so"), env))),
        "SI_fcvSetKeysOnFrames" => |c: &Case| on_curve(c, |cv, env| eax(c, cv.set_keys_on_frames(f(c, "t0"), f(c, "t1"), env))),
        "SI_fcvScaleKeyTime" => |c: &Case| {
            on_curve(c, |cv, env| ax(c, cv.scale_key_time(f(c, "start"), f(c, "pivot"), f(c, "end"), f(c, "scale"), s(c, "so"), s(c, "all"), env)))
        },
        "si_fcvFrameStretch" => |c: &Case| {
            on_curve(c, |cv, env| eax(c, cv.frame_stretch(i(c, "first"), i(c, "last"), i(c, "gap"), i(c, "step"), s(c, "insert"), env)))
        },
        "si_fcurveIncValueAbove" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.inc_value_above(f(c, "thr"), f(c, "inc"), env);
                Ok(())
            })
        },
        "SI_fcurveOffset" => |c: &Case| on_curve(c, |cv, env| truth(c, cv.offset(f(c, "dt"), env))),
        "si_fcurveScale" => |c: &Case| on_curve(c, |cv, env| truth(c, cv.scale(f(c, "scale"), env))),
        "si_fcurveValScale" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.val_scale(f(c, "scale"), f(c, "pivot"), env);
                Ok(())
            })
        },
        "SI_fcurveScaleOrigin" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.scale_origin(f(c, "scale"), f(c, "pivot"), env);
                Ok(())
            })
        },
        "fcurveSpread" => |c: &Case| on_curve(c, |cv, env| truth(c, cv.spread(f(c, "amount"), env))),
        "SI_fcurveOffsetAndScale" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.offset_and_scale(f(c, "start"), f(c, "pivot"), f(c, "end"), f(c, "offset"), f(c, "scale"), env);
                Ok(())
            })
        },
        "si_fcurveRemoveKeyBetween" => |c: &Case| on_curve(c, |cv, env| truth(c, cv.remove_key_between(f(c, "low"), f(c, "high"), env))),
        // a start index below zero makes the original compare with key records in front of
        // the array; the port starts at 0 instead and says so. Not compared (when the two
        // garbage records of a vector happen to steer the original the same way the case is
        // not marked UNINIT, which is why the test is here and not left to the notes)
        "SI_fcurveRemoveKeyBetweenTimes" => |c: &Case| {
            on_curve(c, |cv, env| {
                if i(c, "start") < 0 && cv.nkeys() > 0 {
                    return Err(UNDEFINED.to_string());
                }
                truth(c, cv.remove_key_between_times(i(c, "start"), f(c, "t0"), f(c, "t1"), s(c, "so"), env))
            })
        },
        "si_fcurveRemoveKeyRound" => |c: &Case| on_curve(c, |cv, env| truth(c, cv.remove_key_round(f(c, "delta"), env))),
        "si_fcvCheckTagIndex" => |c: &Case| {
            on_curve(c, |cv, _| {
                let (found, a, b) = cv.check_tag_index();
                ax(c, found as i16)?;
                out_i32(c, "a", a)?;
                out_i32(c, "b", b)
            })
        },
        "si_fcurveSetIntervalKeyLocks" => |c: &Case| {
            on_curve(c, |cv, _| {
                cv.set_interval_key_locks(f(c, "t0"), f(c, "t1"), s(c, "on"));
                Ok(())
            })
        },
        "si_fcurveLastKey" => |c: &Case| on_curve(c, |cv, _| key_ptr(c, cv.last_key(), true)),
        "SI_fcurveKey" => |c: &Case| on_curve(c, |cv, env| key_ptr(c, cv.key(i(c, "index"), env), false)),
        "SI_fcurveSetInterp" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.set_interp(s(c, "kind"), env);
                Ok(())
            })
        },
        "setFcurveKeyType" => |c: &Case| {
            on_curve(c, |cv, _| {
                cv.set_key_type(i(c, "flags"));
                Ok(())
            })
        },
        "setTimeFcurveKeyType" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.set_time_key_type(i(c, "flags"), f(c, "time"), env);
                Ok(())
            })
        },
        "SI_fcvMinMaxValue" => |c: &Case| {
            on_curve(c, |cv, _| {
                cv.min_max_value();
                Ok(())
            })
        },
        "SI_fcurveNextKey" => |c: &Case| on_curve(c, |cv, env| found_frame(c, cv.next_key(i(c, "fr"), env))),
        "SI_fcurvePreviousKey" => |c: &Case| on_curve(c, |cv, env| found_frame(c, cv.previous_key(i(c, "fr"), env))),
        "si_fcurveActiveRangeSet" => |c: &Case| on_curve(c, |cv, env| undefined(cv.active_range_set(f(c, "t0"), f(c, "t1"), s(c, "on"), env))),
        "SI_fcvGetKeyFrame" => |c: &Case| on_curve(c, |cv, env| key_ptr(c, cv.get_key_frame(f(c, "time"), env), false)),
        "SI_fcurveCheckForRawKeys" => |c: &Case| on_curve(c, |cv, _| ax(c, cv.check_for_raw_keys() as i16)),
        "SI_fcurveRemoveRawKeys" => |c: &Case| {
            on_curve(c, |cv, _| {
                cv.remove_raw_keys();
                Ok(())
            })
        },
        "SI_fcvCntKeysBetweenTimes" => |c: &Case| on_curve(c, |cv, _| eax(c, cv.cnt_keys_between_times(f(c, "t0"), f(c, "t1")))),
        "SI_fcvCycleKeysBetweenTimes" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.cycle_keys_between_times(f(c, "t0"), f(c, "t1"), i(c, "cycles"), env);
                Ok(())
            })
        },
        "SI_fcurveSortKeys" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.sort_keys(env);
                Ok(())
            })
        },
        "SI_fcvConvertAngleUnitToDegree" => |c: &Case| {
            on_curve(c, |cv, _| {
                cv.convert_angle_unit_to_degree();
                Ok(())
            })
        },
        "SI_fcurveInitialize" => |c: &Case| {
            on_curve(c, |cv, _| {
                cv.initialize();
                Ok(())
            })
        },
        "printKeys" => |c: &Case| {
            on_curve(c, |cv, env| {
                let text = cv.print_keys(c.output("addr").u32(), env);
                env.host.printf(&text);
                Ok(())
            })
        },

        // ---- routines that take a key
        "SI_fcurveAddKey" => |c: &Case| on_curve(c, |cv, env| eax(c, cv.add_key(key_from(c.input("key").words()), env))),
        "SI_fcurveAddKeyAfter" => |c: &Case| {
            on_curve(c, |cv, env| eax(c, cv.add_key_after(key_from(c.input("key").words()), i(c, "start"), s(c, "warn"), s(c, "settime"), env)))
        },
        "SI_fcurveInitKey" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.init_key(&key_from(c.input("key").words()), i(c, "index"), s(c, "lock"), s(c, "settime"), env);
                Ok(())
            })
        },
        "si_fcurveRemoveKey" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.remove_key(slot(c), env);
                Ok(())
            })
        },
        "fcurveKeyIndex" => |c: &Case| on_curve(c, |cv, _| eax(c, cv.key_index(slot(c)))),
        "fcurveIncValueAfter" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.inc_value_after(slot(c), f(c, "inc"), env);
                Ok(())
            })
        },

        // ---- keys on their own
        "SI_fkeyInitialize" => |c: &Case| {
            let mut k = blank();
            k.initialize();
            // the interpolation word is the one field the original leaves alone
            let untouched = c.output("key").words()[4].value().is_none();
            verdict(lone_key_same(c, "key", &k).and_then(|()| if untouched != mutate() { Ok(()) } else { Err("the original writes the interpolation".into()) }))
        },
        "si_fkeyNew" => |c: &Case| verdict(lone_key_same(c, "ret", &Key::new())),
        "SI_fkeyCopy" => |c: &Case| verdict(lone_key_same(c, "ret", &key_from(c.input("key").words()).copy())),
        "fkeyTransfer" => |c: &Case| verdict(lone_key_same(c, "dst", &key_from(c.input("key").words()).copy())),
        "SI_fkeyCreate" => |c: &Case| verdict(lone_key_same(c, "ret", &Key::create(f(c, "time"), f(c, "value"), f(c, "slope"), s(c, "stype"), s(c, "interp")))),
        "SI_fkeyFrameCreate" => |c: &Case| {
            let k = Key::frame_create(i(c, "fr"), f(c, "value"), f(c, "slope"), s(c, "stype"), s(c, "interp"), &env_from(c).0);
            verdict(lone_key_same(c, "ret", &k))
        },
        "SI_fkeyFrame" => |c: &Case| verdict(eax(c, key_from(c.input("key").words()).frame(&env_from(c).0))),
        "SI_fkeySetTime" => |c: &Case| {
            let mut k = key_from(c.input("key").words());
            k.set_time(f(c, "time"));
            verdict(lone_key_same(c, "key", &k))
        },
        "SI_fkeySetValue" => |c: &Case| {
            let mut k = key_from(c.input("key").words());
            k.set_value(f(c, "value"));
            verdict(lone_key_same(c, "key", &k))
        },
        "SI_fkeySetValue#incurve" => |c: &Case| {
            on_curve(c, |cv, env| {
                cv.key_set_value(slot(c), f(c, "value"), env);
                Ok(())
            })
        },
        "si_fkeySetVectorFrame" => |c: &Case| {
            let (mut k, v) = (key_from(c.input("key").words()), c.input("v").f32s());
            k.set_vector_frame(v[0], v[1], &env_from(c).0);
            verdict(lone_key_same(c, "key", &k))
        },
        "fkeySetVector" => |c: &Case| {
            let (mut k, v) = (key_from(c.input("key").words()), c.input("v").f32s());
            k.set_vector(v[0], v[1]);
            verdict(lone_key_same(c, "key", &k))
        },

        // ---- two curves
        "SI_fcvEqual" => |c: &Case| {
            let a = curve_from(c, "a", "akeys");
            let b = if c.input("b").is_null() { None } else { Some(curve_from(c, "b", "bkeys")) };
            verdict(ax(c, edit::fcv_equal(Some(&a), b.as_ref())))
        },
        // the first argument is the source, the second the destination
        "SI_fcurveTransfer" => |c: &Case| {
            on_two(c, |a, b, env| {
                b.transfer_from(a, env);
                Ok(())
            })
        },
        "SI_fcvCopyKeysBetweenTimes" => |c: &Case| {
            on_two(c, |a, b, env| {
                a.copy_keys_between_times(b, f(c, "at"), f(c, "t0"), f(c, "t1"), f(c, "scale"), env);
                Ok(())
            })
        },
        "SI_fcvSimplePasteInsert" => |c: &Case| on_two(c, |a, b, env| eax(c, a.simple_paste_insert(b, f(c, "t0"), f(c, "t1"), env))),

        // ---- allocation, disposal, the snapshot
        "SI_fcurveAllocate" => |c: &Case| {
            let cv = Fcurve::allocate(i(c, "kind"), c.input("owner").u32(), &env_from(c).0);
            verdict(curve_same(c, "ret", "none", &cv))
        },
        "SI_fcvFreeze" => |c: &Case| {
            on_aux(c, |cv, env| {
                cv.freeze(env);
                Ok(())
            })
        },
        "SI_fcvSnapshot" => |c: &Case| {
            on_aux(c, |cv, env| {
                cv.snapshot(env);
                Ok(())
            })
        },
        "SI_fcvSwapWithAux" => |c: &Case| {
            on_aux(c, |cv, _| {
                cv.swap_with_aux();
                Ok(())
            })
        },
        // the port drops the curve; compared are the cleared pointer, the counters, the calls
        // into the program, and that the original frees exactly the blocks the curve owns
        "SI_fcurveDispose" => |c: &Case| disposal(c, |slot, env| Fcurve::dispose(slot, env)),
        "SI_fcurveRemove" => |c: &Case| disposal(c, |slot, env| Fcurve::remove(slot, env)),

        // ---- the key clipboards
        "si_copyFkeyToBuffer" => |c: &Case| on_clip(c, |cv, env| eax(c, cv.copy_keys_to_buffer(i(c, "tagged"), env))),
        "si_pasteFkeyToBuffer" => |c: &Case| on_clip(c, |cv, env| eax(c, cv.paste_keys_from_buffer(f(c, "time"), env))),
        "copyTimeFKeyToBuffer" => |c: &Case| {
            on_time_slot(c, i(c, "slot"), |cv, env| eax(c, si_fcurve::fkey::copy_time_key_to_buffer(Some(cv), i(c, "slot"), f(c, "time"), i(c, "remove"), env)))
        },
        "pasteTimeFkeyFromBuffer" => |c: &Case| on_time_slot(c, i(c, "type"), |cv, env| eax(c, cv.paste_time_key_from_buffer(f(c, "time"), env))),
        _ => return None,
    })
}

/// As [`on_curve`], with the key clipboard (`fkeyBuffer`, `fkeyBufferSize`): the case's
/// `old` buffer before the call, the array the global points to after it (`clip`).
fn on_clip(c: &Case, run: impl FnOnce(&mut Fcurve, &mut Env) -> Result<(), String>) -> Verdict {
    on_curve(c, |cv, env| {
        let n = c.input("kbufn").values()[0] as usize;
        if !matches!(c.input("kbuf").words()[0], Word::Value { value: 0, .. }) {
            let w = c.input("old").words();
            env.fkey_buffer = Some((0..n).map(|i| key_from(&w[KEY_WORDS * i..KEY_WORDS * (i + 1)])).collect());
        }
        run(cv, env)?;
        let count = c.after("kbufn").values()[0] as u32 as i32;
        let port = env.fkey_buffer.clone().unwrap_or_default();
        if port.len() as i32 != count {
            return Err(format!("clipboard: port {} keys, original {count}", port.len()));
        }
        if count > 0 {
            let w = c.try_output("clip").map(|f| f.words()).unwrap_or_else(|| c.input("old").words());
            for (i, k) in port.iter().enumerate() {
                key_words_same(i, k, &w[KEY_WORDS * i..KEY_WORDS * (i + 1)])?;
            }
        }
        Ok(())
    })
}

/// As [`on_curve`], with one record of the per-type clipboard (`fkeyTimeBuffer`): the
/// field `tb`, filled when its curve pointer is set.
fn on_time_slot(c: &Case, slot: i32, run: impl FnOnce(&mut Fcurve, &mut Env) -> Result<(), String>) -> Verdict {
    on_curve(c, |cv, env| {
        let w = c.input("tb").words();
        let filled = |w: &[Word]| !matches!(w[5], Word::Value { value: 0, .. });
        if filled(w) {
            env.fkey_time_buffer[slot as usize] = Some(key_from(w));
        }
        run(cv, env)?;
        let after = c.after("tb").words();
        match env.fkey_time_buffer[slot as usize] {
            Some(k) if filled(after) => key_words_same(0, &k, after),
            None if !filled(after) => Ok(()),
            other => Err(format!("clipboard slot: port {}, original {}", if other.is_some() { "filled" } else { "empty" }, if filled(after) { "filled" } else { "empty" })),
        }
    })
}

fn blank() -> Key {
    Key { time: 9.0, value: 9.0, right_slope: 9.0, slope_type: 9, interp: 9, tagged: 9, left_slope: 9.0, flags: 9, spline_index: 9, field_20: 9 }
}

/// As [`on_curve`], for a routine on two curves `a` and `b`.
fn on_two(c: &Case, run: impl FnOnce(&mut Fcurve, &mut Fcurve, &mut Env) -> Result<(), String>) -> Verdict {
    let (mut a, mut b) = (curve_from(c, "a", "akeys"), curve_from(c, "b", "bkeys"));
    let (mut env, log) = env_from(c);
    let r = run(&mut a, &mut b, &mut env)
        .and_then(|()| curve_same(c, "a", "ak", &a))
        .and_then(|()| curve_same(c, "b", "bk", &b))
        .and_then(|()| env_same(c, &env))
        .and_then(|()| host_same(c, &log.borrow()));
    verdict(r)
}

/// As [`on_curve`], for a curve that may have a snapshot.
fn on_aux(c: &Case, run: impl FnOnce(&mut Fcurve, &mut Env) -> Result<(), String>) -> Verdict {
    let mut cv = curve_with_aux(c);
    let (mut env, log) = env_from(c);
    let r = run(&mut cv, &mut env)
        .and_then(|()| curve_same(c, "curve", "k", &cv))
        .and_then(|()| aux_same(c, &cv))
        .and_then(|()| env_same(c, &env))
        .and_then(|()| host_same(c, &log.borrow()));
    verdict(r)
}

fn disposal(c: &Case, run: impl FnOnce(&mut Option<Box<Fcurve>>, &mut Env)) -> Verdict {
    let cv = curve_with_aux(c);
    let mut want = Vec::new();
    if cv.keys.is_some() {
        want.push("keys".to_string());
    }
    if let Some(aux) = &cv.aux {
        if aux.keys.is_some() {
            want.push("auxkeys".to_string());
        }
        want.push("aux".to_string());
    }
    want.push("curve".to_string());
    let mut slot = Some(Box::new(cv));
    let (mut env, log) = env_from(c);
    run(&mut slot, &mut env);
    let cleared = c.output("pp").words() == [Word::Value { bytes: 4, value: 0 }];
    let r = (if slot.is_none() && cleared != mutate() { Ok(()) } else { Err("the pointer is not cleared".to_string()) })
        .and_then(|()| if freed(c) == want { Ok(()) } else { Err(format!("freed {:?}, the curve owns {want:?}", freed(c))) })
        .and_then(|()| env_same(c, &env))
        .and_then(|()| host_same(c, &log.borrow()));
    verdict(r)
}

/// A 16-bit result: only the low half of eax is defined.
fn ax(c: &Case, got: i16) -> Result<(), String> {
    let e = c.output("eax").u32() as u16 as i16;
    if e == got ^ mutate() as i16 {
        Ok(())
    } else {
        Err(format!("ax: port {got:#x}, original {e:#x}"))
    }
}

/// A result the original returns as a pointer or NULL and the port as a flag.
fn truth(c: &Case, got: bool) -> Result<(), String> {
    let e = c.output("eax").u32() != 0;
    if e == (got != mutate()) {
        Ok(())
    } else {
        Err(format!("result: port {got}, original {:#x}", c.output("eax").u32()))
    }
}

fn out_i32(c: &Case, name: &str, got: i32) -> Result<(), String> {
    match c.after(name).written()[0] {
        Some(v) if v as u32 as i32 == got => Ok(()),
        Some(v) => Err(format!("{name}: port {got}, original {}", v as u32 as i32)),
        None => Err(format!("{name}: port {got}, the original wrote nothing")),
    }
}

/// The array position a key pointer of the case stands for (`&keys+36·slot`).
fn slot(c: &Case) -> usize {
    match &c.input("key").words()[0] {
        Word::Ref { offset, .. } => *offset as usize / 36,
        _ => usize::MAX,
    }
}

/// A returned key address against the port's array position. `before_empty`: the address
/// in front of an empty array is what the port's `None` stands for (not NULL).
fn key_ptr(c: &Case, got: Option<usize>, before_empty: bool) -> Result<(), String> {
    let w = &c.output("ptr").words()[0];
    let original = match w {
        Word::Ref { name, offset } if name == "keys" || name.starts_with("heap") => Some(*offset as usize / 36),
        _ => None,
    };
    let is_null = matches!(w, Word::Value { value: 0, .. });
    let ok = match got {
        Some(p) => original == Some(p),
        None => original.is_none() && (before_empty || is_null),
    };
    if ok != mutate() {
        Ok(())
    } else {
        Err(format!("returned key: port {got:?}, original {w:?}"))
    }
}

/// `SI_fcurveNextKey` and `SI_fcurvePreviousKey`: a flag, and the frame through a pointer.
fn found_frame(c: &Case, got: Option<i32>) -> Result<(), String> {
    match got {
        Some(frame) => truth(c, true).and_then(|()| out_i32(c, "out", frame)),
        None => truth(c, false),
    }
}
