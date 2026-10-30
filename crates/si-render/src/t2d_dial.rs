// SPDX-License-Identifier: MIT
//! The wrap flags of a 2D texture, on the memory image: three routines of the original's
//! `t2d_dial.c` (the texture dialogue's file) that the renderer's set-up calls for every
//! texture before the first frame (`SI_setUvWrapFlags`, finding 0012, sections 16.2 and
//! 19.4). `si-hrc` has the same three on its file records (the model reader calls them
//! too); here they work on `__SI_Model` and `__SI_LclMap` in the image.
//!
//! Offsets: `__SI_Model` +0x30 the kind, +0x48 the current definition (a patch: +0x10,
//! +0x12 closed in u, in v; a NURBS surface: +0x20, +0x44); `__SI_LclMap` +0x2a the mapping
//! method, +0x58, +0x5c the scaling in u and v, +0x102, +0x104 the wrap flags.

use crate::data::text;
use crate::img::{f, Outside};
use si_mem::Mem;
use si_x87::x;

/// `stderr` (0x08780c88): the C library's variable, copied into the program's `.bss` by
/// the dynamic linker; a `FILE *`.
pub const STDERR: u32 = 0x0878_0c88;

/// [soft 0x082b0788 updDoWrap]: `updDoWrap(node, texture)`: the texture's two wrap flags
/// from its mapping method and the shape it lies on.
///
/// u and v start as 1. With a node: a patch (kind 1) with a definition gives its two
/// "closed" words, a NURBS surface (kind 10) likewise; on a mesh (kind 4) the UV method (4)
/// gives 0 and 0; on any kind other than 0, 1, 4, 5, 6, 9 and 10 **the UV method is
/// replaced by method 6**. Then by method: 1 to 3: both flags 0; 4: u and v; 5 to 8: 1 and
/// 0; anything else: `fprintf(stderr, "Unexpected default in file '%s' at line %d:\n
/// %s\n", <the source file's path>, 0x1582, "Unknown texture surface or reflection
/// map-on.")` and the flags stay. Class: copy.
pub fn upd_do_wrap(m: &mut Mem, env: &mut dyn Outside, node: u32, texture: u32) {
    let (mut u, mut v) = (1u16, 1u16);
    if node != 0 {
        let kind = m.i32(node + 0x30);
        if kind == 1 {
            let patch = m.u32(node + 0x48);
            if patch != 0 {
                u = m.u16(patch + 0x10);
                v = m.u16(patch + 0x12);
            }
        } else if kind == 10 {
            let surface = m.u32(node + 0x48);
            if surface != 0 {
                u = m.u16(surface + 0x20);
                v = m.u16(surface + 0x44);
            }
        } else if kind == 4 {
            if m.i16(texture + 0x2a) == 4 {
                u = 0;
                v = 0;
            }
        } else if !matches!(kind, 1 | 6 | 0 | 5 | 9 | 10 | 4) && m.i16(texture + 0x2a) == 4 {
            m.set_i16(texture + 0x2a, 6);
        }
    }
    match (m.i16(texture + 0x2a) as i32).wrapping_sub(1) as i16 as i32 as u32 {
        0..=2 => {
            m.set_u16(texture + 0x102, 0);
            m.set_u16(texture + 0x104, 0);
        }
        3 => {
            m.set_u16(texture + 0x102, u);
            m.set_u16(texture + 0x104, v);
        }
        4..=7 => {
            m.set_u16(texture + 0x102, 1);
            m.set_u16(texture + 0x104, 0);
        }
        _ => {
            let stream = m.u32(STDERR);
            env.call(m, f::FPRINTF, &[stream, text::UNEXPECTED_DEFAULT, text::T2D_DIAL_PATH, 0x1582, text::UNKNOWN_SURFACE]);
        }
    }
}

