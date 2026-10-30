// SPDX-License-Identifier: MIT
//! Offsets into the records the readers fill. Named offsets, not typed records: the
//! layouts are established only as far as the readers and the allocation routines go
//! (finding 0005 has the tables with the keywords the vendor's converter prints).

/// `light`, 0x240 bytes (`SI_lightAllocate`).
pub mod light {
    pub const SIZE: u32 = 0x240;
    /// 30 bytes, not always terminated.
    pub const NAME: u32 = 0x00;
    /// 16-bit: 1 point, 2 spot, 3 infinite, 4 sun.
    pub const TYPE: u32 = 0x1e;
    /// Red, green, blue.
    pub const COLOUR: u32 = 0x24;
    pub const POSITION: u32 = 0x34;
    pub const INTEREST: u32 = 0x58;
    pub const CONE_ANGLE: u32 = 0x70;
    pub const SPREAD_ANGLE: u32 = 0x74;
    pub const FALL_OFF_START: u32 = 0x84;
    pub const FALL_OFF_END: u32 = 0x88;
    /// 16-bit.
    pub const SHADOW_TYPE: u32 = 0x8c;
    pub const SHADOW_FACTOR: u32 = 0x90;
    pub const SHADOW_MAP_RES: u32 = 0x118;
    pub const SHADOW_FILTER_SIZE: u32 = 0x11c;
    pub const SHADOW_FILTER_STEP: u32 = 0x124;
    pub const SHADOW_FADE_START: u32 = 0x128;
    pub const SHADOW_FADE_RATE: u32 = 0x12c;
    /// The next light of the chain the render commands build.
    pub const NEXT: u32 = 0x140;
    /// The model the position follows, and the one the interest follows.
    pub const POSITION_MODEL: u32 = 0x144;
    pub const INTEREST_MODEL: u32 = 0x148;
    pub const SELECTIVE: u32 = 0x15c;
    pub const INCLUDE: u32 = 0x160;
    /// The sun record ([`super::sun`]), null unless the light is a sun.
    pub const SUN: u32 = 0x16c;
    /// The two custom shaders.
    pub const SHADER: u32 = 0x178;
    pub const SHADER2: u32 = 0x17c;
    /// 16-bit.
    pub const DIFFUSE: u32 = 0x180;
    /// 16-bit.
    pub const AREA_LIGHT: u32 = 0x182;
    pub const AREA_TYPE: u32 = 0x184;
    pub const AREA_U_SAMPLE: u32 = 0x188;
    pub const AREA_V_SAMPLE: u32 = 0x18c;
    pub const AREA_GEOMETRY: u32 = 0x190;
    /// 16-bit.
    pub const AREA_LOCAL_COORDS: u32 = 0x1c0;
    pub const AREA_SCALE: u32 = 0x1c4;
    pub const AREA_ROTATION: u32 = 0x1d0;
    /// 16-bit.
    pub const AREA_SELECTED: u32 = 0x1dc;
    /// Sixteen floats by rows.
    pub const AREA_ROTATION_MAT: u32 = 0x1e0;
    /// The first user data node ([`super::user_node`]).
    pub const USER_DATA: u32 = 0x220;
    pub const ENERGY: u32 = 0x224;
    pub const EXPONENT: u32 = 0x230;
    pub const CAUSTIC_PHOTON: u32 = 0x234;
    pub const GLOBILLUM_PHOTON: u32 = 0x238;
    /// 16-bit each.
    pub const CAUSTIC_ENABLE: u32 = 0x23c;
    pub const GLOBILLUM_ENABLE: u32 = 0x23e;
}

