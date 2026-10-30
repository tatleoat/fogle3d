// SPDX-License-Identifier: MIT
//! The writers of the text form, one per kind of element: `bin2ascii`'s own routines (they
//! are not in `soft`), each read from `bin2ascii`'s disassembly
//! (`re/tools/si-ascii/b2a_dis.py`), and the routines of `a_utilio.c` it shares with `soft`.
//!
//! Every writer prints through C's `fprintf`: a keyword with `"%s "`, an integer with
//! `"%d "` (or `"%ld "`, the same on this build), a real with `"%.7g "` (a single widened to
//! double), a name with `"'%s' "`; a new line is `"%s"` of `SI_indentString`, which
//! [`Out::set_indent`] keeps as a newline and three spaces per level. Field offsets in the
//! comments are those of the original's records.

use super::curves::CurveNames;
use crate::setup::Out;
use si_elemio::anim::{HierAnim, ItemData};
use si_elemio::camera::Camera;
use si_elemio::fcurve::{Fcurve, Key, StaticVal};
use si_elemio::lattice::Lattice;
use si_elemio::light::Light;
use si_hrc::{Material, Texture3D};

/// `SI_versionOutput` of `bin2ascii` (0x08650f94): 4.0, and nothing in the converter
/// changes it.
pub const VERSION_OUTPUT: f32 = 4.0;

/// `SI_versionOutput > 2.65` (the double at 0x0861e360 of `bin2ascii`): the light's area and
/// photon fields are written.
fn above(c: f64) -> bool {
    (VERSION_OUTPUT as f64) > c
}

impl Out {
    /// `"%d "`; `"%ld "` prints the same on this 32-bit build.
    fn long(&mut self, v: i32) {
        self.int(v);
    }

    /// [soft 0x083dc810 SI_valueAsciiWrite]: `"%.7g "` when the value has no function
    /// curve, nothing when it has one. The element writers always pass none.
    pub(crate) fn value(&mut self, v: f32, curve: Option<()>) {
        if curve.is_none() {
            self.g(v);
        }
    }

    /// [soft 0x083dccb8 si_positionAsciiWrite]: `STATIC` and the vector, or, when the fifth
    /// argument (a pointer to what the position follows) is set, `CONSTRAINT`, the sixth
    /// argument and the name `'NO_NAME'`. Every caller in the converter passes a null
    /// fifth argument.
    pub(crate) fn position(&mut self, v: &[f32; 3], follows: Option<i32>) {
        match follows {
            None => {
                self.word("STATIC");
                self.vector(v);
            }
            Some(kind) => {
                self.word("CONSTRAINT");
                self.int(kind);
                self.raw("'NO_NAME' ");
            }
        }
    }

    /// The four interpolation words of a curve and of a key; a code outside 0..4 prints
    /// nothing (the jump tables at 0x0861e33c and 0x0861e328 of `bin2ascii`).
    fn interpolation(&mut self, code: i16) {
        match code {
            0 => self.word("CONSTANT"),
            1 => self.word("LINEAR"),
            2 => self.word("SPLINE"),
            3 => self.word("SLOPE0"),
            4 => self.word("PLATEAU"),
            _ => {}
        }
    }

    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
}

