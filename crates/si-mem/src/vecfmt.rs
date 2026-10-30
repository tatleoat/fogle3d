// SPDX-License-Identifier: MIT
//! Loading and dumping memory in the form the emulation harness's vector files use.
//!
//! Nothing here reproduces a function of the original, and nothing here knows it: this is
//! the text form of `re/tools/emu` (finding 0011; the reader for whole vector lines is
//! `si_x87::vectors`, which this crate cannot depend on and does not need).
//!
//! A field of a vector line is `name=word,word,…`. A word is hexadecimal and as wide as it
//! is long (two digits a byte; twenty for an 80-bit register). Words are numbers, not
//! memory order: the float 1.0 is `3f800000` and lies in memory as `00 00 80 3f`. Dashes
//! are a word the routine did not write, `&name+0x10` is a pointer into the buffer of the
//! field `name`, `&heap2` into the third block the routine got from `malloc`, `"text"` is
//! bytes, `null` a null pointer.
//!
//! With the image this is all a check needs to hand a case to a ported routine and to
//! compare what the routine left: [`load_case`] places every input buffer in the arena and
//! stores its words with the pointers between buffers resolved, the routine runs on the
//! image, and [`Mem::compare_words`] holds each output field against the memory at its
//! buffer. No per-crate code turns words into the crate's own structures and back.

use crate::{Mem, Region};

/// One word of a field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Word {
    /// A value of `bytes` bytes.
    Value { bytes: u8, value: u128 },
    /// A word of `bytes` bytes nobody wrote.
    Unwritten { bytes: u8 },
    /// A pointer `offset` bytes into the buffer (or heap block) `name`.
    Ref { name: String, offset: u32 },
}

impl Word {
    /// The width in bytes; a pointer is four.
    pub fn bytes(&self) -> u32 {
        match self {
            Word::Value { bytes, .. } | Word::Unwritten { bytes } => *bytes as u32,
            Word::Ref { .. } => 4,
        }
    }

    pub fn value(bytes: u8, value: u128) -> Word {
        Word::Value { bytes, value }
    }

    fn parse(text: &str) -> Result<Word, String> {
        if let Some(r) = text.strip_prefix('&') {
            let (name, offset) = match r.split_once('+') {
                Some((n, o)) => (n, parse_int(o)?),
                None => (r, 0),
            };
            return Ok(Word::Ref { name: name.to_string(), offset });
        }
        if text.is_empty() || text.len() % 2 != 0 || text.len() > 32 {
            return Err(format!("bad word `{text}`"));
        }
        let bytes = (text.len() / 2) as u8;
        if text.bytes().all(|c| c == b'-') {
            return Ok(Word::Unwritten { bytes });
        }
        let value = u128::from_str_radix(text, 16).map_err(|_| format!("bad word `{text}`"))?;
        Ok(Word::Value { bytes, value })
    }
}

impl core::fmt::Display for Word {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Word::Value { bytes, value } => write!(f, "{:0width$x}", value, width = 2 * *bytes as usize),
            Word::Unwritten { bytes } => write!(f, "{}", "-".repeat(2 * *bytes as usize)),
            Word::Ref { name, offset: 0 } => write!(f, "&{name}"),
            Word::Ref { name, offset } => write!(f, "&{name}+0x{offset:x}"),
        }
    }
}

fn parse_int(text: &str) -> Result<u32, String> {
    let r = match text.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => text.parse(),
    };
    r.map_err(|_| format!("bad number `{text}`"))
}

/// The words of a field's value (what follows `name=`): `None` for `null`.
pub fn parse_words(text: &str) -> Result<Option<Vec<Word>>, String> {
    if text == "null" {
        return Ok(None);
    }
    if let Some(q) = text.strip_prefix('"') {
        let q = q.strip_suffix('"').ok_or_else(|| format!("unterminated text `{text}`"))?;
        let b = q.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
                let v = u8::from_str_radix(&q[i + 2..i + 4], 16).map_err(|_| format!("bad escape in `{text}`"))?;
                out.push(Word::Value { bytes: 1, value: v as u128 });
                i += 4;
            } else {
                out.push(Word::Value { bytes: 1, value: b[i] as u128 });
                i += 1;
            }
        }
        return Ok(Some(out));
    }
    if text.is_empty() {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(text.split(',').map(Word::parse).collect::<Result<Vec<_>, _>>()?))
}

