// SPDX-License-Identifier: MIT
//! `mi2_light.c`: the lights of the mental ray 2 scene text (package 63, finding 0042). Same conventions as
//! `mi_light`: printed lines are `SI_mrpipeFPRINTF` calls with the format's address and the argument words;
//! callees of other files, and of this file, are `Routine`s called through `Outside`. The frame of
//! `mi2_print_light` is laid out as the original's, so the addresses of its buffers are the original's.

use crate::mi_calc::SDEBUG;
use crate::mi_mr_main::{ext, MRPIPE_FPRINTF};
use crate::mi_patch::{MEMTRACK_ALLOCATE, SI_MEM_PARAMS, VECTOR_X_MATRIX};
use crate::mi_poly::{INVERT_MATRIX_R, NORMALIZE_VECTOR};
use si_mem::{Mem, Outside, Routine};
use si_x87::{trig, F80};
use std::cmp::Ordering;

pub const SI_INIT_MR_SOFT_LIGHT: Routine = Routine::new("SI_init_MRSoftLight", 0x0859_55e0);
pub const MI2_PRINT_LIGHT_DEFS: Routine = Routine::new("mi2_print_light_defs", 0x0859_57d4);
pub const MI2_PRINT_LIGHT: Routine = Routine::new("mi2_print_light", 0x0859_5938);
pub const MI2_PRINT_DEFAULT_LIGHT: Routine = Routine::new("mi2_printDefaultLight", 0x0859_64a8);
pub const MI2_PRINT_SOFT_LIGHT: Routine = Routine::new("mi2_print_soft_light", 0x0859_65d0);
pub const BUILD_SHADER_NAME_WITH_STRING: Routine = Routine::new("buildShaderNameWithString", 0x0859_5380);
const SI_ELEMLIST_COUNT_VALID_ELEMS: Routine = Routine::new("SI_elemlistCountValidElems", 0x0813_3cc0);
const CID_LIGHTS: u32 = 0x0878_1678;
const CID_LIGHT_SHADERS: u32 = 0x0878_16a4;
const CID_LIGHT_PHOTON_SHADERS: u32 = 0x0878_16ec;
const SOFT_PREFIX: u32 = 0x086e_f9b8;
const INCREMENTAL: u32 = 0x086e_f9bc;
const FRAME_CURRENT: u32 = 0x0878_2040;
const W_8791102: u32 = 0x0879_1102;
const W_879117E: u32 = 0x0879_117e;

