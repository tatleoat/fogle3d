// SPDX-License-Identifier: MIT
//! What the batch path calls that is not on the image in this crate: one line per callee,
//! with its name, its address in the Linux binary (a C library routine: the binary's own
//! stub) and the arguments as the original pushes them. Nothing here reproduces a function
//! of the original.
//!
//! The C library's string and memory routines (`strdup`, `strlen`, `strcpy`, `strcat`,
//! `strncmp`, `sprintf`, `sscanf`, `atoi`, `atof`, `free`) are not calls: they work on the
//! image and the port does what they do there (`si_mem::clib`, the image's allocator).
//! What is printed goes to [`si_mem::Outside::print`] (1 `stdout`, 2 `stderr`), already
//! formatted, and `exit` is the port's [`crate::Exit`].

use si_mem::{outside_calls, CStr32};

outside_calls! {
    /// The callees of the batch path that are outside this module.
    pub trait BatchCalls {
        // ---- the C library: services that are not memory
        fn getenv(name: CStr32) -> CStr32 = "getenv", 0x080c3bb8;
        /// `getcwd(NULL, size)` allocates the answer with `malloc` (glibc 2.2: `size`
        /// bytes); [`super::batch::libc_getcwd`] does that on the image.
        fn getcwd(buf: u32, size: u32) -> CStr32 = "getcwd", 0x080c4d08;
        fn access(path: CStr32, mode: i32) -> i32 = "access", 0x080c3f18;
        fn fopen(path: CStr32, mode: CStr32) -> u32 = "fopen", 0x080c4e18;
        /// `fgets(buf, n, stream)`: the next line of the stream (at most `n - 1` bytes and a
        /// terminator, the line feed included) into `buf`; null at the end of the input,
        /// `buf` unchanged then.
        fn fgets(buf: u32, n: i32, stream: u32) -> u32 = "fgets", 0x080c4388;
        fn fclose(stream: u32) -> i32 = "fclose", 0x080c4958;
        /// `time(&local)`: the answer is also stored through the pointer.
        fn time(tloc: u32) -> i32 = "time", 0x080c4328;
        /// The C library's broken-down time (`struct tm`; +0x14 the year less 1900).
        fn localtime(timep: u32) -> u32 = "localtime", 0x080c3a98;
        fn geteuid() -> u32 = "geteuid", 0x080c5128;
        /// The user's password entry (`struct passwd`; +0x00 the name).
        fn getpwuid(uid: u32) -> u32 = "getpwuid", 0x080c3da8;
        fn setlocale(category: i32, locale: CStr32) -> CStr32 = "setlocale", 0x080c3d88;
        /// The absolute path of `path`, written into `resolved`; null when it has none.
        fn realpath(path: CStr32, resolved: u32) -> CStr32 = "realpath", 0x080c4168;

        // ---- other parts of the program
        /// `menutable.c`, `status.c`: the menu tables, where the status file goes.
        fn si_menu_table_init() = "SI_menuTableInit", 0x083a0088;
        fn si_status_init_path() = "SI_statusInitPath", 0x083cd7f8;
        /// `mental_*.c`: whether mental ray 1.9 is enabled; which `.mi` version is written.
        fn si_is_mr19_enable() -> i16 = "SI_isMr19Enable", 0x08580d0c;
        fn si_set_mi2_output(on: i32) = "SI_setmi2Output", 0x08580ce0;
        /// `strutil.c`: a name without its suffix, without its directory (new strings).
        fn si_str_strip_suffix(name: CStr32) -> CStr32 = "SI_strStripSuffix", 0x083cefd4;
        fn si_str_strip_dir(name: CStr32) -> CStr32 = "SI_strStripDir", 0x083cee28;
        /// `custom_*.c`: a custom effect's function run without the program.
        fn si_cus_run_standalone(symbol: CStr32, argc: i32, argv: u32) -> i32 = "SI_cusRunStandalone", 0x0852e7a0;
        /// `dbexternal.c`: a command through the shell.
        fn si_system(command: CStr32) -> i32 = "SI_system", 0x08126748;
        /// `init_dict.c`: the dictionaries and the database list (finding 0007); it sets
        /// `SI_pDbiin` and `SI_pDbiout`.
        fn si_init_dictionaries(resources: CStr32, a: i32, b: i32) = "SI_initDictionaries", 0x0810824c;
        /// `dbtools.c`: a database's chapter, its path, whether it may be written.
        fn si_db_set_chapter(db: u32, chapter: CStr32) -> i32 = "SI_dbSetChapter", 0x08122654;
        fn si_dbfilepath_build(db: u32) = "SI_dbfilepathBuild", 0x081226cc;
        fn si_db_chapter_check_access(db: u32, mode: i32) -> i16 = "SI_dbChapterCheckAccess", 0x08120f20;
        fn si_db_dir_get_database_from_name(dir: u32, name: CStr32) -> u32 = "SI_dbDirGetDatabaseFromName", 0x08121b50;
        /// `dbfile.c`: the latest version of a scene's name (a new string).
        fn si_dbfile_find_latest_version(db: u32, name: CStr32) -> CStr32 = "SI_dbfileFindLatestVersion", 0x08125cf4;
        /// `wave.c`, `graflist.c`, `camera.c`: waves, the graph lists, a camera.
        fn si_init_wave() = "SI_initWave", 0x084c5c70;
        fn si_graf_list_create() = "SI_grafListCreate", 0x0832c1bc;
        fn si_graf_list_add() -> i32 = "SI_grafListAdd", 0x0832c204;
        fn si_new_camera(name: CStr32, flag: i32) -> u32 = "SI_newCamera", 0x080d12dc;
        fn si_graflist_add_data(list: u32, data: u32, kind: i32, flag: i32) = "SI_graflistAddData", 0x0832d240;
        /// `matter.c` (its port is `si_rensetup::native::matter::matter_init`),
        /// `metaball.c`, `custom_*.c`.
        fn si_matter_init() = "SI_matterInit", 0x08291bec;
        fn si_metaball_init() = "SI_metaballInit", 0x0845abc8;
        fn si_cus_init() = "SI_cusInit", 0x085325c4;
        /// `dbloadsave.c`: **the hand-over to Get Scene** (finding 0018).
        fn si_db_load_scene(ed: u32, db: u32, name: CStr32) -> i16 = "SI_dbLoadScene", 0x080da548;
        /// `sys_main.c`, not ported: the render parameters asked on a stream, the answers
        /// read from the standard input or, with `-l`, from `argv` (it moves `argv` and
        /// `argc`).
        fn read_setup(stream: u32) = "readSetup", 0x080cf5e0;
        /// `camera.c`: camera and scene moved to the origin (`-t`).
        fn si_cam_and_scn_to_origin() = "SI_camAndScnToOrigin", 0x080d7170;
        /// `custom_*.c`: custom effects told what the program is doing.
        fn si_cus_call_custom_functions(state: i32, what: i32) = "SI_cusCallCustomFunctions", 0x0852ea24;
        /// The renderers.
        fn si_sequence_wire_frame() = "SI_sequenceWireFrame", 0x0836c690;
        fn si_sequence_hidden_line(flag: i32) = "SI_sequenceHiddenLine", 0x082a1d50;
        fn si_sequence_render() = "SI_sequenceRender", 0x082a2000;
        fn si_sequence_render_mental(flag: i32) = "SI_sequenceRenderMental", 0x082c5ddc;
    }
}

