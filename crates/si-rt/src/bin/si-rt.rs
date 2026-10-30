// SPDX-License-Identifier: MIT
//! `si-rt`: finds and checks the user's program file, and prints our checksum of a range of it
//! (the number a crate writes in its `si_rt::Table`). It never prints the bytes themselves.
//!
//!   si-rt check [<install dir>]                 where the program is, its build and its segments
//!   si-rt sum <addr> <len> [<install dir>]      FNV-1a 64 of addr .. addr+len (hex or decimal), as a Rust literal
//!   si-rt sums <file> [<install dir>]           the same for each "<name> <addr> <len>" line of a file
//!
//! Exit status 2 when the install is missing or not the expected build (the message says why).
//! Our own code.

fn num(s: &str) -> Option<u32> {
    let s = s.replace('_', "");
    match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

fn lit(sum: u64) -> String {
    let h = format!("{sum:016x}");
    format!("0x{}_{}_{}_{}", &h[0..4], &h[4..8], &h[8..12], &h[12..16])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || -> ! {
        eprintln!("usage: si-rt check [<install dir>] | sum <addr> <len> [<install dir>] | sums <file> [<install dir>]");
        std::process::exit(64)
    };
    let root_at = |i: usize| {
        if let Some(r) = args.get(i) {
            si_rt::add_root(r);
        }
    };
    match args.first().map(String::as_str) {
        Some("check") => {
            root_at(1);
            let p = si_rt::check_or_exit("si-rt");
            println!("program: {}", p.path.display());
            println!("build:   {}", p.build.map(|b| b.name).unwrap_or("(unchecked)"));
            for s in &p.image().segments {
                println!(
                    "segment: 0x{:08x} .. 0x{:08x} in the file, .. 0x{:08x} in memory, flags {}",
                    s.vaddr,
                    s.vaddr as u64 + s.filesz as u64,
                    s.vaddr as u64 + s.memsz as u64,
                    s.flags
                );
            }
        }
        Some("sum") => {
            let (Some(a), Some(l)) = (args.get(1).and_then(|s| num(s)), args.get(2).and_then(|s| num(s))) else { usage() };
            root_at(3);
            let p = si_rt::check_or_exit("si-rt");
            match p.bytes("sum", a, l) {
                Ok(b) => println!("0x{a:08x} {l} {}", lit(si_rt::fnv1a64(b))),
                Err(e) => {
                    eprintln!("si-rt: {e}");
                    std::process::exit(1)
                }
            }
        }
        Some("sums") => {
            let Some(f) = args.get(1) else { usage() };
            root_at(2);
            let p = si_rt::check_or_exit("si-rt");
            let text = std::fs::read_to_string(f).unwrap_or_else(|e| {
                eprintln!("si-rt: {f}: {e}");
                std::process::exit(1)
            });
            let mut bad = 0;
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
                let w: Vec<&str> = line.split_whitespace().collect();
                let (Some(name), Some(a), Some(l)) = (w.first(), w.get(1).and_then(|s| num(s)), w.get(2).and_then(|s| num(s))) else {
                    eprintln!("si-rt: want '<name> <addr> <len>': {line}");
                    bad += 1;
                    continue;
                };
                match p.bytes(name, a, l) {
                    Ok(b) => println!("{name} 0x{a:08x} {l} {}", lit(si_rt::fnv1a64(b))),
                    Err(e) => {
                        println!("{name} 0x{a:08x} {l} ERROR {e}");
                        bad += 1;
                    }
                }
            }
            std::process::exit(if bad > 0 { 1 } else { 0 })
        }
        _ => usage(),
    }
}
