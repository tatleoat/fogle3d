// SPDX-License-Identifier: MIT
//! `dbicons.c` of the program: the icon view of the database browser (finding 0007,
//! "Database browser", 2026-10-28 section).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Everything works on the shared memory image: the dialog, its
//! item records (0x28 bytes each, the icon view's data behind the pointer at +0x20), the
//! text list (`SI_Textlist`: a 16-bit count at +0, the node array at +4, 0x14 bytes a node),
//! elements and chapters (`si-types`). Records whose layout is not established are addressed
//! by named offsets, as the module constants say.

use crate::calls::DbCalls;
use crate::imglobals::{cid, GLB_DB_ICON, GLB_DIR_ICON, GLB_NO_ICON, SI_CID};
use si_mem::{Mem, Outside};

/// `__SI_Dialog`: the array of item records (0x28 bytes each).
const DLG_ITEMS: u32 = 0x14;
/// Size of a dialog item record.
const ITEM: u32 = 0x28;
/// A dialog item's pointer to its data (for the icon view a `TviconData`).
const ITEM_DATA: u32 = 0x20;
/// `SI_Textlist`: the node array.
const TL_NODES: u32 = 4;
/// Size of a node of a text list.
const TL_NODE: u32 = 0x14;
/// The icon record `SI_dbChapterIconGet` and the first-use code make: 12 bytes, width and
/// height as 16-bit words at +4 and +6, the pixel pointer at +8.
const ICON_SIZE: u32 = 12;

/// Strings of the read-only data this file hands out by address.
pub mod strings {
    /// (address, text) of the chapter icon names, in `dbGetChapIconName`'s order of tests.
    pub const DIR_ICON: u32 = 0x08680f3d;
    pub const DIR_ICON_2: u32 = 0x08680f45;
    pub const NO_ICON: u32 = 0x08680f4d;
    pub const NO_ICON_DEFAULT: u32 = 0x08680fec;
    /// Every string literal of `dbicons.c` that a function here reads or hands on.
    pub const ALL: &[(u32, &str)] = &[
        (0x08680eec, "%c%-1d%c"),
        (0x08680ef5, "%c%-1d-%-1d%c"),
        (0x08680f03, ".pic"),
        (0x08680f08, "all"),
        (0x08680f0c, "%s"),
        (0x08680f0f, "PICTURES"),
        (0x08680f18, "RENDER_PICTURES"),
        (0x08680f28, "%s"),
        (0x08680f2b, "%s%s"),
        (0x08680f30, "%s.%s-%s"),
        (0x08680f39, "all"),
        (DIR_ICON, "dirIcon"),
        (DIR_ICON_2, "dirIcon"),
        (NO_ICON, "noIcon"),
        (0x08680f54, "scnIcon"),
        (0x08680f5c, "aniIcon"),
        (0x08680f64, "camIcon"),
        (0x08680f6c, "clsIcon"),
        (0x08680f74, "latIcon"),
        (0x08680f7c, "ligIcon"),
        (0x08680f84, "mtrIcon"),
        (0x08680f8c, "hrcIcon"),
        (0x08680f94, "palIcon"),
        (0x08680f9c, "sfkIcon"),
        (0x08680fa4, "skyIcon"),
        (0x08680fac, "stsIcon"),
        (0x08680fb4, "stcIcon"),
        (0x08680fbc, "shpIcon"),
        (0x08680fc4, "txtIcon"),
        (0x08680fcc, "t3DIcon"),
        (0x08680fd4, "wavIcon"),
        (0x08680fdc, "expIcon"),
        (0x08680fe4, "spdIcon"),
        (NO_ICON_DEFAULT, "noIcon"),
        (0x08680ff3, "%s"),
        (0x08680ff6, "%s.%s"),
        (0x08680ffc, "%s.icn"),
        (0x08681003, "%s%s%s"),
        (0x0868100a, "%s.icn"),
    ];
}

