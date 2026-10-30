// SPDX-License-Identifier: MIT
//! One routine of `mat_global.c` (the light dialogue's file; finding 0012, section 20).

use si_mem::Mem;
use si_x87::F80;

/// `ligSliderMode` (0x0869ef00): the colour sliders' mode of the light dialogue.
pub const LIG_SLIDER_MODE: u32 = 0x0869_ef00;

/// [soft 0x0827ab90 SI_lightGetColMode]: `SI_lightGetColMode()`: the 16-bit `ligSliderMode`,
/// sign-extended. Class: copy.
pub fn light_get_col_mode(m: &Mem) -> i32 {
    m.i16(LIG_SLIDER_MODE) as i32
}

si_mem::outside_calls! {
    /// What `mat_global.c` calls in other source files and nobody has ported: a hook answers 0
    /// until the callee's file is ported. Each line names the callee `name (0xADDRESS)`.
    pub trait MatGlobalCalls {
        /// `SI_setStateItem (0x0838b824)`, `dialog.c`: (dialog, item, state): sets a dialog item's state.
        fn si_set_state_item(dialog: u32, item: i32, state: i32) = "SI_setStateItem", 0x0838b824;
        /// `SI_setTexed (0x0838bd10)`, `dialog.c`: (dialog, item, text): sets a dialog's text field.
        fn si_set_texed(dialog: u32, item: i32, text: u32) = "SI_setTexed", 0x0838bd10;
        /// `SI_getTexedFloat (0x0838c138)`, `dialog.c`: (dialog, item): the text field read as a `float`
        /// (in `st0`).
        fn si_get_texed_float(dialog: u32, item: i32) -> si_mem::Ret = "SI_getTexedFloat", 0x0838c138;
        /// `SI_setItemTitle (0x0838b664)`, `dialog.c`: (dialog, item, text).
        fn si_set_item_title(dialog: u32, item: i32, text: u32) = "SI_setItemTitle", 0x0838b664;
        /// `SI_colorConvert (0x082987d0)`, `palette.c`: (r, g, b, w, mode in, four floats out, mode out)
        /// converts a colour between RGB (0), HLS (1) and HSV (2) sliders.
        fn si_color_convert(r: u32, g: u32, b: u32, w: u32, from: i32, out: u32, to: i32) = "SI_colorConvert", 0x082987d0;
        /// `SI_setStateGhost (0x0838b868)`, `dialog.c`: (dialog, item, ghost): greys an item out or not.
        fn si_set_state_ghost(dialog: u32, item: i32, ghost: i32) = "SI_setStateGhost", 0x0838b868;
        /// `c3f (0x0861137c)`, `graphics.c` (ported in `si-gl`): the current colour from three floats at the address.
        fn c3f(rgb: u32) = "c3f", 0x0861137c;
        /// `SI_edAddUDF (0x08103b5c)`, `edtools.c` (ported in `si-scene`): (owner, chapter, shader).
        fn si_ed_add_udf(owner: u32, chapter: i32, udf: u32) = "SI_edAddUDF", 0x08103b5c;
        /// `SI_edDisposeUDF (0x08103ca8)`, `edtools.c`: (shader, chapter).
        fn si_ed_dispose_udf(udf: u32, chapter: i32) = "SI_edDisposeUDF", 0x08103ca8;
        /// `SI_edReplaceUDF (0x08103d44)`, `edtools.c`: (old shader, new shader, chapter).
        fn si_ed_replace_udf(old: u32, new: u32, chapter: i32) = "SI_edReplaceUDF", 0x08103d44;
        /// `SI_udfDispose (0x0854c3c0)`, `udf_fct.c` (ported in `si-hrc`): the shader.
        fn si_udf_dispose(udf: u32) = "SI_udfDispose", 0x0854c3c0;
        /// `SI_statusDisplayMessage (0x083cca94)`: (text, 0): the message line of the window.
        fn si_status_display_message(text: u32, level: i32) = "SI_statusDisplayMessage", 0x083cca94;
        /// `SI_edModelSetLigSelection (0x08100d70)`, `edtools.c`: (light, model).
        fn si_ed_model_set_lig_selection(light: u32, model: u32) = "SI_edModelSetLigSelection", 0x08100d70;
        /// `SI_edModelRemoveLigSelection (0x08100de4)`, `edtools.c`: (light, model).
        fn si_ed_model_remove_lig_selection(light: u32, model: u32) = "SI_edModelRemoveLigSelection", 0x08100de4;
        /// `SI_myclearSelect (0x08335040)`: clears the selection.
        fn si_myclear_select() = "SI_myclearSelect", 0x08335040;
        /// `SI_foolDisplay (0x08323a70)`: (graf list, 0, colour, graf node).
        fn si_fool_display(list: u32, a: i32, b: i32, node: u32) = "SI_foolDisplay", 0x08323a70;
        /// `swapbuffers (0x08611804)`, the graphics library.
        fn swapbuffers() = "swapbuffers", 0x08611804;
        /// `usleep (0x080c37b8)`, the C library (import).
        fn usleep(micros: i32) = "usleep", 0x080c37b8;
        /// `SI_refreshAll (0x08325dc8)`: (1).
        fn si_refresh_all(flag: i32) = "SI_refreshAll", 0x08325dc8;
        /// `SI_statusDisplayMouse (0x083cc650)`, `status.c`: the three mouse-button hints.
        fn si_status_display_mouse(left: u32, middle: u32, right: u32) = "SI_statusDisplayMouse", 0x083cc650;
        /// `SI_superModeOff (0x0837fae0)`, `control.c`: (1).
        fn si_super_mode_off(flag: i32) = "SI_superModeOff", 0x0837fae0;
        /// `SI_refreshSystem (0x08323d44)`, `draw.c`: (0, 0, 0x2f).
        fn si_refresh_system(port: u32, list: u32, what: i32) = "SI_refreshSystem", 0x08323d44;
        /// `getbutton (0x0860e684)`, the graphics library: whether a device button is down.
        fn getbutton(button: i32) -> i32 = "getbutton", 0x0860e684;
        /// `SI_elemdictFindElemFromGrafNode (0x080fcbec)`, `elemdict.c`: (graf node, &out, &out) the element record.
        fn si_elemdict_find_elem_from_graf_node(node: u32, a: u32, b: u32) -> u32 = "SI_elemdictFindElemFromGrafNode", 0x080fcbec;
        /// `SI_elemdictGetElem (0x08132b50)`, `elemdict.c`: (dictionary, chapter, id) the element record.
        fn si_elemdict_get_elem(dict: u32, chapter: i32, id: u32) -> u32 = "SI_elemdictGetElem", 0x08132b50;
        /// `SI_lightAllocate (0x08281d34)`, `light_fct.c` (ported in `si-elemio`): a new light record of the type.
        fn si_light_allocate(kind: i32) -> u32 = "SI_lightAllocate", 0x08281d34;
        /// `SI_matrixCopy (0x08516224)`, `algebra.c` (ported in `si-ascii`): (from, to).
        fn si_matrix_copy(from: u32, to: u32) = "SI_matrixCopy", 0x08516224;
        /// `SI_userDataCopyFromSetup (0x084630dc)`, `model.c` (ported in `si-hrc`): (node, &copy).
        fn si_user_data_copy_from_setup(node: u32, copy: u32) = "SI_userDataCopyFromSetup", 0x084630dc;
        /// `pushmatrix (0x0861052c)`, `graphics.c` (ported in `si-gl`).
        fn pushmatrix() = "pushmatrix", 0x0861052c;
        /// `popmatrix (0x0861053c)`, `graphics.c` (ported in `si-gl`).
        fn popmatrix() = "popmatrix", 0x0861053c;
        /// `bgnline (0x08612b58)`, `graphics.c` (ported in `si-gl`).
        fn bgnline() = "bgnline", 0x08612b58;
        /// `endline (0x08612b6c)`, `graphics.c` (ported in `si-gl`).
        fn endline() = "endline", 0x08612b6c;
        /// `v3f (0x08612c60)`, `graphics.c` (ported in `si-gl`): a vertex from three floats at the address.
        fn v3f(xyz: u32) = "v3f", 0x08612c60;
        /// `translate (0x08610744)`, `graphics.c` (ported in `si-gl`): three floats.
        fn translate(x: u32, y: u32, z: u32) = "translate", 0x08610744;
        /// `scale (0x08610774)`, `graphics.c` (ported in `si-gl`): three floats.
        fn scale(x: u32, y: u32, z: u32) = "scale", 0x08610774;
        /// `circ (0x086129cc)`, `graphics.c` (ported in `si-gl`): (x, y, radius) floats.
        fn circ(x: u32, y: u32, r: u32) = "circ", 0x086129cc;
        /// `rot (0x086106b4)`, `graphics.c` (ported in `si-gl`): (angle float, axis letter).
        fn rot(angle: u32, axis: i32) = "rot", 0x086106b4;
        /// `SI_gLSelect (0x0832ef0c)`: the graf node under the pointer in the window.
        fn si_gl_select(port: u32) -> u32 = "SI_gLSelect", 0x0832ef0c;
    }
}