/// The text of a field's value.
pub fn format_words(words: Option<&[Word]>) -> String {
    match words {
        None => "null".to_string(),
        Some(w) => w.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(","),
    }
}

/// Bytes as the quoted text of a field: blanks, quotes, backslashes and everything outside
/// the printable set as `\xNN`.
pub fn format_text(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in bytes {
        if (0x21..0x7f).contains(&b) && b != b'"' && b != b'\\' {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out.push('"');
    out
}

/// How many bytes the words take in memory.
pub fn words_len(words: &[Word]) -> u32 {
    words.iter().map(Word::bytes).sum()
}

/// One element of a buffer's shape: how [`Mem::dump_words`] cuts memory into words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elem {
    /// A number of that many bytes (1, 2, 4, 8, 10).
    Int(u8),
    /// A pointer: printed as `&name+offset` when it points into a named buffer or a heap
    /// block.
    Ptr,
}

/// The shape a spec line of the harness gives a buffer: `f32[16]`, `u8[72]`,
/// `(i32,ptr,f32*2,u16*4)`, or one element type.
pub fn parse_shape(text: &str) -> Result<Vec<Elem>, String> {
    fn elem(t: &str) -> Result<Elem, String> {
        Ok(match t {
            "u8" | "i8" | "char" => Elem::Int(1),
            "u16" | "i16" => Elem::Int(2),
            "u32" | "i32" | "f32" => Elem::Int(4),
            "u64" | "i64" | "f64" => Elem::Int(8),
            "f80" => Elem::Int(10),
            "ptr" => Elem::Ptr,
            _ => return Err(format!("unknown element type `{t}`")),
        })
    }
    let text = text.trim();
    if let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        let mut out = Vec::new();
        for part in inner.split(',') {
            let (t, n) = match part.split_once('*') {
                Some((t, n)) => (t, parse_int(n)?),
                None => (part, 1),
            };
            out.extend(std::iter::repeat_n(elem(t.trim())?, n as usize));
        }
        return Ok(out);
    }
    if let Some((t, n)) = text.strip_suffix(']').and_then(|t| t.split_once('[')) {
        return Ok(vec![elem(t)?; parse_int(n)? as usize]);
    }
    Ok(vec![elem(text)?])
}

/// The named buffers of a case: where each lies in the image.
#[derive(Clone, Debug, Default)]
pub struct Names {
    list: Vec<(String, u32, u32)>,
}

impl Names {
    pub fn new() -> Names {
        Names::default()
    }

    /// The buffer `name` is the `len` bytes at `addr`.
    pub fn add(&mut self, name: &str, addr: u32, len: u32) {
        self.list.push((name.to_string(), addr, len));
    }

    /// Where a buffer lies; `heapN` is the N-th block `malloc` handed out. A null buffer
    /// (`name=null`) is at 0.
    pub fn addr(&self, m: &Mem, name: &str) -> Option<u32> {
        if let Some(b) = self.list.iter().find(|b| b.0 == name) {
            return Some(b.1);
        }
        let n: usize = name.strip_prefix("heap")?.parse().ok()?;
        m.heap_blocks().get(n).map(|b| b.0)
    }

    /// The buffer or heap block an address points into, and the offset in it. One past the
    /// end still names the buffer.
    pub fn name_of(&self, m: &Mem, addr: u32) -> Option<(String, u32)> {
        if addr == 0 {
            return None;
        }
        // the innermost: a buffer inside which the address is, before one it is just past
        let hit = self.list.iter().filter(|b| b.1 != 0).find(|b| addr.wrapping_sub(b.1) < b.2.max(1));
        if let Some(b) = hit {
            return Some((b.0.clone(), addr - b.1));
        }
        if let Some((i, off)) = m.heap_block_of(addr) {
            return Some((format!("heap{i}"), off));
        }
        self.list.iter().filter(|b| b.1 != 0).find(|b| addr.wrapping_sub(b.1) == b.2).map(|b| (b.0.clone(), b.2))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, u32, u32)> {
        self.list.iter().map(|b| (b.0.as_str(), b.1, b.2))
    }
}

