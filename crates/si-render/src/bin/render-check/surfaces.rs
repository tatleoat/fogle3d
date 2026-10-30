// SPDX-License-Identifier: MIT
//! Every material and every 2D and 3D texture of the sample files through the renderer's
//! set-up (finding 0012, section 13).
//!
//! Per material (of a model's node, or a material file): the record from the file
//! (`si_render::fromfile`), `SI_materials` on a dictionary of that one material with every
//! render switch on and an ambience of 1, `SI_textures` with the picture files looked for
//! on the discs, then the texture routines themselves: nine look-ups a map, with and
//! without interpolation and bump neighbours, each blended into the material
//! (`SI_renderTxt2d`), and four points through the 3D texture (`SI_renderTxt3d`).
//!
//! Where a picture is looked for: the texture's path under the `PICTURES` directory of the
//! database the file lies in (what `SI_texturePath` is while that database is current); if
//! it is not there, any picture of that base name on the discs (counted apart).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use si_hrc::{read_file, Content, Kind};
use si_render::fromfile::{map_from_file, material_from_file, tex3d_from_file};
use si_render::materials::{materials, process_textures, DictElem, MaterialsEnv, Preview, TextureDict};
use si_render::renutil::blookup;
use si_render::shade::ShadeState;
use si_render::t2d_ren::render_txt2d;
use si_render::t3d_ren::render_txt3d;
use si_render::tt::TrigLib;
use si_render::types::Vector;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[derive(Default)]
struct Counts {
    materials: u64,
    models: BTreeMap<i16, u64>,
    flags: BTreeMap<u16, u64>,
    maps: u64,
    methods: BTreeMap<i16, u64>,
    tex3d: u64,
    kinds: BTreeMap<i32, u64>,
    tex3d_refused: BTreeMap<String, u64>,
    pictures_wanted: u64,
    found_here: u64,
    found_elsewhere: u64,
    not_found: u64,
    missing: BTreeMap<String, u64>,
    loaded: u64,
    replaced: u64,
    fatal: BTreeMap<String, u64>,
    lookups: u64,
    blends: u64,
    points3d: u64,
    panics: u64,
    panic_examples: Vec<String>,
    pixels: u64,
    cropped: u64,
    animated: u64,
}

/// The picture files of the discs by base name, and the database a file belongs to.
struct Pictures {
    by_name: BTreeMap<String, Vec<PathBuf>>,
    default: Option<Vec<u8>>,
}

struct Env<'a> {
    pictures: &'a Pictures,
    /// The `PICTURES` directory of the database of the file being checked.
    here: Option<PathBuf>,
    c: &'a mut Counts,
}

impl MaterialsEnv for Env<'_> {
    fn drand48(&mut self) -> f64 {
        0.5
    }
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>> {
        if default {
            self.c.replaced += 1;
            return self.pictures.default.clone();
        }
        self.c.pictures_wanted += 1;
        let name = String::from_utf8_lossy(name).into_owned();
        // the original's rule, through the crate's file host: `<name>.pic` for a name that
        // starts with `/`, else under the PICTURES directory of the file's own database
        // (what `SI_texturePath` is while that database is the current one)
        if let Some(dir) = &self.here {
            use si_render::frame::PictureFiles;
            use std::os::unix::ffi::OsStrExt;
            let mut disk = si_render::host::DiskFiles::direct();
            let mut by_rule = si_render::host::TexturePictures::new(&mut disk, dir.as_os_str().as_bytes(), b"");
            if let Some(d) = by_rule.picture_file(name.as_bytes(), false) {
                self.c.found_here += 1;
                return Some(d);
            }
        }
        let base = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
        if let Some(d) = self.pictures.by_name.get(&base).and_then(|paths| std::fs::read(&paths[0]).ok()) {
            self.c.found_elsewhere += 1;
            return Some(d);
        }
        self.c.not_found += 1;
        *self.c.missing.entry(name).or_default() += 1;
        None
    }
}

