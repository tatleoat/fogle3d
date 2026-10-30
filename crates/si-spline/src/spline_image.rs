// SPDX-License-Identifier: MIT
//! The spline record's primitives on the one memory image (finding 0024; finding 0020,
//! sections 1 and 5): what the routines of the third pass need of `spline.c`, written in
//! place. A function takes the image and the arguments the original takes, records are
//! `si-types`' layouts at their addresses, and every allocation call of the original is
//! made, with its size, in its order.
//!
//! The value forms of the same routines ([`crate::edit`], [`crate::basis`]) stay the ones
//! the rest of the workspace uses. `spline-emucheck` runs the first two passes' vector
//! files through the forms here as well (`SPLINE_IMAGE=1`).
//!
//! Callees outside the package are behind [`Outside`]; the scene's records the touch walks
//! are `si-types`' (`__SI_GrafNode`, `__SI_Model`); the spline deformation record is read
//! by two named offsets ([`FIT_PATH`], [`FIT_TABLE`]: its layout is `fitting.c`'s, another
//! package's data).

use si_math::algebra::matrix_multiply;
use si_mem::{globals, Mem, Ptr32};
use si_types::geom::{Spline, SplineKey, Vector};
use si_types::model::Model;
use si_types::scene::GrafNode;
use si_x87::x;

use crate::basis::{basis_of, matrix_precision};

/// A routine outside this package: its name and its address in the Linux binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callee {
    pub name: &'static str,
    pub addr: u32,
}

/// `SI_findPathFcurveOwner` (0x0821f620): the percentage curve of the object on this path.
pub const SI_FIND_PATH_FCURVE_OWNER: Callee = Callee { name: "SI_findPathFcurveOwner", addr: 0x0821_f620 };
/// `SI_checkSplineToFcurve` (0x0821fc30).
pub const SI_CHECK_SPLINE_TO_FCURVE: Callee = Callee { name: "SI_checkSplineToFcurve", addr: 0x0821_fc30 };
/// `SI_updateFcurvePathKeys` (0x0821fe68).
pub const SI_UPDATE_FCURVE_PATH_KEYS: Callee = Callee { name: "SI_updateFcurvePathKeys", addr: 0x0821_fe68 };
/// `SI_deleteFcurvePathKey` (0x0821ff50).
pub const SI_DELETE_FCURVE_PATH_KEY: Callee = Callee { name: "SI_deleteFcurvePathKey", addr: 0x0821_ff50 };
/// `SI_addFcurvePathKey` (0x08220008).
pub const SI_ADD_FCURVE_PATH_KEY: Callee = Callee { name: "SI_addFcurvePathKey", addr: 0x0822_0008 };
/// `SI_edGetElemFromModel` (0x080fcf20): a model's element; its number through the pointer.
pub const SI_ED_GET_ELEM_FROM_MODEL: Callee = Callee { name: "SI_edGetElemFromModel", addr: 0x080f_cf20 };
/// `SI_elemdictFindRelatedElem` (0x0813508c): the first element related by a relation.
pub const SI_ELEMDICT_FIND_RELATED_ELEM: Callee = Callee { name: "SI_elemdictFindRelatedElem", addr: 0x0813_508c };
/// `SI_waveGetIconModel` (0x084c6f00): the model that stands for a wave on screen.
pub const SI_WAVE_GET_ICON_MODEL: Callee = Callee { name: "SI_waveGetIconModel", addr: 0x084c_6f00 };
/// `SI_refreshFcurve` (0x08323ce0).
pub const SI_REFRESH_FCURVE: Callee = Callee { name: "SI_refreshFcurve", addr: 0x0832_3ce0 };

