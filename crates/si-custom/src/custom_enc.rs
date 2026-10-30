// SPDX-License-Identifier: MIT
//! The text encoder of the custom data (`custom_dialog.c`): the `ANIMATION` and `DATA` sections a
//! custom effect's user data is saved as (finding 0041, "eleventh continuation", unit customrt1).
//! A string under construction is a block grown by `cusAppendString`; its length travels in a cell.

use crate::cfmt;
use crate::custom_val::*;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_FCURVE_VALUE: Routine = Routine::new("SI_fcurveValue", 0x0820_02dc);
pub const SI_SET_INDENT: Routine = Routine::new("SI_setIndent", 0x083d_c75c);
pub const PLT_FFLUSH: Routine = Routine::new("plt:fflush", 0x080c_3be8);
pub const FPUTS: Routine = Routine::new("plt:fputs", 0x080c_4408);
/// `SI_indentString` (a pointer variable), `SI_frameRate` and the key-time tolerance `_DAT_08781498` (floats).
const SI_INDENT_STRING: u32 = 0x086c_eba0;
const SI_FRAME_RATE: u32 = 0x0878_204c;
const KEY_EPSILON: u32 = 0x0878_1498;
/// `tokDATA`, `tokANIMATION` and `tokEND`: pointer variables holding the section keywords.
const TOK_DATA: u32 = 0x086d_c2d4;
const TOK_ANIMATION: u32 = 0x086d_c2e4;
const TOK_END: u32 = 0x086d_c2f0;

/// The 10 bytes of an x87 register (as a callee leaves st0) as the register model's value. Exact for every
/// normal value, infinity and zero; a NaN keeps its sign and quiet bit but not its payload; an unnormal or a
/// denormal register (never produced by a load or an operation here) is taken at its numeric value.
pub fn f80_from_reg(r: [u8; 10]) -> si_x87::F80 {
    use si_x87::F80;
    let mant = u64::from_le_bytes([r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]]);
    let se = u16::from_le_bytes([r[8], r[9]]);
    let neg = se & 0x8000 != 0;
    let e = (se & 0x7fff) as i32;
    let v = if e == 0x7fff {
        if mant << 1 == 0 { F80::INFINITY } else { F80::from_f64(f64::NAN) }
    } else if mant == 0 {
        F80::ZERO
    } else {
        // mant * 2^(e - 16383 - 63): the 64-bit significand is built exactly from two halves
        let top = F80::from_i64((mant >> 1) as i64) * F80::from_i32(2) + F80::from_i32((mant & 1) as i32);
        top.fscale(F80::from_i32(e.max(1) - 16383 - 63))
    };
    if neg { -v } else { v }
}

/// The x87 register as `fstp qword` stores it.
pub fn st0_f64(r: [u8; 10]) -> f64 {
    f80_from_reg(r).to_f64()
}

/// The x87 register as `fstp dword` stores it.
pub fn st0_f32(r: [u8; 10]) -> f32 {
    f80_from_reg(r).to_f32()
}

