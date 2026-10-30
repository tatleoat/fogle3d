// SPDX-License-Identifier: MIT
//! `patch.c`: the patch's picking and drawing routines that only walk the two display grids
//! (`surface0` at +0x1b0 with `u16` columns at +0x1b4 and rows at +0x1b6, `surface1` at
//! +0x1b8 with +0x1bc and +0x1be; 12 bytes a point) on the one memory image (finding 0040,
//! section 4). Every graphics-library call goes through [`GlCalls`] (the checks record
//! them): the vertex call receives the address of the point in the grid, as the original
//! passes it.
//!
//! Read from code: [`patch_draw`] draws each row of each grid as one line strip;
//! [`patch_draw_modif`] does the same with a flag word (`u16`) per point at +0x1d8 /
//! +0x1dc, a strip being open only over the points whose flag is not zero (a point with
//! flag 0 ends the strip, a flagged one begins a new strip when none is open); [`patch_hit`]
//! is the first routine with `loadname` before each row, the name counting up over both
//! grids; [`patch_hit_all`] picks every segment on its own name (see its comment).

use si_mem::{outside_calls, Mem, Outside, Ptr32};
use si_types::geom::Patch;

outside_calls! {
    /// The graphics library and the view code, as the patch routines call them.
    pub trait GlCalls {
        /// `pushmatrix (0x0861052c)`, `graphics.c` (ported in `si-gl`).
        fn pushmatrix() = "pushmatrix", 0x0861052c;
        /// `popmatrix (0x0861053c)`, `graphics.c` (ported in `si-gl`).
        fn popmatrix() = "popmatrix", 0x0861053c;
        /// `multmatrix (0x08610514)`: the matrix at the address (16 floats).
        fn multmatrix(matrix: u32) = "multmatrix", 0x08610514;
        /// `bgnline (0x08612b58)`, `graphics.c` (ported in `si-gl`).
        fn bgnline() = "bgnline", 0x08612b58;
        /// `endline (0x08612b6c)`, `graphics.c` (ported in `si-gl`).
        fn endline() = "endline", 0x08612b6c;
        /// `v3f (0x08612c60)`, `graphics.c` (ported in `si-gl`): a vertex from the three floats at the address.
        fn v3f(xyz: u32) = "v3f", 0x08612c60;
        /// `loadname (0x08615080)`, `pick.c` (ported in `si-gl`): the pick name (a `short`).
        fn loadname(name: i32) = "loadname", 0x08615080;
        /// `initnames (0x086150c0)`, `pick.c` (ported in `si-gl`).
        fn initnames() = "initnames", 0x086150c0;
        /// `pick (0x08614ec8)`, `pick.c` (ported in `si-gl`): (the hit buffer, its size in shorts).
        fn pick(buffer: u32, size: i32) = "pick", 0x08614ec8;
        /// `endpick (0x08614ff0)`, `pick.c` (ported in `si-gl`): the number of hits.
        fn endpick(buffer: u32) -> i32 = "endpick", 0x08614ff0;
        /// `SI_setProjectionMatrix (0x08369d80)`, `view.c`, not ported: the view's projection.
        fn si_set_projection_matrix(view: u32) = "SI_setProjectionMatrix", 0x08369d80;
    }
}

const SURF0: u32 = 0x1b0;
const COLS0: u32 = 0x1b4;
const ROWS0: u32 = 0x1b6;
const SURF1: u32 = 0x1b8;
const COLS1: u32 = 0x1bc;
const ROWS1: u32 = 0x1be;
const MODIF0: u32 = 0x1d8;
const MODIF1: u32 = 0x1dc;

/// The two display grids of a patch: (points, columns offset, rows offset).
const GRIDS: [(u32, u32, u32); 2] = [(SURF0, COLS0, ROWS0), (SURF1, COLS1, ROWS1)];

/// [soft 0x0847d504 SI_patchDraw]: every row of both display grids as a line strip.
pub fn patch_draw(m: &mut Mem, o: &mut dyn Outside, patch: Ptr32<Patch>) {
    let p = patch.addr();
    for (pts, cols, rows) in GRIDS {
        let mut at = m.u32(p + pts);
        if m.u16(p + rows) != 0 {
            let mut i = 0u32;
            loop {
                o.bgnline(m);
                i += 1;
                if m.u16(p + cols) != 0 {
                    let mut j = 0u32;
                    loop {
                        o.v3f(m, at);
                        j += 1;
                        at = at.wrapping_add(12);
                        if j >= m.u16(p + cols) as u32 {
                            break;
                        }
                    }
                }
                o.endline(m);
                if i >= m.u16(p + rows) as u32 {
                    break;
                }
            }
        }
    }
}

