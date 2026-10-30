// SPDX-License-Identifier: MIT
//! The rest of the dictionary files' routines that read or write the memory image and call out
//! by address: `dictin.c` (`SI_elemdictMapStructKey`, `SI_elemdictReplacePrecision`),
//! `physicalio.c` (`SI_elemdictStoreRelationData`, `elemdictRelDataAllocate`, the
//! `SI_elemdictRead*` wrappers) and `init_dict.c` (`SI_initDictionaries`), with
//! `SI_dbLoadRelationDictionary` (`reldict.c`). Checked in `dict2-emucheck`
//! (`re/tools/si-scene/emu_gen_dict2.py`, finding 0026, section "2026-10-28, the rest").
//!
//! Each routine takes the image and the original's arguments. A routine of another source
//! file is reached by address through [`ByAddr`]; the text the original prints with
//! `fprintf(stderr, ...)` goes to [`Outside::stderr`]. `SI_secureLicAlloc`, the counter
//! the original bumps before each allocation, is not touched (as everywhere else).

use crate::edtools::ByAddr;
use si_dict::globals::{SI_CID, SI_ELEM_DICTS, SI_NB_ELEM_DICTS, SI_PROC_DATA, SI_RD};
use si_dict::outside::Outside;
use si_mem::{Mem, Ptr32};
use si_types::dict::{ARelation, Elem, ElemDict};

/// A routine of the dictionaries' neighbours that these functions call: (name, address).
pub const SI_ED_ELEMENT_STATIC_VALUE_SET: u32 = 0x080f_06e4;
/// `fcvAccess.270` (0x086830a4), a record of the binary's data handed to the setter.
pub const FCV_ACCESS_270: u32 = 0x0868_30a4;
pub const SI_DB_SET_CHAPTER: u32 = 0x0812_2654;
pub const SI_DBFILEPATH_BUILD: u32 = 0x0812_26cc;
pub const SI_DBFILENAME_CHANGE_PRECISION: u32 = 0x0812_6394;
pub const SI_DBFILE_FIND_SUBSTITUTION_FILE: u32 = 0x0812_5be8;
pub const SI_DB_FORMAT_FULL_NAME: u32 = 0x0812_23dc;
pub const SI_ELEMDICT_READ_MODEL: u32 = 0x0810_fdd4;
pub const SI_CONSLIST_FIND_PRIORITY: u32 = 0x0849_57cc;
pub const SI_CONSLIST_FIND_PREV_CONSTRAINT: u32 = 0x0849_585c;
pub const SI_REL_DICT_READ_RELATIONS: u32 = 0x0813_e318;
pub const SI_ATTDICT_SET_FCTS_POINTERS: u32 = 0x0811_f370;
pub const SI_ERROR: u32 = 0x0814_17d0;
pub const SI_THREAD_STORAGE_INIT: u32 = 0x080d_0f80;
pub const SI_REL_DICT_INIT: u32 = 0x0813_9db0;
pub const SI_ELEMDICT_ADD: u32 = 0x0813_7b00;
pub const SI_ELEMDICT_INIT: u32 = 0x0813_59bc;
pub const SI_ELEMDICT_LOAD_CHAPTERS: u32 = 0x0813_3cf0;
pub const SI_ELEMDICT_CREATE_DEFAULT_DSC: u32 = 0x0813_69cc;
pub const SI_DB_LOAD_RELATION_DICTIONARY: u32 = 0x0813_ea84;
pub const SI_DB_DATABASE_DIR_PREPARE: u32 = 0x0812_158c;
pub const SI_DATABASE_DIRECTORY_LOAD: u32 = 0x0810_8130;
pub const SI_DB_SET_CURRENT_DATABASE: u32 = 0x0812_25dc;
pub const NOPORT: u32 = 0x0861_9840;
pub const WINOPEN: u32 = 0x0861_93e0;
pub const WINCLOSE: u32 = 0x0861_946c;
pub const EXIT: u32 = 0x080c_4b58;

/// Globals of `init_dict.c`: `SI_cwd` (a pointer to the working directory), `SI_dbdir`,
/// `SI_pDbiin`, `SI_pDbiout`, `SI_standaloneRenderer` (16 bits), the local buffer's size.
const SI_CWD: u32 = 0x087f_ba8c;
const SI_DBDIR: u32 = 0x0878_1390;
const SI_P_DBIIN: u32 = 0x0878_2058;
const SI_P_DBIOUT: u32 = 0x0878_205c;
const SI_STANDALONE_RENDERER: u32 = 0x0867_9400;
const SI_DB_WORK_FNAME: u32 = 0x0878_8140;
const SI_DB: u32 = 0x0878_13a0; // SI_dbed
const SI_DBCID: u32 = 0x0878_1700;
const STR_FMT_RSRC1: &[u8] = b"/Relations.rsrc";

/// `%s` of a possibly null string.
fn s(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

/// Both ways out: calls by address and the text printed on standard error.
pub trait World: ByAddr + Outside {}
impl<T: ByAddr + Outside + ?Sized> World for T {}

/// [soft 0x08128a9c SI_elemdictMapStructKey]: writes the static values a hierarchy animation
/// holds, for the entries that are selected and of type 1, onto the elements they animate;
/// answers how many it wrote. `elem` is the hierarchy-animation element (its `hier_level`
/// must be 0 and its data the animation `{count, elements, chapter}`); with `p7` set and a
/// single entry, the target of every value is `other` instead of the entry's own element.
pub fn map_struct_key<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, _ed: Ptr32<ElemDict>, _rd: u32, elem: Ptr32<Elem>, key: i32, other: u32, p6: i16, p7: i16) -> i32 {
    let mut written = 0;
    let anim = m[elem].data.addr();
    if m[elem].hier_level != 0 || anim == 0 {
        return 0;
    }
    let mut i = 0;
    let mut n = m.i32(anim);
    while i < n {
        let e = m.u32(anim + 4).wrapping_add((i as u32).wrapping_mul(0x28));
        if m.i32(e + 8) > 0 {
            let target;
            if p7 == 0 || n != 1 {
                target = m.u32(e + 0x24);
                if target == 0 {
                    i += 1;
                    n = m.i32(anim);
                    continue;
                }
            } else {
                target = other;
            }
            let aux = m.u32(target + 8);
            if aux == 0 || p6 == 0 || m.u32(aux + 8) != 0 {
                let mut j = 0u32;
                while (j as i32) < m.i32(e + 8) {
                    if m.i16(m.u32(e + 0x1c).wrapping_add(j * 2)) != 0 && m.i16(m.u32(e + 0xc).wrapping_add(j * 2)) == 1 {
                        let vp = m.u32(m.u32(e + 0x18).wrapping_add(j * 4));
                        // `SI_AttributeValue local = { 1, vp[1] }`, a local of the frame
                        let local = m.local(8);
                        m.set_u32(local, 1);
                        let second = m.u32(vp + 4);
                        m.set_u32(local + 4, second);
                        let chapter = m.u32(anim + 8);
                        let id = m.u32(vp);
                        call(o, m, SI_ED_ELEMENT_STATIC_VALUE_SET, &[target, chapter, id, FCV_ACCESS_270, local, key as u32, 0]);
                        m.release(8);
                        written += 1;
                    }
                    j += 1;
                }
            }
        }
        i += 1;
        n = m.i32(anim);
    }
    written
}

