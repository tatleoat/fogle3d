// SPDX-License-Identifier: MIT
//! Checks of the record routines of `nrb_b.c`: allocation, copies, disposal, the exchange
//! of U and V. Composites and surfaces with lists are read from the buffers that
//! `re/tools/si-nurbs/emu_gen.py` names `comp<tag>`, `ca<tag>`, `crv<tag>_<i>`, `tl<tag>`,
//! `tn<tag>_<i>`, `ta`, `pa`.

use std::cell::RefCell;

use si_nurbs::own::{self, CompCrv, CompCurve, Curve, Freed, Host, Surface, TrimLine};
use si_types::Ptr32;
use si_x87::vectors::{Case, Verdict, Word};

use crate::util::*;

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn log(text: String) {
    LOG.with(|l| l.borrow_mut().push(text));
}

/// A host that writes down every call, as the harness records the original's.
pub struct Rec {
    pub index: Option<Vec<i32>>,
}

impl Rec {
    pub fn new(c: &Case) -> Rec {
        LOG.with(|l| l.borrow_mut().clear());
        let index = c.try_input("idx").map(|f| vec![0x5a5a_5a5au32 as i32; f.len()]);
        Rec { index }
    }
}

impl Host for Rec {
    fn srf_dispose_lines(&mut self, s: &mut Surface) {
        // the tree pointer the drawing routine would see: hidden by the two list disposals
        log(format!("SI_nurbsSrfDisposeLines tree={}", if s.rec.unk_c8[4] == 0 { 0 } else { 1 }));
    }
    fn gen_tree_destroy(&mut self, _s: &mut Surface) {
        log("SI_genTreeDestroy".to_string());
    }
    fn trm_cvr_rve(&mut self, trim: usize, _line: &mut TrimLine) {
        log(format!("SI_trmCvrRve(t{trim})"));
    }
    fn gbl_index(&mut self) -> Option<&mut Vec<i32>> {
        self.index.as_mut()
    }
    fn invert_path_keys(&mut self) {
        log("SI_findNURBSPathFcurveOwner".to_string());
        log("SI_invertFcurvePathKeys".to_string());
    }
}

/// The recorded calls of the case against the calls the port made through [`Rec`]: names
/// in order, and for the trim reversal which trim's line it was given.
pub fn same_calls(c: &Case) -> Verdict {
    let want: Vec<String> = c
        .calls
        .iter()
        .filter(|k| k.name.starts_with("SI_"))
        .map(|k| match (k.name.as_str(), k.args.first()) {
            ("SI_trmCvrRve", Some(Word::Ref { name, .. })) => format!("SI_trmCvrRve({})", name.trim_start_matches("tl")),
            (n, _) => n.to_string(),
        })
        .collect();
    let got: Vec<String> = LOG.with(|l| l.borrow().iter().map(|s| s.split(" tree=").next().unwrap().to_string()).collect());
    let got = if mutate() && !got.is_empty() { got[1..].to_vec() } else { got };
    Verdict::from(want == got, || format!("calls: original {want:?}, port {got:?}"))
}

/// True when every display-line disposal the port asked for saw the tree pointer hidden.
pub fn lines_saw_no_tree() -> bool {
    LOG.with(|l| l.borrow().iter().filter(|s| s.starts_with("SI_nurbsSrfDisposeLines")).all(|s| s.ends_with("tree=0")))
}

/// The index table after the call (`idx`) against the host's.
pub fn same_index(c: &Case, host: &Rec) -> Verdict {
    match (c.try_output("idx"), host.index.as_ref()) {
        (Some(f), Some(t)) => {
            let w = nums(f);
            for (i, (o, g)) in w.iter().zip(t).enumerate() {
                let written = *g != 0x5a5a_5a5au32 as i32;
                match o {
                    None if written => return Verdict::Different(format!("idx[{i}]: the port wrote, the original did not")),
                    Some(o) if !written || *o as u32 != *g as u32 => return Verdict::Different(format!("idx[{i}]: original {o:x}, port {g:x}")),
                    _ => {}
                }
            }
            Verdict::Same
        }
        _ => Verdict::Same,
    }
}

pub fn comp_rec_from(w: &[Option<u64>]) -> CompCrv {
    let v = |i: usize| w[i].unwrap_or(0);
    let a = |i: usize| if v(i) == PTR { 1 } else { v(i) as u32 };
    CompCrv {
        boundary: v(0) as i16,
        pad_02: (v(1) as u16).to_le_bytes(),
        nb_curves: v(2) as i32,
        curves: Ptr32::new(a(3)),
        trim_line: Ptr32::new(a(4)),
        unk_10: v(5) as u32,
        block_14: Ptr32::new(a(6)),
    }
}

