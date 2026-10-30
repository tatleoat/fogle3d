// SPDX-License-Identifier: MIT
//! Model nodes rebuilt from a vector's input buffers, and the log of what a port calls
//! outside the crate, in the notation of the emulation harness.
//!
//! `re/tools/si-model/emu_world.py` lays a node out in emulator memory as the record `m<k>`
//! plus the buffers its pointers lead to. [`load`] follows the same pointer words in the
//! vector's inputs and builds node k of a [`Models`] from them. A pointer becomes a
//! *handle*: a number that stands for "this buffer, at this offset" ([`Space`]), kept in the
//! record where the original keeps the pointer, so that the port passes it to its hooks as
//! the original passes the pointer to the routine behind the hook.
//!
//! [`Trace`] implements every hook group: each call is written down as
//! `name(argument,…)` with pointers as `&buffer+0xoffset`, which is how [`expected`] prints
//! the calls the emulator recorded. A port is right about the outside world when the two
//! lists are equal.

use std::collections::HashMap;

use si_model::hierarchy::DefSlot;
use si_model::node::{Current, Definition, Models};
use si_model::node_copy::{LatticeRef, SubelUserData, TextureRef, UpdNode, UserNode};
use si_model::{hierarchy, node_copy, transform, NodeId};
use si_types::model::Model;
use si_types::scene::{LclMap, Material, Texture3D};
use si_types::Ptr32;
use si_x87::vectors::{Case, Field, Verdict, Word};

pub const WORDS: usize = 0x430 / 4;

/// `EMUCHECK_MUTATE=1`: one bit of everything the port produces is flipped before it is
/// compared (every call's text, every record word), to show that the comparisons are live:
/// every case that compares anything must then differ.
pub fn mutate() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("EMUCHECK_MUTATE").is_some())
}

/// Buffer names against handles. Node k's record is the handle k + 1, which is also the
/// crate's pointer to node k; a field of it at `off` is 0x8000_0000 | k << 16 | off; any
/// other buffer is (its number + 1) << 16 | offset.
#[derive(Default)]
pub struct Space {
    names: Vec<String>,
    /// Names the emulator used for blocks that are nodes here (`heap0` → `m7`).
    pub alias: HashMap<String, String>,
}

fn node_number(name: &str) -> Option<u32> {
    name.strip_prefix('m').and_then(|k| k.parse().ok())
}

impl Space {
    pub fn handle(&mut self, name: &str, off: u32) -> u32 {
        if let Some(k) = node_number(name) {
            return if off == 0 { k + 1 } else { 0x8000_0000 | k << 16 | off };
        }
        let i = match self.names.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                self.names.push(name.to_string());
                self.names.len() - 1
            }
        };
        (i as u32 + 1) << 16 | off
    }

    pub fn of(&mut self, w: &Word) -> u32 {
        match w {
            Word::Value { value, .. } => *value as u32,
            Word::Ref { name, offset } => {
                let name = self.alias.get(name).cloned().unwrap_or_else(|| name.clone());
                self.handle(&name, *offset)
            }
            Word::Unwritten { .. } => 0,
        }
    }

    pub fn show(&self, h: u32) -> String {
        if h == 0 {
            "0".to_string()
        } else if h < 0x10000 {
            format!("&m{}+0x0", h - 1)
        } else if h & 0x8000_0000 != 0 {
            format!("&m{}+{:#x}", (h >> 16) & 0x7fff, h & 0xffff)
        } else {
            match self.names.get((h >> 16) as usize - 1) {
                Some(n) => format!("&{n}+{:#x}", h & 0xffff),
                None => format!("?{h:#x}"),
            }
        }
    }

    /// The address of a field of node `node`.
    pub fn field(&self, node: NodeId, off: u32) -> String {
        format!("&m{node}+{off:#x}")
    }

    fn word(&self, w: &Word) -> String {
        match w {
            Word::Value { value, .. } => format!("{value:x}"),
            Word::Ref { name, offset } => format!("&{}+{offset:#x}", self.alias.get(name).unwrap_or(name)),
            Word::Unwritten { .. } => "-".to_string(),
        }
    }
}

/// A stand-in for a definition the routine under test never looks into: told apart by the
/// handle of the buffer it came from.
pub fn tagged(handle: u32) -> Definition {
    Definition::Spline(si_hrc::Spline { kind: handle as i32, ..si_hrc::Spline::default() })
}

pub fn tag_of(d: &Definition) -> u32 {
    match d {
        Definition::Spline(s) => s.kind as u32,
        _ => 0,
    }
}

fn chain(c: &Case, space: &mut Space, mut at: Word, next: usize) -> Vec<(u32, Vec<Word>)> {
    let mut out = Vec::new();
    while let Word::Ref { name, offset: 0 } = &at {
        let words = c.input(name).words().to_vec();
        out.push((space.handle(name, 0), words.clone()));
        at = words[next].clone();
        if out.len() > 64 {
            break;
        }
    }
    out
}

