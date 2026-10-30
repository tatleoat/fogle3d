// SPDX-License-Identifier: MIT
//! `metaball.c`, the association lists of two metasystems (finding 0038): the "adoptive
//! shape" the shape-to-shape morph builds. A record of 0x4c bytes holds, for each of two
//! systems, its bounding box (six floats) and the list of its metaball nodes (0x18 bytes per
//! entry: the node, its position normalised into the box, a partner slot and a flag); the
//! fill pairs the entries of the two lists, and the adopt step gives every ball of the
//! first system a partner's shape, creating balls where the first list has holes.

use crate::metaball::{copy_f64, current_list, next_son, MetaCalls};
use crate::surface::metaball_set_weight;
use crate::animation::si_metaball_save_internal_parameters;
use si_hrc::image::meta::{metaball_allocate, system};
use si_mem::{Mem, Outside};
use si_model::model_image::mx;
use si_x87::{x, F80};

/// The record (`MetaAdoptiveShape`): the first system, its box (min x, y, z, max x, y, z), the
/// number of ball nodes it has, the second system, its box and count, the entry count (the
/// larger of the two counts) and the two lists.
pub mod rec {
    pub const SYS_A: u32 = 0x00;
    pub const BOX_A: u32 = 0x04;
    pub const COUNT_A: u32 = 0x1c;
    pub const SYS_B: u32 = 0x20;
    pub const BOX_B: u32 = 0x24;
    pub const COUNT_B: u32 = 0x3c;
    pub const N: u32 = 0x40;
    pub const LIST_A: u32 = 0x44;
    pub const LIST_B: u32 = 0x48;
    pub const SIZE: u32 = 0x4c;
}

/// A list entry: the node, three floats (the position), the partner's node, a 16-bit flag
/// (1: paired).
pub mod entry {
    pub const NODE: u32 = 0x00;
    pub const POS: u32 = 0x04;
    pub const PARTNER: u32 = 0x10;
    pub const FLAG: u32 = 0x14;
    pub const SIZE: u32 = 0x18;
}

/// `0.001` as a double (0x0865fc08): added to every box side before the reciprocal.
const SIDE_PAD: f64 = 0.001;
/// The distance of an empty entry (0x0865fc14).
const EMPTY_DISTANCE: f32 = 100.0;
/// The start of every minimum search (0x0865fc10, 0x0865fc18): the largest float.
const FAR: f32 = f32::MAX;
/// `SI_frameCurrent (0x08782040)`, an int.
const SI_FRAME_CURRENT: u32 = 0x0878_2040;
/// The string "ball" (0x086d5e0c): the name of a node made here.
const NAME_BALL: u32 = 0x086d_5e0c;

fn count_balls(m: &Mem, sys: u32) -> i32 {
    let top = m.u32(sys + system::MODEL);
    let (mut n, mut at) = (0, 0u32);
    loop {
        at = next_son(m, top, at);
        if at == 0 {
            return n;
        }
        if m.i32(at + 0x30) == 8 {
            n += 1;
        }
    }
}

/// A fresh list of `n` entries: every field written zero.
fn new_list(m: &mut Mem, n: i32) -> u32 {
    let list = m.si_calloc(n as u32, entry::SIZE);
    let mut at = list;
    for _ in 0..n {
        m.set_u32(at + entry::NODE, 0);
        m.set_u32(at + 0x0c, 0);
        m.set_u32(at + 0x08, 0);
        m.set_u32(at + 0x04, 0);
        m.set_u32(at + entry::PARTNER, 0);
        m.set_u16(at + entry::FLAG, 0);
        at = at.wrapping_add(entry::SIZE);
    }
    list
}

/// One ball node's place in the box of its system: the translation row of its matrix times
/// the inverse of the system's, widening the box by it (a NaN widens nothing). The three
/// floats are copied to the entry.
fn place(m: &mut Mem, boxp: u32, p: [f32; 3], e: u32) {
    for k in 0..3u32 {
        // the minimum: strictly below; the maximum: strictly above (both false for a NaN)
        if p[k as usize] < m.f32(boxp + 4 * k) {
            m.set_f32(boxp + 4 * k, p[k as usize]);
        }
        if p[k as usize] > m.f32(boxp + 12 + 4 * k) {
            m.set_f32(boxp + 12 + 4 * k, p[k as usize]);
        }
        m.set_f32(e + entry::POS + 4 * k, p[k as usize]);
    }
}

