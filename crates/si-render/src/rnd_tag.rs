// SPDX-License-Identifier: MIT
//! Tag files: the original's `rnd_tag.c` (finding 0012, "Tag files"). A tag image holds one
//! bit per pixel and channel: which tagged models cover the pixel. The renderer sets the bits
//! while it shades (`SI_sample`, `SI_supersample`) and writes one run-coded line per
//! channel after each line of the picture.

/// `SI_TagImage`: 0x14 bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagImage {
    /// +0x04: width in pixels.
    pub width: i16,
    /// +0x06: number of lines held in memory (the renderer keeps one).
    pub height: i16,
    /// +0x08: a float, 1.0 from `SI_tagAllocate` (the pixel ratio of the picture header it
    /// mirrors: named).
    pub ratio: f32,
    /// +0x0c: a short, 0 from `SI_tagAllocate`.
    pub w0c: i16,
    /// +0x0e: number of channels.
    pub channels: i16,
    /// +0x10: the bits, `[channel][line][byte]`, most significant bit first.
    pub data: Vec<Vec<Vec<u8>>>,
}

/// What `rnd_tag.c` allocates through: the C library's `calloc` and `malloc`, by address
/// (the layout of the blocks is this module's own; the calls are made in the original's
/// order and with its sizes so that an embedding that keeps the heap's books sees the same).
pub trait TagHeap {
    /// `calloc(1, size)`: 0 for failure.
    fn calloc(&mut self, size: u32) -> u32;
    /// `malloc(size)`.
    fn malloc(&mut self, size: u32) -> u32;
}

/// `versionName[version]`: the header's version word. The table at 0x0869c9c4 holds the
/// floats 2.0, 2.1, 2.5, 2.6 and `version` (0x0869c9c0) is 3 in the binary: 2.6. (The
/// picture writer of `rle.c` writes 4.0.)
pub const TAG_VERSION: f32 = 2.6;

/// A new, empty tag image [soft 0x0826d580 SI_tagAllocate]: `SI_tagAllocate()`: 0x14 zeroed
/// bytes with the float at +0x08 set to 1.0; `None` when `calloc` fails.
pub fn tag_allocate(heap: &mut dyn TagHeap) -> Option<TagImage> {
    if heap.calloc(0x14) == 0 {
        return None;
    }
    Some(TagImage { ratio: 1.0, ..TagImage::default() })
}

impl TagImage {
    /// Allocates the bit planes [soft 0x0826d5c8 SI_tagAllocateChannels]:
    /// `SI_tagAllocateChannels(tags)`. Returns 0 and does nothing when the channel count,
    /// the height or the width is 0. Otherwise: a table of `channels` pointers, per channel
    /// a table of `height` pointers, per line `width + 7 − ((width − 1) & 7)` bytes, zeroed:
    /// the width rounded up to a multiple of eight, **in bytes** although a line holds one
    /// bit per pixel. Returns 1. (No allocation is tested.)
    pub fn allocate_channels(&mut self, heap: &mut dyn TagHeap) -> i32 {
        if self.channels == 0 || self.height == 0 || self.width == 0 {
            return 0;
        }
        let w = self.width as i32;
        let size = w.wrapping_sub(((w.wrapping_sub(1) as u32 & 7) as i32).wrapping_sub(7));
        heap.malloc((self.channels as i32).wrapping_shl(2) as u32);
        self.data.clear();
        for _ in 0..self.channels.max(0) {
            heap.malloc((self.height as i32).wrapping_shl(2) as u32);
            let mut lines = Vec::new();
            for _ in 0..self.height.max(0) {
                heap.malloc(size as u32);
                lines.push(vec![0u8; size.max(0) as usize]);
            }
            self.data.push(lines);
        }
        1
    }

