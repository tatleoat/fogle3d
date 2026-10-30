// SPDX-License-Identifier: MIT
//! Checks for the foreign image formats (crate `si-imgfmt`).

use crate::util::*;
use si_imgfmt::{alias, aurora, raw, targa};
use si_picture::Picture;
use si_x87::vectors::{Case, Check, Verdict};

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

/// An in-memory file of the case, if the generator supplied it.
fn opt_file(c: &Case, path: &str) -> Option<Vec<u8>> {
    c.try_input(&format!("file:{path}")).map(|f| f.to_bytes())
}

/// The caller's record: width, height, the spare word and whether the alpha slot is set.
fn caller(c: &Case) -> (i16, i16, i16, bool) {
    let w = c.input("pic").words();
    let v = |i: usize| w[i].value().unwrap_or(0) as u16 as i16;
    (v(0), v(1), v(4), w[8].value() != Some(0))
}

/// A file the routine wrote against ours: both absent, or both present and equal.
fn same_file(c: &Case, path: &str, ours: Option<&[u8]>) -> Verdict {
    match (file_out(c, path), ours) {
        (None, None) => Verdict::Same,
        (Some(t), Some(o)) => same_bytes(path, &t, o),
        (t, o) => Verdict::Different(format!("{path}: original wrote {}, port {}", t.is_some(), o.is_some())),
    }
}

/// A picture the routine read into the caller's record against ours (the spare word, which
/// these readers do not touch, taken from the caller).
fn same_read(c: &Case, r: Result<Picture, String>) -> Verdict {
    match r {
        Ok(mut p) => {
            p.pad = caller(c).2;
            Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_picture(c, c.after("pic").words(), &p))
        }
        Err(e) => Verdict::from(eax(c) == 0, || format!("the port failed: {e}")),
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureReadRaw" => |c: &Case| {
            let (w, h, _, _) = caller(c);
            let files: Vec<Option<Vec<u8>>> = raw::RAW_EXTENSIONS.iter().map(|e| opt_file(c, &format!("/t/a{e}"))).collect();
            let src = raw::RawSource::Files([files[0].as_deref(), files[1].as_deref(), files[2].as_deref(), files[3].as_deref()]);
            same_read(c, raw::read_raw(w, h, src).map_err(|e| format!("{e:?}")))
        },
        "SI_pictureWriteRaw" => |c: &Case| {
            let p = picture(c, c.input("pic").words());
            match raw::write_raw(&p) {
                Ok(files) => {
                    let mut v = Verdict::from(eax(c) == 1, || "the original failed".into());
                    for (e, f) in raw::RAW_EXTENSIONS.iter().zip(&files) {
                        v = v.and(|| same_file(c, &format!("/t/o{e}"), f.as_deref()));
                    }
                    v
                }
                Err(e) => Verdict::Different(format!("the port failed: {e:?}")),
            }
        },
        "SI_pictureReadRgb" => |c: &Case| {
            let (w, h, _, want) = caller(c);
            let (rgb, rgba) = (opt_file(c, "/t/a.rgb"), opt_file(c, "/t/a.rgba"));
            same_read(c, raw::read_rgb(w, h, want, rgb.as_deref(), rgba.as_deref()).map_err(|e| format!("{e:?}")))
        },
        "SI_pictureWriteRgb" => |c: &Case| {
            let p = picture(c, c.input("pic").words());
            let alpha = c.input("alpha").i16() != 0;
            match raw::write_rgb(&p, alpha) {
                Ok(bytes) => Verdict::from(eax(c) == 1, || "the original failed".into())
                    .and(|| same_file(c, if alpha { "/t/o.rgba" } else { "/t/o.rgb" }, Some(&bytes))),
                Err(e) => Verdict::Different(format!("the port failed: {e:?}")),
            }
        },
        "SI_pictureReadAlias" => |c: &Case| {
            let data = file_in(c, "/t/a.als");
            // A last run longer than what is left of the picture is written in full by the
            // original, past the end of each plane and into whatever the allocator put there
            // (in the harness: the next plane). The port stops at the end; such files are
            // not compared.
            if data.len() >= 10 {
                let n = i16::from_be_bytes([data[0], data[1]]) as i32 * i16::from_be_bytes([data[2], data[3]]) as i32;
                let (mut at, mut i) = (0i32, 10usize);
                while at < n {
                    at += data.get(i).copied().unwrap_or(0xff) as i32;
                    i += 4;
                }
                if at > n {
                    return Verdict::Skip;
                }
            }
            same_read_new(c, alias::read_alias(&data).map_err(|e| format!("{e:?}")))
        },
        "SI_pictureReadAurora" => |c: &Case| same_read_new(c, aurora::read_aurora(&file_in(c, "/t/a.sim")).map_err(|e| format!("{e:?}"))),
        "pictureWriteAlias" => |c: &Case| written(c, "/t/o.als", alias::write_alias(&picture(c, c.input("pic").words())).map_err(|e| format!("{e:?}"))),
        "SI_pictureWriteAurora" => |c: &Case| written(c, "/t/o.sim", aurora::write_aurora(&picture(c, c.input("pic").words())).map_err(|e| format!("{e:?}"))),
        "SI_pictureImportAlias" => |c: &Case| {
            let r = alias::import_alias(&file_in(c, "/t/a.als"), Some(&c.input("comment").to_bytes()), false);
            imported(c, r.map_err(|e| (format!("{e:?}"), e.message("a.als"))))
        },
        "SI_pictureImportAurora" => |c: &Case| {
            let r = aurora::import_aurora(&file_in(c, "/t/a.sim"), Some(&c.input("comment").to_bytes()), false);
            imported(c, r.map_err(|e| (format!("{e:?}"), e.message("a.sim"))))
        },
        "SI_pictureExportAlias" => |c: &Case| written(c, "/t/o.als", alias::export_alias(&file_in(c, "/t/a.pic")).map_err(|e| format!("{e:?}"))),
        "SI_pictureExportAurora" => |c: &Case| {
            written(c, "/t/o.sim", aurora::export_aurora(&file_in(c, "/t/a.pic"), c.input("alpha").i32() as u8).map_err(|e| format!("{e:?}")))
        },
        "SI_pictureReadTarga" => |c: &Case| {
            let ratio = c.input("ratio").f64();
            let theirs_err = c.try_output("stderr").map(text).unwrap_or_default();
            match targa::read(&file_in(c, "/t/a.tga"), c.input("verbose").i16() != 0, c.input("opaque").i16() != 0, ratio) {
                Ok(f) => Verdict::from(theirs_err == f.stderr, || format!("stderr {theirs_err:?}, port {:?}", f.stderr))
                    .and(|| same_read_new(c, Ok(f.picture))),
                // where the original reads or writes memory that is not its own (an index
                // outside the colour map, a negative length) the port stops instead
                Err(targa::TargaError::Fault(_)) => Verdict::Skip,
                Err(e) => Verdict::from(eax(c) == 0, || format!("the port failed: {e:?}"))
                    .and(|| Verdict::from(messages(c) == Some(e.message("a.tga")).filter(|m| !m.is_empty()).into_iter().collect::<Vec<_>>(), || format!("messages {:?}, port {:?}", messages(c), e.message("a.tga")))),
            }
        },
        "SI_pictureWriteTarga" => |c: &Case| {
            written(c, "/t/o.tga", targa::write(&picture(c, c.input("pic").words()), c.input("mode").i16() != 0).map_err(|e| format!("{e:?}")))
        },
        _ => return None,
    })
}

