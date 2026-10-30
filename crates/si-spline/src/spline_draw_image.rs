// SPDX-License-Identifier: MIT
//! `spline.c`: the spline's drawing routines on the one memory image (finding 0040, section 5).
//! The graphics-library calls go through [`GlCalls`] (the checks record them). The routines
//! build their points in the stack frame the original lays out (a vertex call receives the
//! address of a local), so the frame is reproduced address for address: the entry stack
//! pointer is the original's, the saved frame pointer takes four bytes below it.

use crate::patch_draw_image::GlCalls;
use si_math::algebra::matrix_multiply;
use si_mem::{Mem, Outside, Ptr32};
use si_types::geom::Spline;
use si_x87::x;

/// The original's frame of `SI_splineDraw`: `ebp` is the entry stack pointer less 4.
struct Frame {
    ebp: u32,
}

impl Frame {
    /// Takes the frame (`frame` bytes of locals plus the saved frame pointer) off the stack.
    fn new(m: &mut Mem, frame: u32) -> Frame {
        let ebp = m.sp().wrapping_sub(4);
        m.local(frame + 4);
        Frame { ebp }
    }
    fn at(&self, off: u32) -> u32 {
        self.ebp.wrapping_sub(off)
    }
}

fn copy3(m: &mut Mem, dst: u32, src: u32) {
    for i in 0..3 {
        let v = m.f32(src + 4 * i);
        m.set_f32(dst + 4 * i, v);
    }
}

/// The 4 x 4 float matrix at an address.
fn read_matrix(m: &Mem, a: u32) -> [[f32; 4]; 4] {
    let mut r = [[0.0f32; 4]; 4];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = m.f32(a + 16 * i as u32 + 4 * j as u32);
        }
    }
    r
}

fn write_matrix(m: &mut Mem, a: u32, r: &[[f32; 4]; 4]) {
    for (i, row) in r.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            m.set_f32(a + 16 * i as u32 + 4 * j as u32, *v);
        }
    }
}

/// Multiplies the spline's matrix (+0x24) by the four points at `geo` into `out` (the
/// original's call of `SI_matrixMultiply`, [`matrix_multiply`]).
fn multiply(m: &mut Mem, s: u32, geo: u32, out: u32) {
    let r = matrix_multiply(&read_matrix(m, s + 0x24), &read_matrix(m, geo));
    write_matrix(m, out, &r);
}

/// The forward-difference loop over `step` steps: rows 3, 2, 1 of the matrix at `mat` each
/// get the row before added (float adds, the sum formed in the register), and row 3 is
/// drawn (the vertex call gets its address `mat + 0x30`).
fn steps(m: &mut Mem, o: &mut dyn Outside, s: u32, mat: u32) {
    let mut n = 0u32;
    if m.u16(s + 0x20) == 0 {
        return;
    }
    loop {
        n += 1;
        for row in (1..4u32).rev() {
            for c in 0..4u32 {
                let a = m.f32(mat + 16 * row + 4 * c);
                let b = m.f32(mat + 16 * (row - 1) + 4 * c);
                m.set_f32(mat + 16 * row + 4 * c, (x(a) + b).to_f32());
            }
        }
        o.v3f(m, mat + 0x30);
        if n >= m.u16(s + 0x20) as u32 {
            break;
        }
    }
}

/// [soft 0x0848b510 SI_splineDraw]: the spline as one line strip. A linear spline
/// (type 0, more than one key) is its keys' positions, closed by the first again; a type 1
/// spline is drawn segment by segment from the key's position, tangent out and the next
/// key's tangent in and position, `step` forward-difference points per segment, a
/// segment of the next key marked 2 being a straight line to its position; types 2 and 3
/// (`nb_keys + 3 * closed - 3` segments) slide a window of four positions over the keys,
/// wrapping round. Any other type draws nothing.
pub fn spline_draw(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>) {
    let sp = m.sp();
    let f = Frame::new(m, 0x9c);
    draw_body(m, o, spline.addr(), &f);
    m.set_sp(sp);
}

