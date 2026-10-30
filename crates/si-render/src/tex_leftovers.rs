// SPDX-License-Identifier: MIT
//! The render-path callees that `mat_validation.c`, the parameter code (`edparams.c`) and the
//! texture dialogue's file leave to other source files (finding 0012, section 35, unit tex1):
//! the "mark changed" routines of textures and materials, the area-light display corners, the
//! picture-name routines of `t2d_dial.c`, the 2D map's effect words and the camera aspect.
//!
//! All of them work on the memory image. What they call in other source files is a hook, written
//! `name (0xADDRESS)`: the element dictionary and the `SI_edGetElemFrom...` lookups behind the
//! "dirty" routines, the screen refresh behind the camera aspect, the status line behind the
//! default picture name; the C library's streams and `sprintf` go through the outside world too.

use si_math::algebra::vector_multiply_matrix;
use si_math::types::{Matrix, Vector};
use si_mem::Mem;
use si_x87::{x, F80};

si_mem::outside_calls! {
    /// What the routines of this file call in other source files (and the C library's streams).
    pub trait TexLeftoverCalls {
        /// `SI_edGetElemFromTexture2D (0x080fd3e0)`, `edtools.c` (ported in `si-scene`): (texture, &index), the element.
        fn ed_get_elem_from_texture2d(texture: u32, index: u32) -> u32 = "SI_edGetElemFromTexture2D", 0x080f_d3e0;
        /// `SI_edGetElemFromTexture3D (0x080fd414)`, `edtools.c` (ported in `si-scene`): (texture, &index), the element.
        fn ed_get_elem_from_texture3d(texture: u32, index: u32) -> u32 = "SI_edGetElemFromTexture3D", 0x080f_d414;
        /// `SI_edGetElemFromMaterial (0x080fd288)`, `edtools.c` (ported in `si-scene`): (material, &index), the element.
        fn ed_get_elem_from_material(material: u32, index: u32) -> u32 = "SI_edGetElemFromMaterial", 0x080f_d288;
        /// `SI_elemdictFindRelatedElem (0x0813508c)`, `elemdict.c` (ported in `si-dict`): (dictionary, element, 0, 0,
        /// relation id, chapter, &a, &b), the first related element's record or null.
        fn elemdict_find_related_elem(dict: u32, elem: u32, a: u32, b: u32, id: u32, cid: u32, out_a: u32, out_b: u32) -> u32 = "SI_elemdictFindRelatedElem", 0x0813_508c;
        /// `SI_elemdictFindNextRelatedElem (0x08135154)`: the same arguments, the next one.
        fn elemdict_find_next_related_elem(dict: u32, elem: u32, a: u32, b: u32, id: u32, cid: u32, out_a: u32, out_b: u32) -> u32 = "SI_elemdictFindNextRelatedElem", 0x0813_5154;
        /// `camAspRefresh (0x080d6d2c)`, `sys_camera.c`: redraws the windows that show the camera (display).
        fn cam_asp_refresh(camera: u32) = "camAspRefresh", 0x080d_6d2c;
        /// `SI_statusDisplayMessage (0x083cca94)`: (text, kind), a line on the status bar (display).
        fn status_display_message(text: u32, kind: u32) = "SI_statusDisplayMessage", 0x083c_ca94;
        /// `fopen (0x080c4e18)`: (path, mode), a stream or 0.
        fn fopen(path: u32, mode: u32) -> u32 = "fopen", 0x080c_4e18;
        /// `fgets (0x080c4388)`: (buffer, size, stream), the buffer or 0.
        fn fgets(buf: u32, size: i32, stream: u32) -> u32 = "fgets", 0x080c_4388;
        /// `fclose (0x080c4958)`: (stream).
        fn fclose(stream: u32) -> i32 = "fclose", 0x080c_4958;
        /// `sprintf (0x080c4f58)` with one argument after the format.
        fn sprintf1(buf: u32, fmt: u32, a: u32) -> i32 = "sprintf", 0x080c_4f58;
        /// `sprintf (0x080c4f58)` with two arguments after the format.
        fn sprintf2(buf: u32, fmt: u32, a: u32, b: u32) -> i32 = "sprintf", 0x080c_4f58;
        /// `fprintf (0x080c3b88)` with three arguments after the format.
        fn fprintf3(stream: u32, fmt: u32, a: u32, b: u32, c: u32) -> i32 = "fprintf", 0x080c_3b88;
    }
}

