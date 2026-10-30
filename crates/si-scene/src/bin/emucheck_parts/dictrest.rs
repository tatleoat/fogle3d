// SPDX-License-Identifier: MIT
//! The outside world of `dictrest.rs`'s routines for `dict2-emucheck`
//! (`re/tools/si-scene/emu_gen_dict3.py`): the routines the harness replaces by recorded
//! stand-ins, with the effects and the logged forms the generator's stand-ins have.
//!
//! A stack local handed to a callee cannot be compared by address: the generator's
//! stand-in logs what the local holds (or a checksum of a text) in the place of the
//! address, and the port's call is translated the same way here.

use super::{Outside, Recorded, Routine};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by `dict2-emucheck`: only that binary's cases use this file's stand-ins.
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
use si_mem::Mem;

macro_rules! routines {
    ($($id:ident = $name:literal, $addr:expr;)*) => {
        $(static $id: Routine = Routine { name: $name, addr: $addr };)*
        pub static CALLEES: &[&Routine] = &[$(&$id),*];
    };
}
use si_scene::dictrest as d;
routines! {
    R_STATIC_VALUE_SET = "SI_edElementStaticValueSet", d::SI_ED_ELEMENT_STATIC_VALUE_SET;
    R_SET_CHAPTER = "SI_dbSetChapter", d::SI_DB_SET_CHAPTER;
    R_FILEPATH_BUILD = "SI_dbfilepathBuild", d::SI_DBFILEPATH_BUILD;
    R_CHANGE_PRECISION = "SI_dbfilenameChangePrecision", d::SI_DBFILENAME_CHANGE_PRECISION;
    R_FIND_SUBST = "si_dbfileFindSubstitutionFile", d::SI_DBFILE_FIND_SUBSTITUTION_FILE;
    R_FORMAT_FULL_NAME = "SI_dbFormatFullName", d::SI_DB_FORMAT_FULL_NAME;
    R_CONVERT_PATCH = "SI_convertPatch", d::SI_CONVERT_PATCH;
    R_READ_MODEL = "SI_elemdictReadModel", d::SI_ELEMDICT_READ_MODEL;
    R_FIND_PRIORITY = "SI_conslistFindPriority", d::SI_CONSLIST_FIND_PRIORITY;
    R_FIND_PREV = "SI_conslistFindPrevConstraint", d::SI_CONSLIST_FIND_PREV_CONSTRAINT;
    R_READ_RELATIONS = "SI_reldictReadRelations", d::SI_REL_DICT_READ_RELATIONS;
    R_SET_FCTS = "SI_attdictSetFctsPointers", d::SI_ATTDICT_SET_FCTS_POINTERS;
    R_ERROR = "SI_error", d::SI_ERROR;
    R_THREAD_INIT = "SI_threadStorageInit", d::SI_THREAD_STORAGE_INIT;
    R_RELDICT_INIT = "SI_reldictInit", d::SI_REL_DICT_INIT;
    R_ELEMDICT_ADD = "SI_elemdictAdd", d::SI_ELEMDICT_ADD;
    R_ELEMDICT_INIT = "SI_elemdictInit", d::SI_ELEMDICT_INIT;
    R_LOAD_CHAPTERS = "SI_elemdictLoadChapters", d::SI_ELEMDICT_LOAD_CHAPTERS;
    R_CREATE_DEFAULT_DSC = "SI_elemdictCreateDefaultDsc", d::SI_ELEMDICT_CREATE_DEFAULT_DSC;
    R_LOAD_REL_DICT = "SI_dbLoadRelationDictionary", d::SI_DB_LOAD_RELATION_DICTIONARY;
    R_DIR_PREPARE = "SI_dbDatabaseDirPrepare", d::SI_DB_DATABASE_DIR_PREPARE;
    R_DIR_LOAD = "SI_databaseDirectoryLoad", d::SI_DATABASE_DIRECTORY_LOAD;
    R_SET_CURRENT_DB = "SI_dbSetCurrentDatabase", d::SI_DB_SET_CURRENT_DATABASE;
    R_NOPORT = "noport", d::NOPORT;
    R_WINOPEN = "winopen", d::WINOPEN;
    R_WINCLOSE = "winclose", d::WINCLOSE;
    R_EXIT = "exit", d::EXIT;
    R_UDF_ALLOCATE = "SI_udfAllocate", d::SI_UDF_ALLOCATE;
    R_UDF_INITIALIZE = "SI_udfInitialize", d::SI_UDF_INITIALIZE;
    R_UDF_DISPOSE = "SI_udfDispose", d::SI_UDF_DISPOSE;
    R_READ_MAGIC = "SI_dbFileReadMagic", d::SI_DB_FILE_READ_MAGIC;
    R_CLUSTER_BINARY = "SI_clusterAnimBinaryRead", d::SI_CLUSTER_ANIM_BINARY_READ;
    R_CLUSTER_ASCII = "SI_oclusterAsciiRead", d::SI_OCLUSTER_ASCII_READ;
    R_SETUP_LOAD = "SI_setupFileLoadPath", d::SI_SETUP_FILE_LOAD_PATH;
    R_CHECK_EMPTY_DESC = "SI_elemdictCheckEmptyDescription", 0x0813_6db0;
    R_SCN_LIST_NODE = "SI_dbScnGetListNode", 0x0813_19f0;
    R_ADD_ELEM2 = "SI_elemdictAddElem", 0x0813_2a5c;
    R_GET_ELEM2 = "SI_elemdictGetElem", 0x0813_2b50;
    R_LOAD_ELEMENTS = "SI_dbLoadElementsFromDict", 0x080d_9924;
    R_CHN_SETUP = "SI_chnSetupGet", d::SI_CHN_SETUP_GET;
    R_PARTICLE_SETUP = "SI_particleSetupGet", d::SI_PARTICLE_SETUP_GET;
    R_FOPEN2 = "fopen", d::FOPEN;
    R_FCLOSE2 = "fclose", d::FCLOSE;
    R_TOKEN_READ = "SI_fileTokenRead", d::SI_FILE_TOKEN_READ;
    R_TOKEN_UPPER = "token2Uppercase", d::TOKEN2UPPERCASE;
    R_READ_ONE_ATTRIBUTE = "reldictReadOneAttribute", d::RELDICT_READ_ONE_ATTRIBUTE;
    R_OAT_NAME = "lookForExistingOatName", d::LOOK_FOR_EXISTING_OAT_NAME;
    R_FCURVE_NAME = "lookForExistingFcurveName", d::LOOK_FOR_EXISTING_FCURVE_NAME;
    R_SEARCH_FCV = "searchAttdictByFcvName", d::SEARCH_ATTDICT_BY_FCV_NAME;
    R_GROUP_BY_NAME = "SI_attdictAttribGroupGetByName", d::ATTDICT_ATTRIB_GROUP_GET_BY_NAME;
    R_ADD_ATTRIBUTE = "si_attdictAddAttribute", d::ATTDICT_ADD_ATTRIBUTE;
    R_ADD_RELATION = "SI_reldictAddRelation", d::RELDICT_ADD_RELATION;
    R_STATUS_MESSAGE = "SI_statusDisplayMessage", d::STATUS_DISPLAY_MESSAGE;
    R_GET_GROUP = "attdictGetAttribGroup", d::ATTDICT_GET_ATTRIB_GROUP;
    R_UPDATE_INDEXES = "SI_attdictUpdateIndexes", d::ATTDICT_UPDATE_INDEXES;
    R_OP_RESULT = "reldictReadOneOpResult", d::RELDICT_READ_ONE_OP_RESULT;
    R_OP_ARGUMENT = "reldictReadOneOpArgument", d::RELDICT_READ_ONE_OP_ARGUMENT;
    R_OP_DEFINITION = "reldictReadOpDefinition", d::RELDICT_READ_OP_DEFINITION;
    R_PARSE_OPTIONS = "SI_cusParseOperatorOptions", d::CUS_PARSE_OPERATOR_OPTIONS;
    R_ARG_NAME = "lookForExistingArgName", d::LOOK_FOR_EXISTING_ARG_NAME;
    R_RES_NAME = "lookForExistingResName", d::LOOK_FOR_EXISTING_RES_NAME;
    R_WRITE_ELEMENTS = "dbWriteChapterElements", d::DB_WRITE_CHAPTER_ELEMENTS;
    R_WRITE_RELATIONS = "dbWriteChapterRelations", d::DB_WRITE_CHAPTER_RELATIONS;
    R_CHN_INIT = "SI_chnInitialisation", d::SI_CHN_INITIALISATION;
    R_CHN_DISPOSE = "si_chnDispose", d::SI_CHN_DISPOSE;
    R_CHN_ALLOCATE = "si_chnAllocate", d::SI_CHN_ALLOCATE;
    R_CHN_ASCII_READ = "si_chnAsciiRead", d::SI_CHN_ASCII_READ;
    R_FSCANF = "fscanf", d::FSCANF;
    R_NODE_ASCII_READ = "clusterAnimNodeAsciiRead", d::CLUSTER_ANIM_NODE_ASCII_READ;
    R_ONE_OPERATOR_ATTRIBUTE = "reldictReadOneOperatorAttribute", d::RELDICT_READ_ONE_OPERATOR_ATTRIBUTE;
}