/// Writes the string literals of this file into the image (a check's image holds none of the
/// binary's data).
pub fn store_strings(m: &mut Mem) {
    for (a, s) in strings::ALL {
        m.put_cstr(*a, s.as_bytes());
    }
}

/// [soft 0x0810c030 si_dbFormatSequenceFrames]
///
/// Writes the frame range of a picture sequence element into `buf`: `[first]` when first and
/// last are equal, else `[first-last]`; braces in place of brackets when the element's flag
/// at +0x7e is non-zero. Returns `buf`.
pub fn format_sequence_frames(m: &mut Mem, elem: u32, buf: u32) -> u32 {
    let braces = m.i16(elem + 0x7e) != 0;
    let (open, close) = if braces { (b'{', b'}') } else { (b'[', b']') };
    let first = m.i32(elem + 0x74);
    let last = m.i32(elem + 0x78);
    let mut text = vec![open];
    text.extend(si_mem::clib::decimal(first));
    if first != last {
        text.push(b'-');
        text.extend(si_mem::clib::decimal(last));
    }
    text.push(close);
    m.put_cstr(buf, &text);
    buf
}

/// [soft 0x0810c0a4 si_dbNameIsPicture]
///
/// 1 when the name is longer than three characters and ends in `.pic`, else 0. (The original
/// finds the length with the word-at-a-time scan, which may read up to three bytes past the
/// terminator inside an aligned word; the answer does not depend on them.)
pub fn name_is_picture(m: &Mem, name: u32) -> i32 {
    let len = m.strlen(name);
    (len > 3 && m.bytes(name + len - 4, 4) == b".pic") as i32
}

/// [soft 0x0810c660 dbGetChapIconName]
///
/// The address of the icon file name for a chapter index: -1 and -2 the directory icon,
/// -3 the "no icon" name, then the first of the scene dictionary's chapter indices that
/// equals it, in the original's order of tests; the "no icon" string at the end otherwise.
pub fn get_chap_icon_name(m: &Mem, chap: i32) -> u32 {
    match chap {
        -1 => return strings::DIR_ICON,
        -2 => return strings::DIR_ICON_2,
        -3 => return strings::NO_ICON,
        _ => {}
    }
    let cid_at = |off: u32| m.i32(SI_CID.0 + off);
    let table: [(u32, u32); 18] = [
        (cid::DSC, 0x08680f54),
        (cid::ANI, 0x08680f5c),
        (cid::CAM, 0x08680f64),
        (cid::CLS, 0x08680f6c),
        (cid::LAT, 0x08680f74),
        (cid::LIG, 0x08680f7c),
        (cid::MAT, 0x08680f84),
        (cid::MODEL, 0x08680f8c),
        (cid::PAL, 0x08680f94),
        (cid::SFK, 0x08680f9c),
        (cid::SKY, 0x08680fa4),
        (cid::STS, 0x08680fac),
        (cid::STC, 0x08680fb4),
        (cid::SHP, 0x08680fbc),
        (cid::T2D, 0x08680fc4),
        (cid::T3D, 0x08680fcc),
        (cid::WAV, 0x08680fd4),
        (cid::EXP, 0x08680fdc),
    ];
    for (off, name) in table {
        if chap == cid_at(off) {
            return name;
        }
    }
    if chap == cid_at(cid::SPD) {
        0x08680fe4
    } else {
        strings::NO_ICON_DEFAULT
    }
}

/// The text list behind a dialog item's data pointer (the icon view keeps it at +4 of the
/// item's `TviconData`): the item record is `items + (item - 1) * 0x28`, its data pointer at
/// +0x20. 0 when the dialog has none.
fn item_data(m: &Mem, dialog: u32, item: i32) -> u32 {
    let items = m.u32(dialog + DLG_ITEMS);
    m.u32(items.wrapping_add((item as u32).wrapping_mul(ITEM)).wrapping_sub(ITEM).wrapping_add(ITEM_DATA))
}

