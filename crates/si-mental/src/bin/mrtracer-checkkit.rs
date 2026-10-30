// SPDX-License-Identifier: MIT
//! `mrtracer-checkkit <directory of vector files>`: `SI_mentalRunTracer` (`si_mental::mr_tracer`) on `si-checkkit` against the emulated original
//! (finding 0042, round 11). Vectors: `re/tools/si-mental/gen_mrtracer.py` (the process, thread, pipe, GL and C library routines are recorded stand-ins).
use si_checkkit::Kit;
use si_mental::mr_tracer::si_mental_run_tracer;

fn main() {
    Kit::new(&[("0x0842a8a0", |m, o, a| si_mental_run_tracer(m, o, a.v("p"), a.i("mode") as u32).into())])
        .normalise_stack_args()
        .run()
}
