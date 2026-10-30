// SPDX-License-Identifier: MIT
//! `elemdict.c`, the names of elements: name records, the look-ups by name, renaming, new
//! names, versions.
//!
//! An element's name is a record of another source file (`SI_DbListNode`, 0x34 bytes,
//! `dbexternal.c`); each of its strings is a block of its own, replaced by `free` and
//! `strdup`. The record is made, freed and formatted outside the dictionary files:
//! `SI_dbListNodeAllocate (0x0812250c)`, `SI_dbListNodeFree (0x08122450)`,
//! `SI_dbFormatFullName (0x081223dc)`, `SI_dbnameCheckValidity (0x08121bdc)`,
//! `dbfilenameExpand (0x081228c0)`: those go through [`Outside`].
//!
//! Several routines here drop blocks without freeing them (the validated copies of the
//! two names in [`change_name`], the full name a renamed record had): the image shows
//! that, block for block.

use crate::elemdict::{self, list_of};
use crate::globals::*;
use crate::libc::{decimal, strcmp, strncmp, strtol10};
use crate::outside::{self, Outside};
use crate::rodata::*;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{DbListNode, Elem, ElemDict};

/// `free(*slot)` and the slot cleared, when it is not null: the idiom of the file.
fn free_string(m: &mut Mem, slot: Ptr32<CStr32>) {
    if !m[slot].is_null() {
        let p = m[slot].addr();
        m.free(p);
        m[slot] = Ptr32::NULL;
    }
}

/// [soft 0x081319f0 SI_dbScnGetListNode]: a name record for a file name. The name is
/// split by `dbfilenameExpand (0x081228c0)` into five buffers this routine owns; the
/// record comes from `SI_dbListNodeAllocate (0x0812250c)` and its three ready-made strings
/// are replaced. +8 becomes a copy of the empty string, +0xc a copy of `alias` when that
/// is given and not empty, the time is `time(NULL)`. With `strip_suffix` and a suffix in
/// the name (`.` not followed by `-`) the full name is the name up to that dot, in a block
/// of `calloc(length + 1, 1)`; otherwise a copy of the whole name.
pub fn db_scn_get_list_node(m: &mut Mem, o: &mut dyn Outside, name: CStr32, alias: CStr32, strip_suffix: i16) -> Ptr32<DbListNode> {
    outside::dbfilename_expand(o, m, name.addr(), [PROJ_246.addr(), NAME_247.addr(), VERS_248.addr(), PREC_249.addr(), SUFF_250.addr()]);
    let node: Ptr32<DbListNode> = outside::si_db_list_node_allocate(o, m);
    free_string(m, si_mem::field!(node, DbListNode, str_08));
    free_string(m, si_mem::field!(node, DbListNode, version));
    free_string(m, si_mem::field!(node, DbListNode, subst));
    m[node].str_08 = Ptr32::new(m.strdup(S_EMPTY.addr()));
    m[node].str_0c = Ptr32::NULL;
    if !alias.is_null() && m.strlen(alias.addr()) != 0 {
        m[node].str_0c = Ptr32::new(m.strdup(alias.addr()));
    }
    m[node].prefix = Ptr32::new(m.strdup(PROJ_246.addr()));
    m[node].name = Ptr32::new(m.strdup(NAME_247.addr()));
    m[node].version = Ptr32::new(m.strdup(VERS_248.addr()));
    m[node].subst = Ptr32::new(m.strdup(PREC_249.addr()));
    m[node].time = outside::time(o, m);
    m[node].mode = 0;
    if strip_suffix != 0 {
        let n = elemdict::filename_check_suffix(m, name);
        if n != -1 {
            let p = m.calloc((n + 1) as u32, 1);
            m[node].fullname = Ptr32::new(p);
            // strncpy(p, name, n): the name is longer than n, so n bytes and no padding
            m.copy(p, name.addr(), n as u32);
            m.set_u8(p.wrapping_add(n as u32), 0);
            return node;
        }
    }
    m[node].fullname = Ptr32::new(m.strdup(name.addr()));
    node
}

