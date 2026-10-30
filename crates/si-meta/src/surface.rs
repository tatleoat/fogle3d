// SPDX-License-Identifier: MIT
//! `metaball.c`, the surface of a system (finding 0038): `SI_metaSysGetSurface`, the volume
//! of the surface and the commands built on it. The metaball library itself (`VC*`, `user.c`)
//! is outside this crate: every call to it goes through [`MetaCalls`](crate::metaball::MetaCalls).

use crate::metaball::*;
use si_hrc::image::meta::{ball, system};
use si_mem::{Mem, Outside};
use si_model::model_image::{metaball as modelball, mx};
use si_x87::{x, F80};

/// `SI_rgbActive (0x086793d0)`, a short: the render-mode flag `SI_metaSystemVolumeSurface` hands
/// `SI_metaSysGetSurface` as the resolution selector.
const SI_RGB_ACTIVE: u32 = 0x0867_93d0;
/// `SI_edSumatraTo3x (0x08680e50)`, a short: when set the library is started with `VCInit`,
/// otherwise with `VCReInit`.
const SI_ED_SUMATRA_TO_3X: u32 = 0x0868_0e50;
/// `SI_MetaSystem` + 0x48: a short cleared at every rebuild.
const SYS_SHORT_48: u32 = 0x48;
/// `.rodata` constants of the routine: the box's start values (the largest float, as doubles)
/// and the smallest resolution step (0.001 as a double, its float used when below).
const BOX_START: f64 = 3.4028234663852886e+38;
const MIN_STEP_TEST: f64 = 0.001;
const MIN_STEP: f32 = 0.0010000000474974513;

fn model_of(m: &Mem, ball_: u32) -> u32 {
    m.u32(ball_ + ball::MODEL)
}

/// `(a - b)` in the register, stored as a float.
fn sub_f32(a: f64, b: f64) -> f32 {
    (x(a) - x(b)).to_f32()
}

