// SPDX-License-Identifier: MIT
//! `cross.c`: the skin of a row of spline models (a polygon mesh by `meshSkinFromSpline`, a patch by `skin2patch`),
//! the model it makes (`SI_createSkinFromSpline`) and the update of a skinning relation (`SI_skinningUpdate`).
//! Finding 0034, section "geomupd6".
//!
//! The skinning record (`SI_SkinData`): shorts at +0 (closed), +4 (the count of curves), +8 (the number of steps per
//! segment of a spline skinned to a mesh), +0xc (end caps of a mesh), +0xe (0: a mesh, else a patch / NURBS surface),
//! words at +0x10 (the spline type of a patch) and +0x18 (a patch flag).
//!
//! Everything works on `si-mem`'s image. A routine of the original that is not ported is called through
//! [`si_mem::Outside`], written `name (0xADDRESS)`. The editor's status line, relation clean-up and model dispose are
//! hooks.

use super::{touch_geometry, update_display_node, SI_ED_RELAX_REL_FROM_DEL_MODEL, SI_MDL_STATUS_MESSAGES, SI_STATUS_DISPLAY_MESSAGE, SLEEP};
use crate::extrusion::{ModelSide, SplineSide};
use si_mem::{Mem, Outside, Ptr32, Routine};
use si_model::mesh_image::{self, off as mo};
use si_model::model_image::{defs, setup};
use si_nurbs::image::Halt;
use si_types::model::Model;
use si_x87::{x, F80};

/// `SI_patchUpdateSurface (0x0847cee4)`: ported in `si-spline`; a hook here, as the model code calls it.
pub const SI_PATCH_UPDATE_SURFACE: Routine = Routine::new("SI_patchUpdateSurface", 0x0847_cee4);
/// `SI_modelDispose (0x08461920)`, not ported (a hook).
pub const SI_MODEL_DISPOSE: Routine = Routine::new("SI_modelDispose", 0x0846_1920);

/// The text at 0x08687250: the name of the model a skin is made in ("skin").
const NAME_SKIN: u32 = 0x0868_7250;