/// [soft 0x08131ba8 SI_dbListNodeCopyInfo]: copies the strings, time and mode of one name
/// record into another (a fresh one: its three ready-made strings are freed first; the
/// others are overwritten as they are). A source without prefix gets the *name* of the
/// first slot of the SCENES chapter of `ed` as prefix, when there is one. The full name
/// is made anew by `SI_dbFormatFullName (0x081223dc)`.
pub fn db_list_node_copy_info(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, src: Ptr32<DbListNode>, dst: Ptr32<DbListNode>) {
    free_string(m, si_mem::field!(dst, DbListNode, str_08));
    free_string(m, si_mem::field!(dst, DbListNode, version));
    free_string(m, si_mem::field!(dst, DbListNode, subst));
    if !m[src].str_08.is_null() {
        m[dst].str_08 = Ptr32::new(m.strdup(m[src].str_08.addr()));
    }
    if !m[src].str_0c.is_null() {
        m[dst].str_0c = Ptr32::new(m.strdup(m[src].str_0c.addr()));
    }
    let mut prefix = m[src].prefix;
    if prefix.is_null() {
        let mut elems: Ptr32<Elem> = Ptr32::NULL;
        if !ed.is_null() {
            let scenes = list_of(m, ed, m[SI_CID].dsc);
            if !scenes.is_null() && -1 < m[scenes].nb_elems {
                elems = m[scenes].elems;
            }
        }
        if !elems.is_null() && !m[elems].node.is_null() {
            prefix = m[m[elems].node].name;
        }
    }
    if !prefix.is_null() {
        m[dst].prefix = Ptr32::new(m.strdup(prefix.addr()));
    }
    if !m[src].name.is_null() {
        m[dst].name = Ptr32::new(m.strdup(m[src].name.addr()));
    }
    if !m[src].version.is_null() {
        m[dst].version = Ptr32::new(m.strdup(m[src].version.addr()));
    }
    if !m[src].subst.is_null() {
        m[dst].subst = Ptr32::new(m.strdup(m[src].subst.addr()));
    }
    m[dst].time = m[src].time;
    m[dst].mode = m[src].mode;
    m[dst].fullname = outside::si_db_format_full_name(o, m, dst);
}

/// [soft 0x08134678 SI_elemdictFindElemByPrjName]: the first element in use, other than
/// `except`, with this prefix and name; −1 without one. A null `prefix` matches an
/// element without prefix; an empty one also an element whose prefix is null. With
/// `only_new` only elements of the current load (new-flag 1); with `root` not negative
/// only those of that root; with `except` given only those of its substitution number.
/// (`_unused` is the sixth argument, which the routine never reads.)
#[allow(clippy::too_many_arguments)]
pub fn find_elem_by_prj_name(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, except: Ptr32<Elem>, prefix: CStr32, name: CStr32, _unused: CStr32, root: i32, only_new: i16) -> i32 {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let p = m[l].elems.add(i);
        let e = m.read(p);
        if except != p && (only_new == 0 || e.new_flag == 1) && e.status == 0 && !e.node.is_null() && (root < 0 || e.root_id == root) && (except.is_null() || e.subst == m[except].subst) {
            let node = m.read(e.node);
            let prefix_matches = if prefix.is_null() {
                node.prefix.is_null()
            } else if !node.prefix.is_null() {
                strcmp(m, node.prefix.addr(), prefix.addr()) == 0
            } else {
                m.u8(prefix.addr()) == 0
            };
            if prefix_matches && !node.name.is_null() && strcmp(m, node.name.addr(), name.addr()) == 0 {
                return i;
            }
        }
        i += 1;
    }
    -1
}