/// A reader that fills a record of its own making (ratio, fields and planes all set by it).
fn same_read_new(c: &Case, r: Result<Picture, String>) -> Verdict {
    match r {
        Ok(p) => Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| {
            let words = c.output("pic").words();
            same_head(&words[..4], p.width, p.height, p.ratio, p.fields, 0).and(|| {
                let n = (p.width.max(0) as usize) * (p.height.max(0) as usize);
                let mut v = Verdict::Same;
                for i in 0..4 {
                    v = v.and(|| match (&p.planes[i], target(c, &words[5 + i])) {
                        (None, None) => Verdict::Same,
                        (Some(a), Some((f, _))) => same_written(&format!("plane {i}"), f, &wide(&a[..n.min(a.len())])),
                        (Some(a), None) => Verdict::from(a.is_empty(), || format!("plane {i} missing in the original")),
                        (None, Some(_)) => Verdict::Different(format!("plane {i} missing in the port")),
                    });
                }
                v
            })
        }),
        Err(e) => Verdict::from(eax(c) == 0, || format!("the port failed: {e}")),
    }
}

/// A writer: the file it wrote against ours.
fn written(c: &Case, path: &str, r: Result<Vec<u8>, String>) -> Verdict {
    match r {
        Ok(bytes) => Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_file(c, path, Some(&bytes))),
        Err(e) => Verdict::from(eax(c) == 0, || format!("the port failed: {e}")),
    }
}

/// An import: the picture file written (its spare word cleared on both sides), or the
/// message of the failure.
fn imported(c: &Case, r: Result<Vec<u8>, (String, String)>) -> Verdict {
    match r {
        Ok(mut bytes) => {
            if bytes.len() >= 104 {
                bytes[102] = 0;
                bytes[103] = 0;
            }
            Verdict::from(eax(c) == 1, || "the original failed".into()).and(|| same_file(c, "/t/o.pic", Some(&bytes)))
        }
        Err((e, m)) => Verdict::from(eax(c) == 0, || format!("the port failed: {e}"))
            .and(|| Verdict::from(messages(c) == Some(m.clone()).filter(|m| !m.is_empty()).into_iter().collect::<Vec<_>>(), || format!("messages {:?}, port {m:?}", messages(c)))),
    }
}
