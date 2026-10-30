// SPDX-License-Identifier: MIT
//! The image forms against the value forms, on made-up meshes.

use super::*;
use crate::mesh::Mesh as V;
use crate::mesh_b::{self, MeshEnv};
use crate::model_image::NoOutside;

/// A small deterministic generator.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    fn coord(&mut self) -> f32 {
        (self.below(2001) as f32 - 1000.0) / 250.0
    }
}

/// A polygon soup: shared and doubled edges, collisions, holes, every flag state.
fn soup(r: &mut Lcg) -> V {
    let nv = 4 + r.below(9) as usize;
    let pos: Vec<[f32; 3]> = (0..nv).map(|_| [r.coord(), r.coord(), r.coord()]).collect();
    let mut m = V::with_vertices(&pos);
    m.flags = [0x804u16, 0x2, 0x4, 0x8, 0x0, 0x80c][r.below(6) as usize];
    m.cos_angle = [0.5f32, 0.0, 0.9][r.below(3) as usize];
    for _ in 0..4 + r.below(11) {
        let n = 3 + r.below(7) as usize;
        let mut corners: Vec<i32> = Vec::new();
        for k in 0..n {
            if k > 2 && k + 2 < n && r.below(8) == 0 {
                corners.push(-1);
            }
            corners.push(r.below(nv as u32) as i32);
        }
        m.push_polygon(&corners);
    }
    m
}

/// A fresh program's image with an empty graph list (what the collision messages search
/// for the model's name).
fn image() -> Mem {
    let mut m = Mem::program();
    crate::model_image::init(&mut m);
    let lists = m.place(0x24, 4);
    m.set_fill_bytes(lists, 0, 0x24);
    m[crate::model_image::g::SI_GRAF_LISTS] = lists;
    m
}

fn same(a: &V, b: &V) {
    assert_eq!(a.vertices.len(), b.vertices.len());
    for (x, y) in a.vertices.iter().zip(&b.vertices) {
        assert_eq!(x.star, y.star);
        assert_eq!(x.pos.map(f32::to_bits), y.pos.map(f32::to_bits));
    }
    assert_eq!(a.polygons.len(), b.polygons.len());
    for (x, y) in a.polygons.iter().zip(&b.polygons) {
        assert_eq!(x.normal.map(f32::to_bits), y.normal.map(f32::to_bits));
        assert_eq!(x.flag, y.flag);
        assert_eq!(x.nodes.len(), y.nodes.len());
        for (p, q) in x.nodes.iter().zip(&y.nodes) {
            assert_eq!((p.vertex, p.edge), (q.vertex, q.edge));
            assert_eq!(p.normal.map(f32::to_bits), q.normal.map(f32::to_bits));
        }
    }
    let ea: Vec<_> = a.edges.iter().skip(1).map(|e| (e.v0, e.v1, e.p0, e.p1, e.flag)).collect();
    let eb: Vec<_> = b.edges.iter().skip(1).map(|e| (e.v0, e.v1, e.p0, e.p1, e.flag)).collect();
    assert_eq!(ea, eb);
    assert_eq!(a.flags, b.flags);
    assert_eq!(a.nb_display, b.nb_display);
    assert_eq!(a.display, b.display);
}

#[test]
fn the_rebuild_on_the_image_is_the_value_forms() {
    si_rt::skip_without_install!();
    let mut r = Lcg(7);
    let mut collisions = 0;
    for _ in 0..600 {
        let v = soup(&mut r);
        let mut value = v.clone();
        let mut env = MeshEnv::default();
        let ok = mesh_b::update_nodes(&mut value, true, &mut env);
        collisions += env.messages.len();
        let mut m = image();
        let a = from_value(&mut m, &v);
        let ok2 = rebuild::update_nodes(&mut m, &mut NoOutside, a, 1);
        assert_eq!(ok, ok2 != 0);
        assert!(m.fault().is_none(), "{:?}", m.fault());
        let image = to_value(&m, a);
        same(&image, &value);
    }
    assert!(collisions > 0);
}

