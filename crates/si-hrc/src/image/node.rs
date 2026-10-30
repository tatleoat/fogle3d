// SPDX-License-Identifier: MIT
//! The model reader of `binary.c` on the image: a hierarchy of nodes with what hangs on
//! them (finding 0004, "Hierarchy"), and the node allocator of `model_b.c` it starts from.
//!
//! `si-types` has the node record (`Model`, 0x430 bytes). The reader addresses the fields
//! it fills by the offsets of [`node`] below, read from `modelBinaryReadNode`; a pointer to
//! a node is typed with `si-types`' record.

use super::geom::mesh;
use super::globals::{FILE_VERSION, MODEL_COUNT, SI_ED_SUMATRA_TO_3X, SI_IDENTITE};
use super::io::{alloc_fits, byteswap_fread, file_skip_field, float_fread, read_bin_header, string_binary_read, swap4byte, vector_fread};
use super::material::mat;
use super::matio::{material_binary_read, next_token, texture3d_binary_read, texture_binary_read};
use super::mesh::mesh_binary_read;
use super::meta::{meta_system_binary_read, metaball_binary_read};
use super::chunks::{clusters_read_binary_chunk, dynajntprop_read_binary_chunk, dynamatter_read_binary_chunk, envelop_info_dispose, envelop_read_binary_chunk, inv_kin_allocate, inv_kin_read_binary_chunk};
use super::nurbs::{nurbs_crv_binary_read, nurbs_srf_binary_read};
use super::patch::patch_binary_read;
use super::spline::{face_binary_read, spline_binary_read};
use super::outside::{Outside, Routine};
use super::proto::{prototype_allocate, prototype_binary_read, prototype_dispose, slot, Prototypes};
use super::texture::{lclmap, matrix_copy, t3d};
use super::userdata::user_data_read;
use si_mem::{Mem, Ptr32};
pub use si_types::model::Model;

/// Offsets in a node (`__SI_Model`), as far as the reader fills them.
pub mod node {
    /// The name: a string in a block of its own.
    pub const NAME: u32 = 0x04;
    /// A byte of flags; bit 3 is set by token 0x224.
    pub const FLAGS: u32 = 0x08;
    /// A short the allocator sets to 0x3e and the reader clears when a node is complete.
    pub const STATE: u32 = 0x0a;
    /// The type: 0 null, 1 patch, 2 face, 4 mesh, 5 joint, 6 spline, 7 metasystem,
    /// 8 metaball, 9 NURBS curve, 10 NURBS surface.
    pub const TYPE: u32 = 0x30;
    /// The number of geometries (definitions), the array of their pointers, and the
    /// current one.
    pub const NB_DEFINITIONS: u32 = 0x34;
    pub const DEFINITIONS: u32 = 0x38;
    pub const GEOMETRY: u32 = 0x48;
    pub const SCALING: u32 = 0x100;
    pub const ROTATION: u32 = 0x10c;
    /// The matrix of the rotation angles.
    pub const ROTATION_MATRIX: u32 = 0x118;
    pub const TRANSLATION: u32 = 0x158;
    pub const NB_MATERIALS: u32 = 0x206;
    /// An array of material pointers; null for a material the file names without storing.
    pub const MATERIALS: u32 = 0x20c;
    pub const NB_TEXTURES: u32 = 0x216;
    /// The first 2D texture of the node's list (linked through a texture's +0x16c); the
    /// reader stores it in both words.
    pub const TEXTURE_CURRENT: u32 = 0x218;
    pub const TEXTURES: u32 = 0x21c;
    pub const TEXTURE3D: u32 = 0x224;
    pub const FATHER: u32 = 0x2bc;
    pub const BROTHER: u32 = 0x2c0;
    pub const SON: u32 = 0x2c4;
    pub const SHAPE_INTERPOLATION: u32 = 0x2e0;
    /// The animation algorithm (token 16).
    pub const ALGORITHM: u32 = 0x2ec;
    pub const KINE_STATE: u32 = 0x2f0;
    pub const DYNA_MATTER: u32 = 0x2f4;
    pub const DYNA_CHAIN: u32 = 0x304;
    pub const INV_KIN: u32 = 0x318;
    pub const ENVELOPE_TYPE: u32 = 0x31c;
    pub const ENVELOPE: u32 = 0x324;
    /// Three vectors in a block of their own: the initial scaling, rotation, translation.
    pub const INITIAL_SRT: u32 = 0x328;
    /// The user data list (`__SI_UserNode`).
    pub const USER_DATA: u32 = 0x34c;
    pub const VISIBILITY: u32 = 0x3ec;
    pub const MOTION_BLUR: u32 = 0x3ee;
    pub const MATTE: u32 = 0x3f4;
    pub const GARBAGE_MATTE: u32 = 0x3f8;
    pub const TOKEN_221: u32 = 0x404;
    pub const TOKEN_222: u32 = 0x406;
    /// The clusters and user data of vertices, polygons and edges: per kind a cluster
    /// record and an array of user data lists, one list per component.
    pub const VERTEX_CLUSTER: u32 = 0x408;
    pub const VERTEX_USER_DATA: u32 = 0x40c;
    pub const POLYGON_CLUSTER: u32 = 0x410;
    pub const POLYGON_USER_DATA: u32 = 0x414;
    pub const EDGE_CLUSTER: u32 = 0x418;
    pub const EDGE_USER_DATA: u32 = 0x41c;
    pub const TOKEN_223: u32 = 0x420;
    /// The number of texture coordinate sets, and for each after the first an 8-byte
    /// record: the array of the texture identifiers that use it, and their number.
    pub const NB_UV_SETS: u32 = 0x424;
    pub const UV_DOMAINS: u32 = 0x428;
    pub const SIZE: u32 = 0x430;
}

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
    };
}

