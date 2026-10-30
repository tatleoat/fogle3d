// SPDX-License-Identifier: MIT
//! What an image costs a check that makes one per case: prints the time for fresh images,
//! snapshots and typed accesses.
//!
//!   cargo run --release -p si-mem --example cost
use si_mem::{Mem, Ptr32, BSS, DATA};
use std::time::Instant;

fn main() {
    let n = 200_000u32;
    let t = Instant::now();
    let mut sum = 0u64;
    for i in 0..n {
        // a case: a few globals, three buffers, two heap blocks, a snapshot, the changes
        let mut m = Mem::new();
        m.set_i32(DATA.0 + 0x400, i as i32);
        m.set_i32(BSS.0 + 0x9000, 7);
        let a = m.place(0x430, 8);
        let b = m.place(64, 8);
        m.set_u32(a + 0x2bc, b);
        let before = m.snapshot();
        let h = m.si_malloc(0x74);
        m.set_u32(a + 0x318, h);
        m.set_f32(b + 8, 1.5);
        let p: Ptr32<[f32; 16]> = Ptr32::new(a + 0x68);
        m[p][5] = 2.0;
        sum += m.changed(&before).len() as u64 + m.committed_bytes() as u64;
    }
    let per = t.elapsed().as_secs_f64() / n as f64 * 1e6;
    println!("{n} cases (fresh image, 3 blocks, snapshot, changes): {per:.2} microseconds each (check {sum})");

    let mut m = Mem::new();
    let p: Ptr32<u32> = Ptr32::new(m.malloc(4096 * 4));
    let t = Instant::now();
    let mut acc = 0u32;
    for round in 0..2000u32 {
        for i in 0..4096 {
            m[p.add(i)] = m[p.add(i)].wrapping_add(round);
            acc = acc.wrapping_add(m.u32(p.add(i).addr()));
        }
    }
    let per = t.elapsed().as_secs_f64() / (2000.0 * 4096.0 * 3.0) * 1e9;
    println!("typed and raw accesses: {per:.2} nanoseconds each (check {acc})");
}
