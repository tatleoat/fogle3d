// SPDX-License-Identifier: MIT
//! The hierarchy of model nodes: links, traversal orders, definitions, names, duplication
//! and deletion. From the original's `model_b.c`, `names.c` and the part of `model.c` that
//! is not about transformations (finding 0015).
//!
//! Every routine follows the same three links the original follows (`father`, `brother`,
//! `son` of the record; see [`crate::node`]), so each traversal order here is the
//! original's by construction, and the tests pin them.
//!
//! All of this is integer and pointer work: no floating point.

use crate::env::Env;
use crate::node::{Current, Definition, Models, NodeId};
use crate::node_copy;
use crate::obj_global;
use crate::transform::Globals;

/// Owned data of a node that the hierarchy routines need beyond [`crate::node::NodeData`].
#[derive(Clone, Debug, Default)]
pub struct NodeExt {}

/// What the hierarchy routines call in parts of the program that are not ported. Every
/// method has a default that does nothing (or the plain thing), so `()` is a usable set.
pub trait Hooks {
    /// `SI_metaballTouch(NULL, system)`: a metaball system above a node being cut loose must
    /// rebuild its surface.
    fn metaball_touch(&mut self, _models: &mut Models, _system: NodeId) {}
    /// `SI_envelopCut(node)`: envelopes spanning the cut are undone.
    fn envelop_cut(&mut self, _models: &mut Models, _node: NodeId) {}
    /// `SI_envelopCopy(source, father of the copy, copy)`.
    fn envelop_copy(&mut self, _models: &mut Models, _source: NodeId, _father: Option<NodeId>, _copy: NodeId) {}
    /// `SI_patchNbTriangles`, `SI_faceNbTriangles`, `SI_nurbsSrfNoTriangles`, and for a
    /// metaball system `SI_metaSysGetSurface` followed by its triangle count: the count for
    /// a definition that is not a mesh.
    fn nb_triangles_of(&mut self, _definition: &Definition) -> i32 {
        0
    }
    /// `SI_elemdictGetNewNameId(dictionary, MODELS, prefix, &id, 0)`: an unused name with
    /// this prefix and its number. The default answers `<prefix>1`, 1.
    fn new_name_id(&mut self, prefix: &[u8]) -> (Vec<u8>, i32) {
        let mut name = prefix.to_vec();
        name.push(b'1');
        (name, 1)
    }
    /// `SI_edModelChangeName(node, name)`: the rename that also tells the element
    /// dictionary. The default is [`change_name`].
    fn ed_model_change_name(&mut self, models: &mut Models, node: NodeId, name: &[u8]) {
        change_name(models, node, name);
    }
}

impl Hooks for () {}

// ------------------------------------------------------------------ traversal

/// The node after `node` in depth-first order over the whole tree it belongs to: its first
/// son; else its next brother; else the next brother of the nearest ancestor that has one.
/// `None` after the last node [soft 0x08478340 SI_modelNextNode].
///
/// The walk is not confined to a subtree: started inside one it runs on through the
/// brothers of the subtree's root and of its ancestors. A root's own brother link is
/// followed too.
pub fn next_node(models: &Models, node: NodeId) -> Option<NodeId> {
    if let Some(s) = models.son(node) {
        return Some(s);
    }
    if let Some(b) = models.brother(node) {
        return Some(b);
    }
    let mut at = node;
    loop {
        match models.father(at) {
            None => return models.brother(at),
            Some(f) => {
                at = f;
                if let Some(b) = models.brother(f) {
                    return Some(b);
                }
            }
        }
    }
}

/// The root of the tree a node belongs to [soft 0x0847838c SI_modelRoot].
pub fn root(models: &Models, mut node: NodeId) -> NodeId {
    while let Some(f) = models.father(node) {
        node = f;
    }
    node
}

