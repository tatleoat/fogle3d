// SPDX-License-Identifier: MIT
//! `overt2soft.c` (finding 0039): the Vertigo `.pol` reader `pmeshReadVertigo`, which builds a mesh from the
//! text file `<name>.pol`.
//!
//! The file, read from the listing: free text up to the word `vertices`; then one `( x y z )` group per vertex
//! (blanks and `#` comment lines may stand between groups); then, anywhere after the vertices, the words `face`
//! and `hole`, each followed by a run of 16-bit numbers (node numbers from 1) that ends at the first thing that is
//! not a number. A `hole` belongs to the face before it and appends a separator corner (-1) and its own corners.
//! The record layouts used are the mesh's (vertices at +0, polygons at +4, counts at +0x14 and +0x18) and the
//! polygon's: the corner count (16 bits) at +0x10 and the corner array at +0xc, whose elements are 0x24 bytes
//! with the node number first.

use crate::omovie2soft::ReadCalls;
use crate::osoft2vert::VertCalls;
use crate::owave2soft::{fscanf, ToolsCalls};
use crate::stdio::{sprintf, StdioCalls};
use si_mem::{clib, Mem, Outside};

pub const S_POL: u32 = 0x0869_0780;
pub const S_RB: u32 = 0x0869_0787;
pub const MSG_MISSING: u32 = 0x0869_078a;
pub const FMT_WORD_A: u32 = 0x0869_0799;
pub const S_VERTICES: u32 = 0x0869_079e;
pub const MSG_TOO_BIG_A: u32 = 0x0869_07a7;
pub const FMT_XYZ: u32 = 0x0869_07c5;
pub const FMT_WORD_B: u32 = 0x0869_07ce;
pub const S_FACE_B: u32 = 0x0869_07d3;
pub const FMT_SHORT_B: u32 = 0x0869_07d8;
pub const MSG_MANY_B: u32 = 0x0869_07dc;
pub const MSG_TOO_BIG_B: u32 = 0x0869_07fa;
pub const FMT_WORD_C: u32 = 0x0869_0818;
pub const S_FACE_C1: u32 = 0x0869_081d;
pub const S_HOLE_C1: u32 = 0x0869_0822;
pub const S_FACE_C2: u32 = 0x0869_0827;
pub const FMT_SHORT_C: u32 = 0x0869_082c;
pub const MSG_MANY_C: u32 = 0x0869_0830;
pub const S_HOLE_C2: u32 = 0x0869_084e;
pub const FMT_SHORT_D: u32 = 0x0869_0853;
pub const MSG_MANY_HOLE: u32 = 0x0869_0860;
/// `SI_nbPolMax`, the largest polygon count a mesh may have.
pub const NB_POL_MAX: u32 = 0x086d_3920;

/// The strings above, for the check binary's `.rodata`.
pub const STRINGS: &[(u32, &str)] = &[
    (S_POL, "%s.pol"),
    (S_RB, "rb"),
    (MSG_MISSING, "Missing a file"),
    (FMT_WORD_A, "%81s"),
    (S_VERTICES, "vertices"),
    (MSG_TOO_BIG_A, "The Vertigo model is too big!"),
    (FMT_XYZ, "%f %f %f"),
    (FMT_WORD_B, "%81s"),
    (S_FACE_B, "face"),
    (FMT_SHORT_B, "%hd"),
    (MSG_MANY_B, "Too many vertices in polygon!"),
    (MSG_TOO_BIG_B, "The Vertigo model is too big!"),
    (FMT_WORD_C, "%81s"),
    (S_FACE_C1, "face"),
    (S_HOLE_C1, "hole"),
    (S_FACE_C2, "face"),
    (FMT_SHORT_C, "%hd"),
    (MSG_MANY_C, "Too many vertices in polygon!"),
    (S_HOLE_C2, "hole"),
    (FMT_SHORT_D, "%hd"),
    (MSG_MANY_HOLE, "Too many vertices in polygon hole!"),
];

fn blank(c: i32) -> bool {
    matches!(c, 0x20 | 9 | 0xa | 0xd | 0x23)
}