/// [soft 0x0827d868 lightType]: `lightType(dialog, kind)`: the four light-type buttons (items 6 to
/// 9) of the light dialogue: the one whose item number is `kind` set, the others cleared.
/// Class: copy.
pub fn light_type(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, kind: i32) {
    for item in 6..=9 {
        o.si_set_state_item(m, dialog, item, (kind == item) as i32);
    }
}

/// [soft 0x0827d80c arealightType]: `arealightType(dialog, kind)`: the three area-light shape
/// buttons (items 0x2e to 0x30), the one equal to `kind` set. Class: copy.
pub fn arealight_type(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, kind: i32) {
    for item in 0x2e..=0x30 {
        o.si_set_state_item(m, dialog, item, (kind == item) as i32);
    }
}

/// [soft 0x0827d8dc shadowType]: `shadowType(dialog, kind)`: the four shadow buttons (items 0x3e
/// to 0x41), the one equal to `kind` set. Class: copy.
pub fn shadow_type(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, kind: i32) {
    for item in 0x3e..=0x41 {
        o.si_set_state_item(m, dialog, item, (kind == item) as i32);
    }
}

/// `SI_udfDispose` etc. take this chapter from `DAT_08781718` (read at each call).
pub const UDF_CHAPTER: u32 = 0x0878_1718;
/// `_SI_temp_` (0x08780f00): the program's scratch text buffer (30 bytes; `sprintf` overruns it as
/// the original's does).
pub const SI_TEMP: u32 = 0x0878_0f00;
/// 57.29577951308232 (0x08654838): radians to degrees.
const RAD_TO_DEG: f64 = 57.29577951308232;

/// [soft 0x0828179c mrUdfAddToDict]: `mrUdfAddToDict(light, other)`: the two shader references of
/// a light (+0x178, +0x17c) brought from `other`'s to `light`'s in the dictionary. Nothing for a
/// null argument. Per slot, with `a` the light's and `b` the other's reference: both null:
/// nothing; `b` null: `SI_edAddUDF(light, chapter, a)`, no disposal; `a` null: `b` is taken out
/// of the dictionary (`SI_edDisposeUDF(b, b+4)`), disposed (`SI_udfDispose`) and the slot
/// cleared. Slot +0x178 only, both set: with the same element (+8) `SI_edReplaceUDF(b, a, b+4)`,
/// else `b` is taken out and `a` added; then `b` is disposed and its slot cleared. Slot +0x17c
/// with both set: nothing. Class: copy.
pub fn mr_udf_add_to_dict(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32, other: u32) {
    if light == 0 || other == 0 {
        return;
    }
    let (a, b) = (m.u32(light + 0x178), m.u32(other + 0x178));
    if a != 0 || b != 0 {
        if b == 0 {
            let chapter = m.i32(UDF_CHAPTER);
            o.si_ed_add_udf(m, light, chapter, a);
        } else {
            if a == 0 {
                let chapter = m.i32(b + 4);
                o.si_ed_dispose_udf(m, b, chapter);
            } else {
                if m.i32(b + 8) == m.i32(a + 8) {
                    let chapter = m.i32(b + 4);
                    o.si_ed_replace_udf(m, b, a, chapter);
                } else {
                    let chapter = m.i32(b + 4);
                    o.si_ed_dispose_udf(m, b, chapter);
                    let chapter = m.i32(UDF_CHAPTER);
                    let a = m.u32(light + 0x178);
                    o.si_ed_add_udf(m, light, chapter, a);
                }
            }
            let b = m.u32(other + 0x178);
            o.si_udf_dispose(m, b);
            m.set_u32(other + 0x178, 0);
        }
    }
    let (a, b) = (m.u32(light + 0x17c), m.u32(other + 0x17c));
    if a != 0 || b != 0 {
        if b == 0 {
            let chapter = m.i32(UDF_CHAPTER);
            o.si_ed_add_udf(m, light, chapter, a);
        } else if a == 0 {
            let chapter = m.i32(b + 4);
            o.si_ed_dispose_udf(m, b, chapter);
            let b = m.u32(other + 0x17c);
            o.si_udf_dispose(m, b);
            m.set_u32(other + 0x17c, 0);
        }
    }
}

/// `sprintf(_SI_temp_, "%1.6f", v)`: the C library's text of a double (`inf`, `nan` and a minus
/// sign as the library writes them) in the scratch buffer.
fn print_f(m: &mut Mem, v: f64) {
    let text = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{v:.6}")
    };
    m.put_cstr(SI_TEMP, text.as_bytes());
}

