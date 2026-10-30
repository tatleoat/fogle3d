// SPDX-License-Identifier: MIT
//! `hrc-imagecheck <dir>`: every sample material and texture file read into a memory image
//! by the image form of this crate, and what it built there held against what the value
//! form reads from the same file.
//!
//! Per file: a fresh image for a whole run (`Mem::program()`), the image form's file
//! reader, then
//!
//! - no fault of the image (an access outside memory, a refused allocation, a free of what
//!   is no block);
//! - the stream stands where the value form's reader ended;
//! - the records in the image, read back field by field into the value form's structures
//!   by the adapters below, equal what the value form read.
//!
//! The report: files of each kind, how many were read by both forms, faults, every field
//! that disagrees with its count and one file, and the largest image (the heap's peak).
//! Run it under `ulimit -v 2000000`.

use si_hrc::image::geom::mesh as moff;
use si_hrc::image::node::node as noff;
use si_hrc::image::texture::lclmap;
use si_hrc::image::values::{material_of, string_at, texture3d_of, texture_of, user_data_of, vec3};
use si_hrc::image::{self, matio, MemOutside};
use si_hrc::{read_file, Cluster, Content, Definition, DynaMatter, Edge, Envelop, EnvelopVertex, Face, InvKin, Kind, Mesh, MetaGroup, MetaSystem, Metaball, Node, NurbsComposite, NurbsCurve, NurbsPoint, NurbsSurface, Patch, PolyNode, Polygon, Spline, SplineKey, Texture2D, Vertex};
use si_mem::Mem;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(t) if t.is_file() => out.push(p),
            _ => {}
        }
    }
}

fn kind_of(p: &Path) -> Option<(Kind, &'static str)> {
    match p.extension()?.to_str()? {
        "mtr" => Some((Kind::Material, "mtr")),
        "txt" => Some((Kind::Texture, "txt")),
        "t3D" => Some((Kind::Texture3D, "t3D")),
        "hrc" => Some((Kind::Model, "hrc")),
        _ => None,
    }
}

fn mesh_of(m: &Mem, p: u32, sets: i32) -> Mesh {
    let mut out = Mesh { angle: m.f32(p + moff::ANGLE), nvertices: m.i32(p + moff::NB_VERTICES), npolygons: m.i32(p + moff::NB_POLYGONS), flag: m.i16(p + moff::FLAGS), ..Mesh::default() };
    out.x38 = m.i16(p + 0x38);
    out.approx = vec3(m, p + 0x3c);
    (out.x48, out.x4a, out.x4c, out.x50, out.x52) = (m.i16(p + 0x48), m.i16(p + 0x4a), m.i16(p + 0x4c), m.i16(p + 0x50), m.i16(p + 0x52));
    let verts = m.u32(p + moff::VERTICES);
    for i in 0..out.nvertices.clamp(0, 10_000_000) as u32 {
        let v = verts + i * moff::VERTEX;
        out.vertices.push(Vertex { pos: vec3(m, v), flag: m.i16(v + moff::VERTEX_FLAG) });
    }
    let polys = m.u32(p + moff::POLYGONS);
    for i in 0..out.npolygons.clamp(0, 10_000_000) as u32 {
        let q = polys + i * moff::POLYGON;
        let mut poly = Polygon { normal: vec3(m, q), nnodes: m.u16(q + moff::POLYGON_NB_NODES), nodes: Vec::new(), material: m.i16(q + moff::POLYGON_MATERIAL), flag: m.i16(q + moff::POLYGON_FLAG) };
        let nodes = m.u32(q + moff::POLYGON_NODES);
        for k in 0..poly.nnodes as u32 {
            let c = nodes + k * moff::NODE;
            let more = m.u32(c + moff::NODE_MORE_UV);
            let more_uv = if more == 0 { Vec::new() } else { (0..(sets - 1).max(0) as u32).map(|j| [m.f32(more + 8 * j), m.f32(more + 8 * j + 4)]).collect() };
            // the colour: the file's big-endian value, swapped by the read into a word
            let colour = m.u32(c + moff::NODE_COLOUR);
            poly.nodes.push(PolyNode { vertex: m.i32(c), normal: vec3(m, c + moff::NODE_NORMAL), uv: [m.f32(c + moff::NODE_UV), m.f32(c + moff::NODE_UV + 4)], colour, more_uv });
        }
        out.polygons.push(poly);
    }
    out
}

