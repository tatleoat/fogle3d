// SPDX-License-Identifier: MIT
//! The ports of unit shdb4 (finding 0041, 2026-10-29), chained from `shdb3::with`.

use si_checkkit::Port;
use si_custom::shdb_preview::*;

/// A double argument (two stack words) of the case.
fn f64arg(a: &si_checkkit::Args, name: &str) -> f64 {
    use si_x87::vectors::Word;
    let f = a.case().try_input(&format!("v_{name}")).unwrap_or_else(|| panic!("no argument {name}"));
    match f.words().first() {
        Some(Word::Value { value, .. }) => f64::from_bits(*value as u64),
        _ => panic!("argument {name} has no value"),
    }
}

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("si_lookup_preview_texture", |m, _o, a| si_lookup_preview_texture(m, a.v("tex"), f64arg(a, "u"), f64arg(a, "v"), a.v("out")).into()),
    ]);
    crate::shdb5::with(&v)
}
