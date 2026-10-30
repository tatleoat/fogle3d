// SPDX-License-Identifier: MIT
//! What the render set-up works on: the dictionaries, the objects their elements point at,
//! and the globals of the render files.
//!
//! The dictionaries are `si-scene`'s ([`Dict`]: `si-types`' records with the arrays the
//! original `malloc`s beside them). Their `data` pointers, which the dictionary code never
//! follows, are followed here: a pointer is a handle into one [`Heap`], zero is null, and two
//! pointers are equal exactly when the original's would be.
//!
//! A model node is `si-types`' typed record. Materials, 2D and 3D textures, lights and the
//! camera are not laid out in `si-types` beyond a few fields (finding 0010), and its
//! material record has the name as a pointer where the code has thirty bytes of text; so
//! each of those is kept here as the bytes of the 32-bit original ([`Image`]) and read
//! through the offsets named in [`mat`], [`t2d`], [`t3d`], [`lig`] and [`cam`]. Finding 0023
//! lists every one of them with its evidence, ready to be lifted onto typed records.

use si_scene::dict::{Arena, Dict, ElemId, Program};
use si_types::model::Model;
use si_types::{Opaque, Ptr32};

/// The relations the render set-up names (ids of `Relations.rsrc`).
pub mod rel {
    /// Parent: son to father, list 1 both ways.
    pub const PARENT: i32 = 110;
    /// Model to material.
    pub const MAT: i32 = 300;
    /// Model to a global 2D texture.
    pub const T2D_GLB: i32 = 400;
    /// Material to a local 2D texture.
    pub const T2D_LOC: i32 = 401;
    /// Model to its global 3D texture.
    pub const T3D_GLB: i32 = 500;
    /// Material to its local 3D texture.
    pub const T3D_LOC: i32 = 501;
    /// An element to its shader.
    pub const SHADER: i32 = 550;
    /// The array form of [`SHADER`]: the end of the range the shade tree walk follows.
    pub const SHADER_ARRAY: i32 = 551;
    /// The attribute `LigVisib` ("Light visibility flag") as a relation: a light has it on
    /// its forward side when that flag is animated.
    pub const LIG_VISIB: i32 = 2100;
    /// The animation of a 2D texture: its picture sequence changes from frame to frame.
    pub const ANIM_T2D: i32 = 15005;
    /// Custom effect.
    pub const CUSTOM: i32 = 20000;
    /// Instance to its master, list 4.
    pub const INST: i32 = 20002;
}

/// `__SI_Model` +0x2ec values the render files test.
pub mod algo {
    /// The icon of a custom effect.
    pub const CUSTOM_ICON: i32 = 13;
    /// An instance: the node stands for a copy of another hierarchy, its master.
    pub const INSTANCE: i32 = 15;
}

/// Offsets in `__SI_Material` (0xdcc bytes).
pub mod mat {
    pub const SIZE: usize = 0xdcc;
    /// Thirty bytes of text: the material's name (`strncpy(material, name, 0x1e)`).
    pub const NAME: usize = 0x000;
    pub const NAME_LEN: usize = 0x1e;
    /// Head of the list of local 2D textures (linked through [`super::t2d::NEXT`]).
    pub const TEXTURES: usize = 0x6ec;
    /// The local 3D texture.
    pub const TEXTURE3D: usize = 0x6fc;
    /// 16 bits: static blur on.
    pub const STATIC_BLUR: usize = 0x758;
    /// The index of the material's element in the MATERIALS chapter.
    pub const ELEM_ID: usize = 0xda8;
    /// Four shader records: material, volume, shadow and photon shader.
    pub const SHADERS: [usize; 4] = [0xdb0, 0xdb4, 0xdb8, 0xdbc];
}

/// Offsets in `__SI_LclMap`, a 2D texture (0x1d8 bytes).
pub mod t2d {
    pub const SIZE: usize = 0x1d8;
    /// A pointer the render dictionary's deletion clears: the texture's picture in memory.
    pub const MAP: usize = 0x24;
    /// 16 bits: the mapping method; 4 is "UV", 7 and 8 the two reflection map methods.
    pub const METHOD: usize = 0x2a;
    /// The next texture of the same model or material.
    pub const NEXT: usize = 0x16c;
    /// The texture's shader record.
    pub const SHADER: usize = 0x198;
    /// 16 bits: non-zero when the texture changes from frame to frame.
    pub const ANIMATED: usize = 0x19c;
}

