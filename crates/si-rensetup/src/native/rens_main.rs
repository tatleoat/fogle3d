// SPDX-License-Identifier: MIT
//! `rens_main.c` on the memory image: the front end of the mental ray translator
//! (finding 0023, section 43; section 18 for the sequence render).
//!
//! `SI_prepareRendering`, `SI_sequenceRenderMental` and `SI_mentalPreview` are ported with
//! the original's control flow. What is the display or the environment (the status line,
//! `usleep`, `getenv`, the highlighted-window and sub-region questions, the tracer) is an
//! [`Outside`] call by address, in the original's order and with its arguments.
//!
//! **Compared with the emulated original**: the three routines were run against the
//! original stepped through in the emulation harness (`re/tools/si-rensetup/emu_gen_pass6.py`
//! for the sequence render, `re/tools/left2/emu_gen_mental.py` for the other two;
//! `rensetup-emucheck`).

use super::globals::*;
use super::outside::*;
use super::rec::ctl;
use super::{current_dict, instance, rnd_dial};
use si_mem::Mem;
use si_x87::x87::{Round, F80};

/// The size of the record of render parameters the routine keeps in its frame (the words
/// it reads: +0, +4 dictionaries, +0x4a0, +0x610 and +0x61c blocks, +0x60e a short).
const PARMS: u32 = 0x620;

/// Renders a sequence through the mental ray translator [soft 0x082c5ddc SI_sequenceRenderMental]: `SI_prepareRendering (0x082c57e4)` fills a record of render
/// parameters (and a render-dictionary element and a float); when it answers 0x7f that is
/// the answer. Else the tracer runs (`SI_mentalRunTracer (0x0842a8a0)`, its answer is the
/// routine's), the translator is cleaned up (`SI_cleanupTranslator (0x08581c78)`), the
/// three blocks at +0x61c, +0x4a0 and +0x610 of the record are freed and zeroed where set,
/// `SI_assignMaxcoc (0x082cb734)` gets the short at +0x60e and the float (through the
/// register), the render dictionary is deleted
/// (`SI_deleteRenderDictionary (0x082c56e0)`: the words at +4 and +0, the element, 1) and
/// the record's first block (the dictionary) is freed and zeroed where set.
pub fn sequence_render_mental<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, preview: i16) -> u32 {
    let elem = m.local(4);
    let float = m.local(4);
    let parms = m.local(PARMS);
    let r = o.call(m, SI_PREPARE_RENDERING, &[parms, elem, float, preview as i32 as u32]);
    let out = if r == 0x7f {
        0x7f
    } else {
        let status = o.call(m, SI_MENTAL_RUN_TRACER, &[parms, 0]);
        o.call(m, SI_CLEANUP_TRANSLATOR, &[parms]);
        for off in [0x61c, 0x4a0, 0x610] {
            let p = m.u32(parms + off);
            if p != 0 {
                m.free(p);
                m.set_u32(parms + off, 0);
            }
        }
        let f = F80::from_f32(m.f32(float)).to_f32().to_bits();
        let maxcoc = m.i16(parms + 0x60e) as i32 as u32;
        o.call(m, SI_ASSIGN_MAXCOC, &[maxcoc, f]);
        let (a, b, e) = (m.u32(parms + 4), m.u32(parms), m.u32(elem));
        o.call(m, SI_DELETE_RENDER_DICTIONARY, &[a, b, e, 1]);
        let d = m.u32(parms);
        if d != 0 {
            m.free(d);
            m.set_u32(parms, 0);
        }
        status
    };
    m.release(PARMS);
    m.release(4);
    m.release(4);
    out
}

