// SPDX-License-Identifier: MIT
//! `matter.c`: the preview command of the Matter module, the routine that turns the
//! sub-region's fractions into pixels, and the start-up state of the render controls
//! (six routines: what `SI_matterInit (0x08291bec)` leaves in `SI_defaultControl`). The
//! other 48 functions of the file are the Matter module's dialogues and menus.

use super::globals::*;
use super::outside::*;
use super::rec::ctl;
use super::{chain_light, chain_root, free_dummies, is_light, scene_graf_list};
use si_mem::{Mem, Ptr32};
use si_types::model::Model;
use si_x87::x87::{Round, F80};

/// A float loaded into the unit and stored again.
fn through(v: f32) -> f32 {
    F80::from_f32(v).to_f32()
}

/// Turns the sub-region of a set of render controls from fractions into pixels and works
/// out the cropped picture's size [soft 0x08292608 SI_updateSubregionValues].
///
/// With w and h the picture's width and height (16-bit, signed): left = w × left + 0.5,
/// right = w × right + 0.5, top = h × top + 0.5, bottom = h × bottom + 0.5, each formed
/// in the register, cut towards zero there (`frndint` rounding up for a negative value,
/// down otherwise) and stored back as a float **in place of the fraction**. The cropped
/// width is (w − right) − left and the cropped height (h − top) − bottom, cut towards
/// zero to 32 bits of which the low 16 are kept. When either is below 1 the status line
/// says ` Current image resolution is (%d x %d). Using (%d x %d).` and each is raised to 1.
///
/// The routine is not idempotent: a second call takes the pixel counts for fractions.
pub fn update_subregion_values<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, controls: u32) {
    let c = controls;
    let cut = |v: F80| v.round_int(if v.lt(F80::ZERO) { Round::Up } else { Round::Down }).to_f32();
    let width = |m: &Mem| F80::from_i32(m.i16(c + ctl::WIDTH) as i32);
    let height = |m: &Mem| F80::from_i32(m.i16(c + ctl::HEIGHT) as i32);

    let left = cut(width(m) * m.f32(c + ctl::CROP_LEFT) + 0.5f32);
    let right = width(m) * m.f32(c + ctl::CROP_RIGHT) + 0.5f32;
    m.set_f32(c + ctl::CROP_LEFT, left);
    let right = cut(right);
    let top = height(m) * m.f32(c + ctl::CROP_TOP) + 0.5f32;
    m.set_f32(c + ctl::CROP_RIGHT, right);
    let top = cut(top);
    let bottom = height(m) * m.f32(c + ctl::CROP_BOTTOM) + 0.5f32;
    m.set_f32(c + ctl::CROP_TOP, top);
    let bottom = cut(bottom);

    let w = (width(m) - m.f32(c + ctl::CROP_RIGHT)) - m.f32(c + ctl::CROP_LEFT);
    m.set_f32(c + ctl::CROP_BOTTOM, bottom);
    let w = w.to_i32(Round::Zero) as i16;
    let h = (height(m) - m.f32(c + ctl::CROP_TOP)) - bottom;
    m.set_i16(c + ctl::CROP_WIDTH, w);
    let h = h.to_i32(Round::Zero) as i16;
    m.set_i16(c + ctl::CROP_HEIGHT, h);
    if w <= 0 || h <= 0 {
        let (w, h) = (if w > 0 { w } else { 1 }, if h > 0 { h } else { 1 });
        // sprintf(RESOLUTION_FORMAT, ...) into a local of 80 bytes
        let text = format!(" Current image resolution is ({} x {}). Using ({} x {}).", m.i16(c + ctl::CROP_WIDTH), m.i16(c + ctl::CROP_HEIGHT), w, h);
        let buffer = m.local(0x50);
        m.put_cstr(buffer, text.as_bytes());
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buffer, 2]);
        m.set_i16(c + ctl::CROP_WIDTH, w);
        m.set_i16(c + ctl::CROP_HEIGHT, h);
        m.release(0x50);
    }
}

