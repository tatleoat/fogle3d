// SPDX-License-Identifier: MIT
//! The checks of the second form on the memory image and of the conversion to triangles
//! (finding 0025, section 13), on the vectors of `re/tools/si-nurbs/gen_convert.py` (its
//! group `mov`: `SI_nurbsCrvMovePoint`'s image form, answered in `eax`; group `len`:
//! `SI_nurbsCurveLength` and `SI_nurbsDistanceToFactor`, answered in st0, compared in all
//! 80 bits; group `upd`: `SI_nurbsCurveUpdateLength` with `SI_nurbsCrvAnimUpdate`, the
//! tables read through the curve as `t.lp`, `t.ld`, the calls into the animation package
//! compared by name and argument words; group `cpy`: `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy`,
//! `SI_nurbsSrfDispose`, `SI_nurbsSrfMovePoint`, `SI_nurbsSrfTransferPointsKnots` on the image,
//! a new curve or surface read through the answer as `n.…` down to its composites' curves,
//! the stubbed calls `SI_genTreeDestroy`, `markDirtyCcrvs`, `SI_nurbsSrfForceCapping`
//! compared like the animation package's; group `lin`: `SI_knotsCrvReparametrization` and
//! `SI_nurbsCrvConvertToLinear` on the image, the curve changed in place read through the
//! case's buffer as `o.…`, a new one through the answer as `n.…`; group `inv`:
//! `SI_nurbsCrvInverse` on the image, answered in `eax`, the curve read as `o.…`, the index
//! table `idx` where `SI_gblIndex` was set, the animation package's two calls compared; group `app`: `SI_nurbsCrvApplyMatrix` on the
//! image, no answer, the curve read as `o.…`, the matrix `m` unchanged):
//!
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec13)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec14)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec15)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec16)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec17)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec18)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec19)
//!   (ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec20)
//!
//! A case is loaded into a fresh `si_mem::Mem` (the harness's addresses, its heap handing
//! out blocks in its order): every input buffer through `si_mem::load_case`, a buffer the
//! original only writes as fresh memory of the output's size, a buffer named `G<address>`
//! at that address (a global). The port runs on the image with the case's arguments.
//! Compared afterwards: every output buffer word for word (a pointer into a block of the
//! heap is compared as the address the port's own block of that number has), every input
//! buffer the original left alone (it must be unchanged), what the routine made as the
//! generator read it after the call (`vb`, `tb`: the vertex and triangle blocks; `nb`: a new
//! array; `n.rec`, `n.knots`, `n.pts`: a new curve), the routine's answer, and the blocks
//! given back to the heap, in order.
//!
//! `EMUCHECK_MUTATE=1`: one bit of the first byte of the first buffer compared (or of the
//! answer) is flipped first; `EMUCHECK_MUTATE=calls`: the last block given back is dropped
//! from the port's list (or one is added). Every compared case must then differ.

use si_mem::{Mem, Names, Word as W};
use si_nurbs::{convert, image};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

fn word(w: &Word) -> W {
    match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    }
}

fn words(f: &Field) -> Vec<W> {
    f.words().iter().map(word).collect()
}