/// `sprintf(_SI_temp_, "%d", v)`.
fn print_d(m: &mut Mem, v: i32) {
    m.put_cstr(SI_TEMP, v.to_string().as_bytes());
}

/// The dialog's number field `item` shows the float at `at` with six decimals.
fn show_f(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, item: i32, v: f64) {
    print_f(m, v);
    o.si_set_texed(m, dialog, item, SI_TEMP);
}

fn show_d(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, item: i32, v: i32) {
    print_d(m, v);
    o.si_set_texed(m, dialog, item, SI_TEMP);
}

/// The dialog record's three words (+0x270, +0x310, +0x3b0) of the record at the dialog's +0x14.
const GUARDS: [u32; 3] = [0x270, 0x310, 0x3b0];

/// [soft 0x0827d364 putLight]: `putLight(dialog, light)`: fills the light dialogue from a light
/// record. Item 4 the name (the record's first bytes), items 6, 7, 8, 9 the type (+0x1e: 3, 1, 2,
/// 4), 0x1b the short at +0x15c; the dialog record's three words are cleared while the colour
/// (`SI_colorConvert(light+0x24.., 0, light+0x24, ligSliderMode)`, in place) is shown in items
/// 0x10, 0x14 (the first two components, six decimals) and put back to the value the first had
/// before, then item 0x18 (third). Then +0x84 (0x1f), +0x88 (0x21), the angles +0x70, +0x74
/// times 57.29577951308232 (0x23, 0x27; product kept in the 80-bit register, stored as a
/// double), +0x90 (0x43), shadow buttons 0x3e to 0x41 (+0x8c: 1 to 4), integers +0x118,
/// +0x11c, +0x124 (0x4b, 0x4d, 0x4f), float +0x128 (0x47), item 0x2c (+0x182), area buttons
/// 0x2e to 0x30 (+0x184: 0 to 2), integers +0x188, +0x18c (0x33, 0x37), items 0x3a (+0x1c0) and
/// 0x59 (+0x180). Class: single (x87).
pub fn put_light(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, light: u32) {
    o.si_set_texed(m, dialog, 4, light);
    let kind = m.i16(light + 0x1e);
    for (item, want) in [(6, 3), (7, 1), (8, 2), (9, 4)] {
        o.si_set_state_item(m, dialog, item, (kind == want) as i32);
    }
    let s = m.i16(light + 0x15c) as i32;
    o.si_set_state_item(m, dialog, 0x1b, s);
    put_light_tail(m, o, dialog, light);
}

/// What `putLight` and `putEditLight` share from the colour on (the same calls in the same order).
fn put_light_tail(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, light: u32) {
    use si_x87::x;
    let rec = m.u32(dialog + 0x14);
    let saved = m.u32(rec + GUARDS[0]);
    for g in GUARDS {
        let rec = m.u32(dialog + 0x14);
        m.set_u32(rec + g, 0);
    }
    let c: [u32; 4] = [m.u32(light + 0x24), m.u32(light + 0x28), m.u32(light + 0x2c), m.u32(light + 0x30)];
    let mode = m.i16(LIG_SLIDER_MODE) as i32;
    o.si_color_convert(m, c[0], c[1], c[2], c[3], 0, light + 0x24, mode);
    let v = m.f32(light + 0x24) as f64;
    show_f(m, o, dialog, 0x10, v);
    let v = m.f32(light + 0x28) as f64;
    show_f(m, o, dialog, 0x14, v);
    for g in GUARDS {
        let rec = m.u32(dialog + 0x14);
        m.set_u32(rec + g, saved);
    }
    let v = m.f32(light + 0x2c) as f64;
    show_f(m, o, dialog, 0x18, v);
    let v = m.f32(light + 0x84) as f64;
    show_f(m, o, dialog, 0x1f, v);
    let v = m.f32(light + 0x88) as f64;
    show_f(m, o, dialog, 0x21, v);
    let v = (x(m.f32(light + 0x70)) * x(RAD_TO_DEG)).to_f64();
    show_f(m, o, dialog, 0x23, v);
    let v = (x(m.f32(light + 0x74)) * x(RAD_TO_DEG)).to_f64();
    show_f(m, o, dialog, 0x27, v);
    let v = m.f32(light + 0x90) as f64;
    show_f(m, o, dialog, 0x43, v);
    let sh = m.i16(light + 0x8c);
    for (item, want) in [(0x3e, 1), (0x3f, 2), (0x40, 3), (0x41, 4)] {
        o.si_set_state_item(m, dialog, item, (sh == want) as i32);
    }
    for (item, off) in [(0x4b, 0x118), (0x4d, 0x11c), (0x4f, 0x124)] {
        let v = m.i32(light + off);
        show_d(m, o, dialog, item, v);
    }
    let v = m.f32(light + 0x128) as f64;
    show_f(m, o, dialog, 0x47, v);
    let s = m.i16(light + 0x182) as i32;
    o.si_set_state_item(m, dialog, 0x2c, s);
    let a = m.i32(light + 0x184);
    for (item, want) in [(0x2e, 0), (0x2f, 1), (0x30, 2)] {
        o.si_set_state_item(m, dialog, item, (a == want) as i32);
    }
    for (item, off) in [(0x33, 0x188), (0x37, 0x18c)] {
        let v = m.i32(light + off);
        show_d(m, o, dialog, item, v);
    }
    let s = m.i16(light + 0x1c0) as i32;
    o.si_set_state_item(m, dialog, 0x3a, s);
    let s = m.i16(light + 0x180) as i32;
    o.si_set_state_item(m, dialog, 0x59, s);
}

/// `SI_sboxfi` (0x086ee2a4): a function pointer, set to `sboxfi (0x0861236c)` of `graphics.c`
/// (four 32-bit corners of a screen box).
pub const SI_SBOXFI: u32 = 0x086e_e2a4;

/// What `SI_getTexedFloat` answers in `st0`, stored as the compiler does (`fstp` to a `float`).
fn texed_float(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, item: i32) -> u32 {
    let r = o.si_get_texed_float(m, dialog, item);
    let v = r.st0.map_or(si_x87::F80::ZERO, |b| si_x87::F80::from_bits(u64::from_le_bytes(b[..8].try_into().unwrap()), u16::from_le_bytes([b[8], b[9]])));
    v.to_f32().to_bits()
}

