// SPDX-License-Identifier: MIT
//! A node's definitions and what deforms them, on the image: the stored definitions and
//! their array, the lattice arrays, the fitting queries, what counts as deformed and what
//! the transformation tools may do to a node.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x;

use super::{callee, elem_dict, g, models_chapter, Outside};

/// A node's active spline deformation [soft 0x08472b44 SI_modelSplineFitting]: the one of
/// set 1 if it is there and its first word (16 bits) is not zero, else the one of set 0
/// likewise; the set's number is stored at `set` only when one is found.
pub fn spline_fitting(m: &mut Mem, node: Ptr32<Model>, set: u32) -> u32 {
    let f = m[node].spline_fit;
    active_of_two(m, f[0].addr(), f[1].addr(), set)
}

/// A node's active patch deformation [soft 0x08472c3c SI_modelPatchFitting], as
/// [`spline_fitting`].
pub fn patch_fitting(m: &mut Mem, node: Ptr32<Model>, set: u32) -> u32 {
    let f = m[node].patch_fit;
    active_of_two(m, f[0].addr(), f[1].addr(), set)
}

fn active_of_two(m: &mut Mem, f0: u32, f1: u32, set: u32) -> u32 {
    if f1 != 0 && m.i16(f1) != 0 {
        m.set_i16(set, 1);
        f1
    } else if f0 != 0 && m.i16(f0) != 0 {
        m.set_i16(set, 0);
        f0
    } else {
        0
    }
}

/// A node's active lattice [soft 0x0843ff1c SI_modelLattice] (`lattice.c`): the first
/// lattice of set 1 if the set has any and that lattice's first word is not zero, else the
/// first of set 0 likewise. The array pointer is not tested, only the count.
pub fn lattice(m: &mut Mem, node: Ptr32<Model>, set: u32) -> u32 {
    if m[node].nb_lattices1 != 0 {
        let l = m.u32(m[node].lattices1.addr());
        if m.i16(l) != 0 {
            m.set_i16(set, 1);
            return m.u32(m[node].lattices1.addr());
        }
    }
    if m[node].nb_lattices0 != 0 {
        let l = m.u32(m[node].lattices0.addr());
        if m.i16(l) != 0 {
            m.set_i16(set, 0);
            return m.u32(m[node].lattices0.addr());
        }
    }
    0
}

/// Gives a node a name [soft 0x0846db14 SI_modelChangeName]: bit 0 of both update flag
/// words; unless `name` is the pointer the node already has, the old name is freed and the
/// node gets a copy (`strdup`).
pub fn change_name(m: &mut Mem, node: Ptr32<Model>, name: u32) {
    let old = m[node].name.addr();
    m.or_u8(node.addr() + 0x0a, 1);
    m.or_u8(node.addr() + 0x3fa, 1);
    if name != old {
        if old != 0 {
            m.free(old);
            m[node].name = Ptr32::NULL;
        }
        let copy = m.strdup(name);
        m[node].name = Ptr32::new(copy);
    }
}

/// The nearest ancestor of a displayed node's model that is displayed and not selected
/// [soft 0x08463710 SI_modelUnselectedParent]. `graf` is a `__SI_GrafNode` (its word at +4
/// is the model; the word at +8 of a displayed node is not zero while it is selected: both
/// offsets are named ones, the record has no layout here). Walks up from the model: the
/// father's displayed node is looked up (`SI_findGlData(list, father, 2)`); none ends the
/// search with 0; a selected one moves the walk to that father. The first one not
/// selected is stored at `out`, answer 1.
pub fn unselected_parent(m: &mut Mem, o: &mut dyn Outside, graf: u32, out: u32) -> u32 {
    let mut at: Ptr32<Model> = Ptr32::new(m.u32(graf + 4));
    loop {
        let father = m[at].father.addr();
        let list = m[g::SI_GRAF_LISTS].wrapping_add((m.i32(m[g::SI_PROC_DATA].addr() + 4) as u32).wrapping_mul(0x24));
        let found = o.call(m, &callee::SI_FIND_GL_DATA, &[list, father, 2]);
        if found == 0 {
            return 0;
        }
        if m.u32(found + 8) != 0 {
            at = m[at].father;
            continue;
        }
        m.set_u32(out, found);
        return 1;
    }
}