routines! {
    /// `SI_modelChangeName (0x0846db14)`, `names.c`: gives a node its name in the scene's
    /// name table.
    SI_MODEL_CHANGE_NAME = "SI_modelChangeName", 0x0846db14;
    /// `SI_modelUpdateMatrix (0x0846559c)`, `model.c`: a node's local matrix from its
    /// scaling, rotation and translation.
    SI_MODEL_UPDATE_MATRIX = "SI_modelUpdateMatrix", 0x0846559c;
    /// `SI_modelUpdateSonState (0x084662f8)`, `model.c`: a node's global transformation
    /// from its father's.
    SI_MODEL_UPDATE_SON_STATE = "SI_modelUpdateSonState", 0x084662f8;
    /// `SI_matrixXyzRotation (0x0851882c)`, `algebra.c`: the matrix of three angles.
    SI_MATRIX_XYZ_ROTATION = "SI_matrixXyzRotation", 0x0851882c;
    /// `SI_txt2dSetWrap (0x082ad9f8)`, `t2d_dial.c`: a 2D texture's wrap flags from the
    /// shape of the node it hangs on.
    SI_TXT2D_SET_WRAP = "SI_txt2dSetWrap", 0x082ad9f8;
    /// `SI_txt2dDispose (0x082ae014)`, `t2d_dial.c`.
    SI_TXT2D_DISPOSE = "SI_txt2dDispose", 0x082ae014;
    /// `SI_txt3dFree (0x082b4054)`, `t3d_dial.c`.
    SI_TXT3D_FREE = "SI_txt3dFree", 0x082b4054;
    /// `SI_materialDispose (0x08284900)`, `mat_dial.c`.
    SI_MATERIAL_DISPOSE = "SI_materialDispose", 0x08284900;
    /// `SI_odynamicReadBinaryChunk (0x084994b0)`, `dynamicio.c`: the dynamics chunk of the
    /// oldest files.
    SI_ODYNAMIC_READ_BINARY_CHUNK = "SI_odynamicReadBinaryChunk", 0x084994b0;
    /// `SI_invKinReadBinaryChunk (0x084adb78)`, `invkin.c`: the inverse kinematics chunk.
    SI_INV_KIN_READ_BINARY_CHUNK = "SI_invKinReadBinaryChunk", 0x084adb78;
    /// `SI_invKinAllocate (0x084ad8f0)`, `invkin.c`.
    SI_INV_KIN_ALLOCATE = "SI_invKinAllocate", 0x084ad8f0;
    /// `SI_oEnvelopReadBinaryChunk (0x084bbe28)`, `envtools.c`: the envelope chunk of the
    /// oldest files.
    SI_O_ENVELOP_READ_BINARY_CHUNK = "SI_oEnvelopReadBinaryChunk", 0x084bbe28;
    /// `SI_dynachnvarAllocate (0x0849fad8)`, `settings.c`: the working state of a chain's
    /// dynamics.
    SI_DYNACHNVAR_ALLOCATE = "SI_dynachnvarAllocate", 0x0849fad8;
    /// `SI_modelKineStateAllocate (0x0849f9f8)`, `settings.c`.
    SI_MODEL_KINE_STATE_ALLOCATE = "SI_modelKineStateAllocate", 0x0849f9f8;
    /// `SI_indexEdgePropagate (0x08176954)`, `index.c`: reorders the edges' user data by
    /// the map from the file's edges to the rebuilt ones.
    SI_INDEX_EDGE_PROPAGATE = "SI_indexEdgePropagate", 0x08176954;
    /// `SI_metaSystemResolveGroupName (0x0845e210)`, `metaball.c`: turns the names in a
    /// metasystem's groups into models.
    SI_META_SYSTEM_RESOLVE_GROUP_NAME = "SI_metaSystemResolveGroupName", 0x0845e210;
    /// `SI_envelopInfoDispose (0x084bbf8c)`, `envtools.c`.
    SI_ENVELOP_INFO_DISPOSE = "SI_envelopInfoDispose", 0x084bbf8c;
    /// `SI_envelopRebuild (0x084bd2a0)`, `envtools.c`: rebuilds the envelopes of a
    /// hierarchy read from a file up to version 2.59.
    SI_ENVELOP_REBUILD = "SI_envelopRebuild", 0x084bd2a0;
    /// `SI_fix_SI_CustomHeaderOld (0x083f966c)`, `byteswap.c`: the byte order of a custom
    /// effect's data, as are the three that follow.
    SI_FIX_CUSTOM_HEADER_OLD = "SI_fix_SI_CustomHeaderOld", 0x083f966c;
    /// `SI_fix_SI_CustomHeader (0x083f96e4)`, `byteswap.c`.
    SI_FIX_CUSTOM_HEADER = "SI_fix_SI_CustomHeader", 0x083f96e4;
    /// `SI_fix_CustomDsoFcurves (0x083f9730)`, `byteswap.c`.
    SI_FIX_CUSTOM_DSO_FCURVES = "SI_fix_CustomDsoFcurves", 0x083f9730;
    /// `SI_fix_CustomFcurves (0x083f9810)`, `byteswap.c`.
    SI_FIX_CUSTOM_FCURVES = "SI_fix_CustomFcurves", 0x083f9810;
    /// `patchBinaryRead (0x083f0690)`, `binary.c`: on the image (`patch.rs`); named here,
    /// like the readers that follow, for [`Outside::stands_in`].
    PATCH_BINARY_READ = "patchBinaryRead", 0x083f0690;
    /// `faceBinaryRead (0x083f043c)`, `binary.c`.
    FACE_BINARY_READ = "faceBinaryRead", 0x083f043c;
    /// `splineBinaryRead (0x083f00dc)`, `binary.c`.
    SPLINE_BINARY_READ = "splineBinaryRead", 0x083f00dc;
    /// `metaSystemBinaryRead (0x083f257c)`, `binary.c`.
    META_SYSTEM_BINARY_READ = "metaSystemBinaryRead", 0x083f257c;
    /// `metaballBinaryRead (0x083f23ec)`, `binary.c`.
    METABALL_BINARY_READ = "metaballBinaryRead", 0x083f23ec;
    /// `nurbsCrvBinaryRead (0x083f2b38)`, `binary.c`.
    NURBS_CRV_BINARY_READ = "nurbsCrvBinaryRead", 0x083f2b38;
    /// `nurbsSrfBinaryRead (0x083f2d90)`, `binary.c`.
    NURBS_SRF_BINARY_READ = "nurbsSrfBinaryRead", 0x083f2d90;
    /// `dynamatterReadBinaryChunk (0x083f1674)`, `binary.c`.
    DYNAMATTER_READ_BINARY_CHUNK = "dynamatterReadBinaryChunk", 0x083f1674;
    /// `dynajntpropReadBinaryChunk (0x083f0b08)`, `binary.c`.
    DYNAJNTPROP_READ_BINARY_CHUNK = "dynajntpropReadBinaryChunk", 0x083f0b08;
    /// `clustersReadBinaryChunk (0x083f0be4)`, `binary.c`.
    CLUSTERS_READ_BINARY_CHUNK = "clustersReadBinaryChunk", 0x083f0be4;
    /// `envelopReadBinaryChunk (0x083f0e98)`, `binary.c`.
    ENVELOP_READ_BINARY_CHUNK = "envelopReadBinaryChunk", 0x083f0e98;
}

