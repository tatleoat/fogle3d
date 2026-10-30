// SPDX-License-Identifier: MIT
use super::*;

fn pts(p: &[[f32; 2]]) -> Vec<P> {
    p.iter().map(|q| [q[0], q[1], 0.0]).collect()
}

#[test]
fn a_counter_clockwise_square_gives_two_ears_in_order() {
    let mut p = pts(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let mut idx = vec![0, 1, 2, 3];
    let (ok, t) = triangulate_contour(&mut p, &mut idx);
    assert!(ok);
    // the ear at corner 1 first, cut between 0 and 2; then what is left
    assert_eq!(t, [[0, 1, 2], [0, 2, 3]]);
}

#[test]
fn a_clockwise_square_is_cut_all_the_same() {
    let mut p = pts(&[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
    let mut idx = vec![0, 1, 2, 3];
    let (ok, t) = triangulate_contour(&mut p, &mut idx);
    assert!(ok);
    // no ear passes; after a whole round the ear at the last corner tested goes
    assert_eq!(t, [[3, 0, 1], [2, 3, 1]]);
}

#[test]
fn repeated_corners_are_dropped_and_coordinates_rounded() {
    let mut p = pts(&[[0.0, 0.0], [1.5000001, 0.0], [123.456789, 250.0], [0.12345678, 20000.5]]);
    let mut idx = vec![0, 0, 1, 2, 2, 3, 0];
    let (ok, t) = triangulate_contour(&mut p, &mut idx);
    assert!(ok);
    assert_eq!(&idx[..4], [1, 2, 3, 0]);
    assert_eq!(t.len(), 2);
    assert_eq!(p[1][0], 1.5);
    assert_eq!(p[2][0], 123.45);
    assert_eq!(p[3][0], 0.123456);
    assert_eq!(p[3][1], 20000.5);
}

#[test]
fn a_hole_is_joined_by_a_bridge_and_adds_two_triangles() {
    // outline counter-clockwise, hole clockwise
    let mut p = pts(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]]);
    let mut idx = vec![0, 1, 2, 3, -1, 4, 5, 6, 7];
    let (ok, t) = triangulate_contour(&mut p, &mut idx);
    assert!(ok);
    assert_eq!(t.len(), 8);
    let mut holes = 1;
    let mut work = idx.clone();
    assert!(merge_holes(&p, &idx, &mut holes, &mut work));
    assert_eq!((holes, work.len()), (0, 10));
    // the first outline corner that sees a hole corner is 0, which sees 4
    assert_eq!(work, [0, 4, 5, 6, 7, 4, 0, 1, 2, 3]);
}

#[test]
fn segments_that_touch_end_to_end_do_not_cross() {
    let v = |x: f32, y: f32| [x, y, 0.0];
    assert!(segment_intersect(v(0.0, 0.0), v(2.0, 2.0), v(0.0, 2.0), v(2.0, 0.0)));
    assert!(!segment_intersect(v(0.0, 0.0), v(1.0, 1.0), v(1.0, 1.0), v(2.0, 0.0)));
    // one ends on the other: counted as a crossing
    assert!(segment_intersect(v(0.0, 0.0), v(2.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)));
    assert!(!segment_intersect(v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0), v(3.0, 0.0)));
}
