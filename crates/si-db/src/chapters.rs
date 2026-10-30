// SPDX-License-Identifier: MIT
//! The chapter table and the chapter ids.

use crate::Bytes;

/// One row of the program's chapter table: three 32-bit words in the binary (two string
/// pointers and a tag).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChapInfo {
    /// Name of the chapter and of its directory in a database.
    pub directory: &'static str,
    /// File suffix of the chapter's elements, without the dot.
    pub suffix: &'static str,
    /// A four-character tag stored as a big-endian word, 0 for none. It is the ASCII header
    /// tag of the chapter's files where one exists (`SDSC`, `SSET`, `SCHN`, `SFLK`, `SWAV`,
    /// the `SH..` shader parameter tags); the table is consulted for it by the user-data
    /// routines `SI_udfInitialize` and `SI_udfWrite`, which have not been read.
    pub tag: u32,
}

const fn tag(t: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*t)
}

const fn row(directory: &'static str, suffix: &'static str, tag: u32) -> ChapInfo {
    ChapInfo { directory, suffix, tag }
}

/// The chapter table, all 36 rows in the binary's order [soft 0x086842c0 SI_chapInfo].
/// The row number is the public chapter number of the SDK (`SAA_ChapterType`, 0 to 35).
///
/// The table has no column for the number of versions kept: that default is the constant
/// [`DEFAULT_VERSIONS`] wherever a chapter record is made.
pub const CHAP_INFO: [ChapInfo; 36] = [
    row("SCENES", "dsc", tag(b"SDSC")),
    row("ANIMATION", "ani", tag(b"SANI")),
    row("CAMERAS", "cam", tag(b"SCAM")),
    row("CLUSTERS", "cls", tag(b"CLUS")),
    row("PICTURES", "pic", 0),
    row("LATTICES", "lat", tag(b"SLAT")),
    row("LIGHTS", "lig", tag(b"SLIG")),
    row("MATERIALS", "mtr", tag(b"SMAT")),
    row("MODELS", "hrc", 0),
    row("NSELECTIONS", "", 0),
    row("PALETTES", "pal", 0),
    row("RENDER_PICTURES", "pic", 0),
    row("SETUP_SOFT", "sts", tag(b"SSET")),
    row("SETUP_CHANNELS", "chn", tag(b"SCHN")),
    row("SETUP_FLOCK", "flk", tag(b"SFLK")),
    row("SHAPES", "shp", 0),
    row("STRUCT_KEYS", "key", tag(b"SKEY")),
    row("TEXTURES2D", "txt", tag(b"ST2D")),
    row("TEXTURES3D", "t3D", tag(b"ST3D")),
    row("WAVES", "wav", tag(b"SWAV")),
    row("EXPRESSIONS", "exp", 0),
    row("MATERIAL_SHADERS", "shd", tag(b"SHAT")),
    row("SHADOW_SHADERS", "shd", tag(b"SHAD")),
    row("VOLUME_SHADERS", "shd", tag(b"SHVL")),
    row("TEXTURE2D_SHADERS", "shd", tag(b"SHT2")),
    row("TEXTURE3D_SHADERS", "shd", tag(b"SHT3")),
    row("LIGHT_SHADERS", "shd", tag(b"SHIG")),
    row("CAMERA_SHADERS", "shd", tag(b"SHAM")),
    row("OUTPUT_SHADERS", "shd", tag(b"SHUT")),
    row("ENVIRON_SHADERS", "shd", tag(b"SHNV")),
    row("CHANNEL_DRIVERS", "", 0),
    row("OUTSIDE_DB_FILES", "all", 0),
    row("SPREADSHEET_SETUP", "spd", 0),
    row("AUDIO", "aiff", 0),
    row("MATERIAL_PHOTON_SHADERS", "shd", tag(b"SHAP")),
    row("LIGHT_PHOTON_SHADERS", "shd", tag(b"SHIP")),
];