/// The tokens `SI_fileTokenRead`'s stand-in puts into the token buffer (the generator's table).
const TOKENS: [&str; 16] = [
    "OPERATOR", "EndOfOPERATOR", "EndOfOPERATORS", "RES", "NEWRES", "ARG", "OAT", "_END", "res", "arg", "oat", "newres", "Foo", "end", "endofoperator",
    "EndOfOPERATORX",
];
/// The binary's own messages lie in this range (logged by address, a built message by its checksum).
const ROD: std::ops::Range<u32> = 0x0868_4000..0x0868_5200;
/// The words `fscanf`'s stand-in puts into the buffer of the ascii cluster reader (the generator's table).
const CLUSTER_WORDS: [&str; 6] = ["Foo", "CLUS", "Cluster_node", "Cluster_nodeX", "", "clus"];
const RES_CAP: i16 = 4;
const ARG_CAP: i16 = 3;

/// An operator definition (ten words) the way the generator logs it: the pad halves of words 4, 7 and 9 masked.
fn def_log(m: &Mem, p: u32) -> Vec<u32> {
    (0..10u32).map(|k| if [4, 7, 9].contains(&k) { m.u32(p + 4 * k) & 0xffff } else { m.u32(p + 4 * k) }).collect()
}

/// The count of a result or argument array grows by one (to its capacity) after a line reader that answered 1.
fn count_effect(rec: &Recorded, m: &mut Mem, r: u32, def: u32, field: u32, cap: i16, name: &str) {
    if r & 0xffff != 1 {
        return;
    }
    let n = m.i16(def + field + 4);
    m.set_u32(def + field, rec.names.addr(m, name).unwrap_or(0));
    if n < cap {
        m.set_i16(def + field + 4, n + 1);
    }
}