/// [soft 0x0827d0cc colorSliderSet]: `colorSliderSet(dialog)`: the colour sliders go to the next
/// colour model. The three fields (items 0x10, 0x14, 0x18) are read as floats and converted
/// from `ligSliderMode` to model 0 into four floats on the stack (the fourth argument of the
/// call is an uninitialised local of the original; 0 here). Mode 0 (HLS sliders next): items
/// 0xf, 0x13, 0x17 titled "H", "L", "S", `ligSliderMode` 1, item 0xd "HLS"; mode 1: "H", "S",
/// "V", 2, "HSV"; mode 2: "R", "G", "B", 0, "RGB"; any other mode: no titles and no change.
/// Then the dialog record's three words (+0x270, +0x310, +0x3b0) are cleared, the four floats
/// converted from model 0 to the new `ligSliderMode` in place, the first two shown with six
/// decimals in items 0x10 and 0x14, the three words set to the value the first had, the third
/// shown in item 0x18. Class: copy.
pub fn color_slider_set(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32) {
    let l0 = texed_float(m, o, dialog, 0x10);
    let l1 = texed_float(m, o, dialog, 0x14);
    let l2 = texed_float(m, o, dialog, 0x18);
    let out = m.local(16);
    let mode = m.i16(LIG_SLIDER_MODE) as i32;
    // (the fourth word is an uninitialised local of the original's frame)
    o.si_color_convert(m, l0, l1, l2, 0, mode, out, 0);
    let (titles, next, name) = match m.u16(LIG_SLIDER_MODE) as i16 {
        0 => (Some([0x0869_efc0u32, 0x0869_efc2, 0x0869_efc4]), 1, 0x0869_efc6u32),
        1 => (Some([0x0869_efca, 0x0869_efcc, 0x0869_efce]), 2, 0x0869_efd0),
        2 => (Some([0x0869_efd4, 0x0869_efd6, 0x0869_efd8]), 0, 0x0869_efda),
        _ => (None, 0, 0),
    };
    if let Some(t) = titles {
        for (item, text) in [0xf, 0x13, 0x17].into_iter().zip(t) {
            o.si_set_item_title(m, dialog, item, text);
        }
        m.set_i16(LIG_SLIDER_MODE, next);
        o.si_set_item_title(m, dialog, 0xd, name);
    }
    let rec = m.u32(dialog + 0x14);
    let saved = m.u32(rec + GUARDS[0]);
    for g in GUARDS {
        let rec = m.u32(dialog + 0x14);
        m.set_u32(rec + g, 0);
    }
    let v: [u32; 4] = [m.u32(out), m.u32(out + 4), m.u32(out + 8), m.u32(out + 12)];
    let to = m.i16(LIG_SLIDER_MODE) as i32;
    o.si_color_convert(m, v[0], v[1], v[2], v[3], 0, out, to);
    let t = m.f32(out) as f64;
    show_f(m, o, dialog, 0x10, t);
    let t = m.f32(out + 4) as f64;
    show_f(m, o, dialog, 0x14, t);
    for g in GUARDS {
        let rec = m.u32(dialog + 0x14);
        m.set_u32(rec + g, saved);
    }
    let t = m.f32(out + 8) as f64;
    show_f(m, o, dialog, 0x18, t);
    m.release(16);
}

/// [soft 0x0827d00c refreshLightArea]: `refreshLightArea(dialog)`: the colour patch of the light
/// dialogue: the fields 0x10, 0x14, 0x18 read as floats, converted from `ligSliderMode` to model
/// 0 (the fourth word is again an uninitialised local), `c3f` of the result, then the box
/// `SI_sboxfi(x0 + 1, y0 + 1, x1 - 1, y1 - 1)` through the pointer in `SI_sboxfi` with the
/// shorts at +0x1be, +0x1c2, +0x1c0, +0x1c4 of the dialog record (the record at the dialog's
/// +0x14). Answers 1. Class: copy.
pub fn refresh_light_area(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32) -> i32 {
    let rec = m.u32(dialog + 0x14);
    let l0 = texed_float(m, o, dialog, 0x10);
    let l1 = texed_float(m, o, dialog, 0x14);
    let l2 = texed_float(m, o, dialog, 0x18);
    let out = m.local(16);
    let mode = m.i16(LIG_SLIDER_MODE) as i32;
    o.si_color_convert(m, l0, l1, l2, 0, mode, out, 0);
    o.c3f(m, out);
    let w = [
        (m.i16(rec + 0x1be) as i32).wrapping_add(1) as u32,
        (m.i16(rec + 0x1c2) as i32).wrapping_add(1) as u32,
        (m.i16(rec + 0x1c0) as i32).wrapping_sub(1) as u32,
        (m.i16(rec + 0x1c4) as i32).wrapping_sub(1) as u32,
    ];
    let f = m.u32(SI_SBOXFI);
    o.call(m, si_mem::Routine::at(f), &w);
    m.release(16);
    1
}

/// [soft 0x0827fbac putEditLight]: `putEditLight(dialog, light, elem)`: the editing dialogue's
/// version of [`put_light`]: item 2 the text at +0x10 of the record the element points to (the
/// empty string at 0x0869f0b3 when that is null), item 4 the text at its +0x14; the type
/// buttons 6 to 9 and item 0x1b as in `putLight`; when the int at +0x15c is 0 items 0x1c and
/// 0x1d are greyed out, else item 0x1c takes the short at +0x160, item 0x1d (the int at +0x160
/// is 0) and both are un-greyed; then everything `putLight` shows from the colour on.
/// Class: single (x87).
pub fn put_edit_light(m: &mut Mem, o: &mut dyn si_mem::Outside, dialog: u32, light: u32, elem: u32) {
    let rec = m.u32(elem);
    let mut name = m.u32(rec + 0x10);
    if name == 0 {
        name = 0x0869_f0b3;
    }
    o.si_set_texed(m, dialog, 2, name);
    let rec = m.u32(elem);
    let t = m.u32(rec + 0x14);
    o.si_set_texed(m, dialog, 4, t);
    let kind = m.i16(light + 0x1e);
    for (item, want) in [(6, 3), (7, 1), (8, 2), (9, 4)] {
        o.si_set_state_item(m, dialog, item, (kind == want) as i32);
    }
    let s = m.i16(light + 0x15c) as i32;
    o.si_set_state_item(m, dialog, 0x1b, s);
    if m.i32(light + 0x15c) == 0 {
        o.si_set_state_ghost(m, dialog, 0x1c, 1);
        o.si_set_state_ghost(m, dialog, 0x1d, 1);
    } else {
        let s = m.i16(light + 0x160) as i32;
        o.si_set_state_item(m, dialog, 0x1c, s);
        let z = (m.i32(light + 0x160) == 0) as i32;
        o.si_set_state_item(m, dialog, 0x1d, z);
        o.si_set_state_ghost(m, dialog, 0x1c, 0);
        o.si_set_state_ghost(m, dialog, 0x1d, 0);
    }
    put_light_tail(m, o, dialog, light);
}

