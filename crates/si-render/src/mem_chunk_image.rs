// SPDX-License-Identifier: MIT
//! The chunk allocator on the memory image: `mem_chunk.c` (0x0857ca00, ten functions) as
//! [`crate::mem_chunk`] has it on a record of its own, here on the `SI_ChunkBuffer` in the
//! image (finding 0012, sections 8a and 19.2). The renderer's arena, `SI_renChunk`
//! (0x0869bc84), is such a record; with these the render commands can call
//! `SI_chunkFreeAll(SI_renChunk)` by address, and `SI_matterInit`'s
//! `SI_chunkAllocate(200, 5000000)` makes the record a loaded scene's image lacks.
//!
//! The record: +0x00 the slot being filled, +0x04 a 16-bit word that is 1 while
//! `chunkAllocSlot` is inside `SI_malloc` or writes the guard bytes, +0x08 the slot's
//! start, +0x0c its last byte, +0x10 the fill pointer, +0x14 the number of saved fill
//! pointers, +0x18 five of them, +0x2c the least block size, +0x30 the slot count, +0x34
//! the slots of 12 bytes: size, start (rounded up to four), block (as `SI_malloc` gave it).
//!
//! `SI_malloc`, `SI_calloc` and `free` are the image's (`si-mem`). Addresses are compared
//! as the instructions compare them: signed in `SI_chunkMalloc`, unsigned in
//! `SI_chunkReclaim`. Class of every function here: copy.

use si_mem::Mem;

/// Offsets into `SI_ChunkBuffer`.
pub mod off {
    pub const CURRENT: u32 = 0x00;
    pub const BUSY: u32 = 0x04;
    pub const BASE: u32 = 0x08;
    pub const END: u32 = 0x0c;
    pub const NEXT: u32 = 0x10;
    pub const MARKS: u32 = 0x14;
    pub const MARK: u32 = 0x18;
    pub const SLOT_SIZE: u32 = 0x2c;
    pub const SLOT_COUNT: u32 = 0x30;
    /// The slots; each [`SLOT`] bytes: +0 size, +4 start, +8 block.
    pub const SLOTS: u32 = 0x34;
    pub const SLOT: u32 = 0x0c;
}

/// `SI_renChunk` (0x0869bc84): the renderer's chunk buffer.
pub const SI_REN_CHUNK: u32 = 0x0869_bc84;
/// What `SI_matterInit` makes it with: 200 blocks of at least 5,000,000 bytes.
pub const REN_CHUNK_SLOTS: i32 = 200;
pub const REN_CHUNK_BLOCK: i32 = 5_000_000;

/// [soft 0x0857cd7c padSize]: `padSize(&size)`: the 32-bit word at `p` rounded up to a
/// multiple of four (left alone when it is one).
pub fn pad_size(m: &mut Mem, p: u32) {
    let v = m.u32(p);
    let r = v & 3;
    if r != 0 {
        m.set_u32(p, v.wrapping_add(4).wrapping_sub(r));
    }
}

/// [soft 0x0857cd98 alignPointer]: `alignPointer(&pointer)`: the pointer at `p` stepped up
/// to a multiple of four, one at a time (left alone when it is one).
pub fn align_pointer(m: &mut Mem, p: u32) {
    let mut v = m.u32(p);
    if v & 3 != 0 {
        while v & 3 != 0 {
            v = v.wrapping_add(1);
        }
        m.set_u32(p, v);
    }
}

/// [soft 0x0857ca00 SI_chunkAllocate]: `SI_chunkAllocate(count, size)`: a record of
/// `count × 12 + 0x40` zeroed bytes (`SI_calloc(that, 1)`), null when that fails; the least
/// block size (`size` rounded up to four) and the slot count are stored and the record
/// initialised.
pub fn chunk_allocate(m: &mut Mem, count: i32, size: i32) -> u32 {
    let buf = m.si_calloc(count.wrapping_mul(12).wrapping_add(0x40) as u32, 1);
    if buf == 0 {
        return 0;
    }
    let sp = m.sp();
    let arg = m.local(4);
    m.set_i32(arg, size);
    pad_size(m, arg);
    let size = m.i32(arg);
    m.set_sp(sp);
    m.set_i32(buf + off::SLOT_SIZE, size);
    m.set_i32(buf + off::SLOT_COUNT, count);
    chunk_initialize(m, buf);
    buf
}

