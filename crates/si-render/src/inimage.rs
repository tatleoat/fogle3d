// SPDX-License-Identifier: MIT
//! Readers of the renderer's records from the memory image (`si_mem::Mem`) into this
//! crate's own records, at the offsets finding 0012 gives (sections 3, 13, 14, 16).
//!
//! Nothing here reproduces a function of the original. The routines below `SI_update` are
//! ported on the crate's own records ([`crate::shade::ShadeState`] and its members); until
//! they are converted to the image, this is where a record of the image becomes one of
//! those: what [`crate::adapter`] builds the scene from, and what the emulation checks read
//! a loaded case with. A pointer member is the caller's to resolve (a list, a table of
//! records): each reader says which.

#[cfg(test)]
use crate::render::Slice;
use crate::shade::{MatWork, ShadeLight, ShadeMap, ShadeMaterial};
use crate::t3d_ren::ShadeTex3d;
use crate::types::{RVertex, Tn, Vector};
#[cfg(test)]
use crate::types::Ray;
use si_math::types::Pspoint;
use si_mem::Mem;
use si_x87::x;

#[inline]
fn v3(m: &Mem, a: u32) -> Vector {
    Vector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8))
}

fn f32s<const N: usize>(m: &Mem, a: u32) -> [f32; N] {
    core::array::from_fn(|i| m.f32(a + 4 * i as u32))
}

/// `SI_RVertex` (0x4c bytes). The pointers at +0x40 and +0x48 are given as their words.
pub fn rvertex_at(m: &Mem, a: u32) -> RVertex {
    RVertex {
        w00: [m.u32(a), m.u32(a + 4)],
        raw: v3(m, a + 0x08),
        raw_normal: v3(m, a + 0x14),
        world: v3(m, a + 0x20),
        ps: Pspoint { x: m.f32(a + 0x2c), y: m.f32(a + 0x30) },
        normal: v3(m, a + 0x34),
        uv: m.u32(a + 0x40),
        w44: m.u32(a + 0x44),
        p48: m.u32(a + 0x48),
    }
}

/// `tn` (0x60 bytes) without its pointers: the corners (+0x00), the material (+0x4c, given
/// as its word) and the link (+0x5c) are the caller's.
pub fn tn_at(m: &Mem, a: u32) -> Tn {
    Tn {
        v: [None; 3],
        n: v3(m, a + 0x0c),
        d: m.f32(a + 0x18),
        inv_d: m.f32(a + 0x1c),
        axis: m.i16(a + 0x20),
        inv_n: m.f32(a + 0x24),
        edge: [v3(m, a + 0x28), v3(m, a + 0x34), v3(m, a + 0x40)],
        material: m.u32(a + 0x4c),
        flags: m.u16(a + 0x50),
        flags2: m.u16(a + 0x52),
        w58: m.i16(a + 0x58),
        next: None,
    }
}

/// `SI_Intersect2(ray, slice)` (0x0825f6bc) on a ray and a slice of the image: the slice's
/// key (+0x00) and triangle (+0x18), the triangle with its three corners and the ray's
/// direction (+0x0c) are read, [`crate::render::intersect2`] runs on them, and on a hit the
/// ray's distance (+0x24) and triangle (+0x34) are stored, as the original stores them.
/// The answer is the original's: 0 a hit, 2 a miss.
///
/// Only the words the test looks at are read from the image: the triangle's dominant axis,
/// the plane normal's component along it, the two components of each edge and of each
/// corner's world position that lie in the plane perpendicular to that axis, and the ray's
/// two direction components of that plane. The arithmetic is that of
/// [`crate::render::intersect2`] with `weights`, `axis_normal` and `inside` (the third
/// direction component's product, which that function forms and never uses, is left out;
/// the x87 model keeps no flags, so nothing observes it). [`intersect2_at_whole`] is the
/// version that builds the whole records; a test holds the two against each other.
pub fn intersect2_at(m: &mut Mem, ray: u32, slice: u32) -> i32 {
    let key = m.f32(slice);
    // (the original tests the key before it looks at the triangle)
    if si_x87::x(key).le(si_x87::x(0.0f32)) {
        return 2;
    }
    let tn_addr = m.u32(slice + 0x18);
    let axis = m.i16(tn_addr + 0x20);
    // (a, b, normal): the components of the weights' plane, as `render::weights` takes them
    let (ia, ib, inorm): (u32, u32, u32) = match axis {
        1 => (2, 1, 0),
        2 => (0, 2, 1),
        _ => (1, 0, 2),
    };
    let tf = (si_x87::F80::ONE / key).to_f32();
    let pa = x(tf) * m.f32(ray + 0x0c + 4 * ia);
    let pb = x(tf) * m.f32(ray + 0x0c + 4 * ib);
    // weight k: (p.a - c.a) * e.b - (p.b - c.b) * e.a, for k = 0, 1, 2 with the corner after
    // it (1, 2, 0) and the edge k
    let w = |k: u32| {
        let corner = m.u32(tn_addr + 4 * ((k + 1) % 3)) + 0x20;
        let edge = tn_addr + 0x28 + 12 * k;
        (pa - m.f32(corner + 4 * ia)) * m.f32(edge + 4 * ib) - (pb - m.f32(corner + 4 * ib)) * m.f32(edge + 4 * ia)
    };
    let weights = [w(0), w(1), w(2)];
    let n = m.f32(tn_addr + 0x0c + 4 * inorm);
    if !crate::render::inside(&weights, n) {
        return 2;
    }
    m.set_f32(ray + 0x24, tf);
    m.set_u32(ray + 0x34, tn_addr);
    0
}