/// Offsets into the record of render parameters (`SI_renderExecuteParms`, 0x674 bytes)
/// that the two front-end routines write or read; the 0x600 bytes at +0x0c are a copy of
/// `SI_defaultControl`, so an offset of a control is that offset plus 0x0c.
pub mod xp {
    pub const SIZE: u32 = 0x674;
    pub const REN_DICT: u32 = 0x000;
    pub const SCENE_DICT: u32 = 0x004;
    pub const CONTROL: u32 = 0x00c;
    /// 16 bits: control +0xfa, the mental ray output kind (9, or 10 with the second output).
    pub const OUTPUT_KIND: u32 = 0x106;
    /// 32 bits, 0: control +0x17c (the name `tempnam` made, in the preview).
    pub const TEMP_NAME: u32 = 0x188;
    pub const W18C: u32 = 0x18c;
    pub const W190: u32 = 0x190;
    /// 16 bits: control +0x178, display on.
    pub const DISPLAY: u32 = 0x184;
    /// 16 bits each: control +0x174 and +0x176, the picture window's position.
    pub const WINDOW_X: u32 = 0x180;
    pub const WINDOW_Y: u32 = 0x182;
    /// 16 bits: control +0x13c (the preview clears it).
    pub const W148: u32 = 0x148;
    /// 16 bits each: the frame range of the preview (control +0x1f4 to +0x1f8 area).
    pub const FRAME_A: u32 = 0x200;
    pub const FRAME_B: u32 = 0x202;
    pub const FRAME_C: u32 = 0x204;
    /// 16 bits: control +0x224, the crop switch.
    pub const CROP: u32 = 0x230;
    /// 16 bits: control +0x226, the switch that asks for the box before a mental ray render.
    pub const CROP_ASK: u32 = 0x232;
    /// 16 bits each: control +0x23c and +0x23e, the cropped picture's size.
    pub const CROP_WIDTH: u32 = 0x248;
    pub const CROP_HEIGHT: u32 = 0x24a;
    pub const W2C4: u32 = 0x2c4;
    pub const W2C8: u32 = 0x2c8;
    /// 16 bits: control +0x3d8, output goes to a file.
    pub const TO_FILE: u32 = 0x3e4;
    /// 16 bits: control +0x3dc, contour rendering.
    pub const CONTOUR: u32 = 0x3e8;
    pub const W3EA: u32 = 0x3ea;
    pub const W3EC: u32 = 0x3ec;
    /// 16 bits: control +0x3e2, copied to +0x618 as a signed number.
    pub const W3EE: u32 = 0x3ee;
    /// 32 bits: control +0x400, BSP tree acceleration (1) or, in the preview, the switch
    /// at +0x40c; +0x410 is the second output's.
    pub const BSP: u32 = 0x40c;
    pub const BSP2: u32 = 0x410;
    pub const W434: u32 = 0x434;
    pub const W43A: u32 = 0x43a;
    pub const F43C: u32 = 0x43c;
    pub const F440: u32 = 0x440;
    pub const F444: u32 = 0x444;
    pub const F448: u32 = 0x448;
    pub const F44C: u32 = 0x44c;
    pub const F450: u32 = 0x450;
    pub const W454: u32 = 0x454;
    /// 32 bits: control +0x494, the output file's name (a `strdup`).
    pub const OUT_NAME: u32 = 0x4a0;
    pub const W4A4: u32 = 0x4a4;
    pub const W4A6: u32 = 0x4a6;
    /// 16 bits each: control +0x59a and +0x59c, the picture's size.
    pub const WIDTH: u32 = 0x5a6;
    pub const HEIGHT: u32 = 0x5a8;
    /// The 0x7c bytes of control +0x508 the preview copies over control +0x584.
    pub const PREVIEW_FROM: u32 = 0x514;
    pub const PREVIEW_TO: u32 = 0x590;
    pub const PREVIEW_LEN: u32 = 0x7c;
    pub const W5E4: u32 = 0x5e4;
    pub const W5BE: u32 = 0x5be;
    pub const W5C0: u32 = 0x5c0;
    /// 16 bits: the caller's mode word (1 in `SI_prepareRendering`; the preview's kind).
    pub const MODE: u32 = 0x60c;
    /// 16 bits: the caller's "live" switch.
    pub const LIVE: u32 = 0x60e;
    pub const CAMERAS: u32 = 0x610;
    pub const NB_CAMERAS: u32 = 0x614;
    pub const W618: u32 = 0x618;
    /// 32 bits: the scene's full name, a `strdup`.
    pub const SCENE_NAME: u32 = 0x61c;
}