impl Mem {
    /// Stores words at `addr` in memory order, pointers resolved through `names`; a word
    /// nobody wrote leaves the bytes as they are (the fill). Answers the bytes taken.
    pub fn load_words(&mut self, addr: u32, words: &[Word], names: &Names) -> Result<u32, String> {
        self.load_words_as(addr, words, names, false)
    }

    /// The same for constants of the original: `.rodata` takes them ([`Mem::set_rom`]).
    pub fn load_rom_words(&mut self, addr: u32, words: &[Word], names: &Names) -> Result<u32, String> {
        self.load_words_as(addr, words, names, true)
    }

    fn load_words_as(&mut self, addr: u32, words: &[Word], names: &Names, rom: bool) -> Result<u32, String> {
        let mut at = addr;
        for w in words {
            let value = match w {
                Word::Value { value, .. } => Some(*value),
                Word::Unwritten { .. } => None,
                Word::Ref { name, offset } => Some(names.addr(self, name).ok_or_else(|| format!("no buffer `{name}`"))?.wrapping_add(*offset) as u128),
            };
            if let Some(value) = value {
                let bytes = &value.to_le_bytes()[..w.bytes() as usize];
                if rom {
                    self.set_rom(at, bytes);
                } else {
                    self.set_bytes(at, bytes);
                }
            }
            at = at.wrapping_add(w.bytes());
        }
        Ok(at.wrapping_sub(addr))
    }

    /// The memory at `addr` cut into words of the given shape. Unmapped bytes come out as
    /// unwritten words (no fault is recorded).
    pub fn dump_words(&self, addr: u32, shape: &[Elem], names: &Names) -> Vec<Word> {
        let mut out = Vec::with_capacity(shape.len());
        let mut at = addr;
        for e in shape {
            let n = match e {
                Elem::Int(n) => *n,
                Elem::Ptr => 4,
            };
            let word = match self.try_bytes(at, n as u32) {
                None => Word::Unwritten { bytes: n },
                Some(b) => {
                    let mut v = [0u8; 16];
                    v[..n as usize].copy_from_slice(b);
                    let value = u128::from_le_bytes(v);
                    match (e, names.name_of(self, value as u32)) {
                        (Elem::Ptr, Some((name, offset))) => Word::Ref { name, offset },
                        _ => Word::Value { bytes: n, value },
                    }
                }
            };
            out.push(word);
            at = at.wrapping_add(n as u32);
        }
        out
    }

    /// Holds the memory at `addr` against the words a vector file gives for it. A word the
    /// original did not write is not compared; a pointer is compared as the address its
    /// buffer has here. `Err` describes the first word that differs.
    pub fn compare_words(&self, addr: u32, expect: &[Word], names: &Names) -> Result<(), String> {
        let mut at = addr;
        for (i, w) in expect.iter().enumerate() {
            let n = w.bytes();
            let want: Option<u128> = match w {
                Word::Value { value, .. } => Some(*value),
                Word::Unwritten { .. } => None,
                Word::Ref { name, offset } => Some(names.addr(self, name).ok_or_else(|| format!("no buffer `{name}`"))?.wrapping_add(*offset) as u128),
            };
            if let Some(want) = want {
                let Some(b) = self.try_bytes(at, n) else {
                    return Err(format!("word {i} at {at:#010x}: not mapped"));
                };
                let mut v = [0u8; 16];
                v[..n as usize].copy_from_slice(b);
                let got = u128::from_le_bytes(v);
                if got != want {
                    return Err(format!("word {i} (+{:#x}): original {w}, port {:0width$x}", at.wrapping_sub(addr), got, width = 2 * n as usize));
                }
            }
            at = at.wrapping_add(n);
        }
        Ok(())
    }
}