/// [`intersect2_at`] by building the triangle's and corners' records and running
/// [`crate::render::intersect2`] on them: the reference for the test.
#[cfg(test)]
fn intersect2_at_whole(m: &mut Mem, ray: u32, slice: u32) -> i32 {
    let key = m.f32(slice);
    if si_x87::x(key).le(si_x87::x(0.0f32)) {
        return 2;
    }
    let tn_addr = m.u32(slice + 0x18);
    let mut tn = tn_at(m, tn_addr);
    let verts: [RVertex; 3] = core::array::from_fn(|k| rvertex_at(m, m.u32(tn_addr + 4 * k as u32)));
    tn.v = [Some(0), Some(1), Some(2)];
    let mut r = Ray { dir: v3(m, ray + 0x0c), t: m.f32(ray + 0x24), ..Ray::default() };
    let s = Slice { key, tn: 0, ..Slice::default() };
    let answer = crate::render::intersect2(&mut r, &s, &[tn], &verts);
    if answer == 0 {
        m.set_f32(ray + 0x24, r.t);
        m.set_u32(ray + 0x34, tn_addr);
    }
    answer
}

// ------------------------------------------------------------------ materials and textures

fn text_at(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        Vec::new()
    } else {
        m.cstr(p)
    }
}

/// `__SI_LclMap` (0x1d8 bytes): a 2D texture as applied to a material or a node. The link
/// (+0x16c) and the picture (+0x24, a `glb_map`) are the caller's to resolve; the path
/// (+0x14) and the frame's file name (+0x20) are read as texts.
pub fn map_at(m: &Mem, a: u32) -> ShadeMap {
    let file = m.u32(a + 0x20);
    ShadeMap {
        path: text_at(m, m.u32(a + 0x14)),
        file: (file != 0).then(|| m.cstr(file)),
        anim: m.i16(a + 0x28),
        method: m.i16(a + 0x2a),
        w2c: m.i16(a + 0x2c),
        crop: m.i16(a + 0x2e),
        crop_x0: m.i16(a + 0x30),
        crop_x1: m.i16(a + 0x32),
        crop_y0: m.i16(a + 0x34),
        crop_y1: m.i16(a + 0x36),
        umin: m.f32(a + 0x38),
        vmin: m.f32(a + 0x3c),
        umax: m.f32(a + 0x40),
        vmax: m.f32(a + 0x44),
        u2min: m.f32(a + 0x48),
        v2min: m.f32(a + 0x4c),
        u2max: m.f32(a + 0x50),
        v2max: m.f32(a + 0x54),
        uscale: m.f32(a + 0x58),
        vscale: m.f32(a + 0x5c),
        pix_u: m.f32(a + 0x60),
        pix_v: m.f32(a + 0x64),
        uoff: m.f32(a + 0x68),
        voff: m.f32(a + 0x6c),
        uoff_frac: m.f32(a + 0x70),
        voff_frac: m.f32(a + 0x74),
        neg_u: m.i16(a + 0x78),
        neg_v: m.i16(a + 0x7a),
        v7c: f32s(m, a + 0x7c),
        centre: f32s(m, a + 0x88),
        rotation: f32s(m, a + 0x94),
        translation: f32s(m, a + 0xa0),
        scaling: f32s(m, a + 0xac),
        tmat: core::array::from_fn(|r| f32s(m, a + 0xb8 + 16 * r as u32)),
        urepeat: m.i16(a + 0xf8),
        vrepeat: m.i16(a + 0xfa),
        swap_uv: m.i16(a + 0xfc),
        alt_u: m.i16(a + 0xfe),
        alt_v: m.i16(a + 0x100),
        band_u: m.i16(a + 0x102),
        band_v: m.i16(a + 0x104),
        wrap_u: m.i16(a + 0x106),
        wrap_v: m.i16(a + 0x108),
        mask: m.i16(a + 0x10a),
        blend: m.f32(a + 0x10c),
        bump_alpha: m.i16(a + 0x110),
        fx_ambient: m.i16(a + 0x112),
        fx_diffuse: m.i16(a + 0x114),
        fx_specular: m.i16(a + 0x116),
        fx_reflect: m.i16(a + 0x118),
        fx_transp: m.i16(a + 0x11a),
        fx_11e: m.i16(a + 0x11e),
        bump: m.i16(a + 0x120),
        skip: m.i16(a + 0x124),
        factors: f32s(m, a + 0x128),
        work: f32s(m, a + 0x148),
        bump_amount: m.f32(a + 0x160),
        env_scale: m.f32(a + 0x164),
        env_rotation: m.f32(a + 0x168),
        black_white: m.i16(a + 0x18e),
        id: m.i32(a + 0x190),
        box_min: f32s(m, a + 0x1a4),
        box_max: f32s(m, a + 0x1b0),
        box_node: m.i32(a + 0x1bc),
        box_r5: m.f32(a + 0x1c0),
        box_r6: m.f32(a + 0x1c4),
        repeat: m.i16(a + 0x1d4),
        next: None,
        glb: None,
    }
}

/// `__SI_Material` (0xdcc bytes). The 2D texture list (+0x6ec), the reflection map (+0x6f4)
/// and the 3D texture (+0x6fc) are the caller's to resolve; the two jitter records (+0x6ac,
/// +0x6bc) are read as their amplitudes (their tables are made by `SI_materials`).
pub fn material_at(m: &Mem, a: u32) -> ShadeMaterial {
    let mut r = ShadeMaterial {
        model: m.i16(a + 0x1e),
        w20: m.i16(a + 0x20),
        ambient: f32s(m, a + 0x34),
        diffuse: f32s(m, a + 0x44),
        specular: f32s(m, a + 0x54),
        reflectivity: m.f32(a + 0x6a8),
        transparency: m.f32(a + 0x6b8),
        refraction: m.f32(a + 0x6dc),
        flags: m.u32(a + 0x6e0) as u16,
        n_maps: m.i16(a + 0x6e8),
        static_blur: m.i16(a + 0x758),
        blur_width: m.f32(a + 0x75c),
        vcol_rgb: m.i16(a + 0xdc8),
        vcol_alpha: m.i16(a + 0xdca),
        ..ShadeMaterial::default()
    };
    r.work = MatWork {
        base_ambient: f32s(m, a + 0x700),
        ambient: f32s(m, a + 0x710),
        diffuse: f32s(m, a + 0x720),
        specular: f32s(m, a + 0x730),
        reflectivity: m.f32(a + 0x740),
        opacity: m.f32(a + 0x744),
        refraction: m.f32(a + 0x748),
        existence: m.f32(a + 0x74c),
    };
    r.spec_table.exponent = m.f32(a + 0x64);
    r.spec_table.pairs = (0..200).map(|i| f32s(m, a + 0x68 + 8 * i)).collect();
    r.spec_table.after = f32s(m, a + 0x6a8);
    r.blur_table.exponent = m.f32(a + 0x760);
    r.blur_table.pairs = (0..200).map(|i| f32s(m, a + 0x764 + 8 * i)).collect();
    r.blur_table.after = f32s(m, a + 0xda4);
    r.jitter_6ac.amplitude = m.f32(a + 0x6ac);
    r.jitter_6bc.amplitude = m.f32(a + 0x6bc);
    r
}