/// The preview of the Matter module [soft 0x082903bc SI_frameRender]: one frame of the
/// scene, the current one, rendered by the native renderer onto the screen.
///
/// `all`: render every model, not only the selected ones. `dialog`: the dialogue the
/// picture is shown in, or null for the full screen. `full`: keep the controls' own
/// sampling (else level 1, not adaptive).
///
/// 1. Material arrays of copied nodes are registered for freeing. The sub-region switch
///    +0x226 of `SI_defaultControl` is cleared; with the switches +0x224 and +0x228 set
///    the user is asked for the box, and a cancelled box ends the command.
/// 2. `SI_defaultControl` is copied to `SI_ren`, and inside `SI_ren` the preview's block
///    (31 words at +0x508) over the block at +0x584. Then the preview's own settings: the
///    adaptive threshold 0.1, 2 sub-samples, no picture file, display on, first and last
///    frame the current one, step 1, "sequence" 1, no fields, no depth file; and, when the
///    preview's block says so (+0x5b8), its own sampling settings in their place.
/// 3. The picture: in a dialogue at most 500 × 424; else, with the switch +0xfe, the
///    height = width ÷ the scene camera's ratio × the pixel ratio + 0.5, cut. With the
///    textures switch (+0x584) off, the five switches after it are cleared too. With crop
///    on, the sub-region's pixel values are worked out and the picture is the cropped one.
/// 4. A picture wider or higher than the screen ends the command with the status line
///    ` Image too large for frame buffer output: max. %d x %d.`
/// 5. Every model of the scene's graf list (kinds 2 and 3) gets its `visible` word:
///    (selected or `all`) and not hidden; a visible model whose algorithm is 18 to 29 is
///    made invisible again unless `SI_shwCtl` is set. With no visible model the status
///    line says ` No models currently selected.` and the command ends.
/// 6. The roots and the lights are chained ([`chain_root`], [`chain_light`]) in one walk
///    of the list.
/// 7. The window's position centres the picture on the screen; the picture's box is
///    stored; the frame is drawn (in a dialogue: a panel with the title `Preview`; else a
///    full-screen RGB window with a grey box).
/// 8. `SI_image (0x08257a70)`.
/// 9. The switch +0x226 is put back; the RGB window is closed; a message of the renderer
///    goes to the status line; the dummy roots are freed; the renderer's chunk memory is
///    freed.
///
/// The paths that end the command early (a cancelled box, a picture too large, no model)
/// leave the switch +0x226 cleared, and the first two leave out the custom effects' "end
/// of render" call as well; all as in the original.
pub fn frame_render<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, all: i16, dialog: u32, full: i16) {
    let (dc, ren) = (SI_DEFAULT_CONTROL.0, SI_REN.0);
    super::instance::set_register_inst_mat_arrays_bool(m, 1);
    let saved = m.u16(dc + ctl::SUBREGION_226);
    m.set_u16(dc + ctl::SUBREGION_226, 0);
    if m.u16(dc + ctl::CROP) != 0 && m.u16(dc + ctl::SUBREGION_228) != 0 && o.call(m, SI_DO_SUBREGION, &[]) as i16 == 0 {
        return;
    }

    m.copy(ren, dc, 0x600);
    m.copy(ren + ctl::SWITCHES, ren + ctl::PREVIEW_BLOCK, 0x7c);
    if full == 0 {
        m.set_u16(ren + ctl::ADAPTIVE, 0);
        m.set_u16(ren + ctl::LEVEL, 1);
    }
    m.set_f32(ren + ctl::ADAPTIVE_THRESHOLD, 0.1);
    let frame = m.u16(SI_FRAME_CURRENT.0);
    m.set_u16(ren + ctl::ADAPTIVE_SAMPLES, 2);
    m.set_u16(ren + ctl::PICTURE_FILE, 0);
    m.set_u16(ren + ctl::DISPLAY, 1);
    m.set_u16(ren + ctl::FRAME_FIRST, frame);
    m.set_u16(ren + ctl::FRAME_LAST, frame);
    m.set_u16(ren + ctl::FRAME_STEP, 1);
    m.set_u16(ren + ctl::SEQUENCE, 1);
    m.set_u16(ren + ctl::FIELDS, 0);
    if m.u16(ren + ctl::PREVIEW_OWN_SAMPLING) != 0 {
        if full == 0 {
            let adaptive = m.u16(ren + ctl::PREVIEW_ADAPTIVE);
            m.set_u16(ren + ctl::ADAPTIVE, adaptive);
            let level = m.u16(ren + ctl::PREVIEW_LEVEL);
            m.set_u16(ren + ctl::LEVEL, level);
        }
        let samples = m.u16(ren + ctl::PREVIEW_SAMPLES);
        let threshold = through(m.f32(ren + ctl::PREVIEW_THRESHOLD));
        m.set_f32(ren + ctl::ADAPTIVE_THRESHOLD, threshold);
        m.set_u16(ren + ctl::ADAPTIVE_SAMPLES, samples);
    }
    m.set_u16(ren + ctl::DEPTH_FILE, 0);

    if dialog != 0 {
        let w = m.i16(ren + ctl::WIDTH).min(500);
        m.set_i16(ren + ctl::WIDTH, w);
        let h = m.i16(ren + ctl::HEIGHT).min(0x1a8);
        m.set_i16(ren + ctl::HEIGHT, h);
    } else if m.u16(dc + ctl::HEIGHT_FROM_RATIO) != 0 {
        let camera = m[m[SI_PROC_DATA]].camera.0;
        let h = F80::from_i32(m.i16(ren + ctl::WIDTH) as i32) / m.f32(camera + super::rec::camera::RATIO) * m.f32(dc + ctl::PIXEL_RATIO) + 0.5f32;
        m.set_i16(ren + ctl::HEIGHT, h.to_i32(Round::Zero) as i16);
    }
    if m.u16(ren + ctl::SWITCHES) == 0 {
        for i in 1..6 {
            m.set_u16(ren + ctl::SWITCHES + 2 * i, 0);
        }
    }
    let (width, height) = if m.u16(ren + ctl::CROP) != 0 {
        update_subregion_values(o, m, ren);
        (m.i16(ren + ctl::CROP_WIDTH) as f32, m.i16(ren + ctl::CROP_HEIGHT) as f32)
    } else {
        (m.i16(ren + ctl::WIDTH) as f32, m.i16(ren + ctl::HEIGHT) as f32)
    };

    // the screen's size against the picture's, as floats
    let screen_x = |m: &Mem| m[SI_XMAXSCREEN] as i32 + 1;
    let screen_y = |m: &Mem| m[SI_YMAXSCREEN] as i32 + 1;
    if F80::from_i32(screen_x(m)).lt(F80::from_f32(width)) || F80::from_i32(screen_y(m)).lt(F80::from_f32(height)) {
        // sprintf(TOO_LARGE_FORMAT, ...) into a local of 80 bytes
        let text = format!(" Image too large for frame buffer output: max. {} x {}.\n", screen_x(m), screen_y(m));
        let buffer = m.local(0x50);
        m.put_cstr(buffer, text.as_bytes());
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[buffer, 2]);
        m.release(0x50);
        return;
    }
    o.call(m, SI_CUS_CALL_CUSTOM_FUNCTIONS, &[0xe, 0x10]);

    // a graf node counts when it is selected (or everything is asked for) and not hidden
    let counts = move |m: &Mem, gn: Ptr32<si_types::scene::GrafNode>| ((all != 0 || m[gn].selection != 0) && m[gn].wire_type != 0) as i16;
    let show_controls = m[SI_SHW_CTL];
    let mut any = false;
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        if (m[gn].kind as u32).wrapping_sub(2) <= 1 {
            let model: Ptr32<Model> = m[gn].data.cast();
            let visible = counts(m, gn);
            m[model].visible = visible;
            if visible != 0 {
                if show_controls == 0 && (m[model].algo_type as u32).wrapping_sub(18) <= 11 {
                    m[model].visible = 0;
                }
                any = true;
            }
        }
        gn = m[gn].next;
    }
    if !any {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[NO_MODELS, 0]);
        o.call(m, SI_CUS_CALL_CUSTOM_FUNCTIONS, &[0xe, 0x11]);
        return;
    }

    let mut roots: Ptr32<Model> = Ptr32::NULL;
    let mut lights = 0u32;
    let mut gn = m[scene_graf_list(m)].first;
    while !gn.is_null() {
        let kind = m[gn].kind;
        if kind == 2 {
            chain_root(o, m, gn, &mut roots, counts);
        } else if is_light(kind) {
            chain_light(m, gn, &mut lights);
        }
        gn = m[gn].next;
    }

    // the picture in the middle of the screen
    let centre = |screen: i32, size: f32| {
        let v = ((F80::from_i32(screen) - size) * 0.5f32 + 0.5f32).to_i32(Round::Zero).wrapping_sub(1) as i16;
        if v < 0 { 0 } else { v }
    };
    let x = centre(screen_x(m), width);
    m.set_i16(ren + ctl::WINDOW_X, x);
    let y = centre(screen_y(m), height);
    m.set_i16(ren + ctl::WINDOW_Y, y);
    let (w, h) = (m.u16(ren + ctl::WIDTH), m.u16(ren + ctl::HEIGHT));
    m.set_u16(ren + ctl::BOX_X0, 0);
    m.set_u16(ren + ctl::BOX_X1, w.wrapping_sub(1));
    m.set_u16(ren + ctl::BOX_Y0, 0);
    m.set_u16(ren + ctl::BOX_Y1, h.wrapping_sub(1));

    // 16-bit words of the controls as the sign-extended numbers the drawing calls take
    let word = |m: &Mem, offset: u32| m.i16(ren + offset) as i32 as u32;
    let (x, y) = (word(m, ctl::WINDOW_X), word(m, ctl::WINDOW_Y));
    if dialog != 0 {
        let (x1, y1) = (word(m, ctl::BOX_X1), word(m, ctl::BOX_Y1));
        let light = |m: &Mem| [m[SI_RGB_DIAL_LIGHT_R] as i32 as u32, m[SI_RGB_DIAL_LIGHT_G] as i32 as u32, m[SI_RGB_DIAL_LIGHT_B] as i32 as u32];
        let dark = |m: &Mem| [m[SI_RGB_DIAL_DARK_R] as i32 as u32, m[SI_RGB_DIAL_DARK_G] as i32 as u32, m[SI_RGB_DIAL_DARK_B] as i32 as u32];
        let frame = [x.wrapping_sub(5), y.wrapping_sub(5), x1.wrapping_add(x).wrapping_add(5), y1.wrapping_add(y).wrapping_add(0x1d)];
        let colour = light(m);
        o.call(m, RGBCOLOR, &colour);
        let filled_box = m[SI_SBOXFI];
        o.call(m, filled_box, &frame);
        let colour = dark(m);
        o.call(m, RGBCOLOR, &colour);
        o.call(m, SBOXI, &frame);
        o.call(m, SBOXI, &[x.wrapping_sub(1), y.wrapping_sub(1), x1.wrapping_add(x).wrapping_add(1), y1.wrapping_add(y).wrapping_add(1)]);
        let filled_box = m[SI_SBOXFI];
        o.call(m, filled_box, &[x.wrapping_sub(1), y1.wrapping_add(y).wrapping_add(5), x1.wrapping_add(x).wrapping_add(1), y1.wrapping_add(y).wrapping_add(0x19)]);
        let colour = light(m);
        o.call(m, RGBCOLOR, &colour);
        let descender = o.call(m, GETDESCENDER, &[]);
        let title_width = o.call(m, STRWIDTH, &[PREVIEW_WIDTH]);
        let indent = (F80::from_i32((m.i16(ren + ctl::WIDTH) as i32).wrapping_sub(title_width as i32)) * 0.5f32 + 0.5f32).to_i32(Round::Zero);
        let text_y = y.wrapping_add(y1).wrapping_add(m[SI_DLEADING] as i32 as u32).wrapping_add(7).wrapping_add(descender);
        let text_x = x.wrapping_add(indent as i16 as i32 as u32).wrapping_sub(1);
        o.call(m, CMOV2I, &[text_x, text_y]);
        o.call(m, CHARSTR, &[PREVIEW_TITLE]);
    } else {
        o.call(m, SI_RGBWINDOW, &[]);
        o.call(m, RGBCOLOR, &[0x64, 0x64, 0x64]);
        if m.u16(ren + ctl::CROP) != 0 {
            let (cw, ch) = (word(m, ctl::CROP_WIDTH), word(m, ctl::CROP_HEIGHT));
            o.call(m, RECTI, &[x.wrapping_sub(1), y.wrapping_sub(1), x.wrapping_add(cw), ch.wrapping_add(y).wrapping_add(1)]);
        } else {
            let (pw, ph) = (word(m, ctl::WIDTH), word(m, ctl::HEIGHT));
            o.call(m, SBOXI, &[x.wrapping_sub(1), y.wrapping_sub(1), x.wrapping_add(pw), y.wrapping_add(ph)]);
        }
    }

    let list = scene_graf_list(m);
    let camera = m[m[SI_PROC_DATA]].camera;
    let (a, b) = (m[list].unk_10[1], m[list].unk_10[3]);
    o.call(m, SI_IMAGE, &[camera.0, a, b, lights, roots.0]);
    m.set_u16(dc + ctl::SUBREGION_226, saved);
    if dialog == 0 {
        o.call(m, SI_MENUWINDOW, &[0]);
    }
    // one byte of the message against the empty text
    if m.u8(SI_REN_MESSAGE.0) != m.u8(EMPTY_86A10EF) {
        o.call(m, SI_STATUS_DISPLAY_MESSAGE, &[SI_REN_MESSAGE.0, 3]);
    }
    free_dummies(m, roots);
    let chunk = m[SI_REN_CHUNK];
    o.call(m, SI_CHUNK_FREE_ALL, &[chunk.0]);
    o.call(m, SI_CUS_CALL_CUSTOM_FUNCTIONS, &[0xe, 0x11]);
}

