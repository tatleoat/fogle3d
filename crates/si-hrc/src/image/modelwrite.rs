// SPDX-License-Identifier: MIT
//! The model writers of `binary_b.c` and `binary.c` on the image, with the chunk writers
//! of `invkin.c` and the cluster count of `klusters.c` they call (finding 0004,
//! "Hierarchy" and the records after it; this note's section on the writers).
//!
//! Like the material writers ([`super::matwrite`]), each is a fixed sequence of fields in
//! the order of the list `SI_prototypeBinaryWrite` writes for its kind, with that list's
//! version tests repeated inside ([`super::wio`]; with `SI_versionOutput` at 4.0 all pass).
//! A field goes out with a swapping write straight from its record, or through a local
//! of the routine's frame when the writer changes it on the way (a node's type in a
//! triangulated save, its animation algorithm), counts something (a list's length) or
//! sums lengths (the user data, the projected curves of a NURBS surface).
//!
//! What the writers call in other source files goes through [`Outside`], each routine
//! written `name (0xADDRESS)` where it is declared: the conversions to meshes and the
//! triangulation of a save with `SI_saveTriMesh`, the copies and transformations of a
//! save in global coordinates, the custom effects' byte order conversions, the check of
//! a metasystem's groups and the look-up of a texture in the element dictionary.
//! `SI_splineCopy`, `SI_splineDispose` and `SI_nurbsCrvDispose` are on the image (the
//! readers' modules) and called directly.
//!
//! Left out, with the project's standing reason: the counter `SI_secureLicAlloc`
//! (0x08679660) that `setT2dMapDomainID` and `userDataArrayWrite` bump before an
//! allocation (licence code).

use super::chunks::{dyna, envelop, invkin, kluster, more};
use super::geom::{edge_at, mesh};
use super::globals::{SI_CID, SI_ELEM_DICTS, SI_OBJECT_PATH, SI_PIPE, SI_PIPED_FILES, SI_PROC_DATA, SI_SAVE_CURR_GEOM, SI_SAVE_ELEM_ERROR_CODE, SI_SAVE_TRI_MESH, S_ERROR_NURBS_CURVE, S_ERROR_NURBS_SURFACE, S_HRCH_CURRENT, S_HRCH_FD, S_HRCH_O, S_UV_DOMAIN_ID_NEW, S_WB_HRC_CURRENT, S_WB_HRC_O};
use super::matwrite::{material_binary_write, texture3d_binary_write, texture_binary_write};
use super::meta::{ball, system};
use super::node::{fix_custom, node};
use super::nurbs::{comp, crv, nurbs_crv_dispose, srf};
use super::outside::{Outside, Routine};
use super::patch::patch;
use super::proto::prototype_binary_write;
use super::spline::{face, spline, spline_copy, spline_dispose};
use super::texture::lclmap;
use super::wio::{byteswap_fwrite, header_binary_write, matrix_fwrite, out_above, out_above_or_nan, out_below, out_from, string_binary_write, three_doubles_fwrite, vector_fwrite};
use si_mem::Mem;

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine the writers reach through [`Outside::call`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

routines! {
    /// `SI_cusAddUserData (0x08535f30)`, `custom_ud.c`: a custom-effect node's data put
    /// into its user data before it is saved.
    SI_CUS_ADD_USER_DATA = "SI_cusAddUserData", 0x08535f30;
    /// `SI_fix_SI_CustomHeaderOld (0x083f966c)`, `byteswap.c`, and the three below: the
    /// byte order of custom-effect data, swapped before the data is written (second
    /// argument 0) and back after (1).
    SI_FIX_CUSTOM_HEADER_OLD = "SI_fix_SI_CustomHeaderOld", 0x083f966c;
    SI_FIX_CUSTOM_HEADER = "SI_fix_SI_CustomHeader", 0x083f96e4;
    SI_FIX_CUSTOM_DSO_FCURVES = "SI_fix_CustomDsoFcurves", 0x083f9730;
    SI_FIX_CUSTOM_FCURVES = "SI_fix_CustomFcurves", 0x083f9810;
    /// `SI_matrixInverse (0x0851672c)`, `algebra.c`.
    SI_MATRIX_INVERSE = "SI_matrixInverse", 0x0851672c;
    /// `SI_matrixMultiply (0x08516244)`, `algebra.c`.
    SI_MATRIX_MULTIPLY = "SI_matrixMultiply", 0x08516244;
    /// `SI_matrixTransposeInverse (0x08518a3c)`, `algebra.c`.
    SI_MATRIX_TRANSPOSE_INVERSE = "SI_matrixTransposeInverse", 0x08518a3c;
    /// `SI_patchCopy (0x0847a2b8)`, `SI_patchTransform (0x084866a4)`, `SI_patchDispose
    /// (0x0847a530)`, `patch.c`, `transform.c`.
    SI_PATCH_COPY = "SI_patchCopy", 0x0847a2b8;
    SI_PATCH_TRANSFORM = "SI_patchTransform", 0x084866a4;
    SI_PATCH_DISPOSE = "SI_patchDispose", 0x0847a530;
    /// `SI_faceCopy (0x08434990)`, `SI_faceTransform (0x08436c88)`, `SI_faceDispose
    /// (0x08434828)`, `face.c`.
    SI_FACE_COPY = "SI_faceCopy", 0x08434990;
    SI_FACE_TRANSFORM = "SI_faceTransform", 0x08436c88;
    SI_FACE_DISPOSE = "SI_faceDispose", 0x08434828;
    /// `SI_meshCopy (0x084404e0)`, `SI_meshTransform (0x08441cd8)`, `SI_meshDispose
    /// (0x084404a0)`, `mesh.c`.
    SI_MESH_COPY = "SI_meshCopy", 0x084404e0;
    SI_MESH_TRANSFORM = "SI_meshTransform", 0x08441cd8;
    SI_MESH_DISPOSE = "SI_meshDispose", 0x084404a0;
    /// `SI_splineTransform (0x08494138)`, `spline.c`.
    SI_SPLINE_TRANSFORM = "SI_splineTransform", 0x08494138;
    /// `SI_nurbsCrvCopy (0x084dbbf4)`, `SI_nurbsCrvApplyMatrix (0x084e9750)`, `nurbs.c`.
    SI_NURBS_CRV_COPY = "SI_nurbsCrvCopy", 0x084dbbf4;
    SI_NURBS_CRV_APPLY_MATRIX = "SI_nurbsCrvApplyMatrix", 0x084e9750;
    /// `SI_nurbsSrfCopy (0x084dc1a8)`, `SI_nurbsSrfApplyMatrix (0x084e9644)`,
    /// `SI_nurbsSrfDispose (0x084dba5c)`, `nurbs.c`.
    SI_NURBS_SRF_COPY = "SI_nurbsSrfCopy", 0x084dc1a8;
    SI_NURBS_SRF_APPLY_MATRIX = "SI_nurbsSrfApplyMatrix", 0x084e9644;
    SI_NURBS_SRF_DISPOSE = "SI_nurbsSrfDispose", 0x084dba5c;
    /// `SI_patchConvertToMesh (0x0847b69c)`, `SI_faceConvertToMesh (0x08435960)`,
    /// `SI_meta2Mesh (0x0826bf7c)`, `SI_nurbsConvertToMesh (0x084df320)`: a node's shape
    /// as a polygon mesh, for a triangulated save.
    SI_PATCH_CONVERT_TO_MESH = "SI_patchConvertToMesh", 0x0847b69c;
    SI_FACE_CONVERT_TO_MESH = "SI_faceConvertToMesh", 0x08435960;
    SI_META2_MESH = "SI_meta2Mesh", 0x0826bf7c;
    SI_NURBS_CONVERT_TO_MESH = "SI_nurbsConvertToMesh", 0x084df320;
    /// `SI_meshTriangulate (0x08443144)`, `mesh.c`.
    SI_MESH_TRIANGULATE = "SI_meshTriangulate", 0x08443144;
    /// `SI_statusDisplayMessage (0x083cca94)`, `status.c`.
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    /// `SI_metaSystemValidateGroups (0x0845e3b4)`, `metaball.c`: marks which primitives of
    /// a group are still metaballs under the system (the short at +0x10 of a primitive).
    SI_META_SYSTEM_VALIDATE_GROUPS = "SI_metaSystemValidateGroups", 0x0845e3b4;
    /// `SI_elemdictGetElem (0x08132b50)`, `elemdict.c`: an element of the scene's
    /// dictionary by chapter and number.
    SI_ELEMDICT_GET_ELEM = "SI_elemdictGetElem", 0x08132b50;
}

/// The length of the string at `p` (`strlen`, inlined in the original).
fn strlen(m: &Mem, p: u32) -> u32 {
    m.cstr(p).len() as u32
}

/// `strcmp(a, b) == 0` with `b` a literal (the original's inlined `repz cmpsb` over the
/// literal and its zero).
fn is(m: &Mem, a: u32, b: &[u8]) -> bool {
    m.cstr(a) == b
}

/// Whether a user-data tag is one the writers never write (`CUSELEMCB`, `CUSNEXTELEMCB`).
fn is_element_link(m: &Mem, tag: u32) -> bool {
    is(m, tag, b"CUSELEMCB") || is(m, tag, b"CUSNEXTELEMCB")
}

/// Whether a tag of 4 bytes is one of the custom effects' (`CUSA` to `CUSD`).
fn is_custom(m: &Mem, tag: u32) -> bool {
    strlen(m, tag) == 4 && [&b"CUSA"[..], b"CUSB", b"CUSC", b"CUSD"].iter().any(|t| is(m, tag, t))
}

/// A null tag: the original takes its length (or its first byte) and dies on the access.
/// The port records the fault at address 0 and treats the block as not written.
fn null_tag(m: &mut Mem) {
    m.record_fault(0, 1, si_mem::Access::Read);
}

/// si_userDataSize (0x083f3780); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): the length of a user-data list in a file: 4 for the
/// count, and for each block written its tag's length, its data's size and 10; `*count`
/// is set to the number of blocks written. Never written: blocks tagged `CUSELEMCB` or
/// `CUSNEXTELEMCB`, and with `mode` 1 the custom effects' blocks `CUSA` to `CUSD`.
pub fn user_data_size(m: &mut Mem, list: u32, mode: i16, count: u32) -> u32 {
    let mut size = 4u32;
    m.set_u32(count, 0);
    let mut p = list;
    while p != 0 {
        let tag = m.u32(p);
        if tag == 0 {
            null_tag(m);
        } else if !(mode == 1 && is_custom(m, tag)) && !is_element_link(m, tag) {
            size = size.wrapping_add(strlen(m, tag)).wrapping_add(m.u32(p + 8)).wrapping_add(10);
            let n = m.u32(count).wrapping_add(1);
            m.set_u32(count, n);
        }
        p = m.u32(p + 0x10);
    }
    size
}

/// si_userDataWrite (0x083f3940); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): the blocks of a list that [`user_data_size`]
/// counts, each as its tag's length (an int through a local), the tag's bytes, the flag
/// (short), the size (int) and the data's bytes.
pub fn user_data_write(m: &mut Mem, o: &mut dyn Outside, file: u32, list: u32, mode: i16) {
    const FRAME: u32 = 4;
    let local = m.local(FRAME);
    let mut p = list;
    while p != 0 {
        let tag = m.u32(p);
        if tag == 0 {
            null_tag(m);
        } else if !(mode != 0 && is_custom(m, tag)) && !is_element_link(m, tag) {
            let len = strlen(m, tag);
            m.set_u32(local, len);
            byteswap_fwrite(m, o, local, 4, 1, file);
            byteswap_fwrite(m, o, tag, 1, len, file);
            byteswap_fwrite(m, o, p + 4, 2, 1, file);
            byteswap_fwrite(m, o, p + 8, 4, 1, file);
            let (data, size) = (m.u32(p + 0xc), m.u32(p + 8));
            byteswap_fwrite(m, o, data, 1, size, file);
        }
        p = m.u32(p + 0x10);
    }
    m.release(FRAME);
}

/// userDataClusterSize (0x083f3904); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): the length of a cluster of user data in a node
/// token (0xff1d, 0xff1f, 0xff21): 0 without a cluster, else 9 and 4 bytes per entry, 8
/// for the vertex cluster (`vertex` 1) of a face.
pub fn user_data_cluster_size(m: &Mem, model: u32, cluster: u32, vertex: i16) -> u32 {
    if cluster == 0 {
        return 0;
    }
    let n = m.u32(cluster + kluster::COUNT);
    let bytes = if vertex == 1 && m.i32(model + node::TYPE) == 2 { n << 3 } else { n << 2 };
    bytes.wrapping_add(9)
}

/// userDataArrayWrite (0x083f3b0c); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): the user data on components (tokens 0xff1e,
/// 0xff20, 0xff22): the length (4, or 4 plus each component's list's length, the lengths
/// kept in a zeroed array of one int per component, `SI_calloc`), then with a cluster and
/// an array the cluster's count and for each component its list's block count (from the
/// array) and blocks; the array is freed. Without both only the length 0 is written.
pub fn user_data_array_write(m: &mut Mem, o: &mut dyn Outside, file: u32, cluster: u32, arrays: u32) {
    // the frame: the length, the count, the array pointer
    const FRAME: u32 = 12;
    let frame = m.local(FRAME);
    let (length_at, count_at) = (frame, frame + 4);
    let mut lengths = 0u32;
    m.set_u32(length_at, 0);
    let both = cluster != 0 && arrays != 0;
    if both {
        m.set_u32(length_at, 4);
        let n = m.u32(cluster + kluster::COUNT);
        lengths = m.si_calloc(n, 4);
        if lengths != 0 {
            let mut i = 0i32;
            while i < m.i32(cluster + kluster::COUNT) {
                let list = m.u32(arrays.wrapping_add(4 * i as u32));
                let size = user_data_size(m, list, 0, lengths.wrapping_add(4 * i as u32));
                let total = m.u32(length_at).wrapping_add(size);
                m.set_u32(length_at, total);
                i += 1;
            }
        }
    }
    byteswap_fwrite(m, o, length_at, 4, 1, file);
    if both {
        let n = m.u32(cluster + kluster::COUNT);
        m.set_u32(count_at, n);
        byteswap_fwrite(m, o, count_at, 4, 1, file);
        let mut i = 0i32;
        while i < m.i32(cluster + kluster::COUNT) {
            // the original walks the array whether or not the allocation succeeded
            byteswap_fwrite(m, o, lengths.wrapping_add(4 * i as u32), 4, 1, file);
            let list = m.u32(arrays.wrapping_add(4 * i as u32));
            user_data_write(m, o, file, list, 0);
            i += 1;
        }
        if lengths != 0 {
            m.free(lengths);
        }
    }
    m.release(FRAME);
}

/// [soft 0x083f5e4c clusterWriteBinaryChunk]: a cluster: its name, its count, its entries
/// (twice the count when `flag` is 1 and the node is a face) and its state.
pub fn cluster_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, cluster: u32, flag: i16) {
    let name = m.u32(cluster + kluster::NAME);
    string_binary_write(m, o, file, name);
    byteswap_fwrite(m, o, cluster + kluster::COUNT, 4, 1, file);
    let mut n = m.i32(cluster + kluster::COUNT);
    if flag == 1 && m.i32(model + node::TYPE) == 2 {
        n = n.wrapping_mul(2);
    }
    let mut i = 0i32;
    while i < n {
        let at = m.u32(cluster + kluster::INDICES).wrapping_add(4 * i as u32);
        byteswap_fwrite(m, o, at, 4, 1, file);
        i += 1;
    }
    byteswap_fwrite(m, o, cluster + kluster::STATE, 4, 1, file);
}