#[test]
fn a_copy_disposed_of_frees_every_block_it_made() {
    si_rt::skip_without_install!();
    let mut r = Lcg(11);
    for _ in 0..50 {
        let v = soup(&mut r);
        let mut m = image();
        let a = from_value(&mut m, &v);
        rebuild::update_nodes(&mut m, &mut NoOutside, a, 1);
        let before = m.heap_stats();
        let c = obj::mesh_copy(&mut m, &mut NoOutside, a);
        // the copy has the display lines but for their closing −1 (the count is one less
        // than the integers, and the copy copies the count)
        let (mut x, mut y) = (to_value(&m, c), to_value(&m, a));
        assert_eq!(x.display.pop(), Some(0));
        y.display.pop();
        same(&x, &y);
        let holder = m.place(4, 4);
        m.set_u32(holder, c);
        obj::mesh_dispose(&mut m, holder);
        assert_eq!(m.u32(holder), 0);
        let after = m.heap_stats();
        // `SI_calloc` of no entries loses the block it asked for: a star of no entries
        let empty = y.vertices.iter().filter(|v| v.star.is_empty()).count() as u32;
        assert_eq!(after.live_blocks - before.live_blocks, empty, "{before:?} {after:?}");
    }
}

#[test]
fn a_quadrilateral_and_a_cube_become_triangles() {
    si_rt::skip_without_install!();
    let mut m = image();
    let mut v = V::with_vertices(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
    v.push_polygon(&[0, 1, 2, 3]);
    let a = from_value(&mut m, &v);
    rebuild::update_nodes(&mut m, &mut NoOutside, a, 1);
    assert_eq!(tri::mesh_triangulate(&mut m, &mut NoOutside, a, 1), 1);
    let t = to_value(&m, a);
    assert_eq!(t.polygons.len(), 2);
    let corners: Vec<Vec<i32>> = t.polygons.iter().map(|p| p.nodes.iter().map(|n| n.vertex).collect()).collect();
    assert_eq!(corners, [vec![0, 1, 2], vec![0, 2, 3]]);
    assert_eq!(t.edges.len() - 1, 5);
    // the old edges keep their flags (borders); the diagonal has 0x4
    assert!(t.edges[1..].iter().all(|e| e.flag & 0x4 != 0 || e.flag & 0x20 != 0));
    // a cube of six quadrilaterals: twelve triangles, no collision
    let mut c = V::with_vertices(&[
        [-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0],
    ]);
    for q in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
        c.push_polygon(&q);
    }
    let a = from_value(&mut m, &c);
    rebuild::update_nodes(&mut m, &mut NoOutside, a, 1);
    assert_eq!(tri::mesh_triangulate(&mut m, &mut NoOutside, a, 1), 1);
    let t = to_value(&m, a);
    assert_eq!(t.polygons.len(), 12);
    assert_eq!(t.edges.len() - 1, 18);
    assert!(t.edges[1..].iter().all(|e| e.p0 != -1 && e.p1 != -1));
}

#[test]
fn the_triangulator_on_the_image_is_the_value_forms() {
    let mut r = Lcg(3);
    for _ in 0..300 {
        let n = 3 + r.below(12) as usize;
        let mut pts: Vec<[f32; 3]> = (0..n).map(|_| [r.coord(), r.coord(), 0.0]).collect();
        let mut idx: Vec<i32> = (0..n as i32).collect();
        if n > 7 && r.below(2) == 0 {
            idx.insert(n / 2, -1);
        }
        let mut m = Mem::program();
        let p = m.place(12 * n as u32, 4);
        for (k, q) in pts.iter().enumerate() {
            put_f3(&mut m, p + 12 * k as u32, *q);
        }
        let c = m.place(4 * idx.len() as u32, 4);
        for (k, &i) in idx.iter().enumerate() {
            m.set_i32(c + 4 * k as u32, i);
        }
        let out = m.place(8, 4);
        let ok = tri::triangulate_contour(&mut m, n as i32, p, idx.len() as i32, c, out, out + 4);
        let (ok2, t2) = crate::triangulate::triangulate_contour(&mut pts, &mut idx);
        assert_eq!(ok != 0, ok2);
        let count = m.i32(out);
        let tris = m.u32(out + 4);
        let t: Vec<[i32; 3]> = (0..count as u32).map(|k| [m.i32(tris + 12 * k), m.i32(tris + 12 * k + 4), m.i32(tris + 12 * k + 8)]).collect();
        if ok2 {
            assert_eq!(t, t2);
        }
        for (k, q) in pts.iter().enumerate() {
            assert_eq!(f3(&m, p + 12 * k as u32).map(f32::to_bits), q.map(f32::to_bits));
        }
    }
}

