// SPDX-License-Identifier: MIT
//! The mesh drivers of `boolean.c` (finding 0037): `si_generateFaceList` and later `si_buildMesh`, `si_buildMesh2`.

use crate::boolean::*;
use si_mem::{Mem, Outside};

pub const SI_MESH_TRANSFORM: Routine = Routine::new("SI_meshTransform", 0x0844_1cd8);
pub const SI_MESH_PLANAR: Routine = Routine::new("SI_meshPlanar", 0x0845_8848);
pub const SI_MESH_TRIANGULATE: Routine = Routine::new("SI_meshTriangulate", 0x0844_3144);
/// The function-static counter of `si_generateFaceList` (`fid.437`): the ids of the faces it makes.
pub const FID: u32 = 0x086b_9858;

use si_mem::Routine;

fn r(m: &Mem, a: u32) -> u32 {
    m.u32(a)
}
fn w(m: &mut Mem, a: u32, v: u32) {
    m.set_u32(a, v)
}

/// `si_generateFaceList(mesh, flag, matrix_a, matrix_b)` [soft 0x08312f58 si_generateFaceList]: builds the pool
/// points, edges and faces of a mesh (points at `+0`, stride 0x14; polygons at `+4`, stride 0x18; the edges in
/// blocks of 1024 through the table at `+8`; counts at `+0x14`, `+0x18`, `+0x1c`) and answers the list of faces
/// with more than two nodes in the first contour. With `flag == 1` the mesh is first transformed, and
/// triangulated (cleanup on) when it is not planar.
pub fn si_generate_face_list(m: &mut Mem, o: &mut dyn Outside, mesh: u32, flag: i32, pa: u32, pb: u32) -> u32 {
    if flag == 1 {
        call(m, o, SI_MESH_TRANSFORM, &[mesh, pa, pb]);
        let planar = call(m, o, SI_MESH_PLANAR, &[mesh]) & 0xffff;
        if planar == 0 {
            call(m, o, SI_MESH_TRIANGULATE, &[mesh, 0]);
            set_cleanup(m, 1, 1);
        }
    }
    let pt_base = m[CUR_NB_POINTS];
    let edge_base = m[CUR_NB_EDGES];
    let npts = m.i32(mesh + 0x14);
    let fr = m.local(0x48);
    // phase 1: the points
    for i in 0..npts.max(0) as u32 {
        let src = r(m, mesh) + i * 0x14;
        let v = [m.f32(src) as f64, m.f32(src + 4) as f64, m.f32(src + 8) as f64];
        let (found, idx) = locate_pt(m, v);
        let pt = get_point(m, o);
        m.set_f64(pt, v[0]);
        m.set_f64(pt + 8, v[1]);
        m.set_f64(pt + 0x10, v[2]);
        let tag = m.u16(src + 0x12);
        m.set_u16(pt + p::CNT, tag);
        m.set_u8(pt + p::FLAG, 0);
        w(m, pt + p::PLANE, 0);
        if found != 0 {
            w(m, pt + p::PLANE, (idx as u32).wrapping_add(1));
        }
    }
    // phase 2: the edges
    let nedges = m.i32(mesh + 0x1c);
    let entry = |m: &Mem, i: u32| -> u32 { r(m, r(m, mesh + 8) + (i >> 10) * 4) + (i & 0x3ff) * 0x14 };
    let mut i = 1u32;
    if nedges > 0 {
        loop {
            let ed = get_edge(m, o);
            let en = entry(m, i);
            let p0 = find_point_id(m, (pt_base as u32).wrapping_add(r(m, en)) as i32);
            w(m, ed + e::PT, p0);
            if r(m, p0 + p::PLANE) != 0 {
                let q = find_point_id(m, r(m, p0 + p::PLANE).wrapping_sub(1) as i32);
                w(m, ed + e::PT, q);
            }
            let p1 = find_point_id(m, (pt_base as u32).wrapping_add(r(m, en + 4)) as i32);
            w(m, ed + e::PT + 4, p1);
            if r(m, p1 + p::PLANE) != 0 {
                let q = find_point_id(m, r(m, p1 + p::PLANE).wrapping_sub(1) as i32);
                w(m, ed + e::PT + 4, q);
            }
            let tag = m.u16(en + 0x12);
            m.set_u16(ed + 4, tag);
            m.set_u8(ed + 6, 0);
            w(m, ed, 0xffff_ffff);
            i += 1;
            if i as i32 > nedges {
                break;
            }
        }
    }
    // phase 3: the faces
    let mut head = 0u32;
    let mut tail = 0u32;
    let npolys = m.i32(mesh + 0x18);
    let mut k = 0u32;
    while (k as i32) < npolys {
        let poly = r(m, mesh + 4) + k * 0x18;
        let face = call(m, o, SI_CREATE_BOOL_FACE, &[0]);
        w(m, face + 0x58, m.i16(poly + 0x14) as i32 as u32);
        w(m, face + f::COUNT, m.i16(poly + 0x12) as i32 as u32);
        m.set_u8(face + f::FLAG, flag as u8);
        let id = r(m, FID);
        w(m, face + f::ID, id);
        w(m, FID, id.wrapping_add(1));
        w(m, face + 0x7c, k);
        let mut con = call(m, o, SI_CREATE_CONTOUR, &[0]);
        w(m, face + f::CONTOURS, con);
        let mut seg_start = 0i32;
        let nn = m.u16(poly + 0x10) as i32;
        k += 1;
        let nodes = r(m, poly + 0xc);
        let mut j = 0i32;
        while j < nn {
            let nd = nodes.wrapping_add((j as u32).wrapping_mul(0x24));
            let a = m.i32(nd);
            let mut skip = false;
            if a < 0 {
                con = call(m, o, SI_CREATE_CONTOUR, &[0]);
                seg_start = j + 1;
                let fc = r(m, face + f::CONTOURS);
                let nx = r(m, fc + 8);
                w(m, con + 8, nx);
                if nx != 0 {
                    w(m, nx + 4, con);
                }
                w(m, con + 4, fc);
                w(m, fc + 8, con);
            } else {
                let mut b = m.i32(nd + 4);
                let end: u32;
                if b < 0 {
                    end = 1;
                    b = b.wrapping_neg();
                } else if b < 1 {
                    let nb: i32;
                    let total = m.i32(mesh + 0x1c);
                    let nxt_ok = j + 1 < nn && m.i32(nd + 0x24) != -1;
                    if nxt_ok {
                        nb = m.i32(nd + 0x24);
                    } else {
                        let mut q = j - 1;
                        if q > -1 && m.i32(nodes.wrapping_add((q as u32).wrapping_mul(0x24))) != -1 {
                            let mut pn = (q as u32).wrapping_mul(0x24);
                            loop {
                                pn = pn.wrapping_sub(0x24);
                                q -= 1;
                                if q < 0 {
                                    break;
                                }
                                if m.i32(nodes.wrapping_add(pn)) == -1 {
                                    break;
                                }
                            }
                        }
                        nb = m.i32(nodes.wrapping_add(((q * 9 + 9) as u32).wrapping_mul(4)));
                    }
                    let mut found_id = 0u32;
                    let mut e_end = 0u32;
                    let mut idx = 1i32;
                    while idx <= total {
                        let en = entry(m, idx as u32);
                        found_id = idx as u32;
                        if a == m.i32(en) && nb == m.i32(en + 4) {
                            e_end = 0;
                            break;
                        }
                        if a == m.i32(en + 4) && nb == m.i32(en) {
                            e_end = 1;
                            break;
                        }
                        idx += 1;
                    }
                    if idx > total {
                        found_id = 0;
                    }
                    b = found_id as i32;
                    end = e_end;
                    if b == 0 {
                        skip = true;
                    }
                } else {
                    end = 0;
                }
                if !skip {
                    let mut other = j + 1;
                    let edge = find_edge_id(m, edge_base.wrapping_sub(1).wrapping_add(b));
                    if nn <= other || m.i32(nodes.wrapping_add((j as u32).wrapping_mul(0x24)).wrapping_add(0x24)) < 0 {
                        other = seg_start;
                    }
                    let n0 = nodes.wrapping_add((j as u32).wrapping_mul(0x24));
                    let n1 = nodes.wrapping_add((other as u32).wrapping_mul(0x24));
                    let v0 = [m.f32(n0 + 8) as f64, m.f32(n0 + 0xc) as f64, m.f32(n0 + 0x10) as f64];
                    let v1 = [m.f32(n1 + 8) as f64, m.f32(n1 + 0xc) as f64, m.f32(n1 + 0x10) as f64];
                    let o0 = end * 0x48 + edge + 0x18;
                    for (t, v) in v0.iter().enumerate() {
                        m.set_f64(o0 + t as u32 * 8, *v);
                    }
                    let o1 = end * 0x18 + 0x18 + edge + 0x18;
                    for (t, v) in v1.iter().enumerate() {
                        m.set_f64(o1 + t as u32 * 8, *v);
                    }
                    if r(m, edge + e::PT) != r(m, edge + e::PT + 4) {
                        w(m, edge + e::FACE + end * 4, face);
                        let nd2 = call(m, o, SI_CREATE_EDGE_NODE, &[]);
                        w(m, nd2 + n::EDGE, edge);
                        m.set_u8(nd2 + n::END, end as u8);
                        let first = r(m, con);
                        if first == 0 {
                            w(m, con, nd2);
                            w(m, nd2 + n::PREV, nd2);
                            w(m, nd2 + n::NEXT, nd2);
                        } else {
                            w(m, nd2 + n::NEXT, first);
                            let pv = r(m, first + n::PREV);
                            w(m, nd2 + n::PREV, pv);
                            w(m, pv + n::NEXT, nd2);
                            let nx = r(m, nd2 + n::NEXT);
                            w(m, nx + n::PREV, nd2);
                        }
                    }
                }
            }
            j += 1;
        }
        // the face plane
        let fc = r(m, face + f::CONTOURS);
        let first = r(m, fc);
        let mut count = 0;
        if first != 0 {
            let mut q = first;
            let mut g = 0u32;
            loop {
                count += 1;
                q = r(m, q + n::NEXT);
                g += 1;
                if q == first || g > (1 << 22) {
                    break;
                }
            }
        }
        if count > 2 {
            compute_average_pt(m, fr, face);
            compute_normal(m, face);
            let (nx, ny, nz) = (d(m, face), d(m, face + 8), d(m, face + 0x10));
            let (ax, ay, az) = (d(m, fr), d(m, fr + 8), d(m, fr + 0x10));
            let plane = -((ny * ay + nx * ax) + nz * az);
            m.set_f64(face + f::D, plane.to_f64());
            let mut nh = face;
            if head != 0 {
                w(m, tail + f::NEXT, face);
                w(m, face + f::PREV, tail);
                nh = head;
            }
            head = nh;
            m.set_u8(face + f::STATUS, 1);
            tail = face;
            let fc = r(m, face + f::CONTOURS);
            let first = r(m, fc);
            if first != 0 {
                compute_face_size(m, first, face + f::MIN, face + f::MAX, true);
            }
        }
    }
    m.release(0x48);
    head
}

