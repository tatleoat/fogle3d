// SPDX-License-Identifier: MIT
//! `model-emucheck <vectors directory>`: compares this crate with the original, routine by
//! routine, on the test vectors of the emulation harness (finding 0011, `re/tools/emu`).
//!
//! The vectors are what a routine of the Linux `soft` binary left behind when its
//! instructions were stepped through in a CPU emulator on made-up inputs
//! (`re/tools/si-model/emu_gen.py` makes them and says what they hold). They are derived
//! from the binary and are not in the repository. Per vector file the report gives the
//! number of cases, how many the port reproduces bit for bit, how many it reproduces given
//! the recorded results of the C library's sine and cosine, and how many differ.

use std::path::Path;

mod facecheck;
mod image;
mod life;
mod set;
mod support;
mod tree;
mod tri;
mod world;

use si_model::mesh::{Mesh, PolyNode, Polygon, Vertex};
use si_model::mesh_b::{self, MeshEnv};
use si_model::node::Models;
use si_model::transform::{self, Globals};
use si_model::{primitive, NodeId};
use si_types::model::Model;
use si_x87::vectors::{check_dir, print_report, Case, Check, Field, Verdict, Word};

/// The six fields `emu_gen.py` dumps for a mesh.
fn dump(m: &Mesh) -> [(&'static str, Vec<u32>); 6] {
    let hdr = vec![m.angle.to_bits(), m.cos_angle.to_bits(), m.nb_vertices() as u32, m.nb_polygons() as u32, m.nb_edges() as u32, m.flags as u32, m.nb_display as u32];
    let (mut vpos, mut stars, mut polys, mut edges) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for v in &m.vertices {
        vpos.extend(v.pos.iter().map(|f| f.to_bits()));
        vpos.push(v.flag as u16 as u32);
        stars.push(v.star.len() as u32);
        for s in &v.star {
            stars.extend([s.vertex as u32, s.edge as u32, s.polygon as u32]);
        }
    }
    for p in &m.polygons {
        polys.extend(p.normal.iter().map(|f| f.to_bits()));
        polys.extend([p.nodes.len() as u32, p.material as u16 as u32, p.flag as u16 as u32]);
        for n in &p.nodes {
            polys.extend([n.vertex as u32, n.edge as u32]);
            polys.extend(n.normal.iter().map(|f| f.to_bits()));
        }
    }
    for e in m.edges.iter().skip(1) {
        edges.extend([e.v0 as u32, e.v1 as u32, e.p0 as u32, e.p1 as u32, e.x10 as u16 as u32, e.flag as u16 as u32]);
    }
    let display = m.display.iter().map(|&i| i as u32).collect();
    [("m.hdr", hdr), ("m.vpos", vpos), ("m.stars", stars), ("m.polys", polys), ("m.edges", edges), ("m.display", display)]
}

fn same_mesh(c: &Case, m: &Mesh) -> Verdict {
    same_mesh_as(c, m, "m.")
}

/// As [`same_mesh`] for a dump whose fields are named `<prefix>hdr`, … instead of `m.hdr`.
fn same_mesh_as(c: &Case, m: &Mesh, prefix: &str) -> Verdict {
    for (name, got) in dump(m) {
        let name = &name.replace("m.", prefix);
        // EMUCHECK_MUTATE=1: one bit of every word the port produced, to show the comparison is live
        let got: Vec<u32> = got.iter().map(|w| w ^ world::mutate() as u32).collect();
        let want = c.output(name);
        if want.len() != got.len() || !want.same_u32(&got) {
            let w = want.u32s();
            let first = w.iter().zip(&got).position(|(a, b)| a != b);
            let around = first.map(|i| format!("{:08x?} against {:08x?}", &w[i.saturating_sub(2)..(i + 4).min(w.len())], &got[i.saturating_sub(2)..(i + 4).min(got.len())]));
            let corners: Vec<Vec<i32>> = m.polygons.iter().map(|p| p.nodes.iter().map(|n| n.vertex).collect()).collect();
            let stars: Vec<Vec<(i32, i32, i32)>> = m.vertices.iter().map(|v| v.star.iter().map(|s| (s.vertex, s.edge, s.polygon)).collect()).collect();
            return Verdict::from(false, || {
                format!("{name}: {} words against the port's {}, first difference at {first:?}: {around:?}; flags {:#x}; polygons {corners:?}; polygon flags {:?}; the port's stars {stars:?}",
                        want.len(), got.len(), m.flags, m.polygons.iter().map(|p| p.flag).collect::<Vec<_>>())
            });
        }
    }
    Verdict::from(true, String::new)
}

fn values(f: &Field) -> Vec<u32> {
    f.words().iter().map(|w| w.value().unwrap_or(0) as u32).collect()
}

/// The mesh a `SI_meshUpdateNodes` case starts from.
fn mesh_of(c: &Case) -> Mesh {
    let head = values(c.input("mesh"));
    let mut m = Mesh::default();
    m.angle = f32::from_bits(head[3]);
    m.cos_angle = f32::from_bits(head[4]);
    m.flags = head[8] as u16;
    for v in values(c.input("verts")).chunks(6) {
        m.vertices.push(Vertex { pos: [f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2])], star: Vec::new(), flag: v[5] as i16 });
    }
    for (k, p) in values(c.input("polys")).chunks(8).enumerate() {
        let nodes = values(c.input(&format!("n{k}")));
        let nodes = nodes
            .chunks(9)
            .take(p[4] as usize)
            .map(|n| PolyNode {
                vertex: n[0] as i32,
                edge: n[1] as i32,
                normal: [f32::from_bits(n[2]), f32::from_bits(n[3]), f32::from_bits(n[4])],
                uv: [f32::from_bits(n[5]), f32::from_bits(n[6])],
                color: n[7],
                extra_uv: Vec::new(),
            })
            .collect();
        m.polygons.push(Polygon { normal: [f32::from_bits(p[0]), f32::from_bits(p[1]), f32::from_bits(p[2])], nodes, material: p[5] as i16, flag: p[6] as i16 });
    }
    m
}

