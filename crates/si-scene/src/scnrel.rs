// SPDX-License-Identifier: MIT
//! The scene's own structures tied from the dictionary's relations (`db2scn.c`), as far as
//! a save reads them back: which materials and textures a model node points at, which
//! textures a material points at, which cluster an element of the CLUSTERS chapter stands
//! for; and a model file's hierarchy put together from all that for the writer.
//!
//! The original keeps pointers: after a load a model node's material array holds the very
//! records the MATERIALS elements point at, so one record is written into the material's own
//! file and into every model file that uses it. Here the nodes and materials keep handles
//! ([`ModelTree::mat_links`], [`Scene::mat_links`], [`Scene::tex_next`]) and the hierarchy to
//! write is assembled from them ([`assemble_model`]).

use crate::dict::ElemId;
use crate::load::Scene;
use crate::object::{ClustersOfModel, MaterialLinks, Object};
use crate::world::{cid, find_related, list, node_text, rel_data_of, rel_list, slot};
use si_ascii::reldata::RelData;
use si_dict::elemdict;
use si_hrc::{Material, Node, Texture2D};
use si_mem::{Mem, Ptr32};
use si_types::dict::{ARelation, ElemDict, ELEM_DELETED};

/// Appends a texture to a linked list of textures (`__SI_LclMap` +0x16c is the link), the
/// way the original does it for a model's global textures and a material's textures: an
/// empty list takes the texture as its head; otherwise the list is walked to its last
/// record, and the texture is linked behind it unless it is that last record or was met on
/// the way. **The texture's own link is not cleared**: a texture already in another list
/// brings what follows it there. Returns the head.
fn append_texture(sc: &mut Scene, head: Option<u32>, tex: u32) -> Option<u32> {
    let Some(first) = head else { return Some(tex) };
    let mut new = true;
    let mut p = first;
    let mut steps = 0;
    while let Some(&next) = sc.tex_next.get(&p) {
        if p == tex {
            new = false;
        }
        p = next;
        steps += 1;
        if steps > 100_000 {
            // a list that runs into itself: the original walks it for ever
            return head;
        }
    }
    if new && p != tex {
        sc.tex_next.insert(p, tex);
    }
    head
}

/// The textures of a linked list, in order. A list that runs into itself is cut where it
/// closes (the original's writers would not come back from it).
pub fn texture_list(sc: &Scene, head: Option<u32>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut p = head;
    while let Some(h) = p {
        if out.contains(&h) {
            break;
        }
        out.push(h);
        p = sc.tex_next.get(&h).copied();
    }
    out
}

/// The node of a model chain whose dynamics record (`__SI_Model` +0x304) relation 800
/// reads and writes: the node itself, or, for a node of kind 0 whose algorithm is 3 or 4,
/// the first ancestor of kind 0 whose algorithm is 1 or 2.
fn dynamics_node(sc: &Scene, tree: u32, node: usize) -> Option<usize> {
    let t = sc.models.get(tree)?;
    // the fathers, from the hierarchy walk: a node's father is the last node one level up
    let mut parents: Vec<Option<usize>> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0usize;
    t.root.walk(0, &mut |depth, _| {
        stack.truncate(depth);
        parents.push(stack.last().copied());
        stack.push(i);
        i += 1;
    });
    let n = t.node(node)?;
    let mut at = node;
    if n.kind == 0 && (n.algorithm == 3 || n.algorithm == 4) {
        let mut alg = n.algorithm;
        while !(alg == 1 || alg == 2) {
            loop {
                at = (*parents.get(at)?)?;
                if t.node(at)?.kind == 0 {
                    break;
                }
            }
            alg = t.node(at)?.algorithm;
        }
    }
    Some(at)
}

