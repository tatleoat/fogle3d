// SPDX-License-Identifier: MIT
//! `io.c`: the metaball volume's text file: reading and writing the blobs, groups and edges of the local lists
//! (finding 0012, section 29). The C library's stream calls (`feof`, `_IO_getc`, `ungetc`, `fscanf`, `fprintf`) are
//! the `Outside` world's; the lists and records live in the image at the addresses of `volume.rs`.
//! Class: model (the stream calls are stood in for).

use crate::hooks::*;
use crate::volume::*;
use si_math::types::MatrixD;
use si_math::vector::{vc_mat4x4_extract_model_xform, vc_mat4x4_multiply};
use si_mem::{Mem, Outside, Routine};
use si_x87::x;

si_mem::outside_calls! {
    /// The stream calls of `io.c`.
    pub trait IoCalls {
        /// `feof(stream)` (C library).
        fn feof(stream: u32) -> i32 = "feof", 0x080c_3948;
        /// `_IO_getc(stream)` (C library): the next character, or -1.
        fn getc(stream: u32) -> i32 = "_IO_getc", 0x080c_4ff8;
        /// `ungetc(c, stream)` (C library).
        fn ungetc(c: i32, stream: u32) -> i32 = "ungetc", 0x080c_39a8;
    }
}

const FSCANF: Routine = Routine::new("fscanf", 0x080c_4058);
const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);

/// `vcS_localMaxMbAlloc` and its neighbours (the file's own statics and the local lists).
pub const LOCAL_MAX_MB_ALLOC: u32 = 0x086e_e748;
pub const LOCAL_MAX_GROUP_ALLOC: u32 = 0x086e_e74c;
pub const LOCAL_MBL_L: u32 = 0x086e_e750;
pub const LOCAL_GL_L: u32 = 0x086e_e754;
pub const LOCAL_MBL_C: u32 = 0x086e_e758;
pub const LOCAL_GL_C: u32 = 0x086e_e75c;
pub const LOCAL_MBALL_LIST: u32 = 0x0882_9cec;
pub const LOCAL_GROUP_LIST: u32 = 0x0882_9cf0;
pub const LOCAL_NUM_MBS: u32 = 0x0882_9cf4;
pub const LOCAL_NUM_GROUPS: u32 = 0x0882_9cf8;

/// The format strings and keywords of `io.c` at their addresses (the part of the data segment the file reads).
pub const STRINGS: &[(u32, &str)] = &[
    (0x086e_e760, "end_blob"),
    (0x086e_e769, "blob"),
    (0x086e_e76e, "%f\n"),
    (0x086e_e772, "%f%f%f\n"),
    (0x086e_e77a, "%f%f%f\n"),
    (0x086e_e782, "%f%f%f\n"),
    (0x086e_e78a, "%f%f%f\n"),
    (0x086e_e792, "%f%f%f\n"),
    (0x086e_e79a, "%f%f%f\n"),
    (0x086e_e7a2, "%d"),
    (0x086e_e7a5, "%d"),
    (0x086e_e7a8, "%d"),
    (0x086e_e7ab, "%d"),
    (0x086e_e7ae, "%d\n"),
    (0x086e_e7b2, "%d"),
    (0x086e_e7b5, "%d"),
    (0x086e_e7b8, "begin_blob"),
    (0x086e_e7c3, "begin_edge"),
    (0x086e_e7ce, "begin_group"),
    (0x086e_e7da, "begin_blob\n"),
    (0x086e_e7e6, "blob_%d\n"),
    (0x086e_e7ef, "%lg\n"),
    (0x086e_e7f4, "%lg %lg %lg\n"),
    (0x086e_e801, "%lg %lg %lg\n"),
    (0x086e_e80e, "%lg %lg %lg\n"),
    (0x086e_e81b, "%lg %lg %lg\n"),
    (0x086e_e828, "%lg %lg %lg\n"),
    (0x086e_e835, "\n"),
    (0x086e_e837, "end_blob\n"),
    (0x086e_e841, "begin_group\n"),
    (0x086e_e84e, "%d\n"),
    (0x086e_e852, "%d  "),
    (0x086e_e857, " %d"),
    (0x086e_e85b, "\n"),
    (0x086e_e85d, "\n"),
    (0x086e_e85f, "begin_edge\n"),
    (0x086e_e86b, "%d\n"),
    (0x086e_e86f, "G%d G%d\n"),
];

