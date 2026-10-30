// SPDX-License-Identifier: MIT
//! The check binary of unit cusrel1 (finding 0041, 2026-10-29): `cusStatusCreateRelations` against the vectors the emulated original
//! made (`re/tools/cusrel1/gen.sh`). Own binary, so that the unit touches no other unit's checker.

use si_checkkit::Kit;
use si_custom::custom_rel::cus_status_create_relations;

fn main() {
    // the data tables come from the user's install (si-rt, unit rtdata1): say so before any case
    si_rt::check_or_exit("cusrel1-check");
    let ports: &[(&'static str, si_checkkit::Port)] = &[("cusStatusCreateRelations", |m, o, a| {
        cus_status_create_relations(m, o, a.v("status"), a.v("eff"), a.v("hdr"), a.v("model"), a.v("n") as i32, a.v("models")).into()
    })];
    Kit::new(ports).setup(setup).normalise_stack_args().run()
}

/// The image the emulated original starts from: the data tables, and the C library's three standard streams as the harness numbers them.
fn setup(m: &mut si_mem::Mem) {
    si_custom::store_data(m);
    m.set_u32(si_custom::STDIN, 0x6f00_0000);
    m.set_u32(si_custom::STDOUT, 0x6f00_0100);
    m.set_u32(si_custom::STDERR, 0x6f00_0200);
}