/// The node after `at` in depth-first order **within the subtree of `top`**; with `at`
/// `None`, `top`'s first son. `None` when the subtree is exhausted
/// [soft 0x0846d70c SI_modelNextSon]. `top` itself is never returned.
///
/// The original climbs from `at` through father links until it meets `top`; a node outside
/// the subtree makes it run off the root through a null pointer. Here that ends the walk.
pub fn next_son(models: &Models, top: NodeId, at: Option<NodeId>) -> Option<NodeId> {
    let Some(mut at) = at else { return models.son(top) };
    if let Some(s) = models.son(at) {
        return Some(s);
    }
    if let Some(b) = models.brother(at) {
        return Some(b);
    }
    while at != top {
        at = models.father(at)?;
        if models.brother(at).is_some() {
            break;
        }
    }
    if at != top { models.brother(at) } else { None }
}

/// True when `ancestor` is `node` or one of its ancestors [soft 0x0846d764 SI_modelIsFather].
/// (A node counts as its own father here.)
pub fn is_father(models: &Models, ancestor: NodeId, node: Option<NodeId>) -> bool {
    let mut at = node;
    while let Some(n) = at {
        if n == ancestor {
            return true;
        }
        at = models.father(n);
    }
    false
}

/// The number of nodes below `node` [soft 0x0846d5ec SI_modelNbSubModels], counted by a
/// depth-first walk in a 16-bit counter (the result is that counter sign-extended: a
/// subtree of 32,768 nodes answers −32,768).
pub fn nb_sub_models(models: &Models, node: NodeId) -> i32 {
    if models.son(node).is_none() {
        return 0;
    }
    let mut count: i16 = 0;
    let mut at = node;
    loop {
        let before = count;
        let mut next = models.son(at).or(models.brother(at));
        if next.is_none() {
            // up to the nearest ancestor below `node` that has a brother
            let mut up = models.father(at);
            let mut found = None;
            while let Some(u) = up {
                if u == node {
                    break;
                }
                at = u;
                if let Some(b) = models.brother(u) {
                    found = Some(b);
                    break;
                }
                up = models.father(u);
            }
            next = found.or_else(|| models.brother(at)).or(Some(node));
            // (the brother of `at` tested last is always null on this path; the test is
            // the original's)
        }
        let next = next.unwrap();
        at = next;
        count = before.wrapping_add(1);
        if next == node {
            return before as i32;
        }
    }
}

/// The number of nodes from `node` up to its root, both included; 0 for no node
/// [soft 0x08473fa0 modelDepth].
pub fn model_depth(models: &Models, node: Option<NodeId>) -> i32 {
    let mut depth = 0;
    let mut at = node;
    while let Some(n) = at {
        depth += 1;
        at = models.father(n);
    }
    depth
}

/// The deepest node that is an ancestor of both (a node being its own ancestor); `None` if
/// either is `None` or the two are in different trees
/// [soft 0x08474304 SI_modelCommonAncestor].
///
/// For nodes of different trees the original walks both off their roots together and ends
/// with both pointers null, which is the same answer.
pub fn common_ancestor(models: &Models, a: Option<NodeId>, b: Option<NodeId>) -> Option<NodeId> {
    let (mut a, mut b) = (a?, b?);
    if a == b {
        return Some(a);
    }
    let (da, db) = (model_depth(models, Some(a)), model_depth(models, Some(b)));
    for _ in db..da {
        a = models.father(a)?;
    }
    for _ in da..db {
        b = models.father(b)?;
    }
    let (mut a, mut b) = (Some(a), Some(b));
    while a != b {
        a = models.father(a?);
        b = models.father(b?);
    }
    a
}

/// The first node named `name` in depth-first order from `start` on, the walk going on
/// through brothers and up through ancestors' brothers as [`next_node`] does
/// [soft 0x08472e90 SI_modelNodeFindByName]. Names are compared whole, as C strings.
pub fn node_find_by_name(models: &Models, start: Option<NodeId>, name: &[u8]) -> Option<NodeId> {
    let mut at = start?;
    loop {
        if c_str(&models.data[at].name) == c_str(name) {
            return Some(at);
        }
        if let Some(s) = models.son(at) {
            at = s;
            continue;
        }
        loop {
            if let Some(b) = models.brother(at) {
                at = b;
                break;
            }
            at = models.father(at)?;
        }
    }
}

