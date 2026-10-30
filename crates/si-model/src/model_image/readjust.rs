// SPDX-License-Identifier: MIT
//! A node's working definition rebuilt from its first stored one and what deforms it
//! (`SI_modelReadjustDefinition`), with what it needs of `model.c` on the image: a
//! definition through a matrix (`SI_modelMultiplyDef`), the working definition given up
//! (`SI_modelDeleteDefinition`), and the eight small questions of `model.c`, `lattice.c`
//! and `wave_sys.c` about what deforms a node (eighth pass of finding 0015), with
//! `wave_sys.c`'s ninth, `SI_modelWave`, which the update asks. The store forms are in
//! [`crate::node_copy`] and [`crate::transform`].

use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x;

use super::{callee, elem_dict, g, mat, models_chapter, put_mat, Callee, Outside};
use crate::mesh_image;

/// The routines of other source files only the readjusting calls, each `name (0xADDRESS)`.
pub mod callees {
    use super::Callee;

    /// `SI_waveTransform` (0x084c7640)
    pub const SI_WAVE_TRANSFORM: Callee = Callee { name: "SI_waveTransform", addr: 0x084c7640 };
    /// `SI_latticeTransform` (0x0843f5c4)
    pub const SI_LATTICE_TRANSFORM: Callee = Callee { name: "SI_latticeTransform", addr: 0x0843f5c4 };
    /// `SI_splineFit` (0x08471250; `model.c`: on the image in [`super::super::fit`], called
    /// through here)
    pub const SI_SPLINE_FIT: Callee = Callee { name: "SI_splineFit", addr: 0x08471250 };
    /// `SI_patchFit` (0x0846f444; `model.c`, not ported)
    pub const SI_PATCH_FIT: Callee = Callee { name: "SI_patchFit", addr: 0x0846f444 };
    /// `SI_edFindUpdateNode` (0x0822b2a4)
    pub const SI_ED_FIND_UPDATE_NODE: Callee = Callee { name: "SI_edFindUpdateNode", addr: 0x0822b2a4 };
    /// `SI_checkActiveTime` (0x0822ab70)
    pub const SI_CHECK_ACTIVE_TIME: Callee = Callee { name: "SI_checkActiveTime", addr: 0x0822ab70 };
    /// `SI_animQuickStretchUpdate` (0x085a144c; `qstretch.c`)
    pub const SI_ANIM_QUICK_STRETCH_UPDATE: Callee = Callee { name: "SI_animQuickStretchUpdate", addr: 0x085a144c };
    /// `SI_animMdlClusterCnsUpdate` (0x08223788)
    pub const SI_ANIM_MDL_CLUSTER_CNS_UPDATE: Callee = Callee { name: "SI_animMdlClusterCnsUpdate", addr: 0x08223788 };
    /// `SI_metaSystemDispose` (0x0845c8fc)
    pub const SI_META_SYSTEM_DISPOSE: Callee = Callee { name: "SI_metaSystemDispose", addr: 0x0845c8fc };
    /// `SI_patchRemoveFitting` (0x0843a580)
    pub const SI_PATCH_REMOVE_FITTING: Callee = Callee { name: "SI_patchRemoveFitting", addr: 0x0843a580 };
    /// Every routine of this table.
    pub const ALL: &[Callee] = &[
        SI_WAVE_TRANSFORM,
        SI_LATTICE_TRANSFORM,
        SI_SPLINE_FIT,
        SI_PATCH_FIT,
        SI_ED_FIND_UPDATE_NODE,
        SI_CHECK_ACTIVE_TIME,
        SI_ANIM_QUICK_STRETCH_UPDATE,
        SI_ANIM_MDL_CLUSTER_CNS_UPDATE,
        SI_META_SYSTEM_DISPOSE,
        SI_PATCH_REMOVE_FITTING,
    ];
}

