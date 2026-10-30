// SPDX-License-Identifier: MIT
//! The lists of selected polygons and their vertices on the image (`meshPolygon.c`;
//! finding 0015, "Selected polygons"): the list primitives, the walk over connected
//! polygons, the two lists of a mesh with each node's coordinate system. The value form is
//! [`crate::mesh_polygon`] (checked by emulation); the floating point of a group's
//! coordinate system and of a polygon's centroid is computed there, on the image's values.
//!
//! A list node (`__SI_MeshListNode`) is a zeroed block of 0x50 bytes: the item (+0), two
//! lists of its own (+4, +8), a matrix (+0xc), the next node (+0x4c).

use si_mem::Mem;

use super::{callee, off as mo, polygon_at};
use crate::model_image::{Callee, Outside};

/// `SI_stopActivity` (0x083d1adc): the busy cursor back to normal.
pub const SI_STOP_ACTIVITY: Callee = Callee { name: "SI_stopActivity", addr: 0x083d1adc };

si_mem::globals! {
    /// Not 1 while the busy cursor is not wanted (`SI_meshUpdatePolygonLists` then stops
    /// the activity cursor at its end).
    SI_BUSY_CURSOR: i16 = 0x086792b2, "SI_busyCursor";
}

const NEXT: u32 = 0x4c;

/// [soft 0x08455cc8 SI_meshAddList]: a new node (`SI_calloc(1, 0x50)`) holding `item`, put in
/// front of the list `*pp`; answers it (null when the allocation fails).
pub fn add_list(m: &mut Mem, pp: u32, item: u32) -> u32 {
    let node = m.si_calloc(1, 0x50);
    if node == 0 {
        return 0;
    }
    m.set_u32(node + NEXT, 0);
    m.set_u32(node, item);
    m.set_u32(node + 4, 0);
    m.set_u32(node + 8, 0);
    if m.u32(pp) != 0 {
        m.set_u32(node + NEXT, m.u32(pp));
    }
    m.set_u32(pp, node);
    node
}

/// [soft 0x08455d18 SI_meshConcatList]: the list `tail` hung behind the last node of `*pp`
/// (or made the list when it is empty); nothing for a null `pp`.
pub fn concat_list(m: &mut Mem, pp: u32, tail: u32) {
    if pp == 0 {
        return;
    }
    let mut node = m.u32(pp);
    if node == 0 {
        m.set_u32(pp, tail);
        return;
    }
    while m.u32(node + NEXT) != 0 {
        node = m.u32(node + NEXT);
    }
    m.set_u32(node + NEXT, tail);
}

/// [soft 0x08455d9c SI_meshFindList]: the first node of `*pp` holding `item`, or null.
pub fn find_list(m: &Mem, pp: u32, item: u32) -> u32 {
    let mut node = m.u32(pp);
    while node != 0 && m.u32(node) != item {
        node = m.u32(node + NEXT);
    }
    node
}

/// [soft 0x08455dc0 SI_meshCntList]: the number of nodes of `*pp`.
pub fn cnt_list(m: &Mem, pp: u32) -> i32 {
    let mut n = 0;
    let mut node = m.u32(pp);
    while node != 0 {
        n += 1;
        node = m.u32(node + NEXT);
    }
    n
}

/// [soft 0x08455ff4 clearTagMark]: the walk's marks (0x4, 0x8) cleared on every polygon of
/// the list (a node without an item is passed over).
pub fn clear_tag_mark(m: &mut Mem, mut node: u32) {
    while node != 0 {
        let item = m.u32(node);
        if item != 0 {
            let b = m.u8(item + 0x14);
            m.set_u8(item + 0x14, b & 0xf3);
        }
        node = m.u32(node + NEXT);
    }
}

/// [soft 0x08456018 getPolygonNeighbors]: a new list of the polygons around the corners of
/// `poly` (through the vertices' stars) that carry neither mark (0x4, 0x8) and, with
/// `selected_only`, are selected; each is marked 0x8 as it is listed. The list is in the
/// reverse order of discovery.
pub fn get_polygon_neighbors(m: &mut Mem, mesh: u32, poly: u32, selected_only: i16) -> u32 {
    let head = m.local(4);
    m.set_u32(head, 0);
    let mut c = 0u32;
    while c < m.u16(poly + mo::POLYGON_NB_NODES) as u32 {
        let v = m.i32(m.u32(poly + mo::POLYGON_NODES).wrapping_add(mo::NODE * c));
        c += 1;
        if v == -1 {
            continue;
        }
        let vr = m.u32(mesh + mo::VERTICES).wrapping_add((v as u32).wrapping_mul(mo::VERTEX));
        let mut k = 0u32;
        while k < m.u16(vr + mo::VERTEX_NB_STAR) as u32 {
            let s = m.u32(vr + mo::VERTEX_STAR).wrapping_add(12 * k);
            k += 1;
            let (w, q) = (m.i32(s), m.i32(s + 8));
            if w == -1 || q == -1 {
                continue;
            }
            let qa = polygon_at(m, mesh, q);
            let f = m.u8(qa + 0x14);
            if f & 0xc == 0 && (selected_only == 0 || f & 1 != 0) {
                add_list(m, head, qa);
                let f = m.u8(qa + 0x14);
                m.set_u8(qa + 0x14, f | 8);
            }
        }
    }
    let out = m.u32(head);
    m.release(4);
    out
}

