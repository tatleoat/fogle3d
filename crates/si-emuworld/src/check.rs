// SPDX-License-Identifier: MIT
//! What a check of ports on a scene's image needs besides the ports: the image put back
//! between cases, the case's buffers placed where the harness placed them, and the
//! comparison of what a port changed with what the original changed.
//!
//! Nothing here reproduces a function of the original.

use crate::case::{arena_layout, Case};
use si_mem::vecfmt::{load_case_at, Names, Word};
use si_mem::{Mem, Region, Snapshot};

/// The image as a case starts from it.
pub struct Base {
    snap: Snapshot,
    /// Copies of the regions that are compared: (address, bytes).
    saved: Vec<(u32, Vec<u8>)>,
    arena: (u32, u32),
}

impl Base {
    pub fn new(m: &Mem) -> Base {
        let mut saved = Vec::new();
        for (_, addr, len) in crate::dump::regions(m) {
            saved.push((addr, m.bytes(addr, len).to_vec()));
        }
        Base { snap: m.snapshot(), saved, arena: m.region(Region::Arena) }
    }

    fn was(&self, addr: u32) -> u8 {
        for (a, bytes) in &self.saved {
            if addr >= *a && ((addr - a) as usize) < bytes.len() {
                return bytes[(addr - a) as usize];
            }
        }
        0
    }

    fn in_arena(&self, addr: u32) -> bool {
        addr.wrapping_sub(self.arena.0) < self.arena.1
    }

    /// Every run of bytes outside the arena and the stack that differs from the start.
    pub fn changes(&self, m: &Mem) -> Vec<(u32, Vec<u8>)> {
        m.changed(&self.snap).into_iter().filter(|(a, _)| !self.in_arena(*a)).collect()
    }

    /// Puts the image back as the case found it (contents only: a block a port allocated
    /// stays allocated, which is why the routines checked here are ones that allocate
    /// nothing).
    pub fn restore(&self, m: &mut Mem) {
        for (a, bytes) in m.changed(&self.snap) {
            let old: Vec<u8> = (0..bytes.len() as u32).map(|k| if self.in_arena(a + k) { 0 } else { self.was(a + k) }).collect();
            m.set_bytes(a, &old);
        }
        m.clear_fault();
    }
}

/// Places the buffers of a case where the harness put them and stores their words.
pub fn place(m: &mut Mem, c: &Case) -> Result<Names, String> {
    let bufs = c.buffers();
    let sizes: Vec<u32> = bufs.iter().map(|(_, w)| w.map_or(0, si_mem::vecfmt::words_len)).collect();
    let at = arena_layout(&sizes);
    let fields: Vec<si_mem::vecfmt::FieldAt<'_>> = bufs.iter().zip(&at).map(|((n, w), a)| (*n, Some(*a), *w)).collect();
    load_case_at(m, &fields)
}

/// A word of an argument: a value, or the address of a buffer of the case.
pub fn word(m: &Mem, names: &Names, w: &[Word]) -> u32 {
    match w.first() {
        Some(Word::Value { value, .. }) => *value as u32,
        Some(Word::Ref { name, offset }) => names.addr(m, name).unwrap_or(0).wrapping_add(*offset),
        _ => 0,
    }
}

/// The bytes the original changed, as (address, value) for every byte that has one.
pub fn harness_bytes(c: &Case) -> Vec<(u32, u8)> {
    let mut out = Vec::new();
    for (a, words) in c.changes() {
        let mut at = a;
        for w in words {
            if let Word::Value { bytes, value } = w {
                for k in 0..*bytes as u32 {
                    out.push((at + k, (value >> (8 * k)) as u8));
                }
            }
            at += w.bytes();
        }
    }
    out.sort();
    out
}

/// The same for a port's changes.
pub fn port_bytes(changes: &[(u32, Vec<u8>)]) -> Vec<(u32, u8)> {
    let mut out: Vec<(u32, u8)> = changes.iter().flat_map(|(a, b)| b.iter().enumerate().map(move |(k, v)| (a + k as u32, *v))).collect();
    out.sort();
    out
}

/// The first difference of two sorted lists of changed bytes, described.
pub fn first_difference(original: &[(u32, u8)], port: &[(u32, u8)]) -> Option<String> {
    if original == port {
        return None;
    }
    let (mut i, mut j) = (0, 0);
    while i < original.len() || j < port.len() {
        match (original.get(i), port.get(j)) {
            (Some(a), Some(b)) if a == b => {
                i += 1;
                j += 1;
            }
            (Some(a), Some(b)) if a.0 == b.0 => return Some(format!("byte at {:08x}: original {:02x}, port {:02x}", a.0, a.1, b.1)),
            (Some(a), Some(b)) if a.0 < b.0 => return Some(format!("the original wrote {:02x} at {:08x}, the port left it", a.1, a.0)),
            (Some(_), Some(b)) => return Some(format!("the port wrote {:02x} at {:08x}, the original left it", b.1, b.0)),
            (Some(a), None) => return Some(format!("the original wrote {:02x} at {:08x}, the port left it", a.1, a.0)),
            (None, Some(b)) => return Some(format!("the port wrote {:02x} at {:08x}, the original left it", b.1, b.0)),
            (None, None) => break,
        }
    }
    Some("(the lists differ)".into())
}
