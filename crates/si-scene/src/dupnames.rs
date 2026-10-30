// SPDX-License-Identifier: MIT
//! Duplicate names (`scn2db.c`), on the memory image: after a scene is loaded and before
//! one is saved, two elements of a chapter with one prefix and name are told apart by
//! renaming the later one `<name>_<n>`.
//!
//! The chapter sort is `si-dict`'s (`SI_elemlistSortChapterElements`); the name records
//! are rebuilt by [`crate::dbnode`]. The two routines of `edtools.c` and `edanim.c` that
//! rename the element's object and its curves are reached through the dictionaries'
//! outside world ([`SI_ED_SOFT_ELEM_CHANGE_NAME`], [`SI_ED_ANIM_ELEM_CHANGE_NAME`]).

use crate::dbnode;
use si_dict::elemdict::compare_elem_name;
use si_dict::elemsort;
use si_dict::globals::SI_CID;
use si_dict::outside::{Outside, Routine};
use si_mem::clib::{strcmp, strncmp};
use si_mem::{CStr32, Mem, Ptr32};
use si_types::dict::{DbListNode, Elem, ElemDict, ElemList};

/// `SI_edSoftElemChangeName (0x080fe96c)`, `edtools.c`: renames the object of an element
/// (`element`, `chapter`, `name`). Its port is [`crate::dictout::rename_object`], on the
/// objects, which are not in the image.
pub const SI_ED_SOFT_ELEM_CHANGE_NAME: Routine = Routine { name: "SI_edSoftElemChangeName", addr: 0x080fe96c };
/// `si_edAnimElemChangeName (0x080eeae4)`, `edanim.c`: renames what animates an element
/// (`element`, `chapter`, `old name`, null, `new name`). Not ported.
pub const SI_ED_ANIM_ELEM_CHANGE_NAME: Routine = Routine { name: "si_edAnimElemChangeName", addr: 0x080eeae4 };

si_mem::globals! {
    /// The buffer the duplicate-name check prints a new name into.
    GLB_WORK_NAME: [u8; 1000] = 0x087851a0, "glbWorkName";
}

