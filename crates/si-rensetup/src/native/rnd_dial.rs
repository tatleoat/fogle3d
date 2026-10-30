// SPDX-License-Identifier: MIT
//! `rnd_dial.c`: the command behind the Render dialogue's button, and the question it
//! asks first. (The other 56 functions of the file are the dialogue itself.)

use super::globals::*;
use super::outside::*;
use super::rec::ctl;
use super::{chain_light, chain_root, current_dict, free_dummies, is_light, scene_graf_list};
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_types::scene::GrafNode;

/// Asks for the sub-region box when the controls want one
/// [soft 0x082a5788 selectSubRegionWindow]: with the switches +0x226 and +0x22a of
/// `SI_defaultControl` set, and not in the stand-alone renderer, the user drags the box;
/// 0 when that was cancelled, else 1.
pub fn select_sub_region_window<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) -> i32 {
    let dc = SI_DEFAULT_CONTROL.0;
    if m.u16(dc + ctl::SUBREGION_226) != 0
        && m.u16(dc + ctl::SUBREGION_22A) != 0
        && m[SI_STANDALONE_RENDERER] == 0
        && o.call(m, SI_DO_SUBREGION, &[]) as i16 == 0
    {
        return 0;
    }
    1
}

/// The size in pixels of the sub-region from its fractions [soft 0x082a2884 SI_updateSubregionRes]:
/// the picture's width times what the left and right fractions leave of 1, plus one half,
/// cut towards zero, goes to the 16-bit word at +0x23c; the height and the top and bottom
/// fractions likewise to +0x23e. Everything is computed in the 80-bit register and the
/// result is the low 16 bits of a 32-bit cut (an overflow gives the "integer indefinite").
pub fn update_subregion_res(m: &mut Mem, controls: u32) {
    use si_x87::x87::{Round, F80};
    let c = controls;
    let width = F80::from_i32(m.i16(c + ctl::WIDTH) as i32);
    let one = F80::from_f32(1.0);
    let a = (one - m.f32(c + ctl::CROP_RIGHT)) - m.f32(c + ctl::CROP_LEFT);
    let w = (width * a + 0.5f64).to_i32(Round::Zero);
    m.set_i16(c + ctl::CROP_WIDTH, w as i16);
    let height = F80::from_i32(m.i16(c + ctl::HEIGHT) as i32);
    let b = (one - m.f32(c + ctl::CROP_TOP)) - m.f32(c + ctl::CROP_BOTTOM);
    let h = (height * b + 0.5f64).to_i32(Round::Zero);
    m.set_i16(c + ctl::CROP_HEIGHT, h as i16);
}

/// Renders the sequence the Render dialogue describes with the native renderer
/// [soft 0x082a2000 SI_sequenceRender]; answers a status (0x7f: cancelled or failed,
/// 0x83: the user stopped it, else what the render window and the renderer answer).
///
/// 1. `SI_previewSelectionCall` is cleared; a cancelled sub-region box ends the command
///    with 0x7f.
/// 2. `SI_defaultControl` is copied to `SI_ren`; material arrays of copied nodes are
///    registered for freeing; the renderer's crop switch (+0x224) is cleared; the render
///    window is opened and its answer is the status so far.
/// 3. Every model of the scene's graf list (kinds 2 and 3) gets its `visible` word: not
///    hidden; a model whose algorithm is 18 to 29 is made invisible unless `SI_shwCtl`.
/// 4. The roots are chained ([`chain_root`]; a copy's `visible` asks only "not hidden"),
///    then, in a walk of their own, the lights.
/// 5. `SI_image (0x08257a70)`; a non-zero answer becomes the status. The render window is
///    closed.
/// 6. A message of the renderer goes to the status line (not in the stand-alone
///    renderer) and makes the status 0x83 when it contains `Voluntary exit...`, else 0x7f.
/// 7. The dummy roots are freed, the renderer's chunk memory is freed, and the word at
///    +0x33c of the node of every model element in use of the current dictionary is
///    cleared.
pub fn sequence_render<O: Outside + ?Sized>(o: &mut O, m: &mut Mem) -> i32 {
    let (dc, ren) = (SI_DEFAULT_CONTROL.0, SI_REN.0);
    m[SI_PREVIEW_SELECTION_CALL] = 0;
    if select_sub_region_window(o, m) as i16 == 0 {
        return 0x7f;
    }
    m.copy(ren, dc, 0x600);
    super::instance::set_register_inst_mat_arrays_bool(m, 1);
    m.set_u16(ren + ctl::CROP, 0);
    let mut status = o.call(m, SI_OPEN_RENDER_WINDOW, &[ren]) as i32;

    let show_controls = m[SI_SHW_CTL];
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        if (m[gn].kind as u32).wrapping_sub(2) <= 1 {
            let model: Ptr32<Model> = m[gn].data.cast();
            m[model].visible = (m[gn].wire_type != 0) as i16;
            if show_controls == 0 && (m[model].algo_type as u32).wrapping_sub(18) <= 11 {
                m[model].visible = 0;
            }
        }
        gn = m[gn].next;
    }

    let shown = |m: &Mem, gn: Ptr32<GrafNode>| (m[gn].wire_type != 0) as i16;
    let mut roots: Ptr32<Model> = Ptr32::NULL;
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        if m[gn].kind == 2 {
            chain_root(o, m, gn, &mut roots, shown);
        }
        gn = m[gn].next;
    }
    let mut lights = 0u32;
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        if is_light(m[gn].kind) {
            chain_light(m, gn, &mut lights);
        }
        gn = m[gn].next;
    }

    let list = scene_graf_list(m);
    let camera = m[m[SI_PROC_DATA]].camera;
    let (a, b) = (m[list].unk_10[1], m[list].unk_10[3]);
    let answer = o.call(m, SI_IMAGE, &[camera.0, a, b, lights, roots.0]) as i32;
    if answer != 0 {
        status = answer;
    }
    o.call(m, SI_CLOSE_RENDER_WINDOW, &[ren]);
    // one byte of the message against the empty text
    if m.u8(SI_REN_MESSAGE.0) != m.u8(EMPTY_86A2B0D) {
        if m[SI_STANDALONE_RENDERER] == 0 {
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[SI_REN_MESSAGE.0, 3]);
        }
        // strstr(SI_renMessage, VOLUNTARY_EXIT)
        let (message, word) = (m.cstr(SI_REN_MESSAGE.0), m.cstr(VOLUNTARY_EXIT));
        let stopped = word.is_empty() || message.windows(word.len()).any(|w| w == &word[..]);
        status = if stopped { 0x83 } else { 0x7f };
    }
    free_dummies(m, roots);
    let chunk = m[SI_REN_CHUNK];
    o.call(m, SI_CHUNK_FREE_ALL, &[chunk.0]);

    let chapter = m[current_dict(m)].lists.add(m[SI_CID].model);
    let mut i = 0;
    while i < m[chapter].nb_elems {
        let elem = m[chapter].elems.add(i);
        if !elem.is_null() && m[elem].status == 0 {
            let model: Ptr32<Model> = m[elem].data.cast();
            if !model.is_null() {
                m[model].unk_33c = Ptr32::NULL;
            }
        }
        i += 1;
    }
    status
}