/// Inserts a stored definition at a position [soft 0x084677c8 SI_modelAddIDefinition].
///
/// The position is brought into 0 ..= count. A node without definitions takes `kind` and
/// gets an array of one pointer (`SI_calloc(1, 4)`), position 0; otherwise the array grows
/// by one pointer (`SI_realloc`), the pointers from the position on move up by one. Then
/// the new pointer is stored, the count goes up, the node's name is "changed" to itself
/// ([`change_name`]: the flag bits), and if the working definition was the one that had
/// been first (null for a node without definitions), it becomes the one that is first now.
/// Answers 1.
pub fn add_idefinition(m: &mut Mem, node: Ptr32<Model>, position: i16, kind: i32, definition: u32) -> u32 {
    let mut position = position;
    if position < 1 {
        position = 0;
    } else if m[node].nb_defs as i32 + 1 <= position as i32 {
        position = (m[node].nb_defs as i16).wrapping_add(1);
    }
    let first;
    if m[node].nb_defs == 0 {
        position = 0;
        m[node].kind = kind;
        let block = m.si_calloc(1, 4);
        m[node].defs = Ptr32::new(block);
        first = 0;
    } else {
        first = m.u32(m[node].defs.addr());
        let block = m.si_realloc(m[node].defs.addr(), m[node].nb_defs as u32 * 4 + 4);
        m[node].defs = Ptr32::new(block);
    }
    let mut k = m[node].nb_defs as i16;
    while position < k {
        let defs = m[node].defs.addr();
        let v = m.u32(defs.wrapping_add((k as i32 * 4 - 4) as u32));
        m.set_u32(defs.wrapping_add((k as i32 * 4) as u32), v);
        k = k.wrapping_sub(1);
    }
    let defs = m[node].defs.addr();
    m.set_u32(defs.wrapping_add((position as i32 * 4) as u32), definition);
    let n = m[node].nb_defs;
    m[node].nb_defs = n.wrapping_add(1);
    let name = m[node].name.addr();
    change_name(m, node, name);
    if m[node].definition.addr() == first {
        let v = m.u32(m[node].defs.addr());
        m[node].definition = Ptr32::new(v);
    }
    1
}

/// Inserts a lattice into one of a node's two lattice arrays
/// [soft 0x08472a0c SI_modelAddILattice]: set 0 when `set` is 0, else set 1. The position
/// is brought into 0 ..= count; the array is made (`SI_calloc(1, 4)`) or grown by one
/// pointer (`SI_realloc`), the pointers from the position on move up, the count goes up.
pub fn add_ilattice(m: &mut Mem, node: Ptr32<Model>, set: i16, position: i16, lattice: u32) {
    // the count and the array of the set, as offsets of the record
    let (count, array) = if set == 0 { (0x4c, 0x50) } else { (0x54, 0x58) };
    let (count, array) = (node.addr() + count, node.addr() + array);
    let mut position = position;
    if position < 1 {
        position = 0;
    } else if m.u16(count) as i32 <= position as i32 {
        position = m.i16(count);
    }
    let block = if m.i16(count) == 0 { m.si_calloc(1, 4) } else { m.si_realloc(m.u32(array), m.u16(count) as u32 * 4 + 4) };
    m.set_u32(array, block);
    let mut k = m.i16(count);
    while position < k {
        let a = m.u32(array);
        let v = m.u32(a.wrapping_add((k as i32 * 4 - 4) as u32));
        m.set_u32(a.wrapping_add((k as i32 * 4) as u32), v);
        k = k.wrapping_sub(1);
    }
    let a = m.u32(array);
    m.set_u32(a.wrapping_add((position as i32 * 4) as u32), lattice);
    let n = m.i16(count);
    m.set_i16(count, n.wrapping_add(1));
}

/// Is anything deforming this node's geometry [soft 0x08472d48 SI_isModelDeformed]: an
/// active lattice, patch or spline deformation, a wave, a quick stretch, an envelope, a
/// geometry that lives in another node's space (`latdep`), a positive word at +0x352, or
/// one of the relations 0xf7 and 0xf6 of the node's element. The element is looked up
/// before the node is tested for null; a null node answers 0.
pub fn is_model_deformed(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) -> u32 {
    let scratch = m.local(8);
    let set = m.local(4);
    let mut by_f7 = false;
    let mut by_f6 = false;
    let element = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), scratch]);
    let mut answer = 0;
    if !node.is_null() {
        if element != 0 {
            let dict = elem_dict(m);
            let chapter = models_chapter(m);
            by_f7 = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 1, 2, 0xf7, chapter, scratch, scratch]) != 0;
            let dict = elem_dict(m);
            let chapter = models_chapter(m);
            if o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 1, 2, 0xf6, chapter, scratch, scratch]) != 0 {
                by_f6 = true;
            }
        }
        if lattice(m, node, set) != 0
            || patch_fitting(m, node, set) != 0
            || spline_fitting(m, node, set) != 0
            || o.call(m, &callee::SI_MODEL_WAVE, &[node.addr(), set]) != 0
            || o.call(m, &callee::SI_QUICK_STRETCH_ACTIVE, &[node.addr()]) as i16 != 0
            || o.call(m, &callee::SI_ENVELOP_ACTIVE, &[node.addr()]) as i16 != 0
            || m[node].latdep != node
            || m[node].unk_352 > 0
            || by_f7
            || by_f6
        {
            answer = 1;
        }
    }
    m.release(12);
    answer
}