fn doubles(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// `SI_fcurveValue(fc, time, 0)` as the callee leaves it in st0.
fn fcurve_value(m: &mut Mem, o: &mut dyn Outside, fc: u32, time: f32) -> [u8; 10] {
    o.call(m, SI_FCURVE_VALUE, &[fc, time.to_bits(), 0]).st0.unwrap_or([0; 10])
}

/// `SI_secAsFrame(sec)`: seconds times the frame rate, in an 80-bit register (the f32 product is exact).
fn sec_as_frame(m: &Mem, sec: f32) -> si_x87::F80 {
    si_x87::F80::from_f32(sec) * m.f32(SI_FRAME_RATE)
}

/// [soft 0x08525fa0 cusAppendString]: appends `add` to the block `s` of `*lenp` characters (a first call
/// with a null block starts one); the block is grown by `SI_realloc` (to the new length plus one) or
/// made by `SI_calloc(length + 1, 1)`. Returns the (possibly new) block; the old one when the length cell
/// is null, negative, `add` is null or the allocation fails.
pub fn cus_append_string(m: &mut Mem, s: u32, lenp: u32, add: u32) -> u32 {
    if add == 0 {
        return cus_append_string_text(m, s, lenp, None);
    }
    let text = m.cstr(add);
    cus_append_string_text(m, s, lenp, Some(&text))
}

fn cus_append_string_text(m: &mut Mem, s: u32, lenp: u32, add: Option<&[u8]>) -> u32 {
    if lenp == 0 || (s != 0 && (m.u32(lenp) as i32) < 0) {
        return s;
    }
    match add {
        Some(text) => append_bytes(m, s, lenp, text),
        None => s,
    }
}

fn append_bytes(m: &mut Mem, s: u32, lenp: u32, text: &[u8]) -> u32 {
    let n = text.len() as u32;
    if s != 0 {
        let newlen = m.u32(lenp).wrapping_add(n);
        let d = m.si_realloc(s, newlen.wrapping_add(1));
        if d == 0 {
            return s;
        }
        // strcat: after the first NUL of the block
        let at = clib::strlen(m, d);
        m.set_bytes(d.wrapping_add(at), text);
        m.set_u8(d.wrapping_add(at).wrapping_add(n), 0);
        m.set_u32(lenp, newlen);
        d
    } else {
        let d = m.si_calloc(n.wrapping_add(1), 1);
        if d == 0 {
            return 0;
        }
        m.set_bytes(d, text);
        m.set_u32(lenp, n);
        d
    }
}

fn sp(m: &mut Mem, s: u32, lenp: u32, fmt: u32, args: &[u32]) -> u32 {
    let t = fmt_lit(m, fmt, args);
    append_bytes(m, s, lenp, &t)
}

/// `app(.., lit)`: `cusAppendString` of one of the program's constant strings (the image holds none).
fn app(m: &mut Mem, s: u32, lenp: u32, addr: u32) -> u32 {
    cus_append_string_text(m, s, lenp, Some(lit(addr)))
}

/// The program's constant string at this address of its read-only data (only those this file reaches).
fn lit(addr: u32) -> &'static [u8] {
    match addr {
        0x086d_cbaf | 0x086d_cbb1 | 0x086d_cbb3 | 0x086d_cbb5 | 0x086d_cbb7 | 0x086d_cbc1 | 0x086d_cbc3 | 0x086d_cd83 => b"\n",
        0x086d_cbb9 => b" %d",
        0x086d_cbbd | 0x086d_cd77 | 0x086d_cd7b | 0x086d_cd7f => b" %f",
        0x086d_cbc5 => b"%s %s\n",
        0x086d_cbcc => b"%s %d\n",
        0x086d_cbd3 => b"%s %hd\n",
        0x086d_cbdb => b"%s %f\n",
        0x086d_cbe2 => b"%s %f %f %f %f\n",
        0x086d_cd85 => b"CURVE_TYPE",
        0x086d_cd90 | 0x086d_cd92 | 0x086d_cda9 | 0x086d_cdc1 | 0x086d_cdcc | 0x086d_cddf | 0x086d_ce0c | 0x086d_ce16 | 0x086d_ce21 | 0x086d_ce31 | 0x086d_ce42 | 0x086d_ce55
        | 0x086d_ce82 | 0x086d_ce94 | 0x086d_cea1 => b" ",
        0x086d_cd94 | 0x086d_cdab | 0x086d_cdc3 | 0x086d_cdce | 0x086d_ce0e | 0x086d_ce84 | 0x086d_ce96 => b"%d",
        0x086d_cd97 => b"PRE_EXTRAPOLATION",
        0x086d_cdae => b"POST_EXTRAPOLATION",
        0x086d_cdc6 => b"FLAGS",
        0x086d_cdd1 | 0x086d_ce47 => b"INTERPOLATION",
        0x086d_cde1 | 0x086d_ce57 => b"CONSTANT",
        0x086d_cdea | 0x086d_ce60 => b"LINEAR",
        0x086d_cdf1 | 0x086d_ce67 => b"SPLINE",
        0x086d_cdf8 | 0x086d_ce6e => b"SLOPE0",
        0x086d_cdff | 0x086d_ce75 => b"PLATEAU",
        0x086d_ce07 => b"KEYS",
        0x086d_ce11 => b"TIME",
        0x086d_ce18 | 0x086d_ce23 | 0x086d_ce33 | 0x086d_ce44 => b"%f",
        0x086d_ce1b => b"VALUE",
        0x086d_ce26 => b"LEFT_SLOPE",
        0x086d_ce36 => b"RIGHT_SLOPE",
        0x086d_ce7d => b"FLAG",
        0x086d_ce87 => b"SPLINE_INDEX",
        0x086d_ce99 => b"KEYTYPE",
        0x086d_cea3 => b"FREEFORM",
        0x086d_ceac => b"KEYFRAME",
        0x086d_ceb5 => b"KEYPATH",
        0x086d_cebd => b"RAWDATA",
        _ => b"",
    }
}

