// SPDX-License-Identifier: MIT
//! A definition's points by number, whatever its kind, on the image.
//!
//! The geometry records are read by named offsets (below), not through `si-types`'
//! layouts: these routines index the point arrays by byte counts of their own, and the
//! offsets are exactly what they read.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::{fmov, fmov64, x, F80};

use super::{at as field, callee, g, mat, mx, state, through_pointer, tree, Outside};

/// The offsets these routines read, by record.
mod at {
    /// `SI_Patch`: the control points (16 bytes each) and their tags (2 bytes each).
    pub const PATCH_POINTS: u32 = 0x1a4;
    pub const PATCH_TAGS: u32 = 0x1a8;
    /// `SI_Patch`: the two 16-bit point counts.
    pub const PATCH_NU: u32 = 0x0c;
    pub const PATCH_NV: u32 = 0x0e;
    /// `SI_Face`: the outline's key count (16 bits) and keys, the number of holes or of
    /// NURBS curves (16 bits), the holes (`SI_Spline`, 0x74 bytes), the NURBS curves
    /// (`__SI_NurbsCrv`, 100 bytes), and the 16-bit word that is zero for a NURBS face.
    pub const FACE_NB_KEYS: u32 = 0x14;
    pub const FACE_KEYS: u32 = 0x18;
    pub const FACE_NB_HOLES: u32 = 0x78;
    pub const FACE_HOLES: u32 = 0x7c;
    pub const FACE_CURVES: u32 = 0x80;
    pub const FACE_SPLINE: u32 = 0x84;
    /// `SI_Spline`: the key count (16 bits) and the keys (0x30 bytes: the position at
    /// +0xc, a flag byte at +0x2c).
    pub const SPLINE_NB_KEYS: u32 = 0x10;
    pub const SPLINE_KEYS: u32 = 0x14;
    /// `SI_Mesh`: the vertices (0x14 bytes: a flag byte at +0x12) and their count.
    pub const MESH_VERTICES: u32 = 0x00;
    pub const MESH_NB_VERTICES: u32 = 0x14;
    /// `__SI_NurbsCrv`: the point count and the points (0x24 bytes: four doubles and a
    /// flag byte at +0x20).
    pub const CRV_NB_POINTS: u32 = 0x04;
    pub const CRV_POINTS: u32 = 0x2c;
    /// `SI_NurbsSrf`: the point count and the points.
    pub const SRF_NB_POINTS: u32 = 0x48;
    pub const SRF_POINTS: u32 = 0x54;
}

/// A NURBS point (x, y, z, w as doubles) as a new block of three floats, each coordinate
/// times 1/w: the quotient is kept at 80 bits, each product rounded to float. The block
/// comes from `SI_calloc(1, 12)` after the quotient is formed.
fn nurbs_point(m: &mut Mem, point: u32) -> u32 {
    let r = x(1.0f32) / x(m.f64(point + 0x18));
    let out = m.si_calloc(1, 0xc);
    let v = (x(m.f64(point)) * r).to_f32();
    m.set_f32(out, v);
    let v = (x(m.f64(point + 8)) * r).to_f32();
    m.set_f32(out + 4, v);
    let v = (r * x(m.f64(point + 0x10))).to_f32();
    m.set_f32(out + 8, v);
    out
}

/// The position of point `index` of a definition of `kind`
/// [soft 0x0847402c SI_modelGetVertexPtr]: a pointer to three floats, and at `tagged` a
/// 16-bit 1 when bit 0 of the point's flag is set, else 0.
///
/// For a patch, a mesh, a spline and a spline face the pointer is into the definition
/// (a face's points are the outline's keys, then each hole's). For a NURBS curve or
/// surface, and for a face of NURBS curves (the curves' points one after the other), it
/// is a **new block** the caller owns (`nurbs_point`). Null for any other kind and for
/// a face index beyond its contours. The index is not held against the counts of the
/// other kinds.
pub fn get_vertex_ptr(m: &mut Mem, definition: u32, kind: i32, index: i32, tagged: u32) -> u32 {
    m.set_i16(tagged, 0);
    let mut index = index;
    match kind {
        1 => {
            let tags = m.u32(definition + at::PATCH_TAGS);
            if m.u8(tags.wrapping_add((index as u32).wrapping_mul(2))) & 1 != 0 {
                m.set_i16(tagged, 1);
            }
            (index as u32).wrapping_mul(0x10).wrapping_add(m.u32(definition + at::PATCH_POINTS))
        }
        2 => {
            if m.i16(definition + at::FACE_SPLINE) == 0 {
                let mut i = 0;
                while i < m.i16(definition + at::FACE_NB_HOLES) as i32 {
                    let curve = m.u32(definition + at::FACE_CURVES).wrapping_add(i as u32 * 100);
                    let count = m.i32(curve + at::CRV_NB_POINTS);
                    if index < count {
                        let off = (index as u32).wrapping_mul(0x24);
                        let points = m.u32(curve + at::CRV_POINTS);
                        if m.u8(points.wrapping_add(off).wrapping_add(0x20)) & 1 != 0 {
                            m.set_i16(tagged, 1);
                        }
                        let points = m.u32(curve + at::CRV_POINTS);
                        return nurbs_point(m, points.wrapping_add(off));
                    }
                    index = index.wrapping_sub(count);
                    i += 1;
                }
                0
            } else {
                let outline = m.u16(definition + at::FACE_NB_KEYS) as u32;
                if index < outline as i32 {
                    let key = m.u32(definition + at::FACE_KEYS).wrapping_add((index as u32).wrapping_mul(0x30));
                    if m.u8(key.wrapping_add(0x2c)) & 1 != 0 {
                        m.set_i16(tagged, 1);
                    }
                    return key.wrapping_add(0xc);
                }
                let mut base = outline;
                let mut i = 0;
                while i < m.i16(definition + at::FACE_NB_HOLES) as i32 {
                    let hole = m.u32(definition + at::FACE_HOLES).wrapping_add(i as u32 * 0x74);
                    let upto = (m.u16(hole + at::SPLINE_NB_KEYS) as u32).wrapping_add(base);
                    if index < upto as i32 {
                        let off = (index as u32).wrapping_sub(base).wrapping_mul(0x30);
                        let keys = m.u32(hole + at::SPLINE_KEYS);
                        if m.u8(keys.wrapping_add(off).wrapping_add(0x2c)) & 1 != 0 {
                            m.set_i16(tagged, 1);
                        }
                        return off.wrapping_add(m.u32(hole + at::SPLINE_KEYS)).wrapping_add(0xc);
                    }
                    base = upto;
                    i += 1;
                }
                0
            }
        }
        4 => {
            let vertex = m.u32(definition + at::MESH_VERTICES).wrapping_add((index as u32).wrapping_mul(0x14));
            if m.u8(vertex.wrapping_add(0x12)) & 1 != 0 {
                m.set_i16(tagged, 1);
            }
            vertex
        }
        6 => {
            let key = m.u32(definition + at::SPLINE_KEYS).wrapping_add((index as u32).wrapping_mul(0x30));
            if m.u8(key.wrapping_add(0x2c)) & 1 != 0 {
                m.set_i16(tagged, 1);
            }
            key.wrapping_add(0xc)
        }
        9 | 10 => {
            let array = if kind == 9 { at::CRV_POINTS } else { at::SRF_POINTS };
            let off = (index as u32).wrapping_mul(0x24);
            let points = m.u32(definition + array);
            if m.u8(points.wrapping_add(off).wrapping_add(0x20)) & 1 != 0 {
                m.set_i16(tagged, 1);
            }
            let points = m.u32(definition + array);
            nurbs_point(m, points.wrapping_add(off))
        }
        _ => 0,
    }
}

/// The message of [`elem_get_nb_vertices`] for a kind it does not know (a string of
/// `.data`; only its address is passed on).
const NB_VERTICES_MESSAGE: u32 = 0x086d_6334;