/// `SI_Txt3d` (0x40a4 bytes): a 3D texture made ready.
pub fn txt3d_at(m: &Mem, a: u32) -> crate::tt::Txt3d {
    crate::tt::Txt3d {
        kind: m.i32(a),
        has_tform: m.i16(a + 0x04),
        tform: core::array::from_fn(|r| f32s(m, a + 0x08 + 16 * r as u32)),
        scale: m.f32(a + 0x48),
        taper: m.f32(a + 0x4c),
        strength: m.f32(a + 0x50),
        terms: m.i32(a + 0x54),
        term: core::array::from_fn(|i| f32s(m, a + 0x58 + 8 * i as u32)),
        ramp: (0..1024).map(|i| f32s(m, a + 0x98 + 16 * i)).collect(),
        stops: core::array::from_fn(|i| m.i32(a + 0x4098 + 4 * i as u32)),
    }
}

/// `__SI_Texture3D` (0x158 bytes), with the texture made ready (+0x8c) when there is one.
pub fn tex3d_at(m: &Mem, a: u32) -> ShadeTex3d {
    let colours: [f32; 20] = f32s(m, a + 0x30);
    let txt = m.u32(a + 0x8c);
    ShadeTex3d {
        kind: m.i32(a + 0x0c),
        spacing: m.f32(a + 0x10),
        taper: m.f32(a + 0x14),
        strength: m.f32(a + 0x18),
        iterations: m.i32(a + 0x1c),
        power: m.f32(a + 0x20),
        mask: m.i16(a + 0x24),
        blend: m.f32(a + 0x28),
        channel: m.i16(a + 0x2c),
        colours: core::array::from_fn(|i| core::array::from_fn(|k| colours[4 * i + k])),
        stops: f32s(m, a + 0x80),
        txt: (txt != 0).then(|| txt3d_at(m, txt)),
        scaling: f32s(m, a + 0x90),
        rotation: f32s(m, a + 0x9c),
        translation: f32s(m, a + 0xa8),
        matrix: core::array::from_fn(|r| f32s(m, a + 0xb4 + 16 * r as u32)),
        fx_ambient: m.i16(a + 0xf4),
        fx_diffuse: m.i16(a + 0xf6),
        fx_specular: m.i16(a + 0xf8),
        fx_reflect: m.i16(a + 0xfa),
        fx_transp: m.i16(a + 0xfc),
        fx_100: m.i16(a + 0x100),
        fx_rough: m.i16(a + 0x102),
        factors: f32s(m, a + 0x108),
        work: f32s(m, a + 0x128),
    }
}

/// `glb_map` (0x28 bytes): a picture as the 2D maps share it, with its pixel block when it
/// has one. The pointer at +0x1c points at the picture's first pixel, width + 4 pixels into
/// a block of (width + 3) × (height + 3) pixels of five bytes, its length rounded up to a
/// multiple of four; `block_bytes` overrides that length (a check whose case holds the
/// block in a buffer of another size says so). The link (+0x24) is the caller's.
pub fn glb_at(m: &Mem, a: u32, block_bytes: Option<u32>) -> crate::materials::GlbMap {
    let (width, height) = (m.i16(a + 0x16), m.i16(a + 0x18));
    let p = m.u32(a + 0x1c);
    let pixels = (p != 0).then(|| {
        let origin = (width as i32 + 4).max(0) as u32;
        let nominal = ((width as i32 + 3).max(0) as u32).wrapping_mul((height as i32 + 3).max(0) as u32).wrapping_mul(5).wrapping_add(3) & !3;
        let start = p.wrapping_sub(origin * 5);
        let n = block_bytes.unwrap_or(nominal);
        let block = if m.is_mapped(start, n) { m.bytes(start, n).to_vec() } else { Vec::new() };
        crate::materials::TexPixels { block, origin: origin as usize }
    });
    crate::materials::GlbMap {
        name: text_at(m, m.u32(a)),
        crop: m.i16(a + 0x04),
        crop_x0: m.i16(a + 0x06),
        crop_x1: m.i16(a + 0x08),
        crop_y0: m.i16(a + 0x0a),
        crop_y1: m.i16(a + 0x0c),
        users: m.i16(a + 0x14),
        width,
        height,
        pixels,
        next: None,
        freed: false,
    }
}

// ----------------------------------------------------------------------------------- lights