/// How many versions of an element a chapter keeps unless its `Chapter.rsrc` says otherwise:
/// the value given to every new chapter record [soft 0x0812164c dbInfoAddChapter], written
/// into every new `Chapter.rsrc`, and returned for a chapter the database does not have.
pub const DEFAULT_VERSIONS: i32 = 4;

/// What [`ed_get_suffix_from_chapter`] returns for a chapter the table does not cover.
pub const UNKNOWN_SUFFIX: &str = "???";

/// Chapter ids. An id is the position of the chapter's element list in an element
/// dictionary, handed out in registration order; the standard registration below gives the
/// table's row numbers.
pub mod chap {
    pub const SCENES: i32 = 0;
    pub const ANIMATION: i32 = 1;
    pub const CAMERAS: i32 = 2;
    pub const CLUSTERS: i32 = 3;
    pub const PICTURES: i32 = 4;
    pub const LATTICES: i32 = 5;
    pub const LIGHTS: i32 = 6;
    pub const MATERIALS: i32 = 7;
    pub const MODELS: i32 = 8;
    pub const NSELECTIONS: i32 = 9;
    pub const PALETTES: i32 = 10;
    pub const RENDER_PICTURES: i32 = 11;
    pub const SETUP_SOFT: i32 = 12;
    pub const SETUP_CHANNELS: i32 = 13;
    pub const SETUP_FLOCK: i32 = 14;
    pub const SHAPES: i32 = 15;
    pub const STRUCT_KEYS: i32 = 16;
    pub const TEXTURES2D: i32 = 17;
    pub const TEXTURES3D: i32 = 18;
    pub const WAVES: i32 = 19;
    pub const EXPRESSIONS: i32 = 20;
    pub const MATERIAL_SHADERS: i32 = 21;
    pub const SHADOW_SHADERS: i32 = 22;
    pub const VOLUME_SHADERS: i32 = 23;
    pub const TEXTURE2D_SHADERS: i32 = 24;
    pub const TEXTURE3D_SHADERS: i32 = 25;
    pub const LIGHT_SHADERS: i32 = 26;
    pub const CAMERA_SHADERS: i32 = 27;
    pub const OUTPUT_SHADERS: i32 = 28;
    pub const ENVIRON_SHADERS: i32 = 29;
    pub const CHANNEL_DRIVERS: i32 = 30;
    pub const OUTSIDE_DB_FILES: i32 = 31;
    pub const SPREADSHEET_SETUP: i32 = 32;
    pub const AUDIO: i32 = 33;
    pub const MATERIAL_PHOTON_SHADERS: i32 = 34;
    pub const LIGHT_PHOTON_SHADERS: i32 = 35;
}

/// Kinds of element list (the 16-bit field at offset 0x18 of the list record).
/// 0: lives only in memory; a database has no directory for it.
pub const LIST_KIND_MEMORY: i16 = 0;
/// 1: a chapter directory of a database (the value a new list starts with).
pub const LIST_KIND_DATABASE: i16 = 1;
/// 2: files outside any database; the list carries its own directory.
pub const LIST_KIND_EXTERNAL: i16 = 2;

/// One element list of a dictionary, as far as the database layer looks at it.
#[derive(Clone, Debug, PartialEq)]
pub struct ElemListInfo {
    /// Offset 0x00 of the list record: the chapter's name.
    pub name: Bytes,
    /// Offset 0x18: one of the `LIST_KIND_` values.
    pub kind: i16,
}

/// What the database layer reads of an element dictionary (`SI_ElemDict`): its lists, in
/// id order. A chapter's id is its position here.
#[derive(Clone, Debug, PartialEq)]
pub struct ElemLists {
    pub lists: Vec<ElemListInfo>,
}

