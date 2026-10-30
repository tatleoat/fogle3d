// SPDX-License-Identifier: MIT
//! Inverse kinematics and skeletons (finding 0022): how a chain of joints is solved so that
//! its effector reaches a goal, the rotation limits of joints, and the skeleton objects
//! around them.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. One module per original source file:
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`rec`] | (record) | `SI_InvKin`, the 0x74-byte record a skeleton node carries, and its store |
//! | [`iktools`] | `iktools.c` | limb lengths, the reachable volume, the closed form for two bones, the chain's own update |
//! | [`jacobian`] | `jacobian.c` | the solver: resolution plane, the one- and two-joint forms, the Jacobian iteration in the plane and in space, pull-out tests, `SI_solvePartialInvKin` |
//! | [`invkin`] | `invkin.c` | the record's life, goals, sub-chains, `SI_modelSolveInvKin` |
//! | [`rotlim`] | `constraint.c` | joint rotation limits as the solver uses them |
//! | [`limits`] | `constraint.c` | the records of the position and rotation limits, their damping and upkeep |
//! | [`skeleton`] | `skeleton.c` | chains as objects: root, leaf, joint count, length, reset |
//! | [`sketransform`] | `sketransform.c` | the tools' moves of skeleton parts: scaling, moving a joint with its bone, turning a joint, moving an effector |
//!
//! The modules of the second pass are written on the shared memory image (`si-mem`,
//! finding 0024): a function takes the image and the arguments the original takes, records
//! are `si-types`' layouts in place, and callees outside the package go through
//! [`outside::Outside`].
//!
//! | Module | Original | Content |
//! |---|---|---|
//! | [`norconstr`] | `constraint.c` | the normal constraint's search: the normal of a mesh at a point |
//! | [`tanconstr`] | `constraint.c` | the tangency constraint's search: the direction of a path at a point |
//! | [`pivot`] | `constraint.c` | the pivot constraint: the lowest point over a plane, the state on the update node, the time of the next step |
//! | [`cnsedit`] | `constraint.c` | setting and relaxing constraints, the update lists |
//! | [`skeimage`] | `skeleton.c` | drawing a joint, a chain from a spline, the tube, the automatic envelope |
//! | [`onimage`] | several | image forms of first-pass functions the modules above call |
//! | [`classic`] | `classic.c` | the classical-scaling mode: chain tests, the solver and the joint mover under it, the compensation angles |
//! | [`cnsdialog`] | `constraint.c` | the limit dialogs' helpers that are not dialog code |
//! | [`callbacks`] | | this package's functions that are called through a pointer, by code address |
//! | [`threepts`] | `constraint.c` | the triangle object of the three-points constraint |
//! | [`globals`] | | the globals the image-form modules use, at their addresses |
//! | [`outside`] | | the one trait for callees outside the package |
//! | [`rodata`] | | the string constants the image-form modules read by address |
//! | [`actor`] | `actor.c` | the Actor module's mode machine: buttons, mode setters, flags, refresh, init, button events |
//! | [`actout`] | | the callees outside `actor.c`, `character.c`, `cha_libkeys.c` and their string constants |
//! | [`character`] | `character.c` | dope-sheet relation and track tables, saving and removing character keys |
//! | [`cha_libkeys`] | `cha_libkeys.c` | newly read elements, hierarchy-animation association, track filters, loading animation and actions from a dictionary |
//!
//! The nodes are `si-model`'s ([`si_model::Models`], typed 0x430-byte records); what a
//! node's `inv_kin` pointer leads to is in an [`rec::IkStore`] beside them.
//!
//! Floating point follows finding 0008: where the original keeps a value in an x87
//! register the port computes on [`si_x87::F80`] and rounds where the original stores.

pub mod actor;
pub mod actout;
pub mod invkin;
pub mod callbacks;
pub mod cha_libkeys;
pub mod character;
pub mod classic;
pub mod cnsdialog;
pub mod cnsedit;
pub mod globals;
pub mod iktools;
pub mod image;
pub mod jacobian;
pub mod limits;
pub mod norconstr;
pub mod onimage;
pub mod outside;
pub mod pivot;
pub mod rec;
pub mod rodata;
pub mod rotlim;
pub mod skeimage;
pub mod skeleton;
pub mod sketransform;
pub mod tanconstr;
pub mod threepts;

#[cfg(test)]
mod tests;

pub use rec::{IkStore, InvKin};

use si_model::{Models, NodeId};
use si_types::geom::Vector;

