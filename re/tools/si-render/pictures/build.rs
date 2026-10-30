// SPDX-License-Identifier: MIT
// render-frames (finding 0012, section 23): si-rensetup's corpus check as it stands, copied
// at build time with one change, so that it renders every frame of a scene's range (or one
// chosen frame) and writes the pictures. rensetup-check itself renders the first frame only
// whenever it writes pictures, and is another package's file: it is not edited, it is read.
use std::path::PathBuf;

fn main() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../crates/si-rensetup/src/bin/rensetup-check.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let text = std::fs::read_to_string(&src).expect("rensetup-check.rs");
    let from = "if std::env::var_os(\"RENSETUP_ALL_FRAMES\").is_none() || host.pictures.is_some() {";
    assert_eq!(text.matches(from).count(), 1, "rensetup-check.rs no longer has the line that picks the first frame: build.rs needs a look");
    let text = text.replace(from, "if frames_range(m) {");
    // the copy's main becomes a function: frames.rs has its own main, which runs it and then
    // finishes the picture of a sub-range's first frame (RENDER_RANGE)
    assert_eq!(text.matches("\nfn main() {").count(), 1, "rensetup-check.rs no longer has one `fn main() {{`: build.rs needs a look");
    let text = text.replace("\nfn main() {", "\nfn rensetup_main() {");
    // the install root comes from the install-path layer (si-install): the argument as before, else SI_INSTALL,
    // else the development copy; checked before the corpus walk (unit pkg1)
    let from = "let args: Vec<String> = std::env::args().skip(1).collect();";
    assert_eq!(text.matches(from).count(), 1, "rensetup-check.rs no longer reads its arguments in one line: build.rs needs a look");
    let text = text.replace(from, "let args: Vec<String> = install_args();");
    // inner doc comments cannot stand in an included file
    let text: String = text.lines().map(|l| if let Some(rest) = l.strip_prefix("//!") { format!("//{rest}\n") } else { format!("{l}\n") }).collect();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("rensetup_check.rs");
    std::fs::write(out, text).unwrap();
}
