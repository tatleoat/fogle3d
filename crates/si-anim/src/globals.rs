// SPDX-License-Identifier: MIT
//! The global variables the three source files read, at their addresses in the binary.
//!
//! Held against the binary's symbol table and initial data by
//! `re/tools/si-mem/check_globals.py si-anim`.

use si_mem::Mem;

si_mem::globals! {
    /// The per-process record (`si-dict` owns it): its first word is the current
    /// dictionary's number.
    SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x08782108;
    /// The array of element dictionaries, 0x70 bytes each (`si-dict` owns it).
    SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts", init 0;
    /// The scene dictionary's chapter numbers (`si-dict` owns them): [0], [2] and [8] are
    /// read.
    SI_CID: [i32; 36] = 0x08781660, "SI_cid";
    /// The first frame of the scene (owned by the playback code).
    SI_FRAME_START: i32 = 0x08782050, "SI_frameStart";
    /// Frames per second (owned by the playback code).
    SI_FRAME_RATE: f32 = 0x0878204c, "SI_frameRate";
    /// The dope sheet's state (0x84 bytes; `dopeSh*.c`): the action list it shows is the
    /// word at +0x70, the item chosen in it the word at +0x6c.
    SI_DOPE_SH: [u32; 0x21] = 0x0878faa0, "SI_dopeSh";
    /// The animation settings (0x40 bytes; si-deform and si-app declare it too): the 16-bit
    /// word at +0x30 is copied into every new action (+0x2c).
    SI_GLB_ANIM: [u16; 0x20] = 0x08781480, "SI_glbAnim";
    /// The scene's list of curves shown in the function curve window (its word at +4 the
    /// first node; nodes of kind 9 at +0 name a curve at +4; linked through +0x1c).
    SI_SCNGL: u32 = 0x08695e9c, "SI_scngl", init 0;
    /// The audio tracks' record (0x9c bytes; the audio code owns it): the word at +0x7c is
    /// the track whose markers are selected.
    SI_AUDIO_CONTAINER: [u32; 0x27] = 0x087fb9c0, "SI_audioContainer";
    /// The current frame (si-model and the playback code declare it too).
    SI_FRAME_CURRENT: i32 = 0x08782040, "SI_frameCurrent";
    /// shape.c's text buffer (0x200 bytes): the weight curves' names are made in it.
    SHAPE_WORK_STR: [u8; 0x200] = 0x0878fb60, "workStr";

    // The static variables of the seven cluster interpolators of cluster.c, three each,
    // with the same use as those of the weighted shape routines below.
    /// `si_clusterMeshInterpolate`'s count.
    CMESH_NBWCLS: i32 = 0x08694b44, "nbwcls.312", init 0;
    /// `si_clusterMeshInterpolate`'s buffer.
    CMESH_WFCV: u32 = 0x08694b48, "wfcv.313", init 0;
    /// `si_clusterMeshInterpolate`'s list.
    CMESH_WFCV_SUBID: u32 = 0x08694b4c, "wfcvSubid.314", init 0;
    /// `si_clusterPatchInterpolate`'s count.
    CPATCH_NBWCLS: i32 = 0x08694b50, "nbwcls.318", init 0;
    /// `si_clusterPatchInterpolate`'s buffer.
    CPATCH_WFCV: u32 = 0x08694b54, "wfcv.319", init 0;
    /// `si_clusterPatchInterpolate`'s list.
    CPATCH_WFCV_SUBID: u32 = 0x08694b58, "wfcvSubid.320", init 0;
    /// `si_clusterNurbsFaceInterpolate`'s count.
    CNFACE_NBWCLS: i32 = 0x08694b5c, "nbwcls.324", init 0;
    /// `si_clusterNurbsFaceInterpolate`'s buffer.
    CNFACE_WFCV: u32 = 0x08694b60, "wfcv.325", init 0;
    /// `si_clusterNurbsFaceInterpolate`'s list.
    CNFACE_WFCV_SUBID: u32 = 0x08694b64, "wfcvSubid.326", init 0;
    /// `si_clusterFaceInterpolate`'s count.
    CFACE_NBWCLS: i32 = 0x08694b68, "nbwcls.330", init 0;
    /// `si_clusterFaceInterpolate`'s buffer.
    CFACE_WFCV: u32 = 0x08694b6c, "wfcv.331", init 0;
    /// `si_clusterFaceInterpolate`'s list.
    CFACE_WFCV_SUBID: u32 = 0x08694b70, "wfcvSubid.332", init 0;
    /// `si_clusterSplineInterpolate`'s count.
    CSPLINE_NBWCLS: i32 = 0x08694b74, "nbwcls.336", init 0;
    /// `si_clusterSplineInterpolate`'s buffer.
    CSPLINE_WFCV: u32 = 0x08694b78, "wfcv.337", init 0;
    /// `si_clusterSplineInterpolate`'s list.
    CSPLINE_WFCV_SUBID: u32 = 0x08694b7c, "wfcvSubid.338", init 0;
    /// `si_clusterNurbsCrvInterpolate`'s count.
    CNCRV_NBWCLS: i32 = 0x08694b80, "nbwcls.342", init 0;
    /// `si_clusterNurbsCrvInterpolate`'s buffer.
    CNCRV_WFCV: u32 = 0x08694b84, "wfcv.343", init 0;
    /// `si_clusterNurbsCrvInterpolate`'s list.
    CNCRV_WFCV_SUBID: u32 = 0x08694b88, "wfcvSubid.344", init 0;
    /// `si_clusterNurbsSrfInterpolate`'s count.
    CNSRF_NBWCLS: i32 = 0x08694b8c, "nbwcls.348", init 0;
    /// `si_clusterNurbsSrfInterpolate`'s buffer.
    CNSRF_WFCV: u32 = 0x08694b90, "wfcv.349", init 0;
    /// `si_clusterNurbsSrfInterpolate`'s list.
    CNSRF_WFCV_SUBID: u32 = 0x08694b94, "wfcvSubid.350", init 0;

    // The static variables of the six weighted shape routines of shape.c, three each: how
    // many curves expressions drive, the buffer `SI_getExprFcurveUsed` answers (handed back
    // to it on the next call) and the list of those curves (12-byte entries).
    /// `si_meshWeightInterpolate`'s count.
    MESH_NBWSHP: i32 = 0x08699720, "nbwshp.267", init 0;
    /// `si_meshWeightInterpolate`'s buffer.
    MESH_WFCV: u32 = 0x08699724, "wfcv.268", init 0;
    /// `si_meshWeightInterpolate`'s list.
    MESH_WFCV_SUBID: u32 = 0x08699728, "wfcvSubid.269", init 0;
    /// `si_patchWeightInterpolate`'s count.
    PATCH_NBWSHP: i32 = 0x0869972c, "nbwshp.273", init 0;
    /// `si_patchWeightInterpolate`'s buffer.
    PATCH_WFCV: u32 = 0x08699730, "wfcv.274", init 0;
    /// `si_patchWeightInterpolate`'s list.
    PATCH_WFCV_SUBID: u32 = 0x08699734, "wfcvSubid.275", init 0;
    /// `si_faceWeightInterpolate`'s count.
    FACE_NBWSHP: i32 = 0x08699738, "nbwshp.279", init 0;
    /// `si_faceWeightInterpolate`'s buffer.
    FACE_WFCV: u32 = 0x0869973c, "wfcv.280", init 0;
    /// `si_faceWeightInterpolate`'s list.
    FACE_WFCV_SUBID: u32 = 0x08699740, "wfcvSubid.281", init 0;
    /// `si_splineWeightInterpolate`'s count.
    SPLINE_NBWSHP: i32 = 0x08699744, "nbwshp.285", init 0;
    /// `si_splineWeightInterpolate`'s buffer.
    SPLINE_WFCV: u32 = 0x08699748, "wfcv.286", init 0;
    /// `si_splineWeightInterpolate`'s list.
    SPLINE_WFCV_SUBID: u32 = 0x0869974c, "wfcvSubid.287", init 0;
    /// `si_nurbsCrvWeightInterpolate`'s count.
    NCRV_NBWSHP: i32 = 0x08699750, "nbwshp.291", init 0;
    /// `si_nurbsCrvWeightInterpolate`'s buffer.
    NCRV_WFCV: u32 = 0x08699754, "wfcv.292", init 0;
    /// `si_nurbsCrvWeightInterpolate`'s list.
    NCRV_WFCV_SUBID: u32 = 0x08699758, "wfcvSubid.293", init 0;
    /// `si_nurbsSrfWeightInterpolate`'s count.
    NSRF_NBWSHP: i32 = 0x0869975c, "nbwshp.297", init 0;
    /// `si_nurbsSrfWeightInterpolate`'s buffer.
    NSRF_WFCV: u32 = 0x08699760, "wfcv.298", init 0;
    /// `si_nurbsSrfWeightInterpolate`'s list.
    NSRF_WFCV_SUBID: u32 = 0x08699764, "wfcvSubid.299", init 0;
}

/// Writes the initial values of this crate's globals.
pub fn init(m: &mut Mem) {
    si_mem::init_globals(m, ALL);
    crate::scene1::init(m);
    crate::edanim::init(m);
}

/// The current element dictionary's address: `SI_elemDicts + SI_procData->dict × 0x70`, as
/// the routines that hand it on compute it.
pub fn cur_dict(m: &Mem) -> u32 {
    let proc = m[SI_PROC_DATA];
    let d = m.u32(proc);
    m[SI_ELEM_DICTS].wrapping_add(d.wrapping_mul(0x70))
}
