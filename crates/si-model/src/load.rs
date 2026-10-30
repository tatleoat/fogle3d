// SPDX-License-Identifier: MIT
//! From a file's mesh to the program's mesh: what `meshBinaryRead` (`binary.c`) does once it
//! has read the last token (finding 0015, "After reading").
//!
//! A file holds vertex positions, polygons with their corners and a list of edges (two
//! vertices and a flag each). The reader keeps the edge list aside, rebuilds the mesh's own
//! edges from the polygons and copies each listed edge's flag to the rebuilt edge between
//! the same two vertices. If no corner of the file had a normal token, it computes the
//! normals; otherwise the stored ones stand.

use si_x87::{trig, x};

use crate::mesh::{Mesh, PolyNode, Polygon, Vertex};
use crate::mesh_b::{self, MeshEnv};

/// The double the reader multiplies an angle in degrees by (0x0865bcb8): 0.0174532925, ten
/// digits of π/180.
const DEG_TO_RAD: f64 = 0.0174532925;

/// The cosine the reader stores beside a discontinuity angle read from a file: the float
/// angle times the double constant, rounded to double, through the C library's `cos`,
/// rounded to float. Class: libm. (For the default 60 degrees this is 0.5, the value the
/// allocator stores.)
pub fn cos_of_angle(degrees: f32) -> f32 {
    trig::cos((x(degrees) * DEG_TO_RAD).to_f64()).to_f32()
}

/// The mesh as the reader has it when it reaches its last token: what the file gave, no
/// stars, no edges. `cos_angle` is recomputed from the angle, which is what the reader does
/// when the file has the angle token and changes nothing when it has not.
pub fn mesh_from_file(f: &si_hrc::Mesh) -> Mesh {
    let mut m = Mesh::default();
    m.angle = f.angle;
    m.cos_angle = cos_of_angle(f.angle);
    m.flags = f.flag as u16;
    m.mr_view = f.x38;
    m.mr = f.approx;
    m.mr_min = f.x48;
    m.mr_max = f.x4a;
    m.mr_rec = f.x4c;
    m.mr_approx = f.x50;
    m.mr_first = f.x52;
    m.vertices = f.vertices.iter().map(|v| Vertex { pos: v.pos, star: Vec::new(), flag: v.flag }).collect();
    m.polygons = f
        .polygons
        .iter()
        .map(|p| Polygon {
            normal: p.normal,
            nodes: p
                .nodes
                .iter()
                .map(|n| PolyNode {
                    vertex: n.vertex,
                    edge: 0,
                    normal: n.normal,
                    uv: n.uv,
                    color: n.colour,
                    extra_uv: n.more_uv.clone(),
                })
                .collect(),
            material: p.material,
            flag: p.flag,
        })
        .collect();
    m
}

/// What the tail of the reader did, for the caller's report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    /// False if a polygon could not be filed (`SI_meshUpdateNodes` returned 0; the reader
    /// ignores it).
    pub filed: bool,
    /// For each edge of the file's list, in order, the number of the rebuilt edge that got
    /// its flag; 0 where the list names two vertices that no polygon joins. This is the
    /// table the reader fills for its caller when asked (its fourth argument).
    pub edge_map: Vec<i32>,
}

