// SPDX-License-Identifier: MIT
//! The C library's string routines on the image, as the dictionaries use them.
//!
//! They live in `si-mem` now (`si_mem::clib`), one copy for every crate; this module
//! keeps the names the dictionaries call them by. Nothing here reproduces a function of
//! the original program.

pub use si_mem::clib::{decimal, qsort, strcasecmp, strcat, strchr, strcmp, strcpy, strncmp, strrchr, strtol10};
