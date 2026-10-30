// SPDX-License-Identifier: MIT
//! The parameter table: which function curve drives which field of which structure
//! (finding 0017, "The table").
//!
//! Every row was read from the routine it names, in `motionUpdate.c`; the rows of routines
//! ported in [`crate::update`] are what that code does (and, where the emulation check
//! covers the routine, what the original was measured to do). The curve names are those of
//! the vendor's `Relations.rsrc`.
//!
//! A curve is found through a relation of its element: `rel` is the relation id (the id of
//! the attribute line in `Relations.rsrc`), `list` the relation list of the element that
//! holds it (0 parameters, 1 transformation, 2 geometry). The curve's own type id says which
//! component it is. `chapter` is the chapter of the element that carries the relation.

/// What is done to the curve's value on its way to the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conv {
    /// Stored as it is (rounded to the field's type).
    Plain,
    /// Degrees in the curve, radians in the field: × the double 0.0174532925.
    DegToRad,
    /// A value below 0 is stored as 0.
    NotNegative,
    /// A value at or below 0 is stored as 0, one at or above 1 as 1.
    Unit,
    /// A value below −1 is stored as −1.
    NotBelowMinusOne,
    /// Truncated toward zero to the integer field.
    Truncate,
    /// On when the value equals 1.
    EqualsOne,
    /// A percentage (0 to 100) of the length of a path; the field gets the point there.
    PathPercent,
    /// A curve without keys counts as 1.
    OneIfNoKeys,
    /// A magnitude below 0.0001 becomes ±0.0001 (a projection plane may not collapse).
    AwayFromZero,
    /// See the routine: the value takes part in a computation.
    Special,
}

/// What kind of destination a row has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    /// A `float` at `offset` of the object (the first curve; the next ones 4 bytes apart).
    F32,
    /// A `double` (curves 8 bytes apart).
    F64,
    /// A 16-bit integer.
    I16,
    /// A 32-bit integer.
    I32,
    /// A bit of the byte at `offset`; the row's note gives the mask.
    Bit,
    /// The value is handed to a routine of another part of the program.
    Call,
    /// The curve is evaluated and nothing is stored (its value stays in the curve record).
    EvaluatedOnly,
    /// The update at a time does not apply the curve at all.
    NotApplied,
}

/// One row: the curves of one parameter.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// Curve type ids, in component order.
    pub curves: &'static [i32],
    /// Relation id, and the relation list of the element.
    pub rel: i32,
    pub list: i8,
    /// Chapter of the element that carries the relation.
    pub chapter: &'static str,
    /// The routine of `motionUpdate.c` that applies the curves.
    pub routine: &'static str,
    /// The structure written, and where it hangs when it is not the element's own object.
    pub object: &'static str,
    pub offset: u32,
    pub dest: Dest,
    pub conv: Conv,
    pub note: &'static str,
}