/// Writes the strings of [`STRINGS`] into an image that does not carry the data segment.
pub fn init_rodata(m: &mut Mem) {
    for &(a, s) in STRINGS {
        for (k, b) in s.bytes().enumerate() {
            m.set_u8(a + k as u32, b);
        }
        m.set_u8(a + s.len() as u32, 0);
    }
}

fn fscanf(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt: u32, ptrs: &[u32]) -> i32 {
    let mut words = vec![file, fmt];
    words.extend_from_slice(ptrs);
    o.call(m, FSCANF, &words).eax as i32
}

fn fprintf(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt: u32, args: &[u32]) {
    let mut words = vec![file, fmt];
    words.extend_from_slice(args);
    o.call(m, FPRINTF, &words);
}

/// The two words of a `double` argument, low first.
fn dw(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

fn read_mat(m: &Mem, a: u32) -> MatrixD {
    let mut r = [[0f64; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f64(a + 32 * i as u32 + 8 * j as u32);
        }
    }
    r
}

fn write_mat(m: &mut Mem, a: u32, v: &MatrixD) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            m.set_f64(a + 32 * i as u32 + 8 * j as u32, *e);
        }
    }
}

/// [soft 0x08579520 VC_fgetstr]: `VC_fgetstr(buf, file)`: the next white-space-delimited word into `buf` (a word
/// that starts with `#` takes the rest of its line too); the length plus one, or 0 at the end of the file.
pub fn fgetstr(m: &mut Mem, o: &mut dyn Outside, buf: u32, file: u32) -> i32 {
    let mut n = 0u32;
    let mut c = 0u8;
    while o.feof(m, file) == 0 {
        c = o.getc(m, file) as u8;
        if c == b' ' || c == b'\n' || c == b'\t' {
            break;
        }
        m.set_u8(buf + n, c);
        n += 1;
    }
    if m.u8(buf) == b'#' && c != b'\n' {
        loop {
            c = o.getc(m, file) as u8;
            if c == b'\n' {
                break;
            }
            m.set_u8(buf + n, c);
            n += 1;
        }
    }
    m.set_u8(buf + n, 0);
    if o.feof(m, file) != 0 {
        0
    } else {
        n as i32 + 1
    }
}

/// [soft 0x085795b4 VC_get_whitespace]: `VC_get_whitespace(file)`: skips blanks, tabs and newlines, then gives the
/// first other character back; answers what `ungetc` answered.
pub fn get_whitespace(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    loop {
        let c = o.getc(m, file);
        if c == 0x20 || c == 0x09 || c == 0x0a {
            continue;
        }
        return o.ungetc(m, c, file);
    }
}

/// The orientation `(pi * v) / 180` of the file's degrees, formed in the register and stored as a `float`.
fn radians(v: f32) -> f32 {
    ((x(std::f64::consts::PI) * x(v)) / x(180.0f64)).to_f32()
}

