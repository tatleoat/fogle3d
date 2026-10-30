// SPDX-License-Identifier: MIT
//! `nurbs-trim9check <directory of vector files>`: `si_nurbs::raylink` (ray.c, the search and linking of rays; finding 0044, section
//! Trim9) against the emulated original, on `si-checkkit`. Vectors from `re/tools/si-nurbs/gen_trim9.py`. `EMUCHECK_MUTATE=1|calls` for
//! the two mutation switches.

use si_checkkit::{Args, Kit};
use si_nurbs::inter as i;
use si_nurbs::raylink as r;
use si_nurbs::raylink::Sampling;
use si_nurbs::trimutils3 as t3;

/// A double passed by value as two stack words `<name>lo`, `<name>hi`.
fn d(a: &Args, name: &str) -> f64 {
    f64::from_bits(a.v(&format!("{name}lo")) as u64 | (a.v(&format!("{name}hi")) as u64) << 32)
}

/// The stand-in for `si_nurbsRayTraceSrf2` (same as `trace_body` of gen_trim9.py): the ray gets the next {list, count} of the slot buffer
/// ({n, next, n * {list, count, status}}); the entry's status, 0 when none is left.
fn trace(m: &mut si_mem::Mem, args: &[u32]) -> si_mem::Ret {
    let (ray, slot) = (args[1], args[2]);
    let (n, k) = (m.u32(slot), m.u32(slot + 4));
    if k >= n {
        return si_mem::Ret::eax(0);
    }
    let e = slot + 8 + 12 * k;
    let (l, c, s) = (m.u32(e), m.u32(e + 4), m.u32(e + 8));
    m.set_u32(ray + 0x54, l);
    m.set_u32(ray + 0x50, c);
    m.set_u32(slot + 4, k + 1);
    si_mem::Ret::eax(s)
}

fn ax(v: i16) -> u32 {
    v as i32 as u32
}

fn main() {
    Kit::new(&[
        ("si_nurbsRayConnectNoTrans", |m, _o, a| ax(r::connect_no_trans(m, a.v("r1"), a.v("r2"), a.v("dir") as i16, a.v("crv"))).into()),
        ("si_nurbsRayLinkCurLess", |m, _o, a| ax(r::link_cur_less(m, a.v("r1"), a.v("r2"), a.v("e3"), a.v("e4"), a.v("dir") as i16, a.v("crv"))).into()),
        ("si_nurbsRayLinkCurMore", |m, _o, a| ax(r::link_cur_more(m, a.v("r1"), a.v("r2"), a.v("e3"), a.v("e4"), a.v("dir") as i16, a.v("crv"))).into()),
        ("upperIsNull", |m, _o, a| {
            ax(r::upper_is_null(m, a.v("e"), a.v("pp"), a.v("g"), a.v("list"), a.v("pf"), a.v("pl"), a.v("pflag"), a.v("dir") as i16)).into()
        }),
        ("addNewRay", |m, o, a| {
            ax(r::add_new_ray(m, o, a.v("e"), a.v("pp"), a.v("g"), a.v("r4"), a.v("r5"), a.v("pl8"), a.v("pl4"), a.v("list"), a.v("pf"), a.v("pl"), a.v("pflag"), a.v("dir") as i16)).into()
        }),
        ("nextIsUpper", |m, o, a| {
            r::next_is_upper(m, o, a.v("pe1"), a.v("ray2"), a.v("ppa"), a.v("ppb"), a.v("ppc"), a.v("pflag"));
            ().into()
        }),
        ("curIsLower", |m, o, a| {
            r::cur_is_lower(m, o, a.v("pe1"), a.v("pe2"), a.v("pa3"), a.v("pa4"), a.v("pa5"), a.v("pa6"), a.v("list"), a.v("pf"), a.v("pl"), a.v("pflag"));
            ().into()
        }),
        ("curIsLowerUpperIsNext", |m, o, a| {
            r::cur_is_lower_upper_is_next(m, o, a.v("pe1"), a.v("pe2"), a.v("pa3"), a.v("pa4"), a.v("pa5"), a.v("pa6"), a.v("list"), a.v("pf"), a.v("pl"), a.v("pflag"));
            ().into()
        }),
        ("curNotLowerUpperNotNext", |m, o, a| {
            r::cur_not_lower_upper_not_next(m, o, a.v("pe1"), a.v("pe2"), a.v("pa3"), a.v("pa4"), a.v("pa5"), a.v("pa6"), a.v("list"), a.v("pf"), a.v("pl"), a.v("pflag"));
            ().into()
        }),
        ("addRayToSampling", |m, o, a| {
            let s = Sampling { pe3: a.v("pe3"), pe4: a.v("pe4"), list: a.v("list"), pf: a.v("pf"), pl: a.v("pl"), pflag: a.v("pflag") };
            ax(r::add_ray_to_sampling(m, o, a.v("p1"), a.v("p2"), a.v("p3"), a.v("p4"), a.v("trim"), a.v("dir") as i32, &s)).into()
        }),
        ("peturbBinarySplit", |m, o, a| r::peturb_binary_split(m, o, a.v("r1"), a.v("r2"), d(a, "t"), d(a, "step"), a.v("trim")).into()),
        ("si_nurbsRayBinarySearch", |m, o, a| r::ray_binary_search(m, o, a.v("e1"), a.v("e2"), a.v("pp3"), a.v("pp4"), a.v("trim")).into()),
        ("bSearchIsNull", |m, o, a| {
            let s = Sampling { pe3: 0, pe4: 0, list: a.v("list"), pf: a.v("pf"), pl: a.v("pl"), pflag: a.v("pflag") };
            ax(r::b_search_is_null(m, o, a.v("elem"), a.v("pp"), a.v("r3"), a.v("r4"), a.v("r5"), a.v("r6"), &s, a.v("dir") as i16, a.v("trim"))).into()
        }),
        ("si_nurbsRayConnectTrans", |m, o, a| {
            ax(r::connect_trans(m, o, a.v("trim"), a.v("elem"), a.v("pp"), a.v("r4"), a.v("r5"), a.v("list"), a.v("pf"), a.v("pl"), a.v("pflag"), a.v("dir") as i16)).into()
        }),
        ("si_divLnTo2Planes", |m, _o, a| {
            let f = |n: &str| f32::from_bits(a.v(n));
            t3::div_ln_to_2_planes(m, f("p1"), f("p2"), f("p3"), d(a, "d1"), f("d2"), f("d3"), a.v("out")).into()
        }),
        ("si_addImageData", |m, o, a| {
            t3::add_image_data(m, o, a.v("st"), a.v("srf"), a.v("ref") as i32, a.v("n") as i32, a.v("uv"), a.v("pts"), a.v("flag") as i16);
            ().into()
        }),
        ("SI_nurbsSrfExtractTrimCurve", |m, o, a| t3::srf_extract_trim_curve(m, o, a.v("srf"), a.v("index") as i16, a.v("table")).into()),
        ("SI_nurbsTrimRemvPtsOnBorder", |m, o, a| t3::trim_remv_pts_on_border(m, o, a.v("srf"), a.v("pcount")).into()),
    ])
    .stand_in("si_nurbsRayTraceSrf2", trace)
    .route(|m, r, args| i::route(m, r, args))
    .run()
}