/// Removes one stored definition by its number
/// [soft 0x08467c24 SI_modelDeleteIDefinition]; 0 for a number outside the array.
///
/// The only one: `SI_modelDeleteDefinition(node, 0)`. Otherwise: a working definition that
/// is the removed one (and the number is not 0) becomes the first; the definition is
/// disposed by the routine of the node's kind, which gets the address of its slot; the
/// count goes down; the pointers behind move down by one, and whenever the working
/// definition equals the pointer being moved it becomes the first of the array **as the
/// array is at that moment**; the slot behind the last is cleared. Then the name's flag
/// bits ([`change_name`]); answer 1.
pub fn delete_idefinition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, number: i16) -> u32 {
    if number < 0 || m[node].nb_defs as i32 <= number as i32 {
        return 0;
    }
    if m[node].nb_defs == 1 {
        o.call(m, &callee::SI_MODEL_DELETE_DEFINITION, &[node.addr(), 0]);
    } else {
        let slot = m[node].defs.addr().wrapping_add(number as u32 * 4);
        if number > 0 && m[node].definition.addr() == m.u32(slot) {
            let v = m.u32(m[node].defs.addr());
            m[node].definition = Ptr32::new(v);
        }
        let dispose = match m[node].kind {
            1 => Some(&callee::SI_PATCH_DISPOSE),
            2 => Some(&callee::SI_FACE_DISPOSE),
            4 => Some(&callee::SI_MESH_DISPOSE),
            6 => Some(&callee::SI_SPLINE_DISPOSE),
            8 => Some(&callee::SI_METABALL_DISPOSE),
            9 => Some(&callee::SI_NURBS_CRV_DISPOSE),
            10 => Some(&callee::SI_NURBS_SRF_DISPOSE),
            _ => None,
        };
        if let Some(routine) = dispose {
            o.call(m, routine, &[slot]);
        }
        let n = m[node].nb_defs;
        m[node].nb_defs = n.wrapping_sub(1);
        let mut k = number;
        while (k as i32) < m[node].nb_defs as i32 {
            let defs = m[node].defs.addr();
            let next = m.u32(defs.wrapping_add(k as u32 * 4 + 4));
            if m[node].definition.addr() == next {
                let v = m.u32(defs);
                m[node].definition = Ptr32::new(v);
            }
            let defs = m[node].defs.addr();
            let next = m.u32(defs.wrapping_add(k as u32 * 4 + 4));
            m.set_u32(defs.wrapping_add(k as u32 * 4), next);
            k = k.wrapping_add(1);
        }
        let defs = m[node].defs.addr();
        m.set_u32(defs.wrapping_add((k as i32 * 4) as u32), 0);
    }
    let name = m[node].name.addr();
    change_name(m, node, name);
    1
}

/// `SI_edIsItemConstrained(dictionary, node, chapter of the models, attribute, 1, time)`
/// with the time the tools use: the current frame (made a float, which rounds a number
/// above 2²⁴) divided by the frame rate, rounded to float.
fn constrained(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, attribute: u32) -> bool {
    let frame = m[g::SI_FRAME_CURRENT] as f32;
    let time = (x(frame) / m[g::SI_FRAME_RATE]).to_f32();
    let chapter = models_chapter(m);
    let dict = elem_dict(m);
    o.call(m, &callee::SI_ED_IS_ITEM_CONSTRAINED, &[dict, node.addr(), chapter, attribute, 1, time.to_bits()]) as i16 != 0
}

/// A chain root: a null with algorithm 1 or 2.
fn chain_root(m: &Mem, node: Ptr32<Model>) -> bool {
    m[node].kind == 0 && (m[node].algo_type.wrapping_sub(1) as u32) < 2
}

/// An effector: a null with algorithm 3 or 4.
fn chain_effector(m: &Mem, node: Ptr32<Model>) -> bool {
    m[node].kind == 0 && (m[node].algo_type.wrapping_sub(3) as u32) < 2
}

