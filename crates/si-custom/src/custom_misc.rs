// SPDX-License-Identifier: MIT
//! Small files of the custom effects: `udflist.c` (the list of user effects: a pointer array and
//! its count), `custom_id.c` (the table of icon serial numbers) and `custom_trace.c` (the debug
//! trace); finding 0041, "sixth continuation".

use crate::cfmt;
use crate::custom_gen::FERROR;
use crate::fprintf;
use si_mem::{clib, Mem, Outside, Routine};

pub const SI_UDF_DISPOSE: Routine = Routine::new("SI_udfDispose", 0x0854_c3c0);
pub const SI_ELEMDICT_GET_REL_SERIAL_NUMBER: Routine = Routine::new("SI_elemdictGetRelSerialNumber", 0x0813_9b68);
pub const SI_SHD_CHAP_INFO_FROM_CHAP_ID: Routine = Routine::new("si_shdChapInfoFromChapId", 0x0854_c824);
pub const SI_UDF_ATTACH_ARRAY_ELEMENTS: Routine = Routine::new("SI_udfAttachArrayElements", 0x0854_5584);
pub const SI_ELEMDICT_EXISTS: Routine = Routine::new("SI_elemdictExists", 0x0813_7c70);
pub const SI_GRAF_LIST_EXISTS: Routine = Routine::new("SI_grafListExists", 0x0832_c144);
pub const GETENV: Routine = Routine::new("plt:getenv", 0x080c_3bb8);

/// `nb_iconId` and `iconId`: the count and the array of (key, serial) pairs.
const NB_ICON_ID: u32 = 0x086e_0220;
const ICON_ID: u32 = 0x086e_0224;
/// `si_bIsDebug` (a short) and the pointer to the name of the environment variable.
const IS_DEBUG: u32 = 0x086e_3820;
const ENV_VAR: u32 = 0x086e_383c;

/// [soft 0x0854bdf0 SI_udflistAdd]: appends `item` to the array `*arr` of `*count` entries (grown by
/// `SI_realloc`, a first block by `SI_calloc(1, 4)`), bumps the count. 1, or 0 for a null argument
/// or when out of memory.
pub fn si_udflist_add(m: &mut Mem, arr: u32, count: u32, item: u32) -> i32 {
    if arr == 0 || count == 0 || item == 0 {
        return 0;
    }
    let a = m.u32(arr);
    let n = m.u32(count);
    let p = if a == 0 { m.si_calloc(1, 4) } else { m.si_realloc(a, n.wrapping_mul(4).wrapping_add(4)) };
    if p == 0 {
        return 0;
    }
    m.set_u32(p.wrapping_add(n.wrapping_mul(4)), item);
    m.set_u32(arr, p);
    m.set_u32(count, n.wrapping_add(1));
    1
}

/// [soft 0x0854be5c SI_udflistDelete]: removes entry `index` (the entry itself is not disposed), moves the
/// later ones up and shrinks the block. 1, or 0 for a null or empty list or an index out of range.
pub fn si_udflist_delete(m: &mut Mem, arr: u32, count: u32, index: u32) -> i32 {
    if arr == 0 {
        return 0;
    }
    let a = m.u32(arr);
    if a == 0 || count == 0 || (index as i32) < 0 {
        return 0;
    }
    let n = m.u32(count);
    if (index as i32) >= n as i32 {
        return 0;
    }
    let tail = n.wrapping_sub(index).wrapping_sub(1) as i32;
    if tail > 0 {
        clib::memmove(m, a.wrapping_add(index.wrapping_mul(4)), a.wrapping_add(index.wrapping_mul(4)).wrapping_add(4), (tail as u32).wrapping_mul(4));
    }
    let n1 = n.wrapping_sub(1);
    let p = m.si_realloc(a, n1.wrapping_mul(4));
    if p == 0 {
        return 0;
    }
    m.set_u32(arr, p);
    m.set_u32(count, n1);
    1
}

