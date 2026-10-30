// SPDX-License-Identifier: MIT
//! last1: `SI_dbLoadScenePIPE` (`dbloadsave.c`): the scene is read from the pipe a custom command was handed
//! (finding 0025, section "last1"). Pure orchestration, so it is written on the image and reaches the dictionary and
//! the loader by address through [`ByAddr`].

use crate::edtools::{ByAddr, SI_ELEMDICT_GET_ELEM};
use si_mem::Mem;

/// `SI_elemdictAddElem (0x08132a5c)`: (dictionary, chapter, node, object, aux, 0, 0).
const SI_ELEMDICT_ADD_ELEM: u32 = 0x0813_2a5c;
/// `SI_elemdictCheckEmptyDescription (0x08136db0)`.
const SI_ELEMDICT_CHECK_EMPTY_DESCRIPTION: u32 = 0x0813_6db0;
/// `SI_dbScnGetListNode (0x081319f0)`: (name, 0, 0).
const SI_DB_SCN_GET_LIST_NODE: u32 = 0x0813_19f0;
/// `SI_dbLoadElementsFromDict (0x080d9924)`: (dictionary, input database, scene flag, latest flag, untold flag).
const SI_DB_LOAD_ELEMENTS_FROM_DICT: u32 = 0x080d_9924;
/// `SI_pDbiin` (0x08782058): the input database's information.
const SI_P_DBIIN: u32 = 0x0878_2058;
/// `SI_pipedFiles` (0x086ceba8): set while the files are read from the pipe.
const SI_PIPED_FILES: u32 = 0x086c_eba8;
/// `SI_cid.SCENES` (0x0878166c): the scenes chapter.
const CID_SCENES: u32 = 0x0878_166c;
/// A short at 0x08781596: while it is 0 the descriptions are asked for (the post-definition flag of `edfirst`).
const POST_DEF_FLAG: u32 = 0x0878_1596;
/// A short at 0x087fb99c, set to 1 here (the loader's "scene being loaded" mark).
const LOADING_MARK: u32 = 0x087f_b99c;
/// The text at 0x0867d8f6: the name of the scene node the loader finds (`SI_dbScnGetListNode`).
const SCENE_NAME: u32 = 0x0867_d8f6;

/// [soft 0x080da5fc SI_dbLoadScenePIPE]: `dict` is the element dictionary; the pipe-files flag is raised, empty descriptions
/// are checked, the descriptions are read (the loader's last argument, 1) when the dictionary's short at +0x6e is not 0 or the
/// post-definition flag is 0, the loading mark is set, the scenes chapter gets an element for the scene node and its short at
/// +0x46 is set to 1, `SI_dbLoadElementsFromDict(dict, SI_pDbiin, 1, 0, flag)` loads, the pipe-files flag is lowered and the
/// loader's answer returned as a short.
pub fn db_load_scene_pipe<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, dict: u32) -> i32 {
    let dbi = m.u32(SI_P_DBIIN);
    m.set_u16(SI_PIPED_FILES, 1);
    o.call_addr(m, SI_ELEMDICT_CHECK_EMPTY_DESCRIPTION, &[dict]);
    let flag = (m.i16(dict + 0x6e) != 0 || m.i16(POST_DEF_FLAG) == 0) as u32;
    m.set_u16(LOADING_MARK, 1);
    let node = o.call_addr(m, SI_DB_SCN_GET_LIST_NODE, &[SCENE_NAME, 0, 0]);
    let chapter = m.u32(CID_SCENES);
    let id = o.call_addr(m, SI_ELEMDICT_ADD_ELEM, &[dict, chapter, node, 0, 0, 0, 0]);
    let chapter = m.u32(CID_SCENES);
    let e = o.call_addr(m, SI_ELEMDICT_GET_ELEM, &[dict, chapter, id]);
    m.set_u16(e + 0x46, 1);
    let r = o.call_addr(m, SI_DB_LOAD_ELEMENTS_FROM_DICT, &[dict, dbi, 1, 0, flag]);
    m.set_u16(SI_PIPED_FILES, 0);
    r as i16 as i32
}
