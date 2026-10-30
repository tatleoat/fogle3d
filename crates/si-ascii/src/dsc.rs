// SPDX-License-Identifier: MIT
//! The scene description file (`.dsc`, tag `SDSC`): reader and writer (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `dictin_scn.c` (reader), `dictout_scn.c` (writer),
//! `physicalio.c` (element display state, play control, relation data), `elemdict.c`
//! (chapter look-up).
//!
//! A scene description lists a scene's elements chapter by chapter, the relations between
//! them and their display state. The original reads it straight into its element dictionary
//! (`SI_ElemDict`) and relation dictionary, and writes it back out of them. Those belong to
//! the dictionary layer and are not ported here: this module keeps what the reader hands to
//! the dictionary in a structure of its own ([`Description`]), with the same look-ups (chapter
//! by name, element by its number in the file), so that every token is consumed exactly as
//! the original consumes it. What is *not* reproduced, because it needs the live dictionary,
//! is said where it happens: the classification of relations, serial numbers, the plug-in
//! look-up for custom effects, the sorting before writing.

use crate::cnum::CLib;
use crate::reldata::{self, RelData, Stuck};
use crate::token::{read_asc_header, AsciiFile};

/// The 36 chapters of a dictionary, in the order the program creates its lists (the order of
/// its table `SI_chapInfo`, 0x086842c0, which `SI_elemdictLoadChapters` 0x08133cf0 walks), so
/// the position is the list index. Each chapter has the suffix of its files; the tag an ASCII
/// file of that chapter starts with is in finding 0006.
pub const CHAPTERS: [(&str, &str); 36] = [
    ("SCENES", "dsc"), ("ANIMATION", "ani"), ("CAMERAS", "cam"), ("CLUSTERS", "cls"), ("PICTURES", "pic"),
    ("LATTICES", "lat"), ("LIGHTS", "lig"), ("MATERIALS", "mtr"), ("MODELS", "hrc"), ("NSELECTIONS", ""),
    ("PALETTES", "pal"), ("RENDER_PICTURES", "pic"), ("SETUP_SOFT", "sts"), ("SETUP_CHANNELS", "chn"),
    ("SETUP_FLOCK", "flk"), ("SHAPES", "shp"), ("STRUCT_KEYS", "key"), ("TEXTURES2D", "txt"), ("TEXTURES3D", "t3D"),
    ("WAVES", "wav"), ("EXPRESSIONS", "exp"), ("MATERIAL_SHADERS", "shd"), ("SHADOW_SHADERS", "shd"),
    ("VOLUME_SHADERS", "shd"), ("TEXTURE2D_SHADERS", "shd"), ("TEXTURE3D_SHADERS", "shd"), ("LIGHT_SHADERS", "shd"),
    ("CAMERA_SHADERS", "shd"), ("OUTPUT_SHADERS", "shd"), ("ENVIRON_SHADERS", "shd"), ("CHANNEL_DRIVERS", ""),
    ("OUTSIDE_DB_FILES", "all"), ("SPREADSHEET_SETUP", "spd"), ("AUDIO", "aiff"), ("MATERIAL_PHOTON_SHADERS", "shd"),
    ("LIGHT_PHOTON_SHADERS", "shd"),
];

/// List indices of the chapters the code names.
pub const SCENES: i32 = 0;
pub const ANIMATION: i32 = 1;
pub const CAMERAS: i32 = 2;
pub const PICTURES: i32 = 4;
pub const LIGHTS: i32 = 6;
pub const MODELS: i32 = 8;
pub const SETUP_SOFT: i32 = 12;
pub const EXPRESSIONS: i32 = 20;

/// The chapter pairs of the RELATIONS section in the order the writer goes through them
/// [soft 0x08130460 SI_dbWriteDescriptionFile] (read from the arguments of its 84 calls;
/// `re/tools/si-ascii/dsc_order.py`). Between entries 61 and 62 it writes the blocks of
/// plug-in relations (`saveCustomRelations`, 0x0812fe78).
pub const RELATION_ORDER: [(&str, &str); 84] = [
    ("MODELS", "MODELS"), ("MODELS", "SHAPES"), ("MODELS", "CLUSTERS"), ("MODELS", "LATTICES"), ("MODELS", "MATERIALS"),
    ("MODELS", "TEXTURES2D"), ("MODELS", "TEXTURES3D"), ("MODELS", "WAVES"), ("MODELS", "CAMERAS"), ("MODELS", "LIGHTS"),
    ("MODELS", "ANIMATION"), ("MODELS", "EXPRESSIONS"), ("CAMERAS", "CAMERAS"), ("CAMERAS", "LIGHTS"), ("CAMERAS", "MODELS"),
    ("CAMERAS", "ANIMATION"), ("CAMERAS", "EXPRESSIONS"), ("LIGHTS", "CAMERAS"), ("LIGHTS", "LIGHTS"), ("LIGHTS", "MODELS"),
    ("LIGHTS", "ANIMATION"), ("LIGHTS", "EXPRESSIONS"), ("WAVES", "MODELS"), ("WAVES", "ANIMATION"), ("WAVES", "EXPRESSIONS"),
    ("MATERIALS", "TEXTURES2D"), ("MATERIALS", "TEXTURES3D"), ("MATERIALS", "ANIMATION"), ("MATERIALS", "MATERIALS"),
    ("MATERIALS", "EXPRESSIONS"), ("SCENES", "ANIMATION"), ("LATTICES", "ANIMATION"), ("SHAPES", "ANIMATION"),
    ("CLUSTERS", "ANIMATION"), ("TEXTURES2D", "ANIMATION"), ("TEXTURES3D", "ANIMATION"), ("LATTICES", "EXPRESSIONS"),
    ("SHAPES", "EXPRESSIONS"), ("CLUSTERS", "EXPRESSIONS"), ("TEXTURES2D", "EXPRESSIONS"), ("TEXTURES3D", "EXPRESSIONS"),
    ("NSELECTIONS", "MODELS"), ("NSELECTIONS", "CAMERAS"), ("NSELECTIONS", "LIGHTS"), ("NSELECTIONS", "WAVES"),
    ("NSELECTIONS", "MATERIALS"), ("NSELECTIONS", "TEXTURES2D"), ("NSELECTIONS", "TEXTURES3D"), ("NSELECTIONS", "CLUSTERS"),
    ("NSELECTIONS", "ANIMATION"), ("MATERIALS", "MATERIAL_SHADERS"), ("MATERIALS", "VOLUME_SHADERS"),
    ("MATERIALS", "SHADOW_SHADERS"), ("MATERIALS", "MATERIAL_PHOTON_SHADERS"), ("LIGHTS", "LIGHT_SHADERS"),
    ("LIGHTS", "LIGHT_PHOTON_SHADERS"), ("TEXTURES2D", "TEXTURE2D_SHADERS"), ("TEXTURES2D", "ENVIRON_SHADERS"),
    ("TEXTURES3D", "TEXTURE3D_SHADERS"), ("SCENES", "OUTPUT_SHADERS"), ("SCENES", "CAMERA_SHADERS"),
    ("SCENES", "VOLUME_SHADERS"),
    // plug-in relation blocks are written here
    ("MATERIAL_SHADERS", "MODELS"), ("VOLUME_SHADERS", "MODELS"), ("SHADOW_SHADERS", "MODELS"),
    ("MATERIAL_PHOTON_SHADERS", "MODELS"), ("LIGHT_SHADERS", "MODELS"), ("LIGHT_PHOTON_SHADERS", "MODELS"),
    ("TEXTURE2D_SHADERS", "MODELS"), ("ENVIRON_SHADERS", "MODELS"), ("TEXTURE3D_SHADERS", "MODELS"),
    ("OUTPUT_SHADERS", "MODELS"), ("CAMERA_SHADERS", "MODELS"), ("MATERIAL_SHADERS", "LIGHTS"),
    ("MATERIAL_PHOTON_SHADERS", "LIGHTS"), ("VOLUME_SHADERS", "LIGHTS"), ("SHADOW_SHADERS", "LIGHTS"),
    ("LIGHT_SHADERS", "LIGHTS"), ("LIGHT_PHOTON_SHADERS", "LIGHTS"), ("TEXTURE2D_SHADERS", "LIGHTS"),
    ("ENVIRON_SHADERS", "LIGHTS"), ("TEXTURE3D_SHADERS", "LIGHTS"), ("OUTPUT_SHADERS", "LIGHTS"),
    ("CAMERA_SHADERS", "LIGHTS"),
];
/// Position in [`RELATION_ORDER`] at which plug-in relation blocks go.
pub const CUSTOM_BLOCKS_AT: usize = 62;

/// The chapters of the ENVIRONMENT section in the order the writer goes through them (same
/// function, its 20 calls of `dbWriteEnvironment`).
pub const ENVIRONMENT_ORDER: [&str; 20] = [
    "CAMERAS", "LIGHTS", "MODELS", "MATERIALS", "TEXTURES2D", "TEXTURES3D", "NSELECTIONS", "WAVES", "ANIMATION",
    "MATERIAL_SHADERS", "TEXTURE2D_SHADERS", "TEXTURE3D_SHADERS", "VOLUME_SHADERS", "SHADOW_SHADERS", "LIGHT_SHADERS",
    "CAMERA_SHADERS", "OUTPUT_SHADERS", "ENVIRON_SHADERS", "MATERIAL_PHOTON_SHADERS", "LIGHT_PHOTON_SHADERS",
];

