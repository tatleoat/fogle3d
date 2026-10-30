// SPDX-License-Identifier: MIT
//! Constructors of Node/Mesh/Material/Light/Camera values from plain arrays (our own code; none of this
//! reproduces a function of the original). The structures come from si-hrc and si-elemio; their
//! defaults are the originals' allocators', so a record built here starts from what the program
//! itself starts from.
use si_hrc::{Definition, Edge, Material, Mesh, Node, PolyNode, Polygon, Vertex};
use std::collections::BTreeMap;

/// One corner of a triangle (or polygon): vertex index, normal, texture coordinate.
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub vertex: usize,
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// What a mesh node is made from.
pub struct MeshData {
    pub name: String,
    pub verts: Vec<[f32; 3]>,
    /// Polygons (any size) with the corners they use.
    pub polys: Vec<Vec<Corner>>,
    /// Material index of each polygon.
    pub poly_material: Vec<i16>,
    pub materials: Vec<Material>,
}

/// Edge flags of the corpus's meshes, by the bits seen there: 0x8000 is set on every edge a model file lists
/// (finding 0004's edge table; the other bits are not understood and left clear).
pub const EDGE_FLAG: i16 = i16::MIN;

/// A mesh node: kind 4, one mesh definition, node normals stored (so the loader does not compute
/// them), edges listed once per vertex pair.
pub fn mesh_node(d: &MeshData) -> Node {
    let mut mesh = Mesh::default();
    mesh.flag = 0xc; // what the corpus's polygon meshes carry
    mesh.compute_normals = false; // normals come from the corners
    mesh.nvertices = d.verts.len() as i32;
    mesh.vertices = d.verts.iter().map(|p| Vertex { pos: *p, flag: 0 }).collect();
    mesh.npolygons = d.polys.len() as i32;
    let mut edges: BTreeMap<(i32, i32), ()> = BTreeMap::new();
    for (i, p) in d.polys.iter().enumerate() {
        let nodes: Vec<PolyNode> =
            p.iter().map(|c| PolyNode { vertex: c.vertex as i32, normal: c.normal, uv: c.uv, ..PolyNode::default() }).collect();
        for k in 0..nodes.len() {
            let (a, b) = (nodes[k].vertex, nodes[(k + 1) % nodes.len()].vertex);
            edges.insert((a.min(b), a.max(b)), ());
        }
        mesh.polygons.push(Polygon { normal: [0.0; 3], nnodes: nodes.len() as u16, nodes, material: d.poly_material.get(i).copied().unwrap_or(0), flag: 0 });
    }
    mesh.edges = edges.keys().map(|&(a, b)| Edge { v: [a, b], flag: EDGE_FLAG }).collect();
    mesh.nedges = mesh.edges.len() as i32;
    let mut n = Node::default();
    n.name = d.name.as_bytes().to_vec();
    n.kind = 4;
    n.definitions = vec![Definition::Mesh(mesh)];
    n.nb_materials = d.materials.len() as i16;
    n.materials = d.materials.iter().cloned().map(Some).collect();
    n
}

/// A flat material: ambient, diffuse, specular colours, exponent, transparency, reflectivity.
pub fn flat_material(name: &str, ambient: [f32; 3], diffuse: [f32; 3], specular: [f32; 3], exponent: f32, transparency: f32, reflectivity: f32) -> Material {
    let mut m = Material::default();
    m.name = name.as_bytes().to_vec();
    m.ambient = ambient;
    m.diffuse = diffuse;
    m.specular = specular;
    m.exponent = exponent;
    m.transparency = transparency;
    m.reflectivity = reflectivity;
    m
}

/// The plain arrays of a mesh node read back (for the rebuild test): vertices, and per polygon
/// its corners and material.
pub fn mesh_data_of(n: &Node) -> Option<MeshData> {
    let Some(Definition::Mesh(m)) = n.definitions.first() else { return None };
    Some(MeshData {
        name: String::from_utf8_lossy(&n.name).to_string(),
        verts: m.vertices.iter().map(|v| v.pos).collect(),
        polys: m.polygons.iter().map(|p| p.nodes.iter().map(|c| Corner { vertex: c.vertex as usize, normal: c.normal, uv: c.uv }).collect()).collect(),
        poly_material: m.polygons.iter().map(|p| p.material).collect(),
        materials: n.materials.iter().flatten().cloned().collect(),
    })
}