/// [soft 0x08134770 SI_elemdictFindElemByNameOnly]: the same by name alone.
pub fn find_elem_by_name_only(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, except: Ptr32<Elem>, name: CStr32, root: i32, only_new: i16) -> i32 {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let p = m[l].elems.add(i);
        let e = m.read(p);
        if except != p && (only_new == 0 || e.new_flag == 1) && e.status == 0 && !e.node.is_null() && (root < 0 || e.root_id == root) {
            let n = m[e.node].name;
            if !n.is_null() && strcmp(m, n.addr(), name.addr()) == 0 {
                return i;
            }
        }
        i += 1;
    }
    -1
}

/// [soft 0x08134804 SI_elemdictCheckDuplElemName]: the first other element in use with
/// the prefix and name of `e`, or −1. Two null prefixes are equal; a null and a non-null
/// one are not (when the other's is null the comparison is the pointer of `e`'s own
/// against null).
pub fn check_dupl_elem_name(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>) -> i32 {
    let mine = m[e].node;
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let p = m[l].elems.add(i);
        let other = m[p].node;
        if e != p && m[p].status == 0 && !other.is_null() {
            let same_prefix = if m[other].prefix.is_null() {
                m[mine].prefix.is_null()
            } else if m[mine].prefix.is_null() {
                false
            } else {
                strcmp(m, m[other].prefix.addr(), m[mine].prefix.addr()) == 0
            };
            if same_prefix && strcmp(m, m[other].name.addr(), m[mine].name.addr()) == 0 {
                return i;
            }
        }
        i += 1;
    }
    -1
}

/// [soft 0x081348b4 SI_elemdictFindElemByFullName]: the first element in use, other than
/// `except`, whose full name is `fullname`.
pub fn find_elem_by_full_name(m: &Mem, ed: Ptr32<ElemDict>, chap: i32, except: Ptr32<Elem>, fullname: CStr32, only_new: i16) -> i32 {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let p = m[l].elems.add(i);
        let e = m.read(p);
        if except != p && (only_new == 0 || e.new_flag == 1) && e.status == 0 && !e.node.is_null() && strcmp(m, m[e.node].fullname.addr(), fullname.addr()) == 0 {
            return i;
        }
        i += 1;
    }
    -1
}

/// The end of the three routines that give a name record a new string: the full name is
/// formatted again (the old block is not freed).
fn reformat(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<DbListNode>) {
    m[node].fullname = outside::si_db_format_full_name(o, m, node);
}

/// [soft 0x081361d8 SI_elemdictChangeDscName]: gives the elements of a scene the scene's
/// name as prefix. Every chapter but SCENES and PICTURES; every element in use that is
/// not shared (in EXPRESSIONS only roots). With `fill_missing` an element without prefix
/// gets `name`. With `replace` an element whose prefix is `old` gets `name` instead, and
/// one whose prefix already is `name` is left alone. A renamed element is marked
/// new-flag 2. (`_index` is not read.)
pub fn change_dsc_name(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, _index: i32, name: CStr32, fill_missing: i16, replace: i16, old: CStr32) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        if c != m[SI_CID].dsc && c != m[SI_CID].img {
            let l = list_of(m, ed, c);
            let mut i = 0;
            while i < m[l].nb_elems {
                let p = m[l].elems.add(i);
                i += 1;
                if !(m[p].status == 0 && m[p].shared == 0 && (c != m[SI_CID].exp || m[p].hier_level == 0)) {
                    continue;
                }
                let node = m[p].node;
                let prefix = m[node].prefix;
                if replace != 0 && !name.is_null() && !prefix.is_null() && strcmp(m, prefix.addr(), name.addr()) == 0 {
                    continue;
                }
                let set = if fill_missing == 0 || !prefix.is_null() {
                    if replace != 0 && !prefix.is_null() && !old.is_null() && strcmp(m, prefix.addr(), old.addr()) == 0 {
                        free_string(m, si_mem::field!(node, DbListNode, prefix));
                        true
                    } else {
                        false
                    }
                } else {
                    true
                };
                if set {
                    m[node].prefix = Ptr32::new(m.strdup(name.addr()));
                    reformat(m, o, node);
                    m[p].new_flag = 2;
                }
            }
        }
        c += 1;
    }
}

