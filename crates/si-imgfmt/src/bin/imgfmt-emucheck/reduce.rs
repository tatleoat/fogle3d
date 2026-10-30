// SPDX-License-Identifier: MIT
//! Checks for colour reduction from end to end (`si_imgfmt::reduce`) and the neural
//! quantiser (`si_imgfmt::neuquant`).

use crate::dgt::{dg_in, pal_in, same_dg};
use crate::util::*;
use si_imgfmt::neuquant::{self, Net};
use si_imgfmt::quant::{HistEntry, QuantOptions};
use si_imgfmt::reduce::{self, QuantizeParameters, Reducer};
use si_picture::PictureRgba;
use si_x87::vectors::{Case, Check, Verdict, Word};
use std::cmp::Ordering;

fn word(c: &Case, name: &str) -> i32 {
    c.input(name).words()[0].value().unwrap_or(0) as u32 as i32
}

fn alpha(c: &Case) -> bool {
    word(c, "alpha") != 0
}

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

fn entries(bytes: &[u8]) -> Vec<HistEntry> {
    bytes
        .chunks_exact(12)
        .map(|e| HistEntry { color: [e[0], e[1], e[2], e[3]], count: i32::from_le_bytes([e[4], e[5], e[6], e[7]]), extra: i32::from_le_bytes([e[8], e[9], e[10], e[11]]) })
        .collect()
}

fn flat(list: &[HistEntry]) -> Vec<u8> {
    list.iter().flat_map(|e| e.color.into_iter().chain(e.count.to_le_bytes()).chain(e.extra.to_le_bytes())).collect()
}

fn picture(c: &Case) -> PictureRgba {
    let pw = c.input("pic").words();
    let (w, h) = (pw[0].value().unwrap() as usize, pw[1].value().unwrap() as usize);
    PictureRgba { width: w as i16, height: h as i16, ratio: 1.0, fields: 3, pad: 1, data: c.input("px").to_bytes()[..4 * w * h].to_vec() }
}

fn bits(c: &Case) -> [i32; 4] {
    c.input("bits").i32s().try_into().unwrap()
}

fn parameters(c: &Case) -> QuantizeParameters {
    let q = c.input("q").i32s();
    QuantizeParameters { method: q[0], quality: q[1], cut: q[2], representative: q[3] }
}

/// The globals a case starts with (`options`: colours, depths, masks), if it sets them.
fn reducer(c: &Case) -> Reducer {
    let mut r = Reducer::new(c.try_input("alpha").is_some() && alpha(c));
    if let Some(o) = c.try_input("options") {
        let o = o.i32s();
        r.options = QuantOptions { colors: o[0], bits: [o[1], o[2], o[3], o[4]] };
        r.masks = [o[5], o[6], o[7], o[8]];
    }
    r
}

fn same_options(c: &Case, r: &Reducer) -> Verdict {
    let ours: Vec<u128> = [r.options.colors].into_iter().chain(r.options.bits).chain(r.masks).map(|v| v as u32 as u128).collect();
    same_written("options", c.after("options"), &ours)
}

fn same_counts(c: &Case, r: &Reducer) -> Verdict {
    match (c.try_output("count"), &r.count_reduce) {
        (Some(f), Some(n)) => same_written("use counts", f, &n.iter().flatten().map(|&v| v as u128).collect::<Vec<_>>()),
        (None, None) => Verdict::Same,
        (None, Some(n)) => Verdict::from(n.is_empty(), || "no use counts in the original".into()),
        (Some(_), None) => Verdict::Different("use counts in the original only".into()),
    }
}

fn same_map(c: &Case, ours: &[HistEntry]) -> Verdict {
    match c.try_output("map") {
        Some(m) => same_bytes("colour list", &m.to_bytes(), &flat(ours)),
        None => Verdict::Different("the original returned no list".into()),
    }
}

fn stderr(c: &Case) -> String {
    c.try_output("stderr").map(text).unwrap_or_default()
}

