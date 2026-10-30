// SPDX-License-Identifier: MIT
//! The picture file the renderer writes: the original's `rle.c` (finding 0012, "The picture
//! file"). `si_rleInit` opens `<name>.<frame>.pic` and writes its header, `SI_rleWrite` adds
//! one line (run-coded by `writeChanalMixed`), `SI_rleClose` closes the file and marks it
//! complete.
//!
//! This is the port on values (the globals as a struct, the file as a host's methods).
//! [`crate::rle_image`] has the same routines on the memory image, which is what
//! `SI_image` is run with; both forms are held against the same emulated cases.
//!
//! The file layout is that of finding 0003. The run coding makes the same choices as the
//! program's interactive picture writer (`si_picture::write_mixed`): a test in this file
//! compares the two. What the renderer's writer does differently is in the header: its own
//! comment, always an alpha packet, and the word finding 0003 calls padding, which it uses
//! as a state mark: `N` while the picture is being written, `C` once it is complete.

/// 0x5380f634: the magic number of a database file's header.
const MAGIC: u32 = 0x5380_f634;
/// 0x08654308: the header's version word, the float 4.0.
const VERSION: f32 = 4.0;
/// 0x0869c5c0: the comment of a rendered picture.
fn comment() -> &'static [u8] {
    si_rt::message(0x0869_c5c0)
}

/// The state `writeChanalMixed` keeps along a line: up to 128 pixels of four bytes each, how
/// many it holds, and whether they are a run of equal pixels.
struct Coder<'a> {
    out: &'a mut Vec<u8>,
    channels: i32,
    buf: [[u8; 4]; 128],
    /// Number of pixels held (for a run: its length; only pixel 0 is stored).
    n: u16,
    /// Non-zero: the pixels held are equal. The original does not initialise it; it is set
    /// before it is first read.
    same: u16,
}

impl Coder<'_> {
    /// `channels` bytes of held pixel `i`.
    fn put(&mut self, i: usize) {
        for k in 0..self.channels.max(0) as usize {
            // (a channel count above four reads the pixels after it in the original)
            self.out.push(self.buf[i + k / 4][k % 4]);
        }
    }

    /// A run: count byte `n + 0x7f` below 128 pixels, else 0x80 and the count as two bytes,
    /// high first; then the pixel.
    fn put_run(&mut self) {
        if self.n < 0x80 {
            self.out.push((self.n as u8).wrapping_add(0x7f));
        } else {
            self.out.push(0x80);
            self.out.extend_from_slice(&self.n.to_be_bytes());
        }
        self.put(0);
    }
}

