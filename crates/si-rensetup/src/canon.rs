// SPDX-License-Identifier: MIT
//! A canonical form of dictionaries and the objects they point at, as a list of 32-bit
//! words, for comparing the port with the original (finding 0023, "The checks").
//!
//! The emulation harness lays a made-up world out in the emulator's memory; after the
//! original routine has run, a walker of ours reads that memory from the dictionaries
//! outwards and writes this form. The check builds the same world from the form
//! ([`parse`]), runs the port, and writes the form again ([`dump`]): the two must be equal
//! word for word. Pointers cannot be compared as numbers, so every pointer to an object the
//! walk follows is replaced by the object's number in order of first appearance; the walk's
//! order is therefore part of the format, and is the same here and in
//! `re/tools/si-rensetup/world.py`.
//!
//! ```text
//! world   = count, dictionary…, count, (kind, object)…, count, object record…
//! dict    = chapters, upd dirty, topo dirty, +0x58, +0x6c, chapter…
//! chapter = name, kind, suffix, slots, element…
//! element = has node, object, +0x08, 13 words from +0x0c, 8 words from +0x40, forward lists,
//!           backward lists, +0x70, +0x80, [node], list…
//! node    = +0x00, 7 strings (+0x04 … +0x1c), time, full name, +0x28, mode
//! list    = count, capacity, cursor, +0x10, flag, 8 words a relation
//! string  = 0xffffffff for null, else length and the bytes, four to a word
//! record  = kind, then by kind: model: name, material array (0xffffffff or count and
//!           objects), latdep, +0x218, textures, 3D texture, father, brother, son; material:
//!           textures, 3D texture; 2D texture: next; light: table (0xffffffff or words);
//!           then (offset, word) for every other word that is not zero, and 0xffffffff
//! ```

use crate::world::{lig, mat, t2d, t3d, cam, Heap, Image, Obj};
use si_db::ListNode;
use si_scene::dict::Dict;
use si_types::dict::{ARelation, ChapIds, Elem, RelList};
use si_types::model::Model;
use si_types::store::{ElemStore, RelListStore};
use si_types::{Ptr32, Zero};
use std::collections::HashMap;

pub const K_MODEL: u32 = 1;
pub const K_MATERIAL: u32 = 2;
pub const K_TEX2D: u32 = 3;
pub const K_TEX3D: u32 = 4;
pub const K_LIGHT: u32 = 5;
pub const K_CAMERA: u32 = 6;
const NIL: u32 = 0xffff_ffff;

/// The kind of object the elements of a chapter point at; 0 for chapters whose objects the
/// walk does not follow (their pointers are compared as numbers).
pub fn kind_of_chapter(cid: &ChapIds, chap: i32) -> u32 {
    if chap == cid.model {
        K_MODEL
    } else if chap == cid.mat {
        K_MATERIAL
    } else if chap == cid.t2d {
        K_TEX2D
    } else if chap == cid.t3d {
        K_TEX3D
    } else if chap == cid.lig {
        K_LIGHT
    } else {
        0
    }
}

fn model_words(m: &Model) -> Vec<u32> {
    let mut w = vec![0u32; Model::SIZE / 4];
    // a plain record of the 32-bit layout, every byte a field or asserted padding
    unsafe { core::ptr::copy_nonoverlapping(m as *const Model as *const u8, w.as_mut_ptr() as *mut u8, Model::SIZE) };
    w
}

fn model_from_words(w: &[u32]) -> Model {
    assert_eq!(w.len() * 4, Model::SIZE);
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const Model) }
}

fn elem_words(e: &Elem) -> Vec<u32> {
    let mut w = vec![0u32; Elem::SIZE / 4];
    unsafe { core::ptr::copy_nonoverlapping(e as *const Elem as *const u8, w.as_mut_ptr() as *mut u8, Elem::SIZE) };
    w
}

fn elem_from_words(w: &[u32]) -> Elem {
    assert_eq!(w.len() * 4, Elem::SIZE);
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const Elem) }
}

