// SPDX-License-Identifier: MIT
//! Checks of the chunk allocator (`mem_chunk.c`) against the vectors of
//! `re/tools/si-render/emu/gen_chunk.py`.
//!
//! The routines compare and store addresses, so the check works with the addresses the
//! emulator used: every vector says where its buffers lay (`at`) and where the blocks the
//! routine got from `malloc` lay (`at_heap`), and [`EmuHeap`] hands out blocks by the
//! harness's own rule.
use si_render::mem_chunk::{align_pointer, chunk_allocate, chunk_destroy, pad_size, ChunkBuffer, ChunkHeap, ChunkSlot};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

/// The harness's `malloc` (`re/tools/emu/emu.py`, `Sandbox.malloc`): a bump arena from
/// 0x62000000, each block after an 8-byte header on a multiple of eight, 16 MB in all.
pub struct EmuHeap {
    top: u32,
    /// Blocks in the order they were handed out: (address, size).
    pub blocks: Vec<(u32, u32)>,
    /// Bytes written: (address, value).
    pub poked: Vec<(u32, u8)>,
    pub freed: Vec<u32>,
}

const HEAP: u32 = 0x6200_0000;
const HEAP_SIZE: u32 = 0x100_0000;

impl EmuHeap {
    pub fn new() -> EmuHeap {
        EmuHeap { top: HEAP, blocks: Vec::new(), poked: Vec::new(), freed: Vec::new() }
    }

    pub fn block(&mut self, n: u32) -> u32 {
        let a = (self.top + 7) & !7;
        if n > HEAP_SIZE || a as u64 + 8 + n as u64 > HEAP as u64 + HEAP_SIZE as u64 {
            return 0;
        }
        self.top = a + 8 + n;
        self.blocks.push((a + 8, n));
        a + 8
    }
}

impl ChunkHeap for EmuHeap {
    fn calloc(&mut self, size: u32) -> u32 {
        let p = self.block(size);
        if size == 0 {
            0
        } else {
            p
        }
    }
    fn malloc(&mut self, size: u32) -> u32 {
        if size == 0 {
            return 0;
        }
        let p = self.block(size);
        if p != 0 {
            // SI_malloc's own guard bytes
            self.poked.push((p, 0x6f));
            self.poked.push((p + size - 1, 0x6b));
        }
        p
    }
    fn free(&mut self, addr: u32) {
        self.freed.push(addr);
    }
    fn poke(&mut self, addr: u32, byte: u8) {
        self.poked.push((addr, byte));
    }
}

const NAMES: [&str; 5] = ["buf", "s0", "s1", "s2", "s3"];