fn matrix(f: &Field) -> si_math::types::Matrix {
    let w = f.f32s();
    let mut m = [[0.0f32; 4]; 4];
    for i in 0..16 {
        m[i / 4][i % 4] = w[i];
    }
    m
}

fn mesh_transform(c: &Case) -> Verdict {
    let mut m = mesh_of(c);
    si_model::obj_global::mesh_transform(&mut m, &matrix(c.input("m")), &matrix(c.input("n")));
    same_mesh(c, &m)
}

fn nb_triangles(c: &Case) -> Verdict {
    same_int(c, "eax", si_model::obj_global::mesh_nb_triangles(&mesh_of(c)) as u32)
}

/// `SI_meshCopy` copies `nb_display` display integers; the dump reads one more, which is
/// the copy's zeroed slack: that field is left out of the comparison (a mesh that was never
/// rebuilt has none anyway).
fn mesh_copy(c: &Case) -> Verdict {
    allocate_cosine();
    let m = si_model::obj_global::mesh_copy(&mesh_of(c));
    for (name, got) in dump(&m) {
        if name == "m.display" {
            continue;
        }
        let want = c.output(name);
        if want.len() != got.len() || !want.same_u32(&got) {
            return Verdict::from(false, || format!("{name} differs"));
        }
    }
    Verdict::from(true, String::new)
}

fn calls(c: &Case, name: &str) -> usize {
    c.calls.iter().filter(|k| k.name == name).count()
}

fn update_nodes(c: &Case) -> Verdict {
    let mut m = mesh_of(c);
    let mut env = MeshEnv::default();
    let ok = mesh_b::update_nodes(&mut m, c.input("normals").i16() != 0, &mut env);
    Verdict::from(c.output("ax").u16s() == [ok as u16], || format!("result: port {ok}"))
        .and(|| Verdict::from(env.messages.len() == calls(c, "SI_statusDisplayMessage"), || format!("{} messages", env.messages.len())))
        .and(|| same_mesh(c, &m))
}

/// `SI_meshAllocate` asks the C library for the cosine of 60 degrees; the port of that
/// allocator (`si-types`) has the result as a constant. The recorded call is consumed here
/// so that the tape lines up for the primitive's own calls.
fn allocate_cosine() {
    let _ = si_x87::trig::cos(f64::from_bits(0x3ff0_c152_37db_38a0));
}

fn primitive_result(c: &Case, m: Option<Mesh>) -> Verdict {
    match m {
        Some(m) => same_mesh(c, &m),
        None => Verdict::from(c.output("m.hdr").is_null(), || "the port makes no mesh".to_string()),
    }
}