fn rel_words(r: &ARelation) -> [u32; 8] {
    let mut w = [0u32; 8];
    unsafe { core::ptr::copy_nonoverlapping(r as *const ARelation as *const u8, w.as_mut_ptr() as *mut u8, 32) };
    w
}

fn rel_from_words(w: &[u32]) -> ARelation {
    unsafe { core::ptr::read_unaligned(w.as_ptr() as *const ARelation) }
}

/// The word offsets of a model record that hold pointers the walk follows or replaces.
const MODEL_SPECIAL: [usize; 9] = [0x004, 0x044, 0x20c, 0x218, 0x21c, 0x224, 0x2bc, 0x2c0, 0x2c4];

struct Out<'a> {
    w: Vec<u32>,
    heap: &'a Heap,
    ids: HashMap<u32, u32>,
    queue: Vec<(u32, u32)>,
}

impl Out<'_> {
    fn id(&mut self, handle: u32, kind: u32) -> u32 {
        if handle == 0 {
            return 0;
        }
        if let Some(i) = self.ids.get(&handle) {
            return *i;
        }
        let i = self.queue.len() as u32 + 1;
        self.ids.insert(handle, i);
        self.queue.push((handle, kind));
        i
    }

    fn string(&mut self, s: Option<&[u8]>) {
        match s {
            None => self.w.push(NIL),
            Some(s) => {
                self.w.push(s.len() as u32);
                for c in s.chunks(4) {
                    let mut b = [0u8; 4];
                    b[..c.len()].copy_from_slice(c);
                    self.w.push(u32::from_le_bytes(b));
                }
            }
        }
    }

    fn sparse(&mut self, words: &[u32], skip: &[usize]) {
        for (i, v) in words.iter().enumerate() {
            if *v != 0 && !skip.contains(&(i * 4)) {
                self.w.push(i as u32 * 4);
                self.w.push(*v);
            }
        }
        self.w.push(NIL);
    }

    fn list(&mut self, l: &RelListStore) {
        self.w.extend([l.rels.len() as u32, l.rec.capacity as u32, l.rec.cur as u32, l.rec.unk_10 as u32, l.rec.flag as u16 as u32]);
        for r in &l.rels {
            self.w.extend(rel_words(r));
        }
    }

    fn node(&mut self, n: &ListNode) {
        self.w.push(n.w00 as u32);
        for s in [&n.s04, &n.sub_path, &n.s0c, &n.prefix, &n.name, &n.version, &n.subst] {
            self.string(s.as_deref());
        }
        self.w.push(n.time as u32);
        self.string(n.full.as_deref());
        self.w.push(n.w28 as u16 as u32 | (n.w2a as u16 as u32) << 16);
        self.w.push(n.mode as u32);
    }

    fn dict(&mut self, d: &Dict, cid: &ChapIds) {
        let r = &d.ed.rec;
        self.w.extend([
            r.nb_lists as u32,
            r.upd_list.dirty as u32,
            r.topo_list.dirty as u32,
            r.is_scene as u16 as u32 | (r.rel_dirty as u16 as u32) << 16,
            r.untold as u16 as u32 | (r.hier_dirty as u16 as u32) << 16,
        ]);
        for (c, l) in d.ed.lists.iter().enumerate() {
            let named = !l.name.is_empty() || !l.rec.name.is_null();
            self.string(named.then_some(&l.name[..]));
            self.w.push(l.rec.kind as u16 as u32);
            self.string(l.suffix.as_deref());
            self.w.push(l.elems.len() as u32);
            let kind = kind_of_chapter(cid, c as i32);
            for e in &l.elems {
                let w = elem_words(&e.rec);
                let node = d.nodes.get(e.rec.node.0);
                self.w.push(node.is_some() as u32);
                let data = if kind != 0 { self.id(e.rec.data.0, kind) } else { e.rec.data.0 };
                self.w.push(data);
                self.w.push(w[2]);
                self.w.extend(&w[3..24]);
                self.w.extend([e.forward.len() as u32, e.backward.len() as u32, w[28], w[32]]);
                if let Some(n) = node {
                    self.node(n);
                }
                for l in e.forward.iter().chain(e.backward.iter()) {
                    self.list(l);
                }
            }
        }
    }

    fn object(&mut self, handle: u32, kind: u32) {
        self.w.push(kind);
        let heap = self.heap;
        match (kind, heap.objs.get(handle)) {
            (K_MODEL, Some(Obj::Model(m))) => {
                self.string(heap.str(m.name.0));
                match heap.words(m.materials.0) {
                    None => self.w.push(NIL),
                    Some(a) => {
                        let n = m.nb_materials.max(0) as usize;
                        self.w.push(n as u32);
                        for k in 0..n {
                            let i = self.id(a.get(k).copied().unwrap_or(0), K_MATERIAL);
                            self.w.push(i);
                        }
                    }
                }
                for (p, k) in [
                    (m.latdep.0, K_MODEL),
                    (m.texture_218.0, K_TEX2D),
                    (m.textures.0, K_TEX2D),
                    (m.texture3d.0, K_TEX3D),
                    (m.father.0, K_MODEL),
                    (m.brother.0, K_MODEL),
                    (m.son.0, K_MODEL),
                ] {
                    let i = self.id(p, k);
                    self.w.push(i);
                }
                let words = model_words(m);
                self.sparse(&words, &MODEL_SPECIAL);
            }
            (K_MATERIAL, Some(Obj::Material(im))) => {
                for (o, k) in [(mat::TEXTURES, K_TEX2D), (mat::TEXTURE3D, K_TEX3D)] {
                    let i = self.id(im.u32(o), k);
                    self.w.push(i);
                }
                self.sparse(&im.words(), &[mat::TEXTURES, mat::TEXTURE3D]);
            }
            (K_TEX2D, Some(Obj::Tex2d(im))) => {
                let i = self.id(im.u32(t2d::NEXT), K_TEX2D);
                self.w.push(i);
                self.sparse(&im.words(), &[t2d::NEXT]);
            }
            (K_TEX3D, Some(Obj::Tex3d(im))) | (K_CAMERA, Some(Obj::Camera(im))) => self.sparse(&im.words(), &[]),
            (K_LIGHT, Some(Obj::Light(im))) => {
                match heap.words(im.u32(lig::TABLE)) {
                    None => self.w.push(NIL),
                    Some(t) => {
                        let n = (im.i32(lig::TABLE_LEN) + 1).max(0) as usize;
                        self.w.push(n as u32);
                        for k in 0..n {
                            self.w.push(t.get(k).copied().unwrap_or(0));
                        }
                    }
                }
                self.sparse(&im.words(), &[lig::TABLE]);
            }
            // a pointer that leads nowhere the port knows: the original's would be a wild read
            _ => self.w.push(NIL),
        }
    }
}