/// [soft 0x081096f4 si_dbCheckChapterDuplicateNames]: renames the duplicates of one
/// chapter. (The count of renamed elements answered here is for the checks: the original
/// answers nothing.)
///
/// The chapter is sorted by full name, ascending. Elements that are freed slots (status
/// 2) or shared, and in ANIMATION everything but the roots, are passed over. Going
/// through the rest in sorted order, an element whose prefix and name
/// (`SI_dbFormatPrefixName`) equal those of the last element that was not a duplicate is
/// renamed `<name>_<n>`:
///
/// - `n` counts the duplicates of that name met so far, plus the elements that follow in
///   sorted order whose prefix and name start with this one's followed by an underscore
///   (the count stops at the first that does not);
/// - if an element further on already has the new name (`SI_compareElemName` on the
///   names alone), `n` goes up by one and the name is printed again; the search stops at
///   the first name that sorts after the candidate.
///
/// The new name is a `strdup` of the work buffer; the full name is rebuilt
/// (`SI_dbFormatFullName`, which frees the old one); for an element in use its object and
/// its curves are renamed; unless the slot is freed its new-flag becomes 2. The old name
/// is freed, except for an element in use of the NSELECTIONS chapter.
///
/// The loop that looks for the first element to compare with re-reads the entry it
/// started from before it moves on, so when the first sorted entries are passed over its
/// counter ends one too high: **the entry that follows the first element that counts is
/// never compared with anything**. That is kept. A name record that is null is read
/// through, as the original reads it.
pub fn check_chapter_duplicate_names(m: &mut Mem, o: &mut dyn Outside, list: Ptr32<ElemList>, chap: i32) -> usize {
    elemsort::sort_chapter_elements(m, o, list, chap, elemsort::key::FULL_NAME, 1, 0);
    let ani = m[SI_CID].ani;
    let counts = |m: &Mem, e: Ptr32<Elem>| !((chap == ani && m[e].hier_level != 0) || m[e].status == 2 || m[e].shared != 0);
    let entry = |m: &Mem, k: i32| m[list].elems.add(m[m[list].sort.add(k)]);
    let mut k = 0;
    let mut first = m[m[list].sort];
    while !counts(m, m[list].elems.add(first)) && k < m[list].nb_elems {
        first = m[m[list].sort.add(k)];
        k += 1;
    }
    if k == m[list].nb_elems {
        return 0;
    }
    // prefix and name of the last element that was no duplicate, of this element, of one
    // further on: three buffers of 0x200 bytes in the routine's frame
    let (buf_other, buf_own, buf_ref): (CStr32, CStr32, CStr32) = (Ptr32::new(m.local(0x200)), Ptr32::new(m.local(0x200)), Ptr32::new(m.local(0x200)));
    let work: CStr32 = GLB_WORK_NAME.cast();
    let mut reference: Ptr32<DbListNode> = m[m[list].elems.add(first)].node;
    dbnode::format_prefix_name(m, buf_ref, m[reference].prefix, m[reference].name);
    let mut dup: i32 = 0;
    let mut renamed = 0;
    k += 1;
    while k < m[list].nb_elems {
        let e = entry(m, k);
        let next = k + 1;
        k = next;
        if !counts(m, e) {
            continue;
        }
        let node = m[e].node;
        dbnode::format_prefix_name(m, buf_own, m[node].prefix, m[node].name);
        let same = !reference.is_null() && !node.is_null() && strcmp(m, buf_ref.addr(), buf_own.addr()) == 0;
        if !same {
            dup = 0;
            reference = node;
            dbnode::format_prefix_name(m, buf_ref, m[node].prefix, m[node].name);
            continue;
        }
        dup += 1;
        // how much of "prefix-name" the names that follow must share
        let mut len: u32 = 0;
        let mut nb = m[list].nb_elems;
        if !node.is_null() && next < nb {
            let prefix = m[node].prefix;
            if !prefix.is_null() && m.strlen(prefix.addr()) != 0 {
                len = m.strlen(prefix.addr()) + 1;
            }
            let name = m[node].name;
            if !name.is_null() {
                len += m.strlen(name.addr());
            }
        }
        let mut j = next;
        while j < nb {
            let other = entry(m, j);
            if counts(m, other) {
                let on = m[other].node;
                dbnode::format_prefix_name(m, buf_other, m[on].prefix, m[on].name);
                if strncmp(m, buf_own.addr(), buf_other.addr(), len) != 0 || m.u8(buf_other.addr().wrapping_add(len)) != b'_' {
                    break;
                }
                dup += 1;
                nb = m[list].nb_elems;
            }
            j += 1;
        }
        // the new name: `sprintf(glbWorkName, "%s_%d", name, n)` until no element further
        // on has it
        loop {
            let mut text = match dbnode::text(m, m[node].name) {
                Some(t) => t,
                None => b"(null)".to_vec(),
            };
            text.extend_from_slice(format!("_{dup}").as_bytes());
            m.put_cstr(work.addr(), &text);
            let mut taken = false;
            let mut j = next;
            while j < m[list].nb_elems {
                let other = entry(m, j);
                if counts(m, other) {
                    let c = compare_elem_name(m, work, m[m[other].node].name);
                    if c == 0 {
                        taken = true;
                        break;
                    }
                    if c < 0 {
                        break;
                    }
                }
                j += 1;
            }
            if !taken {
                break;
            }
            dup += 1;
        }
        let old = m[node].name;
        m[node].name = Ptr32::new(m.strdup(work.addr()));
        m[node].fullname = dbnode::format_full_name(m, node);
        if m[e].status == 0 {
            let new = m[node].name;
            o.call(m, &SI_ED_SOFT_ELEM_CHANGE_NAME, &[e.addr(), chap as u32, new.addr()]);
            let new = m[node].name;
            o.call(m, &SI_ED_ANIM_ELEM_CHANGE_NAME, &[e.addr(), chap as u32, old.addr(), 0, new.addr()]);
        }
        if m[e].status != 2 {
            m[e].new_flag = 2;
        }
        if (m[e].status != 0 || chap != m[SI_CID].nsl) && !old.is_null() {
            m.free(old.addr());
        }
        renamed += 1;
    }
    m.release(0x600);
    renamed
}

/// [soft 0x08109be4 SI_dbCheckDuplicateNames]: [`check_chapter_duplicate_names`] for every
/// chapter with more than one slot, in dictionary order but with ANIMATION, CLUSTERS and
/// SHAPES last, in that order; never PICTURES. The three last are taken by their index in
/// `SI_cid` without a look at the number of chapters. (The count answered is for the
/// checks.)
pub fn db_check_duplicate_names(m: &mut Mem, o: &mut dyn Outside, ed: Ptr32<ElemDict>) -> usize {
    let mut renamed = 0;
    let mut chap = 0;
    while chap < m[ed].nb_lists {
        let cid = &m[SI_CID];
        if chap != cid.ani && chap != cid.img && chap != cid.cls && chap != cid.shp {
            let list = m[ed].lists.add(chap);
            if 1 < m[list].nb_elems {
                renamed += check_chapter_duplicate_names(m, o, list, chap);
            }
        }
        chap += 1;
    }
    for which in 0..3 {
        let cid = &m[SI_CID];
        let chap = [cid.ani, cid.cls, cid.shp][which];
        let list = m[ed].lists.add(chap);
        if 1 < m[list].nb_elems {
            renamed += check_chapter_duplicate_names(m, o, list, chap);
        }
    }
    renamed
}