fn cube(c: &Case) -> Verdict {
    allocate_cosine();
    let f = |n: &str| c.input(n).f32();
    let m = primitive::mesh_cube(f("x1"), f("y1"), f("z1"), f("x0"), f("y0"), f("z0"), &mut MeshEnv::default());
    primitive_result(c, Some(m))
}

fn grid(c: &Case) -> Verdict {
    let (nu, nv) = (c.input("nu").i16(), c.input("nv").i16());
    if nu as i32 * nv as i32 <= primitive::NB_POL_MAX {
        allocate_cosine();
    }
    primitive_result(c, primitive::mesh_grid(nu, nv, c.input("du").f32(), c.input("dv").f32(), primitive::NB_POL_MAX, &mut MeshEnv::default()))
}

fn sphere(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, primitive::mesh_sphere(c.input("r").f32(), c.input("bands").i16(), c.input("meridians").i16(), primitive::NB_POL_MAX, &mut MeshEnv::default()))
}

fn torus(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, primitive::mesh_torus(c.input("r").f32(), c.input("off").f32(), c.input("around").i16(), c.input("turns").i16(), primitive::NB_POL_MAX, &mut MeshEnv::default()))
}

fn cone(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, primitive::mesh_cone(c.input("h").f32(), c.input("r").f32(), c.input("slices").i16(), c.input("base").i16(), c.input("side").i16(), primitive::NB_POL_MAX, &mut MeshEnv::default()))
}

fn cylinder(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, primitive::mesh_cylinder(c.input("h").f32(), c.input("r").f32(), c.input("slices").i16(), c.input("cap").i16(), c.input("side").i16(), primitive::NB_POL_MAX, &mut MeshEnv::default()))
}

fn icosahedron(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, Some(primitive::mesh_icosahedron(c.input("size").f32(), &mut MeshEnv::default())))
}

fn dodecahedron(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, Some(primitive::mesh_dodecahedron(c.input("size").f32(), &mut MeshEnv::default())))
}

fn tetrahedron(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, Some(primitive::mesh_tetrahedron(c.input("size").f32(), &mut MeshEnv::default())))
}

fn octahedron(c: &Case) -> Verdict {
    allocate_cosine();
    primitive_result(c, Some(primitive::mesh_octahedron(c.input("size").f32(), &mut MeshEnv::default())))
}

const WORDS: usize = 0x430 / 4;

fn record(f: &Field) -> Model {
    let w = values(f);
    assert_eq!(w.len(), WORDS);
    // every field of the record is plain data: any bit pattern is a value
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const Model) }
}

fn words(m: &Model) -> Vec<u32> {
    let mut w = vec![0u32; WORDS];
    unsafe { core::ptr::copy_nonoverlapping(m as *const Model as *const u8, w.as_mut_ptr() as *mut u8, 0x430) };
    w
}

/// The words of a record the routine wrote, against the port's; pointers are not compared.
fn same_record(c: &Case, name: &str, got: &Model) -> Verdict {
    let got = words(got);
    for (i, w) in c.output(name).words().iter().enumerate() {
        if let Word::Value { value, .. } = w {
            if *value as u32 != got[i] {
                return Verdict::from(false, || format!("{name}+{:#x}: {:08x} against the port's {:08x}", i * 4, *value as u32, got[i]));
            }
        }
    }
    Verdict::from(true, String::new)
}

/// Counts the calls of the one routine the node update still makes outside the crate.
#[derive(Default)]
struct Counted {
    compensate: usize,
}

impl transform::Hooks for Counted {
    fn compensate_local_coord(&mut self, _: &mut Models, _: NodeId, _: NodeId) {
        self.compensate += 1;
    }
}
impl transform::GeometryHooks for Counted {}
impl si_model::hierarchy::Hooks for Counted {}
impl si_model::node_copy::Hooks for Counted {}

