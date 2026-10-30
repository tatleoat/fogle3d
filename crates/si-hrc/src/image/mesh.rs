// SPDX-License-Identifier: MIT
//! `meshBinaryRead` of `binary.c` on the image: a polygon mesh with its vertices, polygons,
//! corners and the file's edge list (finding 0004, "Mesh").

use super::geom::{edge_allocate, edge_at, mesh, mesh_allocate, polygon_allocate, polygon_node_allocate, vertex_allocate, Mesh};
use super::globals::{CURR_MDL, FILE_VERSION};
use super::io::{alloc_fits, byteswap_fread, file_skip_field, float_fread, vector_fread};
use super::matio::next_token;
use super::outside::{Outside, Routine};
use super::proto::{slot, Prototypes};
use si_mem::{Mem, Ptr32};
use si_x87::x87::F80;

/// `SI_meshUpdateNodes (0x08458cf8)`, `mesh_b.c`: the rebuild of a mesh's stars, edges and
/// normals (finding 0015); its second argument says whether to compute corner normals.
pub const SI_MESH_UPDATE_NODES: Routine = Routine { name: "SI_meshUpdateNodes", addr: 0x08458cf8 };
/// `SI_meshComputesPolygonNormal (0x084599f4)`, `mesh_b.c`.
pub const SI_MESH_COMPUTES_POLYGON_NORMAL: Routine = Routine { name: "SI_meshComputesPolygonNormal", addr: 0x084599f4 };
/// `SI_meshComputeHardEdge (0x0845a6e4)`, `mesh_b.c`.
pub const SI_MESH_COMPUTE_HARD_EDGE: Routine = Routine { name: "SI_meshComputeHardEdge", addr: 0x0845a6e4 };
/// `SI_meshComputesVertexNormal (0x08459e8c)`, `mesh_b.c`.
pub const SI_MESH_COMPUTES_VERTEX_NORMAL: Routine = Routine { name: "SI_meshComputesVertexNormal", addr: 0x08459e8c };

/// [soft 0x08458e10 SI_setCurrentModel]: the node whose mesh is being rebuilt.
pub fn set_current_model(m: &mut Mem, node: u32) {
    m[CURR_MDL] = node;
}