// ---------------------------------------------------------------- the controls at start-up

/// A float constant by its bits: what `flds` of a constant followed by `fstps` stores.
fn fl(m: &mut Mem, at: u32, bits: u32) {
    m.set_u32(at, bits);
}

/// The fade's start-up state [soft 0x08291298 SI_fadeInit] (`SI_Fade`, the record at
/// +0x100 of the controls): start 20, end 100, the five words at +0x18 … +0x28 and the
/// word at +0x30 cleared (colour and transparency), switched off.
pub fn fade_init(m: &mut Mem, fade: u32) {
    fl(m, fade + 0x04, 0x41a0_0000);
    fl(m, fade + 0x0c, 0x42c8_0000);
    for o in [0x18, 0x1c, 0x20, 0x24, 0x28, 0x30] {
        m.set_u32(fade + o, 0);
    }
    m.set_u16(fade, 0);
}

/// The fog's start-up state [soft 0x0829122c SI_fogInit] (`SI_Fog`, the record at +0x25c
/// of the controls): density 0.2, base −1, thickness 5, start 20, end 10000, the four
/// floats −10, 10, −10, 10 at +0x18, the colour 0.7 three times, and the two switches at
/// +0x00 and +0x40 off.
pub fn fog_init(m: &mut Mem, fog: u32) {
    fl(m, fog + 0x04, 0x3e4c_cccd);
    fl(m, fog + 0x08, 0xbf80_0000);
    fl(m, fog + 0x0c, 0x40a0_0000);
    fl(m, fog + 0x10, 0x41a0_0000);
    fl(m, fog + 0x14, 0x461c_4000);
    fl(m, fog + 0x18, 0xc120_0000);
    fl(m, fog + 0x1c, 0x4120_0000);
    fl(m, fog + 0x20, 0xc120_0000);
    fl(m, fog + 0x24, 0x4120_0000);
    fl(m, fog + 0x28, 0x3f33_3333);
    fl(m, fog + 0x30, 0x3f33_3333);
    fl(m, fog + 0x2c, 0x3f33_3333);
    m.set_u16(fog, 0);
    m.set_u16(fog + 0x40, 0);
}

