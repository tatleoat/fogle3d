// SPDX-License-Identifier: MIT
//! `osoft2vert.c` (finding 0039): a mesh written as a Vertigo `.pol` file.
//!
//! Records, read from the listings: a **model** holds its 4x4 matrix at +0x1c4 (the vertex row
//! `x y z 1` is multiplied from the left: the x column is +0x1c4, +0x1d4, +0x1e4, +0x1f4, y is
//! +0x1c8.., z +0x1cc..), its name at +4. A **mesh** holds the vertex array at +0 (0x14 bytes
//! each, the position first), the polygon array at +4 (0x18 bytes each: the vertex entries at
//! +0xc, 0x24 bytes each with the vertex number first, and their count as a 16-bit number at
//! +0x10), the number of vertices at +0x14 and of polygons at +0x18.

use crate::owave2soft::{print_message, ToolsCalls};
use crate::stdio::{dbl, fprintf, StdioCalls};
use si_mem::{clib, outside_calls, Mem, Outside};
use si_x87::x;

outside_calls! {
    /// What `osoft2vert.c` calls in other files and nobody has ported here: the display, the
    /// file name dialog and the object walk.
    pub trait VertCalls {
        /// `pushviewport (0x0860fdcc)`.
        fn pushviewport() = "pushviewport", 0x0860_fdcc;
        /// `viewport (0x0860fe90)`: (x, width, y, height), the first pushed last.
        fn viewport(a: i32, b: i32, c: i32, d: i32) = "viewport", 0x0860_fe90;
        /// `SI_clearSelect (0x08335034)`.
        fn si_clear_select() = "SI_clearSelect", 0x0833_5034;
        /// `popviewport (0x0860fde4)`.
        fn popviewport() = "popviewport", 0x0860_fde4;
        /// `SI_foolDisplay (0x08323a70)`: (graph list, 0, mode, node).
        fn si_fool_display(list: u32, a: i32, mode: i32, node: u32) = "SI_foolDisplay", 0x0832_3a70;
        /// `SI_findGlData (0x0832ee68)`: (graph list, model, 0).
        fn si_find_gl_data(list: u32, model: u32, a: i32) -> u32 = "SI_findGlData", 0x0832_ee68;
        /// `swapbuffers (0x08611804)`.
        fn swapbuffers() = "swapbuffers", 0x0861_1804;
        /// `si_objectFindNextActive (0x081baa68)`, `owave_ui.c`: (pointer to the mesh pointer,
        /// first): the next active object.
        fn si_object_find_next_active(pmesh: u32, first: i32) -> u32 = "si_objectFindNextActive", 0x081b_aa68;
        /// `SI_statusDisplayMessage (0x083cca94)`: (text, level).
        fn si_status_display_message(text: u32, level: i32) = "SI_statusDisplayMessage", 0x083c_ca94;
        /// `SI_dialogFileNameGet (0x083827a0)`: (extension, default name, 0): a text list.
        fn si_dialog_file_name_get(ext: u32, name: u32, a: i32) -> u32 = "SI_dialogFileNameGet", 0x0838_27a0;
        /// `SI_textListDispose (0x0838260c)`: (pointer to the list).
        fn si_text_list_dispose(plist: u32) = "SI_textListDispose", 0x0838_260c;
    }
}

const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;

const MSG_NO_OBJECT: u32 = 0x0869_06c0;
const MSG_CURRENT: u32 = 0x0869_06d6;
const EXT_POL_DIALOG: u32 = 0x0869_06e9;
const MSG_DONE: u32 = 0x0869_06ee;
const EXT_POL: u32 = 0x0869_06f4;
const MODE_W: u32 = 0x0869_06f9;
const FMT_ODD: u32 = 0x0869_06fb;
const FMT_EVEN: u32 = 0x0869_0713;
const FMT_NL: u32 = 0x0869_0722;
const FMT_FACE: u32 = 0x0869_0724;
const FMT_HOLE: u32 = 0x0869_0738;
const FMT_INDEX: u32 = 0x0869_0746;
const FMT_NL2: u32 = 0x0869_074b;
const FMT_END: u32 = 0x0869_074d;
const FMT_HEADER: u32 = 0x0869_0758;

/// [soft 0x081b9678 headerWriteVertigo]: the first line of the file (the name is not used).
pub fn header_write_vertigo(m: &mut Mem, o: &mut dyn Outside, file: u32, _name: u32) {
    fprintf(m, o, file, FMT_HEADER, &[]);
}

