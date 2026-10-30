// SPDX-License-Identifier: MIT
//! `mi_tools.c`: helpers of the translator for texture support, shader names and bounds (package
//! 63, finding 0042).
//!
//! Text the original keeps in its read-only data is embedded here as bytes (the image has none);
//! where a routine only passes such a literal on, its address is passed.

use crate::cprintf;
use crate::mi_mr_main::ext;
use si_mem::{clib, Mem, Outside};
use si_x87::{trig, F80};

/// The texture-support globals: bound (min/max interleaved, 3 floats at +0, +4... as `SI_txtProjectionInfo`
/// writes them), width and center.
const BOUND: u32 = 0x0882_9f8c;
const WIDTH: u32 = 0x0882_9fa4;
const CENTER: u32 = 0x0882_9fb0;
const CID_TEXTURES2D: u32 = 0x0878_16c8;
const CID_TEXTURES3D: u32 = 0x0878_16cc;
const CID_MODELS: u32 = 0x0878_1680;

fn ld(m: &Mem, a: u32) -> F80 {
    F80::from_f32(m.f32(a))
}

/// [soft 0x085952f4 buildShaderName]: `dst = src`, and when it ends in a quote, `<index>"` replaces the quote.
pub fn build_shader_name(m: &mut Mem, _o: &mut dyn Outside, dst: u32, src: u32, index: u32) {
    clib::strcpy(m, dst, src);
    let end = dst.wrapping_add(clib::strlen(m, dst));
    if m.u8(end.wrapping_sub(1)) == b'"' {
        m.set_u8(end.wrapping_sub(1), 0);
        let tmp = m.local(8);
        let text = cprintf::format_bytes(m, b"%d\"", &[index]);
        m.put_cstr(tmp, &text);
        clib::strcat(m, dst, tmp);
        m.release(8);
    }
}

/// [soft 0x08595380 buildShaderNameWithString]: `dst = src`, and with a string `s` and a closing quote on
/// `dst`: the quote becomes `-<s>"`.
pub fn build_shader_name_with_string(m: &mut Mem, _o: &mut dyn Outside, dst: u32, src: u32, s: u32) {
    clib::strcpy(m, dst, src);
    if s == 0 {
        return;
    }
    let end = dst.wrapping_add(clib::strlen(m, dst));
    if m.u8(end.wrapping_sub(1)) == b'"' {
        m.set_u8(end.wrapping_sub(1), 0);
        let tmp = m.local(4);
        m.put_cstr(tmp, b"-");
        clib::strcat(m, dst, tmp);
        clib::strcat(m, dst, s);
        m.put_cstr(tmp, b"\"");
        clib::strcat(m, dst, tmp);
        m.release(4);
    }
}

/// [soft 0x08594fb8 si_checkForTxtGlb]: 1 when the element has 2D texture relations (relation 0x190),
/// else when it has a first 3D texture relation (0x1f4).
pub fn si_check_for_txt_glb(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> i32 {
    let out = m.local(4);
    m.set_u32(out, 0);
    let cid = m.u32(CID_TEXTURES2D);
    let n = o.call(m, ext::SI_ELEMDICT_RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x190, cid]).eax;
    let r = if n != 0 {
        1
    } else {
        let cid = m.u32(CID_TEXTURES3D);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x1f4, cid, out, 0]);
        (m.u32(out) != 0) as i32
    };
    m.release(4);
    r
}

