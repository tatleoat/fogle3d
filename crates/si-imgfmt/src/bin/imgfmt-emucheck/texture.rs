// SPDX-License-Identifier: MIT
//! Checks for the texture loader and its cache (`si_imgfmt::texture`).

use crate::util::*;
use si_imgfmt::texture::{self, LclMap, OptimisedTexture, Options, TextureCache, MAP_SIZE};
use si_picture::PictureRgba;
use si_x87::vectors::{Case, Check, Verdict, Word};

fn word(c: &Case, name: &str) -> i16 {
    c.input(name).words()[0].value().unwrap_or(0) as u16 as i16
}

fn eax(c: &Case) -> u32 {
    c.output("eax").u32()
}

/// A name a pointer word leads to: the buffer's bytes up to the first zero.
fn name_at(c: &Case, w: &Word) -> Option<Vec<u8>> {
    match w {
        Word::Ref { name, offset } => c.try_input(name).map(|f| cstr(&f.to_bytes()[*offset as usize..]).to_vec()),
        _ => None,
    }
}

/// The map record of a case: 16 bytes, two pointers, 448 bytes. The pointers' own bytes are
/// zero in the image.
fn map_in(c: &Case) -> LclMap {
    let w = c.input("map").words();
    let mut raw = [0u8; MAP_SIZE];
    for i in 0..16 {
        raw[i] = w[i].value().unwrap_or(0) as u8;
    }
    for i in 0..448 {
        raw[24 + i] = w[18 + i].value().unwrap_or(0) as u8;
    }
    LclMap { raw, script: name_at(c, &w[16]), name: name_at(c, &w[17]) }
}

const ENTRY_WORDS: usize = 487;

fn cache_in(c: &Case) -> TextureCache {
    let count = c.input("props").words()[132].value().unwrap_or(0) as u32 as i32;
    let w = c.input("ents").words();
    let mut slots = Vec::new();
    for i in 0..count.max(0) as usize {
        let e = &w[ENTRY_WORDS * i..ENTRY_WORDS * (i + 1)];
        let v = |k: usize| e[k].value().unwrap_or(0);
        slots.push(name_at(c, &e[0]).map(|name| {
            let (width, height) = (v(1) as u16 as i16, v(2) as u16 as i16);
            let mut map = [0u8; MAP_SIZE];
            for k in 0..MAP_SIZE {
                map[k] = v(7 + k) as u8;
            }
            let flag = |k: usize| i16::from_le_bytes([v(7 + MAP_SIZE + 2 * k) as u8, v(8 + MAP_SIZE + 2 * k) as u8]);
            OptimisedTexture {
                name,
                picture: PictureRgba { width, height, ratio: f32::from_bits(v(3) as u32), fields: v(4) as u16 as i16, pad: v(5) as u16 as i16, data: name_bytes(c, &e[6]) },
                map,
                made: [flag(0), flag(1), flag(2), flag(3)],
            }
        }));
    }
    TextureCache { slots }
}

fn name_bytes(c: &Case, w: &Word) -> Vec<u8> {
    match w {
        Word::Ref { name, .. } => c.input(name).to_bytes(),
        _ => Vec::new(),
    }
}

/// `SI_pictureReadRGBA` on the case's files: the name with `.pic` added unless it ends so.
fn reader(c: &Case) -> impl FnMut(&[u8]) -> Option<PictureRgba> + '_ {
    move |name: &[u8]| {
        let mut path = String::from_utf8_lossy(name).into_owned();
        if !path.ends_with(".pic") {
            path.push_str(".pic");
        }
        let file = c.try_input(&format!("file:{path}"))?.to_bytes();
        si_picture::read_rgba(&file).ok()
    }
}

fn environment(c: &Case) -> (Option<Vec<u8>>, bool) {
    let e = c.input("env").to_bytes();
    ((e[0] == b'=').then(|| cstr(&e[1..8]).to_vec()), e[8] == b'o')
}

fn same_script(c: &Case, map: &LclMap) -> Verdict {
    let theirs = c.try_output("script").map(|f| f.to_bytes());
    Verdict::from(theirs == map.script, || format!("script name {:?}, port {:?}", theirs.as_deref().map(String::from_utf8_lossy), map.script.as_deref().map(String::from_utf8_lossy)))
}