    /// Bit `p` of line `line` of channel `ch`, most significant bit of a byte first.
    fn bit(&self, ch: usize, line: usize, p: i32) -> u16 {
        ((self.data[ch][line][(p >> 3) as usize] >> (7 - (p & 7))) & 1) as u16
    }

    /// One line of every channel, run-coded [soft 0x0826d6ac si_tagWriteScanline]:
    /// `si_tagWriteScanline(file, tags, line)`. Per channel the line's `width` bits are
    /// written as 16-bit words, **in the machine's byte order** (low byte first in this
    /// build):
    ///
    /// - fewer than 15 bits left: one word holding them, the first in bit 13, downwards;
    /// - else, if the next 14 bits are all equal: a run. Its length counts on while the
    ///   bits stay equal, up to the end of the line; the word is 0x8000 (0xc000 for a run of
    ///   set bits) OR-ed with the length. (A run of 16,384 bits or more spills into the two
    ///   flag bits: the length is not held to 14 bits.)
    /// - else a word holding the next 14 bits.
    ///
    /// Returns the bytes for the file.
    pub fn write_scanline(&self, line: i32) -> Vec<u8> {
        let mut out = Vec::new();
        let line = line as usize;
        for ch in 0..self.channels.max(0) as usize {
            let mut pos = 0i32;
            let mut left = self.width as i32;
            while left > 0 {
                let literal = |n: i32| (0..n).fold(0u16, |w, i| if self.bit(ch, line, pos + i) != 0 { w | 1 << (13 - i) } else { w });
                let (word, n) = if left < 15 {
                    (literal(left), left)
                } else {
                    let first = self.bit(ch, line, pos);
                    if (1..14).all(|i| self.bit(ch, line, pos + i) == first) {
                        let mut n = 14;
                        while n < left && self.bit(ch, line, pos + n) == first {
                            n += 1;
                        }
                        ((if first == 0 { 0x8000 } else { 0xc000 }) | n as u16, n)
                    } else {
                        (literal(14), 14)
                    }
                };
                out.extend_from_slice(&word.to_le_bytes());
                pos += n;
                left -= n;
            }
        }
        out
    }
}

/// The 88-byte header of a database file as `rnd_tag.c` writes it
/// [soft 0x0826dac8 headerWriteBinary]: `headerWriteBinary(file, comment)`: the magic
/// 0x5380f634, the version word, and the comment (`No comment...` for a null pointer)
/// copied by `strncpy` into 80 bytes: zero filled, not terminated when it is 80 bytes or
/// longer. **In the machine's byte order**: low byte first in this build, where every
/// other file of the program is written high byte first. `version` is the float
/// `versionName[version]` ([`TAG_VERSION`] in the binary as shipped).
pub fn header_write_binary(comment: Option<&[u8]>, version: f32) -> [u8; 88] {
    let mut h = [0u8; 88];
    h[..4].copy_from_slice(&0x5380_f634u32.to_le_bytes());
    h[4..8].copy_from_slice(&version.to_le_bytes());
    let text = comment.unwrap_or(b"No comment...");
    let n = text.iter().position(|&b| b == 0).unwrap_or(text.len()).min(80);
    h[8..8 + n].copy_from_slice(&text[..n]);
    h
}