/// Skips the rest of a `#` line. The original has no end-of-file test here and would loop for ever; the port stops
/// at the end of the stream (not reproduced).
fn skip_line(m: &mut Mem, o: &mut dyn Outside, f: u32) -> i32 {
    loop {
        let c = o.fgetc(m, f);
        if c == 0xa || c == -1 {
            return c;
        }
    }
}

/// Reads blanks and `#` lines; the first other byte (or the one read at the end of the stream).
fn next_item(m: &mut Mem, o: &mut dyn Outside, f: u32) -> i32 {
    loop {
        let c = o.fgetc(m, f);
        if blank(c) && o.feof(m, f) == 0 {
            if c == 0x23 {
                skip_line(m, o, f);
            }
            continue;
        }
        return c;
    }
}

/// Reads up to 256 16-bit numbers into `buf`; the count (a number the scan could not make but did not fail on
/// still counts, and so does the failure at the end of the stream), and the last answer of `fscanf`.
fn read_shorts(m: &mut Mem, o: &mut dyn Outside, f: u32, fmt: u32, buf: u32, mut r: i32) -> i32 {
    let mut n = 0i32;
    if r.wrapping_add(1) as u32 > 1 {
        loop {
            r = fscanf(m, o, f, fmt, &[buf.wrapping_add(((n as i16) as i32 as u32) << 1)]);
            if r != 0 {
                n = (n + 1) & 0xffff;
            }
            if r.wrapping_add(1) as u32 <= 1 || (n as i16) > 0xff {
                break;
            }
        }
    }
    n
}

/// [soft 0x081b9740 pmeshReadVertigo]: the mesh made from `<name>.pol`, or 0 (the file is missing, a count is too big, a
/// run of 256 numbers has no end). Everything the original does is done, in its order, including its leaks (the
/// mesh when a polygon has 256 corners). Where it would loop for ever on a file without the word `vertices`, or on
/// a `#` line cut by the end of the stream, the port stops (the first answers 0, the second goes on); a file with
/// more vertex groups than it counted overruns the vertex array in both.
pub fn pmesh_read_vertigo(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    let tok = m.local(0x60);
    let buf1 = m.local(0x200);
    let buf2 = m.local(0x200);
    let slot = m.local(4);
    let r = vertigo(m, o, name, tok, buf1, buf2, slot);
    m.release(4);
    m.release(0x200);
    m.release(0x200);
    m.release(0x60);
    r
}

