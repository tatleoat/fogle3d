// SPDX-License-Identifier: MIT
//! `custom_init.c`: setup of the custom effect lists (new commands, name links, the per-scene status
//! pointers); finding 0041, "seventh continuation". The loader proper (SI_cusLoadEffect and friends) is
//! not ported.

use crate::custom_dyn::{si_dynamic_link_create, si_dynamic_link_re_allocate, si_dynamic_link_set_name, SI_ERROR};
use crate::custom_load::{FOPEN, SI_CUS_DIALOG_READ};
use crate::custom_udf::PLT_FOPEN;
use crate::custom_util::FCLOSE;
use crate::custom_misc::{cus_effect_status_set_latest, GETENV};
use crate::custom_util::*;
use crate::shdb_init::SI_STATUS_DISPLAY_MESSAGE;
use crate::{cfmt, STDERR};
use si_mem::{clib, Mem, Outside, Routine};

pub const PLT_EXIT: Routine = Routine::new("plt:exit", 0x080c_4b58);
pub const SCANDIR: Routine = Routine::new("plt:scandir", 0x080c_4618);
pub const FSCANF: Routine = Routine::new("plt:fscanf", 0x080c_4058);
pub const SI_UDF_LIST_ADD: Routine = Routine::new("SI_udfListAdd", 0x0854_794c);
pub const SI_CUS_PARSE_SECONDARY_DYNAMIC_LINK: Routine = Routine::new("si_cusParseSecondaryDynamicLink", 0x0852_aba0);
pub const SI_RELDICT_READ_OPERATOR_ATTRIBUTES: Routine = Routine::new("SI_reldictReadOperatorAttributes", 0x0813_d39c);
pub const SI_RELDICT: u32 = 0x0878_15f0;
const SI_ELEM_DICTS: u32 = 0x0867_931c;
const SI_CID_MODELS: u32 = 0x0878_1680;

pub const PLT_FPRINTF: Routine = Routine::new("plt:fprintf", 0x080c_3b88);
const S_BAD_NAME_REF: u32 = 0x086e_0020;
pub const SI_RELDICT_GET_REL_INDEX_FROM_NAME: Routine =
    Routine::new("SI_reldictGetRelIndexFromName", 0x0813_eae8);

/// The per-scene persistent list pointers (`persistentLists` and the five after it).
pub const PERSISTENT_LISTS: u32 = 0x086d_f334;

/// [soft 0x08535394 si_cusNewCmd]: appends a zeroed 0x34-byte command record to the effect list's
/// command chain (head pointer at +0xf8, `next` at +0x30; the chain starts in the list's command-list
/// header at +0xc8) and returns it. Its kind is a 16-bit zero and its pipe slot (+0x18) is -1.
pub fn si_cus_new_cmd(m: &mut Mem, list: u32) -> u32 {
    let mut tail = list.wrapping_add(200);
    while m.u32(tail.wrapping_add(0x30)) != 0 {
        tail = m.u32(tail.wrapping_add(0x30));
    }
    let p = m.malloc(0x34);
    m.set_u32(tail.wrapping_add(0x30), p);
    m.set_u32(p.wrapping_add(0x30), 0);
    m.set_u16(p, 0);
    for off in [4u32, 8, 0xc, 0x20, 0x24, 0x14, 0x10, 0x1c, 0x28, 0x2c] {
        m.set_u32(p.wrapping_add(off), 0);
    }
    m.set_u32(p.wrapping_add(0x18), 0xffff_ffff);
    p
}

/// [soft 0x08535274 si_cusResolveNameLinks]: for every command that names another command (kinds in
/// the first set) finds the earlier definition command (kinds in the second set) with that name, links
/// it at +0x28, and warns on stderr when none matches; for every command that names a type
/// (kinds in the third set) links the type record (kind 0x1b) with that name at +0x2c.
pub fn si_cus_resolve_name_links(m: &mut Mem, o: &mut dyn Outside, list: u32) {
    let head = m.u32(list.wrapping_add(0xf8));
    let mut c = head;
    while c != 0 {
        let kind = m.u16(c);
        if matches!(
            kind,
            0xc..=0xf | 0x15..=0x1a | 0x24 | 0x2d..=0x30 | 0x37 | 0x39..=0x46 | 0x4b | 0x4c
        ) {
            let name = m.u32(c.wrapping_add(4));
            let mut found = false;
            let mut q = head;
            if q == c {
                if !found {
                    warn(m, o);
                }
            } else {
                loop {
                    if matches!(m.u16(q), 4..=10 | 0x12 | 0x1e | 0x20 | 0x21 | 0x47 | 0x48)
                        && clib::strcmp(m, name, m.u32(q.wrapping_add(4))) == 0
                    {
                        m.set_u32(c.wrapping_add(0x28), q);
                        found = true;
                    }
                    q = m.u32(q.wrapping_add(0x30));
                    if q == c {
                        if !found {
                            warn(m, o);
                        }
                        break;
                    }
                    if found {
                        break;
                    }
                }
            }
        }
        let kind = m.u16(c);
        if matches!(
            kind,
            2 | 3 | 0xc..=0x10 | 0x12 | 0x14..=0x1a | 0x1d | 0x1e | 0x23..=0x30 | 0x36 | 0x37 | 0x39..=0x46 | 0x49 | 0x4b | 0x4c
        ) {
            let name = m.u32(c.wrapping_add(8));
            let mut found = false;
            let mut q = head;
            if q != c {
                loop {
                    if m.u16(q) == 0x1b && clib::strcmp(m, name, m.u32(q.wrapping_add(4))) == 0 {
                        m.set_u32(c.wrapping_add(0x2c), q);
                        found = true;
                    }
                    q = m.u32(q.wrapping_add(0x30));
                    if q == c || found {
                        break;
                    }
                }
            }
        }
        c = m.u32(c.wrapping_add(0x30));
    }
}

fn warn(m: &mut Mem, o: &mut dyn Outside) {
    let err = m.u32(STDERR);
    o.call(m, PLT_FPRINTF, &[err, S_BAD_NAME_REF]);
}

/// [soft 0x08532390 SI_cusStatMotionPointersUpdate]: points each custom status record's per-scene list
/// slot (+0x14c) at the current scene's entry (0x11c bytes per scene) in its persistent list.
pub fn si_cus_stat_motion_pointers_update(m: &mut Mem) {
    let pd = m.u32(SI_PROC_DATA);
    let scene = m.u32(pd);
    let off = scene.wrapping_mul(0x11c);
    for (rec, slot) in [
        (crate::custom_util::CUS_MODEL, 0u32),
        (crate::custom_util::CUS_MOTION, 1),
        (0x0882_29a0, 2),
        (0x0882_2dc0, 3),
        (0x0882_2840, 4),
        (0x0882_9840, 5),
    ] {
        let base = m.u32(PERSISTENT_LISTS.wrapping_add(slot * 4));
        m.set_u32(rec.wrapping_add(332), base.wrapping_add(off));
    }
}

/// [soft 0x08535728 cusModifyRelDict]: blanks the leading character of the relation names equal to
/// `new` that follow the first entry the dictionary finds for (`old`, `new`); 0 when an argument is
/// missing, else 1.
pub fn cus_modify_rel_dict(m: &mut Mem, o: &mut dyn Outside, dict: u32, old: u32, new: u32) -> u32 {
    if dict == 0 || old == 0 || new == 0 {
        return 0;
    }
    let idx = o.call(m, SI_RELDICT_GET_REL_INDEX_FROM_NAME, &[dict, old, new]).eax;
    let mut off = idx.wrapping_mul(0x50);
    loop {
        let rels = m.u32(dict.wrapping_add(8));
        if m.u32(rels.wrapping_add(off)) != 20000 {
            break;
        }
        let name = m.u32(m.u32(dict.wrapping_add(8)).wrapping_add(4).wrapping_add(off));
        if clib::strcmp(m, name, new) != 0 {
            break;
        }
        m.set_u8(name, 0x20);
        off = off.wrapping_add(0x50);
    }
    1
}