/// Everything the image forms of this crate call outside the package: one method, the
/// original's way of calling (the callee, the 32-bit words pushed in C order, `eax` back).
/// The default answers 0 and does nothing, which is what the animation package's routines
/// amount to for a spline no percentage curve follows.
pub trait Outside {
    fn call(&mut self, _m: &mut Mem, _callee: &Callee, _args: &[u32]) -> u32 {
        0
    }
    /// The same for a callee that answers a `float` or `double`: the value it leaves in
    /// the x87 register (fourth pass: `SI_getTexedFloat`). The default answers 0.
    fn call_float(&mut self, _m: &mut Mem, _callee: &Callee, _args: &[u32]) -> si_x87::F80 {
        si_x87::F80::ZERO
    }
}

/// An outside world with nothing in it.
pub struct NoOutside;
impl Outside for NoOutside {}

globals! {
    /// The counter `SI_splineAllocate` names new splines from (`spl1`, `spl2`, …).
    SPLINE_COUNT: i32 = 0x086d6960, "count.225";
    /// When not null: the table in which the editing routines record where each key went.
    SI_GBL_INDEX: Ptr32<i32> = 0x0878896c, "SI_gblIndex";
    /// The second such table (the second spline of a merge).
    SI_GBL_INDEX2: Ptr32<i32> = 0x08788970, "SI_gblIndex2";
    /// Points at the record whose second word is the number of the current object list.
    SI_PROC_DATA: Ptr32<[i32; 2]> = 0x08679314, "SI_procData", init 0x08782108;
    /// The object lists, 0x24 bytes each (`SI_GrafList`).
    SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists";
    /// `fitCurve.c`: the spline a fit is building.
    SI_C_BEZ: Ptr32<Spline> = 0x0881b620, "SI_cBez";
    /// `fitCurve.c`: non-zero when the next segment starts at a corner.
    JOIN: i16 = 0x0881b208, "join";
    /// `fitCurve.c`: the error the contour fit hands to `SI_fitBezCurve` (0.05).
    FIT_ERROR: si_mem::Double = 0x086d3700, "fitError", init 0x3fa999999999999a;
    /// `fitCurve.c`: how many points ahead the contour's simplification looks (7).
    LOOK_AHEAD: i32 = 0x086d3708, "lookAhead", init 7;
    /// `fitCurve.c`: the largest distance from a line in the contour's simplification (1.5).
    MAX_DIST_LINE: si_mem::Double = 0x086d3710, "maxDistLine", init 0x3ff8000000000000;
    /// `fitCurve.c`: the smallest such distance (0.2).
    MIN_DIST_LINE: si_mem::Double = 0x086d3718, "minDistLine", init 0x3fc999999999999a;
    /// The element dictionaries, 0x70 bytes each; `SI_procData[0]` is the current one's number.
    SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts";
    /// The number of the dictionary chapter that holds the model nodes (no symbol).
    MODEL_CHAPTER: i32 = 0x08781680, "";
}

/// `SI_identite` (0x086dc160), the identity matrix every initialisation copies. Declared
/// outside the `globals!` block because its initial value is sixteen floats (`init` holds
/// one number); [`init`] stores them.
pub const SI_IDENTITE: Ptr32<[[f32; 4]; 4]> = Ptr32::new(0x086d_c160);

/// The initial values of this module's globals, as the binary's `.data` has them.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    m.write(SI_IDENTITE, si_math::types::IDENTITY);
}

/// Offsets in `SI_SplFitting`, the spline deformation record (`fitting.c`; not laid out
/// here): the path model and the table that depends on the path's spline.
pub const FIT_PATH: u32 = 4;
pub const FIT_TABLE: u32 = 8;

/// Sets a spline's fields to their initial values [soft 0x0848a9f0 SI_splineInit], in the
/// original's order. The padding (+0x0a, +0x12, +0x22, +0x6a, +0x72) is left as it was.
pub fn spline_init(m: &mut Mem, s: Ptr32<Spline>) {
    m[s].tension = 0.0;
    m[s].name = Ptr32::NULL;
    m[s].unk_04 = 0x10;
    m[s].closed = 0;
    m[s].phantom = 0;
    m[s].nb_keys = 0;
    m[s].keys = Ptr32::NULL;
    m[s].unk_18 = 0;
    m[s].step = 10;
    m[s].kind = 0;
    m[s].unk_64 = 2;
    m[s].unk_66 = 1;
    // SI_matrixCopy from the global
    let identity = si_math::algebra::matrix_copy(&m[SI_IDENTITE]);
    m[s].matrix = identity;
    m[s].nb_lengths = 0;
    m[s].lengths = Ptr32::NULL;
    m[s].unk_70 = 0;
}

