// SPDX-License-Identifier: MIT
//! Field offsets of the original's structures, as the ported routines use them.
//!
//! One module per structure. An offset listed here was read from code of the Linux `soft`
//! binary: either in this package's source files (the comment names the function) or in
//! findings 0005, 0009 and 0010, which the comment then cites. Fields first established by
//! this package are marked **new** and are listed in finding 0017 for `si-types`.

/// `SI_ElemDict`, 0x70 bytes (finding 0010).
pub mod dict {
    pub const SIZE: u32 = 0x70;
    pub const NB_LISTS: u32 = 0x00;
    pub const LISTS: u32 = 0x04;
    /// The update list of the whole scene (`SI_UpdList`, 12 bytes).
    pub const UPD_LIST: u32 = 0x0c;
    /// The update list of one element or selection, built by `SI_elemdictTopoSortElement`.
    pub const TOPO_LIST: u32 = 0x4c;
    pub const HIER_DIRTY: u32 = 0x6e;
}

/// `SI_ElemList`, 0x40 bytes: one chapter (finding 0010).
pub mod list {
    pub const SIZE: u32 = 0x40;
    pub const NAME: u32 = 0x00;
    pub const NB_ELEMS: u32 = 0x04;
    pub const ELEMS: u32 = 0x08;
    /// 1 for a chapter with files; the topological sort only enters chapters of kind 1.
    pub const KIND: u32 = 0x18;
}

/// `__SI_Elem`, 0x84 bytes (finding 0010).
pub mod elem {
    pub const SIZE: u32 = 0x84;
    pub const NODE: u32 = 0x00;
    /// The object: model, camera, light, material, texture, fcurve … by chapter.
    pub const DATA: u32 = 0x04;
    /// The graf node of an object on screen.
    pub const AUX_DATA: u32 = 0x08;
    pub const WIRE_TYPE: u32 = 0x20;
    /// A mark of the topological sort (`topoSort` sets 1; `SI_elemdictSetSelection`).
    pub const SELECTED: u32 = 0x42;
    /// 0 in use, 2 deleted.
    pub const STATUS: u32 = 0x48;
    pub const ROOT_ID: u32 = 0x50;
    pub const HIER_LEVEL: u32 = 0x54;
    /// `SI_Relations` of which the element is the source: count at +0, lists at +4.
    pub const FORWARD: u32 = 0x60;
    /// `SI_Relations` of which the element is the destination.
    pub const BACKWARD: u32 = 0x68;
}

/// `SI_RelList`, 0x18 bytes: one of the six relation lists of a direction (finding 0010).
pub mod rellist {
    pub const SIZE: u32 = 0x18;
    pub const NB: u32 = 0x00;
    pub const CAPACITY: u32 = 0x04;
    /// The cursor; the topological sort keeps "relations before this are done" here.
    /// **new**: read from `topoSort` and `relTopoSortInitChapter`.
    pub const CUR: u32 = 0x08;
    pub const RELS: u32 = 0x0c;
    /// **new**: the topological sort's state of the list: 0 not visited, 1 being visited
    /// (meeting it again is a cycle), 2 done (`topoSort`, `relTopoSortInitChapter`).
    pub const TOPO_STATE: u32 = 0x10;
    /// **new**: non-zero when the list takes part in the sort (`elemdictTopoSortChapter`
    /// skips a list whose word is 0); 1 whenever the list is reset (finding 0010).
    pub const FLAG: u32 = 0x14;
}

/// `SI_ARelation`, 0x20 bytes (finding 0010).
pub mod arel {
    pub const SIZE: u32 = 0x20;
    pub const RELID: u32 = 0x00;
    pub const ELEMID: u32 = 0x04;
    pub const ACTIVE_ELEM: u32 = 0x08;
    pub const SERIAL: u32 = 0x0c;
    pub const CHAPID: u32 = 0x10;
    pub const GROUP: u32 = 0x12;
    /// **new**: the relation's activation flag (the `NOTACT` word of a scene description
    /// clears it): `si_updlistAddArgument` copies it to the argument's `active`, and
    /// `si_updlistAddNode` to the node's when the relation carries data.
    pub const ACTIVE: u32 = 0x14;
    pub const DATA: u32 = 0x18;
    pub const RANK: u32 = 0x1c;
}