/// `SI_materialAsciiWrite` (bin2ascii 0x0824ef60). 0 for no material, else 1.
///
/// Three fields no binary material file sets are printed from the record as
/// `SI_materialInitialize` (0x082889b0) leaves it: `ATTENUATION` (+0x6cc…+0x6d4, zero) and
/// `MOTION_TRANSPARENCY` (+0x6c8, 0.5). The name is printed up to its terminator; a name of
/// 30 bytes or more, which overruns the record's field in the original, is printed whole
/// here (no sample has one).
pub(crate) fn material_ascii_write(o: &mut Out, m: Option<&Material>) -> i16 {
    let Some(m) = m else { return 0 };
    o.set_indent(1);
    o.nl();
    o.word("MATERIAL_NAME");
    o.quoted(Some(&m.name));
    o.nl();
    o.word("SHADING_MODEL");
    o.int(m.shading as i32);
    o.nl();
    o.word("GLOW");
    o.int(m.glow as i32);
    o.nl();
    o.word("COC");
    o.g(m.glow_amount); // +0x2c, printed directly with "%.7g "
    o.nl();
    o.word("AMBIENT");
    o.g3(&m.ambient);
    o.nl();
    o.word("DIFFUSE");
    o.g3(&m.diffuse);
    o.nl();
    o.word("SPECULAR");
    o.g3(&m.specular);
    o.value(m.exponent, None);
    o.nl();
    o.word("ATTENUATION");
    o.g3(&[0.0; 3]);
    o.nl();
    o.word("TRANSPARENCY");
    o.value(m.transparency, None);
    o.value(m.x6bc, None);
    o.nl();
    o.word("REFLECTION");
    o.value(m.reflectivity, None);
    o.value(m.x6ac, None);
    o.nl();
    o.word("REFRACTION");
    o.value(m.refraction, None);
    o.nl();
    o.word("MOTION_TRANSPARENCY");
    o.value(0.5, None);
    o.nl();
    o.word("STATIC_BLUR");
    o.choice(m.blur != 0, "YES", "NO");
    o.nl();
    o.word("STATIC_BLUR_WIDTH");
    o.g(m.blur_width);
    o.nl();
    o.word("DECAY_VALUE");
    o.g(m.blur_decay);
    o.set_indent(-1);
    1
}

/// `SI_cameraAsciiWrite` (bin2ascii 0x080c9684). 0 for no camera, else 1.
///
/// The positions go through `si_positionAsciiWrite` with a null fifth argument, so a
/// position is always `STATIC` and its vector. The projection plane and the user data are
/// not written.
pub(crate) fn camera_ascii_write(o: &mut Out, c: Option<&Camera>) -> i16 {
    let Some(c) = c else { return 0 };
    o.set_indent(1);
    o.nl();
    o.word("POSITION");
    o.position(&c.position, None);
    o.nl();
    o.word("INTEREST");
    o.position(&c.interest, None);
    o.nl();
    o.word("ROLL");
    o.value(c.yaw_pitch_roll[2], None);
    o.nl();
    o.word("YAW_PITCH_ROLL");
    o.g3(&c.yaw_pitch_roll);
    o.nl();
    o.word("NEAR");
    o.value(c.near, None);
    o.nl();
    o.word("FAR");
    o.value(c.far, None);
    o.nl();
    o.word("FOCAL_LENGTH");
    o.value(c.focal_length, None);
    o.nl();
    o.word("F_STOP");
    o.value(c.f_stop, None);
    o.nl();
    o.word("FOCAL_DISTANCE");
    o.value(c.focal_distance, None);
    o.nl();
    o.word("FIELD_OF_VIEW");
    o.value(c.field_of_view, None);
    o.nl();
    o.word("DEPTH_OF_FIELD");
    match c.depth_of_field {
        0 => o.word("OFF"),
        1 => o.word("AUTOMATIC"),
        2 => o.word("CUSTOM"),
        _ => {}
    }
    o.nl();
    o.word("NEAR_FOCUS");
    o.value(c.near_focus, None);
    o.nl();
    o.word("FAR_FOCUS");
    o.value(c.far_focus, None);
    o.nl();
    o.word("OUT_OF_FOCUS");
    o.value(c.out_of_focus, None);
    o.nl();
    o.word("MAXIMUM_COC");
    o.value(c.maximum_coc, None);
    o.nl();
    o.word("ENLARGE");
    o.choice(c.enlarge != 0, "ACTIVE", "INACTIVE");
    o.set_indent(-1);
    1
}

/// The light's name as `printf("'%s'")` of the record prints it: the 30 bytes of the
/// field up to a zero, and when they hold none, on into the type at +0x1e (16 bits, low
/// byte first) and the zero word at +0x20.
fn light_name(l: &Light) -> Vec<u8> {
    let mut b = l.name.to_vec();
    b.extend_from_slice(&l.kind.to_le_bytes());
    b.push(0);
    let n = b.iter().position(|&x| x == 0).unwrap();
    b.truncate(n);
    b
}

