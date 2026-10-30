// SPDX-License-Identifier: MIT
//! Lattices, clusters of points and the fitting records of Softimage|3D 4.0 on the shared
//! memory image (findings 0024 and 0028).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`; a routine that is not ported is written `name (0xADDRESS)`.
//! Source files: `lattice.c` ([`lattice`]), `klusters.c` and `klusters_b.c` ([`klusters`]),
//! `fitting.c` ([`fitting`]), `deformation.c` ([`deformation`], what the update reaches);
//! the envelope (a skin deformed by a skeleton, finding 0029): `envelop.c` ([`envelop`]),
//! `envelopset.c` ([`envelopset`]), `envtools.c` ([`envtools`]),
//! with their records in [`env_records`], their globals in [`env_globals`] and the routines
//! they call outside the crate in [`env_outside`].
//!
//! Every function here takes the image ([`si_mem::Mem`]) and the arguments the original
//! takes. Records are read and written in place: `Model`, `GrafList` and `GrafNode` are
//! `si-types` layouts; the lattice, the two fitting records and the cluster are `record!`
//! layouts of this crate ([`records`]); the geometry a lattice moves (mesh, patch, face,
//! spline, NURBS curve and surface) is addressed by named offsets ([`records::geom`]),
//! because those records have no typed layout on the image yet. Every `SI_malloc`,
//! `SI_calloc`, `SI_realloc`, `strdup` and `free` of the original is made, with its size
//! and in its order, so the heap a routine leaves is the original's block for block (which
//! `deform-emucheck` compares with the emulated original). Floating-point work follows the
//! instruction listing on `si_x87::F80`: a value is rounded where the original stores it.
//! The routines of `algebra.c` the original calls are `si-math`'s ports. Drawing, dialogs
//! and every routine of another source file are reached through [`outside::Outside`], by
//! the original routine's name and address.

pub mod deformation;
pub mod env_globals;
pub mod env_outside;
pub mod env_records;
pub mod envelop;
pub mod envelopset;
pub mod envtools;
pub mod fitting;
pub mod globals;
pub mod klusters;
pub mod lattice;
pub mod outside;
pub mod records;
pub mod routed;

#[cfg(test)]
mod env_tests;
#[cfg(test)]
mod tests;

pub use outside::{Drand48, NoOutside, Outside, Routine};
pub use records::{Kluster, Lattice, PtchFitting, SplFitting};
pub use si_mem::{CStr32, Mem, Opaque, Ptr32};
pub use si_types::model::Model;

/// Three floats at an address: an `SI_Vector` as the original's pointers see it.
pub type V3 = [f32; 3];

pub(crate) fn rd3(m: &Mem, a: u32) -> V3 {
    [m.f32(a), m.f32(a.wrapping_add(4)), m.f32(a.wrapping_add(8))]
}

pub(crate) fn wr3(m: &mut Mem, a: u32, v: V3) {
    m.set_f32(a, v[0]);
    m.set_f32(a.wrapping_add(4), v[1]);
    m.set_f32(a.wrapping_add(8), v[2]);
}

/// The current element dictionary's address: `SI_elemDicts + SI_procData->dict × 0x70`, as
/// every routine that hands it to the editing layer computes it.
pub(crate) fn cur_dict(m: &Mem) -> u32 {
    let proc = m[globals::SI_PROC_DATA];
    let d = m[proc].dict as u32;
    m[globals::SI_ELEM_DICTS].addr().wrapping_add(d.wrapping_mul(0x70))
}
