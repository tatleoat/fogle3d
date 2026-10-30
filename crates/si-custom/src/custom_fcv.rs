// SPDX-License-Identifier: MIT
//! `custom_fcv.c`: function curves of the custom effects and of the shaders (the data side: class
//! paths, name arrays, offset and scale of the curves); finding 0041, "seventh continuation". The menu
//! builders and the editor hooks of the file are not ported (no menus).

use crate::custom_util::*;
use crate::through_x87;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_FCURVE_OFFSET_AND_SCALE: Routine = Routine::new("SI_fcurveOffsetAndScale", 0x0820_1210);
pub const SI_ELEMDICT_GET_ELEM: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
pub const SI_ELEM_DICTS: u32 = 0x0867_931c;

/// [soft 0x08530978 SI_cusEffectClassGetPathname]: the class path "<Class>/<name>" of the effect whose
/// icon (+0xc) is `icon` (searched in the motion, model, actor, tools and matter records) in a new block
/// stored through `out`; 1 with `*out = 0` when no record has it, 0 for a null argument or out of memory.
pub fn si_cus_effect_class_get_pathname(m: &mut Mem, o: &mut dyn Outside, icon: u32, out: u32) -> i32 {
    let _ = o;
    if icon == 0 || out == 0 {
        return 0;
    }
    m.set_u32(out, 0);
    let mut hit = 0;
    let mut class = 0;
    for (s, text) in [(CUS_MOTION, 0x086d_f2acu32), (CUS_MODEL, 0x086d_f2b3), (CUS_ACTOR, 0x086d_f2b9), (CUS_TOOLS, 0x086d_f2bf), (CUS_MATTER, 0x086d_f2c5)] {
        let e = si_cus_status_get_effect_by_icon(m, s, icon);
        if e != 0 {
            hit = e;
            class = text;
            break;
        }
    }
    if hit == 0 {
        return 1;
    }
    let prefix = clib::strdup(m, class);
    if prefix == 0 {
        return 0;
    }
    let name = m.u32(hit.wrapping_add(4));
    let size = clib::strlen(m, prefix).wrapping_add(clib::strlen(m, name)).wrapping_add(2);
    let p = m.si_malloc(size);
    m.set_u32(out, p);
    if p == 0 {
        m.free(prefix);
        return 0;
    }
    // sprintf(p, "%s%c%s", prefix, '/', name)
    clib::strcpy(m, p, prefix);
    let n = clib::strlen(m, p);
    m.set_u8(p.wrapping_add(n), b'/');
    m.set_u8(p.wrapping_add(n + 1), 0);
    clib::strcat(m, p, name);
    m.free(prefix);
    1
}

/// [soft 0x08530b48 cusFreeStringArray]: frees the `n` strings of the array and clears the slots.
pub fn cus_free_string_array(m: &mut Mem, n: i32, arr: u32) {
    if arr == 0 {
        return;
    }
    let mut i: i32 = 0;
    while i < n {
        let at = arr.wrapping_add((i as u32).wrapping_mul(4));
        let p = m.u32(at);
        if p != 0 {
            m.free(p);
            m.set_u32(at, 0);
        }
        i += 1;
    }
}

/// [soft 0x08530930 cusSearchNameArray]: the index of the first of `n` strings equal to `name`, else -1.
pub fn cus_search_name_array(m: &mut Mem, arr: u32, n: i32, name: u32) -> i32 {
    let mut i: i32 = 0;
    while i < n {
        let s = m.u32(arr.wrapping_add((i as u32).wrapping_mul(4)));
        if clib::strcmp(m, s, name) == 0 {
            return i;
        }
        i += 1;
    }
    -1
}

/// [soft 0x08530b90 cusSortStringArray]: `qsort` of `n` slots of 4 bytes with `strcmp` as the comparator. 1.
pub fn cus_sort_string_array(m: &mut Mem, n: i32, arr: u32) -> i32 {
    // the comparator is `strcmp` itself, called with the addresses of the two slots, so it
    // orders the pointers' own bytes as if they were text (kept)
    clib::qsort(m, arr, n as u32, 4, &mut |m, a, b| clib::strcmp(m, a, b));
    1
}

/// [soft 0x08530e88 cusGetFcurve]: the fcurve (+0x2c) of the value called `name` in the effect's list, else 0.
pub fn cus_get_fcurve(m: &mut Mem, e: u32, name: u32) -> u32 {
    if e == 0 || name == 0 {
        return 0;
    }
    let mut v = m.u32(e.wrapping_add(0x64));
    while v != 0 {
        let n = m.u32(v);
        if clib::strcmp(m, name, n) == 0 {
            return m.u32(v.wrapping_add(0x2c));
        }
        v = m.u32(v.wrapping_add(0x44));
    }
    0
}

