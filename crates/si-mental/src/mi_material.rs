// SPDX-License-Identifier: MIT
//! `mi_material.c`: the translator's materials for mental ray (package 63, finding 0036).
//!
//! Like [`crate::mi_poly`], a routine here writes through `SI_mrpipeFPRINTF`, which is not
//! ported: it is called through [`Outside`].

use crate::mi_calc::SDEBUG;
use crate::mi_patch::{dw, fprintf, g, ELEMDICT_FIRST, ELEMDICT_NEXT};
use si_mem::{Mem, Outside, Routine};
use si_x87::F80;

/// `SI_elemListCountValidElems (0x08133cc0)`.
pub const ELEMLIST_COUNT_VALID: Routine = Routine::new("SI_elemlistCountValidElems", 0x0813_3cc0);
/// `SI_elemdictFindRelatedElem (0x0813508c)`.
pub const ELEMDICT_FIND: Routine = Routine::new("SI_elemdictFindRelatedElem", 0x0813_508c);
/// `SI_cid.LIGHTS` and `SI_cid.MODELS`.
pub const CID_LIGHTS: u32 = 0x0878_1678;
pub const CID_MODELS: u32 = 0x0878_1680;
/// `have_selective_lights`, a 16-bit static.
pub const HAVE_SELECTIVE_LIGHTS: u32 = 0x0882_9ef0;

/// Whether the scene has a light that lights only some objects
/// [soft 0x0857f558 check_selective_lights].
///
/// A light counts when it is not hidden (+0x54 and the word at +0x48 are zero), has a name
/// (+0x20) and its record (+0x4) has a non-zero +0x15c.
pub fn check_selective_lights(m: &mut Mem, o: &mut dyn Outside, dict: u32) -> i32 {
    if dict == 0 {
        return 0;
    }
    let list = m.u32(CID_LIGHTS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    if o.call(m, ELEMLIST_COUNT_VALID, &[list]).eax == 0 {
        return 0;
    }
    let n = m.i32(list + 4);
    let mut e = m.u32(list + 8);
    for _ in 0..n.max(0) {
        if m.u32(e + 0x54) == 0 && m.u16(e + 0x48) == 0 && m.u32(e + 0x20) != 0 {
            let rec = m.u32(e + 4);
            if m.u32(rec + 0x15c) != 0 {
                return 1;
            }
        }
        e = e.wrapping_add(0x84);
    }
    0
}

/// Notes whether the scene has selective lights [soft 0x0857cdc0 init_materials].
pub fn init_materials(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    let r = check_selective_lights(m, o, dict) as u16;
    m.set_u16(HAVE_SELECTIVE_LIGHTS, r);
    let s = if r != 0 { 0x086e_ef00 } else { 0x086e_ef01 };
    o.call(m, SDEBUG, &[0x086e_ef20, s]);
}

/// The material's settings for the soft material of mental ray (the output record) [soft 0x0857cdfc SI_init_MRSoftMaterial].
///
/// `out`: +4 the shading model (material word +0x1e: 1 gives 0, 3 gives 1, 5 gives 3, others
/// 2), +8 the float `1 + (f - 1)` of material +0x6dc, +0xc and +0x10 the blur flag and amount
/// (material +0x758, +0x760), +0x14 whether a 2D texture of kind 8 was found on the material or
/// on its model, +0x16 cleared. Returns the found flag (`edi`) only as the store.
pub fn si_init_mr_soft_material(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, out: u32) {
    // the original's frame: 0x1c bytes of locals, three saved registers, the saved frame pointer
    let l = m.local(0x2c);
    let ebp = l + 0x28;
    si_init_mr_soft_material_body(m, o, dict, elem, out, ebp);
    m.release(0x2c);
}

fn si_init_mr_soft_material_body(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, out: u32, ebp: u32) {
    let mut found: u16 = 0;
    if dict == 0 || elem == 0 {
        return;
    }
    let mat = m.u32(elem + 4);
    if mat == 0 {
        return;
    }
    let model = match m.u16(mat + 0x1e) {
        1 => 0u16,
        3 => 1,
        5 => 3,
        _ => 2,
    };
    m.set_u16(out + 4, model);
    let one = F80::from_f32(1.0);
    let f = F80::from_f32(f32::from_bits(m.u32(mat + 0x6dc)));
    m.set_u32(out + 8, (one + (f - one)).to_f32().to_bits());
    if m.u16(mat + 0x758) != 0 {
        m.set_u16(out + 0xc, 1);
        let v = F80::from_f32(f32::from_bits(m.u32(mat + 0x760)));
        m.set_u32(out + 0x10, v.to_f32().to_bits());
    } else {
        m.set_u32(out + 0x10, 0);
        m.set_u16(out + 0xc, 0);
    }
    m.set_u16(out + 0x14, 0);
    let (l8, l4, lc) = (ebp - 8, ebp - 4, ebp - 0xc);
    let cid = m.u32(g::CID_TEXTURES2D);
    let mut t = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x191, cid, l8, l4]).eax;
    while t != 0 && found == 0 {
        if m.u16(t + 0x2a) == 8 {
            found = 1;
        } else {
            let cid = m.u32(g::CID_TEXTURES2D);
            t = o.call(m, ELEMDICT_NEXT, &[dict, elem, 1, 3, 0x191, cid, l8, l4]).eax;
        }
    }
    if found == 0 {
        let cm = m.u32(CID_MODELS);
        let owner = o.call(m, ELEMDICT_FIND, &[dict, elem, 0, 0, 0x12c, cm, lc, lc]).eax;
        if owner != 0 {
            let cid = m.u32(g::CID_TEXTURES2D);
            let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, 0x190, cid, l8, l4]).eax;
            while t != 0 && found == 0 {
                if m.u16(t + 0x2a) == 8 {
                    found = 1;
                } else {
                    let cid = m.u32(g::CID_TEXTURES2D);
                    t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, 0x190, cid, l8, l4]).eax;
                }
            }
        }
    }
    if found != 0 {
        m.set_u16(out + 0x14, 1);
    }
    m.set_u16(out + 0x16, 0);
}

/// The fog block of the soft material [soft 0x08580430 mi_print_soft_fog].
///
/// Reads nine floats of the writer's state (0x08790ce4 to 0x08790d10): where start (+0xce8),
/// stop (+0xcf0) or rate (+0xcf4) is zero, the float it falls back to (+0xce4, +0xcec, +0xd10) is
/// printed instead; then the three transmit components.
pub fn mi_print_soft_fog(m: &mut Mem, o: &mut dyn Outside) {
    let f = |m: &Mem, a: u32| F80::from_f32(f32::from_bits(m.u32(a)));
    let pick = |m: &Mem, a: u32, b: u32| {
        let v = f(m, a);
        if v.eq(F80::from_f32(0.0)) { f(m, b) } else { v }
    };
    for (a, b, fmt) in [(0x0879_0ce8u32, 0x0879_0ce4u32, 0x086e_f94cu32), (0x0879_0cf0, 0x0879_0cec, 0x086e_f95e), (0x0879_0cf4, 0x0879_0d10, 0x086e_f970)] {
        let v = dw(pick(m, a, b).to_f64());
        fprintf(m, o, &[fmt, v[0], v[1]]);
    }
    // the last call pushes the double at 0xd00 first, so it is the last argument
    let x = dw(f(m, 0x0879_0cf8).to_f64());
    let y = dw(f(m, 0x0879_0cfc).to_f64());
    let z = dw(f(m, 0x0879_0d00).to_f64());
    fprintf(m, o, &[0x086e_f982, x[0], x[1], y[0], y[1], z[0], z[1]]);
}

/// `need_3D_texture (0x08591288)` and `SI_emptyString (0x082cb774)`.
pub const NEED_3D_TEXTURE: Routine = Routine::new("need_3D_texture", 0x0859_1288);
pub const EMPTY_STRING: Routine = Routine::new("SI_emptyString", 0x082c_b774);
/// `mi2output`, a 16-bit flag (mental ray 2 output).
pub const MI2OUTPUT: u32 = 0x086e_f9c0;
/// A 16-bit flag read by the 3D map writer for the third bump value.
const FLAG_879116A: u32 = 0x0879_116a;

fn d32(m: &Mem, a: u32) -> [u32; 2] {
    dw(F80::from_f32(f32::from_bits(m.u32(a))).to_f64())
}

/// One 3D texture of a material, as a mental ray texture/shader block
/// [soft 0x0857f030 print_3d_map].
///
/// `mode` is the caller's kind (0 to 3), `tex` the texture record, `fp` and `name`/`a`/`b`
/// the pipe handle, the optional name and two format arguments. Returns 0 when the texture is
/// not needed (mental ray 1 output and `need_3D_texture` says no), else 1.
pub fn print_3d_map(m: &mut Mem, o: &mut dyn Outside, mode: u32, tex: u32, fp: u32, name: u32, a: u32, b: u32) -> i32 {
    // the original's frame: 0x2c bytes of locals, three saved registers, the saved frame pointer
    let _l = m.local(0x3c);
    let r = print_3d_map_body(m, o, mode, tex, fp, name, a, b);
    m.release(0x3c);
    r
}

