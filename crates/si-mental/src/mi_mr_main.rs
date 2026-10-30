// SPDX-License-Identifier: MIT
//! `mi_mr_main.c`: the translator's main file (package 63, finding 0042): the `.mi` header, the
//! frame loop, the camera, view and option sections, and the live-connection entry points.
//!
//! Like `mi_patch`, every byte of the scene goes through `SI_mrpipeFPRINTF (0x0842c68c)`, which is
//! not ported: a routine here calls it through [`Outside`] with the format string's address and the
//! argument words, exactly as the original pushes them (a `double` as two words, low word first). A
//! check compares the text as format address plus words. Callees of other source files are
//! `Routine`s called through [`Outside`]; the C library's string routines the original calls
//! (`sprintf`, `strdup`, `free`) are done on the image, as the emulator's stand-ins do.
//!
//! The licence-tier branch of the original is not here (none of these routines has one).

use si_mem::{clib, Mem, Outside, Routine};

/// `SI_mrpipeFPRINTF (0x0842c68c)`: printf on the pipe to mental ray.
pub const MRPIPE_FPRINTF: Routine = Routine::new("SI_mrpipeFPRINTF", 0x0842_c68c);

fn fprintf(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

/// Callees of other source files, by address.
pub mod ext {
    use si_mem::Routine;
    macro_rules! r { ($($n:ident = $s:literal @ $a:expr),* $(,)?) => { $(pub const $n: Routine = Routine::new($s, $a);)* } }
    r! {
        SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage" @ 0x083c_ca94,
        SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY = "SI_statusDisplayMessageToFileOnly" @ 0x083c_cf40,
        SI_IS_IGLOO = "si_IsIGLOO" @ 0x080d_3738,
        WINSET = "winset" @ 0x0861_993c,
        GFLUSH = "gflush" @ 0x0861_3094,
        SI_MUTEX_LOCK = "SI_mutexLock" @ 0x0842_fc5c,
        SI_MUTEX_UNLOCK = "SI_mutexUnlock" @ 0x0842_fc80,
        SI_PIPEIO_FILE_OPEN = "SI_pipeioFileOpen" @ 0x0842_fe84,
        SI_PIPEIO_DESTROY = "SI_pipeioDestroy" @ 0x0842_fcfc,
        SI_MRPIPE_FLUSH = "SI_mrpipeFlush" @ 0x0842_c5d8,
        SI_MEMTRACK_FREE = "SI_memtrackFree" @ 0x0859_96e0,
        SI_MEMTRACK_CHECK = "SI_memtrackCheck" @ 0x0859_98fc,
        DESTROY_2D_TEXTURE_LIST = "destroy_2D_texture_list" @ 0x0859_1ff0,
        SI_PREPARE_RENDERING = "SI_prepareRendering" @ 0x082c_57e4,
        SI_INIT_MENTAL_PIPE_PARMS = "SI_initMentalPipeParms" @ 0x0842_b654,
        SI_ASSIGN_MAXCOC = "SI_assignMaxcoc" @ 0x082c_b734,
        SI_DELETE_RENDER_DICTIONARY = "SI_deleteRenderDictionary" @ 0x082c_56e0,
        SI_UDF_IS_ENABLED = "SI_udfIsEnabled" @ 0x0854_5328,
        SI_UDF_GET_SHADER_NAME = "SI_udfGetShaderName" @ 0x0854_5398,
        SI_UDF_GET_SHADER_FLAGS = "SI_udfGetShaderFlags" @ 0x0854_66dc,
        SI_BUILD_PIC_FILE_NAME = "SI_buildPicFileName" @ 0x082c_b7a8,
        SI_GET_RENDER_IMG_SUFFIX = "SI_getRenderImgSuffix" @ 0x082a_59ac,
        SI_DB_DIR_SEARCH_DATABASE_FROM_NAME = "SI_dbDirSearchDatabaseFromName" @ 0x0812_1744,
        SI_MRFILE_GET_EXEC_FULLPATH = "SI_mrfileGetExecFullpath" @ 0x0842_f930,
        SI_MRFILE_CONVERT_TO_UNIX = "SI_mrfileConvertToUnix" @ 0x0842_faa8,
        SI_UDF_GENERATE_CODES = "SI_udfGenerateCodes" @ 0x0854_6018,
        SI_UDF_GENERATE_LINKS = "SI_udfGenerateLinks" @ 0x0854_6030,
        SI_UDF_GENERATE_DECLARES = "SI_udfGenerateDeclares" @ 0x0854_5fe8,
        SI_GET_RENDER_CAMERA = "SI_getRenderCamera" @ 0x082c_b590,
        SI_PIPEIO_WRITE = "SI_pipeioWrite" @ 0x0842_fd68,
        FPUTC = "fputc" @ 0x080c_4708,
        FPUTS = "fputs" @ 0x080c_4408,
        EXIT = "exit" @ 0x080c_4b58,
        SI_TXT_PROJECTION_INFO = "SI_txtProjectionInfo" @ 0x0829_b584,
        SI_ELEMDICT_RELATIONS_COUNT = "SI_elemdictRelationsCount" @ 0x0813_5700,
        SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem" @ 0x0813_2b50,
        SI_STR_APPEND = "SI_strAppend" @ 0x083c_f280,
        TIME = "time" @ 0x080c_4328,
        CTIME = "ctime" @ 0x080c_4998,
        GETENV = "getenv" @ 0x080c_3bb8,
        INIT_MATRIX = "init_matrix" @ 0x0859_83ec,
        SI_FRAME_TO_SEC = "SI_frameToSec" @ 0x0820_1554,
        INIT_MATERIALS = "init_materials" @ 0x0857_cdc0,
        SI_UDF_GENERATE_TEXTURE_DEFS = "SI_udfGenerateTextureDefs" @ 0x0854_609c,
        SI_ELEMLIST_LOW_BYTE_WORK_FLAG_AND = "SI_elemlistLowByteWorkFlagAND" @ 0x0813_2cac,
        PRINT_2D_TEXTURE_MAP = "print_2D_texture_map" @ 0x0859_1380,
        PRINT_LIGHT_DEFS = "print_light_defs" @ 0x0859_8d50,
        PRINT_2D_TEXTURE_DEFS = "print_2D_texture_defs" @ 0x0859_16fc,
        PRINT_ALL_MATERIAL = "print_all_material" @ 0x0857_f5e0,
        PRINT_ALL_MODELS = "print_all_models" @ 0x0858_5e60,
        CLEANUP_2D_TEXTURE_LIST = "cleanup_2D_texture_list" @ 0x0859_219c,
        SI_ROTATE_WORLD = "si_rotateWorld" @ 0x0858_23cc,
        PRINT_VIEW = "print_view" @ 0x0858_26f4,
        SI_MEMTRACK_INITIALIZE = "SI_memtrackInitialize" @ 0x0859_95f0,
        IS_SCANLINE_OFF = "_IsScanlineOff" @ 0x0858_5aa4,
        SI_UDF_GET_OUTPUT_BUFFER_FLAGS = "SI_udfGetOutputBufferFlags" @ 0x0854_6710,
        SI_UDF_OUTPUT_TO_MI = "SI_udfOutputToMI" @ 0x0854_57dc,
        SI_UDF_USE_DEFAULT = "SI_udfUseDefault" @ 0x0854_5488,
        INVERT_MATRIX = "invert_matrix" @ 0x0859_883c,
        SI_MEMTRACK_ALLOCATE = "SI_memtrackAllocate" @ 0x0859_9610,
        SI_INIT_TXT_SEQ_COUNTER = "SI_initTxtSeqCounter" @ 0x0859_1230,
        SI_ELEMDICT_DUMP = "SI_elemdictDump" @ 0x0813_4ba8,
        SI_PERSPECTIVE_NOT_B_VIEW = "SI_perspectiveNotBView" @ 0x082c_bc3c,
        SI_MRPIPE_SET = "SI_mrpipeSet" @ 0x0842_c578,
        FOPEN = "fopen" @ 0x080c_4e18,
        FGETS = "fgets" @ 0x080c_4388,
        FCLOSE = "fclose" @ 0x080c_4958,
        INIT_SEQUENCE_MENTAL = "init_sequence_mental" @ 0x0858_0f48,
        CLEANUP_SEQUENCE_MENTAL = "cleanup_sequence_mental" @ 0x0858_1460,
        PRINT_FRAME = "print_frame" @ 0x0858_1530,
        PRINT_OPTION = "print_option" @ 0x0858_4e8c,
        PRINT_CAMERA = "print_camera" @ 0x0858_39f4,
        SI_MI2_MODEL_FORWARD_DEFS = "si_mi2ModelForwardDefs" @ 0x0859_7e18,
        SI_GENERATE_MI_PER_FRAME_FILE_NAME = "SI_generateMIPerFrameFileName" @ 0x0842_c40c,
        BUILD_FULL_PIC_FILE_NAME = "buildFullPicFileName" @ 0x0858_14a8,
        OPEN_MI_FILE = "openMIfile" @ 0x0858_59c4,
        PRINT_HEADER = "printHeader" @ 0x0858_0700,
        MI2_PRINT_HEADER = "mi2_printHeader" @ 0x0858_08ac,
        DISPLAY_MESSAGES = "displayMessages" @ 0x0858_0c3c,
        SDEBUG = "sdebug" @ 0x0859_8cdc,
        SI_MRTHREAD_EXIT = "SI_mrthreadExit" @ 0x0843_02cc,
        SI_RENDER_SCENE_UPDATE = "SI_renderSceneUpdate" @ 0x082c_8420,
        SI_CAMERA = "SI_camera" @ 0x0827_2450,
        SI_PROCESS_TEXTURES = "SI_processTextures" @ 0x0825_af78,
        SI_PROCESS_MATERIALS = "SI_processMaterials" @ 0x0825_a874,
        PRINT_SCENE = "print_scene" @ 0x0858_1dd4,
        MI2_PRINT_SCENE = "mi2_print_scene" @ 0x0858_2054,
        CREAT = "creat" @ 0x080c_3df8,
        ACCESS = "access" @ 0x080c_3f18,
        CLOSE = "close" @ 0x080c_3b08,
        UNLINK = "unlink" @ 0x080c_3c78,
        USLEEP = "usleep" @ 0x080c_37b8,
        MI2_PRINT_LIGHT_DEFS = "mi2_print_light_defs" @ 0x0859_57d4,
        MI2_PRINT_ALL_MODELS = "mi2_print_all_models" @ 0x0859_6710,
        SI_ELEMDICT_GET_FIRST_RELATED_ELEM = "SI_elemdictGetFirstRelatedElem" @ 0x0813_8094,
        SI_ELEMDICT_GET_NEXT_RELATED_ELEM = "SI_elemdictGetNextRelatedElem" @ 0x0813_8104,
        SI_MENTAL_SHADER_SCAN = "SI_mentalShaderScan" @ 0x082c_7be0,
        SI_TEXTURE_INITIALIZE = "SI_textureInitialize" @ 0x082b_2508,
        SI_TEXTURE3D_INITIALIZE = "SI_texture3DInitialize" @ 0x082b_8ab0,
        PRINT_2D_TEXTURE_DEFS_PREVIEW = "print_2D_texture_defs_preview" @ 0x0859_1938,
        PRINT_MATERIAL = "print_material" @ 0x0857_cfd0,
        MI2_PRINT_VIEW = "mi2_print_view" @ 0x0858_55c8,
        SI_UDF_GET_SHADER_FUNC_DECL_NAME = "SI_udfGetShaderFuncDeclName" @ 0x0854_53e4,
        BUILD_SHADER_NAME = "buildShaderName" @ 0x0859_52f4,
        PRINT_INSTANCE = "printInstance" @ 0x0859_80b8,
        PRINT_GROUP = "print_group" @ 0x0859_8178,
    }
}

/// The translator's globals in `.data`/`.bss` (names are the original's symbols).
pub mod g {
    pub const CONNECTED_TO_NAMED_PIPE: u32 = 0x086e_fa28;
    pub const MI2_OUTPUT: u32 = 0x086e_f9c0; // 16 bits
    pub const INCREMENTAL: u32 = 0x086e_f9bc; // written as 32 bits, read as 16
    pub const BLUR2D_SHADER_LIB: u32 = 0x086e_fa68;
    pub const BLUR_DONE: u32 = 0x086e_fa6c;
    pub const SI_DBDIR: u32 = 0x0878_1390;
    pub const SI_SYSID: u32 = 0x0867_940c;
    pub const SI_GBL_VERT: u32 = 0x086e_f9c8;
    pub const MB_NB_MAX_VERT: u32 = 0x0882_9f10;
    pub const MEMTRACK_PARAMS: u32 = 0x0882_9f14;
    pub const MEMORY_CHECK: u32 = 0x086e_fa24;
    pub const SI_CID_MODELS: u32 = 0x0878_1680;
    pub const VERBOSE_FLAG: u32 = 0x0882_9f24;
    pub const SI_RESDIR: u32 = 0x0878_1628;
    pub const OUT: u32 = 0x0882_9f08;
    pub const STDOUT: u32 = 0x0878_0c84;
    pub const FRAME: u32 = 0x0882_9f38;
    pub const MI_OPTIMIZE: u32 = 0x086e_f9d0;
    pub const CID_TEXTURES2D: u32 = 0x0878_16c8;
    pub const NFRAMES_IN_MIFILE: u32 = 0x0882_9f44;
    pub const RENDER_DEBUG: u32 = 0x086e_fa26; // 16 bits
    pub const SI_REN: u32 = 0x0879_0be0; // 0x600 bytes, a copy of the render control from rp+0xc
    pub const SI_DATABASE: u32 = 0x0879_0640; // 0x13c bytes, a copy of the database record
    pub const RENDER_ORTHO: u32 = 0x086e_f9cc; // 16 bits
    pub const PERSPECTIVE_NOT_B: u32 = 0x086e_f9ce; // 16 bits
    pub const NFRAMES: u32 = 0x0882_9f40;
    pub const FRAMEINC: u32 = 0x0882_9f3c;
    pub const W_879107A: u32 = 0x0879_107a; // 16 bits
    pub const D_87907E8: u32 = 0x0879_07e8;
    pub const SCN_OUT_NAME: u32 = 0x0879_1074; // the name openMIfile is given
    pub const SI_FRAME_CURRENT: u32 = 0x0878_2040;
    pub const W_8790D1C: u32 = 0x0879_0d1c; // 16 bits: a second pass per frame
    pub const W_8790FBA: u32 = 0x0879_0fba; // 16 bits
}

/// Offsets in `SI_renderExecuteParms` (the translator's argument record), by the use made of them
/// here. Read from the code; the record is not laid out in `si-types`.
pub mod rp {
    pub const DICT_ARRAY: u32 = 0x0; // pointer; freed by SI_closeLiveConnection
    pub const DICT: u32 = 0x4; // SI_ElemDict
    pub const LIVE_FLAG: u32 = 0x148; // 16 bits, set by SI_translateInLiveConnection
    pub const FRAME_START: u32 = 0x200;
    pub const FRAME_END: u32 = 0x202;
    pub const FRAME_STEP: u32 = 0x204;
    pub const NO_OUTPUT: u32 = 0x3e6; // 16 bits
    pub const RAY_OPTIONS: u32 = 0x49c; // char*
    pub const FREE_A0: u32 = 0x4a0;
    pub const MBLUR: u32 = 0x4a6; // 16 bits: motion blur on
    pub const MBLUR_MODE: u32 = 0x4a8; // 16 bits
    pub const MOVING: u32 = 0x520; // 16 bits
    pub const DISPLAY_MODE: u32 = 0x60e; // 16 bits
    pub const FREE_610: u32 = 0x610;
    pub const SCENE_NAME: u32 = 0x61c; // char*
    pub const PIPE: u32 = 0x62c; // the pipe-io record: +4 mutex, +0x30 a string
    pub const IN_MESSAGE: u32 = 0x640; // 16 bits
    pub const OUT_MODE: u32 = 0x654;
    pub const TRAILER: u32 = 0x670; // char*
    pub const TEX_MODE: u32 = 0x60c; // 16 bits
    pub const MUTEX: u32 = 0x630;
}

fn s16(m: &Mem, a: u32) -> u32 {
    m.i16(a) as i32 as u32
}

/// `SI_setmi2PipeConnection (0x08580cc8)`: the connection kind becomes 2 (named pipe, extended); answers the old one.
/// [soft 0x08580cc8 SI_setmi2PipeConnection]
pub fn si_set_mi2_pipe_connection(m: &mut Mem) -> u32 {
    let old = m.u32(g::CONNECTED_TO_NAMED_PIPE);
    m.set_u32(g::CONNECTED_TO_NAMED_PIPE, 2);
    old
}

/// [soft 0x08580ce0 SI_setmi2Output]: stores the 16-bit value, answers the old one (sign-extended).
pub fn si_set_mi2_output(m: &mut Mem, v: u32) -> u32 {
    let old = s16(m, g::MI2_OUTPUT);
    m.set_u16(g::MI2_OUTPUT, v as u16);
    old
}

/// [soft 0x08580cfc SI_getmi2Output]
pub fn si_get_mi2_output(m: &Mem) -> u32 {
    s16(m, g::MI2_OUTPUT)
}

/// [soft 0x08580d84 SI_mi2IsIncremental]: the low 16 bits, sign-extended.
pub fn si_mi2_is_incremental(m: &Mem) -> u32 {
    s16(m, g::INCREMENTAL)
}

/// [soft 0x08580d94 SI_setmi2Incremental]: stores the argument sign-extended from 16 bits into all 32 bits.
pub fn si_set_mi2_incremental(m: &mut Mem, v: u32) {
    m.set_u32(g::INCREMENTAL, v as u16 as i16 as i32 as u32);
}

/// [soft 0x08585e20 __static_initialization_and_destruction_0]: `out = stdout` for `(1, 0xffff)`.
pub fn static_init_destruction_0(m: &mut Mem, initialize: u32, priority: u32) {
    if priority == 0xffff && initialize != 0 {
        let so = m.u32(g::STDOUT);
        m.set_u32(g::OUT, so);
    }
}

/// [soft 0x08585e40 global.constructors.keyed.to.progname]
pub fn global_constructors_keyed_to_progname(m: &mut Mem) {
    static_init_destruction_0(m, 1, 0xffff);
}

/// [soft 0x085826dc SI_onoff]: the address of "on" or "off".
pub fn si_onoff(v: u32) -> u32 {
    if v as u16 != 0 { 0x086f_0410 } else { 0x086f_0413 }
}

/// The three `SI_spdlUseNamesrv*` flags: the environment variable named by the global at `name_ptr`
/// is set and not empty (the original's inlined `strlen` walk), once.
fn env_flag(m: &mut Mem, o: &mut dyn Outside, name_ptr: u32, done: u32, flag: u32) {
    if m.i16(done) == 0 {
        let n = m.u32(name_ptr);
        let p = o.call(m, ext::GETENV, &[n]).eax;
        if p != 0 && m.u8(p) != 0 {
            m.set_u16(flag, 1);
        }
        m.set_u16(done, 1);
    }
}

/// `SI_spdlUseNamesrvShdDecl (0x08580da4)`. [soft 0x08580da4 SI_spdlUseNamesrvShdDecl]
pub fn si_spdl_use_namesrv_shd_decl(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    env_flag(m, o, 0x086e_fa3c, 0x086e_fdd0, 0x086e_fdd2);
    s16(m, 0x086e_fdd2)
}

/// `SI_spdlUseNamesrvShdRef (0x08580e28)`: set by its variable, and then the declaration's answer.
/// [soft 0x08580e28 SI_spdlUseNamesrvShdRef]
pub fn si_spdl_use_namesrv_shd_ref(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    env_flag(m, o, 0x086e_fa50, 0x086e_fdd4, 0x086e_fdd6);
    if m.i16(0x086e_fdd6) != 0 {
        (si_spdl_use_namesrv_shd_decl(m, o) as u16 != 0) as u32
    } else {
        0
    }
}

/// `SI_spdlUseNamesrvImgRef (0x08580ec4)`. [soft 0x08580ec4 SI_spdlUseNamesrvImgRef]
pub fn si_spdl_use_namesrv_img_ref(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    env_flag(m, o, 0x086e_fa64, 0x086e_fdd8, 0x086e_fdda);
    s16(m, 0x086e_fdda)
}

/// `SI_isMr19Enable (0x08580d0c)`: whether the executable named by the environment variable exists.
/// [soft 0x08580d0c SI_isMr19Enable]
pub fn si_is_mr19_enable(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let env = o.call(m, ext::GETENV, &[0x086e_fdbf]).eax;
    let out = m.local(4);
    m.set_u32(out, 0);
    if env == 0 {
        return 0;
    }
    // strstr(env, " -") (the string at 0x086efdcc) and cut the string there
    let hay = m.cstr(env);
    if let Some(at) = hay.windows(2).position(|w| w == b" -") {
        m.set_u8(env + at as u32, 0);
    }
    let ok = o.call(m, ext::SI_MRFILE_GET_EXEC_FULLPATH, &[env, out]).eax as u16;
    if ok == 0 {
        return 0;
    }
    let p = m.u32(out);
    if p != 0 {
        m.free(p);
        m.set_u32(out, 0);
    }
    1
}

/// `displayMessages (0x08580c3c)`: a message to the status line, by the display mode and `si_IsIGLOO`.
/// [soft 0x08580c3c displayMessages]
pub fn display_messages(m: &mut Mem, o: &mut dyn Outside, mode: u32, text: u32) {
    let mut sel = (mode as u16 == 0) as u32;
    if o.call(m, ext::SI_IS_IGLOO, &[]).eax as u16 != 0 {
        sel |= 2;
    }
    match sel {
        0 | 2 => {
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[text, 1]);
        }
        1 => {
            let sys = m.u32(g::SI_SYSID);
            o.call(m, ext::WINSET, &[sys]);
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE, &[text, 1]);
            o.call(m, ext::GFLUSH, &[]);
        }
        3 => {
            o.call(m, ext::SI_STATUS_DISPLAY_MESSAGE_TO_FILE_ONLY, &[text, 1]);
        }
        _ => {}
    }
}

