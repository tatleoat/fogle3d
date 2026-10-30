// SPDX-License-Identifier: MIT
//! `nurbs-rendercheck <directory of vector files>`: the trimmed-surface tessellation of
//! `si_nurbs::trimmed` (finding 0044) against the emulated original, on `si-checkkit`.
//! Vectors from `re/tools/si-nurbs/gen_render.py`. `EMUCHECK_MUTATE=1|calls` for the two
//! mutation switches.

use si_checkkit::{Args, Kit};
use si_nurbs::ray_trace as rt;
use si_nurbs::trimmed as t;
use si_nurbs::composite as cp;
use si_nurbs::composite2 as c2;

/// A double passed by value as two stack words `v_<name>lo`, `v_<name>hi`.
fn d(a: &Args, name: &str) -> f64 {
    f64::from_bits(a.v(&format!("{name}lo")) as u64 | (a.v(&format!("{name}hi")) as u64) << 32)
}

/// The port refused (`Halt`): the original reads memory outside the knots or points of a curve
/// (a closed curve whose knots all lie within 1e-5 of the parameter steps `left` down past
/// knot 0), so its result depends on what lies there. Answer with a word of the fill pattern: it
/// differs between the harness's two runs, which makes the case "not compared" (the port's
/// result depends on memory nobody wrote), where a plain 0 would be counted as different.
fn unwritten(m: &si_mem::Mem) -> u32 {
    match m.fill() {
        si_mem::Fill::Pattern { second, .. } => 0x5a5a_0000 | second as u32,
        _ => 0,
    }
}