/// [soft 0x08530e08 SI_cusFetchCustomValue]: for an icon model (+0x2ec = 0xd) with a custom effect, the
/// value of type 1 called `name` (its fcurve stored through `fc` when given); 0 when none.
pub fn si_cus_fetch_custom_value(m: &mut Mem, model: u32, name: u32, fc: u32) -> u32 {
    if model != 0 && name != 0 && m.u16(model.wrapping_add(0x2ec)) == 0xd {
        let cell = m.local(4);
        let e = si_cus_get_effect_by_icon(m, model, cell);
        m.release(4);
        if e != 0 {
            let mut v = m.u32(e.wrapping_add(0x64));
            while v != 0 {
                let n = m.u32(v);
                if m.u16(v.wrapping_add(4)) == 1 && n != 0 && clib::strcmp(m, n, name) == 0 {
                    if fc != 0 {
                        let f = m.u32(v.wrapping_add(0x2c));
                        m.set_u32(fc, f);
                    }
                    return v;
                }
                v = m.u32(v.wrapping_add(0x44));
            }
        }
    }
    0
}

fn graf_list_nonzero(m: &Mem) -> bool {
    let idx = m.u32(m.u32(SI_PROC_DATA).wrapping_add(4));
    m.u32(SI_GRAF_LISTS).wrapping_add(idx.wrapping_mul(0x24)) != 0
}

/// The body of cusGetOutputShader and cusGetLensShader: the count at `count_at` and the array at
/// `array_at` copied into a new block stored through `out`.
fn get_shader_array(m: &mut Mem, out: u32, count: u32, count_at: u32, array_at: u32) -> i32 {
    if out == 0 || count == 0 || !graf_list_nonzero(m) {
        return 0;
    }
    let n = m.u32(count_at);
    m.set_u32(count, n);
    if n != 0 {
        let p = m.si_malloc(n << 2);
        m.set_u32(out, p);
        if p == 0 {
            return 0;
        }
        let mut i: i32 = 0;
        while i < n as i32 {
            let v = m.u32(m.u32(array_at).wrapping_add((i as u32) << 2));
            m.set_u32(p.wrapping_add((i as u32) << 2), v);
            i += 1;
        }
    }
    1
}

/// [soft 0x0852fa5c cusGetOutputShader]: a copy of the output-shader array (count 0x08781be4, array
/// 0x08781bec) stored through `out`, its count through `count`. 1, or 0 for a null argument or out of memory.
pub fn cus_get_output_shader(m: &mut Mem, out: u32, count: u32) -> i32 {
    get_shader_array(m, out, count, 0x0878_1be4, 0x0878_1bec)
}

/// [soft 0x0852f9d4 cusGetLensShader]: the same for the lens shaders (count 0x08781be8, array 0x08781bf0).
pub fn cus_get_lens_shader(m: &mut Mem, out: u32, count: u32) -> i32 {
    get_shader_array(m, out, count, 0x0878_1be8, 0x0878_1bf0)
}

/// [soft 0x0852fae8 cusGetAtmosphereShader]: the atmosphere shader (0x08781c4c) as a one-entry array
/// stored through `out`, 1 as `*count`; 0 entries when there is none. 1, or 0 for a null argument or out of memory.
pub fn cus_get_atmosphere_shader(m: &mut Mem, out: u32, count: u32) -> i32 {
    if out == 0 || count == 0 || !graf_list_nonzero(m) {
        return 0;
    }
    let s = m.u32(0x0878_1c4c);
    if s == 0 {
        m.set_u32(count, 0);
        return 1;
    }
    m.set_u32(count, 1);
    let p = m.si_malloc(4);
    m.set_u32(out, p);
    if p == 0 {
        return 0;
    }
    m.set_u32(p, s);
    1
}

/// [soft 0x0852f52c cusShaderSearchDesc]: the index of the first of `n` descriptions equal to the one of
/// the shader (the string at +0x1c of the record at +0x24), else -1.
pub fn cus_shader_search_desc(m: &mut Mem, shader: u32, n: i32, descs: u32) -> i32 {
    let mut i: i32 = 0;
    while i < n {
        let a = m.u32(m.u32(shader.wrapping_add(0x24)).wrapping_add(0x1c));
        let b = m.u32(descs.wrapping_add((i as u32).wrapping_mul(4)));
        if clib::strcmp(m, a, b) == 0 {
            return i;
        }
        i += 1;
    }
    -1
}