fn fp(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

fn dw(v: f64) -> [u32; 2] {
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

fn strdup_lit(m: &mut Mem, lit: &[u8]) -> u32 {
    let p = m.malloc(lit.len() as u32 + 1);
    m.put_cstr(p, lit);
    p
}

/// `strdup` of the terminated string at `a`.
fn strdup_at(m: &mut Mem, a: u32) -> u32 {
    let t = m.cstr(a);
    strdup_lit(m, &t)
}

/// The shadow-map softness: `a / b` limited to 0..1 (not-a-number stays), as the original compares it on the x87 stack.
fn softness(m: &Mem, ld: u32) -> F80 {
    let r = F80::from_i32(m.i32(ld + 0x11c)) / F80::from_i32(m.i32(ld + 0x118));
    if r.lt(F80::ZERO) {
        F80::ZERO
    } else if F80::ONE.lt(r) {
        F80::ONE
    } else {
        r
    }
}

/// [soft 0x085955e0 SI_init_MRSoftLight]: the record of a light for the soft light shader: factor 1.0 at +0, the
/// shadow switch (+4) and the attenuation switch (+6, off when the range end is within 0.01 of 10000), the colour
/// divided by its largest component's power of ten above 1 (+0x20..0x28; the divisor at +0x2c), and for a spot the
/// cone angle (+0x10) and, for shadow-map type 4 (which also marks the definition at +0x182) the samples and the
/// softness (+0x18), for type 3 only the shadow-map switch (+0x14).
pub fn si_init_mr_soft_light(m: &mut Mem, _o: &mut dyn Outside, _a0: u32, elem: u32, out: u32) {
    if out == 0 || elem == 0 {
        return;
    }
    let ld = m.u32(elem + 4);
    if ld == 0 {
        return;
    }
    m.set_u32(out, 0x3f80_0000);
    m.set_u16(out + 4, (m.u16(ld + 0x8c) != 1) as u16);
    let t = F80::from_f64(10000.0) - f(m, ld + 0x84);
    m.set_u16(out + 6, if t.lt(F80::from_f64(0.01)) { 0 } else { 1 });
    m.set_u32(out + 0xc, 0x3f80_0000);
    m.set_u16(out + 8, 1);
    let (a, b, c) = (f(m, ld + 0x224), f(m, ld + 0x228), f(m, ld + 0x22c));
    let m1 = match b.compare(a) {
        Some(Ordering::Less) | Some(Ordering::Equal) => a,
        _ => b,
    };
    let m2 = if c.compare(m1) == Some(Ordering::Less) { m1 } else { c };
    let mut scale = F80::ONE;
    if m2.compare(F80::ONE) == Some(Ordering::Greater) {
        let ten = F80::from_f64(10.0);
        let mut mm = m2;
        loop {
            mm = F80::from_f32((mm / ten).to_f32());
            scale = F80::from_f32((scale * ten).to_f32());
            if mm.compare(F80::ONE) != Some(Ordering::Greater) {
                break;
            }
        }
    }
    m.set_f32(out + 0x20, (a / scale).to_f32());
    m.set_f32(out + 0x24, (b / scale).to_f32());
    m.set_u32(out + 0x18, 0);
    m.set_u32(out + 0x10, 0);
    m.set_u16(out + 0x14, 0);
    m.set_u32(out + 0x1c, 0);
    m.set_f32(out + 0x28, (c / scale).to_f32());
    m.set_f32(out + 0x2c, scale.to_f32());
    if m.u16(ld + 0x1e) != 2 {
        return;
    }
    let cone = (f(m, ld + 0x70) + f(m, ld + 0x74)) * F80::from_f64(57.29577951308232);
    m.set_f32(out + 0x10, cone.to_f32());
    let ty = m.u16(ld + 0x8c);
    if ty == 4 {
        m.set_u16(ld + 0x182, 1);
        m.set_u16(out + 0x14, 1);
        m.set_u32(out + 0x1c, m.u32(ld + 0x124));
        if m.u32(ld + 0x118) == 0 {
            m.set_u32(out + 0x18, 0);
        } else {
            m.set_f32(out + 0x18, softness(m, ld).to_f32());
        }
    } else if ty == 3 {
        m.set_u16(out + 0x14, 1);
    }
}

/// [soft 0x085964a8 mi2_printDefaultLight]: the light written when a scene has none: the default shader
/// `LIG_default` (colour, factor, intensity, energy 10000), the light, its instance and its group. The
/// returned value is the group's.
pub fn mi2_print_default_light(m: &mut Mem, o: &mut dyn Outside, _mat: u32) -> u32 {
    let l = m.local(0x5c);
    let mat = l + 0x58 - 0x40;
    o.call(m, SDEBUG, &[0x086f_49a0]);
    crate::mi_calc::init_matrix(m, mat);
    let pre = m.u32(SOFT_PREFIX);
    fp(m, o, &[0x086f_49e0, pre, pre]);
    for fmt in [0x086f_4a09, 0x086f_4a1f, 0x086f_4a37, 0x086f_4a4f, 0x086f_4a67, 0x086f_4a7e, 0x086f_4a99] {
        fp(m, o, &[fmt]);
    }
    fp(m, o, &[0x086f_4aa9, 0x086f_4a9d]);
    let pre = m.u32(SOFT_PREFIX);
    fp(m, o, &[0x086f_4abc, pre]);
    fp(m, o, &[0x086f_4ad3, 0, 0, 0, 0, 0, 0]);
    fp(m, o, &[0x086f_4ae7]);
    o.call(m, ext::PRINT_INSTANCE, &[0x086f_4af3, 0, mat]);
    let r = o.call(m, ext::PRINT_GROUP, &[0x086f_4aff, 0, 0, 0]).eax;
    m.release(0x5c);
    r
}

/// [soft 0x085965d0 mi2_print_soft_light]: the parameters of the soft light shader for a light definition: colour,
/// intensity, shadow switch, factor, attenuation (start and stop when the range end is not within 0.01 of
/// 10000), spread (the cone factor in degrees), colour use and energy factor.
pub fn mi2_print_soft_light(m: &mut Mem, o: &mut dyn Outside, ld: u32) {
    let mut a = vec![0x086f_4b0b];
    for k in [0x24, 0x28, 0x2c] {
        a.extend(fw(m, ld + k));
    }
    fp(m, o, &a);
    fp(m, o, &[0x086f_4b21]);
    let sh = if m.u16(ld + 0x8c) == 1 { 0x086f_4b36 } else { 0x086f_4b3a };
    fp(m, o, &[0x086f_4b3d, sh]);
    let mut a = vec![0x086f_4b4d];
    a.extend(fw(m, ld + 0x90));
    fp(m, o, &a);
    let t = F80::from_f64(10000.0) - f(m, ld + 0x84);
    if t.lt(F80::from_f64(0.01)) {
        fp(m, o, &[0x086f_4b5b]);
    } else {
        fp(m, o, &[0x086f_4b6c]);
        let mut a = vec![0x086f_4b7c];
        a.extend(fw(m, ld + 0x84));
        fp(m, o, &a);
        let mut a = vec![0x086f_4b8c];
        a.extend(fw(m, ld + 0x88));
        fp(m, o, &a);
    }
    let spread = (f(m, ld + 0x74) * F80::from_f64(57.29577951308232)).to_f64();
    let mut a = vec![0x086f_4b9b];
    a.extend(dw(spread));
    fp(m, o, &a);
    fp(m, o, &[0x086f_4bab]);
    fp(m, o, &[0x086f_4bbe]);
}

/// [soft 0x085957d4 mi2_print_light_defs]: the lights of the dictionary (not hidden, no parent) written through
/// `mi2_print_light`; returns the made array of their group names and stores the count: the default light's name
/// when there are none (`group_LIG_default` in an incremental file).
pub fn mi2_print_light_defs(m: &mut Mem, o: &mut dyn Outside, dict: u32, mat: u32, a3: u32, count: u32, a4: u32) -> u32 {
    let (a3, a4) = (a3 as u16 as i16 as i32 as u32, a4 as u16 as i16 as i32 as u32);
    if dict == 0 {
        return 0;
    }
    let mut arr = 0u32;
    m.set_u32(count, 0);
    let list = m.u32(CID_LIGHTS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let n = m.u32(list + 4) as i32;
    if o.call(m, SI_ELEMLIST_COUNT_VALID_ELEMS, &[list]).eax == 0 {
        arr = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, 4]).eax;
        if m.u32(INCREMENTAL) == 0 {
            let r = o.call(m, MI2_PRINT_DEFAULT_LIGHT, &[mat]).eax;
            m.set_u32(arr, r);
        } else {
            let s = strdup_lit(m, b"group_LIG_default");
            m.set_u32(arr, s);
        }
        m.set_u32(count, 1);
        return arr;
    }
    if (arr as i32) < n {
        let mut off = 0u32;
        for _ in 0..n {
            let e = off.wrapping_add(m.u32(list + 8));
            if m.u16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 {
                let c = m.u32(count).wrapping_add(1);
                m.set_u32(count, c);
            }
            off = off.wrapping_add(0x84);
        }
    }
    let cnt = m.u32(count) as i32;
    if cnt <= 0 {
        return arr;
    }
    arr = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, (cnt as u32) << 2]).eax;
    if n > 0 {
        let mut off = 0u32;
        let mut idx = 0u32;
        for _ in 0..n {
            let e = off.wrapping_add(m.u32(list + 8));
            if m.u16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 {
                let r = o.call(m, MI2_PRINT_LIGHT, &[dict, e, mat, a3, a4]).eax;
                m.set_u32(arr.wrapping_add(idx << 2), r);
                idx += 1;
            }
            off = off.wrapping_add(0x84);
        }
    }
    arr
}

