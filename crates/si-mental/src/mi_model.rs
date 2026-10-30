// SPDX-License-Identifier: MIT
//! `mi_model.c`: the models of the mental ray scene text (package 63, finding 0042). Same conventions as
//! `mi_light`: printed lines are `SI_mrpipeFPRINTF` calls with the format's address and the argument words
//! (debug and error text on `stderr`/`stdout` goes through `Outside::print`); callees of other files are
//! `Routine`s called through `Outside`.

use crate::cprintf;
use crate::mi_calc::{SDEBUG, SWARNING};
use crate::mi_mr_main::{ext, MRPIPE_FPRINTF};
use crate::mi_patch::RELATION_CHECK;
use crate::mi_poly::{INVERT_MATRIX_R, RELATIONS_COUNT};
use si_mem::{Mem, Outside, Routine};

pub const PRINT_ALL_MODELS: Routine = Routine::new("print_all_models", 0x0858_5e60);
pub const PRINT_MODEL_TREE: Routine = Routine::new("print_model_tree", 0x0858_5ef8);
pub const PRINT_MODEL: Routine = Routine::new("print_model", 0x0858_607c);
pub const MATRIX_X_MATRIX: Routine = Routine::new("matrix_X_matrix", 0x0859_849c);
pub const SI_ELEMDICT_FIND_SON: Routine = Routine::new("SI_elemdictFindSon", 0x0813_7ff4);
pub const SI_ELEMDICT_FIND_NEXT_SON: Routine = Routine::new("SI_elemdictFindNextSon", 0x0813_8030);
pub const SI_MODEL_NB_TRIANGLES: Routine = Routine::new("SI_modelNbTriangles", 0x0846_cd80);
pub const SI_ELEMDICT_GET_ELEM_ID: Routine = Routine::new("SI_elemdictGetElemId", 0x0813_2b94);
pub const PRINT_DEFAULT_MATERIAL: Routine = Routine::new("print_default_material", 0x0857_eab0);
pub const PRINT_MESH_GEOMETRY: Routine = Routine::new("print_mesh_geometry", 0x0858_cc30);
pub const PRINT_NURBS_GEOMETRY: Routine = Routine::new("print_nurbs_geometry", 0x0858_67d0);
pub const PRINT_PATCH_GEOMETRY: Routine = Routine::new("print_patch_geometry", 0x0858_78d0);
pub const PRINT_TRIANGLES: Routine = Routine::new("print_triangles", 0x0858_e6a8);
pub const SI_EMPTY_STRING: Routine = Routine::new("SI_emptyString", 0x082c_b774);

const CID_MATERIALS: u32 = 0x0878_167c;
const CID_MODELS: u32 = 0x0878_1680;
const CID_TEXTURES2D: u32 = 0x0878_16c8;
const CID_TEXTURES3D: u32 = 0x0878_16cc;
const MODEL_NAME: u32 = 0x0882_9f48;
const DEBUG_FLAG: u32 = 0x0882_9f0c;
const SI_SHW_CTL: u32 = 0x0867_9516;

