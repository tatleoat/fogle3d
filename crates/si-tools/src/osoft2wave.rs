// SPDX-License-Identifier: MIT
//! `osoft2wave.c` (finding 0039): a model written as a Wavefront `.obj` file (and the `.mtl`
//! file of its materials).
//!
//! Records, read from the listings: the **writer object** (`si_WaveFrontObject`) holds the
//! output stream at +0, the stream position of the header's blank line at +4, the object name
//! at +8, the list of group names at +0x108 (a chain by next at +0x50), the material file at
//! +0x10c, the material library name at +0x110, the name of the material in use at +0x12e,
//! the list of materials written at +0x14c, and the counts of vertices and normals already
//! written at +0x150 and +0x154 (so that the numbers of a later object continue from them). A
//! **model** holds its matrix at +0x1c4 (as in `osoft2vert.rs`), its number of materials as a
//! 16-bit number at +0x206, the current one's index at +0x208, the array of materials at
//! +0x20c and its parent at +0x2bc. A **material** is described in `owave2soft.rs`; here its
//! name is its first 0x1e bytes, the shading model is a 16-bit number at +0x1e, the ambient,
//! diffuse and specular colours are three floats at +0x34, +0x44 and +0x54, the shininess is
//! at +0x64 and the index of refraction at +0x6dc.

use crate::owave2soft::{print_message, ToolsCalls, DEFAULT_WHITE, MAT_NEXT, MAT_SIZE};
use crate::stdio::{dbl, fprintf, StdioCalls};
use si_mem::{clib, Mem, Outside};
use si_x87::{x, F80};

const OBJ_FILE: u32 = 0;
const OBJ_POS: u32 = 4;
const OBJ_NAME: u32 = 8;
const OBJ_GROUPS: u32 = 0x108;
const OBJ_MTL_FILE: u32 = 0x10c;
const OBJ_MTL_LIB: u32 = 0x110;
const OBJ_CUR_MTL: u32 = 0x12e;
const OBJ_MTL_LIST: u32 = 0x14c;
const OBJ_V_BASE: u32 = 0x150;
const OBJ_VN_BASE: u32 = 0x154;
const GROUP_NEXT: u32 = 0x50;

pub const STRINGS: &[(u32, &str)] = &[
    (0x0869_1500, "v %lf %lf %lf\n"),
    (0x0869_150f, "# %ld vertices\n\n"),
    (0x0869_1520, "vn  %lf %lf %lf\n"),
    (0x0869_1531, "Out of memory."),
    (0x0869_1540, "# %ld normales\n\n"),
    (0x0869_1551, "# 0 texture vertices\n\n\n"),
    (0x0869_1569, "g"),
    (0x0869_156b, " %s"),
    (0x0869_156f, "\n"),
    (0x0869_1571, "default_white"),
    (0x0869_157f, "default_white"),
    (0x0869_158d, "default_white"),
    (0x0869_159b, "usemtl %s\n"),
    (0x0869_15a6, "usemtl\n"),
    (0x0869_15ae, "f"),
    (0x0869_15b0, " \\\n"),
    (0x0869_15b4, " %ld//%ld"),
    (0x0869_15be, "\n"),
    (0x0869_15c0, "# %ld elements\n\n"),
    (0x0869_15d1, "v %lf %lf %lf\n"),
    (0x0869_15e0, "# %ld vertices\n\n"),
    (0x0869_15f1, "g"),
    (0x0869_15f3, " %s"),
    (0x0869_15f7, "\n"),
    (0x0869_15f9, "default_white"),
    (0x0869_1607, "default_white"),
    (0x0869_1615, "default_white"),
    (0x0869_1623, "usemtl %s\n"),
    (0x0869_162e, "usemtl\n"),
    (0x0869_1636, "res 4 4\n"),
    (0x0869_163f, "cdp"),
    (0x0869_1643, "bsp"),
    (0x0869_1647, "bzp"),
    (0x0869_164b, "%s"),
    (0x0869_164e, " %d"),
    (0x0869_1652, "\n"),
    (0x0869_1654, "# %ld elements\n\n"),
    (0x0869_1665, "v %lf %lf %lf\n"),
    (0x0869_1674, "# %ld vertices\n\n"),
    (0x0869_1685, "g"),
    (0x0869_1687, " %s"),
    (0x0869_168b, "\n"),
    (0x0869_168d, "cdc"),
    (0x0869_1691, "l"),
    (0x0869_1693, " \\\n"),
    (0x0869_1697, " %d"),
    (0x0869_169b, " %d %d %d"),
    (0x0869_16a5, "\n"),
    (0x0869_16a7, "# 1 element\n\n"),
    (0x0869_16c0, "# (c) Copyright 1992 Softimage Inc.\n"),
    (0x0869_16e5, "# %s \n\n"),
    (0x0869_16ed, "o %s\n\n"),
    (
        0x0869_1700,
        "                                                                              \n\n",
    ),
    (0x0869_1751, "mtllib %s"),
    (0x0869_175b, "default_white"),
    (0x0869_1769, "*"),
    (0x0869_176b, "newmtl %s\n"),
    (0x0869_1776, "Ka %.4f %.4f %.4f\n"),
    (0x0869_1789, "Kd %.4f %.4f %.4f\n"),
    (0x0869_179c, "Ks %.4f %.4f %.4f\n"),
    (0x0869_17af, "Ns %.4f\n"),
    (0x0869_17b8, "Ni %.4f\n"),
    (0x0869_17c1, "illum 0\n\n\n"),
    (0x0869_17cc, "illum 1\n\n\n"),
    (0x0869_17d7, "illum 2\n\n\n"),
];

