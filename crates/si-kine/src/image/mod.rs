// SPDX-License-Identifier: MIT
//! The solver, the joints' rotation limits, the two limit constraints and the direction
//! constraints on the shared memory image (third pass of the package; finding 0022, "Third
//! pass: the solver on the image").
//!
//! The first pass wrote these routines on `si-model`'s node arena with the records in an
//! [`crate::rec::IkStore`]; those forms stay as they are (their checks keep their numbers)
//! and are the reference for the algorithm: every function here does the same arithmetic in
//! the same order (finding 0022 has it), but takes the image and the original's arguments,
//! reads and writes the records where the original has them (a node is an `__SI_Model`, its
//! record the `SI_InvKin` its +0x318 points to, the root's two matrices `SI_GenMatrixDbl`
//! heap records), and makes the original's allocation calls (`SI_invKinAllocate`'s
//! `SI_malloc(0x74)`, the four work vectors of the Greville inverse and their `free`, the
//! two bytes of relation 116's data).
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`greville`] | `matrix_gen.c` | the pseudo-inverse on the image's matrices, with its work vectors |
//! | [`tools`] | `iktools.c` | limb lengths, the reachable volume, the chain's own update |
//! | [`jacobian`] | `jacobian.c` | the solver |
//! | [`invkin`] | `invkin.c` | records, goals, sub-chains, preferred angles, `SI_modelSolveInvKin` |
//! | [`rotlim`] | `constraint.c` | the joints' rotation limits as the solver uses them |
//! | [`limits`] | `constraint.c` | the updates of relations 102 and 103 |
//! | [`mapdir`] | `constraint.c` | the orientation step of the three-point, normal and tangency constraints |
//! | [`qstretch`] | `qstretch.c` (and two small routines of `qstretch_misc.c`, `settings.c`) | the quick stretch's update (finding 0032) |
//!
//! Callees outside the package go through [`crate::outside::Outside`], as in the second
//! pass: the node update (`SI_modelUpdateSonState`), the node setters, the dictionary, the
//! expressions' curve list, the classical-scaling solver. Mathematics on values (`si-math`,
//! `si-x87`) is called directly.
//!
//! **Where the image form knowingly differs** (said at the spot): where the original walks
//! through a null `son` or `father` pointer (a chain without an effector, a limb without a
//! father) the walk stops, a fault is recorded on the image and counted in
//! [`crate::fault`]; a matrix too small for a limb is used as it is (the original overruns
//! its rows; the image records the read or write outside a block when it leaves the heap).

use si_dict::globals::{SI_CID, SI_ELEM_DICTS, SI_GLB_MANIP};
use si_mem::{record, Access, Mem, Ptr32};
use si_types::geom::Vector;
use si_types::model::Model;

use crate::globals::SI_PROC_DATA;
use crate::outside as out;
use crate::outside::Outside;

pub mod greville;
pub mod invkin;
pub mod jacobian;
pub mod limits;
pub mod mapdir;
pub mod qscalc;
pub mod qstretch;
pub mod rotlim;
pub mod tools;

/// A node of a model hierarchy on the image.
pub type Node = Ptr32<Model>;

record! {
    /// `SI_InvKin`, the record at `__SI_Model` + 0x318 (finding 0022, "The record"; the
    /// first pass's [`crate::rec::InvKin`] has the same layout).
    pub struct IkRec = "SI_InvKin", size 0x74 {
        0x00 kind: i32, "on a root the chain type (10, 20, 30, 40); on a joint 0 or 1";
        0x04 nb_joints: i32, "joints of the chain (root)";
        0x08 goal: Vector, "an effector's goal, global";
        0x14 drot: Vector, "the change of the angles in the last step";
        0x20 prot: Vector, "the preferred angles";
        0x2c jacobian: Ptr32<GenMat>, "root: the Jacobian";
        0x30 inverse: Ptr32<GenMat>, "root: its pseudo-inverse";
        0x34 loaded: i16, "1 after the file reader";
        0x36 pad_36: [u8; 2], "not written by SI_invKinAllocate";
        0x38 state: i32, "an effector's goal: 0 none, 1 set, 2 solved";
        0x3c use_pref: i16, "the limb starts from the preferred angles";
        0x3e pad_3e: [u8; 2], "not written by SI_invKinAllocate";
        0x40 saved_rot: Vector, "the angles on entry of an iterative solve";
        0x4c cns_type: i32, "first joint: 0 none, 1 preferred axis, 2 up vector";
        0x50 cns_pos: Vector, "the constraining object's position";
        0x5c plane_roll: Vector, "the plane's vector after the roll";
        0x68 plane_up: Vector, "the plane's second vector, global";
    }
}