/// [soft 0x08595938 mi2_print_light]: one light of the scene: the light shader (the soft light shader, or the
/// user's procedural one, the photon emitter shader), the light with its shadow map, area, direction and energy,
/// then (outside an incremental file, or when flagged) its instance and group. Returns the group's name.
pub fn mi2_print_light(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, hide: u32, a4: u32) -> u32 {
    let (hide, a4) = (hide as u16, a4 as u16);
    if elem == 0 {
        return 0;
    }
    let ld = m.u32(elem + 4);
    if ld == 0 {
        return 0;
    }
    let own = m.u32(elem);
    let name = if own != 0 { m.u32(own + 0x10) } else { 0 };
    let flags = m.u8(elem + 0x58) as i8 as i32;
    let incr = m.u32(INCREMENTAL);
    let prefix = strdup_lit(m, if incr != 0 { b"incremental " } else { b"" });
    let l = m.local(0x30);
    let (pos, z, dir) = (l, l + 12, l + 24);
    o.call(m, VECTOR_X_MATRIX, &[pos, ld + 0x34, mat]);
    let kind = m.u16(ld + 0x1e);
    if kind == 1 {
        for k in 0..3 {
            m.set_u32(dir + 4 * k, 0);
        }
    } else {
        for k in 0..3 {
            let d = (f(m, ld + 0x58 + 4 * k) - f(m, ld + 0x34 + 4 * k)).to_f32();
            m.set_f32(dir + 4 * k, d);
        }
        o.call(m, NORMALIZE_VECTOR, &[dir]);
        m.set_u32(z, 0);
        m.set_u32(z + 4, 0);
        m.set_f32(z + 8, -1.0);
    }
    let shadow = if m.u16(ld + 0x8c) == 1 { 0x086f_4680 } else { 0x086f_4684 };
    let mut a = vec![0x086f_46a0];
    for k in [0x24, 0x28, 0x2c] {
        a.extend(fw(m, ld + k));
    }
    a.push(shadow);
    o.call(m, SDEBUG, &a);

    let buf124 = m.local(0x100);
    let buf364 = m.local(0x100);
    let buf224 = m.local(0x100);
    let buf570 = m.local(0x100);
    let buf5d4 = m.local(0x64);
    let cid = m.u32(CID_LIGHT_SHADERS);
    let udf1 = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x226, cid, 0, 0]).eax;
    let mut sa = false;
    if udf1 != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf1]).eax as u16 != 0 {
        sa = o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf1, buf124, 0x100]).eax != 0;
    }
    if hide != 0 {
        sa = false;
    }
    let mut sb = false;
    if sa {
        sb = o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf1]).eax as u16 != 0;
    }
    let cid = m.u32(CID_LIGHT_PHOTON_SHADERS);
    let udf2 = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x226, cid, 0, 0]).eax;
    let (mut sc, mut sd) = (false, false);
    if udf2 != 0 {
        if o.call(m, ext::SI_UDF_IS_ENABLED, &[udf2]).eax as u16 != 0 {
            sc = o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf2, buf364, 0x100]).eax != 0;
        }
        if hide != 0 {
            sc = false;
        }
        if sc {
            sd = o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf2]).eax as u16 != 0;
        }
    }
    let mut text = b"LIG_".to_vec();
    if name != 0 {
        text.extend(m.cstr(name));
        text.push(b'-');
    }
    text.extend(m.cstr(ld));
    m.put_cstr(buf224, &text);

    let mut p620 = 0u32;
    let p614;
    if incr == 0 || flags & 2 != 0 {
        if sa {
            o.call(m, SDEBUG, &[0x086f_46e0]);
            let decl = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[udf1, buf5d4, 0x64]).eax != 0;
            let n = if decl { buf5d4 } else { buf124 };
            o.call(m, BUILD_SHADER_NAME_WITH_STRING, &[buf570, n, buf224]);
            p614 = strdup_at(m, buf570);
            fp(m, o, &[0x086f_470b, prefix, p614, buf124]);
        } else {
            let mut t = b"\"".to_vec();
            t.extend(m.cstr(m.u32(SOFT_PREFIX)));
            t.extend(b"_light-");
            t.extend(m.cstr(buf224));
            t.push(b'"');
            m.put_cstr(buf570, &t);
            p614 = strdup_at(m, buf570);
            let pre = m.u32(SOFT_PREFIX);
            fp(m, o, &[0x086f_472c, prefix, buf570, pre]);
            fp(m, o, &[0x086f_4747, 0]);
        }
        if sa && sb {
            fp(m, o, &[0x086f_4756]);
        }
        if !sa || sb {
            o.call(m, MI2_PRINT_SOFT_LIGHT, &[ld]);
        }
        if sa && sb {
            fp(m, o, &[0x086f_4767]);
        }
        if sa && sb {
            fp(m, o, &[0x086f_476a]);
        }
        if sa {
            let fc = m.u32(FRAME_CURRENT);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf1, fc]);
        }
        fp(m, o, &[0x086f_476d]);
        if sc {
            let decl = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[udf2, buf5d4, 0x64]).eax != 0;
            let n = if decl { buf5d4 } else { buf124 };
            o.call(m, BUILD_SHADER_NAME_WITH_STRING, &[buf570, n, buf224]);
            p620 = strdup_at(m, buf570);
            fp(m, o, &[0x086f_4772, prefix, p620, buf364]);
            if sd {
                fp(m, o, &[0x086f_4785]);
                o.call(m, MI2_PRINT_SOFT_LIGHT, &[ld]);
                fp(m, o, &[0x086f_4796]);
            }
            let fc = m.u32(FRAME_CURRENT);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf2, fc]);
            fp(m, o, &[0x086f_479b]);
        }
        fp(m, o, &[0x086f_479f, prefix, buf224]);
        fp(m, o, &[0x086f_47b5, p614]);
        if p614 != 0 {
            m.free(p614);
        }
        if p620 != 0 {
            fp(m, o, &[0x086f_47bc, p620]);
            m.free(p620);
        }
        if kind.wrapping_sub(1) <= 1 {
            let on = if a4 != 0 { m.u16(W_8791102) } else { m.u16(W_879117E) };
            if m.u16(ld + 0x182) != 0 && on != 0 {
                match m.u32(ld + 0x184) {
                    0 => {
                        let t = m.local(36);
                        let (r1, r2, r3) = (t, t + 12, t + 24);
                        o.call(m, VECTOR_X_MATRIX, &[r1, ld + 0x190, mat]);
                        o.call(m, VECTOR_X_MATRIX, &[r2, ld + 0x19c, mat]);
                        o.call(m, VECTOR_X_MATRIX, &[r3, ld + 0x1a8, mat]);
                        let c = F80::from_f64(0.5);
                        let d = |m: &Mem, hi: u32, lo: u32| dw((c * (f(m, hi) - f(m, lo))).to_f64());
                        let mut a = vec![0x086f_47e0];
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
                        let mut a = vec![0x086f_4804];
                        a.extend(fw(m, ld + 0x190));
                        a.push(m.u32(ld + 0x188));
                        a.push(m.u32(ld + 0x18c));
                        fp(m, o, &a);
                    }
                    2 => {
                        let t = m.local(12);
                        o.call(m, VECTOR_X_MATRIX, &[t, ld + 0x190, mat]);
                        let mut a = vec![0x086f_4817];
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
            fp(m, o, &[0x086f_4831, 0, 0, 0, 0, 0, 0]);
            if kind == 2 {
                let half = F80::from_f32(0.5);
                let arg = (half * f(m, ld + 0x70) + f(m, ld + 0x74) * half).to_f64();
                let c = trig::cos(arg).to_f64();
                let mut a = vec![0x086f_4844];
                a.extend(dw(c));
                fp(m, o, &a);
                let ty = m.u16(ld + 0x8c);
                if ty == 4 {
                    fp(m, o, &[0x086f_4851]);
                    fp(m, o, &[0x086f_4860, m.u32(ld + 0x118)]);
                    fp(m, o, &[0x086f_487a, m.u32(ld + 0x124)]);
                    if m.u32(ld + 0x118) == 0 {
                        fp(m, o, &[0x086f_4891]);
                    } else {
                        let mut a = vec![0x086f_48aa];
                        a.extend(dw(softness(m, ld).to_f64()));
                        fp(m, o, &a);
                    }
                } else if ty == 3 {
                    fp(m, o, &[0x086f_48c2]);
                    fp(m, o, &[0x086f_48d1, m.u32(ld + 0x118)]);
                    fp(m, o, &[0x086f_48eb]);
                    fp(m, o, &[0x086f_4901]);
                }
            }
        }
        if kind != 1 {
            let mut a = vec![0x086f_491a];
            for k in 0..3 {
                a.extend(fw(m, z + 4 * k));
            }
            fp(m, o, &a);
        }
        let mut a = vec![0x086f_492f];
        for k in [0x224, 0x228, 0x22c] {
            a.extend(fw(m, ld + k));
        }
        fp(m, o, &a);
        let mut a = vec![0x086f_4942];
        a.extend(fw(m, ld + 0x230));
        fp(m, o, &a);
        let c = if m.u16(ld + 0x23c) != 0 { m.u32(ld + 0x234) } else { 0 };
        fp(m, o, &[0x086f_4950, c]);
        let c = if m.u16(ld + 0x23e) != 0 { m.u32(ld + 0x238) } else { 0 };
        fp(m, o, &[0x086f_4965, c]);
        fp(m, o, &[0x086f_497c]);
    }
    if incr == 0 || flags & 1 != 0 {
        let t = m.local(0x100);
        let (minv, mz, neg) = (t, t + 0x40, t + 0x80);
        let mp;
        if kind != 1 {
            for k in 0..3 {
                m.set_f32(neg + 4 * k, -m.f32(dir + 4 * k));
            }
            o.call(m, NORMALIZE_VECTOR, &[neg]);
            crate::mi_calc::matrix_z_align(m, neg, mz);
            for k in 0..3 {
                m.set_f32(mz + 0x30 + 4 * k, m.f32(pos + 4 * k));
            }
            o.call(m, INVERT_MATRIX_R, &[minv, mz]);
        } else {
            crate::mi_calc::init_matrix(m, minv);
            for k in 0..3 {
                m.set_f32(minv + 0x30 + 4 * k, -m.f32(pos + 4 * k));
            }
        }
        mp = minv;
        let flag = (m.u32(elem + 0x20) == 0) as u32;
        o.call(m, ext::PRINT_INSTANCE, &[buf224, flag, mp]);
        m.release(0x100);
    }
    let r = o.call(m, ext::PRINT_GROUP, &[buf224, 0, 0, 0]).eax;
    m.free(prefix);
    m.release(0x64);
    m.release(0x100);
    m.release(0x100);
    m.release(0x100);
    m.release(0x100);
    m.release(0x30);
    r
}