/// The globals the readjusting reads besides those of [`super::g`].
pub mod rg {
    si_mem::globals! {
        /// Not zero while the animation sets the update time (`SI_updateTime`).
        SI_UPDATE_TIME_SET: i16 = 0x0869bca2, "SI_updateTimeSet";
        /// The time of the update in seconds while `SI_updateTimeSet` is set.
        SI_UPDATE_TIME: f32 = 0x0869bca4, "SI_updateTime";
        /// Not zero in the interactive program.
        SI_INTERACTIVE: i16 = 0x0867953e, "SI_interactive";
    }
}

/// [soft 0x0843ff68 SI_modelNodeLattice] on the image: the first lattice of set 0 when the
/// set has any and that lattice's first word is not zero, else null.
pub fn node_lattice(m: &Mem, node: Ptr32<Model>) -> u32 {
    if m[node].nb_lattices0 != 0 {
        let l = m.u32(m[node].lattices0.addr());
        if m.i16(l) != 0 {
            return l;
        }
    }
    0
}

/// [soft 0x0843ff88 SI_modelTreeLattice] on the image: the same for set 1 (the branch's).
pub fn tree_lattice(m: &Mem, node: Ptr32<Model>) -> u32 {
    if m[node].nb_lattices1 != 0 {
        let l = m.u32(m[node].lattices1.addr());
        if m.i16(l) != 0 {
            return l;
        }
    }
    0
}

fn active(m: &Mem, p: u32) -> u32 {
    if p != 0 && m.i16(p) != 0 {
        p
    } else {
        0
    }
}

/// [soft 0x08472b88 SI_modelNodeSplineFitting] on the image: the node's own spline
/// deformation (+0x60) when its first word is not zero.
pub fn node_spline_fitting(m: &Mem, node: Ptr32<Model>) -> u32 {
    active(m, m[node].spline_fit[0].addr())
}

/// [soft 0x08472ba4 SI_modelTreeSplineFitting] on the image: the branch's (+0x64).
pub fn tree_spline_fitting(m: &Mem, node: Ptr32<Model>) -> u32 {
    active(m, m[node].spline_fit[1].addr())
}

/// [soft 0x08472c88 SI_modelNodePatchFitting] on the image: the node's own patch
/// deformation (+0x2d8).
pub fn node_patch_fitting(m: &Mem, node: Ptr32<Model>) -> u32 {
    active(m, m[node].patch_fit[0].addr())
}

/// [soft 0x08472ca4 SI_modelBranchPatchFitting] on the image: the branch's (+0x2dc).
pub fn branch_patch_fitting(m: &Mem, node: Ptr32<Model>) -> u32 {
    active(m, m[node].patch_fit[1].addr())
}

fn wave_with(m: &Mem, node: Ptr32<Model>, link: i32) -> u32 {
    let mut w = m[node].waves.addr();
    while w != 0 {
        if m.i32(w + 4) == link {
            return m[node].waves.addr();
        }
        w = m.u32(w + 8);
    }
    0
}

/// [soft 0x084c7610 SI_modelNodeWave] on the image: the head of the node's wave list
/// (+0x2e8) when an entry of it has link word (+4) 0, else null; the list follows +8.
pub fn node_wave(m: &Mem, node: Ptr32<Model>) -> u32 {
    wave_with(m, node, 0)
}

/// [soft 0x084c75dc SI_modelBranchWave] on the image: the same for link word 1.
pub fn branch_wave(m: &Mem, node: Ptr32<Model>) -> u32 {
    wave_with(m, node, 1)
}

/// [soft 0x084c759c SI_modelWave] on the image: the head of the node's wave list (+0x2e8),
/// null without one. The 16-bit word at `set` is cleared, then set to 1 when an entry of
/// the list (followed through +8) has link word (+4) 1, the walk stopping there; the head
/// is read again for the answer.
pub fn model_wave(m: &mut Mem, node: Ptr32<Model>, set: u32) -> u32 {
    let head = m[node].waves.addr();
    m.set_i16(set, 0);
    if head == 0 {
        return 0;
    }
    let mut w = head;
    while w != 0 {
        if m.i32(w.wrapping_add(4)) == 1 {
            m.set_i16(set, 1);
            break;
        }
        w = m.u32(w.wrapping_add(8));
    }
    m[node].waves.addr()
}

