// SPDX-License-Identifier: MIT
//! `render-frames <installed dir>` (finding 0012, section 23): `rensetup-check` as it stands
//! (`crates/si-rensetup/src/bin/rensetup-check.rs`, copied by `build.rs`), with the frame
//! range of each scene left as its set-up has it, so that the render command hands
//! `SI_image` the whole range in one call, as `soft -R <scene> -s <first> <last> <step>`
//! does. Every environment variable of `rensetup-check` works as there
//! (`RENSETUP_PICTURES=<dir>` for the pictures, `RENSETUP_ONLY=<text>` for a part of the
//! corpus). Two of its own:
//!
//! - `RENDER_FRAME=<n>`: frame n alone (first = last = n, the set-up's step), as
//!   `soft -R <scene> -s n n 1`;
//! - `RENDER_RANGE=<a>:<b>`: frames a to b of the range (b past the range's last is the last),
//!   for a driver that renders the range in several processes (`re/tools/perf1/render-par.py`);
//!   a must be a frame of the range (the first plus a multiple of the step). Every file is
//!   then the file a whole-range run writes: `SI_image` gives the picture of the first frame
//!   of the call its icon (`SI_rleClose(1, icon)`, icon 1 for the first frame only, 0
//!   after), so a sub-range that does not start at the range's first frame takes the icon
//!   of its first picture off again (the icon is appended at the end of the file and its
//!   offset is held by the last eight bytes: `si_picture::icon_offset`). Frames
//!   that are not a function of their number alone (a scene evaluation that carries state
//!   from frame to frame) are the driver's business: this mode renders what it is told to.
//! - `RENDER_PROBE=1`: renders nothing; prints `probe-range: <first> <last> <step>` for the
//!   scene (the driver's way of learning the range), after the set-up has been loaded;
//! - `RENDER_FIRST_ONLY=1`: the first frame only, as `rensetup-check` does.
//!
//! The lines of the pictures are not kept in memory (`si_render::adapter::keep_picture_lines`),
//! so the index's line count is 0. Nothing here reproduces a function of the original.

/// The frame range the render command takes from the controls: left as the set-up has it,
/// or one frame. Answers whether `rensetup-check`'s own narrowing to the first frame should
/// still run (never).
fn frames_range(m: &mut si_mem::Mem) -> bool {
    use std::sync::atomic::Ordering;
    use si_rensetup::native::globals as g;
    use si_rensetup::native::rec::ctl;
    si_render::adapter::keep_picture_lines(false);
    let (first, last) = (g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_FIRST, g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_LAST);
    if std::env::var_os("RENDER_PROBE").is_some() {
        let (f, l) = (m.i16(first), m.i16(last));
        println!("probe-range: {f} {l} {}", m.i16(g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_STEP));
        m.set_i16(last, f.wrapping_sub(1));
    } else if let Some(n) = std::env::var("RENDER_FRAME").ok().and_then(|v| v.parse::<i16>().ok()) {
        m.set_i16(first, n);
        m.set_i16(last, n);
    } else if let Some((a, b)) = std::env::var("RENDER_RANGE").ok().and_then(|v| {
        let (a, b) = v.split_once(':')?;
        Some((a.parse::<i16>().ok()?, b.parse::<i16>().ok()?))
    }) {
        let (f, l) = (m.i16(first), m.i16(last));
        let step = m.i16(g::SI_DEFAULT_CONTROL.0 + ctl::FRAME_STEP).max(1);
        // a frame the range's loop reaches
        if a < f || a > l || (a as i32 - f as i32) % step as i32 != 0 || b < a {
            eprintln!("RENDER_RANGE={a}:{b}: not inside the scene's range {f}..{l} step {step}");
            std::process::exit(3);
        }
        m.set_i16(first, a);
        m.set_i16(last, b.min(l));
        if a != f {
            SUB_FIRST.store(a as i32, Ordering::Relaxed);
        }
    } else if std::env::var_os("RENDER_FIRST_ONLY").is_some() {
        let f = m.i16(first);
        m.set_i16(last, f);
    }
    false
}

/// The first frame of a sub-range that is not the range's first (-1: none).
static SUB_FIRST: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Takes the icon off the picture file of frame `a` below `dir` (see `RENDER_RANGE`).
fn strip_icon(dir: &std::path::Path, a: i32) -> usize {
    let tail = format!(".{a}.pic");
    let mut done = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&tail)) {
                if let Ok(mut data) = std::fs::read(&p) {
                    if let Some(off) = si_picture::icon_offset(&data) {
                        if off < data.len() {
                            data.truncate(off);
                            if std::fs::write(&p, &data).is_ok() {
                                done += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    done
}

/// The command line with the install layer applied (unit pkg1, release model): `render-frames <dir>` as before;
/// with no argument the root is `SI_INSTALL`, else the development copy. The directory is checked against the
/// inventory (`docs/install-requirements.md`) and a clear message is printed when it is not an install.
/// `rules <Relations.rsrc>` is passed through. Then the program file of the user's install is read and checked
/// (crates/si-rt: the original's tables and messages, which this build does not carry): looked for under the root,
/// then `SI_INSTALL`, then the development copy; missing or another build: the reason and exit status 2.
fn install_args() -> Vec<String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "rules") {
        si_rt::check_or_exit("render-frames");
        return args;
    }
    if args.len() > 1 {
        si_rt::add_root(&args[0]);
        si_rt::check_or_exit("render-frames");
        return args;
    }
    match si_install::locate(args.first().map(String::as_str), si_install::Need::Render) {
        Ok(i) => {
            si_rt::add_root(&i.root);
            si_rt::check_or_exit("render-frames");
            args.clear();
            args.push(i.root.to_string_lossy().into_owned());
            args
        }
        Err(e) => {
            eprintln!("render-frames: {e}");
            std::process::exit(2);
        }
    }
}

fn main() {
    rensetup_main();
    let a = SUB_FIRST.load(std::sync::atomic::Ordering::Relaxed);
    if let (true, Some(dir)) = (a >= 0, std::env::var_os("RENSETUP_PICTURES")) {
        println!("sub-range: icon taken off {} picture(s) of frame {a}", strip_icon(std::path::Path::new(&dir), a));
    }
}

include!(concat!(env!("OUT_DIR"), "/rensetup_check.rs"));