/// [soft 0x0810e4cc SI_dbListIconSetSel]
///
/// Clears the selection flag (the first 16-bit word of each node) of every node of the
/// item's text list and sets node `index` to `value`. -1 when the item has no data or list,
/// `index` is negative or larger than the node count (an index equal to the count is
/// accepted, as in the original), else 0.
pub fn list_icon_set_sel(m: &mut Mem, dialog: u32, item: i32, index: i32, value: i16) -> i32 {
    let data = item_data(m, dialog, item);
    if data == 0 {
        return -1;
    }
    let list = m.u32(data + 4);
    if list == 0 || index < 0 || (m.i16(list) as i32) < index {
        return -1;
    }
    let count = m.i16(list) as i32;
    let nodes = m.u32(list + TL_NODES);
    for i in 0..count {
        m.set_i16(nodes.wrapping_add(i as u32 * TL_NODE), 0);
    }
    m.set_i16(nodes.wrapping_add(index as u32 * TL_NODE), value);
    0
}

/// [soft 0x0810e550 SI_dbListIconGetSel]
///
/// The index of the first node of the item's text list whose selection word is non-zero,
/// -1 when there is none or the item has no list.
pub fn list_icon_get_sel(m: &Mem, dialog: u32, item: i32) -> i32 {
    let data = item_data(m, dialog, item);
    if data == 0 {
        return -1;
    }
    let list = m.u32(data + 4);
    if list == 0 {
        return -1;
    }
    let count = m.i16(list) as i32;
    let nodes = m.u32(list + TL_NODES);
    for i in 0..count {
        if m.i16(nodes.wrapping_add(i as u32 * TL_NODE)) != 0 {
            return i;
        }
    }
    -1
}

/// [soft 0x0810e9c0 SI_dbGetIconList]
///
/// For an icon-view record (whose pointer at +4 is the text list): stores the list's node
/// count at `*count` and returns the node array; 0 for a null record.
pub fn get_icon_list(m: &mut Mem, view: u32, count: u32) -> u32 {
    if view == 0 {
        return 0;
    }
    let list = m.u32(view + 4);
    let n = m.i16(list) as i32;
    m.set_i32(count, n);
    m.u32(list + 4)
}

/// [soft 0x0810d708 si_dbListTurnOffIcons]
///
/// Frees the icon data of the dialog's items: of item `which - 1` when `which` is not
/// negative, else of all items (and clears the dialog's word at +0x2c). Only items of kind 7
/// have data: five blocks (at +0x18, +0x1c, +0x20, +0x10, +0x14, in that order) and then the
/// data record itself.
pub fn list_turn_off_icons(m: &mut Mem, dialog: u32, which: i32) {
    let (first, end) = if which < 0 {
        m.set_u32(dialog + 0x2c, 0);
        (0, m.i16(dialog + 0xc) as i32)
    } else {
        (which - 1, which)
    };
    let mut i = first;
    while i < end {
        let item = m.u32(dialog + DLG_ITEMS).wrapping_add((i as u32).wrapping_mul(ITEM));
        if m.i32(item) == 7 {
            let data = m.u32(item + 0x20);
            if data != 0 {
                for off in [0x18, 0x1c, 0x20, 0x10, 0x14] {
                    let p = m.u32(data + off);
                    if p != 0 {
                        m.free(p);
                        m.set_u32(data + off, 0);
                    }
                }
                m.free(data);
                m.set_u32(item + 0x20, 0);
            }
        }
        i += 1;
    }
}

/// Makes the 12-byte icon record of a picture the way the first-use code does: width and
/// height words and the pixel pointer copied from `src`, the rest as `SI_malloc` leaves it.
fn icon_record_from_picture(m: &mut Mem, src: u32) -> u32 {
    let rec = m.si_malloc(ICON_SIZE);
    m.set_u32(rec + 8, m.u32(src + 12));
    m.set_i16(rec + 4, m.i16(src));
    m.set_i16(rec + 6, m.i16(src + 2));
    rec
}