/// [soft 0x0854bee4 SI_udflistMoveup]: swaps entry `index` with the one before it. 1, or 0 for a null
/// array, an empty list or an index outside 1..count.
pub fn si_udflist_moveup(m: &mut Mem, arr: u32, count: u32, index: u32) -> i32 {
    if arr == 0 || count == 0 || (index as i32) <= 0 || (index as i32) >= count as i32 {
        return 0;
    }
    let at = arr.wrapping_add(index.wrapping_mul(4));
    let (hi, lo) = (m.u32(at), m.u32(at.wrapping_sub(4)));
    m.set_u32(at, lo);
    m.set_u32(at.wrapping_sub(4), hi);
    1
}

/// [soft 0x0854bf20 SI_udflistModify]: disposes entry `index` and puts `item` in its place (the second
/// argument is not used). 1, or 0 for a negative index, a null item or a null array.
pub fn si_udflist_modify(m: &mut Mem, o: &mut dyn Outside, arr: u32, index: u32, item: u32) -> i32 {
    if (index as i32) < 0 || item == 0 || arr == 0 {
        return 0;
    }
    let old = m.u32(arr.wrapping_add(index.wrapping_mul(4)));
    o.call(m, SI_UDF_DISPOSE, &[old]);
    m.set_u32(arr.wrapping_add(index.wrapping_mul(4)), item);
    1
}

/// [soft 0x0854bf60 SI_udflistReorder]: numbers the entries 1.. in the word at +0x14 of each. 0 at the
/// first null entry, else 1.
pub fn si_udflist_reorder(m: &mut Mem, arr: u32, count: u32) -> i32 {
    let mut i: i32 = 0;
    while i < count as i32 {
        let e = m.u32(arr.wrapping_add((i as u32).wrapping_mul(4)));
        if e == 0 {
            return 0;
        }
        i += 1;
        m.set_u32(e.wrapping_add(0x14), i as u32);
    }
    1
}

/// [soft 0x0854bf94 SI_udflistCompact]: removes the null entries, moving the later ones up (the last
/// slot is cleared), and lowers the count. 1 when any was removed, else 0.
pub fn si_udflist_compact(m: &mut Mem, arr: u32, count: u32) -> i32 {
    let a = m.u32(arr);
    let mut n = m.u32(count) as i32;
    let mut removed = 0;
    let mut i: i32 = 0;
    while i < n {
        let at = |k: i32| a.wrapping_add((k as u32).wrapping_mul(4));
        if m.u32(at(i)) == 0 {
            n -= 1;
            let mut j = i;
            while j < n {
                let next = m.u32(at(j + 1));
                m.set_u32(at(j), next);
                j += 1;
            }
            m.set_u32(at(j), 0);
            removed = 1;
            continue;
        }
        i += 1;
    }
    m.set_u32(arr, a);
    m.set_u32(count, n as u32);
    removed
}

/// [soft 0x08535e4c SI_cusIconGetSerialId]: the serial number stored for `key`, -1 when none.
pub fn si_cus_icon_get_serial_id(m: &Mem, key: u32) -> i32 {
    let n = m.u32(NB_ICON_ID) as i32;
    let mut i: i32 = 0;
    while i < n {
        let t = m.u32(ICON_ID).wrapping_add((i as u32).wrapping_mul(8));
        if m.u32(t) == key {
            return m.u32(t + 4) as i32;
        }
        i += 1;
    }
    -1
}

/// [soft 0x08535dc0 SI_cusIconSetSerialId]: the serial number of `key`; a new pair (key, a fresh serial
/// from the element dictionary) is added to the table when there is none (the table is grown by
/// `SI_realloc`; -1 when out of memory, the table is then lost).
pub fn si_cus_icon_set_serial_id(m: &mut Mem, o: &mut dyn Outside, key: u32) -> i32 {
    let have = si_cus_icon_get_serial_id(m, key);
    if have >= 0 {
        return have;
    }
    let tab = m.u32(ICON_ID);
    let p = if tab == 0 { m.si_malloc(8) } else { m.si_realloc(tab, m.u32(NB_ICON_ID).wrapping_add(1).wrapping_mul(8)) };
    m.set_u32(ICON_ID, p);
    if p == 0 {
        return -1;
    }
    let n = m.u32(NB_ICON_ID);
    m.set_u32(p.wrapping_add(n.wrapping_mul(8)), key);
    let serial = o.call(m, SI_ELEMDICT_GET_REL_SERIAL_NUMBER, &[]).eax;
    let n = m.u32(NB_ICON_ID);
    let p = m.u32(ICON_ID);
    m.set_u32(p.wrapping_add(n.wrapping_mul(8)).wrapping_add(4), serial);
    m.set_u32(NB_ICON_ID, n.wrapping_add(1));
    serial as i32
}