/// `SI_UpdList`, 12 bytes (finding 0010).
pub mod updlist {
    pub const NB: u32 = 0x00;
    pub const NODES: u32 = 0x04;
    pub const DIRTY: u32 = 0x08;
}

/// `SI_UpdNode`, 0x2c bytes: one step of an update. **new**: every field, read from
/// `si_updlistAddNode` (0x08141488), `SI_updlistDispose` and the routines of
/// `motionUpdate.c` that take a node.
pub mod updnode {
    pub const SIZE: u32 = 0x2c;
    /// 16 bits: chapter of the element the step updates. −1 in the extra record that ends
    /// the list when the fast hierarchy update is on (`SI_elemdictTopoSort`).
    pub const CHAPID: u32 = 0x00;
    /// 16 bits: which of the element's six relation lists the step belongs to, which is
    /// also the kind of update: 0 parameters, 1 transformation, 2 geometry, 3 rendering,
    /// 5 "other" (`si_updNodeUpdate`).
    pub const GROUP: u32 = 0x02;
    pub const ELEMID: u32 = 0x04;
    /// The element's object (`__SI_Elem` + 4 at the time the node was made).
    pub const DATA: u32 = 0x08;
    /// Two words `si_updlistAddNode` does not set and nothing in these files reads.
    pub const UNSET_0C: u32 = 0x0c;
    pub const UNSET_10: u32 = 0x10;
    /// The relation id of the step.
    pub const RELID: u32 = 0x14;
    /// 16 bits: number of arguments.
    pub const NB_ARGS: u32 = 0x18;
    /// Array of `SI_UpdElem`, owned by the node.
    pub const ARGS: u32 = 0x1c;
    /// 16 bits: 1, or the relation's activation flag when the relation carries data.
    pub const ACTIVE: u32 = 0x20;
    /// The relation's data pointer (`SI_ARelation` + 0x18), null when it has none.
    pub const REL_DATA: u32 = 0x24;
    /// A block owned by the node (freed by `SI_updlistDispose`), null at creation.
    pub const OWNED_28: u32 = 0x28;
}

/// `SI_UpdElem`, 0x14 bytes: one argument of an update node. **new**: every field, read
/// from `si_updlistAddArgument` (0x081413e0).
pub mod updelem {
    pub const SIZE: u32 = 0x14;
    /// 16 bits: chapter of the argument element.
    pub const CHAPID: u32 = 0x00;
    /// 16 bits: the list on the argument element that holds the matching relation entry.
    pub const GROUP: u32 = 0x02;
    pub const ELEMID: u32 = 0x04;
    /// The argument element's object: for an element of the ANIMATION chapter, the `fcurve`.
    pub const DATA: u32 = 0x08;
    /// 16 bits: the relation's activation flag (`SI_ARelation` + 0x14).
    pub const ACTIVE: u32 = 0x0c;
    /// The relation's activation curve (`SI_edGetFcvFromRelation`), or null.
    pub const ACT_FCV: u32 = 0x10;
}

/// `fcurve`, 100 bytes (finding 0009).
pub mod fcurve {
    pub const SIZE: u32 = 100;
    pub const KIND: u32 = 0x00;
    pub const OWNER: u32 = 0x04;
    pub const KEYS: u32 = 0x08;
    pub const NB_KEYS: u32 = 0x0c;
    pub const LAST_TIME: u32 = 0x10;
    pub const LAST_VALUE: u32 = 0x14;
    pub const PRE_EXTRAP: u32 = 0x18;
    pub const POST_EXTRAP: u32 = 0x1a;
    pub const FLAGS: u32 = 0x1c;
    pub const INTERP: u32 = 0x1e;
    pub const ACTIVE: u32 = 0x20;
    pub const RANGE_START: u32 = 0x24;
    pub const RANGE_END: u32 = 0x28;
    pub const ACTIVE_COPY: u32 = 0x2c;
    pub const NAME: u32 = 0x2e;
    pub const CACHE: u32 = 0x50;
    pub const AUX: u32 = 0x54;
    pub const MIN_VALUE: u32 = 0x58;
    pub const MAX_VALUE: u32 = 0x5c;
    pub const FIELD_60: u32 = 0x60;
    pub const DIRTY: u32 = 0x62;
}