pub const SI_MESH_ALLOCATE: Routine = Routine::new("SI_meshAllocate", 0x0845_8c20);
pub const SI_VERTEX_ALLOCATE: Routine = Routine::new("SI_vertexAllocate", 0x0845_9d48);
pub const SI_MESH_UPDATE_NODES: Routine = Routine::new("SI_meshUpdateNodes", 0x0845_8cf8);
pub const SI_NODES_DISPOSE: Routine = Routine::new("SI_nodesDispose", 0x0845_3cd0);

/// `si_buildMesh2(src_mesh, &out_mesh, faces)` [soft 0x083110c4 si_buildMesh2]: turns the list of result faces
/// back into a mesh. First every face's corner count (`+0x60`: one per contour that opens a separator, one per
/// edge node of two different points) is worked out and the edges' marks and points' result ids are cleared;
/// `nbResFeatures` counts the points and faces; the mesh, its vertices and its polygons are allocated; every
/// face's polygon gets its corner nodes (point index, the edge's own attribute vector for that end, and, for a
/// face that came from a polygon of the source mesh, UV and colour by barycentric weights over the source
/// polygon's triangulation). Polygons without corners are squeezed out, the nodes updated, and the surviving
/// edges' tags copied into the mesh's edge table. Answers `SI_meshUpdateNodes`' result (a short).
pub fn si_build_mesh2(m: &mut Mem, o: &mut dyn Outside, src: u32, pout: u32, faces: u32) -> i32 {
    build_mesh_body(m, o, src, pout, faces, true).0
}

