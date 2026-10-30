// SPDX-License-Identifier: MIT
//! Checks of the mesh converter on the vectors of `re/tools/si-render/emu/gen_mesh.py`
//! (finding 0012, section 3b): `SI_meshTriangles` on made-up meshes, and the routines of
//! `softtriangle.c` around it.
//!
//! A vector gives the mesh as numbers (the generator reads the emulator's memory after the
//! original's own `SI_meshUpdateNodes` built the stars and edges) and what the original left:
//! every render vertex (19 words), the pairs of the further texture coordinate sets, every
//! triangle record allocated (corners, material, element number, link, as indices), the two
//! optional tables, the edges' flag words afterwards.
use crate::prep::{mutate, same_words};
use si_model::mesh::{Edge, Mesh, PolyNode, Polygon, Star, Vertex};
use si_math::types::Vector;
use si_render::geometry::RenModel;
use si_render::shade::{ShadeMap, ShadeMaterial, ShadeState};
use si_render::geometry::{Geometry, GeometryEnv};
use si_render::softtriangle::{alloc_extra_ren_vertices, alloc_extra_tcoord, collect, model_ctr_bbox, model_geo_bbox, shape_of, NodeDef, NodeSource, Scene, Shape, Tessellators};
use si_render::update::{update, Prepared, UpdateEnv};
use si_render::softvertex::{face2triangle, mesh_triangles, meta_triangles, MatRef, MeshTriangles, MeshTrianglesArgs, MetaSurface, NodeRender, UvDomains, Wild, NO_MATERIAL, NO_UV};
use si_x87::vectors::{Case, Check, Verdict, Word};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cases the port refused because the original leaves its arrays there, by kind.
pub static REFUSED: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
/// Totals over the compared cases: render vertices, triangles kept, triangles taken back.
pub static TOTALS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

fn wild_slot(w: Wild) -> usize {
    match w {
        Wild::StarOverrun => 0,
        Wild::Index(_) => 1,
        Wild::TriangleOverrun => 2,
        Wild::VertexBeforeArray => 3,
        Wild::NullModel => 4,
        Wild::NullMaterials => 5,
        Wild::Count => 6,
        Wild::FatalAlloc => 7,
    }
}

pub fn refused_report() {
    let names = ["star walked past its end", "index outside its array", "more triangles than counted", "corner before the vertex array", "null model read", "null material array read", "negative count", "fatal: nothing to allocate"];
    let n: Vec<String> = names.iter().zip(&REFUSED).filter(|(_, c)| c.load(Ordering::Relaxed) != 0).map(|(n, c)| format!("{} {n}", c.load(Ordering::Relaxed))).collect();
    let t: Vec<usize> = TOTALS.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    println!("mesh converter: {} render vertices, {} triangles, {} triangle records left unused in the compared cases ({} with a table)", t[0], t[1], t[2], t[3]);
    let k: Vec<usize> = COLLECT.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    println!("SI_collect: {} nodes agree: {} render vertices, {} triangles, {} texture coordinate pairs; {} shown as their box, {} with a triangle across a texture seam", k[0], k[1], k[2], k[3], k[4], k[5]);
    let u: Vec<usize> = UPDATED.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    println!("SI_update on scenes of meshes: {} scenes agree ({} triangles filed, {} with a tree); {} frames agree ({} pixels, {} frames with something to shade)", u[0], u[1], u[2], u[3], u[4], u[5]);
    println!("triangle records: {} words left out that the original never set (records from SI_malloc)", UNSET_TN_WORDS.load(Ordering::Relaxed));
    let l: Vec<usize> = LIT.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    let d: Vec<usize> = DOF.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    println!("depth of field (SI_update#dof): {} scenes agree, {} filter weights", d[0], d[1]);
    println!("depth maps (SI_update#lit): {} lights' maps agree, {} cells, {} of them covered", l[0], l[1], l[2]);
    println!("mesh converter: cases the port refuses because the original leaves its arrays: {}", if n.is_empty() { "none".to_string() } else { n.join(", ") });
}

/// The mesh, the second mesh's positions and the header words of a converter vector.
pub struct MeshCase {
    pub mesh: Mesh,
    pub positions: Mesh,
    pub hdr: Vec<u32>,
    pub domains: Option<UvDomains>,
    pub materials: Option<Vec<MatRef>>,
    pub model_maps: Option<usize>,
}

/// The mesh of a vector's `m.` fields.
pub fn mesh_only(c: &Case) -> Mesh {
    mesh_case_with(c, false, "m").mesh
}

pub fn mesh_case(c: &Case) -> MeshCase {
    mesh_case_with(c, true, "m")
}

fn mesh_case_with(c: &Case, full: bool, prefix: &str) -> MeshCase {
    let field = |n: &str| c.output(&format!("{prefix}.{n}")).u32s();
    let hdr = field("hdr");
    let (nv, npol, ne, ex) = (hdr[0] as usize, hdr[1] as usize, hdr[2] as usize, hdr[4] as usize);
    let v = field("verts");
    let s = field("stars");
    let p = field("polys");
    let flags_in = field("eflags_in");
    let e = field("edges");
    let f = f32::from_bits;
    let mut mesh = Mesh::default();
    mesh.flags = hdr[3] as u16;
    let mut at = 0;
    for k in 0..nv {
        let n = s[at] as usize;
        let star = (0..n).map(|i| Star { vertex: s[at + 1 + 3 * i] as i32, edge: s[at + 2 + 3 * i] as i32, polygon: s[at + 3 + 3 * i] as i32 }).collect();
        at += 1 + 3 * n;
        mesh.vertices.push(Vertex { pos: [f(v[3 * k]), f(v[3 * k + 1]), f(v[3 * k + 2])], star, flag: 0 });
    }
    let mut at = 0;
    let per = 9 + 2 * ex;
    for _ in 0..npol {
        let n = p[at + 3] as usize;
        let nodes = (0..n)
            .map(|i| {
                let w = &p[at + 6 + per * i..at + 6 + per * (i + 1)];
                PolyNode {
                    vertex: w[0] as i32,
                    edge: w[1] as i32,
                    normal: [f(w[2]), f(w[3]), f(w[4])],
                    uv: [f(w[5]), f(w[6])],
                    color: w[7],
                    extra_uv: if w[8] != 0 { (0..ex).map(|j| [f(w[9 + 2 * j]), f(w[10 + 2 * j])]).collect() } else { Vec::new() },
                }
            })
            .collect();
        mesh.polygons.push(Polygon { normal: [f(p[at]), f(p[at + 1]), f(p[at + 2])], nodes, material: p[at + 4] as i16, flag: p[at + 5] as i16 });
        at += 6 + per * n;
    }
    mesh.edges.push(Edge::default());
    for k in 0..ne {
        mesh.edges.push(Edge { v0: e[5 * k] as i32, v1: e[5 * k + 1] as i32, p0: e[5 * k + 2] as i32, p1: e[5 * k + 3] as i32, x10: 0, flag: flags_in[k] as i16 });
    }
    if !full {
        return MeshCase { positions: Mesh::default(), mesh, hdr, domains: None, materials: None, model_maps: None };
    }
    let mut positions = mesh.clone();
    if hdr[9] == 0 {
        let v2 = c.output("m.verts2").u32s();
        for k in 0..nv {
            positions.vertices[k].pos = [f(v2[3 * k]), f(v2[3 * k + 1]), f(v2[3 * k + 2])];
        }
    }
    let ids = [3, 4, 5, 6];
    let domains = (hdr[5] != u32::MAX).then(|| UvDomains { count: hdr[5] as i32, sets: c.output("m.doms").u32s().iter().map(|&n| ids[..n as usize].to_vec()).collect() });
    let maps = [None, Some(100usize), Some(200)];
    let materials = (hdr[14] != 0).then(|| c.output("m.mats").u32s().iter().enumerate().map(|(k, &m)| MatRef { id: k as u32, maps: maps[m as usize] }).collect());
    let model_maps = (hdr[13] != 0).then_some(100);
    MeshCase { mesh, positions, hdr, domains, materials, model_maps }
}

