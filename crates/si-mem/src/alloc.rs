// SPDX-License-Identifier: MIT
//! Allocation as the original's routines see it: the heap, the arena of test records and
//! the stack.
//!
//! Nothing here is cited as a port: the program gets its memory from the C library, so the
//! allocator is ours to build. It serves two needs that pull apart, and which one is a
//! choice made when the image is made ([`HeapPolicy`], [`Mem::with_heap`]).
//!
//! **[`HeapPolicy::Harness`], the default.** The C library's four routines the way the
//! emulation harness's stand-ins behave (`re/tools/emu/emu.py`, `standins.py`), so that a
//! port and the emulated original, started on the same image, get **the same addresses in
//! the same order**: a bump allocator, 8-aligned, an 8-byte header before each block (the
//! size and the mark 0x6d656d21), blocks never reused, `realloc` always a new block, a
//! freed block still readable. The harness stops a case on a double `free` or a `free` of
//! something that is no block; here that is a recorded fault ([`crate::Access::Free`]).
//! Every emulation check compares addresses of blocks with the harness's and uses this.
//!
//! **[`HeapPolicy::Reuse`], for a whole run of the program.** The dictionaries grow every
//! array by one record per addition, each growth a `realloc`; under the harness's rules a
//! chapter of *n* elements costs *n*² × 66 bytes of heap. Here a freed block's room is
//! used again (the smallest free span that fits; neighbours that are free are joined; a
//! free span at the top goes back to the unused part), `realloc` grows a block where it
//! lies when the room behind it is free or unused, and a block that has to move takes half
//! its size again as room to grow into, so that growing one record at a time moves a block
//! a number of times that goes with the logarithm of its size, not with its size. The
//! bookkeeping is kept beside the image (in Rust collections), not in it; the header
//! before a block is written as under the other policy. Nothing of this is the original's
//! C library: block addresses under this policy mean nothing and are compared with nothing.
//!
//! What both policies keep: every address is a multiple of 8 (after the header), a block's
//! fresh bytes hold the fill ([`Fill`]; also the bytes a `realloc` adds, also where the
//! room was used before), `malloc(0)` is a block of no bytes, the heap's end answers null,
//! a `free` of what is no live block is a fault. With [`Fill::Zero`] the bytes of every
//! live block are therefore the same under both, whatever the addresses.
//!
//! The program does not call `malloc` directly but three wrappers of its own in
//! `sys_memory.c`. Findings 0010 and 0019 describe them and `crates/si-app/src/memory.rs`
//! ports and cites them on byte blocks; [`Mem::si_malloc`], [`Mem::si_calloc`] and
//! [`Mem::si_realloc`] are the same three on the image, so that every crate allocates
//! through one place. They carry no citation of their own: the lead may move the three
//! citations here when `si-app` is converted.

use crate::{Access, Fill, Mem};
use std::collections::{BTreeMap, BTreeSet};

/// The mark the harness's `malloc` writes into a block's header, after the size.
pub const BLOCK_MARK: u32 = 0x6d65_6d21;
/// The byte the program's `SI_malloc` puts at the start of a block, and the one at its end.
pub const SI_MALLOC_FIRST: u8 = 0x6f;
pub const SI_MALLOC_LAST: u8 = 0x6b;

/// How the heap hands out blocks (see the module). Chosen when the image is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HeapPolicy {
    /// The emulation harness's allocator, address for address: blocks one after the
    /// other, none used again, `realloc` always a new block. For every check that is
    /// compared with the emulated original. The default.
    #[default]
    Harness,
    /// Freed room is used again and `realloc` grows a block in place where it can. For a
    /// whole run of the program; the addresses are this allocator's own.
    Reuse,
}

/// What the heap has done so far ([`Mem::heap_stats`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapStats {
    pub policy: HeapPolicy,
    /// Bytes from the heap's start to its top now: what is handed out, with the headers,
    /// the free spans between blocks and, under the harness's policy, every freed block.
    pub used: u32,
    /// The largest `used` so far: how large a heap the run needed.
    pub peak: u32,
    /// Blocks that are live, and the bytes that were asked for them.
    pub live_blocks: u32,
    pub live_bytes: u64,
    /// Bytes below the top that a later block can take (always 0 under the harness's
    /// policy, which uses nothing again).
    pub free_bytes: u64,
    /// Blocks handed out by `malloc`, `calloc` and `realloc` of null.
    pub mallocs: u64,
    pub frees: u64,
    /// `realloc`s that left the block where it was, and those that moved it, with the
    /// bytes the moves copied.
    pub reallocs_in_place: u64,
    pub reallocs_moved: u64,
    pub bytes_moved: u64,
}

/// A free span shorter than this is left with the block before it.
const MIN_SPLIT: u32 = 16;

/// The allocator's own knowledge of the heap, beside the image.
pub(crate) struct HeapState {
    pub(crate) policy: HeapPolicy,
    /// Reuse: the live blocks by address: the bytes asked for, and the length of the span
    /// the block lies in (the header, the bytes, and room behind them that is its own).
    live: BTreeMap<u32, (u32, u32)>,
    /// Reuse: the free spans below the top, by address and by length.
    free_at: BTreeMap<u32, u32>,
    free_len: BTreeSet<(u32, u32)>,
    free_bytes: u64,
    /// Both policies: the blocks that were freed and whose address was not handed out
    /// again (under the harness's policy: every freed block).
    freed_set: BTreeSet<u32>,
    /// Reuse: the end of the highest span ever handed out. From here on no block ever
    /// lay, so the memory is as the image made it.
    dirty: u32,
    /// The highest top so far.
    peak: u32,
    live_blocks: u32,
    live_bytes: u64,
    mallocs: u64,
    frees: u64,
    in_place: u64,
    moved: u64,
    bytes_moved: u64,
}

impl HeapState {
    pub(crate) fn new(policy: HeapPolicy, base: u32) -> HeapState {
        HeapState {
            policy,
            live: BTreeMap::new(),
            free_at: BTreeMap::new(),
            free_len: BTreeSet::new(),
            free_bytes: 0,
            freed_set: BTreeSet::new(),
            dirty: base,
            peak: base,
            live_blocks: 0,
            live_bytes: 0,
            mallocs: 0,
            frees: 0,
            in_place: 0,
            moved: 0,
            bytes_moved: 0,
        }
    }

    pub(crate) fn is_freed(&self, p: u32) -> bool {
        self.freed_set.contains(&p)
    }

    /// Reuse: whether `n` bytes at `addr` lie in one live block (one past its end is
    /// still the block, for no bytes).
    pub(crate) fn in_live(&self, addr: u32, n: u32) -> bool {
        match self.live.range(..=addr).next_back() {
            Some((&p, &(size, _))) => addr - p <= size && n <= size - (addr - p),
            None => false,
        }
    }

    fn add_free(&mut self, start: u32, len: u32) {
        self.free_at.insert(start, len);
        self.free_len.insert((len, start));
        self.free_bytes += len as u64;
    }

    fn take_free(&mut self, start: u32, len: u32) {
        self.free_at.remove(&start);
        self.free_len.remove(&(len, start));
        self.free_bytes -= len as u64;
    }

    /// The addresses from `start` to `end` belong to a block again: a block that was
    /// freed there is no longer told apart from memory that never was one.
    fn forget_freed(&mut self, start: u32, end: u32) {
        if self.freed_set.range(start..end).next().is_some() {
            let stale: Vec<u32> = self.freed_set.range(start..end).copied().collect();
            for a in stale {
                self.freed_set.remove(&a);
            }
        }
    }