/// `SI_Fcvkey`, 36 bytes (finding 0009).
pub mod key {
    pub const SIZE: u32 = 0x24;
    pub const TIME: u32 = 0x00;
    pub const VALUE: u32 = 0x04;
    pub const RIGHT_SLOPE: u32 = 0x08;
    pub const SLOPE_TYPE: u32 = 0x0c;
    pub const INTERP: u32 = 0x0e;
    pub const CURVE: u32 = 0x10;
    pub const TAGGED: u32 = 0x14;
    pub const LEFT_SLOPE: u32 = 0x18;
    pub const FLAGS: u32 = 0x1c;
    pub const SPLINE_INDEX: u32 = 0x1e;
    pub const FIELD_20: u32 = 0x20;
}

/// `__SI_GrafNode`, 0x24 bytes (finding 0010).
pub mod grafnode {
    pub const KIND: u32 = 0x00;
    pub const DATA: u32 = 0x04;
    /// Selection state: 0 none, 1 node, 2 inherited, 3 branch.
    pub const SELECTION: u32 = 0x08;
    /// Display type: 0 hidden, 2 visible (finding 0017).
    pub const DISPLAY: u32 = 0x0c;
    /// 16 bits. **new**: 1 when the object has a key at the current time (set by
    /// `SI_sceneKeySetColor`; the wire colour of an object with a key).
    pub const HAS_KEY: u32 = 0x14;
    pub const NEXT: u32 = 0x1c;
}

/// `__SI_Model`, 0x430 bytes (finding 0010 unless marked).
pub mod model {
    pub const NAME: u32 = 0x04;
    /// The low byte of the 16-bit update flags: bit 1 (2) scaling changed, bit 2 (4)
    /// rotation changed, bit 3 (8) translation changed. **new**: the bit meanings, from
    /// `animScalingUpdate`, `animRotationUpdate`, `animExplicitPosUpdate`.
    pub const UPDATE_FLAGS: u32 = 0x0a;
    /// 0 null, 1 patch, 2 face, 4 mesh, 5 joint, 6 spline, 7 metaball system, 8 metaball,
    /// 9 NURBS curve, 10 NURBS surface.
    pub const KIND: u32 = 0x30;
    pub const NB_DEFS: u32 = 0x34;
    pub const CUR_DEF: u32 = 0x36;
    pub const DEFS: u32 = 0x38;
    pub const ORG_DEF: u32 = 0x3c;
    /// The node in whose space the working definition lives (finding 0015: `latdep`); the
    /// node itself unless something deforms the geometry.
    pub const LATDEP: u32 = 0x44;
    pub const DEFINITION: u32 = 0x48;
    /// Lattices of the node (array of pointers; the count is the 16-bit word before it) and
    /// of the branch (finding 0017, section 9d).
    pub const NODE_LATTICES: u32 = 0x50;
    pub const BRANCH_LATTICES: u32 = 0x58;
    /// Spline fitting record of the node, of the branch.
    pub const SPL_FITTING: u32 = 0x60;
    pub const SPL_FITTING_BRANCH: u32 = 0x64;
    /// Local scaling x, y, z.
    pub const SCAL: u32 = 0x100;
    /// Local rotation angles x, y, z, radians.
    pub const VROT: u32 = 0x10c;
    pub const MROT: u32 = 0x118;
    /// Local translation x, y, z.
    pub const TRANS: u32 = 0x158;
    /// **new**: two floats that receive the same value as the x and y rotation angles in
    /// `animRotationUpdate` (finding 0010 has them as unknown, "written by doRotSGI").
    pub const VROT_XY_COPY: u32 = 0x164;
    pub const GSCL: u32 = 0x16c;
    pub const GROT: u32 = 0x178;
    /// Global translation x, y, z.
    pub const GTRS: u32 = 0x1b8;
    pub const GMAT: u32 = 0x1c4;
    pub const FATHER: u32 = 0x2bc;
    pub const BROTHER: u32 = 0x2c0;
    pub const SON: u32 = 0x2c4;
    /// The model of the path the node follows (a spline or NURBS curve node), null when it
    /// follows none.
    pub const PATH: u32 = 0x2c8;
    /// Patch fitting record of the node, of the branch.
    pub const PTCH_FITTING: u32 = 0x2d8;
    pub const PTCH_FITTING_BRANCH: u32 = 0x2dc;
    /// 16 bits: how the shapes of the node are interpolated: 0 linear, 1 cardinal, 2 by
    /// weights (`animShapeUpdate`).
    pub const SHAPE_INTERP: u32 = 0x2e0;
    /// The algorithm type (attribute 178): in memory 1 … 4, 2 and 12 (dynamics).
    pub const ALGO_TYPE: u32 = 0x2ec;
    pub const KINE_INFO: u32 = 0x2f0;
    pub const DYNA_MATTER: u32 = 0x2f4;
    pub const DYNA_CHN_VARS: u32 = 0x304;
    pub const INV_KIN: u32 = 0x318;
    pub const ENVELOP: u32 = 0x31c;
    pub const ENV_SKEL_JNT: u32 = 0x324;
    /// 16 bits: > 0 when a custom effect deforms the node (finding 0015).
    pub const CUSTOM_COUNT: u32 = 0x352;
    pub const ELEM_ID: u32 = 0x354;
    /// The global scaling kept by the classical transformation method (finding 0015: `cscl`).
    pub const CSCL: u32 = 0x3a0;
    /// Node matte (`float`).
    pub const MATTE: u32 = 0x3f4;
    /// The second copy of the update flags, set together with [`UPDATE_FLAGS`].
    pub const UPDATE_FLAGS2: u32 = 0x3fa;
    pub const MASTER_FLAG: u32 = 0x402;
    /// Bit 0 caustic transmitter, bit 1 caustic receiver.
    pub const CAUSTIC: u32 = 0x404;
    /// Bit 0 global illumination transmitter, bit 1 receiver.
    pub const GLOBAL_ILLUM: u32 = 0x406;
}