/// `SI_lightAsciiWrite` (bin2ascii 0x080c7620). 0 for no light, else 1.
///
/// `EXPONENT` is written with the **binary** value writer, si_valueBinaryWrite (0x083fd4f0,
/// through `si-elemio`'s port): token 1 and the float, six bytes, big-endian, in the middle
/// of the text. Reproduced as the original does it.
pub(crate) fn light_ascii_write(o: &mut Out, l: Option<&Light>) -> i16 {
    let Some(l) = l else { return 0 };
    o.set_indent(1);
    o.nl();
    o.word("LIGHT_NAME");
    o.quoted(Some(&light_name(l)));
    o.nl();
    o.word("LIGHT_TYPE");
    match l.kind {
        1 => o.word("POINT"),
        2 => o.word("SPOT"),
        3 => o.word("INFINITE"),
        4 => o.word("SUN"),
        _ => {}
    }
    o.nl();
    o.word("COLOUR");
    o.g3(&l.colour);
    o.nl();
    o.word("FALL_OFF_START");
    o.value(l.fall_off_start, None);
    o.nl();
    o.word("FALL_OFF_END");
    o.value(l.fall_off_end, None);
    o.nl();
    o.word("SHADOW_FACTOR");
    o.value(l.shadow_factor, None);
    o.nl();
    o.word("POSITION");
    o.position(&l.position, None);
    o.nl();
    o.word("INTEREST");
    o.position(&l.interest, None);
    if l.kind == 2 {
        o.nl();
        o.word("CONE_ANGLE");
        o.value(l.cone_angle, None);
        o.nl();
        o.word("SPREAD_ANGLE");
        o.value(l.spread_angle, None);
    }
    o.nl();
    o.word("SHADOW_TYPE");
    o.int(l.shadow_type as i32);
    o.nl();
    o.word("SHADOW_MAP_RES");
    o.int(l.shadow_map_res);
    o.nl();
    o.word("SHADOW_FILTER_SIZE");
    o.int(l.shadow_filter_size);
    o.nl();
    o.word("SHADOW_FILTER_STEP");
    o.int(l.shadow_filter_step);
    o.nl();
    o.word("SHADOW_FADE_START");
    o.value(l.shadow_fade_start, None);
    o.nl();
    o.word("SHADOW_FADE_RATE");
    o.value(l.shadow_fade_rate, None);
    o.nl();
    o.word("SELECTIVE");
    o.int(l.selective[0]);
    o.int(l.selective[1]);
    if l.kind == 4 {
        // the original reads the sun record through +0x16c without looking at it; a sun
        // light always has one (the allocator makes it)
        if let Some(s) = l.sun.as_ref() {
            o.nl();
            o.word("SUN_NAME");
            o.quoted(Some(&s.name));
            o.nl();
            o.word("SUN_LATITUDE");
            o.int(s.latitude as i16 as i32); // a 16-bit read of the 32-bit field
            o.nl();
            o.word("SUN_LONGITUDE");
            o.int(s.longitude as i16 as i32);
            o.nl();
            o.word("SUN_DAYST");
            o.choice(s.daylight_saving != 0, "YES", "NO");
            o.nl();
            o.word("SUN_DATE_TIME");
            o.long(s.date_time);
        }
    }
    if above(2.65) {
        o.nl();
        o.word("AREA_LIGHT");
        o.int(l.area_light as i32);
        o.nl();
        o.word("AREA_TYPE");
        o.int(l.area_type);
        o.nl();
        o.word("AREA_U_SAMPLE");
        o.int(l.area_u_sample);
        o.nl();
        o.word("AREA_V_SAMPLE");
        o.int(l.area_v_sample);
        o.nl();
        o.word("AREA_GEOMETRY");
        let g = &l.area_geometry;
        match l.area_type {
            0 => {
                for p in 0..4 {
                    o.g3(&[g[3 * p], g[3 * p + 1], g[3 * p + 2]]);
                }
            }
            1 => o.value(g[0], None),
            2 => {
                o.g3(&[g[0], g[1], g[2]]);
                o.value(g[3], None);
            }
            _ => {}
        }
        o.nl();
        o.word("AREA_LOCAL_COORDS");
        o.int(l.area_local_coords as i32);
        o.nl();
        o.word("AREA_SCALE");
        o.g3(&l.area_scale);
        o.nl();
        o.word("AREA_ROTATION");
        o.g3(&l.area_rotation);
        o.nl();
        o.word("AREA_SELECTED");
        o.int(l.area_selected as i32);
        o.nl();
        o.word("AREA_ROTATION_MAT");
        for v in l.area_rotation_mat {
            o.value(v, None);
        }
        o.nl();
        o.word("DIFFUSE");
        o.int(l.diffuse as i32);
        o.nl();
        o.word("ENERGY");
        o.g3(&l.energy);
        o.nl();
        o.word("EXPONENT");
        let mut w = si_elemio::io::Writer::new();
        w.value(l.exponent);
        o.bytes(&w.out);
        o.nl();
        o.word("CAUSTIC_PHOTON");
        o.long(l.caustic_photon);
        o.nl();
        o.word("GLOBILLUM_PHOTON");
        o.long(l.globillum_photon);
        o.nl();
        o.word("CAUSTIC_ENABLE");
        o.choice(l.caustic_enable != 0, "YES", "NO");
        o.nl();
        o.word("GLOBILLUM_ENABLE");
        o.choice(l.globillum_enable != 0, "YES", "NO");
    }
    o.set_indent(-1);
    1
}