/// The program-wide settings the routines of this crate read, beside those of `si-model`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Globals {
    /// `si-model`'s: the first word of `SI_glbManip` (classical scaling) and the
    /// continuous-rotation preference among them.
    pub model: si_model::transform::Globals,
    /// `SI_glbManip` + 0x04 (0x08781564, setup keyword `PREF_UNIFORM_SCALING`). Under
    /// classical scaling these routines measure the chain in the positions of the frame
    /// without scaling (`__SI_Model` + 0x3ac) unless this word is 1.
    pub uniform_scaling: i32,
    /// `SI_glbManip` + 0x2c (0x0878158c): 1 when the environment variable
    /// `SI_IK_OLD_JOINT_ROLL` was set at start-up (`SI_manipulationInit`). It selects the
    /// older way of applying a joint's roll in planar chains.
    pub old_joint_roll: bool,
    /// `SI_interactive` (0x0867953e) is not zero.
    pub interactive: bool,
    /// `SI_glbManip` + 0x24 (0x08781584, a 16-bit word, 1 at start-up) is not zero: after a
    /// joint is turned by a tool its preferred angles are made from the angles the tool was
    /// given, not read back from the node (`SI_skeJointRotate`).
    pub pref_follows_input: bool,
}

impl Globals {
    /// Classical scaling without uniform scaling: positions are read at +0x3ac.
    pub fn classic_frame(&self) -> bool {
        self.model.classical_scaling && self.uniform_scaling != 1
    }
}

/// `SI_RelRotlimData`, the data of relation 103 `CnsRotLim`, 0x44 bytes (the keywords are
/// finding 0006's; offsets asserted in [`limits`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotlimData {
    /// +0x00 `MIN_LIMIT`: lower bounds as the file and the dialog have them, degrees.
    pub min_deg: Vector,
    /// +0x0c `MAX_LIMIT`: upper bounds, degrees.
    pub max_deg: Vector,
    /// +0x18 `MIN_ACTIVE`: the lower bound of x, y, z is in force.
    pub min_active: [i16; 3],
    /// +0x1e `MAX_ACTIVE`: the upper bound is in force.
    pub max_active: [i16; 3],
    /// +0x24: lower bounds in radians (what the solver and the constraint compare with).
    pub min: Vector,
    /// +0x30: upper bounds in radians.
    pub max: Vector,
    /// +0x3c, +0x40 `DAMPING`: the band's width in degrees, the strength.
    pub damping: [f32; 2],
}

/// What `SI_elemdictGetCurRelation` answers for a joint's rotation limit: the relation
/// entry's activation flag (`SI_ARelation` + 0x14) and its data (+0x18), if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotlimRelation {
    pub active: i16,
    pub data: Option<RotlimData>,
}