/// [soft 0x083f5edc clustersWriteBinaryChunk]: every cluster of the node's list.
pub fn clusters_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32) {
    let mut c = m.u32(model + more::CLUSTERS);
    while c != 0 {
        cluster_write_binary_chunk(m, o, file, model, c, 1);
        c = m.u32(c + kluster::NEXT);
    }
}

/// [soft 0x0843d8f0 SI_clusterCount]: the length of the node's cluster list.
pub fn cluster_count(m: &Mem, model: u32) -> i32 {
    let mut n = 0i32;
    let mut c = m.u32(model + more::CLUSTERS);
    while c != 0 {
        n = n.wrapping_add(1);
        c = m.u32(c + kluster::NEXT);
    }
    n
}

/// [soft 0x083f5da0 dynamatterWriteBinaryChunk]: density, roughness, elasticity,
/// rigidity, plasticity, kinetic roughness, and the collision level through a local.
pub fn dynamatter_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32) {
    const FRAME: u32 = 4;
    let local = m.local(FRAME);
    for off in [dyna::DENSITY, dyna::ROUGHNESS, dyna::ELASTICITY, dyna::RIGIDITY, dyna::PLASTICITY, dyna::KINETIC_ROUGHNESS] {
        byteswap_fwrite(m, o, p + off, 4, 1, file);
    }
    let v = m.u32(p + dyna::COLLISION);
    m.set_u32(local, v);
    byteswap_fwrite(m, o, local, 4, 1, file);
    m.release(FRAME);
}

/// [soft 0x083f5e28 dynajntpropWriteBinaryChunk]: the joint's friction vector.
pub fn dynajntprop_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32) {
    let v = m.u32(model + more::DYNA_JOINT);
    vector_fwrite(m, o, v, 0xc, 1, file);
}

/// [soft 0x084aea20 SI_invKinWriteBinaryChunk]: an inverse kinematics chunk: type,
/// number of joints, the rotation vector; nothing for a null one.
pub fn inv_kin_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, ik: u32) {
    if ik == 0 {
        return;
    }
    byteswap_fwrite(m, o, ik + invkin::TYPE, 4, 1, file);
    byteswap_fwrite(m, o, ik + invkin::NB_JOINTS, 4, 1, file);
    vector_fwrite(m, o, ik + invkin::ROTATION, 0xc, 1, file);
}

/// [soft 0x083f5f18 envelopVertWriteBinaryChunk]: index, ratio, thickness, weight.
pub fn envelop_vert_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, v: u32) {
    for off in [envelop::VERT_INDEX, envelop::VERT_RATIO, envelop::VERT_THICKNESS, envelop::VERT_WEIGHT] {
        byteswap_fwrite(m, o, v + off, 4, 1, file);
    }
}

