// SPDX-License-Identifier: MIT
//! The relations resource file (`Relations.rsrc`, tag `SREL`): reader (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `reldict.c`; a few look-ups come from `attdict.c`,
//! `enmdict.c` and `elemdict.c`.
//!
//! The file defines three things, in three sections read one after the other: enumerations
//! (`ENUM_VALUES`), the attributes of elements (`ELEMENTS_ATTRIBUTES`) and the operators
//! (`OPERATORS`). The original stores them in three dictionaries: enumerations
//! (`SI_EnumValDict`), attributes (`SI_AttributeDict`: a tree of groups over one flat array of
//! attributes) and relations (`SI_RelDict`: one record per `ARG` line of an operator and one
//! per attribute). The dictionaries themselves are not ported here: [`Relations`] holds what
//! the reader hands to them, argument by argument, in plain structures. The look-ups the
//! reader makes while it parses (an enumeration, an attribute, a group or a chapter by name)
//! are ported, because what it stores depends on their answers; so are the look-ups by which
//! the scene description reader questions the relation dictionary ([`Relations::rel_sub`] and
//! its neighbours).
//!
//! The reader has no notion of a line. It reads blank-separated tokens and, between the
//! keywords it knows, passes over every token it does not: title lines, rules of dashes,
//! `//` comments, lower-case `eat` lines. Each section reader starts by passing over
//! everything up to its section word and then up to its first block, so a section that is
//! missing, or has no block, takes the rest of the file with it. A truncated file ends the
//! reading quietly in most places; the places where the original would spin for ever or read
//! through a bad pointer are reported as an [`Error`].
//!
//! The line readers also have a second mode, for the operator section of a custom effect
//! file (`.cus`), and it is ported with them; the function that drives them in that mode is
//! not (see `Reader::read_one_operator`).

use crate::cnum;
use crate::token::{read_asc_header, AsciiFile};
use std::collections::BTreeMap;

/// C's `EOF`.
const EOF: i32 = -1;

/// A file whose header version is at least this has the three-section layout; below it, the
/// older one-table layout (a double in the original, 0x0864a1d0).
pub const SECTIONS_VERSION: f64 = 3.49;

/// What `*` in a chapter column becomes: any chapter.
pub const CHAPTER_ANY: i16 = -10;

/// The chapter names in the order `SI_elemdictLoadChapters` (0x08133cf0, not ported) adds them
/// to the element dictionary before this file is read: the table `SI_chapInfo` (0x086842c0)
/// from its first entry to its last. A chapter's id is its place in this list; the ids the
/// reader compares with (`SI_cid`, 0x08781660) are the constants below.
pub const CHAPTER_NAMES: [&str; 36] = [
    "SCENES", "ANIMATION", "CAMERAS", "CLUSTERS", "PICTURES", "LATTICES", "LIGHTS", "MATERIALS", "MODELS",
    "NSELECTIONS", "PALETTES", "RENDER_PICTURES", "SETUP_SOFT", "SETUP_CHANNELS", "SETUP_FLOCK", "SHAPES",
    "STRUCT_KEYS", "TEXTURES2D", "TEXTURES3D", "WAVES", "EXPRESSIONS", "MATERIAL_SHADERS", "SHADOW_SHADERS",
    "VOLUME_SHADERS", "TEXTURE2D_SHADERS", "TEXTURE3D_SHADERS", "LIGHT_SHADERS", "CAMERA_SHADERS",
    "OUTPUT_SHADERS", "ENVIRON_SHADERS", "CHANNEL_DRIVERS", "OUTSIDE_DB_FILES", "SPREADSHEET_SETUP", "AUDIO",
    "MATERIAL_PHOTON_SHADERS", "LIGHT_PHOTON_SHADERS",
];
pub const CHAPTER_ANIMATION: i16 = 1;
pub const CHAPTER_CLUSTERS: i16 = 3;
pub const CHAPTER_LATTICES: i16 = 5;
pub const CHAPTER_SHAPES: i16 = 15;

/// Relation groups (`reldictConvertGroupToken`).
pub const GROUP_PARAM: i16 = 0;
pub const GROUP_TRANSFO: i16 = 1;
pub const GROUP_GEOM: i16 = 2;
pub const GROUP_RENDER: i16 = 3;
pub const GROUP_CUSTOM: i16 = 4;
pub const GROUP_OTHER: i16 = 5;

/// Attribute types (`reldictConvertTypeToken`). The range reader also knows types 6 (treated
/// like `FLT`) and 8 (treated like `INT`), which no word of this file produces.
pub const TYPE_FLT: i32 = 1;
pub const TYPE_BOO: i32 = 2;
pub const TYPE_ENM: i32 = 3;
pub const TYPE_INT: i32 = 4;
pub const TYPE_STR: i32 = 5;
pub const TYPE_COORD: i32 = 9;
pub const TYPE_COL: i32 = 11;

/// Access mode bits (`reldictConvertAccessModeToken`): the letters `R`, `W`, `A`.
pub const ACCESS_READ: i16 = 1;
pub const ACCESS_WRITE: i16 = 2;
pub const ACCESS_ANIMATE: i16 = 4;

/// The id and the fcurve id the reader gives to every attribute of a custom effect.
pub const CUSTOM_ID: i32 = 20000;

/// One value of an enumeration: an `ENM` line (`si_enmdictAddValue`'s arguments).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnumValue {
    pub id: i32,
    pub code: Vec<u8>,
    pub name: Vec<u8>,
}

/// An enumeration: an `ENUM` block (`si_enmdictAddEnumType`'s argument, and the values added
/// to what it returns). Its place in [`Relations::enums`] is the "enumeration type" number
/// an attribute refers to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnumType {
    pub name: Vec<u8>,
    pub values: Vec<EnumValue>,
}

/// A condition (`SI_AttribCond`, 12 bytes): "this attribute has one of these values".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttribCond {
    /// Index of the attribute in [`Relations::attributes`], −1 if no attribute has that name.
    /// (When the converter is handed an attribute instead of a name, which no caller in the
    /// program does, this is that attribute's id.)
    pub attribute: i32,
    /// Ids of the enumeration values named, −1 for a code the enumeration does not have.
    pub values: Vec<i32>,
}

/// An attribute (`SI_Attribute`, 100 bytes): an `EAT` line of an element or an `OAT` line of
/// an operator. Offsets are those of the original record. Not kept: the group path at
/// +0x08…+0x18, which `SI_attdictUpdateIndexes` fills after the file is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// +0x00: 0 for an element's attribute, 1 for an operator's ("Elem" / "Oper" in the
    /// original's dump).
    pub owner_kind: i32,
    /// +0x04: for an element's attribute the chapter id of its block (−1 if the block names
    /// no chapter), for an operator's the operator id, cut to 16 bits.
    pub owner_id: i16,
    /// +0x1c: the id column (`xx` reads as 0); the relation id of the attribute.
    pub id: i32,
    /// +0x20
    pub name: Vec<u8>,
    /// +0x24: the quoted text after the fcurve names, if there is one.
    pub description: Option<Vec<u8>>,
    /// +0x28: `ACCESS_*` bits.
    pub access: i16,
    /// +0x2a: 0 node, 1 branch, 2 tree, −1 unknown word.
    pub scope: i16,
    /// +0x2c: `TYPE_*`.
    pub kind: i32,
    /// +0x30: for `ENM(Name)` the index of the enumeration, −1 if there is none of that
    /// name. Other types do not write it: see `Reader::read_one_attribute`.
    pub enum_type: i16,
    /// +0x34, +0x38, +0x3c: lower bound; its kind (0 none, 1 hard, 2 soft: written `~`);
    /// 1 if the bound itself is allowed (`[`), 0 if not (`(`).
    pub min: f32,
    pub min_kind: i32,
    pub min_inclusive: i16,
    /// +0x40, +0x44, +0x48: upper bound, likewise.
    pub max: f32,
    pub max_kind: i32,
    pub max_inclusive: i16,
    /// +0x4c: `GROUP_*`, −1 unknown word.
    pub group: i16,
    /// +0x4e
    pub rank: i16,
    /// +0x50: 0 `NON`, 1 `INS`, 3 `ALL` (and any other word), 4 `SHD`.
    pub incmode: i32,
    /// +0x54: the `#` column: how many values; 0x7fff for `INF`.
    pub count: i16,
    /// +0x56: how many fcurves: the same number, or 1 for `INF`.
    pub fcurves: i16,
    /// +0x58: fcurve ids, 16 bits each; −1 where the file gave none.
    pub fcurve_ids: Vec<i16>,
    /// +0x5c: fcurve names; `None` where the file gave none.
    pub fcurve_names: Vec<Option<Vec<u8>>>,
    /// +0x60: the `vis` column. It becomes the relation's "dope display" flag.
    pub dope_display: i16,
}

impl Attribute {
    /// The record as `si_attdictInitAttribute` (0x0811e7d0, not ported) leaves it.
    pub fn init() -> Self {
        Attribute {
            owner_kind: 0,
            owner_id: 0,
            id: 0,
            name: Vec::new(),
            description: None,
            access: 0,
            scope: 0,
            kind: 1,
            enum_type: 0,
            min: 0.0,
            min_kind: 0,
            min_inclusive: 1,
            max: 0.0,
            max_kind: 0,
            max_inclusive: 1,
            group: 0,
            rank: 0,
            incmode: 3,
            count: 0,
            fcurves: 0,
            fcurve_ids: Vec::new(),
            fcurve_names: Vec::new(),
            dope_display: 0,
        }
    }
}

/// A group of attributes (`__SI_AttribGroup`, 40 bytes): an element, a sub-element or an
/// operator. Holds what `si_attdictAddAttribGroup`, `si_attdictAddAttribSubGroup` and
/// `si_attdictAddAttribute` are given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttribGroup {
    pub name: Vec<u8>,
    /// The `SI_RelType` argument: 0 for an `ELEMENT`, 1 for an operator's group; sub-groups
    /// are not given one and stay 0.
    pub rel_type: i32,
    /// The condition under which a sub-group applies (`COND(…)` of its `SUB` line).
    pub cond: Option<AttribCond>,
    /// Indexes into [`Relations::attributes`].
    pub attributes: Vec<usize>,
    pub subgroups: Vec<AttribGroup>,
}

/// Where a group is in the tree: an index into [`Relations::groups`], then one index per
/// level of sub-groups.
pub type GroupPath = Vec<usize>;

/// A relation (`SI_RelDict` record, 80 bytes): what `SI_reldictAddRelation` (0x08139b80, not
/// ported) is given. The names of the fields are those of the original's own dump:
/// `chp[%d->%d] grp[%d->%d] rk(%d) fo(%d) dd(%d)`. Not kept: the two "active" flags the
/// program sets to 1 and changes while it runs, and the custom effect data, absent here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Relation {
    /// +0x00
    pub id: i32,
    /// +0x04
    pub name: Vec<u8>,
    /// +0x08: "source" in the original's messages. For an operator it is the chapter of the
    /// `RES` line, for an attribute the chapter of its element.
    pub src_chapter: i16,
    /// +0x0a: "destination": the chapter of the `ARG` line; `ANIMATION` for an attribute.
    pub dst_chapter: i16,
    /// +0x0c: group of the `RES` line, or of the attribute.
    pub src_group: i16,
    /// +0x0e: group of the `ARG` line; 0 for an attribute.
    pub dst_group: i16,
    /// +0x10
    pub rank: i16,
    /// +0x12: "fan-out": the `mx` column of the `ARG` line, the `#` column of an attribute.
    pub fanout: i16,
    /// +0x14: the `vis` column.
    pub dope_display: i16,
    /// +0x16, +0x18: the `pur` and `pre` columns of the `ARG` line (what
    /// `si_reldictGetRelUpdlattr` returns); 0 for an attribute.
    pub upd: [i16; 2],
    /// +0x48: 1 if the operator line has `RENDERING`.
    pub render: i16,
    /// +0x4c: the `INCR_SEND_…` word of the operator line, or the `inc` column of an
    /// attribute.
    pub incmode: i32,
}

/// The first line of an operator (`SI_OperDef` up to its two arrays).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperDef {
    pub id: i32,
    pub name: Vec<u8>,
    /// 0 `NODE` (or `N`), 1 `BRANCH` (or `B`), 2 `TREE`. Default 0.
    pub scope: i16,
    /// 1 `MULTI`, 0 `SINGLE`. Default 0.
    pub application: i16,
    /// 0 `NO_OP`, 1 `CV_BASED`, 2 `GENERATOR`, 3 `CREATOR`, 4 `SPATIAL_DEF`, 5 `SUBSTITUTION`,
    /// 6 `BIDIRECTIONAL`. Default 0.
    pub category: i32,
    /// 1 `RENDERING`. Default 0.
    pub render: i16,
    /// 0 `INCR_SEND_NONE`, 1 `INCR_SEND_INST`, 3 `INCR_SEND_ALL`, 4 `INCR_SEND_SHDTRE`,
    /// 7 `INCR_SEND_WITH_SHDTRE`. Default 3.
    pub incmode: i32,
}

/// The five columns `RES` and `ARG` lines share (the start of `SI_OperResult` and of
/// `SI_OperArgument`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperSection {
    /// The id column, 16 bits. 0 in a custom effect, which has no such column.
    pub id: i16,
    pub name: Vec<u8>,
    /// Chapter id, or [`CHAPTER_ANY`].
    pub chapter: i16,
    /// `GROUP_*`, −1 unknown word.
    pub group: i16,
    /// The rank column (`-` reads as 0); 50 in a custom effect.
    pub rank: i16,
}

/// What a `RES` or `ARG` line of a custom effect adds to the shared columns
/// (`reldictCusReadCommonSection`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CusSection {
    /// 0 node, 1 branch, 2 tree, −1 unknown word.
    pub scope: i16,
    pub cond: Option<AttribCond>,
    /// The quoted message shown when the user is asked to pick the element.
    pub pick_msg: Option<Vec<u8>>,
    /// After `MAX`: a number, 1 for `-`, 0x7fff for `INF`. Default 1.
    pub fanout: i16,
}

/// A `RES` line (`SI_OperResult`, 32 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperResult {
    pub section: OperSection,
    /// Only a custom effect's line has this part. For a standard operator the original leaves
    /// these bytes as `malloc` returned them.
    pub cus: Option<CusSection>,
    /// +0x18: 1 for a custom effect's `NEWRES` line, else 0.
    pub new_result: i16,
}

/// An `ARG` line (`SI_OperArgument`, 40 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperArgument {
    pub section: OperSection,
    /// +0x18: the `mx` column: how many elements; 0x7fff for `INF`.
    pub max: i32,
    /// +0x1c: the `vis` column.
    pub dope_display: i16,
    /// +0x1e: the `pur` column.
    pub pur: i16,
    /// +0x20: the `pre` column. (In a custom effect: the number after the group.)
    pub pre: i16,
    /// As for [`OperResult::cus`].
    pub cus: Option<CusSection>,
}

/// An operator as the reader holds it while it reads the block. The original frees the
/// results and arguments at the end of the block: what stays is one [`Relation`] per `ARG`
/// line, and the operator's attribute group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Operator {
    pub def: OperDef,
    pub results: Vec<OperResult>,
    pub arguments: Vec<OperArgument>,
    /// Indexes into [`Relations::attributes`] of its `OAT` lines.
    pub attributes: Vec<usize>,
    /// Indexes into [`Relations::relations`] of the records its `ARG` and `OAT` lines added.
    pub relations: Vec<usize>,
}

/// An `ELEMENT` or `SUBELEMENT` block as the reader went through it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElementBlock {
    pub sub: bool,
    pub name: Vec<u8>,
    /// The name after `OF` (sub-elements only).
    pub parent: Option<Vec<u8>>,
    /// The chapter id given to the block's attributes as their owner: −1 for an `ELEMENT`;
    /// for a `SUBELEMENT` the chapter of that name, else the chapter named after `OF`, else −1.
    pub chapter: i16,
    /// The block's group: for an `ELEMENT` a new one, for a `SUBELEMENT` the group a `SUB`
    /// line of an earlier block made. `None` if there is no such group.
    pub group: Option<GroupPath>,
    /// Indexes into [`Relations::attributes`] of its `EAT` lines.
    pub attributes: Vec<usize>,
    /// Names of its `SUB` lines.
    pub subs: Vec<Vec<u8>>,
}

/// Everything the reader builds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Relations {
    /// The enumeration dictionary (`SI_EVD`).
    pub enums: Vec<EnumType>,
    /// The attribute dictionary (`SI_ATD`): its top-level groups…
    pub groups: Vec<AttribGroup>,
    /// …and its one array of attributes, in the order they were read.
    pub attributes: Vec<Attribute>,
    /// The relation dictionary (`SI_RD`), in the order the records were added.
    pub relations: Vec<Relation>,
    /// The chapter names of the element dictionary: [`CHAPTER_NAMES`], then any name the file
    /// uses that is not among them, which the reader adds as a new chapter.
    pub chapters: Vec<Vec<u8>>,
    /// The `ELEMENT` and `SUBELEMENT` blocks, in file order.
    pub blocks: Vec<ElementBlock>,
    /// The `OPERATOR` blocks, in file order.
    pub operators: Vec<Operator>,
    /// For each distinct relation id, in ascending order of id, the index of its first
    /// record: the original's sort table. Built by [`Relations::sort_relations`].
    rel_sort: Vec<usize>,
}

/// Which of the two layouts a file was read as.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Layout {
    /// Version 3.49 and later: `ENUM_VALUES`, `ELEMENTS_ATTRIBUTES`, `OPERATORS`.
    #[default]
    Sections,
    /// Before 3.49, or no header: one table between `RELATIONS` and `EndOfRELATIONS`.
    Table,
}

/// Where the original would have said something.
#[derive(Clone, Debug, PartialEq)]
pub enum Channel {
    /// `SI_error(code, …)`: the program's error reporter.
    SiError(i32),
    /// `SI_statusDisplayMessage(text, level)`: the status line.
    Status(i32),
    Stderr,
    Stdout,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub channel: Channel,
    pub text: String,
    /// File offset when it was raised.
    pub offset: usize,
}

/// What reading a file showed, beyond the dictionaries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadInfo {
    /// The header's tag, `None` if the file does not start with one of the seven accepted.
    pub tag: Option<u32>,
    /// The header's version; −1.0 without a header.
    pub version: f32,
    pub layout: Layout,
    /// What the section reader returned: non-zero if the operators section was read to its
    /// `EndOfOPERATORS`. The original ignores it. Always 0 for the table layout.
    pub result: i16,
    /// File offset after the last byte read.
    pub end: usize,
    /// Nothing but blanks is left after that offset.
    pub all_tokens_consumed: bool,
    /// Every token read (attempts at the end of the file included).
    pub tokens_read: usize,
    /// Tokens passed over on the way to a keyword, by `SI_fileTokenReach`…
    pub reach_skipped: usize,
    /// …and by the readers' own loops, with how often each word was passed over.
    pub skipped: BTreeMap<Vec<u8>, usize>,
    /// Attributes whose description lost characters (see `Reader::read_description`).
    pub descriptions_cut: Vec<usize>,
    /// Attributes whose fcurve id or name list was longer than their count.
    pub fcurve_overruns: Vec<usize>,
    pub messages: Vec<Message>,
}