/// [soft 0x081292e0 SI_elemdictReplacePrecision]: for a model element, steps the precision
/// of its file name (`SI_dbfilenameChangePrecision`), looks for the substitution file, and if
/// there is one rewrites the name record's substitution string and full name, reads the
/// model file and marks the dictionary dirty; otherwise puts the precision back. Answers the
/// model read, or 0.
pub fn replace_precision<O: World + ?Sized>(m: &mut Mem, o: &mut O, db: u32, ed: Ptr32<ElemDict>, elem: u32, chap: i32, p6: i16) -> u32 {
    let chapter_name = m.u32(m[ed].lists.addr().wrapping_add((chap as u32).wrapping_mul(0x40)));
    call(o, m, SI_DB_SET_CHAPTER, &[db, chapter_name]);
    call(o, m, SI_DBFILEPATH_BUILD, &[db]);
    if chap == m[SI_CID].model {
        let changed = call(o, m, SI_DBFILENAME_CHANGE_PRECISION, &[elem, p6 as i32 as u32]) as u16;
        if changed != 0 {
            let found = call(o, m, SI_DBFILE_FIND_SUBSTITUTION_FILE, &[db, elem]) as i32;
            if found >= 0 {
                let node = m.u32(elem);
                let old = m.u32(node + 0x18);
                if old != 0 {
                    m.free(old);
                    m.set_u32(m.u32(elem) + 0x18, 0);
                }
                let text = found.to_string().into_bytes();
                let copy = m.malloc(text.len() as u32 + 1);
                m.put_cstr(copy, &text);
                m.set_u32(m.u32(elem) + 0x18, copy);
                let full = call(o, m, SI_DB_FORMAT_FULL_NAME, &[m.u32(elem)]);
                m.set_u32(m.u32(elem) + 0x24, full);
                let node = m.u32(elem);
                let path = [s(m, m.u32(db + 0x14)), s(m, m.u32(node + 8)), s(m, m.u32(node + 0x24))].concat();
                m.put_cstr(SI_DB_WORK_FNAME, &path);
                let mut model = 0;
                if chap == m[SI_CID].model {
                    model = call(o, m, SI_ELEMDICT_READ_MODEL, &[SI_DB_WORK_FNAME]);
                }
                // (the original tests a local that only the branch above sets)
                if model != 0 {
                    m.set_u32(ed.addr() + 0x14, 1);
                    m.set_i16(ed.addr() + 0x6e, 1);
                    return model;
                }
            }
            call(o, m, SI_DBFILENAME_CHANGE_PRECISION, &[elem, (p6 == 0) as u32]);
        }
    }
    0
}

/// [soft 0x08111d58 elemdictRelDataAllocate]: room for the data of a relation of kind `id`
/// (a zeroed array of 7, 9, 16 or 1 floats; `*count` is its length), or 0 for a kind that
/// has none.
pub fn rel_data_allocate(m: &mut Mem, id: i32, count: u32) -> u32 {
    let n = if id == 0xf5 {
        return 0;
    } else if id < 0xf6 {
        if id < 0xde {
            if id > 0xdb {
                7
            } else {
                return 0;
            }
        } else if id > 0xe5 && id < 0xe8 {
            9
        } else {
            return 0;
        }
    } else if id > 0x101 {
        if id == 800 {
            0x10
        } else if id == 0x327 {
            1
        } else {
            return 0;
        }
    } else {
        return 0;
    };
    m.set_i32(count, n);
    m.calloc(n as u32, 4)
}

/// A float copied through the x87 stack: a signalling NaN comes out quiet.
fn fld_fst(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// [soft 0x08111dec SI_elemdictStoreRelationData]: fills the data of a relation of the MODELS
/// chapter from the model the element stands for: the 7 floats of a transformation
/// (ids 0xdc, 0xdd), the 9 of a pivot record (0xe6, 0xe7), the 16 of a model's render flags
/// (800) and the one of a constraint's priority (0x327). Answers 1, or 0 where the original
/// leaves the data allocated and unfilled.
pub fn store_relation_data<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, chap: i32, elem: Ptr32<Elem>, rel: Ptr32<ARelation>) -> i32 {
    let mut model = m[elem].data.addr();
    let id = m[rel].relid;
    if id == 0x4e2a || id == 0xf7 {
        return 1;
    }
    if chap != m[SI_CID].model {
        return 0;
    }
    let count = m.local(12);
    let data = rel_data_allocate(m, id, count);
    m.release(12);
    m.set_u32(rel.addr() + 0x18, data);
    if data == 0 {
        return 0;
    }
    let f = |m: &mut Mem, k: u32, from: u32| {
        let v = fld_fst(m.u32(from));
        m.set_u32(data + k * 4, v);
    };
    let sh = |m: &mut Mem, k: u32, from: u32| {
        let v = (m.i16(from) as f32).to_bits();
        m.set_u32(data + k * 4, v);
    };
    if id < 0xe8 {
        if id < 0xe6 {
            if id < 0xde && id > 0xdb {
                let p = if id == 0xdc { m.u32(model + 0x60) } else { m.u32(model + 100) };
                for k in 0..7 {
                    f(m, k, p + 0xc + k * 4);
                }
            }
        } else {
            let p = if id == 0xe6 { m.u32(model + 0x2d8) } else { m.u32(model + 0x2dc) };
            for k in 0..9 {
                f(m, k, p + 8 + k * 4);
            }
        }
    } else if id == 800 {
        let mut p = m.u32(model + 0x304);
        if m.i32(model + 0x30) == 0 {
            let mut kind = m.i32(model + 0x2ec);
            if (kind.wrapping_sub(3) as u32) < 2 {
                while (kind.wrapping_sub(1) as u32) > 1 {
                    loop {
                        model = m.u32(model + 700);
                        if m.i32(model + 0x30) == 0 {
                            break;
                        }
                    }
                    kind = m.i32(model + 0x2ec);
                }
                p = m.u32(model + 0x304);
            }
        }
        if p == 0 {
            return 0;
        }
        for (k, off) in [0x38, 0x42, 0x44, 0x46, 0x4e, 0x3a, 0x48, 0x4a, 0x4c, 0x50, 0x3e].into_iter().enumerate() {
            sh(m, k as u32, p + off);
        }
        let v = (m.i32(p + 0x54) as f32).to_bits();
        m.set_u32(data + 0xb * 4, v);
        sh(m, 0xc, p + 0x40);
        let v = (m.i32(p + 0x58) as f32).to_bits();
        m.set_u32(data + 0xd * 4, v);
        sh(m, 0xe, p + 0x3c);
        sh(m, 0xf, p + 0x5e);
    } else if id == 0x327 {
        let prio = call(o, m, SI_CONSLIST_FIND_PRIORITY, &[9]) as i16;
        let proc = m[SI_PROC_DATA];
        let dict = m[SI_ELEM_DICTS].addr().wrapping_add((m[proc] as u32).wrapping_mul(0x70));
        let lists = m.u32(dict + 4);
        let chapid = m.i16(rel.addr() + 0x10) as i32;
        let index = m.u32(rel.addr() + 4);
        let target = m.u32(lists.wrapping_add(8).wrapping_add((chapid as u32).wrapping_mul(0x40))).wrapping_add(index.wrapping_mul(0x84));
        if target == 0 {
            return 0;
        }
        let obj = m.u32(target + 4);
        if obj == 0 {
            return 0;
        }
        let found = m.local(4);
        let prev = call(o, m, SI_CONSLIST_FIND_PREV_CONSTRAINT, &[model, prio as i32 as u32, 9, obj, 1, found]);
        let flag = m.i16(found);
        m.release(4);
        if flag != 0 {
            let c = if prev == 0 { m.u32(model + 0x2fc) } else { m.u32(prev + 0x3c) };
            let q = m.u32(c + 0x38);
            if q != 0 {
                sh(m, 0, q + 0x78);
            }
        }
    }
    1
}

/// [soft 0x0813ea84 SI_dbLoadRelationDictionary]: reads the relation resource file into the
/// dictionary (`SI_reldictReadRelations`); if that fails and `report` is set, `SI_error(30,
/// "dbLoadRelationDictionary...", path)`; then the attribute groups get their functions
/// (`SI_attdictSetFctsPointers(&SI_ATD)`). Answers the reader's 16-bit answer.
pub fn db_load_relation_dictionary<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, ed: u32, rd: u32, path: u32, report: i16) -> i32 {
    let read = call(o, m, SI_REL_DICT_READ_RELATIONS, &[ed, rd, path]) as i16;
    if read == 0 && report != 0 {
        call(o, m, SI_ERROR, &[0x1e, 0x0868_50e7, path]);
    }
    call(o, m, SI_ATTDICT_SET_FCTS_POINTERS, &[si_dict::globals::SI_ATD.addr()]);
    read as i32
}

/// The local text buffer of `SI_initDictionaries` (1024 bytes) holds `<base>/Relations.rsrc`.
fn rsrc_path(m: &mut Mem, buf: u32, base: u32) {
    let text = [s(m, base), STR_FMT_RSRC1.to_vec()].concat();
    m.put_cstr(buf, &text);
}

