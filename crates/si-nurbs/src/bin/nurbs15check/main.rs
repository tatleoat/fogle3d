// SPDX-License-Identifier: MIT
//! Compares the trim-line routines of `si_nurbs::trimline` (`trimLineOp.c`, finding 0025,
//! section 14) with the emulated original, on the vectors of `re/tools/si-nurbs/gen_trim.py`.
//!
//!   (ulimit -v 2000000; target/<slot>/release/nurbs-trimcheck re/out/si-nurbs/vec30)
//!
//! A case is loaded into a fresh `si_mem::Mem` (`si_mem::load_case`: every input buffer, a
//! buffer named `G<address>` at that address); the port runs on the image with the case's
//! arguments (a pointer argument is the buffer of that name). Compared afterwards: the
//! answer (`eax` or `st0`, all 80 bits), every output buffer word for word (a pointer into
//! a buffer as the port's address of that buffer), every input buffer the original left
//! alone, the blocks given back (`freed`) and the calls out, by name and argument words.
//!
//! `EMUCHECK_MUTATE=1`: one bit of the first compared value (the answer, else the first
//! output buffer) is flipped first; `=calls`: the last block given back, or call, is dropped
//! from the port's list (or one added). Every compared case must then differ.

mod ins2arms;
use si_mem::{Mem, Names, Ret, Trace, Word as W};
use std::{cell::RefCell, rc::Rc};
use si_nurbs::trimline as t;
use si_nurbs::fit;
use si_nurbs::fit2;
use si_nurbs::trimutils as u;
use si_x87::vectors::{check_dir_with, print_report, Case, Check, Verdict, Word};

fn word(w: &Word) -> W {
    match w {
        Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
        Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
        Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
    }
}

fn global_addr(name: &str) -> Option<u32> {
    let hex = name.strip_prefix('G')?;
    if hex.len() != 8 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn mode() -> Option<String> {
    std::env::var("EMUCHECK_MUTATE").ok().filter(|v| !v.is_empty() && v != "0")
}

/// An argument word the original left unwritten (the vector prints it as the all-ones word).
const WILD: u64 = u64::MAX;

type CallLog = Rc<RefCell<Vec<(String, Vec<u32>)>>>;

struct Loaded {
    m: Mem,
    names: Names,
    o: Trace,
    log: CallLog,
}

/// Calls whose pointer arguments point at the caller's stack locals: the generator's stand-in
/// records the words behind the pointer instead of the pointer (arg index, words), and the
/// port's responder does the same.
fn derefs(name: &str) -> &'static [(usize, u32)] {
    match name {
        "addCWBoundaryEdge" | "addCCWBoundaryEdge" => &[(3, 4), (4, 4)],
        _ => &[],
    }
}

fn is_result(name: &str) -> bool {
    matches!(name, "eax" | "ax" | "al" | "st0" | "ptr" | "freed" | "stdout" | "stderr")
}