/// [soft 0x084560e4 getConnectedPolygons]: everything connected to `poly` through shared
/// vertices, depth first: a polygon not yet marked 0x4 is marked, put in front of `*out`
/// (with `selected_only`, only when selected), its neighbours listed
/// ([`get_polygon_neighbors`]) and visited in list order, and that list freed. The original
/// recurses once a polygon; here a stack of frames (the neighbour list and the node
/// reached) keeps its order of calls, allocations and frees.
pub fn get_connected_polygons(m: &mut Mem, mesh: u32, poly: u32, selected_only: i16, out: u32) {
    // (list head kept in a block of the image's stack, node reached)
    let mut frames: Vec<(u32, u32)> = Vec::new();
    let enter = |m: &mut Mem, frames: &mut Vec<(u32, u32)>, p: u32| {
        if p == 0 || m.u16(p + 0x14) & 4 != 0 {
            return;
        }
        let f = m.u16(p + 0x14);
        m.set_u16(p + 0x14, f | 4);
        if selected_only == 0 || m.u8(p + 0x14) & 1 != 0 {
            add_list(m, out, p);
        }
        let nb = get_polygon_neighbors(m, mesh, p, selected_only);
        if nb != 0 {
            let head = m.local(4);
            m.set_u32(head, nb);
            frames.push((head, nb));
        }
    };
    enter(m, &mut frames, poly);
    while let Some(&(head, node)) = frames.last() {
        if node == 0 {
            super::obj::mesh_free_list(m, head);
            frames.pop();
            m.release(4);
            continue;
        }
        let next = m.u32(node + NEXT);
        frames.last_mut().unwrap().1 = next;
        let child = m.u32(node);
        enter(m, &mut frames, child);
    }
}

/// [soft 0x08455ef4 SI_meshGetSelPolInfo]: with a table from vertex number to node
/// (`SI_calloc(nb_vertices, 4)`, freed at the end), every selected polygon (flag 0x1) is put
/// in front of `*polygons` (`SI_showActivity` before each), and for each of its corners'
/// vertices the vertex's node of `*vertices` (made the first time) gets the polygon in front
/// of its own list (+4). Nodes are added to what the lists hold.
pub fn get_sel_pol_info(m: &mut Mem, o: &mut dyn Outside, polygons: u32, vertices: u32, mesh: u32) {
    let table = m.si_calloc(m.u32(mesh + mo::NB_VERTICES), 4);
    let mut p = 0;
    while p < m.i32(mesh + mo::NB_POLYGONS) {
        let pa = polygon_at(m, mesh, p);
        p += 1;
        if m.u8(pa + 0x14) & 1 == 0 {
            continue;
        }
        o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
        add_list(m, polygons, pa);
        let mut c = 0u32;
        while c < m.u16(pa + mo::POLYGON_NB_NODES) as u32 {
            let v = m.i32(m.u32(pa + mo::POLYGON_NODES).wrapping_add(mo::NODE * c));
            c += 1;
            if v == -1 {
                continue;
            }
            let slot = table.wrapping_add((v as u32).wrapping_mul(4));
            let mut node = m.u32(slot);
            if node == 0 {
                let vr = m.u32(mesh + mo::VERTICES).wrapping_add((v as u32).wrapping_mul(mo::VERTEX));
                node = add_list(m, vertices, vr);
                m.set_u32(slot, node);
            }
            add_list(m, node + 4, pa);
        }
    }
    if table != 0 {
        m.free(table);
    }
}

/// [soft 0x084555d0 getAverageCoordSystem]: the coordinate system of the group of polygons
/// listed from `list`, as sixteen floats at `out` (`SI_identite`'s, rows 0 to 3 replaced):
/// the averaged normal, the direction of the last polygon's first edge projected off it,
/// their cross product, the averaged centroid. The arithmetic (with
/// `SI_meshPolygonCentroid`, `projVect` and `SI_vectorNormalize`) is the value form's
/// ([`crate::mesh_polygon::get_average_coord_system`]), on the image's values; the status
/// message for a group whose direction cannot be projected goes through [`Outside`].
pub fn get_average_coord_system(m: &mut Mem, o: &mut dyn Outside, list: u32, mesh: u32, out: u32) {
    use crate::mesh::{ListItem, ListNode, MeshList};
    let value = super::to_value(m, mesh);
    let polys = m.u32(mesh + mo::POLYGONS);
    let mut items = Vec::new();
    let mut node = list;
    while node != 0 {
        let p = (m.u32(node).wrapping_sub(polys) as i32) / mo::POLYGON as i32;
        items.push(ListNode { item: ListItem::Polygon(p), sub: MeshList::default(), sub2: MeshList::default(), matrix: [[0.0; 4]; 4] });
        node = m.u32(node + NEXT);
    }
    items.reverse(); // the value form keeps the head last
    let mut env = crate::mesh_b::MeshEnv::default();
    let frame = crate::mesh_polygon::get_average_coord_system(&MeshList { nodes: items }, &value, &mut env);
    for text in env.messages {
        let buf = m.local(64);
        m.put_cstr(buf, text.as_bytes());
        o.call(m, &callee::SI_STATUS_DISPLAY_MESSAGE, &[buf, 2]);
        m.release(64);
    }
    for (i, row) in frame.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_f32(out + 16 * i as u32 + 4 * j as u32, *v);
        }
    }
}