/// A 32-bit float through the x87 stack: loaded and stored again, so that a signalling
/// NaN comes out quiet.
fn through_x87(bits: u32) -> u32 {
    F80::from_f32(f32::from_bits(bits)).to_f32().to_bits()
}

/// The current dictionary's address as the program works it out from `SI_procData`: the
/// index in the process record times 0x70 beyond `SI_elemDicts`.
fn scene_dict(m: &Mem) -> u32 {
    current_dict(m).0
}

/// Whether a picture of `w` x `h` fits the screen: both are at most the screen's size plus
/// one (16-bit signed values).
fn fits_screen(m: &Mem, w: i16, h: i16) -> u16 {
    let (xmax, ymax) = (m[SI_XMAXSCREEN] as i32, m[SI_YMAXSCREEN] as i32);
    (xmax + 1 >= w as i32 && ymax + 1 >= h as i32) as u16
}

/// The deletion every failure of the two routines ends in: the render dictionary is
/// deleted (`SI_deleteRenderDictionary (0x082c56e0)`: scene dictionary, render dictionary,
/// default material element, 1).
fn delete_dict<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, scene: u32, ren: u32, elem: u32) {
    o.call(m, SI_DELETE_RENDER_DICTIONARY, &[scene, ren, elem, 1]);
}

