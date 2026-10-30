// SPDX-License-Identifier: MIT
//! `kine-corpus <directory> [--list]`: the skeleton chains of every model file under a
//! directory (finding 0022, "Corpus").
//!
//! Each model (`.hrc`) is read with `si-hrc`, its nodes are placed in a `si-model` arena
//! with their stored local values and brought up to date with that crate's node update.
//! Then, for every chain (a null with algorithm 1 or 2, joints below it, an effector):
//!
//! - what it is: the chain type its root's record stores, the number of joints, whether the
//!   record's joint count agrees;
//! - **as stored**: the chain is solved for the position its effector has
//!   ([`si_kine::invkin::model_solve_inv_kin`]) and the joints' angles compared with the
//!   stored ones. A stored pose is a fixed point of the solver only if the file was saved
//!   solved;
//! - **from the preferred pose**: the same solve after the stored pose was pushed away from
//!   the goal (every joint at its preferred angles, the chain updated), to see how close
//!   the solver brings the effector back and whether it finds the stored angles again.
//!
//! No scene is loaded: rotation limits, break joints and plane constraints live in a
//! scene's relations, so every chain is solved without them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{read_file, Content, Kind};
use si_kine::iktools::{dist, limb_length};
use si_kine::rec::IkStore;
use si_kine::skeleton::{is_effector, is_root};
use si_kine::{invkin, Globals};
use si_model::node::Models;
use si_model::transform::update_son_state;
use si_model::NodeId;
use si_types::geom::Vector;

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

fn v3(a: [f32; 3]) -> Vector {
    Vector { x: a[0], y: a[1], z: a[2] }
}

/// Places a file's node, its brothers and their subtrees in the arena.
fn place(models: &mut Models, ik: &mut IkStore, first: &si_hrc::Node, father: Option<NodeId>) -> Option<NodeId> {
    let mut top = None;
    let mut n = Some(first);
    while let Some(x) = n {
        let id = models.allocate(Some(&x.name));
        top.get_or_insert(id);
        {
            let m = models.m_mut(id);
            m.kind = x.kind;
            m.algo_type = x.algorithm;
            m.scal = v3(x.scaling);
            m.vrot = v3(x.rotation);
            m.trans = v3(x.translation);
        }
        if let Some(k) = &x.ik {
            let p = invkin::inv_kin_from_file(ik, k.kind, k.nb_joints, k.rotation);
            models.m_mut(id).inv_kin = p;
            si_kine::rec::push_tx(models, ik, id);
        }
        if let Some(f) = father {
            models.arena.create_parentship(f, id);
        }
        if let Some(s) = &x.son {
            place(models, ik, s, Some(id));
        }
        n = x.brother.as_deref();
    }
    top
}

struct Chain {
    root: NodeId,
    joints: Vec<NodeId>,
    effector: NodeId,
}

fn chains(models: &Models, broken: &mut u64) -> Vec<Chain> {
    let mut out = Vec::new();
    for root in 0..models.len() {
        if !is_root(models, root) {
            continue;
        }
        let mut joints = Vec::new();
        let mut p = models.son(root);
        let effector = loop {
            match p {
                None => break None,
                Some(q) if is_effector(models, q) => break Some(q),
                Some(q) => {
                    joints.push(q);
                    p = models.son(q);
                }
            }
        };
        match effector {
            Some(effector) if !joints.is_empty() && joints.iter().all(|&j| models.m(j).kind == 5) => out.push(Chain { root, joints, effector }),
            _ => *broken += 1,
        }
    }
    out
}

/// The largest change of a joint angle, in radians, and whether every angle kept its bits.
fn moved(models: &Models, joints: &[NodeId], before: &[Vector]) -> (f64, bool) {
    let mut worst = 0.0f64;
    let mut same = true;
    for (&j, b) in joints.iter().zip(before) {
        let a = models.m(j).vrot;
        for (x, y) in [(a.x, b.x), (a.y, b.y), (a.z, b.z)] {
            same &= x.to_bits() == y.to_bits();
            let d = (x as f64 - y as f64).abs();
            worst = if d.is_nan() { f64::INFINITY } else { worst.max(d) };
        }
    }
    (worst, same)
}

