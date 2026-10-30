// SPDX-License-Identifier: MIT
//! `SI_metaballInit`, the start of the metaball code at program start and in the stand-alone renderer (units last2
//! and lic1, finding 0038, 2026-10-29).

use crate::licence::meta_license_check_for_attribute;
use crate::surface::meta_circle_init;
use si_mem::{Mem, Outside, Routine};

/// `VCInit (0x08575ca0)`, `user.c` (ported in si-render, which this crate does not depend on): the metaball
/// library's tables; 0, or an error code.
pub const VC_INIT: Routine = Routine::new("VCInit", 0x0857_5ca0);

/// [soft 0x0845abc8 SI_metaballInit]: starts the metaball library (`VCInit`, through `o`), prints
/// "VCInit() failed: %d\n" on stdout when it answers non-zero (and goes on), fills the ring table
/// ([`meta_circle_init`]) and ends with [`meta_license_check_for_attribute`], which in the stand-alone renderer
/// (`SI_standaloneRenderer` set) takes the write bit off the metaball attributes of `SI_ATD`. Its licence client
/// call is not made: when no flag decides, the answer of a licensed run stands in (see [`crate::licence`]).
pub fn si_metaball_init(m: &mut Mem, o: &mut dyn Outside) {
    let r = o.call(m, VC_INIT, &[]).eax as i32;
    if r != 0 {
        o.print(1, format!("VCInit() failed: {r}\n").as_bytes());
    }
    meta_circle_init(m);
    meta_license_check_for_attribute(m);
}