/// The check's stand-ins for `si_bezPatchInit` / `si_bezPatchCleanup`, which live in `si-spline` (that crate
/// depends on this one, so the port reaches them through `Outside`): their effect on the image, written from the
/// same reading as `si-spline`'s, held against the original's through the heap comparison of the check.
fn route(m: &mut si_mem::Mem, r: si_mem::Routine, args: &[u32]) -> Option<si_mem::Ret> {
    // `NURBS_ROUTED=1`: the calls between the NURBS routines go to the ports (`si_nurbs::routed`), for vectors whose original
    // ran the real callees (generator `re/tools/nurbsfix1/gen_nurbsfix1.py`, groups ctb ...)
    if std::env::var_os("NURBS_ROUTED").is_some() && si_nurbs::routed::ROUTED.contains(&r.addr) {
        let mut none = si_mem::Trace::with(|_m, _r, _a| si_mem::Ret::NONE);
        let mut rt = si_nurbs::routed::Routed::new(&mut none);
        if rt.enabled && !rt.skip.contains(&r.addr) {
            if let Some(ret) = rt.route(m, r.addr, args) {
                if std::env::var_os("NURBS_ROUTED_TRACE").is_some() {
                    eprintln!("  {:08x} {:x?} -> {:?} left {} sp {:x} top {:x}", r.addr, args, ret, m.i32(0x086d_9c20), m.sp(), m.heap_blocks().last().map_or(0, |b| b.0));
                }
                return Some(ret);
            }
        }
    }
    match r.addr {
        0x0850_3724 => {
            let (s, t, bb, p) = (args[0] as i32, args[1] as i32, args[2], args[3]);
            m.set_i32(p, s);
            m.set_i32(p + 4, t);
            for i in 0..12 {
                let w = m.u32(bb.wrapping_add(4 * i));
                m.set_u32(p + 8 + 4 * i, w);
            }
            let n = s.wrapping_mul(t);
            if n < 0x1a {
                m.set_u32(p + 0x98, p + 0xa0);
                m.set_u32(p + 0x9c, p + 0x424);
                m.set_u32(p + 0x7a8, 0);
                m.set_u32(p + 0x7ac, 0);
            } else {
                let a = m.si_calloc(n as u32, 0x24);
                m.set_u32(p + 0x7a8, a);
                let b = m.si_calloc(n as u32, 0x24);
                m.set_u32(p + 0x7ac, b);
                m.set_u32(p + 0x98, a);
                m.set_u32(p + 0x9c, b);
                if b == 0 {
                    return Some(si_mem::Ret::eax(0));
                }
            }
            Some(si_mem::Ret::eax(1))
        }
        0x0850_37cc => {
            let p = args[0];
            for off in [0x7a8, 0x7ac] {
                let a = m.u32(p + off);
                if a != 0 {
                    m.free(a);
                    m.set_u32(p + off, 0);
                }
            }
            m.set_u32(p + 0x98, 0);
            m.set_u32(p + 0x9c, 0);
            Some(si_mem::Ret::eax(p))
        }
        0x0850_3460 => {
            let (s, t, bb) = (args[0] as i32, args[1] as i32, args[2]);
            let p = m.si_calloc(1, 0x7b0);
            if p == 0 {
                return Some(si_mem::Ret::eax(0));
            }
            m.set_i32(p, s);
            m.set_i32(p + 4, t);
            for i in 0..12 {
                let w = m.u32(bb.wrapping_add(4 * i));
                m.set_u32(p + 8 + 4 * i, w);
            }
            let n = s.wrapping_mul(t);
            if n <= 0x19 {
                m.set_u32(p + 0x98, p + 0xa0);
                m.set_u32(p + 0x9c, p + 0x424);
            } else {
                let a = m.si_calloc(n as u32, 0x24);
                m.set_u32(p + 0x7a8, a);
                let b = m.si_calloc(n as u32, 0x24);
                m.set_u32(p + 0x7ac, b);
                m.set_u32(p + 0x98, a);
                m.set_u32(p + 0x9c, b);
            }
            Some(si_mem::Ret::eax(p))
        }
        // genList.c (si-gen, which this crate does not depend on): the four routines of the composite-curve build, written from
        // the same reading as `si-gen`'s and held against the original's through the heap comparison
        0x0851_4fb8 | 0x0851_4ff4 => {
            let (list, first, last) = (args[0], args[1], args[2]);
            m.set_i16(last, 1);
            m.set_i16(first, 1);
            if list == 0 || m.i32(list + 0x10) <= 0 {
                return Some(si_mem::Ret::eax(0));
            }
            if r.addr == 0x0851_4fb8 {
                let head = m.u32(list);
                m.set_u32(list + 0xc, head);
                m.set_u32(list + 8, head);
                m.set_i16(last, 0);
                m.set_i16(first, 0);
                return Some(si_mem::Ret::eax(head));
            }
            let cursor = m.u32(list + 8);
            if cursor == 0 {
                return Some(si_mem::Ret::eax(0));
            }
            let e = m.u32(cursor + 8);
            m.set_u32(list + 8, e);
            let at_head = e == m.u32(list);
            m.set_i16(first, at_head as i16);
            let at_start = e == m.u32(list + 0xc);
            m.set_i16(last, at_start as i16);
            Some(si_mem::Ret::eax(e))
        }
        0x0851_53d4 => Some(si_mem::Ret::eax(if args[0] == 0 { 0 } else { m.u32(args[0]) })),
        0x0851_5154 => {
            let (list, data) = (args[0], args[1]);
            if list == 0 {
                return Some(si_mem::Ret::eax(0));
            }
            let e = m.si_calloc(1, 0x10);
            if e == 0 {
                return Some(si_mem::Ret::eax(0));
            }
            m.set_u32(e, data);
            m.set_u32(e + 0xc, list);
            let tail = m.u32(list + 4);
            if tail == 0 {
                m.set_u32(list, e);
                m.set_u32(list + 4, e);
                m.set_u32(e + 8, e);
                m.set_u32(e + 4, e);
            } else {
                m.set_u32(tail + 8, e);
                let tail = m.u32(list + 4);
                m.set_u32(e + 4, tail);
                let head = m.u32(list);
                m.set_u32(list + 4, e);
                m.set_u32(e + 8, head);
                let (h, t) = (m.u32(list), m.u32(list + 4));
                m.set_u32(h + 4, t);
            }
            let c = m.i32(list + 0x10).wrapping_add(1);
            m.set_i32(list + 0x10, c);
            Some(si_mem::Ret::eax(e))
        }
        0x0851_4ef0 => Some(si_mem::Ret::eax(m.si_calloc(1, 0x18))),
        // SI_genListElemOf with indexEqual as the compare (the only compare the composite routines hand it)
        0x0851_579c => {
            let (list, key, cmp) = (args[0], args[1], args[2]);
            if list == 0 || key == 0 || cmp == 0 {
                return Some(si_mem::Ret::eax(0));
            }
            if m.i32(list + 0x10) <= 0 {
                return Some(si_mem::Ret::eax(0));
            }
            // IterInit / IterNext as the original does (the walk leaves the list's cursor and start where it stopped)
            let head = m.u32(list);
            m.set_u32(list + 0xc, head);
            m.set_u32(list + 8, head);
            let mut e = head;
            let mut at_head = false;
            loop {
                if at_head {
                    return Some(si_mem::Ret::eax(0));
                }
                let data = m.u32(e);
                assert_eq!(cmp, 0x0850_60b4, "unexpected compare");
                if cp::index_equal(m, key, data) as u16 != 0 {
                    return Some(si_mem::Ret::eax(e));
                }
                let cursor = m.u32(list + 8);
                e = m.u32(cursor + 8);
                m.set_u32(list + 8, e);
                at_head = e == m.u32(list);
            }
        }
        0x0851_4f08 => {
            let pl = args[0];
            let list = m.u32(pl);
            if list != 0 {
                let mut e = m.u32(list);
                let mut c = m.i32(list + 0x10);
                while c > 0 && e != 0 {
                    let nx = m.u32(e + 8);
                    m.free(e);
                    e = nx;
                    c -= 1;
                }
                m.free(list);
                m.set_u32(pl, 0);
            }
            Some(si_mem::Ret::eax(0))
        }
        0x0850_dcd8 => {
            let f = |i: usize| {
                let b = (args[i] as u64) | ((args[i + 1] as u64) << 32);
                si_x87::F80::from_f64(f64::from_bits(b))
            };
            let mut acc = si_x87::F80::ZERO;
            for k in 0..3 {
                let dd = f(2 * k) - f(6 + 2 * k);
                acc = acc + dd * dd;
            }
            let (mn, se) = acc.to_bits();
            let mut b = [0u8; 10];
            b[..8].copy_from_slice(&mn.to_le_bytes());
            b[8..].copy_from_slice(&se.to_le_bytes());
            Some(si_mem::Ret::st0(b))
        }
        _ => None,
    }
}

