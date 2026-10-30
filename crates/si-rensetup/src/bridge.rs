// SPDX-License-Identifier: MIT
//! A scene loaded by `si-scene` turned into what the render set-up works on.
//!
//! `si-scene` keeps what the elements point at as the content of the files (`si-hrc` nodes,
//! materials, textures) with the links the loader made beside them. The set-up reads and
//! writes the program's records. This module makes those records: one `__SI_Model` per node
//! of every hierarchy, a record per material, texture, light and camera, with the pointers
//! the loader's links stand for. It ports no original routine. A handle of the scene's
//! object arena is the same handle in the heap made here, so the dictionary is used as it is.
//!
//! Not made: geometry (a node with definitions gets a marker in its geometry pointers), the
//! rotation matrix and the global matrices (they stay as `SI_modelAllocate` leaves them; the
//! program computes them from the angles after loading), shader records.

use crate::world::{lig, mat, t2d, Env, Heap, Image, Obj};
use si_scene::load::Scene;
use si_scene::object::Object;
use si_types::geom::Vector;
use si_types::model::Model;
use si_types::Ptr32;

/// What the bridge made, for the reports.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub nodes: usize,
    pub materials: usize,
    pub textures2d: usize,
    pub textures3d: usize,
    pub lights: usize,
    pub instances: usize,
}

struct Links {
    father: Option<usize>,
    son: Option<usize>,
    brother: Option<usize>,
}

/// Father, first son and next brother of every node, in the order `si_hrc::Node::walk`
/// numbers them: a node, the nodes under its first son, then its next brother.
fn links(n: &si_hrc::Node, father: Option<usize>, out: &mut Vec<Links>) -> usize {
    let i = out.len();
    out.push(Links { father, son: None, brother: None });
    if let Some(s) = &n.son {
        let si = links(s, Some(i), out);
        out[i].son = Some(si);
    }
    if let Some(b) = &n.brother {
        let bi = links(b, father, out);
        out[i].brother = Some(bi);
    }
    i
}

fn vector(v: [f32; 3]) -> Vector {
    Vector { x: v[0], y: v[1], z: v[2] }
}

fn material_image(m: &si_hrc::Material) -> Image {
    let mut im = Image::new(mat::SIZE);
    for (k, b) in m.name.iter().take(mat::NAME_LEN).enumerate() {
        im.bytes[k] = *b;
    }
    im
}

fn tex2d_image(t: &si_hrc::Texture2D) -> Image {
    let mut im = Image::new(t2d::SIZE);
    im.set_u16(t2d::METHOD, t.method as u16);
    im
}

