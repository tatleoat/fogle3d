// SPDX-License-Identifier: MIT
//! Checks for the two routines of `geometry.c` that the harness can drive on plain records:
//! `extend` and `barr`. Vectors: `re/tools/si-math/emu/gen_geometry.py`.
use crate::util::*;
use si_math::geometry as g;
use si_math::geometry::{ClipBox, RVertex, Tn};
use si_math::types::{Pspoint, Vector};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

fn val(w: &Word) -> u32 {
    w.value().unwrap_or(0) as u32
}

fn fl(w: &Word) -> f32 {
    f32::from_bits(val(w))
}

fn vec_at(w: &[Word], i: usize) -> Vector {
    Vector::new(fl(&w[i]), fl(&w[i + 1]), fl(&w[i + 2]))
}

/// The nineteen words of an `SI_RVertex`.
fn vertex(f: &Field) -> RVertex {
    let w = f.words();
    RVertex {
        w00: [val(&w[0]), val(&w[1])],
        raw: vec_at(w, 2),
        raw_normal: vec_at(w, 5),
        world: vec_at(w, 8),
        ps: Pspoint { x: fl(&w[11]), y: fl(&w[12]) },
        normal: vec_at(w, 13),
        uv: val(&w[16]),
        w44: val(&w[17]),
        p48: val(&w[18]),
    }
}

fn vertex_words(v: &RVertex) -> Vec<u32> {
    let f = |x: f32| x.to_bits();
    let mut o = vec![v.w00[0], v.w00[1]];
    for p in [v.raw, v.raw_normal, v.world] {
        o.extend([f(p.x), f(p.y), f(p.z)]);
    }
    o.extend([f(v.ps.x), f(v.ps.y), f(v.normal.x), f(v.normal.y), f(v.normal.z), v.uv, v.w44, v.p48]);
    o
}

fn index(w: &Word, prefix: &str) -> Option<usize> {
    match w {
        Word::Ref { name, .. } => name.strip_prefix(prefix).and_then(|n| n.parse().ok()),
        _ => None,
    }
}

/// The 24 words of a `tn`.
fn tn(f: &Field) -> Tn {
    let w = f.words();
    Tn {
        v: [index(&w[0], "v"), index(&w[1], "v"), index(&w[2], "v")],
        n: vec_at(w, 3),
        d: fl(&w[6]),
        inv_d: fl(&w[7]),
        axis: val(&w[8]) as u16 as i16,
        inv_n: fl(&w[10]),
        edge: [vec_at(w, 11), vec_at(w, 14), vec_at(w, 17)],
        material: val(&w[20]),
        flags: val(&w[21]) as u16,
        flags2: val(&w[22]) as u16,
        w58: val(&w[24]) as u16 as i16,
        next: index(&w[26], "t"),
    }
}

fn tn_same(c: &Case, name: &str, t: &Tn) -> Verdict {
    let w = c.after(name).words();
    let m = mutate() as u32;
    let f = |x: f32| x.to_bits();
    let port: Vec<(usize, u32, &str)> = vec![
        (3, f(t.n.x) ^ m, "n.x"),
        (4, f(t.n.y), "n.y"),
        (5, f(t.n.z), "n.z"),
        (6, f(t.d) ^ m, "d"),
        (7, f(t.inv_d), "1/d"),
        (8, t.axis as u16 as u32, "axis"),
        (10, f(t.inv_n), "1/n"),
        (11, f(t.edge[0].x), "edge 0 x"),
        (12, f(t.edge[0].y), "edge 0 y"),
        (13, f(t.edge[0].z), "edge 0 z"),
        (14, f(t.edge[1].x), "edge 1 x"),
        (15, f(t.edge[1].y), "edge 1 y"),
        (16, f(t.edge[1].z), "edge 1 z"),
        (17, f(t.edge[2].x), "edge 2 x"),
        (18, f(t.edge[2].y), "edge 2 y"),
        (19, f(t.edge[2].z), "edge 2 z"),
        (21, t.flags as u32 ^ m, "flags"),
        (22, t.flags2 as u32, "flags2"),
    ];
    for (i, v, what) in port {
        if val(&w[i]) != v {
            return Verdict::Different(format!("{name} {what}: port {v:08x}, original {:08x}", val(&w[i])));
        }
    }
    Verdict::Same
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "extend" => |c: &Case| {
            let p = c.input("planes").f32s();
            let b = ClipBox { yon: p[0], hither: p[1], ..Default::default() };
            let cv = g::extend(&vertex(c.input("v")), &b);
            // the vertex is copied whole and d[0], d[1] are computed; d[2] to d[5] are whatever
            // the original's stack held, and the vector's buffer ends before them
            let w = c.output("ret").written();
            let mut port = vertex_words(&cv.v);
            port.extend(cv.d.iter().take(2).map(|x| x.to_bits()));
            port[19] ^= mutate() as u32;
            let same = w.len() == port.len() && w.iter().zip(&port).all(|(o, p)| o.map_or(false, |o| o as u32 == *p));
            Verdict::from(same, || format!("port {port:08x?}"))
        },
        "barr" => |c: &Case| {
            let count = c.inputs.iter().filter(|f| f.name.starts_with('t')).count();
            let nv = c.inputs.iter().filter(|f| f.name.starts_with('v')).count();
            let mut tns: Vec<Tn> = (0..count).map(|i| tn(c.input(&format!("t{i}")))).collect();
            let verts: Vec<RVertex> = (0..nv).map(|i| vertex(c.input(&format!("v{i}")))).collect();
            g::barr(&mut tns, Some(0), &verts);
            let mut v = Verdict::Same;
            for (i, t) in tns.iter().enumerate() {
                v = v.and(|| tn_same(c, &format!("t{i}"), t));
            }
            v
        },
        _ => return None,
    })
}