/// A node's arrays in the words the generator writes: 19 a vertex, 6 a triangle record.
pub fn node_words(n: &NodeRender) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    node_words_with(n, true)
}

/// As [`node_words`]; with `raw` false a null pointer at +0x40 is written as all ones.
pub fn node_words_with(n: &NodeRender, raw: bool) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let mut verts = Vec::with_capacity(19 * n.verts.len());
    for r in &n.verts {
        verts.extend(r.w00);
        for q in [r.raw, r.raw_normal, r.world] {
            verts.extend([q.x.to_bits(), q.y.to_bits(), q.z.to_bits()]);
        }
        verts.extend([r.ps.x.to_bits(), r.ps.y.to_bits(), r.normal.x.to_bits(), r.normal.y.to_bits(), r.normal.z.to_bits()]);
        verts.extend([if r.uv == NO_UV && raw { 0 } else { r.uv }, r.w44, r.p48]);
    }
    let uv2 = n.uv2.iter().flatten().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]).collect();
    let idx = |o: Option<usize>| o.map_or(u32::MAX, |i| i as u32);
    let mut tns = Vec::with_capacity(6 * n.tns.len());
    for t in &n.tns {
        tns.extend([idx(t.v[0]), idx(t.v[1]), idx(t.v[2]), if t.material == NO_MATERIAL { u32::MAX } else { t.material }, t.w58 as u16 as u32, idx(t.next)]);
    }
    (verts, uv2, tns)
}

fn check_mesh_triangles(c: &Case) -> Verdict {
    let mut mc = mesh_case(c);
    let h = &mc.hdr;
    let args = MeshTrianglesArgs {
        model: mc.domains.as_ref(),
        positions: &mc.positions,
        n_materials: h[6] as i32,
        materials: mc.materials.as_deref(),
        elem_id: h[7] as i32,
        want_uv2: h[10] != 0,
        model_maps: mc.model_maps,
        all_hard: h[8] as i16,
        want_poly_tris: h[11] != 0,
        want_node_verts: h[12] != 0,
        strict: true,
    };
    let got: MeshTriangles = match mesh_triangles(&args, &mut mc.mesh) {
        Ok(Some(g)) => g,
        Ok(None) => {
            // no triangle to make: only the triangle count and array are written
            let unwritten = matches!(c.output("o.nv").words()[0], Word::Unwritten { .. });
            let r = same_words("triangle count", c.output("o.nt"), &[0]).and_then(|_| same_words("triangles allocated", c.output("o.alloc"), &[0]));
            return if !unwritten && !mutate() { Verdict::Different("the original wrote the vertex count".into()) } else { crate::verdict(r) };
        }
        Err(w) => {
            // the original read or wrote outside its arrays here: what it left has no meaning
            REFUSED[wild_slot(w)].fetch_add(1, Ordering::Relaxed);
            return Verdict::Skip;
        }
    };
    let (verts, uv2, tns) = node_words(&got.node);
    let r = (|| {
        same_words("vertex count", c.output("o.nv"), &[got.node.verts.len() as u32])?;
        same_words("triangle count", c.output("o.nt"), &[got.node.tn_count as u32])?;
        same_words("triangles allocated", c.output("o.alloc"), &[got.node.tns.len() as u32])?;
        same_words("vertices", c.output("o.verts"), &verts)?;
        same_words("further pairs", c.output("o.uv2"), &uv2)?;
        same_words("triangles", c.output("o.tns"), &tns)?;
        if let Some(table) = &got.poly_tris {
            let mut w = Vec::new();
            for row in table {
                let row = row.as_deref().unwrap_or(&[]);
                w.push(row.len() as u32);
                w.extend(row.iter().map(|t| t.map_or(u32::MAX, |i| i as u32)));
            }
            same_words("triangles of the polygons", c.output("o.pt"), &w)?;
        }
        if let Some(table) = &got.node_verts {
            let mut w = Vec::new();
            for corner in table.iter().flatten() {
                w.push(corner.len() as u32);
                w.extend(corner.iter().map(|&i| i as u32));
            }
            same_words("vertices of the corners", c.output("o.nrv"), &w)?;
        }
        // the edges' marks are taken off again; every other bit is as it was
        let after = c.output("m.edges").u32s();
        for (k, e) in mc.mesh.edges.iter().enumerate().skip(1) {
            let port = e.flag as u16 as u32 ^ mutate() as u32;
            if after[5 * (k - 1) + 4] != port {
                return Err(format!("edge {k}: flag {:04x}, port {port:04x}", after[5 * (k - 1) + 4]));
            }
        }
        Ok(())
    })();
    if r.is_ok() {
        TOTALS[0].fetch_add(got.node.verts.len(), Ordering::Relaxed);
        TOTALS[1].fetch_add(got.node.tn_count as usize, Ordering::Relaxed);
        TOTALS[2].fetch_add(got.node.tns.len() - got.node.tn_count as usize, Ordering::Relaxed);
        TOTALS[3].fetch_add((got.poly_tris.is_some() || got.node_verts.is_some()) as usize, Ordering::Relaxed);
    }
    crate::verdict(r)
}