/// A word of a vector as the 32 bits the emulator's memory held; `names` are the buffers
/// whose addresses the vector's `at` field lists, in order.
pub fn abs_in(c: &Case, names: &[&str], w: &Word) -> u32 {
    match w {
        Word::Value { value, .. } => *value as u32,
        Word::Unwritten { .. } => 0,
        Word::Ref { name, offset } => {
            let base = match name.strip_prefix("heap").and_then(|k| k.parse::<usize>().ok()) {
                Some(k) => c.output("at_heap").u32s()[k],
                None => c.output("at").u32s()[names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no address for buffer {name}"))],
            };
            base.wrapping_add(*offset)
        }
    }
}

pub fn abs(c: &Case, w: &Word) -> u32 {
    abs_in(c, &NAMES, w)
}

fn words_in(c: &Case, names: &[&str], f: &Field) -> Vec<u32> {
    f.words().iter().map(|w| abs_in(c, names, w)).collect()
}

fn words(c: &Case, f: &Field) -> Vec<u32> {
    words_in(c, &NAMES, f)
}

/// The record from its words (see `gen_chunk.py`: the slots and three spare words); its own
/// address is the first of `at`.
pub fn buffer_in(c: &Case, names: &[&str], f: &Field) -> ChunkBuffer {
    let w = words_in(c, names, f);
    let n = (w.len() - 16) / 3;
    ChunkBuffer {
        addr: c.output("at").u32s()[0],
        current: w[0] as i32,
        busy: w[1] as i16,
        base: w[3],
        end: w[4],
        next: w[5],
        marks: w[6] as i32,
        mark: [w[7], w[8], w[9], w[10], w[11]],
        slot_size: w[12] as i32,
        slot_count: w[13] as i32,
        slots: (0..n).map(|i| ChunkSlot { size: w[14 + 3 * i] as i32, ptr: w[15 + 3 * i], raw: w[16 + 3 * i] }).collect(),
    }
}

fn buffer(c: &Case, f: &Field) -> ChunkBuffer {
    buffer_in(c, &NAMES, f)
}

/// Compares the record `field` as the routine left it with the port's.
pub fn same_record_in(c: &Case, names: &[&str], field: &str, b: &ChunkBuffer) -> Result<(), String> {
    let want = words_in(c, names, c.after(field));
    let got = record(b, &words_in(c, names, c.input(field)));
    match (0..want.len()).find(|&i| want[i] != got[i]) {
        None => Ok(()),
        Some(i) => Err(format!("chunk record word {i}: original {:08x}, port {:08x}", want[i], got[i])),
    }
}

/// The record as words again; `like` supplies the words the routines never touch (the
/// padding after the busy flag, the spare words).
fn record(b: &ChunkBuffer, like: &[u32]) -> Vec<u32> {
    let mut w = vec![b.current as u32, b.busy as u16 as u32, like.get(2).copied().unwrap_or(0), b.base, b.end, b.next, b.marks as u32];
    w.extend(b.mark);
    w.extend([b.slot_size as u32, b.slot_count as u32]);
    for s in &b.slots {
        w.extend([s.size as u32, s.ptr, s.raw]);
    }
    while w.len() < like.len() {
        w.push(like[w.len()]);
    }
    w.truncate(like.len());
    w
}

fn same_record(c: &Case, b: &ChunkBuffer) -> Result<(), String> {
    same_record_in(c, &NAMES, "buf", b)
}

/// The blocks the routine got from `malloc`: as many, as large, and with the same bytes
/// written.
pub fn same_heap(c: &Case, heap: &EmuHeap) -> Result<(), String> {
    let at = c.output("at_heap").u32s();
    if at != heap.blocks.iter().map(|b| b.0).collect::<Vec<_>>() {
        return Err(format!("heap blocks: original at {at:08x?}, port {:08x?}", heap.blocks));
    }
    for (k, &(addr, size)) in heap.blocks.iter().enumerate() {
        let Some(f) = c.try_output(&format!("heap{k}")) else { continue };
        if f.len() != size as usize {
            return Err(format!("heap block {k}: original {} bytes, port {size}", f.len()));
        }
        let mut bytes: Vec<Option<u8>> = vec![None; size as usize];
        for &(a, v) in &heap.poked {
            if a >= addr && a - addr < size {
                bytes[(a - addr) as usize] = Some(v);
            }
        }
        let want: Vec<Option<u8>> = f.written().iter().map(|w| w.map(|v| v as u8)).collect();
        if want != bytes {
            return Err(format!("heap block {k}: bytes written differ"));
        }
    }
    Ok(())
}

fn same_freed(c: &Case, heap: &EmuHeap) -> Result<(), String> {
    let want = c.try_output("freed").map(|f| words(c, f)).unwrap_or_default();
    if want != heap.freed {
        return Err(format!("freed: original {want:08x?}, port {:08x?}", heap.freed));
    }
    Ok(())
}

fn verdict(r: Result<(), String>) -> Verdict {
    match r {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(e),
    }
}

fn eax(c: &Case, name: &str, got: u32) -> Result<(), String> {
    let want = abs(c, &c.output(name).words()[0]);
    if want != got {
        return Err(format!("{name}: original {want:08x}, port {got:08x}"));
    }
    Ok(())
}

/// Every case through the port on a record of its own ([`value_lookup`]) and, since the
/// tenth pass, through the routine's form on the memory image as well.
pub fn lookup(name: &str) -> Option<Check> {
    value_lookup(name)?;
    Some(match name.split('#').next().unwrap_or("") {
        "padSize" => |c: &Case| value_lookup("padSize").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "alignPointer" => |c: &Case| value_lookup("alignPointer").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkMalloc" => |c: &Case| value_lookup("SI_chunkMalloc").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "chunkAllocSlot" => |c: &Case| value_lookup("chunkAllocSlot").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkSaveCurrent" => |c: &Case| value_lookup("SI_chunkSaveCurrent").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkReclaim" => |c: &Case| value_lookup("SI_chunkReclaim").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkInitialize" => |c: &Case| value_lookup("SI_chunkInitialize").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkFreeAll" => |c: &Case| value_lookup("SI_chunkFreeAll").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkDestroy" => |c: &Case| value_lookup("SI_chunkDestroy").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        "SI_chunkAllocate" => |c: &Case| value_lookup("SI_chunkAllocate").unwrap()(c).and(|| crate::pass10::old_chunk(c)),
        _ => return None,
    })
}

fn value_lookup(name: &str) -> Option<Check> {
    Some(match name {
        "padSize" => |c: &Case| {
            let got = pad_size(c.input("p").i32());
            Verdict::from(c.after("p").i32() == got, || format!("port {got}"))
        },
        "alignPointer" => |c: &Case| {
            let got = align_pointer(c.input("p").u32());
            Verdict::from(c.after("p").u32() == got, || format!("port {got:08x}"))
        },
        "SI_chunkMalloc" => |c: &Case| {
            let (mut b, mut heap) = (buffer(c, c.input("buf")), EmuHeap::new());
            let got = b.malloc(&mut heap, c.input("size").i32());
            verdict(eax(c, "eax", got).and_then(|_| same_record(c, &b)).and_then(|_| same_heap(c, &heap)))
        },
        "chunkAllocSlot" => |c: &Case| {
            let (mut b, mut heap) = (buffer(c, c.input("buf")), EmuHeap::new());
            let got = b.alloc_slot(&mut heap, c.input("slot").i32(), c.input("size").i32());
            verdict(eax(c, "ax", got as u32).and_then(|_| same_record(c, &b)).and_then(|_| same_heap(c, &heap)))
        },
        "SI_chunkSaveCurrent" | "SI_chunkSaveCurrent#null" => |c: &Case| {
            let mut b = buffer(c, c.input("buf"));
            let mut mark = None;
            let null = c.try_input("out").is_some_and(|f| f.is_null());
            let got = if null {
                b.save_current(None)
            } else {
                let mut m = 0;
                let before = b.marks;
                let r = b.save_current(Some(&mut m));
                if r != 0 || b.marks != before {
                    mark = Some(m as u32 as u128);
                }
                r
            };
            if !null && c.output("out").written()[0] != mark {
                return Verdict::Different(format!("mark: original {:?}, port {mark:?}", c.output("out").written()[0]));
            }
            verdict(eax(c, "eax", got as u32).and_then(|_| same_record(c, &b)))
        },
        "SI_chunkReclaim" => |c: &Case| {
            let mut b = buffer(c, c.input("buf"));
            let got = b.reclaim(c.input("mark").i32());
            verdict(eax(c, "eax", got as u32).and_then(|_| same_record(c, &b)))
        },
        "SI_chunkInitialize" => |c: &Case| {
            let mut b = buffer(c, c.input("buf"));
            b.initialize();
            verdict(same_record(c, &b))
        },
        "SI_chunkFreeAll" => |c: &Case| {
            let (mut b, mut heap) = (buffer(c, c.input("buf")), EmuHeap::new());
            b.free_all(&mut heap);
            verdict(same_record(c, &b).and_then(|_| same_freed(c, &heap)))
        },
        "SI_chunkDestroy" => |c: &Case| {
            let (mut b, mut heap) = (Some(buffer(c, c.input("buf"))), EmuHeap::new());
            let mut emptied = b.clone().unwrap();
            emptied.free_all(&mut EmuHeap::new());
            chunk_destroy(&mut heap, &mut b);
            if b.is_some() || abs(c, &c.output("pp").words()[0]) != 0 {
                return Verdict::Different("the buffer pointer was not cleared".into());
            }
            // the record is freed, but what was written to it before that is still there
            verdict(same_record(c, &emptied).and_then(|_| same_freed(c, &heap)))
        },
        "SI_chunkAllocate" => |c: &Case| {
            let mut heap = EmuHeap::new();
            let got = chunk_allocate(&mut heap, c.input("count").i32(), c.input("size").i32());
            let want = c.output("eax").u32();
            if want != got.as_ref().map_or(0, |b| b.addr) {
                return Verdict::Different(format!("eax: original {want:08x}, port {:08x?}", got.map(|b| b.addr)));
            }
            if c.output("at_heap").u32s() != heap.blocks.iter().map(|b| b.0).collect::<Vec<_>>() {
                return Verdict::Different("heap blocks".into());
            }
            let Some(b) = got else { return Verdict::Same };
            // calloc zeroed the record; the port's members must be what the memory holds
            // (the block is read as 32-bit words: the busy flag and its padding are one)
            let mem = c.output("heap0").u32s();
            let mut port = record(&b, &vec![0; mem.len() + 1]);
            port.remove(2);
            Verdict::from(mem == port, || format!("record: original {mem:x?}, port {port:x?}"))
        },
        _ => return None,
    })
}