/// `SI_latticeAsciiWrite` (bin2ascii 0x080c6ef0): the first `count` lattices of the list.
/// 0, with nothing written, for a count of zero or less or no list; else 1.
///
/// The point count is the 32-bit product of the three 16-bit counts; `KEYS` and the points
/// are written only when it is positive, the closing newline always.
pub(crate) fn lattice_ascii_write(o: &mut Out, count: i32, lattices: Option<&[Lattice]>) -> i16 {
    let Some(list) = lattices else { return 0 };
    if count <= 0 {
        return 0;
    }
    o.set_indent(1);
    for i in 0..count as usize {
        // the original reads `count` pointers from the list; the reader gives no more
        // lattices than it counts
        let Some(l) = list.get(i) else { break };
        o.nl();
        o.word("ACTIVE");
        o.choice(l.active != 0, "YES", "NO");
        o.nl();
        o.word("NB_KEYS");
        for k in l.nb_keys {
            o.int(k as i32);
        }
        o.nl();
        o.word("INTERPOLATION");
        for k in l.interpolation {
            o.int(k);
        }
        o.nl();
        o.word("MINIMUM");
        o.vector(&l.minimum);
        o.nl();
        o.word("MAXIMUM");
        o.vector(&l.maximum);
        o.nl();
        o.word("SCALING");
        o.vector(&l.scaling);
        let n = l.key_count();
        if n > 0 {
            o.nl();
            o.word("KEYS");
            for k in 0..n as usize {
                // the reader keeps as many points as the counts ask for (or the file holds)
                let p = l.keys.get(k).copied().unwrap_or([0.0; 3]);
                o.vector(&p);
            }
        }
        o.raw("\n");
    }
    o.set_indent(-1);
    1
}