/// `light` (0x240 bytes). The link (+0x140) is the caller's. The depth map (+0x114) is read
/// as `size` (+0x118) rows of `size` floats when the pointer is set; the table of a
/// selective light (+0x168) as `table_len` entries (one per model element: `SI_image` gives
/// it the MODELS chapter's length, +0x164); the jitter record (+0x130) as its amplitude.
pub fn light_at(m: &Mem, a: u32, table_len: u32) -> ShadeLight {
    let matrix = |at: u32| -> si_math::types::Matrix { core::array::from_fn(|r| f32s(m, at + 16 * r as u32)) };
    let size = m.i32(a + 0x118);
    let rows = m.u32(a + 0x114);
    let table = m.u32(a + 0x168);
    let zmap = if rows != 0 && size > 0 && m.is_mapped(rows, 4 * size as u32) {
        (0..size as u32).map(|y| {
            let row = m.u32(rows + 4 * y);
            (0..size as u32).map(|x| m.f32(row.wrapping_add(4 * x))).collect()
        }).collect()
    } else {
        Vec::new()
    };
    ShadeLight {
        kind: m.i16(a + 0x1e),
        color: f32s(m, a + 0x24),
        pos_given: v3(m, a + 0x34),
        pos: v3(m, a + 0x40),
        dir: v3(m, a + 0x4c),
        interest: v3(m, a + 0x58),
        spot_dir: v3(m, a + 0x64),
        cone: m.f32(a + 0x70),
        spread: m.f32(a + 0x74),
        cone_cos: m.f32(a + 0x78),
        spread_cos: m.f32(a + 0x7c),
        cone_div: m.f32(a + 0x80),
        falloff_start: m.f32(a + 0x84),
        falloff_end: m.f32(a + 0x88),
        shadow: m.i16(a + 0x8c),
        umbra: m.f32(a + 0x90),
        depth_mat: matrix(a + 0x94),
        xy_mat: matrix(a + 0xd4),
        zmap,
        map_size: size,
        soft_full: m.i32(a + 0x11c),
        soft_size: m.i32(a + 0x120),
        soft_step: m.i32(a + 0x124),
        soft_w128: m.f32(a + 0x128),
        soft_scale: m.f32(a + 0x12c),
        jitter: crate::jitter::Jitter { amplitude: m.f32(a + 0x130), ..crate::jitter::Jitter::default() },
        falloff: m.u16(a + 0x13c) & 1 != 0,
        next: None,
        selective: m.i32(a + 0x15c),
        select_id: m.i32(a + 0x160),
        select_table: if table != 0 && m.is_mapped(table, 4 * table_len) { (0..table_len).map(|i| m.i32(table + 4 * i)).collect() } else { Vec::new() },
        on: m.i16(a + 0x176),
    }
}

// ----------------------------------------------------------------------------- the geometry

/// `SI_Mesh` (0x54 bytes) with its vertices (20 bytes each: position, star pointer and
/// count, flag), stars (12 bytes an entry), polygons (24 bytes: normal, corner pointer and
/// count, material, flag), corners (36 bytes: vertex, edge, normal, pair, colour, pointer
/// to the further pairs) and edges (20 bytes, in blocks of 1,024 behind a table of block
/// pointers; record 0 is not an edge). `extra` is the number of further pairs a corner
/// carries where its pointer is set (the node's texture coordinate sets after the first).
/// A count that does not fit the image gives an empty array.
pub fn mesh_at(m: &Mem, a: u32, extra: u32) -> si_model::mesh::Mesh {
    use si_model::mesh::{Edge, Mesh, PolyNode, Polygon, Star, Vertex};
    let (verts, polys, edges) = (m.u32(a), m.u32(a + 4), m.u32(a + 8));
    let count = |n: i32, at: u32, size: u32| if n > 0 && m.is_mapped(at, (n as u32).saturating_mul(size)) { n as u32 } else { 0 };
    let (nv, npol, ne) = (count(m.i32(a + 0x14), verts, 20), count(m.i32(a + 0x18), polys, 24), m.i32(a + 0x1c).max(0) as u32);
    let mut mesh = Mesh::default();
    mesh.angle = m.f32(a + 0x0c);
    mesh.cos_angle = m.f32(a + 0x10);
    mesh.flags = m.u16(a + 0x20);
    for k in 0..nv {
        let v = verts + 20 * k;
        let (star, n) = (m.u32(v + 0x0c), m.u16(v + 0x10) as u32);
        let n = if m.is_mapped(star, 12 * n) { n } else { 0 };
        mesh.vertices.push(Vertex {
            pos: f32s(m, v),
            star: (0..n).map(|e| Star { vertex: m.i32(star + 12 * e), edge: m.i32(star + 12 * e + 4), polygon: m.i32(star + 12 * e + 8) }).collect(),
            flag: m.i16(v + 0x12),
        });
    }
    for k in 0..npol {
        let p = polys + 24 * k;
        let (nodes, n) = (m.u32(p + 0x0c), m.u16(p + 0x10) as u32);
        let n = if m.is_mapped(nodes, 36 * n) { n } else { 0 };
        mesh.polygons.push(Polygon {
            normal: f32s(m, p),
            nodes: (0..n)
                .map(|c| {
                    let node = nodes + 36 * c;
                    let ext = m.u32(node + 0x20);
                    PolyNode {
                        vertex: m.i32(node),
                        edge: m.i32(node + 4),
                        normal: f32s(m, node + 8),
                        uv: f32s(m, node + 0x14),
                        color: m.u32(node + 0x1c),
                        extra_uv: if ext != 0 && m.is_mapped(ext, 8 * extra) { (0..extra).map(|j| f32s(m, ext + 8 * j)).collect() } else { Vec::new() },
                    }
                })
                .collect(),
            material: m.i16(p + 0x12),
            flag: m.i16(p + 0x14),
        });
    }
    if edges != 0 {
        mesh.edges.push(Edge::default());
        for n in 1..=ne {
            let block = m.u32(edges.wrapping_add(4 * (n >> 10)));
            let r = block.wrapping_add(20 * (n & 0x3ff));
            if !m.is_mapped(r, 20) {
                break;
            }
            mesh.edges.push(Edge { v0: m.i32(r), v1: m.i32(r + 4), p0: m.i32(r + 8), p1: m.i32(r + 12), x10: m.i16(r + 0x10), flag: m.i16(r + 0x12) });
        }
    }
    mesh
}