/// The five-and-one status records the per-scene lists belong to, as `SI_cusStatMotionPointersUpdate` walks them.
const NB_SCENES: u32 = 0x086d_f34c;
const NB_POST_DEF_FX: u32 = 0x086d_f350;
const SCENE_CALLBACK_LISTS: u32 = 0x086d_f32c;
const RENDER_ABORT: u32 = 0x086d_f330;
pub const CUS_CHNS: u32 = 0x0882_9840;

/// [soft 0x08532430 SI_cusStatPersistentListsAdd]: makes room for one more scene in the six persistent
/// lists (0x11c bytes a scene, the word at +0xfc of the new entry cleared), in the post-definition counts
/// (4 bytes), the scene callback lists (4 bytes) and the render abort flags (2 bytes); each step that
/// fails reports "Insufficient memory" through SI_error and ends the chain. -1 on failure; on
/// success the scene count grows by one and the status pointers are updated, and the answer is the old scene count.
pub fn si_cus_stat_persistent_lists_add(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    let mut ret: i32 = 0;
    for i in 0..6u32 {
        let slot = PERSISTENT_LISTS.wrapping_add(i * 4);
        let n = m.u32(NB_SCENES).wrapping_add(1).wrapping_mul(0x11c);
        let p = m.si_realloc(m.u32(slot), n);
        m.set_u32(slot, p);
        if p == 0 {
            ret = -1;
            o.call(m, SI_ERROR, &[0x46, 0x086d_f360]);
            break;
        }
        m.set_u32(p.wrapping_add(m.u32(NB_SCENES).wrapping_mul(0x11c)).wrapping_add(0xfc), 0);
    }
    // (variable, bytes per scene, the error text)
    for (var, size, msg) in [(NB_POST_DEF_FX, 4u32, 0x086d_f3a0u32), (SCENE_CALLBACK_LISTS, 4, 0x086d_f3e0), (RENDER_ABORT, 2, 0x086d_f420)] {
        if ret == -1 {
            break;
        }
        let n = m.u32(NB_SCENES).wrapping_add(1).wrapping_mul(size);
        let p = m.si_realloc(m.u32(var), n);
        m.set_u32(var, p);
        if p == 0 {
            ret = -1;
            o.call(m, SI_ERROR, &[0x46, msg]);
        } else if size == 4 {
            m.set_u32(p.wrapping_add(m.u32(NB_SCENES).wrapping_mul(4)), 0);
        } else {
            m.set_u16(p.wrapping_add(m.u32(NB_SCENES).wrapping_mul(2)), 0);
        }
    }
    if ret != -1 {
        // the answer is the scene count before the increment (the original's register still holds it)
        ret = m.u32(NB_SCENES) as i32;
        let n = m.u32(NB_SCENES).wrapping_add(1);
        m.set_u32(NB_SCENES, n);
        si_cus_stat_motion_pointers_update(m);
    }
    ret
}

/// [soft 0x0853555c cusValueListRemoveUnsupportedSymbols]: walks the value list `*listp` and drops every
/// value of type -1 whose name is empty, or does not start with "BROWSER" or "SELECT", or that no
/// element of the dialog shows (or whose text-field partner is absent); `*all` becomes 1 when every
/// type -1 value had a partner field. Answers 1 when a value was removed, else 0 (0 also for a null argument).
pub fn cus_value_list_remove_unsupported_symbols(m: &mut Mem, o: &mut dyn Outside, dialog: u32, listp: u32, all: u32) -> i32 {
    if dialog == 0 || listp == 0 || all == 0 {
        return 0;
    }
    let mut removed: i32 = 0;
    let mut prev: u32 = 0;
    let mut with_field: i32 = 0;
    let mut total: i32 = 0;
    let mut v = m.u32(listp);
    while v != 0 {
        if m.u16(v.wrapping_add(4)) == 0xffff {
            total += 1;
            let name = m.u32(v);
            if name != 0
                && (clib::strncmp(m, name, 0x086e_01a0, 7) == 0 || clib::strncmp(m, name, 0x086e_01a8, 6) == 0)
            {
                // the dialog element showing this value
                let mut el: u32 = 0;
                let mut shown = false;
                let mut idx: i32 = 0;
                let mut off: u32 = 0;
                while idx < m.u32(dialog.wrapping_add(8)) as i32 {
                    el = off.wrapping_add(m.u32(dialog.wrapping_add(0xc)));
                    if o.call(m, SI_CUS_VALUE_IN_DIALOG, &[el, v]).eax as u16 != 0 {
                        shown = true;
                        break;
                    }
                    off = off.wrapping_add(0x3c);
                    idx += 1;
                }
                if shown {
                    let n = m.u16(el.wrapping_add(0xc)) as i16 as i32;
                    let mut i = 0;
                    let mut ent_off: u32 = 0;
                    while i < n {
                        let ent = ent_off.wrapping_add(m.u32(el.wrapping_add(0x14)));
                        let text = m.u32(ent.wrapping_add(0x1c));
                        if m.u32(ent) == 2 && text != 0 && clib::strcmp(m, text, m.u32(v)) == 0 {
                            let id = m.u16(ent.wrapping_add(4)) as i16 as i32 as u32;
                            if (o.call(m, SI_FIND_SELECT_ASSOCIATED_TEXTFIELD, &[el, id]).eax as i32) >= 0 {
                                with_field += 1;
                                break;
                            }
                        }
                        ent_off = ent_off.wrapping_add(0x28);
                        i += 1;
                    }
                }
            }
            // every path of the type -1 branch ends by removing the value from the list
            removed = 1;
            if prev != 0 {
                let next = m.u32(v.wrapping_add(0x44));
                m.set_u32(prev.wrapping_add(0x44), next);
                si_cus_value_delete(m, o, v);
                v = m.u32(prev.wrapping_add(0x44));
            } else {
                let next = m.u32(v.wrapping_add(0x44));
                m.set_u32(listp, next);
                si_cus_value_delete(m, o, v);
                v = m.u32(listp);
            }
        } else {
            prev = v;
            v = m.u32(v.wrapping_add(0x44));
        }
    }
    m.set_u16(all, if total == with_field { 1 } else { 0 });
    removed
}

pub const SI_CUS_VALUE_IN_DIALOG: Routine = Routine::new("si_cusValueInDialog", 0x0852_7b34);
pub const SI_FIND_SELECT_ASSOCIATED_TEXTFIELD: Routine = Routine::new("si_findSelectAssociatedTextfield", 0x0852_6a34);

/// [soft 0x085357ac cusModifyAttribDict]: blanks the first character of the name of the attribute group
/// the dictionary finds for the path "old/new"; 1 when it found one, else 0 (also for a missing argument
/// or no memory).
pub fn cus_modify_attrib_dict(m: &mut Mem, o: &mut dyn Outside, dict: u32, old: u32, new: u32) -> u32 {
    if dict == 0 || old == 0 || new == 0 {
        return 0;
    }
    let n = clib::strlen(m, old).wrapping_add(clib::strlen(m, new)).wrapping_add(2);
    let buf = m.si_malloc(n);
    if buf == 0 {
        return 0;
    }
    let mut s = m.cstr(old);
    s.push(b'/');
    s.extend_from_slice(&m.cstr(new));
    m.put_cstr(buf, &s);
    let grp = o.call(m, SI_ATTDICT_GET_ATT_GRP_BY_PATH, &[dict, buf]).eax;
    m.free(buf);
    if grp == 0 {
        return 0;
    }
    let name = m.u32(grp);
    m.set_u8(name, 0x20);
    1
}