/// `SI_procData` (0x08679314): the pointer to the word that says which dictionary set is current.
pub const SI_PROC_DATA: u32 = 0x0867_9314;
/// `SI_elemDicts` (0x0867931c): the array of element dictionaries, 0x70 bytes each.
pub const SI_ELEM_DICTS: u32 = 0x0867_931c;
/// `SI_cid.MATERIALS` (0x0878167c) and `SI_cid.MODELS` (0x08781680): chapter numbers, read when used.
pub const CID_MATERIALS: u32 = 0x0878_167c;
pub const CID_MODELS: u32 = 0x0878_1680;
/// `SI_lightIcon` (0x087fba88): a pointer to the light symbol's record, whose +0x1c4 holds a matrix.
pub const SI_LIGHT_ICON: u32 = 0x087f_ba88;
/// `SI_fmtAspect` (0x08679540): the picture formats' aspect ratios, floats.
pub const SI_FMT_ASPECT: u32 = 0x0867_9540;
/// `SI_resdir` (0x08781628): the resource directory, a string pointer.
pub const SI_RESDIR: u32 = 0x0878_1628;
/// `stderr` (0x08780c88).
pub const STDERR: u32 = 0x0878_0c88;

/// The binary's texts these routines hand on: `".%d"`, `"%s.%d"`, `"r"`, the unknown-animation message, the source
/// file's path, the "unexpected default" format, `"%s/noIcon"`, the missing-frame message.
pub mod text {
    pub const DOT_D: u32 = 0x086a_450b;
    pub const S_DOT_D: u32 = 0x086a_450f;
    pub const MODE_R: u32 = 0x086a_4515;
    pub const UNKNOWN_ANIMATION: u32 = 0x086a_4517;
    pub const T2D_DIAL_PATH: u32 = 0x086a_4540;
    pub const UNEXPECTED_DEFAULT: u32 = 0x086a_45c0;
    pub const S_NOICON: u32 = 0x086a_45f3;
    pub const NO_FRAME: u32 = 0x086a_4600;
}

/// The current element dictionary: `SI_elemDicts + 0x70 * (*SI_procData)`.
fn current_dict(m: &Mem) -> u32 {
    let proc = m.u32(SI_PROC_DATA);
    m.u32(proc).wrapping_mul(7).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
}

/// [soft 0x082b2488 si_mdlTxtSetDirtyHW]: `(model, flag)`: the model's byte at +0x3fa gets its top bit; with `flag`
/// (16 bits) not 0 the same goes to the first son (+0x2c4) and then each brother (+0x2c0), as long as the one reached
/// has its word at +0x206 zero.
pub fn mdl_txt_set_dirty_hw(m: &mut Mem, model: u32, flag: i32) {
    let b = m.u8(model + 0x3fa);
    m.set_u8(model + 0x3fa, b | 0x80);
    if flag as i16 == 0 {
        return;
    }
    let mut son = m.u32(model + 0x2c4);
    while son != 0 && m.u16(son + 0x206) == 0 {
        mdl_txt_set_dirty_hw(m, son, flag as i16 as i32);
        son = m.u32(son + 0x2c0);
    }
}

/// The related elements of `elem` by relation `id` in chapter `*cid_at`, in the dictionary order the original asks for
/// them: the records' +4 words (the object each stands for), one at a time to `each`.
fn for_each_related(m: &mut Mem, o: &mut dyn si_mem::Outside, elem: u32, id: u32, cid_at: u32, la: u32, lb: u32, each: &mut dyn FnMut(&mut Mem, &mut dyn si_mem::Outside, u32)) {
    let dict = current_dict(m);
    let cid = m.u32(cid_at);
    let mut r = o.elemdict_find_related_elem(m, dict, elem, 0, 0, id, cid, la, lb);
    while r != 0 {
        let obj = m.u32(r + 4);
        each(m, o, obj);
        let dict = current_dict(m);
        let cid = m.u32(cid_at);
        r = o.elemdict_find_next_related_elem(m, dict, elem, 0, 0, id, cid, la, lb);
    }
}