fn global_addr(name: &str) -> Option<u32> {
    let hex = name.strip_prefix('G')?;
    if hex.len() != 8 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// Outputs that are not buffers of the case: the answer, the freed list, what the generator
/// read through pointers after the call.
fn is_result(name: &str) -> bool {
    matches!(name, "eax" | "ax" | "al" | "st0" | "ptr" | "freed" | "stdout" | "stderr" | "vb" | "tb" | "nb" | "r") || name.starts_with("n.") || name.starts_with("o.") || name.starts_with("t.") || name.starts_with("heap")
}

fn mode() -> Option<String> {
    std::env::var("EMUCHECK_MUTATE").ok().filter(|v| !v.is_empty() && v != "0")
}

struct Loaded {
    m: Mem,
    names: Names,
    /// The calls into the animation package (the touch of a keyframed curve), answered as
    /// the generator's stubs answer them.
    o: si_mem::Trace,
}

/// What the generator's stub of `SI_findNURBSPathFcurveOwner` answers.
const OWNER: u32 = 0x5150;

fn load(c: &Case) -> Result<Loaded, String> {
    let mut m = Mem::new();
    let mut held: Vec<(String, Option<Vec<W>>)> = Vec::new();
    for f in &c.inputs {
        if global_addr(&f.name).is_some() {
            continue;
        }
        held.push((f.name.clone(), if f.is_null() { None } else { Some(words(f)) }));
    }
    for f in &c.outputs {
        if global_addr(&f.name).is_some() || is_result(&f.name) || c.try_input(&f.name).is_some() {
            continue;
        }
        held.push((f.name.clone(), Some(f.words().iter().map(|w| W::Unwritten { bytes: w.bytes() as u8 }).collect())));
    }
    let fields: Vec<(&str, Option<&[W]>)> = held.iter().map(|(n, w)| (n.as_str(), w.as_deref())).collect();
    let mut names = si_mem::load_case(&mut m, &fields)?;
    for f in c.inputs.iter().chain(&c.outputs) {
        if let Some(a) = global_addr(&f.name) {
            if names.addr(&m, &f.name).is_none() {
                names.add(&f.name, a, si_mem::words_len(&words(f)));
            }
        }
    }
    for f in &c.inputs {
        if let Some(a) = global_addr(&f.name) {
            m.load_words(a, &words(f), &names)?;
        }
    }
    let o = si_mem::Trace::with(|_, r, _| si_mem::Ret { eax: if r.name == "SI_findNURBSPathFcurveOwner" { OWNER } else { 0 }, ..si_mem::Ret::NONE });
    Ok(Loaded { m, names, o })
}

impl Loaded {
    /// A pointer argument: the case's buffer of that name (null for a null field), or, for a
    /// field of one word, the pointer that word holds.
    fn ptr(&self, c: &Case, name: &str) -> u32 {
        let Some(f) = c.try_input(name) else {
            // a buffer the original only writes
            return self.names.addr(&self.m, name).unwrap_or(0);
        };
        if f.is_null() {
            return 0;
        }
        if f.words().len() == 1 && name == "shape" {
            return match &f.words()[0] {
                Word::Ref { name, offset } => self.names.addr(&self.m, name).unwrap_or(0).wrapping_add(*offset),
                Word::Value { value, .. } => *value as u32,
                Word::Unwritten { .. } => 0,
            };
        }
        self.names.addr(&self.m, name).unwrap_or(0)
    }
}

fn value(c: &Case, name: &str) -> u64 {
    c.input(name).words().first().and_then(|w| w.value()).unwrap_or(0) as u64
}

/// What the port answered.
enum Answer {
    None,
    Ptr(u32),
    Eax(u32),
    St0(si_x87::F80),
}

fn run(name: &str, c: &Case, l: &mut Loaded) -> Result<Answer, image::Halt> {
    let p = |l: &Loaded, n: &str| l.ptr(c, n);
    let i32_ = |n: &str| value(c, n) as u32 as i32;
    let f64_ = |n: &str| f64::from_bits(value(c, n));
    match name {
        "SI_nurbs2Triangle" => {
            let a = [p(l, "shape"), p(l, "srf"), p(l, "mats"), p(l, "nv"), p(l, "verts"), p(l, "nt"), p(l, "tris")];
            convert::nurbs2triangle(&mut l.m, a[0], a[1], a[2], i32_("elem"), a[3], a[4], a[5], a[6], value(c, "flag") as u16 as i16)?;
            Ok(Answer::None)
        }
        "EvaluateNonTrimmedNurbsFromBaseGeometry" => {
            let a = [p(l, "shape"), p(l, "srf"), p(l, "verts"), p(l, "us"), p(l, "vs")];
            convert::evaluate_non_trimmed_from_base(&mut l.m, a[0], a[1], a[2], i32_("nu"), i32_("nv"), a[3], a[4])?;
            Ok(Answer::None)
        }
        "SI_SampleKnotVector" => {
            let a = [p(l, "dir"), p(l, "array"), p(l, "count")];
            image::sample_knot_vector(&mut l.m, a[0], i32_("per"), a[1], a[2])?;
            Ok(Answer::None)
        }
        "repeatKnotsInSampling" => {
            let a = [p(l, "dir"), p(l, "count"), p(l, "array")];
            convert::repeat_knots_in_sampling(&mut l.m, a[0], a[1], a[2]);
            Ok(Answer::None)
        }
        "SI_nurbsSrfExtractCurve" => {
            let s = p(l, "srf");
            Ok(Answer::Ptr(image::srf_extract_curve(&mut l.m, s, f64_("t"), i32_("dir"), i32_("nd"))?))
        }
        "SI_nurbsCrvValue4D" => {
            let (r, crv) = (p(l, "ret"), p(l, "crv"));
            let v = image::crv_value4d(&mut l.m, crv, f64_("t"))?;
            l.m.write(si_mem::Ptr32::new(r), v);
            Ok(Answer::None)
        }
        "getPointInterval" => {
            let a = [p(l, "dir"), p(l, "first"), p(l, "last"), p(l, "count"), p(l, "start")];
            si_nurbs::misc::get_point_interval(&mut l.m, a[0], i32_("i"), a[1], a[2], a[3], a[4])?;
            Ok(Answer::None)
        }
        "SI_nurbsCrvMovePoint" => {
            let crv = p(l, "crv");
            Ok(Answer::Eax(image::crv_move_point(&mut l.m, crv, i32_("i"), f64_("x"), f64_("y"), f64_("z")) as u32))
        }
        "SI_nurbsCrvApplyMatrix" => {
            let (crv, mat) = (p(l, "crv"), p(l, "m"));
            image::crv_apply_matrix(&mut l.m, crv, mat, i32_("mask"));
            Ok(Answer::None)
        }
        "SI_nurbsSrfApplyMatrix" => {
            let (srf, mat) = (p(l, "srf"), p(l, "m"));
            image::srf_apply_matrix(&mut l.m, &mut l.o, srf, mat, i32_("mask"));
            Ok(Answer::None)
        }
        "SI_nurbsSrfForceCapping" => {
            let srf = p(l, "srf");
            image::srf_force_capping(&mut l.m, &mut l.o, srf, value(c, "dir") as u16 as i16, value(c, "mode") as u16 as i16);
            Ok(Answer::None)
        }
        "SI_crvDeleteTagPoints" => {
            let crv = p(l, "crv");
            Ok(Answer::Eax(image::crv_delete_tag_points(&mut l.m, &mut l.o, crv)? as u32))
        }
        "SI_nurbsCrvDeletePoint" => {
            let crv = p(l, "crv");
            image::crv_delete_point(&mut l.m, &mut l.o, crv, i32_("i"))?;
            Ok(Answer::None)
        }
        "SI_nurbsSrfDeleteRow" => {
            let srf = p(l, "srf");
            Ok(Answer::Eax(image::srf_delete_row(&mut l.m, &mut l.o, srf, i32_("i"), i32_("dir"))? as u32))
        }
        "SI_nurbsCrvAddPoint" => {
            let crv = p(l, "crv");
            image::crv_add_point(&mut l.m, &mut l.o, crv, i32_("i"), f64_("t"), f64_("x"), f64_("y"), f64_("z"), f64_("w"))?;
            Ok(Answer::None)
        }
        "knotsWithoutMultiplicity" => {
            let (k, cnt) = (p(l, "knots"), p(l, "cnt"));
            Ok(Answer::Ptr(image::knots_without_multiplicity(&mut l.m, k, i32_("n"), cnt)))
        }
        "SI_srfHitDirAndVal" => {
            let (srf, dir, val) = (p(l, "srf"), p(l, "dir"), p(l, "val"));
            image::srf_hit_dir_and_val(&mut l.m, srf, i32_("index"), dir, val, value(c, "second") as u16)?;
            Ok(Answer::None)
        }
        "SI_nurbsSrfIndHit" => {
            let (srf, a, b, val) = (p(l, "srf"), p(l, "a"), p(l, "b"), p(l, "val"));
            image::srf_ind_hit(&mut l.m, &mut l.o, srf, i32_("index"), value(c, "second") as u16 as i16, a, b, val)?;
            Ok(Answer::None)
        }
        "SI_nurbsCurveLength" => {
            let crv = p(l, "crv");
            Ok(Answer::St0(image::nurbs_curve_length(&mut l.m, &mut l.o, crv)?))
        }
        "SI_nurbsDistanceToFactor" => {
            let crv = p(l, "crv");
            Ok(Answer::St0(image::nurbs_distance_to_factor(&mut l.m, &mut l.o, crv, f64_("d"))?))
        }
        "SI_nurbsCurveUpdateLength" => {
            let crv = p(l, "crv");
            image::nurbs_curve_update_length(&mut l.m, &mut l.o, crv, i32_("samples"), value(c, "flag") as u16 as i16)?;
            Ok(Answer::None)
        }
        "SI_nurbsCrvCopy" => {
            let crv = p(l, "crv");
            Ok(Answer::Ptr(image::crv_copy(&mut l.m, crv)))
        }
        "SI_nurbsCrvConvertToLinear" => {
            let crv = p(l, "crv");
            Ok(Answer::Ptr(image::crv_convert_to_linear(&mut l.m, crv)?))
        }
        "SI_nurbsCrvInverse" => {
            let crv = p(l, "crv");
            Ok(Answer::Eax(image::crv_inverse(&mut l.m, &mut l.o, crv) as u32))
        }
        "SI_knotsCrvReparametrization" => {
            let crv = p(l, "crv");
            image::knots_crv_reparametrization(&mut l.m, crv)?;
            Ok(Answer::None)
        }
        "SI_nurbsSrfCopy" => {
            let srf = p(l, "srf");
            Ok(Answer::Ptr(image::srf_copy(&mut l.m, &mut l.o, srf)))
        }
        "SI_nurbsSrfDispose" => {
            let pp = p(l, "pp");
            image::srf_dispose(&mut l.m, &mut l.o, pp);
            Ok(Answer::None)
        }
        "SI_nurbsSrfMovePoint" => {
            let srf = p(l, "srf");
            Ok(Answer::Eax(image::srf_move_point(&mut l.m, &mut l.o, srf, i32_("i"), f64_("x"), f64_("y"), f64_("z"), f64_("w")) as u32))
        }
        "SI_nurbsSrfTransferPointsKnots" => {
            let (src, dst) = (p(l, "srf"), p(l, "srf2"));
            image::srf_transfer_points_knots(&mut l.m, &mut l.o, src, dst);
            Ok(Answer::None)
        }
        "SI_nurbsCrvValue" => {
            let (r, crv) = (p(l, "ret"), p(l, "crv"));
            let v = image::crv_value(&mut l.m, crv, f64_("t"), i32_("nd"))?;
            for (k, x) in v.iter().enumerate() {
                l.m.set_f64(r + 8 * k as u32, *x);
            }
            Ok(Answer::None)
        }
        _ => unreachable!(),
    }
}

/// Where a field the generator read after the call lies in the port's image.
fn read_at(m: &Mem, names: &Names, answer: &Answer, field: &str, srf: bool) -> Option<u32> {
    let through = |buf: &str| names.addr(m, buf).map(|a| m.u32(a));
    match field {
        "vb" => through("verts"),
        "tb" => through("tris"),
        "nb" => through("array"),
        "r" => match answer {
            Answer::Ptr(p) => Some(*p),
            _ => None,
        },
        // the length tables, through the curve's record
        "t.lp" => names.addr(m, "crv").map(|c| m.u32(c + 0x54)),
        "t.ld" => names.addr(m, "crv").map(|c| m.u32(c + 0x58)),
        // a curve the routine changed in place, read through the case's buffer
        _ if field.starts_with("o.") => match names.addr(m, "crv") {
            Some(c) => through_new(m, c, &field[2..], false),
            None => names.addr(m, "srf").and_then(|c| through_new(m, c, &field[2..], true)),
        },
        _ if field.starts_with("n.") => {
            let Answer::Ptr(c) = answer else { return None };
            through_new(m, *c, &field[2..], srf)
        }
        _ => None,
    }
}

/// A field of the dump of what a routine made (`emu_gen.py`'s `dump_srf`, `dump_comp`,
/// `dump_crv`), read through the record at `a`: a surface's `rec`, `ku`, `kv`, `pts`, `lines`,
/// trims `t<i>.…` and projected curves `p<i>.…`; a composite's `rec` and curves `c<i>.…`; a
/// curve's `rec`, `knots`, `pts`, `lp`, `ld`. `srf`: the answer is a surface.
fn through_new(m: &Mem, a: u32, path: &str, srf: bool) -> Option<u32> {
    let parts: Vec<&str> = path.split('.').collect();
    let nth = |s: &str, tag: char| s.strip_prefix(tag).and_then(|n| n.parse::<u32>().ok());
    // a surface's own fields and its two lists
    let srf_field = |p: &str| match p {
        "ku" => Some(0x0c),
        "kv" => Some(0x30),
        "lines" => Some(0x84),
        "pts" => Some(0x54),
        _ => None,
    };
    let crv_field = |p: &str| match p {
        "rec" => None,
        "knots" => Some(0x0c),
        "pts" => Some(0x2c),
        "lp" => Some(0x54),
        "ld" => Some(0x58),
        _ => None,
    };
    let crv_at = |c: u32, p: &str| if p == "rec" { Some(c) } else { crv_field(p).map(|o| m.u32(c + o)) };
    match parts.as_slice() {
        [p] if srf && srf_field(p).is_some() => Some(m.u32(a + srf_field(p)?)),
        [p] => crv_at(a, p),
        [t, rest @ ..] if nth(t, 't').is_some() || nth(t, 'p').is_some() => {
            let (arr, i) = match nth(t, 't') { Some(i) => (0x64, i), None => (0x6c, nth(t, 'p')?) };
            let comp = m.u32(m.u32(a + arr).wrapping_add(4 * i));
            match rest {
                ["rec"] => Some(comp),
                [c, p] => crv_at(m.u32(m.u32(comp + 8).wrapping_add(4 * nth(c, 'c')?)), p),
                _ => None,
            }
        }
        _ => None,
    }
}

fn compare(c: &Case, l: &mut Loaded, answer: Answer) -> Verdict {
    let mode = mode();
    let mut first = true;
    // the answer
    if let Ok(f) = std::panic::catch_unwind(|| c.output("ptr").clone()) {
        let Answer::Ptr(mut got) = answer else { return Verdict::Different("no answer from the port".into()) };
        if mode.as_deref() == Some("1") {
            got ^= 1;
            first = false;
        }
        let want = match &f.words()[0] {
            Word::Ref { name, offset } => l.names.addr(&l.m, name).map(|a| a.wrapping_add(*offset)),
            Word::Value { value, .. } => Some(*value as u32),
            Word::Unwritten { .. } => None,
        };
        if want.is_some_and(|w| w != got) {
            return Verdict::Different(format!("answer: original {want:x?}, port {got:#x}"));
        }
    }
    // a value the routine returned in eax
    if let Some(f) = c.try_output("eax") {
        let Answer::Eax(mut got) = answer else { return Verdict::Different("no answer from the port".into()) };
        if first && mode.as_deref() == Some("1") {
            got ^= 1;
            first = false;
        }
        let want = f.words().first().and_then(|w| w.value()).map(|v| v as u32);
        if want != Some(got) {
            return Verdict::Different(format!("eax: original {want:x?}, port {got:#x}"));
        }
    }
    // a value the routine left in st0, all 80 bits
    if let Some(f) = c.try_output("st0") {
        let Answer::St0(mut got) = answer else { return Verdict::Different("no answer from the port".into()) };
        if first && mode.as_deref() == Some("1") {
            let (mant, se) = got.to_bits();
            got = si_x87::F80::from_bits(mant ^ 1, se);
            first = false;
        }
        if !f.same_f80(got) {
            return Verdict::Different(format!("st0: original {:?}, port {got:?}", f.f80()));
        }
    }
    // every output field that is memory
    for f in &c.outputs {
        if matches!(f.name.as_str(), "freed" | "ptr" | "eax" | "st0") || f.name.starts_with("heap") {
            continue;
        }
        let srf = c.name.starts_with("SI_nurbsSrfCopy");
        let at = if is_result(&f.name) { read_at(&l.m, &l.names, &answer, &f.name, srf) } else { l.names.addr(&l.m, &f.name) };
        let Some(at) = at else { return Verdict::Different(format!("{}: nothing of the port's to compare with", f.name)) };
        if first && mode.as_deref() == Some("1") && !f.words().is_empty() && at != 0 {
            let b = l.m.u8(at);
            l.m.set_u8(at, b ^ 1);
            first = false;
        }
        if let Err(e) = l.m.compare_words(at, &words(f), &l.names) {
            return Verdict::Different(format!("{}: {e}", f.name));
        }
    }
    // the inputs the original left alone
    for f in &c.inputs {
        if f.is_null() || c.try_output(&f.name).is_some() || (f.name == "shape" && f.words().len() == 1) {
            continue;
        }
        let Some(at) = l.names.addr(&l.m, &f.name) else { continue };
        if at == 0 {
            continue;
        }
        if let Err(e) = l.m.compare_words(at, &words(f), &l.names) {
            return Verdict::Different(format!("input {} changed: {e}", f.name));
        }
    }
    // the blocks given back, in order
    let want: Vec<u32> = match c.try_output("freed") {
        Some(f) => f
            .words()
            .iter()
            .map(|w| match w {
                Word::Ref { name, offset } => l.names.addr(&l.m, name).unwrap_or(u32::MAX).wrapping_add(*offset),
                Word::Value { value, .. } => *value as u32,
                Word::Unwritten { .. } => u32::MAX,
            })
            .collect(),
        None => Vec::new(),
    };
    let mut got = l.m.freed().to_vec();
    if mode.as_deref() == Some("calls") {
        if got.pop().is_none() {
            got.push(1);
        }
    }
    if want != got {
        return Verdict::Different(format!("freed: original {} blocks, port {} (first difference at {:?})", want.len(), got.len(), want.iter().zip(&got).position(|(a, b)| a != b)));
    }
    // the calls into the animation package, by name and argument words, in order
    let want: Vec<(String, Vec<u32>)> = c
        .calls
        .iter()
        .filter(|k| k.name.starts_with("SI_") || k.name == "markDirtyCcrvs" || k.name == "nurbsCrvAddKnotNonUniform" || k.name == "si_trmSrfHitInd")
        .map(|k| {
            let args = k
                .args
                .iter()
                .flat_map(|w| match w {
                    Word::Ref { name, offset } => vec![l.names.addr(&l.m, name).unwrap_or(u32::MAX).wrapping_add(*offset)],
                    // a double argument is two words
                    Word::Value { bytes: 8, value } => vec![*value as u32, (*value >> 32) as u32],
                    Word::Value { value, .. } => vec![*value as u32],
                    Word::Unwritten { .. } => vec![u32::MAX],
                })
                .collect();
            (k.name.clone(), args)
        })
        .collect();
    let mut got: Vec<(String, Vec<u32>)> = l.o.calls.iter().map(|k| (k.routine.name.to_string(), k.args.clone())).collect();
    if mode.as_deref() == Some("calls") && !want.is_empty() {
        got.pop();
    }
    if want != got {
        return Verdict::Different(format!("calls: original {want:x?}, port {got:x?}"));
    }
    Verdict::Same
}

fn check(c: &Case) -> Verdict {
    let name = c.name.split('#').next().unwrap_or("").to_string();
    let mut l = match load(c) {
        Ok(l) => l,
        Err(e) => return Verdict::Different(format!("the case does not load: {e}")),
    };
    match run(&name, c, &mut l) {
        Ok(a) => {
            if let Some(f) = l.m.fault() {
                return Verdict::Different(format!("a fault of the port's image: {f:?}"));
            }
            compare(c, &mut l, a)
        }
        Err(h) => Verdict::Different(format!("the port halted: {h:?}")),
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    match name.split('#').next()? {
        "SI_nurbs2Triangle" | "EvaluateNonTrimmedNurbsFromBaseGeometry" | "SI_SampleKnotVector" | "repeatKnotsInSampling" | "SI_nurbsSrfExtractCurve"
        | "SI_nurbsCrvValue4D" | "SI_nurbsCrvValue" | "getPointInterval" | "SI_nurbsCrvMovePoint" | "SI_nurbsCrvApplyMatrix"
        | "SI_nurbsCurveLength" | "SI_nurbsDistanceToFactor" | "SI_nurbsCurveUpdateLength" | "SI_nurbsCrvCopy" | "SI_nurbsSrfCopy"
        | "SI_nurbsSrfDispose" | "SI_nurbsSrfMovePoint" | "SI_nurbsSrfTransferPointsKnots" | "SI_nurbsCrvConvertToLinear"
        | "SI_knotsCrvReparametrization" | "SI_nurbsCrvInverse" | "SI_nurbsSrfApplyMatrix" | "SI_nurbsSrfForceCapping"
        | "SI_crvDeleteTagPoints" | "SI_nurbsCrvDeletePoint" | "SI_nurbsSrfDeleteRow" | "SI_nurbsCrvAddPoint"
        | "knotsWithoutMultiplicity" | "SI_srfHitDirAndVal" | "SI_nurbsSrfIndHit" => Some(check),
        _ => None,
    }
}
