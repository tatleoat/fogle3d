// SPDX-License-Identifier: MIT
//! Handles: addresses for things that are not memory.
//!
//! Nothing here reproduces a function of the original.
//!
//! A `FILE *`, a display connection, a window of the graphics layer: the original keeps a
//! pointer to each in its records and globals, hands it back to the library it came from,
//! compares it with null, and never looks at what it points to. In the image such a field
//! holds an address from [`crate::HANDLES`], a range that is never mapped (reading through
//! a handle is a recorded fault, as it would be a fault of the port). A [`Handles`] table
//! owns the Rust values the addresses stand for.
//!
//! The range is the emulation harness's (`Sandbox.FILES`, 0x6f000000), and the first kind
//! is numbered as the harness numbers files: 0x100 apart, so `stdin`, `stdout` and
//! `stderr`, opened in that order, are 0x6f000000, 0x6f000100, 0x6f000200 on both sides.
//!
//! A kind has as many handles as are opened. The first 4,096 are the ones 0x100 apart;
//! the next 126,976 lie between them, 8 apart, still in the kind's megabyte; after
//! those 131,072 the addresses come from a second range that is never mapped either
//! ([`HANDLES_HIGH`]), 8 apart, 8,388,608 more for each kind. [`Kind::nth`] and
//! [`Kind::number`] are the rule in both directions.

use crate::HANDLES;

/// The second range of handle addresses, for a kind that has used up its megabyte of
/// [`HANDLES`]: 0xc0000000 to the end of the address space, 64 MB for each kind. In the
/// original's 32-bit process this quarter belongs to the system, so no pointer of the
/// program can point there; no map of the image should put a region there.
pub const HANDLES_HIGH: (u32, u32) = (0xc000_0000, 0x4000_0000);

/// Handles of a kind that are 0x100 apart, and all that fit its megabyte.
const FIRST: u32 = 0x0010_0000 / STEP;
const NEAR: u32 = 0x0010_0000 / FINE;
/// The distance between two handles after the first 4,096.
const FINE: u32 = 8;
/// Handles of a kind in [`HANDLES_HIGH`].
const FAR: u32 = 0x0400_0000 / FINE;

/// The kinds of handles: each has a slice of the range, so an address says what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A `FILE *`. The harness's own numbering.
    File,
    /// A window, display or graphics context of the layer below the program.
    Window,
    /// A loaded library, a process, a timer: anything of the system.
    System,
    /// Kinds of a crate's own, 0 to 12.
    Other(u8),
}

impl Kind {
    const fn slot(self) -> u32 {
        let slot = match self {
            Kind::File => 0,
            Kind::Window => 1,
            Kind::System => 2,
            Kind::Other(n) => 3 + n as u32,
        };
        assert!(slot < 16);
        slot
    }

    const fn of_slot(slot: u32) -> Kind {
        match slot {
            0 => Kind::File,
            1 => Kind::Window,
            2 => Kind::System,
            n => Kind::Other(n as u8 - 3),
        }
    }

    /// The 1 MB slice of [`HANDLES`] the kind's addresses come from.
    pub const fn base(self) -> u32 {
        HANDLES.0 + self.slot() * 0x0010_0000
    }

    /// The 64 MB slice of [`HANDLES_HIGH`] the kind's addresses come from once its
    /// megabyte is used up.
    pub const fn high_base(self) -> u32 {
        HANDLES_HIGH.0 + self.slot() * 0x0400_0000
    }

    /// The most handles a kind can have.
    pub const MAX: u32 = NEAR + FAR;

    /// The address of the kind's `n`-th handle, counted from 0 in the order of opening;
    /// 0 (null) beyond [`Kind::MAX`].
    pub const fn nth(self, n: u32) -> u32 {
        if n < FIRST {
            self.base() + n * STEP
        } else if n < NEAR {
            // between the first ones: 31 to each gap
            let k = n - FIRST;
            self.base() + (k / (STEP / FINE - 1)) * STEP + (k % (STEP / FINE - 1) + 1) * FINE
        } else if n < Kind::MAX {
            self.high_base() + (n - NEAR) * FINE
        } else {
            0
        }
    }