/// The points of a spline (`SI_Spline` at `s`: the type at +0xc, the key count at +0x10, the
/// keys of 0x30 bytes at +0x14) through a matrix: of a spline of type 1 the three points of
/// each key, of any other the position (+0xc) alone, each as
/// [`crate::transform::point_through`] forms it.
fn spline_through(m: &mut Mem, s: u32, mm: &si_math::types::Matrix) {
    let keys = m.u32(s + 0x14);
    let three = m.i32(s + 0xc) == 1;
    let mut k = 0u32;
    if m.u16(s + 0x10) == 0 {
        return;
    }
    loop {
        let key = keys.wrapping_add(0x30 * k);
        let points: &[u32] = if three { &[0, 0xc, 0x18] } else { &[0xc] };
        for &o in points {
            let p = mesh_image::f3(m, key + o);
            mesh_image::put_f3(m, key + o, crate::transform::key_through(p, mm));
        }
        k += 1;
        if k >= m.u16(s + 0x10) as u32 {
            break;
        }
    }
}

/// [soft 0x0846e918 SI_modelMultiplyDef] on the image: the definition at `def` of kind
/// `kind` through the matrix at `matrix`. The transpose of the matrix's inverse is formed
/// first whatever the kind (`SI_matrixTransposeInverse`, `si-math`). Then:
///
/// - patch (1): the points (+0x1a4, `s16(+0xc) × s16(+0xe)` of them, 16 bytes) through
///   `SI_hvectorMultiplyMatrix`, the two evaluated arrays (+0x1b0 and +0x1b8, when there,
///   `s16 × s16` vectors of 12 bytes each, counts at +0x1b4/+0x1b6 and +0x1bc/+0x1be)
///   through `SI_vectorMultiplyMatrix`; the short at +0x1c0 becomes 0;
/// - face (2): with the short at +0x84 zero, the control points of each NURBS curve (+0x80,
///   100 bytes each, `s16(+0x78)` of them; points of 0x24 bytes at +0x2c of a curve, their
///   count the word at +4) rounded from double to float, sent through the matrix as the
///   store form says and stored back as doubles (the weight stays); else the outline (+4)
///   and each hole (+0x7c, 0x74 bytes, `s16(+0x78)` of them) as splines;
/// - mesh (4): [`mesh_image::obj::mesh_transform`] with the matrix and that transpose;
/// - spline (6): its keys;
/// - NURBS curve and surface (9, 10): `SI_nurbsCrvApplyMatrix` / `SI_nurbsSrfApplyMatrix`
///   (the matrix, 0) through `o`;
/// - any other kind: nothing.
///
/// (Measured on the sample scenes for meshes, splines, faces of spline contours and
/// patches; the face of NURBS curves is the store form's reading, not reached there.)
pub fn multiply_def(m: &mut Mem, o: &mut dyn Outside, matrix: u32, kind: i32, def: u32) {
    let mm = mat(m, matrix);
    let (t, _) = si_math::algebra::matrix_transpose_inverse(&mm);
    let normals = m.local(0x40);
    put_mat(m, normals, &t);
    match kind {
        1 => {
            let n = (m.i16(def + 0xc) as i32).wrapping_mul(m.i16(def + 0xe) as i32);
            let pts = m.u32(def + 0x1a4);
            for k in 0..n.max(0) as u32 {
                let a = pts.wrapping_add(16 * k);
                let h = si_math::types::Hvector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8), m.f32(a + 12));
                let r = si_math::algebra::hvector_multiply_matrix(&h, &mm).to_array();
                for (c, v) in r.iter().enumerate() {
                    m.set_f32(a + 4 * c as u32, *v);
                }
            }
            for (arr, cnt) in [(0x1b0u32, 0x1b4u32), (0x1b8, 0x1bc)] {
                let a = m.u32(def + arr);
                if a != 0 {
                    let n = (m.i16(def + cnt) as i32).wrapping_mul(m.i16(def + cnt + 2) as i32);
                    for k in 0..n.max(0) as u32 {
                        let at = a.wrapping_add(12 * k);
                        let v = si_math::types::Vector::new(m.f32(at), m.f32(at + 4), m.f32(at + 8));
                        let r = si_math::algebra::vector_multiply_matrix(&v, &mm);
                        mesh_image::put_f3(m, at, [r.x, r.y, r.z]);
                    }
                }
            }
            m.set_u16(def + 0x1c0, 0);
        }
        2 => {
            if m.i16(def + 0x84) == 0 {
                let mut k = 0i16;
                while k < m.i16(def + 0x78) {
                    let curve = m.u32(def + 0x80).wrapping_add((k as i32 as u32).wrapping_mul(100));
                    k += 1;
                    let mut p = 0i32;
                    while p < m.i32(curve + 4) {
                        let at = m.u32(curve + 0x2c).wrapping_add((p as u32).wrapping_mul(0x24));
                        let v = [m.f64(at) as f32, m.f64(at + 8) as f32, m.f64(at + 0x10) as f32];
                        let r = &mm;
                        m.set_f64(at, (((x(v[0]) * r[0][0] + x(v[1]) * r[1][0]) + x(v[2]) * r[2][0]) + r[3][0]).to_f64());
                        m.set_f64(at + 8, (((x(v[1]) * r[1][1] + x(v[0]) * r[0][1]) + x(v[2]) * r[2][1]) + r[3][1]).to_f64());
                        m.set_f64(at + 0x10, (((x(v[1]) * r[1][2] + x(v[0]) * r[0][2]) + x(v[2]) * r[2][2]) + r[3][2]).to_f64());
                        p += 1;
                    }
                }
            } else {
                let mut k = -1i16;
                while k < m.i16(def + 0x78) {
                    let s = if k == -1 { def + 4 } else { m.u32(def + 0x7c).wrapping_add((k as i32 as u32).wrapping_mul(0x74)) };
                    spline_through(m, s, &mm);
                    k += 1;
                }
            }
        }
        4 => mesh_image::obj::mesh_transform(m, def, matrix, normals),
        6 => spline_through(m, def, &mm),
        9 => {
            o.call(m, &callee::SI_NURBS_CRV_APPLY_MATRIX, &[def, matrix, 0]);
        }
        10 => {
            o.call(m, &callee::SI_NURBS_SRF_APPLY_MATRIX, &[def, matrix, 0]);
        }
        _ => {}
    }
    m.release(0x40);
}