/// The largest distance a joint or the effector moved, and the largest change of an element
/// of a joint's global rotation matrix.
fn displaced(models: &Models, ch: &Chain, before: &Models) -> (f64, f64) {
    let (mut far, mut turned) = (0.0f64, 0.0f64);
    for &j in ch.joints.iter().chain([&ch.effector]) {
        let (a, b) = (models.m(j), before.m(j));
        far = far.max(dist(a.gtrs, b.gtrs).to_f64());
        if j != ch.effector {
            for r in 0..3 {
                for c in 0..3 {
                    turned = turned.max((a.grot[r][c] as f64 - b.grot[r][c] as f64).abs());
                }
            }
        }
    }
    (far, turned)
}

fn place_bucket(r: f64) -> &'static str {
    if r <= 1e-6 {
        "0 no joint moved by more than 1e-6 of the length"
    } else if r <= 8e-4 {
        "1 up to 0.0008 (the solver's tolerance)"
    } else if r <= 1e-2 {
        "2 up to 1%"
    } else if r <= 1e-1 {
        "3 up to 10%"
    } else {
        "4 more"
    }
}

fn turn_bucket(r: f64) -> &'static str {
    if r <= 1e-6 {
        "0 no joint's global rotation changed by more than 1e-6"
    } else if r <= 1e-3 {
        "1 up to 0.001"
    } else if r <= 1e-1 {
        "2 up to 0.1"
    } else {
        "3 more"
    }
}

fn bucket(d: f64, same: bool) -> &'static str {
    if same {
        "0 unchanged, bit for bit"
    } else if d <= 1e-6 {
        "1 moved by up to 1e-6 rad"
    } else if d <= 1e-4 {
        "2 up to 1e-4"
    } else if d <= 1e-2 {
        "3 up to 1e-2"
    } else if d <= 1.0 {
        "4 up to 1"
    } else {
        "5 more than 1 rad"
    }
}

fn residual(r: f64) -> &'static str {
    if r <= 8e-4 {
        "0 within the solver's tolerance (0.0008 of the length)"
    } else if r <= 1e-2 {
        "1 within 1% of the length"
    } else if r <= 1e-1 {
        "2 within 10%"
    } else {
        "3 further"
    }
}