/// A chain of 2D textures: each one's handle and its rotation angles (+0x94).
fn textures(c: &Case, space: &mut Space, at: Word) -> Vec<TextureRef> {
    chain(c, space, at, 91)
        .into_iter()
        .map(|(h, w)| {
            let f = |i: usize| f32::from_bits(w[i].value().unwrap_or(0) as u32);
            TextureRef { ptr: Ptr32::new(h), rot: si_types::geom::Vector { x: f(37), y: f(38), z: f(39) } }
        })
        .collect()
}

pub fn user_chain(c: &Case, space: &mut Space, at: Word) -> Vec<UserNode> {
    chain(c, space, at, 4).into_iter().map(|(ptr, w)| UserNode { ptr, block_0: space.of(&w[0]), block_c: space.of(&w[3]) }).collect()
}

fn i16_of(w: &Word) -> i16 {
    w.value().unwrap_or(0) as i16
}

/// User data per vertex, polygon or edge from the record's two pointers.
pub fn subel(c: &Case, space: &mut Space, cluster: &Word, array: &Word) -> SubelUserData {
    let mut s = SubelUserData::default();
    if let Word::Ref { name, .. } = cluster {
        let k = c.input(name).words().to_vec();
        s.block_0 = space.of(&k[0]);
        s.count = k[1].value().unwrap_or(0) as i32;
        s.block_c = space.of(&k[3]);
    }
    if let Word::Ref { name, .. } = array {
        let a = c.input(name).words().to_vec();
        for i in 0..(s.count.max(0) as usize).min(a.len()) {
            s.chains.push(user_chain(c, space, a[i].clone()));
        }
    }
    s
}

/// The record of a buffer, pointers as handles.
pub fn record(space: &mut Space, f: &Field) -> Model {
    let w: Vec<u32> = f.words().iter().map(|w| space.of(w)).collect();
    assert_eq!(w.len(), WORDS);
    // every field of the record is plain data: any bit pattern is a value
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const Model) }
}

/// Nodes 0 to `count` − 1 from the buffers `m0`, `m1`, … of a case and what they point to.
pub fn load(c: &Case, count: usize) -> (Models, Space) {
    let mut models = Models::default();
    let mut space = Space::default();
    for k in 0..count {
        let id = models.allocate(None);
        let f = c.input(&format!("m{k}"));
        let w = f.words().to_vec();
        let rec = record(&mut space, f);
        *models.m_mut(id) = rec;
        let at = |off: usize| w[off / 4].clone();
        let d = &mut models.data[id];
        d.name = match at(4) {
            Word::Ref { name, .. } => {
                let b = c.input(&name).to_bytes();
                b[..b.iter().position(|&x| x == 0).unwrap_or(b.len())].to_vec()
            }
            _ => Vec::new(),
        };
        models.arena.names[id] = d.name.clone();
        if let Word::Ref { name, .. } = at(0x38) {
            let a = c.input(&name).words().to_vec();
            for i in 0..rec.nb_defs as usize {
                let h = a.get(i).map_or(0, |w| space.of(w));
                d.defs.push(tagged(h));
            }
        }
        let cur = space.of(&at(0x48));
        d.current = if cur == 0 {
            Current::None
        } else {
            match d.defs.iter().position(|x| tag_of(x) == cur) {
                Some(i) => Current::Def(i),
                None => Current::Own(Box::new(tagged(cur))),
            }
        };
        let org = space.of(&at(0x3c));
        d.org_def = (org != 0).then(|| Box::new(tagged(org)));
        if let Word::Ref { name, .. } = at(0x20c) {
            let entries = c.input(&name).words().to_vec();
            d.cx.materials = entries.iter().map(|w| Ptr32::new(space.of(w))).collect();
            for e in &entries {
                // a material laid out up to its first local 2D texture (+0x6ec)
                let local = match e {
                    Word::Ref { name, .. } if c.input(name).len() > 443 => c.input(name).words()[443].clone(),
                    _ => Word::Value { bytes: 4, value: 0 },
                };
                d.cx.material_textures.push(textures(c, &mut space, local));
            }
        }
        d.cx.textures = textures(c, &mut space, at(0x21c));
        for (set, off) in [(0, 0x50), (1, 0x58)] {
            if let Word::Ref { name, .. } = at(off) {
                for e in c.input(&name).words().to_vec() {
                    if let Word::Ref { name: l, .. } = &e {
                        let active = i16_of(&c.input(l).words()[0]);
                        d.cx.lattices[set].push(LatticeRef { ptr: Ptr32::new(space.of(&e)), active });
                    }
                }
            }
        }
        for i in 0..2 {
            if let Word::Ref { name, .. } = at(0x60 + 4 * i) {
                d.cx.spline_fit_active[i] = i16_of(&c.input(&name).words()[0]);
            }
            if let Word::Ref { name, .. } = at(0x2d8 + 4 * i) {
                d.cx.patch_fit_active[i] = i16_of(&c.input(&name).words()[0]);
            }
        }
        d.cx.waves = chain(c, &mut space, at(0x2e8), 2).into_iter().map(|(_, w)| w[1].value().unwrap_or(0) as i32).collect();
        d.cx.user_data = user_chain(c, &mut space, at(0x34c));
        for j in 0..3 {
            d.cx.subel[j] = subel(c, &mut space, &at(0x408 + 8 * j), &at(0x40c + 8 * j));
        }
        if let Word::Ref { name, .. } = at(0x428) {
            let a = c.input(&name).words().to_vec();
            d.cx.uv_domains = a.chunks(2).map(|p| space.of(&p[0])).collect();
        }
        if let Word::Ref { name, .. } = at(0x328) {
            let s = c.input(&name).f32s();
            let v = |i: usize| si_types::geom::Vector { x: s[i], y: s[i + 1], z: s[i + 2] };
            d.init_srt = Some([v(0), v(3), v(6)]);
        }
        if let Word::Ref { name, .. } = at(0x318) {
            let w = c.input(&name).words();
            d.tx.inv_kin_kind = Some(w[0].value().unwrap_or(0) as i32);
            if w.len() >= 15 {
                let g = |i: usize| f32::from_bits(w[i].value().unwrap_or(0) as u32);
                d.tx.inv_kin_goal = [g(2), g(3), g(4)];
                d.tx.inv_kin_state = w[14].value().unwrap_or(0) as i32;
            }
        }
    }
    (models, space)
}

