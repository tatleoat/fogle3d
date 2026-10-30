// SPDX-License-Identifier: MIT
//! The shader rows of the material and light dialogs, `shd_fct.c` (finding 0023, section 39): a row
//! of controls (`SI_UdfUiControls`, 0x20 bytes: +0 the enable check box's item (short), +4 the name
//! field's item, +8 the select button's item, +0xc the edit button's item, +0x10 the delete
//! button's item (-1 for none), +0x14 a pointer to the cell that holds the row's shader, +0x18 a
//! pointer to the cell that holds the chapter id the shader is chosen from, +0x1c the element whose
//! shader this is) is kept in step with the shader it stands for. Everything the dialog toolkit,
//! the user-defined-function module and the dictionary do is an outside call (hooks answered by
//! the caller); the strings are the initial `.data` of `shd_list::store_strings`.

use crate::shd_copy::ShdCalls;
use si_mem::Mem;

si_mem::outside_calls! {
    /// What the dialog glue calls.
    pub trait ShdUiCalls {
        /// `SI_statusDisplayMessage (0x083cca94)`: `(text, level)`.
        fn status_message(text: u32, level: u32) = "SI_statusDisplayMessage", 0x083c_ca94;
        /// `SI_getTexed (0x0838c2c0)`: `(dialog, item, &text)`, a fresh copy of the field's text.
        fn get_texed(dialog: u32, item: u32, out: u32) = "SI_getTexed", 0x0838_c2c0;
        /// `SI_setTexed (0x0838bd10)`: `(dialog, item, text)`.
        fn set_texed(dialog: u32, item: u32, text: u32) = "SI_setTexed", 0x0838_bd10;
        /// `SI_setStateItem (0x0838b824)`: `(dialog, item, state)`.
        fn set_state_item(dialog: u32, item: u32, state: u32) = "SI_setStateItem", 0x0838_b824;
        /// `SI_setStateGhost (0x0838b868)`: `(dialog, item, ghosted)`.
        fn set_state_ghost(dialog: u32, item: u32, ghosted: u32) = "SI_setStateGhost", 0x0838_b868;
        /// `SI_udfGetName (0x0854c7cc)`.
        fn udf_get_name(udf: u32) -> u32 = "SI_udfGetName", 0x0854_c7cc;
        /// `SI_udfSetName (0x0854c7d8)`.
        fn udf_set_name(udf: u32, name: u32) = "SI_udfSetName", 0x0854_c7d8;
        /// `SI_udfAllocate (0x0854c2f4)`.
        fn udf_allocate() -> u32 = "SI_udfAllocate", 0x0854_c2f4;
        /// `SI_udfInitialize (0x08544370)`: `(udf, name, extension, chapter)`; a short.
        fn udf_initialize(udf: u32, name: u32, ext: u32, cid: u32) -> i16 = "SI_udfInitialize", 0x0854_4370;
        /// `SI_udfSetActive (0x0854c790)`.
        fn udf_set_active(udf: u32, on: u32) = "SI_udfSetActive", 0x0854_c790;
        /// `SI_udfIsActive (0x0854c748)`: a short.
        fn udf_is_active(udf: u32) -> i16 = "SI_udfIsActive", 0x0854_c748;
        /// `SI_udfEditParms (0x085454d4)`.
        fn udf_edit_parms(udf: u32) = "SI_udfEditParms", 0x0854_54d4;
        /// `SI_dbGetElement (0x080d8e88)`: `(chapter, &name, extension, flags)`; a short.
        fn db_get_element(cid: u32, out: u32, ext: u32, flags: u32) -> i16 = "SI_dbGetElement", 0x080d_8e88;
        /// `SI_elemdictGetNewName (0x0813697c)`: `(dictionary, chapter, name)`.
        fn elemdict_get_new_name(dict: u32, cid: u32, name: u32) -> u32 = "SI_elemdictGetNewName", 0x0813_697c;
        /// `SI_edElementChangeName (0x080febf4)`: `(element, chapter, old name, &new name)`; a short.
        fn ed_element_change_name(elem: u32, cid: u32, old: u32, new: u32) -> i16 = "SI_edElementChangeName", 0x080f_ebf4;
    }
}

