// SPDX-License-Identifier: MIT
//! `particle_init.c` of Softimage|3D 4.0 (0x084c0450): the particle tool's start-up (finding 0045).

use si_mem::Mem;

/// The particle status record `si_particleStatus` area (0x08822040 .. 0x08822104, our name for the record).
pub const PARTICLE_STATUS: u32 = 0x0882_2040;

/// Sets the particle tool's settings to their defaults, once [soft 0x084c0450 SI_initParticle]: when the flag at
/// +0xc0 is already above 0 nothing happens. Else the flag becomes 1 and the record at 0x08822040 is filled: +0x00 0
/// (the status pointer), +0x04 `strdup("flock")` (the string at 0x086d7a40), +0x10 1.0, +0x18 and +0x1c 17, +0x24 10,
/// +0x30 2, +0x38 1, +0x3c 30, +0x40 1, +0x48 1, +0xa8 30, +0xac 1, the float 0.05 (at 0x08661300) in the four vectors of
/// three at +0x78, +0x84, +0x90, +0x9c, and every other word up to +0xbc 0.
pub fn si_init_particle(m: &mut Mem) {
    let b = PARTICLE_STATUS;
    if m.i32(b + 0xc0) >= 1 {
        return;
    }
    m.set_i32(b + 0xc0, 1);
    m.set_u32(b, 0);
    let name = m.strdup(0x086d_7a40);
    let w = m.u32(0x0866_1300);
    m.set_u32(b + 0x8, 0);
    m.set_u32(b + 0xc, 0);
    m.set_u32(b + 0x10, 0x3f80_0000);
    m.set_u32(b + 0x14, 0);
    m.set_u32(b + 0x4, name);
    m.set_u32(b + 0x18, 0x11);
    m.set_u32(b + 0x1c, 0x11);
    m.set_u32(b + 0x20, 0);
    m.set_u32(b + 0x24, 10);
    m.set_u32(b + 0x28, 0);
    m.set_u32(b + 0x2c, 0);
    m.set_u32(b + 0x30, 2);
    m.set_u32(b + 0x34, 0);
    m.set_u32(b + 0x38, 1);
    m.set_u32(b + 0x3c, 0x1e);
    m.set_u32(b + 0x40, 1);
    m.set_u32(b + 0x44, 0);
    m.set_u32(b + 0x48, 1);
    for k in 0..12 {
        m.set_u32(b + 0x4c + 4 * k, 0);
    }
    for k in 0..3 {
        for v in [0x78u32, 0x84, 0x90, 0x9c] {
            m.set_u32(b + v + 4 * k, w);
        }
    }
    m.set_u32(b + 0xa8, 0x1e);
    m.set_u32(b + 0xb4, 0);
    m.set_u32(b + 0xb8, 0);
    m.set_u32(b + 0xbc, 0);
    m.set_u32(b + 0xb0, 0);
    m.set_u32(b + 0xac, 1);
}
