// SPDX-License-Identifier: MIT
//! The chunk allocator: the original's `mem_chunk.c` (0x0857ca00 to 0x0857cdb3, ten
//! functions; finding 0012, "Chunk memory"). It is not a renderer source file, but the
//! renderer is its main user: everything a frame needs (line buffers, triangles, slices, the
//! BSP tree, secondary rays) comes from one buffer, `SI_renChunk` (0x0869bc84), which
//! `SI_matterInit` makes with `SI_chunkAllocate(200, 5000000)`: up to 200 blocks of 5 MB (or
//! of the size asked, if that is larger). Nothing is ever given back singly: a caller saves
//! the fill pointer (`SI_chunkSaveCurrent`), allocates, and later moves the fill pointer back
//! (`SI_chunkReclaim`); `SI_chunkFreeAll` frees every block.
//!
//! The functions work on addresses, so the port does too: a [`ChunkBuffer`] holds the
//! record's members with their original widths, and the C library's heap comes in through
//! [`ChunkHeap`]. Addresses are compared exactly as the instructions compare them (signed in
//! `SI_chunkMalloc`, unsigned in `SI_chunkReclaim`).
//!
//! Class of every function here: copy (integers only).

/// The program's heap, as `mem_chunk.c` uses it. An address of 0 is the null pointer.
pub trait ChunkHeap {
    /// `SI_calloc(size, 1)` (0x080d0f50, not part of this port): `size` zeroed bytes. It
    /// calls the C library's `calloc` whatever `size` is and returns null when `size` is 0.
    fn calloc(&mut self, size: u32) -> u32;
    /// `SI_malloc(size)` (0x080d0ee8, not part of this port): null for a size of 0 (the C
    /// library is not asked) or when the C library fails; otherwise the block, whose first
    /// byte it sets to `o` and whose last to `k`.
    fn malloc(&mut self, size: u32) -> u32;
    /// The C library's `free`.
    fn free(&mut self, addr: u32);
    /// Writes one byte of a block.
    fn poke(&mut self, addr: u32, byte: u8);
}

/// One block of a chunk buffer: 12 bytes at +0x34 + 12 × index of the record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkSlot {
    /// +0x00: the block's size in bytes, as it was asked of `SI_malloc`.
    pub size: i32,
    /// +0x04: the block's address rounded up to a multiple of four: where allocation starts.
    pub ptr: u32,
    /// +0x08: the block as `SI_malloc` returned it (what `free` is given).
    pub raw: u32,
}

/// `SI_ChunkBuffer`: 0x34 bytes followed by the slots (the record is allocated with one
/// slot's worth of spare room after them).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkBuffer {
    /// The record's own address (what `SI_chunkDestroy` frees). Not a member.
    pub addr: u32,
    /// +0x00: index of the slot being filled.
    pub current: i32,
    /// +0x04: 1 while `chunkAllocSlot` is inside `SI_malloc` or writing the guard bytes.
    pub busy: i16,
    /// +0x08: start of the slot being filled.
    pub base: u32,
    /// +0x0c: its last byte: start + size − 1.
    pub end: u32,
    /// +0x10: the fill pointer: where the next allocation starts.
    pub next: u32,
    /// +0x14: number of saved fill pointers.
    pub marks: i32,
    /// +0x18: the saved fill pointers, five at most.
    pub mark: [u32; 5],
    /// +0x2c: the least size of a block, a multiple of four.
    pub slot_size: i32,
    /// +0x30: number of slots.
    pub slot_count: i32,
    /// +0x34: the slots.
    pub slots: Vec<ChunkSlot>,
}

/// Rounds a size up to a multiple of four [soft 0x0857cd7c padSize]: `padSize(&size)`.
pub fn pad_size(size: i32) -> i32 {
    let r = size & 3;
    if r != 0 {
        size.wrapping_add(4).wrapping_sub(r)
    } else {
        size
    }
}

/// Steps an address up to a multiple of four, one byte at a time
/// [soft 0x0857cd98 alignPointer]: `alignPointer(&pointer)`.
pub fn align_pointer(p: u32) -> u32 {
    let mut p = p;
    while p & 3 != 0 {
        p = p.wrapping_add(1);
    }
    p
}