/// Bits of [`model::UPDATE_FLAGS`].
pub mod updflag {
    pub const SCALING: u8 = 2;
    pub const ROTATION: u8 = 4;
    pub const TRANSLATION: u8 = 8;
}

/// Bits of the 16-bit flag word of a key (`SI_Fcvkey` + 0x1c), as the path routines use
/// them (finding 0009 has the key types; **new**: the path-key meaning).
pub mod keyflag {
    /// The low four bits: the key's type. 2 is a **path key**: the key stands for a point
    /// of the path's curve, whose number is the key's word at +0x1e (−1 for none).
    pub const TYPE_MASK: u16 = 0x0f;
    pub const PATH_KEY: u16 = 2;
    /// The bits a path routine keeps when it makes a key a path key (0x10, 0x20, 0x40).
    pub const KEPT: u16 = 0x70;
    /// The whole word a path key is given to mark it for removal
    /// (`SI_deleteFcurvePathKey`).
    pub const DOOMED: u16 = 0x4d2;
}

/// `SI_Spline`, 0x74 bytes (finding 0010 and `si-types`), as `motionPath.c` uses it.
pub mod spline {
    /// 16 bits: the number of phantom points in front (`SI_pathAddKeyPct` inserts at this
    /// index to put a point before the first one, and at `NB_POINTS − PHANTOM` to append).
    pub const PHANTOM: u32 = 0x06;
    /// The spline type; 2 and 3 are the types whose first and last points are phantoms
    /// (`motionPath.c` subtracts 2 from the point count for them); 1 is Bézier.
    pub const KIND: u32 = 0x0c;
    /// 16 bits, unsigned: number of points.
    pub const NB_POINTS: u32 = 0x10;
    /// Array of points, 0x30 bytes each; the position is at +0x0c of a point.
    pub const POINTS: u32 = 0x14;
    pub const POINT_SIZE: u32 = 0x30;
    pub const POINT_POS: u32 = 0x0c;
    pub const TENSION: u32 = 0x1c;
    /// 16 bits, unsigned: the number of length samples per segment
    /// (`SI_splineUpdateLength(spline, 100, 0)` fills the table).
    pub const LENGTH_STEP: u32 = 0x68;
    /// The table of cumulated lengths, floats: entry `point × step` is the length from the
    /// start to that point.
    pub const LENGTHS: u32 = 0x6c;
    /// 16 bits. **new**: 1 when the percentage curve that drives a model along this spline
    /// has path keys (keys tied to points of the spline); the file keyword `keyframed`.
    pub const KEYFRAMED: u32 = 0x70;
}