/// A new spline [soft 0x0848a990 SI_splineAllocate]: `SI_malloc(0x74)` (so the last byte of
/// the record, in its padding, is the allocator's `k`), [`spline_init`], the counter
/// incremented, and the name `spl<n>` (the C format `spl%1hd` of the counter) in a
/// `strdup` block. Null when the allocation fails.
pub fn spline_allocate(m: &mut Mem) -> Ptr32<Spline> {
    let s: Ptr32<Spline> = Ptr32::new(m.si_malloc(Spline::SIZE as u32));
    if !s.is_null() {
        spline_init(m, s);
        let n = m[SPLINE_COUNT].wrapping_add(1);
        m[SPLINE_COUNT] = n;
        // sprintf into a local of 0x50 bytes, then strdup of it
        let text = format!("spl{}", n as i16).into_bytes();
        let local = m.local(0x50);
        m.put_cstr(local, &text);
        let name = m.strdup(local);
        m.release(0x50);
        m[s].name = Ptr32::new(name);
    }
    s
}

/// A key block for `count` keys [soft 0x0848f8dc SI_splineKeyAllocate]:
/// `SI_calloc(count rounded up to a multiple of 16, 0x30)`, then the first `count` records
/// written field by field: zero vectors, the float 1.0 at +0x24, zero constraint, segment
/// type and flag (not the padding at +0x2e, which `calloc` cleared). Null for a count of
/// 0 or less that rounds to no record.
pub fn spline_key_allocate(m: &mut Mem, count: i32) -> Ptr32<SplineKey> {
    let room = count.wrapping_sub(((count.wrapping_sub(1) as u32 & 0xf) as i32).wrapping_sub(0xf));
    let block: Ptr32<SplineKey> = Ptr32::new(m.si_calloc(room as u32, SplineKey::SIZE as u32));
    let zero = Vector { x: 0.0, y: 0.0, z: 0.0 };
    let mut k = block;
    for _ in 0..count.max(0) {
        m[k].prev = zero;
        m[k].pos = zero;
        m[k].next = zero;
        m[k].unk_24 = 1.0f32.to_le_bytes();
        m[k].constraint = 0;
        m[k].segment = 0;
        m[k].flag = 0;
        k = k.add(1);
    }
    block
}

/// Recomputes the spline's own matrix (+0x24) [soft 0x0848abd0 SI_splineUpdateMatrix]:
/// `SI_matrixPrecision` for the step count (1 for a linear spline) times the basis of the
/// type. For a type outside 0..=3 the original multiplies an uninitialised local; zeros
/// stand for it here.
pub fn spline_update_matrix(m: &mut Mem, s: Ptr32<Spline>) {
    let steps = if m[s].kind != 0 { m[s].step as i32 } else { 1 };
    let basis = basis_of(m[s].kind, m[s].tension);
    m[s].matrix = matrix_multiply(&matrix_precision(steps), &basis);
}

fn twice_less(a: Vector, b: Vector) -> Vector {
    Vector { x: ((x(a.x) + a.x) - b.x).to_f32(), y: ((x(a.y) + a.y) - b.y).to_f32(), z: ((x(a.z) + a.z) - b.z).to_f32() }
}

fn set_all(m: &mut Mem, k: Ptr32<SplineKey>, p: Vector) {
    m[k].pos = p;
    m[k].next = p;
    m[k].prev = p;
}