/// A float through the x87 unit (a load and a store: a signalling NaN comes out quiet).
fn through_x87(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// The spline a skin model holds: its own definition for a spline model (kind 6), the first spline of a face (kind 2,
/// the record at +0x48 holds the spline from +4).
fn skin_spline(m: &Mem, model: u32) -> u32 {
    if m.i32(model + 0x30) == 2 {
        m.u32(model + 0x48).wrapping_add(4)
    } else {
        m.u32(model + 0x48)
    }
}

// ------------------------------------------------------------------------------------------ meshSkinFromSpline

/// [soft 0x0814370c meshSkinFromSpline] `(models, steps, count, caps, closed)`: the polygon mesh that skins the first
/// `count` spline models of `models` (each made linear with `steps` steps per segment, `|steps|` or 1, and carried by
/// the model's matrix at +0x1c4): one row of vertices per spline, one quad per segment between two rows, the last row
/// joined to the first when `closed` is not 0 (rows `count - 1 + closed`), and, with `caps` set (and neither `closed`
/// nor an open first spline model of kind 6), the two end polygons. Answers the mesh, or 0 when it cannot be made.
/// `steps` is taken as 1 when the first spline is not regular (the word at +0xc is 0).
/// The original divides by `count` and by the vertices per row: a zero there is a fault, a `Halt` here.
pub fn mesh_skin_from_spline(m: &mut Mem, o: &mut dyn Outside, models: u32, steps: i16, count: i16, caps: i16, closed: i16) -> Result<u32, Halt> {
    let mut caps = caps;
    let first = m.u32(models);
    let spline = skin_spline(m, first);
    let segments = si_spline::intersect_image::spline_nb_segment(m, Ptr32::new(spline));
    let mut b: i16 = steps;
    if b < 0 {
        b = b.wrapping_neg();
    }
    if b == 0 {
        b = 1;
    }
    if m.i32(spline + 0xc) == 0 {
        b = 1;
    }
    let mut per_row = segments.wrapping_mul(b as i32);
    let mesh = mesh_image::mesh_allocate(m);
    if mesh == 0 {
        return Ok(0);
    }
    let rows = count as i32;
    let extra = closed as i32;
    let vertices = si_hrc::image::geom::vertex_allocate(m, rows.wrapping_mul(per_row.wrapping_add(1)));
    m.set_u32(mesh + mo::VERTICES, vertices);
    let mut made: i32 = 0;
    let mut k: i32 = 0;
    while k < rows {
        let model = m.u32(models.wrapping_add(4 * k as u32));
        let sp = skin_spline(m, model);
        let lin = si_spline::convert_image::spline_convert_to_linear(m, &mut SplineSide(o), Ptr32::new(sp), 1, b as i32, 0.0);
        si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), lin, m.u32(models.wrapping_add(4 * k as u32)).wrapping_add(0x1c4));
        k += 1;
        let n = m.u16(lin.addr() + 0x10) as i32;
        let mut i = 0;
        while i < n {
            let key = m.u32(lin.addr() + 0x14).wrapping_add(0x30u32.wrapping_mul(i as u32));
            let at = m.u32(mesh + mo::VERTICES).wrapping_add(0x14u32.wrapping_mul(made as u32));
            // three floats through the x87 unit
            for (from, to) in [(0xcu32, 0u32), (0x10, 4), (0x14, 8)] {
                let v = through_x87(m.f32(key + from));
                m.set_f32(at + to, v);
            }
            made = made.wrapping_add(1);
            i += 1;
        }
        let slot = m.local(4);
        m.set_u32(slot, lin.addr());
        si_spline::intersect_image::spline_dispose(m, slot);
        m.release(4);
    }
    m.set_i32(mesh + mo::NB_VERTICES, made);
    // a skin from a spline model (kind 6) whose spline is open: the join of its ends is not made
    let open_first = if m.i32(first + 0x30) == 6 { (m.i16(m.u32(first + 0x48) + 8) == 0) as i32 } else { 0 };
    if closed != 0 || open_first != 0 {
        caps = 0;
    }
    let mut polygons: i32 = 0;
    let cursor_start;
    let nb_rows = rows.wrapping_sub(1).wrapping_add(extra);
    if caps == 0 {
        let poly = si_hrc::image::geom::polygon_allocate(m, per_row.wrapping_sub(open_first.wrapping_sub(1)).wrapping_mul(nb_rows));
        m.set_u32(mesh + mo::POLYGONS, poly);
        cursor_start = poly;
    } else {
        let poly = si_hrc::image::geom::polygon_allocate(m, per_row.wrapping_sub(open_first.wrapping_sub(1)).wrapping_mul(nb_rows).wrapping_add(2));
        m.set_u32(mesh + mo::POLYGONS, poly);
        // the two end polygons: the first runs back over the first row, the second forward over the last
        for (which, last_row) in [(0u32, false), (0x18, true)] {
            let p = poly + which;
            m.set_i16(p + mo::POLYGON_NB_NODES, per_row as i16);
            let u = m.u16(p + mo::POLYGON_NB_NODES) as i32;
            let size = (u as u32).wrapping_sub(((u.wrapping_sub(1) & 3) - 3) as u32);
            let nodes = m.si_calloc(size, 0x24);
            m.set_u32(p + mo::POLYGON_NODES, nodes);
            let mut i = 0;
            let mut back = per_row;
            while i < per_row {
                let v = if last_row { i.wrapping_add(rows.wrapping_sub(1).wrapping_mul(per_row)) } else { back = back.wrapping_sub(1); back };
                m.set_i32(nodes.wrapping_add(0x24u32.wrapping_mul(i as u32)), v);
                i += 1;
            }
        }
        cursor_start = poly + 0x30;
        polygons = 2;
    }
    if open_first != 0 {
        per_row = per_row.wrapping_add(1);
    }
    let mut cursor = cursor_start;
    let mut i = 0;
    while i < nb_rows {
        let next_row = i + 1;
        let lim = per_row.wrapping_sub(open_first);
        let mut j = 0;
        while j < lim {
            if rows == 0 {
                return Err(Halt::Refused("meshSkinFromSpline: a division by the count of curves"));
            }
            let following = (next_row.wrapping_rem(rows)).wrapping_mul(per_row);
            let jn = j + 1;
            if per_row == 0 {
                return Err(Halt::Refused("meshSkinFromSpline: a division by the vertices of a row"));
            }
            let wrapped = jn.wrapping_rem(per_row);
            mesh_image::rebuild::polygon_initialize(
                m,
                cursor,
                &[j.wrapping_add(per_row.wrapping_mul(i)), wrapped.wrapping_add(per_row.wrapping_mul(i)), wrapped.wrapping_add(following), j.wrapping_add(following)],
            );
            cursor = cursor.wrapping_add(0x18);
            polygons = polygons.wrapping_add(1);
            j = jn;
        }
        i = next_row;
    }
    m.set_i32(mesh + mo::NB_POLYGONS, polygons);
    mesh_image::rebuild::update(m, &mut ModelSide(o), mesh);
    mesh_image::obj::mesh_init_vertex_colors(m, mesh);
    Ok(mesh)
}

// ------------------------------------------------------------------------------------------------ skin2patch

/// The constants of `skin2patch`: 0.5 (0x0864a230), 0.333333 (0x0864a234), 0.666667 (0x0864a238), -2.0 (0x0864a23c),
/// 4.0 (0x0864a240) and 6.0 (0x0864a244), the floats the image holds.
const HALF: f32 = 0.5;
const THIRD: f32 = f32::from_bits(0x3eaa_aa9f);
const TWO_THIRDS: f32 = f32::from_bits(0x3f2a_aab0);
const MINUS_TWO: f32 = -2.0;
const FOUR: f32 = 4.0;
const SIX: f32 = 6.0;
const ONE_BITS: u32 = 0x3f80_0000;

/// A control point (x, y or z) of the key `j` of the spline model `model`: the spline at +0x48, its key array at +0x14,
/// 0x30 bytes a key, the position from +0xc.
fn key_at(m: &Mem, model: u32, j: u32, comp: u32) -> u32 {
    m.u32(m.u32(model + 0x48) + 0x14).wrapping_add(0x30u32.wrapping_mul(j)).wrapping_add(0xc + 4 * comp)
}