/// [soft 0x08136350 SI_elemdictPropagatePrfxAndShare]: gives every element in use of a
/// chapter whose root is `root` the prefix `prefix` (null clears it) and the shared mark
/// `shared`. An element whose prefix already is `prefix` keeps its record as it is.
pub fn propagate_prfx_and_share(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, root: i32, prefix: CStr32, shared: i16) {
    let l = list_of(m, ed, chap);
    let mut i = 0;
    while i < m[l].nb_elems {
        let p = m[l].elems.add(i);
        if m[p].status == 0 && m[p].root_id == root {
            let node = m[p].node;
            if !node.is_null() {
                let mut keep = false;
                if !m[node].prefix.is_null() {
                    if !prefix.is_null() && strcmp(m, m[node].prefix.addr(), prefix.addr()) == 0 {
                        keep = true;
                    } else {
                        free_string(m, si_mem::field!(node, DbListNode, prefix));
                    }
                }
                if !keep {
                    m[node].prefix = if prefix.is_null() { Ptr32::NULL } else { Ptr32::new(m.strdup(prefix.addr())) };
                    reformat(m, o, node);
                }
            }
            m[p].shared = shared;
        }
        i += 1;
    }
}

/// [soft 0x08136450 SI_elemdictChangeName]: renames an element. Both names are copied and
/// validated (`SI_dbnameCheckValidity (0x08121bdc)`) first. Refused (0) when another
/// element of the chapter has the new prefix and name. Nothing changes when the element
/// already has them. Otherwise the record gets copies of the name and of the prefix (the
/// empty string for a null prefix), a new full name and new-flag 2; a scene then hands its
/// name on as prefix ([`change_dsc_name`]) and a model its prefix and shared mark to its
/// hierarchy ([`propagate_prfx_and_share`]).
///
/// The two validated copies are freed only on the paths that change nothing. A null
/// `name` reaches `strcmp` with a null pointer, as in the original.
pub fn change_name(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, e: Ptr32<Elem>, prefix: CStr32, name: CStr32) -> i32 {
    if e.is_null() {
        return 1;
    }
    let node = m[e].node;
    let mut p = 0;
    if !prefix.is_null() {
        let copy = m.strdup(prefix.addr());
        p = outside::si_dbname_check_validity(o, m, copy, 1);
    }
    let mut n = 0;
    if !name.is_null() {
        let copy = m.strdup(name.addr());
        n = outside::si_dbname_check_validity(o, m, copy, 0);
    }
    if find_elem_by_prj_name(m, ed, chap, e, Ptr32::new(p), Ptr32::new(n), Ptr32::NULL, -1, 0) != -1 {
        if p != 0 {
            m.free(p);
        }
        if n != 0 {
            m.free(n);
        }
        return 0;
    }
    if p == 0 {
        if m[node].prefix.is_null() && !m[node].name.is_null() && strcmp(m, m[node].name.addr(), n) == 0 {
            if n != 0 {
                m.free(n);
            }
            return 1;
        }
    } else if !m[node].prefix.is_null() && strcmp(m, m[node].prefix.addr(), p) == 0 && !m[node].name.is_null() && strcmp(m, m[node].name.addr(), n) == 0 {
        m.free(p);
        if n != 0 {
            m.free(n);
        }
        return 1;
    }
    if n != 0 {
        free_string(m, si_mem::field!(node, DbListNode, name));
        m[node].name = Ptr32::new(m.strdup(n));
    }
    free_string(m, si_mem::field!(node, DbListNode, prefix));
    let from = if p == 0 { S_EMPTY_PREFIX.addr() } else { p };
    m[node].prefix = Ptr32::new(m.strdup(from));
    reformat(m, o, node);
    m[e].new_flag = 2;
    if chap == m[SI_CID].dsc {
        let data = m[e].data;
        let index = elemdict::find_elem_by_data(m, o, ed, chap, data);
        let name = m[node].name;
        change_dsc_name(m, o, ed, index, name, 1, 0, Ptr32::NULL);
    } else if chap == m[SI_CID].model {
        let data = m[e].data;
        let index = elemdict::find_elem_by_data(m, o, ed, chap, data);
        let shared = m[e].shared;
        propagate_prfx_and_share(m, o, ed, chap, index, Ptr32::new(p), shared);
    }
    1
}

