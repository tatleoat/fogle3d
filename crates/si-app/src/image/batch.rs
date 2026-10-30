// SPDX-License-Identifier: MIT
//! The batch renderer on the image: `standAloneRender` and the routines of `sys_main.c` it
//! calls, `SI_callRender` (`server.c`), `SI_replicateCommandLine` (`emergencySave.c`), and
//! the start-up's path from `si_runCreative` to it (finding 0019, sections 2, 3.2 and 12).
//!
//! Every routine takes the image and the original's arguments (pointers as addresses in
//! the image) and an outside world for what is not on the image ([`super::calls`]). The
//! original's `exit(n)` is `Err(Exit(n))`; where the original would read through a null
//! pointer the port records a fault on the image and ends with [`FAULT`].

use super::calls::BatchCalls;
use super::g;
use super::globals_c::{animation_info_init, manipulation_init};
use crate::render_cmd::{atof, atoi, line_with_name, whole_integer, FAULT};
use crate::sysmain::id_string;
use crate::usage_text::{PRINT_USAGE, RENDER_USAGE};
use crate::Exit;
use si_mem::{Access, CStr32, Mem, Outside};
use si_rt::message as rt;
use si_x87::F80;

/// Offsets in `SI_defaultControl` (0x600 bytes) that the batch path reads or writes
/// (finding 0019, section 3.2).
pub mod ctl {
    /// The renderer: 0 wire frame, 1 and 2 hidden line, 4 native, 9 mental ray 1.9, 10
    /// mental ray 2.1 (a `short`).
    pub const RENDERER: u32 = 0x0fa;
    /// Set to 0 and 1 by `SI_callRender`.
    pub const WORD178: u32 = 0x178;
    pub const WORD17A: u32 = 0x17a;
    /// The output file name (a `char *`).
    pub const OUT_NAME: u32 = 0x17c;
    /// The pre-frame and post-frame scripts (`char *`).
    pub const PRE_SCRIPT: u32 = 0x180;
    pub const POST_SCRIPT: u32 = 0x184;
    /// The sequence: first, last, step (`short`s).
    pub const FIRST: u32 = 0x1f4;
    pub const LAST: u32 = 0x1f6;
    pub const STEP: u32 = 0x1f8;
    /// Skip pictures that exist (`-j`).
    pub const SKIP_EXISTING: u32 = 0x20e;
    /// A sub-region (`-b`): the switch and four floats (left, one minus right, one minus
    /// top, bottom).
    pub const SUBREGION: u32 = 0x226;
    pub const SUB_LEFT: u32 = 0x22c;
    pub const SUB_RIGHT_INV: u32 = 0x230;
    pub const SUB_TOP_INV: u32 = 0x234;
    pub const SUB_BOTTOM: u32 = 0x238;
    /// Complete unfinished frames (`-c`), the depth channel (`-z`).
    pub const COMPLETE: u32 = 0x244;
    pub const Z_CHANNEL: u32 = 0x24c;
    /// A `.mi` file instead of a render (`-m`, `-M`), one per frame, its name.
    pub const MI_OUTPUT: u32 = 0x3d8;
    pub const MI_PER_FRAME: u32 = 0x3da;
    pub const MI_NAME: u32 = 0x494;
    /// The five global switches of `-G` (`short`s).
    pub const SWITCHES: u32 = 0x586;
    /// The picture's size (`-r`).
    pub const XRES: u32 = 0x59a;
    pub const YRES: u32 = 0x59c;
}

const CTL: u32 = g::SI_DEFAULT_CONTROL.0;
/// `SI_rsrcPath` (0x086794b0) points at this text: what `getDefaultPath` appends.
const RSRC_PATH: &[u8] = b"../rsrc";

fn err(o: &mut dyn Outside, text: &[u8]) {
    o.print(2, text);
}

fn out(o: &mut dyn Outside, text: &[u8]) {
    o.print(1, text);
}

/// A string argument of `printf`: glibc prints `(null)` for a null pointer.
fn text(m: &Mem, p: u32) -> Vec<u8> {
    if p == 0 {
        b"(null)".to_vec()
    } else {
        m.cstr(p)
    }
}

/// `strdup(p)`: the C library reads through `p`; null faults.
fn strdup(m: &mut Mem, p: u32) -> Result<u32, Exit> {
    if p == 0 {
        m.record_fault(0, 1, Access::Read);
        return Err(FAULT);
    }
    Ok(m.strdup(p))
}

/// The first byte of the string at `p` (the original's `cmp BYTE PTR [eax], '-'`); a null
/// pointer faults.
fn first(m: &Mem, p: u32) -> Result<u8, Exit> {
    if p == 0 {
        m.record_fault(0, 1, Access::Read);
        return Err(FAULT);
    }
    Ok(m.u8(p))
}

/// `strtol(p, NULL, 10)` stored in a `short`.
fn atoi16(m: &Mem, p: u32) -> Result<i16, Exit> {
    first(m, p)?;
    Ok(atoi(&m.cstr(p)) as i16)
}

/// `atof(p)` stored in a `float` (`fstp DWORD`: the double rounded once).
fn atof32(m: &Mem, p: u32) -> Result<f32, Exit> {
    first(m, p)?;
    Ok(atof(&m.cstr(p)) as f32)
}

/// `argv[k]`, `argv` being the global of `standAloneRender`.
fn arg(m: &Mem, k: u32) -> u32 {
    m.u32(m[g::ARGV] + 4 * k)
}

/// `argv += 4` and the new `*argv`.
pub(super) fn next_arg(m: &mut Mem) -> u32 {
    let a = m[g::ARGV] + 4;
    m[g::ARGV] = a;
    m.u32(a)
}

/// What `getcwd(NULL, size)` of glibc 2.2 does, on the image: a block of `size` bytes from
/// `malloc` (of `PATH_MAX` = 4096 when `size` is 0) holding the directory; null when the
/// directory does not fit (`ERANGE`; the block is freed). With a buffer the directory is
/// written into it. For an outside world that answers [`BatchCalls::getcwd`]; nothing of
/// the original is reproduced here.
pub fn libc_getcwd(m: &mut Mem, buf: u32, size: u32, dir: &[u8]) -> u32 {
    if buf != 0 {
        if (dir.len() as u32) < size {
            m.put_cstr(buf, dir);
            return buf;
        }
        return 0;
    }
    let n = if size == 0 { 4096 } else { size };
    let p = m.malloc(n);
    if p == 0 {
        return 0;
    }
    if dir.len() as u32 >= n {
        m.free(p);
        return 0;
    }
    m.put_cstr(p, dir);
    p
}

// ---------------------------------------------------------------------------------------
// sys_main.c: the printed texts