/// Run-codes one line of up to four byte planes [soft 0x08262914 writeChanalMixed]:
/// `writeChanalMixed(file, count, channels, a, b, c, d)`.
///
/// A pixel is the four bytes (a, b, c, d) at one position, a missing plane counting as 0;
/// `channels` bytes of it are written (the planes given must be the first `channels`).
/// Pixels are compared on all four bytes. The coder holds either a list of pixels that each
/// differ from the one before, or a run of equal pixels, and looks at one pixel at a time:
///
/// - the second pixel held decides: equal to the first, the two are a run; else a list;
/// - a list grows while each pixel differs from the last one held; at 128 pixels it is
///   written (0x7f, then the pixels) and the coder is empty. A pixel equal to the last one
///   held ends the list: all but that last pixel are written (count byte: their number − 1)
///   and a run of two starts;
/// - a run grows while pixels equal it; at 65,535 it is written (0x80, the count as two
///   bytes high first, the pixel) and the coder is empty. A different pixel ends it: it is
///   written (count byte length + 0x7f for 2 to 127, else the 0x80 form) and the new pixel
///   is held alone;
/// - at the end of the line whatever is held is written: one pixel or a list as a list, a
///   run as a run.
///
/// So a short-form run of 128 (0xff) is never written, and two equal pixels are always a
/// run. `count` pixels are read from each plane given; a plane shorter than `count` is a
/// read past its end in the original and a panic here. Class: copy.
pub fn write_chanal_mixed(out: &mut Vec<u8>, count: i32, channels: i32, planes: [Option<&[u8]>; 4]) {
    let mut c = Coder { out, channels, buf: [[0; 4]; 128], n: 0, same: 0 };
    let mut at = 0usize;
    let mut left = count;
    // (a negative count never reaches 0 in the original: it reads on until memory ends)
    while left != 0 {
        let mut pix = [0u8; 4];
        for (k, p) in planes.iter().enumerate() {
            if let Some(p) = p {
                pix[k] = p[at];
            }
        }
        at += 1;
        if c.n == 0 {
            c.buf[0] = pix;
            c.n = 1;
        } else if c.n == 1 {
            c.same = (pix == c.buf[0]) as u16;
            if c.same == 0 {
                c.buf[1] = pix;
            }
            c.n = 2;
        } else {
            let equal = if c.same == 0 { pix == c.buf[c.n as usize - 1] } else { pix == c.buf[0] } as u16;
            if equal == c.same {
                if c.same == 0 {
                    c.buf[c.n as usize] = pix;
                }
                let before = c.n;
                c.n = c.n.wrapping_add(1);
                if before == 0x7f && c.same == 0 {
                    c.out.push(0x7f);
                    for i in 0..c.n as usize {
                        c.put(i);
                    }
                    c.n = 0;
                }
                if c.n == 0xffff && c.same != 0 {
                    c.out.push(0x80);
                    c.out.extend_from_slice(&c.n.to_be_bytes());
                    c.put(0);
                    c.n = 0;
                }
            } else if c.same == 0 {
                c.out.push((c.n as u8).wrapping_sub(2));
                for i in 0..c.n as usize - 1 {
                    c.put(i);
                }
                c.buf[0] = pix;
                c.buf[1] = pix;
                c.same = 1;
                c.n = 2;
            } else {
                c.put_run();
                c.buf[0] = pix;
                c.n = 1;
            }
        }
        left = left.wrapping_sub(1);
    }
    if c.n != 0 {
        if c.n == 1 || c.same == 0 {
            c.out.push((c.n as u8).wrapping_sub(1));
            for i in 0..c.n as usize {
                c.put(i);
            }
        } else {
            c.put_run();
        }
    }
}

/// What `rle.c` asks of the system and of the rest of the program. Paths are byte strings.
pub trait PictureHost {
    /// `access(path, mode)`: 0 or −1 (mode 0: exists; 2: may be written).
    fn access(&mut self, path: &[u8], mode: i32) -> i32;
    /// `fopen(path, "wb")` into `pFile` (0x0869c440); false for a null result.
    fn create(&mut self, path: &[u8]) -> bool;
    /// `fputc` and `fwrite` on `pFile`.
    fn write(&mut self, bytes: &[u8]);
    /// `fflush(pFile)` followed by `ferror(pFile)`: true when the file is in error.
    fn flush_failed(&mut self) -> bool;
    /// `fclose(pFile)`.
    fn close(&mut self);
    /// `creat(path, 0)`: whether the lock file could be made.
    fn lock(&mut self, path: &[u8]) -> bool;
    /// `close` and `unlink` of the lock file.
    fn unlock(&mut self, path: &[u8]);
    /// `SI_rleClose`'s second visit: `fopen(path, "rb+")`, the first 104 bytes read, handed
    /// to `change`, written back from the start, `fclose`. (The original does not test the
    /// open.)
    fn rewrite_head(&mut self, path: &[u8], change: &mut dyn FnMut(&mut [u8; 104]));
    /// `SI_addIconToPic(path)`.
    fn add_icon(&mut self, path: &[u8]);
    /// `si_fatal(text, more, code)` (`renutil.c`; ported on the image in the tenth pass,
    /// [`crate::renutil_image::fatal`]): reports and leaves the render through `SI_image`'s
    /// `longjmp`. An implementation that returns lets this file go on exactly as the
    /// original's instructions would.
    fn fatal(&mut self, text: &[u8], more: &[u8], code: i32);
}

