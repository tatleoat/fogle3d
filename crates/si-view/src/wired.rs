// SPDX-License-Identifier: MIT
//! The package's seam: five routines of the package's own source files that stay calls by
//! address.
//!
//! Nothing here reproduces a function of the original. The routines of `graflist.c` that
//! were ported first call four routines that were ported later: `SI_graflistDeleteNode
//! (0x0832d75c)` of the same file, `SI_nselectionSelectContent (0x083465a4)` of
//! `nselections.c`, `SI_schemComputeHierarchiesBBox (0x0833bd74)` and
//! `SI_schematicHierOperation (0x0833c5dc)` of `graphsort.c`; the last calls a fifth,
//! `si_schemElemOper (0x0833c15c)`. Their vectors answer those from recorded stand-ins,
//! so the ports keep making them through [`Outside::call`], every caller alike, and
//! [`Wired`] closes the seam: it hands the five to their ports (`SI_schemComputeHierarchiesBBox`
//! since finding 0018, section 23.12) and everything else to the world it wraps. A program runs the
//! crate under `Wired`; a check runs it under its recorded world, and under `Wired` for
//! the vector files in which the four ran themselves.

use crate::outside::{self, Outside, Routine};
use si_mem::{Mem, Ptr32};

/// The five routines of the seam.
pub const SEAM: [&Routine; 5] = [
    &outside::SI_GRAFLIST_DELETE_NODE,
    &outside::SI_NSELECTION_SELECT_CONTENT,
    &outside::SI_SCHEM_COMPUTE_HIERARCHIES_BBOX,
    &outside::SI_SCHEMATIC_HIER_OPERATION,
    &outside::SI_SCHEM_ELEM_OPER,
];

/// Runs the port of a routine of the seam with the arguments of a call by address; `None`
/// for any other routine. `o` is the world the port itself calls out to.
pub fn call_own(m: &mut Mem, o: &mut dyn Outside, routine: &Routine, a: &[u32]) -> Option<u32> {
    let arg = |k: usize| a.get(k).copied().unwrap_or(0);
    Some(match routine.addr {
        0x0832d75c => crate::graflist::graflist_delete_node(m, o, Ptr32::new(arg(0)), Ptr32::new(arg(1)), arg(2) as i16) as u32,
        0x083465a4 => {
            crate::nselections::nselection_select_content(m, o, Ptr32::new(arg(0)), arg(1) as i32);
            0
        }
        0x0833c5dc => crate::graphsort::schematic_hier_operation(m, o, arg(0), Ptr32::new(arg(1)), Ptr32::new(arg(2)), arg(3) as i32, arg(4) as i32) as u32,
        0x0833c15c => {
            crate::graphsort::schem_elem_oper(m, o, Ptr32::new(arg(0)), Ptr32::new(arg(1)), arg(2) as i32, arg(3) as i16, arg(4), arg(5));
            0
        }
        0x0833bd74 => {
            crate::graphsort::schem_compute_hierarchies_bbox(m, o, Ptr32::new(arg(0)), arg(1) as i16, arg(2) as i16);
            0
        }
        _ => return None,
    })
}

/// An outside world with the seam closed: the routines of the seam that have a port run as it, with this
/// same world as theirs; every other call goes to the world inside.
pub struct Wired<O: Outside> {
    pub inner: O,
}

impl<O: Outside> Wired<O> {
    pub fn new(inner: O) -> Wired<O> {
        Wired { inner }
    }
}

impl<O: Outside> Outside for Wired<O> {
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        match call_own(m, self, routine, args) {
            Some(v) => v,
            None => self.inner.call(m, routine, args),
        }
    }
}
