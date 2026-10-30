// SPDX-License-Identifier: MIT
//! `sys_render.c`: the start-up of the stand-alone renderer (the one function of the file;
//! finding 0018, section "Render set-up: the display side").
//!
//! **Compared with the emulated original**: run against the original stepped through in
//! the emulation harness, every callee of another source file a recorded stand-in
//! (`re/tools/si-rensetup/emu_gen_pass6.py`, `rensetup-emucheck`).

use super::globals::*;
use super::outside::*;
use si_mem::Mem;

/// Sets the stand-alone renderer up [soft 0x0842f730 SI_initStandAloneRender]; `name` is
/// the directory (a text) the scene's database is named by. Answers 1, or 0 when the input
/// database cannot be made.
///
/// 1. `SI_standaloneRenderer` = 1. The text `<name>/soft.init` is formatted into the
///    string buffer at 0x0881ade0; when `access(that, 0)` answers 0 the command `csh
///    <name>/soft.init` is formatted into the same buffer and handed to `SI_system`.
/// 2. `SI_historySetActive(0)`; `SI_initDictionaries(name, 1, 1)`; without an input
///    database (`SI_pDbiin`) the answer is 0.
/// 3. The working directory is `getcwd(NULL, 255)` (kept in `SI_cwd`) and the object and
///    texture paths copies of it. With an output database the chapter RENDER_PICTURES is
///    set, the file paths built, and the picture path is a copy of the chapter's directory
///    (the word at +0x14 of the database) when `SI_dbChapterCheckAccess(database, 2)`
///    answers a non-zero 16-bit value, else a copy of the working directory (which is also
///    what it is without an output database). The material, texture and palette
///    libraries are copies of the working directory too.
/// 4. `SI_initDictionaries(name, 1, 1)` a second time (the original repeats the call), and
///    again no input database means 0.
/// 5. `SI_initWave`, `SI_grafListCreate`, `SI_grafListAdd`; when the last answers a positive
///    index the scene's graf list `SI_scngl` is the array `SI_grafLists` at that index
///    (0x24 bytes each). A camera `Perspective_def` (`SI_newCamera(name, 1)`) becomes the
///    process record's camera and is added to the process record's graf list with kind 4
///    (`SI_graflistAddData`). `SI_animationInfoInit`, `SI_matterInit`, `SI_metaballInit`,
///    `SI_cusInit`.
pub fn init_stand_alone_render<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, name: u32) -> u32 {
    m[SI_STANDALONE_RENDERER] = 1;
    let text = m.cstr(name);
    let mut line = text.clone();
    line.extend_from_slice(b"/soft.init");
    m.put_cstr(STRING_BUFFER, &line);
    if o.call(m, ACCESS, &[STRING_BUFFER, 0]) == 0 {
        let mut command = b"csh ".to_vec();
        command.extend_from_slice(&text);
        command.extend_from_slice(b"/soft.init");
        m.put_cstr(STRING_BUFFER, &command);
        o.call(m, SI_SYSTEM, &[STRING_BUFFER]);
    }
    o.call(m, SI_HISTORY_SET_ACTIVE, &[0]);
    o.call(m, SI_INIT_DICTIONARIES, &[name, 1, 1]);
    if m[SI_P_DBIIN] == 0 {
        return 0;
    }
    let cwd = o.call(m, GETCWD, &[0, 0xff]);
    m[SI_CWD] = cwd;
    m[SI_OBJECT_PATH] = m.strdup(cwd);
    let cwd = m[SI_CWD];
    m[SI_TEXTURE_PATH] = m.strdup(cwd);
    let out = m[SI_P_DBIOUT];
    let mut picture = m[SI_CWD];
    if out != 0 {
        o.call(m, SI_DB_SET_CHAPTER, &[out, RENDER_PICTURES]);
        let out = m[SI_P_DBIOUT];
        o.call(m, SI_DBFILEPATH_BUILD, &[out]);
        let out = m[SI_P_DBIOUT];
        if o.call(m, SI_DB_CHAPTER_CHECK_ACCESS, &[out, 2]) as u16 != 0 {
            picture = m.u32(m[SI_P_DBIOUT] + 0x14);
        }
    }
    m[SI_PICTURE_PATH] = m.strdup(picture);
    let cwd = m[SI_CWD];
    m[SI_MATERIAL_LIB] = m.strdup(cwd);
    let cwd = m[SI_CWD];
    m[SI_TEXTURE_LIB] = m.strdup(cwd);
    let cwd = m[SI_CWD];
    m[SI_PALETTE_LIB] = m.strdup(cwd);
    o.call(m, SI_INIT_DICTIONARIES, &[name, 1, 1]);
    if m[SI_P_DBIIN] == 0 {
        return 0;
    }
    o.call(m, SI_INIT_WAVE, &[]);
    o.call(m, SI_GRAF_LIST_CREATE, &[]);
    let index = o.call(m, SI_GRAF_LIST_ADD, &[]) as i32;
    if index > 0 {
        m[SI_SCNGL] = m[SI_GRAF_LISTS].0.wrapping_add((index as u32).wrapping_mul(0x24));
    }
    let camera = o.call(m, SI_NEW_CAMERA, &[PERSPECTIVE_DEF, 1]);
    let proc = m[SI_PROC_DATA];
    m[proc].camera = si_mem::Ptr32::new(camera);
    let list = m[SI_GRAF_LISTS].0.wrapping_add((m[proc].graf_list as u32).wrapping_mul(0x24));
    let camera = m[proc].camera.0;
    o.call(m, SI_GRAFLIST_ADD_DATA, &[list, camera, 4, 0]);
    o.call(m, SI_ANIMATION_INFO_INIT, &[]);
    o.call(m, SI_MATTER_INIT, &[]);
    o.call(m, SI_METABALL_INIT, &[]);
    o.call(m, SI_CUS_INIT, &[]);
    1
}
