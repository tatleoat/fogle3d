// SPDX-License-Identifier: MIT
//! Layout of a model node (`__SI_Model`) and the routines of `model_b.c` that create one and
//! link it into a hierarchy.
//!
//! A model file is a tree of these nodes. Each node has a kind (null, patch, face, mesh,
//! joint, spline, metaball system, metaball, NURBS curve, NURBS surface), an array of
//! *definitions* (its geometry in each stored shape; `definition` points at the current one),
//! its own scaling, rotation and translation, the matrices derived from them, and links to
//! its father, next brother and first son.
//!
//! Names in the evidence: "the dump" is `SI_modelDump`, which prints fields with labels;
//! "token N" is the N-th token of a hierarchy node in a model file, in the order
//! `SI_modelBinaryWriteNode` writes them.

use crate::geom::Vector;
use crate::scene::{LclMap, Material, Texture3D};
use crate::{layout, CStr32, Matrix, Opaque, Ptr32, Zero, IDENTITY};

/// Records the model points to that are not laid out in this crate.
pub enum Lattice {}
pub enum SplFitting {}
pub enum PtchFitting {}
pub enum Kluster {}
pub enum Cluster {}
pub enum UserNode {}
pub enum InvKin {}
pub enum EnvSkelJnt {}
pub enum CompInfoList {}
pub enum KineInfo {}
pub enum DynaMatterInfo {}
pub enum DynaJntInfo {}
pub enum DynaConstrInfo {}
pub enum DynaJntVars {}
pub enum DynaChnVars {}

/// Values of `Model::kind` (the SDK's `SAA_ModelType`; the resource file's `MdlType`).
pub const MNILL: i32 = 0;
pub const MPTCH: i32 = 1;
pub const MFACE: i32 = 2;
pub const MSMSH: i32 = 4;
pub const MJNT: i32 = 5;
pub const MSPLN: i32 = 6;
pub const MMETA: i32 = 7;
pub const MBALL: i32 = 8;
pub const MNCRV: i32 = 9;
pub const MNSRF: i32 = 10;