/// [soft 0x085350f8 SI_cusReinit]: gets an effect ready to be read again from a changed `.cus` file whose
/// old name was `old`: refuses (status message, error report, -1) while a model in the current scene still
/// uses the effect (a type 0xd model whose effect has the same name), renames its entries in the attribute
/// and relation dictionaries, then empties its value list, command list, dso data and extra text. 1, or -1.
pub fn si_cus_reinit(m: &mut Mem, o: &mut dyn Outside, eff: u32, old: u32) -> i32 {
    if eff == 0 || old == 0 {
        return -1;
    }
    if m.u32(eff.wrapping_add(0xc)) != 0 {
        let pd = m.u32(SI_PROC_DATA);
        let scene = m.u32(pd);
        let dicts = m.u32(SI_ELEM_DICTS);
        let d = m.u32(dicts.wrapping_add(scene.wrapping_mul(7).wrapping_mul(16)).wrapping_add(4));
        let slot = m.u32(SI_CID_MODELS).wrapping_shl(6);
        let count = m.u32(d.wrapping_add(slot).wrapping_add(4)) as i32;
        let mut model = m.u32(d.wrapping_add(slot).wrapping_add(8));
        let mut i = 0;
        while i < count {
            if m.u16(model.wrapping_add(0x48)) == 0 {
                let e = m.u32(model.wrapping_add(4));
                if e != 0 && m.u32(e.wrapping_add(0x2ec)) == 0xd {
                    let used = si_cus_status_get_effect_by_icon(m, m.u32(eff.wrapping_add(0x114)), e);
                    let used_name = m.u32(used.wrapping_add(4));
                    if clib::strcmp(m, m.u32(eff.wrapping_add(4)), used_name) == 0 {
                        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_ffa0, 2]);
                        o.call(m, SI_ERROR, &[3, 0x086d_ffe0]);
                        return -1;
                    }
                }
            }
            i += 1;
            model = model.wrapping_add(0x84);
        }
        if cus_modify_attrib_dict(m, o, SI_ATD, old, m.u32(eff.wrapping_add(4))) as u16 == 0 {
            return -1;
        }
    }
    if cus_modify_rel_dict(m, o, SI_RELDICT, old, m.u32(eff.wrapping_add(4))) as u16 == 0 {
        return -1;
    }
    si_cus_value_list_delete(m, o, m.u32(eff.wrapping_add(0x64)));
    si_cus_free_command_list(m, o, eff.wrapping_add(0xc8));
    si_cus_dso_data_delete(m, m.u32(eff.wrapping_add(0x104)));
    let text = m.u32(eff.wrapping_add(0x108));
    m.set_u32(eff.wrapping_add(0x104), 0);
    m.set_u16(eff.wrapping_add(8), 0);
    if text != 0 {
        m.free(text);
        m.set_u32(eff.wrapping_add(0x108), 0);
    }
    1
}

/// Takes the next `:`-separated word of the path list (libc `strtok`): `s` on the first call, 0 afterwards.
fn strtok_colon(m: &mut Mem, s: u32, saved: &mut u32) -> u32 {
    let mut p = if s != 0 { s } else { *saved };
    if p == 0 {
        return 0;
    }
    while m.u8(p) == b':' {
        p = p.wrapping_add(1);
    }
    if m.u8(p) == 0 {
        *saved = 0;
        return 0;
    }
    let start = p;
    while m.u8(p) != 0 && m.u8(p) != b':' {
        p = p.wrapping_add(1);
    }
    if m.u8(p) != 0 {
        m.set_u8(p, 0);
        *saved = p.wrapping_add(1);
    } else {
        *saved = 0;
    }
    start
}

const MODE_R: u32 = 0x086d_f528;
const FMT_S: u32 = 0x086d_f561;
const FMT_D: u32 = 0x086d_f569;

/// [soft 0x08532c50 cusAddToMenu]: reads a `.cus` file up to its `Menu` keyword and the menu number after
/// it, finds (or appends) the status' group for that menu id (-100 never matches: a group of its own
/// each time), and appends a new effect record named `name` from directory `dir`. When an effect of the
/// same name stem already is the latest version, the new one becomes the latest only when its version is
/// greater; the group counts the effects that are a latest version. A file without a menu number is
/// reported on stderr. Differences kept: the original loops forever when the file ends before a `Menu`
/// word (fscanf keeps answering -1); the port returns then. A status of a kind above 5 reads its table
/// pointer from uninitialised stack in the original; the port takes 0.
pub fn cus_add_to_menu(m: &mut Mem, o: &mut dyn Outside, st: u32, fp: u32, name: u32, dir: u32) {
    let kind = m.u16(st) as i16 as i32;
    let table: u32 = match kind {
        0 => 0x086e_4760,
        1 | 5 => 0x086e_47c0,
        2 => 0x086e_4720,
        3 => 0x086e_48c0,
        4 => 0x086e_46c0,
        _ => 0,
    };
    let word = m.local(0x3c);
    let menu = m.local(4);
    let mut found = false;
    while !found {
        let n = o.call(m, FSCANF, &[fp, FMT_S, word]).eax as i32;
        if n == 1 && m.cstr(word) == b"Menu" {
            found = true;
        } else if n == -1 {
            m.release(4);
            m.release(0x3c);
            return;
        }
    }
    if o.call(m, FSCANF, &[fp, FMT_D, menu]).eax as i32 != 1 {
        m.release(4);
        m.release(0x3c);
        let err = m.u32(STDERR);
        o.call(m, PLT_FPRINTF, &[err, 0x086d_f580, name]);
        return;
    }
    let menu_n = m.u32(menu) as i32;
    m.release(4);
    m.release(0x3c);
    let id = |m: &Mem| if menu_n == -100 { 0xff9c } else { m.u16(table.wrapping_add((menu_n as u32).wrapping_mul(2))) };
    let latest = si_cus_effect_status_get_latest(m, st, name);
    let mut holder = st.wrapping_add(0x28);
    let mut g = m.u32(holder.wrapping_add(0x120));
    let mut group = 0;
    while g != 0 {
        if menu_n != -100 && m.u16(g) == id(m) {
            group = g;
            break;
        }
        holder = g;
        g = m.u32(holder.wrapping_add(0x120));
    }
    if group == 0 {
        group = m.si_calloc(1, 0x124);
        m.set_u32(holder.wrapping_add(0x120), group);
        m.set_u32(group.wrapping_add(0x120), 0);
        let v = id(m);
        m.set_u16(group, v);
        m.set_u16(group.wrapping_add(2), 0);
        m.set_u32(group.wrapping_add(0x100), 0);
    }
    let mut tail = group.wrapping_add(4);
    if m.u32(group.wrapping_add(0x100)) != 0 {
        loop {
            tail = m.u32(tail.wrapping_add(0xfc));
            if m.u32(tail.wrapping_add(0xfc)) == 0 {
                break;
            }
        }
    }
    let e = si_cus_effect_create(m);
    m.set_u32(tail.wrapping_add(0xfc), e);
    if m.u16(st) == 5 {
        m.set_u16(e, 3);
    }
    let nm = clib::strdup(m, name);
    m.set_u32(e.wrapping_add(4), nm);
    let k = m.u16(st);
    m.set_u16(e.wrapping_add(0x10), k);
    let dd = clib::strdup(m, dir);
    m.set_u32(e.wrapping_add(0x10c), dd);
    m.set_u32(e.wrapping_add(0x114), st);
    if latest != 0 {
        let v_old = si_cus_effect_get_version(m, m.u32(latest.wrapping_add(4)), 0);
        let v_new = si_cus_effect_get_version(m, m.u32(e.wrapping_add(4)), 0);
        if v_new <= v_old {
            m.set_u32(e.wrapping_add(0x110), latest);
            return;
        }
    }
    cus_effect_status_set_latest(m, st, latest, e);
    let c = m.u16(group.wrapping_add(2)).wrapping_add(1);
    m.set_u16(group.wrapping_add(2), c);
}

