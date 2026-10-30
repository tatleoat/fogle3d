// SPDX-License-Identifier: MIT
use super::*;
use si_hrc::SplineKey;

fn ring(points: &[[f32; 2]]) -> Spline {
    let keys: Vec<SplineKey> = points.iter().map(|p| SplineKey { pos: [p[0], p[1], 0.0], ..SplineKey::default() }).collect();
    Spline { nkeys: keys.len() as u16, keys, ..Spline::default() }
}

/// A square outline wound counter-clockwise with a square hole wound the same way.
fn face() -> Face {
    let mut f = Face::default();
    f.exterior = ring(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
    f.holes = vec![ring(&[[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]])];
    f.count = 1;
    f
}

#[derive(Default)]
struct Turned(Vec<i32>);

impl Hooks for Turned {
    fn spline_inverse(&mut self, which: i32, s: &mut Spline) {
        self.0.push(which);
        s.keys.reverse();
    }
}

#[test]
fn a_hole_wound_like_the_outline_is_turned_round() {
    let mut f = face();
    assert!(!is_face_visible(&f, false));
    let mut h = Turned::default();
    face_ordinate_points(&mut f, false, &mut h);
    assert_eq!(h.0, [0]);
    assert!(is_face_visible(&f, false));
    // an outline facing down is turned only when asked, and then the hole is too
    let mut g = face();
    g.exterior.keys.reverse();
    let mut h = Turned::default();
    face_ordinate_points(&mut g, true, &mut h);
    assert_eq!(h.0, [-1, 0]);
}

#[test]
fn a_face_becomes_one_polygon_with_a_separator_per_hole() {
    let mut f = face();
    f.holes[0].keys.reverse();
    let m = face_convert_to_mesh(&f, false, &mut MeshEnv::default(), &mut ()).unwrap();
    assert_eq!((m.nb_vertices(), m.nb_polygons()), (8, 1));
    let corners: Vec<i32> = m.polygons[0].nodes.iter().map(|c| c.vertex).collect();
    assert_eq!(corners, [0, 1, 2, 3, -1, 4, 5, 6, 7]);
    assert_eq!(m.polygons[0].normal, [0.0, 0.0, 1.0]);
    assert_eq!(face_nb_triangles(&f), 8);
    assert_eq!(crate::obj_global::mesh_nb_triangles(&m), 8);
    // a face without an outline gives nothing
    f.exterior = Spline::default();
    assert!(face_convert_to_mesh(&f, false, &mut MeshEnv::default(), &mut ()).is_none());
}

#[test]
fn indices_run_over_the_outline_and_then_the_holes() {
    let f = face();
    assert_eq!(face_global_to_local_index(&f, 3, false), (-1, 3));
    assert_eq!(face_global_to_local_index(&f, 5, false), (0, 1));
    assert_eq!(face_global_to_local_index(&f, 9, false), (0, 5));
    // with tangents (three per key) the walk may leave the last contour
    assert_eq!(face_global_to_local_index(&f, 13, true), (0, 1));
    assert_eq!(face_global_to_local_index(&f, 30, true), (1, 6));
    assert_eq!(face_local_to_global_index(&f, 0, 1, false), 5);
    assert_eq!(face_local_to_global_index(&f, 0, 1, true), 13);
    assert_eq!(face_segment_to_point(&f, 5), 15);
    assert_eq!(face_get_indexes(&f, false).unwrap().len(), 8);
    assert!(face_get_indexes(&f, true).is_none());
    let mut g = f.clone();
    face_delete_hole(&mut g, 0);
    assert_eq!(g.count, 1);
    face_delete_hole(&mut g, 1);
    assert_eq!((g.count, g.holes.len()), (0, 0));
}