/// [soft 0x0810824c SI_initDictionaries]: the relation dictionary is emptied, the thread
/// storage initialised, the scene dictionary added (its index the first word of the process
/// data), its chapters loaded into `SI_cid` and the default scene made; `Relations.rsrc` is
/// read from the working directory, else from `resources`, else the program stops (a window
/// is opened and closed to tell, or a message printed); the database browser's dictionary
/// gets its chapters; and, with `load_dirs` set, the list of databases is read and the
/// current one chosen.
pub fn init_dictionaries<O: World + ?Sized>(m: &mut Mem, o: &mut O, resources: u32, version: i16, load_dirs: i16) {
    let ed_of = |m: &Mem| {
        let proc = m[SI_PROC_DATA];
        m[SI_ELEM_DICTS].addr().wrapping_add((m[proc] as u32).wrapping_mul(0x70))
    };
    call(o, m, SI_REL_DICT_INIT, &[SI_RD.addr()]);
    m[SI_NB_ELEM_DICTS] = 0;
    if call(o, m, SI_THREAD_STORAGE_INIT, &[0]) != 0 {
        Outside::stderr(o, b"ERROR: Couldn't initialise thread storage.\n");
    }
    let index = call(o, m, SI_ELEMDICT_ADD, &[]);
    let proc = m[SI_PROC_DATA];
    m.set_u32(proc.addr(), index);
    m.set_u32(proc.addr() + 4, 0);
    let ed = m[SI_ELEM_DICTS].addr().wrapping_add(index.wrapping_mul(0x70));
    call(o, m, SI_ELEMDICT_INIT, &[ed]);
    let ed = ed_of(m);
    call(o, m, SI_ELEMDICT_LOAD_CHAPTERS, &[ed, SI_CID.addr(), 1]);
    let ed = ed_of(m);
    call(o, m, SI_ELEMDICT_CREATE_DEFAULT_DSC, &[ed]);
    let buf = m.local(0x400);
    let cwd = m.u32(SI_CWD);
    rsrc_path(m, buf, cwd);
    let ed = ed_of(m);
    let mut ok = call(o, m, SI_DB_LOAD_RELATION_DICTIONARY, &[ed, SI_RD.addr(), buf, 0]) as i16;
    if ok == 0 {
        rsrc_path(m, buf, resources);
        let ed = ed_of(m);
        ok = call(o, m, SI_DB_LOAD_RELATION_DICTIONARY, &[ed, SI_RD.addr(), buf, 1]) as i16;
        if ok == 0 {
            if m.i16(SI_STANDALONE_RENDERER) == 0 {
                call(o, m, NOPORT, &[]);
                let w = call(o, m, WINOPEN, &[0x0868_0db6]);
                call(o, m, WINCLOSE, &[w]);
            } else {
                let text = [b"Cannot read Relation resources in ".to_vec(), m.cstr(buf), b"\n".to_vec()].concat();
                Outside::stderr(o, &text);
            }
            m.release(0x400);
            call(o, m, EXIT, &[0xffff_ffff]);
            // the original does not come back from exit
            return;
        }
    }
    m.release(0x400);
    call(o, m, SI_ELEMDICT_INIT, &[SI_DB]);
    call(o, m, SI_ELEMDICT_LOAD_CHAPTERS, &[SI_DB, SI_DBCID, 0]);
    call(o, m, SI_DB_DATABASE_DIR_PREPARE, &[SI_DBDIR]);
    if load_dirs != 0 {
        call(o, m, SI_DATABASE_DIRECTORY_LOAD, &[SI_DBDIR, resources, version as i32 as u32]);
        let cur = call(o, m, SI_DB_SET_CURRENT_DATABASE, &[SI_DBDIR]);
        m.set_u32(SI_P_DBIOUT, cur);
        m.set_u32(SI_P_DBIIN, cur);
        if m.i16(SI_STANDALONE_RENDERER) != 0 && cur == 0 {
            Outside::stderr(o, b"ERROR: No valid Database where the scene could be found\n");
        }
    }
}

/// `SI_udfAllocate (0x0854c2f4)`, `SI_udfInitialize (0x08544370)` and `SI_udfDispose
/// (0x0854c3c0)`, `SI_dbFileReadMagic (0x08126994)`, `SI_clusterAnimBinaryRead
/// (0x083f1d20)`, `SI_oclusterAsciiRead (0x083ddf10)`, `SI_setupFileLoadPath (0x083be024)`,
/// `SI_chnSetupGet (0x08568ab4)`, `SI_particleSetupGet (0x084c03d0)`, the C library's
/// `fopen` and `fclose`.
pub const SI_UDF_ALLOCATE: u32 = 0x0854_c2f4;
pub const SI_UDF_INITIALIZE: u32 = 0x0854_4370;
pub const SI_UDF_DISPOSE: u32 = 0x0854_c3c0;
pub const SI_DB_FILE_READ_MAGIC: u32 = 0x0812_6994;
pub const SI_CLUSTER_ANIM_BINARY_READ: u32 = 0x083f_1d20;
pub const SI_OCLUSTER_ASCII_READ: u32 = 0x083d_df10;
pub const SI_SETUP_FILE_LOAD_PATH: u32 = 0x083b_e024;
pub const SI_CHN_SETUP_GET: u32 = 0x0856_8ab4;
pub const SI_PARTICLE_SETUP_GET: u32 = 0x084c_03d0;
pub const FOPEN: u32 = 0x080c_4e18;
pub const FCLOSE: u32 = 0x080c_4958;
const SI_PIPED_FILES: u32 = 0x086c_eba8;
const SI_PIPE: u32 = 0x086c_ebac;
/// The mode string of `SI_elemdictReadClusters`' `fopen` (0x086811cb).
const S_CLS_MODE: u32 = 0x0868_11cb;

/// [soft 0x0810ff38 SI_elemdictReadShader]: a new shader record (`SI_udfAllocate`) set up
/// from the shader file (`SI_udfInitialize(udf, file, name, kind)`); disposed and answered
/// null when that fails.
pub fn elemdict_read_shader<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, name: u32, file: u32, kind: i32) -> u32 {
    let udf = call(o, m, SI_UDF_ALLOCATE, &[]);
    let done = call(o, m, SI_UDF_INITIALIZE, &[udf, file, name, kind as u32]) as i16;
    if done == 0 {
        call(o, m, SI_UDF_DISPOSE, &[udf]);
        return 0;
    }
    udf
}

/// [soft 0x08110144 SI_elemdictReadClusters]: the clusters of a model from `<base>.cls`: the
/// file (or the pipe) is opened to read its magic (`SI_dbFileReadMagic`), closed unless it is
/// the pipe, and the binary reader or, for an ascii file, the ascii reader fills the model.
/// Answers the reader's 16-bit answer, 0 when the file does not open.
pub fn elemdict_read_clusters<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, base: u32, model: u32) -> i32 {
    const FRAME: u32 = 0x41c;
    let frame = m.local(FRAME);
    let path = frame + 0x1c;
    let text = [s(m, base), b".cls".to_vec()].concat();
    // the original's frame has room for 1024 bytes: a longer name runs on over what follows
    m.put_cstr(path, &text[..text.len().min(0x3ff)]);
    let mut file = m.u32(SI_PIPE);
    if m.i16(SI_PIPED_FILES) == 0 {
        file = call(o, m, FOPEN, &[path, S_CLS_MODE]);
    }
    let answer;
    if file == 0 {
        answer = 0;
    } else {
        let (magic, version) = (frame + 0x16, frame + 0x18);
        call(o, m, SI_DB_FILE_READ_MAGIC, &[file, magic, version]);
        if m.i16(SI_PIPED_FILES) == 0 {
            call(o, m, FCLOSE, &[file]);
        }
        let kind = m.i16(magic);
        let r = if kind != 0 { call(o, m, SI_OCLUSTER_ASCII_READ, &[base, model]) } else { call(o, m, SI_CLUSTER_ANIM_BINARY_READ, &[base, model]) };
        answer = r as i16 as i32;
    }
    m.release(FRAME);
    answer
}

/// [soft 0x08111b00 SI_elemdictReadSetup]: `SI_setupFileLoadPath(path)`.
pub fn elemdict_read_setup<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, path: u32) {
    call(o, m, SI_SETUP_FILE_LOAD_PATH, &[path]);
}

/// [soft 0x08111b34 SI_elemdictReadChannelSetup]: `SI_chnSetupGet(path)`.
pub fn elemdict_read_channel_setup<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, path: u32) {
    call(o, m, SI_CHN_SETUP_GET, &[path]);
}

