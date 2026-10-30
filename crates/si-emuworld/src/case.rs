// SPDX-License-Identifier: MIT
//! The vector lines of a case on a scene's image (`re/tools/emu/sceneworld.py`).
//!
//! Nothing here reproduces a function of the original. A line is the harness's usual one
//! (`crates/si-x87/src/vectors.rs` has the format):
//!
//! ```text
//! NAME[#tag] p_ARG=WORD v_ARG=WORD … BUFFER=WORDS … => eax=WORD mXXXXXXXX=BYTES … @call(…)=… ! NOTES
//! ```
//!
//! `p_` and `v_` fields are the routine's arguments in C order (a pointer, a value); any
//! other input field is a buffer of the case, laid out in the arena in the order given
//! ([`arena_layout`]); `mXXXXXXXX` outputs are runs of bytes the routine changed in the
//! scene's image, at that address.

use si_mem::vecfmt::{parse_words, Word};

/// One case.
#[derive(Clone, Debug, Default)]
pub struct Case {
    pub name: String,
    pub inputs: Vec<(String, Option<Vec<Word>>)>,
    pub outputs: Vec<(String, Option<Vec<Word>>)>,
    pub notes: Vec<String>,
}

impl Case {
    pub fn routine(&self) -> &str {
        self.name.split('#').next().unwrap_or("")
    }

    /// The arguments in C order: `p_` and `v_` fields.
    pub fn args(&self) -> Vec<(&str, &[Word])> {
        self.inputs.iter().filter(|(n, _)| n.starts_with("p_") || n.starts_with("v_")).map(|(n, w)| (n.as_str(), w.as_deref().unwrap_or(&[]))).collect()
    }

    /// The buffers of the case, in the order given.
    pub fn buffers(&self) -> Vec<(&str, Option<&[Word]>)> {
        self.inputs.iter().filter(|(n, _)| !(n.starts_with("p_") || n.starts_with("v_") || n.starts_with("file:"))).map(|(n, w)| (n.as_str(), w.as_deref())).collect()
    }

    pub fn output(&self, name: &str) -> Option<&[Word]> {
        self.outputs.iter().find(|(n, _)| n == name).and_then(|(_, w)| w.as_deref())
    }

    pub fn has_note(&self, prefix: &str) -> bool {
        self.notes.iter().any(|n| n.starts_with(prefix))
    }

    pub fn stopped(&self) -> bool {
        self.has_note("STOP")
    }

    /// The runs of image bytes the routine changed: (address, words).
    pub fn changes(&self) -> Vec<(u32, &[Word])> {
        self.outputs
            .iter()
            .filter_map(|(n, w)| {
                let hex = n.strip_prefix('m')?;
                if hex.len() != 8 {
                    return None;
                }
                Some((u32::from_str_radix(hex, 16).ok()?, w.as_deref()?))
            })
            .collect()
    }
}

/// Splits a line at the blanks that are not inside a quoted text.
fn tokens(line: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), None::<usize>, false);
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'"' && (i == 0 || b[i - 1] != b'\\') {
            quoted = !quoted;
        }
        if c == b' ' && !quoted {
            if let Some(s) = start.take() {
                out.push(&line[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    out
}

/// One line of a vector file; `None` for a comment or an empty line.
pub fn parse(line: &str) -> Result<Option<Case>, String> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut c = Case::default();
    let mut part = 0; // 0 inputs, 1 outputs, 2 notes
    for (k, t) in tokens(line).into_iter().enumerate() {
        if k == 0 {
            c.name = t.to_string();
            continue;
        }
        match (part, t) {
            (_, "=>") => part = 1,
            (_, "!") => part = 2,
            (2, note) => c.notes.push(note.to_string()),
            (_, call) if call.starts_with('@') => {}
            (p, field) => {
                let (name, value) = field.split_once('=').ok_or_else(|| format!("no `=` in `{field}`"))?;
                let words = if value.starts_with('"') {
                    // text: as bytes
                    let text = &value[1..value.len().saturating_sub(1)];
                    Some(unescape(text).into_iter().map(|b| Word::value(1, b as u128)).collect())
                } else {
                    parse_words(value)?
                };
                if p == 0 {
                    c.inputs.push((name.to_string(), words));
                } else {
                    c.outputs.push((name.to_string(), words));
                }
            }
        }
    }
    Ok(Some(c))
}

fn unescape(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            if let Ok(v) = u8::from_str_radix(&text[i + 2..i + 4], 16) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Where the harness puts the buffers of a case: from the start of the arena, each at the
/// next multiple of 8, each followed by 1 KB (`Sandbox.alloc`, `Mem::place`).
pub fn arena_layout(sizes: &[u32]) -> Vec<u32> {
    let mut at = si_mem::ARENA.0;
    sizes
        .iter()
        .map(|&n| {
            let a = at.next_multiple_of(8);
            at = a + n + 0x400;
            a
        })
        .collect()
}

/// The first line of a scene's vector file: the scene's label, the index file of its
/// image, and the hash of each region: `# scene LABEL IDX name=HASH …`.
pub fn scene_header(line: &str) -> Option<(String, String, Vec<(String, u64)>)> {
    let rest = line.strip_prefix("# scene\t")?;
    let f: Vec<&str> = rest.split('\t').collect();
    if f.len() < 2 {
        return None;
    }
    let hashes = f[2..].iter().filter_map(|kv| kv.split_once('=')).filter_map(|(k, v)| Some((k.to_string(), u64::from_str_radix(v, 16).ok()?))).collect();
    Some((f[0].to_string(), f[1].to_string(), hashes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_with_buffers_changes_and_notes() {
        let line = "SI_x#t p_ed=10000008 v_chap=0003 name=4d,00 => eax=00000005 m10000010=01,02 heap0=-- @f(&name)=00000000 ! GW:x+0x0 STOP:fault";
        let c = parse(line).unwrap().unwrap();
        assert_eq!(c.routine(), "SI_x");
        assert_eq!(c.args().len(), 2);
        assert_eq!(c.buffers().len(), 1);
        assert_eq!(c.output("eax"), Some(&[Word::value(4, 5)][..]));
        assert_eq!(c.changes(), vec![(0x1000_0010, &[Word::value(1, 1), Word::value(1, 2)][..])]);
        assert!(c.stopped() && c.has_note("GW:"));
        assert!(parse("# scene\tx").unwrap().is_none());
    }

    #[test]
    fn buffers_lie_as_the_harness_lays_them() {
        assert_eq!(arena_layout(&[3, 16, 0]), vec![0x6100_0000, 0x6100_0408, 0x6100_0818]);
    }

    #[test]
    fn the_scene_line() {
        let (label, idx, h) = scene_header("# scene\ta/b.dsc\t0007.idx\tbss=00000000000000ff\theap=10").unwrap();
        assert_eq!((label.as_str(), idx.as_str()), ("a/b.dsc", "0007.idx"));
        assert_eq!(h, vec![("bss".to_string(), 0xff), ("heap".to_string(), 0x10)]);
    }
}
