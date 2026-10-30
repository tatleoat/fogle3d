// SPDX-License-Identifier: MIT
//! A face as a mesh, for `SI_face2triangle` (finding 0012, section 18).
//!
//! Nothing here reproduces a function of the original: the converter
//! `SI_faceConvertToMesh (0x08435960)` is ported in `si-model` on the model file's face
//! record (`si_hrc::Face`), and what it calls in the spline code is ported in `si-spline` on
//! the spline's record in memory. This module binds the two for the renderer's collect
//! step: the routines `si-model` asks for through its `Hooks` are answered with
//! `si-spline`'s `SI_splineConvertToLinear (0x0848beec)`, `SI_splineInverse (0x0848e35c)`
//! and `SI_splineCleanup (0x0848eb28)` ports, a spline carried from one record type to the other and
//! back for each call.
//!
//! A face of NURBS curves (the word at +0x84 of the face is 0) is not bound:
//! `SI_nurbsCrvConvertToLinear` and `SI_nurbsCrvInverse` stay `si-model`'s defaults (a copy;
//! no reversal), which is the original's result only for curves that are linear and wound
//! consistently already.

use si_hrc::{Face, Spline};
use si_model::mesh::Mesh;
use si_model::mesh_b::MeshEnv;
use si_spline::{Curve, NoHost};

fn vector(v: [f32; 3]) -> si_spline::Vector {
    let mut out = <si_spline::Vector as si_types::Zero>::zero();
    (out.x, out.y, out.z) = (v[0], v[1], v[2]);
    out
}

/// A spline of a model's record as the spline code holds one.
pub fn curve_of(s: &Spline) -> Curve {
    let mut c = Curve::new();
    c.rec.phantom = s.phantom;
    c.rec.closed = s.close;
    c.rec.kind = s.kind;
    c.rec.nb_keys = s.nkeys as i16;
    c.rec.tension = s.tension;
    c.rec.step = s.step;
    c.keys = s
        .keys
        .iter()
        .map(|k| {
            let mut n = si_spline::new_key();
            (n.prev, n.pos, n.next) = (vector(k.prev), vector(k.pos), vector(k.next));
            (n.constraint, n.segment, n.flag) = (k.constraint, k.segment, k.flag);
            n
        })
        .collect();
    if !c.keys.is_empty() {
        // (a placeholder for the key block's address: any pointer that is not null)
        c.rec.keys = si_types::Ptr32::new(1);
    }
    c
}

/// [`curve_of`] the other way; the name is `like`'s.
pub fn spline_of(c: &Curve, like: &Spline) -> Spline {
    let n = c.nb().min(c.keys.len());
    Spline {
        name: like.name.clone(),
        phantom: c.rec.phantom,
        close: c.rec.closed,
        kind: c.rec.kind,
        nkeys: c.rec.nb_keys as u16,
        keys: c.keys[..n]
            .iter()
            .map(|k| si_hrc::SplineKey { prev: [k.prev.x, k.prev.y, k.prev.z], pos: [k.pos.x, k.pos.y, k.pos.z], next: [k.next.x, k.next.y, k.next.z], constraint: k.constraint, segment: k.segment, flag: k.flag })
            .collect(),
        tension: c.rec.tension,
        step: c.rec.step,
        old_points: false,
    }
}

/// What `face.c` calls in the spline code, answered by `si-spline`.
#[derive(Default)]
pub struct SplineHooks {
    /// The running number `SI_splineAllocate` names a new spline with.
    pub counter: i32,
}

impl si_model::face::Hooks for SplineHooks {
    fn spline_inverse(&mut self, _: i32, s: &mut Spline) {
        let mut c = curve_of(s);
        si_spline::edit::inverse(&mut c, &mut NoHost);
        *s = spline_of(&c, s);
    }
    fn spline_convert_to_linear(&mut self, _: i32, s: &Spline, step: i16) -> Spline {
        match si_spline::convert::convert_to_linear(&curve_of(s), 1, step as i32, 0.0, &mut self.counter, &mut NoHost) {
            Some(c) => spline_of(&c, s),
            // (a spline without keys: the caller does not ask for it)
            None => Spline::default(),
        }
    }
    fn spline_cleanup(&mut self, _: i32, s: &mut Spline) {
        let mut c = curve_of(s);
        si_spline::convert::cleanup(&mut c, &mut NoHost);
        *s = spline_of(&c, s);
    }
}

/// `SI_faceConvertToMesh(face, 0)` as `SI_face2triangle` calls it: `si-model`'s port with
/// [`SplineHooks`]. `None` where the original returns no mesh (an outline without keys).
///
/// The original begins with `SI_meshAllocate (0x08458c20)`, which asks the C library for the
/// cosine of the constant at 0x0865faf8 (60 degrees in radians) before anything else, also
/// for a face that then gives no mesh. Until the tenth pass that call was made here, because
/// `si-model`'s converter started from a mesh that held the cosine as a constant; `si-model`
/// now makes it itself, first of all, so that a check that replays the original's library
/// calls meets them in the original's order without a call of this crate's.
pub fn face_to_mesh(face: &Face) -> Option<Mesh> {
    si_model::face::face_convert_to_mesh(face, false, &mut MeshEnv::default(), &mut SplineHooks::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::softvertex::{face2triangle, MatRef};

    fn square(close: i16) -> Face {
        let key = |x: f32, y: f32| si_hrc::SplineKey { prev: [x, y, 0.0], pos: [x, y, 0.0], next: [x, y, 0.0], constraint: 0, segment: 0, flag: 0 };
        let exterior = Spline { close, nkeys: 4, keys: vec![key(0.0, 0.0), key(2.0, 0.0), key(2.0, 2.0), key(0.0, 2.0)], ..Spline::default() };
        Face { exterior, ..Face::default() }
    }

    #[test]
    fn a_square_face_is_two_triangles_of_its_four_points() {
        let face = square(1);
        let mesh = face_to_mesh(&face).expect("a face with an outline gives a mesh");
        assert_eq!((mesh.vertices.len(), mesh.polygons.len()), (4, 1));
        let r = face2triangle(true, &mut |_| face_to_mesh(&face), Some(&[MatRef { id: 0, maps: None }]), 7).unwrap();
        assert_eq!((r.verts.len(), r.tn_count), (4, 2));
        assert!(r.tns[..2].iter().all(|t| t.material == 0 && t.w58 == 7 && t.v.iter().all(|c| c.is_some())));
    }

    #[test]
    fn a_face_without_an_outline_gives_nothing() {
        let face = Face::default();
        assert!(face_to_mesh(&face).is_none());
        let r = face2triangle(true, &mut |_| face_to_mesh(&face), None, 0).unwrap();
        assert_eq!((r.verts.len(), r.tn_count), (0, 0));
    }

    #[test]
    fn a_spline_goes_to_the_spline_code_and_back_unchanged() {
        let s = square(0).exterior;
        assert_eq!(spline_of(&curve_of(&s), &s), s);
    }
}