/// Recomputes the phantom end points [soft 0x0848df94 SI_splineSetDummy]; the rules are
/// [`crate::edit::set_dummy`]'s (finding 0020, section 5). Nothing is checked against the
/// key block: a count the block does not hold reads and writes other memory, as in the
/// original.
pub fn spline_set_dummy(m: &mut Mem, s: Ptr32<Spline>) {
    let nb = m[s].nb_keys;
    if m[s].phantom == 0 || m[s].closed != 0 {
        return;
    }
    let (kind, keys) = (m[s].kind, m[s].keys);
    if nb == 3 && (kind == 2 || kind == 3) {
        let p = m[keys.add(1)].pos;
        set_all(m, keys.add(2), p);
        set_all(m, keys, p);
        return;
    }
    if nb <= 3 {
        return;
    }
    let n = nb as i32;
    let (first, last) = if kind == 2 || (nb == 4 && kind == 3) {
        (twice_less(m[keys.add(1)].pos, m[keys.add(2)].pos), twice_less(m[keys.add(n - 2)].pos, m[keys.add(n - 3)].pos))
    } else if kind == 3 {
        let first = twice_less(m[keys.add(1)].pos, twice_less(m[keys.add(2)].pos, m[keys.add(3)].pos));
        (first, twice_less(m[keys.add(n - 2)].pos, twice_less(m[keys.add(n - 3)].pos, m[keys.add(n - 4)].pos)))
    } else {
        return;
    };
    set_all(m, keys, first);
    set_all(m, keys.add(n - 1), last);
}

/// Marks a spline as changed [soft 0x0848c924 SI_splineTouch]:
///
/// 1. [`spline_set_dummy`]; bit 0x10 of the byte at +0x04; the length table freed and its
///    pointer cleared.
/// 2. The walk over the current object list (`SI_grafLists[SI_procData[1]]`, from its
///    first node along `next`): for a node of kind 2 or 3 (a model's root or another of
///    its nodes), with `model` its data: when the model is a spline (kind 6) whose
///    definition is this spline, bit 0x10 is set in both update flag bytes (+0x0a,
///    +0x3fa); and for each of its two spline deformation records (+0x60, +0x64) whose
///    path model's definition is this spline, the record's table is freed and cleared.
/// 3. For a keyframed spline (+0x70 is 1): `SI_findPathFcurveOwner(spline)`,
///    `SI_updateFcurvePathKeys(owner, spline)`, `SI_checkSplineToFcurve(owner, spline)`,
///    `SI_refreshFcurve(0)`.
pub fn spline_touch(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>) {
    spline_set_dummy(m, s);
    m.or_u8(s.addr() + 4, 0x10);
    if !s.is_null() {
        let table = m[s].lengths;
        if !table.is_null() {
            m.free(table.addr());
            m[s].lengths = Ptr32::NULL;
        }
    }
    let current = m[m[SI_PROC_DATA]][1];
    // the head of list `current`: the word at +4 of its 0x24-byte record
    let mut node: Ptr32<GrafNode> = Ptr32::new(m.u32(m[SI_GRAF_LISTS].wrapping_add((current as u32).wrapping_mul(0x24)).wrapping_add(4)));
    while !node.is_null() {
        if (m[node].kind as u32).wrapping_sub(2) <= 1 {
            let model: Ptr32<Model> = m[node].data.cast();
            if m[model].kind == 6 && m[model].definition.addr() == s.addr() {
                m.or_u8(model.addr() + 0x0a, 0x10);
                m.or_u8(model.addr() + 0x3fa, 0x10);
            }
            for which in 0..2 {
                let fit = m[model].spline_fit[which].addr();
                if fit != 0 {
                    let path: Ptr32<Model> = Ptr32::new(m.u32(fit + FIT_PATH));
                    if s.addr() == m[path].definition.addr() {
                        let table = m.u32(fit + FIT_TABLE);
                        if table != 0 {
                            m.free(table);
                            m.set_u32(fit + FIT_TABLE, 0);
                        }
                        m.set_u32(fit + FIT_TABLE, 0);
                    }
                }
            }
        }
        node = m[node].next;
    }
    if m[s].unk_70 == 1 {
        let owner = o.call(m, &SI_FIND_PATH_FCURVE_OWNER, &[s.addr()]);
        o.call(m, &SI_UPDATE_FCURVE_PATH_KEYS, &[owner, s.addr()]);
        o.call(m, &SI_CHECK_SPLINE_TO_FCURVE, &[owner, s.addr()]);
        o.call(m, &SI_REFRESH_FCURVE, &[0]);
    }
}

