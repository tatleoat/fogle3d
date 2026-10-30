// SPDX-License-Identifier: MIT
//! `mrmain-checkkit <directory of vector files>`: the checks of `mi_mr_main.c` (finding 0042) on `si-checkkit`.

use si_checkkit::Kit;

// the C library's streams for the port (finding 0039's model, shared by path: it is the tools check's file)
#[path = "../../../../si-tools/src/bin/tools-checkkit/files.rs"]
mod files;
use si_mental::mi_calc as c;
use si_mental::mi_mr_main as p;

/// `--exit-cases`: the cases whose original ended in `exit(N)` (`si_dumpUsageInfo`), which the kit does not compare. A case
/// is identical when the port prints the same stderr text and calls `exit` with the same code, as the vector says
/// (`stderr="..."`, `! STOP:exit:N`). `EMUCHECK_MUTATE=1`/`calls` damage the port's text / drop its exit call.
fn exit_cases(dir: &str) {
    let text = std::fs::read_to_string(format!("{dir}/si_dumpUsageInfo.vec")).unwrap_or_default();
    let mutate = std::env::var("EMUCHECK_MUTATE").unwrap_or_default();
    let (mut n, mut same, mut diff) = (0, 0, 0);
    for line in text.lines().filter(|l| l.starts_with("si_dumpUsageInfo ")) {
        n += 1;
        let word = |key: &str| -> u32 {
            let at = line.find(key).map(|i| i + key.len()).unwrap_or(0);
            u32::from_str_radix(line[at..].split(' ').next().unwrap_or("0"), 16).unwrap_or(0)
        };
        let want_code: u32 = line.rsplit("STOP:exit:").next().and_then(|c| c.trim().parse().ok()).unwrap_or(u32::MAX);
        let q = line.find("stderr=\"").map(|i| i + 8).unwrap_or(0);
        let q_end = line[q..].find("\"").map(|i| q + i).unwrap_or(q);
        let (mut want, mut it) = (Vec::new(), line[q..q_end].bytes().peekable());
        while let Some(b) = it.next() {
            if b == b'\\' && it.peek() == Some(&b'x') {
                it.next();
                let h: Vec<u8> = (0..2).filter_map(|_| it.next()).collect();
                want.push(u8::from_str_radix(std::str::from_utf8(&h).unwrap_or("0"), 16).unwrap_or(0));
            } else {
                want.push(b);
            }
        }
        let mut m = si_mem::Mem::new();
        let prog = word("g086ef9ac_prog=");
        m.set_u32(0x086e_f9ac, prog);
        m.set_u32(prog, word("pn0="));
        m.set_u32(prog + 4, word("pn1="));
        let mut o = si_mem::Trace::new();
        p::si_dump_usage_info(&mut m, &mut o);
        let mut got: Vec<u8> = o.printed.iter().filter(|x| x.0 == 2).flat_map(|x| x.1.clone()).collect();
        if mutate == "1" {
            if let Some(b) = got.first_mut() { *b ^= 1; }
        }
        let code = o.calls.iter().find(|c| c.routine.name == "exit").map(|c| c.args[0]);
        let code = if mutate == "calls" { None } else { code };
        if got == want && code == Some(want_code) { same += 1 } else { diff += 1 }
    }
    println!("exit cases: si_dumpUsageInfo {n} cases, {same} identical, {diff} different");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 2 && args[2] == "--exit-cases" {
        return exit_cases(&args[1]);
    }
    Kit::new(&[
        ("vector_plus_vector", |m, _o, a| { c::vector_plus_vector(m, a.v("out"), a.v("a"), a.v("b")); ().into() }),
        ("vector_minus_vector", |m, _o, a| { c::vector_minus_vector(m, a.v("out"), a.v("a"), a.v("b")); ().into() }),
        ("vector_X_scalar", |m, _o, a| { c::vector_x_scalar(m, a.v("v"), a.f("s")); ().into() }),
        ("vector_dot_vector", |m, _o, a| {
            // the original answers in st0, which the kit does not compare: a wrong answer damages the input buffer `a`, so the case turns different
            let got = c::vector_dot_vector(m, a.v("a"), a.v("b"));
            if !a.case().output("st0").same_f80(got) {
                let p = a.v("a");
                let w = m.u32(p);
                m.set_u32(p, !w);
            }
            ().into()
        }),
        ("vector_X_vector", |m, _o, a| { c::vector_x_vector(m, a.v("out"), a.v("a"), a.v("b")); ().into() }),
        ("normalize_vector", |m, o, a| { c::normalize_vector(m, o, a.v("v")); ().into() }),
        ("init_matrix", |m, _o, a| { c::init_matrix(m, a.v("mat")); ().into() }),
        ("vector_X_matrix", |m, _o, a| { c::vector_x_matrix(m, a.v("out"), a.v("v"), a.v("mat")); ().into() }),
        ("matrix_X_matrix", |m, _o, a| { c::matrix_x_matrix(m, a.v("out"), a.v("a"), a.v("b")); ().into() }),
        ("rotate_matrix", |m, _o, a| { c::rotate_matrix(m, a.v("out"), a.v("rot")); ().into() }),
        ("invert_matrix", |m, o, a| c::invert_matrix(m, o, a.v("out"), a.v("inp")).into()),
        ("matrixZAlign", |m, _o, a| { c::matrix_z_align(m, a.v("v"), a.v("out")); ().into() }),
        ("SI_setmi2PipeConnection", |m, _o, _a| p::si_set_mi2_pipe_connection(m).into()),
        ("SI_setmi2Output", |m, _o, a| p::si_set_mi2_output(m, a.v("v")).into()),
        ("SI_getmi2Output", |m, _o, _a| p::si_get_mi2_output(m).into()),
        ("SI_mi2IsIncremental", |m, _o, _a| p::si_mi2_is_incremental(m).into()),
        ("SI_setmi2Incremental", |m, _o, a| { p::si_set_mi2_incremental(m, a.v("v")); ().into() }),
        ("SI_onoff", |_m, _o, a| p::si_onoff(a.v("v")).into()),
        ("0x08585e20", |m, _o, a| { p::static_init_destruction_0(m, a.v("initialize"), a.v("priority")); ().into() }),
        ("si_dumpUsageInfo", |m, o, _a| { p::si_dump_usage_info(m, o); ().into() }),
        ("0x08580c3c", |m, o, a| { p::display_messages(m, o, a.v("mode"), a.v("text")); ().into() }),
        ("findBlur2DShaderLib", |m, o, _a| p::find_blur2d_shader_lib(m, o).into()),
        ("buildFullPicFileName", |m, o, a| p::build_full_pic_file_name(m, o, a.v("a"), a.v("b"), a.v("c"), a.v("d"), a.v("e")).into()),
        ("_IsScanlineOff", |m, o, a| p::is_scanline_off(m, o, a.v("dict"), a.v("class")).into()),
        ("openMIfile", |m, o, a| p::open_mi_file(m, o, a.v("path"), a.v("rp")).into()),
        ("cleanup_sequence_mental", |m, o, a| { p::cleanup_sequence_mental(m, o, a.v("rp")); ().into() }),
        ("disposeAllmovingCopy", |m, o, a| { p::dispose_all_moving_copy(m, o, a.v("dict")); ().into() }),
        ("SI_cleanupTranslator", |m, o, a| { p::si_cleanup_translator(m, o, a.v("rp")); ().into() }),
        ("printHeader", |m, o, a| { p::print_header(m, o, a.v("rp")); ().into() }),
        ("print_scene", |m, o, a| { p::print_scene(m, o, a.v("rp"), a.v("cam"), a.v("view"), a.v("hide"), a.v("extra")); ().into() }),
        ("SI_spdlUseNamesrvShdDecl", |m, o, _a| p::si_spdl_use_namesrv_shd_decl(m, o).into()),
        ("SI_spdlUseNamesrvShdRef", |m, o, _a| p::si_spdl_use_namesrv_shd_ref(m, o).into()),
        ("SI_spdlUseNamesrvImgRef", |m, o, _a| p::si_spdl_use_namesrv_img_ref(m, o).into()),
        ("SI_isMr19Enable", |m, o, _a| p::si_is_mr19_enable(m, o).into()),
        ("SI_closeLiveConnection", |m, o, a| p::si_close_live_connection(m, o, a.v("rp"), a.v("arg"), a.v("f")).into()),
        ("sequence_mental", |m, o, a| { p::sequence_mental(m, o, a.v("rp")); ().into() }),
        ("mi2_print_view", |m, o, a| { p::mi2_print_view(m, o, a.v("rp"), a.v("a"), a.v("b"), a.v("c")); ().into() }),
        ("si_rotateWorld", |m, o, a| { p::si_rotate_world(m, o, a.v("cam"), a.v("mat1"), a.v("mat2")); ().into() }),
        ("SI_init_MRCamera", |m, _o, a| { p::si_init_mrcamera(m, a.v("dict"), a.v("elem"), a.v("rc"), a.v("out")); ().into() }),
        ("mi2_printHeader", |m, o, a| { files::begin(a); p::mi2_print_header(m, o, a.v("rp")); ().into() }),
        ("init_sequence_mental", |m, o, a| { files::begin(a); p::init_sequence_mental(m, o, a.v("rp"), a.v("flag")).into() }),
        ("SI_initLiveConnection", |m, o, a| p::si_init_live_connection(m, o, a.v("rp"), a.v("a"), a.v("b"), |m, o, rp, f| o.call(m, INIT_SEQUENCE, &[rp, f]).eax).into()),
        ("print_frame", |m, o, a| { p::print_frame(m, o, a.v("rp"), a.v("frame"), a.v("p3"), a.v("p4"), a.v("p5"), a.v("p6"), a.v("p7")); ().into() }),
        ("mi2_print_scene", |m, o, a| { p::mi2_print_scene(m, o, a.v("rp"), a.v("cam"), a.v("view"), a.v("hide"), a.v("extra")); ().into() }),
        ("SI_translateInLiveConnection", |m, o, a| p::si_translate_in_live_connection(m, o, a.v("rp"), a.v("first"), a.v("last"), a.v("step"), a.v("field")).into()),
        ("SI_customPreviewWriteUDF", |m, o, a| p::si_custom_preview_write_udf(m, o, a.v("dict"), a.v("elem"), a.v("file"), a.v("udf"), a.v("name")).into()),
        ("print_option", |m, o, a| { p::print_option(m, o, a.v("rp"), a.v("cam")); ().into() }),
        ("SI_checkHighlightedWindow", |m, o, _a| si_mental::mr_util::si_check_highlighted_window(m, o).into()),
        ("SI_generateMIPerFrameFileName", |m, o, a| si_mental::mr_util::si_generate_mi_per_frame_file_name(m, o, a.v("name"), a.v("frame"), a.v("n"), a.v("k")).into()),
        ("SI_mrpipeSet", |m, o, a| si_mental::mr_util::si_mrpipe_set(m, o, a.v("id"), a.v("term"), a.v("buffer")).into()),
        ("writePipe", |m, o, a| si_mental::mr_util::write_pipe(m, o, a.v("buf"), a.v("n") as i32).into()),
        ("SI_mrpipeFlush", |m, o, _a| si_mental::mr_util::si_mrpipe_flush(m, o).into()),
        ("SI_mrpipeFPUTC", |m, o, a| { si_mental::mr_util::si_mrpipe_fputc(m, o, a.v("c")); ().into() }),
        ("SI_mrpipeFPRINTF", |m, o, a| { si_mental::mr_util::si_mrpipe_fprintf(m, o, a.v("fmt"), &[a.v("a1"), a.v("a2"), a.v("a3")]); ().into() }),
        ("SI_mentalPrintDefaultDeclare", |m, o, a| { files::begin(a); si_mental::mi_defparms::si_mental_print_default_declare(m, o, a.v("idx"), file_arg(a)); ().into() }),
        ("mi2_mentalPrintDefaultDeclare", |m, o, a| { files::begin(a); si_mental::mi_defparms::mi2_mental_print_default_declare(m, o, a.v("idx"), file_arg(a)); ().into() }),
        ("buildShaderName", |m, o, a| { si_mental::mi_tools::build_shader_name(m, o, a.v("dst"), a.v("src"), a.v("idx")); ().into() }),
        ("buildShaderNameWithString", |m, o, a| { si_mental::mi_tools::build_shader_name_with_string(m, o, a.v("dst"), a.v("src"), a.v("s")); ().into() }),
        ("si_checkForTxtGlb", |m, o, a| si_mental::mi_tools::si_check_for_txt_glb(m, o, a.v("dict"), a.v("elem")).into()),
        ("si_getMatNameExtension", |m, o, a| si_mental::mi_tools::si_get_mat_name_extension(m, o, a.v("dict"), a.v("elem")).into()),
        ("updateTxtSupport", |m, o, a| { si_mental::mi_tools::update_txt_support(m, o, a.v("mdl"), a.v("txt")); ().into() }),
        ("findbound", |m, _o, a| { si_mental::mi_tools::findbound(m, a.v("mdl")); ().into() }),
        ("elemTxtGlbCompare", |m, o, a| si_mental::mi_tools::elem_txt_glb_compare(m, o, a.v("dict"), a.v("e1"), a.v("e2")).into()),
        ("textureVert", |m, o, a| { si_mental::mi_tools::texture_vert(m, o, a.v("v"), a.v("out"), a.v("lm")); ().into() }),
        ("SI_init_MRSoftLight", |m, o, a| { si_mental::mi2_light::si_init_mr_soft_light(m, o, a.v("a0"), a.v("elem"), a.v("out")); ().into() }),
        ("mi2_print_light_defs", |m, o, a| si_mental::mi2_light::mi2_print_light_defs(m, o, a.v("dict"), a.v("mat"), a.v("a3"), a.v("cnt"), a.v("a4")).into()),
        ("mi2_printDefaultLight", |m, o, a| si_mental::mi2_light::mi2_print_default_light(m, o, a.v("mat")).into()),
        ("mi2_print_soft_light", |m, o, a| { si_mental::mi2_light::mi2_print_soft_light(m, o, a.v("ld")); ().into() }),
        ("mi2_print_light", |m, o, a| si_mental::mi2_light::mi2_print_light(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.v("hide"), a.v("area2")).into()),
        ("printTransformMat", |m, o, a| { si_mental::mi2_tools::print_transform_mat(m, o, a.v("mat")); ().into() }),
        ("printInstance", |m, o, a| { si_mental::mi2_tools::print_instance(m, o, a.v("name"), a.v("hide"), a.v("mat")); ().into() }),
        ("printDefaultInstance", |m, o, a| { si_mental::mi2_tools::print_default_instance(m, o, a.v("name")); ().into() }),
        ("print_group", |m, o, a| si_mental::mi2_tools::print_group(m, o, a.v("name"), a.v("names"), a.v("cnt"), a.v("sh")).into()),
        ("addCoreOn", |m, o, a| { si_mental::mr_cmdline::add_core_on(m, o, a.v("list"), a.v("count")); ().into() }),
        ("si_mrfileBuildCmdline", |m, o, a| { si_mental::mr_cmdline::si_mrfile_build_cmdline(m, o, a.v("out"), a.v("pipe"), a.v("val"), a.v("list"), a.v("count")); ().into() }),
        ("modelHasShaderReference", |m, o, a| si_mental::mi2_model::model_has_shader_reference(m, o, a.v("dict"), a.v("elem")).into()),
        ("addToRootList", |m, o, a| { si_mental::mi2_model::add_to_root_list(m, o, a.v("list"), a.v("count"), a.v("name"), a.v("shadow")); ().into() }),
        ("SI_mi2GetModelName", |m, o, a| si_mental::mi2_model::si_mi2_get_model_name(m, o, a.v("dict"), a.v("elem"), a.v("buf"), a.v("size")).into()),
        ("print_empty_geometry", |m, o, a| si_mental::mi2_model::print_empty_geometry(m, o, a.v("dict"), a.v("elem")).into()),
        ("si_mi2ModelForwardDefs", |m, o, a| { si_mental::mi2_model::si_mi2_model_forward_defs(m, o, a.v("dict")); ().into() }),
        ("mi2_print_all_models", |m, o, a| si_mental::mi2_model::mi2_print_all_models(m, o, a.v("dict"), a.v("p"), a.v("q"), a.v("f1"), a.v("f2"), a.v("f3"), a.v("a"), a.v("b"), a.v("c"), a.v("cnt")).into()),
        ("mi2_print_model_tree", |m, o, a| si_mental::mi2_model::mi2_print_model_tree(m, o, a.v("dict"), a.v("elem"), a.v("p"), a.v("q"), a.v("a"), a.v("b"), a.v("c"), a.v("out16"), a.v("arrp"), a.v("idxp")).into()),
        ("mi2_print_model", |m, o, a| si_mental::mi2_model::mi2_print_model(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.v("q"), a.v("name"), a.v("b"), a.v("c"), a.v("out16")).into()),
        ("print_empty_light", |m, o, a| { si_mental::mi2_model::print_empty_light(m, o, a.v("name")); ().into() }),
        ("print_empty_mesh", |m, o, a| { si_mental::mi2_model::print_empty_mesh(m, o, a.v("name")); ().into() }),
        ("print_empty_patch", |m, o, a| { si_mental::mi2_model::print_empty_patch(m, o, a.v("name")); ().into() }),
        ("print_light", |m, o, a| { si_mental::mi_light::print_light(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.v("hide"), a.v("area2")); ().into() }),
        ("si_printDefaultLight", |m, o, a| { si_mental::mi_light::si_print_default_light(m, o, a.v("mat")); ().into() }),
        ("print_model_tree", |m, o, a| { si_mental::mi_model::print_model_tree(m, o, a.v("dict"), a.v("elem"), a.v("p"), a.v("q"), a.v("s1"), a.v("s2"), a.v("s3")); ().into() }),
        ("print_all_models", |m, o, a| { si_mental::mi_model::print_all_models(m, o, a.v("dict"), a.v("p"), a.v("q"), a.v("f1"), a.v("f2"), a.v("f3"), a.v("a"), a.v("b"), a.v("c")); ().into() }),
        ("print_model", |m, o, a| { si_mental::mi_model::print_model(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.v("q"), a.v("name"), a.v("s1"), a.v("s2")); ().into() }),
        ("print_light_defs", |m, o, a| { si_mental::mi_light::print_light_defs(m, o, a.v("dict"), a.v("mat"), a.v("a3"), a.v("a4")); ().into() }),
        ("dumpTxtSupport", |m, o, a| { si_mental::mi_tools::dump_txt_support(m, o, a.v("mdl"), a.v("txt"), a.v("caller")); ().into() }),
        ("sfatal", |m, o, a| { si_mental::mi_error::sfatal(m, o, a.v("fmt"), &[a.v("a1"), a.v("a2"), a.v("a3")]); ().into() }),
        ("swarning", |m, o, a| { si_mental::mi_error::swarning(m, o, a.v("fmt"), &[a.v("a1"), a.v("a2"), a.v("a3")]); ().into() }),
        ("sdebug", |m, o, a| { si_mental::mi_error::sdebug(m, o, a.v("fmt"), &[a.v("a1"), a.v("a2"), a.v("a3")]); ().into() }),
        ("print_camera", |m, o, a| { p::print_camera(m, o, a.v("rp"), a.v("cam"), a.v("mat"), a.v("p4")); ().into() }),
        ("print_view", |m, o, a| { p::print_view(m, o, a.v("rp"), a.v("cam"), a.v("mat"), a.v("p4")); ().into() }),
        ("preprocess_texture", |m, _o, a| { si_mental::mi_texture::preprocess_texture(m, a.v("tex"), a.v("wu"), a.v("wv"), a.v("pu"), a.v("pv")); ().into() }),
        ("generateTextureName", |m, o, a| { si_mental::mi_texture::generate_texture_name(m, o, a.v("tex")).into() }),
        ("destroy_2D_texture_list", |m, o, _a| { si_mental::mi_texture::destroy_2d_texture_list(m, o); ().into() }),
        ("cleanup_2D_texture_list", |m, o, _a| { si_mental::mi_texture::cleanup_2d_texture_list(m, o); ().into() }),
        ("dump_2D_texture_list", |m, o, _a| { si_mental::mi_texture::dump_2d_texture_list(m, o); ().into() }),
        ("setFileName__FP11__SI_LclMapis", |m, o, a| { files::begin(a); si_mental::mi_texture::set_file_name(m, o, a.v("tex"), a.v("frame"), a.v("mode")); ().into() }),
        ("0x08594474", |m, o, a| { si_mental::mi_texture::static_init_destruction_0(m, o, a.v("init"), a.v("prio")); ().into() }),
        ("0x085944a0", |m, o, _a| { si_mental::mi_texture::global_constructors_keyed_to_si_init_mrsoftcolor(m, o); ().into() }),
        ("find_program", |m, o, a| { si_mental::mi_texture::find_program(m, o, a.v("out"), a.v("name")); ().into() }),
        ("SI_init_MRSoftColor3D", |m, o, a| { si_mental::mi_texture::si_init_mr_soft_color_3d(m, o, a.v("first"), a.v("el"), a.v("out")); ().into() }),
        ("add_2D_texture", |m, o, a| { files::begin(a); si_mental::mi_texture::add_2d_texture(m, o, a.v("dict"), a.v("el"), a.v("reuse"), a.v("mode")); ().into() }),
        ("print_model_2D_defs", |m, o, a| { si_mental::mi_texture::print_model_2d_defs(m, o, a.v("dict"), a.v("elem"), a.v("reuse"), a.v("mode")); ().into() }),
        ("add2DMaps", |m, o, a| { si_mental::mi_texture::add_2d_maps(m, o, a.v("dict"), a.v("all"), a.v("reuse"), a.v("mode")); ().into() }),
        ("print_3D_map", |m, o, a| { si_mental::mi_texture::print_3d_map(m, o, a.v("dict"), a.v("el"), a.v("name")); ().into() }),
        ("print_model_3D_maps", |m, o, a| { si_mental::mi_texture::print_model_3d_maps(m, o, a.v("dict"), a.v("el"), a.v("name")); ().into() }),
        ("print_2D_map", |m, o, a| { si_mental::mi_texture::print_2d_map(m, o, a.v("dict"), a.v("el"), a.v("name"), a.v("wu"), a.v("wv")); ().into() }),
        ("print_model_2D_maps", |m, o, a| { si_mental::mi_texture::print_model_2d_maps(m, o, a.v("dict"), a.v("el"), a.v("prefix"), a.v("flag")); ().into() }),
        ("precompute_mi_texture", |m, o, a| { files::begin(a); pic_begin(a); si_mental::mi_texture::precompute_mi_texture(m, o, a.v("name"), a.v("out"), a.v("pw"), a.v("ph"), a.v("fatal")).into() }),
        ("print_texture_names", |m, o, a| { si_mental::mi_texture::print_texture_names(m, o, a.v("dict"), a.v("elem")); ().into() }),
        ("print_2D_texture_defs", |m, o, a| { si_mental::mi_texture::print_2d_texture_defs(m, o, a.v("dict"), a.v("all")); ().into() }),
        ("print_2D_texture_defs_preview", |m, o, a| { si_mental::mi_texture::print_2d_texture_defs_preview(m, o, a.v("dict"), a.v("elem"), a.v("prev")); ().into() }),
        ("print_material", |m, o, a| { si_mental::mi_material::print_material(m, o, a.v("dict"), a.v("elem"), a.v("mat"), a.v("p4"), a.v("p5"), a.v("p6"), a.v("p7")); ().into() }),
        ("SI_init_MRSoftColor", |m, o, a| { si_mental::mi_texture::si_init_mr_soft_color(m, o, a.v("dict"), a.v("el"), a.v("out")); ().into() }),
        ("print_2D_texture_map", |m, o, a| { si_mental::mi_texture::print_2d_texture_map(m, o, a.v("dict"), a.v("all"), a.v("mode"), a.v("start"), a.v("end"), a.v("step"), a.v("rp")); ().into() }),
        ("buildDefaultPicName__FP11__SI_LclMapis", |m, o, a| { si_mental::mi_texture::build_default_pic_name(m, o, a.v("tex"), a.v("frame"), a.v("mode")); ().into() }),
        ("processMsgBuffer", |m, _o, a| si_mental::mr_messages::process_msg_buffer(m, a.v("count"), a.v("buf"), a.v("dst"), a.v("dstsz") as i32).into()),
        ("processMessages", |m, o, a| { si_mental::mr_messages::process_messages(m, o, a.v("p")); ().into() }),
        ("pipeReadMessages", |m, o, a| si_mental::mr_messages::pipe_read_messages(m, o, a.v("p")).into()),
        ("si_mrmsgProc_Protected", |m, o, a| { si_mental::mr_messages::si_mrmsgproc_protected(m, o, a.v("p")); ().into() }),
        ("si_mrmsgProc", |m, o, a| { si_mental::mr_messages::si_mrmsgproc(m, o, a.v("p")); ().into() }),
        ("0x0842be70", |m, o, a| { si_mental::mr_messages::handler(m, o, a.v("sig")); ().into() }),
        ("si_mrmsgProcCleanup", |m, _o, a| { si_mental::mr_messages::si_mrmsgproc_cleanup(m, a.v("p")); ().into() }),
        ("swapLongArray", |m, _o, a| { si_mental::mr_display::swap_long_array(m, a.v("p"), a.v("n") as i32); ().into() }),
        ("updateFrame", |m, _o, a| { si_mental::mr_display::update_frame(m, a.v("count"), a.v("state"), a.v("step") as i32); ().into() }),
        ("0x0842cbd8", |m, o, a| si_mental::mr_display::pipe_read_tile_header(m, o, a.v("pipe"), a.v("stop"), a.v("kind"), a.v("rect")).into()),
        ("0x0842cc8c", |m, o, a| si_mental::mr_display::pipe_read_tile_data(m, o, a.v("pipe"), a.v("stop"), a.v("rect"), a.v("buf"), a.v("cap")).into()),
        ("tileDisplay", |m, o, a| { si_mental::mr_display::tile_display(m, o, a.v("p"), a.v("d0"), a.v("d1"), a.v("pix")); ().into() }),
        ("si_mrtileProc_Protected", |m, o, a| { si_mental::mr_display::si_mrtileproc_protected(m, o, a.v("p")); ().into() }),
        ("si_mrtileProc", |m, o, a| { si_mental::mr_display::si_mrtileproc(m, o, a.v("p")); ().into() }),
        ("0x0842c9c0", |m, o, a| { si_mental::mr_display::handler(m, o, a.v("sig")); ().into() }),
        ("si_mrtileProcCleanup", |m, _o, a| { si_mental::mr_display::si_mrtileproc_cleanup(m, a.v("p")); ().into() }),
        ("si_mrtranslatorProc_Protected", |m, o, a| { si_mental::mr_translator::si_mrtranslatorproc_protected(m, o, a.v("p")); ().into() }),
        ("si_mrtranslatorProc", |m, o, a| { si_mental::mr_translator::si_mrtranslatorproc(m, o, a.v("p")); ().into() }),
        ("0x0842c27c", |m, o, a| { si_mental::mr_translator::handler(m, o, a.v("sig")); ().into() }),
        ("0x0842c294", |m, o, a| { si_mental::mr_translator::handler_sigpipe(m, o, a.v("sig")); ().into() }),
        ("si_mrtranslatorProcCleanup", |m, o, a| { si_mental::mr_translator::si_mrtranslatorproc_cleanup(m, o, a.v("p")); ().into() }),
        ("writeBuffer", |m, o, a| si_mental::mi2control::write_buffer(m, o, a.v("text")).into()),
        ("forwardSlashes", |m, _o, a| { si_mental::mi2control::forward_slashes(m, a.v("s")); ().into() }),
        ("backSlashes", |m, _o, a| { si_mental::mi2control::back_slashes(m, a.v("s")); ().into() }),
        ("getFullPath", |m, _o, a| { si_mental::mi2control::get_full_path(m, a.v("path"), a.v("out")); ().into() }),
        ("extractPathSeparator", |m, _o, a| si_mental::mi2control::extract_path_separator(m, a.v("path")).into()),
        ("extractSceneAndDbName", |m, _o, a| { si_mental::mi2control::extract_scene_and_db_name(m, a.v("path"), a.v("scene"), a.v("db")); ().into() }),
        ("si_mi2InitLexYacc", |m, _o, _a| si_mental::mi2control::si_mi2_init_lex_yacc(m).into()),
        ("SI_startCmdParser", |m, o, a| si_mental::mi2control::si_start_cmd_parser(m, o, a.v("live"), a.v("a"), a.v("b")).into()),
        ("si_mi2SceneRender", |m, o, a| si_mental::mi2control::si_mi2_scene_render(m, o, a.v("first") as i32, a.v("last") as i32, a.v("step") as i32, a.v("field") as i32).into()),
        ("si_mi2ExitTranslator", |_m, _o, _a| si_mental::mi2control::si_mi2_exit_translator().into()),
        ("si_mi2CloseScene", |m, o, _a| si_mental::mi2control::si_mi2_close_scene(m, o).into()),
        ("si_mi2Disconnect", |m, o, _a| si_mental::mi2control::si_mi2_disconnect(m, o).into()),
        ("si_mi2QueryRange", |m, o, _a| si_mental::mi2control::si_mi2_query_range(m, o).into()),
        ("si_mi2QueryFrame", |m, o, _a| si_mental::mi2control::si_mi2_query_frame(m, o).into()),
        ("si_mi2QueryTranslator", |m, o, _a| si_mental::mi2control::si_mi2_query_translator(m, o).into()),
        ("si_mi2QueryName", |m, o, _a| si_mental::mi2control::si_mi2_query_name(m, o).into()),
        ("si_mi2QueryOutput", |m, o, _a| si_mental::mi2control::si_mi2_query_output(m, o).into()),
        ("si_mi2QueryCaps", |m, o, _a| si_mental::mi2control::si_mi2_query_caps(m, o).into()),
        ("si_mi2OpenScene", |m, o, a| si_mental::mi2control::si_mi2_open_scene(m, o, a.v("path")).into()),
        ("databaseGetByPath", |m, o, a| si_mental::mi2control::database_get_by_path(m, o, a.v("path"), a.v("out")).into()),
        ("si_mi2InitTranslator", |m, o, a| si_mental::mi2control::si_mi2_init_translator(m, o, a.v("version")).into()),
        ("initRendering", |m, o, _a| si_mental::mi2control::init_rendering(m, o).into()),
        ("si_mi2ConnectToFile", |m, o, a| si_mental::mi2control::si_mi2_connect_to_file(m, o, a.v("name")).into()),
        ("SI_initMentalPipeParms", |m, o, a| { si_mental::mr_main::si_init_mental_pipe_parms(m, o, a.v("rp")); ().into() }),
        ("0x0842b734", |m, o, a| { si_mental::mr_main::handler(m, o, a.v("sig")); ().into() }),
        ("0x0842b75c", |m, o, a| { si_mental::mr_main::display_messages(m, o, a.v("text")); ().into() }),
        ("detectMessages", |m, o, a| { si_mental::mr_main::detect_messages(m, o, a.v("text")); ().into() }),
        ("checkUserQuit", |m, o, a| si_mental::mr_main::check_user_quit(m, o, a.v("mutex")).into()),
        ("checkResources", |m, o, _a| si_mental::mr_main::check_resources(m, o).into()),
        ("computeWindowSizePos", |m, _o, a| { si_mental::mr_main::compute_window_size_pos(m, a.v("rc"), a.v("thr")); ().into() }),
        ("WaitAfterDone", |m, o, a| { si_mental::mr_main::wait_after_done(m, o, a.v("titled")); ().into() }),
        ("0x08585e40", |m, _o, _a| { p::global_constructors_keyed_to_progname(m); ().into() }),
    ])
    // SI_frameToSec is answered 2.5 in the register and not recorded (genm.py: `!st0@2.5`)
    .stand_in("SI_frameToSec", |_m, _a| si_mem::Ret::st0([0, 0, 0, 0, 0, 0, 0, 0xa0, 0x00, 0x40]))
    .stand_in("getenv", getenv)
    .route(route)
    .effect(effect)
    .normalise_stack_args()
    .given_lib_column()
    .run()
}