/// The number of points of a scene element [soft 0x084769b4 SI_elemGetNbVertices].
/// `element` is an `__SI_Elem` whose word at +4 is the object; `chapter` says what it is,
/// by the chapter numbers of `SI_cid`:
///
/// - the chapter at +8: the word at +4 of the object;
/// - the chapter at +0x14: the product of the three 16-bit counts at +2, +4 and +6 of the
///   object (signed);
/// - the models (+0x20) and the shapes (+0x54): the object is a node, and the count is
///   that of its stored definition `number` (0 when the number is outside the array): a
///   patch's two counts multiplied; a face's outline keys and all its holes' keys, or the
///   points of all its NURBS curves; a mesh's vertices; a spline's keys; for kind 7 the
///   word at +0x4c of the working definition; a NURBS curve's or surface's points; 0 for
///   the kinds −1, 0, 5 and 8. Any other kind is reported (`SI_error(9, message, kind)`),
///   and when that answers non-zero the result is −1.
///
/// Any other chapter: 0.
pub fn elem_get_nb_vertices(m: &mut Mem, o: &mut dyn Outside, element: u32, chapter: u32, number: i32) -> u32 {
    let mut count = 0u32;
    let mut failed = 0u32;
    let cid = m[g::SI_CID];
    if chapter == cid[2] {
        count = m.u32(m.u32(element + 4) + 4);
    } else if chapter == cid[5] {
        let object = m.u32(element + 4);
        count = (m.i16(object + 2) as i32).wrapping_mul(m.i16(object + 4) as i32).wrapping_mul(m.i16(object + 6) as i32) as u32;
    } else if chapter == cid[8] || chapter == cid[0x15] {
        let node: si_mem::Ptr32<si_types::model::Model> = si_mem::Ptr32::new(m.u32(element + 4));
        let mut number = number;
        if number < 0 || m[node].nb_defs as i32 <= number {
            number = 0;
        }
        let kind = m[node].kind;
        let stored = |m: &Mem| m.u32(m[node].defs.addr().wrapping_add(number as u32 * 4));
        match kind {
            1 => {
                let d = stored(m);
                count = m.u16(d + at::PATCH_NU) as u32 * m.u16(d + at::PATCH_NV) as u32;
            }
            2 => {
                let d = stored(m);
                let contours = m.i16(d + at::FACE_NB_HOLES) as i32;
                if m.i16(d + at::FACE_SPLINE) == 0 {
                    let curves = m.u32(d + at::FACE_CURVES);
                    for i in 0..contours.max(0) as u32 {
                        count = count.wrapping_add(m.u32(curves.wrapping_add(i * 100).wrapping_add(at::CRV_NB_POINTS)));
                    }
                } else {
                    count = m.u16(d + at::FACE_NB_KEYS) as u32;
                    let holes = m.u32(d + at::FACE_HOLES);
                    for i in 0..contours.max(0) as u32 {
                        count = count.wrapping_add(m.u16(holes.wrapping_add(i * 0x74).wrapping_add(at::SPLINE_NB_KEYS)) as u32);
                    }
                }
            }
            4 => count = m.u32(stored(m) + at::MESH_NB_VERTICES),
            6 => count = m.u16(stored(m) + at::SPLINE_NB_KEYS) as u32,
            7 => count = m.u32(m[node].definition.addr() + 0x4c),
            9 => count = m.u32(stored(m) + at::CRV_NB_POINTS),
            10 => count = m.u32(stored(m) + at::SRF_NB_POINTS),
            -1 | 0 | 5 | 8 => {}
            _ => failed = o.call(m, &callee::SI_ERROR, &[9, NB_VERTICES_MESSAGE, kind as u32]),
        }
    }
    if failed != 0 {
        count = u32::MAX;
    }
    count
}

/// One of a node's definitions by the "geometry type" the element routines pass
/// [soft 0x08473fc4 SI_modelGetData] (a second port; the store form is
/// `crate::node_copy`): 0 the original definition, or without one the first stored
/// definition (null without an array); 1 the stored definition `number`, for a number
/// from 1 to the count less one (null otherwise, and without an array); 2 the working
/// definition; null for any other type.
pub fn get_data(m: &Mem, node: Ptr32<Model>, geom_type: i32, number: i32) -> u32 {
    match geom_type {
        0 => {
            let original = m[node].org_def.addr();
            if original != 0 {
                return original;
            }
            let array = m[node].defs.addr();
            if array == 0 {
                0
            } else {
                m.u32(array)
            }
        }
        1 => {
            if number <= 0 || number >= m[node].nb_defs as i32 {
                return 0;
            }
            let array = m[node].defs.addr();
            if array == 0 {
                0
            } else {
                m.u32(array.wrapping_add((number as u32).wrapping_mul(4)))
            }
        }
        2 => m[node].definition.addr(),
        _ => 0,
    }
}

/// The messages of the four element accessors (strings of `.data`; only their addresses
/// are passed on to `SI_error`): for an index outside the element's points (error 0x3b),
/// for a cluster without its model (0x1c), for a shape number that names no definition
/// (0x1b), and for a kind whose points cannot be reached (9; the set routines have one
/// for each place they find that out).
struct Messages {
    index: u32,
    cluster: u32,
    shape: u32,
}

/// What the four element accessors do before they touch a point: the element's point
/// count (`SI_elemGetNbVertices` with number 0), the index held against it, and for the
/// chapter at `SI_cid` + 8 (a cluster: its object lists point numbers at +0xc) the step
/// to the model's element (`SI_elemdictFindRelatedElem`, relation 700) and the point's
/// number there. `Err` carries what the routine is to answer: `None` when there is no
/// count (each routine has its own answer), else the answer of `SI_error`.
fn locate(m: &mut Mem, o: &mut dyn Outside, dict: u32, element: u32, chapter: u32, index: i32, msg: &Messages) -> Result<(u32, i32), Option<u32>> {
    let count = elem_get_nb_vertices(m, o, element, chapter, 0);
    if count == u32::MAX {
        return Err(None);
    }
    if index < 0 || index >= count as i32 {
        return Err(Some(o.call(m, &callee::SI_ERROR, &[0x3b, msg.index])));
    }
    let cid = m[g::SI_CID];
    if chapter != cid[2] {
        return Ok((element, index));
    }
    let object = m.u32(element + 4);
    let (second, first) = (m.local(4), m.local(4));
    let model = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 0, 0, 0x2bc, cid[8], first, second]);
    m.release(8);
    if model == 0 {
        return Err(Some(o.call(m, &callee::SI_ERROR, &[0x1c, msg.cluster])));
    }
    let numbers = m.u32(object + 0xc);
    Ok((model, m.i32(numbers.wrapping_add((index as u32).wrapping_mul(4)))))
}

/// The chapters whose element is a node: the models, the clusters (after [`locate`] has
/// stepped to the model) and the shapes.
fn of_a_node(m: &Mem, chapter: u32) -> bool {
    let cid = m[g::SI_CID];
    chapter == cid[8] || chapter == cid[2] || chapter == cid[0x15]
}

/// The position of point `index` of a scene element, as three floats at `out`
/// [soft 0x08476b2c SI_elemGetVertex]; the answer is `out`, or a new 12-byte block
/// (`SI_calloc`) when `out` is null, or null after an error.
///
/// For the chapter at `SI_cid` + 0x14 (a lattice: its object has the points, 12 bytes
/// each, at +0x14) the point is copied from there. For a node it is the point of the
/// definition [`get_data`] answers for `geom_type` and `number`, through
/// [`get_vertex_ptr`] (which also sets `tagged`); the block that routine makes for a
/// NURBS point is freed here. The floats are moved through the unit, then stored as
/// words. An index outside the element's points, a cluster without its model and a shape
/// number without a definition are reported (`SI_error`) and answer null whatever
/// `SI_error` answers; a kind without points answers null only when `SI_error` answers
/// non-zero, and otherwise stores three words nobody set.
#[allow(clippy::too_many_arguments)]
pub fn elem_get_vertex(m: &mut Mem, o: &mut dyn Outside, dict: u32, element: u32, chapter: u32, geom_type: i32, number: i32, index: i32, out: u32, tagged: u32) -> u32 {
    let msg = Messages { index: 0x086d_634c, cluster: 0x086d_6360, shape: 0x086d_6374 };
    let (element, index) = match locate(m, o, dict, element, chapter, index, &msg) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut failed = 0;
    let mut point = [0u32; 3];
    let mut set = false;
    if chapter == m[g::SI_CID][5] {
        let object = m.u32(element + 4);
        let off = (index as u32).wrapping_mul(12);
        for (i, w) in point.iter_mut().enumerate() {
            *w = fmov(m.f32(m.u32(object + 0x14).wrapping_add(off).wrapping_add(4 * i as u32))).to_bits();
        }
        set = true;
    } else if of_a_node(m, chapter) {
        let node: Ptr32<Model> = Ptr32::new(m.u32(element + 4));
        let def = get_data(m, node, geom_type, number);
        if geom_type == 1 && def == 0 {
            o.call(m, &callee::SI_ERROR, &[0x1b, msg.shape]);
            return 0;
        }
        let kind = m[node].kind;
        let p = get_vertex_ptr(m, def, kind, index, tagged);
        if p == 0 {
            let kind = m[node].kind;
            failed = o.call(m, &callee::SI_ERROR, &[9, 0x086d_6388, kind as u32]);
        } else {
            for (i, w) in point.iter_mut().enumerate() {
                *w = fmov(m.f32(p + 4 * i as u32)).to_bits();
            }
            set = true;
            let kind = m[node].kind;
            if (kind.wrapping_sub(9) as u32) <= 1 || (kind == 2 && m.i16(def + at::FACE_SPLINE) == 0) {
                m.free(p);
            }
        }
    }
    if failed != 0 {
        return 0;
    }
    if !set {
        // the original stores its three local words without having set them
        crate::env::stale::note();
    }
    let out = if out == 0 { m.si_calloc(1, 0xc) } else { out };
    for (i, w) in point.iter().enumerate() {
        m.set_u32(out + 4 * i as u32, *w);
    }
    out
}