const ONE: u32 = 0x3f80_0000;

/// [soft 0x08477cf4 SI_modelAllocate]: a new node. The running count of nodes goes up and
/// is the node's number (+0x40); the name is a copy of `name`, or `obj` and the count for
/// null. Scalings 1, matrices the identity, visibility 7, one texture coordinate set, the
/// node its own "+0x44". Some 40 of the record's 0x430 bytes are not written.
pub fn model_allocate(m: &mut Mem, name: u32) -> Ptr32<Model> {
    let p = m.si_malloc(node::SIZE);
    if p == 0 {
        return Ptr32::NULL;
    }
    let count = m[MODEL_COUNT].wrapping_add(1);
    m[MODEL_COUNT] = count;
    m.set_i16(p + 0x40, count as i16);
    m.set_u32(p, 0);
    m.set_u32(p + 0x2d4, 0);
    let text = if name == 0 {
        // sprintf("obj%1hd", count)
        let mut t = b"obj".to_vec();
        t.extend_from_slice(&si_mem::clib::decimal(count as i16 as i32));
        let scratch = m.local(0x50);
        m.put_cstr(scratch, &t);
        let copy = m.strdup(scratch);
        m.release(0x50);
        copy
    } else {
        m.strdup(name)
    };
    m.set_u32(p + node::NAME, text);
    for off in [
        0x18, 0x14, 0x10, 0x24, 0x20, 0x1c, 0x28, 0x2c, 0x30, 0x38, 0x3c, 0x50, 0xfc, 0xf8, 0xf4, 0x114, 0x110, 0x10c, 0x160, 0x15c, 0x158, 0x1c0, 0x1bc,
        0x1b8, 0x164, 0x168, 0x20c, 0x21c, 0x218, 0x224, 0x228, 0x22c, 0x42c, 0x230, 0x234, 0x2bc, 0x2c0, 0x2c4, 0x2c8, 0x58, 0x60, 0x64, 0x2d8, 0x2dc,
        0x48, 0x2cc, 0x2d0, 0x2e8, 0x330, 0x2ec, 0x31c, 0x320, 0x324, 0x328, 0x32c, 0x2f0, 0x2f4, 0x2f8, 0x300, 0x304, 0x308, 0x2fc, 0x30c, 0x310, 0x314,
        0x318, 0x33c, 0x344, 0x348, 0x34c, 0x358, 0x35c, 0x3e8, 0x3f0, 0x3fc, 0x408, 0x40c, 0x410, 0x414, 0x418, 0x41c, 0x428,
    ] {
        m.set_u32(p + off, 0);
    }
    // five vectors of zeros from 0x3ac
    for k in 0..15 {
        m.set_u32(p + 0x3ac + 4 * k, 0);
    }
    for off in [0xa8, 0xac, 0xb0, 0x100, 0x104, 0x108, 0x16c, 0x170, 0x174, 0x3a0, 0x3a4, 0x3a8, 0x3f4] {
        m.set_u32(p + off, ONE);
    }
    for (off, v) in [
        (0x08, 0u16), (0x0a, 0x3e), (0x3fa, 0x3e), (0x0c, 0), (0x34, 0), (0x36, 0), (0x204, 0), (0x206, 0), (0x208, 0), (0x216, 0), (0x2b8, 0), (0x2e4, 0),
        (0x4c, 0), (0x54, 0), (0x2e0, 0), (0x2e2, 0), (0x400, 1), (0x5c, 0), (0x350, 0), (0x352, 0), (0x3ec, 7), (0x3ee, 0), (0x3f8, 0), (0x402, 0),
        (0x404, 0), (0x406, 0), (0x420, 0),
    ] {
        m.set_u16(p + off, v);
    }
    for off in [0xb4, 0x118, 0x178, 0x1c4, 0x238, 0x278, 0x68, 0x360] {
        matrix_copy(m, SI_IDENTITE.addr(), p + off);
    }
    m.set_u32(p + 0x44, p);
    m.set_u32(p + 0x354, 0xffff_ffff);
    m.set_u32(p + node::NB_UV_SETS, 1);
    Ptr32::new(p)
}

/// [soft 0x08467038 SI_modelAddDefinition]: appends a geometry to a node's array of
/// definitions (the first one also sets the node's type; the array grows by one pointer
/// with `SI_realloc`). The node's current geometry follows the array's first entry if it
/// was that before (so the first definition added becomes the current one). Then the
/// node's name is entered again (`SI_modelChangeName`).
pub fn model_add_definition(m: &mut Mem, o: &mut dyn Outside, model: u32, kind: u32, geometry: u32) -> i32 {
    let first;
    if m.i16(model + node::NB_DEFINITIONS) == 0 {
        m.set_u32(model + node::TYPE, kind);
        let array = m.si_malloc(4);
        m.set_u32(model + node::DEFINITIONS, array);
        first = 0;
    } else {
        first = m.u32(m.u32(model + node::DEFINITIONS));
        let array = m.si_realloc(m.u32(model + node::DEFINITIONS), m.u16(model + node::NB_DEFINITIONS) as u32 * 4 + 4);
        m.set_u32(model + node::DEFINITIONS, array);
    }
    let slot = m.u32(model + node::DEFINITIONS).wrapping_add(m.u16(model + node::NB_DEFINITIONS) as u32 * 4);
    m.set_u32(slot, geometry);
    let n = m.i16(model + node::NB_DEFINITIONS).wrapping_add(1);
    m.set_i16(model + node::NB_DEFINITIONS, n);
    if m.u32(model + node::GEOMETRY) == first {
        let head = m.u32(m.u32(model + node::DEFINITIONS));
        m.set_u32(model + node::GEOMETRY, head);
    }
    o.call(m, &SI_MODEL_CHANGE_NAME, &[model, m.u32(model + node::NAME)]);
    1
}