/// The stand-ins the generator gives the original for the routines of other source files:
/// the same two functions of their arguments, and a record of the converters asked for.
#[derive(Default)]
struct StandIns {
    calls: Vec<&'static str>,
}

impl Tessellators for StandIns {
    fn patch2triangle(&mut self, _: usize, _: Option<&[MatRef]>, _: i32, _: i16) -> NodeRender {
        self.calls.push("si_patch2triangle");
        NodeRender::default()
    }
    fn nurbs2triangle(&mut self, _: usize, _: Option<&[MatRef]>, _: i32, _: i16) -> NodeRender {
        self.calls.push("SI_nurbs2Triangle");
        NodeRender::default()
    }
    fn face_is_its_shape(&mut self, _: usize) -> bool {
        self.calls.push("SI_face2triangle");
        true
    }
    fn meta_surface(&mut self, _: usize) -> MetaSurface {
        self.calls.push("SI_metaTriangles");
        MetaSurface::default()
    }
    fn txt_projection_info(&mut self, _: usize, _: usize, map: &mut ShadeMap, _: &[si_math::geometry::RVertex], _: i32, bound: &mut [f32; 6], size: &mut Vector, centre: &mut Vector) {
        let id = map.id as f32;
        *bound = core::array::from_fn(|i| id + i as f32);
        *size = Vector::new(id * 0.5, 1.0, 2.0);
        *centre = Vector::new(0.25 * id, -1.0, id);
        map.tmat[0][0] = id;
    }
    fn map_tcoord_compute(&mut self, _: usize, map: &ShadeMap, p: Vector, pair: &mut [f32; 2], bound: &[f32; 6], size: &Vector, centre: &Vector) -> u16 {
        let mut mix = centre.x.to_bits() ^ centre.y.to_bits() ^ centre.z.to_bits() ^ size.x.to_bits() ^ bound[5].to_bits() ^ ((map.method as i32 as u32) << 3);
        for shift in [16, 8, 4, 2] {
            mix ^= mix >> shift;
        }
        let u = f32::from_bits((p.x.abs() * 0.25).to_bits() ^ (mix & 1));
        let v = f32::from_bits((p.y * 0.5).to_bits() ^ ((mix >> 1) & 1));
        *pair = [u, v];
        (u <= 0.25) as u16 | ((u >= 0.75) as u16) << 1
    }
}