    fn count_in(&mut self, n: u32) {
        self.live_blocks += 1;
        self.live_bytes += n as u64;
    }

    fn count_out(&mut self, n: u32) {
        // never below zero: under a map without headers two blocks can share an address
        // (one of no bytes and the next), and the count must not be what stops a routine
        self.live_blocks = self.live_blocks.saturating_sub(1);
        self.live_bytes = self.live_bytes.saturating_sub(n as u64);
    }
}

impl Mem {
    /// Fresh memory at `addr`: the allocators' own stores, which may lie outside every
    /// block (a header, a gap) and so are not held to strict blocks.
    fn fresh(&mut self, addr: u32, n: u32) {
        if n == 0 {
            return;
        }
        let strict = core::mem::replace(&mut self.strict, false);
        self.fresh_unchecked(addr, n);
        self.strict = strict;
    }

    fn fresh_unchecked(&mut self, addr: u32, n: u32) {
        match self.fill {
            // fresh heap and arena memory is zero already; a local may not be
            Fill::Zero => self.set_fill_bytes(addr, 0, n),
            fill => {
                let bytes: Vec<u8> = (0..n).map(|i| fill.byte(addr.wrapping_add(i))).collect();
                self.set_bytes(addr, &bytes);
            }
        }
    }

    /// The header before a block: its size and the mark, where the map leaves room for it.
    fn write_header(&mut self, start: u32, n: u32) {
        if self.map.heap_header >= 8 {
            let strict = core::mem::replace(&mut self.strict, false);
            self.set_u32(start, n);
            self.set_u32(start + 4, BLOCK_MARK);
            self.strict = strict;
        }
    }

    /// Notes a block as freed, in the order of the frees.
    fn note_freed(&mut self, p: u32) {
        self.freed.push(p);
        self.heap.freed_set.insert(p);
    }

    /// The size of the block that starts at `p` by the harness's tables: (a heap block's,
    /// a placed buffer's).
    fn harness_block(&self, p: u32) -> (Option<u32>, Option<u32>) {
        let find = |v: &Vec<(u32, u32)>| v.binary_search_by_key(&p, |b| b.0).ok().map(|i| v[i].1);
        (find(&self.heap_blocks), find(&self.arena_blocks))
    }

    /// How the heap hands out blocks.
    pub fn heap_policy(&self) -> HeapPolicy {
        self.heap.policy
    }

    /// What the heap has done so far: its size now and at its largest, the live blocks,
    /// how many `realloc`s moved a block.
    pub fn heap_stats(&self) -> HeapStats {
        let h = &self.heap;
        HeapStats {
            policy: h.policy,
            used: self.heap_top - self.map.heap.0,
            peak: h.peak.max(self.heap_top) - self.map.heap.0,
            live_blocks: h.live_blocks,
            live_bytes: h.live_bytes,
            free_bytes: h.free_bytes,
            mallocs: h.mallocs,
            frees: h.frees,
            reallocs_in_place: h.in_place,
            reallocs_moved: h.moved,
            bytes_moved: h.bytes_moved,
        }
    }

    // ---- the C library's heap

    /// `malloc(n)`: a block of `n` bytes holding the fill ([`Fill`]), or null when the heap
    /// is used up. `malloc(0)` is a block of no bytes, not null (as in the C library).
    pub fn malloc(&mut self, n: u32) -> u32 {
        if self.heap.policy == HeapPolicy::Reuse {
            return self.reuse_malloc(n);
        }
        let (base, size) = self.map.heap;
        let header = self.map.heap_header;
        let a = (self.heap_top + 7) & !7;
        if n > size || (a - base) as u64 + header as u64 + n as u64 > size as u64 {
            return 0;
        }
        // a heap backed in steps: the host may have no room for more of it
        if !self.heap_reach(a + header + n) {
            return 0;
        }
        self.write_header(a, n);
        let p = a + header;
        if self.fill != Fill::Zero {
            self.fresh(p, n);
        }
        self.heap_top = p + n;
        self.heap_blocks.push((p, n));
        self.heap.mallocs += 1;
        self.heap.count_in(n);
        p
    }

    /// `calloc(count, size)`: `malloc` of the product, zeroed.
    pub fn calloc(&mut self, count: u32, size: u32) -> u32 {
        let n = count as u64 * size as u64;
        if n > self.map.heap.1 as u64 {
            return 0;
        }
        let p = self.malloc(n as u32);
        if p != 0 {
            self.set_fill_bytes(p, 0, n as u32);
        }
        p
    }

    /// The size of the block that starts at `p`: a heap block or a placed buffer of the
    /// arena (the harness lets a routine free a test buffer). Under the harness's policy
    /// a freed block still has its size; under [`HeapPolicy::Reuse`] only a live one has.
    pub fn block_size(&self, p: u32) -> Option<u32> {
        let (heap, arena) = self.harness_block(p);
        if self.heap.policy == HeapPolicy::Reuse {
            return self.heap.live.get(&p).map(|b| b.0).or(arena);
        }
        heap.or(arena)
    }

    /// `realloc(p, n)`. `realloc(null, n)` is `malloc(n)`; `realloc(p, 0)` frees and answers
    /// null; when there is no room the block stays as it is and the answer is null.
    ///
    /// Under the harness's policy always a new block with the old bytes (the C library may
    /// or may not move a block; a ported routine must not depend on which, and the harness
    /// always moves it). Under [`HeapPolicy::Reuse`] the block stays where it is when it
    /// shrinks, when it still fits the room it has, or when the room behind it is free.
    pub fn realloc(&mut self, p: u32, n: u32) -> u32 {
        if p == 0 {
            return self.malloc(n);
        }
        if self.heap.policy == HeapPolicy::Reuse && self.heap.live.contains_key(&p) {
            return self.reuse_realloc(p, n);
        }
        let Some(old) = self.block_size(p) else {
            self.record_fault(p, 0, Access::Free);
            return 0;
        };
        if self.heap.is_freed(p) {
            self.record_fault(p, 0, Access::Free);
            return 0;
        }
        let in_heap = self.harness_block(p).0.is_some() && self.heap.policy == HeapPolicy::Harness;
        if n == 0 {
            self.note_freed(p);
            if in_heap {
                self.heap.frees += 1;
                self.heap.count_out(old);
            }
            return 0;
        }
        let q = self.malloc(n);
        if q != 0 {
            self.copy(q, p, old.min(n));
            self.note_freed(p);
            // the block `malloc` counted is this one moved, not one more
            self.heap.mallocs -= 1;
            self.heap.moved += 1;
            self.heap.bytes_moved += old.min(n) as u64;
            if in_heap {
                self.heap.count_out(old);
            }
        }
        q
    }

    /// `free(p)`: nothing for null. A block freed twice, or an address that is no block,
    /// is a fault.
    ///
    /// Under the harness's policy the block is noted as freed ([`Mem::freed`]) and stays
    /// readable. Under [`HeapPolicy::Reuse`] its room is free for a later block; its bytes
    /// stay until one takes them.
    pub fn free(&mut self, p: u32) {
        if p == 0 {
            return;
        }
        if self.heap.policy == HeapPolicy::Reuse {
            if let Some((n, span)) = self.heap.live.remove(&p) {
                self.release_span(p - self.map.heap_header, span);
                self.heap.freed_set.insert(p);
                self.heap.frees += 1;
                self.heap.count_out(n);
                return;
            }
        }
        let (heap, arena) = self.harness_block(p);
        let heap = heap.filter(|_| self.heap.policy == HeapPolicy::Harness);
        if heap.or(arena).is_none() || self.heap.is_freed(p) {
            self.record_fault(p, 0, Access::Free);
            return;
        }
        self.note_freed(p);
        if let Some(n) = heap {
            self.heap.frees += 1;
            self.heap.count_out(n);
        }
    }