/// One scaling of [`set_scale`]: below −1 it becomes −1, above 1 it becomes 1, strictly
/// between −0.0001 and 0.0001 it becomes 0.0001 when above 0 and −0.0001 otherwise (zero
/// included); else, and for a NaN, it stays. The limits are doubles.
fn hold(m: &mut Mem, at: u32) {
    let s = x(m.f32(at));
    if s.lt(x(-1.0f64)) {
        m.set_u32(at, 0xbf80_0000);
    } else if x(1.0f64).lt(s) {
        m.set_u32(at, 0x3f80_0000);
    } else if s.gt(x(-0.0001f64)) && s.lt(x(0.0001f64)) {
        m.set_u32(at, if x(0.0f64).lt(s) { 0x38d1_b717 } else { 0xb8d1_b717 });
    }
}

/// [soft 0x082ada24 SI_txt2dSetScale]: `SI_txt2dSetScale(texture)`: the scaling of a
/// wrapping texture kept within −1 to 1 and away from zero ([`hold`]): in u when the u
/// flag (+0x102) is set; in v when the v flag (+0x104) is set or the method is 6, 7 or 8.
/// Class: copy (comparisons only).
pub fn set_scale(m: &mut Mem, texture: u32) {
    if m.i16(texture + 0x102) != 0 {
        hold(m, texture + 0x58);
    }
    let method = m.u16(texture + 0x2a);
    if m.i16(texture + 0x104) == 0 && method != 6 && method.wrapping_sub(7) > 1 {
        return;
    }
    hold(m, texture + 0x5c);
}

/// [soft 0x082ad9f8 SI_txt2dSetWrap]: `SI_txt2dSetWrap(node, texture)`: [`upd_do_wrap`]
/// and [`set_scale`], unless the texture is null.
pub fn set_wrap(m: &mut Mem, env: &mut dyn Outside, node: u32, texture: u32) {
    if texture != 0 {
        upd_do_wrap(m, env, node, texture);
        set_scale(m, texture);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Calls(Vec<Vec<u32>>);
    impl Outside for Calls {
        fn call(&mut self, _: &mut Mem, _: u32, a: &[u32]) -> u32 {
            self.0.push(a.to_vec());
            0
        }
    }

    #[test]
    fn flags_by_shape_and_method() {
        let mut m = Mem::new();
        let (node, map, patch) = (m.calloc(1, 0x430), m.calloc(1, 0x1d8), m.calloc(1, 0x40));
        let mut env = Calls(Vec::new());
        // a UV texture on a mesh does not wrap, and its scaling is not held
        m.set_i32(node + 0x30, 4);
        m.set_i16(map + 0x2a, 4);
        m.set_f32(map + 0x58, 3.0);
        m.set_f32(map + 0x5c, 0.0);
        set_wrap(&mut m, &mut env, node, map);
        assert_eq!((m.u16(map + 0x102), m.u16(map + 0x104), m.f32(map + 0x58), m.f32(map + 0x5c)), (0, 0, 3.0, 0.0));
        // on a patch closed in u it wraps in u: the scaling there is held to 1
        m.set_i32(node + 0x30, 1);
        m.set_u32(node + 0x48, patch);
        m.set_u16(patch + 0x10, 1);
        set_wrap(&mut m, &mut env, node, map);
        assert_eq!((m.u16(map + 0x102), m.u16(map + 0x104), m.f32(map + 0x58), m.f32(map + 0x5c)), (1, 0, 1.0, 0.0));
        // on a face (kind 2) the UV method becomes method 6: wraps in u, and v is held
        m.set_i32(node + 0x30, 2);
        set_wrap(&mut m, &mut env, node, map);
        assert_eq!((m.i16(map + 0x2a), m.u16(map + 0x102), m.u16(map + 0x104)), (6, 1, 0));
        assert_eq!(m.u32(map + 0x5c), 0xb8d1_b717);
        // a method the routine does not know: a line on standard error, the flags stay
        m.set_i16(map + 0x2a, 9);
        m.set_u16(map + 0x102, 7);
        set_wrap(&mut m, &mut env, 0, map);
        assert_eq!(env.0, [vec![0, text::UNEXPECTED_DEFAULT, text::T2D_DIAL_PATH, 0x1582, text::UNKNOWN_SURFACE]]);
        assert_eq!(m.u16(map + 0x102), 7);
        set_wrap(&mut m, &mut env, node, 0);
        assert!(m.fault().is_none());
    }
}