/// [soft 0x08535e80 SI_cusIconClearSerialIdTable]: frees the table and empties it.
pub fn si_cus_icon_clear_serial_id_table(m: &mut Mem) {
    let t = m.u32(ICON_ID);
    if t != 0 {
        m.free(t);
    }
    m.set_u32(ICON_ID, 0);
    m.set_u32(NB_ICON_ID, 0);
}

/// [soft 0x085442f0 si_cusTrace]: `vfprintf(stderr, fmt, ...)` when `si_bIsDebug` is set or the
/// environment variable the program names (`getenv`) is set.
pub fn si_cus_trace(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    if m.u16(IS_DEBUG) == 0 {
        let name = m.u32(ENV_VAR);
        if o.call(m, GETENV, &[name]).eax == 0 {
            return;
        }
    }
    let text = cfmt::format(m, args[0], &args[1..]);
    o.print(2, &text);
}

/// `si_numUdf`, `si_udfList`; `SI_procData`, `SI_elemDicts`.
const NUM_UDF: u32 = 0x086e_49d8;
const UDF_LIST: u32 = 0x086e_49d4;
const PROC_DATA: u32 = 0x0867_9314;
const ELEM_DICTS: u32 = 0x0867_931c;
/// The global words that hold the chapter ids of the array-relation chapters, in the order
/// `SI_makeUDFArrayRelations` visits them.
const ARRAY_CHAPTERS: [u32; 11] = [0x0878_1730, 0x0878_173c, 0x0878_1740, 0x0878_1744, 0x0878_1748, 0x0878_174c, 0x0878_1750, 0x0878_1734, 0x0878_1738, 0x0878_1788, 0x0878_178c];

/// [soft 0x0854c760 SI_udfIsInternal]: the short at +0x10; 0 for a null effect.
pub fn si_udf_is_internal(m: &Mem, u: u32) -> i32 {
    if u == 0 { 0 } else { m.u16(u + 0x10) as i16 as i32 }
}

/// [soft 0x0854c778 SI_udfGetOrder]: the word at +0x14; 1 for a null effect.
pub fn si_udf_get_order(m: &Mem, u: u32) -> u32 {
    if u == 0 { 1 } else { m.u32(u + 0x14) }
}

/// [soft 0x0854c7cc SI_udfGetName].
pub fn si_udf_get_name(m: &Mem, u: u32) -> u32 {
    m.u32(u)
}

/// [soft 0x0854c814 SI_udfGetDatabase].
pub fn si_udf_get_database(m: &Mem, u: u32) -> u32 {
    m.u32(u + 0x20)
}

/// [soft 0x0854c908 SI_getCustomSuffix]: the suffix (+4) of the chapter's information, "???" without one.
pub fn si_get_custom_suffix(m: &mut Mem, o: &mut dyn Outside, chap_id: u32) -> u32 {
    let info = o.call(m, SI_SHD_CHAP_INFO_FROM_CHAP_ID, &[chap_id]).eax;
    if info == 0 { 0x086e_4c64 } else { m.u32(info + 4) }
}

/// [soft 0x0854ca88 SI_udfListGet]: the count and the array of the effects list into the callers' cells
/// (each only when given).
pub fn si_udf_list_get(m: &mut Mem, count: u32, list: u32) {
    if count != 0 {
        let n = m.u32(NUM_UDF);
        m.set_u32(count, n);
    }
    if list != 0 {
        let l = m.u32(UDF_LIST);
        m.set_u32(list, l);
    }
}