/// One element of a chapter: the fields of the original's 0x84-byte element record that the
/// scene description reader and writer touch (offsets in the comments).
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The name as the file gives it: `<prefix>-<name>.<version>-<substitution>`, or, in the
    /// PICTURES chapter, a path. The original splits it into a list node
    /// (`SI_dbScnGetListNode`, 0x081319f0, `dbfilenameExpand`, 0x081228c0) and keeps the
    /// second number at +0x4a; the split is the database layer's and not done here.
    pub name: Vec<u8>,
    /// What follows `ALIAS`, with the scene's own alias in front when it has one (never, for a
    /// scene read on its own).
    pub alias: Option<Vec<u8>>,
    /// +0x54. 1 for an element of MODELS, LIGHTS, CAMERAS or EXPRESSIONS that is **not**
    /// marked `ROOT`, 0 for every other element. A root corresponds to a file of the database;
    /// the others are parts of a root's file.
    pub not_root: i32,
    /// +0x5c. The element's number in the file: its position in its chapter, from 0. Relation
    /// and environment lines name elements by it.
    pub work_id: i32,
    /// +0x0c, +0x10: position in the schematic view (`SCHEM`); +0x18: 1 if collapsed.
    pub schem_x: f32,
    pub schem_y: f32,
    pub collapsed: i32,
    /// +0x2a bit 0: `USR` was present.
    pub usr: bool,
    /// +0x1c in the original is one colour-map index built from the two `WIRECOL` numbers by
    /// a display-dependent formula (`SI_mapIndexToWireColour`, 0x083f9278); the two numbers
    /// are kept here. The default (`SI_grafnodeDefColor`) is the pair the writer leaves out:
    /// unselected colour 0, selected colour 7.
    pub wire: (i32, i32),
    /// +0x24 selection state and +0x20 line type, the two `DISPLAY` numbers. Defaults 0 and 2.
    pub selected: i32,
    pub line_type: i32,
    /// +0x3c: the nine `SRT` numbers of a root model (scaling, rotation, translation).
    pub srt: Option<[f32; 9]>,
    /// +0x2c: the `MPRFLG` number. The reader stores it there **as a float**; the writer does
    /// not print that field but the flags of the element's node in the scene, and only if
    /// there is a node. `None`: no `MPRFLG` on the element's line.
    pub mprflg: Option<f32>,
}

impl Element {
    /// A new element as `SI_elemdictInitElem` (0x081326b0) leaves the fields kept here.
    fn new(name: Vec<u8>, alias: Option<Vec<u8>>, not_root: i32, work_id: i32) -> Element {
        Element {
            name,
            alias,
            not_root,
            work_id,
            schem_x: 0.0,
            schem_y: 0.0,
            collapsed: 0,
            usr: false,
            wire: (0, 7),
            selected: 0,
            line_type: 2,
            srt: None,
            mprflg: None,
        }
    }
}

/// A chapter: one list of the dictionary.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub name: Vec<u8>,
    pub elements: Vec<Element>,
}

/// A sub-relation: one line between `SUBREL` and `EndOfSUBREL`.
#[derive(Clone, Debug, PartialEq)]
pub struct SubRelation {
    /// List index of the chapter (`CHAP`; MODELS until one is named) and its name as written.
    pub chapter: i32,
    /// The element (`ELEM`): its number in the file.
    pub elem: i32,
    /// The data in force on this line: its own `RELDATA`, or the previous line's (the original
    /// keeps the pointer from line to line).
    pub data: Option<RelData>,
    /// Whether this line had a `RELDATA` of its own.
    pub own_data: bool,
}

/// One relation line.
#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    /// The two elements, by their numbers in the file.
    pub elem1: i32,
    pub elem2: i32,
    /// The relation id (names in the relations resource file).
    pub relid: i32,
    /// For relation 551 the number after the id; −1 otherwise. (For expressions and relations
    /// 246 and 247 the original then draws a fresh number from a counter; not reproduced.)
    pub serial: i32,
    /// The sub-definition number: which definition line of the relation applies. 1 when the
    /// line gives none.
    pub subdef: i32,
    /// Custom effects (20000) only: the number that followed the id when no `RELDATA` came
    /// after it. The original passes it on separately and sets `subdef` to 1.
    pub sub_index: i32,
    /// False after `NOTACT`.
    pub active: bool,
    pub data: Option<RelData>,
    /// The lines of a `SUBREL … EndOfSUBREL` block, when there is one.
    pub subrels: Option<Vec<SubRelation>>,
    /// What the reader does with the line once it is parsed.
    pub recorded: Recorded,
}

/// The ways the relation reader hands a line to the dictionary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recorded {
    /// 120 InvKin, 121 JntModel, 207 FlexEnv, 208 EnvShp: parsed to the `;` and dropped. (The
    /// model files carry these themselves.)
    Dropped,
    /// With a serial number that ties the line to its sub-relations
    /// (`SI_elemdictAddSerialRelation`, 0x08133184): expressions (20010), 247, and 246 in files
    /// of version 2.99913 or later draw a fresh number from a counter
    /// (`SI_elemdictGetRelSerialNumber`, 0x08139b68); 551 uses the number in the file; a
    /// custom effect (20000) that has data uses the number of its icon.
    Serial,
    /// Everything else: `SI_elemdictAddUnCompleteRelation` (0x08132fb4) if the relation
    /// dictionary says the relation takes several arguments
    /// (`SI_reldictCheckUnCompleteRelation`, 0x0813a770), else `SI_elemdictAddCompleteRelation`
    /// (0x08132dc4).
    Plain,
    /// A custom effect whose icon is not among the scene's models: reported
    /// ([`Message::CustomIconNotFound`]) and not recorded.
    IconMissing,
}

/// A `CHAPTER a CHAPTER b … EndOfCHAPTER` block of the RELATIONS section.
#[derive(Clone, Debug, PartialEq)]
pub struct RelationBlock {
    /// List indices (−1 for a chapter name the dictionary does not have) and names as written.
    pub list1: i32,
    pub list2: i32,
    pub name1: Vec<u8>,
    pub name2: Vec<u8>,
    pub relations: Vec<Relation>,
}

/// A `CHAPTER … EndOfCHAPTER` block of the ENVIRONMENT section: which elements had a line, in
/// the file's order. The values themselves are in the elements.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvBlock {
    pub list: i32,
    pub name: Vec<u8>,
    /// Indices into the chapter's elements.
    pub elems: Vec<usize>,
}

/// The play control block. In the original these are program globals (`SI_frameStart`
/// 0x08782050, `SI_frameEnd` …44, `SI_frameCurrent` …40, `SI_frameStep` …54, `SI_framePause`
/// …48, `SI_frameRate` …4c, `SI_stepAdaptive` 0x0878fb28).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayControl {
    pub start: i32,
    pub end: i32,
    pub current: i32,
    pub step: i32,
    /// Set to 1 when a step of 0 is read, to 0 otherwise.
    pub step_adaptive: i16,
    pub pause: i32,
    pub rate: f32,
}

/// What the original prints on standard error while reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    /// `Element 1 not found ******`
    Element1NotFound,
    /// `Element 2 not found ******`
    Element2NotFound,
    /// `Element not found ******` (environment)
    ElementNotFound,
    /// `Custom icon not found in dictionary`
    CustomIconNotFound,
    /// `ERROR: Invalid Relation Data encountered while reading scene`
    InvalidRelationData,
}

/// A scene description in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    /// The version in the header: that of the program that wrote the file.
    pub version: f32,
    /// The dictionary's lists: the 36 standard chapters, then any other chapter the file names.
    pub chapters: Vec<Chapter>,
    /// The chapters of the ELEMENTS section in the file's order (list indices).
    pub element_order: Vec<i32>,
    pub relations: Vec<RelationBlock>,
    pub environment: Vec<EnvBlock>,
    pub play: Option<PlayControl>,
    pub messages: Vec<Message>,
}

/// Why a file could not be read to its end as the original would.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DscError {
    /// The original would not return: see [`Stuck`].
    Stuck(Stuck),
}

impl From<Stuck> for DscError {
    fn from(s: Stuck) -> Self {
        DscError::Stuck(s)
    }
}

/// How a read went, for the corpus checks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReadInfo {
    /// The file starts with one of the ASCII tags. If it does not, the original goes on
    /// regardless: the version is −1 and the search for `ELEMENTS` starts at the first byte.
    pub has_header: bool,
    /// Nothing but blanks is left after `EndOfENVIRONMENT`.
    pub all_tokens_consumed: bool,
    /// The three sections each ended on their own end word (not at the end of the file).
    pub sections_closed: bool,
}

impl Description {
    fn new() -> Description {
        Description {
            version: -1.0,
            chapters: CHAPTERS.iter().map(|(n, _)| Chapter { name: n.as_bytes().to_vec(), elements: Vec::new() }).collect(),
            element_order: Vec::new(),
            relations: Vec::new(),
            environment: Vec::new(),
            play: None,
            messages: Vec::new(),
        }
    }

    /// The list index of a chapter name, −1 if there is none
    /// [soft 0x081325f4 SI_elemdictFindListByName]. Names compare exactly, except that
    /// `PICTURES` and `RENDER_PICTURES` are also found in any letter case.
    pub fn find_list(&self, name: &[u8]) -> i32 {
        let any_case = name.eq_ignore_ascii_case(b"PICTURES") || name.eq_ignore_ascii_case(b"RENDER_PICTURES");
        self.chapters
            .iter()
            .position(|c| c.name == name || (any_case && c.name.eq_ignore_ascii_case(name)))
            .map_or(-1, |i| i as i32)
    }

    /// Finds a chapter by name or adds it at the end [soft 0x08133c40 SI_elemdictAddList].
    fn add_list(&mut self, name: &[u8]) -> i32 {
        let i = self.find_list(name);
        if i != -1 {
            return i;
        }
        self.chapters.push(Chapter { name: name.to_vec(), elements: Vec::new() });
        self.chapters.len() as i32 - 1
    }

