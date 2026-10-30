// SPDX-License-Identifier: MIT
//! `fastplay.c` on the image: the fast-play buffers of a window (`__SI_Grafport`). The
//! record has no `si-types` layout; the fields these routines use are named below.

use si_mem::Mem;

use super::{callee, Outside};

/// `__SI_Grafport` + 0x260: the number of entries of the table at +0x26c (a signed int).
pub const PORT_ENTRIES: u32 = 0x260;
/// `__SI_Grafport` + 0x264: the number the single-entry form counts from (an int).
pub const PORT_BASE: u32 = 0x264;
/// `__SI_Grafport` + 0x26c: the table, `PORT_ENTRIES` entries of 0x14 bytes.
pub const PORT_TABLE: u32 = 0x26c;
/// A table entry: a 16-bit dirty flag at +0, a word at +4 (cleared), and the two blocks at
/// +8 and +0x10 (freed and cleared); +0xc is cleared too.
const ENTRY: u32 = 0x14;

/// Marks the fast-play entries of a window as dirty [soft 0x08329898 SI_fastSetDirty].
///
/// With a port and `all` 0, only one entry: `|port.base - index|` (the absolute value of
/// the 32-bit difference, so the most negative number stays negative) times 0x14 into the
/// table, whose flag becomes 1 (nothing is checked against the count; a null table does
/// nothing). With a port and `all` not 0, the whole table of that port is reset; with no
/// port, the whole table of every window `SI_windowGetNextActive` gives (it is handed the
/// address of the port variable and sets it; a 16-bit answer of 0 ends the walk, and it is
/// asked first). Resetting a table: for each of the `count` (signed, above 0) entries, the
/// flag becomes 1, the blocks at +8 and +0x10 are freed if not null, and the words at +4,
/// +8, +0xc, +0x10 are zeroed; the table pointer is read again before each access.
pub fn set_dirty(m: &mut Mem, o: &mut dyn Outside, port: u32, index: i32, all: i32) {
    if port != 0 && all == 0 {
        let d = (m.i32(port.wrapping_add(PORT_BASE))).wrapping_sub(index);
        let n = if d < 0 { d.wrapping_neg() } else { d };
        let table = m.u32(port.wrapping_add(PORT_TABLE));
        if table != 0 {
            m.set_u16(table.wrapping_add((n as u32).wrapping_mul(ENTRY)), 1);
        }
        return;
    }
    let slot = m.local(4);
    m.set_u32(slot, port);
    let single = port != 0;
    if !single {
        if o.call(m, &callee::SI_WINDOW_GET_NEXT_ACTIVE, &[slot]) as u16 == 0 {
            m.release(4);
            return;
        }
    }
    loop {
        let port = m.u32(slot);
        let count = m.i32(port.wrapping_add(PORT_ENTRIES));
        let table = |m: &Mem| m.u32(port.wrapping_add(PORT_TABLE));
        if table(m) != 0 && count > 0 {
            let mut at = 0u32;
            for _ in 0..count {
                let t = table(m);
                m.set_u16(t.wrapping_add(at), 1);
                let b = m.u32(table(m).wrapping_add(at).wrapping_add(8));
                if b != 0 {
                    m.free(b);
                    let t = table(m);
                    m.set_u32(t.wrapping_add(at).wrapping_add(8), 0);
                }
                let b = m.u32(table(m).wrapping_add(at).wrapping_add(0x10));
                if b != 0 {
                    m.free(b);
                    let t = table(m);
                    m.set_u32(t.wrapping_add(at).wrapping_add(0x10), 0);
                }
                for (off, v) in [(0u32, 1u16), (4, 0), (8, 0), (0xc, 0), (0x10, 0)] {
                    let t = table(m);
                    if off == 0 {
                        m.set_u16(t.wrapping_add(at), v);
                    } else {
                        m.set_u32(t.wrapping_add(at).wrapping_add(off), 0);
                    }
                }
                at = at.wrapping_add(ENTRY);
            }
        }
        if single {
            break;
        }
        if o.call(m, &callee::SI_WINDOW_GET_NEXT_ACTIVE, &[slot]) as u16 == 0 {
            break;
        }
    }
    m.release(4);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_image::NoOutside;

    fn port(m: &mut Mem, entries: u32) -> (u32, u32) {
        let p = m.place(0x270, 8);
        let t = m.place(ENTRY * entries.max(1), 8);
        m.set_u32(p + PORT_ENTRIES, entries);
        m.set_u32(p + PORT_BASE, 5);
        m.set_u32(p + PORT_TABLE, t);
        (p, t)
    }

    #[test]
    fn one_entry_or_the_whole_table() {
        let mut m = Mem::new();
        let (p, t) = port(&mut m, 3);
        let blk = m.malloc(8);
        m.set_u32(t + 2 * ENTRY + 8, blk);
        m.set_u32(t + 2 * ENTRY + 4, 99);
        // |5 - 3| = 2: only the flag of entry 2
        set_dirty(&mut m, &mut NoOutside, p, 3, 0);
        assert_eq!(m.u16(t + 2 * ENTRY), 1);
        assert_eq!(m.u32(t + 2 * ENTRY + 8), blk);
        assert_eq!(m.u16(t), 0);
        // the whole table: the block is freed, the words cleared
        set_dirty(&mut m, &mut NoOutside, p, 0, 1);
        assert_eq!((m.u16(t), m.u16(t + ENTRY), m.u16(t + 2 * ENTRY)), (1, 1, 1));
        assert_eq!((m.u32(t + 2 * ENTRY + 4), m.u32(t + 2 * ENTRY + 8)), (0, 0));
        assert_eq!(m.freed(), &[blk]);
        // no port, and no window answers: nothing happens
        set_dirty(&mut m, &mut NoOutside, 0, 0, 0);
        assert!(m.fault().is_none());
    }
}