/// Routines the generator stubs that the crate calls directly, not through a hook: the
/// display lines of a copied mesh, and a definition through a matrix where the case's
/// definitions are opaque blocks (`emu_gen2.py`, `session`).
const ALWAYS_SKIPPED: [&str; 2] = ["SI_meshOptimizeDisplay", "SI_modelMultiplyDef"];

/// The calls the emulator recorded, as text, with each call's result; `skip` names are
/// left out.
pub fn expected(c: &Case, space: &Space, skip: &[&str]) -> Vec<(String, u32)> {
    c.calls
        .iter()
        .filter(|k| !skip.contains(&k.name.as_str()) && !ALWAYS_SKIPPED.contains(&k.name.as_str()) && k.result.bytes() != 10)
        .map(|k| {
            let args: Vec<String> = k
                .args
                .iter()
                .map(|w| match w {
                    // a string argument: its text
                    Word::Ref { name, .. } if name.starts_with("str") && c.try_output(name).is_some() => String::from_utf8_lossy(&c.output(name).to_bytes()).into_owned(),
                    w => space.word(w),
                })
                .collect();
            (format!("{}({})", k.name, args.join(",")), k.result.value().unwrap_or(0) as u32)
        })
        .collect()
}

/// Every hook group, writing down each call and answering as the emulator's stub did.
pub struct Trace {
    pub space: Space,
    pub log: Vec<String>,
    pub expected: Vec<(String, u32)>,
    /// `free` was recorded in this vector file.
    pub frees: bool,
    /// Calls left out of both lists (the names given to [`Trace::new`]).
    skip: Vec<String>,
    /// What `graf_selection` answers when the case has one graf node for the node asked.
    pub selection: Option<i32>,
    /// The case's graf list, when it has one: (kind, object handle, selection state).
    pub graf: Option<Vec<(i32, u32, i32)>>,
    /// The update node of each node's element (`emu_world.py`, group "anim").
    pub upd: HashMap<NodeId, UpdNode>,
    fresh: u32,
}

impl Trace {
    pub fn new(c: &Case, space: Space, skip: &[&str]) -> Trace {
        let expected = expected(c, &space, skip);
        let frees = c.calls.iter().any(|k| k.name == "free");
        Trace { space, log: Vec::new(), expected, frees, skip: skip.iter().map(|n| format!("{n}(")).collect(), selection: None, graf: None, upd: HashMap::new(), fresh: 0 }
    }

    /// Writes a call down; the result of the emulator's call at the same place if it is the
    /// same call, else 0.
    pub fn say(&mut self, text: String) -> u32 {
        if self.skip.iter().any(|n| text.starts_with(n.as_str())) {
            return 0;
        }
        let r = self.expected.get(self.log.len()).filter(|e| e.0 == text).map_or(0, |e| e.1);
        self.log.push(if mutate() { text + "'" } else { text });
        r
    }

    fn node(&self, n: NodeId) -> String {
        format!("&m{n}+0x0")
    }

    fn opt(&self, n: Option<NodeId>) -> String {
        n.map_or("0".to_string(), |n| self.node(n))
    }

