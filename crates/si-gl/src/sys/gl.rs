// SPDX-License-Identifier: MIT
//! The OpenGL calls of the layer.

use super::outside;
use crate::rec::{f32_words, f64_words, i32_words, out_bytes, out_words, Val};

outside! {
    /// OpenGL 1.1, as the layer calls it. Enumerants are passed as the numbers the binary
    /// pushes; [`crate::glc`] names them.
    pub trait Gl, recorded by RecGl {
        fn glBegin(mode: u32);
        fn glEnd();
        fn glVertex2f(x: f32, y: f32);
        fn glVertex2d(x: f64, y: f64);
        fn glVertex3d(x: f64, y: f64, z: f64);
        fn glColor3f(r: f32, g: f32, b: f32);
        fn glColor4f(r: f32, g: f32, b: f32, a: f32);
        fn glColor4d(r: f64, g: f64, b: f64, a: f64);
        fn glClearColor(r: f32, g: f32, b: f32, a: f32);
        fn glClearDepth(depth: f64);
        fn glClearStencil(s: i32);
        fn glClear(mask: u32);
        fn glDrawBuffer(mode: u32);
        fn glReadBuffer(mode: u32);
        fn glMatrixMode(mode: u32);
        fn glLoadIdentity();
        fn glPushMatrix();
        fn glPopMatrix();
        fn glPushAttrib(mask: u32);
        fn glPopAttrib();
        fn glOrtho(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64);
        fn glFrustum(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64);
        fn glViewport(x: i32, y: i32, width: i32, height: i32);
        fn glScissor(x: i32, y: i32, width: i32, height: i32);
        fn glDepthRange(near: f64, far: f64);
        fn glDepthFunc(func: u32);
        fn glEnable(cap: u32);
        fn glDisable(cap: u32);
        /// `GLboolean glIsEnabled(cap)`: the byte in `al`.
        fn glIsEnabled(cap: u32) -> u8;
        fn glFinish();
        fn glFlush();
        fn glGetError() -> u32;
        fn glPixelStorei(pname: u32, param: i32);
        fn glPixelZoom(xfactor: f32, yfactor: f32);
        fn glRasterPos2i(x: i32, y: i32);
        fn glRasterPos3d(x: f64, y: f64, z: f64);
        fn glRotated(angle: f64, x: f64, y: f64, z: f64);
        fn glRotatef(angle: f32, x: f32, y: f32, z: f32);
        fn glTranslatef(x: f32, y: f32, z: f32);
        fn glScalef(x: f32, y: f32, z: f32);
        /// `glLineStipple(factor, pattern)`; the pattern is a `GLushort`, pushed as a word.
        fn glLineStipple(factor: i32, pattern: u16);
        fn glFogi(pname: u32, param: i32);
        fn glPolygonMode(face: u32, mode: u32);
        fn glHint(target: u32, mode: u32);
        fn glBlendFunc(sfactor: u32, dfactor: u32);
        fn glLogicOp(opcode: u32);
        fn glStencilOp(fail: u32, zfail: u32, zpass: u32);
        fn glStencilFunc(func: u32, reference: i32, mask: u32);
        fn glStencilMask(mask: u32);
        fn glCallList(list: u32);
        fn glGenLists(range: i32) -> u32;
        fn glNewList(list: u32, mode: u32);
        fn glEndList();
        fn glDeleteLists(list: u32, range: i32);
        /// `GLboolean glIsList(list)`
        fn glIsList(list: u32) -> u8;
        fn glRenderMode(mode: u32) -> i32;
        fn glInitNames();
        fn glPushName(name: u32);
        fn glPopName();
        fn glLoadName(name: u32);
        manual:
        fn glVertex2dv(&mut self, v: &[f64; 2]) {
            self.log.borrow_mut().call("glVertex2dv", vec![f64_words(v)]);
        }
        fn glVertex2fv(&mut self, v: &[f32; 2]) {
            self.log.borrow_mut().call("glVertex2fv", vec![f32_words(v)]);
        }
        fn glVertex2iv(&mut self, v: &[i32; 2]) {
            self.log.borrow_mut().call("glVertex2iv", vec![i32_words(v)]);
        }
        fn glVertex2sv(&mut self, v: &[i16; 2]) {
            self.log.borrow_mut().call("glVertex2sv", vec![shorts(v)]);
        }
        fn glVertex3dv(&mut self, v: &[f64; 3]) {
            self.log.borrow_mut().call("glVertex3dv", vec![f64_words(v)]);
        }
        fn glVertex3fv(&mut self, v: &[f32; 3]) {
            self.log.borrow_mut().call("glVertex3fv", vec![f32_words(v)]);
        }
        fn glVertex3iv(&mut self, v: &[i32; 3]) {
            self.log.borrow_mut().call("glVertex3iv", vec![i32_words(v)]);
        }
        fn glVertex3sv(&mut self, v: &[i16; 3]) {
            self.log.borrow_mut().call("glVertex3sv", vec![shorts(v)]);
        }
        fn glVertex4dv(&mut self, v: &[f64; 4]) {
            self.log.borrow_mut().call("glVertex4dv", vec![f64_words(v)]);
        }
        fn glVertex4fv(&mut self, v: &[f32; 4]) {
            self.log.borrow_mut().call("glVertex4fv", vec![f32_words(v)]);
        }
        fn glVertex4iv(&mut self, v: &[i32; 4]) {
            self.log.borrow_mut().call("glVertex4iv", vec![i32_words(v)]);
        }
        fn glVertex4sv(&mut self, v: &[i16; 4]) {
            self.log.borrow_mut().call("glVertex4sv", vec![shorts(v)]);
        }
        fn glNormal3fv(&mut self, v: &[f32; 3]) {
            self.log.borrow_mut().call("glNormal3fv", vec![f32_words(v)]);
        }
        fn glLoadMatrixf(&mut self, m: &[f32; 16]) {
            self.log.borrow_mut().call("glLoadMatrixf", vec![f32_words(m)]);
        }
        fn glMultMatrixf(&mut self, m: &[f32; 16]) {
            self.log.borrow_mut().call("glMultMatrixf", vec![f32_words(m)]);
        }
        /// `glFogfv(pname, params)` with four floats.
        fn glFogfv(&mut self, pname: u32, params: &[f32; 4]) {
            self.log.borrow_mut().call("glFogfv", vec![Val::U(pname), f32_words(params)]);
        }
        /// `glPolygonStipple(mask)`: 32 × 32 bits.
        fn glPolygonStipple(&mut self, mask: &[u8; 128]) {
            self.log.borrow_mut().call("glPolygonStipple", vec![Val::B(mask.to_vec())]);
        }
        /// `glBitmap(width, height, xorig, yorig, xmove, ymove, bitmap)`
        fn glBitmap(&mut self, width: i32, height: i32, xorig: f32, yorig: f32, xmove: f32, ymove: f32, bitmap: &[u8]) {
            self.log.borrow_mut().call(
                "glBitmap",
                vec![
                    Val::U(width as u32),
                    Val::U(height as u32),
                    Val::F(xorig),
                    Val::F(yorig),
                    Val::F(xmove),
                    Val::F(ymove),
                    Val::B(bitmap.to_vec()),
                ],
            );
        }
        /// `glDrawPixels(width, height, format, type, pixels)`
        fn glDrawPixels(&mut self, width: i32, height: i32, format: u32, kind: u32, pixels: &[u8]) {
            self.log.borrow_mut().call(
                "glDrawPixels",
                vec![Val::U(width as u32), Val::U(height as u32), Val::U(format), Val::U(kind), Val::B(pixels.to_vec())],
            );
        }
        /// `glReadPixels(x, y, width, height, format, type, pixels)`: fills the start of
        /// `pixels`. Answer: `[B(bytes)]`.
        fn glReadPixels(&mut self, x: i32, y: i32, width: i32, height: i32, format: u32, kind: u32, pixels: &mut [u8]) {
            let a = self.log.borrow_mut().call(
                "glReadPixels",
                vec![
                    Val::U(x as u32),
                    Val::U(y as u32),
                    Val::U(width as u32),
                    Val::U(height as u32),
                    Val::U(format),
                    Val::U(kind),
                    Val::Out,
                ],
            );
            let b = out_bytes(&a, 0);
            let n = b.len().min(pixels.len());
            pixels[..n].copy_from_slice(&b[..n]);
        }
        /// `glGetIntegerv(pname, params)`: writes as many values as the query has; `params`
        /// is the room the original's buffer has for them. Answer: `[W(values)]`; values
        /// not answered are left as they were (as OpenGL does on an error).
        fn glGetIntegerv(&mut self, pname: u32, params: &mut [i32]) {
            let a = self.log.borrow_mut().call_keyed("glGetIntegerv", &format!("glGetIntegerv:{pname:#x}"), vec![Val::U(pname), Val::Out]);
            for (p, w) in params.iter_mut().zip(out_words(&a, 0)) {
                *p = w as i32;
            }
        }
        /// `glGetFloatv(pname, params)`. Answer: `[W(float bits)]`.
        fn glGetFloatv(&mut self, pname: u32, params: &mut [f32]) {
            let a = self.log.borrow_mut().call_keyed("glGetFloatv", &format!("glGetFloatv:{pname:#x}"), vec![Val::U(pname), Val::Out]);
            for (p, w) in params.iter_mut().zip(out_words(&a, 0)) {
                *p = f32::from_bits(w);
            }
        }
        /// `glGetDoublev(pname, params)`. Answer: `[W(two words per double, low first)]`.
        fn glGetDoublev(&mut self, pname: u32, params: &mut [f64]) {
            let a = self.log.borrow_mut().call_keyed("glGetDoublev", &format!("glGetDoublev:{pname:#x}"), vec![Val::U(pname), Val::Out]);
            let w = out_words(&a, 0);
            for (i, p) in params.iter_mut().enumerate() {
                if w.len() >= 2 * i + 2 {
                    *p = f64::from_bits(w[2 * i] as u64 | (w[2 * i + 1] as u64) << 32);
                }
            }
        }
        /// `glGetBooleanv(pname, params)` for a one-byte query. Answer: `[U(byte)]`.
        fn glGetBooleanv(&mut self, pname: u32, params: &mut [u8]) {
            let a = self.log.borrow_mut().call("glGetBooleanv", vec![Val::U(pname), Val::Out]);
            if let (Some(p), Some(v)) = (params.first_mut(), a.as_ref().and_then(|v| v.first())) {
                *p = v.u() as u8;
            }
        }
        /// `glGetString(name)`: `None` is a null pointer. Answer: `[S(text)]`.
        fn glGetString(&mut self, name: u32) -> Option<Vec<u8>> {
            let a = self.log.borrow_mut().call("glGetString", vec![Val::U(name)]);
            match a.as_ref().and_then(|v| v.first()) {
                Some(Val::S(s)) => Some(s.clone()),
                _ => None,
            }
        }
        /// `glSelectBuffer(size, buffer)`: `size` is in words. The buffer belongs to the
        /// layer (`malloc`); OpenGL fills it while in selection mode and the layer reads it
        /// after `glRenderMode(GL_RENDER)` through [`Gl::selection`].
        fn glSelectBuffer(&mut self, size: i32) {
            self.log.borrow_mut().call("glSelectBuffer", vec![Val::U(size as u32), Val::Out]);
        }
        /// Memory, not a call: the words OpenGL has written into the selection buffer.
        /// The recording implementation takes them from the answer queued under the name
        /// `"selection"` (`[W(words)]`) and logs nothing.
        fn selection(&mut self, size: usize) -> Vec<u32> {
            let mut log = self.log.borrow_mut();
            let a = log.take_answer("selection");
            let mut w = out_words(&a, 0);
            w.resize(size, 0);
            w
        }
    }
}

fn shorts(v: &[i16]) -> Val {
    let mut b = Vec::with_capacity(v.len() * 2);
    for s in v {
        b.extend_from_slice(&s.to_le_bytes());
    }
    Val::B(b)
}
