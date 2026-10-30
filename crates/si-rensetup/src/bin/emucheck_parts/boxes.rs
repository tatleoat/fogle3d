// SPDX-License-Identifier: MIT
//! The checks of `si_rensetup::subregion` against the emulated original.

use si_rensetup::subregion::{self, Irect, RegionDraw};
use si_x87::vectors::{Check, Field, Verdict};

fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

fn rect(f: &Field) -> Irect {
    let w = f.u16s();
    Irect { x0: w[0] as i16, x1: w[1] as i16, y0: w[2] as i16, y1: w[3] as i16 }
}

fn words(r: Irect) -> [u16; 4] {
    [r.x0 as u16 ^ mutate() as u16, r.x1 as u16, r.y0 as u16, r.y1 as u16]
}

struct Seen(Vec<u32>);

impl RegionDraw for Seen {
    fn draw_region(&mut self, r: Irect, on: i32) {
        self.0.extend([r.x0 as u16 as u32, r.x1 as u16 as u32, r.y0 as u16 as u32, r.y1 as u16 as u32, on as u32]);
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "clipBox" => |c| {
            let mut r = rect(c.input("r"));
            subregion::clip_box(&mut r, rect(c.input("b")));
            Verdict::from(c.after("r").u16s() == words(r), || format!("port {r:?}"))
        },
        "clippedBox" => |c| {
            let got = subregion::clipped_box(rect(c.input("a")), rect(c.input("b"))) != mutate();
            Verdict::from(c.output("eax").u32() & 0xffff == got as u32, || format!("port {got}"))
        },
        "correctBox" => |c| {
            let mut r = rect(c.input("r"));
            let mut seen = Seen(Vec::new());
            subregion::correct_box(&mut r, &mut seen);
            Verdict::from(c.after("r").u16s() == words(r) && c.output("drawn").u32s() == seen.0, || format!("port {r:?}, drawn {:?}", seen.0))
        },
        "regionMake" => |c| {
            let fr = c.input("fr").f32s();
            let mut resized = c.input("resized").u16s()[0] as i16;
            let region = subregion::region_make(&rect(c.input("a")), &rect(c.input("b")), [fr[0], fr[1], fr[2], fr[3]], &mut resized);
            let mut got = words(region.rect).to_vec();
            got.extend(words(region.bounds));
            got[4] ^= mutate() as u16;
            Verdict::from(c.output("heap0").u16s() == got && c.after("resized").u16s()[0] == resized as u16, || format!("port {region:?}, resized {resized}"))
        },
        _ => return None,
    })
}