const MSG_RENAMED: u32 = 0x086a_2e69;
const MSG_REMOVED: u32 = 0x086a_2e79;
const STR_SHD: u32 = 0x086a_2e89;
/// One-byte empty strings: the compared "empty" text, the name shown for an empty row, the other
/// empties.
const EMPTY_EDIT: u32 = 0x086a_2e68;
const EMPTY_ROW: u32 = 0x086a_2e8d;
const EMPTY_A: u32 = 0x086a_2e8e;
const EMPTY_B: u32 = 0x086a_2e8f;
const MSG_RENAMED2: u32 = 0x086a_2e90;
const MSG_NOT_RENAMED: u32 = 0x086a_2ea0;
const EMPTY_SELECT: u32 = 0x086a_2eb4;
/// `SI_dBWorkFnameExt`, `SI_procData`, `SI_elemDicts`.
const WORK_FNAME_EXT: u32 = 0x0878_8540;
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_ELEM_DICTS: u32 = 0x0867_931c;

fn short(m: &Mem, a: u32) -> i32 {
    i32::from(m.u16(a) as i16)
}

/// [soft 0x082a5d04 udfUiDelete]: disposes the shader in the cell `cell`, empties the cell and
/// reports it. Always 1.
pub fn udf_ui_delete(m: &mut Mem, o: &mut dyn si_mem::Outside, cell: u32) -> i32 {
    let udf = m.u32(cell);
    if udf != 0 {
        o.udf_dispose(m, udf);
        m.set_u32(cell, 0);
        o.status_message(m, MSG_REMOVED, 0);
    }
    1
}

/// [soft 0x082a5c44 udfUiEditText]: reads the name field `item`; an absent or empty text deletes
/// the shader (`udfUiDelete`, answer 1); otherwise a different text than the shader's name renames
/// it ("Shader renamed.") and the answer is 0. The text copy is freed.
pub fn udf_ui_edit_text(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, cell: u32, item: i32) -> i32 {
    let sp = m.sp();
    let text_cell = m.local(4);
    o.get_texed(m, dialog, item as i16 as i32 as u32, text_cell);
    let text = m.u32(text_cell);
    let mut ret = 1;
    if text != 0 && m.u8(text) != m.u8(EMPTY_EDIT) {
        let udf = m.u32(cell);
        if udf != 0 {
            let name = o.udf_get_name(m, udf);
            if clib_strcmp(m, text, name) != 0 {
                let udf = m.u32(cell);
                o.udf_set_name(m, udf, text);
                o.status_message(m, MSG_RENAMED, 0);
            }
        }
        ret = 0;
    } else {
        udf_ui_delete(m, o, cell);
    }
    let text = m.u32(text_cell);
    if text != 0 {
        m.free(text);
        m.set_u32(text_cell, 0);
    }
    m.set_sp(sp);
    i32::from(ret as i16)
}

fn clib_strcmp(m: &Mem, a: u32, b: u32) -> i32 {
    let (x, y) = (m.cstr(a), m.cstr(b));
    match x.cmp(&y) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }
}

/// [soft 0x082a5d40 udfUiSelect]: lets the user pick an element of chapter `cid` (`SI_dbGetElement`,
/// flags 2, file extension "shd"), makes a shader in the cell if there is none, names it from the
/// dictionary's new-name routine and initialises it. 1 when that worked (the shader is made
/// active), 0 when nothing was chosen or the initialisation failed (the shader is disposed).
pub fn udf_ui_select(m: &mut Mem, o: &mut dyn si_mem::Outside, cell: u32, cid: u32) -> i32 {
    let sp = m.sp();
    let chosen = m.local(4);
    let r = o.db_get_element(m, cid, chosen, STR_SHD, 2);
    if r == 0 {
        m.set_sp(sp);
        return 0;
    }
    let ext = m.strdup(WORK_FNAME_EXT);
    if m.u32(cell) == 0 {
        let u = o.udf_allocate(m);
        m.set_u32(cell, u);
    }
    let pd = m.u32(SI_PROC_DATA);
    let dict = m.u32(pd).wrapping_shl(3).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS));
    let picked = m.u32(chosen);
    let nn = o.elemdict_get_new_name(m, dict, cid, picked);
    let name = m.strdup(nn);
    let udf = m.u32(cell);
    let ok = o.udf_initialize(m, udf, name, ext, cid);
    let ret = if ok != 0 {
        let udf = m.u32(cell);
        o.udf_set_active(m, udf, 1);
        1
    } else {
        let udf = m.u32(cell);
        o.udf_dispose(m, udf);
        0
    };
    if name != 0 {
        m.free(name);
    }
    let c = m.u32(chosen);
    if c != 0 {
        m.free(c);
        m.set_u32(chosen, 0);
    }
    if ext != 0 {
        m.free(ext);
    }
    m.set_sp(sp);
    ret
}

