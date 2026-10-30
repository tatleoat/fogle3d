// SPDX-License-Identifier: MIT
//! Function curves, their keys and static values as the element files store them:
//! finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `fcurveio.c` (readers, static values), `utilio.c`
//! (writers).
//!
//! Field names: the keywords the vendor's converter `bin2ascii` prints (`fcurveAsciiWrite`,
//! `fcvkeyAsciiWrite`, `staticvalAsciiWrite`, read from its disassembly); the code names are
//! from the SDK header `SAA_genTypes.h`.

use crate::io::{fread_u16, End, Reader, Writer};

/// Interpolation codes of a curve and of a key (`SAA_FcurveInterpType`; the converter prints
/// `CONSTANT`, `LINEAR`, `SPLINE`, `SLOPE0`, `PLATEAU`).
pub const INTERP_CONSTANT: i16 = 0;
pub const INTERP_LINEAR: i16 = 1;
/// `SAA_INT_HERMITE`; `SPLINE` in the converter's output.
pub const INTERP_SPLINE: i16 = 2;
pub const INTERP_SLOPE0: i16 = 3;
pub const INTERP_PLATEAU: i16 = 4;

/// Extrapolation codes before the first and after the last key (`SAA_FcurveExtrapType`).
pub const EXTRAP_CONSTANT: i16 = 0;
/// `SAA_EXT_GRADT`: continue with the end slope.
pub const EXTRAP_GRADIENT: i16 = 1;
pub const EXTRAP_CYCLE: i16 = 2;

/// Key types (`SAA_FKeyType`; the converter prints `FREEFORM`, `KEYFRAME`, `KEYPATH`,
/// `RAWDATA`).
pub const KEY_FREE_FORM: u16 = 0;
pub const KEY_KEY_FRAME: u16 = 1;
pub const KEY_KEY_PATH: u16 = 2;
pub const KEY_RAW: u16 = 3;

/// Bits of a key's `FLAG` (`SAA_FKeySlopeType` in the SDK manual: 0 is an automatic slope).
pub const KEY_SLOPE_USER: i16 = 1;
/// The right slope differs from the left one.
pub const KEY_SLOPE_DOUBLE: i16 = 2;

/// One key of a function curve (`SI_Fcvkey`, 36 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    /// +0x00 `TIME`, in seconds
    pub time: f32,
    /// +0x04 `VALUE`
    pub value: f32,
    /// +0x08 `RIGHT_SLOPE`
    pub right_slope: f32,
    /// +0x0c `FLAG`: slope bits ([`KEY_SLOPE_USER`], [`KEY_SLOPE_DOUBLE`])
    pub flag: i16,
    /// +0x0e `INTERPOLATION` of the segment after this key
    pub interpolation: i16,
    /// +0x14: zeroed by the curve reader after each key (+0x10, the pointer back to the
    /// curve, is not kept here)
    pub w_14: i16,
    /// +0x18 `LEFT_SLOPE`
    pub left_slope: f32,
    /// +0x1c `KEYTYPE`: the type in the low four bits ([`KEY_FREE_FORM`] …); the curve code
    /// keeps flags in bits 0x10 and 0x20 of the same word, and the file stores the word whole
    pub key_type: u16,
    /// +0x1e `SPLINE_INDEX`: for a `KEYPATH` key, a point of the path's spline; −1 otherwise
    pub spline_index: i16,
    /// +0x20: −1 unless the file has token 9 for the key
    pub w_20: i16,
}

impl Default for Key {
    fn default() -> Self {
        Key::new()
    }
}

impl Key {
    /// A blank key [soft 0x0820bc50 SI_fkeyInitialize]: everything zero, the two words at
    /// +0x1e and +0x20 set to −1. The original does not touch +0x0e (the interpolation): in
    /// the curve reader's freshly allocated array it is stale memory until token 5 sets it.
    /// Here it is zero.
    pub fn new() -> Key {
        Key {
            time: 0.0,
            value: 0.0,
            right_slope: 0.0,
            flag: 0,
            interpolation: 0,
            w_14: 0,
            left_slope: 0.0,
            key_type: 0,
            spline_index: -1,
            w_20: -1,
        }
    }
}

