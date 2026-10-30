// SPDX-License-Identifier: MIT
//! Four conversions of `softvertex.c` on the memory image (finding 0023, section 37): a metaball
//! surface as a mesh, a face as triangles, a mesh as triangles and a metaball surface as triangles.
//! They work on the original's records: `SI_Mesh` (vertices 0x14 bytes at +0, polygons 0x18 at +4,
//! the node pages at +8, counts at +0x14 / +0x18 / +0x1c, flags at +0x20), the render vertex (0x4c
//! bytes) and the render triangle `tn` (0x60 bytes). The routines of other files they call
//! (`si-model`'s mesh allocators and `SI_triangulateContour`, `si-meta`'s surface, `si-math`'s
//! matrix) are recorded hooks, as the callees of the other ports in this crate.
//! Class: model (the register arithmetic of the contour's transformation, the 0.001 tests).

use si_mem::Mem;
use si_x87::x;

si_mem::outside_calls! {
    /// What the conversions call in other source files.
    pub trait VertexCalls {
        /// `SI_metaSysGetSurface (0x0845b2c4)`, `metaball.c` (ported in `si-meta`): `(system, 1, detail)`.
        fn meta_sys_get_surface(system: u32, one: i32, detail: i32) = "SI_metaSysGetSurface", 0x0845b2c4;
        /// `SI_meshAllocate (0x08458c20)`, `mesh.c` (ported in `si-model`): a new empty mesh.
        fn mesh_allocate() -> u32 = "SI_meshAllocate", 0x08458c20;
        /// `SI_vertexAllocate (0x08459d48)`: `count` vertices of 0x14 bytes.
        fn vertex_allocate(count: u32) -> u32 = "SI_vertexAllocate", 0x08459d48;
        /// `SI_polygonAllocate (0x08459e20)`: `count` polygons of 0x18 bytes.
        fn polygon_allocate(count: u32) -> u32 = "SI_polygonAllocate", 0x08459e20;
        /// `SI_polygonInitialize (0x0845a694)`: `(polygon, corners, v2, v1, v0)`.
        fn polygon_initialize(poly: u32, corners: u32, a: u32, b: u32, c: u32) = "SI_polygonInitialize", 0x0845a694;
        /// `SI_meshUpdateNodes (0x08458cf8)`.
        fn mesh_update_nodes(mesh: u32, flag: u32) = "SI_meshUpdateNodes", 0x08458cf8;
        /// `SI_faceConvertToMesh (0x08435960)`, `face.c` (ported in `si-model`).
        fn face_convert_to_mesh(face: u32, flag: u32) -> u32 = "SI_faceConvertToMesh", 0x08435960;
        /// `SI_meshDispose (0x084404a0)` (ported in `si-model`): the address of the mesh pointer.
        fn mesh_dispose(pmesh: u32) = "SI_meshDispose", 0x084404a0;
        /// `SI_meshNbTriangles (0x08444d9c)` (ported in `si-model`).
        fn mesh_nb_triangles(mesh: u32) -> i32 = "SI_meshNbTriangles", 0x08444d9c;
        /// `SI_matrixZAlign (0x08518aa0)` (ported in `si-math`): `(vector, matrix out)`.
        fn matrix_z_align(v: u32, out: u32) = "SI_matrixZAlign", 0x08518aa0;
        /// `SI_triangulateContour (0x0851dadc)` (ported in `si-model`): `(n, points, corners, map, &count, &triangles, 1)`.
        fn triangulate_contour(n: i32, pts: u32, corners: u32, map: u32, count: u32, tris: u32, one: i32) -> i32 = "SI_triangulateContour", 0x0851dadc;
        /// `si_fatal (0x08260560)`, `renutil.c`: `(text, more, seconds)`.
        fn si_fatal(first: u32, more: u32, seconds: u32) = "si_fatal", 0x08260560;
    }
}

const IN_ALLOC: u32 = 0x0879_1228;
/// The tolerance of the discontinuity tests (a double at 0x086543f8).
const TOLERANCE: f64 = 0.001;