/// `cfmt::format` of a constant format string.
fn fmt_lit(m: &mut Mem, addr: u32, args: &[u32]) -> Vec<u8> {
    let f = lit(addr);
    let n = f.len() as u32 + 1;
    let a = m.local(n);
    m.set_bytes(a, f);
    m.set_u8(a + n - 1, 0);
    let t = cfmt::format(m, a, args);
    m.release(n);
    t
}

/// [soft 0x085280d4 cusPrintCustomFcurve]: the value's name, then for each of `count` sample times the
/// value(s) as ` %f` (one for the scalar kinds, four for kind 4): a key's own value when its time (in
/// frames, plus the tolerance) is not before the key's, else the curve's value; a component without a
/// curve prints its stored float; then a newline. Returns the string.
pub fn cus_print_custom_fcurve(m: &mut Mem, o: &mut dyn Outside, s0: u32, lenp: u32, v: u32, count: i32, times: u32) -> u32 {
    let (ncomp, base) = if m.u16(v.wrapping_add(4)) != 4 { (1u32, v.wrapping_add(0x10)) } else { (4, v.wrapping_add(0x1c)) };
    let name = m.u32(v);
    let mut s = cus_append_string(m, s0, lenp, name);
    let mut idx = [0i32; 4];
    let mut j = 0i32;
    while j < count {
        for k in 0..ncomp {
            let fc = m.u32(v.wrapping_add(0x2c).wrapping_add(4 * k));
            if fc == 0 {
                let f = m.f32(base.wrapping_add(4 * k)) as f64;
                s = sp(m, s, lenp, 0x086d_cd7f, &doubles(f));
                continue;
            }
            let time = m.f32(times.wrapping_add(4u32.wrapping_mul(j as u32)));
            let n = m.u32(fc.wrapping_add(0xc)) as i32;
            if idx[k as usize] < n {
                let keys = m.u32(fc.wrapping_add(8));
                let key = keys.wrapping_add((idx[k as usize] as u32).wrapping_mul(0x24));
                let a = sec_as_frame(m, time);
                let ktime = m.f32(key);
                let eps = m.f32(KEY_EPSILON);
                let b = sec_as_frame(m, ktime);
                // fcompp of (A + eps) with B: only a strict "less" skips the key
                if !((a + eps).lt(b)) {
                    let val = m.f32(key.wrapping_add(4)) as f64;
                    s = sp(m, s, lenp, 0x086d_cd7b, &doubles(val));
                    idx[k as usize] += 1;
                    continue;
                }
            }
            let val = st0_f64(fcurve_value(m, o, fc, time));
            s = sp(m, s, lenp, 0x086d_cd77, &doubles(val));
        }
        j += 1;
    }
    app(m, s, lenp, 0x086d_cd83)
}