layout! {
    /// One node of a model hierarchy (`__SI_Model`, also demangled as `SI_Model`).
    pub struct Model = "__SI_Model", size 0x430,
        "SI_modelAllocate allocates 0x430 bytes and its last store is the word at +0x42c" {
        0x000 block_00: Ptr32<Opaque>, Unknown, "A heap block freed with the node in SI_modelDisposeNode; null at creation.";
        0x004 name: CStr32, Read, "Labelled name in the dump; token 1; SI_modelAllocate duplicates its argument, or the text obj<n>.";
        0x008 flags_08: u16, Unknown, "Bit flags, 0 at creation; SI_envSkelSetFlag, SI_polySelection and SI_propagateTransform set bits in both bytes.";
        0x00a update_flags: u16, Named, "0x3e at creation; every routine that sets scaling, rotation or translation ORs bits in; the readers and writers clear it once the matrices are rebuilt.";
        0x00c bbox_valid: i16, Named, "SI_bboxAttrGetValue tests it for zero before returning the bounding box; 0 at creation.";
        0x00e pad_0e: [u8; 2], Unknown, "No access found.";
        0x010 bbox_min: Vector, Read, "Attributes 274 to 276 (BBox minimum).";
        0x01c bbox_max: Vector, Read, "Attributes 277 to 279 (BBox maximum).";
        0x028 unk_28: [f32; 2], Unknown, "Two floats, 0 at creation, written by SI_modelComputeBoundingCP.";
        0x030 kind: i32, Read, "Labelled type in the dump; token 2; compared with 1, 2, 4, 6, 7, 8, 9, 10 to choose the geometry routines.";
        0x034 nb_defs: u16, Read, "Labelled nbdef: number of definitions.";
        0x036 cur_def: u16, Read, "Labelled curdef: index of the current definition.";
        0x038 defs: Ptr32<Ptr32<Opaque>>, Read, "Labelled data: one pointer per definition; the writer saves the first as token 3.";
        0x03c org_def: Ptr32<Opaque>, Named, "Written by SI_modelCopyOrgDef and by the shape and cluster readers.";
        0x040 serial: i16, Read, "Value of a counter at allocation, the number in the default name.";
        0x042 pad_42: [u8; 2], Unknown, "No access found.";
        0x044 latdep: Ptr32<Model>, Read, "Labelled latdep; the node itself at creation; the writer uses its global matrix to save geometry in another node's space.";
        0x048 definition: Ptr32<Opaque>, Read, "Labelled definition: the current geometry (SI_Mesh, SI_Patch, SI_Face, SI_Spline, SI_NurbsSrf, ... by kind).";
        0x04c nb_lattices0: u16, Read, "Length of lattices0.";
        0x04e pad_4e: [u8; 2], Unknown, "No access found.";
        0x050 lattices0: Ptr32<Ptr32<Lattice>>, Read, "Lattices of set 0, each disposed with SI_latticeDispose.";
        0x054 nb_lattices1: u16, Read, "Length of lattices1.";
        0x056 pad_56: [u8; 2], Unknown, "No access found.";
        0x058 lattices1: Ptr32<Ptr32<Lattice>>, Read, "Lattices of set 1.";
        0x05c unk_5c: i16, Unknown, "0 at creation; SI_latticeCopyFromSetup and si_modelSetLattice write it.";
        0x05e pad_5e: [u8; 2], Unknown, "No access found.";
        0x060 spline_fit: [Ptr32<SplFitting>; 2], Read, "Spline deformations, reset with SI_modelSplineFittingReset(model, 0 or 1).";
        0x068 def_matrix: Matrix, Named, "Identity at creation; SI_modelUpdateSonState stores the node's global matrix, or a product with it, here, and SI_modelReadjustDefinition multiplies the definition by it.";
        0x0a8 hscl: Vector, Read, "Labelled hscl; 1, 1, 1 at creation.";
        0x0b4 hrot: Matrix, Read, "Labelled hrot; identity at creation.";
        0x0f4 htrs: Vector, Read, "Labelled htrs.";
        0x100 scal: Vector, Read, "Labelled scal; token 4; attributes 1 to 3 (local scaling).";
        0x10c vrot: Vector, Read, "Labelled vrot; token 5; attributes 4 to 6 (local rotation angles).";
        0x118 mrot: Matrix, Read, "Labelled mrot: the rotation as a matrix.";
        0x158 trans: Vector, Read, "Labelled trans; token 6; attributes 139 to 141 (local translation).";
        0x164 unk_164: [f32; 2], Unknown, "0 at creation; written by doRotSGI.";
        0x16c gscl: Vector, Read, "Labelled gscl; attributes 1 to 3 when asked in global terms.";
        0x178 grot: Matrix, Read, "Labelled grot.";
        0x1b8 gtrs: Vector, Read, "Labelled gtrs; attributes 139 to 141 in global terms.";
        0x1c4 gmat: Matrix, Read, "Labelled gmat: the node's global matrix.";
        0x204 unk_204: i16, Unknown, "0 at creation; pushMaterial and SI_modelClearRenderData write it.";
        0x206 nb_materials: i16, Read, "Length of materials; written as the count of token 7.";
        0x208 cur_material: i16, Named, "SI_materialSetCurrent.";
        0x20a pad_20a: [u8; 2], Unknown, "No access found.";
        0x20c materials: Ptr32<Ptr32<Material>>, Read, "The node's materials; a polygon's material number indexes it.";
        0x210 unk_210: Ptr32<Opaque>, Unknown, "Written by materialEditor and si_cusTransferMaterial. Not set at creation.";
        0x214 unk_214: u16, Unknown, "Written by mdlInitTxtCnt. Not set at creation.";
        0x216 unk_216: i16, Unknown, "0 at creation; mdlInitTxtCnt, SI_texture2Dgdisassociate.";
        0x218 texture_218: Ptr32<LclMap>, Unknown, "A 2D texture pointer beside the list head; SI_txtCopyFromSetup writes it.";
        0x21c textures: Ptr32<LclMap>, Read, "First 2D texture of the node; the list continues through the texture's own link; token 8.";
        0x220 unk_220: Ptr32<Opaque>, Unknown, "Written by si_cusTransferTexture. Not set at creation.";
        0x224 texture3d: Ptr32<Texture3D>, Read, "The node's 3D texture; token 9.";
        0x228 ren_count0: i32, Named, "Render data rebuilt by SI_collect and cleared by SI_modelClearRenderData: a count.";
        0x22c ren_vertices: Ptr32<Opaque>, Named, "Render vertices (allocExtraRenVertices); freed with the node.";
        0x230 ren_count1: i32, Named, "Render data: a second count.";
        0x234 ren_triangles: Ptr32<Opaque>, Named, "Render triangles; freed with the node.";
        0x238 mat_238: Matrix, Unknown, "Identity at creation; no other access found.";
        0x278 mat_278: Matrix, Unknown, "Identity at creation; no other access found.";
        0x2b8 visible: i16, Read, "SI_modelSetHiddenParam: 1 when the element's wire type is 'visible', else 0.";
        0x2ba pad_2ba: [u8; 2], Unknown, "No access found.";
        0x2bc father: Ptr32<Model>, Read, "Labelled father.";
        0x2c0 brother: Ptr32<Model>, Read, "Labelled brother: next son of the same father.";
        0x2c4 son: Ptr32<Model>, Read, "Labelled son: first son.";
        0x2c8 path: Ptr32<Opaque>, Named, "SI_modelSetPath.";
        0x2cc unk_2cc: [u32; 2], Unknown, "0 at creation; no other access found.";
        0x2d4 shape_data: Ptr32<Opaque>, Named, "Written by SI_shapeBinaryRead; freed with the node.";
        0x2d8 patch_fit: [Ptr32<PtchFitting>; 2], Read, "Patch deformations, reset with SI_modelPatchFittingReset(model, 0 or 1).";
        0x2e0 shape_interp: i16, Named, "Token 12 (shape interpolation).";
        0x2e2 unk_2e2: i16, Unknown, "0 at creation; SI_shapeBinaryRead writes it.";
        0x2e4 unk_2e4: i16, Unknown, "0 at creation.";
        0x2e6 pad_2e6: [u8; 2], Unknown, "No access found.";
        0x2e8 waves: Ptr32<Opaque>, Named, "SI_waveAddMdl, SI_modelUnlinkAllWaves.";
        0x2ec algo_type: i32, Read, "Attribute 178 (AlgoType); token 16. In memory 2 and 12 exist beside the file's values.";
        0x2f0 kine_info: Ptr32<KineInfo>, Named, "Attributes 221 to 232 (velocities and accelerations) are read through it.";
        0x2f4 dyna_matter: Ptr32<DynaMatterInfo>, Read, "Attributes 179 to 181 (elasticity, roughness) are read through it; token 14.";
        0x2f8 dyna_jnt: Ptr32<DynaJntInfo>, Named, "Token 19 (joint properties) is present when it is not null.";
        0x2fc dyna_constr: Ptr32<DynaConstrInfo>, Named, "SI_conslistAddConstraint.";
        0x300 dyna_jnt_vars: Ptr32<DynaJntVars>, Named, "SI_modelDynaInfoCopy.";
        0x304 dyna_chn_vars: Ptr32<DynaChnVars>, Named, "Attribute 182 (RootCons) is read through it; token 18 says whether it is null.";
        0x308 unk_308: Ptr32<Opaque>, Unknown, "0 at creation; freed by SI_modelDynaInfoDispose.";
        0x30c dyna_links: [Ptr32<Model>; 3], Named, "Three model pointers, null at creation, set by SI_dynaPhyPropsSetNewRoot and SI_dynamicChainsUpdate.";
        0x318 inv_kin: Ptr32<InvKin>, Read, "Disposed with SI_invKinDispose; token 20.";
        0x31c envelop: i32, Named, "First value of token 17, written as 16 bits.";
        0x320 unk_320: Ptr32<Opaque>, Unknown, "0 at creation; written by SI_modelCopyNode and skeTubingUpdate.";
        0x324 env_skel_jnt: Ptr32<EnvSkelJnt>, Read, "Disposed with SI_envelopInfoDispose; second part of token 17.";
        0x328 init_srt: Ptr32<Vector>, Read, "Three vectors written as token 25 (initial transformation) when not null; freed with the node.";
        0x32c unk_32c: Ptr32<Opaque>, Unknown, "Freed with the node; SI_envSkelResetFlag writes it.";
        0x330 extra_tcoords: Ptr32<Opaque>, Named, "allocExtraTCoord; cleared by SI_modelClearRenderData.";
        0x334 unk_334: i32, Unknown, "Written by SI_textureVerts. Not set at creation.";
        0x338 unk_338: i16, Unknown, "Written by isMoving. Not set at creation.";
        0x33a pad_33a: [u8; 2], Unknown, "No access found.";
        0x33c unk_33c: Ptr32<Opaque>, Unknown, "0 at creation; saveInitialPositions and the mental ray writers write it.";
        0x340 unk_340: f32, Unknown, "Written by isMoving. Not set at creation.";
        0x344 cluster: Ptr32<Cluster>, Read, "Cluster animation, disposed with SI_clusterDispose.";
        0x348 klusters: Ptr32<Kluster>, Named, "SI_klusterAddToList, SI_klusterDisposeAll.";
        0x34c user_data: Ptr32<UserNode>, Read, "List of user data blocks, linked through their word at +0x10; token 22.";
        0x350 unk_350: i16, Unknown, "0 at creation; read by SI_graflistDeleteGrafnode.";
        0x352 unk_352: i16, Unknown, "0 at creation; read by SI_isModelDeformed.";
        0x354 elem_id: i32, Read, "Index of the node's element in the MODELS chapter (SI_elemdictAddModel stores what SI_elemdictAddElem returns); -1 at creation.";
        0x358 unk_358: [u32; 2], Unknown, "0 at creation; no other access found.";
        0x360 cscl_matrix: Matrix, Named, "Identity at creation; filled by SI_modelUpdateCScalingInfo.";
        0x3a0 cscl: Vector, Named, "1, 1, 1 at creation; SI_modelUpdateCScalingInfo, SI_updateScalingMethod.";
        0x3ac unk_3ac: Vector, Unknown, "0 at creation; SI_modelSpecialUpdateSonState writes it, the inverse kinematics routines read it.";
        0x3b8 comp_angles: [Vector; 2], Named, "0 at creation; SI_modelCompensateLocalCoord, compensateAngleUpdate.";
        0x3d0 comp_keys: [Vector; 2], Named, "0 at creation; keyCompensateInfoUpdate reads them.";
        0x3e8 comp_info: Ptr32<CompInfoList>, Read, "Disposed with SI_classicMdlCompInfoDispose.";
        0x3ec visibility: i16, Named, "Token 23; 7 at creation.";
        0x3ee motion_blur: i16, Named, "Token 24.";
        0x3f0 gl_list: Ptr32<Opaque>, Named, "modelCreateGlist, SI_glCallDispList.";
        0x3f4 matte: f32, Read, "Attribute 371 (NMatte); token 26; 1.0 at creation.";
        0x3f8 garbage_matte: i16, Read, "Attribute 538 (GMatte); token 27.";
        0x3fa update_flags2: u16, Named, "0x3e at creation; set together with update_flags.";
        0x3fc unk_3fc: Ptr32<Opaque>, Unknown, "0 at creation; read by plmDraw.";
        0x400 unk_400: i16, Unknown, "1 at creation; no other access found.";
        0x402 unk_402: i16, Unknown, "0 at creation; edSetMasterFlag writes it.";
        0x404 caustic: i16, Read, "Bit 0: attribute 620 (CsTrans); bit 1: attribute 631 (CsReceiv).";
        0x406 global_illum: i16, Read, "Bit 0: attribute 632 (GITrans); bit 1: attribute 633 (GIReceiv).";
        0x408 vertex_ud_cluster: Ptr32<Kluster>, Read, "Per-vertex user data: the cluster of tagged vertices (token 29).";
        0x40c vertex_ud: Ptr32<Ptr32<UserNode>>, Read, "Per-vertex user data (token 30); disposed with its cluster by SI_subelUserDataDispose.";
        0x410 polygon_ud_cluster: Ptr32<Kluster>, Read, "Per-polygon user data: cluster (token 31).";
        0x414 polygon_ud: Ptr32<Ptr32<UserNode>>, Read, "Per-polygon user data (token 32).";
        0x418 edge_ud_cluster: Ptr32<Kluster>, Read, "Per-edge user data: cluster (token 33).";
        0x41c edge_ud: Ptr32<Ptr32<UserNode>>, Read, "Per-edge user data (token 34).";
        0x420 wf_display: i16, Read, "Attribute 676 (WFDisplay).";
        0x422 pad_422: [u8; 2], Unknown, "No access found.";
        0x424 nb_uv: i32, Read, "Number of texture coordinate sets of a mesh, 1 at creation; the mesh writer stores it and each polygon node carries nb_uv - 1 extra pairs.";
        0x428 uv_domains: Ptr32<Opaque>, Read, "nb_uv - 1 entries of 8 bytes, each a pointer (freed with the node) and a count.";
        0x42c unk_42c: Ptr32<Opaque>, Unknown, "0 at creation; freed with the node; SI_collect writes it.";
    }
}