/// `SI_Spline` (0x74 bytes) with its keys (0x30 bytes each: the handle before, the position,
/// the handle after, the constraint +0x28, the segment type +0x2a, the flag +0x2c), as a
/// model file's spline record. A key count that does not fit the image gives no keys.
pub fn spline_at(m: &Mem, a: u32) -> si_hrc::Spline {
    let n = m.u16(a + 0x10);
    let keys = m.u32(a + 0x14);
    let name = m.u32(a);
    let held = if keys != 0 && m.is_mapped(keys, 0x30 * n as u32) { n as u32 } else { 0 };
    si_hrc::Spline {
        name: if name != 0 && m.is_mapped(name, 1) { m.cstr(name) } else { Vec::new() },
        phantom: m.i16(a + 0x06),
        close: m.i16(a + 0x08),
        kind: m.i32(a + 0x0c),
        nkeys: n,
        keys: (0..held)
            .map(|k| {
                let at = keys + 0x30 * k;
                si_hrc::SplineKey { prev: f32s(m, at), pos: f32s(m, at + 0x0c), next: f32s(m, at + 0x18), constraint: m.i16(at + 0x28), segment: m.i16(at + 0x2a), flag: m.i16(at + 0x2c) }
            })
            .collect(),
        tension: m.f32(a + 0x1c),
        step: m.i16(a + 0x20),
        old_points: false,
    }
}

/// `SI_Face` (0x88 bytes) of splines: the outline (the spline embedded at +0x04), the count
/// of holes (+0x78) and the holes (+0x7c: splines 0x74 bytes apart). `None` for a null
/// record and for a face of NURBS curves (the word at +0x84 is 0, the curves at +0x80),
/// which is not read.
pub fn face_at(m: &Mem, a: u32) -> Option<si_hrc::Face> {
    if a == 0 || !m.is_mapped(a, 0x88) || m.i16(a + 0x84) == 0 {
        return None;
    }
    let count = m.i16(a + 0x78);
    let holes = m.u32(a + 0x7c);
    let n = if holes != 0 && m.is_mapped(holes, 0x74 * count.max(0) as u32) { count.max(0) as u32 } else { 0 };
    Some(si_hrc::Face { exterior: spline_at(m, a + 4), count, holes: (0..n).map(|k| spline_at(m, holes + 0x74 * k)).collect(), nurbs: Vec::new(), is_spline: m.i16(a + 0x84) })
}

/// `SI_Patch` (0x204 bytes) with its points (+0x1a4: point count in u (+0x0c) × in v
/// (+0x0e) records of four floats): the types (+0x04, +0x08), the closed switches (+0x10,
/// +0x12), the tensions (+0x14, +0x18) and the steps (+0x1c, +0x1e). The subdivision
/// (+0x1c4 points, +0x1c8 normals) is read when the record's mark (+0x1c0) says it is made,
/// each (u step + 1) × (v step + 1) vectors a cell. `None` for a null record or points that
/// do not fit the image.
pub fn patch_at(m: &Mem, a: u32) -> Option<si_spline::patch::Surface> {
    if a == 0 || !m.is_mapped(a, 0x204) {
        return None;
    }
    let mut s = si_spline::patch::Surface::new();
    let r = &mut s.rec;
    (r.u_type, r.v_type) = (m.i32(a + 0x04), m.i32(a + 0x08));
    (r.nb_u, r.nb_v) = (m.u16(a + 0x0c), m.u16(a + 0x0e));
    (r.u_closed, r.v_closed) = (m.i16(a + 0x10), m.i16(a + 0x12));
    (r.u_tension, r.v_tension) = (m.f32(a + 0x14), m.f32(a + 0x18));
    (r.u_step, r.v_step) = (m.u16(a + 0x1c), m.u16(a + 0x1e));
    let points = m.u32(a + 0x1a4);
    let n = r.nb_u as u32 * r.nb_v as u32;
    if points == 0 || !m.is_mapped(points, n.saturating_mul(16)) {
        return None;
    }
    s.points = (0..n).map(|k| f32s(m, points + 16 * k)).collect();
    // (a placeholder for the block's address: any pointer that is not null)
    s.rec.points = si_types::Ptr32::new(1);
    let vectors = |at: u32, n: u32| -> Vec<si_spline::Vector> {
        (0..n)
            .map(|k| {
                let mut v = <si_spline::Vector as si_types::Zero>::zero();
                (v.x, v.y, v.z) = (m.f32(at + 12 * k), m.f32(at + 12 * k + 4), m.f32(at + 12 * k + 8));
                v
            })
            .collect()
    };
    let ((nu, _), (nv, _)) = s.cells();
    let cells = (nu.max(0) as u32).saturating_mul(nv.max(0) as u32).saturating_mul(s.rec.u_step as u32 + 1).saturating_mul(s.rec.v_step as u32 + 1);
    let (sub0, sub1) = (m.u32(a + 0x1c4), m.u32(a + 0x1c8));
    if m.i16(a + 0x1c0) != 0 && sub0 != 0 && sub1 != 0 && m.is_mapped(sub0, cells.saturating_mul(12)) && m.is_mapped(sub1, cells.saturating_mul(12)) {
        s.rec.unk_1c0 = m.i16(a + 0x1c0);
        (s.subdiv0, s.subdiv1) = (vectors(sub0, cells), vectors(sub1, cells));
        (s.rec.subdiv0, s.rec.subdiv1) = (si_types::Ptr32::new(1), si_types::Ptr32::new(1));
    }
    Some(s)
}

/// The texture coordinate sets of a node: the count (+0x424) and, for each set after the
/// first, the identifiers of the textures that use it (+0x428: 8-byte records, a pointer
/// and a number).
pub fn uv_domains_at(m: &Mem, node: u32) -> crate::softvertex::UvDomains {
    let count = m.i32(node + 0x424);
    let records = m.u32(node + 0x428);
    let n = if records != 0 { (count - 1).max(0) as u32 } else { 0 };
    let n = if m.is_mapped(records, 8u32.saturating_mul(n)) { n } else { 0 };
    crate::softvertex::UvDomains {
        count,
        sets: (0..n)
            .map(|d| {
                let (ids, k) = (m.u32(records + 8 * d), m.i32(records + 8 * d + 4).max(0) as u32);
                if ids != 0 && m.is_mapped(ids, 4u32.saturating_mul(k)) {
                    (0..k).map(|i| m.i32(ids + 4 * i)).collect()
                } else {
                    Vec::new()
                }
            })
            .collect(),
    }
}

