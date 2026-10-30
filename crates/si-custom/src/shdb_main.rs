// SPDX-License-Identifier: MIT
//! shdb_main: the small entry points of the shader previewer (finding 0041): the user-interface
//! queries and the shader-kind check. The setup, start and teardown of the preview (dictionaries,
//! models, threads) are not ported yet.

use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use crate::custom_fcv::SI_ELEMDICT_GET_ELEM;
use crate::custom_init::PLT_EXIT;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_SHDB_DISPLAY_UPDATE: Routine = Routine::new("si_shdbDisplayUpdate", 0x0854_e644);
pub const SI_SHDB_DISPLAY_POPUP: Routine = Routine::new("si_shdbDisplayPopup", 0x0854_e6cc);

const B_RUNNING: u32 = 0x086e_4f08;
const B_INITIALIZED: u32 = 0x086e_4f0a;
const S_TEMP: u32 = 0x0882_4804;
/// The thread parameter block the display routines take.
const THR_TILE_READ: u32 = 0x0882_2f20;
/// The shader kind ids (`SI_cid`-style words) the check compares the shader's kind against.
const KIND_LIGHT: u32 = 0x0878_1744;
const KIND_LIGHT2: u32 = 0x0878_178c;
const KIND_LENS: u32 = 0x0878_1748;
const KIND_OUTPUT: u32 = 0x0878_174c;
const KIND_TEXTURE: u32 = 0x0878_1734;
const KIND_TEXTURE2: u32 = 0x0878_1750;
/// The texture name field of `SI_customPreviewArgs` (+0xde0) and the messages.
const TEXTURE_NAME: u32 = 0x0879_a640;
const S_NO_NAME: u32 = 0x086e_4f69;
const S_NO_LIGHT: u32 = 0x086e_4f15;
const S_NO_LENS: u32 = 0x086e_4f31;
const S_NO_OUTPUT: u32 = 0x086e_4f4c;
const P_ERR_BAD_TEXTURE: u32 = 0x086e_4d88;
const P_ERR_NO_TEXTURE: u32 = 0x086e_4d48;

/// [soft 0x0854cab0 SI_shdbUINeedRefresh]: when the previewer runs and its display was marked for
/// refresh, redraws it and clears the mark; answers 1 then, else 0.
pub fn si_shdb_ui_need_refresh(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    if m.u16(B_RUNNING) == 0 {
        return 0;
    }
    let t = m.u32(S_TEMP);
    if m.u16(t + 4) == 0 {
        return 0;
    }
    o.call(m, SI_SHDB_DISPLAY_UPDATE, &[THR_TILE_READ]);
    let t = m.u32(S_TEMP);
    m.set_u16(t + 4, 0);
    1
}

/// [soft 0x0854caf8 SI_shdbUIActive]: 1 when the previewer runs and is initialised.
pub fn si_shdb_ui_active(m: &Mem) -> i32 {
    (m.u16(B_RUNNING) != 0 && m.u16(B_INITIALIZED) != 0) as i32
}

/// [soft 0x0854cb1c SI_shdbUIDoPopup]: opens the display's popup while the previewer runs.
pub fn si_shdb_ui_do_popup(m: &mut Mem, o: &mut dyn Outside) {
    if m.u16(B_RUNNING) != 0 {
        o.call(m, SI_SHDB_DISPLAY_POPUP, &[THR_TILE_READ]);
    }
}

/// [soft 0x0854cebc SI_shdbValidate]: 1 when the shader at `p` (kind word at +4) can be previewed; 0
/// with a status message (level 3) for lights, lenses, output shaders and for textures without a name
/// (none set or "NO_NAME").
pub fn si_shdb_validate(m: &mut Mem, o: &mut dyn Outside, p: u32) -> i32 {
    if p == 0 {
        return 0;
    }
    let kind = m.u32(p + 4);
    let msg = if kind == m.u32(KIND_LIGHT) || kind == m.u32(KIND_LIGHT2) {
        S_NO_LIGHT
    } else if kind == m.u32(KIND_LENS) {
        S_NO_LENS
    } else if kind == m.u32(KIND_OUTPUT) {
        S_NO_OUTPUT
    } else if kind == m.u32(KIND_TEXTURE) || kind == m.u32(KIND_TEXTURE2) {
        let name = m.u32(TEXTURE_NAME);
        if name == 0 {
            m.u32(P_ERR_BAD_TEXTURE)
        } else if clib::strcmp(m, name, S_NO_NAME) == 0 {
            m.u32(P_ERR_NO_TEXTURE)
        } else {
            return 1;
        }
    } else {
        return 1;
    };
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[msg, 3]);
    0
}