    /// Same calls, same order?
    pub fn verdict(&self) -> Verdict {
        let want: Vec<&String> = self.expected.iter().map(|e| &e.0).collect();
        let got: Vec<&String> = self.log.iter().collect();
        if want == got {
            return Verdict::Same;
        }
        let i = want.iter().zip(&got).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
        Verdict::Different(format!(
            "call {i} of {} (the port makes {}): the original {:?}, the port {:?}; before it {:?}",
            want.len(),
            got.len(),
            want.get(i),
            got.get(i),
            &got[i.saturating_sub(3)..i]
        ))
    }
}

fn dispose_name(kind: i32) -> &'static str {
    match kind {
        1 => "SI_patchDispose",
        2 => "SI_faceDispose",
        4 => "SI_meshDispose",
        6 => "SI_splineDispose",
        7 => "SI_metaSystemDispose",
        8 => "SI_metaballDispose",
        9 => "SI_nurbsCrvDispose",
        _ => "SI_nurbsSrfDispose",
    }
}

fn copy_name(kind: i32) -> &'static str {
    match kind {
        1 => "SI_patchCopy",
        2 => "SI_faceCopy",
        4 => "SI_meshCopy",
        6 => "SI_splineCopy",
        7 => "si_metaSystemCopy",
        8 => "SI_metaballCopy",
        9 => "SI_nurbsCrvCopy",
        _ => "SI_nurbsSrfCopy",
    }
}