/// [soft 0x0843a9b0 SI_klusterAllocate]: a cluster record of 0x20 bytes: name, count,
/// (unwritten), the array of indices, a state of 1, (unwritten), two zeros.
pub fn kluster_allocate(m: &mut Mem) -> u32 {
    let p = m.si_malloc(0x20);
    if p != 0 {
        for off in [0x00, 0x08, 0x04, 0x0c] {
            m.set_u32(p + off, 0);
        }
        m.set_u32(p + 0x10, 1);
        m.set_u32(p + 0x18, 0);
        m.set_u32(p + 0x1c, 0);
    }
    p
}

/// [soft 0x083eca24 modelBinaryReadNode]: one node of a hierarchy with everything under it.
///
/// `mode` 0 (and any value but 1) makes a new node (`SI_modelAllocate`) under the father
/// `model`; a son is read with the new node as father, a brother with the same father.
/// When the node's tokens are through, its local matrix and its state are brought up to
/// date (`SI_modelUpdateMatrix`, `SI_modelUpdateSonState`) and the short at +0xa cleared.
/// `mode` 1 reads the same data over the existing node `model` and its sons and brothers
/// and throws most of it away; the answer is the node, or null as soon as a son or brother
/// that the file has is missing.
///
/// What is kept where is in finding 0004's table and in the offsets of [`node`]. Points
/// the listing settles that the table does not say:
///
/// - The geometry a type's reader answers is kept in a local that nothing clears: a node
///   whose type has no geometry (a null, a joint) and whose list has token 3 hands
///   `SI_modelAddDefinition` whatever that part of the stack held, if it is not zero. The
///   port keeps the local on the image's stack and does the same.
/// - The map from the file's edges to the rebuilt ones (made by the mesh reader) lies in
///   another such local; it is freed at the end of a mesh node whenever it is not null,
///   also when the mesh had no edge list and the local was never set.
/// - A count read through the scratch short (3D textures, clusters, old user data) is
///   what the scratch short holds after the read, whether or not the read succeeded.
/// - After the tokens of a mesh node every polygon whose material index is negative or
///   not below the node's material count gets index 0.
pub fn model_binary_read_node(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, protos: Ptr32<Prototypes>, mode: i16) -> u32 {
    let n;
    if mode == 1 {
        n = model;
        if n == 0 {
            return 0;
        }
    } else {
        n = model_allocate(m, 0).addr();
        if n == 0 {
            return 0;
        }
        m.set_u32(n + node::FATHER, model);
    }
    // the frame: the scratch short with the token behind it, and the locals a callee
    // writes through a pointer
    // Each local is a slot of its own where the original has one (instruction listing:
    // the lengths of the user data on components and of their clusters, the user data's
    // count and a cluster's state, the shorts of token 0xc in mode 1 and of token 0x224,
    // the lengths of tokens 0xff1c and 0xff24 lie in different words of the frame); at the
    // end of a file a read that fails leaves each one as it was.
    const FRAME: u32 = 0x58;
    let frame = m.local(FRAME);
    if frame == 0 {
        // The image's stack is used up (the fault is recorded): sons and brothers nest
        // without a bound, and a flag read at the end of a file keeps the value the
        // scratch short had, so a truncated file can nest for ever. The original dies of
        // the same thing.
        return 0;
    }
    let (scratch, token_at) = (frame, frame + 2);
    let (edge_map_at, geometry_at, count_at, external_at) = (frame + 4, frame + 8, frame + 12, frame + 14);
    let (pointer_at, list_at_local, int_a, int_b, scratch4, byte_at, short_at, vector) = (frame + 0x10, frame + 0x14, frame + 0x18, frame + 0x1c, frame + 0x20, frame + 0x24, frame + 0x26, frame + 0x28);
    // (int_a, int_b: the length and count of the user data on components; these four are
    // the cluster's length and state, the length of token 0xff24, the short of 0x224)
    let (cluster_len, cluster_state, scratch4_uv, flag_224) = (frame + 0x34, frame + 0x38, frame + 0x3c, frame + 0x40);
    let table = protos.addr();
    let hierarchy = table.wrapping_add(4 * slot::HIERARCHY as u32);
    let mut index: i16 = 0;
    next_token(m, o, file, hierarchy, token_at, &mut index);
    let result = loop {
        let token = m.i16(token_at);
        // a short read into the scratch word
        let read_scratch = |m: &mut Mem, o: &mut dyn Outside| {
            byteswap_fread(m, o, scratch, 2, 1, file);
            m.i16(scratch)
        };
        match token {
            0 => {
                if m.i32(n + node::TYPE) == 4 {
                    let g = m.u32(n + node::GEOMETRY);
                    if g != 0 {
                        let mut left = m.i32(g + mesh::NB_POLYGONS);
                        let mut poly = m.u32(g + mesh::POLYGONS);
                        while 0 < left {
                            let index = m.i16(poly + mesh::POLYGON_MATERIAL);
                            if index < 0 || m.i16(n + node::NB_MATERIALS) <= index {
                                m.set_i16(poly + mesh::POLYGON_MATERIAL, 0);
                            }
                            left -= 1;
                            poly = poly.wrapping_add(mesh::POLYGON);
                        }
                    }
                    if m.u32(edge_map_at) != 0 {
                        m.free(m.u32(edge_map_at));
                        m.set_u32(edge_map_at, 0);
                    }
                } else if m.i32(n + node::TYPE) == 7 {
                    o.call(m, &SI_META_SYSTEM_RESOLVE_GROUP_NAME, &[m.u32(n + node::GEOMETRY)]);
                }
                if mode == 0 {
                    o.call(m, &SI_MODEL_UPDATE_MATRIX, &[n]);
                    o.call(m, &SI_MODEL_UPDATE_SON_STATE, &[model, n, 1]);
                    m.set_i16(n + node::STATE, 0);
                }
                break n;
            }
            0xd => {
                let kind = read_scratch(m, o) as i32;
                m.set_i32(n + node::ALGORITHM, kind);
                match kind {
                    2 | 0xc => {
                        o.call(m, &SI_ODYNAMIC_READ_BINARY_CHUNK, &[file, n]);
                        m.set_i32(n + node::ALGORITHM, if kind == 2 { 1 } else { 0 });
                    }
                    1 => {
                        let ik = reader(m, o, &SI_INV_KIN_READ_BINARY_CHUNK, &[file], |m, o| inv_kin_read_binary_chunk(m, o, file));
                        m.set_u32(n + node::INV_KIN, ik);
                    }
                    _ => {
                        file_skip_field(m, o, file, -1);
                    }
                }
            }
            2 => {
                let kind = read_scratch(m, o);
                if mode == 0 {
                    m.set_i32(n + node::TYPE, kind as i32);
                }
            }
            // the user data of vertices (0xff1e), polygons (0xff20), edges (0xff22): a
            // length, and if it is positive a count and that many lists
            -0xe2 | -0xe0 | -0xde => {
                byteswap_fread(m, o, int_a, 4, 1, file);
                if 0 < m.i32(int_a) {
                    byteswap_fread(m, o, int_b, 4, 1, file);
                    let count = m.i32(int_b);
                    let mut array = 0;
                    if 0 < count {
                        array = if alloc_fits(m, o, file, (count as u32).wrapping_mul(4), count as u32) { m.si_calloc(count as u32, 4) } else { 0 };
                        if array != 0 {
                            let mut i: i32 = 0;
                            while i < m.i32(int_b) {
                                user_data_read(m, o, file, Ptr32::new(array.wrapping_add(4 * i as u32)));
                                i += 1;
                            }
                        }
                    }
                    match token {
                        -0xe2 => m.set_u32(n + node::VERTEX_USER_DATA, array),
                        -0xe0 => m.set_u32(n + node::POLYGON_USER_DATA, array),
                        _ => {
                            m.set_u32(n + node::EDGE_USER_DATA, array);
                            if m.u32(edge_map_at) != 0 {
                                o.call(m, &SI_INDEX_EDGE_PROPAGATE, &[n, edge_map_at]);
                            }
                        }
                    }
                }
            }
            // 0xff1c: a list of user data in the new form, appended to the node's list
            -0xe4 => {
                m.set_u32(list_at_local, 0);
                byteswap_fread(m, o, scratch4, 4, 1, file);
                user_data_read(m, o, file, Ptr32::new(list_at_local));
                let mut last = m.u32(n + node::USER_DATA);
                if last != 0 {
                    loop {
                        let next = m.u32(last + 0x10);
                        if next == 0 {
                            break;
                        }
                        last = next;
                    }
                    let list = m.u32(list_at_local);
                    m.set_u32(last + 0x10, list);
                } else {
                    let list = m.u32(list_at_local);
                    m.set_u32(n + node::USER_DATA, list);
                }
            }
            // the clusters of the user data on vertices (0xff1d), polygons (0xff1f) and
            // edges (0xff21): a length, and if it is positive one byte, a count, that
            // many ints (twice as many for the vertices of a face) and a state
            -0xe3 | -0xe1 | -0xdf => {
                byteswap_fread(m, o, cluster_len, 4, 1, file);
                if 0 < m.i32(cluster_len) {
                    let k = kluster_allocate(m);
                    if k != 0 {
                        byteswap_fread(m, o, byte_at, 1, 1, file);
                        byteswap_fread(m, o, k + 4, 4, 1, file);
                        let count = if token == -0xe3 && m.i32(n + node::TYPE) == 2 { m.i32(k + 4).wrapping_mul(2) } else { m.i32(k + 4) };
                        let bytes = (count as u32).wrapping_mul(4);
                        let indices = if alloc_fits(m, o, file, bytes, count as u32) { m.si_malloc(bytes) } else { 0 };
                        m.set_u32(k + 0xc, indices);
                        byteswap_fread(m, o, indices, 4, count as u32, file);
                        byteswap_fread(m, o, cluster_state, 4, 1, file);
                        let state = m.u32(cluster_state);
                        m.set_u32(k + 0x10, state);
                        match token {
                            -0xe3 => m.set_u32(n + node::VERTEX_CLUSTER, k),
                            -0xe1 => m.set_u32(n + node::POLYGON_CLUSTER, k),
                            _ => m.set_u32(n + node::EDGE_CLUSTER, k),
                        }
                    }
                }
            }
            // 0xff24: for each texture coordinate set after the first, the identifiers
            // of the textures that use it
            -0xdc => {
                byteswap_fread(m, o, scratch4_uv, 4, 1, file);
                if 1 < m.i32(n + node::NB_UV_SETS) {
                    let sets = m.i32(n + node::NB_UV_SETS) - 1;
                    let array = if alloc_fits(m, o, file, (sets as u32).wrapping_mul(8), sets as u32) { m.si_calloc(sets as u32, 8) } else { 0 };
                    m.set_u32(n + node::UV_DOMAINS, array);
                }
                let mut i: i16 = 0;
                while (i as i32) < m.i32(n + node::NB_UV_SETS).wrapping_sub(1) {
                    let record = m.u32(n + node::UV_DOMAINS).wrapping_add(8 * i as i32 as u32);
                    byteswap_fread(m, o, record + 4, 4, 1, file);
                    if m.i32(record + 4) != 0 {
                        let count = m.i32(record + 4);
                        let ids = if alloc_fits(m, o, file, (count as u32).wrapping_mul(4), count as u32) { m.si_calloc(count as u32, 4) } else { 0 };
                        m.set_u32(record, ids);
                        let mut k: i16 = 0;
                        while (k as i32) < m.i32(record + 4) {
                            let at = m.u32(record).wrapping_add(4 * k as i32 as u32);
                            byteswap_fread(m, o, at, 4, 1, file);
                            k = k.wrapping_add(1);
                        }
                    }
                    i = i.wrapping_add(1);
                }
            }
            1 => {
                if mode == 1 {
                    let s = string_binary_read(m, o, file);
                    if s != 0 {
                        m.free(s);
                    }
                } else {
                    if m.u32(n + node::NAME) != 0 {
                        m.free(m.u32(n + node::NAME));
                        m.set_u32(n + node::NAME, 0);
                    }
                    let s = string_binary_read(m, o, file);
                    m.set_u32(n + node::NAME, s);
                }
            }
            7 => {
                if mode == 1 {
                    byteswap_fread(m, o, count_at, 2, 1, file);
                    let mut i: i16 = 0;
                    while i < m.i16(count_at) {
                        byteswap_fread(m, o, external_at, 2, 1, file);
                        if m.i16(external_at) == 0 {
                            let material = material_binary_read(m, o, file, protos);
                            m.set_u32(pointer_at, material.addr());
                            o.call(m, &SI_MATERIAL_DISPOSE, &[n, pointer_at]);
                        } else {
                            let s = string_binary_read(m, o, file);
                            if s != 0 {
                                m.free(s);
                            }
                        }
                        i += 1;
                    }
                } else {
                    byteswap_fread(m, o, n + node::NB_MATERIALS, 2, 1, file);
                    if 0 < m.i16(n + node::NB_MATERIALS) {
                        let array = m.si_calloc(m.i16(n + node::NB_MATERIALS) as u32, 4);
                        m.set_u32(n + node::MATERIALS, array);
                        let mut i: i16 = 0;
                        while i < m.i16(n + node::NB_MATERIALS) {
                            byteswap_fread(m, o, external_at, 2, 1, file);
                            if m.i16(external_at) == 0 {
                                let material = material_binary_read(m, o, file, protos);
                                let at = m.u32(n + node::MATERIALS).wrapping_add(4 * i as u32);
                                m.set_u32(at, material.addr());
                                let mut t = m.u32(m.u32(at) + mat::TEXTURES);
                                while t != 0 {
                                    o.call(m, &SI_TXT2D_SET_WRAP, &[n, t]);
                                    t = m.u32(t + lclmap::NEXT);
                                }
                            } else {
                                let s = string_binary_read(m, o, file);
                                let at = m.u32(n + node::MATERIALS).wrapping_add(4 * i as u32);
                                m.set_u32(at, 0);
                                if s != 0 {
                                    m.free(s);
                                }
                            }
                            i += 1;
                        }
                    }
                }
            }
            4 | 6 => {
                let at = if mode == 1 {
                    vector
                } else if token == 4 {
                    n + node::SCALING
                } else {
                    n + node::TRANSLATION
                };
                vector_fread(m, o, at, 0xc, 1, file);
            }
            5 => {
                if mode != 1 {
                    vector_fread(m, o, n + node::ROTATION, 0xc, 1, file);
                    o.call(m, &SI_MATRIX_XYZ_ROTATION, &[n + node::ROTATION, n + node::ROTATION_MATRIX]);
                } else {
                    vector_fread(m, o, vector, 0xc, 1, file);
                }
            }
            3 => {
                let geometry = match m.i32(n + node::TYPE) {
                    1 => Some(reader(m, o, &PATCH_BINARY_READ, &[file, table], |m, o| patch_binary_read(m, o, file, protos).addr())),
                    2 => Some(reader(m, o, &FACE_BINARY_READ, &[file, table], |m, o| face_binary_read(m, o, file, protos).addr())),
                    4 => Some(mesh_binary_read(m, o, file, protos, n, edge_map_at, 1).addr()),
                    6 => Some(reader(m, o, &SPLINE_BINARY_READ, &[file, table], |m, o| spline_binary_read(m, o, file, protos).addr())),
                    7 => Some(reader(m, o, &META_SYSTEM_BINARY_READ, &[file, table, n], |m, o| meta_system_binary_read(m, o, file, protos, n))),
                    8 => Some(reader(m, o, &METABALL_BINARY_READ, &[file, table, n], |m, o| metaball_binary_read(m, o, file, protos, n))),
                    9 => Some(reader(m, o, &NURBS_CRV_BINARY_READ, &[file, table], |m, o| nurbs_crv_binary_read(m, o, file, protos).addr())),
                    10 => Some(reader(m, o, &NURBS_SRF_BINARY_READ, &[file, table], |m, o| nurbs_srf_binary_read(m, o, file, protos).addr())),
                    -1 | 0 | 3 | 5 => None,
                    _ => {
                        file_skip_field(m, o, file, -1);
                        None
                    }
                };
                if let Some(g) = geometry {
                    m.set_u32(geometry_at, g);
                }
                // (see above: for a type without geometry this is what the stack held)
                if m.u32(geometry_at) != 0 {
                    model_add_definition(m, o, n, m.u32(n + node::TYPE), m.u32(geometry_at));
                }
            }
            10 | 0xb => {
                if read_scratch(m, o) != 0 {
                    let link = if token == 10 { node::SON } else { node::BROTHER };
                    if mode == 1 {
                        if model_binary_read_node(m, o, file, m.u32(n + link), protos, 1) == 0 {
                            break 0;
                        }
                    } else {
                        let father = if token == 10 { n } else { model };
                        let child = model_binary_read_node(m, o, file, father, protos, mode);
                        m.set_u32(n + link, child);
                    }
                }
            }
            8 => {
                byteswap_fread(m, o, n + node::NB_TEXTURES, 2, 1, file);
                let mut last = 0u32;
                let mut i: i16 = 0;
                while i < m.i16(n + node::NB_TEXTURES) {
                    byteswap_fread(m, o, external_at, 2, 1, file);
                    if m.i16(external_at) == 0 {
                        let t = texture_binary_read(m, o, file, protos);
                        m.set_u32(pointer_at, t.addr());
                        if mode == 1 {
                            o.call(m, &SI_TXT2D_DISPOSE, &[n, pointer_at]);
                            m.set_u32(pointer_at, 0);
                        } else {
                            o.call(m, &SI_TXT2D_SET_WRAP, &[n, m.u32(pointer_at)]);
                        }
                    } else {
                        let s = string_binary_read(m, o, file);
                        m.set_u32(pointer_at, 0);
                        if s != 0 {
                            m.free(s);
                        }
                    }
                    let t = m.u32(pointer_at);
                    if t != 0 {
                        if last == 0 {
                            m.set_u32(n + node::TEXTURES, t);
                            m.set_u32(n + node::TEXTURE_CURRENT, t);
                        } else {
                            m.set_u32(last + lclmap::NEXT, t);
                        }
                        last = t;
                    }
                    i += 1;
                }
            }
            9 => {
                read_scratch(m, o);
                let mut i: i16 = 0;
                while i < m.i16(scratch) {
                    byteswap_fread(m, o, external_at, 2, 1, file);
                    let t = if m.i16(external_at) == 0 {
                        texture3d_binary_read(m, o, file, protos).addr()
                    } else {
                        let s = string_binary_read(m, o, file);
                        if s != 0 {
                            m.free(s);
                        }
                        0
                    };
                    if t != 0 {
                        if mode == 1 {
                            o.call(m, &SI_TXT3D_FREE, &[n, t]);
                        } else if i == 0 {
                            m.set_u32(n + node::TEXTURE3D, t);
                        } else {
                            if m.u32(t + t3d::NAME) != 0 {
                                m.free(m.u32(t + t3d::NAME));
                                m.set_u32(t + t3d::NAME, 0);
                            }
                            m.free(t);
                        }
                    }
                    i += 1;
                }
            }
            0xc => {
                if mode == 1 {
                    byteswap_fread(m, o, short_at, 2, 1, file);
                } else {
                    byteswap_fread(m, o, n + node::SHAPE_INTERPOLATION, 2, 1, file);
                    if m.i16(n + node::SHAPE_INTERPOLATION) == -1 {
                        m.set_i16(n + node::SHAPE_INTERPOLATION, 0);
                    }
                }
            }
            0xe => {
                if read_scratch(m, o) != 0 {
                    byteswap_fread(m, o, external_at, 2, 1, file);
                    let d = reader(m, o, &DYNAMATTER_READ_BINARY_CHUNK, &[file, table], |m, o| dynamatter_read_binary_chunk(m, o, file, protos));
                    m.set_u32(n + node::DYNA_MATTER, d);
                }
            }
            0xf | 0x11 => {
                let kind = read_scratch(m, o) as i32;
                m.set_i32(n + node::ENVELOPE_TYPE, kind);
                if read_scratch(m, o) != 0 {
                    if token == 0xf {
                        let e = o.call(m, &SI_O_ENVELOP_READ_BINARY_CHUNK, &[file]);
                        m.set_u32(n + node::ENVELOPE, e);
                    } else {
                        let e = reader(m, o, &ENVELOP_READ_BINARY_CHUNK, &[file, n, table], |m, o| envelop_read_binary_chunk(m, o, file, n, protos));
                        m.set_u32(n + node::ENVELOPE, e);
                        // the double at 0x0865bc38: the envelope of a chain root read from
                        // a file below 3.49089 is dropped
                        if (m[FILE_VERSION] as f64) < 3.49089 && m.i32(n + node::TYPE) == 0 && (m.u32(n + node::ALGORITHM).wrapping_sub(1)) < 2 {
                            reader(m, o, &SI_ENVELOP_INFO_DISPOSE, &[n + node::ENVELOPE], |m, _| {
                                envelop_info_dispose(m, n + node::ENVELOPE);
                                0
                            });
                        }
                    }
                }
            }
            0x10 => {
                let v = read_scratch(m, o) as i32;
                m.set_i32(n + node::ALGORITHM, v);
            }
            0x12 => {
                if read_scratch(m, o) != 0 {
                    let mut chain_root = 0;
                    if m.i32(n + node::TYPE) == 0 {
                        chain_root = (m.u32(n + node::ALGORITHM).wrapping_sub(1) < 2) as u32;
                    }
                    let d = o.call(m, &SI_DYNACHNVAR_ALLOCATE, &[chain_root]);
                    m.set_u32(n + node::DYNA_CHAIN, d);
                    let k = o.call(m, &SI_MODEL_KINE_STATE_ALLOCATE, &[]);
                    m.set_u32(n + node::KINE_STATE, k);
                }
            }
            0x13 => {
                if read_scratch(m, o) != 0 {
                    reader(m, o, &DYNAJNTPROP_READ_BINARY_CHUNK, &[file, n, table], |m, o| {
                        dynajntprop_read_binary_chunk(m, o, file, n, protos);
                        0
                    });
                }
            }
            0x14 => {
                if read_scratch(m, o) == 0 {
                    if m[SI_ED_SUMATRA_TO_3X] != 0 {
                        let ik = reader(m, o, &SI_INV_KIN_ALLOCATE, &[], |m, _| inv_kin_allocate(m));
                        m.set_u32(n + node::INV_KIN, ik);
                    }
                } else {
                    let ik = reader(m, o, &SI_INV_KIN_READ_BINARY_CHUNK, &[file], |m, o| inv_kin_read_binary_chunk(m, o, file));
                    m.set_u32(n + node::INV_KIN, ik);
                }
            }
            0x15 => {
                read_scratch(m, o);
                let mut i: i16 = 0;
                while i < m.i16(scratch) {
                    reader(m, o, &CLUSTERS_READ_BINARY_CHUNK, &[file, n, table], |m, o| {
                        clusters_read_binary_chunk(m, o, file, n, protos);
                        0
                    });
                    i = i.wrapping_add(1);
                }
            }
            0x16 => old_user_data(m, o, file, n, scratch),
            0x17 | 0x18 | 0x1b | 0x21b | 0x221 | 0x222 | 0x223 => {
                let off = match token {
                    0x17 => node::VISIBILITY,
                    0x18 => node::MOTION_BLUR,
                    0x221 => node::TOKEN_221,
                    0x222 => node::TOKEN_222,
                    0x223 => node::TOKEN_223,
                    _ => node::GARBAGE_MATTE,
                };
                byteswap_fread(m, o, n + off, 2, 1, file);
            }
            0x19 => {
                if read_scratch(m, o) != 0 {
                    if m.u32(n + node::INITIAL_SRT) == 0 {
                        let block = m.si_calloc(1, 0x24);
                        m.set_u32(n + node::INITIAL_SRT, block);
                    }
                    for k in 0..3 {
                        let at = m.u32(n + node::INITIAL_SRT).wrapping_add(0xc * k);
                        vector_fread(m, o, at, 0xc, 1, file);
                    }
                }
            }
            0x1a | 0x41a => {
                float_fread(m, o, n + node::MATTE, 4, 1, file);
            }
            0x224 => {
                byteswap_fread(m, o, flag_224, 2, 1, file);
                if m.i16(flag_224) == 1 {
                    m.or_u8(n + node::FLAGS, 8);
                }
            }
            _ => {
                file_skip_field(m, o, file, token);
            }
        }
        next_token(m, o, file, hierarchy, token_at, &mut index);
    };
    m.release(FRAME);
    result
}