/// [soft 0x08291bd4 SI_matterFadeInit]: `SI_fadeInit` on the fade of `SI_defaultControl`.
pub fn matter_fade_init(m: &mut Mem) {
    fade_init(m, SI_DEFAULT_CONTROL.0 + 0x100);
}

/// [soft 0x08296200 fogInit]: `SI_fogInit` on the fog of `SI_defaultControl`.
pub fn fog_init_default(m: &mut Mem) {
    fog_init(m, SI_DEFAULT_CONTROL.0 + 0x25c);
}

/// The start-up state of the mental ray part of a set of render controls
/// [soft 0x08291808 SI_mentalRayInit] (`SI_MentalRay`, the record at +0x2a0 of the
/// controls; the names of the members are those of the set-up file's keywords, finding
/// 0006).
///
/// Six floats at +0x00 (50, 2.8, 1, 0.7, 10, 100); samples 1 and −1, threads 1, nice 10,
/// tracer and shadow 1, faces 3; eight bytes of the text `default` at +0x34; 0.01 and
/// 100000 at +0x130; the contour's line width 0.5 and depth 0; the grid size and the gamma
/// 1; the four thresholds 0.1; the filter 2 × 2 of type 1; the two refraction numbers and
/// the three transmit numbers 1; the ray depths 1, 2, 2; memory 10; the tree's depth 40
/// and size 6, twice; the output file's name a copy of `out.mi`; antialias on; the four
/// time contrasts 0.1; the photon map's file a copy of the empty text; its filter 1.1 and
/// the three accuracies 100; everything else named in the routine cleared.
///
/// Not set by the routine, and so left as they are: +0x30, +0x3c … +0x12c, +0x138, +0x13a,
/// +0x1c4, +0x1ca.
pub fn mental_ray_init(m: &mut Mem, mr: u32) {
    let words = |m: &mut Mem, offsets: &[u32], v: u32| {
        for o in offsets {
            m.set_u32(mr + o, v);
        }
    };
    let halves = |m: &mut Mem, offsets: &[u32], v: u16| {
        for o in offsets {
            m.set_u16(mr + o, v);
        }
    };
    const ONE: u32 = 0x3f80_0000;
    const TENTH: u32 = 0x3dcc_cccd;
    fl(m, mr, 0x4248_0000);
    fl(m, mr + 0x04, 0x4033_3333);
    words(m, &[0x08], ONE);
    fl(m, mr + 0x0c, 0x3f33_3333);
    fl(m, mr + 0x10, 0x4120_0000);
    fl(m, mr + 0x14, 0x42c8_0000);
    words(m, &[0x18], 1);
    words(m, &[0x1c], 0xffff_ffff);
    words(m, &[0x20], 1);
    halves(m, &[0x2a], 1);
    halves(m, &[0x142], 0);
    words(m, &[0x24], 10);
    halves(m, &[0x28], 1);
    words(m, &[0x2c], 3);
    // eight bytes of the text at 0x086a10fb, as two words
    let (a, b) = (m.u32(MR_DEFAULT_TEXT), m.u32(MR_DEFAULT_TEXT + 4));
    m.set_u32(mr + 0x34, a);
    fl(m, mr + 0x130, 0x3c23_d70a);
    fl(m, mr + 0x134, 0x47c3_5000);
    fl(m, mr + 0x204, 0x3f00_0000);
    m.set_u32(mr + 0x38, b);
    halves(m, &[0x13c], 0);
    words(m, &[0x208], 0);
    words(m, &[0x21c, 0x184], ONE);
    words(m, &[0x190, 0x194, 0x198, 0x19c], TENTH);
    words(m, &[0x1a0, 0x1a4], 0x4000_0000);
    words(m, &[0x1b0, 0x1b4], ONE);
    halves(m, &[0x13e, 0x140], 0);
    words(m, &[0x154], 1);
    words(m, &[0x158, 0x15c], 2);
    words(m, &[0x168, 0x16c], 0);
    words(m, &[0x170], 10);
    words(m, &[0x174], 0x28);
    words(m, &[0x178], 6);
    words(m, &[0x17c], 0x28);
    words(m, &[0x180], 6);
    words(m, &[0x160, 0x164], 1);
    words(m, &[0x188], 0);
    halves(m, &[0x18c], 0);
    halves(m, &[0x18e], 1);
    words(m, &[0x1a8], 1);
    words(m, &[0x1ac], 0);
    words(m, &[0x1c0, 0x1bc, 0x1b8], ONE);
    words(m, &[0x1cc, 0x1d0, 0x1d4, 0x1d8], 0);
    halves(m, &[0x1c8, 0x1dc, 0x1de], 0);
    words(m, &[0x1e0, 0x1e4, 0x1e8, 0x1ec, 0x1f0], 0);
    let name = m.strdup(MR_OUT_MI);
    m.set_u32(mr + 0x1f4, name);
    halves(m, &[0x1f8], 1);
    halves(m, &[0x1fa, 0x1fc], 0);
    halves(m, &[0x1fe], 1);
    words(m, &[0x200], 1);
    words(m, &[0x20c, 0x210, 0x214, 0x218], TENTH);
    words(m, &[0x144, 0x14c, 0x148, 0x150], 0);
    halves(m, &[0x220, 0x222, 0x224, 0x226, 0x228], 0);
    halves(m, &[0x22a], 1);
    halves(m, &[0x22c, 0x22e, 0x230, 0x232], 0);
    let file = m.strdup(EMPTY_86A110A);
    fl(m, mr + 0x254, 0x3f8c_cccd);
    words(m, &[0x258, 0x25c, 0x260, 0x264], 0);
    m.set_u32(mr + 0x234, file);
    halves(m, &[0x238], 0);
    words(m, &[0x23c, 0x240, 0x244], 0x64);
    words(m, &[0x248, 0x24c, 0x250], 2);
}