fn spline_of(m: &Mem, s: u32) -> Spline {
    let n = m.u16(s + 0x10);
    let keys = m.u32(s + 0x14);
    let key = |i: u32| {
        let k = keys + 0x30 * i;
        SplineKey { prev: vec3(m, k), pos: vec3(m, k + 0xc), next: vec3(m, k + 0x18), constraint: m.i16(k + 0x28), segment: m.i16(k + 0x2a), flag: m.i16(k + 0x2c) }
    };
    Spline { name: string_at(m, m.u32(s)), phantom: m.i16(s + 6), close: m.i16(s + 8), kind: m.i32(s + 0xc), nkeys: n, keys: (0..n as u32).map(key).collect(), tension: m.f32(s + 0x1c), step: m.i16(s + 0x20), old_points: false }
}

fn nurbs_points_of(m: &Mem, at: u32, n: i32) -> Vec<NurbsPoint> {
    if at == 0 {
        return Vec::new();
    }
    (0..n.clamp(0, 10_000_000) as u32).map(|i| at + 0x24 * i).map(|q| NurbsPoint { xyzw: [m.f64(q), m.f64(q + 8), m.f64(q + 0x10), m.f64(q + 0x18)], flag: m.i32(q + 0x20) }).collect()
}

fn doubles_of(m: &Mem, at: u32, n: i32) -> Vec<f64> {
    if at == 0 {
        return Vec::new();
    }
    (0..n.clamp(0, 10_000_000) as u32).map(|i| m.f64(at + 8 * i)).collect()
}

fn nurbs_curve_of(m: &Mem, c: u32) -> NurbsCurve {
    let (npoints, nknots) = (m.i32(c + 4), m.i32(c + 8));
    NurbsCurve { order: m.i32(c), npoints, points: nurbs_points_of(m, m.u32(c + 0x2c), npoints), nknots, knots: doubles_of(m, m.u32(c + 0xc), nknots), start: m.f64(c + 0x10), end: m.f64(c + 0x18), closed: m.i16(c + 0x20), uniformity: m.i32(c + 0x24), closing: m.i32(c + 0x30), step: m.i32(c + 0x34), keyframed: m.i16(c + 0x4c) }
}

fn face_of(m: &Mem, f: u32) -> Face {
    let count = m.i16(f + 0x78);
    let (holes, curves) = (m.u32(f + 0x7c), m.u32(f + 0x80));
    let n = count.max(0) as u32;
    Face { exterior: spline_of(m, f + 4), count, holes: if holes == 0 { Vec::new() } else { (0..n).map(|i| spline_of(m, holes + 0x74 * i)).collect() }, nurbs: if curves == 0 { Vec::new() } else { (0..n).map(|i| nurbs_curve_of(m, curves + 100 * i)).collect() }, is_spline: m.i16(f + 0x84) }
}

fn patch_of(m: &Mem, p: u32) -> Patch {
    let s = |off: u32| m.i16(p + off);
    let (nu, nv) = (m.u16(p + 0xc), m.u16(p + 0xe));
    let n = nu as u32 * nv as u32;
    let (points, tags) = (m.u32(p + 0x1a4), m.u32(p + 0x1a8));
    Patch { kind: [m.i32(p + 4), m.i32(p + 8)], nu, nv, points: if points == 0 { Vec::new() } else { (0..n).map(|i| vec3(m, points + 0x10 * i)).collect() }, tags: if tags == 0 { Vec::new() } else { (0..n).map(|i| m.i16(tags + 2 * i)).collect() }, close: [s(0x10), s(0x12)], tension: [m.f32(p + 0x14), m.f32(p + 0x18)], step: [s(0x1c), s(0x1e)], curves: [s(0x20), s(0x22)], caps: [s(0x1cc), s(0x1ce), s(0x1d0), s(0x1d2)], faceted: s(0x1d4), view: s(0x1e8), edge: m.f32(p + 0x1ec), curvature: [m.f32(p + 0x1f0), m.f32(p + 0x1f4)], rec_min: s(0x1f8), rec_max: s(0x1fa), recursion: s(0x1fc), approx: s(0x200) }
}

