// SPDX-License-Identifier: MIT
//! `spline-checkkit <directory of vector files>`: the checks of the routines `spline-emucheck`
//! does not carry (finding 0040), on `si-checkkit` (finding 0011, "A check binary with
//! si-checkkit"). Vectors come from `re/tools/si-spline/specs/*.spec` through `specgen.py`;
//! `re/tools/si-spline/run.sh SPEC` generates and checks one.

use si_checkkit::Kit;
use si_mem::{Mem, Outside, Ret, Routine};
use si_spline::bez_image as bez;
use si_spline::fit_contour_image as fc;
use si_spline::patch_draw_image as pd;
use si_spline::patch_read_image as pr;
use si_spline::spline_draw_image as sd;
use si_ui::file::Files;
use std::cell::RefCell;
use si_spline::spline_image;
use si_x87::vectors::{Case, Word};

/// A variadic callee (`fprintf`) is recorded with a fixed number of stack words, of which the
/// words past the real arguments are stale residue of the original's stack. The port does not
/// pass them: they are filled in from the recorded call, so only the real arguments are
/// compared (kit candidate: the kit could do this for a record rule with an arity).
struct Padded<'a> {
    inner: &'a mut dyn Outside,
    case: &'a Case,
    made: usize,
}

impl Outside for Padded<'_> {
    fn call(&mut self, m: &mut Mem, r: Routine, args: &[u32]) -> Ret {
        let mut full = args.to_vec();
        if let Some(want) = self.case.calls.get(self.made) {
            for w in want.args.iter().skip(args.len()) {
                full.push(match w {
                    Word::Value { value, .. } => *value as u32,
                    _ => 0,
                });
            }
        }
        self.made += 1;
        self.inner.call(m, r, &full)
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        self.inner.print(stream, text)
    }
}

/// The package's own `Outside` trait (callee name and address, words, `eax`) on the kit's.
struct Bridge<'a>(&'a mut dyn Outside);

impl spline_image::Outside for Bridge<'_> {
    fn call(&mut self, m: &mut Mem, callee: &spline_image::Callee, args: &[u32]) -> u32 {
        self.0.call(m, Routine::new(callee.name, callee.addr), args).eax
    }
}

thread_local! {
    /// The streams over the case's file: `fscanf` is the C library's, the original's stand-in
    /// of the harness (re/tools/emu/standins.py) is not recorded, so the port's is answered
    /// by `si-ui`'s `Files`, which behaves as that stand-in does (kit candidate: the kit could
    /// hold the case's `file` input and answer `fscanf` itself).
    static FILES: RefCell<Files> = RefCell::new(Files::new());
}

/// `fscanf(file, format, pointers…)` on the case's streams; the formats are the reader's.
fn fscanf(m: &mut Mem, args: &[u32]) -> Ret {
    let fmt = pr::format_at(args[1]).expect("a format of patchRead");
    let n = FILES.with(|f| f.borrow_mut().fscanf(m, args[0], fmt, &args[2..]));
    Ret::eax(n as u32)
}

/// The case's `file` input as an open stream; its handle.
fn open_file(a: &si_checkkit::Args) -> u32 {
    let bytes = a.case().try_input("file").map_or(Vec::new(), |f| f.to_bytes());
    FILES.with(|f| {
        let mut files = Files::new();
        files.add(b"file", bytes);
        let h = files.open_named(b"file");
        *f.borrow_mut() = files;
        h
    })
}

fn main() {
    Kit::new(&[
        ("si_bezPatchCreate", |m, _o, a| bez::create(m, a.i("s"), a.i("t"), a.v("bbox")).into()),
        ("SI_bezPatchDestroy", |m, _o, a| bez::destroy(m, a.v("pp")).into()),
        ("si_bezPatchDump", |m, o, a| bez::dump(m, &mut Padded { inner: o, case: a.case(), made: 0 }, a.v("rec"), a.v("file")).into()),
        ("si_bezPatchInit", |m, _o, a| bez::init(m, a.i("s"), a.i("t"), a.v("bbox"), a.v("rec")).into()),
        ("si_bezPatchCleanup", |m, _o, a| bez::cleanup(m, a.v("rec")).into()),
        ("patchRead", |m, o, a| pr::patch_read(m, o, open_file(a)).into()),
        ("SI_patchDraw", |m, o, a| pd::patch_draw(m, o, a.p("p")).into()),
        ("SI_patchHit", |m, o, a| pd::patch_hit(m, o, a.p("p")).into()),
        ("SI_splineDraw", |m, o, a| sd::spline_draw(m, o, a.p("s")).into()),
        ("SI_splineDrawModif", |m, o, a| sd::spline_draw_modif(m, o, a.p("s")).into()),
        ("SI_splineDrawStart", |m, o, a| sd::spline_draw_start(m, o, a.p("s")).into()),
        ("oldsplineHit", |m, o, a| sd::old_spline_hit(m, o, a.p("s")).into()),
        ("SI_patchDrawModif", |m, o, a| pd::patch_draw_modif(m, o, a.p("p")).into()),
        ("SI_patchHitAll", |m, o, a| pd::patch_hit_all(m, o, a.v("view"), a.p("p"), a.v("matrix"), a.v("out")).into()),
        ("SI_bezierFitContour", |m, o, a| fc::bezier_fit_contour(m, &mut Bridge(o), a.v("c"), a.v("flags")).into()),
    ])
    .stand_in("fscanf", fscanf)
    .setup(spline_image::init)
    .run()
}
