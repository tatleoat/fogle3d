// SPDX-License-Identifier: MIT
//! Corpus check of the renderer's mesh converter (finding 0012, section 3b): every sample
//! model under a directory, each mesh node loaded as the program loads it (`si-model`) and
//! sent through `SI_collect` as the renderer would: `SI_makeVertices`, `SI_meshTriangles`,
//! `SI_textureVerts`.
//!
//!   render-check <directory>
//!
//! What the check supplies that a render would have set up elsewhere: a node without a
//! material gets one plain material; the texture projections are the ported
//! `SI_txtProjectionInfo` and `SI_mapTcoordCompute`; a node is its own shape (the files
//! hold one definition a node in all but a few cases, and the second is then lent to the
//! first as its shape). Kinds that need a tessellator of another source file are counted.
//!
//! Then (module `surfaces`, finding 0012 section 13): every material and every 2D and 3D
//! texture of the model, material and texture files goes through the renderer's set-up
//! (`SI_materials`, `SI_textures` with the pictures found on the discs, `SI_updTxt3dTurb`)
//! and its texture routines are run on it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{read_file, Content, Definition, Kind};
use si_model::load::{finish_read, mesh_from_file};
use si_model::mesh_b::MeshEnv;
use si_render::geometry::RenModel;
use si_render::shade::{ShadeMap, ShadeMaterial, ShadeState};
use si_render::softtriangle::{collect, NodeDef, NodeSource, Tessellators};
use si_render::softvertex::{UvDomains, Wild, NO_UV};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|e| e == "hrc") {
            out.push(p);
        }
    }
}

struct NoTextures;
impl Tessellators for NoTextures {}

/// A list of 2D maps appended to the state; returns its head.
fn push_maps(st: &mut ShadeState, textures: &[si_hrc::Texture2D]) -> Option<usize> {
    let first = st.maps.len();
    for (i, t) in textures.iter().enumerate() {
        st.maps.push(ShadeMap {
            method: t.method,
            translation: t.translation,
            next: (i + 1 < textures.len()).then_some(first + i + 1),
            ..ShadeMap::default()
        });
    }
    (!textures.is_empty()).then_some(first)
}

#[derive(Default)]
struct Totals {
    files: u64,
    unread: u64,
    nodes: u64,
    by_kind: BTreeMap<i32, u64>,
    mesh_nodes: u64,
    meshes: u64,
    polygons: u64,
    hidden: u64,
    corners: u64,
    vertices: u64,
    triangles: u64,
    records: u64,
    pairs: u64,
    seam_vertices: u64,
    with_uv2: u64,
    untextured: u64,
    default_material: u64,
    empty: u64,
    refused: BTreeMap<String, (u64, String)>,
    panics: u64,
    shared: u64,
    mesh_vertices: u64,
    wild: (u64, u64),
    wild_example: String,
}