/// `SI_modelDeleteDefinition` (0x084678a8) on the image; unchecked (the sample scenes reach
/// it only with `keep` set on a node that works on its first stored definition, where it
/// changes nothing; the store form is cited). Nothing for a node without
/// stored definitions. By kind, the working definition (+0x48) is disposed of unless it is
/// the first stored one; with `keep` 0 also every stored definition and the original one
/// (+0x3c; not for a metaball), then the array is freed and kind, count, array, working and
/// original definitions are cleared; with `keep` set the working definition becomes the
/// first stored one. A metaball system (7) is disposed of and nothing else is done but
/// that last step. The disposal routines: the mesh's and the face's of this crate,
/// `si-hrc`'s of a spline, the others (patch, NURBS curve and surface, metaball, metaball
/// system, and `SI_patchRemoveFitting` after a patch's) through `o`.
pub fn delete_definition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, keep: i16) {
    if m[node].nb_defs == 0 {
        return;
    }
    let a = node.addr();
    let dispose = |m: &mut Mem, o: &mut dyn Outside, kind: i32, pp: u32| match kind {
        1 => {
            o.call(m, &callee::SI_PATCH_DISPOSE, &[pp]);
        }
        2 => mesh_image::face::face_dispose(m, pp),
        4 => mesh_image::obj::mesh_dispose(m, pp),
        6 => si_hrc::image::spline::spline_dispose(m, pp),
        8 => {
            o.call(m, &callee::SI_METABALL_DISPOSE, &[pp]);
        }
        9 => {
            o.call(m, &callee::SI_NURBS_CRV_DISPOSE, &[pp]);
        }
        _ => {
            o.call(m, &callee::SI_NURBS_SRF_DISPOSE, &[pp]);
        }
    };
    let kind = m.i32(a + 0x30);
    let mut cleared = false;
    match kind {
        1 | 2 | 4 | 6 | 8 | 9 | 10 => {
            if m.u32(a + 0x48) != m.u32(m.u32(a + 0x38)) {
                dispose(m, o, kind, a + 0x48);
            }
            if keep == 0 {
                let mut k = 0i16;
                if m.u16(a + 0x34) != 0 {
                    loop {
                        dispose(m, o, kind, m.u32(a + 0x38).wrapping_add((k as i32 as u32).wrapping_mul(4)));
                        k += 1;
                        if k as i32 >= m.u16(a + 0x34) as i32 {
                            break;
                        }
                    }
                }
                if kind != 8 && m.u32(a + 0x3c) != 0 {
                    dispose(m, o, kind, a + 0x3c);
                }
                if kind == 1 {
                    o.call(m, &callees::SI_PATCH_REMOVE_FITTING, &[a]);
                }
                cleared = true;
            }
        }
        7 => {
            o.call(m, &callees::SI_META_SYSTEM_DISPOSE, &[a + 0x48]);
            cleared = keep == 0;
        }
        _ => cleared = keep == 0,
    }
    if cleared {
        let arr = m.u32(a + 0x38);
        if arr != 0 {
            m.free(arr);
            m.set_u32(a + 0x38, 0);
        }
        m.set_u32(a + 0x38, 0);
        m.set_u32(a + 0x48, 0);
        m.set_u32(a + 0x3c, 0);
        m.set_i32(a + 0x30, 0);
        m.set_u16(a + 0x34, 0);
    } else {
        m.set_u32(a + 0x48, m.u32(m.u32(a + 0x38)));
    }
}

