// SPDX-License-Identifier: MIT
//! invdynamic.c of the dynamics simulation, the walkers over a model tree for fans and wind (finding 0035).

use crate::invdynamic_fan::invdyna_get_bbox_fan_frce;
use crate::invdynamic_mesh::invdyna_get_mesh_wind_frce;
use crate::invdynamic_wind::invdyna_get_bbox_wind_frce;
use crate::outside::DynaCalls;
use si_mem::{Mem, Outside};
use si_x87::x;

/// What the node walkers do with one model kind.
#[derive(PartialEq, Clone, Copy)]
enum Act {
    /// Only the children are visited.
    Skip,
    /// The box force is added with this second model.
    Box(u32),
}

/// [soft 0x0849b730 invdynaGetNodeFanFrce]: adds the force and torque of the fan `wMdl` on the model `mdl2` (or `mdl3`
/// when null) to `out1` and `out2`, by the box force of its kind (kinds 1, 4, 8, 10 use the box of `mdl2`, kind 5
/// none of the second model; kind 0 with a `+0x304` record and the other kinds add nothing), then, when `flag` is not
/// 0, the same for the child chains `+0x2c4` and `+0x2c0` of `mdl2`.
pub fn invdyna_get_node_fan_frce(m: &mut Mem, mdl1: u32, mdl2: u32, mdl3: u32, out1: u32, out2: u32, flag: i32) {
    let chosen = if mdl2 == 0 { mdl3 } else { mdl2 };
    let act = match m.u32(chosen + 0x30) {
        0 => {
            if m.u32(chosen + 0x304) != 0 {
                return;
            }
            Act::Skip
        }
        1 | 4 | 8 | 10 => Act::Box(mdl2),
        5 => Act::Box(0),
        _ => Act::Skip,
    };
    if let Act::Box(second) = act {
        invdyna_get_bbox_fan_frce(m, mdl1, second, mdl3, out1, out2);
    }
    recurse_children(m, mdl2, flag, |m, child| invdyna_get_node_fan_frce(m, mdl1, child, mdl3, out1, out2, flag));
}

fn recurse_children(m: &mut Mem, mdl2: u32, flag: i32, mut f: impl FnMut(&mut Mem, u32)) {
    if flag as i16 != 0 {
        let c = m.u32(mdl2 + 0x2c4);
        if c != 0 {
            f(m, c);
        }
        let c = m.u32(mdl2 + 0x2c0);
        if c != 0 {
            f(m, c);
        }
    }
}

/// [soft 0x0849ccbc invdynaGetNodeWindFrce]: the wind counterpart of the fan walker. When the solver record
/// (`[mdl1+0x304]+0x58`) is 2 a kind 4 model goes through the mesh force `invdynaGetMeshWindFrce`; otherwise
/// (and for the kinds 1, 8, 10 in both cases) the box force is taken, kinds 1, 8, 10 with `mdl3` as the second
/// model and kind 5 with none. Kind 0 with a `+0x304` record adds nothing.
pub fn invdyna_get_node_wind_frce(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, mdl3: u32, out1: u32, out2: u32, flag: i32) {
    let chosen = if mdl2 == 0 { mdl3 } else { mdl2 };
    let two = m.u32(m.u32(mdl1 + 0x304) + 0x58) == 2;
    let kind = m.u32(chosen + 0x30);
    match kind {
        0 => {
            if m.u32(chosen + 0x304) != 0 {
                return;
            }
        }
        4 if two => invdyna_get_mesh_wind_frce(m, o, mdl1, mdl2, mdl3, out1, out2),
        4 => invdyna_get_bbox_wind_frce(m, mdl1, mdl2, mdl3, out1, out2),
        1 | 8 | 10 => invdyna_get_bbox_wind_frce(m, mdl1, mdl3, mdl3, out1, out2),
        5 => invdyna_get_bbox_wind_frce(m, mdl1, 0, mdl3, out1, out2),
        _ => {}
    }
    recurse_children(m, mdl2, flag, |m, child| invdyna_get_node_wind_frce(m, &mut *o, mdl1, child, mdl3, out1, out2, flag));
}