/// Copies another icon record's fields into a fresh record (`SI_malloc(12)`).
fn icon_record_copy(m: &mut Mem, src: u32) -> u32 {
    let rec = m.si_malloc(ICON_SIZE);
    m.set_u32(rec + 8, m.u32(src + 8));
    m.set_i16(rec + 4, m.i16(src + 4));
    m.set_i16(rec + 6, m.i16(src + 6));
    rec
}

/// [soft 0x0810ca20 SI_dbListPrepareChapterIcons]
///
/// Makes the three shared icon records (database, directory, none: globals `glbDbIcon`,
/// `glbDirIcon`, `glbNoIcon`) if they do not exist yet, each from the icon file the
/// `dbGetChapIconName` name (-2, -1, -3) finds; then, for every chapter of the dictionary
/// whose icon word (+0x24) is empty, a record from the chapter's icon file, or a copy of
/// the "none" record when there is no file. The picture a lookup returns is freed once
/// copied.
pub fn list_prepare_chapter_icons(m: &mut Mem, o: &mut dyn Outside, dict: u32) {
    for (global, chap) in [(GLB_DB_ICON, -2), (GLB_DIR_ICON, -1), (GLB_NO_ICON, -3)] {
        if m.u32(global.0) == 0 {
            let name = get_chap_icon_name(m, chap);
            let pic = o.db_chapter_icon_get(m, name);
            if pic != 0 {
                let rec = icon_record_from_picture(m, pic);
                m.set_u32(global.0, rec);
            }
        }
    }
    let n = m.i32(dict);
    let mut i = 0;
    while i < n {
        let chapter = m.u32(dict + 4).wrapping_add((i as u32).wrapping_mul(0x40));
        if m.u32(chapter + 0x24) == 0 {
            let name = get_chap_icon_name(m, i);
            let pic = o.db_chapter_icon_get(m, name);
            if pic == 0 {
                let none = m.u32(GLB_NO_ICON.0);
                let rec = icon_record_copy(m, none);
                m.set_u32(chapter + 0x24, rec);
            } else {
                let rec = icon_record_from_picture(m, pic);
                m.set_u32(chapter + 0x24, rec);
                m.free(pic);
            }
        }
        i += 1;
    }
}

