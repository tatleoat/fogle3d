// SPDX-License-Identifier: MIT
//! `SI_lightDispose` of `light_fct.c` (finding 0023, section 36): the destructor of a light
//! record. (The record's constructor, `SI_lightAllocate`, and `si_lightInitialize` are
//! `si-elemio`'s.)

use si_mem::Mem;

si_mem::outside_calls! {
    /// What `SI_lightDispose` calls in other source files.
    pub trait LightFctCalls {
        /// `SI_udfDispose (0x0854c3c0)`, `udf_fct.c` (ported in `si-hrc`): the shader reference.
        fn si_udf_dispose(udf: u32) = "SI_udfDispose", 0x0854c3c0;
        /// `SI_userDataDispose (0x084619e4)`, `model` (ported in `si-model`, not on the image): the
        /// address of the head pointer of the light's user data.
        fn si_user_data_dispose(head: u32) = "SI_userDataDispose", 0x084619e4;
    }
}

/// Frees a light [soft 0x082818f0 SI_lightDispose]: `SI_lightDispose(light)`; null does
/// nothing. The block at +0x168 is freed and cleared; the sun record at +0x16c: the block
/// its first word points to is freed and that word cleared, then the record itself freed and
/// +0x16c cleared; the two shader references at +0x178 and +0x17c go to `SI_udfDispose` (the
/// fields are not cleared); user data at +0x220, when set, to `SI_userDataDispose` with the
/// address of the field; last the light itself is freed. Class: copy.
pub fn light_dispose(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) {
    use LightFctCalls as _;
    if light == 0 {
        return;
    }
    let p = m.u32(light + 0x168);
    if p != 0 {
        m.free(p);
        m.set_u32(light + 0x168, 0);
    }
    let sun = m.u32(light + 0x16c);
    if sun != 0 {
        let inner = m.u32(sun);
        if inner != 0 {
            m.free(inner);
            let sun = m.u32(light + 0x16c);
            m.set_u32(sun, 0);
        }
    }
    let sun = m.u32(light + 0x16c);
    if sun != 0 {
        m.free(sun);
        m.set_u32(light + 0x16c, 0);
    }
    for off in [0x178, 0x17c] {
        let udf = m.u32(light + off);
        if udf != 0 {
            o.si_udf_dispose(m, udf);
        }
    }
    if m.u32(light + 0x220) != 0 {
        o.si_user_data_dispose(m, light + 0x220);
    }
    m.free(light);
}