/// May a tool transform this node, and if not, why not
/// [soft 0x08464938 SI_modelIsTransformable]: 0 for yes, else a reason code (100 to 0x70;
/// 0x17 for no node or a transformation that is none of the three). `transformation` is
/// 100 for a scaling, 0x65 for a rotation, 0x71 for a translation (named from the tests:
/// each asks for the constraint of one attribute, 0x73, 0x72 and 0x6f); `select_mode` 0 is
/// the mode in which skeleton nodes are protected; `check_envelope` adds the test for a
/// global envelope to the translation.
pub fn is_transformable(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, transformation: i32, select_mode: i32, check_envelope: i16) -> u32 {
    if node.is_null() {
        return 0x17;
    }
    if m[node].kind == 7 && select_mode == 0 {
        return 100;
    }
    let set = m.local(4);
    let fitted = if patch_fitting(m, node, set) != 0 {
        0x65
    } else if spline_fitting(m, node, set) != 0 {
        0x66
    } else {
        0
    };
    m.release(4);
    if fitted != 0 {
        return fitted;
    }
    match transformation {
        0x65 => {
            if select_mode == 0 {
                if m[node].kind == 5 {
                    return 0x69;
                }
                if chain_root(m, node) {
                    return 0x6a;
                }
            }
            if constrained(m, o, node, 0x72) { 0x6b } else { 0 }
        }
        100 => {
            if select_mode == 0 && (chain_root(m, node) || m[node].kind == 5 || chain_effector(m, node)) {
                return 0x67;
            }
            if constrained(m, o, node, 0x73) { 0x68 } else { 0 }
        }
        0x71 => {
            if select_mode == 0 && chain_root(m, node) {
                return 0x6c;
            }
            if m[node].kind == 5 {
                return 0x6d;
            }
            if select_mode == 0 && m[node].algo_type == 4 {
                return 0x6e;
            }
            if check_envelope != 0 && o.call(m, &callee::SI_MAIN_GLOB_ENV, &[node.addr()]) as i16 != 0 {
                return 0x6f;
            }
            if constrained(m, o, node, 0x6f) { 0x70 } else { 0 }
        }
        _ => 0x17,
    }
}

/// Which modes a transformation of this node may be done in
/// [soft 0x08464cd8 SI_modelTransfoModesGet]. The three outputs (reference,
/// hierarchy and manipulation mode) start at 2; a skeleton node's hierarchy mode becomes 1
/// and, for a joint's rotation, a scaling of a skeleton node and a translation under a
/// global envelope, the manipulation mode 0. The answer is 0 or a reason code as
/// [`is_transformable`] gives.
pub fn transfo_modes_get(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, transformation: i32, reference: u32, hierarchy: u32, manipulation: u32) -> u32 {
    if node.is_null() || reference == 0 || hierarchy == 0 || manipulation == 0 {
        return 0x17;
    }
    m.set_u32(reference, 2);
    m.set_u32(hierarchy, 2);
    m.set_u32(manipulation, 2);
    if m[node].kind == 7 {
        m.set_u32(hierarchy, 1);
        return 0;
    }
    let set = m.local(4);
    let fitted = if patch_fitting(m, node, set) != 0 {
        0x65
    } else if spline_fitting(m, node, set) != 0 {
        0x66
    } else {
        0
    };
    m.release(4);
    if fitted != 0 {
        return fitted;
    }
    match transformation {
        0x65 => {
            if m[node].kind != 5 {
                if !chain_root(m, node) {
                    return if constrained(m, o, node, 0x72) { 0x6b } else { 0 };
                }
                m.set_u32(hierarchy, 1);
                return 0;
            }
            m.set_u32(hierarchy, 1);
            m.set_u32(manipulation, 0);
            0
        }
        100 => {
            if !chain_root(m, node) && m[node].kind != 5 && !chain_effector(m, node) {
                return if constrained(m, o, node, 0x73) { 0x68 } else { 0 };
            }
            m.set_u32(hierarchy, 1);
            m.set_u32(manipulation, 0);
            0
        }
        0x71 => {
            if chain_root(m, node) || m[node].algo_type == 4 {
                m.set_u32(hierarchy, 1);
                return 0;
            }
            if m[node].kind == 5 {
                return 0x6d;
            }
            if o.call(m, &callee::SI_MAIN_GLOB_ENV, &[node.addr()]) as i16 == 0 {
                return if constrained(m, o, node, 0x6f) { 0x70 } else { 0 };
            }
            m.set_u32(manipulation, 0);
            0
        }
        _ => 0x17,
    }
}

/// The routine that copies a definition of a kind (`SI_patchCopy` and its like), if the
/// kind is one of the six geometry kinds.
fn copy_of(kind: i32) -> Option<&'static super::Callee> {
    Some(match kind {
        1 => &callee::SI_PATCH_COPY,
        2 => &callee::SI_FACE_COPY,
        4 => &callee::SI_MESH_COPY,
        6 => &callee::SI_SPLINE_COPY,
        9 => &callee::SI_NURBS_CRV_COPY,
        10 => &callee::SI_NURBS_SRF_COPY,
        _ => return None,
    })
}

/// The routine that disposes a definition of a kind through the address of its pointer.
fn dispose_of(kind: i32) -> Option<&'static super::Callee> {
    Some(match kind {
        1 => &callee::SI_PATCH_DISPOSE,
        2 => &callee::SI_FACE_DISPOSE,
        4 => &callee::SI_MESH_DISPOSE,
        6 => &callee::SI_SPLINE_DISPOSE,
        9 => &callee::SI_NURBS_CRV_DISPOSE,
        10 => &callee::SI_NURBS_SRF_DISPOSE,
        _ => return None,
    })
}

/// A node deformed by an envelope: types 5, 6 and 8.
fn enveloped(m: &Mem, node: Ptr32<Model>) -> bool {
    let e = m[node].envelop;
    (e.wrapping_sub(5) as u32) < 2 || e == 8
}