    /// The blocks released so far, in order (the harness's output `freed=`). Under
    /// [`HeapPolicy::Reuse`] the heap keeps no history: only placed buffers of the arena
    /// that were freed are listed.
    pub fn freed(&self) -> &[u32] {
        &self.freed
    }

    /// Whether the block that starts at `p` was freed. Under [`HeapPolicy::Reuse`]: and
    /// its first byte was not handed out again since.
    pub fn is_freed(&self, p: u32) -> bool {
        self.heap.is_freed(p)
    }

    /// The blocks `malloc` handed out, in order: (address, size). The harness names them
    /// `heap0`, `heap1`, … in this order. Empty under [`HeapPolicy::Reuse`], which keeps no
    /// history: see [`Mem::live_blocks`].
    pub fn heap_blocks(&self) -> &[(u32, u32)] {
        &self.heap_blocks
    }

    /// The heap block `addr` points into: (its number, the offset in it). `None` under
    /// [`HeapPolicy::Reuse`], where blocks have no numbers: see [`Mem::live_block_of`].
    pub fn heap_block_of(&self, addr: u32) -> Option<(usize, u32)> {
        let i = self.heap_blocks.partition_point(|b| b.0 <= addr).checked_sub(1)?;
        let (base, size) = self.heap_blocks[i];
        // one past the end still names the block, as a C pointer may
        (addr - base <= size).then_some((i, addr - base))
    }

    /// The live heap blocks in address order: (address, size). Under either policy.
    pub fn live_blocks(&self) -> Vec<(u32, u32)> {
        match self.heap.policy {
            HeapPolicy::Reuse => self.heap.live.iter().map(|(&p, &(n, _))| (p, n)).collect(),
            HeapPolicy::Harness => {
                let mut v: Vec<(u32, u32)> = self.heap_blocks.iter().copied().filter(|b| !self.heap.is_freed(b.0)).collect();
                v.sort();
                v
            }
        }
    }

    /// The live heap block `addr` points into: (its address, its size). One past the end
    /// still names the block. Under either policy.
    pub fn live_block_of(&self, addr: u32) -> Option<(u32, u32)> {
        match self.heap.policy {
            HeapPolicy::Reuse => {
                let (&p, &(n, _)) = self.heap.live.range(..=addr).next_back()?;
                (addr - p <= n).then_some((p, n))
            }
            HeapPolicy::Harness => {
                let (i, _) = self.heap_block_of(addr)?;
                let b = self.heap_blocks[i];
                (!self.heap.is_freed(b.0)).then_some(b)
            }
        }
    }

    // ---- the heap that uses room again (HeapPolicy::Reuse)

    /// The length of the span a block of `n` bytes needs: the header and the bytes, a
    /// multiple of 8 and at least 8 (so that two blocks of no bytes have two addresses).
    fn span_len(&self, n: u32) -> Option<u32> {
        let need = ((self.map.heap_header as u64 + n as u64 + 7) & !7).max(8);
        (need <= self.map.heap.1 as u64).then_some(need as u32)
    }

    /// A span of at least `need` bytes: the smallest free one that fits (what is left over
    /// of it stays free when it is worth a block), or new room at the top.
    fn take_span(&mut self, need: u32) -> Option<(u32, u32)> {
        if let Some(&(len, start)) = self.heap.free_len.range((need, 0)..).next() {
            self.heap.take_free(start, len);
            if len - need >= MIN_SPLIT {
                self.heap.add_free(start + need, len - need);
                return Some((start, need));
            }
            return Some((start, len));
        }
        let (base, size) = self.map.heap;
        let start = (self.heap_top as u64 + 7) & !7;
        let end = start + need as u64;
        if end > base as u64 + size as u64 || !self.heap_reach(end as u32) {
            return None;
        }
        self.heap_top = end as u32;
        self.heap.peak = self.heap.peak.max(self.heap_top);
        Some((start as u32, need))
    }

    /// Gives a span back: joined with free neighbours, and returned to the unused part
    /// when it then ends at the top.
    fn release_span(&mut self, mut start: u32, mut len: u32) {
        if let Some((&before, &n)) = self.heap.free_at.range(..start).next_back() {
            if before + n == start {
                self.heap.take_free(before, n);
                start = before;
                len += n;
            }
        }
        if let Some(&n) = self.heap.free_at.get(&(start + len)) {
            self.heap.take_free(start + len, n);
            len += n;
        }
        if start + len == self.heap_top {
            self.heap_top = start;
        } else {
            self.heap.add_free(start, len);
        }
    }

    /// Bytes that have just become part of a block hold the fill, whatever lay there
    /// before. Memory above every span ever handed out is zero already.
    fn reuse_fresh(&mut self, addr: u32, n: u32) {
        match self.fill {
            Fill::Zero => {
                let stale = self.heap.dirty.saturating_sub(addr).min(n);
                if stale != 0 {
                    self.fresh(addr, stale);
                }
            }
            _ => self.fresh(addr, n),
        }
    }

    /// Makes the span at `start` the block of `n` bytes, of which the first `kept` hold
    /// what they should already.
    fn reuse_install(&mut self, start: u32, span: u32, n: u32, kept: u32) -> u32 {
        let p = start + self.map.heap_header;
        self.write_header(start, n);
        if n > kept {
            self.reuse_fresh(p + kept, n - kept);
        }
        self.heap.dirty = self.heap.dirty.max(start + span);
        self.heap.forget_freed(start, start + span);
        self.heap.live.insert(p, (n, span));
        p
    }

    fn reuse_malloc(&mut self, n: u32) -> u32 {
        let Some(need) = self.span_len(n) else {
            return 0;
        };
        let Some((start, span)) = self.take_span(need) else {
            return 0;
        };
        self.heap.mallocs += 1;
        self.heap.count_in(n);
        self.reuse_install(start, span, n, 0)
    }

    /// `realloc` of a live heap block under [`HeapPolicy::Reuse`].
    fn reuse_realloc(&mut self, p: u32, n: u32) -> u32 {
        if n == 0 {
            self.free(p);
            return 0;
        }
        let (old, span) = self.heap.live[&p];
        let Some(need) = self.span_len(n) else {
            return 0;
        };
        let start = p - self.map.heap_header;
        let mut room = span;
        if need > room && start + room == self.heap_top {
            // the last block: the unused part behind it is its to grow into
            let end = start as u64 + need as u64;
            if end <= self.map.heap.0 as u64 + self.map.heap.1 as u64 && self.heap_reach(end as u32) {
                self.heap_top = end as u32;
                self.heap.peak = self.heap.peak.max(self.heap_top);
                room = need;
            }
        } else if need > room {
            // a free span behind it
            if let Some(&n_free) = self.heap.free_at.get(&(start + room)) {
                if room as u64 + n_free as u64 >= need as u64 {
                    self.heap.take_free(start + room, n_free);
                    room += n_free;
                }
            }
        }
        if need <= room {
            // in place; what is left over behind it goes back when the block shrank or
            // took a free span, and stays with it when it is its own room to grow into
            if room - need >= MIN_SPLIT && (n < old || room != span) {
                self.release_span(start + need, room - need);
                room = need;
            }
            self.heap.live.remove(&p);
            self.heap.live_bytes = self.heap.live_bytes - old as u64 + n as u64;
            self.heap.in_place += 1;
            return self.reuse_install(start, room, n, old.min(n));
        }
        // it has to move: with half its size again as room to grow into, so that a block
        // grown a little at a time is not moved every time
        let roomy = self.span_len(n.saturating_add(n / 2)).filter(|&r| r > need);
        let Some((to, to_span)) = roomy.and_then(|r| self.take_span(r)).or_else(|| self.take_span(need)) else {
            return 0;
        };
        self.heap.live.remove(&p);
        self.heap.live_bytes = self.heap.live_bytes - old as u64 + n as u64;
        let q = self.reuse_install(to, to_span, n, old.min(n));
        let strict = core::mem::replace(&mut self.strict, false);
        self.copy(q, p, old.min(n));
        self.strict = strict;
        self.release_span(start, span);
        self.heap.freed_set.insert(p);
        self.heap.moved += 1;
        self.heap.bytes_moved += old.min(n) as u64;
        q
    }