/// The tail of the mesh reader (the part of `meshBinaryRead`, 0x083ef514, after its token
/// loop; that function belongs to `si-hrc`, which stops where this starts):
///
/// 1. `SI_setCurrentModel(model)`, `SI_meshUpdateNodes(mesh, compute_normals)`,
///    `SI_setCurrentModel(NULL)`;
/// 2. for every edge of the file's list whose first vertex is not −1: the star of that
///    vertex is searched for the second vertex, and the rebuilt edge found there gets the
///    listed edge's flag (the whole 16 bits, replacing what the rebuild computed);
/// 3. if no normal was stored: polygon normals, border and discontinuity edges and corner
///    normals are computed again, now with the file's flags in place.
///
/// Steps 2 and 3 are skipped in the original when the file has no edge list at all;
/// `si-hrc` does not record that, and with an empty list step 2 does nothing and step 3
/// repeats step 1's result. A listed vertex outside the vertex array makes the original
/// read wild memory; here such an edge is skipped.
pub fn finish_read(mesh: &mut Mesh, file_edges: &[si_hrc::Edge], compute_normals: bool, model_name: Option<&[u8]>, env: &mut MeshEnv) -> Loaded {
    env.set_current_model(model_name);
    let filed = mesh_b::update_nodes(mesh, compute_normals, env);
    env.set_current_model(None);
    let mut edge_map = Vec::with_capacity(file_edges.len());
    for fe in file_edges {
        let mut found = 0;
        if fe.v[0] != -1 {
            if let Some(v) = mesh.vertices.get(fe.v[0] as usize) {
                if let Some(s) = v.star.iter().find(|s| s.vertex == fe.v[1]) {
                    found = s.edge.unsigned_abs() as i32;
                    mesh.edges[found as usize].flag = fe.flag;
                }
            }
        }
        edge_map.push(found);
    }
    if compute_normals {
        mesh_b::computes_polygon_normal(mesh, -1);
        mesh_b::compute_hard_edge(mesh, -1);
        mesh_b::computes_vertex_normal(mesh, -1);
    }
    Loaded { filed, edge_map }
}

/// A file's mesh brought to the state the program has after loading it.
pub fn load_mesh(f: &si_hrc::Mesh, model_name: Option<&[u8]>, env: &mut MeshEnv) -> (Mesh, Loaded) {
    let mut m = mesh_from_file(f);
    let loaded = finish_read(&mut m, &f.edges, f.compute_normals, model_name, env);
    (m, loaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_degrees_is_one_half() {
        assert_eq!(cos_of_angle(60.0), 0.5);
        assert_eq!(cos_of_angle(0.0), 1.0);
        assert!(cos_of_angle(90.0).abs() < 1e-7);
    }

    #[test]
    fn listed_flags_replace_computed_ones_and_normals_follow() {
        let mut f = si_hrc::Mesh::default();
        for pos in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 1.0], [2.0, 1.0, 1.0]] {
            f.vertices.push(si_hrc::Vertex { pos, flag: 0 });
        }
        for q in [[0, 1, 2, 3], [1, 4, 5, 2]] {
            let nodes = q.iter().map(|&vertex| si_hrc::PolyNode { vertex, ..si_hrc::PolyNode::default() }).collect();
            f.polygons.push(si_hrc::Polygon { nodes, nnodes: 4, ..si_hrc::Polygon::default() });
        }
        // the fold 1–2 is 45 degrees: smooth by the angle, hard by the file's flag
        f.edges.push(si_hrc::Edge { v: [2, 1], flag: 0x08 });
        f.edges.push(si_hrc::Edge { v: [0, 5], flag: 0x08 }); // joins nothing
        let mut env = MeshEnv::default();
        let (m, loaded) = load_mesh(&f, Some(b"fold"), &mut env);
        assert!(loaded.filed);
        assert_eq!(loaded.edge_map.len(), 2);
        assert_ne!(loaded.edge_map[0], 0);
        assert_eq!(loaded.edge_map[1], 0);
        let e = m.edges[loaded.edge_map[0] as usize];
        assert_eq!((e.v0.min(e.v1), e.v0.max(e.v1), e.flag), (1, 2, 0x08));
        // with the user edge each side keeps its own normal at vertices 1 and 2
        for p in &m.polygons {
            assert!(p.nodes.iter().all(|n| n.normal == p.normal));
        }
        // without it the two normals are averaged there
        f.edges.clear();
        let (m, _) = load_mesh(&f, None, &mut env);
        let at = |p: usize, v: i32| m.polygons[p].nodes.iter().find(|n| n.vertex == v).unwrap().normal;
        assert_eq!(at(0, 1), at(1, 1));
        assert_ne!(at(0, 1), m.polygons[0].normal);
        assert_eq!(at(0, 0), m.polygons[0].normal);
    }
}