/// The words of a composite as the dumps print them: the boundary flag as its two bytes
/// (the allocator's mark is in the first only).
pub fn comp_rec_words(r: &CompCrv) -> Vec<u64> {
    let b = r.boundary.to_le_bytes();
    vec![
        b[0] as u64, b[1] as u64, u16::from_le_bytes(r.pad_02) as u64, r.nb_curves as u32 as u64, r.curves.addr() as u64,
        r.trim_line.addr() as u64, r.unk_10 as u64, r.block_14.addr() as u64,
    ]
}

/// The composite of a case with the tag `t`.
pub fn comp(c: &Case, t: &str) -> CompCurve {
    let rec = comp_rec_from(&nums(c.input(&format!("comp{t}"))));
    let curves = c.try_input(&format!("ca{t}")).map(|f| (0..f.len()).map(|i| Some(Box::new(curve(c, &format!("{t}_{i}"))))).collect());
    let trim_line = c.try_input(&format!("tl{t}")).map(|f| {
        let w = nums(f);
        let mut nodes = Vec::new();
        while let Some(n) = c.try_input(&format!("tn{t}_{}", nodes.len())) {
            let nw = nums(n);
            nodes.push([f64::from_bits(nw[0].unwrap_or(0)), f64::from_bits(nw[1].unwrap_or(0))]);
        }
        Box::new(TrimLine { box4: core::array::from_fn(|i| f64::from_bits(w[4 + i].unwrap_or(0))), nodes })
    });
    CompCurve { rec, curves, trim_line }
}

/// The surface of a case with its trims (`ta`, composites `t0`, `t1`, …) and projected
/// curves (`pa`, `p0`, …); the record is the buffer `name`.
pub fn surface_full(c: &Case, name: &str) -> Surface {
    let mut s = surface_named(c, name, "");
    s.trims = c.try_input("ta").map(|f| (0..f.len()).map(|i| Some(Box::new(comp(c, &format!("t{i}"))))).collect());
    s.projected = c.try_input("pa").map(|f| (0..f.len()).map(|i| Some(Box::new(comp(c, &format!("p{i}"))))).collect());
    s
}

/// A composite the original made (`<o>.rec`, `<o>.c<i>.…`) against the port's.
pub fn same_comp(c: &Case, o: &str, got: &CompCurve) -> Verdict {
    let mut v = same_out(c, &format!("{o}.rec"), &comp_rec_words(&got.rec));
    if let Some(curves) = got.curves.as_ref() {
        for (i, cv) in curves.iter().enumerate() {
            let Some(cv) = cv else { continue };
            v = v.and(|| same_curve(c, &format!("{o}.c{i}"), cv));
        }
    }
    v.and(|| match (c.try_output(&format!("{o}.tl")), got.trim_line.as_ref()) {
        (Some(f), Some(l)) => {
            let mut v = same(&format!("{o}.tl"), f, &f64w(&l.box4));
            for (k, n) in l.nodes.iter().enumerate() {
                v = v.and(|| same_out(c, &format!("{o}.tn{k}"), &f64w(n)));
            }
            v
        }
        // a dump without the line (the copies do not touch it)
        (None, _) => Verdict::Same,
        _ => Verdict::Different(format!("{o}: a trim line in the original only")),
    })
}

/// A surface with its two lists against the dump `<o>.…`.
pub fn same_surface_full(c: &Case, o: &str, got: &Surface) -> Verdict {
    let mut v = same_surface(c, o, got);
    for (tag, list, n) in [("t", &got.trims, got.rec.nb_trims), ("p", &got.projected, got.rec.nb_projected)] {
        let Some(list) = list else { continue };
        for (i, cm) in list.iter().take(n.max(0) as usize).enumerate() {
            let Some(cm) = cm else { continue };
            v = v.and(|| same_comp(c, &format!("{o}.{tag}{i}"), cm));
        }
    }
    v
}

