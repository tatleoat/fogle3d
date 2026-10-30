// SPDX-License-Identifier: MIT
//! The image: the program's writable memory at the original's 32-bit addresses.
//!
//! Nothing here reproduces a function of the original; it is the ground the ported functions
//! stand on.
//!
//! # What is mapped
//!
//! | Region | Default place | Content |
//! |---|---|---|
//! | [`RODATA`] | the binary's `.rodata` | constants and string literals; zero until a crate stores the ones it owns ([`Mem::set_rom`]); an ordinary write faults |
//! | [`DATA`] | the binary's `.data` | initialised globals; zero until a crate writes the initial values of the globals it owns |
//! | [`BSS`] | the binary's `.bss` | globals that start as zero |
//! | [`ARENA`] | the harness's buffer area | records made by tests and checks, at the addresses the emulation harness gives them ([`Mem::place`]) |
//! | [`HEAP`] | the harness's heap | what the ported functions allocate ([`Mem::malloc`]) |
//! | [`STACK`] | the harness's stack | locals whose address is passed on ([`Mem::local`]) |
//!
//! Everything else is unmapped: page zero (the null pointer), the code ([`TEXT`]: a function
//! pointer is looked up in a [`crate::Registry`], never read), and [`HANDLES`], the range
//! whose addresses stand for things that are not memory ([`crate::Handles`]).
//!
//! The binary's own bytes are proprietary and are not in this crate: `.data` and `.rodata`
//! hold only what the crates wrote there, each value checked against the binary by a script
//! of the crate that owns it.
//!
//! # Faults
//!
//! A read or write that is not wholly inside one region is a fault: the original would be
//! killed by the system there. The image records the first one ([`Mem::fault`]) and, by
//! default, goes on: a read answers zeros, a write is dropped, a borrowed record is a
//! scratch record of zeros. Whoever drives a ported routine asks afterwards and treats
//! what the routine did after the fault as meaningless. With [`OnFault::Stop`] the access
//! unwinds instead (see there), which is what a check needs that compares what happened
//! *before* the fault with an emulator that stopped at it.
//!
//! # Borrowed views and copies
//!
//! Both exist and both are sound.
//!
//! - **Copy out, copy in** ([`Mem::read`], [`Mem::write`], the scalar accessors): works at
//!   every address, aligned or not, as the processor does.
//! - **Borrowed in place** (`m[p]`, [`Mem::get`], [`Mem::get_mut`], [`Mem::slice`]): a `&T`
//!   or `&mut T` straight into the image, so `m[p].field = v` is the original's store. It
//!   is sound because `T` is [`Pod`] (any bytes are a value, no padding, alignment at most
//!   4), every region starts at a multiple of 4 in a buffer aligned to a page, and the
//!   borrow is of the whole image: Rust's rules then forbid exactly what would be wrong,
//!   holding a `&mut` to one record while touching any other. Two records are changed
//!   together by copying one out, or by naming them one after the other. A borrowed view
//!   needs the address to be a multiple of `T`'s alignment; one that is not is recorded as a
//!   fault ([`Access::Misaligned`]) although the processor would allow it: use the copying
//!   calls for fields the original reaches at odd addresses.
//!
//! # Cost
//!
//! Regions are reserved whole but a page is zeroed only when it is first touched, so a
//! fresh image costs a few allocations however large the map, and a snapshot copies only
//! the pages in use. A check can make a new image for every case.
//!
//! One region is the exception: **a heap larger than the default 16 MB is backed in
//! steps.** Nothing is taken from the host for it until the allocator hands out the first
//! block; from then on the host's buffer covers the heap from its start to a little beyond
//! the highest block so far (it doubles up to steps of 64 MB), and only that part is
//! memory: an access above it is a fault, as an access above a process's break is. So a
//! map may declare a heap of a gigabyte ([`HEAP_PROGRAM`]) and an image that uses 3 MB of
//! it costs 4 MB of the host's address space and the pages it touched. See
//! [`crate::alloc`] for the two ways the heap hands out blocks.

use crate::{HeapPolicy, Pod, Ptr32};
use core::cell::Cell;
use core::ops::{Index, IndexMut};
use core::ptr::NonNull;
use std::alloc::{alloc, dealloc, handle_alloc_error, realloc, Layout};

/// `.text` of the Linux `soft` binary (section header): 0x080c51b0, 0x57fa84 bytes. Never
/// mapped.
pub const TEXT: (u32, u32) = (0x080c_51b0, 0x0057_fa84);
/// `.rodata`: 0x08644c60, 0x3329b bytes (section header).
pub const RODATA: (u32, u32) = (0x0864_4c60, 0x0003_329b);
/// `.data`: 0x08678f00, 0x88e30 bytes (section header).
pub const DATA: (u32, u32) = (0x0867_8f00, 0x0008_8e30);
/// `.bss`: 0x08780c80, 0xb41e8 bytes (section header).
pub const BSS: (u32, u32) = (0x0878_0c80, 0x000b_41e8);
/// Records made by tests and checks: the emulation harness's buffer area (`Sandbox.BUF`).
pub const ARENA: (u32, u32) = (0x6100_0000, 0x0080_0000);
/// What the ported functions allocate: the harness's heap (`Sandbox.HEAP`, 16 MB).
pub const HEAP: (u32, u32) = (0x6200_0000, 0x0100_0000);
/// The harness's heap made longer: the same start, so that the first blocks get the
/// addresses they get in [`HEAP`], and all the room up to [`HANDLES`] (208 MB). For a check
/// whose routine allocates more than 16 MB. Backed in steps.
pub const HEAP_WIDE: (u32, u32) = (0x6200_0000, 0x0d00_0000);
/// A heap for a whole run of the program: 1.25 GB in the free range below [`ARENA`]
/// (0x10000000 to 0x60000000; the binary's sections end at 0x08834e68). Backed in steps.
pub const HEAP_PROGRAM: (u32, u32) = (0x1000_0000, 0x5000_0000);
/// Addresses of things that are not memory (`Sandbox.FILES`): never mapped.
pub const HANDLES: (u32, u32) = (0x6f00_0000, 0x0100_0000);
/// The stack: the harness's 2 MB below 0x7ff00000.
pub const STACK: (u32, u32) = (0x7fd0_0000, 0x0020_0000);
/// Where the stack pointer starts (`Sandbox.ESP0`).
pub const STACK_START: u32 = 0x7fef_0000;

const PAGE: usize = 4096;
/// A heap backed in steps grows by what it has, at least and at most this much at once.
const STEP_MIN: u32 = 0x0001_0000;
const STEP_MAX: u32 = 0x0400_0000;
/// The alignment asked of the host for a buffer that grows. Not a page: with an alignment
/// the C library's own `realloc` serves, a large buffer is grown by remapping its pages
/// instead of by copying them (and the copy would touch every page of it).
const STEP_ALIGN: usize = 16;
/// The largest record a borrowed view can be asked for.
const SINK: usize = 1 << 20;