/// [soft 0x085260d4 cusPrintCustomValue]: one value as a line of the DATA section: `name text`, `name %d`,
/// `name %hd`, `name %f` or `name %f %f %f %f` by its kind (0, 2, 3, 1, 4), a kind 1 or 4 value being
/// first refreshed from its curves at `frame` when they have keys; with sample times (count > 0) kinds
/// 1 and 4 go through `cusPrintCustomFcurve`. The string unchanged for any other kind.
pub fn cus_print_custom_value(m: &mut Mem, o: &mut dyn Outside, _dlg: u32, s: u32, lenp: u32, v: u32, frame: f32, count: i32, times: u32) -> u32 {
    let t = m.u16(v.wrapping_add(4)) as i16 as i32;
    let name = m.u32(v);
    if t < 0 || t > 4 {
        return s;
    }
    let refresh = |m: &mut Mem, o: &mut dyn Outside, cell: u32, dst: u32| {
        let fc = m.u32(v.wrapping_add(cell));
        if fc != 0 && (m.u32(fc.wrapping_add(0xc)) as i32) > 0 {
            let r = st0_f32(fcurve_value(m, o, fc, frame));
            m.set_f32(v.wrapping_add(dst), r);
        }
    };
    let text = match t {
        0 => {
            let text = m.u32(v.wrapping_add(0x18));
            fmt_lit(m, 0x086d_cbc5, &[name, text])
        }
        2 => {
            let w = m.u32(v.wrapping_add(0x14));
            fmt_lit(m, 0x086d_cbcc, &[name, w])
        }
        3 => {
            let w = m.i16(v.wrapping_add(0xc)) as i32 as u32;
            fmt_lit(m, 0x086d_cbd3, &[name, w])
        }
        1 => {
            if count > 0 && times != 0 {
                return cus_print_custom_fcurve(m, o, s, lenp, v, count, times);
            }
            refresh(m, o, 0x2c, 0x10);
            let d = doubles(m.f32(v.wrapping_add(0x10)) as f64);
            fmt_lit(m, 0x086d_cbdb, &[name, d[0], d[1]])
        }
        _ => {
            if count > 0 && times != 0 {
                return cus_print_custom_fcurve(m, o, s, lenp, v, count, times);
            }
            for k in 0..4u32 {
                refresh(m, o, 0x2c + 4 * k, 0x1c + 4 * k);
            }
            let mut a = vec![name];
            for k in 0..4u32 {
                a.extend_from_slice(&doubles(m.f32(v.wrapping_add(0x1c + 4 * k)) as f64));
            }
            fmt_lit(m, 0x086d_cbe2, &a)
        }
    };
    append_bytes(m, s, lenp, &text)
}

/// [soft 0x08528638 cusFcvkeyAppendString]: one key (36 bytes: time +0, value +4, right slope +8,
/// flag +0xc, interpolation +0xe, left slope +0x18, key type and flags +0x1c, spline index +0x1e)
/// as the lines TIME, VALUE, LEFT_SLOPE, RIGHT_SLOPE, INTERPOLATION, FLAG, [SPLINE_INDEX], KEYTYPE.
pub fn cus_fcvkey_append_string(m: &mut Mem, o: &mut dyn Outside, s0: u32, lenp: u32, key: u32) -> u32 {
    o.call(m, SI_SET_INDENT, &[1]);
    let mut s = s0;
    let field = |m: &mut Mem, s: u32, label: u32, sep: u32, fmt: u32, arg: Vec<u32>| -> u32 {
        let ind = m.u32(SI_INDENT_STRING);
        let s = cus_append_string(m, s, lenp, ind);
        let s = app(m, s, lenp, label);
        let s = app(m, s, lenp, sep);
        sp(m, s, lenp, fmt, &arg)
    };
    let f = |m: &Mem, off: u32| doubles(m.f32(key.wrapping_add(off)) as f64).to_vec();
    // the first line has no indent before... (it does: the indent string leads every line)
    let a = f(m, 0);
    s = field(m, s, 0x086d_ce11, 0x086d_ce16, 0x086d_ce18, a);
    let a = f(m, 4);
    s = field(m, s, 0x086d_ce1b, 0x086d_ce21, 0x086d_ce23, a);
    let a = f(m, 0x18);
    s = field(m, s, 0x086d_ce26, 0x086d_ce31, 0x086d_ce33, a);
    let a = f(m, 8);
    s = field(m, s, 0x086d_ce36, 0x086d_ce42, 0x086d_ce44, a);
    // INTERPOLATION word
    let ind = m.u32(SI_INDENT_STRING);
    s = cus_append_string(m, s, lenp, ind);
    s = app(m, s, lenp, 0x086d_ce47);
    s = app(m, s, lenp, 0x086d_ce55);
    let interp = [0x086d_ce57u32, 0x086d_ce60, 0x086d_ce67, 0x086d_ce6e, 0x086d_ce75];
    let i = m.i16(key.wrapping_add(0xe)) as i32;
    if (0..5).contains(&i) {
        s = app(m, s, lenp, interp[i as usize]);
    }
    let w = m.i16(key.wrapping_add(0xc)) as i32 as u32;
    s = field(m, s, 0x086d_ce7d, 0x086d_ce82, 0x086d_ce84, vec![w]);
    let kt = m.u16(key.wrapping_add(0x1c));
    if kt == 2 {
        let w = m.i16(key.wrapping_add(0x1e)) as i32 as u32;
        s = field(m, s, 0x086d_ce87, 0x086d_ce94, 0x086d_ce96, vec![w]);
    }
    let ind = m.u32(SI_INDENT_STRING);
    s = cus_append_string(m, s, lenp, ind);
    s = app(m, s, lenp, 0x086d_ce99);
    s = app(m, s, lenp, 0x086d_cea1);
    let kind = match kt & 0xf {
        0 => 0x086d_cea3u32,
        1 => 0x086d_ceac,
        2 => 0x086d_ceb5,
        3 => 0x086d_cebd,
        _ => 0,
    };
    if kind != 0 {
        s = app(m, s, lenp, kind);
    }
    o.call(m, SI_SET_INDENT, &[u32::MAX]);
    s
}

