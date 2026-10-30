// SPDX-License-Identifier: MIT
//! What the elements of a loaded scene stand for: the objects read from the element files.
//!
//! In the original an element's `data` is a pointer to the object (`__SI_Model`,
//! `SI_Camera`, `light`, `__SI_Material` …). Here it is a handle into [`Scene::objects`]
//! (`crate::load::Scene`); two elements share an object exactly when their handles are
//! equal, as in the original (a shape's element points at its model; a light and its
//! interest point at one light; every camera element points at the scene's one camera).

use si_elemio::{anim, camera, lattice, light, shape};
use si_hrc::Node;

/// One model file in memory: the hierarchy as `si-hrc` read it, with what the dictionary
/// code keeps per node. Nodes are numbered in the order of the hierarchy walk the original
/// uses everywhere (`modelGetFromHierarchy`): a node, the nodes under its first son, then
/// its next brother.
#[derive(Clone, Debug)]
pub struct ModelTree {
    pub root: Box<Node>,
    /// Per node: its name (`__SI_Model` +4).
    pub names: Vec<Vec<u8>>,
    /// Per node: the index of its element in the MODELS chapter (`__SI_Model` +0x354), -1
    /// until an element claims the node.
    pub elem_ids: Vec<i32>,
    /// Per node: the handle of the object that stands for the node (0 until one is made).
    pub handles: Vec<u32>,
    /// Per node: its type (`__SI_Model` +0x30).
    pub kinds: Vec<i32>,
    /// Per node: the garbage matte flag (+0x3f8), which the loader may set.
    pub garbage_matte: Vec<i16>,
    /// Per node: the materials the node points at (`__SI_Model` +0x20c) once the loader has
    /// tied it to material elements (`SI_scnRelationsFromElemDict`, relation 300): handles
    /// of the materials' objects, in the order of the relation list. `None` while the node
    /// keeps the materials its own file gave it.
    pub mat_links: Vec<Option<Vec<u32>>>,
    /// Per node: its global 2D textures (+0x21c, a linked list) once tied to texture
    /// elements (relation 400): the count stored at +0x216 and the head of the list (the
    /// links are in `Scene::tex_next`).
    pub tex_links: Vec<Option<(i16, Option<u32>)>>,
    /// Per node: its 3D texture (+0x224) once relation 500 has been gone through (the old
    /// one is dropped even if no element then gives one).
    pub tex3d_links: Vec<Option<Option<u32>>>,
    /// Per node: scaling, rotation and translation stored by `SI_modelSetState` (+0x100,
    /// +0x10c, +0x158); `None` while the node has the file's.
    pub state: Vec<Option<[f32; 9]>>,
    /// Per node: the model's "dirty" word (+0x0a): what has changed since the last save.
    /// Bits 1 to 3 are set by `SI_modelSetState`.
    pub dirty: Vec<u16>,
    /// Per node: the node's record in the memory image, as the reader on the image made
    /// it (`crate::image::read_model`); empty when the file's hierarchy is not in the
    /// image.
    pub image: Vec<u32>,
}

impl ModelTree {
    pub fn new(root: Box<Node>) -> ModelTree {
        let mut names = Vec::new();
        let mut kinds = Vec::new();
        let mut gm = Vec::new();
        root.walk(0, &mut |_, n| {
            names.push(si_hrc::io::c_str(&n.name).to_vec());
            kinds.push(n.kind);
            gm.push(n.garbage_matte);
        });
        let count = names.len();
        ModelTree {
            root,
            names,
            elem_ids: vec![-1; count],
            handles: vec![0; count],
            kinds,
            garbage_matte: gm,
            mat_links: vec![None; count],
            tex_links: vec![None; count],
            tex3d_links: vec![None; count],
            state: vec![None; count],
            dirty: vec![0; count],
            image: Vec::new(),
        }
    }

    /// The node with a given number.
    pub fn node(&self, index: usize) -> Option<&Node> {
        let mut found = None;
        let mut i = 0usize;
        self.root.walk(0, &mut |_, n| {
            if i == index {
                found = Some(n);
            }
            i += 1;
        });
        found
    }
}

