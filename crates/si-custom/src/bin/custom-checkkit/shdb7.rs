// SPDX-License-Identifier: MIT
//! The ports of unit shdb7 (finding 0041, 2026-10-29): shdb_hooks' dynamic link, the static constructor, the translator thread
//! entry and the non-drawing routines of shdb_display. Chained from `shdb5::with`.

use si_checkkit::Port;
use si_custom::shdb_display::*;
use si_custom::shdb_global::*;
use si_custom::shdb_hooks::*;
use si_custom::shdb_translator::si_shdb_translator_proc;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("si_shdbHookDynalink", |m, o, a| si_shdb_hook_dynalink(m, o, a.v("handle"), a.v("start"), a.v("abort")).into()),
        ("si_shdbUnhookDynalink", |m, o, a| (si_shdb_unhook_dynalink(m, o, a.v("handle")) as u32).into()),
        ("0854e8a0", |m, o, a| { static_initialization_and_destruction_0(m, o, a.v("initialize"), a.v("priority")); ().into() }),
        ("0854e8e4", |m, o, _a| { global_i_s_mem(m, o); ().into() }),
        ("si_shdbTranslatorProc", |m, o, a| { si_shdb_translator_proc(m, o, a.v("task")); ().into() }),
        ("0854e3a4", |m, o, a| pipe_read_tile_header(m, o, a.v("fd"), a.v("stop"), a.v("scale"), a.v("rect")).into()),
        ("composeImgBuffer", |m, _o, a| { compose_img_buffer(m, a.v("dst"), a.v("src"), a.v("width"), a.v("r0"), a.v("r1"), a.v("scale") as i32); ().into() }),
        ("si_shdbDisplayCreate", |m, o, a| si_shdb_display_create(m, o, a.v("task")).into()),
        ("si_shdbDisplayClose", |m, o, a| { si_shdb_display_close(m, o, a.v("task")); ().into() }),
        ("si_shdbDisplayProcCleanup", |m, _o, a| { si_shdb_display_proc_cleanup(m, a.v("task")); ().into() }),
    ]);
    crate::cus1::with(&v)
}
