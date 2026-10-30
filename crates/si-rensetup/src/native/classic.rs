// SPDX-License-Identifier: MIT
//! `classic.c`: the three routines of the "classic" manipulation mode that the copy of a
//! node calls: the compensation records' allocator, their disposal and their copy. (The
//! file's other routines are the mode's manipulators.)

use super::rec::{CompInfo, CompInfoList};
use si_math::algebra::matrix_copy;
use si_mem::{Mem, Ptr32};
use si_types::model::Model;

type Node = Ptr32<Model>;

/// A new, empty list of compensation records [soft 0x081cb03c compensateInfoListAllocate]:
/// 8 bytes from `SI_malloc`, cleared; null when there is no memory.
pub fn compensate_info_list_allocate(m: &mut Mem) -> Ptr32<CompInfoList> {
    let l: Ptr32<CompInfoList> = Ptr32::new(m.si_malloc(8));
    if !l.is_null() {
        m[l].count = 0;
        m[l].records = Ptr32::<CompInfo>::NULL;
    }
    l
}

/// Disposes of a node's compensation records [soft 0x081cc0f0 SI_classicMdlCompInfoDispose]:
/// `list` is the address of the node's pointer. Nothing for a null pointer; else the
/// records, when there are any, are freed and their pointer cleared, the list is freed and
/// the node's pointer cleared.
pub fn classic_mdl_comp_info_dispose(m: &mut Mem, list: Ptr32<Ptr32<CompInfoList>>) {
    let l = m[list];
    if l.is_null() {
        return;
    }
    let records = m[l].records;
    if !records.is_null() {
        m.free(records.0);
        let l = m[list];
        m[l].records = Ptr32::NULL;
    }
    let l = m[list];
    if !l.is_null() {
        m.free(l.0);
        m[list] = Ptr32::NULL;
    }
    m[list] = Ptr32::NULL;
}

/// Gives a node another node's compensation state [soft 0x081cbf34 SI_classicModelCompInfoCopy].
///
/// Copied: **the whole flag word** (+0x08; a flag the caller set in the destination before
/// is lost), the four vectors at +0x3b8 … +0x3e4 as twelve words, the matrix at +0x360
/// (`SI_matrixCopy`: floats through the unit), and, when the source has compensation
/// records, a new list with a copy of each (the list from
/// [`compensate_info_list_allocate`], written without a test for null; the records from
/// `SI_calloc(count, 0x1c)`, copied one by one while the index is below the new list's
/// count).
pub fn classic_model_comp_info_copy(m: &mut Mem, from: Node, to: Node) {
    m[to].flags_08 = m[from].flags_08;
    for i in 0..12 {
        let word = m.u32(from.0 + 0x3b8 + 4 * i);
        m.set_u32(to.0 + 0x3b8 + 4 * i, word);
    }
    // SI_matrixCopy (0x08516224) is si-math's
    let matrix = m[from].cscl_matrix;
    m[to].cscl_matrix = matrix_copy(&matrix);
    if m[from].comp_info.is_null() {
        return;
    }
    let list = compensate_info_list_allocate(m);
    m[to].comp_info = list.cast();
    let source: Ptr32<CompInfoList> = m[from].comp_info.cast();
    m[list].count = m[source].count;
    let count = m[source].count;
    let records = m.si_calloc(count as u32, 0x1c);
    let list: Ptr32<CompInfoList> = m[to].comp_info.cast();
    m[list].records = Ptr32::new(records);
    let mut i = 0;
    loop {
        let list: Ptr32<CompInfoList> = m[to].comp_info.cast();
        if i >= m[list].count {
            break;
        }
        let source: Ptr32<CompInfoList> = m[from].comp_info.cast();
        let (s, d) = (m[source].records.add(i), m[list].records.add(i));
        m[d].key = m[s].key;
        m[d].a = m[s].a;
        m[d].b = m[s].b;
        i += 1;
    }
}