/// Removes key `index` [soft 0x0848af3c SI_splineDeletePoint]; 0 for an index outside the
/// keys in use. A keyframed spline's percentage curve is told first
/// (`SI_deleteFcurvePathKey(owner, index)`, with `index − 1` for the types 2 and 3, as a
/// 16-bit number, and no check that a curve was found). Then the count goes down, the
/// index table gets −1 at `index`, each later key moves down one record (0x30 bytes, the
/// old last record keeps its content) with `SI_gblIndex[i + 1] = i`, and the spline is
/// touched. Returns 1.
pub fn spline_delete_point(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, index: i32) -> i32 {
    if index < 0 || index >= m[s].nb_keys as u16 as i32 {
        return 0;
    }
    if m[s].unk_70 == 1 {
        let at = if (m[s].kind as u32).wrapping_sub(2) <= 1 { (index - 1) as i16 as i32 } else { index as i16 as i32 };
        let owner = o.call(m, &SI_FIND_PATH_FCURVE_OWNER, &[s.addr()]);
        o.call(m, &SI_DELETE_FCURVE_PATH_KEY, &[owner, at as u32]);
    }
    let table = m[SI_GBL_INDEX];
    m[s].nb_keys = m[s].nb_keys.wrapping_sub(1);
    if !table.is_null() {
        m[table.add(index)] = -1;
    }
    let mut i = index;
    while i < m[s].nb_keys as u16 as i32 {
        let to = m[s].keys.add(i);
        let key = m.read(to.add(1));
        m.write(to, key);
        let table = m[SI_GBL_INDEX];
        if !table.is_null() {
            m[table.add(i + 1)] = i;
        }
        i += 1;
    }
    spline_touch(m, o, s);
    1
}