/// [soft 0x083f5f68 envelopWriteBinaryChunk]: an envelope: level, joint area, the inner
/// curve's type and four floats, the outer curve's; each curve's samples as a short
/// count (0 without an array, else always 200) and the floats; the number of vertex
/// lists (a short), and per list the number of its vertices (an int) and the vertices.
/// Nothing for a null envelope.
pub fn envelop_write_binary_chunk(m: &mut Mem, o: &mut dyn Outside, file: u32, e: u32) {
    if e == 0 {
        return;
    }
    // the frame: the short count, the int count
    const FRAME: u32 = 8;
    let frame = m.local(FRAME);
    let (short_at, int_at) = (frame, frame + 4);
    byteswap_fwrite(m, o, e + envelop::LEVEL, 4, 1, file);
    byteswap_fwrite(m, o, e + envelop::JOINT_AREA, 4, 1, file);
    byteswap_fwrite(m, o, e + envelop::INNER_TYPE, 4, 1, file);
    for k in 0..4 {
        byteswap_fwrite(m, o, e + envelop::INNER + 4 * k, 4, 1, file);
    }
    byteswap_fwrite(m, o, e + envelop::OUTER_TYPE, 4, 1, file);
    for k in 0..4 {
        byteswap_fwrite(m, o, e + envelop::OUTER + 4 * k, 4, 1, file);
    }
    for curve in [envelop::INNER_CURVE, envelop::OUTER_CURVE] {
        m.set_i16(short_at, 0);
        let samples = m.u32(e + curve);
        if samples == 0 {
            byteswap_fwrite(m, o, short_at, 2, 1, file);
        } else {
            m.set_i16(short_at, 200);
            byteswap_fwrite(m, o, short_at, 2, 1, file);
            let mut k: i16 = 0;
            while k < m.i16(short_at) {
                let at = m.u32(e + curve).wrapping_add(4 * k as i32 as u32);
                byteswap_fwrite(m, o, at, 4, 1, file);
                k = k.wrapping_add(1);
            }
        }
    }
    let mut n: i16 = 0;
    let mut list = m.u32(e + envelop::LISTS);
    while list != 0 {
        n = n.wrapping_add(1);
        list = m.u32(list + envelop::LIST_NEXT);
    }
    m.set_i16(short_at, n);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    let mut list = m.u32(e + envelop::LISTS);
    let mut done: i16 = 0;
    while done < m.i16(short_at) {
        done = done.wrapping_add(1);
        let mut count = 0i32;
        let mut v = m.u32(list + envelop::LIST_FIRST);
        while v != 0 {
            count = count.wrapping_add(1);
            v = m.u32(v + envelop::VERT_NEXT);
        }
        m.set_i32(int_at, count);
        byteswap_fwrite(m, o, int_at, 4, 1, file);
        let mut v = m.u32(list + envelop::LIST_FIRST);
        let mut k = 0i32;
        while k < m.i32(int_at) {
            envelop_vert_write_binary_chunk(m, o, file, v);
            v = m.u32(v + envelop::VERT_NEXT);
            k += 1;
        }
        list = m.u32(list + envelop::LIST_NEXT);
    }
    m.release(FRAME);
}

/// [soft 0x083f59e0 splineBinaryWrite]: a spline: name, type (its low short through a
/// local), the number of keys (a short) and the keys (three vectors, three shorts; the
/// loop counts the signed short down), closed flag, tension, step, phantom flag. Answers 1.
pub fn spline_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, s: u32) -> i32 {
    const FRAME: u32 = 4;
    let local = m.local(FRAME);
    let name = m.u32(s + spline::NAME);
    string_binary_write(m, o, file, name);
    let t = m.u16(s + spline::TYPE);
    m.set_u16(local, t);
    byteswap_fwrite(m, o, local, 2, 1, file);
    byteswap_fwrite(m, o, s + spline::NB_KEYS, 2, 1, file);
    let mut key = m.u32(s + spline::KEYS);
    let mut left = m.i16(s + spline::NB_KEYS);
    while 0 < left {
        vector_fwrite(m, o, key, 0xc, 1, file);
        vector_fwrite(m, o, key + spline::KEY_POSITION, 0xc, 1, file);
        vector_fwrite(m, o, key + spline::KEY_NEXT, 0xc, 1, file);
        byteswap_fwrite(m, o, key + spline::KEY_CONSTRAINT, 2, 1, file);
        byteswap_fwrite(m, o, key + spline::KEY_SEGMENT, 2, 1, file);
        byteswap_fwrite(m, o, key + spline::KEY_FLAG, 2, 1, file);
        key = key.wrapping_add(spline::KEY);
        left = left.wrapping_sub(1);
    }
    byteswap_fwrite(m, o, s + spline::CLOSED, 2, 1, file);
    byteswap_fwrite(m, o, s + spline::TENSION, 4, 1, file);
    byteswap_fwrite(m, o, s + spline::STEP, 2, 1, file);
    byteswap_fwrite(m, o, s + spline::PHANTOM, 2, 1, file);
    m.release(FRAME);
    1
}

/// [soft 0x083f58fc faceBinaryWrite]: a face: the "made of splines" flag; for a face of
/// NURBS curves the count and the curves (an array of curve records, 100 bytes apart);
/// otherwise the outline spline (inline in the face), the count and the holes (an array
/// of spline records). The count is a signed short. Answers 1.
pub fn face_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, f: u32) -> i32 {
    const FRAME: u32 = 4;
    let total = m.local(FRAME);
    m.set_u32(total, 0);
    byteswap_fwrite(m, o, f + face::IS_SPLINE, 2, 1, file);
    if m.i16(f + face::IS_SPLINE) == 0 {
        byteswap_fwrite(m, o, f + face::NB_HOLES, 2, 1, file);
        let mut k: i16 = 0;
        while k < m.i16(f + face::NB_HOLES) {
            let c = m.u32(f + face::CURVES).wrapping_add((k as i32 as u32).wrapping_mul(crv::SIZE));
            nurbs_crv_binary_write(m, o, file, c, 0, total);
            k = k.wrapping_add(1);
        }
    } else {
        spline_binary_write(m, o, file, f + face::OUTLINE);
        byteswap_fwrite(m, o, f + face::NB_HOLES, 2, 1, file);
        let mut k: i16 = 0;
        while k < m.i16(f + face::NB_HOLES) {
            let s = m.u32(f + face::HOLES).wrapping_add((k as i32 as u32).wrapping_mul(spline::SIZE));
            spline_binary_write(m, o, file, s);
            k = k.wrapping_add(1);
        }
    }
    m.release(FRAME);
    1
}

/// [soft 0x083f5b00 patchBinaryWrite]: a patch: the types in U and V (their low shorts
/// through a local), the point counts (shorts) and the points (vectors 16 bytes apart),
/// the closed flags, tensions, steps, curves, capping flags and faceted flag; above 2.65
/// the approximation (method, view, three floats, three shorts) and the tags (one short
/// per point). Answers 1.
pub fn patch_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32) -> i32 {
    const FRAME: u32 = 4;
    let local = m.local(FRAME);
    for off in [patch::TYPE_U, patch::TYPE_V] {
        let v = m.u16(p + off);
        m.set_u16(local, v);
        byteswap_fwrite(m, o, local, 2, 1, file);
    }
    byteswap_fwrite(m, o, p + patch::NB_U, 2, 1, file);
    byteswap_fwrite(m, o, p + patch::NB_V, 2, 1, file);
    let points = |m: &Mem| (m.u16(p + patch::NB_U) as i32).wrapping_mul(m.u16(p + patch::NB_V) as i32);
    let mut at = m.u32(p + patch::POINTS);
    let mut left = points(m);
    while 0 < left {
        vector_fwrite(m, o, at, 0xc, 1, file);
        at = at.wrapping_add(0x10);
        left -= 1;
    }
    for (off, size) in [(patch::CLOSED_U, 2), (patch::CLOSED_V, 2), (patch::TENSION_U, 4), (patch::TENSION_V, 4), (patch::STEP_U, 2), (patch::STEP_V, 2), (patch::CURVES_U, 2), (patch::CURVES_V, 2)] {
        byteswap_fwrite(m, o, p + off, size, 1, file);
    }
    for k in 0..5 {
        byteswap_fwrite(m, o, p + patch::CAPS + 2 * k, 2, 1, file);
    }
    if out_above_or_nan(m, 2.65) {
        byteswap_fwrite(m, o, p + patch::APPROX_METHOD, 2, 1, file);
        byteswap_fwrite(m, o, p + patch::APPROX_VIEW, 2, 1, file);
        for k in 0..3 {
            byteswap_fwrite(m, o, p + patch::APPROX_FLOATS + 4 * k, 4, 1, file);
        }
        for off in [patch::APPROX_MIN, patch::APPROX_MAX, patch::APPROX_RECURSION] {
            byteswap_fwrite(m, o, p + off, 2, 1, file);
        }
        let mut at = m.u32(p + patch::TAGS);
        let mut left = points(m);
        while 0 < left {
            byteswap_fwrite(m, o, at, 2, 1, file);
            at = at.wrapping_add(2);
            left -= 1;
        }
    }
    m.release(FRAME);
    1
}