/// [soft 0x08595024 si_getMatNameExtension]: a copy of the name of the model that carries the texture
/// support of a material element (its own, or the one at +0x5c), `_selective` appended for a
/// selective one; null when the element has no textures.
pub fn si_get_mat_name_extension(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> u32 {
    let own = m.u32(elem + 4);
    let tmp = m.local(4);
    let r = if m.u8(elem + 0x5b) == 1 {
        let name = m.u32(own + 4);
        let s = clib::strdup(m, name);
        m.set_u32(tmp, s);
        o.call(m, ext::SI_STR_APPEND, &[tmp, 0x086f_398c]);
        m.u32(tmp)
    } else {
        let idx = m.u32(elem + 0x5c);
        if idx == 0xffff_ffff {
            if si_check_for_txt_glb(m, o, dict, elem) as i16 != 0 {
                let name = m.u32(own + 4);
                clib::strdup(m, name)
            } else {
                0
            }
        } else {
            let cid = m.u32(CID_MODELS);
            let e = o.call(m, ext::SI_ELEMDICT_GET_ELEM, &[dict, cid, idx]).eax;
            if si_check_for_txt_glb(m, o, dict, e) as i16 != 0 {
                let mdl = m.u32(e + 4);
                let name = m.u32(mdl + 4);
                clib::strdup(m, name)
            } else {
                0
            }
        }
    };
    m.release(4);
    r
}

/// [soft 0x08594c04 updateTxtSupport]: after `SI_txtProjectionInfo`, the translation at +0xa0..+0xa8 is
/// transformed by the 3x3 part of the matrix at +0xb8 (rows at +0xb8, +0xc8, +0xd8) and added to the
/// center (the x87 sums kept in extended precision, stored as floats at the end).
pub fn update_txt_support(m: &mut Mem, o: &mut dyn Outside, mdl: u32, txt: u32) {
    o.call(m, ext::SI_TXT_PROJECTION_INFO, &[mdl, txt, BOUND, WIDTH, CENTER]);
    let (a, b, e) = (ld(m, txt + 0xa0), ld(m, txt + 0xa4), ld(m, txt + 0xa8));
    let (c, d, f) = (ld(m, txt + 0xb8), ld(m, txt + 0xbc), ld(m, txt + 0xc0));
    let x = (c * a + d * b) + f * e;
    let (g, h, i) = (ld(m, txt + 0xc8), ld(m, txt + 0xcc), ld(m, txt + 0xd0));
    let y = (g * a + h * b) + i * e;
    let (j, k, l) = (ld(m, txt + 0xd8), ld(m, txt + 0xdc), ld(m, txt + 0xe0));
    let z = e * l + (b * k + a * j);
    let (cx, cy, cz) = (ld(m, CENTER), ld(m, CENTER + 4), ld(m, CENTER + 8));
    m.set_f32(CENTER, (x + cx).to_f32());
    m.set_f32(CENTER + 4, (y + cy).to_f32());
    m.set_f32(CENTER + 8, (z + cz).to_f32());
}

/// A float widened to the two words of a `double` argument.
fn dbl(m: &Mem, a: u32) -> [u32; 2] {
    let b = (m.f32(a) as f64).to_bits();
    [b as u32, (b >> 32) as u32]
}

fn err(m: &mut Mem, o: &mut dyn Outside, fmt: &[u8], words: &[u32]) {
    let t = cprintf::format_bytes(m, fmt, words);
    o.print(2, &t);
}

fn err3(m: &mut Mem, o: &mut dyn Outside, fmt: &[u8], addr: [u32; 3]) {
    let mut w = Vec::new();
    for a in addr {
        w.extend_from_slice(&dbl(m, a));
    }
    err(m, o, fmt, &w);
}

/// [soft 0x08594cdc dumpTxtSupport]: the debug dump of a texture support on `stderr`: the caller's
/// name, the model and texture names, the six vectors, the matrix, and the bound, width and center globals.
pub fn dump_txt_support(m: &mut Mem, o: &mut dyn Outside, mdl: u32, txt: u32, caller: u32) {
    err(m, o, b"dumpTxtSupport() called from %s\n", &[caller]);
    if mdl != 0 {
        let name = m.u32(mdl + 4);
        err(m, o, b"mdl (%s)  ", &[name]);
    }
    let name = m.u32(txt + 0x14);
    err(m, o, b"txt name (%s)\n", &[name]);
    let v = |base: u32| [base, base + 4, base + 8];
    err3(m, o, b" geo (%f, %f, %f)\n", v(txt + 0x7c));
    err3(m, o, b" rot (%f, %f, %f)\n", v(txt + 0x94));
    err3(m, o, b" ctr (%f, %f, %f)\n", v(txt + 0x88));
    err3(m, o, b" trans (%f, %f, %f)\n", v(txt + 0xa0));
    err3(m, o, b" scal (%f, %f, %f)\n", v(txt + 0xac));
    err(m, o, b" txtsup \n", &[]);
    for row in 0..4u32 {
        err(m, o, b"\t", &[]);
        for k in 0..4u32 {
            let w = dbl(m, txt + 0xb8 + row * 16 + k * 4);
            err(m, o, b"%f ", &w);
        }
        err(m, o, b"\n", &[]);
    }
    err3(m, o, b" bound min (%f, %f, %f)\n", [BOUND, BOUND + 8, BOUND + 16]);
    err3(m, o, b" bound max (%f, %f, %f)\n", [BOUND + 4, BOUND + 12, BOUND + 20]);
    err3(m, o, b" width (%f, %f, %f)\n", [WIDTH, WIDTH + 4, WIDTH + 8]);
    err3(m, o, b" center (%f, %f, %f)\n", [CENTER, CENTER + 4, CENTER + 8]);
}

/// [soft 0x08594978 findbound]: the model's bounding box at +0x10..+0x18 (min) and +0x1c..+0x24 (max),
/// started at plus and minus 100000 and widened over the points of its geometry; the kind at +0x30
/// picks the point source (-1/0/2/... see the arms below), other kinds leave the start values.
pub fn findbound(m: &mut Mem, mdl: u32) {
    m.set_f32(mdl + 0x10, 100000.0);
    m.set_f32(mdl + 0x14, 100000.0);
    m.set_f32(mdl + 0x18, 100000.0);
    m.set_f32(mdl + 0x1c, -100000.0);
    m.set_f32(mdl + 0x20, -100000.0);
    m.set_f32(mdl + 0x24, -100000.0);
    let kind = m.u32(mdl + 0x30);
    let grow = |m: &mut Mem, p: u32| {
        for i in 0..3u32 {
            let v = m.f32(p + 4 * i);
            let lo = m.f32(mdl + 0x10 + 4 * i);
            m.set_f32(mdl + 0x10 + 4 * i, if lo > v { v } else { lo });
            let hi = m.f32(mdl + 0x1c + 4 * i);
            m.set_f32(mdl + 0x1c + 4 * i, if hi < v { v } else { hi });
        }
    };
    match kind.wrapping_add(1) {
        5 => {
            let mut g = m.u32(mdl + 0x3c);
            if g == 0 {
                let h = m.u32(mdl + 0x38);
                g = m.u32(h);
            }
            let n = m.u32(g + 0x14) as i32;
            let base = m.u32(g);
            for i in 0..n.max(0) as u32 {
                grow(m, base.wrapping_add(i.wrapping_mul(0x14)));
            }
        }
        2 => {
            let g = m.u32(mdl + 0x48);
            let n = (m.u16(g + 0xc) as i32) * (m.u16(g + 0xe) as i32);
            let base = m.u32(g + 0x1a4);
            for i in 0..n.max(0) as u32 {
                grow(m, base.wrapping_add(i.wrapping_mul(0x10)));
            }
        }
        3 | 8 => {
            let base = m.u32(mdl + 0x22c);
            let n = m.u32(mdl + 0x228);
            for i in 0..n {
                grow(m, base.wrapping_add(i.wrapping_mul(0x4c)).wrapping_add(0x20));
            }
        }
        _ => {}
    }
}

/// [soft 0x085950d4 elemTxtGlbCompare]: 1 when two elements carry the same texture relations: equal
/// counts of 2D (0x190) and 3D (0x1f4) relations, the first 3D relations pointing at the same
/// record (compared by the first word), and the 2D relations pairwise the same word at +0x20.
pub fn elem_txt_glb_compare(m: &mut Mem, o: &mut dyn Outside, dict: u32, e1: u32, e2: u32) -> i32 {
    let mut r: i16 = 1;
    let l = m.local(8);
    let (out2, out1) = (l, l + 4);
    let count = |m: &mut Mem, o: &mut dyn Outside, e: u32, rel: u32, cid: u32| {
        let c = m.u32(cid);
        o.call(m, ext::SI_ELEMDICT_RELATIONS_COUNT, &[dict, e, 1, 3, 0, rel, c]).eax
    };
    let c2a = count(m, o, e1, 0x190, CID_TEXTURES2D);
    let c2b = count(m, o, e2, 0x190, CID_TEXTURES2D);
    let c3a = count(m, o, e1, 0x1f4, CID_TEXTURES3D);
    let c3b = count(m, o, e2, 0x1f4, CID_TEXTURES3D);
    if c3a != c3b {
        r = 0;
    } else if c3a != 0 {
        let cid = m.u32(CID_TEXTURES3D);
        let a = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, e1, 1, 3, 0x1f4, cid, 0, 0]).eax;
        let cid = m.u32(CID_TEXTURES3D);
        let b = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, e2, 1, 3, 0x1f4, cid, 0, 0]).eax;
        r = (m.u32(a) == m.u32(b)) as i16;
    }
    if r != 0 {
        if c2a != c2b {
            r = 0;
        } else if c2b != 0 {
            let cid = m.u32(CID_TEXTURES2D);
            let mut a = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, e1, 1, 3, 0x190, cid, 0, out1]).eax;
            let cid = m.u32(CID_TEXTURES2D);
            let mut b = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, e2, 1, 3, 0x190, cid, 0, out2]).eax;
            if m.u32(a.wrapping_add(0x20)) != m.u32(b.wrapping_add(0x20)) {
                r = 0;
            }
            if r != 0 && a != 0 && b != 0 {
                loop {
                    let cid = m.u32(CID_TEXTURES2D);
                    a = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, e1, 1, 3, 0x190, cid, 0, out1]).eax;
                    let cid = m.u32(CID_TEXTURES2D);
                    b = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, e2, 1, 3, 0x190, cid, 0, out2]).eax;
                    if a == 0 {
                        break;
                    }
                    if b != 0 && m.u32(a.wrapping_add(0x20)) != m.u32(b.wrapping_add(0x20)) {
                        r = 0;
                    }
                    if b == 0 || r == 0 {
                        break;
                    }
                }
            }
        }
    }
    m.release(8);
    r as i32
}