fn check_collect(c: &Case) -> Verdict {
    let mesh = mesh_only(c);
    let nv = mesh.vertices.len();
    let m = c.output("c.model").u32s();
    let f = f32::from_bits;
    let (kind, nb_defs, has_org) = (m[8] as i32, m[9] as i16, m[10] != 0);
    // the record that lends positions, as SI_makeVertices picks it
    let pos = c.output("c.pos").u32s();
    let which = match shape_of(kind, has_org, nb_defs) {
        Shape::Org => 0,
        Shape::First => 1,
        Shape::Second => 2,
    };
    let mut positions = mesh.clone();
    for k in 0..nv {
        let at = 3 * (which * nv + k);
        positions.vertices[k].pos = [f(pos[at]), f(pos[at + 1]), f(pos[at + 2])];
    }
    let mut st = ShadeState::default();
    let head = |w: u32| (w != u32::MAX).then_some(w as usize);
    for q in c.output("c.maps").u32s().chunks(25) {
        st.maps.push(ShadeMap {
            method: q[0] as i16,
            id: q[1] as i32,
            v7c: [f(q[2]), f(q[3]), f(q[4])],
            translation: [f(q[5]), f(q[6]), f(q[7])],
            tmat: core::array::from_fn(|i| core::array::from_fn(|j| f(q[8 + 4 * i + j]))),
            next: head(q[24]),
            ..ShadeMap::default()
        });
    }
    for q in c.output("c.mats").u32s().chunks(2) {
        st.materials.push(ShadeMaterial { n_maps: q[0] as i16, maps: head(q[1]), ..ShadeMaterial::default() });
    }
    let ren = RenModel { reversed: m[0] != 0, kind, materials: vec![0, 1, 2], n_maps: m[14] as i16, ..RenModel::default() };
    let old = m[21] != 0;
    // what an earlier frame left: one vertex, two triangle records, pairs
    let before = if old {
        NodeRender { verts: vec![Default::default()], tns: vec![Default::default(); 2], tn_count: 2, uv2: Some(vec![[0.0; 2]; 2]), uv2_stride: 1, tcoords: vec![[0.0; 2]; 2], wild_corners: 0 }
    } else {
        NodeRender::default()
    };
    let mut src = NodeSource {
        box_only: m[1] as i16,
        bbox_min: Vector::new(f(m[2]), f(m[3]), f(m[4])),
        bbox_max: Vector::new(f(m[5]), f(m[6]), f(m[7])),
        nb_defs,
        def: if kind == 4 { NodeDef::Mesh { mesh, positions: Some(positions) } } else { NodeDef::None },
        n_materials: m[11] as i16,
        no_material_array: m[12] == 0,
        maps: head(m[13]),
        visible: m[15] as i16,
        algo_type: m[16] as i32,
        elem_id: m[17] as i32,
        garbage_matte: m[18] as i16,
        uv: UvDomains { count: m[19] as i32, sets: c.output("m.doms").u32s().iter().map(|&n| [3, 4, 5, 6][..n as usize].to_vec()).collect() },
        render: before.clone(),
    };
    let mut env = StandIns::default();
    let stats = match collect(&mut st, &ren, &mut src, 0, m[20] != 0, &mut env) {
        Ok(s) => s,
        Err(w) => {
            REFUSED[wild_slot(w)].fetch_add(1, Ordering::Relaxed);
            return Verdict::Skip;
        }
    };
    if src.render.wild_corners != 0 {
        // the original keeps a triangle whose corner lies before the vertex array
        REFUSED[wild_slot(Wild::VertexBeforeArray)].fetch_add(1, Ordering::Relaxed);
        return Verdict::Skip;
    }
    let out = c.output("c.out");
    if out.words().iter().any(|w| matches!(w, Word::Unwritten { .. })) {
        // the counts themselves depend on memory the original never set (a node shown as
        // its box, whose vertices carry what the allocator left as texture coordinates)
        return Verdict::Skip;
    }
    let r = &src.render;
    let kept = src.algo_type == 13;
    let ran = !kept && r.tn_count != 0;
    let (verts, uv2, tns) = node_words_with(r, false);
    let res = (|| {
        // the converters of the other kinds, in the order they were asked for
        let asked: Vec<&str> = c.calls.iter().map(|k| k.name.as_str()).filter(|n| ["si_patch2triangle", "SI_nurbs2Triangle", "SI_face2triangle", "SI_metaTriangles"].contains(n)).collect();
        if asked != env.calls {
            return Err(format!("converters asked for: original {asked:?}, port {:?}", env.calls));
        }
        if let Some(call) = c.calls.iter().find(|k| ["si_patch2triangle", "SI_nurbs2Triangle", "SI_face2triangle"].contains(&k.name.as_str())) {
            let want = ["meshO", "meshA", "meshB"][which];
            if !matches!(&call.args[0], Word::Ref { name, .. } if name == want) {
                return Err(format!("the shape handed to {}: original {:?}, port {want}", call.name, call.args[0]));
            }
        }
        if kept {
            return same_words("node (left alone)", out, &[old as u32, old as u32, 2 * old as u32, 0, old as u32, 0x7777, old as u32, old as u32]);
        }
        let tc = if ran { [(!r.tcoords.is_empty()) as u32, r.tcoords.len() as u32] } else { [old as u32, 0x7777] };
        same_words("node", out, &[r.verts.len() as u32, (!r.verts.is_empty()) as u32, r.tn_count as u32, r.tns.len() as u32, tc[0], tc[1], r.uv2.is_some() as u32, 0])?;
        // a vertex that no triangle uses any more (a corner replaced by its copy across a
        // seam) can keep a pointer into the pair array's earlier block: no value to compare
        let mut verts = verts;
        let original = c.output("c.verts").written();
        for (i, w) in original.iter().enumerate().skip(16).step_by(19) {
            if *w == Some(0xffff_fffd) {
                let v = i / 19;
                if r.tns[..r.tn_count as usize].iter().any(|t| t.v.contains(&Some(v))) {
                    return Err(format!("vertex {v}: the original's pair pointer is stale, the port's vertex is in use"));
                }
                verts[i] = 0xffff_fffd ^ mutate() as u32;
            }
        }
        same_words("vertices", c.output("c.verts"), &verts)?;
        if ran {
            let pairs: Vec<u32> = r.tcoords.iter().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]).collect();
            same_words("pairs", c.output("c.tc"), &pairs)?;
        }
        same_words("further pairs", c.output("c.uv2"), &uv2)?;
        same_words("triangles", c.output("c.tns"), &tns)?;
        // the sizes the two growth routines last asked for
        if let Some(blocks) = c.try_output("c.blocks") {
            let mut scratch = NodeRender::default();
            let made = r.verts.len() as i32 - stats.vertices;
            if ran && made > 0 {
                scratch.verts = vec![Default::default(); r.verts.len() - 3];
                same_words("vertex block", blocks, &[alloc_extra_ren_vertices(&mut scratch, 3) ^ mutate() as u32, crate::bits(blocks, 1)])?;
            }
            if ran && !r.tcoords.is_empty() {
                let n = r.tcoords.len() as i32;
                same_words("pair block", blocks, &[crate::bits(blocks, 0), alloc_extra_tcoord(&mut scratch, n - 1, 1) ^ mutate() as u32])?;
            }
        }
        let mut want = c.input("g_stats").u32s();
        want[5] = want[5].wrapping_add(stats.vertices as u32);
        want[6] = want[6].wrapping_add(stats.pairs as u32);
        want[7] = want[7].wrapping_add(stats.triangles as u32);
        same_words("SI_stats", c.after("g_stats"), &want)
    })();
    if res.is_ok() {
        COLLECT[0].fetch_add(1, Ordering::Relaxed);
        COLLECT[1].fetch_add(r.verts.len(), Ordering::Relaxed);
        COLLECT[2].fetch_add(r.tn_count.max(0) as usize, Ordering::Relaxed);
        COLLECT[3].fetch_add(if ran { r.tcoords.len() } else { 0 }, Ordering::Relaxed);
        COLLECT[4].fetch_add((src.box_only != 0 && !r.verts.is_empty()) as usize, Ordering::Relaxed);
        COLLECT[5].fetch_add((r.verts.len() as i32 > stats.vertices) as usize, Ordering::Relaxed);
    }
    crate::verdict(res)
}

/// Over the `SI_collect` cases that agree: cases, render vertices, triangles, pairs, nodes
/// shown as their box, nodes with a triangle across a texture seam.
pub static COLLECT: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];

fn check_meta_triangles(c: &Case) -> Verdict {
    let sys = c.input("sys");
    let (nv, nt) = (sys.words()[19].value().unwrap() as usize, sys.words()[21].value().unwrap() as usize);
    let has_tris = matches!(sys.words()[22], Word::Ref { .. });
    let vd = c.input("vd").f64s();
    let idx = c.input("idx").i32s();
    let surface = MetaSurface {
        verts: (0..nv).map(|k| core::array::from_fn(|i| vd[9 * k + i])).collect(),
        tris: has_tris.then(|| (0..nt).map(|k| [idx[3 * k], idx[3 * k + 1], idx[3 * k + 2]]).collect()),
    };
    let first = (!c.input("mats").is_null()).then_some(0);
    let got = match meta_triangles(&surface, first, c.input("elem").i32()) {
        Ok(g) => g,
        Err(w) => {
            REFUSED[wild_slot(w)].fetch_add(1, Ordering::Relaxed);
            return Verdict::Skip;
        }
    };
    let Some(r) = got else {
        // no triangle array: nothing is written
        let untouched = ["o.nv", "o.nt"].iter().all(|n| matches!(c.output(n).words()[0], Word::Unwritten { .. }));
        return Verdict::from(untouched && !mutate(), || "the original wrote a count".into());
    };
    let (verts, _, tns) = node_words(&r);
    crate::verdict(
        same_words("vertex count", c.output("o.nv"), &[r.verts.len() as u32])
            .and_then(|_| same_words("triangle count", c.output("o.nt"), &[r.tn_count as u32]))
            .and_then(|_| same_words("vertices", c.output("o.verts"), &verts))
            .and_then(|_| same_words("triangles", c.output("o.tns"), &tns)),
    )
}

