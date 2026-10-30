// SPDX-License-Identifier: MIT
//! The picture writer on the memory image: `rle.c` (0x08262220) as [`crate::rle`] has it on
//! values, here in place (finding 0012, sections 8b and 19). The routines take the image
//! and what the original takes; the file's own state lies where the binary has it:
//! `pFile` (0x0869c440), `frameName` (0x087990a0, 512 bytes), `withAlpha` (0x087992a0).
//!
//! The C library's file calls go to a [`FileHost`]; `si_fatal` (0x08260560) and
//! `SI_addIconToPic` (0x085c4808) are called through [`Outside`] by address. A call of
//! `si_fatal` does not come back in the original; here the routine stops when the callee
//! says it jumped ([`Outside::jumped`]) and otherwise goes on exactly as the instructions
//! after the call would (which is how the emulated original is run: with a stand-in that
//! returns).

use crate::byteswap::{byteswap_fwrite, fix_file_chunk, fix_pict_chunk};
use crate::data::text;
use crate::host::FileHost;
use crate::img::{f, g, sprintf, FmtArg, Outside};
use si_mem::{clib, Mem};
use si_x87::{x, Round};

/// One line of up to four byte planes, run-coded, to a file
/// [soft 0x08262914 writeChanalMixed]: `writeChanalMixed(file, count, channels, a, b, c, d)`. The coding is
/// [`crate::rle::write_chanal_mixed`]'s, which this calls on the planes read from the
/// image; a null plane counts as 0.
///
/// Where the Rust differs: the original hands its bytes to `fputc` one at a time (a run's
/// long count through `SI_byteswap_fwrite`); here a line's bytes are handed over in one
/// write. A negative `count` never ends in the original (it reads on until memory does);
/// here it is a recorded fault of the image and nothing is written.
pub fn write_chanal_mixed(m: &mut Mem, files: &mut dyn FileHost, file: u32, count: i32, channels: i32, planes: [u32; 4]) {
    if count < 0 {
        m.record_fault(planes[0].wrapping_add(count as u32), 1, si_mem::Access::Read);
        return;
    }
    let held: Vec<Option<Vec<u8>>> = planes.iter().map(|&p| (p != 0).then(|| m.bytes(p, count as u32).to_vec())).collect();
    let mut out = Vec::new();
    crate::rle::write_chanal_mixed(&mut out, count, channels, [held[0].as_deref(), held[1].as_deref(), held[2].as_deref(), held[3].as_deref()]);
    if !out.is_empty() {
        files.fwrite(file, &out);
    }
}

/// Opens the frame's picture and writes its header [soft 0x08262220 si_rleInit]:
/// `si_rleInit(keep)`; 1 when the frame is to be rendered, 0 when it is to be skipped.
/// What it does is described at [`crate::rle::PictureWriter::init`]; this is the same on
/// the image, in the original's order:
///
/// 1. Size: the picture's (0x0879117a, 0x0879117c), or the crop's (0x08790e1c, 0x08790e1e)
///    when the crop word (0x08790e04, 32 bits) is not 0. With field rendering (0x08790d1c)
///    the height is shifted right by one and the fields word is 1 for the second field
///    (0x08790d20 not 0) and 2 for the first; else 3.
/// 2. `frameName` = `sprintf` of the name (0x08790d5c), the frame (0x08790dda) and the
///    field's suffix, under `SI_picturePath` unless the name starts with `/`.
/// 3. The name is copied to a local and cut after its last `/` (a name without one is a
///    write through a null pointer + 1 in the original, a recorded fault here); `access`
///    of that directory for existence and for writing, `si_fatal(…, directory, 2)` on −1.
/// 4. `keep` = 0: `access(frameName, 0)` and, when that is 0, `access(frameName, 2)`
///    (`si_fatal` on −1); `fopen(frameName, "wb")`. Else `creat("<frameName>.sema", 0)`:
///    on −1 the answer is 0; else `access(frameName, 0)`: 0 (the picture exists): `close`,
///    `unlink`, answer 0; else `fopen`, `close`, `unlink`.
/// 5. No file: `si_fatal("Cannot open output file : ", frameName, 1)`. Else the header in
///    four writes of 0x58, 4, 12, 4 and 4 bytes; `withAlpha` = 1.
///
/// The comment of a cropped picture is `sprintf`ed into the local header: the bytes after
/// its terminating zero are what the image's stack holds there.
pub fn rle_init(m: &mut Mem, env: &mut dyn Outside, files: &mut dyn FileHost, keep: i16) -> u32 {
    let sp = m.sp();
    let r = init(m, env, files, keep);
    m.set_sp(sp);
    r
}