/// [soft 0x08281f9c SI_materialSetDirty]: `(material, flags)`: the material's flag word (+0xdac) becomes 1; each model
/// related to its element by relation 0x12c gets the low 16 bits of `flags` ORed into its update word (+0xa) and is
/// marked for the display ([`mdl_txt_set_dirty_hw`], with 1).
pub fn material_set_dirty(m: &mut Mem, o: &mut dyn si_mem::Outside, material: u32, flags: i32) {
    m.set_u16(material + 0xdac, 1);
    let sp = m.sp();
    let index = m.local(4);
    let elem = o.ed_get_elem_from_material(m, material, index);
    if elem != 0 {
        let (la, lb) = (m.local(4), m.local(4));
        for_each_related(m, o, elem, 0x12c, CID_MODELS, la, lb, &mut |m, _o, mdl| {
            let w = m.u16(mdl + 0xa);
            m.set_u16(mdl + 0xa, w | flags as u16);
            mdl_txt_set_dirty_hw(m, mdl, 1);
        });
    }
    m.set_sp(sp);
}

/// [soft 0x082a8fc0 SI_texture2DSetDirty]: `(texture)`: without an element (`SI_edGetElemFromTexture2D`) nothing
/// happens. Else the map's update byte (+0x194) gets bit 2 and +0x1bc becomes -1; each model related by relation 0x190
/// gets the top bit of its update byte (+0xa) and is marked for the display; each material related by 0x191 is marked
/// ([`material_set_dirty`] with 0x80).
pub fn texture2d_set_dirty(m: &mut Mem, o: &mut dyn si_mem::Outside, texture: u32) {
    let sp = m.sp();
    let index = m.local(4);
    let elem = o.ed_get_elem_from_texture2d(m, texture, index);
    if elem != 0 {
        let b = m.u8(texture + 0x194);
        m.set_u8(texture + 0x194, b | 2);
        m.set_u32(texture + 0x1bc, 0xffff_ffff);
        let (la, lb) = (m.local(4), m.local(4));
        for_each_related(m, o, elem, 0x190, CID_MODELS, la, lb, &mut |m, _o, mdl| {
            let b = m.u8(mdl + 0xa);
            m.set_u8(mdl + 0xa, b | 0x80);
            mdl_txt_set_dirty_hw(m, mdl, 1);
        });
        for_each_related(m, o, elem, 0x191, CID_MATERIALS, la, lb, &mut |m, o, mat| material_set_dirty(m, o, mat, 0x80));
    }
    m.set_sp(sp);
}

/// [soft 0x082b3990 SI_texture3DSetDirty]: `(texture)`: without an element (`SI_edGetElemFromTexture3D`) nothing
/// happens. Else each model related by relation 0x1f4 gets the top bit of its update byte (+0xa) and of the byte at
/// +0x3fa (no son walk); each material related by 0x1f5 is marked ([`material_set_dirty`] with 0x80).
pub fn texture3d_set_dirty(m: &mut Mem, o: &mut dyn si_mem::Outside, texture: u32) {
    let sp = m.sp();
    let index = m.local(4);
    let elem = o.ed_get_elem_from_texture3d(m, texture, index);
    if elem != 0 {
        let (la, lb) = (m.local(4), m.local(4));
        for_each_related(m, o, elem, 0x1f4, CID_MODELS, la, lb, &mut |m, _o, mdl| {
            let b = m.u8(mdl + 0xa);
            m.set_u8(mdl + 0xa, b | 0x80);
            let b = m.u8(mdl + 0x3fa);
            m.set_u8(mdl + 0x3fa, b | 0x80);
        });
        for_each_related(m, o, elem, 0x1f5, CID_MATERIALS, la, lb, &mut |m, o, mat| material_set_dirty(m, o, mat, 0x80));
    }
    m.set_sp(sp);
}