fn c_str(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

/// True when two trees have the same shape: sons compared first, then brothers
/// [soft 0x0846d678 modelCompareTopology]. Only the links are compared, not kinds or names.
pub fn compare_topology(models: &Models, a: NodeId, b: NodeId) -> bool {
    match (models.son(a), models.son(b)) {
        (None, None) => {}
        (Some(x), Some(y)) => {
            if !compare_topology(models, x, y) {
                return false;
            }
        }
        _ => return false,
    }
    match (models.brother(a), models.brother(b)) {
        (None, None) => true,
        (Some(x), Some(y)) => compare_topology(models, x, y),
        _ => false,
    }
}

// ------------------------------------------------------------------ cutting, deleting, copying

fn unlink_from_father(models: &mut Models, node: NodeId) {
    let Some(f) = models.father(node) else { return };
    let after = models.m(node).brother;
    if models.son(f) == Some(node) {
        models.m_mut(f).son = after;
    } else {
        // the original walks the father's sons until it finds the node and reads through
        // a null pointer if it is not among them; here nothing is unlinked then
        let mut at = models.son(f);
        while let Some(a) = at {
            if models.brother(a) == Some(node) {
                models.m_mut(a).brother = after;
                break;
            }
            at = models.brother(a);
        }
    }
}

/// Takes a node out of its father's list of sons [soft 0x0846d790 SI_modelCutFather]. First
/// every metaball system among the node and its ancestors is told ([`Hooks::metaball_touch`])
/// and envelopes are cut ([`Hooks::envelop_cut`]); then, if the node has a father, the list
/// is closed over it and its `father` and `brother` links are cleared. Its sons stay.
pub fn cut_father(models: &mut Models, node: NodeId, hooks: &mut dyn Hooks) {
    let mut at = Some(node);
    while let Some(n) = at {
        if models.m(n).kind == crate::node::MMETA {
            hooks.metaball_touch(models, n);
        }
        at = models.father(n);
    }
    hooks.envelop_cut(models, node);
    if models.father(node).is_some() {
        unlink_from_father(models, node);
        let m = models.m_mut(node);
        m.father = si_types::Ptr32::NULL;
        m.brother = si_types::Ptr32::NULL;
    }
}

/// Frees a node and everything below it; with `brothers`, first every brother after it
/// [soft 0x08461920 SI_modelDispose]. Order: the following brothers (a call for the next
/// brother, which in turn starts with its own next brother), then the sons with all their
/// brothers, then the node is taken out of its father's list and freed
/// ([`node_copy::dispose_node`]).
///
/// So the nodes are freed deepest and last first: for a node with sons a, b, c the order is
/// c, b, a (each after its own subtree), then the node.
pub fn dispose(models: &mut Models, node: NodeId, brothers: bool, g: &Globals, env: &mut dyn Env) {
    if brothers {
        if let Some(b) = models.brother(node) {
            dispose(models, b, true, g, env);
            // the callee clears the link it was reached through
            models.m_mut(node).brother = si_types::Ptr32::NULL;
        }
    }
    if let Some(s) = models.son(node) {
        dispose(models, s, true, g, env);
        models.m_mut(node).son = si_types::Ptr32::NULL;
    }
    unlink_from_father(models, node);
    node_copy::dispose_node(models, node, g, env);
}

/// Duplicates a tree [soft 0x084622cc SI_modelCopy]: the node
/// ([`node_copy::copy_node`] without its envelope flag), whose `father` becomes `father`;
/// then `SI_envelopCopy(node, father, copy)`; then, only when a father was given, its
/// following brothers; then its sons.
///
/// So the copy of a root never takes the root's brothers along, while below it every
/// brother is copied; the copies are made in the order node, brothers, sons.
pub fn copy(models: &mut Models, node: NodeId, father: Option<NodeId>, g: &Globals, env: &mut dyn Env) -> NodeId {
    let new = node_copy::copy_node(models, node, false, g, env);
    models.m_mut(new).father = father.map_or(si_types::Ptr32::NULL, Models::ptr);
    Hooks::envelop_copy(env, models, node, father, new);
    if father.is_some() {
        if let Some(b) = models.brother(node) {
            let c = copy(models, b, father, g, env);
            models.m_mut(new).brother = Models::ptr(c);
        }
    }
    if let Some(s) = models.son(node) {
        let c = copy(models, s, Some(new), g, env);
        models.m_mut(new).son = Models::ptr(c);
    }
    new
}

// ------------------------------------------------------------------ names

/// Gives a node a new name [soft 0x0846db14 SI_modelChangeName]: bit 0 of both update flag
/// words (+0x0a, +0x3fa) is set, and the name replaced by a copy. (The original skips the
/// replacement when handed the node's own name pointer.)
pub fn change_name(models: &mut Models, node: NodeId, name: &[u8]) {
    let m = models.m_mut(node);
    m.update_flags |= 1;
    m.update_flags2 |= 1;
    models.data[node].name = c_str(name).to_vec();
    models.arena.names[node] = c_str(name).to_vec();
}

/// The two counters `names.c` keeps between calls (`cntRoot`, `cntJnt`, 16 bits each).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChainCounters {
    pub root: i16,
    pub joint: i16,
}

