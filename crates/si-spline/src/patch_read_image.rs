// SPDX-License-Identifier: MIT
//! `patch.c`: the patch reader [soft 0x0847a838 patchRead] on the one memory image
//! (finding 0040, section 3).
//!
//! Read from code: the file is a brace block of words. After `{` every word is a keyword:
//! `utype` / `vtype` (then `LINEAR`, `BEZIER`, `BSPLINE` or `CARDINAL`), `upoint` /
//! `vpoint` (a count), `uclosed` / `vclosed` (no value; set the flag), `utens` / `vtens`
//! (a float), `ustep` / `vstep`, `ucurve` / `vcurve` (counts) and `point`, which allocates
//! the point block and the tag block and reads `upoint x vpoint` lines of a number to skip
//! and four floats; any other word is ignored. A `CARDINAL` type also sets the tension
//! (+0x14 or +0x18) to 0.5. The routine ends at the word `}`; when the first word is not
//! `{` it prints ` { expected` and ends at once. It always returns the patch, after
//! [`patch_update_matrix`].
//!
//! The word buffer is a 0x50-byte local, read by `fscanf("%s")` without a width: a longer
//! word overruns it, as in the original (not reproduced; the image has a stack of its own).
//! At the end of the input `fscanf` leaves the buffer as it was, so a file without `}`
//! never ends the loop: the original loops forever there and so does this port (a check
//! never makes such a file).
//!
//! Callees: `SI_patchAllocate` and `SI_patchUpdateMatrix` are ported; `SI_agetString`
//! (0x083fb990, reads the patch's name; called once with the file and the patch), `fscanf`
//! goes through [`Outside`], `printf` as the text it prints ([`Outside::print`], stream 1). The format strings of `fscanf` are at their `.data`
//! addresses ([`format_at`] tells a stand-in which is which).

use si_mem::{Mem, Outside, Ptr32, Routine};
use si_types::geom::Patch;

use crate::patch_eval_image::patch_update_matrix;
use crate::patch_image::patch_allocate;

/// `SI_agetString` (0x083fb990), not ported.
pub const SI_AGET_STRING: Routine = Routine::new("SI_agetString", 0x083f_b990);
/// The C library's `fscanf`, through the binary's stub.
pub const FSCANF: Routine = Routine::new("fscanf", 0x080c_4058);

/// One literal of the reader and its address in `.data`.
#[derive(Clone, Copy)]
struct Lit(u32, &'static [u8]);

const S_BUF1: Lit = Lit(0x086d_6660, b"%s");
const S_OPEN: Lit = Lit(0x086d_6663, b"{");
const S_EXPECTED: Lit = Lit(0x086d_6665, b" { expected\n");
const S_BUF2: Lit = Lit(0x086d_6672, b"%s");
const S_UTYPE: Lit = Lit(0x086d_6675, b"utype");
const S_BUF3: Lit = Lit(0x086d_667b, b"%s");
const S_LINEAR_U: Lit = Lit(0x086d_667e, b"LINEAR");
const S_BSPLINE_U: Lit = Lit(0x086d_6685, b"BSPLINE");
const S_BEZIER_U: Lit = Lit(0x086d_668d, b"BEZIER");
const S_CARDINAL_U: Lit = Lit(0x086d_6694, b"CARDINAL");
const S_UPOINT: Lit = Lit(0x086d_669d, b"upoint");
const S_HD_UPOINT: Lit = Lit(0x086d_66a4, b"%hd");
const S_UCLOSED: Lit = Lit(0x086d_66a8, b"uclosed");
const S_UTENS: Lit = Lit(0x086d_66b0, b"utens");
const S_F_UTENS: Lit = Lit(0x086d_66b6, b"%f");
const S_USTEP: Lit = Lit(0x086d_66b9, b"ustep");
const S_HD_USTEP: Lit = Lit(0x086d_66bf, b"%hd");
const S_UCURVE: Lit = Lit(0x086d_66c3, b"ucurve");
const S_HD_UCURVE: Lit = Lit(0x086d_66ca, b"%hd");
const S_VTYPE: Lit = Lit(0x086d_66ce, b"vtype");
const S_BUF4: Lit = Lit(0x086d_66d4, b"%s");
const S_LINEAR_V: Lit = Lit(0x086d_66d7, b"LINEAR");
const S_BEZIER_V: Lit = Lit(0x086d_66de, b"BEZIER");
const S_BSPLINE_V: Lit = Lit(0x086d_66e5, b"BSPLINE");
const S_CARDINAL_V: Lit = Lit(0x086d_66ed, b"CARDINAL");
const S_VPOINT: Lit = Lit(0x086d_66f6, b"vpoint");
const S_HD_VPOINT: Lit = Lit(0x086d_66fd, b"%hd");
const S_VCLOSED: Lit = Lit(0x086d_6701, b"vclosed");
const S_VTENS: Lit = Lit(0x086d_6709, b"vtens");
const S_F_VTENS: Lit = Lit(0x086d_670f, b"%f");
const S_VSTEP: Lit = Lit(0x086d_6712, b"vstep");
const S_HD_VSTEP: Lit = Lit(0x086d_6718, b"%hd");
const S_VCURVE: Lit = Lit(0x086d_671c, b"vcurve");
const S_HD_VCURVE: Lit = Lit(0x086d_6723, b"%hd");
const S_POINT: Lit = Lit(0x086d_6727, b"point");
const S_POINT_FMT: Lit = Lit(0x086d_672d, b"%*d %f %f %f %f");
const S_CLOSE: Lit = Lit(0x086d_673d, b"}");

const FORMATS: [Lit; 12] = [S_BUF1, S_BUF2, S_BUF3, S_BUF4, S_HD_UPOINT, S_F_UTENS, S_HD_USTEP, S_HD_UCURVE, S_HD_VPOINT, S_F_VTENS, S_HD_VSTEP, S_POINT_FMT];
const FORMATS_2: [Lit; 2] = [S_HD_VCURVE, S_HD_VCURVE];

/// The text of the `fscanf` format of this reader at `addr` in the binary's `.data`.
pub fn format_at(addr: u32) -> Option<&'static [u8]> {
    FORMATS.iter().chain(FORMATS_2.iter()).find(|l| l.0 == addr).map(|l| l.1)
}