/// The globals `si_rleInit` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PictureSetup {
    /// 0x0879117a, 0x0879117c: picture width and height.
    pub width: i16,
    pub height: i16,
    /// 0x08790e04: crop on; 0x08790e1c, 0x08790e1e: its width and height; 0x08790e0c,
    /// 0x08790e18: its x and y offsets.
    pub crop: i32,
    pub crop_width: i16,
    pub crop_height: i16,
    pub crop_x: f32,
    pub crop_y: f32,
    /// 0x08790d1c: field rendering; 0x08790d20: the current field (0 the first).
    pub field: i16,
    pub field_no: i16,
    /// 0x08790d5c: the output name; `SI_picturePath` (pointer at 0x086793cc): the directory
    /// a name that does not start with `/` is put under.
    pub name: Vec<u8>,
    pub picture_path: Vec<u8>,
    /// 0x08790dda: the frame number.
    pub frame: i16,
    /// 0x08790d34: the pixel ratio.
    pub ratio: f32,
}

/// The file-scope state of `rle.c`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PictureWriter {
    /// `frameName` (0x087990a0): the picture's path.
    pub frame_name: Vec<u8>,
    /// `pFile` (0x0869c440) is not null.
    pub open: bool,
    /// `withAlpha` (0x087992a0): set to 1 by `si_rleInit`, never cleared.
    pub with_alpha: i16,
}

impl PictureWriter {
    /// Opens the frame's picture and writes its header [soft 0x08262220 si_rleInit]:
    /// `si_rleInit(keep existing)`. Returns 1 when the frame is to be rendered, 0 when not.
    ///
    /// - Size: the picture's, or the crop's. Name: `<name>.<frame>.pic`, fields value 3; with
    ///   field rendering `<name>.<frame>.1.pic` for the first field (fields value 2) and
    ///   `.2.pic` for the second (1), the height halved (arithmetic shift). A name that
    ///   does not start with `/` goes under the picture directory.
    /// - The name's directory (up to its last `/`) must exist and be writable, else
    ///   `si_fatal(…, directory, 2)`.
    /// - `keep` = 0: an existing picture that may not be written is fatal; the file is
    ///   created. `keep` ≠ 0 (several machines on one sequence): `<path>.sema` is created
    ///   as a lock; if that fails, or the picture exists already, 0 is returned and the
    ///   frame is skipped; else the picture is created. The lock is removed either way.
    /// - If the file could not be created: `si_fatal("Cannot open output file : ", path,
    ///   1)`. Else the header: 88 bytes (magic, version 4.0, an 80-byte comment), `PICT`,
    ///   width, height, ratio, fields, the mark 0x004e (`N`), all high byte first; one
    ///   packet for red, green, blue (chained, 8 bits, mixed coding, 0xe0) and one for alpha
    ///   (0x10): the renderer always writes an alpha plane.
    ///
    /// The comment is `Done with the SOFTIMAGE's renderer`, zero filled; for a crop it is
    /// `SUBREGION <x> <y> IN <width> <height>` written by `sprintf` into a stack buffer:
    /// the bytes after its terminating zero are whatever the stack held (zero here).
    pub fn init(&mut self, host: &mut dyn PictureHost, g: &PictureSetup, keep: i16) -> i32 {
        use si_x87::{x, Round};
        let (mut width, mut height) = (g.width, g.height);
        if g.crop != 0 {
            (width, height) = (g.crop_width, g.crop_height);
        }
        let (fields, suffix): (u16, &str) = if g.field == 0 {
            (3, "")
        } else {
            height >>= 1;
            if g.field_no != 0 {
                (1, ".2")
            } else {
                (2, ".1")
            }
        };
        let mut path = Vec::new();
        // (an empty name reads the terminating zero, which is not `/`)
        if g.name.first() != Some(&b'/') {
            path.extend_from_slice(&g.picture_path);
            path.push(b'/');
        }
        path.extend_from_slice(&g.name);
        path.extend_from_slice(format!(".{}{suffix}.pic", g.frame).as_bytes());
        self.frame_name = path.clone();
        // the original writes a zero after the last `/` and faults if there is none
        let cut = path.iter().rposition(|&c| c == b'/').expect("the picture's path has a directory") + 1;
        let dir = &path[..cut];
        if host.access(dir, 0) == -1 {
            host.fatal(si_rt::message(0x0869_c4a0), dir, 2);
        }
        if host.access(dir, 2) == -1 {
            host.fatal(si_rt::message(0x0869_c4e0), dir, 2);
        }
        if keep == 0 {
            if host.access(&path, 0) == 0 && host.access(&path, 2) == -1 {
                host.fatal(si_rt::message(0x0869_c520), &path, 2);
            }
            self.open = host.create(&path);
        } else {
            let mut lock = path.clone();
            lock.extend_from_slice(b".sema");
            if !host.lock(&lock) {
                return 0;
            }
            if host.access(&path, 0) == 0 {
                host.unlock(&lock);
                return 0;
            }
            self.open = host.create(&path);
            host.unlock(&lock);
        }
        if !self.open {
            host.fatal(si_rt::message(0x0869_c5e3), &path, 1);
            return 1;
        }
        let mut head = Vec::with_capacity(112);
        head.extend_from_slice(&MAGIC.to_be_bytes());
        head.extend_from_slice(&VERSION.to_be_bytes());
        let mut comment = [0u8; 80];
        if g.crop != 0 {
            let text = format!("SUBREGION {} {} IN {} {}", x(g.crop_x).to_i32(Round::Zero), x(g.crop_y).to_i32(Round::Zero), g.width, g.height);
            comment[..text.len()].copy_from_slice(text.as_bytes());
        } else {
            comment[..self::comment().len()].copy_from_slice(self::comment());
        }
        head.extend_from_slice(&comment);
        head.extend_from_slice(b"PICT");
        head.extend_from_slice(&width.to_be_bytes());
        head.extend_from_slice(&height.to_be_bytes());
        head.extend_from_slice(&g.ratio.to_be_bytes());
        head.extend_from_slice(&fields.to_be_bytes());
        head.extend_from_slice(&[0x00, 0x4e]);
        head.extend_from_slice(&[1, 8, 2, 0xe0]);
        self.with_alpha = 1;
        head.extend_from_slice(&[0, 8, 2, 0x10]);
        host.write(&head);
        1
    }

