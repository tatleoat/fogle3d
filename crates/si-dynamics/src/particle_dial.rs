// SPDX-License-Identifier: MIT
//! `particle_dial.c` of Softimage|3D 4.0 (0x084be3c0 …): the small set-up file routines next to the particle dialog
//! (finding 0045). The dialog itself (`si_dialogParticle`, `particleGetDg`, `particleSetDg`) is user interface and not
//! ported. The particle status record is at 0x08822040; its first word counts how many set-ups were read.

use crate::particle_outside::ParticleCalls;
use si_mem::{Mem, Outside, Routine};

const FOPEN: Routine = Routine::new("fopen", 0x080c_4e18);
const FCLOSE: Routine = Routine::new("fclose", 0x080c_4958);
const FFLUSH: Routine = Routine::new("fflush", 0x080c_3be8);
const FERROR: Routine = Routine::new("ferror", 0x080c_3878);
const SI_PARTICLE_STATUS: u32 = 0x0882_2040;
const SI_SAVE_ELEM_ERROR_CODE: u32 = 0x086c_ebb0;
/// The string "w".
const MODE_W: u32 = 0x086d_7a31;

/// Writes the particle set-up to a file [soft 0x084c034c SI_particleSetupSave]: the file is opened for writing (a failure sets
/// the error word 0x086cebb0 to 1 and gives false), the status record is written with `particleWriteParameters`, the stream
/// is flushed and checked (`ferror` non-0 sets the error word to 2 and gives false) and closed.
pub fn si_particle_setup_save(m: &mut Mem, o: &mut dyn Outside, path: u32) -> bool {
    let stream = o.call(m, FOPEN, &[path, MODE_W]).eax;
    if stream == 0 {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 1);
        return false;
    }
    o.particle_write_parameters(m, stream, SI_PARTICLE_STATUS);
    o.call(m, FFLUSH, &[stream]);
    let ok = o.call(m, FERROR, &[stream]).eax == 0;
    if !ok {
        m.set_u32(SI_SAVE_ELEM_ERROR_CODE, 2);
    }
    o.call(m, FCLOSE, &[stream]);
    ok
}

/// Whether the particle set-up is the default one [soft 0x084c0418 SI_particleSetupDefault]: no set-up has been read (the counter
/// at the head of the status record is not above 0, as a signed number).
pub fn si_particle_setup_default(m: &Mem) -> bool {
    m.i32(SI_PARTICLE_STATUS) <= 0
}

/// Forgets the particle set-up [soft 0x084c0438 SI_particleSetupReset]: the counter at the head of the status record is set to 0.
pub fn si_particle_setup_reset(m: &mut Mem) {
    m.set_u32(SI_PARTICLE_STATUS, 0);
}
