// SPDX-License-Identifier: MIT
//! `omovie2soft.c` (finding 0039): the text model file reader `si_movieToSoftimage`, which turns a file of
//! object ranges, nodes and polygons into one model per object.
//!
//! The file, read from the listing: a first line `objects nodes polygons` (three decimal numbers, read from the
//! first 100 bytes of the first line), then one line per object `first last` (polygon numbers from 1, kept from
//! 0), then the nodes (`%g %g %g`), then the polygons, each a run of decimal numbers that ends at the first number
//! that is not positive (a positive number is a node number from 1, the last number `-n` stands for node `n`,
//! and `0` for node `-1`). A polygon keeps at most four nodes; its side count is the count of numbers read.
//! The record layouts used: a **model** holds its name at +4, its definition kind at +0x30 and its definition
//! record's kind count at +0x34 (16 bits), the definition list at +0x38 and the mesh at +0x48; a **mesh** holds
//! the vertices at +0 (0x14 bytes each, three floats first), the polygons at +4 (0x18 bytes each), the vertex
//! count at +0x14 and the polygon count at +0x18; the reader's own **polygon record** is five numbers (four node
//! numbers and the side count) of 0x14 bytes.

use crate::owave2soft::{fscanf, print_message};
use crate::stdio::{sprintf, sscanf};
use si_mem::{clib, outside_calls, Mem, Outside, Routine};
#[allow(unused_imports)]
use crate::owave2soft::ToolsCalls as _;
#[allow(unused_imports)]
use crate::stdio::StdioCalls as _;

outside_calls! {
    /// What the converters' readers call in other files and nobody has ported here, besides [`ToolsCalls`].
    pub trait ReadCalls {
        /// `fgetc(stream)` (C library): the next byte, or -1.
        fn fgetc(stream: u32) -> i32 = "fgetc", 0x080c_3828;
        /// `ungetc(c, stream)` (C library).
        fn ungetc(c: i32, stream: u32) -> i32 = "ungetc", 0x080c_39a8;
        /// `SI_nodesAllocate (0x08453b9c)`, `meshEditFct.c`: (model, pointer to the corner array, count, wanted).
        fn si_nodes_allocate(model: u32, pp: u32, count: i32, n: i32) -> i32 = "SI_nodesAllocate", 0x0845_3b9c;
    }
}

/// `SI_polygonInitialize (0x0845a694)`, `mesh_b.c`, variadic: (polygon, sides, then one node number per side).
const POLYGON_INITIALIZE: Routine = Routine::new("SI_polygonInitialize", 0x0845_a694);

pub const S_R: u32 = 0x0869_04c0;
pub const MSG_CANNOT: u32 = 0x0869_04c2;
pub const FMT_HEADER: u32 = 0x0869_04de;
pub const MSG_INVALID_HEADER: u32 = 0x0869_04e8;
pub const MSG_NEED_OBJECT: u32 = 0x0869_04f5;
pub const MSG_NEED_NODES: u32 = 0x0869_050e;
pub const MSG_NEED_POLYGON: u32 = 0x0869_0528;
pub const MSG_OOM_FIRST: u32 = 0x0869_0542;
pub const MSG_OOM_LAST: u32 = 0x0869_0550;
pub const FMT_RANGE: u32 = 0x0869_055e;
pub const MSG_INVALID_RANGE: u32 = 0x0869_0564;
pub const MSG_COUNTS: u32 = 0x0869_0580;
pub const MSG_OOM_NODES: u32 = 0x0869_05a6;
pub const FMT_NODE: u32 = 0x0869_05b4;
pub const MSG_INVALID_NODE: u32 = 0x0869_05bd;
pub const MSG_OOM_POLYS: u32 = 0x0869_05ca;
pub const FMT_INT: u32 = 0x0869_05d8;
pub const MSG_INVALID_POLY: u32 = 0x0869_05db;
pub const MSG_SIDES: u32 = 0x0869_0600;
pub const MSG_INVALID_OBJECTS: u32 = 0x0869_0620;
pub const FMT_NAME: u32 = 0x0869_0640;
pub const FMT_LABEL: u32 = 0x0869_0645;
pub const MSG_OOM_MODEL: u32 = 0x0869_064f;
pub const MSG_OOM_MESH: u32 = 0x0869_065d;
pub const MSG_OOM_DEF: u32 = 0x0869_066b;
pub const MSG_OOM_VERTS: u32 = 0x0869_0679;
pub const MSG_OOM_POLYGONS: u32 = 0x0869_0687;
pub const FMT_OBJECT: u32 = 0x0869_0695;
pub const MSG_NO_OBJECT: u32 = 0x0869_06b2;