/// [soft 0x0857ca5c SI_chunkDestroy]: `SI_chunkDestroy(&buffer)`: `SI_chunkFreeAll` on the
/// pointer **before it is tested** (a null buffer is a read through null: a recorded fault
/// here), then the record freed and the pointer cleared.
pub fn chunk_destroy(m: &mut Mem, pp: u32) {
    let buf = m.u32(pp);
    chunk_free_all(m, buf);
    let buf = m.u32(pp);
    if buf != 0 {
        m.free(buf);
        m.set_u32(pp, 0);
    }
}

/// [soft 0x0857ca90 SI_chunkInitialize]: `SI_chunkInitialize(buffer)`: the busy word, the
/// slot's start and last byte, the fill pointer, the current slot and the number of marks
/// are cleared, then every slot up to the count the record holds, then the five marks from
/// the last down. Nothing is freed.
pub fn chunk_initialize(m: &mut Mem, buf: u32) {
    m.set_u16(buf + off::BUSY, 0);
    m.set_u32(buf + off::BASE, 0);
    m.set_u32(buf + off::END, 0);
    m.set_u32(buf + off::NEXT, 0);
    m.set_u32(buf + off::CURRENT, 0);
    m.set_u32(buf + off::MARKS, 0);
    let mut i: i32 = 0;
    while i < m.i32(buf + off::SLOT_COUNT) {
        let s = buf.wrapping_add(off::SLOTS).wrapping_add((i as u32).wrapping_mul(off::SLOT));
        m.set_u32(s, 0);
        m.set_u32(s.wrapping_add(4), 0);
        m.set_u32(s.wrapping_add(8), 0);
        i += 1;
    }
    for k in (0..5u32).rev() {
        m.set_u32(buf + off::MARK + 4 * k, 0);
    }
}

/// [soft 0x0857cb00 SI_chunkFreeAll]: `SI_chunkFreeAll(buffer)`: every slot's block freed
/// (and its two pointers cleared), then [`chunk_initialize`].
pub fn chunk_free_all(m: &mut Mem, buf: u32) {
    let mut i: i32 = 0;
    while i < m.i32(buf.wrapping_add(off::SLOT_COUNT)) {
        let s = buf.wrapping_add(off::SLOTS).wrapping_add((i as u32).wrapping_mul(off::SLOT));
        let raw = m.u32(s.wrapping_add(8));
        if raw != 0 {
            m.free(raw);
            m.set_u32(s.wrapping_add(8), 0);
        }
        m.set_u32(s.wrapping_add(4), 0);
        i += 1;
    }
    chunk_initialize(m, buf);
}

/// [soft 0x0857cc3c chunkAllocSlot]: `chunkAllocSlot(buffer, slot, size)`: a block of the
/// larger of `size` and the least block size (signed) from `SI_malloc`, the busy word 1
/// around the call and again around the two guard bytes (`O` first, `K` last); the slot
/// gets the block, its address rounded up to four, and the size. 0 when there is no block.
pub fn chunk_alloc_slot(m: &mut Mem, buf: u32, slot: i32, size: i32) -> u32 {
    let mut size = size;
    if size < m.i32(buf + off::SLOT_SIZE) {
        size = m.i32(buf + off::SLOT_SIZE);
    }
    m.set_u16(buf + off::BUSY, 1);
    let p = m.si_malloc(size as u32);
    m.set_u16(buf + off::BUSY, 0);
    if p == 0 {
        return 0;
    }
    m.set_u16(buf + off::BUSY, 1);
    m.set_u8(p, 0x4f);
    m.set_u8(p.wrapping_add(size as u32).wrapping_sub(1), 0x4b);
    m.set_u16(buf + off::BUSY, 0);
    let s = buf.wrapping_add(off::SLOTS).wrapping_add((slot as u32).wrapping_mul(off::SLOT));
    m.set_u32(s.wrapping_add(8), p);
    let sp = m.sp();
    let local = m.local(4);
    m.set_u32(local, p);
    align_pointer(m, local);
    let aligned = m.u32(local);
    m.set_sp(sp);
    m.set_u32(s.wrapping_add(4), aligned);
    m.set_i32(s, size);
    1
}