/// Reads one key [soft 0x083facb0 fcvkeyBinaryRead]: the key is blanked, then
///
/// | token | field |
/// |---|---|
/// | 0 | end of the key |
/// | 1 | float: `TIME` |
/// | 2 | float: `VALUE` |
/// | 3 | float: `LEFT_SLOPE` |
/// | 4 | float: `RIGHT_SLOPE` |
/// | 5 | 16-bit: `INTERPOLATION` |
/// | 6 | 16-bit: `FLAG` |
/// | 7 | 16-bit: `KEYTYPE` |
/// | 8 | 16-bit: `SPLINE_INDEX` |
/// | 9 | 16-bit: the word at +0x20 |
/// | other | ignored; nothing is read for it |
///
/// Token 7 in a file up to version 2.64918 (`SI_versionInput` against the double at
/// 0x0865c538): if the stored word has bit 0x10 set, the key type gets bit 0x20 set and the
/// stored word is otherwise dropped; without that bit the word is stored as it is.
///
/// With no key to fill (`key` is `None`) the fields are read and dropped.
pub fn read_key(r: &mut Reader, mut key: Option<&mut Key>) -> End {
    if let Some(k) = key.as_deref_mut() {
        *k = Key::new();
    }
    loop {
        let ok = fread_u16(&mut r.s, &mut r.st.token_fcurveio);
        let token = r.st.token_fcurveio;
        r.note("key", token, ok);
        if token == 0 {
            return End::Zero;
        }
        if !ok {
            return End::Eof;
        }
        match token {
            1 => {
                let v = r.fvalue();
                if let Some(k) = key.as_deref_mut() {
                    k.time = v;
                }
            }
            2 => {
                let v = r.fvalue();
                if let Some(k) = key.as_deref_mut() {
                    k.value = v;
                }
            }
            3 => {
                let v = r.fvalue();
                if let Some(k) = key.as_deref_mut() {
                    k.left_slope = v;
                }
            }
            4 => {
                let v = r.fvalue();
                if let Some(k) = key.as_deref_mut() {
                    k.right_slope = v;
                }
            }
            5 => {
                let v = r.svalue();
                r.note("key: interpolation", v as u16, true);
                if let Some(k) = key.as_deref_mut() {
                    k.interpolation = v;
                }
            }
            6 => {
                let v = r.svalue();
                r.note("key: flag", v as u16, true);
                if let Some(k) = key.as_deref_mut() {
                    k.flag = v;
                }
            }
            7 => {
                let v = r.svalue();
                r.note("key: type", v as u16, true);
                if let Some(k) = key.as_deref_mut() {
                    if (r.st.version_input as f64) <= 2.64918 && v & 0x10 != 0 {
                        k.key_type |= 0x20; // a byte OR on the low byte of the field
                    } else {
                        k.key_type = v as u16;
                    }
                }
            }
            8 => {
                let v = r.svalue();
                if let Some(k) = key.as_deref_mut() {
                    k.spline_index = v;
                }
            }
            9 => {
                let v = r.svalue();
                if let Some(k) = key.as_deref_mut() {
                    k.w_20 = v;
                }
            }
            _ => {}
        }
    }
}

