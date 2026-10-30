// SPDX-License-Identifier: MIT
//! What a large heap costs the host: an image with the heap for a whole run of the program
//! (`Map::program`, 1.25 GB) is driven to the end of that heap, and after each step the
//! host's own figures for this process are printed beside the image's.
//!
//!   cargo build --release -p si-mem --example heap
//!   (ulimit -v 2000000; target/si-mem/release/examples/heap)
//!
//! Nothing here touches more than a few megabytes: the blocks are handed out and left
//! unwritten but for a page each, which is the point (the host pays for pages touched,
//! not for the heap's length).
use si_mem::{HeapPolicy, Map, Mem, Region, HEAP_PROGRAM};
use std::time::Instant;

/// A line of `/proc/self/status` in MB (this machine's own account of the process).
fn status(key: &str) -> f64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()).map_or(0.0, |kb| kb / 1024.0)
}

fn mb(n: usize) -> f64 {
    n as f64 / 1_048_576.0
}

fn main() {
    println!("at the start: resident {:.1} MB, address space {:.1} MB", status("VmRSS:"), status("VmSize:"));
    let mut m = Mem::with_heap(Map::program(), HeapPolicy::Harness);
    println!(
        "a fresh image with a heap of {:.0} MB declared: {:.1} MB of address space held, {:.1} MB touched; resident {:.1} MB",
        mb(HEAP_PROGRAM.1 as usize),
        mb(m.reserved_bytes()),
        mb(m.committed_bytes()),
        status("VmRSS:")
    );

    // 24 MB that are really used: every page written
    let used = m.malloc(24 << 20);
    m.set_fill_bytes(used, 0x5a, 24 << 20);
    println!(
        "after 24 MB written: heap reaches {:.0} MB, {:.1} MB held, {:.1} MB touched; resident {:.1} MB",
        mb(m.mapped(Region::Heap) as usize),
        mb(m.reserved_bytes()),
        mb(m.committed_bytes()),
        status("VmRSS:")
    );

    // then blocks of 64 MB, a page of each written, until the heap answers null
    let mut blocks = 0;
    let mut slowest = 0.0f64;
    loop {
        let t = Instant::now();
        let p = m.malloc(64 << 20);
        slowest = slowest.max(t.elapsed().as_secs_f64());
        if p == 0 {
            break;
        }
        m.set_u32(p + (32 << 20), blocks);
        blocks += 1;
        if blocks % 6 == 0 {
            println!(
                "  {blocks} blocks of 64 MB: heap reaches {:.0} MB, {:.1} MB held, {:.1} MB touched; resident {:.1} MB, address space {:.1} MB",
                mb(m.mapped(Region::Heap) as usize),
                mb(m.reserved_bytes()),
                mb(m.committed_bytes()),
                status("VmRSS:"),
                status("VmSize:")
            );
        }
    }
    let s = m.heap_stats();
    println!(
        "the heap answered null after {blocks} blocks: {:.0} MB of {:.0} MB handed out; {:.1} MB touched; resident {:.1} MB (at most {:.1} MB), address space {:.1} MB (at most {:.1} MB); the slowest malloc took {:.3} ms",
        mb(s.used as usize),
        mb(HEAP_PROGRAM.1 as usize),
        mb(m.committed_bytes()),
        status("VmRSS:"),
        status("VmHWM:"),
        status("VmSize:"),
        status("VmPeak:"),
        slowest * 1e3
    );
    // what was written is still there, at both ends
    assert_eq!((m.u8(used), m.u8(used + (24 << 20) - 1)), (0x5a, 0x5a));
    let last = m.heap_blocks()[blocks as usize].0;
    assert_eq!(m.u32(last + (32 << 20)), blocks - 1);
    assert!(m.fault().is_none());
    drop(m);
    println!("after the image is dropped: resident {:.1} MB, address space {:.1} MB", status("VmRSS:"), status("VmSize:"));
}
