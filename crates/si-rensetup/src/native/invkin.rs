// SPDX-License-Identifier: MIT
//! `invkin.c` and the general matrices of `algebra.c`: the copy and the disposal of a
//! node's inverse kinematics record, which the copy of a master's hierarchy makes for every
//! node. (`si-kine` has both files on a store of its own; these are the six routines the
//! hand-over reaches, on the image.) They are another package's routines: the hand-over
//! calls them through its trait; [`super::world`] answers those calls with these.

use si_mem::{record, Double, Mem, Opaque, Ptr32};
use si_x87::x87::F80;

record! {
    /// A matrix of doubles with any number of rows and columns
    /// (`SI_generalMatrixAllocateDbl`): 12 bytes, an array of row pointers, one block of
    /// eight bytes a column per row.
    pub struct GenMatD = "general matrix of doubles", size 0x0c {
        0x00 rows: i32, "number of rows";
        0x04 cols: i32, "number of columns";
        0x08 data: Ptr32<Ptr32<Double>>, "one pointer per row";
    }
}

/// Offsets into an inverse kinematics record (`SI_InvKin`, 0x74 bytes): the two general
/// matrices. The other words are copied or cleared as they are; what they mean is
/// `si-kine`'s.
pub mod ik {
    pub const SIZE: u32 = 0x74;
    pub const MATRIX_A: u32 = 0x2c;
    pub const MATRIX_B: u32 = 0x30;
}

/// A new matrix of doubles [soft 0x0851bed0 SI_generalMatrixAllocateDbl]: the record,
/// the array of `rows` pointers and `rows` rows of `cols` doubles, each from `SI_malloc`
/// and none cleared. Null as soon as a block cannot be had (what was made stays), which
/// includes a matrix without rows or without columns: `SI_malloc(0)` answers null.
pub fn general_matrix_allocate_dbl(m: &mut Mem, rows: i32, cols: i32) -> Ptr32<GenMatD> {
    let g: Ptr32<GenMatD> = Ptr32::new(m.si_malloc(0xc));
    if g.is_null() {
        return g;
    }
    m[g].rows = rows;
    m[g].cols = cols;
    m[g].data = Ptr32::new(m.si_malloc((rows as u32) << 2));
    if m[g].data.is_null() {
        return Ptr32::NULL;
    }
    let mut i = 0;
    while i < rows {
        let row: Ptr32<Double> = Ptr32::new(m.si_malloc((cols as u32) << 3));
        let slot = m[g].data.add(i);
        m[slot] = row;
        if row.is_null() {
            return Ptr32::NULL;
        }
        i += 1;
    }
    g
}

/// A copy of a matrix of doubles [soft 0x0851bfd4 SI_generalMatrixCopyDbl]: null for no
/// matrix or when the copy cannot be had; each double goes through the unit.
pub fn general_matrix_copy_dbl(m: &mut Mem, from: Ptr32<GenMatD>) -> Ptr32<GenMatD> {
    if from.is_null() {
        return from;
    }
    let (rows, cols) = (m[from].rows, m[from].cols);
    let to = general_matrix_allocate_dbl(m, rows, cols);
    if !to.is_null() {
        let mut i = 0;
        while i < m[from].rows {
            let mut j = 0;
            while j < m[from].cols {
                let (d, s) = (m[m[to].data.add(i)].add(j), m[m[from].data.add(i)].add(j));
                // fldl, fstpl: the same bits unless the double is a signalling NaN
                let v = F80::from_f64(f64::from_bits(m.u64(s.0))).to_f64();
                m.set_u64(d.0, v.to_bits());
                j += 1;
            }
            i += 1;
        }
    }
    to
}