/// The strings above, for the check binary's `.rodata`.
pub const STRINGS: &[(u32, &str)] = &[
    (S_R, "r"),
    (MSG_CANNOT, "Cannot read input file : %s"),
    (FMT_HEADER, "%d %d %d\n"),
    (MSG_INVALID_HEADER, "Invalid file"),
    (MSG_NEED_OBJECT, "Need at least one object"),
    (MSG_NEED_NODES, "Need at least three nodes"),
    (MSG_NEED_POLYGON, "Need at least one polygon"),
    (MSG_OOM_FIRST, "Out of memory"),
    (MSG_OOM_LAST, "Out of memory"),
    (FMT_RANGE, "%d %d"),
    (MSG_INVALID_RANGE, "Invalid file"),
    (MSG_COUNTS, "%d object(s)   %d nodes   %d polygons"),
    (MSG_OOM_NODES, "Out of memory"),
    (FMT_NODE, "%g %g %g"),
    (MSG_INVALID_NODE, "Invalid file"),
    (MSG_OOM_POLYS, "Out of memory"),
    (FMT_INT, "%d"),
    (MSG_INVALID_POLY, "Invalid file"),
    (MSG_SIDES, "Invalid # of sides for polygon"),
    (MSG_INVALID_OBJECTS, "There's %d invalid(s) object(s)"),
    (FMT_NAME, "%s%d"),
    (FMT_LABEL, "Object %d"),
    (MSG_OOM_MODEL, "Out of memory"),
    (MSG_OOM_MESH, "Out of memory"),
    (MSG_OOM_DEF, "Out of memory"),
    (MSG_OOM_VERTS, "Out of memory"),
    (MSG_OOM_POLYGONS, "Out of memory"),
    (FMT_OBJECT, "%s: %8d nodes   %8d polygons"),
    (MSG_NO_OBJECT, "Invalid file"),
];

/// [soft 0x081b8540 si_modelSetDefinition]: the model's definition kind (+0x30) is set first; then a 4-byte list is
/// made (`SI_malloc`) holding the definition, which is also the model's mesh (+0x48), and the kind count (+0x34, 16
/// bits) is 1. Answers 0, or -2 when the list cannot be made.
pub fn si_model_set_definition(m: &mut Mem, model: u32, kind: i32, def: u32) -> i32 {
    m.set_u32(model + 0x30, kind as u32);
    let list = m.si_malloc(4);
    m.set_u32(model + 0x38, list);
    if list == 0 {
        return -2;
    }
    m.set_u32(list, def);
    m.set_u32(model + 0x48, def);
    m.set_u16(model + 0x34, 1);
    0
}

fn free_if(m: &mut Mem, p: u32) {
    if p != 0 {
        m.free(p);
    }
}

/// [soft 0x081b8584 si_movieToSoftimage]: reads the file named `path`; one model per valid object, all after the
/// first made children of the first; answers the first model, or 0 when the file cannot be used. `verbose` (16
/// bits) prints the counts and one line per object.
///
/// Where the original reads memory it does not own, the port reads the same address: an object range outside the
/// polygon list, or a first polygon after the last one (the count of nodes is then one, from the first node
/// number). `path` longer than 75 bytes overruns the original's 80-byte name buffer; the port gives it room
/// (not reproduced). The vertex floats are copied bit for bit (the original's `fld`/`fstp` would quiet a
/// signalling NaN, which `%g` never reads).
pub fn si_movie_to_softimage(m: &mut Mem, o: &mut dyn Outside, path: u32, verbose: i16) -> u32 {
    let line = m.local(100);
    let counts = m.local(12);
    let one = m.local(4);
    let namebuf = m.local(0x200);
    let label = m.local(0x100);
    let r = movie(m, o, path, verbose, line, counts, one, namebuf, label);
    m.release(0x100);
    m.release(0x200);
    m.release(4);
    m.release(12);
    m.release(100);
    r
}