/// [soft 0x0845b2c4 SI_metaSysGetSurface]: the surface of `sys` is built when the one it has
/// is not for this `render` (0 wire, else render resolution) or is dirty (`dirty` is the
/// caller's flag for "keep the pieces" mode: stored at +0x44). The old triangles and pieces
/// are freed, the groups are validated, the library is started again and every metaball node
/// the current list shows (or not listed) becomes a primitive with the ball's matrix times its
/// node's, times the inverse of the system's; boxes are collected; groups and unblend pairs
/// are handed over; then the library makes the polygons into the system's triangles
/// (+0x4c, +0x50) and pieces (+0x54, +0x58) on a box that is a power-of-two grid around the
/// balls. The system's box is reset to "empty" at the end and its first short set to 1.
pub fn si_meta_sys_get_surface(m: &mut Mem, o: &mut dyn Outside, sys: u32, pieces_mode: i16, render: i16) {
    let s46 = m.i16(sys + SYS_SHORT_46);
    if render == s46 && (m.u16(sys + SYS_SHORT_44) != 0 || pieces_mode == 0) {
        if m.u32(sys + SYS_TRIANGLES) == 0 {
            // falls into the freeing of the pieces
        } else if m.u32(sys + SYS_PIECES) != 0 {
            return;
        } else {
            free_triangles(m, sys);
        }
    } else {
        free_triangles(m, sys);
    }
    if m.u32(sys + SYS_PIECES) != 0 {
        let mut i: i32 = 0;
        while i < m.i32(sys + SYS_PIECE_COUNT) {
            let slot = m.u32(sys + SYS_PIECES).wrapping_add((i as u32) << 5).wrapping_add(4);
            let blk = m.u32(slot);
            if blk != 0 {
                m.free(blk);
                let slot = m.u32(sys + SYS_PIECES).wrapping_add((i as u32) << 5).wrapping_add(4);
                m.set_u32(slot, 0);
            }
            i += 1;
        }
        m.set_u32(sys + SYS_PIECE_COUNT, 0);
        let p = m.u32(sys + SYS_PIECES);
        if p != 0 {
            m.free(p);
            m.set_u32(sys + SYS_PIECES, 0);
        }
    }
    m.set_u16(sys + SYS_SHORT_46, render as u16);
    m.set_u16(sys + SYS_SHORT_44, 0);
    meta_system_validate_groups(m, sys);
    let bx = m.local(0x30);
    for k in 0..3 {
        m.set_f64(bx + 8 * k, BOX_START);
        m.set_f64(bx + 24 + 8 * k, -BOX_START);
    }
    if m.i16(SI_ED_SUMATRA_TO_3X) != 0 {
        o.vc_init(m);
    } else {
        o.vc_re_init(m);
    }
    m.set_u16(sys + SYS_SHORT_48, 0);
    let inv = m.local(0x40);
    // A system matrix without an inverse leaves the local as the stack had it.
    mx::inverse(m, m.u32(sys + system::MODEL) + 0x1c4, inv);
    let mat = m.local(0x40);
    let mut count = 0;
    let mut son = 0u32;
    let top = m.u32(sys + system::MODEL);
    loop {
        son = next_son(m, top, son);
        if son == 0 {
            break;
        }
        if m.i32(son + 0x30) != 8 {
            continue;
        }
        let list = current_list(m);
        let ent = o.si_find_gl_data(m, list, son, 3);
        if ent != 0 && m.u32(ent + 0xc) == 0 {
            continue;
        }
        count += 1;
        let b = m.u32(son + 0x48);
        let latdep = m.u32(model_of(m, b) + 0x44);
        mx::multiply(m, b + ball::MATRIX, latdep + 0x1c4, mat);
        mx::multiply(m, mat, inv, mat);
        let dm = b + 0x4c;
        for i in 0..4u32 {
            for j in 0..4u32 {
                let v = x(m.f32(mat + 16 * i + 4 * j)).to_f64();
                m.set_f64(dm + 32 * i + 8 * j, v);
            }
        }
        o.vc_create_prim(m, 0, dm, b);
        let h = m.i32(b);
        let junk = m.local(4);
        o.vc_info_prim(m, h, junk, b + 4);
        m.release(4);
        let h = m.i32(b);
        o.vc_set_prim_param(m, h, 0, dm);
        let h = m.i32(b);
        o.vc_set_prim_param(m, h, 2, b + 0xd4);
        let h = m.i32(b);
        o.vc_set_prim_param(m, h, 3, b + 0xdc);
        // `1.0 > weight` skips; unordered does not
        if !x(1.0f64).gt(x(m.f64(b + 0xd4))) {
            modelball::check_bounding_box(m, b, bx, 0.5);
        }
    }
    if count == 0 {
        m.release(0x40);
        m.release(0x40);
        m.release(0x30);
        return;
    }
    let mut g = m.u32(sys + SYS_GROUPS);
    while g != 0 {
        let mut l = m.u32(g + crate::metaball::group::LINKS);
        m.set_u32(g + 4, 0xffff_ffff);
        if l != 0 {
            let idx = m.i32(m.u32(l) + 4);
            m.set_i32(g + 4, idx);
            loop {
                if m.u16(l + link::FLAG) != 0 {
                    let bl = m.u32(l);
                    if m.i32(bl + 4) != idx {
                        let h = m.i32(bl);
                        o.vc_add_to_group(m, h, idx);
                    }
                }
                l = m.u32(l + link::NEXT);
                if l == 0 {
                    break;
                }
            }
        }
        g = m.u32(g + crate::metaball::group::NEXT);
    }
    let mut u = m.u32(sys + SYS_UNBLENDS);
    while u != 0 {
        let ga = m.u32(u + unblend::GROUP_A);
        let gb = m.u32(u + unblend::GROUP_B);
        if m.i32(ga + 4) != -1 {
            let ib = m.i32(gb + 4);
            if ib != -1 {
                let ia = m.i32(ga + 4);
                o.vc_unblend_groups(m, ia, ib);
            }
        }
        u = m.u32(u + unblend::NEXT);
    }
    // the largest extent of the box, as a float
    let (minx, miny, minz) = (m.f64(bx), m.f64(bx + 8), m.f64(bx + 16));
    let (maxx, maxy, maxz) = (m.f64(bx + 24), m.f64(bx + 32), m.f64(bx + 40));
    let dx = sub_f32(maxx, minx);
    let dy = sub_f32(maxy, miny);
    let dz = sub_f32(maxz, minz);
    let wide = if x(dx).gt(x(dy)) { dx } else { dy };
    let big: F80 = if x(dz).lt(x(wide)) { x(wide) } else { x(dz) };
    let res = if render != 0 { m.f32(sys + system::RENDER_RESOLUTION) } else { m.f32(sys + system::WIRE_RESOLUTION) };
    let mut step: F80 = x(res).abs();
    if step.lt(x(MIN_STEP_TEST)) {
        step = x(MIN_STEP);
    }
    let mut n: i32 = 0;
    let mut big = big;
    if step.lt(big) {
        loop {
            big = (big * 0.5f64).to_f32().into();
            n += 1;
            if !big.gt(step) {
                break;
            }
        }
    }
    if n < 4 {
        n = 4;
    }
    let cells: i32 = 1i32.wrapping_shl(n as u32 & 31);
    let r2n = step * x(cells);
    let half = |a: f64, b: f64| -> f32 { ((x(a) + x(b)) * 0.5f64).to_f32() };
    let (hx, hy, hz) = (half(maxx, minx), half(maxy, miny), half(maxz, minz));
    let snap = |h: f32| -> f32 {
        // floor(h / step) through a double, times step
        let q = (x(h) / step).round_int(si_x87::Round::Down).to_f64();
        (x(q) * step).to_f32()
    };
    let (cx, cy, cz) = (snap(hx), snap(hy), snap(hz));
    let hf = (r2n * 0.5f64).to_f32();
    let lo = [x(cx) - x(hf), x(cy) - x(hf), x(cz) - x(hf)];
    let hi = [x(cx) + x(hf), x(cy) + x(hf), x(cz) + x(hf)];
    for k in 0..3u32 {
        m.set_f64(bx + 8 * k, lo[k as usize].to_f64());
        m.set_f64(bx + 24 + 8 * k, hi[k as usize].to_f64());
        m.set_f64(sys + 4 + 8 * k, lo[k as usize].to_f64());
        m.set_f64(sys + 0x1c + 8 * k, hi[k as usize].to_f64());
    }
    m.set_u16(sys + SYS_SHORT_44, pieces_mode as u16);
    let mode = if pieces_mode == 0 { 0 } else { 2 };
    o.vc_gen_poly_data(
        m,
        mode,
        n,
        bx,
        bx,
        sys + SYS_TRI_COUNT,
        sys + SYS_TRIANGLES,
        sys + SYS_PIECE_COUNT,
        sys + SYS_PIECES,
    );
    o.vc_rm_all_tri(m);
    for k in 0..3u32 {
        m.set_f64(sys + 4 + 8 * k, BOX_START);
        m.set_f64(sys + 0x1c + 8 * k, -BOX_START);
    }
    m.set_u16(sys, 1);
    m.release(0x40);
    m.release(0x40);
    m.release(0x30);
}