impl node_copy::Hooks for Trace {
    fn model_reset_all(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_modelResetAll({})", self.node(node)));
    }
    fn material_still_used(&mut self, m: Ptr32<Material>) -> bool {
        self.say(format!("SI_materialStillUsed({})", self.space.show(m.0))) != 0
    }
    fn ed_dispose_material(&mut self, m: Ptr32<Material>) {
        self.say(format!("SI_edDisposeMaterial({},1)", self.space.show(m.0)));
    }
    fn ed_dispose_texture2d(&mut self, t: Ptr32<LclMap>) {
        self.say(format!("SI_edDisposeTexture2D({})", self.space.show(t.0)));
    }
    fn texture3d_still_used(&mut self, t: Ptr32<Texture3D>) -> bool {
        self.say(format!("SI_texture3DStillUsed({})", self.space.show(t.0))) != 0
    }
    fn ed_dispose_texture3d(&mut self, t: Ptr32<Texture3D>) {
        self.say(format!("SI_edDisposeTexture3D({})", self.space.show(t.0)));
    }
    fn fitting_dispose(&mut self, _: &mut Models, node: NodeId, which: usize) {
        self.say(format!("SI_fittingDispose({},{},{which})", self.node(node), self.space.field(node, 0x60 + 4 * which as u32)));
    }
    fn pfitting_dispose(&mut self, _: &mut Models, node: NodeId, which: usize) {
        self.say(format!("SI_pfittingDispose({},{},{which})", self.node(node), self.space.field(node, 0x2d8 + 4 * which as u32)));
    }
    fn unlink_all_waves(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_modelUnlinkAllWaves({})", self.node(node)));
    }
    fn delete_lattice_animation(&mut self, _: &mut Models, node: NodeId, set: usize) {
        self.say(format!("SI_modelDeleteLatticeAnimation({},{set})", self.node(node)));
    }
    fn lattice_dispose(&mut self, models: &mut Models, node: NodeId, set: usize, index: usize) {
        let array = if set == 0 { models.m(node).lattices0.0 } else { models.m(node).lattices1.0 };
        self.say(format!("SI_latticeDispose({})", self.space.show(array + 4 * index as u32)));
    }
    fn dyna_info_dispose(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_modelDynaInfoDispose({},1)", self.node(node)));
    }
    fn inv_kin_dispose(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_invKinDispose({})", self.space.field(node, 0x318)));
    }
    fn envelop_info_dispose(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_envelopInfoDispose({})", self.space.field(node, 0x324)));
    }
    fn kluster_dispose_all(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_klusterDisposeAll({})", self.node(node)));
    }
    fn cluster_dispose(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_clusterDispose({})", self.space.field(node, 0x344)));
    }
    fn classic_comp_info_dispose(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_classicMdlCompInfoDispose({})", self.space.field(node, 0x3e8)));
    }
    fn free(&mut self, block: u32) {
        if self.frees {
            self.say(format!("free({})", self.space.show(block)));
        }
    }
    fn dispose_def(&mut self, models: &mut Models, node: NodeId, kind: i32, slot: DefSlot) {
        let at = match slot {
            DefSlot::Stored(i) => self.space.show(models.m(node).defs.0 + 4 * i as u32),
            DefSlot::Current => self.space.field(node, 0x48),
            DefSlot::Original => self.space.field(node, 0x3c),
        };
        self.say(format!("{}({at})", dispose_name(kind)));
    }
    fn patch_remove_fitting(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_patchRemoveFitting({})", self.node(node)));
    }
    fn copy_def(&mut self, kind: i32, definition: &Definition, to: Option<NodeId>) -> Definition {
        if let Definition::Mesh(m) = definition {
            // a real mesh: the vector was made with the binary's own SI_meshCopy
            return Definition::Mesh(si_model::obj_global::mesh_copy(m));
        }
        // the stand-in logs the block it hands back as a last argument: the copy is known
        // by that block's name from then on
        let from = self.space.show(tag_of(definition));
        let head = match to {
            Some(n) => format!("{}({from},{}", copy_name(kind), self.node(n)),
            None => format!("{}({from}", copy_name(kind)),
        };
        let known = self.expected.get(self.log.len()).filter(|e| e.0.starts_with(&head) && e.0[head.len()..].starts_with(",&")).map(|e| e.0.clone());
        let (text, h) = match known {
            Some(e) => {
                let name = e[head.len() + 2..].split('+').next().unwrap_or("").to_string();
                let h = self.space.handle(&name, 0);
                (e, h)
            }
            None => {
                self.fresh += 1;
                let h = self.space.handle(&format!("copy{}", self.fresh), 0);
                (format!("{head},?)"), h)
            }
        };
        self.say(text);
        tagged(h)
    }
    fn classic_comp_info_copy(&mut self, _: &mut Models, source: NodeId, copy: NodeId) {
        self.say(format!("SI_classicModelCompInfoCopy({},{})", self.node(source), self.node(copy)));
    }
    fn graf_selection(&mut self, _: &Models, node: NodeId, graf_type: i32) -> Option<i32> {
        // SI_findGlData: the first graf node of that object whose kind fits; kinds 2 and 3
        // (the root of a model and its other nodes) count as one
        let Some(list) = &self.graf else { return self.selection };
        let fold = |k: i32| if k == 3 { 2 } else { k };
        list.iter().find(|g| (graf_type == 0 || fold(graf_type) == fold(g.0)) && g.1 == Models::ptr(node).0).map(|g| g.2)
    }
    fn transfer_points(&mut self, models: &mut Models, node: NodeId, kind: i32) {
        let name = match kind {
            1 => "SI_patchTransferPoints",
            2 => "SI_faceTransferPoints",
            4 => "SI_meshTransferVertices",
            6 => "SI_splineTransferPoints",
            9 => "SI_nurbsCrvTransferPointsKnots",
            _ => "SI_nurbsSrfTransferPointsKnots",
        };
        let first = models.data[node].defs.first().map_or(0, tag_of);
        self.say(format!("{name}({},{})", self.space.show(first), self.space.show(cur_tag(models, node))));
    }
    fn wave_transform(&mut self, _: &mut Models, wave: u32, node: NodeId, branch: bool, time: f32) {
        self.say(format!("SI_waveTransform({},{},{},{:x},0)", self.space.show(wave), self.node(node), branch as u8, time.to_bits()));
    }
    fn lattice_transform(&mut self, models: &mut Models, lattice: Ptr32<si_types::model::Lattice>, node: NodeId) {
        let d = self.space.show(cur_tag(models, node));
        self.say(format!("SI_latticeTransform({},{:x},{d},{d})", self.space.show(lattice.0), models.m(node).kind));
    }
    fn spline_fit(&mut self, models: &mut Models, fitting: u32, node: NodeId) {
        let d = self.space.show(cur_tag(models, node));
        self.say(format!("SI_splineFit({},{:x},{d},{d})", self.space.show(fitting), models.m(node).kind));
    }
    fn patch_fit(&mut self, models: &mut Models, fitting: u32, node: NodeId) {
        let d = self.space.show(cur_tag(models, node));
        self.say(format!("SI_patchFit({},{:x},{d},{d})", self.space.show(fitting), models.m(node).kind));
    }
    fn find_related_elem(&mut self, element: u32, relation: i32) -> bool {
        self.say(format!("SI_elemdictFindRelatedElem({},{relation:x})", self.space.show(element))) != 0
    }
    fn ed_find_update_node(&mut self, _: &Models, node: NodeId, relation: i32, cursor: Option<&mut i32>) -> Option<UpdNode> {
        let found = self.say(format!("SI_edFindUpdateNode({},{relation:x})", self.node(node))) != 0;
        if let (true, Some(c)) = (found, cursor) {
            *c += 1;
        }
        found.then(|| self.upd.get(&node).copied().unwrap_or_default())
    }
    fn check_active_time(&mut self, update: &UpdNode, time: f32) -> Option<f32> {
        // the stand-in always hands back 1.5
        (self.say(format!("SI_checkActiveTime({},{:x})", self.space.show(update.ptr), time.to_bits())) != 0).then_some(1.5)
    }
    fn anim_quick_stretch_update(&mut self, _: &mut Models, update: &UpdNode, value: f32) {
        self.say(format!("SI_animQuickStretchUpdate({},{:x})", self.space.show(update.ptr), value.to_bits()));
    }
    fn anim_mdl_cluster_cns_update(&mut self, _: &mut Models, update: &UpdNode, which: i16, time: f32) {
        self.say(format!("SI_animMdlClusterCnsUpdate({},0,{which},0,{:x})", self.space.show(update.ptr), time.to_bits()));
    }
    fn envelop_active(&mut self, _: &Models, node: NodeId) -> bool {
        self.say(format!("SI_envelopActive({})", self.node(node))) != 0
    }
    fn dyna_info_copy(&mut self, _: &mut Models, source: NodeId, copy: NodeId) {
        self.say(format!("SI_modelDynaInfoCopy({},{})", self.node(source), self.node(copy)));
    }
    fn inv_kin_copy(&mut self, models: &mut Models, source: NodeId, _: NodeId) -> u32 {
        let ik = models.m(source).inv_kin.0;
        self.say(format!("SI_invKinCopy({})", self.space.show(ik)))
    }
    fn remove_triangle(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_removeTriangle({},0)", self.node(node)));
    }
    fn touch_def(&mut self, models: &mut Models, node: NodeId, kind: i32) {
        let def = cur_tag(models, node);
        match kind {
            6 => self.say(format!("SI_splineTouch({})", self.space.show(def))),
            9 => self.say(format!("SI_nurbsCrvAnimUpdate({})", self.space.show(def))),
            _ => 0,
        };
    }
}