const SHDB_DICT: u32 = 0x0882_3120;
const SHDB_MDL: u32 = 0x086e_4f0c;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const VIEW: u32 = 0x0882_3000;
const FUNC_PREVIEW: u32 = 0x0882_2fe4;
const FUNC_ABORT: u32 = 0x0882_2fe8;
const H_DYNALINK: u32 = 0x086e_4ef0;
const CUSTOM_PREVIEW_ARGS: u32 = 0x0879_9860;
const THR_TRANSLATOR: u32 = 0x0882_2f80;
const SZ_ERR_PARSE: u32 = 0x086e_4cd0;
const SZ_WRN_PARSE: u32 = 0x086e_4d14;
const SZ_ERR_NO_ENV_VAR: u32 = 0x086e_4de0;
const SZ_ERR_BAD_ENV_VAR: u32 = 0x086e_4e84;
const SZ_ERR_BAD_SYM: u32 = 0x086e_4ecc;
const SZ_MSG_CONNECT: u32 = 0x086e_4eec;

pub const SI_DELETE_RENDER_DICTIONARY: Routine = Routine::new("SI_deleteRenderDictionary", 0x082c_56e0);
pub const SI_SHDB_CTRL_START: Routine = Routine::new("SI_shdbCtrlStart", 0x0854_cf80);
pub const SI_SHDB_TRANSLATOR_PROC_PROTECTED: Routine = Routine::new("si_shdbTranslatorProc_Protected", 0x0854_d69c);
pub const SI_SHDB_VALIDATE: Routine = Routine::new("SI_shdbValidate", 0x0854_cebc);
pub const SI_SHDB_INIT_SCENE: Routine = Routine::new("si_shdbInitScene", 0x0854_f0e0);
pub const SI_SHDB_HOOK_DYNALINK: Routine = Routine::new("si_shdbHookDynalink", 0x0854_e900);
pub const SI_SHDB_SET_OPTIONS: Routine = Routine::new("si_shdbSetOptions", 0x0854_f300);
pub const SI_STR_LIST_DESTROY: Routine = Routine::new("SI_strListDestroy", 0x083c_f7fc);

/// The element dictionary of the current process (`SI_elemDicts + SI_procData->index * 0x70`).
fn current_elem_dict(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    m.u32(pd).wrapping_mul(8).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS))
}

/// [soft 0x0854d4c4 deleteShaderballDict]: removes the previewer's private render dictionary from the
/// current element dictionary and frees the dummy model (its name, then the model).
pub fn delete_shaderball_dict(m: &mut Mem, o: &mut dyn Outside) {
    let d = current_elem_dict(m);
    o.call(m, SI_DELETE_RENDER_DICTIONARY, &[d, SHDB_DICT, 0, 0]);
    let mdl = m.u32(SHDB_MDL);
    let name = m.u32(mdl.wrapping_add(4));
    if name != 0 {
        m.free(name);
        let mdl = m.u32(SHDB_MDL);
        m.set_u32(mdl.wrapping_add(4), 0);
    }
    let mdl = m.u32(SHDB_MDL);
    if mdl != 0 {
        m.free(mdl);
        m.set_u32(SHDB_MDL, 0);
    }
}

/// [soft 0x0854d170 SI_shdbCtrlParmsChanged]: a change of the shader's parameters: starts the previewer
/// when it does not run; else, once a frame was started (the first flag of the previewer's scratch record),
/// clears the record's three flags, calls the library's abort function and has the translator send the shader
/// `udf` again (command 2).
pub fn si_shdb_ctrl_parms_changed(m: &mut Mem, o: &mut dyn Outside, udf: u32) {
    if m.u16(B_RUNNING) == 0 {
        o.call(m, SI_SHDB_CTRL_START, &[udf]);
        return;
    }
    let t = m.u32(S_TEMP);
    if m.u16(t) == 0 {
        return;
    }
    m.set_u16(t.wrapping_add(2), 0);
    let t = m.u32(S_TEMP);
    m.set_u16(t, 0);
    let t = m.u32(S_TEMP);
    m.set_u16(t.wrapping_add(4), 0);
    let f = m.u32(FUNC_ABORT);
    o.call(m, Routine::at(f), &[]);
    m.set_u32(THR_TRANSLATOR + 0x28, udf);
    m.set_u32(THR_TRANSLATOR + 0x1c, 2);
    m.set_u16(THR_TRANSLATOR + 0x22, 0);
    o.call(m, SI_SHDB_TRANSLATOR_PROC_PROTECTED, &[THR_TRANSLATOR]);
}