/// Builds heap and globals from a loaded scene. The scene keeps its dictionary and program
/// globals; take them with `std::mem::take` or use them in place.
pub fn heap_of(sc: &Scene) -> (Heap, Stats) {
    let mut heap = Heap::default();
    let mut st = Stats::default();
    let last = sc.objects.handles().last().unwrap_or(0);
    // one block per object of the scene, same handle
    for h in 1..=last {
        let obj = match sc.objects.get(h) {
            Some(Object::ModelNode { .. }) => Obj::Model(Box::new(Model::allocate(None, &mut 0).0)),
            Some(Object::Material(m)) => {
                st.materials += 1;
                Obj::Material(material_image(m))
            }
            Some(Object::Texture2D(t)) => {
                st.textures2d += 1;
                Obj::Tex2d(tex2d_image(t))
            }
            Some(Object::Texture3D(_)) => {
                st.textures3d += 1;
                Obj::Tex3d(Image::new(crate::world::t3d::SIZE))
            }
            Some(Object::Light(l)) => {
                st.lights += 1;
                let mut im = Image::new(lig::SIZE);
                im.bytes[..30].copy_from_slice(&l.name);
                im.set_u16(0x1e, l.kind as u16);
                im.set_i32(lig::SELECTIVE, l.selective[0]);
                im.set_i32(lig::SELECTIVE + 4, l.selective[1]);
                Obj::Light(im)
            }
            Some(Object::Camera(_)) => Obj::Camera(Image::new(crate::world::cam::SIZE)),
            _ => Obj::Opaque,
        };
        let got = heap.alloc(obj);
        debug_assert_eq!(got, h);
        if sc.objects.get(h).is_none() {
            heap.free(h);
        }
    }
    // the loader's links between materials and textures
    for (m, l) in &sc.mat_links {
        let (tex, tex3) = (l.textures.and_then(|(_, head)| head).unwrap_or(0), l.texture3d.flatten().unwrap_or(0));
        if let Some(im) = heap.image_mut(*m) {
            im.set_u32(mat::TEXTURES, tex);
            im.set_u32(mat::TEXTURE3D, tex3);
        }
    }
    for (t, next) in &sc.tex_next {
        if let Some(im) = heap.image_mut(*t) {
            im.set_u32(t2d::NEXT, *next);
        }
    }
    // the hierarchies
    for th in sc.models.handles().collect::<Vec<_>>() {
        let tree = sc.models.get(th).unwrap();
        let mut ls = Vec::new();
        links(&tree.root, None, &mut ls);
        // a node no element claimed has no object yet: it gets one here
        let handles: Vec<u32> = (0..ls.len())
            .map(|i| match tree.handles.get(i).copied().unwrap_or(0) {
                0 => heap.alloc(Obj::Model(Box::new(Model::allocate(None, &mut 0).0))),
                h => h,
            })
            .collect();
        let ptr = |i: Option<usize>| Ptr32::new(i.map_or(0, |i| handles[i]));
        let mut i = 0;
        let mut nodes: Vec<&si_hrc::Node> = Vec::new();
        tree.root.walk(0, &mut |_, n| nodes.push(n));
        while i < nodes.len().min(ls.len()) {
            let n = nodes[i];
            st.nodes += 1;
            let name = heap.strdup(si_hrc::io::c_str(&n.name));
            let materials = match tree.mat_links.get(i).cloned().flatten() {
                Some(list) => list,
                None => n.materials.iter().map(|m| m.as_ref().map_or(0, |m| heap.alloc(Obj::Material(material_image(m))))).collect(),
            };
            let nb_materials = if tree.mat_links.get(i).cloned().flatten().is_some() { materials.len() as i16 } else { n.nb_materials };
            let (nb_tex, tex) = match tree.tex_links.get(i).copied().flatten() {
                Some((count, head)) => (count, head.unwrap_or(0)),
                None => {
                    // the node's own textures, chained as the file has them
                    let own: Vec<u32> = n.textures.iter().map(|t| heap.alloc(Obj::Tex2d(tex2d_image(t)))).collect();
                    for w in own.windows(2) {
                        heap.image_mut(w[0]).unwrap().set_u32(t2d::NEXT, w[1]);
                    }
                    (n.nb_textures, own.first().copied().unwrap_or(0))
                }
            };
            let tex3 = match tree.tex3d_links.get(i).copied().flatten() {
                Some(t) => t.unwrap_or(0),
                None => n.texture3d.as_ref().map_or(0, |_| heap.alloc(Obj::Tex3d(Image::new(crate::world::t3d::SIZE)))),
            };
            let arr = if materials.is_empty() { 0 } else { heap.alloc(Obj::Words(materials)) };
            let geometry = if n.definitions.is_empty() { 0 } else { heap.alloc(Obj::Opaque) };
            let m = heap.model_mut(handles[i]).unwrap();
            m.name = Ptr32::new(name);
            m.kind = n.kind;
            m.algo_type = n.algorithm;
            if n.flag8 {
                m.flags_08 |= 8;
            }
            let srt = tree.state.get(i).copied().flatten();
            m.scal = vector(srt.map_or(n.scaling, |s| [s[0], s[1], s[2]]));
            m.vrot = vector(srt.map_or(n.rotation, |s| [s[3], s[4], s[5]]));
            m.trans = vector(srt.map_or(n.translation, |s| [s[6], s[7], s[8]]));
            m.nb_materials = nb_materials;
            m.materials = Ptr32::new(arr);
            m.unk_216 = nb_tex;
            m.textures = Ptr32::new(tex);
            m.texture3d = Ptr32::new(tex3);
            m.nb_defs = n.definitions.len() as u16;
            m.defs = Ptr32::new(geometry);
            m.definition = Ptr32::new(geometry);
            m.latdep = Ptr32::new(handles[i]);
            m.father = ptr(ls[i].father);
            m.son = ptr(ls[i].son);
            m.brother = ptr(ls[i].brother);
            m.elem_id = tree.elem_ids.get(i).copied().unwrap_or(-1);
            m.visibility = n.visibility;
            m.motion_blur = n.motion_blur;
            m.matte = n.matte;
            m.garbage_matte = tree.garbage_matte.get(i).copied().unwrap_or(n.garbage_matte);
            if n.algorithm == crate::world::algo::INSTANCE {
                st.instances += 1;
            }
            i += 1;
        }
    }
    (heap, st)
}

/// Everything the set-up needs from a loaded scene but the dictionary: program globals are
/// moved out of the scene (it keeps fresh ones), the heap is made, the scene's camera named.
pub fn env_of(sc: &mut Scene) -> (Env, Stats) {
    let (heap, st) = heap_of(sc);
    let mut env = Env { p: std::mem::take(&mut sc.program), heap, ..Env::default() };
    env.g.scene_camera = sc.camera;
    (env, st)
}
