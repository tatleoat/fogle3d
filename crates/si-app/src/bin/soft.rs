// SPDX-License-Identifier: MIT
//! `soft`: the frame of the program. It takes the original's command line and runs the
//! original's start-up sequence and main loop against recording stand-ins for every part
//! that is not ported, printing each call the frame makes into those parts, in order.
//!
//!   soft [<resources path>] [-h] [-f] [-c] [-d <database>] [-l | -L <scene>] [-R …]
//!        [-s [<file>]] [-k <file>] [-C [<file>]] [-B] [-G] [-v] [-D <DSO> <function> …]
//!
//! No window is opened, no file read and no process started: the stand-ins answer from a
//! script. The built-in script lets a start-up succeed (a display of 1280 × 1024, the five
//! canvases, a reflection picture in the current directory) and ends the session after
//! three events. `SOFT_SCRIPT` adds to it or overrides it, in the recorder's syntax
//! (`name=v,v,…;$key=text;…`, see `si_app::env::Recorder`): for example
//! `SOFT_SCRIPT='qread=17,0x210;client.events=6'`, or `'$env.SI_LOG=14'`.
//! `SOFT_FILES` names files that exist, separated by colons.
//!
//! The lines printed are the record: `name(arguments)` for each call, with what the
//! program wrote to its two output streams after them and the exit status last.

use si_app::env::Recorder;
use si_app::{App, ScriptedClients};

const DEFAULT_SCRIPT: &str = "$cwd=/work;$locale=C;$user=user;$home=/home/user;$env.HOME=/home/user;\
SI_checkHardware=1;hw.xmax=1279;hw.ymax=1023;hw.planes=24;hw.olplanes=8;hw.wm=4095;hw.appf=1;hw.irisf=2;hw.logof=3;\
mem_zbuffer=1;winopen=1;copyRightDisplay=2;SI_canvasReadColor=1;SI_canvasRead=16,17,18,19,20;\
mem_have_input_database=1;mem_have_output_database=0;SI_dbCheckLocalDatabase=1;SI_dbLoadScene=1;SI_cusDsoGetRefreshFlag=1;\
qtest=1,0;client.events=3;getpid=4242;time=1012586329;localtime=102";

fn main() {
    let args: Vec<Vec<u8>> = std::env::args().map(|a| a.into_bytes()).collect();
    let mut script = DEFAULT_SCRIPT.to_string();
    if let Ok(more) = std::env::var("SOFT_SCRIPT") {
        script.push(';');
        script.push_str(&more);
    }
    let mut world = Recorder::from_script(script.as_bytes());
    world.files.insert(b"/work/Reflect.pic".to_vec(), Vec::new());
    if let Ok(files) = std::env::var("SOFT_FILES") {
        for f in files.split(':').filter(|f| !f.is_empty()) {
            world.files.insert(f.as_bytes().to_vec(), Vec::new());
        }
    }
    let mut app = App::new(world);
    let mut clients = ScriptedClients::default();
    let status = app.main(&mut clients, &args);
    for line in app.w.lines() {
        println!("{line}");
    }
    if !app.w.stdout.is_empty() {
        println!("--- standard output\n{}", String::from_utf8_lossy(&app.w.stdout).trim_end_matches('\n'));
    }
    if !app.w.stderr.is_empty() {
        println!("--- standard error\n{}", String::from_utf8_lossy(&app.w.stderr).trim_end_matches('\n'));
    }
    for path in &app.w.changed {
        match app.w.files.get(path) {
            Some(content) => println!("--- file {} ({} bytes)", String::from_utf8_lossy(path), content.len()),
            None => println!("--- file {} removed", String::from_utf8_lossy(path)),
        }
    }
    println!("--- exit status {status}");
    std::process::exit(0);
}