/// A new, empty chunk buffer of `count` slots [soft 0x0857ca00 SI_chunkAllocate]:
/// `SI_chunkAllocate(count, size)`. The record is `count × 12 + 0x40` zeroed bytes; `size`,
/// rounded up to a multiple of four, becomes the least block size. `None` when the record
/// cannot be had.
pub fn chunk_allocate(heap: &mut dyn ChunkHeap, count: i32, size: i32) -> Option<ChunkBuffer> {
    let addr = heap.calloc(count.wrapping_mul(12).wrapping_add(0x40) as u32);
    if addr == 0 {
        return None;
    }
    let mut buf = ChunkBuffer { addr, slot_size: pad_size(size), slot_count: count, slots: vec![ChunkSlot::default(); count.max(0) as usize], ..ChunkBuffer::default() };
    buf.initialize();
    Some(buf)
}

/// Frees a chunk buffer and its blocks [soft 0x0857ca5c SI_chunkDestroy]:
/// `SI_chunkDestroy(&buffer)`. The original calls `SI_chunkFreeAll` on the pointer before it
/// tests it for null, so a null buffer is a fault there; here it is a panic.
pub fn chunk_destroy(heap: &mut dyn ChunkHeap, buf: &mut Option<ChunkBuffer>) {
    let b = buf.as_mut().expect("SI_chunkDestroy of a null buffer reads through the null pointer");
    b.free_all(heap);
    heap.free(b.addr);
    *buf = None;
}

impl ChunkBuffer {
    /// Empties the record [soft 0x0857ca90 SI_chunkInitialize]: the fill state, every slot
    /// and the five saved fill pointers are zeroed; the least block size and the slot count
    /// stay. **Nothing is freed**: blocks still held are forgotten. (`SI_initialize` calls
    /// this at the start of every render; `SI_image` calls `SI_chunkFreeAll` at the end of
    /// one, so nothing is lost as long as a render ends through `SI_image`'s own exits.)
    pub fn initialize(&mut self) {
        self.busy = 0;
        self.base = 0;
        self.end = 0;
        self.next = 0;
        self.current = 0;
        self.marks = 0;
        // the original loops while index < the count member, over memory: a record whose
        // count is larger than its room would be overrun. Here: the slots there are.
        for s in self.slots.iter_mut().take(self.slot_count.max(0) as usize) {
            *s = ChunkSlot::default();
        }
        self.mark = [0; 5];
    }

    /// Frees every block and empties the record [soft 0x0857cb00 SI_chunkFreeAll].
    pub fn free_all(&mut self, heap: &mut dyn ChunkHeap) {
        for s in self.slots.iter_mut().take(self.slot_count.max(0) as usize) {
            if s.raw != 0 {
                heap.free(s.raw);
                s.raw = 0;
            }
            s.ptr = 0;
        }
        self.initialize();
    }

    /// `size` bytes of chunk memory [soft 0x0857cb64 SI_chunkMalloc]: `SI_chunkMalloc(buffer,
    /// size)`; 0 when there is none.
    ///
    /// The size is rounded up to a multiple of four. With no first block yet, slot 0 gets
    /// one. If fill pointer + size is not above the slot's last byte (a **signed**
    /// comparison of the two addresses), the fill pointer is the result and moves up.
    /// Otherwise the slots after the current one are tried in order: an empty slot gets a new
    /// block (of at least the size asked) and is taken; a slot that has a block is taken if
    /// its size is not below the size asked, else it is passed over for good. A slot taken
    /// this way is filled from its start, **without the test against its last byte**.
    ///
    /// Consequences: a block's last byte is never handed out by the first path (the test
    /// compares with size − 1), so a request as large as the least block size, made of an
    /// empty buffer, leaves slot 0 unused and takes slot 1; and what is left of a slot when
    /// a request does not fit is lost until the fill pointer is moved back.
    pub fn malloc(&mut self, heap: &mut dyn ChunkHeap, size: i32) -> u32 {
        let size = pad_size(size);
        if self.slots.first().map_or(0, |s| s.ptr) == 0 {
            if !self.alloc_slot(heap, 0, size) {
                return 0;
            }
            let s = self.slots[0];
            self.base = s.ptr;
            self.end = (s.size.wrapping_sub(1) as u32).wrapping_add(s.ptr);
            self.next = s.ptr;
        }
        let at;
        if (size as u32).wrapping_add(self.next) as i32 <= self.end as i32 {
            at = self.next;
        } else {
            let mut i = self.current.wrapping_add(1);
            let mut found = None;
            while i < self.slot_count {
                let k = i as usize;
                if self.slots[k].ptr == 0 {
                    if !self.alloc_slot(heap, i, size) {
                        return 0;
                    }
                } else if self.slots[k].size < size {
                    i += 1;
                    continue;
                }
                self.current = i;
                self.base = self.slots[k].ptr;
                self.end = (self.slots[k].size.wrapping_sub(1) as u32).wrapping_add(self.slots[k].ptr);
                found = Some(self.slots[k].ptr);
                break;
            }
            if i == self.slot_count {
                return 0;
            }
            // `found` is None only when the current slot's index was already past the count:
            // the original then uses a stack word it never set. Here: null.
            match found {
                Some(p) => at = p,
                None => return 0,
            }
        }
        self.next = at.wrapping_add(size as u32);
        at
    }