/// The canonical form of some dictionaries, some further objects (`roots`: kind and handle)
/// and everything they lead to.
pub fn dump(dicts: &[&Dict], roots: &[(u32, u32)], heap: &Heap, cid: &ChapIds) -> Vec<u32> {
    dump_with_ids(dicts, roots, heap, cid).0
}

/// The same, with the number each object got (by handle).
pub fn dump_with_ids(dicts: &[&Dict], roots: &[(u32, u32)], heap: &Heap, cid: &ChapIds) -> (Vec<u32>, HashMap<u32, u32>) {
    let mut o = Out { w: Vec::new(), heap, ids: HashMap::new(), queue: Vec::new() };
    o.w.push(dicts.len() as u32);
    for d in dicts {
        o.dict(d, cid);
    }
    o.w.push(roots.len() as u32);
    for (kind, handle) in roots {
        let i = o.id(*handle, *kind);
        o.w.extend([*kind, i]);
    }
    let at = o.w.len();
    o.w.push(0);
    let mut k = 0;
    while k < o.queue.len() {
        let (handle, kind) = o.queue[k];
        o.object(handle, kind);
        k += 1;
    }
    o.w[at] = o.queue.len() as u32;
    (o.w, o.ids)
}

/// What [`parse`] builds.
pub struct Parsed {
    pub dicts: Vec<Dict>,
    /// The further objects, kind and handle.
    pub roots: Vec<(u32, u32)>,
    pub heap: Heap,
}

