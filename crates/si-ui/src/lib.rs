// SPDX-License-Identifier: MIT
//! The bottom of Softimage|3D 4.0's own user-interface toolkit (finding 0021): the windows of
//! the views (which the code calls grafports) and the event handling, the layer between the
//! IRIS GL calls (`si-gl`) and every dialog, menu and view of the program.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Original source files, by address because the IRIS GL layer has
//! files of the same names: `event.c` at 0x083939a0, `window.c` at 0x083d1d90, and the four
//! small ones `drawWindow.c` (0x08392400), `graf.c` (0x08399f20), `init_graphic.c`
//! (0x0839be00) and `ui_misc.c` (0x083d17c0). All 221 functions of the six are ported.
//!
//! How the crate is built:
//!
//! - [`mem`]: the program's writable memory as a 32-bit image (`.data`, `.bss`, a heap), and
//!   typed pointers into it. The toolkit is pointer code through and through: an event names
//!   the group, dialog or action string it is about by address, a view's window points to its
//!   camera and rulers, the screen layout is an array the template points to. The records
//!   keep those addresses.
//! - [`records`]: the records this package owns or reads, `#[repr(C)]` with the original's
//!   32-bit layout and every offset asserted, placed in the image.
//! - [`globals`]: the program's global variables these files use, by address, with the
//!   initial value of each one that has one; [`data`]: the constant tables and strings of
//!   `.data` that the routines reach through pointers.
//! - [`sys`]: everything the six files call outside themselves, one trait per subsystem
//!   (IRIS GL, the C library, controls, dialogs, menus, …), with recording implementations
//!   over one shared log ([`rec`]); [`gl_adapter`] puts the IRIS GL trait on `si-gl`.
//! - [`file`]: the C library's streams as far as the layout file's reader needs them.
//! - [`event`], [`window`], [`draw_window`], [`graf`], [`init_graphic`], [`ui_misc`]: the
//!   ports, as methods of [`Ui`], one module per source file.
pub mod data;
pub mod draw_window;
pub mod event;
pub mod file;
pub mod gl_adapter;
pub mod globals;
pub mod graf;
pub mod init_graphic;
pub mod mem;
pub mod rec;
pub mod records;
pub mod scroll;
pub mod scroll1;
pub mod ctl_colour;
pub mod ruler;
pub mod scrollbar;
pub mod slider;
pub mod sys;
pub mod ui_misc;
pub mod window;

/// `fault!(right, wrong)` is `right`. Built with the feature `deliberate-fault` it is
/// `wrong`: `re/tools/si-ui/fault_check.sh` builds the crate that way and counts the cases
/// that then differ from the original, to show that the check sees a fault in a ported
/// routine (finding 0021, "Checks").
macro_rules! fault {
    ($right:expr, $wrong:expr) => {
        if cfg!(feature = "deliberate-fault") {
            $wrong
        } else {
            $right
        }
    };
}
pub(crate) use fault;

use mem::Mem;
use sys::Outside;

/// The toolkit: the memory image and everything outside these files that they call.
pub struct Ui {
    pub m: Mem,
    pub o: Outside,
    /// The C library's streams ([`file`]).
    pub files: file::Files,
}

impl Ui {
    /// The image with every global of [`globals`] at its initial value, over the given
    /// outside world.
    pub fn new(o: Outside) -> Ui {
        let mut m = crate::mem::image();
        globals::init(&mut m);
        // what the C library's start-up leaves in the program's copy of `stderr`: the third
        // stream (the handles' numbering is `si-mem`'s)
        m[globals::STDERR] = 0x6f00_0200;
        Ui { m, o, files: file::Files::new() }
    }
}