/// [soft 0x085300f0 fcvDataInit]: `*pp` = a 12-byte record holding two new arrays of `n` words; 0 (and
/// `*pp` cleared or left null) when out of memory, else 1.
pub fn fcv_data_init(m: &mut Mem, pp: u32, n: i32) -> i32 {
    let r = m.si_malloc(12);
    m.set_u32(pp, r);
    if r == 0 {
        return 0;
    }
    let a = m.si_malloc((n as u32) << 2);
    m.set_u32(r, a);
    if a == 0 {
        let r = m.u32(pp);
        if r != 0 {
            m.free(r);
            m.set_u32(pp, 0);
        }
        return 0;
    }
    let b = m.si_malloc((n as u32) << 2);
    m.set_u32(m.u32(pp).wrapping_add(4), b);
    if m.u32(m.u32(pp).wrapping_add(4)) != 0 {
        return 1;
    }
    let first = m.u32(m.u32(pp));
    if first != 0 {
        m.free(first);
        m.set_u32(m.u32(pp), 0);
    }
    let r = m.u32(pp);
    if r != 0 {
        m.free(r);
        m.set_u32(pp, 0);
    }
    0
}

/// [soft 0x085301b4 fcvDataFree]: frees the two arrays and the record.
pub fn fcv_data_free(m: &mut Mem, r: u32) {
    for off in [0u32, 4] {
        let p = m.u32(r.wrapping_add(off));
        if p != 0 {
            m.free(p);
            m.set_u32(r.wrapping_add(off), 0);
        }
    }
    if r != 0 {
        m.free(r);
    }
}

/// [soft 0x085302ec cusFillShaderFcvData]: for each of `n` shaders: the name (first word of the shader
/// record) into the second array and the `k`-th value after the first (+0x44 links) of its value list
/// (+0x2c) into the first; the count (+8). 1, or 0 for a null argument.
pub fn cus_fill_shader_fcv_data(m: &mut Mem, data: u32, shaders: u32, n: i32, k: i32) -> i32 {
    if data == 0 || shaders == 0 {
        return 0;
    }
    let mut i: i32 = 0;
    while i < n {
        let sh = m.u32(shaders.wrapping_add((i as u32) << 2));
        let mut v = m.u32(sh.wrapping_add(0x2c));
        let mut j = k;
        while j > 0 {
            v = m.u32(v.wrapping_add(0x44));
            j -= 1;
        }
        let name = m.u32(sh);
        m.set_u32(m.u32(data.wrapping_add(4)).wrapping_add((i as u32) << 2), name);
        m.set_u32(m.u32(data).wrapping_add((i as u32) << 2), v);
        i += 1;
    }
    m.set_u32(data.wrapping_add(8), n as u32);
    1
}

/// [soft 0x085319d4 cusvalFcurveOffsetAndScale]: for each value of the list with type 1 or 4, offsets and
/// scales its (up to four) fcurves, stopping at the first empty cell.
pub fn cusval_fcurve_offset_and_scale(m: &mut Mem, o: &mut dyn Outside, list: u32, f: [u32; 5]) {
    let mut v = list;
    while v != 0 {
        let t = m.u16(v.wrapping_add(4));
        if (t == 1 || t == 4) && m.u32(v.wrapping_add(0x2c)) != 0 {
            let mut cell = v.wrapping_add(0x2c);
            let mut i = 0;
            loop {
                let fc = m.u32(cell);
                o.call(m, SI_FCURVE_OFFSET_AND_SCALE, &[fc, f[0], f[1], f[2], f[3], f[4]]);
                cell = cell.wrapping_add(4);
                i += 1;
                if i > 3 || m.u32(cell) == 0 {
                    break;
                }
            }
        }
        v = m.u32(v.wrapping_add(0x44));
    }
}

/// [soft 0x08532344 effectListFcurveOffsetAndScale]: cusvalFcurveOffsetAndScale on the value list of
/// each effect of the list (linked by +0xfc).
pub fn effect_list_fcurve_offset_and_scale(m: &mut Mem, o: &mut dyn Outside, list: u32, f: [u32; 5]) {
    let mut e = list;
    while e != 0 {
        let l = m.u32(e.wrapping_add(0x64));
        cusval_fcurve_offset_and_scale(m, o, l, f);
        e = m.u32(e.wrapping_add(0xfc));
    }
}