/// `findBlur2DShaderLib (0x08580684)`: looks the "Shader_Lib" database up once.
/// [soft 0x08580684 findBlur2DShaderLib]
pub fn find_blur2d_shader_lib(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    if m.i16(g::BLUR_DONE) != 0 {
        return (m.u32(g::BLUR2D_SHADER_LIB) != 0) as u32;
    }
    let l = m.local(8);
    m.set_u32(l, 0);
    let r = o.call(m, ext::SI_DB_DIR_SEARCH_DATABASE_FROM_NAME, &[g::SI_DBDIR, 0x086e_fa6e, l, l + 4]).eax;
    m.set_u16(g::BLUR_DONE, 1);
    let e = m.u32(l);
    if r == 0x64 && e != 0 && m.u32(e + 0x24) == 2 && m.u32(e + 0xc) != 0 {
        m.set_u32(g::BLUR2D_SHADER_LIB, e);
        return 1;
    }
    0
}

/// `buildFullPicFileName (0x085814a8)`: "name.suffix" as a new string.
/// [soft 0x085814a8 buildFullPicFileName]
pub fn build_full_pic_file_name(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, c: u32, d: u32, e: u32) -> u32 {
    let name = o.call(m, ext::SI_BUILD_PIC_FILE_NAME, &[a, b, s16_arg(c), d, 0]).eax;
    let suffix = o.call(m, ext::SI_GET_RENDER_IMG_SUFFIX, &[e]).eax;
    // sprintf("%s.%s"): a null string prints as "(null)" (glibc)
    let cs = |m: &Mem, p: u32| if p == 0 { b"(null)".to_vec() } else { m.cstr(p) };
    let mut text = cs(m, name);
    text.push(b'.');
    text.extend(cs(m, suffix));
    text.push(0);
    if name != 0 {
        m.free(name);
    }
    if suffix != 0 {
        m.free(suffix);
    }
    strdup_bytes(m, &text)
}

fn s16_arg(v: u32) -> u32 {
    v as u16 as i16 as i32 as u32
}

/// `strdup` of a buffer that holds a terminated string.
fn strdup_bytes(m: &mut Mem, text: &[u8]) -> u32 {
    let p = m.malloc(text.len() as u32);
    m.set_bytes(p, text);
    p
}

/// `_IsScanlineOff (0x08585aa4)`: whether a enabled user shader of the class carries flag 0x10000.
/// [soft 0x08585aa4 _IsScanlineOff]
pub fn is_scanline_off(m: &mut Mem, o: &mut dyn Outside, dict: u32, class: u32) -> u32 {
    let tab = m.u32(dict + 4).wrapping_add(class << 6);
    let n = m.i32(tab + 4);
    let buf = m.local(0x100);
    let mut i: i32 = 0;
    while i < n {
        let el = m.u32(tab + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        if m.i16(el + 0x48) == 0 {
            let udf = m.u32(el + 4);
            if udf != 0
                && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0
                && o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, buf, 0x100]).eax != 0
                && o.call(m, ext::SI_UDF_GET_SHADER_FLAGS, &[udf]).eax & 0x10000 != 0
            {
                return 1;
            }
        }
        i += 1;
    }
    0
}

/// `openMIfile (0x085859c4)`: opens the scene file or pipe; on failure tells the status line.
/// [soft 0x085859c4 openMIfile]
pub fn open_mi_file(m: &mut Mem, o: &mut dyn Outside, path: u32, rp: u32) -> u32 {
    let pipe = rp + self::rp::PIPE;
    if m.u32(pipe) != u32::MAX {
        o.print(2, b"ERROR: SCN_OUT is opened in translator!!!\n");
    }
    let kind = m.u32(g::CONNECTED_TO_NAMED_PIPE);
    if kind == 1 {
        return 1;
    }
    let size = if kind == 2 { 0x38 + 0xcc } else { 0x38 };
    if o.call(m, ext::SI_PIPEIO_FILE_OPEN, &[pipe, path, size]).eax as u16 != 0 {
        return 1;
    }
    let mut text = b"Cannot open mi file <".to_vec();
    // sprintf("%s") of a null string prints "(null)" (glibc)
    text.extend(if path == 0 { b"(null)".to_vec() } else { m.cstr(path) });
    text.extend(b">\0");
    let buf = m.local(0x1000);
    m.set_bytes(buf, &text);
    o.call(m, ext::SI_MUTEX_LOCK, &[pipe + 4]);
    m.set_u16(rp + self::rp::IN_MESSAGE, 1);
    display_messages(m, o, s16(m, rp + self::rp::DISPLAY_MODE), buf);
    o.call(m, ext::SI_MUTEX_UNLOCK, &[pipe + 4]);
    m.set_u16(rp + self::rp::IN_MESSAGE, 0);
    0
}

/// `cleanup_sequence_mental (0x08581460)`: the closing text and the pipe's end.
/// [soft 0x08581460 cleanup_sequence_mental]
pub fn cleanup_sequence_mental(m: &mut Mem, o: &mut dyn Outside, rp: u32) {
    if m.u32(rp + self::rp::OUT_MODE) != 3 || m.i16(rp + self::rp::NO_OUTPUT) == 0 {
        fprintf(m, o, &[0x086e_fe9d]);
        o.call(m, ext::SI_MRPIPE_FLUSH, &[]);
        o.call(m, ext::SI_PIPEIO_DESTROY, &[rp + self::rp::PIPE]);
    }
}

/// `disposeAllmovingCopy (0x08581cf8)`: frees the moving copies the models hold.
/// [soft 0x08581cf8 disposeAllmovingCopy]
pub fn dispose_all_moving_copy(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    if dict == 0 {
        return;
    }
    let tab = m.u32(dict + 4).wrapping_add(m.u32(g::SI_CID_MODELS) << 6);
    let n = m.i32(tab + 4);
    let mut i: i32 = 0;
    while i < n {
        let el = m.u32(tab + 8).wrapping_add((i as u32).wrapping_mul(0x84));
        if m.i16(el + 0x48) == 0 {
            let model = m.u32(el + 4);
            let rec = m.u32(model + 0x33c);
            if rec != 0 {
                let p = m.u32(rec + 0x22c);
                o.call(m, ext::SI_MEMTRACK_FREE, &[g::MEMTRACK_PARAMS, p]);
                m.set_u32(rec + 0x22c, 0);
                let p = m.u32(model + 0x33c);
                o.call(m, ext::SI_MEMTRACK_FREE, &[g::MEMTRACK_PARAMS, p]);
                m.set_u32(model + 0x33c, 0);
            }
        }
        i += 1;
    }
}

/// `SI_cleanupTranslator (0x08581c78)`. [soft 0x08581c78 SI_cleanupTranslator]
pub fn si_cleanup_translator(m: &mut Mem, o: &mut dyn Outside, rp: u32) {
    if m.i16(rp + self::rp::MBLUR) != 0 && (m.u32(rp + self::rp::OUT_MODE) != 0 || m.i16(rp + self::rp::MOVING) != 0) {
        let d = m.u32(rp);
        dispose_all_moving_copy(m, o, d);
    }
    let v = m.u32(g::SI_GBL_VERT);
    o.call(m, ext::SI_MEMTRACK_FREE, &[g::MEMTRACK_PARAMS, v]);
    m.set_u32(g::SI_GBL_VERT, 0);
    m.set_u32(g::MB_NB_MAX_VERT, 0);
    o.call(m, ext::DESTROY_2D_TEXTURE_LIST, &[]);
    if m.i16(g::MEMORY_CHECK) != 0 {
        o.call(m, ext::SI_MEMTRACK_CHECK, &[g::MEMTRACK_PARAMS]);
    }
}

/// `SI_closeLiveConnection (0x08585ba0)`. [soft 0x08585ba0 SI_closeLiveConnection]
pub fn si_close_live_connection(m: &mut Mem, o: &mut dyn Outside, rp: u32, arg: u32, f: u32) -> u32 {
    cleanup_sequence_mental(m, o, rp);
    si_cleanup_translator(m, o, rp);
    for off in [rp::SCENE_NAME, rp::FREE_A0, rp::FREE_610] {
        let p = m.u32(rp + off);
        if p != 0 {
            m.free(p);
            m.set_u32(rp + off, 0);
        }
    }
    if m.u32(rp) != 0 {
        o.call(m, ext::SI_ASSIGN_MAXCOC, &[s16(m, rp + rp::DISPLAY_MODE), f]);
        let d = m.u32(rp + rp::DICT);
        let a0 = m.u32(rp);
        o.call(m, ext::SI_DELETE_RENDER_DICTIONARY, &[d, a0, arg, 1]);
        let p = m.u32(rp);
        if p != 0 {
            m.free(p);
            m.set_u32(rp, 0);
        }
    }
    1
}

/// `SI_initLiveConnection (0x08585b54)`. [soft 0x08585b54 SI_initLiveConnection]
pub fn si_init_live_connection(m: &mut Mem, o: &mut dyn Outside, rp: u32, a: u32, b: u32, init_sequence: fn(&mut Mem, &mut dyn Outside, u32, u32) -> u32) -> u32 {
    if o.call(m, ext::SI_PREPARE_RENDERING, &[rp, a, b, 1]).eax == 0x7f {
        return 0;
    }
    o.call(m, ext::SI_INIT_MENTAL_PIPE_PARMS, &[rp]);
    (init_sequence(m, o, rp, 1) as u16 != 0) as u32
}

/// The value an x87-returning callee left in `st0`, as the 80-bit register.
fn st0(r: si_mem::Ret) -> si_x87::F80 {
    r.st0.map_or(si_x87::F80::ZERO, |b| si_x87::F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]])))
}

/// `printHeader (0x08580700)`: the `.mi` file's header for the first scene format: the comment lines, the
/// options, the motion-blur link and the `$include` of the standard declarations.
/// [soft 0x08580700 printHeader]
pub fn print_header(m: &mut Mem, o: &mut dyn Outside, rp: u32) {
    let blur = m.i16(rp + self::rp::MBLUR) != 0 && m.i16(rp + self::rp::MBLUR_MODE) == 1;
    let t = m.local(4);
    let now = o.call(m, ext::TIME, &[0]).eax;
    m.set_u32(t, now);
    let when = o.call(m, ext::CTIME, &[t]).eax;
    let scene = m.u32(rp + self::rp::SCENE_NAME);
    fprintf(m, o, &[0x086e_fa80, scene, when]);
    let verbose = if m.u32(g::VERBOSE_FLAG) != 0 { 0x086e_faac } else { 0x086e_faaf };
    fprintf(m, o, &[0x086e_fab3, verbose]);
    let opts = m.u32(rp + self::rp::RAY_OPTIONS);
    if opts != 0 {
        fprintf(m, o, &[0x086e_fabf, opts]);
    }
    fprintf(m, o, &[0x086e_fac5]);
    o.call(m, ext::SI_UDF_GENERATE_CODES, &[]);
    o.call(m, ext::SI_UDF_GENERATE_LINKS, &[]);
    if blur {
        let lib = m.u32(g::BLUR2D_SHADER_LIB);
        let name = m.u32(lib + 0xc);
        fprintf(m, o, &[0x086e_fac7, name]);
    }
    o.call(m, ext::SI_UDF_GENERATE_DECLARES, &[0]);
    let resdir = m.u32(g::SI_RESDIR);
    let buf = m.local(0x2000);
    if resdir != 0 {
        include_line(m, o, buf, resdir, b"/softimage.mi", 0x086e_faf4);
        if blur {
            include_line(m, o, buf, resdir, b"/mb.mi", 0x086e_fb0c);
        }
    } else {
        fprintf(m, o, &[0x086e_fb1b]);
        if blur {
            fprintf(m, o, &[0x086e_fb34]);
        }
    }
    let trailer = m.u32(rp + self::rp::TRAILER);
    if trailer != 0 {
        fprintf(m, o, &[0x086e_fb46, trailer]);
    }
}

/// `sprintf(buf, "%s<tail>", dir)`, `SI_mrfileConvertToUnix(buf, buf, 0x2000)` and the `$include` line.
fn include_line(m: &mut Mem, o: &mut dyn Outside, buf: u32, dir: u32, tail: &[u8], fmt: u32) {
    let mut text = m.cstr(dir);
    text.extend_from_slice(tail);
    text.push(0);
    m.set_bytes(buf, &text);
    o.call(m, ext::SI_MRFILE_CONVERT_TO_UNIX, &[buf, buf, 0x2000]);
    fprintf(m, o, &[fmt, buf]);
}