    /// The element a relation or environment line names
    /// [soft 0x08129820 SI_dbGetElemFromDscTokenId] [soft 0x081297b0 elemdictGetElemFromWorkId].
    ///
    /// The token is cut at `@` signs and the first part converted with `atoi`; the element is
    /// the one of the chapter with that number. (The original also matches the scene the
    /// element came from, which matters only when sub-scenes are expanded into the same
    /// dictionary; and it copies the parts into fixed buffers, 12 bytes for the first, so a
    /// first part of 12 characters or more overruns its stack.) In the SCENES chapter any
    /// number names the scene itself.
    ///
    /// Returns the element's index in its chapter. A chapter index of −1 or beyond the lists
    /// makes the original read outside its table: here that is "not found".
    pub fn elem_from_token(&self, list: i32, token: &[u8]) -> Option<usize> {
        let first = token.split(|&b| b == b'@').next().unwrap_or(b"");
        let id = crate::cnum::atoi(first);
        if list == SCENES {
            return Some(0);
        }
        let c = self.chapters.get(usize::try_from(list).ok()?)?;
        c.elements.iter().position(|e| e.work_id == id)
    }
}

impl reldata::ReadCtx for Description {
    fn find_list(&self, name: &[u8]) -> i32 {
        Description::find_list(self, name)
    }
    fn elem_index(&self, list: i32, token: &[u8]) -> i32 {
        self.elem_from_token(list, token).map_or(-1, |i| i as i32)
    }
}

/// The version above which the reader warns that the scene comes from a newer program
/// (a double at 0x0864a130).
pub const NEWER_VERSION_ABOVE: f64 = 4.000001;
/// Relations 246 written by a version below this (a double at 0x0864a138) are recorded the
/// ordinary way; from this version on, and always for 247, with a serial number.
pub const SERIAL_246_FROM: f64 = 2.99913;

/// Reads a scene description: what `SI_elemdictExpandDscFile` (0x0812bcfc) does with one
/// file, without its recursion into sub-scenes (elements of a SCENES chapter, each a
/// description of its own that the original reads between the elements and the relations).
pub fn read(data: &[u8]) -> Result<(Description, ReadInfo), DscError> {
    let mut f = AsciiFile::new(data);
    let mut d = Description::new();
    let mut info = ReadInfo::default();
    let e = read_elements(&mut f, &mut d, &mut info)?;
    let r = read_relations(&mut f, &mut d)?;
    let v = read_environment(&mut f, &mut d)?;
    info.sections_closed = e && r && v;
    info.all_tokens_consumed = f.only_blanks_left();
    Ok((d, info))
}

/// The ELEMENTS section [soft 0x0812a4a0 dbReadDescriptionElements].
///
/// Returns whether the section ended on `EndOfELEMENTS`. The original's own result is always
/// 1 here; its only other exit is the newer-version check in a batch render, which is the
/// caller's business ([`NEWER_VERSION_ABOVE`]).
fn read_elements(f: &mut AsciiFile, d: &mut Description, info: &mut ReadInfo) -> Result<bool, DscError> {
    // the tag itself is not looked at: any of the seven ASCII tags will do
    if let Some((_, version)) = read_asc_header(f) {
        d.version = version;
        info.has_header = true;
    }
    f.token_reach("ELEMENTS", b';');
    let mut ok = f.token_read();
    if !ok || f.is("EndOfELEMENTS") {
        return Ok(ok);
    }
    // the current list: not set until a CHAPTER word is seen (uninitialised in the original)
    let mut list: Option<i32> = None;
    loop {
        if ok && !f.is("EndOfCHAPTER") {
            loop {
                if f.is("CHAPTER") {
                    ok = f.token_read();
                    let l = d.add_list(f.token());
                    list = Some(l);
                } else if f.is("NBELEM") {
                    ok = f.token_read();
                    let n = f.atoi();
                    if n > 0 {
                        // `ok` keeps the result of reading the count
                        read_chapter_elements(f, d, list, n)?;
                    } else {
                        ok = f.token_reach("EndOfCHAPTER", b';');
                    }
                } else {
                    ok = f.token_read();
                }
                if !ok || f.is("EndOfCHAPTER") {
                    break;
                }
            }
        }
        ok = f.token_read();
        if !ok {
            return Ok(f.is("EndOfELEMENTS"));
        }
        if f.is("EndOfELEMENTS") {
            return Ok(true);
        }
    }
}

/// The `count` element lines of a chapter [soft 0x0812a180 dbReadChapterElements].
///
/// Each line is a name, optional `ALIAS <name>` and `ROOT` words in any order, and a `;`.
/// The loop that looks for the `;` has no end-of-file test.
///
/// The original can also skip non-root elements (a flag its one caller passes as 0) and, for
/// the PICTURES chapter, keeps the whole path and files the element under the path's last
/// component. An alias that is read stays in force for the following elements until one is
/// recorded; with nothing skipped that is the same line.
fn read_chapter_elements(f: &mut AsciiFile, d: &mut Description, list: Option<i32>, count: i32) -> Result<(), DscError> {
    for i in 0..count {
        f.token_read();
        let name = f.token().to_vec();
        let rootable = matches!(list, Some(MODELS) | Some(LIGHTS) | Some(CAMERAS) | Some(EXPRESSIONS));
        let mut not_root = rootable as i32;
        let mut alias: Option<Vec<u8>> = None;
        f.token_read();
        while !f.is(";") {
            if f.token().is_empty() && f.only_blanks_left() {
                return Err(DscError::Stuck(Stuck::EndlessAtEof));
            }
            if f.is("ALIAS") {
                f.token_read();
                alias = Some(f.token().to_vec());
            } else if f.is("ROOT") {
                not_root = 0;
            }
            f.token_read();
        }
        // NBELEM before any CHAPTER: the original adds to whatever list index its
        // uninitialised variable holds. Here the elements are dropped.
        if let Some(l) = list {
            d.chapters[l as usize].elements.push(Element::new(name, alias, not_root, i));
        }
    }
    if let Some(l) = list {
        if !d.element_order.contains(&l) {
            d.element_order.push(l);
        }
    }
    Ok(())
}

/// The RELATIONS section [soft 0x0812a9fc dbReadDescriptionRelations].
/// Returns whether it ended on `EndOfRELATIONS`.
fn read_relations(f: &mut AsciiFile, d: &mut Description) -> Result<bool, DscError> {
    f.token_reach("RELATIONS", b';');
    let mut ok = f.token_read();
    if !ok || f.is("EndOfRELATIONS") {
        return Ok(ok);
    }
    // the two chapters of the current block; uninitialised in the original until named
    let (mut list1, mut list2) = (-1i32, -1i32);
    let (mut name1, mut name2) = (Vec::new(), Vec::new());
    let mut have_first = false;
    let mut block: Option<usize> = None;
    loop {
        if ok && !f.is("EndOfCHAPTER") {
            loop {
                if f.is("CHAPTER") {
                    f.token_read();
                    let l = d.find_list(f.token());
                    if have_first {
                        list2 = l;
                        name2 = f.token().to_vec();
                    } else {
                        list1 = l;
                        name1 = f.token().to_vec();
                        have_first = true;
                    }
                    block = None;
                } else {
                    if block.is_none() {
                        d.relations.push(RelationBlock {
                            list1,
                            list2,
                            name1: name1.clone(),
                            name2: name2.clone(),
                            relations: Vec::new(),
                        });
                        block = Some(d.relations.len() - 1);
                    }
                    relation_line(f, d, block.unwrap(), list1, list2)?;
                    have_first = false;
                }
                ok = f.token_read();
                if !ok || f.is("EndOfCHAPTER") {
                    break;
                }
            }
        }
        // a block with no relation line still closes here; nothing is recorded for it
        block = None;
        ok = f.token_read();
        if !ok {
            return Ok(f.is("EndOfRELATIONS"));
        }
        if f.is("EndOfRELATIONS") {
            return Ok(true);
        }
    }
}

/// The data of a relation, with the message the original prints when a generic kind lists
/// more values than it holds.
fn read_data(f: &mut AsciiFile, d: &mut Description, relid: i32) -> Result<Option<RelData>, DscError> {
    let data = reldata::read(f, relid, d)?;
    if let Some(RelData::Floats { capacity, values, .. }) = &data {
        if values.len() > *capacity {
            d.messages.push(Message::InvalidRelationData);
        }
    }
    Ok(data)
}