fn composites_of(m: &Mem, array: u32, n: i32) -> Vec<NurbsComposite> {
    if array == 0 {
        return Vec::new();
    }
    (0..n.clamp(0, 1_000_000) as u32).map(|i| m.u32(array + 4 * i)).filter(|&c| c != 0).map(|c| {
        let (ncurves, curves) = (m.i32(c + 4), m.u32(c + 8));
        NurbsComposite { boundary: m.i16(c), ncurves, curves: if curves == 0 { Vec::new() } else { (0..ncurves.clamp(0, 1_000_000) as u32).map(|k| m.u32(curves + 4 * k)).filter(|&q| q != 0).map(|q| nurbs_curve_of(m, q)).collect() } }
    }).collect()
}

fn nurbs_surface_of(m: &Mem, p: u32) -> NurbsSurface {
    let (i, s) = (|off: u32| m.i32(p + off), |off: u32| m.i16(p + off));
    NurbsSurface {
        order: [i(0), i(0x24)], count: [i(4), i(0x28)], points: nurbs_points_of(m, m.u32(p + 0x54), i(0x48)), closed: [s(0x20), s(0x44)], uniformity: i(0x4c), nknots: [i(8), i(0x2c)],
        knots: [doubles_of(m, m.u32(p + 0xc), i(8)), doubles_of(m, m.u32(p + 0x30), i(0x2c))], range: [m.f64(p + 0x10), m.f64(p + 0x18), m.f64(p + 0x34), m.f64(p + 0x3c)], border: s(0x5c),
        ntrims: i(0x60), trims: composites_of(m, m.u32(p + 0x64), i(0x60)), nprojected: i(0x68), projected: composites_of(m, m.u32(p + 0x6c), i(0x68)), steps: [i(0x70), i(0x74)], curves: [i(0x78), i(0x7c)],
        caps: [s(0x88), s(0x8a), s(0x8c), s(0x8e)], closing: [i(0x90), i(0x94)], approx: s(0x98), view: s(0x9c), edge: m.f32(p + 0xa0), curvature: [m.f32(p + 0xa4), m.f32(p + 0xa8)], rec_min: s(0xac), rec_max: s(0xae), recursion: s(0xb0),
    }
}

fn metaball_of(m: &Mem, p: u32) -> Metaball {
    let mut matrix = [0f32; 16];
    for (k, v) in matrix.iter_mut().enumerate() {
        *v = m.f32(p + 0xc + 4 * k as u32);
    }
    Metaball { matrix, ratio: m.f64(p + 0xcc), weight: m.f64(p + 0xd4), exponent: [m.f64(p + 0xdc), m.f64(p + 0xe4), m.f64(p + 0xec)] }
}

fn meta_system_of(m: &Mem, p: u32) -> MetaSystem {
    let mut groups = Vec::new();
    let mut g = m.u32(p + 0x5c);
    while g != 0 && groups.len() < 100_000 {
        let mut primitives = Vec::new();
        let mut q = m.u32(g + 8);
        while q != 0 && primitives.len() < 100_000 {
            primitives.push(string_at(m, m.u32(q + 0xc)));
            q = m.u32(q + 4);
        }
        groups.push(MetaGroup { name: string_at(m, m.u32(g)), primitives });
        g = m.u32(g + 0xc);
    }
    let mut unblends = Vec::new();
    let mut u = m.u32(p + 0x60);
    while u != 0 && unblends.len() < 100_000 {
        unblends.push([string_at(m, m.u32(u + 0x10)), string_at(m, m.u32(u + 0x14))]);
        u = m.u32(u + 0xc);
    }
    MetaSystem { wire_res: m.f32(p + 0x34), render_res: m.f32(p + 0x38), show: m.i16(p + 0x42), groups, unblends }
}