/// The key `j` of the spline model `model` as an x87 value per axis.
fn key_x87(m: &Mem, model: u32, j: u32) -> [F80; 3] {
    [x(m.f32(key_at(m, model, j, 0))), x(m.f32(key_at(m, model, j, 1))), x(m.f32(key_at(m, model, j, 2)))]
}

/// A point of the patch (four floats) at `index`: the x, y, z (an x87 value stored as a float) and the weight 1.0.
fn put_point(m: &mut Mem, points: u32, index: u32, v: [F80; 3]) {
    let at = points.wrapping_add(index.wrapping_mul(0x10));
    m.set_f32(at, v[0].to_f32());
    m.set_f32(at + 4, v[1].to_f32());
    m.set_f32(at + 8, v[2].to_f32());
    m.set_u32(at + 0xc, ONE_BITS);
}

/// The point `cur + d * k` (every operation at 80 bits, `sign` -1 for `cur - d * k`).
fn along(cur: [F80; 3], d: [F80; 3], k: f32, minus: bool) -> [F80; 3] {
    let mut r = cur;
    for c in 0..3 {
        let step = d[c] * k;
        r[c] = if minus { cur[c] - step } else { step + cur[c] };
    }
    r
}

/// The ghost row of a type 3 patch: `2a - b` for two curves, `(2a - 2b) + c` for more, per key `j` (every operation at 80
/// bits, the point stored as floats with the weight 1.0).
fn ghost_point(m: &Mem, a: u32, b: u32, c: Option<u32>, j: u32) -> [F80; 3] {
    let (ka, kb) = (key_x87(m, a, j), key_x87(m, b, j));
    let mut r = [F80::ZERO; 3];
    for i in 0..3 {
        r[i] = match c {
            None => (ka[i] + ka[i]) - kb[i],
            Some(c) => ((ka[i] + ka[i]) - (kb[i] + kb[i])) + key_x87(m, c, j)[i],
        };
    }
    r
}

