// SPDX-License-Identifier: MIT
//! The set of model nodes every routine of this crate works on (finding 0015, "The
//! hierarchy in Rust").
//!
//! The original links 0x430-byte `__SI_Model` records by pointers: `father` (+0x2bc),
//! `brother` (+0x2c0, the next son of the same father) and `son` (+0x2c4, the first son).
//! Here the records live in an arena, [`Models`], and **a pointer to node `i` is the number
//! `i + 1`** (0 is null), stored in the same fields of the same record
//! ([`si_types::model::Model`], whose offsets are asserted). So a traversal written against
//! the original's links reads the same links here, in the same order, and nothing has to be
//! kept in step by hand. A node that the original frees keeps its slot, marked `disposed`;
//! slots are never reused, as a freed pointer is never valid again.
//!
//! What a record reaches through other pointers (its name, its definitions, the three
//! vectors of its initial transformation) is owned by the [`NodeData`] beside it.

use si_math::types::Vector as MVector;
use si_types::geom::Vector as TVector;
pub use si_types::model::{Model, ModelArena, MBALL, MFACE, MJNT, MMETA, MNCRV, MNILL, MNSRF, MPTCH, MSMSH, MSPLN};
use si_types::Ptr32;

use crate::mesh::Mesh;

/// Index of a node in [`Models`]. The original's pointer to it is `id + 1`.
pub type NodeId = usize;

/// One geometry of a node: what an entry of the record's `defs` array (+0x38) points to,
/// by the node's `kind` (+0x30). Meshes are the in-memory form of this crate; the other
/// kinds are the content a model file holds for them (`si-hrc`), which is what the
/// bounding-box, counting and copying routines need.
#[derive(Clone, Debug, PartialEq)]
pub enum Definition {
    Mesh(Mesh),
    Patch(si_hrc::Patch),
    Face(si_hrc::Face),
    Spline(si_hrc::Spline),
    MetaSystem(si_hrc::MetaSystem),
    Metaball(si_hrc::Metaball),
    NurbsCurve(si_hrc::NurbsCurve),
    NurbsSurface(si_hrc::NurbsSurface),
}

/// What the record's `definition` pointer (+0x48) designates. The original compares it with
/// entries of `defs` to tell a node that works on one of its stored definitions from one
/// that works on a separate copy (`SI_modelCheckCreateDefinition` makes that copy).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Current {
    /// A null pointer.
    #[default]
    None,
    /// The same object as `defs[i]`.
    Def(usize),
    /// An object of its own.
    Own(Box<Definition>),
}

/// What a node owns through pointers.
#[derive(Clone, Debug, Default)]
pub struct NodeData {
    /// +4 `name`.
    pub name: Vec<u8>,
    /// +0x38 `defs`, `nb_defs` of them (+0x34), and `cur_def` (+0x36) the number of the
    /// one in use. The record's `nb_defs` is kept equal to `defs.len()`.
    pub defs: Vec<Definition>,
    /// +0x48 `definition`.
    pub current: Current,
    /// +0x3c `org_def`: a further definition of the node's kind, when the pointer is not null.
    pub org_def: Option<Box<Definition>>,
    /// +0x328 `init_srt`: scaling, rotation, translation, when the pointer is not null.
    pub init_srt: Option<[TVector; 3]>,
    /// The original has freed the record (`SI_modelDisposeNode`).
    pub disposed: bool,
    /// Further owned data of the transformation routines ([`crate::transform`]).
    pub tx: crate::transform::NodeExt,
    /// Further owned data of the hierarchy routines ([`crate::hierarchy`]).
    pub hx: crate::hierarchy::NodeExt,
    /// What the copy and disposal routines read through the node's other pointers
    /// ([`crate::node_copy`]).
    pub cx: crate::node_copy::NodeExt,
}

/// The arena: records and their owned data, index for index.
#[derive(Default)]
pub struct Models {
    /// The records, with the original's allocation counter (`ModelArena::counter`).
    pub arena: ModelArena,
    pub data: Vec<NodeData>,
    /// The original's global `glbInit_srt`: a scratch list `model.c` fills before a node is
    /// changed and uses up afterwards ([`crate::setters::compensate_init_srt`]). `None` is
    /// the null pointer.
    pub glb_init_srt: Option<Vec<crate::setters::InitSrtEntry>>,
}

impl Models {
    /// The original's pointer to node `id`.
    pub fn ptr(id: NodeId) -> Ptr32<Model> {
        ModelArena::ptr(id)
    }

    /// The node a pointer designates; `None` for null.
    pub fn id(p: Ptr32<Model>) -> Option<NodeId> {
        if p.is_null() { None } else { Some(p.0 as usize - 1) }
    }

    /// A new node (`SI_modelAllocate`, ported in `si-types`): named `name`, or `obj<n>` from
    /// the allocation counter.
    pub fn allocate(&mut self, name: Option<&[u8]>) -> NodeId {
        let id = self.arena.allocate(name);
        self.data.push(NodeData { name: self.arena.names[id].clone(), ..NodeData::default() });
        id
    }

    pub fn m(&self, id: NodeId) -> &Model {
        &self.arena.nodes[id]
    }

    pub fn m_mut(&mut self, id: NodeId) -> &mut Model {
        &mut self.arena.nodes[id]
    }

    pub fn father(&self, id: NodeId) -> Option<NodeId> {
        Self::id(self.arena.nodes[id].father)
    }

    pub fn brother(&self, id: NodeId) -> Option<NodeId> {
        Self::id(self.arena.nodes[id].brother)
    }

    pub fn son(&self, id: NodeId) -> Option<NodeId> {
        Self::id(self.arena.nodes[id].son)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// A vector of a record as the mathematics crate's vector (same three floats).
pub fn mv(v: TVector) -> MVector {
    MVector { x: v.x, y: v.y, z: v.z }
}

/// The other way.
pub fn tv(v: MVector) -> TVector {
    TVector { x: v.x, y: v.y, z: v.z }
}