/// The scale a side of the box gives: `1 / ((max - min) + 0.001)` in the register, stored as a float.
fn side_scale(max: f32, min: f32) -> f32 {
    (x(1.0f32) / ((x(max) - x(min)) + x(SIDE_PAD))).to_f32()
}

/// [soft 0x0845fac4 metaSystemCreateAssociationList]: the record for the systems `a` and `b`
/// (0 when `SI_malloc` fails). Both boxes start empty; each system's ball nodes (kind 8,
/// found with `SI_modelNextSon`) are counted, the lists made as long as the larger count, and
/// every ball node's translation (its matrix times the inverse of its system's) enters its
/// list in the order of the walk and widens the system's box. Then each position becomes
/// `(p - min) * 1 / ((max - min) + 0.001)` per axis, stored as a float.
///
/// **Differs where the original reads the stack**: a system matrix without an inverse leaves
/// the inverse local as the frame had it (for the second system: as the first inverse left
/// it), and the products of the walk use it. The Rust local holds whatever the image's stack
/// holds there, so such cases are UNINIT in the vectors.
pub fn meta_system_create_association_list(m: &mut Mem, a: u32, b: u32) -> u32 {
    let r = m.si_malloc(rec::SIZE);
    if r == 0 {
        return 0;
    }
    for k in 0..3u32 {
        m.set_f32(r + rec::BOX_A + 4 * k, f32::MAX);
        m.set_f32(r + rec::BOX_A + 12 + 4 * k, -f32::MAX);
        m.set_f32(r + rec::BOX_B + 4 * k, f32::MAX);
        m.set_f32(r + rec::BOX_B + 12 + 4 * k, -f32::MAX);
    }
    m.set_u32(r + rec::SYS_A, a);
    m.set_u32(r + rec::SYS_B, b);
    m.set_i32(r + rec::COUNT_A, 0);
    let ca = count_balls(m, a);
    m.set_i32(r + rec::COUNT_A, ca);
    let cb = count_balls(m, b);
    m.set_i32(r + rec::COUNT_B, cb);
    let n = ca.max(cb);
    m.set_i32(r + rec::N, n);
    // locals of the original, in its frame: the inverse, the product, the three scales
    let inv = m.local(0x40);
    let prod = m.local(0x40);
    let scale = m.local(12);
    for (sys, boxp, list_at, count) in [(a, rec::BOX_A, rec::LIST_A, ca), (b, rec::BOX_B, rec::LIST_B, cb)] {
        let list = new_list(m, n);
        m.set_u32(r + list_at, list);
        let top = m.u32(sys + system::MODEL);
        mx::inverse(m, top + 0x1c4, inv);
        let (mut at, mut idx) = (0u32, 0u32);
        loop {
            at = next_son(m, top, at);
            if at == 0 {
                break;
            }
            if m.i32(at + 0x30) != 8 {
                continue;
            }
            mx::multiply(m, at + 0x1c4, inv, prod);
            let p = [m.f32(prod + 0x30), m.f32(prod + 0x34), m.f32(prod + 0x38)];
            let e = m.u32(r + list_at).wrapping_add(idx);
            place(m, r + boxp, p, e);
            m.set_u32(e + entry::NODE, at);
            idx = idx.wrapping_add(entry::SIZE);
        }
        for k in 0..3u32 {
            let s = side_scale(m.f32(r + boxp + 12 + 4 * k), m.f32(r + boxp + 4 * k));
            m.set_f32(scale + 4 * k, s);
        }
        for i in 0..count as u32 {
            let e = m.u32(r + list_at).wrapping_add(i * entry::SIZE);
            for k in 0..3u32 {
                let v = ((x(m.f32(e + entry::POS + 4 * k)) - x(m.f32(r + boxp + 4 * k))) * x(m.f32(scale + 4 * k))).to_f32();
                m.set_f32(e + entry::POS + 4 * k, v);
            }
        }
    }
    m.release(0x40);
    m.release(0x40);
    m.release(12);
    r
}