#[derive(Default)]
struct Tally {
    files: u64,
    unread: u64,
    nodes: u64,
    with_chain: u64,
    broken: u64,
    kinds: BTreeMap<(i32, usize), u64>,
    count_differs: u64,
    no_record: u64,
    joint_records: u64,
    joints: u64,
    rolls: u64,
    stored: BTreeMap<(i32, &'static str), u64>,
    stored_far: BTreeMap<(i32, &'static str), u64>,
    stored_place: BTreeMap<(i32, &'static str), u64>,
    stored_turn: BTreeMap<(i32, &'static str), u64>,
    rest_place: BTreeMap<(i32, &'static str), u64>,
    shape_place: BTreeMap<(&'static str, &'static str), u64>,
    why: BTreeMap<&'static str, u64>,
    rest: BTreeMap<(i32, &'static str), u64>,
    rest_angles: BTreeMap<(i32, &'static str), u64>,
    faults: u64,
}

fn check_file(data: &[u8], t: &mut Tally, list: Option<&str>) -> Option<()> {
    let f = read_file(data, Kind::Model)?;
    let Content::Model(root) = &f.content else { return None };
    let mut models = Models::default();
    let mut ik = IkStore::default();
    let top = place(&mut models, &mut ik, root, None)?;
    let g = Globals::default();
    update_son_state(&mut models, None, top, true, &g.model, &mut ());
    t.nodes += models.len() as u64;
    let found = chains(&models, &mut t.broken);
    if !found.is_empty() {
        t.with_chain += 1;
    }
    for ch in &found {
        let n = ch.joints.len();
        let kind = match ik.of(&models, ch.root) {
            Some(r) => {
                if r.nb_joints != n as i32 {
                    t.count_differs += 1;
                }
                r.kind
            }
            None => {
                t.no_record += 1;
                -1
            }
        };
        *t.kinds.entry((kind, n)).or_default() += 1;
        for &j in &ch.joints {
            t.joints += 1;
            if let Some(r) = ik.of(&models, j) {
                t.joint_records += 1;
                if r.prot.x != 0.0 {
                    t.rolls += 1;
                }
            }
        }
        if kind < 0 || ch.joints.iter().any(|&j| ik.of(&models, j).is_none()) {
            continue;
        }
        let goal = models.m(ch.effector).gtrs;
        let stored: Vec<Vector> = ch.joints.iter().map(|&j| models.m(j).vrot).collect();
        let (len, _, _) = limb_length(&models, ch.joints[0], n as i32, &g);
        let len = len.to_f64().abs().max(1e-30);

        // as stored
        let (mut m, mut k) = (clone(&models), ik.clone());
        si_kine::fault::reset();
        invkin::model_solve_inv_kin(&mut m, &mut k, Some(ch.effector), goal, 0.0, &g, &mut ());
        t.faults += si_kine::fault::count() as u64;
        let (d, same) = moved(&m, &ch.joints, &stored);
        *t.stored.entry((kind, bucket(d, same))).or_default() += 1;
        let r = dist(goal, m.m(ch.effector).gtrs).to_f64() / len;
        *t.stored_far.entry((kind, residual(r))).or_default() += 1;
        let (far, turned) = displaced(&m, ch, &models);
        *t.stored_place.entry((kind, place_bucket(far / len))).or_default() += 1;
        if kind == 20 {
            // the stored pose by its shape: what the planar solver leaves in the joints after
            // the first is a turn about Z alone, unless a roll is in play
            let rolled = ch.joints.iter().any(|&j| ik.of(&models, j).unwrap().prot.x.abs() > 1e-4);
            let flat = ch.joints[1..].iter().all(|&j| models.m(j).vrot.x.abs() <= 1e-5 && models.m(j).vrot.y.abs() <= 1e-5);
            let shape = match (rolled, flat) {
                (false, true) => "1 no roll, second joint turned about Z only",
                (true, _) => "2 a joint has a roll",
                (false, false) => "3 no roll, second joint turned about X or Y too (not a pose of the planar solver)",
            };
            *t.shape_place.entry((shape, place_bucket(far / len))).or_default() += 1;
            if !rolled && flat && far / len > 8e-4 {
                let bend = models.m(ch.joints[1]).vrot.z;
                let pref = ik.of(&models, ch.joints[1]).unwrap().prot.z;
                let why = if (bend > 0.0) != (pref > 0.0) && bend != 0.0 {
                    "the stored bend is on the other side than the preferred angle's sign"
                } else if (m.m(ch.joints[1]).vrot.z - bend).abs() > 1e-3 {
                    "same side, another bend (the stored pose does not reach the effector's place with these bone lengths)"
                } else {
                    "same bend, another plane (the stored plane is not the default one)"
                };
                *t.why.entry(why).or_default() += 1;
            }
        }
        *t.stored_turn.entry((kind, turn_bucket(turned))).or_default() += 1;
        if let Some(name) = list {
            if !same {
                let rolls: Vec<f32> = ch.joints.iter().map(|&j| ik.of(&models, j).unwrap().prot.x).collect();
                println!("{}\t{kind}\t{n}\tangles {d:e}\tplaces {:e}\trotations {turned:e}\trolls {rolls:?}\t{name}", bucket(d, same), far / len);
            }
        }

        // from the preferred pose
        let (mut m, mut k) = (clone(&models), ik.clone());
        for &j in &ch.joints {
            let p = k.of(&m, j).unwrap().prot;
            m.m_mut(j).vrot = p;
        }
        let f = m.father(ch.joints[0]);
        update_son_state(&mut m, f, ch.joints[0], true, &g.model, &mut ());
        invkin::model_solve_inv_kin(&mut m, &mut k, Some(ch.effector), goal, 0.0, &g, &mut ());
        let r = dist(goal, m.m(ch.effector).gtrs).to_f64() / len;
        *t.rest.entry((kind, residual(r))).or_default() += 1;
        let (d, same) = moved(&m, &ch.joints, &stored);
        *t.rest_angles.entry((kind, bucket(d, same))).or_default() += 1;
        let (far, _) = displaced(&m, ch, &models);
        *t.rest_place.entry((kind, place_bucket(far / len))).or_default() += 1;
    }
    Some(())
}

fn clone(models: &Models) -> Models {
    let mut m = Models::default();
    for i in 0..models.len() {
        let id = m.allocate(None);
        *m.m_mut(id) = *models.m(i);
        m.data[id] = models.data[i].clone();
    }
    m
}

fn table(title: &str, rows: &BTreeMap<(i32, &'static str), u64>) {
    println!("\n{title}");
    let mut classes: Vec<&str> = rows.keys().map(|k| k.1).collect();
    classes.sort();
    classes.dedup();
    for kind in [10, 20, 30, 40] {
        let total: u64 = rows.iter().filter(|(k, _)| k.0 == kind).map(|(_, v)| v).sum();
        if total == 0 {
            continue;
        }
        println!("  chain type {kind}: {total}");
        for c in &classes {
            if let Some(v) = rows.get(&(kind, *c)) {
                println!("    {:>6}  {}", v, &c[2..]);
            }
        }
    }
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list = args.iter().any(|a| a == "--list");
    let Some(dir) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: kine-corpus [--list] <directory>");
        return 2;
    };
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    let mut t = Tally::default();
    for p in &files {
        let Ok(data) = std::fs::read(p) else { continue };
        t.files += 1;
        let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
        if check_file(&data, &mut t, list.then_some(rel.as_str())).is_none() {
            t.unread += 1;
        }
    }
    println!("model files: {} ({} not read), nodes: {}", t.files, t.unread, t.nodes);
    println!("files with a chain: {}", t.with_chain);
    let total: u64 = t.kinds.values().sum();
    println!("chains: {total}; roots without joints or without an effector: {}", t.broken);
    println!("  roots without a record: {}; records whose joint count is not the chain's: {}", t.no_record, t.count_differs);
    println!("  joints: {}, with a record: {}, with a roll (preferred x not 0): {}", t.joints, t.joint_records, t.rolls);
    println!("\nchains by the type their root's record stores and by joints");
    let mut by_kind: BTreeMap<i32, u64> = BTreeMap::new();
    for ((kind, n), v) in &t.kinds {
        *by_kind.entry(*kind).or_default() += v;
        println!("  type {kind:>3}  {n:>2} joints  {v:>5}");
    }
    for (kind, v) in &by_kind {
        println!("  type {kind:>3}  all        {v:>5}");
    }
    table("solved for the effector's stored position: how far the joints' angles move", &t.stored);
    table("... how far the joints and the effector move, as a fraction of the chain's length", &t.stored_place);
    println!("\n... the same for the two-joint chains (type 20), by the shape of the stored pose");
    let mut shapes: Vec<&str> = t.shape_place.keys().map(|k| k.0).collect();
    shapes.dedup();
    for sh in shapes {
        let total: u64 = t.shape_place.iter().filter(|(k, _)| k.0 == sh).map(|(_, v)| v).sum();
        println!("  {}: {total}", &sh[2..]);
        for ((s2, c), v) in &t.shape_place {
            if *s2 == sh {
                println!("    {:>6}  {}", v, &c[2..]);
            }
        }
    }
    println!("  of the first class, those that move by more than the tolerance:");
    for (w, v) in &t.why {
        println!("    {v:>6}  {w}");
    }
    table("... how far the joints' global rotation matrices change (largest element)", &t.stored_turn);
    table("... and where the effector ends, as a fraction of the chain's length", &t.stored_far);
    table("solved from the preferred pose for the same position: where the effector ends", &t.rest);
    table("... how far the angles end from the stored ones", &t.rest_angles);
    table("... and how far the joints end from their stored places", &t.rest_place);
    println!("\nplaces where the port left the original's path (null pointers it would read through): {}", t.faults);
    0
}

fn main() {
    let child = std::thread::Builder::new().stack_size(512 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap());
}