impl ElemLists {
    /// The lists a dictionary starts with: one per row of [`CHAP_INFO`], registered in table
    /// order, so the id of a chapter is its row number. Three are then marked as not being
    /// database chapters: named selections and channel drivers (memory only) and files
    /// outside a database.
    ///
    /// This is what `SI_elemdictLoadChapters` (0x08133cf0, `elemdict.c`) leaves in an empty
    /// dictionary. That routine is read but not ported here: it also records each id in the
    /// global id structure (`SI_cid`, 0x08781660; see [`CHAP_IDS`]), copies the table's suffix
    /// into each list and sets dictionary fields that belong to the element dictionary.
    pub fn standard() -> ElemLists {
        let lists = CHAP_INFO
            .iter()
            .enumerate()
            .map(|(id, c)| ElemListInfo {
                name: c.directory.as_bytes().to_vec(),
                kind: match id as i32 {
                    chap::NSELECTIONS | chap::CHANNEL_DRIVERS => LIST_KIND_MEMORY,
                    chap::OUTSIDE_DB_FILES => LIST_KIND_EXTERNAL,
                    _ => LIST_KIND_DATABASE,
                },
            })
            .collect();
        ElemLists { lists }
    }

    /// The id of the list called `name`, or −1 [soft 0x081325f4 SI_elemdictFindListByName].
    /// Names compare exactly, except that a name which is `PICTURES` or `RENDER_PICTURES`
    /// in any letter case finds the list of that name in any letter case.
    pub fn find_list_by_name(&self, name: &[u8]) -> i32 {
        for (id, l) in self.lists.iter().enumerate() {
            if l.name == name {
                return id as i32;
            }
            for special in [CHAP_INFO[4].directory, CHAP_INFO[11].directory] {
                if special.as_bytes().eq_ignore_ascii_case(name) && l.name.eq_ignore_ascii_case(name) {
                    return id as i32;
                }
            }
        }
        -1
    }
}

/// The global structure of chapter ids (`SI_cid`, 36 words; the SDK's `SAA_ChapIds` is its
/// first 33): for each field in the structure's order, the chapter id the standard
/// registration stores there. The fields are not in table order.
pub const CHAP_IDS: [i32; 36] = [
    chap::ANIMATION,
    chap::CAMERAS,
    chap::CLUSTERS,
    chap::SCENES,
    chap::PICTURES,
    chap::LATTICES,
    chap::LIGHTS,
    chap::MATERIALS,
    chap::MODELS,
    chap::NSELECTIONS,
    chap::PALETTES,
    chap::RENDER_PICTURES,
    chap::MATERIAL_SHADERS,
    chap::TEXTURE2D_SHADERS,
    chap::TEXTURE3D_SHADERS,
    chap::VOLUME_SHADERS,
    chap::SHADOW_SHADERS,
    chap::LIGHT_SHADERS,
    chap::CAMERA_SHADERS,
    chap::OUTPUT_SHADERS,
    chap::ENVIRON_SHADERS,
    chap::SHAPES,
    chap::SETUP_SOFT,
    chap::SETUP_CHANNELS,
    chap::SETUP_FLOCK,
    chap::STRUCT_KEYS,
    chap::TEXTURES2D,
    chap::TEXTURES3D,
    chap::WAVES,
    chap::EXPRESSIONS,
    chap::CHANNEL_DRIVERS,
    chap::OUTSIDE_DB_FILES,
    chap::SPREADSHEET_SETUP,
    chap::AUDIO,
    chap::MATERIAL_PHOTON_SHADERS,
    chap::LIGHT_PHOTON_SHADERS,
];