/// The distance of two positions in the register: `sqrt((dy*dy + dx*dx) + dz*dz)`.
fn distance(p: [f32; 3], q: [f32; 3]) -> F80 {
    let dx = x(p[0]) - x(q[0]);
    let dy = x(p[1]) - x(q[1]);
    let dz = x(p[2]) - x(q[2]);
    ((dy * dy + dx * dx) + dz * dz).sqrt()
}

fn pos(m: &Mem, e: u32) -> [f32; 3] {
    [m.f32(e + entry::POS), m.f32(e + entry::POS + 4), m.f32(e + entry::POS + 8)]
}

/// [soft 0x0845ffd8 metaSystemFillAssociationList]: pairs the entries of the two lists,
/// greedily. Each round picks the unpaired entry of the first list with the smallest key
/// (strictly less than the best so far, which starts at the largest float; an entry without
/// a node has the key 100; otherwise, in mode 0, the distance of its position from the
/// origin, else a number from `drand48`, as a float), then the unpaired entry of the
/// second list with the smallest key (mode 0: its distance from the origin plus its
/// distance from the first entry), marks both paired and gives each the other's node.
/// Rounds go on until the first list has no entry the search accepts (a NaN key is never
/// accepted).
///
/// **Differs where the original overruns**: when the second list offers nothing (every key
/// NaN) the original stores through index -1, 0x18 bytes before the list, and hands the first
/// entry a stack word it never set. Here the first entry gets the word the Rust local holds
/// and the stores before the list are left out. Such cases are UNINIT/UNDERRUN in the vectors.
pub fn meta_system_fill_association_list(m: &mut Mem, r: u32, mode: i32, rand: &mut dyn FnMut() -> f64) {
    let origin = [0.0f32; 3];
    // the original's unset local: the node it hands the first entry when nothing is picked
    let slot = m.local(4);
    let mut bnode = m.u32(slot);
    loop {
        let n = m.i32(r + rec::N);
        let mut best = FAR;
        let (mut ia, mut anode) = (-1i32, 0u32);
        let mut i = 0i32;
        while i < n {
            let e = m.u32(r + rec::LIST_A).wrapping_add((i as u32).wrapping_mul(entry::SIZE));
            if m.u16(e + entry::FLAG) == 0 {
                let mut d = EMPTY_DISTANCE;
                if m.u32(e + entry::NODE) != 0 {
                    d = if mode == 0 { distance(origin, pos(m, e)).to_f32() } else { x(rand()).to_f32() };
                }
                if d < best {
                    ia = i;
                    anode = m.u32(e + entry::NODE);
                    best = d;
                }
            }
            i += 1;
        }
        if ia == -1 {
            break;
        }
        let ea = m.u32(r + rec::LIST_A).wrapping_add((ia as u32).wrapping_mul(entry::SIZE));
        let mut best = FAR;
        let mut ib = -1i32;
        let mut j = 0i32;
        while j < n {
            let e = m.u32(r + rec::LIST_B).wrapping_add((j as u32).wrapping_mul(entry::SIZE));
            if m.u16(e + entry::FLAG) == 0 {
                let mut d = EMPTY_DISTANCE;
                if m.u32(e + entry::NODE) != 0 {
                    d = if mode == 0 {
                        (distance(origin, pos(m, e)) + distance(pos(m, ea), pos(m, e))).to_f32()
                    } else {
                        x(rand()).to_f32()
                    };
                }
                if d < best {
                    ib = j;
                    bnode = m.u32(e + entry::NODE);
                    best = d;
                }
            }
            j += 1;
        }
        let ea = m.u32(r + rec::LIST_A).wrapping_add((ia as u32).wrapping_mul(entry::SIZE));
        m.set_u16(ea + entry::FLAG, 1);
        m.set_u32(ea + entry::PARTNER, bnode);
        // OVERRUN in the original: with ib == -1 it stores the flag and the partner 0x18 bytes
        // before the list (in the heap block's header). The Rust leaves that memory alone.
        if ib != -1 {
            let eb = m.u32(r + rec::LIST_B).wrapping_add((ib as u32).wrapping_mul(entry::SIZE));
            m.set_u16(eb + entry::FLAG, 1);
            m.set_u32(eb + entry::PARTNER, anode);
        }
    }
    m.release(4);
}

