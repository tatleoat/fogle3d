// SPDX-License-Identifier: MIT
//! Deforming objects as Softimage shape animation (unit anim1; our own code, only the file formats are the original's:
//! finding 0005's shape files written through si-elemio's `shape::write_file`, and an animation file with the shape-index
//! curve, written through `anim::ani_file`).
//!
//! The add-on samples a deforming Blender object (armature, shape keys, cloth, soft body, lattice, geometry nodes with a
//! constant topology ...) at the frames the database needs and writes the evaluated vertex positions and corner normals,
//! in Blender's world space, to a side file (layout below). Here every sampled frame becomes one shape (a definition of the
//! model's mesh with that frame's positions and normals), in the layout the corpus's shape scenes have
//! (MOTION-sparkles, AnimResample-Kiss): a SHAPES element per model (`<model>Shp`), relation 200 from the model, and an
//! animation element of chapter SHAPES with one curve, type 8, whose value at a frame is the shape index less one
//! (linear keys, one per sampled frame); relation 15010 ties it to the shape element. The model's shape interpolation
//! (+0x2e0) is 0, linear. The program blends the two shapes around the time (`animShapeUpdate`, ported in si-motion), at
//! every frame and at every step of its motion-blur walk (`si_blurGeometry` calls the scene update at each sub-frame time,
//! and `SI_modelMove` counts a curve of the geometry list as motion), so the deformation is blurred by the program itself.
//!
//! Side file: b"SISHP1", u32 frames, u32 vertices, u32 triangles (all little-endian), i32 frame numbers, then per frame
//! f32 positions (vertices x 3) and f32 corner normals (triangles x 3 x 3), the triangles in the order of the object's
//! `tris` list.
use crate::anim;
use crate::construct::Corner;
use si_elemio::geom::{Edge, Geometry, Mesh, PolyNode, Polygon, Vertex};
use si_elemio::shape::{write_file, ShapeModel};
use si_elemio::Writer;

/// The frames of a deforming object, read from its side file.
pub struct Samples {
    pub frames: Vec<i32>,
    pub nv: usize,
    pub nt: usize,
    /// per frame: positions (nv * 3), corner normals (nt * 9), Blender world space
    pub pos: Vec<Vec<f32>>,
    pub nrm: Vec<Vec<f32>>,
}

pub fn read(path: &str) -> Result<Samples, String> {
    let d = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    if d.len() < 18 || &d[..6] != b"SISHP1" {
        return Err(format!("{path}: not a shape sample file"));
    }
    let u = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as usize;
    let (nf, nv, nt) = (u(6), u(10), u(14));
    // every count is checked against the bytes left before anything is allocated for it
    let per = nv.checked_mul(3).and_then(|a| nt.checked_mul(9).map(|b| (a, b))).ok_or("shape sample counts overflow")?;
    let need = nf.checked_mul(4 + 4 * (per.0 + per.1)).and_then(|n| n.checked_add(18)).ok_or("shape sample counts overflow")?;
    if nf == 0 || nf > 100_000 || d.len() != need {
        return Err(format!("{path}: {} bytes, the counts ({nf} frames, {nv} vertices, {nt} triangles) need {need}", d.len()));
    }
    let f32s = |o: usize, n: usize| -> Vec<f32> { (0..n).map(|i| f32::from_le_bytes([d[o + 4 * i], d[o + 4 * i + 1], d[o + 4 * i + 2], d[o + 4 * i + 3]])).collect() };
    let frames: Vec<i32> = (0..nf).map(|i| u(18 + 4 * i) as i32).collect();
    let mut at = 18 + 4 * nf;
    let (mut pos, mut nrm) = (Vec::with_capacity(nf), Vec::with_capacity(nf));
    for _ in 0..nf {
        pos.push(f32s(at, per.0));
        at += 4 * per.0;
        nrm.push(f32s(at, per.1));
        at += 4 * per.1;
    }
    Ok(Samples { frames, nv, nt, pos, nrm })
}

/// Blender world -> Softimage world (the same rotation as build.rs's `conv`).
fn conv(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = ((v[0] as f64).powi(2) + (v[1] as f64).powi(2) + (v[2] as f64).powi(2)).sqrt();
    if l < 1e-20 {
        [0.0, 1.0, 0.0]
    } else {
        [(v[0] as f64 / l) as f32, (v[1] as f64 / l) as f32, (v[2] as f64 / l) as f32]
    }
}

/// The edges of the polygons, once per vertex pair, in the order construct::mesh_node lists the model's.
pub fn edges_of(polys: &[Vec<Corner>]) -> Vec<(i32, i32)> {
    let mut e = std::collections::BTreeSet::new();
    for p in polys {
        for k in 0..p.len() {
            let (a, b) = (p[k].vertex as i32, p[(k + 1) % p.len()].vertex as i32);
            e.insert((a.min(b), a.max(b)));
        }
    }
    e.into_iter().collect()
}

