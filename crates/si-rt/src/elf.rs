// SPDX-License-Identifier: MIT
//! The loadable segments of a 32-bit little-endian ELF program file: from a virtual address of
//! the running program to the bytes of the file that the loader maps there.
//!
//! Only the program headers are read (`PT_LOAD`: file offset, address, size in the file, size in
//! memory), as the system's loader and the emulation harness (`re/tools/emu/emu.py`) do; the
//! section table is not needed. Nothing here reproduces a function of the original.

/// One `PT_LOAD` segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Virtual address of its first byte.
    pub vaddr: u32,
    /// Offset of that byte in the file.
    pub offset: u32,
    /// Bytes the file holds for it; the rest up to `memsz` is zero in memory (`.bss`).
    pub filesz: u32,
    pub memsz: u32,
    /// `PF_X` 1, `PF_W` 2, `PF_R` 4.
    pub flags: u32,
}

impl Segment {
    /// Whether `addr .. addr + len` lies in the bytes the file holds for this segment.
    pub fn holds(&self, addr: u32, len: u32) -> bool {
        let (a, l) = (addr as u64, len as u64);
        a >= self.vaddr as u64 && a + l <= self.vaddr as u64 + self.filesz as u64
    }
}

/// Why a file is not a program image this module can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    NotElf,
    /// Not 32-bit little-endian (`EI_CLASS`, `EI_DATA`).
    NotElf32Le,
    /// The program-header table or a segment lies outside the file.
    Truncated,
    NoLoadSegment,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ParseError::NotElf => "not an ELF program file",
            ParseError::NotElf32Le => "not a 32-bit little-endian ELF program file",
            ParseError::Truncated => "the program file is cut short (its program headers or a segment lie outside it)",
            ParseError::NoLoadSegment => "the program file has no loadable segment",
        })
    }
}

