// SPDX-License-Identifier: MIT
//! The global variables the readers use, at their addresses in the Linux binary, and the
//! constants they read from its data.
//!
//! Held against the binary by `python3 -I re/tools/si-mem/check_globals.py si-hrc`.
//! Nothing here reproduces a function of the original.

use si_mem::{globals, Mem};

globals! {
    /// The version of the file being read, as its header gave it (4.0 until a file was read).
    SI_VERSION_INPUT: f32 = 0x086793f0, "SI_versionInput", init 0x40800000;
    /// `b_matio.c`'s own copy of the version of the file being read.
    FILE_VERSION_MATIO: f32 = 0x08817f20, "fileVersion";
    /// `binary.c`'s copy. The user-data reader looks at this one, also when a material or
    /// texture file is being read, which sets only the other.
    FILE_VERSION: f32 = 0x08817f28, "fileVersion";
    /// The byte the swap routines move through.
    TMP: u8 = 0x08817f38, "tmp";
    /// Non-zero when files are read from a pipe instead of being opened.
    SI_PIPED_FILES: i16 = 0x086ceba8, "SI_pipedFiles";
    /// The stream to read from when [`SI_PIPED_FILES`] is set.
    SI_PIPE: u32 = 0x086cebac, "SI_pipe";
    /// The identity matrix (stored by [`store_data`]).
    SI_IDENTITE: [[f32; 4]; 4] = 0x086dc160, "SI_identite";
    /// The mapping method a new 2D texture gets.
    DEFAULT_MAP_METHOD: i16 = 0x086a4d40, "defaultMapMethod", init 1;
    /// The array of every shader reference alive, a slot being null when free.
    SI_UDF_LIST: u32 = 0x086e49d4, "si_udfList";
    /// The number of slots of [`SI_UDF_LIST`].
    SI_NUM_UDF: i32 = 0x086e49d8, "si_numUdf";
    /// The record of the running process; its second word is the index of the scene's
    /// entry in [`SI_GRAF_LISTS`].
    SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x08782108;
    /// What [`SI_PROC_DATA`] points to at start-up.
    SI_PROC_DATA_STORAGE: [i32; 3] = 0x08782108, "SI_procDataStorage";
    /// An array of 0x24-byte entries, one per scene held; at +0x14 of an entry the head of
    /// the list of that scene's materials (linked through a material's +0x6e4).
    SI_GRAF_LISTS: u32 = 0x08679498, "SI_grafLists";
    /// The node whose mesh is being rebuilt (`mesh_b.c`).
    CURR_MDL: u32 = 0x086d59c0, "currMdl";
    /// Set while a scene of a later product is being converted; the node reader then
    /// gives every node without an inverse kinematics chunk an empty one.
    SI_ED_SUMATRA_TO_3X: i16 = 0x08680e50, "SI_edSumatraTo3x";
    /// The running number of the nodes made (`count` of `SI_modelAllocate`).
    MODEL_COUNT: i32 = 0x086d6520, "count.228";
    /// The 3 × 3 identity matrix (stored by [`store_data`]).
    SI_MATRIX3_ID: [[f32; 3]; 3] = 0x086dc220, "SI_matrix3ID";
    /// The running number of the splines made (`count` of `SI_splineAllocate`).
    SPLINE_COUNT: i32 = 0x086d6960, "count.225";
    /// The version the writers write and build their prototype table for. Never written by
    /// the program (128 reads in the listing, no store): always 4.0.
    SI_VERSION_OUTPUT: f32 = 0x086793f4, "SI_versionOutput", init 0x40800000;
    /// Set when a saved model's geometry is to be written in global coordinates
    /// (`SI_omodelBinaryWrite` hands it to the node writer).
    SI_SAVE_CURR_GEOM: i16 = 0x086793e0, "SI_saveCurrGeom";
    /// Set when saved models are to be written as triangulated meshes (`evalScene -m`).
    SI_SAVE_TRI_MESH: i16 = 0x086793e2, "SI_saveTriMesh";
    /// Why the last save failed: 1 the file could not be opened, 2 a write failed.
    SI_SAVE_ELEM_ERROR_CODE: i32 = 0x086cebb0, "SI_saveElemErrorCode";
    /// The directory of the model files, or null (`si_filenameCleanup` takes it off a
    /// model's file name).
    SI_OBJECT_PATH: u32 = 0x086793b8, "SI_objectPath";
    /// The element dictionaries, 0x70 bytes per scene held.
    SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts";
    /// The chapter ids of the element dictionaries; entry 26 (+0x68) is the one
    /// `setT2dMapDomainID` looks a 2D texture up in.
    SI_CID: [i32; 36] = 0x08781660, "SI_cid";
}

