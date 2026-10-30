// SPDX-License-Identifier: MIT
//! Checks for the colour quantiser (`si_imgfmt::quant`).

use crate::util::*;
use si_imgfmt::quant::{self, ColorBox, HistEntry, MapError};
use si_picture::PictureRgba;
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

fn alpha(c: &Case) -> bool {
    c.input("alpha").words()[0].value().unwrap_or(0) != 0
}

/// Histogram entries from 12-byte records.
fn entries(bytes: &[u8]) -> Vec<HistEntry> {
    bytes
        .chunks_exact(12)
        .map(|e| HistEntry { color: [e[0], e[1], e[2], e[3]], count: i32::from_le_bytes([e[4], e[5], e[6], e[7]]), extra: i32::from_le_bytes([e[8], e[9], e[10], e[11]]) })
        .collect()
}

fn flat(list: &[HistEntry]) -> Vec<u8> {
    list.iter().flat_map(|e| e.color.into_iter().chain(e.count.to_le_bytes()).chain(e.extra.to_le_bytes())).collect()
}

/// A box record of nine words.
fn a_box(w: &[Word]) -> ColorBox {
    let v = |i: usize| w[i].value().unwrap_or(0);
    ColorBox {
        index: v(0) as u32 as i32,
        colors: v(1) as u32 as i32,
        sum: v(2) as u32,
        mean: [f64::from_bits(v(3) as u64), f64::from_bits(v(4) as u64), f64::from_bits(v(5) as u64), f64::from_bits(v(6) as u64)],
        variance: f32::from_bits(v(7) as u32),
        total: v(8) as u32,
    }
}

/// Our box against the words the original wrote (those it left alone are not compared).
fn same_box(what: &str, theirs: &[Word], ours: &ColorBox) -> Verdict {
    let o = [
        ours.index as u32 as u128,
        ours.colors as u32 as u128,
        ours.sum as u128,
        ours.mean[0].to_bits() as u128,
        ours.mean[1].to_bits() as u128,
        ours.mean[2].to_bits() as u128,
        ours.mean[3].to_bits() as u128,
        ours.variance.to_bits() as u128,
        ours.total as u128,
    ];
    let mut o = o;
    if mutate() {
        o[1] ^= 1;
    }
    match theirs.iter().zip(o).position(|(t, o)| t.value().is_some_and(|t| t != o)) {
        Some(i) => Verdict::Different(format!("{what} word {i}: original {:#x}, port {:#x}", theirs[i].value().unwrap(), o[i])),
        None => Verdict::Same,
    }
}

fn same_list(what: &str, theirs: &Field, ours: &[HistEntry]) -> Verdict {
    same_bytes(what, &theirs.to_bytes(), &flat(ours))
}