/// `si_buildMesh(&out_mesh, faces)` [soft 0x08311900 si_buildMesh]: `si_buildMesh2` without the source mesh (no
/// tesselation, no UV or colour weights), and the mesh's vertex colours initialised at the end
/// (`SI_meshInitVertexColors`, 0x08458bc8). Answers `SI_meshUpdateNodes`' result (a short).
pub fn si_build_mesh(m: &mut Mem, o: &mut dyn Outside, pout: u32, faces: u32) -> i32 {
    let (res, ran) = build_mesh_body(m, o, 0, pout, faces, false);
    if ran && r(m, pout) != 0 {
        call(m, o, SI_MESH_INIT_VERTEX_COLORS, &[r(m, pout)]);
    }
    res
}

pub const SI_MESH_INIT_VERTEX_COLORS: Routine = Routine::new("SI_meshInitVertexColors", 0x0845_8bc8);

fn build_mesh_body(m: &mut Mem, o: &mut dyn Outside, src: u32, pout: u32, faces: u32, uv: bool) -> (i32, bool) {
    m.set_u32(pout, 0);
    if faces == 0 {
        return (0, false);
    }
    // pass 1: the corner counts
    let mut f = faces;
    loop {
        w(m, f + f::RES_ID, 0);
        let mut n = 0i32;
        let mut c = r(m, f + f::CONTOURS);
        while c != 0 {
            let first = r(m, c);
            if r(m, c + 4) != 0 {
                n += 1;
            }
            if first != 0 {
                let mut nd = first;
                loop {
                    let ed = r(m, nd + n::EDGE);
                    if r(m, ed + e::PT) != r(m, ed + e::PT + 4) {
                        n += 1;
                    }
                    w(m, ed, 0);
                    let p0 = r(m, ed + e::PT);
                    w(m, p0 + p::RES_ID, 0);
                    let p1 = r(m, ed + e::PT + 4);
                    w(m, p1 + p::RES_ID, 0);
                    nd = r(m, nd + n::NEXT);
                    if nd == r(m, c) {
                        break;
                    }
                }
            }
            c = r(m, c + 8);
        }
        w(m, f + 0x60, n as u32);
        f = r(m, f + f::NEXT);
        if f == 0 {
            break;
        }
    }
    m[NB_RES_PTS] = 0;
    m[NB_RES_FACES] = 0;
    let mut f = faces;
    while f != 0 {
        nb_res_features(m, f);
        f = r(m, f + f::NEXT);
    }
    let mesh = call(m, o, SI_MESH_ALLOCATE, &[]);
    w(m, pout, mesh);
    let nrp = m[NB_RES_PTS] as u32;
    w(m, mesh + 0x14, nrp);
    let nrf = m[NB_RES_FACES] as u32;
    w(m, r(m, pout) + 0x18, nrf);
    let nv = m.u32(r(m, pout) + 0x14);
    let vtx = call(m, o, SI_VERTEX_ALLOCATE, &[nv]);
    w(m, r(m, pout), vtx);
    let np = m.u32(r(m, pout) + 0x18);
    let polys = call(m, o, SI_POLYGON_ALLOCATE, &[np]);
    w(m, r(m, pout) + 4, polys);
    let vtx = r(m, r(m, pout));
    let polys = r(m, r(m, pout) + 4);
    let fr = m.local(0x20); // +0 tesselated count, +4 index, +8/+12/+16 weights, +20 list head
    let mut f = faces;
    while f != 0 {
        let mut tess = 0u32;
        w(m, fr, 0);
        let mut idx = m.i32(f + f::RES_ID);
        if idx < 0 {
            idx = -idx;
        }
        let poly = polys.wrapping_add((idx.wrapping_mul(3).wrapping_sub(3) as u32).wrapping_mul(8));
        m.set_u16(poly + 0x14, m.u16(f + 0x58));
        m.set_u16(poly + 0x12, m.u16(f + 0x5c));
        m.set_u16(poly + 0x10, m.u16(f + 0x60));
        let cnt = r(m, f + 0x60);
        let nodes = m.si_calloc(cnt.wrapping_sub((cnt.wrapping_sub(1) & 3).wrapping_sub(3)), 0x24);
        w(m, poly + 0xc, nodes);
        let mut k = 0u32;
        let mut c = r(m, f + f::CONTOURS);
        while c != 0 {
            let mut k2 = k;
            if r(m, c + 4) != 0 {
                k2 = k + 1;
                w(m, nodes.wrapping_add(k.wrapping_mul(0x24)), 0xffff_ffff);
            }
            if uv && m.i32(f + 0x7c) != -1 {
                let sp = r(m, src + 4).wrapping_add(r(m, f + 0x7c).wrapping_mul(0x18));
                tess = call_tess(m, o, src, sp, fr);
            }
            let first = r(m, c);
            k = k2;
            if first != 0 {
                let mut nd = first;
                loop {
                    let ed = r(m, nd + n::EDGE);
                    if r(m, ed + e::PT) != r(m, ed + e::PT + 4) {
                        for t in 0..2u32 {
                            let pt = r(m, ed + e::PT + t * 4);
                            let rid = m.i32(pt + p::RES_ID);
                            if rid < 0 {
                                let u = !(rid as u32);
                                let off = u.wrapping_mul(0x14);
                                m.set_f32(vtx.wrapping_add(off), m.f64(pt) as f32);
                                m.set_f32(vtx.wrapping_add(off).wrapping_add(4), m.f64(pt + 8) as f32);
                                m.set_f32(vtx.wrapping_add(off).wrapping_add(8), m.f64(pt + 0x10) as f32);
                                w(m, pt + p::RES_ID, u);
                            }
                        }
                        k2 = k + 1;
                        let end = m.u8(nd + n::END) as u32;
                        let off = k.wrapping_mul(0x24);
                        let q = r(m, ed + e::PT + end * 4);
                        w(m, nodes.wrapping_add(off), r(m, q + p::RES_ID));
                        let v0 = m.f64(ed + 0x18 + end * 0x48) as f32;
                        let v1 = m.f64(ed + 0x20 + end * 0x48) as f32;
                        let v2 = m.f64(ed + 0x28 + end * 0x48) as f32;
                        let nn = nodes.wrapping_add(off);
                        m.set_f32(nn + 8, v0);
                        m.set_f32(nn + 0xc, v1);
                        m.set_f32(nn + 0x10, v2);
                        if uv && m.i32(f + 0x7c) != -1 {
                            w(m, fr + 4, 0);
                            let pi = r(m, nn).wrapping_mul(0x14);
                            let pv = [
                                m.f32(vtx.wrapping_add(pi)),
                                m.f32(vtx.wrapping_add(pi).wrapping_add(4)),
                                m.f32(vtx.wrapping_add(pi).wrapping_add(8)),
                            ];
                            let ok = compute_triangle_barycentric_area(m, src, m.i32(fr), tess, pv, fr + 4, fr + 8, fr + 12, fr + 16) & 0xff;
                            if ok == 0 {
                                w(m, nn + 0x14, 0);
                                w(m, nn + 0x18, 0);
                                w(m, nn + 0x1c, 0xffb2_b2b2);
                            } else {
                                let tp = tess.wrapping_add(r(m, fr + 4).wrapping_mul(0x18));
                                let (a, b, c2) = (m.f32(fr + 8), m.f32(fr + 12), m.f32(fr + 16));
                                compute_barycentric_uv_coord(m, o, tp, 0, a, b, c2, nn);
                                compute_barycentric_color(m, tp, a, b, c2, nn + 0x1c);
                                let pn = r(m, tp + 0xc);
                                if r(m, pn + 0x20) != 0 {
                                    if r(m, nn + 0x20) == 0 {
                                        let z = m.si_calloc(0xe, 4);
                                        w(m, nn + 0x20, z);
                                    }
                                    for which in 1..7u32 {
                                        compute_barycentric_uv_coord(m, o, tp, which, a, b, c2, nn);
                                    }
                                }
                            }
                        }
                    } else {
                        k2 = k;
                    }
                    nd = r(m, nd + n::NEXT);
                    k = k2;
                    if nd == r(m, c) {
                        break;
                    }
                }
            }
            k = k2;
            c = r(m, c + 8);
        }
        f = r(m, f + f::NEXT);
        if tess != 0 {
            let cnt = m.i32(fr);
            for i in 0..cnt.max(0) as u32 {
                let n = m.u16(tess + i * 0x18 + 0x10) as u32;
                call(m, o, SI_NODES_DISPOSE, &[tess + 0xc + i * 0x18, 0xffff_ffff, n]);
            }
            m.free(tess);
        }
    }
    // squeeze out the polygons without corners
    let mut empty = 0u32;
    let mut kept = 0u32;
    let mut idx = 0u32;
    let mut mesh = r(m, pout);
    if r(m, mesh + 0x18) != 0 {
        let mut ac = 0u32;
        let mut dst = 0u32;
        loop {
            let polys = r(m, mesh + 4);
            if m.u16(polys + 0x10 + ac) == 0 {
                empty += 1;
            } else {
                if empty != 0 {
                    for t in 0..6u32 {
                        let v = r(m, polys + ac + t * 4);
                        w(m, polys + dst + t * 4, v);
                    }
                }
                dst += 0x18;
                kept += 1;
                mesh = r(m, pout);
            }
            idx += 1;
            ac += 0x18;
            if idx >= r(m, mesh + 0x18) {
                break;
            }
        }
    }
    w(m, mesh + 0x18, kept);
    let res = call(m, o, SI_MESH_UPDATE_NODES, &[r(m, pout), 0]) as u16 as i16;
    w(m, fr + 20, faces);
    let mut idx = 0u32;
    let mesh = r(m, pout);
    if r(m, mesh + 0x18) != 0 {
        loop {
            let pol = r(m, mesh + 4).wrapping_add(idx.wrapping_mul(0x18));
            let next = idx + 1;
            let mut fc = r(m, fr + 20);
            while fc != 0 {
                let mut rid = m.i32(fc + f::RES_ID);
                if rid < 0 {
                    rid = -rid;
                }
                if (rid as u32).wrapping_sub(1) == idx {
                    remove_face_from_list(m, fr + 20, fc);
                    if m.u16(pol + 0x10) != 0 {
                        let mut u = 0u32;
                        loop {
                            let off = u.wrapping_mul(0x24);
                            u += 1;
                            let nodes = r(m, pol + 0xc);
                            if m.i32(nodes.wrapping_add(off)) >= 0 {
                                let mut ev = m.i32(nodes.wrapping_add(off).wrapping_add(4));
                                if ev < 0 {
                                    ev = ev.wrapping_neg();
                                }
                                let lo = (ev as u32) & 0x3ff;
                                let hi = (ev >> 10) as u32;
                                let blk = r(m, r(m, r(m, pout) + 8) + hi * 4);
                                let a = m.i32(blk + lo * 0x14);
                                let b = m.i32(blk + 4 + lo * 0x14);
                                let mut list = r(m, fc + f::CONTOURS);
                                let mut found = false;
                                if list != 0 {
                                    loop {
                                        let c0 = r(m, list);
                                        if c0 != 0 {
                                            let mut cur = c0;
                                            loop {
                                                let ed = r(m, cur + n::EDGE);
                                                let q0 = m.i32(r(m, ed + e::PT) + p::RES_ID);
                                                let q1 = m.i32(r(m, ed + e::PT + 4) + p::RES_ID);
                                                if (q0 == a && q1 == b) || (q1 == a && q0 == b) {
                                                    if m.i32(ed) == -1 {
                                                        found = true;
                                                    } else {
                                                        let fa = r(m, ed + e::FACE);
                                                        let fb = r(m, ed + e::FACE + 4);
                                                        let tagp = r(m, r(m, r(m, pout) + 8) + hi * 4) + 0x12 + lo * 0x14;
                                                        if fa == 0 || fb == 0 || m.u8(fa + f::FLAG) == m.u8(fb + f::FLAG) {
                                                            let t4 = m.u16(ed + 4);
                                                            if t4 == 0 {
                                                                if m.u32(tagp) & 0xfff != 0 {
                                                                    m.set_u16(tagp, 8);
                                                                }
                                                            } else {
                                                                m.set_u16(tagp, t4);
                                                            }
                                                        } else {
                                                            m.set_u16(tagp, 8);
                                                        }
                                                        w(m, ed, 0xffff_ffff);
                                                        found = true;
                                                    }
                                                    break;
                                                }
                                                cur = r(m, cur + n::NEXT);
                                                if cur == c0 {
                                                    break;
                                                }
                                            }
                                        }
                                        list = r(m, list + 8);
                                        if list == 0 || found {
                                            break;
                                        }
                                    }
                                }
                            }
                            if u >= m.u16(pol + 0x10) as u32 {
                                break;
                            }
                        }
                    }
                    break;
                }
                fc = r(m, fc + f::NEXT);
            }
            let mesh = r(m, pout);
            idx = next;
            if next >= r(m, mesh + 0x18) {
                break;
            }
        }
    }
    m.release(0x20);
    (res as i32, true)
}