/// The comparisons of [`chap_info_offset_from_chap_id_with`], in the original's order:
/// (field of the id structure, table row).
const OFFSET_FROM_ID: [(usize, usize); 35] = [
    (8, 8),
    (0, 1),
    (1, 2),
    (0x15, 0xf),
    (2, 3),
    (6, 6),
    (5, 5),
    (7, 7),
    (0x1a, 0x11),
    (0x1b, 0x12),
    (3, 0),
    (10, 10),
    (0x1c, 0x13),
    (0x1d, 0x14),
    (0x16, 0xc),
    (0x17, 0xd),
    (0x18, 0xe),
    (0x19, 0x10),
    (4, 0xb),
    (0xb, 0xb),
    (0x1e, 0x1e),
    (0x1f, 0x1f),
    (0x20, 0x20),
    (0x21, 0x21),
    (0xc, 0x15),
    (0x22, 0x22),
    (0x10, 0x16),
    (0xf, 0x17),
    (0xd, 0x18),
    (0xe, 0x19),
    (0x11, 0x1a),
    (0x23, 0x23),
    (0x12, 0x1b),
    (0x13, 0x1c),
    (0x14, 0x1d),
];

/// The table row of a chapter id [soft 0x08120794 SI_chapInfoOffsetFromChapId], given the
/// id structure the original reads from its global `SI_cid`: the id is compared with one
/// field after another and the first match decides.
///
/// Two chapters do not get their own row: **pictures share the row of rendered pictures**
/// (row 11, same suffix), and named selections have no row at all.
pub fn chap_info_offset_from_chap_id_with(cid: &[i32; 36], id: i32) -> Option<usize> {
    OFFSET_FROM_ID.iter().find(|(field, _)| cid[*field] == id).map(|(_, row)| *row)
}

/// [`chap_info_offset_from_chap_id_with`] for the ids of the standard registration
/// ([`CHAP_IDS`]), which is the only one the program makes.
pub fn chap_info_offset_from_chap_id(id: i32) -> Option<usize> {
    chap_info_offset_from_chap_id_with(&CHAP_IDS, id)
}

/// [soft 0x08120a18 SI_chapInfoFromChapId]
pub fn chap_info_from_chap_id(id: i32) -> Option<&'static ChapInfo> {
    chap_info_offset_from_chap_id(id).map(|i| &CHAP_INFO[i])
}

/// The file suffix of a chapter, `???` for an id without a row
/// [soft 0x08120a48 SI_edGetSuffixFromChapter].
pub fn ed_get_suffix_from_chapter(id: i32) -> &'static str {
    chap_info_from_chap_id(id).map_or(UNKNOWN_SUFFIX, |c| c.suffix)
}

/// The first chapter id, below `lists` (the number of lists in the dictionary), whose suffix
/// is `suffix` [soft 0x08120a7c SI_edGetChapterFromSuffix]. Suffixes are not unique: `pic`
/// finds pictures, never rendered pictures; `shd` finds material shaders; the empty suffix
/// finds channel drivers and `???` finds named selections.
pub fn ed_get_chapter_from_suffix(lists: i32, suffix: Option<&[u8]>) -> i32 {
    let Some(suffix) = suffix else { return -1 };
    (0..lists).find(|&id| ed_get_suffix_from_chapter(id).as_bytes() == suffix).unwrap_or(-1)
}

/// Whether a chapter's files are text [soft 0x08120ad4 SI_dbIsChapterWithAsciiFile], given
/// the id structure the original reads from `SI_cid`: scenes, the three setups, waves,
/// expressions, every shader chapter and the spreadsheet setup always; with
/// `also_optional` set, also the chapters whose files exist in both a binary and a text
/// form (materials, 2D and 3D textures, models, lights, cameras, lattices, animation,
/// structure keys).
pub fn is_chapter_with_ascii_file_with(cid: &[i32; 36], id: i32, also_optional: bool) -> bool {
    const ALWAYS: [usize; 18] = [3, 0x1c, 0x16, 0x17, 0x18, 0x1d, 0xc, 0x22, 0xd, 0xe, 0xf, 0x12, 0x13, 0x10, 0x14, 0x11, 0x23, 0x20];
    const OPTIONAL: [usize; 9] = [7, 0x1a, 0x1b, 8, 6, 1, 5, 0, 0x19];
    ALWAYS.iter().any(|&f| cid[f] == id) || (also_optional && OPTIONAL.iter().any(|&f| cid[f] == id))
}