/// `SI_texture3DAsciiWrite` (bin2ascii 0x0824f8ec). 0 for no texture, else 1.
///
/// One keyword a line, in the record's order but for the first five (`STRENGTH` +0x18 is
/// printed before `NB_ITER` +0x1c and `SPACING` +0x10). `TYPE` (+0xc) and `NB_ITER` (+0x1c)
/// are 32-bit fields of which the writer prints the low 16 bits (`movsx` of a word). Each
/// colour of the ramp is `COLOUR_<n>` (three values) and `ALPHA_<n>` (the fourth float);
/// `BLEND` is +0x28, `USE_ALPHA` the mask (+0x24), `USE_EFFECT` the channel (+0x2c). The
/// text ends after `USE_EFFECT`'s value, without a newline.
///
/// `name` is the pointer at +0: `None` when it is null (`SI_texture3DInitialize`, 0x082b8ab0,
/// clears it, and only the binary reader's token 1 sets it), printed `'(null)'` as glibc
/// prints a null `%s`. `REFLECTION` (+0x114) is the one printed field the initialiser does
/// not set; the binary reader sets it with token 0x13. For a file without that token the
/// original prints what `SI_malloc` left there and the port 0; all 125 sample texts agree
/// with the port, whether or not some lack the token (not looked at).
pub(crate) fn texture3d_ascii_write(o: &mut Out, x: Option<&Texture3D>, name: Option<&[u8]>) -> i16 {
    let Some(x) = x else { return 0 };
    o.set_indent(1);
    o.nl();
    o.word("TEXTURE_3D_NAME");
    o.quoted(name);
    o.nl();
    o.word("TYPE");
    o.int(x.kind as i16 as i32);
    o.nl();
    o.word("STRENGTH");
    o.value(x.strength, None);
    o.nl();
    o.word("NB_ITER");
    o.int(x.iterations as i16 as i32);
    o.nl();
    o.word("SPACING");
    o.value(x.spacing, None);
    const COLOUR: [&str; 5] = ["COLOUR_1", "COLOUR_2", "COLOUR_3", "COLOUR_4", "COLOUR_5"];
    const ALPHA: [&str; 5] = ["ALPHA_1", "ALPHA_2", "ALPHA_3", "ALPHA_4", "ALPHA_5"];
    for (i, c) in x.colours.iter().enumerate() {
        o.nl();
        o.word(COLOUR[i]);
        o.g3(&[c[0], c[1], c[2]]); // SI_value3AsciiWrite with no curves
        o.nl();
        o.word(ALPHA[i]);
        o.value(c[3], None);
    }
    for (w, v) in [("COLOUR_POS_1", x.factors[0]), ("COLOUR_POS_2", x.factors[1]), ("COLOUR_POS_3", x.factors[2])] {
        o.nl();
        o.word(w);
        o.value(v, None);
    }
    for (w, v) in [("SCALING", &x.scaling), ("ROTATION", &x.rotation), ("TRANSLATION", &x.translation)] {
        o.nl();
        o.word(w);
        o.g3(v);
    }
    for (w, v) in [
        ("AMBIENT", x.ambient),
        ("DIFFUSE", x.diffuse),
        ("SPECULAR", x.specular),
        ("ROUGHNESS", x.roughness),
        ("BLEND", x.blending),
        ("TRANSPARENCY", x.transparency),
        ("REFLECTION", x.reflection),
        ("POWER", x.power),
        ("TAPER", x.taper),
    ] {
        o.nl();
        o.word(w);
        o.value(v, None);
    }
    o.nl();
    o.word("USE_ALPHA");
    o.int(x.mask as i32);
    o.nl();
    o.word("USE_EFFECT");
    o.int(x.channel as i32);
    o.set_indent(-1);
    1
}

/// `fcvkeyAsciiWrite` (bin2ascii 0x080c5800): one key, one level in, after an empty line.
/// `SPLINE_INDEX` only when the whole key-type word is 2 (`KEYPATH` without flags);
/// `KEYTYPE` names the type in the word's low four bits.
pub(crate) fn fcvkey_ascii_write(o: &mut Out, k: &Key) {
    o.set_indent(1);
    o.nl();
    o.nl();
    o.word("TIME");
    o.g(k.time);
    o.raw("    ");
    o.word("VALUE");
    o.g(k.value);
    o.nl();
    o.word("LEFT_SLOPE");
    o.g(k.left_slope);
    o.raw("    ");
    o.word("RIGHT_SLOPE");
    o.g(k.right_slope);
    o.nl();
    o.word("INTERPOLATION");
    o.interpolation(k.interpolation);
    o.raw("    ");
    o.word("FLAG");
    o.int(k.flag as i32);
    if k.key_type == 2 {
        o.raw("    ");
        o.word("SPLINE_INDEX");
        o.int(k.spline_index as i32);
    }
    o.nl();
    o.word("KEYTYPE");
    match k.key_type & 0xf {
        0 => o.word("FREEFORM"),
        1 => o.word("KEYFRAME"),
        2 => o.word("KEYPATH"),
        3 => o.word("RAWDATA"),
        _ => {}
    }
    o.set_indent(-1);
}

