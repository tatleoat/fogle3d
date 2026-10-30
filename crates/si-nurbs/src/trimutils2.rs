// SPDX-License-Identifier: MIT
//! `trimUtils.c`, the two table accessors (finding 0044, section "Trim8"): the linear forms of a surface's trim and projection composites.

use crate::curveinter2::SI_CURVE_INTER_BLD_LIN_FROM_COMP;
use crate::trimstat::update_step_and_order;
use si_mem::{Mem, Outside};

/// Both accessors: the linear curve of each of the `count` composites of the table (records with the curve table at +8 and its number
/// at +4; `BldLinFromComp` outside), their step / order words brought up to the composite's (`updateStepAndOrder`), as a new table.
fn linear_table(m: &mut Mem, o: &mut dyn Outside, count: i32, tab: u32) -> u32 {
    let sp = m.sp();
    let maxo = m.local(4);
    let out = m.si_calloc(count as u32, 4);
    let mut i: i32 = 0;
    while i < count {
        let comp = m.u32(tab.wrapping_add((i as u32).wrapping_mul(4)));
        let c = o.call(m, SI_CURVE_INTER_BLD_LIN_FROM_COMP, &[m.u32(comp + 8), m.u32(comp + 4), maxo]).eax;
        m.set_u32(out.wrapping_add((i as u32).wrapping_mul(4)), c);
        let comp = m.u32(tab.wrapping_add((i as u32).wrapping_mul(4)));
        update_step_and_order(m, comp, c);
        i += 1;
    }
    m.set_sp(sp);
    out
}

/// [soft 0x084f8b78 SI_nurbsSrfGetTrimCrv]: `(srf, *count)`: a null surface gives `*count` = 0 and 0; else a table of the linear curves of the
/// trim composites (count at +0x60, table at +0x64), `*count` the count.
pub fn srf_get_trim_crv(m: &mut Mem, o: &mut dyn Outside, srf: u32, pn: u32) -> u32 {
    if srf == 0 {
        m.set_u32(pn, 0);
        return 0;
    }
    let out = linear_table(m, o, m.i32(srf + 0x60), m.u32(srf + 0x64));
    m.set_u32(pn, m.u32(srf + 0x60));
    out
}

/// [soft 0x084f8ae8 SI_nurbsSrfGetProjCrv]: the same for the projection composites (count at +0x68, table at +0x6c); `*count` is stored first
/// and a count of 0 gives 0 without a table.
pub fn srf_get_proj_crv(m: &mut Mem, o: &mut dyn Outside, srf: u32, pn: u32) -> u32 {
    if srf == 0 {
        m.set_u32(pn, 0);
        return 0;
    }
    m.set_u32(pn, m.u32(srf + 0x68));
    if m.u32(srf + 0x68) == 0 {
        return 0;
    }
    linear_table(m, o, m.i32(srf + 0x68), m.u32(srf + 0x6c))
}