fn histogram(c: &Case) -> Verdict {
    let pw = c.input("p").words();
    let (w, h) = (pw[0].value().unwrap() as usize, pw[1].value().unwrap() as usize);
    let p = PictureRgba { width: w as i16, height: h as i16, ratio: 1.0, fields: 3, pad: 1, data: c.input("px").to_bytes()[..4 * w * h].to_vec() };
    let mut bits: [i32; 4] = c.input("bits").i32s().try_into().unwrap();
    let Ok((mut list, n)) = quant::make_color_hist_vector_list(&p, &mut bits, alpha(c)) else { return Verdict::Skip };
    if mutate() {
        if let Some(e) = list.first_mut() {
            e.count ^= 1;
        }
    }
    Verdict::from(c.output("eax").u32() == 1, || "the original returned nothing".into())
        .and(|| Verdict::from(c.output("n").i32() == n, || format!("count {}, port {n}", c.output("n").i32())))
        .and(|| Verdict::from(c.after("bits").i32s() == bits, || format!("depths {:?}, port {bits:?}", c.after("bits").i32s())))
        .and(|| match c.try_output("list") {
            Some(l) => {
                // six words an entry: four colour bytes, the count, the spare word
                let w = l.values();
                let theirs: Vec<HistEntry> = w.chunks_exact(6).map(|e| HistEntry { color: [e[0] as u8, e[1] as u8, e[2] as u8, e[3] as u8], count: e[4] as u32 as i32, extra: e[5] as u32 as i32 }).collect();
                Verdict::from(theirs == list, || format!("list of {} entries differs from the port's {}", theirs.len(), list.len()))
            }
            None => Verdict::from(list.is_empty(), || "no list".into()),
        })
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_dgColorEquivTable" => |c: &Case| match quant::color_equiv_table(c.input("a").i32(), c.input("b").i32()) {
            Ok(t) => same_bytes("table", &c.output("heap0").to_bytes(), &t),
            Err(_) => Verdict::Skip,
        },
        "makeColorHistVectorList" => histogram,
        "makeColorMap" => |c: &Case| {
            let n = c.input("n").i32();
            let all = c.input("hist").to_bytes();
            let mut hist = entries(&all[..12 * n as usize]);
            let stderr = c.try_output("stderr").map(text).unwrap_or_default();
            match quant::make_color_map(&mut hist, n, c.input("pixels").u32(), c.input("wanted").i32(), c.input("cut").i32(), c.input("method").i32(), alpha(c)) {
                Ok((map, said)) => Verdict::from(c.output("eax").u32() == 1, || "the original returned nothing".into())
                    .and(|| same_bytes("palette", &c.output("map").to_bytes(), &flat(&map)))
                    .and(|| same_bytes("histogram order", &c.after("hist").to_bytes()[..12 * n as usize], &flat(&hist)))
                    .and(|| Verdict::from(stderr == said, || format!("stderr {stderr:?}"))),
                Err(MapError::Fault(_)) => Verdict::Skip,
                Err(e) => Verdict::from(c.output("eax").u32() == 0, || format!("the port gave up: {e:?}"))
                    .and(|| Verdict::from(stderr == if e == MapError::NoBoxes { "Error, unable to allocate boxVector structure\n" } else { "" }, || format!("stderr {stderr:?}"))),
            }
        },
        "boxStats" => |c: &Case| {
            let mut b = a_box(c.input("box").words());
            match quant::box_stats(&mut b, &entries(&c.input("hist").to_bytes()), alpha(c)) {
                Ok(()) => same_box("box", c.after("box").words(), &b),
                Err(_) => Verdict::Skip,
            }
        },
        "medianCutBox" => |c: &Case| {
            let mut b = a_box(c.input("box").words());
            let mut hist = entries(&c.input("hist").to_bytes());
            match quant::median_cut_box(&mut b, &mut hist, alpha(c)) {
                Ok(new) => same_box("box", c.after("box").words(), &b)
                    .and(|| same_box("new box", c.output("new").words(), &new))
                    .and(|| same_list("histogram order", c.after("hist"), &hist)),
                Err(_) => Verdict::Skip,
            }
        },
        "varFindCutPoint" => |c: &Case| {
            let b = a_box(c.input("box").words());
            match quant::var_find_cut_point(&b, c.input("ch").i32() as usize, &entries(&c.input("hist").to_bytes()), alpha(c)) {
                Ok(Some((lo, hi))) => Verdict::from(c.output("eax").u32() == 1, || "the original found no cut".into())
                    .and(|| same_box("low", c.output("lo").words(), &lo))
                    .and(|| same_box("high", c.output("hi").words(), &hi)),
                Ok(None) => Verdict::from(c.output("eax").u32() == 0, || "the original found a cut".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "varCutBox" => |c: &Case| {
            let mut b = a_box(c.input("box").words());
            let mut hist = entries(&c.input("hist").to_bytes());
            match quant::var_cut_box(&mut b, &mut hist, alpha(c)) {
                Ok(Some(new)) => Verdict::from(c.output("ax").u32() & 0xffff == 1, || "the original did not cut".into())
                    .and(|| same_box("box", c.after("box").words(), &b))
                    .and(|| same_box("new box", c.output("new").words(), &new))
                    .and(|| same_list("histogram order", c.after("hist"), &hist)),
                Ok(None) => Verdict::from(c.output("ax").u32() & 0xffff == 0, || "the original cut".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "computeBoxColorRepresentatives" => |c: &Case| {
            let w = c.input("boxes").words();
            let n = c.input("n").i32().max(0) as usize;
            let boxes: Vec<ColorBox> = (0..n).map(|i| a_box(&w[9 * i..9 * i + 9])).collect();
            let mut map = entries(&c.input("map").to_bytes());
            let stderr = c.try_output("stderr").map(text).unwrap_or_default();
            match quant::compute_box_color_representatives(&boxes, &mut map, c.input("method").i32(), &entries(&c.input("hist").to_bytes()), alpha(c)) {
                Ok(r) => same_list("palette", c.after("map"), &map).and(|| Verdict::from(stderr == r.err().unwrap_or(""), || format!("stderr {stderr:?}"))),
                Err(_) => Verdict::Skip,
            }
        },
        "maxVariance" => |c: &Case| {
            let f = c.input("boxes").f32s();
            let boxes: Vec<ColorBox> = (0..c.input("n").i32().max(0) as usize).map(|i| ColorBox { variance: f[13 * i + 11], ..ColorBox::default() }).collect();
            let ours = quant::max_variance(&boxes).map_or(-1, |i| i as i32);
            Verdict::from(c.output("eax").i32() == ours, || format!("port {ours}"))
        },
        "maxNumColors" => |c: &Case| {
            let v = c.input("boxes").i32s();
            let boxes: Vec<ColorBox> = (0..c.input("n").i32().max(0) as usize).map(|i| ColorBox { colors: v[13 * i + 1], ..ColorBox::default() }).collect();
            let ours = quant::max_num_colors(&boxes).map_or(-1, |i| i as i32);
            Verdict::from(c.output("eax").i32() == ours, || format!("port {ours}"))
        },
        "reduceColors" => |c: &Case| {
            let pw = c.input("p").words();
            let (w, h) = (pw[0].value().unwrap() as usize, pw[1].value().unwrap() as usize);
            let px = c.input("px").to_bytes();
            let p = PictureRgba { width: w as i16, height: h as i16, ratio: 1.0, fields: 3, pad: 1, data: px[..4 * w * h].to_vec() };
            let o = c.input("opts").i32s();
            let opts = quant::QuantOptions { colors: o[0], bits: [o[1], o[2], o[3], o[4]] };
            match quant::reduce_colors(&p, &opts, &entries(&c.input("map").to_bytes()), c.input("dither").i16(), alpha(c)) {
                Ok(ix) => {
                    let theirs = c.output("ix").written();
                    let same = (0..theirs.len()).all(|i| if i < ix.len() { theirs[i] == Some(ix[i] as u128) } else { theirs[i].is_none() });
                    Verdict::from(c.output("eax").u32() == 1, || "the original failed".into())
                        .and(|| Verdict::from(same, || format!("indices: port {ix:?}")))
                        .and(|| same_bytes("the picture afterwards", &c.after("px").to_bytes(), &px))
                }
                Err(_) => Verdict::Skip,
            }
        },
        "computeRGBDistance" => |c: &Case| {
            let v: Vec<i32> = (0..8).map(|i| c.input(&format!("v{i}")).i32()).collect();
            let ours = quant::compute_rgb_distance([v[0], v[1], v[2], v[3]], [v[4], v[5], v[6], v[7]]);
            Verdict::from(c.output("eax").i32() == ours, || format!("port {ours}"))
        },
        _ => return None,
    })
}