/// The byte-order fixers of `byteswap.c` for a custom effect's data: the ports of
/// `si-app` on the image, unless the outside world answers for the routine itself
/// ([`Outside::stands_in`]: a check against a vector recorded while it was a stand-in).
pub(crate) fn fix_custom(m: &mut Mem, o: &mut dyn Outside, routine: &'static Routine, node: u32, dir: u32) {
    if o.stands_in(routine) {
        o.call(m, routine, &[node, dir]);
        return;
    }
    let dir = dir as i16;
    match routine.addr {
        0x083f966c => si_app::image::fileswap::fix_custom_header_old(m, node, dir),
        0x083f96e4 => si_app::image::fileswap::fix_custom_header(m, node, dir),
        0x083f9730 => si_app::image::fileswap::fix_custom_dso_fcurves(m, node, dir),
        _ => si_app::image::fileswap::fix_custom_fcurves(m, node, dir),
    }
}

/// One of the readers of `binary.c` that the node reader calls: the port on the image,
/// unless the outside world answers for the routine itself ([`Outside::stands_in`]: a
/// check against a vector recorded while the routine was a stand-in).
pub(crate) fn reader(m: &mut Mem, o: &mut dyn Outside, routine: &'static Routine, args: &[u32], port: impl FnOnce(&mut Mem, &mut dyn Outside) -> u32) -> u32 {
    if o.stands_in(routine) {
        o.call(m, routine, args)
    } else {
        port(m, o)
    }
}

