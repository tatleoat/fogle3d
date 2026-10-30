// SPDX-License-Identifier: MIT
//! `mi2_model.c`: the models of the mental ray 2 scene text (package 63, finding 0042). Same conventions as
//! `mi_model`: printed lines are `SI_mrpipeFPRINTF` calls with the format's address and the argument words;
//! callees of other files, and of this file, are `Routine`s called through `Outside`.

use crate::cprintf;
use crate::mi_mr_main::{ext, MRPIPE_FPRINTF};
use crate::mi_patch::{MEMTRACK_ALLOCATE, MEMTRACK_REALLOC, SI_MEM_PARAMS};
use crate::mi_poly::INVERT_MATRIX_R;
use si_mem::{Mem, Outside, Routine};

pub const SI_MEMTRACK_STRDUP: Routine = Routine::new("SI_memtrackStrdup", 0x0859_97e0);
pub const SI_ELEMDICT_FIND_RELATED_ELEM: Routine = Routine::new("SI_elemdictFindRelatedElem", 0x0813_508c);
pub const MODEL_HAS_SHADER_REFERENCE: Routine = Routine::new("modelHasShaderReference", 0x0859_7de4);
pub const ADD_TO_ROOT_LIST: Routine = Routine::new("addToRootList", 0x0859_6f88);
pub const SI_MI2_GET_MODEL_NAME: Routine = Routine::new("SI_mi2GetModelName", 0x0859_7994);
pub const PRINT_EMPTY_MESH: Routine = Routine::new("print_empty_mesh", 0x0859_7b98);
pub const PRINT_EMPTY_PATCH: Routine = Routine::new("print_empty_patch", 0x0859_7be0);
pub const PRINT_EMPTY_GEOMETRY: Routine = Routine::new("print_empty_geometry", 0x0859_7a40);
pub const PRINT_EMPTY_LIGHT: Routine = Routine::new("print_empty_light", 0x0859_7dbc);
const SI_EMPTY_STRING: Routine = Routine::new("SI_emptyString", 0x082c_b774);
const CID_LIGHTS: u32 = 0x0878_1678;
const CID_MODELS: u32 = 0x0878_1680;
const INCREMENTAL: u32 = 0x086e_f9bc;