fn son_state(c: &Case) -> Verdict {
    let mut models = Models::default();
    let father = if c.input("father").len() == WORDS {
        let id = models.allocate(None);
        *models.m_mut(id) = record(c.input("father"));
        models.m_mut(id).latdep = Models::ptr(id);
        models.data[id].tx.inv_kin_kind = Some(c.input("ik").u32s()[0] as i32);
        Some(id)
    } else {
        None
    };
    let node = models.allocate(None);
    *models.m_mut(node) = record(c.input("node"));
    models.m_mut(node).latdep = Models::ptr(node);
    models.m_mut(node).father = father.map_or(si_types::Ptr32::NULL, Models::ptr);
    let g = Globals { classical_scaling: c.input("manip").u32() == 1, viewer_upd_opt: c.input("upd").u32(), ..Globals::default() };
    let mut hooks = Counted::default();
    transform::update_son_state(&mut models, father, node, c.input("recurse").i16() != 0, &g, &mut hooks);
    same_record(c, "node", models.m(node))
        .and(|| Verdict::from(hooks.compensate == calls(c, "SI_modelCompensateLocalCoord"), || format!("{} calls of SI_modelCompensateLocalCoord", hooks.compensate)))
        .and(|| Verdict::from(calls(c, "SI_modelReadjustDefinition") == 0, || "the original readjusts".to_string()))
}

fn update_matrix(c: &Case) -> Verdict {
    let mut models = Models::default();
    let node = models.allocate(None);
    *models.m_mut(node) = record(c.input("node"));
    transform::update_matrix(&mut models, node);
    same_record(c, "node", models.m(node))
}

const TREE: usize = 7;

fn node_of(w: &Word) -> Option<NodeId> {
    match w {
        Word::Ref { name, .. } => name.strip_prefix('m').and_then(|k| k.parse().ok()),
        _ => None,
    }
}

/// The seven linked records of a tree case, node k from buffer `m<k>`, named from `s<k>`.
fn tree(c: &Case) -> Models {
    let mut models = Models::default();
    for k in 0..TREE {
        let name = c.input(&format!("s{k}")).to_bytes();
        let id = models.allocate(Some(&name));
        let f = c.input(&format!("m{k}"));
        let mut rec = record(f);
        let link = |i: usize| node_of(&f.words()[i]).map_or(si_types::Ptr32::NULL, Models::ptr);
        rec.father = link(0x2bc / 4);
        rec.brother = link(0x2c0 / 4);
        rec.son = link(0x2c4 / 4);
        *models.m_mut(id) = rec;
    }
    models
}

fn arg(c: &Case, name: &str) -> Option<NodeId> {
    node_of(&c.input(name).words()[0])
}

fn same_ptr(c: &Case, got: Option<NodeId>) -> Verdict {
    let want = node_of(&c.output("ptr").words()[0]);
    Verdict::from(want == got, || format!("the original answers node {want:?}, the port {got:?}"))
}

fn same_int(c: &Case, name: &str, got: u32) -> Verdict {
    let want = c.output(name).words()[0].value().unwrap_or(0) as u32;
    Verdict::from(want == got, || format!("the original answers {want}, the port {got}"))
}

fn next_node(c: &Case) -> Verdict {
    same_ptr(c, si_model::hierarchy::next_node(&tree(c), arg(c, "node").unwrap()))
}

fn root(c: &Case) -> Verdict {
    same_ptr(c, Some(si_model::hierarchy::root(&tree(c), arg(c, "node").unwrap())))
}

fn next_son(c: &Case) -> Verdict {
    same_ptr(c, si_model::hierarchy::next_son(&tree(c), arg(c, "top").unwrap(), arg(c, "at")))
}

fn is_father(c: &Case) -> Verdict {
    same_int(c, "al", si_model::hierarchy::is_father(&tree(c), arg(c, "a").unwrap(), arg(c, "b")) as u32)
}

fn nb_sub_models(c: &Case) -> Verdict {
    same_int(c, "eax", si_model::hierarchy::nb_sub_models(&tree(c), arg(c, "node").unwrap()) as u32)
}

fn model_depth(c: &Case) -> Verdict {
    same_int(c, "eax", si_model::hierarchy::model_depth(&tree(c), arg(c, "node")) as u32)
}

fn common_ancestor(c: &Case) -> Verdict {
    same_ptr(c, si_model::hierarchy::common_ancestor(&tree(c), arg(c, "a"), arg(c, "b")))
}

fn compare_topology(c: &Case) -> Verdict {
    same_int(c, "ax", si_model::hierarchy::compare_topology(&tree(c), arg(c, "a").unwrap(), arg(c, "b").unwrap()) as u32)
}