/// [soft 0x08528300 cusFcurveAppendString]: a curve as CURVE_TYPE (the name and the component index),
/// PRE_EXTRAPOLATION and POST_EXTRAPOLATION (both read from the same field, +0x1a), FLAGS (+0x1c),
/// INTERPOLATION (+0x1e), KEYS (the count at +0xc) and each key through `cusFcvkeyAppendString`.
pub fn cus_fcurve_append_string(m: &mut Mem, o: &mut dyn Outside, s0: u32, lenp: u32, name: u32, index: i32, fc: u32) -> u32 {
    let ind = |m: &Mem| m.u32(SI_INDENT_STRING);
    let mut s = s0;
    let i = ind(m);
    s = cus_append_string(m, s, lenp, i);
    s = app(m, s, lenp, 0x086d_cd85);
    s = app(m, s, lenp, 0x086d_cd90);
    s = cus_append_string(m, s, lenp, name);
    s = app(m, s, lenp, 0x086d_cd92);
    s = sp(m, s, lenp, 0x086d_cd94, &[index as u32]);
    let h = |m: &Mem, off: u32| m.i16(fc.wrapping_add(off)) as i32 as u32;
    for (label, sep, fmt, off) in [(0x086d_cd97u32, 0x086d_cda9u32, 0x086d_cdabu32, 0x1au32), (0x086d_cdae, 0x086d_cdc1, 0x086d_cdc3, 0x1a), (0x086d_cdc6, 0x086d_cdcc, 0x086d_cdce, 0x1c)] {
        let i = ind(m);
        s = cus_append_string(m, s, lenp, i);
        s = app(m, s, lenp, label);
        s = app(m, s, lenp, sep);
        let w = h(m, off);
        s = sp(m, s, lenp, fmt, &[w]);
    }
    let i = ind(m);
    s = cus_append_string(m, s, lenp, i);
    s = app(m, s, lenp, 0x086d_cdd1);
    s = app(m, s, lenp, 0x086d_cddf);
    let interp = [0x086d_cde1u32, 0x086d_cdea, 0x086d_cdf1, 0x086d_cdf8, 0x086d_cdff];
    let k = m.i16(fc.wrapping_add(0x1e)) as i32;
    if (0..5).contains(&k) {
        s = app(m, s, lenp, interp[k as usize]);
    }
    let i = ind(m);
    s = cus_append_string(m, s, lenp, i);
    s = app(m, s, lenp, 0x086d_ce07);
    s = app(m, s, lenp, 0x086d_ce0c);
    let n = m.u32(fc.wrapping_add(0xc));
    s = sp(m, s, lenp, 0x086d_ce0e, &[n]);
    let mut j = 0i32;
    while j < m.u32(fc.wrapping_add(0xc)) as i32 {
        let key = m.u32(fc.wrapping_add(8)).wrapping_add((j as u32).wrapping_mul(0x24));
        s = cus_fcvkey_append_string(m, o, s, lenp, key);
        j += 1;
    }
    s
}