/// Writes one key [soft 0x083fd7ac fcvkeyBinaryWrite]: tokens 1 to 6, then token 8 with the
/// spline index if the key type word is exactly 2 (a `KEYPATH` key with no flag bit set),
/// token 7, then token 9 if the word at +0x20 is not −1, and the zero token.
pub fn write_key(w: &mut Writer, k: &Key) {
    w.token_utilio(1);
    w.fvalue(k.time);
    w.token_utilio(2);
    w.fvalue(k.value);
    w.token_utilio(3);
    w.fvalue(k.left_slope);
    w.token_utilio(4);
    w.fvalue(k.right_slope);
    w.token_utilio(5);
    w.svalue(k.interpolation);
    w.token_utilio(6);
    w.svalue(k.flag);
    if k.key_type == KEY_KEY_PATH {
        w.token_utilio(8);
        w.svalue(k.spline_index);
    }
    w.token_utilio(7);
    w.svalue(k.key_type as i16);
    if k.w_20 != -1 {
        w.token_utilio(9);
        w.svalue(k.w_20);
    }
    w.token_utilio(0);
}

/// A function curve (`fcurve`, 100 bytes), as far as a file reaches into it. The rest of the
/// record is the evaluator's working state and belongs with `fcurve.c`.
#[derive(Clone, Debug, PartialEq)]
pub struct Fcurve {
    /// +0x00 `CURVE_TYPE`: which attribute the curve drives (the program's table of curve
    /// types; 16 bits in the file)
    pub curve_type: i32,
    /// +0x0c `KEYS`: the number of keys the curve says it has. It is a field of its own in
    /// the file, read before the keys; `keys` holds the keys actually read.
    pub count: i32,
    /// +0x08 the keys
    pub keys: Vec<Key>,
    /// +0x18 `PRE_EXTRAPOLATION`
    pub pre_extrapolation: i16,
    /// +0x1a `POST_EXTRAPOLATION`
    pub post_extrapolation: i16,
    /// +0x1c `FLAGS`
    pub flags: i16,
    /// +0x1e `INTERPOLATION`: the default for new segments
    pub interpolation: i16,
}

impl Fcurve {
    /// The file-borne fields of the curve the reader starts from: `SI_fcurveAllocate(0, …)`
    /// (0x0820a810, not ported here: it initialises the whole record). Type 0, no keys,
    /// extrapolations and flags zero, and for the interpolation the program's current
    /// default (`SI_glbAnim` + 0x30, 0x087814b0), which the caller supplies.
    pub fn blank(default_interpolation: i16) -> Fcurve {
        Fcurve { curve_type: 0, count: 0, keys: Vec::new(), pre_extrapolation: 0, post_extrapolation: 0, flags: 0, interpolation: default_interpolation }
    }
}