/// [soft 0x0854c000 SI_udfDump]: the effect, its shader information and the chapter records as text on
/// `fp` (nothing for a null effect or file).
pub fn si_udf_dump(m: &mut Mem, o: &mut dyn Outside, u: u32, fp: u32) {
    if u == 0 || fp == 0 {
        return;
    }
    let null = |s: u32, at: u32| if s == 0 { at } else { s };
    let name = null(m.u32(u), 0x086e_4a00);
    fprintf(m, o, fp, 0x086e_4a07, &[name]);
    let db = null(m.u32(u + 0x20), 0x086e_4a13);
    fprintf(m, o, fp, 0x086e_4a1a, &[db]);
    fprintf(m, o, fp, 0x086e_4a2a, &[u]);
    let v = m.u32(u + 4);
    fprintf(m, o, fp, 0x086e_4a3c, &[v]);
    let v = m.u32(u + 0x18);
    fprintf(m, o, fp, 0x086e_4a4e, &[v]);
    for (off, fmt) in [(0xc, 0x086e_4a61), (0xe, 0x086e_4a6f), (0x10, 0x086e_4a7c)] {
        let v = m.u16(u + off) as i16 as i32 as u32;
        fprintf(m, o, fp, fmt, &[v]);
    }
    let v = m.u32(u + 0x14);
    fprintf(m, o, fp, 0x086e_4a8c, &[v]);
    let d = null(m.u32(u + 0x28), 0x086e_4a99);
    fprintf(m, o, fp, 0x086e_4aa0, &[d]);
    let info = m.u32(u + 0x24);
    if info == 0 {
        return;
    }
    fprintf(m, o, fp, 0x086e_4ab5, &[info]);
    let n = null(m.u32(info + 0x1c), 0x086e_4ad1);
    fprintf(m, o, fp, 0x086e_4ad8, &[n]);
    let n = null(m.u32(info + 0x20), 0x086e_4ae7);
    fprintf(m, o, fp, 0x086e_4aee, &[n]);
    let v = m.u32(info + 0x14);
    fprintf(m, o, fp, 0x086e_4b01, &[v]);
    let v = m.u32(info + 0xc);
    fprintf(m, o, fp, 0x086e_4b16, &[v]);
    let v = m.u16(info + 4) as i16 as i32 as u32;
    fprintf(m, o, fp, 0x086e_4b2e, &[v]);
    let v = m.u16(info + 0x18) as i16 as i32 as u32;
    fprintf(m, o, fp, 0x086e_4b3d, &[v]);
    let chap = m.u32(info);
    if chap == 0 {
        return;
    }
    fprintf(m, o, fp, 0x086e_4b60, &[chap]);
    let v = m.u32(chap + 0xc);
    fprintf(m, o, fp, 0x086e_4b85, &[v]);
    let v = m.u32(chap + 8);
    fprintf(m, o, fp, 0x086e_4b93, &[v]);
    let suf = null(m.u32(chap + 4), 0x086e_4bac);
    fprintf(m, o, fp, 0x086e_4bb3, &[suf]);
    let ci = m.u32(chap);
    if ci == 0 {
        return;
    }
    fprintf(m, o, fp, 0x086e_4bc5, &[ci]);
    let n = null(m.u32(ci), 0x086e_4bde);
    fprintf(m, o, fp, 0x086e_4be5, &[n]);
    let n = null(m.u32(ci + 4), 0x086e_4bf6);
    fprintf(m, o, fp, 0x086e_4bfd, &[n]);
    let v = m.u32(ci + 8);
    fprintf(m, o, fp, 0x086e_4c10, &[v]);
}

/// [soft 0x0854c274 SI_udfDumpAll]: a header line and every effect of the list (with its own line) on `fp`;
/// nothing for a null file or one with an error.
pub fn si_udf_dump_all(m: &mut Mem, o: &mut dyn Outside, fp: u32) {
    if fp == 0 || o.call(m, FERROR, &[fp]).eax != 0 {
        return;
    }
    let n = m.u32(NUM_UDF);
    fprintf(m, o, fp, 0x086e_4c2a, &[n]);
    let mut i: i32 = 0;
    while i < m.u32(NUM_UDF) as i32 {
        fprintf(m, o, fp, 0x086e_4c48, &[i as u32]);
        let u = m.u32(m.u32(UDF_LIST).wrapping_add((i as u32).wrapping_mul(4)));
        if u != 0 {
            si_udf_dump(m, o, u, fp);
        }
        i += 1;
    }
}

