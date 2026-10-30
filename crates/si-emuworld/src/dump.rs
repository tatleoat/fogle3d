// SPDX-License-Identifier: MIT
//! The image of a built scene, written out for the emulation harness.
//!
//! Nothing here reproduces a function of the original. Two files per scene:
//!
//! - `NAME.bin`: the bytes of the regions, one after the other;
//! - `NAME.idx`: text, one fact a line, fields separated by tabs:
//!
//! ```text
//! scene   LABEL                      the scene's path below the corpus directory
//! db      PATH                       its database's directory
//! name    NAME                       its file name without `.dsc`
//! rsrc    PATH                       the relations resource file it was loaded with
//! stage   dict|objects|list
//! loaded  true|false
//! region  NAME ADDRESS LENGTH OFFSET HASH    a region: where it lies in memory and in
//!                                    the .bin file; FNV-1a (64 bit) of its bytes
//! block   ADDRESS SIZE               a live block of the heap
//! global  NAME ADDRESS SIZE SYMBOL   a global one of the packages declares (`globals!`)
//! entry   KIND ADDRESS …             an address worth calling a routine on (below)
//! count   NUMBER TEXT                a count of the build
//! note    TEXT                       what is glue, what is missing
//! ```
//!
//! Regions: `rodata`, `data`, `bss` whole (they are zero where no package stored
//! anything: the binary's own data is not in the image), `heap` from its start to the
//! highest byte in use. The arena and the stack are not written: the arena is where a
//! case's own buffers go, and the stack holds dead locals of the load.
//!
//! Entries: `dict ADDRESS chapters`; `chapter ADDRESS index elements name`;
//! `elem ADDRESS chapter index status object node`; `model ADDRESS father kind
//! definitions`; `graflist ADDRESS`; `grafnode ADDRESS kind object sub-models`.

use crate::build::World;
use si_mem::{Mem, Region};
use std::io::Write;
use std::path::Path;

/// FNV-1a, 64 bit.
pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The regions that are written, with the part of each that is: (name, address, length).
pub fn regions(m: &Mem) -> Vec<(&'static str, u32, u32)> {
    let (heap, _) = m.region(Region::Heap);
    let used = m.heap_stats().used as u32;
    let (r, d, b) = (m.region(Region::Rodata), m.region(Region::Data), m.region(Region::Bss));
    vec![("rodata", r.0, r.1), ("data", d.0, d.1), ("bss", b.0, b.1), ("heap", heap, used.min(m.mapped(Region::Heap)))]
}

/// The hash of every region, in the order of [`regions`]: what a check that builds the
/// scene again holds its image against.
pub fn hashes(m: &Mem) -> Vec<(&'static str, u64)> {
    regions(m).into_iter().map(|(name, addr, len)| (name, fnv(m.bytes(addr, len)))).collect()
}

/// The globals the packages behind a scene declare.
pub fn globals() -> Vec<&'static si_mem::Global> {
    let mut all: Vec<&'static si_mem::Global> = Vec::new();
    for table in [si_dict::globals::ALL, si_scene::globals::ALL, si_view::globals::ALL, si_hrc::image::globals::ALL] {
        all.extend(table.iter());
    }
    all.sort_by_key(|g| (g.addr, g.size));
    all.dedup_by_key(|g| (g.addr, g.size));
    all
}

/// Writes `<dir>/<file>.bin` and `<dir>/<file>.idx`.
pub fn write(w: &World, label: &str, db: &Path, name: &[u8], rsrc: &Path, dir: &Path, file: &str) -> std::io::Result<u64> {
    let m = &w.sc.mem;
    std::fs::create_dir_all(dir)?;
    let mut bin = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{file}.bin")))?);
    let mut idx = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{file}.idx")))?);
    writeln!(idx, "scene\t{label}")?;
    writeln!(idx, "db\t{}", db.display())?;
    writeln!(idx, "name\t{}", String::from_utf8_lossy(name))?;
    writeln!(idx, "rsrc\t{}", rsrc.display())?;
    writeln!(idx, "stage\t{}", w.stage.name())?;
    writeln!(idx, "loaded\t{}", w.loaded)?;
    let mut offset = 0u64;
    for (region, addr, len) in regions(m) {
        let bytes = m.bytes(addr, len);
        bin.write_all(bytes)?;
        writeln!(idx, "region\t{region}\t{addr:08x}\t{len:08x}\t{offset}\t{:016x}", fnv(bytes))?;
        offset += len as u64;
    }
    for (addr, size) in m.live_blocks() {
        writeln!(idx, "block\t{addr:08x}\t{size:x}")?;
    }
    for g in globals() {
        writeln!(idx, "global\t{}\t{:08x}\t{:x}\t{}", g.name, g.addr, g.size, g.symbol)?;
    }
    for e in &w.entries {
        write!(idx, "entry\t{}\t{:08x}", e.kind, e.addr)?;
        for x in &e.more {
            write!(idx, "\t{x}")?;
        }
        writeln!(idx)?;
    }
    for (what, n) in &w.counts {
        writeln!(idx, "count\t{n}\t{what}")?;
    }
    for note in &w.notes {
        writeln!(idx, "note\t{note}")?;
    }
    bin.flush()?;
    idx.flush()?;
    Ok(offset)
}