/// [soft 0x081366a0 SI_elemdictGetNewNameId]: a name nobody in the chapter has: the stem
/// of `base` (`base` without the digits at its end; `obj` for a null `base`) followed by
/// a number, in a buffer the routine owns. The number is one more than the highest number
/// that follows the stem in a name (with `use_prefix`: in a prefix) of the chapter's
/// slots that are not deleted, and goes to `*id`. The text behind the stem is read as
/// `strtol` reads it, so `cube` also counts `cube_7x` as 0 and `cube12` as 12.
///
/// With `SI_XSI_LEGACYLOADER` in the environment the chapter is not looked at: the
/// routine's own counter goes on from its last value and the name is `stem_number`.
/// The code that the decompiler shows as a separate fragment at 0x081368d0 is the tail of
/// this routine's loop.
pub fn get_new_name_id(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, base: CStr32, id: Ptr32<i32>, use_prefix: i16) -> CStr32 {
    let l = list_of(m, ed, chap);
    let name = m.cstr(S_LEGACY.addr());
    let legacy = o.getenv(m, &name) != 0;
    if !legacy {
        m[NUM_510] = 0;
    }
    let mut stem = S_OBJ.addr();
    let mut n: u32 = 3;
    if !base.is_null() {
        n = m.strlen(base.addr());
        while n > 0 && m.u8(base.addr() + n - 1).is_ascii_digit() {
            n -= 1;
        }
        stem = base.addr();
    }
    if !legacy {
        let mut i = 0;
        while i < m[l].nb_elems {
            let e = m.read(m[l].elems.add(i));
            i += 1;
            if e.status == 2 || e.node.is_null() || m[e.node].name.is_null() {
                continue;
            }
            let s = if use_prefix != 0 { m[e.node].prefix } else { m[e.node].name };
            if s.is_null() {
                continue;
            }
            if strncmp(m, stem, s.addr(), n) == 0 && n < m.strlen(s.addr()) {
                let v = strtol10(m, s.addr() + n);
                if m[NUM_510] < v {
                    m[NUM_510] = v;
                }
            }
        }
    }
    m[NUM_510] += 1;
    // the stem goes through a local buffer of 500 bytes (strncpy, then the terminator)
    let mut text: Vec<u8> = m.bytes(stem, n).to_vec();
    if let Some(end) = text.iter().position(|&b| b == 0) {
        text.truncate(end);
    }
    if legacy {
        text.push(b'_');
    }
    text.extend(decimal(m[NUM_510]));
    m.put_cstr(NEWNAME_509.addr(), &text);
    m[id] = m[NUM_510];
    NEWNAME_509.cast()
}

/// [soft 0x0813697c SI_elemdictGetNewName]: a new name from `base`.
pub fn get_new_name(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, base: CStr32) -> CStr32 {
    let id: Ptr32<i32> = Ptr32::new(m.local(4));
    let name = get_new_name_id(m, o, ed, chap, base, id, 0);
    m.release(4);
    name
}

/// [soft 0x081369a4 SI_elemdictGetNewPrefix]: a new prefix from `base`.
pub fn get_new_prefix(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, chap: i32, base: CStr32) -> CStr32 {
    let id: Ptr32<i32> = Ptr32::new(m.local(4));
    let name = get_new_name_id(m, o, ed, chap, base, id, 1);
    m.release(4);
    name
}

