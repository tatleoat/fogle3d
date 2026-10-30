// SPDX-License-Identifier: MIT
//! The inverse-kinematics record of a skeleton node, `SI_InvKin` (finding 0022, "The
//! record"), and the store that stands for the original's heap.
//!
//! Every node of a skeleton chain (root, joints, effector) may carry one record through
//! the pointer `inv_kin` (+0x318) of its `__SI_Model`. `si-types` keeps that pointer as an
//! opaque `IkPtr`; this crate gives it a layout. The record is 0x74 bytes
//! (`SI_invKinAllocate` asks `SI_malloc` for 0x74 and `SI_invKinCopy` copies up to +0x70).
//! Which fields mean something depends on the node:
//!
//! | Node | Fields in use |
//! |---|---|
//! | chain root (null, algorithm 1 or 2) | `kind` (the chain type 10, 20, 30, 40), `nb_joints`, `jacobian`, `inverse` |
//! | joint (kind 5) | `kind` (0, or 1: the effector below keeps its own orientation), `drot`, `prot`, `use_pref`, `saved_rot`; on the first joint of a solved stretch also `cns_type`, `cns_pos`, `plane_roll`, `plane_up` |
//! | effector (null, algorithm 3 or 4) | `goal`, `state` |
//!
//! In Rust the records live in [`IkStore`] and **a pointer to record `i` is the number
//! `i + 1`** in the model's `inv_kin` field, as `si-model` does for nodes. The two general
//! matrices a root owns are kept beside the records and named the same way in the fields
//! `jacobian` and `inverse`.

use core::mem::{offset_of, size_of};

use si_math::matrix_gen_dbl::GenMatrixDbl;
use si_model::{Models, NodeId};
use si_types::geom::Vector;
use si_types::Ptr32;

/// The pointer `si-types` gives a model's `inv_kin` field (+0x318): here the number of a
/// record of an [`IkStore`] plus one.
pub type IkPtr = Ptr32<si_types::model::InvKin>;

/// `SI_InvKin`, 0x74 bytes. Evidence per field is in finding 0022; every offset is asserted
/// below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvKin {
    /// +0x00. On a chain root the chain type (the `ChainType` enumeration: 10 one joint in a
    /// plane, 20 two joints in a plane, 30 more joints in a plane, 40 spatial). On a joint 0
    /// or 1; 1 on the last joint makes the effector below keep `grot` = `mrot`.
    pub kind: i32,
    /// +0x04. Number of joints of the chain (root).
    pub nb_joints: i32,
    /// +0x08. The goal of an effector, in global terms.
    pub goal: Vector,
    /// +0x14. The change of the joint's angles in the last step of the iteration (a planar
    /// chain uses z only).
    pub drot: Vector,
    /// +0x20. The joint's preferred angles: x is the roll about the bone, z the bend the
    /// solver starts from and whose sign chooses the side a two-joint chain bends to.
    pub prot: Vector,
    /// +0x2c. The Jacobian of the chain (root): 2 × joints, or 3 × 3·joints for a spatial one.
    pub jacobian: Ptr32<GenMatrixDbl>,
    /// +0x30. Its pseudo-inverse: joints × 2, or 3·joints × 3.
    pub inverse: Ptr32<GenMatrixDbl>,
    /// +0x34. 0 from `SI_invKinRootAllocate`, 1 after `SI_invKinReadBinaryChunk`; no reader
    /// was met in the files of this crate.
    pub loaded: i16,
    pub pad_36: [u8; 2],
    /// +0x38. State of an effector's goal: 0 none, 1 set, 2 solved.
    pub state: i32,
    /// +0x3c. 1 at allocation. `SI_jntRotlimSetFlag` sets it before every solve and clears
    /// it on a joint with an active rotation limit; the iteration starts from the preferred
    /// angles only when the first joint's is set.
    pub use_pref: i16,
    pub pad_3e: [u8; 2],
    /// +0x40. The joint's angles as they were when the solver was entered.
    pub saved_rot: Vector,
    /// +0x4c. Which constraint fixes the resolution plane (first joint): 0 none, 1 a point
    /// the plane's normal is taken from, 2 a point the plane's second axis points at.
    pub cns_type: i32,
    /// +0x50. That point, in global terms.
    pub cns_pos: Vector,
    /// +0x5c. The plane vector below after the roll has been applied to it.
    pub plane_roll: Vector,
    /// +0x68. The second vector the resolution plane was built from, in global terms.
    pub plane_up: Vector,
}

