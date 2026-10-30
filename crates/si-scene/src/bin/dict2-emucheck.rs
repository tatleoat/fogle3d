// SPDX-License-Identifier: MIT
//! Compares the dictionary routines `si-scene` adds on the memory image (`dictdump.rs` and
//! the modules of the "Dictionary: the rest" package) with the original, on vectors of the
//! emulation harness (`re/tools/si-scene/emu_gen_dict2.py` and its siblings).
//!
//!   dict2-emucheck <dir of .vec files, or of one directory of them per routine>
//!
//! The case runner is that of `scene-emucheck` (`emucheck_parts/image.rs`): the case's
//! buffers are loaded into the image, the port runs, and memory, the heap, the calls out and
//! the text printed are compared. `EMUCHECK_MUTATE=1` and `=calls` are its two mutation
//! switches.

use si_x87::vectors::{check_dir, print_report, Check, Verdict};
use std::path::Path;

#[path = "emucheck_parts/image.rs"]
mod image;

/// `EMUCHECK_MUTATE=text`: one bit of the first byte of the first text the port prints is
/// flipped (the shared switches do not touch printed text, and a dump has nothing else to
/// mutate).
struct FlipText<'a> {
    inner: &'a mut dyn si_dict::outside::Outside,
    done: bool,
}

impl si_dict::outside::Outside for FlipText<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, routine: &'static si_dict::outside::Routine, args: &[u32]) -> u32 {
        self.inner.call(m, routine, args)
    }
    fn call_addr(&mut self, m: &mut si_mem::Mem, addr: u32, args: &[u32]) -> u32 {
        self.inner.call_addr(m, addr, args)
    }
    fn getenv(&mut self, m: &mut si_mem::Mem, name: &[u8]) -> u32 {
        self.inner.getenv(m, name)
    }
    fn stdout(&mut self, text: &[u8]) {
        let t = self.flip(text);
        self.inner.stdout(&t)
    }
    fn stderr(&mut self, text: &[u8]) {
        let t = self.flip(text);
        self.inner.stderr(&t)
    }
}

impl FlipText<'_> {
    fn flip(&mut self, text: &[u8]) -> Vec<u8> {
        let mut t = text.to_vec();
        if !self.done && !t.is_empty() {
            t[0] ^= 1;
            self.done = true;
        }
        t
    }
}

fn text_mutation() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some_and(|v| v == "text")
}

/// Runs a dump with the text mutation when asked.
macro_rules! dump {
    ($o:ident, $body:expr) => {{
        if text_mutation() {
            let mut w = FlipText { inner: $o, done: false };
            let $o: &mut dyn si_dict::outside::Outside = &mut w;
            $body
        } else {
            $body
        }
    }};
}

fn entry(name: &str) -> Option<image::Entry> {
    use si_scene::dictdump as d;
    let port: image::Port = match name {
        "SI_enmdictDump" => |m, o, a| dump!(o, d::enmdict_dump(m, o, a.p("dict"))).into(),
        "SI_reldictDump" => |m, o, a| dump!(o, d::reldict_dump(m, o, a.p("rd"))).into(),
        "SI_reldictCusOpDump" => |_, _, a| d::reldict_cus_op_dump(a.p("rd")).into(),
        "si_relGroupDump" => |m, o, a| dump!(o, d::rel_group_dump(m, o, a.p("rels"), a.i("group"))).into(),
        "SI_elemlistDump" => |m, o, a| dump!(o, d::elemlist_dump(m, o, a.p("list"))).into(),
        "SI_elemdictDump" => |m, o, a| dump!(o, d::elemdict_dump(m, o, a.p("ed"))).into(),
        "SI_updlistDump" => |m, o, a| dump!(o, d::updlist_dump(m, o, a.p("ed"), a.p("rd"), a.p("list"), a.s("raw"))).into(),
        "SI_elemdictTopoSortDump" => |m, o, a| dump!(o, d::topo_sort_dump(m, o, a.p("rd"), a.p("ed"), a.p("list"), a.s("raw"))).into(),
        "SI_hieranimDump" => |m, o, a| dump!(o, d::hieranim_dump(m, o, a.v("anim"))).into(),
        _ => return rest_entry(name),
    };
    Some(image::Entry::new(port))
}