/// [soft 0x082a5e7c syncUdfControls]: brings the dialog in line with the rows' shaders: every row's
/// check box shows its shader's active state (off for none) and, when `set_names`, its name field
/// the shader's name (empty for none). The row of the current panel (the dialog's short at +0x10
/// plus 1 is the row's name item; with one row always that row) decides the buttons: when found,
/// the select button is enabled and the edit and delete buttons only if it holds a shader;
/// without one all three are ghosted.
pub fn sync_udf_controls(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, rows: u32, count: i32, set_names: i32) {
    let set_names = set_names as u16;
    let mut found = false;
    let mut has = false;
    if count == 1 {
        found = true;
        let cell = m.u32(rows.wrapping_add(0x14));
        has = m.u32(cell) != 0;
    }
    let mut i = 0i32;
    let mut row = rows;
    while i < count && !found {
        if short(m, dialog.wrapping_add(0x10)).wrapping_add(1) == m.u32(row.wrapping_add(4)) as i32 {
            found = true;
            let cell = m.u32(row.wrapping_add(0x14));
            has = m.u32(cell) != 0;
        }
        i += 1;
        row = row.wrapping_add(0x20);
    }
    let mut row = rows;
    let mut left = count;
    while left > 0 {
        let cell = m.u32(row.wrapping_add(0x14));
        let udf = m.u32(cell);
        let state = if udf != 0 { i32::from(o.udf_is_active(m, udf)) } else { 0 };
        o.set_state_item(m, dialog, short(m, row) as u32, state as u32);
        if set_names != 0 {
            let name = if udf != 0 {
                let cell = m.u32(row.wrapping_add(0x14));
                let u = m.u32(cell);
                o.udf_get_name(m, u)
            } else {
                EMPTY_ROW
            };
            o.set_texed(m, dialog, short(m, row.wrapping_add(4)) as u32, name);
        }
        left -= 1;
        row = row.wrapping_add(0x20);
    }
    let (g1, g2) = if found { (0, u32::from(!has)) } else { (1, 1) };
    let b = m.u32(rows.wrapping_add(8));
    o.set_state_ghost(m, dialog, b, g1);
    let b = m.u32(rows.wrapping_add(0xc));
    o.set_state_ghost(m, dialog, b, g2);
    let b = m.u32(rows.wrapping_add(0x10));
    if b != u32::MAX {
        o.set_state_ghost(m, dialog, b, g2);
    }
}

/// [soft 0x082a6414 SI_udfUiChangeName]: for each row with both a shader and an element: reads the
/// name field; an absent or empty field (or an element without a name) ends the loop with 0; a
/// text different from the element's name asks the dictionary to rename the element
/// (`SI_edElementChangeName`): on success the shader takes the new name ("Shader renamed.", level
/// 0), on failure the element's old name ("Shader NOT renamed.", level 2) and the answer is 0.
/// The loop stops at the first 0. Answers 1 for no rows.
pub fn udf_ui_change_name(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, rows: u32, count: i32) -> i32 {
    let sp = m.sp();
    let text_cell = m.local(4);
    m.set_u32(text_cell, 0);
    let mut ok: i16 = 1;
    let mut i = 0i32;
    let mut row = rows;
    if count > 0 {
        loop {
            let cell = m.u32(row.wrapping_add(0x14));
            let udf = m.u32(cell);
            let elem = m.u32(row.wrapping_add(0x1c));
            if elem != 0 && udf != 0 {
                o.get_texed(m, dialog, short(m, row.wrapping_add(4)) as u32, text_cell);
                let text = m.u32(text_cell);
                if text != 0 {
                    let rec = m.u32(elem);
                    let old = m.u32(rec.wrapping_add(0x14));
                    if m.u8(text) != m.u8(EMPTY_A) && old != 0 && m.u8(old) != m.u8(EMPTY_B) {
                        if clib_strcmp(m, text, old) != 0 {
                            let cid = m.u32(udf.wrapping_add(4));
                            let name = m.u32(rec.wrapping_add(0x10));
                            ok = o.ed_element_change_name(m, elem, cid, name, text_cell);
                            if ok != 0 {
                                let t = m.u32(text_cell);
                                o.udf_set_name(m, udf, t);
                                o.status_message(m, MSG_RENAMED2, 0);
                            } else {
                                o.udf_set_name(m, udf, old);
                                o.status_message(m, MSG_NOT_RENAMED, 2);
                            }
                        }
                        let t = m.u32(text_cell);
                        if t != 0 {
                            m.free(t);
                            m.set_u32(text_cell, 0);
                        }
                    } else {
                        m.free(text);
                        m.set_u32(text_cell, 0);
                        ok = 0;
                        break;
                    }
                } else {
                    ok = 0;
                    break;
                }
            }
            row = row.wrapping_add(0x20);
            i += 1;
            if i >= count || ok == 0 {
                break;
            }
        }
    }
    m.set_sp(sp);
    i32::from(ok)
}

