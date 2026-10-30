// SPDX-License-Identifier: MIT
//! The four-sided patch (finding 0034, "four2"): `si_createFourSidedPatch` and the update of the
//! four-sided effect.
//!
//! Four open splines are matched end to end (the pairing of the end points), the ends are pulled to
//! their midpoints, and a patch is solved whose boundary follows them. Everything works on
//! `si-mem`'s image. A routine of the original that is not ported (the status line, the sleep, the
//! relation drop) is called through [`si_mem::Outside`], written
//! `name (0xADDRESS)`.

use super::{graf_list, ModelS, ViewS, SI_ED_RELAX_REL_FROM_DEL_MODEL, SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use crate::extrusion::{ModelSide, SplineSide};
use si_mem::{Mem, Outside, Ptr32};
use si_model::model_image::{defs, setup, touch};
use si_types::geom::{Patch, Spline};
use si_types::model::Model;
use si_x87::F80;

/// The node name "surf" (0x0868dc4a in the binary's data).
pub const NAME_SURF: u32 = 0x0868_dc4a;
/// "All 4 splines should be open" (29 bytes with the end).
pub const MSG_OPEN: u32 = 0x0868_dc4f;
/// "Two of the opposite splines do not have the same topology", first pair.
pub const MSG_TOPOLOGY_03: u32 = 0x0868_dc80;
/// The same text, second pair.
pub const MSG_TOPOLOGY_12: u32 = 0x0868_dcc0;
/// "You need to have visible curve segments to perform this operation."
pub const MSG_SEGMENTS: u32 = 0x0868_dd00;
/// "Your relation has been deleted. No face allowed."
pub const MSG_NO_FACE: u32 = 0x0868_dee0;
/// "Your relation has been deleted. "
pub const MSG_DELETED: u32 = 0x0868_df20;

const TOPOLOGY_TEXT: &[u8] = b"Two of the opposite splines do not have the same topology";

/// Puts the texts of this file into the image (the program's loader does it in the original).
pub fn store_names(m: &mut Mem) {
    m.put_cstr(NAME_SURF, b"surf");
    m.put_cstr(MSG_OPEN, b"All 4 splines should be open");
    m.put_cstr(MSG_TOPOLOGY_03, TOPOLOGY_TEXT);
    m.put_cstr(MSG_TOPOLOGY_12, TOPOLOGY_TEXT);
    m.put_cstr(MSG_SEGMENTS, b"You need to have visible curve segments to perform this operation.");
    m.put_cstr(MSG_NO_FACE, b"Your relation has been deleted. No face allowed.");
    m.put_cstr(MSG_DELETED, b"Your relation has been deleted. ");
}

const EPS: f64 = 0.0001;

fn f80(v: f32) -> F80 {
    F80::from_f32(v)
}

/// A point through a matrix of the image: `((x*m[c] + y*m[c+4]) + z*m[c+8]) + m[c+12]` per column.
fn transform(m: &Mem, mat: u32, p: [f32; 3]) -> [f32; 3] {
    let f = |k: u32| f80(m.f32(mat + 4 * k));
    let (x, y, z) = (f80(p[0]), f80(p[1]), f80(p[2]));
    let col = |c: u32| (((x * f(c) + y * f(c + 4)) + z * f(c + 8)) + f(c + 12)).to_f32();
    [col(0), col(1), col(2)]
}

/// The distance of two points, in the register's precision: `sqrt((dx^2 + dy^2) + dz^2)`.
fn dist80(a: [f32; 3], b: [f32; 3]) -> F80 {
    let d = |i: usize| f80(a[i]) - f80(b[i]);
    let (dx, dy, dz) = (d(0), d(1), d(2));
    ((dx * dx + dy * dy) + dz * dz).sqrt()
}

/// Moves the end of a spline by `a - b` [soft 0x0819f948 splineSetExtremity]: `which` 0 is the last
/// key, else the first; a linear or Bezier-type curve (type 0, 1) moves its one key, type 3 two keys
/// and type 2 three, walking inwards from that end. A type above 3 leaves the original's count
/// register undefined; it moves nothing here.
pub fn spline_set_extremity(m: &mut Mem, s: u32, which: i16, a: [f32; 3], b: [f32; 3]) {
    let delta: [f32; 3] = [0, 1, 2].map(|i| (f80(a[i]) - f80(b[i])).to_f32());
    let (mut idx, step): (i16, i16) = if which == 0 { (m.u16(s + 0x10).wrapping_sub(1) as i16, -1) } else { (0, 1) };
    let kind = m.u32(s + 0x0c);
    let count: i16 = match kind {
        0 | 1 => 1,
        3 => 2,
        2 => 3,
        _ => 0,
    };
    for _ in 0..count {
        let key = m.u32(s + 0x14).wrapping_add((idx as i32 * 0x30) as u32);
        for (k, d) in delta.iter().enumerate() {
            let at = key + 0x0c + 4 * k as u32;
            let v = (f80(m.f32(at)) + f80(*d)).to_f32();
            m.set_f32(at, v);
        }
        idx = idx.wrapping_add(step);
    }
}

/// Copies a text of the binary (with its end) to a buffer.
fn put_message(m: &mut Mem, buf: u32, from: u32, n: u32) {
    for k in 0..n {
        let b = m.u8(from + k);
        m.set_u8(buf + k, b);
    }
}

/// The four splines are disposed through a cell array of the image.
fn dispose_all(m: &mut Mem, cells: u32) {
    for k in 0..4 {
        si_spline::intersect_image::spline_dispose(m, cells + 4 * k);
    }
}

/// The basis of a type and tension, transposed, as the two rows of four numbers the system uses: the
/// value rows (`T[i][3]` with the zero terms) and the row sums.
fn basis_rows(kind: i32, tension: f32, bm: &mut si_math::types::Matrix) -> ([f32; 4], [f32; 4]) {
    si_spline::basis::spline_basis_matrix(kind, tension, bm);
    let t = si_math::algebra::matrix_transpose(bm);
    let mut r = [0f32; 4];
    let mut s = [0f32; 4];
    for i in 0..4 {
        let v = |j: usize| f80(t[i][j]);
        r[i] = (((v(0) * F80::ZERO + v(1) * F80::ZERO) + v(2) * F80::ZERO) + v(3)).to_f32();
        s[i] = (((v(0) + v(1)) + v(2)) + v(3)).to_f32();
    }
    (r, s)
}

/// Makes (or remakes) the patch node of four open splines [soft 0x0819b460 si_createFourSidedPatch].
/// `models` is the array of the four curve models, `node_cell` holds the node to update (or 0: a new
/// node "surf" is made and stored back), `msg` receives the reason of a refusal. Answers 1 when
/// the node holds the patch, 0 when it was refused.
pub fn create_four_sided_patch(m: &mut Mem, o: &mut dyn Outside, models: u32, node_cell: u32, msg: u32) -> i32 {
    let sp0 = m.sp();
    let mut node = m.u32(node_cell);
    let new_node = node == 0;
    if new_node {
        node = si_hrc::image::node::model_allocate(m, NAME_SURF).addr();
    }
    let cells = m.local(16); // the four working splines
    let mats = m.local(0x100); // their matrices
    let mut src = [0u32; 4];
    for i in 0..4u32 {
        let model = m.u32(models + 4 * i);
        src[i as usize] = m.u32(model + 0x48);
        let mat = mats + 0x40 * i;
        si_hrc::image::texture::matrix_copy(m, model + 0x1c4, mat);
        let s = src[i as usize];
        let copy = if m.i32(s + 0x0c) == 1 {
            si_spline::convert_image::spline_bezier_expand(m, Ptr32::new(s))
        } else {
            si_spline::intersect_image::spline_copy(m, Ptr32::new(s))
        };
        m.set_u32(cells + 4 * i, copy.addr());
        si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), copy, mat);
    }
    // the end points of the splines, through their matrices
    let out = m.local(12);
    let mut p = [[0f32; 3]; 8];
    for i in 0..4usize {
        let mat = mats + 0x40 * i as u32;
        si_spline::intersect_image::spline_value(m, Ptr32::<Spline>::new(src[i]), 0.0, out);
        p[2 * i] = transform(m, mat, [m.f32(out), m.f32(out + 4), m.f32(out + 8)]);
        let nb = si_spline::intersect_image::spline_nb_segment(m, Ptr32::<Spline>::new(m.u32(cells + 4 * i as u32))) as i16;
        si_spline::intersect_image::spline_value(m, Ptr32::<Spline>::new(src[i]), f32::from(nb), out);
        p[2 * i + 1] = transform(m, mat, [m.f32(out), m.f32(out + 4), m.f32(out + 8)]);
    }
    let mut sp: [u32; 4] = [0, 1, 2, 3].map(|i| m.u32(cells + 4 * i));
    let (mut flag1, mut flag2, mut flag3) = (0i16, 0i16, 0i16);

    // the second spline: the one whose end lies nearest the first's start
    let mut best = dist80(p[0], p[2]).to_f32();
    let mut u = 2usize;
    for k in 3..8 {
        let d = dist80(p[0], p[k]).to_f32();
        if d < best {
            u = k;
            best = d;
        }
    }
    if u & 1 == 1 {
        p.swap(u - 1, u);
        flag1 = 1;
    }
    let i = u / 2;
    if i != 1 {
        sp.swap(i, 1);
        p.swap(2 * i, 2);
        p.swap(2 * i + 1, 3);
    }
    // the third: nearest the first's end
    best = dist80(p[1], p[4]).to_f32();
    u = 4;
    for k in 5..8 {
        let d = dist80(p[1], p[k]).to_f32();
        if d < best {
            u = k;
            best = d;
        }
    }
    if u & 1 == 1 {
        p.swap(u - 1, u);
        flag2 = 1;
    }
    let i = u / 2;
    if i != 2 {
        sp.swap(i, 2);
        p.swap(2 * i, 4);
        p.swap(2 * i + 1, 5);
    }
    // the fourth: its direction by the end of the second
    let d7 = dist80(p[3], p[7]);
    let d6 = dist80(p[3], p[6]);
    if d6.gt(d7) {
        p.swap(6, 7);
        flag3 = 1;
    }
    for k in 0..4 {
        m.set_u32(cells + 4 * k as u32, sp[k]);
    }

    let closed = |s: u32| m.i16(s + 8) != 0;
    if closed(sp[0]) || closed(sp[3]) || closed(sp[1]) || closed(sp[2]) {
        dispose_all(m, cells);
        put_message(m, msg, MSG_OPEN, 29);
        m.set_sp(sp0);
        return 0;
    }
    let differ = |m: &Mem, a: u32, b: u32| m.u32(a + 0x0c) != m.u32(b + 0x0c) || m.u16(a + 0x10) != m.u16(b + 0x10);
    if differ(m, sp[0], sp[3]) {
        dispose_all(m, cells);
        put_message(m, msg, MSG_TOPOLOGY_03, 58);
        m.set_sp(sp0);
        return 0;
    }
    if differ(m, sp[1], sp[2]) {
        dispose_all(m, cells);
        put_message(m, msg, MSG_TOPOLOGY_12, 58);
        m.set_sp(sp0);
        return 0;
    }
    if m.u16(sp[0] + 0x10) <= 1 || m.u16(sp[1] + 0x10) <= 1 {
        // the splines and a new node are left, as in the original
        put_message(m, msg, MSG_SEGMENTS, 67);
        m.set_sp(sp0);
        return 0;
    }

    // the ends are pulled to the midpoint of the two that meet
    let half = F80::from_f64(0.5);
    let pairs: [(usize, usize, [(usize, i16, usize); 2]); 4] = [
        (0, 2, [(0, 1, 0), (1, flag1 ^ 1, 2)]),
        (1, 4, [(0, 0, 1), (2, flag2 ^ 1, 4)]),
        (3, 6, [(1, flag1, 3), (3, flag3 ^ 1, 6)]),
        (5, 7, [(2, flag2, 5), (3, flag3, 7)]),
    ];
    for (a, b, calls) in pairs {
        if dist80(p[a], p[b]).gt(F80::from_f64(EPS)) {
            let mid: [f32; 3] = [0, 1, 2].map(|c| ((f80(p[a][c]) + f80(p[b][c])) * half).to_f32());
            for (s, which, pt) in calls {
                spline_set_extremity(m, sp[s], which, mid, p[pt]);
            }
        }
    }

    // the linear system of the surface
    let n_a = m.i16(sp[0] + 0x10) as i32;
    let n_b = m.i16(sp[1] + 0x10) as i32;
    let n = n_a * n_b;
    let per = (n_a + n_b) * 2;
    let cols = n + per;
    let rows = n + 4;
    // one block of (n + 4) * cols zeroed 4-byte cells
    let a_addr = m.si_calloc((rows * cols) as u32, 4);
    let mut a = vec![0f32; (rows * cols) as usize];
    let type_b = m.u32(sp[1] + 0x0c);
    let type_a = m.u32(sp[0] + 0x0c);
    let tension_b = m.f32(sp[1] + 0x1c);
    let tension_a = m.f32(sp[0] + 0x1c);
    let mut bm: si_math::types::Matrix = [[0.0; 4]; 4];
    let mut row = 0i32;
    let mut put = |a: &mut Vec<f32>, idx: i32, v: f32| {
        if idx >= 0 && (idx as usize) < a.len() {
            a[idx as usize] = v;
        }
    };
    let (r_b, s_b) = basis_rows(type_b as i32, tension_b, &mut bm);
    for k in 0..n_a {
        let base = row * cols;
        let mut c = [k, n_a + k, 2 * n_a + k, 3 * n_a + k];
        if type_b == 0 {
            c[2] = n_a + k;
            c[3] = n_a + k;
        }
        for j in 0..4 {
            put(&mut a, base + c[j], r_b[j]);
        }
        put(&mut a, base + n + k, 1.0);
        row += 1;
    }
    for k in 0..n_a {
        let base = row * cols;
        let c0 = n_a * (n_b - 4);
        let mut c = [c0, c0 + n_a, c0 + 2 * n_a, c0 + 3 * n_a];
        if type_b == 0 {
            c = [n_a * (n_b - 2), n_a * (n_b - 1), n_a * (n_b - 2), n_a * (n_b - 2)];
        }
        for j in 0..4 {
            put(&mut a, base + c[j] + k, s_b[j]);
        }
        put(&mut a, base + n + n_a + k, 1.0);
        row += 1;
    }
    let (r_a, s_a) = basis_rows(type_a as i32, tension_a, &mut bm);
    for j in 0..n_b {
        let base = row * cols;
        let mut c = [0, 1, 2, 3];
        if type_a == 0 {
            c[2] = 1;
            c[3] = 1;
        }
        for q in 0..4 {
            put(&mut a, base + c[q] + j * n_a, r_a[q]);
        }
        put(&mut a, base + n + 2 * n_a + j, 1.0);
        row += 1;
    }
    for j in 0..n_b {
        let base = row * cols;
        let mut c = [n_a - 4, n_a - 3, n_a - 2, n_a - 1];
        if type_a == 0 {
            c = [n_a - 2, n_a - 1, n_a - 2, n_a - 2];
        }
        for q in 0..4 {
            put(&mut a, base + c[q] + j * n_a, s_a[q]);
        }
        put(&mut a, base + n + 2 * n_a + n_b + j, 1.0);
        row += 1;
    }
    let (cmin, cmax) = if type_a > 1 { (1, n_a - 2) } else { (0, n_a - 1) };
    let (rmin, rmax) = if type_b > 1 { (1, n_b - 2) } else { (0, n_b - 1) };
    for r in 0..n_b {
        if r == rmin || r == rmax {
            continue;
        }
        for c in 0..n_a {
            if c == cmin || c == cmax {
                continue;
            }
            let base = row * cols;
            put(&mut a, base + r * n_a + c, 1.0);
            put(&mut a, base + r * n_a + cmin, -0.5);
            put(&mut a, base + r * n_a + cmax, -0.5);
            put(&mut a, base + rmin * n_a + c, -0.5);
            put(&mut a, base + rmax * n_a + c, -0.5);
            put(&mut a, base + rmin * n_a + cmin, 0.25);
            put(&mut a, base + rmin * n_a + cmax, 0.25);
            put(&mut a, base + rmax * n_a + cmin, 0.25);
            put(&mut a, base + rmax * n_a + cmax, 0.25);
            row += 1;
        }
    }
    let solved = si_math::algebra::resolve_linear_system(&mut a, rows as i16, cols as i16, n as i16);
    for (k, v) in a.iter().enumerate() {
        m.set_f32(a_addr + 4 * k as u32, *v);
    }

    let mut patch = 0u32;
    if solved {
        // the boundary keys, in the order of the four sides
        let bd = m.si_calloc(per as u32, 12);
        let mut at = 0u32;
        let mut side = |m: &mut Mem, s: u32, reversed: bool| {
            let cnt = m.i16(s + 0x10) as i32;
            for q in 0..cnt {
                let idx = if reversed { cnt - 1 - q } else { q };
                let key = m.u32(s + 0x14).wrapping_add((idx * 0x30) as u32);
                for c in 0..3 {
                    let v = m.f32(key + 0x0c + 4 * c);
                    m.set_f32(bd + at + 4 * c, v);
                }
                at += 12;
            }
        };
        side(m, sp[0], false);
        side(m, sp[3], flag3 != 0);
        side(m, sp[1], flag1 != 0);
        side(m, sp[2], flag2 != 0);
        let pa = si_hrc::image::patch::patch_allocate(m);
        patch = pa.addr();
        if patch != 0 {
            m.set_u32(patch + 4, type_b);
            m.set_u32(patch + 8, type_a);
            m.set_i16(patch + 0x0c, n_b as i16);
            m.set_i16(patch + 0x0e, n_a as i16);
            m.set_i16(patch + 0x10, m.i16(sp[1] + 8));
            m.set_i16(patch + 0x12, m.i16(sp[0] + 8));
            m.set_f32(patch + 0x14, tension_b);
            m.set_f32(patch + 0x18, tension_a);
            m.set_u16(patch + 0x1c, m.u16(sp[1] + 0x20));
            m.set_u16(patch + 0x1e, m.u16(sp[0] + 0x20));
        }
        si_spline::patch_eval_image::patch_update_matrix(m, Ptr32::<Patch>::new(patch));
        let pts = m.si_calloc((n_b * n_a) as u32, 0x10);
        let tags = m.si_calloc((n_b * n_a) as u32, 2);
        m.set_u32(patch + 0x1a4, pts);
        m.set_u32(patch + 0x1a8, tags);
        for g in 0..n {
            m.set_i16(tags + 2 * g as u32, 0);
            let pt = pts + 0x10 * g as u32;
            for c in 0..3 {
                m.set_f32(pt + 4 * c, 0.0);
            }
            m.set_f32(pt + 12, 1.0);
            for j in 0..per {
                let w = a[(g * cols + n + j) as usize];
                for c in 0..3u32 {
                    // the x product stays in the register unrounded (`fst`), y and z go through a float
                    let full = f80(w) * f80(m.f32(bd + 12 * j as u32 + 4 * c));
                    let prod = if c == 0 { full } else { f80(full.to_f32()) };
                    let v = (prod + f80(m.f32(pt + 4 * c))).to_f32();
                    m.set_f32(pt + 4 * c, v);
                }
            }
        }
        m.free(bd);
    }
    m.free(a_addr);
    dispose_all(m, cells);
    let node_p = Ptr32::<Model>::new(node);
    if new_node {
        setup::add_definition(m, node_p, 1, patch);
    } else {
        if m.u32(node + 0x48) != m.u32(m.u32(node + 0x38)) || m.i16(node + 0x0c) != 0 {
            si_spline::patch_image::patch_update_surface(m, Ptr32::<Patch>::new(patch));
        }
        defs::substitute_idefinition(m, &mut ModelSide(o), node_p, 0, patch);
    }
    m.set_u32(node + 0x30, 1);
    m.set_u32(node_cell, node);
    m.set_sp(sp0);
    1
}