/// [soft 0x0847d134 SI_patchHit]: [`patch_draw`] with `loadname` of a counter (a `short`
/// of an `int`, 0 for the first row of the first grid and on across both) before each row.
pub fn patch_hit(m: &mut Mem, o: &mut dyn Outside, patch: Ptr32<Patch>) {
    let p = patch.addr();
    let mut name: i32 = 0;
    for (pts, cols, rows) in GRIDS {
        let mut at = m.u32(p + pts);
        if m.u16(p + rows) != 0 {
            let mut i = 0u32;
            loop {
                o.loadname(m, name as i16 as i32);
                o.bgnline(m);
                i += 1;
                name = name.wrapping_add(1);
                if m.u16(p + cols) != 0 {
                    let mut j = 0u32;
                    loop {
                        o.v3f(m, at);
                        j += 1;
                        at = at.wrapping_add(12);
                        if j >= m.u16(p + cols) as u32 {
                            break;
                        }
                    }
                }
                o.endline(m);
                if i >= m.u16(p + rows) as u32 {
                    break;
                }
            }
        }
    }
}

/// [soft 0x0847d5e4 SI_patchDrawModif]: the grids as strips over the points whose flag
/// (`u16`, the array at +0x1d8 for the first grid and +0x1dc for the second) is not zero.
/// A strip is open from the first flagged point of a row; an unflagged point closes it,
/// the end of the row closes it.
pub fn patch_draw_modif(m: &mut Mem, o: &mut dyn Outside, patch: Ptr32<Patch>) {
    let p = patch.addr();
    let mut open = false;
    for ((pts, cols, rows), modif) in GRIDS.into_iter().zip([MODIF0, MODIF1]) {
        let mut at = m.u32(p + pts);
        let mut flag = m.u32(p + modif);
        if m.u16(p + rows) != 0 {
            let mut i = 0u32;
            loop {
                i += 1;
                if m.u16(p + cols) != 0 {
                    let mut j = 0u32;
                    loop {
                        if m.u16(flag) != 0 {
                            if !open {
                                o.bgnline(m);
                                open = true;
                            }
                            o.v3f(m, at);
                        } else if open {
                            o.endline(m);
                            open = false;
                        }
                        j += 1;
                        at = at.wrapping_add(12);
                        flag = flag.wrapping_add(2);
                        if j >= m.u16(p + cols) as u32 {
                            break;
                        }
                    }
                }
                if open {
                    o.endline(m);
                    open = false;
                }
                if i >= m.u16(p + rows) as u32 {
                    break;
                }
            }
        }
    }
}

/// [soft 0x0847d25c SI_patchHitAll]: picks the segments of the two display grids one at a
/// time. `view` goes to `SI_setProjectionMatrix`, `matrix` to `multmatrix`, `out` receives
/// the segment number. A hit buffer of `SI_calloc(2 × (cells of both grids), 2)` shorts is
/// made first and freed at the end. For each row (grid 0 first, then grid 1 only when grid 0
/// had no hit): `initnames`, `pick(buffer, n)`, the projection, `multmatrix`; then
/// `columns − 1` segments, each with `loadname` of a counter that restarts at 1 in every
/// row (`short`), `bgnline`, two vertices, `endline`; `endpick`. A hit sets `*out` to the
/// first word's neighbour (`buffer[1]`, a `short`) plus the segments of the rows before and
/// ends with the projection reset; without one the row's column count joins the count of
/// segments before, the counter restarts and the projection is set again. Returns 1 for a
/// hit, else 0.
pub fn patch_hit_all(m: &mut Mem, o: &mut dyn Outside, view: u32, patch: Ptr32<Patch>, matrix: u32, out: u32) -> i16 {
    let p = patch.addr();
    let mut before: i32 = 0;
    let mut name: i32 = 0;
    let mut hit: i16 = 0;
    let c0 = (m.u16(p + ROWS0) as i32).wrapping_mul(m.u16(p + COLS0) as i32);
    let c1 = (m.u16(p + ROWS1) as i32).wrapping_mul(m.u16(p + COLS1) as i32);
    let n = c0.wrapping_add(c1).wrapping_mul(2);
    let buf = m.si_calloc(n as u32, 2);
    for (pts, cols, rows) in GRIDS {
        if hit != 0 {
            break;
        }
        let mut at = m.u32(p + pts);
        let columns = m.u16(p + cols) as i32;
        if m.u16(p + rows) == 0 {
            continue;
        }
        let mut i = 0u32;
        loop {
            o.initnames(m);
            o.pick(m, buf, n);
            o.si_set_projection_matrix(m, view);
            o.multmatrix(m, matrix);
            if columns - 1 > 0 {
                for _ in 0..columns - 1 {
                    name = name.wrapping_add(1);
                    o.loadname(m, name as i16 as i32);
                    o.bgnline(m);
                    o.v3f(m, at);
                    at = at.wrapping_add(12);
                    o.v3f(m, at);
                    o.endline(m);
                }
            }
            if o.endpick(m, buf) != 0 {
                hit = 1;
                let v = (m.i16(buf.wrapping_add(2)) as i32).wrapping_add(before);
                m.set_i32(out, v);
                o.si_set_projection_matrix(m, view);
                break;
            }
            before = before.wrapping_add(columns);
            name = 0;
            o.si_set_projection_matrix(m, view);
            at = at.wrapping_add(12);
            i += 1;
            if i >= m.u16(p + rows) as u32 {
                break;
            }
        }
    }
    if buf != 0 {
        m.free(buf);
    }
    hit
}