fn draw_body(m: &mut Mem, o: &mut dyn Outside, s: u32, f: &Frame) {
    let kind = m.u32(s + 0x0c) as i32;
    let nb = m.u16(s + 0x10) as u32;
    match kind {
        0 => {
            if nb <= 1 {
                return;
            }
            o.bgnline(m);
            let mut at = m.u32(s + 0x14);
            for _ in 0..m.u16(s + 0x10) as u32 {
                o.v3f(m, at.wrapping_add(0x0c));
                at = at.wrapping_add(0x30);
            }
            if m.u16(s + 0x08) != 0 {
                let k = m.u32(s + 0x14);
                o.v3f(m, k.wrapping_add(0x0c));
            }
            o.endline(m);
        }
        1 => {
            if nb <= 1 {
                return;
            }
            let (mut prev, mut i) = if m.u16(s + 0x08) != 0 {
                (m.u32(s + 0x14).wrapping_add(nb.wrapping_mul(0x30)).wrapping_sub(0x30), 0u32)
            } else {
                (m.u32(s + 0x14), 1u32)
            };
            o.bgnline(m);
            o.v3f(m, prev.wrapping_add(0x0c));
            for off in [0x04u32, 0x14, 0x24, 0x34] {
                m.set_f32(f.at(off), 1.0);
            }
            while i < m.u16(s + 0x10) as u32 {
                let cur = m.u32(s + 0x14).wrapping_add(i.wrapping_mul(0x30));
                i += 1;
                if m.u16(cur + 0x2a) != 2 {
                    copy3(m, f.at(0x40), prev + 0x0c);
                    copy3(m, f.at(0x30), prev + 0x18);
                    copy3(m, f.at(0x20), cur);
                    copy3(m, f.at(0x10), cur + 0x0c);
                    multiply(m, s, f.at(0x40), f.at(0x80));
                    steps(m, o, s, f.at(0x80));
                } else {
                    o.v3f(m, cur + 0x0c);
                }
                prev = cur;
            }
            o.endline(m);
        }
        2 | 3 => {
            let count = (nb as i32).wrapping_add((m.u16(s + 0x08) as i16 as i32).wrapping_mul(3)).wrapping_sub(3);
            if count <= 0 {
                return;
            }
            for off in [0x04u32, 0x14, 0x24, 0x34] {
                m.set_f32(f.at(off), 1.0);
            }
            let mut at = m.u32(s + 0x14);
            copy3(m, f.at(0x30), at + 0x0c);
            let mut idx = 0u32;
            if m.u16(s + 0x10) != 1 {
                at = at.wrapping_add(0x30);
                idx = 1;
            }
            copy3(m, f.at(0x20), at + 0x0c);
            let advance = |m: &Mem, at: &mut u32, idx: &mut u32| {
                if *idx == (m.u16(s + 0x10) as u32).wrapping_sub(1) {
                    *at = m.u32(s + 0x14);
                    *idx = 0;
                } else {
                    *at = at.wrapping_add(0x30);
                    *idx += 1;
                }
            };
            advance(m, &mut at, &mut idx);
            copy3(m, f.at(0x10), at + 0x0c);
            let mut i = 0i32;
            while i < count {
                advance(m, &mut at, &mut idx);
                copy3(m, f.at(0x40), f.at(0x30));
                copy3(m, f.at(0x30), f.at(0x20));
                copy3(m, f.at(0x20), f.at(0x10));
                copy3(m, f.at(0x10), at + 0x0c);
                multiply(m, s, f.at(0x40), f.at(0x80));
                if i == 0 {
                    o.bgnline(m);
                    o.v3f(m, f.at(0x50));
                }
                i += 1;
                steps(m, o, s, f.at(0x80));
            }
            o.endline(m);
        }
        _ => {}
    }
}

/// Whether bit 5 of the key's flag (the low byte of the `u16` at +0x2c) is set: the key was modified.
fn flagged(m: &Mem, k: u32) -> bool {
    m.u8(k + 0x2c) & 0x20 != 0
}

/// [soft 0x0848b950 SI_splineDrawModif]: [`spline_draw`] restricted to the segments that
/// touch a modified key (flag bit 0x20 of the segment's start or end key), each as its own
/// line. Types 0 and 1 walk the keys (a closed spline starts from the last key to the
/// first); a type 1 segment whose end key is marked 2 is a straight line; types 2 and 3 draw
/// a segment when its newest key is flagged. Any other type draws nothing.
pub fn spline_draw_modif(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>) {
    let sp = m.sp();
    let f = Frame::new(m, 0xac);
    modif_body(m, o, spline.addr(), &f);
    m.set_sp(sp);
}

