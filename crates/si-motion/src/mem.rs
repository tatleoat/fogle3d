// SPDX-License-Identifier: MIT
//! What this crate adds to the shared memory image (`si_mem::Mem`, finding 0024).
//!
//! The routines of this crate are written against the original's own layouts: an object is
//! an address, a field is an offset from it (module [`crate::off`]), a global variable sits
//! at the address it has in the Linux `soft` binary (module [`crate::glob`]). The image they
//! work on is the one every crate shares; until the fifth pass this file held an image of
//! the crate's own, of which only the helpers below remain. They are here because `si-mem`
//! depends on no other crate and these need `si-math`'s types.
//!
//! Nothing here is the original's code.

use si_math::types::{Matrix, Vector};
use si_mem::{Access, Addr, Mem, Region};

/// Vectors and matrices in the image, and two questions the curve bridge asks about blocks.
pub trait MemExt {
    /// An `SI_Vector` (three floats).
    fn vector(&self, a: Addr) -> Vector;
    fn set_vector(&mut self, a: Addr, v: Vector);
    /// A 4 × 4 matrix of floats, by rows.
    fn matrix(&self, a: Addr) -> Matrix;
    fn set_matrix(&mut self, a: Addr, m: &Matrix);
    /// Records that the original stops here on something that is not a memory access (a
    /// division by zero): a fault at address 0.
    fn crash(&self);
    /// True when `n` bytes at `a` lie inside one block that was placed or allocated and not
    /// freed, whatever the image's strict switch says; outside the arena and the heap, when
    /// they are mapped. The curve bridge ([`crate::fcv`]) decides by this whether a key
    /// array has room, so it must not depend on how the caller set the image up.
    fn in_block(&self, a: Addr, n: u32) -> bool;
}

impl MemExt for Mem {
    fn vector(&self, a: Addr) -> Vector {
        Vector::new(self.f32(a), self.f32(a.wrapping_add(4)), self.f32(a.wrapping_add(8)))
    }

    fn set_vector(&mut self, a: Addr, v: Vector) {
        self.set_f32(a, v.x);
        self.set_f32(a.wrapping_add(4), v.y);
        self.set_f32(a.wrapping_add(8), v.z);
    }

    fn matrix(&self, a: Addr) -> Matrix {
        let mut m = [[0.0f32; 4]; 4];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = self.f32(a.wrapping_add((16 * r + 4 * c) as u32));
            }
        }
        m
    }

    fn set_matrix(&mut self, a: Addr, m: &Matrix) {
        for (r, row) in m.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                self.set_f32(a.wrapping_add((16 * r + 4 * c) as u32), *v);
            }
        }
    }

    fn crash(&self) {
        self.record_fault(0, 0, Access::Read);
    }

    fn in_block(&self, a: Addr, n: u32) -> bool {
        let blocks = match self.region_of(a) {
            Some(Region::Arena) => self.arena_blocks(),
            // the live block under either heap policy: the harness's (blocks one after the
            // other, never reused) and the program's (`Mem::program`, freed room reused),
            // which keeps no list of the blocks it handed out (`heap_blocks` is empty
            // there; until the whole evaluation was checked against `evalScene` this
            // answered "no room" for every key array of a loaded scene)
            Some(Region::Heap) => {
                return match self.live_block_of(a) {
                    Some((base, len)) => a - base <= len && n <= len - (a - base),
                    None => false,
                };
            }
            _ => return self.is_mapped(a, n),
        };
        let i = blocks.partition_point(|b| b.0 <= a);
        if i == 0 {
            return false;
        }
        let (base, len) = blocks[i - 1];
        a - base <= len && n <= len - (a - base) && !self.is_freed(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_and_matrices() {
        let mut m = Mem::new();
        let a = m.place(64, 8);
        let id = si_math::types::IDENTITY;
        m.set_matrix(a, &id);
        assert_eq!(m.matrix(a), id);
        m.set_vector(a + 16, Vector::new(1.0, 2.0, 3.0));
        assert_eq!(m.vector(a + 16).to_array(), [1.0, 2.0, 3.0]);
        assert!(m.fault().is_none());
    }

    #[test]
    fn blocks_whatever_the_switch() {
        let mut m = Mem::new();
        let a = m.si_malloc(0x24);
        let b = m.place(16, 8);
        for strict in [false, true] {
            m.set_strict(strict);
            assert!(m.in_block(a, 0x24) && m.in_block(a + 0x20, 4));
            assert!(!m.in_block(a, 0x25) && !m.in_block(a - 4, 4));
            assert!(m.in_block(b + 8, 8) && !m.in_block(b + 8, 9));
        }
        m.free(a);
        assert!(!m.in_block(a, 1));
        // the two differ exactly here: a freed block stays readable unless the image is strict
        m.set_strict(false);
        assert!(m.is_mapped(a, 1));
        m.set_strict(true);
        assert!(!m.is_mapped(a, 1));
        m.crash();
        assert_eq!(m.fault().map(|f| f.addr), Some(0));
    }
}