/// Token 22 of a node: user data in the old form, appended to the node's list. A short
/// count (through the scratch short); per block four bytes of tag, an int size, the data.
///
/// The tag goes into a zeroed block of 5 bytes. In a file below version 3.50009 (the
/// double at 0x0865bc40) a tag that is not one of `CUSA` to `CUSD` is byte-reversed and
/// replaced by a block of 6 bytes holding `_` and the reversed tag, and the data is read
/// as one item of `size` bytes, which the swapping read reverses for 2, 4 and 8. The data
/// of a custom effect (`CUSA` to `CUSD`) is then put into this machine's byte order by
/// the routines of `byteswap.c`, by the file's version and the tag.
fn old_user_data(m: &mut Mem, o: &mut dyn Outside, file: u32, n: u32, scratch: u32) {
    byteswap_fread(m, o, scratch, 2, 1, file);
    let mut last = m.u32(n + node::USER_DATA);
    if last != 0 {
        loop {
            let next = m.u32(last + 0x10);
            if next == 0 {
                break;
            }
            last = next;
        }
    }
    let mut i: i16 = 0;
    while i < m.i16(scratch) {
        let block = m.si_calloc(1, 0x14);
        let tag = m.si_calloc(5, 1);
        m.set_u32(block, tag);
        byteswap_fread(m, o, tag, 1, 4, file);
        let text = m.cstr(m.u32(block));
        let custom = text.len() == 4 && [&b"CUSA"[..], b"CUSB", b"CUSC", b"CUSD"].contains(&&text[..]);
        let old = (m[FILE_VERSION] as f64) < 3.50009;
        if old && !custom {
            swap4byte(m, m.u32(block));
            let renamed = m.si_calloc(6, 1);
            if renamed != 0 {
                // sprintf("%c%s", '_', tag)
                let mut t = vec![b'_'];
                t.extend_from_slice(&m.cstr(m.u32(block)));
                m.put_cstr(renamed, &t);
                if m.u32(block) != 0 {
                    m.free(m.u32(block));
                    m.set_u32(block, 0);
                }
                m.set_u32(block, renamed);
            }
        }
        m.set_i16(block + 4, 1);
        byteswap_fread(m, o, block + 8, 4, 1, file);
        let size = m.u32(block + 8);
        let data = if alloc_fits(m, o, file, size, size) { m.si_malloc(size) } else { 0 };
        m.set_u32(block + 0xc, data);
        if old {
            byteswap_fread(m, o, data, size, 1, file);
        } else {
            byteswap_fread(m, o, data, 1, size, file);
        }
        if custom {
            if (m[FILE_VERSION] as f64) < 3.49089 {
                fix_custom(m, o, &SI_FIX_CUSTOM_HEADER_OLD, block, 1);
            } else {
                match &m.cstr(m.u32(block))[..] {
                    b"CUSA" => {
                        fix_custom(m, o, &SI_FIX_CUSTOM_HEADER, block, 1);
                    }
                    b"CUSD" => {
                        fix_custom(m, o, &SI_FIX_CUSTOM_DSO_FCURVES, block, 1);
                    }
                    b"CUSC" => {
                        fix_custom(m, o, &SI_FIX_CUSTOM_FCURVES, block, 1);
                    }
                    _ => {}
                }
            }
        }
        if last == 0 {
            m.set_u32(n + node::USER_DATA, block);
        } else {
            m.set_u32(last + 0x10, block);
        }
        i = i.wrapping_add(1);
        last = block;
    }
}

