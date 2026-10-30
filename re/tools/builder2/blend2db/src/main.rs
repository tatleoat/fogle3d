// SPDX-License-Identifier: MIT
//! blend2db (unit builder1). Nothing here reproduces a function of the original; the writers it calls
//! are the crates' (si-ascii, si-elemio, si-hrc), which cite theirs.
//!
//!   blend2db roundtrip <db dir> <scene name> <rsrc dir> <out dir>
//!       writes <out dir>/{rsrc,db}: a database holding the scene's elements, each read with the
//!       crates' readers and written with their writers (version 4.0), for render-frames to render
//!       and compare with the original database's picture.
//!   blend2db build <scene.json> <out dir> <template db dir> <template scene> <rsrc dir>
//!       a database from a Blender scene (module build).
//!   blend2db build <scene.json> <out dir> [<install root>]
//!       the same with the template and the resource directory taken from the user's Softimage|3D 4.0
//!       install (root: the argument, else the environment variable SI_INSTALL, else the development copy).
mod build;
mod roundtrip;
mod dump;
mod construct;
mod fit;
mod anim;
mod simat;
mod shapes;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rc = match a.get(1).map(String::as_str) {
        Some("roundtrip") if a.len() == 6 => roundtrip::run(&a[2], &a[3], &a[4], &a[5]),
        Some("dump") if a.len() == 3 => dump::run(&a[2]),
        // install-path layer (unit pkg1): the template database, template scene and resource directory come
        // from the user's install (argument, else SI_INSTALL, else the development copy)
        Some("build") if a.len() == 4 || a.len() == 5 => {
            let arg = if a.len() == 5 { Some(a[4].as_str()) } else { None };
            match si_install::locate(arg, si_install::Need::Blend2db) {
                Ok(i) => {
                    let t = i.template_sts.expect("validated");
                    let tdb = t.parent().and_then(|d| d.parent()).expect("template path").to_string_lossy().into_owned();
                    let tscene = t.file_stem().expect("template name").to_string_lossy().into_owned();
                    build::run(&a[2], &a[3], &tdb, &tscene, &i.rsrc_dir.to_string_lossy())
                }
                Err(e) => {
                    eprintln!("blend2db: {e}");
                    2
                }
            }
        }
        Some("build") if a.len() == 7 => build::run(&a[2], &a[3], &a[4], &a[5], &a[6]),
        _ => {
            eprintln!("usage: blend2db roundtrip <db dir> <scene> <rsrc dir> <out dir>\n       blend2db build <scene.json> <out dir> <template db dir> <template scene> <rsrc dir>\n       blend2db build <scene.json> <out dir> [<install root>]");
            2
        }
    };
    std::process::exit(rc);
}