/// Where the regions lie. [`Map::default`] is the layout above; a crate whose check uses
/// other addresses for the arena and the heap says so here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Map {
    pub rodata: (u32, u32),
    pub data: (u32, u32),
    pub bss: (u32, u32),
    pub arena: (u32, u32),
    pub heap: (u32, u32),
    pub stack: (u32, u32),
    /// The stack pointer at the start.
    pub stack_start: u32,
    /// Bytes [`Mem::malloc`] puts before each block: 8 as the harness's `malloc` does (the
    /// size and a mark), or 0 for a plain bump allocator.
    pub heap_header: u32,
    /// Fill bytes [`Mem::place`] leaves after each block of the arena (the harness: 1 KB).
    pub arena_gap: u32,
}

impl Default for Map {
    fn default() -> Self {
        Map { rodata: RODATA, data: DATA, bss: BSS, arena: ARENA, heap: HEAP, stack: STACK, stack_start: STACK_START, heap_header: 8, arena_gap: 0x400 }
    }
}

impl Map {
    /// The default map with the heap made longer ([`HEAP_WIDE`]): every address of the
    /// default map is the same, and the heap has 208 MB.
    pub fn wide() -> Map {
        Map { heap: HEAP_WIDE, ..Map::default() }
    }

    /// The default map with the heap for a whole run of the program ([`HEAP_PROGRAM`]).
    pub fn program() -> Map {
        Map { heap: HEAP_PROGRAM, ..Map::default() }
    }
}

/// The regions of the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Rodata,
    Data,
    Bss,
    Arena,
    Heap,
    Stack,
}

/// How an access went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// A read outside every region (through null, a stale or a wild pointer).
    Read,
    /// A write outside every region, or into `.rodata`.
    Write,
    /// A borrowed view at an address that is not a multiple of the type's alignment.
    Misaligned,
    /// A call through a pointer that is null or does not point into the code.
    Call,
    /// `free` or `realloc` of something that is not a live block.
    Free,
}

/// An access the original would not have survived (or, for [`Access::Misaligned`], that
/// this model cannot serve).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub addr: u32,
    pub size: u32,
    pub access: Access,
}

impl Fault {
    pub fn is_write(&self) -> bool {
        self.access == Access::Write
    }
}

/// What a fault does besides being recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OnFault {
    /// The access answers a defined value (zeros; a write is dropped) and the routine goes
    /// on. Nothing panics. The default.
    #[default]
    Record,
    /// The access unwinds with a [`Stop`] as its payload, without a panic message
    /// (`std::panic::resume_unwind`): the routine ends where the original would have been
    /// killed. The driver catches it with `std::panic::catch_unwind` and finds the fault in
    /// the payload or in [`Mem::fault`]. For checks that compare a faulting case's calls
    /// and memory up to the fault with the emulated original's, and for routines whose
    /// loops would not end on zeros.
    Stop,
}

/// The payload of the unwinding under [`OnFault::Stop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop(pub Fault);

/// One region: its bytes at their place, zeroed page by page on first touch.
struct Seg {
    which: Region,
    base: u32,
    /// The region's length as the map declares it.
    len: u32,
    /// How many of those bytes are memory now: all of them, or, for a region backed in
    /// steps, the part reached so far (a multiple of a page, or `len`).
    mapped: u32,
    read_only: bool,
    /// Backed in steps ([`Seg::reach`]) and not whole.
    stepped: bool,
    buf: NonNull<u8>,
    cap: usize,
    /// One bit per page of `cap`: its bytes have been zeroed and may be referenced.
    committed: Vec<Cell<u64>>,
}

// A Seg owns its buffer; nothing else points into it once the borrows of the image end.
unsafe impl Send for Seg {}

impl Seg {
    fn new(which: Region, (base, len): (u32, u32), read_only: bool) -> Seg {
        assert!(base % 4 == 0, "a region must start at a multiple of 4");
        assert!(base.checked_add(len).is_some(), "a region must not wrap");
        let cap = (len as usize).div_ceil(PAGE) * PAGE;
        let buf = if cap == 0 {
            NonNull::dangling()
        } else {
            let layout = Layout::from_size_align(cap, PAGE).unwrap();
            // uninitialised: no byte is read before `ensure` has zeroed its page
            NonNull::new(unsafe { alloc(layout) }).unwrap_or_else(|| handle_alloc_error(layout))
        };
        let words = (cap / PAGE).div_ceil(64);
        Seg { which, base, len, mapped: len, read_only, stepped: false, buf, cap, committed: (0..words).map(|_| Cell::new(0)).collect() }
    }

    /// A region backed in steps: nothing of it is memory, and nothing is taken from the
    /// host, until [`Seg::reach`] says how far it goes.
    fn stepped(which: Region, (base, len): (u32, u32)) -> Seg {
        assert!(base % 4 == 0, "a region must start at a multiple of 4");
        assert!(base.checked_add(len).is_some(), "a region must not wrap");
        Seg { which, base, len, mapped: 0, read_only: false, stepped: true, buf: NonNull::dangling(), cap: 0, committed: Vec::new() }
    }

    /// Makes the first `need` bytes of a region backed in steps memory (`need` at most the
    /// region's length). False when the host has no room for them; the region is then as
    /// it was.
    ///
    /// The buffer may move, which is why this takes the region by `&mut`: no reference
    /// into the image can exist while it runs. What the pages hold moves with them, and
    /// the new pages are zeroed on first touch like every other.
    fn reach(&mut self, need: u32) -> bool {
        if need <= self.mapped {
            return true;
        }
        debug_assert!(self.stepped && need <= self.len);
        let step = self.mapped.clamp(STEP_MIN, STEP_MAX);
        let want = need.max(self.mapped.saturating_add(step)).min(self.len);
        let cap = (want as usize).div_ceil(PAGE) * PAGE;
        // SAFETY: `buf` came from `alloc` or `realloc` with this alignment and `self.cap`
        // bytes (or nothing was allocated yet, `cap` 0); the new size is not 0 and, rounded
        // up to the alignment, far below `isize::MAX` (a region is less than 4 GB). A null
        // answer leaves the old buffer valid.
        let buf = unsafe {
            if self.cap == 0 {
                alloc(Layout::from_size_align(cap, STEP_ALIGN).unwrap())
            } else {
                realloc(self.buf.as_ptr(), Layout::from_size_align(self.cap, STEP_ALIGN).unwrap(), cap)
            }
        };
        let Some(buf) = NonNull::new(buf) else {
            return false;
        };
        self.buf = buf;
        self.cap = cap;
        self.mapped = (cap as u64).min(self.len as u64) as u32;
        self.committed.resize((cap / PAGE).div_ceil(64), Cell::new(0));
        true
    }

    fn is_committed(&self, page: usize) -> bool {
        self.committed[page / 64].get() & (1 << (page % 64)) != 0
    }

