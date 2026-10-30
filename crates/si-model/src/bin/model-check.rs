// SPDX-License-Identifier: MIT
//! Runs the mesh rebuild over every model file under a directory and compares what it
//! computes with what the files store.
//!
//! ```text
//!   model-check <dir>            the counts
//!   model-check --list <dir>     also one line per file that is not identical, with its class
//!   model-check --one <file>     every differing corner and edge of one file
//!   model-check --polygon-experiment <dir>   faceted meshes against other normal formulas
//!   model-check --carry-experiment <dir>     normals carried through a move against recomputed ones
//!   model-check --triangulate <dir>          every polygon of every mesh through the triangulator
//! ```
//!
//! A file stores, per mesh, the corner normals and an edge list with flags (newer files),
//! or neither (older files). The check loads each mesh as the program does
//! (`si_model::load`), then, for meshes with stored normals, computes the border and
//! discontinuity edges and the corner normals from the geometry and compares bit for bit.
//! Memory: run it under a cap, `(ulimit -v 2000000; timeout 600 model-check <dir>)`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{read_file, Content, Definition, Kind};
use si_model::load::{finish_read, mesh_from_file};
use si_model::mesh::Mesh;
use si_model::mesh_b::{self, Eval, MeshEnv};

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

#[derive(Default, Clone, Copy, PartialEq)]
struct Count {
    models: u64,
    meshes: u64,
    polygons: u64,
    corners: u64,
}

impl Count {
    fn add(&mut self, o: Count) {
        self.models += o.models;
        self.meshes += o.meshes;
        self.polygons += o.polygons;
        self.corners += o.corners;
    }
}

impl std::fmt::Display for Count {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:>6} models {:>6} meshes {:>8} polygons {:>9} corners", self.models, self.meshes, self.polygons, self.corners)
    }
}

/// How one corner's computed normal relates to the stored one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Diff {
    Same,
    ZeroSign,
    OneUlp,
    Small,
    Other,
}

fn compare(a: [f32; 3], b: [f32; 3]) -> Diff {
    if (0..3).all(|i| a[i].to_bits() == b[i].to_bits()) {
        return Diff::Same;
    }
    if (0..3).all(|i| a[i] == b[i]) {
        return Diff::ZeroSign;
    }
    let ulp = |x: f32, y: f32| (x.to_bits() as i64 - y.to_bits() as i64).abs();
    if (0..3).all(|i| a[i] == b[i] || (a[i].is_sign_negative() == b[i].is_sign_negative() && ulp(a[i], b[i]) <= 1)) {
        return Diff::OneUlp;
    }
    if (0..3).all(|i| (a[i] - b[i]).abs() <= 1e-5) {
        return Diff::Small;
    }
    Diff::Other
}

#[derive(Default)]
struct MeshResult {
    polygons: u64,
    corners: u64,
    /// corners by difference class, polygons whose corners are all `Same`
    by_diff: BTreeMap<Diff, u64>,
    polygons_same: u64,
    worst: Option<Diff>,
}

fn compare_normals(file: &si_hrc::Mesh, m: &Mesh) -> MeshResult {
    let mut r = MeshResult::default();
    for (fp, p) in file.polygons.iter().zip(&m.polygons) {
        r.polygons += 1;
        let mut all = true;
        for (fnode, node) in fp.nodes.iter().zip(&p.nodes) {
            if node.vertex == -1 {
                continue;
            }
            r.corners += 1;
            let d = compare(fnode.normal, node.normal);
            *r.by_diff.entry(d).or_default() += 1;
            if d != Diff::Same {
                all = false;
                if r.worst.is_none_or(|w| d > w) {
                    r.worst = Some(d);
                }
            }
        }
        if all {
            r.polygons_same += 1;
        }
    }
    r
}

#[derive(Default)]
struct Tally {
    files: u64,
    unread: u64,
    nodes: u64,
    meshes: u64,
    // the load itself
    polygons: u64,
    corners: u64,
    not_filed: u64,
    messages: BTreeMap<String, u64>,
    mesh_flags: BTreeMap<u16, u64>,
    edge_flags_file: BTreeMap<i16, u64>,
    bad_vertex: u64,
    single_corner_contours: u64,
    holes: u64,
    // edges against the file's list
    with_edge_list: u64,
    edge_count_same: u64,
    edge_order_same: u64,
    edge_set_same: u64,
    edges_listed: u64,
    edges_rebuilt: u64,
    edges_unmapped: u64,
    edge_classes: BTreeMap<String, (u64, String)>,
    // discontinuity bits against the file's
    hard_edges_compared: u64,
    hard_bits: BTreeMap<(Eval, String), u64>,
    variant: [Count; 6],
    first_corner: [u64; 7],
    first_polygon: [u64; 7],
    first_mesh: [u64; 7],
    first_model: [u64; 7],
    unexplained: BTreeMap<Diff, u64>,
    extra_hits: BTreeMap<String, u64>,
    corners_by_version: BTreeMap<String, [u64; 7]>,
    fan_tried: u64,
    fan_hits: BTreeMap<String, u64>,
    unexplained_place: BTreeMap<(Diff, String), u64>,
    hard_by_flag: BTreeMap<(u16, String), u64>,
    // stored normals
    stored: Count,
    not_automatic: Count,
    corner_diff_geometry: BTreeMap<Diff, u64>,
    model_classes: BTreeMap<String, (u64, String)>,
    by_version: BTreeMap<String, (u64, u64, u64)>,
    // old files
    computed: Count,
    computed_unit: u64,
    computed_zero: u64,
    computed_other: u64,
    computed_flags: BTreeMap<(String, u16), u64>,
}

fn main() {
    let child = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap());
}

fn bits(flag: i16) -> String {
    format!("{:#04x}", flag & 0x38)
}