fn shader_offset_and_scale(m: &mut Mem, o: &mut dyn Outside, rec: u32, off: u32, f: [u32; 5]) {
    if rec == 0 {
        return;
    }
    let u = m.u32(rec.wrapping_add(off));
    if u == 0 {
        return;
    }
    let pd = m.u32(SI_PROC_DATA);
    let k = m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4);
    let dict = k.wrapping_add(m.u32(SI_ELEM_DICTS));
    let (a, b) = (m.u32(u.wrapping_add(4)), m.u32(u.wrapping_add(8)));
    if o.call(m, SI_ELEMDICT_GET_ELEM, &[dict, a, b]).eax == 0 {
        return;
    }
    let l = m.u32(m.u32(rec.wrapping_add(off)).wrapping_add(0x2c));
    let g = f.map(through_x87);
    cusval_fcurve_offset_and_scale(m, o, l, g);
}

/// [soft 0x08532040 t2dShaderOffsetAndScale]: when the 2D texture's shader (+0x198) still exists in the
/// element dictionary, cusvalFcurveOffsetAndScale on its value list (+0x2c).
pub fn t2d_shader_offset_and_scale(m: &mut Mem, o: &mut dyn Outside, t: u32, f: [u32; 5]) {
    shader_offset_and_scale(m, o, t, 0x198, f);
}

/// [soft 0x085320cc t3dShaderOffsetAndScale]: the same for a 3D texture (shader at +0x14c).
pub fn t3d_shader_offset_and_scale(m: &mut Mem, o: &mut dyn Outside, t: u32, f: [u32; 5]) {
    shader_offset_and_scale(m, o, t, 0x14c, f);
}


/// [soft 0x08530204 cusGetShaderByDesc]: collects, in order, the records of the array `arr` (`n` entries)
/// whose shader's name (+0x1c of the shader at +0x24) equals `desc` into a new array stored at `*out`, the
/// count at `*count`; each collected record gets its array elements attached. 1 when the array could be
/// made, 0 for a null argument or no memory.
pub fn cus_get_shader_by_desc(m: &mut Mem, o: &mut dyn Outside, arr: u32, n: i32, desc: u32, out: u32, count: u32) -> i32 {
    if arr == 0 || desc == 0 || out == 0 || count == 0 {
        return 0;
    }
    let name_of = |m: &Mem, i: i32| -> u32 {
        let rec = m.u32(arr.wrapping_add((i as u32).wrapping_mul(4)));
        m.u32(m.u32(rec.wrapping_add(0x24)).wrapping_add(0x1c))
    };
    let mut hits = 0i32;
    let mut i = 0;
    while i < n {
        if clib::strcmp(m, name_of(m, i), desc) == 0 {
            hits += 1;
        }
        i += 1;
    }
    m.set_u32(count, hits as u32);
    let buf = m.si_malloc((hits as u32).wrapping_mul(4));
    m.set_u32(out, buf);
    if buf == 0 {
        return 0;
    }
    let (mut i, mut k) = (0i32, 0i32);
    if n > 0 && hits > 0 {
        loop {
            let rec = m.u32(arr.wrapping_add((i as u32).wrapping_mul(4)));
            o.call(m, crate::custom_misc::SI_UDF_ATTACH_ARRAY_ELEMENTS, &[rec]);
            if clib::strcmp(m, name_of(m, i), desc) == 0 {
                let dst = m.u32(out);
                m.set_u32(dst.wrapping_add((k as u32).wrapping_mul(4)), rec);
                k += 1;
            }
            i += 1;
            if !(i < n && k < hits) {
                break;
            }
        }
    }
    1
}

/// A node of the graf list that counts as a light/shader holder: its record (+4) and its link (+8) are set
/// and its type (+0) is 6, 8, 0x13 or 0x14.
fn graf_node_ok(m: &Mem, n: u32) -> bool {
    if m.u32(n.wrapping_add(8)) == 0 || m.u32(n.wrapping_add(4)) == 0 {
        return false;
    }
    matches!(m.u32(n), 6 | 8 | 0x13 | 0x14)
}

