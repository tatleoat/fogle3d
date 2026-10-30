// SPDX-License-Identifier: MIT
//! `shdui-checkkit <directory of vector files>`: the shader rows of the material dialogs
//! (`shd_ui.rs`) on `si-checkkit` against the emulated original (vectors from
//! `re/tools/si-render/shdlist/gen_shdui.py`, finding 0023, section 39).

use si_checkkit::Kit;
use si_render::shd_ui::*;

fn main() {
    Kit::new(&[
        ("udfUiDelete", |m, o, a| udf_ui_delete(m, o, a.v("cell")).into()),
        ("udfUiEditText", |m, o, a| udf_ui_edit_text(m, o, a.v("dlg"), a.v("cell"), a.v("item") as i32).into()),
        ("udfUiSelect", |m, o, a| udf_ui_select(m, o, a.v("cell"), a.v("cid")).into()),
        ("syncUdfControls", |m, o, a| { sync_udf_controls(m, o, a.v("dlg"), a.v("rows"), a.v("count") as i32, a.v("names") as i32); ().into() }),
        ("SI_udfUiChangeName", |m, o, a| udf_ui_change_name(m, o, a.v("dlg"), a.v("rows"), a.v("count") as i32).into()),
        ("SI_udfUiHandleAction", |m, o, a| udf_ui_handle_action(m, o, a.v("dlg"), a.v("rows"), a.v("count") as i32, a.v("action") as i32).into()),
    ])
    .setup(si_render::shd_list::store_strings)
    .run()
}