fn fp(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

/// [soft 0x08597de4 modelHasShaderReference]: whether the element has a related element of kind 0x227.
pub fn model_has_shader_reference(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> u32 {
    let l = m.local(8);
    let r = o.call(m, SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, elem, 0, 0, 0x227, 0xffff_ffff, l, l + 4]).eax;
    m.release(8);
    (r != 0) as u32
}

/// [soft 0x08597dbc print_empty_light]: the empty light object of a name.
pub fn print_empty_light(m: &mut Mem, o: &mut dyn Outside, name: u32) {
    fp(m, o, &[0x086f_51e9, name]);
    fp(m, o, &[0x086f_51fc]);
}

/// [soft 0x08597b98 print_empty_mesh]: the empty mesh object of a name.
pub fn print_empty_mesh(m: &mut Mem, o: &mut dyn Outside, name: u32) {
    fp(m, o, &[0x086f_4e00, name]);
    for f in [0x086f_4e21, 0x086f_4e2f, 0x086f_4e3b] {
        fp(m, o, &[f]);
    }
}

/// [soft 0x08597be0 print_empty_patch]: the empty bezier patch object (a flat 16-point patch) of a name.
pub fn print_empty_patch(m: &mut Mem, o: &mut dyn Outside, name: u32) {
    fp(m, o, &[0x086f_4e60, name]);
    for f in [
        0x086f_4e81, 0x086f_4e9b, 0x086f_4eb5, 0x086f_4ec4, 0x086f_4edb, 0x086f_4ef2, 0x086f_4f09, 0x086f_4f20, 0x086f_4f37,
        0x086f_4f4e, 0x086f_4f65, 0x086f_4f7c, 0x086f_4f93, 0x086f_4faa, 0x086f_4fc1, 0x086f_4fd8, 0x086f_4fef, 0x086f_5006,
        0x086f_501d, 0x086f_5040, 0x086f_5060, 0x086f_5080, 0x086f_50a0, 0x086f_50c0, 0x086f_50d4, 0x086f_50f1, 0x086f_5120,
        0x086f_5160, 0x086f_51a0, 0x086f_51d0, 0x086f_51dc,
    ] {
        fp(m, o, &[f]);
    }
}

/// [soft 0x08596f88 addToRootList]: appends a copy of the name (and, when asked, `<name>_shadow`) to the
/// made array `*list`, counting in `*count`.
pub fn add_to_root_list(m: &mut Mem, o: &mut dyn Outside, list: u32, count: u32, name: u32, shadow: u32) {
    let shadow = shadow as u16;
    if name == 0 || list == 0 || count == 0 {
        return;
    }
    for pass in 0..2 {
        if pass == 1 && shadow == 0 {
            return;
        }
        let size = m.u32(count).wrapping_add(1).wrapping_shl(2);
        let cur = m.u32(list);
        let p = if cur != 0 {
            o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, cur, size]).eax
        } else {
            o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, size]).eax
        };
        m.set_u32(list, p);
        if p == 0 {
            if pass == 0 {
                continue;
            }
            return;
        }
        let src = if pass == 0 {
            name
        } else {
            let mut t = m.cstr(name);
            t.extend(b"_shadow");
            let tmp = m.local(t.len() as u32 + 1);
            m.put_cstr(tmp, &t);
            m.release(t.len() as u32 + 1);
            tmp
        };
        let s = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, src]).eax;
        let n = m.u32(count);
        m.set_u32(m.u32(list).wrapping_add(n.wrapping_shl(2)), s);
        m.set_u32(count, n.wrapping_add(1));
    }
}

/// [soft 0x08597994 SI_mi2GetModelName]: `MOD_<owner name>_<model name>` (owner name empty when none or empty)
/// copied into `buf` of `size` bytes (`strncpy`, zero-terminated); 0 when anything is missing.
pub fn si_mi2_get_model_name(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, buf: u32, size: u32) -> u32 {
    if dict == 0 || elem == 0 {
        return 0;
    }
    let ld = m.u32(elem + 4);
    if ld == 0 || buf == 0 || (size as i32) <= 0 {
        return 0;
    }
    m.set_u8(buf, 0);
    let own = m.u32(elem);
    let mut name = 0u32;
    if own != 0 {
        let n = m.u32(own + 0x10);
        if n != 0 {
            name = n;
        }
    }
    let who = if o.call(m, SI_EMPTY_STRING, &[name]).eax as u16 != 0 { Vec::new() } else { m.cstr(name) };
    let mut text = b"MOD_".to_vec();
    text.extend(who);
    text.push(b'_');
    text.extend(m.cstr(m.u32(ld + 4)));
    let take = (size - 1) as usize;
    let mut out = text.clone();
    out.truncate(take);
    out.resize(take, 0);
    m.set_bytes(buf, &out);
    m.set_u8(buf + size - 1, 0);
    1
}