/// [`is_chapter_with_ascii_file_with`] for the ids of the standard registration.
pub fn is_chapter_with_ascii_file(id: i32, also_optional: bool) -> bool {
    is_chapter_with_ascii_file_with(&CHAP_IDS, id, also_optional)
}

/// One row of the shader chapter table: four 32-bit words in the binary (a pointer to the
/// chapter's row of [`CHAP_INFO`], a suffix, a tag and a number).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShdChapInfo {
    /// Row of [`CHAP_INFO`] the record points at.
    pub chapter: usize,
    /// Suffix of the chapter's shader *description* files. The chapter table's own suffix
    /// for all these chapters is `shd`.
    pub suffix: &'static str,
    /// The ASCII header tag of those files (`SDAT`, …: "custom shader description").
    pub tag: u32,
    /// A number whose meaning has not been read (1, 9, 2, 3, 4, 5, 6, 10, 7, 14, 15).
    pub number: i32,
}

const fn shd(chapter: usize, suffix: &'static str, t: &[u8; 4], number: i32) -> ShdChapInfo {
    ShdChapInfo { chapter, suffix, tag: tag(t), number }
}

/// The shader chapter table, all 11 rows [soft 0x086e4920 si_shdChapInfo] (its length is
/// the word at 0x086e49d0). It belongs to the shader code (`udf_fct.c`), not to the
/// database layer, and is here because it names the second suffix each shader chapter's
/// directory holds.
pub const SHD_CHAP_INFO: [ShdChapInfo; 11] = [
    shd(21, "mat", b"SDAT", 1),
    shd(22, "sha", b"SDAD", 9),
    shd(23, "vol", b"SDVL", 2),
    shd(24, "tex", b"SDT2", 3),
    shd(25, "tx3", b"SDT3", 4),
    shd(26, "lit", b"SDIG", 5),
    shd(27, "len", b"SDAM", 6),
    shd(28, "out", b"SDUT", 10),
    shd(29, "env", b"SDNV", 7),
    shd(34, "mph", b"SDAP", 14),
    shd(35, "lph", b"SDIP", 15),
];

/// The shader table's row for a chapter id [soft 0x0854c824 si_shdChapInfoFromChapId].
pub fn shd_chap_info_from_chap_id(id: i32) -> Option<&'static ShdChapInfo> {
    let row = chap_info_offset_from_chap_id(id)?;
    SHD_CHAP_INFO.iter().find(|s| s.chapter == row)
}

