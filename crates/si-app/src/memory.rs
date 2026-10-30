// SPDX-License-Identifier: MIT
//! `sys_memory.c`: the allocators and the per-process data.
//!
//! Each of the three allocators also clears `SI_secureLicAlloc` (0x08679660), a word of the
//! program's self-check: licence code, not ported.

use crate::env::World;
use crate::state::{App, ProcData};

/// The byte [soft 0x080d0ee8 SI_malloc] puts at the start of a block, and the one at its
/// end: "ok".
pub const MALLOC_FIRST: u8 = 0x6f;
pub const MALLOC_LAST: u8 = 0x6b;

/// [soft 0x080d0ee8 SI_malloc]: nothing for a size of 0; otherwise a block whose first byte
/// is `o` and whose last byte is `k` (for a size of 1 the `k` wins). The bytes between are
/// not initialised in the original; they are 0 here.
pub fn si_malloc(size: usize) -> Option<Vec<u8>> {
    if size == 0 {
        return None;
    }
    let mut block = vec![0; size];
    block[0] = MALLOC_FIRST;
    block[size - 1] = MALLOC_LAST;
    Some(block)
}

/// [soft 0x080d0f50 SI_calloc]: `calloc(count, size)` is always called, but for a count of
/// 0 the result is thrown away and null returned (a size of 0 is not checked).
pub fn si_calloc(count: usize, size: usize) -> Option<Vec<u8>> {
    if count == 0 {
        return None;
    }
    Some(vec![0; count * size])
}

/// [soft 0x080d0f28 SI_realloc]: for a size of 0 nothing happens and the block comes back
/// as it is (it is not freed); otherwise `realloc`. No marker bytes are written.
pub fn si_realloc(block: Option<Vec<u8>>, size: usize) -> Option<Vec<u8>> {
    if size == 0 {
        return block;
    }
    let mut b = block.unwrap_or_default();
    b.resize(size, 0);
    Some(b)
}

/// [soft 0x080d1004 SI_FreeLegacyLoaderPtr]: `free` unless the pointer is null.
pub fn free_legacy_loader_ptr(block: Option<Vec<u8>>) {
    drop(block);
}

/// [soft 0x080d0ed0 SI_swapSpaceAvailable]: always 1 in this build, whatever is asked for.
pub fn swap_space_available(_bytes: i32) -> i32 {
    1
}

/// [soft 0x080d0edc SI_start4] and [soft 0x080d0ff8 SI_end4]: the two functions that
/// bracket the source file, each returning a constant (every `SI_startN`/`SI_endN` pair
/// does; nothing in the program calls them).
pub fn si_start4() -> i32 {
    0xe
}
pub fn si_end4() -> i32 {
    0xf
}

impl<W: World> App<W> {
    /// [soft 0x080d0f80 SI_threadStorageInit]: copies three words into the process data
    /// when given some; always 0.
    pub fn thread_storage_init(&mut self, from: Option<&ProcData>) -> i32 {
        if let Some(p) = from {
            self.g.proc_data = *p;
        }
        0
    }

    /// [soft 0x080d0fa8 SI_threadStorageFree] and [soft 0x080d0fb4 SI_threadStorageExit]:
    /// nothing, 0.
    pub fn thread_storage_free(&mut self) -> i32 {
        0
    }
    pub fn thread_storage_exit(&mut self) -> i32 {
        0
    }

    /// [soft 0x080d0fc0 SI_threadStorageCopy]: copies the process data out; 6 without a
    /// destination (or without process data, which cannot happen here), else 0.
    pub fn thread_storage_copy(&self, to: Option<&mut ProcData>) -> i32 {
        match to {
            None => 6,
            Some(p) => {
                *p = self.g.proc_data;
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_and_zero_sizes() {
        assert_eq!(si_malloc(0), None);
        assert_eq!(si_malloc(1), Some(vec![MALLOC_LAST]));
        assert_eq!(si_malloc(3), Some(vec![MALLOC_FIRST, 0, MALLOC_LAST]));
        assert_eq!(si_calloc(0, 16), None);
        assert_eq!(si_calloc(2, 0), Some(vec![]));
        assert_eq!(si_realloc(Some(vec![1, 2]), 0), Some(vec![1, 2]));
        assert_eq!(si_realloc(None, 0), None);
        assert_eq!(si_realloc(Some(vec![1, 2]), 3), Some(vec![1, 2, 0]));
    }
}
