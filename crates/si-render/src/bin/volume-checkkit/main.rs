// SPDX-License-Identifier: MIT
//! `volume-checkkit <directory of vector files>`: the volume-effect files (`vceval.c`, `range.c`,
//! `bi_search.c`, `hooks.c`, ...) on `si-checkkit` against the emulated original (generator
//! `re/tools/si-render/volume/gen_vol.py`, finding 0012, section 28).

mod files;

use si_checkkit::Kit;
use si_mem::Mem;
use si_render::{bloom::*, bi_search::*, hooks::*, io, split_proc::*, update_voxel::*, range::*, vceval::*, volume::{ratio_from_weight, weight_from_ratio, wyvill, wyvill2, CORNERS}};
use si_x87::F80;

/// si-checkkit compares `eax` only, so a routine that answers in `st0` is held here: the port's register
/// against the vector's `st0`; a difference is a write to `POISON`, which the kit reports as a stray write
/// (a different case). The mutation switch `EMUCHECK_MUTATE=1` flips a bit of the register first.
const POISON: u32 = 0x0883_4e00;

fn st0_held(m: &mut Mem, a: &si_checkkit::Args, got: F80) -> si_checkkit::Ret {
    let (mut mant, se) = got.to_bits();
    if std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "1") {
        mant ^= 1;
    }
    if let Some(f) = a.case().try_output("st0") {
        if !f.is_empty() && !f.same_f80(F80::from_bits(mant, se)) {
            if std::env::var("VOLDBG").is_ok() {
                eprintln!("st0 differs: original {:016x}.{:04x}  port {:016x}.{:04x}", f.f80().to_bits().0, f.f80().to_bits().1, mant, se);
            }
            let w = m.u32(POISON);
            m.set_u32(POISON, !w);
        }
    }
    ().into()
}

fn dbl(a: &si_checkkit::Args, name: &str) -> f64 {
    a.case().input(&format!("v_{name}")).f64()
}

