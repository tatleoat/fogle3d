// SPDX-License-Identifier: MIT
use crate::check::{check_case, Config, Mutate, Outcome, UNPORTED};
use crate::outside::{Hooks, StackArgs};
use crate::{Effect, Port, Route, StandIn};
use si_mem::Mem;
use si_x87::vectors::Case;
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::PathBuf;

/// A crate's check: its ports and what they need besides the generic outside world.
pub struct Kit {
    pub(crate) ports: Vec<(&'static str, Port)>,
    pub(crate) hooks: Hooks,
    pub(crate) setup: Option<fn(&mut Mem)>,
    pub(crate) allowed_globals: Vec<&'static str>,
    pub(crate) given_lib_column: bool,
    pub(crate) stack_args: StackArgs,
    pub(crate) scratch: Vec<(u32, u32)>,
}

impl Kit {
    pub fn new(ports: &[(&'static str, Port)]) -> Kit {
        Kit { ports: ports.to_vec(), hooks: Hooks::default(), setup: None, allowed_globals: Vec::new(), given_lib_column: false, stack_args: StackArgs::Exact, scratch: Vec::new() }
    }

    /// Runs on every fresh image before the case is loaded: the crate's globals.
    pub fn setup(mut self, f: fn(&mut Mem)) -> Kit {
        self.setup = Some(f);
        self
    }

    /// A stand-in for the callee `name`, answered without the case's recorded call.
    pub fn stand_in(mut self, name: &'static str, f: StandIn) -> Kit {
        self.hooks.stand_ins.push((name, f));
        self
    }

    /// Routes callees to the crate's own Rust: `None` leaves the call to the recorded answer.
    pub fn route(mut self, f: Route) -> Kit {
        self.hooks.route = Some(f);
        self
    }

    /// What a recorded stand-in did to the image, held against the answer the original got.
    pub fn effect(mut self, f: Effect) -> Kit {
        self.hooks.effect = Some(f);
        self
    }

    /// A `GW:` note (the original wrote a global the case does not declare) that is not a
    /// reason to skip the case.
    pub fn allow_global_write(mut self, note: &'static str) -> Kit {
        self.allowed_globals.push(note);
        self
    }

    /// A recorded argument that is a stack address (the original passed the address of a local
    /// of its own, not marked `57ac57ac`) is matched by any stack address of the port within
    /// 64 KiB below its entry stack pointer, and the `NAME.out(...)` words the original's
    /// callee stored through it go to the port's local. Ports then need no `m.local(pad)`.
    /// The default is the exact address. (`57ac57ac` arguments are matched either way.)
    pub fn normalise_stack_args(self) -> Kit {
        self.normalise_stack_args_within(0x1_0000)
    }

    /// [`Kit::normalise_stack_args`] with the frame window in bytes.
    pub fn normalise_stack_args_within(mut self, frame: u32) -> Kit {
        self.stack_args = StackArgs::Normalised { frame };
        self
    }

    /// Globals (statics the allocators bump, scratch statics) that both sides start every
    /// case with at a stated value: `(address, initial word)`. The port's image holds the
    /// value after `setup` and before the case's own `gXXXXXXXX` fields load (which win); the
    /// original's side is the generator's (`#glob` in a spec starts them at the same value).
    /// What the port left in them is put back after its run, so they are neither compared nor
    /// reported as a stray write.
    pub fn reset_globals(mut self, globals: &[(u32, u32)]) -> Kit {
        self.scratch.extend_from_slice(globals);
        self
    }

    /// Prints the `given-lib` column (cases that agree given the C library's answers, `NA:`).
    pub fn given_lib_column(mut self) -> Kit {
        self.given_lib_column = true;
        self
    }

    /// Reads the command line and the environment and runs; exits.
    pub fn run(self) -> ! {
        let mut cfg = Config { mutate: Mutate::from_env(), ..Config::default() };
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--only" => cfg.only = Some(it.next().expect("--only NAME")),
                "--first-diff" => cfg.first_diff = true,
                "--lenient" => cfg.lenient = true,
                "-h" | "--help" => usage(0),
                s if s.starts_with("--") => usage(2),
                _ => paths.push(PathBuf::from(a)),
            }
        }
        if paths.is_empty() {
            usage(2);
        }
        let ok = self.run_paths(&cfg, &paths);
        std::process::exit(if ok { 0 } else { 1 })
    }