/// Reads a function curve [soft 0x083faeb0 SI_fcurveBinaryRead].
///
/// | token | field |
/// |---|---|
/// | 0 | end of the curve |
/// | 1 | 16-bit: `CURVE_TYPE` |
/// | 2 | 16-bit: number of keys (files up to version 2.62) |
/// | 3 | 16-bit: `PRE_EXTRAPOLATION` |
/// | 4 | 16-bit: `POST_EXTRAPOLATION` |
/// | 5 | 16-bit: `FLAGS` |
/// | 6 | 16-bit: `INTERPOLATION` |
/// | 7 | the keys, one after another, as many as the count read so far |
/// | 8 | 32-bit: number of keys |
/// | other | ignored; nothing is read for it |
///
/// With `keep` false (the original's null destination) everything is read and dropped and
/// `None` comes back.
///
/// The original allocates room for the count (rounded up to a multiple of eight keys) before
/// reading the keys. Here the count is first checked against the file: a key takes at least
/// two bytes, so no more keys are read than half the bytes left; if that cuts the count the
/// reader's `clamped` is set. (The original would go on, each further key coming out blank.)
pub fn read_fcurve(r: &mut Reader, keep: bool, default_interpolation: i16) -> (Option<Fcurve>, End) {
    let mut c = if keep { Some(Fcurve::blank(default_interpolation)) } else { None };
    let mut pending = 0i32; // the count, kept aside for a curve that is not kept
    loop {
        let ok = fread_u16(&mut r.s, &mut r.st.token_fcurveio);
        let token = r.st.token_fcurveio;
        r.note("fcurve", token, ok);
        if token == 0 {
            return (c, End::Zero);
        }
        if !ok {
            return (c, End::Eof);
        }
        match token {
            1 => {
                let v = r.svalue();
                r.note("curve: type", v as u16, true);
                if let Some(c) = c.as_mut() {
                    c.curve_type = v as i32;
                }
            }
            2 => {
                let v = r.svalue();
                if let Some(c) = c.as_mut() {
                    c.count = v as i32;
                }
                pending = v as i32;
            }
            3 => {
                let v = r.svalue();
                r.note("curve: pre-extrapolation", v as u16, true);
                if let Some(c) = c.as_mut() {
                    c.pre_extrapolation = v;
                }
            }
            4 => {
                let v = r.svalue();
                r.note("curve: post-extrapolation", v as u16, true);
                if let Some(c) = c.as_mut() {
                    c.post_extrapolation = v;
                }
            }
            5 => {
                let v = r.svalue();
                r.note("curve: flags", v as u16, true);
                if let Some(c) = c.as_mut() {
                    c.flags = v;
                }
            }
            6 => {
                let v = r.svalue();
                r.note("curve: interpolation", v as u16, true);
                if let Some(c) = c.as_mut() {
                    c.interpolation = v;
                }
            }
            7 => {
                let n = c.as_ref().map_or(pending, |c| c.count).max(0) as usize;
                let room = (r.s.len() - r.s.tell()) / 2;
                if n > room {
                    r.clamped = true;
                }
                let n = n.min(room);
                let mut keys = Vec::with_capacity(if keep { n } else { 0 });
                for _ in 0..n {
                    if keep {
                        let mut k = Key::new();
                        let end = read_key(r, Some(&mut k));
                        k.w_14 = 0;
                        keys.push(k);
                        if end == End::Eof {
                            if let Some(c) = c.as_mut() {
                                c.keys = keys;
                            }
                            return (c, End::Eof);
                        }
                    } else if read_key(r, None) == End::Eof {
                        return (c, End::Eof);
                    }
                }
                if let Some(c) = c.as_mut() {
                    // a second token 7 replaces the array; the original leaks the first
                    c.keys = keys;
                }
            }
            8 => {
                let v = r.ivalue();
                if let Some(c) = c.as_mut() {
                    c.count = v;
                }
                pending = v;
            }
            _ => {}
        }
    }
}

/// Writes a function curve [soft 0x083fd9c8 SI_fcurveBinaryWrite]: the type (token 1, its
/// low 16 bits); the number of keys as 32 bits under token 8, or, when the output version is
/// not above 2.62 (the double at 0x0865c5b8), as 16 bits under token 2; tokens 3 to 6;
/// token 7 and the keys; the zero token.
///
/// The original writes `count` keys from the curve's array. Here a curve whose `keys` is
/// shorter than its `count` gets blank keys for the rest.
pub fn write_fcurve(w: &mut Writer, c: &Fcurve) {
    w.token_utilio(1);
    w.svalue(c.curve_type as i16);
    if w.version_output as f64 > 2.62 {
        w.token_utilio(8);
        w.ivalue(c.count);
    } else {
        w.token_utilio(2);
        w.svalue(c.count as i16);
    }
    w.token_utilio(3);
    w.svalue(c.pre_extrapolation);
    w.token_utilio(4);
    w.svalue(c.post_extrapolation);
    w.token_utilio(5);
    w.svalue(c.flags);
    w.token_utilio(6);
    w.svalue(c.interpolation);
    w.token_utilio(7);
    let blank = Key::new();
    for i in 0..c.count.max(0) as usize {
        write_key(w, c.keys.get(i).unwrap_or(&blank));
    }
    w.token_utilio(0);
}

/// Curve types whose values an old, header-less animation file holds in radians.
const ANGLE_CURVE_TYPES: [i32; 20] =
    [0x19, 0x1a, 0x0b, 0x0c, 0x2c, 0x48, 0x5e, 0x5f, 0x60, 0x6e, 0x75, 0xa6, 0xa7, 0xa8, 0x7c, 0x7d, 0x7e, 0x85, 0x86, 0x87];