/// [soft 0x08111b64 SI_elemdictReadParticleSetup]: `SI_particleSetupGet(path)`.
pub fn elemdict_read_particle_setup<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, path: u32) {
    call(o, m, SI_PARTICLE_SETUP_GET, &[path]);
}

/// `ByAddr::call_addr` spelled out (the checks' outside world is also an [`Outside`], whose
/// own `call_addr` would be ambiguous).
fn call<O: ByAddr + ?Sized>(o: &mut O, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
    ByAddr::call_addr(o, m, f, args)
}

/// `SI_convertPatch` (0x080dcb54): not ported; `fixPatch` hands it the patch, the control
/// point's number in the padded grid, the padded grid, its point count and sizes, and the
/// source point's number.
pub const SI_CONVERT_PATCH: u32 = 0x080d_cb54;

/// [soft 0x08129430 fixPatch]: a patch whose grid sides (the 16-bit counts at +0xc and +0xe) are
/// not multiples of 3 is padded to a grid two larger each way: the points (16 bytes, at
/// +0x1a4) and the flags (2 bytes, at +0x1a8) are copied into the padded grid, every third
/// point of it (both indices multiples of 3) is rebuilt by `SI_convertPatch` from the
/// source point's number, and the old arrays are replaced by new ones holding the padded
/// grid (only the first three words of a point are copied; the fourth keeps the fill of
/// `SI_malloc`). The count of the licence scheme is not reproduced. A patch with a side
/// of 0 would divide by zero in the original; not looked at here.
pub fn fix_patch<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, patch: u32) {
    let cols = m.u16(patch + 0xe) as u32;
    let rows = m.u16(patch + 0xc) as u32;
    if cols == (cols / 3) * 3 && rows == (rows / 3) * 3 {
        return;
    }
    let u8_ = rows + 2;
    let u6 = cols + 2;
    let n = (u6 as i32).wrapping_mul(u8_ as i32);
    let n1 = (n as u32).wrapping_add(1);
    let pts = m.si_calloc(n1.wrapping_mul(0x10), 0x10);
    let flags = m.si_calloc(n1.wrapping_mul(2), 2);
    let idx = m.si_malloc((u8_ / 3).wrapping_mul(u6 / 3).wrapping_mul(4).wrapping_add(4));
    let third = |i: i32| {
        let q = i / u6 as i32;
        let r = i % u6 as i32;
        q == (q / 3) * 3 && r == (r / 3) * 3
    };
    if n > 0 {
        let (mut off, mut dst, mut src, mut count) = (0u32, 0i32, 0u32, 0u32);
        loop {
            let base = m.u32(patch + 0x1a4);
            for k in 0..4 {
                let v = m.u32(base.wrapping_add(src.wrapping_mul(0x10)).wrapping_add(k * 4));
                m.set_u32(pts.wrapping_add(off).wrapping_add(k * 4), v);
            }
            let f = m.u16(m.u32(patch + 0x1a8).wrapping_add(src.wrapping_mul(2)));
            m.set_u16(flags.wrapping_add((dst as u32).wrapping_mul(2)), f);
            if third(dst) {
                m.set_u32(idx.wrapping_add(count * 4), src);
                count += 1;
            }
            if src % cols == cols.wrapping_sub(1) {
                off = off.wrapping_add(0x20);
                dst += 2;
            }
            if src == (cols * rows).wrapping_sub(1) {
                break;
            }
            src += 1;
            off = off.wrapping_add(0x10);
            dst += 1;
            if dst >= n {
                break;
            }
        }
    }
    let mut taken = 0u32;
    for i in 0..n.max(0) {
        if third(i) {
            let from = m.u32(idx.wrapping_add(taken * 4));
            call(o, m, SI_CONVERT_PATCH, &[patch, i as u32, pts, n as u32, u8_, u6, from]);
            taken += 1;
        }
    }
    for off in [0x1a4, 0x1a8] {
        let p = m.u32(patch + off);
        if p != 0 {
            m.free(p);
            m.set_u32(patch + off, 0);
        }
    }
    let np = m.si_malloc(n1.wrapping_mul(0x10));
    m.set_u32(patch + 0x1a4, np);
    let nf = m.si_malloc(n1.wrapping_mul(2));
    m.set_u32(patch + 0x1a8, nf);
    for i in 0..n.max(0) as u32 {
        for k in 0..3 {
            let v = m.u32(pts + i * 0x10 + k * 4);
            m.set_u32(np + i * 0x10 + k * 4, v);
        }
        let f = m.u16(flags + i * 2);
        m.set_u16(nf + i * 2, f);
    }
    m.set_u16(patch + 0xe, u6 as u16);
    m.set_u16(patch + 0xc, u8_ as u16);
    for p in [pts, flags, idx] {
        if p != 0 {
            m.free(p);
        }
    }
}

// ---------------------------------------------------------------------------------------
// The readers of the relation operators (`reldict.c`): the `OPERATOR` blocks of
// `Relations.rsrc` and the `_OPERATOR` section of a custom-effect file. Every routine of
// another file, and the file-token reader with the token buffer, is reached by address.

pub const SI_DB_IN_TOKEN: u32 = 0x0878_0f40;
pub const SI_FILE_TOKEN_READ: u32 = 0x0840_4298;
pub const TOKEN2UPPERCASE: u32 = 0x0813_fd88;
pub const RELDICT_READ_ONE_ATTRIBUTE: u32 = 0x0813_bbd8;
pub const LOOK_FOR_EXISTING_OAT_NAME: u32 = 0x0813_fbe0;
pub const LOOK_FOR_EXISTING_FCURVE_NAME: u32 = 0x0813_fc58;
pub const SEARCH_ATTDICT_BY_FCV_NAME: u32 = 0x0813_fd20;
pub const ATTDICT_ATTRIB_GROUP_GET_BY_NAME: u32 = 0x0811_edf8;
pub const ATTDICT_ADD_ATTRIBUTE: u32 = 0x0811_e984;
pub const RELDICT_ADD_RELATION: u32 = 0x0813_9b80;
pub const STATUS_DISPLAY_MESSAGE: u32 = 0x083c_ca94;
pub const ATTDICT_GET_ATTRIB_GROUP: u32 = 0x0813_d098;
pub const ATTDICT_UPDATE_INDEXES: u32 = 0x0811_fb3c;
pub const RELDICT_READ_ONE_OP_RESULT: u32 = 0x0813_c588;
pub const RELDICT_READ_ONE_OP_ARGUMENT: u32 = 0x0813_c63c;
pub const RELDICT_READ_OP_DEFINITION: u32 = 0x0813_ca90;
pub const CUS_PARSE_OPERATOR_OPTIONS: u32 = 0x0852_a3ac;
pub const LOOK_FOR_EXISTING_ARG_NAME: u32 = 0x0813_fb48;
pub const LOOK_FOR_EXISTING_RES_NAME: u32 = 0x0813_fa74;
pub const RELDICT_READ_ONE_OPERATOR_ATTRIBUTE: u32 = 0x0813_d12c;

/// `SI_OperDef` (40 bytes) as the words a call by value pushes. Three of the ten words end
/// in a pad of two bytes (+0x12, +0x1e, +0x26) that the original never writes: they are
/// passed as zero here (the frame of the original holds whatever was there).
fn def_words(m: &Mem, def: u32) -> [u32; 10] {
    let mut w = [0u32; 10];
    for (k, x) in w.iter_mut().enumerate() {
        *x = m.u32(def + 4 * k as u32);
    }
    for k in [4, 7, 9] {
        w[k] &= 0xffff;
    }
    w
}

/// A copy of a C string in a new `SI_malloc` block (the original measures with an inlined
/// `strlen`, allocates `length + 1` and calls `strcpy`); 0 when the allocation fails.
fn dup_cstr(m: &mut Mem, src: u32) -> u32 {
    let text = m.cstr(src);
    let p = m.si_malloc(text.len() as u32 + 1);
    if p != 0 {
        m.put_cstr(p, &text);
    }
    p
}