/// The start-up state of a set of render controls [soft 0x082912e4 SI_renControlInit]
/// (`SI_RenControl`, 0x600 bytes; `SI_matterInit` calls it on `SI_defaultControl`).
///
/// The mental ray part first ([`mental_ray_init`]). Then, among others: the ambience 0.3
/// three times; pixel ratio 1; the adaptive threshold 0.05; rendering type 4; the preview's
/// and the picture's size 500 × 424; the picture's box (0xff, 0x2f3, 0x13f, 0x2a9); ray
/// depth 1, triangle depth 10, tree depth 30; 2 adaptive sub-samples, adaptive on; the
/// output name a copy of `default` and the two scripts copies of the empty text; shutter
/// speed 1, least movement 5; the sub-region's four fractions 0.2; frames 1 to 100 by 1;
/// level 2; **display on and picture file on**; no fields; back-face culling on; no
/// motion blur; the crop and sub-region switches 0, 0, 1, 1; no tags; no dither (its
/// number 10); the preview's and the global switches on.
///
/// The preview's and the picture's window positions are (screen − size) ÷ 2 as C divides
/// (towards zero), from `SI_xmaxscreen` and `SI_ymaxscreen`. The preview's own sampling
/// block at +0x53c gets a 16-bit 0, the adaptive switch, 2 and the sub-samples as 32-bit
/// words, and the threshold at +0x558; then its eight words are copied to +0x5b8. Three of
/// them (+0x54c, +0x550, +0x554) and the upper half of the first are not set by the
/// routine at that point, so the copy carries what the record held there on entry; the
/// routine sets four of those words at +0x55c … +0x57c afterwards, in the block only.
/// Seven words of the mental ray part (+0x430 … +0x448) and two more (+0x2bc, +0x2b8) are
/// copied to +0x5dc … +0x5fc.
///
/// Not set by the routine, and so left as they are, among others: the fade (+0x100) and
/// the fog (+0x25c), which `SI_matterInit` sets with the two routines above; the sequence
/// word +0x9e; the window position +0x174, +0x176; +0x1f0; +0x1fa.
pub fn ren_control_init(m: &mut Mem, controls: u32) {
    let c = controls;
    let halves = |m: &mut Mem, offsets: &[u32], v: u16| {
        for o in offsets {
            m.set_u16(c + o, v);
        }
    };
    mental_ray_init(m, c + 0x2a0);
    for o in [0x1fc, 0x200, 0x204] {
        fl(m, c + o, 0x3e99_999a);
    }
    fl(m, c + 0x154, 0x3f80_0000);
    fl(m, c + 0x16c, 0x3d4c_cccd);
    halves(m, &[0xfa], 4);
    halves(m, &[0x51e], 0x1f4);
    halves(m, &[0x520], 0x1a8);
    halves(m, &[0x59a], 0x1f4);
    halves(m, &[0x59c], 0x1a8);
    halves(m, &[0xfe], 1);
    halves(m, &[0xfc], 7);
    halves(m, &[0x142], 0xff);
    halves(m, &[0x144], 0x2f3);
    halves(m, &[0x146], 0x13f);
    halves(m, &[0x148], 0x2a9);
    halves(m, &[0x1e4], 1);
    halves(m, &[0x1da], 10);
    halves(m, &[0x1dc], 0x1e);
    halves(m, &[0x15c], 2);
    halves(m, &[0x15a], 1);
    let name = m.strdup(RC_DEFAULT);
    m.set_u32(c + 0x17c, name);
    let before = m.strdup(EMPTY_86A10F9);
    m.set_u32(c + 0x180, before);
    let after = m.strdup(EMPTY_86A10FA);
    fl(m, c + 0x214, 0x3f80_0000);
    fl(m, c + 0x21c, 0x40a0_0000);
    for o in [0x22c, 0x230, 0x234, 0x238] {
        fl(m, c + o, 0x3e4c_cccd);
    }
    m.set_u32(c + 0x184, after);
    halves(m, &[0x1e6, 0x1ea, 0x1e8, 0x1ec], 0);
    halves(m, &[0x1f4], 1);
    halves(m, &[0x1f6], 0x64);
    halves(m, &[0x1f8], 1);
    halves(m, &[0x158], 2);
    halves(m, &[0x178, 0x17a], 1);
    halves(m, &[0x13c, 0x13e, 0x138], 0);
    halves(m, &[0x20c], 1);
    halves(m, &[0x20e, 0x210, 0x224, 0x226], 0);
    halves(m, &[0x228, 0x22a], 1);
    halves(m, &[0x23c, 0x23e, 0x240], 0);
    m.set_u32(c + 0x250, 0);
    m.set_u32(c + 0x254, 0);
    fl(m, c + 0x258, 0x4120_0000);
    halves(m, &[0x242, 0x244], 0);
    m.set_u32(c + 0x248, 0);
    halves(m, &[0x508, 0x50c, 0x50a, 0x50e, 0x510, 0x512], 1);
    halves(m, &[0x514, 0x534], 0);
    halves(m, &[0x51c, 0x522, 0x528, 0x52e, 0x52a, 0x52c, 0x524], 1);
    halves(m, &[0x526], 0);
    halves(m, &[0x584, 0x588, 0x586, 0x58a, 0x58c, 0x58e, 0x590], 1);
    halves(m, &[0x5b0], 0);
    halves(m, &[0x598, 0x59e, 0x5a4, 0x5aa, 0x5a6, 0x5a8, 0x5a0, 0x5a2], 1);
    // (screen - size) / 2, as C divides
    let half = |screen: i16, size: i16| ((screen as i32).wrapping_sub(size as i32) / 2) as i16;
    // the threshold is loaded here and stored after the third position
    let threshold = through(m.f32(c + 0x16c));
    let v = half(m[SI_XMAXSCREEN], m.i16(c + 0x51e));
    m.set_i16(c + 0x536, v);
    let v = half(m[SI_YMAXSCREEN], m.i16(c + 0x520));
    m.set_i16(c + 0x538, v);
    let v = half(m[SI_XMAXSCREEN], m.i16(c + 0x59a));
    m.set_i16(c + 0x5b2, v);
    m.set_f32(c + 0x558, threshold);
    let v = half(m[SI_YMAXSCREEN], m.i16(c + 0x59c));
    m.set_i16(c + 0x5b4, v);
    m.set_u16(c + 0x53c, 0);
    let adaptive = m.i16(c + 0x15a) as i32;
    m.set_i32(c + 0x540, adaptive);
    m.set_u32(c + 0x544, 2);
    let samples = m.i16(c + 0x15c) as i32;
    m.set_i32(c + 0x548, samples);
    m.copy(c + 0x5b8, c + 0x53c, 0x20);
    for (from, to) in [(0x430, 0x5dc), (0x434, 0x5e0), (0x438, 0x5e4)] {
        let v = through(m.f32(c + from));
        m.set_f32(c + to, v);
    }
    let v = through(m.f32(c + 0x43c));
    halves(m, &[0x5d8], 0);
    halves(m, &[0x5da], 1);
    m.set_f32(c + 0x5e8, v);
    let word = m.u32(c + 0x448);
    for (from, to) in [(0x440, 0x5f0), (0x444, 0x5f4)] {
        let v = through(m.f32(c + from));
        m.set_f32(c + to, v);
    }
    m.set_u32(c + 0x5ec, word);
    let word = m.u32(c + 0x2bc);
    m.set_u32(c + 0x5f8, word);
    let word = m.u32(c + 0x2b8);
    // 0.2, still in the register from the sub-region's fractions
    for o in [0x560, 0x564, 0x568, 0x56c] {
        fl(m, c + o, 0x3e4c_cccd);
    }
    fl(m, c + 0x574, 0x4000_0000);
    fl(m, c + 0x578, 0x4000_0000);
    m.set_u32(c + 0x5fc, word);
    halves(m, &[0x55c], 0);
    halves(m, &[0x55e], 1);
    m.set_u32(c + 0x570, 1);
    m.set_u32(c + 0x57c, 0xffff_fffe);
    m.set_u32(c + 0x580, 0);
}