/// Renames one skeleton chain [soft 0x084783c0 SI_skeChainRename]. Only for a chain root: a
/// null (kind 0) whose animation algorithm (+0x2ec) is 1 or 2.
///
/// A number comes from the dictionary's next free name with the prefix `chn`
/// ([`Hooks::new_name_id`]). With `rename_root` the root takes that name and the number is
/// used as it is; without, the root keeps its name and the number minus 1 is used. Then
/// the joints below the root (the chain of first sons of kind 5) become `jnt<n>_<k>`,
/// k from 1, and the node after the last joint becomes `eff<n>`.
///
/// The original follows first sons without testing for the end of the chain: a root without
/// sons, or joints without an effector, sends it through a null pointer. Here the renaming
/// stops there.
pub fn ske_chain_rename(models: &mut Models, node: NodeId, rename_root: bool, counters: &mut ChainCounters, hooks: &mut dyn Hooks) {
    let m = models.m(node);
    if m.kind != crate::node::MNILL || !(1..=2).contains(&m.algo_type) {
        return;
    }
    let (name, id) = hooks.new_name_id(b"chn");
    if rename_root {
        hooks.ed_model_change_name(models, node, &name);
        counters.root = id as i16;
    } else {
        counters.root = (id as i16).wrapping_sub(1);
    }
    counters.joint = 0;
    let mut at = models.son(node);
    while let Some(j) = at {
        if models.m(j).kind != crate::node::MJNT {
            // "%s%1hd": the effector
            let name = format!("eff{}", counters.root);
            hooks.ed_model_change_name(models, j, name.as_bytes());
            return;
        }
        counters.joint = counters.joint.wrapping_add(1);
        // "%s%1hd_%1hd"
        let name = format!("jnt{}_{}", counters.root, counters.joint);
        hooks.ed_model_change_name(models, j, name.as_bytes());
        at = models.son(j);
    }
}

