// SPDX-License-Identifier: MIT
//! `SI_edElementChangeName` (`edtools.c`, 0x080febf4): the rename of any element of the
//! scene dictionary, with the messages it asks the request dialog for. Written beside
//! [`crate::edfirst`] (the same outside world, [`ByAddr`]); checked by `scene-emucheck` on
//! the vectors of `re/tools/chgname1/emu_gen_edname.py` (finding 0018, section "The
//! rename of an element").
//!
//! The dialog (`SI_dialogRequest`) is a hook: called by address, its answer is not read.
//! The texts it is given are the binary's literals ([`store`] puts them on the image at
//! their addresses) and two made on the stack with `sprintf`.

use crate::edtools::*;
use crate::world::current_dict;
use si_dict::globals::SI_CID;
use si_mem::Mem;

/// `SI_dialogRequest (0x08141c00)`, `request.c`: four message lines (pointers).
pub const SI_DIALOG_REQUEST: u32 = 0x0814_1c00;
/// `SI_strAppend (0x083cf280)`: (&string, text).
pub const SI_STR_APPEND: u32 = 0x083c_f280;
/// `SI_elemdictGetChapName (0x08132b04)`: (dictionary, chapter).
pub const SI_ELEMDICT_GET_CHAP_NAME: u32 = 0x0813_2b04;
/// `SI_elemdictChangeName (0x08136450)`: (dictionary, chapter, element, prefix, name).
pub const SI_ELEMDICT_CHANGE_NAME_ADDR: u32 = 0x0813_6450;
/// `si_edAnimElemChangeName (0x080eeae4)`: (element, chapter, old name, prefix, new name).
pub const SI_ED_ANIM_ELEM_CHANGE_NAME: u32 = 0x080e_eae4;
/// `SI_edSoftElemChangeName (0x080fe96c)`: (element, chapter, name).
pub const SI_ED_SOFT_ELEM_CHANGE_NAME_ADDR: u32 = 0x080f_e96c;
/// `SI_dopeItemChangeName (0x081fa540)`: (element, chapter).
pub const SI_DOPE_ITEM_CHANGE_NAME: u32 = 0x081f_a540;

/// The binary's literals the routine passes or prints, read from the binary with
/// `re/tools/elfread.py` (address, text).
pub const STRINGS: &[(u32, &[u8])] = &[
    (0x0868_0aa0, b"This element is flagged as SHARED... "),
    (0x0868_0a60, b"You cannot rename it unless you explicitely make"),
    (0x0868_0a20, b"it distinct. Use Info:Elements (Set Share)."),
    (0x0868_0a00, b"Ok"),
    (0x0868_0adb, b"An empty name is invalid."),
    (0x0868_0aca, b"Change not made."),
    (0x0868_0ac9, b""),
    (0x0868_0ac6, b"Ok"),
    (0x0868_0af5, b"obj"),
    (0x0868_0af9, b"-"),
    (0x0868_0afb, b"In <%s>: The requested name"),
    (0x0868_0b17, b"\"%s\" is already used by"),
    (0x0868_0b40, b"another element. Please use a different name."),
    (0x0868_0b6e, b"Ok"),
];

const S_SHARED: [u32; 4] = [0x0868_0aa0, 0x0868_0a60, 0x0868_0a20, 0x0868_0a00];
const S_EMPTY: [u32; 4] = [0x0868_0adb, 0x0868_0aca, 0x0868_0ac9, 0x0868_0ac6];
const S_OBJ: u32 = 0x0868_0af5;
const S_DASH: u32 = 0x0868_0af9;
const S_FMT_CHAPTER: u32 = 0x0868_0afb;
const S_FMT_USED: u32 = 0x0868_0b17;
const S_DIFFERENT: u32 = 0x0868_0b40;
const S_OK: u32 = 0x0868_0b6e;

/// Puts the literals on the image at their addresses.
pub fn store(m: &mut Mem) {
    for (a, s) in STRINGS {
        let mut v = s.to_vec();
        v.push(0);
        m.set_bytes(*a, &v);
    }
}

fn strlen(m: &Mem, p: u32) -> usize {
    m.cstr(p).len()
}

