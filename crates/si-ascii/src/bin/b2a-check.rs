// SPDX-License-Identifier: MIT
//! The port of `bin2ascii` against the original's own outputs (finding 0006, section 9).
//!
//!   b2a-check <oracle tools dir> <Relations.rsrc> [--out <dir>] [--list]
//!
//! `<oracle tools dir>` is `re/out/oracle/tools` (finding 0031, "Second pass" and "Third
//! pass"): its `bin2ascii/default` and `bin2ascii/k` hold what the original wrote for each of
//! the 3,365 sample binary files (`inputs/db/`), without and with `-k`; `bin2ascii/back` and
//! `bin2ascii/back-k` what it wrote when given those texts back (binary files again);
//! `bin2ascii/t3d-lower` and `t3d-lower-k` the 125 3D textures given under a lower-case
//! suffix, and `t3d-lower-back`, `t3d-lower-back-k` their texts given back. Each directory
//! has a manifest of the commands. For each command the port does what `main`
//! does, on the same names (`/in/` is the tools directory, as in the sandbox), and the
//! output (its name and its bytes), the exit status and what was printed are compared.
//! Counts per element kind; with `--out`, a table per variant and the port's output of
//! every file that differs.
use si_ascii::b2a::{self, CurveNames, Env, Host, Outcome, Run};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The runs of finding 0031: the second pass (`default`, `k`, and their texts given back),
/// with the third pass's 3D texture rows in them (the files named `….t3D` as in the install),
/// and the third pass's `t3d-lower` runs (the same 125 files given as `….t3d`, with the
/// `….t3D` beside them), and their texts given back. The flag is `-k`.
const VARIANTS: [(&str, bool); 8] = [
    ("default", false),
    ("k", true),
    ("back", false),
    ("back-k", true),
    ("t3d-lower", false),
    ("t3d-lower-k", true),
    ("t3d-lower-back", false),
    ("t3d-lower-back-k", true),
];

struct Sandbox {
    root: PathBuf,
}

impl Host for Sandbox {
    fn read(&self, path: &[u8]) -> Option<Vec<u8>> {
        let p = std::str::from_utf8(path).ok()?;
        let rel = p.strip_prefix("/in/")?;
        let full = self.root.join(rel);
        // the sandbox's file system is case-sensitive, and so is this one
        std::fs::read(full).ok()
    }
}

fn manifest(dir: &Path) -> Vec<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(dir.join("manifest.tsv")).expect("manifest.tsv");
    let mut lines = text.lines();
    let head: Vec<String> = lines.next().unwrap().split('\t').map(String::from).collect();
    lines.map(|l| head.iter().cloned().zip(l.split('\t').map(String::from)).collect()).collect()
}