/// `SI_grafLists` (0x08679498) and `SI_procData` (0x08679314): the graf lists (0x24 bytes each)
/// and the process data whose word +4 picks the current one.
const SI_GRAF_LISTS: u32 = 0x0867_9498;
const SI_PROC_DATA: u32 = 0x0867_9314;

/// The current graf list: `SI_grafLists + SI_procData[1] * 0x24`.
fn cur_graf_list(m: &Mem) -> u32 {
    let idx = m.u32(m.u32(SI_PROC_DATA) + 4);
    m.u32(SI_GRAF_LISTS).wrapping_add(idx.wrapping_mul(0x24))
}

/// The common tail of the two (dis)associate routines: the selection cleared, the list and the
/// light's node redrawn, a half-second pause, a refresh.
fn light_assoc_redraw(m: &mut Mem, o: &mut dyn si_mem::Outside, node: u32) {
    o.si_myclear_select(m);
    let gl = cur_graf_list(m);
    o.si_fool_display(m, gl, 0, 1, 0);
    let gl = cur_graf_list(m);
    o.si_fool_display(m, gl, 0, 2, node);
    o.swapbuffers(m);
    o.usleep(m, 500000);
    o.si_refresh_all(m, 1);
}

/// [soft 0x08281508 modelAssociateLight]: ties every selected model (node kinds 2 and 3, with a
/// payload) of the current graf list to the selective light of `node`: the light's table of model
/// flags (+0x168, count +0x164) is grown with zeros to hold the model's index (+0x354), the
/// model's flag set to 1 and `SI_edModelSetLigSelection` called. Without a selected model the
/// message "No selected models" is shown instead of the redraw. Class: copy.
pub fn model_associate_light(m: &mut Mem, o: &mut dyn si_mem::Outside, node: u32) {
    let mut any = false;
    o.si_status_display_message(m, 0x0869_f2e6, 0);
    let light = m.u32(node + 4);
    let mut n = m.u32(cur_graf_list(m) + 4);
    while n != 0 {
        if m.u32(n + 8) != 0 && m.u32(n).wrapping_sub(2) < 2 {
            any = true;
            let model = m.u32(n + 4);
            let mut idx = m.i32(model + 0x354);
            if m.i32(light + 0x164) <= idx {
                idx += 1;
                let table = m.u32(light + 0x168);
                let table = m.si_realloc(table, (idx as u32).wrapping_mul(4));
                let mut k = m.i32(light + 0x164);
                m.set_u32(light + 0x168, table);
                while k < idx {
                    let t = m.u32(light + 0x168);
                    m.set_u32(t.wrapping_add((k as u32).wrapping_mul(4)), 0);
                    k += 1;
                }
                m.set_u32(light + 0x164, idx as u32);
                idx = m.i32(model + 0x354);
            }
            let t = m.u32(light + 0x168);
            m.set_u32(t.wrapping_add((idx as u32).wrapping_mul(4)), 1);
            o.si_ed_model_set_lig_selection(m, light, model);
        }
        n = m.u32(n + 0x1c);
    }
    if any {
        light_assoc_redraw(m, o, node);
    } else {
        o.si_status_display_message(m, 0x0869_f2e7, 0);
    }
}

/// [soft 0x0828168c modelDisassociateLight]: the reverse: for every selected model (kinds 2 and 3
/// with a payload) whose index is below the light's table count its flag is set to 0, and
/// `SI_edModelRemoveLigSelection` is called for each; the redraw follows only if some flag was
/// reset, otherwise "No selected models". Class: copy.
pub fn model_disassociate_light(m: &mut Mem, o: &mut dyn si_mem::Outside, node: u32) {
    o.si_status_display_message(m, 0x0869_f2fa, 0);
    let light = m.u32(node + 4);
    let mut any = false;
    let mut n = m.u32(cur_graf_list(m) + 4);
    while n != 0 {
        if m.u32(n + 8) != 0 && m.u32(n).wrapping_sub(2) < 2 {
            let model = m.u32(n + 4);
            let idx = m.i32(model + 0x354);
            if idx < m.i32(light + 0x164) {
                any = true;
                let t = m.u32(light + 0x168);
                m.set_u32(t.wrapping_add((idx as u32).wrapping_mul(4)), 0);
            }
            o.si_ed_model_remove_lig_selection(m, light, model);
        }
        n = m.u32(n + 0x1c);
    }
    if any {
        light_assoc_redraw(m, o, node);
    } else {
        o.si_status_display_message(m, 0x0869_f2fb, 0);
    }
}

/// [soft 0x08281498 SI_selLightHandler]: the event handler for tying models to a selective light:
/// the graf node under the pointer (`SI_gLSelect(event.port)`, event +0xc) must be a light kind
/// (6 to 8, 0x13, 0x14); if its light has no selective flag (+0x15c) "Not a selective light" is
/// said, otherwise `mode` 1 associates and any other mode disassociates. Class: copy.
pub fn sel_light_handler(m: &mut Mem, o: &mut dyn si_mem::Outside, event: u32, mode: i32) {
    let port = m.u32(event + 0xc);
    let node = o.si_gl_select(m, port);
    if node == 0 {
        return;
    }
    let kind = m.u32(node);
    if kind.wrapping_sub(6) < 3 || kind == 0x13 || kind == 0x14 {
        let light = m.u32(node + 4);
        if m.i32(light + 0x15c) == 0 {
            o.si_status_display_message(m, 0x0869_f2d0, 0);
        } else if mode == 1 {
            model_associate_light(m, o, node);
        } else {
            model_disassociate_light(m, o, node);
        }
    }
}

/// `SI_lightLicense` (0x08679320, 16-bit), `SI_tabletButton` (0x086794b4), `SI_cid.MODELS`
/// (0x08781680, the chapter number of models) and `SI_elemDicts` (0x0867931c, the dictionaries,
/// 0x70 bytes each, picked by the first word of `SI_procData`).
const SI_LIGHT_LICENSE: u32 = 0x0867_9320;
const SI_TABLET_BUTTON: u32 = 0x0867_94b4;
const SI_CID_MODELS: u32 = 0x0878_1680;
const SI_ELEM_DICTS: u32 = 0x0867_931c;