/// `SI_Camera`, 0x13c bytes (`SI_newCamera`): what the camera reader stores, and the
/// members `SI_resetCamera` sets besides.
pub mod camera {
    pub const SIZE: u32 = 0x13c;
    /// A text of its own block.
    pub const NAME: u32 = 0x00;
    /// 16-bit: the mode `SI_resetCamera` is given (1 a perspective camera).
    pub const MODE: u32 = 0x0c;
    pub const POSITION: u32 = 0x10;
    pub const INTEREST: u32 = 0x1c;
    /// Yaw, pitch, roll.
    pub const YAW: u32 = 0x28;
    pub const PITCH: u32 = 0x2c;
    pub const ROLL: u32 = 0x30;
    pub const NEAR: u32 = 0x34;
    pub const FAR: u32 = 0x38;
    pub const FIELD_OF_VIEW: u32 = 0x40;
    /// The factor between the projection plane's height and its width.
    pub const ASPECT: u32 = 0x64;
    /// 32-bit: 0 off, 1 automatic, 2 custom.
    pub const DEPTH_OF_FIELD: u32 = 0x6c;
    pub const FOCAL_LENGTH: u32 = 0x70;
    pub const FOCAL_DISTANCE: u32 = 0x74;
    pub const NEAR_FOCUS: u32 = 0x7c;
    pub const FAR_FOCUS: u32 = 0x84;
    pub const OUT_OF_FOCUS: u32 = 0x8c;
    pub const F_STOP: u32 = 0x94;
    pub const MAXIMUM_COC: u32 = 0x98;
    /// 16-bit.
    pub const ENLARGE: u32 = 0x9c;
    /// The model the position follows, and the one the interest follows.
    pub const POSITION_MODEL: u32 = 0xa0;
    pub const INTEREST_MODEL: u32 = 0xa4;
    /// 16-bit: set to 1 when a read starts.
    pub const WORD_A8: u32 = 0xa8;
    /// Cleared when a read starts.
    pub const WORD_AC: u32 = 0xac;
    pub const WORD_B0: u32 = 0xb0;
    /// The projection plane: 16-bit "active", then distance, width, height, offset x, y.
    pub const PLANE_ACTIVE: u32 = 0x108;
    pub const PLANE_DISTANCE: u32 = 0x10c;
    pub const PLANE_WIDTH: u32 = 0x110;
    pub const PLANE_HEIGHT: u32 = 0x114;
    pub const PLANE_OFFSET_X: u32 = 0x118;
    pub const PLANE_OFFSET_Y: u32 = 0x11c;
    /// The first user data node.
    pub const USER_DATA: u32 = 0x138;
}

/// `SI_Lattice`, 0x44 bytes (`SI_latticeBinaryRead`). The last four bytes are not written
/// by the reader.
pub mod lattice {
    pub const SIZE: u32 = 0x44;
    /// 16-bit each: active, and the number of control points along each axis.
    pub const ACTIVE: u32 = 0x00;
    pub const COUNT_X: u32 = 0x02;
    pub const COUNT_Y: u32 = 0x04;
    pub const COUNT_Z: u32 = 0x06;
    /// Three 32-bit interpolation codes.
    pub const INTERPOLATION: u32 = 0x08;
    /// The control points, one vector each, in a block of their own.
    pub const KEYS: u32 = 0x14;
    /// One 16-bit word for each control point, cleared, in a block of their own.
    pub const FLAGS: u32 = 0x18;
    /// Three vectors.
    pub const MINIMUM: u32 = 0x1c;
    pub const MAXIMUM: u32 = 0x28;
    pub const SCALING: u32 = 0x34;
}

/// The header of a database file as the header reader keeps it on its stack, 0x58 bytes.
pub mod file_chunk {
    pub const SIZE: u32 = 0x58;
    pub const MAGIC: u32 = 0x00;
    /// A float.
    pub const VERSION: u32 = 0x04;
    /// 80 bytes.
    pub const COMMENT: u32 = 0x08;
}

/// `fcurve`, 100 bytes (`SI_fcurveAllocate`): a function curve.
pub mod fcurve {
    pub const SIZE: u32 = 100;
    /// The curve's type: which attribute it drives.
    pub const TYPE: u32 = 0x00;
    pub const OWNER: u32 = 0x04;
    /// The keys ([`super::fkey`]), in a block of the count rounded up to a multiple of 8.
    pub const KEYS: u32 = 0x08;
    pub const NB_KEYS: u32 = 0x0c;
    /// A float: −100000 in a new curve.
    pub const FLOAT_10: u32 = 0x10;
    /// 16-bit each: extrapolation before the first key and after the last, flags, the
    /// interpolation of new segments.
    pub const PRE_EXTRAPOLATION: u32 = 0x18;
    pub const POST_EXTRAPOLATION: u32 = 0x1a;
    pub const FLAGS: u32 = 0x1c;
    pub const INTERPOLATION: u32 = 0x1e;
    /// A curve of its own that `SI_fcurveDispose` disposes of with this one.
    pub const SUB_CURVE: u32 = 0x54;
    /// 16-bit: bit 0 is the "dirty" mark.
    pub const MARKS: u32 = 0x62;
}

