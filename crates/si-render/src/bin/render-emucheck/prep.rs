// SPDX-License-Identifier: MIT
//! Checks of scene preparation on the vectors of `re/tools/si-render/emu/gen_prep.py`: the
//! BSP tree build (`si_initAncestry`, `si_bsptree`, `xSplit`, `ySplit`, `zSplit`).
//!
//! A tree vector gives the tree the original built as numbers (the generator's `tree_post`
//! walks the emulator's memory): every node made, in allocation order, ten words each; every
//! list entry, eight words each; the three counters; the bytes of chunk memory used. The
//! port's tree is written out the same way and compared word for word.
use crate::{bits, fl, verdict};
use si_math::geometry::{RVertex, Tn};
use si_math::types::Vector;
use si_math::geometry::ClipBox;
use si_math::matrix::RenderCamera;
use si_render::bsptree::{init_ancestry, BspBuild, BspParams, BspStats};
use si_render::camera::{camera, Camera};
use si_render::geometry::{free_model, process, set_existance, trans_form, Geometry, GeometryEnv, RenModel};
use si_render::initialize::init_data;
use si_render::update::{init_buffers, init_filter, init_viewport, set_bounds, update, Options, Prepared, Settings, UpdateEnv, View};
use si_render::shade::ShadeState;
use si_render::trace::{Bsp, Link, Tree};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

/// List entries and triangles a tree vector has room for (`NL` of the generator).
const NL: usize = 32;

fn offset_in(w: &Word, buffer: &str, size: u32) -> Option<usize> {
    match w {
        Word::Ref { name, offset } if name == buffer => Some((*offset / size) as usize),
        _ => None,
    }
}

/// With `EMUCHECK_MUTATE` set, one bit of every word the port produces is flipped before it
/// is compared: every compared case must then differ.
pub fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

pub fn same_words(what: &str, original: &Field, port: &[u32]) -> Result<(), String> {
    let w = original.words();
    if w.len() != port.len() {
        return Err(format!("{what}: original {} words, port {}", w.len(), port.len()));
    }
    for (i, (o, p)) in w.iter().zip(port).enumerate() {
        let p = if mutate() { *p ^ 1 } else { *p };
        match o {
            Word::Value { value, .. } if *value as u32 == p => {}
            // a word the original never wrote has no value to compare
            Word::Unwritten { .. } if !mutate() => {}
            _ => return Err(format!("{what} word {i}: original {o:?}, port {p:08x}")),
        }
    }
    Ok(())
}

/// The tree a vector starts from: the outside node, the six walls given, the list entries,
/// the triangles and their corners, the settings and counters.
struct TreeCase {
    bsp: Bsp,
    tns: Vec<Tn>,
    verts: Vec<RVertex>,
    params: BspParams,
    stats: BspStats,
    bound: [f32; 6],
    anc: [Option<usize>; 6],
    list: Option<usize>,
}

fn tree_case(c: &Case) -> TreeCase {
    let mut bsp = Bsp::default();
    bsp.nodes.push(Tree { kind: bits(c.input("out"), 0) as i16, ..Tree::default() });
    bsp.outside = Some(0);
    for i in 0..6 {
        let f = c.input(&format!("w{i}"));
        bsp.nodes.push(Tree { kind: bits(f, 0) as i16, level: bits(f, 1) as i16, split: fl(f, 3), low: Some(0), high: Some(0), ..Tree::default() });
    }
    let l = c.input("links");
    let (mut tns, mut verts) = (Vec::new(), Vec::new());
    let v = c.input("verts");
    for k in 0..NL {
        let w = &l.words()[8 * k..8 * k + 8];
        bsp.links.push(Link {
            bound: core::array::from_fn(|i| fl(l, 8 * k + i)),
            tn: offset_in(&w[6], "tns", 0x60).unwrap_or(0),
            next: offset_in(&w[7], "links", 0x20),
        });
        tns.push(Tn { v: [Some(3 * k), Some(3 * k + 1), Some(3 * k + 2)], ..Tn::default() });
        for j in 0..3 {
            let at = 19 * (3 * k + j) + 8;
            verts.push(RVertex { world: Vector::new(fl(v, at), fl(v, at + 1), fl(v, at + 2)), ..RVertex::default() });
        }
    }
    let e = c.input("g_empty").f32s();
    let s = c.input("g_stats").i32s();
    TreeCase {
        bsp,
        tns,
        verts,
        params: BspParams {
            min_count: c.input("g_min").i16(),
            max_level: c.input("g_max").i16(),
            cell_volume: c.input("g_cv").f32(),
            empty_fraction: c.input("g_frac").f32(),
            empty_box: core::array::from_fn(|i| e[i]),
        },
        stats: BspStats { links: s[0], nodes: s[1], leaves: s[2] },
        bound: core::array::from_fn(|i| fl(c.input("bound"), i)),
        anc: core::array::from_fn(|i| offset_in(&c.input("anc").words()[i], &format!("w{i}"), 1).map(|_| 1 + i)),
        list: offset_in(&c.input("list").words()[0], "links", 0x20),
    }
}

/// The port's tree in the generator's numbering: nodes 1 (outside), 2..7 (the walls given),
/// 8.. (made); list entries from 1.
fn same_tree(c: &Case, t: &TreeCase, root: usize) -> Result<(), String> {
    let id = |n: Option<usize>| n.map_or(0, |i| i as u32 + 1);
    same_words("root", c.output("root"), &[id(Some(root))])?;
    let mut tree = Vec::new();
    let mut used = 0u32;
    for n in &t.bsp.nodes[7..] {
        let head = [n.kind as u16 as u32, n.level as u16 as u32, n.mailbox as u32];
        tree.extend(head);
        if n.kind == 0 {
            tree.push(id(n.list));
            tree.extend(n.walls.iter().map(|w| id(*w)));
            used += 0x24;
        } else {
            tree.extend([n.split.to_bits(), id(n.low), id(n.high), 0, 0, 0, 0]);
            used += 0x14;
        }
    }
    same_words("tree", c.output("tree"), &tree)?;
    let mut links = Vec::new();
    for l in &t.bsp.links {
        links.extend(l.bound.iter().map(|f| f.to_bits()));
        links.extend([l.tn as u32, id(l.next)]);
    }
    used += 0x20 * (t.bsp.links.len() - NL) as u32;
    // the entries given that no list holds keep their triangle word (0 in the unused ones)
    let orig = c.output("links_out");
    if orig.len() != links.len() {
        return Err(format!("list entries: original {}, port {}", orig.len() / 8, links.len() / 8));
    }
    same_words("links", orig, &links)?;
    let s = &t.stats;
    same_words("counters", c.output("g_stats"), &[s.links as u32, s.nodes as u32, s.leaves as u32])?;
    let before = match &c.input("chunk").words()[5] {
        Word::Ref { offset, .. } => *offset,
        _ => 0,
    };
    // chunk memory hands out multiples of four: the first allocation rounds the fill up
    let start = (before + 3) & !3;
    same_words("chunk memory used", c.output("used"), &[if used == 0 { before } else { start + used }])
}

