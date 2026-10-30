// SPDX-License-Identifier: MIT
//! Function curves as the files hold them: `fcurveio.c` (a curve, a key, a static value),
//! with the record routines of `fcv_fct.c` and `fkey_fct.c` the readers call.
//!
//! `si-fcurve` has those record routines on Rust values; they are here on the image
//! because the readers allocate the records (reported to the lead as wanted in
//! `si-fcurve`).
//!
//! The three readers share `fcurveio.c`'s static `token` ([`TOKEN_FCURVEIO`]): a key's
//! closing zero leaves it 0, so a file that ends between two keys ends the curve too (the
//! keys still to come stay blank), while a file that ends inside a key or a curve's own
//! tokens leaves a loop that cannot end.

use super::byteswap::byteswap_fread;
use super::globals::*;
use super::off::{fcurve, fkey, static_val};
use super::outside::Outside;
use super::{faults, fmov, NoReturn, Run, COUNT_LIMIT};
use si_mem::Mem;
use si_x87::F80;

/// The float a new curve's member at +0x10 starts as (0x0864d470): −100000.
pub const CURVE_FLOAT_10_BITS: u32 = 0xc7c3_5000;
/// The double up to which a file's version makes the key reader keep an old key type word
/// as a flag (0x0865c538).
pub const OLD_KEY_TYPE_UP_TO: f64 = 2.64918;
/// The double an old file's angles are multiplied by (0x0864d478): degrees in a radian.
pub const DEGREES: f64 = 57.295_779_513_082_32;
/// The curve types whose values are angles (`SI_fcvConvertAngleUnitToDegree`).
pub const ANGLE_TYPES: [i32; 20] = [0x19, 0x1a, 0xb, 0xc, 0x2c, 0x48, 0x5e, 0x5f, 0x60, 0x6e, 0x75, 0xa6, 0xa7, 0xa8, 0x7c, 0x7d, 0x7e, 0x85, 0x86, 0x87];

/// [soft 0x0820bc50 SI_fkeyInitialize]
/// A blank key: everything zero, the words at +0x1e and +0x20 −1. The 16-bit words at
/// +0x0e, +0x16 and +0x22 are not touched.
pub fn fkey_initialize(m: &mut Mem, key: u32) {
    m.set_u32(key + fkey::TIME, 0);
    m.set_u32(key + fkey::VALUE, 0);
    m.set_u32(key + fkey::RIGHT_SLOPE, 0);
    m.set_u32(key + fkey::LEFT_SLOPE, 0);
    m.set_u16(key + fkey::FLAG, 0);
    m.set_u32(key + fkey::CURVE, 0);
    m.set_u16(key + fkey::WORD_14, 0);
    m.set_u16(key + fkey::KEY_TYPE, 0);
    m.set_u16(key + fkey::SPLINE_INDEX, 0xffff);
    m.set_u16(key + fkey::WORD_20, 0xffff);
}

/// [soft 0x0820a9d0 SI_fcurveInitialize]
/// A curve's defaults: no keys, interpolation 2, the float at +0x10 −100000. The type
/// (+0x00) and what the routine does not name are left alone.
pub fn fcurve_initialize(m: &mut Mem, curve: u32) {
    m.set_u32(curve + 0x28, 0);
    m.set_u32(curve + 0x24, 0);
    m.set_u32(curve + fcurve::FLOAT_10, CURVE_FLOAT_10_BITS);
    m.set_u32(curve + 0x14, 0);
    m.set_u32(curve + 0x58, 0);
    m.set_u32(curve + 0x5c, 0);
    m.set_u32(curve + fcurve::OWNER, 0);
    m.set_u32(curve + fcurve::NB_KEYS, 0);
    m.set_u32(curve + fcurve::KEYS, 0);
    m.set_u16(curve + fcurve::PRE_EXTRAPOLATION, 0);
    m.set_u16(curve + fcurve::POST_EXTRAPOLATION, 0);
    m.set_u16(curve + fcurve::INTERPOLATION, 2);
    m.set_u16(curve + fcurve::FLAGS, 0);
    m.set_u16(curve + 0x20, 1);
    m.set_u16(curve + 0x2c, 1);
    m.set_u32(curve + 0x50, 0);
    m.set_u32(curve + fcurve::SUB_CURVE, 0);
    m.set_u16(curve + 0x60, 0);
    m.set_u8(curve + 0x2e, 0);
    m.set_u16(curve + fcurve::MARKS, 0);
}