impl Model {
    /// A new node [soft 0x08477cf4 SI_modelAllocate].
    ///
    /// The original takes a name or null; with null it names the node `obj<n>` (the C format
    /// `obj%1hd`) from the counter. Either way it stores a `strdup`ed copy at `name`; here the
    /// caller owns the string returned beside the record and `name` stays null.
    /// `counter` is the original's static counter, incremented by each call.
    ///
    /// Fields the original does not set (the block comes from `SI_malloc`, which does not
    /// clear it) and that are zero here: `pad_0e`, `pad_42`, `pad_4e`, `pad_56`, `pad_5e`,
    /// `pad_20a`, `unk_210`, `unk_214`, `unk_220`, `pad_2ba`, `pad_2e6`, `unk_334`,
    /// `unk_338`, `pad_33a`, `unk_340`, `pad_422`.
    pub fn allocate(name: Option<&[u8]>, counter: &mut i32) -> (Model, Vec<u8>) {
        let mut m = Model::zero();
        *counter += 1;
        m.serial = *counter as i16;
        // sprintf("obj%1hd", count): the count as a signed 16-bit number
        let name = match name {
            Some(n) => n.to_vec(),
            None => format!("obj{}", *counter as i16).into_bytes(),
        };
        m.update_flags = 0x3e;
        m.update_flags2 = 0x3e;
        m.def_matrix = IDENTITY;
        m.hscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
        m.hrot = IDENTITY;
        m.scal = Vector { x: 1.0, y: 1.0, z: 1.0 };
        m.mrot = IDENTITY;
        m.gscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
        m.grot = IDENTITY;
        m.gmat = IDENTITY;
        m.mat_238 = IDENTITY;
        m.mat_278 = IDENTITY;
        m.unk_400 = 1;
        m.elem_id = -1;
        m.cscl_matrix = IDENTITY;
        m.cscl = Vector { x: 1.0, y: 1.0, z: 1.0 };
        m.matte = 1.0;
        m.visibility = 7;
        m.nb_uv = 1;
        // `latdep` is set to the node's own address; the caller, who knows where the record
        // lives, does that with `set_self`.
        (m, name)
    }