    /// Gives slot `slot` a block [soft 0x0857cc3c chunkAllocSlot]: `chunkAllocSlot(buffer,
    /// slot, size)`. The block has the larger of `size` and the least block size (signed);
    /// its first byte is set to `O` (0x4f) and its last to `K` (0x4b). The slot records the
    /// size, the block and the block's address rounded up to four. (The size recorded is the
    /// whole block's even when rounding moved the start: on a heap that returned an address
    /// that is not a multiple of four the slot's last byte would lie up to three bytes past
    /// the block. The C library's blocks are aligned to eight.) False when `SI_malloc`
    /// returns null.
    pub fn alloc_slot(&mut self, heap: &mut dyn ChunkHeap, slot: i32, size: i32) -> bool {
        let size = if size < self.slot_size { self.slot_size } else { size };
        self.busy = 1;
        let p = heap.malloc(size as u32);
        self.busy = 0;
        if p == 0 {
            return false;
        }
        self.busy = 1;
        heap.poke(p, 0x4f);
        heap.poke(p.wrapping_add(size as u32).wrapping_sub(1), 0x4b);
        self.busy = 0;
        // the original writes the slot whatever its index: the callers pass 0 or an index
        // below the count
        let s = &mut self.slots[slot as usize];
        s.raw = p;
        s.ptr = align_pointer(p);
        s.size = size;
        true
    }

    /// Saves the fill pointer [soft 0x0857ccc4 SI_chunkSaveCurrent]:
    /// `SI_chunkSaveCurrent(buffer, &mark)`. Returns 0 when five are saved already;
    /// otherwise the mark's number goes through `mark` (when given), and 1 is returned. A
    /// mark taken before the first allocation holds a null fill pointer, which
    /// `SI_chunkReclaim` refuses.
    pub fn save_current(&mut self, mark: Option<&mut i32>) -> i32 {
        if self.marks > 4 {
            return 0;
        }
        if let Some(m) = mark {
            *m = self.marks;
        }
        // an index below 0 would write before the table in the original; the count is only
        // ever 0 to 5
        self.mark[self.marks as usize] = self.next;
        self.marks += 1;
        1
    }

    /// Moves the fill pointer back to a saved one [soft 0x0857cd00 SI_chunkReclaim]:
    /// `SI_chunkReclaim(buffer, mark)`. Refused (0) when the mark is negative or above the
    /// number saved, or its fill pointer is null, or no slot from the current one down to 0
    /// holds that address (unsigned comparisons against the slot's start and last byte). On
    /// success that slot becomes the current one, the number of saved marks becomes `mark`
    /// (so the mark itself and every later one are dropped), and 1 is returned. Blocks stay
    /// allocated; `SI_chunkMalloc` reuses them.
    ///
    /// The mark may equal the number saved: the entry read is then one that was dropped
    /// earlier or never set, and with five saved it is the word after the table, the least
    /// block size. An empty slot (start 0, size 0) "holds" every address, its last byte
    /// being 0 − 1 unsigned: so on a buffer that has allocated nothing, with five marks
    /// saved, reclaiming mark 5 succeeds and leaves the fill pointer at the least block size
    /// (the next allocation sets slot 0 up and overwrites that).
    pub fn reclaim(&mut self, mark: i32) -> i32 {
        if self.marks < 0 || mark < 0 || mark > self.marks {
            return 0;
        }
        // the table is indexed without a look at its length: past its five entries lie the
        // least block size, the slot count and the slots. (The number of saved marks only
        // passes 5 in a record something else has damaged.)
        let p = match mark {
            0..=4 => self.mark[mark as usize],
            5 => self.slot_size as u32,
            6 => self.slot_count as u32,
            _ => match self.slots.get((mark as usize - 7) / 3) {
                Some(s) => [s.size as u32, s.ptr, s.raw][(mark as usize - 7) % 3],
                // beyond the record: whatever follows it on the heap
                None => return 0,
            },
        };
        if p == 0 {
            return 0;
        }
        let mut i = self.current;
        while i >= 0 {
            // a current index past the slots would read beyond the record in the original
            let Some(s) = self.slots.get(i as usize) else { return 0 };
            let hi = (s.size.wrapping_sub(1) as u32).wrapping_add(s.ptr);
            if s.ptr <= p && p <= hi {
                self.end = hi;
                self.base = s.ptr;
                self.next = p;
                self.current = i;
                self.marks = mark;
                return 1;
            }
            i -= 1;
        }
        0
    }
}