/// [soft 0x0857cb64 SI_chunkMalloc]: `SI_chunkMalloc(buffer, size)`; null when there is no
/// room. See [`crate::mem_chunk::ChunkBuffer::malloc`] for the rule and its consequences;
/// this is the same on the record in the image.
///
/// One path reads a local the original never set: when the fill pointer does not fit and
/// the slot after the current one is already past the count without being equal to it.
/// The original answers whatever its stack held; here the word of the image's stack.
pub fn chunk_malloc(m: &mut Mem, buf: u32, size: i32) -> u32 {
    let sp = m.sp();
    let arg = m.local(4);
    let found = m.local(4);
    m.set_i32(arg, size);
    pad_size(m, arg);
    let size = m.i32(arg);
    let r = (|| {
        if m.u32(buf + off::SLOTS + 4) == 0 {
            if chunk_alloc_slot(m, buf, 0, size) == 0 {
                return 0;
            }
            let start = m.u32(buf + off::SLOTS + 4);
            m.set_u32(buf + off::BASE, start);
            let end = (m.i32(buf + off::SLOTS).wrapping_sub(1) as u32).wrapping_add(start);
            m.set_u32(buf + off::END, end);
            m.set_u32(buf + off::NEXT, start);
        }
        let mut at = m.u32(buf + off::NEXT);
        if m.i32(buf + off::END) < (size as u32).wrapping_add(m.u32(buf + off::NEXT)) as i32 {
            let mut i = m.i32(buf + off::CURRENT).wrapping_add(1);
            while i < m.i32(buf + off::SLOT_COUNT) {
                let s = buf.wrapping_add(off::SLOTS).wrapping_add((i as u32).wrapping_mul(off::SLOT));
                let take = if m.u32(s.wrapping_add(4)) == 0 {
                    if chunk_alloc_slot(m, buf, i, size) == 0 {
                        return 0;
                    }
                    true
                } else {
                    size <= m.i32(s)
                };
                if take {
                    m.set_i32(buf + off::CURRENT, i);
                    let start = m.u32(s.wrapping_add(4));
                    m.set_u32(found, start);
                    m.set_u32(buf + off::BASE, start);
                    let end = (m.i32(s).wrapping_sub(1) as u32).wrapping_add(start);
                    m.set_u32(buf + off::END, end);
                    break;
                }
                i = i.wrapping_add(1);
            }
            at = m.u32(found);
            if i == m.i32(buf + off::SLOT_COUNT) {
                return 0;
            }
        }
        m.set_u32(buf + off::NEXT, at.wrapping_add(size as u32));
        at
    })();
    m.set_sp(sp);
    r
}

/// [soft 0x0857ccc4 SI_chunkSaveCurrent]: `SI_chunkSaveCurrent(buffer, &mark)`: 0 when five
/// fill pointers are saved; else the mark's number goes through `mark` (unless that is
/// null), the fill pointer into the table, the count up by one, and 1 is answered.
pub fn chunk_save_current(m: &mut Mem, buf: u32, mark: u32) -> u32 {
    let n = m.i32(buf + off::MARKS);
    if n >= 5 {
        return 0;
    }
    if mark != 0 {
        m.set_i32(mark, n);
    }
    let n = m.i32(buf + off::MARKS);
    let next = m.u32(buf + off::NEXT);
    m.set_u32(buf.wrapping_add(off::MARK).wrapping_add((n as u32).wrapping_mul(4)), next);
    let n = m.i32(buf + off::MARKS);
    m.set_i32(buf + off::MARKS, n.wrapping_add(1));
    1
}

