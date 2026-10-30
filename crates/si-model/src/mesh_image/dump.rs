// SPDX-License-Identifier: MIT
//! The text dumps of `obj_global.c` on the image (finding 0015, 2026-10-28 pass): the mesh,
//! its vertex-to-polygon and polygon-to-vertex tables, to a `FILE *` through `fprintf`
//! (and a last `fflush`), in the original's calls: one `fprintf` per piece of text, the
//! format at its address in `.data`, `double`s as two words. Nothing is formatted here:
//! the outside world prints.

use si_mem::Mem;

use super::off as mo;
use crate::model_image::{Callee, Outside};

/// The C library's `fprintf`, through the binary's stub.
pub const FPRINTF: Callee = Callee { name: "fprintf", addr: 0x080c_3b88 };
/// The C library's `fflush`, through the binary's stub.
pub const FFLUSH: Callee = Callee { name: "fflush", addr: 0x080c_3be8 };

/// The words a `double` takes on the stack: the `f32` at `a` converted.
fn dbl(m: &Mem, a: u32) -> [u32; 2] {
    let b = (m.f32(a) as f64).to_bits();
    [b as u32, (b >> 32) as u32]
}

/// A printed list of one word per entry: for each of `count` entries of `step` bytes from
/// `base` (plus `at`), `item` with the word, and after the 18th entry (counter 0x11) the
/// `wrap` text and the counter back to 0.
fn list(m: &mut Mem, o: &mut dyn Outside, file: u32, count: u32, base: u32, at: u32, step: u32, item: u32, wrap: u32) {
    if count == 0 {
        return;
    }
    let mut counter = 0i32;
    for k in 0..count {
        let w = m.u32(base.wrapping_add(at).wrapping_add(k.wrapping_mul(step)));
        o.call(m, &FPRINTF, &[file, item, w]);
        if counter == 0x11 {
            o.call(m, &FPRINTF, &[file, wrap]);
            counter = -1;
        }
        counter += 1;
    }
}

/// The star of a vertex: neighbours, edges, polygons (the three lists of 12-byte entries).
fn star_lists(m: &mut Mem, o: &mut dyn Outside, file: u32, v: u32, t: &[u32; 12]) {
    let n = m.u16(v + 0x10) as u32;
    let star = m.u32(v + 0xc);
    o.call(m, &FPRINTF, &[file, t[0]]);
    list(m, o, file, n, star, 0, 0xc, t[1], t[2]);
    o.call(m, &FPRINTF, &[file, t[3]]);
    o.call(m, &FPRINTF, &[file, t[4]]);
    list(m, o, file, n, star, 4, 0xc, t[5], t[6]);
    o.call(m, &FPRINTF, &[file, t[7]]);
    o.call(m, &FPRINTF, &[file, t[8]]);
    list(m, o, file, n, star, 8, 0xc, t[9], t[10]);
    o.call(m, &FPRINTF, &[file, t[11]]);
}

/// The corners of a polygon: vertices and edges.
fn node_lists(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32, t: &[u32; 6]) {
    let n = m.u16(p + 0x10) as u32;
    let nodes = m.u32(p + 0xc);
    list(m, o, file, n, nodes, 0, 0x24, t[0], t[1]);
    o.call(m, &FPRINTF, &[file, t[2]]);
    o.call(m, &FPRINTF, &[file, t[3]]);
    list(m, o, file, n, nodes, 4, 0x24, t[4], t[5]);
}

const VERTEX_TEXT: [u32; 12] = [0x086d3a8d, 0x086d3aa3, 0x086d3aa8, 0x086d3abe, 0x086d3ac0, 0x086d3ad6, 0x086d3adb, 0x086d3af0, 0x086d3af2, 0x086d3b08, 0x086d3b0d, 0x086d3b22];
const VERTEX_TEXT_PV: [u32; 12] = [0x086d3f9f, 0x086d3fb5, 0x086d3fba, 0x086d3fd0, 0x086d3fd2, 0x086d3fe8, 0x086d3fed, 0x086d4002, 0x086d4004, 0x086d401a, 0x086d401f, 0x086d4034];