    /// Which of the kind's handles an address is: the inverse of [`Kind::nth`]. `None`
    /// for an address that is no handle of this kind.
    pub const fn number(self, addr: u32) -> Option<u32> {
        let off = addr.wrapping_sub(self.base());
        if off < 0x0010_0000 {
            return if off % STEP == 0 {
                Some(off / STEP)
            } else if off % FINE == 0 {
                Some(FIRST + (off / STEP) * (STEP / FINE - 1) + (off % STEP) / FINE - 1)
            } else {
                None
            };
        }
        let off = addr.wrapping_sub(self.high_base());
        if off < 0x0400_0000 && off % FINE == 0 {
            return Some(NEAR + off / FINE);
        }
        None
    }

    /// The kind an address belongs to, if it is a handle at all (of either range).
    pub fn of(addr: u32) -> Option<Kind> {
        let off = addr.wrapping_sub(HANDLES.0);
        if off < HANDLES.1 {
            return Some(Kind::of_slot(off >> 20));
        }
        let off = addr.wrapping_sub(HANDLES_HIGH.0);
        (off < HANDLES_HIGH.1).then(|| Kind::of_slot(off >> 26))
    }
}

/// The distance between two handles of a kind (the harness's, for files).
const STEP: u32 = 0x100;

/// The things of one kind, each with its address. A closed handle's address is never given
/// out again, so a stale handle is found out, not mistaken for a newer thing.
pub struct Handles<T> {
    kind: Kind,
    items: Vec<Option<T>>,
}

impl<T> Handles<T> {
    pub fn new(kind: Kind) -> Self {
        Handles { kind, items: Vec::new() }
    }

    /// Takes a thing and answers its address; 0 (null, as a library that cannot open one
    /// more answers) when the kind has had [`Kind::MAX`] handles.
    pub fn open(&mut self, item: T) -> u32 {
        if self.items.len() as u64 >= Kind::MAX as u64 {
            return 0;
        }
        self.items.push(Some(item));
        self.kind.nth(self.items.len() as u32 - 1)
    }

    fn index(&self, addr: u32) -> Option<usize> {
        self.kind.number(addr).map(|n| n as usize).filter(|&n| n < self.items.len())
    }

    /// How many handles were opened so far, the closed ones included.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The thing a handle stands for; `None` for null, a closed handle, or an address that
    /// is no handle of this kind.
    pub fn get(&self, addr: u32) -> Option<&T> {
        self.items[self.index(addr)?].as_ref()
    }

    pub fn get_mut(&mut self, addr: u32) -> Option<&mut T> {
        let i = self.index(addr)?;
        self.items[i].as_mut()
    }

    /// Gives the thing back and retires the address.
    pub fn close(&mut self, addr: u32) -> Option<T> {
        let i = self.index(addr)?;
        self.items[i].take()
    }