/// `print_scene (0x08581dd4)`: one frame's body: the texture and light definitions, the view, the
/// materials and the models.
/// [soft 0x08581dd4 print_scene]
pub fn print_scene(m: &mut Mem, o: &mut dyn Outside, rp: u32, cam: u32, view_arg: u32, hide: u32, extra: u32) {
    let hide = hide as u16 as i16 as i32 as u32;
    let extra = extra as u16 as i16 as i32 as u32;
    let dict = m.u32(rp);
    let l = m.local(0x80);
    let (mat1, mat2) = (l, l + 0x40);
    o.call(m, ext::INIT_MATRIX, &[mat1]);
    o.call(m, ext::INIT_MATRIX, &[mat2]);
    let one = 1.0f32.to_bits();
    o.call(m, ext::SI_ROTATE_WORLD, &[cam, mat1, mat2]);
    if hide as u16 == 0 {
        let frame = m.u32(g::FRAME);
        let sec = st0(o.call(m, ext::SI_FRAME_TO_SEC, &[frame])).to_f64().to_bits();
        fprintf(m, o, &[0x086f_0317, frame, sec as u32, (sec >> 32) as u32]);
        o.call(m, ext::INIT_MATERIALS, &[dict]);
        let frame = m.u32(g::FRAME);
        o.call(m, ext::SI_UDF_GENERATE_TEXTURE_DEFS, &[frame, 0]);
        o.call(m, ext::SI_MUTEX_LOCK, &[rp + self::rp::MUTEX]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 1);
        let cid = m.u32(g::CID_TEXTURES2D);
        o.call(m, ext::SI_ELEMLIST_LOW_BYTE_WORK_FLAG_AND, &[dict, cid, 1]);
        let tex_mode = s16(m, rp + self::rp::TEX_MODE);
        let two = if texture_list_kept(m, rp) { 0 } else { 2 };
        o.call(m, ext::PRINT_2D_TEXTURE_MAP, &[dict, tex_mode, two, 0, 0, 0, rp]);
        o.call(m, ext::SI_MUTEX_UNLOCK, &[rp + self::rp::MUTEX]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 0);
        let on = (m.u32(rp + self::rp::OUT_MODE) == 0) as u32;
        o.call(m, ext::PRINT_LIGHT_DEFS, &[dict, mat1, 0, on]);
        let tex_mode = s16(m, rp + self::rp::TEX_MODE);
        o.call(m, ext::PRINT_2D_TEXTURE_DEFS, &[dict, tex_mode]);
        o.call(m, ext::PRINT_VIEW, &[rp, cam, mat1, view_arg]);
        let tex_mode = s16(m, rp + self::rp::TEX_MODE);
        o.call(m, ext::PRINT_ALL_MATERIAL, &[dict, tex_mode]);
    }
    let tex_mode = s16(m, rp + self::rp::TEX_MODE);
    o.call(m, ext::PRINT_ALL_MODELS, &[dict, mat1, mat2, one, one, one, tex_mode, hide, extra]);
    if hide as u16 == 0 {
        fprintf(m, o, &[0x086f_0325]);
        if texture_list_kept(m, rp) {
            o.call(m, ext::DESTROY_2D_TEXTURE_LIST, &[]);
        } else {
            o.call(m, ext::CLEANUP_2D_TEXTURE_LIST, &[]);
        }
    }
}

/// The test `print_scene` makes twice: the 2D texture list is not kept (`MI_Optimize` set, or the
/// translator is in mode 3 with the flag at 0x08790fba set).
fn texture_list_kept(m: &Mem, rp: u32) -> bool {
    m.u32(g::MI_OPTIMIZE) != 0 || (m.u32(rp + self::rp::OUT_MODE) == 3 && m.i16(g::W_8790FBA) != 0)
}

/// The `mrcamera` record `SI_init_MRCamera` fills: five floats (the lens and film numbers the `.mi` camera needs).
/// [soft 0x08580500 SI_init_MRCamera]: `elem` is the camera element (its record at `+4`), `rc` the render control, `out` the five floats.
/// The arithmetic is the x87's: sums, products and quotients are kept in 80 bits until they are stored as floats; `tan` is the C library's.
pub fn si_init_mrcamera(m: &mut Mem, _dict: u32, elem: u32, rc: u32, out: u32) {
    use si_x87::{trig, F80};
    if elem == 0 {
        return;
    }
    let cam = m.u32(elem + 4);
    if cam == 0 || rc == 0 || out == 0 {
        return;
    }
    let half = 0.5f32;
    // the camera's +0x70 is raised to 1.0 when 1.0 > it; +0x40 is raised to 1e-4 (as a float) when it is below 0.0001
    if F80::ONE.gt(F80::from_f32(m.f32(cam + 0x70))) {
        m.set_f32(cam + 0x70, 1.0);
    }
    if F80::from_f32(m.f32(cam + 0x40)).lt(F80::from_f64(0.0001)) {
        m.set_f32(cam + 0x40, f32::from_bits(0x38d1_b717));
    }
    // rc+0x10c is pushed at least 1e-6 above rc+0x104
    let (a, b) = (m.f32(rc + 0x104), m.f32(rc + 0x10c));
    let diff = F80::from_f32(b) - F80::from_f32(a);
    if F80::from_f64(1e-6).gt(diff) {
        m.set_f32(rc + 0x10c, (F80::from_f32(a) + 1e-6f64).to_f32());
    }
    // the three window numbers: a zero is replaced by the other field (a value copied through the register: a signalling NaN comes out quiet)
    let pick = |m: &Mem, v: u32, alt: u32| -> f32 {
        let x = m.f32(v);
        F80::from_f32(if F80::from_f32(x).eq(F80::ZERO) { m.f32(alt) } else { x }).to_f32()
    };
    let o0 = pick(m, rc + 0x108, rc + 0x104);
    m.set_f32(out, o0);
    let o1 = pick(m, rc + 0x110, rc + 0x10c);
    m.set_f32(out + 4, o1);
    let o2 = pick(m, rc + 0x114, rc + 0x130);
    m.set_f32(out + 8, o2);
    let mode = m.u32(cam + 0x6c);
    let (c40, c70, c74, c7c, c84, c94, c98) = (m.f32(cam + 0x40), m.f32(cam + 0x70), m.f32(cam + 0x74), m.f32(cam + 0x7c), m.f32(cam + 0x84), m.f32(cam + 0x94), m.f32(cam + 0x98));
    if mode == 2 {
        let t = trig::tan((F80::from_f32(c40) * half).to_f64());
        let h = F80::from_i32(m.i16(rc + 0x59a) as i32);
        let w = F80::from_i32(m.i16(rc + 0x59c) as i32);
        let aa = (F80::from_f32(c70) + F80::from_f32(c70)) * h / w;
        let n = -(F80::from_f32(c84) + c7c);
        let at = aa * t;
        m.set_f32(out + 0xc, (n * 0.5f64).to_f32());
        let q = F80::from_f32(c98);
        let q = q / h;
        m.set_f32(out + 0x10, ((at * m.f32(rc + 0x154)) * q).to_f32());
    } else if mode == 1 {
        m.set_f32(out + 0xc, (-F80::from_f32(c74)).to_f32());
        let v = F80::from_f32(c70) * half / c94;
        m.set_f32(out + 0x10, (0.0032834f64 * v).to_f32());
    }
}

/// `sprintf(buf, fmt, dir)` with a `%s` conversion only: a null string prints as "(null)" (glibc).
fn sprintf_s(m: &mut Mem, buf: u32, pre: &[u8], dir: u32, tail: &[u8]) {
    let mut text = pre.to_vec();
    text.extend_from_slice(&if dir == 0 { b"(null)".to_vec() } else { m.cstr(dir) });
    text.extend_from_slice(tail);
    text.push(0);
    m.set_bytes(buf, &text);
}

/// `mi2_printHeader (0x085808ac)`: the header of the second scene format (`.mi2`): the comment lines, the options, the
/// declarations read from the resource directory's `softimage.mi2` (through the user-defined-shader code), the `$include` lines
/// or the `soft_GUIDs.rsrc` file copied through the pipe, and the forward references of the models.
/// The files are opened through `fopen`/`fgets`/`fclose` of the C library (answered by the caller).
/// [soft 0x085808ac mi2_printHeader]
pub fn mi2_print_header(m: &mut Mem, o: &mut dyn Outside, rp: u32) {
    let blur = m.i16(rp + self::rp::MBLUR) != 0 && m.i16(rp + self::rp::MBLUR_MODE) == 1;
    let n = m.u32(g::NFRAMES_IN_MIFILE);
    fprintf(m, o, &[0x086e_fb64, n]);
    let t = m.local(4);
    let now = o.call(m, ext::TIME, &[0]).eax;
    m.set_u32(t, now);
    let when = o.call(m, ext::CTIME, &[t]).eax;
    let scene = m.u32(rp + self::rp::SCENE_NAME);
    fprintf(m, o, &[0x086e_fb80, scene, when]);
    let (b, e, st) = (s16(m, rp + rp::FRAME_START), s16(m, rp + rp::FRAME_END), s16(m, rp + rp::FRAME_STEP));
    fprintf(m, o, &[0x086e_fbac, b]);
    fprintf(m, o, &[0x086e_fbbe, e]);
    fprintf(m, o, &[0x086e_fbce, st]);
    if m.i16(rp + self::rp::LIVE_FLAG) != 0 {
        fprintf(m, o, &[0x086e_fbdf, 3]);
    } else {
        fprintf(m, o, &[0x086e_fbf1]);
    }
    fprintf(m, o, &[0x086e_fc45, 0x086e_fc20]);
    fprintf(m, o, &[0x086e_fc5c, scene]);
    let verbose = if m.u32(g::VERBOSE_FLAG) != 0 { 0x086e_fc73 } else { 0x086e_fc76 };
    fprintf(m, o, &[0x086e_fc7a, verbose]);
    let opts = m.u32(rp + self::rp::RAY_OPTIONS);
    if opts != 0 {
        fprintf(m, o, &[0x086e_fc86, opts]);
    }
    fprintf(m, o, &[0x086e_fc8c]);
    o.call(m, ext::SI_UDF_GENERATE_CODES, &[]);
    o.call(m, ext::SI_UDF_GENERATE_LINKS, &[]);
    if blur && si_spdl_use_namesrv_shd_decl(m, o) as u16 == 0 {
        let lib = m.u32(g::BLUR2D_SHADER_LIB);
        let name = m.u32(lib + 0xc);
        fprintf(m, o, &[0x086e_fca0, name]);
    }
    let resdir = m.u32(g::SI_RESDIR);
    let (path, inc1, inc_mb, line) = (m.local(0x2000), m.local(0x2000), m.local(0x2000), m.local(0x200));
    // the mode strings "r" (0x086efce0, 0x086efd5c) are in the program's read-only data, which this image does not hold
    let mode_r = m.local(4);
    m.set_bytes(mode_r, b"r\0");
    // the declarations file
    if resdir != 0 {
        sprintf_s(m, path, b"", resdir, b"/softimage.mi2");
    } else {
        m.set_bytes(path, b"./softimage.mi2\0");
    }
    let f = o.call(m, ext::FOPEN, &[path, mode_r]).eax;
    o.call(m, ext::SI_UDF_GENERATE_DECLARES, &[f]);
    o.call(m, ext::FCLOSE, &[f]);
    if resdir != 0 {
        o.call(m, ext::SI_MRFILE_CONVERT_TO_UNIX, &[path, path, 0x2000]);
        sprintf_s(m, inc1, b"$include \"", path, b"\"\n");
        if blur {
            sprintf_s(m, path, b"", resdir, b"/mb.mi2");
            o.call(m, ext::SI_MRFILE_CONVERT_TO_UNIX, &[path, path, 0x2000]);
            sprintf_s(m, inc_mb, b"$include \"", path, b"\"\n");
        }
    } else {
        m.set_bytes(inc1, b"$include <softimage.mi2>\n\0");
        if blur {
            m.set_bytes(inc_mb, b"$include <mb.mi2>\n\0");
        }
    }
    if si_spdl_use_namesrv_shd_decl(m, o) as u16 != 0 {
        if resdir != 0 {
            sprintf_s(m, path, b"", resdir, b"/soft_GUIDs.rsrc");
        } else {
            m.set_bytes(path, b"./soft_GUIDs.rsrc\0");
        }
        let f = o.call(m, ext::FOPEN, &[path, mode_r]).eax;
        if f != 0 {
            while o.call(m, ext::FGETS, &[line, 0x1f4, f]).eax != 0 {
                fprintf(m, o, &[line]);
            }
            o.call(m, ext::FCLOSE, &[f]);
        }
    } else {
        fprintf(m, o, &[inc1]);
        if blur {
            fprintf(m, o, &[inc_mb]);
        }
    }
    let trailer = m.u32(rp + self::rp::TRAILER);
    if trailer != 0 {
        fprintf(m, o, &[0x086e_fd5e, trailer]);
    }
    fprintf(m, o, &[0x086e_fd80]);
    let d = m.u32(rp);
    o.call(m, ext::SI_MI2_MODEL_FORWARD_DEFS, &[d]);
    fprintf(m, o, &[0x086e_fda5]);
}

/// `init_sequence_mental (0x08580f48)`: the start of a translation: the debugging switches from the environment (once), the
/// copies of the render control and database records, the frame counters, the output file and the `.mi`/`.mi2` header, and the
/// definitions of the 2D textures. Answers 0 when the output file cannot be opened, else 1. `_flag` is the second argument the
/// callers pass (1); the original does not read it.
/// A frame step of 0 would trap in the original's `idiv`; here it divides as `checked_div` and leaves the count 0.
/// [soft 0x08580f48 init_sequence_mental]
pub fn init_sequence_mental(m: &mut Mem, o: &mut dyn Outside, rp: u32, _flag: u32) -> u32 {
    const DID_GETENV: u32 = 0x086e_fddc;
    if m.i16(DID_GETENV) == 0 {
        let p = o.call(m, ext::GETENV, &[0x086e_fdde]).eax;
        m.set_u16(g::MEMORY_CHECK, (p != 0) as u16);
        let p = o.call(m, ext::GETENV, &[0x086e_fdf1]).eax;
        m.set_u16(g::RENDER_DEBUG, (p != 0) as u16);
        if m.i16(g::MI2_OUTPUT) == 0 {
            let p = o.call(m, ext::GETENV, &[0x086e_fdfe]).eax;
            m.set_u32(g::MI_OPTIMIZE, p);
        }
        m.set_u16(DID_GETENV, 1);
    }
    let pipe = rp + self::rp::PIPE;
    let stdout = m.u32(g::STDOUT);
    m.set_u32(g::OUT, stdout);
    o.call(m, ext::SI_MEMTRACK_INITIALIZE, &[g::MEMTRACK_PARAMS]);
    o.call(m, ext::SI_INIT_TXT_SEQ_COUNTER, &[]);
    if m.i16(g::RENDER_DEBUG) != 0 {
        o.print(2, b"\n\n --- Rendering dictionary --- \n\n");
        let d = m.u32(rp);
        o.call(m, ext::SI_ELEMDICT_DUMP, &[d]);
    }
    if m.i16(rp + self::rp::MBLUR) != 0 && m.i16(rp + self::rp::MBLUR_MODE) == 1 && find_blur2d_shader_lib(m, o) as u16 == 0 {
        m.set_u16(rp + self::rp::MBLUR, 0);
        let buf = m.local(0x200);
        let mut text = b"Cannot find 2D motion blur shader lib. Motion blur disabled.".to_vec();
        text.push(0);
        m.set_bytes(buf, &text);
        o.call(m, ext::SI_MUTEX_LOCK, &[pipe + 4]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 1);
        display_messages(m, o, s16(m, rp + self::rp::DISPLAY_MODE), buf);
        o.call(m, ext::SI_MUTEX_UNLOCK, &[pipe + 4]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 0);
    }
    m.copy(g::SI_REN, rp + 0xc, 0x600);
    let verbose = m.u32(rp + 0x618);
    m.set_u32(g::VERBOSE_FLAG, verbose);
    let src = m.u32(m.u32(rp + self::rp::FREE_610));
    m.copy(g::SI_DATABASE, src, 0x13c);
    m.set_u16(g::RENDER_ORTHO, (m.i16(g::SI_DATABASE + 4) != 0x7d3) as u16);
    let pnb = o.call(m, ext::SI_PERSPECTIVE_NOT_B_VIEW, &[]).eax;
    m.set_u16(g::PERSPECTIVE_NOT_B, pnb as u16);
    let (start, end, step) = (s16(m, rp + rp::FRAME_START) as i32, s16(m, rp + rp::FRAME_END) as i32, s16(m, rp + rp::FRAME_STEP) as i32);
    m.set_u32(g::FRAME, start as u32);
    m.set_u32(g::NFRAMES, (end - start + 1) as u32);
    m.set_u32(g::FRAMEINC, step as u32);
    m.set_u32(g::INCREMENTAL, 0);
    if m.i16(g::W_879107A) != 0 {
        let v = o.call(m, ext::SI_MEMTRACK_ALLOCATE, &[g::MEMTRACK_PARAMS, 0x5cc60]).eax;
        m.set_u32(g::SI_GBL_VERT, v);
        m.set_u32(g::MB_NB_MAX_VERT, 0x1388);
    }
    m.set_u32(g::D_87907E8, 0);
    let live = m.u32(rp + self::rp::OUT_MODE) == 3;
    if live && m.i16(rp + self::rp::NO_OUTPUT) == 0 {
        let path = m.u32(g::SCN_OUT_NAME);
        if open_mi_file(m, o, path, rp) as u16 == 0 {
            return 0;
        }
    }
    if live && m.i16(rp + self::rp::NO_OUTPUT) != 0 {
        m.set_u32(g::NFRAMES_IN_MIFILE, 1);
    } else {
        let n = (end - start).checked_div(step).map_or(0, |q| q.wrapping_add(1));
        m.set_u32(g::NFRAMES_IN_MIFILE, n as u32);
        let (a, b) = (m.u32(rp + self::rp::PIPE), pipe);
        o.call(m, ext::SI_MRPIPE_SET, &[a, b + 0x16, b + 0x2c]);
        if m.i16(g::MI2_OUTPUT) != 0 {
            mi2_print_header(m, o, rp);
        } else {
            print_header(m, o, rp);
        }
    }
    let frame = m.u32(g::FRAME);
    m.set_u32(g::SI_FRAME_CURRENT, frame);
    if (live && m.i16(g::W_8790FBA) != 0) || m.u32(g::MI_OPTIMIZE) != 0 {
        return 1;
    }
    o.call(m, ext::SI_MUTEX_LOCK, &[pipe + 4]);
    m.set_u16(pipe + 0x14, 1);
    let mode = s16(m, rp + self::rp::TEX_MODE);
    let d = m.u32(rp);
    o.call(m, ext::PRINT_2D_TEXTURE_MAP, &[d, mode, 1, start as u32, end as u32, step as u32, rp]);
    o.call(m, ext::SI_MUTEX_UNLOCK, &[pipe + 4]);
    m.set_u16(pipe + 0x14, 0);
    1
}

