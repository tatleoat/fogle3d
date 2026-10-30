// SPDX-License-Identifier: MIT
//! The constants the dictionary functions read by address: string literals and the
//! chapter table. The image holds none of the binary's data; a crate stores what it owns.
//! Every entry is held against the binary by `re/tools/si-dict/check_rodata.py`.

use crate::globals::SI_CHAP_INFO;
use si_mem::{CStr32, Mem, Ptr32, RODATA};

/// `unknown`: what `SI_reldictGetRelName` answers for an id that is not in the dictionary.
pub const S_UNKNOWN: CStr32 = Ptr32::new(0x0868456c);
/// The empty string `SI_dbScnGetListNode` copies into a new name record's +8.
pub const S_EMPTY: CStr32 = Ptr32::new(0x08683b84);

/// (address, text) of every string literal a function of this crate reads or hands on.
pub const STRINGS: &[(u32, &str)] = &[
    (S_UNKNOWN.0, "unknown"),
    (S_EMPTY.0, ""),
    (S_EMPTY_PREFIX.0, ""),
    (S_EMPTY_DSC_PREFIX.0, ""),
    (S_NO_NAME.0, "no_name"),
    (S_OBJ.0, "obj"),
    (S_LEGACY.0, "SI_XSI_LEGACYLOADER"),
    (S_LEGACY_2.0, "SI_XSI_LEGACYLOADER"),
    (S_NOTFOUND.0, "notfound"),
    (S_HIDDEN.0, "hidden"),
    (S_CAMHID.0, "camhid"),
    (S_LIGHID.0, "lighid"),
    (S_T2NORM.0, "t2norm"),
];

/// What the static `default_name` of `SI_attdictGetNameFromFcurve` points at.
pub const S_NOTFOUND: CStr32 = Ptr32::new(0x086827ac);
/// Four component names of older versions that `SI_attdictGetFcurveFromName` knows.
pub const S_HIDDEN: CStr32 = Ptr32::new(0x08682790);
pub const S_CAMHID: CStr32 = Ptr32::new(0x08682797);
pub const S_LIGHID: CStr32 = Ptr32::new(0x0868279e);
pub const S_T2NORM: CStr32 = Ptr32::new(0x086827a5);