fn check_bsptree(c: &Case) -> Verdict {
    let mut t = tree_case(c);
    let (list, bound, anc) = (t.list, t.bound, t.anc);
    let root = {
        let mut b = BspBuild { bsp: &mut t.bsp, tns: &t.tns, verts: &t.verts, params: t.params, stats: &mut t.stats };
        b.bsptree(list, c.input("count").i32(), &bound, &anc, c.input("level").i32())
    };
    verdict(same_tree(c, &t, root))
}

fn check_split(c: &Case, axis: usize) -> Verdict {
    let mut t = tree_case(c);
    let (list, bound, anc) = (t.list, t.bound, t.anc);
    let root = {
        let mut b = BspBuild { bsp: &mut t.bsp, tns: &t.tns, verts: &t.verts, params: t.params, stats: &mut t.stats };
        b.split(axis, list, &bound, &anc, c.input("level").i32())
    };
    verdict(same_tree(c, &t, root))
}

fn check_ancestry(c: &Case) -> Verdict {
    let mut bsp = Bsp::default();
    let mut stats = BspStats { nodes: c.input("g_nodes").i32(), ..BspStats::default() };
    let b = c.input("g_box").f32s();
    let anc = init_ancestry(&mut bsp, &core::array::from_fn(|i| b[i]), &mut stats);
    let id = |n: Option<usize>| n.map_or(0, |i| i as u32 + 1);
    let mut nodes = Vec::new();
    for n in &bsp.nodes {
        nodes.extend([n.kind as u16 as u32, n.level as u16 as u32, n.mailbox as u32, n.split.to_bits(), id(n.low), id(n.high)]);
    }
    let before = match &c.input("chunk").words()[5] {
        Word::Ref { offset, .. } => *offset,
        _ => 0,
    };
    let start = (before + 3) & !3;
    verdict(
        same_words("record", c.output("record"), &anc.map(id))
            .and_then(|_| same_words("outside", c.output("outside"), &[id(bsp.outside)]))
            .and_then(|_| same_words("nodes", c.output("nodes"), &nodes))
            .and_then(|_| same_words("node counter", c.output("g_nodes"), &[stats.nodes as u32]))
            .and_then(|_| same_words("record's place", c.output("rec_at"), &[start]))
            .and_then(|_| same_words("chunk memory used", c.output("used"), &[start + 0x18 + 7 * 0x14])),
    )
}

// ------------------------------------------------------------------ transForm, process

/// Triangles and vertices a geometry vector has room for (`GT`, `GV` of the
/// generator).
const GT: usize = 12;
const GV: usize = 36;

struct GeoCase {
    st: ShadeState,
    g: Geometry,
    node: usize,
    nmap: usize,
    /// Counters as the case starts (`SI_stats`), to tell what the routine added.
    stats0: Vec<i32>,
}

fn name_of(w: &Word, names: &[&str]) -> Option<usize> {
    match w {
        Word::Ref { name, .. } => names.iter().position(|n| n == name),
        _ => None,
    }
}

fn geo_case(c: &Case) -> GeoCase {
    let mut st = ShadeState::default();
    let loaded = crate::outer::Loaded::of(c);
    for name in ["mat0", "mat1"] {
        st.materials.push(si_render::inimage::material_at(&loaded.m, loaded.at(name)));
    }
    let t = c.input("tns");
    for k in 0..GT {
        let w = &t.words()[27 * k..27 * k + 27];
        let f = |i: usize| fl(t, 27 * k + i);
        st.tns.push(Tn {
            v: core::array::from_fn(|i| offset_in(&w[i], "verts", 0x4c)),
            n: Vector::new(f(3), f(4), f(5)),
            d: f(6),
            inv_d: f(7),
            axis: bits(t, 27 * k + 8) as i16,
            inv_n: f(10),
            edge: core::array::from_fn(|e| Vector::new(f(11 + 3 * e), f(12 + 3 * e), f(13 + 3 * e))),
            material: name_of(&w[20], &["mat0", "mat1"]).unwrap_or(0) as u32,
            flags: bits(t, 27 * k + 21) as u16,
            flags2: bits(t, 27 * k + 22) as u16,
            w58: bits(t, 27 * k + 24) as i16,
            next: offset_in(&w[26], "tns", 0x60),
        });
    }
    let v = c.input("verts");
    for k in 0..GV {
        let f = |i: usize| fl(v, 19 * k + i);
        st.verts.push(RVertex {
            w00: [bits(v, 19 * k), bits(v, 19 * k + 1)],
            raw: Vector::new(f(2), f(3), f(4)),
            raw_normal: Vector::new(f(5), f(6), f(7)),
            world: Vector::new(f(8), f(9), f(10)),
            ps: si_math::types::Pspoint { x: f(11), y: f(12) },
            normal: Vector::new(f(13), f(14), f(15)),
            uv: offset_in(&v.words()[19 * k + 16], "uvs", 8).unwrap_or(0) as u32,
            w44: bits(v, 19 * k + 17),
            p48: bits(v, 19 * k + 18),
        });
    }
    st.uvs = c.input("uvs").f32s().chunks(2).map(|p| [p[0], p[1]]).collect();
    let m = c.input("g_mat").f32s();
    let mat = |at: usize| -> si_math::types::Matrix { core::array::from_fn(|i| core::array::from_fn(|j| m[at + 4 * i + j])) };
    let win = c.input("g_win").f32s();
    st.camera = RenderCamera { world_to_raw_mat: mat(0), raw_to_world_mat: mat(16), vect_mat: mat(32), prsp_mat: mat(48), near_z: win[4] };
    let stats0 = c.input("g_stats").i32s();
    let names = ["model", "lat"];
    let registered = name_of(&c.input("elems").words()[1], &names).expect("the dictionary names a node");
    let mut g = Geometry::default();
    for name in names {
        let f = c.input(name);
        let w = f.words();
        g.models.push(RenModel {
            reversed: bits(f, 2) & 8 != 0,
            kind: bits(f, 12) as i32,
            latdep: name_of(&w[17], &names).expect("latdep"),
            gscl: Vector::new(fl(f, 91), fl(f, 92), fl(f, 93)),
            cscl: Vector::new(fl(f, 232), fl(f, 233), fl(f, 234)),
            gmat: core::array::from_fn(|i| core::array::from_fn(|j| fl(f, 113 + 4 * i + j))),
            materials: vec![0, 1],
            flags: bits(f, 133) as u16,
            n_maps: (bits(f, 133) >> 16) as i16,
            vert_count: bits(f, 138) as i32,
            verts: offset_in(&w[139], "verts", 0x4c),
            tn_count: bits(f, 140) as i32,
            tns: offset_in(&w[141], "tns", 0x60),
            tcoords: offset_in(&w[204], "uvs", 8),
            uv2: offset_in(&w[267], "uv2", 8),
            brother: None,
            son: None,
            registered,
        });
    }
    // the window; the two depth members by their addresses (0x087907c0, 0x087907c4)
    g.window = ClipBox { xmin: win[0], xmax: win[1], ymin: win[2], ymax: win[3], yon: win[4], hither: win[5] };
    let psb = c.input("g_psb").f32s();
    g.bins.lines = vec![Vec::new(); bits(c.input("meta"), 1) as usize];
    g.bins.bounds = [psb[0], psb[1], psb[2], psb[3]];
    g.bins.accepted = stats0[0];
    g.bins.made = stats0[8];
    let b = c.input("g_box").f32s();
    g.trace.bounds = core::array::from_fn(|i| b[i]);
    g.trace.made = stats0[9];
    g.made_tns = stats0[7];
    g.clipper.made_vertices = stats0[5];
    g.clipper.made_pairs = stats0[6];
    g.raytrace = bits(c.input("g_rt"), 0) & 1 != 0;
    g.area_sort = bits(c.input("g_sort"), 0) == 0x0827_4dac;
    g.right_hand = c.input("g_rh").i16() != 0;
    g.glb_manip = c.input("g_manip").i32();
    let node = name_of(&c.input("node").words()[0], &names).expect("the node");
    GeoCase { st, g, node, nmap: bits(c.input("meta"), 0) as usize, stats0 }
}