/// [soft 0x0813d12c reldictReadOneOperatorAttribute]: one `OAT` line of an operator. The
/// attribute record is a local of 100 bytes whose owner kind is 1 and owner id the low half
/// of the operator definition's id; `reldictReadOneAttribute` fills it. With `cus` 1 (a
/// custom effect) a name the group already has is refused (`SI_error(4, ..)`, a status
/// message, answer 0) and, with `SI_CUSTOM_VERIFY` set, a name that a standard fcurve has
/// is reported. The attribute is added to the dictionary and, with `cus` 0, one relation from the
/// chapter of the operator's first result (or of the last argument, if that is in SHAPES,
/// CLUSTERS or LATTICES) to ANIMATION. `def` is the operator definition passed by value,
/// `arg` the last argument read. Answers 1.
pub fn reldict_read_one_operator_attribute<O: World + ?Sized>(
    m: &mut Mem, o: &mut O, rd: u32, atd: u32, group: u32, file: u32, cus: i16, custom: u32, def: &[u32; 10], arg: u32,
) -> i32 {
    let attr = m.local(100);
    let answer = one_operator_attribute(m, o, attr, rd, atd, group, file, cus, custom, def, arg);
    m.release(100);
    answer
}

#[allow(clippy::too_many_arguments)]
fn one_operator_attribute<O: World + ?Sized>(
    m: &mut Mem, o: &mut O, attr: u32, rd: u32, atd: u32, group: u32, file: u32, cus: i16, custom: u32, def: &[u32; 10], arg: u32,
) -> i32 {
    m.set_u32(attr, 1);
    m.set_i16(attr + 4, def[0] as i16);
    call(o, m, RELDICT_READ_ONE_ATTRIBUTE, &[rd, file, cus as i32 as u32, attr]); // its answer is not looked at
    if cus == 1 {
        let name = m.u32(attr + 0x20);
        if group != 0 && call(o, m, LOOK_FOR_EXISTING_OAT_NAME, &[atd, group, name]) as i16 == 1 {
            call(o, m, SI_ERROR, &[4, 0x0868_49e0]);
            call(o, m, STATUS_DISPLAY_MESSAGE, &[0x0868_4a40, 3]);
            return 0;
        }
        if Outside::getenv(o, m, b"SI_CUSTOM_VERIFY") != 0 {
            let general = call(o, m, ATTDICT_ATTRIB_GROUP_GET_BY_NAME, &[atd, 0x0868_4a92]);
            let mut known = call(o, m, LOOK_FOR_EXISTING_FCURVE_NAME, &[atd, general, name]) as i16 != 0;
            if !known {
                let models = call(o, m, ATTDICT_ATTRIB_GROUP_GET_BY_NAME, &[atd, 0x0868_4aa2]);
                known = call(o, m, SEARCH_ATTDICT_BY_FCV_NAME, &[atd, models, name]) as i16 != 0;
            }
            if known {
                let (n1, effect) = (m.cstr(name), m.cstr(m.u32(custom + 4)));
                let buf = m.si_malloc((effect.len() + n1.len()) as u32 + 0x46);
                if buf != 0 {
                    let text = [n1, b" fcurve of custom effect ".to_vec(), effect, b" has same name as a pre-defined S|3D fcurve.".to_vec()].concat();
                    m.put_cstr(buf, &text);
                    call(o, m, STATUS_DISPLAY_MESSAGE, &[buf, 2]);
                    m.free(buf);
                }
            }
        }
    }
    call(o, m, ATTDICT_ADD_ATTRIBUTE, &[atd, group, attr]);
    if cus == 0 {
        // the chapter of the first result, unless the last argument's is SHAPES, CLUSTERS or LATTICES
        let mut from = m.i16(def[6].wrapping_add(8)) as i32;
        let last = m.i16(arg.wrapping_add(8)) as i32;
        if last == m[SI_CID].shp || last == m[SI_CID].cls || last == m[SI_CID].lat {
            from = last;
        }
        let animation = m[SI_CID].ani as u32;
        let s = |m: &Mem, at: u32| m.i16(at) as i32 as u32;
        let args = [
            rd, m.u32(attr + 0x20), m.u32(attr + 0x1c), from as u32, animation, s(m, attr + 0x4c), 0, s(m, attr + 0x4e), s(m, attr + 0x54), s(m, attr + 0x60),
            0, 0, 0, 0, 0, m.u32(attr + 0x50),
        ];
        call(o, m, RELDICT_ADD_RELATION, &args);
    }
    1
}

/// [soft 0x0813d39c SI_reldictReadOperatorAttributes]: the `OPERATOR` section of a custom-effect
/// file (`.cus`), seen as the attributes of one operator whose definition is made up (id 20000,
/// the effect's name): from the current token on, every `OAT` line is read with
/// `reldictReadOneOperatorAttribute` (the group of the first is looked up once) until one of
/// `OPERATOR`, `EndOfOPERATORS`, `EndOfOPERATOR`, `_END` or the end of the file; then the
/// attribute indexes are renewed. Answers the last token read's answer (0 at the end of the
/// file), 0 when an allocation fails or an attribute is refused. The two copies of the
/// effect's names are never freed.
pub fn reldict_read_operator_attributes<O: World + ?Sized>(m: &mut Mem, o: &mut O, rd: u32, atd: u32, file: u32, custom: u32) -> i32 {
    let def = m.local(40);
    let answer = operator_attributes(m, o, def, rd, atd, file, custom);
    m.release(40);
    answer
}

const MEMORY_ERROR_A: u32 = 0x0868_4b20;
const MEMORY_ERROR_B: u32 = 0x0868_4b80;

fn operator_attributes<O: World + ?Sized>(m: &mut Mem, o: &mut O, def: u32, rd: u32, atd: u32, file: u32, custom: u32) -> i32 {
    let tok = SI_DB_IN_TOKEN;
    m.set_i16(def + 0x1c, 0);
    m.set_i16(def + 0x24, 0);
    m.set_u32(def + 0x18, 0);
    m.set_u32(def + 0x20, 0);
    let mut name1 = 0;
    if custom != 0 {
        name1 = dup_cstr(m, m.u32(custom + 4));
        if name1 == 0 {
            call(o, m, SI_ERROR, &[0x46, MEMORY_ERROR_A]);
            return 0;
        }
    }
    let mut name2 = 0;
    if custom != 0 && m.u32(custom + 0x114) != 0 {
        name2 = dup_cstr(m, m.u32(custom + 0x114).wrapping_add(2));
        if name2 == 0 {
            call(o, m, SI_ERROR, &[0x46, MEMORY_ERROR_B]);
            return 0;
        }
    }
    m.set_u32(def, 20000);
    m.set_u32(def + 4, name1);
    m.set_i16(def + 8, 0);
    m.set_u32(def + 0xc, 0);
    m.set_i16(def + 0xa, 0);
    m.set_i16(def + 0x10, 0);
    m.set_u32(def + 0x14, 3);
    let mut need_group = true;
    let mut group = 0u32;
    let mut bx: i16 = 1;
    call(o, m, SI_FILE_TOKEN_READ, &[file]); // (its answer is not looked at)
    call(o, m, TOKEN2UPPERCASE, &[tok, tok]);
    loop {
        if bx == 0 {
            break;
        }
        let t = m.cstr(tok);
        if t == b"OPERATOR" || t == b"EndOfOPERATORS" || t == b"EndOfOPERATOR" || t == b"_END" {
            break;
        }
        if t == b"OAT" {
            let w = def_words(m, def);
            if need_group {
                let mut args = vec![atd];
                args.extend_from_slice(&w);
                args.extend_from_slice(&[1, name2]);
                group = call(o, m, ATTDICT_GET_ATTRIB_GROUP, &args);
                need_group = false;
            }
            let w = def_words(m, def);
            let mut args = vec![rd, atd, group, file, 1, custom];
            args.extend_from_slice(&w);
            args.push(0);
            if call(o, m, RELDICT_READ_ONE_OPERATOR_ATTRIBUTE, &args) as i16 == 0 {
                return 0;
            }
        }
        loop {
            bx = call(o, m, SI_FILE_TOKEN_READ, &[file]) as i16;
            let t = m.cstr(tok);
            if t != b"EndOfOPERATOR" && t != b"EndOfOPERATORS" {
                call(o, m, TOKEN2UPPERCASE, &[tok, tok]);
            }
            if bx == 0 {
                break;
            }
            let t = m.cstr(tok);
            if [&b"OPERATOR"[..], b"EndOfOPERATOR", b"EndOfOPERATORS", b"OAT", b"_END"].contains(&&t[..]) {
                break;
            }
        }
    }
    call(o, m, ATTDICT_UPDATE_INDEXES, &[si_dict::globals::SI_ATD.addr()]);
    bx as i32
}