/// The node (0x14 bytes) of a mesh by its number: pages of 1024 behind the pointer at +8.
fn node(m: &Mem, mesh: u32, n: i32) -> u32 {
    let page = m.u32(m.u32(mesh + 8).wrapping_add(((n >> 10) as u32).wrapping_mul(4)));
    page.wrapping_add(((n & 0x3ff) as u32).wrapping_mul(0x14))
}

/// `|n|` as the original computes it (negation of the word).
fn abs(n: i32) -> i32 {
    if n < 0 {
        n.wrapping_neg()
    } else {
        n
    }
}

/// A metaball system's surface as a mesh [soft 0x0826bf7c SI_meta2Mesh]: `SI_meta2Mesh(system)`;
/// the surface is made (`SI_metaSysGetSurface(system, 1, 0)`), a mesh allocated (null answers null);
/// the system's vertex table (+0x50, count +0x4c, 0x48 bytes each: position at +0 and normal at +0x18,
/// doubles) becomes the mesh's vertices (positions as floats), its triangles (+0x58, count +0x54, 0x20
/// bytes each with a pointer to three indices at +4) the mesh's polygons of three corners
/// (`SI_polygonInitialize(poly, 3, i2, i1, i0)`: the order reversed), each corner's normal (+8, +0x2c,
/// +0x50 of the corner records, 0x24 apart) the normal of the vertex of that index, as floats. Last
/// `SI_meshUpdateNodes(mesh, 0)`. Class: copy (a double stored as a float).
pub fn meta_to_mesh(m: &mut Mem, o: &mut dyn si_mem::Outside, system: u32) -> u32 {
    use VertexCalls as _;
    o.meta_sys_get_surface(m, system, 1, 0);
    let mesh = o.mesh_allocate(m);
    if mesh == 0 {
        return mesh;
    }
    let nv = m.u32(system + 0x4c);
    m.set_u32(mesh + 0x14, nv);
    let verts = o.vertex_allocate(m, nv);
    m.set_u32(mesh, verts);
    let src = m.u32(system + 0x50);
    for i in 0..nv as i32 {
        let v = m.u32(mesh).wrapping_add((i as u32).wrapping_mul(0x14));
        let s = src.wrapping_add((i as u32).wrapping_mul(0x48));
        for k in 0..3u32 {
            let f = m.f64(s + 8 * k) as f32;
            m.set_f32(v + 4 * k, f);
        }
    }
    let np = m.u32(system + 0x54);
    m.set_u32(mesh + 0x18, np);
    let polys = o.polygon_allocate(m, np);
    m.set_u32(mesh + 4, polys);
    let tri = m.u32(system + 0x58);
    for i in 0..np as i32 {
        let t = m.u32(tri.wrapping_add((i as u32).wrapping_mul(0x20)).wrapping_add(4));
        let poly = m.u32(mesh + 4).wrapping_add((i as u32).wrapping_mul(0x18));
        let (i0, i1, i2) = (m.u32(t), m.u32(t + 4), m.u32(t + 8));
        o.polygon_initialize(m, poly, 3, i2, i1, i0);
        let corners = m.u32(m.u32(mesh + 4).wrapping_add((i as u32).wrapping_mul(0x18)).wrapping_add(0xc));
        for (c, idx) in [(0u32, i2), (1, i1), (2, i0)] {
            let s = src.wrapping_add(idx.wrapping_mul(0x48));
            for k in 0..3u32 {
                let f = m.f64(s + 0x18 + 8 * k) as f32;
                m.set_f32(corners.wrapping_add(8 + 0x24 * c + 4 * k), f);
            }
        }
    }
    o.mesh_update_nodes(m, mesh, 0);
    mesh
}