/// `si_rotateWorld (0x085823cc)`: the camera's view matrices. `mat1` gets translate(-eye) * look-at basis * roll, `mat2` the same
/// without the translation; a nonzero camera flag at +0x108 multiplies both by a scale of the two aspect ratios (+0x118, +0x11c)
/// over +0x10c. The mi_calc routines (`normalize_vector`, `vector_X_vector`, `init_matrix`, `rotate_matrix`, `matrix_X_matrix`) run
/// here as ported. The debug line goes to `sdebug` with the format address and seven doubles.
/// [soft 0x085823cc si_rotateWorld]
pub fn si_rotate_world(m: &mut Mem, o: &mut dyn Outside, cam: u32, mat1: u32, mat2: u32) {
    use crate::mi_calc as c;
    use si_x87::F80;
    let top = m.sp();
    let f = m.local(0x210);
    let l = |k: u32| f + 0x210 - k;
    let rf = |m: &Mem, off: u32| f32::from_bits(m.u32(cam + off));
    let mut args = vec![0x086f_03e0u32];
    for off in [0x10u32, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x30] {
        let b = (rf(m, off) as f64).to_bits();
        args.push(b as u32);
        args.push((b >> 32) as u32);
    }
    o.call(m, c::SDEBUG, &args);
    for (k, off) in [(0xcu32, 0x10u32), (8, 0x14), (4, 0x18)] {
        let v = m.u32(cam + off);
        m.set_u32(l(k), v);
    }
    let ld = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(cam + off)));
    let d0 = ld(m, 0x10) - ld(m, 0x1c);
    m.set_f32(l(0x30), d0.to_f32());
    let d1 = ld(m, 0x14) - ld(m, 0x20);
    m.set_f32(l(0x2c), d1.to_f32());
    let d2 = ld(m, 0x18) - ld(m, 0x24);
    m.set_u32(l(0x24), 0);
    m.set_u32(l(0x20), 0x3f80_0000);
    m.set_u32(l(0x1c), 0);
    m.set_f32(l(0x28), d2.to_f32());
    let zero = F80::from_f32(0.0);
    if d0.eq(zero) && d1.eq(zero) && d2.eq(zero) {
        m.set_f32(l(0x28), -1.0);
    }
    c::normalize_vector(m, o, l(0x30));
    c::vector_x_vector(m, l(0x18), l(0x24), l(0x30));
    let z = |m: &Mem, k: u32| F80::from_f32(m.f32(l(k))).eq(zero);
    if z(m, 0x18) && z(m, 0x14) && z(m, 0x10) {
        m.set_u32(l(0x18), 0x3f80_0000);
    }
    c::normalize_vector(m, o, l(0x18));
    c::vector_x_vector(m, l(0x24), l(0x30), l(0x18));
    let (r, t, rot, tr) = (l(0xf0), l(0x70), l(0xb0), l(0x130));
    let rows: [[Option<u32>; 4]; 4] = [
        [Some(0x18), Some(0x24), Some(0x30), None],
        [Some(0x14), Some(0x20), Some(0x2c), None],
        [Some(0x10), Some(0x1c), Some(0x28), None],
        [None, None, None, None],
    ];
    for (i, row) in rows.iter().enumerate() {
        for (j, src) in row.iter().enumerate() {
            let v = match src {
                Some(k) => m.u32(l(*k)),
                None if i == 3 && j == 3 => 0x3f80_0000,
                None => 0,
            };
            m.set_u32(r + 16 * i as u32 + 4 * j as u32, v);
        }
    }
    c::init_matrix(m, t);
    for (k, off) in [(0x40u32, 0x10u32), (0x3c, 0x14), (0x38, 0x18)] {
        m.set_f32(l(k), -rf(m, off));
    }
    c::matrix_x_matrix(m, tr, t, r);
    c::init_matrix(m, rot);
    m.set_f32(l(0xc), -rf(m, 0x2c));
    m.set_f32(l(8), -rf(m, 0x28));
    m.set_f32(l(4), -rf(m, 0x30));
    c::rotate_matrix(m, rot, l(0xc));
    if m.u16(cam + 0x108) != 0 {
        let (rr, trr, s) = (l(0x200), l(0x1c0), l(0x180));
        c::matrix_x_matrix(m, rr, r, rot);
        c::matrix_x_matrix(m, trr, tr, rot);
        c::init_matrix(m, s);
        let div = ld(m, 0x10c);
        m.set_f32(l(0x160), (ld(m, 0x118) / div).to_f32());
        m.set_f32(l(0x15c), (ld(m, 0x11c) / div).to_f32());
        c::matrix_x_matrix(m, mat2, rr, s);
        c::matrix_x_matrix(m, mat1, trr, s);
    } else {
        c::matrix_x_matrix(m, mat2, r, rot);
        c::matrix_x_matrix(m, mat1, tr, rot);
    }
    m.set_sp(top);
}

/// `sequence_mental (0x08581310)`: the frames of one translation: `init_sequence_mental`, an extra leading frame (one step back) when
/// motion blur is on and the scene moves, then every frame of the range, once or twice per frame, and `cleanup_sequence_mental`.
/// [soft 0x08581310 sequence_mental]
pub fn sequence_mental(m: &mut Mem, o: &mut dyn Outside, rp: u32) {
    if o.call(m, ext::INIT_SEQUENCE_MENTAL, &[rp, 0]).eax as u16 == 0 {
        return;
    }
    let pipe = rp + self::rp::PIPE;
    let passes: i32 = if m.u16(g::W_8790D1C) != 0 { 2 } else { 1 };
    let mut flag: u16 = 0;
    if m.u16(rp + self::rp::MBLUR) != 0 && (m.u32(rp + 0x654) != 0 || m.u16(rp + self::rp::MOVING) != 0) {
        flag = 1;
    }
    let inc = m.u32(g::FRAMEINC);
    if flag != 0 {
        let frame = m.u32(g::FRAME).wrapping_sub(inc);
        m.set_u32(g::FRAME, frame);
        o.call(m, ext::PRINT_FRAME, &[rp, frame, 0, 0, (passes - 1) as u32, 1, flag as i16 as i32 as u32]);
        let inc = m.u32(g::FRAMEINC);
        let frame = m.u32(g::FRAME).wrapping_add(inc);
        m.set_u32(g::FRAME, frame);
    }
    let mut i: i32 = 0;
    let mut n: i32 = 0;
    while i < m.u32(g::NFRAMES) as i32 {
        let next = n + 1;
        for pass in 0..passes {
            m.set_u32(pipe + 0x30, 0);
            let frame = m.u32(g::FRAME);
            o.call(m, ext::PRINT_FRAME, &[rp, frame, i as u32, n as u32, pass as u32, 0, flag as i16 as i32 as u32]);
            let p = m.u32(pipe + 0x30);
            if p != 0 {
                m.free(p);
                m.set_u32(pipe + 0x30, 0);
            }
        }
        let inc = m.u32(g::FRAMEINC);
        i = i.wrapping_add(inc as i32);
        let frame = m.u32(g::FRAME).wrapping_add(inc);
        m.set_u32(g::FRAME, frame);
        n = next;
    }
    o.call(m, ext::CLEANUP_SEQUENCE_MENTAL, &[rp]);
}

/// `mi2_print_view (0x085855c8)`: `print_option`, then `print_camera`.
/// [soft 0x085855c8 mi2_print_view]
pub fn mi2_print_view(m: &mut Mem, o: &mut dyn Outside, rp: u32, a: u32, b: u32, c: u32) {
    o.call(m, ext::PRINT_OPTION, &[rp, a]);
    o.call(m, ext::PRINT_CAMERA, &[rp, a, b, c]);
}

/// `sprintf(buf, fmt, words..)` into the image (the C library's formatting, [`crate::cprintf`]).
fn sprintf_words(m: &mut Mem, buf: u32, fmt: u32, words: &[u32]) {
    let text = crate::cprintf::format(m, fmt, words);
    m.put_cstr(buf, &text);
}

/// `print_frame (0x08581530)`: one frame (or one field of it) of the scene: the per-frame file and its semaphore, the header
/// of a per-frame `.mi` file, the status line, the scene update, the pre- and post-frame scripts, the scene body, and the
/// wait for the renderer to finish the file. `p6` is the "extra leading frame" flag, `p7` the motion-blur flag (both 16 bits).
/// The strings behind the global pointers `strMsgFrameCount`/`strMsgField` are read from the image.
/// [soft 0x08581530 print_frame]
#[allow(clippy::too_many_arguments)]
pub fn print_frame(m: &mut Mem, o: &mut dyn Outside, rp: u32, frame: u32, p3: u32, p4: u32, p5: u32, p6: u32, p7: u32) {
    let (p6, p7) = (p6 as u16, p7 as u16);
    m.set_u32(g::FRAME, frame);
    let dict = m.u32(rp);
    let pipe = rp + self::rp::PIPE;
    let buf = m.local(0x600);
    let path = buf; // ebp-0x600, 0x400 bytes
    let msg = buf + 0x400; // ebp-0x200
    let mode3 = m.u32(rp + self::rp::OUT_MODE) == 3;
    let per_frame = m.i16(g::W_8790FBA) != 0;
    let mut name = 0u32; // the picture's file name (local_630)
    let mut fd = 0u32; // the semaphore file (local_62c)
    let mut saved = 0u32; // the scene file name to put back at the end (local_624)
    let field = |m: &Mem| if m.i16(g::W_8790D1C) != 0 { p5.wrapping_add(1) } else { u32::MAX };
    let numbered = (m.i16(W_8790DD4) as i32 as u32).wrapping_add(p3);
    if p6 == 0 {
        if m.i16(W_8790DEE) != 0 {
            let mut skip = false;
            if mode3 {
                if !per_frame {
                    skip = true;
                } else {
                    let f = field(m);
                    let cur = m.u32(g::SCN_OUT_NAME);
                    name = o.call(m, ext::SI_GENERATE_MI_PER_FRAME_FILE_NAME, &[cur, 1, numbered, f]).eax;
                }
            } else {
                let a = m.u32(rp + 0x188);
                let c = s16(m, rp + 0x148);
                let e = m.u32(rp + 0x498);
                name = o.call(m, ext::BUILD_FULL_PIC_FILE_NAME, &[a, frame, c, p5.wrapping_add(1), e]).eax;
            }
            if !skip {
                sprintf_words(m, path, 0x086e_feb3, &[name]);
                fd = o.call(m, ext::CREAT, &[path, 0]).eax;
                if fd == u32::MAX {
                    return;
                }
                if o.call(m, ext::ACCESS, &[name, 0]).eax == 0 {
                    o.call(m, ext::CLOSE, &[fd]);
                    o.call(m, ext::UNLINK, &[path]);
                    return;
                }
                let dup = si_mem::clib::strdup(m, name);
                m.set_u32(pipe + 0x30, dup);
                m.set_u32(pipe + 0x34, fd);
            }
        }
        if mode3 && per_frame {
            saved = m.u32(g::SCN_OUT_NAME);
            let f = field(m);
            let new = o.call(m, ext::SI_GENERATE_MI_PER_FRAME_FILE_NAME, &[saved, 1, numbered, f]).eax;
            m.set_u32(g::SCN_OUT_NAME, new);
            if new == 0 {
                o.print(2, b"Memory error\n");
                return;
            }
            if o.call(m, ext::OPEN_MI_FILE, &[new, rp]).eax as u16 == 0 {
                return;
            }
            let first = m.u32(rp + self::rp::PIPE);
            o.call(m, ext::SI_MRPIPE_SET, &[first, rp + 0x642, rp + 0x658]);
            if m.i16(g::MI2_OUTPUT) != 0 {
                o.call(m, ext::MI2_PRINT_HEADER, &[rp]);
            } else {
                o.call(m, ext::PRINT_HEADER, &[rp]);
            }
        }
    }
    // the time of the frame: (frame + field * 0.5) / SI_frameRate, in the x87's order, stored as a float
    use si_x87::F80;
    let t = ((F80::from_i32(frame as i32) + F80::from_i32(p5 as i32) * 0.5f64) / F80::from_f32(m.f32(SI_FRAME_RATE))).to_f32();
    let secs = (t as f64).to_bits();
    o.call(m, ext::SDEBUG, &[0x086e_fee0, frame, secs as u32, (secs >> 32) as u32]);
    m.set_u32(g::SI_FRAME_CURRENT, frame);
    m.set_u16(W_8790DDA, frame as u16);
    if m.i16(rp + 0x642) != 0 {
        o.call(m, ext::SI_MRTHREAD_EXIT, &[0]);
    }
    let next = p5.wrapping_add(1);
    if m.i16(g::W_8790D1C) != 0 {
        let fmt = m.u32(STR_MSG_FIELD);
        sprintf_words(m, msg, fmt, &[frame, next]);
    } else {
        let fmt = m.u32(STR_MSG_FRAME_COUNT);
        sprintf_words(m, msg, fmt, &[frame]);
    }
    let mutex = rp + self::rp::MUTEX;
    o.call(m, ext::SI_MUTEX_LOCK, &[mutex]);
    m.set_u16(rp + self::rp::IN_MESSAGE, 1);
    let dm = s16(m, rp + self::rp::DISPLAY_MODE);
    o.call(m, ext::DISPLAY_MESSAGES, &[dm, msg]);
    o.call(m, ext::SI_MUTEX_UNLOCK, &[mutex]);
    m.set_u16(rp + self::rp::IN_MESSAGE, 0);
    if p7 != 0 || m.u32(rp + self::rp::OUT_MODE) != 0 {
        o.call(m, ext::SI_MUTEX_LOCK, &[mutex]);
        let old = m.u16(STANDALONE_RENDERER);
        m.set_u16(rp + self::rp::IN_MESSAGE, 1);
        m.set_u16(STANDALONE_RENDERER, 1);
        let d4 = m.u32(rp + 4);
        o.call(m, ext::SI_RENDER_SCENE_UPDATE, &[t.to_bits(), g::SI_DATABASE, d4, dict]);
        if m.i16(g::RENDER_ORTHO) != 0 || m.i16(g::PERSPECTIVE_NOT_B) != 0 {
            let src = m.u32(m.u32(rp + self::rp::FREE_610));
            m.copy(g::SI_DATABASE, src, 0x13c);
        }
        m.set_u16(STANDALONE_RENDERER, old);
        o.call(m, ext::SI_MUTEX_UNLOCK, &[mutex]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 0);
    }
    o.call(m, ext::SI_MUTEX_LOCK, &[mutex]);
    m.set_u16(rp + self::rp::IN_MESSAGE, 1);
    o.call(m, ext::SI_CAMERA, &[]);
    o.call(m, ext::SI_PROCESS_TEXTURES, &[dict, 0]);
    o.call(m, ext::SI_PROCESS_MATERIALS, &[dict]);
    o.call(m, ext::SDEBUG, &[0x086e_ff08]);
    o.call(m, ext::SI_MUTEX_UNLOCK, &[mutex]);
    m.set_u16(rp + self::rp::IN_MESSAGE, 0);
    let script = |m: &mut Mem, o: &mut dyn Outside, var: u32, sd: u32, pf: u32| {
        let s = m.u32(var);
        if s != 0 && m.u8(s) != 0 {
            o.call(m, ext::SDEBUG, &[sd, s]);
            let (a, b) = (m.u32(SCRIPT_ARG), (m.i16(W_8790DD4) as i32 as u32).wrapping_add(p3));
            fprintf(m, o, &[pf, s, a, b, p4, next]);
        }
    };
    if p6 == 0 {
        script(m, o, PRE_FRAME_SCRIPT, 0x086e_ff1b, 0x086e_ff32);
    }
    let mi2 = m.i16(g::MI2_OUTPUT) != 0;
    let (p6s, p7s) = (p6 as i16 as i32 as u32, p7 as i16 as i32 as u32);
    if !mi2 {
        o.call(m, ext::PRINT_SCENE, &[rp, g::SI_DATABASE, next, p6s, p7s]);
    } else {
        o.call(m, ext::MI2_PRINT_SCENE, &[rp, g::SI_DATABASE, next, p6s, p7s]);
    }
    if p6 == 0 {
        if m.i16(W_8790DEE) != 0 && (!mode3 || per_frame) {
            o.call(m, ext::SI_MRPIPE_FLUSH, &[]);
            loop {
                let r = o.call(m, ext::ACCESS, &[name, 4]).eax;
                o.call(m, ext::USLEEP, &[500_000]);
                if r == 0 {
                    break;
                }
            }
            if name != 0 {
                m.free(name);
            }
            o.call(m, ext::CLOSE, &[fd]);
            o.call(m, ext::UNLINK, &[path]);
        }
        script(m, o, POST_FRAME_SCRIPT, 0x086e_ff4c, 0x086e_ff64);
    }
    if mi2 {
        if p6 != 0 {
            return;
        }
        if !mode3 || !per_frame {
            m.set_u32(g::INCREMENTAL, 1);
        }
    }
    if p6 == 0 && mode3 && per_frame {
        fprintf(m, o, &[0x086e_ff7e]);
        o.call(m, ext::SI_MRPIPE_FLUSH, &[]);
        o.call(m, ext::SI_PIPEIO_DESTROY, &[pipe]);
        let s = m.u32(pipe + 0x2c);
        if s != 0 {
            m.free(s);
            m.set_u32(pipe + 0x2c, 0);
        }
        let cur = m.u32(g::SCN_OUT_NAME);
        if cur != 0 {
            m.free(cur);
            m.set_u32(g::SCN_OUT_NAME, 0);
        }
        m.set_u32(g::SCN_OUT_NAME, saved);
    }
}