/// Faces and patches through their converters (ninth pass).
#[derive(Default)]
struct Others {
    faces: u64,
    nurbs_faces: u64,
    face_made: u64,
    face_triangles: u64,
    face_refused: u64,
    patches: u64,
    patch_made: u64,
    patch_triangles: u64,
    patch_vertices: u64,
    patch_refused: u64,
    panics: u64,
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: render-check <directory>");
        std::process::exit(2);
    };
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let mut t = Totals::default();
    let mut o = Others::default();
    let mut env = MeshEnv::default();
    for path in &files {
        let Ok(data) = std::fs::read(path) else { continue };
        t.files += 1;
        let rel = path.strip_prefix(&dir).unwrap_or(path).display().to_string();
        let Some(f) = read_file(&data, Kind::Model) else {
            t.unread += 1;
            continue;
        };
        let Content::Model(root) = &f.content else {
            t.unread += 1;
            continue;
        };
        let mut nodes = Vec::new();
        root.walk(0, &mut |_, n| nodes.push(n));
        for (elem, n) in nodes.into_iter().enumerate() {
            t.nodes += 1;
            *t.by_kind.entry(n.kind).or_default() += 1;
            // faces and patches (ninth pass): the node's last definition through the
            // converter, with one material
            let one = [si_render::softvertex::MatRef { id: 0, maps: None }];
            if let (2, Some(Definition::Face(face))) = (n.kind, n.definitions.last()) {
                o.faces += 1;
                o.nurbs_faces += (face.is_spline == 0) as u64;
                let face = face.clone();
                match std::panic::catch_unwind(move || si_render::softvertex::face2triangle(true, &mut |_| si_render::facemesh::face_to_mesh(&face), Some(&one), elem as i32)) {
                    Err(_) => o.panics += 1,
                    Ok(Err(_)) => o.face_refused += 1,
                    Ok(Ok(r)) => {
                        o.face_made += (r.tn_count != 0) as u64;
                        o.face_triangles += r.tn_count.max(0) as u64;
                    }
                }
            }
            if let (1, Some(Definition::Patch(patch))) = (n.kind, n.definitions.last()) {
                o.patches += 1;
                let mut surface = si_render::fromfile::surface_from_file(patch);
                match std::panic::catch_unwind(move || si_render::softvertex::patch2triangle(None, &mut surface, Some(&one), elem as i32, -1)) {
                    Err(_) => o.panics += 1,
                    Ok(Err(_)) => o.patch_refused += 1,
                    Ok(Ok(r)) => {
                        o.patch_made += (r.tn_count != 0) as u64;
                        o.patch_triangles += r.tn_count.max(0) as u64;
                        o.patch_vertices += r.verts.len() as u64;
                    }
                }
            }
            let meshes: Vec<&si_hrc::Mesh> = n.definitions.iter().filter_map(|d| if let Definition::Mesh(m) = d { Some(m) } else { None }).collect();
            if n.kind != 4 || meshes.is_empty() {
                continue;
            }
            t.mesh_nodes += 1;
            // the materials and maps of the node
            let mut st = ShadeState::default();
            for m in &n.materials {
                let textures: &[si_hrc::Texture2D] = m.as_ref().map_or(&[], |m| &m.textures);
                let maps = push_maps(&mut st, textures);
                st.materials.push(ShadeMaterial { n_maps: textures.len() as i16, maps, ..ShadeMaterial::default() });
            }
            if st.materials.is_empty() {
                st.materials.push(ShadeMaterial::default());
                t.default_material += 1;
            }
            let own = push_maps(&mut st, &n.textures);
            let ren = RenModel { kind: 4, materials: (0..st.materials.len()).collect(), n_maps: n.textures.len() as i16, ..RenModel::default() };
            for fm in meshes {
                t.meshes += 1;
                let mut mesh = mesh_from_file(fm);
                env.messages.clear();
                finish_read(&mut mesh, &fm.edges, fm.compute_normals, Some(&n.name), &mut env);
                t.polygons += mesh.polygons.len() as u64;
                t.hidden += mesh.polygons.iter().filter(|p| p.flag & 0x800 != 0).count() as u64;
                t.corners += mesh.polygons.iter().map(|p| p.nodes.iter().filter(|c| c.vertex != -1).count() as u64).sum::<u64>();
                t.mesh_vertices += mesh.vertices.len() as u64;
                let mut src = NodeSource {
                    def: NodeDef::Mesh { mesh, positions: None },
                    nb_defs: 1,
                    n_materials: st.materials.len() as i16,
                    maps: own,
                    visible: 1,
                    elem_id: elem as i32,
                    uv: UvDomains { count: n.uv_sets, sets: n.uv_domains.clone().unwrap_or_default() },
                    ..NodeSource::default()
                };
                let mut st2 = st.clone();
                let ren2 = ren.clone();
                let result = std::panic::catch_unwind(move || {
                    let r = collect(&mut st2, &ren2, &mut src, 0, true, &mut NoTextures);
                    (r, src)
                });
                match result {
                    Err(_) => t.panics += 1,
                    Ok((Err(w), _)) => {
                        let key = match w {
                            Wild::Index(what) => format!("an index outside its array ({what})"),
                            other => format!("{other:?}"),
                        };
                        let e = t.refused.entry(key).or_insert((0, format!("{rel}: {}", String::from_utf8_lossy(&n.name))));
                        e.0 += 1;
                    }
                    Ok((Ok(stats), src)) => {
                        let r = &src.render;
                        t.vertices += r.verts.len() as u64;
                        t.triangles += r.tn_count as u64;
                        t.records += r.tns.len() as u64;
                        t.pairs += stats.pairs as u64;
                        t.seam_vertices += (r.verts.len() as i32 - stats.vertices) as u64;
                        t.with_uv2 += r.uv2.is_some() as u64;
                        t.untextured += r.verts.iter().all(|v| v.uv == NO_UV) as u64;
                        t.empty += (r.tn_count == 0) as u64;
                        if r.wild_corners != 0 {
                            t.wild.0 += 1;
                            t.wild.1 += r.wild_corners as u64;
                            if t.wild_example.is_empty() {
                                t.wild_example = format!("{rel}: {}", String::from_utf8_lossy(&n.name));
                            }
                        }
                        // every triangle's corners must be vertices of the node
                        assert!(r.tns[..r.tn_count as usize].iter().all(|x| x.v.iter().all(|c| c.is_some_and(|i| i < r.verts.len()))));
                        if let NodeDef::Mesh { mesh, .. } = &src.def {
                            t.shared += (r.verts.len() == mesh.vertices.iter().filter(|v| !v.star.is_empty()).count()) as u64;
                        }
                    }
                }
            }
        }
    }
    println!("model files {}  not read {}  nodes {}", t.files, t.unread, t.nodes);
    let name = |k: i32| match k {
        0 => "null",
        1 => "patch: si_patch2triangle (Tessellators::patch2triangle)",
        2 => "face: SI_face2triangle (ported; SI_faceConvertToMesh through Tessellators::face_to_mesh)",
        4 => "mesh: SI_meshTriangles (ported)",
        5 => "joint",
        6 => "spline",
        7 => "metaball system: SI_metaTriangles (ported; SI_metaSysGetSurface through Tessellators::meta_surface)",
        8 => "metaball",
        9 => "NURBS curve",
        10 => "NURBS surface: SI_nurbs2Triangle (Tessellators::nurbs2triangle)",
        _ => "other",
    };
    println!("nodes by kind (what SI_makeVertices calls for it):");
    for (k, c) in &t.by_kind {
        println!("  {k:>3} {c:>7}  {}", name(*k));
    }
    println!("\nmesh nodes {}  meshes (definitions) {}  mesh vertices {}  polygons {} (hidden {})  corners {}", t.mesh_nodes, t.meshes, t.mesh_vertices, t.polygons, t.hidden, t.corners);
    println!("converted: {} meshes; render vertices {}  triangles {}  triangle records allocated {}", t.meshes - t.panics - t.refused.values().map(|v| v.0).sum::<u64>(), t.vertices, t.triangles, t.records);
    println!("  texture coordinate pairs {}  vertices added across texture seams {}", t.pairs, t.seam_vertices);
    println!("  meshes with further texture coordinate sets {}  meshes whose vertices carry no pair {}  meshes that give no triangle {}", t.with_uv2, t.untextured, t.empty);
    println!("  meshes with exactly one render vertex per mesh vertex in use {}  nodes given the stand-in material {}", t.shared, t.default_material);
    println!("  meshes with triangles taken back because the original points a corner before the vertex array: {} ({} triangles; first: {})", t.wild.0, t.wild.1, t.wild_example);
    println!("refused (the original leaves its arrays there): {}", t.refused.values().map(|v| v.0).sum::<u64>());
    for (k, (n, example)) in &t.refused {
        println!("  {n:>5}  {k}  (first: {example})");
    }
    println!("panics: {}", t.panics);
    println!(
        "\nface nodes {} ({} of NURBS curves, which are not bound): {} give triangles ({} triangles), {} refused, {} panics in the face or patch converters",
        o.faces, o.nurbs_faces, o.face_made, o.face_triangles, o.face_refused, o.panics
    );
    println!("patch nodes {}: {} give triangles ({} render vertices, {} triangles), {} refused", o.patches, o.patch_made, o.patch_vertices, o.patch_triangles, o.patch_refused);
    let surface_panics = surfaces::run(Path::new(&dir));
    std::process::exit(if t.panics == 0 && surface_panics == 0 && o.panics == 0 { 0 } else { 1 });
}

mod surfaces;
