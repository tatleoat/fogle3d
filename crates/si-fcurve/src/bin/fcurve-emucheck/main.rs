// SPDX-License-Identifier: MIT
//! `fcurve-emucheck <vectors directory>`: compares this crate with the original, routine by
//! routine, on the test vectors of the emulation harness (finding 0011, `re/tools/emu`).
//!
//! The vectors come from `re/tools/si-fcurve/emu_gen.py`: each is one call of a routine of
//! the Linux `soft` binary, stepped through in a CPU emulator, on a made-up curve. They are
//! derived from the binary and are not in the repository. The report gives, per vector file,
//! the cases, how many the port reproduces bit for bit, how many differ, and how many were
//! not compared (the original's result depends on memory nobody wrote, or it did not
//! return).
use si_x87::vectors::{check_dir_with, print_report, Check};
use std::path::Path;

mod checks;
mod record;

fn lookup(name: &str) -> Option<Check> {
    checks::lookup(name)
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: fcurve-emucheck <directory of .vec files>");
        std::process::exit(2);
    };
    match check_dir_with(Path::new(&dir), &lookup, true) {
        Ok(rows) => {
            let ok = print_report(&rows);
            let n = checks::UNDEFINED_BUT_STABLE.load(std::sync::atomic::Ordering::Relaxed);
            println!("skipped because the port calls the input undefined although the original's result did not change with the memory content: {n}");
            let n = checks::STATE_BEFORE_UNDEFINED.load(std::sync::atomic::Ordering::Relaxed);
            println!("inactive-cycle cases (undefined result) whose two stores before the undefined read agree with the port: {n}");
            let n = checks::NEGATIVE_COUNTS.load(std::sync::atomic::Ordering::Relaxed);
            println!("skipped because the original leaves a negative key count, which the port cannot hold: {n}");
            if !ok {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("fcurve-emucheck: {e}");
            std::process::exit(2);
        }
    }
}
