// SPDX-License-Identifier: MIT
//! The one image of the program's memory (finding 0024).
//!
//! Softimage|3D is a C program: thousands of globals, heap records linked by 32-bit
//! pointers, unions, pointers into the middle of records and arrays, `void *` fields,
//! function pointers in tables. This crate is how the Rust port holds all of that: **the
//! original's 32-bit memory layout is the interface between crates.** There is one
//! [`Mem`]; a record is a typed `#[repr(C)]` value at a 32-bit address in it; a ported
//! function takes the image and addresses, exactly as the original takes pointers. No
//! other crate defines an arena, an image or a store of its own.
//!
//! No function of the original is reproduced or cited here. The crate depends on no other
//! crate of the workspace, so every crate can depend on it.
//!
//! | Module | Content |
//! |---|---|
//! | [`ptr`] | [`Ptr32`], the typed 32-bit pointer; [`FnPtr`], [`CStr32`], [`field!`] |
//! | [`pod`] | [`Pod`], the marker of what can lie in the image; the i386 layout rules ([`Double`], [`LongLong`]); [`record!`] |
//! | [`image`] | [`Mem`]: the regions, typed and raw access, recorded faults, snapshots |
//! | [`alloc`] | the heap, handing out blocks as the emulation harness does or using freed room again ([`HeapPolicy`]); the program's three wrappers, the arena of test records, the stack of locals |
//! | [`clib`] | the C library's string and memory routines on the image |
//! | [`vecfmt`] | loading and dumping in the form of the harness's vector files |
//! | [`mod@globals`] | [`globals!`]: a crate's global variables as typed pointers at their addresses, with initial values |
//! | [`code`] | [`Registry`]: from a code address of the original to a Rust handler |
//! | [`handle`] | [`Handles`]: addresses for things that are not memory |
//! | [`outside`] | a proposal, used by no crate yet: [`Outside`], the one trait through which a ported function calls what is not on the image |
//!
//! # A check and a whole run
//!
//! [`Mem::new`] is the image an emulation check makes for every case: the harness's
//! addresses, and a heap of 16 MB that hands out blocks in the harness's order, so that a
//! block's address can be compared with the emulated original's. [`Mem::program`] is the
//! image for a whole run (a scene loaded, a session): a heap of 1.25 GB, of which the host
//! pays for the pages that are touched, and whose blocks are used again when they are
//! freed ([`HeapPolicy::Reuse`]). The ported functions are the same on both; only the
//! addresses of heap blocks differ. [`Mem::with_heap`] takes any map with either policy.
//!
//! # A ported function, before and after
//!
//! The original (`window.c`, in words): given a view window, if it is not the selected one,
//! clear the mark on the selected one and set it here.
//!
//! ```
//! use si_mem::{record, Mem, Ptr32, BSS};
//!
//! record! {
//!     /// A view's window (the first fields of it).
//!     pub struct Grafport = "__SI_Grafport", size 0x0c {
//!         0x00 name: Ptr32<u8>, "the view's name";
//!         0x04 id: i16, "1000 to 1003";
//!         0x06 visible: i16, "non-zero when shown";
//!         0x08 hilite: i16, "non-zero for the selected window";
//!         0x0a enabled: i16, "reacts to clicks";
//!     }
//! }
//! /// A global: the address it has in the binary, typed.
//! const SELECTED: Ptr32<Ptr32<Grafport>> = Ptr32::new(BSS.0 + 0x40);
//!
//! /// The port takes what the original takes, and leaves what it leaves.
//! fn select(m: &mut Mem, w: Ptr32<Grafport>) {
//!     let old = m[SELECTED];
//!     if old != w {
//!         if !old.is_null() {
//!             m[old].hilite = 0;
//!         }
//!         m[w].hilite = 1;
//!         m[SELECTED] = w;
//!     }
//! }
//!
//! let mut m = Mem::new();
//! let views: Ptr32<Grafport> = Ptr32::new(m.calloc(4, Grafport::SIZE));
//! select(&mut m, views.add(2));
//! select(&mut m, views.add(1));
//! assert_eq!((m[views.add(2)].hilite, m[views.add(1)].hilite), (0, 1));
//! assert!(m.fault().is_none());
//! ```

// A borrowed view shows a record's bytes in the original's order only on a little-endian
// machine (the copying calls would work anywhere, but the views are the point).
#[cfg(not(target_endian = "little"))]
compile_error!("si-mem needs a little-endian host: records are viewed in place in the original's byte order");

pub mod alloc;
pub mod clib;
pub mod code;
pub mod globals;
pub mod handle;
pub mod image;
pub mod outside;
pub mod pod;
pub mod ptr;
pub mod vecfmt;

pub use alloc::{HeapPolicy, HeapStats, BLOCK_MARK, SI_MALLOC_FIRST, SI_MALLOC_LAST};
pub use code::{is_code, Called, Handler, Registry, Ret};
pub use globals::{init_globals, Global};
pub use handle::{Handles, Kind, HANDLES_HIGH};
pub use outside::{Outside, Registered, Routine, Trace, Wired, World};
pub use image::{Access, Fault, Fill, Map, Mem, OnFault, Region, Snapshot, Stop, ARENA, BSS, DATA, HANDLES, HEAP, HEAP_PROGRAM, HEAP_WIDE, RODATA, STACK, STACK_START, TEXT};
pub use pod::{assert_pod, Double, LongLong, Pod, ULongLong};
pub use ptr::{Addr, CStr32, Code, FnPtr, Opaque, Ptr32};
pub use vecfmt::{format_text, format_words, load_case, load_case_at, parse_shape, parse_words, words_len, Elem, FieldAt, Names, Word};
