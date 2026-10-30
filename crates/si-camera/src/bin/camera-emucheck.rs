// SPDX-License-Identifier: MIT
//! `camera-emucheck <vector dir>`: si-camera's value functions held against the vectors the
//! emulation harness wrote for the original routines (`re/tools/si-camera/dof.spec`;
//! finding 0011). `EMUCHECK_MUTATE=1` flips one bit of every result the port gives: every
//! compared case must then differ. The image forms are checked on sample scenes by
//! `camera-scenecheck` (`re/tools/si-camera/scenecheck`).
//!
//! Nothing here reproduces a function of the original.

use si_camera::dof;
use si_x87::vectors::{check_dir, print_report, Case, Check, Verdict};
use si_x87::F80;

/// `EMUCHECK_MUTATE=1`: one bit of every result flipped (the lowest of the significand).
fn mutated(v: F80) -> F80 {
    if std::env::var("EMUCHECK_MUTATE").is_ok_and(|m| m == "1") {
        let (m, se) = v.to_bits();
        F80::from_bits(m ^ 1, se)
    } else {
        v
    }
}

fn st0(c: &Case, got: F80) -> Verdict {
    let got = mutated(got);
    Verdict::from(c.output("st0").same_f80(got), || format!("port {got:?}"))
}

fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "findhyperfocal" => |c: &Case| st0(c, dof::find_hyperfocal(c.input("a").f32(), c.input("b").f32(), c.input("c").f32())),
        "findnear" => |c: &Case| st0(c, dof::find_near(c.input("a").f32(), c.input("b").f32(), c.input("c").f32())),
        "findfar" => |c: &Case| st0(c, dof::find_far(c.input("a").f32(), c.input("b").f32(), c.input("c").f32())),
        "findoutofocus" => |c: &Case| st0(c, dof::find_out_of_focus(c.input("a").f32(), c.input("b").f32(), c.input("c").f32(), c.input("d").f32())),
        "findcoc" | "findcoc#lens" => |c: &Case| st0(c, F80::from_f32(dof::find_coc(c.input("a").f32(), c.input("b").f32(), c.input("c").f32()))),
        _ => return None,
    })
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "re/out/si-camera/dofvec".into());
    match check_dir(std::path::Path::new(&dir), &lookup) {
        Ok(rows) => {
            let ok = print_report(&rows);
            std::process::exit(if ok { 0 } else { 1 });
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