    /// Makes the pages of `n` bytes at `off` usable (`n` at least 1, inside the region).
    ///
    /// Sound under a shared borrow: a page is written here only while its bit is clear, and
    /// no reference into a page is handed out before its bit is set.
    #[inline]
    fn ensure(&self, off: usize, n: usize) {
        let (first, last) = (off / PAGE, (off + n - 1) / PAGE);
        if first == last {
            let cell = &self.committed[first / 64];
            let bit = 1u64 << (first % 64);
            if cell.get() & bit != 0 {
                return;
            }
        }
        for page in first..=last {
            let cell = &self.committed[page / 64];
            let bit = 1u64 << (page % 64);
            if cell.get() & bit == 0 {
                unsafe { core::ptr::write_bytes(self.buf.as_ptr().add(page * PAGE), 0, PAGE) };
                cell.set(cell.get() | bit);
            }
        }
    }

    fn pages(&self) -> impl Iterator<Item = usize> + '_ {
        self.committed.iter().enumerate().flat_map(|(w, c)| {
            let bits = c.get();
            (0..64).filter(move |b| bits & (1 << b) != 0).map(move |b| w * 64 + b)
        })
    }

    /// The bytes of a committed page that belong to the region.
    fn page(&self, page: usize) -> &[u8] {
        debug_assert!(self.is_committed(page));
        let n = PAGE.min(self.mapped as usize - page * PAGE);
        unsafe { core::slice::from_raw_parts(self.buf.as_ptr().add(page * PAGE), n) }
    }
}

impl Drop for Seg {
    fn drop(&mut self) {
        if self.cap != 0 {
            let align = if self.stepped { STEP_ALIGN } else { PAGE };
            unsafe { dealloc(self.buf.as_ptr(), Layout::from_size_align(self.cap, align).unwrap()) };
        }
    }
}

/// What fresh memory holds: the bytes of a heap block before the routine writes it, of a
/// local, of a placed buffer and the gap behind it.
///
/// The original's `malloc` hands out whatever the heap held, and a local holds whatever
/// the stack held. A port cannot have those values; what it can do is show that its result
/// does not depend on them, the way the emulation harness does: run the case twice with
/// two patterns of which no byte is the same, and compare. A byte that differs between
/// the runs depends on memory nobody initialised; a byte that holds its pattern both times
/// was never written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fill {
    /// Zeros (what the ports assumed before there was an image).
    #[default]
    Zero,
    /// A fixed pseudo-random byte per address, shifted by `salt`; `second` is its
    /// complement, for the second run. The bytes are not the harness's (its vector files
    /// print unwritten words as dashes, never the pattern), only made the same way.
    Pattern { second: bool, salt: u32 },
}

impl Fill {
    /// The byte fresh memory holds at `addr`.
    pub fn byte(self, addr: u32) -> u8 {
        match self {
            Fill::Zero => 0,
            Fill::Pattern { second, salt } => {
                let mut x = addr.wrapping_add(salt).wrapping_mul(0x9e37_79b1);
                x ^= x >> 15;
                x = x.wrapping_mul(0x85eb_ca77);
                x ^= x >> 13;
                let b = (x >> 24) as u8;
                if second {
                    !b
                } else {
                    b
                }
            }
        }
    }
}

/// The image.
pub struct Mem {
    segs: Vec<Seg>,
    /// What a faulting read borrows: zeros, never written.
    zeros: Seg,
    /// What a faulting write borrows: zeroed before each use, never read back.
    scratch: Seg,
    pub(crate) map: Map,
    first_fault: Cell<Option<Fault>>,
    n_faults: Cell<u32>,
    on_fault: OnFault,
    pub(crate) fill: Fill,
    // the allocators (alloc.rs)
    pub(crate) heap_top: u32,
    pub(crate) heap_blocks: Vec<(u32, u32)>,
    pub(crate) arena_top: u32,
    pub(crate) arena_blocks: Vec<(u32, u32)>,
    pub(crate) freed: Vec<u32>,
    pub(crate) sp: u32,
    pub(crate) strict: bool,
    /// How the heap hands out blocks, and what that needs to know (alloc.rs). Boxed: kept
    /// in the image itself its tables push the fields every access reads apart, which was
    /// measured as a tenth more time per access (`examples/cost.rs`).
    pub(crate) heap: Box<crate::alloc::HeapState>,
    /// Which of `segs` is the heap.
    heap_seg: usize,
    /// The region the last access was in: tried first.
    last: Cell<usize>,
    /// Per 16 MB window of the address space: the first region that touches it (else 255).
    window: [u8; 256],
}

/// The content of the image at one moment, to find out later what changed.
pub struct Snapshot {
    /// Per region: the committed pages, ascending, and their bytes one after the other.
    segs: Vec<(Vec<u32>, Vec<u8>)>,
}

impl Default for Mem {
    fn default() -> Self {
        Mem::new()
    }
}

impl Mem {
    /// All zero, with the default map.
    pub fn new() -> Mem {
        Mem::with_map(Map::default())
    }

    /// All zero, with the regions where `map` puts them. They must not overlap. The heap
    /// hands out blocks as the emulation harness does ([`HeapPolicy::Harness`]).
    pub fn with_map(map: Map) -> Mem {
        Mem::with_heap(map, HeapPolicy::Harness)
    }

    /// For a whole run of the program: the default map with the large heap
    /// ([`Map::program`]) and blocks that are used again ([`HeapPolicy::Reuse`]). It costs
    /// the host what the run touches, not what the map declares.
    pub fn program() -> Mem {
        Mem::with_heap(Map::program(), HeapPolicy::Reuse)
    }

    /// All zero, with the regions where `map` puts them and a heap that hands out blocks
    /// by `policy`. The regions must not overlap. A heap larger than the default 16 MB is
    /// backed in steps (see the module's section on cost).
    pub fn with_heap(map: Map, policy: HeapPolicy) -> Mem {
        let heap = if map.heap.1 > HEAP.1 { Seg::stepped(Region::Heap, map.heap) } else { Seg::new(Region::Heap, map.heap, false) };
        let mut segs = vec![
            Seg::new(Region::Rodata, map.rodata, true),
            Seg::new(Region::Data, map.data, false),
            Seg::new(Region::Bss, map.bss, false),
            Seg::new(Region::Arena, map.arena, false),
            heap,
            Seg::new(Region::Stack, map.stack, false),
        ];
        segs.sort_by_key(|s| s.base);
        for w in segs.windows(2) {
            assert!(w[0].base + w[0].len <= w[1].base, "regions overlap: {:?} and {:?}", w[0].which, w[1].which);
        }
        let heap_seg = segs.iter().position(|s| s.which == Region::Heap).unwrap();
        let mut window = [255u8; 256];
        for (i, s) in segs.iter().enumerate() {
            if s.len != 0 {
                for w in (s.base >> 24)..=((s.base + (s.len - 1)) >> 24) {
                    if window[w as usize] == 255 {
                        window[w as usize] = i as u8;
                    }
                }
            }
        }
        Mem {
            segs,
            zeros: Seg::new(Region::Rodata, (0, SINK as u32), true),
            scratch: Seg::new(Region::Stack, (0, SINK as u32), false),
            map,
            first_fault: Cell::new(None),
            n_faults: Cell::new(0),
            on_fault: OnFault::Record,
            fill: Fill::Zero,
            heap_top: map.heap.0,
            heap_blocks: Vec::new(),
            arena_top: map.arena.0,
            arena_blocks: Vec::new(),
            freed: Vec::new(),
            sp: map.stack_start,
            strict: false,
            heap: Box::new(crate::alloc::HeapState::new(policy, map.heap.0)),
            heap_seg,
            last: Cell::new(0),
            window,
        }
    }