/// [soft 0x0854c92c makeUDFChapterArrayRelations]: attaches the array elements of every entry of the
/// chapter that is not yet marked (short at +0x48 zero).
pub fn make_udf_chapter_array_relations(m: &mut Mem, o: &mut dyn Outside, chap_id: u32) {
    let idx = m.u32(m.u32(PROC_DATA));
    let dict = m.u32(ELEM_DICTS).wrapping_add(idx.wrapping_mul(112));
    let chapter = m.u32(dict.wrapping_add(4)).wrapping_add(chap_id << 6);
    let mut i: i32 = 0;
    let mut off: u32 = 0;
    while i < m.u32(chapter + 4) as i32 {
        let e = off.wrapping_add(m.u32(chapter + 8));
        if e != 0 && m.u16(e + 0x48) == 0 {
            let a = m.u32(e + 4);
            o.call(m, SI_UDF_ATTACH_ARRAY_ELEMENTS, &[a]);
        }
        off = off.wrapping_add(0x84);
        i += 1;
    }
}

/// [soft 0x0854c994 SI_makeUDFArrayRelations]: the array relations of eleven chapters.
pub fn si_make_udf_array_relations(m: &mut Mem, o: &mut dyn Outside) {
    for g in ARRAY_CHAPTERS {
        let c = m.u32(g);
        make_udf_chapter_array_relations(m, o, c);
    }
}

/// `nbPostDefFX` and `renderAbort` (pointers to per-process arrays), `sceneCallbackLists`,
/// `SI_standaloneRenderer` (a short).
const NB_POST_DEF_FX: u32 = 0x086d_f350;
const RENDER_ABORT: u32 = 0x086d_f330;
const SCENE_CALLBACK_LISTS: u32 = 0x086d_f32c;
const STANDALONE_RENDERER: u32 = 0x0867_9400;

fn proc_index(m: &Mem) -> u32 {
    m.u32(m.u32(PROC_DATA))
}

/// [soft 0x08532c0c cusSelect]: a file name (past its first eleven characters) whose last '.' starts ".cus".
pub fn cus_select(m: &Mem, name: u32) -> i32 {
    let p = clib::strrchr(m, name.wrapping_add(0xb), b'.');
    (p != 0 && clib::strcmp(m, p, 0x086d_f55c) == 0) as i32
}

/// [soft 0x085358c4 si_cusGetNbPostDefFX]: the process's count of post-defined effects.
pub fn si_cus_get_nb_post_def_fx(m: &Mem) -> u32 {
    m.u32(m.u32(NB_POST_DEF_FX).wrapping_add(proc_index(m).wrapping_mul(4)))
}

/// [soft 0x085358dc si_cusIncNbPostDefFX]: adds one, answers the new count.
pub fn si_cus_inc_nb_post_def_fx(m: &mut Mem) -> u32 {
    let at = m.u32(NB_POST_DEF_FX).wrapping_add(proc_index(m).wrapping_mul(4));
    let v = m.u32(at).wrapping_add(1);
    m.set_u32(at, v);
    v
}

/// [soft 0x085358fc si_cusDecNbPostDefFX]: takes one, answers the new count.
pub fn si_cus_dec_nb_post_def_fx(m: &mut Mem) -> u32 {
    let at = m.u32(NB_POST_DEF_FX).wrapping_add(proc_index(m).wrapping_mul(4));
    let v = m.u32(at).wrapping_sub(1);
    m.set_u32(at, v);
    v
}

/// [soft 0x08535d7c SI_cusSetRenderAbortFlag]: the process's abort flag (a short); answers it sign-extended.
pub fn si_cus_set_render_abort_flag(m: &mut Mem, v: u32) -> i32 {
    let at = m.u32(RENDER_ABORT).wrapping_add(proc_index(m).wrapping_mul(2));
    m.set_u16(at, v as u16);
    v as u16 as i16 as i32
}

/// [soft 0x08535d9c SI_cusGetRenderAbortFlag].
pub fn si_cus_get_render_abort_flag(m: &Mem) -> i32 {
    m.u16(m.u32(RENDER_ABORT).wrapping_add(proc_index(m).wrapping_mul(2))) as i16 as i32
}