/// [soft 0x08143f60 skin2patch] `(models, count, spline_type, closed, flag)`: the patch that skins the first `count`
/// (at least 2, else 0) spline models of `models`, a row of control points per curve and per kind of skin:
/// type 0 the curves' own points; type 1 (Bezier) two points are added between each curve and the next, a third
/// off (1/3 and 2/3 of the way; the ends continue the last and first segment when `closed` is 0); type 2 (B-spline,
/// interpolating) a linear system solved for the points (`SI_resolveLinearSystem`), `count` rows when closed, `count + 2`
/// otherwise; type 3 (Catmull-Rom) the curves' points with a row added in front and one behind when open (twice the
/// first curve less the second, and the same at the end) and the patch's +0x14 set to 0.5. Another type leaves the
/// patch with the rows and points `SI_patchAllocate` gave it. Each curve is taken from a copy of the model's spline made
/// in a scratch model (a Bezier spline, marked at +0xc as 1, expanded by `SI_bezierExpand`, its step flag at +8 set to 1
/// for type 1 and restored), carried by the model's matrix; the scratch models are disposed of at the end
/// (`SI_modelDispose`, a hook; the array that holds them is not freed). A model of a kind other than 2 and 6 takes
/// the spline of the model before it (the first one: the register the caller left, 0 here).
pub fn skin2patch(m: &mut Mem, o: &mut dyn Outside, models: u32, count: i16, spline_type: i32, closed: i16, flag: i32) -> u32 {
    if count < 2 {
        return 0;
    }
    let n = count as i32;
    let arr = m.si_malloc((n as u32).wrapping_mul(4));
    let mut sp: u32 = 0;
    let mut last: u32 = 0;
    let mut i: i32 = 0;
    while i < n {
        let model = m.u32(models.wrapping_add(4 * i as u32));
        if m.i32(model + 0x30) == 2 {
            sp = m.u32(model + 0x48).wrapping_add(4);
        } else if m.i32(model + 0x30) == 6 {
            sp = m.u32(model + 0x48);
        }
        let copy;
        if m.i32(sp + 0xc) == 1 {
            let saved = m.u16(sp + 8);
            if spline_type == 1 {
                m.set_u16(sp + 8, 1);
            }
            copy = si_spline::convert_image::spline_bezier_expand(m, Ptr32::new(sp)).addr();
            m.set_u16(sp + 8, saved);
            m.set_u16(copy + 8, saved);
        } else {
            copy = si_hrc::image::spline::spline_copy(m, sp).addr();
        }
        let matrix = m.u32(models.wrapping_add(4 * i as u32)).wrapping_add(0x1c4);
        si_spline::convert_image::spline_multiply_definition(m, &mut SplineSide(o), Ptr32::new(copy), matrix);
        let scratch = si_hrc::image::node::model_allocate(m, 0);
        m.set_u32(arr.wrapping_add(4 * i as u32), scratch.addr());
        setup::add_definition(m, scratch, 6, copy);
        last = copy;
        i += 1;
    }
    let patch = si_hrc::image::patch::patch_allocate(m).addr();
    if patch != 0 {
        m.set_u16(patch + 0xe, m.u16(last + 0x10));
        m.set_u32(patch + 8, m.u32(last + 0xc));
        m.set_u16(patch + 0x12, m.u16(last + 8));
        let w = through_x87(m.f32(last + 0x1c));
        m.set_f32(patch + 0x18, w);
        m.set_u16(patch + 0x1e, m.u16(last + 0x20));
        m.set_i16(patch + 0x10, closed);
        m.set_i32(patch + 4, spline_type);
        m.set_i16(patch + 0x1c, flag as i16);
        let model_of = |m: &Mem, k: i32| m.u32(arr.wrapping_add(4 * k as u32));
        let cols = |m: &Mem| m.u16(patch + 0xe) as u32;
        if spline_type == 1 {
            m.set_u16(patch + 0xc, (n.wrapping_mul(3)) as u16);
            let points = m.si_calloc(m.u16(patch + 0xc) as u32 * m.u16(patch + 0xe) as u32, 0x10);
            m.set_u32(patch + 0x1a4, points);
            let mut i = 0;
            while i < n {
                let mut j: u32 = 0;
                let mut c = cols(m);
                while c != 0 && (j as i32) < c as i32 {
                    let here = model_of(m, i);
                    let base = c.wrapping_mul(3).wrapping_mul(i as u32).wrapping_add(j);
                    let pts = m.u32(patch + 0x1a4);
                    let cur = key_x87(m, here, j);
                    // the point itself: three floats through the x87 unit
                    put_point(m, pts, base, cur);
                    let last_curve = i == n - 1;
                    if closed == 0 {
                        let next = model_of(m, if last_curve { i - 1 } else { i + 1 });
                        let nx = key_x87(m, next, j);
                        let mut d = [nx[0] - cur[0], nx[1] - cur[1], nx[2] - cur[2]];
                        let row1 = base.wrapping_add(c);
                        put_point(m, pts, row1, along(cur, d, THIRD, last_curve));
                        let (from, from_x) = if last_curve {
                            let first = model_of(m, 0);
                            let second = model_of(m, 1);
                            let f = key_x87(m, first, j);
                            let s = key_x87(m, second, j);
                            d = [s[0] - f[0], s[1] - f[1], s[2] - f[2]];
                            (f, true)
                        } else {
                            (cur, false)
                        };
                        let row2 = row1.wrapping_add(cols(m));
                        let r = if from_x { along(from, d, THIRD, true) } else { along(from, d, TWO_THIRDS, false) };
                        put_point(m, pts, row2, r);
                    } else {
                        let next = model_of(m, if last_curve { 0 } else { i + 1 });
                        let nx = key_x87(m, next, j);
                        let d = [nx[0] - cur[0], nx[1] - cur[1], nx[2] - cur[2]];
                        let row1 = base.wrapping_add(cols(m));
                        put_point(m, pts, row1, along(cur, d, THIRD, false));
                        let row2 = row1.wrapping_add(cols(m));
                        put_point(m, pts, row2, along(cur, d, TWO_THIRDS, false));
                    }
                    j += 1;
                    c = cols(m);
                }
                i += 1;
            }
        } else if spline_type < 2 {
            if spline_type == 0 {
                m.set_u16(patch + 0xc, n as u16);
                let points = m.si_calloc(m.u16(patch + 0xc) as u32 * m.u16(patch + 0xe) as u32, 0x10);
                m.set_u32(patch + 0x1a4, points);
                let mut index: u32 = 0;
                let mut i = 0;
                if m.i16(patch + 0xc) != 0 {
                    while {
                        let here = model_of(m, i);
                        i += 1;
                        let mut j: u32 = 0;
                        if m.i16(patch + 0xe) != 0 {
                            while {
                                let pts = m.u32(patch + 0x1a4);
                                let at = pts.wrapping_add(index.wrapping_mul(0x10));
                                for comp in 0..3 {
                                    let v = through_x87(m.f32(key_at(m, here, j, comp)));
                                    m.set_f32(at + 4 * comp, v);
                                }
                                m.set_u32(at + 0xc, ONE_BITS);
                                index = index.wrapping_add(1);
                                j += 1;
                                (j as i32) < m.u16(patch + 0xe) as i32
                            } {}
                        }
                        i < m.u16(patch + 0xc) as i32
                    } {}
                }
            }
        } else if spline_type == 2 {
            let rows = if closed == 0 { n + 2 } else { n };
            m.set_i16(patch + 0xc, rows as i16);
            let width = rows + n;
            let points = m.si_calloc(m.u16(patch + 0xc) as u32 * m.u16(patch + 0xe) as u32, 0x10);
            m.set_u32(patch + 0x1a4, points);
            let total = rows.wrapping_mul(width);
            let mat = m.si_malloc((total as u32).wrapping_mul(4));
            if mat != 0 {
                let mut a = vec![0.0f32; total.max(0) as usize];
                // the original clears the matrix word by word: the same through the image below
                let put = |a: &mut Vec<f32>, at: i32, v: f32| {
                    // a position outside the matrix is the original's write past its block; the port stops there
                    if at >= 0 && (at as usize) < a.len() {
                        a[at as usize] = v;
                    }
                };
                if closed == 0 {
                    put(&mut a, 0, 1.0);
                    put(&mut a, 1, MINUS_TWO);
                    put(&mut a, 2, 1.0);
                    let mut i = 1;
                    let mut row = width;
                    if 0 < n {
                        while {
                            let e = i + row;
                            put(&mut a, e - 1, 1.0);
                            put(&mut a, e, FOUR);
                            put(&mut a, e + 1, 1.0);
                            put(&mut a, e + n + 1, SIX);
                            i += 1;
                            row += width;
                            i <= n
                        } {}
                    }
                    let e = (n + 1) * width + n;
                    put(&mut a, e - 1, 1.0);
                    put(&mut a, e, MINUS_TWO);
                    put(&mut a, e + 1, 1.0);
                } else {
                    let mut i = 0;
                    let mut start = 0;
                    while i < n {
                        let base;
                        if i == 0 {
                            put(&mut a, n - 1, 1.0);
                            base = 0;
                        } else {
                            put(&mut a, start + i - 1, 1.0);
                            base = i * width;
                        }
                        put(&mut a, i + base, FOUR);
                        if i == n - 1 {
                            put(&mut a, base, 1.0);
                        } else {
                            put(&mut a, i + base + 1, 1.0);
                        }
                        put(&mut a, base + i + n, SIX);
                        start += width;
                        i += 1;
                    }
                }
                // the matrix lives in the image: the solver works on a copy of it
                for (k, v) in a.iter().enumerate() {
                    m.set_f32(mat + 4 * k as u32, *v);
                }
                let solved = si_math::algebra::resolve_linear_system(&mut a, rows as i16, width as i16, rows as i16);
                for (k, v) in a.iter().enumerate() {
                    m.set_f32(mat + 4 * k as u32, *v);
                }
                if solved && 0 < rows {
                    let mut out = m.u32(patch + 0x1a4);
                    let mut r = 0;
                    while r < rows {
                        let mut c: i32 = 0;
                        if m.i16(patch + 0xe) != 0 {
                            while {
                                m.set_f32(out, 0.0);
                                m.set_f32(out + 4, 0.0);
                                m.set_f32(out + 8, 0.0);
                                m.set_u32(out + 0xc, ONE_BITS);
                                let mut src = mat.wrapping_add(4u32.wrapping_mul((r.wrapping_mul(width).wrapping_add(rows)) as u32));
                                let mut k = 0;
                                while k < n {
                                    let coef = m.f32(src);
                                    let model = model_of(m, k);
                                    for comp in 0..3u32 {
                                        let p = m.f32(key_at(m, model, c as u32, comp));
                                        let acc = m.f32(out + 4 * comp);
                                        let v = (x(coef) * p + acc).to_f32();
                                        m.set_f32(out + 4 * comp, v);
                                    }
                                    src = src.wrapping_add(4);
                                    k += 1;
                                }
                                out = out.wrapping_add(0x10);
                                c += 1;
                                c < m.u16(patch + 0xe) as i32
                            } {}
                        }
                        r += 1;
                    }
                }
                m.free(mat);
            }
        } else if spline_type == 3 {
            let rows = if closed == 0 { count.wrapping_add(2) } else { count };
            m.set_i16(patch + 0xc, rows);
            let points = m.si_calloc(m.u16(patch + 0xc) as u32 * m.u16(patch + 0xe) as u32, 0x10);
            m.set_u32(patch + 0x1a4, points);
            let mut cursor = points;
            if closed == 0 {
                let (a, b) = (model_of(m, 0), model_of(m, 1));
                let c = if count == 2 { None } else { Some(model_of(m, 2)) };
                let mut j: u32 = 0;
                if m.i16(patch + 0xe) != 0 {
                    while {
                        put_point(m, cursor, 0, ghost_point(m, a, b, c, j));
                        cursor = cursor.wrapping_add(0x10);
                        j += 1;
                        (j as i32) < m.u16(patch + 0xe) as i32
                    } {}
                }
            }
            let mut i = 0;
            while i < n {
                let here = model_of(m, i);
                let mut j: u32 = 0;
                if m.i16(patch + 0xe) != 0 {
                    while {
                        for comp in 0..3u32 {
                            let v = through_x87(m.f32(key_at(m, here, j, comp)));
                            m.set_f32(cursor + 4 * comp, v);
                        }
                        j += 1;
                        cursor = cursor.wrapping_add(0x10);
                        (j as i32) < m.u16(patch + 0xe) as i32
                    } {}
                }
                i += 1;
            }
            if closed == 0 {
                let (a, b) = (model_of(m, n - 1), model_of(m, n - 2));
                let c = if count == 2 { None } else { Some(model_of(m, n - 3)) };
                let mut j: u32 = 0;
                if m.i16(patch + 0xe) != 0 {
                    while {
                        put_point(m, cursor, 0, ghost_point(m, a, b, c, j));
                        cursor = cursor.wrapping_add(0x10);
                        j += 1;
                        (j as i32) < m.u16(patch + 0xe) as i32
                    } {}
                }
            }
            m.set_f32(patch + 0x14, through_x87(HALF));
        }
        let cells = m.u16(patch + 0xc) as u32 * m.u16(patch + 0xe) as u32;
        let shorts = m.si_calloc(cells, 2);
        m.set_u32(patch + 0x1a8, shorts);
        let mut k = cells;
        while k != 0 {
            m.set_u16(m.u32(patch + 0x1a8).wrapping_add(k.wrapping_mul(2)).wrapping_sub(2), 0);
            k -= 1;
        }
        si_spline::patch_eval_image::patch_update_matrix(m, Ptr32::new(patch));
    }
    let mut left = n;
    let mut at = arr;
    while left != 0 {
        o.call(m, SI_MODEL_DISPOSE, &[at, 0]);
        at = at.wrapping_add(4);
        left -= 1;
    }
    patch
}