// ---------------------------------------------------------------- the module's start-up

/// `SI_defaultLight` (0x08781e00, `.bss`, 0x240 bytes): the light `initData` copies into a
/// frame of a scene that has none (finding 0012, section 21).
pub const SI_DEFAULT_LIGHT: u32 = 0x0878_1e00;
/// The float `SI_matterInit` loads for the default light's position: 100.0 at 0x086551e8
/// (held against the binary by `re/tools/si-rensetup/check_strings.py`, [`RODATA`]).
pub const MATTER_INIT_HUNDRED: u32 = 0x0865_51e8;

/// `material` (0x0879abc0), the Matter module's work material, and `SI_defaultMaterial`
/// (0x0869dde0): their list link is +0x6e4.
const WORK_MATERIAL: u32 = 0x0879_abc0;
const DEFAULT_MATERIAL: u32 = 0x0869_dde0;
const MATERIAL_NEXT: u32 = 0x6e4;
/// `SI_grafLists` (0x08679498, a pointer to the graph lists, 0x24 bytes each) and
/// `SI_procData` (0x08679314, whose second word is the current graph list's index).
const GRAF_LISTS_AT: u32 = 0x0867_9498;
const PROC_DATA_AT: u32 = 0x0867_9314;

/// [soft 0x0828205c si_linkDefaultMaterial]: the work material's link takes the current
/// graph list's material list (+0x14), `SI_defaultMaterial`'s link becomes the work
/// material, and the list's head `SI_defaultMaterial` (listing). Held against the emulated
/// original on 18 sample scenes' images (finding 0018, section 20; finding 0023, section 23).
pub fn link_default_material(m: &mut Mem) {
    let gl = m.u32(GRAF_LISTS_AT).wrapping_add((m.i32(m.u32(PROC_DATA_AT) + 4) as u32).wrapping_mul(0x24));
    m.set_u32(WORK_MATERIAL + MATERIAL_NEXT, m.u32(gl + 0x14));
    m.set_u32(DEFAULT_MATERIAL + MATERIAL_NEXT, WORK_MATERIAL);
    let gl = m.u32(GRAF_LISTS_AT).wrapping_add((m.i32(m.u32(PROC_DATA_AT) + 4) as u32).wrapping_mul(0x24));
    m.set_u32(gl + 0x14, DEFAULT_MATERIAL);
}

