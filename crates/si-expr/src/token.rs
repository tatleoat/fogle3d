// SPDX-License-Identifier: MIT
//! The word tables of the expression language (`exp_token.c`, finding 0013).
//!
//! Three tables in the program's data: the functions (`math_fct_array`, 0x0869a740, 27
//! entries of 28 bytes), the predefined words (`predefined_token_array`, 0x0869ac40, 15
//! entries of 16 bytes) and the element type words (`elem_type_array`, 0x0869aa60, 8 entries
//! of 16 bytes). They were read with `re/tools/si-expr/token_tables.py`.

/// How a function wants one argument (the first word of a 12-byte entry of its argument
/// type array): 1 a real value, 4 an integer value, 10 the argument's tree, unevaluated.
pub type ArgKind = i32;

/// One entry of `math_fct_array`.
#[derive(Debug)]
pub struct Function {
    /// +0x00: the label the dialog lists.
    pub label: &'static str,
    /// +0x04: the name as written in an expression.
    pub name: &'static str,
    /// +0x0c: the text the dialog inserts after the name, when it is short (see [`Function::help`]).
    help_lit: &'static str,
    /// The address of the help text in the original's data when it is a message (0: `help_lit`).
    help_at: u32,
    /// +0x10: the address of the original function.
    pub address: u32,
    /// +0x14: how many arguments it takes; −1 for any number.
    pub nargs: i32,
    /// +0x18: one kind per argument; a single kind, applied to every argument, when `nargs`
    /// is −1.
    pub arg_kinds: &'static [ArgKind],
}

macro_rules! f {
    ($label:literal, $name:literal, rt $help:literal, $addr:literal, $n:literal, $kinds:expr) => {
        Function { label: $label, name: $name, help_lit: "", help_at: $help, address: $addr, nargs: $n, arg_kinds: $kinds }
    };
    ($label:literal, $name:literal, $help:literal, $addr:literal, $n:literal, $kinds:expr) => {
        Function { label: $label, name: $name, help_lit: $help, help_at: 0, address: $addr, nargs: $n, arg_kinds: $kinds }
    };
}

impl Function {
    /// The text the dialog inserts after the name: the original's message, read at run time from
    /// the user's install when it is one (`si-rt`; release model: no message ships).
    pub fn help(&self) -> &'static str {
        if self.help_at == 0 { self.help_lit } else { crate::util::rt(self.help_at) }
    }
}

/// `math_fct_array`. The word at +0x08 of each entry is −1 and is not kept.
pub static FUNCTIONS: [Function; 27] = [
    f!("Sine", "sin", "(", 0x08249d14, 1, &[1]),
    f!("Cosine", "cos", "(", 0x08249d50, 1, &[1]),
    f!("Tangent", "tan", "(", 0x08249d8c, 1, &[1]),
    f!("ArcSine", "asin", "(", 0x08249e50, 1, &[1]),
    f!("ArcCosine", "acos", "(", 0x08249eb0, 1, &[1]),
    f!("ArcTangent", "atan", "(", 0x08249e20, 1, &[1]),
    f!("Logarithm", "log", "(", 0x08249f10, 1, &[1]),
    f!("Exponential", "exp", "(", 0x08249f5c, 1, &[1]),
    f!("Absolute", "abs", "(", 0x08249f98, 1, &[1]),
    f!("Square root", "sqrt", "(", 0x08249fc0, 1, &[1]),
    f!("Power", "pow", "( <x> , <y> )", 0x0824a160, 2, &[1, 1]),
    f!("Minimum value", "MIN", "( <val1> , <val2> )", 0x0824a0ac, 2, &[1, 1]),
    f!("Maximum value", "MAX", "( <val1> , <val2> )", 0x0824a108, 2, &[1, 1]),
    f!("Average", "av", "(", 0x0824a054, -1, &[1]),
    f!("Condition", "cond", rt 0x0869a480, 0x0824a008, 3, &[1, 1, 1]),
    f!("Time Offset", "at_time", rt 0x0869a4bc, 0x0824a214, 2, &[1, 10]),
    f!("Frame Offset", "at_frame", rt 0x0869a4ea, 0x0824a2fc, 2, &[1, 10]),
    f!("Center Distance", "ctr_dist", rt 0x0869a51b, 0x0824a3e0, 2, &[10, 10]),
    f!("Dist. to Camera", "ctr_dist_cam", "( <elem1> )", 0x0824a5fc, 1, &[10]),
    f!("Dist. to Cam. Int.", "ctr_dist_cam_int", "( <elem1> )", 0x0824a720, 1, &[10]),
    f!("Sinus", "sinus", rt 0x0869a5a0, 0x0824a844, 3, &[1, 1, 4]),
    f!("Oscillation", "oscil", rt 0x0869a5e0, 0x0824a9a0, 8, &[4, 4, 1, 1, 1, 1, 1, 1]),
    f!("Ease In", "ease_in", rt 0x0869a640, 0x0824ab90, 3, &[4, 4, 4]),
    f!("Ease Out", "ease_out", rt 0x0869a640, 0x0824ac74, 3, &[4, 4, 4]),
    f!("Noise", "noise", "( <x>, <y>, <z> )", 0x0824ad6c, 3, &[1, 1, 1]),
    f!("Fmod", "fmod", "( <val1>, <val2> )", 0x0824ade0, 2, &[1, 1]),
    f!("Has Relation", "has_relation", rt 0x0869a700, 0x0824ae44, 3, &[10, 4, 4]),
];

