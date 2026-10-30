# 0024 — One memory image: the model of program state every crate shares

Date: 2026-10-13. Package: `crates/si-mem`, `re/tools/si-mem/`.

**Second pass, the same day: section 11.** The image made large enough for a whole scene:
a second way for the heap to hand out blocks, a heap of up to 1.25 GB that costs the host
what is touched, more than 4,096 handles of a kind, a case loader for buffers at fixed
addresses, the C library's string and memory routines, and a proposal for the one context
type section 8 left open. Sections 1 to 10 are the first pass as it was written; where a
sentence of them is no longer the whole truth it says so and points to section 11.

This note is not about a part of the original. It is about how the Rust port holds the
original's state, so that functions ported in different crates can call each other. It has
four parts: a survey of how every crate holds state today (read from the code, counted by
`re/tools/si-mem/survey.py`), the design of the shared crate `si-mem` with its reasons, a
conversion guide a later engineer can follow crate by crate, and where the work stands.

What is read from code, what is measured and what is judgement is said at each place.
`si-mem` reproduces and cites no function of the original.

## 1. The problem

The original is a C program. Its state is thousands of globals in `.data` and `.bss`, heap
records linked by 32-bit pointers, unions, pointers into the middle of records and arrays,
`void *` fields, function pointers in tables. Twenty-three crates were ported by engineers
working separately, and each chose a way to hold the state it needed. Read from the crates:

- A function of `si-motion` takes `(&mut Mem, Addr, …)`, a function of `si-model` takes
  `(&mut Models, NodeId, …)`, a function of `si-scene` takes `(&mut Dict, ElemId, …)`. None
  can be handed what another produces. `si-motion` therefore reaches `si-model`'s setters
  through a host trait whose methods "take the memory image and then exactly the arguments
  the original passes" (`si-motion/src/host.rs`), with default bodies that do nothing.