/// [soft 0x085795ec VC_read_blob_data]: `VC_read_blob_data(file)`: reads the metaballs of a `blob` section up to
/// `end_blob` into new metaballs (matrix from scale, rotation in degrees and translation; strength, colour and
/// exponents) and appends their indices to the local list. 0x10: no memory; else the answer of `VC_createMB`.
pub fn read_blob_data(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    let line = m.local(0x54);
    let fl = m.local(12);
    let (p0, p1, p2) = (fl, fl + 4, fl + 8);
    let id = m.local(4);
    let strength = m.local(8);
    let color = m.local(24);
    let expo = m.local(24);
    let mat_a = m.local(0x80);
    if m.i32(LOCAL_MAX_MB_ALLOC) == 0 {
        let p = m.malloc(0x28);
        m.set_u32(LOCAL_MBALL_LIST, p);
        if p == 0 {
            return 0x10;
        }
        m.set_i32(LOCAL_MAX_MB_ALLOC, 10);
    }
    m.set_i32(LOCAL_NUM_MBS, 0);
    loop {
        if fgetstr(m, o, line, file) == 0 {
            break;
        }
        if si_mem::clib::strncmp(m, line, 0x086e_e760, 7) == 0 {
            break;
        }
        if si_mem::clib::strncmp(m, line, 0x086e_e769, 4) != 0 {
            continue;
        }
        fscanf(m, o, file, 0x086e_e76e, &[p0]);
        let k = m.f32(p0) as f64;
        fscanf(m, o, file, 0x086e_e772, &[p0, p1, p2]);
        let rgb = [m.f32(p0) as f64, m.f32(p1) as f64, m.f32(p2) as f64];
        fscanf(m, o, file, 0x086e_e77a, &[p0, p1, p2]);
        fscanf(m, o, file, 0x086e_e782, &[p0, p1, p2]);
        let trans = [m.f32(p0) as f64, m.f32(p1) as f64, m.f32(p2) as f64];
        fscanf(m, o, file, 0x086e_e78a, &[p0, p1, p2]);
        let mut scale = [[0f64; 4]; 4];
        scale[0][0] = m.f32(p0) as f64;
        scale[1][1] = m.f32(p1) as f64;
        scale[2][2] = m.f32(p2) as f64;
        scale[3][3] = 1.0;
        fscanf(m, o, file, 0x086e_e792, &[p0, p1, p2]);
        let (v0, v1, v2) = (m.f32(p0), m.f32(p1), m.f32(p2));
        let ax = radians(v0);
        let ay = radians(v1);
        let az = radians(v2);
        m.set_f32(p0, ax);
        m.set_f32(p1, ay);
        m.set_f32(p2, az);
        let (ax, ay, az) = (ax as f64, ay as f64, az as f64);
        let (cx, sx) = (si_x87::trig::cos(ax), si_x87::trig::sin(ax));
        let mut rx = [[0f64; 4]; 4];
        rx[0][0] = 1.0;
        rx[1][1] = cx.to_f64();
        rx[1][2] = sx.to_f64();
        rx[2][1] = (-sx).to_f64();
        rx[2][2] = cx.to_f64();
        rx[3][3] = 1.0;
        let (cy, sy) = (si_x87::trig::cos(ay), si_x87::trig::sin(ay));
        let mut ry = [[0f64; 4]; 4];
        ry[0][0] = cy.to_f64();
        ry[0][2] = (-sy).to_f64();
        ry[1][1] = 1.0;
        ry[2][0] = sy.to_f64();
        ry[2][2] = cy.to_f64();
        ry[3][3] = 1.0;
        let (cz, sz) = (si_x87::trig::cos(az), si_x87::trig::sin(az));
        let mut rz = [[0f64; 4]; 4];
        rz[0][0] = cz.to_f64();
        rz[0][1] = sz.to_f64();
        rz[1][0] = (-sz).to_f64();
        rz[1][1] = cz.to_f64();
        rz[2][2] = 1.0;
        rz[3][3] = 1.0;
        let t1 = vc_mat4x4_multiply(&scale, &rx);
        let t2 = vc_mat4x4_multiply(&t1, &ry);
        let mut mat = vc_mat4x4_multiply(&t2, &rz);
        mat[3][0] = trans[0];
        mat[3][1] = trans[1];
        mat[3][2] = trans[2];
        fscanf(m, o, file, 0x086e_e79a, &[p0, p1, p2]);
        for (j, v) in [m.f32(p0), m.f32(p1), m.f32(p2)].into_iter().enumerate() {
            m.set_f64(expo + 8 * j as u32, v as f64);
        }
        write_mat(m, mat_a, &mat);
        let r = create_mb(m, mat_a, id);
        if r != 0 {
            return r;
        }
        let mb = m.i32(id);
        m.set_f64(strength, k);
        set_mb_param(m, mb, 2, strength);
        for (j, v) in rgb.into_iter().enumerate() {
            m.set_f64(color + 8 * j as u32, v);
        }
        set_mb_param(m, mb, 5, color);
        set_mb_param(m, mb, 3, expo);
        let n = m.i32(LOCAL_NUM_MBS);
        let list = m.u32(LOCAL_MBALL_LIST);
        m.set_i32(list.wrapping_add((n as u32).wrapping_mul(4)), mb);
        m.set_i32(LOCAL_NUM_MBS, n.wrapping_add(1));
        if n.wrapping_add(1) == m.i32(LOCAL_MAX_MB_ALLOC) {
            let max = n.wrapping_add(11);
            m.set_i32(LOCAL_MAX_MB_ALLOC, max);
            let p = m.realloc(m.u32(LOCAL_MBALL_LIST), (max as u32).wrapping_mul(4));
            m.set_u32(LOCAL_MBALL_LIST, p);
            if p == 0 {
                return 0x10;
            }
        }
    }
    0
}