/// The lists of an image mesh, head first: (item index, matrix, the node's own list).
fn image_lists(m: &Mem, a: u32) -> (Vec<(i32, [[u32; 4]; 4])>, Vec<(i32, Vec<i32>)>) {
    let (verts, polys) = (m.u32(a), m.u32(a + 4));
    let mut p = Vec::new();
    let mut node = m.u32(a + 0x30);
    while node != 0 {
        let mut mat = [[0u32; 4]; 4];
        for (i, row) in mat.iter_mut().enumerate() {
            for (j, w) in row.iter_mut().enumerate() {
                *w = m.u32(node + 0xc + 16 * i as u32 + 4 * j as u32);
            }
        }
        p.push(((m.u32(node) - polys) as i32 / 0x18, mat));
        node = m.u32(node + 0x4c);
    }
    let mut v = Vec::new();
    let mut node = m.u32(a + 0x34);
    while node != 0 {
        let mut sub = Vec::new();
        let mut s = m.u32(node + 4);
        while s != 0 {
            sub.push((m.u32(s) - polys) as i32 / 0x18);
            s = m.u32(s + 0x4c);
        }
        v.push(((m.u32(node) - verts) as i32 / 0x14, sub));
        node = m.u32(node + 0x4c);
    }
    (p, v)
}

#[test]
fn the_lists_on_the_image_are_the_value_forms() {
    si_rt::skip_without_install!();
    use crate::mesh::ListItem;
    let mut r = Lcg(23);
    let mut nonempty = 0;
    for _ in 0..400 {
        let mut v = soup(&mut r);
        for p in v.polygons.iter_mut() {
            if r.below(3) == 0 {
                p.flag |= 1;
            }
        }
        let mut value = v.clone();
        let mut env = MeshEnv::default();
        mesh_b::update_nodes(&mut value, true, &mut env);
        let mut m = image();
        let a = from_value(&mut m, &v);
        rebuild::update_nodes(&mut m, &mut NoOutside, a, 1);
        assert!(m.fault().is_none(), "{:?}", m.fault());
        let image = to_value(&m, a);
        same(&image, &value);
        let (lp, lv) = image_lists(&m, a);
        let vp: Vec<(i32, [[u32; 4]; 4])> = value
            .selected_polygons
            .iter()
            .map(|n| {
                let ListItem::Polygon(p) = n.item else { panic!() };
                (p, n.matrix.map(|row| row.map(f32::to_bits)))
            })
            .collect();
        let vv: Vec<(i32, Vec<i32>)> = value
            .selected_vertices
            .iter()
            .map(|n| {
                let ListItem::Vertex(x) = n.item else { panic!() };
                (x, n.sub.iter().map(|s| if let ListItem::Polygon(p) = s.item { p } else { -1 }).collect())
            })
            .collect();
        assert_eq!(lp, vp);
        assert_eq!(lv, vv);
        nonempty += !lp.is_empty() as u32;
        // a copy keeps the matrices of the polygon list
        let c = obj::mesh_copy(&mut m, &mut NoOutside, a);
        assert_eq!(image_lists(&m, c).0, lp);
    }
    assert!(nonempty > 100);
}

/// A made-up node with its material count, materials array, first texture and father.
fn node(m: &mut Mem, nb: i16, mats: u32, txt: u32, father: u32) -> u32 {
    use si_hrc::image::node::node as n;
    let a = m.calloc(1, 0x430);
    m.set_i16(a + n::NB_MATERIALS, nb);
    m.set_u32(a + n::MATERIALS, mats);
    m.set_u32(a + n::TEXTURE_CURRENT, txt);
    m.set_u32(a + n::FATHER, father);
    a
}

/// A chain of made-up textures of the given mapping methods; answers the first.
fn textures(m: &mut Mem, methods: &[u16]) -> (u32, Vec<u32>) {
    use si_hrc::image::texture::lclmap;
    let all: Vec<u32> = methods.iter().map(|_| m.calloc(1, 0x1d8)).collect();
    for (k, (&t, &method)) in all.iter().zip(methods).enumerate() {
        m.set_u16(t + lclmap::METHOD, method);
        m.set_u32(t + lclmap::NEXT, all.get(k + 1).copied().unwrap_or(0));
    }
    (all.first().copied().unwrap_or(0), all)
}