/// [soft 0x080ca910 SI_standDisplayVersion]: the banner on the standard error stream: a
/// blank line, `name` and `version` (`4.0` when null), `extra` (when not null), and the
/// copyright line with the years `from` (1990 when 0) to `to` (this year when 0: `time`
/// and `localtime`; 1993 when the clock fails; a year below 1900 counted from 1900).
pub fn stand_display_version(m: &mut Mem, o: &mut dyn Outside, name: CStr32, extra: CStr32, version: CStr32, from: i32, to: i32) {
    let v = if version.is_null() { b"4.0".to_vec() } else { m.cstr(version.0) };
    err(o, &[b"\n".as_slice(), &text(m, name.0), b" ", &v].concat());
    if extra.is_null() {
        err(o, b"\n");
    } else {
        err(o, &[m.cstr(extra.0), b"\n".to_vec()].concat());
    }
    let from = if from == 0 { 0x7c6 } else { from };
    let mut to = to;
    if to == 0 {
        // time(NULL), kept in a local whose address localtime is given
        let local = m.local(4);
        let t = o.time(m, 0);
        m.set_i32(local, t);
        if t == -1 {
            to = 0x7c9;
        } else {
            let tm = o.localtime(m, local);
            to = m.i32(tm + 0x14);
            if to <= 0x76b {
                to += 0x76c;
            }
        }
        m.release(4);
    }
    err(o, b"(c) ");
    if from == to {
        err(o, from.to_string().as_bytes());
    } else {
        err(o, format!("{from}-{to}").as_bytes());
    }
    err(o, rt(0x086796e0));
}

/// [soft 0x080cdafc usage]: the batch renderer's help on the standard error stream (the
/// banner with the years from 1986, then 60 lines, `%s` the program's name) and the end of
/// the program with status `code`.
pub fn usage(m: &mut Mem, o: &mut dyn Outside, code: i32, name: CStr32) -> Exit {
    stand_display_version(m, o, name, CStr32::NULL, CStr32::NULL, 0x7c2, 0);
    let n = text(m, name.0);
    for line in RENDER_USAGE.iter() {
        err(o, &line_with_name(line, &n));
    }
    Exit(code)
}

/// [soft 0x080cd10c printUsage]: the interactive program's help (the banner with the years
/// from 1986, then 29 lines) and the end of the program with status 0.
pub fn print_usage(m: &mut Mem, o: &mut dyn Outside, name: CStr32) -> Exit {
    stand_display_version(m, o, name, CStr32::NULL, CStr32::NULL, 0x7c2, 0);
    let n = text(m, name.0);
    for line in PRINT_USAGE.iter() {
        err(o, &line_with_name(line, &n));
    }
    Exit(0)
}

/// [soft 0x080d059c error]: `***Error:` with a message and a detail, then the batch
/// renderer's help and the end of the program with status 1.
pub fn error(m: &mut Mem, o: &mut dyn Outside, message: CStr32, detail: CStr32) -> Exit {
    let (a, b) = (text(m, message.0), text(m, detail.0));
    error_text(m, o, &a, &b)
}

/// `error` with its two texts given as bytes (the batch renderer's own messages).
pub(super) fn error_text(m: &mut Mem, o: &mut dyn Outside, message: &[u8], detail: &[u8]) -> Exit {
    err(o, &[b"\n***Error: ".as_slice(), message, b" ", detail, b"\n\n"].concat());
    let name = CStr32::new(m[g::PROGRAM_NAME]);
    usage(m, o, 1, name)
}