const VARIANT: [&str; 6] = [
    "x87 (the port), from the geometry", "x87 (the port), the file's edge flags", "single, from the geometry", "single, the file's edge flags",
    "double, from the geometry", "double, the file's edge flags",
];
const FIRST: [&str; 7] = [
    "the port, from the geometry alone", "the port, with the file's edge flags", "single-precision evaluation, from the geometry",
    "single-precision evaluation, with the file's edge flags", "double evaluation, from the geometry",
    "double evaluation, with the file's edge flags", "none of the six",
];

fn check_file(data: &[u8], t: &mut Tally, detail: bool) -> Option<Option<String>> {
    let f = read_file(data, Kind::Model)?;
    let Content::Model(root) = &f.content else { return None };
    let version = f.header.as_ref().map_or(-1.0, |h| h.version);
    let mut class = None;
    let mut file_has_stored = false;
    let mut file_variant = [true; 6];
    let mut file_first = 0usize;
    let mut file_computed = false;
    let mut shown = 0;
    let mut env = MeshEnv::default();
    let mut nodes = Vec::new();
    root.walk(0, &mut |_, n| nodes.push(n));
    for n in nodes {
        t.nodes += 1;
        for d in &n.definitions {
            let Definition::Mesh(fm) = d else { continue };
            t.meshes += 1;
            *t.mesh_flags.entry(fm.flag as u16).or_default() += 1;
            let nv = fm.vertices.len() as i32;
            for p in &fm.polygons {
                let mut run = 0;
                for (i, c) in p.nodes.iter().enumerate() {
                    if c.vertex == -1 {
                        t.holes += 1;
                        if run == 1 {
                            t.single_corner_contours += 1;
                        }
                        run = 0;
                    } else {
                        run += 1;
                        if c.vertex < 0 || c.vertex >= nv {
                            t.bad_vertex += 1;
                        }
                    }
                    if i + 1 == p.nodes.len() && run == 1 {
                        t.single_corner_contours += 1;
                    }
                }
            }
            for e in &fm.edges {
                *t.edge_flags_file.entry(e.flag).or_default() += 1;
            }
            // the program's load
            let mut m = mesh_from_file(fm);
            env.messages.clear();
            let loaded = finish_read(&mut m, &fm.edges, fm.compute_normals, Some(&n.name), &mut env);
            let corners: u64 = m.polygons.iter().map(|p| p.nodes.iter().filter(|c| c.vertex != -1).count() as u64).sum();
            t.polygons += m.polygons.len() as u64;
            t.corners += corners;
            if !loaded.filed {
                t.not_filed += 1;
            }
            for msg in &env.messages {
                let kind = msg.split('(').nth(1).and_then(|s| s.split(',').next()).unwrap_or("?").to_string();
                *t.messages.entry(kind).or_default() += 1;
            }
            // edges
            if !fm.edges.is_empty() {
                t.with_edge_list += 1;
                t.edges_listed += fm.edges.len() as u64;
                t.edges_rebuilt += m.nb_edges() as u64;
                t.edges_unmapped += loaded.edge_map.iter().filter(|&&e| e == 0).count() as u64;
                let same_count = fm.edges.len() as i32 == m.nb_edges();
                let same_order = same_count && fm.edges.iter().zip(&m.edges[1..]).all(|(a, b)| a.v == [b.v0, b.v1]);
                let key = |a: i32, b: i32| (a.min(b), a.max(b));
                let mut fs: Vec<_> = fm.edges.iter().map(|e| key(e.v[0], e.v[1])).collect();
                let mut ms: Vec<_> = m.edges.iter().skip(1).map(|e| key(e.v0, e.v1)).collect();
                fs.sort();
                ms.sort();
                let same_set = fs == ms;
                t.edge_count_same += same_count as u64;
                t.edge_order_same += same_order as u64;
                t.edge_set_same += same_set as u64;
                if !same_order {
                    let class = if !loaded.filed {
                        "a polygon could not be filed"
                    } else if same_set {
                        "same edges, other order or direction"
                    } else if fs.len() > ms.len() {
                        "the file lists more edges"
                    } else if fs.len() < ms.len() {
                        "the file lists fewer edges"
                    } else {
                        "same number, other edges"
                    };
                    let e = t.edge_classes.entry(class.to_string()).or_insert((0, String::new()));
                    e.0 += 1;
                }
            }
            if fm.compute_normals {
                // an old file: the program computed the normals during the load
                if corners > 0 {
                    *t.computed_flags.entry((format!("{version}"), fm.flag as u16)).or_default() += 1;
                    file_computed = true;
                    t.computed.meshes += 1;
                    t.computed.polygons += m.polygons.len() as u64;
                    t.computed.corners += corners;
                    for p in &m.polygons {
                        for c in p.nodes.iter().filter(|c| c.vertex != -1) {
                            let l = c.normal.iter().map(|&v| v as f64 * v as f64).sum::<f64>().sqrt();
                            if (l - 1.0).abs() < 1e-5 {
                                t.computed_unit += 1;
                            } else if l == 0.0 {
                                t.computed_zero += 1;
                            } else {
                                t.computed_other += 1;
                            }
                        }
                    }
                }
                continue;
            }
            if corners == 0 {
                continue;
            }
            // stored normals: compute them and compare
            let this = Count { models: 0, meshes: 1, polygons: m.polygons.len() as u64, corners };
            if m.flags & 0x800 == 0 && m.flags & 0xa == 0 {
                t.not_automatic.add(this);
                continue;
            }
            file_has_stored = true;
            t.stored.add(this);
            // six computations: three evaluations, each from the geometry alone (border and
            // discontinuity bits recomputed) and with the edge flags the file lists
            let mut results: Vec<Mesh> = Vec::with_capacity(6);
            for eval in [Eval::X87, Eval::Single, Eval::Double] {
                let mut with_flags = m.clone();
                mesh_b::computes_polygon_normal_with(&mut with_flags, -1, eval);
                let mut geometry = with_flags.clone();
                mesh_b::compute_hard_edge_with(&mut geometry, -1, eval);
                for (i, fe) in fm.edges.iter().enumerate() {
                    let e = loaded.edge_map[i];
                    if e != 0 {
                        if eval == Eval::X87 {
                            t.hard_edges_compared += 1;
                        }
                        let (was, now) = (fe.flag & 0x38, geometry.edges[e as usize].flag & 0x38);
                        let k = if was == now { "same".to_string() } else { format!("file {} computed {}", bits(was), bits(now)) };
                        if eval == Eval::X87 {
                            *t.hard_by_flag.entry((m.flags, k.clone())).or_default() += 1;
                        }
                        *t.hard_bits.entry((eval, k)).or_default() += 1;
                        if detail && was != now && eval == Eval::X87 {
                            let ed = geometry.edges[e as usize];
                            let cos = if ed.p0 >= 0 && ed.p1 >= 0 {
                                let (n0, n1) = (geometry.polygons[ed.p0 as usize].normal, geometry.polygons[ed.p1 as usize].normal);
                                format!("{:?}", si_math::algebra::vector_cos_angle(&si_math::types::Vector::from_array(n0), &si_math::types::Vector::from_array(n1)))
                            } else {
                                "-".into()
                            };
                            println!("  edge {e} ({},{}) polygons {} {}: file {} computed {} cos {cos} limit {:?} angle {}",
                                     ed.v0, ed.v1, ed.p0, ed.p1, bits(was), bits(now), geometry.cos_angle, geometry.angle);
                        }
                    }
                }
                mesh_b::computes_vertex_normal_with(&mut geometry, -1, eval);
                mesh_b::computes_vertex_normal_with(&mut with_flags, -1, eval);
                results.push(geometry);
                results.push(with_flags);
            }
            for (vi, r) in results.iter().enumerate() {
                let c = compare_normals(fm, r);
                t.variant[vi].polygons += c.polygons_same;
                t.variant[vi].corners += c.by_diff.get(&Diff::Same).copied().unwrap_or(0);
                if c.worst.is_none() {
                    t.variant[vi].meshes += 1;
                } else {
                    file_variant[vi] = false;
                }
                if vi == 0 {
                    for (d, n) in &c.by_diff {
                        *t.corner_diff_geometry.entry(*d).or_default() += n;
                    }
                }
            }
            // each corner: the first computation that reproduces it
            let mut extra: Option<Vec<(String, Mesh)>> = None;
            let mut mesh_first = 0usize;
            for (pi, fp) in fm.polygons.iter().enumerate() {
                let mut poly_first = 0usize;
                for (ci, fc) in fp.nodes.iter().enumerate() {
                    if fc.vertex == -1 {
                        continue;
                    }
                    let first = (0..6).find(|&vi| compare(fc.normal, results[vi].polygons[pi].nodes[ci].normal) == Diff::Same).unwrap_or(6);
                    t.first_corner[first] += 1;
                    poly_first = poly_first.max(first);
                    let vkey = format!("{version}");
                    let by = t.corners_by_version.entry(vkey).or_default();
                    by[if first == 0 { 0 } else if first < 6 { 1 } else { 2 + compare(fc.normal, results[0].polygons[pi].nodes[ci].normal) as usize }] += 1;
                    if first == 6 {
                        let d = compare(fc.normal, results[0].polygons[pi].nodes[ci].normal);
                        if d <= Diff::Small {
                            let extra = extra.get_or_insert_with(|| {
                                let mut v = Vec::new();
                                for (float_sums, float_len) in [(false, false), (true, false), (true, true)] {
                                    for eval in [Eval::Single, Eval::X87] {
                                        let mut g = m.clone();
                                        float_polygon_normals(&mut g, float_sums, float_len);
                                        mesh_b::compute_hard_edge_with(&mut g, -1, eval);
                                        mesh_b::computes_vertex_normal_with(&mut g, -1, eval);
                                        v.push((format!("polygon normals from float terms, {} sums, {} length; the rest {eval:?}",
                                                        if float_sums { "float" } else { "double" }, if float_len { "float" } else { "double" }), g));
                                    }
                                }
                                v
                            });
                            let hit = extra.iter().find(|(_, g)| compare(fc.normal, g.polygons[pi].nodes[ci].normal) == Diff::Same);
                            *t.extra_hits.entry(hit.map_or("none".to_string(), |(name, _)| name.clone())).or_default() += 1;
                        }
                        *t.unexplained.entry(d).or_default() += 1;
                        let p = &results[0].polygons[pi];
                        let c = &p.nodes[ci];
                        let star = &results[0].vertices[c.vertex as usize].star;
                        // why? predicates that can be tested, in this order
                        let open = star.iter().any(|s| s.polygon == -1);
                        let kept = p.flag & 0x400 != 0;
                        let len = fc.normal.iter().map(|&v| v as f64 * v as f64).sum::<f64>().sqrt();
                        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4);
                        let place = if len == 0.0 {
                            "the stored normal is zero".to_string()
                        } else if (len - 1.0).abs() > 1e-4 {
                            "the stored normal is not of length 1".to_string()
                        } else if d <= Diff::Small {
                            "within 1e-5 of the port's value".to_string()
                        } else {
                            // is it the normalised sum of some run of polygon normals around the vertex?
                            let polys: Vec<i32> = star.iter().map(|s| s.polygon).filter(|&q| q != -1).collect();
                            let k = polys.len();
                            let mut run = false;
                            'outer: for from in 0..k {
                                let mut sum = [0f32; 3];
                                for l in 0..k {
                                    let q = &results[0].polygons[polys[(from + l) % k] as usize];
                                    for c in 0..3 {
                                        sum[c] += q.normal[c];
                                    }
                                    let sl = sum.iter().map(|&v| v * v).sum::<f32>().sqrt();
                                    if sl > 1e-6 && near(fc.normal, [sum[0] / sl, sum[1] / sl, sum[2] / sl]) {
                                        run = true;
                                        break 'outer;
                                    }
                                }
                            }
                            // or the whole fan weighted by polygon area (the unnormalised Newell vectors)?
                            let mut area = [0f64; 3];
                            for &q in &polys {
                                let nodes = &results[0].polygons[q as usize].nodes;
                                let pts: Vec<[f32; 3]> = nodes.iter().filter(|c| c.vertex >= 0).map(|c| results[0].vertices[c.vertex as usize].pos).collect();
                                for i in 0..pts.len() {
                                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                                    area[0] += (a[1] as f64 - b[1] as f64) * (a[2] as f64 + b[2] as f64);
                                    area[1] += (a[2] as f64 - b[2] as f64) * (a[0] as f64 + b[0] as f64);
                                    area[2] += (a[0] as f64 - b[0] as f64) * (a[1] as f64 + b[1] as f64);
                                }
                            }
                            let al = (area[0] * area[0] + area[1] * area[1] + area[2] * area[2]).sqrt();
                            let weighted = al > 1e-12 && near(fc.normal, [(area[0] / al) as f32, (area[1] / al) as f32, (area[2] / al) as f32]);
                            if run {
                                format!("the normalised sum of another run of the polygons around the vertex (other discontinuities){}", if m.flags & 2 != 0 { ", faceted mesh" } else { "" })
                            } else if weighted {
                                "the area-weighted average of the polygons around the vertex".to_string()
                            } else {
                                format!("no run of the present polygon normals gives it{}", if m.flags & 2 != 0 { ", faceted mesh" } else { "" })
                            }
                        };
                        let _ = (open, kept);
                        *t.unexplained_place.entry((d, place)).or_default() += 1;
                        if !open && !kept && d <= Diff::Small {
                            // a smooth closed fan? then try other ways of summing it
                            let r = &results[4]; // polygon normals under double evaluation
                            let st = &r.vertices[c.vertex as usize].star;
                            if st.iter().all(|s| r.edges[s.edge.unsigned_abs() as usize].flag & 0x38 == 0) {
                                t.fan_tried += 1;
                                for (ev, rr) in [("x87", &results[0]), ("double", &results[4]), ("single", &results[2])] {
                                    let normals: Vec<[f32; 3]> = st.iter().map(|s| rr.polygons[s.polygon as usize].normal).collect();
                                    let mut seen = std::collections::BTreeSet::new();
                                    for (name, v) in fan_candidates(&normals) {
                                        if compare(fc.normal, v) == Diff::Same && seen.insert(name.clone()) {
                                            *t.fan_hits.entry(format!("polygon normals {ev}; {name}")).or_default() += 1;
                                        }
                                    }
                                }
                            }
                        }
                        if detail && shown < 30 {
                            shown += 1;
                            let fl: Vec<String> = star.iter().map(|s| format!("{}:{:#x}:{}", s.vertex, results[0].edges[s.edge.unsigned_abs() as usize].flag & 0x38, s.polygon)).collect();
                            println!("  node {:?} polygon {pi} corner {ci} vertex {}: file {:?} x87 {:?} single {:?} ({d:?}) polygon normal {:?} flag {:#x} star [{}]",
                                     String::from_utf8_lossy(&n.name), c.vertex, fc.normal, c.normal, results[2].polygons[pi].nodes[ci].normal, p.normal, p.flag, fl.join(" "));
                        }
                    }
                }
                t.first_polygon[poly_first] += 1;
                mesh_first = mesh_first.max(poly_first);
            }
            t.first_mesh[mesh_first] += 1;
            file_first = file_first.max(mesh_first);
        }
    }
    let v = format!("{version}");
    if file_has_stored {
        t.stored.models += 1;
        let e = t.by_version.entry(v).or_default();
        e.0 += 1;
        for vi in 0..6 {
            if file_variant[vi] {
                t.variant[vi].models += 1;
            }
        }
        if file_variant[0] {
            e.1 += 1;
        }
        if file_variant[2] {
            e.2 += 1;
        }
        t.first_model[file_first] += 1;
        class = Some(FIRST[file_first].to_string());
    }
    if file_computed {
        t.computed.models += 1;
    }
    Some(class)
}