/// 0.5, the tension a `CARDINAL` type sets (the constant at 0x086602e4).
const HALF: f32 = 0.5;

/// [soft 0x0847a838 patchRead]: reads one patch from the open file `file`.
pub fn patch_read(m: &mut Mem, o: &mut dyn Outside, file: u32) -> Ptr32<Patch> {
    let p = patch_allocate(m);
    let pa = p.addr();
    o.call(m, SI_AGET_STRING, &[file, pa]);
    let frame = m.local(0x50);
    let buf = frame;
    let scan = |m: &mut Mem, o: &mut dyn Outside, f: Lit, ptrs: &[u32]| {
        let mut a = vec![file, f.0];
        a.extend_from_slice(ptrs);
        o.call(m, FSCANF, &a);
    };
    let is = |m: &Mem, lit: Lit| m.cstr(buf) == lit.1;
    scan(m, o, S_BUF1, &[buf]);
    if !is(m, S_OPEN) {
        o.print(1, S_EXPECTED.1);
    } else {
        loop {
            scan(m, o, S_BUF2, &[buf]);
            if is(m, S_UTYPE) {
                scan(m, o, S_BUF3, &[buf]);
                if is(m, S_LINEAR_U) {
                    m.set_i32(pa + 4, 0);
                }
                if is(m, S_BSPLINE_U) {
                    m.set_i32(pa + 4, 2);
                }
                if is(m, S_BEZIER_U) {
                    m.set_i32(pa + 4, 1);
                }
                if is(m, S_CARDINAL_U) {
                    m.set_f32(pa + 0x14, HALF);
                    m.set_i32(pa + 4, 3);
                }
            }
            if is(m, S_UPOINT) {
                scan(m, o, S_HD_UPOINT, &[pa + 0xc]);
            }
            if is(m, S_UCLOSED) {
                m.set_i16(pa + 0x10, 1);
            }
            if is(m, S_UTENS) {
                scan(m, o, S_F_UTENS, &[pa + 0x14]);
            }
            if is(m, S_USTEP) {
                scan(m, o, S_HD_USTEP, &[pa + 0x1c]);
            }
            if is(m, S_UCURVE) {
                scan(m, o, S_HD_UCURVE, &[pa + 0x20]);
            }
            if is(m, S_VTYPE) {
                scan(m, o, S_BUF4, &[buf]);
                if is(m, S_LINEAR_V) {
                    m.set_i32(pa + 8, 0);
                }
                if is(m, S_BEZIER_V) {
                    m.set_i32(pa + 8, 1);
                }
                if is(m, S_BSPLINE_V) {
                    m.set_i32(pa + 8, 2);
                }
                if is(m, S_CARDINAL_V) {
                    m.set_f32(pa + 0x18, HALF);
                    m.set_i32(pa + 8, 3);
                }
            }
            if is(m, S_VPOINT) {
                scan(m, o, S_HD_VPOINT, &[pa + 0xe]);
            }
            if is(m, S_VCLOSED) {
                m.set_i16(pa + 0x12, 1);
            }
            if is(m, S_VTENS) {
                scan(m, o, S_F_VTENS, &[pa + 0x18]);
            }
            if is(m, S_VSTEP) {
                scan(m, o, S_HD_VSTEP, &[pa + 0x1e]);
            }
            if is(m, S_VCURVE) {
                scan(m, o, S_HD_VCURVE, &[pa + 0x22]);
            }
            if is(m, S_POINT) {
                let count = (m.u16(pa + 0xc) as u32).wrapping_mul(m.u16(pa + 0xe) as u32);
                let pts = m.si_calloc(count, 16);
                m.set_u32(pa + 0x1a4, pts);
                let count = (m.u16(pa + 0xc) as u32).wrapping_mul(m.u16(pa + 0xe) as u32);
                let tags = m.si_calloc(count, 2);
                m.set_u32(pa + 0x1a8, tags);
                let (mut at, mut tag) = (pts, tags);
                for _ in 0..m.u16(pa + 0xc) {
                    // the counts are re-read from the patch on every pass, as the original does
                    let mut j = 0;
                    while j < m.u16(pa + 0xe) {
                        scan(m, o, S_POINT_FMT, &[at, at + 4, at + 8, at + 12]);
                        m.set_u16(tag, 0);
                        j += 1;
                        at = at.wrapping_add(16);
                        tag = tag.wrapping_add(2);
                    }
                }
            }
            if is(m, S_CLOSE) {
                break;
            }
        }
    }
    patch_update_matrix(m, p);
    p
}