/// The members of `__SI_Model` (0x430 bytes) the mesh converter reads: the box switch
/// (+0x0c) and the box (+0x10, +0x1c), the number of definitions (+0x34), the geometry by
/// the node's kind (+0x30), the material count (+0x206) and whether there is an array
/// (+0x20c), visibility (+0x2b8), the algorithm (+0x2ec), the element number (+0x354), the
/// matte word (+0x3f8) and the texture coordinate sets. The node's own 2D texture list
/// (+0x21c) is the caller's to resolve.
///
/// Geometry: a mesh (kind 4) is read: the current one (+0x48), and the one that lends
/// positions when it is another record (+0x3c when set, else the first definition of the
/// array at +0x38 for a node with one definition and the second otherwise). The other
/// rendered kinds: a face of splines (kind 2) is read ([`face_at`]: the current one, and
/// the definition that lends positions when it is another record); a patch (0x08269988)
/// for kind 1, a metaball system for 7 and a NURBS surface (0x084e16f8) for 10 are named
/// only (a NURBS surface is then converted on the image by the adapter:
/// `adapter::nurbs_on_image`).
pub fn node_source_at(m: &Mem, node: u32) -> crate::softtriangle::NodeSource {
    use crate::softtriangle::{NodeDef, NodeSource};
    let kind = m.i32(node + 0x30);
    let nb_defs = m.i16(node + 0x34);
    let uv = uv_domains_at(m, node);
    let extra = (uv.count - 1).max(0) as u32;
    let def = match kind {
        4 => {
            let current = m.u32(node + 0x48);
            let defs = m.u32(node + 0x38);
            let mut shape = m.u32(node + 0x3c);
            if shape == 0 && defs != 0 {
                shape = m.u32(defs + if nb_defs == 1 { 0 } else { 4 });
            }
            if current == 0 {
                NodeDef::None
            } else {
                NodeDef::Mesh { mesh: mesh_at(m, current, extra), positions: (shape != 0 && shape != current).then(|| mesh_at(m, shape, extra)) }
            }
        }
        1 => {
            let current = m.u32(node + 0x48);
            let defs = m.u32(node + 0x38);
            let shape = if defs != 0 { m.u32(defs + if nb_defs == 1 { 0 } else { 4 }) } else { 0 };
            match patch_at(m, current) {
                Some(patch) => NodeDef::PatchOf { patch: Box::new(patch), shape: if shape != 0 && shape != current { patch_at(m, shape).map(Box::new) } else { None } },
                None => NodeDef::Patch,
            }
        }
        2 => {
            // the face that lends positions: a face node takes no record from +0x3c
            let current = m.u32(node + 0x48);
            let defs = m.u32(node + 0x38);
            let shape = if defs != 0 { m.u32(defs + if nb_defs == 1 { 0 } else { 4 }) } else { 0 };
            match face_at(m, current) {
                Some(face) => NodeDef::FaceOf { face: Box::new(face), shape: if shape != 0 && shape != current { face_at(m, shape).map(Box::new) } else { None } },
                None => NodeDef::Face,
            }
        }
        7 => NodeDef::Meta,
        10 => NodeDef::Nurbs,
        _ => NodeDef::None,
    };
    NodeSource {
        box_only: m.i16(node + 0x0c),
        bbox_min: v3(m, node + 0x10),
        bbox_max: v3(m, node + 0x1c),
        nb_defs,
        def,
        n_materials: m.i16(node + 0x206),
        no_material_array: m.u32(node + 0x20c) == 0,
        maps: None,
        visible: m.i16(node + 0x2b8),
        algo_type: m.i32(node + 0x2ec),
        elem_id: m.i32(node + 0x354),
        garbage_matte: m.i16(node + 0x3f8),
        uv,
        render: Default::default(),
    }
}

// ------------------------------------------------------------------ the other direction