const OOM: u32 = 0x0869_1531;
const F_V: u32 = 0x0869_1500;
const F_NVERTS: u32 = 0x0869_150f;
const F_VN: u32 = 0x0869_1520;
const F_NNORM: u32 = 0x0869_1540;
const F_NTEX: u32 = 0x0869_1551;
const F_G: u32 = 0x0869_1569;
const F_GNAME: u32 = 0x0869_156b;
const F_NL: u32 = 0x0869_156f;
const DW_MESH: u32 = 0x0869_1571;
const F_USEMTL: u32 = 0x0869_159b;
const F_USEMTL_NONE: u32 = 0x0869_15a6;
const F_F: u32 = 0x0869_15ae;
const F_CONT: u32 = 0x0869_15b0;
const F_VN_REF: u32 = 0x0869_15b4;
const F_NL2: u32 = 0x0869_15be;
const F_NELEM: u32 = 0x0869_15c0;
const DW_PATCH: u32 = 0x0869_15f9;
const F_RES: u32 = 0x0869_1636;
const S_CDP: u32 = 0x0869_163f;
const S_BSP: u32 = 0x0869_1643;
const S_BZP: u32 = 0x0869_1647;
const F_S: u32 = 0x0869_164b;
const F_D: u32 = 0x0869_164e;
const S_CDC: u32 = 0x0869_168d;
const S_L: u32 = 0x0869_1691;
const F_D3: u32 = 0x0869_169b;
const F_ONE_ELEM: u32 = 0x0869_16a7;
const F_COPYRIGHT: u32 = 0x0869_16c0;
const F_CTIME: u32 = 0x0869_16e5;
const F_OBJ: u32 = 0x0869_16ed;
const F_PAD: u32 = 0x0869_1700;
const F_MTLLIB: u32 = 0x0869_1751;
const DW_CHECK: u32 = 0x0869_175b;
const STAR: u32 = 0x0869_1769;
const F_NEWMTL: u32 = 0x0869_176b;
const F_KA: u32 = 0x0869_1776;
const F_KD: u32 = 0x0869_1789;
const F_KS: u32 = 0x0869_179c;
const F_NS: u32 = 0x0869_17af;
const F_NI: u32 = 0x0869_17b8;
const F_ILLUM0: u32 = 0x0869_17c1;
const F_ILLUM1: u32 = 0x0869_17cc;
const F_ILLUM2: u32 = 0x0869_17d7;

/// The first 14 bytes of `s` equal "default_white" with its terminator (the inlined `repz
/// cmpsb` of the unit): only that name itself.
fn is_default_white(m: &Mem, s: u32) -> bool {
    (0..14).all(|i| m.u8(s.wrapping_add(i)) == DEFAULT_WHITE[i as usize])
}