/// Turns the values and slopes of an angle curve from radians into degrees
/// [soft 0x0820aa60 SI_fcvConvertAngleUnitToDegree]: for the twenty curve types that are
/// angles, each key's value, left slope and right slope is multiplied by 180/π. Applied to
/// the curves of an animation file whose header was not accepted.
///
/// The factor is the double 57.29577951308232 (0x0864d478), loaded on the x87 stack; each
/// product is rounded to the register's 64 bits and then, by the store, to a single
/// ([`crate::x87::mul_double_single`]).
pub fn convert_angle_unit_to_degree(c: &mut Fcurve) {
    if !ANGLE_CURVE_TYPES.contains(&c.curve_type) {
        return;
    }
    const K: f64 = 57.295_779_513_082_32;
    let n = c.count.max(0) as usize;
    for k in c.keys.iter_mut().take(n) {
        k.value = crate::x87::mul_double_single(K, k.value);
        k.left_slope = crate::x87::mul_double_single(K, k.left_slope);
        k.right_slope = crate::x87::mul_double_single(K, k.right_slope);
    }
}

/// A value that is not animated (`SI_HaStaticVal`, 8 bytes): what a struct key file holds
/// in place of curves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StaticVal {
    /// +0x00 `TYPE`: the curve type the value belongs to (16 bits in the file)
    pub curve_type: i32,
    /// +0x04 `VALUE`
    pub value: f32,
}

/// Reads a static value [soft 0x083fb228 SI_staticvalBinaryRead]: a zeroed record, then
/// token 1 (16-bit: `TYPE`) and token 2 (float: `VALUE`) in any order up to the zero token;
/// other tokens are ignored and nothing is read for them.
pub fn read_static_val(r: &mut Reader) -> (StaticVal, End) {
    let mut v = StaticVal::default();
    loop {
        let ok = fread_u16(&mut r.s, &mut r.st.token_fcurveio);
        let token = r.st.token_fcurveio;
        r.note("static value", token, ok);
        if token == 0 {
            return (v, End::Zero);
        }
        if !ok {
            return (v, End::Eof);
        }
        match token {
            1 => {
                v.curve_type = r.svalue() as i32;
                r.note("static value: type", v.curve_type as u16, true);
            }
            2 => v.value = r.fvalue(),
            _ => {}
        }
    }
}

