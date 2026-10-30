// SPDX-License-Identifier: MIT
//! `custom-checkkit <directory of vector files>`: the shader-database sources on
//! `si-checkkit` against the emulated original (finding 0041 and 0011).

mod files;

use si_checkkit::Kit;
use si_custom::custom_io::*;
use si_custom::shdb_support::*;
use si_custom::shdb_lex::*;
use si_custom::shdb_yacc::*;
use si_custom::shdb_init::*;
use si_custom::strlist::*;
use si_custom::shdb_main::*;
use si_custom::shdb_translator::*;
use si_custom::custom_shader::*;
use si_custom::custom_gen::*;
use si_custom::custom_init::*;
use si_custom::custom_load::*;
use si_custom::custom_dyn::*;
use si_custom::custom_misc::*;
use si_custom::custom_util::*;
use si_custom::custom_fcv::*;
use si_custom::custom_udf::*;
mod cv;
mod c11;
mod dso;
mod dso2;
mod udf3;
mod shdb3;
mod shdb4;
mod shdb5;
mod shdb7;
mod cus1;
mod last2;

static ALL: std::sync::OnceLock<Vec<(&'static str, si_checkkit::Port)>> = std::sync::OnceLock::new();

/// Every port runs through here: it records whether the case declares the capture buffer of the display messages
/// (`udf3::effect` captures only then; the callback capture buffer cap2 at 0x08788d00 covers it too; the vectors of the older groups declare none and the checker flags a write
/// outside the case's buffers), then calls the port.
fn tramp(m: &mut si_mem::Mem, o: &mut dyn si_mem::Outside, a: &si_checkkit::Args) -> si_checkkit::Ret {
    files::reset();
    let declared = a.case().inputs.iter().any(|f| f.name == "g08788f00_cap" || f.name == "g08788d00_cap2");
    udf3::CAP_DECLARED.with(|c| c.set(declared));
    let name = a.case().name.split('#').next().unwrap_or("");
    let p = ALL.get().unwrap().iter().find(|(n, _)| *n == name).unwrap().1;
    let k = watchdog::start(&a.case().name);
    let r = p(m, &mut Budget { inner: o, left: CALL_BUDGET }, a);
    watchdog::stop(k);
    r
}

/// A port that loops over made-up memory without any outside call (a random count or a cyclic chain; unit shdb6 met
/// it on stale SI_cusUpdateEffect vectors) cannot be stopped from inside: after `CUSTOM_CASE_LIMIT` seconds (default
/// 30) a watchdog thread names the case on stderr and ends the process with status 124, so a run fails loudly
/// instead of hanging.
mod watchdog {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    static CURRENT: Mutex<Option<(u64, Instant, String)>> = Mutex::new(None);

    pub fn spawn() {
        let limit = std::env::var("CUSTOM_CASE_LIMIT").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(30);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(500));
            let over = CURRENT.lock().ok().and_then(|c| c.as_ref().filter(|c| c.1.elapsed().as_secs() >= limit).map(|c| (c.0, c.2.clone())));
            if let Some((k, name)) = over {
                eprintln!("custom-checkkit: case {k} ({name}) ran over {limit} s without returning (the port loops without outside calls); stopping");
                std::process::exit(124);
            }
        });
    }

    pub fn start(name: &str) -> u64 {
        let k = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut c) = CURRENT.lock() {
            *c = Some((k, Instant::now(), name.to_string()));
        }
        k
    }

    pub fn stop(k: u64) {
        if let Ok(mut c) = CURRENT.lock() {
            if c.as_ref().is_some_and(|c| c.0 == k) {
                *c = None;
            }
        }
    }
}

/// Outside calls one case may make. The largest case of the vectors records 446; a port that loops on answers the
/// original never gave (an fscanf that the recorded calls answer 0 for ever: cusAddToMenu under SI_cusScan, unit shdb6)
/// would otherwise hang the checker. Past the budget the case panics, which the kit counts as different.
const CALL_BUDGET: u32 = 200_000;

struct Budget<'a> {
    inner: &'a mut dyn si_mem::Outside,
    left: u32,
}

impl si_mem::Outside for Budget<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, routine: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        if self.left == 0 {
            panic!("more than {CALL_BUDGET} outside calls (the port loops; last {})", routine.name);
        }
        self.left -= 1;
        self.inner.call(m, routine, args)
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        self.inner.print(stream, text)
    }
}