/// [soft 0x083ec740 SI_fdmodelBinaryRead]: a model from a stream: the header (a refused
/// one ends it: null), `binary.c`'s `fileVersion`, the prototype block if the tag is
/// `PROT`, and the root node if the tag is `HRCH`: read over `model` for `mode` 1 (the
/// answer is then `model`), otherwise a new hierarchy without a father. Then the envelopes
/// are rebuilt (`SI_envelopRebuild`, told whether the file is below version 2.6, the
/// double at 0x0865bc28) and the prototype table freed.
pub fn fdmodel_binary_read(m: &mut Mem, o: &mut dyn Outside, file: u32, model: u32, mode: i16) -> u32 {
    const FRAME: u32 = 12;
    let frame = m.local(FRAME);
    let (protos_at, tag_at, version_at) = (frame, frame + 4, frame + 8);
    let magic = read_bin_header(m, o, file, version_at, 0);
    m.set_i32(tag_at, magic);
    let result;
    if magic < 0 {
        result = 0;
    } else {
        m[FILE_VERSION] = m.f32(version_at);
        let protos = prototype_allocate(m);
        m.set_u32(protos_at, protos.addr());
        byteswap_fread(m, o, tag_at, 4, 1, file);
        if m.u32(tag_at) == 0x5052_4f54 {
            prototype_binary_read(m, o, file, protos);
            byteswap_fread(m, o, tag_at, 4, 1, file);
        }
        let mut root = 0;
        if m.u32(tag_at) == 0x4852_4348 {
            if mode == 1 {
                model_binary_read_node(m, o, file, model, protos, 1);
                root = model;
            } else {
                root = model_binary_read_node(m, o, file, 0, protos, mode);
            }
        }
        let old = (m.f32(version_at) as f64) < 2.6;
        o.call(m, &SI_ENVELOP_REBUILD, &[root, 1, old as u32]);
        prototype_dispose(m, Ptr32::new(protos_at));
        result = root;
    }
    m.release(FRAME);
    result
}