fn check_face2triangle(c: &Case) -> Verdict {
    let mc = mesh_case(c);
    let fc = c.output("f.case").u32s();
    let (same, none) = (fc[0] != 0, fc[1] != 0);
    let mut asked = Vec::new();
    let got = face2triangle(
        same,
        &mut |shape| {
            asked.push(shape);
            if shape {
                Some(mc.positions.clone())
            } else {
                (!none).then(|| mc.mesh.clone())
            }
        },
        mc.materials.as_deref(),
        mc.hdr[7] as i32,
    );
    let r = match got {
        Ok(r) => r,
        Err(w) => {
            REFUSED[wild_slot(w)].fetch_add(1, Ordering::Relaxed);
            return Verdict::Skip;
        }
    };
    // the conversions and disposals the original made
    let made = c.calls.iter().filter(|k| k.name == "SI_faceConvertToMesh").count();
    let freed = c.calls.iter().filter(|k| k.name == "SI_meshDispose").count();
    if made != asked.len() || freed != if none { 0 } else { asked.len() } {
        return Verdict::Different(format!("{made} conversions and {freed} disposals, port asked {asked:?}"));
    }
    let (verts, uv2, tns) = node_words(&r);
    crate::verdict(
        same_words("vertex count", c.output("o.nv"), &[r.verts.len() as u32])
            .and_then(|_| same_words("triangle count", c.output("o.nt"), &[r.tn_count as u32]))
            .and_then(|_| same_words("triangles allocated", c.output("o.alloc"), &[r.tns.len() as u32]))
            .and_then(|_| same_words("vertices", c.output("o.verts"), &verts))
            .and_then(|_| same_words("further pairs", c.output("o.uv2"), &uv2))
            .and_then(|_| same_words("triangles", c.output("o.tns"), &tns)),
    )
}

/// The cases of a file that stop in the original (it reads through a null pointer): the
/// port must refuse each of them.
pub fn stopped_pass(dir: &std::path::Path, file: &str) -> (usize, usize) {
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{file}.vec"))) else { return (0, 0) };
    let (mut stopped, mut refused) = (0, 0);
    for line in text.lines() {
        let Ok(Some(c)) = Case::parse(line) else { continue };
        if !c.notes.iter().any(|n| n.starts_with("STOP:fault")) {
            continue;
        }
        stopped += 1;
        let before: usize = REFUSED.iter().map(|k| k.load(Ordering::Relaxed)).sum();
        let v = std::panic::catch_unwind(|| check_collect(&c));
        let after: usize = REFUSED.iter().map(|k| k.load(Ordering::Relaxed)).sum();
        if matches!(v, Ok(Verdict::Skip)) && after > before {
            refused += 1;
        } else {
            println!("    {file}: the original stops ({}), the port does not refuse", c.notes.join(" "));
        }
    }
    (stopped, refused)
}

/// The scene of an `SI_update#mesh` vector: `SI_collect` runs on the nodes' meshes.
struct MeshScene {
    scene: Scene,
    tess: StandIns,
    /// What the vertices of a node shown as its box hold in the words the original never
    /// sets (the array comes from the allocator): the colour word and the first pair. A
    /// frame is rendered with two values; a pixel that differs between the two has no
    /// value in the original.
    box_fill: u32,
}

impl GeometryEnv for MeshScene {
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize) {
        self.scene.collect(st, g, node, &mut self.tess);
        if self.scene.nodes[node].box_only != 0 && self.box_fill != 0 {
            let m = &g.models[node];
            for v in &mut st.verts[m.verts.unwrap_or(0)..m.verts.unwrap_or(0) + m.vert_count.max(0) as usize] {
                v.w44 = self.box_fill;
                v.w00 = [self.box_fill; 2];
            }
        }
    }
}
impl UpdateEnv for MeshScene {}

fn mesh_scene(c: &Case, unset: f32, frame: bool) -> (Prepared, usize, MeshScene) {
    let (mut p, nmap) = crate::prep::scene_case(c);
    if frame {
        crate::prep::shading_of(c, &mut p, unset);
    }
    // the nodes own no render arrays: the pools start empty
    p.st.tns.clear();
    p.st.verts.clear();
    p.st.uvs.clear();
    // the nodes and their meshes: read from the case loaded into an image (eighth pass)
    let loaded = crate::outer::Loaded::of(c);
    let mut scene = Scene::default();
    for k in 0..3 {
        p.g.models[k].materials = vec![0, 1];
        let mut node = si_render::inimage::node_source_at(&loaded.m, loaded.at(&format!("n{k}")));
        // (these vectors answer SI_face2triangle with a recorder: what a node of kind 2
        // points to is not a face)
        if matches!(node.def, NodeDef::FaceOf { .. }) {
            node.def = NodeDef::Face;
        }
        if matches!(node.def, NodeDef::PatchOf { .. }) {
            node.def = NodeDef::Patch;
        }
        scene.nodes.push(node);
    }
    (p, nmap, MeshScene { scene, tess: StandIns::default(), box_fill: if unset == 0.0 { 0 } else { 0x5a3c_a5c3 } })
}

/// The render vertices and triangles `SI_collect` made inside `SI_update`, as `transForm`,
/// `clip` and `barr` left them, then everything else `SI_update` left.
fn same_mesh_scene(c: &Case, p: &Prepared, nmap: usize, env: &MeshScene) -> Result<(), String> {
    same_scene_of(c, p, nmap, &env.scene)
}