fn main() {
    Kit::new(&[
        ("VC_initEvalContrib", |m, _o, a| init_eval_contrib(m, a.i("n")).into()),
        ("VC_createTri", |m, _o, a| create_tri(m, a.v("tri"), a.v("cell")).into()),
        ("VC_decompPoly", |m, _o, a| decomp_poly(m, a.v("poly"), a.i("n"), a.v("cell")).into()),
        ("VC_createVtx", |m, _o, a| create_vtx(m, a.v("pt"), a.i("index")).into()),
        ("VC_vtxLookup", |m, _o, a| vtx_lookup(m, a.v("edge"), a.v("cell"), a.v("out")).into()),
        ("VC_markEdge", |m, _o, a| { mark_edge(m, a.v("c"), a.v("marks")); ().into() }),
        ("VC_getPt", |m, _o, a| { get_pt(m, a.v("out"), a.v("idx"), a.v("g"), a.v("v")); ().into() }),
        ("VC_traceCube", |m, _o, a| trace_cube(m, a.v("flags"), a.v("cell"), a.v("g"), a.v("v")).into()),
        ("VC_leaf_action", |m, _o, a| leaf_action(m, a.v("p"), dbl(&a, "dx"), dbl(&a, "dy"), a.v("cell"), a.i("s")).into()),
        ("VC_quad_divide", |m, _o, a| quad_divide(m, a.v("p"), dbl(&a, "dx"), dbl(&a, "dy"), a.v("cell"), a.i("n")).into()),
        ("VC_mbtoP", |m, _o, a| mbto_p(m, a.i("level"), dbl(&a, "scale"), a.v("bounds"), a.i("poly"), a.v("box"), a.i("update") as i16).into()),
        ("VC_InitMbtoP", |m, _o, _a| init_mbto_p(m).into()),
        ("VC_ratioFromWeight", |m, _o, a| st0_held(m, &a, ratio_from_weight(dbl(&a, "w")))),
        ("VC_weightFromRatio", |m, _o, a| st0_held(m, &a, weight_from_ratio(dbl(&a, "r")))),
        ("VC_fgetstr", |m, o, a| { files::begin(a); io::fgetstr(m, o, a.v("buf"), files::HANDLE0).into() }),
        ("VC_get_whitespace", |m, o, a| { files::begin(a); io::get_whitespace(m, o, files::HANDLE0).into() }),
        ("VC_read_blob_data", |m, o, a| { files::begin(a); io::read_blob_data(m, o, files::HANDLE0).into() }),
        ("VC_read_group_data", |m, o, a| { files::begin(a); io::read_group_data(m, o, files::HANDLE0).into() }),
        ("VC_read_edge_data", |m, o, a| { files::begin(a); io::read_edge_data(m, o, files::HANDLE0).into() }),
        ("VC_input_data", |m, o, a| { files::begin(a); io::input_data(m, o, files::HANDLE0).into() }),
        ("VC_write_blob_data", |m, o, a| { files::begin(a); io::write_blob_data(m, o, files::HANDLE0); files::emit(o); ().into() }),
        ("VC_write_group_data", |m, o, a| { files::begin(a); io::write_group_data(m, o, files::HANDLE0); files::emit(o); ().into() }),
        ("VC_write_edge_data", |m, o, a| { files::begin(a); io::write_edge_data(m, o, files::HANDLE0); files::emit(o); ().into() }),
        ("VC_output_data", |m, o, a| { files::begin(a); let r = io::output_data(m, o, files::HANDLE0); files::emit(o); r.into() }),
        ("VC_wyvill", |m, _o, a| st0_held(m, &a, wyvill(dbl(&a, "x")))),
        ("VC_wyvill2", |m, _o, a| st0_held(m, &a, wyvill2(dbl(&a, "x")))),
        ("VC_evalComputeSQr2", |m, _o, a| { let r = eval_compute_sqr2(m, a.v("v"), a.i("index")); st0_held(m, &a, r) }),
        ("VC_evalList", |m, _o, a| { let r = eval_list(m, a.v("list"), a.v("pt"), a.v("cb")); st0_held(m, &a, r) }),
        ("VC_evalContrib", |m, _o, a| {
            eval_contrib(m, a.v("list"), a.v("pt"), a.v("cb"));
            ().into()
        }),
        ("VC_evalDens", |m, _o, a| { let r = eval_dens(m, a.v("list"), a.v("pt")); st0_held(m, &a, r) }),
        ("VC_evalSurf", |m, _o, a| eval_surf(m, a.v("list"), a.v("pt"), a.v("surf")).into()),
        ("VC_evalNorm", |m, _o, a| eval_norm(m, a.v("list"), a.v("pt"), a.v("out"), dbl(&a, "step"), a.i("tries")).into()),
        ("VC_getmaxmin", |m, _o, a| {
            get_max_min(m, a.v("out"));
            ().into()
        }),
        ("VC_influ_test", |m, _o, a| influ_test(m, a.v("list"), a.v("pt"), dbl(&a, "rx"), dbl(&a, "ry")).into()),
        ("VC_extract_Zslab", |m, _o, a| {
            extract_zslab(m, a.v("list"), dbl(&a, "z"), dbl(&a, "width"));
            ().into()
        }),
        ("VC_getBoundingVolume", |m, _o, a| {
            get_bounding_volume(m, a.i("index"));
            ().into()
        }),
        ("VC_check_upbv", |m, _o, a| check_upbv(m, a.v("a"), a.v("b"), a.i("c")).into()),
        ("VC_initLocalLists", |m, _o, a| init_local_lists(m, a.i("n")).into()),
        ("VC_initOrigLists", |m, _o, _a| init_orig_lists(m).into()),
        ("VC_setMBParam", |m, _o, a| set_mb_param(m, a.i("index"), a.i("which"), a.v("p")).into()),
        ("VC_getMBParam", |m, _o, a| get_mb_param(m, a.i("index"), a.i("which"), a.v("o")).into()),
        ("VC_createMB", |m, _o, a| create_mb(m, a.v("mat"), a.v("out")).into()),
        ("VC_destroyMB", |m, _o, a| destroy_mb(m, a.i("index")).into()),
        ("VC_listAllMBs", |m, _o, a| list_all_mbs(m, a.v("list"), a.v("count")).into()),
        ("VC_numMBs", |m, _o, a| num_mbs(m, a.v("p")).into()),
        ("VC_addMBtoGroup", |m, _o, a| add_mb_to_group(m, a.i("mb"), a.i("group")).into()),
        ("VC_removeMBfromGroup", |m, _o, a| remove_mb_from_group(m, a.i("mb"), a.i("group")).into()),
        ("VC_dissolveMBGroup", |m, _o, a| dissolve_mb_group(m, a.i("group")).into()),
        ("VC_destroyMBGroup", |m, _o, a| destroy_mb_group(m, a.i("group")).into()),
        ("VC_listMBGroup", |m, _o, a| list_mb_group(m, a.i("group"), a.v("list"), a.v("count")).into()),
        ("VC_memberMBGroup", |m, _o, a| member_mb_group(m, a.i("mb"), a.i("group"), a.v("o")).into()),
        ("VC_getMBGroupID", |m, _o, a| get_mb_group_id(m, a.i("mb"), a.v("o")).into()),
        ("VC_listAllMBGroups", |m, _o, a| list_all_mb_groups(m, a.v("list"), a.v("count")).into()),
        ("VC_numMBGroups", |m, _o, a| num_mb_groups(m, a.v("p")).into()),
        ("VC_createMBLink", |m, _o, a| create_mb_link(m, a.i("a"), a.i("b")).into()),
        ("VC_destroyMBLink", |m, _o, a| destroy_mb_link(m, a.i("a"), a.i("b")).into()),
        ("VC_listAllMBLinks__FPPA1_iPi", |m, _o, a| list_all_mb_links(m, a.v("list"), a.v("count")).into()),
        ("VC_listMBLinks", |m, _o, a| list_mb_links(m, a.i("group"), a.v("list"), a.v("count")).into()),
        ("VC_numMBLinks", |m, _o, a| num_mb_links(m, a.v("p")).into()),
        ("VCCleanHooks", |m, _o, _a| {
            vc_clean_hooks(m);
            ().into()
        }),
        ("VC_rmVertex", |m, _o, a| rm_vertex(m, a.i("tri"), a.i("vtx")).into()),
        ("VC_rmTriangle", |m, _o, a| rm_triangle(m, a.i("tri")).into()),
        ("VC_rm_tri_in_voxel", |m, _o, a| rm_tri_in_voxel(m, a.i("vox")).into()),
        ("VC_rm_tri_in_upbv", |m, _o, a| rm_tri_in_upbv(m, a.i("cells"), a.v("step"), a.v("low"), a.v("box"), a.v("out")).into()),
        ("VC_rmAllTri", |m, _o, _a| rm_all_tri(m).into()),
        ("VC_realSplitMBList", |m, _o, a| real_split_mb_list(m, a.v("list"), a.v("count"), a.v("parts")).into()),
        ("VC_splitMBList", |m, _o, a| split_mb_list(m, a.v("list"), a.v("count"), a.v("parts")).into()),
        ("VC_initSplitData", |m, _o, a| init_split_data(m, a.i("n"), a.i("edges")).into()),
        ("VCCleanSplit", |m, _o, _a| {
            vc_clean_split(m);
            ().into()
        }),
        ("VC_bi_search", |m, _o, a| {
            bi_search(m, a.v("list"), a.v("a"), a.v("b"));
            ().into()
        }),
    ])
    .setup(|m| {
        io::init_rodata(m);
        // the eight corners of the cube, each a triple of plus/minus one (gen_vol.py's table)
        for c in 0..8u32 {
            for k in 0..3u32 {
                m.set_f64(CORNERS + 24 * c + 8 * k, if c >> k & 1 == 1 { 1.0 } else { -1.0 });
            }
        }
    })
    .normalise_stack_args()
    .given_lib_column()
    .route(files::route)
    .run()
}