/// Writes a static value [soft 0x083fb188 SI_staticvalBinaryWrite]: tokens 1 and 2 and the
/// zero token; for no value, the zero token alone.
pub fn write_static_val(w: &mut Writer, v: Option<&StaticVal>) {
    if let Some(v) = v {
        w.token_fcurveio(1);
        w.svalue(v.curve_type as i16);
        w.token_fcurveio(2);
        w.fvalue(v.value);
    }
    w.token_fcurveio(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> Fcurve {
        let key = |t: f32, v: f32, kt: u16| Key {
            time: t,
            value: v,
            right_slope: 0.5,
            flag: 3,
            interpolation: INTERP_SPLINE,
            w_14: 0,
            left_slope: -0.5,
            key_type: kt,
            spline_index: if kt == KEY_KEY_PATH { 4 } else { -1 },
            w_20: if kt == KEY_RAW { 7 } else { -1 },
        };
        Fcurve {
            curve_type: 7,
            count: 3,
            keys: vec![key(0.0, 1.0, KEY_FREE_FORM), key(1.0, 2.0, KEY_KEY_PATH), key(2.0, -1.0, KEY_RAW)],
            pre_extrapolation: EXTRAP_CYCLE,
            post_extrapolation: EXTRAP_GRADIENT,
            flags: 0,
            interpolation: INTERP_LINEAR,
        }
    }

    #[test]
    fn curve_round_trip_both_count_forms() {
        let c = curve();
        for (version, count_bytes) in [(4.0f32, 6), (2.62, 4)] {
            let mut w = Writer::with_version(version);
            write_fcurve(&mut w, &c);
            // type 4, count, four words 16, token 7, keys 38 + 42 + 42, end
            assert_eq!(w.out.len(), 4 + count_bytes + 16 + 2 + 38 + 42 + 42 + 2);
            let mut r = Reader::new(&w.out);
            let (got, end) = read_fcurve(&mut r, true, 9);
            assert_eq!((got, end, r.s.tell()), (Some(c.clone()), End::Zero, w.out.len()));
            // read and dropped
            let mut r = Reader::new(&w.out);
            assert_eq!(read_fcurve(&mut r, false, 9), (None, End::Zero));
            assert_eq!(r.s.tell(), w.out.len());
        }
        // 2.62 as a float is below the double 2.62: the short count
        assert!((2.62f32 as f64) < 2.62);
    }

    #[test]
    fn defaults_and_missing_tokens() {
        // a curve with nothing but its end
        let (c, end) = read_fcurve(&mut Reader::new(&[0, 0]), true, 2);
        assert_eq!((c, end), (Some(Fcurve::blank(2)), End::Zero));
        // a key with only a value
        let mut k = Key { time: 9.0, ..Key::new() };
        assert_eq!(read_key(&mut Reader::new(&[0, 2, 0x3f, 0x80, 0, 0, 0, 0]), Some(&mut k)), End::Zero);
        assert_eq!(k, Key { value: 1.0, ..Key::new() });
        assert_eq!((k.spline_index, k.w_20), (-1, -1));
    }

    #[test]
    fn old_key_type_word() {
        let bytes = [0, 7, 0, 0x13, 0, 0];
        let mut k = Key::new();
        let mut r = Reader::new(&bytes);
        r.st.version_input = 2.64;
        read_key(&mut r, Some(&mut k));
        assert_eq!(k.key_type, 0x20);
        let mut r = Reader::new(&bytes);
        r.st.version_input = 2.65;
        read_key(&mut r, Some(&mut k));
        assert_eq!(k.key_type, 0x13);
        // without bit 0x10 the old word is stored
        let mut r = Reader::new(&[0, 7, 0, 3, 0, 0]);
        r.st.version_input = 2.6;
        read_key(&mut r, Some(&mut k));
        assert_eq!(k.key_type, 3);
    }

    #[test]
    fn a_count_beyond_the_file_and_a_cut_key() {
        // count 1000, then one whole key and the start of another
        let mut b = vec![0, 8, 0, 0, 3, 0xe8, 0, 7];
        b.extend_from_slice(&[0, 1, 0x40, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&[0, 2, 0x40]);
        let mut r = Reader::new(&b);
        let (c, end) = read_fcurve(&mut r, true, 2);
        let c = c.unwrap();
        assert_eq!((c.count, c.keys.len(), end), (1000, 2, End::Eof));
        assert_eq!(c.keys[0].time, 2.0);
        assert!(r.clamped);
    }

    #[test]
    fn static_values_and_degrees() {
        let v = StaticVal { curve_type: 0x19, value: 0.25 };
        let mut w = Writer::new();
        write_static_val(&mut w, Some(&v));
        write_static_val(&mut w, None);
        assert_eq!(w.out.len(), 12 + 2);
        let mut r = Reader::new(&w.out);
        assert_eq!(read_static_val(&mut r), (v, End::Zero));
        assert_eq!(read_static_val(&mut r), (StaticVal::default(), End::Zero));
        let mut c = curve();
        c.curve_type = 0x19;
        c.keys[0].value = std::f32::consts::PI;
        convert_angle_unit_to_degree(&mut c);
        assert_eq!(c.keys[0].value, 180.0);
        let mut c = curve();
        let before = c.clone();
        convert_angle_unit_to_degree(&mut c);
        assert_eq!(c, before);
    }
}