/// [soft 0x0820a810 SI_fcurveAllocate]
/// A new curve of a type for an owner (`SI_malloc(100)`: what `SI_fcurveInitialize` does
/// not name is not cleared). Its interpolation is 0 for nineteen types, 1 for type 0xbd, 3
/// for 0x9a and 0xb8, and for every other type the program's current default (the word at
/// +0x30 of `SI_glbAnim`). Null when there is no memory.
pub fn fcurve_allocate(m: &mut Mem, kind: i32, owner: u32) -> u32 {
    let curve = m.si_malloc(fcurve::SIZE);
    if curve != 0 {
        fcurve_initialize(m, curve);
        let interpolation = match kind {
            0xaf | 0x111 | 0xb1 | 0xb0 | 0x17a | 0xb2 | 0x173 | 0x174 | 0xb6 | 0xc4 | 0x26c | 0x277 | 0x278 | 0x279 | 0x273 | 0x274 | 0x2a4 | 0x271 | 0x272 => 0,
            0xbd => 1,
            0x9a | 0xb8 => 3,
            _ => m[SI_GLB_ANIM_30],
        };
        m.set_u16(curve + fcurve::INTERPOLATION, interpolation);
        m.set_i32(curve + fcurve::TYPE, kind);
        m.set_u32(curve + fcurve::OWNER, owner);
    }
    curve
}

/// [soft 0x0820a928 SI_fcurveDispose]
/// Frees the curve `*place` points at: its keys, the curve at +0x54 the same way, the
/// record; `*place` is then null. While a custom value callback is installed a curve that
/// carries the "dirty" mark is taken off the count of such curves.
pub fn fcurve_dispose(m: &mut Mem, place: u32) {
    let curve = m.u32(place);
    if curve == 0 {
        return;
    }
    if m[SI_CUS_VAL_NUM_CALLBACK] > 0 && m.u8(curve + fcurve::MARKS) & 1 != 0 {
        if m.i32(curve + fcurve::TYPE) == 0x236 {
            m[SI_CUS_FCURVE_DIRTY] = m[SI_CUS_FCURVE_DIRTY].wrapping_sub(1);
        } else {
            m[SI_FCURVE_DIRTY] = m[SI_FCURVE_DIRTY].wrapping_sub(1);
        }
    }
    let curve = m.u32(place);
    let keys = m.u32(curve + fcurve::KEYS);
    if keys != 0 {
        m.free(keys);
        let curve = m.u32(place);
        m.set_u32(curve + fcurve::KEYS, 0);
    }
    let curve = m.u32(place);
    m.set_u32(curve + fcurve::KEYS, 0);
    let curve = m.u32(place);
    if m.u32(curve + fcurve::SUB_CURVE) != 0 {
        fcurve_dispose(m, curve + fcurve::SUB_CURVE);
    }
    let curve = m.u32(place);
    m.set_u32(curve + fcurve::NB_KEYS, 0);
    let curve = m.u32(place);
    if curve != 0 {
        m.free(curve);
        m.set_u32(place, 0);
    }
    m.set_u32(place, 0);
}

/// [soft 0x0820aa60 SI_fcvConvertAngleUnitToDegree]
/// For a curve whose type is one of the twenty angle types: value, left slope and right
/// slope of every key multiplied by the double 57.29577951308232 on the x87 (the product
/// is rounded to the register and then to a float by the store).
pub fn fcv_convert_angle_unit_to_degree(m: &mut Mem, curve: u32) {
    let kind = m.i32(curve + fcurve::TYPE);
    if !ANGLE_TYPES.contains(&kind) {
        return;
    }
    let factor = F80::from_f64(DEGREES);
    let mut i: i32 = 0;
    let mut at = 0u32;
    while i < m.i32(curve + fcurve::NB_KEYS) {
        for off in [fkey::VALUE, fkey::LEFT_SLOPE, fkey::RIGHT_SLOPE] {
            let keys = m.u32(curve + fcurve::KEYS);
            let p = keys.wrapping_add(at).wrapping_add(off);
            let v = (factor * F80::from_f32(m.f32(p))).to_f32();
            m.set_u32(p, v.to_bits());
        }
        i += 1;
        at = at.wrapping_add(fkey::SIZE);
    }
}