fn find_by_name(c: &Case) -> Verdict {
    same_ptr(c, si_model::hierarchy::node_find_by_name(&tree(c), arg(c, "node"), &c.input("name").to_bytes()))
}

fn set_state(c: &Case) -> Verdict {
    let mut models = Models::default();
    let node = models.allocate(None);
    *models.m_mut(node) = record(c.input("node"));
    models.m_mut(node).latdep = Models::ptr(node);
    models.m_mut(node).father = si_types::Ptr32::NULL;
    let g = Globals { classical_scaling: c.input("manip").u32() == 1, viewer_upd_opt: c.input("upd").u32(), ..Globals::default() };
    let v = |n: &str| {
        let f = c.input(n).f32s();
        si_types::geom::Vector { x: f[0], y: f[1], z: f[2] }
    };
    let mut hooks = Counted::default();
    transform::set_state(&mut models, node, v("scal"), v("rot"), v("trans"), &g, &mut hooks);
    same_record(c, "node", models.m(node)).and(|| Verdict::from(calls(c, "SI_modelReadjustDefinition") == 0, || "the original readjusts".to_string()))
}

fn multiply_def(c: &Case) -> Verdict {
    let d = values(c.input("def"));
    let k = c.input("keys").f32s();
    let mut sp = si_hrc::Spline { kind: d[3] as i32, ..si_hrc::Spline::default() };
    for q in k.chunks(12) {
        sp.keys.push(si_hrc::SplineKey { prev: [q[0], q[1], q[2]], pos: [q[3], q[4], q[5]], next: [q[6], q[7], q[8]], ..si_hrc::SplineKey::default() });
    }
    let mut def = si_model::Definition::Spline(sp);
    transform::multiply_def(&matrix(c.input("m")), c.input("kind").i32(), &mut def, &mut ());
    let si_model::Definition::Spline(sp) = def else { unreachable!() };
    let mut got = k.clone();
    for (i, key) in sp.keys.iter().enumerate() {
        got[12 * i..12 * i + 3].copy_from_slice(&key.prev);
        got[12 * i + 3..12 * i + 6].copy_from_slice(&key.pos);
        got[12 * i + 6..12 * i + 9].copy_from_slice(&key.next);
    }
    Verdict::from(c.after("keys").same_f32(&got), || "the keys differ".to_string())
}

fn lookup(name: &str) -> Option<Check> {
    if let Some(check) = image::lookup(name) {
        return Some(check);
    }
    Some(match name {
        "SI_meshUpdateNodes" => update_nodes,
        "SI_meshTransform" => mesh_transform,
        "SI_meshNbTriangles" => nb_triangles,
        "SI_meshCopy" => mesh_copy,
        "SI_meshCube" => cube,
        "SI_meshGrid" => grid,
        "SI_meshSphere" => sphere,
        "SI_meshTetrahedron" => tetrahedron,
        "SI_meshOctahedron" => octahedron,
        "SI_meshTorus" => torus,
        "SI_meshCone" => cone,
        "SI_meshCylinder" => cylinder,
        "SI_meshIcosahedron" => icosahedron,
        "SI_meshDodecahedron" => dodecahedron,
        "SI_modelUpdateSonState" => son_state,
        "SI_modelUpdateMatrix" => update_matrix,
        "SI_modelNextNode" => next_node,
        "SI_modelRoot" => root,
        "SI_modelNextSon" => next_son,
        "SI_modelIsFather" => is_father,
        "SI_modelNbSubModels" => nb_sub_models,
        "modelDepth" => model_depth,
        "SI_modelCommonAncestor" => common_ancestor,
        "modelCompareTopology" => compare_topology,
        "SI_modelNodeFindByName" => find_by_name,
        "SI_modelSetState" => set_state,
        "SI_modelMultiplyDef" => multiply_def,
        _ => return support::lookup(name).or_else(|| tri::lookup(name)).or_else(|| facecheck::lookup(name)).or_else(|| life::lookup(name)).or_else(|| tree::lookup(name)).or_else(|| set::lookup(name)),
    })
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: model-emucheck <directory of .vec files>");
        std::process::exit(2);
    };
    match check_dir(Path::new(&dir), &lookup) {
        Ok(rows) => {
            if !print_report(&rows) {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("model-emucheck: {e}");
            std::process::exit(2);
        }
    }
}
