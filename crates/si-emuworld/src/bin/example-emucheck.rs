// SPDX-License-Identifier: MIT
//! The worked example of finding 0011, "Writing a check for a routine": the port of
//! `si_graflistFindParent` (`si-view`) held against the vectors of
//! `re/tools/emu/example/gen_findparent.py`.
//!
//!   example-emucheck <vector file>          (EMUCHECK_MUTATE=1: every case must differ)
use si_emuworld::case::parse;
use si_emuworld::check::{place, word};
use si_mem::vecfmt::Word;
use si_mem::{Mem, Ptr32};

fn main() {
    let path = std::env::args().nth(1).expect("usage: example-emucheck <vector file>");
    let mutate = std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "1");
    let (mut cases, mut same, mut differ, mut skipped) = (0, 0, 0, 0);
    for line in std::fs::read_to_string(&path).expect("vector file").lines() {
        let Some(c) = parse(line).expect("vector line") else { continue };
        cases += 1;
        if c.stopped() || c.has_note("UNINIT") {
            skipped += 1;
            continue;
        }
        let mut m = Mem::new(); // a fresh image a case: the case's buffers where the harness put them
        let names = place(&mut m, &c).expect("buffers");
        let node = word(&m, &names, c.args()[0].1);
        let got = si_view::graflist::graflist_find_parent(&m, Ptr32::new(node)).addr() ^ mutate as u32;
        let want = match c.output("ptr").and_then(|w| w.first()) {
            Some(Word::Ref { name, offset }) => names.addr(&m, name).unwrap_or(0) + offset,
            Some(Word::Value { value, .. }) => *value as u32,
            _ => u32::MAX,
        };
        if got == want && m.fault().is_none() { same += 1 } else { differ += 1; eprintln!("differs: port {got:08x}: {line}") }
    }
    println!("si_graflistFindParent  {cases} cases  {same} identical  {differ} different  {skipped} not compared");
    std::process::exit((differ != 0) as i32);
}