/// An envelope with its lists; the matrices of an old file (kept in a record that the
/// node's user data pointer holds: no tag, the table of the lists' matrix arrays as data).
fn envelop_of(m: &Mem, e: u32, node: u32) -> Envelop {
    let f = |off: u32| m.f32(e + off);
    let curve = |at: u32| if at == 0 { None } else { Some((0..m.block_size(at).unwrap_or(0) / 4).map(|i| m.f32(at + 4 * i)).collect::<Vec<f32>>()) };
    let record = m.u32(node + noff::USER_DATA);
    let table = if record != 0 && m.u32(record) == 0 && m.u32(record + 8) == 0 { m.u32(record + 0xc) } else { 0 };
    let (mut lists, mut old_matrices) = (Vec::new(), Vec::new());
    let mut list = m.u32(e + 4);
    while list != 0 && lists.len() < 100_000 {
        let mut vertices = Vec::new();
        let mut v = m.u32(list + 4);
        while v != 0 && vertices.len() < 10_000_000 {
            vertices.push(EnvelopVertex { index: m.i32(v), ratio: m.f32(v + 4), thickness: m.f32(v + 8), weight: m.f32(v + 0xc) });
            v = m.u32(v + 0x10);
        }
        if table != 0 {
            let array = m.u32(table + 4 * lists.len() as u32);
            old_matrices.push((0..if array == 0 { 0 } else { vertices.len() as u32 }).map(|i| {
                let mut x = [0f32; 16];
                for (k, v) in x.iter_mut().enumerate() {
                    *v = m.f32(array + 0x40 * i + 4 * k as u32);
                }
                x
            }).collect());
        }
        lists.push(vertices);
        list = m.u32(list + 0xc);
    }
    Envelop { level: m.i32(e), lists, x54: vec3(m, e + 0x54), joint_area: f(0x6c), in_type: m.i32(e + 0x70), in_curve: curve(m.u32(e + 0x74)), inner: [f(0x78), f(0x7c), f(0x80), f(0x84)], out_type: m.i32(e + 0x88), out_curve: curve(m.u32(e + 0x8c)), outer: [f(0x90), f(0x94), f(0x98), f(0x9c)], old_matrices }
}

fn cluster_of(m: &Mem, p: u32, doubled: bool) -> Option<Cluster> {
    if p == 0 {
        return None;
    }
    let count = m.i32(p + 4);
    let n = if doubled { count.wrapping_mul(2) } else { count }.clamp(0, 10_000_000) as u32;
    let at = m.u32(p + 0xc);
    Some(Cluster { name: Vec::new(), count, indices: (0..n).map(|i| m.i32(at + 4 * i)).collect(), state: m.i32(p + 0x10) })
}

/// The nodes of a hierarchy in the image in the file's order (a node, its sons, then its
/// brothers), each with its depth and without its links.
fn image_nodes(m: &Mem, root: u32) -> Vec<(usize, Node)> {
    let mut out = Vec::new();
    let mut stack = vec![(0usize, root)];
    while let Some((depth, p)) = stack.pop() {
        if p == 0 || out.len() > 10_000_000 {
            continue;
        }
        stack.push((depth, m.u32(p + noff::BROTHER)));
        stack.push((depth + 1, m.u32(p + noff::SON)));
        out.push((depth, node_of(m, p)));
    }
    out
}

/// The same for the value form's tree, which is taken apart.
fn value_nodes(root: Node) -> Vec<(usize, Node)> {
    let mut out = Vec::new();
    let mut stack = vec![(0usize, root)];
    while let Some((depth, mut n)) = stack.pop() {
        let (son, brother) = (n.son.take(), n.brother.take());
        if let Some(b) = brother {
            stack.push((depth, *b));
        }
        if let Some(s) = son {
            stack.push((depth + 1, *s));
        }
        out.push((depth, n));
    }
    out
}