/// Shows the `count` strings of the list `list` at `level`.
fn show_list(m: &mut Mem, o: &mut dyn Outside, list: u32, count: i32, level: u32) {
    let mut i = 0i32;
    while i < count {
        let s = m.u32(list.wrapping_add((i as u32).wrapping_mul(4)));
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, level]);
        i += 1;
    }
}

/// [soft 0x0854cb40 SI_shdbInitialize]: gets the previewer ready for the dialog `dlg` and shader `udf`
/// (neither null, and the shader previewable): hooks the preview library (messages for a missing or bad
/// environment variable, a missing symbol; "connected" otherwise), sets the scene and the options up from
/// the shader's arguments (an option error shows the error and the strings it listed; warnings are shown),
/// builds the shader ball dictionary and marks the previewer initialised.
pub fn si_shdb_initialize(m: &mut Mem, o: &mut dyn Outside, dlg: u32, udf: u32) {
    m.set_u16(B_RUNNING, 0);
    m.set_u16(B_INITIALIZED, 0);
    let t = m.u32(S_TEMP);
    m.set_u16(t.wrapping_add(2), 0);
    let t = m.u32(S_TEMP);
    m.set_u16(t.wrapping_add(4), 0);
    if dlg == 0 || udf == 0 {
        return;
    }
    if o.call(m, SI_SHDB_VALIDATE, &[udf]).eax as u16 == 0 {
        return;
    }
    let rc = o.call(m, SI_SHDB_HOOK_DYNALINK, &[H_DYNALINK, FUNC_PREVIEW, FUNC_ABORT]).eax as i32;
    let (msg, level) = match rc {
        9 => (SZ_ERR_BAD_ENV_VAR, 2),
        8 => (SZ_ERR_NO_ENV_VAR, 2),
        0xb => (SZ_ERR_BAD_SYM, 3),
        _ => (0, 0),
    };
    if msg != 0 {
        let s = m.u32(msg);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, level]);
        return;
    }
    let s = m.u32(SZ_MSG_CONNECT);
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, 0]);
    o.call(m, SI_SHDB_INIT_SCENE, &[VIEW]);
    let loc = m.local(8);
    let (cnt, list) = (loc, loc + 4);
    m.set_u32(cnt, 0);
    m.set_u32(list, 0);
    let ok = o.call(m, SI_SHDB_SET_OPTIONS, &[VIEW, udf, CUSTOM_PREVIEW_ARGS, cnt, list]).eax as u16;
    if ok == 0 {
        let s = m.u32(SZ_ERR_PARSE);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, 3]);
        let (l, c) = (m.u32(list), m.i32(cnt));
        show_list(m, o, l, c, 3);
        let (l, c) = (m.u32(list), m.u32(cnt));
        o.call(m, SI_STR_LIST_DESTROY, &[l, c]);
    } else {
        if m.i32(cnt) > 0 {
            let s = m.u32(SZ_WRN_PARSE);
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[s, 2]);
            let (l, c) = (m.u32(list), m.i32(cnt));
            show_list(m, o, l, c, 2);
        }
        let (l, c) = (m.u32(list), m.u32(cnt));
        o.call(m, SI_STR_LIST_DESTROY, &[l, c]);
        if o.call(m, CREATE_SHADERBALL_DICT, &[udf]).eax as u16 != 0 {
            m.set_u16(B_INITIALIZED, 1);
        }
    }
    m.release(8);
}

