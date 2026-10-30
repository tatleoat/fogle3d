// SPDX-License-Identifier: MIT
//! Checks of the renderer's picture writer (`rle.c`) against the vectors of
//! `re/tools/si-render/emu/gen_out.py`: the bytes the original wrote to the harness's
//! in-memory files against the port's.
use si_render::rle::{write_chanal_mixed, PictureHost, PictureSetup, PictureWriter};
use si_render::rnd_tag::{header_write_binary, tag_allocate, tag_file_header, TagHeap, TagImage, TAG_VERSION};
use si_x87::vectors::{Case, Check, Verdict, Word};

fn plane(c: &Case, name: &str) -> Option<Vec<u8>> {
    let f = c.input(name);
    if f.is_null() {
        None
    } else {
        Some(f.u8s())
    }
}

fn same_bytes(what: &str, want: &[u8], got: &[u8]) -> Result<(), String> {
    if want == got {
        return Ok(());
    }
    let at = want.iter().zip(got).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
    Err(format!("{what}: original {} bytes, port {}; first difference at {at}", want.len(), got.len()))
}

fn verdict(r: Result<(), String>) -> Verdict {
    match r {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(e),
    }
}

fn check_mixed(c: &Case) -> Verdict {
    let planes: Vec<Option<Vec<u8>>> = ["a", "b", "c", "d"].iter().map(|n| plane(c, n)).collect();
    let mut out = Vec::new();
    write_chanal_mixed(&mut out, c.input("count").i32(), c.input("channels").i32(), [planes[0].as_deref(), planes[1].as_deref(), planes[2].as_deref(), planes[3].as_deref()]);
    verdict(same_bytes("file", &c.output("f").u8s(), &out))
}

/// The file system as the harness presents it: every `access` answers 0, every file opens,
/// nothing fails; what is written and asked is kept.
#[derive(Default)]
struct Host {
    file: Vec<u8>,
    asked: Vec<Vec<u8>>,
    closed: usize,
    head: Vec<u8>,
    fatal: bool,
}

impl PictureHost for Host {
    fn access(&mut self, path: &[u8], _: i32) -> i32 {
        self.asked.push(path.to_vec());
        0
    }
    fn create(&mut self, _: &[u8]) -> bool {
        true
    }
    fn write(&mut self, bytes: &[u8]) {
        self.file.extend_from_slice(bytes);
    }
    fn flush_failed(&mut self) -> bool {
        false
    }
    fn close(&mut self) {
        self.closed += 1;
    }
    fn lock(&mut self, _: &[u8]) -> bool {
        panic!("creat called")
    }
    fn unlock(&mut self, _: &[u8]) {}
    fn rewrite_head(&mut self, _: &[u8], change: &mut dyn FnMut(&mut [u8; 104])) {
        let mut head: [u8; 104] = self.head[..104].try_into().expect("the picture has its header");
        change(&mut head);
        self.head[..104].copy_from_slice(&head);
    }
    fn add_icon(&mut self, _: &[u8]) {
        panic!("SI_addIconToPic called")
    }
    fn fatal(&mut self, _: &[u8], _: &[u8], _: i32) {
        self.fatal = true;
    }
}

fn cstr(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().take_while(|&&b| b != 0).copied().collect()
}

fn check_write(c: &Case) -> Verdict {
    let mut w = PictureWriter { open: true, with_alpha: c.input("g_alpha").i16(), ..PictureWriter::default() };
    let mut host = Host::default();
    w.write(&mut host, c.input("y").i32(), c.input("width").i32(), &c.input("r").u8s(), &c.input("g").u8s(), &c.input("b").u8s(), &c.input("a").u8s());
    verdict(same_bytes("file", &c.output("f").u8s(), &host.file))
}

fn check_init(c: &Case) -> Verdict {
    let g = PictureSetup {
        width: c.input("g_w").i16(),
        height: c.input("g_h").i16(),
        crop: c.input("g_crop").i32(),
        crop_width: c.input("g_cw").i16(),
        crop_height: c.input("g_ch").i16(),
        crop_x: c.input("g_cx").f32(),
        crop_y: c.input("g_cy").f32(),
        field: c.input("g_field").i16(),
        field_no: c.input("g_fno").i16(),
        name: cstr(&c.input("name").u8s()),
        picture_path: cstr(&c.input("path").u8s()),
        frame: c.input("g_frame").i16(),
        ratio: c.input("g_ratio").f32(),
    };
    let (mut w, mut host) = (PictureWriter::default(), Host::default());
    let got = w.init(&mut host, &g, c.input("keep").i16());
    verdict((|| {
        if c.output("eax").i32() != got {
            return Err(format!("eax: original {}, port {got}", c.output("eax").i32()));
        }
        // `frameName` as far as it was written: the text and its terminating zero
        let name: Vec<u8> = c.output("g_fname").written().iter().map_while(|w| w.map(|v| v as u8)).collect();
        same_bytes("frameName", &cstr(&name), &w.frame_name)?;
        if c.output("g_alpha").i16() != w.with_alpha {
            return Err("withAlpha".into());
        }
        for (k, path) in host.asked.iter().enumerate() {
            same_bytes(&format!("path of access call {k}"), &c.output(&format!("str{k}")).u8s(), path)?;
        }
        if c.try_output(&format!("str{}", host.asked.len())).is_some() {
            return Err("the original asked `access` more often".into());
        }
        let file = c.output(&format!("file:{}", String::from_utf8_lossy(&w.frame_name))).u8s();
        let mut port = host.file.clone();
        if g.crop != 0 && file.len() == port.len() {
            // the comment was written by `sprintf` into a stack buffer: what follows its
            // terminating zero is whatever the stack held
            let end = 8 + port[8..88].iter().position(|&b| b == 0).unwrap_or(80) + 1;
            port[end.min(88)..88].copy_from_slice(&file[end.min(88)..88]);
        }
        same_bytes("picture file", &file, &port)
    })())
}

