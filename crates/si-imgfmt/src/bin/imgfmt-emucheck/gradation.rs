// SPDX-License-Identifier: MIT
//! Checks for the dithers and the gradation picture (`si_imgfmt::gradation`).

use crate::ops::same_picture_after;
use crate::util::*;
use si_imgfmt::gradation::{self, Diffusion, Drand48, OutputDither, Rgbazo};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

/// The first `n` words of a float buffer against ours, bit for bit; the rest untouched.
fn same_floats(what: &str, theirs: &Field, ours: &[f32]) -> Verdict {
    let w = theirs.written();
    let mut ours = ours.to_vec();
    if mutate() && !ours.is_empty() {
        ours[0] = f32::from_bits(ours[0].to_bits() ^ 1);
    }
    match ours.iter().enumerate().find(|(i, v)| w[*i].is_some_and(|t| t != v.to_bits() as u128)) {
        Some((i, v)) => Verdict::Different(format!("{what}[{i}]: original {:#x}, port {:#x}", w[i].unwrap(), v.to_bits())),
        None => Verdict::Same,
    }
}

fn same_prefix(what: &str, theirs: &Field, ours: &[u8]) -> Verdict {
    let w = theirs.written();
    let mut ours = ours.to_vec();
    if mutate() && !ours.is_empty() {
        ours[0] ^= 1;
    }
    let ok = (0..w.len()).all(|i| if i < ours.len() { w[i] == Some(ours[i] as u128) } else { w[i].is_none() });
    Verdict::from(ok, || format!("{what}: port {ours:?}"))
}

fn width(c: &Case) -> i32 {
    c.input("gl").words()[2].value().unwrap() as u32 as i32
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "fsInit" => |c: &Case| {
            let fs = Diffusion::new(c.input("w").i32());
            let g = c.output("gl").words();
            let acc = match &g[0] {
                Word::Ref { name, .. } => c.output(name).u32s(),
                _ => return Verdict::Different("no error row".into()),
            };
            Verdict::from(g[2].value() == Some(fs.width as u32 as u128), || "width".into())
                .and(|| Verdict::from(acc.len() == fs.acc.len() && acc.iter().zip(&fs.acc).all(|(a, b)| *a == b.to_bits()), || "error row".into()))
                .and(|| Verdict::from(matches!(&g[1], Word::Ref { .. }), || "no working row".into()))
        },
        "fsDitherFloat" => |c: &Case| {
            let w = width(c);
            let mut fs = Diffusion { width: w, scan: vec![0.0; 3 * w as usize], acc: c.input("acc").f32s()[..3 * w as usize].to_vec() };
            let (mut r, mut g, mut b) = (c.input("r").f32s(), c.input("g").f32s(), c.input("b").f32s());
            if fs.dither_float(&mut r, &mut g, &mut b).is_err() {
                return Verdict::Skip;
            }
            Verdict::from(c.after("r").same_f32(&r) && c.after("g").same_f32(&g) && c.after("b").same_f32(&b), || format!("rows: port {r:?}"))
                .and(|| same_floats("errors", c.after("acc"), &fs.acc))
                .and(|| same_floats("working row", c.output("scan"), &fs.scan))
        },
        "SI_pictureMakeGradation" => |c: &Case| {
            let mut p = picture_in(c, c.input("pic").words());
            let v: Vec<f32> = (0..12).map(|i| c.input(&format!("c{i}")).f32()).collect();
            let corners = [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]], [v[9], v[10], v[11]]];
            match gradation::make_gradation(&mut p, corners) {
                Ok(()) => same_picture_after(c, c.after("pic").words(), &p),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_fsDitherScan" => |c: &Case| {
            let w = width(c);
            let n = w as usize;
            let noise = c.input("gl").words()[3].value().unwrap() as i16;
            let mut d = OutputDither { rows: Diffusion { width: w, scan: vec![0.0; 3 * n], acc: c.input("acc").f32s()[..3 * n].to_vec() }, noise: noise != 0, random: Drand48::unseeded() };
            let f = c.input("in").f32s();
            let input: Vec<Rgbazo> = f.chunks_exact(6).map(|p| Rgbazo { rgba: [p[0], p[1], p[2], p[3]], depth: p[4] }).collect();
            let (mut r, mut g, mut b, mut a, mut z) = (vec![0u8; n], vec![0u8; n], vec![0u8; n], vec![0u8; n], vec![0f32; n]);
            if d.scan(&input, [&mut r, &mut g, &mut b, &mut a], &mut z, c.input("amp").f32()).is_err() {
                return Verdict::Skip;
            }
            same_prefix("red", c.output("r"), &r)
                .and(|| same_prefix("green", c.output("g"), &g))
                .and(|| same_prefix("blue", c.output("b"), &b))
                .and(|| same_prefix("alpha", c.output("a"), &a))
                .and(|| same_floats("depth", c.output("z"), &z))
                .and(|| same_floats("errors", c.after("acc"), &d.rows.acc))
                .and(|| same_floats("working row", c.output("scan"), &d.rows.scan))
        },
        _ => return None,
    })
}