/// `init_sequence_mental (0x08580f48)` as a recorded stand-in (its own check is `init_sequence_mental`).
const INIT_SEQUENCE: si_mem::Routine = si_mem::Routine::new("init_sequence_mental", 0x0858_0f48);

/// getenv as genm.py's `genv` answers it: two bits of the knob word at 0x08829ff0, selected by `(name >> 2) & 3`:
/// 0 null, 1 "", 2 "x", 3 "a -b".
fn getenv(m: &mut si_mem::Mem, a: &[u32]) -> si_mem::Ret {
    let knob = m.u32(0x0882_9ff0);
    let v = (knob >> (2 * ((a[0] >> 2) & 3))) & 3;
    if v == 0 {
        return si_mem::Ret::eax(0);
    }
    let p = m.malloc(16);
    m.set_bytes(p, [&b""[..], b"", b"x\0", b"a -b\0"][v as usize]);
    if v < 2 {
        m.set_u8(p, 0);
    }
    si_mem::Ret::eax(p)
}

/// The ported callees the original runs for real in the emulator, and the made strings of the check's stand-ins.
/// The `FILE *` argument of the case as the port gets it: `file_route` shifts every stream by 0x100, so the case's stream 0 is passed 0x100 below.
fn file_arg(a: &si_checkkit::Args) -> u32 {
    if a.case().try_input("f").is_some() { files::HANDLE0 - 0x100 } else { 0 }
}