/// [soft 0x0852f8c8 cusGetSelectedLightShader]: the shader records (+0x178 of the records) of the nodes
/// of the current graf list that are lights/cameras-like (type 6, 8, 0x13 or 0x14), counted at `*count` and
/// stored in a new array at `*out`. 0 for a null argument, an empty graf list or no memory, else 1.
pub fn cus_get_selected_light_shader(m: &mut Mem, out: u32, count: u32) -> i32 {
    if out == 0 || count == 0 || !graf_list_nonzero(m) {
        return 0;
    }
    let idx = m.u32(m.u32(SI_PROC_DATA).wrapping_add(4));
    let mut node = m.u32(m.u32(SI_GRAF_LISTS).wrapping_add(4).wrapping_add(idx.wrapping_mul(0x24)));
    let (mut n, mut first) = (0i32, 0u32);
    while node != 0 {
        if graf_node_ok(m, node) {
            if m.u32(m.u32(node.wrapping_add(4)).wrapping_add(0x178)) != 0 {
                n += 1;
            }
            if first == 0 {
                first = node;
            }
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    m.set_u32(count, n as u32);
    if n != 0 {
        let buf = m.si_malloc((n as u32).wrapping_mul(4));
        m.set_u32(out, buf);
        if buf == 0 {
            return 0;
        }
        let (mut i, mut p) = (0i32, first);
        loop {
            if graf_node_ok(m, p) {
                let rec = m.u32(p.wrapping_add(4));
                let s = m.u32(rec.wrapping_add(0x178));
                if s != 0 {
                    let dst = m.u32(out);
                    m.set_u32(dst.wrapping_add((i as u32).wrapping_mul(4)), s);
                }
                i += 1;
            }
            p = m.u32(p.wrapping_add(0x1c));
            if i >= n {
                break;
            }
        }
    }
    1
}

/// The shader slot a selected-model shader query of kind `kind` reads from the model record `rec`: the
/// kinds 0x80a/0x817, 0x80c/0x818, 0x80d/0x819 read a field (+0xdb0, +0xdb4, +0xdb8) of the model's current
/// element, 0x80e/0x81a and 0x810/0x81c follow one more link of it (+0x6f0, +0x6fc) to +0x198 / +0x14c,
/// 0x80f/0x81b and 0x811/0x81d follow a link of the record itself (+0x218, +0x224). `elem` is the current
/// element (null reads as no shader); any other kind has none. 0 is "no shader".
fn model_shader_slot(m: &Mem, rec: u32, elem: u32, kind: u16) -> u32 {
    let follow = |base: u32, link: u32, field: u32| -> u32 {
        if base == 0 {
            return 0;
        }
        let l = m.u32(base.wrapping_add(link));
        if l == 0 {
            0
        } else {
            m.u32(l.wrapping_add(field))
        }
    };
    let direct = |off: u32| if elem == 0 { 0 } else { m.u32(elem.wrapping_add(off)) };
    match kind {
        0x80a | 0x817 => direct(0xdb0),
        0x80c | 0x818 => direct(0xdb4),
        0x80d | 0x819 => direct(0xdb8),
        0x80e | 0x81a => follow(elem, 0x6f0, 0x198),
        0x80f | 0x81b => follow(rec, 0x218, 0x198),
        0x810 | 0x81c => follow(elem, 0x6fc, 0x14c),
        0x811 | 0x81d => follow(rec, 0x224, 0x14c),
        _ => 0,
    }
}

/// [soft 0x0852f578 cusGetSelectedModelShader]: the shaders of one kind (`kind`, 0x80a..0x811 or 0x817..0x81d)
/// held by the models of the current graf list, counted at `*count` and stored in a new array at `*out`
/// (the array is made only when the count is above 0). A node counts when it is set, its link (+8) is not
/// 0 or 2, its record (+4) is set, its type (+0) is 2 or 3 and the record's element table (+0x20c) is set.
/// 0 for a null argument, an empty graf list or no memory, else 1. Not reproduced as in the original:
/// a null link where it reads through the list's end faults there.
pub fn cus_get_selected_model_shader(m: &mut Mem, out: u32, count: u32, kind: u32) -> i32 {
    if out == 0 || count == 0 || !graf_list_nonzero(m) {
        return 0;
    }
    let kind = kind as u16;
    let idx = m.u32(m.u32(SI_PROC_DATA).wrapping_add(4));
    let mut node = m.u32(m.u32(SI_GRAF_LISTS).wrapping_add(4).wrapping_add(idx.wrapping_mul(0x24)));
    let (mut n, mut first) = (0i32, 0u32);
    let node_rec = |m: &Mem, node: u32| -> Option<u32> {
        let link = m.u32(node.wrapping_add(8));
        let rec = m.u32(node.wrapping_add(4));
        if link == 0 || link == 2 || rec == 0 || !matches!(m.u32(node), 2 | 3) {
            None
        } else {
            Some(rec)
        }
    };
    let elem_of = |m: &Mem, rec: u32| -> u32 {
        let tab = m.u32(rec.wrapping_add(0x20c));
        let i = m.i16(rec.wrapping_add(0x208)) as i32 as u32;
        m.u32(tab.wrapping_add(i.wrapping_mul(4)))
    };
    while node != 0 {
        if let Some(rec) = node_rec(m, node) {
            if m.u32(rec.wrapping_add(0x20c)) != 0 {
                let elem = elem_of(m, rec);
                if model_shader_slot(m, rec, elem, kind) != 0 {
                    n += 1;
                }
                if first == 0 && n > 0 {
                    first = node;
                }
            }
        }
        node = m.u32(node.wrapping_add(0x1c));
    }
    m.set_u32(count, n as u32);
    if n != 0 {
        let buf = m.si_malloc((n as u32).wrapping_mul(4));
        m.set_u32(out, buf);
        if buf == 0 {
            return 0;
        }
        let (mut i, mut p) = (0i32, first);
        while i < n {
            if let Some(rec) = node_rec(m, p) {
                let elem = elem_of(m, rec);
                let s = model_shader_slot(m, rec, elem, kind);
                if s != 0 {
                    let dst = m.u32(out);
                    m.set_u32(dst.wrapping_add((i as u32).wrapping_mul(4)), s);
                }
                i += 1;
            }
            p = m.u32(p.wrapping_add(0x1c));
        }
    }
    1
}

// ---------------------------------------------------------------------------------------------------------
// The passes over the element dictionaries (finding 0041, twelfth continuation, unit custom11): the offset
// and scale of the animation of every shader (all of them, or those of the selected objects), and of the
// custom effects. The five words are handed on as they are.

const SI_CID_LIGHTS: u32 = 0x0878_1678;
const SI_CID_MODELS: u32 = 0x0878_1680;
const SI_CID_CAMERAS: u32 = 0x0878_1664;
const SI_CID_CAMERA_SHADERS: u32 = 0x0878_16a8;
/// The classes `SI_shaderFcurveOffsetAndScaleAll` visits: `SI_cid.MATERIAL_SHADERS`, `MATERIAL_PHOTON_SHADERS`,
/// `TEXTURE2D_SHADERS`, `TEXTURE3D_SHADERS`, `VOLUME_SHADERS`, `CAMERA_SHADERS`, `OUTPUT_SHADERS`, `SHADOW_SHADERS`,
/// `ENVIRON_SHADERS`, `LIGHT_SHADERS`, `LIGHT_PHOTON_SHADERS`.
const SHADER_CLASSES: [u32; 11] = [0x0878_1690, 0x0878_16e8, 0x0878_1694, 0x0878_1698, 0x0878_169c, 0x0878_16a8, 0x0878_16ac, 0x0878_16a0, 0x0878_16b0, 0x0878_16a4, 0x0878_16ec];
/// The effect-list head pointers `SI_customFcurveOffsetAndScaleAll` walks (each points to a record whose +0xfc is a list).
const EFFECT_LIST_HEADS: [u32; 5] = [0x0882_2c4c, 0x0882_2dac, 0x0882_298c, 0x0882_2aec, 0x0882_2f0c];

pub(crate) fn elem_dicts(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
}

/// Whether the shader record `u` (words +4 and +8 name it) still exists in the dictionary.
fn shader_exists(m: &mut Mem, o: &mut dyn Outside, dict: u32, u: u32) -> bool {
    let (a, b) = (m.u32(u.wrapping_add(4)), m.u32(u.wrapping_add(8)));
    o.call(m, SI_ELEMDICT_GET_ELEM, &[dict, a, b]).eax != 0
}

/// [soft 0x08531e60 matAndLocTxtShaderOffsetAndScale]: for a material (or light/camera) record, offsets and scales
/// the animation of its four shaders (+0xdb0, +0xdb4, +0xdb8, +0xdbc) that still exist in the dictionary, of its
/// chain of 2D textures (+0x6ec, linked by +0x16c) and of its 3D texture (+0x6fc).
pub fn mat_and_loc_txt_shader_offset_and_scale(m: &mut Mem, o: &mut dyn Outside, rec: u32, f: [u32; 5]) {
    if rec == 0 {
        return;
    }
    for off in [0xdb0u32, 0xdb4, 0xdb8, 0xdbc] {
        let u = m.u32(rec.wrapping_add(off));
        if u != 0 {
            let dict = elem_dicts(m);
            if shader_exists(m, o, dict, u) {
                let l = m.u32(m.u32(rec.wrapping_add(off)).wrapping_add(0x2c));
                cusval_fcurve_offset_and_scale(m, o, l, f);
            }
        }
    }
    let mut t = m.u32(rec.wrapping_add(0x6ec));
    while t != 0 {
        t2d_shader_offset_and_scale(m, o, t, f);
        t = m.u32(t.wrapping_add(0x16c));
    }
    let t3 = m.u32(rec.wrapping_add(0x6fc));
    t3d_shader_offset_and_scale(m, o, t3, f);
}

/// [soft 0x08531a4c SI_shaderFcurveOffsetAndScaleAll]: for every record (not marked deleted: short +0x48 = 2) of
/// the shader classes of the element dictionary, offsets and scales the animation of its value list.
pub fn si_shader_fcurve_offset_and_scale_all(m: &mut Mem, o: &mut dyn Outside, f: [u32; 5]) {
    let dict = elem_dicts(m);
    if dict == 0 {
        return;
    }
    let mut i: i32 = 0;
    while i < m.u32(dict) as i32 {
        let cur = i;
        i += 1;
        if !SHADER_CLASSES.iter().any(|&a| m.u32(a) as i32 == cur) {
            continue;
        }
        let class = (cur as u32).wrapping_shl(6).wrapping_add(m.u32(dict.wrapping_add(4)));
        let mut k: i32 = 0;
        while k < m.u32(class.wrapping_add(4)) as i32 {
            let rec = m.u32(class.wrapping_add(8)).wrapping_add((k as u32).wrapping_mul(0x84));
            let sh = m.u32(rec.wrapping_add(4));
            if sh != 0 && m.u16(rec.wrapping_add(0x48)) != 2 {
                let l = m.u32(sh.wrapping_add(0x2c));
                cusval_fcurve_offset_and_scale(m, o, l, f);
            }
            k += 1;
        }
    }
}

/// [soft 0x08531b54 SI_shaderFcurveOffsetAndScaleSelected]: the same for the shaders of the selected objects: of
/// each light (not deleted, with a node of a kind other than 7 and its two shaders at +0x178 and +0x17c), of each
/// model (its material and texture shaders) and, for the first camera whose node is not of kind 5, all camera
/// shaders (which ends the pass).
pub fn si_shader_fcurve_offset_and_scale_selected(m: &mut Mem, o: &mut dyn Outside, f: [u32; 5]) {
    let dict = elem_dicts(m);
    if dict == 0 {
        return;
    }
    let lights = m.u32(SI_CID_LIGHTS) as i32;
    if lights < m.u32(dict) as i32 {
        let class = (lights as u32).wrapping_shl(6).wrapping_add(m.u32(dict.wrapping_add(4)));
        let mut k: i32 = 0;
        while k < m.u32(class.wrapping_add(4)) as i32 {
            let rec = m.u32(class.wrapping_add(8)).wrapping_add((k as u32).wrapping_mul(0x84));
            let (ob, node) = (m.u32(rec.wrapping_add(4)), m.u32(rec.wrapping_add(8)));
            k += 1;
            if m.u16(rec.wrapping_add(0x48)) == 2 || node == 0 || m.u32(node.wrapping_add(8)) == 0 || m.u32(node) == 7 || ob == 0 {
                continue;
            }
            for off in [0x178u32, 0x17c] {
                let u = m.u32(ob.wrapping_add(off));
                if u != 0 && shader_exists(m, o, dict, u) {
                    let l = m.u32(m.u32(ob.wrapping_add(off)).wrapping_add(0x2c));
                    cusval_fcurve_offset_and_scale(m, o, l, f);
                }
            }
        }
    }
    let models = m.u32(SI_CID_MODELS) as i32;
    if models < m.u32(dict) as i32 {
        let class = (models as u32).wrapping_shl(6).wrapping_add(m.u32(dict.wrapping_add(4)));
        let mut k: i32 = 0;
        while k < m.u32(class.wrapping_add(4)) as i32 {
            let rec = m.u32(class.wrapping_add(8)).wrapping_add((k as u32).wrapping_mul(0x84));
            let (ob, node) = (m.u32(rec.wrapping_add(4)), m.u32(rec.wrapping_add(8)));
            k += 1;
            if m.u16(rec.wrapping_add(0x48)) == 2 || ob == 0 || node == 0 || m.u32(node.wrapping_add(8)) == 0 {
                continue;
            }
            let mut j: i32 = 0;
            while j < m.u16(ob.wrapping_add(0x206)) as i16 as i32 {
                let r = m.u32(m.u32(ob.wrapping_add(0x20c)).wrapping_add((j as u32).wrapping_mul(4)));
                mat_and_loc_txt_shader_offset_and_scale(m, o, r, f);
                j += 1;
            }
            let mut t = m.u32(ob.wrapping_add(0x21c));
            while t != 0 {
                t2d_shader_offset_and_scale(m, o, t, f);
                t = m.u32(t.wrapping_add(0x16c));
            }
            let t3 = m.u32(ob.wrapping_add(0x224));
            t3d_shader_offset_and_scale(m, o, t3, f);
        }
    }
    let cameras = m.u32(SI_CID_CAMERAS) as i32;
    let count = m.u32(dict) as i32;
    if cameras < count {
        let base = m.u32(dict.wrapping_add(4));
        let class = (cameras as u32).wrapping_shl(6).wrapping_add(base);
        let mut k: i32 = 0;
        while k < m.u32(class.wrapping_add(4)) as i32 {
            let rec = m.u32(class.wrapping_add(8)).wrapping_add((k as u32).wrapping_mul(0x84));
            let (ob, node) = (m.u32(rec.wrapping_add(4)), m.u32(rec.wrapping_add(8)));
            k += 1;
            if m.u16(rec.wrapping_add(0x48)) == 2 || node == 0 || m.u32(node.wrapping_add(8)) == 0 || ob == 0 || m.u32(node) == 5 {
                continue;
            }
            let cs = m.u32(SI_CID_CAMERA_SHADERS) as i32;
            if cs >= count {
                continue;
            }
            let sc = (cs as u32).wrapping_shl(6).wrapping_add(base);
            let mut j: i32 = 0;
            while j < m.u32(sc.wrapping_add(4)) as i32 {
                let r2 = m.u32(sc.wrapping_add(8)).wrapping_add((j as u32).wrapping_mul(0x84));
                let sh = m.u32(r2.wrapping_add(4));
                if sh != 0 && m.u16(r2.wrapping_add(0x48)) != 2 {
                    let l = m.u32(sh.wrapping_add(0x2c));
                    cusval_fcurve_offset_and_scale(m, o, l, f);
                }
                j += 1;
            }
            return;
        }
    }
}

/// [soft 0x08532158 SI_customFcurveOffsetAndScaleSelected]: for each model of the dictionary (not deleted, with a
/// node) that is an icon of a custom effect (+0x2ec = 0xd), offsets and scales the fcurves of every value of the
/// effect's list that is not of type 4 (the components of at most four cells, up to the first empty one).
pub fn si_custom_fcurve_offset_and_scale_selected(m: &mut Mem, o: &mut dyn Outside, f: [u32; 5]) {
    let dict = elem_dicts(m);
    let models = m.u32(SI_CID_MODELS) as i32;
    if dict == 0 || m.u32(dict) as i32 <= models {
        return;
    }
    let class = (models as u32).wrapping_shl(6).wrapping_add(m.u32(dict.wrapping_add(4)));
    if class == 0 {
        return;
    }
    let mut k: i32 = 0;
    while k < m.u32(class.wrapping_add(4)) as i32 {
        let rec = m.u32(class.wrapping_add(8)).wrapping_add((k as u32).wrapping_mul(0x84));
        k += 1;
        if rec == 0 || m.u16(rec.wrapping_add(0x48)) == 2 {
            continue;
        }
        let (ob, node) = (m.u32(rec.wrapping_add(4)), m.u32(rec.wrapping_add(8)));
        if ob == 0 || node == 0 || m.u32(node.wrapping_add(8)) == 0 || m.u32(ob.wrapping_add(0x2ec)) != 0xd {
            continue;
        }
        let out = m.local(4);
        let eff = o.call(m, crate::custom_io::SI_CUS_GET_EFFECT_BY_ICON, &[ob, out]).eax;
        m.release(4);
        if eff == 0 {
            continue;
        }
        let mut v = m.u32(eff.wrapping_add(0x64));
        while v != 0 {
            if m.u16(v.wrapping_add(4)) != 4 && m.u32(v.wrapping_add(0x2c)) != 0 {
                let mut cell = v.wrapping_add(0x2c);
                let mut i = 0;
                loop {
                    let fc = m.u32(cell);
                    o.call(m, SI_FCURVE_OFFSET_AND_SCALE, &[fc, f[0], f[1], f[2], f[3], f[4]]);
                    cell = cell.wrapping_add(4);
                    i += 1;
                    if i > 3 || m.u32(cell) == 0 {
                        break;
                    }
                }
            }
            v = m.u32(v.wrapping_add(0x44));
        }
    }
}

/// [soft 0x0853227c SI_customFcurveOffsetAndScaleAll]: effectListFcurveOffsetAndScale on the effect lists (+0xfc)
/// of the five global effect-list records.
pub fn si_custom_fcurve_offset_and_scale_all(m: &mut Mem, o: &mut dyn Outside, f: [u32; 5]) {
    for g in EFFECT_LIST_HEADS {
        let head = m.u32(g);
        let list = m.u32(head.wrapping_add(0xfc));
        effect_list_fcurve_offset_and_scale(m, o, list, f);
    }
}