const _: () = {
    assert!(size_of::<InvKin>() == 0x74);
    assert!(offset_of!(InvKin, kind) == 0x00);
    assert!(offset_of!(InvKin, nb_joints) == 0x04);
    assert!(offset_of!(InvKin, goal) == 0x08);
    assert!(offset_of!(InvKin, drot) == 0x14);
    assert!(offset_of!(InvKin, prot) == 0x20);
    assert!(offset_of!(InvKin, jacobian) == 0x2c);
    assert!(offset_of!(InvKin, inverse) == 0x30);
    assert!(offset_of!(InvKin, loaded) == 0x34);
    assert!(offset_of!(InvKin, state) == 0x38);
    assert!(offset_of!(InvKin, use_pref) == 0x3c);
    assert!(offset_of!(InvKin, saved_rot) == 0x40);
    assert!(offset_of!(InvKin, cns_type) == 0x4c);
    assert!(offset_of!(InvKin, cns_pos) == 0x50);
    assert!(offset_of!(InvKin, plane_roll) == 0x5c);
    assert!(offset_of!(InvKin, plane_up) == 0x68);
};

/// The chain types of a root's `kind` (the `ChainType` enumeration of the attribute
/// dictionary, finding 0006): the names are the vendor's.
pub const CHAIN_SGL_2D: i32 = 10;
pub const CHAIN_DBL_2D: i32 = 20;
pub const CHAIN_CPX_2D: i32 = 30;
pub const CHAIN_CPX_3D: i32 = 40;

/// Values of `state`.
pub const GOAL_NONE: i32 = 0;
pub const GOAL_SET: i32 = 1;
pub const GOAL_SOLVED: i32 = 2;

const ZERO: Vector = Vector { x: 0.0, y: 0.0, z: 0.0 };

impl InvKin {
    /// A record as [soft 0x084ad8f0 SI_invKinAllocate] leaves it: everything zero except
    /// `use_pref` = 1. (The original clears every field but the two pad words, which keep
    /// what the heap held; here they are zero.)
    pub fn allocate() -> InvKin {
        InvKin {
            kind: 0,
            nb_joints: 0,
            goal: ZERO,
            drot: ZERO,
            prot: ZERO,
            jacobian: Ptr32::NULL,
            inverse: Ptr32::NULL,
            loaded: 0,
            pad_36: [0; 2],
            state: 0,
            use_pref: 1,
            pad_3e: [0; 2],
            saved_rot: ZERO,
            cns_type: 0,
            cns_pos: ZERO,
            plane_roll: ZERO,
            plane_up: ZERO,
        }
    }
}

/// The records and matrices the nodes' `inv_kin` pointers lead to.
#[derive(Clone, Debug, Default)]
pub struct IkStore {
    /// Record `i` is the pointer value `i + 1`. A freed record keeps its slot (`None`).
    pub recs: Vec<Option<InvKin>>,
    /// Matrix `i` is the pointer value `i + 1` in a record's `jacobian` or `inverse`.
    pub mats: Vec<Option<GenMatrixDbl>>,
}

impl IkStore {
    /// Adds a record and returns the pointer to it.
    pub fn add(&mut self, rec: InvKin) -> IkPtr {
        self.recs.push(Some(rec));
        Ptr32::new(self.recs.len() as u32)
    }

