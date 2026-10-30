// SPDX-License-Identifier: MIT
//! `globals.c`'s initialisers of the parameter records, on the image (finding 0019,
//! section 6.5). What the fields mean is not established; each is written at its offset in
//! the record, in the original's order, with the original's calls of `getenv` (whose
//! argument is the address of the variable's name in `.data`, [`super::TEXTS`]).

use super::calls::BatchCalls;
use super::g;
use si_mem::{CStr32, Mem, Outside};

fn set16(m: &mut Mem, base: u32, offset: u32, v: i16) {
    m.set_i16(base + offset, v);
}

fn set32(m: &mut Mem, base: u32, offset: u32, v: u32) {
    m.set_u32(base + offset, v);
}

/// Whether the environment has a variable (the original tests `getenv`'s answer for null).
fn have(m: &mut Mem, o: &mut dyn Outside, name: u32) -> bool {
    !o.getenv(m, CStr32::new(name)).is_null()
}

/// [soft 0x080d3210 SI_manipulationInit]: `SI_glbManip`; five of its words come from the
/// environment, read in this order: `SI_NO_INTERACTIVE_FCV_UPD` (+0x26 is 1 when it is
/// absent), `SI_IK_OLD_JOINT_ROLL` (+0x2c), `SI_MANIPULATION_OPTIM` (+0x2e),
/// `SI_NO_OPTIM_UNTOLD_REL` (+0x36, 1 when absent), `SI_CLASSICAL_SCL_FIX` (+0x38).
pub fn manipulation_init(m: &mut Mem, o: &mut dyn Outside) {
    let r = g::SI_GLB_MANIP.addr();
    for (offset, v) in [(0x00, 0u32), (0x04, 0), (0x08, 2), (0x0c, 4), (0x10, 0), (0x14, 4), (0x18, 0)] {
        set32(m, r, offset, v);
    }
    for (offset, v) in [(0x1c, 0i16), (0x30, 0), (0x1e, 0), (0x20, 1), (0x22, 0), (0x24, 1)] {
        set16(m, r, offset, v);
    }
    let v = !have(m, o, 0x0867cd34) as i16;
    set16(m, r, 0x26, v);
    set16(m, r, 0x28, 0);
    set16(m, r, 0x2a, 0);
    let v = have(m, o, 0x0867cd4e) as i16;
    set16(m, r, 0x2c, v);
    let v = have(m, o, 0x0867cd63) as i16;
    set16(m, r, 0x2e, v);
    set16(m, r, 0x32, 0);
    set16(m, r, 0x34, 0);
    let v = !have(m, o, 0x0867cd79) as i16;
    set16(m, r, 0x36, v);
    let v = have(m, o, 0x0867cd90) as i16;
    set16(m, r, 0x38, v);
}

/// [soft 0x080d335c SI_animationInfoInit]: `SI_glbAnim`; two words come from the
/// environment: `SI_ENV_RND_FST_JNT` (+0x2e) and `SI_NO_ENVELOP_OPTIM` (+0x38, 1 when it
/// is absent). +0x18 is 0.15 and +0x28 0.01 (floats).
pub fn animation_info_init(m: &mut Mem, o: &mut dyn Outside) {
    let r = g::SI_GLB_ANIM.addr();
    set32(m, r, 0x18, 0.15f32.to_bits());
    set32(m, r, 0x28, 0.01f32.to_bits());
    for (offset, v) in [(0x00, 0i16), (0x02, 0), (0x04, 1), (0x08, 0), (0x0a, 0), (0x0c, 0)] {
        set16(m, r, offset, v);
    }
    set32(m, r, 0x10, 0);
    for (offset, v) in [(0x14, 0i16), (0x2c, 0), (0x1c, 1), (0x1e, 0), (0x20, 1), (0x22, 1), (0x24, 0), (0x26, 1), (0x30, 2), (0x32, 1)] {
        set16(m, r, offset, v);
    }
    set32(m, r, 0x34, 0);
    set16(m, r, 0x3c, 0);
    set16(m, r, 0x3a, 0);
    let v = have(m, o, 0x0867cda5) as i16;
    set16(m, r, 0x2e, v);
    let v = !have(m, o, 0x0867cdb8) as i16;
    set16(m, r, 0x38, v);
    set16(m, r, 0x3e, 0);
}

/// [soft 0x080d347c SI_incrementInfoInit]: `SI_glbIncrem`, twelve floats (three groups of
/// four: 0.01, 0.002, 0.01, 0.05 twice, then 0.02, 0.004, 0.02, 0.1).
pub fn increment_info_init(m: &mut Mem) {
    const V: [f32; 12] = [0.01, 0.002, 0.01, 0.05, 0.01, 0.002, 0.01, 0.05, 0.02, 0.004, 0.02, 0.1];
    for (i, v) in V.iter().enumerate() {
        m.set_f32(g::SI_GLB_INCREM.addr() + 4 * i as u32, *v);
    }
}

/// [soft 0x080d34f8 SI_duplicationInfoInit]: `SI_glbDuplic`; the second group only when
/// `all` (a `short`) is not 0.
pub fn duplication_info_init(m: &mut Mem, all: i16) {
    let r = g::SI_GLB_DUPLIC.addr();
    const FIRST: [i16; 23] = [1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0];
    for (i, v) in FIRST.iter().enumerate() {
        set16(m, r, 2 * i as u32, *v);
    }
    set16(m, r, 0x50, 0);
    set16(m, r, 0x52, 0);
    set16(m, r, 0x54, 0);
    if all != 0 {
        set32(m, r, 0x3c, 1.0f32.to_bits());
        set32(m, r, 0x40, 100.0f32.to_bits());
        set32(m, r, 0x48, 10.0f32.to_bits());
        set32(m, r, 0x30, 1);
        set16(m, r, 0x34, 1);
        set16(m, r, 0x36, 1);
        set16(m, r, 0x38, 1);
        set16(m, r, 0x44, 1);
        set32(m, r, 0x4c, 10);
    }
}

/// [soft 0x080d364c SI_schematicInfoInit]: `SI_glbSchem`.
pub fn schematic_info_init(m: &mut Mem) {
    let r = g::SI_GLB_SCHEM.addr();
    for (offset, v) in [(0x2c, 0u32), (0x30, 0), (0x34, 0), (0x38, 0), (0x00, 0), (0x04, 0), (0x08, 0), (0x0c, 0), (0x10, 1), (0x14, 0), (0x18, 2), (0x24, 0), (0x28, 0)] {
        set32(m, r, offset, v);
    }
    for (offset, v) in [(0x3c, 0i16), (0x3e, 1), (0x40, 1), (0x42, 0), (0x44, 0), (0x46, 0), (0x48, 1), (0x1e, 0), (0x20, 0), (0x4c, 0), (0x4e, 0)] {
        set16(m, r, offset, v);
    }
}