/// [soft 0x082b289c SI_updTxt2dEffects]: `(map)`: the words that say what a 2D map drives, from its factors (the limit
/// is 0.0001, a double): +0x112 ambient (+0x128 above), +0x114 diffuse (+0x12c), +0x116 specular (+0x130), +0x11a
/// transparency (+0x138 beyond either limit, or the black-and-white key +0x18e set), +0x118 reflectivity (+0x134 beyond
/// either limit), +0x120 roughness (+0x140 beyond either limit), +0x124 the method (+0x2a) is 7 or 8. All 16 bits.
pub fn upd_txt2d_effects(m: &mut Mem, map: u32) {
    let hi = x(0.0001f64);
    let lo = x(-0.0001f64);
    let above = |v: f32| u16::from(x(v).gt(hi));
    let beyond = |v: f32| x(v).lt(lo) || x(v).gt(hi);
    let f = |m: &Mem, at: u32| m.f32(map + at);
    let (ambient, diffuse, specular) = (above(f(m, 0x128)), above(f(m, 0x12c)), above(f(m, 0x130)));
    let transp = u16::from(beyond(f(m, 0x138)) || m.u16(map + 0x18e) != 0);
    let reflect = u16::from(beyond(f(m, 0x134)));
    let rough = u16::from(beyond(f(m, 0x140)));
    let reflection_map = u16::from(m.u16(map + 0x2a).wrapping_sub(7) <= 1);
    m.set_u16(map + 0x112, ambient);
    m.set_u16(map + 0x114, diffuse);
    m.set_u16(map + 0x116, specular);
    m.set_u16(map + 0x11a, transp);
    m.set_u16(map + 0x118, reflect);
    m.set_u16(map + 0x120, rough);
    m.set_u16(map + 0x124, reflection_map);
}

/// `SI_vectorMultiplyMatrix(point, matrix, point, 1)` (ported in `si-math`) on the image: the point at `at` (three
/// floats) times the matrix at `mat` (rows of four floats, the first three of each used).
fn transform_point(m: &mut Mem, at: u32, mat: u32) {
    let mut mx: Matrix = [[0.0; 4]; 4];
    for (r, row) in mx.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().take(3).enumerate() {
            *v = m.f32(mat + 16 * r as u32 + 4 * c as u32);
        }
    }
    let p = vector_multiply_matrix(&Vector::new(m.f32(at), m.f32(at + 4), m.f32(at + 8)), &mx);
    m.set_f32(at, p.x);
    m.set_f32(at + 4, p.y);
    m.set_f32(at + 8, p.z);
}

/// Whether the light's corners are moved by its position and not by the symbol's matrix: a light that is not kind 2, or
/// whose word at +0x1c0 is 0.
fn area_uses_position(m: &Mem, light: u32) -> bool {
    m.i16(light + 0x1e) != 2 || m.u16(light + 0x1c0) == 0
}