fn init(m: &mut Mem, env: &mut dyn Outside, files: &mut dyn FileHost, keep: i16) -> u32 {
    // the locals, as the original's frame has them: the 0x68 bytes of header (88 of file
    // header, 12 of picture chunk at +0x58 … a copy of the two is made at `work`), two
    // names of 1024 bytes, the tag and the packet words
    let head = m.local(0x68);
    let dir = m.local(1024);
    let sema = m.local(1024);
    let work = m.local(0x58);
    let words = m.local(8);
    let (mut width, mut height) = (m.i16(g::WIDTH), m.i16(g::HEIGHT));
    if m.i32(g::CROP) != 0 {
        height = m.i16(g::CROP_HEIGHT);
        width = m.i16(g::CROP_WIDTH);
    }
    let name = m.u32(g::NAME);
    let absolute = m.u8(name) == b'/';
    let (fields, format): (u16, u32) = if m.i16(g::FIELDS) == 0 {
        (3, if absolute { text::FMT_PIC } else { text::FMT_PATH_PIC })
    } else {
        height >>= 1;
        if m.i16(g::FIELD_NOW) != 0 {
            (1, if absolute { text::FMT_PIC_2 } else { text::FMT_PATH_PIC_2 })
        } else {
            (2, if absolute { text::FMT_PIC_1 } else { text::FMT_PATH_PIC_1 })
        }
    };
    let frame = FmtArg::Int(m.i16(g::FRAME_NOW) as i32);
    let mut args = Vec::new();
    if !absolute {
        args.push(FmtArg::Text(m.cstr(m.u32(g::SI_PICTURE_PATH))));
    }
    args.push(FmtArg::Text(m.cstr(name)));
    args.push(frame);
    let path = sprintf(&m.cstr(format), &args);
    m.put_cstr(g::FRAME_NAME, &path);
    clib::strcpy(m, dir, g::FRAME_NAME);
    let slash = clib::strrchr(m, dir, b'/');
    m.set_u8(slash.wrapping_add(1), 0);
    macro_rules! fatal {
        ($text:expr, $more:expr, $code:expr) => {{
            env.call(m, f::SI_FATAL, &[$text, $more, $code]);
            if env.jumped() {
                return 1;
            }
        }};
    }
    if files.access(&m.cstr(dir), 0) == -1 {
        fatal!(text::NO_DIRECTORY, dir, 2);
    }
    if files.access(&m.cstr(dir), 2) == -1 {
        fatal!(text::DIRECTORY_REFUSED, dir, 2);
    }
    let path = m.cstr(g::FRAME_NAME);
    if keep == 0 {
        if files.access(&path, 0) == 0 && files.access(&path, 2) == -1 {
            fatal!(text::FILE_REFUSED, g::FRAME_NAME, 2);
        }
        let file = files.fopen(&path, &m.cstr(text::MODE_WB));
        m.set_u32(g::P_FILE, file);
    } else {
        let lock = sprintf(&m.cstr(text::FMT_SEMA), &[FmtArg::Text(path.clone())]);
        m.put_cstr(sema, &lock);
        let fd = files.creat(&lock, 0);
        if fd == -1 {
            return 0;
        }
        if files.access(&path, 0) == 0 {
            files.close(fd);
            files.unlink(&lock);
            return 0;
        }
        let file = files.fopen(&path, &m.cstr(text::MODE_WB_KEEP));
        m.set_u32(g::P_FILE, file);
        files.close(fd);
        files.unlink(&lock);
    }
    let file = m.u32(g::P_FILE);
    if file == 0 {
        fatal!(text::NO_OUTPUT, g::FRAME_NAME, 1);
        return 1;
    }
    // the file header: magic, version (the float 4.0 at 0x08654308), the comment
    m.set_u32(head, 0x5380_f634);
    m.set_f32(head + 4, 4.0);
    if m.i32(g::CROP) == 0 {
        clib::strncpy(m, head + 8, text::COMMENT, 0x50);
    } else {
        let a = [
            FmtArg::Int(x(m.f32(g::CROP_LEFT)).to_i32(Round::Zero)),
            FmtArg::Int(x(m.f32(g::CROP_BOTTOM)).to_i32(Round::Zero)),
            FmtArg::Int(m.i16(g::WIDTH) as i32),
            FmtArg::Int(m.i16(g::HEIGHT) as i32),
        ];
        let comment = sprintf(&m.cstr(text::FMT_SUBREGION), &a);
        m.put_cstr(head + 8, &comment);
    }
    m.copy(work, head, 0x58);
    fix_file_chunk(m, work);
    files.fwrite(file, m.bytes(work, 0x58));
    m.set_u32(words + 4, 0x5049_4354);
    byteswap_fwrite(m, files, words + 4, 4, 1, file);
    // the picture chunk: width, height, ratio, fields, and the mark `N`
    m.set_i16(work, width);
    m.set_i16(work + 2, height);
    let ratio = m.u32(g::PIXEL_RATIO);
    m.set_u32(work + 4, ratio);
    m.set_u16(work + 8, fields);
    m.set_u16(work + 10, 0x4e);
    fix_pict_chunk(m, work);
    files.fwrite(file, m.bytes(work, 0xc));
    // red, green and blue in one packet (chained, 8 bits, mixed run coding), then alpha
    m.set_bytes(words, &[1, 8, 2, 0xe0]);
    files.fwrite(file, m.bytes(words, 4));
    m.set_i16(g::WITH_ALPHA, 1);
    m.set_u32(words, 0x1002_0800);
    files.fwrite(file, m.bytes(words, 4));
    1
}