/// The edges (of `edges`, in that order) whose two sides' corner normals differ, at either end, in any of the given
/// definitions (`normals` per definition: per polygon, per corner) by more than half the renderer's 0.01.
///
/// The renderer makes one render vertex per run of polygons round a vertex between two marked edges, and marks an edge
/// whose corners there differ in normal by more than 0.01 (`SI_meshTriangles`, step 2, ported in si-render's
/// softvertex.rs). A flat-shaded mesh that bends (a subdivided cube's side: coplanar quads, one normal, until they fold)
/// therefore has more render vertices at one time than at another, and the blur walk's movement measure (`maxPsMovement`)
/// answers -1 for two vertex counts that differ: the model is taken as still. An edge marked here gets the user's
/// discontinuity bit (0x08) in every definition, which the renderer marks first (step 1), so the count is that of the
/// union at every time. Half the threshold keeps an edge near it on the marked side (the renderer compares in the x87
/// register); the edge is then hard where its normals were within 0.005..0.01, which shades the same.
pub fn hard_edges(polys: &[Vec<Corner>], normals: &[Vec<Vec<[f32; 3]>>], edges: &[(i32, i32)]) -> Vec<bool> {
    use std::collections::HashMap;
    let index: HashMap<(i32, i32), usize> = edges.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    // per edge: (polygon, corner at the lower end, corner at the higher end)
    let mut sides: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); edges.len()];
    for (pi, p) in polys.iter().enumerate() {
        for k in 0..p.len() {
            let (k2, a, b) = ((k + 1) % p.len(), p[k].vertex as i32, p[(k + 1) % p.len()].vertex as i32);
            if let Some(&e) = index.get(&(a.min(b), a.max(b))) {
                let (lo, hi) = if a <= b { (k, k2) } else { (k2, k) };
                sides[e].push((pi, lo, hi));
            }
        }
    }
    let far = |a: [f32; 3], b: [f32; 3]| {
        let d = [(a[0] - b[0]) as f64, (a[1] - b[1]) as f64, (a[2] - b[2]) as f64];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() > 0.005
    };
    sides
        .iter()
        .map(|s| {
            s.len() > 1
                && normals.iter().any(|nd| {
                    (0..s.len()).any(|i| {
                        (i + 1..s.len()).any(|j| {
                            let (p, q) = (s[i], s[j]);
                            let get = |pi: usize, c: usize| nd.get(pi).and_then(|v| v.get(c)).copied().unwrap_or([0.0; 3]);
                            far(get(p.0, p.1), get(q.0, q.1)) || far(get(p.0, p.2), get(q.0, q.2))
                        })
                    })
                })
        })
        .collect()
}

/// One definition of the model's mesh: the model file's polygons (`polys`, whose corners carry the vertex index and
/// UV), with the vertex positions `verts` and corner normals given (Softimage space), and the edges of `edges`.
pub fn mesh_def(verts: &[[f32; 3]], polys: &[Vec<Corner>], normals: &[Vec<[f32; 3]>], pmat: &[i16], edges: &[(i32, i32)], hard: &[bool]) -> Mesh {
    let mut m = Mesh::default();
    m.flag = 0xc; // what construct::mesh_node gives the model's own definition
    m.nb_vertices = verts.len() as i32;
    m.vertices = verts.iter().map(|p| Vertex { position: *p, flag: 0 }).collect();
    m.nb_polygons = polys.len() as i32;
    m.polygons = polys
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let nodes: Vec<PolyNode> = p
                .iter()
                .enumerate()
                .map(|(k, c)| PolyNode { vertex: c.vertex as i32, normal: normals[i][k], uv: c.uv, ..PolyNode::default() })
                .collect();
            Polygon { normal: [0.0; 3], nb_nodes: nodes.len() as u16, nodes, material: pmat.get(i).copied().unwrap_or(0), flag: 0 }
        })
        .collect();
    m.edges = edges.iter().enumerate().map(|(i, &(a, b))| Edge { vertex: [a, b], flag: crate::construct::EDGE_FLAG | if hard.get(i) == Some(&true) { 0x08 } else { 0 } }).collect();
    m.nb_edges = m.edges.len() as i32;
    m.normals_read = true;
    m.edge_field = true;
    m
}

/// The positions and corner normals (Softimage space) of every sampled frame, for a model whose polygons are `polys`
/// (corner k of polygon i came from the side file's triangle `tri_of[i]`, corner k).
pub fn definitions(s: &Samples, polys: &[Vec<Corner>], tri_of: &[usize]) -> Result<Vec<(Vec<[f32; 3]>, Vec<Vec<[f32; 3]>>)>, String> {
    let mut out = Vec::with_capacity(s.frames.len());
    for (p, n) in s.pos.iter().zip(&s.nrm) {
        let verts: Vec<[f32; 3]> = (0..s.nv).map(|i| conv([p[3 * i], p[3 * i + 1], p[3 * i + 2]])).collect();
        let mut nrm = Vec::with_capacity(polys.len());
        for (i, poly) in polys.iter().enumerate() {
            let t = *tri_of.get(i).ok_or("shape: polygon without a triangle")?;
            if t >= s.nt || poly.len() != 3 || poly.iter().any(|c| c.vertex >= s.nv) {
                return Err(format!("shape: triangle {t} does not fit the samples ({} vertices, {} triangles)", s.nv, s.nt));
            }
            nrm.push((0..3).map(|k| unit(conv([n[9 * t + 3 * k], n[9 * t + 3 * k + 1], n[9 * t + 3 * k + 2]]))).collect::<Vec<_>>());
        }
        out.push((verts, nrm));
    }
    Ok(out)
}

/// The shape file of a mesh model: the model's own definition (`own`, also the current one, as the corpus's files have it)
/// followed by the shapes.
pub fn shape_file(own: Mesh, shapes: Vec<Mesh>) -> Vec<u8> {
    let mut sm = ShapeModel::new(4);
    sm.current = Some(Geometry::Mesh(own.clone()));
    sm.definitions = std::iter::once(own).chain(shapes).map(Geometry::Mesh).collect();
    let mut w = Writer::new();
    write_file(&mut w, &sm);
    w.out
}

/// The animation file of a shape element: curve type 8 (the shape index less one), one linear key per sampled frame.
pub fn shape_anim(elem: &str, frames: &[i32], fps: f64) -> Vec<u8> {
    let vals: Vec<f64> = (0..frames.len()).map(|k| k as f64).collect();
    anim::ani_file("SHAPES", elem, &[anim::Curve { ty: 8, vals }], frames, fps)
}