/// One material with its textures through the set-up and the texture routines.
fn one(material: &si_hrc::Material, own_maps: &[si_hrc::Texture2D], own_tex3d: Option<&si_hrc::Texture3D>, env: &mut Env) {
    let mut st = ShadeState::default();
    st.glb.textures = 1;
    st.glb.textures_2d = 1;
    st.glb.textures_3d = 1;
    st.glb.reflection = 1;
    st.glb.refraction = 1;
    st.glb.ambience = [1.0; 3];
    let mut m = material_from_file(material);
    env.c.materials += 1;
    *env.c.models.entry(m.model).or_default() += 1;
    let link = |st: &mut ShadeState, list: &[si_hrc::Texture2D], c: &mut Counts| {
        let first = st.maps.len();
        for (i, t) in list.iter().enumerate() {
            let mut map = map_from_file(t);
            map.next = (i + 1 < list.len()).then_some(first + i + 1);
            c.maps += 1;
            *c.methods.entry(map.method).or_default() += 1;
            c.cropped += u64::from(map.crop != 0);
            c.animated += u64::from(map.anim != 0);
            st.maps.push(map);
        }
        (!list.is_empty()).then_some(first)
    };
    m.maps = link(&mut st, &material.textures, env.c);
    let own = link(&mut st, own_maps, env.c);
    for t in [material.texture3d.as_ref(), own_tex3d].into_iter().flatten() {
        env.c.tex3d += 1;
        *env.c.kinds.entry(t.kind).or_default() += 1;
        match tex3d_from_file(t, &mut st.noise, &mut TrigLib) {
            Ok(r) => {
                st.tex3ds.push(r);
                if m.tex3d == 0 {
                    m.tex3d = st.tex3ds.len() as u32;
                }
            }
            Err(e) => *env.c.tex3d_refused.entry(e.to_string()).or_default() += 1,
        }
    }
    st.materials.push(m);
    st.models.push(si_render::shade::ShadeModel { maps: own, materials: vec![Some(0)], ..Default::default() });
    let mats = [DictElem { data: Some(0), ..DictElem::default() }];
    let models = [DictElem { data: Some(0), ..DictElem::default() }];
    if let Err(e) = materials(&mut st, &mats, &models, env) {
        *env.c.fatal.entry(format!("{e:?}").chars().take(60).collect()).or_default() += 1;
        return;
    }
    *env.c.flags.entry(st.materials[0].flags).or_default() += 1;
    let mut dict = TextureDict {
        pictures: (0..st.pictures.len()).map(|i| DictElem { data: Some(i), ..DictElem::default() }).collect(),
        maps: (0..st.maps.len()).map(|i| DictElem { data: Some(i), ..DictElem::default() }).collect(),
        models: Vec::new(),
    };
    if let Err(e) = process_textures(&mut st, &mut dict, 1, Preview { selection_call: 0, optimize: 1 }, env) {
        *env.c.fatal.entry(format!("{e:?}").chars().take(60).collect()).or_default() += 1;
        return;
    }
    for g in &st.pictures {
        if let Some(p) = &g.pixels {
            env.c.loaded += 1;
            env.c.pixels += (p.block.len() / 5) as u64;
        }
    }
    for map in 0..st.maps.len() {
        let Some(g) = st.maps[map].glb else { continue };
        if st.pictures[g].pixels.is_none() || st.pictures[g].width < 1 || st.pictures[g].height < 1 {
            continue;
        }
        let (lo, hi) = ([st.maps[map].umin, st.maps[map].vmin], [st.maps[map].umax, st.maps[map].vmax]);
        for i in 0..3 {
            for j in 0..3 {
                let u = lo[0] + (hi[0] - lo[0]) * (0.1 + 0.4 * i as f32);
                let v = lo[1] + (hi[1] - lo[1]) * (0.1 + 0.4 * j as f32);
                if !u.is_finite() || !v.is_finite() {
                    continue;
                }
                for interp in [0i16, 1] {
                    let s = blookup(&st, u, v, map, interp, true);
                    env.c.lookups += 1;
                    render_txt2d(&mut st, 0, map, s[0]);
                    env.c.blends += 1;
                }
            }
        }
    }
    if st.materials[0].tex3d != 0 {
        for p in [[0.0f32, 0.0, 0.0], [0.3, 1.7, -2.2], [10.5, -4.25, 3.0], [-0.01, 0.02, 0.5]] {
            let mut n = Vector::new(0.0, 0.0, 1.0);
            render_txt3d(&mut st, 0, Vector::new(p[0], p[1], p[2]), Some(&mut n), &mut TrigLib);
            env.c.points3d += 1;
        }
    }
}