/// Turns the port's names of freed blocks into the case's buffer names.
fn buffer_name(n: &str) -> String {
    // a curve's blocks under a tag
    let crv = |tag: &str, what: &str| if what == "crv" { format!("crv{tag}") } else { format!("{what}{tag}") };
    let comp = |tag: &str, rest: &str| -> String {
        if let Some((c, what)) = rest.split_once('.') {
            return crv(&format!("{tag}_{}", &c[1..]), what);
        }
        match rest {
            "curves" => format!("ca{tag}"),
            "tl" => format!("tl{tag}"),
            "b14" => format!("b14{tag}"),
            "comp" => format!("comp{tag}"),
            r if r.starts_with("tn") => format!("tn{tag}_{}", &r[2..]),
            r => r.to_string(),
        }
    };
    if let Some((head, rest)) = n.split_once('.') {
        if head.starts_with('t') || head.starts_with('p') {
            return comp(head, rest);
        }
        return comp("", n);
    }
    match n {
        "trims" => "ta".to_string(),
        "projected" => "pa".to_string(),
        "curves" | "tl" | "b14" | "comp" => comp("", n),
        r if r.starts_with("tn") => comp("", r),
        r => r.to_string(),
    }
}

/// The blocks the original freed, in order (`freed=&a,&b,…`), against the port's.
pub fn same_freed(c: &Case, got: &Freed) -> Verdict {
    let want: Vec<String> = match c.try_output("freed") {
        Some(f) => f.words().iter().map(|w| match w {
            Word::Ref { name, .. } => name.clone(),
            _ => "?".to_string(),
        }).collect(),
        None => Vec::new(),
    };
    let mut got: Vec<String> = got.iter().map(|n| buffer_name(n)).collect();
    if mutate() && !got.is_empty() {
        got.remove(0);
    }
    Verdict::from(want == got, || format!("freed: original {want:?}, port {got:?}"))
}

pub fn boxed(c: Curve) -> Option<Box<Curve>> {
    Some(Box::new(c))
}