/// [soft 0x083f54f4 meshBinaryWrite]: a polygon mesh, for the node `model`: flags,
/// angle, from 3.93 the node's number of texture coordinate sets; the vertices (count,
/// then position and flag each); the polygons (count, then per polygon the number of
/// corners through a local short, each corner's vertex, normal, texture pair and colour
/// and from 3.93 the length and pairs of the further sets, then the material index and
/// above 2.65 the flag); the edges numbered from 1 (count, then two vertices and the
/// flag each); from 3.8109 the shorts and floats 0x20a to 0x212. Answers 1.
pub fn mesh_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32, model: u32) -> i32 {
    // the frame: the corner count, the further sets' length
    const FRAME: u32 = 8;
    let frame = m.local(FRAME);
    let (short_at, length_at) = (frame, frame + 4);
    byteswap_fwrite(m, o, p + mesh::FLAGS, 2, 1, file);
    byteswap_fwrite(m, o, p + mesh::ANGLE, 4, 1, file);
    if out_from(m, 3.93) {
        byteswap_fwrite(m, o, model + node::NB_UV_SETS, 4, 1, file);
    }
    byteswap_fwrite(m, o, p + mesh::NB_VERTICES, 4, 1, file);
    let mut v = m.u32(p + mesh::VERTICES);
    let mut left = m.i32(p + mesh::NB_VERTICES);
    while 0 < left {
        vector_fwrite(m, o, v, 0xc, 1, file);
        byteswap_fwrite(m, o, v + mesh::VERTEX_FLAG, 2, 1, file);
        v = v.wrapping_add(mesh::VERTEX);
        left -= 1;
    }
    byteswap_fwrite(m, o, p + mesh::NB_POLYGONS, 4, 1, file);
    let mut left = m.i32(p + mesh::NB_POLYGONS);
    let mut poly = m.u32(p + mesh::POLYGONS);
    while 0 < left {
        let n = m.u16(poly + mesh::POLYGON_NB_NODES);
        m.set_u16(short_at, n);
        byteswap_fwrite(m, o, short_at, 2, 1, file);
        left -= 1;
        if m.i16(poly + mesh::POLYGON_NB_NODES) != 0 {
            let mut k = 0i32;
            loop {
                let corner = m.u32(poly + mesh::POLYGON_NODES).wrapping_add((k as u32).wrapping_mul(mesh::NODE));
                byteswap_fwrite(m, o, corner, 4, 1, file);
                vector_fwrite(m, o, corner + mesh::NODE_NORMAL, 0xc, 1, file);
                byteswap_fwrite(m, o, corner + mesh::NODE_UV, 4, 1, file);
                byteswap_fwrite(m, o, corner + mesh::NODE_UV + 4, 4, 1, file);
                byteswap_fwrite(m, o, corner + mesh::NODE_COLOUR, 4, 1, file);
                k += 1;
                if out_from(m, 3.93) {
                    let sets = m.i32(model + node::NB_UV_SETS).wrapping_sub(1);
                    m.set_i32(length_at, sets.wrapping_mul(8));
                    byteswap_fwrite(m, o, length_at, 4, 1, file);
                    let mut s = 0i32;
                    while s < m.i32(model + node::NB_UV_SETS).wrapping_sub(1) {
                        let pairs = m.u32(corner + mesh::NODE_MORE_UV);
                        byteswap_fwrite(m, o, pairs.wrapping_add(8 * s as u32), 4, 1, file);
                        byteswap_fwrite(m, o, pairs.wrapping_add(8 * s as u32 + 4), 4, 1, file);
                        s += 1;
                    }
                }
                if k >= m.u16(poly + mesh::POLYGON_NB_NODES) as i32 {
                    break;
                }
            }
        }
        byteswap_fwrite(m, o, poly + mesh::POLYGON_MATERIAL, 2, 1, file);
        if out_above(m, 2.65) {
            byteswap_fwrite(m, o, poly + mesh::POLYGON_FLAG, 2, 1, file);
        }
        poly = poly.wrapping_add(mesh::POLYGON);
    }
    byteswap_fwrite(m, o, p + mesh::NB_EDGES, 4, 1, file);
    let mut k = 1i32;
    while 0 < m.i32(p + mesh::NB_EDGES) && k <= m.i32(p + mesh::NB_EDGES) {
        let e = edge_at(m, m.u32(p + mesh::EDGES), k);
        byteswap_fwrite(m, o, e, 4, 1, file);
        byteswap_fwrite(m, o, e + 4, 4, 1, file);
        byteswap_fwrite(m, o, e + mesh::EDGE_FLAG, 2, 1, file);
        k += 1;
    }
    if out_from(m, 3.8109) {
        for (off, size) in [(0x52, 2), (0x50, 2), (0x38, 2), (0x3c, 4), (0x40, 4), (0x44, 4), (0x48, 2), (0x4a, 2), (0x4c, 2)] {
            byteswap_fwrite(m, o, p + off, size, 1, file);
        }
    }
    m.release(FRAME);
    1
}

