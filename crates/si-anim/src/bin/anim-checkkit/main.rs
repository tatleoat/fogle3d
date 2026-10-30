// SPDX-License-Identifier: MIT
//! `anim-checkkit <directory of vector files>`: the checks of `si-anim` routines on `si-checkkit`
//! (finding 0011, "A check binary with si-checkkit"); vectors from `re/tools/actor3/mkspecs4.py`
//! through `re/tools/actor3/run4.sh`.
//!
//! Nothing here reproduces a function of the original.
use si_anim::cluster as cl;
use si_anim::dictin;
use si_anim::outside::{Outside, Routine};
use si_checkkit::Kit;
use si_mem::Mem;
use si_x87::F80;

/// The crate's own callee trait over the check's one.
struct Bridge<'a>(&'a mut dyn si_mem::Outside);

impl Outside for Bridge<'_> {
    fn call(&mut self, m: &mut Mem, r: &'static Routine, args: &[u32]) -> u32 {
        self.0.call(m, si_mem::Routine::new(r.name, r.addr), args).eax
    }
    fn call_f(&mut self, m: &mut Mem, r: &'static Routine, args: &[u32]) -> F80 {
        self.0.call(m, si_mem::Routine::new(r.name, r.addr), args);
        F80::ZERO
    }
    fn print(&mut self, text: &[u8]) {
        self.0.print(1, text)
    }
}

fn main() {
    // the original's tables and messages the ported routines use are read from the user's install
    si_rt::check_or_exit("anim-checkkit");
    Kit::new(&[
        ("clusterAnimDump", |m, o, a| cl::cluster_anim_dump(m, &mut Bridge(o), a.p("k"), a.i("kind"), a.i("faces")).into()),
        ("elemdictCheckPasteFcv", |m, o, a| {
            dictin::elemdict_check_paste_fcv(m, &mut Bridge(o), a.v("dict"), a.v("rd"), a.v("elem"), a.i("p4"), a.i("p5"), a.v("fcv"), a.i("p7"), a.s("active"), a.i("root"), a.s("complete"), a.i("frames"));
            ().into()
        }),
    ])
    .run()
}
