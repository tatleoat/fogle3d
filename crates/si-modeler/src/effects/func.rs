// SPDX-License-Identifier: MIT
//! `func.c`: the update of a graph node's routine.

use super::ViewS;
use si_mem::{Mem, Outside, Ptr32};
use si_types::scene::GrafNode;

/// `SI_updateGnFunc` on a node and then on as many nodes along the `+0x1c` link as its signed
/// 16-bit count at +0x10 says (count + 1 calls in all; none for a negative count)
/// [soft 0x08176700 SI_adjustGnFunc]. The links are not tested for null, as in the original.
pub fn adjust_gn_func(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<GrafNode>) {
    let mut node = node;
    let mut n = m.i16(node.addr() + 0x10);
    while n > -1 {
        si_view::func::update_gn_func(m, &mut ViewS(o), node);
        node = Ptr32::new(m.u32(node.addr() + 0x1c));
        n = n.wrapping_sub(1);
    }
}