const SHDB_MDL_PLM: u32 = 0x086e_4f10;
const SHDB_MAT: u32 = 0x0882_31a0;
const SHDB_T2D: u32 = 0x0882_3f80;
const SHDB_T3D: u32 = 0x0882_4160;
const SI_RD: u32 = 0x0878_15f0;
const SZ_DUMMY_MODEL: u32 = 0x086e_4f71;
const CID_MODELS: u32 = 0x0878_1680;
const CID_MATERIALS: u32 = 0x0878_167c;
const CID_T2D: u32 = 0x0878_1768;
const CID_T3D: u32 = 0x0878_176c;
const CID_LIGHTS: u32 = 0x0878_171c;
/// The ids of the shader kinds the previewer cannot show (checked first) and of the three groups the dictionary
/// relates differently (light-like, 2D texture, 3D texture).
const NO_BALL_KINDS: [u32; 4] = [0x0878_1744, 0x0878_178c, 0x0878_1748, 0x0878_174c];
const LIGHT_KINDS: [u32; 4] = [0x0878_1730, 0x0878_1788, 0x0878_1740, 0x0878_173c];
const T2D_KINDS: [u32; 2] = [0x0878_1750, 0x0878_1734];
const KIND_T3D: u32 = 0x0878_1738;

pub const CREATE_SHADERBALL_DICT: Routine = Routine::new("createShaderballDict", 0x0854_d1ec);
pub const SI_CREATE_RENDER_DICTIONARY: Routine = Routine::new("SI_createRenderDictionary", 0x082c_5600);
pub const SI_DICT_UNSELECT_ALL_CHAPTER_ELEMS: Routine = Routine::new("SI_dictUnselectAllChapterElems", 0x082c_95d8);
pub const SI_MODEL_ALLOCATE: Routine = Routine::new("SI_modelAllocate", 0x0847_7cf4);
pub const SI_DICT_ADD_ELEM_DATA: Routine = Routine::new("SI_dictAddElemData", 0x082c_88d8);
pub const SI_MATERIAL_INITIALIZE: Routine = Routine::new("SI_materialInitialize", 0x0828_89b0);
pub const SI_ELEMDICT_ADD_UNCOMPLETE_RELATION: Routine = Routine::new("SI_elemdictAddUnCompleteRelation", 0x0813_2fb4);
pub const SI_TEXTURE_INITIALIZE: Routine = Routine::new("SI_textureInitialize", 0x082b_2508);
pub const SI_DICT_ADD_T2D_LCL_RELATIONS: Routine = Routine::new("SI_dictAddT2dLclRelations", 0x082c_8eec);
pub const SI_ELEMDICT_FIND_ELEM_BY_DATA: Routine = Routine::new("SI_elemdictFindElemByData", 0x0813_4364);
pub const SI_TEXTURE3D_INITIALIZE: Routine = Routine::new("SI_texture3DInitialize", 0x082b_8ab0);
pub const SI_DICT_ADD_T3D_LCL_RELATION: Routine = Routine::new("SI_dictAddT3dLclRelation", 0x082c_8fe4);