/// [soft 0x081369cc SI_elemdictCreateDefaultDsc]: when the SCENES chapter has no slot, a
/// scene with an empty prefix, the name `no_name` and description number −1 is added;
/// the answer is slot 0 of the chapter otherwise, the new slot then.
pub fn create_default_dsc(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) -> Ptr32<Elem> {
    let dsc = m[SI_CID].dsc;
    let index = if m[list_of(m, ed, dsc)].nb_elems < 1 {
        let node: Ptr32<DbListNode> = outside::si_db_list_node_allocate(o, m);
        m[node].prefix = Ptr32::new(m.strdup(S_EMPTY_DSC_PREFIX.addr()));
        m[node].name = Ptr32::new(m.strdup(S_NO_NAME.addr()));
        reformat(m, o, node);
        let dsc = m[SI_CID].dsc;
        elemdict::add_elem(m, o, ed, dsc, node, Ptr32::NULL, Ptr32::NULL, 0, -1)
    } else {
        0
    };
    let dsc = m[SI_CID].dsc;
    elemdict::get_elem(m, ed, dsc, index)
}

/// What [`set_version`] and [`set_substitution`] do to one name record: the string at
/// `field` and the full name are freed, the string becomes a copy of `text`, the full
/// name is formatted anew.
fn restamp(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<DbListNode>, field: Ptr32<CStr32>, text: &[u8]) {
    free_string(m, field);
    free_string(m, si_mem::field!(node, DbListNode, fullname));
    let p = m.malloc(text.len() as u32 + 1);
    m.put_cstr(p, text);
    m[field] = Ptr32::new(p);
    reformat(m, o, node);
}

/// [soft 0x08136a60 elemdictSetVersion]: gives every element in use that has a name
/// record the version `version` (printed with `%d`).
pub fn set_version(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, version: i32) {
    let text = decimal(version);
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let p = m[l].elems.add(i);
            if m[p].status == 0 && !m[p].node.is_null() {
                let node = m[p].node;
                restamp(m, o, node, si_mem::field!(node, DbListNode, version), &text);
            }
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08136b50 SI_elemdictSetSubstitution]: the same for the substitution number,
/// which is also stored in the element (+0x4a).
pub fn set_substitution(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>, subst: i32) {
    let text = decimal(subst);
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let p = m[l].elems.add(i);
            if m[p].status == 0 && !m[p].node.is_null() {
                m[p].subst = subst as i16;
                let node = m[p].node;
                restamp(m, o, node, si_mem::field!(node, DbListNode, subst), &text);
            }
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08137884 si_elemdictResetOriginDB]: every slot that is not deleted and whose
/// name record points at the database path `path` (compared as text) forgets it and is
/// marked new-flag 2.
pub fn reset_origin_db(m: &mut Mem, ed: Ptr32<ElemDict>, path: CStr32) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let p = m[l].elems.add(i);
            let node = m[p].node;
            if m[p].status != 2 && !node.is_null() {
                let origin = m.u32(node.addr());
                if origin != 0 && strcmp(m, path.addr(), origin) == 0 {
                    m.set_u32(node.addr(), 0);
                    m[p].new_flag = 2;
                }
            }
            i += 1;
        }
        c += 1;
    }
}

/// [soft 0x08137918 si_elemdictSetOriginDB]: every slot that is not deleted and has a
/// name record is pointed at the database path `path` (the pointer, not a copy).
pub fn set_origin_db(m: &mut Mem, ed: Ptr32<ElemDict>, path: CStr32) {
    let mut c = 0;
    while c < m[ed].nb_lists {
        let l = list_of(m, ed, c);
        let mut i = 0;
        while i < m[l].nb_elems {
            let p = m[l].elems.add(i);
            let node = m[p].node;
            if m[p].status != 2 && !node.is_null() {
                m.set_u32(node.addr(), path.addr());
            }
            i += 1;
        }
        c += 1;
    }
}
