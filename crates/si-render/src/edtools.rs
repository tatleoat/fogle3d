// SPDX-License-Identifier: MIT
//! The dictionary's two picture routines, on the memory image: `SI_elemdictAddPicture` and
//! `SI_elemdictPictureElemRemove` of the original's `edtools.c` (finding 0012, section
//! 19.4). They are the dictionary package's; the renderer's picture list calls them when a
//! texture's picture record is made or loses its last user (`SI_global2dMapInsert`,
//! `SI_global2dMapRemove`), and nobody had them. Their callees stay the dictionary's and
//! are called through [`Outside`] by address.
//!
//! Offsets: a dictionary's chapters at +0x04, 0x40 bytes each (+0x04 the count, +0x08 the
//! elements of 0x84 bytes: +0x00 the name record, +0x04 the object, +0x46 a 16-bit word,
//! +0x48 the state: 0 in use); a name record (`SI_DbListNode`): +0x08 and +0x24 texts.

use crate::img::{f, g, Outside};
use si_mem::{clib, Mem};

/// `SI_addInDictionary` (0x086792b0, 16 bits, 1 in the binary's data): 0 while a scene is
/// being loaded; both routines then do nothing.
pub const SI_ADD_IN_DICTIONARY: u32 = 0x0867_92b0;
/// The text `pic` (0x08680bcc): the suffix of a picture's file name.
pub const SUFFIX_PIC: u32 = crate::data::text::SUFFIX_PIC_FILE;

/// [soft 0x08102ab0 SI_elemdictAddPicture]: `SI_elemdictAddPicture(dictionary, name,
/// record, by file name)`; always answers 1.
///
/// Nothing when `SI_addInDictionary` is 0. Else, in this order:
///
/// 1. `SI_dbfilenameRemovePath(name)`, `SI_dbfilenameAddSuffix(that, "pic")` and
///    `SI_dbScnGetListNode(that, 0, 1)`: a name record for a new element.
/// 2. The PICTURES chapter is searched for an element in use (+0x48 = 0): with the fourth
///    argument 0, one whose name record's text at +0x08 equals `name` **and that has no
///    object**: it takes `record` as its object; with the fourth argument set, one whose
///    name record's text at +0x24 equals the name without its path (nothing is stored).
///    When one is found the new name record is given back (`SI_dbListNodeFree`) and the
///    routine is done: **the suffixed name is not freed** on this path.
/// 3. Else the name record's text at +0x08 is freed when there is one and replaced by a
///    copy of `name` (`strdup`); `SI_elemdictAddElem(dictionary, chapter, name record, 0,
///    record, 0, 0)` gives the new element's number, `SI_elemdictGetElem` the element: its
///    word at +0x46 becomes 1 and its object `record`; the suffixed name is freed.
///
/// Class: copy.
pub fn elemdict_add_picture(m: &mut Mem, env: &mut dyn Outside, dictionary: u32, name: u32, record: u32, by_file: i16) -> u32 {
    if m.i16(SI_ADD_IN_DICTIONARY) == 0 {
        return 1;
    }
    let cid = m.u32(g::CID_PICTURES);
    let chapter = cid.wrapping_shl(6).wrapping_add(m.u32(dictionary + 4));
    let base = env.call(m, f::SI_DBFILENAME_REMOVE_PATH, &[name]);
    let file = env.call(m, f::SI_DBFILENAME_ADD_SUFFIX, &[base, SUFFIX_PIC]);
    let node = env.call(m, f::SI_DB_SCN_GET_LIST_NODE, &[file, 0, 1]);
    let mut i: i32 = 0;
    let mut at: u32 = 0;
    while i < m.i32(chapter + 4) {
        let e = at.wrapping_add(m.u32(chapter + 8));
        if m.i16(e + 0x48) == 0 {
            let found = if by_file == 0 {
                let text = m.u32(m.u32(e).wrapping_add(8));
                if clib::strcmp(m, text, name) == 0 && m.u32(e + 4) == 0 {
                    m.set_u32(e + 4, record);
                    true
                } else {
                    false
                }
            } else {
                let text = m.u32(m.u32(e).wrapping_add(0x24));
                clib::strcmp(m, text, base) == 0
            };
            if found {
                env.call(m, f::SI_DB_LIST_NODE_FREE, &[node]);
                return 1;
            }
        }
        i = i.wrapping_add(1);
        at = at.wrapping_add(0x84);
    }
    let old = m.u32(node + 8);
    if old != 0 {
        m.free(old);
        m.set_u32(node + 8, 0);
    }
    let copy = m.strdup(name);
    m.set_u32(node + 8, copy);
    let number = env.call(m, f::SI_ELEMDICT_ADD_ELEM, &[dictionary, cid, node, 0, record, 0, 0]);
    let e = env.call(m, f::SI_ELEMDICT_GET_ELEM, &[dictionary, cid, number]);
    m.set_u16(e + 0x46, 1);
    m.set_u32(e + 4, record);
    if file != 0 {
        m.free(file);
    }
    1
}