/// [soft 0x08330450 SI_setAreaRect]: `(light)`: the four corners of a rectangular area light's display (+0x190 ..
/// +0x1bf, three floats each, z 0) are (-a, -b), (-a, b), (a, b), (a, -b) for the half sizes a = +0x1c4, b = +0x1c8,
/// each turned by the matrix at +0x1e0. Then, for a light that is not kind 2 or has +0x1c0 zero, each corner gets the
/// light's position (+0x34, +0x38, +0x3c) added; for a kind 2 light with +0x1c0 set, each is turned again by the
/// matrix at `SI_lightIcon` + 0x1c4.
pub fn set_area_rect(m: &mut Mem, light: u32) {
    for at in [0x198, 0x1a4, 0x1b0, 0x1bc] {
        m.set_u32(light + at, 0);
    }
    let (a, b) = (m.f32(light + 0x1c4), m.f32(light + 0x1c8));
    for (at, v) in [(0x1a0, b), (0x1a8, a), (0x1ac, b), (0x1b4, a), (0x190, -a), (0x194, -b), (0x19c, -a), (0x1b8, -b)] {
        m.set_f32(light + at, v);
    }
    for i in 0..4 {
        transform_point(m, light + 0x190 + 12 * i, light + 0x1e0);
    }
    if area_uses_position(m, light) {
        for i in 0..4 {
            for (k, at) in [0x34u32, 0x38, 0x3c].into_iter().enumerate() {
                let p = light + 0x190 + 12 * i + 4 * k as u32;
                let v = (x(m.f32(p)) + m.f32(light + at)).to_f32();
                m.set_f32(p, v);
            }
        }
    }
    if m.i16(light + 0x1e) == 2 && m.u16(light + 0x1c0) != 0 {
        let icon = m.u32(SI_LIGHT_ICON);
        for i in 0..4 {
            transform_point(m, light + 0x190 + 12 * i, icon.wrapping_add(0x1c4));
        }
    }
}

/// [soft 0x083305a0 SI_setAreaDisk]: `(light)`: the disk's centre (a local origin turned by the matrix at +0x1e0) and
/// its edge point (+0x190: (0, 0, -1) turned likewise), both moved by the light's position (+0x34..) or, for a kind 2
/// light with +0x1c0 set, by the symbol's matrix (`SI_lightIcon` + 0x1c4); the edge point is then made relative to the
/// centre (subtracted, each coordinate stored as a float).
pub fn set_area_disk(m: &mut Mem, light: u32) {
    let sp = m.sp();
    let c = m.local(12);
    for k in 0..3 {
        m.set_u32(c + 4 * k, 0);
    }
    m.set_u32(light + 0x190, 0);
    m.set_u32(light + 0x194, 0);
    m.set_f32(light + 0x198, -1.0);
    transform_point(m, c, light + 0x1e0);
    transform_point(m, light + 0x190, light + 0x1e0);
    if area_uses_position(m, light) {
        for (k, at) in [0x34u32, 0x38, 0x3c].into_iter().enumerate() {
            let v = (x(m.f32(c + 4 * k as u32)) + m.f32(light + at)).to_f32();
            m.set_f32(c + 4 * k as u32, v);
        }
        for (k, at) in [0x34u32, 0x38, 0x3c].into_iter().enumerate() {
            let p = light + 0x190 + 4 * k as u32;
            let v = (x(m.f32(light + at)) + m.f32(p)).to_f32();
            m.set_f32(p, v);
        }
    }
    if m.i16(light + 0x1e) == 2 && m.u16(light + 0x1c0) != 0 {
        let icon = m.u32(SI_LIGHT_ICON);
        transform_point(m, c, icon.wrapping_add(0x1c4));
        transform_point(m, light + 0x190, icon.wrapping_add(0x1c4));
    }
    for k in 0..3 {
        let p = light + 0x190 + 4 * k;
        let v = (x(m.f32(p)) - x(m.f32(c + 4 * k))).to_f32();
        m.set_f32(p, v);
    }
    m.set_sp(sp);
}

/// [soft 0x082b050c isBlank]: `(char)`: 1 for a space, newline, carriage return or tab, else 0 (the answer in `ax`).
pub fn is_blank(c: i32) -> i32 {
    matches!(c as u8, b' ' | b'\n' | b'\r' | b'\t') as i32
}

/// [soft 0x082b0538 SI_stripPicExtension]: `(text)`: the text without its leading and trailing blanks (the trailing ones
/// overwritten by 0; the first character is never cleared) and without a final `.pic` (the dot overwritten); the
/// pointer to the first character kept, or 0 when nothing is left.
pub fn strip_pic_extension(m: &mut Mem, text: u32) -> u32 {
    let mut p = text;
    while m.u8(p) != 0 && is_blank(m.u8(p) as i8 as i32) != 0 {
        p += 1;
    }
    let len = si_mem::clib::strlen(m, p);
    let mut i = len as i32 - 1;
    while i > 0 && is_blank(m.u8(p.wrapping_add(i as u32)) as i8 as i32) != 0 {
        m.set_u8(p.wrapping_add(i as u32), 0);
        i -= 1;
    }
    let dot = si_mem::clib::strrchr(m, p, b'.');
    if dot != 0 && m.cstr(dot) == b".pic" {
        m.set_u8(dot, 0);
    }
    if si_mem::clib::strlen(m, p) == 0 {
        0
    } else {
        p
    }
}