fn modif_body(m: &mut Mem, o: &mut dyn Outside, s: u32, f: &Frame) {
    let kind = m.u32(s + 0x0c) as i32;
    let nb = m.u16(s + 0x10) as i32;
    match kind {
        0 | 1 => {
            if nb <= 1 {
                return;
            }
            let (mut prev, mut i) = if m.u16(s + 0x08) != 0 {
                (m.u32(s + 0x14).wrapping_add((nb as u32).wrapping_mul(0x30)).wrapping_sub(0x30), 0i32)
            } else {
                (m.u32(s + 0x14), 1i32)
            };
            if kind == 1 {
                for off in [0x04u32, 0x14, 0x24, 0x34] {
                    m.set_f32(f.at(off), 1.0);
                }
            }
            while i < nb {
                let cur = m.u32(s + 0x14).wrapping_add((i as u32).wrapping_mul(0x30));
                let touched = flagged(m, prev) || flagged(m, cur);
                i += 1;
                if kind == 0 {
                    if touched {
                        o.bgnline(m);
                        o.v3f(m, prev + 0x0c);
                        o.v3f(m, cur + 0x0c);
                        o.endline(m);
                    }
                } else if touched && m.u16(cur + 0x2a) == 2 {
                    o.bgnline(m);
                    o.v3f(m, prev + 0x0c);
                    o.v3f(m, cur + 0x0c);
                    o.endline(m);
                } else {
                    copy3(m, f.at(0x40), prev + 0x0c);
                    copy3(m, f.at(0x30), prev + 0x18);
                    copy3(m, f.at(0x20), cur);
                    copy3(m, f.at(0x10), cur + 0x0c);
                    if touched {
                        multiply(m, s, f.at(0x40), f.at(0x80));
                        o.bgnline(m);
                        o.v3f(m, prev + 0x0c);
                        steps(m, o, s, f.at(0x80));
                        o.endline(m);
                    }
                }
                prev = cur;
            }
        }
        2 | 3 => {
            let count = nb.wrapping_add((m.u16(s + 0x08) as i16 as i32).wrapping_mul(3)).wrapping_sub(3);
            if count <= 0 {
                return;
            }
            for off in [0x04u32, 0x14, 0x24, 0x34] {
                m.set_f32(f.at(off), 1.0);
            }
            let mut at = m.u32(s + 0x14);
            copy3(m, f.at(0x30), at + 0x0c);
            let mut idx = 0i32;
            if idx != nb - 1 {
                at = at.wrapping_add(0x30);
                idx = 1;
            }
            copy3(m, f.at(0x20), at + 0x0c);
            let advance = |m: &Mem, at: &mut u32, idx: &mut i32| {
                if *idx == nb - 1 {
                    *at = m.u32(s + 0x14);
                    *idx = 0;
                } else {
                    *at = at.wrapping_add(0x30);
                    *idx += 1;
                }
            };
            advance(m, &mut at, &mut idx);
            copy3(m, f.at(0x10), at + 0x0c);
            let mut i = 0i32;
            while i < count {
                advance(m, &mut at, &mut idx);
                copy3(m, f.at(0x40), f.at(0x30));
                copy3(m, f.at(0x30), f.at(0x20));
                copy3(m, f.at(0x20), f.at(0x10));
                copy3(m, f.at(0x10), at + 0x0c);
                i += 1;
                if flagged(m, at) {
                    multiply(m, s, f.at(0x40), f.at(0x80));
                    o.bgnline(m);
                    o.v3f(m, f.at(0x50));
                    steps(m, o, s, f.at(0x80));
                    o.endline(m);
                }
            }
        }
        _ => {}
    }
}

/// One forward-difference step of the matrix at `mat` (rows 3, 2, 1 each get the row before
/// added, all four columns) and the vertex call for row 3: the one step `SI_splineDrawStart`
/// takes.
fn one_step(m: &mut Mem, o: &mut dyn Outside, mat: u32) {
    for row in (1..4u32).rev() {
        for c in 0..4u32 {
            let a = m.f32(mat + 16 * row + 4 * c);
            let b = m.f32(mat + 16 * (row - 1) + 4 * c);
            m.set_f32(mat + 16 * row + 4 * c, (x(a) + b).to_f32());
        }
    }
    o.v3f(m, mat + 0x30);
}

/// [soft 0x0849191c SI_splineDrawStart]: the first stretch of the spline as a line, as the
/// start of an interactive draw: a linear spline its first two key positions; a type 1
/// spline the first key's position and, when the second key is not marked 2, the first
/// forward-difference point of the first segment (a marked one: the second key's
/// position); types 2 and 3 (`nb_keys - 3 + 3 * closed` segments, 16-bit arithmetic) the
/// start point and first difference point of the first segment of the sliding window. The
/// step count is not read: one difference step is taken whatever it is.
pub fn spline_draw_start(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>) {
    let sp = m.sp();
    let f = Frame::new(m, 0x9c);
    start_body(m, o, spline.addr(), &f);
    m.set_sp(sp);
}