/// [soft 0x0854d1ec createShaderballDict]: builds the previewer's private render dictionary for the shader `udf`
/// (kind at +4): a dummy model "dummy_model" (flag 1 at +0x24 of its dictionary entry), the shader ball material
/// related to it, the shader itself, and for the 2D and 3D texture kinds the texture's local map; the shader is then
/// related to the material (or to the texture, or to the light-like id). Returns 0 for the kinds that have no ball
/// and when the model or its name cannot be made, else 1.
pub fn create_shaderball_dict(m: &mut Mem, o: &mut dyn Outside, udf: u32) -> u32 {
    let kind = m.u32(udf.wrapping_add(4));
    if NO_BALL_KINDS.iter().any(|&g| kind == m.u32(g)) {
        return 0;
    }
    let d = current_elem_dict(m);
    o.call(m, SI_CREATE_RENDER_DICTIONARY, &[d, SHDB_DICT, 0, 0]);
    let models = m.u32(CID_MODELS);
    o.call(m, SI_DICT_UNSELECT_ALL_CHAPTER_ELEMS, &[SHDB_DICT, models]);
    let mdl = o.call(m, SI_MODEL_ALLOCATE, &[0]).eax;
    m.set_u32(SHDB_MDL, mdl);
    if mdl == 0 {
        return 0;
    }
    let nm = clib::strdup(m, SZ_DUMMY_MODEL);
    let mdl = m.u32(SHDB_MDL);
    m.set_u32(mdl.wrapping_add(4), nm);
    if nm == 0 {
        m.free(mdl);
        m.set_u32(SHDB_MDL, 0);
        return 0;
    }
    let models = m.u32(CID_MODELS);
    let esi = o.call(m, SI_DICT_ADD_ELEM_DATA, &[SHDB_DICT, models, mdl]).eax;
    let kind = m.u32(udf.wrapping_add(4));
    let edi = o.call(m, SI_DICT_ADD_ELEM_DATA, &[SHDB_DICT, kind, udf]).eax;
    let models = m.u32(CID_MODELS);
    let plm = o.call(m, SI_ELEMDICT_GET_ELEM, &[SHDB_DICT, models, esi]).eax;
    m.set_u32(SHDB_MDL_PLM, plm);
    m.set_u32(plm.wrapping_add(0x24), 1);
    o.call(m, SI_MATERIAL_INITIALIZE, &[SHDB_MAT]);
    let mats = m.u32(CID_MATERIALS);
    let ebx = o.call(m, SI_DICT_ADD_ELEM_DATA, &[SHDB_DICT, mats, SHDB_MAT]).eax;
    let (models, mats) = (m.u32(CID_MODELS), m.u32(CID_MATERIALS));
    o.call(m, SI_ELEMDICT_ADD_UNCOMPLETE_RELATION, &[SHDB_DICT, SI_RD, models, mats, esi, ebx, 0x12c, 0, 0, 0, 1]);
    let kind = m.u32(udf.wrapping_add(4));
    let (mut t2d, mut t3d) = (0u32, 0u32);
    if T2D_KINDS.iter().any(|&g| kind == m.u32(g)) {
        o.call(m, SI_TEXTURE_INITIALIZE, &[SHDB_T2D]);
        o.call(m, SI_DICT_ADD_T2D_LCL_RELATIONS, &[SHDB_DICT, ebx, SHDB_T2D]);
        let c = m.u32(CID_T2D);
        t2d = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[SHDB_DICT, c, SHDB_T2D]).eax;
    } else if kind == m.u32(KIND_T3D) {
        o.call(m, SI_TEXTURE3D_INITIALIZE, &[SHDB_T3D]);
        o.call(m, SI_DICT_ADD_T3D_LCL_RELATION, &[SHDB_DICT, ebx, SHDB_T3D]);
        let c = m.u32(CID_T3D);
        t3d = o.call(m, SI_ELEMDICT_FIND_ELEM_BY_DATA, &[SHDB_DICT, c, SHDB_T3D]).eax;
    }
    let kind = m.u32(udf.wrapping_add(4));
    let rel = if LIGHT_KINDS.iter().any(|&g| kind == m.u32(g)) {
        Some((m.u32(CID_LIGHTS), ebx))
    } else if T2D_KINDS.iter().any(|&g| kind == m.u32(g)) {
        Some((m.u32(CID_T2D), t2d))
    } else if kind == m.u32(KIND_T3D) {
        Some((m.u32(CID_T3D), t3d))
    } else {
        None
    };
    if let Some((cid, from)) = rel {
        o.call(m, SI_ELEMDICT_ADD_UNCOMPLETE_RELATION, &[SHDB_DICT, SI_RD, cid, kind, from, edi, 0x226, 1, 0, 0, 1]);
    }
    1
}

const SZ_END_FRAME: u32 = 0x086e_4c8c;
const PIPE_SCN: u32 = 0x086e_4ef4;
const PIPE_SCN_B: u32 = 0x086e_4ef8;
const PIPE_IMG: u32 = 0x086e_4efc;
const PIPE_IMG_B: u32 = 0x086e_4f00;
const THRD_PREVIEW: u32 = 0x086e_4f04;
const SZ_MSG_TERMINATED: u32 = 0x086e_4f14;
const TRANSLATOR_PIPE: u32 = 0x0882_2f94;

pub const PLT_WAITPID: Routine = Routine::new("plt:waitpid", 0x080c_42f8);
pub const PLT_USLEEP: Routine = Routine::new("plt:usleep", 0x080c_37b8);
pub const SI_PIPEIO_WRITE: Routine = Routine::new("SI_pipeioWrite", 0x0842_fd68);
pub const SI_PIPEIO_DESTROY: Routine = Routine::new("SI_pipeioDestroy", 0x0842_fcfc);
pub const SI_MRTHREAD_DESTROY: Routine = Routine::new("SI_mrthreadDestroy", 0x0843_0274);
pub const SI_MRTHREAD_IS_ALIVE: Routine = Routine::new("SI_mrthreadIsAlive", 0x0843_028c);
pub const SI_SHDB_DISPLAY_PROC_CLEANUP: Routine = Routine::new("si_shdbDisplayProcCleanup", 0x0854_e838);
pub const SI_SHDB_DISPLAY_CLOSE: Routine = Routine::new("si_shdbDisplayClose", 0x0854_e61c);
pub const SI_SHDB_UNHOOK_DYNALINK: Routine = Routine::new("si_shdbUnhookDynalink", 0x0854_ea70);