/// One entry of `predefined_token_array`: the label (+0), the word (+4), the token number
/// the scanner returns for it (+8), the text the dialog inserts after it (+12).
#[derive(Debug)]
pub struct Predefined {
    pub label: &'static str,
    pub word: &'static str,
    pub token: i32,
    help_lit: &'static str,
    help_at: u32,
}

impl Predefined {
    /// The text the dialog inserts after the word (read at run time when it is a message).
    pub fn help(&self) -> &'static str {
        if self.help_at == 0 { self.help_lit } else { crate::util::rt(self.help_at) }
    }
}

macro_rules! p {
    ($label:literal, $word:literal, $token:literal, rt $help:literal) => {
        Predefined { label: $label, word: $word, token: $token, help_lit: "", help_at: $help }
    };
    ($label:literal, $word:literal, $token:literal, $help:literal) => {
        Predefined { label: $label, word: $word, token: $token, help_lit: $help, help_at: 0 }
    };
}

pub static PREDEFINED: [Predefined; 15] = [
    p!("Current Frame", "Fc", 261, ""),
    p!("First Frame", "Fs", 259, ""),
    p!("Last Frame", "Fe", 260, ""),
    p!("Current Time", "T", 262, ""),
    p!("Random: 0 or 1", "randBool", 264, "( [<seed>] )"),
    p!("Random: 0.0 to 1.0", "rand_0_1", 265, "( [<seed>] )"),
    p!("User-Defined Random", "RAND", 263, rt 0x0869ab88),
    p!("PI", "PI", 266, ""),
    p!("E", "E", 267, ""),
    p!("TRUE", "TRUE", 268, ""),
    p!("FALSE", "FALSE", 269, ""),
    p!("Linear Interp.", "LinearInterp", 270, ""),
    p!("Spline Interp.", "SplineInterp", 271, ""),
    p!("FORWARD", "FORWARD", 272, ""),
    p!("BACKWARD", "BACKWARD", 273, ""),
];

/// `elem_type_array`: word and token number. The grammar has no rule that uses these eight
/// tokens: a word that reaches the parser as one of them is a syntax error.
pub static ELEM_TYPES: [(&str, i32); 8] =
    [("MOD", 274), ("MAT", 275), ("T2D", 276), ("T3D", 277), ("CAM", 278), ("LIG", 279), ("WAV", 280), ("CHN", 281)];

/// Chapter ids: places in the program's chapter table (finding 0006, section 7). The
/// original reads them from the `SI_cid` array (0x08781660), which start-up fills.
pub mod chapter {
    pub const CAMERAS: i32 = 2;
    pub const LIGHTS: i32 = 6;
    pub const MATERIALS: i32 = 7;
    pub const MODELS: i32 = 8;
    pub const SETUP_CHANNELS: i32 = 13;
    pub const TEXTURES2D: i32 = 17;
    pub const TEXTURES3D: i32 = 18;
    pub const WAVES: i32 = 19;
    pub const EXPRESSIONS: i32 = 20;
    pub const CHANNEL_DRIVERS: i32 = 30;
}

/// The token of an element type word, −1 if it is none [soft 0x0824f2a0 si_getElemTypeTok].
pub fn get_elem_type_tok(word: &[u8]) -> i32 {
    ELEM_TYPES.iter().find(|(w, _)| w.as_bytes() == word).map_or(-1, |&(_, t)| t)
}