/// [soft 0x08579bd8 VC_read_group_data]: `VC_read_group_data(file)`: the group count, then per group its member
/// count and the local metaball numbers: the first metaball names the group, the others are added to it. 0x10: no
/// memory; else the answer of the group routines.
pub fn read_group_data(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    let s = m.local(16);
    let (cnt, first, gid, mb) = (s, s + 4, s + 8, s + 12);
    fscanf(m, o, file, 0x086e_e7a2, &[cnt]);
    let n = m.i32(cnt);
    m.set_i32(VCG_NUM_GROUPS, n);
    if n > m.i32(LOCAL_MAX_GROUP_ALLOC) {
        m.set_i32(LOCAL_MAX_GROUP_ALLOC, n);
        let p = m.realloc(m.u32(LOCAL_GROUP_LIST), (n as u32).wrapping_mul(4));
        m.set_u32(LOCAL_GROUP_LIST, p);
        if p == 0 {
            return 0x10;
        }
    }
    m.set_i32(LOCAL_NUM_GROUPS, m.i32(cnt));
    let mut i = 0i32;
    while i < m.i32(VCG_NUM_GROUPS) {
        fscanf(m, o, file, 0x086e_e7a5, &[cnt]);
        fscanf(m, o, file, 0x086e_e7a8, &[first]);
        let mbi = m.i32(m.u32(LOCAL_MBALL_LIST).wrapping_add((m.i32(first) as u32).wrapping_mul(4)));
        let r = get_mb_group_id(m, mbi, gid);
        let list = m.u32(LOCAL_GROUP_LIST);
        m.set_i32(list.wrapping_add((i as u32).wrapping_mul(4)), m.i32(gid));
        if r != 0 {
            return r;
        }
        let mut k = 1;
        while k < m.i32(cnt) {
            fscanf(m, o, file, 0x086e_e7ab, &[mb]);
            let mbi = m.i32(m.u32(LOCAL_MBALL_LIST).wrapping_add((m.i32(mb) as u32).wrapping_mul(4)));
            let r = add_mb_to_group(m, mbi, m.i32(gid));
            if r != 0 {
                return r;
            }
            k += 1;
        }
        i += 1;
    }
    0
}

/// One end of an edge in the file: a `g`/`G` before the number names a local group, else a local metaball whose
/// group is meant. Gives the group, or the failure of `VC_getMBGroupID`.
fn edge_end(m: &mut Mem, o: &mut dyn Outside, file: u32, fmt: u32, v: u32, out: u32) -> i32 {
    get_whitespace(m, o, file);
    let c = o.getc(m, file);
    let is_group = c == 0x67 || c == 0x47;
    if !is_group {
        o.ungetc(m, c, file);
    }
    fscanf(m, o, file, fmt, &[v]);
    if is_group {
        let g = m.i32(m.u32(LOCAL_GROUP_LIST).wrapping_add((m.i32(v) as u32).wrapping_mul(4)));
        m.set_i32(out, g);
        0
    } else {
        let mbi = m.i32(m.u32(LOCAL_MBALL_LIST).wrapping_add((m.i32(v) as u32).wrapping_mul(4)));
        get_mb_group_id(m, mbi, out)
    }
}

