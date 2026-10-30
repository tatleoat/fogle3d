// SPDX-License-Identifier: MIT
//! `printf-check FILE`: the cases of `re/tools/si-mental/printf_cases.py` (a format, its argument words, the host C library's
//! text) against `si_mental::cprintf`. Prints the count of identical and different cases and the first differences.

use si_mental::cprintf;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("printf-check FILE");
    let text = std::fs::read_to_string(&path).expect("read cases");
    let (mut same, mut diff) = (0, 0);
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (fmt, args, want) = (unhex(f[0]), f[1], unhex(f[2]));
        let mut m = si_mem::Mem::new();
        let mut words: Vec<u32> = Vec::new();
        for t in args.split_whitespace() {
            let (k, v) = t.split_once(':').unwrap();
            match k {
                "i" => words.push(u32::from_str_radix(v, 16).unwrap()),
                "q" | "d" => {
                    let b = u64::from_str_radix(v, 16).unwrap();
                    words.push(b as u32);
                    words.push((b >> 32) as u32);
                }
                "s" => {
                    let s = unhex(v);
                    let p = m.malloc(s.len() as u32 + 1);
                    m.set_bytes(p, &s);
                    m.set_u8(p + s.len() as u32, 0);
                    words.push(p);
                }
                _ => words.push(0),
            }
        }
        let got = cprintf::format_bytes(&mut m, &fmt, &words);
        if got == want {
            same += 1;
        } else {
            diff += 1;
            if diff <= 15 {
                println!("DIFF fmt={:?} args={} want={:?} got={:?}", String::from_utf8_lossy(&fmt), args, String::from_utf8_lossy(&want), String::from_utf8_lossy(&got));
            }
        }
    }
    println!("printf-check: {same} identical, {diff} different of {}", same + diff);
}
