// SPDX-License-Identifier: MIT
//! The GLU calls of the layer.

use super::{outside, Ptr};
use crate::rec::{f64_words, i32_words, out_words, ret_u, Val};

outside! {
    /// GLU, as the layer calls it.
    pub trait Glu, recorded by RecGlu {
        fn gluOrtho2D(left: f64, right: f64, bottom: f64, top: f64);
        fn gluPerspective(fovy: f64, aspect: f64, near: f64, far: f64);
        fn gluLookAt(ex: f64, ey: f64, ez: f64, cx: f64, cy: f64, cz: f64, ux: f64, uy: f64, uz: f64);
        fn gluNewQuadric() -> Ptr;
        fn gluPartialDisk(quad: Ptr, inner: f64, outer: f64, slices: i32, loops: i32, start: f64, sweep: f64);
        fn gluDeleteQuadric(quad: Ptr);
        manual:
        /// `gluPickMatrix(x, y, w, h, viewport)`
        fn gluPickMatrix(&mut self, x: f64, y: f64, w: f64, h: f64, viewport: &[i32; 4]) {
            self.log.borrow_mut().call(
                "gluPickMatrix",
                vec![Val::D(x), Val::D(y), Val::D(w), Val::D(h), i32_words(viewport)],
            );
        }
        /// `gluUnProject(winx, winy, winz, model, proj, viewport, &x, &y, &z)`: the result
        /// and the three object coordinates. Answer: `[return, W(six words: three doubles)]`.
        fn gluUnProject(
            &mut self,
            winx: f64,
            winy: f64,
            winz: f64,
            model: &[f64; 16],
            proj: &[f64; 16],
            viewport: &[i32; 4],
        ) -> (i32, [f64; 3]) {
            let a = self.log.borrow_mut().call(
                "gluUnProject",
                vec![
                    Val::D(winx),
                    Val::D(winy),
                    Val::D(winz),
                    f64_words(model),
                    f64_words(proj),
                    i32_words(viewport),
                    Val::Out,
                    Val::Out,
                    Val::Out,
                ],
            );
            let w = out_words(&a, 1);
            let mut o = [0.0f64; 3];
            for (i, v) in o.iter_mut().enumerate() {
                if w.len() >= 2 * i + 2 {
                    *v = f64::from_bits(w[2 * i] as u64 | (w[2 * i + 1] as u64) << 32);
                }
            }
            (ret_u(&a, 1) as i32, o)
        }
    }
}