/// [soft 0x081c19a4 copyMtl]: the material `src` copied over `dst` (name, shading model, the
/// three colour groups with their fourth floats, shininess and index of refraction); `dst`'s
/// next is cleared. Nothing happens for a null pointer.
pub fn copy_mtl(m: &mut Mem, dst: u32, src: u32) {
    if dst == 0 || src == 0 {
        return;
    }
    clib::strcpy(m, dst, src);
    let shade = m.u16(src + 0x1e);
    m.set_u16(dst + 0x1e, shade);
    for off in (0x34..=0x60).step_by(4) {
        let w = m.u32(src + off);
        m.set_u32(dst + off, w);
    }
    // the two last go through the x87 stack: a signalling NaN comes out quiet
    for off in [0x64u32, 0x6dc] {
        let mut w = m.u32(src + off);
        if (w & 0x7f80_0000) == 0x7f80_0000 && (w & 0x007f_ffff) != 0 {
            w |= 0x0040_0000;
        }
        m.set_u32(dst + off, w);
    }
    m.set_u32(dst + MAT_NEXT, 0);
}

fn differs(m: &Mem, a: u32, b: u32) -> bool {
    // `!(a == b)` of the floats: a NaN differs from everything
    m.f32(a) != m.f32(b)
}

/// [soft 0x081c1a3c checkMtl]: is the material `mtl` new to the list at `*plist`? "default_white"
/// answers 0. A material of the same name and the same colours, shininess and index answers 0;
/// of the same name and other values it gets a "*" appended to its name and is looked up again
/// (2 if that found it new or not, see the code: 2 when the second look-up answers non-zero,
/// the new name having been appended there). A new name is put at the end of the list: 1.
pub fn check_mtl(m: &mut Mem, mtl: u32, plist: u32) -> i32 {
    let mut result = 0;
    let mut p = 0u32;
    if !is_default_white(m, mtl) {
        p = m.u32(plist);
        if p != 0 {
            loop {
                if clib::strcmp(m, mtl, p) == 0 {
                    let mut diff = false;
                    for off in [0x34u32, 0x38, 0x3c, 0x44, 0x48, 0x4c, 0x54, 0x58, 0x5c, 0x64, 0x6dc] {
                        if differs(m, mtl + off, p + off) {
                            diff = true;
                        }
                    }
                    if diff {
                        clib::strcat(m, mtl, STAR);
                        result = check_mtl(m, mtl, plist);
                        if result != 0 {
                            result = 2;
                        }
                    }
                    break;
                }
                p = m.u32(p + MAT_NEXT);
                if p == 0 {
                    break;
                }
            }
        }
        if p == 0 {
            result = 1;
        }
    }
    if result == 1 {
        let head = m.u32(plist);
        if head == 0 {
            m.set_u32(plist, mtl);
        } else {
            let mut last = head;
            while m.u32(last + MAT_NEXT) != 0 {
                last = m.u32(last + MAT_NEXT);
            }
            m.set_u32(last + MAT_NEXT, mtl);
        }
    }
    result
}

fn f64_of(m: &Mem, a: u32) -> [u32; 2] {
    dbl(m.f32(a) as f64)
}

/// [soft 0x081c1c38 writeMaterial]: the material as the lines of a `.mtl` file: the name,
/// Ka, Kd, Ks (four decimals), Ns, Ni and the illumination model (0 for shading model 1, 1
/// for 3, else 2).
pub fn write_material(m: &mut Mem, o: &mut dyn Outside, file: u32, mtl: u32) {
    fprintf(m, o, file, F_NEWMTL, &[mtl]);
    for (fmt, base) in [(F_KA, 0x34u32), (F_KD, 0x44), (F_KS, 0x54)] {
        let mut args = Vec::new();
        for k in 0..3 {
            args.extend_from_slice(&f64_of(m, mtl + base + 4 * k));
        }
        fprintf(m, o, file, fmt, &args);
    }
    let ns = f64_of(m, mtl + 0x64);
    fprintf(m, o, file, F_NS, &ns);
    let ni = f64_of(m, mtl + 0x6dc);
    fprintf(m, o, file, F_NI, &ni);
    let shade = m.u16(mtl + 0x1e);
    let fmt = if shade == 1 {
        F_ILLUM0
    } else if shade == 3 {
        F_ILLUM1
    } else {
        F_ILLUM2
    };
    fprintf(m, o, file, fmt, &[]);
}