/// Lays a mesh out in the image as [`mesh_at`] reads it: the `SI_Mesh` record (0x54 bytes)
/// and its vertices, stars, polygons, corners, further pairs and edge blocks, each a block
/// of the image's allocator. Returns the record's address. For whoever builds a scene in an
/// image from loaded files (a corner gets its further pairs only when it has `extra` of
/// them); nothing of the original is reproduced here.
pub fn mesh_to_image(m: &mut Mem, mesh: &si_model::mesh::Mesh, extra: u32) -> u32 {
    let a = m.calloc(1, 0x54);
    let (nv, npol) = (mesh.vertices.len() as u32, mesh.polygons.len() as u32);
    let ne = mesh.edges.len().saturating_sub(1) as u32;
    m.set_f32(a + 0x0c, mesh.angle);
    m.set_f32(a + 0x10, mesh.cos_angle);
    m.set_i32(a + 0x14, nv as i32);
    m.set_i32(a + 0x18, npol as i32);
    m.set_i32(a + 0x1c, ne as i32);
    m.set_u16(a + 0x20, mesh.flags);
    if nv != 0 {
        let verts = m.calloc(nv, 20);
        m.set_u32(a, verts);
        for (k, v) in mesh.vertices.iter().enumerate() {
            let at = verts + 20 * k as u32;
            for (i, c) in v.pos.iter().enumerate() {
                m.set_f32(at + 4 * i as u32, *c);
            }
            if !v.star.is_empty() {
                let star = m.calloc(v.star.len() as u32, 12);
                for (e, st) in v.star.iter().enumerate() {
                    m.set_i32(star + 12 * e as u32, st.vertex);
                    m.set_i32(star + 12 * e as u32 + 4, st.edge);
                    m.set_i32(star + 12 * e as u32 + 8, st.polygon);
                }
                m.set_u32(at + 0x0c, star);
            }
            m.set_u16(at + 0x10, v.star.len() as u16);
            m.set_i16(at + 0x12, v.flag);
        }
    }
    if npol != 0 {
        let polys = m.calloc(npol, 24);
        m.set_u32(a + 4, polys);
        for (k, p) in mesh.polygons.iter().enumerate() {
            let at = polys + 24 * k as u32;
            for (i, c) in p.normal.iter().enumerate() {
                m.set_f32(at + 4 * i as u32, *c);
            }
            if !p.nodes.is_empty() {
                let nodes = m.calloc(p.nodes.len() as u32, 36);
                for (c, n) in p.nodes.iter().enumerate() {
                    let node = nodes + 36 * c as u32;
                    m.set_i32(node, n.vertex);
                    m.set_i32(node + 4, n.edge);
                    for (i, v) in n.normal.iter().chain(n.uv.iter()).enumerate() {
                        m.set_f32(node + 8 + 4 * i as u32, *v);
                    }
                    m.set_u32(node + 0x1c, n.color);
                    if extra != 0 && n.extra_uv.len() as u32 >= extra {
                        let ext = m.calloc(extra, 8);
                        for (j, pair) in n.extra_uv.iter().take(extra as usize).enumerate() {
                            m.set_f32(ext + 8 * j as u32, pair[0]);
                            m.set_f32(ext + 8 * j as u32 + 4, pair[1]);
                        }
                        m.set_u32(node + 0x20, ext);
                    }
                }
                m.set_u32(at + 0x0c, nodes);
            }
            m.set_u16(at + 0x10, p.nodes.len() as u16);
            m.set_i16(at + 0x12, p.material);
            m.set_i16(at + 0x14, p.flag);
        }
    }
    if !mesh.edges.is_empty() {
        let blocks = (ne >> 10) + 1;
        let table = m.calloc(blocks, 4);
        m.set_u32(a + 8, table);
        for b in 0..blocks {
            let block = m.calloc(1024, 20);
            m.set_u32(table + 4 * b, block);
        }
        for (n, e) in mesh.edges.iter().enumerate().skip(1) {
            let n = n as u32;
            let r = m.u32(table + 4 * (n >> 10)) + 20 * (n & 0x3ff);
            m.set_i32(r, e.v0);
            m.set_i32(r + 4, e.v1);
            m.set_i32(r + 8, e.p0);
            m.set_i32(r + 12, e.p1);
            m.set_i16(r + 0x10, e.x10);
            m.set_i16(r + 0x12, e.flag);
        }
    }
    a
}

// ------------------------------------------------- what the set-up leaves, into the image

fn put_f32s(m: &mut Mem, a: u32, v: &[f32]) {
    for (i, x) in v.iter().enumerate() {
        m.set_f32(a + 4 * i as u32, *x);
    }
}

/// What `SI_materials` (0x08258e40) leaves in a material's record, stored where the original
/// stores it: the working block (+0x700 … +0x74c), the pairs of the two decay tables
/// (+0x68, +0x764), the flag word (+0x6e0, 32 bits, the 16 sign-extended) and the count of
/// 2D textures (+0x6e8). The reflection map (+0x6f4) is a pointer and the caller's; the two
/// jitter tables (+0x6b0, +0x6c0) come from chunk memory and are not stored.
pub fn material_setup_to_image(m: &mut Mem, a: u32, r: &ShadeMaterial) {
    let w = &r.work;
    put_f32s(m, a + 0x700, &w.base_ambient);
    put_f32s(m, a + 0x710, &w.ambient);
    put_f32s(m, a + 0x720, &w.diffuse);
    put_f32s(m, a + 0x730, &w.specular);
    put_f32s(m, a + 0x740, &[w.reflectivity, w.opacity, w.refraction, w.existence]);
    for (i, p) in r.spec_table.pairs.iter().enumerate() {
        put_f32s(m, a + 0x68 + 8 * i as u32, p);
    }
    m.set_u32(a + 0x6e0, r.flags as i16 as i32 as u32);
    m.set_i16(a + 0x6e8, r.n_maps);
    for (i, p) in r.blur_table.pairs.iter().enumerate() {
        put_f32s(m, a + 0x764 + 8 * i as u32, p);
    }
}

/// What the set-up leaves in a 2D texture's record (`SI_materials` and `setTxtFlag`: the
/// working factors; `SI_processTextures`: the region, the pixels per unit, the fractions
/// and the four flags; `SI_txtProjectionInfo`: the three floats at +0x7c, the centre, the
/// matrix and the cached box). The picture (+0x24) and the frame's file name (+0x20) are
/// pointers and the caller's.
pub fn map_setup_to_image(m: &mut Mem, a: u32, r: &ShadeMap) {
    put_f32s(m, a + 0x38, &[r.umin, r.vmin, r.umax, r.vmax, r.u2min, r.v2min, r.u2max, r.v2max]);
    put_f32s(m, a + 0x60, &[r.pix_u, r.pix_v]);
    put_f32s(m, a + 0x70, &[r.uoff_frac, r.voff_frac]);
    m.set_i16(a + 0x78, r.neg_u);
    m.set_i16(a + 0x7a, r.neg_v);
    put_f32s(m, a + 0x7c, &r.v7c);
    put_f32s(m, a + 0x88, &r.centre);
    for (k, row) in r.tmat.iter().enumerate() {
        put_f32s(m, a + 0xb8 + 16 * k as u32, row);
    }
    m.set_i16(a + 0x106, r.wrap_u);
    m.set_i16(a + 0x108, r.wrap_v);
    put_f32s(m, a + 0x148, &r.work);
    m.set_f32(a + 0x160, r.bump_amount);
    m.set_f32(a + 0x164, r.env_scale);
    put_f32s(m, a + 0x1a4, &r.box_min);
    put_f32s(m, a + 0x1b0, &r.box_max);
    m.set_i32(a + 0x1bc, r.box_node);
    m.set_f32(a + 0x1c0, r.box_r5);
    m.set_f32(a + 0x1c4, r.box_r6);
}