    /// Holds the allocator's tables against each other and against the image: the spans
    /// of the live blocks and the free spans lie one after the other from the heap's start
    /// to its top without a gap or an overlap, no two free spans touch, every block fits
    /// its span, every header is intact, the counts are right. For tests and for finding
    /// out who wrote over a header; `Err` says what is wrong.
    pub fn heap_verify(&self) -> Result<(), String> {
        let header = self.map.heap_header;
        let (base, size) = self.map.heap;
        if self.heap_top < base || (self.heap_top - base) as u64 > size as u64 {
            return Err(format!("the top {:#x} is outside the heap", self.heap_top));
        }
        let marked = |start: u32, n: u32| header < 8 || (self.try_bytes(start, 8).map(|b| (u32::from_le_bytes(b[..4].try_into().unwrap()), u32::from_le_bytes(b[4..].try_into().unwrap()))) == Some((n, BLOCK_MARK)));
        if self.heap.policy == HeapPolicy::Harness {
            let mut at = base;
            let mut live = (0u32, 0u64);
            for &(p, n) in &self.heap_blocks {
                if p - header < at || p as u64 + n as u64 > self.heap_top as u64 {
                    return Err(format!("block {p:#x} of {n} bytes overlaps the one before it or the top"));
                }
                if !self.strict && !marked(p - header, n) {
                    return Err(format!("the header of block {p:#x} was written over"));
                }
                if !self.heap.is_freed(p) {
                    live = (live.0 + 1, live.1 + n as u64);
                }
                at = p + n;
            }
            if live != (self.heap.live_blocks, self.heap.live_bytes) {
                return Err(format!("{} live blocks of {} bytes counted, {} of {} found", self.heap.live_blocks, self.heap.live_bytes, live.0, live.1));
            }
            return Ok(());
        }
        let mut spans: Vec<(u32, u32, Option<u32>)> = self.heap.live.iter().map(|(&p, &(n, span))| (p - header, span, Some(n))).collect();
        spans.extend(self.heap.free_at.iter().map(|(&s, &len)| (s, len, None)));
        spans.sort();
        let mut at = (base + 7) & !7;
        let mut last_free = false;
        let mut live = (0u32, 0u64);
        let mut free = 0u64;
        for (start, len, block) in spans {
            if start != at {
                return Err(format!("a span starts at {start:#x} where {at:#x} was expected (a gap or an overlap)"));
            }
            match block {
                Some(n) => {
                    if (header as u64 + n as u64) > len as u64 {
                        return Err(format!("block {:#x} of {n} bytes does not fit its span of {len}", start + header));
                    }
                    if !self.strict && !marked(start, n) {
                        return Err(format!("the header of block {:#x} was written over", start + header));
                    }
                    live = (live.0 + 1, live.1 + n as u64);
                    last_free = false;
                }
                None => {
                    if last_free {
                        return Err(format!("two free spans touch at {start:#x}"));
                    }
                    if !self.heap.free_len.contains(&(len, start)) {
                        return Err(format!("the free span at {start:#x} is missing from the table by length"));
                    }
                    free += len as u64;
                    last_free = true;
                }
            }
            at = start + len;
        }
        if at != self.heap_top && !(self.heap.live.is_empty() && self.heap.free_at.is_empty() && self.heap_top == base) {
            return Err(format!("the spans end at {at:#x}, the top is {:#x}", self.heap_top));
        }
        if last_free {
            return Err("a free span lies at the top".to_string());
        }
        if live != (self.heap.live_blocks, self.heap.live_bytes) || free != self.heap.free_bytes || self.heap.free_len.len() != self.heap.free_at.len() {
            return Err(format!("{} live blocks of {} bytes and {} free bytes counted; {} of {} and {} found", self.heap.live_blocks, self.heap.live_bytes, self.heap.free_bytes, live.0, live.1, free));
        }
        Ok(())
    }

    // ---- the program's wrappers (sys_memory.c; ported and cited in si-app)

    /// `SI_malloc(n)`: null for a size of 0; otherwise `malloc(n)` with the first byte set
    /// to `o` and the last to `k` (for a size of 1 the `k` wins). The bytes between are the
    /// fill: the original clears nothing.
    pub fn si_malloc(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let p = self.malloc(n);
        if p != 0 {
            self.set_u8(p, SI_MALLOC_FIRST);
            self.set_u8(p + n - 1, SI_MALLOC_LAST);
        }
        p
    }

    /// `SI_calloc(count, size)`: `calloc` is always called, but for a count of 0 its result
    /// is dropped (the block is lost) and null returned; a size of 0 is not looked at.
    pub fn si_calloc(&mut self, count: u32, size: u32) -> u32 {
        let p = self.calloc(count, size);
        if count == 0 {
            return 0;
        }
        p
    }

    /// `SI_realloc(p, n)`: for a size of 0 nothing happens and `p` comes back (it is not
    /// freed); otherwise `realloc`. No marker bytes.
    pub fn si_realloc(&mut self, p: u32, n: u32) -> u32 {
        if n == 0 {
            return p;
        }
        self.realloc(p, n)
    }

    /// `strdup`: a copy of the string at `s` in a new block; null when the heap is used up.
    pub fn strdup(&mut self, s: u32) -> u32 {
        let text = self.cstr(s);
        let p = self.malloc(text.len() as u32 + 1);
        if p != 0 {
            self.put_cstr(p, &text);
        }
        p
    }

    // ---- the arena: records made by tests and checks

    /// A buffer of `n` bytes in the arena, as the harness's own `alloc` places a case's
    /// buffers: aligned, holding the fill, followed by a gap of fill bytes (1 KB) so that a
    /// read past its end meets memory nobody wrote and not the next buffer. 0 when the
    /// arena is used up.
    pub fn place(&mut self, n: u32, align: u32) -> u32 {
        let (base, size) = self.map.arena;
        let gap = self.map.arena_gap;
        let align = align.max(1);
        let a = self.arena_top.next_multiple_of(align);
        if (a - base) as u64 + n as u64 + gap as u64 > size as u64 {
            return 0;
        }
        self.arena_top = a + n + gap;
        if self.fill != Fill::Zero {
            self.fresh(a, n + gap);
        }
        self.arena_blocks.push((a, n));
        a
    }