/// [soft 0x08102c34 SI_elemdictPictureElemRemove]: `SI_elemdictPictureElemRemove(
/// dictionary, record, name)`; the name is not read. 0 when the record is null, when no
/// element of the PICTURES chapter has it as its object (`SI_elemdictFindElemByData`
/// answers below 0), or when `SI_elemdictGetElem` answers null. Else
/// `SI_elemdictElementSetDeleteFlag(dictionary, element, chapter, number, 1, null, null,
/// 0)` and the same call once more with a last argument of 1, and 1 is answered.
///
/// What those two calls do is `edtools.c`'s (0x080fd6c8; read, not ported): the first
/// takes an element in use out of use (its relations removed both ways, state 1, the
/// father count cleared), the second turns state 1 into 2 and frees the element's name
/// record and icon and clears its object; neither does anything while `SI_addInDictionary`
/// is 0. Class: copy.
pub fn elemdict_picture_elem_remove(m: &mut Mem, env: &mut dyn Outside, dictionary: u32, record: u32, _name: u32) -> u32 {
    if record == 0 {
        return 0;
    }
    let cid = m.u32(g::CID_PICTURES);
    let number = env.call(m, f::SI_ELEMDICT_FIND_ELEM_BY_DATA, &[dictionary, cid, record]);
    if (number as i32) < 0 {
        return 0;
    }
    let cid = m.u32(g::CID_PICTURES);
    let e = env.call(m, f::SI_ELEMDICT_GET_ELEM, &[dictionary, cid, number]);
    if e == 0 {
        return 0;
    }
    let cid = m.u32(g::CID_PICTURES);
    env.call(m, f::SI_ELEMDICT_ELEMENT_SET_DELETE_FLAG, &[dictionary, e, cid, number, 1, 0, 0, 0]);
    let cid = m.u32(g::CID_PICTURES);
    env.call(m, f::SI_ELEMDICT_ELEMENT_SET_DELETE_FLAG, &[dictionary, e, cid, number, 1, 0, 0, 1]);
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dictionary whose routines note their calls; `SI_elemdictGetElem` answers the
    /// element at that number of the chapter.
    struct Dict {
        calls: Vec<u32>,
        node: u32,
        found: i32,
    }
    impl Outside for Dict {
        fn call(&mut self, m: &mut Mem, addr: u32, a: &[u32]) -> u32 {
            self.calls.push(addr);
            match addr {
                f::SI_DBFILENAME_REMOVE_PATH => a[0] + m.cstr(a[0]).iter().rposition(|c| *c == b'/').map_or(0, |i| i as u32 + 1),
                f::SI_DBFILENAME_ADD_SUFFIX => {
                    let mut t = m.cstr(a[0]);
                    t.push(b'.');
                    t.extend_from_slice(&m.cstr(a[1]));
                    let p = m.malloc(t.len() as u32 + 1);
                    m.put_cstr(p, &t);
                    p
                }
                f::SI_DB_SCN_GET_LIST_NODE => self.node,
                f::SI_ELEMDICT_ADD_ELEM => 1,
                f::SI_ELEMDICT_GET_ELEM => {
                    let chapter = m.u32(a[0] + 4) + a[1] * 0x40;
                    m.u32(chapter + 8) + a[2] * 0x84
                }
                f::SI_ELEMDICT_FIND_ELEM_BY_DATA => self.found as u32,
                _ => 0,
            }
        }
    }

    #[test]
    fn a_picture_enters_and_leaves_the_pictures_chapter() {
        let mut m = Mem::new();
        m.put_cstr(SUFFIX_PIC, b"pic");
        m.set_i16(SI_ADD_IN_DICTIONARY, 1);
        m.set_u32(g::CID_PICTURES, 1);
        let (dict, lists, elems) = (m.calloc(1, 0x70), m.calloc(2, 0x40), m.calloc(2, 0x84));
        m.set_u32(dict + 4, lists);
        m.set_i32(lists + 0x40 + 4, 1);
        m.set_u32(lists + 0x40 + 8, elems);
        // element 0: a picture named `wood` that has lost its record
        let (rec0, wood) = (m.calloc(1, 0x28), m.place_cstr(b"/db/PICTURES/wood"));
        m.set_u32(rec0 + 8, wood);
        m.set_u32(elems, rec0);
        let node = m.calloc(1, 0x28);
        let mut d = Dict { calls: Vec::new(), node, found: 0 };
        // the same name: the element takes the record, nothing is added
        let name = m.place_cstr(b"/db/PICTURES/wood");
        assert_eq!(elemdict_add_picture(&mut m, &mut d, dict, name, 0x5000, 0), 1);
        assert_eq!(m.u32(elems + 4), 0x5000);
        assert_eq!(d.calls, [f::SI_DBFILENAME_REMOVE_PATH, f::SI_DBFILENAME_ADD_SUFFIX, f::SI_DB_SCN_GET_LIST_NODE, f::SI_DB_LIST_NODE_FREE]);
        // (the suffixed name stays allocated on this path)
        assert_eq!(m.live_blocks().len(), 6);
        // once more: the element has its object now, so a new element is made
        d.calls.clear();
        assert_eq!(elemdict_add_picture(&mut m, &mut d, dict, name, 0x6000, 0), 1);
        assert_eq!(&d.calls[3..], [f::SI_ELEMDICT_ADD_ELEM, f::SI_ELEMDICT_GET_ELEM]);
        assert_eq!((m.u16(elems + 0x84 + 0x46), m.u32(elems + 0x84 + 4)), (1, 0x6000));
        assert_eq!(m.cstr(m.u32(node + 8)), b"/db/PICTURES/wood");
        // while a scene is loaded nothing happens at all
        m.set_i16(SI_ADD_IN_DICTIONARY, 0);
        d.calls.clear();
        assert_eq!(elemdict_add_picture(&mut m, &mut d, dict, name, 0x7000, 0), 1);
        assert!(d.calls.is_empty());
        // leaving: the delete flag twice, the second time for good
        assert_eq!(elemdict_picture_elem_remove(&mut m, &mut d, dict, 0x5000, 0), 1);
        assert_eq!(d.calls, [f::SI_ELEMDICT_FIND_ELEM_BY_DATA, f::SI_ELEMDICT_GET_ELEM, f::SI_ELEMDICT_ELEMENT_SET_DELETE_FLAG, f::SI_ELEMDICT_ELEMENT_SET_DELETE_FLAG]);
        d.found = -1;
        assert_eq!(elemdict_picture_elem_remove(&mut m, &mut d, dict, 0x5000, 0), 0);
        assert_eq!(elemdict_picture_elem_remove(&mut m, &mut d, dict, 0, 0), 0);
        assert!(m.fault().is_none());
    }
}