/// The groups `SI_attdictSetFctsPointers` installs functions for, in its order: the
/// address and text of the group's name, then address and name of the function that
/// reads an attribute of the group and of the one that writes it (0: none). The
/// functions are not ported; a group holds their code addresses.
pub const ATT_FCTS: &[(u32, &str, u32, &str, u32, &str)] = &[
    (0x08682500, "General_Element", 0x080f6460, "SI_generalAttrGetValue", 0x080f67ec, "SI_generalAttrSetValue"),
    (0x08682510, "MODELS", 0x080f6dd0, "SI_modelAttrGetValue", 0x080f7318, "SI_modelAttrSetValue"),
    (0x08682517, "Geometry", 0x080f73f0, "SI_geomAttrGetValue", 0x080f74f4, "SI_geomAttrSetValue"),
    (0x08682520, "CtrlVertex", 0x080f75c8, "SI_ctrlVertxAttrGetValue", 0x080f779c, "SI_ctrlVertxAttrSetValue"),
    (0x0868252b, "PtchInfo", 0x080f79d4, "SI_ptchAttrGetValue", 0x080f7ba4, "SI_ptchAttrSetValue"),
    (0x08682534, "SplineInfo", 0x080f7d90, "SI_splAttrGetValue", 0x080f7ea4, "SI_splAttrSetValue"),
    (0x0868253f, "FaceInfo", 0x080f7ffc, "SI_faceAttrGetValue", 0x080f80f4, "SI_faceAttrSetValue"),
    (0x08682548, "PolygonInfo", 0x080f81a0, "SI_meshAttrGetValue", 0x080f8364, "SI_meshAttrSetValue"),
    (0x08682554, "CurveInfo", 0x080f8588, "SI_crvAttrGetValue", 0x080f8684, "SI_crvAttrSetValue"),
    (0x0868255e, "SurfaceInfo", 0x080f8824, "SI_srfAttrGetValue", 0x080f89b4, "SI_srfAttrSetValue"),
    (0x0868256a, "MetaballInfo", 0x080f8b48, "SI_metaballAttrGetValue", 0x080f8cb4, "SI_metaballAttrSetValue"),
    (0x08682577, "MetaElement", 0x080f8e74, "SI_metaElementAttrGetValue", 0x080f8fd4, "SI_metaElementAttrSetValue"),
    (0x08682583, "PhysicalProps", 0x080f90d4, "SI_physicalAttrGetValue", 0x080f917c, "SI_physicalAttrSetValue"),
    (0x08682591, "BoundingBox", 0x080f9240, "SI_bboxAttrGetValue", 0x00000000, ""),
    (0x0868259d, "ChainInfo", 0x080f9340, "SI_chainAttrGetValue", 0x080f94f8, "SI_chainAttrSetValue"),
    (0x086825a7, "MATERIALS", 0x080f9674, "SI_materialsAttrGetValue", 0x080f9728, "SI_materialsAttrSetValue"),
    (0x086825b1, "TEXTURES2D", 0x080f97e0, "SI_texture2DAttrGetValue", 0x080f9894, "SI_texture2DAttrSetValue"),
    (0x086825bc, "TEXTURES3D", 0x080f994c, "SI_texture3DAttrGetValue", 0x080f9a04, "SI_texture3DAttrSetValue"),
    (0x086825c7, "CAMERAS", 0x080f9abc, "SI_camerasAttrGetValue", 0x080f9bac, "SI_camerasAttrSetValue"),
    (0x086825cf, "LIGHTS", 0x080f9cb8, "SI_lightsAttrGetValue", 0x080f9dec, "SI_lightsAttrSetValue"),
    (0x086825d6, "Spot", 0x080f9f4c, "SI_spotAttrGetValue", 0x080fa010, "SI_spotAttrSetValue"),
    (0x086825db, "Sun", 0x080fa0c4, "SI_sunAttrGetValue", 0x080fa184, "SI_sunAttrSetValue"),
    (0x086825df, "WAVES", 0x080fa3a0, "SI_wavesAttrGetValue", 0x080fa44c, "SI_wavesAttrSetValue"),
    (0x086825e5, "SCENES", 0x080fa23c, "SI_scenesAttrGetValue", 0x080fa2e8, "SI_scenesAttrSetValue"),
    (0x086825ec, "Schematic", 0x080fa514, "SI_schemAttrGetValue", 0x080fa5ec, "SI_schemAttrSetValue"),
    (0x086825f6, "ANIMATION", 0x080fa5f8, "SI_animAttrGetValue", 0x080fa784, "SI_animAttrSetValue"),
    (0x08682600, "FcvKey", 0x080fa8e4, "SI_fcvkeyAttrGetValue", 0x080faa50, "SI_fcvkeyAttrSetValue"),
    (0x08682607, "Action", 0x080fc4b4, "SI_actionsAttrGetValue", 0x080fc7ac, "SI_actionsAttrSetValue"),
];

/// The empty prefix `SI_elemdictChangeName` gives an element whose new prefix is null.
pub const S_EMPTY_PREFIX: CStr32 = Ptr32::new(0x08683e25);
/// The default scene's prefix (empty) and name.
pub const S_EMPTY_DSC_PREFIX: CStr32 = Ptr32::new(0x08683e4f);
pub const S_NO_NAME: CStr32 = Ptr32::new(0x08683e50);
/// The stem of a new name when the caller gives none.
pub const S_OBJ: CStr32 = Ptr32::new(0x08683e40);
/// The environment variable two routines ask for (two literals of the same text).
pub const S_LEGACY: CStr32 = Ptr32::new(0x08683e2c);
pub const S_LEGACY_2: CStr32 = Ptr32::new(0x08683e5e);