/// [soft 0x082a658c SI_udfUiHandleAction]: an item of the dialog was used. -1 means refresh: the
/// rows are synchronised with the dialog's panel marker set to -1 for the call, answer 1.
/// Otherwise the rows are scanned for the item: a row's check box toggles its shader's active
/// state (or, when it has none, clears the name field and runs `udfUiSelect`); then, for the
/// first row whose name field is the item the text is applied (`udfUiEditText`), and (for the
/// current panel's row, or the only row) its select, edit and delete buttons act
/// (`udfUiSelect`, `SI_udfEditParms`, `udfUiDelete`), anything else there re-reads the name
/// field. The rows are then synchronised (names rewritten when a shader was chosen or deleted).
/// The answer is 1 when an item was recognised, else 0.
pub fn udf_ui_handle_action(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, rows: u32, count: i32, action: i32) -> i32 {
    let mut done: i16 = 0;
    let mut names: i16 = 0;
    if action == -1 {
        let old = m.u16(dialog.wrapping_add(0x10));
        m.set_u16(dialog.wrapping_add(0x10), action as u16);
        sync_udf_controls(m, o, dialog, rows, count, 1);
        m.set_u16(dialog.wrapping_add(0x10), old);
        return 1;
    }
    let mut i = 0i32;
    let mut row = rows;
    while i < count {
        if action == m.u32(row) as i32 {
            let cell = m.u32(row.wrapping_add(0x14));
            let udf = m.u32(cell);
            if udf != 0 {
                let a = o.udf_is_active(m, udf);
                let on = u32::from(a == 0);
                let cell = m.u32(row.wrapping_add(0x14));
                let udf = m.u32(cell);
                o.udf_set_active(m, udf, on);
                o.set_state_item(m, dialog, short(m, row) as u32, on);
            } else {
                o.set_texed(m, dialog, short(m, row.wrapping_add(4)) as u32, EMPTY_SELECT);
                let cid_cell = m.u32(row.wrapping_add(0x18));
                let cid = m.u32(cid_cell);
                udf_ui_select(m, o, m.u32(row.wrapping_add(0x14)), cid);
                names = 1;
            }
            done = 1;
        }
        i += 1;
        row = row.wrapping_add(0x20);
        if i >= count || done != 0 {
            break;
        }
    }
    let mut i = 0i32;
    let mut row = rows;
    while i < count && done == 0 {
        let name_item = m.u32(row.wrapping_add(4)) as i32;
        if action == name_item {
            let cell = m.u32(row.wrapping_add(0x14));
            udf_ui_edit_text(m, o, dialog, cell, action);
            done = 1;
        } else if count == 1 || short(m, dialog.wrapping_add(0x10)) + 1 == name_item {
            let cell = m.u32(row.wrapping_add(0x14));
            if action == m.u32(row.wrapping_add(8)) as i32 {
                let cid_cell = m.u32(row.wrapping_add(0x18));
                let cid = m.u32(cid_cell);
                udf_ui_select(m, o, cell, cid);
                names = 1;
                done = 1;
            } else if action == m.u32(row.wrapping_add(0xc)) as i32 {
                let udf = m.u32(cell);
                if udf != 0 {
                    o.udf_edit_parms(m, udf);
                }
                done = 1;
            } else if action == m.u32(row.wrapping_add(0x10)) as i32 {
                udf_ui_delete(m, o, cell);
                names = 1;
                done = 1;
            }
            if done == 0 {
                udf_ui_edit_text(m, o, dialog, cell, name_item);
                i = count;
            }
        }
        i += 1;
        row = row.wrapping_add(0x20);
    }
    sync_udf_controls(m, o, dialog, rows, count, i32::from(names));
    i32::from(done)
}