/// [soft 0x083facb0 fcvkeyBinaryRead]
/// One key: a token stream closed by token 0 (finding 0005 has the table), read into `key`
/// after that was made blank; with a null `key` the fields are read and dropped. A token
/// nobody knows is ignored and nothing is stepped over.
///
/// For a file version up to 2.64918 a key type word (token 7) with bit 0x10 set is not
/// stored: bit 0x20 of the key type is set instead.
pub fn fcvkey_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, key: u32) -> Run<u32> {
    if key != 0 {
        fkey_initialize(m, key);
    }
    loop {
        let got = byteswap_fread(m, o, TOKEN_FCURVEIO.addr(), 2, 1, stream);
        let token = m[TOKEN_FCURVEIO];
        if token == 0 {
            return Ok(1);
        }
        if got == 0 {
            return Err(NoReturn::Endless("fcvkeyBinaryRead"));
        }
        match token {
            1..=4 => {
                byteswap_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
                if key != 0 {
                    let off = match token {
                        1 => fkey::TIME,
                        2 => fkey::VALUE,
                        3 => fkey::LEFT_SLOPE,
                        _ => fkey::RIGHT_SLOPE,
                    };
                    let v = fmov(m[SI_FVALUE]);
                    m.set_u32(key + off, v);
                }
            }
            5 | 6 | 8 | 9 => {
                byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                if key != 0 {
                    let off = match token {
                        5 => fkey::INTERPOLATION,
                        6 => fkey::FLAG,
                        8 => fkey::SPLINE_INDEX,
                        _ => fkey::WORD_20,
                    };
                    let v = m[SI_SVALUE];
                    m.set_i16(key + off, v);
                }
            }
            7 => {
                byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                if key != 0 {
                    let version = f32::from_bits(m[SI_VERSION_INPUT]) as f64;
                    let v = m[SI_SVALUE];
                    if version <= OLD_KEY_TYPE_UP_TO && v & 0x10 != 0 {
                        m.or_u8(key + fkey::KEY_TYPE, 0x20);
                    } else {
                        m.set_i16(key + fkey::KEY_TYPE, v);
                    }
                }
            }
            _ => {}
        }
    }
}