/// A program file and its loadable segments, borrowed.
#[derive(Clone, Debug)]
pub struct Image<'a> {
    pub file: &'a [u8],
    pub segments: Vec<Segment>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl<'a> Image<'a> {
    /// Reads the program headers. Every count and offset read from the file is checked against
    /// the bytes there before it is used.
    pub fn parse(file: &'a [u8]) -> Result<Self, ParseError> {
        if file.len() < 0x34 || &file[..4] != b"\x7fELF" {
            return Err(ParseError::NotElf);
        }
        if file[4] != 1 || file[5] != 1 {
            return Err(ParseError::NotElf32Le);
        }
        let t = ParseError::Truncated;
        let phoff = u32_at(file, 0x1c).ok_or(t.clone())? as usize;
        let phentsize = u16_at(file, 0x2a).ok_or(t.clone())? as usize;
        let phnum = u16_at(file, 0x2c).ok_or(t.clone())? as usize;
        if phentsize < 0x20 || phoff.checked_add(phentsize * phnum).is_none_or(|e| e > file.len()) {
            return Err(t);
        }
        let mut segments = Vec::new();
        for i in 0..phnum {
            let o = phoff + i * phentsize;
            let w = |k: usize| u32_at(file, o + 4 * k).unwrap();
            if w(0) != 1 {
                continue; // PT_LOAD only
            }
            let s = Segment { offset: w(1), vaddr: w(2), filesz: w(4), memsz: w(5), flags: w(6) };
            if s.offset as u64 + s.filesz as u64 > file.len() as u64 || s.filesz > s.memsz {
                return Err(ParseError::Truncated);
            }
            segments.push(s);
        }
        if segments.is_empty() {
            return Err(ParseError::NoLoadSegment);
        }
        Ok(Image { file, segments })
    }

    /// The segment whose file bytes hold `addr .. addr + len`.
    pub fn segment_of(&self, addr: u32, len: u32) -> Option<&Segment> {
        self.segments.iter().find(|s| s.holds(addr, len))
    }

    /// The file bytes the program has at `addr .. addr + len`, if the file holds all of them
    /// (a range reaching into `.bss` is not file bytes: see [`Image::read_into`]).
    pub fn bytes(&self, addr: u32, len: u32) -> Option<&'a [u8]> {
        let s = self.segment_of(addr, len)?;
        let o = (s.offset + (addr - s.vaddr)) as usize;
        Some(&self.file[o..o + len as usize])
    }

    /// The memory the program starts with at `addr ..`, `.bss` included (zeros): false when a
    /// byte is in no segment.
    pub fn read_into(&self, addr: u32, out: &mut [u8]) -> bool {
        for (i, b) in out.iter_mut().enumerate() {
            let a = match addr.checked_add(i as u32) {
                Some(a) => a,
                None => return false,
            };
            let Some(s) = self.segments.iter().find(|s| a >= s.vaddr && (a as u64) < s.vaddr as u64 + s.memsz as u64) else {
                return false;
            };
            *b = if a - s.vaddr < s.filesz { self.file[(s.offset + (a - s.vaddr)) as usize] } else { 0 };
        }
        true
    }

    /// The C string at `addr`, without its terminating zero; `None` when `addr` is not in the
    /// file bytes of a segment or no zero follows before the segment's end.
    pub fn cstr(&self, addr: u32) -> Option<&'a [u8]> {
        let s = self.segment_of(addr, 1)?;
        let o = (s.offset + (addr - s.vaddr)) as usize;
        let end = (s.offset + s.filesz) as usize;
        let n = self.file[o..end].iter().position(|&c| c == 0)?;
        Some(&self.file[o..o + n])
    }

    /// The little-endian word at `addr` (file bytes only).
    pub fn word(&self, addr: u32) -> Option<u32> {
        self.bytes(addr, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A made-up program file: two segments, the second with a `.bss` tail.
    pub(crate) fn tiny() -> Vec<u8> {
        let mut f = vec![0u8; 0x180];
        f[..4].copy_from_slice(b"\x7fELF");
        f[4] = 1;
        f[5] = 1;
        f[0x1c..0x20].copy_from_slice(&0x34u32.to_le_bytes());
        f[0x2a..0x2c].copy_from_slice(&0x20u16.to_le_bytes());
        f[0x2c..0x2e].copy_from_slice(&3u16.to_le_bytes());
        let ph = |f: &mut Vec<u8>, i: usize, w: [u32; 8]| {
            for (k, v) in w.iter().enumerate() {
                let o = 0x34 + 0x20 * i + 4 * k;
                f[o..o + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        ph(&mut f, 0, [1, 0x100, 0x1000_0100, 0, 0x40, 0x40, 5, 0x1000]);
        ph(&mut f, 1, [4, 0, 0, 0, 0, 0, 4, 4]); // a note: skipped
        ph(&mut f, 2, [1, 0x140, 0x2000_0140, 0, 0x40, 0x80, 6, 0x1000]);
        for i in 0x100..0x180 {
            f[i] = (i as u8).wrapping_mul(7) | 1;
        }
        f[0x110..0x116].copy_from_slice(b"hello\0");
        f
    }

    #[test]
    fn segments_and_bytes() {
        let f = tiny();
        let im = Image::parse(&f).unwrap();
        assert_eq!(im.segments.len(), 2);
        assert_eq!(im.cstr(0x1000_0110), Some(&b"hello"[..]));
        assert_eq!(im.bytes(0x1000_0100, 2), Some(&f[0x100..0x102]));
        assert_eq!(im.bytes(0x1000_013f, 2), None, "past the segment's file bytes");
        assert_eq!(im.bytes(0x2000_0140, 0x40), Some(&f[0x140..0x180]));
        assert_eq!(im.bytes(0x2000_0140, 0x41), None, "into .bss");
        let mut b = [0xffu8; 4];
        assert!(im.read_into(0x2000_017e, &mut b));
        assert_eq!(b, [f[0x17e], f[0x17f], 0, 0]);
        assert!(!im.read_into(0x2000_01be, &mut b), "past memsz");
        assert_eq!(im.word(0x1000_0110), Some(u32::from_le_bytes(*b"hell")));
    }

    #[test]
    fn refuses_bad_files() {
        assert_eq!(Image::parse(b"not an elf").unwrap_err(), ParseError::NotElf);
        let mut f = tiny();
        f[4] = 2;
        assert_eq!(Image::parse(&f).unwrap_err(), ParseError::NotElf32Le);
        let mut f = tiny();
        f[0x2c..0x2e].copy_from_slice(&200u16.to_le_bytes());
        assert_eq!(Image::parse(&f).unwrap_err(), ParseError::Truncated);
        let f = tiny();
        assert_eq!(Image::parse(&f[..0x170]).unwrap_err(), ParseError::Truncated);
    }
}
