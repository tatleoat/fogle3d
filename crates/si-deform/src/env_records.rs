// SPDX-License-Identifier: MIT
//! Records of the three envelope source files (`envelop.c`, `envelopset.c`, `envtools.c`)
//! that `si-types` does not lay out. Each field's evidence is in finding 0029: read from the
//! listings of the routines named there, and for the routines that are checked, measured.
//!
//! How the records hang together: a node of a skeleton (the chain's root, a joint, the
//! effector, or any node used as a "bone") carries an [`EnvInfo`] at `Model` +0x324. Its
//! `lists` is a chain of [`EnvList`], one per skin (envelope) node the skeleton deforms;
//! each list is the chain of [`EnvVert`] of that skin's points that are assigned to this
//! bone, each with its weight. So the question "which joints weigh on a vertex" is answered
//! by walking every bone's list for the skin and collecting the entries with the vertex's
//! number.

use si_mem::{record, Ptr32};
use si_types::model::Model;

record! {
    /// One of the two deformation curves of a joint (`SI_EnvDefCrvPrm`, 0x18 bytes; passed
    /// by value to `yDef`): how far the skin bulges or creases around a bending joint.
    pub struct DefCrv = "SI_EnvDefCrvPrm", size 0x18 {
        0x00 kind: i32, "0: none (with no table: no deformation), 1: the exponential profile, 2: the cosine profile";
        0x04 table: Ptr32<f32>, "null, or 100 pairs (position, value) from SI_malloc(0x320): a drawn profile that replaces the formula";
        0x08 lo: f32, "the profile acts on points whose position along the bone is at least this";
        0x0c hi: f32, "and at most this";
        0x10 amplitude: f32, "nothing happens unless above 0.0001";
        0x14 scale: f32, "a factor on the result";
    }
}

record! {
    /// A bone's envelope record (`__SI_EnvSkelJnt`, 0xa0 bytes from `SI_malloc`), at `Model`
    /// +0x324 of every node of an envelope's skeleton.
    pub struct EnvInfo = "__SI_EnvSkelJnt", size 0xa0 {
        0x00 level: i32, "depth below the skeleton's root: 0 for the root, -1 at creation and for a node taken out";
        0x04 lists: Ptr32<EnvList>, "one list per skin this bone moves points of";
        0x08 matrix: [[f32; 4]; 4], "the inverse of the bone's matrix in the skeleton's rest position (envUpdateJointState); identity at creation";
        0x48 v48: [f32; 3], "0 at creation";
        0x54 saved_rot: [f32; 3], "0 at creation; envAutoSet parks the rest rotation here";
        0x60 v60: [f32; 3], "0 at creation";
        0x6c joint_area: f32, "how far along the bones the blend around the joint reaches; 0 at creation";
        0x70 inner: DefCrv, "the profile on the inside of the bend; 0, null, 0, 1, 0, 1 at creation";
        0x88 outer: DefCrv, "the profile on the outside of the bend; the same at creation";
    }
}

record! {
    /// The points of one skin that one bone moves (`envlist`, 0x10 bytes from `SI_malloc`).
    pub struct EnvList = "envlist", size 0x10 {
        0x00 owner: Ptr32<Model>, "the skin's node (SI_envelopListAllocate's argument)";
        0x04 verts: Ptr32<EnvVert>, "the points";
        0x08 prev: Ptr32<EnvList>, "the list before this one in the bone's chain";
        0x0c next: Ptr32<EnvList>, "the list after it";
    }
}

record! {
    /// One point of a skin assigned to a bone (`envvert`, 0x14 bytes from `SI_malloc`).
    pub struct EnvVert = "envvert", size 0x14 {
        0x00 index: i32, "the point's number in the skin's geometry";
        0x04 ratio: f32, "where the point lies along the bone, in bone lengths; 0 at creation";
        0x08 dist: f32, "0 at creation";
        0x0c weight: f32, "the bone's share of the point, in percent; 100 at creation";
        0x10 next: Ptr32<EnvVert>, "the next point of the list";
    }
}

record! {
    /// What the skeleton does to one point of a skin, gathered instead of applied while the
    /// user moves points by hand (`editvert`'s element, 0x4c bytes from `SI_malloc`), in a
    /// chain from the file's static `editvert`: the weighted sum of the matrices of the
    /// bones that move the point.
    pub struct EditVert = "editvert element", size 0x4c {
        0x00 index: i32, "the point's number; 0 at creation";
        0x04 weight: f32, "the sum of the weights of the bones met so far, in percent; 0 at creation (written as a word)";
        0x08 matrix: [[f32; 4]; 4], "the sum of the bones' matrices, each times its weight / 100; identity at creation";
        0x48 next: Ptr32<EditVert>, "the next element";
    }
}

record! {
    /// What `Model` +0x328 points to: the node's rest transformation, three vectors.
    pub struct InitSrt = "initial SRT", size 0x24 {
        0x00 scale: [f32; 3], "rest scaling";
        0x0c rot: [f32; 3], "rest rotation, three angles";
        0x18 trans: [f32; 3], "rest translation";
    }
}

/// Values of `Model::envelop` (+0x31c) as the three files use them. The names are ours.
pub mod word {
    /// A chain's first joint whose chain root carries the skeleton's lists.
    pub const CHAIN_FIRST: i32 = 4;
    /// A skin deformed by the chain it is a son of ("local" envelope).
    pub const LOCAL: i32 = 5;
    /// A skin deformed by a skeleton that is one of its sons, or of its fathers' sons
    /// ("global" envelope).
    pub const GLOBAL: i32 = 6;
    /// A node carried rigidly by a bone, as a global envelope is.
    pub const GLOBAL_RIGID: i32 = 7;
    /// A local envelope made automatically: its chain's rest rotations are parked.
    pub const LOCAL_AUTO: i32 = 8;
    /// A node of a skeleton that is not its root.
    pub const PART: i32 = 9;
    /// A skeleton's root that is not a chain.
    pub const ROOT: i32 = 10;
    /// The root of the skeleton of a local envelope.
    pub const LOCAL_ROOT: i32 = 11;
}