struct In<'a> {
    w: &'a [u32],
    at: usize,
}

impl In<'_> {
    fn next(&mut self) -> u32 {
        let v = self.w[self.at];
        self.at += 1;
        v
    }

    fn take(&mut self, n: usize) -> &[u32] {
        let s = &self.w[self.at..self.at + n];
        self.at += n;
        s
    }

    fn string(&mut self) -> Option<Vec<u8>> {
        let n = self.next();
        if n == NIL {
            return None;
        }
        let n = n as usize;
        let mut s: Vec<u8> = self.take(n.div_ceil(4)).iter().flat_map(|w| w.to_le_bytes()).collect();
        s.truncate(n);
        Some(s)
    }

    fn sparse(&mut self, size: usize) -> Vec<u32> {
        let mut w = vec![0u32; size / 4];
        loop {
            let o = self.next();
            if o == NIL {
                return w;
            }
            w[o as usize / 4] = self.next();
        }
    }

    fn list(&mut self) -> RelListStore {
        let mut rec = RelList::zero();
        rec.nb = self.next() as i32;
        rec.capacity = self.next() as i32;
        rec.cur = self.next() as i32;
        rec.unk_10 = self.next() as i32;
        rec.flag = self.next() as i16;
        let rels = (0..rec.nb).map(|_| rel_from_words(self.take(8))).collect();
        RelListStore { rec, rels }
    }

    fn node(&mut self, now: i32) -> ListNode {
        let mut n = ListNode::allocate(now);
        n.w00 = self.next() as i32;
        n.s04 = self.string();
        n.sub_path = self.string();
        n.s0c = self.string();
        n.prefix = self.string();
        n.name = self.string();
        n.version = self.string();
        n.subst = self.string();
        n.time = self.next() as i32;
        n.full = self.string();
        let w = self.next();
        n.w28 = w as i16;
        n.w2a = (w >> 16) as i16;
        n.mode = self.next() as u16;
        n
    }
}

fn image_from(words: Vec<u32>) -> Image {
    Image { bytes: words.iter().flat_map(|w| w.to_le_bytes()).collect() }
}

