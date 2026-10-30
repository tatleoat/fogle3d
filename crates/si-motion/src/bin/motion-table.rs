// SPDX-License-Identifier: MIT
//! Prints the parameter table of `si-motion` as tab-separated text (one line per curve
//! type), for the corpus check `re/tools/si-motion/corpus.py` and for finding 0017.

fn main() {
    print!("{}", si_motion::table::tsv());
}