/// [soft 0x081c110c si_checkMeshTriangulate]: 1 if some polygon has a hole (an entry with
/// the vertex number -1), else 0.
pub fn si_check_mesh_triangulate(m: &Mem, mesh: u32) -> u32 {
    let np = m.i32(mesh + 0x18);
    let mut i = 0i32;
    let mut off = 0u32;
    while i < np {
        let n = m.u16(m.u32(mesh + 4).wrapping_add(off).wrapping_add(0x10)) as u32;
        if n != 0 {
            let mut e = m.u32(m.u32(mesh + 4).wrapping_add(off).wrapping_add(0xc));
            let mut j = 0u32;
            while (j as i32) < n as i32 {
                if m.i32(e) == -1 {
                    return 1;
                }
                e = e.wrapping_add(0x24);
                j += 1;
            }
        }
        off = off.wrapping_add(0x18);
        i += 1;
    }
    0
}

/// [soft 0x081c1894 si_putWavefrontHeader]: mode 1 writes the copyright line, the time (the
/// `ctime` text with its last character, the newline, overwritten by a terminator), the
/// object name and a line of 78 blanks, and keeps the position of that line in the object;
/// mode 2 (with a material list) goes back to it and writes the material library name.
pub fn si_put_wavefront_header(m: &mut Mem, o: &mut dyn Outside, obj: u32, mode: i32) {
    let file = m.u32(obj + OBJ_FILE);
    if mode != 1 {
        if mode != 2 || m.u32(obj + OBJ_MTL_LIST) == 0 {
            return;
        }
        let pos = m.i32(obj + OBJ_POS);
        o.fseek(m, file, pos, 0);
        fprintf(m, o, file, F_MTLLIB, &[obj + OBJ_MTL_LIB]);
        return;
    }
    fprintf(m, o, file, F_COPYRIGHT, &[]);
    let t = m.local(4);
    o.time(m, t);
    let text = o.ctime(m, t);
    m.release(4);
    let len = clib::strlen(m, text);
    m.set_u8(text.wrapping_add(len).wrapping_sub(1), 0);
    fprintf(m, o, file, F_CTIME, &[text]);
    fprintf(m, o, file, F_OBJ, &[obj + OBJ_NAME]);
    let pos = o.ftell(m, file);
    m.set_u32(obj + OBJ_POS, pos as u32);
    fprintf(m, o, file, F_PAD, &[]);
}

/// The three coordinates of a point through the model's matrix, each as the unit's 80-bit sum;
/// `p` is the address of the point's three floats.
fn transform(m: &Mem, model: u32, p: u32) -> [F80; 3] {
    let (v0, v1, v2) = (x(m.f32(p)), x(m.f32(p + 4)), x(m.f32(p + 8)));
    let mf = |off: u32| m.f32(model + off);
    let t1 = ((v1 * mf(0x1d4) + v0 * mf(0x1c4)) + v2 * mf(0x1e4)) + mf(0x1f4);
    let t2 = ((v0 * mf(0x1c8) + v1 * mf(0x1d8)) + v2 * mf(0x1e8)) + mf(0x1f8);
    let t3 = (v2 * mf(0x1ec) + (v1 * mf(0x1dc) + v0 * mf(0x1cc))) + mf(0x1fc);
    [t1, t2, t3]
}

fn put_vertex(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt: u32, model: u32, p: u32) {
    let t = transform(m, model, p);
    let mut args = Vec::new();
    for v in t {
        args.extend_from_slice(&dbl(v.to_f64()));
    }
    fprintf(m, o, file, fmt, &args);
}

/// The line of group names: "g", each name after a blank, a newline; nothing without groups.
fn put_groups(m: &mut Mem, o: &mut dyn Outside, obj: u32, file: u32, g_fmt: u32, name_fmt: u32, nl_fmt: u32) {
    if m.u32(obj + OBJ_GROUPS) != 0 {
        fprintf(m, o, file, g_fmt, &[]);
        let mut g = m.u32(obj + OBJ_GROUPS);
        while g != 0 {
            fprintf(m, o, file, name_fmt, &[g]);
            g = m.u32(g + GROUP_NEXT);
        }
        fprintf(m, o, file, nl_fmt, &[]);
    }
}