/// The part both entry points share: the model and flag for the walker, or `None` when `mdl2` is not a fan model.
fn entry_model(m: &Mem, mdl2: u32) -> (u32, i32) {
    if m.u32(mdl2 + 0x2ec) == 0xc {
        return (mdl2, 1);
    }
    let c = m.u32(m.u32(mdl2 + 0x2c4) + 0x2c0);
    if c != 0 && m.u32(mdl2 + 0x31c) == 1 {
        (c, 1)
    } else {
        (0, 0)
    }
}

/// Turns the old `wDir` by the `+0x220` .. `+0x240` matrix of the model (each component rounded to f32 on store).
fn turn_w_dir(m: &mut Mem, mdl2: u32) {
    let q = m.u32(mdl2 + 0x300);
    let f = |m: &Mem, a: u32| x(m.f32(a));
    let (wd0, wd1, wd2) = (f(m, 0x08822018), f(m, 0x0882201c), f(m, 0x08822020));
    let nx = ((f(m, q + 0x224) * wd1) + (f(m, q + 0x220) * wd0)) + (f(m, q + 0x228) * wd2);
    let ny = ((f(m, q + 0x22c) * wd0) + (f(m, q + 0x230) * wd1)) + (f(m, q + 0x234) * wd2);
    let nz = ((wd1 * f(m, q + 0x23c)) + (wd0 * f(m, q + 0x238))) + (wd2 * f(m, q + 0x240));
    m.set_f32(0x08822018, nx.to_f32());
    m.set_f32(0x0882201c, ny.to_f32());
    m.set_f32(0x08822020, nz.to_f32());
}

/// [soft 0x0849b844 si_invdynaGetFanFrce]: the force and torque of the fan constraint `info` (its model at
/// `+0x10`) on the body `mdl2`, written to `out1` and `out2` (cleared first). Fans of kind 2 or 12 only: `wMdl`,
/// `wAmpl` (the absolute strength `[fan+0x104]`), the direction `wDir` and position `wPos` of the fan are taken
/// from the constraint hooks, the direction is turned into the frame of a kind 2 body, and the node walker runs.
pub fn si_invdyna_get_fan_frce(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, info: u32, out1: u32, out2: u32) {
    let g = m.u32(info + 0x10);
    if g == 0 {
        return;
    }
    for k in 0..3 {
        m.set_u32(out1 + 4 * k, 0);
        m.set_u32(out2 + 4 * k, 0);
    }
    let (model, flag) = entry_model(m, mdl2);
    let kind = m.u32(mdl2 + 0x2ec);
    if kind != 2 && kind != 0xc {
        return;
    }
    m.set_u32(0x08822010, g);
    let amp = x(m.f32(g + 0x104)).abs();
    m.set_f32(0x08822014, amp.to_f32());
    o.si_constraint_orientation_get(m, g, mdl1, 1, 1, 0x78, 0x08822018);
    o.si_constraint_position_get(m, g, mdl1, 1, 0x08822024);
    if kind == 2 {
        turn_w_dir(m, mdl2);
    }
    invdyna_get_node_fan_frce(m, mdl1, model, mdl2, out1, out2, flag);
}

/// [soft 0x0849cdc8 si_invdynaGetWindFrce]: the same for a wind constraint (no position is taken), ending in the
/// wind node walker.
pub fn si_invdyna_get_wind_frce(m: &mut Mem, o: &mut dyn Outside, mdl1: u32, mdl2: u32, info: u32, out1: u32, out2: u32) {
    let g = m.u32(info + 0x10);
    if g == 0 {
        return;
    }
    for k in 0..3 {
        m.set_u32(out1 + 4 * k, 0);
        m.set_u32(out2 + 4 * k, 0);
    }
    let (model, flag) = entry_model(m, mdl2);
    let kind = m.u32(mdl2 + 0x2ec);
    if kind != 2 && kind != 0xc {
        return;
    }
    m.set_u32(0x08822010, g);
    let amp = x(m.f32(g + 0x104)).abs();
    m.set_f32(0x08822014, amp.to_f32());
    o.si_constraint_orientation_get(m, g, mdl1, 1, 1, 0x78, 0x08822018);
    if kind == 2 {
        turn_w_dir(m, mdl2);
    }
    invdyna_get_node_wind_frce(m, o, mdl1, model, mdl2, out1, out2, flag);
}