/// nurbsCrvBinaryWrite (0x083f6d0c); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): a NURBS curve, or with `count` set only its
/// length added to `*total` (the counting pass of a surface's projected curves): order,
/// number of points, the points (four doubles copied to the stack and written swapped
/// there, then the int flag), closed flag, uniformity, number of knots and the knots,
/// start, end, display step, closing, key-framed flag. A write that does not answer its
/// count of items makes it stop and answer 0; the length is added either way. Answers 1.
pub fn nurbs_crv_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, c: u32, count: i16, total: u32) -> i16 {
    const FRAME: u32 = 0x20;
    let local = m.local(FRAME);
    let mut ok: i16 = 1;
    let mut len = 0i32;
    // one field: written and checked, or counted
    let field = |m: &mut Mem, o: &mut dyn Outside, at: u32, size: u32, n: u32, ok: &mut i16, len: &mut i32| {
        if count == 0 {
            if byteswap_fwrite(m, o, at, size, n, file) != n {
                *ok = 0;
            }
        } else {
            *len = len.wrapping_add(size.wrapping_mul(n) as i32);
        }
    };
    let done = |m: &mut Mem, len: i32| {
        let t = m.i32(total).wrapping_add(len);
        m.set_i32(total, t);
    };
    'out: {
        field(m, o, c + crv::ORDER, 4, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        field(m, o, c + crv::NB_POINTS, 4, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        let mut i = 0i32;
        while i < m.i32(c + crv::NB_POINTS) {
            let point = m.u32(c + crv::POINTS).wrapping_add((i as u32).wrapping_mul(crv::POINT));
            for k in 0..4 {
                let v = m.u64(point + 8 * k);
                m.set_u64(local + 8 * k, v);
            }
            field(m, o, local, 8, 4, &mut ok, &mut len);
            if ok == 0 {
                break 'out;
            }
            field(m, o, point + crv::POINT_FLAG, 4, 1, &mut ok, &mut len);
            if ok == 0 {
                break 'out;
            }
            i += 1;
        }
        field(m, o, c + crv::CLOSED, 2, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        field(m, o, c + crv::UNIFORMITY, 4, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        field(m, o, c + crv::NB_KNOTS, 4, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        let knots = m.u32(c + crv::NB_KNOTS);
        field(m, o, m.u32(c + crv::KNOTS), 8, knots, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        field(m, o, c + crv::START, 8, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        field(m, o, c + crv::END, 8, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        // the step is written (or counted) whatever the end's write answered
        field(m, o, c + crv::STEP, 4, 1, &mut ok, &mut len);
        if ok == 0 {
            break 'out;
        }
        if count == 0 {
            byteswap_fwrite(m, o, c + crv::CLOSING, 4, 1, file);
            byteswap_fwrite(m, o, c + crv::KEYFRAMED, 2, 1, file);
        } else {
            len = len.wrapping_add(6);
        }
        done(m, len);
        m.release(FRAME);
        return ok;
    }
    done(m, len);
    m.release(FRAME);
    0
}

/// nurbsCompBinaryWrite (0x083f76c0); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): a composite curve, or with `count` set its
/// length added to `*total`: the boundary flag (short), the number of curves (int), the
/// curves. A null composite answers 1 and writes nothing.
pub fn nurbs_comp_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32, count: i16, total: u32) -> i32 {
    if p == 0 {
        return 1;
    }
    const FRAME: u32 = 4;
    let len_at = m.local(FRAME);
    m.set_i32(len_at, 0);
    let mut ok: i16 = 1;
    let finish = |m: &mut Mem| {
        let t = m.i32(total).wrapping_add(m.i32(len_at));
        m.set_i32(total, t);
    };
    if count == 0 {
        if byteswap_fwrite(m, o, p + comp::BOUNDARY, 2, 1, file) != 1 {
            ok = 0;
        }
    } else {
        m.set_i32(len_at, 2);
    }
    if ok != 0 {
        if count == 0 {
            if byteswap_fwrite(m, o, p + comp::NB_CURVES, 4, 1, file) != 1 {
                ok = 0;
            }
        } else {
            let v = m.i32(len_at).wrapping_add(4);
            m.set_i32(len_at, v);
        }
        if ok != 0 {
            let mut i = 0i32;
            while i < m.i32(p + comp::NB_CURVES) {
                let c = m.u32(m.u32(p + comp::CURVES).wrapping_add(4 * i as u32));
                ok = nurbs_crv_binary_write(m, o, file, c, count, len_at);
                if ok == 0 {
                    finish(m);
                    m.release(FRAME);
                    return 0;
                }
                i += 1;
            }
            finish(m);
            m.release(FRAME);
            return ok as i32;
        }
    }
    finish(m);
    m.release(FRAME);
    0
}

/// [soft 0x083f7038 nurbsSrfBinaryWrite]: a NURBS surface: the orders and point counts in
/// U and V, the points (as a curve's), the closed flags, the uniformity, the knots in U
/// and in V with their counts, the ranges, the border flag, the trims (a count and the
/// composite curves), the steps and curves, capping flags, closings, the approximation;
/// then the projected curves: their total length (a first, counting pass, plus 4), their
/// count and the curves. A failed write makes it answer 0 at once. Answers 1.
pub fn nurbs_srf_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, p: u32) -> i32 {
    // the frame: the projected curves' length, the trims' (unused) length, a point
    const FRAME: u32 = 0x28;
    let frame = m.local(FRAME);
    let (proj_len, trim_len, point_at) = (frame, frame + 4, frame + 8);
    m.set_i32(trim_len, 0);
    m.set_i32(proj_len, 0);
    let w = |m: &mut Mem, o: &mut dyn Outside, at: u32, size: u32, n: u32| byteswap_fwrite(m, o, at, size, n, file) == n;
    let result = 'out: {
        if !(w(m, o, p + srf::ORDER, 4, 1) && w(m, o, p + srf::V + srf::ORDER, 4, 1) && w(m, o, p + srf::NB_POINTS, 4, 1) && w(m, o, p + srf::V + srf::NB_POINTS, 4, 1)) {
            break 'out 0;
        }
        let mut i = 0i32;
        while i < m.i32(p + srf::TOTAL_POINTS) {
            let point = m.u32(p + srf::POINTS).wrapping_add((i as u32).wrapping_mul(crv::POINT));
            for k in 0..4 {
                let v = m.u64(point + 8 * k);
                m.set_u64(point_at + 8 * k, v);
            }
            if !w(m, o, point_at, 8, 4) {
                break 'out 0;
            }
            if !w(m, o, point + crv::POINT_FLAG, 4, 1) {
                break 'out 0;
            }
            i += 1;
        }
        let ok = w(m, o, p + srf::CLOSED, 2, 1);
        if !(ok && w(m, o, p + srf::V + srf::CLOSED, 2, 1) && w(m, o, p + srf::UNIFORMITY, 4, 1) && w(m, o, p + srf::NB_KNOTS, 4, 1)) {
            break 'out 0;
        }
        let n = m.u32(p + srf::NB_KNOTS);
        if !w(m, o, m.u32(p + srf::KNOTS), 8, n) || !w(m, o, p + srf::V + srf::NB_KNOTS, 4, 1) {
            break 'out 0;
        }
        let n = m.u32(p + srf::V + srf::NB_KNOTS);
        if !w(m, o, m.u32(p + srf::V + srf::KNOTS), 8, n) {
            break 'out 0;
        }
        for off in [srf::START, srf::END, srf::V + srf::START, srf::V + srf::END] {
            if !w(m, o, p + off, 8, 1) {
                break 'out 0;
            }
        }
        if !w(m, o, p + srf::BORDER, 2, 1) || !w(m, o, p + srf::NB_TRIMS, 4, 1) {
            break 'out 0;
        }
        let mut i = 0i32;
        while i < m.i32(p + srf::NB_TRIMS) {
            let c = m.u32(m.u32(p + srf::TRIMS).wrapping_add(4 * i as u32));
            if nurbs_comp_binary_write(m, o, file, c, 0, trim_len) as i16 == 0 {
                break 'out 0;
            }
            i += 1;
        }
        for off in [srf::STEPS, srf::STEPS + 4, srf::CURVES, srf::CURVES + 4] {
            if !w(m, o, p + off, 4, 1) {
                break 'out 0;
            }
        }
        for k in 0..4 {
            byteswap_fwrite(m, o, p + srf::CAPS + 2 * k, 2, 1, file);
        }
        byteswap_fwrite(m, o, p + srf::CLOSING, 4, 1, file);
        byteswap_fwrite(m, o, p + srf::CLOSING + 4, 4, 1, file);
        byteswap_fwrite(m, o, p + srf::APPROX_METHOD, 2, 1, file);
        byteswap_fwrite(m, o, p + srf::APPROX_VIEW, 2, 1, file);
        for k in 0..3 {
            byteswap_fwrite(m, o, p + srf::APPROX_FLOATS + 4 * k, 4, 1, file);
        }
        for off in [srf::APPROX_MIN, srf::APPROX_MAX, srf::APPROX_RECURSION] {
            byteswap_fwrite(m, o, p + off, 2, 1, file);
        }
        if 0 < m.i32(p + srf::NB_PROJECTED) {
            let mut i = 0i32;
            while i < m.i32(p + srf::NB_PROJECTED) {
                let c = m.u32(m.u32(p + srf::PROJECTED).wrapping_add(4 * i as u32));
                nurbs_comp_binary_write(m, o, file, c, 1, proj_len);
                i += 1;
            }
        }
        let v = m.i32(proj_len).wrapping_add(4);
        m.set_i32(proj_len, v);
        let mut ok = w(m, o, proj_len, 4, 1);
        if !w(m, o, p + srf::NB_PROJECTED, 4, 1) {
            ok = false;
        }
        if !ok {
            break 'out 0;
        }
        let mut i = 0i32;
        while i < m.i32(p + srf::NB_PROJECTED) {
            let c = m.u32(m.u32(p + srf::PROJECTED).wrapping_add(4 * i as u32));
            if nurbs_comp_binary_write(m, o, file, c, 0, trim_len) as i16 == 0 {
                break 'out 0;
            }
            i += 1;
        }
        1
    };
    m.release(FRAME);
    result
}

/// si_metaballBinaryWrite (0x083f2520); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): ratio and weight (doubles), the exponents
/// (three doubles), the matrix.
pub fn metaball_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, b: u32) {
    byteswap_fwrite(m, o, b + ball::RATIO, 8, 1, file);
    byteswap_fwrite(m, o, b + ball::WEIGHT, 8, 1, file);
    three_doubles_fwrite(m, o, b + ball::EXPONENT, 0x18, 1, file);
    matrix_fwrite(m, o, b + ball::MATRIX, 0x40, 1, file);
}

/// si_metaSystemBinaryWrite (0x083f29ac); unchecked (no file the original wrote reaches it; held against
/// the value form's writer by `the_writers_write_back_what_the_readers_read`): a metasystem, after its groups were
/// checked (`SI_metaSystemValidateGroups`): wire-frame and rendering resolutions, the show
/// flag; the number of groups and per group its name, the number of its primitives still
/// marked valid (short +0x10) and their models' names; the number of unblend pairs and
/// per pair the names of its two groups. The lists go out in their order in memory.
pub fn meta_system_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, s: u32) {
    const FRAME: u32 = 4;
    let local = m.local(FRAME);
    o.call(m, &SI_META_SYSTEM_VALIDATE_GROUPS, &[s]);
    byteswap_fwrite(m, o, s + system::WIRE_RESOLUTION, 4, 1, file);
    byteswap_fwrite(m, o, s + system::RENDER_RESOLUTION, 4, 1, file);
    byteswap_fwrite(m, o, s + system::SHOW, 2, 1, file);
    let count = |m: &Mem, first: u32, next: u32| {
        let mut n: i16 = 0;
        let mut p = first;
        while p != 0 {
            n = n.wrapping_add(1);
            p = m.u32(p + next);
        }
        n
    };
    let n = count(m, m.u32(s + system::GROUPS), system::GROUP_NEXT);
    m.set_i16(local, n);
    byteswap_fwrite(m, o, local, 2, 1, file);
    let mut g = m.u32(s + system::GROUPS);
    while g != 0 {
        let name = m.u32(g + system::GROUP_NAME);
        string_binary_write(m, o, file, name);
        let mut n: i16 = 0;
        let mut p = m.u32(g + system::GROUP_PRIMITIVES);
        while p != 0 {
            if m.i16(p + 0x10) != 0 {
                n = n.wrapping_add(1);
            }
            p = m.u32(p + system::PRIMITIVE_NEXT);
        }
        m.set_i16(local, n);
        byteswap_fwrite(m, o, local, 2, 1, file);
        let mut p = m.u32(g + system::GROUP_PRIMITIVES);
        while p != 0 {
            if m.i16(p + 0x10) != 0 {
                // the primitive's metaball, its model, the model's name
                let model = m.u32(m.u32(p) + ball::MODEL);
                let name = m.u32(model + node::NAME);
                string_binary_write(m, o, file, name);
            }
            p = m.u32(p + system::PRIMITIVE_NEXT);
        }
        g = m.u32(g + system::GROUP_NEXT);
    }
    let n = count(m, m.u32(s + system::UNBLENDS), system::UNBLEND_NEXT);
    m.set_i16(local, n);
    byteswap_fwrite(m, o, local, 2, 1, file);
    let mut u = m.u32(s + system::UNBLENDS);
    while u != 0 {
        for k in 0..2 {
            let group = m.u32(u + 4 * k);
            let name = m.u32(group);
            string_binary_write(m, o, file, name);
        }
        u = m.u32(u + system::UNBLEND_NEXT);
    }
    m.release(FRAME);
}

/// C's `strtol(s, NULL, 10)`: leading white space, a sign, decimal digits; saturated at
/// the bounds of a 32-bit long.
fn strtol10(text: &[u8]) -> i32 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut negative = false;
    if i < text.len() && (text[i] == b'+' || text[i] == b'-') {
        negative = text[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < text.len() && text[i].is_ascii_digit() {
        v = (v * 10 + (text[i] - b'0') as i64).min(1 << 40);
        i += 1;
    }
    let v = if negative { -v } else { v };
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// setT2dMapDomainID (0x083f3574); unchecked (only its first step, the dictionary look-up answered 0, is reached): the 2D texture numbered `id` in the scene's
/// dictionary (`SI_elemdictGetElem` on the dictionary of the scene `SI_procData` names
/// and chapter `SI_cid[26]`) is given a user-data block `_UVDomainID_` holding `id` in
/// decimal: an existing block of that tag keeps its data if it already reads as `id`
/// (`strtol`), else its data is freed and replaced; without one a zeroed block (`SI_calloc`)
/// with a copy of the tag is put at the head of the texture's list. The data is a copy
/// (`strdup`) of the text, its size the text's length with the zero.
pub fn set_t2d_map_domain_id(m: &mut Mem, o: &mut dyn Outside, id: i32) {
    let scene = m.i32(m[SI_PROC_DATA]);
    let dict = m[SI_ELEM_DICTS].wrapping_add((scene as u32).wrapping_mul(0x70));
    let chapter = m[SI_CID][26];
    let elem = o.call(m, &SI_ELEMDICT_GET_ELEM, &[dict, chapter as u32, id as u32]);
    if elem == 0 {
        return;
    }
    let texture = m.u32(elem + 4);
    let mut block = m.u32(texture + lclmap::USER_DATA);
    while block != 0 && !is(m, m.u32(block), b"_UVDomainID_") {
        block = m.u32(block + 0x10);
    }
    if block != 0 {
        let data = m.u32(block + 0xc);
        if data == 0 {
            // strtol of a null pointer: the original dies here
            m.record_fault(0, 1, si_mem::Access::Read);
            return;
        }
        if strtol10(&m.cstr(data)) == id {
            return;
        }
        m.free(data);
        m.set_u32(block + 0xc, 0);
    } else {
        block = m.si_calloc(1, 0x14);
        let tag = m.strdup(S_UV_DOMAIN_ID_NEW);
        m.set_u32(block, tag);
        let head = m.u32(texture + lclmap::USER_DATA);
        m.set_u32(block + 0x10, head);
        m.set_u32(texture + lclmap::USER_DATA, block);
    }
    // sprintf("%d") into 128 bytes of stack, then its length plus one and a copy
    const FRAME: u32 = 0x80;
    let text = m.local(FRAME);
    m.put_cstr(text, id.to_string().as_bytes());
    let size = strlen(m, text) + 1;
    m.set_u32(block + 8, size);
    let copy = m.strdup(text);
    m.set_u32(block + 0xc, copy);
    m.release(FRAME);
}

/// si_filenameCleanup (0x083f6238); unchecked by itself (`SI_shapeBinaryRead`'s cases reach it): a copy (`strdup`) of `name` without the
/// directory `base` in front: when `base` is not null, is shorter than `name`, begins
/// `name`, and a `/` follows it there, the copy starts behind that `/`.
pub fn filename_cleanup(m: &mut Mem, name: u32, base: u32) -> u32 {
    let mut from = name;
    if base != 0 {
        let (n, b) = (m.cstr(name), m.cstr(base));
        if b.len() < n.len() && n.starts_with(&b) && n[b.len()] == b'/' {
            from = name.wrapping_add(b.len() as u32 + 1);
        }
    }
    m.strdup(from)
}

/// The node's user data in the old form (tokens 22): the count of the blocks written and
/// the blocks, each as the first four bytes of its tag, the size, the data (the custom
/// effects' data converted to the file's byte order around the write). At 4.0: every
/// block whose tag does not start with `_`, except `CUSELEMCB` and `CUSNEXTELEMCB`.
fn old_user_data(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u32, short_at: u32) {
    let mut count: u16 = 0;
    let mut p = m.u32(n + node::USER_DATA);
    while p != 0 {
        let tag = m.u32(p);
        if tag == 0 {
            null_tag(m);
        } else if !is_element_link(m, tag) {
            if m.u8(tag) == b'_' {
                if !out_from(m, 3.50009) && strlen(m, tag) <= 5 {
                    count = count.wrapping_add(1);
                }
            } else {
                count = count.wrapping_add(1);
            }
        }
        p = m.u32(p + 0x10);
    }
    m.set_u16(short_at, count);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    let mut p = m.u32(n + node::USER_DATA);
    while p != 0 {
        let tag = m.u32(p);
        if tag == 0 {
            null_tag(m);
            p = m.u32(p + 0x10);
            continue;
        }
        let underscore = m.u8(tag) == b'_';
        let written = if underscore {
            // not above 3.49091: only a tag of fewer than 6 bytes
            !out_above(m, 3.49091) && strlen(m, tag) < 6
        } else {
            !is_element_link(m, tag)
        };
        if written {
            if out_below(m, 3.50009) && underscore {
                byteswap_fwrite(m, o, tag + 1, 4, 1, file);
            } else {
                byteswap_fwrite(m, o, tag, 1, 4, file);
            }
            byteswap_fwrite(m, o, p + 8, 4, 1, file);
            let fix = |m: &mut Mem, o: &mut dyn Outside, back: u32| {
                if underscore {
                    return;
                }
                if out_below(m, 3.49089) {
                    fix_custom(m, o, &SI_FIX_CUSTOM_HEADER_OLD, p, back);
                } else if is(m, tag, b"CUSA") {
                    fix_custom(m, o, &SI_FIX_CUSTOM_HEADER, p, back);
                } else if is(m, tag, b"CUSD") {
                    fix_custom(m, o, &SI_FIX_CUSTOM_DSO_FCURVES, p, back);
                } else if is(m, tag, b"CUSC") {
                    fix_custom(m, o, &SI_FIX_CUSTOM_FCURVES, p, back);
                }
            };
            fix(m, o, 0);
            let (data, size) = (m.u32(p + 0xc), m.u32(p + 8));
            if out_below(m, 3.50009) {
                byteswap_fwrite(m, o, data, size, 1, file);
            } else {
                byteswap_fwrite(m, o, data, 1, size, file);
            }
            fix(m, o, 1);
        }
        p = m.u32(p + 0x10);
    }
}

/// The node's user data in the new form (token 0xff1c): the length (4 plus, per block
/// written, its tag's length, its size and 10), the count, the blocks as
/// `si_userDataWrite` writes them. Written: the blocks whose tag starts with `_`, and none
/// at all when the list's **first** block's tag is `__NodeInfo` (the original compares
/// the head's tag for every block).
fn new_user_data(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u32, frame: u32) {
    let (size_at, count_at, len_at) = (frame, frame + 4, frame + 8);
    let mut size = 0u32;
    let mut count = 0u32;
    let head_is_node_info = |m: &Mem| {
        let head = m.u32(n + node::USER_DATA);
        is(m, m.u32(head), b"__NodeInfo")
    };
    let mut p = m.u32(n + node::USER_DATA);
    while p != 0 {
        let tag = m.u32(p);
        if m.u8(tag) == b'_' && !head_is_node_info(m) && (out_above(m, 3.49091) || strlen(m, tag) >= 6) {
            size = size.wrapping_add(strlen(m, tag)).wrapping_add(m.u32(p + 8)).wrapping_add(10);
            count = count.wrapping_add(1);
        }
        p = m.u32(p + 0x10);
    }
    m.set_u32(size_at, size.wrapping_add(4));
    m.set_u32(count_at, count);
    byteswap_fwrite(m, o, size_at, 4, 1, file);
    byteswap_fwrite(m, o, count_at, 4, 1, file);
    let mut p = m.u32(n + node::USER_DATA);
    while p != 0 {
        let tag = m.u32(p);
        if m.u8(tag) == b'_' && !head_is_node_info(m) && !(out_below(m, 3.50009) && strlen(m, tag) < 6) {
            let len = strlen(m, tag);
            m.set_u32(len_at, len);
            byteswap_fwrite(m, o, len_at, 4, 1, file);
            byteswap_fwrite(m, o, tag, 1, len, file);
            byteswap_fwrite(m, o, p + 4, 2, 1, file);
            byteswap_fwrite(m, o, p + 8, 4, 1, file);
            let (data, size) = (m.u32(p + 0xc), m.u32(p + 8));
            byteswap_fwrite(m, o, data, 1, size, file);
        }
        p = m.u32(p + 0x10);
    }
}

/// The geometry of a node as a plain save writes it (`SI_saveTriMesh` clear): each type's
/// writer on the node's first definition, or with `global` on a copy of its current
/// geometry (+0x48) moved by the matrices `m44` (points) and `m84` (normals). Answers what
/// the NURBS writers answer (1 otherwise); on their failure a message is shown.
#[allow(clippy::too_many_arguments)]
fn write_geometry(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u32, global: bool, m44: u32, m84: u32, copy_at: u32, nurbs_len: u32) -> i16 {
    let first = |m: &Mem| m.u32(m.u32(n + node::DEFINITIONS));
    let current = m.u32(n + node::GEOMETRY);
    let mut result: i16 = 1;
    match m.i32(n + node::TYPE) {
        1 => {
            if !global {
                let g = first(m);
                patch_binary_write(m, o, file, g);
            } else {
                let c = o.call(m, &SI_PATCH_COPY, &[current]);
                m.set_u32(copy_at, c);
                o.call(m, &SI_PATCH_TRANSFORM, &[c, m44]);
                patch_binary_write(m, o, file, c);
                o.call(m, &SI_PATCH_DISPOSE, &[copy_at]);
            }
        }
        2 => {
            if !global {
                let g = first(m);
                face_binary_write(m, o, file, g);
            } else {
                let c = o.call(m, &SI_FACE_COPY, &[current]);
                m.set_u32(copy_at, c);
                o.call(m, &SI_FACE_TRANSFORM, &[c, m44]);
                face_binary_write(m, o, file, c);
                o.call(m, &SI_FACE_DISPOSE, &[copy_at]);
            }
        }
        4 => {
            if !global {
                let g = first(m);
                mesh_binary_write(m, o, file, g, n);
            } else {
                let c = o.call(m, &SI_MESH_COPY, &[current]);
                m.set_u32(copy_at, c);
                o.call(m, &SI_MESH_TRANSFORM, &[c, m44, m84]);
                mesh_binary_write(m, o, file, c, n);
                o.call(m, &SI_MESH_DISPOSE, &[copy_at]);
            }
        }
        6 => {
            if !global {
                let g = first(m);
                spline_binary_write(m, o, file, g);
            } else {
                let c = spline_copy(m, current).addr();
                m.set_u32(copy_at, c);
                o.call(m, &SI_SPLINE_TRANSFORM, &[c, m44]);
                spline_binary_write(m, o, file, c);
                spline_dispose(m, copy_at);
            }
        }
        7 => {
            let g = if global { current } else { first(m) };
            meta_system_binary_write(m, o, file, g);
        }
        8 => {
            let g = if global { current } else { first(m) };
            metaball_binary_write(m, o, file, g);
        }
        9 => {
            if !global {
                let g = first(m);
                result = nurbs_crv_binary_write(m, o, file, g, 0, nurbs_len);
            } else {
                let c = o.call(m, &SI_NURBS_CRV_COPY, &[current]);
                m.set_u32(copy_at, c);
                o.call(m, &SI_NURBS_CRV_APPLY_MATRIX, &[c, m44, 0]);
                result = nurbs_crv_binary_write(m, o, file, c, 0, nurbs_len);
                nurbs_crv_dispose(m, copy_at);
            }
            if result == 0 {
                o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[S_ERROR_NURBS_CURVE, 3]);
            }
        }
        10 => {
            if !global {
                let g = first(m);
                result = nurbs_srf_binary_write(m, o, file, g) as i16;
            } else {
                let c = o.call(m, &SI_NURBS_SRF_COPY, &[current]);
                m.set_u32(copy_at, c);
                o.call(m, &SI_NURBS_SRF_APPLY_MATRIX, &[c, m44, 0]);
                result = nurbs_srf_binary_write(m, o, file, c) as i16;
                o.call(m, &SI_NURBS_SRF_DISPOSE, &[copy_at]);
            }
            if result == 0 {
                o.call(m, &SI_STATUS_DISPLAY_MESSAGE, &[S_ERROR_NURBS_SURFACE, 3]);
            }
        }
        _ => {}
    }
    result
}

/// The geometry of a node in a triangulated save (`SI_saveTriMesh` set): patches, faces,
/// meshes and NURBS surfaces become a copy as a polygon mesh (`SI_patchConvertToMesh`,
/// `SI_faceConvertToMesh`, `SI_meshCopy`, `SI_nurbsConvertToMesh`), moved by the matrices
/// with `global`, triangulated (`SI_meshTriangulate`, second argument 1), written and
/// disposed of; a metasystem becomes a mesh (`SI_meta2Mesh`) that is written without
/// being triangulated; splines and NURBS curves are written as they are (the curve's
/// answer not looked at); a metaball writes nothing (its type was written as 0).
#[allow(clippy::too_many_arguments)]
fn write_geometry_triangulated(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u32, global: bool, m44: u32, m84: u32, copy_at: u32, nurbs_len: u32) {
    let first = |m: &Mem| m.u32(m.u32(n + node::DEFINITIONS));
    let current = m.u32(n + node::GEOMETRY);
    let shape = |m: &Mem| if global { current } else { first(m) };
    let mesh = match m.i32(n + node::TYPE) {
        1 => {
            let g = shape(m);
            o.call(m, &SI_PATCH_CONVERT_TO_MESH, &[n, g])
        }
        2 => {
            let g = shape(m);
            o.call(m, &SI_FACE_CONVERT_TO_MESH, &[g, 1])
        }
        4 => {
            let g = shape(m);
            o.call(m, &SI_MESH_COPY, &[g])
        }
        10 => {
            let g = shape(m);
            o.call(m, &SI_NURBS_CONVERT_TO_MESH, &[n, g])
        }
        6 => {
            if !global {
                let g = first(m);
                spline_binary_write(m, o, file, g);
            } else {
                let c = spline_copy(m, current).addr();
                m.set_u32(copy_at, c);
                o.call(m, &SI_SPLINE_TRANSFORM, &[c, m44]);
                spline_binary_write(m, o, file, c);
                spline_dispose(m, copy_at);
            }
            return;
        }
        7 => {
            let g = shape(m);
            let c = o.call(m, &SI_META2_MESH, &[g]);
            m.set_u32(copy_at, c);
            if global {
                o.call(m, &SI_MESH_TRANSFORM, &[c, m44, m84]);
            }
            mesh_binary_write(m, o, file, c, n);
            o.call(m, &SI_MESH_DISPOSE, &[copy_at]);
            return;
        }
        9 => {
            if !global {
                let g = first(m);
                nurbs_crv_binary_write(m, o, file, g, 0, nurbs_len);
            } else {
                let c = o.call(m, &SI_NURBS_CRV_COPY, &[current]);
                m.set_u32(copy_at, c);
                o.call(m, &SI_NURBS_CRV_APPLY_MATRIX, &[c, m44, 0]);
                nurbs_crv_binary_write(m, o, file, c, 0, nurbs_len);
                nurbs_crv_dispose(m, copy_at);
            }
            return;
        }
        _ => return,
    };
    m.set_u32(copy_at, mesh);
    if global {
        o.call(m, &SI_MESH_TRANSFORM, &[mesh, m44, m84]);
    }
    o.call(m, &SI_MESH_TRIANGULATE, &[mesh, 1]);
    mesh_binary_write(m, o, file, mesh, n);
    o.call(m, &SI_MESH_DISPOSE, &[copy_at]);
}

/// [soft 0x083f3c20 SI_modelBinaryWriteNode]: a node and, as asked, its son and brother
/// (each written with both, so a whole hierarchy below the first node): finding 0004's
/// hierarchy tokens in the order of the table. `with_son`, `with_brother`: whether the
/// node's son and brother go out (their flags are written 0 otherwise). `state`: the save
/// clears the node's state short (+0xa) and keeps bit 0x10 of it, also setting bit 4 of
/// the byte at +0x3fa, for a node with several definitions or a word at +0x344. `global`:
/// the geometry in global coordinates (matrices of the node at +0x1c4 and of the node at
/// +0x44: `SI_matrixInverse`, `SI_matrixMultiply`, `SI_matrixTransposeInverse`).
///
/// Before anything a custom-effect node (algorithm 13) gets its data into its user data
/// (`SI_cusAddUserData`), and a node with several texture coordinate sets gets each set's
/// texture ids into those textures (`setT2dMapDomainID`). Then: the name; the type (with
/// `SI_saveTriMesh`: 8 written as 0, and 1, 2, 7, 10 as 4); the geometry; the shape
/// interpolation; scaling, rotation, translation; the materials (each after a zero
/// "external" short, with its textures); the 2D textures; the 3D texture; the dynamics;
/// the algorithm (2 written as 1, 12 as 0); the envelope type and the envelope; the chain
/// dynamics flag; the joint; the inverse kinematics; the clusters; the old user data;
/// above 2.65 visibility and motion blur; above 2.69904 the initial transformation and from
/// 3.089 matte and garbage matte; from 3.49089 the new user data; from 3.4909 the clusters
/// and user data on vertices, polygons and edges; from 3.93 the texture sets' ids; from
/// 3.8109 the shorts at +0x404 and +0x406; from 3.8209 the one at +0x420; bit 3 of the byte
/// at +8; the son; the brother. Answers what the NURBS writers answered (1 otherwise).
pub fn model_binary_write_node(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u32, with_son: i16, with_brother: i16, state: i16, global: i16) -> i32 {
    // the frame: the zero short, the short local, the flag short, the NURBS length, the
    // copies' pointer, the new user data's three words, the cluster sizes, the sets'
    // length, a zero word, and the two matrices
    const FRAME: u32 = 0xc0;
    let frame = m.local(FRAME);
    let (zero_at, short_at, flag_at, nurbs_len, copy_at, ud_at, size_at, sets_at, zero4_at, m84, m44) =
        (frame, frame + 2, frame + 4, frame + 8, frame + 0xc, frame + 0x10, frame + 0x1c, frame + 0x20, frame + 0x24, frame + 0x28, frame + 0x68);
    m.set_i16(zero_at, 0);
    m.set_i32(nurbs_len, 0);
    if m.i32(n + node::ALGORITHM) == 0xd {
        o.call(m, &SI_CUS_ADD_USER_DATA, &[n]);
    }
    if state != 0 {
        let was = m.u16(n + node::STATE);
        m.set_u16(n + node::STATE, 0);
        if was & 0x10 != 0 && (1 < m.u16(n + node::NB_DEFINITIONS) || m.u32(n + 0x344) != 0) {
            m.set_u16(n + node::STATE, 0x10);
            m.or_u8(n + 0x3fa, 0x10);
        }
    }
    if 1 < m.i32(n + node::NB_UV_SETS) {
        let mut set: i16 = 0;
        loop {
            let rec = m.u32(n + node::UV_DOMAINS).wrapping_add(8 * set as i32 as u32);
            set = set.wrapping_add(1);
            let (ids, count) = (m.u32(rec), m.i32(rec + 4));
            if 0 < count && ids != 0 {
                let mut k = 0i32;
                while k < m.i32(rec + 4) {
                    let id = m.i32(m.u32(rec).wrapping_add(4 * k as u32));
                    set_t2d_map_domain_id(m, o, id);
                    k += 1;
                }
            }
            if (set as i32) >= m.i32(n + node::NB_UV_SETS).wrapping_sub(1) {
                break;
            }
        }
    }
    let name = m.u32(n + node::NAME);
    string_binary_write(m, o, file, name);
    let mut kind = m.u16(n + node::TYPE);
    let tri = m[SI_SAVE_TRI_MESH] != 0;
    if tri {
        let t = m.i32(n + node::TYPE);
        if t == 8 {
            kind = 0;
        }
        if t == 7 || t == 2 || t == 1 || t == 10 {
            kind = 4;
        }
    }
    m.set_u16(short_at, kind);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    if global != 0 {
        o.call(m, &SI_MATRIX_INVERSE, &[n + 0x1c4, m84]);
        let other = m.u32(n + 0x44).wrapping_add(0x1c4);
        o.call(m, &SI_MATRIX_MULTIPLY, &[other, m84, m44]);
        o.call(m, &SI_MATRIX_TRANSPOSE_INVERSE, &[m44, m84]);
    }
    let mut result: i16 = 1;
    if !tri {
        result = write_geometry(m, o, file, n, global != 0, m44, m84, copy_at, nurbs_len);
    } else {
        write_geometry_triangulated(m, o, file, n, global != 0, m44, m84, copy_at, nurbs_len);
    }
    byteswap_fwrite(m, o, n + node::SHAPE_INTERPOLATION, 2, 1, file);
    vector_fwrite(m, o, n + node::SCALING, 0xc, 1, file);
    vector_fwrite(m, o, n + node::ROTATION, 0xc, 1, file);
    vector_fwrite(m, o, n + node::TRANSLATION, 0xc, 1, file);
    byteswap_fwrite(m, o, n + node::NB_MATERIALS, 2, 1, file);
    let mut k: i16 = 0;
    while k < m.i16(n + node::NB_MATERIALS) {
        let material = m.u32(m.u32(n + node::MATERIALS).wrapping_add(4 * k as i32 as u32));
        byteswap_fwrite(m, o, zero_at, 2, 1, file);
        material_binary_write(m, o, file, material, 1);
        k = k.wrapping_add(1);
    }
    let mut count: i16 = 0;
    let mut x = m.u32(n + node::TEXTURES);
    while x != 0 {
        count = count.wrapping_add(1);
        x = m.u32(x + lclmap::NEXT);
    }
    m.set_i16(short_at, count);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    let mut x = m.u32(n + node::TEXTURES);
    let mut k: i16 = 0;
    while k < m.i16(short_at) {
        byteswap_fwrite(m, o, zero_at, 2, 1, file);
        texture_binary_write(m, o, file, x);
        x = m.u32(x + lclmap::NEXT);
        k = k.wrapping_add(1);
    }
    let x3 = m.u32(n + node::TEXTURE3D);
    m.set_i16(short_at, (x3 != 0) as i16);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    if x3 != 0 {
        byteswap_fwrite(m, o, zero_at, 2, 1, file);
        texture3d_binary_write(m, o, file, x3);
    }
    let flag = |m: &mut Mem, o: &mut dyn Outside, off: u32| {
        let set = m.u32(n + off) != 0;
        m.set_i16(short_at, set as i16);
        byteswap_fwrite(m, o, short_at, 2, 1, file);
        set
    };
    if flag(m, o, node::DYNA_MATTER) {
        byteswap_fwrite(m, o, zero_at, 2, 1, file);
        let d = m.u32(n + node::DYNA_MATTER);
        dynamatter_write_binary_chunk(m, o, file, d);
    }
    let mut algorithm = m.u16(n + node::ALGORITHM);
    if algorithm == 2 {
        algorithm = 1;
    } else if algorithm == 0xc {
        algorithm = 0;
    }
    m.set_u16(short_at, algorithm);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    let envelope_type = m.u16(n + node::ENVELOPE_TYPE);
    m.set_u16(short_at, envelope_type);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    if flag(m, o, node::ENVELOPE) {
        let e = m.u32(n + node::ENVELOPE);
        envelop_write_binary_chunk(m, o, file, e);
    }
    flag(m, o, node::DYNA_CHAIN);
    if flag(m, o, more::DYNA_JOINT) {
        dynajntprop_write_binary_chunk(m, o, file, n);
    }
    if flag(m, o, node::INV_KIN) {
        let ik = m.u32(n + node::INV_KIN);
        inv_kin_write_binary_chunk(m, o, file, ik);
    }
    let clusters = cluster_count(m, n) as i16;
    m.set_i16(short_at, clusters);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    if 0 < m.i16(short_at) {
        clusters_write_binary_chunk(m, o, file, n);
    }
    old_user_data(m, o, file, n, short_at);
    if out_above(m, 2.65) {
        byteswap_fwrite(m, o, n + node::VISIBILITY, 2, 1, file);
        byteswap_fwrite(m, o, n + node::MOTION_BLUR, 2, 1, file);
    }
    if out_above(m, 2.69904) {
        if flag(m, o, node::INITIAL_SRT) {
            let srt = m.u32(n + node::INITIAL_SRT);
            for k in 0..3 {
                vector_fwrite(m, o, srt + 0xc * k, 0xc, 1, file);
            }
        }
        if out_from(m, 3.089) {
            byteswap_fwrite(m, o, n + node::MATTE, 4, 1, file);
            byteswap_fwrite(m, o, n + node::GARBAGE_MATTE, 2, 1, file);
        }
    }
    if out_from(m, 3.49089) {
        new_user_data(m, o, file, n, ud_at);
    }
    for (cluster_off, arrays_off, vertex) in [(node::VERTEX_CLUSTER, node::VERTEX_USER_DATA, 1i16), (node::POLYGON_CLUSTER, node::POLYGON_USER_DATA, 0), (node::EDGE_CLUSTER, node::EDGE_USER_DATA, 0)] {
        if out_from(m, 3.4909) {
            let cluster = m.u32(n + cluster_off);
            let size = user_data_cluster_size(m, n, cluster, vertex);
            m.set_u32(size_at, size);
            byteswap_fwrite(m, o, size_at, 4, 1, file);
            if cluster != 0 {
                cluster_write_binary_chunk(m, o, file, n, cluster, vertex);
            }
        }
        if out_from(m, 3.4909) {
            let (cluster, arrays) = (m.u32(n + cluster_off), m.u32(n + arrays_off));
            user_data_array_write(m, o, file, cluster, arrays);
        }
    }
    if out_from(m, 3.93) {
        let mut length = 0i32;
        if 1 < m.i32(n + node::NB_UV_SETS) {
            let last = m.i32(n + node::NB_UV_SETS) - 1;
            let mut set: i16 = 0;
            while (set as i32) < last {
                let rec = m.u32(n + node::UV_DOMAINS).wrapping_add(8 * set as i32 as u32);
                length = length.wrapping_add(4);
                set = set.wrapping_add(1);
                let count = m.i32(rec + 4);
                if 0 < count && m.u32(rec) != 0 {
                    length = length.wrapping_add(4i32.wrapping_mul(count));
                }
            }
        }
        m.set_i32(sets_at, length);
        byteswap_fwrite(m, o, sets_at, 4, 1, file);
        if 1 < m.i32(n + node::NB_UV_SETS) {
            let mut set: i16 = 0;
            loop {
                let rec = m.u32(n + node::UV_DOMAINS).wrapping_add(8 * set as i32 as u32);
                if m.i32(rec + 4) < 1 || m.u32(rec) == 0 {
                    m.set_u32(zero4_at, 0);
                    byteswap_fwrite(m, o, zero4_at, 4, 1, file);
                } else {
                    byteswap_fwrite(m, o, rec + 4, 4, 1, file);
                    let mut k = 0i32;
                    while k < m.i32(rec + 4) {
                        byteswap_fwrite(m, o, m.u32(rec).wrapping_add(4 * k as u32), 4, 1, file);
                        k += 1;
                    }
                }
                set = set.wrapping_add(1);
                if (set as i32) >= m.i32(n + node::NB_UV_SETS).wrapping_sub(1) {
                    break;
                }
            }
        }
    }
    if out_from(m, 3.8109) {
        byteswap_fwrite(m, o, n + node::TOKEN_221, 2, 1, file);
        byteswap_fwrite(m, o, n + node::TOKEN_222, 2, 1, file);
    }
    if out_from(m, 3.8209) {
        byteswap_fwrite(m, o, n + node::TOKEN_223, 2, 1, file);
    }
    let bit = (m.u8(n + node::FLAGS) & 8 != 0) as u16;
    m.set_u16(flag_at, bit);
    byteswap_fwrite(m, o, flag_at, 2, 1, file);
    let son = if with_son != 0 { (m.u32(n + node::SON) != 0) as i16 } else { 0 };
    m.set_i16(short_at, son);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    if 0 < m.i16(short_at) {
        let s = m.u32(n + node::SON);
        model_binary_write_node(m, o, file, s, 1, 1, state, global);
    }
    let brother = if with_brother != 0 { (m.u32(n + node::BROTHER) != 0) as i16 } else { 0 };
    m.set_i16(short_at, brother);
    byteswap_fwrite(m, o, short_at, 2, 1, file);
    if 0 < m.i16(short_at) {
        let b = m.u32(n + node::BROTHER);
        model_binary_write_node(m, o, file, b, 1, 1, state, global);
    }
    m.release(FRAME);
    result as i32
}

/// The end the model file writers share: unless files go to a pipe, `fflush` and
/// `ferror`; a failed write sets `SI_saveElemErrorCode` to 2 (except for
/// `modelBinaryWriteCurrent`) and answers 0.
fn flush_and_check(m: &mut Mem, o: &mut dyn Outside, file: u32, set_code: bool) -> i32 {
    let mut ok = 1;
    if m[SI_PIPED_FILES] == 0 {
        o.fflush(m, file);
        ok = (o.ferror(m, file) == 0) as i32;
        if ok == 0 && set_code {
            m[SI_SAVE_ELEM_ERROR_CODE] = 2;
        }
    }
    ok
}

/// [soft 0x083ec3f0 SI_fdmodelBinaryWrite]: a model into an open stream: the general
/// header (`SI_headerBinaryWrite`, no comment), the table, `HRCH`, and the hierarchy from
/// `model` (its son and their brothers; not the root's own brothers). The stream is not
/// closed. Answers 1, or 0 when a write failed.
pub fn fdmodel_binary_write(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, state: i16) -> i32 {
    header_binary_write(m, o, file, 0);
    prototype_binary_write(m, o, file);
    o.fwrite(m, S_HRCH_FD, 4, 1, file);
    model_binary_write_node(m, o, file, model, 1, 0, state, 0);
    flush_and_check(m, o, file, true)
}

/// What [`omodel_binary_write`] and [`model_binary_write_current`] share: the path
/// `name.hrc` in 256 bytes of stack, the stream (the pipe's when files are piped), with
/// `state` the node's file name (+0) replaced by `name` without `SI_objectPath`, then the
/// file's content and its closing.
fn write_hrc(m: &mut Mem, o: &mut dyn Outside, name: u32, model: u32, state: i16, mode_at: u32, tag_at: u32, global: i16, set_code: bool) -> i32 {
    const FRAME: u32 = 0x104;
    let path = m.local(FRAME);
    let mut text = m.cstr(name);
    text.extend_from_slice(b".hrc");
    if text.len() >= 0x100 {
        m.record_fault(path + 0x100, text.len() as u32 - 0xff, si_mem::Access::Write);
        m.release(FRAME);
        return 0;
    }
    m.put_cstr(path, &text);
    let mut file = m[SI_PIPE];
    if m[SI_PIPED_FILES] == 0 {
        file = o.fopen(m, path, mode_at);
    }
    let result;
    if file == 0 {
        if set_code {
            m[SI_SAVE_ELEM_ERROR_CODE] = 1;
        }
        result = 0;
    } else {
        if state != 0 {
            if m.u32(model) != 0 {
                m.free(m.u32(model));
                m.set_u32(model, 0);
            }
            let base = m[SI_OBJECT_PATH];
            let clean = filename_cleanup(m, name, base);
            m.set_u32(model, clean);
        }
        header_binary_write(m, o, file, 0);
        prototype_binary_write(m, o, file);
        o.fwrite(m, tag_at, 4, 1, file);
        model_binary_write_node(m, o, file, model, 1, 0, state, global);
        result = flush_and_check(m, o, file, set_code);
        if m[SI_PIPED_FILES] == 0 {
            o.fclose(m, file);
        }
    }
    m.release(FRAME);
    result
}

/// [soft 0x083f3430 SI_omodelBinaryWrite]: the model file `name` + `.hrc` as
/// [`fdmodel_binary_write`] writes a stream, in global coordinates when
/// `SI_saveCurrGeom` is set; the file is closed unless piped. No file: 0 and
/// `SI_saveElemErrorCode` 1.
pub fn omodel_binary_write(m: &mut Mem, o: &mut dyn Outside, name: u32, model: u32, state: i16) -> i32 {
    let global = m[SI_SAVE_CURR_GEOM];
    write_hrc(m, o, name, model, state, S_WB_HRC_O, S_HRCH_O, global, true)
}

/// modelBinaryWriteCurrent (0x083ec484); unchecked (nothing in the checks reaches it): the same always in global coordinates, and
/// without touching `SI_saveElemErrorCode`.
pub fn model_binary_write_current(m: &mut Mem, o: &mut dyn Outside, name: u32, model: u32, state: i16) -> i32 {
    write_hrc(m, o, name, model, state, S_WB_HRC_CURRENT, S_HRCH_CURRENT, 1, false)
}