/// Inserts a point before key `index` [soft 0x0848acc0 SI_splineAddPoint] and returns the
/// new key's address.
///
/// - No key block: `SI_splineKeyAllocate(1)` and the index is 0. Else, when the count is a
///   multiple of 16 (0 included), `SI_realloc` to room for 16 more records; the new
///   records hold what the allocator gave.
/// - `index` is clamped to `[0, count]`; the keys from it on move up one record, last
///   first, with `SI_gblIndex[i] = i + 1`.
/// - The new key gets `*p` as `next` handle, position and `prev` handle (copied in that
///   order, each from the one before), the spline's defaults (+0x64, +0x66) as constraint
///   and segment type, and flag 0. Its float at +0x24 and its padding are not set.
/// - A keyframed spline: `SI_addFcurvePathKey(SI_findPathFcurveOwner(spline), index)`,
///   with `index − 1` for the types 2 and 3.
/// - The count goes up by one and the spline is touched.
pub fn spline_add_point(m: &mut Mem, o: &mut dyn Outside, s: Ptr32<Spline>, index: i32, p: u32) -> Ptr32<SplineKey> {
    let mut index = index;
    let keys = m[s].keys;
    if !keys.is_null() {
        let n = m[s].nb_keys as u16 as u32;
        if n & 0xf == 0 {
            let block = m.si_realloc(keys.addr(), (n + 16) * SplineKey::SIZE as u32);
            m[s].keys = Ptr32::new(block);
        }
    } else {
        let block = spline_key_allocate(m, 1);
        m[s].keys = block;
        index = 0;
    }
    let nb = m[s].nb_keys as u16 as i32;
    if index > nb {
        index = nb;
    }
    if index < 0 {
        index = 0;
    }
    let mut i = nb - 1;
    while i >= index {
        let from = m[s].keys.add(i);
        let key = m.read(from);
        m.write(from.add(1), key);
        let table = m[SI_GBL_INDEX];
        if !table.is_null() {
            m[table.add(i)] = i + 1;
        }
        i -= 1;
    }
    let k = m[s].keys.add(index);
    let a = k.addr();
    // next from *p, position from next, prev from position: word by word
    let px = m.u32(p);
    m.set_u32(a + 0x18, px);
    let w = m.u32(p + 4);
    m.set_u32(a + 0x1c, w);
    let w = m.u32(p + 8);
    m.set_u32(a + 0x20, w);
    m.set_u32(a + 0x0c, px);
    let w = m.u32(a + 0x1c);
    m.set_u32(a + 0x10, w);
    let w = m.u32(a + 0x20);
    m.set_u32(a + 0x14, w);
    m.set_u32(a, px);
    let w = m.u32(a + 0x10);
    m.set_u32(a + 4, w);
    let w = m.u32(a + 0x14);
    m.set_u32(a + 8, w);
    m[k].constraint = m[s].unk_64;
    m[k].segment = m[s].unk_66;
    m[k].flag = 0;
    if m[s].unk_70 == 1 {
        let at = if (m[s].kind as u32).wrapping_sub(2) <= 1 { index - 1 } else { index };
        let owner = o.call(m, &SI_FIND_PATH_FCURVE_OWNER, &[s.addr()]);
        o.call(m, &SI_ADD_FCURVE_PATH_KEY, &[owner, at as u32]);
    }
    m[s].nb_keys = m[s].nb_keys.wrapping_add(1);
    spline_touch(m, o, s);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_spline_is_named_from_the_counter() {
        let mut m = Mem::new();
        init(&mut m);
        let a = spline_allocate(&mut m);
        let b = spline_allocate(&mut m);
        assert_eq!(m.cstr(m[a].name.addr()), b"spl1");
        assert_eq!(m.cstr(m[b].name.addr()), b"spl2");
        assert_eq!((m[a].step, m[a].unk_64, m[a].unk_66, m[a].unk_04), (10, 2, 1, 0x10));
        assert_eq!(m[a].matrix, si_math::types::IDENTITY);
        // the allocator's mark in the padding at the end of the record
        assert_eq!(m.u8(a.addr() + 0x73), 0x6b);
        assert!(m.fault().is_none());
    }

    #[test]
    fn a_key_block_has_room_for_a_multiple_of_sixteen() {
        let mut m = Mem::new();
        let k = spline_key_allocate(&mut m, 17);
        assert_eq!(m.block_size(k.addr()), Some(32 * 0x30));
        assert_eq!(m[k.add(16)].unk_24, 1.0f32.to_le_bytes());
        assert_eq!(m[k.add(17)].unk_24, [0; 4]);
        assert!(spline_key_allocate(&mut m, 0).is_null());
    }

    #[test]
    fn deleting_a_point_closes_the_keys_up_and_frees_the_length_table() {
        let mut m = Mem::new();
        init(&mut m);
        // an empty object list
        let lists = m.calloc(1, 0x24);
        m[SI_GRAF_LISTS] = lists;
        let s = spline_allocate(&mut m);
        let keys = spline_key_allocate(&mut m, 3);
        m[s].keys = keys;
        m[s].nb_keys = 3;
        for i in 0..3 {
            m[keys.add(i)].pos.x = i as f32;
        }
        let table = m.calloc(4, 4);
        m[s].lengths = Ptr32::new(table);
        assert_eq!(spline_delete_point(&mut m, &mut NoOutside, s, 1), 1);
        assert_eq!((m[s].nb_keys, m[keys.add(1)].pos.x, m[keys.add(2)].pos.x), (2, 2.0, 2.0));
        assert!(m[s].lengths.is_null() && m.is_freed(table));
        assert_eq!(spline_delete_point(&mut m, &mut NoOutside, s, 2), 0);
        assert!(m.fault().is_none());
    }
}
