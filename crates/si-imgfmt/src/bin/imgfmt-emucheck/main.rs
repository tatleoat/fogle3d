// SPDX-License-Identifier: MIT
//! Compares the Rust port of the picture library with vectors from the emulation harness
//! (`re/tools/emu`): original routines run in a CPU emulator on made-up pictures and files
//! by `re/tools/si-imgfmt/emu_gen.py` (picture files, icons, foreign formats, effects, tiling)
//! `emu_gen2.py` (resizing, compositing, dithering, quantisation) and `emu_gen3.py` (palettes,
//! index pictures, colour reduction end to end, the neural quantiser, more formats).
//!
//!   imgfmt-emucheck <vector directory> [--partly-undefined]
//!
//! With `--partly-undefined` the cases the harness marks `UNINIT` are compared too, on the
//! words that have a value (the fourth pass's vectors, `emu_gen4.py`, are run so). The cases
//! in which a port refuses (a fault: what the original does there is not defined) are counted
//! by reason after the table.
//!
//! With `EMUCHECK_MUTATE=1` one bit of what the port produced is flipped before the byte,
//! plane, float-row, box and list comparisons: a self-test that those comparisons are live.

mod compose;
mod composite;
mod dialog;
mod dgt;
mod everest;
mod formats;
mod formats3;
mod glue;
mod gradation;
mod ops;
mod pict;
mod picture;
mod quant;
mod reduce;
mod sgiimage;
mod size;
mod texture;
mod tiff;
mod util;
mod zoom;

use si_x87::vectors::{check_dir_with, print_report, Check};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

thread_local! {
    /// Faults of the ports (cases not compared), by reason.
    pub static FAULTS: RefCell<BTreeMap<String, usize>> = RefCell::new(BTreeMap::new());
    /// Cases the harness marks `UNINIT` that agree on every defined word, by routine (the
    /// report counts them as skipped).
    pub static PARTLY: RefCell<BTreeMap<String, usize>> = RefCell::new(BTreeMap::new());
}

fn lookup(name: &str) -> Option<Check> {
    picture::lookup(name).or_else(|| formats::lookup(name)).or_else(|| ops::lookup(name)).or_else(|| size::lookup(name)).or_else(|| zoom::lookup(name)).or_else(|| composite::lookup(name)).or_else(|| gradation::lookup(name)).or_else(|| quant::lookup(name)).or_else(|| dgt::lookup(name)).or_else(|| reduce::lookup(name)).or_else(|| formats3::lookup(name)).or_else(|| texture::lookup(name)).or_else(|| tiff::lookup(name)).or_else(|| pict::lookup(name)).or_else(|| everest::lookup(name)).or_else(|| dialog::lookup(name)).or_else(|| sgiimage::lookup(name)).or_else(|| glue::lookup(name)).or_else(|| compose::lookup(name))
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: imgfmt-emucheck <vector directory> [--partly-undefined]");
    let partly = std::env::args().nth(2).is_some_and(|a| a == "--partly-undefined");
    let rows = check_dir_with(Path::new(&dir), &lookup, partly).expect("reading the vectors");
    let ok = print_report(&rows);
    tiff::never_ends(Path::new(&dir));
    PARTLY.with(|m| {
        for (name, n) in m.borrow().iter() {
            println!("  {name}: {n} of the skipped agree on every word the original defines");
        }
    });
    FAULTS.with(|m| {
        for (why, n) in m.borrow().iter() {
            println!("  port refused {n}: {why}");
        }
    });
    std::process::exit(if ok { 0 } else { 1 });
}