/// [`same_mesh_scene`] given the scene alone.
pub fn same_scene_of(c: &Case, p: &Prepared, nmap: usize, scene: &Scene) -> Result<(), String> {
    struct Of<'a> {
        scene: &'a Scene,
    }
    let env = Of { scene };
    if let Some((node, w)) = env.scene.refused.first() {
        REFUSED[wild_slot(*w)].fetch_add(1, Ordering::Relaxed);
        return Err(format!("SKIP node {node}: {w:?}"));
    }
    if env.scene.nodes.iter().any(|n| n.render.wild_corners != 0) {
        REFUSED[wild_slot(Wild::VertexBeforeArray)].fetch_add(1, Ordering::Relaxed);
        return Err("SKIP a corner before the vertex array".into());
    }
    let order: Vec<usize> = c.output("u.order").u32s().iter().map(|&k| k as usize).collect();
    let mut nodes = Vec::new();
    let mut heap = [0i32; 3];
    for &k in &order {
        let r = &env.scene.nodes[k].render;
        nodes.extend([r.verts.len() as u32, r.tn_count as u32, r.tns.len() as u32, r.tcoords.len() as u32]);
        if r.tn_count != 0 {
            heap = [heap[0] + r.verts.len() as i32, heap[1] + r.tcoords.len() as i32, heap[2] + r.tn_count];
        }
    }
    same_words("the nodes' counts", c.output("u.nodes"), &nodes)?;
    // where each node's arrays lie in the pools (the clipper's vertices and triangles lie
    // between them; those are compared through the scan-line table)
    let (mut vr, mut tr, mut ur) = (Vec::new(), Vec::new(), Vec::new());
    let (mut vb, mut tb, mut ub) = (0, 0, 0);
    for &k in &order {
        let (m, r) = (&p.g.models[k], &env.scene.nodes[k].render);
        vr.push((m.verts.unwrap_or(0), r.verts.len(), vb));
        tr.push((m.tns.unwrap_or(0), r.tns.len(), tb));
        ur.push((m.tcoords.unwrap_or(0), r.tcoords.len(), ub));
        (vb, tb, ub) = (vb + r.verts.len(), tb + r.tns.len(), ub + r.tcoords.len());
    }
    let numbered = |ranges: &Vec<(usize, usize, usize)>, i: usize| ranges.iter().find(|r| r.0 <= i && i < r.0 + r.1).map_or(u32::MAX, |r| (r.2 + i - r.0) as u32);
    let mut mv = Vec::new();
    for &(start, n, _) in &vr {
        for v in &p.st.verts[start..start + n] {
            mv.extend(v.w00);
            for q in [v.raw, v.raw_normal, v.world] {
                mv.extend([q.x.to_bits(), q.y.to_bits(), q.z.to_bits()]);
            }
            let pair = if v.uv == NO_UV { u32::MAX } else { numbered(&ur, v.uv as usize) };
            // (+0x48: the vertex's further pairs, by their number in the node's array, where
            // the vector gives those arrays: ninth pass)
            let further = if c.try_output("uv2").is_some() { v.p48 } else { 0 };
            mv.extend([v.ps.x.to_bits(), v.ps.y.to_bits(), v.normal.x.to_bits(), v.normal.y.to_bits(), v.normal.z.to_bits(), pair, v.w44, further]);
        }
    }
    // a vertex that no triangle uses any more (a corner replaced by its copy across a seam)
    // can keep a pointer into the pair array's earlier block: no value to compare
    let stale: Vec<usize> = c.output("mv").written().iter().enumerate().skip(16).step_by(19).filter(|(_, w)| **w == Some(0xffff_fffd)).map(|(i, _)| i).collect();
    if !stale.is_empty() {
        let mut k = 0;
        for &(start, n, _) in &vr {
            for i in start..start + n {
                if stale.contains(&(19 * k + 16)) {
                    if tr.iter().zip(&order).any(|(t, &node)| p.st.tns[t.0..t.0 + env.scene.nodes[node].render.tn_count.max(0) as usize].iter().any(|t| t.v.contains(&Some(i)))) {
                        return Err(format!("vertex {k}: the original's pair pointer is stale, the port's vertex is in use"));
                    }
                    mv[19 * k + 16] = 0xffff_fffd ^ mutate() as u32;
                }
                k += 1;
            }
        }
    }
    same_words("render vertices", c.output("mv"), &mv)?;
    if let Some(original) = c.try_output("uv2") {
        let mut uv2 = Vec::new();
        for &k in &order {
            let pairs = env.scene.nodes[k].render.uv2.as_deref().unwrap_or(&[]);
            uv2.push(2 * pairs.len() as u32);
            uv2.extend(pairs.iter().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]));
        }
        same_words("further pairs", original, &uv2)?;
    }
    let idx = |o: Option<usize>| o.map_or(u32::MAX, |i| numbered(&vr, i));
    let original = c.output("mt").written();
    let mut k = 0;
    for &(start, n, _) in &tr {
        for t in &p.st.tns[start..start + n] {
            let mut port = vec![idx(t.v[0]), idx(t.v[1]), idx(t.v[2]), t.n.x.to_bits(), t.n.y.to_bits(), t.n.z.to_bits(), t.d.to_bits(), t.inv_d.to_bits(), t.axis as u16 as u32, 0, t.inv_n.to_bits()];
            for e in t.edge {
                port.extend([e.x, e.y, e.z].map(f32::to_bits));
            }
            let next = t.next.map_or(u32::MAX, |i| numbered(&tr, i));
            port.extend([if t.material == NO_MATERIAL { u32::MAX } else { t.material }, t.flags as u32, t.flags2 as u32, 0, t.w58 as u16 as u32, 0, next]);
            for (i, q) in port.iter().enumerate() {
                // +0x22, +0x54 and +0x5a are not members the port keeps
                if [9, 23, 25].contains(&i) {
                    continue;
                }
                let q = *q ^ mutate() as u32;
                if original.get(27 * k + i) == Some(&None) {
                    // a word the original never set: a patch's triangle records come from
                    // SI_malloc, and transForm leaves a member alone for some triangles
                    // (here the record starts as zeros)
                    UNSET_TN_WORDS.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                if original.get(27 * k + i).copied().flatten() != Some(q as u128) {
                    return Err(format!("triangle record {k} word {i}: original {:?}, port {q:08x}", original.get(27 * k + i)));
                }
            }
            k += 1;
        }
    }
    if original.len() != 27 * k {
        return Err(format!("triangle records: original {}, port {k}", original.len() / 27));
    }
    // (the vertices a seam adds are in the array and not in the counter)
    let _ = heap;
    let heap = env.scene.counted;
    crate::prep::set_pool(tr, vr);
    let r = crate::prep::same_rest(c, p, nmap, heap);
    crate::prep::reset_pool();
    r
}

pub fn skip_or(r: Result<(), String>) -> Verdict {
    match r {
        Err(e) if e.starts_with("SKIP") => Verdict::Skip,
        other => crate::verdict(other),
    }
}