/// [soft 0x0857cd00 SI_chunkReclaim]: `SI_chunkReclaim(buffer, mark)`: the fill pointer
/// back to a saved one. 0 when the count of marks or `mark` is negative, `mark` is above
/// the count, the saved pointer is null, or no slot from the current one down holds it
/// (unsigned: start ≤ pointer ≤ last byte). Else that slot becomes the current one, its
/// start and last byte and the pointer are stored, **the count of marks becomes `mark`**,
/// and 1 is answered. The table is indexed by `mark` without a look at its five entries.
pub fn chunk_reclaim(m: &mut Mem, buf: u32, mark: i32) -> u32 {
    if m.i32(buf + off::MARKS) < 0 || mark < 0 || mark > m.i32(buf + off::MARKS) {
        return 0;
    }
    let p = m.u32(buf.wrapping_add(off::MARK).wrapping_add((mark as u32).wrapping_mul(4)));
    if p == 0 {
        return 0;
    }
    let mut i = m.i32(buf + off::CURRENT);
    while i >= 0 {
        let s = buf.wrapping_add(off::SLOTS).wrapping_add((i as u32).wrapping_mul(off::SLOT));
        let start = m.u32(s.wrapping_add(4));
        let last = (m.i32(s).wrapping_sub(1) as u32).wrapping_add(start);
        if start <= p && p <= last {
            m.set_u32(buf + off::END, last);
            m.set_u32(buf + off::BASE, start);
            m.set_u32(buf + off::NEXT, p);
            m.set_i32(buf + off::CURRENT, i);
            m.set_i32(buf + off::MARKS, mark);
            return 1;
        }
        i -= 1;
    }
    0
}

/// The routines by the original's code address, for a caller that has an address and the
/// argument words (the render commands' `SI_chunkFreeAll(SI_renChunk)`, `SI_matterInit`'s
/// `SI_chunkAllocate`): the answer is what the routine leaves in `eax`, `None` for an
/// address that is none of the ten. Not a routine of the original.
pub fn call(m: &mut Mem, addr: u32, args: &[u32]) -> Option<u32> {
    let a = |k: usize| args.get(k).copied().unwrap_or(0);
    Some(match addr {
        0x0857_ca00 => chunk_allocate(m, a(0) as i32, a(1) as i32),
        0x0857_ca5c => {
            chunk_destroy(m, a(0));
            0
        }
        0x0857_ca90 => {
            chunk_initialize(m, a(0));
            0
        }
        0x0857_cb00 => {
            chunk_free_all(m, a(0));
            0
        }
        0x0857_cb64 => chunk_malloc(m, a(0), a(1) as i32),
        0x0857_cc3c => chunk_alloc_slot(m, a(0), a(1) as i32, a(2) as i32),
        0x0857_ccc4 => chunk_save_current(m, a(0), a(1)),
        0x0857_cd00 => chunk_reclaim(m, a(0), a(1) as i32),
        0x0857_cd7c => {
            pad_size(m, a(0));
            0
        }
        0x0857_cd98 => {
            align_pointer(m, a(0));
            0
        }
        _ => return None,
    })
}