/// [soft 0x08532994 SI_cusScan]: for each directory of the status' `:`-separated path list (+0x24) lists
/// the `.cus` files (scandir with `cusSelect`, sorted by name), opens each and adds it to the status
/// through `cusAddToMenu` under its name without the extension (cut at 0x27 characters when longer), or
/// reports on stderr that a file of an already known name is ignored; the file and its list entry are
/// then released. Nothing happens when the status has no path list.
pub fn si_cus_scan(m: &mut Mem, o: &mut dyn Outside, st: u32) {
    let dirs = m.u32(st.wrapping_add(0x24));
    if dirs == 0 {
        return;
    }
    let dup = clib::strdup(m, dirs);
    let mut saved = 0u32;
    let mut tok = strtok_colon(m, dup, &mut saved);
    let lp = m.local(4);
    let path = m.local(0x1000);
    loop {
        let n = o.call(m, SCANDIR, &[tok, lp, 0x0853_2c0c, 0x080c_3ce8]).eax as i32;
        if n > 0 {
            for i in 0..n as u32 {
                let ent = m.u32(m.u32(lp).wrapping_add(i * 4));
                let name = ent.wrapping_add(0xb);
                let mut p = m.cstr(tok);
                p.push(b'/');
                p.extend_from_slice(&m.cstr(name));
                m.put_cstr(path, &p);
                let f = o.call(m, FOPEN, &[path, MODE_R]).eax;
                if f == 0 {
                    continue;
                }
                let dot = clib::strrchr(m, name, b'.');
                if dot == 0 {
                    o.call(m, FCLOSE, &[f]);
                    continue;
                }
                let mut cut = dot;
                if (dot.wrapping_sub(name) as i32) > 0x27 {
                    cut = ent.wrapping_add(0x32);
                }
                m.set_u8(cut, 0);
                let e = si_cus_status_get_effect_by_name(m, st, name);
                if e == 0 {
                    cus_add_to_menu(m, o, st, f, name, tok);
                } else {
                    let slash = m.local(2);
                    m.put_cstr(slash, b"/");
                    let msg = si_cus_concat_strings(
                        m,
                        &[0x086d_f547, path, 0x086d_f530, m.u32(e.wrapping_add(0x10c)), slash, m.u32(e.wrapping_add(4)), 0x086d_f52a],
                    );
                    m.release(2);
                    let err = m.u32(STDERR);
                    o.call(m, PLT_FPRINTF, &[err, msg]);
                    if msg != 0 {
                        m.free(msg);
                    }
                }
                o.call(m, FCLOSE, &[f]);
                let ent = m.u32(m.u32(lp).wrapping_add(i * 4));
                if ent != 0 {
                    m.free(ent);
                    let l = m.u32(lp);
                    m.set_u32(l.wrapping_add(i * 4), 0);
                }
            }
            let l = m.u32(lp);
            if l != 0 {
                m.free(l);
                m.set_u32(lp, 0);
            }
        }
        tok = strtok_colon(m, 0, &mut saved);
        if tok == 0 {
            break;
        }
    }
    m.release(0x1000);
    m.release(4);
    if dup != 0 {
        m.free(dup);
    }
}

/// Sets one status record up as `SI_cusInit` does: kind, name, no path, no group list, the two table
/// words (+0x150, +0x154) and the path list from the first environment variable that is set, else the
/// second, else the program's default (a pointer variable).
fn init_status(m: &mut Mem, st: u32, kind: u16, name: &[u8]) {
    m.set_u16(st, kind);
    m.set_bytes(st.wrapping_add(2), name);
    m.set_u32(st.wrapping_add(0x24), 0);
    m.set_u32(st.wrapping_add(0x148), 0);
    m.set_u32(st.wrapping_add(0x150), 0);
    m.set_u32(st.wrapping_add(0x154), 0);
}

fn env_path(m: &mut Mem, o: &mut dyn Outside, st: u32, env1: u32, env2: u32, default: u32) {
    let mut v = o.call(m, GETENV, &[env1]).eax;
    m.set_u32(st.wrapping_add(0x24), v);
    if v == 0 && env2 != 0 {
        v = o.call(m, GETENV, &[env2]).eax;
        m.set_u32(st.wrapping_add(0x24), v);
    }
    if m.u32(st.wrapping_add(0x24)) == 0 {
        let d = m.u32(default);
        m.set_u32(st.wrapping_add(0x24), d);
    }
}

/// [soft 0x085325c4 SI_cusInit]: adds the first scene's persistent lists and the user data (udf) list
/// (the program exits with status 1 if either fails), then sets up the six custom status records and
/// scans their directories: model (kind 0, "SI_CUSTOM_MODEL" or "CUSTOM_MODEL"), motion (1; also a table
/// of 24 empty pairs at +0x154, +0x150 = 0x18), matter (2), tools (3), actor (4) and channels (5, only
/// the "SI_CHNLDRIVER" variable). Each directory is scanned right after its record is set.
pub fn si_cus_init(m: &mut Mem, o: &mut dyn Outside) {
    if si_cus_stat_persistent_lists_add(m, o) == -1 {
        o.call(m, PLT_EXIT, &[1]);
    }
    if o.call(m, SI_UDF_LIST_ADD, &[]).eax as i32 == -1 {
        o.call(m, PLT_EXIT, &[1]);
    }
    // model
    init_status(m, CUS_MODEL, 0, b"Model\0");
    env_path(m, o, CUS_MODEL, 0x086d_f459, 0x086d_f469, 0x0867_935c);
    si_cus_scan(m, o, CUS_MODEL);
    // motion
    init_status(m, CUS_MOTION, 1, b"Motion\0");
    m.set_u32(CUS_MOTION.wrapping_add(0x150), 0x18);
    let tab = m.si_malloc(0xc0);
    m.set_u32(CUS_MOTION.wrapping_add(0x154), tab);
    for i in 0..0x18u32 {
        m.set_u32(tab.wrapping_add(i * 8), 0);
        m.set_u32(tab.wrapping_add(i * 8 + 4), 0);
    }
    env_path(m, o, CUS_MOTION, 0x086d_f47d, 0x086d_f48e, 0x0867_9390);
    si_cus_scan(m, o, CUS_MOTION);
    // matter
    init_status(m, CUS_MATTER, 2, b"Matter\0");
    env_path(m, o, CUS_MATTER, 0x086d_f4a3, 0x086d_f4b4, 0x086e_4710);
    si_cus_scan(m, o, CUS_MATTER);
    // tools
    init_status(m, CUS_TOOLS, 3, b"Tools\0");
    env_path(m, o, CUS_TOOLS, 0x086d_f4c8, 0x086d_f4d8, 0x086e_48ac);
    si_cus_scan(m, o, CUS_TOOLS);
    // actor
    init_status(m, CUS_ACTOR, 4, b"Actor\0");
    env_path(m, o, CUS_ACTOR, 0x086d_f4eb, 0x086d_f4fb, 0x086e_46bc);
    si_cus_scan(m, o, CUS_ACTOR);
    // channels
    init_status(m, CUS_CHNS, 5, b"Channels\0");
    env_path(m, o, CUS_CHNS, 0x086d_f511, 0, 0x0867_92f4);
    si_cus_scan(m, o, CUS_CHNS);
}

