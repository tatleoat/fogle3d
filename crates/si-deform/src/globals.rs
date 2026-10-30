// SPDX-License-Identifier: MIT
//! The global variables the four source files read, at their addresses in the binary.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py si-deform`.

use crate::records::ProcData;
use si_mem::{Mem, Ptr32};
use si_types::scene::GrafList;

si_mem::globals! {
    /// The per-process record: current dictionary, current graph list, camera.
    SI_PROC_DATA: Ptr32<ProcData> = 0x08679314, "SI_procData", init 0x08782108;
    /// The array of element dictionaries, 0x70 bytes each (`si-dict` owns it; these files
    /// hand the current one's address to the editing layer).
    SI_ELEM_DICTS: Ptr32<si_types::dict::ElemDict> = 0x0867931c, "SI_elemDicts", init 0;
    /// The scene dictionary's chapter indices, one word per chapter of the chapter table.
    /// Read here: [0], [2] (clusters), [5] (lattices), [8] (models).
    SI_CID: [i32; 36] = 0x08781660, "SI_cid";
    /// The array of graph lists (`si-view` owns it; `SI_patchRemoveFitting` walks the
    /// scene's list).
    SI_GRAF_LISTS: Ptr32<GrafList> = 0x08679498, "SI_grafLists", init 0;
    /// The Duplicate settings (0x58 bytes by the symbol table; finding 0015). `SI_latticeCopyFromSetup` reads
    /// the 16-bit word at [`GLB_DUPLIC_LATTICE_ANIM`].
    SI_GLB_DUPLIC: [u8; 0x58] = 0x087814c0, "SI_glbDuplic";
    /// The identity matrix, a variable of `algebra.c` in writable data that every "new
    /// matrix" is copied from. Its value is written by [`init`] (the `globals!` form takes
    /// scalars only) and held against the binary by `re/tools/si-deform/check_data.py`.
    SI_IDENTITE: [[f32; 4]; 4] = 0x086dc160, "SI_identite";
    /// The attribute dictionary (`si-dict` owns it; its address is handed on).
    SI_ATD: si_types::dict::AttributeDict = 0x08781438, "SI_ATD";
    /// The relation dictionary (`si-dict` owns it; its address is handed on).
    SI_RD: si_types::dict::RelDict = 0x087815f0, "SI_RD";
    /// The static text buffer of `klusters.c` (one of several statics of that name in the
    /// binary): `SI_klusterListAssignToModel` formats an element's name in it.
    WORK_STR: [u8; 0x400] = 0x0881b220, "workStr";
}

/// Offset in [`SI_GLB_DUPLIC`] of the 16-bit word "copy every lattice of the set, with its
/// animation" (else the active one only).
pub const GLB_DUPLIC_LATTICE_ANIM: u32 = 0x20;

/// Chapters of [`SI_CID`] by index: the words at 0x08781660, +0x08, +0x14, +0x20.
pub const CID_FIRST: usize = 0;
pub const CID_CLUSTERS: usize = 2;
pub const CID_LATTICES: usize = 5;
pub const CID_MODELS: usize = 8;

/// Writes the initial values of this crate's globals: the scalars, and the identity matrix.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    for i in 0..4 {
        m[SI_IDENTITE][i][i] = 1.0;
    }
}