    /// Makes the heap memory up to the address `end` (at most the heap's end). Only a
    /// heap backed in steps has anything to do; false when the host has no room.
    pub(crate) fn heap_reach(&mut self, end: u32) -> bool {
        let s = &mut self.segs[self.heap_seg];
        !s.stepped || s.reach(end - s.base)
    }

    /// How many bytes of a region are memory now: its length, or for a heap backed in
    /// steps the part the allocator has reached so far.
    pub fn mapped(&self, which: Region) -> u32 {
        self.segs.iter().find(|s| s.which == which).map_or(0, |s| s.mapped)
    }

    /// Where the regions lie.
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// The place of one region: (address, length).
    pub fn region(&self, which: Region) -> (u32, u32) {
        match which {
            Region::Rodata => self.map.rodata,
            Region::Data => self.map.data,
            Region::Bss => self.map.bss,
            Region::Arena => self.map.arena,
            Region::Heap => self.map.heap,
            Region::Stack => self.map.stack,
        }
    }

    /// The region `addr` lies in, if any.
    pub fn region_of(&self, addr: u32) -> Option<Region> {
        self.segs.iter().find(|s| addr.wrapping_sub(s.base) < s.len).map(|s| s.which)
    }

    // ---- faults

    /// What fresh memory holds from now on (see [`Fill`]).
    pub fn set_fill(&mut self, fill: Fill) {
        self.fill = fill;
    }

    pub fn fill(&self) -> Fill {
        self.fill
    }

    /// What a fault does besides being recorded (see [`OnFault`]).
    pub fn on_fault(&mut self, policy: OnFault) {
        self.on_fault = policy;
    }

    /// Strict blocks: when on, an access in the arena or the heap must lie inside one block
    /// that was placed ([`Mem::place`], [`Mem::place_at`]) or allocated and not freed;
    /// anything else there is a fault. Off (the default) the two regions are mapped whole,
    /// as in the emulation harness, where a read past a buffer meets fill bytes and a freed
    /// block stays readable. On, a port's overrun or use of a freed block is caught at the
    /// access instead of by comparing two runs with different fill.
    pub fn set_strict(&mut self, on: bool) {
        self.strict = on;
    }

    /// The first access the original would not have survived, if any.
    pub fn fault(&self) -> Option<Fault> {
        self.first_fault.get()
    }

    /// How many faulting accesses there were since the last [`Mem::clear_fault`].
    pub fn fault_count(&self) -> u32 {
        self.n_faults.get()
    }

    /// Forgets the recorded faults.
    pub fn clear_fault(&mut self) {
        self.first_fault.set(None);
        self.n_faults.set(0);
    }

    /// Records a fault that something outside the image found (a call through a bad
    /// pointer, a `free` of a block that is none). Unwinds under [`OnFault::Stop`].
    pub fn record_fault(&self, addr: u32, size: u32, access: Access) {
        let f = Fault { addr, size, access };
        if self.first_fault.get().is_none() {
            self.first_fault.set(Some(f));
        }
        self.n_faults.set(self.n_faults.get().saturating_add(1));
        if self.on_fault == OnFault::Stop {
            std::panic::resume_unwind(Box::new(Stop(f)));
        }
    }

    // ---- finding bytes

    #[inline(always)]
    fn find(&self, addr: u32, n: u32) -> Option<(&Seg, usize)> {
        let s = &self.segs[self.last.get()];
        let off = addr.wrapping_sub(s.base);
        if off <= s.mapped && n <= s.mapped - off && !self.strict {
            return Some((s, off as usize));
        }
        self.find_other(addr, n)
    }

    /// [`Mem::find`] when the region of the last access is not it (or strict blocks are
    /// on): the region that owns the address's 16 MB window is tried, then all in order.
    /// Regions do not overlap, so the order only matters for speed.
    #[inline(never)]
    fn find_other(&self, addr: u32, n: u32) -> Option<(&Seg, usize)> {
        let hit = |i: usize| {
            let s = &self.segs[i];
            let off = addr.wrapping_sub(s.base);
            (off <= s.mapped && n <= s.mapped - off).then_some((s, off as usize))
        };
        // the region that starts in the address's 16 MB window or covers it, else all in order
        let c = self.window[(addr >> 24) as usize] as usize;
        let mut found = if c < self.segs.len() { hit(c).map(|f| (c, f)) } else { None };
        if found.is_none() {
            let mut i = 0;
            while i < self.segs.len() {
                if let Some(f) = hit(i) {
                    found = Some((i, f));
                    break;
                }
                i += 1;
            }
        }
        let (i, found) = found?;
        if self.strict && !self.in_live_block(found.0.which, addr, n) {
            return None;
        }
        self.last.set(i);
        Some(found)
    }

    /// Under strict blocks: whether `n` bytes at `addr` of that region lie in one live block.
    fn in_live_block(&self, which: Region, addr: u32, n: u32) -> bool {
        let blocks = match which {
            Region::Arena => &self.arena_blocks,
            Region::Heap if self.heap.policy == HeapPolicy::Reuse => return self.heap.in_live(addr, n),
            Region::Heap => &self.heap_blocks,
            _ => return true,
        };
        let i = blocks.partition_point(|b| b.0 <= addr);
        if i == 0 {
            return false;
        }
        let (base, len) = blocks[i - 1];
        addr - base <= len && n <= len - (addr - base) && !self.heap.is_freed(base)
    }

    /// `n` readable bytes at `addr` (`n` at least 1).
    #[inline(always)]
    fn rptr(&self, addr: u32, n: u32) -> Option<*mut u8> {
        let (s, off) = self.find(addr, n)?;
        s.ensure(off, n as usize);
        Some(unsafe { s.buf.as_ptr().add(off) })
    }

    /// `n` writable bytes at `addr` (`n` at least 1): not in `.rodata` unless `rom`.
    #[inline(always)]
    fn wptr(&self, addr: u32, n: u32, rom: bool) -> Option<*mut u8> {
        let (s, off) = self.find(addr, n)?;
        if s.read_only && !rom {
            return None;
        }
        s.ensure(off, n as usize);
        Some(unsafe { s.buf.as_ptr().add(off) })
    }

    /// True when `n` bytes at `addr` lie inside one region. Never a fault.
    pub fn is_mapped(&self, addr: u32, n: u32) -> bool {
        self.find(addr, n).is_some()
    }

    // ---- bytes

    /// `n` bytes at `addr`; empty, and a fault, when they are not all mapped. No bytes at
    /// any address are no fault (the original's `memcpy` of nothing does not touch memory).
    pub fn bytes(&self, addr: u32, n: u32) -> &[u8] {
        if n == 0 {
            return &[];
        }
        match self.rptr(addr, n) {
            Some(p) => unsafe { core::slice::from_raw_parts(p, n as usize) },
            None => {
                self.record_fault(addr, n, Access::Read);
                &[]
            }
        }
    }