    /// A buffer of `n` bytes at an address of the arena that the caller chose (a vector
    /// file that carries its buffers' addresses, a scenario that names them). False when
    /// it does not fit in the arena. Later [`Mem::place`] calls go behind it.
    pub fn place_at(&mut self, addr: u32, n: u32) -> bool {
        let (base, size) = self.map.arena;
        let off = addr.wrapping_sub(base);
        if off > size || n > size - off {
            return false;
        }
        let i = self.arena_blocks.partition_point(|b| b.0 < addr);
        self.arena_blocks.insert(i, (addr, n));
        self.arena_top = self.arena_top.max(addr + n);
        if self.fill != Fill::Zero {
            self.fresh(addr, n);
        }
        true
    }

    /// A buffer in the arena holding `bytes` (8-aligned).
    pub fn place_bytes(&mut self, bytes: &[u8]) -> u32 {
        let a = self.place(bytes.len() as u32, 8);
        if a != 0 {
            self.set_bytes(a, bytes);
        }
        a
    }

    /// A record in the arena (8-aligned, as every buffer of the harness).
    pub fn place_record<T: crate::Pod>(&mut self, value: T) -> crate::Ptr32<T> {
        let p = crate::Ptr32::new(self.place(core::mem::size_of::<T>() as u32, 8));
        if !p.is_null() {
            self.write(p, value);
        }
        p
    }

    /// A zero-terminated string in the arena.
    pub fn place_cstr(&mut self, s: &[u8]) -> u32 {
        let a = self.place(s.len() as u32 + 1, 8);
        if a != 0 {
            self.put_cstr(a, s);
        }
        a
    }

    /// The buffers placed so far: (address, size).
    pub fn arena_blocks(&self) -> &[(u32, u32)] {
        &self.arena_blocks
    }

    // ---- the stack: locals whose address is passed on

    /// Room for a local variable whose address the original hands to a callee (an output
    /// argument, a record built on the stack): `size` bytes rounded up to 4, below the
    /// stack pointer, holding the fill. Give it back with [`Mem::release`] in reverse
    /// order, or keep [`Mem::sp`] and restore it with [`Mem::set_sp`]. Null, and a fault,
    /// when the stack is used up.
    ///
    /// The address is not the original's (that depends on the frame the compiler laid
    /// out); what is the same is that it is a real address the callee can store through
    /// and that goes stale when the caller returns.
    pub fn local(&mut self, size: u32) -> u32 {
        let n = size.wrapping_add(3) & !3;
        let (base, _) = self.map.stack;
        if n > self.sp.wrapping_sub(base) || self.sp < base {
            self.record_fault(self.sp.wrapping_sub(n), n, Access::Write);
            return 0;
        }
        self.sp -= n;
        let a = self.sp;
        self.fresh(a, n);
        a
    }

    /// A local holding a record.
    pub fn local_record<T: crate::Pod>(&mut self, value: T) -> crate::Ptr32<T> {
        let p = crate::Ptr32::new(self.local(core::mem::size_of::<T>() as u32));
        if !p.is_null() {
            self.write(p, value);
        }
        p
    }

    /// Gives back the most recent [`Mem::local`] of that size.
    pub fn release(&mut self, size: u32) {
        self.sp = self.sp.wrapping_add(size.wrapping_add(3) & !3);
    }

    /// The stack pointer.
    pub fn sp(&self) -> u32 {
        self.sp
    }

