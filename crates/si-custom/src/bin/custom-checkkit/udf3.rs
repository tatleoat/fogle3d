// SPDX-License-Identifier: MIT
//! The ports of unit shdb2 (custom_udf.c rest, shdb_hooks, shdb_display), kept apart from `main.rs`
//! so the lists merge cleanly.

use si_checkkit::Port;
use si_custom::custom_udf2::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("SI_udfGenerateLDflags", |m, o, a| (si_udf_generate_ldflags(m, o, a.v("a"), a.v("b")) as u32).into()),
        ("SI_udfGetShaderFlags", |m, o, a| si_udf_get_shader_flags(m, o, a.v("u")).into()),
        ("SI_udfGetOutputBufferFlags", |m, o, a| si_udf_get_output_buffer_flags(m, o, a.v("u")).into()),
        ("SI_udfListAdd", |m, o, _a| (si_udf_list_add(m, o) as u32).into()),
        ("SI_udfListUpdate", |m, _o, _a| { si_udf_list_update(m); ().into() }),
        ("shaderVerifyPicturePath", |m, o, a| (shader_verify_picture_path(m, o, a.v("dict"), a.v("path")) as u32).into()),
        ("SI_sequenceGetImage", |m, o, a| (si_sequence_get_image(m, o, a.v("seq"), a.v("n"), a.v("out")) as u32).into()),
        ("SI_udfGenerateTextureDefs", |m, o, a| (si_udf_generate_texture_defs(m, o, a.v("frame"), a.v("only")) as u32).into()),
    ]);
    v
}

/// The message the display stand-ins show, appended to the case's capture buffer (the generator's
/// `capture` body does the same in the emulator): length at +0, text from +4, each message and a newline.
const CAP: u32 = 0x0878_8f00;

pub fn effect(m: &mut si_mem::Mem, r: si_mem::Routine, a: &[u32], _answer: u32) -> Result<(), String> {
    // NO_CAPTURE: the SI_udfOutputToMI cases declare no capture buffer (the emulated original's display stub reads and writes nothing,
    // and its SI_strAppend stand-in builds no text, so the message is 0 or junk)
    if (r.addr == 0x083c_ca94 || r.addr == 0x083c_cf40) && !NO_CAPTURE.with(|c| c.get()) && CAP_DECLARED.with(|c| c.get()) {
        let mut s = if (0x0860_0000..0x0870_0000).contains(&a[0]) { format!("@{:08x}", a[0]).into_bytes() } else { m.cstr(a[0]) };
        s.push(b'\n');
        let n = m.u32(CAP);
        m.set_bytes(CAP + 4 + n, &s);
        m.set_u32(CAP, n + s.len() as u32);
    }
    if r.addr == 0x080f_cf20 && a.len() == 2 && ED_INDEX_FROM_MODEL.with(|c| c.get()) {
        // the stand-in of the unit custom12 check: stores the word at +0x10 of the model through the second argument
        let v = m.u32(a[0].wrapping_add(0x10));
        m.set_u32(a[1], v);
    }
    Ok(())
}

thread_local! {
    /// Set per case by the checker's trampoline: the case declares the capture buffer.
    pub static CAP_DECLARED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// Set by the SI_udfOutputToMI check: the display stand-ins capture nothing.
    pub static NO_CAPTURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set by the checks of `SI_cusRelDataCreate` (their stand-in of `SI_edGetElemFromModel` stores an index).
    pub static ED_INDEX_FROM_MODEL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
