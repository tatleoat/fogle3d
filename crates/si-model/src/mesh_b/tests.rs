// SPDX-License-Identifier: MIT
use super::*;
use crate::mesh::Mesh;

fn star(m: &Mesh, v: usize) -> Vec<(i32, i32, i32)> {
    m.vertices[v].star.iter().map(|s| (s.vertex, s.edge, s.polygon)).collect()
}

fn edges(m: &Mesh) -> Vec<(i32, i32, i32, i32)> {
    m.edges[1..].iter().map(|e| (e.v0, e.v1, e.p0, e.p1)).collect()
}

fn corner_edges(m: &Mesh, p: usize) -> Vec<i32> {
    m.polygons[p].nodes.iter().map(|n| n.edge).collect()
}

#[test]
fn one_triangle_is_filed_last_corner_first() {
    let mut m = Mesh::with_vertices(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    m.push_polygon(&[0, 1, 2]);
    let mut env = MeshEnv::default();
    assert!(insert_ipolygon(&mut m, 0, &mut env));
    // calls: (prev 1, v 2, next 0), (2, 0, 1), (0, 1, 2); an edge is created by the first
    // corner that needs it, prev → v before v → next
    assert_eq!(edges(&m), [(1, 2, 0, -1), (2, 0, 0, -1), (0, 1, 0, -1)]);
    assert_eq!(corner_edges(&m, 0), [3, 1, 2]);
    assert_eq!(star(&m, 2), [(0, 2, 0), (1, -1, -1)]);
    assert_eq!(star(&m, 0), [(1, 3, 0), (2, -2, -1)]);
    assert_eq!(star(&m, 1), [(2, 1, 0), (0, -3, -1)]);
    assert_eq!(m.edges[0], Edge::default()); // record 0 of the zeroed block
    assert!(env.messages.is_empty());
}

#[test]
fn a_second_polygon_takes_the_other_side_of_a_shared_edge() {
    let mut m = Mesh::with_vertices(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]);
    m.push_polygon(&[0, 1, 2]);
    m.push_polygon(&[2, 1, 3]);
    let mut env = MeshEnv::default();
    assert!(update_nodes(&mut m, true, &mut env));
    // edge 1 is 1 → 2 with polygon 0 along it; polygon 1 runs 2 → 1
    assert_eq!(edges(&m)[0], (1, 2, 0, 1));
    assert_eq!(m.nb_edges(), 5);
    // at vertex 1 polygon 1 has next 3 and prev 2: 3's entry goes before 2's
    assert_eq!(star(&m, 1).iter().map(|s| (s.0, s.2)).collect::<Vec<_>>(), [(3, 1), (2, 0), (0, -1)]);
    // at vertex 2 polygon 1 has next 1 (known) and prev 3: inserted after 1's entry
    assert_eq!(star(&m, 2).iter().map(|s| (s.0, s.2)).collect::<Vec<_>>(), [(0, 0), (1, 1), (3, -1)]);
    // coplanar: the inner edge is no discontinuity, the four others are borders
    let flags: Vec<i16> = m.edges[1..].iter().map(|e| e.flag & 0x38).collect();
    assert_eq!(flags, [0, 0x20, 0x20, 0x20, 0x20]);
    for p in &m.polygons {
        assert_eq!(p.normal, [0.0, 0.0, 1.0]);
        for n in &p.nodes {
            assert_eq!(n.normal, [0.0, 0.0, 1.0]);
        }
    }
    // the first pass turns 0x800 into 0x8 and keeps it
    assert_eq!(m.flags, 0x80c);
}

#[test]
fn two_polygons_along_the_same_directed_edge_collide() {
    let mut m = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    m.push_polygon(&[0, 1, 2]);
    m.push_polygon(&[0, 1, 3]);
    let mut env = MeshEnv::default();
    assert!(!update_nodes(&mut m, true, &mut env));
    assert_eq!(env.messages.len(), 1);
    assert!(env.messages[0].starts_with("collision of polygons (v"), "{}", env.messages[0]);
    env.messages.clear();
    env.set_current_model(Some(b"cube"));
    let mut m2 = Mesh::with_vertices(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    m2.push_polygon(&[0, 1, 2]);
    m2.push_polygon(&[0, 1, 3]);
    update_nodes(&mut m2, true, &mut env);
    assert!(env.messages[0].starts_with("\"cube\" : collision of polygons (v"), "{}", env.messages[0]);
}

fn cube() -> Mesh {
    let mut m = Mesh::with_vertices(&[
        [-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0],
    ]);
    for q in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
        m.push_polygon(&q);
    }
    m
}