    /// The last step of the original's allocator: `latdep` points at the node itself.
    pub fn set_self(&mut self, this: Ptr32<Model>) {
        self.latdep = this;
    }
}

/// A set of model nodes addressed by index, for the routines that follow the father,
/// brother and son links. Index `i` is the pointer value `i + 1`; 0 is null.
#[derive(Default)]
pub struct ModelArena {
    pub nodes: Vec<Model>,
    pub names: Vec<Vec<u8>>,
    /// The original's allocation counter.
    pub counter: i32,
}

impl ModelArena {
    pub fn ptr(index: usize) -> Ptr32<Model> {
        Ptr32::new(index as u32 + 1)
    }

    fn index(p: Ptr32<Model>) -> Option<usize> {
        if p.is_null() { None } else { Some(p.0 as usize - 1) }
    }

    /// [soft 0x08477cf4 SI_modelAllocate], with the node placed in the arena.
    pub fn allocate(&mut self, name: Option<&[u8]>) -> usize {
        let (mut m, name) = Model::allocate(name, &mut self.counter);
        let i = self.nodes.len();
        m.set_self(Self::ptr(i));
        self.nodes.push(m);
        self.names.push(name);
        i
    }

    /// Makes `son` the last son of `father` [soft 0x08477c90 SI_modelCreateParentship].
    /// Refused (false) when `son` already has a father, or when `son` is `father` or one of
    /// its ancestors.
    pub fn create_parentship(&mut self, father: usize, son: usize) -> bool {
        if !self.nodes[son].father.is_null() {
            return false;
        }
        let mut p = Some(father);
        while let Some(i) = p {
            if i == son {
                return false;
            }
            p = Self::index(self.nodes[i].father);
        }
        match Self::index(self.nodes[father].son) {
            None => self.nodes[father].son = Self::ptr(son),
            Some(mut last) => {
                while let Some(next) = Self::index(self.nodes[last].brother) {
                    last = next;
                }
                self.nodes[last].brother = Self::ptr(son);
            }
        }
        self.nodes[son].father = Self::ptr(father);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_sets_what_the_original_sets() {
        let mut arena = ModelArena::default();
        let a = arena.allocate(None);
        let b = arena.allocate(Some(b"cube"));
        assert_eq!(arena.names[a], b"obj1");
        assert_eq!(arena.names[b], b"cube");
        let m = &arena.nodes[b];
        assert_eq!(m.serial, 2);
        assert_eq!((m.update_flags, m.update_flags2), (0x3e, 0x3e));
        assert_eq!(m.latdep, ModelArena::ptr(b));
        assert_eq!((m.scal.x, m.hscl.y, m.gscl.z, m.cscl.x), (1.0, 1.0, 1.0, 1.0));
        assert_eq!(m.gmat, IDENTITY);
        assert_eq!((m.elem_id, m.visibility, m.matte, m.nb_uv, m.unk_400), (-1, 7, 1.0, 1, 1));
        assert_eq!((m.kind, m.nb_defs, m.nb_materials), (MNILL, 0, 0));
    }

    #[test]
    fn parentship_appends_and_refuses_cycles() {
        let mut arena = ModelArena::default();
        let (root, a, b, c) = (arena.allocate(None), arena.allocate(None), arena.allocate(None), arena.allocate(None));
        assert!(arena.create_parentship(root, a));
        assert!(arena.create_parentship(root, b));
        assert!(arena.create_parentship(a, c));
        assert_eq!(arena.nodes[root].son, ModelArena::ptr(a));
        assert_eq!(arena.nodes[a].brother, ModelArena::ptr(b));
        assert_eq!(arena.nodes[c].father, ModelArena::ptr(a));
        assert!(!arena.create_parentship(b, c)); // c already has a father
        assert!(!arena.create_parentship(c, root)); // root is an ancestor of c
        assert!(!arena.create_parentship(root, root));
        assert!(arena.nodes[root].father.is_null());
    }
}