/// The update of a four-sided effect node [soft 0x0819fa08 SI_fourSidedUpdate]: `upd` the update
/// node (the model at +8, the data at +0x24, the four relations from +0x1c, 0x28 bytes each). The
/// node is made again from its four curve models; a refusal is shown, and the relations dropped.
pub fn four_sided_update(m: &mut Mem, o: &mut dyn Outside, upd: u32) {
    let model = m.u32(upd + 8);
    if model == 0 {
        return;
    }
    let data = m.u32(upd + 0x24);
    let ptrs = m.si_malloc(0x10);
    let rels = m.u32(upd + 0x1c);
    for i in 0..4u32 {
        let rel = rels.wrapping_add(0x28 * i);
        if m.i16(rel + 0x0c) == 0 {
            m.free(ptrs);
            return;
        }
        let mi = m.u32(rel + 8);
        m.set_u32(ptrs + 4 * i, mi);
        if mi == 0 || m.u32(mi + 0x48) == 0 {
            return;
        }
        let kind = m.i32(mi + 0x30);
        if kind != 6 && kind != 9 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_NO_FACE, 2]);
            o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[mi, 0xfd]);
            m.free(ptrs);
            return;
        }
    }
    let sp = m.sp();
    let cell = m.local(4);
    m.set_u32(cell, model);
    let buf = m.local(80);
    let first = m.u32(ptrs);
    let kind = m.i32(first + 0x30);
    let ok: i16;
    if kind == 6 {
        ok = create_four_sided_patch(m, o, ptrs, cell, buf) as i16;
    } else if kind == 9 {
        if data == 0 {
            m.set_sp(sp);
            m.free(ptrs);
            return;
        }
        let dcell = m.local(4);
        m.set_u32(dcell, data);
        // [soft 0x0819d954 si_createFourSidedNurbs], ported in `four_nurbs` (a halt of the image
        // counts as an answer of 0, the update then shows `buf`).
        ok = super::four_nurbs::create_four_sided_nurbs(m, o, ptrs, cell, buf, 0, dcell).unwrap_or(0) as i16;
    } else {
        ok = 0;
    }
    if ok != 0 {
        let node = Ptr32::<Model>::new(m.u32(cell));
        touch::touch_geometry(m, &mut ModelS(o), node);
        let gl = graf_list(m);
        let gn = si_view::graflist::find_gl_data(m, gl, Ptr32::new(node.addr()), 2);
        si_view::func::update_gn_func(m, &mut ViewS(o), gn);
        m.set_sp(sp);
        m.free(ptrs);
    } else {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buf, 3]);
        o.call(m, SLEEP, &[1]);
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MSG_DELETED, 2]);
        for i in 0..4u32 {
            let mi = m.u32(ptrs + 4 * i);
            o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[mi, 0xfd]);
        }
        o.call(m, SLEEP, &[1]);
        m.set_sp(sp);
        m.free(ptrs);
    }
}