/// The stored definition `k` of a node.
fn stored(m: &Mem, node: Ptr32<Model>, k: u32) -> u32 {
    m.u32(m[node].defs.addr().wrapping_add(k.wrapping_mul(4)))
}

/// Replaces one stored definition by another [soft 0x08467158 SI_modelSubstituteIDefinition];
/// 0 for a number outside the array, else 1.
///
/// When the number is the current one, the node either works on that stored definition
/// itself ("same") or on a separate working copy ("apart"). The stored definition is
/// disposed through its slot; if apart, the working copy is disposed too and replaced by
/// a copy of the new definition. The slot gets the new definition; if same, so does the
/// working pointer. If apart the definition is readjusted and an enveloped node's
/// envelope updated. Then `SI_modelTouchGeometry`. For a kind without geometry nothing is
/// disposed or copied, the rest happens all the same.
pub fn substitute_idefinition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, number: i16, definition: u32) -> u32 {
    if number < 0 || m[node].nb_defs as i32 <= number as i32 {
        return 0;
    }
    let k = number as u32;
    let current = k == m[node].cur_def as u32;
    let apart = current && m[node].definition.addr() != stored(m, node, k);
    let same = current && m[node].definition.addr() == stored(m, node, k);
    let kind = m[node].kind;
    if let (Some(dispose), Some(copy)) = (dispose_of(kind), copy_of(kind)) {
        let slot = m[node].defs.addr().wrapping_add(k * 4);
        o.call(m, dispose, &[slot]);
        if apart {
            o.call(m, dispose, &[super::at!(node, definition)]);
            let c = o.call(m, copy, &[definition]);
            m[node].definition = Ptr32::new(c);
        }
    }
    let slot = m[node].defs.addr().wrapping_add(k * 4);
    m.set_u32(slot, definition);
    if same {
        m[node].definition = Ptr32::new(definition);
    }
    if apart {
        o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
        if enveloped(m, node) {
            o.call(m, &callee::SI_ENVELOP_COMP_UPDATE, &[node.addr()]);
        }
    }
    o.call(m, &callee::SI_MODEL_TOUCH_GEOMETRY, &[node.addr()]);
    1
}

/// Makes the first stored definition fit the second when their point counts differ
/// [soft 0x08466ca4 SI_modelValidateShape]: with fewer than two definitions nothing (0);
/// otherwise, by kind, a count of the first two is compared and, when they differ, a copy
/// of the **second** takes the first's place ([`substitute_idefinition`]) and the answer
/// is 1. The counts (offsets of records without a layout here, named from the code): a
/// patch's two 16-bit counts at +0xc and +0xe, multiplied; a face's 16-bit count at +0x14
/// when the first's word at +0x84 is not zero, else the word at +4 of what +0x80 points to
/// (the outline's point count of a NURBS face); a mesh's word at +0x14; a spline's 16-bit
/// word at +0x10; a NURBS curve's word at +4; a NURBS surface's word at +0x48.
pub fn validate_shape(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) -> u16 {
    if m[node].nb_defs < 2 {
        return 0;
    }
    let (a, b) = (stored(m, node, 0), stored(m, node, 1));
    let kind = m[node].kind;
    let same = match kind {
        1 => (m.u16(a + 0xc) as u32 * m.u16(a + 0xe) as u32).wrapping_sub(m.u16(b + 0xc) as u32 * m.u16(b + 0xe) as u32) == 0,
        2 => {
            if m.i16(a + 0x84) == 0 {
                m.u32(m.u32(a + 0x80) + 4) == m.u32(m.u32(b + 0x80) + 4)
            } else {
                m.i16(a + 0x14) == m.i16(b + 0x14)
            }
        }
        4 => m.u32(a + 0x14) == m.u32(b + 0x14),
        6 => m.i16(a + 0x10) == m.i16(b + 0x10),
        9 => m.u32(a + 4) == m.u32(b + 4),
        10 => m.u32(a + 0x48) == m.u32(b + 0x48),
        _ => return 0,
    };
    if same {
        return 0;
    }
    let copy = o.call(m, copy_of(kind).unwrap(), &[b]);
    substitute_idefinition(m, o, node, 0, copy);
    1
}