const fn row(curves: &'static [i32], rel: i32, list: i8, chapter: &'static str, routine: &'static str, object: &'static str,
             offset: u32, dest: Dest, conv: Conv, note: &'static str)
             -> Row {
    Row { curves, rel, list, chapter, routine, object, offset, dest, conv, note }
}

use Conv::*;
use Dest::*;

const MP: &str = "animModelParametersUpdate";
const CP: &str = "animCameraParametersUpdate";
const LP: &str = "animLightParametersUpdate";
const MATP: &str = "animMatParametersUpdate";
const T2P: &str = "animTexParametersUpdate";
const T3P: &str = "animTex3DParametersUpdate";
const SPL: &str = "animSplineDefUpdate";
const PTC: &str = "animPatchDefUpdate";

/// The table. Rows are in the order of `Relations.rsrc`'s element blocks.
pub static TABLE: &[Row] = &[
    // ---- models
    row(&[1, 2, 3], 100, 1, "MODELS", "animScalingUpdate", "__SI_Model", 0x100, F32, OneIfNoKeys, "local scaling x, y, z; update flag 2"),
    row(&[4, 5, 6], 101, 1, "MODELS", "animRotationUpdate", "__SI_Model", 0x10c, F32, DegToRad,
        "local rotation angles; x and y also copied to +0x164, +0x168; update flag 4; a joint's preferred angle is reset"),
    row(&[139, 140, 141], 113, 1, "MODELS", "animExplicitPosUpdate", "__SI_Model", 0x158, F32, Plain,
        "local translation, or the inverse-kinematics goal of an IK target; yields to a path outside the curve's keys and to a position constraint; update flag 8"),
    row(&[7], 112, 1, "MODELS", "animAllTypePathUpdate", "__SI_Model", 0x158, F32, PathPercent,
        "all three translation floats, the path point brought into the node's father's space; or the IK goal"),
    row(&[175], 10, 0, "MODELS", MP, "", 0, Call, EqualsOne, "SI_modelSetHiddenParam(graf node, model, element, 2 or 0, 0): node visibility"),
    row(&[273], 12, 0, "MODELS", MP, "", 0, Call, EqualsOne, "SI_modelSetHiddenParam(…, 1): branch visibility"),
    row(&[178], 11, 0, "MODELS", "animAlgoTypeUpdate", "", 0, Call, EqualsOne, "SI_dynaAlgoTypeSet: dynamics on or off, read as an integer flag from the chain root's element"),
    row(&[371], 15, 0, "MODELS", MP, "__SI_Model", 0x3f4, F32, Plain, "node matte"),
    row(&[372], 16, 0, "MODELS", MP, "", 0, Call, Plain, "SI_modelPropagateMatteParam(model, value): branch matte"),
    row(&[538], 0, 0, "MODELS", "", "", 0, NotApplied, Plain, "garbage matte: no relation id of its own, no case in the update"),
    row(&[620], 4000, 0, "MODELS", MP, "__SI_Model", 0x404, Bit, EqualsOne, "mask 1: caustic transmitter"),
    row(&[631], 4001, 0, "MODELS", MP, "__SI_Model", 0x404, Bit, EqualsOne, "mask 2: caustic receiver"),
    row(&[632], 4002, 0, "MODELS", MP, "__SI_Model", 0x406, Bit, EqualsOne, "mask 1: global illumination transmitter"),
    row(&[633], 4003, 0, "MODELS", MP, "__SI_Model", 0x406, Bit, EqualsOne, "mask 2: global illumination receiver"),
    row(&[676], 4100, 0, "MODELS", "", "", 0, NotApplied, Plain, "wire-frame display: the parameter dispatcher has no case for relation 4100"),
    row(&[185], 241, 2, "MODELS", "animVClayUpdate", "SI_Metaball (the model's definition)", 0xd4, F64, Plain,
        "metaball weight; +0xcc is derived from it through the metaball library"),
    row(&[186, 187, 188], 242, 2, "MODELS", "animVClayUpdate", "SI_Metaball (the model's definition)", 0xdc, F64, Plain, "super-ellipsoid exponents"),
    row(&[179], 50, 0, "MODELS", MP, "SI_DynaMatterInfo (model + 0x2f4)", 0x14, F32, Plain, "elasticity"),
    row(&[180], 51, 0, "MODELS", MP, "SI_DynaMatterInfo (model + 0x2f4)", 0x0c, F32, Plain, "static roughness"),
    row(&[181], 52, 0, "MODELS", MP, "SI_DynaMatterInfo (model + 0x2f4)", 0x10, F32, Plain, "kinetic roughness"),
    row(&[182], 53, 0, "MODELS", MP, "SI_DynaChnVars (model + 0x304)", 0x38, I16, Truncate, "root constraint"),
    row(&[264], 0, 2, "MODELS", "", "", 0, NotApplied, Plain, "substitution number: not applied by motionUpdate.c"),
    // the kinematic state: curves the plot command writes (SI_scenePlotKinStateFromAnim, mot_plot.c);
    // the transformation dispatcher has no case for relations 90 to 93, so nothing reads them back
    row(&[221, 222, 223], 90, 1, "MODELS", "", "kinematic state (model + 0x2f0)", 0, NotApplied, Plain, "linear velocity, a plotted result"),
    row(&[224, 225, 226], 91, 1, "MODELS", "", "kinematic state (model + 0x2f0)", 0, NotApplied, Plain, "linear acceleration, a plotted result"),
    row(&[227, 228, 229], 92, 1, "MODELS", "", "kinematic state (model + 0x2f0)", 0, NotApplied, Plain, "angular velocity, a plotted result"),
    row(&[230, 231, 232], 93, 1, "MODELS", "", "kinematic state (model + 0x2f0)", 0, NotApplied, Plain, "angular acceleration, a plotted result"),
    // ---- deformations of a model (operator attributes)
    row(&[8], 201, 0, "SHAPES", "animShapeUpdate", "__SI_Model", 0x48, Call, Special,
        "shape interpolation: the position among the model's definitions (section 8a; shape.c does the blending)"),
    row(&[154], 202, 0, "SHAPES", "animShapeUpdate", "", 0, Call, Special, "shape weights, used when the model's interpolation mode (+0x2e0) is 2: read by `si_<kind>WeightInterpolate` of shape.c, not here"),
    row(&[66], 211, 2, "LATTICES", "animLatticeInterpUpdate", "SI_Lattice", 0x40, F32, Plain, "position among the node lattice's keys"),
    row(&[67], 216, 2, "LATTICES", "animLatticeInterpUpdate", "SI_Lattice", 0x40, F32, Plain, "position among the branch lattice's keys"),
    row(&[107, 108, 109], 220, 2, "MODELS", SPL, "SI_SplFitting (model + 0x60)", 0x0c, F32, OneIfNoKeys, "node deformation by spline: scaling"),
    row(&[110], 220, 2, "MODELS", SPL, "SI_SplFitting (model + 0x60)", 0x18, F32, DegToRad, "roll"),
    row(&[111, 112, 113], 220, 2, "MODELS", SPL, "SI_SplFitting (model + 0x60)", 0x1c, F32, Plain, "translation"),
    row(&[114, 115, 116], 221, 2, "MODELS", SPL, "SI_SplFitting (model + 0x64)", 0x0c, F32, OneIfNoKeys, "branch deformation by spline: scaling"),
    row(&[117], 221, 2, "MODELS", SPL, "SI_SplFitting (model + 0x64)", 0x18, F32, DegToRad, "roll"),
    row(&[118, 119, 120], 221, 2, "MODELS", SPL, "SI_SplFitting (model + 0x64)", 0x1c, F32, Plain, "translation"),
    row(&[121, 122, 123], 230, 2, "MODELS", PTC, "SI_PtchFitting (model + 0x2d8)", 0x08, F32, OneIfNoKeys, "node deformation by patch: scaling"),
    row(&[124, 125, 126], 230, 2, "MODELS", PTC, "SI_PtchFitting (model + 0x2d8)", 0x14, F32, DegToRad, "rotation"),
    row(&[127, 128, 129], 230, 2, "MODELS", PTC, "SI_PtchFitting (model + 0x2d8)", 0x20, F32, Plain, "translation"),
    row(&[130, 131, 132], 231, 2, "MODELS", PTC, "SI_PtchFitting (model + 0x2dc)", 0x08, F32, OneIfNoKeys, "branch deformation by patch: scaling"),
    row(&[133, 134, 135], 231, 2, "MODELS", PTC, "SI_PtchFitting (model + 0x2dc)", 0x14, F32, DegToRad, "rotation"),
    row(&[136, 137, 138], 231, 2, "MODELS", PTC, "SI_PtchFitting (model + 0x2dc)", 0x20, F32, Plain, "translation"),
    row(&[233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255, 256], 239, 2,
        "MODELS", "updNodeGeometryUpdate", "", 0, NotApplied, Plain,
        "quick stretch parameters: the step only recomputes the kinematic state and leaves post info 3; the curves are read by the quick stretch code"),
    row(&[183], 701, 2, "CLUSTERS", "animClustersUpdate", "", 0, Call, Special, "cluster shape interpolation: read by the `si_cluster…Interpolate` routines of cluster.c, which `animClustersUpdate` asks point by point (section 8a)"),
    row(&[184], 702, 2, "CLUSTERS", "animClustersUpdate", "", 0, Call, Special, "cluster shape weights: as 183"),
    // ---- relations
    row(&[196], 11000, 5, "*", "SI_checkActiveTime", "", 0, Call, Special,
        "a relation's activation curve: read as an integer flag; while it is off the relation's step is evaluated at the time it went off"),
    row(&[197], 11001, 5, "*", "", "", 0, NotApplied, Plain, "a relation's weight: not read in motionUpdate.c"),
    row(&[566], 20000, 4, "MODELS", "SI_cusUpdateEffect", "", 0, Call, Special, "a custom effect's parameter: evaluated by the effect"),
    // ---- materials
    row(&[45, 46, 47], 301, 0, "MATERIALS", "animMatColorUpdate", "__SI_Material", 0x34, F32, Plain, "ambient red, green, blue"),
    row(&[48, 49, 50], 302, 0, "MATERIALS", "animMatColorUpdate", "__SI_Material", 0x44, F32, Plain, "diffuse"),
    row(&[51, 52, 53], 303, 0, "MATERIALS", "animMatColorUpdate", "__SI_Material", 0x54, F32, Plain, "specular"),
    row(&[54], 304, 0, "MATERIALS", MATP, "__SI_Material", 0x64, F32, NotNegative, "specular decay"),
    row(&[55], 305, 0, "MATERIALS", MATP, "__SI_Material", 0x6a8, F32, NotNegative, "reflectivity"),
    row(&[56], 306, 0, "MATERIALS", MATP, "__SI_Material", 0x6ac, F32, NotNegative, "reflection jitter"),
    row(&[57], 307, 0, "MATERIALS", MATP, "__SI_Material", 0x6b8, F32, NotNegative, "transparency"),
    row(&[58], 308, 0, "MATERIALS", MATP, "__SI_Material", 0x6bc, F32, NotNegative, "transparency jitter"),
    row(&[59], 310, 0, "MATERIALS", MATP, "__SI_Material", 0x6dc, F32, NotNegative, "refraction index"),
    row(&[257], 311, 0, "MATERIALS", MATP, "__SI_Material", 0x75c, F32, Plain, "blur width"),
    row(&[258], 312, 0, "MATERIALS", MATP, "__SI_Material", 0x760, F32, Plain, "blur decay"),
    // ---- 2D textures
    row(&[33], 411, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x58, F32, Plain, "u scale"),
    row(&[34], 412, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x5c, F32, Plain, "v scale"),
    row(&[35], 413, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x68, F32, Plain, "u offset"),
    row(&[36], 414, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x6c, F32, Plain, "v offset"),
    row(&[161], 415, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x10c, F32, Plain, "blending"),
    row(&[37], 416, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x128, F32, NotNegative, "ambient"),
    row(&[38], 417, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x12c, F32, NotNegative, "diffuse"),
    row(&[39], 418, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x130, F32, NotNegative, "specular"),
    row(&[41], 419, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x138, F32, Plain, "transparency"),
    row(&[162], 420, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x134, F32, Plain, "reflectivity"),
    row(&[43], 421, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x140, F32, Plain, "roughness"),
    row(&[40], 422, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x144, F32, NotNegative, "reflection map"),
    row(&[44], 423, 0, "TEXTURES2D", T2P, "__SI_LclMap", 0x168, F32, DegToRad, "rotation of the picture"),
    row(&[206, 207, 208], 424, 0, "TEXTURES2D", "animTex2dVecUpdate", "__SI_LclMap", 0x94, F32, DegToRad,
        "rotation of the projection support; a component without a curve is converted again at every update"),
    row(&[209, 210, 211], 425, 0, "TEXTURES2D", "animTex2dVecUpdate", "__SI_LclMap", 0xac, F32, OneIfNoKeys, "scaling of the support"),
    row(&[212, 213, 214], 426, 0, "TEXTURES2D", "animTex2dVecUpdate", "__SI_LclMap", 0xa0, F32, Plain, "translation of the support"),
    // ---- 3D textures
    row(&[69], 511, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x10, F32, Plain, "spacing"),
    row(&[72], 512, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x14, F32, DegToRad, "taper angle"),
    row(&[68], 513, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x18, F32, Plain, "strength"),
    row(&[70], 514, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x20, F32, Plain, "power"),
    row(&[155], 515, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x28, F32, Plain, "blending"),
    row(&[76, 77, 78, 156], 516, 0, "TEXTURES3D", "animTex3dColorUpdate", "__SI_Texture3D", 0x30, F32, NotNegative, "colour 0: red, green, blue, alpha"),
    row(&[79, 80, 81, 157], 517, 0, "TEXTURES3D", "animTex3dColorUpdate", "__SI_Texture3D", 0x40, F32, NotNegative, "colour 1"),
    row(&[82, 83, 84, 158], 518, 0, "TEXTURES3D", "animTex3dColorUpdate", "__SI_Texture3D", 0x50, F32, NotNegative, "colour 2"),
    row(&[85, 86, 87, 159], 519, 0, "TEXTURES3D", "animTex3dColorUpdate", "__SI_Texture3D", 0x60, F32, NotNegative, "colour 3"),
    row(&[88, 89, 90, 160], 520, 0, "TEXTURES3D", "animTex3dColorUpdate", "__SI_Texture3D", 0x70, F32, NotNegative, "colour 4"),
    row(&[73], 521, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x80, F32, Unit, "factor 1"),
    row(&[74], 522, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x84, F32, Unit, "factor 2"),
    row(&[75], 523, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x88, F32, Unit, "factor 3"),
    row(&[91, 92, 93], 524, 0, "TEXTURES3D", "animTex3dVecUpdate", "__SI_Texture3D", 0x90, F32, OneIfNoKeys, "scaling"),
    row(&[94, 95, 96], 525, 0, "TEXTURES3D", "animTex3dVecUpdate", "__SI_Texture3D", 0x9c, F32, DegToRad,
        "rotation; a component without a curve is converted again at every update"),
    row(&[97, 98, 99], 526, 0, "TEXTURES3D", "animTex3dVecUpdate", "__SI_Texture3D", 0xa8, F32, Plain, "translation"),
    row(&[100], 527, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x108, F32, NotNegative, "ambient"),
    row(&[101], 528, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x10c, F32, NotNegative, "diffuse"),
    row(&[102], 529, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x110, F32, NotNegative, "specular"),
    row(&[104], 530, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x118, F32, NotBelowMinusOne, "transparency"),
    row(&[103], 531, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x114, F32, NotBelowMinusOne, "reflectivity"),
    row(&[106], 532, 0, "TEXTURES3D", T3P, "__SI_Texture3D", 0x120, F32, Plain, "roughness"),
    // ---- cameras
    row(&[177], 1100, 0, "CAMERAS", CP, "__SI_GrafNode", 0x0c, I32, EqualsOne,
        "display type 2 or 0 of every camera graf node of the current list, and the element's wire type"),
    row(&[142, 143, 144], 1113, 1, "CAMERAS", "animCameraExplicitPosUpdate", "SI_Camera", 0x10, F32, Plain, "position"),
    row(&[145, 146, 147], 1115, 1, "CAMERAS", "animCameraExplicitPosUpdate", "SI_Camera", 0x1c, F32, Plain, "interest"),
    row(&[9], 1112, 1, "CAMERAS", "animAllTypePathUpdate", "SI_Camera", 0x10, F32, PathPercent, "position, the path point in world space"),
    row(&[10], 1114, 1, "CAMERAS", "animAllTypePathUpdate", "SI_Camera", 0x1c, F32, PathPercent, "interest, the path point in world space"),
    row(&[11], 1130, 1, "CAMERAS", CP, "SI_Camera", 0x30, F32, DegToRad, "roll"),
    row(&[12], 1131, 0, "CAMERAS", CP, "SI_Camera", 0x40, F32, DegToRad,
        "field of view, kept 0.0001 away from zero; then projection plane distance (+0x10c) = (0.5 × height) / tan(0.5 × fov)"),
    row(&[13], 1132, 0, "CAMERAS", CP, "SI_Camera", 0x34, F32, Plain, "near plane"),
    row(&[14], 1133, 0, "CAMERAS", CP, "SI_Camera", 0x38, F32, Plain, "far plane"),
    row(&[15], 1134, 0, "CAMERAS", CP, "SI_Camera", 0x70, F32, Plain, "focal length"),
    row(&[16], 1135, 0, "CAMERAS", CP, "SI_Camera", 0x94, F32, Plain, "f-stop"),
    row(&[17], 1136, 0, "CAMERAS", CP, "SI_Camera", 0x74, F32, Plain, "focal distance"),
    row(&[18], 1137, 0, "CAMERAS", CP, "SI_Camera", 0x7c, F32, Plain, "near focus"),
    row(&[19], 1138, 0, "CAMERAS", CP, "SI_Camera", 0x84, F32, Plain, "far focus"),
    row(&[20], 1139, 0, "CAMERAS", CP, "SI_Camera", 0x8c, F32, Plain, "out of focus"),
    row(&[21], 1140, 0, "CAMERAS", CP, "SI_Camera", 0x98, F32, Plain, "maximum circle of confusion"),
    row(&[373], 1141, 0, "CAMERAS", CP, "SI_Camera", 0x10c, F32, AwayFromZero, "projection plane distance; then fov = 2 × atan(0.5 × height / distance)"),
    row(&[374, 375], 1142, 0, "CAMERAS", CP, "SI_Camera", 0x110, F32, AwayFromZero, "projection plane width, height; the height also recomputes the fov"),
    row(&[376, 377], 1143, 0, "CAMERAS", CP, "SI_Camera", 0x118, F32, Plain, "projection plane offset x, y"),
    row(&[378], 1144, 0, "CAMERAS", CP, "SI_Camera", 0x108, I16, Truncate, "projection plane active"),
    // ---- lights
    row(&[176], 2100, 0, "LIGHTS", LP, "light", 0x176, I16, EqualsOne, "visible flag; display type of the light's graf nodes and the element's wire type"),
    row(&[148, 149, 150], 2113, 1, "LIGHTS", "animLightExplicitPosUpdate", "light", 0x34, F32, Plain, "position"),
    row(&[151, 152, 153], 2115, 1, "LIGHTS", "animLightExplicitPosUpdate", "light", 0x58, F32, Plain, "interest (spot)"),
    row(&[31], 2112, 1, "LIGHTS", "animAllTypePathUpdate", "light", 0x34, F32, PathPercent, "position, the path point in world space"),
    row(&[32], 2114, 1, "LIGHTS", "animAllTypePathUpdate", "light", 0x58, F32, PathPercent, "interest, the path point in world space"),
    row(&[189], 2120, 1, "LIGHTS", "animLightSunTimeUpdate", "sun record (light + 0x16c)", 0x10, I32, Truncate,
        "date and time (time_t); the light's position then comes from SI_lightSunGetPosition"),
    row(&[22, 23, 24], 2130, 0, "LIGHTS", LP, "light", 0x24, F32, Plain, "colour"),
    row(&[25], 2131, 0, "LIGHTS", LP, "light", 0x70, F32, DegToRad, "cone angle"),
    row(&[26], 2132, 0, "LIGHTS", LP, "light", 0x74, F32, DegToRad, "spread angle"),
    row(&[28], 2134, 0, "LIGHTS", LP, "light", 0x84, F32, Plain, "fall-off start"),
    row(&[29], 2135, 0, "LIGHTS", LP, "light", 0x88, F32, Plain, "fall-off end"),
    row(&[30], 2136, 0, "LIGHTS", LP, "light", 0x90, F32, Plain, "shadow factor"),
    row(&[215, 216, 217], 2201, 0, "LIGHTS", LP, "light", 0x1d0, F32, Special,
        "area rotation: stored as it is (no conversion from degrees) into angles re-read from the matrix at +0x1e0, which is then rebuilt"),
    row(&[218, 219, 220], 2202, 0, "LIGHTS", LP, "light", 0x1c4, F32, Plain, "area scaling; x is also the radius of a sphere (+0x190) or disk (+0x19c)"),
    row(&[621, 622, 623], 2203, 0, "LIGHTS", LP, "light", 0x224, F32, NotNegative, "photon energy"),
    row(&[624], 2204, 0, "LIGHTS", LP, "light", 0x230, F32, NotNegative, "energy fall-off exponent"),
    row(&[625], 2205, 0, "LIGHTS", LP, "light", 0x234, I32, Truncate, "caustic photons; 0 when below 0"),
    row(&[626], 2206, 0, "LIGHTS", LP, "light", 0x238, I32, Truncate, "global illumination photons; 0 when below 0"),
    row(&[627], 2207, 0, "LIGHTS", LP, "light", 0x23c, I16, EqualsOne, "caustics"),
    row(&[628], 2208, 0, "LIGHTS", LP, "light", 0x23e, I16, EqualsOne, "global illumination"),
    // ---- the scene
    row(&[60], 3000, 0, "SCENES", "animFadParametersUpdate", "render control (SI_defaultControl)", 0x104, F32, Plain, "depth fading start"),
    row(&[61], 3001, 0, "SCENES", "animFadParametersUpdate", "render control (SI_defaultControl)", 0x10c, F32, Plain, "depth fading stop"),
    row(&[65], 3002, 0, "SCENES", "animFadParametersUpdate", "render control (SI_defaultControl)", 0x130, F32, Plain, "depth fading transparency"),
    row(&[62, 63, 64], 3003, 0, "SCENES", "animFadParametersUpdate", "render control (SI_defaultControl)", 0x118, F32, Plain, "depth fading colour"),
    row(&[198], 3030, 0, "SCENES", "animFogParametersUpdate", "render control (SI_defaultControl)", 0x26c, F32, Plain, "fog start"),
    row(&[199], 3031, 0, "SCENES", "animFogParametersUpdate", "render control (SI_defaultControl)", 0x270, F32, Plain, "fog stop"),
    row(&[203], 3032, 0, "SCENES", "animFogParametersUpdate", "render control (SI_defaultControl)", 0x260, F32, Plain, "fog density"),
    row(&[200, 201, 202], 3033, 0, "SCENES", "animFogParametersUpdate", "render control (SI_defaultControl)", 0x284, F32, Plain, "fog colour"),
    row(&[204], 3034, 0, "SCENES", "animFogParametersUpdate", "render control (SI_defaultControl)", 0x264, F32, Plain, "fog base"),
    row(&[205], 3035, 0, "SCENES", "animFogParametersUpdate", "render control (SI_defaultControl)", 0x268, F32, Plain, "fog thickness"),
    // ---- waves
    row(&[163, 164, 165], 610, 1, "WAVES", "animScalingUpdate", "__SI_Model (the wave's icon)", 0x100, F32, OneIfNoKeys, "scaling"),
    row(&[166, 167, 168], 611, 1, "WAVES", "animRotationUpdate", "__SI_Model (the wave's icon)", 0x10c, F32, DegToRad, "rotation"),
    row(&[169, 170, 171], 613, 1, "WAVES", "animExplicitPosUpdate", "__SI_Model (the wave's icon)", 0x158, F32, Plain, "translation"),
    row(&[172], 612, 1, "WAVES", "animAllTypePathUpdate", "__SI_Model (the wave's icon)", 0x158, F32, PathPercent, "translation from the path"),
    row(&[173], 614, 0, "WAVES", "animWaveParametersUpdate", "", 0, EvaluatedOnly, Plain, "decay"),
    row(&[174], 615, 0, "WAVES", "animWaveParametersUpdate", "", 0, EvaluatedOnly, Plain, "spread"),
];

impl Row {
    /// Distance in bytes between the fields of successive curves of the row.
    pub fn step(&self) -> u32 {
        match self.dest {
            F64 => 8,
            F32 => 4,
            _ => 0,
        }
    }
}

/// The row of a curve type, if the table has it.
pub fn row_of(curve: i32) -> Option<&'static Row> {
    TABLE.iter().find(|r| r.curves.contains(&curve))
}

/// The rows as tab-separated text, one line per curve type: curve, relation, list, chapter,
/// routine, object, offset, destination, conversion, note.
pub fn tsv() -> String {
    let mut out = String::new();
    for r in TABLE {
        for (i, c) in r.curves.iter().enumerate() {
            // the alpha of a 3D texture colour sits behind the three components
            let off = r.offset + r.step() * i as u32;
            out.push_str(&format!("{c}\t{}\t{}\t{}\t{}\t{}\t{:#x}\t{:?}\t{:?}\t{}\n", r.rel, r.list, r.chapter, r.routine, r.object, off,
                                  r.dest, r.conv, r.note));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_curve_twice_and_fields_do_not_collide() {
        let mut seen = std::collections::HashMap::new();
        for r in TABLE {
            for c in r.curves {
                assert!(seen.insert(*c, r.rel).is_none(), "curve {c} is in two rows");
            }
        }
        assert_eq!(row_of(12).unwrap().offset, 0x40);
        assert_eq!(row_of(156).unwrap().offset + 3 * row_of(156).unwrap().step(), 0x3c);
        assert!(row_of(9999).is_none());
        assert_eq!(tsv().lines().count(), 273);
    }
}