/// A float widened to extended precision, as `fld dword` does.
fn lf(m: &Mem, a: u32) -> F80 {
    F80::from_f32(m.f32(a))
}

/// [soft 0x085944c0 textureVert]: the texture coordinates of a point `v` (3 floats) for the mapping `lm`
/// (kind at +0x2a: 1 to 3 planar on the x, y or z axis pairs, 5 cylindrical, 6 spherical, other kinds
/// do nothing); the point goes through the matrix at +0xb8 (rows +0xb8, +0xc8, +0xd8), then through
/// the bound/width/center globals. Writes two floats at `out`. All sums are x87 extended; a store
/// to a local float rounds, and the rounded value is what later reads see.
pub fn texture_vert(m: &mut Mem, _o: &mut dyn Outside, v: u32, out: u32, lm: u32) {
    let kind = m.u16(lm + 0x2a).wrapping_sub(1) as i16;
    if !(0..=5).contains(&kind) || kind == 3 {
        return;
    }
    let (x, y, z) = (lf(m, v), lf(m, v + 4), lf(m, v + 8));
    let row = |m: &Mem, base: u32| (lf(m, lm + base) * x + lf(m, lm + base + 4) * y) + lf(m, lm + base + 8) * z;
    let a = row(m, 0xb8);
    let yy = row(m, 0xc8);
    let zz = row(m, 0xd8);
    let zero = F80::from_f32(0.0);
    // 1 - (value - bound) / width, or 0 for a width of zero
    let flip = |m: &Mem, val: F80, bound: u32, width: u32| {
        let w = lf(m, width);
        if w.eq(zero) { zero } else { F80::ONE - (val - lf(m, bound)) / w }
    };
    let plain = |m: &Mem, val: F80, bound: u32, width: u32| {
        let w = lf(m, width);
        if w.eq(zero) { zero } else { (val - lf(m, bound)) / w }
    };
    let (b0, b1, b2) = (BOUND, BOUND + 8, BOUND + 16);
    let (w0, w1, w2) = (WIDTH, WIDTH + 4, WIDTH + 8);
    let rf = |v: F80| F80::from_f32(v.to_f32());
    let pi = F80::from_f64(core::f64::consts::PI);
    let angle = |zc: F80, ac: F80| {
        let t = F80::from_f64(trig::fpatan(zc, -ac).to_f64());
        (((t + pi) / pi) * F80::from_f64(0.5)).to_f32()
    };
    match kind {
        0 => {
            m.set_f32(out, plain(m, a, b0, w0).to_f32());
            m.set_f32(out + 4, flip(m, rf(yy), b1, w1).to_f32());
        }
        1 => {
            m.set_f32(out, plain(m, a, b0, w0).to_f32());
            m.set_f32(out + 4, flip(m, rf(zz), b2, w2).to_f32());
        }
        2 => {
            m.set_f32(out, plain(m, zz, b2, w2).to_f32());
            m.set_f32(out + 4, flip(m, rf(yy), b1, w1).to_f32());
        }
        _ => {
            let ac = a - lf(m, CENTER);
            let yc = yy - lf(m, CENTER + 4);
            let zc = zz - lf(m, CENTER + 8);
            let len = ((yc * yc + ac * ac) + zc * zc).sqrt();
            let (mut ac32, mut yc32, mut zc32) = (rf(ac), rf(yc), rf(zc));
            if !len.eq(zero) {
                ac32 = rf(ac / len);
                yc32 = rf(yc / len);
                zc32 = rf(zc / len);
            }
            m.set_f32(out, angle(zc32, ac32));
            if kind == 4 {
                m.set_f32(out + 4, flip(m, rf(yy), b1, w1).to_f32());
            } else {
                let r = if yc32.lt(F80::from_f32(-1.0)) {
                    F80::ONE
                } else if F80::ONE.lt(yc32) {
                    zero
                } else {
                    trig::acos(yc32.to_f64()) / pi
                };
                m.set_f32(out + 4, r.to_f32());
            }
        }
    }
}