/// One relation line, entered with the first element's number in the token buffer.
///
/// `<id1> <id2> <relid> [<serial>] [NOTACT] [<subdef>] [RELDATA … EndOfRELDATA]
/// [SUBREL … EndOfSUBREL] ;` — the serial number only for relation 551, read straight after
/// the id. Whatever is left before the `;` is skipped.
///
/// The original then hands the relation to the dictionary in one of several ways depending on
/// the id (`SI_elemdictAddSerialRelation` 0x08133184 for 20010, 246 from version 2.99913, 247,
/// 551 and custom effects; `SI_elemdictAddUnCompleteRelation` or `…AddCompleteRelation` for
/// the rest, as the relation dictionary says). Here the line is recorded as read.
fn relation_line(f: &mut AsciiFile, d: &mut Description, block: usize, list1: i32, list2: i32) -> Result<(), DscError> {
    let Some(e1) = d.elem_from_token(list1, f.token()) else {
        d.messages.push(Message::Element1NotFound);
        f.token_reach(";", b';');
        return Ok(());
    };
    f.token_read();
    let Some(e2) = d.elem_from_token(list2, f.token()) else {
        d.messages.push(Message::Element2NotFound);
        f.token_reach(";", b';');
        return Ok(());
    };
    let id_of = |d: &Description, list: i32, i: usize| -> i32 {
        d.chapters.get(list as usize).and_then(|c| c.elements.get(i)).map_or(0, |e| e.work_id)
    };
    let (id1, id2) = (id_of(d, list1, e1), id_of(d, list2, e2));
    f.token_read();
    let relid = f.atoi();
    let mut rel = Relation {
        elem1: id1,
        elem2: id2,
        relid,
        serial: -1,
        subdef: 1,
        sub_index: 0,
        active: true,
        data: None,
        subrels: None,
        recorded: Recorded::Plain,
    };
    if relid == reldata::REL_SHADER_ARRAY {
        f.token_read();
        rel.serial = f.atoi();
    }
    f.token_read();
    if f.is("NOTACT") {
        rel.active = false;
        f.token_read();
    }
    if !f.is(";") {
        if f.is("RELDATA") {
            rel.data = read_data(f, d, relid)?;
            if f.is("EndOfRELDATA") {
                f.token_read();
            }
        } else if f.is("SUBREL") {
        } else {
            let n = f.atoi();
            rel.subdef = n;
            if relid == reldata::REL_CUSTOM {
                f.token_read();
                if f.is("RELDATA") {
                    rel.data = read_data(f, d, relid)?;
                } else {
                    rel.sub_index = n;
                    rel.subdef = 1;
                }
            }
            if !f.is(";") {
                f.token_reach(";", b';');
            }
        }
    }
    // what becomes of the relation
    let version = d.version as f64;
    rel.recorded = match relid {
        0x78 | 0x79 | 0xcf | 0xd0 => Recorded::Dropped,
        reldata::REL_EXP => {
            // the attribute group comes from the data's parameter id: with no RELDATA the
            // original reads through a null pointer
            if rel.data.is_none() {
                return Err(DscError::Stuck(Stuck::NullDeref));
            }
            Recorded::Serial
        }
        0xf6 if version >= SERIAL_246_FROM => Recorded::Serial,
        reldata::REL_CLUSTER_CENTRE | reldata::REL_SHADER_ARRAY => Recorded::Serial,
        reldata::REL_CUSTOM if rel.data.is_some() => {
            let Some(RelData::Custom(c)) = &mut rel.data else { unreachable!() };
            // With an effect name the original first asks the plug-in system for the effect
            // (`SI_cusPrepareEffect`, 0x0853a688) and replaces the two names by the ones the
            // installed effect goes by. Not reproduced: there are no plug-ins here.
            let Some(icon) = c.icon_id.clone() else {
                return Err(DscError::Stuck(Stuck::NullDeref));
            };
            match d.elem_from_token(MODELS, &icon) {
                Some(i) => {
                    c.icon_elem = i as i32;
                    Recorded::Serial
                }
                None => {
                    c.icon_elem = -1;
                    d.messages.push(Message::CustomIconNotFound);
                    Recorded::IconMissing
                }
            }
        }
        _ => Recorded::Plain,
    };
    if f.is("SUBREL") {
        rel.subrels = Some(read_sub_relations(f, d, relid)?);
        f.token_reach(";", b';');
    } else if !f.is(";") {
        f.token_reach(";", b';');
    }
    // Lines the original drops or cannot record are kept here all the same, marked as such,
    // so that nothing read is lost.
    d.relations[block].relations.push(rel);
    Ok(())
}

/// The lines between `SUBREL` and `EndOfSUBREL` [soft 0x0812a788 dbReadSubRelations]:
/// `[CHAP <chapter>] [ELEM <id>] [RELDATA … EndOfRELDATA] ;`, each part optional and tested
/// in that order.
///
/// A line with no `ELEM` records nothing. A token that is none of the expected words is never
/// consumed, so the original spins on it; so it does at the end of the file.
fn read_sub_relations(f: &mut AsciiFile, d: &mut Description, relid: i32) -> Result<Vec<SubRelation>, DscError> {
    let mut out = Vec::new();
    let mut data: Option<RelData> = None;
    let mut chapter = MODELS;
    f.token_read();
    while !f.is("EndOfSUBREL") {
        let before = f.tokens_read;
        if f.is("CHAP") {
            f.token_read();
            chapter = d.find_list(f.token());
            f.token_read();
        }
        let mut elem: Option<i32> = None;
        if f.is("ELEM") {
            f.token_read();
            elem = d.elem_from_token(chapter, f.token()).map(|i| d.chapters[chapter as usize].elements.get(i).map_or(0, |e| e.work_id));
            f.token_read();
        }
        let mut own_data = false;
        if f.is("RELDATA") {
            data = read_data(f, d, relid)?;
            own_data = true;
            f.token_reach(";", b';');
        }
        if let Some(elem) = elem {
            if relid == reldata::REL_EXP && data.is_none() {
                return Err(DscError::Stuck(Stuck::NullDeref));
            }
            out.push(SubRelation { chapter, elem, data: data.clone(), own_data });
        }
        if f.is(";") {
            f.token_read();
        }
        if f.tokens_read == before {
            let at_eof = f.token().is_empty() && f.only_blanks_left();
            return Err(DscError::Stuck(if at_eof { Stuck::EndlessAtEof } else { Stuck::EndlessOnToken }));
        }
    }
    Ok(out)
}

/// The ENVIRONMENT section [soft 0x0812b498 dbReadDescriptionEnvironment].
/// Returns whether it ended on `EndOfENVIRONMENT`.
///
/// Inside it: `CHAPTER <name>` selects a chapter, `PLAYCTRL` starts the play control block,
/// `RENDER_SETUP` is accepted and ignored, `SETUP <name>` names the scene's setup file the
/// old way (it becomes the one element of the SETUP_SOFT chapter if that chapter is still
/// empty); anything else is an element line: `<id>` followed by `SCHEM`, `SRT`, `USR`,
/// `WIRECOL`, `DISPLAY`, `MPRFLG` items up to a `;`. Unknown words on an element line are
/// skipped one token at a time.
fn read_environment(f: &mut AsciiFile, d: &mut Description) -> Result<bool, DscError> {
    f.token_reach("ENVIRONMENT", b';');
    let mut ok = f.token_read();
    if !ok || f.is("EndOfENVIRONMENT") {
        return Ok(f.is("EndOfENVIRONMENT"));
    }
    let mut list = -1i32; // uninitialised in the original until a CHAPTER word
    let mut block: Option<usize> = None;
    loop {
        if ok && !f.is("EndOfCHAPTER") && !f.is("EndOfENVIRONMENT") {
            loop {
                if f.is("CHAPTER") {
                    f.token_read();
                    list = d.find_list(f.token());
                    d.environment.push(EnvBlock { list, name: f.token().to_vec(), elems: Vec::new() });
                    block = Some(d.environment.len() - 1);
                } else if f.is("PLAYCTRL") {
                    let mut p = d.play.take().unwrap_or(PlayControl { start: 0, end: 0, current: 0, step: 0, step_adaptive: 0, pause: 0, rate: 0.0 });
                    read_play_control(f, &mut p, false);
                    d.play = Some(p);
                } else if f.is("RENDER_SETUP") {
                } else if f.is("SETUP") {
                    f.token_read();
                    let c = &mut d.chapters[SETUP_SOFT as usize];
                    if c.elements.is_empty() {
                        c.elements.push(Element::new(f.token().to_vec(), None, 0, 0));
                    }
                } else {
                    match d.elem_from_token(list, f.token()) {
                        None => {
                            d.messages.push(Message::ElementNotFound);
                            f.token_reach(";", b';');
                        }
                        Some(i) => {
                            if let Some(b) = block {
                                d.environment[b].elems.push(i);
                            }
                            element_line(f, d, list, i);
                        }
                    }
                }
                ok = f.token_read();
                if !ok || f.is("EndOfCHAPTER") || f.is("EndOfENVIRONMENT") {
                    break;
                }
            }
        }
        if !f.is("EndOfENVIRONMENT") {
            ok = f.token_read();
        }
        if !ok {
            return Ok(f.is("EndOfENVIRONMENT"));
        }
        if f.is("EndOfENVIRONMENT") {
            return Ok(true);
        }
    }
}

/// The items of one element's environment line, up to its `;`.
fn element_line(f: &mut AsciiFile, d: &mut Description, list: i32, i: usize) {
    // In the SCENES chapter the line applies to the scene's own element, which this
    // structure does not hold: the values are read and dropped.
    let mut scratch = Element::new(Vec::new(), None, 0, 0);
    let e = match d.chapters.get_mut(list as usize).and_then(|c| c.elements.get_mut(i)) {
        Some(e) => e,
        None => &mut scratch,
    };
    let mut ok = f.token_read();
    if ok && !f.is(";") {
        loop {
            if f.is("SCHEM") {
                // [soft 0x08129a18 dbReadElementSchematics]
                e.schem_x = f.next_f32();
                e.schem_y = f.next_f32();
                e.collapsed = f.next_int();
            } else if f.is("SRT") {
                // [soft 0x08129bec dbReadRootModelSRT]: nine floats into a fresh block
                let mut srt = [0f32; 9];
                for v in srt.iter_mut() {
                    *v = f.next_f32();
                }
                e.srt = Some(srt);
            } else if f.is("USR") {
                e.usr = true;
            } else if f.is("WIRECOL") {
                // [soft 0x081117b8 SI_dbReadElementWireColor]: the original keeps the two
                // numbers cut to 16 bits each
                let a = f.next_int() as i16 as i32;
                let b = f.next_int() as i16 as i32;
                e.wire = (a, b);
            } else if f.is("DISPLAY") {
                // [soft 0x08111988 SI_dbReadElementDisplay]
                e.selected = f.next_int();
                e.line_type = f.next_int();
            } else if f.is("MPRFLG") {
                // [soft 0x0811167c SI_dbReadElementFlags]: an integer stored as a float
                e.mprflg = Some(f.next_int() as f32);
            }
            ok = f.token_read();
            if !ok || f.is(";") {
                break;
            }
        }
    }
    if !f.is(";") {
        f.token_reach(";", b';');
    }
}