pub fn run(dir: &Path) -> u64 {
    let mut files = Vec::new();
    walk(dir, &mut files);
    let mut pictures = Pictures { by_name: BTreeMap::new(), default: None };
    for p in &files {
        if p.extension().is_some_and(|e| e == "pic") {
            let base = p.file_stem().unwrap().to_string_lossy().to_lowercase();
            if base == "noicon" && pictures.default.is_none() {
                pictures.default = std::fs::read(p).ok();
            }
            pictures.by_name.entry(base).or_default().push(p.clone());
        }
    }
    let mut c = Counts::default();
    let mut sources = [0u64; 4];
    for path in &files {
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = match ext.as_str() {
            "hrc" => Kind::Model,
            "mtr" => Kind::Material,
            "txt" => Kind::Texture,
            "t3D" => Kind::Texture3D,
            _ => continue,
        };
        let Ok(data) = std::fs::read(path) else { continue };
        let Some(f) = read_file(&data, kind) else { continue };
        // <database>/<CHAPTER>/file: the database's pictures
        let here = path.parent().and_then(|p| p.parent()).map(|db| db.join("PICTURES")).filter(|p| p.is_dir());
        let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
        let mut work: Vec<(si_hrc::Material, Vec<si_hrc::Texture2D>, Option<si_hrc::Texture3D>)> = Vec::new();
        match &f.content {
            Content::Model(root) => {
                sources[0] += 1;
                root.walk(0, &mut |_, n| {
                    let mats: Vec<&si_hrc::Material> = n.materials.iter().flatten().collect();
                    if mats.is_empty() && (!n.textures.is_empty() || n.texture3d.is_some()) {
                        // a node with textures of its own and no material: the default material
                        work.push((si_hrc::Material::default(), n.textures.clone(), n.texture3d.clone()));
                    }
                    for (i, m) in mats.into_iter().enumerate() {
                        work.push((m.clone(), if i == 0 { n.textures.clone() } else { Vec::new() }, if i == 0 { n.texture3d.clone() } else { None }));
                    }
                });
            }
            Content::Material(m) => {
                sources[1] += 1;
                work.push((m.clone(), Vec::new(), None));
            }
            Content::Texture(t) => {
                sources[2] += 1;
                work.push((si_hrc::Material::default(), vec![t.clone()], None));
            }
            Content::Texture3D(t) => {
                sources[3] += 1;
                work.push((si_hrc::Material::default(), Vec::new(), Some(t.clone())));
            }
            Content::None => {}
        }
        for (k, (m, maps, t3)) in work.iter().enumerate() {
            let pictures = &pictures;
            let here = here.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut local = Counts::default();
                one(m, maps, t3.as_ref(), &mut Env { pictures, here, c: &mut local });
                local
            }));
            match result {
                Ok(local) => merge(&mut c, local),
                Err(_) => {
                    c.panics += 1;
                    if c.panic_examples.len() < 5 {
                        c.panic_examples.push(format!("{rel} (surface {k})"));
                    }
                }
            }
        }
    }
    println!("\nmaterials and textures (finding 0012, section 13)");
    println!("files read: {} models, {} materials, {} 2D textures, {} 3D textures; picture files on the discs: {} names", sources[0], sources[1], sources[2], sources[3], pictures.by_name.len());
    println!("materials through SI_materials: {}  by shading model: {:?}", c.materials, c.models);
    println!("  flag words they get (hex: count): {}", c.flags.iter().map(|(k, v)| format!("{k:x}: {v}")).collect::<Vec<_>>().join("  "));
    println!("2D textures: {}  by mapping method: {:?}  cropped {}  sequences or scripts {}", c.maps, c.methods, c.cropped, c.animated);
    println!("3D textures: {}  by kind (0 marble, 1 wood, 2 cloud): {:?}  refused: {:?}", c.tex3d, c.kinds, c.tex3d_refused);
    println!("picture files asked for: {}  found in the file's own database {}  found elsewhere by name {}  not found {} (the default picture read instead: {})", c.pictures_wanted, c.found_here, c.found_elsewhere, c.not_found, c.replaced);
    println!("  pixel blocks loaded {} ({} pixels with their borders)", c.loaded, c.pixels);
    let mut missing: Vec<_> = c.missing.iter().collect();
    missing.sort_by(|a, b| b.1.cmp(a.1));
    println!("  not found, most asked for: {}", missing.iter().take(8).map(|(k, v)| format!("{k} ({v})")).collect::<Vec<_>>().join(", "));
    println!("stopped by a fatal error of the original: {:?}", c.fatal);
    println!("look-ups {}  blends {}  points through a 3D texture {}", c.lookups, c.blends, c.points3d);
    println!("panics: {} {:?}", c.panics, c.panic_examples);
    c.panics
}

fn merge(c: &mut Counts, l: Counts) {
    c.materials += l.materials;
    c.maps += l.maps;
    c.tex3d += l.tex3d;
    c.pictures_wanted += l.pictures_wanted;
    c.found_here += l.found_here;
    c.found_elsewhere += l.found_elsewhere;
    c.not_found += l.not_found;
    c.loaded += l.loaded;
    c.replaced += l.replaced;
    c.lookups += l.lookups;
    c.blends += l.blends;
    c.points3d += l.points3d;
    c.pixels += l.pixels;
    c.cropped += l.cropped;
    c.animated += l.animated;
    for (k, v) in l.models {
        *c.models.entry(k).or_default() += v;
    }
    for (k, v) in l.flags {
        *c.flags.entry(k).or_default() += v;
    }
    for (k, v) in l.methods {
        *c.methods.entry(k).or_default() += v;
    }
    for (k, v) in l.kinds {
        *c.kinds.entry(k).or_default() += v;
    }
    for (k, v) in l.tex3d_refused {
        *c.tex3d_refused.entry(k).or_default() += v;
    }
    for (k, v) in l.missing {
        *c.missing.entry(k).or_default() += v;
    }
    for (k, v) in l.fatal {
        *c.fatal.entry(k).or_default() += v;
    }
}