fn read_crop(c: &Case, with_cache: bool) -> Verdict {
    let mut map = map_in(c);
    let mut cache = with_cache.then(|| cache_in(c));
    let before = cache.clone();
    let o = Options { border: word(c, "border"), swap: word(c, "swap"), tile: word(c, "tile"), no_alpha: word(c, "noalpha") };
    let (limit, old_uv) = environment(c);
    let limit = texture::texture_size_limit(limit.as_deref());
    let mut read = reader(c);
    match texture::pic_read_crop(&mut map, o, cache.as_mut(), b"/res", limit, old_uv, &mut read) {
        Ok(Some(p)) => {
            let mut v = Verdict::from(eax(c) == 1, || "the original returned nothing".into())
                .and(|| {
                    let r = c.output("rec").words();
                    same_head(r, p.width, p.height, p.ratio, p.fields, p.pad)
                })
                .and(|| match c.try_output("data") {
                    Some(d) => same_written("pixels", d, &wide(&p.data)),
                    None => Verdict::from(p.data.is_empty(), || "no pixels in the original".into()),
                })
                .and(|| same_script(c, &map));
            if let (Some(cache), Some(before)) = (&cache, &before) {
                for (i, slot) in cache.slots.iter().enumerate() {
                    if before.slots[i] == *slot {
                        continue;
                    }
                    let e = slot.as_ref().unwrap();
                    let w = &c.after("ents").words()[ENTRY_WORDS * i..ENTRY_WORDS * (i + 1)];
                    v = v
                        .and(|| Verdict::from(c.try_output(&format!("name{i}")).map(|f| f.to_bytes()).as_deref() == Some(&e.name[..]), || format!("name of entry {i}")))
                        .and(|| same_head(&w[1..6], e.picture.width, e.picture.height, e.picture.ratio, e.picture.fields, e.picture.pad))
                        .and(|| match c.try_output(&format!("kept{i}")) {
                            Some(d) => same_written("cached pixels", d, &wide(&e.picture.data)),
                            None => Verdict::from(e.picture.data.is_empty(), || "no cached pixels".into()),
                        })
                        .and(|| {
                            // the copy of the map, but for its two pointers; then the four flags
                            let theirs: Vec<u8> = w[7..7 + MAP_SIZE + 8].iter().map(|x| x.value().unwrap_or(0) as u8).collect();
                            let mut ours = e.map.to_vec();
                            ours.extend(e.made.iter().flat_map(|f| f.to_le_bytes()));
                            let skip = |k: usize| (0x10..0x18).contains(&k);
                            let same = (0..ours.len()).all(|k| skip(k) || theirs[k] == ours[k] ^ (mutate() && k == 0) as u8);
                            Verdict::from(same, || format!("map and flags of entry {i}"))
                        });
                }
            }
            v
        }
        Ok(None) => Verdict::from(eax(c) == 0, || "the original returned a picture".into()).and(|| same_script(c, &map)),
        Err(_) => Verdict::Skip,
    }
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "optimTextureSet" => |c: &Case| {
            let mut b = c.input("ent").to_bytes();
            let m = c.input("map").to_bytes();
            let map = LclMap { raw: m[..MAP_SIZE].try_into().unwrap(), script: None, name: None };
            let mut e = OptimisedTexture { name: Vec::new(), picture: PictureRgba::default(), map: [0; MAP_SIZE], made: [0; 4] };
            texture::optim_texture_set(&mut e, &map, word(c, "a"), word(c, "b"), word(c, "c"), word(c, "d"));
            b[0x14..0x1ec].copy_from_slice(&e.map);
            for (k, f) in e.made.iter().enumerate() {
                b[0x1ec + 2 * k..0x1ee + 2 * k].copy_from_slice(&f.to_le_bytes());
            }
            same_bytes("entry", &c.after("ent").to_bytes(), &b)
        },
        "optimTextureFind" => |c: &Case| match texture::optim_texture_find(&map_in(c), Some(&cache_in(c)), word(c, "a"), word(c, "b"), word(c, "c"), word(c, "d")) {
            Ok((found, at)) => Verdict::from((c.output("ax").u32() & 0xffff == 1) == found, || format!("found {}, port {found}", c.output("ax").u32()))
                .and(|| Verdict::from(c.output("idx").i32() == at ^ mutate() as i32, || format!("entry {}, port {at}", c.output("idx").i32()))),
            Err(_) => Verdict::Skip,
        },
        "loadScriptImage" => |c: &Case| {
            let mut map = map_in(c);
            let mut read = reader(c);
            match texture::load_script_image(&mut map, b"/res", &mut read) {
                Some(p) => Verdict::from(eax(c) == 1, || "the original failed".into())
                    .and(|| same_head(c.output("pic").words(), p.width, p.height, p.ratio, p.fields, p.pad))
                    .and(|| match c.try_output("data") {
                        Some(d) => same_written("pixels", d, &wide(&p.data)),
                        None => Verdict::Different("no pixels in the original".into()),
                    })
                    .and(|| same_script(c, &map)),
                None => Verdict::from(eax(c) == 0, || "the original read a picture".into()).and(|| same_script(c, &map)),
            }
        },
        "SI_picReadCrop" => |c: &Case| read_crop(c, true),
        "SI_picReadCrop#nocache" => |c: &Case| read_crop(c, false),
        _ => return None,
    })
}