/// `%s` of a string pointer that may be null (what the C library prints for null).
fn text(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

/// `strcasecmp(m[a], literal)` is 0.
fn eq_nocase(m: &Mem, a: u32, literal: u32) -> bool {
    si_mem::clib::strcasecmp(m, a, literal) == 0
}

/// `strcmp` of a chapter's file suffix (+0x3c) with the literal "all" (at `literal`).
fn suffix_is_all(m: &Mem, list: u32, literal: u32) -> bool {
    list != 0 && m.u32(list + 0x3c) != 0 && si_mem::clib::strcmp(m, m.u32(list + 0x3c), literal) == 0
}

/// [soft 0x0810c118 dbGetIconData]
///
/// What the icon view needs to place one entry: the icon record of the entry, its pixel
/// pointer (`*out_pix`), icon width and height (`*out_w`, `*out_h`, 16-bit), the text width
/// (`*out_text_w`) and a spacing word (`*out_flag`: 0xf; 0x1e when the chapter's suffix is
/// "all" or the entry has a second name; 0x2d when it has a suffix). `tv` is the view's data
/// record (kind at +0x24; the five fixed icon records of the special entries -1 to -5 at
/// +0x10 to +0x20), `node` the text-list node (the entry's text at +4, its element index at
/// +8, a record for the sequence view at +0xc). Returns the icon record (for an element:
/// the element's own, which may be null although the pixel pointer was set), 0 when an
/// element has none and neither has the chapter. Selects the application font on entry and
/// the IRIS font on exit.
///
/// The original counts the allocations of the shared "directory" copy in a licence
/// counter (`SI_secureLicAlloc`); that counter is not touched here.
#[allow(clippy::too_many_arguments)]
pub fn get_icon_data(
    m: &mut Mem,
    o: &mut dyn Outside,
    list: u32,
    elem: u32,
    tv: u32,
    node: u32,
    out_w: u32,
    out_h: u32,
    out_text_w: u32,
    out_flag: u32,
    out_pix: u32,
) -> u32 {
    use crate::imglobals::*;
    let appf = m.i16(SI_APPF.0) as i32;
    o.font(m, appf);
    let all = 0x08680f08;
    let flag = if list != 0 && m.u32(list + 0x3c) != 0 && si_mem::clib::strcmp(m, m.u32(list + 0x3c), all) == 0 { 0x1e } else { 0xf };
    m.set_i32(out_flag, flag);
    let rec;
    if m.i32(tv + 0x24) == 0x10 {
        rec = m.u32(node + 0xc);
    } else if elem != 0 {
        let mut r = m.u32(elem + 0x70);
        let src;
        if r == 0 || m.u32(r + 8) == 0 {
            let mut chap_icon = m.u32(list + 0x24);
            if chap_icon == 0 {
                if m.i16(list + 0x18) == 2 {
                    chap_icon = m.u32(GLB_NO_ICON.0);
                    let name_rec = m.u32(elem);
                    if m.u16(name_rec + 0x2c) & 0xf000 == 0x4000 {
                        chap_icon = m.u32(GLB_DIR_ICON.0);
                    }
                }
                if chap_icon == 0 {
                    let irisf = m.i16(SI_IRISF.0) as i32;
                    o.font(m, irisf);
                    return 0;
                }
            }
            src = chap_icon;
        } else {
            src = r;
        }
        m.set_u32(out_pix, m.u32(src + 8));
        m.set_i16(out_w, m.i16(src + 4));
        m.set_i16(out_h, m.i16(src + 6));
        r = m.u32(elem + 0x70);
        let nd = m.u32(elem);
        let pic = name_is_picture(m, m.u32(nd + 0x24));
        let work = WORK_STR.0;
        if m.i16(list + 0x18) == 2 && pic == 0 {
            let t = text(m, m.u32(nd + 0x24));
            m.put_cstr(work, &t);
        } else if eq_nocase(m, m.u32(list), 0x08680f0f) || eq_nocase(m, m.u32(list), 0x08680f18) || pic != 0 {
            let t = text(m, m.u32(nd + 0x24));
            m.put_cstr(work, &t);
            let stripped = o.str_strip_digit(m, work);
            let seq = m.local(12);
            format_sequence_frames(m, elem, seq);
            let mut t = text(m, stripped);
            t.extend(m.cstr(seq));
            m.release(12);
            m.put_cstr(work, &t);
        } else {
            let mut t = text(m, m.u32(nd + 0x14));
            t.push(b'.');
            t.extend(text(m, m.u32(nd + 0x18)));
            t.push(b'-');
            t.extend(text(m, m.u32(nd + 0x1c)));
            m.put_cstr(work, &t);
        }
        let second = m.u32(nd + 0x10);
        if second == 0 || m.strlen(second) == 0 {
            let w = o.strwidth(m, work);
            m.set_i32(out_text_w, w);
        } else {
            let w_second = o.strwidth(m, second);
            let w_work = o.strwidth(m, work);
            let chosen = if w_work < w_second { second } else { work };
            let w = o.strwidth(m, chosen);
            m.set_i32(out_text_w, w);
            m.set_i32(out_flag, 0x1e);
        }
        let suffix = m.u32(elem + 0x80);
        let mut done = false;
        if suffix == 0 && !suffix_is_all(m, list, 0x08680f39) {
            done = true;
        }
        if !done {
            if suffix != 0 {
                let w_suffix = o.strwidth(m, suffix);
                let mut cur = m.i32(out_text_w);
                if cur <= w_suffix {
                    cur = o.strwidth(m, suffix);
                }
                m.set_i32(out_text_w, cur);
            }
            m.set_i32(out_flag, 0x2d);
        }
        let irisf = m.i16(SI_IRISF.0) as i32;
        o.font(m, irisf);
        return r;
    } else {
        let id = m.i32(node + 8);
        let l34 = m.u32(list + 0x34);
        rec = if id == -5 && l34 == 0 {
            m.u32(tv + 0x20)
        } else if id == -4 && l34 == 0 {
            m.u32(tv + 0x1c)
        } else if id == -3 && l34 == 0 {
            m.u32(tv + 0x18)
        } else if id == -2 && l34 == 0 {
            m.u32(tv + 0x14)
        } else if id == -1 {
            m.u32(tv + 0x10)
        } else {
            let slot = l34.wrapping_add(((-2 - id) as u32).wrapping_mul(4));
            let mut r = m.u32(slot);
            if r == 0 {
                r = m.si_malloc(ICON_SIZE);
                let dir = m.u32(GLB_DIR_ICON.0);
                m.set_u32(r + 8, m.u32(dir + 8));
                m.set_i16(r + 4, m.i16(dir + 4));
                m.set_i16(r + 6, m.i16(dir + 6));
                let l34 = m.u32(list + 0x34);
                m.set_u32(l34.wrapping_add(((-2 - id) as u32).wrapping_mul(4)), r);
            }
            r
        };
    }
    m.set_u32(out_pix, m.u32(rec + 8));
    m.set_i16(out_w, m.i16(rec + 4));
    m.set_i16(out_h, m.i16(rec + 6));
    let w = o.strwidth(m, m.u32(node + 4));
    m.set_i32(out_text_w, w);
    let irisf = m.i16(SI_IRISF.0) as i32;
    o.font(m, irisf);
    rec
}

/// The element a text-list node stands for: null for a negative index (a special entry),
/// else the element of the chapter's array (`SI_ElemList` +8, 0x84 bytes each).
fn node_elem(m: &Mem, list: u32, node: u32) -> u32 {
    let index = m.i32(node + 8);
    if index < 0 {
        0
    } else {
        m.u32(list + 8).wrapping_add((index as u32).wrapping_mul(0x84))
    }
}

/// [soft 0x0810c568 si_dbIconsOffsetOnSelected]
///
/// Scrolls the icon view (the view data's 16-bit word at +0xc) so that the first selected
/// node that has an icon shows: the offset is the icon's y plus its height and spacing seen
/// from the item's height, never negative. Nothing happens when no selected node has one.
pub fn icons_offset_on_selected(m: &mut Mem, o: &mut dyn Outside, dialog: u32, tl: u32, item: i32, list: u32) {
    let rec = m.u32(dialog + DLG_ITEMS).wrapping_add((item as u32).wrapping_mul(ITEM)).wrapping_sub(ITEM);
    let tv = m.u32(rec + 0x20);
    let mut top = m.i16(rec + 0xc);
    let base = m.i16(dialog + 8);
    let out = m.local(24);
    let (w, h, text_w, flag, pix) = (out, out + 2, out + 4, out + 8, out + 12);
    let count = m.i16(tl) as i32;
    let mut i = 0;
    while i < count {
        let node = m.u32(tl + TL_NODES).wrapping_add(i as u32 * TL_NODE);
        if m.i16(node) != 0 {
            let elem = node_elem(m, list, node);
            let icon = get_icon_data(m, o, list, elem, tv, node, w, h, text_w, flag, pix);
            if icon != 0 && m.u32(pix) != 0 {
                let mut v = (m.i16(icon + 2) as i32).wrapping_sub((top.wrapping_sub(base).wrapping_sub(0x23)) as i32);
                if v < 0 {
                    v = -v;
                }
                top = (v as i16).wrapping_sub(m.i16(h)).wrapping_sub(m.i32(flag) as i16);
                m.set_i16(tv + 0xc, top);
                if top < 0 {
                    top = 0;
                }
                m.set_i16(tv + 0xc, top);
                m.release(24);
                return;
            }
        }
        i += 1;
    }
    m.release(24);
}

/// [soft 0x0810c7e8 SI_dbListHandleIconEvent]
///
/// A click in the icon view: finds the node whose icon (or, in the sequence view, whose
/// text) lies under the pointer, marks it selected (its first word set to 1) and returns 1;
/// 0 when the item has no view data, the view is not the sequence view (kind 0x10) and the
/// dictionary's option word at +0x28 is 0, or nothing is hit. Unless a modifier button is
/// down (`getbutton(6)`, asked twice) all selection words are cleared first.
pub fn list_handle_icon_event(m: &mut Mem, o: &mut dyn Outside, dialog: u32, dict: u32, list: u32, tl: u32, item: i32) -> i32 {
    use crate::imglobals::{SI_COORDX, SI_COORDY};
    let rec = m.u32(dialog + DLG_ITEMS).wrapping_add((item as u32).wrapping_mul(ITEM)).wrapping_sub(ITEM);
    let tv = m.u32(rec + 0x20);
    if tv == 0 || (m.i32(tv + 0x24) != 0x10 && m.i16(dict + 0x28) == 0) {
        return 0;
    }
    let (cx, cy) = (m.u16(SI_COORDX.0) as u32, m.u16(SI_COORDY.0) as u32);
    let px = o.getvaluator(m, cx);
    let py = o.getvaluator(m, cy);
    let dlg_x = m.i16(dialog + 4) as i32;
    let dlg_y = m.i16(dialog + 8) as i32;
    let item_lo = m.i16(rec + 0xa) as i32;
    let item_hi = m.i16(rec + 0xc) as i32;
    let count = m.i16(tl) as i32;
    if o.getbutton(m, 6) == 0 && o.getbutton(m, 6) == 0 {
        for i in 0..count {
            m.set_i16(m.u32(tl + TL_NODES).wrapping_add(i as u32 * TL_NODE), 0);
        }
    }
    let py = py.wrapping_sub(dlg_y);
    let out = m.local(24);
    let (w, h, text_w, flag, pix) = (out, out + 2, out + 4, out + 8, out + 12);
    let mut i = 0;
    while i < count {
        let node = m.u32(tl + TL_NODES).wrapping_add(i as u32 * TL_NODE);
        // in the sequence view the original passes an element variable it never set; the
        // routine does not read it there
        let elem = if m.i32(tv + 0x24) != 0x10 { node_elem(m, list, node) } else { 0 };
        let icon = get_icon_data(m, o, list, elem, tv, node, w, h, text_w, flag, pix);
        if icon != 0 && m.u32(pix) != 0 {
            let icon_w = m.i16(w) as i32;
            let icon_h = m.i16(h) as i32;
            let x0: i16 = if m.i32(tv + 0x24) == 0x10 {
                m.i16(icon)
            } else {
                let half = (m.i32(text_w) / 2) as i16;
                let c = (((icon_w as u16 >> 15) & 1) as i16).wrapping_add(icon_w as i16);
                half.wrapping_add(m.i16(icon)).wrapping_sub(c >> 1)
            };
            let y0 = (m.i16(tv + 0xc).wrapping_add(m.i16(icon + 2))) as i32;
            let y_screen = dlg_y + y0;
            if y_screen <= item_hi
                && item_lo <= y_screen + icon_h
                && (x0 as i32) <= px.wrapping_sub(dlg_x)
                && px.wrapping_sub(dlg_x) <= icon_w + x0 as i32
                && y0 <= py
                && py <= icon_h + y0
            {
                m.set_i16(node, 1);
                m.release(24);
                return 1;
            }
        }
        i += 1;
    }
    m.release(24);
    0
}
