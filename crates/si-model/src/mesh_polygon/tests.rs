// SPDX-License-Identifier: MIT
use super::*;
use crate::mesh_b::update;

/// Two quadrilaterals in the plane z = 0 sharing an edge, and a triangle apart from them.
fn strip() -> Mesh {
    let mut m = Mesh::with_vertices(&[
        [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0],
        [5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 0.0, 1.0],
    ]);
    m.push_polygon(&[0, 1, 2, 3]);
    m.push_polygon(&[1, 4, 5, 2]);
    m.push_polygon(&[6, 7, 8]);
    m
}

#[test]
fn lists_read_in_reverse_order_of_insertion() {
    let mut l = MeshList::default();
    add_list(&mut l, ListItem::Polygon(3));
    add_list(&mut l, ListItem::Polygon(5));
    assert_eq!(l.iter().map(|n| n.item).collect::<Vec<_>>(), [ListItem::Polygon(5), ListItem::Polygon(3)]);
    let mut tail = MeshList::default();
    add_list(&mut tail, ListItem::Vertex(1));
    concat_list(&mut l, tail);
    assert_eq!(l.iter().last().unwrap().item, ListItem::Vertex(1));
    assert_eq!((cnt_list(&l), find_list(&l, ListItem::Polygon(3))), (3, Some(1)));
    free_list(&mut l);
    assert!(l.is_empty());
}

#[test]
fn a_group_of_connected_selected_polygons_shares_one_system() {
    let mut m = strip();
    for p in &mut m.polygons {
        p.flag = POLY_SELECTED;
    }
    m.polygons[2].flag = 0;
    let mut env = MeshEnv::default();
    update(&mut m, &mut env);
    // the list reads last polygon first; both nodes carry the same system
    let items: Vec<_> = m.selected_polygons.iter().map(|n| n.item).collect();
    assert_eq!(items, [ListItem::Polygon(1), ListItem::Polygon(0)]);
    let a = m.selected_polygons.at(0).unwrap().matrix;
    assert_eq!(a, m.selected_polygons.at(1).unwrap().matrix);
    // normal +z in row 1, the centroid of the two squares in row 3
    assert_eq!(a[1], [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(a[3], [1.0, 0.5, 0.0, 1.0]);
    // six vertices, each with the selected polygons at it
    assert_eq!(cnt_list(&m.selected_vertices), 6);
    let v1 = m.selected_vertices.iter().find(|n| n.item == ListItem::Vertex(1)).unwrap();
    assert_eq!(cnt_list(&v1.sub), 2);
    assert!(env.messages.is_empty());
}

#[test]
fn a_scaling_moves_a_shared_vertex_once_and_marks_are_cleared() {
    let mut m = strip();
    m.polygons[0].flag = POLY_SELECTED;
    m.polygons[1].flag = POLY_SELECTED;
    update(&mut m, &mut MeshEnv::default());
    let mut models = Models::default();
    let node = models.allocate(None);
    transform_selected_polygons(&mut m, &models, node, MOVE_SCALE, Vector::new(2.0, 2.0, 2.0), &Globals::default(), &mut ());
    // about the centroid (1, 0.5, 0): the corner (0, 0, 0) goes to (−1, −0.5, 0)
    assert_eq!(m.vertices[0].pos, [-1.0, -0.5, 0.0]);
    assert_eq!(m.vertices[1].pos, [1.0, -0.5, 0.0]);
    assert_eq!(m.vertices[6].pos, [5.0, 0.0, 0.0]);
    assert!(m.vertices.iter().all(|v| v.flag >= 0));
    assert_eq!(m.vertices[1].flag, 0x20);
    assert!(m.polygons.iter().all(|p| p.flag & 0x400 == 0));
    assert_eq!(m.polygons[0].normal, [0.0, 0.0, 1.0]);
}

#[test]
fn projecting_a_vector_along_the_normal_leaves_nothing() {
    let (v, ok) = proj_vect([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
    assert_eq!((v, ok), ([0.0, 0.0, 0.0], false));
    let (v, ok) = proj_vect([1.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
    assert_eq!((v, ok), ([1.0, 0.0, 0.0], true));
    // a dot product within the tolerance: the vector is returned as it is
    assert_eq!(proj_vect([1.0, 0.0, 0.00005], [0.0, 0.0, 1.0]), ([1.0, 0.0, 0.00005], true));
}