/// [soft 0x0813d75c SI_reldictReadOneOperator]: one `OPERATOR` block, from the token after its
/// keyword (`cus` 0: the standard resource file, the definition line read by
/// `reldictReadOpDefinition`) or one custom effect's `_OPERATOR` section (`cus` 1: the
/// definition made up, the `OPTIONS` line parsed, names checked for repeats and one relation
/// added for each pair of a result and an argument). `RES`, `NEWRES`, `ARG` and `OAT` lines
/// are read by the routines of that name, everything else is passed over to the next
/// of these words or the end of the block. Answers the last token read's answer; 0 for a
/// failure (after the message: `SI_error(4, ..)` and, for a repeated name, a status
/// message too). The result and argument arrays the line readers made are freed at the end
/// of a block that ran to its end (not on a failure); the two copies of the effect's names are
/// never freed.
pub fn reldict_read_one_operator<O: World + ?Sized>(m: &mut Mem, o: &mut O, ed: u32, rd: u32, atd: u32, file: u32, cus: i16, custom: u32) -> i32 {
    let frame = m.local(40 + 4 + 0x28);
    let (def, opts, info) = (frame, frame + 40, frame + 44);
    let answer = one_operator(m, o, (def, opts, info), ed, rd, atd, file, cus, custom);
    m.release(40 + 4 + 0x28);
    answer
}

#[allow(clippy::too_many_arguments)]
fn one_operator<O: World + ?Sized>(m: &mut Mem, o: &mut O, locals: (u32, u32, u32), ed: u32, rd: u32, atd: u32, file: u32, cus: i16, custom: u32) -> i32 {
    let (def, opts, info) = locals;
    let tok = SI_DB_IN_TOKEN;
    let s = |m: &Mem, at: u32| m.i16(at) as i32 as u32;
    m.set_u32(opts, 0x32);
    let mut seen_arg = false;
    m.set_i16(def + 0x1c, 0);
    m.set_i16(def + 0x24, 0);
    m.set_u32(def + 0x18, 0);
    m.set_u32(def + 0x20, 0);
    let mut name1 = 0;
    if cus == 1 && custom != 0 {
        name1 = dup_cstr(m, m.u32(custom + 4));
        if name1 == 0 {
            call(o, m, SI_ERROR, &[0x46, 0x0868_4c60]);
            return 0;
        }
    }
    let mut name2 = 0;
    if cus == 1 && custom != 0 && m.u32(custom + 0x114) != 0 {
        name2 = dup_cstr(m, m.u32(custom + 0x114).wrapping_add(2));
        if name2 == 0 {
            call(o, m, SI_ERROR, &[0x46, 0x0868_4cc0]);
            return 0;
        }
    }
    if cus == 0 {
        call(o, m, RELDICT_READ_OP_DEFINITION, &[file, def]);
    } else {
        m.set_u32(def, 20000);
        m.set_u32(def + 4, name1);
        m.set_i16(def + 8, 0);
        m.set_u32(def + 0xc, 0);
        m.set_i16(def + 0xa, 0);
        m.set_i16(def + 0x10, 0);
        m.set_u32(def + 0x14, 3);
    }
    let mut need_group = true;
    let mut group = 0u32;
    let mut last_arg = 0u32; // (uninitialised until the first ARG line in the original)
    let mut bx: i16 = 1;
    if cus == 1 && call(o, m, CUS_PARSE_OPERATOR_OPTIONS, &[file, custom, opts]) as i16 == 0 {
        call(o, m, SI_ERROR, &[4, 0x0868_4d20]);
        return 0;
    }
    if cus != 0 {
        call(o, m, TOKEN2UPPERCASE, &[tok, tok]);
    }
    loop {
        if bx == 0 {
            break;
        }
        let t = m.cstr(tok);
        if t == b"OPERATOR" || t == b"EndOfOPERATORS" || t == b"EndOfOPERATOR" || (cus != 0 && t == b"_END") {
            break;
        }
        let new_res = cus != 0 && t == b"NEWRES";
        if t == b"RES" || new_res {
            if cus == 1 && seen_arg {
                call(o, m, SI_ERROR, &[4, 0x0868_4da0]);
                return 0;
            }
            if call(o, m, RELDICT_READ_ONE_OP_RESULT, &[ed, rd, file, cus as i32 as u32, def]) as i16 != 1 {
                call(o, m, SI_ERROR, &[4, 0x0868_4e00]);
                return 0;
            }
            if cus == 1 {
                let last = m.u32(def + 0x18).wrapping_add((s(m, def + 0x1c)).wrapping_mul(32)).wrapping_sub(32);
                let (name, chapter, grp) = (m.u32(last + 4), s(m, last + 8), s(m, last + 0xa));
                if call(o, m, LOOK_FOR_EXISTING_RES_NAME, &[def, name, chapter, grp, new_res as u32, 0]) as i16 == 1 {
                    call(o, m, SI_ERROR, &[4, 0x0868_4e40]);
                    call(o, m, STATUS_DISPLAY_MESSAGE, &[0x0868_4e80, 3]);
                    return 0;
                }
            }
            let last = m.u32(def + 0x18).wrapping_add((s(m, def + 0x1c)).wrapping_mul(32)).wrapping_sub(32);
            m.set_i16(last + 0x18, new_res as i16);
        } else if t == b"ARG" {
            seen_arg = true;
            if call(o, m, RELDICT_READ_ONE_OP_ARGUMENT, &[ed, rd, file, cus as i32 as u32, def]) as i16 != 1 {
                call(o, m, SI_ERROR, &[4, 0x0868_4ee0]);
                return 0;
            }
            let a = m.u32(def + 0x20).wrapping_add(s(m, def + 0x24).wrapping_mul(40)).wrapping_sub(40);
            last_arg = a;
            let (name, chapter, grp) = (m.u32(a + 4), s(m, a + 8), s(m, a + 0xa));
            if cus == 1 {
                if call(o, m, LOOK_FOR_EXISTING_ARG_NAME, &[def, name, chapter, grp]) as i16 != 0
                    || call(o, m, LOOK_FOR_EXISTING_RES_NAME, &[def, name, chapter, grp, 0, 1]) as i16 != 0
                {
                    call(o, m, SI_ERROR, &[4, 0x0868_4f20]);
                    call(o, m, STATUS_DISPLAY_MESSAGE, &[0x0868_4f60, 3]);
                    return 0;
                }
                m.set_u32(info, name2);
                m.set_u32(info + 8, m.u32(a + 0x10));
                m.set_u32(info + 0x10, m.u32(a + 4));
                m.set_u32(info + 0x18, m.u32(a + 0x24));
                m.set_u16(info + 0x24, m.u16(a + 0x14));
                m.set_u16(info + 0x20, m.u16(a + 0x22));
                let mut i = 0u32;
                if m.i16(def + 0x1c) > 0 {
                    loop {
                        let r = m.u32(def + 0x18).wrapping_add(i.wrapping_mul(32));
                        m.set_u32(info + 4, m.u32(r + 0x10));
                        m.set_u32(info + 0xc, m.u32(r + 4));
                        m.set_u32(info + 0x14, m.u32(r + 0x1c));
                        m.set_u16(info + 0x22, m.u16(r + 0x14));
                        m.set_u16(info + 0x1c, m.u16(r + 0x18));
                        m.set_u16(info + 0x1e, m.u16(r + 0x16));
                        let args = [
                            rd, m.u32(def + 4), m.u32(def), s(m, r + 8), s(m, a + 8), s(m, r + 0xa), s(m, a + 0xa), m.u32(opts), m.u32(a + 0x18),
                            s(m, a + 0x1c), s(m, a + 0x1e), s(m, a + 0x20), 1, info, s(m, def + 0x10), m.u32(def + 0x14),
                        ];
                        call(o, m, RELDICT_ADD_RELATION, &args);
                        i += 1;
                        if (i as i32) >= m.i16(def + 0x1c) as i32 {
                            break;
                        }
                    }
                }
            } else {
                let r = m.u32(def + 0x18);
                let args = [
                    rd, m.u32(def + 4), m.u32(def), s(m, r + 8), s(m, a + 8), s(m, r + 0xa), s(m, a + 0xa), s(m, r + 0xc), m.u32(a + 0x18), s(m, a + 0x1c),
                    s(m, a + 0x1e), s(m, a + 0x20), 0, 0, s(m, def + 0x10), m.u32(def + 0x14),
                ];
                call(o, m, RELDICT_ADD_RELATION, &args);
            }
        } else if t == b"OAT" {
            if need_group {
                let mut args = vec![atd];
                args.extend_from_slice(&def_words(m, def));
                args.extend_from_slice(&[cus as i32 as u32, name2]);
                group = call(o, m, ATTDICT_GET_ATTRIB_GROUP, &args);
                need_group = false;
            }
            let mut args = vec![rd, atd, group, file, cus as i32 as u32, custom];
            args.extend_from_slice(&def_words(m, def));
            args.push(last_arg);
            if call(o, m, RELDICT_READ_ONE_OPERATOR_ATTRIBUTE, &args) as i16 == 0 {
                return 0;
            }
        }
        // the next of the words that start a line
        loop {
            bx = call(o, m, SI_FILE_TOKEN_READ, &[file]) as i16;
            let t = m.cstr(tok);
            if cus != 0 && t != b"EndOfOPERATOR" && t != b"EndOfOPERATORS" {
                call(o, m, TOKEN2UPPERCASE, &[tok, tok]);
            }
            if bx == 0 {
                break;
            }
            let t = m.cstr(tok);
            let stops: &[&[u8]] = &[b"OPERATOR", b"EndOfOPERATOR", b"EndOfOPERATORS", b"RES", b"ARG", b"OAT"];
            if stops.contains(&&t[..]) || (cus != 0 && (t == b"NEWRES" || t == b"_END")) {
                break;
            }
        }
    }
    if cus == 1 {
        call(o, m, ATTDICT_UPDATE_INDEXES, &[si_dict::globals::SI_ATD.addr()]);
    }
    let results = m.u32(def + 0x18);
    if results != 0 {
        m.free(results);
        m.set_u32(def + 0x18, 0);
    }
    let args = m.u32(def + 0x20);
    if args != 0 {
        m.free(args);
    }
    bx as i32
}