/// callee1: the scratch statics of the real SI_nurbsSrfValueAndNormal (basis temporaries `aj`, `dl`, `dr`, `nb_aj`) that the
/// original writes when it runs as itself in addIntersection's vectors; the port keeps its temporaries in locals.
trait ScratchStatics {
    fn allow_scratch_statics(self) -> Self;
}

impl ScratchStatics for Kit {
    fn allow_scratch_statics(mut self) -> Kit {
        for (name, idx) in [("aj", 249), ("dl", 250), ("dr", 251), ("nb_aj", 253)] {
            for off in 0..8 {
                let note: &'static str = Box::leak(format!("GW:{name}.{idx}+0x{off:x}").into_boxed_str());
                self = self.allow_global_write(note);
            }
        }
        self
    }
}

fn ray_args(a: &Args) -> rt::RayArgs {
    rt::RayArgs { srf: a.v("srf"), bbox: a.v("bbox"), ray: a.v("ray"), p6: a.v("p6"), p7: a.v("p7"), p8: a.v("p8"), mat: a.v("mat") }
}

fn main() {
    Kit::new(&[
        ("NewuvSample", |m, _o, a| t::new_uv_sample(m, d(a, "u"), d(a, "v"), a.i("flags")).into()),
        ("FreeSampleList", |m, _o, a| t::free_sample_list(m, a.v("list")).into()),
        ("ClassifyOneSample", |m, _o, a| t::classify_one_sample(m, a.v("s")).into()),
        ("ClassifySamples", |m, _o, a| t::classify_samples(m, a.v("crv")).into()),
        ("CounterClockwise", |m, _o, a| t::counter_clockwise(m, a.v("crv"), a.v("out")).into()),
        ("NewSampleCrvSegment", |m, _o, a| t::new_sample_crv_segment(m, a.v("s0"), a.v("s1")).into()),
        ("FreeSampleCrv", |m, _o, a| t::free_sample_crv(m, a.v("crv")).into()),
        ("FreevStripArray", |m, _o, a| t::free_vstrip_array(m, a.v("strips"), a.i("n")).into()),
        ("FreeTrimmedTemporaryData", |m, _o, a| t::free_trimmed_temporary_data(m, a.v("a"), a.v("b"), a.v("crvs"), a.v("c"), a.v("d"), a.v("strips"), a.i("n")).into()),
        ("MakeBreakVector", |m, _o, a| t::make_break_vector(m, a.v("samples"), a.i("n"), a.v("dir"), a.v("cnt")).into()),
        ("AddUKnotsToSampleCrvs", |m, _o, a| t::add_u_knots_to_sample_crvs(m, a.v("params"), a.i("n"), a.v("flags"), a.i("k"), a.v("crvs")).into()),
        ("FindUCritcalPoints", |m, _o, a| t::find_u_critical_points(m, a.v("crvs"), a.v("head"), a.v("byv") as u16).into()),
        ("AddUCritcalPointsToSamplings", |m, _o, a| t::add_u_critical_points_to_samplings(m, a.v("crvs"), a.v("nu"), a.v("us"), a.v("nv"), a.v("vs")).into()),
        ("UnlinkMates", |m, _o, a| t::unlink_mates(m, a.v("strips"), a.i("n"), a.v("counts"), a.i("limit")).into()),
        ("CountVerticesTriangles", |m, _o, a| t::count_vertices_triangles(m, a.v("strips"), a.i("n"), a.v("pnv"), a.v("pnt")).into()),
        ("AddHorizontalSamples", |m, _o, a| t::add_horizontal_samples(m, a.v("strips"), a.i("n"), a.v("ys"), a.i("nx"), a.v("xs"), a.v("marks")).into()),
        ("AddUSamplesFullMultiplicity", |m, _o, a| t::add_u_samples_full_multiplicity(m, a.v("strips"), a.i("n")).into()),
        ("SortedSegmentInsert", |m, _o, a| t::sorted_segment_insert(m, a.v("seg"), a.v("strip")).into()),
        ("EncloseTrimRegions", |m, _o, a| t::enclose_trim_regions(m, a.v("strips"), a.i("n")).into()),
        ("OutputTriangles", |m, _o, a| t::output_triangles(m, a.v("strips"), a.i("n"), a.v("mat"), a.i("mid"), a.i("ntri"), a.v("verts"), a.v("tris"), a.v("want") as i16, a.v("itri"), &mut |_, _| {}).into()),
        ("update3DBbox", |m, _o, a| { rt::update_3d_bbox(m, a.v("pt"), a.v("bb")); ().into() }),
        ("update2DBbox", |m, _o, a| { rt::update_2d_bbox(m, a.v("pt"), a.v("bb")); ().into() }),
        ("SI_nurbsCountIntervals", |m, _o, a| (rt::nurbs_count_intervals(m, a.v("dir")) as u32).into()),
        ("calculate2DBbox", |m, _o, a| { rt::calculate_2d_bbox(m, a.v("bez")); ().into() }),
        ("trivialReject", |m, _o, a| rt::trivial_reject(m, a.v("bez"), d(a, "tol")).into()),
        ("perturbMinMax", |m, _o, a| { rt::perturb_min_max(m, d(a, "d"), a.v("lo"), a.v("hi")); ().into() }),
        ("calcDistFromLine", |m, _o, a| { rt::calc_dist_from_line(m, a.v("pos"), a.v("npos"), a.v("neg"), a.v("nneg"), a.v("zero"), a.v("nzero"), a.i("n"), a.v("pts"), a.i("total"), d(a, "a"), d(a, "b"), a.v("flag") as i16); ().into() }),
        ("cleanupBclip", |m, _o, a| { rt::cleanup_b_clip(m, a.v("p0"), a.v("q0"), a.v("p1"), a.v("q1"), a.v("p2"), a.v("q2")); ().into() }),
        ("initBClip", |m, _o, a| rt::init_b_clip(m, a.v("pa"), a.v("pb"), a.v("pc"), a.v("sa"), a.v("sb"), a.v("sc"), a.v("n"), a.v("lo"), a.v("hi"), a.v("nu"), a.v("nv"), a.v("pp"), a.v("bez")).into()),
        ("updateMinMaxFromClip", |m, _o, a| { rt::update_min_max_from_clip(m, a.v("a"), a.i("na"), a.v("b"), a.i("nb"), a.v("c"), a.i("nc"), a.v("lo"), a.v("hi")); ().into() }),
        ("buildLine", |m, _o, a| rt::build_line(m, a.v("pts"), a.v("py"), a.v("px"), a.i("n"), a.i("k"), a.v("flag") as i16).into()),
        ("tBezierClip", |m, _o, a| rt::t_bezier_clip(m, a.v("bez"), a.v("lo"), a.v("hi")).into()),
        ("sBezierClip", |m, _o, a| rt::s_bezier_clip(m, a.v("bez"), a.v("lo"), a.v("hi")).into()),
        ("splitBezT2D", |m, _o, a| rt::split_bez_t2d(m, a.v("bez"), d(a, "t"), a.v("d1"), a.v("d2")).into()),
        ("splitBezS2D", |m, _o, a| rt::split_bez_s2d(m, a.v("bez"), d(a, "t"), a.v("d1"), a.v("d2")).into()),
        ("splitPatchAtMinMax", |m, _o, a| rt::split_patch_at_min_max(m, a.v("bez"), d(a, "a"), d(a, "b"), a.v("dir") as i16, a.v("d1"), a.v("d2"), a.v("d3")).into()),
        ("binarySplitPatch", |m, _o, a| rt::binary_split_patch(m, a.v("bez"), a.v("d1"), a.v("d2"), a.v("dir") as i16).into()),
        ("interOnBorder", |m, _o, a| rt::inter_on_border(m, a.v("bez"), d(a, "a2"), d(a, "a3"), d(a, "a4"), d(a, "a5"), d(a, "a6"), a.v("slo"), a.v("shi"), a.v("tlo"), a.v("thi"), a.v("st")).into()),
        ("projBezPts2D", |m, _o, a| rt::proj_bez_pts_2d(m, a.v("bez"), a.v("ray")).into()),
        ("projBezBboxPts2D", |m, _o, a| rt::proj_bez_bbox_pts_2d(m, a.v("bez"), a.v("ray")).into()),
        ("interSetup", |m, o, a| { rt::inter_setup(m, o, a.v("patch"), a.v("a"), a.v("b"), a.v("c"), a.v("bbox"), a.v("x0"), a.v("x1"), a.v("y0"), a.v("y1"), a.v("dx"), a.v("dy")); ().into() }),
        ("interCleanup", |m, o, a| { rt::inter_cleanup(m, o, a.v("a"), a.v("b"), a.v("c")); ().into() }),
        ("addIntersection", |m, o, a| rt::add_intersection(m, o, a.v("up"), a.v("vp"), d(a, "x0"), d(a, "x1"), d(a, "y0"), d(a, "y1"), a.v("bez"), a.v("ray"), a.v("srf"), a.v("p6"), a.v("mat")).into()),
        ("intersectBezT2D", |m, o, a| rt::intersect_bez_t2d(m, o, a.v("bez"), d(a, "t"), ray_args(a)).into()),
        ("intersectBezS2D", |m, o, a| rt::intersect_bez_s2d(m, o, a.v("bez"), d(a, "t"), ray_args(a)).into()),
        ("performBsplit", |m, o, a| rt::perform_bsplit(m, o, a.v("bez"), a.v("ha"), a.v("hb"), a.v("flag") as i16, d(a, "t"), [d(a, "x0"), d(a, "x1"), d(a, "y0"), d(a, "y1")], ray_args(a)).into()),
        ("updateHierBbox", |m, _o, a| { rt::update_hier_bbox(m, a.v("node")); ().into() }),
        ("buildBboxHier", |m, o, a| rt::build_bbox_hier(m, o, a.v("items"), a.i("nu"), a.i("nv")).into()),
        ("intersectBboxHier", |m, o, a| rt::intersect_bbox_hier(m, o, a.v("node"), d(a, "t"), a.v("srf"), a.v("ray"), a.v("mat")).into()),
        ("convertToBez", |m, o, a| rt::convert_to_bez(m, o, a.v("srf"), a.v("pv"), a.v("pu"), a.v("mat")).into()),
        ("si_nurbsRayTraceSrf2", |m, o, a| rt::nurbs_ray_trace_srf2(m, o, a.v("srf"), a.v("ray"), a.v("slot"), a.v("mat")).into()),
        ("si_nurbsTrimConvertToBez", |m, o, a| rt::nurbs_trim_convert_to_bez(m, o, a.v("srf"), a.v("pv"), a.v("pu"), a.v("mat")).into()),
        ("si_nurbsCompositeCurveCreate", |m, _o, a| {
            let mut b = [0u32; 12];
            for (i, w) in b.iter_mut().enumerate() {
                *w = a.v(&format!("b{i}"));
            }
            cp::composite_curve_create(m, a.v("curve"), b, a.v("flag")).into()
        }),
        ("si_nurbsCompositeCurveDestroy", |m, _o, a| { cp::composite_curve_destroy(m, a.v("pp")); ().into() }),
        ("SI_nurbsCompGetOpenNC", |m, _o, a| cp::comp_get_open_nc(m, a.i("n"), a.i("order"), a.v("flag")).into()),
        ("SI_nurbsCompGetClosedNC", |m, _o, a| cp::comp_get_closed_nc(m, a.i("n"), a.i("order"), a.v("flag")).into()),
        ("getBorder", |m, _o, a| cp::get_border(m, a.v("comp"), a.v("which") as u16).into()),
        ("nextBorder", |_m, _o, a| cp::next_border(a.v("b")).into()),
        ("allCorners", |m, _o, a| cp::all_corners(m, a.v("a"), a.v("b"), a.v("x"), a.v("y")).into()),
        ("addCorner", |m, _o, a| { cp::add_corner(m, a.v("comp"), a.v("idx"), a.v("border"), a.v("box")); ().into() }),
        ("insertCorners", |m, _o, a| { cp::insert_corners(m, a.v("comp"), a.v("idx"), a.v("from"), a.v("to"), a.v("box")); ().into() }),
        ("onBorderFlags", |m, _o, a| cp::on_border_flags(m, a.v("p"), a.v("box")).into()),
        ("indexDestroy", |m, _o, a| { cp::index_destroy(m, a.v("pp")); ().into() }),
        ("indexEqual", |m, _o, a| cp::index_equal(m, a.v("a"), a.v("b")).into()),
        ("insertCornerWrap", |m, _o, a| cp::insert_corner_wrap(m, a.v("comp")).into()),
        ("si_nurbsCompositeCurvePointOnBorder", |_m, _o, a| {
            let b: Vec<f64> = (0..6).map(|i| d(a, &format!("b{i}"))).collect();
            cp::composite_curve_point_on_border(d(a, "px"), d(a, "py"), [b[0], b[1], b[2], b[3], b[4], b[5]]).into()
        }),
        ("SI_nurbsSrfCrvOnBorder", |m, _o, a| cp::srf_crv_on_border(m, a.v("srf"), a.v("comp")).into()),
        ("addCompositeNoInter", |m, o, a| cp::add_composite_no_inter(m, o, a.v("item"), a.v("la"), a.v("lb")).into()),
        ("findOrientation", |m, o, a| cp::find_orientation(m, o, a.v("e")).into()),
        ("addCompositeInter", |m, o, a| cp::add_composite_inter(m, o, a.v("item"), a.v("la"), a.v("lb"), a.v("lc")).into()),
        ("si_nurbsCompositeCurveBuild", |m, o, a| cp::composite_curve_build(m, o, a.v("list"), a.v("la"), a.v("lb"), a.v("lc")).into()),
        ("rebuildKnotVector", |m, o, a| cp::rebuild_knot_vector(m, o, a.v("crv"), a.v("out"), a.i("order")).into()),
        ("fitFailed", |m, o, a| cp::fit_failed(m, o, a.v("crv")).into()),
        ("nurbsSrfLinearizeTrimCrvs", |m, o, a| cp::nurbs_srf_linearize_trim_crvs(m, o, a.v("srf")).into()),
        ("curveInCurve", |m, o, a| cp::curve_in_curve(m, o, a.v("a"), a.v("b")).into()),
        ("linearTrimCrvsAdjust", |m, o, a| { cp::linear_trim_crvs_adjust(m, o, a.v("curves"), a.v("list"), a.v("box")); ().into() }),
        ("SI_nurbsProjectedCurveDelete", |m, o, a| cp::projected_curve_delete(m, o, a.v("obj"), a.v("idx")).into()),
        ("ptsMarkForDelete", |m, o, a| c2::pts_mark_for_delete(m, o, a.v("curves"), a.v("poly"), a.v("box")).into()),
        ("elevateFromLinear", |m, o, a| c2::elevate_from_linear(m, o, a.v("comp"), a.v("cur"), a.v("total"), a.i("stop")).into()),
        ("fitHighDegree", |m, o, a| {
            let b: Vec<f64> = (0..6).map(|i| d(a, &format!("b{i}"))).collect();
            c2::fit_high_degree(m, o, a.v("comp"), a.v("cur"), a.i("stop"), a.v("total"), [b[0], b[1], b[2], b[3], b[4], b[5]]).into()
        }),
        ("buildHighDegree", |m, o, a| {
            let b: Vec<f64> = (0..6).map(|i| d(a, &format!("b{i}"))).collect();
            c2::build_high_degree(m, o, a.v("comp"), [b[0], b[1], b[2], b[3], b[4], b[5]]).into()
        }),
        ("EvaluateVertices", |m, o, a| {
            t::evaluate_vertices(m, o, a.v("srf"), a.v("strips"), a.i("n"), a.v("out"), a.v("ys"), a.i("nx"), a.v("xs"), a.v("flg")).into()
        }),
        ("EvaluateTrimmedNurbsFromBaseGeometry", |m, o, a| {
            t::evaluate_trimmed_nurbs_from_base_geometry(m, o, a.v("srf"), a.v("base"), a.v("out"), a.i("n"), a.v("strips")).into()
        }),
        ("SI_noTrimmedTriangles", |m, _o, a| t::no_trimmed_triangles(m, a.v("srf")).unwrap_or_else(|_| unwritten(m)).into()),
        ("TriangulateTrimmedNurbsSrf", |m, o, a| {
            t::triangulate_trimmed_nurbs_srf(
                m, o, a.v("base"), a.v("srf"), a.v("mat"), a.i("mid"), a.v("vn"), a.v("tn"), a.v("verts"), a.v("tris"), a.v("want") as i16,
                a.v("idx"), &mut |_, _| {},
            )
            .unwrap_or_else(|_| {
                unwritten(m);
            });
            // `NURBS_HEAPTRACE=1`: the port's heap blocks in malloc order and the blocks it freed, to set against the
            // original's `heapN=` / `freed=` (re/tools/si-nurbs/heapdiff.py).
            if std::env::var_os("NURBS_HEAPTRACE").is_some() {
                let hb: Vec<String> = m.heap_blocks().iter().map(|(p, n)| format!("{p:08x}:{n}")).collect();
                let fr: Vec<String> = m.freed().iter().map(|p| format!("{p:08x}")).collect();
                eprintln!("HEAP {}\nFREED {}", hb.join(" "), fr.join(" "));
            }
            ().into()
        }),
        ("IntersectVStrips", |m, _o, a| t::intersect_vstrips(m, a.v("strips"), a.i("n"), a.v("ys"), a.v("crvs"), &mut |_| {}).into()),
        ("CreateDomainBoundary", |m, _o, a| t::create_domain_boundary(m, a.i("nu"), a.v("us"), a.i("nv"), a.v("vs")).into()),
        ("PtInteriorToEdge", |m, _o, a| t::pt_interior_to_edge(m, a.v("a"), a.v("b"), a.v("c")).into()),
        ("advanceTri", |m, _o, a| t::advance_tri(m, a.v("pa"), a.v("pb"), a.v("pc")).into()),
        ("NewSampleCrv", |m, _o, a| t::new_sample_crv(m, a.v("comp"), d(a, "u0"), d(a, "u1"), d(a, "v0"), d(a, "v1")).unwrap_or_else(|_| unwritten(m)).into()),
        ("si_validTrimCurve", |m, _o, a| {
            let mut b = [0u32; 12];
            for (i, w) in b.iter_mut().enumerate() {
                *w = a.v(&format!("b{i}"));
            }
            t::si_valid_trim_curve(m, b, a.v("comp")).unwrap_or_else(|_| unwritten(m)).into()
        }),
    ])
    .route(route)
    // `left` and (trimtri1) the scratch statics of SI_nurbsSrfExtractCurveSeg (aj, dl, dr, nb_dlr, nb_aj), which the image ports
    // grow as the original does: vectors that do not declare them (the original's GW: notes) leave them uncompared; a vector that
    // declares them (gen_trimscene.py's g086d9c0c_aj ...) has them compared
    .reset_globals(&[(0x086d_9c20, 0), (0x086d_9c0c, 0), (0x086d_9c10, 0), (0x086d_9c14, 0), (0x086d_9c18, 0), (0x086d_9c1c, 0)])
    .allow_global_write("GW:left.272+0x0")
    .allow_scratch_statics()
    .run()
}