/// [soft 0x08532eb4 si_cusLoadEffectExtensions]: reads the extension file `<dir>/<name>` of an effect
/// (opened in binary mode) after its main one: `Dialog` adds a dialog command (the dialog is read by
/// si_cusDialogRead), `_DYNAMIC_LINK` reads the secondary dynamic link (once only), `OPERATOR` and
/// `_OPERATOR` read operator attributes into the relation and attribute dictionaries, `#` skips the
/// rest of its line, any other word is reported on stderr. 1 when the file is read (also when the effect
/// is already loaded, its +8 > 0) and held a dialog, else -1 (also on any failure, a null argument, an
/// unopened file, a second `_DYNAMIC_LINK`).
pub fn si_cus_load_effect_extensions(m: &mut Mem, o: &mut dyn Outside, eff: u32, links: u32, name: u32) -> i32 {
    let mut dialog_read = false;
    let mut link_read = false;
    if eff == 0 || name == 0 {
        return -1;
    }
    if m.u16(eff.wrapping_add(8)) as i16 > 0 {
        return 1;
    }
    let path = m.local(0x1a0);
    let tok = m.local(0x100);
    let line = m.local(0xa0);
    let mut p = m.cstr(m.u32(eff.wrapping_add(0x10c)));
    p.push(b'/');
    p.extend_from_slice(&m.cstr(name));
    m.put_cstr(path, &p);
    let r = (|| -> i32 {
        let fp = o.call(m, PLT_FOPEN, &[path, 0x086d_f5b4]).eax;
        if fp == 0 {
            return -1;
        }
        if m.u32(eff.wrapping_add(0x104)) == 0 {
            let d = si_cus_dso_data_create(m);
            m.set_u32(eff.wrapping_add(0x104), d);
            if d == 0 {
                o.call(m, FCLOSE, &[fp]);
                return -1;
            }
        }
        let dso = m.u32(eff.wrapping_add(0x104));
        let ac = si_dynamic_link_re_allocate(m, o, links, dso.wrapping_add(0x40));
        if ac == 0 {
            o.call(m, FCLOSE, &[fp]);
            return -1;
        }
        loop {
            m.set_u8(tok, 0);
            if o.call(m, FSCANF, &[fp, 0x086d_f5b7, tok]).eax as i32 != 1 {
                break;
            }
            if m.u8(tok) == b'#' {
                si_cus_line_read(m, o, fp, line, 0xa0);
                continue;
            }
            let w = m.cstr(tok);
            if w == b"Dialog" {
                let c = si_cus_new_cmd(m, eff);
                m.set_u16(c, 1);
                o.call(m, SI_CUS_DIALOG_READ, &[fp, eff.wrapping_add(0x10), links]);
                dialog_read = true;
            } else if w == b"_DYNAMIC_LINK" {
                if link_read {
                    o.call(m, FCLOSE, &[fp]);
                    return -1;
                }
                if o.call(m, SI_CUS_PARSE_SECONDARY_DYNAMIC_LINK, &[fp, ac]).eax as u16 == 0 {
                    o.call(m, FCLOSE, &[fp]);
                    return -1;
                }
                link_read = true;
            } else if w == b"OPERATOR" || w == b"_OPERATOR" {
                if o.call(m, SI_RELDICT_READ_OPERATOR_ATTRIBUTES, &[SI_RELDICT, SI_ATD, fp, eff]).eax as u16 == 0 {
                    o.call(m, FCLOSE, &[fp]);
                    return -1;
                }
            } else {
                let err = m.u32(STDERR);
                o.call(m, PLT_FPRINTF, &[err, 0x086d_f5e2, tok]);
            }
        }
        o.call(m, FCLOSE, &[fp]);
        if dialog_read {
            1
        } else {
            -1
        }
    })();
    m.release(0xa0);
    m.release(0x100);
    m.release(0x1a0);
    r
}

// ---- SI_cusLoadEffect ------------------------------------------------------------------------------------------

pub const SI_RELDICT_GET_GRP: Routine = Routine::new("SI_reldictGetGrp", 0x0813_f7e8);
pub const SI_RELDICT_GET_NAME: Routine = Routine::new("SI_reldictGetName", 0x0813_f98c);
pub const SI_RELDICT_GET_SCOPE: Routine = Routine::new("SI_reldictGetScope", 0x0813_f948);
pub const SI_RELDICT_IS_NEW_SRC: Routine = Routine::new("SI_reldictIsNewSrc", 0x0813_fa18);
pub const SI_RELDICT_GET_NB_RES: Routine = Routine::new("SI_reldictGetNbRes", 0x0813_eb7c);
pub const SI_RELDICT_GET_RES_PICK_MESSAGE: Routine = Routine::new("SI_reldictGetResPickMessage", 0x0813_f2f8);
pub const SI_RELDICT_ADD_RELATION: Routine = Routine::new("SI_reldictAddRelation", 0x0813_9b80);
pub const SI_RELDICT_GET_REL_RANK_BY_INDEX: Routine = Routine::new("SI_reldictGetRelRankByIndex", 0x0813_fa44);
pub const SI_RELDICT_GET_COND: Routine = Routine::new("SI_reldictGetCond", 0x0813_f904);
pub const SI_RELDICT_CUS_OP_DUMP: Routine = Routine::new("SI_reldictCusOpDump", 0x0813_eae0);
pub const SI_RELDICT_READ_ONE_OPERATOR: Routine = Routine::new("SI_reldictReadOneOperator", 0x0813_d75c);
pub const SI_RELDICT_GET_CHAP: Routine = Routine::new("SI_reldictGetChap", 0x0813_f7a0);
pub const SI_RELDICT_GET_MAX_FANOUT: Routine = Routine::new("SI_reldictGetMaxFanout", 0x0813_f9cc);
pub const SI_CUS_DIALOG_DSO_DATA_CREATE: Routine = Routine::new("si_cusDialogDsoDataCreate", 0x0854_aaec);
pub const SI_CUS_READ_DESCRIPTION_P: Routine = Routine::new("si_cusReadDescription", 0x0853_7428);
pub const SI_CUS_PARSE_DYNAMIC_LINK: Routine = Routine::new("si_cusParseDynamicLink", 0x0852_a6d4);
pub const SI_CUS_READ_SECONDARY_DIALOG: Routine = Routine::new("si_cusReadSecondaryDialog", 0x0852_aa58);

const FMT_PATH: u32 = 0x086d_f5fe;
const MODE_RB: u32 = 0x086d_f608;
const FMT_KEYWORD: u32 = 0x086d_f60b;
const FMT_MENU_D: u32 = 0x086d_f613;
const S_INVALID_CMD: u32 = 0x086d_fc3e;

/// New command of kind `kind` on the effect's list.
fn lf_cmd(m: &mut Mem, eff: u32, kind: u16) -> u32 {
    let c = si_cus_new_cmd(m, eff);
    m.set_u16(c, kind);
    c
}

/// The error exit that frees the command list, the dso data and closes the file (the original's shared tails).
fn lf_fail_dso(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32) -> i32 {
    si_cus_free_command_list(m, o, eff.wrapping_add(0xc8));
    si_cus_dso_data_delete(m, m.u32(eff.wrapping_add(0x104)));
    m.set_u32(eff.wrapping_add(0x104), 0);
    o.call(m, FCLOSE, &[fp]);
    -1
}

/// [soft 0x08533138 SI_cusLoadEffect]: reads the `.cus` file `<dir>/<name>.cus` of effect `eff` (directory
/// at +0x10c, name at +4) into its command list: one command per keyword (kinds as in the original's keyword
/// table; text arguments are copied with `strdup`), the dialog, the `_DYNAMIC_LINK`, `OPERATOR`,
/// `_DESCRIPTION` and `_SECONDARY_DIALOGS` sections through their readers; then the icon/operator relation
/// set-up, the dso link names, the value list copy and prune, the dialog selection and the effect's
/// kind (3 for a status of kind 5, 2 composite, 1 icon, 4 hybrid icon). Answers 1 (also when the effect
/// already is loaded: +8 above 0), -1 on an unopenable file or any error (reported through `SI_error` and
/// the status line). Differences kept: none known; the buffers of the original's frame are laid out
/// contiguously in the same offsets.
pub fn si_cus_load_effect(m: &mut Mem, o: &mut dyn Outside, status: u32, eff: u32) -> i32 {
    if m.u16(eff.wrapping_add(8)) as i16 > 0 {
        return 1;
    }
    let f = m.local(0x400);
    let at = |off: u32| f.wrapping_add(0x400).wrapping_sub(off);
    let r = load_effect_frame(m, o, status, eff, at(0x1c8), at(0x128), at(0x100), at(0x3f4), at(0x3f6), at(0x3f0), at(0x1f0));
    m.release(0x400);
    r
}