/// A node and what hangs on it, as far as the image form builds it itself; the links to
/// its son and brother are not followed.
fn node_of(m: &Mem, p: u32) -> Node {
    let kind = m.i32(p + noff::TYPE);
    let sets = m.i32(p + noff::NB_UV_SETS);
    let mut n = Node { name: string_at(m, m.u32(p + noff::NAME)), kind, ..Node::default() };
    n.flag8 = m.u8(p + noff::FLAGS) & 8 != 0;
    let g = m.u32(p + noff::GEOMETRY);
    if g != 0 {
        match kind {
            1 => n.definitions.push(Definition::Patch(patch_of(m, g))),
            2 => n.definitions.push(Definition::Face(face_of(m, g))),
            4 => n.definitions.push(Definition::Mesh(mesh_of(m, g, sets))),
            6 => n.definitions.push(Definition::Spline(spline_of(m, g))),
            7 => n.definitions.push(Definition::MetaSystem(meta_system_of(m, g))),
            8 => n.definitions.push(Definition::Metaball(metaball_of(m, g))),
            9 => n.definitions.push(Definition::NurbsCurve(nurbs_curve_of(m, g))),
            10 => n.definitions.push(Definition::NurbsSurface(nurbs_surface_of(m, g))),
            _ => {}
        }
    }
    n.scaling = vec3(m, p + noff::SCALING);
    n.rotation = vec3(m, p + noff::ROTATION);
    n.translation = vec3(m, p + noff::TRANSLATION);
    n.nb_materials = m.i16(p + noff::NB_MATERIALS);
    let mats = m.u32(p + noff::MATERIALS);
    if mats != 0 {
        for i in 0..n.nb_materials.max(0) as u32 {
            let q = m.u32(mats + 4 * i);
            n.materials.push(if q == 0 { None } else { Some(material_of(m, q)) });
        }
    }
    n.nb_textures = m.i16(p + noff::NB_TEXTURES);
    let mut t = m.u32(p + noff::TEXTURES);
    while t != 0 && n.textures.len() < 100_000 {
        n.textures.push(texture_of(m, t));
        t = m.u32(t + lclmap::NEXT);
    }
    let t3 = m.u32(p + noff::TEXTURE3D);
    n.texture3d = if t3 == 0 { None } else { Some(texture3d_of(m, t3)) };
    let d = m.u32(p + noff::DYNA_MATTER);
    if d != 0 {
        n.dynamics = Some(DynaMatter { density: m.f32(d), roughness: m.f32(d + 0xc), kinetic: m.f32(d + 0x10), elasticity: m.f32(d + 0x14), rigidity: m.f32(d + 0x18), plasticity: m.f32(d + 0x1c), collision: m.i32(d + 0x80) });
    }
    let friction = m.u32(p + 0x2f8);
    if friction != 0 {
        n.joint_friction = Some(vec3(m, friction));
    }
    let ik = m.u32(p + noff::INV_KIN);
    if ik != 0 {
        n.ik = Some(InvKin { kind: m.i32(ik), nb_joints: m.i32(ik + 4), rotation: vec3(m, ik + 0x20) });
    }
    let e = m.u32(p + noff::ENVELOPE);
    if e != 0 {
        n.envelop = Some(envelop_of(m, e, p));
    }
    let mut k = m.u32(p + 0x348);
    while k != 0 && n.clusters.len() < 100_000 {
        let mut c = cluster_of(m, k, kind == 2).unwrap();
        c.name = string_at(m, m.u32(k));
        n.clusters.push(c);
        k = m.u32(k + 0x1c);
    }
    n.shape_interp = m.i16(p + noff::SHAPE_INTERPOLATION);
    n.algorithm = m.i32(p + noff::ALGORITHM);
    n.envelop_type = m.i32(p + noff::ENVELOPE_TYPE);
    let srt = m.u32(p + noff::INITIAL_SRT);
    if srt != 0 {
        n.init_transfo = Some([vec3(m, srt), vec3(m, srt + 0xc), vec3(m, srt + 0x18)]);
    }
    n.user_nodes = user_data_of(m, m.u32(p + noff::USER_DATA));
    n.visibility = m.i16(p + noff::VISIBILITY);
    n.motion_blur = m.i16(p + noff::MOTION_BLUR);
    n.matte = m.f32(p + noff::MATTE);
    n.garbage_matte = m.i16(p + noff::GARBAGE_MATTE);
    (n.x404, n.x406, n.x420) = (m.i16(p + noff::TOKEN_221), m.i16(p + noff::TOKEN_222), m.i16(p + noff::TOKEN_223));
    for k in 0..3u32 {
        n.ud_clusters[k as usize] = cluster_of(m, m.u32(p + noff::VERTEX_CLUSTER + 8 * k), k == 0 && kind == 2);
        let array = m.u32(p + noff::VERTEX_USER_DATA + 8 * k);
        if array != 0 {
            let count = n.ud_clusters[k as usize].as_ref().map_or(0, |c| c.count).clamp(0, 10_000_000) as u32;
            n.ud_arrays[k as usize] = Some((0..count).map(|i| user_data_of(m, m.u32(array + 4 * i))).collect());
        }
    }
    n.uv_sets = sets;
    let domains = m.u32(p + noff::UV_DOMAINS);
    if domains != 0 {
        n.uv_domains = Some((0..(sets - 1).max(0) as u32).map(|i| {
            let (ids, count) = (m.u32(domains + 8 * i), m.i32(domains + 8 * i + 4).clamp(0, 10_000_000) as u32);
            (0..count).map(|k| m.i32(ids + 4 * k)).collect()
        }).collect());
    }
    n
}