// ------------------------------------------------------------------------------------------ SI_createSkinFromSpline

/// [soft 0x08142eb4 si_createSkinFromSpline] `(models, model_slot, skin)` makes the skin of the spline models `models`
/// described by the skinning record `skin`: a polygon mesh (kind 4, `meshSkinFromSpline` with the record's steps at +8,
/// count at +4, caps at +0xc and closed at +0) when the short at +0xe is 0, else a patch (kind 1, `skin2patch` with the
/// count, the type at +0x10, closed and the flag at +0x18). The model is the one in `model_slot`, or a new one named "skin"
/// (`SI_modelAllocate`, the skin added as its definition) when the slot is null, otherwise the model's first definition
/// is replaced (`SI_modelSubstituteIDefinition`; a patch skin first brings its surface up to date, `SI_patchUpdateSurface`,
/// when the model's current definition is not its first or the short at +0xc is set). The model's kind (+0x30) is set
/// to the skin's whether or not it could be made, and the slot gets the model. Answers 1, or 0 when no skin was made.
pub fn create_skin_from_spline(m: &mut Mem, o: &mut dyn Outside, models: u32, model_slot: u32, skin: u32) -> Result<u16, Halt> {
    let count = m.i16(skin + 4);
    let steps = m.i16(skin + 8);
    let closed = m.i16(skin);
    let caps = m.i16(skin + 0xc);
    let mut result: u16 = 1;
    let mut model = m.u32(model_slot);
    let fresh = model == 0;
    if fresh {
        model = si_hrc::image::node::model_allocate(m, NAME_SKIN).addr();
    }
    let node = Ptr32::<Model>::new(model);
    if m.i16(skin + 0xe) == 0 {
        let mesh = mesh_skin_from_spline(m, o, models, steps, count, caps, closed)?;
        if mesh == 0 {
            result = 0;
        } else if fresh {
            setup::add_definition(m, node, 4, mesh);
        } else {
            defs::substitute_idefinition(m, &mut ModelSide(o), node, 0, mesh);
        }
        m.set_i32(model + 0x30, 4);
    } else {
        let patch = skin2patch(m, o, models, count, m.i32(skin + 0x10), closed, m.i32(skin + 0x18));
        if patch == 0 {
            result = 0;
        } else if fresh {
            setup::add_definition(m, node, 1, patch);
        } else {
            if m.u32(model + 0x48) != m.u32(m.u32(model + 0x38)) || m.i16(model + 0xc) != 0 {
                o.call(m, SI_PATCH_UPDATE_SURFACE, &[patch]); // a hook: the surface is the model code's
            }
            defs::substitute_idefinition(m, &mut ModelSide(o), node, 0, patch);
        }
        m.set_i32(model + 0x30, 1);
    }
    m.set_u32(model_slot, model);
    Ok(result)
}