impl hierarchy::Hooks for Trace {
    fn metaball_touch(&mut self, models: &mut Models, system: NodeId) {
        let def = models.m(system).definition.0;
        self.say(format!("SI_metaballTouch(0,{})", self.space.show(def)));
    }
    fn envelop_cut(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_envelopCut({})", self.node(node)));
    }
    fn envelop_copy(&mut self, _: &mut Models, source: NodeId, father: Option<NodeId>, copy: NodeId) {
        self.say(format!("SI_envelopCopy({},{},{})", self.node(source), self.opt(father), self.node(copy)));
    }
    fn new_name_id(&mut self, prefix: &[u8]) -> (Vec<u8>, i32) {
        // the emulator's stand-in answers "<prefix><n>" and n, n being 3 plus the number of
        // calls recorded so far
        let n = self.log.len() as i32 + 3;
        self.say(format!("SI_elemdictGetNewNameId({})", String::from_utf8_lossy(prefix)));
        (format!("{}{n}", String::from_utf8_lossy(prefix)).into_bytes(), n)
    }
    fn ed_model_change_name(&mut self, _: &mut Models, node: NodeId, name: &[u8]) {
        self.say(format!("SI_edModelChangeName({},{})", self.node(node), String::from_utf8_lossy(name)));
    }
}

impl transform::Hooks for Trace {
    fn metaball_moved(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("metaball_moved({})", self.node(node)));
    }
    fn compensate_local_coord(&mut self, _: &mut Models, father: NodeId, node: NodeId) {
        self.say(format!("SI_modelCompensateLocalCoord({},{})", self.node(father), self.node(node)));
    }
    fn compensate_angle(&mut self, _: &mut Models, _: &si_math::types::Matrix, son: NodeId) {
        self.say(format!("SI_modelCompensateAngle({})", self.node(son)));
    }
    fn clusters_apply_transformation(&mut self, _: &mut Models, node: NodeId, _: &si_math::types::Matrix) {
        self.say(format!("SI_clustersApplyTransformation({})", self.node(node)));
    }
    fn inv_kin_goal_invalidate(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_invKinGoalInvalidate({})", self.node(node)));
    }
    fn inv_kin_reset_pref_angle(&mut self, _: &mut Models, node: NodeId, a: si_types::geom::Vector) {
        self.say(format!("SI_invKinResetPrefAngle({},0,{:x},{:x},{:x})", self.node(node), a.x.to_bits(), a.y.to_bits(), a.z.to_bits()));
    }
    fn metaball_touch_ball(&mut self, models: &mut Models, node: NodeId) {
        let def = models.m(node).definition.0;
        self.say(format!("SI_metaballTouch({},0)", self.space.show(def)));
    }
    fn texture2d_set_dirty(&mut self, texture: Ptr32<LclMap>) {
        self.say(format!("SI_texture2DSetDirty({})", self.space.show(texture.0)));
    }
    fn ed_get_elem_from_model(&mut self, models: &Models, node: NodeId) -> u32 {
        // the stand-in answers the block at the node's +0
        if self.say(format!("SI_edGetElemFromModel({})", self.node(node))) != 0 { models.m(node).block_00.0 } else { 0 }
    }
    fn ed_find_fcv(&mut self, element: u32, id: i32) -> bool {
        self.say(format!("SI_edFindFcv({},1,{id:x})", self.space.show(element))) != 0
    }
}