/// A face as render triangles [soft 0x0826c144 SI_convertFace2triangle]:
/// `(face, face2, material, flag, &count, &vertices, &n, &triangles)` clears the four outputs, converts
/// `face2` to a mesh (`SI_faceConvertToMesh(face2, 0)`), and if that worked converts the mesh to
/// triangles (`SI_convertMesh2Triangles(mesh, mesh2, material != 0, material, flag, ..)` with `mesh` a
/// conversion of `face`, unless both faces are the same one, whose mesh is then used twice) and
/// disposes the meshes. Class: copy.
#[allow(clippy::too_many_arguments)]
pub fn convert_face_to_triangles(
    m: &mut Mem,
    o: &mut dyn si_mem::Outside,
    face: u32,
    face2: u32,
    material: u32,
    flag: i32,
    count: u32,
    vertices: u32,
    nvert: u32,
    triangles: u32,
) {
    use VertexCalls as _;
    m.set_u32(vertices, 0);
    m.set_u32(count, 0);
    m.set_u32(triangles, 0);
    m.set_u32(nvert, 0);
    let slot2 = m.local(4);
    let mesh2 = o.face_convert_to_mesh(m, face2, 0);
    m.set_u32(slot2, mesh2);
    if mesh2 == 0 {
        return;
    }
    if face != face2 {
        let slot1 = m.local(4);
        let mesh1 = o.face_convert_to_mesh(m, face, 0);
        m.set_u32(slot1, mesh1);
        convert_mesh_to_triangles(m, o, mesh1, mesh2, (material != 0) as i32, material, flag, count, vertices, nvert, triangles);
        o.mesh_dispose(m, slot1);
    } else {
        convert_mesh_to_triangles(m, o, mesh2, mesh2, (material != 0) as i32, material, flag, count, vertices, nvert, triangles);
    }
    o.mesh_dispose(m, slot2);
}