/// Where two texts first differ, and the line around it, for the table.
fn first_diff(a: &[u8], b: &[u8]) -> (usize, String) {
    let n = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let line = |t: &[u8]| {
        let s = t[..n.min(t.len())].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
        let e = t[n.min(t.len())..].iter().position(|&c| c == b'\n').map_or(t.len(), |i| n + i);
        String::from_utf8_lossy(&t[s..e]).replace('\t', " ")
    };
    (n, format!("orig [{}] port [{}]", line(a), line(b)))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: b2a-check <oracle tools dir> <Relations.rsrc> [--out <dir>] [--list]");
        std::process::exit(2);
    }
    let tools = PathBuf::from(&args[1]);
    let rsrc = std::fs::read(&args[2]).expect("Relations.rsrc");
    let out = args.iter().position(|a| a == "--out").map(|i| PathBuf::from(&args[i + 1]));
    let list = args.iter().any(|a| a == "--list");
    let (rel, _) = si_ascii::srel::read(&rsrc).expect("the relations resources");
    let host = Sandbox { root: tools.clone() };
    let mut total: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
    for (variant, keep) in VARIANTS {
        let dir = tools.join("bin2ascii").join(variant);
        if !dir.join("manifest.tsv").exists() {
            println!("{variant:8} (no run in {})", dir.display());
            continue;
        }
        let mut rows = vec!["id\text\tclass\tdetail".to_string()];
        for m in manifest(&dir) {
            let id = &m["id"];
            let input = format!("/in/{}", m["input"]);
            let ext = Path::new(&m["input"]).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
            let mut words = m["arguments"].split(' ');
            let (_, outname) = (words.next(), words.next().unwrap_or(""));
            let orig_name = m["outputs"].split(',').find(|o| !o.is_empty()).map(|o| o.rsplit_once(':').unwrap().0.to_string());
            let orig_out: Option<Vec<u8>> = orig_name.as_ref().map(|n| std::fs::read(dir.join(n)).expect("output"));
            let orig_status: i32 = m["status"].parse().unwrap_or(-1);
            let orig_printed = std::fs::read(dir.join("printed").join(format!("{id}.txt"))).unwrap_or_default();
            // a fresh process for every command: the converter's statics start anew
            let mut env = Env { curves: CurveNames::new(&rel), now: 0, default_interpolation: 0 };
            let outcome = b2a::main(&host, &mut env, input.as_bytes(), outname.as_bytes(), keep);
            let (class, detail) = match (&outcome, &orig_out) {
                (Outcome::NotPorted(why), Some(_)) => ("port refuses (not ported)".to_string(), why.to_string()),
                (Outcome::NotPorted(why), None) => ("original refused; port: not ported".to_string(), why.to_string()),
                (Outcome::Done(run), o) => {
                    let (mut class, mut detail) = compare(run, o.as_deref(), orig_status, &orig_printed);
                    let port_name = String::from_utf8_lossy(&run.output_name).rsplit('/').next().unwrap_or("").to_string();
                    if let (Some(n), true) = (orig_name.as_ref(), run.output.is_some()) {
                        if *n != port_name {
                            class = format!("{class}, other file name");
                            detail = format!("{detail} (name {n} vs {port_name})");
                        }
                    }
                    (class, detail)
                }
            };
            if let (Some(dir), Outcome::Done(Run { output: Some(text), .. })) = (out.as_ref(), &outcome) {
                if class != "identical" {
                    let d = dir.join("port").join(variant);
                    std::fs::create_dir_all(&d).unwrap();
                    std::fs::write(d.join(format!("{id}.{ext}")), text).unwrap();
                }
            }
            if list && class != "identical" {
                println!("{variant}\t{id}\t{ext}\t{class}\t{detail}");
            }
            *total.entry((variant.to_string(), ext.clone())).or_default().entry(class.clone()).or_default() += 1;
            rows.push(format!("{id}\t{ext}\t{class}\t{detail}"));
        }
        if let Some(dir) = out.as_ref() {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("compare.{variant}.tsv")), rows.join("\n") + "\n").unwrap();
        }
    }
    let mut sums: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for ((variant, ext), classes) in &total {
        let line: Vec<String> = classes.iter().map(|(c, n)| format!("{c}: {n}")).collect();
        println!("{variant:8} .{ext:4} {}", line.join(", "));
        for (c, n) in classes {
            *sums.entry(variant.clone()).or_default().entry(c.clone()).or_default() += n;
        }
    }
    for (variant, classes) in &sums {
        let line: Vec<String> = classes.iter().map(|(c, n)| format!("{c}: {n}")).collect();
        println!("{variant:8} all   {}", line.join(", "));
    }
}

/// The class of one command: the text, then the exit status and the messages.
fn compare(run: &Run, orig: Option<&[u8]>, orig_status: i32, orig_printed: &[u8]) -> (String, String) {
    let mut printed = run.stdout.clone();
    printed.extend_from_slice(&run.stderr);
    let side = if run.status != orig_status || printed != orig_printed {
        format!(" (status {} vs {}, messages {})", run.status, orig_status, if printed == orig_printed { "same" } else { "differ" })
    } else {
        String::new()
    };
    match (orig, run.output.as_deref()) {
        (Some(a), Some(b)) if a == b => {
            if side.is_empty() {
                ("identical".into(), String::new())
            } else {
                ("same text, other status or messages".into(), side)
            }
        }
        (Some(a), Some(b)) => {
            let (at, what) = first_diff(a, b);
            ("differing".into(), format!("at {at} of {} / {} bytes: {what}{side}", a.len(), b.len()))
        }
        (Some(_), None) => ("port refuses".into(), format!("{}{side}", String::from_utf8_lossy(&printed).trim_end())),
        (None, None) => {
            if side.is_empty() {
                ("original refused; port refuses the same way".into(), String::from_utf8_lossy(orig_printed).trim_end().to_string())
            } else {
                ("original refused; port refuses otherwise".into(), side)
            }
        }
        (None, Some(_)) => ("original refused; port writes".into(), side),
    }
}