/// `SI_renderGprops` (0x08782120), the render window's graphic properties; `SI_GPROPS`
/// (0x086fd720, 0x48 bytes, the GL layer's), and `si_initRenderGprops`' statics
/// `hwGlbGprops.428` (0x0879dba0), `_.tmp_2.429` (0x0879dbe8) and `first.427` (0x086a2dcc,
/// 1 in the binary).
pub const SI_RENDER_GPROPS: u32 = 0x0878_2120;
const SI_GPROPS: u32 = 0x086f_d720;
const HW_GLB_GPROPS: u32 = 0x0879_dba0;
const TMP_2_429: u32 = 0x0879_dbe8;
const FIRST_427: u32 = 0x086a_2dcc;

/// `si_initRenderGprops (0x082a5920)(properties)` (listing; unchecked): the first time,
/// `SI_GPROPS` copied into `hwGlbGprops.428` (18 words); then, while `first.427` is set: it
/// is cleared, `SI_GpropsInit(properties)` (0x085f15f0, the GL layer's: **not done**, its
/// fields are the GL layer's private state), +0x10 = 1 (0 with `SI_GL_RENDER_TXT_OFF` in
/// the environment; none is set here), +0x12 = 0, +0x08 = 3, +0x30 = `&hwGlbGprops.428`.
pub fn init_render_gprops(m: &mut Mem, props: u32) {
    if m.u32(TMP_2_429) == 0 {
        for k in 0..0x12 {
            m.set_u32(HW_GLB_GPROPS + 4 * k, m.u32(SI_GPROPS + 4 * k));
        }
        m.set_u32(TMP_2_429, 1);
    }
    if m.u32(FIRST_427) != 0 {
        m.set_u32(FIRST_427, 0);
        m.set_i16(props + 0x10, 1);
        m.set_i16(props + 0x12, 0);
        m.set_u32(props + 8, 3);
        m.set_u32(props + 0x30, HW_GLB_GPROPS);
    }
}

/// `SI_incrust` (0x0879cc00): the incrustation's switch (+0), its kind (+2), its text (+4),
/// the font (0x680 16-bit words from +8) and a word at +0xd08; `SI_softFont` (0x086794c8),
/// a pointer to `/Softimage.fnt` in the binary's data; `SI_resdir` (0x08781628).
pub const SI_INCRUST: u32 = 0x0879_cc00;
const SI_SOFT_FONT: u32 = 0x0867_94c8;
const SI_RESDIR: u32 = 0x0878_1628;

