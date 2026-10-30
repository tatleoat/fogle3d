// SPDX-License-Identifier: MIT
//! Checks for the Everest routines of `si_imgfmt::everest` against the vectors of
//! `re/tools/si-imgfmt/emu_gen6.py`: the in-memory reader (and its wrapper) on made-up files,
//! the writer (and its wrapper) on made-up pictures. Compared with undefined words allowed
//! (`--partly-undefined`): the header's 992 bytes come from the stack and a plane byte the
//! reader never set from the allocator.

use crate::pict::{booked, mem_words, ptr, refused, same_log, same_words, word};
use crate::util::{cstr, mutate};
use si_imgfmt::everest::{self, ReadEverest};
use si_x87::vectors::{Case, Check, Verdict, Word};

fn read_check(c: &Case, wrapper: bool) -> Verdict {
    let name = cstr(&c.input("name").to_bytes()).to_vec();
    let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
    // `SI_softOpen` adds the extension `.ima` when the name has none
    let file = file.or_else(|| c.try_input(&format!("file:{}.ima", String::from_utf8_lossy(&name))).map(|f| f.to_bytes()));
    let r = if wrapper { everest::read_everest_picture(file.as_deref(), b"soft", &name) } else { everest::read_everest(file.as_deref(), b"soft", &name) };
    match r {
        Ok(r) => same_words("eax", c.try_output("eax"), &[Some(r.ret as u32 as u128)])
            .and(|| same_picture(c, &r))
            .and(|| same_log(c, &r.messages, b"")),
        Err(f) => refused(c, f.0),
    }
}

fn same_picture(c: &Case, r: &ReadEverest) -> Verdict {
    let rec = c.output("pic").words();
    let Some(p) = &r.picture else {
        return Verdict::from(rec.iter().all(|w| matches!(w, Word::Unwritten { .. })), || "the original wrote the picture record".into());
    };
    // width, height, ratio, fields, (the caller's pad word is not touched), four plane pointers
    let ours: Vec<Option<u128>> = vec![
        Some(p.width as u16 as u128),
        Some(p.height as u16 as u128),
        Some(p.ratio.to_bits() as u128),
        Some(p.fields as u16 as u128),
        None,
        ptr(&p.planes[0]),
        ptr(&p.planes[1]),
        ptr(&p.planes[2]),
        Some(0),
    ];
    let mut v = same_words("picture record", Some(c.output("pic")), &ours);
    for k in 0..3 {
        v = v.and(|| match (c.try_output(&format!("p{k}")), &p.planes[k]) {
            (Some(f), Some(m)) => same_words(&format!("plane {k}"), Some(f), &mem_words(m)),
            (None, Some(m)) => Verdict::from(m.len() > 70000 || m.is_empty(), || format!("plane {k} missing in the original")),
            (Some(_), None) => Verdict::Different(format!("plane {k} missing in the port")),
            (None, None) => Verdict::Same,
        });
    }
    v
}

fn write_check(c: &Case, wrapper: bool) -> Verdict {
    let name = cstr(&c.input("name").to_bytes()).to_vec();
    let w = c.input("pic").words();
    let mut pic = crate::util::picture(c, w);
    for (k, n) in ["r", "g", "b"].iter().enumerate() {
        pic.planes[k] = matches!(w[5 + k], Word::Ref { .. }).then(|| c.input(n).to_bytes());
    }
    let _ = word;
    let path = match si_picture::soft_open(&String::from_utf8_lossy(&name), Some(".ima"), "wb", &|_| false) {
        si_picture::SoftOpen::Path(p) => p,
        _ => return Verdict::Skip,
    };
    let made = if wrapper { everest::write_everest_picture(&pic) } else { everest::write_everest(&pic).map(|f| (1, f)) };
    match made {
        Ok((ret, ours)) => same_words("eax", c.try_output("eax"), &[Some(ret as u32 as u128)]).and(|| match c.try_output(&format!("file:{path}")) {
            Some(f) => {
                let theirs = f.to_bytes();
                let ours = mem_words(&ours);
                if theirs.len() != ours.len() {
                    return Verdict::Different(format!("file: {} bytes against {}", theirs.len(), ours.len()));
                }
                let mut flip = mutate();
                for (k, (a, b)) in theirs.iter().zip(&ours).enumerate() {
                    if let Some(mut b) = *b {
                        if flip {
                            b ^= 1;
                            flip = false;
                        }
                        if b != *a as u128 {
                            return Verdict::Different(format!("file byte {k:#x} is {a:#x}, port {b:#x}"));
                        }
                    }
                }
                Verdict::Same
            }
            None => Verdict::Different("no file written".into()),
        }),
        Err(f) => refused(c, f.0),
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureReadEverest" => |c: &Case| booked(c, read_check(c, false)),
        "readEverestPicture" => |c: &Case| booked(c, read_check(c, true)),
        "SI_pictureWriteEverest" => |c: &Case| booked(c, write_check(c, false)),
        "writeEverestPicture" => |c: &Case| booked(c, write_check(c, true)),
        _ => return None,
    })
}