// ------------------------------------------------------------------------------------------------- SI_skinningUpdate

/// `SI_modelChangeName (0x0846db14)`: the rename a definition added to a model ends with, a hook for the scratch models
/// of the compatible copy (as in `si-nurbs`, which logs it the same way).
const SI_MODEL_CHANGE_NAME: Routine = Routine::new("SI_modelChangeName", 0x0846_db14);

/// The body of `SI_modelAddDefinition (0x08467038)` with the rename as a hook (the port in `setup` renames itself): the
/// geometry is appended, the type set with the first one, the working definition follows the first, the model is renamed.
fn add_definition_hooked(m: &mut Mem, o: &mut dyn Outside, model: u32, kind: u32, geometry: u32) {
    let first;
    if m.i16(model + 0x34) == 0 {
        m.set_u32(model + 0x30, kind);
        let array = m.si_malloc(4);
        m.set_u32(model + 0x38, array);
        first = 0;
    } else {
        first = m.u32(m.u32(model + 0x38));
        let array = m.si_realloc(m.u32(model + 0x38), m.u16(model + 0x34) as u32 * 4 + 4);
        m.set_u32(model + 0x38, array);
    }
    let slot = m.u32(model + 0x38).wrapping_add(m.u16(model + 0x34) as u32 * 4);
    m.set_u32(slot, geometry);
    let n = m.i16(model + 0x34).wrapping_add(1);
    m.set_i16(model + 0x34, n);
    if m.u32(model + 0x48) == first {
        let head = m.u32(m.u32(model + 0x38));
        m.set_u32(model + 0x48, head);
    }
    o.call(m, SI_MODEL_CHANGE_NAME, &[model, m.u32(model + 4)]);
}