/// Exchanges the geometry of two nodes, with `recurse` down two trees of the same shape
/// [soft 0x08467374 SI_modelSubstituteGeometry].
///
/// Only when `a` has a displayed node (`SI_findGlData(list, a, 0)`) whose word at +8 is
/// not zero: if `a` works on a copy apart from its current stored definition while `b`
/// works on its stored one, `b` first gets a working copy of its **first** stored
/// definition; then kind, count, current number, original definition, array, working
/// pointer and the cluster list (+0x348) change places; a patch has its surface updated
/// (the first stored one, and the working one when apart); `SI_modelTouchGeometry(a)`.
/// With `recurse`: the sons, then the brothers, pairwise, stopping with 0 at the first
/// pair that answers 0 (as 16 bits); `b`'s links are followed without a test.
pub fn substitute_geometry(m: &mut Mem, o: &mut dyn Outside, a: Ptr32<Model>, b: Ptr32<Model>, recurse: i16) -> u32 {
    let list = m[g::SI_GRAF_LISTS].wrapping_add((m.i32(m[g::SI_PROC_DATA].addr() + 4) as u32).wrapping_mul(0x24));
    let shown = o.call(m, &callee::SI_FIND_GL_DATA, &[list, a.addr(), 0]);
    if shown != 0 && m.u32(shown + 8) != 0 {
        if !m[a].defs.is_null()
            && !m[b].defs.is_null()
            && m[a].definition.addr() != stored(m, a, m[a].cur_def as u32)
            && m[b].definition.addr() == stored(m, b, m[b].cur_def as u32)
        {
            if let Some(copy) = copy_of(m[b].kind) {
                let first = stored(m, b, 0);
                let c = o.call(m, copy, &[first]);
                m[b].definition = Ptr32::new(c);
            }
        }
        let (ra, rb) = (m.read(a), m.read(b));
        {
            let n = m.get_mut(a);
            n.kind = rb.kind;
            n.nb_defs = rb.nb_defs;
            n.cur_def = rb.cur_def;
            n.org_def = rb.org_def;
            n.defs = rb.defs;
            n.definition = rb.definition;
            n.klusters = rb.klusters;
        }
        {
            let n = m.get_mut(b);
            n.kind = ra.kind;
            n.nb_defs = ra.nb_defs;
            n.cur_def = ra.cur_def;
            n.org_def = ra.org_def;
            n.defs = ra.defs;
            n.definition = ra.definition;
            n.klusters = ra.klusters;
        }
        if m[a].kind == 1 {
            let first = stored(m, a, 0);
            o.call(m, &callee::SI_PATCH_UPDATE_SURFACE, &[first]);
            let working = m[a].definition.addr();
            if working != stored(m, a, m[a].cur_def as u32) {
                o.call(m, &callee::SI_PATCH_UPDATE_SURFACE, &[working]);
            }
        }
        o.call(m, &callee::SI_MODEL_TOUCH_GEOMETRY, &[a.addr()]);
    }
    if recurse == 0 {
        return 1;
    }
    let son = m[a].son;
    if !son.is_null() {
        let other = m[b].son;
        if substitute_geometry(m, o, son, other, recurse) as i16 == 0 {
            return 0;
        }
    }
    let brother = m[a].brother;
    if !brother.is_null() {
        let other = m[b].brother;
        if substitute_geometry(m, o, brother, other, recurse) as i16 == 0 {
            return 0;
        }
    }
    1
}

/// Exchanges a node's stored geometry with one held by the caller
/// [soft 0x084675f0 SI_modelSwapGeometry]: kind, count, array and original definition
/// change places with the four variables. 0 for no node, no array or no working
/// definition; else 1.
///
/// A working copy apart from the current stored definition is disposed first. Afterwards,
/// if the node had worked on its stored definition: with a new array the current number
/// becomes 0, the working pointer the first of the new array and
/// `SI_modelTouchDefinition`; without one the working pointer becomes null. If it had
/// worked apart: without a new array the working pointer becomes null (and nothing
/// else); with one it becomes a copy of the new first definition (by the new kind; kept
/// for a kind without geometry), and then an enveloped node's envelope is updated, any
/// other node's definition readjusted.
pub fn swap_geometry(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, kind: u32, count: u32, array: u32, original: u32) -> u32 {
    if node.is_null() || m[node].defs.is_null() {
        return 0;
    }
    let working = m[node].definition.addr();
    if working == 0 {
        return 0;
    }
    let same = working == stored(m, node, m[node].cur_def as u32);
    if !same {
        if let Some(dispose) = dispose_of(m[node].kind) {
            o.call(m, dispose, &[super::at!(node, definition)]);
        }
    }
    let v = m.i32(kind);
    let mine = m[node].kind;
    m.set_i32(kind, mine);
    m[node].kind = v;
    let v = m.u16(count);
    let mine = m[node].nb_defs;
    m.set_u16(count, mine);
    m[node].nb_defs = v;
    let v = m.u32(array);
    let mine = m[node].defs.addr();
    m.set_u32(array, mine);
    m[node].defs = Ptr32::new(v);
    let v = m.u32(original);
    let mine = m[node].org_def.addr();
    m.set_u32(original, mine);
    m[node].org_def = Ptr32::new(v);
    if m[node].defs.is_null() {
        m[node].definition = Ptr32::NULL;
        return 1;
    }
    if same {
        m[node].cur_def = 0;
        let first = stored(m, node, 0);
        m[node].definition = Ptr32::new(first);
        o.call(m, &callee::SI_MODEL_TOUCH_DEFINITION, &[node.addr()]);
        return 1;
    }
    if let Some(copy) = copy_of(m[node].kind) {
        let first = stored(m, node, 0);
        let c = o.call(m, copy, &[first]);
        m[node].definition = Ptr32::new(c);
    }
    if enveloped(m, node) {
        o.call(m, &callee::SI_ENVELOP_COMP_UPDATE, &[node.addr()]);
    } else {
        o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
    }
    1
}