/// Renames the skeleton chains of a node or of a whole tree [soft 0x08478504 SI_modelRename].
///
/// The node itself, if it is a chain root (a null with algorithm 1 or 2), is renamed with
/// [`ske_chain_rename`]. With `recurse` the walk goes on: from a chain root it first skips
/// down the chain of first sons to the chain's effector (the first null met whose algorithm
/// is 3 or 4), then in every case visits the first son and after it the next brother, each
/// with the same rule. Nodes that are not chain roots are not renamed at all: this routine
/// names skeletons, nothing else.
pub fn rename(models: &mut Models, node: NodeId, recurse: bool, rename_root: bool, counters: &mut ChainCounters, hooks: &mut dyn Hooks) {
    let is_chain_root = |models: &Models, n: NodeId| {
        let m = models.m(n);
        m.kind == crate::node::MNILL && (1..=2).contains(&m.algo_type)
    };
    if is_chain_root(models, node) {
        ske_chain_rename(models, node, rename_root, counters, hooks);
    }
    if !recurse {
        return;
    }
    let mut at = node;
    if is_chain_root(models, at) {
        let mut algo = models.m(at).algo_type;
        // (the original runs off a chain that has no effector; here the walk ends there)
        'chain: while !(3..=4).contains(&algo) {
            loop {
                match models.son(at) {
                    None => break 'chain,
                    Some(s) => at = s,
                }
                if models.m(at).kind == crate::node::MNILL {
                    break;
                }
            }
            algo = models.m(at).algo_type;
        }
    }
    if let Some(s) = models.son(at) {
        rename(models, s, recurse, rename_root, counters, hooks);
    }
    if let Some(b) = models.brother(at) {
        rename(models, b, recurse, rename_root, counters, hooks);
    }
}

// ------------------------------------------------------------------ definitions

pub(crate) fn current_is_stored(models: &Models, node: NodeId) -> bool {
    models.data[node].current == Current::Def(models.m(node).cur_def as usize)
}

/// The number of geometries a node holds [soft 0x08466e2c SI_modelDefCount]: its stored
/// definitions, plus 1 if it works on a definition of its own (the `definition` pointer is
/// not the stored definition number `cur_def`), plus 1 if it has an original definition
/// (`org_def`). 0 for no node.
///
/// (The original reads the array at `cur_def` whatever `nb_defs` is: for a node that never
/// had a definition that is a null pointer and a fault. Here such a node answers 0, or 1
/// with a working definition.)
pub fn def_count(models: &Models, node: Option<NodeId>) -> u32 {
    let Some(node) = node else { return 0 };
    let mut n = models.m(node).nb_defs as u32;
    if !current_is_stored(models, node) && !(n == 0 && models.data[node].current == Current::None) {
        n += 1;
    }
    if models.data[node].org_def.is_some() {
        n += 1;
    }
    n
}

/// Which of a node's geometries [`next_def_get`] and [`next_def_set`] mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefSlot {
    Stored(usize),
    Current,
    Original,
}

/// Steps through a node's geometries [soft 0x08466e64 SI_modelNextDefGet]: the stored
/// definitions in order, then the working definition if it is one of its own, then the
/// original definition. `cursor` starts at 0 and is advanced; `None` ends the walk (and
/// is all a node without stored definitions ever answers).
///
/// A walk can end early with `Some` followed by `None`: after the stored definitions the
/// cursor moves on by one whether or not there is a working definition, and the original
/// definition is returned at that step even when it is null.
pub fn next_def_get(models: &Models, node: NodeId, cursor: &mut i32) -> Option<DefSlot> {
    let n = models.m(node).nb_defs as i32;
    if n == 0 || *cursor < 0 || *cursor > n + 1 {
        return None;
    }
    if *cursor < n {
        let slot = DefSlot::Stored(*cursor as usize);
        *cursor += 1;
        return Some(slot);
    }
    if *cursor == n {
        *cursor = n + 1;
        if !current_is_stored(models, node) {
            return match models.data[node].current {
                Current::None => None,
                _ => Some(DefSlot::Current),
            };
        }
    }
    *cursor += 1;
    models.data[node].org_def.as_ref().map(|_| DefSlot::Original)
}

/// The geometry a slot designates.
pub fn definition(models: &Models, node: NodeId, slot: DefSlot) -> Option<&Definition> {
    let d = &models.data[node];
    match slot {
        DefSlot::Stored(i) => d.defs.get(i),
        DefSlot::Current => match &d.current {
            Current::None => None,
            Current::Def(i) => d.defs.get(*i),
            Current::Own(b) => Some(b),
        },
        DefSlot::Original => d.org_def.as_deref(),
    }
}