fn print_3d_map_body(m: &mut Mem, o: &mut dyn Outside, mode: u32, tex: u32, fp: u32, name: u32, a: u32, b: u32) -> i32 {
    if m.u16(MI2OUTPUT) == 0 && o.call(m, NEED_3D_TEXTURE, &[tex]).eax == 0 {
        return 0;
    }
    let empty = o.call(m, EMPTY_STRING, &[name]).eax as u16;
    let s = if empty != 0 { 0x086e_f682 } else { name };
    fprintf(m, o, &[0x086e_f683, fp, s, a, b]);
    let k = match m.u16(tex + 0x24) {
        1 => 1u32,
        2 => 2,
        _ => 0,
    };
    fprintf(m, o, &[0x086e_f69f, k]);
    let k = if m.u16(tex + 0x2c) == 2 { 2 } else { 1 };
    fprintf(m, o, &[0x086e_f6ae, k]);
    let v = d32(m, tex + 0x28);
    fprintf(m, o, &[0x086e_f6bd, v[0], v[1]]);
    fprintf(m, o, &[0x086e_f6cb]);
    if m.u16(tex + 0xf4) != 0 && mode == 0 {
        let v = d32(m, tex + 0x108);
        fprintf(m, o, &[0x086e_f6dc, v[0], v[1]]);
    }
    if m.u16(tex + 0xf6) != 0 {
        let v = d32(m, tex + 0x10c);
        fprintf(m, o, &[0x086e_f6ed, v[0], v[1]]);
    }
    if m.u16(tex + 0xf8) != 0 && mode == 0 {
        let v = d32(m, tex + 0x110);
        fprintf(m, o, &[0x086e_f6fe, v[0], v[1]]);
    }
    if m.u16(tex + 0xfa) != 0 && mode == 0 {
        let v = if m.u16(FLAG_879116A) != 0 { d32(m, tex + 0x114) } else { dw(0.0) };
        fprintf(m, o, &[0x086e_f70f, v[0], v[1]]);
    }
    if m.u16(tex + 0xfc) != 0 {
        let v = d32(m, tex + 0x118);
        fprintf(m, o, &[0x086e_f720, v[0], v[1]]);
    }
    let mut skip = false;
    if m.u16(tex + 0x102) != 0 {
        if mode == 3 && m.u16(tex + 0x150) != 0 {
            let v = d32(m, tex + 0x120);
            fprintf(m, o, &[0x086e_f730, v[0], v[1]]);
        }
        if mode != 0 {
            skip = true;
        } else if m.u16(tex + 0x150) == 0 {
            let v = F80::from_f32(f32::from_bits(m.u32(tex + 0x120))) * -2.0f64;
            let v = dw(v.to_f64());
            fprintf(m, o, &[0x086e_f742, v[0], v[1]]);
        }
    }
    if !skip && mode == 0 {
        let c = F80::from_f64(0.05);
        let u = dw((c * f32::from_bits(m.u32(tex + 0x90))).to_f64());
        fprintf(m, o, &[0x086e_f751, u[0], u[1]]);
        let v = dw((c * f32::from_bits(m.u32(tex + 0x94))).to_f64());
        fprintf(m, o, &[0x086e_f761, v[0], v[1]]);
    }
    fprintf(m, o, &[0x086e_f771]);
    1
}

/// One of the two light lists of a material [soft 0x0857f2c0 print_light_list].
///
/// Prints `"lights"` (lights that are not diffuse-only: +0x180 of the light record is zero) and
/// then `"difflights"` (the others), each as `[ name, name ]` with `LIG_` in front for mental ray
/// 2 output. A light counts when it is not hidden, has a name and, if it carries a selection
/// (+0x15c), the selection array (+0x168) at `index` equals +0x160. `first` says whether a
/// comma is needed before the first list. Returns nothing.
pub fn print_light_list(m: &mut Mem, o: &mut dyn Outside, dict: u32, index: u32, first: u32) {
    // the original's frame: 0x2c bytes of locals, three saved registers, the saved frame pointer
    let _l = m.local(0x3c);
    print_light_list_body(m, o, dict, index, first as u16);
    m.release(0x3c);
}