/// A mesh as render triangles [soft 0x0826c220 SI_convertMesh2Triangles]:
/// `(mesh1, mesh2, nmat, material, flag, &nvert, &vertices, &ntri_vertices.., &triangles)`. `mesh1` supplies
/// the second position of every render vertex (+0x20), `mesh2` the structure.
///
/// 1. Node flags (word at +0x12 of the 0x14-byte node): bit 15 cleared on all; set again where one of
///    the bits 0x38 is; set on all when the mesh's flag byte (+0x20) has bit 1; then, for every node
///    without bit 15 and bit 0x20 whose two polygons (+8, +0xc) exist, the corner of each polygon at the
///    node's first and then second vertex are compared: texture coordinates (+0x14, +0x18 of the corner)
///    farther apart than 0.001 (in the register) or the word at +0x1c different set bit 7 of the
///    byte at +0x13 (the seam).
/// 2. The render vertices: every vertex's references (0xc bytes, count at +0x10 of the vertex) are
///    walked in rings (a ring ends at an entry whose first word is -1 or at the count); a new render vertex is
///    made at the ring's first usable entry and at every entry whose node has bit 15 set; the entry map
///    says which render vertex each reference belongs to. A first pass counts, the second fills: the
///    vertex position (+8), the second position (+0x20), the corner's normal (+0x14) and
///    1 - u (+0x00), v (+4) and the word at +0x44, or the polygon's normal when the mesh's flag bit 1 is set.
/// 3. The triangles: `SI_meshNbTriangles` of them are chained (0x60 bytes, +0x5c next, +0x58 the flag
///    word, +0x4c the material); every polygon's distinct vertices are gathered (blocks of 1000 grown by
///    reallocation), turned flat by `SI_matrixZAlign` of the polygon's normal (x87: ((x*m0 + y*m1) + z*m2) + m3
///    per component), triangulated by `SI_triangulateContour`, and each triangle's three render vertices
///    looked up through the map; a triangle with a missing vertex is dropped.
///
/// Class: model. The original reads a block past the reference list of a vertex with a ring and
/// no end marker (not made by the generator).
#[allow(clippy::too_many_arguments)]
pub fn convert_mesh_to_triangles(
    m: &mut Mem,
    o: &mut dyn si_mem::Outside,
    mesh1: u32,
    mesh2: u32,
    nmat: i32,
    material: u32,
    flag: i32,
    p_nvert: u32,
    p_verts: u32,
    p_ntri: u32,
    p_tris: u32,
) {
    use VertexCalls as _;
    let nnodes = m.i32(mesh2 + 0x1c);
    // --- node flags
    for n in 1..=nnodes {
        let a = node(m, mesh2, n) + 0x12;
        let v = m.u16(a) & 0x7fff;
        m.set_u16(a, v);
    }
    for n in 1..=nnodes {
        let a = node(m, mesh2, n) + 0x12;
        let v = m.u16(a);
        if v & 0x38 != 0 {
            m.set_u16(a, v | 0x8000);
        }
    }
    if m.u8(mesh2 + 0x20) & 2 != 0 {
        for n in 1..=nnodes {
            let a = node(m, mesh2, n) + 0x13;
            let v = m.u8(a) | 0x80;
            m.set_u8(a, v);
        }
    }
    let eps = x(TOLERANCE);
    for n in 1..=nnodes {
        let nd = node(m, mesh2, n);
        let fl = m.u16(nd + 0x12);
        let (pa, pb) = (m.i32(nd + 8), m.i32(nd + 0xc));
        if (fl as i16) < 0 || fl & 0x20 != 0 || pa == -1 || pb == -1 {
            continue;
        }
        let polys = m.u32(mesh2 + 4);
        let a = polys.wrapping_add((pa as u32).wrapping_mul(0x18));
        let b = polys.wrapping_add((pb as u32).wrapping_mul(0x18));
        for which in [0u32, 4] {
            let vtx = m.u32(nd + which);
            let find = |m: &Mem, p: u32| -> u32 {
                let cnt = u32::from(m.u16(p + 0x10));
                let mut i = 0u32;
                if cnt != 0 {
                    let mut c = m.u32(p + 0xc);
                    if m.u32(c) != vtx {
                        loop {
                            c = c.wrapping_add(0x24);
                            i += 1;
                            if i as i32 >= cnt as i32 {
                                break;
                            }
                            if m.u32(c) == vtx {
                                break;
                            }
                        }
                    }
                }
                i
            };
            let ia = find(m, a);
            let ib = find(m, b);
            if ia == u32::from(m.u16(a + 0x10)) || ib == u32::from(m.u16(b + 0x10)) {
                continue;
            }
            let ca = m.u32(a + 0xc).wrapping_add(ia.wrapping_mul(0x24));
            let cb = m.u32(b + 0xc).wrapping_add(ib.wrapping_mul(0x24));
            let far = |off: u32| (x(m.f32(ca + off)) - x(m.f32(cb + off))).abs().gt(eps);
            if far(0x14) || far(0x18) || m.u32(ca + 0x1c) != m.u32(cb + 0x1c) {
                let q = m.u8(nd + 0x13) | 0x80;
                m.set_u8(nd + 0x13, q);
            }
        }
    }
    let ntri_total = o.mesh_nb_triangles(m, mesh2);
    let nverts = m.i32(mesh2 + 0x14);

    // The rings of one vertex's references: the closure visits (ring start, last, pivot) per ring.
    // refs: base of the 0xc-byte entries; cnt: their number.
    let pivot_of = |m: &Mem, refs: u32, cnt: i32, start: i32, k: i32| -> i32 {
        let last = k - 1;
        let mut pivot = start;
        if start == 0 && k == cnt {
            let mut j = 0;
            while j <= last {
                let id = abs(m.i32(refs.wrapping_add((j as u32).wrapping_mul(0xc)).wrapping_add(4)));
                if (m.u16(node(m, mesh2, id) + 0x12) as i16) < 0 {
                    pivot = j;
                    break;
                }
                j += 1;
            }
        }
        pivot
    };

    // --- first pass: count the render vertices
    let mut total: i32 = 0;
    let mut v = m.u32(mesh2);
    for _ in 0..nverts {
        let cnt = i32::from(m.u16(v + 0x10));
        if cnt != 0 {
            let refs = m.u32(v + 0xc);
            let mut start = 0;
            let mut k = 0;
            while k <= cnt {
                if k == cnt || m.i32(refs.wrapping_add((k as u32).wrapping_mul(0xc))) == -1 {
                    let last = k - 1;
                    let pivot = pivot_of(m, refs, cnt, start, k);
                    let mut cur = pivot;
                    loop {
                        let e = refs.wrapping_add((cur as u32).wrapping_mul(0xc));
                        if m.i32(e + 8) != -1 {
                            let count_it = if cur != pivot {
                                let id = abs(m.i32(e + 4));
                                (m.u16(node(m, mesh2, id) + 0x12) as i16) < 0
                            } else {
                                true
                            };
                            if count_it {
                                total += 1;
                            }
                        }
                        cur = if cur != last { cur + 1 } else { start };
                        if cur == pivot {
                            break;
                        }
                    }
                    start = k + 1;
                }
                k += 1;
            }
        }
        v = v.wrapping_add(0x14);
    }
    m.set_u32(p_nvert, total as u32);
    let rverts = m.si_calloc(total as u32, 0x4c);
    m.set_u32(p_verts, rverts);
    for i in 0..total {
        m.set_u32(rverts.wrapping_add((i as u32).wrapping_mul(0x4c)).wrapping_add(0x40), 0);
    }
    let maps = m.si_calloc(nverts as u32, 4);
    let mut made: i32 = -1;

    // --- second pass: fill them and map every reference to its render vertex
    let mut v = m.u32(mesh2);
    for vi in 0..nverts {
        let cnt = i32::from(m.u16(v + 0x10));
        let map = m.si_calloc(cnt as u32, 4);
        m.set_u32(maps.wrapping_add((vi as u32).wrapping_mul(4)), map);
        for j in 0..cnt {
            m.set_u32(map.wrapping_add((j as u32).wrapping_mul(4)), u32::MAX);
        }
        if cnt != 0 {
            let refs = m.u32(v + 0xc);
            let mut start = 0;
            let mut k = 0;
            while k <= cnt {
                if k == cnt || m.i32(refs.wrapping_add((k as u32).wrapping_mul(0xc))) == -1 {
                    let last = k - 1;
                    let pivot = pivot_of(m, refs, cnt, start, k);
                    let mut cur = pivot;
                    loop {
                        let e = m.u32(v + 0xc).wrapping_add((cur as u32).wrapping_mul(0xc));
                        if m.i32(e + 8) != -1 {
                            let create = if cur != pivot {
                                let id = abs(m.i32(e + 4));
                                (m.u16(node(m, mesh2, id) + 0x12) as i16) < 0
                            } else {
                                true
                            };
                            if create {
                                made += 1;
                                let r = rverts.wrapping_add((made as u32).wrapping_mul(0x4c));
                                for w in 0..3 {
                                    let val = m.u32(v + 4 * w);
                                    m.set_u32(r + 8 + 4 * w, val);
                                }
                                let p1 = m.u32(mesh1).wrapping_add((vi as u32).wrapping_mul(0x14));
                                for w in 0..3 {
                                    let val = m.u32(p1 + 4 * w);
                                    m.set_u32(r + 0x20 + 4 * w, val);
                                }
                                let poly = m.u32(mesh2 + 4).wrapping_add(m.u32(e + 8).wrapping_mul(0x18));
                                let pc = u32::from(m.u16(poly + 0x10));
                                let mut c = m.u32(poly + 0xc);
                                for _ in 0..pc {
                                    if m.u32(c) == vi as u32 {
                                        for w in 0..3 {
                                            let val = m.u32(c + 8 + 4 * w);
                                            m.set_u32(r + 0x14 + 4 * w, val);
                                        }
                                        let u = (x(1.0f32) - x(m.f32(c + 0x14))).to_f32();
                                        m.set_f32(r, u);
                                        let val = m.u32(c + 0x18);
                                        m.set_u32(r + 4, val);
                                        let val = m.u32(c + 0x1c);
                                        m.set_u32(r + 0x44, val);
                                        break;
                                    }
                                    c = c.wrapping_add(0x24);
                                }
                                if m.u8(mesh2 + 0x20) & 2 != 0 {
                                    for w in 0..3 {
                                        let val = m.u32(poly + 4 * w);
                                        m.set_u32(r + 0x14 + 4 * w, val);
                                    }
                                }
                            }
                        }
                        let map = m.u32(maps.wrapping_add((vi as u32).wrapping_mul(4)));
                        m.set_u32(map.wrapping_add((cur as u32).wrapping_mul(4)), made as u32);
                        cur = if cur != last { cur + 1 } else { start };
                        if cur == pivot {
                            break;
                        }
                    }
                    start = k + 1;
                }
                k += 1;
            }
        }
        v = v.wrapping_add(0x14);
    }

    // --- the triangles
    m.set_u32(p_ntri, ntri_total as u32);
    let tris = m.si_calloc(ntri_total as u32, 0x60);
    m.set_u32(p_tris, tris);
    for i in 0..ntri_total {
        let t = tris.wrapping_add((i as u32).wrapping_mul(0x60));
        m.set_u32(t, 0);
        m.set_u32(t + 4, 0);
        m.set_u32(t + 8, 0);
        m.set_u32(t + 0x5c, if i == ntri_total - 1 { 0 } else { t.wrapping_add(0x60) });
        m.set_u16(t + 0x58, flag as u16);
        m.set_u32(t + 0x4c, material);
    }
    let mut uniq = m.si_calloc(1000, 4); // distinct vertex numbers
    let mut cmap = m.si_calloc(1000, 4); // corner -> distinct index
    let mut pts = m.si_calloc(1000, 0xc); // their positions
    let mut blocks: i32 = 1;
    let mut kept: i32 = 0;
    let npolys = m.i32(mesh2 + 0x18);
    let mat = m.local(64);
    let cnt_slot = m.local(4);
    let tri_slot = m.local(4);
    let mut cur_tri: u32 = 0; // byte offset of the next triangle to fill
    let mut poly = m.u32(mesh2 + 4);
    for pi in 0..npolys {
        let ncorner = i32::from(m.u16(poly + 0x10));
        let mut nuniq: i32 = 0;
        if ncorner > 0x3e6 {
            cmap = m.si_realloc(cmap, (ncorner as u32).wrapping_mul(4).wrapping_add(4));
        }
        if ncorner != 0 {
            let mut cap_bytes = (blocks as u32).wrapping_mul(4000);
            let mut cap_pts = (blocks as u32).wrapping_mul(12000);
            let mut cap_idx = blocks.wrapping_mul(1000).wrapping_sub(1);
            for c in 0..ncorner {
                let vtx = m.i32(m.u32(poly + 0xc).wrapping_add((c as u32).wrapping_mul(0x24)));
                let slot = cmap.wrapping_add((c as u32).wrapping_mul(4));
                if vtx == -1 {
                    m.set_u32(slot, u32::MAX);
                    continue;
                }
                let mut j: i32 = 0;
                if nuniq > 0 {
                    let mut at = m.i32(uniq);
                    while at != vtx {
                        j += 1;
                        if j >= nuniq {
                            break;
                        }
                        at = m.i32(uniq.wrapping_add((j as u32).wrapping_mul(4)));
                    }
                }
                if j == nuniq {
                    if cap_idx <= j {
                        cap_bytes = cap_bytes.wrapping_add(4000);
                        cap_pts = cap_pts.wrapping_add(12000);
                        cap_idx = cap_idx.wrapping_add(1000);
                        blocks += 1;
                        uniq = m.si_realloc(uniq, cap_bytes);
                        pts = m.si_realloc(pts, cap_pts);
                    }
                    let dst = pts.wrapping_add((j as u32).wrapping_mul(0xc));
                    let src = m.u32(mesh2).wrapping_add((vtx as u32).wrapping_mul(0x14));
                    for w in 0..3 {
                        let val = m.u32(src + 4 * w);
                        m.set_u32(dst + 4 * w, val);
                    }
                    m.set_u32(uniq.wrapping_add((j as u32).wrapping_mul(4)), vtx as u32);
                    nuniq = j + 1;
                }
                m.set_u32(slot, j as u32);
            }
        }
        o.matrix_z_align(m, poly, mat);
        let mt: [[f32; 4]; 4] = std::array::from_fn(|r| std::array::from_fn(|c| m.f32(mat + 16 * r as u32 + 4 * c as u32)));
        let mut pp = pts;
        for _ in 0..nuniq {
            let (px, py, pz) = (x(m.f32(pp)), x(m.f32(pp + 4)), x(m.f32(pp + 8)));
            // ((x*m[0][c] + y*m[1][c]) + z*m[2][c]) + m[3][c], kept in the register until stored
            let comp = |c: usize| (px * x(mt[0][c]) + py * x(mt[1][c]) + pz * x(mt[2][c]) + x(mt[3][c])).to_f32();
            let (a, b, d) = (comp(0), comp(1), comp(2));
            m.set_f32(pp, a);
            m.set_f32(pp + 4, b);
            m.set_f32(pp + 8, d);
            pp = pp.wrapping_add(0xc);
        }
        let ok = o.triangulate_contour(m, nuniq, pts, ncorner as u32, cmap, cnt_slot, tri_slot, 1) as i16;
        if ok != 0 {
            let nt = m.i32(cnt_slot);
            let list = m.u32(tri_slot);
            for ti in 0..nt {
                let t = tris.wrapping_add(cur_tri);
                for k in 0..3u32 {
                    let idx = m.u32(list.wrapping_add((ti as u32).wrapping_mul(12)).wrapping_add(4 * k)) as usize as u32;
                    let vnum = m.u32(uniq.wrapping_add(idx.wrapping_mul(4)));
                    let vr = m.u32(mesh2).wrapping_add(vnum.wrapping_mul(0x14));
                    let nrefs = u32::from(m.u16(vr + 0x10));
                    for e in 0..nrefs {
                        let ent = m.u32(vr + 0xc).wrapping_add(e * 0xc);
                        if m.i32(ent + 8) == pi {
                            let map = m.u32(maps.wrapping_add(vnum.wrapping_mul(4)));
                            let rv = m.u32(map.wrapping_add(e * 4));
                            m.set_u32(t + 4 * k, rverts.wrapping_add(rv.wrapping_mul(0x4c)));
                        }
                    }
                }
                let sw = m.i16(poly + 0x12);
                if sw >= 0 && i32::from(sw) < nmat {
                    m.set_u32(t + 0x4c, material);
                }
                if m.u32(t) == 0 || m.u32(t + 4) == 0 || m.u32(t + 8) == 0 {
                    m.set_u32(t, 0);
                    m.set_u32(t + 4, 0);
                    m.set_u32(t + 8, 0);
                    m.set_u32(t + 0x4c, 0);
                } else {
                    cur_tri = cur_tri.wrapping_add(0x60);
                    kept += 1;
                }
            }
            if list != 0 {
                m.free(list);
                m.set_u32(tri_slot, 0);
            }
        }
        poly = poly.wrapping_add(0x18);
    }
    m.set_u32(p_ntri, kept as u32);
    if kept != 0 {
        m.set_u32(tris.wrapping_add((kept as u32).wrapping_mul(0x60)).wrapping_sub(4), 0);
    }
    for n in 1..=nnodes {
        let a = node(m, mesh2, n) + 0x12;
        let v = m.u16(a) & 0x7fff;
        m.set_u16(a, v);
    }
    for vi in 0..nverts {
        let slot = maps.wrapping_add((vi as u32).wrapping_mul(4));
        let p = m.u32(slot);
        if p != 0 {
            m.free(p);
            m.set_u32(slot, 0);
        }
    }
    if maps != 0 {
        m.free(maps);
    }
    if uniq != 0 {
        m.free(uniq);
    }
    if cmap != 0 {
        m.free(cmap);
    }
    if pts != 0 {
        m.free(pts);
    }
}