pub const SI_PIPEIO_CREATE: Routine = Routine::new("SI_pipeioCreate", 0x0842_fcb0);
pub const SI_MRTHREAD_CREATE: Routine = Routine::new("SI_mrthreadCreate", 0x0843_01b8);
/// `fork` (PLT stub, 0x080c3b98): an import that the emulator does not offer; the product answers it itself.
pub const PLT_FORK: Routine = Routine::new("plt:fork", 0x080c_3b98);
/// `si_shdbDisplayProc` (0x0854e190): the tile-reading thread's body (the address is handed to `SI_mrthreadCreate`).
const SI_SHDB_DISPLAY_PROC: u32 = 0x0854_e190;

/// [soft 0x0854cf80 SI_shdbCtrlStart]: starts the previewer for the shader `udf` when it is initialised, not yet running and
/// `udf` is not null: makes the two pipe pairs (scene and picture), fills the view record (picture pipe's write end at +4, flag 1),
/// clears and fills the tile-reading thread's record (picture pipe, the process data's first three words, the window position and
/// size from the view's copies, the shader, the view, no tile yet), starts that thread on `si_shdbDisplayProc`, opens the window,
/// hands the translator record the scene pipe and the preview function, and forks. In the child the preview function (the
/// loaded library's, with the scene pipe's read end) runs and the process exits: the port returns there, since an Outside
/// `exit` does not end the call. In the parent the translator record gets the scene pipe's write end, the shader ball dictionary
/// and model, the shader, the view and the process data's first three words, command 1; the translator runs
/// (`si_shdbTranslatorProc_Protected`), the running flag is set and the scratch record's second flag (+4) cleared.
/// The caller's `fork` answer is kept in the preview-thread word (0x086e4f04), as the original does.
pub fn si_shdb_ctrl_start(m: &mut Mem, o: &mut dyn Outside, udf: u32) {
    if m.u16(B_INITIALIZED) == 0 || m.u16(B_RUNNING) != 0 || udf == 0 {
        return;
    }
    o.call(m, SI_PIPEIO_CREATE, &[PIPE_SCN, PIPE_SCN_B]);
    o.call(m, SI_PIPEIO_CREATE, &[PIPE_IMG, PIPE_IMG_B]);
    let t = THR_TILE_READ;
    m.set_u32(VIEW + 4, m.u32(PIPE_IMG_B));
    m.set_u32(VIEW, 1);
    clib::memset(m, t, 0, 0x5c);
    m.set_u32(t + 0x14, m.u32(PIPE_IMG));
    m.set_u16(t + 0x22, 0);
    m.set_u16(t + 0x24, 0);
    m.set_u32(t + 0x38, 0xffff_ffff);
    let pd = m.u32(SI_PROC_DATA);
    for k in 0..3u32 {
        m.set_u32(t + 8 + 4 * k, m.u32(pd.wrapping_add(4 * k)));
    }
    m.set_u32(t + 0x40, m.u32(VIEW + 0x34));
    m.set_u32(t + 0x44, m.u32(VIEW + 0x38));
    m.set_u32(t + 0x48, m.u32(VIEW + 0x28));
    m.set_u32(t + 0x4c, m.u32(VIEW + 0x2c));
    m.set_u32(t + 0x28, udf);
    m.set_u32(t + 0x2c, VIEW);
    let th = o.call(m, SI_MRTHREAD_CREATE, &[SI_SHDB_DISPLAY_PROC, t, 1]).eax;
    m.set_u32(t + 4, th);
    crate::shdb_display::si_shdb_display_create(m, o, t);
    m.set_u32(THR_TRANSLATOR + 0x5c, m.u32(PIPE_SCN));
    m.set_u32(THR_TRANSLATOR + 0x60, m.u32(FUNC_PREVIEW));
    let pid = o.call(m, PLT_FORK, &[]).eax;
    m.set_u32(THRD_PREVIEW, pid);
    if pid == 0 {
        let f = m.u32(FUNC_PREVIEW);
        let pipe = m.u32(PIPE_SCN);
        o.call(m, Routine::at(f), &[pipe]);
        o.call(m, PLT_EXIT, &[0]);
        return;
    }
    let tr = THR_TRANSLATOR;
    m.set_u32(tr + 0x14, m.u32(PIPE_SCN_B));
    m.set_u32(tr + 0x30, SHDB_DICT);
    m.set_u32(tr + 0x34, m.u32(SHDB_MDL_PLM));
    m.set_u16(tr + 0x24, 0);
    m.set_u32(tr + 0x28, udf);
    m.set_u32(tr + 0x2c, VIEW);
    let pd = m.u32(SI_PROC_DATA);
    for k in 0..3u32 {
        m.set_u32(tr + 8 + 4 * k, m.u32(pd.wrapping_add(4 * k)));
    }
    m.set_u16(tr + 0x22, 0);
    m.set_u32(tr + 0x1c, 1);
    o.call(m, SI_SHDB_TRANSLATOR_PROC_PROTECTED, &[tr]);
    m.set_u16(B_RUNNING, 1);
    let s = m.u32(S_TEMP);
    m.set_u16(s.wrapping_add(4), 0);
}