const W_8790DD4: u32 = 0x0879_0dd4; // 16 bits: the first frame number of the sequence
const W_8790DEE: u32 = 0x0879_0dee; // 16 bits: a picture file is wanted (semaphore protocol)
const W_8790DDA: u32 = 0x0879_0dda; // 16 bits: the current frame
const STANDALONE_RENDERER: u32 = 0x0867_9400; // 16 bits
const SI_FRAME_RATE: u32 = 0x0878_204c; // float
const STR_MSG_FIELD: u32 = 0x086e_fa20; // char*: the status line's format with a field
const STR_MSG_FRAME_COUNT: u32 = 0x086e_f9ec; // char*: the status line's format
const PRE_FRAME_SCRIPT: u32 = 0x0879_0d60;
const POST_FRAME_SCRIPT: u32 = 0x0879_0d64;
const SCRIPT_ARG: u32 = 0x0879_0d5c;

/// `mi2_print_scene (0x08582054)`: print_scene for the second scene format: the lists of light and model names that the
/// light and model writers return are used for the root instance group and freed.
/// [soft 0x08582054 mi2_print_scene]
pub fn mi2_print_scene(m: &mut Mem, o: &mut dyn Outside, rp: u32, cam: u32, view_arg: u32, hide: u32, extra: u32) {
    let (hide, extra) = (hide as u16, extra as u16);
    let dict = m.u32(rp);
    let l = m.local(0x80);
    let (mat1, mat2) = (l, l + 0x40); // the third matrix is only the light writer's
    let mat3 = m.local(0x40);
    let counts = m.local(8); // [light count, model count]
    let mut lights = 0u32;
    o.call(m, ext::INIT_MATRIX, &[mat1]);
    o.call(m, ext::INIT_MATRIX, &[mat2]);
    o.call(m, ext::INIT_MATRIX, &[mat3]);
    let one = 1.0f32.to_bits();
    o.call(m, ext::SI_ROTATE_WORLD, &[cam, mat1, mat2]);
    if hide == 0 {
        o.call(m, ext::INIT_MATERIALS, &[dict]);
        let frame = m.u32(g::FRAME);
        o.call(m, ext::SI_UDF_GENERATE_TEXTURE_DEFS, &[frame, 0]);
        o.call(m, ext::SI_MUTEX_LOCK, &[rp + self::rp::MUTEX]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 1);
        let cid = m.u32(g::CID_TEXTURES2D);
        o.call(m, ext::SI_ELEMLIST_LOW_BYTE_WORK_FLAG_AND, &[dict, cid, 1]);
        let tex_mode = s16(m, rp + self::rp::TEX_MODE);
        let two = if texture_list_kept(m, rp) { 0 } else { 2 };
        o.call(m, ext::PRINT_2D_TEXTURE_MAP, &[dict, tex_mode, two, 0, 0, 0, rp]);
        o.call(m, ext::SI_MUTEX_UNLOCK, &[rp + self::rp::MUTEX]);
        m.set_u16(rp + self::rp::IN_MESSAGE, 0);
        let on = (m.u32(rp + self::rp::OUT_MODE) == 0) as u32;
        lights = o.call(m, ext::MI2_PRINT_LIGHT_DEFS, &[dict, mat3, 0, counts, on]).eax;
        let tex_mode = s16(m, rp + self::rp::TEX_MODE);
        o.call(m, ext::PRINT_2D_TEXTURE_DEFS, &[dict, tex_mode]);
        o.call(m, ext::MI2_PRINT_VIEW, &[rp, cam, mat1, view_arg]);
        let tex_mode = s16(m, rp + self::rp::TEX_MODE);
        o.call(m, ext::PRINT_ALL_MATERIAL, &[dict, tex_mode]);
    }
    let tex_mode = s16(m, rp + self::rp::TEX_MODE);
    let models = o
        .call(m, ext::MI2_PRINT_ALL_MODELS, &[dict, mat1, mat2, one, one, one, tex_mode, hide as i16 as i32 as u32, extra as i16 as i32 as u32, counts + 4])
        .eax;
    if hide == 0 {
        let first = if m.u32(g::INCREMENTAL) == 0 { 0x086f_0362 } else { 0x086f_0340 };
        fprintf(m, o, &[first]);
        fprintf(m, o, &[0x086f_037f, 0x086f_0378]);
        let mut i = 0;
        while i < m.i32(counts) {
            let name = m.u32(lights + 4 * i as u32);
            fprintf(m, o, &[0x086f_038b, name]);
            m.free(name);
            i += 1;
        }
        o.call(m, ext::SI_MEMTRACK_FREE, &[g::MEMTRACK_PARAMS, lights]);
        let mut i = 0;
        while i < m.i32(counts + 4) {
            let name = m.u32(models + 4 * i as u32);
            if name != 0 {
                fprintf(m, o, &[0x086f_0392, name]);
                m.free(name);
            }
            i += 1;
        }
        m.free(models);
        fprintf(m, o, &[0x086f_0399]);
        fprintf(m, o, &[0x086f_03b1, 0x086f_03aa]);
        if texture_list_kept(m, rp) {
            o.call(m, ext::DESTROY_2D_TEXTURE_LIST, &[]);
        }
    }
}

/// `SI_translateInLiveConnection (0x08585c74)`: the frames `first..=last` in steps of `step` in the live connection: a leading
/// extra frame when motion blur is on, then every frame (twice for fields, `field == 3`), and a flush.
/// [soft 0x08585c74 SI_translateInLiveConnection]
pub fn si_translate_in_live_connection(m: &mut Mem, o: &mut dyn Outside, rp: u32, first: u32, last: u32, step: u32, field: u32) -> u32 {
    let pipe = rp + self::rp::PIPE;
    m.set_u16(rp + self::rp::LIVE_FLAG, (field != 0) as u16);
    m.set_u16(g::W_8790D1C, (field != 0) as u16);
    let passes: i32 = if field == 3 { 2 } else { 1 };
    m.set_u16(W_8790DD4, first as u16);
    m.set_u16(rp + self::rp::FRAME_START, first as u16);
    m.set_u16(0x0879_0dd6, last as u16);
    m.set_u16(rp + self::rp::FRAME_END, last as u16);
    m.set_u16(0x0879_0dd8, step as u16);
    m.set_u16(rp + self::rp::FRAME_STEP, step as u16);
    let count = last.wrapping_sub(first).wrapping_add(1) as i32;
    let blur = m.u16(rp + self::rp::MBLUR);
    let mut frame = first;
    if blur != 0 {
        let pass = if field == 2 { 1 } else { (passes - 1) as u32 };
        frame = first.wrapping_sub(step);
        o.call(m, ext::PRINT_FRAME, &[rp, frame, 0, 0, pass, 1, blur as i16 as i32 as u32]);
        frame = frame.wrapping_add(step);
    }
    let mut which = (field == 2) as u32;
    let (mut i, mut n) = (0i32, 0u32);
    while i < count {
        let next_i = i.wrapping_add(step as i32);
        let next_frame = frame.wrapping_add(step);
        let mut pass = 0;
        while pass < passes {
            if field == 3 {
                which = pass as u32;
            }
            m.set_u32(pipe + 0x30, 0);
            o.call(m, ext::PRINT_FRAME, &[rp, frame, i as u32, n, which, 0, blur as i16 as i32 as u32]);
            let p = m.u32(pipe + 0x30);
            if p != 0 {
                m.free(p);
                m.set_u32(pipe + 0x30, 0);
            }
            pass += 1;
        }
        i = next_i;
        frame = next_frame;
        n += 1;
    }
    o.call(m, ext::SI_MRPIPE_FLUSH, &[]);
    1
}

/// `SI_customPreviewWriteUDF (0x08585600)`: the material preview of a user-defined shader: the render control is reset to
/// the defaults, the preview-argument block is wired to the material, its texture or 3D texture, and the shader scan, the
/// texture definitions, the lights and the material are written.
/// [soft 0x08585600 SI_customPreviewWriteUDF]
pub fn si_custom_preview_write_udf(m: &mut Mem, o: &mut dyn Outside, dict: u32, elem: u32, _file: u32, udf: u32, name: u32) -> u32 {
    if dict == 0 || elem == 0 {
        return 0;
    }
    m.copy(g::SI_REN, SI_DEFAULT_CONTROL, 0x600);
    let rec = m.u32(elem + 4);
    if rec == 0 {
        return 0;
    }
    let outs = m.local(16); // the related elements: material, texture, ...
    let mat_out = outs;
    let cid_mat = m.u32(CID_MATERIALS);
    o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, elem, 1, 3, 300, cid_mat, mat_out, 0]);
    let mat = m.u32(mat_out);
    if mat == 0 {
        return 0;
    }
    for a in [ARGS_DB0, ARGS_DB4, ARGS_DB8, ARGS_DBC, ARGS_F64, ARGS_10F0] {
        m.set_u32(a, 0);
    }
    m.set_u16(ARGS_6E8, 0);
    for a in [ARGS_6EC, ARGS_6F0, ARGS_6FC, ARGS_6F4] {
        m.set_u32(a, 0);
    }
    let kind = m.u32(udf + 4);
    let (k30, k34, k38, k3c, k40, k50) = (m.u32(TYPE_30), m.u32(TYPE_34), m.u32(TYPE_38), m.u32(TYPE_3C), m.u32(TYPE_40), m.u32(TYPE_50));
    if kind != k34 && kind != k50 {
        o.call(m, ext::SI_TEXTURE_INITIALIZE, &[LCLMAP]);
    }
    if kind != k38 {
        o.call(m, ext::SI_TEXTURE3D_INITIALIZE, &[TEXTURE3D]);
    }
    m.set_u16(W_879116E, 0);
    m.set_u16(W_8791166, 0);
    m.set_u16(W_8791168, 0);
    let kind = m.u32(udf + 4);
    let tex_cid2d = m.u32(g::CID_TEXTURES2D);
    if kind == k30 {
        m.set_u32(mat + 4, CUSTOM_PREVIEW_ARGS);
        m.set_u32(ARGS_DB0, udf);
    } else if kind == k3c {
        m.set_u16(W_879116E, 1);
        m.set_u32(mat + 4, CUSTOM_PREVIEW_ARGS);
        m.set_u32(ARGS_DB4, udf);
    } else if kind == k40 {
        m.set_u16(W_879116E, 1);
        m.set_u32(mat + 4, CUSTOM_PREVIEW_ARGS);
        m.set_u32(ARGS_DB8, udf);
    } else if kind == k50 || kind == k34 {
        m.set_u16(W_8791166, 1);
        if kind == k50 {
            m.set_u16(W_879116A, 1);
        }
        let out = outs + 4;
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, mat, 1, 3, 0x191, tex_cid2d, out, 0]);
        let t = m.u32(out);
        m.set_u32(t + 4, LCLMAP);
        m.set_u32(mat + 4, CUSTOM_PREVIEW_ARGS);
        m.set_u32(ARGS_F64, udf);
        m.set_u8(t + 0x5b, 1);
    } else if kind == k38 {
        m.set_u16(W_8791166, 1);
        m.set_u16(W_8791168, 1);
        let out = outs + 8;
        let cid3d = m.u32(CID_TEXTURES3D);
        o.call(m, ext::SI_ELEMDICT_GET_FIRST_RELATED_ELEM, &[dict, mat, 1, 3, 0x1f5, cid3d, out, 0]);
        let t = m.u32(out);
        m.set_u32(t + 4, TEXTURE3D);
        m.set_u32(mat + 4, CUSTOM_PREVIEW_ARGS);
        m.set_u32(ARGS_10F0, udf);
        m.set_u8(t + 0x5b, 1);
    }
    o.call(m, ext::SI_MEMTRACK_INITIALIZE, &[g::MEMTRACK_PARAMS]);
    m.set_u16(udf + 0xc, 1);
    o.call(m, ext::SI_MENTAL_SHADER_SCAN, &[dict, g::SI_REN, 1]);
    o.call(m, ext::SI_UDF_GENERATE_TEXTURE_DEFS, &[1, udf]);
    let kind = m.u32(udf + 4);
    if kind == m.u32(TYPE_34) || kind == m.u32(TYPE_38) {
        o.call(m, ext::PRINT_2D_TEXTURE_DEFS_PREVIEW, &[dict, elem, CUSTOM_PREVIEW_ARGS]);
    }
    let mats = m.local(0x80);
    let (mat1, mat2) = (mats, mats + 0x40);
    o.call(m, ext::INIT_MATRIX, &[mat1]);
    o.call(m, ext::INIT_MATRIX, &[mat2]);
    o.call(m, ext::SI_ROTATE_WORLD, &[g::SI_DATABASE, mat1, mat2]);
    o.call(m, ext::PRINT_LIGHT_DEFS, &[dict, mat1, 1, 0]);
    let last = m.u32(rec + 4);
    o.call(m, ext::PRINT_MATERIAL, &[dict, elem, mat, 0, name, 0, last]);
    1
}

const SI_DEFAULT_CONTROL: u32 = 0x0878_1800;
const CUSTOM_PREVIEW_ARGS: u32 = 0x0879_9860;
const LCLMAP: u32 = 0x0879_a62c; // SI_customPreviewArgs + 0xdcc
const TEXTURE3D: u32 = 0x0879_a804; // SI_customPreviewArgs + 0xfa4
const ARGS_DB0: u32 = 0x0879_a610;
const ARGS_DB4: u32 = 0x0879_a614;
const ARGS_DB8: u32 = 0x0879_a618;
const ARGS_DBC: u32 = 0x0879_a61c;
const ARGS_F64: u32 = 0x0879_a7c4;
const ARGS_10F0: u32 = 0x0879_a950;
const ARGS_6E8: u32 = 0x0879_9f48; // 16 bits
const ARGS_6EC: u32 = 0x0879_9f4c;
const ARGS_6F0: u32 = 0x0879_9f50;
const ARGS_6F4: u32 = 0x0879_9f54;
const ARGS_6FC: u32 = 0x0879_9f5c;
const CID_MATERIALS: u32 = 0x0878_167c;
const CID_TEXTURES3D: u32 = 0x0878_16cc;
const TYPE_30: u32 = 0x0878_1730;
const TYPE_34: u32 = 0x0878_1734;
const TYPE_38: u32 = 0x0878_1738;
const TYPE_3C: u32 = 0x0878_173c;
const TYPE_40: u32 = 0x0878_1740;
const TYPE_50: u32 = 0x0878_1750;
const W_879116E: u32 = 0x0879_116e;
const W_8791166: u32 = 0x0879_1166;
const W_8791168: u32 = 0x0879_1168;
const W_879116A: u32 = 0x0879_116a;

/// The f32 at `a` as the two words of the double the original pushes for a `%f` (an x87 load and a 64-bit store).
fn f32_as_f64_words(m: &Mem, a: u32) -> [u32; 2] {
    let b = (m.f32(a) as f64).to_bits();
    [b as u32, (b >> 32) as u32]
}

/// "on" or "off" as the strings of the options block: the first address when the 16-bit flag at `a` is set.
fn on_off_at(m: &Mem, a: u32, on: u32, off: u32) -> u32 {
    if m.u16(a) != 0 { on } else { off }
}