fn cur_tag(models: &Models, node: NodeId) -> u32 {
    match &models.data[node].current {
        Current::Own(b) => tag_of(b),
        Current::Def(i) => models.data[node].defs.get(*i).map_or(0, tag_of),
        Current::None => 0,
    }
}

impl transform::GeometryHooks for Trace {
    fn transform(&mut self, kind: i32, definition: &mut Definition, _: &si_math::types::Matrix) {
        let name = match kind {
            1 => "SI_patchTransform",
            2 => "SI_faceTransform",
            6 => "SI_splineTransform",
            9 => "SI_nurbsCrvApplyMatrix",
            _ => "SI_nurbsSrfApplyMatrix",
        };
        self.say(format!("{name}({})", self.space.show(tag_of(definition))));
    }
    fn compute_bounding(&mut self, _: &mut Models, node: NodeId) {
        self.say(format!("SI_modelComputeBounding({})", self.node(node)));
    }
}

fn words_of(m: &Model) -> Vec<u32> {
    let mut w = vec![0u32; WORDS];
    unsafe { core::ptr::copy_nonoverlapping(m as *const Model as *const u8, w.as_mut_ptr() as *mut u8, 0x430) };
    if mutate() {
        for x in &mut w {
            *x ^= 0x0001_0001;
        }
    }
    w
}

/// Offsets of the record's pointers to what the crate owns in `NodeData` (name, definition
/// array, original and working definition, initial transformation): the port does not keep
/// those handles up to date, [`same_defs`] compares what they stand for.
const OWNED: [usize; 5] = [0x04, 0x38, 0x3c, 0x48, 0x328];

/// The words of record `name` after the call against the port's node: values bit for bit,
/// pointers as handles (pointers into blocks the case does not name are not compared).
pub fn same_node(c: &Case, name: &str, models: &Models, node: NodeId, space: &mut Space) -> Verdict {
    let got = words_of(models.m(node));
    for (i, w) in c.after(name).words().iter().enumerate() {
        let want = match w {
            Word::Value { value, .. } => *value as u32,
            Word::Ref { name, .. } => {
                if OWNED.contains(&(i * 4)) || (name.starts_with("heap") && !space.alias.contains_key(name)) {
                    continue;
                }
                space.of(w)
            }
            Word::Unwritten { .. } => continue,
        };
        if OWNED.contains(&(i * 4)) && want == 0 && i * 4 != 0x38 {
            // a null the port must have as "nothing owned"; checked by same_defs
            continue;
        }
        if want != got[i] {
            return Verdict::Different(format!("{name}+{:#x}: {want:08x} against the port's {:08x}", i * 4, got[i]));
        }
    }
    Verdict::Same
}

/// The definitions of record `name` after the call against what the port's node owns: the
/// stored ones by number, the working one (which stored one, or a separate one), whether
/// there is an original one.
pub fn same_defs(c: &Case, name: &str, models: &Models, node: NodeId, space: &mut Space) -> Verdict {
    let w = c.after(name).words();
    let d = &models.data[node];
    let nb = models.m(node).nb_defs as usize;
    if d.defs.len() != nb {
        return Verdict::Different(format!("{name}: the port holds {} definitions for a count of {nb}", d.defs.len()));
    }
    let mut stored = Vec::new();
    if let Word::Ref { name: array, .. } = &w[0x38 / 4] {
        for e in c.after(array).words().iter().take(nb) {
            stored.push(space.of(e));
        }
    }
    for (i, h) in stored.iter().enumerate() {
        if *h != 0 && *h != tag_of(&d.defs[i]) {
            return Verdict::Different(format!("{name}: stored definition {i} is {} against the port's {}", space.show(*h), space.show(tag_of(&d.defs[i]))));
        }
    }
    let own = |x: &Word| matches!(x, Word::Ref { name, .. } if name.starts_with("heap"));
    let cur = &w[0x48 / 4];
    let same_cur = match &d.current {
        Current::None => matches!(cur, Word::Value { value: 0, .. }),
        Current::Def(i) => !own(cur) && stored.get(*i) == Some(&space.of(cur)),
        Current::Own(b) => own(cur) || (space.of(cur) == tag_of(b) && !stored.contains(&tag_of(b))),
    };
    if !same_cur {
        return Verdict::Different(format!("{name}: working definition {cur:?} against the port's {:?}", d.current));
    }
    let org = &w[0x3c / 4];
    if matches!(org, Word::Value { value: 0, .. }) != d.org_def.is_none() {
        return Verdict::Different(format!("{name}: original definition {org:?} against the port's {:?}", d.org_def.is_some()));
    }
    Verdict::Same
}

/// Offsets of every pointer field of the record that `emu_world.py` types as a pointer.
pub const POINTERS: [usize; 38] = [
    0x00, 0x04, 0x38, 0x3c, 0x44, 0x48, 0x50, 0x58, 0x60, 0x64, 0x20c, 0x21c, 0x224, 0x22c, 0x234, 0x2bc, 0x2c0, 0x2c4, 0x2c8, 0x2d4, 0x2d8,
    0x2dc, 0x2e8, 0x318, 0x324, 0x328, 0x32c, 0x344, 0x34c, 0x3e8, 0x408, 0x40c, 0x410, 0x414, 0x418, 0x41c, 0x428, 0x42c,
];