/// What the routines of this crate call in parts of the program that are not ported, or
/// not in this crate. Every method names the original behind it. `si-model`'s hooks are
/// part of it because the solver ends in that crate's node update.
pub trait Host: si_model::Env {
    /// `SI_edGetElemFromModel(node, …)` followed by `SI_elemdictFindRelatedElem(dict, elem,
    /// 1, 1, 0x67, SI_cid, …) != 0`: the node's element has a rotation-limit relation
    /// (`CnsRotLim`, id 103).
    fn find_rotlim(&mut self, _models: &Models, _node: NodeId) -> bool {
        false
    }
    /// `SI_elemdictGetCurRelation(dict, elem, 1, 1)` after [`Host::find_rotlim`]: the entry
    /// that search stopped at; `None` for a null pointer.
    fn cur_rotlim(&mut self, _models: &Models, _node: NodeId) -> Option<RotlimRelation> {
        None
    }
    /// `SI_elemdictFindRelatedElem(dict, elem, 1, 1, 0x66, −1, …)` and
    /// `SI_elemdictGetCurRelation(dict, elem, 1, 1)`: the data of the node's position-limit
    /// relation (`CnsPosLim`, id 102); `None` when the entry's data pointer is null.
    fn cur_poslim(&mut self, _models: &Models, _node: NodeId) -> Option<limits::PoslimData> {
        None
    }
    /// The selection state of the graf node of the node's element: the word at +8 of what
    /// `__SI_Elem` + 8 points to (finding 0015: 1 is "selected as a node").
    fn elem_selection(&mut self, _models: &Models, _node: NodeId) -> i32 {
        0
    }
    /// The word a rotation limit's update leaves for relation 116 of the node's element:
    /// if `SI_elemdictFindRelatedElem(dict, elem, 1, 1, 0x74, SI_cid, …)` finds one and
    /// `SI_elemdictGetCurRelation` is not null, the entry's data block (two bytes, made if
    /// the entry has none) takes `state`. A host without the relation does nothing.
    fn set_rotlim_state(&mut self, _models: &Models, _node: NodeId, _state: i16) {}
    /// `SI_edGetElemFromModel` and `SI_elemdictFindRelatedElem(dict, elem, 1, 1, 0x80,
    /// SI_cid, …)`: the model (`__SI_Elem` + 4) of the object at the other end of the node's
    /// up-vector relation (128), if it has one.
    fn up_vector_object(&mut self, _models: &Models, _node: NodeId) -> Option<NodeId> {
        None
    }
    /// `SI_constrLocation(node, 2, &position)` (the animation's constraint code): where a
    /// node that is not a skeleton part is, for a position limit.
    fn constr_location(&mut self, models: &Models, node: NodeId) -> Vector {
        models.m(node).gtrs
    }
    /// `SI_classicSolvePartialInvKin(root, first, n, goal, first_pass, &stop)`
    /// (`classic.c`, not ported): the solver under classical scaling.
    fn classic_solve_partial_inv_kin(&mut self, _models: &mut Models, _ik: &mut IkStore, _root: NodeId, _first: NodeId, _n: i32, _goal: Vector, _first_pass: i16, _stop: &mut i16) {
    }
    /// `SI_displayModelScal(…, node)`: the scaling shown in the interface after a tool
    /// changed it.
    fn display_model_scal(&mut self, _models: &Models, _node: NodeId) {}
    /// `SI_modelRotationRefMatrixSet(view, node, angles, 0, 0)` (`model.c`, not ported): a
    /// rotation set in one of the reference modes (rotation mode 1, 3 or 4).
    fn model_rotation_ref_matrix_set(&mut self, _models: &mut Models, _view: Option<&si_model::setters::View>, _node: NodeId, _angles: Vector) {}
    /// `si_classicSkeMoveJointBranch(node, vector, turn_father, keep_angles)` (`classic.c`,
    /// not ported): a joint moved under classical scaling.
    fn classic_ske_move_joint_branch(&mut self, _models: &mut Models, _ik: &mut IkStore, _node: NodeId, _by: Vector, _turn_father: i16, _keep_angles: i16) {}
    /// `SI_edIsItemConstrained(dict, node, SI_cid, relation, flag, time) != 0` (`edtools.c`).
    fn ed_is_item_constrained(&mut self, _models: &Models, _node: NodeId, _relation: i32, _flag: i16, _time: f32) -> bool {
        false
    }
    /// `SI_edGetElemFromModel(node, …)` and `SI_elemdictFindSerialRelation(dict, elem,
    /// 0x4e2a, …, −1, 1, first)`: the first (`first`) or the next entry of the expression
    /// relations (20010) of the node's element; `None` for a null pointer.
    fn find_serial_relation(&mut self, _models: &Models, _node: NodeId, _first: bool) -> Option<SerialRelation> {
        None
    }
    /// `SI_getExprFcurveUsed(elem, &count, …, −1)` (`exp_fct.c`): the ids of the curves the
    /// element's expressions drive, `count` of them.
    fn expr_fcurve_used(&mut self, _models: &Models, _node: NodeId) -> Vec<i32> {
        Vec::new()
    }
    /// `SI_elemdictGetElem(dict, SI_cid, activation)` and `SI_activationFcvCheckTime(that
    /// element's curve, time, …, 1) != 0` (`motionUpdate.c`).
    fn activation_on(&mut self, _activation: i32, _time: f32) -> bool {
        true
    }
}

/// What `SI_elemdictFindSerialRelation` answers in the break test of
/// [`invkin::inv_kin_is_break_jnt`]: the entry's activation flag (`SI_ARelation` + 0x14) and
/// the index at +0x08 (−1 for none) of the element whose curve switches the entry on and
/// off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SerialRelation {
    pub active: i16,
    pub activation: i32,
}

impl Host for () {}

/// Places where a port cannot follow the original, counted per thread: a null pointer the
/// original reads through (it would fault), a walk down a chain that ends before the
/// effector. The ports go on in the way their comments say and note it here.
pub mod fault {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<u32> = const { Cell::new(0) };
    }

    pub fn note() {
        COUNT.with(|c| c.set(c.get() + 1));
    }

    pub fn count() -> u32 {
        COUNT.with(|c| c.get())
    }

    pub fn reset() {
        COUNT.with(|c| c.set(0));
    }
}