/// `fcurveAsciiWrite` (bin2ascii 0x080c5af0): a function curve, one level in. 0 for no
/// curve. The type is printed by name ([`CurveNames::name`]) and by number.
///
/// The keys are written as many as the curve's count (+0x0c) says. The value form keeps the
/// keys the file held; were the count larger (a file cut short), the original would print
/// the blank keys of its array, here blank keys too.
pub(crate) fn fcurve_ascii_write(o: &mut Out, c: Option<&Fcurve>, names: &mut CurveNames) -> i16 {
    let Some(c) = c else { return 0 };
    o.set_indent(1);
    o.nl();
    o.word("CURVE_TYPE");
    let name = names.name(c.curve_type);
    o.word(&String::from_utf8_lossy(name.as_deref().unwrap_or(b"(null)")));
    o.int(c.curve_type);
    o.nl();
    o.word("PRE_EXTRAPOLATION");
    o.int(c.pre_extrapolation as i32);
    o.raw("    ");
    o.word("POST_EXTRAPOLATION");
    o.int(c.post_extrapolation as i32);
    o.nl();
    o.word("FLAGS");
    o.int(c.flags as i32);
    o.nl();
    o.word("INTERPOLATION");
    o.interpolation(c.interpolation);
    o.nl();
    o.word("KEYS");
    o.int(c.count);
    for i in 0..c.count.max(0) as usize {
        let k = c.keys.get(i).copied().unwrap_or_default();
        fcvkey_ascii_write(o, &k);
    }
    o.nl();
    o.set_indent(-1);
    1
}

/// `staticvalAsciiWrite` (bin2ascii 0x080c5ea0): a static value on the item's line.
pub(crate) fn staticval_ascii_write(o: &mut Out, v: &StaticVal) {
    o.word("TYPE");
    o.int(v.curve_type);
    o.raw("    ");
    o.word("VALUE");
    o.g(v.value);
    o.raw("    ");
    o.word("SVAL_END");
    o.nl();
}

/// [soft 0x08138d70 SI_hieranimFindElemIndex]: the index of the first element of that name,
/// or −1.
pub fn hieranim_find_elem_index(ha: &HierAnim, name: &[u8]) -> i32 {
    ha.elems.iter().position(|e| e.name == name).map_or(-1, |i| i as i32)
}

/// The body of `si_elemdictWriteAscAnimations` (bin2ascii 0x080c6b60) after its header: the
/// element list (`HIER_CHAPNAME` is the chapter's index in the element dictionary), then
/// every item of every element.
///
/// An item's `OWNER_ID` is the index of the first element with its element's name. Its
/// header has `ITM_TYPE` (the output version is above 2.62, the double at 0x0861e358), and
/// `ACT_RELID` and `ACT_OCCID` when the first is not negative. A curve item is followed by
/// a new line, a static value item ends its own line; any other type writes nothing more.
pub(crate) fn hieranim_ascii_write(o: &mut Out, ha: &HierAnim, chapter: i32, names: &mut CurveNames) {
    o.set_indent(1);
    o.nl();
    o.word("HIER_CHAPNAME");
    o.int(chapter);
    o.nl();
    for (i, e) in ha.elems.iter().enumerate() {
        o.word("HIER_NAME");
        o.quoted(Some(&e.name));
        o.int(i as i32);
        o.nl();
        o.word("HIER_FTH");
        o.int(e.father);
        o.nl();
    }
    o.word("HIER_END");
    o.nl();
    o.nl();
    for e in &ha.elems {
        let owner = hieranim_find_elem_index(ha, &e.name);
        for it in &e.items {
            o.word("OWNER_ID");
            o.int(owner);
            if above(2.62) {
                o.word("ITM_TYPE");
                o.int(it.kind as i32);
                if it.act_rel_id >= 0 {
                    o.word("ACT_RELID");
                    o.int(it.act_rel_id);
                    o.word("ACT_OCCID");
                    o.int(it.act_occ_id as i32);
                }
            }
            o.word("ANIM_END");
            match (it.kind as u16, &it.data) {
                (0, d) => {
                    let c = if let ItemData::Curve(c) = d { Some(c) } else { None };
                    fcurve_ascii_write(o, c, names);
                    o.nl();
                }
                // a type-1 item always holds a static value (the reader makes one for it)
                (1, ItemData::Static(v)) => staticval_ascii_write(o, v),
                _ => {}
            }
        }
    }
    o.set_indent(-1);
}