/// `SI_Fcvkey`, 36 bytes: a key of a function curve.
pub mod fkey {
    pub const SIZE: u32 = 0x24;
    pub const TIME: u32 = 0x00;
    pub const VALUE: u32 = 0x04;
    pub const RIGHT_SLOPE: u32 = 0x08;
    /// 16-bit each.
    pub const FLAG: u32 = 0x0c;
    pub const INTERPOLATION: u32 = 0x0e;
    /// The curve the key belongs to.
    pub const CURVE: u32 = 0x10;
    /// 16-bit.
    pub const WORD_14: u32 = 0x14;
    pub const LEFT_SLOPE: u32 = 0x18;
    /// 16-bit each.
    pub const KEY_TYPE: u32 = 0x1c;
    pub const SPLINE_INDEX: u32 = 0x1e;
    pub const WORD_20: u32 = 0x20;
}

/// `SI_HaStaticVal`, 8 bytes: a static value of a struct key file.
pub mod static_val {
    pub const SIZE: u32 = 8;
    pub const TYPE: u32 = 0x00;
    /// A float.
    pub const VALUE: u32 = 0x04;
}

/// `SI_HierAnim`, 0x1c bytes (`SI_hieranimAllocate`): what an animation or struct key file
/// holds.
pub mod hier_anim {
    pub const SIZE: u32 = 0x1c;
    pub const NB_ELEMS: u32 = 0x00;
    /// The elements ([`super::hier_elem`]), grown by one record for each.
    pub const ELEMS: u32 = 0x04;
    /// The index of the dictionary's chapter the elements belong to.
    pub const CHAPTER: u32 = 0x08;
    /// The two action lists ([`super::actions`]).
    pub const ACTIONS: u32 = 0x0c;
    pub const ACTIONS2: u32 = 0x14;
}

/// An element of an `SI_HierAnim`, 0x28 bytes (`SI_hieranimAddElem`). Its items are six
/// arrays of one entry an item, each a block of its own grown by one entry for each.
pub mod hier_elem {
    pub const SIZE: u32 = 0x28;
    /// A text of its own block.
    pub const NAME: u32 = 0x00;
    /// The index of the element's father in the list, −1 for the root.
    pub const FATHER: u32 = 0x04;
    pub const NB_ITEMS: u32 = 0x08;
    /// 16-bit each: 0 a function curve, 1 a static value.
    pub const TYPES: u32 = 0x0c;
    /// 32-bit each: the action relation's id, −1 for none.
    pub const REL_IDS: u32 = 0x10;
    /// 16-bit each.
    pub const OCC_IDS: u32 = 0x14;
    /// 32-bit each: the curve or the static value.
    pub const DATA: u32 = 0x18;
    /// 16-bit each: 1.
    pub const ONES: u32 = 0x1c;
    /// 16-bit each: the word under token 7.
    pub const FLAGS: u32 = 0x20;
    pub const WORD_24: u32 = 0x24;
}

/// `SI_Actions`, 8 bytes: an action list.
pub mod actions {
    pub const NB_ELEMS: u32 = 0x00;
    /// The elements ([`super::actions_elem`]).
    pub const ELEMS: u32 = 0x04;
}

