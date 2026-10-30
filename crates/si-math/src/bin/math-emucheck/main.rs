// SPDX-License-Identifier: MIT
//! `math-emucheck <vectors directory>`: compares this crate with the original, routine by
//! routine, on the test vectors of the emulation harness (finding 0011, `re/tools/emu`).
//!
//! The vectors are what each routine of the Linux `soft` binary left behind when its
//! instructions were stepped through in a CPU emulator on made-up inputs
//! (`re/tools/si-math/emu/*.spec` and `gen_*.py` say which). They are derived from the
//! binary and are not in the repository. For every vector file the report gives the number
//! of cases, how many the port reproduces bit for bit, how many it reproduces given the
//! recorded results of the C library's mathematics (which no emulator can vouch for), and
//! how many differ.
use si_x87::vectors::{check_dir, print_report, Check};
use std::path::Path;

mod algebra;
mod geometry;
mod gl_matrix;
mod matrix;
mod matrix3tools;
mod matrix_gen_dbl;
mod util;
mod vector;

fn lookup(name: &str) -> Option<Check> {
    algebra::lookup(name).or_else(|| matrix3tools::lookup(name)).or_else(|| matrix_gen_dbl::lookup(name)).or_else(|| gl_matrix::lookup(name)).or_else(|| vector::lookup(name)).or_else(|| matrix::lookup(name)).or_else(|| geometry::lookup(name))
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: math-emucheck <directory of .vec files>");
        std::process::exit(2);
    };
    match check_dir(Path::new(&dir), &lookup) {
        Ok(rows) => {
            if !print_report(&rows) {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("math-emucheck: {e}");
            std::process::exit(2);
        }
    }
}