/// `print_option (0x08584e8c)`: the `options "opt"` block of the scene, from the render control copied into the globals
/// at 0x08790e8c..0x087911xx: shadow, trace, dither, acceleration, filter, samples, scanline, gamma, contrast, trace
/// depth, face, desaturate, shutter, time contrast, shadow map, caustic, photon and final gather settings.
/// [soft 0x08584e8c print_option]
pub fn print_option(m: &mut Mem, o: &mut dyn Outside, rp: u32, cam: u32) {
    let dict = m.u32(rp);
    let incremental = m.u32(0x086e_f9bc);
    fprintf(m, o, &[if incremental != 0 { 0x086f_11dd } else { 0x086f_11f8 }]);
    if m.u16(0x0879_116e) == 0 {
        fprintf(m, o, &[0x086f_1207]);
    } else {
        let s = si_onoff(s16(m, 0x0879_0eaa));
        fprintf(m, o, &[0x086f_1214, s]);
    }
    let s = si_onoff(s16(m, 0x0879_0ea8));
    fprintf(m, o, &[0x086f_1220, s]);
    fprintf(m, o, &[if m.u32(0x0879_0e30) != 0 { 0x086f_122b } else { 0x086f_1237 }]);
    if m.u32(0x0879_0fe4) == 1 {
        fprintf(m, o, &[0x086f_1244]);
        let v = m.u32(0x0879_1000);
        fprintf(m, o, &[0x086f_1257, v]);
        let v = m.u32(0x0879_0ffc);
        fprintf(m, o, &[0x086f_1265, v]);
    } else {
        fprintf(m, o, &[0x086f_1274]);
        let [a, b] = f32_as_f64_words(m, 0x0879_109c);
        fprintf(m, o, &[0x086f_1288, a, b]);
    }
    let filter = m.u32(0x0879_1028);
    if filter != 1 {
        match filter {
            4 => fprintf(m, o, &[0x086f_12a6]),
            2 => fprintf(m, o, &[0x086f_1297]),
            8 => fprintf(m, o, &[0x086f_12b3]),
            _ => {}
        }
        let [a, b] = f32_as_f64_words(m, 0x0879_1020);
        let [c, d] = f32_as_f64_words(m, 0x0879_1024);
        fprintf(m, o, &[0x086f_12c5, a, b, c, d]);
    }
    if m.u16(0x0879_1078) == 0 {
        fprintf(m, o, &[0x086f_12de]);
    } else {
        let (x, y) = (m.i32(0x0879_0e9c), m.i32(0x0879_0e98));
        let (mut lo, mut hi) = if x > y { (y, x) } else { (x, y) };
        let t = hi.wrapping_sub(3);
        if lo < t {
            lo = t;
        }
        if lo < 0 && (m.u16(0x0879_0fbe) != 0 || m.u16(0x0879_0fc0) != 0) {
            lo = 0;
            if hi < 0 {
                hi = 0;
            }
        }
        fprintf(m, o, &[0x086f_12ce, lo as u32, hi as u32]);
    }
    let mode = m.u32(cam + 0x6c);
    let cls = m.u32(0x0878_16a8);
    if mode.wrapping_sub(1) <= 1 || o.call(m, ext::IS_SCANLINE_OFF, &[dict, cls]).eax as u16 != 0 {
        fprintf(m, o, &[0x086f_12ec]);
    }
    let [a, b] = f32_as_f64_words(m, 0x0879_1004);
    fprintf(m, o, &[0x086f_12fb, a, b]);
    let (c0, c1, c2) = (f32_as_f64_words(m, 0x0879_1010), f32_as_f64_words(m, 0x0879_1014), f32_as_f64_words(m, 0x0879_1018));
    fprintf(m, o, &[0x086f_1306, c0[0], c0[1], c1[0], c1[1], c2[0], c2[1]]);
    let (d0, d1, d2) = (m.u32(0x0879_0fd4), m.u32(0x0879_0fd8), m.u32(0x0879_0fdc));
    fprintf(m, o, &[0x086f_131a, d0, d1, d2]);
    let face = m.u32(0x0879_0eac);
    let face_s = if face == 3 { 0x086f_133c } else if face == 1 { 0x086f_1331 } else { 0x086f_1337 };
    fprintf(m, o, &[0x086f_1341, face_s]);
    let s = si_onoff(s16(m, 0x0879_100c));
    fprintf(m, o, &[0x086f_134b, s]);
    let shutter = m.f32(0x0879_0e8c);
    if !(shutter == 0.0) && m.u16(0x0879_107a) != 0 && m.u16(0x0879_107c) == 0 {
        let [a, b] = f32_as_f64_words(m, 0x0879_0e8c);
        fprintf(m, o, &[0x086f_135b, a, b]);
    }
    let mut args = vec![0x086f_1368u32];
    for a in [0x0879_108cu32, 0x0879_1090, 0x0879_1094, 0x0879_1098] {
        args.extend(f32_as_f64_words(m, a));
    }
    fprintf(m, o, &args);
    fprintf(m, o, &[0x086f_1384]);
    let set = if m.u32(rp + 0x654) == 0 { 0x0879_10e8 } else { 0x0879_1164 };
    let s = on_off_at(m, set + 0x1c, 0x086f_1393, 0x086f_1396);
    fprintf(m, o, &[0x086f_139a, s]);
    fprintf(m, o, &[0x086f_13a9]);
    let s = on_off_at(m, set + 0x1e, 0x086f_13c0, 0x086f_13c3);
    fprintf(m, o, &[0x086f_13c7, s]);
    let s = on_off_at(m, set + 0x20, 0x086f_13dd, 0x086f_13e0);
    fprintf(m, o, &[0x086f_13e4, s]);
    let s = on_off_at(m, set + 0x26, 0x086f_13f2, 0x086f_13f5);
    fprintf(m, o, &[0x086f_13f9, s]);
    let s = on_off_at(m, set + 0x22, 0x086f_1405, 0x086f_1408);
    fprintf(m, o, &[0x086f_140c, s]);
    let s = on_off_at(m, set + 0x24, 0x086f_1416, 0x086f_1419);
    fprintf(m, o, &[0x086f_141d, s]);
    fprintf(m, o, &[0x086f_1429]);
    if m.u32(rp + 0x654) == 0 {
        let s = on_off_at(m, 0x0879_10a4, 0x086f_1435, 0x086f_1438);
        fprintf(m, o, &[0x086f_143c, s]);
    } else {
        let s = on_off_at(m, 0x0879_10a0, 0x086f_1449, 0x086f_144c);
        fprintf(m, o, &[0x086f_1450, s]);
    }
    let v = m.u32(0x0879_10bc);
    fprintf(m, o, &[0x086f_145d, v]);
    if m.u16(0x0879_10ac) != 0 {
        let [a, b] = f32_as_f64_words(m, 0x0879_10d8);
        fprintf(m, o, &[0x086f_1472, a, b]);
    }
    fprintf(m, o, &[0x086f_1476]);
    let s = if m.u16(0x0879_10b8) != 0 { 0x086f_147c } else { 0x086f_1478 };
    fprintf(m, o, &[0x086f_1481, s]);
    if m.f32(0x0879_10d4) as f64 > 1.0001f64 {
        let [a, b] = f32_as_f64_words(m, 0x0879_10d4);
        fprintf(m, o, &[0x086f_1494, a, b]);
    }
    fprintf(m, o, &[0x086f_1498]);
    let (p0, p1, p2) = (m.u32(0x0879_10c8), m.u32(0x0879_10cc), m.u32(0x0879_10d0));
    fprintf(m, o, &[0x086f_149a, p0, p1, p2]);
    let file = m.u32(0x0879_10b4);
    if o.call(m, crate::mi_patch::EMPTY_STRING, &[file]).eax as u16 == 0 {
        let file = m.u32(0x0879_10b4);
        fprintf(m, o, &[0x086f_14b8, file]);
    }
    let s = on_off_at(m, 0x0879_10aa, 0x086f_14ce, 0x086f_14d1);
    fprintf(m, o, &[0x086f_14d5, s]);
    fprintf(m, o, &[0x086f_14ec]);
    if m.u32(rp + 0x654) == 0 {
        let s = on_off_at(m, 0x0879_10a6, 0x086f_14fa, 0x086f_14fd);
        fprintf(m, o, &[0x086f_1501, s]);
    } else {
        let s = on_off_at(m, 0x0879_10a2, 0x086f_1510, 0x086f_1513);
        fprintf(m, o, &[0x086f_1517, s]);
    }
    let v = m.u32(0x0879_10c0);
    fprintf(m, o, &[0x086f_1526, v]);
    if m.u16(0x0879_10ae) != 0 {
        let [a, b] = f32_as_f64_words(m, 0x0879_10dc);
        fprintf(m, o, &[0x086f_153d, a, b]);
    }
    fprintf(m, o, &[0x086f_1541]);
    let s = on_off_at(m, 0x0879_10a8, 0x086f_1543, 0x086f_1546);
    fprintf(m, o, &[0x086f_154a, s]);
    let v = m.u32(0x0879_10c4);
    fprintf(m, o, &[0x086f_155b, v]);
    if m.u16(0x0879_10b2) != 0 {
        let [a, b] = f32_as_f64_words(m, 0x0879_10e4);
        fprintf(m, o, &[0x086f_1574, a, b]);
        if m.u16(0x0879_10b0) != 0 {
            let [a, b] = f32_as_f64_words(m, 0x0879_10e0);
            fprintf(m, o, &[0x086f_1578, a, b]);
        }
    }
    fprintf(m, o, &[0x086f_157c]);
    fprintf(m, o, &[0x086f_157e]);
}

fn strcat_bytes(m: &mut Mem, dst: u32, lit: &[u8]) {
    let end = dst + m.cstr(dst).len() as u32;
    m.set_bytes(end, lit);
    m.set_u8(end + lit.len() as u32, 0);
}