fn free_triangles(m: &mut Mem, sys: u32) {
    let t = m.u32(sys + SYS_TRIANGLES);
    if t != 0 {
        m.set_u32(sys + SYS_TRI_COUNT, 0);
        m.free(t);
        m.set_u32(sys + SYS_TRIANGLES, 0);
    }
}

/// `SI_dblIdentity (0x086d3940)`: a 4 x 4 matrix of doubles, the identity.
const SI_DBL_IDENTITY: u32 = 0x086d_3940;

/// [soft 0x0845e768 metaballSetWeight]: the ball's weight (a double at +0xd4) is the float, and
/// the library is asked what ratio (+0xcc) goes with it: a scratch primitive is made, given the
/// weight, asked for its parameter 1 into the ball, and deleted.
pub fn metaball_set_weight(m: &mut Mem, o: &mut dyn Outside, ball_: u32, weight: f32) {
    m.set_f64(ball_ + ball::WEIGHT, x(weight).to_f64());
    let id = m.local(4);
    o.vc_create_prim(m, 0, SI_DBL_IDENTITY, id);
    let h = m.i32(id);
    o.vc_set_prim_param(m, h, 2, ball_ + ball::WEIGHT);
    let h = m.i32(id);
    o.vc_get_prim_param(m, h, 1, ball_ + ball::RATIO);
    let h = m.i32(id);
    o.vc_delete_prim(m, h);
    m.release(4);
}