/// Why the image form cannot read a model, if it cannot: nothing any more (every geometry
/// and chunk of the corpus has its reader on the image). `--skip-kind N` leaves out the
/// models with a node of type N, as if its reader were not on the image (for counting
/// what each group of readers added).
fn unsupported(n: &Node) -> Option<&'static str> {
    let skip: Vec<i32> = std::env::args().skip_while(|a| a != "--skip-kind").nth(1).map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let mut why = None;
    n.walk(0, &mut |_, x| {
        if why.is_none() && skip.contains(&x.kind) {
            why = Some("a node of a type left out with --skip-kind");
        }
    });
    why
}

/// The value form's node with what the image form does not build itself taken out, on
/// both sides alike: the file's edge list (the original frees it after copying its flags
/// onto the rebuilt edges), the "compute normals" mark (a local of the mesh reader), the
/// chain dynamics state (an allocator of `settings.c`), and the five fields of a 2D
/// texture that `SI_txt2dSetWrap (0x082ad9f8)` sets from the node's shape.
fn comparable(n: &mut Node) {
    let texture = |t: &mut Texture2D| {
        (t.wrap_u, t.wrap_v, t.method, t.uscale, t.vscale) = (0, 0, 0, 0.0, 0.0);
    };
    for d in &mut n.definitions {
        if let Definition::Mesh(mesh) = d {
            mesh.nedges = 0;
            mesh.edges = Vec::<Edge>::new();
            mesh.compute_normals = false;
        }
    }
    n.chain_dynamics = false;
    n.textures.iter_mut().for_each(texture);
    for material in n.materials.iter_mut().flatten() {
        material.textures.iter_mut().for_each(texture);
    }
}

/// A spline the value form read from an old point list of type 1 holds the points; the
/// image holds what `SI_bezierShrink` made of them (the value form has no such step). The
/// keys of such a pair are left out of the comparison; the answer is how many there were.
fn shrunk(value: &mut Node, image: &mut Node) -> u32 {
    fn pair(v: &mut Spline, i: &mut Spline) -> u32 {
        let was = v.old_points && v.kind == 1;
        if was {
            (v.nkeys, i.nkeys) = (0, 0);
            v.keys.clear();
            i.keys.clear();
        }
        v.old_points = false;
        was as u32
    }
    let mut n = 0;
    for (a, b) in value.definitions.iter_mut().zip(image.definitions.iter_mut()) {
        match (a, b) {
            (Definition::Spline(v), Definition::Spline(i)) => n += pair(v, i),
            (Definition::Face(v), Definition::Face(i)) => {
                n += pair(&mut v.exterior, &mut i.exterior);
                for (x, y) in v.holes.iter_mut().zip(i.holes.iter_mut()) {
                    n += pair(x, y);
                }
            }
            _ => {}
        }
    }
    n
}