/// [soft 0x08280978 SI_setSelSelection]: the start of the selective-light mode. The first node of
/// the current graf list of kind 6, 8 or 0x13 whose flag (+8) is 1 or 3 gives the light (its
/// payload); with none the status line says "Selective light: Select a light to edit." and the
/// mouse hints are set, answer 0; if the light is not selective (+0x15c is 0) "This is not a
/// selective light." and 0. Otherwise `SI_superModeOff(1)`, the object-selection message, the hints
/// "Pick node"/"Pick branch"/"End mode", every node of kind 2 or 3 flagged 1 when its model's
/// index (+0x354) lies in the light's table (+0x168, count +0x164) with a non-zero entry and
/// (the licence is 0 or the node's byte +0x17 has no 0x10 bit), else 0, then
/// `SI_refreshSystem(0, 0, 0x2f)`, answer 1. Class: copy.
pub fn set_sel_selection(m: &mut Mem, o: &mut dyn si_mem::Outside) -> i32 {
    let mut light = 0;
    let mut n = m.u32(cur_graf_list(m) + 4);
    while n != 0 {
        let k = m.u32(n);
        if (k == 6 || k == 8 || k == 0x13) && matches!(m.u32(n + 8), 1 | 3) {
            light = m.u32(n + 4);
            break;
        }
        n = m.u32(n + 0x1c);
    }
    if light == 0 {
        o.si_status_display_message(m, 0x0869_f100, 1);
        o.si_status_display_mouse(m, 0x0869_f133, 0x0869_f132, 0x0869_f129);
        return 0;
    }
    if m.i32(light + 0x15c) == 0 {
        o.si_status_display_message(m, 0x0869_f140, 1);
        o.si_status_display_mouse(m, 0x0869_f161, 0x0869_f160, 0x0869_f15f);
        return 0;
    }
    o.si_super_mode_off(m, 1);
    o.si_status_display_message(m, 0x0869_f180, 4);
    o.si_status_display_mouse(m, 0x0869_f1b8, 0x0869_f1ac, 0x0869_f1a3);
    let license = m.u16(SI_LIGHT_LICENSE);
    if m.i32(light + 0x164) > 0 {
        let mut n = m.u32(cur_graf_list(m) + 4);
        while n != 0 {
            if m.u32(n).wrapping_sub(2) < 2 {
                let idx = m.i32(m.u32(n + 4) + 0x354);
                let on = idx < m.i32(light + 0x164)
                    && m.u32(m.u32(light + 0x168).wrapping_add((idx as u32).wrapping_mul(4))) != 0
                    && (license == 0 || m.u8(n + 0x17) & 0x10 == 0);
                m.set_u32(n + 8, on as u32);
            }
            n = m.u32(n + 0x1c);
        }
    }
    o.si_refresh_system(m, 0, 0, 0x2f);
    1
}

/// Toggles (or sets) the light's flag of a model, growing the table with zeros as
/// `modelAssociateLight` does (`SI_realloc` of the table to `index + 1` words).
fn light_flag_toggle(m: &mut Mem, light: u32, model: u32) {
    let idx = m.i32(model + 0x354);
    if idx < m.i32(light + 0x164) {
        let t = m.u32(light + 0x168).wrapping_add((idx as u32).wrapping_mul(4));
        let v = (m.u32(t) == 0) as u32;
        m.set_u32(t, v);
    } else {
        let n = idx + 1;
        let table = m.u32(light + 0x168);
        let table = m.si_realloc(table, (n as u32).wrapping_mul(4));
        m.set_u32(light + 0x168, table);
        let mut k = m.i32(light + 0x164);
        while k < n {
            let t = m.u32(light + 0x168);
            m.set_u32(t.wrapping_add((k as u32).wrapping_mul(4)), 0);
            k += 1;
        }
        m.set_i32(light + 0x164, n);
        let idx = m.i32(model + 0x354);
        let t = m.u32(light + 0x168);
        m.set_u32(t.wrapping_add((idx as u32).wrapping_mul(4)), 1);
    }
}

/// [soft 0x08280648 SI_selectiveLightHandler]: the pick handler of the selective-light mode.
/// `SI_setSelSelection` first (answer 0 if it says 0); the light is the payload of the last node
/// of the current graf list that is flagged 1 or 3 and of kind 6, 8, 0x13 or 0x14, and must be
/// selective, else 0. The node under the pointer (`SI_gLSelect(event.port)`) must be of kind 2, 3
/// or 0xc, else 1. A node of kind 0xc is an element node: every entry of its element table
/// (count at +0, entries of 0x20 bytes at +0xc) of the models chapter is one model, found in the
/// dictionary; otherwise the node itself (and, when button 0x66 is down or the tablet button is
/// 0x66, the next `node.u16(+0x10)` nodes along +0x1c). For each model the light's flag is toggled
/// and the node's flag (+8) turned 0 with `SI_edModelRemoveLigSelection` when it was 1, else
/// turned 1 with `SI_edModelSetLigSelection` unless the licence is set and the node's byte
/// +0x17 has bit 0x10. Then `SI_refreshSystem(0, 0, 0x2f)`; answer 1. Class: copy.
pub fn selective_light_handler(m: &mut Mem, o: &mut dyn si_mem::Outside, event: u32) -> i32 {
    if set_sel_selection(m, o) == 0 {
        return 0;
    }
    let mut light = 0;
    let mut found = false;
    let mut n = m.u32(cur_graf_list(m) + 4);
    while n != 0 {
        let k = m.u32(n);
        if matches!(m.u32(n + 8), 1 | 3) && (k == 6 || k == 8 || k == 0x13 || k == 0x14) {
            light = m.u32(n + 4);
            found = true;
        }
        n = m.u32(n + 0x1c);
    }
    if !found || m.i32(light + 0x15c) == 0 {
        return 0;
    }
    let port = m.u32(event + 0xc);
    let mut node = o.si_gl_select(m, port);
    if node == 0 {
        return 1;
    }
    let kind = m.u32(node);
    if !(kind.wrapping_sub(2) < 2 || kind == 0xc) {
        return 1;
    }
    let button = o.getbutton(m, 0x66) != 0 || m.i32(SI_TABLET_BUTTON) == 0x66;
    let elem_node = m.u32(node) == 0xc;
    let mut table = 0;
    let mut count: i32 = 0;
    if elem_node {
        let (a, b) = (m.local(4), m.local(4));
        let r = o.si_elemdict_find_elem_from_graf_node(m, node, a, b);
        table = m.u32(r + 0x64);
        let total = m.i32(table);
        if 0 < total {
            let base = m.u32(table + 0xc);
            let models = m.i32(SI_CID_MODELS);
            for i in 0..total as u32 {
                if m.i16(base.wrapping_add(i.wrapping_mul(0x20)) + 0x10) as i32 == models {
                    count += 1;
                }
            }
        }
    } else if !button {
        count = 1;
    } else {
        count = m.u16(node + 0x10) as i32 + 1;
    }
    let license = m.u16(SI_LIGHT_LICENSE);
    let mut at = 0i32;
    let (mut model, mut gnode) = (0u32, 0u32);
    while count > 0 {
        if elem_node {
            count -= 1;
            let total = m.i32(table);
            if at < total {
                let base = m.u32(table + 0xc);
                let models = m.i32(SI_CID_MODELS);
                loop {
                    let entry = base.wrapping_add((at as u32).wrapping_mul(0x20));
                    at += 1;
                    if m.i16(entry + 0x10) as i32 == models {
                        let p0 = m.u32(m.u32(SI_PROC_DATA));
                        let dict = m.u32(SI_ELEM_DICTS).wrapping_add(p0.wrapping_mul(7).wrapping_mul(0x10));
                        let chapter = m.i16(entry + 0x10) as i32;
                        let id = m.u32(entry + 4);
                        let e = o.si_elemdict_get_elem(m, dict, chapter, id);
                        model = m.u32(e + 4);
                        gnode = m.u32(e + 8);
                        break;
                    }
                    if at >= total {
                        break;
                    }
                }
            }
        } else {
            gnode = node;
            count -= 1;
            model = m.u32(node + 4);
            node = m.u32(node + 0x1c);
        }
        light_flag_toggle(m, light, model);
        if m.u32(gnode + 8) == 1 {
            m.set_u32(gnode + 8, 0);
            o.si_ed_model_remove_lig_selection(m, light, model);
        } else if license == 0 || m.u8(gnode + 0x17) & 0x10 == 0 {
            m.set_u32(gnode + 8, 1);
            o.si_ed_model_set_lig_selection(m, light, model);
        }
    }
    o.si_refresh_system(m, 0, 0, 0x2f);
    1
}