/// `"NO_NAME"`, the picture name of a new 2D texture.
pub const S_NO_NAME: u32 = 0x086a4d42;
/// `"\tdone\n"`, the trace line at the end of the copy of a shader reference.
pub const S_DONE: u32 = 0x086e4c5d;
/// `"default"`, the name of a new material (copied as two words).
pub const S_DEFAULT: u32 = 0x086a06a0;

/// `"No comment..."`, the comment of a header written without one (two copies: one for
/// `SI_headerBinaryWrite`, one for `si_newheaderBinaryWrite`).
pub const S_NO_COMMENT_HEADER: u32 = 0x086cf1f2;
pub const S_NO_COMMENT_NEWHEADER: u32 = 0x086cf200;
/// `"PROT"`, written in front of the prototype table.
pub const S_PROT: u32 = 0x086cf640;
/// The tags the file writers put after the table, one copy per writer.
pub const S_HRCH_FD: u32 = 0x086ce6a0;
pub const S_HRCH_CURRENT: u32 = 0x086ce6af;
pub const S_HRCH_O: u32 = 0x086ce7ca;
pub const S_MTRL: u32 = 0x086ce52a;
pub const S_TXMP: u32 = 0x086ce543;
pub const S_TX3D: u32 = 0x086ce55c;
/// `"wb"`, the mode the file writers open with, one copy per writer.
pub const S_WB_MTR: u32 = 0x086ce527;
pub const S_WB_TXT: u32 = 0x086ce540;
pub const S_WB_T3D: u32 = 0x086ce559;
pub const S_WB_HRC_O: u32 = 0x086ce7c7;
pub const S_WB_HRC_CURRENT: u32 = 0x086ce6ac;
/// `"_UVDomainID_"`, the tag `setT2dMapDomainID` looks for, and its copy it duplicates.
pub const S_UV_DOMAIN_ID_FIND: u32 = 0x086ce7cf;
pub const S_UV_DOMAIN_ID_NEW: u32 = 0x086ce7dc;
/// What the node writer shows when a NURBS curve or surface could not be written.
pub const S_ERROR_NURBS_CURVE: u32 = 0x086ce860;
pub const S_ERROR_NURBS_SURFACE: u32 = 0x086ce8a0;

const WRITER_STRINGS: &[(u32, &[u8])] = &[
    (S_NO_COMMENT_HEADER, b"No comment..."),
    (S_NO_COMMENT_NEWHEADER, b"No comment..."),
    (S_PROT, b"PROT"),
    (S_HRCH_FD, b"HRCH"),
    (S_HRCH_CURRENT, b"HRCH"),
    (S_HRCH_O, b"HRCH"),
    (S_MTRL, b"MTRL"),
    (S_TXMP, b"TXMP"),
    (S_TX3D, b"TX3D"),
    (S_WB_MTR, b"wb"),
    (S_WB_TXT, b"wb"),
    (S_WB_T3D, b"wb"),
    (S_WB_HRC_O, b"wb"),
    (S_WB_HRC_CURRENT, b"wb"),
    (S_UV_DOMAIN_ID_FIND, b"_UVDomainID_"),
    (S_UV_DOMAIN_ID_NEW, b"_UVDomainID_"),
];

/// The identity matrix [`SI_IDENTITE`] and the strings of the binary's data that the
/// readers and writers hand to callees, write or compare with.
pub fn store_data(m: &mut Mem) {
    for i in 0..4 {
        m[SI_IDENTITE][i][i] = 1.0;
    }
    for i in 0..3 {
        m[SI_MATRIX3_ID][i][i] = 1.0;
    }
    for (at, text) in WRITER_STRINGS {
        m.put_cstr(*at, text);
    }
    // the two status-line messages of the NURBS writers: the original's own text, from the install
    for at in [S_ERROR_NURBS_CURVE, S_ERROR_NURBS_SURFACE] {
        m.put_cstr(at, si_rt::message(at));
    }
    m.put_cstr(S_NO_NAME, b"NO_NAME");
    m.put_cstr(S_DONE, b"\tdone\n");
    m.put_cstr(S_DEFAULT, b"default");
    for at in [super::matio::S_RB_MTR, super::matio::S_RB_TXT, super::matio::S_RB_T3D, super::shape::S_RB_SHP] {
        m.put_cstr(at, b"rb");
    }
}