/// `SI_chapInfo` (0x086842c0): for each of the 36 chapters the address and text of its
/// name, the address and text of its file suffix, and the tag of its ASCII files.
pub const CHAP_INFO: [(u32, &str, u32, &str, u32); 36] = [
    (0x0868406c, "SCENES", 0x08684073, "dsc", 0x53445343),
    (0x08684077, "ANIMATION", 0x08684081, "ani", 0x53414e49),
    (0x08684085, "CAMERAS", 0x0868408d, "cam", 0x5343414d),
    (0x08684091, "CLUSTERS", 0x0868409a, "cls", 0x434c5553),
    (0x0868409e, "PICTURES", 0x086840a7, "pic", 0x00000000),
    (0x086840ab, "LATTICES", 0x086840b4, "lat", 0x534c4154),
    (0x086840b8, "LIGHTS", 0x086840bf, "lig", 0x534c4947),
    (0x086840c3, "MATERIALS", 0x086840cd, "mtr", 0x534d4154),
    (0x086840d1, "MODELS", 0x086840d8, "hrc", 0x00000000),
    (0x086840dc, "NSELECTIONS", 0x086840e8, "", 0x00000000),
    (0x086840e9, "PALETTES", 0x086840f2, "pal", 0x00000000),
    (0x086840f6, "RENDER_PICTURES", 0x08684106, "pic", 0x00000000),
    (0x0868410a, "SETUP_SOFT", 0x08684115, "sts", 0x53534554),
    (0x08684119, "SETUP_CHANNELS", 0x08684128, "chn", 0x5343484e),
    (0x0868412c, "SETUP_FLOCK", 0x08684138, "flk", 0x53464c4b),
    (0x0868413c, "SHAPES", 0x08684143, "shp", 0x00000000),
    (0x08684147, "STRUCT_KEYS", 0x08684153, "key", 0x534b4559),
    (0x08684157, "TEXTURES2D", 0x08684162, "txt", 0x53543244),
    (0x08684166, "TEXTURES3D", 0x08684171, "t3D", 0x53543344),
    (0x08684175, "WAVES", 0x0868417b, "wav", 0x53574156),
    (0x0868417f, "EXPRESSIONS", 0x0868418b, "exp", 0x00000000),
    (0x0868418f, "MATERIAL_SHADERS", 0x086841a0, "shd", 0x53484154),
    (0x086841a4, "SHADOW_SHADERS", 0x086841b3, "shd", 0x53484144),
    (0x086841b7, "VOLUME_SHADERS", 0x086841c6, "shd", 0x5348564c),
    (0x086841ca, "TEXTURE2D_SHADERS", 0x086841dc, "shd", 0x53485432),
    (0x086841e0, "TEXTURE3D_SHADERS", 0x086841f2, "shd", 0x53485433),
    (0x086841f6, "LIGHT_SHADERS", 0x08684204, "shd", 0x53484947),
    (0x08684208, "CAMERA_SHADERS", 0x08684217, "shd", 0x5348414d),
    (0x0868421b, "OUTPUT_SHADERS", 0x0868422a, "shd", 0x53485554),
    (0x0868422e, "ENVIRON_SHADERS", 0x0868423e, "shd", 0x53484e56),
    (0x08684242, "CHANNEL_DRIVERS", 0x08684252, "", 0x00000000),
    (0x08684253, "OUTSIDE_DB_FILES", 0x08684264, "all", 0x00000000),
    (0x08684268, "SPREADSHEET_SETUP", 0x0868427a, "spd", 0x00000000),
    (0x0868427e, "AUDIO", 0x08684284, "aiff", 0x00000000),
    (0x08684289, "MATERIAL_PHOTON_SHADERS", 0x086842a1, "shd", 0x53484150),
    (0x086842a5, "LIGHT_PHOTON_SHADERS", 0x086842ba, "shd", 0x53484950),
];

fn put(m: &mut Mem, addr: u32, text: &str) {
    if addr >= RODATA.0 && addr - RODATA.0 < RODATA.1 {
        m.put_rom_cstr(addr, text.as_bytes());
    } else {
        m.put_cstr(addr, text.as_bytes());
    }
}

/// Stores the crate's constants in the image: the string literals and the chapter table
/// with its strings.
pub fn store(m: &mut Mem) {
    for &(addr, text) in STRINGS {
        put(m, addr, text);
    }
    for &(addr, text, ..) in ATT_FCTS {
        put(m, addr, text);
    }
    for (i, &(name, name_text, suffix, suffix_text, tag)) in CHAP_INFO.iter().enumerate() {
        put(m, name, name_text);
        put(m, suffix, suffix_text);
        let row = SI_CHAP_INFO.cast::<si_types::dict::ChapInfo>().add(i as i32);
        m[row].name = Ptr32::new(name);
        m[row].suffix = Ptr32::new(suffix);
        m[row].tag = tag;
    }
}