- The same original functions are ported twice on different models. Counted by citation
  (library code only): **120 of 2,891 cited functions are cited in more than one crate.**
  The pairs that come from the state model, not from shared helpers: 17 in `si-motion` and
  `si-scene` (the element dictionary's relation queries, the cycle check, the topological
  sort, on the image and on stores), 2 more in `si-motion` and `si-types`, 8 in `si-app` and
  `si-ui` (`newEvent`, `SI_getEvent`, `SI_getMainEvent`, `SI_initEvent`, `SI_fndEvent`,
  `popQE` and two more of `event.c`, on a typed struct of globals and on the image), 6 in
  `si-expr` and `si-scene` (the attribute dictionary's curve index). The lead's reading
  said eleven for `event.c`; by citation it is eight.
- A pointer has five meanings: an address (`si-motion`, `si-ui`), an index plus one into a
  `Vec` (`si-model`'s nodes, `si-kine`'s records), a handle into a per-type arena
  (`si-scene`'s `Arena<T>`), a handle into a heap of an enum (`si-rensetup`'s
  `Heap`/`Obj`), or nothing at all (`si-types`' `Ptr32`, "a placeholder nothing follows",
  with the pointed-to data in a `Vec` beside the record).

## 2. Survey

Columns (all counted by `re/tools/si-mem/survey.py`; its patterns are in the script):
library lines (`src` without `src/bin`; unit tests inside a file are in), lines of check
binaries (`src/bin`), functions cited in library code, and of those how many **touch the
crate's state model**: a file is cut at each doc comment that carries a citation, and a
piece counts when its text names the types or accessors the crate's state lives in. That is
a measure of the code a conversion has to visit, not a judgement of each function. "Marshal"
is the lines of the check-binary files that only move data between harness words and the
crate's model (the "world" builders), which a check on the shared image does not need.

Crates other engineers were editing during the survey (`si-motion`, `si-kine`,
`si-rensetup`, `si-render`, `si-imgfmt`) are counted as they stood at 15:20.

| Crate | State model today | Lib lines | Check lines | Cited | On state | Lines on state | Marshal |
|---|---|---:|---:|---:|---:|---:|---:|
| `si-types` | typed `#[repr(C)]` records, offsets asserted; `Ptr32` never followed; owned stores (`Vec` beside the record); `ModelArena` | 4,122 | 56 | 102 | 94 | 1,915 | 0 |
| `si-model` | `si-types` records in `Models` (pointer = index + 1), a `NodeData` of owned Rust values beside each; meshes as Rust structs (`Mesh`, `Vec` of polygons) | 8,903 | 4,077 | 188 | 167 | 6,007 | 1,360 |
| `si-scene` | `si-types` stores plus its own `Arena<T>` of handles (`Dict`), globals in a Rust struct (`Program`) | 9,403 | 2,732 | 166 | 124 | 6,250 | (in the 2,732) |
| `si-spline` | `si-types` `Spline`/`Patch` record with its arrays in `Vec`s beside it (`Curve`, `Surface`) | 5,046 | 2,005 | 90 | 61 | 3,219 | 356 |
| `si-kine` | `si-model`'s `Models` plus its own `IkStore` (pointer = index + 1), globals in a Rust struct | 3,718 | 1,753 | 78 | 63 | 2,721 | 415 |
| `si-motion` | a sparse raw image (`Mem`: blocks, recorded faults, a stack pointer, the freed list); every offset a named constant; no typed records | 13,225 | 1,645 | 218 | 217 | 9,146 | 0 |
| `si-ui` | a 32-bit image with `.data`/`.bss` at their addresses, an arena and a heap; typed records in place | 6,308 | 315 | 143 | 112 | 4,151 | 0 |
| `si-rensetup` | `si-scene`'s `Dict`, its own `Heap` of an `Obj` enum, a byte `Image` per record that has no typed layout | 3,475 | 384 | 83 | 81 | 2,173 | 0 |
| `si-app` | one typed Rust struct of globals (`App.g`, each field with symbol and address), byte images for five parameter records, `Vec<u8>` blocks from the allocators | 5,530 | 1,368 | 112 | 76 | 3,771 | 0 |
| `si-render` | Rust structs of its own (`Scene`, `Prepared`, `Bsp`, chunk buffers), indices for pointers | 8,369 | 3,338 | 85 | 54 | 4,908 | 2,164 |
| `si-expr` | a tree of Rust nodes, a scene trait | 5,260 | 767 | 126 | 100 | 3,849 | 0 |
| `si-fcurve` | a Rust `Fcurve` with a `Vec<Key>` | 3,266 | 941 | 103 | 96 | 2,449 | 318 |
| `si-gl` | Rust structs per module of the layer (`Context`, `Window`, …), every X11/GLX/GL call behind a trait | 8,355 | 834 | 620 | 304 | 4,698 | 0 |
| `si-ascii` | leaf: text files into values of its own | 16,544 | 632 | 192 | | | |
| `si-elemio` | leaf: element files into values of its own | 8,753 | 906 | 117 | | | |
| `si-hrc` | leaf: model files into values of its own | 4,462 | 351 | 100 | | | |
| `si-db` | leaf: database layout on disk, names, scans | 4,768 | 397 | 107 | | | |
| `si-imgfmt` | leaf: picture formats and operations on pixel buffers | 8,494 | 1,618 | 137 | | | |
| `si-picture` | leaf: picture files | 2,689 | 0 | 63 | | | |
| `si-math` | leaf: values in, values out | 8,454 | 1,075 | 155 | | | |
| `si-x87` | leaf: the x87 model, the vector reader | 2,639 | 137 | 0 | | | |
| `si-file` | leaf: the file layer | 370 | 0 | 13 | | | |
| `si-check` | driver | 120 | 0 | 0 | | | |

Totals: 144,973 library lines, 25,331 lines of check binaries, 1,549 cited functions in
pieces that touch a state model, 55,257 lines in those pieces.

### What each check compares, and what that says about cost

Every stateful crate but two has an emulation check that already moves records between the
harness's memory words and the crate's model. How much code that takes is the cost of *not*
sharing the layout, paid once per crate:

- **`si-motion`** (`motion-emucheck`, 1,639 lines): the vectors carry each buffer's address
  (`a:<buffer>`); the check "rebuilds the same memory at the same addresses, runs the port,
  and requires every word of every buffer" to agree. No marshalling at all: bytes in, bytes
  compared.
- **`si-ui`** (`ui-emucheck`, 315 lines): a scenario pokes bytes at addresses on both sides;
  compared is every outside call with its arguments, the return value and **every byte of
  memory the call changed**. No marshalling. Its engineer's report, confirmed here: set-up
  and whole-memory comparison are the same text on both sides.
- **`si-model`** (`model-emucheck`, 4,077 lines): `world.rs` (820 lines) "follows the same
  pointer words in the vector's inputs and builds node k of a `Models` from them", turning
  pointers into handles; `support.rs` (540) and the per-file checks turn `Mesh`es and nodes
  back into words to compare. About a third of the check is marshalling (1,360 lines in the
  two files that do nothing else).
- **`si-kine`** (`kine-emucheck`, 1,753 lines): `world.rs` (415) rebuilds a chain "in the
  crate's terms: node k is `NodeId` k, its record is record k of the store".
- **`si-scene`** (`scene-emucheck`, 781 + 732 lines): each case's dictionary "is rebuilt
  here as the crate's own structures, the port is run, and the result is laid out again and
  compared word for word".
- **`si-spline`** (`util.rs`, 356 lines), **`si-fcurve`** (`record.rs`, 318 lines): "between
  a vector's fields and the crate's records".
- **`si-render`** (`render-emucheck`, 3,338 lines): the generator walks the emulator's
  memory and writes the tree as numbers; "the port's tree is written out the same way".
  `scene.rs`, `mesh.rs` and `prep.rs` (2,164 lines) are mostly this.
- **`si-rensetup`** (`rensetup-emucheck`, 352 lines; being written): a "canonical form" of
  a made-up world before and after, on both sides: a third notation, written because the
  crate's world has no layout.
- **`si-app`** (`app-emucheck`, 1,368 lines): results, memory written and the trace of
  outside calls; the globals are compared field by field through the typed struct.
- **`si-gl`**: transcripts of outside calls plus "the layer's whole state" printed by both
  sides in a notation of the check. **`si-expr`**: values and printed text.

Reading across: the two crates on an image have no marshalling and compare all memory; the
crates on stores spend 350 to 1,400 lines each on it and compare what the marshalling code
chose to lay out again. A shared image with a loader for the harness's vector format
(`si_mem::load_case`, `Mem::compare_words`) replaces those files by a dozen lines per
routine (section 5.3).

### What converting each takes (judgement, from the reading above)

| Crate | Kind of change | What it takes | Suggested order |
|---|---|---|---|
| `si-ui` | image to image | its `mem.rs` becomes a re-export; accessor names; the fault payload in the check. **Done here** (section 6) | done |
| `si-motion` | image to image | swap `mem::Mem` for `si_mem::Mem`. Counted in its library: about 1,790 scalar accesses (`m.u32(a)`, `m.set_f32(a, v)`, `m.ptr`, `m.or_u8`, …) whose names and meanings are `si-mem`'s on purpose, and 163 `local`/`release` likewise: no change. To change: 78 allocator calls (its `malloc`/`calloc`/`realloc` are the program's wrappers: `si_malloc`, `si_calloc`, `si_realloc`; its `calloc` also answers null for a size of 0, which the original does not), 56 `vector`/`matrix` helpers (they need `si-math`: an extension trait in the crate), about 15 lengths typed `usize` (`bytes`, `is_mapped`), `map_zero` in the check (`place_at`; globals are always mapped). Turn on `set_strict(true)` to keep its faults on overruns and freed blocks; heap addresses move from 0x40000000 to the harness's heap. Hours. Afterwards, record by record, `off::X` + `m.u32` becomes `m[p].x` as `si-types` gains the records: open-ended, and optional | 2 |
| `si-types` | stores to image | records are `Pod` now (section 7). Each store routine (`store.rs`, `reldict.rs`, `attdict.rs`, `enmdict.rs`, `ModelArena`): the `Vec` beside the record becomes the heap array the original has, with the original's allocation calls (sizes and growth read again where the port wrote `Vec::push`). 94 functions, 1,915 lines | 1 (with `si-scene`) |
| `si-scene` | stores to image | on top of `si-types`: `Arena<T>` handles become addresses, `Dict`/`Program` become globals in the image; the 17 functions doubled in `si-motion` are reconciled to one port. 124 functions, 6,250 lines; the check's two layout files go | 1 |
| `si-model` | stores to image | `Models`/`NodeId` become `Ptr32<Model>`; the hard part is `Mesh`, `NodeData`, `Definition`: Rust-owned values that must become the records of `si-types::geom` in heap blocks, with the allocation calls of the original. 167 functions, 6,007 lines; 1,360 lines of check marshalling go | 3 |
| `si-kine` | stores to image | follows `si-model`: `IkStore` becomes heap records (`InvKin` is laid out already). 63 functions, 2,721 lines | 4 |
| `si-spline` | stores to image | `Curve`/`Surface` (record + `Vec`s) become the record with its heap arrays; the key block's "multiple of 16" allocation is already described in the crate. 61 functions, 3,219 lines | 4 |
| `si-rensetup` | mixed to image | its `Image` byte records move into the shared image unchanged (offsets stay named); `Heap`/`Obj` goes; follows `si-scene` | 5 |
| `si-app` | typed globals to image | `Globals` fields become `globals!` entries at the addresses the struct already documents; the five parameter images are byte arrays at fixed addresses already. 76 functions, 3,771 lines; the 8 `event.c` duplicates with `si-ui` are reconciled | 5 |
| `si-fcurve` | Rust structs to image | the 100-byte curve and 36-byte key need typed records (the check's `record.rs` knows the layout); first a boundary adapter (section 5.4), then in place. 96 functions | 6 |
| `si-expr` | Rust tree to image | node layouts are not established; adapter at the boundary until then | 7 |
| `si-render` | Rust structs to image | only its inputs (nodes, materials, lights) need the image; its inner state (chunk memory, tree, samples) is private to the renderer and its checks already compare it by walking; convert the input side with `si-rensetup`, leave the inside | 7, input side only |
| `si-gl` | stays | the layer below the program: its state is its own, the program reaches it only through entry points with scalar arguments and handles; 613 functions, a passing check; no pointer into shared state | stays |
| parsers, picture formats, `si-math`, `si-x87`, `si-file`, `si-db` | stay | values in, values out. One caveat: the original's readers write straight into heap records; the port's readers answer file-content values and a loader of the owning crate (`si-model/src/load.rs` already) writes the records. That boundary is the right one; what a truncated file leaves in memory is then the loader's business | stay |

## 3. The decision

The lead engineer's, built here: **the original's 32-bit memory layout is the interface
between crates.** One image of the program's writable memory in one crate, `si-mem`; a
record is a typed `#[repr(C)]` value at a 32-bit address in it; a ported function takes the
image and addresses, exactly as the original takes pointers; no other crate defines an
arena, an image or a store.

Why this and not typed Rust ownership (judgement, with the survey as evidence):

1. It is the only model in which the ported function has the original's signature, so a
   call from one crate to another needs no translation and a host trait's method can be
   bound to the real function the day it exists.
2. Unions, `void *`, pointers into the middle of arrays and records owned by parts not
   ported yet cost nothing: an address can always be held, cast and compared.
3. Set-up and comparison with the emulated original are the same on both sides, for all
   memory, without per-crate code (the two crates that work this way have no marshalling).
4. The typed records of `si-types` lose nothing: `m[p].field` is as typed as before.

Where reading the code makes me disagree, or add to it, is section 8.

## 4. `si-mem` as built

No dependency on any crate of the workspace. 27 unit tests and 3 documentation tests
(after the second pass: 42 and 4, section 11.7).

### 4.1 The image (`image.rs`)

| Region | Address, size | Content |
|---|---|---|
| `RODATA` | 0x08644c60, 0x3329b | `.rodata`: zero until a crate stores the constants it owns (`set_rom`, `put_rom_cstr`); an ordinary write is a fault |
| `DATA` | 0x08678f00, 0x88e30 | `.data`: zero until a crate writes the initial values of the globals it owns |
| `BSS` | 0x08780c80, 0xb41e8 | `.bss` |
| `ARENA` | 0x61000000, 8 MB | records made by tests and checks: the harness's buffer area (`Sandbox.BUF`) |
| `HEAP` | 0x62000000, 16 MB | what ported functions allocate: the harness's heap |
| `STACK` | 0x7fd00000, 2 MB | locals whose address is passed on; the stack pointer starts at 0x7fef0000 (`Sandbox.ESP0`) |
| `TEXT` | 0x080c51b0, 0x57fa84 | never mapped; a function pointer is looked up, never read |
| `HANDLES` | 0x6f000000, 16 MB | never mapped; addresses of things that are not memory (`Sandbox.FILES`) |

Section addresses are the binary's section headers (`readelf -S`). The gap between `.data`
and `.bss` (`.eh_frame`, `.got`, `.dynamic`) is not mapped: no program code reaches it.
`Mem::with_map(Map { … })` puts the arena, heap and stack elsewhere and chooses a heap
without block headers; `si-ui` uses that because its own driver has the heap at 0x61100000.
(Second pass: `Map::wide()` and `Map::program()` are two larger heaps, 208 MB at the same
start and 1.25 GB at 0x10000000, and a heap larger than 16 MB is not reserved whole:
section 11.1.)

**The binary's data is not embedded.** `.data` and `.rodata` hold only what crates wrote,
each value held against the binary by a script (section 4.6).

**Cost.** Regions are reserved whole but a page is zeroed when first touched (a bitmap of
committed pages per region, sound under a shared borrow because a page is written only
while no reference into it can exist). Measured (`cargo run --release -p si-mem --example
cost`): a fresh image, three blocks, a snapshot and the list of changes, 14 to 17
microseconds per case; a typed or raw access about 4 to 5 nanoseconds. A check can make a
new image for every case; 200,000 cases cost three seconds.

### 4.2 Typed access (`ptr.rs`, `pod.rs`)

- `Ptr32<T>`: the definition moved from `si-types` unchanged in shape (`#[repr(transparent)]`
  over a `u32`, `NULL`, `new`, `is_null`, `.0` public), with what `si-ui`'s `P<T>` had
  (`at`, `addr`, `add`, `cast`, `field`) and `sub`, `offset_from` (C's `p - q`), `byte_add`,
  `elem` (pointer to an array's element), `Eq`, `Ord` (unsigned), `Hash`, `Default`.
  `FnPtr = Ptr32<Code>`, `CStr32 = Ptr32<u8>`, `Opaque` for `void`.
- `field!(p, Record, name)`: `&p->name` as a `Ptr32` of the field's type, the offset from
  the compiler's `offset_of!`.
- `Pod`: every bit pattern a value, no padding, alignment at most 4. Implemented for the
  integers up to 32 bits, `f32`, arrays, `Ptr32`, and the three types that carry the
  i386 rule the host does not share: **`Double`, `LongLong`, `ULongLong` are 8 bytes
  aligned to 4** (`#[repr(C, packed(4))]`). A plain `f64` field would be aligned to 8 by
  the host and shift every later offset; the offset assertions catch it.
- `record! { pub struct Name = "original name", size 0x.. { 0x00 field: type, "what"; … } }`
  defines a record and asserts at compile time its size, its alignment, each offset, that
  the fields tile it and that each field is `Pod` (generalised from `si-ui`'s macro; the
  syntax is the same). `si-types`' `layout!` records get `Pod` from a file of their own
  (section 7).

**Copy or borrow? Both, and both are sound.**

- Copy out, copy in (`m.read(p)`, `m.write(p, v)`, `m.u32(a)`, `m.set_f32(a, v)`, …) works
  at every address, aligned or not, as the processor does.
- Borrowed in place (`m[p]`, `m.get(p)`, `m.get_mut(p)`, `m.slice(p, n)`): a `&T` or
  `&mut T` straight into the image, so `m[p].hilite = 0` is the original's store and
  `m[p].matrix[2][3]` its indexed read. Sound because `T: Pod`, every region starts at a
  multiple of 4 in a page-aligned buffer, and the borrow is of the whole image. What Rust
  then forbids is exactly what would be wrong: holding `&mut` to one record while touching
  another. The idiom for two records is to name them one after the other
  (`let v = m[a].x; m[b].x = v;`) or to copy one out. A view needs the address to be a
  multiple of the type's alignment; one that is not is recorded as a fault
  (`Access::Misaligned`) although the processor would allow it. No record of the original
  is at an odd address unless a test puts it there; fields the original reaches at odd
  addresses (a word inside a byte buffer) are read with the copying calls.

### 4.3 Faults

A read or write not wholly inside one region is recorded (`m.fault()` is the first,
`m.fault_count()` the number) and answers a defined value: zeros for a read, nothing for a
write, a scratch record of zeros for a borrowed view, no elements for a slice. Nothing
panics. No bytes at any address are no access (the original's `memcpy` of nothing).
`set_strict(true)` adds `si-motion`'s stricter rule: inside the arena and the heap only a
placed or allocated block that was not freed is memory.

**Addition to the decision: `OnFault::Stop`.** With it the faulting access unwinds, without
a panic message, carrying a `Stop(Fault)` payload; the driver catches it. This is not a
convenience. `si-ui`'s check compares a faulting case **up to the fault**: the original's
transcript ends in `= stop fault` after the outside calls made before it, and the port must
print the same calls and stop the same way (`compare.py`: "the port must stop the same
way"). In the current run 101 cases end in a fault on both sides and are counted
identical. A port that went on after the fault would log further calls. Section 6 gives
the measurement of what happens to those cases without it.

### 4.4 Allocation (`alloc.rs`)

(This is the default of two policies since the second pass; the other one uses freed room
again and is for whole runs: section 11.1.)

The C library's four routines as the harness's stand-ins behave (`emu.py` `malloc`,
`standins.py`): a bump allocator, 8-aligned, an 8-byte header before each block (the size,
the mark 0x6d656d21), blocks never reused, `malloc(0)` a block of no bytes, `realloc`
always moves, a freed block noted in `freed()` and still readable. **A port and the
emulated original, started alike, get the same addresses in the same order**, so `&heap2`
in a vector file is `m.heap_blocks()[2]`. Where the harness stops a case (a double `free`,
a `free` of no block) the image records a fault (`Access::Free`).

The program's own wrappers, as findings 0010 and 0019 give them and `si-app/src/memory.rs`
ports and cites them on byte blocks, are here on the image, without citation, so that
every crate allocates through one place: `si_malloc` (null for 0; first byte `o`, last
`k`; nothing cleared), `si_calloc` (`calloc` always called, null for a count of 0, a size
of 0 not looked at), `si_realloc` (a size of 0 answers the pointer unchanged).

Fresh memory holds the fill: zeros, or `Fill::Pattern { second, salt }`, a pseudo-random
byte per address whose `second` form is the complement. Two runs of a case with the two
patterns show what the harness's two passes show: a byte that differs depends on memory
nobody initialised; a byte that holds its pattern both times was never written.

The arena: `place(n, align)` as the harness's own `alloc` lays out a case's buffers (a
1 KB gap of fill after each), `place_at(addr, n)` for vectors that carry addresses,
`place_record`, `place_cstr`. The stack: `local(size)`, `release(size)`, `sp()`/`set_sp()`
for a local whose address is passed to a callee.

### 4.5 For checks (`image.rs`, `vecfmt.rs`)

- `snapshot()` and `changed(&snapshot)`: every run of bytes that differs, in address
  order: what `si-ui` prints as `= mem ADDRESS BYTES`. The stack is left out (the harness
  does not report locals); `changed_in(&s, Region::Stack)` gives it.
- The vector format: `parse_words`, `format_words`, `parse_shape` (the spec line's
  `f32[16]` or `(i32,ptr,f32*2)`), and on the image `load_words`, `dump_words`,
  `compare_words` (unwritten words skipped, pointers compared as the address the named
  buffer has here), with `Names` mapping buffer names and `heapN` to addresses.
  `load_case(&mut m, &[(name, words)])` places a case's buffers and stores them with the
  pointers between them resolved (`load_case_at` takes buffers the vector puts at
  addresses, globals among them: section 11.3). `si-mem` does not depend on `si-x87`; its `Word` has the
  shape of `si_x87::vectors::Word`, and a check converts with a three-line function until
  the lead points one at the other.

### 4.6 Globals (`globals.rs`)

`globals! { NAME: type = 0xADDRESS, "symbol"[, init value]; … }` (from `si-ui`'s) makes
each a `pub const NAME: Ptr32<type>` and a table `ALL`; `init_globals(&mut m, ALL)` writes
the scalars. `m[SI_FRAME_CURRENT]` is then the variable for every crate.
`re/tools/si-mem/check_globals.py` holds every `globals!` block of the workspace against
the binary: address and size against the symbol table, initial value against the bytes of
`.data`, and that a global without one lies in `.bss` or is zero in `.data`.

### 4.7 Function pointers (`code.rs`)

A field that holds a function holds the function's address in the Linux binary's `.text`
(`FnPtr`), as the original does, so tables stay comparable byte for byte and a record can
point to a function nobody ported. `Registry<C>` maps an address to a
`Handler<C> = fn(&mut C, &mut Mem, &[u32]) -> Ret`: the original's calling convention
(every argument a 32-bit stack word, in C order; `Ret` has `eax`, `edx` and the ten bytes
of the x87 register for a `float`/`double` result). `C` is whatever the handlers need
besides the image. An address nobody registered: inside `.text` it is a gap of the port
(noted in `unported()`, answers zeros, no fault); outside it the original would have died
(a fault, `Access::Call`).

What wiring the whole program needs beyond this (not built):

1. One context type. Today each crate has its own outside world (`Ui<O: Outside>`,
   `App<W: World>`, `si-motion`'s `Host`, `si-model`'s `Env`). A handler registered by one
   crate must be callable from another, so `C` has to become one program type, or a trait
   object every crate's world implements. This is the next design decision after the
   image, and the registry is generic so as not to pre-empt it. (A proposal, with what
   could be built of it without touching a client: section 11.5.)
2. Thunks. A ported function has a typed Rust signature; a handler takes stack words.
   Each registered function needs a small adapter (`|c, m, a| Ret::int(f(c, m, Ptr32::new(a[0]), a[1] as i16))`).
   A macro can write it from the signature; `si-ui`'s check binary has 150 of these by
   hand in its `dispatch`.
3. Floating-point arguments and results cross as bits; a `float` returned in the register
   unrounded (finding 0008) needs the 80-bit value, hence `Ret::st0` as bytes: `si-mem`
   cannot name `si_x87::F80`.
4. Callbacks from library stand-ins (`qsort`'s comparison function) go through the same
   registry.
5. A registration check: every address against the symbol table, as for globals.
6. `setjmp`/`longjmp` and the program's `exit` (today `si-app`'s `Exit` error value)
   need one convention for all crates.

### 4.8 Handles (`handle.rs`)

`Handles<T>` owns Rust values and gives each an address in the never-mapped range
0x6f000000, by `Kind` (`File`, `Window`, `System`, `Other(n)`: 1 MB each, so an address
says what it is). Files are numbered as the harness numbers them, 0x100 apart: `stdin`,
`stdout`, `stderr` are 0x6f000000, 0x6f000100, 0x6f000200 on both sides. A closed handle's
address is not given out again. Reading through a handle is a recorded fault. (A kind had
4,096 handles; since the second pass it has as many as are opened, the first 4,096 where
they were: section 11.2.)

## 5. Conversion guide

### 5.1 The rule

State that another source file of the original can reach (through a pointer, a global, a
record field) lives in the image at the original's layout. A ported function takes
`&mut Mem` (or `&Mem`) and the arguments the original takes: pointers as `Ptr32<T>` (or
`Addr` where the record has no type yet), numbers as they are. It returns what the
original returns. Rust values are for what the original keeps in registers and for leaf
computations (a vector, a matrix, an `F80`).

### 5.2 A function, before and after

`SI_modelCreateParentship` (0x08477c90), as `si-types/src/model.rs` has it on its arena:

```rust
pub fn create_parentship(&mut self, father: usize, son: usize) -> bool {
    if !self.nodes[son].father.is_null() { return false; }
    let mut p = Some(father);
    while let Some(i) = p {
        if i == son { return false; }
        p = Self::index(self.nodes[i].father);
    }
    match Self::index(self.nodes[father].son) {
        None => self.nodes[father].son = Self::ptr(son),
        Some(mut last) => {
            while let Some(next) = Self::index(self.nodes[last].brother) { last = next; }
            self.nodes[last].brother = Self::ptr(son);
        }
    }
    self.nodes[son].father = Self::ptr(father);
    true
}
```

On the image the record type and its fields are the same; what changes is how a record is
reached, and the index arithmetic goes:

```rust
pub fn create_parentship(m: &mut Mem, father: Ptr32<Model>, son: Ptr32<Model>) -> bool {
    if !m[son].father.is_null() { return false; }
    let mut p = father;
    while !p.is_null() {
        if p == son { return false; }
        p = m[p].father;
    }
    let mut last = m[father].son;
    if last.is_null() {
        m[father].son = son;
    } else {
        loop {
            let next = m[last].brother;
            if next.is_null() { break; }
            last = next;
        }
        m[last].brother = son;
    }
    m[son].father = father;
    true
}
```

The patterns that recur:

| Before | After |
|---|---|
| `models.m(id).field`, `arena.nodes[i].field` | `m[p].field` |
| `Models::ptr(id)` / `Models::id(p)`, handles of an `Arena<T>` | the pointer itself |
| a `Vec<T>` beside a record, `rec.count` kept equal to its length | `rec.items: Ptr32<T>` and `rec.count`; `m[rec.items.add(i)]`, `m.slice(items, n)` |
| `vec.push(x)` | the original's growth, read from its code: `si_realloc` by its chunk size, then the store |
| `Option<Box<T>>` for a pointer that may be null | `Ptr32<T>`, tested with `is_null()` |
| an enum for "what this `void *` points to" | `Ptr32<Opaque>` and `.cast()` where the original casts by a kind field |
| a `String`/`Vec<u8>` name | `CStr32`; `m.cstr(p)`, `m.strdup(p)` |
| a Rust struct of globals | `globals!` entries; `m[NAME]` |
| a local whose address the original passes on | `let p = m.local_record(v); … m.release(size)` |
| a hook or host-trait call into another crate | the other crate's function, same arguments |
| two records borrowed at once | read one field into a value first, or `m.read(p)` a copy |
| `off::FIELD` + `m.u32(a + off::FIELD)` (`si-motion`) | unchanged until the record is typed, then `m[p].field` |

What must be read again in the original: every place the port abstracted memory. Where a
port says `Vec::push`, the original says how many bytes it asks for and when it asks
again; the image form has to make those calls, because the heap's block sequence is now
observable (and comparable: `heap=` in a vector). That reading is the real cost of a
conversion from stores, not the typing.

### 5.3 The emulation check, before and after

Before (`model-emucheck`, shortened): the check turns words into the crate's structures,
calls the port, turns the result back and compares the fields it chose.

```rust
fn mesh_of(c: &Case) -> Mesh {            // 27 lines of this per record kind
    let head = values(c.input("mesh"));
    let mut m = Mesh::default();
    m.angle = f32::from_bits(head[3]);
    for v in values(c.input("verts")).chunks(6) { m.vertices.push(Vertex { pos: [..], flag: v[5] as i16, .. }); }
    …
}
fn order_mesh(c: &Case) -> Verdict {
    let m = si_model::order::order_mesh(&crate::mesh_of(c));
    same_mesh(c, &m, "m.", 4).and(|| same(c, "r.uv", &uv_of(&m)))
}
```

After: the case's buffers go into the arena as they are, the port runs on them, and every
output buffer is held against memory.

```rust
fn words(f: &si_x87::vectors::Field) -> Option<Vec<si_mem::Word>> { /* one match, three arms */ }

fn order_mesh(c: &Case) -> Verdict {
    let mut m = Mem::new();
    let inputs: Vec<_> = c.inputs.iter().map(|f| (f.name.as_str(), words(f))).collect();
    let names = si_mem::load_case(&mut m, &inputs.iter().map(|(n, w)| (*n, w.as_deref())).collect::<Vec<_>>()).unwrap();
    let mesh = Ptr32::new(names.addr(&m, "mesh").unwrap());
    let out = si_model::order_image::order_mesh(&mut m, mesh);          // the port, on the image
    if m.fault().is_some() { return Verdict::different("the port faulted"); }
    for f in &c.outputs {                                               // every buffer the original left
        if let Some(a) = names.addr(&m, &f.name) {
            if let Err(e) = m.compare_words(a, &words(f).unwrap(), &names) { return Verdict::different(e); }
        }
    }
    Verdict::Same
}
```

Three things follow.

- **Keeping every vector file passing with its numbers.** During a conversion both forms
  exist: the new module beside the old one (`order_image.rs` beside `order.rs`), the old
  one in use. The check binary gets a switch (an environment variable or a second lookup
  table) and the same vector files go through both; the conversion of a module is done
  when the image form's row equals the old form's row, case for case, and only then does
  the crate's public name move to the new form and the old module go. A vector file made
  by a generator that walked the emulator's memory in a `post` hook and printed chosen
  fields (`m.verts`, `r.uv`) stays valid: the image check walks the same pointers in the
  image with the same few lines of the generator, in Rust. Regenerating with `heap=TYPE`
  outputs later compares every heap block instead; that changes the numbers (more words
  compared per case), so it is a second step with its own before and after.
- **Not-comparable cases.** A port on stores refuses inputs the original mishandles (reads
  past an array, writes through null) and the check skips them by name. On the image a
  read past a buffer meets fill and a write through null is a recorded fault: run the
  case twice with the two patterns and treat "outputs differ between the runs" as the
  harness's `UNINIT`, "faulted" as its `STOP`. The counts of skipped cases should come
  out the same; where they do not, the difference is a finding about the old port.
- **The mutation switch** (`EMUCHECK_MUTATE=1`) flips a bit of the image where the port
  wrote, before the comparison; `changed()` says where.

### 5.4 Three levels, so that nothing has to wait

A crate whose inner structures have no established layout (`si-fcurve`, `si-expr`,
`si-render`'s inside) does not have to be rewritten before others can call it.

- **Level 0, leaf.** Values in, values out, no pointer into shared state. Stays as it is.
- **Level 1, adapter at the boundary.** The function's public form takes the image and
  addresses; inside it copies the records out into the crate's existing Rust structures,
  runs the existing code and copies the result back. Other crates can call it and the
  state lives in the image between calls. It is honest only if marked (`// adapter: not
  in place; allocation calls not reproduced`), because the heap's block sequence and what
  a fault leaves half-written are not the original's. The existing vector files still
  pass through it unchanged.
- **Level 2, in place.** The function reads and writes the image directly and makes the
  original's allocation calls. Only this one can be compared with the emulated original
  on all memory.

### 5.5 Order

1. `si-types` stores with `si-scene`, reconciling the 17 dictionary functions doubled in
   `si-motion` (decide which port survives per function: the one on the image, lifted
   onto the typed records).
2. `si-motion`: swap the image (hours), then lift offsets as records arrive.
3. `si-model`: nodes first (the record is 75% established), then meshes.
4. `si-kine`, `si-spline`.
5. `si-rensetup`, `si-app` (globals; reconcile `event.c` with `si-ui`).
6. `si-fcurve`: level 1 at once (`si-motion` already copies curves out of its image to
   evaluate them), level 2 later.
7. `si-expr`, `si-render` input side.

Stay as they are: `si-gl`, `si-math`, `si-x87`, `si-file`, `si-db`, `si-picture`,
`si-imgfmt`, `si-hrc`, `si-elemio`, `si-ascii`, `si-check`.

## 6. Proof on a real client: `si-ui`

`crates/si-ui` now stands on `si-mem`. Its `src/mem.rs` (326 lines: the image, the typed
pointer, the `Pod` marker, the snapshot) is 81 lines: re-exports of `si_mem::{Mem, Pod,
Snapshot, Addr, DATA, BSS}` and of `Ptr32` under the crate's name `P`, the crate's two
region constants, and `image()`, which makes an image with this crate's map (arena
0x61000000 and heap 0x61100000 side by side, a heap without block headers: the layout
`re/tools/si-ui/emu_ui.py` gives the emulator) and `OnFault::Stop`. Its two unit tests
are kept in the new terms. `records.rs` is untouched: its `record!` macro implements the
re-exported `Pod`.

The rest is names: 73 lines in eight files (`u32_at` → `u32`, `i16_at` → `i16`, …,
`m.write` → `m.set_bytes`, `m.alloc` → `m.malloc`, `Mem::new()` → `mem::image()`), done by
one script, and in the check binary the test for a fault (the unwinding's payload is a
`si_mem::Stop` where it was a panic message that began with "fault") plus the measurement
switch described below.

| | Routines | Cases | Identical | Different | Not comparable | Unit tests |
|---|---:|---:|---:|---:|---:|---:|
| before (the lead's run of 15:04, and the unchanged crate rebuilt here) | 159 | 27,824 | 27,807 | 0 | 17 | 14 pass |
| after, `re/tools/si-ui/check.sh` whole (15:45 to 15:47; globals: 137 checked, 0 wrong) | 159 | 27,824 | 27,807 | 0 | 17 | 14 pass |

The 17: 5 where the original stops in licence code, 12 where its two runs differ
(uninitialised memory), as before. Beyond the counts: **the port's 24 transcripts are
byte for byte the ones from before** (one checksum over all of them, 501e5bb2… before and
after): every outside call, return value and changed byte of 27,824 cases.

The check's own self-test is unchanged too: with `EMUCHECK_MUTATE=1` (one bit of
everything the port prints flipped) 26,543 cases differ and 1,264 are identical (cases
that print nothing, or stop), before and after, with the same transcripts.

Two measurements on the way:

- **Speed.** The port's side of the check took 15.5 s before and 2.2 s after, for the
  same transcripts. The old image cleared 3.3 MB for every `reset` and copied it for
  every snapshot; the new one touches the pages in use.
- **What stopping at a fault is worth** (`UI_FAULT_RECORD=1` in `ui-emucheck`; a
  measurement, not part of the check). 101 cases end in a fault in the original. With
  faults only recorded and the routine going on, 27,729 cases are identical and **78
  differ**: the port logs outside calls the original never made. No case hung. With
  `OnFault::Stop` all 101 agree. Hence the addition in section 4.3.

Not done in `si-ui`: its `globals!` macro and its `record!` macro are still its own
copies (the same syntax as `si-mem`'s; `check_globals.py` of `si-mem` reads its block as
it is); its gadget handlers are dispatched by its own table, not yet a `si_mem::Registry`;
its map is its own rather than the default one, because its driver script puts the heap
at 0x61100000. None of these changes a number; each is a few lines when wanted.

## 7. `si-types`

Two edits, as allowed, and the two lines that make them reachable.

- `Ptr32` is now `pub use si_mem::Ptr32;` (`src/lib.rs`; the dependency is in the
  manifest). Same name, same shape, the three things it had (`NULL`, `new`, `is_null`)
  and the public `.0`; the `Zero` implementation stays in `si-types` (a local trait on the
  foreign type). `cargo check --all-targets` passes for `si-types`, `si-hrc`, `si-db`,
  `si-model`, `si-scene`, `si-spline`, `si-kine`, `si-elemio`, `si-ascii` and
  `si-rensetup` with no change to any of them.
- `src/pod.rs` (new; `pub mod pod;` added to `lib.rs`): `si_mem::Pod` for all 45 layouts
  and for `si-types`' `Double`. Per record it asserts **at compile time** that the fields
  tile the structure (until now a unit test), that its size is the layout's and its
  alignment at most 4. That every field is itself `Pod` follows from the layout macro,
  which builds a zero value from `Zero::zero()` of every field, and `Zero` is implemented
  only for the integers, `f32`, arrays, `Ptr32`, `Double` and the layouts; the file's
  comment says what must be done if that ever changes. A test holds the list of `Pod`
  records against `all_layouts()`, and another puts two model nodes in the image and
  links them: `m[father].son = son; m[m[father].son].kind`.

`si-types`: 35 unit tests pass (33 before, 2 new). `model-emucheck` over all of
`re/out/si-model/vec`, built against the changed `si-types`: 144,012 cases, 107,228
identical, 29,737 identical given the library's answers, 1 different, 7,046 skipped: the
numbers of finding 0015.

Left for the lead (outside the two edits allowed): `si-types`' `Opaque`, `Code`, `CStr32`
and `Double` are still its own; `si-mem` has types of the same names and shapes. Re-exporting
them, as was done for `Ptr32`, is four lines and makes `Ptr32<Opaque>` one type in all
crates.

## 7a. Second proof: `order.c` of `si-model` in both forms

`crates/si-model/src/order.rs` (630 lines, 17 functions) works on Rust structures: the
file's tables are `Vec`s of structs with `Vec`s inside, the mesh comes in and goes out as
`si_model::Mesh`. `crates/si-model/src/order_image.rs` is the same file on the image, a
new module beside it; **`order.rs` stays the one in use.**

What the image form is: the mesh is an `SI_Mesh` at an address (`si-types`' record), the
three tables are `record!`s of the original's layouts (`polTable[]` 0x24 bytes,
`edgTable[]` 0x10, `vrtTable[]` 0x18; layouts read from the code of the 17 functions),
the file's ten variables are `globals!` at 0x086d6554 to 0x086d6578 (held against the
symbol table and `.data` by `check_globals.py`), every `malloc`, `realloc`, `free`,
`SI_calloc` and `SI_malloc` of the original is made with its size, and the answer is a
new `SI_Mesh` in the heap. Three allocators the routine calls are written out with it
(`SI_meshAllocate`, `SI_vertexAllocate`, `SI_polygonAllocate`; `si-types` has them on
stores).

**The same vector files through both** (`model-emucheck`; `ORDER_IMAGE=1` selects the
image form; four files, 2,000 cases, all `NA:libm:cos` because the mesh allocator asks the
C library for a cosine):

| Form | Cases | Identical | Given the library | Different | Skipped |
|---|---:|---:|---:|---:|---:|
| `order.rs` (Rust structures) | 2,000 | 0 | 2,000 | 0 | 0 |
| `order_image.rs`, the same five output fields | 2,000 | 0 | 2,000 | 0 | 0 |
| `order_image.rs`, also the result's address and the order of frees (`ORDER_IMAGE=2`) | 2,000 | 0 | 2,000 | 0 | 0 |
| either image row with `EMUCHECK_MUTATE=1` | 2,000 | 0 | 0 | 2,000 | 0 |

The third row is what the old form cannot be asked. Each vector carries `eax`, the
address of the new mesh in the emulator's heap (for instance 0x62002c98), and `freed`,
the blocks the routine released in order (`&heap51,&heap36,…`, 57 to 171 of them). The
image form returns the same address and frees the same blocks in the same order in all
2,000 cases, which can only happen if every allocation of the original was made, in
order, with its size. It came out so at the first run; nothing was adjusted. Two things
it shows about the design: `si-mem`'s allocator is the harness's, address for address;
and a table that `realloc` moved under a pointer the original goes on using
(`associatePolygon` keeps its pointer into the edge table across `createEdge`) needs no
special handling: the old block is still there, as in the emulator.

Things the image form reproduces that `order.rs` describes in comments or cannot have:
the three arrays per polygon the original never frees (a unit test counts them live), the
six bytes of the new mesh record nobody writes (they hold the fill), the ten variables
back at zero.

**The measure.**

| | Lines | Of which tests | Cited functions |
|---|---:|---:|---:|
| `order.rs` | 630 | 39 | 17 |
| `order_image.rs` | 1,264 | 89 | 20 (the 17 and three allocators) |
| check, old form (`mesh_of` + `order_mesh`) | 27 + 6 | | |
| check, image form (`order_mesh_image`, both variants, with its comment) | 76 | | |

- **Lines carried over from the old module: none.** This is the finding of the proof.
  The old port had turned the tables into Rust collections, so the sizes, the growth
  steps (200 vertices, 100 edges, 10 to-do entries, 4 polygons per edge) and the order of
  allocations were not in it; the image form was written from the original's 17
  functions (35,000 characters of decompiled text), not from `order.rs`. A conversion
  from stores is a second port of the function, with the first as a description.
- The image form is twice as long because it follows the original statement for
  statement, loops in the compiler's shape included. That is deliberate where pointers
  are re-read after calls; a later pass may fold what is provably the same.
- **Time**: 10 minutes of wall clock for the module and its check (four reading the
  original, four writing, two building and running), by one agent with the original's
  text at hand, compiled at the first build and identical at the first run. Per thousand
  lines of the old module that is about a quarter of an hour; **as a planning number:
  0.25 to 0.5 hours of agent work per thousand lines of old module for integer and table
  code that has vectors**, most of it reading. Three things make other modules cost
  more, in this order: no vectors that carry the heap (then the allocation order is
  unchecked until they are regenerated with `heap=`); records without a layout
  (`si-model`'s `NodeData`, `Definition`); floating-point bodies, whose arithmetic lines
  carry over unchanged but whose every operand load and result store changes.
- Run time of the check: 0.10 s for the 2,000 cases on Rust structures, 0.22 s on the
  image (a fresh image per case, about 100 allocations each).

## 8. Where I depart from the decision, and what is not settled

Departures and additions, each with its reason from the code:

1. **A fault may stop the routine** (`OnFault::Stop`), besides being recorded. The
   decision says a fault "answers a defined value, never panics". That is the default.
   But `si-ui`'s check compares faulting cases up to the fault, and needs the port to
   stop there as the emulator does (sections 4.3 and 6). The stop is an unwinding with a
   typed payload and no message, caught by the driver; nothing else in the image panics.
2. **"No crate defines an arena, image or store of its own" is right for state another
   source file can reach, and too strong beyond that.** `si-gl` (613 functions, a passing
   check that compares its whole state) is the layer below the program; the program
   reaches it through entry points with scalar arguments and window handles, never
   through a pointer into its state. Converting it would cost a re-port of ten source
   files and buy nothing another crate can use. The same holds for the renderer's inner
   structures, the parsers' token state and the arithmetic crates. The rule proposed in
   section 10 draws the line at reachability.
3. **A crate may wrap its existing code in a boundary adapter first** (level 1, section
   5.4), marked as such. Without that, `si-fcurve`, `si-expr` and `si-render` block every
   crate that calls them until their records are established.
4. **`.rodata` is a region too**, read-only, because records hold pointers to string
   literals and constants there; and the stack is one, because the original passes
   addresses of locals. Neither is in the decision's list as such.
5. **Strict blocks** (`set_strict`) are kept as an option. `si-motion`'s image faults on
   an overrun of one block and on a freed block; the harness's memory does not (it catches
   those by comparing two runs with different fill). Both are useful, so the image has
   both, the harness's way as the default.
6. **A conversion from stores is a second port, not an edit** (section 7a): the old
   module could not be carried over line by line. The plan should count such crates at the
   cost of reading the original again, with the first port as a guide and the vectors as
   the judge, not at the cost of a refactoring.

Not settled:

- **The one context type** for calls between crates and through function pointers
  (section 4.7, point 1). The image settles what a pointer is; it does not settle what a
  ported function takes besides the image when it has to call the outside world
  (graphics, files, parts not ported). Today that is a different trait per crate.
  Still not settled; section 11.5 is a proposal for the lead to decide.
- **Who owns a global.** Two crates may declare the same one and the check holds both
  against the binary, so they cannot disagree; but a global's *documentation* then lives
  in two places. A shared `globals` module per original source file, in the crate that
  ports the file, is the tidy end state; `si-ui` declares 137, of which most belong to
  files it does not port.
- **Which of the doubled functions survive.** 17 dictionary functions in `si-motion`
  (on an image, untyped) and `si-scene` (typed, on stores), 8 of `event.c` in `si-app`
  and `si-ui`. Per function: the form on the image, lifted onto the typed records. That
  is the lead's reconciliation; the survey script prints the pairs.
- **Vectors made by `post` hooks** compare what the generator chose to print. They stay
  valid for a converted module (section 7a ran four such files through both forms), but
  only regenerating with `heap=` outputs compares all memory. Whether to regenerate
  per crate at conversion, or later, is a cost decision.
- **`si-x87` and `si-mem` do not know each other.** `si-mem` has a `Word` of the same
  shape as `si_x87::vectors::Word` and each check converts between them in one closure.
  Making the vector reader use `si-mem`'s type (a dependency of `si-x87` on `si-mem`)
  removes that; it is an edit to `si-x87`, not made here.
- **The fill pattern is not the harness's bytes**, only made the same way (two
  complementary patterns, shifted per case). That is enough to find dependence on
  uninitialised memory; it cannot reproduce a result that depends on it, and nothing
  could.
- **Misaligned borrowed views** are faults although the processor allows them. No ported
  code met one. If a record of the original turns out to lie at an odd address, its
  accesses must use the copying calls.
- **A big-endian host** is refused at compile time.
- **The ledger.** `order_image.rs` cites the 17 functions of `order.c` a second time and
  three allocators `si-types` already cites. The ledger counts a function once, so the
  count does not change; the lead may want the image forms marked.

## 9. Where the work stands

Done, with every crate touched compiling and its tests passing:

| Item | State |
|---|---|
| `crates/si-mem` | built: 2,900 lines, 27 unit tests, 3 documentation tests; no dependency in the workspace. After the second pass: 5,240 lines, 42 unit tests, 4 documentation tests (section 11) |
| `re/tools/si-mem/survey.py` | the survey's counts; `re/out/si-mem/survey.tsv` |
| `re/tools/si-mem/outside_survey.py` | second pass: what each converted crate calls outside itself, and how (section 11.5) |
| `re/tools/si-mem/check_globals.py` | every `globals!` block against the binary: 150 globals in three crates, 0 wrong; flips caught (a wrong address and a wrong initial value were tried) |
| `crates/si-ui` | on `si-mem`: 159 routines, 27,824 cases, 27,807 identical, 0 different, 17 not comparable; 14 unit tests; transcripts byte-identical |
| `crates/si-types` | `Ptr32` re-exported from `si-mem`; `pod.rs`; 35 tests; all nine dependents check |
| `crates/si-model` | `order_image.rs` beside `order.rs` (the old one in use); dependency on `si-mem`; 58 unit tests; `model-emucheck` whole: 144,012 cases, 107,228 / 29,737 / 1 / 7,046 as in finding 0015, with the old form and with `ORDER_IMAGE=2` |

Not done:

- No crate other than `si-ui` is converted; `si-model` has one module in both forms.
- `si-motion`'s swap to the shared image (section 2, order 2) is described, not tried:
  an engineer was in the crate.
- `Registry` and `Handles` have unit tests and no client yet.
- The loader for vector files is used by one check (`order_mesh_image`); `dump_words` and
  `parse_shape` by tests only.
- `si-types`' `Opaque`, `Code`, `CStr32`, `Double` are not yet re-exports (section 7).
- `si-app`'s allocator routines keep their citations; the image has the same three
  without (section 4.4).

How to run everything of this package (the second pass's own commands are in section
11.9):

```bash
export CARGO_TARGET_DIR=$PWD/target/si-mem
(ulimit -v 8000000; cargo test -p si-mem -p si-types -p si-ui -p si-model)
(ulimit -v 8000000; cargo build --release -p si-mem --example cost -p si-model -p si-ui)
(ulimit -v 2000000; target/si-mem/release/examples/cost)
(ulimit -v 2000000; target/si-mem/release/model-emucheck re/out/si-model/vec)                 # order.rs in use
(ulimit -v 2000000; ORDER_IMAGE=2 target/si-mem/release/model-emucheck re/out/si-model/vec)   # order.c on the image, with eax and freed
re/tools/si-ui/check.sh                                                                      # si-ui on si-mem, about two minutes
python3 -I re/tools/si-mem/check_globals.py
python3 -I re/tools/si-mem/survey.py
```

## 10. The rule proposed for `AGENTS.md`

To replace the "In-memory data" convention:

> **In-memory data: one image.** The program's state lives in `si-mem`'s memory image at
> the original's 32-bit layout: globals at their addresses in `.data`/`.bss`
> (`globals!`), heap records as typed `#[repr(C)]` records with asserted offsets
> (`record!`, or `si-types`' layouts) at 32-bit addresses, pointers as `Ptr32<T>`,
> function pointers as the original's code addresses. A ported function takes the image
> and the arguments the original takes, and makes the original's allocation calls. No
> crate defines an arena, a memory image or a store of its own for state that another
> source file of the original can reach through a pointer or a global. Exempt: leaf
> libraries that take and return values (file parsers, picture formats, the mathematics,
> the x87 model) and the private state of the layer below the program (`si-gl`). Where a
> record's layout is not established yet, address it by named offsets in the same image
> (`m.u32(p + off::FIELD)`) and say so. A crate not yet converted may wrap its code in an
> adapter that copies records out of the image and back, marked `// adapter: not in
> place`; it counts as converted only when the adapter is gone. An emulation check loads
> a case's buffers into the image (`si_mem::load_case`) and compares memory
> (`compare_words`, `changed`); it does not rebuild the case in structures of its own.

## 11. Second pass: large enough for a whole scene (2026-10-13)

Why: every ported routine keeps its state in this crate's image, and that worked for one
routine at a time. It did not work for a scene. The engineer moving the scene loader onto
the image stopped (finding 0018, section 13.3) because the heap never used a block again
and was 16 MB, while the dictionaries grow every array by one record per addition, each
growth a new block, and because a kind had 4,096 handles. This pass removes those two
limits and adds three things clients had written for themselves or missed (findings 0026
section 9, 0017, 0023).

Nothing here reproduces a function of the original: the program takes its memory from
the C library, so the allocator is engineering of ours and carries no citation. What is
**measured** below was run; what is **judgement** says so. Every change is an addition:
no name, signature or behaviour a client used was changed, and section 11.7 is the proof.

### 11.1 The heap: two policies, chosen when the image is made

| | `HeapPolicy::Harness` (the default) | `HeapPolicy::Reuse` |
|---|---|---|
| Made by | `Mem::new()`, `Mem::with_map(map)`, `Mem::with_heap(map, HeapPolicy::Harness)` | `Mem::program()`, `Mem::with_heap(map, HeapPolicy::Reuse)` |
| For | every check compared with the emulated original: a block's address is the harness's | a whole run of the program; addresses are this allocator's own and are compared with nothing |
| `malloc` | the next room at the top, as before, bit for bit | the smallest free span that fits (what is left over stays free when it is 16 bytes or more), else the top |
| `free` | noted in `freed()`; the room is never used again | the room is free; joined with free neighbours; given back to the unused part when it ends at the top |
| `realloc` | always a new block | in place when it shrinks, when it fits the room the block has, when the block is the last one, or when the span behind it is free; otherwise it moves, and takes half its size again as room to grow into |
| History | `heap_blocks()` (the harness's `heap0`, `heap1`, …), `freed()`, `heap_block_of()` | none kept: those three are empty. `live_blocks()`, `live_block_of()`, `block_size()`, `is_freed()` answer under both |

Common to both: addresses are multiples of 8 after the 8-byte header (size and mark,
written under both; none under a map with `heap_header: 0`); `malloc(0)` is a block of no
bytes with an address of its own (except, as before, under the harness's policy in a map
without headers, where it shares the next block's); the heap's end answers null and a `realloc` that finds
no room leaves the block as it was; a `free` or `realloc` of what is no live block is a
recorded fault; **fresh bytes hold the fill** (`Fill`), also the bytes a `realloc` adds
and also where the room was used before. From the last point: with the zero fill the
bytes of every live block are the same under both policies (a test runs one made-up
program of 12,000 calls under each and compares all blocks). The bookkeeping of `Reuse`
(live blocks, free spans by address and by length) is kept beside the image in Rust
collections, not in it: 12 bytes of content a live block, somewhat more with the
collection's own overhead (not measured).

The room to grow into is what makes the dictionaries' pattern affordable in time as well
as in room: a block grown one record at a time is moved a number of times that goes with
the logarithm of its size. It is invisible to the program (a block's size is what was
asked for; strict blocks hold accesses to that).

`Mem::heap_stats()` answers a `HeapStats`: bytes from the heap's start to its top now
(`used`) and at most (`peak`), live blocks and bytes, free bytes, and how many `realloc`s
stayed in place or moved with the bytes copied. `Mem::heap_verify()` holds the
allocator's tables against each other and against the headers in the image (a routine
that writes over a header is found by it). Two things in the default policy changed
without changing an answer: whether a block was freed is looked up in a set (it was a
scan of the list of frees, in `free`, `realloc` and under strict blocks in every access),
and the counts for `heap_stats` are kept.

**Room beyond 16 MB.** Two maps: `Map::wide()` has the heap `HEAP_WIDE` (0x62000000, 208
MB: the harness's start, so every block gets the address it gets in the default map, up
to the handle range), `Map::program()` has `HEAP_PROGRAM` (0x10000000, 1,280 MB, in the
free range between the binary's sections and the arena). **A heap larger than 16 MB is
backed in steps** (`image.rs`, `Seg::reach`): nothing is taken from the host until the
first block; then one buffer covers the heap from its start to a little beyond the
highest block so far, doubling from 64 KB until the steps are 64 MB. The buffer is grown
with the host's `realloc` at an alignment the C library's own `realloc` serves, which
remaps the pages of a large block instead of copying them (measured below). Only the
part reached is memory: an access above it is a fault, as above a process's break. That
is the one difference in behaviour from a heap reserved whole, where a wild read above
the last block answers zeros; the default 16 MB heap is reserved whole as before, so no
existing check sees it. When the host refuses more address space, `malloc` answers null;
nothing aborts (run under a cap of 700 MB: null after 664 MB).

One new `unsafe` block, in `Seg::reach`: the calls of `alloc` and `realloc` for the
growing buffer, with the argument at the spot (the pointer and layout are the ones the
buffer was made with; the region is borrowed `&mut`, so no reference into it exists
while it may move). `Drop` frees with the alignment the buffer was made with.

**Measured: the loader's pattern** (`cargo test -p si-mem the_loaders -- --nocapture`).
The pattern is what findings 0026 (1.3) and 0018 (13.3) describe: 36 chapters (the
chapter table's rows), six of 10,000 to 30,000 elements and thirty of 1,000, 150,000
elements in all, the chapters taking turns; each element grows its chapter's array by
one record of 0x84 bytes (`SI_malloc(0x84)`, then `SI_realloc(…, (n + 1) × 0x84)`) and
brings five blocks that stay: a name record of 0x34 bytes, three strings of 1, 2 and 2
bytes, a name of 6 to 19 bytes. Sizes in units of 2^20 bytes.

| | Heap needed | Host memory touched | Outcome |
|---|---:|---:|---|
| Harness policy, counted without memory (the count is held against the image at 1% of the pattern: 1,500 elements, 18.1 MB, equal to the byte) | 178,157 MB | every byte of it | cannot be run: beyond a 32-bit address space |
| Harness policy, heap of 16 MB (default) | | | used up after 2,967 of 150,000 elements |
| Harness policy, heap of 208 MB (`HEAP_WIDE`) | | | used up after 10,849 |
| Harness policy, heap of 1,280 MB (`HEAP_PROGRAM`) | | | used up after 26,999 |
| Reuse policy, on the image (`Mem::program()`) | **48.7 MB** at most, for 28.7 MB live in 750,036 blocks | 46.1 MB (75.4 MB of address space held) | all 150,000; 550 of 149,964 reallocs moved a block (48.3 MB copied); 1.4 s in a debug build; every element, name record and name read back |

The corpus's largest chapter (finding 0018: 2,593 elements, the element array alone):
443,957,900 bytes under the harness's policy, counted (the "444 MB" of that finding; 423
in units of 2^20); **0.33 MB** under `Reuse`, measured.

**Measured: the largest image and what it costs the host** (`examples/heap.rs`, run under
`ulimit -v 2000000`). A fresh image with `Map::program()`: 11.4 MB of address space held
(the regions that are reserved whole), nothing touched. After 24 MB written and nineteen
blocks of 64 MB handed out with one page of each written: 1,240 of the 1,280 MB handed
out, then null; 24.1 MB touched; the process's resident memory 26.8 MB at most and its
address space 1,257 MB at most (the system's own figures); the slowest `malloc` 0.14 ms
(3 ms in a run beside four emulators): the buffer grew to 1.2 GB without a page being
copied. So: **a heap of 1.25 GB is
possible under the 2 GB cap; the host pays the pages touched plus address space equal to
the heap's reach** (at most 64 MB beyond the highest block). A map may declare any heap
that fits its free range (below the arena there are 1,296 MB); nothing larger was tried.

**Cost for checks, unchanged** (`examples/cost.rs`, the crate as it was and as it is, run
in turn on a shared machine): a fresh image with three blocks, a snapshot and the list of
changes 13.8 then 13.6 microseconds; an access 2.5 then 2.4 nanoseconds. (The
allocator's tables are behind a box in `Mem`: inline they made an access a tenth slower,
measured, by pushing apart the fields every access reads.)

### 11.2 Handles: as many as are opened

`Handles::open` no longer answers null after 4,096. The numbering of the first 4,096 of
a kind is unchanged (`base + n × 0x100`: `stdin` is still 0x6f000000). The next 126,976
lie between those, 8 apart, still in the kind's megabyte (the 4,097th is `base + 8`);
after those 131,072 the addresses come from a second range that is never mapped either,
`HANDLES_HIGH` (0xc0000000 to the end of the address space: in the original's 32-bit
process the system's quarter), 64 MB a kind, 8 apart: 8,388,608 more. `Kind::nth(n)` and
`Kind::number(addr)` are the rule in both directions, `Kind::of(addr)` knows both
ranges, `Kind::MAX` is the limit; `Handles::len()` counts what was opened. A test opens
140,000 of one kind and holds every address against the rule.

### 11.3 `load_case_at`: buffers where the vector puts them

`load_case_at(&mut m, &[(name, Option<address>, Option<words>)])` (the field type is
`FieldAt`). A field without an address is placed as `load_case` places it. With one: in
the arena it becomes a placed buffer there (`place_at`: a routine may `free` and
`realloc` it; fields placed later go behind it); in `.data`, `.bss` or on the stack the
words are stored there and nothing is placed; in `.rodata` they are stored as constants
(`load_rom_words`, new, is `load_words` with `set_rom`); in the heap or where there is no
memory it is an error (a heap block comes from `malloc`, in the routine's order). Then
all buffers are filled with one table of names, so a global can point at an arena buffer
and the other way round. This is the loop `dict-emucheck` and `motion-emucheck` each
wrote for themselves (finding 0026, 9.1; finding 0017) and what the render set-up check
was designed around (finding 0023: "the world lies at a fixed address of the test-buffer
region").

### 11.4 The C library's string and memory routines: `si_mem::clib`

Free functions on the image, addresses as `u32`: `strlen`, `strcpy`, `strncpy`,
`strcat`, `strncat`, `strcmp`, `strncmp`, `strcasecmp`, `strncasecmp`, `strchr`,
`strrchr`, `strstr`, `strspn`, `strcspn`, `strdup`, `strtok` (the library's hidden
variable is the caller's), `strtok_r`, `memcpy`, `memmove`, `bcopy`, `memset`, `bzero`,
`memcmp`, `toupper`, `tolower`, `strtol`, `strtoul` (any base, the end pointer stored),
`strtol10`, `atoi`, `decimal`, `qsort`, `bsearch`. Each behaves as the harness's stand-in
of the same name (`re/tools/emu/standins.py`, read for this): comparisons answer −1, 0,
1; `memcpy` copies as `memmove`; `strtok` leaves null as the saved position at the end;
`qsort` is the merge sort that asks about elements in the array itself in a fixed order;
`bsearch` compares with the middle of what is left. Where they go beyond the stand-ins:
a comparison reads no further than the first difference (the stand-in reads both strings
whole first, so it can stop a case the C routine would survive), and a count that is
nonsense allocates nothing (`strncpy` with 0xfffffff0 is a fault where the zeros would
go, `qsort` of an array that is not memory is a fault).

The ten that `si-dict` has in `crates/si-dict/src/libc.rs` (`strcmp`, `strncmp`,
`strcasecmp`, `strcpy`, `strcat`, `strchr`, `strrchr`, `qsort`, `strtol10`, `decimal`)
have the same names and signatures here. **Measured** (a program outside the repository
that includes that file beside this module): 20,000 rounds of made-up strings and arrays
through both, every answer equal, `qsort`'s questions equal in order, and the two images
equal byte for byte afterwards. One difference by design, not reachable in that run:
`si-dict`'s comparisons stop after one byte once any fault has been recorded in the
image; these do not look at earlier faults.

Not here: `printf` and `scanf` and their families (a formatter on the image is a piece
of work of its own; finding 0026 lists it), the streams, `getenv`.

### 11.5 The one context type: a proposal

Section 8 left open what a ported function takes besides the image when it has to call
graphics, files or parts not ported. **Survey** (read in the four converted crates;
counted by `re/tools/si-mem/outside_survey.py` over their declarations, as they stood at
18:20; `si-motion` and `si-rensetup` were being edited):

| Crate | Its outside world today | Callees |
|---|---|---:|
| `si-ui` | 16 traits by subsystem (`sys.rs`, one macro that also generates a recording implementation and is read by `emu_ui.py`); a struct `Outside` of 16 boxed trait objects; typed methods under the original's names, the image first | 224, of which 19 take Rust references (outputs, arrays, strings by content) and 3 take floats |
| `si-motion` | 21 traits by source file (`host_trait!`), chained as supertraits; typed methods with default bodies that answer zero; a `Trace` that records the name and `Arg::{I, F, D}`; results `Ret { eax, st0: F80 }` | 335, of which 74 take or answer floats |
| `si-dict` | one trait `Outside` with one method for calls: `call(m, &'static Routine { name, addr }, &[u32]) -> u32`; `call_addr` for a function pointer; `getenv`, `stdout`, `stderr`; a typed wrapper function per callee | 18 |
| `si-rensetup`, `native` | one trait `Outside` with one method: `call(m, FnPtr, &[u32]) -> u32`; `Registered` makes a `si_mem::Registry` an implementation | 69 (72 an hour earlier) |

All four pass the image first and then the original's arguments. None can hand its
outside world to a function of another crate. Two arrived separately at the same shape:
the original's own way of calling.

**The shape proposed** (judgement):

1. One trait, `si_mem::Outside`, with one method: `call(&mut self, m: &mut Mem, routine:
   Routine, args: &[u32]) -> Ret`. A callee is named by its address in the binary (and
   its name, for logs); the arguments are the 32-bit words the caller pushes, in C order
   (a `float` its bits, a `double` two words, a structure by value its words); the answer
   is the registers (`eax`, `edx`, the ten bytes of the x87 register). Beside it `print(
   stream, text)` for what the program prints, already formatted (formatted output is
   variadic and has no words to compare; the checks compare text).
2. A ported function that calls outside is `fn f(m: &mut Mem, o: &mut dyn Outside, …the
   original's arguments…)`; one that does not takes the image only.
3. The typed names the code calls stay. They are an extension of the one trait, declared
   per source file with `outside_calls!`: one line per callee with its arguments, its
   result, its name and its address; every `Outside` then has the methods.
4. Three implementations serve every use: `Wired` (ported functions registered by
   address in front of a rest; a port is run with the same value as its own outside
   world, so a call goes on from crate to crate, and porting a package means registering
   its functions), `Trace` (records every call and answers through a closure: unit
   tests, routines nobody ported, a check replaying the emulator's answers and comparing
   the calls), `Registered` (a registry whose handlers have a context of their own).
5. What it asks of the callers: an output parameter is an address in the image (a local
   from `Mem::local`), never a Rust reference; a `float` result is read from `Ret::st0`
   by the caller's crate with `si-x87`'s type (`si-mem` cannot name it).

Not in the proposal: one convention for `setjmp`/`longjmp` and the program's `exit`
(section 4.7, point 6), and thunks from typed ports to handlers written by a macro (point
2 there; `Wired`'s registration is a closure by hand).

Alternatives looked at: one typed trait for the whole program (a thousand methods that
every crate depends on and every port changes); a context type parameter on every
function (every crate must agree on it, which is the same trait by another road);
keeping the traits per crate with adapters between pairs (their number grows with the
square of the crates).

**Cost per crate** (judgement from the counts above; none of it was tried, by the rules
of this pass):

| Crate | What changes | Size |
|---|---|---|
| `si-rensetup`, `native` | the trait and `Registered` become re-exports; the 69 `callees!` constants become `Routine`s (the macro's body); `call` answers `Ret`, so about 30 call sites read `.eax` | smallest; an hour |
| `si-dict` | the trait and `Routine` become re-exports; the 18 wrappers read `.eax` or become one `outside_calls!` block; its two implementations (`NoOutside`, the check's) take `Routine` by value and answer `Ret`; `getenv` becomes a call of the binary's stub or stays a helper of the crate; `stdout`/`stderr` become `print` | about 80 lines; `dict-emucheck` must give its numbers again |
| `si-motion` | `host_trait!`'s body is rewritten once into the extension form; each of the 335 declarations gets its address (they carry names only: a script over the symbol table, with care for the snake-case names); `Trace` and `Arg` become `si-mem`'s (floats as words); the 229 bounds `H: …Host` in the code keep working, because every `Outside` has every such trait; the 74 float methods and the check's recorded-call format have to be gone through | a day; 325,200 cases to run again |
| `si-ui` | the same treatment of `outside!` and 224 declarations with addresses; the struct of 16 boxes becomes one `&mut dyn Outside`; the 19 methods with Rust references must take addresses, so their callers change (locals on the image's stack) and `emu_ui.py`, which reads the declarations, with them | the largest; the 27,824 transcripts must stay byte for byte |
| `si-model` (`Hooks`), `si-app` (`World`), `si-scene` (`Unported`), `si-expr` | the same pattern when each is converted; not surveyed here | |

**Built**, in `si-mem` only and used by no crate: `outside.rs` (`Routine`, `Outside`, the
alias `World = dyn Outside`, `Trace`, `Wired`, `Registered`, the argument and result
conversions `Words` and `FromRet`, `outside_calls!`), with two tests in which a function
of one made-up package calls one of another through the outside world, first unported and
then registered; and `Registry<C>` now takes a context that is a trait object. The
decision is the lead's; if it goes another way the module can be deleted without
touching anything else.

### 11.6 What a client changes to use each addition

| Crate | For a large heap | For handles | For `load_case_at` | For `clib` |
|---|---|---|---|---|
| `si-scene` (the loader's move) | make the image with `Mem::program()`; the ported functions are unchanged. Do not read `heap_blocks()`, `freed()` or `heapN` names in a whole run: they are the harness policy's history | use `si_mem::Handles` for objects and relation data instead of the draft's own numbering (`re/out/si-scene/draft/dict_image.rs`); nothing else | `scene-emucheck` on the image: one call with the globals (`cid` and the others) at their addresses | as needed |
| `si-dict` | nothing (its checks stay on the default) | nothing | optional: the placement loop of `dict-emucheck` (about 20 lines) becomes one call with `global_addr(name)` as the address | `libc.rs` becomes `pub use si_mem::clib::*;` (same ten names and signatures) |
| `si-motion` | nothing | nothing | optional: `motion-emucheck`'s `image()` (the `load_case` attempt, the `place_at` fall-back and the globals, about 70 lines) becomes one call with every recorded address; under its strict blocks the memory is the same | its own string helpers, if it has any, as needed |
| `si-rensetup` | `Mem::program()` for a whole frame's set-up, like `si-scene` | `Handles` for what its `Heap`/`Obj` holds until those are records | the check it designed (finding 0023, 16.4): the world at its fixed address | `strstr` and the others it lists as "not behind the trait" |
| `si-ui`, `si-model`, `si-types` | nothing | nothing | nothing | nothing |

A check that needs more than 16 MB **in the harness's order** (compared with an emulator
run whose own heap was made larger) uses `Mem::with_map(Map::wide())`: same addresses,
208 MB.

### 11.7 Tests, and the proof that nothing moved

New unit tests (15; made-up data only) and one documentation test:

| File | Tests |
|---|---|
| `alloc.rs` | the reuse policy step by step (reuse, in place, moving, shrinking, joining, the empty heap, a placed buffer); under strict blocks and a pattern fill; in a 4 KB heap without headers (null when full, a block that cannot grow stays); one program under both policies with all blocks compared; a heap backed in steps (the harness's addresses beyond 16 MB, 24 MB handed out for five pages touched, a fault above the reach, a snapshot across a step, null at the end); the loader's pattern (11.1) |
| `handle.rs` | 140,000 handles of one kind |
| `vecfmt.rs` | a case with a global, a constant, a buffer at an arena address, a local and a null field; the four refusals; equality with `load_case` when no field has an address |
| `clib.rs` | comparisons; copies and searches; tokens; numbers; sorting and searching with the order of the questions |
| `outside.rs` | calls as words at addresses; ported functions wired by address |

`cargo test -p si-mem`: 42 unit tests and 4 documentation tests pass (27 and 3 before);
the test binary alone under `ulimit -v 2000000`: 42 pass, 107 MB resident at most.

The two clients nobody was editing, run before the first edit and after the last (the
check binaries rebuilt from the final sources; that was verified by cargo's own account
of what it rebuilt, after one run turned out to have used a binary older than the last
edit and was thrown away):

| Check | Before | After |
|---|---|---|
| `re/tools/si-ui/check.sh` | 159 routines, 27,824 cases: 27,807 identical, 0 different, 17 not comparable | the same; the script's output identical but for the times; **the port's 24 transcripts byte for byte the same** (one checksum over all, 2f92ee51… before and after) |
| `dict-emucheck re/out/si-dict/vec` | 186 routines, 15,726 cases: 15,217 identical, 346 given, 0 different, 163 not comparable | the same; **output byte for byte** the run before and the hand-over's `re/out/si-dict/check.txt` |
| the same with `EMUCHECK_MUTATE=1` | 14,584 different (the binary built before the pass) | 14,584 different; output identical to that binary's |
| unit tests of `si-dict`, `si-ui` | | 3 and 14 pass |
| `cargo check --all-targets` of `si-types`, `si-model`, `si-scene` | | compile unchanged |
| `check_globals.py`, the five crates nobody was editing (`si-dict`, `si-mem`, `si-model`, `si-scene`, `si-ui`) | 191 checked, 1 wrong | 191 checked, 1 wrong |
| `check_globals.py`, all crates | 221 checked, 2 wrong | 223 checked, 3 wrong (`si-rensetup` went from 30 declarations to 32 meanwhile) |

The globals check was never clean, and none of its complaints is in `si-mem`. The one
that stays: `si-ui` declares `SI_ELEM_DICTS` without an initial value and `si-dict` with
`init 0`; that is a false alarm of the script's rule (no value and zero are the same
statement for `.data`), left alone in this pass so that the yardstick stayed the same
from before to after. The other two are `si-rensetup`'s, whose engineer was editing it:
`SI_GLB_MANIP` declared as 4 bytes where `si-dict` declares the whole 0x3c, and, new
during the pass (its `globals.rs` changed at 18:04), `SI_IDENTITE`, a matrix the crate
fills itself, which the script wants listed in its `BUILT` table or given a value.
`si-motion`, `si-rensetup` and `si-render` were not built.

### 11.8 Not done

- **No client was changed** (the rule of the pass): `si-dict` still has its own
  `libc.rs`, both checks their own placement loops, and nothing uses `Reuse`, `Handles`
  beyond 4,096 or `Outside` yet.
- **No scene was loaded on the image.** The loader is not on the image; what was run is
  the replayed pattern of 11.1, not a real description. The room a real scene needs
  under `Reuse` is therefore an estimate from that pattern (about 1.7 times what is
  live).
- **Under `Reuse` a vector's `heapN` and `freed=` cannot be compared**: the policy keeps
  no history. A routine is checked under the default policy and run at scale under the
  other; that the two leave the same bytes in every block is tested for the allocator,
  not for any ported routine.
- **`Mem::cstr` under strict blocks** still tests only the first byte against the block
  and then reads to the end of the page (finding 0017 noticed it). Fixing it changes
  what `si-motion`'s strict check sees, and its engineer was at work; `clib`'s
  comparisons read byte by byte and are held to the block.
- **Snapshots under a pattern fill** (finding 0026, 9.4): `changed` still takes a page
  never touched as zero.
- **`printf`/`scanf` on the image, `getenv`, the streams**: not here (11.4).
- **`si_x87::vectors::Word` and `si_mem::Word`** are still two types.
- **The context type is a proposal** (11.5): built in `si-mem`, tried on no crate.
- **The stack** of `Map::program()` is still the harness's 2 MB (a map can declare
  another); what a whole run needs was not measured.
- **A heap above 1.28 GB**, and a second process-like region (the original's `mmap`
  range), were not tried. A run that touches more than the host has is the host's
  problem as for any program: only address space is refused gracefully.
- **`check_globals.py`** was not changed (11.7).

### 11.9 Commands

```bash
export CARGO_TARGET_DIR=$PWD/target/si-mem
(ulimit -v 8000000; cargo test -p si-mem)                                   # 42 + 4
(ulimit -v 8000000; cargo test -p si-mem the_loaders -- --nocapture)        # the table of 11.1
(ulimit -v 8000000; cargo build --release -p si-mem --examples)
(ulimit -v 2000000; timeout 120 target/si-mem/release/examples/heap)        # the largest image and its cost
(ulimit -v 2000000; timeout 120 target/si-mem/release/examples/cost)        # what a check pays per case
# the two clients nobody edits, and the globals
(ulimit -v 8000000; cargo build --release -p si-dict)                       # not with --examples: that builds no binary
(ulimit -v 2000000; timeout 600 target/si-mem/release/dict-emucheck re/out/si-dict/vec | cmp - re/out/si-dict/check.txt)
re/tools/si-ui/check.sh                                                     # about two minutes; builds into target/si-ui
python3 -I re/tools/si-mem/check_globals.py
(ulimit -v 8000000; cargo test -p si-dict -p si-ui; cargo check --all-targets -p si-types -p si-model -p si-scene)
python3 -I re/tools/si-mem/outside_survey.py                                # the counts of 11.5
```

Scratch of this pass, in `re/out/si-mem/`: the checks' outputs before and after
(`dict-before.txt`, `dict-after.txt`, `ui-check-before.log`, `ui-check-after.log`, the
checksums of the transcripts, `globals-before.txt`, `globals-after.txt`), the crate's
sources as they were (`before-src/`) and the check binary built from them
(`dict-emucheck.before`), and the program that held `clib` against `si-dict`'s own
routines (`clibdiff/`, a crate of its own outside the workspace: `cd
re/out/si-mem/clibdiff && CARGO_TARGET_DIR=$OLDPWD/target/si-mem/clibdiff cargo run
--release`; it stops being meaningful the day `si-dict` re-exports `clib`).

## 2026-10-28: `si-expr` scanner and clause code on the image

`si-expr` gained `image/lexrt.rs` (the flex run-time of `exp_lex.c` on the image: buffer records and the globals at
0x0869ba40.. and 0x0878fea4..) and `image/parsing.rs` (clause, Sumatra, install and set-expression routines of
`exp_parsing.c`, and image forms of the query path of `exp_fct.c`). Both keep their state at the original's 32-bit layout on
`si-mem`. The checks use `si-checkkit` (`lexrt-checkkit`, `parse-checkkit`) with world generators `gen_lexrt.py` and
`gen_parse.py`; the numbers and the cited functions are in finding 0013, section 16. Not ported for want of the parser and
printer on the image: `SI_copyElemExpression`, `SI_generateQueryFromClause`.

## 2026-10-29: `si-app` render-path leftovers (unit app1)

Added `image/hardware.rs` and `image/leftovers.rs` to `si-app`.

Ported and checked (emulator, world generators in `re/tools/si-app/gen_image.py`, checker `app-imagecheck`):
- `SI_checkHardware` (0x080d378c), whose body continues past the 122-byte symbol at 0x080d3877: the uname release goes
  through `strtod` into `SI_operSys` as an f32. 150 cases: 150 identical, 0 different, 0 not compared. Mutation 1: 150 of 150
  differ; mutated calls: 150 of 150 differ. The full-product path is taken where the original tests the tier flag.
- `SI_getRawFcurvePointer` (0x083e009c), `SI_getRawFcurvePointerPointer` (0x083e00a8), `si_getLayoutWindow` (0x083f90c8),
  `resetSize` (0x080d1bdc), `resetRotation` (0x080d1c18), `SI_resetCounter` (0x080d1c4c), `SI_refreshCounter` (0x080d1d20),
  `SI_displayModelScal` (0x080d1320; floats go through an x87 round trip, signalling-NaN inputs left out per finding 0011).
  Partial: `setT2dMapDomainID` (0x083f3574) leaves out the licence-counter increment on purpose.
  Leftovers group, 551 cases: 539 identical, 0 different, 12 stopped (missing `__strtol_internal` stand-in in the
  emulator, all in `setT2dMapDomainID`), 0 not compared. Mutation 1: 539 of 539 compared differ; mutated calls: 413 of 539
  differ (zero-argument routines cannot differ).
- Strings `_UVDomainID_` and the nine panel texts were added to `TEXTS` in `image/mod.rs`.

Not ported, on purpose: UI and dialog code (dialog-bound fcurve read/write, desktop colours, fonts, key-binding preferences,
`SI_setupFileLoad*`, `SI_modelRead`), licence code (`IsAcademic`, `SI_getAllOptions`, `genChecksums`,
`SI_licenseAddCheckTable`), and the Lite model readers (used only by a list dialog; `si-hrc` depends on `si-app`).
Open: `SI_displayModelTrans` (0x080d1844) and `SI_displayModelRot` (0x080d1490).

How to check: `re/tools/lead/slot.sh emu -- uv run --with unicorn python3 -I re/tools/si-app/gen_image.py OUT hardware|leftovers`,
then `app-imagecheck OUT` (optionally `EMUCHECK_MUTATE=1` or `calls`).