fn route(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> Option<si_mem::Ret> {
    let made = |m: &mut si_mem::Mem, v: u32| {
        if v == 0 {
            return si_mem::Ret::eax(0);
        }
        let p = m.malloc(16);
        m.set_bytes(p, format!("n{v:x}\0").as_bytes());
        si_mem::Ret::eax(p)
    };
    match r.addr {
        // SI_buildPicFileName(a, b, c, d, 0): "n<d>" when d is not 0
        0x082c_b7a8 => Some(made(m, a[3])),
        // SI_getRenderImgSuffix(e): "n<e>" when e is not 0
        0x082a_59ac => Some(made(m, a[0])),
        // buildFullPicFileName / SI_generateMIPerFrameFileName (genm.py `genname`): null when (word 2 & 7) == 3, else "g<word 2>"
        0x0858_14a8 | 0x0842_c40c => {
            if a[2] & 7 == 3 {
                return Some(si_mem::Ret::eax(0));
            }
            let p = m.malloc(16);
            m.set_bytes(p, format!("g{:x}\0", a[2]).as_bytes());
            Some(si_mem::Ret::eax(p))
        }
        // mi2_print_light_defs (`lightlist`): n = frame & 3 names "l<i>" in a made array, count = n
        0x0859_57d4 => {
            let n = m.u32(0x0882_9f38) & 3;
            let arr = m.malloc(4 * n + 4);
            for i in 0..n {
                let p = m.malloc(16);
                m.set_bytes(p, format!("l{i}\0").as_bytes());
                m.set_u32(arr + 4 * i, p);
            }
            m.set_u32(a[3], n);
            Some(si_mem::Ret::eax(arr))
        }
        // mi2_print_all_models (`modlist`): n = (frame >> 2) & 3 names "m<i>" (null for odd i)
        0x0859_6710 => {
            let n = (m.u32(0x0882_9f38) >> 2) & 3;
            let arr = m.malloc(4 * n + 4);
            for i in 0..n {
                let mut p = 0;
                if i % 2 == 0 {
                    p = m.malloc(16);
                    m.set_bytes(p, format!("m{i}\0").as_bytes());
                }
                m.set_u32(arr + 4 * i, p);
            }
            m.set_u32(a[9], n);
            Some(si_mem::Ret::eax(arr))
        }
        // SI_memtrackAllocate(params, n) (`memtrack`): a block of the harness's heap, null for n = 0
        0x0859_9610 => Some(si_mem::Ret::eax(if a[1] != 0 { m.malloc(a[1]) } else { 0 })),
        // SI_memtrackStrdup(params, s) (`mstrdup`): a made copy of the string
        0x0859_97e0 => {
            let t = m.cstr(a[1]);
            let p = m.malloc(t.len() as u32 + 1);
            m.put_cstr(p, &t);
            Some(si_mem::Ret::eax(p))
        }
        // SI_memtrackRealloc(params, p, n) (`memrealloc`): the harness's realloc, at most 24 KB of the block made
        0x0859_9770 => {
            let n = a[2].min(24576);
            // a block that is not the harness's (tex9's static texture list): a new block holding its first 0x48 bytes (genm6.py `memrealloc2`)
            if a[1] != 0 && m.block_size(a[1]).is_none() {
                let q = m.malloc(n);
                let head = m.bytes(a[1], n.min(0x48)).to_vec();
                m.set_bytes(q, &head);
                return Some(si_mem::Ret::eax(q));
            }
            Some(si_mem::Ret::eax(m.realloc(a[1], n)))
        }
        // mi2_print_model(.., out16) (`pmodel`): *out16 = bit 1 of the frame word; a made block of 16 bytes when bit 0 is set
        0x0859_709c => {
            let fr = m.u32(0x0882_9f38);
            m.set_u16(a[7], ((fr >> 1) & 1) as u16);
            Some(si_mem::Ret::eax(if fr & 1 != 0 { m.malloc(16) } else { 0 }))
        }
        // print_group (`pgroup`): a made block of 16 bytes when bit 0 of the frame is set
        0x0859_8178 | 0x0859_5938 | 0x0859_64a8 | 0x0859_7a40 => {
            let p = if m.u32(0x0882_9f38) & 1 != 0 { m.malloc(16) } else { 0 };
            if p != 0 && m.u32(0x0879_5fc8) == 0x600d_cafe {
                m.put_cstr(p, b"pg");
            }
            Some(si_mem::Ret::eax(p))
        }
        // SI_strListToString(list, count, sep) (`ltostr`): a made block holding "L<count>" when count > 0, else null
        0x083c_f77c => {
            if a[1] == 0 || a[1] >= 0x8000_0000 {
                return Some(si_mem::Ret::eax(0));
            }
            let p = m.malloc(16);
            m.put_cstr(p, format!("L{}", a[1]).as_bytes());
            Some(si_mem::Ret::eax(p))
        }
        // SI_calloc(n, size) (`calloc`): a zeroed block, null when the knob word 0x08795f08 is 1
        0x080d_0f50 => {
            if m.u32(0x0879_5f08) == 1 {
                return Some(si_mem::Ret::eax(0));
            }
            let n = a[0].wrapping_mul(a[1]);
            let q = m.malloc(n);
            m.set_bytes(q, &vec![0u8; n as usize]);
            Some(si_mem::Ret::eax(q))
        }
        // SI_getRenderCamera (`camptr`): the made camera at 0x08795600 when the word at 0x08795608 is not 0
        0x082c_b590 => Some(si_mem::Ret::eax(if m.u32(0x0879_5608) != 0 { 0x0879_5600 } else { 0 })),
        _ => file_route(m, r, a),
    }
}

thread_local! {
    /// precompute_mi_texture: the case's named files, the file the last fopen opened and the read position in it
    static PIC: std::cell::RefCell<(Vec<(String, Vec<u8>)>, Vec<u8>, usize)> = std::cell::RefCell::new((Vec::new(), Vec::new(), 0));
}

/// The case's `file:<path>` inputs for the picture reader and `access` (the file model of `files` keeps them private).
fn pic_begin(a: &si_checkkit::Args) {
    PIC.with(|p| {
        let mut p = p.borrow_mut();
        p.0.clear();
        p.1.clear();
        p.2 = 0;
        for f in &a.case().inputs {
            if let Some(path) = f.name.strip_prefix("file:") {
                p.0.push((path.to_string(), f.words().iter().map(|w| w.value().unwrap_or(0) as u8).collect()));
            }
        }
    });
}

fn file_route(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32]) -> Option<si_mem::Ret> {
    match r.name {
        // SI_byteswap_fread (0x083f9ba0): fread from the file the last fopen opened, then every item of 2, 4 or 8 bytes swapped
        "SI_byteswap_fread" => {
            let (buf, size, count) = (a[0], a[1], a[2]);
            let n = PIC.with(|p| {
                let mut p = p.borrow_mut();
                let want = (size as usize) * (count as usize);
                let left = p.1.len().saturating_sub(p.2);
                let take = want.min(left);
                let mut got = p.1[p.2..p.2 + take].to_vec();
                p.2 += take;
                let items = if size == 0 { 0 } else { take / size as usize };
                if matches!(size, 2 | 4 | 8) {
                    for it in 0..items {
                        got[it * size as usize..(it + 1) * size as usize].reverse();
                    }
                }
                if !got.is_empty() {
                    m.set_bytes(buf, &got);
                }
                items as u32
            });
            Some(si_mem::Ret::eax(n))
        }
        // no `f` argument here: the first fopen of the original gets the first handle, the model's stream 0 is the case's `f`
        "fopen" => {
            let path = String::from_utf8_lossy(&m.cstr(a[0])).into_owned();
            let v = files::route(m, r, a);
            PIC.with(|p| {
                let mut p = p.borrow_mut();
                p.2 = 0;
                p.1 = p.0.iter().find(|f| f.0 == path).map(|f| f.1.clone()).unwrap_or_default();
            });
            v.map(|v| if v.eax != 0 { si_mem::Ret::eax(v.eax - 0x100) } else { v })
        }
        // the case's own FILE (`f=FILE(r)`, handle 0x6f000300) is stream 0 of the model; fopen'd streams are shifted by 0x100
        "rewind" => files::route(m, si_mem::Routine::new("fseek", 0), &[a[0] + 0x100, 0, 0]),
        "fgets" => files::route(m, r, &[a[0], a[1], a[2] + 0x100]),
        "fclose" => files::route(m, r, &[a[0] + 0x100]),
        _ => None,
    }
}