/// Brings the points of stored definition `number` into the first stored definition
/// [soft 0x08472ef4 SI_modelSetCurrentDefinition]; 0 for a number outside the array, 1
/// at once for the current number. **The current number itself (+0x36) is not written**:
/// that is the caller's.
///
/// "Apart" is a working copy other than the current stored definition on a node that is
/// not enveloped: it is disposed. Then, by kind, the points of definition `number` are
/// transferred into definition 0 (a patch has its surface updated first), and an
/// enveloped node's envelope is updated. Not apart: the working pointer becomes the
/// current stored definition, unless the node is enveloped. Apart: it becomes a copy of
/// definition 0 (kept for a kind without geometry) and the definition is readjusted.
pub fn set_current_definition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, number: i16) -> u32 {
    if number < 0 || m[node].nb_defs as i32 <= number as i32 {
        return 0;
    }
    let k = number as u32;
    if k == m[node].cur_def as u32 {
        return 1;
    }
    let apart = m[node].definition.addr() != stored(m, node, m[node].cur_def as u32) && !enveloped(m, node);
    let kind = m[node].kind;
    if apart {
        if let Some(dispose) = dispose_of(kind) {
            o.call(m, dispose, &[super::at!(node, definition)]);
        }
    }
    let transfer = match kind {
        1 => Some(&callee::SI_PATCH_TRANSFER_POINTS),
        2 => Some(&callee::SI_FACE_TRANSFER_POINTS),
        4 => Some(&callee::SI_MESH_TRANSFER_VERTICES),
        6 => Some(&callee::SI_SPLINE_TRANSFER_POINTS),
        9 => Some(&callee::SI_NURBS_CRV_TRANSFER_POINTS_KNOTS),
        10 => Some(&callee::SI_NURBS_SRF_TRANSFER_POINTS_KNOTS),
        _ => None,
    };
    if let Some(transfer) = transfer {
        if kind == 1 {
            let from = stored(m, node, k);
            o.call(m, &callee::SI_PATCH_UPDATE_SURFACE, &[from]);
        }
        let (from, to) = (stored(m, node, k), stored(m, node, 0));
        o.call(m, transfer, &[from, to]);
    }
    if enveloped(m, node) {
        o.call(m, &callee::SI_ENVELOP_COMP_UPDATE, &[node.addr()]);
    }
    if !apart {
        if !enveloped(m, node) {
            let current = stored(m, node, m[node].cur_def as u32);
            m[node].definition = Ptr32::new(current);
        }
        return 1;
    }
    if let Some(copy) = copy_of(kind) {
        let first = stored(m, node, 0);
        let c = o.call(m, copy, &[first]);
        m[node].definition = Ptr32::new(c);
    }
    o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
    1
}

/// The number of definitions a geometry node has to update: the stored ones, and one more
/// when it works on a copy apart from the current stored one; 0 for the other kinds.
fn all_defs(m: &Mem, node: Ptr32<Model>) -> u32 {
    if matches!(m[node].kind, 4 | 1 | 6 | 2 | 9 | 10) {
        let n = m[node].nb_defs as u32;
        if m[node].definition.addr() != stored(m, node, m[node].cur_def as u32) {
            n + 1
        } else {
            n
        }
    } else {
        0
    }
}

/// Definition `i` of `all_defs`: a stored one, then the working one.
fn def_at(m: &Mem, node: Ptr32<Model>, i: u32) -> u32 {
    if (i as i32) < m[node].nb_defs as i32 {
        stored(m, node, i)
    } else {
        m[node].definition.addr()
    }
}