fn start_body(m: &mut Mem, o: &mut dyn Outside, s: u32, f: &Frame) {
    let kind = m.u32(s + 0x0c) as i32;
    let nb = m.u16(s + 0x10);
    match kind {
        0 => {
            if nb <= 1 {
                return;
            }
            o.bgnline(m);
            let k = m.u32(s + 0x14);
            o.v3f(m, k.wrapping_add(0x0c));
            let k = m.u32(s + 0x14);
            o.v3f(m, k.wrapping_add(0x3c));
            o.endline(m);
        }
        1 => {
            if nb <= 1 {
                return;
            }
            o.bgnline(m);
            for off in [0x04u32, 0x14, 0x24, 0x34] {
                m.set_f32(f.at(off), 1.0);
            }
            let k0 = m.u32(s + 0x14);
            o.v3f(m, k0 + 0x0c);
            let k0 = m.u32(s + 0x14);
            let k1 = k0.wrapping_add(0x30);
            if m.u16(k1 + 0x2a) == 2 {
                o.v3f(m, k0.wrapping_add(0x3c));
            } else {
                copy3(m, f.at(0x40), k0 + 0x0c);
                copy3(m, f.at(0x30), k0 + 0x18);
                copy3(m, f.at(0x20), k1);
                copy3(m, f.at(0x10), k1 + 0x0c);
                multiply(m, s, f.at(0x40), f.at(0x80));
                one_step(m, o, f.at(0x80));
            }
            o.endline(m);
        }
        2 | 3 => {
            let count = (nb.wrapping_sub(3) as i16).wrapping_add((m.u16(s + 0x08) as i16).wrapping_mul(3));
            if count <= 0 {
                return;
            }
            for off in [0x04u32, 0x14, 0x24, 0x34] {
                m.set_f32(f.at(off), 1.0);
            }
            let mut at = m.u32(s + 0x14);
            copy3(m, f.at(0x30), at + 0x0c);
            let mut idx = 0i32;
            if nb != 1 {
                at = at.wrapping_add(0x30);
                idx = 1;
            }
            copy3(m, f.at(0x20), at + 0x0c);
            let last = (nb as i32) - 1;
            if idx == last {
                at = m.u32(s + 0x14);
                idx = 0;
            } else {
                at = at.wrapping_add(0x30);
                idx += 1;
            }
            copy3(m, f.at(0x10), at + 0x0c);
            at = if idx == last { m.u32(s + 0x14) } else { at.wrapping_add(0x30) };
            copy3(m, f.at(0x40), f.at(0x30));
            copy3(m, f.at(0x30), f.at(0x20));
            copy3(m, f.at(0x20), f.at(0x10));
            copy3(m, f.at(0x10), at + 0x0c);
            multiply(m, s, f.at(0x40), f.at(0x80));
            o.bgnline(m);
            o.v3f(m, f.at(0x50));
            one_step(m, o, f.at(0x80));
            o.endline(m);
        }
        _ => {}
    }
}

/// [soft 0x0848be6c oldsplineHit]: the control polygon for picking. With at least one key:
/// one line strip through the keys' positions, `loadname` of the key's number (a `short`)
/// before each vertex, and for a closed spline the first key again under the name `nb_keys`.
pub fn old_spline_hit(m: &mut Mem, o: &mut dyn Outside, spline: Ptr32<Spline>) {
    let s = spline.addr();
    if m.u16(s + 0x10) == 0 {
        return;
    }
    let mut at = m.u32(s + 0x14);
    o.bgnline(m);
    let mut i = 0u32;
    if m.u16(s + 0x10) != 0 {
        loop {
            o.loadname(m, i as u16 as i16 as i32);
            o.v3f(m, at.wrapping_add(0x0c));
            i += 1;
            at = at.wrapping_add(0x30);
            if i as i32 >= m.u16(s + 0x10) as i32 {
                break;
            }
        }
    }
    if m.u16(s + 0x08) != 0 {
        let k = m.u32(s + 0x14);
        o.loadname(m, m.u16(s + 0x10) as i16 as i32);
        o.v3f(m, k.wrapping_add(0x0c));
    }
    o.endline(m);
}