    /// The same without recording a fault: `None` when the bytes are not all mapped. For
    /// drivers and dumps, not for ported code.
    pub fn try_bytes(&self, addr: u32, n: u32) -> Option<&[u8]> {
        if n == 0 {
            return Some(&[]);
        }
        self.rptr(addr, n).map(|p| unsafe { core::slice::from_raw_parts(p as *const u8, n as usize) })
    }

    /// Stores bytes; a fault, and nothing stored, when they do not all fit in one region
    /// or the region is `.rodata`.
    pub fn set_bytes(&mut self, addr: u32, data: &[u8]) {
        self.store(addr, data, false)
    }

    /// Stores constants: like [`Mem::set_bytes`], and `.rodata` takes them. For the code
    /// that sets up a crate's constants, never for a ported function.
    pub fn set_rom(&mut self, addr: u32, data: &[u8]) {
        self.store(addr, data, true)
    }

    fn store(&mut self, addr: u32, data: &[u8], rom: bool) {
        if data.is_empty() {
            return;
        }
        let n = u32::try_from(data.len()).unwrap_or(u32::MAX);
        match self.wptr(addr, n, rom) {
            Some(p) if n as usize == data.len() => unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len()) },
            _ => self.record_fault(addr, n, Access::Write),
        }
    }

    /// `memset(addr, byte, n)`.
    pub fn set_fill_bytes(&mut self, addr: u32, byte: u8, n: u32) {
        if n == 0 {
            return;
        }
        match self.wptr(addr, n, false) {
            Some(p) => unsafe { core::ptr::write_bytes(p, byte, n as usize) },
            None => self.record_fault(addr, n, Access::Write),
        }
    }

    /// `memmove(dst, src, n)`: the ranges may overlap.
    pub fn copy(&mut self, dst: u32, src: u32, n: u32) {
        if n == 0 {
            return;
        }
        let Some(s) = self.rptr(src, n) else {
            return self.record_fault(src, n, Access::Read);
        };
        let Some(d) = self.wptr(dst, n, false) else {
            return self.record_fault(dst, n, Access::Write);
        };
        unsafe { core::ptr::copy(s as *const u8, d, n as usize) };
    }

    /// `N` bytes stored at `addr` (a scalar setter): a fault, and nothing stored, when they do
    /// not lie in one writable region.
    #[inline(always)]
    fn wr<const N: usize>(&mut self, addr: u32, data: [u8; N]) {
        match self.wptr(addr, N as u32, false) {
            Some(p) => unsafe { core::ptr::write_unaligned(p as *mut [u8; N], data) },
            None => self.record_fault(addr, N as u32, Access::Write),
        }
    }

    #[inline(always)]
    fn rd<const N: usize>(&self, addr: u32) -> [u8; N] {
        match self.rptr(addr, N as u32) {
            Some(p) => unsafe { core::ptr::read_unaligned(p as *const [u8; N]) },
            None => {
                self.record_fault(addr, N as u32, Access::Read);
                [0; N]
            }
        }
    }

    // ---- scalars, at any address (the original reads such fields with plain moves)

    #[inline]
    pub fn u8(&self, a: u32) -> u8 {
        self.rd::<1>(a)[0]
    }
    #[inline]
    pub fn i8(&self, a: u32) -> i8 {
        self.rd::<1>(a)[0] as i8
    }
    #[inline]
    pub fn u16(&self, a: u32) -> u16 {
        u16::from_le_bytes(self.rd(a))
    }
    #[inline]
    pub fn i16(&self, a: u32) -> i16 {
        i16::from_le_bytes(self.rd(a))
    }
    #[inline]
    pub fn u32(&self, a: u32) -> u32 {
        u32::from_le_bytes(self.rd(a))
    }
    #[inline]
    pub fn i32(&self, a: u32) -> i32 {
        i32::from_le_bytes(self.rd(a))
    }
    #[inline]
    pub fn u64(&self, a: u32) -> u64 {
        u64::from_le_bytes(self.rd(a))
    }
    #[inline]
    pub fn i64(&self, a: u32) -> i64 {
        i64::from_le_bytes(self.rd(a))
    }
    /// A pointer field.
    #[inline]
    pub fn ptr(&self, a: u32) -> u32 {
        self.u32(a)
    }
    /// A `float`, bits untouched (an integer move; where the original copies through the
    /// x87 unit a signalling NaN is quieted, which is the arithmetic crate's business).
    #[inline]
    pub fn f32(&self, a: u32) -> f32 {
        f32::from_le_bytes(self.rd(a))
    }
    #[inline]
    pub fn f64(&self, a: u32) -> f64 {
        f64::from_le_bytes(self.rd(a))
    }

    #[inline]
    pub fn set_u8(&mut self, a: u32, v: u8) {
        self.wr(a, [v]);
    }
    #[inline]
    pub fn set_i8(&mut self, a: u32, v: i8) {
        self.wr(a, [v as u8]);
    }
    #[inline]
    pub fn set_u16(&mut self, a: u32, v: u16) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_i16(&mut self, a: u32, v: i16) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_u32(&mut self, a: u32, v: u32) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_i32(&mut self, a: u32, v: i32) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_u64(&mut self, a: u32, v: u64) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_i64(&mut self, a: u32, v: i64) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_ptr(&mut self, a: u32, v: u32) {
        self.set_u32(a, v);
    }
    #[inline]
    pub fn set_f32(&mut self, a: u32, v: f32) {
        self.wr(a, v.to_le_bytes());
    }
    #[inline]
    pub fn set_f64(&mut self, a: u32, v: f64) {
        self.wr(a, v.to_le_bytes());
    }

    /// `*a |= bits` on one byte, as the original's `orb` does.
    pub fn or_u8(&mut self, a: u32, bits: u8) {
        let v = self.u8(a);
        self.set_u8(a, v | bits);
    }
    /// `*a &= bits` on one byte.
    pub fn and_u8(&mut self, a: u32, bits: u8) {
        let v = self.u8(a);
        self.set_u8(a, v & bits);
    }

    // ---- typed, by copy: any address

    /// The record at `p`, copied out; zeros and a fault when it is not mapped.
    pub fn read<T: Pod>(&self, p: Ptr32<T>) -> T {
        let size = core::mem::size_of::<T>() as u32;
        if size != 0 {
            match self.rptr(p.0, size) {
                Some(b) => return unsafe { core::ptr::read_unaligned(b as *const T) },
                None => self.record_fault(p.0, size, Access::Read),
            }
        }
        // every bit pattern of a Pod is a value
        unsafe { core::mem::zeroed() }
    }

    /// Stores a record at `p`; a fault and nothing stored when it does not fit.
    pub fn write<T: Pod>(&mut self, p: Ptr32<T>, value: T) {
        let size = core::mem::size_of::<T>() as u32;
        if size == 0 {
            return;
        }
        match self.wptr(p.0, size, false) {
            Some(b) => unsafe { core::ptr::write_unaligned(b as *mut T, value) },
            None => self.record_fault(p.0, size, Access::Write),
        }
    }

    // ---- typed, in place: aligned addresses

    /// The record at `p`, in place. On a fault: a record of zeros.
    pub fn get<T: Pod>(&self, p: Ptr32<T>) -> &T {
        const { assert!(core::mem::align_of::<T>() <= 4 && core::mem::size_of::<T>() <= SINK) };
        let size = core::mem::size_of::<T>() as u32;
        if size == 0 {
            return unsafe { NonNull::<T>::dangling().as_ref() };
        }
        if p.0 % core::mem::align_of::<T>() as u32 != 0 {
            self.record_fault(p.0, size, Access::Misaligned);
        } else if let Some(b) = self.rptr(p.0, size) {
            // in bounds, aligned (the region's base is a multiple of 4 in a page-aligned
            // buffer), initialised (the page was zeroed), and T is Pod
            return unsafe { &*(b as *const T) };
        } else {
            self.record_fault(p.0, size, Access::Read);
        }
        self.zeros.ensure(0, size as usize);
        unsafe { &*(self.zeros.buf.as_ptr() as *const T) }
    }

    /// The record at `p`, in place, to change. On a fault: a scratch record of zeros whose
    /// changes are lost.
    pub fn get_mut<T: Pod>(&mut self, p: Ptr32<T>) -> &mut T {
        const { assert!(core::mem::align_of::<T>() <= 4 && core::mem::size_of::<T>() <= SINK) };
        let size = core::mem::size_of::<T>() as u32;
        if size == 0 {
            return unsafe { NonNull::<T>::dangling().as_mut() };
        }
        if p.0 % core::mem::align_of::<T>() as u32 != 0 {
            self.record_fault(p.0, size, Access::Misaligned);
        } else if let Some(b) = self.wptr(p.0, size, false) {
            return unsafe { &mut *(b as *mut T) };
        } else {
            self.record_fault(p.0, size, Access::Write);
        }
        self.scratch.ensure(0, size as usize);
        unsafe {
            core::ptr::write_bytes(self.scratch.buf.as_ptr(), 0, size as usize);
            &mut *(self.scratch.buf.as_ptr() as *mut T)
        }
    }

    /// `n` records from `p` on, in place. On a fault: no records (so index the result only
    /// below its length, or take the elements one by one with `m[p.add(i)]`).
    pub fn slice<T: Pod>(&self, p: Ptr32<T>, n: u32) -> &[T] {
        const { assert!(core::mem::align_of::<T>() <= 4) };
        let total = n as u64 * core::mem::size_of::<T>() as u64;
        if total == 0 {
            return &[];
        }
        if p.0 % core::mem::align_of::<T>() as u32 != 0 {
            self.record_fault(p.0, total.min(u32::MAX as u64) as u32, Access::Misaligned);
            return &[];
        }
        match u32::try_from(total).ok().and_then(|t| self.rptr(p.0, t)) {
            Some(b) => unsafe { core::slice::from_raw_parts(b as *const T, n as usize) },
            None => {
                self.record_fault(p.0, total.min(u32::MAX as u64) as u32, Access::Read);
                &[]
            }
        }
    }

    /// `n` records from `p` on, in place, to change. On a fault: no records.
    pub fn slice_mut<T: Pod>(&mut self, p: Ptr32<T>, n: u32) -> &mut [T] {
        const { assert!(core::mem::align_of::<T>() <= 4) };
        let total = n as u64 * core::mem::size_of::<T>() as u64;
        if total == 0 {
            return &mut [];
        }
        if p.0 % core::mem::align_of::<T>() as u32 != 0 {
            self.record_fault(p.0, total.min(u32::MAX as u64) as u32, Access::Misaligned);
            return &mut [];
        }
        match u32::try_from(total).ok().and_then(|t| self.wptr(p.0, t, false)) {
            Some(b) => unsafe { core::slice::from_raw_parts_mut(b as *mut T, n as usize) },
            None => {
                self.record_fault(p.0, total.min(u32::MAX as u64) as u32, Access::Write);
                &mut []
            }
        }
    }

    // ---- C strings

    /// The bytes of the zero-terminated string at `addr`, without the zero. Where the
    /// string runs out of mapped memory: what there was, and a fault.
    pub fn cstr(&self, addr: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut a = addr;
        loop {
            let Some((s, off)) = self.find(a, 1) else {
                self.record_fault(a, 1, Access::Read);
                return out;
            };
            // to the end of this page or of the region, whichever comes first
            let n = (PAGE - off % PAGE).min(s.mapped as usize - off);
            s.ensure(off, n);
            let chunk = unsafe { core::slice::from_raw_parts(s.buf.as_ptr().add(off) as *const u8, n) };
            match chunk.iter().position(|&b| b == 0) {
                Some(i) => {
                    out.extend_from_slice(&chunk[..i]);
                    return out;
                }
                None => {
                    out.extend_from_slice(chunk);
                    a = a.wrapping_add(n as u32);
                }
            }
        }
    }

    /// `strlen`.
    pub fn strlen(&self, addr: u32) -> u32 {
        self.cstr(addr).len() as u32
    }

    /// Stores a string and its terminating zero (`strcpy` from Rust bytes).
    pub fn put_cstr(&mut self, addr: u32, s: &[u8]) {
        self.set_bytes(addr, s);
        self.set_u8(addr.wrapping_add(s.len() as u32), 0);
    }

    /// The same into `.rodata` or anywhere else: a string literal of the original.
    pub fn put_rom_cstr(&mut self, addr: u32, s: &[u8]) {
        self.set_rom(addr, s);
        self.set_rom(addr.wrapping_add(s.len() as u32), &[0]);
    }

    // ---- snapshots

    /// The content now.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            segs: self
                .segs
                .iter()
                .map(|s| {
                    let pages: Vec<u32> = s.pages().map(|p| p as u32).collect();
                    let mut data = Vec::with_capacity(pages.len() * PAGE);
                    for &p in &pages {
                        let b = s.page(p as usize);
                        data.extend_from_slice(b);
                        data.resize(data.len() + PAGE - b.len(), 0);
                    }
                    (pages, data)
                })
                .collect(),
        }
    }

    /// Every run of bytes of the program's state that differs from the snapshot: (address,
    /// the bytes now), in address order. The stack is left out (locals end with the call;
    /// the harness does not report them either): see [`Mem::changed_in`].
    pub fn changed(&self, old: &Snapshot) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        for (s, o) in self.segs.iter().zip(&old.segs) {
            if s.which != Region::Stack {
                Self::diff_seg(s, o, &mut out);
            }
        }
        out
    }

    /// The same for one region.
    pub fn changed_in(&self, old: &Snapshot, which: Region) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        for (s, o) in self.segs.iter().zip(&old.segs) {
            if s.which == which {
                Self::diff_seg(s, o, &mut out);
            }
        }
        out
    }

    fn diff_seg(s: &Seg, (old_pages, old_data): &(Vec<u32>, Vec<u8>), out: &mut Vec<(u32, Vec<u8>)>) {
        const ZERO: [u8; PAGE] = [0; PAGE];
        let first = out.len();
        let mut k = 0;
        for page in s.pages() {
            while k < old_pages.len() && (old_pages[k] as usize) < page {
                k += 1;
            }
            let now = s.page(page);
            let was = if k < old_pages.len() && old_pages[k] as usize == page { &old_data[k * PAGE..][..now.len()] } else { &ZERO[..now.len()] };
            if now == was {
                continue;
            }
            for i in 0..now.len() {
                if now[i] != was[i] {
                    let a = s.base + (page * PAGE + i) as u32;
                    match out[first..].last_mut() {
                        // a run goes on over a page boundary, never from one region to the next
                        Some((start, bytes)) if *start + bytes.len() as u32 == a => bytes.push(now[i]),
                        _ => out.push((a, vec![now[i]])),
                    }
                }
            }
        }
    }

    /// How many bytes of the image are in use (pages touched so far × 4096): what a fresh
    /// image and a snapshot cost.
    pub fn committed_bytes(&self) -> usize {
        self.segs.iter().map(|s| s.pages().count() * PAGE).sum()
    }

    /// How many bytes of the host's address space the image's regions hold: the regions
    /// that are reserved whole, and of a heap backed in steps what it has reached. Of
    /// these the host pays for [`Mem::committed_bytes`], the pages that were touched.
    pub fn reserved_bytes(&self) -> usize {
        self.segs.iter().map(|s| s.cap).sum()
    }
}