/// [soft 0x08597a40 print_empty_geometry]: an empty object for a model whose own text is not written (by its
/// kind a patch or a mesh) and its instance, and returns a made copy of its name; in an incremental file only
/// the instance of a model that has a shader reference and is flagged.
pub fn print_empty_geometry(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32) -> u32 {
    if dict == 0 || elem == 0 || m.u32(elem + 4) == 0 {
        return 0;
    }
    let flags = m.u8(elem + 0x58) as i8 as i32;
    let l = m.local(0x440);
    let (name, mat) = (l, l + 0x400);
    o.call(m, SI_MI2_GET_MODEL_NAME, &[dict, elem, name, 0x400]);
    let md = m.u32(elem + 4);
    if md != 0 {
        if m.u32(INCREMENTAL) == 0 {
            let k = m.u32(md + 0x30).wrapping_sub(1);
            if k == 0 || k == 9 {
                o.call(m, PRINT_EMPTY_PATCH, &[name]);
            } else {
                o.call(m, PRINT_EMPTY_MESH, &[name]);
            }
            let v = m.u32(md + 0x44);
            if v == 0xffff_fe3c {
                crate::mi_calc::init_matrix(m, mat);
            } else {
                o.call(m, INVERT_MATRIX_R, &[mat, v.wrapping_add(0x1c4)]);
            }
            o.call(m, ext::PRINT_INSTANCE, &[name, 1, mat]);
        } else if o.call(m, MODEL_HAS_SHADER_REFERENCE, &[dict, elem]).eax as u16 != 0 && (flags & 1 != 0 || flags & 2 != 0) {
            let v = m.u32(md + 0x44);
            if v != 0xffff_fe3c {
                o.call(m, INVERT_MATRIX_R, &[mat, v.wrapping_add(0x1c4)]);
                o.call(m, ext::PRINT_INSTANCE, &[name, 1, mat]);
            }
        }
    }
    let r = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, name]).eax;
    m.release(0x440);
    r
}