/// One of a node's geometries by role [soft 0x08473fc4 SI_modelGetData]:
///
/// - `which` 0, the shape at rest: the original definition if there is one, else the first
///   stored definition;
/// - `which` 1: stored definition number `index`, **for `index` from 1 up only** (0 and
///   anything out of range answer nothing);
/// - `which` 2: the working definition;
/// - any other value: nothing.
pub fn get_data(models: &Models, node: NodeId, which: i32, index: i32) -> Option<DefSlot> {
    let d = &models.data[node];
    match which {
        0 if d.org_def.is_some() => Some(DefSlot::Original),
        // (the original tests the array pointer, not the count)
        0 => (!d.defs.is_empty()).then_some(DefSlot::Stored(0)),
        1 if index > 0 && index < models.m(node).nb_defs as i32 => Some(DefSlot::Stored(index as usize)),
        2 if d.current != Current::None => Some(DefSlot::Current),
        _ => None,
    }
}

/// Frees one of a node's geometries through the disposal routine of the given kind
/// [soft 0x08466ebc modelDefDispose]: patch, face, mesh, spline, NURBS curve and NURBS
/// surface; for any other kind (a metaball system among them) nothing happens.
pub fn model_def_dispose(models: &mut Models, node: NodeId, kind: i32, slot: DefSlot, env: &mut dyn Env) {
    if matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        node_copy::drop_def(models, node, kind, slot, env);
    }
}

/// Replaces the geometry the cursor is at and advances it
/// [soft 0x08466f30 SI_modelNextDefSet]. True while there is a further geometry to replace.
///
/// A stored definition is freed and replaced; if the node was working on that very
/// definition, or on none, it now works on the new one. After the stored ones comes the
/// working definition, if it is one of its own, then the original definition (which is
/// replaced only if there is one; the cursor is not advanced past it).
pub fn next_def_set(models: &mut Models, node: NodeId, cursor: &mut i32, new: Definition, env: &mut dyn Env) -> bool {
    let n = models.m(node).nb_defs as i32;
    if n == 0 || *cursor < 0 || *cursor > n + 1 {
        return false;
    }
    let kind = models.m(node).kind;
    if *cursor < n {
        let i = *cursor as usize;
        // the working pointer is cleared first if it is this definition, and afterwards a
        // null working pointer takes the new one
        let was_this = models.data[node].current == Current::Def(i);
        if was_this {
            models.data[node].current = Current::None;
        }
        model_def_dispose(models, node, kind, DefSlot::Stored(i), env);
        // (the kinds modelDefDispose does not free are simply overwritten: a leak there)
        models.data[node].defs[i] = new;
        if models.data[node].current == Current::None {
            models.data[node].current = Current::Def(i);
        }
        *cursor += 1;
        return !(*cursor >= n && current_is_stored(models, node) && !has_org_def(models, node));
    }
    if *cursor == n && !current_is_stored(models, node) {
        // (a node working on another stored definition has that one freed here, and its
        // array entry left dangling, in the original; here the stored one stays)
        model_def_dispose(models, node, kind, DefSlot::Current, env);
        models.data[node].current = Current::Own(Box::new(new));
        *cursor += 1;
        return has_org_def(models, node);
    }
    if has_org_def(models, node) {
        model_def_dispose(models, node, kind, DefSlot::Original, env);
        models.data[node].org_def = Some(Box::new(new));
    }
    false
}

fn has_org_def(models: &Models, node: NodeId) -> bool {
    models.data[node].org_def.is_some()
}