/// `incrustInit (0x082964b8)` (listing; unchecked): the switch 0, the kind 3, the text
/// `strdup("Hello World!")`, +0xd08 = 0; then the font file `<SI_resdir><SI_softFont>` read
/// with `fscanf(f, "%hx", …)` 0x680 times into the words from +8 (a read that fails stores
/// nothing, and every later one fails too). `read` is the file system: the file's bytes or
/// `None` when `fopen` fails. Where a pointer is null the original prints `(null)`.
pub fn incrust_init(m: &mut Mem, read: &dyn Fn(&[u8]) -> Option<Vec<u8>>) {
    m.set_i16(SI_INCRUST, 0);
    m.set_i16(SI_INCRUST + 2, 3);
    let text = b"Hello World!";
    let p = m.malloc(text.len() as u32 + 1);
    m.put_cstr(p, text);
    m.set_u32(SI_INCRUST + 4, p);
    m.set_i16(SI_INCRUST + 0xd08, 0);
    // wire4: `SI_softFont` is a pointer into the binary's data (the string "/Softimage.fnt"); this image has no such
    // data, so the start-up sets it (else the path below reads "<resdir>(null)"). `RENSETUP_NO_WIRE4=1` leaves it null.
    if m.u32(SI_SOFT_FONT) == 0 && std::env::var_os("RENSETUP_NO_WIRE4").is_none() {
        let name = b"/Softimage.fnt";
        let q = m.malloc(name.len() as u32 + 1);
        m.put_cstr(q, name);
        m.set_u32(SI_SOFT_FONT, q);
    }
    let s = |m: &Mem, at: u32| match m.u32(at) {
        0 => b"(null)".to_vec(),
        q => m.cstr(q),
    };
    let mut path = s(m, SI_RESDIR);
    path.extend_from_slice(&s(m, SI_SOFT_FONT));
    let Some(data) = read(&path) else { return };
    // `%hx`: white space skipped, an optional sign and `0x`, hex digits (the value kept to
    // 16 bits); no digit is a failed match, which every later call meets again
    let mut pos = 0usize;
    for k in 0..0x680u32 {
        while pos < data.len() && data[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        let mut neg = false;
        if pos < data.len() && (data[pos] == b'-' || data[pos] == b'+') {
            neg = data[pos] == b'-';
            pos += 1;
        }
        if pos + 1 < data.len() && data[pos] == b'0' && (data[pos + 1] | 0x20) == b'x' && data.get(pos + 2).is_some_and(|c| c.is_ascii_hexdigit()) {
            pos += 2;
        }
        let digits = pos;
        let mut v: u32 = 0;
        while pos < data.len() && data[pos].is_ascii_hexdigit() {
            v = v.wrapping_mul(16).wrapping_add((data[pos] as char).to_digit(16).unwrap_or(0));
            pos += 1;
        }
        if pos == digits {
            pos = start;
            break;
        }
        let v = if neg { v.wrapping_neg() } else { v };
        m.set_u16(SI_INCRUST + 8 + 2 * k, v as u16);
    }
    let _ = pos;
}

/// What `SI_matterInit (0x08291bec)` does at the program's start; unchecked as a whole
/// (its steps are ported routines, each cited where it stands, or written "unchecked").
/// Its steps, in the listing's order:
///
/// 1. `si_lightInitialize(&SI_defaultLight, 1)`: `si-elemio`'s port (a point light; the
///    type makes no call out).
/// 2. The float at 0x086551e8 (100.0) stored at +0x34, +0x38 and +0x3c of the light: its
///    position (`flds`, `fsts` twice, `fstps`: the bits pass unchanged).
/// 3. [`link_default_material`]: `SI_defaultMaterial` and the work material at the head of
///    the graph list's material list.
/// 4. [`init_render_gprops`] on `SI_renderGprops` (without the GL layer's `SI_GpropsInit`).
/// 5. [`ren_control_init`] on `SI_defaultControl`, [`matter_fade_init`],
///    [`fog_init_default`]: this package's ports.
/// 6. [`incrust_init`]: the incrustation and its font file (`read` gives the file).
/// 7. `SI_renChunk = SI_chunkAllocate(200, 5000000)`: `si-render`'s port on the image.
///
/// Where it should finally live: the routine is `matter.c`'s, this package's; its call is
/// the start-up's (`si_runCreative` and `SI_initStandAloneRender`, finding 0019: `si-app`),
/// after the graph lists exist and before anything is loaded. Until `si-app` makes the
/// image, the program that makes it (`rensetup-check`) calls this right after
/// `si_scene::load::Scene::start`. Needs the screen's size (`SI_xmaxscreen`,
/// `SI_ymaxscreen`) set first: `SI_renControlInit` centres the windows on it; and
/// `SI_resdir` for the font. `MATTER_INIT_STEPS=<digits>` (a check's aid) leaves out the
/// steps 3, 4, 6, 7 not named, to measure what each changes.
pub fn matter_init(m: &mut Mem, read: &dyn Fn(&[u8]) -> Option<Vec<u8>>) {
    struct NoCalls;
    impl si_elemio::image::Outside for NoCalls {
        fn call(&mut self, _: &mut Mem, _: &'static si_elemio::image::Routine, _: &[u32]) -> u32 {
            0
        }
        fn file_left(&mut self, _: &mut Mem, _: u32) -> u32 {
            0
        }
    }
    let steps = std::env::var("MATTER_INIT_STEPS").unwrap_or_else(|_| "3467".into());
    si_elemio::image::lights::light_initialize(m, &mut NoCalls, SI_DEFAULT_LIGHT, 1);
    for o in [0x34, 0x38, 0x3c] {
        fl(m, SI_DEFAULT_LIGHT + o, 0x42c8_0000);
    }
    if steps.contains('3') {
        link_default_material(m);
    }
    if steps.contains('4') {
        init_render_gprops(m, SI_RENDER_GPROPS);
    }
    ren_control_init(m, SI_DEFAULT_CONTROL.0);
    matter_fade_init(m);
    fog_init_default(m);
    if steps.contains('6') {
        incrust_init(m, read);
    }
    if steps.contains('7') {
        // `SI_renChunk` (0x0869bc84)
        let c = si_render::mem_chunk_image::chunk_allocate(m, 200, 5_000_000);
        m[SI_REN_CHUNK] = Ptr32::new(c);
    }
}