#[test]
fn cube_is_closed_and_all_its_edges_are_discontinuities() {
    let mut m = cube();
    let mut env = MeshEnv::default();
    assert!(update_nodes(&mut m, true, &mut env));
    assert_eq!(m.nb_edges(), 12);
    assert!(m.edges[1..].iter().all(|e| e.p0 != -1 && e.p1 != -1 && e.flag & 0x38 == 0x10));
    let normals = [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    for (p, n) in m.polygons.iter().zip(normals) {
        assert_eq!(p.normal, n);
        assert!(p.nodes.iter().all(|c| c.normal == n));
    }
    for v in &m.vertices {
        assert_eq!(v.star.len(), 3);
        assert!(v.star.iter().all(|s| s.polygon != -1 && s.edge != 0));
    }
    // every edge is drawn once: 12 edges in lines of (vertices, −1)
    let lines = m.display.split(|&i| i == -1).filter(|l| !l.is_empty()).map(|l| l.len() - 1).sum::<usize>();
    assert_eq!(lines, 12);
    assert_eq!(m.nb_display, m.display.len() as i32 - 1);
    assert!(m.edges[1..].iter().all(|e| e.flag < 0));
}

#[test]
fn a_wide_angle_smooths_the_cube_and_faceted_overrides_it() {
    let mut m = cube();
    m.angle = 120.0;
    m.cos_angle = -0.5;
    let mut env = MeshEnv::default();
    update_nodes(&mut m, true, &mut env);
    assert!(m.edges[1..].iter().all(|e| e.flag & 0x38 == 0));
    // three unit normals summed and divided by the float length √3
    let len = 3.0f32.sqrt();
    let c = 1.0f32 / len;
    for p in &m.polygons {
        for n in &p.nodes {
            let v = m.vertices[n.vertex as usize].pos;
            assert_eq!(n.normal, [v[0] * c, v[1] * c, v[2] * c]);
        }
    }
    m.flags |= 2;
    computes_vertex_normal(&mut m, -1);
    assert!(m.polygons.iter().all(|p| p.nodes.iter().all(|c| c.normal == p.normal)));
}

#[test]
fn user_edges_and_kept_polygons() {
    // a flat fan of four quads around vertex 4
    let mut pos = Vec::new();
    for y in 0..3 {
        for x in 0..3 {
            pos.push([x as f32, y as f32, 0.0]);
        }
    }
    let mut m = Mesh::with_vertices(&pos);
    for q in [[0, 1, 4, 3], [1, 2, 5, 4], [3, 4, 7, 6], [4, 5, 8, 7]] {
        m.push_polygon(&q);
    }
    let mut env = MeshEnv::default();
    update_nodes(&mut m, true, &mut env);
    assert_eq!(m.vertices[4].star.len(), 4);
    assert!(m.vertices[4].star.iter().all(|s| s.polygon != -1));
    // tilt the stored polygon normals by hand and mark one spoke as a user edge: the fan
    // is then no longer summed as a whole
    let e = m.vertices[4].star[0].edge.unsigned_abs() as usize;
    m.edges[e].flag |= 0x08;
    m.polygons[0].normal = [1.0, 0.0, 0.0];
    m.polygons[3].flag |= 0x400;
    let kept = m.polygons[3].nodes[0].normal;
    computes_vertex_normal(&mut m, 4);
    // polygon 3 keeps its corner; the others get a sum that stops at the user edge
    assert_eq!(m.polygons[3].nodes[0].normal, kept);
    let at4 = |m: &Mesh, p: usize| m.polygons[p].nodes.iter().find(|n| n.vertex == 4).unwrap().normal;
    let all: Vec<[f32; 3]> = (0..3).map(|p| at4(&m, p)).collect();
    // one hard spoke in a closed fan: every polygon sums the whole fan, starting after it
    assert!(all.iter().all(|n| *n == all[0]));
    assert!(all[0][0] > 0.0 && all[0][2] > 0.0);
    // compute_hard_edge leaves the user edge's bits alone
    compute_hard_edge(&mut m, -1);
    assert_eq!(m.edges[e].flag & 0x38, 0x08);
}

#[test]
fn polygon_normal_sums_holes_and_leaves_degenerates_at_zero() {
    // a square with a square hole wound the other way: the hole reduces the sum, the
    // direction stays
    let mut m = Mesh::with_vertices(&[
        [0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 4.0, 0.0], [0.0, 4.0, 0.0],
        [1.0, 1.0, 0.0], [1.0, 3.0, 0.0], [3.0, 3.0, 0.0], [3.0, 1.0, 0.0],
    ]);
    m.push_polygon(&[0, 1, 2, 3, -1, 4, 5, 6, 7]);
    m.push_polygon(&[0, 1, 1]);
    m.push_polygon(&[]);
    m.push_polygon(&[-1, 0, 1, 2]);
    computes_polygon_normal(&mut m, -1);
    assert_eq!(m.polygons[0].normal, [0.0, 0.0, 1.0]);
    assert_eq!(m.polygons[1].normal, [0.0, 0.0, 0.0]);
    assert_eq!(m.polygons[2].normal, [0.0, 0.0, 0.0]);
    assert_eq!(m.polygons[3].normal, [0.0, 0.0, 1.0]);
    // one polygon only
    m.vertices[2].pos = [4.0, 0.0, 4.0];
    m.vertices[3].pos = [0.0, 0.0, 4.0];
    computes_polygon_normal(&mut m, 3);
    assert_eq!(m.polygons[3].normal, [0.0, -1.0, 0.0]);
    assert_eq!(m.polygons[0].normal, [0.0, 0.0, 1.0]);
}

#[test]
fn polygon_normal_keeps_its_sums_in_doubles() {
    // coordinates whose products do not fit a float: the double sums keep what float sums
    // would lose, and the result is the correctly rounded quotient
    let a = [1.0e-3f32, 2.0e-3, 3.0e-3];
    let b = [1.000001f32, 0.25, -0.75];
    let c = [-0.3333333f32, 0.7777777, 0.1234567];
    let mut m = Mesh::with_vertices(&[a, b, c]);
    m.push_polygon(&[0, 1, 2]);
    computes_polygon_normal(&mut m, -1);
    let d = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
    let (a, b, c) = (d(a), d(b), d(c));
    let mut s = [0.0f64; 3];
    for (p, q) in [(c, a), (a, b), (b, c)] {
        s[0] += (p[1] - q[1]) * (p[2] + q[2]);
        s[1] += (p[0] + q[0]) * (p[2] - q[2]);
        s[2] += (p[1] + q[1]) * (p[0] - q[0]);
    }
    let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
    let n = m.polygons[0].normal;
    for i in 0..3 {
        assert!((n[i] as f64 - s[i] / len).abs() < 1e-7, "{i}: {} {}", n[i], s[i] / len);
    }
    let l = ((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) as f64).sqrt();
    assert!((l - 1.0).abs() < 1e-6);
}

#[test]
fn a_corner_shared_by_two_fans_keeps_them_apart() {
    // two triangles that touch only at vertex 0
    let mut m = Mesh::with_vertices(&[
        [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0],
    ]);
    m.push_polygon(&[0, 1, 2]);
    m.push_polygon(&[0, 3, 4]);
    let mut env = MeshEnv::default();
    assert!(update_nodes(&mut m, true, &mut env));
    assert_eq!(star(&m, 0).iter().map(|s| (s.0, s.2)).collect::<Vec<_>>(), [(1, 0), (2, -1), (3, 1), (4, -1)]);
    assert_eq!(m.polygons[0].nodes[0].normal, m.polygons[0].normal);
    assert_eq!(m.polygons[1].nodes[0].normal, m.polygons[1].normal);
}

#[test]
fn polygon_initialize_and_insert_edge_follow_the_table() {
    let mut p = Polygon::default();
    polygon_initialize(&mut p, &[5, 6, 7]);
    assert_eq!(p.nodes.len(), 3);
    assert_eq!((p.nodes[1].vertex, p.nodes[1].edge, p.nodes[1].color), (6, 0, 0xffb2_b2b2));
    let mut m = Mesh::with_vertices(&[[0.0; 3]; 2]);
    let mut env = MeshEnv { index3: Some(vec![2, -1]), ..MeshEnv::default() };
    assert_eq!(insert_edge(&mut m, 0, 1, 7, -1, &mut env), 0);
    assert_eq!(insert_edge(&mut m, 1, 0, 8, -1, &mut env), 1);
    assert_eq!(insert_edge(&mut m, 1, 1, 9, -1, &mut env), 2);
    assert_eq!(env.index3.as_ref().unwrap()[0], 0); // counted down to zero and left there
    assert_eq!(m.nb_edges(), 3);
}