/// The fields of two values of one structure that differ, by the names `Debug` prints:
/// the two are printed with one field per line and the lines compared. Floats are compared
/// by `Debug` text, so two NaNs agree and 0.0 and −0.0 do not.
fn differing_fields<T: std::fmt::Debug>(a: &T, b: &T) -> Vec<(String, String)> {
    let (ta, tb) = (format!("{a:#?}"), format!("{b:#?}"));
    if ta == tb {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let (la, lb): (Vec<&str>, Vec<&str>) = (ta.lines().collect(), tb.lines().collect());
    if la.len() != lb.len() {
        // lists of different lengths: name the first line that differs
        let i = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(la.len().min(lb.len()));
        let line = la.get(i).or(lb.get(i)).copied().unwrap_or("");
        return vec![(format!("(shape) near `{}`", line.trim()), String::new())];
    }
    for (x, y) in la.iter().zip(&lb) {
        let depth = (x.len() - x.trim_start().len()) / 4;
        let name = x.trim().split(':').next().unwrap_or("").to_string();
        path.truncate(depth);
        if x != y {
            let mut full: Vec<&str> = path.iter().map(String::as_str).filter(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())).collect();
            if name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                full.push(&name);
            }
            let key = full.join(".");
            if !out.iter().any(|(k, _)| *k == key) {
                out.push((key, format!("value form `{}`, image `{}`", x.trim(), y.trim())));
            }
        }
        path.push(name);
    }
    out
}

#[derive(Default)]
struct Tally {
    files: u32,
    not_read_by_value_form: u32,
    both: u32,
    nothing_in_both: u32,
    same: u32,
    faults: BTreeMap<String, (u32, String)>,
    end_differs: (u32, String),
    fields: BTreeMap<String, (u32, String)>,
    unsupported: BTreeMap<&'static str, u32>,
    nodes: u32,
    shrunk: u32,
    peak: (u32, String),
    blocks: (u32, String),
}