/// Sets up a mental ray render [soft 0x082c57e4 SI_prepareRendering]; `parms` is the
/// record of render parameters it fills, `elem_out` receives the default material's
/// element, `maxcoc` a float (`SI_clampMaxcoc`'s). Answers 0, 0x7e (a message was shown:
/// something is not as asked, the render goes on) or 0x7f (nothing is to be rendered; the
/// dictionary is already deleted and its block freed).
///
/// 1. The registration of instance material arrays is switched off, the user-defined
///    shaders' array relations are made, the parameters are cleared, the preview structure
///    emptied, a 0x70-byte block becomes the render dictionary
///    (`SI_createRenderDictionary`), the selective lights and the shader scan follow, and
///    the live switch, the mode (1) and a one-camera array are stored.
/// 2. Interactively (live 0) `SI_checkHighlightedWindow` must answer yes; the sub-region
///    box is asked for (`SI_doSubregion`) when the switches at control +0x226 and +0x22a
///    are on (with the crop switch cleared meanwhile and put back), live the box's pixel
///    size is worked out (`SI_updateSubregionRes`) when +0x226 is on. A "no" ends with the
///    dictionary deleted and 0x7f.
/// 3. The controls are copied into the record; the output kind is 9 or 10 by
///    `SI_getmi2Output`; the output file name is duplicated; "display on" is whether the
///    picture (the cropped one when +0x226 is on) fits the screen.
/// 4. The messages (each answers 0x7e, and, interactively, waits a second): too large to
///    display; the tracer's environment variable is not set (the render goes to a file);
///    contour images cannot be displayed; contour render with a BSP tree.
/// 5. With output to a file: display off, the name defaults to `out.mi`, a name that does
///    not end in `.mi` is replaced by `SI_generateMIPerFrameFileName`; no memory ends as in 2.
/// 6. `SI_clampMaxcoc`, the window position (the screen's size less the picture's width,
///    half of it, rounded down, less one; not below 0; the width is used for both axes)
///    and a copy of the scene's name.
pub fn prepare_rendering<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, parms: u32, elem_out: u32, maxcoc: u32, live: i16) -> u32 {
    let dc = SI_DEFAULT_CONTROL.0;
    let mut status = 0u32;
    instance::set_register_inst_mat_arrays_bool(m, 0);
    o.call(m, SI_MAKE_UDF_ARRAY_RELATIONS, &[]);
    o.call(m, INITIALISE_RENDER_PARMS, &[parms]);
    o.call(m, SI_INITIALIZE_PREVIEW_STRUCT, &[SI_PREV_STRUCT]);
    let block = m.si_malloc(0x70);
    m.set_u32(parms + xp::REN_DICT, block);
    let dict = scene_dict(m);
    let elem = o.call(m, SI_CREATE_RENDER_DICTIONARY, &[dict, block, 1, 1]);
    m.set_u32(elem_out, elem);
    let dict = scene_dict(m);
    m.set_u32(parms + xp::SCENE_DICT, dict);
    let ren = m.u32(parms + xp::REN_DICT);
    o.call(m, UPDATE_SELECTIVE_LIGHTS, &[ren]);
    o.call(m, SI_MENTAL_SHADER_SCAN, &[ren, dc, 1]);
    m.set_u16(parms + xp::LIVE, live as u16);
    m.set_u16(parms + xp::MODE, 1);
    let cameras = m.si_malloc(4);
    m.set_u32(parms + xp::CAMERAS, cameras);
    let camera = o.call(m, SI_GET_RENDER_CAMERA, &[0]);
    let cameras = m.u32(parms + xp::CAMERAS);
    m.set_u32(cameras, camera);
    m.set_u32(parms + xp::NB_CAMERAS, 1);

    // the end of every failure: the dictionary deleted, its block freed, 0x7f
    macro_rules! fail {
        () => {{
            let (scene, ren, elem) = (m.u32(parms + xp::SCENE_DICT), m.u32(parms + xp::REN_DICT), m.u32(elem_out));
            delete_dict(o, m, scene, ren, elem);
            let block = m.u32(parms + xp::REN_DICT);
            if block != 0 {
                m.free(block);
                m.set_u32(parms + xp::REN_DICT, 0);
            }
            return 0x7f;
        }};
    }
    let usleep_if_interactive = |o: &mut O, m: &mut Mem, live_word: u16| {
        if live_word == 0 {
            o.call(m, USLEEP, &[1_000_000]);
        }
    };

    if m.u16(parms + xp::LIVE) == 0 && o.call(m, SI_CHECK_HIGHLIGHTED_WINDOW, &[]) as u16 == 0 {
        fail!();
    }
    let crop = m.u16(dc + ctl::CROP);
    m.set_u16(dc + ctl::CROP, 0);
    if m.u16(parms + xp::LIVE) == 0 {
        if m.u16(dc + ctl::SUBREGION_226) != 0 && m.u16(dc + ctl::SUBREGION_22A) != 0 && o.call(m, SI_DO_SUBREGION, &[]) as u16 == 0 {
            m.set_u16(dc + ctl::CROP, crop);
            fail!();
        }
    } else if m.u16(dc + ctl::SUBREGION_226) != 0 {
        rnd_dial::update_subregion_res(m, dc);
    }

    m.copy(parms + xp::CONTROL, dc, 0x600);
    let w3ee = m.i16(parms + xp::W3EE) as i32 as u32;
    m.set_u32(parms + xp::W618, w3ee);
    let mi2 = o.call(m, SI_GETMI2_OUTPUT, &[]) as u16;
    m.set_u16(parms + xp::OUTPUT_KIND, if mi2 != 0 { 10 } else { 9 });
    let name = m.u32(dc + 0x494);
    if name != 0 {
        let copy = m.strdup(name);
        m.set_u32(parms + xp::OUT_NAME, copy);
    }
    m.set_u16(dc + ctl::CROP, crop);
    let (w, h) = if m.u16(parms + xp::CROP_ASK) == 0 {
        (m.i16(parms + xp::WIDTH), m.i16(parms + xp::HEIGHT))
    } else {
        (m.i16(parms + xp::CROP_WIDTH), m.i16(parms + xp::CROP_HEIGHT))
    };
    let fits = fits_screen(m, w, h);
    m.set_u16(parms + xp::DISPLAY, fits);

    if m.u16(parms + xp::LIVE) == 0
        && m.u16(parms + xp::DISPLAY) == 0
        && m.u16(parms + xp::TO_FILE) == 0
        && (m.u16(parms + xp::CONTOUR) == 0 || o.call(m, SI_GETMI2_OUTPUT, &[]) as u16 != 0)
    {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[PR_TOO_LARGE_TO_DISPLAY, 2]);
        o.call(m, USLEEP, &[1_000_000]);
        status = 0x7e;
    }
    let mi2 = o.call(m, SI_GETMI2_OUTPUT, &[]) as u16;
    if m.u16(parms + xp::TO_FILE) == 0 {
        let var = if mi2 == 0 { PR_TRACER } else { PR_TRACER2 };
        if o.call(m, GETENV, &[var]) == 0 {
            m.set_u16(parms + xp::TO_FILE, 1);
            m.set_u16(parms + xp::DISPLAY, 0);
            let text = if mi2 != 0 { PR_TRACER2_UNSET } else { PR_TRACER_UNSET };
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 2]);
            let l = m.u16(parms + xp::LIVE);
            usleep_if_interactive(o, m, l);
            status = 0x7e;
        }
    }
    let mut to_file = m.u16(parms + xp::TO_FILE);
    if m.u16(parms + xp::CONTOUR) != 0 && mi2 == 0 {
        if to_file == 0 {
            m.set_u16(parms + xp::DISPLAY, 0);
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[PR_CONTOUR_NO_DISPLAY, 2]);
            let l = m.u16(parms + xp::LIVE);
            usleep_if_interactive(o, m, l);
            status = 0x7e;
            to_file = m.u16(parms + xp::TO_FILE);
        }
        if to_file == 0 && m.u32(parms + xp::BSP) == 1 {
            m.set_u32(parms + xp::BSP, 0);
            o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[PR_CONTOUR_BSP, 2]);
            let l = m.u16(parms + xp::LIVE);
            usleep_if_interactive(o, m, l);
            status = 0x7e;
            to_file = m.u16(parms + xp::TO_FILE);
        }
    }
    if to_file != 0 {
        m.set_u16(parms + xp::DISPLAY, 0);
        if m.u32(parms + xp::OUT_NAME) == 0 {
            let copy = m.strdup(PR_OUT_MI);
            m.set_u32(parms + xp::OUT_NAME, copy);
            if copy == 0 {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[PR_MEMORY_ERROR_1, 3]);
                fail!();
            }
        }
        let name = m.u32(parms + xp::OUT_NAME);
        let end = name.wrapping_add(m.cstr(name).len() as u32);
        // the last three characters and the end, four bytes against ".mi" and its zero: for
        // a name shorter than three characters these lie before the name, as in the original
        let tail = end.wrapping_sub(3);
        let ends_mi = (0..4u32).all(|k| m.u8(tail.wrapping_add(k)) == m.u8(PR_DOT_MI + k));
        if !ends_mi {
            let new = o.call(m, SI_GENERATE_MI_PER_FRAME_FILE_NAME, &[name, 0, 0, 0xffff_ffff]);
            m.set_u32(parms + xp::OUT_NAME, new);
            if new == 0 {
                o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[PR_MEMORY_ERROR_2, 3]);
                fail!();
            }
        }
    }

    let live_word = m.i16(parms + xp::LIVE) as i32 as u32;
    o.call(m, SI_CLAMP_MAXCOC, &[live_word, maxcoc]);
    // the window position: (screen - (width - 1)) * 0.5 + 0.5 cut towards zero, less one
    let width = m.i16(parms + xp::WIDTH) as i32;
    for (axis, field) in [(m[SI_XMAXSCREEN], xp::WINDOW_X), (m[SI_YMAXSCREEN], xp::WINDOW_Y)] {
        let v = F80::from_i32(axis as i32 - (width - 1)) * 0.5f64 + 0.5f64;
        let p = v.to_i32(Round::Zero).wrapping_sub(1) as i16;
        m.set_u16(parms + field, if p < 0 { 0 } else { p as u16 });
    }
    let ren = m.u32(parms + xp::REN_DICT);
    let chapters = m.u32(ren + 4);
    let dsc = m[SI_CID].dsc as u32;
    let elems = m.u32(chapters.wrapping_add(dsc << 6).wrapping_add(8));
    let first = m.u32(elems);
    let full_name = m.u32(first + 0x24);
    let copy = m.strdup(full_name);
    m.set_u32(parms + xp::SCENE_NAME, copy);
    status
}

