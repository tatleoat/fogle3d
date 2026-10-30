// SPDX-License-Identifier: MIT
//! `mi_light.c`: the lights of the mental ray scene text (package 63, finding 0042). Same conventions
//! as `mi_mr_main`: every printed line is a call of `SI_mrpipeFPRINTF` with the format's address and the
//! argument words; callees of other files, and of this file, are `Routine`s called through `Outside`.

use crate::mi_calc::SDEBUG;
use crate::mi_mr_main::{ext, g, MRPIPE_FPRINTF};
use crate::mi_patch::VECTOR_X_MATRIX;
use crate::mi_poly::NORMALIZE_VECTOR;
use si_mem::{Mem, Outside, Routine};
use si_x87::{trig, F80};

pub const SI_ELEMLIST_COUNT_VALID_ELEMS: Routine = Routine::new("SI_elemlistCountValidElems", 0x0813_3cc0);
pub const PRINT_LIGHT: Routine = Routine::new("print_light", 0x0859_8df0);
pub const SI_PRINT_DEFAULT_LIGHT: Routine = Routine::new("si_printDefaultLight", 0x0859_94a8);
const CID_LIGHTS: u32 = 0x0878_1678;
const CID_LIGHT_SHADERS: u32 = 0x0878_16a4;
const SOFT_PREFIX: u32 = 0x086e_f9b8;