/// Appends a stored definition [soft 0x08467038 SI_modelAddDefinition]. The first one sets
/// the node's kind. If the node was working on its first stored definition, or on none, it
/// works on the first stored definition afterwards. The name is "changed" to itself, which
/// sets bit 0 of the two update flag words. Always true.
pub fn add_definition(models: &mut Models, node: NodeId, kind: i32, definition: Definition) -> bool {
    let first = models.m(node).nb_defs == 0;
    if first {
        models.m_mut(node).kind = kind;
    }
    let was_first = match models.data[node].current {
        Current::None => first,
        Current::Def(0) => !first,
        _ => false,
    };
    models.data[node].defs.push(definition);
    models.m_mut(node).nb_defs = models.m(node).nb_defs.wrapping_add(1);
    if was_first {
        models.data[node].current = Current::Def(0);
    }
    let name = models.data[node].name.clone();
    change_name(models, node, &name);
    true
}

/// A node of one definition [soft 0x0846d5b4 SI_modelGetNode]: allocated with `name` (or
/// the default name) and given the definition.
pub fn get_node(models: &mut Models, definition: Definition, kind: i32, name: Option<&[u8]>) -> NodeId {
    let id = models.allocate(name);
    add_definition(models, id, kind, definition);
    id
}

/// Gives a node a working definition of its own
/// [soft 0x084670bc SI_modelCheckCreateDefinition]: if it has stored definitions and works
/// on the one numbered `cur_def`, a copy of its **first** stored definition becomes the
/// working definition. Kinds: patch, face, mesh, spline, NURBS curve and surface; a node of
/// any other kind is left alone.
///
/// (The copy is of definition 0, not of `cur_def`: the same thing only for a node working
/// on its first shape. The copy is made by the kind's own routine,
/// [`node_copy::Hooks::copy_def`].)
pub fn check_create_definition(models: &mut Models, node: NodeId, env: &mut dyn Env) {
    if models.m(node).nb_defs == 0 || !current_is_stored(models, node) {
        return;
    }
    let kind = models.m(node).kind;
    if !matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        return;
    }
    let Some(first) = models.data[node].defs.first() else { return };
    let copy = env.copy_def(kind, first, None);
    models.data[node].current = Current::Own(Box::new(copy));
}

// ------------------------------------------------------------------ counting

/// The number of triangles of a node's current stored definition
/// [soft 0x0846cd80 SI_modelNbTriangles]: for a mesh [`obj_global::mesh_nb_triangles`]; for
/// a patch, a face, a NURBS surface and a metaball system the count of their own routines
/// ([`Hooks::nb_triangles_of`]); 0 for every other kind.
pub fn nb_triangles(models: &Models, node: NodeId, hooks: &mut dyn Hooks) -> i32 {
    let m = models.m(node);
    let def = models.data[node].defs.get(m.cur_def as usize);
    match (m.kind, def) {
        (4, Some(Definition::Mesh(mesh))) => obj_global::mesh_nb_triangles(mesh),
        (1 | 2 | 10, Some(d)) => hooks.nb_triangles_of(d),
        (7, _) => match definition(models, node, DefSlot::Current) {
            Some(d) => hooks.nb_triangles_of(d),
            None => 0,
        },
        _ => 0,
    }
}

/// [soft 0x08477c30 nbTrianglesBranch]: the triangles of `node`, of everything below it,
/// and, unless `node` is `top`, of its following brothers with everything below them.
/// The sons are counted first, then the brothers, then the node.
pub fn nb_triangles_branch_from(models: &Models, node: NodeId, top: NodeId, hooks: &mut dyn Hooks) -> i32 {
    let mut total = 0i32;
    if let Some(s) = models.son(node) {
        total = nb_triangles_branch_from(models, s, top, hooks);
    }
    if node != top {
        if let Some(b) = models.brother(node) {
            total = total.wrapping_add(nb_triangles_branch_from(models, b, top, hooks));
        }
    }
    nb_triangles(models, node, hooks).wrapping_add(total)
}

/// The triangles of a node and everything below it
/// [soft 0x0846ce20 SI_modelNbTrianglesBranch].
pub fn nb_triangles_branch(models: &Models, node: NodeId, hooks: &mut dyn Hooks) -> i32 {
    nb_triangles_branch_from(models, node, node, hooks)
}

#[cfg(test)]
mod tests;