/// What `SI_materials` leaves in a 3D texture's record (the working factors, +0x128) and
/// `updateTxt3DTransf` in the texture made ready behind +0x8c (the switch at +0x04 and the
/// matrix at +0x08 of `SI_Txt3d`).
pub fn tex3d_setup_to_image(m: &mut Mem, a: u32, t: &ShadeTex3d) {
    put_f32s(m, a + 0x128, &t.work);
    let made = m.u32(a + 0x8c);
    if let (Some(txt), true) = (&t.txt, made != 0) {
        m.set_i16(made + 0x04, txt.has_tform);
        for (k, row) in txt.tform.iter().enumerate() {
            put_f32s(m, made + 0x08 + 16 * k as u32, row);
        }
    }
}

/// The members of a `glb_map` (0x28 bytes) that are not pointers: the crop switch and its
/// four bounds, the users, the size. The name (+0x00), the pixels (+0x1c) and the link
/// (+0x24) are the caller's.
pub fn glb_to_image(m: &mut Mem, a: u32, g: &crate::materials::GlbMap) {
    for (o, v) in [(0x04, g.crop), (0x06, g.crop_x0), (0x08, g.crop_x1), (0x0a, g.crop_y0), (0x0c, g.crop_y1), (0x14, g.users), (0x16, g.width), (0x18, g.height)] {
        m.set_i16(a + o, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_model::mesh::Mesh;
    use si_model::mesh_b::{update_nodes, MeshEnv};

    #[test]
    fn a_mesh_laid_out_reads_back_the_same() {
        let mut mesh = Mesh::with_vertices(&[[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.5], [-1.0, 1.0, 0.0], [0.0, 2.0, 0.0]]);
        mesh.push_polygon(&[0, 1, 2, 3]);
        mesh.push_polygon(&[3, 2, 4]);
        mesh.polygons[1].material = 1;
        update_nodes(&mut mesh, true, &mut MeshEnv::default());
        for p in &mut mesh.polygons {
            for (i, n) in p.nodes.iter_mut().enumerate() {
                n.uv = [i as f32 * 0.25, 0.5];
                n.extra_uv = vec![[0.125, 0.75], [1.0, i as f32]];
            }
        }
        // (the wire-frame lines of the interactive views are neither laid out nor read)
        mesh.display.clear();
        mesh.nb_display = 0;
        let mut m = Mem::new();
        let a = mesh_to_image(&mut m, &mesh, 2);
        assert_eq!(mesh_at(&m, a, 2), mesh);
        // without further sets the corners carry none
        let mut plain = mesh.clone();
        for p in &mut plain.polygons {
            for n in &mut p.nodes {
                n.extra_uv.clear();
            }
        }
        let b = mesh_to_image(&mut m, &mesh, 0);
        assert_eq!(mesh_at(&m, b, 0), plain);
        assert!(m.fault().is_none());
    }
}

#[cfg(test)]
mod lean_tests {
    use super::*;

    /// A small generator (xorshift) so the cases are the same every run.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn float(&mut self) -> f32 {
            match self.next() % 16 {
                0 => 0.0,
                1 => -0.0,
                2 => f32::NAN,
                3 => f32::INFINITY,
                4 => f32::NEG_INFINITY,
                5 => f32::from_bits(self.next() as u32),
                _ => ((self.next() % 20001) as f32 - 10000.0) / 1000.0,
            }
        }
    }

    /// An image with a triangle, its corners, a slice and a ray, filled from `rng`.
    fn world(rng: &mut Rng, tame: bool) -> (Mem, u32, u32) {
        let mut m = Mem::new();
        let tn = m.place(0x60, 4);
        let vs = [m.place(0x4c, 4), m.place(0x4c, 4), m.place(0x4c, 4)];
        let slice = m.place(0x28, 4);
        let ray = m.place(0x48, 4);
        let mut f = |rng: &mut Rng, span: f32| if tame { ((rng.next() % 2001) as f32 - 1000.0) / span } else { rng.float() };
        for (k, &v) in vs.iter().enumerate() {
            m.set_u32(tn + 4 * k as u32, v);
            for off in (0..0x4c).step_by(4) {
                m.set_f32(v + off, f(rng, 500.0));
            }
        }
        for off in (0x0c..0x4c).step_by(4) {
            m.set_f32(tn + off, f(rng, 500.0));
        }
        m.set_i16(tn + 0x20, (rng.next() % 5) as i16);
        let key = if tame { ((rng.next() % 4001) as f32 - 500.0) / 700.0 } else { rng.float() };
        m.set_f32(slice, key);
        m.set_u32(slice + 0x18, tn);
        for off in (0..0x48).step_by(4) {
            m.set_f32(ray + off, f(rng, 800.0));
        }
        m.set_f32(ray + 0x24, 100000.0);
        (m, ray, slice)
    }

    #[test]
    fn the_lean_reader_agrees_with_the_whole_records() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let (mut hits, mut misses) = (0, 0);
        for case in 0..60_000u32 {
            let seed = rng.0;
            let (mut a, ray, slice) = world(&mut rng, case % 4 != 0);
            rng.0 = seed;
            let (mut b, ray_b, slice_b) = world(&mut rng, case % 4 != 0);
            assert_eq!((ray, slice), (ray_b, slice_b));
            let (ra, rb) = (intersect2_at(&mut a, ray, slice), intersect2_at_whole(&mut b, ray, slice));
            assert_eq!(ra, rb, "case {case}");
            if ra == 0 {
                hits += 1;
            } else {
                misses += 1;
            }
            // the same bytes everywhere the case lives (the triangle, corners, slice, ray)
            assert_eq!(a.bytes(ray, 0x48), b.bytes(ray, 0x48), "case {case}");
            assert_eq!(a.bytes(slice, 0x28), b.bytes(slice, 0x28), "case {case}");
        }
        assert!(hits > 1000 && misses > 1000, "hits {hits}, misses {misses}");
    }
}