#[allow(clippy::too_many_arguments)]
fn load_effect_frame(m: &mut Mem, o: &mut dyn Outside, status: u32, eff: u32, b1c8: u32, b128: u32, key: u32, menu: u32, flag: u32, b3f0: u32, info: u32) -> i32 {
    let mut has_secdlg = false; // -0x410
    let mut has_op = false; // -0x40c
    let mut composite = 0i32; // -0x408
    let mut icon = 0i32; // -0x404
    let mut icon_cmd = 0u32; // -0x400
    let mut has_dialog = 0i32; // -0x414
    let dir = m.u32(eff.wrapping_add(0x10c));
    let name = m.u32(eff.wrapping_add(4));
    let t = cfmt::format(m, FMT_PATH, &[dir, name]);
    m.put_cstr(b1c8, &t);
    let fp = o.call(m, PLT_FOPEN, &[b1c8, MODE_RB]).eax;
    if fp == 0 {
        return -1;
    }
    m.set_u16(eff, 0);
    loop {
        m.set_u8(key, 0);
        let n = o.call(m, FSCANF, &[fp, FMT_KEYWORD, key]).eax as i32;
        if n != 1 {
            break;
        }
        let kw = m.cstr(key);
        let kw: &[u8] = &kw;
        if kw.first() == Some(&b'#') {
            si_cus_line_read(m, o, fp, b1c8, 0xa0);
            continue;
        }
        match kw {
            b"Menu" => {
                o.call(m, FSCANF, &[fp, FMT_MENU_D, menu]);
            }
            b"Icon" | b"IconRead" | b"IconHybrid" if m.u16(status) != 5 => {
                let c = lf_cmd(m, eff, 0x1e);
                o.call(m, FSCANF, &[fp, 0x086d_f62f, b128, b1c8]);
                let a = clib::strdup(m, b128);
                m.set_u32(c.wrapping_add(4), a);
                let b = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(8), b);
                icon = if kw == b"IconHybrid" { -1 } else { 1 };
                icon_cmd = c;
            }
            b"Dialog" => {
                let c = lf_cmd(m, eff, 1);
                let _ = c;
                o.call(m, SI_CUS_DIALOG_READ, &[fp, eff.wrapping_add(0x10), 0xffff_ffff]);
                has_dialog = 1;
            }
            b"Data" | b"DataWrite" => lf_two_same(m, o, eff, fp, b1c8, 2, 0x086d_f656),
            b"Frame" => lf_two_same(m, o, eff, fp, b1c8, 0x23, 0x086d_f65f),
            b"Time" => lf_two_same(m, o, eff, fp, b1c8, 0x1d, 0x086d_f667),
            b"Select" => lf_select(m, o, eff, fp, b1c8, b128),
            b"Mouse" => lf_line(m, o, eff, fp, b1c8, 0x1c),
            b"Message" => lf_line(m, o, eff, fp, b1c8, 0xb),
            b"Write" => lf_write(m, o, eff, fp, b1c8, b128),
            b"SceneWrite" => lf_scene(m, o, eff, fp, b1c8, 0x10, 0x086d_f72a),
            b"ReadScene" => lf_scene(m, o, eff, fp, b1c8, 0x14, 0x086d_f737),
            b"ReadAnimatedModel" => lf_scene(m, o, eff, fp, b1c8, 0x49, 0x086d_f74c),
            b"WriteAnimatedModel" => lf_scene(m, o, eff, fp, b1c8, 0x4a, 0x086d_f762),
            b"Read" => {
                let c = lf_cmd(m, eff, 0x12);
                o.call(m, FSCANF, &[fp, 0x086d_f76a, b128, b1c8]);
                let a = clib::strdup(m, b128);
                m.set_u32(c.wrapping_add(4), a);
                let b = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(8), b);
            }
            b"Replace" => lf_replace(m, o, eff, fp, b1c8, b128),
            b"Savekey" => lf_savekey(m, o, eff, fp, b1c8, b128),
            b"Refresh" => {
                lf_cmd(m, eff, 0x38);
            }
            b"Shell" => lf_line(m, o, eff, fp, b1c8, 0x11),
            b"NT_Shell" => lf_line(m, o, eff, fp, b1c8, 0x4d),
            b"Process" => {
                let c = lf_cmd(m, eff, 0x1b);
                o.call(m, FSCANF, &[fp, 0x086d_f82b, b128]);
                let a = clib::strdup(m, b128);
                m.set_u32(c.wrapping_add(4), a);
                si_cus_line_read(m, o, fp, b1c8, 0xa0);
                let b = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(0xc), b);
            }
            b"Resolution" => {
                let c = lf_cmd(m, eff, 0x29);
                o.call(m, FSCANF, &[fp, 0x086d_f839, b1c8]);
                let a = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(8), a);
                let b = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(4), b);
            }
            b"Image" => {
                let c = lf_cmd(m, eff, 0x2a);
                o.call(m, FSCANF, &[fp, 0x086d_f842, b1c8]);
                let a = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(8), a);
                let b = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(4), b);
                if si_cus_line_read(m, o, fp, b128, 0xa0) != 0 {
                    cus_next_string(m, b128, b1c8);
                    let w = m.cstr(b1c8);
                    if w == b"-z" {
                        m.set_u16(c, 0x2c);
                    }
                    if w == b"-rgbz" {
                        m.set_u16(c, 0x2b);
                    }
                }
            }
            b"CameraData" => lf_two_same(m, o, eff, fp, b1c8, 0x25, 0x086d_f859),
            b"Composite" => {
                let c = lf_cmd(m, eff, 0x26);
                o.call(m, FSCANF, &[fp, 0x086d_f866, b1c8]);
                let a = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(8), a);
                let b = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(4), b);
                if si_cus_line_read(m, o, fp, b128, 0xa0) != 0 {
                    cus_next_string(m, b128, b1c8);
                    let w = m.cstr(b1c8);
                    if w == b"-under" {
                        m.set_u16(c, 0x27);
                    }
                    if w == b"-z" {
                        m.set_u16(c, 0x28);
                    }
                }
                composite = 1;
            }
            b"Channels" => {
                let c = lf_cmd(m, eff, 0x36);
                o.call(m, FSCANF, &[fp, 0x086d_f87c, b1c8]);
                m.set_u32(c.wrapping_add(4), 0);
                let a = clib::strdup(m, b1c8);
                m.set_u32(c.wrapping_add(8), a);
            }
            b"Quit" => {
                lf_cmd(m, eff, 0x1f);
            }
            b"_DYNAMIC_LINK" => {
                let dso = m.u32(eff.wrapping_add(0x104));
                if dso == 0 {
                    let d = si_cus_dso_data_create(m);
                    m.set_u32(eff.wrapping_add(0x104), d);
                    if d == 0 {
                        o.call(m, SI_ERROR, &[0x46, 0x086d_f920]);
                        si_cus_free_command_list(m, o, eff.wrapping_add(0xc8));
                        o.call(m, FCLOSE, &[fp]);
                        return -1;
                    }
                } else if m.u32(dso) != 0 {
                    o.call(m, SI_ERROR, &[3, 0x086d_f8a0]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_f8e0, 3]);
                    return lf_fail_dso(m, o, eff, fp);
                }
                let link = si_dynamic_link_create(m);
                let dso = m.u32(eff.wrapping_add(0x104));
                m.set_u32(dso, link);
                if link == 0 {
                    o.call(m, SI_ERROR, &[0x46, 0x086d_f980]);
                    return lf_fail_dso(m, o, eff, fp);
                }
                let dso = m.u32(eff.wrapping_add(0x104));
                let l = m.u32(dso);
                if o.call(m, SI_CUS_PARSE_DYNAMIC_LINK, &[fp, l]).eax as u16 == 0 {
                    o.call(m, SI_ERROR, &[4, 0x086d_f9e0]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fa20, 3]);
                    return lf_fail_dso(m, o, eff, fp);
                }
            }
            b"OPERATOR" | b"_OPERATOR" => {
                let pd = m.u32(SI_PROC_DATA);
                let d = m.u32(pd).wrapping_shl(3).wrapping_sub(m.u32(pd)).wrapping_shl(4).wrapping_add(m.u32(SI_ELEM_DICTS));
                if o.call(m, SI_RELDICT_READ_ONE_OPERATOR, &[d, SI_RELDICT, SI_ATD, fp, 1, eff]).eax as u16 == 0 {
                    o.call(m, SI_ERROR, &[4, 0x086d_fa80]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fac0, 3]);
                    return lf_fail_dso(m, o, eff, fp);
                }
                has_op = true;
            }
            b"_DESCRIPTION" => {
                if o.call(m, SI_CUS_READ_DESCRIPTION_P, &[fp, eff.wrapping_add(0x108)]).eax as u16 == 0 {
                    o.call(m, SI_ERROR, &[4, 0x086d_fb20]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fb60, 3]);
                    return -1;
                }
            }
            b"_SECONDARY_DIALOGS" => {
                if o.call(m, SI_CUS_READ_SECONDARY_DIALOG, &[fp, eff]).eax as u16 == 0 {
                    o.call(m, SI_ERROR, &[4, 0x086d_fbc0]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fc00, 3]);
                    return lf_fail_dso(m, o, eff, fp);
                }
                has_secdlg = true;
            }
            _ => {
                let err = m.u32(STDERR);
                o.call(m, PLT_FPRINTF, &[err, S_INVALID_CMD, key]);
            }
        }
    }
    o.call(m, FCLOSE, &[fp]);
    let dso = m.u32(eff.wrapping_add(0x104));
    if ((dso != 0 && m.u32(dso) != 0) || has_op || has_secdlg) && si_cus_effect_is_process_based(m, eff) as u16 != 0 {
        o.call(m, SI_ERROR, &[4, 0x086d_fc60]);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fca0, 3]);
        si_cus_free_command_list(m, o, eff.wrapping_add(0xc8));
        si_cus_dso_data_delete(m, m.u32(eff.wrapping_add(0x104)));
        m.set_u32(eff.wrapping_add(0x104), 0);
        return -1;
    }
    if icon == 1 && has_op {
        let rel_name = status.wrapping_add(2);
        let rel = o.call(m, SI_RELDICT_GET_REL_INDEX_FROM_NAME, &[SI_RELDICT, rel_name, m.u32(eff.wrapping_add(4))]).eax as i32;
        if rel == -1 {
            o.call(m, SI_ERROR, &[4, 0x086d_fce0]);
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fd40, 3]);
            return -1;
        }
        let nres = o.call(m, SI_RELDICT_GET_NB_RES, &[SI_RELDICT, rel as u32]).eax as i32;
        m.set_u32(info, rel_name);
        m.set_u32(info.wrapping_add(8), 0);
        let ic4 = m.u32(icon_cmd.wrapping_add(4));
        m.set_u32(info.wrapping_add(0x10), ic4);
        m.set_u32(info.wrapping_add(0x18), 0);
        m.set_u16(info.wrapping_add(0x24), 0);
        m.set_u16(info.wrapping_add(0x20), 1);
        let mut k = 0i32;
        while k < nres {
            let ix = rel.wrapping_add(k) as u32;
            let cond = o.call(m, SI_RELDICT_GET_COND, &[SI_RELDICT, ix, 1]).eax;
            m.set_u32(info.wrapping_add(4), cond);
            let nm = o.call(m, SI_RELDICT_GET_NAME, &[SI_RELDICT, ix, 1]).eax;
            m.set_u32(info.wrapping_add(0xc), nm);
            let pick = o.call(m, SI_RELDICT_GET_RES_PICK_MESSAGE, &[SI_RELDICT, ix]).eax;
            m.set_u32(info.wrapping_add(0x14), pick);
            let scope = o.call(m, SI_RELDICT_GET_SCOPE, &[SI_RELDICT, ix, 1]).eax as u16;
            m.set_u16(info.wrapping_add(0x22), scope);
            let ns = o.call(m, SI_RELDICT_IS_NEW_SRC, &[SI_RELDICT, ix]).eax as u16;
            m.set_u16(info.wrapping_add(0x1c), ns);
            let fan = o.call(m, SI_RELDICT_GET_MAX_FANOUT, &[SI_RELDICT, ix, 1]).eax as u16;
            m.set_u16(info.wrapping_add(0x1e), fan);
            let chap = o.call(m, SI_RELDICT_GET_CHAP, &[SI_RELDICT, ix, 1]).eax;
            let grp = o.call(m, SI_RELDICT_GET_GRP, &[SI_RELDICT, ix, 1]).eax;
            let rank = o.call(m, SI_RELDICT_GET_REL_RANK_BY_INDEX, &[SI_RELDICT, ix]).eax;
            let models = m.u32(SI_CID_MODELS);
            let en = m.u32(eff.wrapping_add(4));
            o.call(
                m,
                SI_RELDICT_ADD_RELATION,
                &[SI_RELDICT, en, 20000, chap, models, grp, 5, rank, 0x7fff, 0, 0, 0, 1, info, 0, 3],
            );
            k += 1;
        }
    }
    let dso = m.u32(eff.wrapping_add(0x104));
    if dso != 0 {
        let link = m.u32(dso);
        if link != 0 && has_secdlg {
            let ndlg = (m.u32(eff.wrapping_add(0x18)) as i32).wrapping_sub(has_dialog);
            let nm = m.u32(link);
            clib::strcpy(m, b3f0, nm);
            let mut i = 0i32;
            let mut off = 0u32;
            while i < ndlg {
                let arr = m.u32(m.u32(eff.wrapping_add(0x104)).wrapping_add(0x40));
                if si_dynamic_link_set_name(m, b3f0, arr.wrapping_add(off)) as u16 == 0 {
                    o.call(m, SI_ERROR, &[4, 0x086d_fda0]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fde0, 3]);
                    return -1;
                }
                off = off.wrapping_add(0xc);
                i += 1;
            }
        }
        o.call(m, SI_RELDICT_CUS_OP_DUMP, &[SI_RELDICT]);
    }
    if (m.u32(eff.wrapping_add(0x18)) as i32) > 0 {
        m.set_u16(flag, 0);
        let vl = si_cus_value_list_copy(m, o, m.u32(eff.wrapping_add(0x64)));
        m.set_u32(eff.wrapping_add(0xb4), vl);
        if m.u32(eff.wrapping_add(0x64)) != 0 && vl == 0 {
            o.call(m, SI_ERROR, &[4, 0x086d_fe20]);
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fe60, 3]);
            return -1;
        }
        let dso = m.u32(eff.wrapping_add(0x104));
        if dso == 0 || m.u32(dso) == 0 {
            let w = m.u32(eff.wrapping_add(0x1c));
            m.set_u32(eff.wrapping_add(0x14), w);
        } else {
            let dlg = eff.wrapping_add(0x10);
            if cus_value_list_remove_unsupported_symbols(m, o, dlg, eff.wrapping_add(0x64), flag) as u16 == 1 {
                if m.u16(flag) == 0 {
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_fea0, 2]);
                }
                cus_value_list_remove_unsupported_symbols(m, o, dlg, eff.wrapping_add(0xb4), flag);
            }
            let dd = o.call(m, SI_CUS_DIALOG_DSO_DATA_CREATE, &[dlg]).eax;
            m.set_u32(eff.wrapping_add(0xc4), dd);
            if has_dialog == 1 {
                let count = m.u32(eff.wrapping_add(0x18)) as i32;
                let arr = m.u32(eff.wrapping_add(0x1c));
                let mut idx: i32 = -1;
                if count >= 1 {
                    if m.u16(arr) == 100 {
                        idx = 0;
                    } else {
                        let mut e = 0i32;
                        loop {
                            e += 1;
                            if e >= count {
                                break;
                            }
                            if m.u16(arr.wrapping_add((e as u32).wrapping_mul(0x3c))) == 100 {
                                idx = e;
                                break;
                            }
                        }
                    }
                }
                if idx < 0 {
                    o.call(m, SI_ERROR, &[4, 0x086d_ff20]);
                    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[0x086d_ff60, 3]);
                    return -1;
                }
                let p = (idx as u32).wrapping_mul(0x3c).wrapping_add(arr);
                m.set_u32(eff.wrapping_add(0x14), p);
                let dd = m.u32(eff.wrapping_add(0xc4));
                if dd != 0 {
                    m.set_u32(eff.wrapping_add(0xc0), dd.wrapping_add((idx as u32).wrapping_mul(0x10)));
                }
            }
            let count = m.u32(eff.wrapping_add(0x18)) as i32;
            let mut off = 0u32;
            let mut i = 0;
            while i < count {
                let arr = m.u32(eff.wrapping_add(0x1c));
                m.set_u16(arr.wrapping_add(off).wrapping_add(0x38), 1);
                off = off.wrapping_add(0x3c);
                i += 1;
            }
        }
    }
    si_cus_resolve_name_links(m, o, eff);
    m.set_u16(eff.wrapping_add(8), 1);
    if m.u16(status) == 5 {
        m.set_u16(eff, 3);
    } else if composite != 0 {
        m.set_u16(eff, 2);
    } else if icon > 0 {
        m.set_u16(eff, 1);
    } else if icon < 0 {
        m.set_u16(eff, 4);
    }
    1
}