/// The token of a predefined word, −1 if it is none [soft 0x0824f2ec si_getPredefTok].
pub fn get_predef_tok(word: &[u8]) -> i32 {
    PREDEFINED.iter().find(|p| p.word.as_bytes() == word).map_or(-1, |p| p.token)
}

/// The chapter an element type code stands for [soft 0x0824f338 si_getChapidFromText]. Only
/// the first three bytes are compared (`strncmp(…, 3)`), so `MODx` is `MOD`; −1 for anything
/// else.
pub fn get_chapid_from_text(text: &[u8]) -> i32 {
    // strncmp with a three-letter literal: equal when the first three bytes are the letters
    let is = |w: &str| text.len() >= 3 && &text[..3] == w.as_bytes();
    if is("MOD") {
        chapter::MODELS
    } else if is("CAM") {
        chapter::CAMERAS
    } else if is("LIG") {
        chapter::LIGHTS
    } else if is("MAT") {
        chapter::MATERIALS
    } else if is("T2D") {
        chapter::TEXTURES2D
    } else if is("T3D") {
        chapter::TEXTURES3D
    } else if is("WAV") {
        chapter::WAVES
    } else if is("CHN") {
        chapter::SETUP_CHANNELS
    } else {
        -1
    }
}

/// A predefined word followed by its help text, for the dialog
/// [soft 0x0824f458 si_getPredefValidToken]; empty for an index outside the table.
pub fn get_predef_valid_token(index: i32) -> String {
    match PREDEFINED.get(index as u32 as usize) {
        // the format is "%s%s ": word, help text, one blank
        Some(p) => format!("{}{} ", p.word, p.help()),
        None => String::new(),
    }
}

/// The labels of the predefined words, for the dialog's list
/// [soft 0x0824f494 si_getPredefTokenListDial].
pub fn get_predef_token_list_dial() -> Vec<&'static str> {
    PREDEFINED.iter().map(|p| p.label).collect()
}

/// The labels of the functions, for the dialog's list [soft 0x0824f4e0 si_getFctListDial].
pub fn get_fct_list_dial() -> Vec<&'static str> {
    FUNCTIONS.iter().map(|f| f.label).collect()
}

/// The index of a function by its name, −1 if there is none [soft 0x0824f528 si_getFctIndex].
pub fn get_fct_index(name: &[u8]) -> i32 {
    FUNCTIONS.iter().position(|f| f.name.as_bytes() == name).map_or(-1, |i| i as i32)
}

/// A function's name; empty for an index outside the table [soft 0x0824f570 si_getValidFct].
pub fn get_valid_fct(index: i32) -> &'static str {
    FUNCTIONS.get(index as u32 as usize).map_or("", |f| f.name)
}

/// A function's name followed by its help text [soft 0x0824f5b0 si_getFctText].
pub fn get_fct_text(index: i32) -> String {
    match FUNCTIONS.get(index as u32 as usize) {
        Some(f) => format!("{}{}", f.name, f.help()),
        None => String::new(),
    }
}

/// A function's table entry [soft 0x0824f5fc si_getFctPtr]. The original answers null for a
/// negative index and does not check the upper bound: an index of 27 or more is the memory
/// behind the table. Rust answers `None` there too.
pub fn get_fct_ptr(index: i32) -> Option<&'static Function> {
    if index < 0 {
        None
    } else {
        FUNCTIONS.get(index as usize)
    }
}

/// The element type word of a chapter, with the blank that follows it
/// [soft 0x0824f620 SI_getObjTypeFromChapid]. Every chapter that is not one of six is `MOD `.
pub fn get_obj_type_from_chapid(chapter: i32) -> &'static str {
    match chapter {
        chapter::CAMERAS => "CAM ",
        chapter::LIGHTS => "LIG ",
        chapter::MATERIALS => "MAT ",
        chapter::TEXTURES2D => "T2D ",
        chapter::TEXTURES3D => "T3D ",
        chapter::WAVES => "WAV ",
        _ => "MOD ",
    }
}

/// The function-curve editor's element class of a chapter [soft 0x0824f6cc si_fromChap2Graf].
pub fn from_chap_to_graf(chapter: i32) -> i32 {
    match chapter {
        chapter::CAMERAS => 4,
        chapter::LIGHTS => 6,
        chapter::MATERIALS => 14,
        chapter::TEXTURES2D => 15,
        chapter::TEXTURES3D => 16,
        chapter::WAVES => 11,
        _ => 2,
    }
}