/// What the check's stand-ins do to the image (gen.py's `CUSTOM`, genm.py's).
fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], answer: u32) -> Result<(), String> {
    if r.addr == 0x0842_fd90 && m.u32(0x0879_5f2c) == 0x600d_d15c {
        // SI_pipeioRead(id, ptr, n, &got) as the tile pipe's script of messages (`pipet`, genm6.py): see `disp_msg`
        return disp_pipet(m, a, answer);
    }
    if r.addr == 0x0812_1744 && (0x64..=0x66).contains(&answer) {
        // SI_dbDirSearchDatabaseFromName: *out = a made record, kind 2 (1 for 0x66), +0xc zero for 0x65
        let rec = m.malloc(0x30);
        m.set_u32(rec + 0x24, if answer == 0x66 { 1 } else { 2 });
        m.set_u32(rec + 0xc, if answer == 0x65 { 0 } else { 0x777 });
        m.set_u32(a[2], rec);
    }
    if r.addr == 0x0842_f930 && answer == 2 {
        // SI_mrfileGetExecFullpath(name, &out): out = a made block when it answers 2
        let blk = m.malloc(16);
        m.set_u32(a[1], blk);
    }
    if r.addr == 0x0859_86f4 && answer == 0 {
        // rotate_matrix(out, rot) as a recorded stand-in (`rotid`): the identity in out
        for i in 0..16u32 {
            m.set_u32(a[0] + 4 * i, if i % 5 == 0 { 0x3f80_0000 } else { 0 });
        }
    }
    if r.addr == 0x0859_52f4 {
        // buildShaderName(buf, name, index) (`bsname`): writes "bs<index>" into buf
        m.put_cstr(a[0], format!("bs{}", a[2]).as_bytes());
    }
    if r.addr == 0x0858_1530 {
        // print_frame as a recorded stand-in (`pipe30`): a made string in rp+0x65c or null
        let p = if answer != 0 {
            let p = m.malloc(16);
            m.set_bytes(p, b"pf\0");
            p
        } else {
            0
        };
        m.set_u32(a[0] + 0x65c, p);
    }
    if r.addr == 0x0813_8104 && a[6] != 0 {
        // SI_elemdictGetNextRelatedElem (`mnext`): *out = the answer (a fixed material record or null)
        m.set_u32(a[6], answer);
    }
    if r.addr == 0x0813_8094 && a[6] != 0 && answer >= 3 {
        // SI_elemdictGetFirstRelatedElem (`mfirst`): *out = the fixed material record
        m.set_u32(a[6], answer);
    }
    if r.addr == 0x0813_8094 && a[6] != 0 && answer < 3 {
        // SI_elemdictGetFirstRelatedElem (`firstrel`): 0 out = null, 1 out = owner, 2 out = a made zeroed block
        match answer {
            0 => m.set_u32(a[6], 0),
            1 => m.set_u32(a[6], a[1]),
            _ => {
                let p = m.malloc(0x60);
                m.set_bytes(p, &[0u8; 0x60]);
                m.set_u32(a[6], p);
            }
        }
    }
    if r.addr == 0x0859_390c && answer & 1 != 0 {
        // precompute_mi_texture(name, out, &w, &h, 0) (`pmt`): w = bits 8..19, h = bits 20..31 of the answer
        m.set_u32(a[2], (answer >> 8) & 0xfff);
        m.set_u32(a[3], answer >> 20);
    }
    if (r.addr == 0x0813_8094 || r.addr == 0x0813_8104) && a[6] != 0 {
        // Get(First|Next)RelatedElem (`t2first`, `t2next`): a relation record 0x08797000.. has its texture element 0x100 above it,
        // a material record 0x0879a000 + 0x1000 k its element 0x08795e00 + 0x40 k
        if (0x0879_7000..0x0879_7100).contains(&answer) {
            m.set_u32(a[6], answer + 0x100);
        } else if (0x0879_a000..0x0879_c000).contains(&answer) {
            m.set_u32(a[6], 0x0879_5e00 + ((answer - 0x0879_a000) >> 12) * 0x40);
        }
    }
    if (r.addr == 0x0854_5398 || r.addr == 0x0854_53e4) && answer != 0 {
        // SI_udfGetShaderName ("sh<answer>") / SI_udfGetShaderFuncDeclName ("fd<answer>") (`udfname`, `udffd`): the name in buf
        let pre = if r.addr == 0x0854_5398 { "sh" } else { "fd" };
        m.put_cstr(a[1], format!("{}{}", pre, answer).as_bytes());
    }
    if r.addr == 0x0859_5380 {
        // buildShaderNameWithString(out, name, str) (`bsnames`): writes "bw_" + str into out
        let mut t = b"bw_".to_vec();
        t.extend(m.cstr(a[2]));
        m.put_cstr(a[0], &t);
    }
    if r.addr == 0x0859_68c0 && a[9] != 0 {
        // mi2_print_model_tree(.., &slot, &arr, &idx) (`treefx`): *idx += 1
        m.set_u32(a[9], m.u32(a[9]).wrapping_add(1));
    }
    if r.addr == 0x0859_7994 {
        // SI_mi2GetModelName(dict, elem, buf, size) (`modname`): writes "MOD_x" into buf
        m.put_cstr(a[2], b"MOD_x");
    }
    if r.addr == 0x0859_883c {
        // invert_matrix(out, in): out[i] = in[i] * 0.5 + 0.125 * (i + 1)
        for k in 0..16u32 {
            let v = f32::from_bits(m.u32(a[1] + 4 * k)) as f64;
            m.set_f32(a[0] + 4 * k, (v * 0.5 + 0.125 * (k + 1) as f64) as f32);
        }
    }
    if r.addr == 0x0859_8430 {
        // vector_X_matrix(&out[3], &in[3], mat) (`vecmat`): out = in * 1.5 + (0.25, 0.5, 0.75)
        for k in 0..3u32 {
            let v = f32::from_bits(m.u32(a[1] + 4 * k)) as f64;
            m.set_f32(a[0] + 4 * k, (v * 1.5 + 0.25 * (k + 1) as f64) as f32);
        }
    }
    if r.addr == 0x0842_fd68 {
        // SI_pipeioWrite(id, ptr, n, &written) (`pipew`): *written = n for a multiple of 3, else half of n rounded up
        let n = a[2];
        m.set_u32(a[3], if n % 3 == 0 { n } else { n.wrapping_add(1) / 2 });
    }
    if r.addr == 0x083c_ca94 && m.u32(0x0879_5f04) == 0x600d_cafe {
        // the message callback (`cbhash`): folds the line into the word at 0x08795f00
        let mut h = m.u32(0x0879_5f00);
        let t = m.cstr(a[0]);
        for b in &t {
            h = h.wrapping_mul(33).wrapping_add(*b as u32);
        }
        m.set_u32(0x0879_5f00, h.wrapping_mul(33).wrapping_add(t.len() as u32));
    }
    if r.addr == 0x0860_e188 {
        // qtest() (`qseq`): counts the word 0x08795fd0 down
        let q = m.u32(0x0879_5fd0);
        if q > 0 {
            m.set_u32(0x0879_5fd0, q - 1);
        }
    }
    if r.addr == 0x080c_4238 && answer == 0 {
        // __xstat(3, path, &st) (`xst`): st is zeros with dev = the byte sum of the path % 3 and ino = sum % 5 + 1
        let sm: u32 = m.cstr(a[1]).iter().map(|&b| b as u32).sum();
        for i in 0..0x58 {
            m.set_u8(a[2] + i, 0);
        }
        m.set_u32(a[2], sm % 3);
        m.set_u32(a[2] + 12, sm % 5 + 1);
    }
    if r.addr == 0x080c_4f08 {
        // getrlimit(7, &rl) (`rlim`): rl = {0, the word 0x08795fd4}
        m.set_u32(a[1], 0);
        m.set_u32(a[1] + 4, m.u32(0x0879_5fd4));
    }
    if r.addr == 0x0842_fd90 {
        // SI_pipeioRead(id, ptr, n, &got) (`pipeio`): got is 0, n, n / 2 or 3 by n, filled with the pattern
        let n = a[2];
        let got = [0, n, n / 2, n.min(3)][((n >> 1) & 3) as usize];
        let pat = b"ab\ncd\n\0e";
        for i in 0..got {
            m.set_u8(a[1].wrapping_add(i), pat[((i + n) % 8) as usize]);
        }
        m.set_u32(a[3], got);
    }
    Ok(())
}