/// CRC-32 of a text (the generator's `zlib.crc32`).
pub fn crc32(text: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in text {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The made-up stream `fopen` answers for a path (`path_word` of emu_gen_read.py).
pub fn path_word(text: &[u8]) -> u32 {
    0x6f00_0000 | (crc32(text) & 0xfff) << 8
}

/// Answers the call of a routine of this file's list as the generator's stand-in does, or
/// `None` when `f` is not one.
pub fn call(rec: &mut Recorded, m: &mut Mem, f: u32, args: &[u32]) -> Option<u32> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    let routine = CALLEES.iter().copied().find(|r| r.addr == f)?;
    let a = |k: usize| args.get(k).copied().unwrap_or(0);
    let logged: Vec<u32> = match f {
        d::SI_ED_ELEMENT_STATIC_VALUE_SET => {
            // the local {kind, value} in place of its address, then its second word last
            vec![a(0), a(1), a(2), a(3), m.u32(a(4)), a(5), a(6), m.u32(a(4) + 4)]
        }
        d::SI_CONSLIST_FIND_PREV_CONSTRAINT => {
            // the stand-in sets the short the caller's local holds: 1 for a non-zero priority
            let found = (a(1) != 0) as u16;
            let r = Outside::call(rec, m, routine, &[a(0), a(1), a(2), a(3), a(4), 0]);
            m.set_u16(a(5), found);
            return Some(r);
        }
        d::SI_DB_LOAD_RELATION_DICTIONARY => vec![a(0), a(1), crc32(&m.cstr(a(2))), a(3)],
        d::FOPEN => {
            let text = m.cstr(a(0));
            return Some(Outside::call(rec, m, routine, &[crc32(&text), a(1)]));
        }
        d::SI_DB_FILE_READ_MAGIC => {
            // the stand-in writes the magic (a function of the stream) and a version
            let r = Outside::call(rec, m, routine, &[a(0)]);
            m.set_u16(a(1), ((a(0) >> 8) & 1) as u16);
            m.set_u32(a(2), 0x3fc0_0000);
            return Some(r);
        }
        d::SI_FILE_TOKEN_READ => {
            let r = Outside::call(rec, m, routine, &[a(0)]);
            let mut buf = [0u8; 0x40];
            let text = TOKENS[(r >> 16) as usize % TOKENS.len()].as_bytes();
            buf[..text.len()].copy_from_slice(text);
            m.set_bytes(d::SI_DB_IN_TOKEN, &buf);
            return Some(r);
        }
        d::FSCANF => {
            let r = Outside::call(rec, m, routine, &[a(0), a(1)]);
            if a(1) == d::S_CLUSTER_FMT_FLOAT {
                m.set_u32(a(2), 0x3f80_0000);
            } else {
                m.put_cstr(a(2), CLUSTER_WORDS[r as usize % CLUSTER_WORDS.len()].as_bytes());
            }
            return Some(r);
        }
        d::TOKEN2UPPERCASE => {
            let r = Outside::call(rec, m, routine, &[a(0), a(1)]);
            let up = m.cstr(a(0)).to_ascii_uppercase();
            m.put_cstr(a(1), &up);
            return Some(r);
        }
        d::RELDICT_READ_ONE_ATTRIBUTE => {
            let attr = a(3);
            let r = Outside::call(rec, m, routine, &[a(0), a(1), a(2), m.u32(attr), m.u32(attr + 4) & 0xffff]);
            let t = (r & 7) as i16;
            let name = rec.names.addr(m, &format!("nm{}", t % 4)).unwrap_or(0);
            m.set_u32(attr + 0x1c, 1000 + t as u32);
            m.set_u32(attr + 0x20, name);
            m.set_i16(attr + 0x4c, t * 3 - 4);
            m.set_i16(attr + 0x4e, 50 + t);
            m.set_u32(attr + 0x50, (t % 4) as u32);
            m.set_i16(attr + 0x54, t & 1);
            m.set_i16(attr + 0x60, -t);
            return Some(r);
        }
        d::ATTDICT_ADD_ATTRIBUTE => {
            let p = a(2);
            let w = |o: u32| m.u32(p + o);
            vec![a(0), a(1), w(0), w(4) & 0xffff, w(0x1c), w(0x20), w(0x4c) & 0xffff, (w(0x4c) >> 16) & 0xffff, w(0x50), w(0x54) & 0xffff, w(0x60) & 0xffff]
        }
        d::RELDICT_ADD_RELATION => {
            let mut v: Vec<u32> = (0..13).map(a).collect();
            let p = a(13);
            if p != 0 {
                v.extend([0, 4, 8, 0xc, 0x10, 0x14, 0x18].map(|o| m.u32(p + o)));
                v.extend([0x1c, 0x1e, 0x20, 0x22, 0x24].map(|o| m.u32(p + o) & 0xffff));
            } else {
                v.push(0);
            }
            v.extend([a(14), a(15)]);
            v
        }
        d::STATUS_DISPLAY_MESSAGE => vec![if ROD.contains(&a(0)) { a(0) } else { crc32(&m.cstr(a(0))) }, a(1)],
        d::RELDICT_READ_ONE_OP_RESULT | d::RELDICT_READ_ONE_OP_ARGUMENT => {
            let mut v = vec![a(0), a(1), a(2), a(3)];
            v.extend(def_log(m, a(4)));
            let r = Outside::call(rec, m, routine, &v);
            if f == d::RELDICT_READ_ONE_OP_RESULT {
                count_effect(rec, m, r, a(4), 0x18, RES_CAP, "resarr");
            } else {
                count_effect(rec, m, r, a(4), 0x20, ARG_CAP, "argarr");
            }
            return Some(r);
        }
        d::RELDICT_READ_OP_DEFINITION => {
            let r = Outside::call(rec, m, routine, &[a(0)]);
            let def = a(1);
            let nm1 = rec.names.addr(m, "nm1").unwrap_or(0);
            m.set_u32(def, 100 + (r & 7));
            m.set_u32(def + 4, nm1);
            m.set_i16(def + 8, 3);
            m.set_i16(def + 0xa, -2);
            m.set_u32(def + 0xc, 7);
            m.set_i16(def + 0x10, 2);
            m.set_u32(def + 0x14, (r >> 3) & 3);
            return Some(r);
        }
        d::CUS_PARSE_OPERATOR_OPTIONS => {
            let r = Outside::call(rec, m, routine, &[a(0), a(1), m.u32(a(2))]);
            m.set_u32(a(2), r >> 16);
            return Some(r);
        }
        d::LOOK_FOR_EXISTING_ARG_NAME | d::LOOK_FOR_EXISTING_RES_NAME => {
            let mut v = def_log(m, a(0));
            v.extend((1..if f == d::LOOK_FOR_EXISTING_ARG_NAME { 4 } else { 6 }).map(a));
            v
        }
        _ => args.to_vec(),
    };
    Some(Outside::call(rec, m, routine, &logged))
}
