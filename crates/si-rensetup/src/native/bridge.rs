// SPDX-License-Identifier: MIT
//! What a scene loaded by `si-scene` lacks in the memory image before a render command can
//! run on it (finding 0023, sections 18.2 and 19).
//!
// adapter: not in place
//!
//! `si-scene` builds the scene in the image: the dictionaries, every element's object as
//! the file readers on the image make it (model nodes with their geometry, materials,
//! textures, lights, the camera), the scene's graph list with every element's node (its
//! builder is `si-view`'s port, called where the loader calls it). This module ports no
//! original routine. What it still supplies, and why:
//!
//! 1. **The initial values of `.data`** this package's routines read (`ptrDefault`, the
//!    24 words of `SI_defaultMaterial`, the message texts): no crate the loader calls
//!    stores them (the check stores them at start-up too, before the load).
//! 2. **The render controls** ([`controls_from_setup`]): the set-up file's reader is on
//!    values.
//!
//! Gone (finding 0023, section 21): the play control, which the loader now stores in its
//! globals where the description's reader does (`si_scene::load::play_to_image`).
//!
//! Not in the image, and so missing for a render: see finding 0023, section 19.

use super::globals::*;
use super::outside::Outside;
use si_mem::{Mem, Ptr32};
use si_scene::load::Scene;
use si_types::dict::{ElemDict, ELEM_VALID};
use si_types::model::Model;

/// What was found in the image, for whoever runs a command on it.
#[derive(Debug, Default)]
pub struct Placed {
    /// The scene's dictionary (the loader's).
    pub dict: Ptr32<ElemDict>,
    /// The camera record (the process record's third word).
    pub camera: u32,
    /// The model nodes of the hierarchies the loader read into the image.
    pub nodes: usize,
    /// The root nodes (no father) of the MODELS elements in use, in chapter order.
    pub roots: Vec<Ptr32<Model>>,
    /// The light records, in the order their files were read.
    pub lights: Vec<u32>,
    /// Elements whose object is a record of the image, and elements whose object is still
    /// a value of the loader (an address of the handle range), by chapter name.
    pub elements_in_image: usize,
    pub elements_left: usize,
    pub left_by_chapter: std::collections::BTreeMap<Vec<u8>, usize>,
}

/// Completes the image of a loaded scene: see the head of this file. `sc` is the scene
/// the loader left (its image is `m`, taken out of it).
pub fn place_scene<O: Outside>(m: &mut Mem, _o: &mut O, sc: &Scene) -> Result<Placed, String> {
    let mut placed = Placed::default();
    // ---- item 1: initial values
    init_loaded(m);
    // ---- what is in the image
    let ed = super::current_dict(m);
    placed.dict = ed;
    placed.camera = m[m[SI_PROC_DATA]].camera.0;
    placed.lights = sc.contents.lights_read.clone();
    placed.nodes = sc.models.handles().filter_map(|h| sc.models.get(h)).map(|t| t.image.len()).sum();
    let cid = m.read(SI_CID);
    for chap in 0..m[ed].nb_lists {
        let chapter = m[ed].lists.add(chap);
        for i in 0..m[chapter].nb_elems {
            let e = m[chapter].elems.add(i);
            let data = m[e].data.0;
            if data == 0 {
                continue;
            }
            if si_scene::world::obj_of(m[e].data) != 0 {
                placed.elements_left += 1;
                *placed.left_by_chapter.entry(m.cstr(m[chapter].name.0)).or_default() += 1;
                continue;
            }
            placed.elements_in_image += 1;
            if chap == cid.model && m[e].status == ELEM_VALID {
                let node: Ptr32<Model> = Ptr32::new(data);
                if m[node].father.is_null() {
                    placed.roots.push(node);
                }
            }
        }
    }
    if let Some(f) = m.fault() {
        return Err(format!("the loaded scene's image has a fault: {f:?}"));
    }
    Ok(placed)
}