/// [soft 0x0854cd14 SI_shdbTerminate]: shuts the previewer down when it runs or is initialised: clears both flags
/// and the scratch record's two flags, sends the "end of frame" text to the translator's pipe, closes the pipes,
/// waits (polling with a zero sleep) until the preview process has ended, destroys its thread, tells the tile-reading
/// thread to stop (its flag at +0x22), waits until it is not alive, destroys it, cleans and closes the display, unhooks
/// the preview library, deletes the shader ball dictionary and shows the "terminated" message at level 4.
pub fn si_shdb_terminate(m: &mut Mem, o: &mut dyn Outside) {
    if m.u16(B_RUNNING) == 0 && m.u16(B_INITIALIZED) == 0 {
        return;
    }
    let t = m.u32(S_TEMP);
    m.set_u16(B_RUNNING, 0);
    m.set_u16(B_INITIALIZED, 0);
    m.set_u16(t.wrapping_add(2), 0);
    let t = m.u32(S_TEMP);
    m.set_u16(t.wrapping_add(4), 0);
    let s = m.u32(SZ_END_FRAME);
    let len = clib::strlen(m, s);
    let loc = m.local(8);
    let (written, status) = (loc, loc + 4);
    let fd = m.u32(TRANSLATOR_PIPE);
    o.call(m, SI_PIPEIO_WRITE, &[fd, s, len, written]);
    o.call(m, SI_PIPEIO_DESTROY, &[PIPE_SCN_B]);
    m.set_u32(status, 0);
    loop {
        let pid = m.u32(THRD_PREVIEW);
        if o.call(m, PLT_WAITPID, &[pid, status, 1]).eax != 0 {
            break;
        }
        o.call(m, PLT_USLEEP, &[0]);
    }
    let p = m.u32(THRD_PREVIEW);
    o.call(m, SI_MRTHREAD_DESTROY, &[p]);
    o.call(m, SI_PIPEIO_DESTROY, &[PIPE_SCN]);
    o.call(m, SI_PIPEIO_DESTROY, &[PIPE_IMG_B]);
    o.call(m, SI_PIPEIO_DESTROY, &[PIPE_IMG]);
    m.set_u16(THR_TILE_READ + 0x22, 1);
    loop {
        let th = m.u32(THR_TILE_READ + 4);
        if o.call(m, SI_MRTHREAD_IS_ALIVE, &[th]).eax as u16 == 0 {
            break;
        }
        o.call(m, PLT_USLEEP, &[0]);
    }
    let th = m.u32(THR_TILE_READ + 4);
    o.call(m, SI_MRTHREAD_DESTROY, &[th]);
    o.call(m, SI_SHDB_DISPLAY_PROC_CLEANUP, &[THR_TILE_READ]);
    o.call(m, SI_SHDB_DISPLAY_CLOSE, &[THR_TILE_READ]);
    o.call(m, SI_SHDB_UNHOOK_DYNALINK, &[H_DYNALINK]);
    delete_shaderball_dict(m, o);
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[SZ_MSG_TERMINATED, 4]);
    m.release(8);
}