fn main() {
    // brother chains recurse once per node, in the readers and in the adapters
    let child = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.iter().enumerate().find(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--skip-kind")).map(|(_, a)| a) else {
        eprintln!("usage: hrc-imagecheck <dir>");
        return 2;
    };
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    let mut tallies: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for path in &files {
        let Some((kind, ext)) = kind_of(path) else { continue };
        let Ok(data) = std::fs::read(path) else { continue };
        let t = tallies.entry(ext).or_default();
        t.files += 1;
        let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        // the value form; a file it does not take for a binary one (no header) is not compared
        let value = if data.is_empty() { None } else { read_file(&data, kind) };
        let Some(value) = value.filter(|f| f.header.is_some()) else {
            t.not_read_by_value_form += 1;
            continue;
        };

        if let Content::Model(root) = &value.content {
            if let Some(why) = unsupported(root) {
                *t.unsupported.entry(why).or_default() += 1;
                continue;
            }
        }
        // the image form: the file under a made-up name, the reader of its kind. The image
        // stops at the first fault, as the original is killed at its first wild access.
        let mut m = Mem::program();
        image::init(&mut m);
        let mut o = MemOutside::default();
        let name = m.place_cstr(b"/corpus/element");
        let suffix: &[u8] = match kind {
            Kind::Material => b".mtr",
            Kind::Texture => b".txt",
            Kind::Texture3D => b".t3D",
            Kind::Model => b".hrc",
        };
        o.files.add(&[b"/corpus/element", suffix].concat(), data.clone());
        m.on_fault(si_mem::OnFault::Stop);
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
            Kind::Material => matio::material_read_binary_file(&mut m, &mut o, name).addr(),
            Kind::Texture => matio::texture_read_binary_file(&mut m, &mut o, name).addr(),
            Kind::Texture3D => matio::texture3d_read_binary_file(&mut m, &mut o, name).addr(),
            Kind::Model => {
                let file = o.files.open_bytes(b"/corpus/element.hrc", data.clone());
                si_hrc::image::node::fdmodel_binary_read(&mut m, &mut o, file, 0, 0)
            }
        }));
        m.on_fault(si_mem::OnFault::Record);
        let record = ran.unwrap_or(0);
        t.both += 1;
        if let Some(f) = m.fault() {
            let e = t.faults.entry(format!("{:?}", f.access)).or_insert((0, rel.clone()));
            e.0 += 1;
        }
        if let Err(e) = m.heap_verify() {
            let e = t.faults.entry(format!("heap: {e}")).or_insert((0, rel.clone()));
            e.0 += 1;
        }
        let stats = m.heap_stats();
        if stats.peak as u32 >= t.peak.0 {
            t.peak = (stats.peak as u32, rel.clone());
        }
        let live = m.live_blocks().len() as u32;
        if live >= t.blocks.0 {
            t.blocks = (live, rel.clone());
        }
        let end = o.files.stream(o.files.last()).map_or(0, |s| s.pos);
        if end != value.end {
            t.end_differs.0 += 1;
            t.end_differs.1 = format!("{rel}: value form {}, image form {end}", value.end);
        }
        let differing = match (value.content, record) {
            (Content::None, 0) => {
                t.nothing_in_both += 1;
                Vec::new()
            }
            (Content::Material(v), p) if p != 0 => differing_fields(&v, &material_of(&m, p)),
            (Content::Texture(v), p) if p != 0 => differing_fields(&v, &texture_of(&m, p)),
            (Content::Texture3D(v), p) if p != 0 => differing_fields(&v, &texture3d_of(&m, p)),
            (Content::Model(v), p) if p != 0 => {
                let (a, b) = (value_nodes(*v), image_nodes(&m, p));
                t.nodes += a.len() as u32;
                let mut out: Vec<(String, String)> = Vec::new();
                if a.len() != b.len() {
                    out.push(("(number of nodes)".to_string(), format!("value form {}, image {}", a.len(), b.len())));
                }
                for ((da, mut x), (db, mut y)) in a.into_iter().zip(b) {
                    if da != db {
                        out.push(("(depth of a node)".to_string(), format!("value form {da}, image {db}")));
                        break;
                    }
                    comparable(&mut x);
                    comparable(&mut y);
                    t.shrunk += shrunk(&mut x, &mut y);
                    for d in differing_fields(&x, &y) {
                        if !out.iter().any(|(k, _)| *k == d.0) {
                            out.push(d);
                        }
                    }
                }
                out
            }
            _ => vec![("(one form read a record, the other none)".to_string(), String::new())],
        };
        if differing.is_empty() {
            t.same += 1;
        }
        for (field, how) in differing {
            let e = t.fields.entry(field).or_insert((0, format!("{rel}: {how}")));
            e.0 += 1;
        }
    }
    let mut bad = false;
    for (ext, t) in &tallies {
        println!(".{ext}: {} files; {} without a binary header (not read by the value form, not compared); {} read by both forms", t.files, t.not_read_by_value_form, t.both);
        println!("  same record in both: {} (of which no record at all: {})", t.same, t.nothing_in_both);
        for (why, n) in &t.unsupported {
            println!("  not attempted, the model has {why}, whose reader is not on the image: {n}");
        }
        if t.nodes != 0 {
            println!("  nodes in the models compared: {}", t.nodes);
        }
        if t.shrunk != 0 {
            println!("  splines read from an old point list and shrunk by the image form (keys not compared): {}", t.shrunk);
        }
        println!("  faults of the image: {}", t.faults.values().map(|v| v.0).sum::<u32>());
        for (what, (n, file)) in &t.faults {
            println!("    {what}: {n} files, e.g. {file}");
        }
        println!("  the stream ends elsewhere than the value form's reader: {} {}", t.end_differs.0, t.end_differs.1);
        for (field, (n, file)) in &t.fields {
            println!("  field `{field}` differs in {n} files, e.g. {file}");
        }
        println!("  largest image: heap peak {} bytes ({}); most live blocks {} ({})", t.peak.0, t.peak.1, t.blocks.0, t.blocks.1);
        bad |= !t.faults.is_empty() || t.end_differs.0 != 0 || !t.fields.is_empty();
    }
    bad as i32
}