outside_calls! {
    /// The callees of `system.c`'s editing commands that are another file's.
    pub trait SystemCalls {
        /// `window.c`, `event.c`, `graflist.c`: the windows, the event queue, the selection.
        fn si_window_get_next(slot: u32) -> i16 = "SI_windowGetNext", 0x083d6484;
        fn si_get_event() -> u32 = "SI_getEvent", 0x08394058;
        fn si_check_supra(ev: u32, flag: i32) -> i16 = "SI_checkSupra", 0x0841c020;
        fn si_fast_key(ev: u32) = "SI_fastKey", 0x08403d80;
        fn si_gl_select(win: u32) -> u32 = "SI_gLSelect", 0x0832ef0c;
        /// `model*.c`, `wave.c`, `metaball.c`: what the update of a deformation asks.
        fn si_model_spline_fitting(model: u32, out: u32) -> u32 = "SI_modelSplineFitting", 0x08472b44;
        fn si_model_lattice(model: u32, out: u32) -> u32 = "SI_modelLattice", 0x0843ff1c;
        fn si_model_patch_fitting(model: u32, out: u32) -> u32 = "SI_modelPatchFitting", 0x08472c3c;
        fn si_model_node_wave(model: u32) -> u32 = "SI_modelNodeWave", 0x084c7610;
        fn si_model_branch_wave(model: u32) -> u32 = "SI_modelBranchWave", 0x084c75dc;
        fn si_model_readjust_definition(model: u32) = "SI_modelReadjustDefinition", 0x0846e340;
        fn si_metaball_touch(a: u32, b: u32) = "SI_metaballTouch", 0x0845abf8;
        fn si_refresh_counter() = "SI_refreshCounter", 0x080d1d20;
        fn si_refresh_all(flag: i32) = "SI_refreshAll", 0x08325dc8;
        /// `scene*.c`, `audio*.c`: the scene's state and the audio markers.
        fn si_scene_dirty() -> i16 = "SI_sceneDirty", 0x082149f4;
        fn si_system_warning(text: u32, details: u32, question: u32) -> i16 = "SI_systemWarning", 0x081422d8;
        fn si_edit_normals_end() = "SI_editNormalsEnd", 0x082ccaf0;
        fn si_audio_mng_at_count(container: u32) -> i32 = "SI_audioMngAtCount", 0x08376020;
        fn si_audio_mng_at_delete(container: u32, node: u32, flag: i32, out: u32) = "SI_audioMngAtDelete", 0x08371b7c;
        fn si_item_relax_constraint(item: u32, kind: i32) = "SI_itemRelaxConstraint", 0x081f533c;
        fn si_scene_file_reset() = "SI_sceneFileReset", 0x0823cc5c;
        fn si_matter_fade_init() = "SI_matterFadeInit", 0x08291bd4;
        fn si_scene_delete_all_not_in_history() = "SI_sceneDeleteAllNotInHistory", 0x0832c5d0;
        fn si_fcv_scngl_clear() = "SI_fcvScnglClear", 0x0823c948;
        fn si_cam_id_convert(id: i32) -> i32 = "SI_camIdConvert", 0x080d6f94;
        /// `dov*.c`, `camera.c`, `screen.c`: what a zoom rectangle asks of the camera.
        fn si_do_save_cam_pos(win: u32) = "SI_doSaveCamPos", 0x0834a024;
        fn si_dov_crop(win: u32, x0: i32, x1: i32, y0: i32, y1: i32, button: i32) = "SI_dovCrop", 0x0834a5c4;
        fn si_dov_set_cam_ortho(win: u32, flag: i32, button: i32) = "SI_dovSetCamOrtho", 0x0834bfb8;
        fn si_screen_to_world(win: u32, x: i32, y: i32, out: u32, a: i32, b: i32) -> i32 = "SI_screenToWorld", 0x08334878;
        fn si_set_cam_ortho_view(cam: u32, p0: u32, p1: u32, a: i32, b: i32) = "SI_setCamOrthoView", 0x0836c1bc;
        fn si_set_cam_view(cam: u32, p0: u32, p1: u32, flag: i32) = "SI_setCamView", 0x080d6048;
    }
}