/// [soft 0x084601f8 metaAdoptShapeDispose]: frees the two lists and the record that `*p`
/// points to, and clears `*p`. Nothing happens when `*p` is null.
pub fn meta_adopt_shape_dispose(m: &mut Mem, p: u32) {
    if m.u32(p) == 0 {
        return;
    }
    let l = m.u32(m.u32(p) + rec::LIST_A);
    if l != 0 {
        m.free(l);
        m.set_u32(m.u32(p) + rec::LIST_A, 0);
    }
    let l = m.u32(m.u32(p) + rec::LIST_B);
    if l != 0 {
        m.free(l);
        m.set_u32(m.u32(p) + rec::LIST_B, 0);
    }
    if m.u32(p) != 0 {
        m.free(m.u32(p));
        m.set_u32(p, 0);
    }
}

/// The node's scale, rotation and translation taken from `src`'s (offsets 0x100, 0x10c, 0x158).
fn take_placement(m: &mut Mem, o: &mut dyn Outside, node: u32, src: u32) {
    o.si_model_set_scaling(m, node, src + 0x100, 0, 0);
    o.si_model_set_rotation(m, node, src + 0x10c, 0, 0);
    o.si_model_set_translation(m, node, src + 0x158, 0, 0);
}

/// [soft 0x08460260 metaSystemAdoptShape]: for every entry of the first list, last to first,
/// the ball node it names takes the shape of its partner (the node in the entry's partner
/// slot): scale, rotation and translation of the partner's node and, in the ball, the
/// partner's weight (as a float) through `metaballSetWeight` and its three exponents.
/// An entry without a node gets a new node "ball" first (made like `SI_addMetaball` does,
/// parented to the first system's graf entry), placed like the first node of the first list
/// nearest to the entry's partner; its weight is 0, keyed at the current curve's next and
/// previous keys (found on the first listed node's `0xf2` curve) with the ball's values.
/// An entry whose partner slot is empty takes the nearest second-list node's placement and
/// weight 0 instead.
///
/// **Differs where the original reads the stack**: the next/previous key answers when the
/// first listed node has no element or curve, and the partner's index when no second-list node
/// is the partner's, are the frame's leftovers (UNINIT in the vectors).
pub fn meta_system_adopt_shape(m: &mut Mem, o: &mut dyn Outside, r: u32) {
    // the frame: [0] key answers (next, previous), [4] next frame, [8] previous frame, [12] id,
    // [16] the partner's index
    let loc = m.local(0x14);
    let (ret_at, next_at, prev_at, id_at, match_at) = (loc, loc + 4, loc + 8, loc + 12, loc + 16);
    let mut closest = 0u32;
    let first = m.u32(r + rec::SYS_A);
    let n = m.i32(r + rec::N);
    let mut i = 0i32;
    while i < n {
        let node = m.u32(m.u32(r + rec::LIST_A).wrapping_add((i as u32).wrapping_mul(entry::SIZE)));
        if node != 0 {
            let elem = o.si_ed_get_elem_from_model(m, node, id_at);
            let fcv = o.si_ed_find_fcv(m, elem, 2, 0xf2);
            if fcv != 0 {
                let frame = m.i32(SI_FRAME_CURRENT);
                let nx = o.si_fcurve_next_key(m, fcv, frame, next_at);
                m.set_u16(ret_at, nx as u16);
                let frame = m.i32(SI_FRAME_CURRENT);
                let pv = o.si_fcurve_previous_key(m, fcv, frame, prev_at);
                m.set_u16(ret_at + 2, pv as u16);
            }
            break;
        }
        i += 1;
    }
    let list = current_list(m);
    let parent = o.si_find_gl_data(m, list, m.u32(first + system::MODEL), 2);
    let mut i = m.i32(r + rec::N) - 1;
    while i >= 0 {
        let off = (i as u32).wrapping_mul(entry::SIZE);
        let mut node = m.u32(m.u32(r + rec::LIST_A).wrapping_add(off));
        if node == 0 {
            node = o.si_model_allocate(m, NAME_BALL);
            let ball = metaball_allocate(m, node);
            o.si_model_add_definition(m, node, 8, ball);
            let list = current_list(m);
            let gl = o.si_graflist_add_data(m, list, node, 2, 1);
            o.si_model_create_parent(m, parent, gl, 0);
            let n = m.i32(r + rec::N);
            let partner = m.u32(m.u32(r + rec::LIST_A).wrapping_add(off + entry::PARTNER));
            let mut k = 0i32;
            while k < n {
                if m.u32(m.u32(r + rec::LIST_B).wrapping_add((k as u32).wrapping_mul(entry::SIZE))) == partner {
                    m.set_i32(match_at, k);
                }
                k += 1;
            }
            let mut best = FAR;
            let mut k = 0i32;
            let n = m.i32(r + rec::N);
            while k < n {
                let e = m.u32(r + rec::LIST_A).wrapping_add((k as u32).wrapping_mul(entry::SIZE));
                if m.u32(e + entry::NODE) != 0 {
                    let eb = m.u32(r + rec::LIST_B).wrapping_add((m.i32(match_at) as u32).wrapping_mul(entry::SIZE));
                    let d = distance(pos(m, e), pos(m, eb)).to_f32();
                    if d < best {
                        best = d;
                        closest = m.u32(e + entry::NODE);
                    }
                }
                k += 1;
            }
            if closest != 0 {
                take_placement(m, o, node, closest);
            }
            let saved = m.u32(SI_FRAME_CURRENT);
            metaball_set_weight(m, o, ball, 0.0);
            if m.u16(ret_at) != 0 {
                let f = m.i32(next_at);
                si_metaball_save_internal_parameters(m, o, ball, f);
                m.set_i32(SI_FRAME_CURRENT, f);
                o.si_savekey_object_all(m, node);
            }
            if m.u16(ret_at + 2) != 0 {
                let f = m.i32(prev_at);
                si_metaball_save_internal_parameters(m, o, ball, f);
                m.set_i32(SI_FRAME_CURRENT, f);
                o.si_savekey_object_all(m, node);
            }
            m.set_u32(SI_FRAME_CURRENT, saved);
        }
        let partner = m.u32(m.u32(r + rec::LIST_A).wrapping_add(off + entry::PARTNER));
        let n = m.i32(r + rec::N);
        let (src, weight) = if partner != 0 {
            take_placement(m, o, node, partner);
            let src = m.u32(partner + 0x48);
            (src, Some(x(m.f64(src + 0xd4)).to_f32()))
        } else {
            let mut best = FAR;
            let mut near = 0u32;
            let mut k = 0i32;
            if n > 0 {
                while k < n {
                    let e = m.u32(r + rec::LIST_B).wrapping_add((k as u32).wrapping_mul(entry::SIZE));
                    if m.u32(e + entry::NODE) != 0 {
                        let ea = m.u32(r + rec::LIST_A).wrapping_add(off);
                        let d = distance(pos(m, ea), pos(m, e)).to_f32();
                        if d < best {
                            best = d;
                            near = m.u32(e + entry::NODE);
                        }
                    }
                    k += 1;
                }
            }
            if near != 0 {
                take_placement(m, o, node, near);
                (m.u32(near + 0x48), Some(0.0f32))
            } else {
                (0, None)
            }
        };
        if let Some(w) = weight {
            let dst = m.u32(node + 0x48);
            metaball_set_weight(m, o, dst, w);
            for k in [0xdc, 0xe4, 0xec] {
                copy_f64(m, src + k, dst + k);
            }
        }
        i -= 1;
    }
    m.release(0x14);
}