/// [soft 0x08597e18 si_mi2ModelForwardDefs]: the empty objects of the models (not hidden, not of type 0xd) and
/// of the lights (not hidden, no parent) that have a shader reference; for a light its `LIG_<owner>-<name>`
/// object and an identity instance.
pub fn si_mi2_model_forward_defs(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    let list = m.u32(CID_MODELS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(list + 4) {
        let e = off.wrapping_add(m.u32(list + 8));
        if m.u16(e + 0x48) == 0 {
            let ld = m.u32(e + 4);
            if ld != 0
                && m.u32(ld + 0x2ec) != 0xd
                && o.call(m, MODEL_HAS_SHADER_REFERENCE, &[dict, e]).eax as u16 != 0
            {
                let r = o.call(m, PRINT_EMPTY_GEOMETRY, &[dict, e]).eax;
                if r != 0 {
                    m.free(r);
                }
            }
        }
        i += 1;
        off = off.wrapping_add(0x84);
    }
    let list = m.u32(CID_LIGHTS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let l = m.local(0x140);
    let (name, mat) = (l, l + 0x100);
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(list + 4) {
        let e = off.wrapping_add(m.u32(list + 8));
        if m.u16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 {
            let ld = m.u32(e + 4);
            if ld != 0 && o.call(m, MODEL_HAS_SHADER_REFERENCE, &[dict, e]).eax as u16 != 0 {
                let own = m.u32(e);
                let n = if own != 0 { m.u32(own + 0x10) } else { 0 };
                let mut t = b"LIG_".to_vec();
                if n != 0 {
                    t.extend(m.cstr(n));
                    t.push(b'-');
                }
                t.extend(m.cstr(ld));
                m.put_cstr(name, &t);
                o.call(m, PRINT_EMPTY_LIGHT, &[name]);
                crate::mi_calc::init_matrix(m, mat);
                o.call(m, ext::PRINT_INSTANCE, &[name, 1, mat]);
            }
        }
        i += 1;
        off = off.wrapping_add(0x84);
    }
    m.release(0x140);
}

pub const MI2_PRINT_MODEL_TREE: Routine = Routine::new("mi2_print_model_tree", 0x0859_68c0);
pub const MI2_PRINT_MODEL: Routine = Routine::new("mi2_print_model", 0x0859_709c);
const SI_SHW_CTL: u32 = 0x0867_9516;

fn sext(v: u32) -> u32 {
    v as u16 as i16 as i32 as u32
}

/// [soft 0x08596710 mi2_print_all_models]: every visible top-level model of the dictionary (not hidden, no
/// parent) through `mi2_print_model_tree`. With `a == 1` it collects: the made array of the returned group names
/// (counted beforehand into `*count`); otherwise the trees add their names to the array themselves and the
/// count is stored at the end. (The original also stores 1.0 into three words of its caller's argument area;
/// they are not read again.)
pub fn mi2_print_all_models(
    m: &mut Mem, o: &mut dyn Outside, dict: u32, p: u32, q: u32, _f1: u32, _f2: u32, _f3: u32, a: u32, b: u32, c: u32, count: u32,
) -> u32 {
    if dict == 0 {
        return 0;
    }
    let (a, b, c) = (a as u16, b as u16, c as u16);
    let list = m.u32(CID_MODELS).wrapping_shl(6).wrapping_add(m.u32(dict + 4));
    let slot = m.local(4);
    let arr_idx = m.local(8);
    let (arr_p, idx_p) = (arr_idx, arr_idx + 4);
    m.set_u32(arr_p, 0);
    m.set_u32(idx_p, 0);
    m.set_u32(count, 0);
    if b == 0 {
        if a == 1 {
            let mut i = 0i32;
            let mut off = 0u32;
            while i < m.i32(list + 4) {
                let e = off.wrapping_add(m.u32(list + 8));
                if m.u16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 {
                    let n = m.u32(count).wrapping_add(1);
                    m.set_u32(count, n);
                }
                off = off.wrapping_add(0x84);
                i += 1;
            }
        }
        let n = m.u32(count) as i32;
        if n <= 0 && a == 1 {
            m.release(8);
            m.release(4);
            return 0;
        }
        if a == 1 && n > 0 {
            let blk = crate::mi_mr_main::si_malloc_blk(m, (n as u32).wrapping_shl(2));
            m.set_u32(arr_p, blk);
        }
    }
    let mut i = 0i32;
    let mut off = 0u32;
    while i < m.i32(list + 4) {
        let e = off.wrapping_add(m.u32(list + 8));
        if m.u16(e + 0x48) == 0 && m.u32(e + 0x54) == 0 {
            if b == 0 {
                if a == 1 {
                    let r = o.call(m, MI2_PRINT_MODEL_TREE, &[dict, e, p, q, 1, 0, sext(c as u32), slot, 0, 0]).eax;
                    let (arr, idx) = (m.u32(arr_p), m.u32(idx_p));
                    m.set_u32(arr.wrapping_add(idx.wrapping_shl(2)), r);
                    m.set_u32(idx_p, idx.wrapping_add(1));
                } else {
                    o.call(m, MI2_PRINT_MODEL_TREE, &[dict, e, p, q, sext(a as u32), 0, sext(c as u32), slot, arr_p, idx_p]);
                }
            } else {
                o.call(m, MI2_PRINT_MODEL_TREE, &[dict, e, p, q, sext(a as u32), sext(b as u32), sext(c as u32), slot, 0, 0]);
            }
        }
        off = off.wrapping_add(0x84);
        i += 1;
    }
    if a != 1 {
        let n = m.u32(idx_p);
        m.set_u32(count, n);
    }
    let r = m.u32(arr_p);
    m.release(8);
    m.release(4);
    r
}

/// [soft 0x085968c0 mi2_print_model_tree]: one model, its sons and the group of them. `a == 1` collects the group
/// names in the made array `*arr_p` (the sons') and returns the group's name; otherwise a name is added to the
/// caller's array by `addToRootList`. `b != 0` writes only the empty geometry; a model of kind 0xd or an
/// unprintable kind prints nothing itself (an instance not expanded is reported on stdout, a P-mesh warned).
/// `*out16` is the shadow switch `mi2_print_model` leaves.
pub fn mi2_print_model_tree(
    m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, _q: u32, a: u32, b: u32, c: u32, out16: u32, arr_p: u32, idx_p: u32,
) -> u32 {
    let (a, b) = (a as u16, b as u16);
    let c = c as u16;
    let mut ok = true;
    let mut last16 = 0u16;
    let mut mres = 0u32;
    let mut grp = 0u32;
    if dict == 0 || elem == 0 || m.u16(elem + 0x48) != 0 {
        return 0;
    }
    let md = m.u32(elem + 4);
    if md == 0 {
        return 0;
    }
    let mut arr = 0u32;
    let l = m.local(0x80 + 0x400 + 8);
    let (m80, m40) = (l, l + 0x40);
    let buf = l + 0x80;
    crate::mi_calc::init_matrix(m, m80);
    let pre = if m.u32(INCREMENTAL) != 0 { &b"incremental "[..] } else { &b""[..] };
    let tmp = m.local(16);
    m.put_cstr(tmp, pre);
    let pre_s = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, tmp]).eax;
    m.release(16);
    m.set_u16(out16, 0);
    o.call(m, crate::mi_model::MATRIX_X_MATRIX, &[m40, m.u32(md + 0x44).wrapping_add(0x1c4), mat]);
    let ty = m.u32(md + 0x2ec);
    let mut body = false;
    if ty == 0xf {
        let t = cprintf::format_bytes(m, b"ERROR: Non-expanded instance found: <%s>\n", &[m.u32(md + 4)]);
        o.print(1, &t);
        ok = false;
    } else if ty == 0xd {
        ok = false;
    } else {
        match m.u32(md + 0x30).wrapping_add(1) {
            2 | 3 | 5 | 8 | 11 => body = true,
            0 | 1 | 6 | 7 | 9 | 10 => ok = false,
            4 => {
                o.call(m, crate::mi_calc::SWARNING, &[0x086f_4c40]);
                ok = false;
            }
            _ => {}
        }
    }
    if body {
        let go = if a == 1 {
            true
        } else {
            let st = m.u32(elem + 0x24);
            st == 1 || st == 3 || st == 2
        };
        if go && (m.u16(SI_SHW_CTL) != 0 || ty.wrapping_sub(0x12) > 0xb) {
            let name = m.u32(m.u32(elem).wrapping_add(0x10));
            mres = o.call(m, MI2_PRINT_MODEL, &[dict, elem, m40, m80, name, sext(b as u32), sext(c as u32), out16]).eax;
            last16 = m.u16(out16);
        }
        if mres == 0 && a == 1 {
            ok = false;
        }
    }
    let cid = m.u32(CID_MODELS);
    let mut ncnt = o.call(m, crate::mi_poly::RELATIONS_COUNT, &[dict, elem, 0, 1, 1, 0x6e, cid]).eax as i32;
    let l16 = last16 as i16 as i32 as u32;
    if ncnt != 0 {
        if b == 0 && a == 1 {
            arr = o.call(m, MEMTRACK_ALLOCATE, &[SI_MEM_PARAMS, (ncnt as u32).wrapping_shl(2)]).eax;
        }
        let mut sidx = 0u32;
        let mut son = o.call(m, crate::mi_model::SI_ELEMDICT_FIND_SON, &[dict, elem]).eax;
        if son != 0 {
            while son != 0 {
                let r = mi2_print_model_tree(m, o, dict, son, mat, m80, a as u32, b as u32, sext(c as u32), out16, arr_p, idx_p);
                if r != 0 && !(arr == 0 && a == 1) {
                    if arr != 0 {
                        m.set_u32(arr.wrapping_add(sidx.wrapping_shl(2)), r);
                    }
                    sidx += 1;
                    if m.u16(out16) != 0 {
                        let t = cprintf::format_bytes(m, b"%s_shadow", &[r]);
                        ncnt += 1;
                        let tmp = m.local(t.len() as u32 + 1);
                        m.put_cstr(tmp, &t);
                        if arr != 0 {
                            arr = o.call(m, MEMTRACK_REALLOC, &[SI_MEM_PARAMS, arr, (ncnt as u32).wrapping_shl(2)]).eax;
                            let s = o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, tmp]).eax;
                            m.set_u32(arr.wrapping_add(sidx.wrapping_shl(2)), s);
                        }
                        m.release(t.len() as u32 + 1);
                        sidx += 1;
                    }
                }
                son = o.call(m, crate::mi_model::SI_ELEMDICT_FIND_NEXT_SON, &[dict, elem]).eax;
            }
            if ncnt as u32 != sidx {
                ncnt = sidx as i32;
            }
        }
        if b == 0 {
            if !ok
                && ((ncnt > 0 && a == 1) || o.call(m, MODEL_HAS_SHADER_REFERENCE, &[dict, elem]).eax as u16 != 0)
            {
                mres = o.call(m, PRINT_EMPTY_GEOMETRY, &[dict, elem]).eax;
            }
            if ok || ncnt > 0 {
                if a == 1 {
                    grp = o.call(m, ext::PRINT_GROUP, &[mres, arr, ncnt as u32, l16]).eax;
                } else if arr_p != 0 && idx_p != 0 {
                    o.call(m, ADD_TO_ROOT_LIST, &[arr_p, idx_p, mres, l16]);
                }
            }
        } else if !ok && o.call(m, MODEL_HAS_SHADER_REFERENCE, &[dict, elem]).eax as u16 != 0 {
            mres = o.call(m, PRINT_EMPTY_GEOMETRY, &[dict, elem]).eax;
        }
    } else if ok {
        if b == 0 {
            if a == 1 {
                grp = o.call(m, ext::PRINT_GROUP, &[mres, 0, 0, l16]).eax;
            } else if arr_p != 0 && idx_p != 0 {
                o.call(m, ADD_TO_ROOT_LIST, &[arr_p, idx_p, mres, l16]);
            }
        }
    } else if o.call(m, MODEL_HAS_SHADER_REFERENCE, &[dict, elem]).eax as u16 != 0 {
        mres = o.call(m, PRINT_EMPTY_GEOMETRY, &[dict, elem]).eax;
    }
    m.free(pre_s);
    if mres != 0 {
        m.free(mres);
    }
    if arr != 0 {
        for k in 0..ncnt {
            let s = m.u32(arr.wrapping_add((k as u32).wrapping_shl(2)));
            m.free(s);
        }
        o.call(m, crate::mi_patch::MEMTRACK_FREE, &[SI_MEM_PARAMS, arr]);
    }
    m.release(0x80 + 0x400 + 8);
    let _ = buf;
    grp
}