/// [soft 0x0845e7d0 SI_metaSystemVolumeSurface]: the surface is built (`SI_metaSysGetSurface` with
/// the render flag `SI_rgbActive` as the pieces mode) and every triangle of every piece (three
/// vertex indexes at the piece's block, vertices of 0x48 bytes starting with three doubles)
/// adds its edge-cross vector `n` to the sums: `*volume` gets `a . n` (a is the first corner)
/// and `*area` gets `|n|`, each as a float; at the end `*volume / 6` and `*area * 0.5`.
pub fn si_meta_system_volume_surface(m: &mut Mem, o: &mut dyn Outside, sys: u32, volume: u32, area: u32) {
    m.set_u32(volume, 0);
    m.set_u32(area, 0);
    let rgb = m.i16(SI_RGB_ACTIVE);
    si_meta_sys_get_surface(m, o, sys, rgb, 0);
    let mut i: i32 = 0;
    while i < m.i32(sys + SYS_PIECE_COUNT) {
        let pieces = m.u32(sys + SYS_PIECES);
        let blk = m.u32(pieces.wrapping_add((i as u32) << 5).wrapping_add(4));
        let verts = m.u32(sys + SYS_TRIANGLES);
        let corner = |m: &Mem, k: u32| -> [f32; 3] {
            let idx = m.u32(blk + 4 * k);
            let v = verts.wrapping_add(idx.wrapping_mul(9) << 3);
            [0u32, 8, 16].map(|d| x(m.f64(v + d)).to_f32())
        };
        let a = corner(m, 0);
        let b = corner(m, 1);
        let c = corner(m, 2);
        let u = [x(a[0]) - x(b[0]), x(a[1]) - x(b[1]), x(a[2]) - x(b[2])];
        let w = [x(c[0]) - x(b[0]), x(c[1]) - x(b[1]), x(c[2]) - x(b[2])];
        let nx = u[1] * w[2] - u[2] * w[1];
        let ny = u[2] * w[0] - u[0] * w[2];
        let nz = u[0] * w[1] - u[1] * w[0];
        let t = nz * nz + (nx * nx + ny * ny);
        let len = t.sqrt().to_f32();
        let sum = (x(len) + x(m.f32(area))).to_f32();
        m.set_f32(area, sum);
        let dot = (nx * a[0] + ny * a[1]) + nz * a[2];
        let sum = (dot + x(m.f32(volume))).to_f32();
        m.set_f32(volume, sum);
        i += 1;
    }
    let v = (x(m.f32(volume)) / 6.0f64).to_f32();
    m.set_f32(volume, v);
    let a = (x(m.f32(area)) * 0.5f64).to_f32();
    m.set_f32(area, a);
}

/// `SI_frameCurrent (0x08782040)`, `SI_frameEnd (0x08782044)`, `SI_frameStart (0x08782050)`,
/// `SI_frameStep (0x08782054)`; `SI_tabletActive (0x086793f8)`, `SI_tabletButton (0x086794b4)`.
const SI_FRAME_CURRENT: u32 = 0x0878_2040;
const SI_FRAME_END: u32 = 0x0878_2044;
const SI_FRAME_START: u32 = 0x0878_2050;
const SI_FRAME_STEP: u32 = 0x0878_2054;
const SI_TABLET_ACTIVE: u32 = 0x0867_93f8;
const SI_TABLET_BUTTON: u32 = 0x0867_94b4;
/// `.rodata` 0x0865fbc8: the exponent (a float just under a third) the scale is raised to.
const THIRD: f32 = 0.33333298563957214;