/// [soft 0x082b0480 buildDefaultPicName]: `(map, frame)`: the map's picture name (+0x20) and original name (+0x10)
/// become a copy of `<SI_resdir>/noIcon` (the old +0x10 freed), and the status line says `Cannot find frame %d in script
/// '%s'. Using default picture.` with the frame and the map's script name (+0x14), kind 3.
pub fn build_default_pic_name(m: &mut Mem, o: &mut dyn si_mem::Outside, map: u32, frame: i32) {
    let sp = m.sp();
    let buf = m.local(0x1000);
    let resdir = m.u32(SI_RESDIR);
    o.sprintf1(m, buf, text::S_NOICON, resdir);
    let d = m.strdup(buf);
    m.set_u32(map + 0x20, d);
    let old = m.u32(map + 0x10);
    if old != 0 {
        m.free(old);
        m.set_u32(map + 0x10, 0);
    }
    let d = m.strdup(buf);
    m.set_u32(map + 0x10, d);
    let name = m.u32(map + 0x14);
    o.sprintf2(m, buf, text::NO_FRAME, frame as u32, name);
    o.status_display_message(m, buf, 3);
    m.set_sp(sp);
}

/// [soft 0x082b0200 SI_setFileName]: `(map, frame)`: the picture name of an animated 2D map (+0x20) for a frame: the
/// old name is freed; by the animation kind (+0x28, 16 bits): 0 a copy of the script name (+0x14); 1 the script name's
/// text before its last dot (or all of it) and `.<frame>`; 2 (the name is a script of picture names, a frame > 0)
/// the `frame`-th line of the file, stripped ([`strip_pic_extension`]), kept as the picture name and the original name
/// (+0x10, the old one freed): when the file cannot be opened nothing more happens, when it has fewer lines or the
/// line is empty the default name is built; any other kind is reported on `stderr`.
pub fn set_file_name(m: &mut Mem, o: &mut dyn si_mem::Outside, map: u32, frame: i32) {
    let sp = m.sp();
    let buf = m.local(0x1000);
    let old = m.u32(map + 0x20);
    if old != 0 {
        m.free(old);
        m.set_u32(map + 0x20, 0);
    }
    match m.u16(map + 0x28) {
        0 => {
            let n = m.u32(map + 0x14);
            let d = m.strdup(n);
            m.set_u32(map + 0x20, d);
        }
        1 => {
            let name = m.u32(map + 0x14);
            let dot = si_mem::clib::strrchr(m, name, b'.');
            let shown = if dot != 0 {
                let n = si_mem::clib::strlen(m, name) - si_mem::clib::strlen(m, dot);
                si_mem::clib::strncpy(m, buf, name, n);
                o.sprintf1(m, buf + n, text::DOT_D, frame as u32);
                buf
            } else {
                o.sprintf2(m, buf, text::S_DOT_D, name, frame as u32);
                buf
            };
            let d = m.strdup(shown);
            m.set_u32(map + 0x20, d);
        }
        2 => {
            let mut default_name = frame <= 0;
            if !default_name {
                let path = m.u32(map + 0x14);
                let fp = o.fopen(m, path, text::MODE_R);
                if fp == 0 {
                    m.set_sp(sp);
                    return;
                }
                let mut n = 0;
                while n < frame {
                    if o.fgets(m, buf, 0xfff, fp) == 0 {
                        o.fclose(m, fp);
                        default_name = true;
                        break;
                    }
                    n += 1;
                }
                if !default_name {
                    o.fclose(m, fp);
                    m.set_u8(buf + 0xfff, 0);
                    let s = strip_pic_extension(m, buf);
                    if s == 0 {
                        default_name = true;
                    } else {
                        let d = m.strdup(s);
                        m.set_u32(map + 0x20, d);
                        let old = m.u32(map + 0x10);
                        if old != 0 {
                            m.free(old);
                            m.set_u32(map + 0x10, 0);
                        }
                        let d = m.strdup(s);
                        m.set_u32(map + 0x10, d);
                    }
                }
            }
            if default_name {
                build_default_pic_name(m, o, map, frame);
            }
        }
        _ => {
            let stream = m.u32(STDERR);
            o.fprintf3(m, stream, text::UNEXPECTED_DEFAULT, text::T2D_DIAL_PATH, 0x145b, text::UNKNOWN_ANIMATION);
        }
    }
    m.set_sp(sp);
}