fn fp(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

/// A `double` as its two stack words.
pub(crate) fn dw(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// The float at `a`, widened and pushed as a `double`.
fn fw(m: &Mem, a: u32) -> [u32; 2] {
    dw(m.f32(a) as f64)
}

fn f(m: &Mem, a: u32) -> F80 {
    F80::from_f32(m.f32(a))
}

/// [soft 0x085994a8 si_printDefaultLight]: the light written when a scene has none: a white point light
/// "default" at (100, 100, 100) taken through the matrix.
pub fn si_print_default_light(m: &mut Mem, o: &mut dyn Outside, mat: u32) {
    let l = m.local(24);
    let (out, inp) = (l, l + 12);
    o.call(m, SDEBUG, &[0x086f_56a0]);
    for k in 0..3 {
        m.set_f32(inp + 4 * k, 100.0);
    }
    o.call(m, VECTOR_X_MATRIX, &[out, inp, mat]);
    let pre = m.u32(SOFT_PREFIX);
    fp(m, o, &[0x086f_56c7, pre]);
    fp(m, o, &[0x086f_56e5]);
    fp(m, o, &[0x086f_56fd]);
    let mut a = vec![0x086f_570f];
    for k in 0..3 {
        a.extend(fw(m, out + 4 * k));
    }
    fp(m, o, &a);
    fp(m, o, &[0x086f_5723]);
    m.release(24);
}

/// [soft 0x08598d50 print_light_defs]: every light of the dictionary that is not hidden (+0x48), has a
/// data record (+0x20) and no parent (+0x54); the default light when there are none.
pub fn print_light_defs(m: &mut Mem, o: &mut dyn Outside, dict: u32, mat: u32, a3: u32, a4: u32) {
    if dict == 0 {
        return;
    }
    let (a3, a4) = (a3 as u16 as i16 as i32 as u32, a4 as u16 as i16 as i32 as u32);
    let cid = m.u32(CID_LIGHTS);
    let list = cid.wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let count = m.u32(list + 4) as i32;
    let valid = o.call(m, SI_ELEMLIST_COUNT_VALID_ELEMS, &[list]).eax;
    if valid == 0 {
        o.call(m, SI_PRINT_DEFAULT_LIGHT, &[mat]);
        return;
    }
    let mut off = 0u32;
    let mut n = count;
    while n > 0 {
        let e = off.wrapping_add(m.u32(list + 8));
        if m.u16(e + 0x48) == 0 && m.u32(e + 0x20) != 0 && m.u32(e + 0x54) == 0 {
            o.call(m, PRINT_LIGHT, &[dict, e, mat, a3, a4]);
        }
        off = off.wrapping_add(0x84);
        n -= 1;
    }
}

/// [soft 0x08598df0 print_light]: one light: its debug line, the `light` declaration (or the user
/// shader's), colour, shadow, factor, attenuation, spot direction, cone and spread, the area form
/// (rectangle, sphere, disc), and the origin or direction. `hide` suppresses the shader form, `area2` picks
/// which of two area-light switches is read.
pub fn print_light(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, hide: u32, area2: u32) {
    let (hide, area2) = (hide as u16, area2 as u16);
    if elem == 0 {
        return;
    }
    let ld = m.u32(elem + 4);
    if ld == 0 {
        return;
    }
    let mut name = 0u32;
    let own = m.u32(elem);
    if own != 0 {
        name = m.u32(own + 0x10);
    }
    let kind = m.u16(ld + 0x1e);
    let kname = match kind {
        2 => 0x086f_546f,
        1 => 0x086f_5469,
        _ => 0x086f_5460,
    };
    let l = m.local(24);
    let (v1, v2) = (l, l + 12); // v2 is the light's direction (not set for a point light)
    o.call(m, VECTOR_X_MATRIX, &[v1, ld + 0x34, mat]);
    if kind != 1 {
        o.call(m, VECTOR_X_MATRIX, &[v2, ld + 0x58, mat]);
        for k in 0..3 {
            let d = (f(m, v2 + 4 * k) - f(m, v1 + 4 * k)).to_f32();
            m.set_f32(v2 + 4 * k, d);
        }
        o.call(m, NORMALIZE_VECTOR, &[v2]);
    }
    let shadow = if m.u16(ld + 0x8c) == 1 { 0x086f_5474 } else { 0x086f_5478 };
    let mut a = vec![0x086f_5480, kname];
    for k in [0x24, 0x28, 0x2c] {
        a.extend(fw(m, ld + k));
    }
    a.push(shadow);
    o.call(m, SDEBUG, &a);
    let cid = m.u32(CID_LIGHT_SHADERS);
    let udf = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x226, cid, 0, 0]).eax;
    let mut shader = false;
    let buf = m.local(256);
    if udf != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0 {
        shader = o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, buf, 0x100]).eax != 0;
    }
    if hide != 0 {
        shader = false;
    }
    let mut use_default = false;
    if shader {
        use_default = o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0;
        o.call(m, SDEBUG, &[0x086f_54c0]);
        let sep = if name != 0 { 0x086f_54ec } else { 0x086f_54eb };
        let n = if name != 0 { name } else { 0x086f_54ee };
        fp(m, o, &[0x086f_54ef, n, sep, ld, buf]);
    } else {
        let sep = if name != 0 { 0x086f_5505 } else { 0x086f_5504 };
        let n = if name != 0 { name } else { 0x086f_5507 };
        let pre = m.u32(SOFT_PREFIX);
        fp(m, o, &[0x086f_5508, n, sep, ld, pre, kname]);
    }
    // the body is written unless a shader without the default block replaces it
    let mut body = true;
    if shader {
        if use_default {
            fp(m, o, &[0x086f_5522]);
        } else {
            body = false;
        }
    }
    if body {
        let mut a = vec![0x086f_5533];
        for k in [0x24, 0x28, 0x2c] {
            a.extend(fw(m, ld + k));
        }
        fp(m, o, &a);
        let s = if m.u16(ld + 0x8c) == 1 { 0x086f_5549 } else { 0x086f_554d };
        fp(m, o, &[0x086f_5550, s]);
        let mut a = vec![0x086f_5560];
        a.extend(fw(m, ld + 0x90));
        fp(m, o, &a);
        let x = F80::from_f64(10000.0) - f(m, ld + 0x84);
        if x.lt(F80::from_f64(0.01)) {
            fp(m, o, &[0x086f_556e]);
        } else {
            fp(m, o, &[0x086f_557f]);
            let mut a = vec![0x086f_558f];
            a.extend(fw(m, ld + 0x84));
            fp(m, o, &a);
            let mut a = vec![0x086f_559f];
            a.extend(fw(m, ld + 0x88));
            fp(m, o, &a);
        }
        if kind == 2 {
            let mut a = vec![0x086f_55ae];
            for k in 0..3 {
                a.extend(fw(m, v2 + 4 * k));
            }
            fp(m, o, &a);
            let half = F80::from_f32(0.5);
            let c = trig::cos((half * f(m, ld + 0x70)).to_f64()).to_f64();
            let mut a = vec![0x086f_55c7];
            a.extend(dw(c));
            fp(m, o, &a);
            let c = trig::cos((half * f(m, ld + 0x70) + half * f(m, ld + 0x74)).to_f64()).to_f64();
            let mut a = vec![0x086f_55d6];
            a.extend(dw(c));
            fp(m, o, &a);
        }
        if shader && use_default {
            fp(m, o, &[0x086f_55e6]);
        }
    }
    if shader {
        if use_default {
            fp(m, o, &[0x086f_55e9]);
        }
        let fc = m.u32(g::SI_FRAME_CURRENT);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
    }
    if kind.wrapping_sub(1) < 2 {
        fp(m, o, &[0x086f_55ec]);
        if m.u16(ld + 0x182) != 0 {
            let on = if area2 != 0 { m.u16(0x0879_1102) } else { m.u16(0x0879_117e) };
            if on != 0 {
                match m.u32(ld + 0x184) {
                    0 => {
                        let t = m.local(36);
                        let (r1, r2, r3) = (t, t + 12, t + 24);
                        o.call(m, VECTOR_X_MATRIX, &[r1, ld + 0x190, mat]);
                        o.call(m, VECTOR_X_MATRIX, &[r2, ld + 0x19c, mat]);
                        o.call(m, VECTOR_X_MATRIX, &[r3, ld + 0x1a8, mat]);
                        let c = F80::from_f64(0.5);
                        let d = |m: &Mem, hi: u32, lo: u32| dw((c * (f(m, hi) - f(m, lo))).to_f64());
                        let mut a = vec![0x086f_5600];
                        a.extend(d(m, r3, r2));
                        a.extend(d(m, r3 + 4, r2 + 4));
                        a.extend(d(m, r3 + 8, r2 + 8));
                        a.extend(d(m, r2, r1));
                        a.extend(d(m, r2 + 4, r1 + 4));
                        a.extend(d(m, r2 + 8, r1 + 8));
                        a.push(m.u32(ld + 0x188));
                        a.push(m.u32(ld + 0x18c));
                        fp(m, o, &a);
                        m.release(36);
                    }
                    1 => {
                        let mut a = vec![0x086f_5624];
                        a.extend(fw(m, ld + 0x190));
                        a.push(m.u32(ld + 0x188));
                        a.push(m.u32(ld + 0x18c));
                        fp(m, o, &a);
                    }
                    2 => {
                        let t = m.local(12);
                        o.call(m, VECTOR_X_MATRIX, &[t, ld + 0x190, mat]);
                        let mut a = vec![0x086f_5637];
                        for k in 0..3 {
                            a.extend(fw(m, t + 4 * k));
                        }
                        a.extend(fw(m, ld + 0x19c));
                        a.push(m.u32(ld + 0x188));
                        a.push(m.u32(ld + 0x18c));
                        fp(m, o, &a);
                        m.release(12);
                    }
                    _ => {}
                }
            }
        }
        let mut a = vec![0x086f_5651];
        for k in 0..3 {
            a.extend(fw(m, v1 + 4 * k));
        }
        fp(m, o, &a);
    } else {
        let mut a = vec![0x086f_5664];
        for k in 0..3 {
            a.extend(fw(m, v2 + 4 * k));
        }
        fp(m, o, &a);
    }
    fp(m, o, &[0x086f_567d]);
    m.release(256);
    m.release(24);
}