// ---------------------------------------------------------------------------------------
// The description-file dump (`dictout_scn.c`).

pub const DB_WRITE_CHAPTER_ELEMENTS: u32 = 0x0812_f1bc;
pub const DB_WRITE_CHAPTER_RELATIONS: u32 = 0x0812_f7b4;
/// The `stdout` slot of the C library and the dictionary writers' global `file`.
pub const STDOUT_SLOT: u32 = 0x0878_0c84;
pub const DB_FILE: u32 = 0x0878_76a4;

/// The chapter-id globals (`SI_cid`) by address: the blocks of the RELATIONS section, as (from, to) pairs in the order written.
const MODELS: u32 = 0x0878_1680;
const CHAPTER_PAIRS: [(u32, u32); 37] = [
    (MODELS, MODELS), (MODELS, 0x0878_16b4), (MODELS, 0x0878_1668), (MODELS, 0x0878_1674), (MODELS, 0x0878_167c), (MODELS, 0x0878_16c8),
    (MODELS, 0x0878_16cc), (MODELS, 0x0878_16d0), (MODELS, 0x0878_1664), (MODELS, 0x0878_1678), (MODELS, 0x0878_1660),
    (0x0878_1664, 0x0878_1664), (0x0878_1664, MODELS), (0x0878_1664, 0x0878_1660),
    (0x0878_1678, 0x0878_1678), (0x0878_1678, MODELS), (0x0878_1678, 0x0878_1660),
    (0x0878_16d0, MODELS), (0x0878_16d0, 0x0878_1660),
    (0x0878_167c, 0x0878_16c8), (0x0878_167c, 0x0878_16cc), (0x0878_167c, 0x0878_1660),
    (0x0878_166c, 0x0878_1660), (0x0878_1674, 0x0878_1660), (0x0878_16b4, 0x0878_1660), (0x0878_1668, 0x0878_1660),
    (0x0878_16c8, 0x0878_1660), (0x0878_16cc, 0x0878_1660),
    (0x0878_1684, MODELS), (0x0878_1684, 0x0878_1664), (0x0878_1684, 0x0878_1678), (0x0878_1684, 0x0878_16d0), (0x0878_1684, 0x0878_167c),
    (0x0878_1684, 0x0878_16c8), (0x0878_1684, 0x0878_16cc), (0x0878_1684, 0x0878_1668), (0x0878_1684, 0x0878_1660),
];

/// [soft 0x08131344 SI_dbDescriptionFileDump]: the ELEMENTS and, with `relations` set, the
/// RELATIONS sections of a description file on the standard output (the global `file` is set to
/// it). The ELEMENTS section is the chapter `chapter` (every chapter when it is -1) written by
/// `dbWriteChapterElements`; the RELATIONS section is the 37 blocks of `dbWriteChapterRelations`
/// for the pairs of chapters of the table above, each with 1, `precision` and 1.
pub fn db_description_file_dump<O: World + ?Sized>(m: &mut Mem, o: &mut O, ed: u32, rd: u32, chapter: i32, precision: i16, relations: i16) {
    let p = precision as i32 as u32;
    let stdout = m.u32(STDOUT_SLOT);
    m.set_u32(DB_FILE, stdout);
    o.stdout(b"ELEMENTS ");
    o.stdout(b"\n");
    if chapter == -1 {
        let mut i = 0i32;
        while i < m.i32(ed) {
            call(o, m, DB_WRITE_CHAPTER_ELEMENTS, &[ed, i as u32, p]);
            i += 1;
        }
    } else {
        call(o, m, DB_WRITE_CHAPTER_ELEMENTS, &[ed, chapter as u32, p]);
    }
    o.stdout(b"EndOfELEMENTS ");
    o.stdout(b"\n");
    o.stdout(b"\n");
    if relations != 0 {
        o.stdout(b"RELATIONS ");
        o.stdout(b"\n");
        for (from, to) in CHAPTER_PAIRS {
            let (from_id, to_id) = (m.u32(from), m.u32(to));
            call(o, m, DB_WRITE_CHAPTER_RELATIONS, &[ed, rd, chapter as u32, from_id, to_id, 1, p, 1]);
        }
        o.stdout(b"EndOfRELATIONS ");
        o.stdout(b"\n");
        o.stdout(b"\n");
    }
}

// ---------------------------------------------------------------------------------------
// The channel-setup and ascii cluster readers behind the physicalio wrappers.

pub const SI_CHN_INITIALISATION: u32 = 0x0856_89cc;
pub const SI_CHN_DISPOSE: u32 = 0x0855_9834;
pub const SI_CHN_ALLOCATE: u32 = 0x0855_8f0c;
pub const SI_CHN_ASCII_READ: u32 = 0x0857_2214;
/// `si_softChnTmp` (0x086ebc28) and `si_softChn` (0x08829998): the channel setup the program is using and
/// the one it was using before the last read.
pub const SI_SOFT_CHN_TMP: u32 = 0x086e_bc28;
pub const SI_SOFT_CHN: u32 = 0x0882_9998;

/// [soft 0x08568ab4 SI_chnSetupGet]: reads a channel-setup file. The channel module is
/// initialised, a channel set kept from the read before is disposed of, the current set
/// becomes the kept one and a new set of 4 (`si_chnAllocate`) is filled from `path` by
/// `si_chnAsciiRead(path, set, 1)`. Answers that routine's 16-bit answer.
pub fn chn_setup_get<O: ByAddr + ?Sized>(m: &mut Mem, o: &mut O, path: u32) -> i32 {
    call(o, m, SI_CHN_INITIALISATION, &[]);
    if m.u32(SI_SOFT_CHN_TMP) != 0 {
        call(o, m, SI_CHN_DISPOSE, &[SI_SOFT_CHN_TMP]);
    }
    let current = m.u32(SI_SOFT_CHN);
    m.set_u32(SI_SOFT_CHN_TMP, current);
    let set = call(o, m, SI_CHN_ALLOCATE, &[4]);
    m.set_u32(SI_SOFT_CHN, set);
    call(o, m, SI_CHN_ASCII_READ, &[path, set, 1]) as i16 as i32
}