/// The "usemtl" line for the material `mtl` (already looked up as `fresh`): the material file
/// gets its record when new, and the line follows when the name changes.
fn put_material(m: &mut Mem, o: &mut dyn Outside, obj: u32, file: u32, mtl: u32, fresh: i32, f_usemtl: u32, f_none: u32) {
    let cur = obj + OBJ_CUR_MTL;
    let mtl_file = m.u32(obj + OBJ_MTL_FILE);
    if mtl_file != 0 && fresh != 0 {
        write_material(m, o, mtl_file, mtl);
    }
    if clib::strcmp(m, mtl, cur) != 0 && (!is_default_white(m, mtl) || m.u8(cur) != 0) {
        clib::strcpy(m, cur, mtl);
        if is_default_white(m, cur) {
            fprintf(m, o, file, f_none, &[]);
        } else {
            fprintf(m, o, file, f_usemtl, &[cur]);
        }
    }
    if fresh == 0 && mtl != 0 {
        m.free(mtl);
    }
}

/// [soft 0x081c0a70 si_writeWavefrontMesh]: the vertices through the model's matrix, the normals
/// (each one written when it differs from the one before), the group names, and per polygon the
/// material lines and the face with "vertex//normal" numbers (a line continued after each 20).
pub fn si_write_wavefront_mesh(m: &mut Mem, o: &mut dyn Outside, model: u32, mesh: u32, obj: u32) {
    if mesh == 0 {
        o.si_max_nb_pol_warning(m);
        return;
    }
    let file = m.u32(obj + OBJ_FILE);
    let nv = m.i32(mesh + 0x14);
    let mut count = 0i32;
    let mut vp = 0u32;
    while count < nv {
        let p = m.u32(mesh).wrapping_add(vp);
        put_vertex(m, o, file, F_V, model, p);
        count += 1;
        vp = vp.wrapping_add(0x14);
    }
    fprintf(m, o, file, F_NVERTS, &[count as u32]);

    let (mut lx, mut ly, mut lz) = (0.0f32, 0.0f32, 0.0f32);
    let mut normals_written = 0i32;
    let mut stored = 0i32; // entries of the number table
    let mut grown = 0i32; // 1000-entry steps the table has grown by
    let mut table = 0u32;
    let np = m.i32(mesh + 0x18);
    let mut i = 0i32;
    while i < np {
        let poly = m.u32(mesh + 4).wrapping_add((i as u32).wrapping_mul(0x18));
        let n = m.u16(poly + 0x10) as i32;
        let mut j = 0i32;
        while j < n {
            let e = m.u32(poly + 0xc).wrapping_add((j as u32).wrapping_mul(0x24));
            let (nx, ny, nz) = (m.f32(e + 8), m.f32(e + 0xc), m.f32(e + 0x10));
            if !(nx == lx && ny == ly && nz == lz) || stored == 0 {
                lx = nx;
                ly = ny;
                lz = nz;
                let mut args = Vec::new();
                for v in [lx, ly, lz] {
                    args.extend_from_slice(&dbl(v as f64));
                }
                fprintf(m, o, file, F_VN, &args);
                normals_written += 1;
            }
            let mut next = table;
            if grown.wrapping_mul(1000) <= stored {
                grown += 1;
                next = m.si_malloc((grown as u32).wrapping_mul(4000));
                if next == 0 {
                    print_message(m, o, 3, OOM, &[]);
                    break;
                }
                for k in 0..stored.max(0) as u32 {
                    let w = m.u32(table.wrapping_add(k * 4));
                    m.set_u32(next.wrapping_add(k * 4), w);
                }
                if table != 0 {
                    m.free(table);
                }
            }
            table = next;
            m.set_u32(table.wrapping_add((stored as u32).wrapping_mul(4)), normals_written as u32);
            stored += 1;
            j += 1;
        }
        i += 1;
    }
    fprintf(m, o, file, F_NNORM, &[normals_written as u32]);
    fprintf(m, o, file, F_NTEX, &[]);
    put_groups(m, o, obj, file, F_G, F_GNAME, F_NL);

    let mut at = 0i32; // position in the number table
    let np = m.i32(mesh + 0x18);
    let mut i = 0i32;
    while i < np {
        let poly = m.u32(mesh + 4).wrapping_add((i as u32).wrapping_mul(0x18));
        let mtl = m.si_malloc(MAT_SIZE);
        if m.i16(model + 0x206) == 0 {
            let mut p = model;
            let mut found = false;
            while p != 0 {
                if m.i16(p + 0x206) != 0 {
                    let arr = m.u32(p + 0x20c);
                    let idx = m.i16(p + 0x208) as i32 as u32;
                    let src = m.u32(arr.wrapping_add(idx.wrapping_mul(4)));
                    copy_mtl(m, mtl, src);
                    found = true;
                    break;
                }
                p = m.u32(p + 0x2bc);
            }
            if !found {
                for k in 0..14 {
                    let b = m.u8(DW_MESH + k);
                    m.set_u8(mtl + k, b);
                }
            }
        } else {
            let arr = m.u32(model + 0x20c);
            let idx = m.i16(poly + 0x12) as i32 as u32;
            let src = m.u32(arr.wrapping_add(idx.wrapping_mul(4)));
            copy_mtl(m, mtl, src);
        }
        let fresh = check_mtl(m, mtl, obj + OBJ_MTL_LIST);
        put_material(m, o, obj, file, mtl, fresh, F_USEMTL, F_USEMTL_NONE);
        fprintf(m, o, file, F_F, &[]);
        let n = m.u16(poly + 0x10) as i32;
        let mut j = 0i32;
        while j < n {
            if j == (j / 20) * 20 {
                fprintf(m, o, file, F_CONT, &[]);
            }
            let e = m.u32(poly + 0xc).wrapping_add((j as u32).wrapping_mul(0x24));
            let v = m.u32(obj + OBJ_V_BASE).wrapping_add(1).wrapping_add(m.u32(e));
            let nn = m.u32(table.wrapping_add((at as u32).wrapping_mul(4))).wrapping_add(m.u32(obj + OBJ_VN_BASE));
            fprintf(m, o, file, F_VN_REF, &[v, nn]);
            at += 1;
            j += 1;
        }
        fprintf(m, o, file, F_NL2, &[]);
        i += 1;
    }
    let done = if np > 0 { np } else { 0 };
    fprintf(m, o, file, F_NELEM, &[done as u32]);
    let b = m.u32(obj + OBJ_V_BASE).wrapping_add(nv as u32);
    m.set_u32(obj + OBJ_V_BASE, b);
    let b = m.u32(obj + OBJ_VN_BASE).wrapping_add(normals_written as u32);
    m.set_u32(obj + OBJ_VN_BASE, b);
    if table != 0 {
        m.free(table);
    }
}