/// The mental ray preview of the editor's selection [soft 0x082c5ef0 SI_mentalPreview]
/// (`mode`: 1 all models, 2 the selected ones; `flags` bit 0 the second output's switches
/// and the preview's own sampling, bit 1 the light factors, bit 2 the frame step cleared).
///
/// 1. No highlighted window: two seconds' pause and nothing else. Else the custom
///    functions are told (event 0xe, state 0x10), the instance arrays and UDF relations are
///    made, the render dictionary is made on a block of the frame, the instance transforms,
///    selective lights and the shader scan follow, live is 0, a one-camera array is set.
/// 2. With `SI_defaultControl` +0x224 and +0x228 on, the box is asked for, the switch at
///    +0x226 being 0 meanwhile; a cancel ends (state 0x11). No model (`SI_checkAllModels`)
///    or no tracer variable ends with a message, the dictionary deleted and a pause.
/// 3. The controls are copied into the parameters and the preview's block over the
///    switches; the output is not to a file, the frame is the current one, there is no
///    contour, the display is on; the flags set the second output, the light factors and
///    the step as above. If the picture does not fit the screen: message, dictionary
///    deleted, state 0x11.
/// 4. Else a temporary picture name is made (`tempnam` in `$TMPDIR`, prefix `CE`), the
///    tracer is run and the translator cleaned up, the picture file is removed, the
///    blocks freed, the maximum coc given back, the dictionary deleted, the event queue
///    emptied, the preview's window position copied (when the window is open) and the
///    custom functions told (0xe, 0x11).
pub fn mental_preview<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, mode: i16, flags: u32) {
    if o.call(m, SI_CHECK_HIGHLIGHTED_WINDOW, &[]) as u16 == 0 {
        o.call(m, USLEEP, &[2_000_000]);
        return;
    }
    o.call(m, SI_CUS_CALL_CUSTOM_FUNCTIONS, &[0xe, 0x10]);
    instance::set_register_inst_mat_arrays_bool(m, 0);
    o.call(m, SI_MAKE_UDF_ARRAY_RELATIONS, &[]);
    // the frame: the parameters, the dictionary's block, the camera array, the float and
    // the word `qread` fills
    let parms = m.local(xp::SIZE);
    let block = m.local(0x70);
    let camera = m.local(4);
    let maxcoc = m.local(4);
    let event = m.local(4);
    mental_preview_body(o, m, mode, flags, parms, block, camera, maxcoc, event);
    m.release(4);
    m.release(4);
    m.release(4);
    m.release(0x70);
    m.release(xp::SIZE);
}