/// `Data`, `Frame`, `Time`, `CameraData`: one word, copied to +8 and +4.
fn lf_two_same(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, kind: u16, fmt: u32) {
    let c = lf_cmd(m, eff, kind);
    o.call(m, FSCANF, &[fp, fmt, b1c8]);
    let a = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(8), a);
    let b = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(4), b);
}

/// `Mouse`, `Message`, `Shell`, `NT_Shell`: the rest of the line copied to +0xc.
fn lf_line(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, kind: u16) {
    let c = lf_cmd(m, eff, kind);
    si_cus_line_read(m, o, fp, b1c8, 0xa0);
    let a = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(0xc), a);
}

/// `SceneWrite`, `ReadScene`, `ReadAnimatedModel`, `WriteAnimatedModel`: one word copied to +8.
fn lf_scene(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, kind: u16, fmt: u32) {
    let c = lf_cmd(m, eff, kind);
    o.call(m, FSCANF, &[fp, fmt, b1c8]);
    let a = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(8), a);
}

/// `Select`: the selection type word picks the kind (spline 4, mesh 5, patch 6, face 7, model 8, branch 9,
/// hrc 0xa, nurbsCurve 0x47, nurbsSurface 0x48) and a name word follows it.
fn lf_select(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, b128: u32) {
    let c = lf_cmd(m, eff, 0); // kind written by the matches below (the original does not zero it)
    o.call(m, FSCANF, &[fp, 0x086d_f671, b1c8]);
    let table: [(&[u8], u16, u32); 9] = [
        (b"spline", 4, 0x086d_f67b),
        (b"mesh", 5, 0x086d_f683),
        (b"patch", 6, 0x086d_f68c),
        (b"face", 7, 0x086d_f694),
        (b"model", 8, 0x086d_f69d),
        (b"branch", 9, 0x086d_f6a7),
        (b"hrc", 0xa, 0x086d_f6ae),
        (b"nurbsCurve", 0x47, 0x086d_f6bc),
        (b"nurbsSurface", 0x48, 0x086d_f6cc),
    ];
    for (w, kind, fmt) in table {
        if m.cstr(b1c8) == w {
            m.set_u16(c, kind);
            o.call(m, FSCANF, &[fp, fmt, b128]);
            let a = clib::strdup(m, b128);
            m.set_u32(c.wrapping_add(4), a);
        }
    }
}