/// Offsets in `__SI_Texture3D` (0x158 bytes).
pub mod t3d {
    pub const SIZE: usize = 0x158;
    /// The texture's shader record.
    pub const SHADER: usize = 0x14c;
}

/// Offsets in `light` (0x240 bytes).
pub mod lig {
    pub const SIZE: usize = 0x240;
    /// Non-zero: the light is selective (it lights only some models, or all but some).
    pub const SELECTIVE: usize = 0x15c;
    /// The number of model elements the table below was sized for.
    pub const TABLE_LEN: usize = 0x164;
    /// One word per model element, and one more.
    pub const TABLE: usize = 0x168;
}

/// Offsets in `SI_Camera` (0x13c bytes).
pub mod cam {
    pub const SIZE: usize = 0x13c;
    /// 16 bits: the kind of view the camera belongs to; 0x7d3 is a perspective view.
    pub const VIEW: usize = 0x04;
    /// Depth of field mode; 2 is the mode that uses the greatest circle of confusion.
    pub const DOF_MODE: usize = 0x6c;
    /// The greatest circle of confusion, a float.
    pub const MAX_COC: usize = 0x98;
    /// The view code of a perspective window.
    pub const VIEW_PERSPECTIVE: i16 = 0x7d3;
}

/// A record kept as the bytes of the 32-bit original.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({} bytes)", self.bytes.len())
    }
}

impl Image {
    pub fn new(size: usize) -> Image {
        Image { bytes: vec![0; size] }
    }

    pub fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]])
    }

    pub fn i16(&self, at: usize) -> i16 {
        self.u16(at) as i16
    }

    pub fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes([self.bytes[at], self.bytes[at + 1], self.bytes[at + 2], self.bytes[at + 3]])
    }

    pub fn i32(&self, at: usize) -> i32 {
        self.u32(at) as i32
    }

    pub fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }

    pub fn set_u16(&mut self, at: usize, v: u16) {
        self.bytes[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_u32(&mut self, at: usize, v: u32) {
        self.bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_i32(&mut self, at: usize, v: i32) {
        self.set_u32(at, v as u32);
    }

    pub fn set_f32(&mut self, at: usize, v: f32) {
        self.set_u32(at, v.to_bits());
    }

    /// The record as 32-bit words (its size is a multiple of four).
    pub fn words(&self) -> Vec<u32> {
        self.bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    }
}

/// What a pointer of the render set-up designates.
#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    /// `__SI_Model`. Its pointers to other nodes, to its name, its material array, its
    /// textures hold handles of this heap; its geometry pointers are whatever they were
    /// given (the set-up copies them and never follows them).
    Model(Box<Model>),
    /// `__SI_Material`.
    Material(Image),
    /// `__SI_LclMap`.
    Tex2d(Image),
    /// `__SI_Texture3D`.
    Tex3d(Image),
    /// `light`.
    Light(Image),
    /// `SI_Camera`.
    Camera(Image),
    /// A C string (a model's name).
    Str(Vec<u8>),
    /// An array of words: a model's material pointers (+0x20c), a selective light's table.
    Words(Vec<u32>),
    /// Something the set-up only passes on: a shader record, a selection, a wave.
    Opaque,
}

/// The heap blocks the set-up follows pointers into.
#[derive(Clone, Debug, Default)]
pub struct Heap {
    pub objs: Arena<Obj>,
}

impl Heap {
    pub fn alloc(&mut self, o: Obj) -> u32 {
        self.objs.alloc(o)
    }

    pub fn model(&self, h: u32) -> Option<&Model> {
        match self.objs.get(h) {
            Some(Obj::Model(m)) => Some(m),
            _ => None,
        }
    }

    pub fn model_mut(&mut self, h: u32) -> Option<&mut Model> {
        match self.objs.get_mut(h) {
            Some(Obj::Model(m)) => Some(m),
            _ => None,
        }
    }

    /// The record behind a material, texture, light or camera pointer.
    pub fn image(&self, h: u32) -> Option<&Image> {
        match self.objs.get(h) {
            Some(Obj::Material(i) | Obj::Tex2d(i) | Obj::Tex3d(i) | Obj::Light(i) | Obj::Camera(i)) => Some(i),
            _ => None,
        }
    }

    pub fn image_mut(&mut self, h: u32) -> Option<&mut Image> {
        match self.objs.get_mut(h) {
            Some(Obj::Material(i) | Obj::Tex2d(i) | Obj::Tex3d(i) | Obj::Light(i) | Obj::Camera(i)) => Some(i),
            _ => None,
        }
    }

    pub fn words(&self, h: u32) -> Option<&Vec<u32>> {
        match self.objs.get(h) {
            Some(Obj::Words(w)) => Some(w),
            _ => None,
        }
    }

    pub fn words_mut(&mut self, h: u32) -> Option<&mut Vec<u32>> {
        match self.objs.get_mut(h) {
            Some(Obj::Words(w)) => Some(w),
            _ => None,
        }
    }

    pub fn str(&self, h: u32) -> Option<&[u8]> {
        match self.objs.get(h) {
            Some(Obj::Str(s)) => Some(s),
            _ => None,
        }
    }

    /// `strdup`.
    pub fn strdup(&mut self, s: &[u8]) -> u32 {
        self.alloc(Obj::Str(s.to_vec()))
    }

    /// `free`; null and freed blocks are passed over.
    pub fn free(&mut self, h: u32) {
        self.objs.free(h);
    }

    /// A model's name (`__SI_Model` +4), `None` for a null pointer.
    pub fn model_name(&self, h: u32) -> Option<&[u8]> {
        self.str(self.model(h)?.name.0)
    }

    /// A model's material pointers (`__SI_Model` +0x20c), `None` for a null pointer.
    pub fn model_materials(&self, h: u32) -> Option<&Vec<u32>> {
        self.words(self.model(h)?.materials.0)
    }
}

/// `SI_PreviewStructure` (0x18 bytes; `SI_initializePreviewStruct` clears that many): what a
/// material or texture editor wants shown in a preview in place of what the scene holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// +0x00: 0 a material, 1 a global 2D texture, 2 a global 3D texture, 3 a local 2D
    /// texture, 4 a local 3D texture.
    pub kind: i32,
    /// +0x04: the model being edited.
    pub model: u32,
    /// +0x08: the material to show.
    pub material: u32,
    /// +0x0c: the first 2D texture to show.
    pub tex2d: u32,
    /// +0x10: the 3D texture to show.
    pub tex3d: u32,
    /// +0x14: the material the one at +0x08 takes the place of.
    pub replaced: u32,
}