#[allow(clippy::too_many_arguments)]
fn movie(m: &mut Mem, o: &mut dyn Outside, path: u32, verbose: i16, line: u32, counts: u32, one: u32, namebuf: u32, label: u32) -> u32 {
    let (pn_obj, pn_nodes, pn_polys) = (counts, counts + 4, counts + 8);
    let mut first_model = 0u32;
    let mut invalid = 0i32;
    let f = o.fopen(m, path, S_R);
    if f == 0 {
        print_message(m, o, 3, MSG_CANNOT, &[path]);
        return 0;
    }
    o.fgets(m, line, 100, f);
    if o.feof(m, f) != 0 || sscanf(m, o, line, FMT_HEADER, &[pn_obj, pn_nodes, pn_polys]) != 3 {
        print_message(m, o, 3, MSG_INVALID_HEADER, &[]);
        o.fclose(m, f);
        return 0;
    }
    let (nobj, nnodes, npoly) = (m.i32(pn_obj), m.i32(pn_nodes), m.i32(pn_polys));
    if nobj <= 0 {
        print_message(m, o, 3, MSG_NEED_OBJECT, &[nobj as u32]);
        o.fclose(m, f);
        return 0;
    }
    if nnodes <= 2 {
        print_message(m, o, 3, MSG_NEED_NODES, &[nnodes as u32]);
        o.fclose(m, f);
        return 0;
    }
    if npoly <= 0 {
        print_message(m, o, 3, MSG_NEED_POLYGON, &[npoly as u32]);
        o.fclose(m, f);
        return 0;
    }
    let firsts = m.si_malloc((nobj as u32) << 2);
    if firsts == 0 {
        print_message(m, o, 3, MSG_OOM_FIRST, &[]);
        o.fclose(m, f);
        return 0;
    }
    let lasts = m.si_malloc((nobj as u32) << 2);
    if lasts == 0 {
        print_message(m, o, 3, MSG_OOM_LAST, &[]);
        o.fclose(m, f);
        free_if(m, firsts);
        return 0;
    }
    for i in 0..nobj as u32 {
        if o.feof(m, f) != 0 || fscanf(m, o, f, FMT_RANGE, &[firsts + 4 * i, lasts + 4 * i]) != 2 {
            print_message(m, o, 3, MSG_INVALID_RANGE, &[]);
            o.fclose(m, f);
            free_if(m, firsts);
            free_if(m, lasts);
            return 0;
        }
        m.set_i32(firsts + 4 * i, m.i32(firsts + 4 * i).wrapping_sub(1));
        m.set_i32(lasts + 4 * i, m.i32(lasts + 4 * i).wrapping_sub(1));
    }
    if verbose != 0 {
        print_message(m, o, 1, MSG_COUNTS, &[nobj as u32, nnodes as u32, npoly as u32]);
    }
    let nodes = m.si_malloc((nnodes as u32).wrapping_mul(12));
    if nodes == 0 {
        print_message(m, o, 3, MSG_OOM_NODES, &[]);
        o.fclose(m, f);
        free_if(m, firsts);
        free_if(m, lasts);
        return 0;
    }
    // the original frees whatever it holds, in this order, on every way out
    let free_all = |m: &mut Mem, nodes: u32, polys: u32| {
        free_if(m, firsts);
        free_if(m, lasts);
        free_if(m, nodes);
        free_if(m, polys);
    };
    for i in 0..nnodes as u32 {
        let p = nodes + 12 * i;
        if o.feof(m, f) != 0 || fscanf(m, o, f, FMT_NODE, &[p, p + 4, p + 8]) != 3 {
            print_message(m, o, 3, MSG_INVALID_NODE, &[]);
            o.fclose(m, f);
            free_all(m, nodes, 0);
            return 0;
        }
    }
    let polys = m.si_malloc((npoly as u32).wrapping_mul(20));
    if polys == 0 {
        print_message(m, o, 3, MSG_OOM_POLYS, &[]);
        o.fclose(m, f);
        free_all(m, nodes, 0);
        return 0;
    }
    for i in 0..npoly as u32 {
        let base = polys + 20 * i;
        let mut n = 0i32;
        loop {
            if o.feof(m, f) != 0 || fscanf(m, o, f, FMT_INT, &[one]) != 1 {
                print_message(m, o, 3, MSG_INVALID_POLY, &[]);
                o.fclose(m, f);
                free_all(m, nodes, polys);
                return 0;
            }
            let v = m.i32(one);
            if n <= 3 {
                m.set_i32(base + 4 * n as u32, if v > 0 { v - 1 } else { !v });
            }
            n += 1;
            if v <= 0 {
                break;
            }
        }
        m.set_i32(base + 0x10, n);
        if n == 3 {
            m.set_i32(base + 0xc, -1);
        }
    }
    // the objects whose polygons all have three or four sides stay valid; the others get last = -1
    for i in 0..nobj as u32 {
        let mut v = m.i32(firsts + 4 * i);
        while v <= m.i32(lasts + 4 * i) {
            let sides = m.i32(polys.wrapping_add((v as u32).wrapping_mul(20)).wrapping_add(0x10));
            if (sides.wrapping_sub(3) as u32) > 1 {
                m.set_i32(lasts + 4 * i, -1);
                invalid += 1;
                print_message(m, o, 2, MSG_SIDES, &[]);
                break;
            }
            v += 1;
        }
    }
    if invalid > 0 {
        print_message(m, o, 2, MSG_INVALID_OBJECTS, &[invalid as u32]);
    }
    let mut k = 0i32;
    for i in 0..nobj as u32 {
        if m.i32(lasts + 4 * i) <= 0 {
            continue;
        }
        k += 1;
        sprintf(m, o, namebuf, FMT_NAME, &[path, k as u32]);
        sprintf(m, o, label, FMT_LABEL, &[k as u32]);
        let model = o.si_model_allocate(m, 0);
        if model == 0 {
            print_message(m, o, 3, MSG_OOM_MODEL, &[]);
            o.fclose(m, f);
            free_all(m, nodes, polys);
            return 0;
        }
        let mesh = o.si_mesh_allocate(m);
        if mesh == 0 {
            print_message(m, o, 3, MSG_OOM_MESH, &[]);
            o.fclose(m, f);
            m.free(model);
            free_all(m, nodes, polys);
            return 0;
        }
        if si_model_set_definition(m, model, 4, mesh) == -2 {
            print_message(m, o, 3, MSG_OOM_DEF, &[]);
            o.fclose(m, f);
            m.free(model);
            m.free(mesh);
            free_all(m, nodes, polys);
            return 0;
        }
        let dup = clib::strdup(m, namebuf);
        m.set_u32(model + 4, dup);
        let (first, last) = (m.i32(firsts + 4 * i), m.i32(lasts + 4 * i));
        // the smallest non-negative node number and the largest, over the object's polygons
        let at = |m: &Mem, p: i32, t: u32| m.i32(polys.wrapping_add((p as u32).wrapping_mul(20)).wrapping_add(4 * t));
        let mut lo = at(m, first, 0);
        let mut hi = lo;
        if first <= last {
            let mut p = first;
            while p <= last {
                for t in 0..4 {
                    let v = at(m, p, t);
                    if v < lo && v >= 0 {
                        lo = v;
                    }
                    if v > hi {
                        hi = v;
                    }
                }
                p += 1;
            }
        }
        let nv = hi.wrapping_sub(lo).wrapping_add(1);
        m.set_i32(mesh + 0x14, nv);
        let verts = o.si_vertex_allocate(m, nv);
        m.set_u32(mesh, verts);
        if verts == 0 {
            print_message(m, o, 3, MSG_OOM_VERTS, &[]);
            o.fclose(m, f);
            free_all(m, nodes, polys);
            return 0;
        }
        let mut j = lo;
        let mut dst = verts;
        while j <= hi {
            let src = nodes.wrapping_add((j as u32).wrapping_mul(12));
            for t in 0..3 {
                m.set_u32(dst + 4 * t, m.u32(src.wrapping_add(4 * t)));
            }
            j += 1;
            dst = dst.wrapping_add(0x14);
        }
        let np = last.wrapping_sub(first).wrapping_add(1);
        m.set_i32(mesh + 0x18, np);
        let out = o.si_polygon_allocate(m, np);
        m.set_u32(mesh + 4, out);
        if out == 0 {
            print_message(m, o, 3, MSG_OOM_POLYGONS, &[]);
            o.fclose(m, f);
            free_all(m, nodes, polys);
            return 0;
        }
        let mut dst = out;
        let mut p = first;
        while p <= last {
            let src = polys.wrapping_add((p as u32).wrapping_mul(20));
            for t in 0..4 {
                m.set_i32(src + 4 * t, m.i32(src + 4 * t).wrapping_sub(lo));
            }
            match m.i32(src + 0x10) {
                3 => {
                    let w = [dst, 3, m.u32(src), m.u32(src + 4), m.u32(src + 8)];
                    o.call(m, POLYGON_INITIALIZE, &w);
                }
                4 => {
                    let w = [dst, 4, m.u32(src), m.u32(src + 4), m.u32(src + 8), m.u32(src + 12)];
                    o.call(m, POLYGON_INITIALIZE, &w);
                }
                _ => {}
            }
            dst = dst.wrapping_add(0x18);
            p += 1;
        }
        o.si_mesh_update(m, mesh);
        if k == 1 {
            first_model = model;
        } else {
            o.si_model_create_parentship(m, first_model, model);
        }
        if verbose != 0 {
            print_message(m, o, 1, FMT_OBJECT, &[label, nv as u32, np as u32]);
        }
    }
    o.fclose(m, f);
    free_all(m, nodes, polys);
    if first_model == 0 {
        print_message(m, o, 3, MSG_NO_OBJECT, &[]);
    }
    first_model
}
