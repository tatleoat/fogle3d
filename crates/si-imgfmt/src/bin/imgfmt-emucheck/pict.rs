// SPDX-License-Identifier: MIT
//! Checks for the Macintosh PICT routines of `si_imgfmt::pict` against the vectors of
//! `re/tools/si-imgfmt/emu_gen5.py`: the reader on made-up files (with `PixMap`, `buf0` and
//! `buf1` handed in at their addresses and compared after the call), the writer on made-up
//! pictures, and `unPackBit9a` on made-up rows. Compared with undefined words allowed
//! (`--partly-undefined`), as the TIFF checks: a pixel the original takes from memory nobody
//! set is printed as dashes, and the port has it as `None`.

use crate::util::{cstr, mutate, text};
use si_imgfmt::pict::{self, PictPicture, PictState};
use si_imgfmt::tiff::{known, Blk};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

pub fn word(c: &Case, name: &str, i: usize) -> u128 {
    c.input(name).words()[i].value().unwrap_or(0)
}

/// Compares words with ours: `None` is not compared; an original pointer counts as 1.
pub fn same_words(what: &str, theirs: Option<&Field>, ours: &[Option<u128>]) -> Verdict {
    let Some(theirs) = theirs else { return Verdict::Different(format!("{what}: not in the vector")) };
    let w = theirs.words();
    if w.len() != ours.len() {
        return Verdict::Different(format!("{what}: {} words against {}", w.len(), ours.len()));
    }
    let mut flipped = !mutate();
    for (k, (t, o)) in w.iter().zip(ours).enumerate() {
        let Some(mut o) = *o else { continue };
        let t = match t {
            Word::Value { value, .. } => Some(*value),
            Word::Ref { .. } => Some(1),
            Word::Unwritten { .. } => None,
        };
        if !flipped {
            o ^= 1;
            flipped = true;
        }
        match t {
            Some(t) if t == o => {}
            Some(t) => return Verdict::Different(format!("{what}: word {k} is {t:#x}, port {o:#x}")),
            None => return Verdict::Different(format!("{what}: word {k} is undefined in the original, port {o:#x}")),
        }
    }
    Verdict::Same
}

pub fn mem_words(m: &[Option<u8>]) -> Vec<Option<u128>> {
    m.iter().map(|b| b.map(|v| v as u128)).collect()
}

pub fn ptr(b: &Blk) -> Option<u128> {
    Some(b.is_some() as u128)
}

/// What went to the status line: (text, level) for each call of the stub.
fn status(c: &Case) -> Vec<(Vec<u8>, i16)> {
    c.calls
        .iter()
        .filter(|k| k.name == "SI_statusDisplayMessage")
        .map(|k| {
            let s = match &k.args[0] {
                Word::Ref { name, .. } => c.try_output(name).map(|f| f.to_bytes()).unwrap_or_default(),
                _ => Vec::new(),
            };
            let s = s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())].to_vec();
            (s, k.args[1].value().unwrap_or(0) as u16 as i16)
        })
        .collect()
}

pub fn same_log(c: &Case, messages: &[(Vec<u8>, i16)], stderr: &[u8]) -> Verdict {
    let mut ours = messages.to_vec();
    if std::env::var("EMUCHECK_MUTATE").is_ok_and(|v| v == "calls") {
        if let Some(m) = ours.first_mut() {
            m.1 ^= 1;
        }
    }
    let st = status(c);
    Verdict::from(st == ours, || {
        format!(
            "messages {:?}, port {:?}",
            st.iter().map(|m| String::from_utf8_lossy(&m.0).into_owned()).collect::<Vec<_>>(),
            ours.iter().map(|m| String::from_utf8_lossy(&m.0).into_owned()).collect::<Vec<_>>()
        )
    })
    .and(|| {
        let theirs = c.try_output("stderr").map(|f| f.to_bytes()).unwrap_or_default();
        Verdict::from(theirs == stderr, || format!("stderr {:?}, port {:?}", c.try_output("stderr").map(text), String::from_utf8_lossy(stderr)))
    })
}

fn same_picture(c: &Case, p: Option<&PictPicture>) -> Verdict {
    let rec = c.output("pic").words();
    let Some(p) = p else {
        return Verdict::from(rec.iter().all(|w| matches!(w, Word::Unwritten { .. })), || "the original wrote the picture record".into());
    };
    let mut ours: Vec<Option<u128>> =
        vec![Some(p.width as u16 as u128), Some(p.height as u16 as u128), Some(p.ratio.to_bits() as u128), Some(p.fields as u16 as u128), None];
    ours.extend(p.planes.iter().map(ptr));
    let mut v = same_words("picture record", Some(c.output("pic")), &ours);
    let n = (p.width as i32 * p.height as i32).max(0) as usize;
    for k in 0..4 {
        v = v.and(|| match (c.try_output(&format!("p{k}")), &p.planes[k]) {
            (Some(f), Some(m)) => same_words(&format!("plane {k}"), Some(f), &mem_words(m)),
            (None, Some(m)) => Verdict::from(n == 0 || n > 65536 || m.len() != n, || format!("plane {k} missing in the original")),
            (Some(_), None) => Verdict::Different(format!("plane {k} missing in the port")),
            (None, None) => Verdict::Same,
        });
    }
    v
}