/// The play control block, after the word `PLAYCTRL` [soft 0x08110dd0 SI_dbReadPlayControlInfo]:
/// `FRAME <start> <end> <current> <step>`, `PAUSE <frame>`, `RATE <frames per second>`, up to
/// `EndOfPLAYCTRL`.
///
/// `reading_anim_model` is the original's `SI_cusReadingAnimModel()` (0x0853b274): while a
/// custom effect loads an animated model, the start can only move down, the end only up, and
/// the other values are read and dropped.
pub fn read_play_control(f: &mut AsciiFile, p: &mut PlayControl, reading_anim_model: bool) {
    let mut ok = f.token_read();
    while ok && !f.is("EndOfPLAYCTRL") {
        if f.is("FRAME") {
            let v = f.next_int();
            if !reading_anim_model || v < p.start {
                p.start = v;
            }
            let v = f.next_int();
            if !reading_anim_model || v > p.end {
                p.end = v;
            }
            let v = f.next_int();
            if !reading_anim_model {
                p.current = v;
            }
            let v = f.next_int();
            if !reading_anim_model {
                p.step = v;
                p.step_adaptive = (v == 0) as i16;
            }
        } else if f.is("PAUSE") {
            let v = f.next_int();
            if !reading_anim_model {
                p.pause = v;
            }
        } else if f.is("RATE") {
            let v = f.next_f32();
            if !reading_anim_model {
                p.rate = v;
            }
        }
        ok = f.token_read();
    }
}

/// The order of element names [soft 0x08131d60 SI_compareElemName]
/// [soft 0x08131ce4 strcmp_underscore]: bytes compare as signed characters, as in `strcmp`,
/// except that at the first difference an underscore sorts **before** a hexadecimal digit
/// (`isxdigit`: `0`–`9`, `a`–`f`, `A`–`F`), and a name that is a prefix of another sorts
/// first. Negative, zero or positive.
///
/// This is the first key of the sort the writer applies to the elements of a chapter
/// (`elemdictCompareElems`, 0x08131d7c, which goes on to other keys for equal names; the
/// PICTURES chapters sort differently, by name without digits and then by sequence number).
pub fn compare_elem_name(a: &[u8], b: &[u8]) -> i32 {
    let n = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    match (a.get(n), b.get(n)) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(&x), Some(&y)) => {
            if x == b'_' && y.is_ascii_hexdigit() {
                -1
            } else if y == b'_' && x.is_ascii_hexdigit() {
                1
            } else {
                x as i8 as i32 - y as i8 as i32
            }
        }
    }
}

/// The display state the wire-colour conversions depend on: four program globals set from
/// the graphics configuration at start-up (`SI_bitPlanes` 0x08781350, `SI_color1` 0x08781354,
/// `SI_unselectWM` 0x087817c4, `SI_select7WM` 0x08781638). Their values are not in the binary.
/// The select mask is presumably 0x1c0 (the files' second number runs from 1 to 7 and the
/// default pair is (0, 7)); for the two conversions to undo each other the unselect mask must
/// let through the six low bits and the bits of `color1`. Both are guesses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireMap {
    pub bit_planes: i32,
    pub color1: i16,
    pub unselect_mask: i16,
    pub select7_mask: i16,
}

impl WireMap {
    /// The colour-map index an element keeps for the two numbers of a `WIRECOL` item
    /// [soft 0x083f9278 SI_mapIndexToWireColour]. In 16-bit arithmetic: `c2·64 + c1 + color1`;
    /// on a display with exactly 8 bit planes `2·c2 + color1` (for `c2` > 0), the first
    /// number being dropped.
    pub fn index_to_wire(&self, c1: i16, c2: i16) -> i32 {
        if self.bit_planes == 8 {
            let twice = if c2 > 0 { 2 * c2 as i32 } else { 0 };
            (twice + self.color1 as u16 as i32) as i16 as i32
        } else {
            (c2 as i32).wrapping_shl(6).wrapping_add(c1.wrapping_add(self.color1) as i32) as i16 as i32
        }
    }

    /// One of the two numbers back from the index [soft 0x083f92bc SI_mapWireToIndexColour]:
    /// `first` gives the unselected colour (`(w & unselect mask) − color1`), otherwise the
    /// selected one (`(w & select mask) >> 6`). With 8 bit planes the first number is always
    /// 0 and the second is `w / 2` for `w` > 1, without `color1` being taken off: the
    /// conversion does not undo [`WireMap::index_to_wire`] there.
    pub fn wire_to_index(&self, wire: i16, first: bool) -> i32 {
        if self.bit_planes == 8 {
            if first || wire <= 1 {
                0
            } else {
                // counts the steps of 2 down to 1 or 0
                ((wire as i32 - 2) / 2 + 1) as i16 as i32
            }
        } else if first {
            (wire & self.unselect_mask).wrapping_sub(self.color1) as i32
        } else {
            ((wire & self.select7_mask) >> 6) as i32
        }
    }

    /// The index the writer takes for "default colour, write nothing":
    /// `color1 + select mask`, both as unsigned 16-bit numbers.
    pub fn default_wire(&self) -> i32 {
        self.color1 as u16 as i32 + self.select7_mask as u16 as i32
    }
}

/// The number the writer prints after `MPRFLG` for a node's flag word
/// [soft 0x08111634 SI_dbWriteElementFlags]: the word sign-extended, with bits 12 to 15
/// cleared. A word with its top bit set therefore prints as a large negative number.
pub fn mprflg_printed(node_flags: i16) -> i32 {
    node_flags as i32 & !0xf000
}

/// The same comparison as the NT build compiles it (`soft.exe` 0x004c0dc0, found by its
/// shape: the build has no names): letters compare through `toupper`, so the order ignores
/// case, and the final difference is of unsigned characters. A description written on NT
/// lists `cam_int1` before `Camera1`; one written on Linux the other way round.
pub fn compare_elem_name_nt(a: &[u8], b: &[u8]) -> i32 {
    let n = a.iter().zip(b.iter()).take_while(|(x, y)| x.eq_ignore_ascii_case(y)).count();
    match (a.get(n), b.get(n)) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(&x), Some(&y)) => {
            if x == b'_' && y.is_ascii_hexdigit() {
                -1
            } else if y == b'_' && x.is_ascii_hexdigit() {
                1
            } else {
                x.to_ascii_uppercase() as i32 - y.to_ascii_uppercase() as i32
            }
        }
    }
}

// ---- writer ----

/// `fprintf(f, "%s ", w)`
fn word(out: &mut Vec<u8>, w: &[u8]) {
    out.extend_from_slice(w);
    out.push(b' ');
}

fn int(out: &mut Vec<u8>, v: i32) {
    word(out, v.to_string().as_bytes());
}

/// `fprintf(f, "%.7g ", (double)v)`
fn float(out: &mut Vec<u8>, lib: CLib, v: f32) {
    word(out, lib.fmt_g(v as f64, 7).as_bytes());
}

/// The two indentation strings (`SI_sp3Token` 0x08680e40, `SI_sp6Token` 0x08680e44), each
/// printed with `"%s "`.
const SP3: &[u8] = b"   ";
const SP6: &[u8] = b"      ";

impl Description {
    /// Writes the description in the layout of [soft 0x08130460 SI_dbWriteDescriptionFile].
    ///
    /// `version` is the number for the header; the original always writes its own,
    /// `si_file::VERSION_OUTPUT`.
    ///
    /// What this reproduces: every word, number format, blank and line break of the original
    /// writer, its rules for what is left out, and its order of sections, chapters and
    /// blocks: the ELEMENTS chapters sorted by name with `strcmp`
    /// (`SI_elemdictSortChapNames`, 0x0813259c), the relation blocks in [`RELATION_ORDER`]
    /// and the environment chapters in [`ENVIRONMENT_ORDER`].
    ///
    /// What it takes from the structure instead of computing it from a live dictionary as
    /// the original does:
    /// - the order and the numbers of the elements in a chapter (the original sorts them,
    ///   `SI_elemlistSortChapterElements` 0x08132330, first key [`compare_elem_name`], and
    ///   numbers them as it writes them);
    /// - the order of the lines in a relation block and in an environment chapter (the
    ///   dictionary's internal element order), and which elements have an environment line;
    /// - relation blocks whose chapter pair is not in [`RELATION_ORDER`]: they go where the
    ///   original writes the blocks of plug-in relations (`saveCustomRelations`, 0x0812fe78,
    ///   not ported).
    ///
    /// The original also names and opens the file, and afterwards clears the "modified" marks
    /// of the scene and its elements.
    pub fn write(&self, version: f32) -> Vec<u8> {
        self.write_as(version, CLib::Glibc)
    }