/// Disposes of a matrix of doubles [soft 0x0851bf50 SI_generalMatrixDisposeDbl]: `g` is the
/// address of the pointer to it. Every row that is not null is freed and its pointer
/// cleared, then the array of rows, then the record; the caller's pointer is cleared.
pub fn general_matrix_dispose_dbl(m: &mut Mem, g: Ptr32<Ptr32<GenMatD>>) {
    if m[g].is_null() {
        return;
    }
    let mut i = 0;
    while i < m[m[g]].rows {
        let slot = m[m[g]].data.add(i);
        let row = m[slot];
        if !row.is_null() {
            m.free(row.0);
            let slot = m[m[g]].data.add(i);
            m[slot] = Ptr32::NULL;
        }
        i += 1;
    }
    let data = m[m[g]].data;
    if !data.is_null() {
        m.free(data.0);
        let record = m[g];
        m[record].data = Ptr32::NULL;
    }
    let record = m[g];
    if !record.is_null() {
        m.free(record.0);
        m[g] = Ptr32::NULL;
    }
    m[g] = Ptr32::NULL;
}

/// A new inverse kinematics record [soft 0x084ad8f0 SI_invKinAllocate]: 0x74 bytes from
/// `SI_malloc`, cleared but for the 16-bit word at +0x3c, which is 1, and the two 16-bit
/// words at +0x36 and +0x3e, which keep what the allocator handed out. Null when there is
/// no memory.
pub fn inv_kin_allocate(m: &mut Mem) -> Ptr32<Opaque> {
    let r = m.si_malloc(ik::SIZE);
    if r == 0 {
        return Ptr32::NULL;
    }
    for o in [0x10, 0x0c, 0x08, 0x1c, 0x18, 0x14, 0x28, 0x24, 0x20, 0x48, 0x44, 0x40, 0x58, 0x54, 0x50, 0x64, 0x60, 0x5c, 0x70, 0x6c, 0x68, 0x00, 0x04, 0x2c, 0x30] {
        m.set_u32(r + o, 0);
    }
    m.set_u16(r + 0x34, 0);
    m.set_u32(r + 0x38, 0);
    m.set_u16(r + 0x3c, 1);
    m.set_u32(r + 0x4c, 0);
    Ptr32::new(r)
}

/// A copy of an inverse kinematics record [soft 0x084adab0 SI_invKinCopy]: null for no
/// record or when none can be had. Copied: the words at +0x00, +0x04, +0x20 … +0x28 and
/// +0x40 … +0x70, the 16-bit words at +0x34 and +0x3c, and the two general matrices (each
/// a copy of its own). The six words at +0x08 … +0x1c and the word at +0x38 stay as the
/// allocator set them.
pub fn inv_kin_copy(m: &mut Mem, from: Ptr32<Opaque>) -> Ptr32<Opaque> {
    if from.is_null() {
        return from;
    }
    let to = inv_kin_allocate(m);
    if to.is_null() {
        return to;
    }
    let (s, d) = (from.0, to.0);
    for o in [0x00, 0x04, 0x20, 0x24, 0x28] {
        let w = m.u32(s + o);
        m.set_u32(d + o, w);
    }
    for o in [ik::MATRIX_A, ik::MATRIX_B] {
        let source = Ptr32::new(m.u32(s + o));
        let copy = general_matrix_copy_dbl(m, source);
        m.set_u32(d + o, copy.0);
    }
    for o in [0x34, 0x3c] {
        let w = m.u16(s + o);
        m.set_u16(d + o, w);
    }
    for o in (0x40..=0x70).step_by(4) {
        let w = m.u32(s + o);
        m.set_u32(d + o, w);
    }
    to
}

/// Disposes of an inverse kinematics record [soft 0x084ada60 SI_invKinDispose]: `record`
/// is the address of the node's pointer. Nothing for a null pointer; else the two general
/// matrices are disposed of, the record is freed and the pointer cleared.
pub fn inv_kin_dispose(m: &mut Mem, record: Ptr32<Ptr32<Opaque>>) {
    if m[record].is_null() {
        return;
    }
    let r = m[record].0;
    general_matrix_dispose_dbl(m, Ptr32::new(r + ik::MATRIX_A));
    let r = m[record].0;
    general_matrix_dispose_dbl(m, Ptr32::new(r + ik::MATRIX_B));
    let r = m[record];
    if !r.is_null() {
        m.free(r.0);
        m[record] = Ptr32::NULL;
    }
    m[record] = Ptr32::NULL;
}