/// The routines of `dictrest.rs` (`emu_gen_dict3.py`).
fn rest_entry(name: &str) -> Option<image::Entry> {
    use si_scene::dictrest as r;
    let port: image::Port = match name {
        "fixPatch" => |m, o, a| r::fix_patch(m, o, a.v("patch")).into(),
        "SI_elemdictMapStructKey" => |m, o, a| r::map_struct_key(m, o, a.p("ed"), a.v("rd"), a.p("elem"), a.i("key"), a.v("other"), a.s("p6"), a.s("p7")).into(),
        "SI_elemdictReplacePrecision" => |m, o, a| r::replace_precision(m, o, a.v("db"), a.p("ed"), a.v("elem"), a.i("chap"), a.s("p6")).into(),
        "SI_elemdictStoreRelationData" => |m, o, a| r::store_relation_data(m, o, a.i("chap"), a.p("elem"), a.p("rel")).into(),
        "SI_dbLoadRelationDictionary" => |m, o, a| r::db_load_relation_dictionary(m, o, a.v("ed"), a.v("rd"), a.v("path"), a.s("report")).into(),
        "SI_initDictionaries" => |m, o, a| r::init_dictionaries(m, o, a.v("res"), a.s("version"), a.s("dirs")).into(),
        "SI_elemdictReadShader" => |m, o, a| r::elemdict_read_shader(m, o, a.v("name"), a.v("file"), a.i("kind")).into(),
        "SI_elemdictReadClusters" => |m, o, a| r::elemdict_read_clusters(m, o, a.v("base"), a.v("model")).into(),
        "SI_dbLoadScenePIPE" => |m, o, a| si_scene::load_pipe::db_load_scene_pipe(m, o, a.v("dict")).into(),
        "SI_elemdictReadSetup" => |m, o, a| r::elemdict_read_setup(m, o, a.v("path")).into(),
        "SI_elemdictReadChannelSetup" => |m, o, a| r::elemdict_read_channel_setup(m, o, a.v("path")).into(),
        "SI_elemdictReadParticleSetup" => |m, o, a| r::elemdict_read_particle_setup(m, o, a.v("path")).into(),
        "reldictReadOneOperatorAttribute" => |m, o, a| {
            let def: [u32; 10] = std::array::from_fn(|k| a.v(&format!("d{k}")));
            r::reldict_read_one_operator_attribute(m, o, a.v("rd"), a.v("atd"), a.v("group"), a.v("file"), a.s("cus"), a.v("custom"), &def, a.v("arg")).into()
        },
        "SI_reldictReadOperatorAttributes" => |m, o, a| r::reldict_read_operator_attributes(m, o, a.v("rd"), a.v("atd"), a.v("file"), a.v("custom")).into(),
        "SI_reldictReadOneOperator" => |m, o, a| r::reldict_read_one_operator(m, o, a.v("ed"), a.v("rd"), a.v("atd"), a.v("file"), a.s("cus"), a.v("custom")).into(),
        "SI_dbDescriptionFileDump" => {
            let port: image::Port = |m, o, a| r::db_description_file_dump(m, o, a.v("ed"), a.v("rd"), a.i("chapter"), a.s("precision"), a.s("relations")).into();
            return Some(image::Entry::with_stdout(port));
        }
        "SI_chnSetupGet" => |m, o, a| r::chn_setup_get(m, o, a.v("path")).into(),
        "SI_oclusterAsciiRead" => |m, o, a| r::ocluster_ascii_read_with(m, o, a.v("base"), a.v("model"), false).into(),
        "elemdictRelDataAllocate" => |m, _, a| r::rel_data_allocate(m, a.i("id"), a.v("count")).into(),
        _ => return None,
    };
    Some(image::Entry::new(port))
}

fn dispatch(c: &si_x87::vectors::Case) -> Verdict {
    match entry(c.name.split('#').next().unwrap_or("")) {
        Some(e) => image::check(c, &e),
        None => Verdict::Skip,
    }
}

fn lookup(name: &str) -> Option<Check> {
    entry(name.split('#').next().unwrap_or("")).map(|_| dispatch as Check)
}

fn main() {
    image::dictrest::ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: dict2-emucheck <dir of .vec files, or of one directory of them per routine>");
        std::process::exit(2)
    });
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    if dirs.is_empty() {
        dirs.push(Path::new(&dir).to_path_buf());
    }
    let mut rows = Vec::new();
    for d in dirs {
        match check_dir(&d, &lookup) {
            Ok(r) => rows.extend(r),
            Err(e) => {
                eprintln!("{}: {e}", d.display());
                std::process::exit(2)
            }
        }
    }
    std::process::exit(if print_report(&rows) { 0 } else { 1 });
}