/// [soft 0x08456174 SI_meshGetSelPolCoordSys]: every node of the list `*list` gets its
/// group's coordinate system (`SI_calloc(1, 0x40)` for the matrix, freed). When the list has
/// as many nodes as the mesh has polygons, all are one group; otherwise the marks are
/// cleared ([`clear_tag_mark`]) and each listed polygon not yet marked 0x4 starts a group of
/// the selected polygons connected to it ([`get_connected_polygons`]), whose system is
/// copied into each member's node of `*list`, the group's list freed. The marks stay set.
pub fn get_sel_pol_coord_sys(m: &mut Mem, o: &mut dyn Outside, list: u32, mesh: u32) {
    if mesh == 0 {
        return;
    }
    let frame = m.si_calloc(1, 0x40);
    let np = m.i32(mesh + mo::NB_POLYGONS);
    if np < 1 || cnt_list(m, list) != np {
        clear_tag_mark(m, m.u32(list));
        let group = m.local(4);
        m.set_u32(group, 0);
        let mut node = m.u32(list);
        while node != 0 {
            o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
            let p = m.u32(node);
            if m.u8(p + 0x14) & 4 == 0 {
                get_connected_polygons(m, mesh, p, 1, group);
                if m.u32(group) != 0 {
                    get_average_coord_system(m, o, m.u32(group), mesh, frame);
                    let mut g = m.u32(group);
                    while g != 0 {
                        let at = find_list(m, list, m.u32(g));
                        m.copy(at + 0xc, frame, 0x40);
                        g = m.u32(g + NEXT);
                    }
                    super::obj::mesh_free_list(m, group);
                }
            }
            node = m.u32(node + NEXT);
        }
        m.release(4);
    } else {
        get_average_coord_system(m, o, m.u32(list), mesh, frame);
        let mut node = m.u32(list);
        while node != 0 {
            m.copy(node + 0xc, frame, 0x40);
            node = m.u32(node + NEXT);
        }
    }
    if frame != 0 {
        m.free(frame);
    }
}

/// [soft 0x084584a8 SI_meshUpdatePolygonLists]: the two lists of the mesh (+0x30, +0x34)
/// freed and built again ([`get_sel_pol_info`], [`get_sel_pol_coord_sys`]), between
/// `SI_showActivity` and, unless `SI_busyCursor` is 1, `SI_stopActivity`.
pub fn update_polygon_lists(m: &mut Mem, o: &mut dyn Outside, mesh: u32) {
    let stop = m[SI_BUSY_CURSOR] != 1;
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    super::obj::mesh_free_list(m, mesh + 0x30);
    super::obj::mesh_free_list(m, mesh + 0x34);
    get_sel_pol_info(m, o, mesh + 0x30, mesh + 0x34, mesh);
    get_sel_pol_coord_sys(m, o, mesh + 0x30, mesh);
    if stop {
        o.call(m, &SI_STOP_ACTIVITY, &[]);
    }
}

/// [soft 0x08458514 SI_meshCopyPolygonLists]: the lists of `dst` built ([`get_sel_pol_info`]
/// adding to what they hold, [`get_sel_pol_coord_sys`], with `SI_showActivity` after each),
/// then the matrices of `src`'s old polygon list copied onto the new list's nodes, head for
/// head (`SI_matrixCopy`). With `src` = `dst`, the old lists are taken off first and freed
/// at the end. Where the new list is shorter the original writes through a null pointer
/// (a recorded fault here).
pub fn copy_polygon_lists(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) {
    let old = m.local(8);
    m.set_u32(old, m.u32(src + 0x30));
    m.set_u32(old + 4, m.u32(src + 0x34));
    if src == dst {
        m.set_u32(src + 0x30, 0);
        m.set_u32(src + 0x34, 0);
    }
    get_sel_pol_info(m, o, dst + 0x30, dst + 0x34, dst);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    get_sel_pol_coord_sys(m, o, dst + 0x30, dst);
    o.call(m, &callee::SI_SHOW_ACTIVITY, &[]);
    let mut new = m.u32(dst + 0x30);
    let mut node = m.u32(old);
    while node != 0 {
        m.copy(new + 0xc, node + 0xc, 0x40);
        new = m.u32(new + NEXT);
        node = m.u32(node + NEXT);
    }
    if src == dst {
        super::obj::mesh_free_list(m, old);
        super::obj::mesh_free_list(m, old + 4);
    }
    m.release(8);
}