    /// Adds a matrix and returns the pointer to it; null for `None`.
    pub fn add_matrix(&mut self, m: Option<GenMatrixDbl>) -> Ptr32<GenMatrixDbl> {
        match m {
            None => Ptr32::NULL,
            Some(m) => {
                self.mats.push(Some(m));
                Ptr32::new(self.mats.len() as u32)
            }
        }
    }

    pub fn get(&self, p: IkPtr) -> Option<&InvKin> {
        self.recs.get((p.0 as usize).wrapping_sub(1)).and_then(|r| r.as_ref())
    }

    pub fn get_mut(&mut self, p: IkPtr) -> Option<&mut InvKin> {
        self.recs.get_mut((p.0 as usize).wrapping_sub(1)).and_then(|r| r.as_mut())
    }

    pub fn matrix(&self, p: Ptr32<GenMatrixDbl>) -> Option<&GenMatrixDbl> {
        self.mats.get((p.0 as usize).wrapping_sub(1)).and_then(|m| m.as_ref())
    }

    pub fn matrix_mut(&mut self, p: Ptr32<GenMatrixDbl>) -> Option<&mut GenMatrixDbl> {
        self.mats.get_mut((p.0 as usize).wrapping_sub(1)).and_then(|m| m.as_mut())
    }

    /// The record of a node, if its `inv_kin` pointer is not null.
    pub fn of(&self, models: &Models, node: NodeId) -> Option<&InvKin> {
        self.get(models.m(node).inv_kin)
    }

    pub fn of_mut(&mut self, models: &Models, node: NodeId) -> Option<&mut InvKin> {
        self.get_mut(models.m(node).inv_kin)
    }

    /// The record of a node the original reads through `inv_kin` without testing the
    /// pointer. Where the pointer is null the original faults; here a record is made on the
    /// spot (as [`InvKin::allocate`] leaves it) and the use is counted in
    /// [`crate::fault`], so that a caller and the emulation check can tell.
    pub fn force(&mut self, models: &mut Models, node: NodeId) -> &mut InvKin {
        if self.get(models.m(node).inv_kin).is_none() {
            crate::fault::note();
            let p = self.add(InvKin::allocate());
            models.m_mut(node).inv_kin = p;
            push_tx(models, self, node);
        }
        self.get_mut(models.m(node).inv_kin).unwrap()
    }
}

/// Copies what `si-model` mirrors of a node's record (the first word, the goal, the state)
/// into the node's `NodeData`, where that crate's update and setters read it. Call it after
/// changing one of the three in a record.
pub fn push_tx(models: &mut Models, ik: &IkStore, node: NodeId) {
    let rec = ik.get(models.m(node).inv_kin).copied();
    let tx = &mut models.data[node].tx;
    match rec {
        Some(r) => {
            tx.inv_kin_kind = Some(r.kind);
            tx.inv_kin_goal = [r.goal.x, r.goal.y, r.goal.z];
            tx.inv_kin_state = r.state;
        }
        None => {
            tx.inv_kin_kind = None;
            tx.inv_kin_goal = [0.0; 3];
            tx.inv_kin_state = 0;
        }
    }
}

/// The other way: what `si-model`'s setters wrote into the mirror (an effector's translation
/// parked as the goal, the state set to 1) is brought into the record.
///
/// The store is what the routines of this crate read: a caller that lets `si-model` move an
/// effector calls this afterwards. It does nothing for a node whose mirror was never
/// filled ([`push_tx`]).
pub fn pull_tx(models: &Models, ik: &mut IkStore, node: NodeId) {
    let tx = &models.data[node].tx;
    if tx.inv_kin_kind.is_none() {
        return;
    }
    let (goal, state) = (tx.inv_kin_goal, tx.inv_kin_state);
    if let Some(r) = ik.get_mut(models.m(node).inv_kin) {
        r.goal = Vector { x: goal[0], y: goal[1], z: goal[2] };
        r.state = state;
    }
}