fn print_light_list_body(m: &mut Mem, o: &mut dyn Outside, dict: u32, index: u32, first: u16) {
    if dict == 0 {
        return;
    }
    let list = m.u32(CID_LIGHTS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let mut sep = if first != 0 { 0x086e_f773u32 } else { 0x086e_f781 };
    if o.call(m, ELEMLIST_COUNT_VALID, &[list]).eax == 0 {
        let pre = if m.u16(MI2OUTPUT) != 0 { 0x086e_f78d } else { 0x086e_f792 };
        fprintf(m, o, &[0x086e_f793, sep, pre]);
        return;
    }
    // (diffuse-only lights, texts of the first list; then the second list)
    for (want_diff, texts) in [
        (false, [0x086e_f7ac_u32, 0x086e_f7a3, 0x086e_f7a2, 0x086e_f7a5, 0x086e_f7a6, 0x086e_f7ab, 0x086e_f7bc, 0x086e_f7b9]),
        (true, [0x086e_f7ea, 0x086e_f7e1, 0x086e_f7e0, 0x086e_f7e3, 0x086e_f7e4, 0x086e_f7e9, 0x086e_f7fa, 0x086e_f7f7]),
    ] {
        let [fmt, dash_on, dash_off, no_name, lig, plain, close, comma] = texts;
        if want_diff {
            sep = if first != 0 { 0x086e_f7be } else { 0x086e_f7d0 };
        }
        let mut printed = 0u32;
        let mut i = 0i32;
        let mut off = 0u32;
        while i < m.i32(list + 4) {
            let e = off.wrapping_add(m.u32(list + 8));
            let rec = m.u32(e + 4);
            let ok = m.u32(e + 0x54) == 0 && m.u16(e + 0x48) == 0 && m.u32(e + 0x20) != 0 && (m.u16(rec + 0x180) != 0) == want_diff;
            if ok {
                let owner = m.u32(e);
                let name = if owner != 0 { m.u32(owner + 0x10) } else { 0 };
                let selected = m.u32(rec + 0x15c) == 0 || {
                    let arr = m.u32(rec + 0x168);
                    m.u32(arr.wrapping_add(index.wrapping_mul(4))) == m.u32(rec + 0x160)
                };
                if selected {
                    let pre = if m.u16(MI2OUTPUT) != 0 { lig } else { plain };
                    let nm = if name != 0 { name } else { no_name };
                    let dash = if name != 0 { dash_on } else { dash_off };
                    fprintf(m, o, &[fmt, sep, pre, nm, dash, rec]);
                    sep = comma;
                    printed += 1;
                }
            }
            i += 1;
            off = off.wrapping_add(0x84);
        }
        if printed != 0 {
            fprintf(m, o, &[close]);
        }
    }
}

/// `need_texture (0x08591244)`.
pub const NEED_TEXTURE: Routine = Routine::new("need_texture", 0x0859_1244);

fn st_f32(m: &Mem, a: u32) -> F80 {
    F80::from_f32(f32::from_bits(m.u32(a)))
}

fn d32f(m: &Mem, a: u32) -> [u32; 2] {
    dw(st_f32(m, a).to_f64())
}

/// One 2D texture of a material as a mental ray texture block
/// [soft 0x0857ebc8 print_2d_map].
///
/// `mode`: 0 colour, 1 and 3 other channels, 2 the bump/displacement kinds (texture kind 7 or 8
/// goes with mode 2 and only with it; otherwise nothing is written and 0 returned). Returns 1
/// after writing the block. `a`, `b`, `c` are the format arguments of the opening lines.
pub fn print_2d_map(m: &mut Mem, o: &mut dyn Outside, mode: u32, tex: u32, a: u32, b: u32, c: u32) -> i32 {
    // the original's frame: 0x30 bytes of locals, two saved registers, the saved frame pointer
    let _l = m.local(0x3c);
    let r = print_2d_map_body(m, o, mode, tex, a, b, c);
    m.release(0x3c);
    r
}

fn print_2d_map_body(m: &mut Mem, o: &mut dyn Outside, mode: u32, tex: u32, a: u32, b: u32, c: u32) -> i32 {
    let kind = m.u16(tex + 0x2a);
    let bumpy = kind.wrapping_sub(7) <= 1;
    if (mode == 2) != bumpy {
        return 0;
    }
    if m.u16(MI2OUTPUT) == 0 && o.call(m, NEED_TEXTURE, &[tex]).eax == 0 {
        return 0;
    }
    fprintf(m, o, &[0x086e_f53a, a, b]);
    if mode != 2 {
        fprintf(m, o, &[0x086e_f54a, c]);
    }
    if mode == 0 {
        let v = match kind {
            1 => 0u32,
            2 => 1,
            3 => 2,
            4 => 3,
            _ => 4,
        };
        fprintf(m, o, &[0x086e_f55a, v]);
    }
    let w10a = m.u16(tex + 0x10a);
    if m.u16(MI2OUTPUT) == 0 && o.call(m, crate::mi_poly::GETENV, &[0x086e_f56a]).eax != 0 {
        if mode != 3 {
            let v = match w10a {
                1 => 1u32,
                2 => 2,
                _ => 0,
            };
            fprintf(m, o, &[0x086e_f579, v]);
        }
    } else if mode != 3 {
        let v = match w10a {
            1 => 1u32,
            2 => 2,
            4 => 3,
            _ => 0,
        };
        fprintf(m, o, &[0x086e_f588, v]);
    }
    if mode != 2 {
        let k = if m.u16(tex + 0x110) == 2 { 2 } else { 1 };
        fprintf(m, o, &[0x086e_f597, k]);
    }
    if mode != 3 {
        let v = if mode == 2 { d32f(m, tex + 0x144) } else { d32f(m, tex + 0x10c) };
        fprintf(m, o, &[0x086e_f5a6, v[0], v[1]]);
    }
    let one = |m: &mut Mem, o: &mut dyn Outside, flag: u32, val: u32, fmt: u32, cond: bool| {
        if m.u16(tex + flag) != 0 && cond {
            let v = d32f(m, tex + val);
            fprintf(m, o, &[fmt, v[0], v[1]]);
        }
    };
    one(m, o, 0x112, 0x128, 0x086e_f5b6, mode == 0);
    one(m, o, 0x114, 0x12c, 0x086e_f5c7, (mode as i32) <= 1);
    one(m, o, 0x116, 0x130, 0x086e_f5d8, mode == 0);
    if m.u16(tex + 0x118) != 0 && mode == 0 {
        let v = if m.u16(FLAG_879116A) != 0 { d32f(m, tex + 0x134) } else { dw(0.0) };
        fprintf(m, o, &[0x086e_f5ea, v[0], v[1]]);
    }
    one(m, o, 0x11a, 0x138, 0x086e_f5fb, (mode as i32) <= 1);
    let mut uv = true;
    if m.u16(tex + 0x120) != 0 {
        if mode == 3 && m.u16(tex + 0x19c) != 0 {
            let v = d32f(m, tex + 0x140);
            fprintf(m, o, &[0x086e_f60b, v[0], v[1]]);
        }
        if mode != 0 {
            uv = false;
        } else if m.u16(tex + 0x19c) == 0 {
            let v = dw((-st_f32(m, tex + 0x140)).to_f64());
            fprintf(m, o, &[0x086e_f61d, v[0], v[1]]);
        }
    }
    if uv && mode == 0 {
        let sh = |m: &Mem, hi: u32, lo: u32| F80::from_i32((m.u16(tex + hi) as i16 as i32) - (m.u16(tex + lo) as i16 as i32));
        let one80 = F80::from_f32(1.0);
        let x = sh(m, 0x32, 0x30) + one80;
        let y = sh(m, 0x36, 0x34) + one80;
        let mut a1 = st_f32(m, tex + 0x58);
        let w = m.u16(tex + 0xf8);
        if w != 0 {
            a1 = a1 / F80::from_i32(w as i16 as i32);
        }
        let mut u = F80::from_f64(a1.to_f64());
        if !x.eq(F80::from_f32(0.0)) {
            u = F80::from_f64((u / x).to_f64());
        }
        let mut b1 = st_f32(m, tex + 0x5c);
        let w = m.u16(tex + 0xfa);
        if w != 0 {
            b1 = b1 / F80::from_i32(w as i16 as i32);
        }
        let mut v = F80::from_f64(b1.to_f64());
        if !y.eq(F80::from_f32(0.0)) {
            v = F80::from_f64((v / y).to_f64());
        }
        let (ud, vd) = (dw(u.to_f64()), dw(v.to_f64()));
        fprintf(m, o, &[0x086e_f62c, ud[0], ud[1]]);
        fprintf(m, o, &[0x086e_f63c, vd[0], vd[1]]);
        if m.u16(tex + 0x1cc) != 0 {
            let r = dw((u - one80).to_f64());
            fprintf(m, o, &[0x086e_f64c, r[0], r[1]]);
        }
        if m.u16(tex + 0x1ce) != 0 {
            let r = dw((v - one80).to_f64());
            fprintf(m, o, &[0x086e_f65c, r[0], r[1]]);
        }
    }
    if m.u16(tex + 0x18e) != 0 && mode == 0 {
        fprintf(m, o, &[0x086e_f66c]);
    }
    fprintf(m, o, &[0x086e_f680]);
    1
}

/// `strdup`, `free` (imports), `SI_strAppend (0x083cf280)`, `SI_elemdictGetElemId (0x08132b94)`
/// and `print_material (0x0857cfd0)`.
pub const STRDUP: Routine = Routine::new("strdup", 0x080c_39c8);
pub const FREE: Routine = Routine::new("free", 0x080c_4c98);
pub const STR_APPEND: Routine = Routine::new("SI_strAppend", 0x083c_f280);
pub const ELEMDICT_GET_ELEM_ID: Routine = Routine::new("SI_elemdictGetElemId", 0x0813_2b94);
pub const PRINT_MATERIAL: Routine = Routine::new("print_material", 0x0857_cfd0);
/// `incremental` (32 bits), `soft_prefix` (a pointer): the writer's state.
pub const INCREMENTAL: u32 = 0x086e_f9bc;
pub const SOFT_PREFIX: u32 = 0x086e_f9b8;
/// The static default colour (three floats) of the default material.
pub const DEFAULT_COLOUR: u32 = 0x0879_0ddc;

/// The default material, written when the scene has none for an object
/// [soft 0x0857eab0 print_default_material].
pub fn print_default_material(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) {
    // the original's frame: 0x1c bytes of locals, three saved registers, the saved frame pointer
    let _l = m.local(0x2c);
    if dict != 0 && elem != 0 && m.u32(elem + 4) != 0 {
        let prefix = m.u32(SOFT_PREFIX);
        let fmt = if m.u32(INCREMENTAL) != 0 { 0x086e_f460 } else { 0x086e_f4a0 };
        fprintf(m, o, &[fmt, prefix]);
        fprintf(m, o, &[0x086e_f4c4]);
        fprintf(m, o, &[0x086e_f4d2]);
        let one = F80::from_f32(1.0);
        let c = |m: &Mem, k: u32| st_f32(m, DEFAULT_COLOUR + k);
        let (a, b, d) = (dw((one - c(m, 0)).to_f64()), dw((one - c(m, 4)).to_f64()), dw((one - c(m, 8)).to_f64()));
        fprintf(m, o, &[0x086e_f4e3, a[0], a[1], b[0], b[1], d[0], d[1]]);
        let (a, b, d) = (d32f(m, DEFAULT_COLOUR), d32f(m, DEFAULT_COLOUR + 4), d32f(m, DEFAULT_COLOUR + 8));
        fprintf(m, o, &[0x086e_f4f9, a[0], a[1], b[0], b[1], d[0], d[1]]);
        fprintf(m, o, &[0x086e_f50f]);
        let cm = m.u32(CID_MODELS);
        let id = o.call(m, ELEMDICT_GET_ELEM_ID, &[dict, cm, elem]).eax;
        print_light_list(m, o, dict, id, 1);
        fprintf(m, o, &[0x086e_f527]);
    }
    m.release(0x2c);
}

/// A material, or its `_selective` twin when the light selection asks for one
/// [soft 0x0857faf4 print_extension].
///
/// `sel` is the name of the material to extend (null for the plain material), `twin` whether a
/// `_selective` copy of that name is wanted.
pub fn print_extension(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, owner: u32, sel: u32, extra: u32, twin: u16) {
    // the original's frame: 0xc bytes of locals, three saved registers, the saved frame pointer
    let l = m.local(0x1c);
    let s_slot = l + 0x14;
    let name = |m: &Mem| {
        if owner != 0 {
            let p = m.u32(owner);
            if p != 0 {
                return m.u32(p + 0x10);
            }
        }
        0
    };
    if sel == 0 {
        let n = name(m);
        o.call(m, PRINT_MATERIAL, &[dict, owner, elem, n, 0, 0, extra]);
    } else if twin != 0 {
        let s = o.call(m, STRDUP, &[sel]).eax;
        m.set_u32(s_slot, s);
        o.call(m, STR_APPEND, &[s_slot, 0x086e_f7fc]);
        let n = name(m);
        let s = m.u32(s_slot);
        o.call(m, PRINT_MATERIAL, &[dict, owner, elem, n, 0, s, extra]);
        let s = m.u32(s_slot);
        if s != 0 {
            o.call(m, FREE, &[s]);
            m.set_u32(s_slot, 0);
        }
    } else {
        let n = name(m);
        o.call(m, PRINT_MATERIAL, &[dict, owner, elem, n, 0, sel, extra]);
    }
    m.release(0x1c);
}

/// Marks the models that a selective light lights [soft 0x0857fbcc flagModelWithSelectiveLight].
///
/// For every light that is shown, named and carries a selection (+0x15c), the models related to
/// it (relation 0x898) of kinds 10, 1, 2, 4 and 7 get byte +0x5b set.
pub fn flag_model_with_selective_light(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    // the original's frame: 0x1c bytes of locals, three saved registers, the saved frame pointer
    let l = m.local(0x2c);
    let ebp = l + 0x28;
    let (l4, l8) = (ebp - 4, ebp - 8);
    let list = m.u32(CID_LIGHTS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    if o.call(m, ELEMLIST_COUNT_VALID, &[list]).eax != 0 {
        let mut i = 0i32;
        while i < m.i32(list + 4) {
            let e = m.u32(list + 8).wrapping_add((i as u32).wrapping_mul(0x84));
            i += 1;
            if e != 0 && m.u16(e + 0x48) == 0 && m.u32(e + 0x20) != 0 && m.u32(m.u32(e + 4) + 0x15c) != 0 {
                let cm = m.u32(CID_MODELS);
                o.call(m, ELEMDICT_FIRST, &[dict, e, 1, 3, 0x898, cm, l8, l4]);
                while m.u32(l8) != 0 {
                    let mo = m.u32(l8);
                    let k = m.u32(m.u32(mo + 4) + 0x30);
                    if matches!(k, 10 | 1 | 2 | 4 | 7) {
                        m.set_u8(mo + 0x5b, 1);
                    }
                    let cm = m.u32(CID_MODELS);
                    o.call(m, ELEMDICT_NEXT, &[dict, e, 1, 3, 0x898, cm, l8, l4]);
                }
            }
        }
    }
    m.release(0x2c);
}

/// `si_checkForTxtGlb (0x08594fb8)`, `SI_elemdictGetElem (0x08132b50)`,
/// `SI_elemlistClearLowByteWorkFlag (0x08132c70)`, `SI_elemdictSetDirty (0x08132be8)` and
/// `SI_elemdictFindNextRelatedElem (0x08135154)`.
pub const CHECK_FOR_TXT_GLB: Routine = Routine::new("si_checkForTxtGlb", 0x0859_4fb8);
pub const ELEMDICT_GET_ELEM: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
pub const CLEAR_LOW_BYTE_WORK_FLAG: Routine = Routine::new("SI_elemlistClearLowByteWorkFlag", 0x0813_2c70);
pub const ELEMDICT_SET_DIRTY: Routine = Routine::new("SI_elemdictSetDirty", 0x0813_2be8);
pub const ELEMDICT_FIND_NEXT: Routine = Routine::new("SI_elemdictFindNextRelatedElem", 0x0813_5154);
pub const CID_MATERIALS: u32 = 0x0878_167c;

/// A material together with the model that uses it, once per (material, model) pair
/// [soft 0x0857f92c print_mat_with_extension].
///
/// `mat` is the material element, `owner` the element the material hangs on (the extension is
/// named after it) and `done` points at a 16-bit flag that says the plain material of this
/// element has been written. A model that carries the `_selective` flag (byte +0x5b) or that
/// has a related model (+0x5c) is written through [`print_extension`] and marked done (+0x40).
pub fn print_mat_with_extension(m: &mut Mem, o: &mut dyn Outside, dict: u32, mat: u32, owner: u32, done: u32) {
    // the original's frame: 0xc bytes of locals, three saved registers, the saved frame pointer
    let _l = m.local(0x1c);
    let rec = m.u32(mat + 4);
    if rec != 0 {
        let w = m.u32(mat + 0x5c);
        if m.u8(mat + 0x5b) == 1 {
            let x = if w != 0xffff_ffff {
                let cm = m.u32(CID_MODELS);
                let b = o.call(m, ELEMDICT_GET_ELEM, &[dict, cm, w]).eax;
                o.call(m, CHECK_FOR_TXT_GLB, &[dict, b]);
                let r = m.u32(b + 4);
                m.u32(r + 4)
            } else {
                m.u32(rec + 4)
            };
            print_extension(m, o, dict, owner, mat, m.u32(rec + 4), x, 1);
        } else if w == 0xffff_ffff {
            let r = o.call(m, CHECK_FOR_TXT_GLB, &[dict, mat]).eax as u16;
            if r != 0 {
                let s = m.u32(rec + 4);
                print_extension(m, o, dict, owner, mat, s, s, 0);
            } else if m.u16(done) == 0 {
                let s = m.u32(rec + 4);
                print_extension(m, o, dict, owner, mat, 0, s, 0);
                m.set_u16(done, 1);
            }
            m.set_u16(mat + 0x40, 1);
        } else {
            let cm = m.u32(CID_MODELS);
            let b = o.call(m, ELEMDICT_GET_ELEM, &[dict, cm, w]).eax;
            let brec = m.u32(b + 4);
            if m.u8(b + 0x5b) == 1 {
                let r = o.call(m, CHECK_FOR_TXT_GLB, &[dict, b]).eax as u16;
                let x = m.u32(brec + 4);
                if r != 0 {
                    if m.u16(b + 0x40) != 0 {
                        m.release(0x1c);
                        return;
                    }
                    print_extension(m, o, dict, owner, mat, x, x, 0);
                } else {
                    print_extension(m, o, dict, owner, mat, 0, x, 0);
                }
                m.set_u16(b + 0x40, 1);
            } else {
                let r = o.call(m, CHECK_FOR_TXT_GLB, &[dict, b]).eax as u16;
                if m.u16(b + 0x40) == 0 {
                    let x = m.u32(brec + 4);
                    if r != 0 {
                        print_extension(m, o, dict, owner, mat, x, x, 0);
                    } else {
                        print_extension(m, o, dict, owner, mat, 0, x, 0);
                    }
                    m.set_u16(b + 0x40, 1);
                }
            }
        }
    }
    m.release(0x1c);
}

fn is_light_kind(k: u32) -> bool {
    matches!(k, 10 | 1 | 2 | 4 | 7)
}

/// Every material of the scene, through every model that uses it
/// [soft 0x0857f5e0 print_all_material].
///
/// First every model that has a material of kind 10, 1, 2, 4 or 7 flags that material (byte
/// +0x5b); then each flagged material is written once per model that uses it.
pub fn print_all_material(m: &mut Mem, o: &mut dyn Outside, dict: u32, a: u32) {
    // the original's frame: 0x2c bytes of locals, three saved registers, the saved frame pointer
    let l = m.local(0x3c);
    let ebp = l + 0x38;
    let (l4, l8, lc, l10, l12) = (ebp - 4, ebp - 8, ebp - 0xc, ebp - 0x10, ebp - 0x12);
    let a = a as u16;
    m.set_u16(ebp - 0x14, a);
    let cm = m.u32(CID_MODELS);
    o.call(m, CLEAR_LOW_BYTE_WORK_FLAG, &[dict, cm]);
    let cmat = m.u32(CID_MATERIALS);
    o.call(m, CLEAR_LOW_BYTE_WORK_FLAG, &[dict, cmat]);
    let models = m.u32(CID_MODELS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let mut i = 0i32;
    while i < m.i32(models + 4) {
        let e = m.u32(models + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        i += 1;
        if e == 0 || m.u16(e + 0x48) != 0 {
            continue;
        }
        o.call(m, ELEMDICT_SET_DIRTY, &[e, 0]);
        let rec = m.u32(e + 4);
        if m.u16(MI2OUTPUT) == 0 && m.u32(e + 0x20) == 0 {
            continue;
        }
        if a != 1 {
            let k = m.u32(e + 0x24);
            if !matches!(k, 1 | 3 | 2) {
                continue;
            }
        }
        let cmat = m.u32(CID_MATERIALS);
        o.call(m, ELEMDICT_FIRST, &[dict, e, 1, 3, 0x12c, cmat, l8, l4]);
        while m.u32(l8) != 0 {
            if is_light_kind(m.u32(rec + 0x30)) {
                let t = m.u32(l8);
                m.set_u8(t + 0x5b, 1);
            }
            let cmat = m.u32(CID_MATERIALS);
            o.call(m, ELEMDICT_NEXT, &[dict, e, 1, 3, 0x12c, cmat, l8, l4]);
        }
    }
    let mats = m.u32(CID_MATERIALS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    if m.u16(HAVE_SELECTIVE_LIGHTS) != 0 {
        flag_model_with_selective_light(m, o, dict);
    }
    let mut i = 0i32;
    while i < m.i32(mats + 4) {
        let e = m.u32(mats + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        i += 1;
        m.set_u16(l12, 0);
        if e == 0 || m.u16(e + 0x48) != 0 || m.u32(e + 4) == 0 || m.u8(e + 0x5b) == 0 {
            continue;
        }
        let ok = |m: &Mem, mo: u32| {
            if m.u16(MI2OUTPUT) == 0 && m.u32(mo + 0x20) == 0 {
                return false;
            }
            if a != 1 && !matches!(m.u32(mo + 0x24), 1 | 3 | 2) {
                return false;
            }
            if m.u32(INCREMENTAL) != 0 && m.u8(e + 0x58) & 4 == 0 {
                return false;
            }
            is_light_kind(m.u32(m.u32(mo + 4) + 0x30))
        };
        let cm = m.u32(CID_MODELS);
        let mut mo = o.call(m, ELEMDICT_FIND, &[dict, e, 0, 0, 0x12c, cm, l10, lc]).eax;
        if mo != 0 && ok(m, mo) {
            print_mat_with_extension(m, o, dict, mo, e, l12);
        }
        loop {
            let cm = m.u32(CID_MODELS);
            mo = o.call(m, ELEMDICT_FIND_NEXT, &[dict, e, 0, 0, 0x12c, cm, l10, lc]).eax;
            if mo == 0 {
                break;
            }
            if ok(m, mo) {
                print_mat_with_extension(m, o, dict, mo, e, l12);
            }
        }
    }
    m.release(0x3c);
}

/// `SI_textureInitialize (0x082b2508)`.
pub const SI_TEXTURE_INITIALIZE: Routine = Routine::new("SI_textureInitialize", 0x082b_2508);
/// `texsurfaceno`, an array of 32-bit surface numbers indexed by the texture's place in the material.
pub const TEXSURFACENO: u32 = 0x0882_9d60;
const FLAG_8791166: u32 = 0x0879_1166;
const FLAG_8791168: u32 = 0x0879_1168;
const FLAG_879116C: u32 = 0x0879_116c;
const FLAG_8790EA8: u32 = 0x0879_0ea8;
const TEXT_END_BLOCK: u32 = 0x086e_f8fb; // "\t}, {\n"

/// `sprintf` with the two shapes `mi_print_soft_material` uses (`%s` and `%d` only), writing the
/// string and its terminator at `buf`. A null `%s` argument is written as `(null)`, as the C
/// library does.
fn sprintf_s(m: &mut Mem, buf: u32, fmt: u32, args: &[u32]) {
    let f = m.cstr(fmt);
    let mut out: Vec<u8> = Vec::new();
    let mut it = args.iter();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' && i + 1 < f.len() {
            let a = *it.next().unwrap_or(&0);
            match f[i + 1] {
                b's' => {
                    if a == 0 {
                        out.extend_from_slice(b"(null)");
                    } else {
                        out.extend(m.cstr(a));
                    }
                }
                _ => out.extend_from_slice((a as i32).to_string().as_bytes()),
            }
            i += 2;
        } else {
            out.push(f[i]);
            i += 1;
        }
    }
    out.push(0);
    m.set_bytes(buf, &out);
}

/// The soft material of one (material, model) pair as a mental ray material block body
/// [soft 0x0857fcd8 mi_print_soft_material].
///
/// Writes the mode, refraction, motion blur, shading values and the list of textures (the model's
/// own, the 3D one and those related to the material and to the element), then the light list
/// when `last` is zero, else the text `last` points at. `tex20` is the texture element whose kind 8 turns on `notrace`.
pub fn mi_print_soft_material(m: &mut Mem, o: &mut dyn Outside, args: &[u32; 13]) {
    // the original's frame: 0x40c bytes of locals, three saved registers, the saved frame pointer
    let l = m.local(0x41c);
    mi_print_soft_material_body(m, o, args, l + 0x418);
    m.release(0x41c);
}

fn mi_print_soft_material_body(m: &mut Mem, o: &mut dyn Outside, a: &[u32; 13], ebp: u32) {
    let [dict, elem, mat, name1, name2, arg1c, tex20, tex24, tex28, n2c, n30, n34, last] = *a;
    let rec_e = m.u32(elem + 4);
    let rec = m.u32(mat + 4);
    let (l_str, l_cnt, l_c1, l_c2, buf, trec) = (ebp - 0x3f4, ebp - 0x3f8, ebp - 0x3f0, ebp - 0x3ec, ebp - 0x200, ebp - 0x3e8);
    let f64w = |m: &Mem, at: u32| d32f(m, at);
    let mode = match m.u16(rec + 0x1e) {
        1 => 0,
        3 => 1,
        5 => 3,
        _ => 2,
    };
    fprintf(m, o, &[0x086e_f807, mode]);
    if m.u16(FLAG_879116C) != 0 {
        let v = if m.u16(FLAG_8790EA8) != 0 { F80::ONE + (st_f32(m, rec + 0x6dc) - F80::ONE) } else { F80::ONE };
        let d = dw(v.to_f64());
        fprintf(m, o, &[0x086e_f815, d[0], d[1]]);
    } else {
        fprintf(m, o, &[0x086e_f822]);
    }
    if m.u16(rec + 0x758) != 0 {
        fprintf(m, o, &[0x086e_f830]);
        let d = f64w(m, rec + 0x760);
        fprintf(m, o, &[0x086e_f83f, d[0], d[1]]);
    }
    if tex20 != 0 && m.u16(FLAG_8791166) != 0 && m.u16(FLAG_879116A) != 0 && m.u16(tex20 + 0x2a) == 8 {
        fprintf(m, o, &[0x086e_f852]);
    }
    let d = f64w(m, rec + 0x64);
    fprintf(m, o, &[0x086e_f862, d[0], d[1]]);
    let d = f64w(m, rec + 0x6b8);
    fprintf(m, o, &[0x086e_f871, d[0], d[1]]);
    let d = if m.u16(FLAG_879116A) != 0 { f64w(m, rec + 0x6a8) } else { dw(0.0) };
    fprintf(m, o, &[0x086e_f880, d[0], d[1]]);
    for (fmt, base) in [(0x086e_f890u32, rec + 0x34), (0x086e_f8a6, rec + 0x44), (0x086e_f8bc, rec + 0x54), (0x086e_f8d3, 0x0879_0ddc)] {
        let (x, y, z) = (f64w(m, base), f64w(m, base + 4), f64w(m, base + 8));
        fprintf(m, o, &[fmt, x[0], x[1], y[0], y[1], z[0], z[1]]);
    }
    m.set_u32(l_str, 0x086e_f8e8);
    if m.u32(rec + 0xdc8) != 0x0001_0001 {
        o.call(m, SI_TEXTURE_INITIALIZE, &[trec]);
        for (off, v) in [(0x10c, 0x3f80_0000u32), (0x128, 0), (0x12c, 0x3f80_0000), (0x130, 0), (0x134, 0), (0x138, 0x3f80_0000)] {
            m.set_u32(trec + off, v);
        }
        for (off, v) in [(0x2a, 1u16), (0x10a, 0), (0x110, 0), (0x112, 1), (0x114, 1), (0x116, 1), (0x118, 1), (0x11a, 1), (0x120, 0)] {
            m.set_u16(trec + off, v);
        }
        let edi = if m.u16(FLAG_8791166) != 0 { n2c.wrapping_add(n30) } else { 0 };
        let empty = |m: &mut Mem, o: &mut dyn Outside, s: u32| o.call(m, EMPTY_STRING, &[s]).eax as u16 != 0;
        let idx = m.u32(elem + 0x5c);
        let mut go = true;
        if idx != 0xffff_ffff {
            let cid = m.u32(CID_MODELS);
            let me = o.call(m, ELEMDICT_GET_ELEM, &[dict, cid, idx]).eax;
            if me == 0 {
                go = false;
            } else {
                let owner = m.u32(me);
                let nm0 = m.u32(owner + 0x10);
                let nm = if empty(m, o, nm0) { 0x086e_f902 } else { nm0 };
                let s2 = m.u32(m.u32(me + 4) + 4);
                sprintf_s(m, buf, 0x086e_f903, &[nm, s2, rec, edi]);
            }
        } else {
            let nm = if empty(m, o, name1) { 0x086e_f90f } else { name1 };
            sprintf_s(m, buf, 0x086e_f910, &[nm, name2, rec, edi]);
        }
        if go {
            let t = m.u32(l_str);
            if print_2d_map(m, o, 0, trec, t, buf, n34) != 0 {
                m.set_u32(l_str, TEXT_END_BLOCK);
            }
        }
    }
    if m.u16(FLAG_8791168) != 0 {
        let pick = if tex24 != 0 { Some((tex24, rec)) } else if tex28 != 0 { Some((tex28, 0x086e_f91c)) } else { None };
        if let Some((t, b)) = pick {
            let s = m.u32(l_str);
            if print_3d_map(m, o, 0, t, s, name1, name2, b) != 0 {
                m.set_u32(l_str, TEXT_END_BLOCK);
            }
        }
    }
    if m.u16(FLAG_8791166) != 0 {
        for pass in 0..2 {
            let (owner, rel) = if pass == 0 { (mat, 0x191) } else { (elem, 0x190) };
            let cid = m.u32(g::CID_TEXTURES2D);
            let mut t = o.call(m, ELEMDICT_FIRST, &[dict, owner, 1, 3, rel, cid, l_c1, l_c2]).eax;
            m.set_u32(l_cnt, 0);
            let mut n = 0u32;
            while t != 0 {
                if m.u16(MI2OUTPUT) != 0 {
                    let on = m.u32(m.u32(m.u32(l_c1)) + 0x10);
                    let on = if on != 0 { on } else if pass == 0 { 0x086e_f91d } else { 0x086e_f932 };
                    let s2 = m.u32(t + 0x18);
                    sprintf_s(m, buf, if pass == 0 { 0x086e_f91e } else { 0x086e_f933 }, &[on, s2]);
                } else if pass == 0 {
                    let s1 = if arg1c != 0 { arg1c } else { 0x086e_f925 };
                    sprintf_s(m, buf, 0x086e_f926, &[s1, 0x086e_f924, rec, n]);
                } else {
                    let nm = if o.call(m, EMPTY_STRING, &[name1]).eax as u16 != 0 { 0x086e_f93a } else { name1 };
                    sprintf_s(m, buf, 0x086e_f93b, &[nm, name2, 0x086e_f939, n]);
                }
                let ts = if pass == 0 { m.u32(TEXSURFACENO.wrapping_add(n2c.wrapping_add(n).wrapping_mul(4))) } else { m.u32(TEXSURFACENO.wrapping_add(n.wrapping_mul(4))) };
                let s = m.u32(l_str);
                if print_2d_map(m, o, 0, t, s, buf, ts) != 0 {
                    m.set_u32(l_str, TEXT_END_BLOCK);
                }
                let cid = m.u32(g::CID_TEXTURES2D);
                t = o.call(m, ELEMDICT_NEXT, &[dict, owner, 1, 3, rel, cid, l_c1, l_c2]).eax;
                n += 1;
                m.set_u32(l_cnt, n);
            }
        }
    }
    if m.u32(l_str) == TEXT_END_BLOCK {
        fprintf(m, o, &[0x086e_f947]);
    }
    if last == 0 {
        print_light_list(m, o, dict, m.u32(rec_e + 0x354), 1);
    } else {
        // a non-zero `last` is a text written in place of the light list
        fprintf(m, o, &[last]);
    }
}

// ---------------------------------------------------------------------------------------------
// print_material
// ---------------------------------------------------------------------------------------------

const P3D_MAP: Routine = Routine::new("print_3d_map", 0x0857_f030);
const P2D_MAP: Routine = Routine::new("print_2d_map", 0x0857_ebc8);
const MI_PRINT_SOFT_MATERIAL: Routine = Routine::new("mi_print_soft_material", 0x0857_fcd8);
const MI_PRINT_SOFT_FOG: Routine = Routine::new("mi_print_soft_fog", 0x0858_0430);
const CID_TEXTURES2D: u32 = 0x0878_16c8;
const CID_TEXTURES3D: u32 = 0x0878_16cc;
const CID_MATERIAL_SHADERS: u32 = 0x0878_1690;
const CID_SHADOW_SHADERS: u32 = 0x0878_16a0;
const CID_VOLUME_SHADERS: u32 = 0x0878_169c;
const CID_ENVIRON_SHADERS: u32 = 0x0878_16b0;
const CID_PHOTON_SHADERS: u32 = 0x0878_16e8;
const FRAME_CURRENT: u32 = 0x0878_2040;
const FLAG_879116E: u32 = 0x0879_116e;

/// `strdup` (an import the checks run natively): a heap copy of the string.
fn strdup_m(m: &mut Mem, s: u32) -> u32 {
    let t = m.cstr(s);
    let p = m.malloc(t.len() as u32 + 1);
    m.put_cstr(p, &t);
    p
}

fn spf(m: &mut Mem, scr: u32, buf: u32, f: &[u8], w: &[u32]) {
    // the original's empty and one-dash string constants are not in the image of a check: stand-ins at `scr`
    let w: Vec<u32> = w
        .iter()
        .map(|&a| match a {
            0x086e_ef50 | 0x086e_ef51 | 0x086e_ef52 | 0x086e_ef55 | 0x086e_ef56 | 0x086e_ef59 | 0x086e_ef68 | 0x086e_ef6b | 0x086e_f134 | 0x086e_f135 | 0x086e_f13c | 0x086e_f13d | 0x086e_f14a | 0x086e_f151 | 0x086e_f152 | 0x086e_f24c | 0x086e_f254 | 0x086e_f261 | 0x086e_f262 | 0x086e_f31f | 0x086e_f327 | 0x086e_f334 | 0x086e_f335 => scr,
            0x086e_ef53 | 0x086e_ef57 | 0x086e_ef69 => scr + 2,
            _ => a,
        })
        .collect();
    let mut t = crate::cprintf::format_bytes(m, f, &w);
    t.push(0);
    m.set_bytes(buf, &t);
}

/// One material of a model written as a mental ray material block, with the shaders of its
/// displace, shadow, volume, environment and photon slots [soft 0x0857cfd0 print_material].
///
/// `param_4` / `param_7` are the model-name strings the texture names are built from; `param_6`
/// (a string, or null) is the suffix of the material's name.
pub fn print_material(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, p4: u32, p5: u32, p6: u32, p7: u32) {
    // the original's frame: 0x87c bytes of locals, three saved registers, the saved frame pointer
    let l = m.local(0x88c);
    let scr = m.local(8);
    m.set_bytes(scr, &[0, 0, b'-', 0]);
    print_material_body(m, o, [dict, elem, mat, p4, p5, p6, p7], l + 0x888, scr);
    m.release(8);
    m.release(0x88c);
}

fn print_material_body(m: &mut Mem, o: &mut dyn Outside, a: [u32; 7], ebp: u32, scr: u32) {
    use crate::mi_mr_main::ext;
    let [dict, elem, mat, p4, p5, p6, p7] = a;
    let (buf72c, buf52c, buf32c, buf12c, bufc8, buf64, buf800) = (ebp - 0x72c, ebp - 0x52c, ebp - 0x32c, ebp - 0x12c, ebp - 0xc8, ebp - 0x64, ebp - 0x800);
    let (l_state, l_out6, l_outd) = (ebp - 0x804, ebp - 0x808, ebp - 0x80c);
    if dict == 0 || elem == 0 || m.u32(elem + 4) == 0 || mat == 0 || m.u32(mat + 4) == 0 {
        return;
    }
    let erec = m.u32(elem + 4);
    let mrec = m.u32(mat + 4);
    let c2d = m.u32(CID_TEXTURES2D);
    let n190 = o.call(m, crate::mi_poly::RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x190, c2d]).eax as i32;
    let n191 = o.call(m, crate::mi_poly::RELATIONS_COUNT, &[dict, mat, 1, 3, 0, 0x191, c2d]).eax as i32;
    let c3d = m.u32(CID_TEXTURES3D);
    let t3m = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x1f5, c3d, 0, 0]).eax;
    let t3e = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x1f4, c3d, 0, 0]).eax;
    let mut name = if m.u32(mat) != 0 { m.u32(m.u32(mat) + 0x10) } else { 0 };
    let incr = {
        // strdup of "" or "incremental " (the literals are not in the image of a check)
        let t: &[u8] = if m.u32(INCREMENTAL) == 0 { b"" } else { b"incremental " };
        let p = m.malloc(t.len() as u32 + 1);
        m.put_cstr(p, t);
        p
    };
    if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 {
        name = 0;
    }
    let mi2 = m.u16(MI2OUTPUT) != 0;
    let mut counter: i32 = if t3m != 0 || t3e != 0 { 1 } else { 0 };
    // the surface number of every texture: the material's after the element's slots, then the element's
    let mut e = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x191, c2d, 0, l_state]).eax;
    let mut i = 0i32;
    if e != 0 && 0 < n191 {
        loop {
            let at = TEXSURFACENO.wrapping_add(((n190 + i) as u32).wrapping_mul(4));
            if m.u16(e + 0x2a).wrapping_sub(7) < 2 {
                m.set_u32(at, 0);
            } else {
                m.set_u32(at, counter as u32);
                counter += 1;
            }
            e = o.call(m, ELEMDICT_NEXT, &[dict, mat, 1, 3, 0x191, c2d, 0, l_state]).eax;
            i += 1;
            if !(e != 0 && i < n191) {
                break;
            }
        }
    }
    e = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x190, c2d, 0, l_state]).eax;
    i = 0;
    while e != 0 && i < n190 {
        let at = TEXSURFACENO.wrapping_add((i as u32).wrapping_mul(4));
        if m.u16(e + 0x2a).wrapping_sub(7) < 2 {
            m.set_u32(at, 0);
        } else {
            m.set_u32(at, counter as u32);
            counter += 1;
        }
        e = o.call(m, ELEMDICT_NEXT, &[dict, elem, 1, 3, 0x190, c2d, 0, l_state]).eax;
        i += 1;
    }
    // the last texture of kind 7 or 8: the environment texture
    let (mut env, mut env_pre, mut env_idx, mut env_owner) = (0u32, 0u32, 0u32, 0u32);
    if m.u16(FLAG_8791166) != 0 && m.u16(FLAG_879116A) != 0 {
        e = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x191, c2d, l_out6, l_state]).eax;
        let mut idx = 0u32;
        while e != 0 {
            if m.u16(e + 0x2a).wrapping_sub(7) < 2 {
                env_owner = m.u32(l_out6);
                env_idx = idx;
                env_pre = mrec;
                env = e;
            }
            e = o.call(m, ELEMDICT_NEXT, &[dict, mat, 1, 3, 0x191, c2d, l_out6, l_state]).eax;
            idx += 1;
        }
        e = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x190, c2d, l_out6, l_state]).eax;
        idx = 0;
        while e != 0 {
            if m.u16(e + 0x2a).wrapping_sub(7) < 2 {
                env_pre = 0x086e_ef51;
                env_owner = m.u32(l_out6);
                env_idx = idx;
                env = e;
            }
            e = o.call(m, ELEMDICT_NEXT, &[dict, elem, 1, 3, 0x190, c2d, l_out6, l_state]).eax;
            idx += 1;
        }
    }
    // the user-defined material shader
    let cms = m.u32(CID_MATERIAL_SHADERS);
    let udf = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x226, cms, 0, 0]).eax;
    let mut udf_en = false;
    if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, buf64, 100]).eax as u16 != 0 && udf != 0 {
        udf_en = o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0;
    }
    let mut opaque;
    if udf_en || m.u16(mrec + 0x758) != 0 || m.u32(mrec + 0xdc8) != 0x0001_0001 || mi2 {
        opaque = false;
    } else {
        opaque = m.f32(mrec + 0x6b8) == 0.0;
        e = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x191, c2d, 0, l_state]).eax;
        if e != 0 && opaque {
            loop {
                opaque = m.f32(e + 0x138) == 0.0 && m.u16(e + 0x18e) == 0;
                e = o.call(m, ELEMDICT_NEXT, &[dict, mat, 1, 3, 0x191, c2d, 0, l_state]).eax;
                if !(e != 0 && opaque) {
                    break;
                }
            }
        }
        if opaque && t3m != 0 {
            opaque = m.f32(t3m + 0x118) == 0.0;
        }
        e = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x190, c2d, 0, l_state]).eax;
        if e != 0 && opaque {
            loop {
                opaque = m.f32(e + 0x138) == 0.0 && m.u16(e + 0x18e) == 0;
                e = o.call(m, ELEMDICT_NEXT, &[dict, elem, 1, 3, 0x190, c2d, 0, l_state]).eax;
                if !(e != 0 && opaque) {
                    break;
                }
            }
        }
        if opaque && t3e != 0 {
            opaque = m.f32(t3e + 0x118) == 0.0;
        }
    }
    // the material's name in the file
    let sp = m.u32(SOFT_PREFIX);
    if p6 != 0 {
        let a0 = if name != 0 { name } else { 0x086e_ef59 };
        let a1 = if name != 0 { 0x086e_ef57 } else { 0x086e_ef56 };
        let a3 = if p4 != 0 { p4 } else { 0x086e_ef55 };
        let a4 = if p4 != 0 { 0x086e_ef53 } else { 0x086e_ef52 };
        spf(m, scr, buf32c, b"%s%s%s_%s%s%s", &[a0, a1, mrec, a3, a4, p6]);
    } else {
        let a0 = if name != 0 { name } else { 0x086e_ef6b };
        let a1 = if name != 0 { 0x086e_ef69 } else { 0x086e_ef68 };
        spf(m, scr, buf32c, b"%s%s%s", &[a0, a1, mrec]);
    }
    let mut hdr = 0u32; // the strdup of the shader name (-0x848)
    let (mut s_disp, mut s_shad, mut s_vol, mut s_env) = (0u32, 0u32, 0u32, 0u32);
    let soft = |m: &mut Mem, o: &mut dyn Outside| {
        o.call(m, MI_PRINT_SOFT_MATERIAL, &[dict, elem, mat, p4, p7, name, env, t3m, t3e, n190 as u32, n191 as u32, counter as u32, p5]);
    };
    if !mi2 {
        if udf_en {
            fprintf(m, o, &[0x086e_efb3, incr, buf32c, buf64]);
        } else {
            fprintf(m, o, &[0x086e_efe0, incr, buf32c, if opaque { 0x086e_efc9 } else { 0x086e_efd1 }, sp]);
        }
    } else if udf_en {
        let ax = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[udf, buf12c, 100]).eax as u16 != 0;
        o.call(m, crate::mi2_light::BUILD_SHADER_NAME_WITH_STRING, &[buf52c, if ax { buf12c } else { buf64 }, buf32c]);
        hdr = strdup_m(m, buf52c);
        fprintf(m, o, &[0x086e_ef73, incr, buf52c, buf64]);
    } else {
        spf(m, scr, buf52c, b"\"%s_material-%s\"", &[sp, buf32c]);
        hdr = strdup_m(m, buf52c);
        fprintf(m, o, &[0x086e_ef96, incr, buf52c, sp]);
    }
    let frame = |m: &Mem| m.u32(FRAME_CURRENT);
    if udf_en {
        if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
            if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
                fprintf(m, o, &[0x086e_f004]);
            }
            soft(m, o);
            if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
                fprintf(m, o, &[0x086e_f015]);
            }
        }
        if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
            fprintf(m, o, &[0x086e_f018]);
        }
        let fc = frame(m);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
        fprintf(m, o, &[0x086e_f01b]);
    } else {
        soft(m, o);
        fprintf(m, o, &[0x086e_f020]);
    }
    // displace / static blur
    if m.u16(mrec + 0x758) != 0 && m.u32(erec + 0x30) == 1 {
        let d = dw(m.f32(mrec + 0x75c) as f64);
        if !mi2 {
            fprintf(m, o, &[0x086e_f080, d[0], d[1]]);
        } else {
            spf(m, scr, buf52c, b"\"soft_staticblur-%s\"", &[buf32c]);
            s_disp = strdup_m(m, buf52c);
            fprintf(m, o, &[0x086e_f040, incr, buf52c, d[0], d[1]]);
        }
    } else {
        let (mut t_cur, t_end);
        if !mi2 {
            t_cur = 0x086e_f100;
            t_end = 0x086e_f12d;
        } else {
            t_cur = buf52c;
            spf(m, scr, buf52c, b"\"soft_displace-%s\"", &[buf32c]);
            s_disp = strdup_m(m, buf52c);
            spf(m, scr, buf52c, b"%sshader %s \n\"soft_displace\" (\n\t\"texture\" [\n\t{\n", &[incr, s_disp]);
            t_end = 0x086e_f0f0;
        }
        let mut hit = false;
        if m.u16(FLAG_8791168) != 0 {
            if t3m != 0 && m.u16(t3m + 0x102) != 0 && m.u16(t3m + 0x150) != 0 {
                hit = o.call(m, P3D_MAP, &[3, t3m, t_cur, p4, p7, mrec]).eax != 0;
            }
            if !hit && t3e != 0 && m.u16(t3e + 0x102) != 0 && m.u16(t3e + 0x150) != 0 {
                hit = o.call(m, P3D_MAP, &[3, t3e, t_cur, p4, p7, 0x086e_f134]).eax != 0;
            }
            if hit {
                t_cur = t_end;
            }
        }
        if m.u16(FLAG_8791166) != 0 {
            let mut e = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x191, c2d, l_outd, l_state]).eax;
            let mut i = 0u32;
            while e != 0 {
                if (m.u16(e + 0x120) != 0 || mi2) && m.u16(e + 0x19c) != 0 {
                    if !mi2 {
                        let nm = if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 { 0x086e_f13d } else { name };
                        spf(m, scr, buf72c, b"%s_%s_%s_%d", &[nm, 0x086e_f13c, mrec, i]);
                    } else {
                        let ow = m.u32(l_outd);
                        let mut s = m.u32(m.u32(ow) + 0x10);
                        if s == 0 {
                            s = 0x086e_f135;
                        }
                        let tn = m.u32(e + 0x18);
                        spf(m, scr, buf72c, b"%s_%s", &[s, tn]);
                    }
                    let sn = m.u32(TEXSURFACENO.wrapping_add((n190 as u32).wrapping_add(i).wrapping_mul(4)));
                    if o.call(m, P2D_MAP, &[3, e, t_cur, buf72c, sn]).eax != 0 {
                        t_cur = t_end;
                    }
                }
                e = o.call(m, ELEMDICT_NEXT, &[dict, mat, 1, 3, 0x191, c2d, l_outd, l_state]).eax;
                i += 1;
            }
            e = o.call(m, ELEMDICT_FIRST, &[dict, elem, 1, 3, 0x190, c2d, l_outd, l_state]).eax;
            i = 0;
            while e != 0 {
                if (m.u16(e + 0x120) != 0 || mi2) && m.u16(e + 0x19c) != 0 {
                    if !mi2 {
                        let nm = if o.call(m, EMPTY_STRING, &[p4]).eax as u16 != 0 { 0x086e_f152 } else { p4 };
                        spf(m, scr, buf72c, b"%s_%s_%s_%d", &[nm, p7, 0x086e_f151, i]);
                    } else {
                        let ow = m.u32(l_outd);
                        let mut s = m.u32(m.u32(ow) + 0x10);
                        if s == 0 {
                            s = 0x086e_f14a;
                        }
                        let tn = m.u32(e + 0x18);
                        spf(m, scr, buf72c, b"%s_%s", &[s, tn]);
                    }
                    let sn = m.u32(TEXSURFACENO.wrapping_add(i.wrapping_mul(4)));
                    if o.call(m, P2D_MAP, &[3, e, t_cur, buf72c, sn]).eax != 0 {
                        t_cur = t_end;
                    }
                }
                e = o.call(m, ELEMDICT_NEXT, &[dict, elem, 1, 3, 0x190, c2d, l_outd, l_state]).eax;
                i += 1;
            }
        }
        if t_cur == t_end {
            fprintf(m, o, &[0x086e_f15f]);
        } else if s_disp != 0 {
            m.free(s_disp);
            s_disp = 0;
        }
    }
    // shadow
    if m.u16(FLAG_879116E) != 0 {
        let cs = m.u32(CID_SHADOW_SHADERS);
        let u = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x226, cs, 0, 0]).eax;
        if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[u, bufc8, 100]).eax as u16 != 0 && u != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[u]).eax as u16 != 0 {
            if !mi2 {
                fprintf(m, o, &[0x086e_f17a, bufc8]);
            } else {
                let ax = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[u, buf12c, 100]).eax as u16 != 0;
                o.call(m, crate::mi2_light::BUILD_SHADER_NAME_WITH_STRING, &[buf52c, if ax { buf12c } else { bufc8 }, buf32c]);
                s_shad = strdup_m(m, buf52c);
                fprintf(m, o, &[0x086e_f168, incr, buf52c, bufc8]);
            }
            if o.call(m, ext::SI_UDF_USE_DEFAULT, &[u]).eax as u16 != 0 {
                fprintf(m, o, &[0x086e_f188]);
                soft(m, o);
                fprintf(m, o, &[0x086e_f199]);
            }
            let fc = frame(m);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, u, fc]);
            fprintf(m, o, &[0x086e_f19e]);
        } else if !mi2 {
            if udf_en {
                fprintf(m, o, &[0x086e_f1a2, buf64]);
            } else if !(opaque && m.u16(HAVE_SELECTIVE_LIGHTS) == 0) {
                fprintf(m, o, &[0x086e_f1b1, sp]);
            }
        }
    }
    // with mental ray 2 output the shadow name falls back to the header's
    if mi2 && s_shad == 0 {
        s_shad = strdup_m(m, hdr);
    }
    // volume
    let cv = m.u32(CID_VOLUME_SHADERS);
    let u = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x226, cv, 0, 0]).eax;
    if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[u, bufc8, 100]).eax as u16 != 0 && u != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[u]).eax as u16 != 0 {
        if !mi2 {
            fprintf(m, o, &[0x086e_f1dd, bufc8]);
        } else {
            let ax = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[u, buf12c, 100]).eax as u16 != 0;
            o.call(m, crate::mi2_light::BUILD_SHADER_NAME_WITH_STRING, &[buf52c, if ax { buf12c } else { bufc8 }, buf32c]);
            s_vol = strdup_m(m, buf52c);
            fprintf(m, o, &[0x086e_f1cb, incr, buf52c, bufc8]);
        }
        if o.call(m, ext::SI_UDF_USE_DEFAULT, &[u]).eax as u16 != 0 {
            fprintf(m, o, &[0x086e_f1eb]);
            o.call(m, MI_PRINT_SOFT_FOG, &[]); // takes no argument
            fprintf(m, o, &[0x086e_f1fc]);
        }
        let fc = frame(m);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, u, fc]);
        fprintf(m, o, &[0x086e_f201]);
    }
    // environment
    if env != 0 && m.u16(FLAG_8791166) != 0 && m.u16(FLAG_879116A) != 0 {
        let ce = m.u32(CID_ENVIRON_SHADERS);
        let u = o.call(m, ELEMDICT_FIRST, &[dict, env_owner, 1, 3, 0x226, ce, 0, 0]).eax;
        let mut flag = false;
        if u != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[u]).eax as u16 != 0 {
            flag = o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[u, bufc8, 100]).eax != 0;
        }
        let owner_name = |m: &mut Mem| -> u32 { m.u32(m.u32(env_owner) + 0x10) };
        if flag {
            let usedef = o.call(m, ext::SI_UDF_USE_DEFAULT, &[u]).eax as u16 != 0;
            if !mi2 {
                fprintf(m, o, &[0x086e_f217, bufc8]);
            } else {
                let ax = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[u, buf12c, 100]).eax as u16 != 0;
                o.call(m, crate::mi2_light::BUILD_SHADER_NAME_WITH_STRING, &[buf52c, if ax { buf12c } else { bufc8 }, buf32c]);
                s_env = strdup_m(m, buf52c);
                fprintf(m, o, &[0x086e_f205, incr, buf52c, bufc8]);
            }
            if usedef {
                fprintf(m, o, &[0x086e_f22a]);
                if !mi2 {
                    if env_pre == 0x086e_f253 {
                        let nm = if o.call(m, EMPTY_STRING, &[p4]).eax as u16 != 0 { 0x086e_f254 } else { p4 };
                        spf(m, scr, buf72c, b"%s_%s_%s_%d", &[nm, p7, env_pre, env_idx]);
                    } else {
                        let nm = if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 { 0x086e_f262 } else { name };
                        spf(m, scr, buf72c, b"%s_%s_%s_%d", &[nm, 0x086e_f261, env_pre, env_idx]);
                    }
                } else {
                    let on = owner_name(m);
                    let nm = if o.call(m, EMPTY_STRING, &[on]).eax as u16 != 0 { 0x086e_f24c } else { on };
                    let tn = m.u32(env + 0x18);
                    spf(m, scr, buf72c, b"%s_%s", &[nm, tn]);
                }
                if o.call(m, P2D_MAP, &[2, env, 0x086e_f23b, buf72c, 0]).eax != 0 {
                    fprintf(m, o, &[0x086e_f26f]);
                }
            }
            let fc = frame(m);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, u, fc]);
            fprintf(m, o, &[0x086e_f27a]);
        } else {
            let d = dw(m.f32(env + 0x168) as f64);
            let tgt;
            if !mi2 {
                tgt = buf800;
                spf(m, scr, buf800, b"\tenvironment \"soft_env_sphere\" (\n\t\"rotate\"\t%f,\n\t\"texture\" [ {\n", &[d[0], d[1]]);
            } else {
                tgt = buf52c;
                spf(m, scr, buf52c, b"\"soft_env_sphere-%s\"", &[buf32c]);
                s_env = strdup_m(m, buf52c);
                spf(m, scr, buf52c, b"%sshader %s \n\"soft_env_sphere\" (\n\t\"rotate\"\t%f,\n\t\"texture\" [ {\n", &[incr, s_env, d[0], d[1]]);
            }
            if !mi2 {
                if env_pre == 0x086e_f326 {
                    let nm = if o.call(m, EMPTY_STRING, &[p4]).eax as u16 != 0 { 0x086e_f327 } else { p4 };
                    spf(m, scr, buf72c, b"%s_%s_%s_%d", &[nm, p7, env_pre, env_idx]);
                } else {
                    let nm = if o.call(m, EMPTY_STRING, &[name]).eax as u16 != 0 { 0x086e_f335 } else { name };
                    spf(m, scr, buf72c, b"%s_%s_%s_%d", &[nm, 0x086e_f334, env_pre, env_idx]);
                }
            } else {
                let on = owner_name(m);
                let nm = if o.call(m, EMPTY_STRING, &[on]).eax as u16 != 0 { 0x086e_f31f } else { on };
                let tn = m.u32(env + 0x18);
                spf(m, scr, buf72c, b"%s_%s", &[nm, tn]);
            }
            if o.call(m, P2D_MAP, &[2, env, tgt, buf72c, 0]).eax != 0 {
                fprintf(m, o, &[0x086e_f342]);
            }
        }
    }
    // the end of the block, and (mental ray 2) the photon shader
    let mi2_now = m.u16(MI2OUTPUT) != 0;
    let mut go = mi2_now;
    if !mi2_now {
        fprintf(m, o, &[0x086e_f34b]);
        go = m.u16(MI2OUTPUT) != 0;
    }
    if go {
        let cp = m.u32(CID_PHOTON_SHADERS);
        let u = o.call(m, ELEMDICT_FIRST, &[dict, mat, 1, 3, 0x226, cp, 0, 0]).eax;
        let p15;
        if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[u, bufc8, 100]).eax as u16 == 0 || u == 0 || o.call(m, ext::SI_UDF_IS_ENABLED, &[u]).eax as u16 == 0 {
            if udf_en {
                spf(m, scr, buf52c, b"\"%s_material-%s\"", &[sp, buf32c]);
                p15 = strdup_m(m, buf52c);
                fprintf(m, o, &[0x086e_f397, incr, buf52c, sp]);
                soft(m, o);
                fprintf(m, o, &[0x086e_f3b4]);
            } else {
                spf(m, scr, buf52c, b"\"%s_material-%s\"", &[sp, buf32c]);
                p15 = strdup_m(m, buf52c);
            }
        } else {
            let ax = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[u, buf12c, 100]).eax as u16 != 0;
            o.call(m, crate::mi2_light::BUILD_SHADER_NAME_WITH_STRING, &[buf52c, if ax { buf12c } else { bufc8 }, buf32c]);
            p15 = strdup_m(m, buf52c);
            fprintf(m, o, &[0x086e_f35a, incr, buf52c, bufc8]);
            if o.call(m, ext::SI_UDF_USE_DEFAULT, &[u]).eax as u16 != 0 {
                fprintf(m, o, &[0x086e_f36c]);
                soft(m, o);
                fprintf(m, o, &[0x086e_f37d]);
            }
            let fc = frame(m);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, u, fc]);
            fprintf(m, o, &[0x086e_f382, incr, buf52c]);
        }
        if m.u32(INCREMENTAL) == 0 {
            fprintf(m, o, &[0x086e_f3d3, incr, buf32c, if opaque { 0x086e_f3ca } else { 0x086e_f3d2 }]);
            fprintf(m, o, &[0x086e_f3e7, hdr]);
            if s_disp != 0 {
                fprintf(m, o, &[0x086e_f3ee, s_disp]);
            }
            if s_shad != 0 {
                fprintf(m, o, &[0x086e_f3fe, s_shad]);
            }
            if s_vol != 0 {
                fprintf(m, o, &[0x086e_f40c, s_vol]);
            }
            if s_env != 0 {
                fprintf(m, o, &[0x086e_f41a, s_env]);
            }
            if p15 != 0 {
                fprintf(m, o, &[0x086e_f42d, p15]);
            }
            fprintf(m, o, &[0x086e_f43b]);
        }
        for p in [hdr, s_disp, s_shad, s_vol, s_env, p15] {
            if p != 0 {
                m.free(p);
            }
        }
    }
    m.free(incr);
}