/// [soft 0x0845e9d0 SI_metaSystemPreserveVolume]: the active system's volume at the start frame
/// is kept at every later frame: the node is scaled by `(volume0 / volume)^(1/3)` (the volumes
/// are multiplied by the node's matrix determinant) and its keys saved. Pressing one of the
/// buttons 0x65..0x67 (device or tablet) stops it.
pub fn si_meta_system_preserve_volume(m: &mut Mem, o: &mut dyn Outside) {
    let sys = si_get_active_meta_system(m, o, 0);
    if sys == 0 {
        return;
    }
    let node = m.u32(sys + system::MODEL);
    let start = m.i32(SI_FRAME_START);
    o.si_scene_frame_update(m, start);
    let v0 = m.local(4);
    let junk = m.local(4);
    let v1 = m.local(4);
    let scale = m.local(12);
    si_meta_system_volume_surface(m, o, sys, v0, junk);
    let latdep = m.u32(node + 0x44);
    let det = si_math::algebra::matrix_determinant3(&mat_at(m, latdep + 0x1c4));
    let prod = (det * m.f32(v0)).to_f32();
    m.set_f32(v0, prod);
    o.si_savekey_object_all(m, node);
    let mut frame = m.i32(SI_FRAME_STEP).wrapping_add(m.i32(SI_FRAME_START));
    while frame <= m.i32(SI_FRAME_END) {
        if m.i32(SI_TABLET_ACTIVE) != 0 {
            o.si_tablet_get_button_up_event(m);
        }
        let mut stop = false;
        for b in [0x65, 0x66, 0x67] {
            if o.getbutton(m, b) != 0 || m.i32(SI_TABLET_BUTTON) == b {
                stop = true;
                break;
            }
        }
        if stop {
            break;
        }
        m.set_i32(SI_FRAME_CURRENT, frame);
        o.si_scene_frame_update(m, frame);
        si_meta_system_volume_surface(m, o, sys, v1, junk);
        let latdep = m.u32(node + 0x44);
        let det = si_math::algebra::matrix_determinant3(&mat_at(m, latdep + 0x1c4));
        let prod = det * m.f32(v1);
        m.set_f32(v1, prod.to_f32());
        let base = (x(m.f32(v0)) / prod).to_f32();
        let r = si_x87::trig::replay::next("powf", &[base.to_bits() as u128, THIRD.to_bits() as u128])
            .unwrap_or_else(|| x((base as f64).powf(THIRD as f64) as f32))
            .to_f32();
        for k in 0..3 {
            m.set_f32(scale + 4 * k, r);
        }
        o.si_model_add_scaling(m, node, scale, 1, 0);
        o.si_savekey_object_all(m, node);
        frame = frame.wrapping_add(m.i32(SI_FRAME_STEP));
    }
    m.release(12);
    m.release(4);
    m.release(4);
    m.release(4);
}

fn mat_at(m: &Mem, a: u32) -> si_math::types::Matrix {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, e) in row.iter_mut().enumerate() {
            *e = m.f32(a + (16 * i + 4 * j) as u32);
        }
    }
    out
}

/// The system a node's definition list starts with (`*(*(node + 0x38))`).
fn first_definition(m: &Mem, node: u32) -> u32 {
    let list = m.u32(node + 0x38);
    m.u32(list)
}