#[allow(clippy::too_many_arguments)]
fn mental_preview_body<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, mode: i16, flags: u32, parms: u32, block: u32, camera: u32, maxcoc: u32, event: u32) {
    let dc = SI_DEFAULT_CONTROL.0;
    o.call(m, INITIALISE_RENDER_PARMS, &[parms]);
    let dict = scene_dict(m);
    let elem = o.call(m, SI_CREATE_RENDER_DICTIONARY, &[dict, block, 1, 1]);
    let dict = scene_dict(m);
    m.set_u32(parms + xp::SCENE_DICT, dict);
    m.set_u32(parms + xp::REN_DICT, block);
    o.call(m, SI_UPDATE_INST_TRANSFORMS, &[dict, block]);
    let ren = m.u32(parms + xp::REN_DICT);
    o.call(m, UPDATE_SELECTIVE_LIGHTS, &[ren]);
    o.call(m, SI_MENTAL_SHADER_SCAN, &[ren, dc, (mode == 1) as u32]);
    m.set_u16(parms + xp::LIVE, 0);
    m.set_u16(parms + xp::MODE, mode as u16);
    let cam = o.call(m, SI_GET_RENDER_CAMERA, &[0]);
    m.set_u32(camera, cam);
    m.set_u32(parms + xp::CAMERAS, camera);
    m.set_u32(parms + xp::NB_CAMERAS, 1);
    // the scene's slots, looked up now and read at the end
    let ren = m.u32(parms + xp::REN_DICT);
    let dsc = m[SI_CID].dsc as u32;
    let chapters = m.u32(ren + 4);
    let scenes = m.u32(chapters.wrapping_add(dsc << 6).wrapping_add(8));
    let saved = m.u16(dc + ctl::SUBREGION_226);
    m.set_u16(dc + ctl::SUBREGION_226, 0);

    let finish = |o: &mut O, m: &mut Mem| {
        o.call(m, SI_CUS_CALL_CUSTOM_FUNCTIONS, &[0xe, 0x11]);
    };
    let delete = |o: &mut O, m: &mut Mem| {
        let (scene, ren) = (m.u32(parms + xp::SCENE_DICT), m.u32(parms + xp::REN_DICT));
        delete_dict(o, m, scene, ren, elem);
    };

    if m.u16(dc + ctl::CROP) != 0 && m.u16(dc + ctl::SUBREGION_228) != 0 && o.call(m, SI_DO_SUBREGION, &[]) as u16 == 0 {
        m.set_u16(dc + ctl::SUBREGION_226, saved);
        delete(o, m);
        finish(o, m);
        return;
    }
    let mode_word = m.i16(parms + xp::MODE);
    let ren = m.u32(parms + xp::REN_DICT);
    if o.call(m, SI_CHECK_ALL_MODELS, &[ren, mode_word as i32 as u32]) as u16 == 0 {
        let text = if mode_word == 2 { MP_NO_MODELS_SELECTED } else { MP_NO_MODELS };
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 2]);
        delete(o, m);
        o.call(m, USLEEP, &[2_000_000]);
        finish(o, m);
        return;
    }
    let mi2 = o.call(m, SI_GETMI2_OUTPUT, &[]) as u16;
    let var = if mi2 == 0 { MP_TRACER } else { MP_TRACER2 };
    if o.call(m, GETENV, &[var]) == 0 {
        let text = if mi2 != 0 { MP_TRACER2_UNSET } else { MP_TRACER_UNSET };
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[text, 3]);
        delete(o, m);
        o.call(m, USLEEP, &[1_000_000]);
        finish(o, m);
        return;
    }

    m.copy(parms + xp::CONTROL, dc, 0x600);
    m.copy(parms + xp::PREVIEW_TO, parms + xp::PREVIEW_FROM, xp::PREVIEW_LEN);
    m.set_u16(parms + xp::OUTPUT_KIND, if mi2 != 0 { 10 } else { 9 });
    m.set_u32(parms + xp::W18C, 0);
    m.set_u32(parms + xp::W190, 0);
    let frame = m[SI_FRAME_CURRENT] as u16;
    m.set_u16(parms + xp::FRAME_A, frame);
    m.set_u16(parms + xp::FRAME_B, frame);
    m.set_u16(parms + xp::FRAME_C, 1);
    m.set_u16(parms + xp::TO_FILE, 0);
    m.set_u32(parms + xp::OUT_NAME, 0);
    m.set_u16(parms + xp::CONTOUR, 0);
    m.set_u16(parms + xp::W3EA, 0);
    m.set_u16(parms + xp::W3EC, 0);
    m.set_u32(parms + xp::W434, 0);
    m.set_u16(parms + xp::W4A6, 0);
    m.set_u16(parms + xp::W148, 0);
    m.set_u16(parms + xp::DISPLAY, 1);
    if flags & 1 != 0 {
        m.set_u32(parms + if mi2 != 0 { xp::BSP2 } else { xp::BSP }, 1);
        m.set_u16(parms + xp::W4A4, 0);
        m.set_u32(parms + xp::W454, 1);
        m.set_u16(parms + xp::W43A, 1);
        m.set_u32(parms + xp::W2C8, 0xffff_fffe);
        m.set_u32(parms + xp::W2C4, 0);
        if m.u16(parms + xp::W5E4) != 0 {
            m.set_u16(parms + xp::W4A4, 1);
            let v = m.u16(parms + 0x5e6);
            m.set_u16(parms + xp::W43A, v);
            let v = m.u32(parms + 0x5f8);
            m.set_u32(parms + xp::W454, v);
            let v = through_x87(m.u32(parms + 0x5fc));
            m.set_u32(parms + xp::F44C, v);
            let v = through_x87(m.u32(parms + 0x600));
            m.set_u32(parms + xp::F450, v);
            let v = m.u32(parms + 0x604);
            m.set_u32(parms + xp::W2C8, v);
            let v = m.u32(parms + 0x608);
            m.set_u32(parms + xp::W2C4, v);
        }
    }
    if flags & 4 != 0 {
        m.set_u16(parms + xp::W3EE, 0);
    }
    if flags & 2 != 0 {
        // 0.2 (0x086564e4)
        let c = 0x3e4c_cccdu32;
        for f in [xp::F43C, xp::F440, xp::F444, xp::F448] {
            m.set_u32(parms + f, c);
        }
        if m.u16(parms + xp::W5E4) != 0 {
            let first = through_x87(m.u32(parms + 0x5e8));
            m.set_u32(parms + xp::F43C, first);
            let v = through_x87(m.u32(parms + 0x5ec));
            m.set_u32(parms + xp::F440, v);
            let v = through_x87(m.u32(parms + 0x5f0));
            m.set_u32(parms + xp::F444, v);
            m.set_u32(parms + xp::F448, first);
        }
    }
    let w3ee = m.i16(parms + xp::W3EE) as i32 as u32;
    m.set_u32(parms + xp::W618, w3ee);
    m.set_u16(dc + ctl::SUBREGION_226, saved);
    let (w, h) = if m.u16(parms + xp::CROP) != 0 {
        (m.i16(parms + xp::CROP_WIDTH), m.i16(parms + xp::CROP_HEIGHT))
    } else {
        (m.i16(parms + xp::WIDTH), m.i16(parms + xp::HEIGHT))
    };
    let fits = fits_screen(m, w, h);
    m.set_u16(parms + xp::DISPLAY, fits);
    if fits == 0 {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[MP_TOO_LARGE, 3]);
        delete(o, m);
        finish(o, m);
        return;
    }
    if m.u16(parms + xp::PREVIEW_TO) == 0 {
        for k in 1..=5 {
            m.set_u16(parms + xp::PREVIEW_TO + 2 * k, 0);
        }
    }
    let dir = o.call(m, GETENV, &[MP_TMPDIR]);
    let temp = o.call(m, TEMPNAM, &[dir, MP_TEMP_PREFIX]);
    m.set_u32(parms + xp::TEMP_NAME, temp);
    o.call(m, SI_CLAMP_MAXCOC, &[0, maxcoc]);
    let first = m.u32(scenes);
    let full_name = m.u32(first + 0x24);
    let copy = m.strdup(full_name);
    m.set_u32(parms + xp::SCENE_NAME, copy);
    o.call(m, SI_MENTAL_RUN_TRACER, &[parms, 1]);
    o.call(m, SI_CLEANUP_TRANSLATOR, &[parms]);
    let temp = m.u32(parms + xp::TEMP_NAME);
    let first_frame = m.i16(dc + ctl::FRAME_FIRST) as i32 as u32;
    let field = m.i16(parms + xp::W148) as i32 as u32;
    let picture = o.call(m, SI_BUILD_PIC_FILE_NAME, &[temp, first_frame, field, 1, 1]);
    o.call(m, UNLINK, &[picture]);
    let name = m.u32(parms + xp::SCENE_NAME);
    if name != 0 {
        m.free(name);
        m.set_u32(parms + xp::SCENE_NAME, 0);
    }
    let temp = m.u32(parms + xp::TEMP_NAME);
    if temp != 0 {
        m.free(temp);
        m.set_u32(parms + xp::TEMP_NAME, 0);
    }
    if picture != 0 {
        m.free(picture);
    }
    let f = through_x87(m.u32(maxcoc));
    o.call(m, SI_ASSIGN_MAXCOC, &[0, f]);
    delete(o, m);
    while o.call(m, QTEST, &[]) != 0 {
        o.call(m, QREAD, &[event]);
    }
    if m.u16(dc + 0x534) != 0 {
        let x = m.u16(parms + xp::W5BE);
        m.set_u16(dc + 0x536, x);
        let y = m.u16(parms + xp::W5C0);
        m.set_u16(dc + 0x538, y);
    }
    finish(o, m);
}