    /// Adds one line to the picture [soft 0x08262854 SI_rleWrite]: `SI_rleWrite(line number,
    /// width, red, green, blue, alpha)`: the three colour planes as one run-coded sequence
    /// and, with `withAlpha`, the alpha plane as another. The file is flushed; if it is then
    /// in error it is closed and `si_fatal(path, " stop at line <n> (check disk space)", 1)`
    /// is called.
    pub fn write(&mut self, host: &mut dyn PictureHost, y: i32, width: i32, red: &[u8], green: &[u8], blue: &[u8], alpha: &[u8]) {
        let mut bytes = Vec::new();
        write_chanal_mixed(&mut bytes, width, 3, [Some(red), Some(green), Some(blue), None]);
        if self.with_alpha != 0 {
            write_chanal_mixed(&mut bytes, width, 1, [Some(alpha), None, None, None]);
        }
        host.write(&bytes);
        if host.flush_failed() {
            host.close();
            let name = self.frame_name.clone();
            host.fatal(&name, format!(" stop at line {y} (check disk space)").as_bytes(), 1);
        }
    }

    /// Closes the picture [soft 0x082626ec SI_rleClose]: `SI_rleClose(complete, icon)`.
    /// Nothing when no picture is open. The file is flushed; in error: closed and
    /// `si_fatal(path, " is incomplete (check disk space)", 1)` (after which the original,
    /// should that return, closes it a second time). Closed. With `complete` the file is
    /// opened again and the mark in its header (the two bytes at offset 102) is changed to
    /// 0x0043 (`C`). With `icon`, `SI_addIconToPic(path)`.
    pub fn close(&mut self, host: &mut dyn PictureHost, complete: i16, icon: i16) {
        if !self.open {
            return;
        }
        let name = self.frame_name.clone();
        if host.flush_failed() {
            host.close();
            host.fatal(&name, si_rt::message(0x0869_c600), 1);
        }
        host.close();
        if complete != 0 {
            host.rewrite_head(&name, &mut |head| {
                head[102] = 0x00;
                head[103] = 0x43;
            });
        }
        if icon != 0 {
            host.add_icon(&name);
        }
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(planes: [Option<&[u8]>; 4], channels: i32) -> Vec<u8> {
        let mut out = Vec::new();
        write_chanal_mixed(&mut out, planes[0].unwrap().len() as i32, channels, planes);
        out
    }

    #[test]
    fn runs_and_lists() {
        let one = |v: &[u8]| code([Some(v), None, None, None], 1);
        assert_eq!(one(&[]), Vec::<u8>::new());
        assert_eq!(one(&[5]), vec![0, 5]);
        assert_eq!(one(&[5, 6, 7]), vec![2, 5, 6, 7]);
        // two equal pixels are always a run, even between others
        assert_eq!(one(&[5, 5]), vec![0x81, 5]);
        assert_eq!(one(&[1, 5, 5, 2]), vec![0, 1, 0x81, 5, 0, 2]);
        // a list ends before the pixel that starts a run
        assert_eq!(one(&[1, 2, 3, 3, 3, 4, 5]), vec![1, 1, 2, 0x82, 3, 1, 4, 5]);
        // runs: 127 in the short form, 128 in the long one; 0xff is never written
        assert_eq!(one(&[9; 127]), vec![0xfe, 9]);
        assert_eq!(one(&[9; 128]), vec![0x80, 0, 128, 9]);
        // a run of 65,535 is written at once; the 65,536th pixel starts afresh
        let long = vec![7u8; 65536];
        assert_eq!(one(&long), vec![0x80, 0xff, 0xff, 7, 0, 7]);
        // a list is cut at 128 pixels
        let ramp: Vec<u8> = (0..130).map(|i| i as u8).collect();
        let out = one(&ramp);
        assert_eq!((out[0], out.len()), (0x7f, 1 + 128 + 1 + 2));
        assert_eq!(&out[129..], &[1, 128, 129]);
        // three planes: pixels are compared whole and written plane by plane
        let (r, g, b) = ([1u8, 1, 1, 2], [3u8, 3, 3, 3], [4u8, 4, 5, 5]);
        assert_eq!(code([Some(&r), Some(&g), Some(&b), None], 3), vec![0x81, 1, 3, 4, 1, 1, 3, 5, 2, 3, 5]);
    }

    #[test]
    fn same_choices_as_the_interactive_writer() {
        // lines with runs of every length class, compared with si-picture's encoder
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        for case in 0..400 {
            let n = [1usize, 2, 3, 127, 128, 129, 255, 256, 257, 640, 1000][case % 11];
            let mut planes: Vec<Vec<u8>> = vec![Vec::new(); 3];
            while planes[0].len() < n {
                let run = match next() % 6 {
                    0 => 1,
                    1 => 2,
                    2 => 3 + next() % 5,
                    3 => 126 + next() % 5,
                    4 => 1,
                    _ => 1 + next() % 300,
                } as usize;
                let pixel = [(next() % 3) as u8, (next() % 2) as u8, (next() % 2) as u8];
                for _ in 0..run.min(n - planes[0].len()) {
                    for k in 0..3 {
                        planes[k].push(pixel[k]);
                    }
                }
            }
            let ours = code([Some(&planes[0]), Some(&planes[1]), Some(&planes[2]), None], 3);
            let mut theirs = Vec::new();
            si_picture::write_mixed(&mut theirs, &[&planes[0], &planes[1], &planes[2]]);
            assert_eq!(ours, theirs, "case {case}, {n} pixels");
            let alpha = code([Some(&planes[0]), None, None, None], 1);
            let mut theirs = Vec::new();
            si_picture::write_mixed(&mut theirs, &[&planes[0]]);
            assert_eq!(alpha, theirs);
        }
    }

    #[derive(Default)]
    struct Host {
        file: Vec<u8>,
        log: Vec<String>,
        exists: bool,
        fail: bool,
    }
    impl PictureHost for Host {
        fn access(&mut self, path: &[u8], mode: i32) -> i32 {
            self.log.push(format!("access {} {mode}", String::from_utf8_lossy(path)));
            if path.ends_with(b".pic") && !self.exists {
                -1
            } else {
                0
            }
        }
        fn create(&mut self, path: &[u8]) -> bool {
            self.log.push(format!("create {}", String::from_utf8_lossy(path)));
            true
        }
        fn write(&mut self, bytes: &[u8]) {
            self.file.extend_from_slice(bytes);
        }
        fn flush_failed(&mut self) -> bool {
            self.fail
        }
        fn close(&mut self) {
            self.log.push("close".into());
        }
        fn lock(&mut self, path: &[u8]) -> bool {
            self.log.push(format!("lock {}", String::from_utf8_lossy(path)));
            true
        }
        fn unlock(&mut self, _: &[u8]) {
            self.log.push("unlock".into());
        }
        fn rewrite_head(&mut self, _: &[u8], change: &mut dyn FnMut(&mut [u8; 104])) {
            let mut head: [u8; 104] = self.file[..104].try_into().unwrap();
            change(&mut head);
            self.file[..104].copy_from_slice(&head);
        }
        fn add_icon(&mut self, _: &[u8]) {
            self.log.push("icon".into());
        }
        fn fatal(&mut self, text: &[u8], more: &[u8], code: i32) {
            self.log.push(format!("fatal {}{} {code}", String::from_utf8_lossy(text), String::from_utf8_lossy(more)));
        }
    }

    #[test]
    fn a_picture_from_header_to_mark() {
        si_rt::skip_without_install!();
        let g = PictureSetup { width: 4, height: 2, name: b"shot".to_vec(), picture_path: b"/db/PICTURES".to_vec(), frame: 12, ratio: 1.0, ..PictureSetup::default() };
        let (mut w, mut host) = (PictureWriter::default(), Host::default());
        assert_eq!(w.init(&mut host, &g, 0), 1);
        assert_eq!(w.frame_name, b"/db/PICTURES/shot.12.pic");
        assert_eq!(host.log, vec!["access /db/PICTURES/ 0", "access /db/PICTURES/ 2", "access /db/PICTURES/shot.12.pic 0", "create /db/PICTURES/shot.12.pic"]);
        assert_eq!(host.file.len(), 112);
        assert_eq!(&host.file[..8], &[0x53, 0x80, 0xf6, 0x34, 0x40, 0x80, 0, 0]);
        assert_eq!(&host.file[8..42], comment());
        assert_eq!(&host.file[88..112], &[b'P', b'I', b'C', b'T', 0, 4, 0, 2, 0x3f, 0x80, 0, 0, 0, 3, 0, 0x4e, 1, 8, 2, 0xe0, 0, 8, 2, 0x10]);
        w.write(&mut host, 1, 4, &[1, 1, 1, 1], &[2, 2, 2, 2], &[3, 3, 3, 3], &[255, 255, 0, 0]);
        assert_eq!(&host.file[112..], &[0x83, 1, 2, 3, 0x81, 255, 0x81, 0]);
        w.close(&mut host, 1, 1);
        assert_eq!(&host.file[102..104], &[0, 0x43]);
        assert_eq!(&host.log[4..], &["close", "icon"]);
        assert!(!w.open);
        // the second field of a cropped frame, under an absolute name
        let g = PictureSetup { crop: 1, crop_width: 3, crop_height: 5, crop_x: 7.9, crop_y: 2.0, field: 1, field_no: 1, name: b"/tmp/x".to_vec(), frame: 3, width: 640, height: 486, ..g };
        let mut host = Host::default();
        assert_eq!(w.init(&mut host, &g, 0), 1);
        assert_eq!(w.frame_name, b"/tmp/x.3.2.pic");
        assert_eq!(&host.file[8..36], b"SUBREGION 7 2 IN 640 486\0\0\0\0");
        assert_eq!(&host.file[92..96], &[0, 3, 0, 2]);
        assert_eq!(&host.file[100..104], &[0, 1, 0, 0x4e]);
        // keeping existing pictures: an existing frame is skipped, the lock removed
        let mut host = Host { exists: true, ..Host::default() };
        assert_eq!(w.init(&mut host, &g, 1), 0);
        assert_eq!(&host.log[2..], &["lock /tmp/x.3.2.pic.sema", "access /tmp/x.3.2.pic 0", "unlock"]);
        // a full disk
        let mut host = Host { fail: true, ..Host::default() };
        w.open = true;
        w.write(&mut host, 9, 1, &[0], &[0], &[0], &[0]);
        assert_eq!(host.log, vec!["close", "fatal /tmp/x.3.2.pic stop at line 9 (check disk space) 1"]);
    }
}