/// Brings every definition of a patch or NURBS surface node up to date with the working
/// one's display settings [soft 0x0846ef2c SI_modelUpdateAll].
///
/// With `settings`, for a patch (kind 1): every definition other than the working one
/// takes twelve fields of it (16-bit words at +0x20, +0x22, +0x1c, +0x1e, +0x200, +0x1e8,
/// +0x1fc, words at +0x1f0, +0x1f4, +0x1ec, 16-bit words at +0x1f8, +0x1fa: the step and
/// display settings of `SI_Patch`, named offsets), and each has its matrix and surface
/// updated. For a NURBS surface (kind 10): each gets bits 0x60 in the byte at +0x5b, its
/// lines disposed, and, unless it is the working one, twelve fields of it (words at +0x70
/// to +0x7c, 16-bit words at +0x98, +0x9c, +0xb0, words at +0xa4, +0xa8, +0xa0, 16-bit
/// words at +0xac, +0xae). Both set bit 0x10 of the node's two update flag words. Any
/// other kind, or no `settings`: none of this.
///
/// With `readjust`: an enveloped node's envelope is updated, then the definition is
/// readjusted.
pub fn update_all(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, settings: i16, readjust: i16) {
    let kind = m[node].kind;
    if (kind == 1 || kind == 10) && settings != 0 {
        let working = m[node].definition.addr();
        let count = all_defs(m, node);
        for i in 0..count {
            let d = def_at(m, node, i);
            if kind == 1 {
                if d != working {
                    for off in [0x20, 0x22, 0x1c, 0x1e, 0x200, 0x1e8, 0x1fc] {
                        let v = m.u16(working + off);
                        m.set_u16(d + off, v);
                    }
                    for off in [0x1f0, 0x1f4, 0x1ec] {
                        let v = m.u32(working + off);
                        m.set_u32(d + off, v);
                    }
                    for off in [0x1f8, 0x1fa] {
                        let v = m.u16(working + off);
                        m.set_u16(d + off, v);
                    }
                }
                o.call(m, &callee::SI_PATCH_UPDATE_MATRIX, &[d]);
                o.call(m, &callee::SI_PATCH_UPDATE_SURFACE, &[d]);
            } else {
                m.or_u8(d + 0x5b, 0x60);
                o.call(m, &callee::SI_NURBS_SRF_DISPOSE_LINES, &[d]);
                if d != working {
                    for off in [0x70, 0x74, 0x78, 0x7c] {
                        let v = m.u32(working + off);
                        m.set_u32(d + off, v);
                    }
                    for off in [0x98, 0x9c, 0xb0] {
                        let v = m.u16(working + off);
                        m.set_u16(d + off, v);
                    }
                    for off in [0xa4, 0xa8, 0xa0] {
                        let v = m.u32(working + off);
                        m.set_u32(d + off, v);
                    }
                    for off in [0xac, 0xae] {
                        let v = m.u16(working + off);
                        m.set_u16(d + off, v);
                    }
                }
            }
        }
        m.or_u8(node.addr() + 0x0a, 0x10);
        m.or_u8(node.addr() + 0x3fa, 0x10);
    }
    if readjust != 0 {
        if enveloped(m, node) {
            o.call(m, &callee::SI_ENVELOP_COMP_UPDATE, &[node.addr()]);
        }
        o.call(m, &callee::SI_MODEL_READJUST_DEFINITION, &[node.addr()]);
    }
}

/// Turns a node's geometry inside out [soft 0x084732fc SI_modelInverse]:
/// `SI_modelTouchGeometry`; unless the node's element has the relation 0x104 (the element
/// is not tested for null), an index of modifications is allocated into `SI_gblIndex`,
/// else that global is cleared; every definition (`all_defs`) is inverted by the routine
/// of the kind (a patch with the index, a NURBS surface with 1 and 0); patches and NURBS
/// surfaces then have their zip indices updated; and an index that was allocated is
/// propagated.
pub fn inverse(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    let scratch = m.local(4);
    o.call(m, &callee::SI_MODEL_TOUCH_GEOMETRY, &[node.addr()]);
    let element = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), scratch]);
    let dict = elem_dict(m);
    let chapter = models_chapter(m);
    let related = o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, element, 1, 2, 0x104, chapter, scratch, scratch]);
    if related == 0 {
        let index = o.call(m, &callee::SI_INDEX_MODIF_ALLOCATE, &[node.addr(), 0]);
        m[g::SI_GBL_INDEX] = index;
    } else {
        m[g::SI_GBL_INDEX] = 0;
    }
    let kind = m[node].kind;
    let count = all_defs(m, node);
    if matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        for i in 0..count {
            let d = def_at(m, node, i);
            match kind {
                1 => {
                    let index = m[g::SI_GBL_INDEX];
                    o.call(m, &callee::SI_PATCH_INVERSE, &[d, index])
                }
                2 => o.call(m, &callee::SI_FACE_INVERSE, &[d]),
                4 => o.call(m, &callee::SI_MESH_INVERSE, &[d]),
                6 => o.call(m, &callee::SI_SPLINE_INVERSE, &[d]),
                9 => o.call(m, &callee::SI_NURBS_CRV_INVERSE, &[d]),
                _ => o.call(m, &callee::SI_NURBS_SRF_INVERSE, &[d, 1, 0]),
            };
        }
        if kind == 1 || kind == 10 {
            o.call(m, &callee::SI_PATCH_ZIP_UPDATE_INDEX, &[node.addr()]);
        }
    }
    if related == 0 && m[g::SI_GBL_INDEX] != 0 {
        o.call(m, &callee::SI_INDEX_MODIF_PROPAGATE, &[node.addr(), g::SI_GBL_INDEX.addr(), 1]);
    }
    m.release(4);
}