/// [soft 0x08579d0c VC_read_edge_data]: `VC_read_edge_data(file)`: the edge count, then per edge two ends (`G`n a
/// local group, n a local metaball), linked with `VC_createMBLink`. 0 or the failing routine's answer.
pub fn read_edge_data(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    let s = m.local(24);
    let (list, count, n, v, a, b) = (s, s + 4, s + 8, s + 12, s + 16, s + 20);
    list_all_mb_groups(m, list, count);
    fscanf(m, o, file, 0x086e_e7ae, &[n]);
    let total = m.i32(n);
    m.set_i32(n, 0);
    let mut i = 0;
    while i < total {
        let r = edge_end(m, o, file, 0x086e_e7b2, v, a);
        if r != 0 {
            return r;
        }
        let r = edge_end(m, o, file, 0x086e_e7b5, v, b);
        if r != 0 {
            return r;
        }
        let r = create_mb_link(m, m.i32(a), m.i32(b));
        if r != 0 {
            return r;
        }
        i += 1;
        m.set_i32(n, i);
    }
    0
}

/// [soft 0x08579e88 VC_input_data]: `VC_input_data(file)`: reads the sections (`begin_blob`, `begin_edge`,
/// `begin_group`) of a file in turn, skipping other words and comment lines; 0 at the end of the file or the first
/// failing section's answer.
pub fn input_data(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    let line = m.local(0x54);
    loop {
        if fgetstr(m, o, line, file) == 0 {
            return 0;
        }
        if m.u8(line) == b'#' {
            continue;
        }
        let word = m.cstr(line);
        let r = if word == b"begin_blob" {
            read_blob_data(m, o, file)
        } else if word == b"begin_edge" {
            read_edge_data(m, o, file)
        } else if word == b"begin_group" {
            read_group_data(m, o, file)
        } else {
            continue;
        };
        if r != 0 {
            return r;
        }
    }
}

/// [soft 0x08579f2c VC_write_blob_data]: `VC_write_blob_data(file)`: `begin_blob`, then per metaball of the local
/// list `blob_n`, the strength, the first colour vector, translation, scale and rotation (radians) read back from
/// the matrix as given, and the exponents; `end_blob`.
pub fn write_blob_data(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    let mut rot = [0f64; 3];
    let mut trans = [0f64; 3];
    let mut scale = [0f64; 3];
    fprintf(m, o, file, 0x086e_e7da, &[]);
    let mut i = 0i32;
    while i < m.i32(LOCAL_MBL_C) {
        fprintf(m, o, file, 0x086e_e7e6, &[i as u32]);
        let rec = mb_at(m, m.i32(m.u32(LOCAL_MBL_L).wrapping_add((i as u32).wrapping_mul(4))));
        let [a, b] = dw(m.f64(rec + mb::KSCALE));
        fprintf(m, o, file, 0x086e_e7ef, &[a, b]);
        let p = m.u32(rec + mb::COLOR);
        let mut w = Vec::new();
        for k in 0..3 {
            w.extend_from_slice(&dw(m.f64(p + 8 * k)));
        }
        fprintf(m, o, file, 0x086e_e7f4, &w);
        let mat = read_mat(m, rec + mb::MAT_IN);
        vc_mat4x4_extract_model_xform(&mat, &mut trans, &mut scale, &mut rot);
        for (fmt, v) in [(0x086e_e801u32, trans), (0x086e_e80e, scale), (0x086e_e81b, rot)] {
            let w: Vec<u32> = v.iter().flat_map(|e| dw(*e)).collect();
            fprintf(m, o, file, fmt, &w);
        }
        let mut w = Vec::new();
        for k in 0..3 {
            w.extend_from_slice(&dw(m.f64(rec + mb::EXPON + 8 * k)));
        }
        fprintf(m, o, file, 0x086e_e828, &w);
        fprintf(m, o, file, 0x086e_e835, &[]);
        i += 1;
    }
    fprintf(m, o, file, 0x086e_e837, &[]);
}