/// What `fld`/`fstp` of a 32-bit float does to its bits: a signalling NaN comes out quiet.
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// `SI_defaultLight` (0x08781e00): the 0x240-byte record a new light is first filled from.
const SI_DEFAULT_LIGHT: u32 = 0x0878_1e00;
/// 16-bit flag (0x087814e6): whether user data is copied with a light.
const USER_DATA_FLAG: u32 = 0x0878_14e6;

/// [soft 0x08280200 SI_lightCopy]: a copy of a light: `SI_lightAllocate(1)` (null: answer null),
/// the record filled with `SI_defaultLight`, then the name (`strcpy`), the type, colour, angles,
/// shadow, shader and area fields copied one by one (the 32-bit floats through the x87, so a
/// signalling NaN comes out quiet); the selective table (+0x164, +0x168) and the shader slots
/// (+0x178, +0x17c) cleared; for a spot light (type 4) the 0x14-byte extension at +0x16c is copied
/// into a new `SI_malloc` block whose first word is a `strdup` of the original's; the 0x184 mode
/// decides how much of +0x190 is copied (1: one float; 0: twelve floats; 2: four floats; else
/// none); the matrix at +0x1e0 by `SI_matrixCopy`; the user data (+0x220) by
/// `SI_userDataCopyFromSetup` when the flag word is set and the original has some, else cleared.
/// Class: copy.
pub fn light_copy(m: &mut Mem, o: &mut dyn si_mem::Outside, src: u32) -> u32 {
    let dst = o.si_light_allocate(m, 1);
    if dst == 0 {
        return dst;
    }
    for i in 0..0x90 {
        let w = m.u32(SI_DEFAULT_LIGHT + 4 * i);
        m.set_u32(dst + 4 * i, w);
    }
    let mut name = m.cstr(src);
    name.push(0);
    m.set_bytes(dst, &name);
    let h = |m: &mut Mem, off: u32| {
        let v = m.u16(src + off);
        m.set_u16(dst + off, v);
    };
    let w = |m: &mut Mem, off: u32| {
        let v = m.u32(src + off);
        m.set_u32(dst + off, v);
    };
    let f = |m: &mut Mem, off: u32| {
        let v = through_x87(m.u32(src + off));
        m.set_u32(dst + off, v);
    };
    h(m, 0x176);
    h(m, 0x1e);
    for off in [0x24, 0x28, 0x2c, 0x30] {
        w(m, off);
    }
    f(m, 0x90);
    for off in [0x34, 0x38, 0x3c] {
        w(m, off);
    }
    h(m, 0x8c);
    for off in [0x84, 0x88, 0x70, 0x74] {
        f(m, off);
    }
    for off in [0x58, 0x5c, 0x60, 0x118, 0x11c, 0x124] {
        w(m, off);
    }
    f(m, 0x128);
    f(m, 0x12c);
    w(m, 0x15c);
    w(m, 0x160);
    m.set_u32(dst + 0x164, 0);
    m.set_u32(dst + 0x168, 0);
    if m.i16(src + 0x1e) == 4 {
        let block = m.si_malloc(0x14);
        m.set_u32(dst + 0x16c, block);
        let from = m.u32(src + 0x16c);
        for i in 0..5 {
            let v = m.u32(from + 4 * i);
            m.set_u32(block + 4 * i, v);
        }
        let first = m.u32(m.u32(src + 0x16c));
        let copy = m.strdup(first);
        let block = m.u32(dst + 0x16c);
        m.set_u32(block, copy);
    } else {
        m.set_u32(dst + 0x16c, 0);
    }
    m.set_u32(dst + 0x178, 0);
    m.set_u32(dst + 0x17c, 0);
    h(m, 0x182);
    w(m, 0x184);
    w(m, 0x188);
    w(m, 0x18c);
    match m.i32(src + 0x184) {
        1 => f(m, 0x190),
        0 => {
            for off in 0x190..0x1c0 {
                if off % 4 == 0 {
                    f(m, off);
                }
            }
        }
        2 => {
            for off in [0x190, 0x194, 0x198, 0x19c] {
                f(m, off);
            }
        }
        _ => {}
    }
    h(m, 0x1c0);
    for off in [0x1c4, 0x1c8, 0x1cc, 0x1d0, 0x1d4, 0x1d8] {
        w(m, off);
    }
    h(m, 0x1dc);
    for off in [0x224, 0x228, 0x22c] {
        w(m, off);
    }
    f(m, 0x230);
    w(m, 0x234);
    w(m, 0x238);
    h(m, 0x23c);
    h(m, 0x23e);
    o.si_matrix_copy(m, src + 0x1e0, dst + 0x1e0);
    h(m, 0x180);
    if m.u16(USER_DATA_FLAG) == 0 || m.u32(src + 0x220) == 0 {
        m.set_u32(dst + 0x220, 0);
    } else {
        let node = m.u32(src + 0x220);
        o.si_user_data_copy_from_setup(m, node, dst + 0x220);
    }
    dst
}

/// A 32-bit float through the x87 (`fld`/`fstp`): the value as the processor stores it back.
fn x87_f32(bits: u32) -> u32 {
    F80::from_f32(f32::from_bits(bits)).to_f32().to_bits()
}

/// The 32-bit float `v` (an extended value) as the word the x87 stores.
fn store_f32(v: F80) -> u32 {
    v.to_f32().to_bits()
}

