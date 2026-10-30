// SPDX-License-Identifier: MIT
//! The mathematics core of Softimage|3D 4.0: vectors, matrices, rotations, transformations
//! (finding 0008).
//!
//! Everything else in the program computes through these functions, and the original
//! computes on the x87 unit: intermediate values have a 64-bit significand and are rounded to
//! `float` or `double` only when stored. To get the same bits, every function here repeats
//! the original's instruction sequence on a software model of that unit:
//!
//! - [`x87`]: the register value [`x87::F80`] with the unit's arithmetic, comparisons and
//!   rounding stores; checked against a real unit in its tests.
//! - [`trig`]: `sin`, `cos`, `tan`, `asin`, `acos` and `fpatan` as the C library of the time
//!   computes them with the unit's transcendental instructions. These cannot be bit-exact
//!   (they differ between processors); the module is deterministic and as close as measured.
//! - [`types`]: matrices and vectors.
//! - one module per source file of the original: [`algebra`] (`algebra.c`), [`matrix3tools`]
//!   (`matrix3tools.c`), [`matrix_gen_dbl`] (`matrixGenDbl.c`), [`gl_matrix`] (`gl_matrix.c`),
//!   [`vector`] (`vector.c`, doubles), [`matrix`] (`matrix.c`, the native renderer's camera
//!   matrices) and [`geometry`] (`geometry.c`, the renderer's clipper and scan-line filing).
//!
//! A function that returns a value in a register hands back an [`x87::F80`], unrounded, as
//! the original does; the caller rounds it where the original's caller stores it.
pub mod algebra;
pub mod geometry;
pub mod gl_matrix;
pub mod matrix;
pub mod matrix3tools;
pub mod matrix_gen_dbl;
mod text;
pub mod types;
pub mod vector;
// the x87 model lives in the shared `si-x87` crate; the old paths keep working
pub use si_x87::{trig, x87};