/// [soft 0x08527ed0 cusKeytimesUpdate]: merges the key times of the value's curves (the first for the scalar
/// kinds, four for kind 4) into the sorted array `*times` of `*count` floats: a time (in frames) more than the
/// tolerance after the last smaller entry and before the next one is inserted in a new block.
pub fn cus_keytimes_update(m: &mut Mem, v: u32, countp: u32, timesp: u32) {
    let ncomp = if m.u16(v.wrapping_add(4)) != 4 { 1u32 } else { 4 };
    let eps = m.f32(KEY_EPSILON);
    for ci in 0..ncomp {
        let fc = m.u32(v.wrapping_add(0x2c).wrapping_add(4 * ci));
        if fc == 0 || (m.u32(fc.wrapping_add(0xc)) as i32) <= 0 {
            continue;
        }
        let mut ki = 0i32;
        while ki < m.u32(fc.wrapping_add(0xc)) as i32 {
            let key = m.u32(fc.wrapping_add(8)).wrapping_add((ki as u32).wrapping_mul(0x24));
            let ktime = m.f32(key);
            let mut b = 0i32;
            let count = m.u32(countp) as i32;
            while b < count {
                let t = m.f32(m.u32(timesp).wrapping_add(4 * b as u32));
                let x = sec_as_frame(m, t);
                let y = sec_as_frame(m, ktime) - eps; // 80-bit
                // fcompp (frame(t), frame(key) - eps): continue only on "less"
                if x.lt(y) {
                    b += 1;
                } else {
                    break;
                }
            }
            let mut insert = b == count;
            if !insert {
                let t = m.f32(m.u32(timesp).wrapping_add(4 * b as u32));
                let x = sec_as_frame(m, t);
                let y = sec_as_frame(m, ktime) + eps;
                // skipped when frame(t) <= frame(key) + eps or unordered
                insert = y.lt(x);
            }
            if insert {
                let count = m.u32(countp);
                let nb = m.si_calloc(count.wrapping_add(1), 4);
                if (b as i32) < count as i32 {
                    let src = m.u32(timesp).wrapping_add(4 * b as u32);
                    clib::memcpy(m, nb.wrapping_add(4 * b as u32).wrapping_add(4), src, (count.wrapping_sub(b as u32)).wrapping_mul(4));
                }
                let kt = m.u32(key);
                m.set_u32(nb.wrapping_add(4 * b as u32), kt);
                if b > 0 {
                    let old = m.u32(timesp);
                    clib::memcpy(m, nb, old, 4 * b as u32);
                }
                let old = m.u32(timesp);
                if old != 0 {
                    m.free(old);
                    m.set_u32(timesp, 0);
                }
                m.set_u32(timesp, nb);
                m.set_u32(countp, count.wrapping_add(1));
            }
            ki += 1;
        }
    }
}

