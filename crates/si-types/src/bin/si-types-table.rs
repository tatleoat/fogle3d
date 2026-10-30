// SPDX-License-Identifier: MIT
//! Prints the field tables of every structure, and the share of each structure's bytes that
//! is read, named or unknown, as Markdown (the tables of finding 0010 are this output).
//!   cargo run -p si-types --bin si-types-table            every table
//!   cargo run -p si-types --bin si-types-table -- summary  the percentages only
//!   cargo run -p si-types --bin si-types-table -- Elem     one structure

use si_types::{all_layouts, Confidence, Layout};

fn pct(n: usize, of: usize) -> String {
    format!("{:.1}%", 100.0 * n as f64 / of as f64)
}

fn summary(layouts: &[Layout]) {
    println!("| Structure | Original | Size | Fields | Read | Named | Unknown |");
    println!("|---|---|---|---|---|---|---|");
    for l in layouts {
        let (r, n, u) = l.bytes();
        println!(
            "| `{}` | `{}` | 0x{:x} ({}) | {} | {} | {} | {} |",
            l.name, l.original, l.size, l.size, l.fields.len(), pct(r, l.size), pct(n, l.size), pct(u, l.size)
        );
    }
}

fn table(l: &Layout) {
    let (r, n, u) = l.bytes();
    println!("### `{}` — `{}`, 0x{:x} bytes\n", l.name, l.original, l.size);
    println!("Size: {}. Read {} · named {} · unknown {}.\n", l.size_evidence, pct(r, l.size), pct(n, l.size), pct(u, l.size));
    println!("| Offset | Size | Type | Field | Confidence | Evidence |");
    println!("|---|---|---|---|---|---|");
    for f in l.fields {
        let c = match f.confidence {
            Confidence::Read => "read",
            Confidence::Named => "named",
            Confidence::Unknown => "unknown",
        };
        println!("| 0x{:03x} | {} | `{}` | `{}` | {} | {} |", f.offset, f.size, f.ty.replace('|', "\\|"), f.name, c, f.evidence);
    }
    println!();
}

fn main() {
    let layouts = all_layouts();
    match std::env::args().nth(1).as_deref() {
        None => {
            summary(&layouts);
            println!();
            layouts.iter().for_each(table);
        }
        Some("summary") => summary(&layouts),
        Some(name) => match layouts.iter().find(|l| l.name == name) {
            Some(l) => table(l),
            None => eprintln!("no structure named {name}"),
        },
    }
}