/// [soft 0x080d6cf0 SI_camAspSet]: `(camera, format)`: the camera's aspect (+0x64) becomes the format's aspect ratio
/// (`SI_fmtAspect[format]`) when it differs from it by more than 0.0001 (a double); answers 1 then, else 0.
pub fn cam_asp_set(m: &mut Mem, camera: u32, format: i32) -> i32 {
    let f = m.f32(SI_FMT_ASPECT.wrapping_add((format as u32).wrapping_shl(2)));
    let d = (x(m.f32(camera + 0x64)) - x(f)).abs();
    if d.gt(x(0.0001f64)) {
        m.set_f32(camera + 0x64, F80::from_f32(f).to_f32());
        1
    } else {
        0
    }
}

/// [soft 0x080d6cc0 SI_changeCameraAspect]: `(camera, format)`: [`cam_asp_set`]; when it changed the aspect, the windows
/// showing the camera are redrawn (`camAspRefresh (0x080d6d2c)`, display, a hook).
pub fn change_camera_aspect(m: &mut Mem, o: &mut dyn si_mem::Outside, camera: u32, format: i32) {
    if cam_asp_set(m, camera, format) as i16 != 0 {
        o.cam_asp_refresh(m, camera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::{Ret, Trace};

    #[test]
    fn strip_blanks_and_extension() {
        let mut m = Mem::new();
        let t = m.place_cstr(b"  \tpics/a.pic \n ");
        let r = strip_pic_extension(&mut m, t);
        assert_eq!(m.cstr(r), b"pics/a");
        let t = m.place_cstr(b" \n ");
        assert_eq!(strip_pic_extension(&mut m, t), 0);
        let t = m.place_cstr(b"x.picture");
        let r = strip_pic_extension(&mut m, t);
        assert_eq!(m.cstr(r), b"x.picture");
    }

    #[test]
    fn model_walk_stops_at_a_son_with_materials() {
        let mut m = Mem::new();
        let (a, b, c) = (m.calloc(1, 0x500), m.calloc(1, 0x500), m.calloc(1, 0x500));
        m.set_u32(a + 0x2c4, b);
        m.set_u32(b + 0x2c0, c);
        m.set_u16(c + 0x206, 1);
        mdl_txt_set_dirty_hw(&mut m, a, 1);
        assert_eq!((m.u8(a + 0x3fa), m.u8(b + 0x3fa), m.u8(c + 0x3fa)), (0x80, 0x80, 0));
    }

    #[test]
    fn aspect_is_set_only_when_it_differs() {
        let mut m = Mem::new();
        let cam = m.calloc(1, 0x80);
        m.set_f32(SI_FMT_ASPECT + 8, 1.33);
        m.set_f32(cam + 0x64, 1.33);
        let mut o = Trace::with(|_, _, _| Ret::int(0));
        change_camera_aspect(&mut m, &mut o, cam, 2);
        assert!(o.calls.is_empty());
        m.set_f32(cam + 0x64, 1.0);
        change_camera_aspect(&mut m, &mut o, cam, 2);
        assert_eq!(m.f32(cam + 0x64), 1.33);
        assert_eq!(o.names(), ["camAspRefresh"]);
    }
}