fn main() {
    // the shader-database data comes from the user's install (si-rt, unit rtdata1): say so before any case
    si_rt::check_or_exit("custom-checkkit");
    watchdog::spawn();
    let all = shdb3::with(&dso2::with(&dso::with(&c11::with(&udf3::with(&cv::with(&[
        ("addNodeFloat", |m, _o, a| add_node_float(m, a.v("kind") as i32, a.v("parent"), a.v("value")).into()),
        ("addNodeVector3", |m, _o, a| add_node_vector3(m, a.v("kind") as i32, a.v("parent"), a.v("x"), a.v("y"), a.v("z")).into()),
        ("addNodeColor3", |m, _o, a| add_node_color3(m, a.v("kind") as i32, a.v("parent"), a.v("a"), a.v("b"), a.v("c"), a.v("d")).into()),
        ("addNodeInteger", |m, _o, a| add_node_integer(m, a.v("kind") as i32, a.v("parent"), a.v("v") as i32).into()),
        ("si_shdbParse", |m, o, a| si_shdb_parse(m, o, a.v("text"), a.v("nerrs"), a.v("errs"), a.v("root")).into()),
        ("SHDB_yylex", |m, o, _a| shdb_yylex(m, o).into()),
        ("si_shdbAllocNode", |m, _o, _a| si_shdb_alloc_node(m).into()),
        ("SI_strListAppend", |m, _o, a| si_str_list_append(m, a.v("text"), a.v("list"), a.v("count")).into()),
        ("SI_shdbUINeedRefresh", |m, o, _a| si_shdb_ui_need_refresh(m, o).into()),
        ("SI_shdbUIActive", |m, _o, _a| si_shdb_ui_active(m).into()),
        ("SI_shdbUIDoPopup", |m, o, _a| { si_shdb_ui_do_popup(m, o); ().into() }),
        ("SI_shdbValidate", |m, o, a| si_shdb_validate(m, o, a.v("p")).into()),
        ("SI_strAppend", |m, _o, a| si_str_append(m, a.v("dest"), a.v("text")).into()),
        ("initPlaneDefaultMaterial", |m, _o, a| { init_plane_default_material(m, a.v("p")); ().into() }),
        ("initPlaneCheckerMaterial", |m, _o, a| { init_plane_checker_material(m, a.v("p")); ().into() }),
        ("si_shdbInitScene", |m, _o, a| { si_shdb_init_scene(m, a.v("p")); ().into() }),
        ("setPreviewSphereOptions", |m, _o, a| { set_preview_sphere_options(m, a.v("sphere"), a.v("mtl"), a.v("node")); ().into() }),
        ("setPreviewPlaneOptions", |m, _o, a| { set_preview_plane_options(m, a.v("p"), a.v("node")); ().into() }),
        ("setPreviewCutplaneOptions", |m, _o, a| { set_preview_cutplane_options(m, a.v("p"), a.v("node")); ().into() }),
        ("setPreviewLightOptions", |m, _o, a| { set_preview_light_options(m, a.v("p"), a.v("node")); ().into() }),
        ("setPreviewWindowOptions", |m, _o, a| { set_preview_window_options(m, a.v("p"), a.v("node")); ().into() }),
        ("setPreviewRenderOptions", |m, _o, a| { set_preview_render_options(m, a.v("p"), a.v("node")); ().into() }),
        ("si_shdbParseAll", |m, o, a| si_shdb_parse_all(m, o, a.v("scene"), a.v("out")).into()),
        ("si_shdbSetOptions", |m, o, a| si_shdb_set_options(m, o, a.v("p"), a.v("shader"), a.v("mtl"), a.v("nerrs"), a.v("errs")).into()),
        ("writeString", |m, o, a| {
            let mut v = Vec::new();
            for i in 0.. { let n = format!("a{i}"); if !a.has(&n) { break; } v.push(a.v(&n)); }
            write_string(m, o, a.v("fd"), a.v("fmt"), &v); ().into()
        }),
        ("generateLightList", |m, _o, a| generate_light_list(m, a.v("light"), a.v("out")).into()),
        ("writeShader", |m, o, a| write_shader(m, o, a.v("task"), a.v("fd"), a.v("shader"), a.v("scene")).into()),
        ("writeView", |m, o, a| { write_view(m, o, a.v("fd"), a.v("view")); ().into() }),
        ("writeDefaultLightDefinition", |m, o, a| { write_default_light_definition(m, o, a.v("fd"), a.v("light")); ().into() }),
        ("writePlaneMaterial", |m, o, a| { write_plane_material(m, o, a.v("fd"), a.v("p")); ().into() }),
        ("writeSceneFull", |m, o, a| write_scene_full(m, o, a.v("task"), a.v("fd"), a.v("shader"), a.v("scene")).into()),
        ("writeSceneIncremental", |m, o, a| write_scene_incremental(m, o, a.v("task"), a.v("fd"), a.v("shader"), a.v("scene")).into()),
        ("si_shdbTranslatorProc_Protected", |m, o, a| { si_shdb_translator_proc_protected(m, o, a.v("task")); ().into() }),
        ("SI_shaderAllocate", |m, _o, _a| si_shader_allocate(m).into()),
        ("linksListFind", |m, _o, a| links_list_find(m, a.v("name"), a.v("name2")).into()),
        ("linksListAdd", |m, _o, a| links_list_add(m, a.v("tok")).into()),
        ("linksListRemove", |m, _o, a| links_list_remove(m, a.v("tok")).into()),
        ("SI_shaderDispose", |m, o, a| si_shader_dispose(m, o, a.v("sh")).into()),
        ("SI_shaderGetName", |m, _o, a| si_shader_get_name(m, a.v("sh"), a.v("buf"), a.v("size") as i32).into()),
        ("SI_shaderUseDefault", |m, _o, a| si_shader_use_default(m, a.v("sh")).into()),
        ("shaderGetFuncName", |m, o, a| shader_get_func_name(m, o, a.v("sh"), a.v("buf"), a.v("size") as i32).into()),
        ("SI_shaderGetFuncName", |m, o, a| si_shader_get_func_name(m, o, a.v("sh"), a.v("buf"), a.v("size") as i32).into()),
        ("SI_shaderGetFuncDeclName", |m, o, a| { si_shader_get_func_decl_name(m, o, a.v("sh"), a.v("buf"), a.v("size") as i32); ().into() }),
        ("SI_shaderSetActive", |m, o, a| si_shader_set_active(m, o, a.v("sh"), a.v("on")).into()),
        ("SI_shaderResetActive", |m, _o, _a| { si_shader_reset_active(m); ().into() }),
        ("SI_shaderIsActive", |m, _o, a| si_shader_is_active(m, a.v("sh")).into()),
        ("SI_shaderGetType", |m, _o, a| si_shader_get_type(m, a.v("sh")).into()),
        ("SI_shaderFind", |m, _o, a| si_shader_find(m, a.v("name"), a.v("name2")).into()),
        ("SI_shaderInitialize", |m, _o, a| si_shader_initialize(m, a.v("sh"), a.v("name"), a.v("name2"), a.v("info")).into()),
        ("SI_shaderCopy", |m, _o, a| si_shader_copy(m, a.v("sh")).into()),
        ("cusAllocateToken", |m, _o, a| cus_allocate_token(m, a.v("kind"), a.v("name"), a.v("name2"), a.v("w10"), a.v("next")).into()),
        ("SI_mrpipeFPRINTF", |m, o, a| { mr_printf(m, o, a.v("fmt"), &[a.v("a0"), a.v("a1")]); ().into() }),
        ("getArrayArgumentIndex", |m, _o, a| get_array_argument_index(m, a.v("name")).into()),
        ("writeShaderDeclareArrayMembers", |m, o, a| { write_shader_declare_array_members(m, o, a.v("list")); ().into() }),
        ("shaderGenerateDeclareDefaults", |m, o, a| shader_generate_declare_defaults(m, o, a.v("sh"), a.v("file")).into()),
        ("shaderGenerateInclude", |_m, _o, _a| shader_generate_include().into()),
        ("shaderTokenInCustomShaderList", |m, _o, a| shader_token_in_custom_shader_list(m, a.v("tok"), a.v("sh")).into()),
        ("SI_shaderGenerateDeclare", |m, o, a| si_shader_generate_declare(m, o, a.v("sh"), a.v("file")).into()),
        ("SI_shaderGenerateDeclares", |m, o, a| si_shader_generate_declares(m, o, a.v("file")).into()),
        ("SI_shaderGenerateIncludes", |m, o, a| si_shader_generate_includes(m, o, a.v("file")).into()),
        ("SI_shaderGenerateCodes", |m, o, a| { files::begin(a); si_shader_generate_codes(m, o, a.v("sh")).into() }),
        ("SI_shaderGenerateLinks", |m, o, a| { files::begin(a); si_shader_generate_links(m, o, a.v("sh")).into() }),
        ("shaderLinksUseRegistry", |m, o, _a| shader_links_use_registry(m, o).into()),
        ("shaderCorrectLinkPath", |m, _o, a| { shader_correct_link_path(m, a.v("path")); ().into() }),
        ("SI_shaderGenerateCflags", |m, o, a| si_shader_generate_cflags(m, o, a.v("a"), a.v("b")).into()),
        ("SI_shaderGenerateLDflags", |m, o, a| si_shader_generate_ldflags(m, o, a.v("a"), a.v("b")).into()),
        ("SI_shaderGenerateCcomp", |m, o, a| si_shader_generate_ccomp(m, o, a.v("a"), a.v("b")).into()),
        ("shaderLoad", |m, o, a| { files::begin(a); si_shader_load(m, o, a.v("sh")).into() }),
        ("shaderReadHeader", |m, o, a| { files::begin(a); shader_read_header(m, o, files::handle(), a.v("sh")).into() }),
        ("shaderReadDialog", |m, o, a| { files::begin(a); shader_read_dialog(m, o, files::handle(), a.v("sh")).into() }),
        ("shaderReadPreviewer", |m, o, a| { files::begin(a); shader_read_previewer(m, o, files::handle(), a.v("sh")).into() }),
        ("shaderReadOptions", |m, o, a| { files::begin(a); shader_read_options(m, o, files::handle(), a.v("sh")).into() }),
        ("shaderReadSPDL", |m, o, a| { files::begin(a); shader_read_spdl(m, o, files::handle(), a.v("sh")).into() }),
        ("SI_dynalinkLoad", |m, o, a| si_dynalink_load(m, o, a.v("name"), a.v("handle")).into()),
        ("SI_dynalinkResolveSymbol", |m, o, a| si_dynalink_resolve_symbol(m, o, a.v("handle"), a.v("name"), a.v("address")).into()),
        ("SI_dynalinkUnload", |m, o, a| si_dynalink_unload(m, o, a.v("handle")).into()),
        ("SI_dynalinkResolveMainProgramSymbol", |m, o, a| si_dynalink_resolve_main_program_symbol(m, o, a.v("name"), a.v("address")).into()),
        ("SI_dynamicSymbolCreate", |m, _o, a| si_dynamic_symbol_create(m, a.v("name"), a.v("type")).into()),
        ("SI_dynamicSymbolDelete", |m, _o, a| { si_dynamic_symbol_delete(m, a.v("pp")); ().into() }),
        ("SI_dynamicSymbolGetName", |m, _o, a| si_dynamic_symbol_get_name(m, a.v("s")).into()),
        ("SI_dynamicSymbolResolve", |m, o, a| si_dynamic_symbol_resolve(m, o, a.v("handle"), a.v("s")).into()),
        ("SI_dynamicSymbolSetAddress", |m, _o, a| { si_dynamic_symbol_set_address(m, a.v("address"), a.v("s")); ().into() }),
        ("SI_dynamicSymbolDump", |m, o, a| { si_dynamic_symbol_dump(m, o, a.v("s"), a.v("fp")); ().into() }),
        ("SI_dynamicSymbolListInsert", |m, _o, a| si_dynamic_symbol_list_insert(m, a.v("s"), a.v("list")).into()),
        ("SI_dynamicSymbolListRemoveAll", |m, _o, a| { si_dynamic_symbol_list_remove_all(m, a.v("list")); ().into() }),
        ("SI_dynamicSymbolListGetNumSymByType", |m, _o, a| si_dynamic_symbol_list_get_num_sym_by_type(m, a.v("type"), a.v("list")).into()),
        ("SI_dynamicSymbolListGetSymByType", |m, _o, a| si_dynamic_symbol_list_get_sym_by_type(m, a.v("type"), a.v("list"), a.v("n"), a.v("out")).into()),
        ("SI_dynamicSymbolListDump", |m, o, a| { si_dynamic_symbol_list_dump(m, o, a.v("list"), a.v("fp")); ().into() }),
        ("SI_dynamicLinkCreate", |m, _o, _a| si_dynamic_link_create(m).into()),
        ("SI_dynamicLinkReAllocate", |m, o, a| si_dynamic_link_re_allocate(m, o, a.v("n"), a.v("pp")).into()),
        ("SI_dynamicLinkDelete", |m, o, a| { si_dynamic_link_delete(m, o, a.v("pp")); ().into() }),
        ("SI_dynamicLinkSetName", |m, _o, a| si_dynamic_link_set_name(m, a.v("name"), a.v("l")).into()),
        ("SI_dynamicLinkGetHandle", |m, _o, a| si_dynamic_link_get_handle(m, a.v("l")).into()),
        ("SI_dynamicLinkSetHandle", |m, _o, a| si_dynamic_link_set_handle(m, a.v("handle"), a.v("l")).into()),
        ("SI_dynamicLinkLoad", |m, o, a| si_dynamic_link_load(m, o, a.v("l")).into()),
        ("SI_dynamicLinkUnload", |m, o, a| si_dynamic_link_unload(m, o, a.v("l")).into()),
        ("SI_dynamicLinkAddSymbol", |m, _o, a| si_dynamic_link_add_symbol(m, a.v("l"), a.v("name"), a.v("type")).into()),
        ("SI_dynamicLinkDump", |m, o, a| { si_dynamic_link_dump(m, o, a.v("l"), a.v("fp")); ().into() }),
        ("SI_udflistAdd", |m, _o, a| si_udflist_add(m, a.v("arr"), a.v("count"), a.v("item")).into()),
        ("SI_udflistDelete", |m, _o, a| si_udflist_delete(m, a.v("arr"), a.v("count"), a.v("index")).into()),
        ("SI_udflistMoveup", |m, _o, a| si_udflist_moveup(m, a.v("arr"), a.v("count"), a.v("index")).into()),
        ("SI_udflistModify", |m, o, a| si_udflist_modify(m, o, a.v("arr"), a.v("index"), a.v("item")).into()),
        ("SI_udflistReorder", |m, _o, a| si_udflist_reorder(m, a.v("arr"), a.v("count")).into()),
        ("SI_udflistCompact", |m, _o, a| si_udflist_compact(m, a.v("arr"), a.v("count")).into()),
        ("SI_cusIconGetSerialId", |m, _o, a| si_cus_icon_get_serial_id(m, a.v("key")).into()),
        ("SI_cusIconSetSerialId", |m, o, a| si_cus_icon_set_serial_id(m, o, a.v("key")).into()),
        ("SI_cusIconClearSerialIdTable", |m, _o, _a| { si_cus_icon_clear_serial_id_table(m); ().into() }),
        ("si_cusTrace", |m, o, a| {
            let mut v = vec![a.v("fmt")];
            for i in 0.. { let n = format!("a{i}"); if !a.has(&n) { break; } v.push(a.v(&n)); }
            si_cus_trace(m, o, &v); ().into()
        }),
        ("SI_udfDump", |m, o, a| { si_udf_dump(m, o, a.v("u"), a.v("fp")); ().into() }),
        ("SI_udfDumpAll", |m, o, a| { si_udf_dump_all(m, o, a.v("fp")); ().into() }),
        ("SI_udfIsInternal", |m, _o, a| si_udf_is_internal(m, a.v("u")).into()),
        ("SI_udfGetOrder", |m, _o, a| si_udf_get_order(m, a.v("u")).into()),
        ("SI_udfGetName", |m, _o, a| si_udf_get_name(m, a.v("u")).into()),
        ("SI_udfGetDatabase", |m, _o, a| si_udf_get_database(m, a.v("u")).into()),
        ("SI_getCustomSuffix", |m, o, a| si_get_custom_suffix(m, o, a.v("chap")).into()),
        ("SI_udfListGet", |m, _o, a| { si_udf_list_get(m, a.v("count"), a.v("list")); ().into() }),
        ("makeUDFChapterArrayRelations", |m, o, a| { make_udf_chapter_array_relations(m, o, a.v("chap")); ().into() }),
        ("SI_makeUDFArrayRelations", |m, o, _a| { si_make_udf_array_relations(m, o); ().into() }),
        ("cusSelect", |m, _o, a| cus_select(m, a.v("name")).into()),
        ("si_cusGetNbPostDefFX", |m, _o, _a| si_cus_get_nb_post_def_fx(m).into()),
        ("si_cusIncNbPostDefFX", |m, _o, _a| si_cus_inc_nb_post_def_fx(m).into()),
        ("si_cusDecNbPostDefFX", |m, _o, _a| si_cus_dec_nb_post_def_fx(m).into()),
        ("SI_cusSetRenderAbortFlag", |m, _o, a| si_cus_set_render_abort_flag(m, a.v("v")).into()),
        ("SI_cusGetRenderAbortFlag", |m, _o, _a| si_cus_get_render_abort_flag(m).into()),
        ("cusEffectStatusSetLatest", |m, _o, a| { cus_effect_status_set_latest(m, a.v("status"), a.v("old"), a.v("new")); ().into() }),
        ("SI_cusSceneCallbackAdd", |m, o, a| si_cus_scene_callback_add(m, o, a.v("scene"), a.v("list"), a.v("routine"), a.v("callback"), a.v("event"), a.v("flag")).into()),
        ("SI_cusSceneCallbackRemove", |m, o, a| si_cus_scene_callback_remove(m, o, a.v("scene"), a.v("list"), a.v("routine"), a.v("callback"), a.v("event"), a.v("flag")).into()),
        ("si_cusValueListDelete", |m, o, a| { si_cus_value_list_delete(m, o, a.v("list")); ().into() }),
        ("si_cusValueDelete", |m, o, a| { si_cus_value_delete(m, o, a.v("v")); ().into() }),
        ("si_cusValueListCopy", |m, o, a| si_cus_value_list_copy(m, o, a.v("list")).into()),
        ("si_cusValueListCreate", |m, _o, _a| si_cus_value_list_create(m).into()),
        ("si_cusDsoDataCopy", |m, _o, a| si_cus_dso_data_copy(m, a.v("src"), a.v("dst")).into()),
        ("si_cusDsoDataDelete", |m, _o, a| { si_cus_dso_data_delete(m, a.v("d")); ().into() }),
        ("si_cusDsoDataCreate", |m, _o, _a| si_cus_dso_data_create(m).into()),
        ("SI_cusClosePipes", |m, o, a| { si_cus_close_pipes(m, o, a.v("e")); ().into() }),
        ("si_cusFreeCommandList", |m, o, a| { si_cus_free_command_list(m, o, a.v("list")); ().into() }),
        ("SI_cusDeleteEffect", |m, o, a| { si_cus_delete_effect(m, o, a.v("e")); ().into() }),
        ("si_cusGetIconName", |m, _o, a| si_cus_get_icon_name(m, a.v("e")).into()),
        ("si_cusEffectIsProcessBased", |m, _o, a| si_cus_effect_is_process_based(m, a.v("e")).into()),
        ("SI_cusGetEffectByIcon", |m, _o, a| si_cus_get_effect_by_icon(m, a.v("icon"), a.v("out")).into()),
        ("si_cusStatusGetEffectByIcon", |m, _o, a| si_cus_status_get_effect_by_icon(m, a.v("status"), a.v("icon")).into()),
        ("SI_cusGetEffectByName", |m, _o, a| si_cus_get_effect_by_name(m, a.v("name"), a.v("out")).into()),
        ("SI_cusStatusGetEffectByName", |m, _o, a| si_cus_status_get_effect_by_name(m, a.v("status"), a.v("name")).into()),
        ("SI_cusRemoveEffect", |m, _o, a| si_cus_remove_effect(m, a.v("status"), a.v("model")).into()),
        ("SI_cusEffectClassGetPathname", |m, o, a| si_cus_effect_class_get_pathname(m, o, a.v("icon"), a.v("out")).into()),
        ("si_cusNewCmd", |m, _o, a| si_cus_new_cmd(m, a.v("list")).into()),
        ("SI_cusStatMotionPointersUpdate", |m, _o, _a| { si_cus_stat_motion_pointers_update(m); ().into() }),
        ("cusModifyRelDict", |m, o, a| cus_modify_rel_dict(m, o, a.v("dict"), a.v("old"), a.v("new")).into()),
        ("SI_cusEffectGetVersion", |m, _o, a| (si_cus_effect_get_version(m, a.v("name"), a.v("stem")) as u32).into()),
        ("si_cusEffectCreate", |m, _o, _a| si_cus_effect_create(m).into()),
        ("cusEffectStatusGetLatest", |m, _o, a| cus_effect_status_get_latest(m, a.v("status"), a.v("name"), a.v("stem")).into()),
        ("si_cusEffectStatusGetLatest", |m, _o, a| si_cus_effect_status_get_latest(m, a.v("status"), a.v("name")).into()),
        ("si_cusEffectGetLatest", |m, _o, a| si_cus_effect_get_latest(m, a.v("name")).into()),
        ("SI_cusGetStatusByName", |m, _o, a| si_cus_get_status_by_name(m, a.v("name")).into()),
        ("SI_cusStatPersistentListsAdd", |m, o, _a| (si_cus_stat_persistent_lists_add(m, o) as u32).into()),
        ("cusValueListRemoveUnsupportedSymbols", |m, o, a| (cus_value_list_remove_unsupported_symbols(m, o, a.v("dialog"), a.v("listp"), a.v("all")) as u32).into()),
        ("cusModifyAttribDict", |m, o, a| cus_modify_attrib_dict(m, o, a.v("dict"), a.v("old"), a.v("new")).into()),
        ("SI_cusReinit", |m, o, a| (si_cus_reinit(m, o, a.v("eff"), a.v("old")) as u32).into()),
        ("cusAddToMenu", |m, o, a| { cus_add_to_menu(m, o, a.v("st"), a.v("fp"), a.v("name"), a.v("dir")); ().into() }),
        ("SI_cusScan", |m, o, a| { files::begin(a); si_cus_scan(m, o, a.v("st")); ().into() }),
        ("SI_cusLoadEffect", |m, o, a| (si_cus_load_effect(m, o, a.v("status"), a.v("eff")) as u32).into()),
        ("si_cusLoadEffectExtensions", |m, o, a| { files::begin(a); (si_cus_load_effect_extensions(m, o, a.v("eff"), a.v("links"), a.v("name")) as u32).into() }),
        ("SI_cusCopyEffect", |m, o, a| { si_cus_copy_effect(m, o, a.v("src"), a.v("dst"), a.v("all") as i16); ().into() }),
        ("si_cusAddMotionEffect", |m, o, a| si_cus_add_motion_effect(m, o, a.v("st"), a.v("eff"), a.v("at") as i32).into()),
        ("si_cusTransferMaterial", |m, o, a| (si_cus_transfer_material(m, o, a.v("src"), a.v("dst")) as u32).into()),
        ("si_cusTransferTexture", |m, o, a| (si_cus_transfer_texture(m, o, a.v("src"), a.v("dst")) as u32).into()),
        ("SI_cusMakeDefinition", |m, o, a| { si_cus_make_definition(m, o, a.v("m"), a.v("all")); ().into() }),
        ("si_cusSwapGeometry", |m, o, a| (si_cus_swap_geometry(m, o, a.v("a"), a.v("b"), a.v("normals") as i16) as u32).into()),
        ("si_cusSaveKeyTransformation", |m, o, a| (si_cus_save_key_transformation(m, o, a.v("fp"), a.v("kind") as i32) as u32).into()),
        ("si_cusTransferUserData", |m, o, a| (si_cus_transfer_user_data(m, o, a.v("src"), a.v("dst")) as u32).into()),
        ("si_cusTransferCV", |m, o, a| (si_cus_transfer_cv(m, o, a.v("src"), a.v("dst"), a.v("keep") as i32, a.v("first") as i16, a.v("deep") as i16) as u32).into()),
        ("SI_cusTransferToDefinition", |m, o, a| { si_cus_transfer_to_definition(m, o, a.v("m"), a.v("deep")); ().into() }),
        ("si_cusTransferToData", |m, o, a| { si_cus_transfer_to_data(m, o, a.v("m"), a.v("deep")); ().into() }),
        ("SI_cusDeleteModel", |m, o, a| { si_cus_delete_model(m, o, a.v("model")); ().into() }),
        ("SI_cusDeleteElement", |m, o, a| { si_cus_delete_element(m, o, a.v("elem"), a.v("p2"), a.v("p3")); ().into() }),
        ("SI_udfInitializeFromDummy", |m, o, a| { si_udf_initialize_from_dummy(m, o, a.v("dst"), a.v("src")); ().into() }),
        ("SI_udfResetActive", |m, o, _a| { si_udf_reset_active(m, o); ().into() }),
        ("SI_udfSetAsSelected", |m, o, a| { si_udf_set_as_selected(m, o, a.v("u")); ().into() }),
        ("SI_udfIsEnabled", |m, o, a| (si_udf_is_enabled(m, o, a.v("u")) as u32).into()),
        ("SI_udfGetShaderName", |m, o, a| si_udf_get_shader_name(m, o, a.v("u"), a.v("buf"), a.v("size") as i32).into()),
        ("SI_udfGetShaderFuncDeclName", |m, o, a| si_udf_get_shader_func_decl_name(m, o, a.v("u"), a.v("buf"), a.v("size") as i32).into()),
        ("SI_udfGetDescName", |m, o, a| si_udf_get_desc_name(m, o, a.v("u"), a.v("buf"), a.v("size") as i32).into()),
        ("SI_udfUseDefault", |m, o, a| (si_udf_use_default(m, o, a.v("u")) as u32).into()),
        ("setElementDirty", |m, o, a| { set_element_dirty(m, o, a.v("u"), a.v("flag") as i16); ().into() }),
        ("SI_udfEditParms", |m, o, a| (si_udf_edit_parms(m, o, a.v("u")) as u32).into()),
        ("udfEncodeRawData", |m, o, a| (udf_encode_raw_data(m, o, a.v("u")) as u32).into()),
        ("truncateCustomValueToken", |m, _o, a| { truncate_custom_value_token(m, a.v("v"), a.v("dst"), a.v("n") as i32); ().into() }),
        ("is_valid_token", |m, _o, a| (is_valid_token(m, a.v("v")) as u32).into()),
        ("formatCustomValue", |m, o, a| (format_custom_value(m, o, a.v("v"), a.f("frame"), a.v("dst")) as u32).into()),
        ("SI_udfGenerateDeclares", |m, o, a| (si_udf_generate_declares(m, o, a.v("fp")) as u32).into()),
        ("SI_udfGenerateIncludes", |m, o, a| (si_udf_generate_includes(m, o, a.v("fp")) as u32).into()),
        ("SI_udfGenerateCodes", |m, o, _a| (si_udf_generate_codes(m, o) as u32).into()),
        ("SI_udfGenerateLinks", |m, o, _a| (si_udf_generate_links(m, o) as u32).into()),
        ("SI_udfListPointerUpdate", |m, _o, _a| { si_udf_list_pointer_update(m); ().into() }),
        ("SI_getElemId", |m, o, a| (si_get_elem_id(m, o, a.v("name"), a.v("d"), a.v("cls")) as u32).into()),
        ("SI_verifyDatabaseElementName", |m, o, a| (si_verify_database_element_name(m, o, a.v("name"), a.v("d"), a.v("cls")) as u32).into()),
        ("SI_verifyElemVisible", |m, o, a| (si_verify_elem_visible(m, o, a.v("name"), a.v("d"), a.v("cls")) as u32).into()),
        ("verifyElementArray", |m, o, a| (verify_element_array(m, o, a.v("s"), a.v("d"), a.v("cls")) as u32).into()),
        ("extractArrayId", |m, o, a| extract_array_id(m, o, a.v("s"), a.v("id")).into()),
        ("outputArrayListWarnings", |m, o, a| { output_array_list_warnings(m, o, a.v("d"), a.v("u"), a.v("name"), a.v("list"), a.v("kind")); ().into() }),
        ("SI_filterElemTokens", |m, o, a| { si_filter_elem_tokens(m, o, a.v("pp"), a.v("d"), a.v("cls"), a.v("keep") as i16, a.v("cb")); ().into() }),
        ("filterHiddenLights", |m, o, a| { filter_hidden_lights(m, o, a.v("pp"), a.v("d"), a.v("u"), a.v("name")); ().into() }),
        ("writeMatrix", |m, o, a| { write_matrix(m, o, a.v("mat"), a.v("suffix")); ().into() }),
        ("writeModelArrayElement", |m, o, a| (write_model_array_element(m, o, a.v("kind"), a.v("token"), a.v("model"), a.v("d"), a.v("elem"), a.v("first") as i16) as u32).into()),
        ("writeLightArray", |m, o, a| (write_light_array(m, o, a.v("d"), a.v("names"), a.v("member"), a.v("x"), a.v("first") as i16) as u32).into()),
        ("writeModelArray", |m, o, a| (write_model_array(m, o, a.v("d"), a.v("names"), a.v("member"), a.v("attrs"), a.v("first") as i16) as u32).into()),
        ("writeArray", |m, o, a| (write_array(m, o, a.v("d"), a.v("u"), a.v("cls"), a.v("value"), a.v("member"), a.v("attrs"), a.v("first") as i16) as u32).into()),
        ("SI_udfWrite", |m, o, a| (si_udf_write(m, o, a.v("u"), a.v("path"), a.v("chap"), a.v("version")) as u32).into()),
        ("SI_udfInitialize", |m, o, a| (si_udf_initialize(m, o, a.v("u"), a.v("name"), a.v("path"), a.v("chap")) as u32).into()),
        ("udfRead", |m, o, a| (udf_read(m, o, a.v("u"), a.v("path"), a.v("chap")) as u32).into()),
        ("SI_udfOutputToMI", |m, o, a| { udf3::NO_CAPTURE.with(|c| c.set(true)); let r = si_udf_output_to_mi(m, o, a.v("d"), a.v("u"), a.v("frame") as i32) as u32; udf3::NO_CAPTURE.with(|c| c.set(false)); r.into() }),
        ("cusGetShaderByDesc", |m, o, a| (cus_get_shader_by_desc(m, o, a.v("arr"), a.v("n") as i32, a.v("desc"), a.v("out"), a.v("count")) as u32).into()),
        ("si_cusModelSearchUserData", |m, _o, a| si_cus_model_search_user_data(m, a.v("model"), a.v("name")).into()),
        ("si_cusModelDisposeUserData", |m, _o, a| (si_cus_model_dispose_user_data(m, a.v("model"), a.v("name")) as u32).into()),
        ("si_cusModelAttachUserData", |m, _o, a| (si_cus_model_attach_user_data(m, a.v("model"), a.v("name"), a.v("size"), a.v("data")) as u32).into()),
        ("cusIconGetSavedHeader", |m, o, a| cus_icon_get_saved_header(m, o, a.v("model"), a.v("node")).into()),
        ("cusIconGetSavedScript", |m, o, a| cus_icon_get_saved_script(m, o, a.v("model"), a.v("node")).into()),
        ("SI_cusGetRelName", |m, _o, a| si_cus_get_rel_name(m, a.v("id")).into()),
        ("si_cusReadDescription", |m, o, a| (si_cus_read_description(m, o, a.v("fp"), a.v("out")) as u32).into()),
        ("cusEffectMappingCmp", |m, _o, a| (cus_effect_mapping_cmp(m, a.v("i"), a.v("a"), a.v("b")) as u32).into()),
        ("cusEffectMappingGet", |m, _o, a| cus_effect_mapping_get(m, a.v("a"), a.v("b"), a.v("pos")).into()),
        ("cusEffectMappingAdd", |m, _o, a| { cus_effect_mapping_add(m, a.v("pos") as i32, a.v("a"), a.v("b"), a.v("v")); ().into() }),
        ("cusEffectMappingClear", |m, _o, _a| { cus_effect_mapping_clear(m); ().into() }),
        ("cusDsoFcvkeyBinaryRead", |m, o, a| cus_dso_fcvkey_binary_read(m, o, a.v("key"), a.v("len"), a.v("buf"), a.v("pos")).into()),
        ("cusRestoreCustomElem", |m, o, a| cus_restore_custom_elem(m, o, a.v("id"), a.v("kind") as u16, a.v("group") as i32, a.v("param") as i32, a.v("refd"), a.v("a"), a.v("b"), a.v("c"), a.v("d")).into()),
        ("cusReInitDsoEffect", |m, o, a| cus_re_init_dso_effect(m, o, a.v("model")).into()),
        ("cusRestoreDsoEffect", |m, o, a| cus_restore_dso_effect(m, o, a.v("model")).into()),
        ("SI_cusPrepareEffect", |m, o, a| si_cus_prepare_effect(m, o, a.v("lib"), a.v("script")).into()),
        ("SI_cusRestoreCustomEffects", |m, o, a| si_cus_restore_custom_effects(m, o, a.v("dict")).into()),
        ("SI_cusCreateRelations", |m, o, a| si_cus_create_relations(m, o, a.v("model"), a.v("flag"), a.v("out")).into()),
        ("SI_cusSceneRead", |m, o, _a| { si_cus_scene_read(m, o); ().into() }),
        ("SI_cusAddUserData", |m, o, a| { si_cus_add_user_data(m, o, a.v("model")); ().into() }),
        ("deleteShaderballDict", |m, o, _a| { delete_shaderball_dict(m, o); ().into() }),
        ("createShaderballDict", |m, o, a| create_shaderball_dict(m, o, a.v("udf")).into()),
        ("SI_shdbTerminate", |m, o, _a| { si_shdb_terminate(m, o); ().into() }),
        ("cusStatusAddUserData", |m, o, a| (cus_status_add_user_data(m, o, a.v("status"), a.v("model")) as u32).into()),
        ("SI_shdbCtrlStart", |m, o, a| { si_shdb_ctrl_start(m, o, a.v("udf")); ().into() }),
        ("dynamicSymbolListNodeDelete", |m, _o, a| si_custom::custom_dyn::dynamic_symbol_list_node_delete(m, a.v("pp")).into()),
        ("SI_shdbCtrlParmsChanged", |m, o, a| { si_shdb_ctrl_parms_changed(m, o, a.v("udf")); ().into() }),
        ("SI_shdbInitialize", |m, o, a| { si_shdb_initialize(m, o, a.v("dlg"), a.v("udf")); ().into() }),
        ("cusDsoFcurveBinaryRead", |m, o, a| cus_dso_fcurve_binary_read(m, o, a.v("fc"), a.v("parent"), a.v("len"), a.v("buf"), a.v("pos")).into()),
        ("cusDsoFcvkeyBinaryWrite", |m, _o, a| cus_dso_fcvkey_binary_write(m, a.v("key"), a.v("pos"), a.v("cap"), a.v("bp")).into()),
        ("cusDsoFcurveBinaryWrite", |m, _o, a| cus_dso_fcurve_binary_write(m, a.v("fc"), a.v("len"), a.v("bp")).into()),
        ("cusStoreDsoFcurves", |m, o, a| cus_store_dso_fcurves(m, o, a.v("list"), a.v("len"), a.v("bp")).into()),
        ("cusDsoDialogWriteDataToString", |m, _o, a| cus_dso_dialog_write_data_to_string(m, a.v("list"), a.v("outp")).into()),
        ("cusDialogRestoreData", |m, o, a| cus_dialog_restore_data(m, o, a.v("list"), a.v("len") as i32, a.v("text"), a.v("defaults")).into()),
        ("cusRestoreDsoFcurves", |m, o, a| cus_restore_dso_fcurves(m, o, a.v("model"), a.v("status"), a.v("size") as i32).into()),
        ("cusDsoDialogGetDataSize", |m, _o, a| (cus_dso_dialog_get_data_size(m, a.v("list"), a.v("size"), a.v("count")) as u32).into()),
        ("computeRGBABufferLength", |m, _o, a| (compute_rgba_buffer_length(m, a.v("p")) as u32).into()),
        ("cusGetSelectedModelShader", |m, _o, a| (cus_get_selected_model_shader(m, a.v("out"), a.v("count"), a.v("kind")) as u32).into()),
        ("cusGetSelectedLightShader", |m, _o, a| (cus_get_selected_light_shader(m, a.v("out"), a.v("count")) as u32).into()),
        ("printUDFArrayWarning", |m, o, a| { print_udf_array_warning(m, o, a.v("dict"), a.v("u"), a.v("name")); ().into() }),
        ("SI_cusInit", |m, o, _a| { si_cus_init(m, o); ().into() }),
        ("si_cusResolveNameLinks", |m, o, a| { si_cus_resolve_name_links(m, o, a.v("list")); ().into() }),
        ("SI_cusContextInit", |m, _o, a| si_cus_context_init(m, a.v("p"), a.v("a2"), a.v("a3"), a.v("a4"), a.v("a5")).into()),
        ("cusNextString", |m, _o, a| cus_next_string(m, a.v("src"), a.v("dst")).into()),
        ("si_cusNumStrings", |m, _o, a| si_cus_num_strings(m, a.v("s")).into()),
        ("si_cusConcatStrings", |m, _o, a| {
            let mut v = Vec::new();
            for i in 0..a.v("n") { v.push(a.v(&format!("s{i}"))); }
            si_cus_concat_strings(m, &v).into()
        }),
        ("si_cusCreateShape", |m, o, a| si_cus_create_shape(m, o, a.v("model"), a.v("children") as i16).into()),
        ("si_cusTransferSrt", |m, _o, a| si_cus_transfer_srt(m, a.v("src"), a.v("dst")).into()),
        ("si_cusHrcReplicate", |m, o, a| si_cus_hrc_replicate(m, o, a.v("src")).into()),
        ("cusRecursiveHrcReplicate", |m, o, a| { cus_recursive_hrc_replicate(m, o, a.v("src"), a.v("parent")); ().into() }),
        ("cusPatchTransfer", |m, _o, a| { cus_patch_transfer(m, a.v("src"), a.v("dst")); ().into() }),
        ("si_cusFindModelByName", |m, o, a| si_cus_find_model_by_name(m, o, a.v("name")).into()),
        ("SI_cusCreateFcurves", |m, o, a| si_cus_create_fcurves(m, o, a.v("e")).into()),
        ("cusSortStringArray", |m, _o, a| cus_sort_string_array(m, a.v("n") as i32, a.v("arr")).into()),
        ("cusFreeStringArray", |m, _o, a| { cus_free_string_array(m, a.v("n") as i32, a.v("arr")); ().into() }),
        ("cusSearchNameArray", |m, _o, a| cus_search_name_array(m, a.v("arr"), a.v("n") as i32, a.v("name")).into()),
        ("cusGetFcurve", |m, _o, a| cus_get_fcurve(m, a.v("e"), a.v("name")).into()),
        ("SI_cusFetchCustomValue", |m, _o, a| si_cus_fetch_custom_value(m, a.v("model"), a.v("name"), a.v("fc")).into()),
        ("cusGetOutputShader", |m, _o, a| cus_get_output_shader(m, a.v("out"), a.v("count")).into()),
        ("cusGetLensShader", |m, _o, a| cus_get_lens_shader(m, a.v("out"), a.v("count")).into()),
        ("cusGetAtmosphereShader", |m, _o, a| cus_get_atmosphere_shader(m, a.v("out"), a.v("count")).into()),
        ("cusShaderSearchDesc", |m, _o, a| cus_shader_search_desc(m, a.v("shader"), a.v("n") as i32, a.v("descs")).into()),
        ("fcvDataInit", |m, _o, a| fcv_data_init(m, a.v("pp"), a.v("n") as i32).into()),
        ("fcvDataFree", |m, _o, a| { fcv_data_free(m, a.v("r")); ().into() }),
        ("cusFillShaderFcvData", |m, _o, a| cus_fill_shader_fcv_data(m, a.v("data"), a.v("shaders"), a.v("n") as i32, a.v("k") as i32).into()),
        ("cusvalFcurveOffsetAndScale", |m, o, a| { cusval_fcurve_offset_and_scale(m, o, a.v("list"), fl(a)); ().into() }),
        ("effectListFcurveOffsetAndScale", |m, o, a| { effect_list_fcurve_offset_and_scale(m, o, a.v("list"), fl(a)); ().into() }),
        ("t2dShaderOffsetAndScale", |m, o, a| { t2d_shader_offset_and_scale(m, o, a.v("t"), fl(a)); ().into() }),
        ("t3dShaderOffsetAndScale", |m, o, a| { t3d_shader_offset_and_scale(m, o, a.v("t"), fl(a)); ().into() }),
        ("shaderReadLinks", |m, o, a| { files::begin(a); shader_read_links(m, o, files::handle(), a.v("sh")).into() }),
        ("shaderReadDeclare", |m, o, a| { files::begin(a); shader_read_declare(m, o, files::handle(), a.v("sh")).into() }),
        ("SI_shaderGetSymbolType", |m, _o, a| si_shader_get_symbol_type(m, a.v("sh"), a.v("name"), a.v("kind"), a.v("text")).into()),
    ]))))));
    let _ = ALL.set(all.clone());
    let tramps: Vec<(&'static str, si_checkkit::Port)> = all.iter().map(|(n, _)| (*n, tramp as si_checkkit::Port)).collect();
    Kit::new(&tramps)
    .stand_in("SI_fcurveValue", cv::fcurve_value)
    .stand_in("SI_fcurveFrameValue", c11::fcurve_frame_value)
    .setup(setup)
    .effect(shdb5::effect)
    .normalise_stack_args()
    .route(files::route)
    .run()
}

fn fl(a: &si_checkkit::Args) -> [u32; 5] {
    [a.v("f0"), a.v("f1"), a.v("f2"), a.v("f3"), a.v("f4")]
}

/// The image the emulated original starts from: the shader-database data, and the C library's
/// three standard streams as the harness numbers them (finding 0011).
fn setup(m: &mut si_mem::Mem) {
    si_custom::store_data(m);
    m.set_u32(si_custom::STDIN, 0x6f00_0000);
    m.set_u32(si_custom::STDOUT, 0x6f00_0100);
    m.set_u32(si_custom::STDERR, 0x6f00_0200);
}