/// Builds dictionaries and heap from the canonical form. Object number *n* gets handle *n*;
/// the strings and arrays the objects own come after.
pub fn parse(words: &[u32]) -> Parsed {
    let mut i = In { w: words, at: 0 };
    let mut dicts = Vec::new();
    for _ in 0..i.next() {
        let mut d = Dict::new();
        let nb = i.next();
        d.ed.rec.upd_list.dirty = i.next() as i32;
        d.ed.rec.topo_list.dirty = i.next() as i32;
        let w = i.next();
        d.ed.rec.is_scene = w as i16;
        d.ed.rec.rel_dirty = (w >> 16) as i16;
        let w = i.next();
        d.ed.rec.untold = w as i16;
        d.ed.rec.hier_dirty = (w >> 16) as i16;
        for c in 0..nb {
            let name = i.string();
            // chapters must have distinct names to be added; a freed name gets a stand-in
            let at = d.ed.add_list(name.as_deref().unwrap_or(format!("\u{1}{c}").as_bytes())) as usize;
            assert_eq!(at, c as usize);
            if name.is_none() {
                d.ed.lists[at].name.clear();
            } else {
                d.ed.lists[at].rec.name = Ptr32::new(1);
            }
            d.ed.lists[at].rec.kind = i.next() as i16;
            d.ed.lists[at].suffix = i.string();
            let n = i.next();
            for _ in 0..n {
                let has_node = i.next() != 0;
                let data = i.next();
                let aux = i.next();
                let mut w = vec![0u32; Elem::SIZE / 4];
                w[3..24].copy_from_slice(i.take(21));
                let (nf, nbk) = (i.next() as usize, i.next() as usize);
                w[28] = i.next();
                w[32] = i.next();
                let mut rec = elem_from_words(&w);
                rec.data = Ptr32::new(data);
                rec.aux_data = Ptr32::new(aux);
                rec.forward.nb_groups = nf as i32;
                rec.backward.nb_groups = nbk as i32;
                if has_node {
                    let node = i.node(0);
                    rec.node = Ptr32::new(d.nodes.alloc(node));
                }
                let forward = (0..nf).map(|_| i.list()).collect();
                let backward = (0..nbk).map(|_| i.list()).collect();
                d.ed.lists[at].elems.push(ElemStore { rec, forward, backward });
            }
            d.ed.lists[at].rec.nb_elems = n as i32;
        }
        dicts.push(d);
    }
    let roots: Vec<(u32, u32)> = (0..i.next()).map(|_| (i.next(), i.next())).collect();
    let n = i.next();
    let mut heap = Heap::default();
    // the objects first, so that object n has handle n; their strings and arrays after
    let mut late: Vec<(u32, Option<Vec<u8>>, Option<Vec<u32>>)> = Vec::new();
    for h in 1..=n {
        let kind = i.next();
        let obj = match kind {
            K_MODEL => {
                let name = i.string();
                let c = i.next();
                let mats = (c != NIL).then(|| i.take(c as usize).to_vec());
                let p: Vec<u32> = i.take(7).to_vec();
                let mut m = model_from_words(&i.sparse(Model::SIZE));
                m.latdep = Ptr32::new(p[0]);
                m.texture_218 = Ptr32::new(p[1]);
                m.textures = Ptr32::new(p[2]);
                m.texture3d = Ptr32::new(p[3]);
                m.father = Ptr32::new(p[4]);
                m.brother = Ptr32::new(p[5]);
                m.son = Ptr32::new(p[6]);
                late.push((h, name, mats));
                Obj::Model(Box::new(m))
            }
            K_MATERIAL => {
                let (t, t3) = (i.next(), i.next());
                let mut im = image_from(i.sparse(mat::SIZE));
                im.set_u32(mat::TEXTURES, t);
                im.set_u32(mat::TEXTURE3D, t3);
                Obj::Material(im)
            }
            K_TEX2D => {
                let next = i.next();
                let mut im = image_from(i.sparse(t2d::SIZE));
                im.set_u32(t2d::NEXT, next);
                Obj::Tex2d(im)
            }
            K_TEX3D => Obj::Tex3d(image_from(i.sparse(t3d::SIZE))),
            K_CAMERA => Obj::Camera(image_from(i.sparse(cam::SIZE))),
            K_LIGHT => {
                let c = i.next();
                let table = (c != NIL).then(|| i.take(c as usize).to_vec());
                late.push((h, None, table));
                Obj::Light(image_from(i.sparse(lig::SIZE)))
            }
            _ => panic!("canonical form: object kind {kind}"),
        };
        assert_eq!(heap.alloc(obj), h);
    }
    for (h, name, array) in late {
        let name = name.map(|s| heap.strdup(&s));
        let array = array.map(|a| heap.alloc(Obj::Words(a)));
        match heap.objs.get_mut(h) {
            Some(Obj::Model(m)) => {
                m.name = Ptr32::new(name.unwrap_or(0));
                m.materials = Ptr32::new(array.unwrap_or(0));
            }
            Some(Obj::Light(im)) => im.set_u32(lig::TABLE, array.unwrap_or(0)),
            _ => {}
        }
    }
    assert_eq!(i.at, words.len(), "canonical form: words left over");
    Parsed { dicts, roots, heap }
}
