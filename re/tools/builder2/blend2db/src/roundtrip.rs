// SPDX-License-Identifier: MIT
//! Round-trip test of the writers (design.md plan step 1): a corpus scene's database is read with the
//! crates' readers and written back with their writers into a new database, element by element.
use si_ascii::dsc::{self, CHAPTERS};
use si_ascii::setup;
use si_elemio::{camera, light, Reader, Writer};
use si_hrc::{Content, Kind};
use std::path::{Path, PathBuf};

pub fn first_diff(a: &[u8], b: &[u8]) -> String {
    let n = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    format!("first difference at {n} (original {} bytes, written {} bytes)", a.len(), b.len())
}

pub fn copy_rsrc(rsrc: &Path, out: &Path) -> std::io::Result<()> {
    let d = out.join("rsrc");
    std::fs::create_dir_all(&d)?;
    std::fs::copy(rsrc.join("Relations.rsrc"), d.join("Relations.rsrc"))?;
    if rsrc.join("noIcon.pic").exists() {
        std::fs::copy(rsrc.join("noIcon.pic"), d.join("noIcon.pic"))?;
    }
    Ok(())
}

/// What a file's writer gives, or None for a kind with no writer here (copied verbatim).
fn rewrite(suffix: &str, data: &[u8]) -> Result<Option<Vec<u8>>, String> {
    Ok(match suffix {
        "hrc" => match si_hrc::read_file(data, Kind::Model).map(|f| f.content) {
            Some(Content::Model(root)) => {
                // REBUILD=1: a single-mesh model is rebuilt from its plain arrays (construct.rs) first
                if std::env::var_os("REBUILD").is_some() && root.son.is_none() && root.brother.is_none() {
                    if let Some(d) = crate::construct::mesh_data_of(&root) {
                        let mut n = crate::construct::mesh_node(&d);
                        // what the constructor does not take from arrays: the file's own settings
                        n.scaling = root.scaling;
                        n.rotation = root.rotation;
                        n.translation = root.translation;
                        if let (Some(si_hrc::Definition::Mesh(a)), Some(si_hrc::Definition::Mesh(b))) = (n.definitions.first_mut(), root.definitions.first()) {
                            a.angle = b.angle;
                            if std::env::var_os("REBUILD_EDGES").is_some() { a.edges = b.edges.clone(); a.nedges = b.nedges; }
                        }
                        return Ok(Some(si_hrc::write_model(&n)));
                    }
                }
                Some(si_hrc::write_model(&root))
            }
            _ => return Err("model not read".into()),
        },
        "mtr" => match si_hrc::read_file(data, Kind::Material) {
            Some(f) => match f.content {
                Content::Material(m) => Some(si_hrc::write_material(&m, si_file::VERSION_OUTPUT)),
                _ => return Err("material not read".into()),
            },
            None => return Err("material not read".into()),
        },
        "txt" => match si_hrc::read_file(data, Kind::Texture).map(|f| f.content) {
            Some(Content::Texture(t)) => Some(si_hrc::write_texture(&t, si_file::VERSION_OUTPUT)),
            _ => return Err("texture not read".into()),
        },
        "t3D" => match si_hrc::read_file(data, Kind::Texture3D).map(|f| f.content) {
            Some(Content::Texture3D(t)) => Some(si_hrc::write_texture3d(&t, si_file::VERSION_OUTPUT)),
            _ => return Err("texture3d not read".into()),
        },
        "cam" => {
            let mut cam = camera::Camera::perspective_default();
            let f = camera::read_file(&mut Reader::new(data), &mut cam);
            if f.end != si_elemio::End::Zero {
                return Err("camera stream did not end at zero".into());
            }
            let mut w = Writer::new();
            camera::write_file(&mut w, &cam);
            Some(w.out)
        }
        "lig" => {
            let f = light::read_file(&mut Reader::new(data), 0);
            let l = f.read.light.ok_or("no light")?;
            let mut w = Writer::new();
            light::write_file(&mut w, &l);
            Some(w.out)
        }
        "sts" => {
            let (s, _) = setup::read(data).map_err(|e| format!("{e:?}"))?;
            Some(setup::write(&s, &setup::WriteOptions::default()))
        }
        _ => None,
    })
}