/// `SI_edElementChangeName(element, chapter, prefix, &name)`. Asks for the element to take
/// the prefix `prefix` (null or empty: none) and the name `*name_at`; answers 1 when the
/// dictionary did the rename (or nothing had to change) and 0 when it did not, in which case
/// `*name_at` is replaced (a fresh `strdup`, the old text freed) by what the user is told to
/// have been used instead: the element's own name for a SHARED element, else a free name
/// (`SI_elemdictGetNewName`).
///
/// [soft 0x080febf4 SI_edElementChangeName]. Where the original would fault (`name_at` null,
/// or `*name_at` null on the way to the empty-name test) this reads address 0 like the
/// image does.
pub fn ed_element_change_name<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, elem: u32, chapter: i32, prefix_in: u32, name_at: u32) -> u32 {
    let sp = m.sp();
    // the original's frame: three 512-byte buffers one above the other, the two pointers
    let base = m.local(0x600);
    let (buf_a, buf_b, buf_c) = (base, base.wrapping_add(0x200), base.wrapping_add(0x400)); // local_604, local_404, local_204
    let joined = m.local(4); // local_608
    m.set_u32(joined, 0);
    let mut requested: u32 = 0; // local_610
    // an empty prefix counts as none in the comparison below
    let prefix = if prefix_in != 0 && strlen(m, prefix_in) == 0 { 0 } else { prefix_in };
    let rec = m.u32(elem);
    let (cur_prefix, cur_name) = (m.u32(rec.wrapping_add(0x10)), m.u32(rec.wrapping_add(0x14)));
    let same_prefix = if prefix == 0 {
        cur_prefix == 0
    } else {
        cur_prefix != 0 && m.cstr(prefix) == m.cstr(cur_prefix)
    };
    if same_prefix {
        let name = m.u32(name_at);
        if name != 0 && cur_name != 0 && m.cstr(name) == m.cstr(cur_name) {
            m.set_sp(sp);
            return 1;
        }
    }
    let d = current_dict(m).0;
    if m.i16(elem.wrapping_add(0x44)) != 0 {
        // a SHARED element keeps its name
        o.call_addr(m, SI_DIALOG_REQUEST, &S_SHARED);
        let name = m.u32(name_at);
        if name != 0 {
            m.free(name);
            m.set_u32(name_at, 0);
        }
        let cur = m.u32(m.u32(elem).wrapping_add(0x14));
        let copy = m.strdup(cur);
        m.set_u32(name_at, copy);
        m.set_sp(sp);
        return 0;
    }
    let old = m.strdup(m.u32(m.u32(elem).wrapping_add(0x14)));
    if prefix_in != 0 {
        requested = m.strdup(prefix_in);
    }
    let empty = name_at != 0 && strlen(m, m.u32(name_at)) == 0;
    let ask;
    if empty {
        o.call_addr(m, SI_DIALOG_REQUEST, &S_EMPTY);
        ask = S_OBJ;
    } else {
        let name = m.u32(name_at);
        let r = o.call_addr(m, SI_ELEMDICT_CHANGE_NAME_ADDR, &[d, chapter as u32, elem, prefix_in, name]) as u16;
        if r != 0 {
            o.call_addr(m, SI_ED_ANIM_ELEM_CHANGE_NAME, &[elem, chapter as u32, old, requested, m.u32(name_at)]);
            if chapter == m[SI_CID].nsl {
                let data = m.u32(elem.wrapping_add(4));
                if data != 0 {
                    let n = m.u32(m.u32(elem).wrapping_add(0x14));
                    m.set_u32(data.wrapping_add(4), n);
                }
            } else {
                o.call_addr(m, SI_ED_SOFT_ELEM_CHANGE_NAME_ADDR, &[elem, chapter as u32, m.u32(name_at)]);
            }
            o.call_addr(m, SI_DOPE_ITEM_CHANGE_NAME, &[elem, chapter as u32]);
            if old != 0 {
                m.free(old);
            }
            if requested != 0 {
                m.free(requested);
            }
            m.set_sp(sp);
            return 1;
        }
        // refused: "In <chapter>: The requested name "prefix-name" is already used by another ..."
        o.call_addr(m, SI_STR_APPEND, &[joined, prefix_in]);
        if prefix_in != 0 && strlen(m, prefix_in) != 0 {
            o.call_addr(m, SI_STR_APPEND, &[joined, S_DASH]);
        }
        o.call_addr(m, SI_STR_APPEND, &[joined, m.u32(name_at)]);
        let chap_name = o.call_addr(m, SI_ELEMDICT_GET_CHAP_NAME, &[d, chapter as u32]);
        let chap_text = if chap_name == 0 { b"(null)".to_vec() } else { m.cstr(chap_name) };
        let line_a = sprintf_s(m, S_FMT_CHAPTER, &chap_text);
        m.put_cstr(buf_c, &line_a);
        let joined_p = m.u32(joined);
        let joined_text = if joined_p == 0 { b"(null)".to_vec() } else { m.cstr(joined_p) };
        let line_b = sprintf_s(m, S_FMT_USED, &joined_text);
        m.put_cstr(buf_b, &line_b);
        let tail = m.cstr(S_DIFFERENT);
        m.put_cstr(buf_a, &tail);
        if joined_p != 0 {
            m.free(joined_p);
            m.set_u32(joined, 0);
        }
        o.call_addr(m, SI_DIALOG_REQUEST, &[buf_c, buf_b, buf_a, S_OK]);
        ask = m.u32(name_at);
    }
    let fresh = o.call_addr(m, SI_ELEMDICT_GET_NEW_NAME, &[d, chapter as u32, ask]);
    let name = m.u32(name_at);
    if name != 0 {
        m.free(name);
        m.set_u32(name_at, 0);
    }
    if old != 0 {
        m.free(old);
    }
    if requested != 0 {
        m.free(requested);
    }
    let copy = m.strdup(fresh);
    m.set_u32(name_at, copy);
    m.set_sp(sp);
    0
}

/// `sprintf(buffer, format, text)` with the one `%s` the formats here have: the text of the
/// format with the first `%s` replaced.
fn sprintf_s(m: &Mem, format: u32, text: &[u8]) -> Vec<u8> {
    let f = m.cstr(format);
    match f.windows(2).position(|w| w == b"%s") {
        Some(i) => {
            let mut v = f[..i].to_vec();
            v.extend_from_slice(text);
            v.extend_from_slice(&f[i + 2..]);
            v
        }
        None => f,
    }
}