fn locks(c: &Case, with_pal: bool) -> Verdict {
    let all = c.input("map").to_bytes();
    let mut map = entries(&all);
    let pal = with_pal.then(|| pal_in(c, "pal"));
    match reduce::palette_integrate_locks(Some(&mut map), word(c, "n"), &bits(c), pal.as_ref(), word(c, "locks"), word(c, "reserve") as i16) {
        Ok(done) => Verdict::from((eax(c) != 0) == done, || format!("returned {}, port {done}", eax(c))).and(|| same_bytes("colour list", &c.after("map").to_bytes(), &flat(&map))),
        Err(_) => Verdict::Skip,
    }
}

fn color_reduce(c: &Case, with_pal: bool) -> Verdict {
    let mut r = reducer(c);
    let mut b = bits(c);
    let pal = with_pal.then(|| pal_in(c, "pal"));
    match r.color_reduce(&picture(c), word(c, "colors"), &mut b, pal.as_ref(), word(c, "dither") as i16, word(c, "reserve") as i16, parameters(c), word(c, "use") != 0) {
        Ok((Some(d), said)) => Verdict::from(eax(c) == 1, || "no picture".into())
            .and(|| same_dg(c, "n", &d))
            .and(|| same_counts(c, &r))
            .and(|| Verdict::from(c.after("bits").i32s() == b, || format!("depths {:?}, port {b:?}", c.after("bits").i32s())))
            .and(|| Verdict::from(stderr(c) == said, || format!("stderr {:?}", stderr(c)))),
        Ok((None, said)) => Verdict::from(eax(c) == 0, || "a picture".into()).and(|| Verdict::from(stderr(c) == said, || format!("stderr {:?}", stderr(c)))),
        Err(_) => Verdict::Skip,
    }
}

// ---------------------------------------------------------------- the network

fn net_in(c: &Case) -> Net {
    let size = word(c, "netsize");
    let rows = c.input("rows").i32s();
    let n = size.max(0) as usize;
    Net {
        size,
        alpha: alpha(c),
        channels: word(c, "nch"),
        network: rows.chunks_exact(5).take(n).map(|r| [r[0], r[1], r[2], r[3], r[4]]).collect(),
        bias: c.input("biasv").i32s()[..n].to_vec(),
        freq: c.input("freqv").i32s()[..n].to_vec(),
        radpower: c.input("radv").i32s(),
        picture: c.try_input("sample").map(|f| f.to_bytes()).unwrap_or_default(),
        lengthcount: c.try_input("lengthcount").map_or(0, |f| f.i32()),
        samplefac: c.try_input("samplefac").map_or(0, |f| f.i32()),
        alphadec: 0,
    }
}

/// Our network against the buffers as the routine left them; rows and words beyond the
/// network's size are the input's.
fn same_net(c: &Case, net: &Net) -> Verdict {
    let over = |name: &str, ours: &[i32]| {
        let mut all = c.input(name).i32s();
        all[..ours.len()].copy_from_slice(ours);
        let mut all: Vec<u128> = all.iter().map(|&v| v as u32 as u128).collect();
        if mutate() && !all.is_empty() {
            all[0] ^= 1;
        }
        let theirs = c.after(name).written();
        match theirs.iter().zip(&all).position(|(t, o)| t.is_some_and(|t| t != *o)) {
            Some(i) => Verdict::Different(format!("{name} word {i}: original {:#x}, port {:#x}", theirs[i].unwrap(), all[i])),
            None => Verdict::Same,
        }
    };
    let rows: Vec<i32> = net.network.iter().flatten().copied().collect();
    over("rows", &rows).and(|| over("biasv", &net.bias)).and(|| over("freqv", &net.freq)).and(|| over("radv", &net.radpower))
}

fn sample(c: &Case) -> [i32; 4] {
    ["b", "g", "r", "a"].map(|n| word(c, n))
}