/// Adds one line to the picture [soft 0x08262854 SI_rleWrite]: `SI_rleWrite(line, width,
/// red, green, blue, alpha)`: the three colour planes as one run-coded sequence and, with
/// `withAlpha`, the alpha plane as another; `fflush`; when `ferror` then answers anything
/// the file is closed and `si_fatal(frameName, " stop at line <n> (check disk space)", 1)`
/// called.
pub fn rle_write(m: &mut Mem, env: &mut dyn Outside, files: &mut dyn FileHost, y: i32, width: i32, red: u32, green: u32, blue: u32, alpha: u32) {
    let sp = m.sp();
    let file = m.u32(g::P_FILE);
    write_chanal_mixed(m, files, file, width, 3, [red, green, blue, 0]);
    if m.i16(g::WITH_ALPHA) != 0 {
        let file = m.u32(g::P_FILE);
        write_chanal_mixed(m, files, file, width, 1, [alpha, 0, 0, 0]);
    }
    let file = m.u32(g::P_FILE);
    files.fflush(file);
    if files.ferror(file) != 0 {
        files.fclose(file);
        let more = m.local(256);
        let t = sprintf(&m.cstr(text::FMT_STOP_AT_LINE), &[FmtArg::Int(y)]);
        m.put_cstr(more, &t);
        env.call(m, f::SI_FATAL, &[g::FRAME_NAME, more, 1]);
    }
    m.set_sp(sp);
}