/// A heap for callers that only need the allocator's arithmetic (tests, and a renderer whose
/// data lives in Rust collections): blocks are numbered upwards from `top` at multiples of
/// eight and never reused; the bytes are not kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountingHeap {
    /// Where the next block goes.
    pub top: u32,
    /// The address no block may reach.
    pub limit: u32,
    /// Blocks handed out and not freed: (address, size).
    pub live: Vec<(u32, u32)>,
    /// Guard bytes written: (address, byte).
    pub poked: Vec<(u32, u8)>,
}

impl CountingHeap {
    pub fn new(top: u32, limit: u32) -> CountingHeap {
        CountingHeap { top, limit, live: Vec::new(), poked: Vec::new() }
    }

    fn block(&mut self, size: u32) -> u32 {
        let a = self.top.wrapping_add(7) & !7;
        if a < self.top || size > self.limit.saturating_sub(a) {
            return 0;
        }
        self.top = a + size;
        self.live.push((a, size));
        a
    }
}

impl ChunkHeap for CountingHeap {
    fn calloc(&mut self, size: u32) -> u32 {
        let a = self.block(size);
        if size == 0 {
            0
        } else {
            a
        }
    }
    fn malloc(&mut self, size: u32) -> u32 {
        if size == 0 {
            return 0;
        }
        self.block(size)
    }
    fn free(&mut self, addr: u32) {
        self.live.retain(|b| b.0 != addr);
    }
    fn poke(&mut self, addr: u32, byte: u8) {
        self.poked.push((addr, byte));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_addresses_round_up_to_four() {
        assert_eq!([pad_size(0), pad_size(1), pad_size(4), pad_size(5), pad_size(7), pad_size(8)], [0, 4, 4, 8, 8, 8]);
        assert_eq!(pad_size(-1), 0);
        assert_eq!(pad_size(-5), -4);
        assert_eq!([align_pointer(0x1000), align_pointer(0x1001), align_pointer(0x1003)], [0x1000, 0x1004, 0x1004]);
        assert_eq!(align_pointer(0xffff_fffd), 0);
    }

    #[test]
    fn a_buffer_fills_slot_by_slot() {
        let mut heap = CountingHeap::new(0x1000, 0x10_0000);
        let mut b = chunk_allocate(&mut heap, 3, 62).unwrap();
        assert_eq!((b.addr, b.slot_size, b.slot_count), (0x1000, 64, 3));
        // the record is count × 12 + 0x40 bytes
        assert_eq!(heap.live, vec![(0x1000, 3 * 12 + 0x40)]);
        // the first request makes slot 0: 64 bytes at the next multiple of eight
        let p0 = b.malloc(&mut heap, 10);
        let s0 = b.slots[0];
        assert_eq!((p0, s0.ptr, s0.raw, s0.size), (0x1068, 0x1068, 0x1068, 64));
        assert_eq!((b.base, b.end, b.next), (0x1068, 0x1068 + 63, 0x1068 + 12));
        assert_eq!(heap.poked, vec![(0x1068, 0x4f), (0x1068 + 63, 0x4b)]);
        // 12 + 48 = 60 ≤ 63 fits; then 4 more would end at 64 > 63: the last word of a block
        // is never handed out by the fitting test
        assert_eq!(b.malloc(&mut heap, 48), 0x1068 + 12);
        let p2 = b.malloc(&mut heap, 4);
        assert_eq!((p2, b.current), (b.slots[1].ptr, 1));
        // a request above the least size gets a block of its own size, used whole
        let big = b.malloc(&mut heap, 100);
        assert_eq!((big, b.current, b.slots[2].size, b.next), (b.slots[2].ptr, 2, 100, b.slots[2].ptr + 100));
        // no slot left
        assert_eq!(b.malloc(&mut heap, 4), 0);
        // a request of exactly the least size leaves slot 0 of a fresh buffer unused
        let mut c = chunk_allocate(&mut heap, 2, 64).unwrap();
        let p = c.malloc(&mut heap, 64);
        assert_eq!((c.current, p), (1, c.slots[1].ptr));
        assert_ne!(c.slots[0].ptr, 0);
    }

    #[test]
    fn marks_move_the_fill_pointer_back() {
        let mut heap = CountingHeap::new(0x2000, 0x10_0000);
        let mut b = chunk_allocate(&mut heap, 3, 64).unwrap();
        // a mark before anything is allocated holds null and cannot be reclaimed
        let mut m = -1;
        assert_eq!(b.save_current(Some(&mut m)), 1);
        assert_eq!((m, b.marks, b.mark[0]), (0, 1, 0));
        assert_eq!(b.reclaim(0), 0);
        let a = b.malloc(&mut heap, 8);
        assert_eq!(b.save_current(Some(&mut m)), 1);
        assert_eq!((m, b.mark[1]), (1, a + 8));
        b.malloc(&mut heap, 40);
        b.malloc(&mut heap, 40);
        assert_eq!(b.current, 1);
        // back into slot 0; the mark itself is dropped, slot 1 keeps its block
        assert_eq!(b.reclaim(1), 1);
        assert_eq!((b.current, b.next, b.marks, b.base, b.end), (0, a + 8, 1, a, a + 63));
        let blocks = heap.live.len();
        b.malloc(&mut heap, 40);
        // slot 1 is reused from its start: no new block
        assert_eq!(b.malloc(&mut heap, 40), b.slots[1].ptr);
        assert_eq!(heap.live.len(), blocks);
        // an existing block too small for the request is passed over
        assert_eq!(b.reclaim(1), 1);
        b.malloc(&mut heap, 40);
        let p = b.malloc(&mut heap, 200);
        assert_eq!((b.current, p), (2, b.slots[2].ptr));
        // five marks at most; the null `mark` argument is allowed
        b.marks = 0;
        for _ in 0..5 {
            assert_eq!(b.save_current(None), 1);
        }
        assert_eq!(b.save_current(Some(&mut m)), 0);
        // out of range marks
        assert_eq!((b.reclaim(-1), b.reclaim(6)), (0, 0));
        // mark 5 with five saved reads the least block size as an address
        assert_eq!(b.reclaim(5), 0);
        // on an empty buffer that succeeds: the empty slot 0 spans every address
        let mut e = chunk_allocate(&mut heap, 2, 64).unwrap();
        for _ in 0..5 {
            e.save_current(None);
        }
        assert_eq!(e.reclaim(5), 1);
        assert_eq!((e.base, e.end, e.next, e.marks), (0, u32::MAX, 64, 5));
        let p = e.malloc(&mut heap, 8);
        assert_eq!((p, e.base, e.next), (e.slots[0].ptr, p, p + 8));
    }

    #[test]
    fn free_all_and_destroy() {
        let mut heap = CountingHeap::new(0x3000, 0x10_0000);
        let mut b = chunk_allocate(&mut heap, 2, 32).unwrap();
        b.malloc(&mut heap, 8);
        b.malloc(&mut heap, 28);
        assert_eq!(heap.live.len(), 3);
        // initialising forgets the blocks without freeing them
        let mut forgotten = b.clone();
        forgotten.initialize();
        assert_eq!(forgotten.slots, vec![ChunkSlot::default(); 2]);
        assert_eq!(heap.live.len(), 3);
        b.free_all(&mut heap);
        assert_eq!(heap.live, vec![(0x3000, 2 * 12 + 0x40)]);
        assert_eq!((b.next, b.current, b.slot_size, b.slot_count), (0, 0, 32, 2));
        let mut ob = Some(b);
        chunk_destroy(&mut heap, &mut ob);
        assert!(ob.is_none() && heap.live.is_empty());
        // the record cannot be had
        assert!(chunk_allocate(&mut CountingHeap::new(0, 16), 1, 8).is_none());
        // a failing block allocation
        let mut small = CountingHeap::new(8, 0x68);
        let mut b = chunk_allocate(&mut small, 1, 64).unwrap();
        assert_eq!(b.malloc(&mut small, 4), 0);
        assert_eq!(b.busy, 0);
    }
}