/// [soft 0x085354f8 cusEffectStatusSetLatest]: in the effect list of the scene's status record (+0x148,
/// chained by +0x120) every effect (chained by +0xfc from +0x100) whose latest (+0x110) is `old`
/// takes `new`; one whose latest is the effect itself first counts the record's short at +2 down.
pub fn cus_effect_status_set_latest(m: &mut Mem, status: u32, old: u32, new: u32) {
    let mut rec = m.u32(status + 0x148);
    while rec != 0 {
        let mut e = m.u32(rec + 0x100);
        while e != 0 {
            let latest = m.u32(e + 0x110);
            if latest == old {
                if latest == e {
                    let c = m.u16(rec + 2).wrapping_sub(1);
                    m.set_u16(rec + 2, c);
                }
                m.set_u32(e + 0x110, new);
            }
            e = m.u32(e + 0xfc);
        }
        rec = m.u32(rec + 0x120);
    }
}

/// The checks the scene-callback calls start with: 0x10 when `callback` is null or the scene or
/// the list do not exist, or no routine is given outside the stand-alone renderer.
fn callback_args_bad(m: &mut Mem, o: &mut dyn Outside, scene: u32, list: u32, callback: u32, routine: u32) -> bool {
    callback == 0
        || o.call(m, SI_ELEMDICT_EXISTS, &[scene]).eax as u16 == 0
        || o.call(m, SI_GRAF_LIST_EXISTS, &[list]).eax as u16 == 0
        || (m.u16(STANDALONE_RENDERER) == 0 && routine == 0)
}

/// [soft 0x0853591c SI_cusSceneCallbackAdd]: appends (routine, callback, event, flag) to the process's
/// list of scene callbacks. 0, 0x10 for bad arguments, 0x46 when out of memory.
pub fn si_cus_scene_callback_add(m: &mut Mem, o: &mut dyn Outside, scene: u32, list: u32, routine: u32, callback: u32, event: u32, flag: u32) -> i32 {
    if callback_args_bad(m, o, scene, list, callback, routine) {
        return 0x10;
    }
    let lists = m.u32(SCENE_CALLBACK_LISTS);
    let at = lists.wrapping_add(proc_index(m).wrapping_mul(4));
    let mut last = m.u32(at);
    while last != 0 {
        let next = m.u32(last + 0x10);
        if next == 0 {
            break;
        }
        last = next;
    }
    let n = m.si_malloc(0x14);
    if n == 0 {
        return 0x46;
    }
    m.set_u32(n, routine);
    m.set_u32(n + 4, callback);
    m.set_u32(n + 8, event);
    m.set_u32(n + 0xc, flag);
    m.set_u32(n + 0x10, 0);
    if last == 0 {
        let at = m.u32(SCENE_CALLBACK_LISTS).wrapping_add(proc_index(m).wrapping_mul(4));
        m.set_u32(at, n);
    } else {
        m.set_u32(last + 0x10, n);
    }
    0
}

/// [soft 0x085359f0 SI_cusSceneCallbackRemove]: removes the first entry equal to (routine, callback,
/// event, flag) from the list. 0, or 0x10 for bad arguments or when there is none.
pub fn si_cus_scene_callback_remove(m: &mut Mem, o: &mut dyn Outside, scene: u32, list: u32, routine: u32, callback: u32, event: u32, flag: u32) -> i32 {
    if callback_args_bad(m, o, scene, list, callback, routine) {
        return 0x10;
    }
    let at = m.u32(SCENE_CALLBACK_LISTS).wrapping_add(proc_index(m).wrapping_mul(4));
    let mut prev = 0;
    let mut n = m.u32(at);
    loop {
        if n == 0 {
            return 0x10;
        }
        if m.u32(n + 4) == callback && m.u32(n + 8) == event && m.u32(n) == routine && m.u32(n + 0xc) == flag {
            break;
        }
        prev = n;
        n = m.u32(n + 0x10);
    }
    let next = m.u32(n + 0x10);
    if prev == 0 {
        m.set_u32(at, next);
    } else {
        m.set_u32(prev + 0x10, next);
    }
    m.free(n);
    0
}