/// A record the routine made itself, dumped as 16-bit words (so that a 16-bit field next to
/// padding nobody wrote can be told from it), against the port's node. Pointer fields are
/// compared only for being null or not.
pub fn same_new_node(c: &Case, name: &str, m: &Model) -> Verdict {
    let got = words_of(m);
    let half = |i: usize| (got[i / 2] >> (16 * (i % 2))) as u16;
    for (i, w) in c.output(name).written().iter().enumerate() {
        let Some(want) = w else { continue };
        let off = i * 2;
        if POINTERS.contains(&(off & !3)) {
            continue;
        }
        if *want as u16 != half(i) {
            return Verdict::Different(format!("{name}+{off:#x}: {:04x} against the port's {:04x}", *want as u16, half(i)));
        }
    }
    Verdict::Same
}

/// The update node of each node's element (`emu_world.py`, group "anim"), for
/// [`Trace::upd`].
pub fn update_nodes(c: &Case, space: &mut Space, count: usize) -> HashMap<NodeId, UpdNode> {
    let mut out = HashMap::new();
    for k in 0..count {
        let Word::Ref { name: el, .. } = &c.input(&format!("m{k}")).words()[0] else { continue };
        let Some(el) = c.try_input(el) else { continue };
        if el.len() != 4 {
            continue;
        }
        let Word::Ref { name: up, .. } = &el.words()[1] else { continue };
        let w = c.input(up).words();
        let args = match &w[7] {
            Word::Ref { name, .. } => Some(c.input(name).words()[3].value().unwrap_or(0) as u16),
            _ => None,
        };
        out.insert(k, UpdNode { ptr: space.handle(up, 0), has_arguments: args.is_some(), active: w[8].value().unwrap_or(0) as u16 != 0, argument_active: args.unwrap_or(0) != 0 });
    }
    out
}

/// What a node owns beside its record, where the case lays it out for comparison: the
/// initial transformation (`srt<k>`), the goal and state of the inverse kinematics
/// (`ik<k>`), the rotation angles of every 2D texture.
pub fn same_extras(c: &Case, models: &Models, node: NodeId, space: &Space) -> Verdict {
    let d = &models.data[node];
    if let (Some(f), Some(init)) = (c.try_input(&format!("srt{node}")), &d.init_srt) {
        let got: Vec<f32> = init.iter().flat_map(|v| [v.x, v.y, v.z]).collect();
        let _ = f;
        if !c.after(&format!("srt{node}")).same_f32(&got) {
            return Verdict::Different(format!("srt{node}: the port has {got:?}"));
        }
    }
    if let Some(f) = c.try_input(&format!("ik{node}")) {
        if f.len() >= 15 && d.tx.inv_kin_kind.is_some() {
            let w = c.after(&format!("ik{node}")).written();
            let got = [d.tx.inv_kin_goal[0].to_bits(), d.tx.inv_kin_goal[1].to_bits(), d.tx.inv_kin_goal[2].to_bits()];
            for i in 0..3 {
                if w[2 + i].is_some_and(|v| v as u32 != got[i] ^ mutate() as u32) {
                    return Verdict::Different(format!("ik{node}: goal {:?} against the port's {:?}", &w[2..5], d.tx.inv_kin_goal));
                }
            }
            if w[14].is_some_and(|v| v as u32 != d.tx.inv_kin_state as u32) {
                return Verdict::Different(format!("ik{node}: state {:?} against the port's {}", w[14], d.tx.inv_kin_state));
            }
        }
    }
    for t in d.cx.material_textures.iter().flatten().chain(d.cx.textures.iter()) {
        let name = space.show(t.ptr.0);
        let name = name.trim_start_matches('&').split('+').next().unwrap_or("");
        let Some(f) = c.try_input(name) else { continue };
        if f.len() < 40 {
            continue;
        }
        let w = c.after(name).written();
        let got = [t.rot.x.to_bits(), t.rot.y.to_bits(), t.rot.z.to_bits()];
        for i in 0..3 {
            if w[37 + i].is_some_and(|v| v as u32 != got[i] ^ mutate() as u32) {
                return Verdict::Different(format!("{name}: angles {:x?} against the port's {got:x?}", &w[37..40]));
            }
        }
    }
    Verdict::Same
}

/// The written words of a float field against the first values of `got`.
pub fn same_f32_prefix(want: &si_x87::vectors::Field, got: &[f32]) -> bool {
    want.written().iter().zip(got).all(|(w, g)| w.is_none_or(|w| w as u32 == g.to_bits() ^ mutate() as u32))
}