pub fn lookup(name: &str) -> Option<si_x87::vectors::Check> {
    Some(match name {
        "SI_nurbsPointsAllocate" => |c| {
            let got = own::points_allocate(c.input("n").i32());
            let null = nums(c.output("ptr"))[0] == Some(0);
            match (null, got) {
                (false, Some(p)) => {
                    // the block as words: each point is two words a double, then the flag
                    let w: Vec<u64> = p.iter().flat_map(|q| pt_words(q).into_iter().enumerate().flat_map(|(i, v)| if i < 4 { vec![v & 0xffff_ffff, v >> 32] } else { vec![v] })).collect();
                    same_out(c, "heap0", &w)
                }
                (true, None) => Verdict::from(!mutate(), String::new),
                (a, b) => Verdict::Different(format!("block: original null {a}, port {}", b.is_none())),
            }
        },
        "si_knotsAllocate" => |c| {
            let got = own::knots_allocate(c.input("n").i32());
            let null = nums(c.output("ptr"))[0] == Some(0);
            match (null, got) {
                (false, Some(k)) => same_out(c, "heap0", &vec![0u64; 2 * k.len()]),
                (true, None) => Verdict::from(!mutate(), String::new),
                (a, b) => Verdict::Different(format!("block: original null {a}, port {}", b.is_none())),
            }
        },
        "SI_nurbsCrvAllocate" => |c| same_out(c, "n.rec", &crv_rec_words(&Curve::allocate().rec)),
        "SI_nurbsSrfAllocate" => |c| same_out(c, "n.rec", &srf_rec_words(&Surface::allocate().rec)),
        "SI_nurbsCrvCopy" => |c| match own::crv_copy(&curve(c, "")) {
            Some(n) => same_curve(c, "n", &n),
            None => refused(c, "curve copy"),
        },
        "SI_nurbsCrvInit" => |c| {
            let mut d = curve(c, "");
            match own::crv_init(&mut d, &curve(c, "2")) {
                Some(()) => same_curve(c, "o", &d),
                None => refused(c, "curve init"),
            }
        },
        "SI_nurbsCrvDispose" => |c| {
            let mut slot = if c.input("pp").is_null() || nums(c.input("pp"))[0] == Some(0) { None } else { boxed(curve(c, "")) };
            let f = own::crv_dispose(&mut slot);
            same_freed(c, &f).and(|| same_out(c, "pp", &[0]))
        },
        "SI_nurbsCrvInverse" => |c| {
            let mut cv = curve(c, "");
            let mut host = Rec::new(c);
            let (mut k, mut p) = (cv.knots.take().unwrap_or_default(), cv.points.take().unwrap_or_default());
            let r = si_nurbs::edit::crv_inverse(&mut host, &mut cv.rec, &mut k, &mut p);
            cv.knots = Some(k);
            cv.points = Some(p);
            match r {
                Some(r) => same_eax(c, r as u32).and(|| same_curve(c, "o", &cv)).and(|| same_calls(c)).and(|| same_index(c, &host)),
                None => refused(c, "curve reversal"),
            }
        },
        "SI_nurbsCompCopy" => |c| match own::comp_copy(&comp(c, "")) {
            Some(n) => same_comp(c, "n", &n),
            None => refused(c, "composite copy"),
        },
        "SI_nurbsCompFromCurve" => |c| match own::comp_from_curve(&curve(c, "")) {
            Some(n) => same_comp(c, "n", &n),
            None => refused(c, "composite from curve"),
        },
        "SI_nurbsCompDispose" => |c| {
            let mut slot = if nums(c.input("pp"))[0] == Some(0) { None } else { Some(Box::new(comp(c, ""))) };
            let f = own::comp_dispose(&mut slot);
            same_freed(c, &f).and(|| same_out(c, "pp", &[0]))
        },
        "SI_nurbsSrfNoVlines" => |c| same_eax(c, own::srf_no_vlines(&srf_rec_from(&nums(c.input("srf")))) as u32),
        "SI_nurbsSrfNoUlines" => |c| same_eax(c, own::srf_no_ulines(&srf_rec_from(&nums(c.input("srf")))) as u32),
        "SI_nurbsSrfSwapUV" => |c| {
            let mut s = surface_full(c, "srf");
            let mut host = Rec::new(c);
            match own::srf_swap_uv(&mut host, &mut s) {
                Some(r) => {
                    let mut v = same_eax(c, r as u32).and(|| same_surface(c, "o", &s)).and(|| same_calls(c)).and(|| same_index(c, &host));
                    if let Some(t) = s.trims.as_ref() {
                        for (i, cm) in t.iter().enumerate() {
                            let Some(cm) = cm else { continue };
                            v = v.and(|| same_comp(c, &format!("o.t{i}"), cm));
                        }
                    }
                    v
                }
                None => refused(c, "exchange of U and V"),
            }
        },
        "SI_nurbsSrfCopy" => |c| {
            let s = surface_full(c, "srf");
            let mut host = Rec::new(c);
            match own::srf_copy(&mut host, &s) {
                Some(n) => same_surface_full(c, "n", &n).and(|| same_calls(c)),
                None => refused(c, "surface copy"),
            }
        },
        "SI_nurbsSrfTrmCpy" | "SI_nurbsSrfProjCpy" => |c| {
            let src = surface_full(c, "src");
            let mut dst = Surface::from_parts(srf_rec_from(&nums(c.input("dst"))), None, None, None);
            dst.rec = srf_rec_from(&nums(c.input("dst")));
            let mut host = Rec::new(c);
            let r = if c.name.starts_with("SI_nurbsSrfTrmCpy") { own::srf_trm_cpy(&mut host, &mut dst, &src) } else { own::srf_proj_cpy(&mut dst, &src) };
            match r {
                Some(()) => same_surface_full(c, "o", &dst).and(|| same_calls(c)),
                None => refused(c, "list copy"),
            }
        },
        "SI_nurbsSrfTrmDispose" | "SI_nurbsSrfProjDispose" => |c| {
            let mut s = surface_full(c, "srf");
            let mut host = Rec::new(c);
            let f = if c.name.starts_with("SI_nurbsSrfTrmDispose") { own::srf_trm_dispose(&mut host, &mut s) } else { own::srf_proj_dispose(&mut host, &mut s) };
            same_freed(c, &f)
                .and(|| same_out(c, "o.rec", &srf_rec_words(&s.rec)))
                .and(|| same_calls(c))
                .and(|| Verdict::from(lines_saw_no_tree(), || "the tree pointer was visible to the line disposal".to_string()))
        },
        "SI_nurbsSrfDispose" => |c| {
            let mut slot = Some(Box::new(surface_full(c, "srf")));
            let mut host = Rec::new(c);
            let f = own::srf_dispose(&mut host, &mut slot);
            same_freed(c, &f).and(|| same_out(c, "pp", &[0])).and(|| same_calls(c))
        },
        "SI_nurbsSrfAddTrim" => |c| {
            let mut s = surface_full(c, "srf");
            match own::srf_add_trim(&mut s, &comp(c, "X")) {
                Some(()) => same_eax(c, 1).and(|| same_surface_full(c, "o", &s)),
                None => refused(c, "add trim"),
            }
        },
        _ => return None,
    })
}
