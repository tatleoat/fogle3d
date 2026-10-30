// SPDX-License-Identifier: MIT
// adapter: not in place
//! The crate's struct form (`App`, its `Globals` record and its `World` of traits) running
//! the image ports: a method of `App` whose routine is on the image makes a scratch image,
//! writes the globals the routine reads, runs the port and copies back what it wrote. The
//! calls out go to `App`'s world under the names it already records, so that the checks of
//! the struct form (`app-emucheck`) measure the image ports too. This goes when `App`'s
//! globals are on the image (finding 0024, section 5: `si-app` is fifth in the order).

use crate::env::World;
use si_mem::{Mem, Outside, Ret, Routine};

/// `App`'s world as an outside world of the image ports.
pub(crate) struct AppOutside<'a, W: World>(pub &'a mut W);

impl<W: World> Outside for AppOutside<'_, W> {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Ret {
        match r.name {
            "getenv" => {
                let name = m.cstr(a[0]);
                match self.0.getenv(&name) {
                    Some(v) => Ret::eax(m.place_cstr(&v)),
                    None => Ret::NONE,
                }
            }
            "time" => {
                let t = self.0.time();
                if a[0] != 0 {
                    m.set_i32(a[0], t);
                }
                Ret::int(t)
            }
            "localtime" => {
                let t = m.i32(a[0]);
                let year = self.0.local_year(t);
                let tm = m.place(0x2c, 4);
                m.set_i32(tm + 0x14, year);
                Ret::eax(tm)
            }
            "realpath" => {
                let path = m.cstr(a[0]);
                match self.0.realpath(&path) {
                    Some(v) => {
                        m.put_cstr(a[1], &v);
                        Ret::eax(a[1])
                    }
                    None => Ret::NONE,
                }
            }
            "getcwd" => match self.0.getcwd(a[1] as usize) {
                Some(dir) => Ret::eax(super::batch::libc_getcwd(m, a[0], a[1], &dir)),
                None => Ret::NONE,
            },
            "SI_cusCallCustomFunctions" => {
                self.0.SI_cusCallCustomFunctions(a[0] as i32, a[1] as i32);
                Ret::NONE
            }
            "SI_sequenceWireFrame" => {
                self.0.SI_sequenceWireFrame();
                Ret::NONE
            }
            "SI_sequenceHiddenLine" => {
                self.0.SI_sequenceHiddenLine(a[0] as i32);
                Ret::NONE
            }
            "SI_sequenceRender" => {
                self.0.SI_sequenceRender();
                Ret::NONE
            }
            "SI_sequenceRenderMental" => {
                self.0.SI_sequenceRenderMental(a[0] as i32);
                Ret::NONE
            }
            // nothing else is reached from the routines run this way
            _ => Ret::NONE,
        }
    }

    fn print(&mut self, stream: u32, text: &[u8]) {
        if stream == 1 {
            self.0.out(text)
        } else {
            self.0.err(text)
        }
    }
}

/// A scratch image with this module's `.data` values.
pub(crate) fn scratch() -> Mem {
    let mut m = Mem::new();
    super::init_data(&mut m);
    m
}

/// A string on a scratch image, or null.
pub(crate) fn place(m: &mut Mem, s: Option<&[u8]>) -> si_mem::CStr32 {
    match s {
        Some(s) => si_mem::CStr32::new(m.place_cstr(s)),
        None => si_mem::CStr32::NULL,
    }
}