const CID_MATERIALS: u32 = 0x0878_167c;
const CID_TEXTURES2D: u32 = 0x0878_16c8;
const CID_TEXTURES3D: u32 = 0x0878_16cc;
const DEBUG_FLAG: u32 = 0x0882_9f0c;
pub const MI2_PRINT_NURBS_GEOMETRY: Routine = Routine::new("mi2_print_nurbs_geometry", 0x0858_a2d4);
pub const MI2_PRINT_PATCH_GEOMETRY: Routine = Routine::new("mi2_print_patch_geometry", 0x0858_b914);

fn err(m: &mut Mem, o: &mut dyn Outside, fmt: &[u8], words: &[u32]) {
    let t = cprintf::format_bytes(m, fmt, words);
    o.print(2, &t);
}

fn cstr_len(m: &Mem, a: u32) -> u32 {
    let mut n = 0;
    while m.u8(a + n) != 0 {
        n += 1;
    }
    n
}

/// [soft 0x0859709c mi2_print_model]: the object of one model (a "_master" object per material class: the
/// plain one, then the shadow one when the model has both kinds of material) and its instance. `b != 0` prints
/// only the instance data; `c != 0` is the shadow run (only the shadow-flagged models). `*out16` is set to 1
/// when both classes of material exist. Returns the instance's name made by `SI_memtrackStrdup`, 0 if none.
pub fn mi2_print_model(
    m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, mat: u32, q: u32, name: u32, b: u32, c: u32, out16: u32,
) -> u32 {
    let (b, c) = (b as u16, c as u16);
    if dict == 0 || elem == 0 {
        return 0;
    }
    let md = m.u32(elem + 4);
    if md == 0 {
        return 0;
    }
    let l = m.local(0x450);
    let (buf, inv, flags, outp, state) = (l, l + 0x400, l + 0x440, l + 0x448, l + 0x44c);
    m.set_u8(buf, 0);
    m.set_u16(out16, 0);
    let mut shadow_run = false;
    if c != 0 {
        shadow_run = m.u32(md + 0x3ee) & 3 != 0;
    }
    let r = (|| -> u32 {
        if (o.call(m, crate::mi_model::SI_MODEL_NB_TRIANGLES, &[md]).eax as i32) <= 0 {
            return 0;
        }
        let ef = m.u8(elem + 0x58) as i8 as i32 as u32;
        let cid_mat = m.u32(CID_MATERIALS);
        let nmat = o.call(m, crate::mi_poly::RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x12c, cid_mat]).eax;
        let mut nmat2 = nmat;
        if nmat != 0 && m.u32(md + 0x30) != 4 {
            nmat2 = 1;
        }
        if m.u32(DEBUG_FLAG) != 0 {
            let first = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, outp, state]).eax;
            if nmat != 0 {
                let cid2 = m.u32(CID_TEXTURES2D);
                let n2 = o.call(m, crate::mi_poly::RELATIONS_COUNT, &[dict, elem, 1, 3, 0, 0x190, cid2]).eax;
                let ior = if first != 0 { m.f32(first + 0x6dc) as f64 } else { 1.0 };
                let bits = ior.to_bits();
                o.call(m, crate::mi_calc::SDEBUG, &[0x086f_4c83, nmat, bits as u32, (bits >> 32) as u32, n2]);
            } else {
                o.call(m, crate::mi_calc::SDEBUG, &[0x086f_4c9c]);
            }
            while m.u32(outp) != 0 {
                let cid2 = m.u32(CID_TEXTURES2D);
                let n = o.call(m, crate::mi_poly::RELATIONS_COUNT, &[dict, m.u32(outp), 1, 3, 0, 0x191, cid2]).eax;
                err(m, o, b"+%d", &[n]);
                o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, outp, state]);
            }
            let cid3 = m.u32(CID_TEXTURES3D);
            let chk = o.call(m, crate::mi_patch::RELATION_CHECK, &[dict, elem, 1, 0x1f4, cid3]).eax as u16;
            err(m, o, b" 2D maps, %d", &[(chk == 0) as u32]);
            o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, outp, state]);
            while m.u32(outp) != 0 {
                let cid3 = m.u32(CID_TEXTURES3D);
                let chk = o.call(m, crate::mi_patch::RELATION_CHECK, &[dict, m.u32(outp), 1, 0x1f5, cid3]).eax as u16;
                err(m, o, b"+%d", &[(chk == 0) as u32]);
                o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, outp, state]);
            }
            err(m, o, b" 3D maps\n", &[]);
        }
        if b == 0 {
            m.set_u32(flags, (nmat2 == 0) as u32);
            m.set_u32(flags + 4, 0);
            if nmat2 == 0 {
                o.call(m, crate::mi_model::PRINT_DEFAULT_MATERIAL, &[dict, elem]);
            }
            let mut e = o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, 0, state]).eax;
            while e != 0 {
                if m.u16(e + 0x1e) == 6 {
                    m.set_u32(flags + 4, 1);
                } else {
                    m.set_u32(flags, 1);
                }
                e = o.call(m, ext::SI_ELEMDICT_GET_NEXT_RELATED_ELEM, &[dict, elem, 1, 3, 0x12c, cid_mat, 0, state]).eax;
            }
            if m.u32(flags) == 1 && m.u32(flags + 4) == 1 {
                m.set_u16(out16, 1);
            }
        } else {
            m.set_u32(flags, 1);
            m.set_u32(flags + 4, 0);
        }
        for pass in 0..2u32 {
            if m.u32(flags + 4 * pass) != 0 {
                if b == 0 {
                    let mdname = m.u32(md + 4);
                    let slen = cstr_len(m, mdname);
                    let is_master = slen > 7 && m.bytes(mdname + slen - 7, 8) == b"_master\0";
                    let e = o.call(m, SI_EMPTY_STRING, &[name]).eax as u16;
                    let who = if e != 0 { Vec::new() } else { m.cstr(name) };
                    let mut t = Vec::new();
                    t.extend(b"MOD_");
                    t.extend(who);
                    t.push(b'_');
                    t.extend(m.cstr(mdname));
                    if is_master && pass == 0 {
                        t.push(b'_');
                    }
                    if pass != 0 {
                        t.extend(b"_shadow");
                    }
                    m.put_cstr(buf, &t);
                }
                if m.u32(INCREMENTAL) == 0 || ef & 2 != 0 || shadow_run {
                    let objname = buf;
                    if b == 0 {
                        let f = if m.u32(INCREMENTAL) != 0 { 0x086f_4d20 } else { 0x086f_4d3f };
                        fp(m, o, &[f, objname]);
                        let fl = m.u32(md + 0x3ec);
                        if fl & 1 != 0 && pass == 0 {
                            fp(m, o, &[0x086f_4d52]);
                        }
                        if fl & 4 != 0 || pass != 0 {
                            fp(m, o, &[0x086f_4d5b]);
                        }
                        if fl & 2 != 0 && pass == 0 {
                            fp(m, o, &[0x086f_4d63]);
                        }
                        let ca = m.u32(md + 0x404);
                        if ca & 1 == 0 && ca & 2 == 0 {
                            fp(m, o, &[0x086f_4d6a]);
                        } else {
                            fp(m, o, &[0x086f_4d77, sext(ca)]);
                        }
                        let gi = m.u32(md + 0x406);
                        if gi & 1 == 0 && gi & 2 == 0 {
                            fp(m, o, &[0x086f_4d83]);
                        } else {
                            fp(m, o, &[0x086f_4d92, sext(gi)]);
                        }
                        let cid = m.u32(CID_MODELS);
                        let id = o.call(m, crate::mi_model::SI_ELEMDICT_GET_ELEM_ID, &[dict, cid, elem]).eax;
                        fp(m, o, &[0x086f_4da0, id.wrapping_add(1)]);
                    }
                    let kind = m.u32(md + 0x30);
                    match kind.wrapping_sub(1) {
                        3 => {
                            o.call(m, crate::mi_model::PRINT_MESH_GEOMETRY, &[dict, elem, mat, q, pass, sext(b as u32), sext(c as u32)]);
                        }
                        9 => {
                            o.call(m, MI2_PRINT_NURBS_GEOMETRY, &[dict, elem, mat, sext(b as u32), sext(c as u32)]);
                        }
                        0 => {
                            o.call(m, MI2_PRINT_PATCH_GEOMETRY, &[dict, elem, mat, sext(b as u32), sext(c as u32)]);
                        }
                        1 | 6 => {
                            o.call(m, crate::mi_model::PRINT_TRIANGLES, &[dict, elem, mat, q, sext(b as u32), sext(c as u32)]);
                        }
                        _ => {
                            o.call(m, crate::mi_calc::SWARNING, &[0x086f_4da9, kind]);
                        }
                    }
                    if b == 0 {
                        fp(m, o, &[0x086f_4dc5]);
                    }
                }
                if m.u32(INCREMENTAL) == 0 || ef & 1 != 0 || shadow_run {
                    o.call(m, INVERT_MATRIX_R, &[inv, m.u32(md + 0x44).wrapping_add(0x1c4)]);
                    if b == 0 {
                        let is_root = (m.u32(elem + 0x20) == 0) as u32;
                        o.call(m, ext::PRINT_INSTANCE, &[buf, is_root, inv]);
                    }
                }
            }
        }
        if m.u16(out16) == 1 {
            let e = o.call(m, SI_EMPTY_STRING, &[name]).eax as u16;
            let mut t = b"MOD_".to_vec();
            if e == 0 {
                t.extend(m.cstr(name));
            }
            t.push(b'_');
            t.extend(m.cstr(m.u32(md + 4)));
            m.put_cstr(buf, &t);
        }
        if cstr_len(m, buf) != 0 {
            o.call(m, SI_MEMTRACK_STRDUP, &[SI_MEM_PARAMS, buf]).eax
        } else {
            0
        }
    })();
    m.release(0x450);
    r
}