/// The data of a genList from its head, `count` times.
fn list_data(m: &Mem, list: u32) -> Vec<u32> {
    use si_gen::list::{COUNT, DATA, HEAD, NEXT};
    let mut e = m.u32(list + HEAD);
    (0..m.i32(list + COUNT)).map(|_| {
        let d = m.u32(e + DATA);
        e = m.u32(e + NEXT);
        d
    }).collect()
}

#[test]
fn a_node_without_materials_draws_with_the_default_one() {
    let mut m = Mem::new();
    si_mem::init_globals(&mut m, g::ALL);
    let n = node(&mut m, 0, 0, 0, 0);
    let out = m.calloc(1, 12);
    mattxt::find_mat_info(&mut m, &mut NoOutside, n, out, out + 4, out + 8);
    assert_eq!((m.u32(out), m.i32(out + 4), m.u32(out + 8)), (g::PTRMAT.addr(), 1, 0));
    assert_eq!(m.u32(m.u32(out)), 0x0869_dde0);
}

#[test]
fn the_walk_takes_the_first_texture_and_the_first_materials_up_the_fathers() {
    let mut m = Mem::new();
    let (t_top, _) = textures(&mut m, &[4]);
    let (t_mid, _) = textures(&mut m, &[4]);
    let mats = m.calloc(2, 4);
    let top = node(&mut m, 0, mats, t_top, 0);
    let mid = node(&mut m, 0, 0, t_mid, top);
    let low = node(&mut m, 0, 0, 0, mid);
    let out = m.calloc(1, 12);
    mattxt::find_mat_info(&mut m, &mut NoOutside, low, out, out + 4, out + 8);
    // the middle node's texture (met first), the top node's materials, a count of 0 read as 1
    assert_eq!((m.u32(out), m.i32(out + 4), m.u32(out + 8)), (mats, 1, t_mid));
}

#[test]
fn the_textures_of_one_method_are_gathered_by_owner() {
    use si_hrc::image::material::mat;
    let mut m = Mem::new();
    si_mem::init_globals(&mut m, g::ALL);
    let (own, own_all) = textures(&mut m, &[4, 1, 4]);
    let (m0_first, m0_all) = textures(&mut m, &[1, 4]);
    let (m1_first, _) = textures(&mut m, &[7]);
    let (mat0, mat1) = (m.calloc(1, 0x6f4), m.calloc(1, 0x6f4));
    m.set_u32(mat0 + mat::TEXTURES, m0_first);
    m.set_u32(mat1 + mat::TEXTURES, m1_first);
    let mats = m.calloc(2, 4);
    m.set_u32(mats, mat0);
    m.set_u32(mats + 4, mat1);
    let n = node(&mut m, 2, mats, own, 0);
    let list = mattxt::mdl_get_mat_and_txt2d(&mut m, &mut NoOutside, n, 4);
    let recs = list_data(&m, list);
    assert_eq!(recs.len(), 2);
    assert_eq!(m.u32(recs[0] + mattxt::rec::MATERIAL), 0);
    assert_eq!(list_data(&m, m.u32(recs[0] + mattxt::rec::TEXTURES)), [own_all[0], own_all[2]]);
    assert_eq!(m.u32(recs[1] + mattxt::rec::MATERIAL), mat0);
    assert_eq!(list_data(&m, m.u32(recs[1] + mattxt::rec::TEXTURES)), [m0_all[1]]);
    // nothing of method 3: no list
    assert_eq!(mattxt::mdl_get_mat_and_txt2d(&mut m, &mut NoOutside, n, 3), 0);
    // given up: every record and list freed, the textures kept
    let p = m.calloc(1, 4);
    m.set_u32(p, list);
    si_gen::list::destroy(&mut m, &mut mattxt::Callbacks, p, mattxt::MAT_AND_TXT2D_DESTROY);
    assert_eq!(m.u32(p), 0);
    assert!(recs.iter().all(|r| m.freed().contains(r)));
    assert!(!m.freed().contains(&own_all[0]));
}