/// A metaball surface as render triangles [soft 0x0826d2f8 SI_convertMeta2Triangles]:
/// `(system, material, flag, &nvert, &vertices, &ntri, &triangles, detail)`. The surface is made
/// (`SI_metaSysGetSurface(system, 1, detail)`); with no triangle table (+0x58) nothing more happens.
/// Otherwise the vertices (0x4c bytes: position at +8 and +0x20 as floats, normal at +0x14, +0x40 = 0,
/// +0x44 = 0xffb2b2b2) and the triangles (0x60 bytes, chained, flag word and material set, the three
/// vertex pointers from the table's three indices reversed) are made in blocks of `SI_malloc` (the heap's
/// failure goes to `si_fatal`). Class: copy.
#[allow(clippy::too_many_arguments)]
pub fn convert_meta_to_triangles(
    m: &mut Mem,
    o: &mut dyn si_mem::Outside,
    system: u32,
    material: u32,
    flag: i32,
    p_nvert: u32,
    p_verts: u32,
    p_ntri: u32,
    p_tris: u32,
    detail: i16,
) {
    use VertexCalls as _;
    o.meta_sys_get_surface(m, system, 1, i32::from(detail));
    let src = m.u32(system + 0x50);
    let table = m.u32(system + 0x58);
    if table == 0 {
        return;
    }
    let nv = m.u32(system + 0x4c);
    m.set_u32(p_nvert, nv);
    m.set_u32(IN_ALLOC, 1);
    let size = m.u32(p_nvert).wrapping_mul(0x4c);
    let size = (size.wrapping_add(3)) & !3;
    let out = m.si_malloc(size);
    if out == 0 {
        o.si_fatal(m, 0x0869_c96a, 0x0869_c940, 6);
    }
    m.set_u32(IN_ALLOC, 0);
    m.set_u32(p_verts, out);
    for i in 0..m.i32(p_nvert) {
        let r = out.wrapping_add((i as u32).wrapping_mul(0x4c));
        let s = src.wrapping_add((i as u32).wrapping_mul(0x48));
        for k in 0..3u32 {
            let f = m.f64(s + 8 * k) as f32;
            m.set_f32(r + 8 + 4 * k, f);
            m.set_f32(r + 0x14 + 4 * k, m.f64(s + 0x18 + 8 * k) as f32);
        }
        for k in 0..3u32 {
            let val = m.u32(r + 8 + 4 * k);
            m.set_u32(r + 0x20 + 4 * k, val);
        }
        m.set_u32(r + 0x40, 0);
        m.set_u32(r + 0x44, 0xffb2_b2b2);
    }
    let nt = m.u32(system + 0x54);
    m.set_u32(p_ntri, nt);
    m.set_u32(IN_ALLOC, 1);
    let size = m.u32(p_ntri).wrapping_mul(0x60);
    let size = (size.wrapping_add(3)) & !3;
    let tris = m.si_malloc(size);
    if tris == 0 {
        o.si_fatal(m, 0x0869_c9aa, 0x0869_c980, 6);
    }
    m.set_u32(IN_ALLOC, 0);
    m.set_u32(p_tris, tris);
    let count = m.i32(p_ntri);
    for i in 0..count {
        let t = tris.wrapping_add((i as u32).wrapping_mul(0x60));
        m.set_u32(t + 0x5c, if i < count - 1 { t.wrapping_add(0x60) } else { 0 });
        m.set_u16(t + 0x58, flag as u16);
        m.set_u32(t + 0x4c, material);
        let idx = m.u32(table.wrapping_add((i as u32).wrapping_mul(0x20)).wrapping_add(4));
        for k in 0..3u32 {
            let vi = m.u32(idx + 8 - 4 * k);
            m.set_u32(t + 4 * k, out.wrapping_add(vi.wrapping_mul(0x4c)));
        }
    }
}
