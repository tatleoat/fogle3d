// SPDX-License-Identifier: MIT
//! The zip of two patches (`zip.c`), on the one memory image: the limits of the rows and
//! points the zip may move, [soft 0x0819fe78 initMaxValue].
//!
//! Checked against the original in the emulator (re/tools/si-spline/emu_gen10.py, vec10).

use si_mem::{Mem, Ptr32};
use si_types::geom::Patch;

use crate::patch_image::idiv;

si_mem::globals! {
    /// `zip.c`: the point of the second patch the zip starts from, row-major (`nb_v` to a row).
    INDICE_ZIP: i32 = 0x087889fc, "indiceZip";
    /// `zip.c`: the same for the first patch.
    INDICE_SUP: i32 = 0x087889f8, "indiceSup";
    /// `zip.c`: non-zero when the second patch's U and V swap roles.
    ZIP_U_ZIP: i16 = 0x08788a02, "zipUZip";
    /// `zip.c`: the same for the first patch.
    ZIP_U_SUP: i16 = 0x08788a00, "zipUSup";
    /// `zip.c`: the smallest move down the rows.
    MAX_RW_DW: i32 = 0x087889e0, "maxRwDw";
    /// `zip.c`: the smallest move up the rows.
    MAX_RW_UP: i32 = 0x087889e4, "maxRwUp";
    /// `zip.c`: the smallest move down the points.
    MAX_PT_DW: i32 = 0x087889d8, "maxPtDw";
    /// `zip.c`: the smallest move up the points.
    MAX_PT_UP: i32 = 0x087889dc, "maxPtUp";
}

/// `initMaxValue(first, second)` [soft 0x0819fe78 initMaxValue]: from the two patches'
/// sizes and closed flags and the two start indices, how far the zip may move each way
/// (`maxRwDw`, `maxRwUp`, `maxPtDw`, `maxPtUp`). All integer; the sizes are unsigned
/// 16-bit values widened, the sums and differences wrap as 32-bit. A patch with a V size of
/// 0 divides by zero in the original (a processor fault): here a recorded fault and the
/// globals are left alone.
pub fn init_max_value(m: &mut Mem, sup: Ptr32<Patch>, zip: Ptr32<Patch>) {
    let z = m.read(zip);
    let s = m.read(sup);
    let (nu, nv) = (z.nb_u as i32, z.nb_v as i32);
    let ind = m[INDICE_ZIP];
    let (mut rw_dw, r) = idiv(m, ind, nv);
    if nv == 0 {
        return;
    }
    let (l_c, l_14, l_10);
    if m[ZIP_U_ZIP] == 0 {
        if z.u_closed == 0 {
            l_c = nu.wrapping_sub(rw_dw).wrapping_sub(1);
        } else {
            rw_dw = nu.wrapping_sub(1);
            l_c = rw_dw;
        }
        if z.v_closed == 0 {
            l_14 = nv.wrapping_sub(r).wrapping_sub(1);
            l_10 = r;
        } else {
            l_14 = nv.wrapping_sub(1);
            l_10 = l_14;
        }
    } else {
        if z.u_closed == 0 {
            l_14 = nu.wrapping_sub(rw_dw).wrapping_sub(1);
            l_10 = rw_dw;
        } else {
            l_14 = nu.wrapping_sub(1);
            l_10 = l_14;
        }
        if z.v_closed == 0 {
            l_c = nv.wrapping_sub(r).wrapping_sub(1);
            rw_dw = r;
        } else {
            l_c = nv.wrapping_sub(1);
            rw_dw = l_c;
        }
    }
    let (c, c14, c10) = (l_c, l_14, l_10);
    let snv = s.nb_v as i32;
    let (q, rr) = idiv(m, m[INDICE_SUP], snv);
    if snv == 0 {
        return;
    }
    let snu = s.nb_u as i32;
    let mut a = rw_dw; // iVar2
    let mut b = c10; // iVar5
    let mut d = c; // iVar6
    let mut e = c14; // local_8
    if m[ZIP_U_SUP] == 0 {
        if s.u_closed == 0 {
            a = q;
            d = snu.wrapping_sub(q).wrapping_sub(1);
        }
        if s.v_closed == 0 {
            e = snv.wrapping_sub(rr).wrapping_sub(1);
            b = rr;
        }
    } else {
        if s.u_closed == 0 {
            e = snu.wrapping_sub(q).wrapping_sub(1);
            b = q;
        }
        if s.v_closed == 0 {
            a = rr;
            d = snv.wrapping_sub(rr).wrapping_sub(1);
        }
    }
    m[MAX_RW_DW] = rw_dw.min(a);
    m[MAX_RW_UP] = c.min(d);
    m[MAX_PT_DW] = c10.min(b);
    m[MAX_PT_UP] = c14.min(e);
}