/// The tag file's name and header [soft 0x0826d950 si_tagWriteFileHeader]:
/// `si_tagWriteFileHeader(name, tags)`. The file is `name` when that is longer than four
/// characters and ends in `.tag`, else `name` + `.tag`; it is opened with mode `w` (the
/// caller here does that with the name returned; the original returns the file, null when
/// it could not be opened, and then writes nothing). The header: the 88 bytes of
/// `headerWriteBinary(file, null)`, `STAG`, then the width, the height, the float at +0x08,
/// the short at +0x0c and the channel count as they lie in memory (low byte first here).
pub fn tag_file_header(name: &[u8], tags: &TagImage, version: f32) -> (Vec<u8>, Vec<u8>) {
    let mut path = name.to_vec();
    if !(name.len() > 4 && name.ends_with(b".tag")) {
        path.extend_from_slice(b".tag");
    }
    let mut h = header_write_binary(None, version).to_vec();
    h.extend_from_slice(b"STAG");
    h.extend_from_slice(&tags.width.to_le_bytes());
    h.extend_from_slice(&tags.height.to_le_bytes());
    h.extend_from_slice(&tags.ratio.to_le_bytes());
    h.extend_from_slice(&tags.w0c.to_le_bytes());
    h.extend_from_slice(&tags.channels.to_le_bytes());
    (path, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Heap(Vec<u32>);
    impl TagHeap for Heap {
        fn calloc(&mut self, size: u32) -> u32 {
            self.0.push(size);
            8
        }
        fn malloc(&mut self, size: u32) -> u32 {
            self.0.push(size);
            8
        }
    }

    #[test]
    fn allocation_sizes() {
        let mut heap = Heap(Vec::new());
        let mut t = tag_allocate(&mut heap).unwrap();
        assert_eq!((t.ratio, t.channels, heap.0.clone()), (1.0, 0, vec![0x14]));
        assert_eq!(t.allocate_channels(&mut heap), 0);
        (t.width, t.height, t.channels) = (20, 1, 2);
        assert_eq!(t.allocate_channels(&mut heap), 1);
        // 20 pixels: 24 bytes a line (the width rounded up to eight, in bytes)
        assert_eq!(&heap.0[1..], &[8, 4, 24, 4, 24]);
        assert_eq!(t.data, vec![vec![vec![0u8; 24]]; 2]);
        t.width = 8;
        t.allocate_channels(&mut heap);
        assert_eq!(t.data[0][0].len(), 8);
    }

    #[test]
    fn lines_are_runs_and_literals() {
        let mut t = TagImage { width: 40, height: 1, channels: 1, data: vec![vec![vec![0u8; 40]]], ..TagImage::default() };
        // all clear: one run of 40
        assert_eq!(t.write_scanline(0), vec![40, 0x80]);
        // bits 0..15 set: a run of 16 set bits, then 24 clear ones
        t.data[0][0][0] = 0xff;
        t.data[0][0][1] = 0xff;
        assert_eq!(t.write_scanline(0), vec![16, 0xc0, 24, 0x80]);
        // a lone bit: 14 literal bits (bit 2 → 1 << 11), then a run of 26
        t.data[0][0][0] = 0x20;
        t.data[0][0][1] = 0;
        assert_eq!(t.write_scanline(0), vec![0x00, 0x08, 26, 0x80]);
        // fewer than 15 bits left are always a literal
        t.width = 20;
        t.data[0][0][0] = 0;
        t.data[0][0][2] = 0x10;
        assert_eq!(t.write_scanline(0), vec![19, 0x80, 0x00, 0x20]);
        t.width = 14;
        assert_eq!(t.write_scanline(0), vec![0, 0]);
    }

    #[test]
    fn header_bytes() {
        let h = header_write_binary(None, TAG_VERSION);
        assert_eq!(&h[..8], &[0x34, 0xf6, 0x80, 0x53, 0x66, 0x66, 0x26, 0x40]);
        assert_eq!(&h[8..22], b"No comment...\0");
        let t = TagImage { width: 640, height: 1, ratio: 1.0, channels: 3, ..TagImage::default() };
        let (path, h) = tag_file_header(b"shot.3", &t, TAG_VERSION);
        assert_eq!(path, b"shot.3.tag");
        assert_eq!(&h[88..], &[b'S', b'T', b'A', b'G', 0x80, 2, 1, 0, 0, 0, 0x80, 0x3f, 0, 0, 3, 0]);
        assert_eq!(tag_file_header(b"x.tag", &t, 1.0).0, b"x.tag");
        assert_eq!(tag_file_header(b".tag", &t, 1.0).0, b".tag.tag");
    }
}