record! {
    /// `SI_GenMatrixDbl` (finding 0008): a heap record of 12 bytes, a table of row
    /// pointers, each row a block of `cols` doubles.
    pub struct GenMat = "SI_GenMatrixDbl", size 0xc {
        0x00 rows: i32, "rows";
        0x04 cols: i32, "columns";
        0x08 row_table: Ptr32<u32>, "the row pointers";
    }
}

/// Element `(i, j)` of a matrix on the image: the double at row `i`'s pointer + 8 j.
pub(crate) fn mat_at(m: &Mem, mat: Ptr32<GenMat>, i: u32, j: u32) -> u32 {
    let table = m[mat].row_table.addr();
    m.u32(table.wrapping_add(4 * i)).wrapping_add(8 * j)
}

/// The record of a node (its +0x318), null or not.
pub(crate) fn ik(m: &Mem, n: Node) -> Ptr32<IkRec> {
    Ptr32::new(m[n].inv_kin.addr())
}

/// A null of algorithm 1 or 2.
pub(crate) fn is_root(m: &Mem, n: Node) -> bool {
    m[n].kind == 0 && (m[n].algo_type.wrapping_sub(1) as u32) <= 1
}

/// A null of algorithm 3 or 4.
pub(crate) fn is_effector(m: &Mem, n: Node) -> bool {
    m[n].kind == 0 && (m[n].algo_type.wrapping_sub(3) as u32) <= 1
}

/// A root, a joint or an effector.
pub(crate) fn is_chain_node(m: &Mem, n: Node) -> bool {
    is_root(m, n) || m[n].kind == 5 || is_effector(m, n)
}

/// Where the original follows a pointer it does not test and it is null: the read it
/// would fault on is recorded on the image (`at` is the field's offset), the use counted
/// in [`crate::fault`], and the walk stops.
pub(crate) fn null_walk(m: &Mem, at: u32) {
    crate::fault::note();
    m.record_fault(at, 4, Access::Read);
}

/// The first son, which the original takes without a test; `None` (a recorded fault) for
/// null.
pub(crate) fn son(m: &Mem, n: Node) -> Option<Node> {
    let s = m[n].son;
    if s.is_null() {
        null_walk(m, 0x30);
        None
    } else {
        Some(s)
    }
}

/// The father, likewise.
pub(crate) fn father(m: &Mem, n: Node) -> Option<Node> {
    let f = m[n].father;
    if f.is_null() {
        null_walk(m, 0x178);
        None
    } else {
        Some(f)
    }
}

/// The node `n` sons below `node`, stopping at a null son (recorded).
pub(crate) fn down(m: &Mem, node: Node, n: i32) -> Node {
    let mut p = node;
    for _ in 0..n.max(0) {
        match son(m, p) {
            Some(s) => p = s,
            None => break,
        }
    }
    p
}

/// What the routines read of `SI_glbManip`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Manip {
    /// +0x00 is 1: classical scaling.
    pub classical: bool,
    /// +0x04: uniform scaling, compared with 1.
    pub uniform: i32,
    /// +0x20 (16 bits): continuous rotation, `SI_matrixExtractXyzRotation`'s switch.
    pub keep_turns: bool,
    /// +0x2c (16 bits): `SI_IK_OLD_JOINT_ROLL` was set at start-up.
    pub old_roll: bool,
}