/// The part of `SI_scnRelationsFromElemDict(dictionary, …, chapter, list, relation)`
/// (0x0810b320) that a save reads back. The routine goes through the elements of the
/// chapter that have an object and are of this load (new-flag 1), and for each through the
/// entries of one forward list that carry the relation id and do not lead to ANIMATION; an
/// entry counts when its other end is a slot not freed, and takes effect when that element
/// has an object.
///
/// | Relation | Element | What is done |
/// |---|---|---|
/// | 300 | model | if an entry takes effect: the node's own materials are disposed of and its array (+0x20c) is rebuilt, one material element's record per entry in list order; +0x206 is their number |
/// | 400 | model | the node's global 2D textures (+0x21c) are disposed of, +0x216 becomes the number of entries that take effect, and each texture element's record is appended to the list |
/// | 500 | model | the node's 3D texture (+0x224) is freed and replaced by each entry's in turn |
/// | 401 | material | as 400, on the material's list (+0x6ec, count +0x6e8) |
/// | 501 | material | as 500, on +0x6fc |
/// | 800 | model | the sixteen numbers of the relation's data go into the chain's dynamics record as integers (16 bits, cut as a C cast does; the twelfth and fourteenth 32 bits) and the data is freed. The record is another package's: here the numbers stay with the relation, cut the same way, when the chain's node has a record |
/// | 807 | model | the same for the one number of an obstacle (the constraint record is another package's: the number stays with the relation, cut to 16 bits) |
///
/// Everything else the routine does (fittings, waves, constraints, the camera's and lights'
/// links, light selections, named selections, custom effects) writes into records that are
/// not ported; the caller records the call. Not cited: the routine is reproduced in part.
pub fn scn_relations_from_elem_dict(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>, chap: i32, group: i32, relid: i32) {
    let cid = cid(m);
    let on_model = chap == cid.model && [300, 400, 500, 800, 0x327].contains(&relid);
    let on_material = chap == cid.mat && [0x191, 0x1f5].contains(&relid);
    if !(on_model || on_material) || group < 0 {
        return;
    }
    let l = list(m, ed, chap);
    for i in 0..m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].data.is_null() || m[e].new_flag != 1 {
            continue;
        }
        let data = sc.obj(m[e].data);
        let rl = rel_list(m, e, true, group);
        if rl.is_null() {
            continue;
        }
        let entries: Vec<(i32, ARelation)> = (0..m[rl].nb).map(|k| (k, m.read(m[rl].rels.add(k)))).filter(|(_, r)| r.relid == relid && r.chapid as i32 != cid.ani).collect();
        if entries.is_empty() {
            continue;
        }
        // the object at the other end of each entry (`SI_elemdictGetElem`: none for a slot
        // out of range or freed)
        let with_obj: Vec<(i32, u32)> = entries
            .iter()
            .filter_map(|(k, r)| {
                let o = slot(m, ed, ElemId::new(r.chapid as i32, r.elemid)).filter(|&o| m[o].status != ELEM_DELETED && !m[o].data.is_null())?;
                Some((*k, sc.obj(m[o].data)))
            })
            .collect();
        let node_of = match sc.objects.get(data) {
            Some(Object::ModelNode { tree, node }) => Some((*tree, *node)),
            _ => None,
        };
        match relid {
            300 => {
                let Some((tree, node)) = node_of else { continue };
                if !with_obj.is_empty() {
                    let handles: Vec<u32> = with_obj.iter().map(|&(_, h)| h).collect();
                    if let Some(t) = sc.models.get_mut(tree) {
                        t.mat_links[node] = Some(handles);
                    }
                }
            }
            400 => {
                let Some((tree, node)) = node_of else { continue };
                let mut head = None;
                for &(_, h) in &with_obj {
                    head = append_texture(sc, head, h);
                }
                if let Some(t) = sc.models.get_mut(tree) {
                    t.tex_links[node] = Some((with_obj.len() as i16, head));
                }
            }
            500 => {
                let Some((tree, node)) = node_of else { continue };
                if let Some(t) = sc.models.get_mut(tree) {
                    t.tex3d_links[node] = Some(with_obj.last().map(|&(_, h)| h));
                }
            }
            0x191 => {
                let mut head = None;
                for &(_, h) in &with_obj {
                    head = append_texture(sc, head, h);
                }
                sc.mat_links.entry(data).or_insert_with(MaterialLinks::default).textures = Some((with_obj.len() as i16, head));
            }
            0x1f5 => {
                sc.mat_links.entry(data).or_insert_with(MaterialLinks::default).texture3d = Some(with_obj.last().map(|&(_, h)| h));
            }
            800 | 0x327 => {
                let Some((tree, node)) = node_of else { continue };
                // 800: only where the chain's node has a dynamics record. 807: where the
                // new constraint has an obstacle record (taken as always: the constraint
                // code is not ported)
                let has_record = relid == 0x327 || dynamics_node(sc, tree, node).and_then(|n| sc.models.get(tree)?.node(n).map(|x| x.chain_dynamics)).unwrap_or(false);
                if !has_record {
                    continue;
                }
                for &(k, _) in &with_obj {
                    let handle = rel_data_of(sc, m[m[rl].rels.add(k)].data);
                    if let Some(RelData::Floats { values, .. }) = sc.rel_data.get_mut(handle) {
                        for (j, v) in values.iter_mut().enumerate() {
                            let whole = crate::elemsort::c_cast_to_int(*v as f64);
                            *v = if relid == 800 && (j == 11 || j == 13) { whole as f32 } else { whole as i16 as f32 };
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// [soft 0x0810af54 si_edUpdateClusterPointers]: every element in use of this load in the
/// CLUSTERS chapter that is related to a model (relation 700, backward side) is pointed at
/// that model's cluster of the element's name (`SI_klusterGetByName`, the node's list
/// +0x348), or else at its cluster named by what follows the first characters of the
/// element's name, as many as the model's name has (a cluster element is named
/// `<model><cluster>`); a null pointer when the model has neither. The chapter's look-up
/// table is marked dirty first.
///
/// On the image as the original does it: the model is the related element's object (the
/// node the model file was read into), the name the element's name record's (+0x14); for a
/// name shorter than the model's the original reads behind the name's end, and so does
/// this. The stores' form of the same choice (a cluster named by its model node and its
/// position in the node's list, for the saver) is tied to the record found
/// ([`Scene::tie`]); where the model element's object is not a record of the image the
/// element keeps the stores' handle.
pub fn ed_update_cluster_pointers(m: &mut Mem, sc: &mut Scene, ed: Ptr32<ElemDict>) {
    let cid = cid(m);
    let l = list(m, ed, cid.cls);
    elemdict::sort_tbl_set_dirty(m, l);
    for i in 0..m[l].nb_elems {
        let e = m[l].elems.add(i);
        if m[e].status != 0 || m[e].new_flag != 1 {
            continue;
        }
        let Some((mp, _)) = find_related(m, ed, e, 0, 0, 700, cid.model, true) else { continue };
        // the stores' form
        let model = sc.obj(m[mp].data);
        let name = node_text(m, m[e].node, |n| n.name).unwrap_or_default();
        let model_name_len = node_text(m, m[mp].node, |n| n.name).map_or(0, |x| x.len());
        let names: Vec<Vec<u8>> = match sc.objects.get(model) {
            Some(Object::ModelNode { tree, node }) => sc.models.get(*tree).and_then(|t| t.node(*node)).map(|n| n.clusters.iter().map(|c| si_hrc::io::c_str(&c.name).to_vec()).collect()).unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut found = names.iter().position(|n| *n == name);
        if found.is_none() {
            found = name.get(model_name_len..).and_then(|rest| names.iter().position(|n| n == rest));
        }
        let handle = match found {
            Some(cluster) => sc.objects.alloc(Object::Cluster { model, cluster }),
            None => 0,
        };
        // the image's
        let node = m[mp].data;
        if node.is_null() || si_mem::handle::Kind::of(node.addr()).is_some() {
            m[e].data = sc.obj_addr(handle);
            continue;
        }
        let name_ptr = m[m[e].node].name.addr();
        let mut k = si_deform::klusters::kluster_get_by_name(m, node.cast(), name_ptr);
        if k.is_null() {
            let model_name = m[m[mp].node].name.addr();
            let n = si_mem::clib::strlen(m, model_name);
            k = si_deform::klusters::kluster_get_by_name(m, node.cast(), name_ptr.wrapping_add(n));
        }
        m[e].data = k.cast();
        sc.tie(handle, k.addr());
    }
}

/// What the cluster files gave a model, made on first use from the node's own clusters
/// (name and number of points, as the model file has them).
pub fn clusters_of_model(sc: &mut Scene, model: u32) -> Option<usize> {
    if let Some(p) = sc.clusters.iter().position(|c| c.model == model) {
        return Some(p);
    }
    let klusters = match sc.objects.get(model) {
        Some(Object::ModelNode { tree, node }) => sc
            .models
            .get(*tree)
            .and_then(|t| t.node(*node))
            .map(|n| n.clusters.iter().map(|c| si_elemio::cluster::Kluster { name: si_hrc::io::c_str(&c.name).to_vec(), nb_points: c.count, data: None }).collect::<Vec<_>>())?,
        _ => return None,
    };
    sc.clusters.push(ClustersOfModel { model, klusters, anim: None, current: None });
    Some(sc.clusters.len() - 1)
}

/// A material as the writers see it: its own record with the textures it points at.
/// `None` when the handle is not a material (a default record put in place of a missing
/// file is another package's).
pub fn material_value(sc: &Scene, handle: u32) -> Option<Material> {
    let Some(Object::Material(m)) = sc.objects.get(handle) else { return None };
    let mut m = (**m).clone();
    if let Some(links) = sc.mat_links.get(&handle) {
        if let Some((_, head)) = &links.textures {
            m.textures = texture_list(sc, *head).into_iter().filter_map(|h| texture_value(sc, h)).collect();
        }
        if let Some(t3) = &links.texture3d {
            m.texture3d = t3.and_then(|h| match sc.objects.get(h) {
                Some(Object::Texture3D(t)) => Some((**t).clone()),
                _ => None,
            });
        }
    }
    Some(m)
}

fn texture_value(sc: &Scene, handle: u32) -> Option<Texture2D> {
    match sc.objects.get(handle) {
        Some(Object::Texture2D(t)) => Some((**t).clone()),
        _ => None,
    }
}

fn walk_mut(node: &mut Node, index: &mut usize, f: &mut dyn FnMut(usize, &mut Node)) {
    let mut n = Some(node);
    while let Some(x) = n {
        f(*index, x);
        *index += 1;
        if let Some(s) = x.son.as_deref_mut() {
            walk_mut(s, index, f);
        }
        n = x.brother.as_deref_mut();
    }
}

/// What could not be put into an assembled hierarchy: a node points at a record that is not
/// a material or texture here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assembled {
    /// Material or texture handles that are no material or texture of this crate.
    pub foreign: usize,
}

/// Which of the things the program changes in a loaded hierarchy go into an assembled one:
/// all of them for a save; one at a time for the checks, to tell why a written file
/// differs from the one that was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// The nodes' names (`SI_modelChangeName`).
    pub names: bool,
    /// The transformation `SI_modelSetState` stored.
    pub state: bool,
    /// The materials and textures of the material and texture elements.
    pub links: bool,
}

impl Parts {
    pub const ALL: Parts = Parts { names: true, state: true, links: true };
    pub const NONE: Parts = Parts { names: false, state: false, links: false };
}

/// A model file's hierarchy as the program has it in memory when it saves: the nodes as
/// read, each with its name (`SI_modelChangeName`), the materials, 2D textures and 3D
/// texture it points at, and the transformation `SI_modelSetState` stored.
pub fn assemble_model(sc: &Scene, tree: u32) -> Option<(Box<Node>, Assembled)> {
    assemble_model_parts(sc, tree, Parts::ALL)
}

/// [`assemble_model`] with only some of the changes applied.
pub fn assemble_model_parts(sc: &Scene, tree: u32, parts: Parts) -> Option<(Box<Node>, Assembled)> {
    let t = sc.models.get(tree)?;
    let mut root = t.root.clone();
    let mut report = Assembled::default();
    let mut index = 0usize;
    walk_mut(&mut root, &mut index, &mut |i, n| {
        if parts.names {
            if let Some(name) = t.names.get(i) {
                if si_hrc::io::c_str(&n.name) != &name[..] {
                    n.name = name.clone();
                }
            }
        }
        if parts.state {
            if let Some(Some(s)) = t.state.get(i) {
                n.scaling = [s[0], s[1], s[2]];
                n.rotation = [s[3], s[4], s[5]];
                n.translation = [s[6], s[7], s[8]];
            }
        }
        if !parts.links {
            return;
        }
        if let Some(Some(handles)) = t.mat_links.get(i) {
            n.nb_materials = handles.len() as i16;
            n.materials = handles
                .iter()
                .map(|&h| {
                    let m = material_value(sc, h);
                    report.foreign += m.is_none() as usize;
                    m
                })
                .collect();
        }
        if let Some(Some((count, head))) = t.tex_links.get(i) {
            n.nb_textures = *count;
            let list = texture_list(sc, *head);
            n.textures = list.iter().filter_map(|&h| texture_value(sc, h)).collect();
            report.foreign += list.len() - n.textures.len();
        }
        if let Some(Some(t3)) = t.tex3d_links.get(i) {
            n.texture3d = t3.and_then(|h| match sc.objects.get(h) {
                Some(Object::Texture3D(x)) => Some((**x).clone()),
                _ => None,
            });
        }
    });
    Some((root, report))
}