/// The relation is deleted: the message (`SI_statusDisplayMessage`, level 3, the text made by `sprintf` in a 100-byte
/// buffer on the stack), `sleep(1)`, the modeller's message line (`SI_mdlStatusMessages`) and the editor's clean-up of the
/// relation (`SI_edRelaxRelFromDelModel(model, 0xfc)`): all of them hooks.
fn relation_deleted(m: &mut Mem, o: &mut dyn Outside, buffer: u32, text: &str, model: u32) {
    m.put_cstr(buffer, text.as_bytes());
    o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buffer, 3]);
    o.call(m, SLEEP, &[1]);
    o.call(m, SI_MDL_STATUS_MESSAGES, &[]);
    o.call(m, SI_ED_RELAX_REL_FROM_DEL_MODEL, &[model, 0xfc]);
}

/// The spline of a skin model for the checks of [`skinning_update`]: a spline model (kind 6) holds its own, a face the
/// first of its record (+4), the answer being 0 for another kind.
fn check_spline(m: &Mem, model: u32) -> u32 {
    if m.i32(model + 0x30) == 6 {
        m.u32(model + 0x48)
    } else {
        m.u32(model + 0x48).wrapping_add(4)
    }
}

/// [soft 0x08143208 SI_skinningUpdate] `(node, time)`: the update function of a skinning relation. `node` is the
/// relation's update node: +8 the model that is made (the target), +0x1c the table of the inputs (0x28 bytes each: +8 the
/// model, +0xc a short that is 0 when the input is not connected), +0x24 the skinning record (+4 the number of curves).
/// Nothing happens when the target or the record is missing.
///
/// The inputs are checked one by one against the one before. NURBS curves (a curve model, kind 9, or a face, kind 2, whose
/// flag at +0x84 is 0, its curves at +0x80): each curve needs as many points as its degree (the record's first two words,
/// "N points is NOT enough"), the two curves the same degree ("The curves do not have ALL the same degree"), and a different
/// point count makes the curves be made compatible below. Splines (kind 6, or a face whose flag is set): each needs two
/// keys at least and as many as the one before. A failed check shows the message (level 3), sleeps a second, shows the
/// modeller's messages and relaxes the relation (`SI_edRelaxRelFromDelModel(input, 0xfc)`), and ends the update; an input
/// that is missing (null model, definition, or a flag of 0) ends it silently. The skin is then made: from splines
/// ([`create_skin_from_spline`]), from NURBS curves (`SI_createSkinFromNurbs`, in scratch copies made compatible
/// (`SI_nurbsCrvMakeCompatible`) when the counts differ) or, for another kind, the target is only touched
/// (`SI_modelTouchGeometry`); the display node of the result is brought up to date. The editor's routines, `sleep` and
/// `SI_modelDispose` are hooks. The array of the models is not freed (as in the original), nor are the scratch arrays of
/// the compatible copy but one. A message that reads the curve of an earlier NURBS input when there was none (a pure spline
/// list; address 0 in the original) is a `Halt`.
pub fn skinning_update(m: &mut Mem, o: &mut dyn Outside, node: u32, time: f32) -> Result<(), Halt> {
    // the frame's locals (the target's slot, the message buffer) go with the return, whichever it is
    let sp = m.sp();
    let r = skinning_update_frame(m, o, node, time);
    m.set_sp(sp);
    r
}