fn call_tess(m: &mut Mem, o: &mut dyn Outside, src: u32, poly: u32, pcount: u32) -> u32 {
    polygon_tesselate(m, o, src, poly, pcount)
}

pub const MALLOPT: Routine = Routine::new("mallopt", 0x080c_3c58);
pub const SIGNAL: Routine = Routine::new("signal", 0x080c_3bd8);
pub const SETJMP: Routine = Routine::new("_setjmp", 0x080c_4e48);
pub const GETENV: Routine = Routine::new("getenv", 0x080c_3bb8);
pub const SI_SIGNAL_INIT: Routine = Routine::new("SI_signalInit", 0x080d_1038);
/// `fatalError` (0x0830c990), the signal handler it installs for SIGBUS and SIGSEGV.
const FATAL_ERROR: u32 = 0x0830_c990;
/// `SI_actorCollisionMode` (a 16-bit global of the collision unit).
const ACTOR_COLLISION_MODE: u32 = 0x086d_7142;
/// The two environment names that switch cleanups off (strings of the binary's data).
const ENV_NO_FACES_CLEANUP: u32 = 0x086b_9890;
const ENV_NO_EDGES_CLEANUP: u32 = 0x086b_98a9;

/// `SI_computeBoolean(setup, mesh_a, mesh_b, &out_a, &out_b)` [soft 0x08315348 SI_computeBoolean]: the boolean of
/// two meshes. `setup` holds the operation (`+0`: 1 union, 2 intersection, 3 difference), the output mode (`+4`),
/// two cleanup switches (`+0xc`, `+0xe`), two 4x4 float matrices (`+0x14`, `+0x54`) and the count shift and limit
/// (`+0x94`, `+0x98`). It makes the face lists of both meshes (`mesh_b` transformed by the matrices), their groups and
/// boxes; boxes that do not meet (or an object that cannot be opened) skip the tree work: union then merges both
/// lists, difference keeps the first, intersection nothing. Otherwise both lists go through `si_buildBsp`, the
/// second tree is reversed for a difference (operation 3 then runs as 2), its faces are collected and classified
/// against the first tree, the trees cleaned up, and the resulting faces cleaned (`si_cleanUpFaces`, and
/// `si_cleanUpEdges` unless the collision mode is on, each unless its environment variable is set) and built into
/// one or two meshes (mode 0: all faces, flagged ones shifted by the count shift and dropped above the limit;
/// otherwise `divideList`). A fault jumps back (`_setjmp`) to free the pools and answer -1.
pub fn si_compute_boolean(m: &mut Mem, o: &mut dyn Outside, setup: u32, mesh_a: u32, mesh_b: u32, pout_a: u32, pout_b: u32) -> u32 {
    let mut keep_going = true;
    call(m, o, MALLOPT, &[1, 10000]);
    let oper = r(m, setup) as i32;
    let mode = r(m, setup + 4);
    let sv3 = m.u8(setup + 0xc);
    let sv4 = m.u8(setup + 0xe);
    let fr = m.local(0xa0);
    // fr+0: matrix from setup+0x54 (pb), fr+0x40: matrix from setup+0x14 (pa)
    let mat_b = fr;
    let mat_a = fr + 0x40;
    for row in 0..4u32 {
        for col in 0..4u32 {
            let v = r(m, setup + 0x14 + row * 16 + col * 4);
            w(m, mat_a + row * 16 + col * 4, v);
            let v = r(m, setup + 0x54 + row * 16 + col * 4);
            w(m, mat_b + row * 16 + col * 4, v);
        }
    }
    let local14 = fr + 0x80; // the result list
    let plast = fr + 0x84;
    let keep = fr + 0x88;
    let moved = fr + 0x8c;
    call(m, o, SIGNAL, &[7, FATAL_ERROR]);
    call(m, o, SIGNAL, &[0xb, FATAL_ERROR]);
    if call(m, o, SETJMP, &[JMP_ENV.addr()]) != 0 {
        si_release_mem(m);
        w(m, pout_a, 0);
        w(m, pout_b, 0);
        call(m, o, SI_SIGNAL_INIT, &[0xffff_ffff]);
        call(m, o, MALLOPT, &[1, 1000]);
        m.release(0xa0);
        return 0xffff_ffff;
    }
    set_merge_dist(m, 2.5e-13);
    init_boolean(m);
    set_classifying(m, 0);
    m[OPER] = oper;
    set_cleanup(m, sv3, sv4);
    let list_b = si_generate_face_list(m, o, mesh_b, 1, mat_a, mat_b);
    let list_a = si_generate_face_list(m, o, mesh_a, 0, mat_a, mat_b);
    let g13 = call(m, o, SI_CREATE_FACE_GROUP, &[]);
    w(m, g13 + 0x30, list_b);
    compute_face_group_size(m, g13);
    let g14 = call(m, o, SI_CREATE_FACE_GROUP, &[]);
    w(m, g14 + 0x30, list_a);
    compute_face_group_size(m, g14);
    w(m, local14, 0);
    if open_object(m, r(m, g14 + 0x30)) & 0xff == 0 && open_object(m, r(m, g13 + 0x30)) & 0xff == 0 {
        let d = |m: &Mem, a: u32| m.f64(a);
        let apart = d(m, g13 + 0x18) < d(m, g14)
            || d(m, g13 + 0x20) < d(m, g14 + 8)
            || d(m, g13 + 0x28) < d(m, g14 + 0x10)
            || d(m, g14 + 0x18) < d(m, g13)
            || d(m, g14 + 0x20) < d(m, g13 + 8)
            || d(m, g14 + 0x28) < d(m, g13 + 0x10);
        if apart {
            keep_going = false;
            if oper == 1 {
                let l = merge_two_lists_of_faces(m, r(m, g14 + 0x30), 0, r(m, g13 + 0x30), 0, plast);
                w(m, local14, l);
            } else if oper == 3 {
                let l = r(m, g14 + 0x30);
                w(m, local14, l);
            }
        }
    }
    if keep_going {
        m[CHANGE_STATUS] = 0;
        w(m, g14 + 0x30, list_a);
        let tree_a = si_build_bsp(m, o, 0, g14, 1);
        w(m, g13 + 0x30, list_b);
        let tree_b = si_build_bsp(m, o, 0, g13, 1);
        if oper == 3 {
            reset_edge_id(m);
            reverse_tree(m, tree_b);
            reset_edge_id(m);
            m[OPER] = 2;
        }
        si_get_resulting_faces(m, tree_b, g13 + 0x30, plast);
        compute_face_group_size(m, g13);
        m[CHANGE_STATUS] = 1;
        set_classifying(m, 1);
        let t = classify_faces(m, o, tree_a, g13);
        m[CHANGE_STATUS] = 0;
        let t = clean_up_tree(m, t, tree_b);
        if list_a == 0 && list_b == 0 {
            w(m, local14, 0);
        } else {
            si_get_resulting_faces(m, t, local14, plast);
        }
    }
    let result = if r(m, local14) == 0 {
        w(m, pout_a, 0);
        false
    } else {
        w(m, keep, 0);
        w(m, moved, 0);
        if call(m, o, GETENV, &[ENV_NO_FACES_CLEANUP]) == 0 {
            crate::clean::si_clean_up_faces(m, o, local14);
        }
        if m.u16(ACTOR_COLLISION_MODE) == 0 && call(m, o, GETENV, &[ENV_NO_EDGES_CLEANUP]) == 0 {
            crate::clean::si_clean_up_edges(m, local14);
        }
        let head = r(m, local14);
        let first;
        if mode == 0 {
            let add = m.i32(setup + 0x94);
            let limit = m.i32(setup + 0x98);
            let mut f = head;
            while f != 0 {
                if m.u8(f + f::FLAG) != 0 {
                    let c = m.i32(f + f::COUNT).wrapping_add(add);
                    m.set_i32(f + f::COUNT, c);
                }
                if limit < m.i32(f + f::COUNT) {
                    w(m, f + f::COUNT, 0);
                }
                f = r(m, f + f::NEXT);
            }
            first = head;
        } else {
            divide_list(m, o, local14, m.i32(setup + 0x94), m.i32(setup + 0x98), keep, moved);
            first = if mode.wrapping_sub(1) <= 1 { r(m, keep) } else { r(m, moved) };
        }
        si_build_mesh(m, o, pout_a, first);
        mode != 0 && mode != 2 && mode != 3
    };
    if result {
        let mv = r(m, moved);
        si_build_mesh(m, o, pout_b, mv);
    } else {
        w(m, pout_b, 0);
    }
    si_release_mem(m);
    call(m, o, SI_SIGNAL_INIT, &[0xffff_ffff]);
    call(m, o, MALLOPT, &[1, 1000]);
    m.release(0xa0);
    0
}