fn f64_words(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// `print_view (0x085826f4)`: the `view` block of the scene: the output statements (depth of field, extra buffers of the
/// user shaders, motion blur, picture files, contour), the lens shaders, focal, aperture, aspect, resolution, window,
/// acceleration, filter, samples, scanline, gamma, contrast, trace depth, clip, face, shadow, fog and the transform that
/// is the inverse of the camera matrix. The arithmetic is the x87's (80-bit sums, products and quotients until they are
/// stored); `tan` is the C library's.
/// [soft 0x085826f4 print_view]
pub fn print_view(m: &mut Mem, o: &mut dyn Outside, rp: u32, cam: u32, mat: u32, p4: u32) {
    use si_x87::{trig, Round, F80};
    let f = F80::from_f32;
    let half = f(0.5);
    let dict = m.u32(rp);
    let frame = m.u32(g::FRAME);
    let mat_local = m.local(0x40);
    let buf = m.local(0x2000);
    let namebuf = m.local(0x100);
    let sbuf = m.local(0x100);
    let name = o.call(m, ext::SI_BUILD_PIC_FILE_NAME, &[m.u32(rp + 0x188), frame, s16(m, rp + 0x148), p4, 0]).eax;
    // strncpy(buf, name, 0x1fff) pads with zeros up to the count
    let text = m.cstr(name);
    let n = text.len().min(0x1fff);
    m.set_bytes(buf, &text[..n]);
    for i in n..0x1fff {
        m.set_u8(buf + i as u32, 0);
    }
    m.set_u8(buf + 0x1fff, 0);
    o.call(m, ext::SI_MRFILE_CONVERT_TO_UNIX, &[buf, buf, 0x2000]);
    if f(1.0).gt(f(m.f32(cam + 0x70))) {
        m.set_f32(cam + 0x70, 1.0);
    }
    if f(m.f32(cam + 0x40)).lt(F80::from_f64(0.0001)) {
        m.set_f32(cam + 0x40, f32::from_bits(0x38d1_b717));
    }
    fprintf(m, o, &[0x086f_0417]);
    let (w, h) = (m.i16(0x0879_117a) as i32, m.i16(0x0879_117c) as i32);
    let pa = m.f32(0x0879_0d34);
    let (c40, c70) = (m.f32(cam + 0x40), m.f32(cam + 0x70));
    let mut aperture: f64 = if m.u16(cam + 0x108) != 0 {
        (f(c70) * m.f32(cam + 0x110) / m.f32(cam + 0x10c)).to_f64()
    } else {
        let t = trig::tan((f(c40) * 0.5f32).to_f64()).to_f64();
        let v = F80::from_i32(w) * (f(c70) + f(c70)) / F80::from_i32(h) * t * pa;
        v.to_f64()
    };
    if m.u16(0x086e_f9cc) != 0 {
        aperture = (f(m.f32(cam + 0x40)) * 1.175f64).to_f64();
    }
    let env = o.call(m, ext::GETENV, &[0x086f_041d]).eax;
    let mut plane = 0.0f32;
    let mut radius = 0.0f32;
    let mode = m.u32(cam + 0x6c);
    if env != 0 {
        if mode == 2 {
            radius = (F80::from_f32(m.f32(cam + 0x98)) / F80::from_i32(w) * aperture).to_f32();
            fprintf(m, o, &[0x086f_0440]);
            let a = f32_as_f64_words(m, cam + 0x7c);
            fprintf(m, o, &[0x086f_0469, a[0], a[1]]);
            let a = f32_as_f64_words(m, cam + 0x84);
            fprintf(m, o, &[0x086f_0478, a[0], a[1]]);
            let r = f64_words(radius as f64);
            fprintf(m, o, &[0x086f_0486, r[0], r[1]]);
            let v = (f(m.f32(cam + 0x98)) * 0.5f32).to_i32(Round::Zero);
            fprintf(m, o, &[0x086f_0496, v as u32]);
        } else if mode == 1 {
            radius = (F80::from_f32(0.5) * m.f32(cam + 0x70) / m.f32(cam + 0x94) * 0.0032834f64).to_f32();
            fprintf(m, o, &[0x086f_04c0]);
            let a = f32_as_f64_words(m, cam + 0x74);
            fprintf(m, o, &[0x086f_04e9, a[0], a[1]]);
            fprintf(m, o, &[0x086f_04f8, a[0], a[1]]);
            let r = f64_words(radius as f64);
            fprintf(m, o, &[0x086f_0506, r[0], r[1]]);
            let v = (half * m.f32(cam + 0x98)).to_i32(Round::Zero);
            fprintf(m, o, &[0x086f_0516, v as u32]);
        }
    }
    let _ = half;
    let mut acc = 0u32;
    let n = m.i32(0x0879_0fc4);
    for i in 0..n.max(0) as u32 {
        let udf = m.u32(m.u32(0x0879_0fcc) + i * 4);
        if udf == 0 || o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 == 0 {
            continue;
        }
        if m.u16(udf + 0x10) != 0 {
            continue;
        }
        if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, namebuf, 0x100]).eax as u16 == 0 {
            continue;
        }
        let flags = o.call(m, ext::SI_UDF_GET_OUTPUT_BUFFER_FLAGS, &[udf]).eax;
        m.set_bytes(sbuf, b"rgba\0");
        if flags & 0x10 != 0 {
            strcat_bytes(m, sbuf, b"_fp");
        } else if flags & 8 != 0 {
            strcat_bytes(m, sbuf, b"_16");
        }
        if flags & 1 != 0 {
            strcat_bytes(m, sbuf, b",z");
        }
        if flags & 4 != 0 {
            strcat_bytes(m, sbuf, b",n");
        }
        if flags & 2 != 0 {
            strcat_bytes(m, sbuf, b",tag");
        }
        fprintf(m, o, &[0x086f_053e, sbuf, namebuf]);
        let fc = m.u32(g::SI_FRAME_CURRENT);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
        fprintf(m, o, &[0x086f_0551]);
        acc |= flags;
    }
    let blur = m.u16(0x0879_107a) != 0 && m.u16(0x0879_107c) == 1;
    if blur {
        fprintf(m, o, &[0x086f_0560]);
        let a = f32_as_f64_words(m, 0x0879_0e8c);
        fprintf(m, o, &[0x086f_0582, a[0], a[1]]);
        fprintf(m, o, &[0x086f_0590, m.i16(0x0879_107e) as i32 as u32]);
        let v = m.u32(0x0879_1080);
        fprintf(m, o, &[0x086f_059b, v]);
    }
    let contour = m.u16(0x0879_0fbc) != 0;
    if !contour {
        let ext_name = o.call(m, ext::SI_GET_RENDER_IMG_SUFFIX, &[m.u32(0x0879_106c)]).eax;
        m.set_u32(0x0882_9f20, ext_name);
        fprintf(m, o, &[0x086f_05ab, ext_name, buf, ext_name]);
        let e = m.u32(0x0882_9f20);
        if e != 0 {
            m.free(e);
            m.set_u32(0x0882_9f20, 0);
        }
    }
    if m.u16(0x0879_0fbe) != 0 {
        fprintf(m, o, &[0x086f_05c1, buf]);
    }
    if m.u16(0x0879_0fc0) != 0 {
        fprintf(m, o, &[0x086f_05db, buf]);
    }
    if contour {
        fprintf(m, o, &[0x086f_0600, buf]);
        fprintf(m, o, &[0x086f_0622]);
        let a = f32_as_f64_words(m, 0x0879_1084);
        fprintf(m, o, &[0x086f_062f, a[0], a[1]]);
        let a = f32_as_f64_words(m, 0x0879_1088);
        fprintf(m, o, &[0x086f_0647, a[0], a[1]]);
    }
    if blur {
        fprintf(m, o, &[0x086f_065a]);
    }
    if env == 0 {
        if mode == 2 {
            plane = (-(f(m.f32(cam + 0x84)) + m.f32(cam + 0x7c)) * 0.5f64).to_f32();
            radius = (F80::from_f32(m.f32(cam + 0x98)) / F80::from_i32(w) * aperture).to_f32();
            let (a, b) = (f64_words(plane as f64), f64_words(radius as f64));
            fprintf(m, o, &[0x086f_0680, a[0], a[1], b[0], b[1]]);
        } else if mode == 1 {
            plane = -m.f32(cam + 0x74);
            radius = (half * m.f32(cam + 0x70) / m.f32(cam + 0x94) * 0.0032834f64).to_f32();
            let (a, b) = (f64_words(plane as f64), f64_words(radius as f64));
            fprintf(m, o, &[0x086f_06c0, a[0], a[1], b[0], b[1]]);
        }
    }
    let n = m.i32(0x0879_0fc8);
    for i in 0..n.max(0) as u32 {
        let udf = m.u32(m.u32(0x0879_0fd0) + i * 4);
        if udf == 0 || o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 == 0 {
            continue;
        }
        if m.u16(udf + 0x10) != 0 {
            continue;
        }
        if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, sbuf, 0x100]).eax as u16 == 0 {
            continue;
        }
        fprintf(m, o, &[0x086f_06f7, sbuf]);
        if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
            fprintf(m, o, &[0x086f_0703]);
            let a = f64_words(plane as f64);
            fprintf(m, o, &[0x086f_0714, a[0], a[1]]);
            let a = f64_words(radius as f64);
            fprintf(m, o, &[0x086f_0723, a[0], a[1]]);
        }
        let fc = m.u32(g::SI_FRAME_CURRENT);
        o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
        fprintf(m, o, &[0x086f_0735]);
    }
    if m.u16(0x086e_f9cc) != 0 {
        fprintf(m, o, &[0x086f_0739]);
    } else {
        let a = f32_as_f64_words(m, cam + 0x70);
        fprintf(m, o, &[0x086f_074a, a[0], a[1]]);
    }
    if m.u16(0x0879_107a) != 0 {
        let k = m.u16(0x0879_107c);
        if k == 0 {
            let a = f32_as_f64_words(m, 0x0879_0e8c);
            fprintf(m, o, &[0x086f_0755, a[0], a[1]]);
        } else if k == 2 {
            fprintf(m, o, &[0x086f_0762]);
        }
    }
    let a = f64_words(aperture);
    fprintf(m, o, &[0x086f_0770, a[0], a[1]]);
    if m.u16(cam + 0x108) != 0 {
        let v = (f(m.f32(cam + 0x110)) / m.f32(cam + 0x114)).to_f64();
        let a = f64_words(v);
        fprintf(m, o, &[0x086f_077e, a[0], a[1]]);
    } else {
        let v = (f(pa) * (F80::from_i32(w) / F80::from_i32(h))).to_f64();
        let a = f64_words(v);
        fprintf(m, o, &[0x086f_078a, a[0], a[1]]);
    }
    fprintf(m, o, &[0x086f_0796, w as u32, h as u32]);
    if m.u32(0x0879_0e04) != 0 {
        let (wf, hf, c) = (F80::from_i32(w), F80::from_i32(h), half);
        let ia = (f(m.f32(0x0879_0e0c)) * wf + c).to_i32(Round::Zero);
        let edi = (wf * (F80::from_i32(1) - m.f32(0x0879_0e10)) + c).to_i32(Round::Zero);
        let esi = ((F80::from_i32(1) - m.f32(0x0879_0e14)) * hf + c).to_i32(Round::Zero);
        let ebx = (c + hf * m.f32(0x0879_0e18)).to_i32(Round::Zero);
        fprintf(m, o, &[0x086f_07a9, ia as u32, ebx as u32, edi as u32, esi as u32]);
        o.call(m, ext::SDEBUG, &[0x086f_07be, ia as u32, ebx as u32, edi as u32, esi as u32]);
    }
    if m.u32(0x0879_0fe0) == 0 || m.u16(0x0879_0fbc) != 0 {
        fprintf(m, o, &[0x086f_07e0]);
        let (a, b) = (m.u32(0x0879_0fe8), m.u32(0x0879_0fec));
        fprintf(m, o, &[0x086f_0802, a, b]);
        let v = m.u32(0x0879_0ff0);
        fprintf(m, o, &[0x086f_0817, v]);
    } else {
        fprintf(m, o, &[0x086f_0840]);
        let v = m.u32(0x0879_0ff8);
        fprintf(m, o, &[0x086f_0863, v]);
        let v = m.u32(0x0879_0ff4);
        fprintf(m, o, &[0x086f_0871, v]);
    }
    let filter = m.u32(0x0879_1028);
    if filter != 1 {
        match filter {
            4 => fprintf(m, o, &[0x086f_088f]),
            2 => fprintf(m, o, &[0x086f_0880]),
            8 => fprintf(m, o, &[0x086f_089c]),
            _ => {}
        }
        let [a, b] = f32_as_f64_words(m, 0x0879_1020);
        let [c, d] = f32_as_f64_words(m, 0x0879_1024);
        fprintf(m, o, &[0x086f_08ae, a, b, c, d]);
    }
    let (fbe, fc0) = (m.u16(0x0879_0fbe) != 0, m.u16(0x0879_0fc0) != 0);
    if m.u16(0x0879_1078) != 0 {
        if m.u16(0x0879_100e) != 0 {
            fprintf(m, o, &[0x086f_08b7]);
            fprintf(m, o, &[0x086f_08c6]);
        } else {
            fprintf(m, o, &[0x086f_08d4]);
            fprintf(m, o, &[0x086f_08e4]);
        }
        if acc & 2 != 0 {
            m.set_u32(0x0879_0e9c, 0);
            if m.i32(0x0879_0e98) < 0 {
                m.set_u32(0x0879_0e98, 0);
            }
        }
        let mut lo = m.i32(0x0879_0e9c);
        let t = m.i32(0x0879_0e98).wrapping_sub(3);
        if lo < t {
            lo = t;
        }
        if lo < 0 && (fbe || fc0 || blur) {
            lo = 0;
        }
        fprintf(m, o, &[0x086f_08f3, lo as u32]);
        let mx = m.u32(0x0879_0e98);
        fprintf(m, o, &[0x086f_0904, mx]);
        fprintf(m, o, &[0x086f_0915, (mx as i32).max(1) as u32]);
    } else {
        fprintf(m, o, &[0x086f_0922]);
        fprintf(m, o, &[0x086f_0932]);
        fprintf(m, o, &[0x086f_0941]);
        let v = if fbe || fc0 || blur { 0 } else { m.u32(0x0879_0e9c) };
        fprintf(m, o, &[0x086f_094d, v]);
        let v = m.u32(0x0879_0e98);
        fprintf(m, o, &[0x086f_095e, v]);
    }
    let cls = m.u32(0x0878_16a8);
    if mode.wrapping_sub(1) <= 1 || o.call(m, ext::IS_SCANLINE_OFF, &[dict, cls]).eax as u16 != 0 {
        fprintf(m, o, &[0x086f_096f]);
    }
    let a = f32_as_f64_words(m, 0x0879_1004);
    fprintf(m, o, &[0x086f_097e, a[0], a[1]]);
    let mut args = vec![0x086f_0989u32];
    for k in [0x0879_1010u32, 0x0879_1014, 0x0879_1018] {
        args.extend(f32_as_f64_words(m, k));
    }
    fprintf(m, o, &args);
    let (a, b) = (m.u32(0x0879_0fd4), m.u32(0x0879_0fd8));
    fprintf(m, o, &[0x086f_099d, a, b]);
    let near = if F80::from_f64(0.0001).lt(f(m.f32(cam + 0x34))) { (m.f32(cam + 0x34) as f64).to_bits() } else { 0.0001f64.to_bits() };
    let far = f32_as_f64_words(m, cam + 0x38);
    fprintf(m, o, &[0x086f_09b1, near as u32, (near >> 32) as u32, far[0], far[1]]);
    let face = m.u32(0x0879_0eac);
    let face_s = if face == 3 { 0x086f_09c9 } else if face == 1 { 0x086f_09be } else { 0x086f_09c4 };
    fprintf(m, o, &[0x086f_09ce, face_s]);
    if m.u16(0x0879_116e) == 0 {
        fprintf(m, o, &[0x086f_09d8]);
    } else {
        let s = si_onoff(s16(m, 0x0879_0eaa));
        fprintf(m, o, &[0x086f_09e5, s]);
    }
    let s = si_onoff(s16(m, 0x0879_0ea8));
    fprintf(m, o, &[0x086f_09f1, s]);
    let s = si_onoff(s16(m, 0x0879_100c));
    fprintf(m, o, &[0x086f_09fc, s]);
    fprintf(m, o, &[if m.u32(0x0879_0e30) != 0 { 0x086f_0a0c } else { 0x086f_0a18 }]);
    let soft_prefix = m.u32(0x086e_f9b8);
    let pair = |m: &Mem, a: u32| f32_as_f64_words(m, a);
    let last_fmt;
    if m.u16(0x0879_0e3c) != 0 {
        fprintf(m, o, &[0x086f_0a25, soft_prefix]);
        for (fmt, a) in [(0x086f_0a41u32, 0x0879_0e4cu32), (0x086f_0a51, 0x0879_0e50), (0x086f_0a60, 0x0879_0e44), (0x086f_0a6f, 0x0879_0e48), (0x086f_0a82, 0x0879_0e40)] {
            let w2 = pair(m, a);
            fprintf(m, o, &[fmt, w2[0], w2[1]]);
        }
        let (r, gg, b) = (pair(m, 0x0879_0e64), pair(m, 0x0879_0e68), pair(m, 0x0879_0e6c));
        fprintf(m, o, &[0x086f_0a93, r[0], r[1], gg[0], gg[1], b[0], b[1]]);
        last_fmt = Some(0x086f_0aaau32);
    } else if m.u16(0x0879_0ce0) != 0 {
        let (a, b) = (m.f32(0x0879_0ce4), m.f32(0x0879_0cec));
        if F80::from_f64(1e-6).gt(f(b) - f(a)) {
            m.set_f32(0x0879_0cec, (f(a) + 1e-6f64).to_f32());
        }
        let udf = m.u32(0x0879_102c);
        let en = udf != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0;
        let si = if en { o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 } else { 1 };
        if en {
            o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, sbuf, 0x100]);
            fprintf(m, o, &[0x086f_0aae, sbuf]);
        } else {
            fprintf(m, o, &[0x086f_0abc, soft_prefix]);
        }
        if si != 0 {
            if en {
                fprintf(m, o, &[0x086f_0ad0]);
            }
            let pick = |m: &Mem, x: u32, alt: u32| if F80::from_f32(m.f32(x)).eq(F80::ZERO) { alt } else { x };
            for (fmt, x, alt) in [(0x086f_0ae1u32, 0x0879_0ce8u32, 0x0879_0ce4u32), (0x086f_0af3, 0x0879_0cf0, 0x0879_0cec), (0x086f_0b05, 0x0879_0cf4, 0x0879_0d10)] {
                let src = pick(m, x, alt);
                let w2 = pair(m, src);
                fprintf(m, o, &[fmt, w2[0], w2[1]]);
            }
            let (a, b, c) = (pair(m, 0x0879_0cf8), pair(m, 0x0879_0cfc), pair(m, 0x0879_0d00));
            fprintf(m, o, &[0x086f_0b17, a[0], a[1], b[0], b[1], c[0], c[1]]);
            if en {
                fprintf(m, o, &[0x086f_0b2d]);
            }
        }
        if en {
            if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
                fprintf(m, o, &[0x086f_0b30]);
            }
            let fc = m.u32(g::SI_FRAME_CURRENT);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
        }
        last_fmt = Some(0x086f_0b33);
    } else {
        last_fmt = None;
    }
    if let Some(fmt) = last_fmt {
        fprintf(m, o, &[fmt]);
    }
    crate::mi_calc::invert_matrix(m, o, mat_local, mat);
    let v = |m: &Mem, i: u32| f32_as_f64_words(m, mat_local + i * 4);
    for (fmt, r) in [(0x086f_0b37u32, 0u32), (0x086f_0b50, 4), (0x086f_0b62, 8), (0x086f_0b74, 12)] {
        let (a, b, c) = (v(m, r), v(m, r + 1), v(m, r + 2));
        fprintf(m, o, &[fmt, a[0], a[1], b[0], b[1], c[0], c[1]]);
    }
    fprintf(m, o, &[0x086f_0b86]);
    if name != 0 {
        m.free(name);
    }
}

/// `SI_malloc (0x080d0ee8)`: nothing for a size of 0, otherwise a block with `o` first and `k` last.
pub(crate) fn si_malloc_blk(m: &mut Mem, n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let p = m.malloc(n);
    if p != 0 {
        m.set_u8(p, 0x6f);
        m.set_u8(p + n - 1, 0x6b);
    }
    p
}

/// `strdup` of a literal in the original's read-only data (the image has none): a block of its length and a terminator.
pub(crate) fn strdup_lit(m: &mut Mem, lit: &[u8]) -> u32 {
    let p = m.malloc(lit.len() as u32 + 1);
    m.set_bytes(p, lit);
    m.set_u8(p + lit.len() as u32, 0);
    p
}

const W_8829F00: u32 = 0x0882_9f00; // char*: the value of SI_MI_INTERPOLATION
const W_8829F04: u32 = 0x0882_9f04; // read once
const EXTENSION: u32 = 0x0882_9f20; // char*: the picture file extension

/// What the environment variable `SI_MI_INTERPOLATION` says: `yes`, `1`, `on` true, `no`, `0`, `off` false, anything else
/// (or unset) the global at 0x08791078. The inlined comparisons of the original are `strcmp`s against the literals.
fn mi_interpolation(m: &mut Mem, o: &mut dyn Outside) -> bool {
    if m.u32(W_8829F04) == 0 {
        let v = o.call(m, ext::GETENV, &[0x086f_0b91]).eax;
        m.set_u32(W_8829F00, v);
        m.set_u32(W_8829F04, 1);
    }
    let p = m.u32(W_8829F00);
    if p != 0 {
        let s = m.cstr(p);
        if s == b"yes" || s == b"1" || s == b"on" {
            return true;
        }
        if s == b"no" || s == b"0" || s == b"off" {
            return false;
        }
    }
    m.u16(0x0879_1078) != 0
}