/// Stretches of a pool that are the nodes' own arrays: (first index, length, the number the
/// vector gives the first).
pub type Ranges = Vec<(usize, usize, usize)>;

thread_local! {
    /// Which triangles and vertices of the pools are the nodes' own (a vector names those
    /// by number); the others the clipper made. The arrays of a case by default.
    static POOL: std::cell::RefCell<(Ranges, Ranges)> = std::cell::RefCell::new((vec![(0, GT, 0)], vec![(0, GV, 0)]));
}

thread_local! {
    /// Bytes of chunk memory a case's `SI_update` leaves taken beside what the port's own
    /// counters know: the depth maps of its lights (eighth pass).
    static EXTRA_CHUNK: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

pub fn set_extra_chunk(bytes: i32) {
    EXTRA_CHUNK.with(|b| b.set(bytes));
}

/// See `POOL`: for scenes whose nodes got their arrays from the converters.
pub fn set_pool(tns: Ranges, verts: Ranges) {
    POOL.with(|p| *p.borrow_mut() = (tns, verts));
}

pub fn reset_pool() {
    set_pool(vec![(0, GT, 0)], vec![(0, GV, 0)]);
}

fn place(ranges: &Ranges, i: usize) -> Option<u32> {
    ranges.iter().find(|r| r.0 <= i && i < r.0 + r.1).map(|r| (r.2 + i - r.0) as u32)
}

/// The number a vector gives a triangle of the pool, if it is a node's own.
pub fn tn_place(i: usize) -> Option<u32> {
    POOL.with(|p| place(&p.borrow().0, i))
}

pub fn vert_place(i: usize) -> Option<u32> {
    POOL.with(|p| place(&p.borrow().1, i))
}

fn vert_stream(st: &ShadeState, nmap: usize, v: usize, out: &mut Vec<u32>) {
    if let Some(n) = vert_place(v) {
        out.extend([0, n]);
        return;
    }
    let r = &st.verts[v];
    out.extend([1, 0, r.w00[0], r.w00[1]]);
    out.extend([r.raw.x, r.raw.y, r.raw.z, r.raw_normal.x, r.raw_normal.y, r.raw_normal.z, r.world.x, r.world.y, r.world.z, r.ps.x, r.ps.y, r.normal.x, r.normal.y, r.normal.z].map(f32::to_bits));
    for p in 0..nmap {
        let pair = st.uvs[r.uv as usize + p];
        out.extend([pair[0].to_bits(), pair[1].to_bits()]);
    }
    // (a null pointer at +0x48 is all ones in the converters' vertices)
    out.extend([r.w44, if r.p48 == u32::MAX { 0 } else { r.p48 }]);
}

fn tri_stream(st: &ShadeState, nmap: usize, k: usize, out: &mut Vec<u32>) {
    if let Some(n) = tn_place(k) {
        out.extend([0, n]);
        return;
    }
    let n = &st.tns[k];
    out.extend([1, 0]);
    out.extend([n.n.x, n.n.y, n.n.z, n.d, n.inv_d].map(f32::to_bits));
    out.push(n.axis as u16 as u32);
    out.push(n.inv_n.to_bits());
    for e in n.edge {
        out.extend([e.x, e.y, e.z].map(f32::to_bits));
    }
    out.extend([n.material, n.flags as u32, n.flags2 as u32, n.w58 as u16 as u32, n.next.map_or(0, |_| 1)]);
    for v in n.v {
        vert_stream(st, nmap, v.expect("a triangle the clipper made has three corners"), out);
    }
}

fn same_text(c: &Case, messages: &[&'static str]) -> Result<(), String> {
    let port: String = messages.concat();
    let original = c.try_output("stdout").map(|f| String::from_utf8_lossy(&f.to_bytes()).into_owned()).unwrap_or_default();
    if port != original {
        return Err(format!("printed: original {original:?}, port {port:?}"));
    }
    Ok(())
}

fn same_matrices(c: &Case, cam: &RenderCamera) -> Result<(), String> {
    let mut port = Vec::new();
    for m in [&cam.world_to_raw_mat, &cam.raw_to_world_mat, &cam.vect_mat, &cam.prsp_mat] {
        port.extend(m.iter().flatten().map(|f| f.to_bits()));
    }
    same_words("the four matrices", c.after("g_mat"), &port)
}

fn check_transform(c: &Case) -> Verdict {
    let mut t = geo_case(c);
    trans_form(&mut t.st, &mut t.g, t.node);
    verdict((|| {
        let after = c.after("verts");
        for (k, v) in t.st.verts.iter().enumerate() {
            let port = [v.world.x, v.world.y, v.world.z, v.ps.x, v.ps.y, v.normal.x, v.normal.y, v.normal.z].map(f32::to_bits);
            for (i, p) in port.iter().enumerate() {
                let p = if mutate() { *p ^ 1 } else { *p };
                if bits(after, 19 * k + 8 + i) != p {
                    return Err(format!("vertex {k} word {}: original {:08x}, port {p:08x}", 8 + i, bits(after, 19 * k + 8 + i)));
                }
            }
        }
        same_matrices(c, &t.st.camera)?;
        same_text(c, &t.g.messages)
    })())
}

/// The words `barr` and `clip` leave in the triangles of the case's array.
fn same_triangles(c: &Case, st: &ShadeState) -> Result<(), String> {
    let after = c.after("tns");
    for (k, n) in st.tns.iter().take(GT).enumerate() {
        let mut port = vec![n.n.x.to_bits(), n.n.y.to_bits(), n.n.z.to_bits(), n.d.to_bits(), n.inv_d.to_bits(), n.axis as u16 as u32];
        port.push(n.inv_n.to_bits());
        for e in n.edge {
            port.extend([e.x, e.y, e.z].map(f32::to_bits));
        }
        port.extend([n.flags as u32, n.flags2 as u32]);
        let at = [3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21, 22];
        for (p, i) in port.iter().zip(at) {
            let p = if mutate() { *p ^ 1 } else { *p };
            if bits(after, 27 * k + i) != p {
                return Err(format!("triangle {k} word {i}: original {:08x}, port {p:08x}", bits(after, 27 * k + i)));
            }
        }
    }
    Ok(())
}

/// The scan-line table and the trace list in the generator's numbers. A line that still
/// holds the marker the check put there (a record whose triangle is `usize::MAX`) was not
/// cleared and is left out.
pub fn same_lists(c: &Case, st: &ShadeState, g: &Geometry, nmap: usize, with_trace: bool) -> Result<(), String> {
    let mut lines = Vec::new();
    for (y, list) in g.bins.lines.iter().enumerate() {
        if list.is_empty() || list[0].tn == usize::MAX {
            continue;
        }
        lines.extend([y as u32, list.len() as u32]);
        for s in list.iter().rev() {
            lines.extend([s.x0.to_bits(), s.dx0.to_bits(), s.x1.to_bits(), s.dx1.to_bits(), s.y_end as u32]);
            tri_stream(st, nmap, s.tn, &mut lines);
        }
    }
    if !g.bins.stray.is_empty() {
        return Err(format!("{} records filed outside the table", g.bins.stray.len()));
    }
    same_words("scan lines", c.output("lines"), &lines)?;
    if !with_trace {
        return Ok(());
    }
    let mut trace = Vec::new();
    for item in g.trace.items.iter().rev() {
        trace.extend(item.bounds.iter().map(|f| f.to_bits()));
        tri_stream(st, nmap, item.tn, &mut trace);
    }
    same_words("trace list", c.output("trace"), &trace)
}

/// Bytes of chunk memory the geometry stage took since the counters stood at `s0`.
fn geometry_bytes(g: &Geometry, s0: &[i32]) -> i32 {
    0x4c * (g.clipper.made_vertices - s0[5]) + 8 * (g.clipper.made_pairs - s0[6]) + 0x60 * (g.made_tns - s0[7]) + 0x28 * (g.bins.made - s0[8]) + 0x20 * (g.trace.made - s0[9])
}

fn check_process(c: &Case) -> Verdict {
    let mut t = geo_case(c);
    process(&mut t.st, &mut t.g, t.node);
    verdict((|| {
        same_triangles(c, &t.st)?;
        same_lists(c, &t.st, &t.g, t.nmap, true)?;
        let (g, s0) = (&t.g, &t.stats0);
        let mut stats: Vec<u32> = s0.iter().map(|v| *v as u32).collect();
        (stats[0], stats[5], stats[6], stats[7], stats[8], stats[9]) =
            (g.bins.accepted as u32, g.clipper.made_vertices as u32, g.clipper.made_pairs as u32, g.made_tns as u32, g.bins.made as u32, g.trace.made as u32);
        same_words("counters", c.output("g_stats"), &stats)?;
        same_words("picture bounds", c.output("g_psb"), &g.bins.bounds.map(f32::to_bits))?;
        same_words("scene box", c.output("g_box"), &g.trace.bounds.map(f32::to_bits))?;
        same_words("chunk memory used", c.output("used"), &[geometry_bytes(g, s0) as u32])?;
        same_matrices(c, &t.st.camera)?;
        same_text(c, &t.g.messages)
    })())
}

/// `setExistance(node, fraction)`: the fraction as stored in each material (+0x74c) and the
/// materials' flag words.
fn check_set_existance(c: &Case) -> Verdict {
    let mut t = geo_case(c);
    for (k, name) in ["mat0", "mat1"].iter().enumerate() {
        t.st.materials[k].work.existence = fl(c.input(name), 468);
    }
    set_existance(&mut t.st, &t.g, t.node, c.input("e").f32());
    verdict((|| {
        for (k, name) in ["mat0", "mat1"].iter().enumerate() {
            let (before, after) = (c.input(name), c.after(name));
            let m = &t.st.materials[k];
            let flags = (bits(before, 441) & 0xffff_0000) | m.flags as u32;
            same_words(&format!("material {k}"), &Field { name: String::new(), words: Some(vec![after.words()[441].clone(), after.words()[468].clone()]) }, &[flags, m.work.existence.to_bits()])?;
        }
        Ok(())
    })())
}

/// `freeModel(node)`: what was freed, in order, and the node's four pointers and two counts.
fn check_free_model(c: &Case) -> Verdict {
    let mut t = geo_case(c);
    let freed = free_model(&mut t.g, t.node);
    verdict((|| {
        let names = ["verts", "uv2", "uvs", "tns"];
        let port: Vec<&str> = names.iter().zip(freed).filter(|(_, f)| *f).map(|(n, _)| *n).collect();
        let original: Vec<String> = c.try_output("freed").map(|f| f.words().iter().map(|w| match w {
            Word::Ref { name, .. } => name.clone(),
            other => format!("{other:?}"),
        }).collect()).unwrap_or_default();
        if original != port {
            return Err(format!("freed: original {original:?}, port {port:?}"));
        }
        let after = c.after("model");
        let m = &t.g.models[t.node];
        for (i, null) in [(139, m.verts.is_none()), (141, m.tns.is_none()), (204, m.tcoords.is_none()), (267, m.uv2.is_none())] {
            if matches!(after.words()[i], Word::Value { value: 0, .. }) != null {
                return Err(format!("pointer at word {i}: original {:?}, port null {null}", after.words()[i]));
            }
        }
        same_words("counts", &Field { name: String::new(), words: Some(vec![after.words()[138].clone(), after.words()[140].clone()]) }, &[m.vert_count as u32, m.tn_count as u32])
    })())
}

// ----------------------------------------------------------------------------- set-up

fn camera_of(c: &Case) -> Camera {
    let f = c.input("g_cam");
    let v = |i: usize| Vector::new(fl(f, i), fl(f, i + 1), fl(f, i + 2));
    Camera {
        kind: bits(f, 3) as i16,
        position: v(5),
        interest: v(8),
        roll: fl(f, 13),
        near: fl(f, 14),
        far: fl(f, 15),
        fov: fl(f, 17),
        dof: bits(f, 28) as i32,
        window: bits(f, 67) as i16,
        window_v: core::array::from_fn(|i| fl(f, 69 + i)),
        ..Camera::default()
    }
}

fn settings_of(c: &Case) -> Settings {
    let crop = c.input("g_cropv").f32s();
    let wh = c.input("g_cropwh").i16s();
    Settings {
        width: c.input("g_w").i16(),
        height: c.input("g_h").i16(),
        pixel_ratio: c.input("g_ratio").f32(),
        level: c.input("g_level").i16(),
        field: c.input("g_field").i16(),
        field_no: c.input("g_fno").i16(),
        crop: c.input("g_crop").i32(),
        crop_left: crop[0],
        crop_right: crop[1],
        crop_top: crop[2],
        crop_bottom: crop[3],
        crop_width: wh[0],
        crop_height: wh[1],
        resume: c.input("g_resume").i16(),
        resume_line: c.input("g_rline").i32(),
    }
}

fn view_of(c: &Case) -> View {
    let f4 = |name: &str| -> [f32; 4] { core::array::from_fn(|i| fl(c.input(name), i)) };
    let i4 = |name: &str| -> [i32; 4] { core::array::from_fn(|i| bits(c.input(name), i) as i32) };
    let filter = c.input("g_filter").f32s();
    let dof = c.input("g_dofn").i32s();
    View {
        picture: f4("g_pic"),
        region: i4("g_region"),
        region_px: i4("g_regpx"),
        window: f4("g_window"),
        minus_near: fl(c.input("g_planes"), 0),
        minus_far: fl(c.input("g_planes"), 1),
        dir: core::array::from_fn(|i| fl(c.input("g_dir"), i)),
        bounds: f4("g_bounds"),
        field_half: c.input("g_fhalf").f32(),
        filter_size: c.input("g_fsize").i16(),
        filter: core::array::from_fn(|i| core::array::from_fn(|j| filter[11 * i + j])),
        buffer_len: c.input("g_blen").i32(),
        dof_lines: dof[0],
        dof_half: dof[1],
    }
}

/// Every word of the view against what the routine left in the globals.
fn same_view(c: &Case, v: &View) -> Result<(), String> {
    same_view_with(c, v, bits(c.input("g_bounds"), 4))
}

fn same_view_with(c: &Case, v: &View, fifth: u32) -> Result<(), String> {
    let f = |a: &[f32]| -> Vec<u32> { a.iter().map(|x| x.to_bits()).collect() };
    let i = |a: &[i32]| -> Vec<u32> { a.iter().map(|x| *x as u32).collect() };
    same_words("picture", c.after("g_pic"), &f(&v.picture))?;
    same_words("region", c.after("g_region"), &i(&v.region))?;
    same_words("region in pixels", c.after("g_regpx"), &i(&v.region_px))?;
    same_words("window", c.after("g_window"), &f(&v.window))?;
    same_words("depth planes", c.after("g_planes"), &f(&[v.minus_near, v.minus_far]))?;
    same_words("direction", c.after("g_dir"), &f(&v.dir))?;
    same_words("field half", c.after("g_fhalf"), &f(&[v.field_half]))?;
    same_words("filter width", c.after("g_fsize"), &[v.filter_size as u16 as u32])?;
    same_words("filter", c.after("g_filter"), &f(&v.filter.concat()))?;
    same_words("buffer length", c.after("g_blen"), &[v.buffer_len as u32])?;
    let mut bounds = f(&v.bounds);
    bounds.push(fifth);
    same_words("bounds", c.after("g_bounds"), &bounds)
}

fn check_viewport(c: &Case) -> Verdict {
    let mut v = view_of(c);
    init_viewport(&camera_of(c), &settings_of(c), &mut v);
    verdict(same_view(c, &v))
}

fn check_filter(c: &Case) -> Verdict {
    let mut v = view_of(c);
    init_filter(c.input("g_level").i16(), &mut v);
    verdict(same_view(c, &v))
}

fn check_bounds(c: &Case) -> Verdict {
    let mut v = view_of(c);
    set_bounds(camera_of(c).dof, &settings_of(c), &mut v);
    verdict(same_view(c, &v))
}

/// `initBuffers`: where each of the twelve buffers starts in chunk memory (which gives
/// their sizes), the three counters, that both tables are clear.
fn check_buffers(c: &Case) -> Verdict {
    let v = view_of(c);
    let st = c.input("g_st").i32s();
    let mut stats = [st[0], st[1], st[3]];
    let b = init_buffers(&v, c.input("g_w").i16(), &mut stats);
    verdict((|| {
        let pad = |n: i32| ((n + 3) & !3) as u32;
        let w = b.line as i32;
        let sizes = [4 * b.scan_lines as i32, 4 * b.columns as i32, 48 * b.columns as i32, w, w, w, w, 4 * w, w, w, w, w];
        let mut at = 0u32;
        let mut offsets = Vec::new();
        for n in sizes {
            offsets.push(at);
            at = at.wrapping_add(pad(n));
        }
        same_words("buffers", c.output("offsets"), &offsets)?;
        same_words("chunk memory used", c.output("used"), &[at])?;
        same_words("tables", c.output("dirty"), &[0])?;
        same_words("counters", c.after("g_st"), &[stats[0] as u32, stats[1] as u32, st[2] as u32, stats[2] as u32])?;
        same_view(c, &v)
    })())
}

/// `SI_camera`: the direction floats and the matrices; a case in which the original calls
/// `si_fatal` (answered by a recorder in the harness, so that the routine went on where in
/// the program it would not) is compared on the text only.
fn check_camera(c: &Case) -> Verdict {
    let mut v = view_of(c);
    let m = c.input("g_mat").f32s();
    let mat = |at: usize| -> si_math::types::Matrix { core::array::from_fn(|i| core::array::from_fn(|j| m[at + 4 * i + j])) };
    let mut rc = RenderCamera { world_to_raw_mat: mat(0), raw_to_world_mat: mat(16), vect_mat: mat(32), prsp_mat: mat(48), near_z: 0.0 };
    let got = camera(&camera_of(c), &settings_of(c), &mut v, &mut rc);
    let fatal = c.calls.iter().any(|k| k.name == "si_fatal");
    if fatal {
        // the original's calls after the fatal one are not the port's to make
        si_x87::trig::replay::start(Vec::new());
        let text = String::from_utf8_lossy(&c.output("str0").to_bytes()).into_owned();
        return Verdict::from(got == Err(text.as_str()), || format!("fatal: original {text:?}, port {got:?}"));
    }
    verdict((|| {
        if let Err(e) = got {
            return Err(format!("the port stops with {e:?}, the original does not"));
        }
        same_matrices(c, &rc)?;
        same_view(c, &v)
    })())
}

// --------------------------------------------------------------------- SI_update, whole

/// What stands for the routines the harness answered with recorders.
struct Recorded;
impl GeometryEnv for Recorded {
    fn collect(&mut self, _: &mut ShadeState, _: &mut Geometry, _: usize) {}
}
impl UpdateEnv for Recorded {}

/// The scene of an `SI_update` vector before the call.
pub fn scene_case(c: &Case) -> (Prepared, usize) {
    let mut p = Prepared { camera: camera_of(c), settings: settings_of(c), view: view_of(c), ..Prepared::default() };
    let loaded = crate::outer::Loaded::of(c);
    for name in ["mat0", "mat1"] {
        p.st.materials.push(si_render::inimage::material_at(&loaded.m, loaded.at(name)));
    }
    let t = c.input("tns");
    for k in 0..GT {
        let w = &t.words()[27 * k..27 * k + 27];
        let f = |i: usize| fl(t, 27 * k + i);
        p.st.tns.push(Tn {
            v: core::array::from_fn(|i| offset_in(&w[i], "verts", 0x4c)),
            n: Vector::new(f(3), f(4), f(5)),
            d: f(6),
            inv_d: f(7),
            axis: bits(t, 27 * k + 8) as i16,
            inv_n: f(10),
            edge: core::array::from_fn(|e| Vector::new(f(11 + 3 * e), f(12 + 3 * e), f(13 + 3 * e))),
            material: name_of(&w[20], &["mat0", "mat1"]).unwrap_or(0) as u32,
            flags: bits(t, 27 * k + 21) as u16,
            flags2: bits(t, 27 * k + 22) as u16,
            w58: bits(t, 27 * k + 24) as i16,
            next: offset_in(&w[26], "tns", 0x60),
        });
    }
    let v = c.input("verts");
    for k in 0..GV {
        let f = |i: usize| fl(v, 19 * k + i);
        p.st.verts.push(RVertex {
            w00: [bits(v, 19 * k), bits(v, 19 * k + 1)],
            raw: Vector::new(f(2), f(3), f(4)),
            raw_normal: Vector::new(f(5), f(6), f(7)),
            world: Vector::new(f(8), f(9), f(10)),
            ps: si_math::types::Pspoint { x: f(11), y: f(12) },
            normal: Vector::new(f(13), f(14), f(15)),
            uv: offset_in(&v.words()[19 * k + 16], "uvs", 8).unwrap_or(0) as u32,
            w44: bits(v, 19 * k + 17),
            p48: bits(v, 19 * k + 18),
        });
    }
    p.st.uvs = c.input("uvs").f32s().chunks(2).map(|q| [q[0], q[1]]).collect();
    let m = c.input("g_mat").f32s();
    let mat = |at: usize| -> si_math::types::Matrix { core::array::from_fn(|i| core::array::from_fn(|j| m[at + 4 * i + j])) };
    p.st.camera = RenderCamera { world_to_raw_mat: mat(0), raw_to_world_mat: mat(16), vect_mat: mat(32), prsp_mat: mat(48), near_z: 0.0 };
    let names = ["n0", "n1", "n2"];
    for name in names {
        let f = c.input(name);
        let w = f.words();
        let me = names.iter().position(|n| *n == name).unwrap();
        p.g.models.push(RenModel {
            reversed: bits(f, 2) & 8 != 0,
            kind: bits(f, 12) as i32,
            latdep: name_of(&w[17], &names).expect("latdep"),
            gscl: Vector::new(fl(f, 91), fl(f, 92), fl(f, 93)),
            cscl: Vector::new(fl(f, 232), fl(f, 233), fl(f, 234)),
            gmat: core::array::from_fn(|i| core::array::from_fn(|j| fl(f, 113 + 4 * i + j))),
            materials: vec![0, 1],
            flags: bits(f, 133) as u16,
            n_maps: (bits(f, 133) >> 16) as i16,
            vert_count: bits(f, 138) as i32,
            verts: offset_in(&w[139], "verts", 0x4c),
            tn_count: bits(f, 140) as i32,
            tns: offset_in(&w[141], "tns", 0x60),
            tcoords: None,
            uv2: None,
            brother: name_of(&w[176], &names),
            son: name_of(&w[177], &names),
            registered: me,
        });
    }
    let s0 = c.input("g_stats").i32s();
    let marker = si_math::geometry::Span { tn: usize::MAX, ..Default::default() };
    p.g.bins.lines = vec![vec![marker]; bits(c.input("meta"), 1) as usize];
    p.g.bins.accepted = s0[0];
    p.g.bins.made = s0[8];
    p.g.trace.made = s0[9];
    p.g.made_tns = s0[7];
    p.g.clipper.made_vertices = s0[5];
    p.g.clipper.made_pairs = s0[6];
    p.tree_counts = [s0[10], s0[11]];
    p.stats = core::array::from_fn(|i| s0[i]);
    p.st.texture_pixels = s0[13];
    p.g.area_sort = bits(c.input("g_sort"), 0) == 0x0827_4dac;
    p.g.right_hand = c.input("g_rh").i16() != 0;
    p.g.glb_manip = c.input("g_manip").i32();
    p.g.raytrace = bits(c.input("g_rt"), 0) & 1 != 0;
    let fr = c.input("g_frames").i16s();
    let sf = c.input("g_sflags").i16s();
    p.options = Options { raytrace: bits(c.input("g_rt"), 0) as u16, sequence: c.input("g_seq").i16(), blur: c.input("g_blur").i16(), textures_2d: c.input("g_tex2d").i16(), tree_stats: sf[0], mem_stats: sf[1], first_frame: fr[0], frame: fr[3], ..Options::default() };
    let e = c.input("g_empty").f32s();
    p.bsp_params = BspParams { min_count: c.input("g_min").i16(), max_level: c.input("g_max").i16(), cell_volume: c.input("g_cv").f32(), empty_fraction: c.input("g_frac").f32(), empty_box: core::array::from_fn(|i| e[i]) };
    let eb = c.input("g_ebounds").f32s();
    p.empty_bounds = [eb[0], eb[1], eb[2], eb[3]];
    p.root = name_of(&c.input("g_rootm").words()[0], &names);
    (p, bits(c.input("meta"), 0) as usize)
}

/// Everything `SI_update` left, against the port's [`update`]: the vertices (position in
/// the renderer's space, on the picture plane, normal), the triangles, the scan-line table
/// with every record, triangle and vertex the clipper made, the trace list, the tree, the
/// view, the matrices, the counters, the chunk memory used.
fn same_prepared(c: &Case, p: &Prepared, nmap: usize) -> Result<(), String> {
    let after = c.after("verts");
    for (k, v) in p.st.verts.iter().take(GV).enumerate() {
        let port = [v.world.x, v.world.y, v.world.z, v.ps.x, v.ps.y, v.normal.x, v.normal.y, v.normal.z].map(f32::to_bits);
        for (i, q) in port.iter().enumerate() {
            let q = if mutate() { *q ^ 1 } else { *q };
            if bits(after, 19 * k + 8 + i) != q {
                return Err(format!("vertex {k} word {}: original {:08x}, port {q:08x}", 8 + i, bits(after, 19 * k + 8 + i)));
            }
        }
    }
    same_triangles(c, &p.st)?;
    same_rest(c, p, nmap, [0; 3])
}

/// The rest of what `SI_update` left: the scan-line table, the trace list, the view, the
/// matrices, the counters, the scene box, the tree, the chunk memory used. `heap` is what
/// the converters added to the vertex, pair and triangle counters for arrays that are not
/// in chunk memory (zero when `SI_collect` was answered by a recorder).
pub fn same_rest(c: &Case, p: &Prepared, nmap: usize, heap: [i32; 3]) -> Result<(), String> {
    // the tree build relinks the trace list's records into the cells' lists: after it the
    // list as such is gone (its records are compared with the tree)
    same_lists(c, &p.st, &p.g, nmap, p.bsp.nodes.is_empty())?;
    let uncleared = p.g.bins.lines.iter().position(|l| l.first().is_some_and(|s| s.tn == usize::MAX)).unwrap_or(p.g.bins.lines.len());
    same_words("first line not cleared", c.output("uncleared"), &[uncleared as u32])?;
    same_view_with(c, &p.view, bits(c.input("g_ebounds"), 4))?;
    same_matrices(c, &p.st.camera)?;
    let stats: Vec<u32> = p.stats_words().iter().map(|v| *v as u32).collect();
    same_words("counters", c.output("g_stats"), &stats)?;
    same_words("scene box", c.output("g_box"), &p.g.trace.bounds.map(f32::to_bits))?;
    // the tree
    let id = |n: Option<usize>| n.map_or(0, |i| i as u32 + 1);
    let has_tree = !p.bsp.nodes.is_empty();
    same_words("root", c.output("root"), &[if has_tree { id(Some(p.bsp.root)) } else { 0 }])?;
    // (the list entry counter was cleared at the start: the trace list is counted by its length)
    let s0 = c.input("g_stats").i32s();
    let g = &p.g;
    let mut bytes = 0x4c * (g.clipper.made_vertices - s0[5] - heap[0]) + 8 * (g.clipper.made_pairs - s0[6] - heap[1]) + 0x60 * (g.made_tns - s0[7] - heap[2]) + 0x28 * (g.bins.made - s0[8]) + 0x20 * g.trace.items.len() as i32;
    if has_tree {
        same_words("outside", c.output("outside"), &[id(p.bsp.outside)])?;
        let mut tree = Vec::new();
        for n in &p.bsp.nodes {
            tree.extend([n.kind as u16 as u32, n.level as u16 as u32, n.mailbox as u32]);
            if n.kind == 0 {
                tree.push(id(n.list));
                tree.extend(n.walls.iter().map(|w| id(*w)));
            } else {
                tree.extend([n.split.to_bits(), id(n.low), id(n.high), 0, 0, 0, 0]);
            }
        }
        same_words("tree", c.output("tree"), &tree)?;
        let mut links = Vec::new();
        for l in &p.bsp.links {
            links.extend(l.bound.iter().map(|f| f.to_bits()));
            links.extend([tn_place(l.tn).unwrap_or(u32::MAX), id(l.next)]);
        }
        same_words("list entries", c.output("links_out"), &links)?;
        bytes += 0x20 * (p.bsp.links.len() - g.trace.items.len()) as i32 + 0x18;
        bytes += p.bsp.nodes.iter().map(|n| if n.kind == 0 { 0x24 } else { 0x14 }).sum::<i32>();
    }
    bytes += EXTRA_CHUNK.with(|b| b.get());
    same_words("chunk memory used", c.output("used"), &[bytes as u32])
}

fn check_update(c: &Case) -> Verdict {
    let (mut p, nmap) = scene_case(c);
    let got = update(&mut p, &mut Recorded, c.input("time").f32());
    if c.calls.iter().any(|k| k.name == "si_fatal") {
        si_x87::trig::replay::start(Vec::new());
        let text = String::from_utf8_lossy(&c.output("str0").to_bytes()).into_owned();
        return Verdict::from(got == Err(text.as_str()), || format!("fatal: original {text:?}, port {got:?}"));
    }
    verdict((|| {
        if let Err(e) = got {
            return Err(format!("the port stops with {e:?}, the original does not"));
        }
        same_prepared(c, &p, nmap)
    })())
}

/// The shading side of a frame vector: full materials, the nodes' flag words, the lights
/// and the shading globals, as `gen_prep.py`'s `frame_case` lays them out.
pub fn shading_of(c: &Case, p: &mut Prepared, unset: f32) {
    let loaded = crate::outer::Loaded::of(c);
    for (k, name) in ["mat0", "mat1"].iter().enumerate() {
        p.st.materials[k] = si_render::inimage::material_at(&loaded.m, loaded.at(name));
    }
    for name in ["n0", "n1", "n2"] {
        p.st.models.push(si_render::shade::ShadeModel { flags: bits(c.input(name), 133) as u16, ..Default::default() });
    }
    p.st.lights = vec![crate::light_in(&loaded, c, "l0"), crate::light_in(&loaded, c, "l1")];
    p.st.light_head = if matches!(c.input("g_lights").words()[0], Word::Ref { .. }) { Some(0) } else { None };
    let g = &mut p.st.glb;
    g.unset = unset;
    g.fade = bits(c.input("g_fade"), 0) as i16;
    let fp = c.input("g_fadep").f32s();
    (g.fade_start, g.fade_end, g.fade_scale) = (fp[0], fp[2], fp[3]);
    let bg = c.input("g_bg").f32s();
    g.background = si_render::types::Rgbazo { r: bg[0], g: bg[1], b: bg[2], a: bg[3], z: bg[4], o: bg[5] };
    g.fog = bits(c.input("g_fog"), 0) as i16;
    let fog = c.input("fog").f32s();
    (g.fog_density, g.fog_base, g.fog_thickness, g.fog_start, g.fog_end) = (fog[0], fog[1], fog[2], fog[3], fog[4]);
    let fc = c.input("fogc").f32s();
    g.fog_color = [fc[0], fc[1], fc[2]];
    g.dof = 0;
    g.raytrace = bits(c.input("g_rt"), 0) & 1 != 0;
    let sw = c.input("g_sw");
    g.textures = bits(c.input("g_sw0"), 0) as i16;
    g.textures_2d = c.input("g_tex2d").i16();
    (g.textures_3d, g.reflection, g.refraction, g.shadows) = (bits(sw, 0) as i16, bits(sw, 1) as i16, bits(sw, 2) as i16, bits(sw, 3) as i16);
    g.max_depth = bits(c.input("g_depth"), 0) as i16;
    g.no_spec_alpha = bits(c.input("g_nsa"), 0) as i16;
    p.options.sub = c.input("g_sub").i16();
    p.options.contrast = c.input("g_con").f32();
}

/// A frame from the port's own preparation against the frame the original's `colSubdivid`
/// rendered, pixel by pixel, from the original's own preparation (`SI_update`): first the
/// prepared state as in [`check_update`], then red, green, blue, alpha and depth of every
/// pixel that had something to shade.
fn check_update_frame(c: &Case) -> Verdict {
    let run = |unset: f32| {
        si_x87::trig::replay::start(c.tape());
        let (mut p, nmap) = scene_case(c);
        shading_of(c, &mut p, unset);
        let got = update(&mut p, &mut Recorded, c.input("time").f32());
        (p, nmap, got)
    };
    let (p, nmap, got) = run(0.0);
    verdict((|| {
        if let Err(e) = got {
            return Err(format!("the port stops with {e:?}"));
        }
        same_prepared(c, &p, nmap)?;
        same_frame(c, p, run(4321.5).0)
    })())
}

/// The frame the port renders from its own preparation against the one the original's
/// `colSubdivid` rendered from the original's: red, green, blue, alpha and depth of every
/// pixel that had something to shade. `second` is the same scene prepared with another
/// value for the words the original never sets: a result word that differs between the two
/// has no value in the original and is left out.
pub fn same_frame(c: &Case, p: Prepared, second: Prepared) -> Result<(), String> {
    same_frame_many(c, p, vec![second], false)
}

/// [`same_frame`] with several other preparations of the scene, each with its own value for
/// the words the original never sets: a result word is compared only when all of them give
/// the port's (eighth pass: one other value does not show a word that turns on a threshold,
/// such as a pixel the adaptive sampler refines or not by a corner whose colour is unset).
/// With `whole_pixels` a pixel of which the original leaves any value undefined is not
/// compared at all: where its colour comes from unset vertex words (a node shown as its
/// box), so does the contrast that decides whether the pixel is refined, and with it the
/// depth it ends with; the port's stand-in for those words is one value for every vertex,
/// which never gives a contrast.
pub fn same_frame_many(c: &Case, p: Prepared, seconds: Vec<Prepared>, whole_pixels: bool) -> Result<(), String> {
    same_frame_with(c, p, seconds, whole_pixels, &|| {})
}

/// [`same_frame_many`] with something to do before each rendering (ninth pass: a frame with
/// 3D textures calls the C library, and each rendering replays those calls from their
/// start). The port's own rendering is the last, so that what it asked of the library is
/// what the case's replay ends with.
pub fn same_frame_with(c: &Case, p: Prepared, seconds: Vec<Prepared>, whole_pixels: bool, before: &dyn Fn()) -> Result<(), String> {
    let frame = c.output("frame");
    let region: Vec<u32> = p.view.region.iter().map(|v| *v as u32).collect();
    same_words("region", c.output("frame_region"), &region)?;
    if frame.is_empty() {
        return Ok(());
    }
    let width = p.settings.width as usize;
    let render = |p: Prepared| {
        let mut s = p.sampler();
        s.ss.dof_limit = c.input("g_lim").f32();
        crate::scene::render_prepared(p.st, p.bsp, c.input("g_mb").i32(), s, width)
    };
    let y_max = p.view.region[3];
    let others: Vec<_> = seconds
        .into_iter()
        .map(|q| {
            before();
            render(q)
        })
        .collect();
    before();
    let a = render(p);
    let w = frame.words();
    for px in w.chunks(7) {
        let (x, y) = (px[0].value().unwrap_or(0) as usize, px[1].value().unwrap_or(0) as i32);
        let row = (y_max - y) as usize;
        let pa = a.get(row).and_then(|l| l.get(x)).ok_or_else(|| format!("the port has no pixel ({x}, {y})"))?;
        let va = [pa.r, pa.g, pa.b, pa.a, pa.z];
        if whole_pixels && px[2..7].iter().any(|w| !matches!(w, Word::Value { .. })) {
            continue;
        }
        for k in 0..5 {
            let port = if mutate() { va[k].to_bits() ^ 1 } else { va[k].to_bits() };
            let settled = others.iter().all(|b| {
                let pb = &b[row][x];
                [pb.r, pb.g, pb.b, pb.a, pb.z][k].to_bits() == va[k].to_bits()
            });
            if let Word::Value { value, .. } = &px[2 + k] {
                if settled && *value as u32 != port {
                    return Err(format!("pixel ({x}, {y}) value {k}: original {:08x}, port {port:08x}", *value as u32));
                }
            }
        }
    }
    Ok(())
}

/// `initData`: every global it writes. The statistics file is opened in the harness's own
/// file system, where opening for writing does not fail: the failing branch is not reached.
fn check_init_data(c: &Case) -> Verdict {
    struct Env(Vec<Vec<u8>>);
    impl GeometryEnv for Env {
        fn collect(&mut self, _: &mut ShadeState, _: &mut Geometry, _: usize) {}
    }
    impl UpdateEnv for Env {
        fn stats_open(&mut self, name: &[u8]) -> bool {
            self.0.push(name.to_vec());
            true
        }
    }
    let mut p = Prepared::default();
    let sw = c.input("g_sw").i16s();
    let s0 = c.input("g_stats").i32s();
    p.settings.level = c.input("g_level").i16();
    p.options = Options { raytrace: bits(c.input("g_rt"), 0) as u16, adaptive: c.input("g_adapt").i16(), contrast: c.input("g_con").f32(), shutter: fl(c.input("g_shut"), 0), blur_step: c.input("g_bstep").f32(), timing: sw[0], tree_stats: sw[1], mem_stats: sw[2], name: c.input("name").to_bytes().split(|b| *b == 0).next().unwrap_or(&[]).to_vec(), ..Options::default() };
    p.stats = core::array::from_fn(|i| s0[i]);
    p.contrast_third = c.input("g_third").f32();
    let had_light = bits(c.input("g_lights"), 0) != 0;
    if had_light {
        p.st.lights.push(Default::default());
        p.st.light_head = Some(0);
    }
    let mut env = Env(Vec::new());
    init_data(&mut p, &mut env);
    verdict((|| {
        same_words("ray tracing word", c.after("g_rt"), &[p.options.raytrace as u32])?;
        let text = |t: &str| -> Vec<u32> { t.bytes().chain(core::iter::repeat(0)).take(12).map(|b| b as u32).collect() };
        same_words("version text", c.after("g_ver"), &text(si_render::initialize::RENDER_VERSION))?;
        same_words("second text", c.after("g_uns"), &text(si_render::initialize::UNSPECIFIED))?;
        same_words("empty fraction", c.after("g_frac"), &[p.bsp_params.empty_fraction.to_bits()])?;
        same_words("level", c.after("g_level"), &[p.settings.level as u16 as u32])?;
        same_words("blur threshold", c.after("g_thr"), &[p.blur_threshold.to_bits()])?;
        same_words("shutter", c.after("g_shut"), &[p.options.shutter.to_bits(), p.shutter_copy.to_bits()])?;
        same_words("word at 0x08790db8", c.after("g_db8"), &[0])?;
        same_words("switches", c.after("g_sw"), &[p.options.timing as u16 as u32, p.options.tree_stats as u16 as u32, p.options.mem_stats as u16 as u32])?;
        same_words("statistics file", c.output("file"), &[!env.0.is_empty() as u32])?;
        if let Some(name) = env.0.first() {
            let path = format!("file:{}", String::from_utf8_lossy(name));
            if c.try_output(&path).is_none() {
                return Err(format!("the port opens {path}, the original did not"));
            }
        }
        same_words("filing routine", c.after("g_sort"), &[if p.g.area_sort { 0x0827_4dac } else { 0x0827_50dc }])?;
        same_words("a third of the contrast", c.after("g_third"), &[p.contrast_third.to_bits()])?;
        let made = !had_light;
        if bits(c.output("light"), 0) != made as u32 || (made && (p.st.light_head != Some(0) || p.st.lights.len() != 1)) {
            return Err(format!("default light: original {:?}, port made {made}", c.output("light").words()));
        }
        same_words("chunk memory used", c.output("used"), &[if made { 0x240 } else { 0 }])?;
        let b = &p.blank;
        let mut blank = vec![0u32; 5];
        blank.extend([b.r, b.g, b.b, b.a, b.z, b.o].map(f32::to_bits));
        blank.extend(p.bsp_params.empty_box.map(f32::to_bits));
        blank.extend(p.empty_bounds.map(f32::to_bits));
        same_words("blank sample and empty boxes", c.after("g_blank"), &blank)?;
        let stats: Vec<u32> = p.stats_words().iter().map(|v| *v as u32).collect();
        same_words("counters", c.after("g_stats"), &stats)
    })())
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "initData" => check_init_data,
        "SI_update#frame" => check_update_frame,
        "SI_update" => check_update,
        "si_initViewport" => check_viewport,
        "initFilter" => check_filter,
        "si_setBounds" => check_bounds,
        "initBuffers" => check_buffers,
        "SI_camera" => check_camera,
        "setExistance" => check_set_existance,
        "freeModel" => check_free_model,
        "transForm" => check_transform,
        "process" => check_process,
        "si_initAncestry" => check_ancestry,
        "si_bsptree" => check_bsptree,
        "xSplit" => |c: &Case| check_split(c, 0),
        "ySplit" => |c: &Case| check_split(c, 1),
        "zSplit" => |c: &Case| check_split(c, 2),
        _ => return None,
    })
}