/// A heap block of the original's run (`heap<K>`, the K-th block malloc handed out): the port's
/// K-th block is compared with it (the spec asks for the dump with `heap=u32`).
fn is_heap(name: &str) -> bool {
    name.strip_prefix("heap").is_some_and(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
}

fn load(c: &Case) -> Result<Loaded, String> {
    let mut m = Mem::new();
    let mut held: Vec<(String, Option<Vec<W>>)> = Vec::new();
    for f in &c.inputs {
        if global_addr(&f.name).is_some() {
            continue;
        }
        held.push((f.name.clone(), if f.is_null() { None } else { Some(f.words().iter().map(word).collect()) }));
    }
    for f in &c.outputs {
        if global_addr(&f.name).is_some() || is_result(&f.name) || is_heap(&f.name) || c.try_input(&f.name).is_some() {
            continue;
        }
        held.push((f.name.clone(), Some(f.words().iter().map(|w| W::Unwritten { bytes: w.bytes() as u8 }).collect())));
    }
    let fields: Vec<(&str, Option<&[W]>)> = held.iter().map(|(n, w)| (n.as_str(), w.as_deref())).collect();
    let mut names = si_mem::load_case(&mut m, &fields)?;
    for f in c.inputs.iter().chain(&c.outputs) {
        if let Some(a) = global_addr(&f.name) {
            if names.addr(&m, &f.name).is_none() {
                let w: Vec<W> = f.words().iter().map(word).collect();
                names.add(&f.name, a, si_mem::words_len(&w));
            }
        }
    }
    for f in &c.inputs {
        if let Some(a) = global_addr(&f.name) {
            let w: Vec<W> = f.words().iter().map(word).collect();
            m.load_words(a, &w, &names)?;
        }
    }
    // the routines outside the file answer 0 (the generator's stubs do)
    let log: CallLog = Rc::new(RefCell::new(Vec::new()));
    let lg = log.clone();
    let real = c.try_input("realins").is_some();
    // `ported`: the four small trim routines run as their ports (the original ran the real ones, so no call is logged)
    let ported = c.try_input("ported").is_some();
    // `NURBS_ROUTED=1`: the calls between the NURBS routines go to the ports (`si_nurbs::routed`); the vector's original ran
    // the real callees, so these are not logged; what no port answers is logged as before
    let routed = std::env::var_os("NURBS_ROUTED").is_some();
    let o = Trace::with(move |m, r, args| {
        if routed {
            let lg2 = lg.clone();
            let mut inner = Trace::with(move |_m, r2, a2| {
                lg2.borrow_mut().push((r2.name.to_string(), a2.to_vec()));
                Ret::NONE
            });
            let mut rt = si_nurbs::routed::Routed::new(&mut inner);
            if rt.enabled && !rt.skip.contains(&r.addr) {
                if std::env::var_os("NURBS_ROUTED_TRACE").is_some() {
                    eprintln!("routed {} {:08x} {:x?}", r.name, r.addr, args);
                    if r.addr == 0x084d_96bc {
                        let c = args[0];
                        for off in [0u32, 0x24] {
                            let (nk, kp) = (m.i32(c + off + 8), m.u32(c + off + 0xc));
                            let ks: Vec<f64> = (0..nk.clamp(0, 40) as u32).map(|k| m.f64(kp + 8 * k)).collect();
                            eprintln!("  dir@{off:x} order {} n {} nk {} range {} {} closed {:x} knots {:?}", m.i32(c + off), m.i32(c + off + 4), nk, m.f64(c + off + 0x10), m.f64(c + off + 0x18), m.u32(c + off + 0x20), ks);
                        }
                        eprintln!("  total {} param {} pts {:x}", m.i32(c + 0x48), m.i32(c + 0x4c), m.u32(c + 0x54));
                    }
                    if r.addr == 0x084d_4130 {
                        let c = args[0];
                        let (nk, kp) = (m.i32(c + 8), m.u32(c + 0xc));
                        let ks: Vec<f64> = (0..nk.clamp(0, 40) as u32).map(|k| m.f64(kp + 8 * k)).collect();
                        eprintln!("  order {} n {} nk {} range {} {} closed {:x} t {} knots {:?}", m.i32(c), m.i32(c + 4), nk, m.f64(c + 0x10), m.f64(c + 0x18), m.u32(c + 0x20), f64::from_bits(args[1] as u64 | (args[2] as u64) << 32), ks);
                    }
                }
                if let Some(ret) = rt.route(m, r.addr, args) {
                    return ret;
                }
            }
        }
        if ported {
            // the product's wiring (trimutils::TrimUtilsOutside) answers the four small trim routines
            if let Some(ret) = u::call_trim_util(m, r.addr, args) {
                return ret;
            }
        }
        let mut a = args.to_vec();
        for &(i, n) in derefs(r.name).iter().rev() {
            if let Some(&p) = args.get(i) {
                let words: Vec<u32> = (0..n).map(|k| m.u32(p.wrapping_add(4 * k))).collect();
                a.splice(i..=i, words);
            }
        }
        if r.name == "si_nurbsCurveInterBldLinFromComp" {
            a.truncate(2);
        }
        if r.name == "SI_nurbsMatrixBasisFunction" && a.len() > 3 {
            a[3] = 0; // the address of a stack local: not the original's
        }
        if r.name == "SI_nurbsCrvKnotInsert" && !a.is_empty() {
            a[0] = 0; // the working curve is a stack local
        }
        lg.borrow_mut().push((r.name.to_string(), a));
        if r.name == "SI_nurbsCrvKnotInsert" {
            return Ret::eax(1); // the stand-in changes nothing and says it inserted
        }
        if !real {
            return Ret::NONE;
        }
        match r.name {
            // the stand-ins of the update check: a curve of `count` points, and a node appended
            "si_nurbsCurveInterBldLinFromComp" => {
                let c = args[1] as i32;
                if args[0] == 0 {
                    return Ret::NONE;
                }
                let n = if c != 0 { c + 1 } else { 3 };
                let rec = m.si_malloc(0x64);
                for k in 0..0x19 {
                    m.set_u32(rec + 4 * k, 0);
                }
                let pts = m.si_malloc(0x24 * n as u32);
                for k in 0..(9 * n as u32) {
                    m.set_u32(pts + 4 * k, 0);
                }
                for i in 0..n {
                    let j = if c % 2 == 1 && i == n - 1 { 0 } else { i } as f64;
                    m.set_f64(pts + 0x24 * i as u32, 0.25 * j * ((c % 3 + 1) as f64));
                    m.set_f64(pts + 0x24 * i as u32 + 8, 0.1 * j + c as f64);
                }
                m.set_u32(rec + 4, n as u32);
                m.set_u32(rec + 0x2c, pts);
                Ret::eax(rec)
            }
            "si_trmDataIns" => {
                let line = args[0];
                let node = m.si_malloc(0x18);
                m.set_u32(node, args[1]);
                m.set_u32(node + 4, args[2]);
                m.set_u32(node + 8, args[3]);
                m.set_u32(node + 12, args[4]);
                m.set_u32(node + 0x10, 0);
                m.set_u32(node + 0x14, 0);
                let (cnt, head, cur) = (m.u32(line), m.u32(line + 8), m.u32(line + 0xc));
                let head = if head == 0 {
                    node
                } else {
                    m.set_u32(cur + 0x14, node);
                    head
                };
                m.set_u32(line, cnt.wrapping_add(1));
                m.set_u32(line + 8, head);
                m.set_u32(line + 0xc, node);
                Ret::NONE
            }
            _ => Ret::NONE,
        }
    });
    Ok(Loaded { m, names, o, log })
}

fn value(c: &Case, name: &str) -> u64 {
    c.input(name).words().first().and_then(|w| w.value()).unwrap_or(0) as u64
}

enum Answer {
    None,
    Eax(u32),
    St0(si_x87::F80),
}

fn run(name: &str, c: &Case, l: &mut Loaded) -> Answer {
    let p = |l: &Loaded, n: &str| -> u32 {
        match c.try_input(n) {
            Some(f) if f.is_null() => 0,
            // a pointer argument: its one word is the pointer
            Some(f) if f.words().len() == 1 && matches!(f.words()[0], Word::Ref { .. }) => match &f.words()[0] {
                Word::Ref { name, offset } => l.names.addr(&l.m, name).unwrap_or(0).wrapping_add(*offset),
                _ => 0,
            },
            // a pointer argument given as a plain value (0 for a null one)
            Some(f) if f.words().len() == 1 && matches!(f.words()[0], Word::Value { value: 0, .. }) => 0,
            _ => l.names.addr(&l.m, n).unwrap_or(0),
        }
    };
    let i = |n: &str| value(c, n) as u32 as i32;
    let f = |n: &str| f64::from_bits(value(c, n));
    match name {
        "linerInterp" => Answer::St0(t::liner_interp(f("a"), f("b"), f("c"), f("d"), f("e"))),
        "pointRelativeToPlane" => Answer::St0(si_nurbs::curveorient::point_relative_to_plane([f("a"), f("b"), f("c")], f("d"), [f("e"), f("f"), f("g")])),
        "si_nurbsRayBinarySplitRays" => Answer::St0(si_nurbs::inter::ray_binary_split_rays(f("a"), f("b"), f("c"), f("d"))),
        "clearTLnFlag" => {
            let line = p(l, "line");
            t::clear_tln_flag(&mut l.m, line);
            Answer::None
        }
        "bboxesIntsect" => Answer::Eax(t::bboxes_intsect(f("a"), f("b"), f("c"), f("d"), f("e"), f("f"), f("g"), f("h")) as u32),
        "atSameSide" => Answer::Eax(t::at_same_side(&l.m, f("a"), f("b"), f("c"), p(l, "p"), p(l, "q")) as u32),
        "nextNode" => Answer::Eax(t::next_node(&l.m, p(l, "first"), p(l, "node"))),
        "newPt" => Answer::Eax(t::new_pt(f("a"), f("b"), f("c"), f("d")) as u32),
        "atEdge" => Answer::Eax(t::at_edge(&l.m, f("u"), f("v")) as u32),
        "forceAtEdge" => Answer::Eax(t::force_at_edge(&l.m, f("u"), f("v")) as u32),
        "edgVal" => Answer::St0(t::edg_val(&l.m, f("a"), f("b"), i("edge"))),
        "brdIntsct" => Answer::Eax(t::brd_intsct(&l.m, f("a"), f("b"), f("c"), f("d")) as u32),
        "getIntPt" => {
            let line = p(l, "line");
            Answer::Eax(t::get_int_pt(&mut l.m, line, i("k")) as u32)
        }
        "nxtIntsect" => {
            let (a, b) = (p(l, "l1"), p(l, "l2"));
            Answer::Eax(t::nxt_intsect(&mut l.m, a, b) as u32)
        }
        "resTrmCrv" => {
            let line = p(l, "line");
            t::res_trm_crv(&mut l.m, line);
            Answer::None
        }
        "delTrimLine" => {
            let s = p(l, "sp");
            t::del_trim_line(&mut l.m, s);
            Answer::None
        }
        "reorderTrimStat" => {
            let st = p(l, "stp");
            t::reorder_trim_stat(&mut l.m, &mut l.o, st);
            Answer::None
        }
        "fillTheCoefs" => {
            let a = ["line", "dv", "du", "c", "o5", "o6", "o7", "o8"].map(|n| p(l, n));
            t::fill_the_coefs(&mut l.m, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
            Answer::None
        }
        "resetTLnFlag" => {
            let (a, b) = (p(l, "l1"), p(l, "l2"));
            t::reset_tln_flag(&mut l.m, &mut l.o, a, b);
            Answer::None
        }
        "closeLine" => {
            let (o, a, b) = (p(l, "o_"), p(l, "a_"), p(l, "b_"));
            t::close_line(&mut l.m, &mut l.o, o, a, b, i("i"), i("j"));
            Answer::None
        }
        "checkAtBorder" => {
            let (nd, ep) = (p(l, "ndp"), p(l, "ep"));
            let mut e = l.m.i32(ep);
            t::check_at_border(&mut l.m, nd, &mut e);
            l.m.set_i32(ep, e);
            Answer::None
        }
        "perturbSngPtOnBorder" => {
            let nd = p(l, "ndp");
            t::perturb_sng_pt_on_border(&mut l.m, nd, i("edge"));
            Answer::None
        }
        "moveTrimLine" => {
            let (st, n3, n4) = (p(l, "stp"), p(l, "n3"), p(l, "n4"));
            t::move_trim_line(&mut l.m, &mut l.o, st, i("idx"), n3, n4, i("cnt"), i("mode"), f("p7"), f("p8"));
            Answer::None
        }
        "joinTwoLnAtEnd" => {
            let st = p(l, "stp");
            t::join_two_ln_at_end(&mut l.m, &mut l.o, st);
            Answer::None
        }
        "midJoinTwoLines" => {
            let (st, a, b) = (p(l, "stp"), p(l, "na"), p(l, "nb"));
            t::mid_join_two_lines(&mut l.m, &mut l.o, st, a, b);
            Answer::None
        }
        "addUEgSmp" => {
            let ln = p(l, "ln");
            t::add_u_eg_smp(&mut l.m, &mut l.o, ln, f("p2"), f("p3"), f("p4"));
            Answer::None
        }
        "addVEgSmp" => {
            let ln = p(l, "ln");
            t::add_v_eg_smp(&mut l.m, &mut l.o, ln, f("p2"), f("p3"), f("p4"));
            Answer::None
        }
        "addCWBoundaryEdge" | "addCCWBoundaryEdge" => {
            let ln = p(l, "ln");
            let (e1, e2, a, b) = (i("e1"), i("e2"), p(l, "p"), p(l, "q"));
            if name == "addCWBoundaryEdge" {
                t::add_cw_boundary_edge(&mut l.m, &mut l.o, ln, e1, e2, a, b);
            } else {
                t::add_ccw_boundary_edge(&mut l.m, &mut l.o, ln, e1, e2, a, b);
            }
            Answer::None
        }
        "trimHalfPeak" => {
            let ln = p(l, "ln");
            t::trim_half_peak(&mut l.m, &mut l.o, ln);
            Answer::None
        }
        "handleUCsdMulIntsct" => {
            let st = p(l, "stp");
            t::handle_u_csd_mul_intsct(&mut l.m, &mut l.o, st, i("idx"));
            Answer::None
        }
        "handleVCsdMulIntsct" => {
            let st = p(l, "stp");
            t::handle_v_csd_mul_intsct(&mut l.m, &mut l.o, st, i("idx"));
            Answer::None
        }
        "trmCrvAEdg" => {
            let st = p(l, "stp");
            Answer::Eax(t::trm_crv_a_edg(&mut l.m, &mut l.o, st, i("idx")))
        }
        "trmCrvRevAEdg" => {
            let st = p(l, "stp");
            t::trm_crv_rev_a_edg(&mut l.m, &mut l.o, st, i("idx"));
            Answer::None
        }
        "trimPeak" => {
            let st = p(l, "stp");
            t::trim_peak(&mut l.m, &mut l.o, st);
            Answer::None
        }
        "addPtAtEdge" => {
            let (a, b, pp, q) = (p(l, "l1"), p(l, "l2"), p(l, "p"), p(l, "q"));
            t::add_pt_at_edge(&mut l.m, &mut l.o, a, b, pp, q);
            Answer::None
        }
        "travelingIntCrvs" => {
            let (st, a, b) = (p(l, "stp"), p(l, "l1"), p(l, "l2"));
            t::traveling_int_crvs(&mut l.m, &mut l.o, st, a, b);
            Answer::None
        }
        "si_lnSegsIntsect" => {
            let mut a = [0f64; 14];
            for (k, v) in a.iter_mut().enumerate() {
                *v = f(&format!("p{}", k + 1));
            }
            let (o1, o2) = (p(l, "o1"), p(l, "o2"));
            Answer::Eax(t::ln_segs_intsect(&mut l.m, a, o1, o2) as u32)
        }
        "tCrvIntscted" => {
            let (l1, l2) = (p(l, "l1"), p(l, "l2"));
            let mut a = [0u32; 14];
            for (k, v) in a.iter_mut().enumerate() {
                *v = p(l, &format!("q{}", k + 3));
            }
            let fp = p(l, "flagp");
            Answer::Eax(t::t_crv_intscted(&mut l.m, &mut l.o, l1, l2, a, fp, i("flag") as i16) as u32)
        }
        "si_delTrimLine" => {
            let lp = p(l, "lp");
            u::si_del_trim_line(&mut l.m, lp);
            Answer::None
        }
        "si_trmDataIns" => {
            let lp = p(l, "lp");
            u::si_trm_data_ins(&mut l.m, lp, f("u"), f("v"));
            Answer::None
        }
        "SI_trmCvrRve" => {
            let lp = p(l, "lp");
            u::si_trm_cvr_rve(&mut l.m, lp);
            Answer::None
        }
        "revTrimFg" => {
            let nd = p(l, "ndp");
            u::rev_trim_fg(&mut l.m, nd);
            Answer::None
        }
        "si_trmCrvBBox" => {
            let lp = p(l, "lp");
            u::si_trm_crv_bbox(&mut l.m, lp);
            Answer::None
        }
        "si_getEdgeSteps" => {
            let tp = p(l, "tp");
            Answer::St0(u::si_get_edge_steps(&l.m, tp, i("n"), i("k")))
        }
        "SI_NurbsPointVecSubtractXYZ" => {
            let (a, b, o) = (p(l, "ap"), p(l, "bp"), p(l, "op"));
            fit::point_vec_subtract_xyz(&mut l.m, i("n"), a, b, o);
            Answer::None
        }
        "SI_NurbsPointVecInnerProduct" => {
            let (a, b) = (p(l, "ap"), p(l, "bp"));
            let (x, y, z) = (p(l, "rxp"), p(l, "ryp"), p(l, "rzp"));
            fit::point_vec_inner_product(&mut l.m, i("n"), a, b, x, y, z);
            Answer::None
        }
        "SI_NurbsPointVecGaxpy" => {
            let (a, b, o) = (p(l, "ap"), p(l, "bp"), p(l, "op"));
            fit::point_vec_gaxpy(&mut l.m, i("n"), [f("s0"), f("s1"), f("s2")], a, b, o);
            Answer::None
        }
        "classifySample" => {
            let pt = |i: usize| [f(&format!("p{i}0")), f(&format!("p{i}1")), f(&format!("p{i}2"))];
            Answer::Eax(fit::classify_sample(pt(0), pt(1), pt(2), f("tol")) as u32)
        }
        "freeSegmentList" => {
            let h = p(l, "h");
            fit::free_segment_list(&mut l.m, h);
            Answer::None
        }
        "cleanupPartition" => {
            let (arr, a, b, c2, d) = (p(l, "arrp"), p(l, "a"), p(l, "b"), p(l, "c"), p(l, "d"));
            fit::cleanup_partition(&mut l.m, arr, i("n"), a, b, c2, d);
            Answer::None
        }
        "freeEvalTemps" => {
            let (tab, ex) = (p(l, "tabp"), p(l, "extra"));
            fit::free_eval_temps(&mut l.m, i("n"), tab, ex);
            Answer::None
        }
        "nurbsPowerBasis" => {
            let (d, t) = (p(l, "dirp"), p(l, "tabp"));
            Answer::Eax(fit::nurbs_power_basis(&mut l.m, d, i("i"), t) as u32)
        }
        "adjustOpenData" => {
            let (o, s, a, b) = (p(l, "outp"), p(l, "srcp"), p(l, "ap"), p(l, "bp"));
            fit::adjust_open_data(&mut l.m, i("n"), o, s, a, b);
            Answer::None
        }
        "computeSegmentNormalEquations" => {
            let (sg, mt) = (p(l, "segp"), p(l, "matp"));
            Answer::Eax(fit::compute_segment_normal_equations(&mut l.m, sg, i("k"), i("flag"), mt) as u32)
        }
        "plusEqualSegmentNormalEqns" => {
            let (sg, mt) = (p(l, "segp"), p(l, "matp"));
            fit::plus_equal_segment_normal_eqns(&mut l.m, i("row"), i("col"), i("k"), i("flag"), sg, mt);
            Answer::None
        }
        "computeNormalEquations" => {
            let (sg, mt) = (p(l, "sp"), p(l, "matp"));
            fit::compute_normal_equations(&mut l.m, sg, i("flag"), i("k"), i("n"), mt);
            Answer::None
        }
        "multVecByNrmlEqns" => {
            let (mt, pt, o) = (p(l, "matp"), p(l, "ptsp"), p(l, "outp"));
            fit::mult_vec_by_nrml_eqns(&mut l.m, mt, i("k"), pt, o);
            Answer::None
        }
        "SI_nurbsMatrixBasisFunction" => {
            let (d, ix) = (p(l, "dirp"), p(l, "idxp"));
            Answer::Eax(fit2::nurbs_matrix_basis_function(&mut l.m, d, f("t"), ix, i("flag") as i16))
        }
        "SI_nurbsCrvBasisFunctions" => {
            let mx = p(l, "matp");
            Answer::Eax(fit2::nurbs_crv_basis_functions(&mut l.m, mx, i("flag") as i16))
        }
        "allocateFixedData" => {
            let q = |l: &Loaded, n: &str| p(l, n);
            let (a, b, c2, d, e, f, g2) = (q(l, "ptsp"), q(l, "map"), q(l, "mbp"), q(l, "pts2p"), q(l, "p5p"), q(l, "p6p"), q(l, "p7p"));
            Answer::Eax(fit2::allocate_fixed_data(&mut l.m, i("n"), i("k"), a, b, c2, d, e, f, g2) as u32)
        }
        "updateKnotVector" => {
            let (d, pp, dn, fl, pr, tp) = (p(l, "dirp"), p(l, "ptspp"), p(l, "donep"), p(l, "flagsp"), p(l, "parp"), p(l, "tplp"));
            Answer::Eax(fit2::update_knot_vector(&mut l.m, &mut l.o, d, pp, i("k"), dn, fl, i("m"), pr, tp) as u32)
        }
        "multVecByEvalMatTrans" => {
            let (sg, mt, pt, ou) = (p(l, "segp"), p(l, "matp"), p(l, "ptsp"), p(l, "outp"));
            Answer::Eax(fit2::mult_vec_by_eval_mat_trans(&mut l.m, sg, mt, i("closed"), pt, i("n"), ou) as u32)
        }
        "nurbsCrvFromComponents" => {
            let (d, pl) = (p(l, "dirsp"), p(l, "plp"));
            Answer::Eax(fit2::nurbs_crv_from_components(&mut l.m, d, i("n"), pl, i("closed")))
        }
        "calculateError" => {
            let (d, a, b, cn, pr, nf, rs) = (p(l, "dirp"), p(l, "pap"), p(l, "pbp"), p(l, "cntp"), p(l, "parp"), p(l, "nflp"), p(l, "resp"));
            Answer::Eax(fit2::calculate_error(&mut l.m, d, a, b, i("n"), cn, pr, nf, rs, f("tol")) as u32)
        }
        "evaluateSegment" => {
            let (d, sg, mt, pr, tb, ou) = (p(l, "dirp"), p(l, "segp"), p(l, "matp"), p(l, "parp"), p(l, "tabp"), p(l, "outp"));
            Answer::Eax(fit::evaluate_segment(&mut l.m, &mut l.o, d, sg, i("i"), mt, pr, tb, ou) as u32)
        }
        "computeNurbsEvaluationMatrix" => {
            let (d, mt, lo, pr) = (p(l, "dirp"), p(l, "matp"), p(l, "lop"), p(l, "parp"));
            Answer::Eax(fit::compute_nurbs_evaluation_matrix(&mut l.m, &mut l.o, d, mt, lo, i("n"), pr) as u32)
        }
        "multVecByEvalMat" => {
            let (sg, mt, pt, ou) = (p(l, "segp"), p(l, "matp"), p(l, "ptsp"), p(l, "outp"));
            fit::mult_vec_by_eval_mat(&mut l.m, sg, mt, i("closed"), i("n"), pt, ou);
            Answer::None
        }
        "si_uniqueD" => {
            let s = p(l, "srfp");
            Answer::Eax(u::si_unique_d(&l.m, s, f("u1"), f("v1"), f("u2"), f("v2")) as i32 as u32)
        }
        "si_detectEdgeCross" => {
            let st = p(l, "stp");
            Answer::Eax(t::si_detect_edge_cross(&mut l.m, &mut l.o, st) as u32)
        }
        "mergerIntesectedTCrv" => {
            let st = p(l, "stp");
            Answer::Eax(t::merger_intersected_t_crv(&mut l.m, &mut l.o, st) as u32)
        }
        "si_convertTrimLineToTrimCrv" => {
            let cp = p(l, "comp");
            t::convert_trim_line_to_trim_crv(&mut l.m, cp);
            Answer::None
        }
        "SI_trimCrvToLineUpdate" => {
            let cp = p(l, "comp");
            t::trim_crv_to_line_update(&mut l.m, &mut l.o, cp);
            Answer::None
        }
        "si_handleAutoBorder" => {
            let (srf, lp) = (p(l, "srf"), p(l, "linep"));
            Answer::Eax(t::handle_auto_border(&mut l.m, &mut l.o, srf, i("flag") as i16, lp) as u32)
        }
        "edgeDir" => {
            let (a, b, d) = (p(l, "p1"), p(l, "p2"), p(l, "p3"));
            Answer::Eax(t::edge_dir(&l.m, a, b, d, i("e1"), i("e2"), i("e3")) as u32)
        }
        n if ins2arms::NAMES.contains(&n) => ins2arms::run(n, l, &p, &i, &f),
        _ => unreachable!("{name}"),
    }
}

fn compare(c: &Case, l: &mut Loaded, answer: Answer) -> Verdict {
    let mode = mode();
    let blocks: Vec<(u32, u32)> = l.m.heap_blocks().to_vec();
    let want_blocks = c.outputs.iter().filter(|f| is_heap(&f.name)).count();
    // the vector lists every block the routine allocated (a routine that gives back what it allocated itself, as
    // nurbsPowerBasis does, lists none: the block is then only named in `freed`)
    let live = blocks.iter().filter(|b| !l.m.is_freed(b.0)).count();
    if want_blocks != blocks.len() && !(want_blocks == live && c.try_output("freed").is_some()) {
        return Verdict::Different(format!("heap blocks: original {want_blocks}, port {} (live {live})", blocks.len()));
    }
    if std::env::var_os("NURBS_BLOCKS").is_some() {
        for (k, &(a, n)) in blocks.iter().enumerate() {
            let w: Vec<String> = (0..(n / 4).min(6)).map(|j| format!("{:08x}", l.m.u32(a + 4 * j))).collect();
            let wantw = c.try_output(&format!("heap{k}")).map(|f| f.words().iter().take(6).map(|x| format!("{:x?}", x.value())).collect::<Vec<_>>().join(","));
            eprintln!("BLOCK heap{k} @{a:x} size {n} freed {} port {} orig {:?} (orig {} words)", l.m.is_freed(a), w.join(","), wantw, c.try_output(&format!("heap{k}")).map(|f| f.words().len()).unwrap_or(0));
        }
    }
    for (k, &(a, n)) in blocks.iter().enumerate() {
        l.names.add(&format!("heap{k}"), a, n);
    }
    let mut first = true;
    if let Some(f) = c.try_output("eax").or_else(|| c.try_output("ptr")) {
        let Answer::Eax(mut got) = answer else { return Verdict::Different("no eax answer from the port".into()) };
        if mode.as_deref() == Some("1") {
            got ^= 1;
            first = false;
        }
        // a pointer answer is a buffer address
        let want = match f.words().first() {
            Some(Word::Ref { name, offset }) => l.names.addr(&l.m, name).map(|a| a.wrapping_add(*offset)),
            Some(w) => w.value().map(|v| v as u32),
            None => None,
        };
        // narrow answers (`ax`, `al`) are compared on their own width by the caller
        if want != Some(got) {
            return Verdict::Different(format!("eax: original {want:x?}, port {got:#x}"));
        }
    }
    if let Some(f) = c.try_output("ax") {
        let Answer::Eax(mut got) = answer else { return Verdict::Different("no answer from the port".into()) };
        if mode.as_deref() == Some("1") {
            got ^= 1;
            first = false;
        }
        let want = f.words().first().and_then(|w| w.value()).map(|v| v as u16);
        if want != Some(got as u16) {
            return Verdict::Different(format!("ax: original {want:x?}, port {:#x}", got as u16));
        }
    }
    if let Some(f) = c.try_output("st0") {
        let Answer::St0(mut got) = answer else { return Verdict::Different("no st0 answer from the port".into()) };
        if mode.as_deref() == Some("1") {
            let (mant, se) = got.to_bits();
            got = si_x87::F80::from_bits(mant ^ 1, se);
            first = false;
        }
        if !f.same_f80(got) {
            return Verdict::Different(format!("st0: original {:?}, port {got:?}", f.f80()));
        }
    }
    for f in &c.outputs {
        if is_result(&f.name) {
            continue;
        }
        let Some(at) = l.names.addr(&l.m, &f.name) else { return Verdict::Different(format!("{}: no buffer of that name", f.name)) };
        if first && mode.as_deref() == Some("1") && !f.words().is_empty() && at != 0 {
            let b = l.m.u8(at);
            l.m.set_u8(at, b ^ 1);
            first = false;
        }
        let w: Vec<W> = f.words().iter().map(word).collect();
        if let Err(e) = l.m.compare_words(at, &w, &l.names) {
            return Verdict::Different(format!("{}: {e}", f.name));
        }
    }
    for f in &c.inputs {
        if f.is_null() || c.try_output(&f.name).is_some() || global_addr(&f.name).is_some() && c.try_output(&f.name).is_some() {
            continue;
        }
        let Some(at) = l.names.addr(&l.m, &f.name) else { continue };
        if at == 0 {
            continue;
        }
        let w: Vec<W> = f.words().iter().map(word).collect();
        if let Err(e) = l.m.compare_words(at, &w, &l.names) {
            return Verdict::Different(format!("input {} changed: {e}", f.name));
        }
    }
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
    if mode.as_deref() == Some("calls") && got.pop().is_none() {
        got.push(1);
    }
    if want != got {
        return Verdict::Different(format!("freed: original {} blocks, port {} ({want:?} vs {got:?})", want.len(), got.len()));
    }
    let want: Vec<(String, Vec<u64>)> = c
        .calls
        .iter()
        .filter(|k| !matches!(k.name.as_str(), "cos" | "sin" | "acos")) // libm: the port computes it, the result is compared
        .map(|k| {
            let args = k
                .args
                .iter()
                .flat_map(|w| match w {
                    Word::Ref { name, offset } => vec![l.names.addr(&l.m, name).unwrap_or(u32::MAX).wrapping_add(*offset) as u64],
                    Word::Value { bytes: 8, value } => vec![*value as u32 as u64, (*value >> 32) as u32 as u64],
                    Word::Value { value, .. } => vec![*value as u32 as u64],
                    Word::Unwritten { bytes } => vec![WILD; (*bytes as usize / 4).max(1)],
                })
                .collect();
            let mut args: Vec<u64> = args;
            if k.name == "SI_nurbsMatrixBasisFunction" && args.len() > 3 {
                args[3] = 0; // the address of a stack local: not the original's
            }
            if k.name == "SI_nurbsCrvKnotInsert" && !args.is_empty() {
                args[0] = 0;
            }
            (k.name.clone(), args)
        })
        .collect();
    let mut got: Vec<(String, Vec<u32>)> = l.log.borrow().clone();
    if mode.as_deref() == Some("calls") && !want.is_empty() {
        got.pop();
    }
    // a word the original never wrote (it passes uninitialised stack as a data word) matches any word of the port
    let same_calls = want.len() == got.len()
        && want.iter().zip(&got).all(|(w, g)| {
            w.0 == g.0 && w.1.len() == g.1.len() && w.1.iter().zip(&g.1).all(|(a, b)| *a == WILD || *a == *b as u64)
        });
    if !same_calls {
        return Verdict::Different(format!("calls: original {want:x?}, port {got:x?}"));
    }
    Verdict::Same
}

fn check(c: &Case) -> Verdict {
    if std::env::var("NURBS_TRACE").is_ok() { eprintln!("CHECK {}", c.name); }
    let name = c.name.split('#').next().unwrap_or("").to_string();
    let mut l = match load(c) {
        Ok(l) => l,
        Err(e) => return Verdict::Different(format!("the case does not load: {e}")),
    };
    let _ = t::take_smp_outside_reads();
    let _ = si_nurbs::ins2::take_outside_reads();
    let _ = fit2::take_undefined_reads();
    let a = run(&name, c, &mut l);
    // the port read an edge-sample table that the routine allocated itself outside its block: the original reads a
    // chunk header or a neighbour there (undefined memory), so a difference says nothing; not compared
    let outside = t::take_smp_outside_reads() > 0 && mode().is_none(); // (under a mutation switch every case must differ)
    if let Some(f) = l.m.fault() {
        if std::env::var("NURBS_WHYSKIP").is_ok_and(|w| w == name) {
            eprintln!("WHYSKIP {name} fault (outside={outside}): {f:?}");
        }
        if outside {
            return Verdict::Skip;
        }
        return Verdict::Different(format!("a fault of the port's image: {f:?}"));
    }
    let undef = (fit2::take_undefined_reads() > 0 || si_nurbs::ins2::take_outside_reads() > 0) && mode().is_none();
    let v = compare(c, &mut l, a);
    if let (Verdict::Different(d), Ok(w)) = (&v, std::env::var("NURBS_WHYSKIP")) {
        if name == w {
            eprintln!("WHYSKIP {name} (undef={undef} outside={outside}): {d}");
        }
    }
    if undef && matches!(v, Verdict::Different(_)) {
        return Verdict::Skip;
    }
    if outside && matches!(v, Verdict::Different(_)) {
        return Verdict::Skip;
    }
    if let (Verdict::Different(d), Some(w)) = (&v, std::env::var("NURBS_WHY").ok()) {
        if name == w {
            eprintln!("WHY {name} [{}]: {d} :: {}", c.name, format!("{:?}", c.inputs).chars().take(0).collect::<String>() + &format!("{:?}", c.inputs.iter().filter(|f| f.name == "cu" || f.name == "r" || f.name == "co" || f.name == "t" || f.name == "s").collect::<Vec<_>>()).chars().take(400).collect::<String>());
        }
    }
    v
}

fn lookup(name: &str) -> Option<Check> {
    let head = name.split('#').next()?;
    if ins2arms::NAMES.contains(&head) {
        return Some(check);
    }
    match head {
        "pointRelativeToPlane" | "si_nurbsRayBinarySplitRays" | "linerInterp" | "clearTLnFlag" | "bboxesIntsect" | "atSameSide" | "nextNode" | "newPt" | "atEdge" | "forceAtEdge" | "edgVal" | "brdIntsct"
        | "getIntPt" | "nxtIntsect" | "resTrmCrv" | "delTrimLine" | "reorderTrimStat" | "fillTheCoefs"
        | "resetTLnFlag" | "closeLine" | "checkAtBorder" | "perturbSngPtOnBorder" | "edgeDir" | "travelingIntCrvs" | "moveTrimLine" | "joinTwoLnAtEnd" | "midJoinTwoLines" | "trimPeak" | "addPtAtEdge" | "addUEgSmp" | "addVEgSmp" | "trimHalfPeak" | "trmCrvRevAEdg" | "trmCrvAEdg" | "handleUCsdMulIntsct" | "handleVCsdMulIntsct" | "addCWBoundaryEdge" | "addCCWBoundaryEdge" | "si_lnSegsIntsect" | "tCrvIntscted" | "si_convertTrimLineToTrimCrv" | "SI_trimCrvToLineUpdate" | "si_handleAutoBorder" | "mergerIntesectedTCrv" | "si_delTrimLine" | "si_trmDataIns" | "SI_trmCvrRve" | "revTrimFg" | "si_trmCrvBBox" | "si_getEdgeSteps" | "si_uniqueD" | "si_detectEdgeCross" | "SI_NurbsPointVecSubtractXYZ" | "SI_NurbsPointVecInnerProduct" | "SI_NurbsPointVecGaxpy" | "classifySample" | "freeSegmentList" | "cleanupPartition" | "freeEvalTemps" | "nurbsPowerBasis" | "adjustOpenData" | "computeSegmentNormalEquations" | "plusEqualSegmentNormalEqns" | "computeNormalEquations" | "multVecByNrmlEqns" | "evaluateSegment" | "multVecByEvalMat" | "computeNurbsEvaluationMatrix" | "SI_nurbsMatrixBasisFunction" | "SI_nurbsCrvBasisFunctions" | "allocateFixedData" | "updateKnotVector" | "calculateError" | "nurbsCrvFromComponents" | "multVecByEvalMatTrans" => Some(check),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir] = args.as_slice() else {
        eprintln!("usage: nurbs-trimcheck <directory of vector files>");
        std::process::exit(2);
    };
    let dir = dir.clone();
    let child = std::thread::Builder::new().stack_size(256 << 20).spawn(move || {
        let rows = check_dir_with(std::path::Path::new(&dir), &lookup, std::env::var("NURBS_PARTLY").is_ok()).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        });
        print_report(&rows)
    });
    let ok = child.unwrap().join().unwrap_or(false);
    std::process::exit(if ok { 0 } else { 1 });
}
