// SPDX-License-Identifier: MIT
//! One node duplicated and one node freed, with what hangs on it: definitions, the original
//! definition, fittings, user data; and the bookkeeping of which node's deformation a
//! geometry depends on. From the original's `model.c`, with four small predicates of
//! `lattice.c` and `wave_sys.c` that these routines cannot do without (finding 0015).
//!
//! A node points to many things this crate does not hold: materials, textures, lattices,
//! fittings, waves, dynamics, inverse kinematics, envelopes, clusters, render data. The
//! record keeps the original's pointer to each as an opaque 32-bit handle; what a routine
//! here reads *through* such a pointer is in [`NodeExt`]; what the original does *to* such a
//! thing is a method of [`Hooks`], called with the handles where the original passes the
//! pointers, in the original's order. A block the original frees is reported the same way
//! ([`Hooks::free`]) when its handle is not null; what this crate owns in its place is
//! dropped regardless.
//!
//! All of this is integer and pointer work: no floating point.

use si_types::model::{Lattice, Model};
use si_types::scene::{LclMap, Material, Texture3D};
use si_types::Ptr32;

use crate::env::Env;
use crate::hierarchy::{self, DefSlot};
use crate::node::{Current, Definition, Models, NodeId, MFACE, MMETA, MBALL, MNCRV, MNSRF, MPTCH, MSMSH, MSPLN};
use crate::obj_global;
use crate::transform::Globals;

/// One lattice of a node: the entry of the pointer array at +0x50 or +0x58, and the first
/// 16-bit word of the lattice it points to (not zero: the lattice is active).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatticeRef {
    pub ptr: Ptr32<Lattice>,
    pub active: i16,
}

/// One block of user data (`__SI_UserNode`): the record's own address and the two blocks it
/// owns, at +0 and +0xc. Records are chained through their word at +0x10.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UserNode {
    pub ptr: u32,
    pub block_0: u32,
    pub block_c: u32,
}

/// User data per vertex, polygon or edge: a cluster record (its two blocks at +0 and +0xc,
/// its element count at +4) and an array of that many chains of user data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubelUserData {
    pub block_0: u32,
    pub count: i32,
    pub block_c: u32,
    pub chains: Vec<Vec<UserNode>>,
}

/// One 2D texture (`__SI_LclMap`): its address and its rotation angles (+0x94, radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureRef {
    pub ptr: Ptr32<LclMap>,
    pub rot: si_types::geom::Vector,
}

/// What the routines of this module read through a node's pointers.
#[derive(Clone, Debug, Default)]
pub struct NodeExt {
    /// +0x20c: the `nb_materials` entries of the material array.
    pub materials: Vec<Ptr32<Material>>,
    /// Per material of that array, its local 2D textures: the chain from the material's
    /// +0x6ec on, linked through each texture's +0x16c.
    pub material_textures: Vec<Vec<TextureRef>>,
    /// The chain of 2D textures from +0x21c on, linked through each texture's +0x16c.
    pub textures: Vec<TextureRef>,
    /// +0x50 and +0x58: the two sets of lattices, `nb_lattices0` and `nb_lattices1` long.
    pub lattices: [Vec<LatticeRef>; 2],
    /// The first 16-bit word of the records `spline_fit` (+0x60, +0x64) point to: active.
    pub spline_fit_active: [i16; 2],
    /// The same of `patch_fit` (+0x2d8, +0x2dc).
    pub patch_fit_active: [i16; 2],
    /// The chain of wave links from +0x2e8 on (linked through +8): the word at +4 of each,
    /// 0 for a wave on the node, 1 for a wave on the branch.
    pub waves: Vec<i32>,
    /// The chain of user data from +0x34c on.
    pub user_data: Vec<UserNode>,
    /// What +0x408/+0x40c (vertices), +0x410/+0x414 (polygons), +0x418/+0x41c (edges) lead to.
    pub subel: [SubelUserData; 3],
    /// +0x428: the pointer of each of the `nb_uv` − 1 eight-byte entries.
    pub uv_domains: Vec<u32>,
}

/// An update node (`SI_UpdElem`, finding 0018) as [`readjust_definition`] looks at it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UpdNode {
    /// Its address.
    pub ptr: u32,
    /// +0x1c: the argument array is not null.
    pub has_arguments: bool,
    /// +0x20: the 16-bit word there is not zero.
    pub active: bool,
    /// The 16-bit word at +0x0c of the first argument (its "active" word) is not zero.
    pub argument_active: bool,
}

/// The settings of the Duplicate command these routines read: 16-bit words of
/// `SI_glbDuplic` (0x087814c0), named after their setup-file keywords (finding 0006).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Duplic {
    /// +0x1e `DUP_LATTICE_DUP`.
    pub lattice_dup: bool,
    /// +0x2a `DUP_SHAPE_ANIM`.
    pub shape_anim: bool,
    /// +0x2c `DUP_DEFORM_ANIM`.
    pub deform_anim: bool,
    /// +0x36 `DUP_GEOMETRY_CURR`.
    pub geometry_curr: bool,
}