/// The globals the render files read and write, other than the dictionaries.
#[derive(Clone, Debug, Default)]
pub struct Globals {
    /// `SI_prevStruct` (0x086a6620).
    pub prev: Preview,
    /// `SI_standaloneRenderer` (0x08679400).
    pub standalone_renderer: bool,
    /// `SI_procData` + 8 (0x0867931c): the scene's camera.
    pub scene_camera: u32,
    /// The camera of the default perspective window, `SI_mdlPers` +0x2c.
    pub pers_camera: u32,
    /// `SI_picturePath` (0x086793cc): where pictures are written.
    pub picture_path: Vec<u8>,
    /// The root model of the native renderer's `SI_database` (0x087907e8), which
    /// `SI_createRenderDictionary` clears.
    pub database_root: u32,
    /// `regInstMatArrays` (0x0868e3c4): material arrays made for expanded instances are
    /// entered in the list of blocks the native renderer frees after a frame.
    pub reg_inst_mat_arrays: i16,
    /// `memList` (0x0868e3c0): the blocks entered so, newest first.
    pub mem_list: Vec<u32>,
    /// `SI_defaultControl` +0x59a (0x08781d9a): the picture width.
    pub picture_width: i16,
    /// `SI_defaultControl` +0x586 (0x08781d86): 2D textures are rendered.
    pub textures_2d_on: i16,
}

/// Everything but the dictionaries: the program globals of the dictionary code, the heap,
/// the render globals.
#[derive(Default)]
pub struct Env {
    pub p: Program,
    pub heap: Heap,
    pub g: Globals,
}

/// The object of an element, as a handle (0 for none, for a freed slot or a slot that does
/// not exist).
pub fn data_of(d: &Dict, id: ElemId) -> u32 {
    d.slot(id).map_or(0, |e| e.rec.data.0)
}

/// `SI_elemdictGetElem` on (chapter, index): the slot unless it is out of range or freed.
pub fn get_elem(d: &Dict, chap: i32, index: i32) -> Option<ElemId> {
    d.ed.get_elem(chap, index).map(|_| ElemId::new(chap, index))
}

pub fn ptr(h: u32) -> Ptr32<Opaque> {
    Ptr32::new(h)
}