/// Candidate ways of computing a polygon normal, tried against faceted meshes, whose stored
/// corner normals are the writer's polygon normals. A diagnostic, not a port.
fn candidates(pos: &[[f32; 3]]) -> Vec<(&'static str, [f32; 3])> {
    let n = pos.len();
    let mut out = Vec::new();
    // Newell, float terms, float sums
    {
        let (mut sx, mut sy, mut sz) = (0f32, 0f32, 0f32);
        let mut a = pos[n - 1];
        for &b in pos {
            sx += (a[1] - b[1]) * (a[2] + b[2]);
            sy += (a[2] - b[2]) * (a[0] + b[0]);
            sz += (a[0] - b[0]) * (a[1] + b[1]);
            a = b;
        }
        let len = ((sx * sx + sy * sy + sz * sz) as f64).sqrt() as f32;
        out.push(("newell, float terms and sums, float length", [sx / len, sy / len, sz / len]));
        let len = (sx as f64 * sx as f64 + sy as f64 * sy as f64 + sz as f64 * sz as f64).sqrt();
        out.push(("newell, float terms and sums, double length", [(sx as f64 / len) as f32, (sy as f64 / len) as f32, (sz as f64 / len) as f32]));
    }
    // Newell in doubles throughout, current corner first
    {
        let d = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
        let (mut sx, mut sy, mut sz) = (0f64, 0f64, 0f64);
        for i in 0..n {
            let (a, b) = (d(pos[i]), d(pos[(i + 1) % n]));
            sx += (a[1] - b[1]) * (a[2] + b[2]);
            sy += (a[2] - b[2]) * (a[0] + b[0]);
            sz += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let len = (sx * sx + sy * sy + sz * sz).sqrt();
        out.push(("newell, doubles, starting at corner 0", [(sx / len) as f32, (sy / len) as f32, (sz / len) as f32]));
        let lf = len as f32;
        out.push(("newell, doubles, float length", [(sx / lf as f64) as f32, (sy / lf as f64) as f32, (sz / lf as f64) as f32]));
        let (fx, fy, fz) = (sx as f32, sy as f32, sz as f32);
        let len = ((fx * fx + fy * fy + fz * fz) as f64).sqrt() as f32;
        out.push(("newell, doubles, stored to float, float length", [fx / len, fy / len, fz / len]));
        let inv = 1.0 / len;
        out.push(("newell, doubles, stored to float, times float reciprocal", [fx * inv, fy * inv, fz * inv]));
    }
    // cross product of the first two edges
    for (name, i0, i1, i2) in [("cross (1-0)x(2-0)", 0, 1, 2), ("cross (1-0)x(last-0)", 0, 1, n - 1), ("cross (2-1)x(0-1)", 1, 2, 0)] {
        let d = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
        let (a, b, c) = (d(pos[i0]), d(pos[i1]), d(pos[i2]));
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let nrm = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt();
        out.push((name, [(nrm[0] / len) as f32, (nrm[1] / len) as f32, (nrm[2] / len) as f32]));
    }
    out
}

/// Polygon normals by Newell's sums with the terms computed in single precision: `sums`
/// says whether the three sums are floats too, `float_len` whether the length comes from
/// float squares. A diagnostic for old files, not a port.
fn float_polygon_normals(m: &mut Mesh, float_sums: bool, float_len: bool) {
    for pi in 0..m.polygons.len() {
        let nodes = &m.polygons[pi].nodes;
        let nb = nodes.len();
        let (mut d, mut f) = ([0f64; 3], [0f32; 3]);
        let mut start = 0;
        for i in 0..=nb {
            if i == nb || nodes[i].vertex == -1 {
                if i > start {
                    let mut a = m.vertices[nodes[i - 1].vertex as usize].pos;
                    for node in &nodes[start..i] {
                        let b = m.vertices[node.vertex as usize].pos;
                        let t = [(a[1] - b[1]) * (a[2] + b[2]), (a[2] - b[2]) * (a[0] + b[0]), (a[0] - b[0]) * (a[1] + b[1])];
                        for c in 0..3 {
                            d[c] += t[c] as f64;
                            f[c] += t[c];
                        }
                        a = b;
                    }
                }
                start = i + 1;
            }
        }
        let s = if float_sums { [f[0] as f64, f[1] as f64, f[2] as f64] } else { d };
        let normal = if float_len {
            let (x, y, z) = (s[0] as f32, s[1] as f32, s[2] as f32);
            let len = (x * x + y * y + z * z).sqrt();
            if len as f64 > 1e-6 { [x / len, y / len, z / len] } else { [0.0; 3] }
        } else {
            let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
            if len > 1e-6 { [(s[0] / len) as f32, (s[1] / len) as f32, (s[2] / len) as f32] } else { [0.0; 3] }
        };
        m.polygons[pi].normal = normal;
    }
}

/// Ways an older or other build might have turned the polygon normals around a vertex into a
/// corner normal, tried on smooth closed fans that the port does not reproduce. A
/// diagnostic, not a port.
fn fan_candidates(normals: &[[f32; 3]]) -> Vec<(String, [f32; 3])> {
    let mut out = Vec::new();
    let n = normals.len();
    let d3 = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
    let finish = |name: String, s: [f32; 3], out: &mut Vec<(String, [f32; 3])>| {
        // the port's: float length, float division
        let len = ((s[1] as f64 * s[1] as f64 + s[0] as f64 * s[0] as f64) + s[2] as f64 * s[2] as f64).sqrt();
        let lf = len as f32;
        out.push((format!("{name}; float length, divide"), [s[0] / lf, s[1] / lf, s[2] / lf]));
        out.push((format!("{name}; double length, divide in double"), [(s[0] as f64 / len) as f32, (s[1] as f64 / len) as f32, (s[2] as f64 / len) as f32]));
        let inv = 1.0f32 / lf;
        out.push((format!("{name}; float length, times float reciprocal"), [s[0] * inv, s[1] * inv, s[2] * inv]));
        let invd = 1.0 / len;
        out.push((format!("{name}; times double reciprocal"), [(s[0] as f64 * invd) as f32, (s[1] as f64 * invd) as f32, (s[2] as f64 * invd) as f32]));
        let lenf = ((s[1] * s[1] + s[0] * s[0]) + s[2] * s[2]).sqrt();
        out.push((format!("{name}; float squares, float length, divide"), [s[0] / lenf, s[1] / lenf, s[2] / lenf]));
    };
    for rot in 0..n {
        for rev in [false, true] {
            let order: Vec<usize> = (0..n).map(|k| if rev { (rot + n - k) % n } else { (rot + k) % n }).collect();
            let mut s = [0f32; 3];
            let mut sd = [0f64; 3];
            for &k in &order {
                for c in 0..3 {
                    s[c] += normals[k][c];
                    sd[c] += d3(normals[k])[c];
                }
            }
            let tag = if rot == 0 && !rev { "star order".to_string() } else { "another order".to_string() };
            finish(format!("float sums, {tag}"), s, &mut out);
            let len = (sd[0] * sd[0] + sd[1] * sd[1] + sd[2] * sd[2]).sqrt();
            out.push((format!("double sums, {tag}; double length"), [(sd[0] / len) as f32, (sd[1] / len) as f32, (sd[2] / len) as f32]));
        }
    }
    out
}

fn experiment(dir: &str) -> i32 {
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    let mut total = 0u64;
    let mut hits: BTreeMap<String, u64> = BTreeMap::new();
    let mut none = 0u64;
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        let Some(f) = read_file(&data, Kind::Model) else { continue };
        let Content::Model(root) = &f.content else { continue };
        let mut nodes = Vec::new();
        root.walk(0, &mut |_, n| nodes.push(n));
        for n in nodes {
            for d in &n.definitions {
                let Definition::Mesh(fm) = d else { continue };
                if fm.flag & 2 == 0 || fm.compute_normals {
                    continue;
                }
                for eval in [Eval::X87, Eval::Single, Eval::Double] {
                    let mut m = mesh_from_file(fm);
                    mesh_b::computes_polygon_normal_with(&mut m, -1, eval);
                    for (fp, p) in fm.polygons.iter().zip(&m.polygons) {
                        if fp.nodes.is_empty() || fp.nodes.iter().any(|c| c.vertex < 0) {
                            continue;
                        }
                        let stored = fp.nodes[0].normal;
                        if eval == Eval::X87 {
                            total += 1;
                            let pos: Vec<[f32; 3]> = fp.nodes.iter().map(|c| fm.vertices[c.vertex as usize].pos).collect();
                            let mut any = compare(stored, p.normal) == Diff::Same;
                            for (name, v) in candidates(&pos) {
                                if compare(stored, v) == Diff::Same {
                                    *hits.entry(name.to_string()).or_default() += 1;
                                    any = true;
                                }
                            }
                            if !any {
                                none += 1;
                            }
                        }
                        if compare(stored, p.normal) == Diff::Same {
                            *hits.entry(format!("the port's routine under {eval:?}")).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    println!("polygons of faceted meshes with stored normals: {total}; reproduced by no candidate (x87, or the listed): {none}");
    for (k, c) in &hits {
        println!("  {c:>8}  {k}");
    }
    0
}

/// Tests the guess that the stored normals which lie within 1e-5 of the port's were carried
/// through a transformation (`SI_meshTransform`) instead of being recomputed: every mesh the
/// port reproduces bit for bit is moved by a made-up matrix with its normals carried, the
/// normals are then recomputed from the moved geometry, and the two are compared with the
/// same classes as the corpus comparison.
fn carry_experiment(dir: &str) -> i32 {
    use si_math::algebra::{matrix_scal_rot_trans, matrix_transpose_inverse};
    use si_math::types::Vector;
    let moves: [(&str, [f32; 3], [f32; 3], [f32; 3]); 4] = [
        ("translation only", [1.0; 3], [0.0; 3], [1.5, -2.25, 0.75]),
        ("rotation and translation", [1.0; 3], [0.3, -1.1, 2.0], [1.5, -2.25, 0.75]),
        ("uniform scaling 2.5", [2.5; 3], [0.0; 3], [0.0; 3]),
        ("scaling (2, 1, 0.5) and rotation", [2.0, 1.0, 0.5], [0.3, -1.1, 2.0], [0.0; 3]),
    ];
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    let mut meshes = [0u64; 2];
    let mut tally: BTreeMap<(usize, bool, Diff), u64> = BTreeMap::new();
    let mut env = MeshEnv::default();
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        let Some(f) = read_file(&data, Kind::Model) else { continue };
        let Content::Model(root) = &f.content else { continue };
        let mut nodes = Vec::new();
        root.walk(0, &mut |_, n| nodes.push(n));
        for n in nodes {
            for d in &n.definitions {
                let Definition::Mesh(fm) = d else { continue };
                if fm.compute_normals || fm.polygons.is_empty() {
                    continue;
                }
                let mut m = mesh_from_file(fm);
                if m.flags & 0x800 == 0 && m.flags & 0xa == 0 {
                    continue;
                }
                finish_read(&mut m, &fm.edges, false, None, &mut env);
                mesh_b::compute_hard_edge(&mut m, -1);
                mesh_b::computes_vertex_normal(&mut m, -1);
                if compare_normals(fm, &m).worst.is_some() {
                    continue; // only meshes the port reproduces exactly
                }
                let carries = m.flags & 4 != 0;
                meshes[carries as usize] += 1;
                for (k, (_, s, r, t)) in moves.iter().enumerate() {
                    let v = |a: &[f32; 3]| Vector::new(a[0], a[1], a[2]);
                    let mat = matrix_scal_rot_trans(&v(s), &v(r), &v(t));
                    let (inv, _) = matrix_transpose_inverse(&mat);
                    let mut moved = m.clone();
                    si_model::obj_global::mesh_transform(&mut moved, &mat, &inv);
                    let mut fresh = moved.clone();
                    mesh_b::computes_polygon_normal(&mut fresh, -1);
                    mesh_b::compute_hard_edge(&mut fresh, -1);
                    mesh_b::computes_vertex_normal(&mut fresh, -1);
                    for (a, b) in moved.polygons.iter().zip(&fresh.polygons) {
                        for (ca, cb) in a.nodes.iter().zip(&b.nodes) {
                            if ca.vertex != -1 {
                                *tally.entry((k, carries, compare(ca.normal, cb.normal))).or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    println!("meshes the port reproduces bit for bit: {} with mesh flag 0x4 (normals carried), {} without (normals left)", meshes[1], meshes[0]);
    println!("corners, carried normal against the normal recomputed after the move:");
    for (k, (name, ..)) in moves.iter().enumerate() {
        for carries in [true, false] {
            let row: Vec<String> = [Diff::Same, Diff::ZeroSign, Diff::OneUlp, Diff::Small, Diff::Other]
                .iter()
                .map(|d| format!("{d:?} {}", tally.get(&(k, carries, *d)).copied().unwrap_or(0)))
                .collect();
            println!("  {name:<34} {}: {}", if carries { "flag 0x4   " } else { "no flag 0x4" }, row.join(", "));
        }
    }
    0
}

/// Every polygon of every mesh of the corpus through the port of the triangulator
/// (`si_model::triangulate`), each mesh loaded as the program loads it and each polygon
/// prepared as `SI_meshTriangulate` prepares it (`polygon_points`: its distinct vertices
/// turned into the XY plane by `SI_matrixZAlign` of its stored normal).
fn triangulate_pass(dir: &str) -> i32 {
    use si_model::obj_global::mesh_nb_triangles;
    use si_model::triangulate::{polygon_points, triangulate_contour};
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    let (mut nfiles, mut unread, mut meshes, mut polygons) = (0u64, 0u64, 0u64, 0u64);
    let mut by_corners: BTreeMap<String, [u64; 4]> = BTreeMap::new(); // polygons, with holes, triangles, refused
    let (mut triangles, mut refused, mut too_few, mut hidden) = (0u64, 0u64, 0u64, 0u64);
    let (mut repeats, mut fewer) = (0u64, 0u64);
    let (mut flat, mut backward, mut no_normal, mut backward_polygons) = (0u64, 0u64, 0u64, 0u64);
    let (mut mesh_same, mut mesh_other, mut mesh_same_visible) = (0u64, 0u64, 0u64);
    let mut other_examples: Vec<String> = Vec::new();
    let mut refused_examples: Vec<String> = Vec::new();
    let mut env = MeshEnv::default();
    for path in &files {
        let Ok(data) = std::fs::read(path) else { continue };
        nfiles += 1;
        let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let Some(f) = read_file(&data, Kind::Model) else {
            unread += 1;
            continue;
        };
        let Content::Model(root) = &f.content else {
            unread += 1;
            continue;
        };
        let mut nodes = Vec::new();
        root.walk(0, &mut |_, n| nodes.push(n));
        for n in nodes {
            for d in &n.definitions {
                let Definition::Mesh(fm) = d else { continue };
                meshes += 1;
                let mut m = mesh_from_file(fm);
                env.messages.clear();
                finish_read(&mut m, &fm.edges, fm.compute_normals, Some(&n.name), &mut env);
                let (mut all, mut visible) = (0i64, 0i64);
                for p in 0..m.polygons.len() {
                    polygons += 1;
                    let poly = &m.polygons[p];
                    let records = poly.nodes.len();
                    let seps = poly.nodes.iter().filter(|c| c.vertex == -1).count();
                    let key = match records - seps {
                        0..=2 => "fewer than 3".to_string(),
                        3 => "3".to_string(),
                        4 => "4".to_string(),
                        5..=8 => "5 to 8".to_string(),
                        9..=16 => "9 to 16".to_string(),
                        17..=64 => "17 to 64".to_string(),
                        _ => "more than 64".to_string(),
                    };
                    let row = by_corners.entry(key).or_default();
                    row[0] += 1;
                    row[1] += (seps > 0) as u64;
                    let is_hidden = poly.flag & 0x800 != 0;
                    hidden += is_hidden as u64;
                    if records < 3 {
                        too_few += 1;
                        continue;
                    }
                    if poly.normal == [0.0; 3] {
                        no_normal += 1;
                    }
                    let mut pp = polygon_points(&m, p);
                    let before = pp.indices.clone();
                    let (ok, tris) = triangulate_contour(&mut pp.points, &mut pp.indices);
                    if !ok {
                        refused += 1;
                        row[3] += 1;
                        if refused_examples.len() < 5 {
                            refused_examples.push(format!("{rel}: {} polygon {p} ({} corners, {} holes)", String::from_utf8_lossy(&n.name), records - seps, seps));
                        }
                        continue;
                    }
                    triangles += tris.len() as u64;
                    row[2] += tris.len() as u64;
                    all += tris.len() as i64;
                    if !is_hidden {
                        visible += tris.len() as i64;
                    }
                    let expected = records as i64 - 2 + seps as i64;
                    if (tris.len() as i64) < expected {
                        fewer += 1;
                    }
                    let mut last = *before.last().unwrap();
                    if before.iter().any(|&i| {
                        let same = i == last;
                        last = i;
                        same
                    }) {
                        repeats += 1;
                    }
                    // triangles that are not counter-clockwise in the polygon's plane
                    let mut any_back = false;
                    for t in &tris {
                        let q = |k: usize| pp.points[t[k] as usize];
                        let (a, b, c) = (q(0), q(1), q(2));
                        let area = (b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64) - (c[0] as f64 - a[0] as f64) * (b[1] as f64 - a[1] as f64);
                        if area == 0.0 {
                            flat += 1;
                        } else if area < 0.0 {
                            backward += 1;
                            any_back = true;
                        }
                    }
                    backward_polygons += any_back as u64;
                }
                let stated = mesh_nb_triangles(&m) as i64;
                if stated == visible {
                    mesh_same_visible += 1;
                }
                let stated_all: i64 = m.polygons.iter().map(|p| p.nodes.len() as i64 - 2 + p.nodes.iter().filter(|c| c.vertex == -1).count() as i64).sum();
                if stated_all == all {
                    mesh_same += 1;
                } else {
                    mesh_other += 1;
                    if other_examples.len() < 5 {
                        other_examples.push(format!("{rel}: {} counts {stated_all}, triangulated {all}", String::from_utf8_lossy(&n.name)));
                    }
                }
            }
        }
    }
    println!("files {nfiles}  not read {unread}  meshes {meshes}  polygons {polygons}");
    println!("\npolygons by corner count (polygons, of them with holes, triangles produced, refused):");
    for key in ["fewer than 3", "3", "4", "5 to 8", "9 to 16", "17 to 64", "more than 64"] {
        if let Some(r) = by_corners.get(key) {
            println!("  {key:>12}: {:>8} {:>6} {:>8} {:>4}", r[0], r[1], r[2], r[3]);
        }
    }
    println!("\ntriangles produced: {triangles}");
    println!("polygons with fewer than three corner records (not triangulated): {too_few}");
    println!("polygons the routine refuses (a hole that cannot be joined): {refused}");
    for e in &refused_examples {
        println!("    {e}");
    }
    println!("polygons with a corner that repeats the one before it: {repeats}; polygons with fewer triangles than corners + 2 x holes - 2: {fewer}");
    println!("hidden polygons (flag 0x800): {hidden}; polygons without a stored normal: {no_normal}");
    println!("triangles of no area in the polygon's plane: {flat}; clockwise there: {backward} (in {backward_polygons} polygons)");
    println!("\nper mesh, triangles produced against the count of corners - 2 + 2 x holes over all polygons:");
    println!("  the same: {mesh_same}; different: {mesh_other}");
    for e in &other_examples {
        println!("    {e}");
    }
    println!("per mesh, triangles of the visible polygons against SI_meshNbTriangles: the same in {mesh_same_visible} of {meshes}");
    0
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--triangulate") && args.len() > 1 {
        return triangulate_pass(&args[1]);
    }
    if args.first().map(String::as_str) == Some("--carry-experiment") && args.len() > 1 {
        return carry_experiment(&args[1]);
    }
    if args.first().map(String::as_str) == Some("--polygon-experiment") && args.len() > 1 {
        return experiment(&args[1]);
    }
    let list = args.iter().any(|a| a == "--list");
    let mut t = Tally::default();
    if args.first().map(String::as_str) == Some("--one") && args.len() > 1 {
        let Ok(data) = std::fs::read(&args[1]) else { return 2 };
        check_file(&data, &mut t, true);
        report(&t);
        return 0;
    }
    let Some(dir) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: model-check [--list] <dir> | --one <file>");
        return 2;
    };
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        t.files += 1;
        let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
        match check_file(&data, &mut t, false) {
            None => t.unread += 1,
            Some(class) => {
                if let Some(class) = class {
                    if list && !class.starts_with("the port, from") {
                        println!("{class}\t{rel}");
                    }
                    let e = t.model_classes.entry(class).or_insert((0, rel.clone()));
                    e.0 += 1;
                }
            }
        }
    }
    report(&t);
    0
}

fn report(t: &Tally) {
    println!("files {}  not read {}  nodes {}  meshes {}  polygons {}  corners {}", t.files, t.unread, t.nodes, t.meshes, t.polygons, t.corners);
    println!("\nthe load (SI_meshUpdateNodes):");
    println!("  meshes with a polygon that could not be filed: {}", t.not_filed);
    for (k, c) in &t.messages {
        println!("    collision messages ({k}): {c}");
    }
    println!("  corners naming a vertex outside the array: {}; contours of one corner: {}; contour separators: {}", t.bad_vertex, t.single_corner_contours, t.holes);
    println!("  mesh flags: {}", t.mesh_flags.iter().map(|(k, c)| format!("{k:#x}: {c}")).collect::<Vec<_>>().join(", "));
    println!("  listed edge flags: {}", t.edge_flags_file.iter().map(|(k, c)| format!("{:#x}: {c}", *k as u16)).collect::<Vec<_>>().join(", "));
    println!("\nedges, rebuilt against listed ({} meshes with a list, {} edges listed, {} rebuilt):", t.with_edge_list, t.edges_listed, t.edges_rebuilt);
    println!("  same count {}  same edges in the same order and direction {}  same set of vertex pairs {}", t.edge_count_same, t.edge_order_same, t.edge_set_same);
    println!("  listed edges that join no two corners: {}", t.edges_unmapped);
    for (k, (c, _)) in &t.edge_classes {
        println!("    {k}: {c}");
    }
    println!("\nborder and discontinuity bits (0x38) of the listed edges, computed against stored ({} per evaluation):", t.hard_edges_compared);
    for ((eval, k), c) in &t.hard_bits {
        println!("    {eval:?}: {k}: {c}");
    }
    println!("  the same by mesh flag (x87):");
    for ((flag, k), c) in &t.hard_by_flag {
        println!("    {flag:#x}: {k}: {c}");
    }
    println!("\nstored normals:");
    println!("  compared               {}", t.stored);
    println!("  not automatic (skipped){}", t.not_automatic);
    println!("  bit-identical throughout, per computation:");
    for (vi, name) in VARIANT.iter().enumerate() {
        println!("    {name:<40} {}", t.variant[vi]);
    }
    println!("  x87 from the geometry, corners by difference: {:?}", t.corner_diff_geometry);
    println!("  the first computation that reproduces it (corners, whole polygons, whole meshes, whole models):");
    for (vi, name) in FIRST.iter().enumerate() {
        println!("    {name:<52} {:>9} {:>8} {:>6} {:>6}", t.first_corner[vi], t.first_polygon[vi], t.first_mesh[vi], t.first_model[vi]);
    }
    println!("  corners none reproduces, by distance from the port's value: {:?}", t.unexplained);
    let mut causes: BTreeMap<&str, u64> = BTreeMap::new();
    for ((_, place), c) in &t.unexplained_place {
        *causes.entry(place.as_str()).or_default() += c;
    }
    for (place, c) in &causes {
        println!("    {c:>8}  {place}");
    }
    println!("  corners within 1e-5 that none reproduces, against polygon normals computed in single precision (first that reproduces):");
    for (k, c) in &t.extra_hits {
        println!("    {c:>8}  {k}");
    }
    println!("  corners by file version: the port | another of the six | none: zero sign, one ulp, up to 1e-5, more");
    for (v, c) in &t.corners_by_version {
        println!("    {v:>8}: {:>8} | {:>7} | {:>5} {:>6} {:>6} {:>7}", c[0], c[1], c[3], c[4], c[5], c[6]);
    }
    println!("  smooth closed fans among them (differences up to 1e-5) tried against other ways of summing: {}", t.fan_tried);
    for (k, c) in &t.fan_hits {
        println!("    {c:>8}  {k}");
    }
    println!("  models by the weakest computation that reproduces all of them:");
    for (k, (c, example)) in &t.model_classes {
        println!("    {c:>5}  {k}   e.g. {example}");
    }
    println!("  by file version (models compared, identical by the port from the geometry, identical by single-precision evaluation):");
    for (v, (n, same, single)) in &t.by_version {
        println!("    {v:>8}: {n:>5} {same:>5} {single:>5}");
    }
    println!("\nfiles without stored normals, completed as the program does:");
    println!("  {}", t.computed);
    println!("  corner normals of length 1: {}  zero: {}  other: {}", t.computed_unit, t.computed_zero, t.computed_other);
    println!("  meshes by (file version, mesh flag): {}", t.computed_flags.iter().map(|((v, f), c)| format!("{v}/{f:#x}: {c}")).collect::<Vec<_>>().join(", "));
}