impl Manip {
    pub fn read(m: &Mem) -> Manip {
        let g = SI_GLB_MANIP.addr();
        Manip { classical: m.i32(g) == 1, uniform: m.i32(g + 4), keep_turns: m.i16(g + 0x20) != 0, old_roll: m.i16(g + 0x2c) != 0 }
    }

    /// Classical scaling without uniform scaling: positions are read at +0x3ac.
    pub fn classic_frame(&self) -> bool {
        self.classical && self.uniform != 1
    }
}

/// The current scene's dictionary: `SI_elemDicts` + 0x70 × the first word of the record
/// `SI_procData` points at, as every routine here computes it.
pub(crate) fn scene_dict(m: &Mem) -> u32 {
    let index = m.i32(m[SI_PROC_DATA].addr());
    m[SI_ELEM_DICTS].addr().wrapping_add((index as u32).wrapping_mul(0x70))
}

/// The models' chapter (`SI_cid` + 0x20).
pub(crate) fn models_chapter(m: &Mem) -> i32 {
    m[SI_CID].model
}

/// `SI_edGetElemFromModel(node, &index)`: the node's element; the index goes to a local.
pub(crate) fn elem_from_model(m: &mut Mem, o: &mut dyn Outside, node: Node) -> u32 {
    let sp = m.sp();
    let index = m.local(4);
    let e = o.call(m, &out::SI_ED_GET_ELEM_FROM_MODEL, &[node.addr(), index]).eax;
    m.set_sp(sp);
    e
}

/// `SI_elemdictFindRelatedElem(scene, elem, 1, 1, relation, chapter, &chapter found, &index
/// found)`, the found pair in locals.
pub(crate) fn find_related(m: &mut Mem, o: &mut dyn Outside, elem: u32, relation: i32, chapter: i32) -> u32 {
    let sp = m.sp();
    let (a, b) = (m.local(4), m.local(4));
    let d = scene_dict(m);
    let r = o.call(m, &out::SI_ELEMDICT_FIND_RELATED_ELEM, &[d, elem, 1, 1, relation as u32, chapter as u32, a, b]).eax;
    m.set_sp(sp);
    r
}

/// `SI_elemdictGetCurRelation(scene, elem, 1, 1)`: the relation entry the last search
/// stopped at.
pub(crate) fn cur_relation(m: &mut Mem, o: &mut dyn Outside, elem: u32) -> u32 {
    let d = scene_dict(m);
    o.call(m, &out::SI_ELEMDICT_GET_CUR_RELATION, &[d, elem, 1, 1]).eax
}

/// `SI_modelUpdateSonState(father, node, sons)`.
pub(crate) fn update_son_state(m: &mut Mem, o: &mut dyn Outside, father: Node, node: Node, sons: i16) {
    out::si_model_update_son_state(o, m, father, node, sons);
}

/// `SI_modelSetRotation(node, &angles, mode, geometry)`: the angles in a local.
pub(crate) fn set_rotation(m: &mut Mem, o: &mut dyn Outside, node: Node, angles: Vector, mode: i32, geometry: i16) {
    let sp = m.sp();
    let a = m.local(12);
    m.set_f32(a, angles.x);
    m.set_f32(a + 4, angles.y);
    m.set_f32(a + 8, angles.z);
    o.call(m, &out::SI_MODEL_SET_ROTATION, &[node.addr(), a, mode as u32, geometry as i32 as u32]);
    m.set_sp(sp);
}

pub(crate) fn v(a: f32, b: f32, c: f32) -> Vector {
    Vector { x: a, y: b, z: c }
}

pub(crate) const ZERO: Vector = Vector { x: 0.0, y: 0.0, z: 0.0 };

pub(crate) fn mv(a: Vector) -> si_math::types::Vector {
    si_math::types::Vector::new(a.x, a.y, a.z)
}

pub(crate) fn tv(a: si_math::types::Vector) -> Vector {
    Vector { x: a.x, y: a.y, z: a.z }
}