    /// The open things with their addresses, in the order they were opened.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        let kind = self.kind;
        self.items.iter().enumerate().filter_map(move |(i, t)| t.as_ref().map(|t| (kind.nth(i as u32), t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Access, Mem};

    #[test]
    fn files_are_numbered_as_the_harness_numbers_them() {
        let mut files: Handles<&'static str> = Handles::new(Kind::File);
        let (i, o, e) = (files.open("stdin"), files.open("stdout"), files.open("stderr"));
        assert_eq!((i, o, e), (0x6f00_0000, 0x6f00_0100, 0x6f00_0200));
        let f = files.open("/x/y.hrc");
        assert_eq!(f, 0x6f00_0300);
        assert_eq!(files.get(f), Some(&"/x/y.hrc"));
        *files.get_mut(o).unwrap() = "log";
        assert_eq!(files.close(f), Some("/x/y.hrc"));
        // closed, null, wild
        assert_eq!(files.close(f), None);
        assert_eq!((files.get(f), files.get(0), files.get(f + 1)), (None, None, None));
        assert_eq!(files.open("again"), 0x6f00_0400); // the address is not reused
        assert_eq!(files.iter().map(|(a, _)| a).collect::<Vec<_>>(), vec![i, o, e, 0x6f00_0400]);
    }

    #[test]
    fn more_handles_than_the_first_4096() {
        let mut data: Handles<u32> = Handles::new(Kind::Other(1));
        let base = Kind::Other(1).base();
        assert_eq!(base, 0x6f40_0000);
        let mut all = Vec::new();
        for i in 0..140_000u32 {
            all.push(data.open(i));
        }
        // the first 4,096 as before; then between them, 8 apart; then the second range
        assert_eq!((all[0], all[1], all[4095]), (base, base + 0x100, base + 0x000f_ff00));
        assert_eq!((all[4096], all[4097], all[4096 + 30], all[4096 + 31]), (base + 8, base + 16, base + 0xf8, base + 0x108));
        assert_eq!(all[131_071], base + 0x000f_fff8);
        assert_eq!((all[131_072], all[131_073]), (0xd000_0000, 0xd000_0008));
        assert_eq!(data.len(), 140_000);
        // every address is its own, says its kind and its number, and finds its thing
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
        for (i, &a) in all.iter().enumerate() {
            assert!(a != 0 && a % 8 == 0);
            assert_eq!((Kind::of(a), Kind::Other(1).number(a), data.get(a)), (Some(Kind::Other(1)), Some(i as u32), Some(&(i as u32))));
            assert_eq!(Kind::File.number(a), None);
        }
        // not a handle: between two, in another kind's megabyte, not yet opened
        assert_eq!((data.get(base + 4), data.get(base - 8), data.get(Kind::Other(1).nth(140_000))), (None, None, None));
        assert_eq!(data.close(all[5000]), Some(5000));
        assert_eq!((data.get(all[5000]), data.get(all[5001])), (None, Some(&5001)));
        assert_eq!(data.iter().count(), 139_999);
        assert_eq!(data.iter().nth(5000), Some((all[5001], &5001)));
        // the rule's ends
        let last = Kind::Other(12);
        assert_eq!(Kind::MAX, 131_072 + 8_388_608);
        assert_eq!((last.nth(Kind::MAX - 1), last.nth(Kind::MAX)), (0xffff_fff8, 0));
        assert_eq!((last.number(0xffff_fff8), Kind::of(0xffff_fff8)), (Some(Kind::MAX - 1), Some(last)));
        assert_eq!((Kind::of(0xbfff_fff8), Kind::of(0xc000_0000)), (None, Some(Kind::File)));
        // none of it is memory
        let m = Mem::new();
        assert!(!m.is_mapped(all[4096], 1) && !m.is_mapped(all[131_072], 1));
    }

    #[test]
    fn a_handle_says_its_kind_and_is_not_memory() {
        let mut windows: Handles<u32> = Handles::new(Kind::Window);
        let w = windows.open(7);
        assert_eq!(w, 0x6f10_0000);
        assert_eq!((Kind::of(w), Kind::of(0x6f00_0100), Kind::of(0x6f35_0000)), (Some(Kind::Window), Some(Kind::File), Some(Kind::Other(0))));
        assert_eq!((Kind::of(0), Kind::of(0x7000_0000)), (None, None));
        // a record can hold it; reading through it is a fault
        let mut m = Mem::new();
        m.set_u32(crate::BSS.0, w);
        assert_eq!(windows.get(m.u32(crate::BSS.0)), Some(&7));
        assert_eq!(m.u32(w), 0);
        assert_eq!(m.fault().map(|f| f.access), Some(Access::Read));
    }
}
