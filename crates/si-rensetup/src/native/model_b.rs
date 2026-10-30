// SPDX-License-Identifier: MIT
//! `model_b.c` and `settings.c`: the two routines of the model library that the hand-over
//! needs on the image and no crate has there: a new node, and the copy of a node's
//! algorithm. (`si-types` has `SI_modelAllocate` as a value, `si-model` the other on a
//! store of its own.) They are other packages' routines: the hand-over calls them through
//! its trait; [`super::world`] answers those calls with these.

use super::globals::{MODEL_ALLOCATE_COUNT, MODEL_ALLOCATE_FORMAT, SI_IDENTITE};
use si_math::algebra::matrix_copy;
use si_mem::{CStr32, Mem, Ptr32};
use si_types::model::Model;

/// A new model node [soft 0x08477cf4 SI_modelAllocate]: 0x430 bytes from `SI_malloc`,
/// null when there is no memory.
///
/// The routine's own counter is counted up and its low 16 bits are the node's serial
/// number. The name is a `strdup` of the argument, or, for a null argument, of `obj<n>`
/// with n the counter as a signed 16-bit number. Then every field is set but sixteen
/// (+0x0e, +0x42, +0x4e, +0x56, +0x5e, +0x20a, +0x210, +0x214, +0x220, +0x2ba, +0x2e6,
/// +0x334, +0x338, +0x33a, +0x340, +0x422), which keep what the allocator handed out:
/// zeros, but for the three scalings and the centre scaling (1, 1, 1), the eight matrices
/// (each copied from `SI_identite`), the two update words (0x3e), the node itself as its
/// lattice dependency, the element number −1, the visibility 7, the matte 1.0, the word
/// +0x400 (1) and the number of texture coordinate sets (1).
pub fn model_allocate(m: &mut Mem, name: CStr32) -> Ptr32<Model> {
    let node: Ptr32<Model> = Ptr32::new(m.si_malloc(0x430));
    if node.is_null() {
        return node;
    }
    let n = node.0;
    let count = m[MODEL_ALLOCATE_COUNT].wrapping_add(1);
    m[MODEL_ALLOCATE_COUNT] = count;
    m.set_u16(n + 0x40, count as u16);
    m.set_u32(n, 0);
    m.set_u32(n + 0x2d4, 0);
    let text = if name.is_null() {
        // sprintf(buffer, "obj%1hd", count) into a local of 80 bytes
        debug_assert_eq!(m.cstr(MODEL_ALLOCATE_FORMAT), b"obj%1hd");
        let buffer = m.local(0x50);
        m.put_cstr(buffer, format!("obj{}", count as i16).as_bytes());
        let copy = m.strdup(buffer);
        m.release(0x50);
        copy
    } else {
        m.strdup(name.0)
    };
    m.set_u32(n + 4, text);
    let words = |m: &mut Mem, offsets: &[u32], value: u32| {
        for o in offsets {
            m.set_u32(n + o, value);
        }
    };
    let halves = |m: &mut Mem, offsets: &[u32], value: u16| {
        for o in offsets {
            m.set_u16(n + o, value);
        }
    };
    // SI_matrixCopy (0x08516224) from SI_identite: si-math's copy of what the global holds
    let identity = |m: &mut Mem, offset: u32| {
        let matrix = matrix_copy(&m[SI_IDENTITE]);
        m.write(Ptr32::new(n + offset), matrix);
    };
    const ONE: u32 = 0x3f80_0000;
    words(m, &[0x18, 0x14, 0x10, 0x24, 0x20, 0x1c, 0x28, 0x2c], 0);
    words(m, &[0xa8, 0xac, 0xb0], ONE);
    halves(m, &[0x08], 0);
    words(m, &[0x30], 0);
    halves(m, &[0x0a, 0x3fa], 0x3e);
    halves(m, &[0x0c, 0x34], 0);
    words(m, &[0x38, 0x3c, 0x50], 0);
    halves(m, &[0x36], 0);
    identity(m, 0xb4);
    words(m, &[0xfc, 0xf8, 0xf4], 0);
    words(m, &[0x100, 0x104, 0x108], ONE);
    words(m, &[0x114, 0x110, 0x10c], 0);
    identity(m, 0x118);
    words(m, &[0x160, 0x15c, 0x158], 0);
    words(m, &[0x16c, 0x170, 0x174], ONE);
    identity(m, 0x178);
    words(m, &[0x1c0, 0x1bc, 0x1b8], 0);
    identity(m, 0x1c4);
    words(m, &[0x164, 0x168], 0);
    halves(m, &[0x204, 0x206], 0);
    words(m, &[0x20c], 0);
    halves(m, &[0x208, 0x216], 0);
    words(m, &[0x21c, 0x218, 0x224, 0x228, 0x22c, 0x42c, 0x230, 0x234], 0);
    identity(m, 0x238);
    identity(m, 0x278);
    halves(m, &[0x2b8], 0);
    words(m, &[0x2bc, 0x2c0, 0x2c4, 0x2c8], 0);
    halves(m, &[0x2e4, 0x4c, 0x54], 0);
    words(m, &[0x58, 0x60, 0x64, 0x2d8, 0x2dc], 0);
    halves(m, &[0x2e0, 0x2e2], 0);
    words(m, &[0x48], 0);
    identity(m, 0x68);
    words(m, &[0x2cc, 0x2d0], 0);
    m.set_u32(n + 0x44, n);
    halves(m, &[0x400], 1);
    words(m, &[0x2e8, 0x330, 0x2ec, 0x31c, 0x320, 0x324, 0x328, 0x32c, 0x2f0, 0x2f4, 0x2f8, 0x300, 0x304, 0x308, 0x2fc, 0x30c, 0x310, 0x314, 0x318], 0);
    halves(m, &[0x5c], 0);
    words(m, &[0x33c, 0x344, 0x348, 0x34c], 0);
    halves(m, &[0x350, 0x352], 0);
    words(m, &[0x354], 0xffff_ffff);
    words(m, &[0x358, 0x35c], 0);
    identity(m, 0x360);
    words(m, &[0x3a0, 0x3a4, 0x3a8], ONE);
    words(m, &[0x3b4, 0x3b0, 0x3ac, 0x3c0, 0x3bc, 0x3b8, 0x3cc, 0x3c8, 0x3c4, 0x3d8, 0x3d4, 0x3d0, 0x3e4, 0x3e0, 0x3dc], 0);
    words(m, &[0x3f4], ONE);
    words(m, &[0x3e8], 0);
    halves(m, &[0x3ec], 7);
    halves(m, &[0x3ee], 0);
    words(m, &[0x3f0], 0);
    halves(m, &[0x3f8], 0);
    words(m, &[0x3fc], 0);
    halves(m, &[0x402], 0);
    words(m, &[0x408, 0x40c, 0x410, 0x414, 0x418, 0x41c], 0);
    halves(m, &[0x404, 0x406, 0x420], 0);
    words(m, &[0x424], 1);
    words(m, &[0x428], 0);
    node
}

/// Gives a node the algorithm of another, as a copy has it
/// [soft 0x084a0efc SI_modelAlgoTypeCopy]: 2 becomes 1, 4 becomes 3, 8 becomes 5, 12
/// becomes 0, anything else is copied as it is.
pub fn model_algo_type_copy(m: &mut Mem, from: Ptr32<Model>, to: Ptr32<Model>) {
    m[to].algo_type = match m[from].algo_type {
        2 => 1,
        4 => 3,
        12 => 0,
        8 => 5,
        other => other,
    };
}