/// [soft 0x08525b88 SI_cusEncodeDataString]: the text of a dialog's custom data. For a dialog of kind 6 an
/// ANIMATION section holds the curves of its keyed values; then the DATA section: (kind 6 only) the merged
/// key times of the dialog's keyed values and of the keyed values of the list `extra`, as a count and the
/// times, then a line per value of the dialog's list (+0x54) and of `extra`, then the end keyword. With no
/// dialog the section is empty. Returns the string (0 when nothing could be made).
pub fn si_cus_encode_data_string(m: &mut Mem, o: &mut dyn Outside, dlg: u32, frame: f32, extra: u32) -> u32 {
    // the locals: the string, its length, the count of key times and their array
    let mut s = 0u32;
    let lenp = m.local(16);
    let countp = lenp.wrapping_add(4);
    let timesp = lenp.wrapping_add(8);
    m.set_u32(lenp, 0);
    m.set_u32(countp, 0);
    m.set_u32(timesp, 0);
    if dlg != 0 && m.u16(dlg) == 6 {
        let tok = m.u32(TOK_ANIMATION);
        s = cus_append_string(m, 0, lenp, tok);
        let mut v = dlg.wrapping_add(0x10);
        loop {
            v = cusval_get_next_keyed_value(m, v, 0);
            if v == 0 {
                break;
            }
            let ncomp = if m.u16(v.wrapping_add(4)) != 4 { 1 } else { 4 };
            for i in 0..ncomp {
                let fc = m.u32(v.wrapping_add(0x2c).wrapping_add(4 * i));
                if fc != 0 {
                    let name = m.u32(v);
                    s = cus_fcurve_append_string(m, o, s, lenp, name, i as i32, fc);
                }
            }
        }
        s = app(m, s, lenp, 0x086d_cbaf);
        let tok = m.u32(TOK_END);
        s = cus_append_string(m, s, lenp, tok);
        s = app(m, s, lenp, 0x086d_cbb1);
        s = app(m, s, lenp, 0x086d_cbb3);
    }
    let tok = m.u32(TOK_DATA);
    s = cus_append_string(m, s, lenp, tok);
    if dlg == 0 {
        s = app(m, s, lenp, 0x086d_cbb5);
        let tok = m.u32(TOK_END);
        s = cus_append_string(m, s, lenp, tok);
        return app(m, s, lenp, 0x086d_cbb7);
    }
    if m.u16(dlg) == 6 {
        let mut v = dlg.wrapping_add(0x10);
        loop {
            v = cusval_get_next_keyed_value(m, v, 0);
            if v == 0 {
                break;
            }
            cus_keytimes_update(m, v, countp, timesp);
        }
        if extra != 0 {
            let mut p = extra;
            if cusval_is_keyed_value(m, p) as u16 == 0 {
                p = cusval_get_next_keyed_value(m, p, 0);
            }
            while p != 0 {
                cus_keytimes_update(m, p, countp, timesp);
                p = cusval_get_next_keyed_value(m, p, 0);
            }
        }
        let count = m.u32(countp) as i32;
        if count > 0 {
            s = sp(m, s, lenp, 0x086d_cbb9, &[count as u32]);
            let mut b = 0i32;
            while b < m.u32(countp) as i32 {
                let t = m.f32(m.u32(timesp).wrapping_add(4 * b as u32)) as f64;
                s = sp(m, s, lenp, 0x086d_cbbd, &doubles(t));
                b += 1;
            }
        }
    }
    s = app(m, s, lenp, 0x086d_cbc1);
    let count = m.u32(countp) as i32;
    let times = m.u32(timesp);
    let mut p = m.u32(dlg.wrapping_add(0x54));
    while p != 0 {
        s = cus_print_custom_value(m, o, dlg, s, lenp, p, frame, count, times);
        p = m.u32(p.wrapping_add(0x44));
    }
    let mut p = extra;
    while p != 0 {
        s = cus_print_custom_value(m, o, dlg, s, lenp, p, frame, count, times);
        p = m.u32(p.wrapping_add(0x44));
    }
    let tok = m.u32(TOK_END);
    s = cus_append_string(m, s, lenp, tok);
    app(m, s, lenp, 0x086d_cbc3)
}

/// [soft 0x08524054 SI_cusDialogWriteData]: encodes the dialog's data (no extra values) and writes it to
/// the file with `fputs`, frees the text, flushes the file.
pub fn si_cus_dialog_write_data(m: &mut Mem, o: &mut dyn Outside, dlg: u32, frame: f32, fp: u32) {
    let s = si_cus_encode_data_string(m, o, dlg, frame, 0);
    if s != 0 {
        o.call(m, FPUTS, &[s, fp]);
        m.free(s);
    }
    o.call(m, PLT_FFLUSH, &[fp]);
}