/// [soft 0x083ef514 meshBinaryRead]: a mesh from the file, for the node `node`.
///
/// The mesh's own tokens: flags (6), the old "faceted" short (1), the angle in degrees
/// with its cosine (2; the product with the double 0.0174532925 at 0x0865bcb8 is rounded to
/// a double, handed to the C library's `cos`, and the answer stored as a float), the
/// vertices (7, or 3 with a short count), polygons (8, or 4) and edges (9, or 5), the six
/// shorts 0x20a to 0x212 and three floats 0x40d to 0x40f, and the number of texture
/// coordinate sets (0x413), which goes into the node (+0x424).
///
/// The file's edges are read into a table of their own, numbered from 1. If `keep_map` is
/// set, `*edge_map` gets a new array of count + 1 ints in which entry i is first i. After
/// the tokens the mesh is rebuilt (`SI_meshUpdateNodes`, with the node as the "current
/// model"; corner normals are computed unless a corner stored one). Then, if the file had
/// an edge list, each listed edge with a first vertex looks through that vertex's star for
/// its second vertex; where it finds it, the rebuilt edge gets the listed edge's flag and
/// the map entry becomes the rebuilt edge's number. The file's table is freed, and without
/// stored normals the three normal passes run once more.
///
/// The count of edges lies in a local that only the edge token sets, and is used after
/// the tokens only if that token came.
pub fn mesh_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>, node: u32, edge_map: u32, keep_map: i16) -> Ptr32<Mesh> {
    // the frame: the edge count, a 4-byte scratch, and the word whose low half is the
    // scratch short and whose high half is the token
    const FRAME: u32 = 12;
    let mesh_ptr = mesh_allocate(m, o);
    if mesh_ptr.is_null() {
        return mesh_ptr;
    }
    let p = mesh_ptr.addr();
    let frame = m.local(FRAME);
    let (nb_edges_at, scratch4, scratch, token_at) = (frame, frame + 4, frame + 8, frame + 10);
    let table = protos.addr();
    let list_at = |k: usize| table.wrapping_add(4 * k as u32);
    let mut file_edges = 0u32;
    let mut no_normals: i16 = 1;
    // a count stored as a short (old tokens) or as an int
    let count = |m: &mut Mem, o: &mut dyn Outside, short: bool, at: u32| {
        if short {
            byteswap_fread(m, o, scratch, 2, 1, file);
            let v = m.i16(scratch) as i32;
            m.set_i32(at, v);
        } else {
            byteswap_fread(m, o, at, 4, 1, file);
        }
    };
    let mut index: i16 = 0;
    next_token(m, o, file, list_at(slot::MESH), token_at, &mut index);
    while m.i16(token_at) != 0 {
        let token = m.i16(token_at);
        match token {
            0x20a | 0x20b | 0x20c | 0x210 | 0x211 | 0x212 | 6 => {
                let off = match token {
                    0x20a => 0x52,
                    0x20b => 0x50,
                    0x20c => 0x38,
                    0x210 => 0x48,
                    0x211 => 0x4a,
                    0x212 => 0x4c,
                    _ => mesh::FLAGS,
                };
                byteswap_fread(m, o, p + off, 2, 1, file);
            }
            0x40d | 0x40e | 0x40f => {
                float_fread(m, o, p + 0x3c + 4 * (token as u32 - 0x40d), 4, 1, file);
            }
            0x413 => {
                byteswap_fread(m, o, node.wrapping_add(0x424), 4, 1, file);
            }
            1 => {
                byteswap_fread(m, o, scratch, 2, 1, file);
                if m.i16(scratch) != 0 {
                    m.or_u8(p + mesh::FLAGS, 2);
                }
            }
            2 => {
                byteswap_fread(m, o, p + mesh::ANGLE, 4, 1, file);
                let radians = (F80::from_f32(m.f32(p + mesh::ANGLE)) * F80::from_f64(0.0174532925)).to_f64();
                let c = o.cos(m, radians);
                m.set_f32(p + mesh::ANGLE_COS, c as f32);
            }
            3 | 7 => {
                count(m, o, token == 3, p + mesh::NB_VERTICES);
                let n = m.i32(p + mesh::NB_VERTICES);
                let mut v = if alloc_fits(m, o, file, (n as u32).wrapping_mul(mesh::VERTEX), n as u32) { vertex_allocate(m, n) } else { 0 };
                m.set_u32(p + mesh::VERTICES, v);
                let mut left = m.i32(p + mesh::NB_VERTICES);
                while 0 < left {
                    let mut index: i16 = 0;
                    next_token(m, o, file, list_at(slot::VERTEX), token_at, &mut index);
                    left -= 1;
                    while m.i16(token_at) != 0 {
                        match m.i16(token_at) {
                            1 => {
                                vector_fread(m, o, v, 0xc, 1, file);
                            }
                            4 => {
                                byteswap_fread(m, o, v + mesh::VERTEX_FLAG, 2, 1, file);
                            }
                            t => {
                                file_skip_field(m, o, file, t);
                            }
                        }
                        next_token(m, o, file, list_at(slot::VERTEX), token_at, &mut index);
                    }
                    v = v.wrapping_add(mesh::VERTEX);
                }
            }
            4 | 8 => {
                count(m, o, token == 4, p + mesh::NB_POLYGONS);
                let n = m.i32(p + mesh::NB_POLYGONS);
                let mut poly = if alloc_fits(m, o, file, (n as u32).wrapping_mul(mesh::POLYGON), n as u32) { polygon_allocate(m, n) } else { 0 };
                m.set_u32(p + mesh::POLYGONS, poly);
                let mut left = m.i32(p + mesh::NB_POLYGONS);
                while 0 < left {
                    let mut index: i16 = 0;
                    next_token(m, o, file, list_at(slot::POLYGON), token_at, &mut index);
                    left -= 1;
                    while m.i16(token_at) != 0 {
                        match m.i16(token_at) {
                            2 => {
                                byteswap_fread(m, o, scratch, 2, 1, file);
                                let n = m.u16(scratch);
                                m.set_u16(poly + mesh::POLYGON_NB_NODES, n);
                                let nodes = if alloc_fits(m, o, file, (n as u32 + 3) * mesh::NODE, n as u32) { polygon_node_allocate(m, n as i32) } else { 0 };
                                m.set_u32(poly + mesh::POLYGON_NODES, nodes);
                                let mut k: i32 = 0;
                                while k < m.u16(poly + mesh::POLYGON_NB_NODES) as i32 {
                                    read_corner(m, o, file, protos, node, poly, k, scratch4, scratch, token_at, &mut no_normals);
                                    k += 1;
                                }
                            }
                            1 => {
                                vector_fread(m, o, poly, 0xc, 1, file);
                            }
                            3 => {
                                byteswap_fread(m, o, poly + mesh::POLYGON_MATERIAL, 2, 1, file);
                            }
                            4 => {
                                byteswap_fread(m, o, poly + mesh::POLYGON_FLAG, 2, 1, file);
                            }
                            t => {
                                file_skip_field(m, o, file, t);
                            }
                        }
                        next_token(m, o, file, list_at(slot::POLYGON), token_at, &mut index);
                    }
                    poly = poly.wrapping_add(mesh::POLYGON);
                }
            }
            5 | 9 => {
                count(m, o, token == 5, nb_edges_at);
                let n = m.i32(nb_edges_at);
                let blocks = (n.wrapping_add(1) >> 10).wrapping_add(1) as u32;
                file_edges = if alloc_fits(m, o, file, blocks.wrapping_mul(0x400 * mesh::EDGE), n as u32) { edge_allocate(m, n) } else { 0 };
                if keep_map != 0 {
                    let map = if alloc_fits(m, o, file, (n as u32).wrapping_add(1).wrapping_mul(4), n as u32) { m.si_calloc((n as u32).wrapping_add(1), 4) } else { 0 };
                    m.set_u32(edge_map, map);
                }
                let mut i: i32 = 1;
                while i <= m.i32(nb_edges_at) {
                    let e = edge_at(m, file_edges, i);
                    if edge_map != 0 {
                        let map = m.u32(edge_map);
                        m.set_i32(map.wrapping_add(4 * i as u32), i);
                    }
                    let mut index: i16 = 0;
                    next_token(m, o, file, list_at(slot::EDGE), token_at, &mut index);
                    i += 1;
                    while m.i16(token_at) != 0 {
                        match m.i16(token_at) {
                            1 => {
                                for k in 0..2 {
                                    byteswap_fread(m, o, scratch, 2, 1, file);
                                    let v = m.i16(scratch) as i32;
                                    m.set_i32(e + 4 * k, v);
                                }
                            }
                            2 | 5 => {}
                            3 => {
                                byteswap_fread(m, o, e + mesh::EDGE_FLAG, 2, 1, file);
                            }
                            4 => {
                                byteswap_fread(m, o, e, 4, 1, file);
                                byteswap_fread(m, o, e + 4, 4, 1, file);
                            }
                            t => {
                                file_skip_field(m, o, file, t);
                            }
                        }
                        next_token(m, o, file, list_at(slot::EDGE), token_at, &mut index);
                    }
                }
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        next_token(m, o, file, list_at(slot::MESH), token_at, &mut index);
    }
    set_current_model(m, node);
    o.call(m, &SI_MESH_UPDATE_NODES, &[p, no_normals as i32 as u32]);
    set_current_model(m, 0);
    if file_edges != 0 {
        let mut i: i32 = 1;
        while i <= m.i32(nb_edges_at) {
            let e = edge_at(m, file_edges, i);
            if m.i32(e) != -1 {
                let v = m.u32(p + mesh::VERTICES).wrapping_add((m.i32(e) as u32).wrapping_mul(mesh::VERTEX));
                let mut k: i32 = 0;
                while k < m.u16(v + mesh::VERTEX_NB_STAR) as i32 {
                    let entry = m.u32(v + mesh::VERTEX_STAR).wrapping_add(0xc * k as u32);
                    if m.i32(entry) == m.i32(e + 4) {
                        // the star's edge number is signed by direction
                        let mut number = m.i32(entry + 4);
                        if number < 1 {
                            number = number.wrapping_neg();
                        }
                        let rebuilt = edge_at(m, m.u32(p + mesh::EDGES), number);
                        if edge_map != 0 {
                            let map = m.u32(edge_map);
                            m.set_i32(map.wrapping_add(4 * i as u32), number);
                        }
                        let flag = m.u16(e + mesh::EDGE_FLAG);
                        m.set_u16(rebuilt + mesh::EDGE_FLAG, flag);
                        break;
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        let blocks = (m.i32(nb_edges_at).wrapping_add(1) >> 10).wrapping_add(1);
        let mut k = 0;
        while k < blocks {
            let at = file_edges.wrapping_add(4 * k as u32);
            if m.u32(at) != 0 {
                m.free(m.u32(at));
                m.set_u32(at, 0);
            }
            k += 1;
        }
        m.free(file_edges);
        if no_normals != 0 {
            o.call(m, &SI_MESH_COMPUTES_POLYGON_NORMAL, &[p, 0xffff_ffff]);
            o.call(m, &SI_MESH_COMPUTE_HARD_EDGE, &[p, 0xffff_ffff]);
            o.call(m, &SI_MESH_COMPUTES_VERTEX_NORMAL, &[p, 0xffff_ffff]);
        }
    }
    m.release(FRAME);
    mesh_ptr
}

/// One corner of a polygon: vertex index (3, or 1 as a short), normal (2), texture pair
/// (4), colour (0x405; its fourth byte is forced to 0xff in a file below 3.71, the float
/// at 0x0865bcc0), and the texture pairs of the further coordinate sets (0xff06: an unused
/// length, then for each set after the first two floats, in a new zeroed array).
#[allow(clippy::too_many_arguments)]
fn read_corner(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>, node: u32, poly: u32, k: i32, scratch4: u32, scratch: u32, token_at: u32, no_normals: &mut i16) {
    let list_at = protos.addr().wrapping_add(4 * slot::POLY_NODE as u32);
    let corner = |m: &Mem| m.u32(poly + mesh::POLYGON_NODES).wrapping_add((k as u32).wrapping_mul(mesh::NODE));
    let mut index: i16 = 0;
    next_token(m, o, file, list_at, token_at, &mut index);
    while m.i16(token_at) != 0 {
        match m.i16(token_at) {
            2 => {
                *no_normals = 0;
                let at = corner(m) + mesh::NODE_NORMAL;
                vector_fread(m, o, at, 0xc, 1, file);
            }
            -0xfa => {
                byteswap_fread(m, o, scratch4, 4, 1, file);
                let sets = m.i32(node.wrapping_add(0x424)).wrapping_sub(1);
                let more = m.si_calloc((sets as u32).wrapping_mul(2), 4);
                let at = corner(m) + mesh::NODE_MORE_UV;
                m.set_u32(at, more);
                let mut i: i32 = 0;
                while i < m.i32(node.wrapping_add(0x424)).wrapping_sub(1) {
                    let pairs = m.u32(corner(m) + mesh::NODE_MORE_UV);
                    byteswap_fread(m, o, pairs.wrapping_add(8 * i as u32), 4, 1, file);
                    let pairs = m.u32(corner(m) + mesh::NODE_MORE_UV);
                    byteswap_fread(m, o, pairs.wrapping_add(8 * i as u32 + 4), 4, 1, file);
                    i += 1;
                }
            }
            1 => {
                byteswap_fread(m, o, scratch, 2, 1, file);
                let v = m.i16(scratch) as i32;
                let at = corner(m);
                m.set_i32(at, v);
            }
            4 => {
                let at = corner(m) + mesh::NODE_UV;
                byteswap_fread(m, o, at, 4, 1, file);
                let at = corner(m) + mesh::NODE_UV + 4;
                byteswap_fread(m, o, at, 4, 1, file);
            }
            3 => {
                let at = corner(m);
                byteswap_fread(m, o, at, 4, 1, file);
            }
            0x405 => {
                let at = corner(m) + mesh::NODE_COLOUR;
                byteswap_fread(m, o, at, 4, 1, file);
                if m[FILE_VERSION] < 3.71 {
                    let at = corner(m) + mesh::NODE_COLOUR + 3;
                    m.set_u8(at, 0xff);
                }
            }
            t => {
                file_skip_field(m, o, file, t);
            }
        }
        next_token(m, o, file, list_at, token_at, &mut index);
    }
}