fn fp(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

fn err(m: &mut Mem, o: &mut dyn Outside, fmt: &[u8], words: &[u32]) {
    let t = cprintf::format_bytes(m, fmt, words);
    o.print(2, &t);
}

/// A `double` as its two stack words.
fn dw(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// Three floats at `a` widened to three `double` arguments after `fmt`.
fn line3(m: &Mem, fmt: u32, a: u32) -> Vec<u32> {
    let mut v = vec![fmt];
    for k in 0..3 {
        v.extend(dw(m.f32(a + 4 * k) as f64));
    }
    v
}

fn sext(v: u32) -> u32 {
    v as u16 as i16 as i32 as u32
}

/// [soft 0x08585e60 print_all_models]: every visible top-level model of the dictionary (not hidden at
/// +0x48, no parent at +0x54) goes through `print_model_tree`. (The original also stores 1.0 into three
/// words of its caller's argument area; they are not read again.)
pub fn print_all_models(m: &mut Mem, o: &mut dyn Outside, dict: u32, p: u32, q: u32, _f1: u32, _f2: u32, _f3: u32, a: u32, b: u32, c: u32) {
    if dict == 0 {
        return;
    }
    let (a, b, c) = (sext(a), sext(b), sext(c));
    let list = m.u32(CID_MODELS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(list + 4) {
        let e = off.wrapping_add(m.u32(list + 8));
        if m.u16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 {
            o.call(m, PRINT_MODEL_TREE, &[dict, e, p, q, a, b, c]);
        }
        off = off.wrapping_add(0x84);
        i += 1;
    }
}

/// [soft 0x08585ef8 print_model_tree]: one model and then its sons. The model is written when its kind
/// (+0x30 of the definition) is one of the printable ones, it has a data record (+0x20), the pass is
/// the first (`s1 == 1`) or the element's state (+0x24) is 1, 2 or 3, and either shading control is on or
/// the element's type is outside the 0x12..0x1d range. A non-expanded instance (type 0xf) is reported on
/// `stdout`; a P-mesh (kind 3) gives a warning; an instance of type 0xd is skipped.
pub fn print_model_tree(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, p: u32, q: u32, s1: u32, s2: u32, s3: u32) {
    let (s1, s2, s3) = (s1 as u16, s2 as u16, s3 as u16);
    if dict == 0 || elem == 0 || m.u16(elem + 0x48) != 0 {
        return;
    }
    let md = m.u32(elem + 4);
    if md == 0 {
        return;
    }
    let l = m.local(0x40);
    o.call(m, MATRIX_X_MATRIX, &[l, m.u32(md + 0x44).wrapping_add(0x1c4), p]);
    let ty = m.u32(md + 0x2ec);
    let mut body = false;
    if ty == 0xf {
        let t = cprintf::format_bytes(m, b"ERROR: Non-expanded instance found: <%s>\n", &[m.u32(md + 4)]);
        o.print(1, &t);
    } else if ty != 0xd {
        let sw = m.u32(md + 0x30).wrapping_add(1);
        match sw {
            2 | 3 | 5 | 8 | 11 => body = true,
            4 => {
                o.call(m, SWARNING, &[0x086f_1640]);
            }
            _ => {}
        }
    }
    if body {
        let st = m.u32(elem + 0x24);
        let ok = m.u32(elem + 0x20) != 0
            && (s1 == 1 || (st == 1 || st == 3 || st == 2))
            && (m.u16(SI_SHW_CTL) != 0 || ty.wrapping_sub(0x12) > 0xb);
        if ok {
            let name = m.u32(m.u32(elem).wrapping_add(0x10));
            o.call(m, PRINT_MODEL, &[dict, elem, l, q, name, sext(s2 as u32), sext(s3 as u32)]);
        }
    }
    let mut son = o.call(m, SI_ELEMDICT_FIND_SON, &[dict, elem]).eax;
    while son != 0 {
        print_model_tree(m, o, dict, son, p, q, sext(s1 as u32), sext(s2 as u32), sext(s3 as u32));
        son = o.call(m, SI_ELEMDICT_FIND_NEXT_SON, &[dict, elem]).eax;
    }
    m.release(0x40);
}

/// [soft 0x0858607c print_model]: the materials of the model (debug counts on `stderr` when the debug
/// flag is on), the default material when it has none, and then one or two `object` blocks (the plain
/// one and the shadow one, by the kinds of materials found): the flags, tag, the inverted matrix as the
/// `transform` lines, the geometry by the model's kind and `end object`. `no_out` (`s1`) only collects
/// the geometry and writes no `object` text. The global model name is set for the diagnostics and put back
/// at the end (not when the model has no triangles).
pub fn print_model(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, q: u32, name: u32, s1: u32, s2: u32) {
    let (s1, s2) = (sext(s1), sext(s2));
    let saved = m.u32(MODEL_NAME);
    if dict == 0 || elem == 0 {
        return;
    }
    let md = m.u32(elem + 4);
    if md == 0 {
        return;
    }
    m.set_u32(MODEL_NAME, m.u32(md + 4));
    if (o.call(m, SI_MODEL_NB_TRIANGLES, &[md]).eax as i32) <= 0 {
        return;
    }
    let cid_mat = m.u32(CID_MATERIALS);
    let nmat = o.call(m, RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x12c, cid_mat]).eax;
    let mut nmat2 = nmat;
    if nmat != 0 && m.u32(md + 0x30) != 4 {
        nmat2 = 1;
    }
    let l = m.local(0x48);
    let (inv, out, state) = (l, l + 0x40, l + 0x44);
    if m.u32(DEBUG_FLAG) != 0 {
        let first = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, out, state]).eax;
        if nmat != 0 {
            let cid2 = m.u32(CID_TEXTURES2D);
            let n2 = o.call(m, RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x190, cid2]).eax;
            let ior = if first != 0 { m.f32(first + 0x6dc) as f64 } else { 1.0 };
            let mut a = vec![0x086f_1665, nmat];
            a.extend(dw(ior));
            a.push(n2);
            o.call(m, SDEBUG, &a);
        } else {
            o.call(m, SDEBUG, &[0x086f_167e]);
        }
        while m.u32(out) != 0 {
            let cid2 = m.u32(CID_TEXTURES2D);
            let n = o.call(m, RELATIONS_COUNT, &[dict, m.u32(out), 1, 3, 0, 0x191, cid2]).eax;
            err(m, o, b"+%d", &[n]);
            o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, out, state]);
        }
        let cid3 = m.u32(CID_TEXTURES3D);
        let chk = o.call(m, RELATION_CHECK, &[dict, elem, 1, 0x1f4, cid3]).eax as u16;
        err(m, o, b" 2D maps, %d", &[(chk == 0) as u32]);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, out, state]);
        while m.u32(out) != 0 {
            let cid3 = m.u32(CID_TEXTURES3D);
            let chk = o.call(m, RELATION_CHECK, &[dict, m.u32(out), 1, 0x1f5, cid3]).eax as u16;
            err(m, o, b"+%d", &[(chk == 0) as u32]);
            o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, out, state]);
        }
        err(m, o, b" 3D maps\n", &[]);
    }
    let mut flags = [0u32; 2];
    if s1 != 0 {
        flags = [1, 0];
    } else {
        flags[0] = (nmat2 == 0) as u32;
        flags[1] = 0;
        if nmat2 == 0 {
            o.call(m, PRINT_DEFAULT_MATERIAL, &[dict, elem]);
        }
        let mut e = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, 0, state]).eax;
        while e != 0 {
            if m.u16(e + 0x1e) == 6 {
                flags[1] = 1;
            } else {
                flags[0] = 1;
            }
            e = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, 0, state]).eax;
        }
    }
    o.call(m, INVERT_MATRIX_R, &[inv, mat]);
    for pass in 0..2u32 {
        if flags[pass as usize] == 0 {
            continue;
        }
        if s1 == 0 {
            let e = o.call(m, SI_EMPTY_STRING, &[name]).eax as u16;
            let who = if e != 0 { 0x086f_16a9 } else { name };
            let fmt = if pass != 0 { 0x086f_16aa } else { 0x086f_16bd };
            let mdname = m.u32(md + 4);
            fp(m, o, &[fmt, who, mdname]);
            let fl = m.u32(md + 0x3ec);
            if fl & 1 != 0 && pass == 0 {
                fp(m, o, &[0x086f_16cc]);
            }
            if fl & 4 != 0 || pass != 0 {
                fp(m, o, &[0x086f_16d5]);
            }
            if fl & 2 != 0 && pass == 0 {
                fp(m, o, &[0x086f_16dd]);
            }
            let cid = m.u32(CID_MODELS);
            let id = o.call(m, SI_ELEMDICT_GET_ELEM_ID, &[dict, cid, elem]).eax;
            fp(m, o, &[0x086f_16e4, id.wrapping_add(1)]);
            let a = line3(m, 0x086f_16ec, inv);
            fp(m, o, &a);
            let a = line3(m, 0x086f_1706, inv + 0x10);
            fp(m, o, &a);
            let a = line3(m, 0x086f_1717, inv + 0x20);
            fp(m, o, &a);
            let a = line3(m, 0x086f_1728, inv + 0x30);
            fp(m, o, &a);
        }
        let kind = m.u32(md + 0x30);
        match kind.wrapping_sub(1) {
            3 => {
                o.call(m, PRINT_MESH_GEOMETRY, &[dict, elem, mat, q, pass, sext(s1 as u32), sext(s2 as u32)]);
            }
            9 => {
                o.call(m, PRINT_NURBS_GEOMETRY, &[dict, elem, mat, sext(s1 as u32), sext(s2 as u32)]);
            }
            0 => {
                o.call(m, PRINT_PATCH_GEOMETRY, &[dict, elem, mat, sext(s1 as u32), sext(s2 as u32)]);
            }
            1 | 6 => {
                o.call(m, PRINT_TRIANGLES, &[dict, elem, mat, q, sext(s1 as u32), sext(s2 as u32)]);
            }
            _ => {
                o.call(m, SWARNING, &[0x086f_1739, kind]);
            }
        }
        if s1 == 0 {
            fp(m, o, &[0x086f_1755]);
        }
    }
    m.release(0x48);
    m.set_u32(MODEL_NAME, saved);
}
