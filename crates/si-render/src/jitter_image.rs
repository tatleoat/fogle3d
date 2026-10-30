// SPDX-License-Identifier: MIT
//! `si_initJitter` on the memory image: the jitter table of a material or a light, made
//! where the original makes it, in the renderer's chunk memory (finding 0012, section
//! 19.2). The numbers are [`crate::jitter::init_jitter`]'s, which this follows operation
//! for operation; that port keeps its table in a vector of its own.

use crate::data::text;
use crate::img::{f, g, Outside};
use crate::jitter::unirand;
use si_mem::Mem;
use si_x87::{trig, x};

/// The sampling level (16 bits of the controls, 0x08790d38).
const LEVEL: u32 = 0x0879_0d38;

/// Fills a jitter record's table with `count` rows [soft 0x082596b4 si_initJitter]:
/// `si_initJitter(count, jitter)`; the record is `SI_Jitter`: +0x00 the amplitude, +0x04
/// the table, +0x08 a 16-bit word that is cleared.
///
/// With a sampling level below 4, `SI_warning("irresponsible use of jitter: sampling < 4",
/// " ", 2)` first. The table is `count` pointers from `SI_chunkMalloc(SI_renChunk, count ×
/// 4)`; each row a block of its own from the same arena: 8 bytes at level 1, else
/// (2 × level − 1)² pairs of floats. The pairs are as [`crate::jitter::init_jitter`]
/// describes them. The amplitude is read after the table is asked for. Nothing is tested:
/// a null from the arena is written through (a recorded fault here).
///
/// `drand48` is the C library's; `SI_chunkMalloc` (0x0857cb64) and `SI_warning`
/// (0x0826072c) are called through `env` by address. Class: libm (`sinf`, `cosf`); level 1
/// is model.
pub fn init_jitter(m: &mut Mem, env: &mut dyn Outside, drand48: &mut dyn FnMut() -> f64, count: i32, jitter: u32) {
    let level = m.i16(LEVEL);
    if level < 4 {
        env.call(m, f::SI_WARNING, &[text::JITTER_WARNING, text::JITTER_BLANK, 2]);
    }
    let table;
    if m.i16(LEVEL) == 1 {
        let chunk = m.u32(g::SI_REN_CHUNK);
        table = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (count as u32) << 2]);
        let amplitude = m.f32(jitter);
        let mut i: i32 = 0;
        while i < count {
            let chunk = m.u32(g::SI_REN_CHUNK);
            let row = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, 8]);
            let slot = table.wrapping_add((i as u32) << 2);
            m.set_u32(slot, row);
            let a = (x(amplitude) * unirand(drand48)).to_f32();
            let row = m.u32(slot);
            m.set_f32(row, a);
            let u = unirand(drand48);
            let row = m.u32(slot);
            m.set_f32(row.wrapping_add(4), ((x(u) + u) - 1.0f32).to_f32());
            i += 1;
        }
    } else {
        let side = (m.i16(LEVEL) as i32).wrapping_mul(2).wrapping_sub(1);
        let pairs = side.wrapping_mul(side);
        let step = (x(core::f64::consts::TAU) / x(pairs.wrapping_sub(1))).to_f32();
        let chunk = m.u32(g::SI_REN_CHUNK);
        table = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (count as u32) << 2]);
        let amplitude = m.f32(jitter);
        let mut i: i32 = 0;
        while i < count {
            let chunk = m.u32(g::SI_REN_CHUNK);
            let row = env.call(m, f::SI_CHUNK_MALLOC, &[chunk, (pairs as u32).wrapping_mul(8)]);
            let slot = table.wrapping_add((i as u32) << 2);
            m.set_u32(slot, row);
            let mut angle = 0.0f32;
            let mut j: i32 = 0;
            while j < pairs {
                let at = (j as u32).wrapping_mul(8);
                if j == pairs / 2 {
                    let row = m.u32(slot);
                    m.set_u32(row.wrapping_add(4).wrapping_add(at), 0);
                    m.set_u32(row.wrapping_add(at), 0);
                } else {
                    let r = x(unirand(drand48)) * amplitude;
                    let a = (x(unirand(drand48)) * step + angle).to_f32();
                    let row = m.u32(slot);
                    m.set_f32(row.wrapping_add(at), (trig::sinf(a) * r).to_f32());
                    let row = m.u32(slot);
                    m.set_f32(row.wrapping_add(4).wrapping_add(at), (trig::cosf(a) * r).to_f32());
                    angle += step;
                }
                j += 1;
            }
            i += 1;
        }
    }
    m.set_u32(jitter.wrapping_add(4), table);
    m.set_u16(jitter.wrapping_add(8), 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jitter::{Drand48, Jitter};
    use crate::mem_chunk_image as chunk;

    /// The arena behind `SI_chunkMalloc`; the warning is counted.
    struct Arena(u32);
    impl Outside for Arena {
        fn call(&mut self, m: &mut Mem, addr: u32, a: &[u32]) -> u32 {
            match addr {
                f::SI_CHUNK_MALLOC => chunk::chunk_malloc(m, a[0], a[1] as i32),
                f::SI_WARNING => {
                    self.0 += 1;
                    0
                }
                _ => panic!("{addr:#x}"),
            }
        }
    }

    #[test]
    fn the_table_lies_in_chunk_memory_and_holds_the_other_form_s_numbers() {
        si_rt::skip_without_install!();
        for level in [1i16, 2, 3, 5] {
            let mut m = Mem::new();
            crate::data::init(&mut m);
            let buf = chunk::chunk_allocate(&mut m, 8, 256);
            m.set_u32(g::SI_REN_CHUNK, buf);
            m.set_i16(LEVEL, level);
            let record = m.calloc(1, 12);
            m.set_f32(record, 0.75);
            m.set_u16(record + 8, 0x1234);
            let (mut rng, mut rng2) = (Drand48::seeded(5), Drand48::seeded(5));
            let mut env = Arena(0);
            init_jitter(&mut m, &mut env, &mut || rng.next(), 3, record);
            let mut j = Jitter { amplitude: 0.75, ..Jitter::default() };
            let warning = crate::jitter::init_jitter(3, &mut j, level, &mut || rng2.next());
            assert_eq!(env.0, warning.is_some() as u32);
            assert_eq!(m.u16(record + 8), 0);
            let table = m.u32(record + 4);
            // the table is the arena's first piece: the first block's start
            assert_eq!(table, m.u32(buf + chunk::off::SLOTS + 4));
            for (i, row) in j.table.iter().enumerate() {
                let p = m.u32(table + 4 * i as u32);
                // every row lies in a block of the arena
                assert!((0..8).any(|s| {
                    let at = buf + chunk::off::SLOTS + 12 * s;
                    let (size, start) = (m.u32(at), m.u32(at + 4));
                    start != 0 && p >= start && p - start < size
                }));
                for (k, pair) in row.iter().enumerate() {
                    assert_eq!([m.f32(p + 8 * k as u32).to_bits(), m.f32(p + 8 * k as u32 + 4).to_bits()], [pair[0].to_bits(), pair[1].to_bits()], "level {level}, row {i}, pair {k}");
                }
            }
            assert!(m.fault().is_none());
        }
    }
}