/// Closes the picture [soft 0x082626ec SI_rleClose]: `SI_rleClose(complete, icon)`.
/// Nothing when `pFile` is null. `fflush`; with `ferror`: `fclose` and
/// `si_fatal(frameName, " is incomplete (check disk space)", 1)`, after which (should that
/// come back) the file is closed a second time. `fclose`. With `complete` the file is
/// opened again (`rb+`), 0x58, 4 and 12 bytes are read into locals, the picture chunk is
/// swapped, its last word set to 0x0043 (`C`) and swapped back, the file rewound, the three
/// pieces written and the file closed. With `icon`, `SI_addIconToPic(frameName)`. `pFile`
/// becomes null.
///
/// A picture shorter than 104 bytes grows to 104: what could not be read is what the
/// locals held (the image's stack here). The original does not test the second `fopen`;
/// a null file is a fault there, and here the host is asked to read from no file.
pub fn rle_close(m: &mut Mem, env: &mut dyn Outside, files: &mut dyn FileHost, complete: i16, icon: i16) {
    let file = m.u32(g::P_FILE);
    if file == 0 {
        return;
    }
    let sp = m.sp();
    files.fflush(file);
    if files.ferror(file) != 0 {
        files.fclose(file);
        env.call(m, f::SI_FATAL, &[g::FRAME_NAME, text::INCOMPLETE, 1]);
        if env.jumped() {
            m.set_sp(sp);
            return;
        }
    }
    files.fclose(file);
    if complete != 0 {
        let path = m.cstr(g::FRAME_NAME);
        let file = files.fopen(&path, &m.cstr(text::MODE_RB_PLUS));
        m.set_u32(g::P_FILE, file);
        let head = m.local(0x58);
        let chunk = m.local(0xc);
        let tag = m.local(4);
        for (at, n) in [(head, 0x58), (tag, 4), (chunk, 0xc)] {
            let got = files.fread(file, n);
            m.set_bytes(at, &got);
        }
        fix_pict_chunk(m, chunk);
        m.set_u16(chunk + 10, 0x43);
        fix_pict_chunk(m, chunk);
        files.rewind(file);
        for (at, n) in [(head, 0x58), (tag, 4), (chunk, 0xc)] {
            files.fwrite(file, m.bytes(at, n));
        }
        files.fclose(file);
    }
    if icon != 0 {
        env.call(m, f::SI_ADD_ICON_TO_PIC, &[g::FRAME_NAME]);
    }
    m.set_u32(g::P_FILE, 0);
    m.set_sp(sp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemFiles;

    struct Calls(Vec<(u32, Vec<u32>)>);
    impl Outside for Calls {
        fn call(&mut self, _: &mut Mem, f: u32, a: &[u32]) -> u32 {
            self.0.push((f, a.to_vec()));
            0
        }
    }

    fn scene() -> (Mem, MemFiles) {
        let mut m = Mem::new();
        crate::data::init(&mut m);
        m.set_i16(g::WIDTH, 4);
        m.set_i16(g::HEIGHT, 2);
        m.set_f32(g::PIXEL_RATIO, 1.0);
        m.set_i16(g::FRAME_NOW, 12);
        let name = m.place_cstr(b"shot");
        m.set_u32(g::NAME, name);
        let path = m.place_cstr(b"/db/PICTURES");
        m.set_u32(g::SI_PICTURE_PATH, path);
        let mut h = MemFiles::new();
        h.files.insert(b"/db/PICTURES/".to_vec(), Vec::new());
        (m, h)
    }

    #[test]
    fn a_picture_on_the_image_from_header_to_mark() {
        si_rt::skip_without_install!();
        let (mut m, mut h) = scene();
        let mut env = Calls(Vec::new());
        assert_eq!(rle_init(&mut m, &mut env, &mut h, 0), 1);
        assert_eq!(m.cstr(g::FRAME_NAME), b"/db/PICTURES/shot.12.pic");
        assert_eq!(h.log, ["access /db/PICTURES/ 0", "access /db/PICTURES/ 2", "access /db/PICTURES/shot.12.pic 0", "fopen /db/PICTURES/shot.12.pic wb"]);
        assert_ne!(m.u32(g::P_FILE), 0);
        let lines = m.calloc(4, 4);
        m.set_bytes(lines, &[1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 255, 255, 0, 0]);
        rle_write(&mut m, &mut env, &mut h, 1, 4, lines, lines + 4, lines + 8, lines + 12);
        rle_close(&mut m, &mut env, &mut h, 1, 1);
        assert_eq!(m.u32(g::P_FILE), 0);
        let file = &h.files[&b"/db/PICTURES/shot.12.pic".to_vec()];
        assert_eq!(&file[..8], &[0x53, 0x80, 0xf6, 0x34, 0x40, 0x80, 0, 0]);
        assert_eq!(&file[8..42], b"Done with the SOFTIMAGE's renderer");
        assert!(file[42..88].iter().all(|&b| b == 0));
        assert_eq!(&file[88..112], &[b'P', b'I', b'C', b'T', 0, 4, 0, 2, 0x3f, 0x80, 0, 0, 0, 3, 0, 0x43, 1, 8, 2, 0xe0, 0, 8, 2, 0x10]);
        assert_eq!(&file[112..], &[0x83, 1, 2, 3, 0x81, 255, 0x81, 0]);
        // the icon was asked for, with the name's address
        assert_eq!(env.0, [(f::SI_ADD_ICON_TO_PIC, vec![g::FRAME_NAME])]);
        // and the project's reader takes it
        let pic = si_picture::OpenPicture::open_signed(file).expect("the reader opens it");
        assert_eq!((pic.picture.width, pic.picture.height), (4, 2));
    }

    #[test]
    fn a_missing_directory_and_a_kept_picture() {
        si_rt::skip_without_install!();
        let (mut m, mut h) = scene();
        h.files.clear();
        let mut env = Calls(Vec::new());
        // the stand-in for si_fatal returns: the routine goes on and opens the file
        assert_eq!(rle_init(&mut m, &mut env, &mut h, 0), 1);
        assert_eq!(env.0.len(), 2);
        assert_eq!((env.0[0].0, env.0[0].1[0], env.0[0].1[2]), (f::SI_FATAL, text::NO_DIRECTORY, 2));
        assert_eq!(env.0[1].1[0], text::DIRECTORY_REFUSED);
        // keeping what exists: the picture is there now, so the frame is skipped
        let (mut m, _) = scene();
        let before = h.files[&b"/db/PICTURES/shot.12.pic".to_vec()].clone();
        h.log.clear();
        assert_eq!(rle_init(&mut m, &mut env, &mut h, 1), 0);
        assert_eq!(&h.log[2..], ["creat /db/PICTURES/shot.12.pic.sema 0", "access /db/PICTURES/shot.12.pic 0", "close 4", "unlink /db/PICTURES/shot.12.pic.sema"]);
        assert_eq!(h.files[&b"/db/PICTURES/shot.12.pic".to_vec()], before);
    }
}