/// What this module calls outside the crate. Handles are the record's pointer values.
pub trait Hooks {
    /// `SI_modelResetAll(node)` (`motionReset.c`): the node's animation is taken off.
    fn model_reset_all(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_materialStillUsed(material)`.
    fn material_still_used(&mut self, _material: Ptr32<Material>) -> bool {
        false
    }
    /// `SI_edDisposeMaterial(material, 1)`.
    fn ed_dispose_material(&mut self, _material: Ptr32<Material>) {}
    /// `SI_edDisposeTexture2D(texture)`.
    fn ed_dispose_texture2d(&mut self, _texture: Ptr32<LclMap>) {}
    /// `SI_texture3DStillUsed(texture)`.
    fn texture3d_still_used(&mut self, _texture: Ptr32<Texture3D>) -> bool {
        false
    }
    /// `SI_edDisposeTexture3D(texture)`.
    fn ed_dispose_texture3d(&mut self, _texture: Ptr32<Texture3D>) {}
    /// `SI_fittingDispose(node, &node.spline_fit[which], which)`.
    fn fitting_dispose(&mut self, _models: &mut Models, _node: NodeId, _which: usize) {}
    /// `SI_pfittingDispose(node, &node.patch_fit[which], which)`.
    fn pfitting_dispose(&mut self, _models: &mut Models, _node: NodeId, _which: usize) {}
    /// `SI_modelUnlinkAllWaves(node)`.
    fn unlink_all_waves(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_modelDeleteLatticeAnimation(node, set)`.
    fn delete_lattice_animation(&mut self, _models: &mut Models, _node: NodeId, _set: usize) {}
    /// `SI_latticeDispose(&lattices[set][index])`.
    fn lattice_dispose(&mut self, _models: &mut Models, _node: NodeId, _set: usize, _index: usize) {}
    /// `SI_modelDynaInfoDispose(node, 1)`.
    fn dyna_info_dispose(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_invKinDispose(&node.inv_kin)`.
    fn inv_kin_dispose(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_envelopInfoDispose(&node.env_skel_jnt)`.
    fn envelop_info_dispose(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_klusterDisposeAll(node)`.
    fn kluster_dispose_all(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_clusterDispose(&node.cluster)`.
    fn cluster_dispose(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_classicMdlCompInfoDispose(&node.comp_info)`.
    fn classic_comp_info_dispose(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `free(block)`: a block the original frees, by its handle. Never called with null.
    fn free(&mut self, _block: u32) {}

    /// The disposal routine of a geometry kind called on one of the node's slots:
    /// `SI_patchDispose`, `SI_faceDispose`, `SI_meshDispose`, `SI_splineDispose`,
    /// `SI_metaSystemDispose`, `SI_metaballDispose`, `SI_nurbsCrvDispose`,
    /// `SI_nurbsSrfDispose`. The crate drops what it holds in that slot afterwards (a mesh
    /// through [`obj_global::mesh_delete`]); the hook is for storage it does not hold.
    fn dispose_def(&mut self, _models: &mut Models, _node: NodeId, _kind: i32, _slot: DefSlot) {}
    /// `SI_patchRemoveFitting(node)`.
    fn patch_remove_fitting(&mut self, _models: &mut Models, _node: NodeId) {}
    /// The copy routine of a geometry kind: `SI_patchCopy`, `SI_faceCopy`, `SI_meshCopy`,
    /// `SI_splineCopy`, `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy`, and for the copy node `to`
    /// `si_metaSystemCopy(definition, to)`, `SI_metaballCopy(definition, to)`. The default is
    /// the port for a mesh ([`obj_global::mesh_copy`]) and a plain clone for the others.
    fn copy_def(&mut self, _kind: i32, definition: &Definition, _to: Option<NodeId>) -> Definition {
        match definition {
            Definition::Mesh(m) => Definition::Mesh(obj_global::mesh_copy(m)),
            d => d.clone(),
        }
    }

    /// `SI_classicModelCompInfoCopy(source, copy)` (`classic.c`), under the classical
    /// scaling method only.
    fn classic_comp_info_copy(&mut self, _models: &mut Models, _source: NodeId, _copy: NodeId) {}
    /// The word at +8 of the graf node `SI_findGlData(current graf list, node, graf_type)`
    /// finds: the node's selection state on screen (1: selected). `graf_type` 0 looks at
    /// every graf node, 3 at those of models. `None` when there is none.
    fn graf_selection(&mut self, _models: &Models, _node: NodeId, _graf_type: i32) -> Option<i32> {
        None
    }
    /// `SI_modelDynaInfoCopy(source, copy)` (`settings.c`).
    fn dyna_info_copy(&mut self, _models: &mut Models, _source: NodeId, _copy: NodeId) {}
    /// `copy.inv_kin = SI_invKinCopy(source.inv_kin)`; the result is the new handle.
    fn inv_kin_copy(&mut self, _models: &mut Models, _source: NodeId, _copy: NodeId) -> u32 {
        0
    }

    /// The kind's routine that brings the points of the first stored definition into the
    /// working one: `SI_patchTransferPoints`, `SI_faceTransferPoints`,
    /// `SI_meshTransferVertices`, `SI_splineTransferPoints`, `SI_nurbsCrvTransferPointsKnots`,
    /// `SI_nurbsSrfTransferPointsKnots` (first stored definition, working definition).
    fn transfer_points(&mut self, _models: &mut Models, _node: NodeId, _kind: i32) {}
    /// `SI_waveTransform(wave, node, branch, time, 0)`.
    fn wave_transform(&mut self, _models: &mut Models, _wave: u32, _node: NodeId, _branch: bool, _time: f32) {}
    /// `SI_latticeTransform(lattice, kind, definition, definition)` on the node's working
    /// definition.
    fn lattice_transform(&mut self, _models: &mut Models, _lattice: Ptr32<Lattice>, _node: NodeId) {}
    /// `SI_splineFit(fitting, kind, definition, definition)` (`model.c`; ported on the image
    /// only, `model_image::fit`).
    fn spline_fit(&mut self, _models: &mut Models, _fitting: u32, _node: NodeId) {}
    /// `SI_patchFit(fitting, kind, definition, definition)` (`model.c`, not ported).
    fn patch_fit(&mut self, _models: &mut Models, _fitting: u32, _node: NodeId) {}
    /// `SI_elemdictFindRelatedElem(scene dictionary, element, 1, 2, relation, MODELS, …) != 0`:
    /// the element has a relation of that id.
    fn find_related_elem(&mut self, _element: u32, _relation: i32) -> bool {
        false
    }
    /// `SI_edFindUpdateNode(update list, node, relation, cursor)`: the next update node of
    /// that relation for the node. `cursor` is the routine's own position (`None`: a null
    /// pointer, the first one).
    fn ed_find_update_node(&mut self, _models: &Models, _node: NodeId, _relation: i32, _cursor: Option<&mut i32>) -> Option<UpdNode> {
        None
    }
    /// `SI_checkActiveTime(update node, time, &value)`: the value when the answer is not 0.
    fn check_active_time(&mut self, _update: &UpdNode, _time: f32) -> Option<f32> {
        None
    }
    /// `SI_animQuickStretchUpdate(update node, value)`.
    fn anim_quick_stretch_update(&mut self, _models: &mut Models, _update: &UpdNode, _value: f32) {}
    /// `SI_animMdlClusterCnsUpdate(update node, 0, which, NULL, time)`.
    fn anim_mdl_cluster_cns_update(&mut self, _models: &mut Models, _update: &UpdNode, _which: i16, _time: f32) {}
    /// `SI_envelopActive(node) != 0`.
    fn envelop_active(&mut self, _models: &Models, _node: NodeId) -> bool {
        false
    }

    /// `SI_removeTriangle(node, 0)`: the node's triangulation is dropped.
    fn remove_triangle(&mut self, _models: &mut Models, _node: NodeId) {}
    /// What `SI_modelTouchDefinition` does to a working definition that is not a mesh:
    /// `SI_splineTouch` for a spline; for a NURBS curve the bits 0x60 of the byte at +0x3b
    /// and `SI_nurbsCrvAnimUpdate`; for a NURBS surface the bits 0x60 of the byte at +0x5b.
    fn touch_def(&mut self, _models: &mut Models, _node: NodeId, _kind: i32) {}
}

impl Hooks for () {}

const GEOMETRY: [i32; 6] = [MPTCH, MFACE, MSMSH, MSPLN, MNCRV, MNSRF];

fn release(env: &mut dyn Env, p: &mut u32) {
    if *p != 0 {
        env.free(*p);
        *p = 0;
    }
}

// ------------------------------------------------------------------ deformation predicates

/// The node's active lattice and its set [soft 0x0843ff1c SI_modelLattice]: the first
/// lattice of set 1 if there is one and it is active, else that of set 0 likewise.
pub fn model_lattice(models: &Models, node: NodeId) -> Option<(Ptr32<Lattice>, i16)> {
    let (m, x) = (models.m(node), &models.data[node].cx);
    for set in [1usize, 0] {
        let n = if set == 1 { m.nb_lattices1 } else { m.nb_lattices0 };
        if n != 0 {
            // (the original reads through the array without testing its pointer)
            if let Some(l) = x.lattices[set].first() {
                if l.active != 0 {
                    return Some((l.ptr, set as i16));
                }
            }
        }
    }
    None
}

/// The active lattice of set 1, the one that acts on the whole branch
/// [soft 0x0843ff88 SI_modelTreeLattice].
pub fn model_tree_lattice(models: &Models, node: NodeId) -> Option<Ptr32<Lattice>> {
    if models.m(node).nb_lattices1 == 0 {
        return None;
    }
    models.data[node].cx.lattices[1].first().filter(|l| l.active != 0).map(|l| l.ptr)
}

/// The head of the node's wave list if any wave is on the node itself (link word 0)
/// [soft 0x084c7610 SI_modelNodeWave].
pub fn model_node_wave(models: &Models, node: NodeId) -> Option<u32> {
    models.data[node].cx.waves.iter().any(|&k| k == 0).then(|| models.m(node).waves.0)
}

/// The same for a wave on the branch (link word 1) [soft 0x084c75dc SI_modelBranchWave].
pub fn model_branch_wave(models: &Models, node: NodeId) -> Option<u32> {
    models.data[node].cx.waves.iter().any(|&k| k == 1).then(|| models.m(node).waves.0)
}

fn fit(ptr: u32, active: i16) -> Option<u32> {
    (ptr != 0 && active != 0).then_some(ptr)
}

/// The node's active spline deformation and which it is: 1 the branch's (+0x64), else 0 the
/// node's (+0x60) [soft 0x08472b44 SI_modelSplineFitting].
pub fn model_spline_fitting(models: &Models, node: NodeId) -> Option<(u32, i16)> {
    model_tree_spline_fitting(models, node).map(|p| (p, 1)).or_else(|| model_node_spline_fitting(models, node).map(|p| (p, 0)))
}

/// [soft 0x08472b88 SI_modelNodeSplineFitting]
pub fn model_node_spline_fitting(models: &Models, node: NodeId) -> Option<u32> {
    fit(models.m(node).spline_fit[0].0, models.data[node].cx.spline_fit_active[0])
}

/// [soft 0x08472ba4 SI_modelTreeSplineFitting]
pub fn model_tree_spline_fitting(models: &Models, node: NodeId) -> Option<u32> {
    fit(models.m(node).spline_fit[1].0, models.data[node].cx.spline_fit_active[1])
}

/// The node's active patch deformation and which it is: 1 the branch's (+0x2dc), else 0 the
/// node's (+0x2d8) [soft 0x08472c3c SI_modelPatchFitting].
pub fn model_patch_fitting(models: &Models, node: NodeId) -> Option<(u32, i16)> {
    model_branch_patch_fitting(models, node).map(|p| (p, 1)).or_else(|| model_node_patch_fitting(models, node).map(|p| (p, 0)))
}

/// [soft 0x08472c88 SI_modelNodePatchFitting]
pub fn model_node_patch_fitting(models: &Models, node: NodeId) -> Option<u32> {
    fit(models.m(node).patch_fit[0].0, models.data[node].cx.patch_fit_active[0])
}

/// [soft 0x08472ca4 SI_modelBranchPatchFitting]
pub fn model_branch_patch_fitting(models: &Models, node: NodeId) -> Option<u32> {
    fit(models.m(node).patch_fit[1].0, models.data[node].cx.patch_fit_active[1])
}

/// Takes a spline deformation off a node [soft 0x08472bc0 SI_modelSplineFittingReset]:
/// the branch's with `branch`, else the node's. If there is one it is disposed of
/// ([`Hooks::fitting_dispose`]) and the dependencies of the whole tree are worked out again.
pub fn spline_fitting_reset(models: &mut Models, node: NodeId, branch: bool, g: &Globals, env: &mut dyn Env) {
    let which = branch as usize;
    if !models.m(node).spline_fit[which].is_null() {
        env.fitting_dispose(models, node, which);
        let root = hierarchy::root(models, node);
        update_dependency(models, root, None, g, env);
    }
}

/// Takes a patch deformation off a node [soft 0x08472cc0 SI_modelPatchFittingReset], as
/// [`spline_fitting_reset`].
pub fn patch_fitting_reset(models: &mut Models, node: NodeId, branch: bool, g: &Globals, env: &mut dyn Env) {
    let which = branch as usize;
    if !models.m(node).patch_fit[which].is_null() {
        env.pfitting_dispose(models, node, which);
        let root = hierarchy::root(models, node);
        update_dependency(models, root, None, g, env);
    }
}

/// Takes all four deformations off a node [soft 0x08476d0c SI_modelRemoveAllFittings]: the
/// node's spline, the branch's spline, the node's patch, the branch's patch.
pub fn remove_all_fittings(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    for branch in [false, true] {
        if !models.m(node).spline_fit[branch as usize].is_null() {
            spline_fitting_reset(models, node, branch, g, env);
        }
    }
    for branch in [false, true] {
        if !models.m(node).patch_fit[branch as usize].is_null() {
            patch_fitting_reset(models, node, branch, g, env);
        }
    }
}

// ------------------------------------------------------------------ touching

/// Marks a node's working definition as changed [soft 0x0846d530 SI_modelTouchDefinition]:
/// bit 0x10 of both update flag words, the triangulation dropped, and the definition's own
/// "changed" mark by kind (a mesh: [`obj_global::mesh_touch`]; spline and NURBS: a hook).
pub fn touch_definition(models: &mut Models, node: NodeId, env: &mut dyn Env) {
    let m = models.m_mut(node);
    m.update_flags |= 0x10;
    m.update_flags2 |= 0x10;
    let kind = m.kind;
    env.remove_triangle(models, node);
    match kind {
        MSPLN | MNCRV | MNSRF => env.touch_def(models, node, kind),
        MSMSH => {
            let d = &mut models.data[node];
            let mesh = match &mut d.current {
                Current::Own(b) => Some(&mut **b),
                Current::Def(i) => d.defs.get_mut(*i),
                Current::None => None,
            };
            if let Some(Definition::Mesh(mesh)) = mesh {
                obj_global::mesh_touch(mesh);
            }
        }
        _ => {}
    }
}

/// Marks a node's geometry as changed [soft 0x0846d48c SI_modelTouchGeometry]: bit 0x10 of
/// both update flag words; then, for a node that no lattice, fitting or wave deforms and
/// whose geometry lives in its own space, [`touch_definition`], and for any other node the
/// definition is readjusted ([`readjust_definition`]); last the bounding box is recomputed
/// if the node has a valid one (a hook).
pub fn touch_geometry(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    let m = models.m_mut(node);
    m.update_flags |= 0x10;
    m.update_flags2 |= 0x10;
    let plain = model_lattice(models, node).is_none()
        && model_spline_fitting(models, node).is_none()
        && model_patch_fitting(models, node).is_none()
        && model_node_wave(models, node).is_none()
        && model_branch_wave(models, node).is_none()
        && models.m(node).latdep == Models::ptr(node);
    if plain {
        touch_definition(models, node, env);
    } else {
        readjust_definition(models, node, g, env);
    }
    if models.m(node).bbox_valid != 0 {
        env.compute_bounding(models, node);
    }
}

/// The active lattice of set 0, the one that acts on the node alone
/// [soft 0x0843ff68 SI_modelNodeLattice].
pub fn model_node_lattice(models: &Models, node: NodeId) -> Option<Ptr32<Lattice>> {
    if models.m(node).nb_lattices0 == 0 {
        return None;
    }
    models.data[node].cx.lattices[0].first().filter(|l| l.active != 0).map(|l| l.ptr)
}

/// Rebuilds a node's working definition from its stored one and everything that deforms
/// it [soft 0x0846e340 SI_modelReadjustDefinition]. Deformations are evaluated at the
/// update time, or at the current frame over the frame rate (rounded to float).
///
/// 1. The points of the first stored definition are brought into the working one (the
///    kind's transfer routine, a hook), unless the animation is inside an update
///    ([`Globals::envelope_hold`]) and the node is enveloped (type 5, 6 or 8).
/// 2. For a node with stored definitions, in this order, each a hook: the node's own wave,
///    lattice (set 0), spline fitting and patch fitting; then, if the node has an element
///    with the relations: quick stretch (relation 0xef, when its update node has arguments,
///    is active, its first argument is active and the time is in range), and the cluster
///    constraints (relations 0xf6 and 0xf7, every update node in range); then the branch
///    deformations of the node itself: wave, lattice (set 1), spline fitting, patch fitting.
/// 3. If the geometry depends on another node (`latdep`), the walk goes up from the node:
///    the working definition is multiplied by the `def_matrix` of the node walked from
///    ([`crate::transform::multiply_def`]; not for an enveloped node that is inside its own
///    update in interactive mode), then the branch deformations of the node walked to are
///    applied; until the node walked to is `latdep`. So the geometry ends in the space of
///    the deforming node, having taken every deformation of the branches on the way.
/// 4. If nothing deformed the node, it depends on no other node and its envelope is not
///    active, the working definition is given up ([`delete_definition`] keeping the stored
///    ones): the node works on its first stored definition again.
/// 5. [`touch_definition`].
///
/// (The walk of step 3 follows `father` links without testing them: a `latdep` that is not
/// an ancestor sends the original through a null pointer; here the walk ends there.)
pub fn readjust_definition(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    use si_x87::x;
    let time = g.update_time.unwrap_or_else(|| (x(g.frame_current) / g.frame_rate).to_f32());
    let kind = models.m(node).kind;
    let enveloped = matches!(models.m(node).envelop, 5 | 6 | 8);
    if (g.envelope_hold == 0 || !enveloped) && GEOMETRY.contains(&kind) {
        env.transfer_points(models, node, kind);
    }
    let elem = env.ed_get_elem_from_model(models, node);
    if models.m(node).nb_defs != 0 {
        let mut plain = true;
        if let Some(w) = model_node_wave(models, node) {
            env.wave_transform(models, w, node, false, time);
            plain = false;
        }
        if let Some(l) = model_node_lattice(models, node) {
            env.lattice_transform(models, l, node);
            plain = false;
        }
        if let Some(f) = model_node_spline_fitting(models, node) {
            env.spline_fit(models, f, node);
            plain = false;
        }
        if let Some(f) = model_node_patch_fitting(models, node) {
            env.patch_fit(models, f, node);
            plain = false;
        }
        if elem != 0 {
            if env.find_related_elem(elem, 0xef) {
                if let Some(u) = env.ed_find_update_node(models, node, 0xef, None) {
                    if u.has_arguments && u.active && u.argument_active {
                        if let Some(value) = env.check_active_time(&u, time) {
                            plain = false;
                            env.anim_quick_stretch_update(models, &u, value);
                        }
                    }
                }
            }
            for (relation, which) in [(0xf6, 0i16), (0xf7, 1)] {
                if env.find_related_elem(elem, relation) {
                    let mut cursor = 0;
                    while let Some(u) = env.ed_find_update_node(models, node, relation, Some(&mut cursor)) {
                        if env.check_active_time(&u, time).is_some() {
                            env.anim_mdl_cluster_cns_update(models, &u, which, time);
                            plain = false;
                        }
                    }
                }
            }
        }
        if let Some(w) = model_branch_wave(models, node) {
            plain = false;
            env.wave_transform(models, w, node, true, time);
        }
        if let Some(l) = model_tree_lattice(models, node) {
            plain = false;
            env.lattice_transform(models, l, node);
        }
        if let Some(f) = model_tree_spline_fitting(models, node) {
            plain = false;
            env.spline_fit(models, f, node);
        }
        if let Some(f) = model_branch_patch_fitting(models, node) {
            plain = false;
            env.patch_fit(models, f, node);
        }
        let own = models.m(node).latdep == Models::ptr(node);
        if !own {
            let mut from = node;
            let mut to = models.father(node);
            while let Some(up) = to {
                let m = models.m(node);
                let held = matches!(m.envelop, 5 | 6 | 8) && g.interactive && (m.flags_08 as i16) < 0;
                if !held {
                    let matrix = models.m(from).def_matrix;
                    let d = &mut models.data[node];
                    let def = match &mut d.current {
                        Current::Own(b) => Some(&mut **b),
                        Current::Def(i) => d.defs.get_mut(*i),
                        Current::None => None,
                    };
                    // (a null working definition goes to the kind's routine in the original)
                    if let Some(def) = def {
                        crate::transform::multiply_def(&matrix, kind, def, env);
                    }
                }
                if let Some(w) = model_branch_wave(models, up) {
                    env.wave_transform(models, w, node, true, time);
                }
                if let Some(l) = model_tree_lattice(models, up) {
                    env.lattice_transform(models, l, node);
                }
                if let Some(f) = model_tree_spline_fitting(models, up) {
                    env.spline_fit(models, f, node);
                }
                if let Some(f) = model_branch_patch_fitting(models, up) {
                    env.patch_fit(models, f, node);
                }
                if models.m(node).latdep == Models::ptr(up) {
                    break;
                }
                from = up;
                to = models.father(up);
            }
        }
        if plain && models.m(node).latdep == Models::ptr(node) && !env.envelop_active(models, node) {
            delete_definition(models, node, true, env);
        }
    }
    touch_definition(models, node, env);
}

// ------------------------------------------------------------------ dependency

fn works_on_stored(models: &Models, node: NodeId) -> bool {
    models.data[node].current == Current::Def(models.m(node).cur_def as usize)
}

/// Works out, for a node, its following brothers and everything below them, which node's
/// deformation each geometry depends on [soft 0x0846f200 SI_modelUpdateDependency].
///
/// `holder` is the nearest node above whose lattice, spline or patch deformation or wave
/// acts on its whole branch (`None`: there is none). `latdep` (+0x44) becomes the holder,
/// or the node itself.
///
/// - A node under a holder, or one that is deformed itself (an active lattice or fitting of
///   either set, a wave, or `unk_352` > 0), must work on a definition of its own: if it has
///   stored definitions and works on the one numbered `cur_def`, that one is copied (patch,
///   face, mesh, spline, NURBS; other kinds are left). Then the definition is readjusted
///   ([`readjust_definition`]).
/// - Any other node goes back to its stored definition: if it works on a separate one, that
///   is disposed of (the same six kinds) and `definition` is `defs[cur_def]` again, and the
///   geometry is touched ([`touch_geometry`]). A node already on its stored definition, or
///   without definitions, is left alone, without a touch.
///
/// Then the next brother with the same holder; then the first son, whose holder is this
/// node if none came from above and the node has a branch deformation (an active lattice of
/// set 1, an active spline or patch fitting of the branch, or a branch wave).
pub fn update_dependency(models: &mut Models, node: NodeId, holder: Option<NodeId>, g: &Globals, env: &mut dyn Env) {
    let kind = models.m(node).kind;
    let cur = models.m(node).cur_def as usize;
    let deformed = match holder {
        Some(h) => {
            models.m_mut(node).latdep = Models::ptr(h);
            true
        }
        None => {
            models.m_mut(node).latdep = Models::ptr(node);
            model_spline_fitting(models, node).is_some()
                || model_lattice(models, node).is_some()
                || model_patch_fitting(models, node).is_some()
                || model_node_wave(models, node).is_some()
                || model_branch_wave(models, node).is_some()
                || models.m(node).unk_352 > 0
        }
    };
    if deformed {
        if models.m(node).nb_defs != 0 && works_on_stored(models, node) && GEOMETRY.contains(&kind) {
            // (the original reads defs[cur_def] without testing cur_def against nb_defs)
            if let Some(d) = models.data[node].defs.get(cur) {
                let copy = env.copy_def(kind, d, None);
                models.data[node].current = Current::Own(Box::new(copy));
            }
        }
        readjust_definition(models, node, g, env);
    } else if models.m(node).nb_defs != 0 && !works_on_stored(models, node) {
        if GEOMETRY.contains(&kind) {
            drop_def(models, node, kind, DefSlot::Current, env);
            models.data[node].current = Current::Def(cur);
        }
        touch_geometry(models, node, g, env);
    }
    if let Some(b) = models.brother(node) {
        update_dependency(models, b, holder, g, env);
    }
    let mut holder = holder;
    if holder.is_none()
        && (model_tree_lattice(models, node).is_some()
            || model_tree_spline_fitting(models, node).is_some()
            || model_branch_wave(models, node).is_some()
            || model_branch_patch_fitting(models, node).is_some())
    {
        holder = Some(node);
    }
    if let Some(s) = models.son(node) {
        update_dependency(models, s, holder, g, env);
    }
}

// ------------------------------------------------------------------ definitions

/// One slot of a node's geometry through its kind's disposal routine: the hook is told,
/// then what this crate holds in the slot is dropped (a mesh emptied first).
///
/// `DefSlot::Current` for a node working on stored definition k is that stored definition:
/// the original frees it and leaves its pointer in the array; here it stays in the array,
/// emptied if it is a mesh.
pub(crate) fn drop_def(models: &mut Models, node: NodeId, kind: i32, slot: DefSlot, env: &mut dyn Env) {
    env.dispose_def(models, node, kind, slot);
    let d = &mut models.data[node];
    let empty = |def: &mut Definition| {
        if let Definition::Mesh(m) = def {
            obj_global::mesh_delete(m);
        }
    };
    match slot {
        DefSlot::Stored(i) => {
            if let Some(def) = d.defs.get_mut(i) {
                empty(def);
            }
        }
        DefSlot::Current => match core::mem::take(&mut d.current) {
            Current::Own(mut b) => empty(&mut b),
            Current::Def(i) => {
                if let Some(def) = d.defs.get_mut(i) {
                    empty(def);
                }
            }
            Current::None => {}
        },
        DefSlot::Original => {
            if let Some(mut b) = d.org_def.take() {
                empty(&mut b);
            }
        }
    }
}

/// Frees a node's geometry [soft 0x084678a8 SI_modelDeleteDefinition]. Nothing for a node
/// without stored definitions.
///
/// By kind, each through its own disposal routine:
/// - patch, face, mesh, spline, NURBS curve and surface: the working definition unless it
///   is the **first** stored one; then, unless `keep_stored`, every stored definition and
///   the original definition (and for a patch `SI_patchRemoveFitting`);
/// - a metaball system: the working definition, always;
/// - a metaball: as the first group, without the original definition.
///
/// Afterwards, with `keep_stored` the node works on its first stored definition; without,
/// the array is freed and the node has no geometry: `defs`, `definition`, `org_def` null,
/// kind 0 (a null), `nb_defs` 0. `cur_def` is left as it was.
///
/// The test is against the first stored definition, not against `cur_def`: a node working
/// on stored definition k > 0 has that one disposed of as "the working definition" and,
/// without `keep_stored`, a second time in the loop (the original frees the block twice).
pub fn delete_definition(models: &mut Models, node: NodeId, keep_stored: bool, env: &mut dyn Env) {
    let n = models.m(node).nb_defs as usize;
    if n == 0 {
        return;
    }
    let kind = models.m(node).kind;
    let separate = models.data[node].current != Current::Def(0);
    match kind {
        MPTCH | MFACE | MSMSH | MSPLN | MNCRV | MNSRF | MBALL => {
            if separate {
                drop_def(models, node, kind, DefSlot::Current, env);
            }
            if !keep_stored {
                for i in 0..n {
                    drop_def(models, node, kind, DefSlot::Stored(i), env);
                }
                if kind != MBALL && has_org_def(models, node) {
                    drop_def(models, node, kind, DefSlot::Original, env);
                }
                if kind == MPTCH {
                    env.patch_remove_fitting(models, node);
                }
            }
        }
        MMETA => drop_def(models, node, kind, DefSlot::Current, env),
        _ => {}
    }
    if keep_stored {
        models.data[node].current = Current::Def(0);
    } else {
        release(env, &mut models.m_mut(node).defs.0);
        let d = &mut models.data[node];
        d.defs.clear();
        d.current = Current::None;
        d.org_def = None;
        let m = models.m_mut(node);
        m.definition = Ptr32::NULL;
        m.org_def = Ptr32::NULL;
        m.kind = 0;
        m.nb_defs = 0;
    }
}

fn has_org_def(models: &Models, node: NodeId) -> bool {
    models.data[node].org_def.is_some() || !models.m(node).org_def.is_null()
}

/// Gives a node a copy of a definition as its original definition
/// [soft 0x08467d6c SI_modelCopyOrgDef]: for a patch, face, mesh, spline, NURBS curve or
/// surface node, the present original definition is disposed of and replaced by a copy of
/// `definition`. Nothing for another kind, or without a definition.
pub fn copy_org_def(models: &mut Models, node: NodeId, definition: Option<&Definition>, env: &mut dyn Env) {
    let Some(definition) = definition else { return };
    let kind = models.m(node).kind;
    if !GEOMETRY.contains(&kind) {
        return;
    }
    if has_org_def(models, node) {
        drop_def(models, node, kind, DefSlot::Original, env);
    }
    let copy = env.copy_def(kind, definition, None);
    models.data[node].org_def = Some(Box::new(copy));
}

// ------------------------------------------------------------------ user data

/// Frees a chain of user data [soft 0x084619e4 SI_userDataDispose]: per record its two
/// blocks, then the record; the head is cleared.
pub fn user_data_dispose(chain: &mut Vec<UserNode>, head: &mut u32, env: &mut dyn Env) {
    for u in chain.drain(..) {
        for b in [u.block_0, u.block_c, u.ptr] {
            if b != 0 {
                env.free(b);
            }
        }
    }
    *head = 0;
}

/// Frees user data per vertex, polygon or edge
/// [soft 0x084631a4 SI_subelUserDataDispose]: if there is an array, its chains (as many as
/// the cluster counts; none without a cluster) and the array; then the cluster's two blocks
/// and the cluster. `cluster` and `array` are the record's two pointers.
pub fn subel_user_data_dispose(data: &mut SubelUserData, cluster: &mut u32, array: &mut u32, env: &mut dyn Env) {
    if *array != 0 {
        if *cluster != 0 {
            for i in 0..data.count.max(0) as usize {
                // (a chain's head lives in the array, which is freed next)
                let mut head = 0;
                match data.chains.get_mut(i) {
                    Some(chain) => user_data_dispose(chain, &mut head, env),
                    None => {}
                }
            }
        }
        release(env, array);
        data.chains.clear();
    }
    if *cluster != 0 {
        release(env, &mut data.block_0);
        release(env, &mut data.block_c);
        release(env, cluster);
        data.count = 0;
    }
}

// ------------------------------------------------------------------ one node freed

fn subel_fields(m: &mut Model, k: usize) -> (&mut u32, &mut u32) {
    match k {
        0 => (&mut m.vertex_ud_cluster.0, &mut m.vertex_ud.0),
        1 => (&mut m.polygon_ud_cluster.0, &mut m.polygon_ud.0),
        _ => (&mut m.edge_ud_cluster.0, &mut m.edge_ud.0),
    }
}

/// Frees one node with what it owns [soft 0x08461a54 SI_modelDisposeNode]. The node must be
/// out of its father's list already ([`hierarchy::dispose`] sees to that); its own `father`
/// link is still followed to find the root.
///
/// In this order: the node's animation is taken off (`SI_modelResetAll`); all geometry
/// ([`delete_definition`]); the name, the block at +0 and the shape data; each material no
/// other node uses, and the material array; every 2D texture of the list; the 3D texture
/// if no other node uses it; the four fittings ([`spline_fitting_reset`],
/// [`patch_fitting_reset`]); the waves; the lattices of set 0, then of set 1, each set
/// followed by [`update_dependency`] from the root of the tree; dynamics, inverse
/// kinematics, the envelope record, the initial transformation and the block at +0x32c;
/// clusters; the user data ([`user_data_dispose`]); the classical-scaling compensation
/// list; the render data; the user data per vertex, polygon and edge
/// ([`subel_user_data_dispose`]); the texture-coordinate domains; the record.
///
/// The slot is marked `disposed`.
pub fn dispose_node(models: &mut Models, node: NodeId, g: &Globals, env: &mut dyn Env) {
    env.model_reset_all(models, node);
    delete_definition(models, node, false, env);
    {
        let m = models.m_mut(node);
        release(env, &mut m.name.0);
        release(env, &mut m.block_00.0);
        release(env, &mut m.shape_data.0);
    }
    if !models.m(node).materials.is_null() {
        let mut i: i16 = 0;
        while i < models.m(node).nb_materials {
            // (an entry past the array this crate holds is a null handle)
            let mat = models.data[node].cx.materials.get(i as usize).copied().unwrap_or(Ptr32::NULL);
            if !env.material_still_used(mat) {
                env.ed_dispose_material(mat);
            }
            i += 1;
        }
        release(env, &mut models.m_mut(node).materials.0);
        models.data[node].cx.materials.clear();
    }
    // the list is walked by each texture's own link, read before the texture is disposed of
    for t in core::mem::take(&mut models.data[node].cx.textures) {
        env.ed_dispose_texture2d(t.ptr);
    }
    let t3 = models.m(node).texture3d;
    if !t3.is_null() && !env.texture3d_still_used(t3) {
        env.ed_dispose_texture3d(t3);
    }
    for branch in [false, true] {
        if !models.m(node).spline_fit[branch as usize].is_null() {
            spline_fitting_reset(models, node, branch, g, env);
        }
    }
    for branch in [false, true] {
        if !models.m(node).patch_fit[branch as usize].is_null() {
            patch_fitting_reset(models, node, branch, g, env);
        }
    }
    if !models.m(node).waves.is_null() {
        env.unlink_all_waves(models, node);
    }
    for set in 0..2 {
        let array = if set == 0 { models.m(node).lattices0.0 } else { models.m(node).lattices1.0 };
        if array == 0 {
            continue;
        }
        env.delete_lattice_animation(models, node, set);
        let mut i = 0usize;
        loop {
            let n = if set == 0 { models.m(node).nb_lattices0 } else { models.m(node).nb_lattices1 };
            if i >= n as usize {
                break;
            }
            env.lattice_dispose(models, node, set, i);
            i += 1;
        }
        let m = models.m_mut(node);
        if set == 0 {
            release(env, &mut m.lattices0.0);
            m.nb_lattices0 = 0;
        } else {
            release(env, &mut m.lattices1.0);
            m.nb_lattices1 = 0;
        }
        models.data[node].cx.lattices[set].clear();
        let root = hierarchy::root(models, node);
        update_dependency(models, root, None, g, env);
    }
    env.dyna_info_dispose(models, node);
    if !models.m(node).inv_kin.is_null() || models.data[node].tx.inv_kin_kind.is_some() {
        env.inv_kin_dispose(models, node);
    }
    if !models.m(node).env_skel_jnt.is_null() {
        env.envelop_info_dispose(models, node);
    }
    {
        let m = models.m_mut(node);
        release(env, &mut m.init_srt.0);
        release(env, &mut m.unk_32c.0);
    }
    models.data[node].init_srt = None;
    env.kluster_dispose_all(models, node);
    env.cluster_dispose(models, node);
    if !models.m(node).user_data.is_null() {
        let mut chain = core::mem::take(&mut models.data[node].cx.user_data);
        user_data_dispose(&mut chain, &mut models.m_mut(node).user_data.0, env);
    }
    env.classic_comp_info_dispose(models, node);
    {
        let m = models.m_mut(node);
        release(env, &mut m.ren_vertices.0);
        release(env, &mut m.unk_42c.0);
        release(env, &mut m.ren_triangles.0);
        m.ren_count0 = 0;
        m.ren_count1 = 0;
    }
    for k in 0..3 {
        let mut data = core::mem::take(&mut models.data[node].cx.subel[k]);
        let (cluster, array) = subel_fields(models.m_mut(node), k);
        subel_user_data_dispose(&mut data, cluster, array, env);
        models.data[node].cx.subel[k] = data;
    }
    if !models.m(node).uv_domains.is_null() {
        // nb_uv − 1 entries, counted by a 16-bit index
        let n = models.m(node).nb_uv.wrapping_sub(1);
        let mut i: i16 = 0;
        while (i as i32) < n {
            if let Some(p) = models.data[node].cx.uv_domains.get_mut(i as usize) {
                release(env, p);
            }
            i += 1;
        }
        release(env, &mut models.m_mut(node).uv_domains.0);
        models.data[node].cx.uv_domains.clear();
    }
    let this = Models::ptr(node).0;
    env.free(this);
    let d = &mut models.data[node];
    d.disposed = true;
    d.name.clear();
}

// ------------------------------------------------------------------ one node duplicated

/// Which of the source's geometries a copy starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopySource {
    /// The working definition (`definition`, +0x48): the shape as deformed now.
    Working,
    /// The original definition (`org_def`), or the first stored one if there is none.
    Original,
    /// The first stored definition.
    First,
}

/// The choice `SI_modelCopyNode` makes (part of 0x0846234c, see [`copy_node`]).
///
/// `stored` is the source's number of stored definitions; `lattice`, `fitting`: the source,
/// or the node its geometry depends on, has an active lattice, an active spline or patch
/// fitting; `envelope`: the source's envelope type is 5, 6 or 8; `as_envelope` is the
/// routine's second argument.
pub fn copy_source(d: &Duplic, stored: u16, lattice: bool, fitting: bool, envelope: bool, as_envelope: bool) -> CopySource {
    let several = stored > 1;
    let shapes_kept = several && d.shape_anim;
    let cur = d.geometry_curr;
    let lattice_dropped = lattice && !d.lattice_dup && cur;
    let fitting_dropped = fitting && !d.deform_anim && cur;
    let any = lattice || fitting || envelope;
    if any && envelope && as_envelope {
        if shapes_kept {
            CopySource::First
        } else if !cur {
            CopySource::Original
        } else {
            CopySource::Working
        }
    } else if shapes_kept {
        CopySource::First
    } else if several || lattice_dropped || fitting_dropped {
        match (envelope, cur) {
            (true, true) => CopySource::First,
            (false, true) => CopySource::Working,
            (_, false) => CopySource::Original,
        }
    } else if !cur || any {
        CopySource::First
    } else {
        CopySource::Working
    }
}

fn deformed_by(models: &Models, node: NodeId) -> (bool, bool) {
    let lattice = model_lattice(models, node).is_some();
    let fitting = model_spline_fitting(models, node).is_some() || model_patch_fitting(models, node).is_some();
    (lattice, fitting)
}

/// Duplicates one node without its links [soft 0x0846234c SI_modelCopyNode]. The copy is a
/// new node of the same name ([`Models::allocate`]).
///
/// **Copied from the record**: kind, the bounding box with its valid flag, matte and garbage
/// matte, the local scaling, angles, rotation matrix and translation, the global matrix
/// `gmat`, visibility, motion blur, the caustic and global-illumination words, the
/// wire-frame display word, shape interpolation and `unk_2e2`. Everything else is as a new
/// node has it: no materials, textures, lattices, fittings, clusters or user data come
/// along (the callers that want them use `SI_modelCopyFromSetup`).
///
/// **Geometry.** The original definition is copied ([`copy_org_def`]). Then one definition
/// is added to the copy, a copy of the one [`copy_source`] chooses; for a mesh copied from
/// the first stored definition of an enveloped node the display lines are rebuilt
/// (`SI_meshOptimizeDisplay`); for a patch bit 0x2 of every point tag is cleared. A metaball
/// system and a metaball are always copied from the first stored definition, by routines
/// that take the copy node. Nulls and joints have no geometry.
///
/// **The rest**: the animation algorithm goes through `SI_modelAlgoTypeCopy`
/// ([`algo_type_copy`]); the envelope type by the rule below; the initial transformation is
/// duplicated; `unk_320` is cleared; dynamics and inverse kinematics through their own copy
/// routines (hooks); and, unless the source's envelope type is 5, 6 or 8,
/// [`update_dependency`] on the copy.
///
/// The envelope type (+0x31c) of a source that has one other than 3: with "detach" true
/// when the node is selected on screen ([`Hooks::graf_selection`] is 1; a node that is not on
/// screen makes the original read through a null pointer) or `as_envelope` is set, and never under `SI_edSumatraTo3x`: types 5 to 8 are dropped (the copy keeps 0)
/// when detaching; types 9 to 11 become 9 with `SI_envelopCopy(source, NULL, copy)` when
/// detaching; in every other case the type is copied.
///
/// Where the original would read through a null pointer (a geometry kind without stored
/// definitions, a working definition that is null) no definition is added here. A source
/// whose name pointer is null (the original then gives the copy a default name `obj<n>`)
/// cannot be told from one with an empty name here; the copy takes the name as it is.
pub fn copy_node(models: &mut Models, source: NodeId, as_envelope: bool, g: &Globals, env: &mut dyn Env) -> NodeId {
    let name = models.data[source].name.clone();
    let copy = models.allocate(Some(&name));
    let s = *models.m(source);
    {
        let c = models.m_mut(copy);
        c.kind = s.kind;
        c.bbox_valid = s.bbox_valid;
        c.matte = s.matte;
        c.garbage_matte = s.garbage_matte;
        c.bbox_min = s.bbox_min;
        c.bbox_max = s.bbox_max;
        c.scal = s.scal;
        c.vrot = s.vrot;
        c.mrot = s.mrot;
        c.trans = s.trans;
        c.gmat = s.gmat;
        c.visibility = s.visibility;
        c.motion_blur = s.motion_blur;
        c.caustic = s.caustic;
        c.global_illum = s.global_illum;
        c.wf_display = s.wf_display;
        c.shape_interp = s.shape_interp;
        c.unk_2e2 = s.unk_2e2;
    }
    if g.classical_scaling {
        env.classic_comp_info_copy(models, source, copy);
    }
    let holder = Models::id(s.latdep).filter(|&h| h != source);
    let (mut lattice, mut fitting) = deformed_by(models, source);
    if let Some(h) = holder {
        let (l, f) = deformed_by(models, h);
        lattice |= l;
        fitting |= f;
    }
    let envelope = matches!(s.envelop, 5 | 6 | 8);
    let which = copy_source(&g.duplic, s.nb_defs, lattice, fitting, envelope, as_envelope);

    let org = models.data[source].org_def.as_deref().cloned();
    copy_org_def(models, copy, org.as_ref(), env);

    let kind = s.kind;
    let src = &models.data[source];
    let chosen: Option<Definition> = match kind {
        MPTCH | MFACE | MSMSH | MSPLN | MNCRV | MNSRF => match which {
            CopySource::Working => match &src.current {
                Current::Own(b) => Some((**b).clone()),
                Current::Def(i) => src.defs.get(*i).cloned(),
                Current::None => None,
            },
            CopySource::Original => src.org_def.as_deref().or(src.defs.first()).cloned(),
            CopySource::First => src.defs.first().cloned(),
        },
        MMETA | MBALL => src.defs.first().cloned(),
        _ => None,
    };
    if let Some(d) = chosen {
        let to = matches!(kind, MMETA | MBALL).then_some(copy);
        let new = env.copy_def(kind, &d, to);
        hierarchy::add_definition(models, copy, kind, new);
        match (kind, &mut models.data[copy].defs[0]) {
            (MSMSH, Definition::Mesh(m)) if which == CopySource::First && envelope => crate::mesh_b::optimize_display(m),
            (MPTCH, Definition::Patch(p)) => {
                let n = p.nu as usize * p.nv as usize;
                for t in p.tags.iter_mut().take(n) {
                    *t &= !2;
                }
            }
            _ => {}
        }
    }
    models.m_mut(copy).algo_type = algo_type_copy(s.algo_type);
    if s.envelop != 0 && s.envelop != 3 {
        let detach = !g.sumatra_to_3x && (env.graf_selection(models, source, 0) == Some(1) || as_envelope);
        match s.envelop {
            5..=8 if detach => {}
            9..=11 if detach => {
                models.m_mut(copy).envelop = 9;
                hierarchy::Hooks::envelop_copy(env, models, source, None, copy);
            }
            e => models.m_mut(copy).envelop = e,
        }
    }
    if models.data[source].init_srt.is_some() {
        models.data[copy].init_srt = models.data[source].init_srt;
    }
    models.m_mut(copy).unk_320 = Ptr32::NULL;
    env.dyna_info_copy(models, source, copy);
    let ik = env.inv_kin_copy(models, source, copy);
    models.m_mut(copy).inv_kin = Ptr32::new(ik);
    if !envelope {
        update_dependency(models, copy, None, g, env);
    }
    copy
}

/// The animation algorithm a copy gets from its source's [soft 0x084a0efc SI_modelAlgoTypeCopy]
/// (`settings.c`): 4 becomes 3, 2 becomes 1, 8 becomes 5, 12 becomes 0, any other value is
/// kept. (A chain root or effector of the second kind is copied as the first kind; a node
/// under dynamics loses that.)
pub fn algo_type_copy(algo: i32) -> i32 {
    match algo {
        4 => 3,
        2 => 1,
        8 => 5,
        12 => 0,
        a => a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;

    fn mesh(n: usize) -> Definition {
        Definition::Mesh(Mesh::with_vertices(&vec![[0.0; 3]; n]))
    }

    fn mesh_node(models: &mut Models, shapes: usize) -> NodeId {
        let n = models.allocate(None);
        for i in 0..shapes {
            hierarchy::add_definition(models, n, MSMSH, mesh(3 + i));
        }
        n
    }

    #[test]
    fn the_copied_shape_follows_the_duplicate_settings() {
        use CopySource::*;
        let cur = Duplic { geometry_curr: true, ..Duplic::default() };
        let rest = Duplic::default();
        // a plain node: the shape as it is now, or its first stored shape
        assert_eq!(copy_source(&cur, 1, false, false, false, false), Working);
        assert_eq!(copy_source(&rest, 1, false, false, false, false), First);
        // several shapes kept as shape animation: always the first
        let shapes = Duplic { shape_anim: true, ..cur };
        assert_eq!(copy_source(&shapes, 3, true, true, true, true), First);
        // several shapes otherwise: the working one, or the original without "current"
        assert_eq!(copy_source(&cur, 3, false, false, false, false), Working);
        assert_eq!(copy_source(&rest, 3, false, false, false, false), Original);
        // a lattice that is not duplicated: the deformed shape is frozen into the copy
        assert_eq!(copy_source(&cur, 1, true, false, false, false), Working);
        // ... one that is duplicated: the undeformed shape, to be deformed again
        assert_eq!(copy_source(&Duplic { lattice_dup: true, ..cur }, 1, true, false, false, false), First);
        // an enveloped node copied as an envelope takes the deformed shape with "current"
        assert_eq!(copy_source(&cur, 1, false, false, true, true), Working);
        assert_eq!(copy_source(&cur, 1, false, false, true, false), First);
    }

    #[test]
    fn a_copy_takes_one_definition_and_the_listed_fields_only() {
        let mut models = Models::default();
        let src = mesh_node(&mut models, 2);
        models.m_mut(src).visibility = 3;
        models.m_mut(src).nb_materials = 2;
        models.m_mut(src).algo_type = 4;
        models.m_mut(src).envelop = 5;
        models.data[src].init_srt = Some([si_types::geom::Vector { x: 1.0, y: 2.0, z: 3.0 }; 3]);
        let g = Globals::default();
        let copy = copy_node(&mut models, src, false, &g, &mut ());
        assert_eq!(models.data[copy].name, models.data[src].name);
        assert_eq!((models.m(copy).nb_defs, models.m(copy).kind, models.m(copy).visibility), (1, MSMSH, 3));
        assert_eq!(models.m(copy).nb_materials, 0);
        assert_eq!(models.m(copy).algo_type, 3); // SI_modelAlgoTypeCopy: 4 becomes 3
        assert_eq!(models.m(copy).envelop, 5); // not selected, not asked for: the type is kept
        assert_eq!(models.data[copy].init_srt, models.data[src].init_srt);
        // several shapes, no "current": the original definition, or the first stored one
        let Definition::Mesh(m) = &models.data[copy].defs[0] else { panic!() };
        assert_eq!(m.vertices.len(), 3);
    }

    struct Log(Vec<String>);
    impl Hooks for Log {
        fn free(&mut self, block: u32) {
            self.0.push(format!("free {block:#x}"));
        }
        fn dispose_def(&mut self, _: &mut Models, _: NodeId, _: i32, slot: DefSlot) {
            self.0.push(format!("dispose {slot:?}"));
        }
        fn lattice_dispose(&mut self, _: &mut Models, _: NodeId, set: usize, index: usize) {
            self.0.push(format!("lattice {set} {index}"));
        }
        fn kluster_dispose_all(&mut self, _: &mut Models, _: NodeId) {
            self.0.push("klusters".to_string());
        }
    }
    impl hierarchy::Hooks for Log {}
    impl crate::transform::Hooks for Log {}
    impl crate::transform::GeometryHooks for Log {}

    #[test]
    fn disposal_goes_in_the_originals_order() {
        let mut models = Models::default();
        let n = mesh_node(&mut models, 2);
        {
            let m = models.m_mut(n);
            m.name = Ptr32::new(0x10);
            m.lattices0 = Ptr32::new(0x20);
            m.nb_lattices0 = 2;
            m.unk_32c = Ptr32::new(0x30);
            m.defs = Ptr32::new(0x40);
        }
        let mut log = Log(Vec::new());
        dispose_node(&mut models, n, &Globals::default(), &mut log);
        assert_eq!(
            log.0,
            ["dispose Stored(0)", "dispose Stored(1)", "free 0x40", "free 0x10", "lattice 0 0", "lattice 0 1", "free 0x20", "free 0x30", "klusters", "free 0x1"]
        );
        assert!(models.data[n].disposed && models.data[n].defs.is_empty());
        assert_eq!((models.m(n).kind, models.m(n).nb_defs, models.m(n).nb_lattices0), (0, 0, 0));
    }

    #[test]
    fn a_branch_lattice_holds_the_geometry_below_it() {
        // root(0) with a branch lattice; a(1) under it; b(2) under a
        let mut models = Models::default();
        let (root, a, b) = (models.allocate(None), mesh_node(&mut models, 1), mesh_node(&mut models, 1));
        models.arena.create_parentship(root, a);
        models.arena.create_parentship(a, b);
        models.m_mut(root).nb_lattices1 = 1;
        models.data[root].cx.lattices[1].push(LatticeRef { ptr: Ptr32::new(0x50), active: 1 });
        let g = Globals::default();
        update_dependency(&mut models, root, None, &g, &mut ());
        assert_eq!(models.m(root).latdep, Models::ptr(root));
        assert_eq!((models.m(a).latdep, models.m(b).latdep), (Models::ptr(root), Models::ptr(root)));
        // each node below now works on a copy of its own
        assert!(matches!(models.data[a].current, Current::Own(_)) && matches!(models.data[b].current, Current::Own(_)));
        // the lattice goes: both return to their stored definition
        models.data[root].cx.lattices[1][0].active = 0;
        update_dependency(&mut models, root, None, &g, &mut ());
        assert_eq!((models.m(a).latdep, models.m(b).latdep), (Models::ptr(a), Models::ptr(b)));
        assert_eq!((models.data[a].current.clone(), models.data[b].current.clone()), (Current::Def(0), Current::Def(0)));
    }
}