/// `Write`: two words, then the option line: `-srt` (kind 0xf, 0x39 with `-g`), `-matrix` (0x3b, 0x3d with `-g`),
/// `-user_data` 0x2e, `-hrc_srt` 0x2d, `-tag` 0xd, `-original` 0xe; each test looks at the option word buffer, which
/// the `-g` read overwrites.
fn lf_write(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, b128: u32) {
    let c = lf_cmd(m, eff, 0xc);
    o.call(m, FSCANF, &[fp, 0x086d_f6e3, b128, b1c8]);
    let a = clib::strdup(m, b128);
    m.set_u32(c.wrapping_add(4), a);
    let b = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(8), b);
    if si_cus_line_read(m, o, fp, b128, 0xa0) == 0 {
        return;
    }
    let rest = cus_next_string(m, b128, b1c8);
    if m.cstr(b1c8) == b"-srt" {
        if rest != 0 {
            cus_next_string(m, rest, b1c8);
            m.set_u16(c, if m.cstr(b1c8) == b"-g" { 0x39 } else { 0xf });
        } else {
            m.set_u16(c, 0xf);
        }
    }
    if m.cstr(b1c8) == b"-matrix" {
        if rest != 0 {
            cus_next_string(m, rest, b1c8);
            m.set_u16(c, if m.cstr(b1c8) == b"-g" { 0x3d } else { 0x3b });
        } else {
            m.set_u16(c, 0x3b);
        }
    }
    let w = m.cstr(b1c8);
    if w == b"-user_data" {
        m.set_u16(c, 0x2e);
    }
    if w == b"-hrc_srt" {
        m.set_u16(c, 0x2d);
    }
    if w == b"-tag" {
        m.set_u16(c, 0xd);
    }
    if w == b"-original" {
        m.set_u16(c, 0xe);
    }
}

/// `Replace`: like `Write` with the kinds 0x16 (`-srt`, 0x3a with `-g`), 0x3c (`-matrix`, 0x3e with `-g`), `-normal` 0x19,
/// `-tag` 0x1a, `-user_data` 0x30, `-hrc_srt` 0x2f, `-material` 0x24, `-all` 0x17 (0x4c with a following `-n`),
/// `-geometry` 0x15 (0x4b with `-n`), `-original` 0x37.
fn lf_replace(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, b128: u32) {
    let c = lf_cmd(m, eff, 0x18);
    o.call(m, FSCANF, &[fp, 0x086d_f778, b128, b1c8]);
    let a = clib::strdup(m, b128);
    m.set_u32(c.wrapping_add(4), a);
    let b = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(8), b);
    if si_cus_line_read(m, o, fp, b128, 0xa0) == 0 {
        return;
    }
    let rest = cus_next_string(m, b128, b1c8);
    if m.cstr(b1c8) == b"-srt" {
        if rest != 0 {
            cus_next_string(m, rest, b1c8);
            m.set_u16(c, if m.cstr(b1c8) == b"-g" { 0x3a } else { 0x16 });
        } else {
            m.set_u16(c, 0x16);
        }
    }
    if m.cstr(b1c8) == b"-matrix" {
        if rest != 0 {
            cus_next_string(m, rest, b1c8);
            m.set_u16(c, if m.cstr(b1c8) == b"-g" { 0x3e } else { 0x3c });
        } else {
            m.set_u16(c, 0x3c);
        }
    }
    for (w, k) in [(&b"-normal"[..], 0x19u16), (b"-tag", 0x1a), (b"-user_data", 0x30), (b"-hrc_srt", 0x2f), (b"-material", 0x24)] {
        if m.cstr(b1c8) == w {
            m.set_u16(c, k);
        }
    }
    if m.cstr(b1c8) == b"-all" {
        m.set_u16(c, 0x17);
        if rest != 0 {
            cus_next_string(m, rest, b1c8);
            if m.cstr(b1c8) == b"-n" {
                m.set_u16(c, 0x4c);
            }
        }
    }
    if m.cstr(b1c8) == b"-geometry" {
        m.set_u16(c, 0x15);
        if rest != 0 {
            cus_next_string(m, rest, b1c8);
            if m.cstr(b1c8) == b"-n" {
                m.set_u16(c, 0x4b);
            }
        }
    }
    if m.cstr(b1c8) == b"-original" {
        m.set_u16(c, 0x37);
    }
}

/// `Savekey`: two words, then one option word: `-s` 0x3f, `-r` 0x40, `-t` 0x41, `-hrc_s` 0x43, `-hrc_r` 0x44,
/// `-hrc_t` 0x45, `-hrc` 0x46 (the command starts as kind 0x42).
fn lf_savekey(m: &mut Mem, o: &mut dyn Outside, eff: u32, fp: u32, b1c8: u32, b128: u32) {
    let c = lf_cmd(m, eff, 0x42);
    o.call(m, FSCANF, &[fp, 0x086d_f7e3, b128, b1c8]);
    let a = clib::strdup(m, b128);
    m.set_u32(c.wrapping_add(4), a);
    let b = clib::strdup(m, b1c8);
    m.set_u32(c.wrapping_add(8), b);
    if si_cus_line_read(m, o, fp, b128, 0xa0) == 0 {
        return;
    }
    cus_next_string(m, b128, b1c8);
    let w = m.cstr(b1c8);
    for (s, k) in [(&b"-s"[..], 0x3fu16), (b"-r", 0x40), (b"-t", 0x41), (b"-hrc_s", 0x43), (b"-hrc_r", 0x44), (b"-hrc_t", 0x45), (b"-hrc", 0x46)] {
        if w == s {
            m.set_u16(c, k);
            break;
        }
    }
}