fn ordering(v: u32) -> Ordering {
    (v as i32).cmp(&0)
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_convertColorMap" => |c: &Case| {
            let mut r = reducer(c);
            match r.convert_color_map(&pal_in(c, "pal")) {
                Ok(Some(m)) => Verdict::from(eax(c) == 1, || "no list".into()).and(|| same_map(c, &m)).and(|| same_options(c, &r)),
                Ok(None) => Verdict::from(eax(c) == 0, || "a list".into()).and(|| same_options(c, &r)),
                Err(_) => Verdict::Skip,
            }
        },
        "si_paletteIntegrateLocks" => |c: &Case| locks(c, true),
        "si_paletteIntegrateLocks#nopal" => |c: &Case| locks(c, false),
        "SI_generateColorMap" => |c: &Case| {
            let mut r = reducer(c);
            let mut b = bits(c);
            match r.generate_color_map(&picture(c), word(c, "colors"), word(c, "reserve"), &mut b, parameters(c)) {
                Ok((map, said)) => match map {
                    Some(m) => Verdict::from(eax(c) == 1, || "no list".into()).and(|| same_map(c, &m)),
                    None => Verdict::from(eax(c) == 0, || "a list".into()),
                }
                .and(|| same_options(c, &r))
                .and(|| Verdict::from(c.after("bits").i32s() == b, || format!("depths {:?}, port {b:?}", c.after("bits").i32s())))
                .and(|| Verdict::from(stderr(c) == said, || format!("stderr {:?}", stderr(c)))),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_pictureRgb2ColorIndex" => |c: &Case| {
            let mut r = reducer(c);
            let map = entries(&c.input("map").to_bytes());
            match r.picture_rgb2color_index(&picture(c), &map, word(c, "dither") as i16, word(c, "reserve"), &bits(c), word(c, "most")) {
                Ok(Some(d)) => Verdict::from(eax(c) == 1, || "no picture".into()).and(|| same_dg(c, "n", &d)).and(|| same_counts(c, &r)),
                Ok(None) => Verdict::from(eax(c) == 0, || "a picture".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "si_paletteFilterRedundancy" => |c: &Case| {
            let mut r = reducer(c);
            let mut d = dg_in(c);
            match r.palette_filter_redundancy(Some(&mut d)) {
                Ok(()) => same_dg(c, "n", &d).and(|| same_counts(c, &r)),
                Err(_) => Verdict::Skip,
            }
        },
        "si_paletteCountEffectiveEntries" => |c: &Case| match reducer(c).palette_count_effective_entries(Some(&dg_in(c))) {
            Ok(n) => Verdict::from(eax(c) as i32 == n ^ mutate() as i32, || format!("{}, port {n}", eax(c))),
            Err(_) => Verdict::Skip,
        },
        "calculeColorCount" => |c: &Case| match Reducer::calcule_color_count(&dg_in(c)) {
            Ok(n) => {
                let mut r = Reducer::new(false);
                r.count_reduce = n;
                Verdict::from((eax(c) == 1) == r.count_reduce.is_some(), || "one has counts, one has not".into()).and(|| same_counts(c, &r))
            }
            Err(_) => Verdict::Skip,
        },
        "si_resetGlobalColorCount" => |c: &Case| {
            let mut r = Reducer::new(false);
            match r.reset_global_color_count(&dg_in(c)) {
                Ok(()) => same_counts(c, &r),
                Err(_) => Verdict::Skip,
            }
        },
        "si_totalCouleursRGB" => |c: &Case| match reducer(c).total_couleurs_rgb(&picture(c)) {
            Ok(n) => Verdict::from(eax(c) as i32 == n ^ mutate() as i32, || format!("{}, port {n}", eax(c))),
            Err(_) => Verdict::Skip,
        },
        "SI_reduceImage" => |c: &Case| {
            let mut r = reducer(c);
            match r.reduce_image(Some(&picture(c)), word(c, "reserve"), word(c, "dither"), Some(&pal_in(c, "pal"))) {
                Ok(Some(d)) => Verdict::from(eax(c) == 1, || "no picture".into()).and(|| same_dg(c, "n", &d)).and(|| same_counts(c, &r)),
                Ok(None) => Verdict::from(eax(c) == 0, || "a picture".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_colorReduce" => |c: &Case| color_reduce(c, true),
        "SI_colorReduce#nopal" => |c: &Case| color_reduce(c, false),
        "SI_savePalette" => |c: &Case| {
            let name = c.try_input("name").filter(|f| !f.is_null()).map(|f| f.to_bytes());
            let kind = c.try_input("kind").filter(|f| !f.is_null()).map(|f| f.to_bytes());
            match reduce::save_palette(name.as_deref(), kind.as_deref(), Some(&dg_in(c))) {
                Ok(Some(b)) => Verdict::from(c.output("al").u32() & 0xff == 1, || "the original failed".into()).and(|| same_bytes("file", &file_out(c, "/t/o.x").unwrap_or_default(), &b)),
                Ok(None) => Verdict::from(c.output("al").u32() & 0xff == 0 && file_out(c, "/t/o.x").is_none(), || "the original wrote".into()),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_computePalette" => |c: &Case| {
            let mut r = reducer(c);
            let mut b = bits(c);
            let count = c.input("list").words()[0].value().unwrap_or(0) as u16 as i16;
            let names: Vec<Vec<u8>> = (0..count.max(0) as usize).map(|i| cstr(&c.input(&format!("nm{i}")).to_bytes()).to_vec()).collect();
            let names: Vec<&[u8]> = names.iter().map(|n| &n[..]).collect();
            let mut load = |name: &[u8]| -> Option<PictureRgba> {
                let i = (name.last()? - b'0') as usize;
                let d = c.input(&format!("load{i}")).to_bytes();
                let (w, h) = (i16::from_le_bytes([d[0], d[1]]), i16::from_le_bytes([d[2], d[3]]));
                (w > 0).then(|| PictureRgba { width: w, height: h, ratio: 1.0, fields: 3, pad: 1, data: d[4..4 + 4 * w as usize * h as usize].to_vec() })
            };
            let pal = pal_in(c, "pal");
            let (colors, reserve) = (word(c, "colors"), word(c, "reserve"));
            match r.compute_palette(&names, &mut load, colors, &mut b, parameters(c), reserve, Some(&pal)) {
                Ok((made, said)) => match made {
                    // with a reserved entry the last one is filled from behind the list
                    Some(p) => Verdict::from(eax(c) == 1, || "no palette".into()).and(|| crate::dgt::same_pal_upto(c, "n", &p, if reserve != 0 { (colors - 1).max(0) as usize } else { usize::MAX })),
                    None => Verdict::from(eax(c) == 0, || "a palette".into()),
                }
                .and(|| Verdict::from(messages(c) == said, || format!("messages {:?}, port {said:?}", messages(c)))),
                Err(_) => Verdict::Skip,
            }
        },
        "SI_getPaletteFromPictureRGBA" => |c: &Case| {
            let mut r = reducer(c);
            let a = c.input("asked").i32s();
            let asked = (a[0] != 0).then(|| (a[1], [a[2], a[3], a[4], a[5]], QuantizeParameters { method: a[6], quality: a[7], cut: a[8], representative: a[9] }));
            match r.get_palette_from_picture_rgba(&picture(c), Some(&pal_in(c, "pal")), asked) {
                Ok((made, said)) => match made {
                    Some(p) => Verdict::from(eax(c) == 1, || "no palette".into()).and(|| crate::dgt::same_pal(c, "n", &p)),
                    None => Verdict::from(eax(c) == 0, || "a palette".into()),
                }
                .and(|| Verdict::from(stderr(c) == said, || format!("stderr {:?}", stderr(c)))),
                Err(_) => Verdict::Skip,
            }
        },
        "contest" => |c: &Case| {
            let mut net = net_in(c);
            match net.contest(sample(c)) {
                Ok(i) => Verdict::from(eax(c) as i32 == i, || format!("neuron {}, port {i}", eax(c) as i32)).and(|| same_net(c, &net)),
                Err(_) => Verdict::Skip,
            }
        },
        "altersingle" => |c: &Case| {
            let mut net = net_in(c);
            match net.altersingle(word(c, "al"), word(c, "i"), sample(c)) {
                Ok(()) => same_net(c, &net),
                Err(_) => Verdict::Skip,
            }
        },
        "alterneigh" => |c: &Case| {
            let mut net = net_in(c);
            match net.alterneigh(word(c, "rad"), word(c, "i"), sample(c)) {
                Ok(()) => same_net(c, &net),
                Err(_) => Verdict::Skip,
            }
        },
        "unbiasnet" => |c: &Case| {
            let mut net = net_in(c);
            match net.unbiasnet() {
                Ok(()) => same_net(c, &net),
                Err(_) => Verdict::Skip,
            }
        },
        "initnet" => |c: &Case| {
            let (size, len, fac) = (word(c, "size"), word(c, "len"), word(c, "fac"));
            let Ok(net) = Net::initnet(size, Vec::new(), len, fac, alpha(c), 0) else { return Verdict::Skip };
            let g = c.output("g").words();
            let width = c.output("width").u32() as usize;
            let rows: Vec<u128> = net.network.iter().flat_map(|r| r[..width].to_vec()).map(|v| v as u32 as u128).collect();
            let wide32 = |v: &[i32]| v.iter().map(|&x| x as u32 as u128).collect::<Vec<_>>();
            Verdict::from(g[1].value() == Some(size as u32 as u128) && g[2].value() == Some((size - 1) as u32 as u128), || "size".into())
                .and(|| Verdict::from(matches!(g[3], Word::Ref { .. }) && g[4].value() == Some(len as u32 as u128) && g[5].value() == Some(fac as u32 as u128), || "sample globals".into()))
                .and(|| Verdict::from(width == if alpha(c) { 5 } else { 4 }, || format!("rows of {width} words")))
                .and(|| same_written("network", c.output("net"), &rows))
                .and(|| same_written("bias", c.output("nbias"), &wide32(&net.bias)))
                .and(|| same_written("frequency", c.output("nfreq"), &wide32(&net.freq)))
                .and(|| match c.try_output("nrad") {
                    Some(r) => same_written("weights", r, &wide32(&net.radpower)),
                    None => Verdict::from(net.radpower.is_empty(), || "no weights in the original".into()),
                })
        },
        "learn" => |c: &Case| {
            let mut net = net_in(c);
            match net.learn() {
                Ok(done) => Verdict::from((c.output("ax").u32() & 0xffff == 1) == done, || "interrupted".into())
                    .and(|| same_net(c, &net))
                    .and(|| Verdict::from(c.output("alphadec").i32() == net.alphadec, || format!("alphadec {}, port {}", c.output("alphadec").i32(), net.alphadec))),
                Err(_) => Verdict::Skip,
            }
        },
        "Neural_makeColorMap" => |c: &Case| {
            let ch = word(c, "nch");
            let n = word(c, "n");
            let all = c.input("pic").to_bytes();
            let len = (n.max(0) as usize * ch.max(0) as usize).min(all.len());
            match neuquant::neural_make_color_map(all[..len].to_vec(), n, word(c, "size"), word(c, "quality"), alpha(c), ch) {
                Ok(Ok((map, _))) => Verdict::from(eax(c) == 1, || "no palette".into()).and(|| same_map(c, &map)),
                Ok(Err(said)) => Verdict::from(eax(c) == 0 && stderr(c) == said, || format!("stderr {:?}", stderr(c))),
                Err(_) => Verdict::Skip,
            }
        },
        "colormapCompare" => |c: &Case| {
            let (a, b) = (entries(&c.input("a").to_bytes())[0], entries(&c.input("b").to_bytes())[0]);
            let ours = neuquant::colormap_compare(&a, &b, alpha(c));
            let ours = if mutate() { ours.reverse().then(Ordering::Less) } else { ours };
            Verdict::from(ordering(eax(c)) == ours, || format!("{}, port {ours:?}", eax(c) as i32))
        },
        "histoCompare" => |c: &Case| {
            let e = |n: &str| {
                let w = c.input(n).i32s();
                HistEntry { color: (w[0] as u32).to_le_bytes(), count: w[1], extra: w[2] }
            };
            let ours = neuquant::histo_compare(&e("a"), &e("b"));
            let ours = if mutate() { ours.reverse().then(Ordering::Less) } else { ours };
            Verdict::from(ordering(eax(c)) == ours, || format!("{}, port {ours:?}", eax(c) as i32))
        },
        "si_neuralMakeColorMap" => |c: &Case| {
            let n = word(c, "n");
            let all = c.input("hist").to_bytes();
            let mut hist = entries(&all);
            match neuquant::si_neural_make_color_map(&mut hist, n, c.input("pixels").u32(), &bits(c), word(c, "wanted"), word(c, "quality"), alpha(c)) {
                Ok(Some(map)) => Verdict::from(eax(c) == 1, || "no palette".into()).and(|| same_map(c, &map)).and(|| same_bytes("histogram order", &c.after("hist").to_bytes(), &flat(&hist))),
                Ok(None) => Verdict::from(eax(c) == 0, || "a palette".into()),
                Err(_) => Verdict::Skip,
            }
        },
        _ => return None,
    })
}
