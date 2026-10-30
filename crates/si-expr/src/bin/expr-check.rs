// SPDX-License-Identifier: MIT
//! Runs the expression code over a directory of original files and reports counts.
//!
//!   expr-check <dir> [Relations.rsrc]          every expression file under <dir>
//!   expr-check <dir> [Relations.rsrc] show N   also print the first N differing blocks
//!
//! Expression files are found by their first four bytes (`SEXP`). Each block is read
//! (checked, then built into a tree), written back with the original's writer and compared
//! with its bytes in the file; then the whole file is written and compared.

use si_expr::env::Rsrc;
use si_expr::io;
use si_expr::parse::Parser;
use si_expr::tree::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_FILE: u64 = 64 << 20;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
        if md.is_dir() {
            walk(&p, out);
        } else if md.is_file() && md.len() >= 4 && md.len() <= MAX_FILE {
            out.push(p);
        }
    }
}

fn starts_with_tag(p: &Path, tag: &[u8; 4]) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).is_ok() && &b == tag
}

/// The byte ranges of the blocks of a file: each starts at the line break before a line
/// that begins with `EXPRESSION ` and runs to the next one or to the end.
fn block_spans(data: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let pat = b"\nEXPRESSION ";
    let mut i = 0;
    while i + pat.len() <= data.len() {
        if &data[i..i + pat.len()] == pat {
            starts.push(i);
            i += pat.len();
        } else {
            i += 1;
        }
    }
    let mut spans = Vec::new();
    for (k, &s) in starts.iter().enumerate() {
        spans.push((s, starts.get(k + 1).copied().unwrap_or(data.len())));
    }
    spans
}