pub fn refused(c: &Case, why: &str) -> Verdict {
    if std::env::var("EMUCHECK_FAULTS").is_ok() {
        eprintln!("refused {} ({why}): notes {:?}", c.name, c.notes);
    }
    crate::FAULTS.with(|m| *m.borrow_mut().entry(format!("{}: {why}", c.name)).or_insert(0) += 1);
    Verdict::Skip
}

fn read_check(c: &Case) -> Verdict {
    let name = cstr(&c.input("name").to_bytes()).to_vec();
    let file = c.try_input(&format!("file:{}", String::from_utf8_lossy(&name))).map(|f| f.to_bytes());
    let mac = word(c, "mac", 0) as u32 as i32;
    let mut st = PictState::default();
    for (k, b) in c.input("pm").to_bytes().into_iter().enumerate() {
        st.pixmap[k] = Some(b);
    }
    let gb = c.input("gb").words();
    let opaque = |w: &Word| -> Blk { (w.value() != Some(0)).then(Vec::new) };
    st.buf0 = opaque(&gb[0]);
    st.buf1 = opaque(&gb[1]);
    match st.read(file.as_deref(), mac) {
        Ok(r) => same_words("eax", c.try_output("eax"), &[Some(r.ret as u32 as u128)])
            .and(|| same_picture(c, r.picture.as_ref()))
            .and(|| same_log(c, &r.messages, &r.stderr))
            .and(|| same_words("PixMap", c.try_output("pm"), &mem_words(&st.pixmap)))
            .and(|| same_words("buf0, buf1", c.try_output("gb"), &[ptr(&st.buf0), ptr(&st.buf1)])),
        Err(f) => refused(c, f.0),
    }
}

fn write_check(c: &Case) -> Verdict {
    let name = cstr(&c.input("name").to_bytes()).to_vec();
    let w = c.input("pic").words();
    let mut pic = crate::util::picture(c, w);
    for (k, n) in ["r", "g", "b", "a"].iter().enumerate() {
        pic.planes[k] = matches!(w[5 + k], Word::Ref { .. }).then(|| c.input(n).to_bytes());
    }
    let mac = word(c, "mac", 0) as u32 as i32;
    let path = match si_picture::soft_open(&String::from_utf8_lossy(&name), Some(".pict"), "wb", &|_| false) {
        si_picture::SoftOpen::Path(p) => p,
        _ => return Verdict::Skip,
    };
    match pict::write_pict(&pic, &name, mac) {
        Ok(mut ours) => {
            if mutate() {
                if let Some(b) = ours.last_mut() {
                    *b ^= 1;
                }
            }
            same_words("eax", c.try_output("eax"), &[Some(1)]).and(|| match c.try_output(&format!("file:{path}")) {
                Some(f) => {
                    let theirs = f.to_bytes();
                    Verdict::from(theirs == ours, || {
                        let k = theirs.iter().zip(&ours).position(|(a, b)| a != b).unwrap_or(theirs.len().min(ours.len()));
                        format!("file: {} bytes against {}, first difference at {k:#x}", theirs.len(), ours.len())
                    })
                }
                None => Verdict::Different("no file written".into()),
            })
        }
        Err(f) => refused(c, f.0),
    }
}

fn unpack_check(c: &Case) -> Verdict {
    let src = Some(known(&c.input("src").to_bytes()));
    let n = word(c, "n", 0) as u32 as i32 as i64;
    let len = c.output("dst").words().len();
    let mut dst: Blk = Some(vec![None; len]);
    match pict::un_pack_bit_9a(&mut dst, &src, n) {
        Ok(()) => same_words("dst", c.try_output("dst"), &mem_words(dst.as_ref().unwrap())),
        Err(f) => refused(c, f.0),
    }
}

/// Books a case that agrees with the original on every defined word while the original has
/// undefined ones (the report counts such a case as skipped).
pub fn booked(c: &Case, v: Verdict) -> Verdict {
    if v == Verdict::Same && c.has_note("UNINIT") {
        crate::PARTLY.with(|m| *m.borrow_mut().entry(c.name.clone()).or_insert(0) += 1);
    }
    v
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_pictureReadPict" => |c: &Case| booked(c, read_check(c)),
        "SI_pictureWritePict" => |c: &Case| booked(c, write_check(c)),
        "unPackBit9a" => |c: &Case| booked(c, unpack_check(c)),
        _ => return None,
    })
}