fn vertigo(m: &mut Mem, o: &mut dyn Outside, name: u32, tok: u32, buf1: u32, buf2: u32, slot: u32) -> u32 {
    let len = clib::strlen(m, name);
    let path = m.si_malloc(len + 5);
    sprintf(m, o, path, S_POL, &[name]);
    let f = o.fopen(m, path, S_RB);
    if f == 0 {
        o.si_status_display_message(m, MSG_MISSING, 3);
        if path != 0 {
            m.free(path);
        }
        return 0;
    }
    if path != 0 {
        m.free(path);
    }
    let mesh = o.si_mesh_allocate(m);
    m.set_u32(slot, mesh);
    // up to the word "vertices"
    loop {
        let r = fscanf(m, o, f, FMT_WORD_A, &[tok]);
        if clib::strcmp(m, tok, S_VERTICES) == 0 {
            break;
        }
        if r == -1 {
            return 0;
        }
    }
    let mut pos = o.ftell(m, f);
    m.set_i32(mesh + 0x14, 0);
    // first pass: count the groups
    loop {
        let c = next_item(m, o, f);
        if c == 0x28 {
            let mut last;
            loop {
                last = o.fgetc(m, f);
                if last == 0x29 || o.feof(m, f) != 0 {
                    break;
                }
            }
            if last == 0x29 {
                m.set_i32(mesh + 0x14, m.i32(mesh + 0x14) + 1);
            }
            continue;
        }
        break;
    }
    if m.i32(mesh + 0x14) > 0x7fff {
        o.si_status_display_message(m, MSG_TOO_BIG_A, 3);
        o.si_mesh_dispose(m, slot);
        return 0;
    }
    let verts = o.si_vertex_allocate(m, m.i32(mesh + 0x14));
    m.set_u32(mesh, verts);
    o.fseek(m, f, pos, 0);
    let mut v = verts;
    loop {
        let c = next_item(m, o, f);
        if c != 0x28 {
            o.ungetc(m, c, f);
            break;
        }
        fscanf(m, o, f, FMT_XYZ, &[v, v.wrapping_add(4), v.wrapping_add(8)]);
        v = v.wrapping_add(0x14);
        loop {
            let c = o.fgetc(m, f);
            if c == 0x29 || o.feof(m, f) != 0 {
                break;
            }
        }
    }
    // second pass: count the polygons
    pos = o.ftell(m, f);
    m.set_i32(mesh + 0x18, 0);
    while o.feof(m, f) == 0 {
        let mut r;
        loop {
            r = fscanf(m, o, f, FMT_WORD_B, &[tok]);
            if r == -1 || clib::strcmp(m, tok, S_FACE_B) == 0 {
                break;
            }
        }
        let n = read_shorts(m, o, f, FMT_SHORT_B, buf1, r);
        if n == 0x100 {
            o.si_status_display_message(m, MSG_MANY_B, 3);
            return 0;
        }
        if n as i16 > 0 {
            m.set_i32(mesh + 0x18, m.i32(mesh + 0x18) + 1);
        }
    }
    let npol = m.i32(mesh + 0x18);
    if npol > m.i32(NB_POL_MAX) {
        o.si_status_display_message(m, MSG_TOO_BIG_B, 3);
        o.si_max_nb_pol_warning(m);
        o.si_mesh_dispose(m, slot);
        return 0;
    }
    let polys = o.si_polygon_allocate(m, npol);
    m.set_u32(mesh + 4, polys);
    o.fseek(m, f, pos, 0);
    let mut p = polys.wrapping_sub(0x18);
    while o.feof(m, f) == 0 {
        let mut r;
        loop {
            r = fscanf(m, o, f, FMT_WORD_C, &[tok]);
            if r == -1 || clib::strcmp(m, tok, S_FACE_C1) == 0 || clib::strcmp(m, tok, S_HOLE_C1) == 0 {
                break;
            }
        }
        if clib::strcmp(m, tok, S_FACE_C2) == 0 {
            p = p.wrapping_add(0x18);
            let n = read_shorts(m, o, f, FMT_SHORT_C, buf1, r);
            if n == 0x100 {
                o.si_status_display_message(m, MSG_MANY_C, 3);
                return 0;
            }
            m.set_u16(p.wrapping_add(0x10), n as u16);
            let sides = n as u32;
            let c = sides.wrapping_add(3).wrapping_sub(sides.wrapping_sub(1) & 3);
            let nodes = m.si_calloc(c, 0x24);
            m.set_u32(p.wrapping_add(0xc), nodes);
            for k in 0..sides {
                let v = (m.i16(buf1 + 2 * k) as i32) - 1;
                m.set_i32(nodes.wrapping_add(k * 0x24), v);
            }
        } else if clib::strcmp(m, tok, S_HOLE_C2) == 0 {
            let n = read_shorts(m, o, f, FMT_SHORT_D, buf2, r);
            if n == 0x100 {
                o.si_status_display_message(m, MSG_MANY_HOLE, 3);
                return 0;
            }
            if n != 0 {
                let cnt = m.u16(p.wrapping_add(0x10)) as i32;
                o.si_nodes_allocate(m, 0, p.wrapping_add(0xc), cnt, n + cnt + 1);
                let nodes = m.u32(p.wrapping_add(0xc));
                let cnt = m.u16(p.wrapping_add(0x10)) as u32;
                m.set_i32(nodes.wrapping_add(cnt * 0x24), -1);
                m.set_u16(p.wrapping_add(0x10), (cnt + 1) as u16);
                for k in 0..n as u32 {
                    let nodes = m.u32(p.wrapping_add(0xc));
                    let cnt = m.u16(p.wrapping_add(0x10)) as u32;
                    let v = (m.i16(buf2 + 2 * k) as i32) - 1;
                    m.set_i32(nodes.wrapping_add(cnt * 0x24), v);
                    m.set_u16(p.wrapping_add(0x10), (cnt + 1) as u16);
                }
            }
        }
    }
    o.fclose(m, f);
    o.si_mesh_update(m, mesh);
    o.si_mesh_init_vertex_colors(m, mesh);
    mesh
}