/// Places the buffers of a case in the arena and stores their words: each `(name, words)`
/// becomes a buffer of the words' size ([`Mem::place`], 8-aligned, in the order given,
/// which is how the harness lays out a case), then every buffer is filled with the
/// pointers between them resolved. A null field (`None`) is the null pointer.
pub fn load_case(m: &mut Mem, fields: &[(&str, Option<&[Word]>)]) -> Result<Names, String> {
    let mut names = Names::new();
    for (name, words) in fields {
        match words {
            None => names.add(name, 0, 0),
            Some(w) => {
                let len = words_len(w);
                let a = m.place(len, 8);
                if a == 0 {
                    return Err(format!("no room in the arena for `{name}` ({len} bytes)"));
                }
                names.add(name, a, len);
            }
        }
    }
    for (name, words) in fields {
        if let Some(w) = words {
            let a = names.addr(m, name).unwrap();
            m.load_words(a, w, &names)?;
        }
    }
    Ok(names)
}

/// One buffer of a case for [`load_case_at`]: its name, the address the vector gives it
/// (`None`: wherever the loader places it) and its words (`None`: a null pointer).
pub type FieldAt<'a> = (&'a str, Option<u32>, Option<&'a [Word]>);

/// [`load_case`] for a case whose vector says where some of its buffers lie: each field is
/// `(name, address, words)`.
///
/// A field without an address is placed as [`load_case`] places it ([`Mem::place`],
/// 8-aligned, in the order given). A field with one lies there:
///
/// - in the arena it becomes a placed buffer at that address ([`Mem::place_at`]), which a
///   routine may `free` and `realloc` like any other; fields placed after it go behind it;
/// - in `.data`, `.bss` or on the stack it is a global or a local of the case: the words
///   are stored there, and nothing is placed (those regions are memory as a whole);
/// - in `.rodata` the words are stored as constants ([`Mem::set_rom`]);
/// - in the heap, or where there is no memory, it is an error: a block of the heap comes
///   from `malloc`, in the routine's own order.
///
/// Then every buffer is filled, with the pointers between all of them resolved, whichever
/// way they point: a global at an arena buffer, an arena buffer at a global. A null field
/// (`None` for the words) is the null pointer, whatever address it carries.
pub fn load_case_at(m: &mut Mem, fields: &[FieldAt<'_>]) -> Result<Names, String> {
    let mut names = Names::new();
    let mut rom = vec![false; fields.len()];
    for (i, (name, at, words)) in fields.iter().enumerate() {
        let Some(w) = words else {
            names.add(name, 0, 0);
            continue;
        };
        let len = words_len(w);
        let a = match *at {
            None => m.place(len, 8),
            Some(a) => {
                match m.region_of(a) {
                    Some(Region::Arena) => {
                        if !m.place_at(a, len) {
                            return Err(format!("`{name}` ({len} bytes) at {a:#010x} does not fit the arena"));
                        }
                    }
                    Some(Region::Heap) => return Err(format!("`{name}` at {a:#010x} is in the heap: a heap block comes from malloc")),
                    Some(region) => {
                        if !m.is_mapped(a, len) {
                            return Err(format!("`{name}` ({len} bytes) at {a:#010x} does not fit its region"));
                        }
                        rom[i] = region == Region::Rodata;
                    }
                    None => return Err(format!("`{name}` at {a:#010x}: no memory there")),
                }
                a
            }
        };
        if a == 0 {
            return Err(format!("no room in the arena for `{name}` ({len} bytes)"));
        }
        names.add(name, a, len);
    }
    for (i, (name, _, words)) in fields.iter().enumerate() {
        if let Some(w) = words {
            let a = names.addr(m, name).unwrap();
            m.load_words_as(a, w, &names, rom[i])?;
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fill, ARENA, BSS, DATA, HEAP, RODATA, STACK_START};

    #[test]
    fn a_case_with_buffers_at_addresses() {
        let mut m = Mem::new();
        m.set_fill(Fill::Pattern { second: false, salt: 2 });
        let w = |t: &str| parse_words(t).unwrap().unwrap();
        // a global that points at an arena buffer, an arena buffer that points at the
        // global and at a literal, a buffer the vector puts at an address of the arena, one
        // it leaves to the loader, a null one
        let (dict, elems, lit, world, out) = (w("&elems,00000002"), w("&dict+0x4,&lit,----,0007"), w("\"MDL\""), w("&out,&elems+0x8"), w("--------"));
        let fields: Vec<FieldAt<'_>> = vec![
            ("dict", Some(BSS.0 + 0x100), Some(&dict)),
            ("elems", None, Some(&elems)),
            ("lit", Some(RODATA.0 + 0x20), Some(&lit)),
            ("world", Some(ARENA.0 + 0x4000), Some(&world)),
            ("out", None, Some(&out)),
            ("local", Some(STACK_START - 0x10), Some(&dict)),
            ("none", Some(DATA.0), None),
        ];
        let names = load_case_at(&mut m, &fields).unwrap();
        assert!(m.fault().is_none());
        // (no heap block is named here, so the image is not needed to find a buffer)
        let none = Mem::new();
        let at = |n: &str| names.addr(&none, n).unwrap();
        // placed in the order given; what follows a fixed buffer goes behind it
        assert_eq!((at("dict"), at("elems"), at("lit"), at("world"), at("out"), at("none")), (BSS.0 + 0x100, ARENA.0, RODATA.0 + 0x20, ARENA.0 + 0x4000, ARENA.0 + 0x4008, 0));
        // pointers resolved both ways
        assert_eq!((m.u32(BSS.0 + 0x100), m.u32(BSS.0 + 0x104)), (ARENA.0, 2));
        assert_eq!((m.u32(ARENA.0), m.u32(ARENA.0 + 4), m.u16(ARENA.0 + 10)), (BSS.0 + 0x104, RODATA.0 + 0x20, 7));
        assert_eq!(m.u8(ARENA.0 + 9), m.fill().byte(ARENA.0 + 9)); // the word nobody wrote
        assert_eq!(m.bytes(RODATA.0 + 0x20, 3), b"MDL");
        assert_eq!((m.u32(ARENA.0 + 0x4000), m.u32(ARENA.0 + 0x4004)), (ARENA.0 + 0x4008, ARENA.0 + 8));
        assert_eq!(m.u32(STACK_START - 0x10), ARENA.0);
        // the arena's buffers are blocks a routine may free; a global is not one
        assert_eq!((m.block_size(at("world")), m.block_size(at("elems")), m.block_size(at("dict"))), (Some(8), Some(12), None));
        m.free(at("world"));
        assert!(m.fault().is_none());
        // what the routine leaves is held against the vector by the same names
        m.set_u32(at("out"), at("dict") + 4);
        assert_eq!(m.compare_words(at("out"), &w("&dict+0x4"), &names), Ok(()));
        assert_eq!(format_words(Some(&m.dump_words(at("dict"), &parse_shape("(ptr,i32)").unwrap(), &names))), "&elems,00000002");

        // where a buffer cannot lie
        let mut m = Mem::new();
        let e = |m: &mut Mem, a: u32| load_case_at(m, &[("x", Some(a), Some(&out))]).unwrap_err();
        assert!(e(&mut m, HEAP.0 + 8).contains("in the heap"));
        assert!(e(&mut m, 0x1000).contains("no memory there"));
        assert!(e(&mut m, BSS.0 + BSS.1 - 2).contains("does not fit its region"));
        assert!(e(&mut m, ARENA.0 + ARENA.1 - 2).contains("does not fit the arena"));
        assert!(m.fault().is_none());
        // without addresses it is load_case
        let plain: Vec<(&str, Option<&[Word]>)> = vec![("a", Some(&elems[2..])), ("b", Some(&out))];
        let fixed: Vec<FieldAt<'_>> = plain.iter().map(|&(n, w)| (n, None, w)).collect();
        let (mut m1, mut m2) = (Mem::new(), Mem::new());
        let (n1, n2) = (load_case(&mut m1, &plain).unwrap(), load_case_at(&mut m2, &fixed).unwrap());
        assert_eq!(n1.iter().collect::<Vec<_>>(), n2.iter().collect::<Vec<_>>());
        assert_eq!(m1.arena_blocks(), m2.arena_blocks());
    }

    #[test]
    fn words_parse_and_print() {
        let text = "3f800000,--------,&keys+0x10,&heap2,00ff,4000ef77500000000000";
        let w = parse_words(text).unwrap().unwrap();
        assert_eq!(w[0], Word::Value { bytes: 4, value: 0x3f80_0000 });
        assert_eq!(w[1], Word::Unwritten { bytes: 4 });
        assert_eq!(w[2], Word::Ref { name: "keys".into(), offset: 0x10 });
        assert_eq!(w[3], Word::Ref { name: "heap2".into(), offset: 0 });
        assert_eq!((w[4].bytes(), w[5].bytes()), (2, 10));
        assert_eq!(format_words(Some(&w)), text);
        assert_eq!(words_len(&w), 4 + 4 + 4 + 4 + 2 + 10);
        assert_eq!(parse_words("null").unwrap(), None);
        assert_eq!(format_words(None), "null");
        assert_eq!(parse_words("").unwrap().unwrap().len(), 0);
        let t = parse_words("\"a\\x20b\"").unwrap().unwrap();
        assert_eq!(t.iter().map(|w| w.to_string()).collect::<String>(), "612062");
        assert_eq!(format_text(b"a b\"\n"), "\"a\\x20b\\x22\\x0a\"");
        assert!(parse_words("3f8").is_err());
    }

    #[test]
    fn shapes() {
        assert_eq!(parse_shape("f32[3]").unwrap(), vec![Elem::Int(4); 3]);
        assert_eq!(
            parse_shape("(i32,ptr,f32*2,u16*2,f80)").unwrap(),
            vec![Elem::Int(4), Elem::Ptr, Elem::Int(4), Elem::Int(4), Elem::Int(2), Elem::Int(2), Elem::Int(10)]
        );
        assert_eq!(parse_shape("ptr").unwrap(), vec![Elem::Ptr]);
        assert!(parse_shape("(i32,bool)").is_err());
    }

    #[test]
    fn a_case_goes_in_and_comes_out() {
        let mut m = Mem::new();
        m.set_fill(Fill::Pattern { second: false, salt: 1 });
        // a record that points to its key array and to itself, the keys, an output buffer
        let curve = parse_words("00000002,&keys,&curve+0x8,3f800000").unwrap().unwrap();
        let keys = parse_words("0001,0002,0003,----").unwrap().unwrap();
        let out = parse_words("--------,--------").unwrap().unwrap();
        let names = load_case(&mut m, &[("curve", Some(&curve)), ("keys", Some(&keys)), ("out", Some(&out)), ("none", None)]).unwrap();
        let (c, k, o) = (names.addr(&m, "curve").unwrap(), names.addr(&m, "keys").unwrap(), names.addr(&m, "out").unwrap());
        assert_eq!((c, k), (ARENA.0, ARENA.0 + 0x410));
        assert_eq!(names.addr(&m, "none"), Some(0));
        assert_eq!((m.u32(c), m.u32(c + 4), m.u32(c + 8), m.f32(c + 12)), (2, k, c + 8, 1.0));
        assert_eq!((m.u16(k), m.u16(k + 4)), (1, 3));
        // the word nobody wrote holds the fill
        assert_eq!(m.u8(k + 6), m.fill().byte(k + 6));
        // the "routine": stores a pointer to the second key and to a new block
        let block = m.malloc(8);
        m.set_u32(o, k + 2);
        m.set_u32(o + 4, block + 4);
        assert_eq!(block, HEAP.0 + 8);
        let shape = parse_shape("ptr[2]").unwrap();
        assert_eq!(format_words(Some(&m.dump_words(o, &shape, &names))), "&keys+0x2,&heap0+0x4");
        // compared with what the original left: unwritten words are skipped
        let want = parse_words("&keys+0x2,--------").unwrap().unwrap();
        assert_eq!(m.compare_words(o, &want, &names), Ok(()));
        let wrong = parse_words("&keys+0x4,&heap0+0x4").unwrap().unwrap();
        assert!(m.compare_words(o, &wrong, &names).unwrap_err().starts_with("word 0 (+0x0): original &keys+0x4, port "));
        // a pointer that points nowhere known is a number; a dump of unmapped memory is dashes
        m.set_u32(o, 0x1234);
        assert_eq!(format_words(Some(&m.dump_words(o, &shape[..1], &names))), "00001234");
        assert_eq!(format_words(Some(&m.dump_words(0, &shape[..1], &names))), "--------");
        assert!(m.fault().is_none());
        // the record dumped whole
        let shape = parse_shape("(i32,ptr,ptr,f32)").unwrap();
        assert_eq!(format_words(Some(&m.dump_words(c, &shape, &names))), "00000002,&keys,&curve+0x8,3f800000");
    }
}