/// The inputs on which the original does not come back with an answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The file ended inside a list the original reads "until it has enough": it would read
    /// empty tokens for ever.
    EndlessLoop { what: &'static str, offset: usize },
    /// The original would read through a null or uninitialised pointer here.
    BadPointer { what: &'static str, offset: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::EndlessLoop { what, offset } => {
                write!(f, "the file ends at offset {offset} inside {what}: the original would loop for ever")
            }
            Error::BadPointer { what, offset } => {
                write!(f, "at offset {offset}: {what}: the original would read through a bad pointer")
            }
        }
    }
}

impl std::error::Error for Error {}

// ---------------------------------------------------------------------------------------------
// The words of the file
// ---------------------------------------------------------------------------------------------

/// [soft 0x0813a878 reldictConvertGroupToken]
pub fn convert_group_token(tok: &[u8]) -> i32 {
    match tok {
        b"PARAM" | b"RELGR_PARAM" => 0,
        // the three kinds of transformation are one group
        b"TRANSFO" | b"SCAL" | b"ROTN" | b"TRANSL" | b"RELGR_TRANSFO" => 1,
        b"GEOM" | b"RELGR_GEOM" => 2,
        b"RENDER" | b"RELGR_RENDER" => 3,
        b"CUSTOM" | b"RELGR_CUSTOM" => 4,
        b"OTHER" | b"RELGR_OTHER" => 5,
        _ => -1,
    }
}

/// [soft 0x0813aa0c reldictConvertScopeToken]
pub fn convert_scope_token(tok: &[u8]) -> i32 {
    match tok {
        b"N" | b"NODE" => 0,
        b"B" | b"BRANCH" => 1,
        b"TREE" => 2,
        _ => -1,
    }
}

/// [soft 0x0813aa9c reldictConvertApplicationToken]
pub fn convert_application_token(tok: &[u8]) -> i32 {
    match tok {
        b"MULTI" => 1,
        b"SINGLE" => 0,
        _ => -1,
    }
}

/// [soft 0x0813aae8 reldictConvertOpCategToken]
pub fn convert_op_categ_token(tok: &[u8]) -> i32 {
    match tok {
        b"CV_BASED" => 1,
        b"GENERATOR" => 2,
        b"CREATOR" => 3,
        b"SPATIAL_DEF" => 4,
        b"SUBSTITUTION" => 5,
        b"BIDIRECTIONAL" => 6,
        b"NO_OP" => 0,
        _ => -1,
    }
}

/// [soft 0x0813abd8 reldictConvertRenderToken]: the flag (set to 0, then to 1 for the word)
/// and the result, −1 for any other word. The original also returns −1, without touching the
/// flag, when given no place to put it.
pub fn convert_render_token(tok: &[u8]) -> (i16, i32) {
    if tok == b"RENDERING" {
        (1, 1)
    } else {
        (0, -1)
    }
}

/// [soft 0x0813ac18 reldictConvertIncmodeToken]: −1 for a word it does not know.
pub fn convert_incmode_token(tok: &[u8]) -> i32 {
    match tok {
        b"INCR_SEND_NONE" => 0,
        b"INCR_SEND_INST" => 1,
        b"INCR_SEND_ALL" => 3,
        b"INCR_SEND_SHDTRE" => 4,
        b"INCR_SEND_WITH_SHDTRE" => 7,
        _ => -1,
    }
}

/// [soft 0x0813ace4 reldictConvertAttribIncmodeToken]: any word it does not know counts as
/// `ALL`.
pub fn convert_attrib_incmode_token(tok: &[u8]) -> i32 {
    match tok {
        b"NON" => 0,
        b"INS" => 1,
        b"SHD" => 4,
        _ => 3,
    }
}

/// [soft 0x0813b5e8 reldictConvertAccessModeToken]: one bit per letter found anywhere in the
/// word.
pub fn convert_access_mode_token(tok: &[u8]) -> i16 {
    let mut bits = 0;
    if tok.contains(&b'R') {
        bits |= ACCESS_READ;
    }
    if tok.contains(&b'W') {
        bits |= ACCESS_WRITE;
    }
    if tok.contains(&b'A') {
        bits |= ACCESS_ANIMATE;
    }
    bits
}

/// [soft 0x0813b644 reldictCusConvertFanoutToken]: the original returns 0 when either pointer
/// is null and 1 otherwise; here only the value.
pub fn cus_convert_fanout_token(tok: &[u8]) -> i16 {
    match tok {
        b"-" => 1,
        b"INF" => 0x7fff,
        _ => cnum::atoi(tok) as i16,
    }
}

/// [soft 0x0813fd88 token2Uppercase], always called with one buffer as source and
/// destination. `toupper` in the C locale: the 26 letters.
fn token2uppercase(s: &mut [u8]) {
    s.make_ascii_uppercase();
}

/// The pieces C's `strtok` hands out when called on `s` until it returns null: the runs of
/// bytes between delimiters, empty runs dropped.
fn strtok_pieces<'t>(s: &'t [u8], delims: &[u8]) -> Vec<&'t [u8]> {
    s.split(|b| delims.contains(b)).filter(|p| !p.is_empty()).collect()
}

/// What a buffer reads as a C string after `strtok` went over it: the first piece's
/// delimiter has become a NUL.
fn after_strtok(s: &mut Vec<u8>, delims: &[u8]) {
    if let Some(a) = s.iter().position(|b| !delims.contains(b)) {
        if let Some(b) = s[a..].iter().position(|b| delims.contains(b)) {
            s.truncate(a + b);
        }
    }
}

/// `strcspn`: the length of the start of `s` holding none of `reject`.
fn strcspn(s: &[u8], reject: &[u8]) -> usize {
    s.iter().position(|b| reject.contains(b)).unwrap_or(s.len())
}

/// Rounds `mag × 2^e` to `bits` significant bits: to nearest, ties to even. `sticky`: the
/// true value is a little above `mag`.
fn round_mag(mag: u128, e: i32, sticky: bool, bits: u32) -> (u128, i32) {
    if mag == 0 {
        return (0, e);
    }
    let top = 127 - mag.leading_zeros();
    if top < bits {
        return (mag, e);
    }
    let drop = top + 1 - bits;
    let q = mag >> drop;
    let rem = mag & ((1u128 << drop) - 1);
    let half = 1u128 << (drop - 1);
    let up = rem > half || (rem == half && (sticky || q & 1 == 1));
    (q + up as u128, e + drop as i32)
}

/// `mag × 2^e`, exact, rounded once to a single.
fn mag_to_f32(mag: u128, e: i32) -> f32 {
    if mag == 0 {
        return 0.0;
    }
    let top = (127 - mag.leading_zeros()) as i32;
    let exp = top + e; // the value is in [2^exp, 2^(exp+1))
    if exp > 127 {
        return f32::INFINITY;
    }
    // bits that fit: 24 for a normal number, fewer below 2^-126
    let keep = if exp >= -126 { 24 } else { 24 - (-126 - exp) };
    if keep <= 0 {
        let above_half = keep == 0 && mag > (1u128 << top);
        return if above_half { f32::from_bits(1) } else { 0.0 };
    }
    let (q, e2) = round_mag(mag, e, false, keep as u32);
    // q has at most 25 bits and q × 2^e2 is a single or overflows: both steps are exact
    (q as f64 * 2f64.powi(e2)) as f32
}

/// `a + b` as the original computes it where it loads a single, adds a double to it on the
/// x87 stack and stores the result as a single: the sum is rounded to the 64-bit mantissa of
/// the extended format, then to the 24 bits of a single. (The unit's default control word is
/// assumed: extended precision, round to nearest.) A sum in doubles would round to 53 bits
/// first and can land on the other side of a tie.
fn x87_add_store_single(a: f32, b: f64) -> f32 {
    let a = a as f64;
    if !a.is_finite() || !b.is_finite() || a == 0.0 || b == 0.0 {
        return (a + b) as f32; // nothing to round twice
    }
    fn parts(v: f64) -> (bool, u64, i32) {
        let bits = v.to_bits();
        let e = ((bits >> 52) & 0x7ff) as i32;
        let m = bits & ((1u64 << 52) - 1);
        if e == 0 {
            (v < 0.0, m, -1074)
        } else {
            (v < 0.0, m | 1 << 52, e - 1075)
        }
    }
    let (pa, pb) = (parts(a), parts(b));
    let ((nx, mx, ex), (ny, my, ey)) = if pa.2 >= pb.2 { (pa, pb) } else { (pb, pa) };
    let d = (ex - ey) as u32;
    let (neg, mag, e, sticky) = if d <= 70 {
        let x = (mx as i128) << d;
        let y = my as i128;
        let s = if nx { -x } else { x } + if ny { -y } else { y };
        (s < 0, s.unsigned_abs(), ey, false)
    } else {
        // the smaller operand lies wholly below the 64 bits kept: it only says on which side
        // of the larger one the sum is
        let x = (mx as u128) << 70;
        (nx, if nx == ny { x } else { x - 1 }, ex - 70, true)
    };
    if mag == 0 {
        return 0.0;
    }
    let (q, e2) = round_mag(mag, e, sticky, 64);
    let v = mag_to_f32(q, e2);
    if neg {
        -v
    } else {
        v
    }
}

/// The amount by which an excluded bound of a real attribute is moved inwards (a double,
/// 0x0864a1c8).
const RANGE_EPSILON: f64 = 0.0001;