/// [soft 0x0845c078 SI_metaGroupCopy]: the groups of the system in `src_node` are copied into
/// the system in `dst_node`: the first group with its name and word, the others with name
/// only (their word stays as the allocator left it), linked in order; then each link of the
/// original puts the metaball of the destination's node that has the same name (kind 8, the
/// first one) into the matching new group (its group word cleared first); the unblend pairs
/// are copied last. Nothing happens if either node, system or the source's groups are missing.
pub fn si_meta_group_copy(m: &mut Mem, dst_node: u32, src_node: u32) {
    if dst_node == 0 || src_node == 0 {
        return;
    }
    let src_sys = first_definition(m, src_node);
    if src_sys == 0 {
        return;
    }
    let mut sg = m.u32(src_sys + SYS_GROUPS);
    if sg == 0 {
        return;
    }
    let dst_sys = first_definition(m, dst_node);
    if dst_sys == 0 {
        return;
    }
    let mut last = 0;
    let n = m.si_malloc(group::SIZE);
    if n != 0 {
        let name = m.u32(sg + group::NAME);
        let v = m.strdup(name);
        m.set_u32(n + group::NAME, v);
        let w = m.u32(sg + group::WORD);
        m.set_u32(n + group::WORD, w);
        for off in [group::LINKS, group::NEXT, group::PREV] {
            m.set_u32(n + off, 0);
        }
        m.set_u32(dst_sys + SYS_GROUPS, n);
        last = n;
    }
    sg = m.u32(sg + group::NEXT);
    while sg != 0 {
        let n = m.si_malloc(group::SIZE);
        if n != 0 {
            let name = m.u32(sg + group::NAME);
            let v = m.strdup(name);
            m.set_u32(n + group::NAME, v);
            m.set_u32(n + group::LINKS, 0);
            m.set_u32(n + group::NEXT, 0);
            m.set_u32(n + group::PREV, last);
            m.set_u32(last + group::NEXT, n);
            last = n;
        }
        sg = m.u32(sg + group::NEXT);
    }
    let mut dg = m.u32(dst_sys + SYS_GROUPS);
    let mut sg = m.u32(src_sys + SYS_GROUPS);
    while dg != 0 {
        let mut l = m.u32(sg + group::LINKS);
        while l != 0 {
            let ball_ = m.u32(l + link::BALL);
            let node = m.u32(ball_ + 8);
            let name = m.strdup(m.u32(node + 4));
            let top = m.u32(dst_sys + system::MODEL);
            let mut p = 0;
            loop {
                p = next_son(m, top, p);
                if p == 0 {
                    break;
                }
                if m.i32(p + 0x30) == 8 {
                    let pname = m.u32(p + 4);
                    if si_mem::clib::strcmp(m, name, pname) == 0 {
                        let b = m.u32(p + 0x48);
                        m.set_u32(b + BALL_GROUP, 0);
                        meta_group_add_primitive(m, dg, b);
                        break;
                    }
                }
            }
            if name != 0 {
                m.free(name);
            }
            l = m.u32(l + link::NEXT);
        }
        dg = m.u32(dg + group::NEXT);
        sg = m.u32(sg + group::NEXT);
    }
    meta_unblend_copy(m, dst_sys, src_sys);
}

/// The start of the ring table `SI_metaCircle` in `.bss`: 141 rows (exponent steps) of 24 pairs.
pub const SI_META_CIRCLE: u32 = 0x0881_b640;
const CIRCLE_ROWS: u32 = 0x8d;
const CIRCLE_COLS: u32 = 24;
const CIRCLE_RADIUS_STEP: f32 = 0.05000000074505806;
const CIRCLE_ANGLE_STEP: f32 = 0.2617993950843811;

/// [soft 0x084606f8 metaCircleInit]: fills the table the metaball outline drawing and the
/// traversal of its points read. Row `r` has the exponent `n = 1 + 0.05·r` (a float running
/// sum); column `c` the angle `c · 15°` (a float running sum, restarted per row). The pair
/// stored is `(sign(sin a) · |sin a|^(2/n), sign(cos a) · |cos a|^(2/n))` as floats, the
/// sine and cosine first rounded to float, the power taken by the library on doubles and
/// multiplied by the sign in the register before the single rounding.
pub fn meta_circle_init(m: &mut Mem) {
    let mut n = 1.0f32;
    for row in 0..CIRCLE_ROWS {
        let next = n + CIRCLE_RADIUS_STEP;
        let mut a = 0.0f32;
        for col in 0..CIRCLE_COLS {
            let s = si_x87::trig::sin(a as f64).to_f32();
            let sign_s = if s < 0.0 { -1.0f64 } else { 1.0 };
            let c = si_x87::trig::cos(a as f64).to_f32();
            let sign_c = if c < 0.0 { -1.0f64 } else { 1.0 };
            let e = (x(2.0f64) / x(n as f64)).to_f64();
            let ps = si_x87::trig::pow((s.abs()) as f64, e);
            m.set_f32(SI_META_CIRCLE + row * 0xc0 + col * 8, (ps * x(sign_s)).to_f32());
            let pc = si_x87::trig::pow((c.abs()) as f64, e);
            m.set_f32(SI_META_CIRCLE + row * 0xc0 + col * 8 + 4, (pc * x(sign_c)).to_f32());
            a += CIRCLE_ANGLE_STEP;
        }
        n = next;
    }
}