/// [soft 0x0846e340 SI_modelReadjustDefinition] on the image. The time of the
/// deformations: `SI_updateTime` while `SI_updateTimeSet` is set, else `SI_frameCurrent /
/// SI_frameRate` rounded to float. In the original's order (the store form,
/// [`crate::node_copy::readjust_definition`], says what each step means):
///
/// 1. unless `SI_glbAnim` + 6 is set and the node's envelope word (+0x31c) is 5, 6 or 8,
///    the first stored definition's points go into the working one by the kind's transfer
///    routine (a mesh's here, the others through `o`);
/// 2. the node's element (`SI_edGetElemFromModel(node, &local)`, through `o`);
/// 3. with stored definitions: the node's own wave (`SI_waveTransform(wave, node, 0, time,
///    0)`), lattice, spline and patch deformations (`SI_latticeTransform`, `SI_splineFit`,
///    `SI_patchFit` with kind, working definition twice), each through `o`; with an element,
///    the quick-stretch relation (0xef: `SI_elemdictFindRelatedElem`, then
///    `SI_edFindUpdateNode` without cursor, whose node must have an argument block (+0x1c)
///    whose short at +0xc is set and its own short at +0x20 set, then
///    `SI_checkActiveTime(node, time, &value)` and `SI_animQuickStretchUpdate(node, value)`)
///    and the two cluster-constraint relations (0xf6 and 0xf7: every update node the cursor
///    finds, active at the time, through `SI_animMdlClusterCnsUpdate(node, 0, 0 or 1, 0,
///    time)`); the branch's wave (link 1), lattice, spline and patch deformations of the
///    node; then, when the node's geometry lives in another node's space (`latdep`, +0x44),
///    up the fathers until that node: the definition through the def matrix (+0x68) of the
///    node walked from ([`multiply_def`]; not for an enveloped node in the interactive
///    program whose flag word +8 is negative), then the branch deformations of the node
///    walked to; anything that deformed makes the node not "plain";
/// 4. a plain node that is its own `latdep` and whose envelope is not active
///    (`SI_envelopActive`, through `o`) gives up its working definition
///    ([`delete_definition`] keeping the stored ones);
/// 5. [`super::touch::touch_definition`].
///
/// The walk of step 3 follows the fathers without a test, as the original (whose walk
/// through a null father reads address 0).
pub fn readjust_definition(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    let a = node.addr();
    let time = if m[rg::SI_UPDATE_TIME_SET] == 0 { (si_x87::F80::from_i32(m[g::SI_FRAME_CURRENT]) / m[g::SI_FRAME_RATE]).to_f32() } else { m[rg::SI_UPDATE_TIME] };
    let t = time.to_bits();
    let mut plain = true;
    let enveloped = |m: &Mem| {
        let e = m.i32(a + 0x31c);
        (e.wrapping_sub(5) as u32) <= 1 || e == 8
    };
    if m[g::SI_GLB_ANIM][3] == 0 || !enveloped(m) {
        // (the first stored definition is read only for the six kinds)
        let pair = |m: &Mem| [m.u32(m.u32(a + 0x38)), m.u32(a + 0x48)];
        match m.i32(a + 0x30) {
            1 => {
                let p = pair(m);
                o.call(m, &callee::SI_PATCH_TRANSFER_POINTS, &p);
            }
            2 => {
                let p = pair(m);
                o.call(m, &callee::SI_FACE_TRANSFER_POINTS, &p);
            }
            4 => {
                let [stored, cur] = pair(m);
                mesh_image::obj::mesh_transfer_vertices(m, stored, cur)
            }
            6 => {
                let p = pair(m);
                o.call(m, &callee::SI_SPLINE_TRANSFER_POINTS, &p);
            }
            9 => {
                let p = pair(m);
                o.call(m, &callee::SI_NURBS_CRV_TRANSFER_POINTS_KNOTS, &p);
            }
            10 => {
                let p = pair(m);
                o.call(m, &callee::SI_NURBS_SRF_TRANSFER_POINTS_KNOTS, &p);
            }
            _ => {}
        }
    }
    let local = m.local(4);
    let elem = o.call(m, &callee::SI_ED_GET_ELEM_FROM_MODEL, &[a, local]);
    if m.u16(a + 0x34) != 0 {
        let deform = |m: &mut Mem, o: &mut dyn Outside, c: &Callee, d: u32| {
            let (kind, cur) = (m.u32(a + 0x30), m.u32(a + 0x48));
            o.call(m, c, &[d, kind, cur, cur]);
        };
        let w = node_wave(m, node);
        if w != 0 {
            plain = false;
            o.call(m, &callees::SI_WAVE_TRANSFORM, &[w, a, 0, t, 0]);
        }
        let l = node_lattice(m, node);
        if l != 0 {
            plain = false;
            deform(m, o, &callees::SI_LATTICE_TRANSFORM, l);
        }
        let f = node_spline_fitting(m, node);
        if f != 0 {
            plain = false;
            deform(m, o, &callees::SI_SPLINE_FIT, f);
        }
        let f = node_patch_fitting(m, node);
        if f != 0 {
            plain = false;
            deform(m, o, &callees::SI_PATCH_FIT, f);
        }
        if elem != 0 {
            let find = |m: &mut Mem, o: &mut dyn Outside, relation: u32| {
                let dict = elem_dict(m);
                let chap = models_chapter(m);
                o.call(m, &callee::SI_ELEMDICT_FIND_RELATED_ELEM, &[dict, elem, 1, 2, relation, chap, local, local])
            };
            let list = |m: &Mem| elem_dict(m).wrapping_add(0xc);
            if find(m, o, 0xef) != 0 {
                let l = list(m);
                let u = o.call(m, &callees::SI_ED_FIND_UPDATE_NODE, &[l, a, 0xef, 0]);
                let ready = u != 0 && m.u32(u + 0x1c) != 0 && m.i16(u + 0x20) != 0 && m.i16(m.u32(u + 0x1c) + 0xc) != 0;
                if ready {
                    let value = m.local(4);
                    if o.call(m, &callees::SI_CHECK_ACTIVE_TIME, &[u, t, value]) as u16 != 0 {
                        plain = false;
                        let v = m.u32(value);
                        o.call(m, &callees::SI_ANIM_QUICK_STRETCH_UPDATE, &[u, v]);
                    }
                    m.release(4);
                }
            }
            for (relation, which) in [(0xf6u32, 0u32), (0xf7, 1)] {
                if find(m, o, relation) != 0 {
                    let cursor = m.local(4);
                    let value = m.local(4);
                    m.set_u32(cursor, 0);
                    loop {
                        let l = list(m);
                        let u = o.call(m, &callees::SI_ED_FIND_UPDATE_NODE, &[l, a, relation, cursor]);
                        if u == 0 {
                            break;
                        }
                        if o.call(m, &callees::SI_CHECK_ACTIVE_TIME, &[u, t, value]) as u16 != 0 {
                            o.call(m, &callees::SI_ANIM_MDL_CLUSTER_CNS_UPDATE, &[u, 0, which, 0, t]);
                            plain = false;
                        }
                    }
                    m.release(4);
                    m.release(4);
                }
            }
        }
        let branch = |m: &mut Mem, o: &mut dyn Outside, of: Ptr32<Model>, plain: &mut bool, mark: bool| {
            let w = branch_wave(m, of);
            if w != 0 {
                if mark {
                    *plain = false;
                }
                o.call(m, &callees::SI_WAVE_TRANSFORM, &[w, a, 1, t, 0]);
            }
            let l = tree_lattice(m, of);
            if l != 0 {
                if mark {
                    *plain = false;
                }
                deform(m, o, &callees::SI_LATTICE_TRANSFORM, l);
            }
            let f = tree_spline_fitting(m, of);
            if f != 0 {
                if mark {
                    *plain = false;
                }
                deform(m, o, &callees::SI_SPLINE_FIT, f);
            }
            let f = branch_patch_fitting(m, of);
            if f != 0 {
                if mark {
                    *plain = false;
                }
                deform(m, o, &callees::SI_PATCH_FIT, f);
            }
        };
        branch(m, o, node, &mut plain, true);
        let mut latdep = m.u32(a + 0x44);
        if latdep != a {
            let mut from = a;
            let mut to = m[node].father;
            loop {
                let held = enveloped(m) && m[rg::SI_INTERACTIVE] != 0 && m.i16(a + 8) < 0;
                if !held {
                    let (kind, cur) = (m.i32(a + 0x30), m.u32(a + 0x48));
                    multiply_def(m, o, from + 0x68, kind, cur);
                }
                branch(m, o, to, &mut plain, false);
                from = to.addr();
                to = m[to].father;
                latdep = m.u32(a + 0x44);
                if from == latdep {
                    break;
                }
            }
        }
        if plain && latdep == a && o.call(m, &callee::SI_ENVELOP_ACTIVE, &[a]) as u16 == 0 {
            delete_definition(m, o, node, 1);
        }
    }
    m.release(4);
    super::touch::touch_definition(m, o, node);
}