fn check_update_mesh(c: &Case) -> Verdict {
    si_x87::trig::replay::start(c.tape());
    let (mut p, nmap, mut env) = mesh_scene(c, 0.0, false);
    let got = update(&mut p, &mut env, c.input("time").f32());
    if c.calls.iter().any(|k| k.name == "si_fatal") {
        si_x87::trig::replay::start(Vec::new());
        let text = String::from_utf8_lossy(&c.output("str0").to_bytes()).into_owned();
        return Verdict::from(got == Err(text.as_str()), || format!("fatal: original {text:?}, port {got:?}"));
    }
    if let Err(e) = got {
        return Verdict::Different(format!("the port stops with {e:?}, the original does not"));
    }
    let r = same_mesh_scene(c, &p, nmap, &env);
    if r.is_ok() {
        UPDATED[0].fetch_add(1, Ordering::Relaxed);
        UPDATED[1].fetch_add(p.g.bins.accepted.max(0) as usize, Ordering::Relaxed);
        UPDATED[2].fetch_add(!p.bsp.nodes.is_empty() as usize, Ordering::Relaxed);
    }
    skip_or(r)
}

fn check_update_meshframe(c: &Case) -> Verdict {
    let run = |unset: f32| {
        si_x87::trig::replay::start(c.tape());
        let (mut p, nmap, mut env) = mesh_scene(c, unset, true);
        let got = update(&mut p, &mut env, c.input("time").f32());
        (p, nmap, env, got)
    };
    let (p, nmap, env, got) = run(0.0);
    if let Err(e) = got {
        return Verdict::Different(format!("the port stops with {e:?}"));
    }
    let r = same_mesh_scene(c, &p, nmap, &env).and_then(|_| {
        let pixels = c.output("frame").len() / 7;
        crate::prep::same_frame(c, p, run(4321.5).0)?;
        UPDATED[3].fetch_add(1, Ordering::Relaxed);
        UPDATED[4].fetch_add(pixels, Ordering::Relaxed);
        UPDATED[5].fetch_add((pixels != 0) as usize, Ordering::Relaxed);
        Ok(())
    });
    skip_or(r)
}

/// `SI_update#lit` (eighth pass): the scenes of `SI_update#meshframe` with `si_lights`
/// running in the original, and behind it `shadowBuffer`, the traversal seen from the
/// light, `si_setBounds` and `zSample`. The port's `si_lights` is given the binding of
/// `si_render::bound` for the depth-map pass.
struct LitScene {
    inner: MeshScene,
    notes: Vec<String>,
    rng: si_render::jitter::Drand48,
}

impl GeometryEnv for LitScene {
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize) {
        self.inner.collect(st, g, node);
    }
}

impl UpdateEnv for LitScene {
    fn lights(&mut self, p: &mut Prepared) {
        let mut tess = StandIns::default();
        let mut pass = si_render::bound::ShadowPass {
            scene: &self.inner.scene,
            g: &p.g,
            settings: p.settings,
            options: p.options.clone(),
            camera: p.camera,
            root: p.root,
            buffers: p.buffers,
            empty_bounds: p.empty_bounds,
            depth_cells: &mut p.stats[si_render::update::stat::SHADOW_DEPTH],
            notes: &mut self.notes,
            rng: &mut self.rng,
            tess: &mut tess,
        };
        let r = si_render::lights::lights(&mut p.st, &mut p.options.raytrace, p.settings.level, &mut pass);
        p.g.raytrace = p.options.raytrace & 1 != 0;
        p.st.glb.raytrace = p.g.raytrace;
        if let Err(e) = r {
            self.notes.push(format!("fatal: {e}"));
        }
    }
}

/// Depth maps of lights that agree: lights with a map, cells, cells something covers.
pub static LIT: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

/// What `si_lights` left of the lights of the chain: every word of a record the port has a
/// member for (the first 97 words but the map's row pointer), and the map's cells.
fn same_lights(c: &Case, st: &ShadeState) -> Result<[usize; 3], String> {
    let mut counts = [0usize; 3];
    let mut cur = st.light_head;
    while let Some(k) = cur {
        let name = ["l0", "l1"][k];
        let (before, after, l) = (c.input(name), c.after(name), &st.lights[k]);
        let mut port: Vec<u32> = (0..97).map(|i| crate::bits(before, i)).collect();
        for (i, v) in [l.pos.x, l.pos.y, l.pos.z, l.dir.x, l.dir.y, l.dir.z].iter().enumerate() {
            port[17 + i] = v.to_bits();
        }
        if l.kind == 2 {
            for (i, v) in [l.spot_dir.x, l.spot_dir.y, l.spot_dir.z].iter().enumerate() {
                port[26 + i] = v.to_bits();
            }
            (port[31], port[32], port[33]) = (l.cone_cos.to_bits(), l.spread_cos.to_bits(), l.cone_div.to_bits());
        }
        let mapped = l.shadow == 3 || l.shadow == 4;
        if mapped {
            for (i, v) in l.depth_mat.iter().flatten().chain(l.xy_mat.iter().flatten()).enumerate() {
                port[39 + i] = v.to_bits();
            }
            (port[74], port[77]) = (l.soft_size as u32, l.soft_scale.to_bits());
        }
        port[81] = (port[81] & 0xffff_0000) | l.falloff as u32;
        for i in 0..97 {
            if i == 71 {
                continue;
            }
            let q = port[i] ^ mutate() as u32;
            if crate::bits(after, i) != q {
                return Err(format!("{name} word {i}: original {:08x}, port {q:08x}", crate::bits(after, i)));
            }
        }
        let cells: Vec<u32> = if mapped { l.zmap.iter().flatten().map(|v| v.to_bits()).collect() } else { Vec::new() };
        same_words(&format!("the depth map of {name}"), c.output(&format!("zmap{k}")), &cells)?;
        if mapped {
            counts[0] += 1;
            counts[1] += cells.len();
            counts[2] += cells.iter().filter(|v| **v != (-100000.0f32).to_bits()).count();
        }
        cur = l.next;
    }
    Ok(counts)
}

