// SPDX-License-Identifier: MIT
//! The splines and patches of the sample model files as `si-hrc`'s image readers leave
//! them in memory, written out for `re/tools/si-spline/emu_gen5.py`, which makes the
//! emulator's cases from them (finding 0020, section 26). The readers' three calls into
//! this package (`SI_splineUpdateMatrix`, `SI_allDummy`, `SI_patchUpdateMatrix`) are
//! answered by its image forms.
//!
//!   spline-samples <model directory> <output file>
//!
//! One line per object: `S <file> <node> <which> <record> <key block> <name>` for a
//! spline (`which` -1 for a spline node, 0 for a face's outline, k + 1 for its hole k) and
//! `P <file> <node> <record> <points> <tags>` for a patch; each field the bytes in hex.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use si_hrc::image::outside::{MemOutside, Outside as HrcOutside, Routine};
use si_mem::{Mem, Ptr32};
use si_spline::spline_image::{self, NoOutside, SI_GRAF_LISTS};
use si_types::model::Model;

/// `si-hrc`'s readers' outside world, with this package's three routines answered.
struct World {
    inner: MemOutside,
}

impl HrcOutside for World {
    fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        self.inner.fopen(m, path, mode)
    }
    fn fclose(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.inner.fclose(m, file)
    }
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        self.inner.fread(m, ptr, size, n, file)
    }
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.inner.fgetc(m, file)
    }
    fn rewind(&mut self, m: &mut Mem, file: u32) {
        self.inner.rewind(m, file)
    }
    fn bytes_left(&mut self, m: &mut Mem, file: u32) -> u64 {
        self.inner.bytes_left(m, file)
    }
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        match routine.addr {
            0x0848_abd0 => spline_image::spline_update_matrix(m, Ptr32::new(args[0])),
            0x0848_e2f0 => si_spline::convert_image::all_dummy(m, &mut NoOutside, Ptr32::new(args[0])),
            0x0847_a680 => si_spline::patch_eval_image::patch_update_matrix(m, Ptr32::new(args[0])),
            _ => return self.inner.call(m, routine, args),
        }
        0
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "hrc") => out.push(p),
            _ => {}
        }
    }
}

fn hex(m: &Mem, a: u32, n: u32) -> String {
    let mut s = String::with_capacity(2 * n as usize);
    match m.try_bytes(a, n) {
        Some(b) => b.iter().for_each(|x| {
            let _ = write!(s, "{x:02x}");
        }),
        None => s.push('-'),
    }
    if s.is_empty() {
        s.push('.');
    }
    s
}

fn spline_line(m: &Mem, file: &str, node: usize, which: i32, s: u32) -> String {
    let nb = m.u16(s + 0x10) as u32;
    let keys = m.u32(s + 0x14);
    let room = if keys == 0 { 0 } else { m.block_size(keys).map_or(nb.div_ceil(16) * 16, |b| b / 0x30) };
    let name = m.u32(s);
    let name_hex = if name == 0 { ".".to_string() } else { hex(m, name, m.cstr(name).len() as u32 + 1) };
    format!("S {file} {node} {which} {} {} {name_hex}\n", hex(m, s, 0x74), if keys == 0 { ".".into() } else { hex(m, keys, room * 0x30) })
}

fn main() {
    let child = std::thread::Builder::new().stack_size(256 << 20).spawn(run).unwrap();
    std::process::exit(child.join().unwrap());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: spline-samples <model directory> <output file>");
        return 2;
    }
    let mut files = Vec::new();
    walk(Path::new(&args[0]), &mut files);
    let mut out = String::new();
    let (mut nfiles, mut nsplines, mut npatches, mut refused) = (0, 0, 0, 0);
    for path in &files {
        let Ok(data) = std::fs::read(path) else { continue };
        let rel = path.strip_prefix(&args[0]).unwrap_or(path).display().to_string().replace(' ', "%20");
        let mut m = Mem::program();
        si_hrc::image::init(&mut m);
        spline_image::init(&mut m);
        let lists = m.calloc(1, 0x24);
        m[SI_GRAF_LISTS] = lists;
        let mut o = World { inner: MemOutside::default() };
        m.on_fault(si_mem::OnFault::Stop);
        let root = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let file = o.inner.files.open_bytes(b"/corpus/element.hrc", data.clone());
            si_hrc::image::node::fdmodel_binary_read(&mut m, &mut o, file, 0, 0)
        }));
        m.on_fault(si_mem::OnFault::Record);
        let Ok(root) = root else {
            refused += 1;
            continue;
        };
        if root == 0 || m.fault().is_some() {
            refused += 1;
            continue;
        }
        nfiles += 1;
        // the nodes in the loader's order: a node, its first son's subtree, its next brother
        let mut stack = vec![Ptr32::<Model>::new(root)];
        let mut node = 0usize;
        while let Some(n) = stack.pop() {
            if n.is_null() || node > 100_000 {
                continue;
            }
            let (kind, def) = (m[n].kind, m[n].definition.addr());
            stack.push(m[n].brother);
            stack.push(m[n].son);
            if def != 0 {
                match kind {
                    6 => {
                        out.push_str(&spline_line(&m, &rel, node, -1, def));
                        nsplines += 1;
                    }
                    2 if m.i16(def + 0x84) != 0 => {
                        out.push_str(&spline_line(&m, &rel, node, 0, def + 4));
                        nsplines += 1;
                        let holes = m.u32(def + 0x7c);
                        for k in 0..m.i16(def + 0x78).max(0) as u32 {
                            out.push_str(&spline_line(&m, &rel, node, k as i32 + 1, holes + 0x74 * k));
                            nsplines += 1;
                        }
                    }
                    1 => {
                        let n_pts = m.u16(def + 0xc) as u32 * m.u16(def + 0xe) as u32;
                        let (pts, tags) = (m.u32(def + 0x1a4), m.u32(def + 0x1a8));
                        let p = if pts == 0 { ".".into() } else { hex(&m, pts, 16 * n_pts) };
                        let t = if tags == 0 { ".".into() } else { hex(&m, tags, 2 * n_pts) };
                        let _ = writeln!(out, "P {rel} {node} {} {p} {t}", hex(&m, def, 0x204));
                        npatches += 1;
                    }
                    _ => {}
                }
            }
            node += 1;
        }
    }
    if let Err(e) = std::fs::write(&args[1], out) {
        eprintln!("{}: {e}", args[1]);
        return 1;
    }
    println!("{} model files, {nfiles} read into the image, {refused} refused; {nsplines} splines, {npatches} patches", files.len());
    0
}