/// `SI_modelUpdateDependency` (0x0846f200) on the image, checked on 32 sample scenes with
/// the copies paired block by block (`emu_scene8r.py --dependency`, `model-scenecheck` with
/// `SCENECHECK_HARNESS_HEAP=1`). For `node`, its following
/// brothers and everything below them, which node's deformation each geometry depends on.
/// `holder` is the nearest node above whose branch deformation acts on the node (null:
/// none); `latdep` (+0x44) becomes the holder, or the node itself.
///
/// A node under a holder, or deformed itself (`SI_modelSplineFitting`, `SI_modelLattice`,
/// `SI_modelPatchFitting` with one 16-bit local, the node's or the branch's wave, or the
/// short at +0x352 above 0; the questions stop at the first yes), works on a definition of
/// its own: when it has stored definitions and works on the one numbered `cur_def` (+0x36),
/// that one is copied by its kind's routine (this crate's mesh copy, `si-hrc`'s spline
/// copy, the others through `o`; other kinds keep theirs), then the definition is
/// readjusted ([`readjust_definition`]). Any other node with stored definitions that works
/// on another one gives it up (the kind's disposal, then the stored one numbered `cur_def`
/// again; other kinds only the touch) and has its geometry touched
/// (`super::touch::touch_geometry`). Then the following brother with the same holder, then
/// the first son with this node as holder if it has a branch deformation (lattice, spline,
/// wave, patch, in that order) and there was none above. The recursion is Rust recursion.
pub fn update_dependency(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, holder: u32) {
    let a = node.addr();
    m.set_u32(a + 0x44, if holder != 0 { holder } else { a });
    let deformed = holder != 0 || {
        let set = m.local(4);
        let yes = super::defs::spline_fitting(m, node, set) != 0
            || super::defs::lattice(m, node, set) != 0
            || super::defs::patch_fitting(m, node, set) != 0
            || node_wave(m, node) != 0
            || branch_wave(m, node) != 0
            || m.i16(a + 0x352) > 0;
        m.release(4);
        yes
    };
    let stored = |m: &Mem| m.u32(m.u32(a + 0x38).wrapping_add((m.u16(a + 0x36) as u32).wrapping_mul(4)));
    if deformed {
        if m.u16(a + 0x34) != 0 && m.u32(a + 0x48) == stored(m) {
            let s = stored(m);
            let copy = match m.i32(a + 0x30) {
                1 => Some(o.call(m, &callee::SI_PATCH_COPY, &[s])),
                2 => Some(o.call(m, &callee::SI_FACE_COPY, &[s])),
                4 => Some(mesh_image::obj::mesh_copy(m, o, s)),
                6 => Some(si_hrc::image::spline::spline_copy(m, s).addr()),
                9 => Some(o.call(m, &callee::SI_NURBS_CRV_COPY, &[s])),
                10 => Some(o.call(m, &callee::SI_NURBS_SRF_COPY, &[s])),
                _ => None,
            };
            if let Some(c) = copy {
                m.set_u32(a + 0x48, c);
            }
        }
        readjust_definition(m, o, node);
    } else if m.u16(a + 0x34) != 0 && m.u32(a + 0x48) != stored(m) {
        let pp = a + 0x48;
        let disposed = match m.i32(a + 0x30) {
            1 => {
                o.call(m, &callee::SI_PATCH_DISPOSE, &[pp]);
                true
            }
            2 => {
                mesh_image::face::face_dispose(m, pp);
                true
            }
            4 => {
                mesh_image::obj::mesh_dispose(m, pp);
                true
            }
            6 => {
                si_hrc::image::spline::spline_dispose(m, pp);
                true
            }
            9 => {
                o.call(m, &callee::SI_NURBS_CRV_DISPOSE, &[pp]);
                true
            }
            10 => {
                o.call(m, &callee::SI_NURBS_SRF_DISPOSE, &[pp]);
                true
            }
            _ => false,
        };
        if disposed {
            m.set_u32(a + 0x48, stored(m));
        }
        super::touch::touch_geometry(m, o, node);
    }
    let brother = m.u32(a + 0x2c0);
    if brother != 0 {
        update_dependency(m, o, Ptr32::new(brother), holder);
    }
    let mut h = holder;
    if h == 0 && (tree_lattice(m, node) != 0 || tree_spline_fitting(m, node) != 0 || branch_wave(m, node) != 0 || branch_patch_fitting(m, node) != 0) {
        h = a;
    }
    let son = m.u32(a + 0x2c4);
    if son != 0 {
        update_dependency(m, o, Ptr32::new(son), h);
    }
}
