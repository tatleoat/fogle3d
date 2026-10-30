// SPDX-License-Identifier: MIT
//! `dynadial.c`: the dynamics chains of the scene's graph list, set up after a load (finding 0017, section 15).

use crate::{constr, playback, settings_dyn, settings_rec};
use si_mem::{Mem, Outside};

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

/// The first graf node of the current process's graf list (`SI_grafLists + 0x24 * process`, word +4).
fn first_graf_node(m: &Mem) -> u32 {
    let pd = m.u32(SI_PROC_DATA);
    let lists = m.u32(SI_GRAF_LISTS);
    m.u32(lists.wrapping_add(m.u32(pd.wrapping_add(4)).wrapping_mul(0x24)).wrapping_add(4))
}

/// SI_dynamicChainsUpdate (0x081cc1cc): for every graf node of kind 2 or 3 (a model) in the current graf list, in two
/// passes. Pass one: a model with a chain record (`+0x304`) has its "simulated keys" flag looked up
/// (`SI_dynaSimulationCheckExistence`), its chain linked (`SI_dynachnSetChnListPtrs` for a null, `+0x314 = model`, `+0x30c =
/// +0x310 = 0` for any other kind); a joint (kind 5) with a dynamics matter record whose first child's father (`+0x2c4`, then
/// `+0x2c0`) has a matter and no chain record gets the four dynamics records of a chain root (chain variables, joint
/// property, joint variables, kinematic state); a model with a constraint list (`+0x2fc`) has it cleaned. Pass two: every
/// model with a chain record gets `SI_dynachnGeometryUpdate`.
pub fn si_dynamic_chains_update(m: &mut Mem, o: &mut dyn Outside) {
    let mut n = first_graf_node(m);
    while n != 0 {
        if m.u32(n).wrapping_sub(2) < 2 {
            let model = m.u32(n + 4);
            if m.u32(model + 0x304) != 0 {
                playback::si_dyna_simulation_check_existence(m, o, model);
                if m.u32(model + 0x30) == 0 {
                    settings_rec::si_dynachn_set_chn_list_ptrs(m, model);
                } else {
                    m.set_u32(model + 0x314, model);
                    m.set_u32(model + 0x30c, 0);
                    m.set_u32(model + 0x310, 0);
                }
            }
            if m.u32(model + 0x30) == 5 && m.u32(model + 0x2f4) != 0 {
                let first = m.u32(model + 0x2c4);
                let root = m.u32(first.wrapping_add(0x2c0));
                if root != 0 && m.u32(root + 0x2f4) != 0 && m.u32(root + 0x304) == 0 {
                    let a = settings_dyn::si_dynachnvar_allocate(m, 0);
                    m.set_u32(root + 0x304, a);
                    let b = si_hrc::image::chunks::dynajntprop_allocate(m);
                    m.set_u32(root + 0x2f8, b);
                    let c = settings_dyn::si_dynajntvar_allocate(m);
                    m.set_u32(root + 0x300, c);
                    let d = si_kine::image::qstretch::kine_state_allocate(m);
                    m.set_u32(root + 0x2f0, d);
                }
            }
            if m.u32(model + 0x2fc) != 0 {
                constr::si_modeldyna_constr_list_clean(m, model);
            }
        }
        n = m.u32(n + 0x1c);
    }
    let mut n = first_graf_node(m);
    while n != 0 {
        if m.u32(n).wrapping_sub(2) < 2 {
            let model = m.u32(n + 4);
            if m.u32(model + 0x304) != 0 {
                settings_dyn::si_dynachn_geometry_update(m, o, model);
            }
        }
        n = m.u32(n + 0x1c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::Trace;

    /// A graf list of one model node: a joint with a matter record, whose first son's brother has a matter record and no chain
    /// record, gets the four dynamics records of a chain root; a node of another kind is left alone.
    #[test]
    fn joint_with_matter_gives_its_root_the_chain_records() {
        let mut m = Mem::new();
        let pd = m.si_malloc(0x40);
        let lists = m.si_malloc(0x100);
        m.set_u32(SI_PROC_DATA, pd);
        m.set_u32(SI_GRAF_LISTS, lists);
        m.set_u32(pd + 4, 1);
        let node = |m: &mut Mem, kind: u32, model: u32| {
            let n = m.si_malloc(0x40);
            m.set_u32(n, kind);
            m.set_u32(n + 4, model);
            n
        };
        let matter = m.si_malloc(0x40);
        let joint = m.si_malloc(0x430);
        let son = m.si_malloc(0x430);
        let root = m.si_malloc(0x430);
        let other = m.si_malloc(0x430);
        m.set_u32(joint + 0x30, 5);
        m.set_u32(joint + 0x2f4, matter);
        m.set_u32(joint + 0x2c4, son);
        m.set_u32(son + 0x2c0, root);
        m.set_u32(root + 0x2f4, matter);
        let (n1, n2) = (node(&mut m, 2, joint), node(&mut m, 5, other));
        m.set_u32(n1 + 0x1c, n2);
        m.set_u32(lists + 0x24 + 4, n1);
        let mut o = Trace::with(|_m, r, _a| panic!("{} was asked for", r.name));
        si_dynamic_chains_update(&mut m, &mut o);
        for at in [0x304, 0x2f8, 0x300, 0x2f0] {
            assert_ne!(m.u32(root + at), 0, "+{at:#x}");
        }
        assert_eq!(m.u32(other + 0x304), 0);
        assert_eq!(m.u32(joint + 0x304), 0);
    }
}