    /// Puts the stack pointer back to a value [`Mem::sp`] gave: every local taken since is
    /// released.
    pub fn set_sp(&mut self, sp: u32) {
        self.sp = sp;
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn malloc_is_the_harness_bump_allocator() {
        let mut m = Mem::new();
        // emu.py: a = (top + 7) & ~7; header (size, 0x6d656d21); the block at a + 8
        let a = m.malloc(0x24);
        assert_eq!(a, HEAP.0 + 8);
        assert_eq!((m.u32(HEAP.0), m.u32(HEAP.0 + 4)), (0x24, BLOCK_MARK));
        let b = m.malloc(1);
        assert_eq!(b, HEAP.0 + 0x30 + 8); // 0x2c rounded up to 0x30
        let z = m.malloc(0);
        assert_eq!(z, HEAP.0 + 0x40 + 8); // not null, no bytes
        assert_eq!(m.malloc(0), HEAP.0 + 0x48 + 8);
        assert_eq!(m.heap_blocks()[..2], [(a, 0x24), (b, 1)]);
        assert_eq!(m.heap_block_of(a + 0x10), Some((0, 0x10)));
        assert_eq!(m.heap_block_of(b), Some((1, 0)));
        assert_eq!(m.malloc(HEAP.1 + 1), 0);
        assert_eq!(m.calloc(0x10000, 0x10000), 0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn free_and_realloc() {
        let mut m = Mem::new();
        let a = m.malloc(8);
        m.set_i32(a + 4, 77);
        let b = m.realloc(a, 0x48);
        assert!(b != a && m.i32(b + 4) == 77);
        assert_eq!(m.freed(), &[a]);
        assert_eq!(m.i32(a + 4), 77); // a freed block stays readable, as in the harness
        m.free(0);
        m.free(b);
        assert_eq!(m.freed(), &[a, b]);
        assert!(m.fault().is_none());
        m.free(b); // twice
        assert_eq!(m.fault(), Some(Fault { addr: b, size: 0, access: Access::Free }));
        m.clear_fault();
        m.free(a + 4); // not a block
        assert_eq!(m.fault().map(|f| f.access), Some(Access::Free));
        m.clear_fault();
        assert_eq!(m.realloc(0, 4), b + 0x48 + 8);
        let c = m.malloc(4);
        assert_eq!(m.realloc(c, 0), 0);
        assert!(m.is_freed(c));
        // a test buffer may be freed too
        let t = m.place(16, 8);
        m.free(t);
        assert!(m.fault().is_none());
    }

    #[test]
    fn the_programs_wrappers() {
        let mut m = Mem::new();
        m.set_fill(Fill::Pattern { second: false, salt: 0 });
        assert_eq!(m.si_malloc(0), 0);
        assert!(m.heap_blocks().is_empty());
        let a = m.si_malloc(3);
        assert_eq!((m.u8(a), m.u8(a + 2)), (b'o', b'k'));
        assert_eq!(m.u8(a + 1), m.fill().byte(a + 1)); // not cleared
        let one = m.si_malloc(1);
        assert_eq!(m.u8(one), b'k');
        // a count of 0: calloc is called, its block lost, null returned
        let before = m.heap_blocks().len();
        assert_eq!(m.si_calloc(0, 16), 0);
        assert_eq!(m.heap_blocks().len(), before + 1);
        // a size of 0 is not looked at
        assert!(m.si_calloc(2, 0) != 0);
        let c = m.si_calloc(3, 4);
        assert_eq!(m.bytes(c, 12), &[0; 12]);
        assert_eq!(m.si_realloc(c, 0), c);
        assert!(m.freed().is_empty());
        let d = m.si_realloc(c, 16);
        assert!(d != c && m.freed() == [c]);
        assert_eq!(m.si_realloc(0, 0), 0);
        let s = m.place_cstr(b"obj1");
        let t = m.strdup(s);
        assert_eq!(m.cstr(t), b"obj1");
        assert!(m.fault().is_none());
    }

    #[test]
    fn a_plain_heap_without_headers() {
        let map = Map { arena: (0x6100_0000, 0x0010_0000), heap: (0x6110_0000, 0x0010_0000), heap_header: 0, ..Map::default() };
        let mut m = Mem::with_map(map);
        assert_eq!(m.malloc(0x18), 0x6110_0000);
        assert_eq!(m.malloc(1), 0x6110_0018);
        assert_eq!(m.malloc(0x10_0000), 0);
        assert_eq!(m.malloc(4), 0x6110_0020);
    }

    #[test]
    fn placed_buffers_and_their_gaps() {
        let mut m = Mem::new();
        m.set_fill(Fill::Pattern { second: true, salt: 9 });
        let a = m.place(10, 8);
        let b = m.place(4, 8);
        assert_eq!((a, b), (ARENA.0, ARENA.0 + 0x410)); // 10 + 0x400, rounded up to 8
        assert_eq!(m.u8(a + 10 + 5), m.fill().byte(a + 15)); // the gap holds the fill
        let p: Ptr32<[i16; 2]> = m.place_record([3i16, 4]);
        assert_eq!(m[p], [3, 4]);
        assert_eq!(m.block_size(a), Some(10));
        assert_eq!(m.arena_blocks().len(), 3);
        assert_eq!(m.place(ARENA.1, 8), 0);
    }

    #[test]
    fn locals() {
        let mut m = Mem::new();
        let top = m.sp();
        assert_eq!(top, STACK_START);
        let a = m.local(6);
        assert_eq!(a, top - 8);
        m.set_u32(a, 0x1234_5678);
        let b: Ptr32<i16> = m.local_record(-3i16);
        assert_eq!((b.addr(), m[b]), (top - 12, -3));
        m.release(2);
        m.release(6);
        assert_eq!(m.sp(), top);
        // a new local in the same place does not see the old one's bytes
        let c = m.local(8);
        assert_eq!((c, m.u32(c)), (a, 0));
        m.set_sp(top);
        assert_eq!(m.local(STACK.1), 0);
        assert!(m.fault().is_some());
    }

    // ---- the two policies

    #[test]
    fn reuse_takes_freed_room_and_grows_in_place() {
        let mut m = Mem::with_heap(Map::default(), HeapPolicy::Reuse);
        assert_eq!(m.heap_policy(), HeapPolicy::Reuse);
        let a = m.malloc(0x20);
        let b = m.malloc(0x20);
        let c = m.malloc(0x20);
        // the first blocks lie where the harness's lie, headers included
        assert_eq!((a, b, c), (HEAP.0 + 8, HEAP.0 + 0x30, HEAP.0 + 0x58));
        assert_eq!((m.u32(b - 8), m.u32(b - 4)), (0x20, BLOCK_MARK));
        m.set_u32(b, 0xdead_beef);
        m.free(b);
        assert!(m.is_freed(b) && m.block_size(b).is_none() && m.heap_blocks().is_empty() && m.freed().is_empty());
        assert_eq!(m.u32(b), 0xdead_beef); // its bytes stay until a block takes them
        // freed room is used again, and holds the fill again
        let d = m.malloc(0x18);
        assert_eq!(d, b);
        assert!(!m.is_freed(b));
        assert_eq!(m.bytes(d, 0x18), &[0; 0x18]);
        // the last block grows where it lies; so does one with free room behind it
        m.set_u32(c + 0x1c, 7);
        assert_eq!(m.realloc(c, 0x1000), c);
        assert_eq!((m.u32(c + 0x1c), m.block_size(c)), (7, Some(0x1000)));
        let e = m.malloc(0x40);
        m.free(d);
        assert_eq!(m.realloc(a, 0x30), a); // into the room d left
        assert_eq!(m.heap_stats().reallocs_in_place, 2);
        // a block that cannot grow moves, with its bytes, and takes room to grow into
        m.set_u32(a + 0x2c, 9);
        let a2 = m.realloc(a, 0x100);
        assert!(a2 != a && a2 > e && m.u32(a2 + 0x2c) == 9 && m.is_freed(a));
        let top = m.heap_stats().used;
        assert_eq!(m.realloc(a2, 0x140), a2); // inside the room it took
        assert_eq!(m.heap_stats().used, top);
        assert_eq!(m.bytes(a2 + 0x30, 0x110), &[0u8; 0x110][..]);
        // shrinking gives the rest back; no bytes is a block, twice over
        assert_eq!(m.realloc(a2, 8), a2);
        let (z1, z2) = (m.malloc(0), m.malloc(0));
        assert!(z1 != 0 && z2 != 0 && z1 != z2 && m.block_size(z1) == Some(0));
        assert_eq!(m.heap_verify(), Ok(()));
        assert!(m.fault().is_none());
        // what is no live block
        m.free(z2);
        m.free(z2);
        assert_eq!(m.fault(), Some(Fault { addr: z2, size: 0, access: Access::Free }));
        m.clear_fault();
        assert_eq!(m.realloc(c + 4, 8), 0);
        assert_eq!(m.fault().map(|f| f.access), Some(Access::Free));
        m.clear_fault();
        // everything freed: the heap is empty again and starts over at its start
        for p in [a2, c, e, z1] {
            m.free(p);
        }
        let last = m.malloc(4);
        assert_eq!(m.realloc(last, 0), 0);
        let s = m.heap_stats();
        assert_eq!((s.used, s.live_blocks, s.live_bytes, s.free_bytes), (0, 0, 0, 0));
        assert_eq!(m.malloc(0x20), a);
        assert_eq!(m.heap_verify(), Ok(()));
        // a placed buffer can be freed and reallocated under this policy too
        let t = m.place(16, 8);
        m.set_u32(t + 12, 5);
        let t2 = m.realloc(t, 32);
        assert!(m.region_of(t2) == Some(Region::Heap) && m.u32(t2 + 12) == 5 && m.freed() == [t]);
        assert!(m.fault().is_none());
    }

    #[test]
    fn reuse_under_strict_blocks_and_a_pattern() {
        let mut m = Mem::with_heap(Map::default(), HeapPolicy::Reuse);
        let fill = Fill::Pattern { second: true, salt: 5 };
        m.set_fill(fill);
        m.set_strict(true);
        let a = m.malloc(24);
        let b = m.malloc(24);
        m.set_fill_bytes(a, 0xee, 24);
        m.free(a);
        assert_eq!(m.u8(a), 0); // freed: not memory under strict blocks
        assert!(m.fault().is_some());
        m.clear_fault();
        let c = m.malloc(16);
        assert_eq!(c, a);
        // used room holds the pattern again, not what the freed block held
        assert!((0..16).all(|i| m.u8(c + i) == fill.byte(c + i)));
        assert_eq!(m.u8(c + 16), 0); // past the new block's end
        assert!(m.fault().is_some());
        m.clear_fault();
        let b2 = m.realloc(b, 4000);
        assert!((24..4000).all(|i| m.u8(b2 + i) == fill.byte(b2 + i)));
        assert!(m.fault().is_none());
        m.set_strict(false);
        assert_eq!(m.heap_verify(), Ok(()));
    }

    #[test]
    fn reuse_in_a_small_heap_without_headers() {
        // the user-interface check's kind of map: no header before a block
        let map = Map { arena: (0x6100_0000, 0x0010_0000), heap: (0x6110_0000, 0x1000), heap_header: 0, ..Map::default() };
        let mut m = Mem::with_heap(map, HeapPolicy::Reuse);
        let a = m.malloc(0x800);
        let b = m.malloc(0x7f0);
        let (z1, z2) = (m.malloc(0), m.malloc(0));
        assert_eq!((a, b, z1, z2), (0x6110_0000, 0x6110_0800, 0x6110_0ff0, 0x6110_0ff8));
        // full: null, and a block that cannot grow stays as it is
        assert_eq!((m.malloc(1), m.calloc(0x1000, 0x1000), m.malloc(u32::MAX)), (0, 0, 0));
        m.set_u32(b + 0x7ec, 9);
        assert_eq!(m.realloc(b, 0x900), 0);
        assert_eq!((m.block_size(b), m.u32(b + 0x7ec)), (Some(0x7f0), 9));
        assert!(m.fault().is_none());
        // room given back is room again: b cannot grow where it lies (a block follows it)
        // and moves into what a left, which has no room for half as much again
        m.free(a);
        let b2 = m.realloc(b, 0x7fc);
        assert_eq!((b2, m.u32(b2 + 0x7ec)), (a, 9));
        assert_eq!(m.heap_verify(), Ok(()));
        let s = m.heap_stats();
        assert_eq!((s.used, s.peak, s.live_blocks, s.live_bytes, s.free_bytes), (0x1000, 0x1000, 3, 0x7fc, 0x7f0));
    }

    /// A made-up program: `steps` allocations, reallocations and frees over 48 pointers,
    /// some blocks written whole, some not at all. Answers the pointers at the end.
    fn program(m: &mut Mem, steps: u32) -> Vec<u32> {
        let mut slots = vec![0u32; 48];
        let mut sizes = [0u32; 48];
        let mut x = 12345u32;
        let mut next = || {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            x >> 8
        };
        for step in 0..steps {
            let (i, r) = (next() as usize % 48, next());
            let n = match r % 16 {
                0 => 0,
                1 => 20_000 + next() % 100_000,
                _ => next() % 900,
            };
            if slots[i] == 0 {
                slots[i] = match r % 3 {
                    0 => m.malloc(n),
                    1 => m.calloc(n.div_ceil(4), 4),
                    _ => m.si_realloc(0, n.max(1)),
                };
                sizes[i] = m.block_size(slots[i]).unwrap();
            } else if r % 5 == 0 {
                m.free(slots[i]);
                (slots[i], sizes[i]) = (0, 0);
                continue;
            } else {
                slots[i] = m.realloc(slots[i], n);
                sizes[i] = if n == 0 { 0 } else { n };
            }
            if slots[i] != 0 && step % 2 == 0 {
                m.set_fill_bytes(slots[i], step as u8 | 1, sizes[i]);
            }
            if step % 997 == 0 {
                assert_eq!(m.heap_verify(), Ok(()), "step {step}");
            }
        }
        assert_eq!(m.heap_verify(), Ok(()));
        assert!(m.fault().is_none());
        slots
    }

    #[test]
    fn both_policies_leave_the_same_bytes_in_every_block() {
        let mut harness = Mem::with_heap(Map::wide(), HeapPolicy::Harness);
        let mut reuse = Mem::with_heap(Map::default(), HeapPolicy::Reuse);
        let a = program(&mut harness, 12_000);
        let b = program(&mut reuse, 12_000);
        let mut live = 0;
        for (&p, &q) in a.iter().zip(&b) {
            assert_eq!(p == 0, q == 0);
            if p != 0 {
                let n = harness.block_size(p).unwrap();
                assert_eq!(reuse.block_size(q), Some(n));
                assert!(harness.bytes(p, n) == reuse.bytes(q, n), "a block of {n} bytes differs");
                live += 1;
            }
        }
        let (h, r) = (harness.heap_stats(), reuse.heap_stats());
        assert_eq!((h.live_blocks, h.live_bytes), (r.live_blocks, r.live_bytes));
        assert_eq!((h.live_blocks, h.mallocs, h.frees), (live, r.mallocs, r.frees));
        assert_eq!(h.reallocs_moved, r.reallocs_moved + r.reallocs_in_place);
        assert_eq!(harness.live_blocks().len(), reuse.live_blocks().len());
        // the harness's heap only grows: here far beyond the default 16 MB; the other one
        // stays within a few times what is live
        assert!(h.used > 0x0100_0000 && h.used == h.peak, "{h:?}");
        assert!((r.peak as u64) < 3 * r.live_bytes.max(600_000), "{r:?}");
        assert_eq!(reuse.live_block_of(b.iter().copied().find(|&q| q != 0).unwrap()).map(|x| x.0), b.iter().copied().find(|&q| q != 0));
    }

    #[test]
    fn a_heap_backed_in_steps() {
        // the default heap is reserved whole, as it always was
        let m = Mem::new();
        assert_eq!(m.mapped(Region::Heap), HEAP.1);
        assert_eq!(m.u32(HEAP.0 + HEAP.1 - 4), 0);
        assert!(m.fault().is_none());
        let whole = m.reserved_bytes();

        // a longer one costs nothing until a block is asked for, and then what was reached
        let mut m = Mem::with_map(Map::wide());
        assert_eq!((m.mapped(Region::Heap), m.reserved_bytes()), (0, whole - HEAP.1 as usize));
        assert_eq!(m.region_of(HEAP_WIDE.0 + HEAP_WIDE.1 - 1), Some(Region::Heap));
        assert!(!m.is_mapped(HEAP.0, 4));
        let a = m.malloc(0x24);
        let b = m.malloc(1);
        assert_eq!((a, b), (HEAP.0 + 8, HEAP.0 + 0x30 + 8)); // the harness's addresses
        assert_eq!(m.mapped(Region::Heap), 0x1_0000);
        m.set_u32(a + 0x20, 77);
        // beyond the default heap's end, and still the harness's arithmetic
        let big = m.malloc(0x0180_0000);
        assert_eq!(big, HEAP.0 + 0x40 + 8);
        let after = m.malloc(16);
        assert_eq!(after, big + 0x0180_0000 + 8);
        m.set_u32(after + 12, 5);
        assert_eq!((m.u32(a + 0x20), m.u32(after + 12), m.u32(big + 0x00ff_fff0)), (77, 5, 0));
        assert!(m.mapped(Region::Heap) >= after + 16 - HEAP.0 && m.mapped(Region::Heap) < 0x0400_0000);
        // 24 MB handed out; the host has paid for the pages that were touched
        assert!(m.committed_bytes() <= 5 * 4096, "{}", m.committed_bytes());
        assert!(m.fault().is_none());
        // above what the allocator reached there is no memory yet
        assert_eq!(m.u32(HEAP_WIDE.0 + 0x0c00_0000), 0);
        assert_eq!(m.fault().map(|f| (f.addr, f.access)), Some((HEAP_WIDE.0 + 0x0c00_0000, Access::Read)));
        m.clear_fault();
        // a snapshot and the changes work across a step
        let before = m.snapshot();
        let more = m.malloc(0x0400_0000);
        m.set_u8(more + 0x03ff_ffff, 1);
        assert!(m.changed_in(&before, Region::Heap).contains(&(more + 0x03ff_ffff, vec![1])));
        // the end of the heap answers null
        assert_eq!(m.malloc(HEAP_WIDE.1), 0);
        assert_eq!(m.heap_verify(), Ok(()));
    }

    /// What the scene loader does to the heap, as findings 0026 (section 1.3) and 0018
    /// (section 13.3) describe it: every chapter's element array grows by one record of
    /// 0x84 bytes per element (`SI_malloc(0x84)`, then `SI_realloc(…, (n + 1) × 0x84)`),
    /// the chapters taking turns, and each element brings blocks of its own that stay: a
    /// name record of 0x34 bytes, the three one-character strings `SI_dbListNodeAllocate`
    /// duplicates, a name. Implemented by the image and by a count of what the harness's
    /// allocator would need, which touches no memory.
    trait Loader {
        /// A block of `n` bytes; 0 when there is no room.
        fn block(&mut self, n: u32) -> u32;
        fn grow(&mut self, p: u32, n: u32) -> u32;
        fn put(&mut self, a: u32, v: u32);
    }

    impl Loader for Mem {
        fn block(&mut self, n: u32) -> u32 {
            self.si_malloc(n)
        }
        fn grow(&mut self, p: u32, n: u32) -> u32 {
            self.si_realloc(p, n)
        }
        fn put(&mut self, a: u32, v: u32) {
            self.set_u32(a, v)
        }
    }

    /// The harness's arithmetic without the memory: the top after each block.
    struct Count {
        top: u64,
        limit: u64,
    }

    impl Loader for Count {
        fn block(&mut self, n: u32) -> u32 {
            let p = ((self.top + 7) & !7) + 8;
            if p + n as u64 > self.limit {
                return 0;
            }
            self.top = p + n as u64;
            8
        }
        fn grow(&mut self, _p: u32, n: u32) -> u32 {
            self.block(n)
        }
        fn put(&mut self, _a: u32, _v: u32) {}
    }

    const ELEM: u32 = 0x84;

    /// Loads `chapters[c]` elements into chapter `c`, one element per chapter in turn.
    /// Answers the arrays and how many elements went in before the heap was used up.
    fn load(h: &mut impl Loader, chapters: &[u32], with_names: bool) -> (Vec<u32>, u64) {
        let mut arrays = vec![0u32; chapters.len()];
        let mut counts = vec![0u32; chapters.len()];
        let mut done = 0u64;
        loop {
            let mut any = false;
            for c in 0..chapters.len() {
                let n = counts[c];
                if n == chapters[c] {
                    continue;
                }
                any = true;
                let a = if n == 0 { h.block(ELEM) } else { h.grow(arrays[c], (n + 1) * ELEM) };
                if a == 0 {
                    return (arrays, done);
                }
                arrays[c] = a;
                let e = a + n * ELEM;
                h.put(e, c as u32 * 0x0100_0000 + n);
                if with_names {
                    let node = h.block(0x34);
                    let (s1, s2, s3) = (h.block(1), h.block(2), h.block(2));
                    let name = h.block(6 + (n * 7 + c as u32) % 14);
                    if node == 0 || s1 == 0 || s2 == 0 || s3 == 0 || name == 0 {
                        return (arrays, done);
                    }
                    h.put(e + 4, node);
                    h.put(node + 4, name);
                    h.put(node + 8, n ^ 0x5a5a_5a5a);
                }
                counts[c] = n + 1;
                done += 1;
            }
            if !any {
                return (arrays, done);
            }
        }
    }

    #[test]
    fn the_loaders_pattern_under_each_policy() {
        let mb = |n: u64| n as f64 / 1_048_576.0;
        // 36 chapters, as the chapter table has rows: six of tens of thousands of
        // elements, thirty of a thousand
        let mut chapters = vec![30_000u32, 30_000, 20_000, 20_000, 10_000, 10_000];
        chapters.extend([1_000u32; 30]);
        let total: u64 = chapters.iter().map(|&n| n as u64).sum();

        // the count is the harness's allocator: held against the image where it fits
        let small: Vec<u32> = chapters.iter().map(|&n| n / 100).collect();
        let mut m = Mem::with_map(Map::wide());
        let mut count = Count { top: HEAP.0 as u64, limit: u64::MAX };
        let (_, n1) = load(&mut m, &small, true);
        let (_, n2) = load(&mut count, &small, true);
        assert_eq!((n1, m.heap_stats().used as u64), (n2, count.top - HEAP.0 as u64));
        assert_eq!(m.heap_verify(), Ok(()));
        println!("1% of the pattern ({n1} elements), harness policy on the image: {:.1} MB of heap, {:.1} MB touched", mb(m.heap_stats().used as u64), mb(m.committed_bytes() as u64));
        drop(m);

        // the harness's policy: what the whole pattern would need, and how far each heap gets
        let mut count = Count { top: 0, limit: u64::MAX };
        load(&mut count, &chapters, true);
        println!("the loader's pattern: 36 chapters, {total} elements, one record at a time in turn");
        println!("  harness policy, counted: {:.0} MB of heap needed (every byte of it written)", mb(count.top));
        assert!(count.top > 100 << 30);
        for (name, size) in [("16 MB (default)", HEAP.1), ("208 MB (HEAP_WIDE)", HEAP_WIDE.1), ("1280 MB (HEAP_PROGRAM)", HEAP_PROGRAM.1)] {
            let mut count = Count { top: 0, limit: size as u64 };
            let (_, done) = load(&mut count, &chapters, true);
            println!("  harness policy, a heap of {name}: used up after {done} of {total} elements");
            assert!(done < total / 4);
        }

        // the other policy, on the image
        let t = std::time::Instant::now();
        let mut m = Mem::program();
        let (arrays, done) = load(&mut m, &chapters, true);
        let s = m.heap_stats();
        println!(
            "  reuse policy, on the image: all {done} elements; heap needed {:.1} MB for {:.1} MB live in {} blocks; {:.1} MB touched, {:.1} MB of the host's address space; {} of {} reallocs moved a block ({:.1} MB copied); {:.2} s",
            mb(s.peak as u64),
            mb(s.live_bytes),
            s.live_blocks,
            mb(m.committed_bytes() as u64),
            mb(m.reserved_bytes() as u64),
            s.reallocs_moved,
            s.reallocs_moved + s.reallocs_in_place,
            mb(s.bytes_moved),
            t.elapsed().as_secs_f64()
        );
        assert_eq!(done, total);
        assert_eq!(m.heap_verify(), Ok(()));
        assert!(m.fault().is_none());
        // nothing was lost on the way: every element, its name record and its name
        for (c, &a) in arrays.iter().enumerate() {
            assert_eq!(m.block_size(a), Some(chapters[c] * ELEM));
            for n in 0..chapters[c] {
                let e = a + n * ELEM;
                let node = m.u32(e + 4);
                assert_eq!((m.u32(e), m.u32(node + 8), m.block_size(node)), (c as u32 * 0x0100_0000 + n, n ^ 0x5a5a_5a5a, Some(0x34)));
                assert_eq!(m.block_size(m.u32(node + 4)), Some(6 + (n * 7 + c as u32) % 14));
            }
        }
        // within three times what is live, and few moves
        assert!((s.peak as u64) < 3 * s.live_bytes, "{s:?}");
        assert!(s.reallocs_moved * 50 < s.reallocs_in_place, "{s:?}");

        // the corpus's largest chapter (finding 0018, 13.3): 2,593 elements, the element
        // array alone
        let mut count = Count { top: 0, limit: u64::MAX };
        load(&mut count, &[2_593], false);
        let mut m = Mem::with_heap(Map::default(), HeapPolicy::Reuse);
        load(&mut m, &[2_593], false);
        let s = m.heap_stats();
        println!(
            "one chapter of 2593 elements, the array alone: harness policy {} bytes (counted; the 444 MB of finding 0018, {:.0} MB in units of 2^20); reuse policy {:.2} MB at most, {:.2} MB at the end",
            count.top,
            mb(count.top),
            mb(s.peak as u64),
            mb(s.used as u64)
        );
        assert_eq!((count.top + 500_000) / 1_000_000, 444);
        assert!(s.peak < 0x0010_0000);
    }
}