pub fn run(db: &str, scene: &str, rsrc: &str, out: &str) -> i32 {
    let (db, rsrc, out) = (PathBuf::from(db), PathBuf::from(rsrc), PathBuf::from(out));
    let dsc_path = db.join("SCENES").join(format!("{scene}.dsc"));
    let dsc_bytes = std::fs::read(&dsc_path).expect("dsc");
    let (d, _) = dsc::read(&dsc_bytes).expect("dsc read");
    copy_rsrc(&rsrc, &out).expect("rsrc");
    let odb = out.join("db");
    let (mut same, mut diff, mut copied, mut failed) = (0, 0, 0, 0);
    // the scene description itself: written with the writer
    let w = d.write(si_file::VERSION_OUTPUT);
    std::fs::create_dir_all(odb.join("SCENES")).unwrap();
    std::fs::write(odb.join("SCENES").join(format!("{scene}.dsc")), &w).unwrap();
    println!("SCENES/{scene}.dsc: {}", if w == dsc_bytes { "identical".to_string() } else { first_diff(&dsc_bytes, &w) });
    for ch in &d.chapters {
        let suffix = CHAPTERS.iter().find(|(n, _)| n.as_bytes() == ch.name).map(|c| c.1).unwrap_or("");
        let chname = String::from_utf8_lossy(&ch.name).to_string();
        if chname == "SCENES" {
            continue;
        }
        if chname == "PICTURES" {
            // a picture is named by a path of the machine it was made on; the loader finds it by its last
            // component in the database's PICTURES directory: copy the files of that stem
            for e in &ch.elements {
                let stem = String::from_utf8_lossy(&e.name).rsplit('/').next().unwrap_or("").to_string();
                if let Ok(rd) = std::fs::read_dir(db.join("PICTURES")) {
                    for f in rd.flatten() {
                        let n = f.file_name().to_string_lossy().to_string();
                        if n == stem || n.strip_prefix(&stem).is_some_and(|r| r.starts_with('.')) {
                            std::fs::create_dir_all(odb.join("PICTURES")).unwrap();
                            std::fs::copy(f.path(), odb.join("PICTURES").join(&n)).unwrap();
                            copied += 1;
                            println!("PICTURES/{n}: copied");
                        }
                    }
                }
            }
            continue;
        }
        for e in &ch.elements {
            if e.not_root != 0 || chname == "PICTURES" || chname == "RENDER_PICTURES" || suffix.is_empty() {
                continue;
            }
            let name = String::from_utf8_lossy(&e.name).to_string();
            let src = db.join(&chname).join(format!("{name}.{suffix}"));
            let Ok(data) = std::fs::read(&src) else {
                println!("{chname}/{name}.{suffix}: missing in the source database");
                failed += 1;
                continue;
            };
            std::fs::create_dir_all(odb.join(&chname)).unwrap();
            let dst = odb.join(&chname).join(format!("{name}.{suffix}"));
            match rewrite(suffix, &data) {
                Ok(Some(w)) => {
                    // header differs (version 4.0 / comment): compare after the 88-byte header for binary files
                    let eq_after = data.len() >= 88 && w.len() >= 88 && data[88..] == w[88..];
                    let eq = w == data;
                    if eq {
                        same += 1;
                    } else {
                        diff += 1;
                    }
                    println!("{chname}/{name}.{suffix}: {}", if eq { "identical".to_string() } else if eq_after { "identical after the header".to_string() } else { first_diff(&data, &w) });
                    std::fs::write(&dst, &w).unwrap();
                }
                Ok(None) => {
                    copied += 1;
                    println!("{chname}/{name}.{suffix}: no writer here, copied");
                    std::fs::write(&dst, &data).unwrap();
                }
                Err(m) => {
                    failed += 1;
                    println!("{chname}/{name}.{suffix}: FAILED {m}, copied");
                    std::fs::write(&dst, &data).unwrap();
                }
            }
        }
    }
    println!("ROUNDTRIP identical {same}, different {diff}, copied {copied}, failed {failed}");
    0
}