impl<T: Pod> Index<Ptr32<T>> for Mem {
    type Output = T;
    fn index(&self, p: Ptr32<T>) -> &T {
        self.get(p)
    }
}
impl<T: Pod> IndexMut<Ptr32<T>> for Mem {
    fn index_mut(&mut self, p: Ptr32<T>) -> &mut T {
        self.get_mut(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Double, Opaque};

    crate::record! {
        pub struct Node = "SI_Node", size 0x18 {
            0x00 next: Ptr32<Node>, "the next node";
            0x04 id: i16, "identifier";
            0x06 flags: [u8; 2], "two flag bytes";
            0x08 weight: Double, "a double";
            0x10 name: Ptr32<u8>, "the name";
            0x14 data: Ptr32<Opaque>, "anything";
        }
    }

    #[test]
    fn the_sections_are_at_their_addresses() {
        let m = Mem::new();
        assert_eq!(m.region_of(0x0867_8f00), Some(Region::Data));
        assert_eq!(m.region_of(0x0867_8f00 + 0x0008_8e30 - 1), Some(Region::Data));
        assert_eq!(m.region_of(0x0867_8f00 + 0x0008_8e30), None); // .eh_frame: not mapped
        assert_eq!(m.region_of(0x0878_0c80), Some(Region::Bss));
        assert_eq!(m.region_of(0x0864_4c60), Some(Region::Rodata));
        assert_eq!(m.region_of(0x6100_0000), Some(Region::Arena));
        assert_eq!(m.region_of(0x6200_0000), Some(Region::Heap));
        assert_eq!(m.region_of(0x7fee_fffc), Some(Region::Stack));
        assert_eq!(m.region_of(0), None);
        assert_eq!(m.region_of(TEXT.0), None);
        assert_eq!(m.region_of(HANDLES.0), None);
        assert_eq!(m.committed_bytes(), 0);
        assert_eq!(m.u32(BSS.0 + BSS.1 - 4), 0);
        assert_eq!(m.committed_bytes(), 4096);
        assert!(m.fault().is_none());
    }

    #[test]
    fn records_in_place_and_by_copy() {
        let mut m = Mem::new();
        let a: Ptr32<Node> = Ptr32::new(ARENA.0);
        let b = a.add(1);
        m[a].next = b;
        m[a].id = -2;
        m[a].flags[1] = 0x80;
        m[a].weight = Double(1.5);
        m[b].id = 7;
        // the same bytes, seen raw: little-endian, the double at +8
        assert_eq!(m.u32(ARENA.0), ARENA.0 + 0x18);
        assert_eq!(m.u32(ARENA.0 + 4), 0x8000_fffe);
        assert_eq!(m.f64(ARENA.0 + 8), 1.5);
        // follow the pointer
        let next = m[a].next;
        assert_eq!(m[next].id, 7);
        // a field's address, and a copy at an odd address
        let id: Ptr32<i16> = crate::field!(b, Node, id);
        assert_eq!(m[id], 7);
        let odd: Ptr32<Node> = Ptr32::new(ARENA.0 + 0x101);
        let mut n = m.read(a);
        n.id = 9;
        m.write(odd, n);
        assert_eq!(m.read(odd).id, 9);
        assert_eq!(m.i16(ARENA.0 + 0x105), 9);
        assert!(m.fault().is_none());
        // arrays
        let v: Ptr32<f32> = Ptr32::new(ARENA.0 + 0x200);
        m.slice_mut(v, 3).copy_from_slice(&[1.0, 2.0, 3.0]);
        assert_eq!(m[v.add(2)], 3.0);
        assert_eq!(m.slice(v, 3), &[1.0, 2.0, 3.0]);
        let arr: Ptr32<[i16; 4]> = Ptr32::new(BSS.0 + 0x40);
        m[arr][3] = 5;
        assert_eq!(m[arr.elem(3)], 5);
    }

    #[test]
    fn null_and_wild_accesses_are_recorded_and_answer_zeros() {
        let mut m = Mem::new();
        let null: Ptr32<Node> = Ptr32::NULL;
        assert_eq!(m[null].id, 0);
        assert_eq!(m.fault(), Some(Fault { addr: 0, size: 0x18, access: Access::Read }));
        m[null].id = 5; // dropped
        assert_eq!(m[null].id, 0);
        assert_eq!(m.u32(0xdead_beec), 0);
        m.set_u32(4, 1);
        assert_eq!(m.read(null.add(1)).id, 0);
        assert_eq!(m.bytes(0x10, 4), &[] as &[u8]);
        assert_eq!(m.slice(null, 4).len(), 0);
        assert_eq!(m.cstr(0), b"");
        // the first one stays, the others are counted
        assert_eq!(m.fault(), Some(Fault { addr: 0, size: 0x18, access: Access::Read }));
        assert_eq!(m.fault_count(), 9);
        m.clear_fault();
        // an access that runs over the end of a region
        assert_eq!(m.u32(DATA.0 + DATA.1 - 2), 0);
        assert_eq!(m.fault(), Some(Fault { addr: DATA.0 + DATA.1 - 2, size: 4, access: Access::Read }));
        m.clear_fault();
        // no bytes are no access
        assert_eq!(m.bytes(0, 0), &[] as &[u8]);
        m.set_bytes(0, &[]);
        m.copy(0, 0, 0);
        assert!(m.fault().is_none());
        // a view at an odd address
        let odd: Ptr32<i32> = Ptr32::new(ARENA.0 + 2);
        assert_eq!(m[odd], 0);
        assert_eq!(m.fault().map(|f| f.access), Some(Access::Misaligned));
        m.clear_fault();
        assert_eq!(m.read(odd), 0);
        assert!(m.fault().is_none());
    }

    #[test]
    fn rodata_takes_constants_only() {
        let mut m = Mem::new();
        m.put_rom_cstr(RODATA.0 + 0x10, b"Front");
        assert_eq!(m.cstr(RODATA.0 + 0x10), b"Front");
        assert!(m.fault().is_none());
        m.set_u8(RODATA.0 + 0x10, b'f');
        assert_eq!(m.fault(), Some(Fault { addr: RODATA.0 + 0x10, size: 1, access: Access::Write }));
        assert_eq!(m.cstr(RODATA.0 + 0x10), b"Front");
    }

    #[test]
    fn stop_unwinds_at_the_fault_without_a_message() {
        let mut m = Mem::new();
        m.on_fault(OnFault::Stop);
        m.set_i32(BSS.0, 1);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            m.set_i32(BSS.0, 2);
            let v = m.u32(8);
            m.set_i32(BSS.0, 3); // not reached
            v
        }));
        let stop = *r.unwrap_err().downcast::<Stop>().unwrap();
        assert_eq!(stop.0, Fault { addr: 8, size: 4, access: Access::Read });
        assert_eq!(m.fault(), Some(stop.0));
        assert_eq!(m.i32(BSS.0), 2);
    }

    #[test]
    fn strings() {
        let mut m = Mem::new();
        m.put_cstr(ARENA.0 + 0xffd, b"Edit Scale in X / L"); // over a page boundary
        assert_eq!(m.cstr(ARENA.0 + 0xffd), b"Edit Scale in X / L");
        assert_eq!(m.strlen(ARENA.0 + 0x1000), 16);
        // a string without an end runs to the end of the region and faults there
        let end = BSS.0 + BSS.1;
        m.set_bytes(end - 3, b"abc");
        assert_eq!(m.cstr(end - 3), b"abc");
        assert_eq!(m.fault(), Some(Fault { addr: end, size: 1, access: Access::Read }));
    }

    #[test]
    fn snapshot_and_changes() {
        let mut m = Mem::new();
        m.set_i16(BSS.0 + 0x10, 1);
        let before = m.snapshot();
        assert!(m.changed(&before).is_empty());
        m.set_i16(BSS.0 + 0x10, -2);
        m.set_i16(BSS.0 + 0x12, 7);
        // a run over a page boundary, in a page the snapshot did not hold
        m.set_bytes(ARENA.0 + 0xffe, &[1, 2, 3, 4]);
        // a store of the value that was there is no change
        m.set_u32(DATA.0, 0);
        // the stack is not part of the state
        m.set_u32(STACK_START - 4, 9);
        assert_eq!(
            m.changed(&before),
            vec![(BSS.0 + 0x10, vec![0xfe, 0xff, 0x07]), (ARENA.0 + 0xffe, vec![1, 2, 3, 4])]
        );
        assert_eq!(m.changed_in(&before, Region::Stack), vec![(STACK_START - 4, vec![9])]);
    }

    #[test]
    fn a_map_of_ones_own() {
        // the user-interface check's layout: arena and heap side by side, a plain heap
        let map = Map { arena: (0x6100_0000, 0x0010_0000), heap: (0x6110_0000, 0x0010_0000), heap_header: 0, ..Map::default() };
        let mut m = Mem::with_map(map);
        let before = m.snapshot();
        m.set_bytes(0x610f_fffe, &[1, 2]);
        m.set_bytes(0x6110_0000, &[3, 4]);
        // adjacent regions: two runs, and no access across the border
        assert_eq!(m.changed(&before), vec![(0x610f_fffe, vec![1, 2]), (0x6110_0000, vec![3, 4])]);
        assert_eq!(m.u32(0x610f_fffe), 0);
        assert!(m.fault().is_some());
    }

    #[test]
    fn strict_blocks() {
        let mut m = Mem::new();
        m.set_strict(true);
        let a = m.place(8, 8);
        let h = m.malloc(8);
        assert!(m.place_at(ARENA.0 + 0x2000, 16));
        m.set_u32(a + 4, 1);
        m.set_u32(h + 4, 2);
        m.set_u32(ARENA.0 + 0x200c, 3);
        m.set_u32(BSS.0, 4); // globals are not blocks
        assert!(m.fault().is_none());
        assert_eq!(m.u32(a + 6), 0); // runs over the end of the buffer
        assert_eq!(m.fault(), Some(Fault { addr: a + 6, size: 4, access: Access::Read }));
        m.clear_fault();
        assert_eq!(m.u32(ARENA.0 + 0x1000), 0); // between buffers
        assert!(m.fault().is_some());
        m.clear_fault();
        m.free(h);
        assert_eq!(m.u32(h + 4), 0); // a freed block
        assert_eq!(m.fault(), Some(Fault { addr: h + 4, size: 4, access: Access::Read }));
        m.clear_fault();
        m.set_strict(false);
        assert_eq!((m.u32(h + 4), m.u32(a + 6)), (2, 0));
        assert!(m.fault().is_none());
    }

    #[test]
    fn fill_patterns_are_complements() {
        let (a, b) = (Fill::Pattern { second: false, salt: 3 }, Fill::Pattern { second: true, salt: 3 });
        let mut same = 0;
        for addr in 0x6200_0000..0x6200_1000u32 {
            assert_eq!(a.byte(addr), !b.byte(addr));
            same += (a.byte(addr) == a.byte(addr + 1)) as u32;
        }
        assert!(same < 64); // neighbours differ: two fresh values are not equal by construction
        assert_eq!(Fill::Zero.byte(5), 0);
    }

    #[test]
    fn accesses_alternating_between_regions_find_their_bytes() {
        // the two regions tried first (the last two used) must never hide a third
        let mut m = Mem::new();
        let h = m.malloc(16);
        let a = m.place(16, 8);
        let addrs = [h, a, BSS.0 + 8, DATA.0 + 4, STACK.0 + 0x100, h + 4, BSS.0 + 8, a + 4];
        // exact values: write distinct values, read back in another order
        for (k, &ad) in addrs.iter().enumerate().filter(|(k, _)| *k != 6) {
            m.set_u32(ad, 7000 + k as u32);
        }
        for (k, &ad) in addrs.iter().enumerate().rev().filter(|(k, _)| *k != 6) {
            assert_eq!(m.u32(ad), 7000 + k as u32);
        }
        assert!(m.fault().is_none());
        assert_eq!(m.u32(0x0000_0010), 0); // unmapped between region hits
        assert_eq!(m.fault(), Some(Fault { addr: 0x10, size: 4, access: Access::Read }));
        m.clear_fault();
        assert_eq!(m.u32(DATA.0 + 4), 7003);
        assert!(m.fault().is_none());
        // a read straddling the end of a region is a fault whichever region was last
        assert_eq!(m.u32(STACK.0 + STACK.1 - 2), 0);
        assert!(m.fault().is_some());
    }
}