    /// [`Description::write`] with the number formatting of another platform's C library: the
    /// NT build writes the same file except for three-digit exponents.
    pub fn write_as(&self, version: f32, lib: CLib) -> Vec<u8> {
        let mut out = Vec::new();
        si_file::write_asc_header(&mut out, version, u32::from_be_bytes(*b"SDSC"));
        word(&mut out, b"ELEMENTS");
        out.push(b'\n');
        let mut lists: Vec<i32> = (0..self.chapters.len() as i32).collect();
        lists.sort_by(|&a, &b| self.chapters[a as usize].name.cmp(&self.chapters[b as usize].name));
        for l in lists {
            self.write_chapter_elements(&mut out, l);
        }
        word(&mut out, b"EndOfELEMENTS");
        out.extend_from_slice(b"\n\n");
        word(&mut out, b"RELATIONS");
        out.push(b'\n');
        let slot = |b: &RelationBlock| RELATION_ORDER.iter().position(|(x, y)| x.as_bytes() == b.name1 && y.as_bytes() == b.name2);
        for i in 0..=RELATION_ORDER.len() {
            if i == CUSTOM_BLOCKS_AT {
                for b in self.relations.iter().filter(|b| slot(b).is_none()) {
                    self.write_chapter_relations(&mut out, lib, b);
                }
            }
            for b in self.relations.iter().filter(|b| i < RELATION_ORDER.len() && slot(b) == Some(i)) {
                self.write_chapter_relations(&mut out, lib, b);
            }
        }
        word(&mut out, b"EndOfRELATIONS");
        out.extend_from_slice(b"\n\n");
        word(&mut out, b"ENVIRONMENT");
        out.push(b'\n');
        // chapters outside the writer's list are not written
        for name in ENVIRONMENT_ORDER {
            for b in self.environment.iter().filter(|b| b.name == name.as_bytes()) {
                self.write_environment(&mut out, lib, b);
            }
        }
        if let Some(p) = &self.play {
            write_play_control(&mut out, lib, p);
        }
        word(&mut out, b"EndOfENVIRONMENT");
        out.push(b'\n');
        out
    }

    /// One chapter of the ELEMENTS section [soft 0x0812f1bc dbWriteChapterElements]: nothing
    /// for the SCENES chapter or a chapter with no element to write. `ROOT` marks the roots
    /// of the four chapters that have them; elements of other chapters that are not roots
    /// are not written at all. (The original also leaves out deleted elements and those a
    /// restricted save excludes.)
    fn write_chapter_elements(&self, out: &mut Vec<u8>, list: i32) {
        if list == SCENES {
            return;
        }
        let c = &self.chapters[list as usize];
        let rootable = matches!(list, MODELS | LIGHTS | CAMERAS | EXPRESSIONS);
        let written: Vec<&Element> = c.elements.iter().filter(|e| e.not_root == 0 || rootable).collect();
        if written.is_empty() {
            return;
        }
        word(out, SP3);
        word(out, b"CHAPTER");
        word(out, &c.name);
        word(out, b"NBELEM");
        int(out, written.len() as i32);
        word(out, SP3);
        out.push(b'\n');
        for e in written {
            word(out, SP6);
            word(out, &e.name);
            if rootable && e.not_root == 0 {
                word(out, b"ROOT");
            }
            word(out, b";");
            out.push(b'\n');
        }
        word(out, SP3);
        word(out, b"EndOfCHAPTER");
        out.extend_from_slice(b"\n\n");
    }

    /// One block of the RELATIONS section [soft 0x0812f7b4 dbWriteChapterRelations] (its
    /// printing pass, with element numbers; the original can also print names).
    ///
    /// A line: the two element numbers, the relation id; `NOTACT` if inactive; the
    /// sub-definition number if above 1; the relation data if the sub-definition is 1 or the
    /// relation is a custom effect; then for 551 the serial number, for expressions (and 246
    /// and 247 between models) the sub-relation block, for a named selection its own data.
    /// Note the serial number comes *after* `NOTACT` here while the reader expects it before:
    /// an inactive 551 relation does not read back.
    fn write_chapter_relations(&self, out: &mut Vec<u8>, lib: CLib, b: &RelationBlock) {
        if b.relations.is_empty() {
            return;
        }
        word(out, SP3);
        word(out, b"CHAPTER");
        word(out, &b.name1);
        word(out, b"CHAPTER");
        word(out, &b.name2);
        out.push(b'\n');
        for r in &b.relations {
            word(out, SP6);
            // [soft 0x0812eacc dbnameSetHierName]: the first element's number as text
            int(out, r.elem1);
            int(out, r.elem2);
            int(out, r.relid);
            // The sub-definition number. The original computes it: for a custom effect a
            // running count when the first element is an effect's icon, else the second value
            // of the relation's data, else 1 with a complaint; for any other relation what
            // the relation dictionary says (`si_reldictGetRelSub`, 0x0813a12c). Here it is
            // the number that was read.
            let sub = if r.relid == reldata::REL_CUSTOM && r.data.is_none() && r.sub_index != 0 { r.sub_index } else { r.subdef };
            if !r.active {
                word(out, b"NOTACT");
            }
            if sub > 1 {
                int(out, sub);
            }
            if sub == 1 || r.relid == reldata::REL_CUSTOM {
                if let Some(data) = &r.data {
                    reldata::write(out, lib, b.list1 == MODELS, r.relid, data);
                }
            }
            if r.relid == reldata::REL_SHADER_ARRAY {
                int(out, r.serial);
            } else if let Some(subs) = &r.subrels {
                self.write_sub_relations(out, lib, r.relid, subs);
            } else if let (reldata::REL_NAMED_SELECTION, ANIMATION, Some(RelData::NamedSelection(n))) = (r.relid, b.list2, &r.data) {
                reldata::write_named_selection(out, n);
            }
            word(out, b";");
            out.push(b'\n');
        }
        word(out, SP3);
        word(out, b"EndOfCHAPTER");
        out.extend_from_slice(b"\n\n");
    }

    /// [soft 0x0812f5c4 dbWriteSubRelations]: a line break, `SUBREL`, one line per
    /// sub-relation, `EndOfSUBREL` (the caller adds the `;`). The original finds the
    /// sub-relations as the relations that follow in the same group with the same id and
    /// serial number.
    fn write_sub_relations(&self, out: &mut Vec<u8>, lib: CLib, relid: i32, subs: &[SubRelation]) {
        out.push(b'\n');
        word(out, SP6);
        word(out, b"SUBREL");
        out.push(b'\n');
        for s in subs {
            word(out, SP6);
            word(out, b"CHAP");
            word(out, self.chapters.get(s.chapter as usize).map_or(&b"?"[..], |c| &c.name));
            word(out, b"ELEM");
            int(out, s.elem);
            if let Some(data) = &s.data {
                reldata::write(out, lib, s.chapter == MODELS, relid, data);
            }
            word(out, b";");
            out.push(b'\n');
        }
        word(out, SP6);
        word(out, b"EndOfSUBREL");
    }

    /// One chapter of the ENVIRONMENT section [soft 0x08130154 dbWriteEnvironment]. A line:
    /// the element's number, `SCHEM` with its three values, then what applies of `USR`,
    /// `WIRECOL`, `DISPLAY`, `SRT`, `MPRFLG`. In the ANIMATION chapter elements that are not
    /// roots get no line.
    fn write_environment(&self, out: &mut Vec<u8>, lib: CLib, b: &EnvBlock) {
        let Some(c) = self.chapters.get(b.list as usize) else { return };
        let lines: Vec<&Element> =
            b.elems.iter().filter_map(|&i| c.elements.get(i)).filter(|e| !(b.list == ANIMATION && e.not_root != 0)).collect();
        if lines.is_empty() {
            return;
        }
        word(out, SP3);
        word(out, b"CHAPTER");
        word(out, &c.name);
        out.push(b'\n');
        for e in lines {
            word(out, SP6);
            int(out, e.work_id);
            // [soft 0x0812efd0 dbWriteElementSchematics]
            word(out, b"SCHEM");
            float(out, lib, e.schem_x);
            float(out, lib, e.schem_y);
            int(out, e.collapsed);
            if e.usr {
                word(out, b"USR");
            }
            // [soft 0x08111738 SI_dbWriteElementWireColor]: left out for the default colour.
            // The original compares the element's colour-map index with the default index
            // and converts it back to two numbers ([`WireMap`]); with the pair kept as read
            // that is "the pair is not (0, 7)" on a display with more than 8 bit planes.
            if e.wire != (0, 7) {
                word(out, b"WIRECOL");
                int(out, e.wire.0);
                int(out, e.wire.1);
            }
            // [soft 0x08111928 SI_dbWriteElementDisplay]: left out when unselected and visible
            if e.selected != 0 || e.line_type != 2 {
                word(out, b"DISPLAY");
                int(out, e.selected);
                int(out, e.line_type);
            }
            // [soft 0x0812f068 dbWriteRootModelSRT]: root models that carry one. (The original
            // frees the nine numbers once printed and clears the element's pointer: a second
            // save writes no SRT. This writer does not change the structure.)
            if b.list == MODELS && e.not_root == 0 {
                if let Some(srt) = &e.srt {
                    word(out, b"SRT");
                    for &v in srt {
                        float(out, lib, v);
                    }
                }
            }
            // The original prints the flag word of the element's scene node, if the element
            // has a node. The node is not here: the number that was read stands in for its
            // flag word, and its presence for the node's.
            if let Some(v) = e.mprflg {
                word(out, b"MPRFLG");
                int(out, mprflg_printed(v as i32 as i16));
            }
            word(out, b";");
            out.push(b'\n');
        }
        word(out, SP3);
        word(out, b"EndOfCHAPTER");
        out.extend_from_slice(b"\n\n");
    }
}