fn main() {
    // the parser's tables are read from the user's install (docs/design/runtime-data.md)
    si_rt::check_or_exit("expr-check");
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: expr-check <dir> [Relations.rsrc] [show N]");
        std::process::exit(2);
    }
    let dir = Path::new(&args[1]);
    let mut files = Vec::new();
    walk(dir, &mut files);
    let show: usize = args.iter().position(|a| a == "show").and_then(|i| args.get(i + 1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    let rsrc_path = args
        .get(2)
        .filter(|a| a.as_str() != "show")
        .map(PathBuf::from)
        .or_else(|| files.iter().find(|p| p.file_name().is_some_and(|n| n == "Relations.rsrc")).cloned());
    let Some(rsrc_path) = rsrc_path else {
        eprintln!("no Relations.rsrc under {}", dir.display());
        std::process::exit(2);
    };
    let dict = Rsrc::read(&std::fs::read(&rsrc_path).expect("read Relations.rsrc")).expect("parse Relations.rsrc");
    println!("dictionaries from {}", rsrc_path.display());
    let dups = dict.duplicate_ids();
    println!("fcurve index: {} entries, {} ids carried by more than one entry", dict.fcurves().count(), dups.len());
    for (id, names) in &dups {
        println!("  id {id}: {}", names.join(" "));
    }

    let mut n_files = 0;
    let mut distinct = std::collections::BTreeSet::new();
    let (mut blocks, mut parsed, mut failed, mut same_block, mut diff_block) = (0, 0, 0, 0, 0);
    let (mut same_file, mut diff_file, mut stuck) = (0, 0, 0);
    let mut messages: BTreeMap<String, usize> = BTreeMap::new();
    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    let mut diff_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    let mut stdout_lines = 0usize;
    let mut distinct_blocks = std::collections::BTreeSet::new();
    let mut distinct_same = std::collections::BTreeSet::new();
    let mut distinct_trees: BTreeMap<Vec<u8>, Expression> = BTreeMap::new();
    for p in &files {
        if !starts_with_tag(p, b"SEXP") {
            continue;
        }
        let data = std::fs::read(p).unwrap();
        n_files += 1;
        distinct.insert(data.clone());
        let line_end = data.iter().position(|&c| c == b'\n').unwrap_or(data.len());
        let ver = String::from_utf8_lossy(&data[4..line_end]).trim().to_string();
        *versions.entry(ver.clone()).or_default() += 1;
        let mut parser = Parser::new();
        let read = io::read_expressions(&data, &mut parser, &dict, &mut |_| true);
        stdout_lines += parser.stdout.iter().filter(|&&c| c == b'\n').count();
        let list = match read {
            Ok(l) => l,
            Err(e) => {
                stuck += 1;
                println!("{}: {:?}", p.display(), e);
                continue;
            }
        };
        let spans = block_spans(&data);
        if spans.len() != list.len() {
            println!("{}: {} EXPRESSION lines, {} blocks read", p.display(), spans.len(), list.len());
        }
        let mut whole: Vec<(&[u8], &si_expr::tree::Expression)> = Vec::new();
        for (k, (name, block)) in list.iter().enumerate() {
            blocks += 1;
            let orig = spans.get(k).map(|&(a, b)| &data[a..b]);
            if let Some(o) = orig {
                distinct_blocks.insert(o.to_vec());
            }
            match &block.expr {
                None => {
                    failed += 1;
                    let (var, at, msg) = block.error.clone().unwrap_or((-2, 0, String::new()));
                    *messages.entry(msg.clone()).or_default() += 1;
                    if shown < show {
                        shown += 1;
                        println!("FAILED {} {}: var {var} at {at}: {msg}\n  text: {}", p.display(), String::from_utf8_lossy(name), String::from_utf8_lossy(&block.text));
                    }
                }
                Some(expr) => {
                    parsed += 1;
                    whole.push((name, expr));
                    let mut mine = b"\nEXPRESSION ".to_vec();
                    mine.extend_from_slice(name);
                    mine.extend_from_slice(b" \n");
                    match io::expression_write(&mut mine, &dict, expr) {
                        Ok(()) => {}
                        Err(e) => {
                            *diff_kinds.entry(format!("writer: {e:?}")).or_default() += 1;
                            diff_block += 1;
                            continue;
                        }
                    }
                    if orig == Some(&mine[..]) {
                        same_block += 1;
                        // without the line that carries the element's name
                        let body = mine[mine[1..].iter().position(|&c| c == b'\n').map_or(0, |i| i + 1)..].to_vec();
                        distinct_trees.insert(body, expr.clone());
                        distinct_same.insert(mine.clone());
                    } else {
                        diff_block += 1;
                        let o = orig.unwrap_or(b"");
                        let kind = classify(o, &mine);
                        *diff_kinds.entry(kind).or_default() += 1;
                        if shown < show {
                            shown += 1;
                            println!("DIFF {} {}\n--- original\n{}\n--- ours\n{}", p.display(), String::from_utf8_lossy(name), String::from_utf8_lossy(o), String::from_utf8_lossy(&mine));
                        }
                    }
                }
            }
        }
        let version: f32 = ver.parse().unwrap_or(-1.0);
        match io::write_expressions(&dict, version, &whole) {
            Ok(out) if out == data => same_file += 1,
            _ => diff_file += 1,
        }
    }
    println!("expression files: {n_files} ({} distinct contents); versions {:?}", distinct.len(), versions);
    println!("blocks: {blocks} ({} distinct texts); read into a tree: {parsed}; rejected by the check: {failed}; files the original would not finish: {stuck}", distinct_blocks.len());
    for (m, n) in &messages {
        println!("  rejected with \"{m}\": {n}");
    }
    println!("lines the original would print on standard output while reading: {stdout_lines}");
    println!("blocks written back identical: {same_block} ({} distinct); different: {diff_block}", distinct_same.len());
    for (k, n) in &diff_kinds {
        println!("  {k}: {n}");
    }
    println!("whole files written back identical: {same_file}; different: {diff_file}");
    println!("what the {} distinct trees are made of:", distinct_trees.len());
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in distinct_trees.values() {
        census(e.root.as_deref().and_then(|r| r.right.as_deref()), &mut counts);
        for v in &e.vars {
            census(v.expr.as_deref(), &mut counts);
        }
        *counts.entry(format!("blocks with {} variable(s)", e.vars.len())).or_default() += 1;
    }
    for (k, n) in &counts {
        println!("  {k}: {n}");
    }
    scenes(&files, &dict);
}

/// Counts what a tree is made of.
fn census(node: Option<&ExprNode>, counts: &mut BTreeMap<String, usize>) {
    let Some(node) = node else { return };
    let mut add = |k: String| *counts.entry(k).or_default() += 1;
    if node.is_paren() {
        add("parentheses".into());
    }
    if node.get_oper() != op::LEAF {
        let o = si_expr::format::get_expr_oper_str(node.get_oper());
        add(format!("operator {}", if o.is_empty() { if node.get_oper() == op::NEG { "unary -" } else { "!" } } else { o }));
        census(node.left.as_deref(), counts);
        census(node.right.as_deref(), counts);
        return;
    }
    let Some(leaf) = node.leaf.as_deref() else { return };
    match &leaf.data {
        NopData::Int(_) => add("integer constant".into()),
        NopData::Float(_) => add("real constant".into()),
        NopData::Text(_) if leaf.kind == nop::VAR => add("variable".into()),
        NopData::Text(_) => add("string".into()),
        NopData::Enum(_) => add("enumeration value".into()),
        NopData::Rand(r) => add(format!("random call form {}", r.form)),
        NopData::Arg(a) => {
            add(if leaf.kind == nop::CUSTOM_ARG { "reference to a custom parameter".to_string() } else { "reference".to_string() });
            if a.generic != 0 {
                add("reference to `this`".into());
            }
            if a.fcv == 0 {
                add("reference without a parameter".into());
            }
            if a.access0 == 1 && !matches!(a.fcv, 0x9a | 8 | 0xb7 | 0xb8) {
                add("reference marked global".into());
            }
            if matches!(a.fcv, 0x9a | 8 | 0xb7 | 0xb8) {
                add(format!("shape or cluster parameter {}", a.fcv));
            }
            if a.chapter != si_expr::token::chapter::MODELS {
                add(format!("reference in chapter {}", a.chapter));
            }
        }
        NopData::Fct(f) => {
            add(format!("function {}", si_expr::token::get_valid_fct(f.index)));
            for a in &f.args {
                census(a.as_deref(), counts);
            }
        }
        NopData::Channel(_) => add("channel".into()),
        NopData::None => add(format!("predefined word (kind {})", leaf.kind)),
    }
}

/// Every reference of a tree: (argument number, chapter, fcurve type, custom name).
fn references(node: Option<&ExprNode>, out: &mut Vec<(i32, i32, i32, Option<Vec<u8>>)>) {
    let Some(node) = node else { return };
    if node.get_oper() != op::LEAF {
        references(node.left.as_deref(), out);
        references(node.right.as_deref(), out);
        return;
    }
    match node.leaf.as_deref().map(|l| &l.data) {
        Some(NopData::Arg(a)) => out.push((a.num, a.chapter, a.fcv, a.custom_name.clone())),
        Some(NopData::Fct(f)) => f.args.iter().for_each(|a| references(a.as_deref(), out)),
        _ => {}
    }
}

/// Scene descriptions against their expression files: each relation 20010 names an element
/// of the EXPRESSIONS chapter, whose block must have the relation's parameter as its target
/// and one sub-relation per argument, in the order of the argument numbers.
fn scenes(files: &[PathBuf], dict: &Rsrc) {
    use si_ascii::reldata::RelData;
    let chapter_id = |name: &[u8]| dict.relations().find_chapter(name);
    let (mut n_dsc, mut with_chapter, mut n_elems, mut n_rel, mut no_file, mut no_block) = (0, 0, 0, 0, 0, 0);
    let (mut ok, mut resolved, mut no_args, mut with_elems) = (0, 0, 0, 0);
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    let mut subrel_hist: BTreeMap<usize, usize> = BTreeMap::new();
    for p in files {
        if !starts_with_tag(p, b"SDSC") {
            continue;
        }
        n_dsc += 1;
        let Ok((d, _)) = si_ascii::dsc::read(&std::fs::read(p).unwrap()) else { continue };
        let Some(chap) = d.chapters.iter().find(|c| c.name == b"EXPRESSIONS") else { continue };
        with_chapter += 1;
        n_elems += chap.elements.iter().filter(|e| e.not_root != 0).count();
        if !chap.elements.is_empty() {
            with_elems += 1;
        }
        // the file: the chapter's root element, in the EXPRESSIONS directory next to SCENES
        let mut blocks: BTreeMap<Vec<u8>, si_expr::io::Block> = BTreeMap::new();
        let mut found_file = false;
        for root in chap.elements.iter().filter(|e| e.not_root == 0) {
            let file = p.parent().and_then(|s| s.parent()).map(|db| db.join("EXPRESSIONS").join(format!("{}.exp", String::from_utf8_lossy(&root.name))));
            if let Some(Ok(data)) = file.map(std::fs::read) {
                found_file = true;
                let mut parser = Parser::new();
                if let Ok(list) = io::read_expressions(&data, &mut parser, dict, &mut |_| true) {
                    blocks.extend(list);
                }
            }
        }
        for block in &d.relations {
            if block.name2 != b"EXPRESSIONS" {
                continue;
            }
            for r in block.relations.iter().filter(|r| r.relid == 20010) {
                n_rel += 1;
                if !found_file {
                    no_file += 1;
                    continue;
                }
                let name = chap.elements.get(r.elem2 as usize).map(|e| e.name.clone()).unwrap_or_default();
                let Some(expr) = blocks.get(&name).and_then(|b| b.expr.as_ref()) else {
                    no_block += 1;
                    continue;
                };
                let mut bad = |k: &str| *problems.entry(k.to_string()).or_default() += 1;
                let mut fine = true;
                let Some(RelData::Expression(rd)) = &r.data else {
                    bad("relation without data");
                    continue;
                };
                let root = expr.root.as_deref().unwrap();
                let mut target = Vec::new();
                references(root.left.as_deref(), &mut target);
                match target.first() {
                    Some((num, chap, fcv, custom)) => {
                        // A block with an empty argument list is not read in "loading mode":
                        // its target takes the running number 1, and the relation's single
                        // sub-relation is the target itself.
                        let own = r.subrels.as_ref().is_some_and(|s| s.len() == 1 && s[0].elem == r.elem1 && s[0].chapter == chapter_id(&block.name1));
                        if *num == 1 && blocks.get(&name).is_some_and(|b| b.args.is_none()) && own {
                            no_args += 1;
                        } else if *num != 0 {
                            bad("target is not argument 0");
                            fine = false;
                        }
                        if *fcv != rd.fcurve || (*fcv == 566 && *custom != rd.name) {
                            bad("target parameter differs from the relation's");
                            fine = false;
                        }
                        if *chap != chapter_id(&block.name1) {
                            bad("target chapter differs from the relation's");
                            fine = false;
                        }
                    }
                    None => {
                        bad("block without a target");
                        fine = false;
                    }
                }
                let subs = r.subrels.clone().unwrap_or_default();
                *subrel_hist.entry(subs.len()).or_default() += 1;
                let mut refs = Vec::new();
                references(root.right.as_deref(), &mut refs);
                for v in &expr.vars {
                    references(v.expr.as_deref(), &mut refs);
                }
                let max = refs.iter().map(|r| r.0).max().unwrap_or(0) as usize;
                // an expression without arguments has one sub-relation: the target itself
                if !(max == subs.len() || (max == 0 && subs.len() == 1)) {
                    bad("number of sub-relations differs from the highest argument number");
                    fine = false;
                }
                for (num, chap, fcv, custom) in &refs {
                    let Some(sub) = subs.get((*num as usize).wrapping_sub(1)) else { continue };
                    let Some(RelData::Expression(sd)) = &sub.data else {
                        bad("sub-relation without data");
                        fine = false;
                        continue;
                    };
                    if sd.fcurve != *fcv || (*fcv == 566 && sd.name != *custom) {
                        bad("argument parameter differs from its sub-relation's");
                        fine = false;
                    }
                    if sub.chapter != *chap {
                        bad("argument chapter differs from its sub-relation's");
                        fine = false;
                    }
                }
                if fine {
                    ok += 1;
                }
                // tie the references to the elements, as the loader does
                let mut list = vec![si_expr::util::RelArg { chapter: chapter_id(&block.name1), elem: r.elem1, fcv: rd.fcurve }];
                list.extend(subs.iter().map(|s| si_expr::util::RelArg { chapter: s.chapter, elem: s.elem, fcv: 0 }));
                let mut e = expr.clone();
                let mut status = Vec::new();
                let mut all = true;
                for v in e.vars.iter_mut() {
                    all &= si_expr::util::expr_resolve_relation(v.expr.as_deref_mut(), &list, &mut |_, _| None, &mut status).is_ok();
                }
                all &= si_expr::util::expr_resolve_relation(e.root.as_deref_mut(), &list, &mut |_, _| None, &mut status).is_ok();
                if all {
                    resolved += 1;
                }
            }
        }
    }
    println!("scene descriptions: {n_dsc}; with an EXPRESSIONS chapter: {with_chapter}; with elements in it: {with_elems}, holding {n_elems} expression elements");
    println!("of these relations, for a block with an empty argument list (target numbered 1, its own sub-relation): {no_args}");
    println!("relation 20010 lines toward EXPRESSIONS: {n_rel}; expression file missing: {no_file}; block missing or rejected: {no_block}");
    println!("relations whose block agrees in target, parameters, chapters and argument count: {ok}");
    println!("relations whose references could all be tied to elements: {resolved}");
    println!("sub-relations per relation: {subrel_hist:?}");
    for (k, n) in &problems {
        println!("  {k}: {n}");
    }
}

/// A rough label for how a rewritten block differs from the original.
fn classify(orig: &[u8], mine: &[u8]) -> String {
    let squeeze = |s: &[u8]| -> Vec<u8> { s.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect() };
    if orig.contains(&b'\r') {
        return "original has carriage returns".into();
    }
    if squeeze(orig) == squeeze(mine) {
        return "blanks only".into();
    }
    let lines = |s: &[u8]| s.split(|&c| c == b'\n').map(|l| l.to_vec()).collect::<Vec<_>>();
    let (lo, lm) = (lines(orig), lines(mine));
    if lo.len() != lm.len() {
        return "different number of lines".into();
    }
    let which: Vec<&str> = lo
        .iter()
        .zip(&lm)
        .filter(|(a, b)| a != b)
        .map(|(a, _)| {
            let t = String::from_utf8_lossy(a);
            if t.contains("return(") {
                "return line"
            } else if t.contains("ret_el") {
                "header line"
            } else if t.contains(" = ") {
                "variable line"
            } else if t.contains("arg") || t.contains("this") {
                "argument list"
            } else {
                "other line"
            }
        })
        .collect();
    let mut w = which.clone();
    w.dedup();
    w.join(" + ")
}