/// [soft 0x081c1170 si_writeWavefrontPatch]: the control points through the model's matrix, the
/// group names, the material lines for the model's first material, and the patch as `cdp`,
/// `bsp` or `bzp` surfaces of 4 by 4 control points, "res 4 4" first.
pub fn si_write_wavefront_patch(m: &mut Mem, o: &mut dyn Outside, model: u32, patch: u32, obj: u32) {
    let file = m.u32(obj + OBJ_FILE);
    let (w, h) = (m.u16(patch + 0xc) as u32, m.u16(patch + 0xe) as u32);
    let total = w.wrapping_mul(h) as i32;
    let mut count = 0i32;
    while count < total {
        let p = m.u32(patch + 0x1a4).wrapping_add((count as u32).wrapping_mul(0x10));
        put_vertex(m, o, file, F_V, model, p);
        count += 1;
    }
    fprintf(m, o, file, 0x0869_15e0, &[count as u32]);
    put_groups(m, o, obj, file, 0x0869_15f1, 0x0869_15f3, 0x0869_15f7);

    let mtl = m.si_malloc(MAT_SIZE);
    if m.i16(model + 0x206) == 0 {
        for k in 0..14 {
            let b = m.u8(DW_PATCH + k);
            m.set_u8(mtl + k, b);
        }
    } else {
        let arr = m.u32(model + 0x20c);
        let src = m.u32(arr);
        copy_mtl(m, mtl, src);
    }
    let fresh = check_mtl(m, mtl, obj + OBJ_MTL_LIST);
    put_material(m, o, obj, file, mtl, fresh, 0x0869_1623, 0x0869_162e);
    fprintf(m, o, file, F_RES, &[]);
    let buf = m.local(12);
    let kind = m.i32(patch + 4);
    let which = match kind {
        3 => Some(S_CDP),
        2 => Some(S_BSP),
        1 => Some(S_BZP),
        _ => None,
    };
    if let Some(fmt) = which {
        crate::stdio::sprintf(m, o, buf, fmt, &[]);
    }
    let nu = if m.i16(patch + 0x10) == 1 { w as i32 } else { (w as i32).wrapping_sub(3) };
    let nv = if m.i16(patch + 0x12) == 1 { h as i32 } else { (h as i32).wrapping_sub(3) };
    let step = if kind == 1 { 3 } else { 1 };
    let mut a = 0i32;
    while a < nu {
        let mut b = 0i32;
        while b < nv {
            fprintf(m, o, file, F_S, &[buf]);
            let mut off = 1i32;
            let mut k = 1i32;
            let mut row = a;
            while k < 17 {
                let next = row + 1;
                let hh = m.u16(patch + 0xe) as i32;
                let mut idx = row.wrapping_mul(hh).wrapping_add(b).wrapping_add(off);
                if next.wrapping_mul(hh) < idx {
                    idx = idx.wrapping_sub(hh);
                }
                let base = m.u32(obj + OBJ_V_BASE) as i32;
                fprintf(m, o, file, F_D, &[idx.wrapping_add(base) as u32]);
                let mut next = next;
                if (k & 3) == 0 {
                    if (m.u16(patch + 0xc) as i32 - 1) < next {
                        next = 0;
                    }
                    off = 0;
                    row = next;
                }
                k += 1;
                off += 1;
            }
            fprintf(m, o, file, 0x0869_1652, &[]);
            b += step;
        }
        a += step;
    }
    fprintf(m, o, file, 0x0869_1654, &[(nu as u32).wrapping_mul(nv as u32)]);
    let b = m.u32(obj + OBJ_V_BASE).wrapping_add(w.wrapping_mul(h));
    m.set_u32(obj + OBJ_V_BASE, b);
    m.release(12);
}