/// After a lattice point was set: the lattice's element must be related to a model by
/// relation 0xd7, or some element by 0xd2 (asked with a null element); neither is
/// reported as error 0x1d. The answer is the routine's.
fn lattice_point_set(m: &mut Mem, o: &mut dyn Outside, dict: u32, element: u32, message: u32) -> u32 {
    let models = m[g::SI_CID][8];
    let (second, first) = (m.local(4), m.local(4));
    let mut related = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 0, 0, 0xd7, models, first, second]);
    if related == 0 {
        related = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, 0, 0, 0, 0xd2, models, first, second]);
    }
    m.release(8);
    if related != 0 {
        0
    } else {
        o.call(m, &callee::SI_ERROR, &[0x1d, message])
    }
}

/// A double as the two stack words of an argument, low word first.
fn double_words(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// `SI_faceGlobalToLocalIndex(face, index, &curve, &point, 1)` for a face of NURBS
/// curves: the curve's address and the point's number in it, when that number is below
/// the curve's count. The two results start as 0 on the stack.
fn face_curve_point(m: &mut Mem, o: &mut dyn Outside, face: u32, index: i32) -> Option<(u32, i32)> {
    let (point_at, curve_at) = (m.local(4), m.local(4));
    m.set_u32(curve_at, 0);
    m.set_u32(point_at, 0);
    o.call(m, &callee::SI_FACE_GLOBAL_TO_LOCAL_INDEX, &[face, index as u32, curve_at, point_at, 1]);
    let curve = (m.u32(curve_at)).wrapping_mul(100).wrapping_add(m.u32(face + at::FACE_CURVES));
    let point = m.i32(point_at);
    m.release(8);
    (point < m.i32(curve + at::CRV_NB_POINTS)).then_some((curve, point))
}

/// Sets the position of point `index` of a scene element from the three floats at `p`
/// [soft 0x08476d70 SI_elemSetVertex]; the answer is 0 or an error's number: `SI_errno`
/// when the element has no count, else what `SI_error` answers.
///
/// A lattice's point is stored in its object (then see `lattice_point_set`). A node's
/// point by kind: a NURBS curve or surface through `SI_nurbsCrvMovePoint` and
/// `SI_nurbsSrfMovePoint` (the floats widened to doubles; a surface point keeps its
/// weight); a face of NURBS curves through `SI_faceGlobalToLocalIndex` and
/// `SI_nurbsCrvMovePoint`; anything else through [`get_vertex_ptr`]. The floats are moved
/// through the unit. Any other chapter answers 0.
#[allow(clippy::too_many_arguments)]
pub fn elem_set_vertex(m: &mut Mem, o: &mut dyn Outside, dict: u32, element: u32, chapter: u32, geom_type: i32, number: i32, index: i32, p: u32) -> u32 {
    let msg = Messages { index: 0x086d_639c, cluster: 0x086d_63b0, shape: 0x086d_63da };
    let (element, index) = match locate(m, o, dict, element, chapter, index, &msg) {
        Ok(v) => v,
        Err(None) => return m[g::SI_ERRNO] as u32,
        Err(Some(e)) => return e,
    };
    if chapter == m[g::SI_CID][5] {
        let object = m.u32(element + 4);
        let off = (index as u32).wrapping_mul(12);
        for i in 0..3u32 {
            let v = fmov(m.f32(p + 4 * i));
            let points = m.u32(object + 0x14);
            m.set_f32(points.wrapping_add(off).wrapping_add(4 * i), v);
        }
        return lattice_point_set(m, o, dict, element, 0x086d_63c4);
    }
    if !of_a_node(m, chapter) {
        return 0;
    }
    let node: Ptr32<Model> = Ptr32::new(m.u32(element + 4));
    let def = get_data(m, node, geom_type, number);
    if geom_type == 1 && def == 0 {
        return o.call(m, &callee::SI_ERROR, &[0x1b, msg.shape]);
    }
    let kind = m[node].kind;
    // the three floats as doubles, pushed last coordinate first
    let wide = |m: &Mem| {
        let z = double_words(x(m.f32(p + 8)).to_f64());
        let y = double_words(x(m.f32(p + 4)).to_f64());
        let xx = double_words(x(m.f32(p)).to_f64());
        [xx[0], xx[1], y[0], y[1], z[0], z[1]]
    };
    let store = |m: &mut Mem, to: u32| {
        for i in 0..3u32 {
            let v = fmov(m.f32(p + 4 * i));
            m.set_f32(to + 4 * i, v);
        }
    };
    let tag = m.local(4);
    let result = match kind {
        9 => {
            let c = wide(m);
            o.call(m, &callee::SI_NURBS_CRV_MOVE_POINT, &[def, index as u32, c[0], c[1], c[2], c[3], c[4], c[5]]);
            0
        }
        10 => {
            let points = m.u32(def + at::SRF_POINTS);
            let w = double_words(fmov64(m.f64(points.wrapping_add((index as u32).wrapping_mul(0x24)).wrapping_add(0x18))));
            let c = wide(m);
            o.call(m, &callee::SI_NURBS_SRF_MOVE_POINT, &[def, index as u32, c[0], c[1], c[2], c[3], c[4], c[5], w[0], w[1]]);
            0
        }
        2 if m.i16(def + at::FACE_SPLINE) == 0 => match face_curve_point(m, o, def, index) {
            Some((curve, point)) => {
                let c = wide(m);
                o.call(m, &callee::SI_NURBS_CRV_MOVE_POINT, &[curve, point as u32, c[0], c[1], c[2], c[3], c[4], c[5]]);
                0
            }
            None => {
                let kind = m[node].kind;
                o.call(m, &callee::SI_ERROR, &[9, 0x086d_6402, kind as u32])
            }
        },
        _ => {
            let to = get_vertex_ptr(m, def, kind, index, tag + 2);
            if to != 0 {
                store(m, to);
                0
            } else {
                let kind = m[node].kind;
                o.call(m, &callee::SI_ERROR, &[9, if kind == 2 { 0x086d_63ee } else { 0x086d_6416 }, kind as u32])
            }
        }
    };
    m.release(4);
    result
}

/// A point of a scene element as a control vertex at `cv`: x, y, z and the weight as
/// four doubles [soft 0x084770bc SI_edGetCtrlVertex]; the answer is 0 or what a callee
/// answers (`SI_error`, `SI_nurbsCrvGetCtrlVertex`, `SI_nurbsSrfGetCtrlVertex`).
///
/// A lattice point is widened to three doubles (the weight at `cv` + 0x18 is left as it
/// was). For a node the weight is set to 1.0 first; a NURBS curve, a NURBS surface and a
/// face of NURBS curves hand `cv` to the NURBS routine; any other kind widens the three
/// floats [`get_vertex_ptr`] points to (a block it made is not freed here: it makes none
/// for these kinds).
#[allow(clippy::too_many_arguments)]
pub fn ed_get_ctrl_vertex(m: &mut Mem, o: &mut dyn Outside, dict: u32, element: u32, chapter: u32, geom_type: i32, number: i32, index: i32, cv: u32) -> u32 {
    let msg = Messages { index: 0x086d_642a, cluster: 0x086d_643e, shape: 0x086d_6452 };
    let (element, index) = match locate(m, o, dict, element, chapter, index, &msg) {
        Ok(v) => v,
        Err(None) => return 0,
        Err(Some(e)) => return e,
    };
    if chapter == m[g::SI_CID][5] {
        let object = m.u32(element + 4);
        let off = (index as u32).wrapping_mul(12);
        for i in 0..3u32 {
            let v = x(m.f32(m.u32(object + 0x14).wrapping_add(off).wrapping_add(4 * i))).to_f64();
            m.set_f64(cv + 8 * i, v);
        }
        return 0;
    }
    if !of_a_node(m, chapter) {
        return 0;
    }
    let node: Ptr32<Model> = Ptr32::new(m.u32(element + 4));
    let def = get_data(m, node, geom_type, number);
    if geom_type == 1 && def == 0 {
        return o.call(m, &callee::SI_ERROR, &[0x1b, msg.shape]);
    }
    m.set_u32(cv + 0x18, 0);
    m.set_u32(cv + 0x1c, 0x3ff0_0000);
    let kind = m[node].kind;
    match kind {
        9 => o.call(m, &callee::SI_NURBS_CRV_GET_CTRL_VERTEX, &[def, index as u32, cv]),
        10 => o.call(m, &callee::SI_NURBS_SRF_GET_CTRL_VERTEX, &[def, index as u32, cv]),
        2 if m.i16(def + at::FACE_SPLINE) == 0 => match face_curve_point(m, o, def, index) {
            Some((curve, point)) => o.call(m, &callee::SI_NURBS_CRV_GET_CTRL_VERTEX, &[curve, point as u32, cv]),
            None => 0,
        },
        _ => {
            let tag = m.local(4);
            let kind = m[node].kind;
            let p = get_vertex_ptr(m, def, kind, index, tag + 2);
            m.release(4);
            if p == 0 {
                let kind = m[node].kind;
                o.call(m, &callee::SI_ERROR, &[9, 0x086d_6466, kind as u32])
            } else {
                for i in 0..3u32 {
                    let v = x(m.f32(p + 4 * i)).to_f64();
                    m.set_f64(cv + 8 * i, v);
                }
                0
            }
        }
    }
}

/// Sets a point of a scene element from the control vertex at `cv` (four doubles)
/// [soft 0x084772f4 SI_edSetCtrlVertex]; the answers are those of [`elem_set_vertex`].
///
/// As [`elem_set_vertex`] with doubles: a lattice point and a point reached through
/// [`get_vertex_ptr`] are the doubles rounded to `float`; a NURBS curve's point (also in
/// a face of curves) gets its weight stored into the point record before
/// `SI_nurbsCrvMovePoint` is called with x, y and z; a surface's point passes the weight
/// to `SI_nurbsSrfMovePoint`.
#[allow(clippy::too_many_arguments)]
pub fn ed_set_ctrl_vertex(m: &mut Mem, o: &mut dyn Outside, dict: u32, element: u32, chapter: u32, geom_type: i32, number: i32, index: i32, cv: u32) -> u32 {
    let msg = Messages { index: 0x086d_647a, cluster: 0x086d_648e, shape: 0x086d_64b8 };
    let (element, index) = match locate(m, o, dict, element, chapter, index, &msg) {
        Ok(v) => v,
        Err(None) => return m[g::SI_ERRNO] as u32,
        Err(Some(e)) => return e,
    };
    if chapter == m[g::SI_CID][5] {
        let object = m.u32(element + 4);
        let off = (index as u32).wrapping_mul(12);
        for i in 0..3u32 {
            let v = x(m.f64(cv + 8 * i)).to_f32();
            let points = m.u32(object + 0x14);
            m.set_f32(points.wrapping_add(off).wrapping_add(4 * i), v);
        }
        return lattice_point_set(m, o, dict, element, 0x086d_64a2);
    }
    if !of_a_node(m, chapter) {
        return 0;
    }
    let node: Ptr32<Model> = Ptr32::new(m.u32(element + 4));
    let def = get_data(m, node, geom_type, number);
    if geom_type == 1 && def == 0 {
        return o.call(m, &callee::SI_ERROR, &[0x1b, msg.shape]);
    }
    let kind = m[node].kind;
    // the doubles as pushed, last first, each moved through the unit
    let wide = |m: &Mem, count: u32| {
        let mut out = [0u32; 8];
        for i in (0..count).rev() {
            let w = double_words(fmov64(m.f64(cv + 8 * i)));
            out[2 * i as usize] = w[0];
            out[2 * i as usize + 1] = w[1];
        }
        out
    };
    // a NURBS curve's point gets the weight before it is moved
    let weight = |m: &mut Mem, curve: u32, point: i32| {
        let points = m.u32(curve + at::CRV_POINTS);
        let w = fmov64(m.f64(cv + 0x18));
        m.set_f64(points.wrapping_add((point as u32).wrapping_mul(0x24)).wrapping_add(0x18), w);
    };
    let tag = m.local(4);
    let result = match kind {
        9 => {
            weight(m, def, index);
            let c = wide(m, 3);
            o.call(m, &callee::SI_NURBS_CRV_MOVE_POINT, &[def, index as u32, c[0], c[1], c[2], c[3], c[4], c[5]]);
            0
        }
        10 => {
            let c = wide(m, 4);
            o.call(m, &callee::SI_NURBS_SRF_MOVE_POINT, &[def, index as u32, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
            0
        }
        2 if m.i16(def + at::FACE_SPLINE) == 0 => match face_curve_point(m, o, def, index) {
            Some((curve, point)) => {
                weight(m, curve, point);
                let c = wide(m, 3);
                o.call(m, &callee::SI_NURBS_CRV_MOVE_POINT, &[curve, point as u32, c[0], c[1], c[2], c[3], c[4], c[5]]);
                0
            }
            None => {
                let kind = m[node].kind;
                o.call(m, &callee::SI_ERROR, &[9, 0x086d_64e0, kind as u32])
            }
        },
        _ => {
            let to = get_vertex_ptr(m, def, kind, index, tag + 2);
            if to != 0 {
                for i in 0..3u32 {
                    let v = x(m.f64(cv + 8 * i)).to_f32();
                    m.set_f32(to + 4 * i, v);
                }
                0
            } else {
                let kind = m[node].kind;
                o.call(m, &callee::SI_ERROR, &[9, if kind == 2 { 0x086d_64cc } else { 0x086d_64f4 }, kind as u32])
            }
        }
    };
    m.release(4);
    result
}

/// A point through a matrix, in the register: `((x·m0j + y·m1j) + z·m2j) + m3j`.
fn through(p: [F80; 3], mm: &si_math::types::Matrix) -> [F80; 3] {
    let mut out = [F80::ZERO; 3];
    for (j, o) in out.iter_mut().enumerate() {
        *o = ((p[0] * mm[0][j] + p[1] * mm[1][j]) + p[2] * mm[2][j]) + mm[3][j];
    }
    out
}

/// A vector by the rows of a matrix, in the register: `(m_i0·a + m_i1·b) + m_i2·c`.
fn by_rows(v: [F80; 3], mm: &si_math::types::Matrix) -> [F80; 3] {
    let mut out = [F80::ZERO; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (x(mm[i][0]) * v[0] + x(mm[i][1]) * v[1]) + x(mm[i][2]) * v[2];
    }
    out
}

fn floats(m: &Mem, a: u32) -> [F80; 3] {
    [x(m.f32(a)), x(m.f32(a + 4)), x(m.f32(a + 8))]
}

/// Stores three words at `out`, or at a new 12-byte block (`SI_calloc`) when `out` is
/// null; the answer is where.
fn answer_vector(m: &mut Mem, out: u32, v: [f32; 3]) -> u32 {
    let out = if out == 0 { m.si_calloc(1, 0xc) } else { out };
    for (i, c) in v.iter().enumerate() {
        m.set_u32(out + 4 * i as u32, c.to_bits());
    }
    out
}

/// The tree a node hangs in, brought up to date: `SI_modelUpdateSonState(NULL, root, 1)`.
fn update_tree(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) -> Ptr32<Model> {
    let root = tree::root(m, node);
    state::update_son_state(m, o, Ptr32::NULL, root, 1);
    root
}

/// A position in the space of node `from` as a position in the space of node `to`
/// [soft 0x08477658 SI_elemConvertPos]; a null node stands for the world. The result is
/// at `out`, or at a new 12-byte block when `out` is null; the answer is where.
///
/// The trees of the nodes named are updated first (once when both hang in one tree).
/// The spaces are those the geometry lives in: `gmat` of each node's `latdep`. From a
/// node, the point goes through that matrix; to a node, through its inverse (taken into
/// a stack block: a matrix without an inverse leaves there what the stack had, counted
/// in [`crate::env::stale`]). With both, the point is rounded to `float` in between.
pub fn elem_convert_pos(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>, p: u32, out: u32) -> u32 {
    let inverse = m.local(0x40);
    let invert = |m: &mut Mem| {
        let space = m[to].latdep;
        if !mx::inverse(m, field!(space, gmat), inverse) {
            crate::env::stale::note();
        }
    };
    let rounded = |v: [F80; 3]| [v[0].to_f32(), v[1].to_f32(), v[2].to_f32()];
    let v = if from.is_null() {
        update_tree(m, o, to);
        invert(m);
        rounded(through(floats(m, p), &mat(m, inverse)))
    } else if to.is_null() {
        update_tree(m, o, from);
        let space = m[from].latdep;
        rounded(through(floats(m, p), &mat(m, field!(space, gmat))))
    } else {
        let a = tree::root(m, from);
        let b = tree::root(m, to);
        state::update_son_state(m, o, Ptr32::NULL, a, 1);
        if a != b {
            state::update_son_state(m, o, Ptr32::NULL, b, 1);
        }
        let space = m[from].latdep;
        let world = rounded(through(floats(m, p), &mat(m, field!(space, gmat))));
        invert(m);
        rounded(through([x(world[0]), x(world[1]), x(world[2])], &mat(m, inverse)))
    };
    m.release(0x40);
    answer_vector(m, out, v)
}

/// A direction in the space of node `from` as a direction in the space of node `to`
/// [soft 0x08477960 SI_elemConvertOri]; a null node stands for the world. The result is
/// at `out`, or at a new block; the answer is where.
///
/// The matrices are the nodes' own `grot` (not their `latdep`'s). From a node, the
/// vector goes through `grot` **as a point**, row 3 added; to a node, it is multiplied by
/// the rows of `grot` (the transpose, which inverts a rotation), without row 3. With
/// both, what comes out of the first step stays in the register, unrounded.
pub fn elem_convert_ori(m: &mut Mem, o: &mut dyn Outside, from: Ptr32<Model>, to: Ptr32<Model>, v: u32, out: u32) -> u32 {
    let r = if from.is_null() {
        update_tree(m, o, to);
        by_rows(floats(m, v), &mat(m, field!(to, grot)))
    } else if to.is_null() {
        update_tree(m, o, from);
        through(floats(m, v), &mat(m, field!(from, grot)))
    } else {
        let a = tree::root(m, from);
        let b = tree::root(m, to);
        state::update_son_state(m, o, Ptr32::NULL, a, 1);
        if a != b {
            state::update_son_state(m, o, Ptr32::NULL, b, 1);
        }
        let world = through(floats(m, v), &mat(m, field!(from, grot)));
        by_rows(world, &mat(m, field!(to, grot)))
    };
    answer_vector(m, out, [r[0].to_f32(), r[1].to_f32(), r[2].to_f32()])
}

/// A NURBS point as three floats: x, y and z each times 1/w, the quotient kept at 80
/// bits, each product rounded to `float`. The array's address is read again before each
/// coordinate.
fn nurbs_floats(m: &Mem, record: u32, points_at: u32, offset: u32) -> [f32; 3] {
    let p = m.u32(record + points_at).wrapping_add(offset);
    let r = x(1.0f32) / x(m.f64(p + 0x18));
    let px = (x(m.f64(p)) * r).to_f32();
    let p = m.u32(record + points_at).wrapping_add(offset);
    let py = (x(m.f64(p + 8)) * r).to_f32();
    let p = m.u32(record + points_at).wrapping_add(offset);
    let pz = (r * x(m.f64(p + 0x10))).to_f32();
    [px, py, pz]
}

/// Hands every control point of a node's **working** definition to a callback, in world
/// space [soft 0x08474390 SI_modelTraverseCVs]: `callback(&point, index, data)` with the
/// point three floats on the stack. The answer is 1, or 0 for a kind without points
/// (3, 8, anything outside 0 ..= 10) and for a spline whose type is not 0 to 3.
///
/// The matrix is a copy of `gmat` of the node's `latdep`; of the node itself when
/// `own_space` is not zero, and always for a null (kind 0) and a joint (kind 5), which
/// give one point: the origin through the matrix. A point is
/// `((x·m0j + y·m1j) + z·m2j) + m3j`, each coordinate rounded to `float` on its store.
///
/// The index counts from 0: over a patch's points, a mesh's vertices, a spline's keys
/// (positions only), a NURBS curve's or surface's points; over **all** contours of a face
/// of splines together (outline first), but **from 0 again for every curve** of a face of
/// NURBS curves. A metaball system (kind 7) is handed to
/// `si_metaballContourTraversePnts` with the matrix.
pub fn traverse_cvs(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, callback: u32, data: u32, own_space: i16) -> i16 {
    let kind = m[node].kind;
    let space = if kind != 0 && kind != 5 && own_space == 0 { m[node].latdep } else { node };
    let matrix = m.local(0x40);
    let point = m.local(12);
    mx::copy(m, field!(space, gmat), matrix);
    let def = m[node].definition.addr();
    let call = |m: &mut Mem, o: &mut dyn Outside, p: [F80; 3], index: i32| {
        let v = through(p, &mat(m, matrix));
        // the first and the third coordinate are stored before the second
        m.set_f32(point, v[0].to_f32());
        m.set_f32(point + 8, v[2].to_f32());
        m.set_f32(point + 4, v[1].to_f32());
        o.call(m, &through_pointer(callback), &[point, index as u32, data]);
    };
    let wide = |p: [f32; 3]| [x(p[0]), x(p[1]), x(p[2])];
    let done = match kind {
        0 | 5 => {
            call(m, o, [F80::ZERO; 3], 0);
            true
        }
        1 => {
            let mut p = m.u32(def + at::PATCH_POINTS);
            let count = (m.u16(def + at::PATCH_NU) as u32).wrapping_mul(m.u16(def + at::PATCH_NV) as u32) as i32;
            let mut i = 0;
            while i < count {
                call(m, o, floats(m, p), i);
                i += 1;
                p = p.wrapping_add(0x10);
            }
            true
        }
        4 => {
            let count = m.i32(def + at::MESH_NB_VERTICES);
            let mut i = 0;
            while i < count {
                let p = m.u32(def + at::MESH_VERTICES).wrapping_add((i as u32).wrapping_mul(0x14));
                call(m, o, floats(m, p), i);
                i += 1;
            }
            true
        }
        2 => {
            if m.i16(def + at::FACE_SPLINE) != 0 {
                let mut index = 0;
                if m.u8(def + at::FACE_NB_HOLES + 1) & 0x80 == 0 {
                    let mut hole: i32 = -1;
                    loop {
                        // (the outline is the `SI_Spline` inside the face, at +4)
                        let contour = if hole == -1 { def + 4 } else { (hole as u32).wrapping_mul(0x74).wrapping_add(m.u32(def + at::FACE_HOLES)) };
                        let mut key = m.u32(contour + at::SPLINE_KEYS);
                        hole += 1;
                        if m.u16(contour + at::SPLINE_NB_KEYS) != 0 {
                            let mut i = 0;
                            loop {
                                call(m, o, floats(m, key.wrapping_add(0xc)), index);
                                index += 1;
                                i += 1;
                                key = key.wrapping_add(0x30);
                                if i >= m.u16(contour + at::SPLINE_NB_KEYS) as i32 {
                                    break;
                                }
                            }
                        }
                        if hole >= m.i16(def + at::FACE_NB_HOLES) as i32 {
                            break;
                        }
                    }
                }
            } else if m.i16(def + at::FACE_NB_HOLES) > 0 {
                let mut curve: i32 = 0;
                loop {
                    let c = (curve as u32).wrapping_mul(100).wrapping_add(m.u32(def + at::FACE_CURVES));
                    curve += 1;
                    let mut i = 0;
                    while i < m.i32(c + at::CRV_NB_POINTS) {
                        let p = nurbs_floats(m, c, at::CRV_POINTS, (i as u32).wrapping_mul(0x24));
                        call(m, o, wide(p), i);
                        i += 1;
                    }
                    if curve >= m.i16(def + at::FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            }
            true
        }
        6 => {
            let kind_of_spline = m.i32(def + 0xc);
            if !(0..=3).contains(&kind_of_spline) {
                false
            } else {
                if m.u16(def + at::SPLINE_NB_KEYS) != 0 {
                    let mut i = 0;
                    loop {
                        let key = m.u32(def + at::SPLINE_KEYS).wrapping_add((i as u32).wrapping_mul(0x30));
                        call(m, o, floats(m, key.wrapping_add(0xc)), i);
                        i += 1;
                        if i >= m.u16(def + at::SPLINE_NB_KEYS) as i32 {
                            break;
                        }
                    }
                }
                true
            }
        }
        7 => {
            o.call(m, &callee::SI_METABALL_CONTOUR_TRAVERSE_PNTS, &[def, matrix, callback, data]);
            true
        }
        9 | 10 => {
            let (count_at, points_at) = if kind == 9 { (at::CRV_NB_POINTS, at::CRV_POINTS) } else { (at::SRF_NB_POINTS, at::SRF_POINTS) };
            let count = m.i32(def + count_at);
            let mut i = 0;
            while i < count {
                let p = nurbs_floats(m, def, points_at, (i as u32).wrapping_mul(0x24));
                call(m, o, wide(p), i);
                i += 1;
            }
            true
        }
        _ => false,
    };
    m.release(0x40 + 12);
    done as i16
}

/// The offsets of an `SI_Spline` the curve walks read beyond the keys: the 16-bit word
/// that is not zero for a closed spline (it adds three segments), the steps a segment
/// and the matrix of forward differences (`SI_splineUpdateMatrix`, finding 0020); in a
/// key, the 16-bit segment type (2: the Bézier segment ending here is a straight line).
mod curve {
    pub const CLOSED: u32 = 0x08;
    pub const TYPE: u32 = 0x0c;
    pub const STEPS: u32 = 0x20;
    pub const DIFFERENCES: u32 = 0x24;
    pub const KEY_SEGMENT: u32 = 0x2a;
}

/// One step of forward differences on the 4 × 4 block at `l`: row 3 += row 2, row 2 +=
/// row 1, row 1 += row 0, each element one float addition. Row 3 is then the next point.
fn forward_step(m: &mut Mem, l: u32) {
    for row in (1..4u32).rev() {
        for k in 0..4u32 {
            let to = l + 16 * row + 4 * k;
            let v = (x(m.f32(to)) + x(m.f32(to - 16))).to_f32();
            m.set_f32(to, v);
        }
    }
}

/// Three words copied from `from` to `to`.
fn copy3(m: &mut Mem, from: u32, to: u32) {
    for i in 0..3u32 {
        let w = m.u32(from + 4 * i);
        m.set_u32(to + 4 * i, w);
    }
}

/// The geometry block of a spline segment at `g` (rows of x, y, z, 1) and the block of
/// differences at `l`: both on the stack, the last column of `g` set to 1.0 once.
struct Segment {
    g: u32,
    l: u32,
}

impl Segment {
    fn new(m: &mut Mem) -> Segment {
        let g = m.local(0x40);
        let l = m.local(0x40);
        Segment { g, l }
    }

    fn ones(&self, m: &mut Mem) {
        for row in (0..4u32).rev() {
            m.set_f32(self.g + 16 * row + 12, 1.0);
        }
    }

    /// A Bézier segment between two keys: the first key's position and the point after
    /// it, the second key's point before it and its position.
    fn bezier(&self, m: &mut Mem, spline: u32, previous: u32, key: u32) {
        copy3(m, previous + 0xc, self.g);
        copy3(m, previous + 0x18, self.g + 16);
        copy3(m, key, self.g + 32);
        copy3(m, key + 0xc, self.g + 48);
        mx::multiply(m, spline + curve::DIFFERENCES, self.g, self.l);
    }

    /// The point the differences stand at: row 3.
    fn point(&self) -> u32 {
        self.l + 48
    }
}

/// The walk of a spline of type 2 or 3 over its keys' positions, four at a time: the
/// window of the segment before the first holds keys 0, 1 and 2 in rows 1 to 3 (the
/// second and third key are the first again in a spline of one key, and the walk goes
/// round to the first key after the last).
struct Window {
    key: u32,
    number: u32,
}

impl Window {
    fn next(&mut self, m: &Mem, spline: u32) {
        if self.number == (m.u16(spline + at::SPLINE_NB_KEYS) as u32).wrapping_sub(1) {
            self.key = m.u32(spline + at::SPLINE_KEYS);
            self.number = 0;
        } else {
            self.key = self.key.wrapping_add(0x30);
            self.number += 1;
        }
    }

    fn start(m: &mut Mem, spline: u32, s: &Segment) -> Window {
        s.ones(m);
        let mut key = m.u32(spline + at::SPLINE_KEYS);
        copy3(m, key + 0xc, s.g + 16);
        let second = m.u16(spline + at::SPLINE_NB_KEYS) != 1;
        if second {
            key = key.wrapping_add(0x30);
        } else {
            key = m.u32(spline + at::SPLINE_KEYS);
        }
        let mut w = Window { key, number: second as u32 };
        copy3(m, w.key + 0xc, s.g + 32);
        w.next(m, spline);
        copy3(m, w.key + 0xc, s.g + 48);
        w
    }

    /// The next segment: the window moves on by one key and the differences are set up.
    fn segment(&mut self, m: &mut Mem, spline: u32, s: &Segment) {
        self.next(m, spline);
        copy3(m, s.g + 16, s.g);
        copy3(m, s.g + 32, s.g + 16);
        copy3(m, s.g + 48, s.g + 32);
        copy3(m, self.key + 0xc, s.g + 48);
        mx::multiply(m, spline + curve::DIFFERENCES, s.g, s.l);
    }
}

/// The number of segments of a spline of type 2 or 3: keys + 3 × the closed word − 3.
fn segments(m: &Mem, spline: u32) -> i32 {
    (m.u16(spline + at::SPLINE_NB_KEYS) as i32).wrapping_add((m.i16(spline + curve::CLOSED) as i32).wrapping_mul(3)).wrapping_sub(3)
}

/// Hands every point of a node's **working** definition as it is drawn to a callback, in
/// world space [soft 0x08474a94 SI_modelTraversePnts]: as [`traverse_cvs`] (the matrix,
/// the call, the answer 1 or 0), but with the points of the curves instead of their
/// control points:
///
/// - a patch gives its display curves, those running in U and then those in V, numbered
///   through;
/// - a spline of type 1 (Bézier) gives its first key, then for every further key either
///   the key alone (segment type 2: a straight line) or `steps` points of the segment by
///   forward differences; **with fewer than two keys the answer is 0**, although the
///   first key was handed on; a spline of type 2 or 3 gives the start of its first
///   segment and `steps` points of every segment, and answers 0 without a segment; type
///   0 gives its keys; any other type answers 0;
/// - a face of splines gives its keys only (no curve points), numbered through all
///   contours; a face of NURBS curves has each curve converted
///   (`SI_nurbsCrvConvertToLinear`), walked and disposed, the index running on;
/// - a NURBS curve likewise (0 when the conversion answers null); a NURBS surface gives
///   the points of its display lines (updated first when there are none or bit 0x20 of
///   the byte at +0x5b is set), numbered through.
pub fn traverse_pnts(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, callback: u32, data: u32, own_space: i16) -> i16 {
    let kind = m[node].kind;
    let space = if kind != 0 && kind != 5 && own_space == 0 { m[node].latdep } else { node };
    let matrix = m.local(0x40);
    let point = m.local(12);
    let linear = m.local(4);
    let seg = Segment::new(m);
    mx::copy(m, field!(space, gmat), matrix);
    let def = m[node].definition.addr();
    let call = |m: &mut Mem, o: &mut dyn Outside, p: [F80; 3], index: i32| {
        let v = through(p, &mat(m, matrix));
        m.set_f32(point, v[0].to_f32());
        m.set_f32(point + 8, v[2].to_f32());
        m.set_f32(point + 4, v[1].to_f32());
        o.call(m, &through_pointer(callback), &[point, index as u32, data]);
    };
    let wide = |p: [f32; 3]| [x(p[0]), x(p[1]), x(p[2])];
    // the points of the converted curve the stack word at `linear` names, from `index` on
    let converted = |m: &mut Mem, o: &mut dyn Outside, index: &mut i32| {
        let mut i = 0;
        while i < m.i32(m.u32(linear) + at::CRV_NB_POINTS) {
            let curve = m.u32(linear);
            let p = nurbs_floats(m, curve, at::CRV_POINTS, (i as u32).wrapping_mul(0x24));
            call(m, o, wide(p), *index);
            i += 1;
            *index += 1;
        }
    };
    let mut index = 0;
    let done = match kind {
        0 | 5 => {
            call(m, o, [F80::ZERO; 3], 0);
            true
        }
        1 => {
            for (points, counts) in [(0x1b0, 0x1b4), (0x1b8, 0x1bc)] {
                let mut count = (m.u16(def + counts + 2) as u32).wrapping_mul(m.u16(def + counts) as u32) as i32;
                let mut p = m.u32(def + points);
                if count > 0 {
                    loop {
                        call(m, o, floats(m, p), index);
                        index += 1;
                        p = p.wrapping_add(12);
                        count -= 1;
                        if count == 0 {
                            break;
                        }
                    }
                }
            }
            true
        }
        4 => {
            let count = m.i32(def + at::MESH_NB_VERTICES);
            let mut i = 0;
            while i < count {
                let p = m.u32(def + at::MESH_VERTICES).wrapping_add((i as u32).wrapping_mul(0x14));
                call(m, o, floats(m, p), i);
                i += 1;
            }
            true
        }
        2 => {
            if m.i16(def + at::FACE_SPLINE) != 0 {
                if m.u8(def + at::FACE_NB_HOLES + 1) & 0x80 == 0 {
                    let mut hole: i32 = -1;
                    loop {
                        let contour = if hole == -1 { def + 4 } else { (hole as u32).wrapping_mul(0x74).wrapping_add(m.u32(def + at::FACE_HOLES)) };
                        hole += 1;
                        let mut key = m.u32(contour + at::SPLINE_KEYS);
                        if m.u16(contour + at::SPLINE_NB_KEYS) != 0 {
                            let mut i = 0;
                            loop {
                                call(m, o, floats(m, key.wrapping_add(0xc)), index);
                                index += 1;
                                i += 1;
                                key = key.wrapping_add(0x30);
                                if i >= m.u16(contour + at::SPLINE_NB_KEYS) as i32 {
                                    break;
                                }
                            }
                        }
                        if hole >= m.i16(def + at::FACE_NB_HOLES) as i32 {
                            break;
                        }
                    }
                }
            } else if m.i16(def + at::FACE_NB_HOLES) > 0 {
                let mut curve: i32 = 0;
                loop {
                    let c = (curve as u32).wrapping_mul(100).wrapping_add(m.u32(def + at::FACE_CURVES));
                    let made = o.call(m, &callee::SI_NURBS_CRV_CONVERT_TO_LINEAR, &[c]);
                    m.set_u32(linear, made);
                    curve += 1;
                    converted(m, o, &mut index);
                    o.call(m, &callee::SI_NURBS_CRV_DISPOSE, &[linear]);
                    if curve >= m.i16(def + at::FACE_NB_HOLES) as i32 {
                        break;
                    }
                }
            }
            true
        }
        6 => match m.i32(def + curve::TYPE) {
            1 => 'bezier: {
                if m.u16(def + at::SPLINE_NB_KEYS) == 0 {
                    break 'bezier false;
                }
                let mut previous = m.u32(def + at::SPLINE_KEYS);
                call(m, o, floats(m, previous + 0xc), 0);
                index = 1;
                if m.u16(def + at::SPLINE_NB_KEYS) < 2 {
                    break 'bezier false;
                }
                let mut number = 1;
                seg.ones(m);
                while number < m.u16(def + at::SPLINE_NB_KEYS) as i32 {
                    let key = (number as u32).wrapping_mul(0x30).wrapping_add(m.u32(def + at::SPLINE_KEYS));
                    if m.i16(key + curve::KEY_SEGMENT) == 2 {
                        call(m, o, floats(m, key + 0xc), index);
                        index += 1;
                    } else {
                        seg.bezier(m, def, previous, key);
                        if m.u16(def + curve::STEPS) != 0 {
                            let mut step = 0;
                            loop {
                                step += 1;
                                forward_step(m, seg.l);
                                call(m, o, floats(m, seg.point()), index);
                                index += 1;
                                if step >= m.u16(def + curve::STEPS) as i32 {
                                    break;
                                }
                            }
                        }
                    }
                    number += 1;
                    previous = key;
                }
                true
            }
            0 => {
                if m.u16(def + at::SPLINE_NB_KEYS) != 0 {
                    let mut i = 0;
                    loop {
                        let key = m.u32(def + at::SPLINE_KEYS).wrapping_add((i as u32).wrapping_mul(0x30));
                        call(m, o, floats(m, key.wrapping_add(0xc)), i);
                        i += 1;
                        if i >= m.u16(def + at::SPLINE_NB_KEYS) as i32 {
                            break;
                        }
                    }
                }
                true
            }
            2 | 3 => {
                let count = segments(m, def);
                if count < 1 {
                    false
                } else {
                    let mut w = Window::start(m, def, &seg);
                    let mut number = 0;
                    loop {
                        w.segment(m, def, &seg);
                        if number == 0 {
                            call(m, o, floats(m, seg.point()), index);
                            index += 1;
                        }
                        number += 1;
                        if m.u16(def + curve::STEPS) != 0 {
                            let mut step = 0;
                            loop {
                                step += 1;
                                forward_step(m, seg.l);
                                call(m, o, floats(m, seg.point()), index);
                                index += 1;
                                if step >= m.u16(def + curve::STEPS) as i32 {
                                    break;
                                }
                            }
                        }
                        if number >= count {
                            break;
                        }
                    }
                    true
                }
            }
            _ => false,
        },
        7 => {
            o.call(m, &callee::SI_METABALL_CONTOUR_TRAVERSE_PNTS, &[def, matrix, callback, data]);
            true
        }
        9 => {
            let made = o.call(m, &callee::SI_NURBS_CRV_CONVERT_TO_LINEAR, &[def]);
            m.set_u32(linear, made);
            if made == 0 {
                false
            } else {
                converted(m, o, &mut index);
                o.call(m, &callee::SI_NURBS_CRV_DISPOSE, &[linear]);
                true
            }
        }
        10 => {
            if m.i32(def + 0x80) == 0 || m.u8(def + 0x5b) & 0x20 != 0 {
                o.call(m, &callee::SI_NURB_SRF_UPDATE_LINES, &[def]);
            }
            if def == 0 {
                false
            } else {
                // the display lines: records of 0x14 bytes (a count, and at +0x10 the
                // points, 0x28 bytes each with x, y, z as doubles at +0x10)
                let mut line = 0;
                if m.i32(def + 0x80) > 0 {
                    loop {
                        let record = m.u32(def + 0x84).wrapping_add((line as u32).wrapping_mul(0x14));
                        let mut count = m.i32(record);
                        let mut p = m.u32(m.u32(def + 0x84).wrapping_add((line as u32).wrapping_mul(0x14)).wrapping_add(0x10));
                        line += 1;
                        if count > 0 {
                            loop {
                                let v = [x(m.f64(p + 0x10)).to_f32(), x(m.f64(p + 0x18)).to_f32(), x(m.f64(p + 0x20)).to_f32()];
                                call(m, o, wide(v), index);
                                index += 1;
                                p = p.wrapping_add(0x28);
                                count -= 1;
                                if count == 0 {
                                    break;
                                }
                            }
                        }
                        if line >= m.i32(def + 0x80) {
                            break;
                        }
                    }
                }
                true
            }
        }
        _ => false,
    };
    m.release(0x40 + 12 + 4 + 0x80);
    done as i16
}

/// One point of a node's working definition, numbered as [`traverse_pnts`] numbers them,
/// in world space, at `out` [soft 0x08475da8 SI_modelGetPnt]. **Nothing is stored for a
/// number beyond the points** of a face or a curve, for a kind without points and for a
/// spline type outside 0 to 3; the other kinds do not hold the number against a count.
///
/// The matrix is `gmat` of the node's `latdep`, read in place (the node's own for a null
/// and a joint, whose point is the origin). What differs from the traversal:
///
/// - a NURBS curve or surface and a face of NURBS curves give their **control points**,
///   not the points of the converted curve or of the display lines; in a face, **the
///   weight is read from the point whose number is the curve's number** in the face, not
///   from the point asked for;
/// - a metaball system (kind 7) has its surface made (`SI_metaSysGetSurface(system, 0,
///   0)`) and gives the point `number` of the array at +0x50 (0x48 bytes each, three
///   doubles first);
/// - a Bézier segment is not stepped through when the number lies beyond it.
pub fn get_pnt(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, number: i32, out: u32) {
    let kind = m[node].kind;
    let def = m[node].definition.addr();
    let wide = |p: [f32; 3]| [x(p[0]), x(p[1]), x(p[2])];
    let put = |m: &mut Mem, p: [F80; 3]| {
        let space = m[node].latdep;
        let v = through(p, &mat(m, field!(space, gmat)));
        for (i, c) in v.iter().enumerate() {
            m.set_f32(out + 4 * i as u32, c.to_f32());
        }
    };
    // a point of the differences: stored as it is, then put through the matrix
    let put_row = |m: &mut Mem, at_: u32| {
        copy3(m, at_, out);
        put(m, floats(m, at_));
    };
    match kind {
        0 | 5 => {
            for i in (0..3u32).rev() {
                m.set_f32(out + 4 * i, 0.0);
            }
            let v = through([F80::ZERO; 3], &mat(m, field!(node, gmat)));
            for (i, c) in v.iter().enumerate() {
                m.set_f32(out + 4 * i as u32, c.to_f32());
            }
        }
        1 => {
            let first = (m.u16(def + 0x1b4) as u32).wrapping_mul(m.u16(def + 0x1b6) as u32) as i32;
            let p = if number < first {
                m.u32(def + 0x1b0).wrapping_add((number as u32).wrapping_mul(12))
            } else {
                m.u32(def + 0x1b8).wrapping_add((number.wrapping_sub(first) as u32).wrapping_mul(12))
            };
            put(m, floats(m, p));
        }
        4 => {
            let p = m.u32(def + at::MESH_VERTICES).wrapping_add((number as u32).wrapping_mul(0x14));
            put(m, floats(m, p));
        }
        2 => {
            if m.i16(def + at::FACE_SPLINE) == 0 {
                if m.i16(def + at::FACE_NB_HOLES) < 1 {
                    return;
                }
                let mut curve = m.u32(def + at::FACE_CURVES);
                let mut which = 0i32;
                let mut before = 0i32;
                loop {
                    let count = m.i32(curve + at::CRV_NB_POINTS);
                    if before.wrapping_add(count) > number {
                        break;
                    }
                    curve = curve.wrapping_add(100);
                    which += 1;
                    before = before.wrapping_add(count);
                    if m.i16(def + at::FACE_NB_HOLES) as i32 <= which {
                        return;
                    }
                }
                // the weight of point `which` (the curve's number), the coordinates of
                // the point asked for
                let points = m.u32(curve + at::CRV_POINTS);
                let r = x(1.0f32) / x(m.f64(points.wrapping_add((which as u32).wrapping_mul(0x24)).wrapping_add(0x18)));
                let off = (number.wrapping_sub(before) as u32).wrapping_mul(0x24);
                let px = (x(m.f64(points.wrapping_add(off))) * r).to_f32();
                let py = (x(m.f64(m.u32(curve + at::CRV_POINTS).wrapping_add(off).wrapping_add(8))) * r).to_f32();
                let pz = (r * x(m.f64(m.u32(curve + at::CRV_POINTS).wrapping_add(off).wrapping_add(0x10)))).to_f32();
                put(m, wide([px, py, pz]));
            } else if (m.u16(def + at::FACE_NB_KEYS) as i32) <= number {
                if m.i16(def + at::FACE_NB_HOLES) < 1 {
                    return;
                }
                let mut hole = 0u32;
                let mut which = 0i32;
                let mut before = m.u16(def + at::FACE_NB_KEYS) as i32;
                loop {
                    let count = m.u16(m.u32(def + at::FACE_HOLES).wrapping_add(hole).wrapping_add(at::SPLINE_NB_KEYS)) as i32;
                    if count + before > number {
                        break;
                    }
                    hole = hole.wrapping_add(0x74);
                    which += 1;
                    before += count;
                    if m.i16(def + at::FACE_NB_HOLES) as i32 <= which {
                        return;
                    }
                }
                let keys = m.u32(m.u32(def + at::FACE_HOLES).wrapping_add(hole).wrapping_add(at::SPLINE_KEYS));
                let key = keys.wrapping_add((number.wrapping_sub(before) as u32).wrapping_mul(0x30));
                put(m, floats(m, key.wrapping_add(0xc)));
            } else {
                let key = m.u32(def + at::FACE_KEYS).wrapping_add((number as u32).wrapping_mul(0x30));
                put(m, floats(m, key.wrapping_add(0xc)));
            }
        }
        6 => match m.i32(def + curve::TYPE) {
            0 => {
                let key = m.u32(def + at::SPLINE_KEYS).wrapping_add((number as u32).wrapping_mul(0x30));
                put(m, floats(m, key.wrapping_add(0xc)));
            }
            1 => {
                if m.u16(def + at::SPLINE_NB_KEYS) == 0 {
                    return;
                }
                let mut previous = m.u32(def + at::SPLINE_KEYS);
                if number == 0 {
                    put(m, floats(m, previous + 0xc));
                    return;
                }
                if m.u16(def + at::SPLINE_NB_KEYS) < 2 {
                    return;
                }
                let seg = Segment::new(m);
                let mut count = 1i32;
                let mut k = 1i32;
                seg.ones(m);
                let mut found = false;
                while !found && k < m.u16(def + at::SPLINE_NB_KEYS) as i32 {
                    let key = (k as u32).wrapping_mul(0x30).wrapping_add(m.u32(def + at::SPLINE_KEYS));
                    if m.i16(key + curve::KEY_SEGMENT) == 2 {
                        if number == count {
                            put(m, floats(m, key + 0xc));
                            found = true;
                        }
                        count += 1;
                    } else {
                        let upto = (m.u16(def + curve::STEPS) as i32).wrapping_add(count);
                        if number < upto {
                            seg.bezier(m, def, previous, key);
                            if m.u16(def + curve::STEPS) != 0 {
                                let mut step = 0;
                                loop {
                                    forward_step(m, seg.l);
                                    if number == count {
                                        put_row(m, seg.point());
                                        found = true;
                                        break;
                                    }
                                    count += 1;
                                    step += 1;
                                    if step >= m.u16(def + curve::STEPS) as i32 {
                                        break;
                                    }
                                }
                            }
                        } else {
                            count = upto;
                        }
                    }
                    k += 1;
                    previous = key;
                }
                m.release(0x80);
            }
            2 | 3 => {
                let segments = segments(m, def);
                if segments < 1 {
                    return;
                }
                let seg = Segment::new(m);
                let mut w = Window::start(m, def, &seg);
                let mut count = 0i32;
                let mut number_of = 0;
                'walk: loop {
                    w.segment(m, def, &seg);
                    if number_of == 0 {
                        if number == 0 {
                            put_row(m, seg.point());
                            break 'walk;
                        }
                        count += 1;
                    }
                    let upto = count.wrapping_add(m.u16(def + curve::STEPS) as i32);
                    if number < upto && m.u16(def + curve::STEPS) != 0 {
                        let mut step = 0;
                        loop {
                            forward_step(m, seg.l);
                            if number == count {
                                put_row(m, seg.point());
                                break 'walk;
                            }
                            count += 1;
                            step += 1;
                            if step >= m.u16(def + curve::STEPS) as i32 {
                                break;
                            }
                        }
                    } else {
                        count = upto;
                    }
                    number_of += 1;
                    if segments <= number_of {
                        break;
                    }
                }
                m.release(0x80);
            }
            _ => {}
        },
        7 => {
            o.call(m, &callee::SI_META_SYS_GET_SURFACE, &[def, 0, 0]);
            let off = (number as u32).wrapping_mul(0x48);
            let mut p = [0f32; 3];
            for (i, c) in p.iter_mut().enumerate() {
                *c = x(m.f64(m.u32(def + 0x50).wrapping_add(off).wrapping_add(8 * i as u32))).to_f32();
                m.set_f32(out + 4 * i as u32, *c);
            }
            put(m, wide(p));
        }
        9 | 10 => {
            let points_at = if kind == 9 { at::CRV_POINTS } else { at::SRF_POINTS };
            let p = nurbs_floats(m, def, points_at, (number as u32).wrapping_mul(0x24));
            put(m, wide(p));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_image::Callee;

    /// An outside world that keeps what a callback is handed.
    #[derive(Default)]
    struct Seen(Vec<(i32, [f32; 3])>);

    impl Outside for Seen {
        fn call(&mut self, m: &mut Mem, callee: &Callee, args: &[u32]) -> u32 {
            if callee.name == crate::model_image::THROUGH_POINTER {
                self.0.push((args[1] as i32, [m.f32(args[0]), m.f32(args[0] + 4), m.f32(args[0] + 8)]));
            }
            0
        }
    }

    #[test]
    fn a_linear_and_a_stepped_spline_point_by_point() {
        let mut m = Mem::new();
        let node: Ptr32<Model> = Ptr32::new(m.place(0x430, 8));
        let (def, keys) = (m.place(0x74, 8), m.place(0x30 * 3, 8));
        for k in 0..3u32 {
            m.set_f32(keys + 0x30 * k + 0xc, k as f32);
            m.set_f32(keys + 0x30 * k + 0x10, 10.0 * k as f32);
        }
        m.set_u32(def + at::SPLINE_KEYS, keys);
        m.set_u16(def + at::SPLINE_NB_KEYS, 3);
        m[node].kind = 6;
        m[node].definition = Ptr32::new(def);
        m[node].latdep = node;
        m[node].gmat = si_math::types::IDENTITY;
        m[node].gmat[3][2] = 5.0;
        // type 0: the keys, by both walks and by number
        let mut seen = Seen::default();
        assert_eq!(traverse_cvs(&mut m, &mut seen, node, 0x0800_0000, 0, 0), 1);
        assert_eq!(seen.0, vec![(0, [0.0, 0.0, 5.0]), (1, [1.0, 10.0, 5.0]), (2, [2.0, 20.0, 5.0])]);
        let mut drawn = Seen::default();
        assert_eq!(traverse_pnts(&mut m, &mut drawn, node, 0x0800_0000, 0, 0), 1);
        assert_eq!(drawn.0, seen.0);
        // type 2 with two steps a segment and a matrix of differences that only has a
        // 1 at the bottom right: every point is the window's last key... and the walk's
        // numbers are those `get_pnt` answers to
        m.set_u32(def + curve::TYPE, 2);
        m.set_u16(def + curve::STEPS, 2);
        m.set_f32(def + curve::DIFFERENCES + 60, 1.0);
        let mut drawn = Seen::default();
        assert_eq!(traverse_pnts(&mut m, &mut drawn, node, 0x0800_0000, 0, 0), 0);
        m.set_u16(def + at::SPLINE_NB_KEYS, 5);
        let keys5 = m.place(0x30 * 5, 8);
        for k in 0..5u32 {
            m.set_f32(keys5 + 0x30 * k + 0xc, k as f32);
        }
        m.set_u32(def + at::SPLINE_KEYS, keys5);
        assert_eq!(traverse_pnts(&mut m, &mut drawn, node, 0x0800_0000, 0, 0), 1);
        // two segments: the start and two steps of the first, two steps of the second
        assert_eq!(drawn.0.len(), 5);
        let out = m.place(12, 8);
        for (number, p) in &drawn.0 {
            get_pnt(&mut m, &mut NoPoints, node, *number, out);
            assert_eq!([m.f32(out), m.f32(out + 4), m.f32(out + 8)], *p);
        }
        assert!(m.fault().is_none());
    }

    struct NoPoints;

    impl Outside for NoPoints {
        fn call(&mut self, _: &mut Mem, _: &Callee, _: &[u32]) -> u32 {
            0
        }
    }

    #[test]
    fn a_vertex_is_read_and_set_through_its_element() {
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        crate::model_image::init(&mut m);
        let node: Ptr32<Model> = Ptr32::new(m.place(0x430, 8));
        let (def, vertices, array, element) = (m.place(0x60, 8), m.place(0x14 * 2, 8), m.place(4, 8), m.place(8, 8));
        m.set_u32(def + at::MESH_VERTICES, vertices);
        m.set_u32(def + at::MESH_NB_VERTICES, 2);
        m.set_u32(array, def);
        m[node].kind = 4;
        m[node].nb_defs = 1;
        m[node].defs = Ptr32::new(array);
        m[node].definition = Ptr32::new(def);
        m.set_u32(element + 4, node.addr());
        let mut cid = m[g::SI_CID];
        for (i, c) in cid.iter_mut().enumerate() {
            *c = 0x100 + i as u32;
        }
        m[g::SI_CID] = cid;
        let p = m.place(12, 8);
        for (i, v) in [1.5f32, -2.0, 8.0].iter().enumerate() {
            m.set_f32(p + 4 * i as u32, *v);
        }
        assert_eq!(elem_set_vertex(&mut m, &mut NoPoints, 0, element, cid[8], 2, 0, 1, p), 0);
        assert_eq!(m.f32(vertices + 0x14 + 8), 8.0);
        let tagged = m.place(2, 8);
        let got = elem_get_vertex(&mut m, &mut NoPoints, 0, element, cid[8], 2, 0, 1, 0, tagged);
        assert_eq!((m.heap_blocks().len(), m.f32(got), m.f32(got + 4)), (1, 1.5, -2.0));
        // an index outside the points is reported and answers null
        assert_eq!(elem_get_vertex(&mut m, &mut NoPoints, 0, element, cid[8], 2, 0, 2, 0, tagged), 0);
        let cv = m.place(32, 8);
        assert_eq!(ed_get_ctrl_vertex(&mut m, &mut NoPoints, 0, element, cid[8], 2, 0, 1, cv), 0);
        assert_eq!((m.f64(cv), m.f64(cv + 0x10), m.f64(cv + 0x18)), (1.5, 8.0, 1.0));
        assert!(m.fault().is_none());
    }
}