/// The range column of an attribute: `[low,high]` [soft 0x0813aeb0 reldictConvertRangeToken].
///
/// `[` or `]` allows the bound itself, `(` or `)` does not: such a bound is moved inwards
/// when read, by 0.0001 for a real attribute (types 1 and 6) and by 1 for an integer one
/// (types 4 and 8), and not at all for any other type. `~` before a number makes the bound a
/// soft one (kind 2). A missing number means no bound on that side (kind 0).
///
/// False for a word that is not a range: no comma, no opening bracket before it, no closing
/// bracket after it, or a `~` with no number on the upper side (on the lower side that is
/// "no bound"). The caller then clears both kinds; the other fields keep what was written
/// before the fault was met.
///
/// The original cuts the token buffer at the comma; `tok` is cut the same way.
pub fn convert_range_token(tok: &mut Vec<u8>, attr: &mut Attribute) -> bool {
    attr.min = 0.0;
    attr.max = 0.0;
    attr.min_kind = 1;
    attr.max_kind = 1;
    let Some(comma) = tok.iter().position(|&b| b == b',') else { return false };
    let high: Vec<u8> = tok[comma + 1..].to_vec();
    tok.truncate(comma);
    let low: &[u8] = tok;
    let n = strcspn(low, b"[(");
    if n == low.len() {
        return false;
    }
    let mut lo = &low[n + 1..];
    if lo.is_empty() {
        attr.min_kind = 0;
    } else {
        attr.min_kind = 1;
        attr.min_inclusive = (low[n] == b'[') as i16;
        if lo[0] == b'~' {
            attr.min_kind = 2;
            lo = &lo[1..];
            if lo.is_empty() {
                attr.min_kind = 0; // and the bound is read all the same: 0
            }
        }
        attr.min = cnum::atof(lo) as f32;
        if attr.min_inclusive == 0 {
            match attr.kind {
                1 | 6 => attr.min = x87_add_store_single(attr.min, RANGE_EPSILON),
                // 1.0 + a single: exact in the extended format or far from any tie, so one
                // rounding, the same as in doubles
                4 | 8 => attr.min = (1.0 + attr.min as f64) as f32,
                _ => {}
            }
        }
    }
    let n = strcspn(&high, b"])");
    if n == 0 {
        attr.max_kind = 0;
        return true;
    }
    if n == high.len() {
        return false;
    }
    attr.max_kind = 1;
    attr.max_inclusive = (high[n] == b']') as i16;
    let mut hi = &high[..n];
    if hi[0] == b'~' {
        attr.max_kind = 2;
        hi = &hi[1..];
        if hi.is_empty() {
            return false;
        }
    }
    attr.max = cnum::atof(hi) as f32;
    if attr.max_inclusive == 0 {
        match attr.kind {
            1 | 6 => attr.max = x87_add_store_single(attr.max, -RANGE_EPSILON),
            4 | 8 => attr.max = (attr.max as f64 - 1.0) as f32,
            _ => {}
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Look-ups in the dictionaries
// ---------------------------------------------------------------------------------------------

/// `strcasecmp(a, b) == 0` in the C locale.
fn eq_nocase(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// [soft 0x0811ed88 si_attdictAttribSubGroupGetByName]: the group itself if it has the name,
/// else the first match among its sub-groups, depth first. `path` is the group's own path on
/// entry and the found group's on a true return.
fn sub_group_get_by_name(group: &AttribGroup, name: &[u8], path: &mut GroupPath) -> bool {
    if group.name == name {
        return true;
    }
    for (i, sub) in group.subgroups.iter().enumerate() {
        path.push(i);
        if sub_group_get_by_name(sub, name, path) {
            return true;
        }
        path.pop();
    }
    false
}

impl Relations {
    /// A dictionary as the program has it before the file is read: no enumerations, no
    /// attributes, no relations, and the element dictionary's 36 chapters.
    pub fn new() -> Self {
        Relations { chapters: CHAPTER_NAMES.iter().map(|n| n.as_bytes().to_vec()).collect(), ..Default::default() }
    }

    /// Index of the enumeration of that name, −1 if there is none
    /// [soft 0x08138888 SI_enmdictEnumTypeGetName].
    pub fn enum_type_index(&self, name: &[u8]) -> i32 {
        self.enums.iter().position(|e| e.name == name).map_or(-1, |i| i as i32)
    }

    /// The value of enumeration `enum_type` with that code, letter case ignored
    /// [soft 0x0813fe30 enmdictEnumValGetByCode].
    pub fn enum_value_by_code(&self, enum_type: i32, code: &[u8]) -> Option<&EnumValue> {
        if enum_type < 0 || enum_type as usize >= self.enums.len() {
            return None;
        }
        self.enums[enum_type as usize].values.iter().find(|v| eq_nocase(&v.code, code))
    }

    /// Index of the first attribute of that name, letter case ignored
    /// [soft 0x0813fddc attdictAttributeGetByName].
    pub fn attribute_by_name(&self, name: &[u8]) -> Option<usize> {
        self.attributes.iter().position(|a| eq_nocase(&a.name, name))
    }

    /// The first group of that name: each top-level group is searched in turn, itself first,
    /// then its sub-groups depth first [soft 0x0811edf8 SI_attdictAttribGroupGetByName].
    pub fn group_by_name(&self, name: &[u8]) -> Option<GroupPath> {
        for (i, g) in self.groups.iter().enumerate() {
            let mut path = vec![i];
            if sub_group_get_by_name(g, name, &mut path) {
                return Some(path);
            }
        }
        None
    }

    /// The group at a path.
    pub fn group(&self, path: &[usize]) -> &AttribGroup {
        let mut g = &self.groups[path[0]];
        for &i in &path[1..] {
            g = &g.subgroups[i];
        }
        g
    }

    fn group_mut(&mut self, path: &[usize]) -> &mut AttribGroup {
        let mut g = &mut self.groups[path[0]];
        for &i in &path[1..] {
            g = &mut g.subgroups[i];
        }
        g
    }

    /// Chapter id of a name, −1 if there is none. The comparison is exact, except that
    /// `PICTURES` and `RENDER_PICTURES` are found in any letter case
    /// [soft 0x081325f4 SI_elemdictFindListByName].
    pub fn find_chapter(&self, name: &[u8]) -> i32 {
        for (i, c) in self.chapters.iter().enumerate() {
            if c == name {
                return i as i32;
            }
            if eq_nocase(b"PICTURES", name) && eq_nocase(c, name) {
                return i as i32;
            }
            if eq_nocase(b"RENDER_PICTURES", name) && eq_nocase(c, name) {
                return i as i32;
            }
        }
        -1
    }

    /// Chapter id of a name, adding the name as a new chapter if there is none
    /// [soft 0x08133c40 SI_elemdictAddList]. (A chapter is more than its name in the
    /// original; the name is all the reader needs.)
    pub fn add_chapter(&mut self, name: &[u8]) -> i32 {
        let i = self.find_chapter(name);
        if i != -1 {
            return i;
        }
        self.chapters.push(name.to_vec());
        self.chapters.len() as i32 - 1
    }

    /// The name of a chapter id, for display: `*` for [`CHAPTER_ANY`], `?` for no chapter.
    pub fn chapter_name(&self, id: i16) -> String {
        if id == CHAPTER_ANY {
            "*".into()
        } else {
            self.chapters.get(id as usize).map_or("?".into(), |n| String::from_utf8_lossy(n).into_owned())
        }
    }

    // -----------------------------------------------------------------------------------------
    // The relation dictionary as the scene description reader questions it
    // -----------------------------------------------------------------------------------------

    /// Binary search of the sort table for an id [soft 0x08139e88 findElemInRelSortTbl]:
    /// the place in the table if it is there, else the place where it would go.
    fn find_in_sort_table(&self, id: i32) -> Result<usize, usize> {
        let (mut low, mut high) = (0usize, self.rel_sort.len() + 1);
        let mut mid = high / 2;
        while mid != low {
            let v = self.relations[self.rel_sort[mid - 1]].id;
            if id < v {
                high = mid;
            } else if id == v {
                return Ok(mid - 1);
            } else {
                low = mid;
            }
            mid = (high + low) / 2;
        }
        Err(mid)
    }

    /// Puts a record in the sort table unless a record of the same id is there already
    /// [soft 0x08139ef8 addElemInRelSortTbl].
    fn add_to_sort_table(&mut self, index: usize) {
        if let Err(at) = self.find_in_sort_table(self.relations[index].id) {
            self.rel_sort.insert(at, index);
        }
    }

    /// Rebuilds the sort table. [`read`] does it; call it again after changing
    /// [`Relations::relations`] by hand. (The original rebuilds on the first look-up after a
    /// relation was added.)
    pub fn sort_relations(&mut self) {
        self.rel_sort.clear();
        for i in 0..self.relations.len() {
            self.add_to_sort_table(i);
        }
    }

    /// Index of the **first** record with that id, −1 if there is none
    /// [soft 0x08139f68 SI_reldictGetRelIndexFromId].
    ///
    /// Several records can share an id: the `ARG` lines of one operator, the attributes whose
    /// id column is `xx` (id 0). The original keeps a table of the first record of each id,
    /// sorted by id, searches it, and remembers the last question and answer in three
    /// statics; without a table (an empty dictionary) it walks the records. The answer is the
    /// same either way.
    pub fn rel_index_from_id(&self, id: i32) -> i32 {
        match self.find_in_sort_table(id) {
            Ok(at) => self.rel_sort[at] as i32,
            Err(_) => -1,
        }
    }

    /// [soft 0x0813a0f4 SI_reldictGetRelName]
    pub fn rel_name(&self, id: i32) -> &[u8] {
        match self.rel_index_from_id(id) {
            i if i < 0 => b"unknown",
            i => &self.relations[i as usize].name,
        }
    }

    /// The "sub-definition" of a relation: which of the records of this id, counted from 1
    /// in the order they were added, has these two chapters and two groups; with it, that
    /// record's fan-out [soft 0x0813a12c si_reldictGetRelSub].
    ///
    /// Only the unbroken run of records with this id that starts at the first one is looked
    /// at. No match gives 1 and no fan-out (the original leaves the caller's variable alone).
    /// For an unknown id the original starts one record before the array; here that is "no
    /// match".
    pub fn rel_sub(
        &self,
        id: i32,
        src_chapter: i32,
        dst_chapter: i32,
        src_group: i32,
        dst_group: i32,
    ) -> (i32, Option<i32>) {
        let first = self.rel_index_from_id(id);
        if first < 0 {
            return (1, None);
        }
        let mut sub = 1;
        for r in &self.relations[first as usize..] {
            if r.id != id {
                break;
            }
            if r.src_chapter as i32 == src_chapter
                && r.dst_chapter as i32 == dst_chapter
                && r.src_group as i32 == src_group
                && r.dst_group as i32 == dst_group
            {
                return (sub, Some(r.fanout as i32));
            }
            sub += 1;
        }
        (1, None)
    }

    /// How many records the run of this id has [soft 0x0813a1ac SI_reldictGetRelNbRules].
    pub fn rel_nb_rules(&self, id: i32) -> i32 {
        let first = self.rel_index_from_id(id);
        if first < 0 {
            return 0; // the original looks at the record before the array
        }
        self.relations[first as usize..].iter().take_while(|r| r.id == id).count() as i32
    }

    /// The record of a relation's sub-definition (below 1 counts as 1): its two chapters, two
    /// groups, rank and fan-out are what the original hands back
    /// [soft 0x0813a28c SI_reldictGetRelAttributes]. `None` for an unknown id (the
    /// original's false).
    ///
    /// The record is simply the one `sub − 1` places after the first of that id: nothing
    /// checks that it still has the id. Past the end of the dictionary the original reads
    /// whatever follows the array; here that is `None` too.
    pub fn rel_attributes(&self, id: i32, sub: i32) -> Option<&Relation> {
        let sub = if sub < 1 { 1 } else { sub };
        let first = self.rel_index_from_id(id);
        if first < 0 {
            return None;
        }
        self.relations.get(first as usize + sub as usize - 1)
    }

    /// True for a relation whose first record has `*` as either chapter: the scene
    /// description alone does not say between which chapters it holds
    /// [soft 0x0813a770 SI_reldictCheckUnCompleteRelation].
    pub fn check_uncomplete_relation(&self, id: i32) -> bool {
        let i = self.rel_index_from_id(id);
        if i < 0 {
            return false;
        }
        let r = &self.relations[i as usize];
        r.dst_chapter == CHAPTER_ANY || r.src_chapter == CHAPTER_ANY
    }

    /// Id of the first record with that name, −1 if there is none
    /// [soft 0x0813a5bc SI_reldictGetRelIdByName].
    pub fn rel_id_by_name(&self, name: &[u8]) -> i32 {
        self.relations.iter().find(|r| r.name == name).map_or(-1, |r| r.id)
    }

    /// Source chapter and source group of a relation's first record; (−1, 0) for an unknown
    /// id [soft 0x0813a464 SI_reldictGetRelChapGrp].
    pub fn rel_chap_grp(&self, id: i32) -> (i32, i32) {
        match self.rel_index_from_id(id) {
            i if i < 0 => (-1, 0),
            i => (self.relations[i as usize].src_chapter as i32, self.relations[i as usize].src_group as i32),
        }
    }

    /// [soft 0x0813a4e8 SI_reldictGetRelRank]: 0 for an unknown id.
    pub fn rel_rank(&self, id: i32) -> i32 {
        match self.rel_index_from_id(id) {
            i if i < 0 => 0,
            i => self.relations[i as usize].rank as i32,
        }
    }

    /// [soft 0x0813a4b4 reldictGetRelDopeDisp]: 0 for an unknown id.
    pub fn rel_dope_disp(&self, id: i32) -> i32 {
        match self.rel_index_from_id(id) {
            i if i < 0 => 0,
            i => self.relations[i as usize].dope_display as i32,
        }
    }

    /// [soft 0x0813a3f4 SI_reldictGetRelRenderFlag]: 0 for an unknown id.
    pub fn rel_render_flag(&self, id: i32) -> i32 {
        match self.rel_index_from_id(id) {
            i if i < 0 => 0,
            i => self.relations[i as usize].render as i32,
        }
    }

    /// [soft 0x0813a428 SI_reldictGetRelIncmodeFlag]: −1 for an unknown id.
    pub fn rel_incmode_flag(&self, id: i32) -> i32 {
        match self.rel_index_from_id(id) {
            i if i < 0 => -1,
            i => self.relations[i as usize].incmode,
        }
    }

    /// The `pur` and `pre` columns of a relation's first record; `None` for an unknown id
    /// (the original's false) [soft 0x0813a51c si_reldictGetRelUpdlattr].
    pub fn rel_updlattr(&self, id: i32) -> Option<(i32, i32)> {
        match self.rel_index_from_id(id) {
            i if i < 0 => None,
            i => Some((self.relations[i as usize].upd[0] as i32, self.relations[i as usize].upd[1] as i32)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------------------------

/// The file being read, the token buffer, and the dictionaries being filled.
///
/// `tok` shadows the original's global token buffer `SI_dbInToken`: the readers upper-case it
/// and cut it up in place (`strtok`), and later comparisons see the result.
struct Reader<'a> {
    f: AsciiFile<'a>,
    tok: Vec<u8>,
    out: Relations,
    info: ReadInfo,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { f: AsciiFile::new(data), tok: Vec::new(), out: Relations::new(), info: ReadInfo::default() }
    }

    /// `SI_fileTokenRead` into the token buffer.
    fn tread(&mut self) -> bool {
        let r = self.f.token_read();
        self.tok.clear();
        self.tok.extend_from_slice(self.f.token());
        r
    }

    /// `SI_fileTokenReach(file, word, ';')`
    fn reach(&mut self, word: &str) -> bool {
        let before = self.f.tokens_read;
        let r = self.f.token_reach(word, b';');
        self.tok.clear();
        self.tok.extend_from_slice(self.f.token());
        self.info.reach_skipped += self.f.tokens_read - before - 1;
        r
    }

    /// Notes the token in the buffer as one a reader's loop passed over.
    fn pass_over(&mut self) {
        *self.info.skipped.entry(self.tok.clone()).or_insert(0) += 1;
    }

    fn say(&mut self, channel: Channel, text: String) {
        let offset = self.f.tell();
        self.info.messages.push(Message { channel, text, offset });
    }

    /// True when the file has ended and every further read gives an empty token.
    fn at_end(&self, read_ok: bool) -> bool {
        !read_ok && self.tok.is_empty() && self.f.rest().is_empty()
    }

    /// The text in double quotes that may follow an attribute
    /// [soft 0x0813b910 reldictReadDescription]. `None` if the next thing after blanks is not
    /// a quote (that byte is put back). The text runs to the next quote or the end of the
    /// file, line ends included.
    ///
    /// The original collects the text in a buffer of 80 bytes that it enlarges by 20 when it
    /// is full, and the byte that found it full is not stored: a description loses its 81st
    /// byte and, counting what is kept, every 21st after it. Reproduced. (The enlarged
    /// buffer is also one byte short for its terminator when the text ends exactly at its
    /// size; that overrun has no counterpart here.)
    fn read_description(&mut self) -> (Option<Vec<u8>>, bool) {
        let c = self.f.skip_blanks();
        if c != b'"' as i32 {
            // ungetc: nothing for −1, which is the end of the file or a 0xff byte (swallowed)
            if c != -1 {
                self.f.seek(self.f.tell() - 1);
            }
            return (None, false);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut size = 0x50;
        let mut cut = false;
        loop {
            let c = self.f.getc();
            if c == b'"' as i32 || c == EOF {
                break;
            }
            if buf.len() < size {
                buf.push(c as u8);
            } else {
                size += 0x14;
                cut = true;
            }
        }
        // a C string: it ends at the first NUL
        if let Some(n) = buf.iter().position(|&b| b == 0) {
            buf.truncate(n);
        }
        (Some(buf), cut)
    }

    /// The pick message of a custom effect's `RES` or `ARG` line, after its opening quote:
    /// tokens joined by single blanks, up to and without a token that ends in a quote
    /// [soft 0x0813b734 reldictCusReadPickMsg]. The end of the file also ends it.
    ///
    /// For an empty token (the end of the file) the original tests the byte *before* its copy
    /// of the token for a quote: heap bookkeeping. Here an empty token has no quote.
    fn cus_read_pick_msg(&mut self) -> Vec<u8> {
        let mut msg: Vec<u8> = Vec::new();
        loop {
            let r = self.tread();
            let mut n = self.tok.len();
            let mut closed = false;
            if n > 0 && self.tok[n - 1] == b'"' {
                n -= 1;
                closed = true;
            }
            if !msg.is_empty() {
                msg.push(b' ');
            }
            msg.extend_from_slice(&self.tok[..n]);
            if !r || closed {
                return msg;
            }
        }
    }

    /// The end of a custom effect's `RES` or `ARG` line: scope, condition, pick message, and
    /// an optional `MAX` with a fan-out [soft 0x0813b9f0 reldictCusReadCommonSection].
    ///
    /// - scope: a word, upper-cased (`NODE`, `BRANCH`, `TREE`);
    /// - condition: `COND(Attribute=CODE,…)`, or any other word (`-`) for none;
    /// - then, after blanks: a quote starts the pick message; a `-` (that one byte) stands
    ///   for none; anything else is put back and ends the line's reading here;
    /// - then, if the next token upper-cased is `MAX`, one more token: the fan-out. If it is
    ///   not, the file is moved back to before it (the token buffer keeps it, upper-cased).
    ///
    /// `sec.scope` is written only if its token was read; the original's other three outputs
    /// are reset first.
    fn cus_read_common_section(&mut self, sec: &mut CusSection) -> Result<bool, Error> {
        let mut done = false;
        sec.cond = None;
        sec.pick_msg = None;
        sec.fanout = 1;
        let mut r = self.tread();
        if r {
            token2uppercase(&mut self.tok);
            sec.scope = convert_scope_token(&self.tok) as i16;
            r = self.tread();
        }
        if r {
            let text = self.tok.clone();
            sec.cond = self.convert_condition_token(&text, None)?.0;
            let c = self.f.skip_blanks();
            if c == b'"' as i32 {
                sec.pick_msg = Some(self.cus_read_pick_msg());
                r = true;
            } else if c != b'-' as i32 {
                if c != -1 {
                    self.f.seek(self.f.tell() - 1);
                }
                done = true;
            }
        }
        if !done {
            let pos = self.f.tell();
            if r {
                r = self.tread();
            }
            if r {
                token2uppercase(&mut self.tok);
                if self.tok != b"MAX" {
                    self.f.seek(pos);
                } else {
                    r = self.tread();
                    if r {
                        token2uppercase(&mut self.tok);
                        // the converter cannot fail here, so the original's "Error reading
                        // fanout in OPERATOR section in .cus file" is never raised
                        sec.fanout = cus_convert_fanout_token(&self.tok);
                    }
                }
            }
        }
        Ok(r)
    }

    /// `COND(Attribute=CODE,CODE,…)` [soft 0x0813b374 reldictConvertConditionToken]. Returns
    /// the condition and the "it was a condition word" flag.
    ///
    /// The text is cut at the characters `(=,)`. The first piece, upper-cased, must be
    /// `COND`: if not, no condition and the flag is false. The second piece names an
    /// attribute, found by name without regard to letter case among all attributes read so
    /// far; without a second piece (`COND()`) there is no condition, flag true. Every
    /// further piece is the code of a value of **that attribute's** enumeration, again
    /// without regard to case.
    ///
    /// `attr` is the original's other mode (a flag and an attribute): the text is then only
    /// the codes, and the condition names that attribute's id. No caller uses it.
    ///
    /// Differences: the original copies the text into 128 bytes of stack without a check (a
    /// longer token overruns them; here it is just read). If the attribute named is unknown
    /// and at least one code follows, the original reads the enumeration number through a
    /// null pointer: an error here.
    fn convert_condition_token(
        &mut self,
        text: &[u8],
        attr: Option<&Attribute>,
    ) -> Result<(Option<AttribCond>, bool), Error> {
        let pieces = strtok_pieces(text, b"(=,)");
        let mut pieces = pieces.into_iter();
        let mut ok = false;
        let mut cond = AttribCond::default();
        let enum_type: Option<i16>;
        match attr {
            None => {
                let Some(first) = pieces.next() else { return Ok((None, false)) };
                let mut first = first.to_vec();
                token2uppercase(&mut first);
                if first != b"COND" {
                    return Ok((None, false));
                }
                ok = true;
                let Some(name) = pieces.next() else { return Ok((None, true)) };
                let found = self.out.attribute_by_name(name);
                cond.attribute = found.map_or(-1, |i| i as i32);
                enum_type = found.map(|i| self.out.attributes[i].enum_type);
            }
            Some(a) => {
                cond.attribute = a.id;
                enum_type = Some(a.enum_type);
            }
        }
        for code in pieces {
            let Some(enum_type) = enum_type else {
                return Err(Error::BadPointer { what: "a condition on an attribute that does not exist", offset: self.f.tell() });
            };
            cond.values.push(self.out.enum_value_by_code(enum_type as i32, code).map_or(-1, |v| v.id));
        }
        Ok((Some(cond), ok))
    }

    /// The chapter column [soft 0x0813b58c reldictConvertChapterToken]: `*` is any chapter;
    /// a name the element dictionary does not have is added to it as a new chapter.
    fn convert_chapter_token(&mut self) -> i32 {
        if self.tok == b"*" {
            return CHAPTER_ANY as i32;
        }
        let id = self.out.find_chapter(&self.tok);
        if id >= 0 {
            return id;
        }
        let name = self.tok.clone();
        self.out.add_chapter(&name)
    }

    /// The type column [soft 0x0813ad5c reldictConvertTypeToken].
    ///
    /// A word starting with `ENM` is cut at `(=,)`: the second piece names the enumeration,
    /// whose index (−1 if unknown) goes into the attribute, and the type is 3. Without a
    /// second piece (a bare `ENM`) the type is 4, the integer. Any word it does not know is
    /// 1, the real.
    fn convert_type_token(&mut self, attr: &mut Attribute) -> i32 {
        if self.tok.starts_with(b"ENM") {
            let name = strtok_pieces(&self.tok, b"(=,)").get(1).map(|p| p.to_vec());
            after_strtok(&mut self.tok, b"(=,)");
            return match name {
                None => TYPE_INT,
                Some(name) => {
                    attr.enum_type = self.out.enum_type_index(&name) as i16;
                    TYPE_ENM
                }
            };
        }
        match self.tok.as_slice() {
            b"BOO" => TYPE_BOO,
            b"INT" => TYPE_INT,
            b"STR" => TYPE_STR,
            b"COL" => TYPE_COL,
            b"COORD" => TYPE_COORD,
            _ => TYPE_FLT, // `FLT` and everything else
        }
    }

    /// One attribute, from after its `EAT` or `OAT` keyword
    /// [soft 0x0813bbd8 reldictReadOneAttribute]. The result is that of the last token read
    /// (or of the description: true).
    ///
    /// Columns of a standard attribute (`cus` 0), one token each: id (`atoi`: `xx` is 0),
    /// name, access mode, scope, type, range (`-` for none), group, rank, count (`INF` or a
    /// number), `vis`, increment mode; then, if the count is above zero, tokens of
    /// comma-separated fcurve ids until there are as many as the count, tokens of
    /// comma-separated fcurve names likewise; then an optional description in quotes.
    ///
    /// A custom effect's attribute (`cus` 1) has only name, access mode, type and range, the
    /// words upper-cased except the name; the rest is fixed: id 20000, scope 0, group 5,
    /// rank 50, count 1, increment mode 3, one fcurve of id 20000 named by the first six
    /// bytes of the attribute's name.
    ///
    /// The record is the caller's and is only written where a column is read, so a column
    /// the line does not have keeps what the record held: the enumeration number unless the
    /// type is `ENM(…)`, the two bounds and their brackets when the range is `-`, the
    /// description when the file ended. Where the caller is the elements reader that is the
    /// previous attribute of the block; see `Reader::read_one_operator_attribute` for the
    /// other caller.
    ///
    /// Differences: with a count of zero or less the original leaves the record's two array
    /// pointers as they were (nothing walks them, the count being what it is); here the
    /// lists are emptied. More ids or names in a token than the count overrun the original's
    /// arrays; here the lists grow. A file that ends inside the two lists makes the original
    /// read empty tokens for ever: an error here.
    fn read_one_attribute(&mut self, cus: i16, attr: &mut Attribute) -> Result<bool, Error> {
        if cus == 1 {
            attr.id = CUSTOM_ID;
        } else {
            self.tread();
            attr.id = cnum::atoi(&self.tok);
        }
        self.tread();
        attr.name = self.tok.clone();
        if self.tread() && cus != 0 {
            token2uppercase(&mut self.tok);
        }
        attr.access = convert_access_mode_token(&self.tok);
        if cus == 1 {
            attr.scope = 0;
        } else {
            self.tread();
            attr.scope = convert_scope_token(&self.tok) as i16;
        }
        if self.tread() && cus != 0 {
            token2uppercase(&mut self.tok);
        }
        attr.kind = self.convert_type_token(attr);
        let mut r = self.tread();
        if self.tok != b"-" {
            if !convert_range_token(&mut self.tok, attr) {
                let text = format!("Attribute \"{}\" : invalid range specification", String::from_utf8_lossy(&attr.name));
                self.say(Channel::Stdout, text);
                attr.max_kind = 0;
                attr.min_kind = 0;
            }
        } else {
            attr.max_kind = 0;
            attr.min_kind = 0;
        }
        if cus == 1 {
            attr.group = GROUP_OTHER;
            attr.rank = 0x32;
            attr.count = 1;
            attr.fcurves = 1;
            attr.dope_display = 0;
            attr.incmode = 3;
            // si_attdictAllocateFcurveArray, then the one fcurve
            attr.fcurve_ids = vec![CUSTOM_ID as i16];
            attr.fcurve_names = vec![Some(attr.name.iter().take(6).copied().collect())];
            return Ok(r);
        }
        self.tread();
        attr.group = convert_group_token(&self.tok) as i16;
        self.tread();
        attr.rank = cnum::atoi(&self.tok) as i16;
        self.tread();
        if self.tok == b"INF" {
            attr.fcurves = 1;
            attr.count = 0x7fff;
        } else {
            attr.count = cnum::atoi(&self.tok) as i16;
            attr.fcurves = attr.count;
        }
        self.tread();
        attr.dope_display = cnum::atoi(&self.tok) as i16;
        r = self.tread();
        attr.incmode = convert_attrib_incmode_token(&self.tok);
        attr.fcurve_ids.clear();
        attr.fcurve_names.clear();
        let mut overrun = false;
        if attr.fcurves > 0 {
            let n = attr.fcurves as usize;
            // every id and every name is at least one byte of the file: with fewer bytes left
            // than the count the lists cannot be completed, and the original ends up reading
            // empty tokens for ever. Said before anything is allocated for the count.
            if n > self.f.rest().len() {
                return Err(Error::EndlessLoop { what: "the fcurve ids of an attribute", offset: self.f.tell() });
            }
            // si_attdictAllocateFcurveArray: ids of −1, no names
            attr.fcurve_ids = vec![-1; n];
            attr.fcurve_names = vec![None; n];
            let mut i = 0;
            loop {
                r = self.tread();
                if self.at_end(r) {
                    return Err(Error::EndlessLoop { what: "the fcurve ids of an attribute", offset: self.f.tell() });
                }
                for p in strtok_pieces(&self.tok, b",") {
                    let v = cnum::atoi(p) as i16;
                    if i < n {
                        attr.fcurve_ids[i] = v;
                    } else {
                        attr.fcurve_ids.push(v);
                        overrun = true;
                    }
                    i += 1;
                }
                after_strtok(&mut self.tok, b",");
                if i >= n {
                    break;
                }
            }
            i = 0;
            loop {
                r = self.tread();
                if self.at_end(r) {
                    return Err(Error::EndlessLoop { what: "the fcurve names of an attribute", offset: self.f.tell() });
                }
                for p in strtok_pieces(&self.tok, b",") {
                    if i < n {
                        attr.fcurve_names[i] = Some(p.to_vec());
                    } else {
                        attr.fcurve_names.push(Some(p.to_vec()));
                        overrun = true;
                    }
                    i += 1;
                }
                after_strtok(&mut self.tok, b",");
                if i >= n {
                    break;
                }
            }
        }
        if overrun {
            self.info.fcurve_overruns.push(self.out.attributes.len());
        }
        if r {
            let (text, cut) = self.read_description();
            attr.description = text;
            if cut {
                self.info.descriptions_cut.push(self.out.attributes.len());
            }
        }
        Ok(r)
    }

    /// The columns `RES` and `ARG` share [soft 0x0813c330 reldictReadOneSection]: id (not in
    /// a custom effect), name, chapter, group, rank (50 in a custom effect, which has no such
    /// column). A custom effect's chapter and group words are upper-cased first, and an
    /// unknown group is an error there; in the standard file it is quietly −1. The result is
    /// that of the last token read.
    fn read_one_section(&mut self, cus: i16, sec: &mut OperSection) -> bool {
        if cus != 0 {
            sec.id = 0;
        } else {
            self.tread();
            sec.id = cnum::atoi(&self.tok) as i16;
        }
        self.tread();
        sec.name = self.tok.clone();
        if self.tread() && cus != 0 {
            token2uppercase(&mut self.tok);
        }
        sec.chapter = self.convert_chapter_token() as i16;
        let mut r = self.tread();
        if r && cus != 0 {
            token2uppercase(&mut self.tok);
        }
        sec.group = convert_group_token(&self.tok) as i16;
        if cus != 0 {
            if sec.group == -1 {
                self.say(Channel::SiError(4), si_rt::text(0x0868_4860).into());
                self.say(Channel::Status(3), si_rt::text(0x0868_4860).into());
                return false;
            }
            sec.rank = 0x32;
        } else {
            r = self.tread();
            sec.rank = cnum::atoi(&self.tok) as i16;
        }
        r
    }

    /// A `RES` line, added to the operator's results
    /// [soft 0x0813c588 reldictReadOneOpResult].
    fn read_one_op_result(&mut self, cus: i16, results: &mut Vec<OperResult>) -> Result<bool, Error> {
        let mut res = OperResult::default();
        let mut r = self.read_one_section(cus, &mut res.section);
        if r && cus == 1 {
            let mut sec = CusSection::default();
            r = self.cus_read_common_section(&mut sec)?;
            res.cus = Some(sec);
        }
        results.push(res);
        Ok(r)
    }

    /// An `ARG` line, added to the operator's arguments
    /// [soft 0x0813c63c reldictReadOneOpArgument].
    ///
    /// After the shared columns a standard line has five more tokens: `mx` (`INF` is 0x7fff),
    /// `vis`, `pur`, `pre`, and a last one that is read and dropped (the column the file's
    /// title lines call `unu`). A custom effect's line has one number, stored where `pre`
    /// goes, and then the common part; its `mx` is 0x7fff, `vis` and `pur` 0.
    ///
    /// If the shared columns fail, the original has already counted the new argument and
    /// leaves it unwritten; here it is an empty record.
    fn read_one_op_argument(&mut self, cus: i16, args: &mut Vec<OperArgument>) -> Result<bool, Error> {
        let mut arg = OperArgument::default();
        if !self.read_one_section(cus, &mut arg.section) {
            args.push(OperArgument::default());
            return Ok(false);
        }
        if cus == 1 {
            arg.max = 0x7fff;
            arg.dope_display = 0;
            arg.pur = 0;
        } else {
            self.tread();
            arg.max = if self.tok == b"INF" { 0x7fff } else { cnum::atoi(&self.tok) };
            self.tread();
            arg.dope_display = cnum::atoi(&self.tok) as i16;
            self.tread();
            arg.pur = cnum::atoi(&self.tok) as i16;
        }
        let mut r = self.tread();
        arg.pre = cnum::atoi(&self.tok) as i16;
        if cus == 0 {
            r = self.tread();
        }
        if r && cus == 1 {
            let mut sec = CusSection::default();
            r = self.cus_read_common_section(&mut sec)?;
            arg.cus = Some(sec);
        }
        args.push(arg);
        Ok(r)
    }

    /// The `OPERATOR` line, from after its keyword [soft 0x0813ca90 reldictReadOpDefinition]:
    /// id, name, then flag words in any order and number, each tried as a scope, an
    /// application, a category, `RENDERING` and an increment mode, in that order. The first
    /// token that is none of these ends the line; it stays in the token buffer for the
    /// caller. The result is that of the last token read.
    fn read_op_definition(&mut self, def: &mut OperDef) -> bool {
        self.tread();
        def.id = cnum::atoi(&self.tok);
        let mut r = self.tread();
        def.name = self.tok.clone();
        def.scope = 0;
        def.application = 0;
        def.category = 0;
        def.render = 0;
        def.incmode = 3;
        while r {
            r = self.tread();
            let v = convert_scope_token(&self.tok);
            if v >= 0 {
                def.scope = v as i16;
                continue;
            }
            let v = convert_application_token(&self.tok);
            if v >= 0 {
                def.application = v as i16;
                continue;
            }
            let v = convert_op_categ_token(&self.tok);
            if v >= 0 {
                def.category = v;
                continue;
            }
            let (flag, v) = convert_render_token(&self.tok);
            if v >= 0 {
                def.render = flag;
                continue;
            }
            let v = convert_incmode_token(&self.tok);
            if v < 0 {
                break;
            }
            def.incmode = v;
        }
        r
    }

    /// The group an operator's attributes go to [soft 0x0813d098 attdictGetAttribGroup].
    ///
    /// Without a parent (the standard file): a new top-level group named after the operator
    /// is added, and the result is the **first** group of that name anywhere in the
    /// dictionary, which is an older one if the name was taken. With a parent (a custom
    /// effect's): the parent group is found or added, then the operator's group found or
    /// added under it.
    fn attdict_get_attrib_group(&mut self, def: &OperDef, parent: Option<&[u8]>) -> Option<GroupPath> {
        match parent {
            Some(parent) => {
                let mut path = self.out.group_by_name(parent);
                if path.is_none() {
                    self.out.groups.push(AttribGroup { name: parent.to_vec(), rel_type: 1, ..Default::default() });
                    path = self.out.group_by_name(parent);
                }
                let mut path = path?;
                let base = path.len();
                if sub_group_get_by_name(self.out.group(&path[..base]), &def.name, &mut path) {
                    return Some(path);
                }
                let g = self.out.group_mut(&path);
                g.subgroups.push(AttribGroup { name: def.name.clone(), ..Default::default() });
                path.push(g.subgroups.len() - 1);
                Some(path)
            }
            None => {
                self.out.groups.push(AttribGroup { name: def.name.clone(), rel_type: 1, ..Default::default() });
                self.out.group_by_name(&def.name)
            }
        }
    }

    /// Adds an attribute to the dictionary's array and to a group, as
    /// `si_attdictAddAttribute` (0x0811e984, not ported) does; the index of the attribute.
    fn add_attribute(&mut self, group: &[usize], attr: &Attribute) -> usize {
        let index = self.out.attributes.len();
        self.out.attributes.push(attr.clone());
        self.out.group_mut(group).attributes.push(index);
        index
    }

    /// Hands a relation to the dictionary. The arguments are those of
    /// `SI_reldictAddRelation` (0x08139b80, not ported), source before destination in each
    /// pair; it stores all but the id and the increment mode as 16-bit values.
    #[allow(clippy::too_many_arguments)]
    fn add_relation(
        &mut self,
        name: &[u8],
        id: i32,
        chapters: (i32, i32),
        groups: (i32, i32),
        rank: i32,
        fanout: i32,
        dope_display: i16,
        upd: [i32; 2],
        render: i16,
        incmode: i32,
    ) -> usize {
        self.out.relations.push(Relation {
            id,
            name: name.to_vec(),
            src_chapter: chapters.0 as i16,
            dst_chapter: chapters.1 as i16,
            src_group: groups.0 as i16,
            dst_group: groups.1 as i16,
            rank: rank as i16,
            fanout: fanout as i16,
            dope_display,
            upd: [upd[0] as i16, upd[1] as i16],
            render,
            incmode,
        });
        self.out.relations.len() - 1
    }

    /// An `OAT` line of a standard operator: the attribute is read, added to the operator's
    /// group, and added to the relation dictionary as a relation of its own
    /// `reldictReadOneOperatorAttribute` (0x0813d12c; its standard-file mode only, so not counted as ported), for its `cus` argument 0. Not
    /// ported: what it does for a custom effect (it refuses a name the group already has,
    /// and with `SI_CUSTOM_VERIFY` set in the environment warns about a name that a standard
    /// fcurve has).
    ///
    /// The relation goes from the chapter of the operator's **first** `RES` line, or from the
    /// chapter of the **latest** `ARG` line if that is `SHAPES`, `CLUSTERS` or `LATTICES`,
    /// to `ANIMATION`.
    ///
    /// The original's attribute record is a local that it does not initialise beyond owner
    /// kind and owner id, so the columns an `OAT` line lacks (see
    /// `Reader::read_one_attribute`) are whatever the stack held: the enumeration number
    /// unless the type is `ENM(…)`, the bounds and their brackets when the range is `-`.
    /// Here they have the values of [`Attribute::init`]. The result of reading the attribute
    /// is ignored, as in the original.
    ///
    /// An `OAT` line before any `RES` line reads through a null pointer in the original, one
    /// before any `ARG` line through an uninitialised one: errors here.
    fn read_one_operator_attribute(
        &mut self,
        group: Option<&GroupPath>,
        def: &OperDef,
        results: &[OperResult],
        last_arg: Option<&OperArgument>,
    ) -> Result<(usize, usize), Error> {
        let mut attr = Attribute::init();
        attr.owner_kind = 1;
        attr.owner_id = def.id as i16;
        self.read_one_attribute(0, &mut attr)?;
        let Some(group) = group else {
            return Err(Error::BadPointer { what: "an operator attribute with no group", offset: self.f.tell() });
        };
        let index = self.add_attribute(group, &attr);
        let Some(first_result) = results.first() else {
            return Err(Error::BadPointer { what: "OAT before any RES in an operator", offset: self.f.tell() });
        };
        let Some(arg) = last_arg else {
            return Err(Error::BadPointer { what: "OAT before any ARG in an operator", offset: self.f.tell() });
        };
        let mut chapter = first_result.section.chapter;
        if [CHAPTER_SHAPES, CHAPTER_CLUSTERS, CHAPTER_LATTICES].contains(&arg.section.chapter) {
            chapter = arg.section.chapter;
        }
        let chapters = (chapter as i32, CHAPTER_ANIMATION as i32);
        let rel = self.add_relation(&attr.name, attr.id, chapters, (attr.group as i32, 0), attr.rank as i32, attr.count as i32, attr.dope_display, [0, 0], 0, attr.incmode);
        Ok((index, rel))
    }

    /// One `OPERATOR` block of the standard file, from after its keyword to the keyword that
    /// ends it `SI_reldictReadOneOperator` (0x0813d75c; its standard-file mode only, so not counted as ported), for its `cus` argument 0. The
    /// same function reads the `_OPERATOR` section of a custom effect file (`.cus`) when
    /// called by `SI_cusLoadEffect` with `cus` 1; that mode (the `OPTIONS:` line, `NEWRES`,
    /// `_END`, the checks on repeated names, one relation per result and argument pair) is
    /// **not** ported.
    ///
    /// After the definition line, tokens are taken one at a time:
    /// - `RES`: a result line;
    /// - `ARG`: an argument line, and one relation added to the dictionary: from the chapter
    ///   and group of the **first** `RES` line to those of this `ARG`, with the rank of that
    ///   `RES` line and the `mx`, `vis`, `pur` and `pre` columns of this `ARG`;
    /// - `OAT`: an attribute line;
    /// - `OPERATOR`, `EndOfOPERATORS`, `EndOfOPERATOR`: the end of the block;
    /// - anything else is passed over, as is everything after one of the three lines up to
    ///   the next of these six words.
    ///
    /// The result is that of the last token read: false if the file ended inside the block
    /// or a `RES` or `ARG` line was cut short by it.
    ///
    /// An `ARG` line before any `RES` line reads through a null pointer in the original: an
    /// error here.
    fn read_one_operator(&mut self) -> Result<bool, Error> {
        let mut op = Operator::default();
        let mut group: Option<GroupPath> = None;
        let mut need_group = true;
        self.read_op_definition(&mut op.def); // its result is not looked at
        let mut r = true;
        let ended = loop {
            if !r {
                break r;
            }
            match self.tok.as_slice() {
                b"OPERATOR" | b"EndOfOPERATORS" | b"EndOfOPERATOR" => break r,
                b"RES" => {
                    if !self.read_one_op_result(0, &mut op.results)? {
                        self.say(Channel::SiError(4), si_rt::text(0x0868_4e00).into());
                        break false;
                    }
                    op.results.last_mut().unwrap().new_result = 0;
                }
                b"ARG" => {
                    if !self.read_one_op_argument(0, &mut op.arguments)? {
                        self.say(Channel::SiError(4), si_rt::text(0x0868_4ee0).into());
                        break false;
                    }
                    let arg = op.arguments.last().unwrap();
                    let Some(res) = op.results.first() else {
                        return Err(Error::BadPointer { what: "ARG before any RES in an operator", offset: self.f.tell() });
                    };
                    let (a, s) = (arg.clone(), res.section.clone());
                    let chapters = (s.chapter as i32, a.section.chapter as i32);
                    let groups = (s.group as i32, a.section.group as i32);
                    let upd = [a.pur as i32, a.pre as i32];
                    let rel = self.add_relation(&op.def.name, op.def.id, chapters, groups, s.rank as i32, a.max, a.dope_display, upd, op.def.render, op.def.incmode);
                    op.relations.push(rel);
                }
                b"OAT" => {
                    if need_group {
                        group = self.attdict_get_attrib_group(&op.def, None);
                        need_group = false;
                    }
                    let (attr, rel) = self.read_one_operator_attribute(group.as_ref(), &op.def, &op.results, op.arguments.last())?;
                    op.attributes.push(attr);
                    op.relations.push(rel);
                }
                _ => self.pass_over(),
            }
            loop {
                r = self.tread();
                if !r || matches!(self.tok.as_slice(), b"OPERATOR" | b"EndOfOPERATOR" | b"EndOfOPERATORS" | b"RES" | b"ARG" | b"OAT") {
                    break;
                }
                self.pass_over();
            }
        };
        // the original frees the results and the arguments here (not after a failed line)
        self.out.operators.push(op);
        Ok(ended)
    }

    /// The `OPERATORS` section [soft 0x0813df94 reldictReadOperators]: operators are read
    /// from the first `OPERATOR` after the word `OPERATORS` until one ends on
    /// `EndOfOPERATORS`, or fails.
    fn read_operators(&mut self) -> Result<bool, Error> {
        self.reach("OPERATORS");
        let mut r = self.reach("OPERATOR");
        if !r || self.tok == b"EndOfOPERATORS" {
            return Ok(r);
        }
        loop {
            r = self.read_one_operator()?;
            if !r || self.tok == b"EndOfOPERATORS" {
                return Ok(r);
            }
        }
    }

    /// The `ELEMENTS_ATTRIBUTES` section [soft 0x0813cc20 reldictReadElemsAttributes].
    ///
    /// After the section word, tokens are passed over up to the first `ELEMENT` or
    /// `SUBELEMENT`. Then, block after block:
    ///
    /// - `ELEMENT name`: a new top-level attribute group of that name.
    /// - `SUBELEMENT name OF parent` (tokens between the name and `OF` are passed over): the
    ///   group is the one called `name` under the first group called `parent`, which a `SUB`
    ///   line of an earlier block must have made. The block's chapter is the chapter called
    ///   `name`, else the one called `parent`, else none (−1).
    /// - Everything up to the first `EAT` is passed over (so a `SUB` line before the first
    ///   `EAT` of a block is lost, and a block without an `EAT` runs on into the next).
    /// - `EAT`: an attribute. It is added to the group and, as a relation of its own, to the
    ///   relation dictionary: from the block's chapter to `ANIMATION`.
    /// - `SUB id name COND(…)`: a sub-group of the block's group with that condition. The id
    ///   is read and dropped. A third token that is not a condition gets a message on
    ///   standard error and the sub-group is added without one.
    /// - After either line, tokens are passed over up to the next `EAT`, `SUB`,
    ///   `EndOfELEMENT` or `EndOfSUBELEMENT`. That is how lower-case `eat` lines, title lines
    ///   and rules are skipped.
    /// - After the block's end, tokens are passed over up to `ELEMENT`, `SUBELEMENT` or
    ///   `EndOfELEMENTS_ATTRIBUTES`.
    ///
    /// One attribute record serves a whole block, which is how a column an `EAT` line does
    /// not write keeps the previous line's value (see `Reader::read_one_attribute`).
    ///
    /// The result is that of the last token read, or false after "Error reading attribute"
    /// (an attribute whose last token ran into the end of the file).
    ///
    /// A `SUBELEMENT` whose group does not exist: its `SUB` lines do nothing in the original,
    /// and its first `EAT` writes through a null pointer: an error here.
    fn read_elems_attributes(&mut self) -> Result<bool, Error> {
        self.reach("ELEMENTS_ATTRIBUTES");
        let mut r;
        loop {
            r = self.tread();
            if !r {
                return Ok(false);
            }
            if self.tok == b"ELEMENT" || self.tok == b"SUBELEMENT" {
                break;
            }
            self.pass_over();
        }
        if self.tok == b"EndOfELEMENTS_ATTRIBUTES" {
            return Ok(r);
        }
        loop {
            let mut attr = Attribute::init();
            attr.owner_kind = 0;
            attr.owner_id = -1;
            let sub = self.tok == b"SUBELEMENT";
            self.tread();
            let name = self.tok.clone();
            let mut block = ElementBlock { sub, name: name.clone(), chapter: -1, ..Default::default() };
            let group: Option<GroupPath>;
            if sub {
                self.reach("OF");
                self.tread();
                let parent = self.tok.clone();
                group = self.out.group_by_name(&parent).and_then(|mut path| {
                    let base = path.len();
                    sub_group_get_by_name(self.out.group(&path[..base]), &name, &mut path).then_some(path)
                });
                attr.owner_id = self.out.find_chapter(&name) as i16;
                if attr.owner_id < 0 {
                    attr.owner_id = self.out.find_chapter(&parent) as i16;
                }
                block.parent = Some(parent);
            } else {
                self.out.groups.push(AttribGroup { name: name.clone(), rel_type: 0, ..Default::default() });
                group = self.out.group_by_name(&name);
            }
            block.chapter = attr.owner_id;
            block.group = group.clone();
            let index = self.out.blocks.len();
            self.out.blocks.push(block);
            r = self.reach("EAT");
            if r && self.tok != b"EndOfELEMENT" && self.tok != b"EndOfSUBELEMENT" {
                loop {
                    if self.tok == b"SUB" {
                        self.tread();
                        self.tread();
                        let sub_name = self.tok.clone();
                        self.tread();
                        let text = self.tok.clone();
                        let (cond, ok) = self.convert_condition_token(&text, None)?;
                        if !ok {
                            self.say(Channel::Stderr, format!("Invalid Condition Token [{}]", String::from_utf8_lossy(&text)));
                        }
                        // si_attdictAddAttribSubGroup: nothing when there is no group
                        if let Some(path) = &group {
                            self.out.group_mut(path).subgroups.push(AttribGroup { name: sub_name.clone(), cond, ..Default::default() });
                        }
                        self.out.blocks[index].subs.push(sub_name);
                    } else {
                        if !self.read_one_attribute(0, &mut attr)? {
                            self.say(Channel::SiError(4), si_rt::text(0x0868_4960).into());
                            return Ok(false);
                        }
                        let Some(path) = &group else {
                            return Err(Error::BadPointer { what: "EAT in a SUBELEMENT that no SUB line declared", offset: self.f.tell() });
                        };
                        let a = self.add_attribute(path, &attr);
                        self.out.blocks[index].attributes.push(a);
                        let chapters = (attr.owner_id as i32, CHAPTER_ANIMATION as i32);
                        self.add_relation(&attr.name, attr.id, chapters, (attr.group as i32, 0), attr.rank as i32, attr.count as i32, attr.dope_display, [0, 0], 0, attr.incmode);
                    }
                    loop {
                        r = self.tread();
                        if !r || matches!(self.tok.as_slice(), b"EndOfELEMENT" | b"EndOfSUBELEMENT" | b"SUB" | b"EAT") {
                            break;
                        }
                        self.pass_over();
                    }
                    if !r || self.tok == b"EndOfELEMENT" || self.tok == b"EndOfSUBELEMENT" {
                        break;
                    }
                }
            }
            loop {
                r = self.tread();
                if !r {
                    return Ok(false);
                }
                if matches!(self.tok.as_slice(), b"EndOfELEMENTS_ATTRIBUTES" | b"ELEMENT" | b"SUBELEMENT") {
                    break;
                }
                self.pass_over();
            }
            if self.tok == b"EndOfELEMENTS_ATTRIBUTES" {
                return Ok(r);
            }
        }
    }

    /// The `ENUM_VALUES` section [soft 0x0813e040 reldictReadEnumeratedTypes].
    ///
    /// From the first `ENUM` after the section word: the next token names an enumeration;
    /// everything up to the first `ENM` is passed over; each `ENM` is followed by an id, a
    /// code and a name; after a value, tokens are passed over up to the next `ENM` or
    /// `EndOfENUM`; after `EndOfENUM`, up to the next `ENUM` or `EndOfENUM_VALUES`. An
    /// enumeration without any `ENM` therefore swallows the next one's values. The result is
    /// that of the last token read.
    fn read_enumerated_types(&mut self) -> bool {
        self.reach("ENUM_VALUES");
        let mut r = self.reach("ENUM");
        if !r || self.tok == b"EndOfENUM_VALUES" {
            return r;
        }
        loop {
            self.tread();
            let t = self.out.enums.len();
            self.out.enums.push(EnumType { name: self.tok.clone(), values: Vec::new() });
            if self.reach("ENM") && self.tok != b"EndOfENUM" {
                'values: loop {
                    self.tread();
                    let id = cnum::atoi(&self.tok);
                    self.tread();
                    let code = self.tok.clone();
                    self.tread();
                    let name = self.tok.clone();
                    self.out.enums[t].values.push(EnumValue { id, code, name });
                    loop {
                        if !self.tread() {
                            break 'values;
                        }
                        if self.tok == b"EndOfENUM" || self.tok == b"ENM" {
                            break;
                        }
                        self.pass_over();
                    }
                    if self.tok == b"EndOfENUM" {
                        break;
                    }
                }
            }
            loop {
                r = self.tread();
                if !r {
                    return false;
                }
                if self.tok == b"EndOfENUM_VALUES" || self.tok == b"ENUM" {
                    break;
                }
                self.pass_over();
            }
            if self.tok == b"EndOfENUM_VALUES" {
                return r;
            }
        }
    }

    /// The three sections, each starting where the one before stopped
    /// [soft 0x0813e2d0 SI_reldictReadNewRelations]. "New" is the layout: this is the reader
    /// of every file of version 3.49 or later, the shipped `Relations.rsrc` included. The
    /// results of the first two are dropped; the third's is returned.
    fn read_new_relations(&mut self) -> Result<bool, Error> {
        self.read_enumerated_types();
        self.read_elems_attributes()?;
        self.read_operators()
    }

    /// The table layout: after the word `RELATIONS`, one relation per group of nine tokens
    /// (twelve under the `SREL` tag) until `EndOfRELATIONS`: id, name, source chapter,
    /// destination chapter, source group, destination group, rank, fan-out, dope display;
    /// then, under `SREL`, the two "update" values and a number that is dropped. Part of
    /// `Reader::read_relations`. No file of this layout is on the discs.
    fn read_relations_table(&mut self, is_srel: bool) {
        self.reach("RELATIONS");
        if !self.tread() || self.tok == b"EndOfRELATIONS" {
            return;
        }
        loop {
            let id = cnum::atoi(&self.tok);
            self.tread();
            let name = self.tok.clone();
            self.tread();
            let src = self.convert_chapter_token();
            self.tread();
            let dst = self.convert_chapter_token();
            self.tread();
            let src_group = convert_group_token(&self.tok);
            self.tread();
            let dst_group = convert_group_token(&self.tok);
            self.tread();
            let rank = cnum::atoi(&self.tok);
            self.tread();
            let fanout = cnum::atoi(&self.tok);
            self.tread();
            let dope = cnum::atoi(&self.tok);
            let mut upd = [0, 0];
            if is_srel {
                self.tread();
                upd[0] = cnum::atoi(&self.tok);
                self.tread();
                upd[1] = cnum::atoi(&self.tok);
                self.tread();
            }
            let shown = String::from_utf8_lossy(&name).into_owned();
            if src_group < 0 || dst_group < 0 {
                self.say(Channel::SiError(0x1f), format!("reldictReadRelations() {shown}"));
            }
            // a chapter name is never refused (an unknown one becomes a new chapter), so
            // these two cannot happen; the second is the only fault that drops the line
            if src < 0 && src != CHAPTER_ANY as i32 {
                self.say(Channel::SiError(0x20), format!("reldictReadRelations() source {shown}"));
            }
            if dst < 0 && dst != CHAPTER_ANY as i32 {
                self.say(Channel::SiError(0x20), format!("reldictReadRelations() destination {shown}"));
            } else {
                self.add_relation(&name, id, (src, dst), (src_group, dst_group), rank, fanout, dope as i16, upd, 0, 3);
            }
            if !self.tread() || self.tok == b"EndOfRELATIONS" {
                return;
            }
        }
    }

    /// The whole file, from after `fopen` [soft 0x0813e318 SI_reldictReadRelations].
    ///
    /// The ASCII header is read. A version of 3.49 or more selects the three sections,
    /// whatever the tag (any of the seven the header reader accepts); anything else, a file
    /// without a header included (version −1), is read as the older table, with three more
    /// columns per line if the tag is `SREL`. The original answers 1 in every case once the
    /// file could be opened: no fault in the text is a failure to it.
    ///
    /// After the sections the original sorts the attribute dictionary by fcurve id and
    /// numbers its groups (`SI_attdictSortAttribFromFcurveIds`, `SI_attdictUpdateIndexes`:
    /// not ported).
    fn read_relations(&mut self) -> Result<(), Error> {
        let header = read_asc_header(&mut self.f);
        self.tok = self.f.token().to_vec();
        self.info.tag = header.map(|h| h.0);
        self.info.version = header.map_or(-1.0, |h| h.1);
        let is_srel = self.info.tag == Some(u32::from_be_bytes(*b"SREL"));
        if self.info.version as f64 >= SECTIONS_VERSION {
            self.info.layout = Layout::Sections;
            self.info.result = self.read_new_relations()? as i16;
        } else {
            self.info.layout = Layout::Table;
            self.read_relations_table(is_srel);
        }
        Ok(())
    }
}

/// Reads a relations resource file as the program does at start-up.
///
/// `SI_initDictionaries` (0x0810824c) looks for `Relations.rsrc` in the current directory,
/// then in the resource directory, through `SI_dbLoadRelationDictionary` (0x0813ea84), and
/// exits if neither can be opened. Neither function is ported.
pub fn read(data: &[u8]) -> Result<(Relations, ReadInfo), Error> {
    let mut rd = Reader::new(data);
    rd.read_relations()?;
    rd.info.end = rd.f.tell();
    rd.info.all_tokens_consumed = rd.f.only_blanks_left();
    rd.info.tokens_read = rd.f.tokens_read;
    rd.out.sort_relations();
    Ok((rd.out, rd.info))
}

// ---------------------------------------------------------------------------------------------
// Checks over a file (for the corpus check binary)
// ---------------------------------------------------------------------------------------------

/// How many lines of a text start with each first word. Not part of the reader, which knows
/// no lines: the check compares its counts with these.
pub fn first_words(data: &[u8]) -> BTreeMap<Vec<u8>, usize> {
    let mut out = BTreeMap::new();
    for line in data.split(|&b| b == b'\n') {
        let word: Vec<u8> = line.iter().copied().skip_while(|b| b" \t\r".contains(b)).take_while(|b| !b" \t\r".contains(b)).collect();
        *out.entry(word).or_insert(0) += 1;
    }
    out
}

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The display name of a group number.
pub fn group_name(group: i16) -> &'static str {
    match group {
        0 => "PARAM",
        1 => "TRANSFO",
        2 => "GEOM",
        3 => "RENDER",
        4 => "CUSTOM",
        5 => "OTHER",
        _ => "?",
    }
}

/// One line per operator, with every number the reader took from its block, in a form a
/// script can rebuild from the text (`re/tools/si-ascii/srel_xcheck.py`).
pub fn operator_lines(rel: &Relations) -> Vec<String> {
    let mut out = Vec::new();
    for op in &rel.operators {
        let d = &op.def;
        let mut s = format!("OP {} {} scope={} app={} cat={} render={} inc={}", d.id, show(&d.name), d.scope, d.application, d.category, d.render, d.incmode);
        for r in &op.results {
            let c = &r.section;
            s += &format!(" | RES {} {} {} {} {}", c.id, show(&c.name), rel.chapter_name(c.chapter), c.group, c.rank);
        }
        for a in &op.arguments {
            let c = &a.section;
            s += &format!(" | ARG {} {} {} {} {} {} {} {} {}", c.id, show(&c.name), rel.chapter_name(c.chapter), c.group, c.rank, a.max, a.dope_display, a.pur, a.pre);
        }
        for &i in &op.attributes {
            let a = &rel.attributes[i];
            s += &format!(" | OAT {} {}", a.id, show(&a.name));
        }
        out.push(s);
    }
    out
}

/// One line per attribute, likewise.
pub fn attribute_lines(rel: &Relations) -> Vec<String> {
    let mut out = Vec::new();
    for a in &rel.attributes {
        let ids: Vec<String> = a.fcurve_ids.iter().map(|v| v.to_string()).collect();
        let names: Vec<String> = a.fcurve_names.iter().map(|n| n.as_ref().map_or("-".into(), |n| show(n))).collect();
        out.push(format!(
            "{} {} {} acc={} scope={} type={} enum={} min={}:{}:{} max={}:{}:{} grp={} rank={} count={} fcv={} vis={} inc={} ids={} names={} desc={}",
            if a.owner_kind == 0 { "EAT" } else { "OAT" },
            a.id,
            show(&a.name),
            a.access,
            a.scope,
            a.kind,
            if a.kind == TYPE_ENM { a.enum_type.to_string() } else { "-".into() },
            a.min_kind,
            a.min_inclusive,
            cnum::fmt_g(a.min as f64, 9),
            a.max_kind,
            a.max_inclusive,
            cnum::fmt_g(a.max as f64, 9),
            a.group,
            a.rank,
            a.count,
            a.fcurves,
            a.dope_display,
            a.incmode,
            ids.join(","),
            names.join(","),
            a.description.as_ref().map_or("-".into(), |d| d.len().to_string()),
        ));
    }
    out
}

/// The outcome of checking one file: report lines, and how many of the comparisons failed.
#[derive(Clone, Debug, Default)]
pub struct Check {
    pub lines: Vec<String>,
    pub failures: usize,
    pub relations: Option<Relations>,
}

/// Reads a file and compares what the reader built with a count of the file's lines by first
/// word; lists what the reader passed over and everything odd in the result.
pub fn check(data: &[u8]) -> Check {
    let mut c = Check::default();
    let (rel, info) = match read(data) {
        Ok(x) => x,
        Err(e) => {
            c.lines.push(format!("NOT READ: {e}"));
            c.failures += 1;
            return c;
        }
    };
    let tag = info.tag.map_or("none".into(), |t| show(&t.to_be_bytes()));
    c.lines.push(format!(
        "header {tag} {}  layout {:?}  result {}  stopped at {} of {} bytes  all tokens consumed: {}  tokens read {}",
        cnum::fmt_g(info.version as f64, 6),
        info.layout,
        info.result,
        info.end,
        data.len(),
        info.all_tokens_consumed,
        info.tokens_read
    ));
    if !info.all_tokens_consumed || info.result == 0 {
        c.failures += 1;
    }
    let words = first_words(data);
    let text = |w: &str| words.get(w.as_bytes()).copied().unwrap_or(0);
    let eat = rel.attributes.iter().filter(|a| a.owner_kind == 0).count();
    let oat = rel.attributes.iter().filter(|a| a.owner_kind == 1).count();
    let rows: [(&str, usize, usize); 10] = [
        ("ENUM", text("ENUM"), rel.enums.len()),
        ("ENM", text("ENM"), rel.enums.iter().map(|e| e.values.len()).sum()),
        ("ELEMENT", text("ELEMENT"), rel.blocks.iter().filter(|b| !b.sub).count()),
        ("SUBELEMENT", text("SUBELEMENT"), rel.blocks.iter().filter(|b| b.sub).count()),
        ("EAT", text("EAT"), eat),
        ("SUB", text("SUB"), rel.blocks.iter().map(|b| b.subs.len()).sum()),
        ("OPERATOR", text("OPERATOR"), rel.operators.len()),
        ("RES", text("RES"), rel.operators.iter().map(|o| o.results.len()).sum()),
        ("ARG", text("ARG"), rel.operators.iter().map(|o| o.arguments.len()).sum()),
        ("OAT", text("OAT"), oat),
    ];
    c.lines.push("lines starting with the keyword / read by the reader:".into());
    for (w, t, p) in rows {
        let ok = t == p;
        if !ok {
            c.failures += 1;
        }
        c.lines.push(format!("  {w:<11} {t:5} {p:5}  {}", if ok { "same" } else { "DIFFERENT" }));
    }
    let ids: std::collections::BTreeSet<i32> = rel.relations.iter().map(|r| r.id).collect();
    c.lines.push(format!(
        "dictionaries: {} enumerations, {} top-level groups, {} attributes, {} relation records ({} distinct ids), {} chapters ({} added by the file)",
        rel.enums.len(),
        rel.groups.len(),
        rel.attributes.len(),
        rel.relations.len(),
        ids.len(),
        rel.chapters.len(),
        rel.chapters.len() - CHAPTER_NAMES.len()
    ));
    if rel.relations.len() != eat + oat + rows[8].2 {
        c.failures += 1;
        c.lines.push("  DIFFERENT: relation records are not EAT + OAT + ARG".into());
    }
    for name in &rel.chapters[CHAPTER_NAMES.len()..] {
        c.lines.push(format!("  chapter added: {}", show(name)));
    }
    for m in &info.messages {
        c.lines.push(format!("  message {:?} at {}: {}", m.channel, m.offset, m.text));
    }

    // what the reader never looked at
    let known: [&[u8]; 20] = [
        b"ENUM_VALUES", b"ENUM", b"ENM", b"EndOfENUM", b"EndOfENUM_VALUES", b"ELEMENTS_ATTRIBUTES", b"ELEMENT", b"SUBELEMENT", b"EAT", b"SUB",
        b"EndOfELEMENT", b"EndOfSUBELEMENT", b"EndOfELEMENTS_ATTRIBUTES", b"OPERATORS", b"OPERATOR", b"RES", b"ARG", b"OAT", b"EndOfOPERATORS", b"",
    ];
    c.lines.push("lines whose first word is not a keyword of the reader:".into());
    let mut other: Vec<(&Vec<u8>, &usize)> = words.iter().filter(|(w, _)| !known.contains(&w.as_slice())).collect();
    other.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let (mut lists, mut list_lines) = (0, 0);
    for (w, n) in other {
        if w.contains(&b',') {
            lists += 1;
            list_lines += n;
        } else {
            c.lines.push(format!("  {n:5}  {}", show(w)));
        }
    }
    c.lines.push(format!("  {list_lines:5}  (continuation lines of fcurve id or name lists, {lists} distinct)"));
    c.lines.push(format!("  {:5}  (empty lines)", text("")));
    let passed: usize = info.skipped.values().sum();
    c.lines.push(format!("tokens passed over: {} on the way to a keyword after a section or block start, {} between lines ({} distinct words)", info.reach_skipped, passed, info.skipped.len()));
    for w in ["//", "eat", "/*ENM", "cat"] {
        c.lines.push(format!("  {:5}  {w}", info.skipped.get(w.as_bytes()).copied().unwrap_or(0)));
    }

    // descriptions
    let described = rel.attributes.iter().filter(|a| a.description.is_some()).count();
    c.lines.push(format!("descriptions: {described} of {} attributes have one; {} lost characters to the 80-byte buffer", rel.attributes.len(), info.descriptions_cut.len()));
    for &i in &info.descriptions_cut {
        let a = &rel.attributes[i];
        c.lines.push(format!("  cut: {} ({} bytes kept)", show(&a.name), a.description.as_ref().map_or(0, |d| d.len())));
    }
    c.lines.push(format!("fcurve lists longer than the count: {}", info.fcurve_overruns.len()));
    for &i in &info.fcurve_overruns {
        let a = &rel.attributes[i];
        c.lines.push(format!("  overrun: {} count {} ids {} names {}", show(&a.name), a.fcurves, a.fcurve_ids.len(), a.fcurve_names.len()));
    }

    // attributes
    let mut by = BTreeMap::new();
    for a in &rel.attributes {
        *by.entry(("type", a.kind as i64)).or_insert(0usize) += 1;
        *by.entry(("scope", a.scope as i64)).or_insert(0) += 1;
        *by.entry(("group", a.group as i64)).or_insert(0) += 1;
        *by.entry(("access", a.access as i64)).or_insert(0) += 1;
        *by.entry(("incmode", a.incmode as i64)).or_insert(0) += 1;
        *by.entry(("min kind", a.min_kind as i64)).or_insert(0) += 1;
        *by.entry(("max kind", a.max_kind as i64)).or_insert(0) += 1;
        *by.entry(("vis", a.dope_display as i64)).or_insert(0) += 1;
        if a.min_kind != 0 {
            *by.entry(("min allowed", a.min_inclusive as i64)).or_insert(0) += 1;
        }
        if a.max_kind != 0 {
            *by.entry(("max allowed", a.max_inclusive as i64)).or_insert(0) += 1;
        }
        *by.entry(("id is 0", (a.id == 0) as i64)).or_insert(0) += 1;
        *by.entry(("count is INF", (a.count == 0x7fff) as i64)).or_insert(0) += 1;
    }
    c.lines.push("attributes by column value:".into());
    let mut last = "";
    let mut line = String::new();
    for ((k, v), n) in &by {
        if *k != last {
            if !line.is_empty() {
                c.lines.push(line);
            }
            line = format!("  {k}:");
            last = k;
        }
        line += &format!(" {v}×{n}");
    }
    c.lines.push(line);
    let bad_enum = rel.attributes.iter().filter(|a| a.kind == TYPE_ENM && a.enum_type < 0).count();
    c.lines.push(format!("attributes naming an enumeration that does not exist: {bad_enum}"));
    let mut fcv_seen: BTreeMap<i16, usize> = BTreeMap::new();
    let mut unset = 0;
    for a in &rel.attributes {
        for &id in &a.fcurve_ids {
            *fcv_seen.entry(id).or_insert(0) += 1;
        }
        unset += a.fcurve_names.iter().filter(|n| n.is_none()).count();
    }
    let dup: Vec<String> = fcv_seen.iter().filter(|(_, &n)| n > 1).map(|(id, n)| format!("{id}×{n}")).collect();
    c.lines.push(format!("fcurve ids: {} distinct, from {} to {}; used by more than one attribute: {}; names missing: {unset}", fcv_seen.len(), fcv_seen.keys().next().copied().unwrap_or(0), fcv_seen.keys().last().copied().unwrap_or(0), if dup.is_empty() { "none".into() } else { dup.join(" ") }));

    // groups and conditions
    fn walk(g: &AttribGroup, depth: usize, stat: &mut (usize, usize, usize, usize, usize)) {
        stat.0 += 1;
        stat.1 = stat.1.max(depth);
        if let Some(cond) = &g.cond {
            stat.2 += 1;
            if cond.attribute < 0 {
                stat.3 += 1;
            }
            stat.4 += cond.values.iter().filter(|&&v| v < 0).count();
        }
        for s in &g.subgroups {
            walk(s, depth + 1, stat);
        }
    }
    let mut stat = (0, 0, 0, 0, 0);
    for g in &rel.groups {
        walk(g, 0, &mut stat);
    }
    c.lines.push(format!("groups: {} in all, deepest level {}; {} with a condition, {} on an unknown attribute, {} unknown value codes", stat.0, stat.1, stat.2, stat.3, stat.4));
    for b in &rel.blocks {
        if b.group.is_none() {
            c.failures += 1;
            c.lines.push(format!("  DIFFERENT: block {} has no group", show(&b.name)));
        }
    }
    let no_chapter: Vec<String> = rel.blocks.iter().filter(|b| b.chapter < 0).map(|b| show(&b.name)).collect();
    c.lines.push(format!("blocks whose attributes have no chapter (−1): {}", no_chapter.join(" ")));

    // operators
    let neg = rel.operators.iter().filter(|o| o.def.id < 0).count();
    c.lines.push(format!("operators: {} with a positive id, {} with a negative id", rel.operators.len() - neg, neg));
    let mut seen_id: BTreeMap<i32, usize> = BTreeMap::new();
    let mut seen_name: BTreeMap<&[u8], usize> = BTreeMap::new();
    for o in &rel.operators {
        *seen_id.entry(o.def.id).or_insert(0) += 1;
        *seen_name.entry(&o.def.name).or_insert(0) += 1;
    }
    let dup_id: Vec<String> = seen_id.iter().filter(|(_, &n)| n > 1).map(|(i, n)| format!("{i}×{n}")).collect();
    let dup_name: Vec<String> = seen_name.iter().filter(|(_, &n)| n > 1).map(|(i, n)| format!("{}×{n}", show(i))).collect();
    c.lines.push(format!("  ids used twice: {}; names used twice: {}", if dup_id.is_empty() { "none".into() } else { dup_id.join(" ") }, if dup_name.is_empty() { "none".into() } else { dup_name.join(" ") }));
    let mut shape: BTreeMap<(usize, usize, usize), usize> = BTreeMap::new();
    for o in &rel.operators {
        *shape.entry((o.results.len(), o.arguments.len(), o.attributes.len())).or_insert(0) += 1;
    }
    let shapes: Vec<String> = shape.iter().map(|((r, a, t), n)| format!("{r}/{a}/{t}×{n}")).collect();
    c.lines.push(format!("  RES/ARG/OAT lines per operator: {}", shapes.join(" ")));
    let unknown_group = rel.operators.iter().flat_map(|o| o.results.iter().map(|r| &r.section).chain(o.arguments.iter().map(|a| &a.section))).filter(|s| s.group < 0).count();
    c.lines.push(format!("  RES or ARG lines with an unknown group word: {unknown_group}"));
    for o in &rel.operators {
        if o.attributes.is_empty() {
            continue;
        }
        let path = rel.group_by_name(&o.def.name);
        let own = path.as_ref().is_some_and(|p| p.len() == 1 && rel.groups[p[0]].rel_type == 1 && rel.groups[p[0]].attributes == o.attributes);
        if !own {
            c.lines.push(format!("  operator {} {}: its attributes went to another group of the same name ({:?})", o.def.id, show(&o.def.name), path));
        }
    }
    let flag = |f: &dyn Fn(&OperDef) -> i64| -> String {
        let mut m: BTreeMap<i64, usize> = BTreeMap::new();
        for o in &rel.operators {
            *m.entry(f(&o.def)).or_insert(0) += 1;
        }
        m.iter().map(|(v, n)| format!("{v}×{n}")).collect::<Vec<_>>().join(" ")
    };
    c.lines.push(format!("  scope: {}; application: {}; category: {}; render: {}; incmode: {}", flag(&|d| d.scope as i64), flag(&|d| d.application as i64), flag(&|d| d.category as i64), flag(&|d| d.render as i64), flag(&|d| d.incmode as i64)));

    // relation records
    let mut runs: BTreeMap<i32, (usize, usize)> = BTreeMap::new(); // id -> (records, runs)
    let mut prev: Option<i32> = None;
    for r in &rel.relations {
        let e = runs.entry(r.id).or_insert((0, 0));
        e.0 += 1;
        if prev != Some(r.id) {
            e.1 += 1;
        }
        prev = Some(r.id);
    }
    let several = runs.iter().filter(|(_, v)| v.0 > 1).count();
    let broken: Vec<String> = runs.iter().filter(|(_, v)| v.1 > 1).map(|(id, v)| format!("id {id} ({} records in {} runs)", v.0, v.1)).collect();
    c.lines.push(format!("relation ids with more than one record: {several}; with records that are not all in one run: {}", if broken.is_empty() { "none".into() } else { broken.join(", ") }));
    let multi: Vec<String> = runs.iter().filter(|(id, v)| v.0 > 1 && **id != 0).map(|(id, v)| format!("{id}×{}", v.0)).collect();
    c.lines.push(format!("  ids other than 0 with several records: {}", multi.join(" ")));
    let op_ids: std::collections::BTreeSet<i32> = rel.operators.iter().map(|o| o.def.id).collect();
    let mut attr_ids: BTreeMap<i32, Vec<String>> = BTreeMap::new();
    for a in rel.attributes.iter().filter(|a| a.id != 0) {
        attr_ids.entry(a.id).or_default().push(show(&a.name));
    }
    let both: Vec<String> = attr_ids.iter().filter(|(id, _)| op_ids.contains(id)).map(|(id, n)| format!("{id} {}", n.join("+"))).collect();
    let twice: Vec<String> = attr_ids.iter().filter(|(_, n)| n.len() > 1).map(|(id, n)| format!("{id} {}", n.join("+"))).collect();
    c.lines.push(format!("  attribute ids that are also an operator's id: {}", if both.is_empty() { "none".into() } else { both.join(", ") }));
    c.lines.push(format!("  ids other than 0 given to more than one attribute: {}", if twice.is_empty() { "none".into() } else { twice.join(", ") }));
    let mut linear_ok = true;
    for &id in &ids {
        let first = rel.relations.iter().position(|r| r.id == id).map_or(-1, |i| i as i32);
        if rel.rel_index_from_id(id) != first {
            linear_ok = false;
        }
    }
    if !linear_ok || rel.rel_index_from_id(i32::MAX) != -1 {
        c.failures += 1;
        c.lines.push("  DIFFERENT: the sort table does not give the first record of each id".into());
    }
    let unc: Vec<String> = ids.iter().filter(|&&id| rel.check_uncomplete_relation(id)).map(|id| format!("{id} {}", show(rel.rel_name(*id)))).collect();
    c.lines.push(format!("\"uncomplete\" relations (a `*` chapter in the first record): {}", unc.join(", ")));

    // the file's own list of chapters against the program's
    if let Some(e) = rel.enums.iter().find(|e| e.name == b"ChpType") {
        let off: Vec<String> = e
            .values
            .iter()
            .filter(|v| {
                let n = rel.chapters.get(v.id as usize).map(|c| c.to_ascii_uppercase());
                let want = v.name.to_ascii_uppercase();
                // the same word but for plural, underscores and abbreviation: compare loosely
                n.is_none_or(|n| {
                    let a: Vec<u8> = n.iter().copied().filter(|b| b.is_ascii_alphanumeric()).collect();
                    let b: Vec<u8> = want.iter().copied().filter(|b| b.is_ascii_alphanumeric()).collect();
                    !(a.starts_with(&b[..b.len().min(4)]) || b.starts_with(&a[..a.len().min(4)]))
                })
            })
            .map(|v| format!("{} {} (chapter {} is {})", v.id, show(&v.name), v.id, rel.chapter_name(v.id as i16)))
            .collect();
        c.lines.push(format!("ChpType values whose name does not look like the program's chapter of that number: {}", if off.is_empty() { "none".into() } else { off.join("; ") }));
    }
    c.relations = Some(rel);
    c
}

/// What the line readers made of the operator section of one custom effect file.
#[derive(Clone, Debug, Default)]
pub struct CusLines {
    /// `RES`, `NEWRES`, `ARG` and `OAT` lines met between `_OPERATOR` (or `OPERATOR`) and `_END`.
    pub lines: usize,
    /// Lines of which the reader took every token and nothing of the next line.
    pub exact: usize,
    /// The others, each with what happened.
    pub odd: Vec<String>,
    /// Lines by keyword and outcome.
    pub tally: BTreeMap<String, usize>,
}

/// A line-level check of the custom effect modes of the line readers, the only parts of this
/// module the relations file itself does not exercise. The section of a custom effect file
/// (`.cus`) between `_OPERATOR` and `_END` is the business of `SI_reldictReadOneOperator` in
/// a mode that is not ported; this check walks the lines itself and hands each `RES`,
/// `NEWRES`, `ARG` and `OAT` line to the ported reader of that kind of line, then looks
/// where the reader stopped. `rel` is the dictionary the standard file gave (conditions name
/// its attributes).
pub fn check_cus_lines(rel: &Relations, data: &[u8]) -> CusLines {
    let mut out = CusLines::default();
    let mut rd = Reader::new(data);
    rd.out = rel.clone();
    let mut in_section = false;
    let mut start = 0;
    while start < data.len() {
        let end = data[start..].iter().position(|&b| b == b'\n').map_or(data.len(), |n| start + n);
        let line = &data[start..end];
        let lead = line.iter().take_while(|b| b" \t\r".contains(b)).count();
        let word: Vec<u8> = line[lead..].iter().take_while(|b| !b" \t\r".contains(b)).map(|b| b.to_ascii_uppercase()).collect();
        match word.as_slice() {
            b"_OPERATOR" | b"OPERATOR" => in_section = true,
            b"_END" => in_section = false,
            b"RES" | b"NEWRES" | b"ARG" | b"OAT" if in_section => {
                out.lines += 1;
                rd.f.seek(start + lead + word.len());
                rd.info.messages.clear();
                let result = match word.as_slice() {
                    b"ARG" => rd.read_one_op_argument(1, &mut Vec::new()),
                    b"OAT" => rd.read_one_attribute(1, &mut Attribute::init()),
                    _ => rd.read_one_op_result(1, &mut Vec::new()),
                };
                let pos = rd.f.tell();
                let blank = |b: &[u8]| b.iter().all(|c| b" \t\r\n".contains(c));
                let outcome = match result {
                    Err(e) => format!("error: {e}"),
                    Ok(_) if !rd.info.messages.is_empty() => format!("message: {}", rd.info.messages[0].text),
                    Ok(false) => "the last token read ran into the end of the file".to_string(),
                    Ok(true) if pos > end && !blank(&data[end..pos]) => "took tokens of the next line".to_string(),
                    Ok(true) if pos <= end && !blank(&data[pos..end]) => {
                        let rest = show(&data[pos..end]);
                        if rest.trim_start().starts_with('#') { "a comment is left on the line".to_string() } else { "text is left on the line".to_string() }
                    }
                    Ok(true) => "exact".to_string(),
                };
                if outcome == "exact" {
                    out.exact += 1;
                } else {
                    out.odd.push(format!("{outcome}: {}", show(line).trim()));
                }
                *out.tally.entry(format!("{} {outcome}", show(&word))).or_insert(0) += 1;
            }
            _ => {}
        }
        start = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up file with every construct of the three sections.
    const SAMPLE: &str = "SREL4.0
A made-up title line
=====
ENUM_VALUES
ENUM Flavour
-----
cat id code name
ENM 00 SWT Sweet
ENM 01\tSRR\tSour
/*ENM 02 BTR Bitter*/
EndOfENUM
ENUM Outline // a remark
ENM 5 RND Round
EndOfENUM
EndOfENUM_VALUES
ELEMENTS_ATTRIBUTES
ELEMENT Thing
-----
cat id name mod scop type range group rnk # vis inc fcv ids fcurve names
EAT xx Taste R N ENM(Flavour) - PARAM 0 1 0 NON 900 tst \"What it tastes of\"
EAT 77 Size RWA B FLT [0,~10] SCAL 4 3 1 INS 1,2,3 sx,sy,sz \"Size; of
it\"
eat 78 Old - - - - PARAM 0 1 0 NON 5 old
EAT xx Plain RW N INT - ROTN 2 0 0 ZZZ
SUB 01 MODELS COND(Taste=SWT,srr)
SUB 02 Extras COND()
SUB 03 Loose WHEN(Taste=SWT)
EndOfELEMENT
SUBELEMENT MODELS OF Thing
EAT xx Sides R N INT - GEOM 0 1 0 NON 901 sides
SUB 01 Corner cond(Taste=XXX)
EndOfSUBELEMENT
SUBELEMENT Corner of some kind OF MODELS
EAT xx Angle RW N FLT - PARAM 0 INF 0 NON 902 ang
EndOfSUBELEMENT
SUBELEMENT Extras OF Thing
EAT 5 Many RW N INT [1,] PARAM 0 4 0 ALL 10,11,
   12,13 ma,mb,mc,
   md \"four of them\"
EndOfSUBELEMENT
EndOfELEMENTS_ATTRIBUTES
OPERATORS
=====
OPERATOR 300 Wobble NODE MULTI RENDERING INCR_SEND_INST // a remark
-----
cat id name chap group rnk mx vis pur pre unu
RES 00 - MODELS TRANSL 7
ARG 01 src * TRANSFO - INF 1 0 1 0
ARG 02 src SHAPES PARAM - 2 0 1 0 9 // more
OAT 301 Amount RA N FLT [~0,~100] TRANSFO 5 1 1 INS 7 amt //odd?
OPERATOR -5 Quiet TBD BRANCH
RES 00 - LIGHTS PARAM 0
ARG 01 - ANIMATION PARAM - 1 0 1 0 0

EndOfOPERATORS
";

    /// The least a file must have before its next section is seen: each section reader starts
    /// by running to its section word and its first block, to the end of the file if need be.
    const ENUMS: &str = "SREL4.0 ENUM_VALUES ENUM One ENM 0 A a EndOfENUM EndOfENUM_VALUES\n";
    const ELEMS: &str = "ELEMENTS_ATTRIBUTES ELEMENT Base EAT 1 Zero R N INT - PARAM 0 0 0 NON EndOfELEMENT EndOfELEMENTS_ATTRIBUTES\n";

    fn reader(text: &str) -> Reader<'_> {
        Reader::new(text.as_bytes())
    }

    fn v(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn words() {
        for (w, g) in [("PARAM", 0), ("TRANSFO", 1), ("SCAL", 1), ("ROTN", 1), ("TRANSL", 1), ("GEOM", 2), ("RENDER", 3), ("CUSTOM", 4), ("OTHER", 5),
                       ("RELGR_PARAM", 0), ("RELGR_TRANSFO", 1), ("RELGR_GEOM", 2), ("RELGR_RENDER", 3), ("RELGR_CUSTOM", 4), ("RELGR_OTHER", 5),
                       ("param", -1), ("", -1), ("-", -1)] {
            assert_eq!(convert_group_token(w.as_bytes()), g, "{w}");
        }
        for (w, s) in [("N", 0), ("NODE", 0), ("B", 1), ("BRANCH", 1), ("TREE", 2), ("T", -1), ("node", -1)] {
            assert_eq!(convert_scope_token(w.as_bytes()), s, "{w}");
        }
        assert_eq!((convert_application_token(b"MULTI"), convert_application_token(b"SINGLE"), convert_application_token(b"x")), (1, 0, -1));
        for (w, c) in [("NO_OP", 0), ("CV_BASED", 1), ("GENERATOR", 2), ("CREATOR", 3), ("SPATIAL_DEF", 4), ("SUBSTITUTION", 5), ("BIDIRECTIONAL", 6), ("x", -1)] {
            assert_eq!(convert_op_categ_token(w.as_bytes()), c, "{w}");
        }
        assert_eq!(convert_render_token(b"RENDERING"), (1, 1));
        assert_eq!(convert_render_token(b"RENDER"), (0, -1));
        for (w, i) in [("INCR_SEND_NONE", 0), ("INCR_SEND_INST", 1), ("INCR_SEND_ALL", 3), ("INCR_SEND_SHDTRE", 4), ("INCR_SEND_WITH_SHDTRE", 7), ("INCR_SEND_ALL//", -1)] {
            assert_eq!(convert_incmode_token(w.as_bytes()), i, "{w}");
        }
        for (w, i) in [("NON", 0), ("INS", 1), ("ALL", 3), ("SHD", 4), ("XYZ", 3), ("", 3)] {
            assert_eq!(convert_attrib_incmode_token(w.as_bytes()), i, "{w}");
        }
        for (w, a) in [("R", 1), ("RW", 3), ("RWA", 7), ("RA", 5), ("A", 4), ("-", 0), ("rwa", 0), ("WARM", 7)] {
            assert_eq!(convert_access_mode_token(w.as_bytes()), a, "{w}");
        }
        for (w, f) in [("-", 1), ("INF", 0x7fff), ("12", 12), ("x", 0), ("70000", 4464)] {
            assert_eq!(cus_convert_fanout_token(w.as_bytes()), f, "{w}");
        }
    }

    #[test]
    fn strtok_like_c() {
        assert_eq!(strtok_pieces(b"COND(Taste=SWT,,SRR)", b"(=,)"), vec![&b"COND"[..], b"Taste", b"SWT", b"SRR"]);
        assert_eq!(strtok_pieces(b",,", b","), Vec::<&[u8]>::new());
        let mut t = v("ENM(Flavour)");
        after_strtok(&mut t, b"(=,)");
        assert_eq!(t, b"ENM");
        let mut t = v(",,7,8");
        after_strtok(&mut t, b",");
        assert_eq!(t, b",,7");
        let mut t = v("plain");
        after_strtok(&mut t, b",");
        assert_eq!(t, b"plain");
    }

    #[test]
    fn x87_sum_rounds_to_64_bits_first() {
        // 1 + 2^-24 + 2^-60: in doubles the last term is lost and the tie goes to even (1.0);
        // with 64 bits it survives and the sum is above the tie
        let b = 2f64.powi(-24) + 2f64.powi(-60);
        assert_eq!((1.0f64 + b) as f32, 1.0);
        assert_eq!(x87_add_store_single(1.0, b), 1.0 + f32::EPSILON);
        assert_eq!(x87_add_store_single(-1.0, -b), -(1.0 + f32::EPSILON));
        // an exact tie stays a tie
        assert_eq!(x87_add_store_single(1.0, 2f64.powi(-24)), 1.0);
        // far apart: the small operand only tips the rounding
        assert_eq!(x87_add_store_single(1.0, 1e-300), 1.0);
        assert_eq!(x87_add_store_single(1.0, -1e-300), 1.0);
        assert_eq!(x87_add_store_single(0.0, 0.0001), 0.0001f32);
        assert_eq!(x87_add_store_single(2.5, -2.5), 0.0);
        assert_eq!(x87_add_store_single(1.0, -0.0001), 0.9999f32);
        assert_eq!(x87_add_store_single(f32::MAX, f64::MAX), f32::INFINITY);
        assert_eq!(x87_add_store_single(1e-45, 1e-46), 1e-45);
    }

    fn range(text: &str, kind: i32) -> (bool, Attribute, Vec<u8>) {
        let mut a = Attribute::init();
        a.kind = kind;
        let mut t = v(text);
        let ok = convert_range_token(&mut t, &mut a);
        (ok, a, t)
    }

    #[test]
    fn ranges() {
        let (ok, a, t) = range("[0,1]", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_kind, a.min_inclusive, a.max, a.max_kind, a.max_inclusive), (0.0, 1, 1, 1.0, 1, 1));
        assert_eq!(t, b"[0"); // the token buffer is cut at the comma
        // an excluded bound of a real moves in by 0.0001
        let (ok, a, _) = range("(0,1)", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_inclusive, a.max, a.max_inclusive), (0.0001, 0, 0.9999, 0));
        // of an integer, by one
        let (_, a, _) = range("(2,9)", TYPE_INT);
        assert_eq!((a.min, a.max), (3.0, 8.0));
        let (_, a, _) = range("(2,9]", 8);
        assert_eq!((a.min, a.max, a.max_inclusive), (3.0, 9.0, 1));
        // of anything else, not at all
        let (_, a, _) = range("(2,9)", TYPE_STR);
        assert_eq!((a.min, a.min_inclusive, a.max, a.max_inclusive), (2.0, 0, 9.0, 0));
        // soft bounds
        let (ok, a, _) = range("[~5,~50]", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_kind, a.max, a.max_kind), (5.0, 2, 50.0, 2));
        // open sides
        let (ok, a, _) = range("[3,]", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_kind, a.max, a.max_kind), (3.0, 1, 0.0, 0));
        let (ok, a, _) = range("[,7]", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_kind, a.max, a.max_kind), (0.0, 0, 7.0, 1));
        // a lone ~ is "no bound" below and a fault above
        let (ok, a, _) = range("(~,7]", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_kind, a.min_inclusive), (0.0001, 0, 0));
        let (ok, a, _) = range("[1,~]", TYPE_FLT);
        assert!(!ok);
        assert_eq!((a.min, a.max_kind), (1.0, 2)); // what was written before the fault stays
        // not ranges
        assert!(!range("nocomma", TYPE_FLT).0);
        assert!(!range("1,2]", TYPE_FLT).0);
        assert!(!range("[1,2", TYPE_FLT).0);
        let (ok, a, _) = range("[1,)", TYPE_FLT);
        assert!(ok);
        assert_eq!(a.max_kind, 0);
        // text that is no number reads as 0
        let (ok, a, _) = range("[a,b]", TYPE_FLT);
        assert!(ok);
        assert_eq!((a.min, a.min_kind, a.max, a.max_kind), (0.0, 1, 0.0, 1));
    }

    #[test]
    fn description_loses_the_bytes_that_find_the_buffer_full() {
        let text: String = (0..130).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let file = format!("  \t\n\"{text}\" next\n");
        let mut rd = reader(&file);
        let (d, cut) = rd.read_description();
        let want: String = text.chars().enumerate().filter(|(i, _)| ![80, 101, 122].contains(i)).map(|(_, c)| c).collect();
        assert_eq!(d, Some(v(&want)));
        assert!(cut);
        assert!(rd.tread());
        assert_eq!(rd.tok, b"next");
        // exactly 80 bytes fit
        let file = format!("\"{}\"", &text[..80]);
        let mut rd = reader(&file);
        assert_eq!(rd.read_description(), (Some(v(&text[..80])), false));
        // no quote: the byte is put back
        let mut rd = reader("   word");
        assert_eq!(rd.read_description(), (None, false));
        assert!(!rd.tread()); // the last token of the file
        assert_eq!(rd.tok, b"word");
        // the end of the file ends a description; an empty one is empty, not absent
        let mut rd = reader("\"half");
        assert_eq!(rd.read_description(), (Some(v("half")), false));
        let mut rd = reader("\"\"");
        assert_eq!(rd.read_description(), (Some(v("")), false));
        let mut rd = reader("  ");
        assert_eq!(rd.read_description(), (None, false));
    }

    #[test]
    fn sample_file() {
        let (rel, info) = read(SAMPLE.as_bytes()).unwrap();
        assert_eq!(info.tag, Some(u32::from_be_bytes(*b"SREL")));
        assert_eq!(info.version, 4.0);
        assert_eq!(info.layout, Layout::Sections);
        assert_eq!(info.result, 1);
        assert!(info.all_tokens_consumed);
        assert_eq!(info.skipped.get(&v("eat")), Some(&1));
        assert_eq!(info.skipped.get(&v("/*ENM")), Some(&1));
        assert!(info.descriptions_cut.is_empty() && info.fcurve_overruns.is_empty());
        assert_eq!(info.messages.len(), 1);
        assert_eq!(info.messages[0].channel, Channel::Stderr);
        assert_eq!(info.messages[0].text, "Invalid Condition Token [WHEN(Taste=SWT)]");

        // enumerations: the commented value is passed over
        assert_eq!(rel.enums.len(), 2);
        assert_eq!(rel.enums[0].name, b"Flavour");
        assert_eq!(rel.enums[0].values, vec![EnumValue { id: 0, code: v("SWT"), name: v("Sweet") }, EnumValue { id: 1, code: v("SRR"), name: v("Sour") }]);
        assert_eq!(rel.enums[1], EnumType { name: v("Outline"), values: vec![EnumValue { id: 5, code: v("RND"), name: v("Round") }] });

        // attributes
        let names: Vec<&[u8]> = rel.attributes.iter().map(|a| a.name.as_slice()).collect();
        assert_eq!(names, [&b"Taste"[..], b"Size", b"Plain", b"Sides", b"Angle", b"Many", b"Amount"]);
        let taste = &rel.attributes[0];
        assert_eq!((taste.owner_kind, taste.owner_id, taste.id, taste.access, taste.scope, taste.kind, taste.enum_type), (0, -1, 0, 1, 0, TYPE_ENM, 0));
        assert_eq!((taste.min_kind, taste.max_kind, taste.group, taste.rank, taste.count, taste.fcurves, taste.dope_display, taste.incmode), (0, 0, 0, 0, 1, 1, 0, 0));
        assert_eq!((taste.fcurve_ids.clone(), taste.fcurve_names.clone()), (vec![900], vec![Some(v("tst"))]));
        assert_eq!(taste.description, Some(v("What it tastes of")));
        let size = &rel.attributes[1];
        assert_eq!((size.id, size.access, size.scope, size.kind, size.group, size.rank, size.count, size.fcurves, size.dope_display, size.incmode), (77, 7, 1, TYPE_FLT, 1, 4, 3, 3, 1, 1));
        assert_eq!((size.min, size.min_kind, size.min_inclusive, size.max, size.max_kind, size.max_inclusive), (0.0, 1, 1, 10.0, 2, 1));
        assert_eq!(size.fcurve_ids, [1, 2, 3]);
        assert_eq!(size.description, Some(v("Size; of\nit"))); // character by character, `;` and line end included
        assert_eq!(size.enum_type, 0); // not written for a type that is no enumeration
        // a count of 0: no fcurve lists are read, and the unknown increment word counts as ALL;
        // the bounds of the line before stay, with "no bound" kinds
        let plain = &rel.attributes[2];
        assert_eq!((plain.kind, plain.group, plain.rank, plain.count, plain.fcurves, plain.incmode), (TYPE_INT, 1, 2, 0, 0, 3));
        assert!(plain.fcurve_ids.is_empty() && plain.fcurve_names.is_empty());
        assert_eq!((plain.min, plain.min_kind, plain.max, plain.max_kind), (0.0, 0, 10.0, 0));
        assert_eq!(plain.description, None);
        // the block MODELS is a chapter; Corner is not, its parent is
        assert_eq!(rel.attributes[3].owner_id, 8);
        let angle = &rel.attributes[4];
        assert_eq!((angle.owner_id, angle.count, angle.fcurves, angle.fcurve_ids.clone()), (8, 0x7fff, 1, vec![902]));
        // lists over several tokens
        let many = &rel.attributes[5];
        assert_eq!((many.owner_id, many.id, many.count, many.fcurves), (-1, 5, 4, 4));
        assert_eq!(many.fcurve_ids, [10, 11, 12, 13]);
        assert_eq!(many.fcurve_names, [Some(v("ma")), Some(v("mb")), Some(v("mc")), Some(v("md"))]);
        assert_eq!((many.min, many.min_kind, many.max_kind), (1.0, 1, 0));
        assert_eq!(many.description, Some(v("four of them")));
        let amount = &rel.attributes[6];
        assert_eq!((amount.owner_kind, amount.owner_id, amount.id, amount.access, amount.group, amount.rank), (1, 300, 301, 5, 1, 5));
        assert_eq!((amount.min, amount.min_kind, amount.max, amount.max_kind), (0.0, 2, 100.0, 2));
        assert_eq!(amount.description, None);

        // groups
        assert_eq!(rel.groups.len(), 2);
        let thing = &rel.groups[0];
        assert_eq!((thing.name.as_slice(), thing.rel_type, thing.attributes.clone()), (&b"Thing"[..], 0, vec![0, 1, 2]));
        let subs: Vec<&[u8]> = thing.subgroups.iter().map(|g| g.name.as_slice()).collect();
        assert_eq!(subs, [&b"MODELS"[..], b"Extras", b"Loose"]);
        assert_eq!(thing.subgroups[0].cond, Some(AttribCond { attribute: 0, values: vec![0, 1] }));
        assert_eq!(thing.subgroups[1].cond, None);
        assert_eq!(thing.subgroups[2].cond, None);
        assert_eq!(thing.subgroups[0].attributes, [3]);
        assert_eq!(thing.subgroups[0].subgroups[0].name, b"Corner");
        assert_eq!(thing.subgroups[0].subgroups[0].cond, Some(AttribCond { attribute: 0, values: vec![-1] }));
        assert_eq!(thing.subgroups[0].subgroups[0].attributes, [4]);
        assert_eq!(thing.subgroups[1].attributes, [5]);
        assert_eq!((rel.groups[1].name.as_slice(), rel.groups[1].rel_type, rel.groups[1].attributes.clone()), (&b"Wobble"[..], 1, vec![6]));
        assert_eq!(rel.group_by_name(b"Corner"), Some(vec![0, 0, 0]));
        assert_eq!(rel.group_by_name(b"Nobody"), None);

        // blocks
        let blocks: Vec<(&[u8], bool, i16, usize, usize)> = rel.blocks.iter().map(|b| (b.name.as_slice(), b.sub, b.chapter, b.attributes.len(), b.subs.len())).collect();
        assert_eq!(blocks, [(&b"Thing"[..], false, -1, 3, 3), (b"MODELS", true, 8, 1, 1), (b"Corner", true, 8, 1, 0), (b"Extras", true, -1, 1, 0)]);
        assert_eq!(rel.blocks[2].parent, Some(v("MODELS")));
        assert_eq!(rel.blocks[2].group, Some(vec![0, 0, 0]));

        // operators
        assert_eq!(rel.operators.len(), 2);
        let w = &rel.operators[0];
        assert_eq!(w.def, OperDef { id: 300, name: v("Wobble"), scope: 0, application: 1, category: 0, render: 1, incmode: 1 });
        assert_eq!(w.results, [OperResult { section: OperSection { id: 0, name: v("-"), chapter: 8, group: 1, rank: 7 }, cus: None, new_result: 0 }]);
        assert_eq!(w.arguments[0], OperArgument { section: OperSection { id: 1, name: v("src"), chapter: CHAPTER_ANY, group: 1, rank: 0 }, max: 0x7fff, dope_display: 1, pur: 0, pre: 1, cus: None });
        assert_eq!(w.arguments[1], OperArgument { section: OperSection { id: 2, name: v("src"), chapter: 15, group: 0, rank: 0 }, max: 2, dope_display: 0, pur: 1, pre: 0, cus: None });
        assert_eq!((w.attributes.clone(), w.relations.clone()), (vec![6], vec![6, 7, 8]));
        // the first word that is no flag ends the flags: BRANCH is never seen
        let q = &rel.operators[1];
        assert_eq!(q.def, OperDef { id: -5, name: v("Quiet"), scope: 0, application: 0, category: 0, render: 0, incmode: 3 });
        assert_eq!((q.results[0].section.chapter, q.arguments[0].section.chapter), (6, 1));

        // relation records: one per EAT, ARG and OAT, in file order
        let ids: Vec<i32> = rel.relations.iter().map(|r| r.id).collect();
        assert_eq!(ids, [0, 77, 0, 0, 0, 5, 300, 300, 301, -5]);
        assert_eq!(rel.relations[1], Relation { id: 77, name: v("Size"), src_chapter: -1, dst_chapter: 1, src_group: 1, dst_group: 0, rank: 4, fanout: 3, dope_display: 1, upd: [0, 0], render: 0, incmode: 1 });
        assert_eq!(rel.relations[4].fanout, 0x7fff);
        assert_eq!(rel.relations[6], Relation { id: 300, name: v("Wobble"), src_chapter: 8, dst_chapter: CHAPTER_ANY, src_group: 1, dst_group: 1, rank: 7, fanout: 0x7fff, dope_display: 1, upd: [0, 1], render: 1, incmode: 1 });
        assert_eq!(rel.relations[7], Relation { id: 300, name: v("Wobble"), src_chapter: 8, dst_chapter: 15, src_group: 1, dst_group: 0, rank: 7, fanout: 2, dope_display: 0, upd: [1, 0], render: 1, incmode: 1 });
        // the attribute's relation starts at the chapter of the latest ARG because that is SHAPES
        assert_eq!(rel.relations[8], Relation { id: 301, name: v("Amount"), src_chapter: 15, dst_chapter: 1, src_group: 1, dst_group: 0, rank: 5, fanout: 1, dope_display: 1, upd: [0, 0], render: 0, incmode: 1 });
        assert_eq!(rel.relations[9].src_chapter, 6);
        assert_eq!(rel.chapters.len(), 36);

        // the look-ups of the scene description reader
        assert_eq!(rel.rel_index_from_id(0), 0);
        assert_eq!(rel.rel_index_from_id(300), 6);
        assert_eq!(rel.rel_index_from_id(-5), 9);
        assert_eq!(rel.rel_index_from_id(4), -1);
        for id in [0, 5, 77, 300, 301, -5, 1, 1000, -1000] {
            assert_eq!(rel.rel_index_from_id(id), rel.relations.iter().position(|r| r.id == id).map_or(-1, |i| i as i32), "{id}");
        }
        assert_eq!(rel.rel_name(300), b"Wobble");
        assert_eq!(rel.rel_name(4), b"unknown");
        assert_eq!(rel.rel_sub(300, 8, -10, 1, 1), (1, Some(0x7fff)));
        assert_eq!(rel.rel_sub(300, 8, 15, 1, 0), (2, Some(2)));
        assert_eq!(rel.rel_sub(300, 8, 3, 1, 0), (1, None));
        assert_eq!(rel.rel_sub(4, 8, 15, 1, 0), (1, None));
        // only the unbroken run from the first record counts: the later records of id 0 are out of reach
        assert_eq!(rel.rel_sub(0, 8, 1, 2, 0), (1, None));
        assert_eq!((rel.rel_nb_rules(300), rel.rel_nb_rules(0), rel.rel_nb_rules(4)), (2, 1, 0));
        assert_eq!(rel.rel_attributes(300, 2), Some(&rel.relations[7]));
        assert_eq!(rel.rel_attributes(300, 0), Some(&rel.relations[6]));
        assert_eq!(rel.rel_attributes(300, 3).map(|r| r.id), Some(301)); // nothing checks the id
        assert_eq!(rel.rel_attributes(-5, 2), None);
        assert_eq!(rel.rel_attributes(4, 1), None);
        assert!(rel.check_uncomplete_relation(300));
        assert!(!rel.check_uncomplete_relation(301) && !rel.check_uncomplete_relation(4));
        assert_eq!((rel.rel_id_by_name(b"Amount"), rel.rel_id_by_name(b"amount")), (301, -1));
        assert_eq!((rel.rel_chap_grp(300), rel.rel_chap_grp(4)), ((8, 1), (-1, 0)));
        assert_eq!((rel.rel_rank(300), rel.rel_dope_disp(300), rel.rel_render_flag(300), rel.rel_incmode_flag(300), rel.rel_incmode_flag(4)), (7, 1, 1, 1, -1));
        assert_eq!((rel.rel_updlattr(300), rel.rel_updlattr(4)), (Some((0, 1)), None));

        // the check agrees with itself on this file
        let c = check(SAMPLE.as_bytes());
        assert_eq!(c.failures, 0, "{:#?}", c.lines);
        assert_eq!(operator_lines(&rel).len(), 2);
        assert_eq!(attribute_lines(&rel).len(), 7);
    }

    #[test]
    fn columns_a_line_lacks_keep_the_previous_line_of_the_block() {
        let text = "SREL4.0 ENUM_VALUES ENUM One ENM 0 A a EndOfENUM ENUM Two ENM 0 B b EndOfENUM EndOfENUM_VALUES
ELEMENTS_ATTRIBUTES ELEMENT Thing
EAT 1 First R N ENM(Two) (2,5] PARAM 0 1 0 NON 1 a \"text\"
EAT 2 Second R N INT - PARAM 0 1 0 NON 2 b
EndOfELEMENT ELEMENT Other
EAT 3 Third R N INT - PARAM 0 1 0 NON 3 c
EndOfELEMENT EndOfELEMENTS_ATTRIBUTES OPERATORS EndOfOPERATORS\n";
        let (rel, info) = read(text.as_bytes()).unwrap();
        let (a, b, c) = (&rel.attributes[0], &rel.attributes[1], &rel.attributes[2]);
        assert_eq!((a.enum_type, a.min, a.min_inclusive, a.max, a.min_kind, a.max_kind), (1, 2.0, 0, 5.0, 1, 1));
        assert_eq!((b.kind, b.enum_type, b.min, b.min_inclusive, b.max, b.min_kind, b.max_kind), (TYPE_INT, 1, 2.0, 0, 5.0, 0, 0));
        assert_eq!(b.description, None);
        // a new block starts from a fresh record
        assert_eq!((c.enum_type, c.min, c.min_inclusive, c.max), (0, 0.0, 1, 0.0));
        // no operator at all: the section reader's result is that of not finding one
        assert_eq!(info.result, 0);
        assert!(rel.operators.is_empty());
    }

    #[test]
    fn enumerations_odd_cases() {
        // an enumeration without values takes the next one's, and that one's name is lost
        let (rel, _) = read(b"SREL4.0 ENUM_VALUES ENUM Empty EndOfENUM ENUM Full ENM 1 A a ENM 2 B b EndOfENUM EndOfENUM_VALUES\n").unwrap();
        assert_eq!(rel.enums.len(), 1);
        assert_eq!((rel.enums[0].name.as_slice(), rel.enums[0].values.len()), (&b"Empty"[..], 2));
        // the file ends inside a value: what was read is kept, empty strings for the rest
        let (rel, info) = read(b"SREL4.0 ENUM_VALUES ENUM Cut ENM 1 A a ENM 2").unwrap();
        assert_eq!(rel.enums[0].values, vec![EnumValue { id: 1, code: v("A"), name: v("a") }, EnumValue { id: 2, code: v(""), name: v("") }]);
        assert_eq!(info.result, 0);
        // no section at all
        let (rel, info) = read(b"SREL4.0\nnothing here\n").unwrap();
        assert!(rel.enums.is_empty() && rel.relations.is_empty());
        assert_eq!((info.result, info.all_tokens_consumed), (0, true));
    }

    #[test]
    fn inputs_the_original_does_not_survive() {
        let head = "SREL4.0 ENUM_VALUES ENUM One ENM 0 A a EndOfENUM EndOfENUM_VALUES ELEMENTS_ATTRIBUTES ELEMENT Thing\n";
        // the file ends inside the fcurve ids, or the names
        let text = format!("{head}EAT 5 Many RW N INT - PARAM 0 4 0 ALL 10,11");
        assert!(matches!(read(text.as_bytes()), Err(Error::EndlessLoop { what: "the fcurve ids of an attribute", .. })));
        let text = format!("{head}EAT 5 Many RW N INT - PARAM 0 2 0 ALL 10,11 a,\n");
        assert!(matches!(read(text.as_bytes()), Err(Error::EndlessLoop { what: "the fcurve names of an attribute", .. })));
        // a sub-element no SUB line declared
        let text = format!("{head}EAT 1 One R N INT - PARAM 0 0 0 NON\nEndOfELEMENT SUBELEMENT Ghost OF Thing EAT 2 Two R N INT - PARAM 0 0 0 NON\n");
        assert!(matches!(read(text.as_bytes()), Err(Error::BadPointer { .. })));
        // a condition with codes on an attribute that is not there
        let text = format!("{head}EAT 1 One R N INT - PARAM 0 0 0 NON\nSUB 1 Part COND(Nothing=A)\n");
        assert!(matches!(read(text.as_bytes()), Err(Error::BadPointer { .. })));
        // …but without codes it is only an unknown attribute
        let text = format!("{head}EAT 1 One R N INT - PARAM 0 0 0 NON\nSUB 1 Part COND(Nothing)\nEndOfELEMENT EndOfELEMENTS_ATTRIBUTES\n");
        let (rel, _) = read(text.as_bytes()).unwrap();
        assert_eq!(rel.groups[0].subgroups[0].cond, Some(AttribCond { attribute: -1, values: vec![] }));
        // ARG before RES, OAT before ARG
        let ops = format!("{ENUMS}{ELEMS}OPERATORS OPERATOR 9 Odd NODE\n");
        assert!(matches!(read(format!("{ops}ARG 1 a MODELS PARAM - 1 0 0 0 0\n").as_bytes()), Err(Error::BadPointer { .. })));
        assert!(matches!(read(format!("{ops}RES 0 - MODELS PARAM 0\nOAT 9 x R N FLT - PARAM 0 0 0 NON\n").as_bytes()), Err(Error::BadPointer { .. })));
        assert!(matches!(read(format!("{ops}OAT 9 x R N FLT - PARAM 0 0 0 NON\n").as_bytes()), Err(Error::BadPointer { .. })));
        assert!(!format!("{}", read(format!("{ops}OAT 9 x R N FLT - PARAM 0 0 0 NON\n").as_bytes()).unwrap_err()).is_empty());
    }

    #[test]
    fn truncated_files_end_quietly() {
        si_rt::skip_without_install!();
        // an attribute whose last token runs into the end of the file: "Error reading attribute"
        let text = format!("{ENUMS}ELEMENTS_ATTRIBUTES ELEMENT Thing EAT 1 One R N INT - PARAM 0 1 0 NON 4 a");
        let (rel, info) = read(text.as_bytes()).unwrap();
        assert!(rel.attributes.is_empty());
        assert_eq!(info.messages[0].channel, Channel::SiError(4));
        assert_eq!(info.result, 0);
        // inside an ARG line: the operator fails, the relation is not added
        let text = format!("{ENUMS}{ELEMS}OPERATORS OPERATOR 9 Odd NODE RES 0 - MODELS PARAM 0 ARG 1 a MODELS PARAM - 1 0");
        let (rel, info) = read(text.as_bytes()).unwrap();
        assert_eq!((rel.operators.len(), rel.operators[0].arguments.len(), rel.relations.len()), (1, 1, 1)); // the element's only
        assert_eq!(info.messages[0].text, si_rt::text(0x0868_4ee0));
        assert_eq!(info.result, 0);
        // inside a RES line
        let (rel, info) = read(format!("{ENUMS}{ELEMS}OPERATORS OPERATOR 9 Odd NODE RES 0 - MODELS").as_bytes()).unwrap();
        assert_eq!(rel.operators[0].results[0].section.chapter, 8);
        assert_eq!(info.messages[0].text, si_rt::text(0x0868_4e00));
        // between lines
        let (rel, info) = read(format!("{ENUMS}{ELEMS}OPERATORS OPERATOR 9 Odd NODE\nRES 0 - MODELS PARAM 0\nARG 1 a MODELS PARAM - 1 0 0 0 0\n").as_bytes()).unwrap();
        assert_eq!((rel.relations.len(), info.result, info.messages.len()), (2, 0, 0));
        // a chapter name the program does not know becomes a chapter
        let (rel, _) = read(format!("{ENUMS}{ELEMS}OPERATORS OPERATOR 9 Odd RES 0 - Widgets BOGUS 0 ARG 1 a pictures PARAM - 1 0 0 0 0 EndOfOPERATORS\n").as_bytes()).unwrap();
        assert_eq!(rel.chapters.len(), 37);
        assert_eq!(rel.chapters[36], b"Widgets");
        assert_eq!(rel.relations[1].src_chapter, 36);
        assert_eq!(rel.relations[1].src_group, -1); // an unknown group word is not an error here
        assert_eq!(rel.relations[1].dst_chapter, 4); // PICTURES is found in any letter case
        assert_eq!((rel.find_chapter(b"Render_Pictures"), rel.find_chapter(b"models"), rel.find_chapter(b"MODELS")), (11, -1, 8));
    }

    #[test]
    fn operator_attributes_go_to_the_first_group_of_that_name() {
        let text = format!("{ENUMS}ELEMENTS_ATTRIBUTES ELEMENT Twin EAT 1 One R N INT - PARAM 0 0 0 NON EndOfELEMENT EndOfELEMENTS_ATTRIBUTES
OPERATORS OPERATOR 9 Twin RES 0 - MODELS PARAM 0 ARG 1 a CLUSTERS PARAM - 1 0 0 0 0 OAT 10 Two R N FLT - GEOM 3 0 0 NON EndOfOPERATORS\n");
        let (rel, info) = read(text.as_bytes()).unwrap();
        assert_eq!(info.result, 1);
        assert_eq!(rel.groups.len(), 2);
        assert_eq!((rel.groups[0].attributes.clone(), rel.groups[0].rel_type), (vec![0, 1], 0));
        assert_eq!((rel.groups[1].attributes.len(), rel.groups[1].rel_type), (0, 1));
        assert_eq!(rel.relations[2].src_chapter, CHAPTER_CLUSTERS);
        assert!(check(text.as_bytes()).lines.iter().any(|l| l.contains("another group of the same name")));
    }

    #[test]
    fn table_layout() {
        // under the SREL tag: twelve columns
        let text = "SREL3.2\nRELATIONS\n12 Glue MODELS * TRANSFO GEOM 3 1 0 1 0 99\n13 Tape Widgets LIGHTS PARAM BOGUS 1 2 1 0 5 0\nEndOfRELATIONS\nignored\n";
        let (rel, info) = read(text.as_bytes()).unwrap();
        assert_eq!((info.layout, info.version, info.result, info.all_tokens_consumed), (Layout::Table, 3.2, 0, false));
        assert_eq!(rel.relations[0], Relation { id: 12, name: v("Glue"), src_chapter: 8, dst_chapter: CHAPTER_ANY, src_group: 1, dst_group: 2, rank: 3, fanout: 1, dope_display: 0, upd: [1, 0], render: 0, incmode: 3 });
        assert_eq!(rel.relations[1], Relation { id: 13, name: v("Tape"), src_chapter: 36, dst_chapter: 6, src_group: 0, dst_group: -1, rank: 1, fanout: 2, dope_display: 1, upd: [0, 5], render: 0, incmode: 3 });
        assert_eq!(info.messages.len(), 1);
        assert_eq!(info.messages[0].channel, Channel::SiError(0x1f));
        // under another tag, or without a header: nine
        for text in ["SSET2.0\nRELATIONS\n12 Glue MODELS * TRANSFO GEOM 3 1 0\n14 Clip CAMERAS LIGHTS PARAM PARAM 0 0 0\nEndOfRELATIONS", "RELATIONS 12 Glue MODELS * TRANSFO GEOM 3 1 0 14 Clip CAMERAS LIGHTS PARAM PARAM 0 0 0"] {
            let (rel, info) = read(text.as_bytes()).unwrap();
            assert_eq!(info.layout, Layout::Table);
            assert_eq!(rel.relations.len(), 2, "{text}");
            assert_eq!((rel.relations[1].id, rel.relations[1].src_chapter, rel.relations[1].upd), (14, 2, [0, 0]));
        }
        let (_, info) = read(b"RELATIONS EndOfRELATIONS\n").unwrap();
        assert_eq!((info.tag, info.version), (None, -1.0));
        // the version decides, not the tag
        let (rel, info) = read(b"SDSC3.49 ENUM_VALUES ENUM One ENM 0 A a EndOfENUM EndOfENUM_VALUES\n").unwrap();
        assert_eq!((info.layout, rel.enums.len()), (Layout::Sections, 1));
        let (_, info) = read(b"SREL3.48 ENUM_VALUES ENUM One ENM 0 A a EndOfENUM EndOfENUM_VALUES\n").unwrap();
        assert_eq!(info.layout, Layout::Table);
        assert!(read(b"").is_ok());
    }

    /// A reader with one enumeration and one attribute of it, for conditions.
    fn with_taste(text: &str) -> Reader<'_> {
        let mut rd = reader(text);
        rd.out.enums.push(EnumType { name: v("Flavour"), values: vec![EnumValue { id: 3, code: v("SWT"), name: v("Sweet") }] });
        let mut a = Attribute::init();
        a.name = v("Taste");
        a.id = 40;
        rd.out.attributes.push(a);
        rd
    }

    #[test]
    fn conditions() {
        let mut rd = with_taste("");
        assert_eq!(rd.convert_condition_token(b"COND(taste=swt,nope)", None).unwrap(), (Some(AttribCond { attribute: 0, values: vec![3, -1] }), true));
        assert_eq!(rd.convert_condition_token(b"Cond()", None).unwrap(), (None, true));
        assert_eq!(rd.convert_condition_token(b"-", None).unwrap(), (None, false));
        assert_eq!(rd.convert_condition_token(b"", None).unwrap(), (None, false));
        assert_eq!(rd.convert_condition_token(b"(=,)", None).unwrap(), (None, false));
        // the other mode: only codes, and the attribute's id
        let a = rd.out.attributes[0].clone();
        assert_eq!(rd.convert_condition_token(b"SWT,XXX", Some(&a)).unwrap(), (Some(AttribCond { attribute: 40, values: vec![3, -1] }), false));
    }

    #[test]
    fn custom_effect_lines() {
        si_rt::skip_without_install!();
        // shared columns: no id, words upper-cased, rank 50
        let mut rd = reader("probe models Geom rest");
        let mut sec = OperSection::default();
        assert!(rd.read_one_section(1, &mut sec));
        assert_eq!(sec, OperSection { id: 0, name: v("probe"), chapter: 8, group: 2, rank: 50 });
        let mut rd = reader("probe models bogus rest");
        assert!(!rd.read_one_section(1, &mut sec));
        assert_eq!(rd.info.messages.len(), 2);

        // an argument with everything
        let mut rd = with_taste("probe MODELS GEOM 1 node COND(Taste=SWT) \"Pick the  base\n thing\" max inf\nNEXT");
        let mut args = Vec::new();
        assert!(rd.read_one_op_argument(1, &mut args).unwrap());
        assert_eq!((args[0].max, args[0].dope_display, args[0].pur, args[0].pre), (0x7fff, 0, 0, 1));
        assert_eq!(args[0].cus, Some(CusSection { scope: 0, cond: Some(AttribCond { attribute: 0, values: vec![3] }), pick_msg: Some(v("Pick the base thing")), fanout: 0x7fff }));
        rd.tread();
        assert_eq!(rd.tok, b"NEXT");

        // `-` for no condition and no message; what follows is not MAX and is left unread
        let mut rd = reader("probe MODELS TRANSFO 2 TREE - - NEXT line");
        let mut args = Vec::new();
        assert!(rd.read_one_op_argument(1, &mut args).unwrap());
        assert_eq!(args[0].cus, Some(CusSection { scope: 2, cond: None, pick_msg: None, fanout: 1 }));
        assert_eq!(args[0].pre, 2);
        assert_eq!(rd.tok, b"NEXT"); // the buffer keeps the word it looked at
        assert!(rd.tread());
        assert_eq!(rd.tok, b"NEXT");

        // anything else where the message would start is put back and nothing more is read
        let mut rd = reader("probe MODELS TRANSFO 1 NODE - xyz MAX 3");
        let mut args = Vec::new();
        assert!(rd.read_one_op_argument(1, &mut args).unwrap());
        assert_eq!(args[0].cus.as_ref().unwrap().fanout, 1);
        assert!(rd.tread());
        assert_eq!(rd.tok, b"xyz");

        // a result: no number after the group
        let mut rd = reader("out MODELS TRANSFO branch - \"Pick one\" MAX 4 ");
        let mut results = Vec::new();
        assert!(rd.read_one_op_result(1, &mut results).unwrap());
        assert_eq!(results[0].section, OperSection { id: 0, name: v("out"), chapter: 8, group: 1, rank: 50 });
        assert_eq!(results[0].cus, Some(CusSection { scope: 1, cond: None, pick_msg: Some(v("Pick one")), fanout: 4 }));

        // a message the file ends in
        let mut rd = reader("Pick the");
        assert_eq!(rd.cus_read_pick_msg(), b"Pick the");
        let mut rd = reader("\" x");
        assert_eq!(rd.cus_read_pick_msg(), b"");

        // an attribute: four columns, the rest fixed
        let mut rd = reader("Wiggle_Rate a flt - NEXT");
        let mut a = Attribute::init();
        assert!(rd.read_one_attribute(1, &mut a).unwrap());
        assert_eq!((a.id, a.name.as_slice(), a.access, a.scope, a.kind, a.group, a.rank, a.count, a.fcurves, a.incmode), (20000, &b"Wiggle_Rate"[..], 4, 0, TYPE_FLT, 5, 50, 1, 1, 3));
        assert_eq!((a.fcurve_ids.clone(), a.fcurve_names.clone()), (vec![20000], vec![Some(v("Wiggle"))]));
        // its type word is upper-cased before the enumeration is looked up, so a mixed-case name is never found
        let mut rd = with_taste("Kind RW enm(Flavour) [0,1]");
        let mut a = Attribute::init();
        rd.read_one_attribute(1, &mut a).unwrap();
        assert_eq!((a.kind, a.enum_type, a.max, a.max_kind), (TYPE_ENM, -1, 1.0, 1));
    }

    #[test]
    fn group_for_a_custom_effect() {
        let mut rd = reader("");
        let def = OperDef { name: v("Effect"), ..Default::default() };
        assert_eq!(rd.attdict_get_attrib_group(&def, Some(b"Parent")), Some(vec![0, 0]));
        assert_eq!(rd.attdict_get_attrib_group(&def, Some(b"Parent")), Some(vec![0, 0]));
        let other = OperDef { name: v("Other"), ..Default::default() };
        assert_eq!(rd.attdict_get_attrib_group(&other, Some(b"Parent")), Some(vec![0, 1]));
        assert_eq!(rd.attdict_get_attrib_group(&other, None), Some(vec![0, 1])); // the older group of that name
        assert_eq!(rd.out.groups.len(), 2);
        assert_eq!(rd.out.groups[0].rel_type, 1);
    }

    #[test]
    fn fcurve_lists_longer_than_the_count() {
        let text = format!("{ENUMS}ELEMENTS_ATTRIBUTES ELEMENT Thing EAT 1 One R N INT - PARAM 0 2 0 NON 4,5,6 a,b,c,d \"x\" EndOfELEMENT EndOfELEMENTS_ATTRIBUTES\n");
        let (rel, info) = read(text.as_bytes()).unwrap();
        assert_eq!((rel.attributes[0].fcurves, rel.attributes[0].fcurve_ids.len(), rel.attributes[0].fcurve_names.len()), (2, 3, 4));
        assert_eq!(info.fcurve_overruns, [0]);
    }
}