fn check_close(c: &Case) -> Verdict {
    let name = cstr(&c.input("g_fname").u8s());
    let path = format!("file:{}", String::from_utf8_lossy(&name));
    let mut w = PictureWriter { frame_name: name, open: true, with_alpha: 1 };
    let mut host = Host { head: c.input(&path).u8s(), ..Host::default() };
    w.close(&mut host, c.input("complete").i16(), c.input("icon").i16());
    verdict((|| {
        if w.open || !matches!(c.output("g_pfile").words()[0], Word::Value { value: 0, .. }) {
            return Err("pFile was not cleared".into());
        }
        if host.closed != 1 {
            return Err(format!("the port closed the file {} times", host.closed));
        }
        match c.try_output(&path) {
            Some(f) => same_bytes("the picture after the mark", &f.u8s(), &host.head),
            None => same_bytes("the picture, untouched", &c.input(&path).u8s(), &host.head),
        }
    })())
}

// ------------------------------------------------------------------------------ rnd_tag.c

/// The tag image of a vector: the record's words and three channels of two lines each.
fn tags_of(c: &Case) -> TagImage {
    let t = c.input("tags");
    let w = t.words();
    let v = |i: usize| w[i].value().unwrap_or(0);
    TagImage {
        width: v(1) as i16,
        height: v(2) as i16,
        ratio: f32::from_bits(v(3) as u32),
        w0c: v(4) as i16,
        channels: v(5) as i16,
        data: (0..3).map(|ch| (0..2).map(|l| c.try_input(&format!("b{ch}{l}")).map(|f| f.u8s()).unwrap_or_default()).collect()).collect(),
    }
}

/// Records the sizes asked of the heap.
struct Sizes(Vec<u32>);
impl TagHeap for Sizes {
    fn calloc(&mut self, size: u32) -> u32 {
        self.0.push(size);
        8
    }
    fn malloc(&mut self, size: u32) -> u32 {
        self.0.push(size);
        8
    }
}

fn check_tag_alloc(c: &Case) -> Verdict {
    let mut heap = Sizes(Vec::new());
    let Some(t) = tag_allocate(&mut heap) else { return Verdict::Different("the port returned none".into()) };
    let m = c.output("heap0").u32s();
    let port = [0, (t.width as u16 as u32) | (t.height as u16 as u32) << 16, t.ratio.to_bits(), (t.w0c as u16 as u32) | (t.channels as u16 as u32) << 16, 0];
    Verdict::from(c.output("eax").u32() != 0 && m == port && heap.0 == [m.len() as u32 * 4], || format!("record: original {m:x?}, port {port:x?}"))
}

fn check_tag_channels(c: &Case) -> Verdict {
    let mut t = tags_of(c);
    let mut heap = Sizes(Vec::new());
    let got = t.allocate_channels(&mut heap);
    verdict((|| {
        if c.output("eax").i32() != got {
            return Err(format!("eax: original {}, port {got}", c.output("eax").i32()));
        }
        let blocks: Vec<u32> = (0..).map_while(|k| c.try_output(&format!("heap{k}")).map(|f| f.len() as u32)).collect();
        if blocks != heap.0 {
            return Err(format!("blocks asked: original {blocks:?}, port {:?}", heap.0));
        }
        // every line block was zeroed over its whole length
        for (k, &size) in blocks.iter().enumerate() {
            let f = c.output(&format!("heap{k}"));
            let line = k > 0 && (k - 1) % (t.height as usize + 1) != 0;
            if line && f.written().iter().any(|w| *w != Some(0)) {
                return Err(format!("line block {k} ({size} bytes) is not zero throughout"));
            }
        }
        let table = matches!(c.after("tags").words()[6], Word::Ref { .. });
        if table != (got != 0) {
            return Err("the channel table pointer".into());
        }
        Ok(())
    })())
}

fn check_tag_header(c: &Case) -> Verdict {
    let comment = if c.input("comment").is_null() { None } else { Some(c.input("comment").u8s()) };
    verdict(same_bytes("header", &c.output("f").u8s(), &header_write_binary(comment.as_deref(), TAG_VERSION)))
}

fn check_tag_file(c: &Case) -> Verdict {
    let (path, bytes) = tag_file_header(&c.input("name").u8s(), &tags_of(c), TAG_VERSION);
    match c.try_output(&format!("file:{}", String::from_utf8_lossy(&path))) {
        Some(f) => verdict(same_bytes("tag file", &f.u8s(), &bytes)),
        None => Verdict::Different(format!("the original did not write {}", String::from_utf8_lossy(&path))),
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        // (tenth pass: each case also through the routine's form on the memory image)
        "writeChanalMixed" | "writeChanalMixed#long" => |c: &Case| check_mixed(c).and(|| crate::pass10::old_mixed(c)),
        "SI_rleWrite" => |c: &Case| check_write(c).and(|| crate::pass10::old_write(c)),
        "si_rleInit" => |c: &Case| check_init(c).and(|| crate::pass10::old_init(c)),
        "SI_rleClose" => |c: &Case| check_close(c).and(|| crate::pass10::old_close(c)),
        "si_tagWriteScanline" => |c: &Case| verdict(same_bytes("file", &c.output("f").u8s(), &tags_of(c).write_scanline(c.input("line").i32()))),
        "SI_tagAllocate" => check_tag_alloc,
        "SI_tagAllocateChannels" => check_tag_channels,
        "headerWriteBinary" | "headerWriteBinary#null" => check_tag_header,
        "si_tagWriteFileHeader" => check_tag_file,
        _ => return None,
    })
}