/// The render controls of a loaded scene: `SI_defaultControl` as the program has it when a
/// render is asked.
///
// adapter: not in place
///
/// The start-up state is [`super::matter::ren_control_init`]'s (the original's routine,
/// on the image). The scene's set-up file is read by `si-ascii` into a value
/// (`si_ascii::setup::RenderControl`), not into the image: its members are stored here at
/// the offsets that value documents for them. Members the value does not carry keep
/// their start-up state.
pub fn controls_from_setup(m: &mut Mem, c: &si_ascii::setup::RenderControl) {
    let dc = SI_DEFAULT_CONTROL.0;
    let text = |m: &mut Mem, offset: u32, s: &Option<Vec<u8>>| {
        if let Some(s) = s {
            let old = m.u32(dc + offset);
            if old != 0 {
                m.free(old);
            }
            let local = m.local(s.len() as u32 + 1);
            m.put_cstr(local, s);
            let copy = m.strdup(local);
            m.release(s.len() as u32 + 1);
            m.set_u32(dc + offset, copy);
        }
    };
    let f3 = |m: &mut Mem, offset: u32, v: [f32; 3]| {
        for (k, x) in v.iter().enumerate() {
            m.set_f32(dc + offset + 4 * k as u32, *x);
        }
    };
    m.set_i16(dc + 0xfa, c.rendering_type);
    m.set_i16(dc + 0xfe, c.has_picture);
    m.set_i16(dc + 0x100, c.fade);
    m.set_f32(dc + 0x104, c.fade_start);
    m.set_f32(dc + 0x10c, c.fade_end);
    f3(m, 0x118, c.fade_colour);
    m.set_f32(dc + 0x130, c.fade_transparency);
    m.set_i16(dc + 0x13c, c.on_field);
    m.set_i16(dc + 0x13e, c.first_field);
    m.set_f32(dc + 0x154, c.pixel_ratio);
    m.set_i16(dc + 0x158, c.filter);
    m.set_i16(dc + 0x15a, c.antialias);
    m.set_i16(dc + 0x15c, c.antialias_level);
    m.set_f32(dc + 0x16c, c.antialias_threshold);
    m.set_i16(dc + 0x178, c.frame_buffer);
    text(m, 0x17c, &c.output_file);
    text(m, 0x180, &c.pre_rendering);
    text(m, 0x184, &c.post_rendering);
    m.set_i16(dc + 0x1da, c.max_tria_depth);
    m.set_i16(dc + 0x1dc, c.max_tree_depth);
    m.set_i16(dc + 0x1e4, c.max_ray_depth);
    m.set_i16(dc + 0x1e6, c.time_statistics);
    m.set_i16(dc + 0x1e8, c.tree_statistics);
    m.set_i16(dc + 0x1ea, c.memory_statistics);
    m.set_i16(dc + 0x1ec, c.line_statistics);
    for (k, v) in c.frames.iter().enumerate() {
        m.set_i16(dc + 0x1f4 + 2 * k as u32, *v);
    }
    f3(m, 0x1fc, c.ambience);
    m.set_i16(dc + 0x20c, c.back_culling);
    m.set_i16(dc + 0x210, c.motion_blur);
    m.set_f32(dc + 0x214, c.shutter_speed);
    m.set_f32(dc + 0x21c, c.min_movement);
    m.set_i16(dc + 0x226, c.subregion);
    m.set_i16(dc + 0x22a, c.draw_subregion);
    for (k, v) in c.subregion_frame.iter().enumerate() {
        m.set_f32(dc + 0x22c + 4 * k as u32, *v);
    }
    m.set_i16(dc + 0x23c, c.subregion_size[0]);
    m.set_i16(dc + 0x23e, c.subregion_size[1]);
    m.set_i16(dc + 0x240, c.tag_file);
    m.set_i16(dc + 0x24c, c.z_channel);
    m.set_i32(dc + 0x250, c.dithering[0]);
    m.set_i32(dc + 0x254, c.dithering[1]);
    m.set_f32(dc + 0x258, c.dithering_value);
    m.set_i16(dc + 0x25c, c.fog);
    m.set_f32(dc + 0x260, c.fog_density);
    m.set_f32(dc + 0x264, c.fog_base);
    m.set_f32(dc + 0x268, c.fog_thickness);
    m.set_f32(dc + 0x26c, c.fog_start);
    m.set_f32(dc + 0x270, c.fog_end);
    f3(m, 0x284, c.fog_colour);
    // GLOBAL_SWITCHES, in the order of the value's table of keywords
    for (k, offset) in [0x584u32, 0x586, 0x588, 0x58a, 0x58c, 0x58e, 0x59e, 0x5a4, 0x5aa, 0x5a6, 0x5a8, 0x5a0, 0x5a2].into_iter().enumerate() {
        m.set_i16(dc + offset, c.switches[k]);
    }
    m.set_i16(dc + 0x59a, c.resolution[0]);
    m.set_i16(dc + 0x59c, c.resolution[1]);
}