/// `SI_ActionsElem`, 0x30 bytes (`SI_actionsAddElemTime` and the action list reader). The
/// last two bytes are written by neither.
pub mod actions_elem {
    pub const SIZE: u32 = 0x30;
    /// A text of its own block.
    pub const NAME: u32 = 0x00;
    /// Floats, seconds.
    pub const START: u32 = 0x08;
    pub const END: u32 = 0x0c;
    /// 10, or 2 for an element the file marks with token 0x10; bit 0 is "selected".
    pub const FLAGS: u32 = 0x18;
    pub const WORD_1C: u32 = 0x1c;
    pub const WORD_20: u32 = 0x20;
    pub const CYCLES: u32 = 0x24;
    /// The name of an element of the second list, a text of its own block.
    pub const NAME2: u32 = 0x04;
    /// Floats: the clip's start and end.
    pub const CLIP_START: u32 = 0x10;
    pub const CLIP_END: u32 = 0x14;
    /// The path animation record ([`super::path_acts`]), or null.
    pub const PATH: u32 = 0x28;
    /// 16-bit: the interpolation code.
    pub const INTERPOLATION: u32 = 0x2c;
}

/// `SI_PathAnimActs`, 0x1c bytes (`SI_actionsElemBinaryRead`).
pub mod path_acts {
    pub const SIZE: u32 = 0x1c;
    /// −1 each in a new record.
    pub const WORD_00: u32 = 0x00;
    pub const WORD_04: u32 = 0x04;
    /// A function curve.
    pub const CURVE: u32 = 0x08;
    /// Four texts, each a block of its own (tokens 0x17, 0x16, 0x19, 0x18).
    pub const TEXT_0C: u32 = 0x0c;
    pub const TEXT_10: u32 = 0x10;
    pub const TEXT_14: u32 = 0x14;
    pub const TEXT_18: u32 = 0x18;
}

/// A sun's record, 0x14 bytes (`si_lightInitialize`).
pub mod sun {
    pub const SIZE: u32 = 0x14;
    /// A text of its own block.
    pub const NAME: u32 = 0x00;
    pub const LATITUDE: u32 = 0x04;
    pub const LONGITUDE: u32 = 0x08;
    /// 16-bit: daylight saving time.
    pub const DAYST: u32 = 0x0c;
    /// A C `time_t`.
    pub const DATE_TIME: u32 = 0x10;
}

/// A graph node (`__SI_GrafNode`; `si-types` has the record): the three members the light
/// reader's old display tokens store.
pub mod graf_node {
    pub const DISPLAY: u32 = 0x08;
    pub const WORD_0C: u32 = 0x0c;
    /// 16-bit: the wire colour.
    pub const COLOUR: u32 = 0x12;
}

/// A user data node, 0x14 bytes (`si_userDataRead`).
pub mod user_node {
    pub const SIZE: u32 = 0x14;
    /// A text of its own block.
    pub const NAME: u32 = 0x00;
    /// 16-bit.
    pub const WORD: u32 = 0x04;
    pub const DATA_SIZE: u32 = 0x08;
    pub const DATA: u32 = 0x0c;
    pub const NEXT: u32 = 0x10;
}

/// A position that follows a model, 0x1c bytes (`oldpositionBinaryRead`), on the list
/// `si_cnstrLst`.
pub mod cnstr {
    pub const SIZE: u32 = 0x1c;
    /// The camera or light the position belongs to.
    pub const OWNER: u32 = 0x00;
    /// 4 and 5 a camera's position and interest, 6 and 7 a light's.
    pub const KIND: u32 = 0x04;
    pub const WORD_08: u32 = 0x08;
    /// The model's name, a text of its own block.
    pub const NAME: u32 = 0x0c;
    pub const WORD_10: u32 = 0x10;
    /// The number read before the name.
    pub const NUMBER: u32 = 0x14;
    pub const NEXT: u32 = 0x18;
}

/// A custom shader record (`__SI_UDF`, made by `SI_udfAllocate (0x0854c2f4)`): the four
/// members `si_udfRead` stores.
pub mod udf {
    /// A text of its own block.
    pub const NAME: u32 = 0x00;
    /// 16-bit each.
    pub const WORD_0C: u32 = 0x0c;
    pub const WORD_10: u32 = 0x10;
    pub const WORD_14: u32 = 0x14;
}

/// The prototype block the shader reader is handed (`si_Prototypes`, 0x64 bytes): its last
/// word points at the shader's token list, or is null.
pub mod prototypes {
    pub const SIZE: u32 = 0x64;
    pub const UDF: u32 = 0x60;
}