/// What `SI_matterInit (0x08291bec)` does for the renderer's arena:
/// `SI_renChunk = SI_chunkAllocate(200, 5000000)`. Not a routine of the original (the rest
/// of `SI_matterInit` is the render set-up's); for an image nothing has run it on. Does
/// nothing when the pointer is set already; answers the record.
pub fn ren_chunk(m: &mut Mem) -> u32 {
    if m.u32(SI_REN_CHUNK) == 0 {
        let buf = chunk_allocate(m, REN_CHUNK_SLOTS, REN_CHUNK_BLOCK);
        m.set_u32(SI_REN_CHUNK, buf);
    }
    m.u32(SI_REN_CHUNK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem_chunk::{chunk_allocate as value_allocate, ChunkHeap};

    /// The image's heap as the port on a record of its own asks for it.
    struct ImageHeap<'a>(&'a mut Mem);
    impl ChunkHeap for ImageHeap<'_> {
        fn calloc(&mut self, size: u32) -> u32 {
            self.0.si_calloc(size, 1)
        }
        fn malloc(&mut self, size: u32) -> u32 {
            self.0.si_malloc(size)
        }
        fn free(&mut self, addr: u32) {
            self.0.free(addr)
        }
        fn poke(&mut self, addr: u32, byte: u8) {
            self.0.set_u8(addr, byte)
        }
    }

    /// The same sequence of requests through the two forms, each on an image of its own:
    /// the same answers, and the record in the image holds what the other form's does.
    #[test]
    fn the_two_forms_agree_step_by_step() {
        let (mut a, mut b) = (Mem::new(), Mem::new());
        let buf = chunk_allocate(&mut a, 4, 62);
        let mut v = value_allocate(&mut ImageHeap(&mut b), 4, 62).unwrap();
        assert_eq!(buf, v.addr);
        let mark = a.calloc(1, 4);
        b.calloc(1, 4);
        let mut seed = 7u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 16
        };
        for step in 0..400 {
            match next() % 8 {
                0 => {
                    let mut out = -1;
                    assert_eq!(chunk_save_current(&mut a, buf, mark), v.save_current(Some(&mut out)) as u32, "step {step}");
                    if out >= 0 {
                        assert_eq!(a.i32(mark), out);
                    }
                }
                1 => {
                    let k = (next() % 7) as i32 - 1;
                    assert_eq!(chunk_reclaim(&mut a, buf, k), v.reclaim(k) as u32, "step {step}");
                }
                2 if next() % 9 == 0 => {
                    chunk_free_all(&mut a, buf);
                    v.free_all(&mut ImageHeap(&mut b));
                }
                _ => {
                    let size = [0, 1, 4, 8, 12, 20, 28, 33, 60, 64, 100][(next() % 11) as usize];
                    assert_eq!(chunk_malloc(&mut a, buf, size), v.malloc(&mut ImageHeap(&mut b), size), "step {step}");
                }
            }
            assert_eq!((a.i32(buf), a.u32(buf + off::BASE), a.u32(buf + off::END), a.u32(buf + off::NEXT), a.i32(buf + off::MARKS)), (v.current, v.base, v.end, v.next, v.marks), "step {step}");
            for (i, s) in v.slots.iter().enumerate() {
                let at = buf + off::SLOTS + 12 * i as u32;
                assert_eq!((a.i32(at), a.u32(at + 4), a.u32(at + 8)), (s.size, s.ptr, s.raw), "step {step}, slot {i}");
            }
        }
        assert_eq!(a.live_blocks(), b.live_blocks());
        assert!(a.fault().is_none());
    }

    #[test]
    fn the_renderer_s_arena() {
        let mut m = Mem::program();
        assert_eq!(m.u32(SI_REN_CHUNK), 0);
        let buf = ren_chunk(&mut m);
        assert_eq!((m.i32(buf + off::SLOT_SIZE), m.i32(buf + off::SLOT_COUNT)), (5_000_000, 200));
        assert_eq!(m.block_size(buf), Some(200 * 12 + 0x40));
        assert_eq!(ren_chunk(&mut m), buf);
        // by address, as a render command calls it
        let p = call(&mut m, 0x0857_cb64, &[buf, 100]).unwrap();
        assert_eq!(p, m.u32(buf + off::SLOTS + 4));
        assert_eq!(m.block_size(m.u32(buf + off::SLOTS + 8)), Some(5_000_000));
        assert_eq!(call(&mut m, 0x0857_cb00, &[buf]), Some(0));
        assert_eq!(m.u32(buf + off::SLOTS + 8), 0);
        assert_eq!(call(&mut m, 0x0857_0000, &[]), None);
        assert!(m.fault().is_none());
    }
}