/// [soft 0x0844105c SI_meshDump]: the whole mesh as text (vertices with their stars,
/// polygons with their corners and corner normals, edges, then the surface approximation
/// settings), under the title `name`. It also sets the mesh's approximation angle (+0x44) to 45.
pub fn mesh_dump(m: &mut Mem, o: &mut dyn Outside, file: u32, mesh: u32, _flag: i16, name: u32) {
    let f = |o: &mut dyn Outside, m: &mut Mem, a: &[u32]| {
        let mut w = vec![file];
        w.extend_from_slice(a);
        o.call(m, &FPRINTF, &w);
    };
    f(o, m, &[0x086d3a00]);
    f(o, m, &[0x086d3a0b]);
    f(o, m, &[0x086d3a17, name]);
    f(o, m, &[0x086d3a23, m.u32(mesh + mo::NB_VERTICES)]);
    f(o, m, &[0x086d3a33]);
    let mut v = m.u32(mesh + mo::VERTICES);
    let mut i = 0u32;
    while (i as i32) < m.i32(mesh + mo::NB_VERTICES) {
        let (x, y, z) = (dbl(m, v), dbl(m, v + 4), dbl(m, v + 8));
        f(o, m, &[0x086d3a60, i, x[0], x[1], y[0], y[1], z[0], z[1]]);
        i += 1;
        f(o, m, &[0x086d3a7f, m.i16(v + 0x12) as i32 as u32]);
        star_lists(m, o, file, v, &VERTEX_TEXT);
        v = v.wrapping_add(0x14);
    }
    f(o, m, &[0x086d3b24, m.u32(mesh + mo::NB_POLYGONS)]);
    f(o, m, &[0x086d3b34]);
    let mut p = m.u32(mesh + mo::POLYGONS);
    let mut i = 0u32;
    while (i as i32) < m.i32(mesh + mo::NB_POLYGONS) {
        let (x, y, z) = (dbl(m, p), dbl(m, p + 4), dbl(m, p + 8));
        f(o, m, &[0x086d3b60, i, x[0], x[1], y[0], y[1], z[0], z[1], m.i16(p + 0x14) as i32 as u32]);
        f(o, m, &[0x086d3b8b]);
        i += 1;
        node_lists(m, o, file, p, &[0x086d3b9d, 0x086d3ba2, 0x086d3bb7, 0x086d3bb9, 0x086d3bcb, 0x086d3bd0]);
        f(o, m, &[0x086d3be5]);
        f(o, m, &[0x086d3be7]);
        let n = m.u16(p + 0x10) as u32;
        if n != 0 {
            let mut counter = 0i32;
            for k in 0..n {
                let c = m.u32(p + 0xc).wrapping_add(k.wrapping_mul(0x24));
                let (a, b, d) = (dbl(m, c + 8), dbl(m, c + 0xc), dbl(m, c + 0x10));
                f(o, m, &[0x086d3bf9, a[0], a[1], b[0], b[1], d[0], d[1]]);
                if counter == 0x11 {
                    f(o, m, &[0x086d3c0e]);
                    counter = -1;
                }
                counter += 1;
            }
        }
        f(o, m, &[0x086d3c23]);
        p = p.wrapping_add(0x18);
    }
    f(o, m, &[0x086d3c25, m.u32(mesh + mo::NB_EDGES)]);
    f(o, m, &[0x086d3c32]);
    let mut e = 1i32;
    while m.i32(mesh + mo::NB_EDGES) > 0 && e <= m.i32(mesh + mo::NB_EDGES) {
        let table = m.u32(mesh + mo::EDGES);
        let r = m.u32(table.wrapping_add(((e >> 10) as u32).wrapping_mul(4))).wrapping_add(((e & 0x3ff) as u32).wrapping_mul(0x14));
        let w = [m.u32(r), m.u32(r + 4), m.u32(r + 8), m.u32(r + 12), m.i16(r + 0x12) as i32 as u32];
        f(o, m, &[0x086d3c60, e as u32, w[0], w[1], w[2], w[3], w[4]]);
        e += 1;
    }
    f(o, m, &[0x086d3c8f]);
    let t = |v: bool, yes: u32, no: u32| if v { yes } else { no };
    f(o, m, &[0x086d3ca0, t(m.i16(mesh + 0x52) != 0, 0x086d3c91, 0x086d3c96)]);
    f(o, m, &[0x086d3ce0, t(m.i16(mesh + 0x50) == 2, 0x086d3cbf, 0x086d3cc7)]);
    f(o, m, &[0x086d3d20, t(m.i16(mesh + 0x38) != 0, 0x086d3d00, 0x086d3d06)]);
    let s = dbl(m, mesh + 0x3c);
    f(o, m, &[0x086d3d60, s[0], s[1]]);
    let l = dbl(m, mesh + 0x40);
    f(o, m, &[0x086d3da0, l[0], l[1]]);
    m.set_f32(mesh + 0x44, 45.0);
    let a = 45.0f64.to_bits();
    f(o, m, &[0x086d3de0, a as u32, (a >> 32) as u32]);
    f(o, m, &[0x086d3e20, t(m.i16(mesh + 0x4c) != 0, 0x086d3e09, 0x086d3e0e)]);
    f(o, m, &[0x086d3e60, m.u16(mesh + 0x48) as u32]);
    f(o, m, &[0x086d3ea0, m.u16(mesh + 0x4a) as u32]);
    o.call(m, &FFLUSH, &[file]);
}