/// [soft 0x081c160c si_writeWavefrontCurve]: the control points (a key is 0x30 bytes, its position
/// at +0xc), the group names, "cdc" for a type 3 curve or "l" for 0, the vertex numbers (a line
/// continued after each 20), a closed curve's first three again, and the element count 1.
pub fn si_write_wavefront_curve(m: &mut Mem, o: &mut dyn Outside, model: u32, spline: u32, obj: u32) {
    let file = m.u32(obj + OBJ_FILE);
    let n = m.u16(spline + 0x10) as i32;
    let mut count = 0i32;
    let mut off = 0u32;
    if n != 0 {
        loop {
            let p = m.u32(spline + 0x14).wrapping_add(off).wrapping_add(0xc);
            put_vertex(m, o, file, 0x0869_1665, model, p);
            off = off.wrapping_add(0x30);
            count += 1;
            if count >= n {
                break;
            }
        }
    }
    fprintf(m, o, file, 0x0869_1674, &[count as u32]);
    put_groups(m, o, obj, file, 0x0869_1685, 0x0869_1687, 0x0869_168b);
    let kind = m.i32(spline + 0xc);
    if kind == 3 {
        fprintf(m, o, file, S_CDC, &[]);
    } else if kind == 0 {
        fprintf(m, o, file, S_L, &[]);
    }
    let mut k = 1i32;
    if n != 0 {
        loop {
            if k == (k / 20) * 20 {
                fprintf(m, o, file, 0x0869_1693, &[]);
            }
            let base = m.u32(obj + OBJ_V_BASE);
            fprintf(m, o, file, 0x0869_1697, &[(k as u32).wrapping_add(base)]);
            k += 1;
            if k > n {
                break;
            }
        }
    }
    if m.i16(spline + 8) == 1 {
        let base = m.u32(obj + OBJ_V_BASE);
        fprintf(m, o, file, F_D3, &[base.wrapping_add(1), base.wrapping_add(2), base.wrapping_add(3)]);
    }
    fprintf(m, o, file, 0x0869_16a5, &[]);
    fprintf(m, o, file, F_ONE_ELEM, &[]);
    let b = m.u32(obj + OBJ_V_BASE).wrapping_add(n as u32);
    m.set_u32(obj + OBJ_V_BASE, b);
}