/// An object an element points at.
#[derive(Clone, Debug)]
pub enum Object {
    /// A node of a model hierarchy: the tree (a handle into `Scene::models`) and the node's
    /// number in it.
    ModelNode { tree: u32, node: usize },
    /// The scene's camera (`SI_procData` + 8): there is one, and every camera file is read
    /// into it.
    Camera(Box<camera::Camera>),
    Light(Box<light::Light>),
    Material(Box<si_hrc::Material>),
    Texture2D(Box<si_hrc::Texture2D>),
    Texture3D(Box<si_hrc::Texture3D>),
    /// An animation or struct key file (`SI_HierAnim`).
    Animation(Box<anim::HierAnim>),
    /// The first lattice of a model's node list or branch list, as read from a lattice file.
    Lattice(Box<lattice::Lattice>),
    /// One item of a hierarchy animation: a function curve (or a static value) of an
    /// animation file in memory. The original's pointer is to the curve itself, which the
    /// hierarchy animation owns; here it is named by the animation's object, the element of
    /// its list and the item's position.
    Curve { anim: u32, elem: usize, item: usize },
    /// A cluster of a model node (`kluster`): what an element of the CLUSTERS chapter points
    /// at once `si_edUpdateClusterPointers` has run. `model` is the node's object, `cluster`
    /// the cluster's position in the node's list.
    Cluster { model: u32, cluster: usize },
    /// The one byte `SI_elemdictCheckSetupElem` gives a setup element (a copy of the string
    /// `" "`) or the twelve zero bytes it gives the root expression, so that the element
    /// counts as having something to save; the element writer frees it again.
    Placeholder,
    /// A named selection made for an element of the NSELECTIONS chapter
    /// (`SI_nselectionAllocate(name, 0)`).
    NSelection { name: Vec<u8> },
    /// An object made by a package that is not ported: what made it and from which file.
    /// (A wave, an expression, a shader, a spreadsheet setup; a default material, texture,
    /// light or cube put in place of a missing file.)
    Foreign { made_by: &'static str, path: Vec<u8> },
}

/// What a material points at besides its own record, once the loader has tied it to
/// texture elements (`SI_scnRelationsFromElemDict`, relations 401 and 501): its 2D textures
/// (`__SI_Material` +0x6ec, a linked list, with the count at +0x6e8) and its 3D texture
/// (+0x6fc), as handles of the textures' objects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialLinks {
    /// The count stored at +0x6e8 and the head of the list.
    pub textures: Option<(i16, Option<u32>)>,
    pub texture3d: Option<Option<u32>>,
}

/// What a cluster file put into its model: the key data of the model's clusters, the
/// model's cluster animation record and its current shape.
#[derive(Clone, Debug)]
pub struct ClustersOfModel {
    /// Handle of the model node's object.
    pub model: u32,
    /// The node's clusters (name and number of points from the model file) with the key
    /// data the cluster files gave them.
    pub klusters: Vec<si_elemio::cluster::Kluster>,
    pub anim: Option<si_elemio::cluster::ClusterAnim>,
    /// The model's current shape (model +0x3c), if a cluster file held one.
    pub current: Option<si_elemio::geom::Geometry>,
}

/// The lattices the lattice files put into a model (`SI_modelAddILattice`): its list of
/// node lattices (`__SI_Model` +0x50, count +0x4c) or of branch lattices (+0x58, +0x54), in
/// the order they were read.
#[derive(Clone, Debug)]
pub struct LatticesOfModel {
    /// Handle of the model node's object.
    pub model: u32,
    pub branch: bool,
    pub list: Vec<lattice::Lattice>,
}

/// What a shape file put into its model (the element of the shape points at the model).
#[derive(Clone, Debug)]
pub struct ShapeOfModel {
    /// Handle of the model node's object.
    pub model: u32,
    pub shapes: shape::ShapeModel,
    pub accepted: bool,
}