fn skinning_update_frame(m: &mut Mem, o: &mut dyn Outside, node: u32, _time: f32) -> Result<(), Halt> {
    let mut flag2 = false;
    let mut curve: u32 = 0;
    let target = m.u32(node + 8);
    if target == 0 {
        return Ok(());
    }
    let skin = m.u32(node + 0x24);
    if skin == 0 {
        return Ok(());
    }
    let slot = m.local(4);
    m.set_u32(slot, target);
    let buffer = m.local(100);
    let n = m.u32(skin + 4);
    let arr = m.si_malloc(n.wrapping_shl(2));
    let mut idx: i32 = 0;
    let mut at: u32 = 0;
    while idx < n as i32 {
        let table = m.u32(node + 0x1c);
        if m.i16(table.wrapping_add(0xc).wrapping_add(at)) == 0 {
            if arr != 0 {
                m.free(arr);
            }
            return Ok(());
        }
        let mdl = m.u32(table.wrapping_add(8).wrapping_add(at));
        m.set_u32(arr.wrapping_add(4u32.wrapping_mul(idx as u32)), mdl);
        if mdl == 0 {
            return Ok(());
        }
        let geom = m.u32(mdl + 0x48);
        if geom == 0 {
            return Ok(());
        }
        if idx >= 1 {
            let prev = m.u32(arr.wrapping_add(4u32.wrapping_mul((idx - 1) as u32)));
            let kind = m.i32(mdl + 0x30);
            let nurbs_pair = if kind == 9 {
                curve = geom;
                Some(if m.i32(prev + 0x30) == 9 { m.u32(prev + 0x48) } else { m.u32(m.u32(prev + 0x48) + 0x80) })
            } else if kind == 2 && m.i16(geom + 0x84) == 0 {
                curve = m.u32(geom + 0x80);
                Some(if m.i32(prev + 0x30) == 9 { m.u32(prev + 0x48) } else { m.u32(m.u32(prev + 0x48) + 0x80) })
            } else {
                None
            };
            if let Some(before) = nurbs_pair {
                let a = m.i32(curve + 4);
                let b = m.i32(curve);
                if b <= a && m.i32(before) <= m.i32(before + 4) {
                    if b != m.i32(before) {
                        relation_deleted(m, o, buffer, "Your relation is deleted: The curves do not have ALL the same degree.\n", m.u32(arr.wrapping_add(4u32.wrapping_mul(idx as u32))));
                        return Ok(());
                    }
                    if a != m.i32(before + 4) {
                        flag2 = true;
                    }
                } else {
                    let text = format!("Your relation is deleted. {a} points is NOT enough. Minimum is {b}. ");
                    relation_deleted(m, o, buffer, &text, m.u32(arr.wrapping_add(4u32.wrapping_mul(idx as u32))));
                    return Ok(());
                }
            } else {
                // splines
                let this = m.u32(arr.wrapping_add(4u32.wrapping_mul(idx as u32)));
                let (cur, before);
                if m.i32(this + 0x30) == 6 {
                    cur = m.u32(this + 0x48);
                } else if m.i32(this + 0x30) != 2 || m.i16(m.u32(this + 0x48) + 0x84) == 0 {
                    // neither a spline model nor a face of splines: no check
                    at = at.wrapping_add(0x28);
                    idx += 1;
                    continue;
                } else {
                    cur = m.u32(this + 0x48).wrapping_add(4);
                }
                before = check_spline(m, prev);
                if idx == 0 && m.u16(before + 0x10) < 2 {
                    // not reached: an input after the first has an index above 0
                    let text = format!("Your relation is deleted. {} points is NOT enough. Minimum is 2. ", m.u16(before + 0x10));
                    relation_deleted(m, o, buffer, &text, m.u32(arr));
                    m.set_i32(skin + 4, m.i32(skin + 4).wrapping_sub(1));
                    return Ok(());
                }
                if m.u16(cur + 0x10) < 2 {
                    if curve == 0 {
                        return Err(Halt::Refused("SI_skinningUpdate: the message reads the curve of an earlier input, and there is none"));
                    }
                    let text = format!("Your relation is deleted. {} points is NOT enough. Minimum is {}. ", m.u16(cur + 0x10), m.i32(curve));
                    relation_deleted(m, o, buffer, &text, this);
                    return Ok(());
                }
                if m.u16(cur + 0x10) != m.u16(before + 0x10) {
                    relation_deleted(m, o, buffer, "Your relation is deleted. Curves do not have the same number of points.", this);
                    return Ok(());
                }
            }
        }
        at = at.wrapping_add(0x28);
        idx += 1;
    }
    let first = m.u32(arr);
    let first_kind = m.i32(first + 0x30);
    if first_kind == 6 || (first_kind == 2 && m.i16(m.u32(first + 0x48) + 0x84) != 0) {
        create_skin_from_spline(m, o, arr, slot, skin)?;
    } else if first_kind == 9 || (first_kind == 2 && m.i16(m.u32(first + 0x48) + 0x84) == 0) {
        if flag2 {
            let models = m.si_calloc(n, 4);
            let curves = m.si_calloc(n, 4);
            let mut i: u32 = 0;
            while (i as i32) < n as i32 {
                let model = m.u32(arr.wrapping_add(4 * i));
                if m.i32(model + 0x30) == 9 {
                    curve = m.u32(model + 0x48);
                } else if m.i32(model + 0x30) == 2 && m.i16(m.u32(model + 0x48) + 0x84) == 0 {
                    curve = m.u32(m.u32(model + 0x48) + 0x80);
                }
                let copy = si_nurbs::image::crv_copy(m, curve);
                si_nurbs::image::crv_apply_matrix(m, copy, m.u32(arr.wrapping_add(4 * i)).wrapping_add(0x1c4), 0);
                let scratch = si_hrc::image::node::model_allocate(m, 0);
                m.set_u32(models.wrapping_add(4 * i), scratch.addr());
                add_definition_hooked(m, o, scratch.addr(), 9, copy);
                m.set_u32(curves.wrapping_add(4 * i), m.u32(scratch.addr() + 0x48));
                i += 1;
            }
            si_nurbs::blend::nurbs_crv_make_compatible(m, o, curves, n as i32);
            si_nurbs::geom13::create_skin_from_nurbs(m, o, models, slot, skin);
            let mut left = n as i32;
            let mut p = models;
            while left > 0 {
                o.call(m, SI_MODEL_DISPOSE, &[p, 0]);
                p = p.wrapping_add(4);
                left -= 1;
            }
            if curves != 0 {
                m.free(curves);
            }
        } else {
            si_nurbs::geom13::create_skin_from_nurbs(m, o, arr, slot, skin);
        }
    } else {
        touch_geometry(m, o, m.u32(slot));
    }
    let shown = m.u32(slot);
    update_display_node(m, o, shown);
    Ok(())
}