/// The circles and the cone of the lamp symbol, which `SI_drawInfLight` and `SI_drawSun`
/// share: unit circle (0, 0, 1.0) in the plane, turned by 45 degrees about y three times, then
/// by 90 about x, another circle, and the shifted pair (z -0.5 and +1.0 with the radius
/// 0x3f5db3d7).
fn lamp_circles(m: &mut Mem, o: &mut dyn si_mem::Outside) {
    const ONE: u32 = 0x3f80_0000;
    o.circ(m, 0, 0, ONE);
    for _ in 0..3 {
        o.rot(m, 0x4234_0000, 0x79);
        o.circ(m, 0, 0, ONE);
    }
    o.rot(m, 0x42b4_0000, 0x78);
    o.circ(m, 0, 0, ONE);
    o.translate(m, 0, 0, 0xbf00_0000);
    o.circ(m, 0, 0, 0x3f5d_b3d7);
    o.translate(m, 0, 0, ONE);
    o.circ(m, 0, 0, 0x3f5d_b3d7);
}

/// [soft 0x08280b00 SI_drawInfLight]: the symbol of an infinite light at its position
/// (`light+0x34`, `+0x38`, `+0x3c`): a line from the origin to the position, a box (twelve
/// edges of the corners that lie 0.2 from the position, on the side of its signs) from the
/// origin and, translated to the position and scaled by 0.5, the lamp circles. The corner
/// coordinates live in the static buffer at 0x0869f1e0 (and the last offset in 0x0879a980), as
/// the original leaves them. Class: copy.
pub fn draw_inf_light(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) {
    const P: u32 = 0x0869_f1e0;
    const LEN1: u32 = 0x0879_a980;
    let w = |m: &Mem, off: u32| m.u32(light + off);
    o.pushmatrix(m);
    for (i, off) in [0x34u32, 0x38, 0x3c].into_iter().enumerate() {
        let v = x87_f32(w(m, off));
        m.set_u32(P + 0xc + 4 * i as u32, v);
    }
    o.bgnline(m);
    o.v3f(m, P);
    o.v3f(m, P + 0xc);
    o.endline(m);
    let k = F80::from_f32(0.2);
    let ld = |m: &Mem, off: u32| F80::from_f32(f32::from_bits(m.u32(light + off)));
    let t1 = ld(m, 0x34) * k;
    let t2 = ld(m, 0x38) * k;
    let t3 = ld(m, 0x3c) * k;
    let (c1, c2, c3) = (store_f32(t1), store_f32(t2), store_f32(t3));
    let zero = F80::from_f32(0.0);
    let plus = F80::from_f32(0.2);
    let minus = F80::from_f32(-0.2);
    let phi1 = if ld(m, 0x34).lt(zero) { minus } else { plus };
    m.set_u32(LEN1, store_f32(phi1));
    m.set_u32(P + 0x1c, c2);
    m.set_u32(P + 0x20, c3);
    m.set_u32(P + 0x18, store_f32(phi1 + t1));
    let phi2 = if ld(m, 0x38).lt(zero) { minus } else { plus };
    m.set_u32(LEN1, store_f32(phi2));
    m.set_u32(P + 0x24, c1);
    m.set_u32(P + 0x2c, c3);
    m.set_u32(P + 0x28, store_f32(t2 + phi2));
    let phi3 = if ld(m, 0x3c).lt(zero) { minus } else { plus };
    m.set_u32(LEN1, store_f32(phi3));
    m.set_u32(P + 0x30, c1);
    m.set_u32(P + 0x34, c2);
    m.set_u32(P + 0x38, store_f32(phi3 + t3));
    for corner in [0x18, 0x24, 0x30] {
        o.bgnline(m);
        o.v3f(m, P);
        o.v3f(m, P + corner);
        o.endline(m);
    }
    m.set_u32(P + 0x3c, c1);
    m.set_u32(P + 0x40, c2);
    m.set_u32(P + 0x44, c3);
    for corner in [0x18, 0x24, 0x30] {
        o.bgnline(m);
        o.v3f(m, P + corner);
        o.v3f(m, P + 0x3c);
        o.endline(m);
    }
    let (x, y, z) = (x87_f32(w(m, 0x34)), x87_f32(w(m, 0x38)), x87_f32(w(m, 0x3c)));
    o.translate(m, x, y, z);
    o.scale(m, 0x3f00_0000, 0x3f00_0000, 0x3f00_0000);
    lamp_circles(m, o);
    o.popmatrix(m);
}

/// [soft 0x08280e78 SI_drawSun]: the symbol of a sun light at its position: the six line
/// segments of a star (static vertex table at 0x0869f240, twelve points of 12 bytes) drawn
/// five times with a 45-degree turn between (about y, x, x, y), then the lamp circles.
/// Class: copy.
pub fn draw_sun(m: &mut Mem, o: &mut dyn si_mem::Outside, light: u32) {
    const P: u32 = 0x0869_f240;
    o.pushmatrix(m);
    let (x, y, z) = (x87_f32(m.u32(light + 0x34)), x87_f32(m.u32(light + 0x38)), x87_f32(m.u32(light + 0x3c)));
    o.translate(m, x, y, z);
    o.pushmatrix(m);
    for turn in [Some(0x79), Some(0x78), Some(0x78), Some(0x79), None] {
        for seg in 0..6u32 {
            o.bgnline(m);
            o.v3f(m, P + 24 * seg);
            o.v3f(m, P + 24 * seg + 12);
            o.endline(m);
        }
        if let Some(axis) = turn {
            o.rot(m, 0x4234_0000, axis);
        }
    }
    o.popmatrix(m);
    lamp_circles(m, o);
    o.popmatrix(m);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The processor's `fld`/`fstp` of a 32-bit signalling NaN stores it back quiet (checked on
    /// real hardware; the emulator keeps the word as it was, so the vectors leave these out).
    #[test]
    fn x87_copy_quiets_a_signalling_nan() {
        assert_eq!(through_x87(0x7fa0_0000), 0x7fe0_0000);
        assert_eq!(through_x87(0xffa0_0001), 0xffe0_0001);
        assert_eq!(through_x87(0x7fc0_0000), 0x7fc0_0000);
        assert_eq!(through_x87(0x0000_0001), 0x0000_0001);
        assert_eq!(x87_f32(0x7fa0_0000), 0x7fe0_0000);
        assert_eq!(x87_f32(0x3e4c_cccd), 0x3e4c_cccd);
    }

    #[test]
    fn light_type_buttons() {
        let mut m = Mem::new();
        let mut o = si_mem::Trace::with(|_, _, _| si_mem::Ret::int(0));
        light_type(&mut m, &mut o, 0x6100_0000, 8);
        let states: Vec<u32> = o.calls.iter().map(|c| c.args[2]).collect();
        assert_eq!(states, [0, 0, 1, 0]);
    }
}