/// The shader table's row for a suffix [soft 0x0854c8b0 si_shdChapInfoFromSuffix].
pub fn shd_chap_info_from_suffix(suffix: &[u8]) -> Option<&'static ShdChapInfo> {
    SHD_CHAP_INFO.iter().find(|s| s.suffix.as_bytes() == suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_ids() {
        assert_eq!(CHAP_INFO[chap::MODELS as usize], row("MODELS", "hrc", 0));
        assert_eq!(CHAP_INFO[0].tag, 0x5344_5343);
        assert_eq!(ed_get_suffix_from_chapter(chap::TEXTURES3D), "t3D");
        assert_eq!(ed_get_suffix_from_chapter(chap::NSELECTIONS), "???");
        assert_eq!(ed_get_suffix_from_chapter(99), "???");
        // pictures use the row of rendered pictures
        assert_eq!(chap_info_from_chap_id(chap::PICTURES).unwrap().directory, "RENDER_PICTURES");
        // every id is stored once in the id structure
        let mut seen = CHAP_IDS;
        seen.sort();
        assert!(seen.iter().enumerate().all(|(i, &v)| v == i as i32));
        assert_eq!(CHAP_IDS[3], chap::SCENES);
        assert_eq!(CHAP_IDS[0x15], chap::SHAPES);
    }

    #[test]
    fn suffix_to_chapter_takes_the_first() {
        assert_eq!(ed_get_chapter_from_suffix(36, Some(b"pic")), chap::PICTURES);
        assert_eq!(ed_get_chapter_from_suffix(36, Some(b"shd")), chap::MATERIAL_SHADERS);
        assert_eq!(ed_get_chapter_from_suffix(36, Some(b"")), chap::CHANNEL_DRIVERS);
        assert_eq!(ed_get_chapter_from_suffix(36, Some(b"???")), chap::NSELECTIONS);
        assert_eq!(ed_get_chapter_from_suffix(36, Some(b"xyz")), -1);
        assert_eq!(ed_get_chapter_from_suffix(8, Some(b"hrc")), -1);
        assert_eq!(ed_get_chapter_from_suffix(36, None), -1);
    }

    #[test]
    fn kinds_and_text_chapters() {
        let std = ElemLists::standard();
        assert_eq!(std.lists.len(), 36);
        assert_eq!(std.lists.iter().filter(|l| l.kind == LIST_KIND_DATABASE).count(), 33);
        assert_eq!(std.lists[chap::OUTSIDE_DB_FILES as usize].kind, LIST_KIND_EXTERNAL);
        assert!(is_chapter_with_ascii_file(chap::SCENES, false));
        assert!(!is_chapter_with_ascii_file(chap::MODELS, false));
        assert!(is_chapter_with_ascii_file(chap::MODELS, true));
        assert!(!is_chapter_with_ascii_file(chap::PICTURES, true));
        let text: Vec<i32> = (0..36).filter(|&i| is_chapter_with_ascii_file(i, false)).collect();
        assert_eq!(text, [0, 12, 13, 14, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 32, 34, 35]);
        let both: Vec<i32> = (0..36).filter(|&i| is_chapter_with_ascii_file(i, true) && !is_chapter_with_ascii_file(i, false)).collect();
        assert_eq!(both, [1, 2, 5, 6, 7, 8, 16, 17, 18]);
    }

    #[test]
    fn rows_follow_the_id_structure() {
        // every id but named selections has a row; only pictures land on another's
        for id in 0..36 {
            let want = match id {
                chap::PICTURES => Some(11),
                chap::NSELECTIONS => None,
                _ => Some(id as usize),
            };
            assert_eq!(chap_info_offset_from_chap_id(id), want);
        }
        assert_eq!(chap_info_offset_from_chap_id(36), None);
        // with other ids in the structure the first matching field decides
        let mut cid = CHAP_IDS;
        cid[8] = 100; // models
        cid[0] = 100; // animation
        assert_eq!(chap_info_offset_from_chap_id_with(&cid, 100), Some(8));
        assert_eq!(chap_info_offset_from_chap_id_with(&cid, chap::MODELS), None);
    }

    #[test]
    fn shader_table() {
        assert_eq!(shd_chap_info_from_chap_id(chap::CAMERA_SHADERS).unwrap().suffix, "len");
        assert_eq!(shd_chap_info_from_chap_id(chap::LIGHT_PHOTON_SHADERS).unwrap().tag, u32::from_be_bytes(*b"SDIP"));
        assert_eq!(shd_chap_info_from_chap_id(chap::MODELS), None);
        assert_eq!(CHAP_INFO[shd_chap_info_from_suffix(b"tx3").unwrap().chapter].directory, "TEXTURE3D_SHADERS");
        assert_eq!(shd_chap_info_from_suffix(b"shd"), None);
    }

    #[test]
    fn list_names() {
        let std = ElemLists::standard();
        assert_eq!(std.find_list_by_name(b"MODELS"), chap::MODELS);
        assert_eq!(std.find_list_by_name(b"models"), -1);
        assert_eq!(std.find_list_by_name(b"Pictures"), chap::PICTURES);
        assert_eq!(std.find_list_by_name(b"render_pictures"), chap::RENDER_PICTURES);
        assert_eq!(std.find_list_by_name(b"SYSTEMS"), -1);
    }
}