/// Where `target` stands in the `count` words at `list`: the first match, else `stale` (the caller's last answer).
fn index_in(m: &Mem, list: u32, count: i32, target: i32, stale: i32) -> i32 {
    for p in 0..count.max(0) {
        if m.i32(list.wrapping_add((p as u32).wrapping_mul(4))) == target {
            return p;
        }
    }
    stale
}

/// [soft 0x0857a0f0 VC_write_group_data]: `VC_write_group_data(file)`: `begin_group`, the group count, per group the
/// member count and the members' places in the local metaball list; an empty line.
pub fn write_group_data(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    let s = m.local(8);
    let (list, count) = (s, s + 4);
    fprintf(m, o, file, 0x086e_e841, &[]);
    fprintf(m, o, file, 0x086e_e84e, &[m.u32(LOCAL_GL_C)]);
    let mut last = 0i32;
    let mut i = 0i32;
    while i < m.i32(LOCAL_GL_C) {
        let g = m.i32(m.u32(LOCAL_GL_L).wrapping_add((i as u32).wrapping_mul(4)));
        list_mb_group(m, g, list, count);
        fprintf(m, o, file, 0x086e_e852, &[m.u32(count)]);
        i += 1;
        let mut k = 0i32;
        while k < m.i32(count) {
            let target = m.i32(m.u32(list).wrapping_add((k as u32).wrapping_mul(4)));
            last = index_in(m, m.u32(LOCAL_MBL_L), m.i32(LOCAL_MBL_C), target, last);
            fprintf(m, o, file, 0x086e_e857, &[last as u32]);
            k += 1;
        }
        fprintf(m, o, file, 0x086e_e85b, &[]);
    }
    fprintf(m, o, file, 0x086e_e85d, &[]);
}

/// [soft 0x0857a228 VC_write_edge_data]: `VC_write_edge_data(file)`: `begin_edge`, the link count, and per link the
/// places of its two groups in the local group list as `Gn Gm`.
pub fn write_edge_data(m: &mut Mem, o: &mut dyn Outside, file: u32) {
    let s = m.local(8);
    let (list, count) = (s, s + 4);
    fprintf(m, o, file, 0x086e_e85f, &[]);
    list_all_mb_links(m, list, count);
    fprintf(m, o, file, 0x086e_e86b, &[m.u32(count)]);
    let (mut ia, mut ib) = (0i32, 0i32);
    let mut e = 0i32;
    while e < m.i32(count) {
        let base = m.u32(list).wrapping_add((e as u32).wrapping_mul(8));
        let (a, b) = (m.i32(base), m.i32(base + 4));
        e += 1;
        ia = index_in(m, m.u32(LOCAL_GL_L), m.i32(LOCAL_GL_C), a, ia);
        ib = index_in(m, m.u32(LOCAL_GL_L), m.i32(LOCAL_GL_C), b, ib);
        fprintf(m, o, file, 0x086e_e86f, &[ia as u32, ib as u32]);
    }
}

/// [soft 0x0857a304 VC_output_data]: `VC_output_data(file)`: lists all metaballs and groups into the local lists and
/// writes the three sections. Answers 0.
pub fn output_data(m: &mut Mem, o: &mut dyn Outside, file: u32) -> i32 {
    list_all_mbs(m, LOCAL_MBL_L, LOCAL_MBL_C);
    list_all_mb_groups(m, LOCAL_GL_L, LOCAL_GL_C);
    write_blob_data(m, o, file);
    write_group_data(m, o, file);
    write_edge_data(m, o, file);
    0
}