/// [soft 0x080cf4f8 testsubRegion]: the sub-region of `-b` must lie in the unit square:
/// each of the four stored numbers in 0..1, and left plus one-minus-right, one-minus-top
/// plus bottom, not above 1 (summed in the unit's 80-bit registers); otherwise the help
/// and status 0. A NaN passes every test.
pub fn test_sub_region(m: &mut Mem, o: &mut dyn Outside) -> Result<(), Exit> {
    let (l, r, t, b) = (m.f32(CTL + ctl::SUB_LEFT), m.f32(CTL + ctl::SUB_RIGHT_INV), m.f32(CTL + ctl::SUB_TOP_INV), m.f32(CTL + ctl::SUB_BOTTOM));
    let name = CStr32::new(m[g::PROGRAM_NAME]);
    if t < 0.0 || t > 1.0 || b < 0.0 || b > 1.0 || r < 0.0 || r > 1.0 || l < 0.0 || l > 1.0 {
        return Err(usage(m, o, 0, name));
    }
    if (F80::from_f32(l) + r).gt(F80::ONE) || (F80::from_f32(t) + b).gt(F80::ONE) {
        return Err(usage(m, o, 0, name));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// sys_main.c: the start-up constants, the resource directory

/// [soft 0x080d0e6c set_init_buf]: `strncpy(init_buf, from, 7)`.
pub fn set_init_buf(m: &mut Mem, from: u32) {
    let mut ended = false;
    for i in 0..7 {
        let c = if ended { 0 } else { m.u8(from + i) };
        ended |= c == 0;
        m.set_u8(g::INIT_BUF.0 + i, c);
    }
}

/// [soft 0x080d0e8c get_init_value]: constant `index` of the start-up code: byte `index`
/// of `init_buf` as a signed char, less 11, less `index`, and less 1 more for index 0.
/// With the bytes the licensed start-up leaves there (`srv_buf`) the seven values are 1, 4,
/// 1, 1, 0, 4, 13.
pub fn get_init_value(m: &Mem, index: i32) -> i32 {
    let c = m.i8(g::INIT_BUF.0.wrapping_add(index as u32)) as i32;
    c - (index == 0) as i32 - 11 - index
}

/// [soft 0x080caae8 getDefaultPath]: `*out` = a 4,096-byte block from `SI_malloc` holding
/// the directory of the program file (`realpath`, cut at its last `/`), or else the current
/// directory, followed by `/../rsrc`; an empty string when neither can be had. Nothing
/// with a null `out` or when the block cannot be had.
pub fn get_default_path(m: &mut Mem, o: &mut dyn Outside, program: CStr32, out: u32) {
    if out == 0 {
        return;
    }
    let buf = m.si_malloc(0x1000);
    m.set_u32(out, buf);
    if buf == 0 {
        return;
    }
    m.set_u8(buf, 0);
    let mut have = false;
    if !o.realpath(m, program, buf).is_null() {
        let slash = si_mem::clib::strrchr(m, buf, b'/');
        if slash != 0 {
            m.set_u8(slash, 0);
            have = true;
        }
    }
    if !have && o.getcwd(m, buf, 0xfff).is_null() {
        m.set_u8(buf, 0);
        return;
    }
    let mut path = m.cstr(buf);
    path.push(b'/');
    path.extend_from_slice(RSRC_PATH);
    m.put_cstr(buf, &path);
}

// ---------------------------------------------------------------------------------------
// the batch renderer

/// Where `standAloneRender` hands the scene to Get Scene: the three arguments of
/// `SI_dbLoadScene(ed, db, name)` and the routine's locals the second half needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandOver {
    /// The element dictionary of this process (`SI_elemDicts` + 0x70 × `SI_procData[0]`).
    pub ed: u32,
    /// The input database (`SI_pDbiin`).
    pub db: u32,
    /// The scene's name: `argv`'s string with a `.dsc` cut off, or the latest version's.
    pub scene: u32,
    /// `standAloneRender`'s third argument (freed after the options).
    pub resources: u32,
    /// How many `-D` the first pass counted (a `-o` after one keeps the picture path).
    pub out_db: i32,
}

/// The first half of `standAloneRender (0x080cdffc)`, up to the load (see
/// [`stand_alone_render`]): the start-up of the batch renderer, the first pass over the
/// options, "Reading scene from Database \[path\]".
pub fn stand_alone_render_start(m: &mut Mem, o: &mut dyn Outside, argc: i32, argv: u32, resources: CStr32) -> Result<HandOver, Exit> {
    let mut latest = false;
    let mut out_db = 0i32;
    m[g::SI_STANDALONE_RENDERER] = 1;
    m[g::ARGV] = argv;
    m[g::ARGC] = argc;
    if argc == 1 {
        let name = CStr32::new(m[g::PROGRAM_NAME]);
        return Err(usage(m, o, 0, name));
    }
    // sprintf(string, "%s/soft.init", resources): a site's start-up script
    let res = text(m, resources.0);
    m.put_cstr(g::STRING.0, &[&res[..], b"/soft.init"].concat());
    if o.access(m, CStr32::new(g::STRING.0), 0) == 0 {
        m.put_cstr(g::STRING.0, &[b"csh ".as_slice(), &res, b"/soft.init"].concat());
        o.si_system(m, CStr32::new(g::STRING.0));
    }
    let mut scene = next_arg(m);
    m[g::ARGC] -= 1;
    o.si_init_dictionaries(m, resources, 1, 1);
    if m[g::SI_PDBIIN] == 0 {
        return Err(Exit(-1));
    }
    // the path globals: copies of the current directory; the pictures go to the output
    // database's RENDER_PICTURES chapter if it may be written
    let cwd = o.getcwd(m, 0, 0xff).0;
    m[g::SI_CWD] = cwd;
    m[g::SI_OBJECT_PATH] = strdup(m, cwd)?;
    let p = strdup(m, m[g::SI_CWD])?;
    m[g::SI_TEXTURE_PATH] = p;
    let mut picture = m[g::SI_CWD];
    let out_db_rec = m[g::SI_PDBIOUT];
    if out_db_rec != 0 {
        o.si_db_set_chapter(m, out_db_rec, CStr32::new(0x0867b4c0));
        o.si_dbfilepath_build(m, m[g::SI_PDBIOUT]);
        if o.si_db_chapter_check_access(m, m[g::SI_PDBIOUT], 2) != 0 {
            picture = m.u32(m[g::SI_PDBIOUT] + 0x14);
        } else {
            picture = m[g::SI_CWD];
        }
    }
    m[g::SI_PICTURE_PATH] = strdup(m, picture)?;
    for at in [g::SI_MATERIAL_LIB, g::SI_TEXTURE_LIB, g::SI_PALETTE_LIB] {
        let p = strdup(m, m[g::SI_CWD])?;
        m[at] = p;
    }
    o.si_init_dictionaries(m, resources, 1, 1);
    if m[g::SI_PDBIIN] == 0 {
        return Err(Exit(-1));
    }
    o.si_init_wave(m);
    o.si_graf_list_create(m);
    let k = o.si_graf_list_add(m);
    if k > 0 {
        m[g::SI_SCNGL] = m[g::SI_GRAF_LISTS].wrapping_add((k as u32).wrapping_mul(0x24));
    }
    let camera = o.si_new_camera(m, CStr32::new(0x0867b4d0), 1);
    let pd = m[g::SI_PROC_DATA];
    m.set_u32(pd + 8, camera);
    let list = m[g::SI_GRAF_LISTS].wrapping_add(m.u32(pd + 4).wrapping_mul(0x24));
    let cam = m.u32(pd + 8);
    o.si_graflist_add_data(m, list, cam, 4, 0);
    manipulation_init(m, o);
    animation_info_init(m, o);
    o.si_matter_init(m);
    o.si_metaball_init(m);
    o.si_cus_init(m);
    // a ".dsc" at the end of the name is cut: the four bytes in front of the string's end
    // are compared, also when the name is shorter (the original then compares the bytes
    // that lie before the string, as here)
    first(m, scene)?;
    let end = scene.wrapping_add(m.strlen(scene));
    let tail = end.wrapping_sub(4);
    if m.bytes(tail, 4) == b".dsc" {
        m.set_u8(tail, 0);
    }

    // the first pass: -d <database> (input and output), -L, and how many -D
    let mut p = m[g::ARGV];
    let mut n = m[g::ARGC];
    loop {
        n -= 1;
        if n <= 0 {
            break;
        }
        p = p.wrapping_add(4);
        let a = m.u32(p);
        if first(m, a)? != b'-' {
            continue;
        }
        match m.u8(a + 1) {
            b'L' => latest = true,
            b'D' => {
                out_db += 1;
                p = p.wrapping_add(4);
                n -= 1;
            }
            b'd' => {
                p = p.wrapping_add(4);
                let name = m.u32(p);
                n -= 1;
                let db = o.si_db_dir_get_database_from_name(m, g::SI_DBDIR.0, CStr32::new(name));
                if db != 0 && m.i16(db + 0x18) != 0 {
                    m[g::SI_PDBIIN] = db;
                    m[g::SI_PDBIOUT] = db;
                } else {
                    let t = text(m, name);
                    err(o, &[b"ERROR: input database [".as_slice(), &t, rt(0x0867b519)].concat());
                }
            }
            _ => {}
        }
    }
    if latest {
        o.si_db_set_chapter(m, m[g::SI_PDBIIN], CStr32::new(0x0867b52f));
        o.si_dbfilepath_build(m, m[g::SI_PDBIIN]);
        scene = o.si_dbfile_find_latest_version(m, m[g::SI_PDBIIN], CStr32::new(scene)).0;
        let t = text(m, scene);
        err(o, &[b"Latest version of scene used: [".as_slice(), &t, b"]\n"].concat());
    }
    let path = text(m, m.u32(m[g::SI_PDBIIN] + 0x0c));
    err(o, &[b"Reading scene from Database [".as_slice(), &path, b"]\n"].concat());
    let index = m.u32(m[g::SI_PROC_DATA]);
    let ed = m[g::SI_ELEM_DICTS].wrapping_add(index.wrapping_mul(0x70));
    Ok(HandOver { ed, db: m[g::SI_PDBIIN], scene, resources: resources.0, out_db })
}

/// The second half of `standAloneRender (0x080cdffc)`, from the load's answer `loaded`
/// on (see [`stand_alone_render`]): the options applied to the render set-up the scene
/// brought (`SI_defaultControl`), the renderer number adjusted by the start-up constant,
/// `SI_callRender`.
pub fn stand_alone_render_finish(m: &mut Mem, o: &mut dyn Outside, h: &HandOver, loaded: i16) -> Result<(), Exit> {
    let mut mr_given = false;
    if loaded == 0 {
        let (s, d) = (text(m, h.scene), text(m, m.u32(m[g::SI_PDBIIN] + 8)));
        err(o, &[b"ERROR: Scene [".as_slice(), &s, b"] could not be found in database [", &d, b"]\n"].concat());
        return Err(Exit(-1));
    }
    loop {
        m[g::ARGC] -= 1;
        if m[g::ARGC] <= 0 {
            break;
        }
        let a = next_arg(m);
        if first(m, a)? != b'-' {
            let d = m.cstr(a);
            return Err(error_text(m, o, b"Unknown option", &d));
        }
        let left = m[g::ARGC];
        let letter = m.u8(a + 1);
        match letter {
            b'd' => {
                m[g::ARGC] -= 1;
                m[g::ARGV] += 4;
            }
            b'L' => {}
            b's' => {
                let mut ok = true;
                if left <= 3 {
                    ok = false;
                } else {
                    let (d1, d2, d3) = (first(m, arg(m, 1))? == b'-', first(m, arg(m, 2))? == b'-', first(m, arg(m, 3))? == b'-');
                    if d1 || d2 || d3 {
                        if d3 {
                            ok = false;
                        } else {
                            if d1 && !whole_integer(&m.cstr(arg(m, 1) + 1)) {
                                ok = false;
                            }
                            if ok && d2 && !whole_integer(&m.cstr(arg(m, 2) + 1)) {
                                ok = false;
                            }
                        }
                    }
                }
                if !ok {
                    return Err(error_text(m, o, rt(0x0867b620), b""));
                }
                for at in [ctl::FIRST, ctl::LAST, ctl::STEP] {
                    let s = next_arg(m);
                    let v = atoi16(m, s)?;
                    m.set_i16(CTL + at, v);
                }
                m[g::ARGC] -= 3;
            }
            b'f' => {
                if left <= 1 || first(m, arg(m, 1))? == b'-' {
                    return Err(error_text(m, o, rt(0x0867b660), b""));
                }
                let s = next_arg(m);
                let v = atoi16(m, s)?;
                m[g::SI_REPORT_FREQUENCY] = if v > 0 { v } else { 10 };
                m[g::ARGC] -= 1;
            }
            b'r' => {
                if left <= 2 || first(m, arg(m, 1))? == b'-' || first(m, arg(m, 2))? == b'-' {
                    return Err(error_text(m, o, rt(0x0867b6e0), b""));
                }
                let s = next_arg(m);
                let x = atoi16(m, s)?;
                m.set_i16(CTL + ctl::XRES, x);
                let s = next_arg(m);
                let y = atoi16(m, s)?;
                m.set_i16(CTL + ctl::YRES, y);
                if m.i16(CTL + ctl::XRES) <= 0 || y <= 0 {
                    return Err(error_text(m, o, rt(0x0867b6a0), b""));
                }
                m[g::ARGC] -= 2;
            }
            b'v' => m[g::VERBOSE_OPTION] = 1,
            b'l' => {
                if m[g::LINE_OPTION] == 0 {
                    m[g::LINE_OPTION] = 1;
                    // the questions go to /dev/null unless -v came before (to the standard
                    // output if /dev/null cannot be opened)
                    let mut stream = 0;
                    if m[g::VERBOSE_OPTION] == 0 {
                        stream = o.fopen(m, CStr32::new(0x0867b709), CStr32::new(0x0867b707));
                    }
                    if stream == 0 {
                        stream = m.u32(g::STDOUT);
                    }
                    o.read_setup(m, stream);
                    if stream != m.u32(g::STDOUT) {
                        o.fclose(m, stream);
                    }
                }
            }
            b'S' => {
                let stream = m.u32(g::STDOUT);
                o.read_setup(m, stream);
            }
            b'G' => {
                if left <= 1 || first(m, arg(m, 1))? == b'-' {
                    return Err(error_text(m, o, rt(0x0867b748), b""));
                }
                let s = next_arg(m);
                if m.strlen(s) != 5 {
                    return Err(error_text(m, o, rt(0x0867b720), b""));
                }
                for k in 0..5 {
                    let on = (m.u8(s + k) != b'0') as i16;
                    m.set_i16(CTL + ctl::SWITCHES + 2 * k, on);
                }
                m[g::ARGC] -= 1;
            }
            b'j' => m.set_i16(CTL + ctl::SKIP_EXISTING, 1),
            b'c' => m.set_i16(CTL + ctl::COMPLETE, 1),
            b'z' => m.set_i16(CTL + ctl::Z_CHANNEL, 1),
            b'o' => {
                if h.out_db == 0 {
                    let s = next_arg(m);
                    m[g::SI_PICTURE_PATH] = strdup(m, s)?;
                    strip_out_name(m, o)?;
                } else {
                    m[g::ARGV] += 4;
                }
                m[g::ARGC] -= 1;
            }
            b'n' => {
                let s = next_arg(m);
                let p = strdup(m, s)?;
                m.set_u32(CTL + ctl::OUT_NAME, p);
                m[g::ARGC] -= 1;
            }
            b'b' => {
                let r = m.u16(CTL + ctl::RENDERER);
                if r != 4 && r != 9 && r != 10 {
                    return Err(error_text(m, o, rt(0x0867b780), b""));
                }
                if m[g::ARGC] <= 4 {
                    let name = CStr32::new(m[g::PROGRAM_NAME]);
                    return Err(usage(m, o, 0, name));
                }
                m.set_i16(CTL + ctl::SUBREGION, 1);
                let s = next_arg(m);
                let v = atof32(m, s)?;
                m.set_f32(CTL + ctl::SUB_LEFT, v);
                m[g::ARGC] -= 1;
                let s = next_arg(m);
                let v = atof32(m, s)?;
                m.set_f32(CTL + ctl::SUB_RIGHT_INV, (F80::ONE - F80::from_f32(v)).to_f32());
                m[g::ARGC] -= 1;
                let s = next_arg(m);
                let v = atof32(m, s)?;
                m.set_f32(CTL + ctl::SUB_BOTTOM, v);
                m[g::ARGC] -= 1;
                let s = next_arg(m);
                let v = atof32(m, s)?;
                m.set_f32(CTL + ctl::SUB_TOP_INV, (F80::ONE - F80::from_f32(v)).to_f32());
                m[g::ARGC] -= 1;
                test_sub_region(m, o)?;
            }
            b'D' => {
                let name = next_arg(m);
                m[g::ARGC] -= 1;
                let db = o.si_db_dir_get_database_from_name(m, g::SI_DBDIR.0, CStr32::new(name));
                if db != 0 && m.i16(db + 0x18) != 0 {
                    m[g::SI_PDBIOUT] = db;
                    o.si_db_set_chapter(m, db, CStr32::new(0x0867b7c2));
                    o.si_dbfilepath_build(m, m[g::SI_PDBIOUT]);
                    if o.si_db_chapter_check_access(m, m[g::SI_PDBIOUT], 2) != 0 {
                        let chapter = m.u32(m[g::SI_PDBIOUT] + 0x14);
                        m[g::SI_PICTURE_PATH] = strdup(m, chapter)?;
                    } else {
                        m[g::SI_PICTURE_PATH] = strdup(m, m[g::SI_CWD])?;
                        let d = text(m, m.u32(m[g::SI_PDBIOUT] + 8));
                        err(o, &[b"ERROR: chapter RENDER_PICTURES of database ".as_slice(), &d, b" not found:\n"].concat());
                        let pp = text(m, m[g::SI_PICTURE_PATH]);
                        err(o, &[b" Rendering images to ".as_slice(), &pp, b"\n"].concat());
                    }
                } else {
                    m[g::SI_PICTURE_PATH] = strdup(m, m[g::SI_CWD])?;
                    let d = text(m, name);
                    err(o, &[b"ERROR: output database [".as_slice(), &d, rt(0x0867b519)].concat());
                    let pp = text(m, m[g::SI_PICTURE_PATH]);
                    err(o, &[b" Rendering images to ".as_slice(), &pp, b"\n"].concat());
                }
                strip_out_name(m, o)?;
            }
            b't' => o.si_cam_and_scn_to_origin(m),
            b'M' | b'm' => {
                if m.cstr(a + 1) == b"mr" {
                    m[g::ARGC] -= 1;
                    if m[g::ARGC] <= 0 {
                        return Err(error_text(m, o, rt(0x0867b8a0), b""));
                    }
                    let v = next_arg(m);
                    first(m, v)?;
                    let v = m.cstr(v);
                    if v == b"19" {
                        o.si_set_mi2_output(m, 0);
                        m.set_i16(CTL + ctl::RENDERER, 9);
                        mr_given = true;
                    } else if v == b"21" {
                        o.si_set_mi2_output(m, 1);
                        m.set_i16(CTL + ctl::RENDERER, 10);
                        mr_given = true;
                    } else {
                        return Err(error_text(m, o, rt(0x0867b8a0), b""));
                    }
                } else {
                    if !mr_given {
                        match m.u16(CTL + ctl::RENDERER) {
                            10 => o.si_set_mi2_output(m, 1),
                            9 => o.si_set_mi2_output(m, 0),
                            _ => {
                                o.si_set_mi2_output(m, 1);
                                m.set_i16(CTL + ctl::RENDERER, 10);
                            }
                        }
                    }
                    m.set_i16(CTL + ctl::MI_OUTPUT, 1);
                    let old = m.u32(CTL + ctl::MI_NAME);
                    if old != 0 {
                        m.free(old);
                        m.set_u32(CTL + ctl::MI_NAME, 0);
                    }
                    let cur = m.u32(m[g::ARGV]);
                    m.set_i16(CTL + ctl::MI_PER_FRAME, (m.u8(cur + 1) == b'M') as i16);
                    let left = m[g::ARGC] - 1;
                    let name = if left > 0 && first(m, arg(m, 1))? != b'-' {
                        m[g::ARGC] = left;
                        next_arg(m)
                    } else {
                        0x0867b948
                    };
                    let p = strdup(m, name)?;
                    m.set_u32(CTL + ctl::MI_NAME, p);
                }
            }
            b'P' | b'p' => {
                let pre = letter == b'p';
                if m[g::ARGC] - 1 <= 0 || first(m, arg(m, 1))? == b'-' {
                    let name = CStr32::new(m[g::PROGRAM_NAME]);
                    return Err(usage(m, o, 0, name));
                }
                let s = next_arg(m);
                let script = strdup(m, s)?;
                m[g::ARGC] -= 1;
                if o.access(m, CStr32::new(script), 4) != 0 {
                    let which: &[u8] = if pre { b"Pre" } else { b"Post" };
                    let t = m.cstr(script);
                    err(o, &[b"Unknown ".as_slice(), which, b"-frame script ", &t, b"\n"].concat());
                    if script != 0 {
                        m.free(script);
                    }
                } else {
                    let at = CTL + if pre { ctl::PRE_SCRIPT } else { ctl::POST_SCRIPT };
                    let old = m.u32(at);
                    if old != 0 {
                        m.free(old);
                        m.set_u32(at, 0);
                    }
                    m.set_u32(at, script);
                }
            }
            b'h' => {
                let name = CStr32::new(m[g::PROGRAM_NAME]);
                return Err(usage(m, o, 0, name));
            }
            _ => {
                let d = m.cstr(m.u32(m[g::ARGV]));
                return Err(error_text(m, o, b"Unknown option", &d));
            }
        }
    }
    if h.resources != 0 {
        m.free(h.resources);
    }
    // the renderer number plus 13 less a start-up constant (0 on the full product's path)
    let v = get_init_value(m, 6);
    let r = (m.u16(CTL + ctl::RENDERER) as i32 + 0xd - v) as i16;
    m.set_i16(CTL + ctl::RENDERER, r);
    call_render(m, o);
    Ok(())
}

/// The output name without its directory, after `-o` and `-D`: `SI_strStripDir` makes a
/// new string of the old name, the old one is freed, a copy of the new one takes its place
/// and the new one is freed.
fn strip_out_name(m: &mut Mem, o: &mut dyn Outside) -> Result<(), Exit> {
    let old = m.u32(CTL + ctl::OUT_NAME);
    let stripped = o.si_str_strip_dir(m, CStr32::new(old)).0;
    let old = m.u32(CTL + ctl::OUT_NAME);
    if old != 0 {
        m.free(old);
        m.set_u32(CTL + ctl::OUT_NAME, 0);
    }
    let p = strdup(m, stripped)?;
    m.set_u32(CTL + ctl::OUT_NAME, p);
    if stripped != 0 {
        m.free(stripped);
    }
    Ok(())
}

/// [soft 0x080cdffc standAloneRender]: the batch renderer, `soft [<resources>] -R <scene>
/// [options]`. `argc` and `argv` start at the `-R` argument; `resources` is the resource
/// directory (a block of `si_runCreative`'s, freed here after the options).
///
/// 1. `SI_standaloneRenderer` = 1; without a scene the help and status 0.
/// 2. `<resources>/soft.init`, if it can be reached, run through `csh`.
/// 3. The dictionaries (twice, with the path globals from the current directory in
///    between: `SI_cwd` = `getcwd(NULL, 255)`, six `strdup`s), waves, the graph lists, the
///    camera `Perspective_def`, `SI_manipulationInit`, `SI_animationInfoInit`,
///    `SI_matterInit`, meta-balls, custom effects.
/// 4. A `.dsc` at the end of the name is cut; the first pass takes `-d <database>`, `-L`
///    and counts `-D`; with `-L` the latest version. "Reading scene from Database \[path\]".
/// 5. **`SI_dbLoadScene`**: the hand-over to Get Scene. Failure: a message and status −1.
/// 6. The second pass applies every option in order to `SI_defaultControl` ([`ctl`]).
/// 7. The resource directory is freed; the renderer number adjusted; `SI_callRender`.
///
/// [`stand_alone_render_start`] is steps 1 to 4, [`stand_alone_render_finish`] 6 and 7,
/// for a caller that makes the load itself.
pub fn stand_alone_render(m: &mut Mem, o: &mut dyn Outside, argc: i32, argv: u32, resources: CStr32) -> Result<(), Exit> {
    let h = stand_alone_render_start(m, o, argc, argv, resources)?;
    let loaded = o.si_db_load_scene(m, h.ed, h.db, CStr32::new(h.scene));
    stand_alone_render_finish(m, o, &h, loaded)
}

/// [soft 0x083bdd38 SI_callRender]: the working set-up `SI_ren` is a copy of
/// `SI_defaultControl` (0x600 bytes); +0x178 and +0x17a of the default become 0 and 1;
/// custom effects are told (0xe, 0x10) before and (0xe, 0x11) after; renderer 0 wire
/// frame, 1 and 2 hidden line (with 1 and 0), 4 the native renderer, 9 and 10 mental ray,
/// each after "Rendering..." on the standard output; any other is refused on the
/// standard error stream. Answers 0.
pub fn call_render(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    m.copy(g::SI_REN.0, CTL, 0x600);
    m.set_i16(CTL + ctl::WORD178, 0);
    m.set_i16(CTL + ctl::WORD17A, 1);
    o.si_cus_call_custom_functions(m, 0xe, 0x10);
    m[g::SI_CUS_UPDATE_STATE] = 0xe;
    match m.i16(CTL + ctl::RENDERER) as i32 as u32 {
        0 => {
            out(o, b"Rendering...\n");
            o.si_sequence_wire_frame(m);
        }
        1 => {
            out(o, b"Rendering...\n");
            o.si_sequence_hidden_line(m, 1);
        }
        2 => {
            out(o, b"Rendering...\n");
            o.si_sequence_hidden_line(m, 0);
        }
        4 => {
            out(o, b"Rendering...\n");
            o.si_sequence_render(m);
        }
        9 | 10 => {
            out(o, b"Rendering...\n");
            o.si_sequence_render_mental(m, 1);
        }
        _ => {
            err(o, rt(0x086c73e0));
            err(o, rt(0x086c7420));
        }
    }
    m[g::SI_CUS_UPDATE_STATE] = 2;
    o.si_cus_call_custom_functions(m, 0xe, 0x11);
    0
}

// ---------------------------------------------------------------------------------------
// emergencySave.c

/// [soft 0x08392a88 SI_replicateCommandLine]: the command line as one string in
/// `commandLine` (each argument followed by a blank), for the offer to restart after a
/// crash. Answers the buffer.
pub fn replicate_command_line(m: &mut Mem, argc: i32, argv: u32) -> u32 {
    let at = g::COMMAND_LINE.0;
    m.set_u8(at, 0);
    let mut line = Vec::new();
    let mut i = 0;
    while i < argc {
        line.extend_from_slice(&m.cstr(m.u32(argv + 4 * i as u32)));
        line.push(b' ');
        m.put_cstr(at, &line);
        i += 1;
    }
    at
}

/// `SI_setupEmergencySave (0x08392c30)` with mode 0, and so `initEmergencySave
/// (0x08392ff4)` with mode 0 and no directory: emergency save off while the program
/// starts. The stores of that path, in the original's order; the routines' other paths
/// are in the crate's struct form (`App::setup_emergency_save`). Not cited: one path of
/// each (measured on its own, finding 0019, section 12).
pub fn setup_emergency_save_off(m: &mut Mem) {
    let e = g::EMERGENCY_DETAILS.0;
    m.set_u8(e + 0x1031, 0); // the save directory
    m.set_u8(e + 0x32, 0); // the directory asked for
    m.set_u32(e + 0x2c, 0); // times entered
    m.set_i16(e + 0x30, 0); // saving
    m.set_i16(e, 0); // enabled
    m.set_u32(e + 0x503c, 3);
    m.set_u32(e + 0x5030, 0); // the handler
    m.set_i16(e + 0x5038, 0); // gclear, xconfirm, sfplay found
    m.set_i16(e + 0x5036, 0);
    m.set_i16(e + 0x5034, 0);
    m.set_u8(e + 0x2030, 0); // the crash log's name
}

// ---------------------------------------------------------------------------------------
// si_runCreative's path to the batch renderer, and the entry points

/// The command line asks for the interactive program: its start-up (windows, canvases,
/// the main loop) is not on the image. [`start_up`] answers `Ok(None)` and [`run`] this.
pub const INTERACTIVE: Exit = Exit(i32::MIN);

/// The part of `si_runCreative (0x080cab88)` that runs before the batch renderer: the
/// command line read option by option until `-R` (finding 0019, section 3.1), in the
/// original's order, with the original's allocations. Unchecked as a whole: the routine
/// cannot be emulated (finding 0019, section 9.3); its callees are ported and measured one
/// by one. Answers the hand-over of `-R`, `None` when the command line ends without `-R`
/// (the interactive program), or the end of the program.
pub fn run_creative(m: &mut Mem, o: &mut dyn Outside, argc: i32, argv: u32) -> Result<Option<HandOver>, Exit> {
    let argn = |m: &Mem, i: i32| m.u32(argv.wrapping_add(4 * i as u32));
    // the keys file's name: a copy of SI_userPrefKeysRsrc's
    let mut keys = strdup(m, m[g::SI_USER_PREF_KEYS_RSRC])?;
    let mut resources = 0u32;
    let mut setup = 0u32;
    // (_setjmp and the bookkeeping of the licence checks around it, not ported)
    replicate_command_line(m, argc, argv);
    // licence check, not ported
    setup_emergency_save_off(m);
    m[g::SI_INIT_POSE_COMPENSATE] = 0;
    if !o.getenv(m, CStr32::new(0x0867978a)).is_null() {
        m[g::SI_INIT_POSE_COMPENSATE] = 1;
    }
    // licence checks, not ported (getpid, the licence client's user list)
    let uid = o.geteuid(m);
    let pw = o.getpwuid(m, uid);
    if pw == 0 {
        m.record_fault(0, 4, Access::Read);
        return Err(FAULT);
    }
    let user = m.u32(pw);
    m[g::SI_USERNAME] = strdup(m, user)?;
    // the start-up constants: on the full product's path the local copy is srv_buf's
    // (the fourth argument of si_runCreative is 1)
    let local = m.local(8);
    let srv = m.bytes(g::SRV_BUF.0, 7).to_vec();
    m.set_bytes(local, &srv);
    m[g::PROGRAM_NAME] = argn(m, 0);
    o.si_menu_table_init(m);
    o.si_status_init_path(m);
    set_init_buf(m, local);
    m.release(8);
    // licence checks, not ported; around them the locale is read, set to "C" and put back
    let locale = o.setlocale(m, 6, CStr32::NULL).0;
    if locale == 0 {
        out(o, rt(0x08678fdc));
        return Err(Exit(1));
    }
    let saved = m.strdup(locale);
    if saved == 0 {
        out(o, b"Error\n");
        return Err(Exit(1));
    }
    o.setlocale(m, 6, CStr32::new(0x08679822));
    o.setlocale(m, 6, CStr32::new(saved));
    let mr19 = o.si_is_mr19_enable(m);
    o.si_set_mi2_output(m, (mr19 == 0) as i32);
    m[g::SI_BASE_LICENSE] = 0;
    m[g::SI_LIGHT_LICENSE] = 0;
    m[g::SI_LIGHT_LICENSE2] = 0;
    let dash = |m: &Mem, i: i32| -> Result<bool, Exit> { Ok(first(m, argn(m, i))? == b'-') };
    let program = |m: &Mem| CStr32::new(m[g::PROGRAM_NAME]);
    let mut i = 1;
    while i < argc {
        let a = argn(m, i);
        if first(m, a)? != b'-' {
            resources = strdup(m, a)?;
            i += 1;
            continue;
        }
        let letter = m.u8(a + 1);
        match letter {
            // product-tier flags: stored, never branched on
            b'B' => m[g::SI_BASE_LICENSE] = 1,
            b'G' => {
                m[g::SI_LIGHT_LICENSE] = 1;
                m[g::SI_LIGHT_LICENSE2] = 1;
            }
            b'f' => m[g::SI_FOREGROUND] = 1,
            b'c' => m[g::COREDUMP] = 1,
            b'h' => {
                if m.cstr(a + 2) == b"dev" {
                    // the program's self-check, licence code: not ported
                    return Err(Exit(0));
                }
                return Err(print_usage(m, o, program(m)));
            }
            b's' => {
                if i + 1 != argc && !dash(m, i + 1)? {
                    if resources == 0 {
                        err(o, rt(0x08679840));
                        return Err(Exit(0));
                    }
                    i += 1;
                    setup = strdup(m, argn(m, i))?;
                }
                if setup == 0 {
                    setup = strdup(m, m[g::SI_SETUP_RSRC])?;
                }
            }
            b'k' => {
                if i + 1 == argc || dash(m, i + 1)? {
                    err(o, format!("The -{} option need a filename as argument.\n", letter as char).as_bytes());
                    return Err(Exit(0));
                }
                if keys != 0 {
                    m.free(keys);
                }
                keys = m.si_calloc(1, m.strlen(argn(m, i + 1)) + 5);
                if keys == 0 {
                    return Err(Exit(0));
                }
                i += 1;
                let name = m.cstr(argn(m, i));
                m.put_cstr(keys, &name);
            }
            b'L' | b'l' => {
                if i + 1 != argc && !dash(m, i + 1)? {
                    i += 1;
                    strdup(m, argn(m, i))?;
                }
            }
            b'd' => {
                if i + 1 == argc || dash(m, i + 1)? {
                    err(o, format!("The -{} option need a filename as argument.\n", letter as char).as_bytes());
                    return Err(Exit(0));
                }
                if resources == 0 {
                    err(o, rt(0x08679900));
                    return Err(Exit(0));
                }
                i += 1;
                strdup(m, argn(m, i))?;
            }
            b'R' => {
                if resources == 0 {
                    let out = m.local(4);
                    m.set_u32(out, 0);
                    get_default_path(m, o, CStr32::new(argn(m, 0)), out);
                    resources = m.u32(out);
                    m.release(4);
                }
                if resources != 0 {
                    m[g::SI_RESDIR] = strdup(m, resources)?;
                }
                let h = stand_alone_render_start(m, o, argc - i, argv.wrapping_add(4 * i as u32), CStr32::new(resources))?;
                return Ok(Some(h));
            }
            b'C' => {
                if i + 1 != argc && !dash(m, i + 1)? {
                    if resources == 0 {
                        err(o, rt(0x08679940));
                        err(o, rt(0x08679980));
                        return Err(Exit(0));
                    }
                    i += 1;
                    o.si_str_strip_suffix(m, CStr32::new(argn(m, i)));
                }
            }
            b'v' => out(o, &[id_string(), b"\n"].concat()),
            b'D' => {
                // licence check, not ported
                if i > 1 {
                    err(o, format!("The -{} option must be the only one.\n", letter as char).as_bytes());
                    return Err(Exit(0));
                }
                if i + 2 >= argc || dash(m, i + 1)? || dash(m, i + 2)? {
                    err(o, format!("The -{} option needs a filename and a symbol.\n", letter as char).as_bytes());
                    return Err(Exit(0));
                }
                let symbol = argn(m, i + 2);
                let file = argn(m, i + 1);
                let slash = si_mem::clib::strrchr(m, file, b'/');
                let base = if slash == 0 { file } else { slash + 1 };
                let dot = si_mem::clib::strchr(m, base, b'.');
                let library = if dot == 0 {
                    let p = m.si_malloc(m.strlen(file) + 4);
                    let name = [m.cstr(file), b".so".to_vec()].concat();
                    m.put_cstr(p, &name);
                    p
                } else if m.strlen(dot) == 1 {
                    let p = m.si_malloc(m.strlen(file) + 2 + 1);
                    let name = [m.cstr(file), b"so".to_vec()].concat();
                    m.put_cstr(p, &name);
                    p
                } else {
                    file
                };
                m.set_u32(argv.wrapping_add(4 * (i + 2) as u32), library);
                m[g::SI_CUS_STANDALONE] = 1;
                let status = o.si_cus_run_standalone(m, CStr32::new(symbol), argc - i - 2, argv.wrapping_add(4 * (i + 2) as u32));
                return Err(Exit(status));
            }
            _ => {
                let (n, t) = (text(m, m[g::PROGRAM_NAME]), m.cstr(a));
                out(o, &[&n[..], b": illegal option ", &t, b"\n"].concat());
                return Err(print_usage(m, o, program(m)));
            }
        }
        i += 1;
    }
    Ok(None)
}

/// Places a command line on the image as the system hands it to `main`: the strings at
/// the top of the stack, then the array of pointers ended by a null. Answers `argv`.
pub fn place_argv(m: &mut Mem, args: &[&[u8]]) -> u32 {
    let mut at = Vec::new();
    for a in args {
        let p = m.local(a.len() as u32 + 1);
        m.put_cstr(p, a);
        at.push(p);
    }
    let argv = m.local(4 * (args.len() as u32 + 1));
    for (k, p) in at.iter().enumerate() {
        m.set_u32(argv + 4 * k as u32, *p);
    }
    m.set_u32(argv + 4 * args.len() as u32, 0);
    argv
}

/// **The entry point**: the program started as `args` (`args[0]` the program's name), up
/// to the hand-over of the batch renderer to Get Scene. On the image: `.data`'s values of
/// this module ([`super::init_data`]), the C library's `stdout` and `stderr`, the command
/// line ([`place_argv`]), then `si_runCreative`'s path ([`run_creative`]) and the first
/// half of `standAloneRender` ([`stand_alone_render_start`]), whose last act is the line
/// "Reading scene from Database \[path\]".
///
/// Answers the hand-over (the caller loads the scene: `SI_dbLoadScene(h.ed, h.db,
/// h.scene)`, then calls [`finish`]), `None` for a command line without `-R`, or the end
/// of the program (`-h`, a usage error, no input database: the status `exit` was given).
pub fn start_up(m: &mut Mem, o: &mut dyn Outside, args: &[&[u8]]) -> Result<Option<HandOver>, Exit> {
    super::init_data(m);
    m.set_u32(g::STDIN, g::STDIN_HANDLE);
    m.set_u32(g::STDOUT, g::STDOUT_HANDLE);
    m.set_u32(g::STDERR, g::STDERR_HANDLE);
    let argv = place_argv(m, args);
    run_creative(m, o, args.len() as i32, argv)
}

/// After the load: `loaded` is what `SI_dbLoadScene` answered. The second half of
/// `standAloneRender` ([`stand_alone_render_finish`]: the options on the render set-up,
/// `SI_callRender`), then `si_runCreative`'s `exit(0)`. Answers the program's exit
/// status.
pub fn finish(m: &mut Mem, o: &mut dyn Outside, h: &HandOver, loaded: i16) -> Exit {
    match stand_alone_render_finish(m, o, h, loaded) {
        Ok(()) => Exit(0),
        Err(e) => e,
    }
}

/// The whole batch path with the load as a call of the outside world
/// ([`BatchCalls::si_db_load_scene`]): [`start_up`], the load, [`finish`]. A command line
/// without `-R` answers [`INTERACTIVE`].
pub fn run(m: &mut Mem, o: &mut dyn Outside, args: &[&[u8]]) -> Exit {
    match start_up(m, o, args) {
        Ok(Some(h)) => {
            let loaded = o.si_db_load_scene(m, h.ed, h.db, CStr32::new(h.scene));
            finish(m, o, &h, loaded)
        }
        Ok(None) => INTERACTIVE,
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    //! The entry points on made-up worlds (the routines themselves are measured against the
    //! emulated original by `app-imagecheck`).
    use super::*;
    use si_mem::{Ret, Routine};

    /// A world that answers as a container with one database would, records the names
    /// called and what is printed.
    #[derive(Default)]
    struct World {
        names: Vec<&'static str>,
        printed: [Vec<u8>; 2],
        db: u32,
        loaded: Option<Vec<u8>>,
    }

    impl Outside for World {
        fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Ret {
            self.names.push(r.name);
            match r.name {
                "getcwd" => Ret::eax(libc_getcwd(m, a[0], a[1], b"/tmp/work")),
                "geteuid" => Ret::eax(1000),
                "getpwuid" => {
                    let pw = m.place(0x1c, 4);
                    let name = m.place_cstr(b"soft");
                    m.set_u32(pw, name);
                    Ret::eax(pw)
                }
                "setlocale" => Ret::eax(if a[1] == 0 { m.place_cstr(b"C") } else { a[1] }),
                "access" => Ret::int(-1),
                "SI_initDictionaries" => {
                    if self.db == 0 {
                        self.db = m.place(0x1c, 4);
                        let p = m.place_cstr(b"/data/DB");
                        m.set_u32(self.db + 0xc, p);
                        m.set_i16(self.db + 0x18, 1);
                    }
                    m[g::SI_PDBIIN] = self.db;
                    m[g::SI_PDBIOUT] = self.db;
                    Ret::NONE
                }
                "SI_grafListAdd" => Ret::int(1),
                "SI_dbLoadScene" => {
                    self.loaded = Some(m.cstr(a[2]));
                    let p = m.place_cstr(b"/old/default");
                    m.set_u32(CTL + ctl::OUT_NAME, p);
                    m.set_i16(CTL + ctl::RENDERER, 4);
                    Ret::int(1)
                }
                "SI_strStripDir" => {
                    let s = m.cstr(a[0]);
                    let base = s.rsplit(|&c| c == b'/').next().unwrap().to_vec();
                    let p = m.malloc(base.len() as u32 + 1);
                    m.put_cstr(p, &base);
                    Ret::eax(p)
                }
                _ => Ret::NONE,
            }
        }
        fn print(&mut self, stream: u32, text: &[u8]) {
            self.printed[(stream == 2) as usize].extend_from_slice(text);
        }
    }

    #[test]
    fn the_batch_renderer_up_to_the_load_and_after() {
        let mut m = Mem::new();
        let mut w = World::default();
        let args: [&[u8]; 10] = [b"soft", b"/res", b"-R", b"scene.dsc", b"-s", b"3", b"7", b"2", b"-o", b"/out/p"];
        let h = start_up(&mut m, &mut w, &args).unwrap().expect("-R");
        assert_eq!(w.printed[1], b"Reading scene from Database [/data/DB]\n");
        // the start-up words the loader and the renderer read
        assert_eq!(m[g::SI_STANDALONE_RENDERER], 1);
        assert_eq!(m.cstr(m[g::SI_RESDIR]), b"/res");
        assert_eq!(m.cstr(m[g::SI_CWD]), b"/tmp/work");
        assert_eq!(m.cstr(m[g::SI_TEXTURE_PATH]), b"/tmp/work");
        assert_eq!(m.cstr(m[g::SI_PICTURE_PATH]), b"/tmp/work");
        assert_eq!(m.i16(g::SI_GLB_MANIP.0 + 0x36), 1);
        assert_eq!(m.f32(g::SI_GLB_ANIM.0 + 0x18), 0.15);
        assert_eq!(get_init_value(&m, 6), 13);
        assert_eq!(m.cstr(h.scene), b"scene");
        let i = w.names.iter().position(|n| *n == "SI_matterInit").unwrap();
        assert!(w.names[..i].contains(&"getenv") && !w.names.contains(&"SI_dbLoadScene"));
        // the caller loads, then the options go on the scene's set-up
        let loaded = w.call(&mut m, Routine::new("SI_dbLoadScene", 0x080da548), &[h.ed, h.db, h.scene]).eax as i16;
        assert_eq!(finish(&mut m, &mut w, &h, loaded), Exit(0));
        assert_eq!((m.i16(CTL + ctl::FIRST), m.i16(CTL + ctl::LAST), m.i16(CTL + ctl::STEP)), (3, 7, 2));
        assert_eq!(m.cstr(m[g::SI_PICTURE_PATH]), b"/out/p");
        assert_eq!(m.cstr(m.u32(CTL + ctl::OUT_NAME)), b"default");
        assert_eq!(m.bytes(g::SI_REN.0 + ctl::FIRST, 2), &3i16.to_le_bytes());
        assert_eq!(w.printed[0], b"Rendering...\n");
        assert!(w.names.ends_with(&["SI_cusCallCustomFunctions", "SI_sequenceRender", "SI_cusCallCustomFunctions"]));
        assert!(m.fault().is_none());
    }

    #[test]
    fn help_unknown_options_and_the_interactive_program() {
        // the help texts and messages are read from the user's install (unit rta)
        si_rt::skip_without_install!();
        let mut m = Mem::new();
        let mut w = World::default();
        assert_eq!(run(&mut m, &mut w, &[b"soft", b"-h"]), Exit(0));
        assert!(w.printed[1].starts_with(b"\nsoft 4.0\n(c) 1986-"));
        assert!(w.printed[1].ends_with(b"\t     option)\n\n"));
        let mut m = Mem::new();
        let mut w = World::default();
        assert_eq!(run(&mut m, &mut w, &[b"soft", b"/res", b"-R", b"s", b"-q"]), Exit(1));
        assert!(w.printed[1].windows(24).any(|t| t == b"***Error: Unknown option"));
        let mut m = Mem::new();
        assert_eq!(run(&mut m, &mut World::default(), &[b"soft", b"/res"]), INTERACTIVE);
    }
}