/// `__SI_NurbsCrv`, 100 bytes (finding 0010 and `si-types`), as `motionPath.c` uses it.
pub mod nurbs {
    /// Number of control points.
    pub const NB_POINTS: u32 = 0x04;
    /// The parameter range, two doubles: start and end.
    pub const RANGE_START: u32 = 0x10;
    /// The curve's type; 3 and 4 have their knots reparametrised after a point moved
    /// (**new**, from `cnsFPntObjSetPtsLocation`). The first word of the record is compared
    /// with the point count on the same occasion.
    pub const KIND: u32 = 0x24;
    /// `SI_NurbsSrf`: the same type word, and the array of control points (0x24 bytes
    /// each, the weight at +0x18).
    pub const SRF_KIND: u32 = 0x4c;
    pub const SRF_POINTS: u32 = 0x54;
    pub const RANGE_END: u32 = 0x18;
    /// Array of control points, 0x24 bytes each: x, y, z, w as doubles (x, y, z are stored
    /// multiplied by the weight), and a word.
    pub const POINTS: u32 = 0x2c;
    pub const POINT_SIZE: u32 = 0x24;
    pub const POINT_W: u32 = 0x18;
    /// 16 bits: the same "has path keys" flag as [`super::spline::KEYFRAMED`].
    pub const KEYFRAMED: u32 = 0x4c;
    /// **new**: number of entries of the length table (`SI_nurbsCurveUpdateLength`), and at
    /// +0x58 the table itself, doubles; the last entry is the curve's length.
    pub const NB_LENGTHS: u32 = 0x50;
    pub const LENGTHS: u32 = 0x58;
}

/// One point of an `SI_Spline`, 0x30 bytes, as the constraint and trajectory routines use
/// it (**new**: the lock word).
pub mod splpoint {
    pub const SIZE: u32 = 0x30;
    /// Three vectors: +0x00, the position at +0x0c, +0x18 (the face update of the
    /// three-point constraint writes all three alike).
    pub const POS: u32 = 0x0c;
    /// 16 bits: the point of a trajectory is locked (`animCheckTrajectoryLock`).
    pub const LOCKED: u32 = 0x2c;
}

/// `kluster` (a cluster of points of a model), as the constraint routines use it (**new**).
pub mod kluster {
    /// Number of members.
    pub const NB: u32 = 0x04;
    /// Array of the members' numbers (32 bits each).
    pub const MEMBERS: u32 = 0x0c;
    /// The cluster's type; 2 is left alone by the cluster constraints.
    pub const KIND: u32 = 0x10;
}

/// `SI_Face` (the definition of a model of kind 2), as `animCns3PtsFaceUpdate` uses it
/// (**new**).
pub mod face {
    /// The outline as an `SI_Spline`, embedded.
    pub const SPLINE: u32 = 0x04;
    /// The outline as a NURBS curve (a pointer).
    pub const NURBS: u32 = 0x80;
    /// 16 bits: non-zero when the outline is the spline, zero when it is the NURBS curve.
    pub const IS_SPLINE: u32 = 0x84;
}

/// Where an object keeps the model of the path it follows (finding 0017).
pub mod pathslot {
    /// `__SI_Model` + 0x2c8.
    pub const MODEL: u32 = 0x2c8;
    /// `SI_Camera` + 0xa0 (position), + 0xa4 (interest).
    pub const CAMERA_POSITION: u32 = 0xa0;
    pub const CAMERA_INTEREST: u32 = 0xa4;
    /// `light` + 0x144 (position), + 0x148 (interest).
    pub const LIGHT_POSITION: u32 = 0x144;
    pub const LIGHT_INTEREST: u32 = 0x148;
}

/// Graf kinds (`SI_GrafType`, the first word of a graf node; finding 0010).
pub mod graf {
    pub const MODEL: i32 = 2;
    pub const MODEL_3: i32 = 3;
    pub const CAMERA: i32 = 4;
    pub const CAMERA_INTEREST: i32 = 5;
    pub const LIGHT: i32 = 6;
    pub const LIGHT_INTEREST: i32 = 7;
    pub const LIGHT_8: i32 = 8;
    pub const WAVE: i32 = 0x0b;
    pub const LIGHT_13: i32 = 0x13;
}