/// [soft 0x081b93dc pmeshWriteVertigo]: the mesh `mesh` of `model` written to the file named
/// `name` with ".pol" appended (the name's block is made 5 bytes longer than the name by
/// `SI_realloc`, and stays so): the vertices through the model's matrix, as doubles of the
/// unit's 80-bit sums, then the faces with their vertex numbers from 1 (a hole as `hole`).
/// Always 1, also when the file cannot be opened.
pub fn pmesh_write_vertigo(m: &mut Mem, o: &mut dyn Outside, model: u32, mesh: u32, name: u32) -> u32 {
    let len = clib::strlen(m, name);
    let name2 = m.si_realloc(name, len + 5);
    clib::strcat(m, name2, EXT_POL);
    let file = o.fopen(m, name2, MODE_W);
    if file == 0 {
        return 1;
    }
    header_write_vertigo(m, o, file, name);
    let nverts = m.i32(mesh + 0x14);
    let mut vp = 0u32;
    let mut i = 0i32;
    while i < nverts {
        let base = m.u32(mesh).wrapping_add(vp);
        let (v0, v1, v2) = (x(m.f32(base)), x(m.f32(base + 4)), x(m.f32(base + 8)));
        let mf = |m: &Mem, off: u32| m.f32(model + off);
        // the sums as the unit builds them (every product and sum in the 80-bit register)
        let t1 = ((v1 * mf(m, 0x1d4) + v0 * mf(m, 0x1c4)) + v2 * mf(m, 0x1e4)) + mf(m, 0x1f4);
        let t2 = ((v0 * mf(m, 0x1c8) + v1 * mf(m, 0x1d8)) + v2 * mf(m, 0x1e8)) + mf(m, 0x1f8);
        let t3 = (v2 * mf(m, 0x1ec) + (v1 * mf(m, 0x1dc) + v0 * mf(m, 0x1cc))) + mf(m, 0x1fc);
        let mut args = Vec::new();
        for t in [t1, t2, t3] {
            args.extend_from_slice(&dbl(t.to_f64()));
        }
        fprintf(m, o, file, if i & 1 != 0 { FMT_ODD } else { FMT_EVEN }, &args);
        i += 1;
        vp = vp.wrapping_add(0x14);
    }
    fprintf(m, o, file, FMT_NL, &[]);
    let npoly = m.i32(mesh + 0x18);
    let mut i = 0i32;
    while i < npoly {
        fprintf(m, o, file, FMT_FACE, &[i as u32]);
        let poly = m.u32(mesh + 4).wrapping_add((i as u32).wrapping_mul(0x18));
        let count = m.u16(poly + 0x10);
        let mut j = 0u32;
        while j < count as u32 {
            let entries = m.u32(poly + 0xc);
            let idx = m.i32(entries.wrapping_add(j * 0x24));
            if idx == -1 {
                fprintf(m, o, file, FMT_HOLE, &[]);
            } else {
                fprintf(m, o, file, FMT_INDEX, &[idx.wrapping_add(1) as u32]);
            }
            j += 1;
        }
        fprintf(m, o, file, FMT_NL2, &[]);
        i += 1;
    }
    fprintf(m, o, file, FMT_END, &[]);
    o.fclose(m, file);
    1
}

/// The graph list of the current process: `SI_grafLists + SI_procData->index * 0x24`.
fn graf_list(m: &Mem) -> u32 {
    let proc = m.u32(SI_PROC_DATA);
    m.u32(SI_GRAF_LISTS).wrapping_add(m.u32(proc.wrapping_add(4)).wrapping_mul(0x24))
}

/// [soft 0x081b9694 showObject]: the object drawn alone in the preview area of the window.
pub fn show_object(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    o.pushviewport(m);
    o.viewport(m, 0x80, 0x47f, 0x57, 0x3d0);
    o.si_clear_select(m);
    o.popviewport(m);
    let list = graf_list(m);
    o.si_fool_display(m, list, 0, 0, 0);
    let list = graf_list(m);
    let node = o.si_find_gl_data(m, list, model, 0);
    let list = graf_list(m);
    o.si_fool_display(m, list, 0, 3, node);
    o.swapbuffers(m);
}

/// [soft 0x081b9300 si_dialogModelWriteVertigo]: every active object written to a `.pol`
/// file whose name the user gives; the messages "No object to convert!" (level 3) and "Done."
/// (level 0). An object without a mesh gives the polygon-count warning and ends it.
pub fn si_dialog_model_write_vertigo(m: &mut Mem, o: &mut dyn Outside) {
    let pmesh = m.local(4);
    let plist = m.local(4);
    let mut model = o.si_object_find_next_active(m, pmesh, 1);
    if model == 0 {
        o.si_status_display_message(m, MSG_NO_OBJECT, 3);
    } else if m.u32(pmesh) == 0 {
        o.si_max_nb_pol_warning(m);
    } else {
        loop {
            show_object(m, o, model);
            let name = m.u32(model + 4);
            print_message(m, o, 1, MSG_CURRENT, &[name]);
            let list = o.si_dialog_file_name_get(m, EXT_POL_DIALOG, name, 0);
            m.set_u32(plist, list);
            if list != 0 {
                let entry = m.u32(list + 4);
                let fname = m.u32(entry + 4);
                let mesh = m.u32(pmesh);
                pmesh_write_vertigo(m, o, model, mesh, fname);
                o.si_mesh_dispose(m, pmesh);
                o.si_text_list_dispose(m, plist);
            }
            model = o.si_object_find_next_active(m, pmesh, 0);
            if model == 0 {
                break;
            }
        }
        o.si_status_display_message(m, MSG_DONE, 0);
    }
    m.release(4);
    m.release(4);
}