/// Message `k` of the tile pipe's script (genm6.py `_disp_msg`): five big-endian header words (kind, x0, x1, y0, y1), and w*h*4 pixel bytes for kind 2.
fn disp_msg(k: u32, word: u32) -> Vec<u8> {
    let (t, ww, hh) = (word & 0xff, (word >> 8) & 0xff, (word >> 16) & 0xff);
    let x0 = k.wrapping_mul(3).wrapping_sub(2);
    let y0 = 5u32.wrapping_sub(k);
    let mut b = Vec::new();
    for v in [t, x0, x0.wrapping_add(ww).wrapping_sub(1), y0, y0.wrapping_add(hh).wrapping_sub(1)] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    if t == 2 {
        b.extend((0..ww * hh * 4).map(|i| (i.wrapping_mul(7).wrapping_add(k) & 0xff) as u8));
    }
    b
}

/// The `pipet` stand-in's effects: the call counter, the stop flag of the block at 0x08796100 (+0x1e) at call `stopk`, the next bytes of the script and the
/// message index and offset (words 0x08795f10..0x08795f28); the answer is 1 once the script is used up.
fn disp_pipet(m: &mut si_mem::Mem, a: &[u32], answer: u32) -> Result<(), String> {
    let (n, ptr, gp) = (a[2], a[1], a[3]);
    let ctr = m.u32(0x0879_5f20).wrapping_add(1);
    m.set_u32(0x0879_5f20, ctr);
    let stopk = m.u32(0x0879_5f24);
    if stopk != 0 && ctr == stopk {
        m.set_u16(0x0879_611e, 1);
    }
    let (mut msgi, count, mut off, mode) = (m.u32(0x0879_5f10), m.u32(0x0879_5f14), m.u32(0x0879_5f28), m.u32(0x0879_5f18));
    if msgi >= count {
        return if answer == 1 { Ok(()) } else { Err(format!("pipet: script used up but the recorded answer is {answer}")) };
    }
    let cur = disp_msg(msgi, m.u32(0x0879_6400 + 4 * msgi));
    let avail = cur.len() as u32 - off;
    let want = match mode & 3 { 0 => n, 1 => (n + 1) / 2, 2 => n.min(3), _ => if ctr % 2 == 0 { n } else { 0 } };
    let got = want.min(avail);
    for i in 0..got {
        m.set_u8(ptr.wrapping_add(i), cur[(off + i) as usize]);
    }
    m.set_u32(gp, got);
    off += got;
    if off as usize >= cur.len() {
        msgi += 1;
        off = 0;
    }
    m.set_u32(0x0879_5f10, msgi);
    m.set_u32(0x0879_5f28, off);
    Ok(())
}
