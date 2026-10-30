// SPDX-License-Identifier: MIT
//! `globals.c`'s hardware inquiry on the image: [soft 0x080d378c SI_checkHardware], the
//! routine of `si_runCreative` that asks the operating system and the graphics layer what
//! the display is and records the answers in the screen globals the renderer's set-up and
//! the picture code read (`SI_xmaxscreen`, `SI_ymaxscreen`, `SI_bitPlanes`, ...).
//!
//! The binary's symbol ends the function after its first 122 bytes; the rest of the body
//! (from 0x080d3877: the store of `SI_operSys` and everything after it) is the same
//! routine, entered by a jump (finding 0019, section 12).

use super::calls::BatchCalls;
use si_mem::{outside_calls, CStr32, Mem, Outside};

si_mem::globals! {
    /// `SI_operSys`: the kernel release as a number (`strtod` of `uname`'s release, kept as a
    /// float); 0 when `uname` failed.
    SI_OPER_SYS: f32 = 0x086793c4, "SI_operSys";
    SI_BIT_PLANES: i32 = 0x08781350, "SI_bitPlanes";
    SI_OLBIT_PLANES: i32 = 0x087815b4, "SI_olbitPlanes";
    SI_SCREEN_RES: i16 = 0x087817ae, "SI_screenRes";
    SI_XMAXSCREEN: i16 = 0x08781794, "SI_xmaxscreen";
    SI_YMAXSCREEN: i16 = 0x08781796, "SI_ymaxscreen";
    SI_IRISF: i16 = 0x08781790, "SI_irisf";
    SI_APPF: i16 = 0x08781644, "SI_appf";
    SI_LOGOF: i16 = 0x08781792, "SI_logof";
    SI_TWO_SIDE: i16 = 0x087817c0, "SI_twoSide";
    SI_ZBUFFER: i32 = 0x087817ec, "SI_zbuffer";
    SI_ZBUFFER_MAX: i32 = 0x087817f0, "SI_zbufferMax";
}

outside_calls! {
    /// The callees of `SI_checkHardware`.
    pub trait HardwareCalls {
        /// `uname(&buf)`: fills a `struct utsname` (six names of 65 bytes; the release is the
        /// third); -1 on failure.
        fn uname(buf: u32) -> i32 = "uname", 0x080c4578;
        fn errno_location() -> u32 = "__errno_location", 0x080c3e38;
        fn strerror(code: i32) -> CStr32 = "strerror", 0x080c3d28;
        /// The graphics layer's description of the display (`graphics.c`).
        fn getgdesc(inquiry: i32) -> i32 = "getgdesc", 0x086107a4;
        /// `si_IsIGLOO` (0x080d3738): whether the graphics layer is the IGLOO.
        fn si_is_igloo() -> i16 = "si_IsIGLOO", 0x080d3738;
    }
}

/// `SI_sboxfi`: the pointer the filled screen box is drawn through.
const SI_SBOXFI: u32 = 0x086e_e2a4;
/// `okSboxfi` and `sboxfi`: what the pointer is set to without and with the IGLOO.
const OK_SBOXFI: u32 = 0x080d_3768;
const SBOXFI: u32 = 0x0861_236c;
/// `SI_SCREENRES`, the variable that names the display class.
const SCREEN_RES_ENV: u32 = 0x0867_cde3;
const UNAME_ERROR: &[u8] = b"uname call error (";

/// [soft 0x080d378c SI_checkHardware]: the kernel number, the display's depth and size,
/// then the screen class. `SI_SCREENRES` (exactly `SI_1920X1035`, `SI_1920X1080`,
/// `SI_1920X1200` or `SI_1600X1024`) picks 3 to 6 when the display's height is one of
/// those and its width fits; any other display is class 0 (1280 by 1024) except a height
/// of 768 (class 1, 1024 by 768) and 1200 (class 2, 1600 by 1200). Answers 1.
pub fn check_hardware(m: &mut Mem, o: &mut dyn Outside) -> i16 {
    let mut choice: i32 = -1;
    let buf = m.local(0x186);
    if o.uname(m, buf) >= 0 {
        // the release string: strtod reads its longest decimal prefix; kept as a float
        let rel = m.cstr(buf + 130);
        m.set_f32(SI_OPER_SYS.addr(), crate::render_cmd::atof(&rel) as f32);
    } else {
        let p = o.errno_location(m);
        let code = m.i32(p);
        let text = o.strerror(m, code);
        let mut line = UNAME_ERROR.to_vec();
        line.extend_from_slice(&m.cstr(text.0));
        line.extend_from_slice(b")\n");
        o.print(2, &line);
        m.set_f32(SI_OPER_SYS.addr(), 0.0);
    }
    m.release(0x186);
    m[SI_BIT_PLANES] = o.getgdesc(m, 0xc);
    let width = o.getgdesc(m, 0);
    let height = o.getgdesc(m, 1);
    let env = o.getenv(m, CStr32::new(SCREEN_RES_ENV));
    if !env.is_null() {
        let name = m.cstr(env.0);
        if !name.is_empty() {
            for (k, n) in [b"SI_1920X1035".as_slice(), b"SI_1920X1080", b"SI_1920X1200", b"SI_1600X1024"].iter().enumerate() {
                if name.as_slice() == *n {
                    choice = 3 + k as i32;
                    break;
                }
            }
        }
    }
    // (screenRes, xmax, ymax, irisf); irisf is left at 12 for the 1024 by 768 display
    let set = |m: &mut Mem, res: i16, x: i16, y: i16, iris: i16| {
        m[SI_SCREEN_RES] = res;
        m[SI_XMAXSCREEN] = x;
        m[SI_YMAXSCREEN] = y;
        m[SI_IRISF] = iris;
    };
    let mut done = false;
    if height == 0x300 {
        set(m, 1, 0x3ff, 0x2ff, 0xc);
        done = true;
    } else if height == 0x4b0 {
        if choice == 5 && width == 0x780 {
            set(m, 5, 0x77f, 0x4af, 1);
        } else {
            set(m, 2, 0x63f, 0x4af, 1);
        }
        done = true;
    } else if (height == 0x40b || height == 0x438) && choice != -1 && width == 0x780 {
        if choice == 4 && height == 0x438 {
            set(m, 4, 0x77f, 0x437, 1);
            done = true;
        } else if choice == 3 && height == 0x40b {
            set(m, 3, 0x77f, 0x40a, 1);
            done = true;
        }
    }
    if !done {
        if choice == 6 && width == 0x640 {
            m[SI_SCREEN_RES] = 6;
            m[SI_XMAXSCREEN] = 0x63f;
        } else {
            m[SI_SCREEN_RES] = 0;
            m[SI_XMAXSCREEN] = 0x4ff;
        }
        m[SI_YMAXSCREEN] = 0x3ff;
        m[SI_IRISF] = 1;
    }
    m[SI_APPF] = 0xc;
    m[SI_LOGOF] = 2;
    m[SI_TWO_SIDE] = o.getgdesc(m, 0x4c) as i16;
    m[SI_OLBIT_PLANES] = o.getgdesc(m, 0x11);
    if m[SI_OLBIT_PLANES] == 4 {
        m[SI_OLBIT_PLANES] = 2;
    }
    m[SI_ZBUFFER] = o.getgdesc(m, 0x10);
    m[SI_ZBUFFER_MAX] = o.getgdesc(m, 5);
    let igloo = o.si_is_igloo(m);
    m.set_u32(SI_SBOXFI, if igloo != 0 { SBOXFI } else { OK_SBOXFI });
    1
}