    /// The report for the vector files under `paths` (files, or directories of `.vec` files).
    pub fn run_paths(&self, cfg: &Config, paths: &[PathBuf]) -> bool {
        let mut files: Vec<PathBuf> = Vec::new();
        for p in paths {
            if p.is_dir() {
                let mut v: Vec<PathBuf> = std::fs::read_dir(p).expect("the directory").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "vec")).collect();
                v.sort();
                files.extend(v);
            } else {
                files.push(p.clone());
            }
        }
        std::panic::set_hook(Box::new(|_| {}));
        let g = self.given_lib_column;
        let header = |t: &mut String| {
            *t = if g {
                format!("{:<44} {:>6} {:>9} {:>9} {:>8} {:>9} {:>8}", "function", "cases", "identical", "given-lib", "defined", "different", "not-comp")
            } else {
                format!("{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}", "function", "cases", "identical", "defined", "different", "not-comp")
            };
        };
        let mut h = String::new();
        header(&mut h);
        println!("{h}");
        let line = |name: &str, t: &Tally| {
            if g {
                format!("{:<44} {:>6} {:>9} {:>9} {:>8} {:>9} {:>8}", name, t.cases, t.identical, t.given_lib, t.defined_part, t.different, t.skipped)
            } else {
                format!("{:<44} {:>6} {:>9} {:>8} {:>9} {:>8}", name, t.cases, t.identical, t.defined_part, t.different, t.skipped)
            }
        };
        let mut total = Tally::default();
        let mut used = 0;
        let mut all_reasons: BTreeMap<&'static str, u32> = BTreeMap::new();
        for path in &files {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            if cfg.only.as_deref().is_some_and(|o| o != stem) {
                continue;
            }
            used += 1;
            let row = self.run_file(cfg, path);
            println!("{}", line(&stem, &row));
            for e in &row.examples {
                println!("    {e}");
            }
            total.add(&row);
            for (k, v) in row.reasons {
                *all_reasons.entry(k).or_default() += v;
            }
        }
        println!("{}", line(&format!("TOTAL ({used} files)"), &total));
        for (why, n) in &all_reasons {
            println!("  not compared, {why}: {n}");
        }
        UNPORTED.with(|u| {
            for (name, n) in u.borrow().iter() {
                println!("  callee answered 0 ({n} calls, not ported or not recorded): {name}");
            }
        });
        total.different == 0
    }

    fn run_file(&self, cfg: &Config, path: &PathBuf) -> Tally {
        let mut row = Tally::default();
        let file = std::io::BufReader::new(std::fs::File::open(path).expect("a vector file"));
        for (number, line) in file.lines().enumerate() {
            let line = line.expect("a line");
            let case = match Case::parse(&line) {
                Ok(Some(c)) => c,
                Ok(None) => continue,
                Err(e) => panic!("{}:{}: {e}", path.display(), number + 1),
            };
            row.cases += 1;
            let outcome = check_caught(self, cfg, &case, number as u32);
            row.count(&case, outcome, number + 1, &line, cfg.first_diff);
        }
        row
    }
}

/// [`check_case`] with a panic of the port (or a fault the image turned into one) as a
/// difference.
pub fn check_caught(kit: &Kit, cfg: &Config, case: &Case, number: u32) -> Outcome {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_case(kit, cfg, case, number))).unwrap_or_else(|p| {
        let text = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
        match p.downcast_ref::<si_mem::Stop>() {
            Some(s) => Outcome::Different(format!("the port stopped at a fault: {:?}", s.0)),
            None => Outcome::Different(format!("the port panicked: {}", text.unwrap_or_default())),
        }
    })
}

fn usage(code: i32) -> ! {
    eprintln!("usage: <check> [--only NAME] [--first-diff] [--lenient] <vector file or directory>...\n  EMUCHECK_MUTATE=1      flip a bit of the port's first value (every case must then differ)\n  EMUCHECK_MUTATE=calls  flip a bit of the first argument of every call");
    std::process::exit(code)
}

/// The numbers of one file or of the run.
#[derive(Default, Debug)]
pub struct Tally {
    pub cases: u32,
    pub identical: u32,
    pub given_lib: u32,
    pub defined_part: u32,
    pub different: u32,
    pub skipped: u32,
    pub reasons: BTreeMap<&'static str, u32>,
    pub examples: Vec<String>,
}

impl Tally {
    /// Counts an outcome in the column it belongs to.
    pub fn count(&mut self, case: &Case, outcome: Outcome, number: usize, line: &str, all_examples: bool) {
        match outcome {
            Outcome::Same if case.has_note("UNINIT") => self.defined_part += 1,
            Outcome::Same if case.has_note("NA") => self.given_lib += 1,
            Outcome::Same => self.identical += 1,
            Outcome::Skip(why) => {
                self.skipped += 1;
                *self.reasons.entry(why).or_default() += 1;
            }
            Outcome::Different(d) => {
                self.different += 1;
                if self.examples.len() < if all_examples { 50 } else { 2 } {
                    let mut text = line.to_string();
                    if text.len() > 700 {
                        text.truncate(700);
                        text.push('…');
                    }
                    self.examples.push(format!("line {number}: {d}\n      {text}"));
                }
            }
        }
    }

    fn add(&mut self, o: &Tally) {
        self.cases += o.cases;
        self.identical += o.identical;
        self.given_lib += o.given_lib;
        self.defined_part += o.defined_part;
        self.different += o.different;
        self.skipped += o.skipped;
    }
}

/// `run(&[("SI_name", |m, o, a| …), …])`: the whole of a check binary.
pub fn run(ports: &[(&'static str, Port)]) -> ! {
    Kit::new(ports).run()
}