/// `print_camera (0x085839f4)`: the `camera` block of the scene: the fog (layered, or the user shader's or the default's),
/// the output depth-of-field shader, the motion-blur shader, the user shaders for extra output buffers and lenses, the
/// camera statement itself (frame, outputs, focal, aperture, aspect, resolution, window, clip), and the camera instance.
/// Like `print_view`, the arithmetic is the x87's and `tan` is the C library's. The stack buffers are the original's sizes.
/// [soft 0x085839f4 print_camera]
pub fn print_camera(m: &mut Mem, o: &mut dyn Outside, rp: u32, cam: u32, mat: u32, field: u32) {
    use si_x87::{trig, Round, F80};
    let f = F80::from_f32;
    let dict = m.u32(rp);
    let frame = m.u32(g::FRAME);
    let buf_path = m.local(0x2000);
    let b2200 = m.local(0x200);
    let b2300 = m.local(0x100);
    let b2400 = m.local(0x100);
    let b2438 = m.local(0x20);
    let mut volume = 0u32;
    let mut out_dof = 0u32;
    let mut arr1 = 0u32;
    let mut lens_dof = 0u32;
    let mut arr2 = 0u32;
    let mut cursor;
    let mut plane = 0.0f32;
    let mut radius = 0.0f32;
    let interp = mi_interpolation(m, o);
    let prefix = if m.u32(g::INCREMENTAL) != 0 { strdup_lit(m, b"incremental ") } else { strdup_lit(m, b"") };
    let name = o.call(m, ext::SI_BUILD_PIC_FILE_NAME, &[m.u32(rp + 0x188), frame, s16(m, rp + 0x148), field, 0]).eax;
    // strncpy(buf, name, 0x1fff) pads with zeros up to the count
    let text = m.cstr(name);
    let n = text.len().min(0x1fff);
    m.set_bytes(buf_path, &text[..n]);
    for i in n..0x1fff {
        m.set_u8(buf_path + i as u32, 0);
    }
    m.set_u8(buf_path + 0x1fff, 0);
    o.call(m, ext::SI_MRFILE_CONVERT_TO_UNIX, &[buf_path, buf_path, 0x2000]);
    if f(1.0).gt(f(m.f32(cam + 0x70))) {
        m.set_f32(cam + 0x70, 1.0);
    }
    if f(m.f32(cam + 0x40)).lt(F80::from_f64(0.0001)) {
        m.set_f32(cam + 0x40, f32::from_bits(0x38d1_b717));
    }
    let soft_prefix = m.u32(0x086e_f9b8);
    let pair = |m: &Mem, a: u32| f32_as_f64_words(m, a);
    let mut last_fmt = None;
    if m.u16(0x0879_0e3c) != 0 {
        sprintf_lit(m, b2200, b"\"%s_layered_fog0\"", &[soft_prefix]);
        volume = clib::strdup(m, b2200);
        fprintf(m, o, &[0x086f_0be0, prefix, b2200, soft_prefix]);
        for (fmt, a) in [(0x086f_0c00u32, 0x0879_0e4cu32), (0x086f_0c0f, 0x0879_0e50), (0x086f_0c1d, 0x0879_0e44), (0x086f_0c2b, 0x0879_0e48), (0x086f_0c3d, 0x0879_0e40)] {
            let w2 = pair(m, a);
            fprintf(m, o, &[fmt, w2[0], w2[1]]);
        }
        let (r, gg, b) = (pair(m, 0x0879_0e64), pair(m, 0x0879_0e68), pair(m, 0x0879_0e6c));
        fprintf(m, o, &[0x086f_0c4d, r[0], r[1], gg[0], gg[1], b[0], b[1]]);
        last_fmt = Some(0x086f_0c63u32);
    } else if m.u16(0x0879_0ce0) != 0 {
        let (a, b) = (m.f32(0x0879_0ce4), m.f32(0x0879_0cec));
        if F80::from_f64(1e-6).gt(f(b) - f(a)) {
            m.set_f32(0x0879_0cec, (f(a) + 1e-6f64).to_f32());
        }
        let udf = m.u32(0x0879_102c);
        let en = udf != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0;
        let si = if en { o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 } else { 1 };
        if en {
            if o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, b2300, 0x100]).eax as u16 != 0 {
                let decl = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[udf, b2400, 0x100]).eax as u16 != 0;
                o.call(m, ext::BUILD_SHADER_NAME, &[b2200, if decl { b2400 } else { b2300 }, 0]);
                volume = clib::strdup(m, b2200);
                fprintf(m, o, &[0x086f_0c67, prefix, b2200, b2300]);
            }
        } else {
            sprintf_lit(m, b2200, b"\"%s_fog0\"", &[soft_prefix]);
            volume = clib::strdup(m, b2200);
            fprintf(m, o, &[0x086f_0c83, prefix, b2200, soft_prefix]);
        }
        if si != 0 {
            if en {
                fprintf(m, o, &[0x086f_0c9b]);
            }
            let pick = |m: &Mem, x: u32, alt: u32| if F80::from_f32(m.f32(x)).eq(F80::ZERO) { alt } else { x };
            for (fmt, x, alt) in [(0x086f_0cacu32, 0x0879_0ce8u32, 0x0879_0ce4u32), (0x086f_0cbe, 0x0879_0cf0, 0x0879_0cec), (0x086f_0cd0, 0x0879_0cf4, 0x0879_0d10)] {
                let src = pick(m, x, alt);
                let w2 = pair(m, src);
                fprintf(m, o, &[fmt, w2[0], w2[1]]);
            }
            let (a, b, c) = (pair(m, 0x0879_0cf8), pair(m, 0x0879_0cfc), pair(m, 0x0879_0d00));
            fprintf(m, o, &[0x086f_0ce2, a[0], a[1], b[0], b[1], c[0], c[1]]);
            if en {
                fprintf(m, o, &[0x086f_0cf8]);
            }
        }
        if en {
            if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
                fprintf(m, o, &[0x086f_0cfb]);
            }
            let fc = m.u32(g::SI_FRAME_CURRENT);
            o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
        }
        last_fmt = Some(0x086f_0cfeu32);
    }
    if let Some(fmt) = last_fmt {
        fprintf(m, o, &[fmt]);
    }
    let w = m.i16(0x0879_117a) as i32;
    let h = m.i16(0x0879_117c) as i32;
    let pa = m.f32(0x0879_0d34);
    let (c40, c70) = (m.f32(cam + 0x40), m.f32(cam + 0x70));
    let mut aperture: f64 = if m.u16(cam + 0x108) != 0 {
        (f(c70) * m.f32(cam + 0x110) / m.f32(cam + 0x10c)).to_f64()
    } else {
        let t = trig::tan((f(c40) * 0.5f32).to_f64()).to_f64();
        (F80::from_i32(w) * (f(c70) + f(c70)) / F80::from_i32(h) * t * pa).to_f64()
    };
    if m.u16(0x086e_f9cc) != 0 {
        aperture = (f(m.f32(cam + 0x40)) * 1.175f64).to_f64();
    }
    let env = o.call(m, ext::GETENV, &[0x086f_0d03]).eax;
    let mode = m.u32(cam + 0x6c);
    if env != 0 {
        if mode == 2 {
            sprintf_lit(m, b2200, b"\"out_depth_of_field0\"", &[]);
            out_dof = clib::strdup(m, b2200);
            radius = (F80::from_f32(m.f32(cam + 0x98)) / F80::from_i32(w) * aperture).to_f32();
            fprintf(m, o, &[0x086f_0d40, prefix, out_dof]);
            let a = pair(m, cam + 0x7c);
            fprintf(m, o, &[0x086f_0d64, a[0], a[1]]);
            let a = pair(m, cam + 0x84);
            fprintf(m, o, &[0x086f_0d72, a[0], a[1]]);
            let r = f64_words(radius as f64);
            fprintf(m, o, &[0x086f_0d7f, r[0], r[1]]);
            let v = (f(m.f32(cam + 0x98)) * 0.5f32).to_i32(Round::Zero);
            fprintf(m, o, &[0x086f_0d8e, v as u32]);
        } else if mode == 1 {
            sprintf_lit(m, b2200, b"\"out_depth_of_field0\"", &[]);
            out_dof = clib::strdup(m, b2200);
            radius = (F80::from_f32(0.5) * m.f32(cam + 0x70) / m.f32(cam + 0x94) * 0.0032834f64).to_f32();
            fprintf(m, o, &[0x086f_0dc0, prefix, out_dof]);
            let a = pair(m, cam + 0x74);
            fprintf(m, o, &[0x086f_0de4, a[0], a[1]]);
            fprintf(m, o, &[0x086f_0df2, a[0], a[1]]);
            let r = f64_words(radius as f64);
            fprintf(m, o, &[0x086f_0dff, r[0], r[1]]);
            let v = (F80::from_f32(0.5) * m.f32(cam + 0x98)).to_i32(Round::Zero);
            fprintf(m, o, &[0x086f_0e0e, v as u32]);
        }
    }
    if m.u16(0x0879_107a) != 0 && m.u16(0x0879_107c) == 1 {
        fprintf(m, o, &[0x086f_0e20, prefix]);
        let a = pair(m, 0x0879_0e8c);
        fprintf(m, o, &[0x086f_0e47, a[0], a[1]]);
        fprintf(m, o, &[0x086f_0e57, m.i16(0x0879_107e) as i32 as u32]);
        let v = m.u32(0x0879_1080);
        fprintf(m, o, &[0x086f_0e64, v]);
    }
    let fc = m.u32(g::SI_FRAME_CURRENT);
    let n1 = m.i32(0x0879_0fc4);
    cursor = 0;
    if n1 > 0 {
        arr1 = si_malloc_blk(m, (n1 as u32) << 2);
        cursor = arr1;
        let n = m.u32(0x0879_0fc4) << 2;
        clib::memset(m, arr1, 0, n);
    }
    let mut i = 0i32;
    while i < m.i32(0x0879_0fc4) {
        m.set_u32(cursor, 0);
        let udf = m.u32(m.u32(0x0879_0fcc) + i as u32 * 4);
        if udf != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0 {
            let udf = m.u32(m.u32(0x0879_0fcc) + i as u32 * 4);
            if m.u16(udf + 0x10) == 0 && o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, b2300, 0x100]).eax as u16 != 0 {
                let decl = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[udf, b2400, 0x100]).eax as u16 != 0;
                o.call(m, ext::BUILD_SHADER_NAME, &[b2200, if decl { b2400 } else { b2300 }, i as u32]);
                let dup = clib::strdup(m, b2200);
                m.set_u32(cursor, dup);
                fprintf(m, o, &[0x086f_0e77, prefix, b2200, b2300]);
                o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
                fprintf(m, o, &[0x086f_0e89]);
            }
        }
        i += 1;
        cursor = cursor.wrapping_add(4);
    }
    if env == 0 {
        if mode == 2 {
            sprintf_lit(m, b2200, b"\"lens_depth_of_field0\"", &[]);
            lens_dof = clib::strdup(m, b2200);
            plane = (-(f(m.f32(cam + 0x84)) + m.f32(cam + 0x7c)) * 0.5f64).to_f32();
            radius = (F80::from_f32(m.f32(cam + 0x98)) / F80::from_i32(w) * aperture).to_f32();
            let (a, b) = (f64_words(plane as f64), f64_words(radius as f64));
            fprintf(m, o, &[0x086f_0ec0, prefix, b2200, a[0], a[1], b[0], b[1]]);
        } else if mode == 1 {
            sprintf_lit(m, b2200, b"\"lens_depth_of_field0\"", &[]);
            lens_dof = clib::strdup(m, b2200);
            plane = -m.f32(cam + 0x74);
            radius = (F80::from_f32(0.5) * m.f32(cam + 0x70) / m.f32(cam + 0x94) * 0.0032834f64).to_f32();
            let (a, b) = (f64_words(plane as f64), f64_words(radius as f64));
            fprintf(m, o, &[0x086f_0f20, prefix, b2200, a[0], a[1], b[0], b[1]]);
        }
    }
    let n2 = m.i32(0x0879_0fc8);
    if n2 > 0 {
        arr2 = si_malloc_blk(m, (n2 as u32) << 2);
        cursor = arr2;
    }
    let mut i = 0i32;
    while i < m.i32(0x0879_0fc8) {
        m.set_u32(cursor, 0);
        let udf = m.u32(m.u32(0x0879_0fd0) + i as u32 * 4);
        if udf != 0 && o.call(m, ext::SI_UDF_IS_ENABLED, &[udf]).eax as u16 != 0 {
            let udf = m.u32(m.u32(0x0879_0fd0) + i as u32 * 4);
            if m.u16(udf + 0x10) == 0 && o.call(m, ext::SI_UDF_GET_SHADER_NAME, &[udf, b2300, 0x100]).eax as u16 != 0 {
                let decl = o.call(m, ext::SI_UDF_GET_SHADER_FUNC_DECL_NAME, &[udf, b2400, 0x100]).eax as u16 != 0;
                o.call(m, ext::BUILD_SHADER_NAME, &[b2200, if decl { b2400 } else { b2300 }, i as u32]);
                let dup = clib::strdup(m, b2200);
                m.set_u32(cursor, dup);
                fprintf(m, o, &[0x086f_0f5f, prefix, b2200, b2300]);
                if o.call(m, ext::SI_UDF_USE_DEFAULT, &[udf]).eax as u16 != 0 {
                    fprintf(m, o, &[0x086f_0f71]);
                    let a = f64_words(plane as f64);
                    fprintf(m, o, &[0x086f_0f82, a[0], a[1]]);
                    let a = f64_words(radius as f64);
                    fprintf(m, o, &[0x086f_0f91, a[0], a[1]]);
                }
                o.call(m, ext::SI_UDF_OUTPUT_TO_MI, &[dict, udf, fc]);
                fprintf(m, o, &[0x086f_0fa3]);
            }
        }
        i += 1;
        cursor = cursor.wrapping_add(4);
    }
    fprintf(m, o, &[0x086f_0faf, prefix, 0x086f_0fa8]);
    let sec = st0(o.call(m, ext::SI_FRAME_TO_SEC, &[frame])).to_f64().to_bits();
    fprintf(m, o, &[0x086f_0fc5, frame, sec as u32, (sec >> 32) as u32]);
    if m.u16(g::W_8790D1C) != 0 {
        fprintf(m, o, &[0x086f_0fd2, field]);
    }
    fprintf(m, o, &[0x086f_0fdd]);
    if volume != 0 {
        fprintf(m, o, &[0x086f_0fdf, volume]);
        m.free(volume);
    }
    if out_dof != 0 {
        fprintf(m, o, &[0x086f_0ff0, if interp { 0x086f_0fed } else { 0x086f_0fee }, out_dof]);
        m.free(out_dof);
    }
    if arr1 != 0 {
        cursor = arr1;
        let mut i = 0i32;
        while i < m.i32(0x0879_0fc4) {
            if m.u32(cursor) != 0 {
                let udf = m.u32(m.u32(0x0879_0fcc) + i as u32 * 4);
                let flags = o.call(m, ext::SI_UDF_GET_OUTPUT_BUFFER_FLAGS, &[udf]).eax;
                m.set_bytes(b2438, if interp { b"rgba\0" } else { b"-rgba\0" });
                if flags & 0x10 != 0 {
                    strcat_bytes(m, b2438, b"_fp");
                } else if flags & 8 != 0 {
                    strcat_bytes(m, b2438, b"_16");
                }
                if flags & 1 != 0 {
                    strcat_bytes(m, b2438, b",z");
                }
                if flags & 4 != 0 {
                    strcat_bytes(m, b2438, b",n");
                }
                if flags & 2 != 0 {
                    strcat_bytes(m, b2438, b",tag");
                }
                let e = m.u32(cursor);
                fprintf(m, o, &[0x086f_1027, b2438, e]);
                m.free(m.u32(cursor));
            }
            i += 1;
            cursor = cursor.wrapping_add(4);
        }
        if arr1 != 0 {
            m.free(arr1);
        }
    }
    if m.u16(0x0879_107a) != 0 && m.u16(0x0879_107c) == 1 {
        fprintf(m, o, &[0x086f_1040, if interp { 0x086f_103b } else { 0x086f_103c }]);
    }
    if lens_dof != 0 {
        fprintf(m, o, &[0x086f_1066, lens_dof]);
        m.free(lens_dof);
    }
    if arr2 != 0 {
        cursor = arr2;
        let mut i = 0i32;
        while i < m.i32(0x0879_0fc8) {
            let e = m.u32(cursor);
            if e != 0 {
                fprintf(m, o, &[0x086f_1073, e]);
                m.free(m.u32(cursor));
            }
            i += 1;
            cursor = cursor.wrapping_add(4);
        }
        if arr2 != 0 {
            m.free(arr2);
        }
    }
    let kind = m.u32(0x0879_106c);
    let suffix = o.call(m, ext::SI_GET_RENDER_IMG_SUFFIX, &[kind]).eax;
    m.set_u32(EXTENSION, suffix);
    let mut ext_fmt = 0u32;
    let pick: Option<(&[u8], &[u8])> = match m.u32(0x0879_106c) {
        0 => Some((b"\"rgba\"", b"\"-rgba\"")),
        1 => Some((b"\"rgba\"", b"\"-rgba\"")),
        2 => Some((b"\"rgba_16\"", b"\"-rgba_16\"")),
        3 => Some((b"\"rgb\"", b"\"-rgb\"")),
        4 => Some((b"\"rgb\"", b"\"-rgb\"")),
        _ => None,
    };
    if let Some((yes, no)) = pick {
        ext_fmt = strdup_lit(m, if interp { yes } else { no });
    }
    let ext_now = m.u32(EXTENSION);
    if m.u32(0x0879_106c) == 2 {
        fprintf(m, o, &[0x086f_10ce, if ext_fmt == 0 { 0x086f_10cd } else { ext_fmt }, buf_path, ext_now]);
    } else {
        fprintf(m, o, &[0x086f_10e8, if ext_fmt == 0 { 0x086f_10e7 } else { ext_fmt }, ext_now, buf_path, ext_now]);
    }
    if ext_fmt != 0 {
        m.free(ext_fmt);
    }
    let e = m.u32(EXTENSION);
    if e != 0 {
        m.free(e);
        m.set_u32(EXTENSION, 0);
    }
    if m.u16(0x0879_0fbe) != 0 {
        fprintf(m, o, &[0x086f_1101, buf_path]);
    }
    if m.u16(0x0879_0fc0) != 0 {
        fprintf(m, o, &[0x086f_111b, buf_path]);
    }
    if m.u16(0x086e_f9cc) != 0 {
        fprintf(m, o, &[0x086f_1133]);
    } else {
        let a = pair(m, cam + 0x70);
        fprintf(m, o, &[0x086f_1144, a[0], a[1]]);
    }
    let a = f64_words(aperture);
    fprintf(m, o, &[0x086f_114f, a[0], a[1]]);
    if m.u16(cam + 0x108) != 0 {
        let v = (f(m.f32(cam + 0x110)) / m.f32(cam + 0x114)).to_f64();
        let a = f64_words(v);
        fprintf(m, o, &[0x086f_115d, a[0], a[1]]);
    } else {
        let v = (f(pa) * (F80::from_i32(w) / F80::from_i32(h))).to_f64();
        let a = f64_words(v);
        fprintf(m, o, &[0x086f_1169, a[0], a[1]]);
    }
    fprintf(m, o, &[0x086f_1175, w as u32, h as u32]);
    if m.u32(0x0879_0e04) != 0 {
        let (wf, hf, c) = (F80::from_i32(w), F80::from_i32(h), F80::from_f32(0.5));
        let ia = (f(m.f32(0x0879_0e0c)) * wf + c).to_i32(Round::Zero);
        let edi = (wf * (F80::from_i32(1) - m.f32(0x0879_0e10)) + c).to_i32(Round::Zero);
        let esi = ((F80::from_i32(1) - m.f32(0x0879_0e14)) * hf + c).to_i32(Round::Zero);
        let ebx = (c + hf * m.f32(0x0879_0e18)).to_i32(Round::Zero);
        fprintf(m, o, &[0x086f_1188, ia as u32, ebx as u32, edi as u32, esi as u32]);
        o.call(m, ext::SDEBUG, &[0x086f_119d, ia as u32, ebx as u32, edi as u32, esi as u32]);
    }
    let near = if F80::from_f64(0.0001).lt(f(m.f32(cam + 0x34))) { (m.f32(cam + 0x34) as f64).to_bits() } else { 0.0001f64.to_bits() };
    let far = pair(m, cam + 0x38);
    fprintf(m, o, &[0x086f_11b4, near as u32, (near >> 32) as u32, far[0], far[1]]);
    fprintf(m, o, &[0x086f_11c1]);
    o.call(m, ext::PRINT_INSTANCE, &[0x086f_11cf, 0, mat]);
    let grp = o.call(m, ext::PRINT_GROUP, &[0x086f_11d6, 0, 0, 0]).eax;
    m.free(grp);
}

/// `sprintf(buf, fmt, ..)` with a format that is a literal of the original's read-only data.
pub(crate) fn sprintf_lit(m: &mut Mem, buf: u32, fmt: &[u8], words: &[u32]) {
    let text = crate::cprintf::format_bytes(m, fmt, words);
    m.put_cstr(buf, &text);
}

/// `si_dumpUsageInfo (0x08581da8)`: the usage text on stderr with the program name, then `exit(2)`.
/// [soft 0x08581da8 si_dumpUsageInfo]
pub fn si_dump_usage_info(m: &mut Mem, o: &mut dyn Outside) {
    const USAGE: &[u8] = b"Usage: %s [options] Softimage_scene_file\nOptions:\n\t-v\tput verbose flag into output file\n\t-D\tprint debugging information during translation\n\t-V\tuse 'aux' shaders, rather than 'soft' shaders\n\t-l\tuse latest Softimage scene file version\n\t-u\tugly but fast render, suppress oversampling and filtering\n\t-o SI_P\tprint mi file to file SI_P (default is \"out\"), unless -p is given\n\t-o -\tprint mi file to stdout (default is \"out\"), unless -p is given\n\t-d D\tuse SI_database D\n\t-r R\tuse resource directory R\n\t-t T\tevaluate scene at time T\n\t-f F\tevaluate scene at frame F (-t and -f are mutually exclusive)\n\t-n N\tevaluate N frames of scene, and put .XXXX into filename\n\t-i I\tif -n is used, frame increment I (default 1, consecutive frames)\n\t-x SI_X\timage file extension SI_X (rla, rlb, qnt576, alias etc; default is pic)\n\t-p SI_P\tpipe generated mi file to program SI_P, rather than printing to stdout\n";
    let name = m.u32(0x086e_f9ac);
    let t = crate::cprintf::format_bytes(m, USAGE, &[name]);
    o.print(2, &t);
    o.call(m, ext::EXIT, &[2]);
}