pub const FSCANF: u32 = 0x080c_4058;
const S_ASCII_MODE: u32 = 0x086c_bdd9;
/// A world lent to a port that wants a plain `&mut dyn si_mem::Outside` (the world itself may be unsized): a call goes
/// to the world by its address, the answer is what it leaves in `eax` (every callee of the node reader answers there).
struct AsOutside<'a, O: ?Sized>(&'a mut O);
impl<O: World + ?Sized> si_mem::Outside for AsOutside<'_, O> {
    fn call(&mut self, m: &mut Mem, r: si_mem::Routine, args: &[u32]) -> si_mem::Ret {
        si_mem::Ret::eax(ByAddr::call_addr(self.0, m, r.addr, args))
    }
    fn print(&mut self, stream: u32, text: &[u8]) {
        if stream == 2 { Outside::stderr(self.0, text) } else { Outside::stdout(self.0, text) }
    }
}

pub const CLUSTER_ANIM_NODE_ASCII_READ: u32 = 0x083d_dc00;
/// The formats of the four `fscanf`s, the message and the words they read.
pub const S_CLUSTER_FMT_WORD: u32 = 0x086c_bddc;
pub const S_CLUSTER_FMT_FLOAT: u32 = 0x086c_be36;
pub const S_CLUSTER_FMT_NODE: u32 = 0x086c_be39;
pub const S_CLUSTER_FMT_NEXT: u32 = 0x086c_be49;

/// [soft 0x083ddf10 SI_oclusterAsciiRead]: the ascii cluster file `<base>.cls` (or the pipe):
/// its first word must be `CLUS` (otherwise the message "ClusterAsciiRead: Error file <base> is not
/// a cluster file" goes to the status line with level 3 and the answer is 0), then a number
/// (read and dropped), then a word; while that word is `Cluster_node` the node reader
/// (`clusterAnimNodeAsciiRead`, not ported) reads one node of the model and the next word is
/// read. The file is closed unless it is the pipe. Answers 1 (0 when nothing is opened). The
/// name is built in a buffer of 256 bytes, as in the original.
pub fn ocluster_ascii_read<O: World + ?Sized>(m: &mut Mem, o: &mut O, base: u32, model: u32) -> i32 {
    ocluster_ascii_read_with(m, o, base, model, true)
}

/// [`ocluster_ascii_read`] with the node reader left a call by address ([`CLUSTER_ANIM_NODE_ASCII_READ`]) to the world, as
/// the vectors of the original (which stood the node reader in) have it. With `routed` the node reader is the port of
/// `si-app` ([`si_app::image::clusterread::cluster_anim_node_ascii_read`]), run on this world (routing5: nothing here
/// reproduces a function of the original; `si-app` depends on `si-mem` and `si-x87` only, so there is no cycle).
pub fn ocluster_ascii_read_with<O: World + ?Sized>(m: &mut Mem, o: &mut O, base: u32, model: u32, routed: bool) -> i32 {
    if base == 0 {
        return 0;
    }
    const FRAME: u32 = 0x104;
    let frame = m.local(FRAME);
    let (value, buf) = (frame, frame + 4);
    let answer = ocluster_body(m, o, base, model, value, buf, routed);
    m.release(FRAME);
    answer
}

fn ocluster_body<O: World + ?Sized>(m: &mut Mem, o: &mut O, base: u32, model: u32, value: u32, buf: u32, routed: bool) -> i32 {
    let put = |m: &mut Mem, text: Vec<u8>| m.put_cstr(buf, &text[..text.len().min(0xff)]);
    let text = [m.cstr(base), b".cls".to_vec()].concat();
    put(m, text);
    let mut file = m.u32(SI_PIPE);
    if m.i16(SI_PIPED_FILES) == 0 {
        file = call(o, m, FOPEN, &[buf, S_ASCII_MODE]);
    }
    if file == 0 {
        return 0;
    }
    call(o, m, FSCANF, &[file, S_CLUSTER_FMT_WORD, buf]);
    if m.cstr(buf) != b"CLUS" {
        let text = [b"ClusterAsciiRead: Error file ".to_vec(), m.cstr(base), b" is not a cluster file".to_vec()].concat();
        put(m, text);
        call(o, m, STATUS_DISPLAY_MESSAGE, &[buf, 3]);
        if m.i16(SI_PIPED_FILES) == 0 {
            call(o, m, FCLOSE, &[file]);
        }
        return 0;
    }
    call(o, m, FSCANF, &[file, S_CLUSTER_FMT_FLOAT, value]);
    call(o, m, FSCANF, &[file, S_CLUSTER_FMT_NODE, buf]);
    if m.cstr(buf) == b"Cluster_node" {
        loop {
            if routed {
                si_app::image::clusterread::cluster_anim_node_ascii_read(m, &mut AsOutside(o), file, model);
            } else {
                call(o, m, CLUSTER_ANIM_NODE_ASCII_READ, &[file, model]);
            }
            call(o, m, FSCANF, &[file, S_CLUSTER_FMT_NEXT, buf]);
            if m.cstr(buf) != b"Cluster_node" {
                break;
            }
        }
    }
    if m.i16(SI_PIPED_FILES) == 0 {
        call(o, m, FCLOSE, &[file]);
    }
    1
}

#[cfg(test)]
mod routing5_tests {
    use super::*;
    use si_dict::outside::Routine;
    use std::collections::VecDeque;

    /// A world with a cluster file of words: `fopen` gives a handle, `fscanf` the next word (`%s`), float (`%g`) or
    /// number (`%d`), the cluster allocators a fresh record.
    struct W {
        words: VecDeque<&'static str>,
        calls: Vec<u32>,
    }
    impl ByAddr for W {
        fn call_addr(&mut self, m: &mut Mem, f: u32, a: &[u32]) -> u32 {
            self.calls.push(f);
            match f {
                FOPEN => 0x4000,
                FSCANF => {
                    let Some(w) = self.words.pop_front() else { return u32::MAX };
                    match m.cstr(a[1]).as_slice() {
                        b"%g" => m.set_f32(a[2], w.parse().unwrap()),
                        b"%d" => m.set_u32(a[2], w.parse().unwrap()),
                        _ => m.put_cstr(a[2], w.as_bytes()),
                    }
                    1
                }
                0x081e_7e40 => m.si_malloc(0x20),
                0x081e_7ecc => m.si_malloc(0x40),
                _ => 0,
            }
        }
    }
    impl Outside for W {
        fn call(&mut self, _m: &mut Mem, _r: &'static Routine, _a: &[u32]) -> u32 {
            0
        }
    }

    fn world(words: &[&'static str]) -> (Mem, W, u32, u32) {
        let mut m = Mem::new();
        for (a, t) in si_app::image::clusterread::TEXTS {
            m.set_bytes(*a, t);
        }
        for (a, t) in [(S_CLUSTER_FMT_WORD, &b"%s\0"[..]), (S_CLUSTER_FMT_FLOAT, b"%g\0"), (S_CLUSTER_FMT_NODE, b"%s\0"), (S_CLUSTER_FMT_NEXT, b"%s\0")] {
            m.set_bytes(a, t);
        }
        let base = m.si_malloc(16);
        m.put_cstr(base, b"scene");
        let model = m.si_malloc(0x400);
        m.set_u32(model + 0x30, 1);
        (m, W { words: words.iter().copied().collect(), calls: vec![] }, base, model)
    }

    /// The cluster file `CLUS 1.0 Cluster_node numKeys 3 End Done` read with the node reader of `si-app`: the model gets its
    /// cluster list (+0x344, count word 1), the node's record takes `numKeys` (+0x14), the answer is 1 and the file is
    /// closed; the node reader is not asked of the world (routed), where the hook variant asks it once.
    #[test]
    fn routed_node_reader_reads_a_node_and_is_not_a_hook() {
        let file = ["CLUS", "1.0", "Cluster_node", "numKeys", "3", "End", "Done"];
        let (mut m, mut w, base, model) = world(&file);
        assert_eq!(ocluster_ascii_read(&mut m, &mut w, base, model), 1);
        assert!(!w.calls.contains(&CLUSTER_ANIM_NODE_ASCII_READ));
        assert!(w.calls.contains(&FCLOSE));
        let list = m.u32(model + 0x344);
        assert_ne!(list, 0);
        assert_eq!(m.u32(list + 4), 1);
        assert!(w.words.is_empty());
        let (mut m, mut w, base, model) = world(&file);
        assert_eq!(ocluster_ascii_read_with(&mut m, &mut w, base, model, false), 1);
        assert_eq!(w.calls.iter().filter(|&&f| f == CLUSTER_ANIM_NODE_ASCII_READ).count(), 1);
    }
}