/// [soft 0x08110c20 SI_dbWritePlayControlInfo]
pub fn write_play_control(out: &mut Vec<u8>, lib: CLib, p: &PlayControl) {
    word(out, SP3);
    word(out, b"PLAYCTRL");
    out.push(b'\n');
    word(out, SP6);
    word(out, b"FRAME");
    int(out, p.start);
    int(out, p.end);
    int(out, p.current);
    int(out, p.step);
    out.push(b'\n');
    word(out, SP6);
    word(out, b"PAUSE");
    int(out, p.pause);
    out.push(b'\n');
    word(out, SP6);
    word(out, b"RATE");
    float(out, lib, p.rate);
    out.push(b'\n');
    word(out, SP3);
    word(out, b"EndOfPLAYCTRL");
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "SDSC3.71\nELEMENTS \n    CHAPTER CAMERAS NBELEM 2     \n       a-cam.1-0 ; \n       a-int.1-0 ROOT ; \n    EndOfCHAPTER \n\n    CHAPTER MODELS NBELEM 3     \n       a-m0.1-0 ROOT ; \n       a-m1.1-0 ; \n       a-m2.2-0 ROOT ; \n    EndOfCHAPTER \n\n    CHAPTER SETUP_SOFT NBELEM 1     \n       a.1-0 ; \n    EndOfCHAPTER \n\nEndOfELEMENTS \n\nRELATIONS \n    CHAPTER MODELS CHAPTER MODELS \n       1 0 110 ; \n       2 0 112 2 ; \n       0 2 251 RELDATA CLOSE 1 DEGREE 360 STEP 5 AXIS 1 SUBDIVISION 5 IS_PATCH 0 SPL_TYPE 1 REVOL_SUB 3 EndOfRELDATA ; \n    EndOfCHAPTER \n\n    CHAPTER CAMERAS CHAPTER CAMERAS \n       0 1 1110 ; \n    EndOfCHAPTER \n\nEndOfRELATIONS \n\nENVIRONMENT \n    CHAPTER CAMERAS \n       0 SCHEM 5.5 3 1 MPRFLG 0 ; \n       1 SCHEM 5.5 5 1 MPRFLG 0 ; \n    EndOfCHAPTER \n\n    CHAPTER MODELS \n       0 SCHEM 1 0 0 USR WIRECOL 1 7 DISPLAY 2 2 SRT 1 1 1 0 0 0 0 2.5 0 MPRFLG 0 ; \n       1 SCHEM 2 0 0 MPRFLG 0 ; \n       2 SCHEM 3 0 0 MPRFLG 0 ; \n    EndOfCHAPTER \n\n    PLAYCTRL \n       FRAME 1 100 1 0 \n       PAUSE 0 \n       RATE 30 \n    EndOfPLAYCTRL \nEndOfENVIRONMENT \n";

    #[test]
    fn sample_reads_and_writes_back() {
        let (d, info) = read(SAMPLE.as_bytes()).unwrap();
        assert_eq!(info, ReadInfo { has_header: true, all_tokens_consumed: true, sections_closed: true });
        assert_eq!(d.version, 3.71);
        assert_eq!(d.element_order, vec![CAMERAS, MODELS, SETUP_SOFT]);
        let models = &d.chapters[MODELS as usize].elements;
        assert_eq!(models.iter().map(|e| e.not_root).collect::<Vec<_>>(), vec![0, 1, 0]);
        assert_eq!(d.chapters[SETUP_SOFT as usize].elements[0].not_root, 0); // no roots outside the four chapters
        assert_eq!(models[0].srt, Some([1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 2.5, 0.0]));
        assert_eq!((models[0].usr, models[0].wire, models[0].selected, models[1].line_type), (true, (1, 7), 2, 2));
        assert_eq!(d.relations.len(), 2);
        assert_eq!(d.relations[0].relations[1].subdef, 2);
        assert!(matches!(d.relations[0].relations[2].data, Some(RelData::Record(_))));
        assert_eq!(d.play, Some(PlayControl { start: 1, end: 100, current: 1, step: 0, step_adaptive: 1, pause: 0, rate: 30.0 }));
        assert!(d.messages.is_empty());
        assert_eq!(String::from_utf8(d.write(3.71)).unwrap(), SAMPLE);
        // and the written file reads back to the same structure
        assert_eq!(read(&d.write(3.71)).unwrap().0, d);
    }

    #[test]
    fn defaults_are_left_out_when_written() {
        let text = SAMPLE.replace("1 SCHEM 2 0 0 MPRFLG 0 ;", "1 SCHEM 2 0 0 WIRECOL 0 7 DISPLAY 0 2 ;");
        let (d, _) = read(text.as_bytes()).unwrap();
        let s = String::from_utf8(d.write(4.0)).unwrap();
        assert!(s.starts_with("SDSC4\n"));
        assert!(s.contains("       1 SCHEM 2 0 0 ; \n"));
    }

    #[test]
    fn missing_elements_are_reported_and_skipped() {
        let text = SAMPLE.replace("1 0 110 ;", "9 0 110 ;").replace("2 0 112 2 ;", "2 7 112 2 ;").replace("2 SCHEM 3 0 0", "5 SCHEM 3 0 0");
        let (d, info) = read(text.as_bytes()).unwrap();
        assert!(info.all_tokens_consumed);
        assert_eq!(d.messages, vec![Message::Element1NotFound, Message::Element2NotFound, Message::ElementNotFound]);
        assert_eq!(d.relations[0].relations.len(), 1);
    }

    #[test]
    fn unknown_chapters_and_words() {
        // a chapter the dictionary does not know is added in ELEMENTS, and found afterwards
        let text = "SDSC3.5\nELEMENTS\n CHAPTER GADGETS NBELEM 1\n g.1-0 ALIAS x_ FOO ;\n EndOfCHAPTER\n CHAPTER EMPTY NBELEM 0 junk ; EndOfCHAPTER\nEndOfELEMENTS\nRELATIONS\n CHAPTER GADGETS CHAPTER NOWHERE\n 0 0 5 ;\n EndOfCHAPTER\nEndOfRELATIONS\nENVIRONMENT\n RENDER_SETUP\n SETUP old.1-0\n CHAPTER GADGETS\n 0 SCHEM 1 2 0 WHATEVER 3 ;\n EndOfCHAPTER\nEndOfENVIRONMENT\n";
        let (d, info) = read(text.as_bytes()).unwrap();
        assert!(info.all_tokens_consumed && info.sections_closed);
        let g = d.find_list(b"GADGETS");
        assert_eq!(g, 36);
        assert_eq!(d.chapters[g as usize].elements[0].alias, Some(b"x_".to_vec()));
        assert_eq!(d.find_list(b"EMPTY"), 37);
        assert_eq!(d.messages, vec![Message::Element2NotFound]); // NOWHERE is not a chapter
        assert_eq!(d.chapters[SETUP_SOFT as usize].elements[0].name, b"old.1-0");
        assert_eq!(d.chapters[g as usize].elements[0].schem_y, 2.0);
        // only PICTURES and RENDER_PICTURES are found in another letter case
        assert_eq!(d.find_list(b"pictures"), PICTURES);
        assert_eq!(d.find_list(b"models"), -1);
    }

    #[test]
    fn expression_relations_with_sub_relations() {
        let text = "SDSC3.71\nELEMENTS \n CHAPTER EXPRESSIONS NBELEM 1 \n e.1-0 ROOT ; \n EndOfCHAPTER \n CHAPTER MODELS NBELEM 2 \n m0 ROOT ; \n m1 ROOT ; \n EndOfCHAPTER \nEndOfELEMENTS \nRELATIONS \n    CHAPTER MODELS CHAPTER EXPRESSIONS \n       0 0 20010 RELDATA 139 EndOfRELDATA \n       SUBREL \n       CHAP MODELS ELEM 1 RELDATA 140 EndOfRELDATA ; \n       ELEM 0 ; \n       EndOfSUBREL ; \n    EndOfCHAPTER \n\nEndOfRELATIONS \nENVIRONMENT \nEndOfENVIRONMENT \n";
        let (d, info) = read(text.as_bytes()).unwrap();
        assert!(info.all_tokens_consumed && info.sections_closed);
        let r = &d.relations[0].relations[0];
        let subs = r.subrels.as_ref().unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!((subs[0].elem, subs[0].own_data), (1, true));
        // the second line has no data of its own: the first line's stays in force
        assert_eq!((subs[1].elem, subs[1].own_data), (0, false));
        assert_eq!(subs[1].data, subs[0].data);
        let s = String::from_utf8(d.write(3.71)).unwrap();
        assert!(s.contains("       0 0 20010 RELDATA 139 EndOfRELDATA \n       SUBREL \n       CHAP MODELS ELEM 1 RELDATA 140 EndOfRELDATA ; \n"));
        assert!(s.contains("       EndOfSUBREL ; \n"));
        // an expression relation with no data, and a stray word in a SUBREL block: the
        // original crashes on the first and never returns from the second
        let bad = text.replace("0 0 20010 RELDATA 139 EndOfRELDATA", "0 0 20010");
        assert_eq!(read(bad.as_bytes()).unwrap_err(), DscError::Stuck(Stuck::NullDeref));
        let bad = text.replace("ELEM 0 ;", "STRAY ;");
        assert_eq!(read(bad.as_bytes()).unwrap_err(), DscError::Stuck(Stuck::EndlessOnToken));
    }

    #[test]
    fn serial_number_and_custom_effect_lines() {
        let text = "SDSC3.71\nELEMENTS \n CHAPTER MODELS NBELEM 2 \n m0 ROOT ; \n m1 ROOT ; \n EndOfCHAPTER \n CHAPTER VOLUME_SHADERS NBELEM 1 \n v ; \n EndOfCHAPTER \nEndOfELEMENTS \nRELATIONS \n    CHAPTER VOLUME_SHADERS CHAPTER MODELS \n       0 1 551 3 ; \n    EndOfCHAPTER \n\n    CHAPTER MODELS CHAPTER MODELS \n       0 1 20000 2 RELDATA 0 2 1 0 0 AOCC 0 ROCC 0 ENAME E.v1 MNAME Motion EndOfRELDATA ; \n       0 1 20000 4 ; \n       0 1 207 ; \n    EndOfCHAPTER \n\nEndOfRELATIONS \nENVIRONMENT \nEndOfENVIRONMENT \n";
        let (d, info) = read(text.as_bytes()).unwrap();
        assert!(info.all_tokens_consumed);
        assert_eq!(d.relations[0].relations[0].serial, 3);
        let m = &d.relations[1].relations;
        assert_eq!((m[0].subdef, m[0].sub_index), (2, 0));
        let Some(RelData::Custom(c)) = &m[0].data else { panic!() };
        assert_eq!((c.icon_id.as_deref(), c.icon_elem), (Some(&b"1"[..]), 1));
        // a number after a custom effect's id with no data behind it is not a sub-definition
        assert_eq!((m[1].subdef, m[1].sub_index), (1, 4));
        assert_eq!((m[0].recorded, m[1].recorded, m[2].recorded), (Recorded::Serial, Recorded::Plain, Recorded::Dropped));
        assert_eq!(d.relations[0].relations[0].recorded, Recorded::Serial);
        let s = String::from_utf8(d.write(3.71)).unwrap();
        assert!(s.contains("       0 1 551 3 ; \n"));
        assert!(s.contains("       0 1 20000 2 RELDATA 0 2 1 0 0 AOCC 0 ROCC 0 ENAME E.v1 MNAME Motion EndOfRELDATA ; \n"));
        assert!(s.contains("       0 1 20000 4 ; \n"));
    }

    #[test]
    fn an_empty_relation_block_keeps_its_first_chapter() {
        // The reader files a CHAPTER word as "first" or "second" by a flag that only a
        // relation line clears. After a block with no line the flag is still set, so both
        // chapter words of the next header land in "second": the first chapter stays MODELS.
        let text = "SDSC3.71\nELEMENTS \n CHAPTER CAMERAS NBELEM 1 \n c ROOT ; \n EndOfCHAPTER \n CHAPTER MODELS NBELEM 1 \n m ROOT ; \n EndOfCHAPTER \nEndOfELEMENTS \nRELATIONS \n CHAPTER MODELS CHAPTER MODELS \n EndOfCHAPTER \n CHAPTER CAMERAS CHAPTER CAMERAS \n 0 0 1110 ; \n EndOfCHAPTER \nEndOfRELATIONS \nENVIRONMENT \nEndOfENVIRONMENT \n";
        let (d, _) = read(text.as_bytes()).unwrap();
        assert_eq!(d.relations.len(), 1);
        assert_eq!((d.relations[0].list1, d.relations[0].list2), (MODELS, CAMERAS));
        assert!(d.messages.is_empty());
    }

    #[test]
    fn the_writer_orders_chapters_and_blocks_itself() {
        // sections of the structure in an order no file has: the writer puts the ELEMENTS
        // chapters in strcmp order, the relation blocks and the environment chapters in its
        // fixed order
        let (mut d, _) = read(SAMPLE.as_bytes()).unwrap();
        d.relations.reverse();
        d.environment.reverse();
        d.element_order.reverse();
        assert_eq!(String::from_utf8(d.write(3.71)).unwrap(), SAMPLE);
        // a block whose chapter pair the writer does not know goes where plug-in relations go:
        // after SCENES/VOLUME_SHADERS (61) and before MATERIAL_SHADERS/MODELS (62)
        let (mut d, _) = read(SAMPLE.as_bytes()).unwrap();
        let mut odd = d.relations[1].clone();
        odd.name1 = b"CAMERAS".to_vec();
        odd.name2 = b"WAVES".to_vec();
        d.relations.insert(0, odd);
        let s = String::from_utf8(d.write(3.71)).unwrap();
        let at = |w: &str| s.find(w).unwrap();
        assert!(at("CHAPTER MODELS CHAPTER MODELS") < at("CHAPTER CAMERAS CHAPTER CAMERAS"));
        assert!(at("CHAPTER CAMERAS CHAPTER CAMERAS") < at("CHAPTER CAMERAS CHAPTER WAVES"));
    }

    #[test]
    fn truncated_files() {
        // cut inside an element line: the search for `;` never ends in the original
        assert_eq!(read(b"SDSC3.71\nELEMENTS \n CHAPTER MODELS NBELEM 2 \n m0 ROOT").unwrap_err(), DscError::Stuck(Stuck::EndlessAtEof));
        // cut between sections: the readers stop at the end of the file
        let (d, info) = read(b"SDSC3.71\nELEMENTS \n CHAPTER MODELS NBELEM 1 \n m0 ROOT ; \n EndOfCHAPTER \nEndOfELEMENTS \n").unwrap();
        assert_eq!(d.chapters[MODELS as usize].elements.len(), 1);
        assert_eq!(info, ReadInfo { has_header: true, all_tokens_consumed: true, sections_closed: false });
        // no tag: the original reads on all the same
        let (d, info) = read(b"hello ELEMENTS EndOfELEMENTS\n").unwrap();
        assert_eq!((d.version, info.has_header, info.all_tokens_consumed), (-1.0, false, true));
    }

    #[test]
    fn play_control_while_loading_an_animated_model() {
        let mut p = PlayControl { start: 10, end: 50, current: 20, step: 1, step_adaptive: 0, pause: 3, rate: 25.0 };
        read_play_control(&mut AsciiFile::new(b"FRAME 5 40 7 2 PAUSE 9 RATE 30 EndOfPLAYCTRL "), &mut p, true);
        assert_eq!(p, PlayControl { start: 5, end: 50, current: 20, step: 1, step_adaptive: 0, pause: 3, rate: 25.0 });
        read_play_control(&mut AsciiFile::new(b"FRAME 5 40 7 2 PAUSE 9 RATE 30 EndOfPLAYCTRL "), &mut p, false);
        assert_eq!(p, PlayControl { start: 5, end: 40, current: 7, step: 2, step_adaptive: 0, pause: 9, rate: 30.0 });
    }

    #[test]
    fn wire_colour_conversions() {
        let m = WireMap { bit_planes: 24, color1: 100, unselect_mask: !0x1c0, select7_mask: 0x1c0 };
        let w = m.index_to_wire(3, 5);
        assert_eq!(w, 5 * 64 + 3 + 100);
        // more than 8 planes, a base colour clear of the select bits: the two numbers survive
        let m0 = WireMap { color1: 512, ..m };
        let w = m0.index_to_wire(3, 5) as i16;
        assert_eq!((m0.wire_to_index(w, true), m0.wire_to_index(w, false)), (3, 5));
        assert_eq!(m0.index_to_wire(0, 7), m0.default_wire());
        // exactly 8 planes: the first number is lost, the second comes back shifted by color1 / 2
        let m8 = WireMap { bit_planes: 8, ..m };
        assert_eq!(m8.index_to_wire(9, 3), 106);
        assert_eq!(m8.index_to_wire(9, 0), 100);
        assert_eq!((m8.wire_to_index(106, true), m8.wire_to_index(106, false)), (0, 53));
        assert_eq!(m8.wire_to_index(1, false), 0);
        assert_eq!(m8.wire_to_index(2, false), 1);
        assert_eq!(m8.wire_to_index(7, false), 3);
    }

    #[test]
    fn flag_word_as_printed() {
        assert_eq!(mprflg_printed(0), 0);
        assert_eq!(mprflg_printed(0x1234), 0x234);
        assert_eq!(mprflg_printed(-1), -61441); // 0xffff0fff
    }

    #[test]
    fn element_name_order() {
        assert!(compare_elem_name(b"abc", b"abd") < 0);
        assert!(compare_elem_name(b"ab", b"abc") < 0);
        assert_eq!(compare_elem_name(b"abc", b"abc"), 0);
        // an underscore goes before digits and before a–f / A–F, against its byte value
        assert!(compare_elem_name(b"grid_1", b"grid1") < 0);
        assert!(compare_elem_name(b"x_", b"xF") < 0);
        assert!(compare_elem_name(b"xA", b"x_") > 0);
        // ... but after other capitals, as its byte value says
        assert!(compare_elem_name(b"x_", b"xG") > 0);
        // bytes above 0x7f are negative characters
        assert!(compare_elem_name(b"\xe9", b"a") < 0);
        // the NT build ignores case; the Linux build does not
        assert!(compare_elem_name(b"a-Camera1", b"a-cam_int1") < 0);
        assert!(compare_elem_name_nt(b"a-Camera1", b"a-cam_int1") > 0);
        assert!(compare_elem_name_nt(b"\xe9", b"a") > 0);
    }

    #[test]
    fn the_order_tables_name_known_chapters() {
        let known = |n: &str| CHAPTERS.iter().any(|(c, _)| *c == n);
        assert!(RELATION_ORDER.iter().all(|(a, b)| known(a) && known(b)));
        assert!(ENVIRONMENT_ORDER.iter().all(|n| known(n)));
        assert_eq!(CHAPTERS[MODELS as usize].0, "MODELS");
        assert_eq!(CHAPTERS[EXPRESSIONS as usize].0, "EXPRESSIONS");
        assert_eq!(CHAPTERS[SETUP_SOFT as usize].0, "SETUP_SOFT");
        assert_eq!(RELATION_ORDER[CUSTOM_BLOCKS_AT].0, "MATERIAL_SHADERS");
    }
}