/// [soft 0x08441714 SI_meshDumpVertexPolygon]: the polygons with the vertices and the
/// edges of their corners.
pub fn mesh_dump_vertex_polygon(m: &mut Mem, o: &mut dyn Outside, file: u32, mesh: u32) {
    o.call(m, &FPRINTF, &[file, 0x086d3ec3]);
    o.call(m, &FPRINTF, &[file, 0x086d3ecd]);
    let mut p = m.u32(mesh + mo::POLYGONS);
    let mut i = 0u32;
    while (i as i32) < m.i32(mesh + mo::NB_POLYGONS) {
        let (x, y, z) = (dbl(m, p), dbl(m, p + 4), dbl(m, p + 8));
        o.call(m, &FPRINTF, &[file, 0x086d3ee0, i, x[0], x[1], y[0], y[1], z[0], z[1], m.i16(p + 0x14) as i32 as u32]);
        o.call(m, &FPRINTF, &[file, 0x086d3f0b]);
        i += 1;
        node_lists(m, o, file, p, &[0x086d3f1d, 0x086d3f22, 0x086d3f37, 0x086d3f39, 0x086d3f4b, 0x086d3f50]);
        o.call(m, &FPRINTF, &[file, 0x086d3f65]);
        p = p.wrapping_add(0x18);
    }
    o.call(m, &FFLUSH, &[file]);
}

/// [soft 0x084418dc SI_meshDumpPolygonVertex]: the vertices with their neighbours, edges
/// and polygons.
pub fn mesh_dump_polygon_vertex(m: &mut Mem, o: &mut dyn Outside, file: u32, mesh: u32) {
    o.call(m, &FPRINTF, &[file, 0x086d3f67]);
    o.call(m, &FPRINTF, &[file, 0x086d3f71]);
    let mut v = m.u32(mesh + mo::VERTICES);
    let mut i = 0u32;
    while (i as i32) < m.i32(mesh + mo::NB_VERTICES) {
        let (x, y, z) = (dbl(m, v), dbl(m, v + 4), dbl(m, v + 8));
        o.call(m, &FPRINTF, &[file, 0x086d3f80, i, x[0], x[1], y[0], y[1], z[0], z[1]]);
        i += 1;
        star_lists(m, o, file, v, &VERTEX_TEXT_PV);
        v = v.wrapping_add(0x14);
    }
    o.call(m, &FFLUSH, &[file]);
}