/// [soft 0x083faeb0 SI_fcurveBinaryRead]
/// One function curve: a token stream closed by token 0 (finding 0005 has the table).
/// With a `place`, the curve found there is disposed of and a new one of type 0 for
/// `owner` is stored there and filled; without one everything is read and dropped.
///
/// The keys token (7) allocates the key array for the count read before it, rounded up to
/// a multiple of eight (`SI_malloc`: the keys beyond the count are not cleared), reads the
/// keys and gives each its curve; a second keys token loses the first array's block.
///
/// Not done: the original increments `SI_secureLicAlloc` (0x08679660) before the
/// allocation; that counter belongs to the licence scheme, which is not ported. And a
/// count of keys that the file cannot hold (two bytes a key at the least, beyond
/// [`COUNT_LIMIT`]) is not allocated for: the port stops.
pub fn fcurve_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, place: u32, owner: u32) -> Run<u32> {
    let before = faults(m);
    let mut count: i32 = 0;
    if place != 0 {
        if m.u32(place) != 0 {
            fcurve_dispose(m, place);
        }
        let curve = fcurve_allocate(m, 0, owner);
        m.set_u32(place, curve);
    }
    let curve_of = |m: &Mem| if place == 0 { 0 } else { m.u32(place) };
    loop {
        let got = byteswap_fread(m, o, TOKEN_FCURVEIO.addr(), 2, 1, stream);
        let token = m[TOKEN_FCURVEIO];
        if token == 0 {
            return Ok(1);
        }
        if got == 0 {
            return Err(NoReturn::Endless("SI_fcurveBinaryRead"));
        }
        if faults(m) != before {
            return Err(NoReturn::Fault("SI_fcurveBinaryRead"));
        }
        match token {
            1 | 2 => {
                byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                let curve = curve_of(m);
                let v = m[SI_SVALUE] as i32;
                if curve != 0 {
                    m.set_i32(curve + if token == 1 { fcurve::TYPE } else { fcurve::NB_KEYS }, v);
                }
                if token == 2 {
                    count = m[SI_SVALUE] as i32;
                }
            }
            3..=6 => {
                byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
                let curve = curve_of(m);
                if curve != 0 {
                    let off = match token {
                        3 => fcurve::PRE_EXTRAPOLATION,
                        4 => fcurve::POST_EXTRAPOLATION,
                        5 => fcurve::FLAGS,
                        _ => fcurve::INTERPOLATION,
                    };
                    let v = m[SI_SVALUE];
                    m.set_i16(curve + off, v);
                }
            }
            7 => {
                let curve = curve_of(m);
                if curve == 0 {
                    let mut left = count;
                    if left > 0 {
                        if left as u32 > COUNT_LIMIT && left as u64 * 2 > o.file_left(m, stream) as u64 {
                            return Err(NoReturn::Count("SI_fcurveBinaryRead"));
                        }
                        loop {
                            fcvkey_binary_read(m, o, stream, 0)?;
                            left -= 1;
                            if left == 0 {
                                break;
                            }
                        }
                    }
                } else {
                    let n = m.i32(curve + fcurve::NB_KEYS);
                    if n <= 0 {
                        m.set_u32(curve + fcurve::KEYS, 0);
                    } else {
                        let rounded = (n as u32).wrapping_sub(((n as u32).wrapping_sub(1) & 7).wrapping_sub(7));
                        let bytes = rounded.wrapping_mul(fkey::SIZE);
                        if (rounded as u64 * fkey::SIZE as u64) > COUNT_LIMIT as u64 && n as u64 * 2 > o.file_left(m, stream) as u64 {
                            return Err(NoReturn::Count("SI_fcurveBinaryRead"));
                        }
                        let keys = m.si_malloc(bytes);
                        let curve = m.u32(place);
                        m.set_u32(curve + fcurve::KEYS, keys);
                    }
                    let mut curve = m.u32(place);
                    let mut i: i32 = 0;
                    let mut at = 0u32;
                    while i < m.i32(curve + fcurve::NB_KEYS) {
                        let key = m.u32(curve + fcurve::KEYS).wrapping_add(at);
                        fcvkey_binary_read(m, o, stream, key)?;
                        let c = m.u32(place);
                        let keys = m.u32(c + fcurve::KEYS);
                        m.set_u32(keys.wrapping_add(at).wrapping_add(fkey::CURVE), c);
                        let c = m.u32(place);
                        let keys = m.u32(c + fcurve::KEYS);
                        m.set_u16(keys.wrapping_add(at).wrapping_add(fkey::WORD_14), 0);
                        curve = m.u32(place);
                        i += 1;
                        at = at.wrapping_add(fkey::SIZE);
                        if faults(m) != before {
                            // the key array could not be had: the original has died
                            return Err(NoReturn::Fault("SI_fcurveBinaryRead"));
                        }
                    }
                }
            }
            8 => {
                byteswap_fread(m, o, SI_IVALUE.addr(), 4, 1, stream);
                let curve = curve_of(m);
                let v = m[SI_IVALUE];
                if curve != 0 {
                    m.set_i32(curve + fcurve::NB_KEYS, v);
                }
                count = m[SI_IVALUE];
            }
            _ => {}
        }
    }
}

/// [soft 0x083fb228 SI_staticvalBinaryRead]
/// One static value: a cleared record of 8 bytes (`SI_calloc`), filled from a token stream
/// closed by token 0 (1 the type, 2 the value) and stored through `place`.
///
/// Not done: the original increments `SI_secureLicAlloc` (0x08679660) before the
/// allocation (see [`fcurve_binary_read`]).
pub fn staticval_binary_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, place: u32) -> Run<u32> {
    let value = m.si_calloc(1, static_val::SIZE);
    loop {
        let got = byteswap_fread(m, o, TOKEN_FCURVEIO.addr(), 2, 1, stream);
        let token = m[TOKEN_FCURVEIO];
        if token == 0 {
            break;
        }
        if got == 0 {
            return Err(NoReturn::Endless("SI_staticvalBinaryRead"));
        }
        if token == 1 {
            byteswap_fread(m, o, SI_SVALUE.addr(), 2, 1, stream);
            let v = m[SI_SVALUE] as i32;
            m.set_i32(value + static_val::TYPE, v);
        } else if token == 2 {
            byteswap_fread(m, o, SI_FVALUE.addr(), 4, 1, stream);
            let v = fmov(m[SI_FVALUE]);
            m.set_u32(value + static_val::VALUE, v);
        }
    }
    m.set_u32(place, value);
    Ok(1)
}