fn check_update_lit(c: &Case) -> Verdict {
    if c.calls.iter().any(|k| k.name == "si_fatal") {
        // (the recorder lets the original run on where the render would have ended)
        return Verdict::Skip;
    }
    // `fill`: what a box node's unset vertex words hold (None: mesh_scene's own choice)
    let run = |unset: f32, fill: Option<u32>| {
        si_x87::trig::replay::start(c.tape());
        let (mut p, nmap, mut inner) = mesh_scene(c, unset, true);
        if let Some(fill) = fill {
            inner.box_fill = fill;
        }
        let mut env = LitScene { inner, notes: Vec::new(), rng: si_render::jitter::Drand48::seeded(0) };
        let got = update(&mut p, &mut env, c.input("time").f32());
        (p, nmap, env, got)
    };
    let (p, nmap, env, got) = run(0.0, None);
    if let Err(e) = got {
        return Verdict::Different(format!("the port stops with {e:?}"));
    }
    if let Some(n) = env.notes.iter().find(|n| n.starts_with("fatal") || n.contains("faulted")) {
        return Verdict::Different(format!("the port: {n}"));
    }
    crate::prep::set_extra_chunk(c.output("zbytes").u32() as i32);
    let r = same_mesh_scene(c, &p, nmap, &env.inner);
    crate::prep::set_extra_chunk(0);
    let r = r.and_then(|_| same_lights(c, &p.st)).and_then(|counts| {
        let pixels = c.output("frame").len() / 7;
        let others = [None, Some(0xffff_ffff), Some(0x8040_2010), Some(0x0000_00ff)].into_iter().map(|fill| run(4321.5, fill).0).collect();
        crate::prep::same_frame_many(c, p, others, true)?;
        for (slot, n) in LIT.iter().zip(counts) {
            slot.fetch_add(n, Ordering::Relaxed);
        }
        UPDATED[3].fetch_add(1, Ordering::Relaxed);
        UPDATED[4].fetch_add(pixels, Ordering::Relaxed);
        UPDATED[5].fetch_add((pixels != 0) as usize, Ordering::Relaxed);
        Ok(())
    });
    skip_or(r)
}

/// `SI_update#dof` (eighth pass): the scenes of `SI_update#mesh` with a camera that has
/// depth of field and `si_initDof` running in the original. The port's `SI_update` is given
/// the binding of `si_render::bound`.
struct DofScene {
    inner: MeshScene,
    notes: Vec<String>,
}

impl GeometryEnv for DofScene {
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize) {
        self.inner.collect(st, g, node);
    }
}

impl UpdateEnv for DofScene {
    fn init_dof(&mut self, p: &mut Prepared) {
        si_render::bound::init_dof(p, &mut self.notes);
    }
}

/// Scenes with depth of field that agree: cases, filter cells compared.
pub static DOF: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];

fn check_update_dof(c: &Case) -> Verdict {
    si_x87::trig::replay::start(c.tape());
    let (mut p, nmap, inner) = mesh_scene(c, 0.0, false);
    let cam = c.input("g_cam");
    let f = |i: usize| crate::fl(cam, i);
    (p.camera.dof_focal, p.camera.dof_focus, p.camera.dof_near, p.camera.dof_far) = (f(29), f(30), f(32), f(34));
    (p.camera.dof_beyond, p.camera.dof_aperture, p.camera.dof_circle) = (f(36), f(38), f(39));
    p.options.default_width = c.input("g_defw").i16();
    let mut env = DofScene { inner, notes: Vec::new() };
    let got = update(&mut p, &mut env, c.input("time").f32());
    if let Some(k) = c.calls.iter().find(|k| k.name == "si_fatal") {
        si_x87::trig::replay::start(Vec::new());
        // (the fatal text is the call's own first argument: a status line may come before it)
        let text = match k.args.first() {
            Some(Word::Ref { name, .. }) => String::from_utf8_lossy(&c.output(name).to_bytes()).into_owned(),
            _ => String::new(),
        };
        return Verdict::from(got == Err(text.as_str()), || format!("fatal: original {text:?}, port {got:?}"));
    }
    if let Err(e) = got {
        return Verdict::Different(format!("the port stops with {e:?}, the original does not"));
    }
    crate::prep::set_extra_chunk(c.output("dof_bytes").u32() as i32);
    let r = same_mesh_scene(c, &p, nmap, &env.inner);
    crate::prep::set_extra_chunk(0);
    let r = r.and_then(|_| {
        same_words("blur lines", c.after("g_dofn"), &[p.view.dof_lines as u32, p.view.dof_half as u32])?;
        same_words("limit", c.output("g_dlim"), &[p.dof.limit.to_bits()])?;
        same_words("default radius", c.output("g_drad"), &[p.dof.radius as u32])?;
        let after = c.after("g_cam");
        for (i, v) in [(33, p.st.glb.dof_z0), (35, p.st.glb.dof_z1), (37, p.st.glb.dof_z2)] {
            let q = v.to_bits() ^ mutate() as u32;
            if crate::bits(after, i) != q {
                return Err(format!("camera word {i}: original {:08x}, port {q:08x}", crate::bits(after, i)));
            }
        }
        let cells: Vec<u32> = p.dof.filters.iter().flatten().flatten().map(|v| v.to_bits()).collect();
        same_words("filter tables", c.output("dof_filters"), &cells)?;
        let said = c.calls.iter().filter(|k| k.name == "SI_statusDisplayMessage").count();
        if said != env.notes.len() {
            return Err(format!("status lines: original {said}, port {:?}", env.notes));
        }
        DOF[0].fetch_add(1, Ordering::Relaxed);
        DOF[1].fetch_add(cells.len(), Ordering::Relaxed);
        Ok(())
    });
    skip_or(r)
}

/// Words of triangle records left out of the comparison because the original never set
/// them.
pub static UNSET_TN_WORDS: AtomicUsize = AtomicUsize::new(0);

/// Scenes of meshes that agree: `SI_update` cases, triangles filed in them, with a tree;
/// frames, pixels compared, frames with something to shade.
pub static UPDATED: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_meshTriangles" => check_mesh_triangles,
        "SI_update#mesh" => check_update_mesh,
        "SI_update#meshframe" => check_update_meshframe,
        "SI_update#lit" => check_update_lit,
        "SI_update#dof" => check_update_dof,
        "SI_modelCtrBbox" => |c: &Case| {
            let (a, b) = (c.input("a").f32s(), c.input("b").f32s());
            let v = model_ctr_bbox(Vector::new(a[0], a[1], a[2]), Vector::new(b[0], b[1], b[2]));
            crate::verdict(same_words("centre", c.output("ret"), &[v.x, v.y, v.z].map(f32::to_bits)))
        },
        "SI_modelGeoBbox" => |c: &Case| {
            let (a, b) = (c.input("a").f32s(), c.input("b").f32s());
            let v = model_geo_bbox(c.input("method").i16(), Vector::new(a[0], a[1], a[2]), Vector::new(b[0], b[1], b[2]), c.input("r").f32(), c.input("s").f32());
            crate::verdict(same_words("half extents", c.output("ret"), &[v.x, v.y, v.z].map(f32::to_bits)))
        },
        "SI_metaTriangles" => check_meta_triangles,
        "SI_face2triangle" => check_face2triangle,
        "SI_collect" => check_collect,
        _ => return None,
    })
}
