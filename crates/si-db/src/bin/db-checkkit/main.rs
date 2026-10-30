// SPDX-License-Identifier: MIT
//! `db-checkkit <directory of vector files>`: the browser's functions on the shared image
//! (`dbicons.rs`, `dbmanager.rs`, `dbfile.rs`) against vectors from `re/tools/si-db/gen_db.py`
//! (finding 0007, 2026-10-28 section; finding 0011, "A check binary with si-checkkit").

use si_checkkit::Kit;
use si_db::dbicons as ic;
use si_db::dbfile as df;
use si_db::dbmanager as dm;
use si_db::dictlist as dl;

fn main() {
    Kit::new(&[
        ("si_dbFormatSequenceFrames", |m, _o, a| ic::format_sequence_frames(m, a.v("elem"), a.v("buf")).into()),
        ("si_dbNameIsPicture", |m, _o, a| ic::name_is_picture(m, a.v("name")).into()),
        ("dbGetChapIconName", |m, _o, a| ic::get_chap_icon_name(m, a.i("chap")).into()),
        ("SI_dbListIconSetSel", |m, _o, a| ic::list_icon_set_sel(m, a.v("dialog"), a.i("item"), a.i("index"), a.s("value")).into()),
        ("SI_dbListIconGetSel", |m, _o, a| ic::list_icon_get_sel(m, a.v("dialog"), a.i("item")).into()),
        ("SI_dbGetIconList", |m, _o, a| ic::get_icon_list(m, a.v("view"), a.v("count")).into()),
        ("si_dbListTurnOffIcons", |m, _o, a| ic::list_turn_off_icons(m, a.v("dialog"), a.i("which")).into()),
        ("SI_dbListPrepareChapterIcons", |m, o, a| ic::list_prepare_chapter_icons(m, o, a.v("dict")).into()),
        ("si_dbMElemBuildName", |m, o, a| dm::elem_build_name(m, o, a.v("info"), a.v("list"), a.i("chap"), a.v("elem"), a.v("out1"), a.v("out2")).into()),
        ("dbMtextlistResetFlags", |m, _o, a| dm::textlist_reset_flags(m, a.v("tl")).into()),
        ("dbMtextlistDelValidate", |m, _o, a| dm::textlist_del_validate(m, a.v("tl"), a.v("list"), a.v("selected"), a.v("deletable"), a.s("clear")).into()),
        ("dbMtextlistFlagSelected", |m, _o, a| dm::textlist_flag_selected(m, a.v("tl"), a.v("list")).into()),
        ("dbMtextlistUnSelFlagged", |m, _o, a| dm::textlist_unsel_flagged(m, a.v("tl"), a.v("list"), a.v("with"), a.v("sel"), a.v("unsel")).into()),
        ("dbfileCheckElemInList", |m, _o, a| df::check_elem_in_list(m, a.v("list"), a.v("name"), a.v("found")).into()),
        ("SI_dbDatabaseDelete", |m, o, a| df::database_delete(m, o, a.v("dir"), a.v("info"), a.s("ext"), a.s("only"), a.s("write"), a.v("path")).into()),
        ("dbGetIconData", |m, o, a| ic::get_icon_data(m, o, a.v("list"), a.v("elem"), a.v("tv"), a.v("node"), a.v("ow"), a.v("oh"), a.v("otw"), a.v("ofl"), a.v("opix")).into()),
        ("si_dbIconsOffsetOnSelected", |m, o, a| ic::icons_offset_on_selected(m, o, a.v("dialog"), a.v("tl"), a.i("item"), a.v("list")).into()),
        ("SI_dbListHandleIconEvent", |m, o, a| ic::list_handle_icon_event(m, o, a.v("dialog"), a.v("dict"), a.v("list"), a.v("tl"), a.i("item")).into()),
        ("SI_dbMGetInputLine", |m, o, a| dm::get_input_line(m, o, a.v("buf")).into()),
        ("SI_dbMListElems", |m, o, a| dm::list_elems(m, o, a.v("a"), a.v("b"), a.i("kind"), a.i("u"), a.v("name"), a.v("title")).into()),
        ("SI_dbMElemHeadersGet", |m, o, a| dm::elem_headers_get(m, o, a.v("info"), a.v("list"), a.i("idx"), a.s("flag")).into()),
        ("SI_dbMFlagUnused", |m, o, a| dm::flag_unused(m, o, a.v("info"), a.v("list"), a.s("flag")).into()),
        ("SI_dbMBuildTextlistFromName", |m, _o, a| dm::build_textlist_from_name(m, a.v("name")).into()),
        ("SI_dbMBuildElemDictFromName", |m, o, a| dm::build_elem_dict_from_name(m, o, a.v("name"), a.v("info"), a.v("unused"), a.v("out"), a.i("chap"), a.s("use")).into()),
        ("SI_dbMWhoUses", |m, o, a| dm::who_uses(m, o, a.v("info"), a.v("dialog"), a.v("list"), a.v("tl"), a.s("flag")).into()),
        ("SI_dbMFileBinary2Ascii", |m, o, a| dm::file_binary2ascii(m, o, a.i("chap"), a.v("file"), a.v("out1"), a.v("out2"), a.v("cmdout")).into()),
        ("si_dbMRequest", |m, o, a| dm::db_request(m, o, a.i("code"), a.i("n1"), a.i("n2"), a.v("path")).into()),
        ("SI_dbMRequest2", |m, o, a| dm::db_request2(m, o, a.i("code"), a.i("n1"), a.i("n2"), a.v("path")).into()),
        ("SI_dbMMessage", |m, o, a| dm::db_message(m, o, a.i("code"), a.s("id"), a.v("dlg"), a.v("a"), a.v("b")).into()),
        ("dbGetPathFromFullname", |m, _o, a| dl::get_path_from_fullname(m, a.v("full"), a.v("rest")).into()),
        ("dbConcatPathAndFilename", |m, _o, a| dl::concat_path_and_filename(m, a.v("dir"), a.v("name"), a.v("out")).into()),
        ("dbListCollapse", |m, o, a| dl::list_collapse(m, o, a.v("list"), a.i("idx")).into()),
        ("dbListCollapseAll", |m, _o, a| dl::list_collapse_all(m, a.v("dict"), a.v("list")).into()),
        ("dbDisplaySubDir", |m, _o, a| dl::display_sub_dir(m, a.v("list"), a.v("path")).into()),
        ("txtlistTransferSelection2ElemList", |m, _o, a| dl::txtlist_transfer_selection_to_elemlist(m, a.v("tl"), a.v("list"), a.s("skip")).into()),
        ("elemlistTransferFlags2TextList", |m, _o, a| dl::elemlist_transfer_flags_to_textlist(m, a.v("tl"), a.v("list"), a.s("skip")).into()),
        ("dbSetPickable", |m, o, a| dl::set_pickable(m, o, a.v("tl"), a.v("list"), a.s("skip"), a.s("flag")).into()),
        ("dialogGetPicked", |m, _o, a| dl::dialog_get_picked(m, a.v("tl"), a.v("found"), a.v("second")).into()),
        ("dialogGetPickedStr", |m, _o, a| dl::dialog_get_picked_str(m, a.v("tl")).into()),
        ("dbAddTypedElementToList", |m, o, a| dl::add_typed_element_to_list(m, o, a.v("name"), a.v("list"), a.s("kind")).into()),
        ("dbPrepareWithAndWithoutSuffixStrings", |m, o, a| dl::prepare_with_and_without_suffix(m, o, a.v("list"), a.v("name"), a.v("out1"), a.v("out2")).into()),
        ("dbPrepareReturnStrings", |m, o, a| dl::prepare_return_strings(m, o, a.v("name"), a.v("info"), a.v("list"), a.i("idx"), a.v("dir_out"), a.v("sfx_out")).into()),
        ("dbSetSubDirectory", |m, _o, a| dl::set_sub_directory(m, a.v("tl"), a.v("list")).into()),
        ("txtlistSetSelection", |m, _o, a| dl::txtlist_set_selection(m, a.v("tl"), a.s("skip"), a.s("value")).into()),
        ("dialogGetSuffixFromId", |m, _o, a| dl::dialog_get_suffix_from_id(m, a.i("id")).into()),
        ("dialogGetIdFromSuffix", |m, _o, a| dl::dialog_get_id_from_suffix(m, a.v("s")).into()),
        ("copySelFieldsInTextList", |m, _o, a| dl::copy_sel_fields_in_text_list(m, a.v("a"), a.v("b")).into()),
        ("reverseText", |m, _o, a| dl::reverse_text(m, a.v("tl")).into()),
        ("setSelectionDialog", |m, o, a| dl::set_selection_dialog(m, o, a.v("dlg"), a.i("chap")).into()),
        ("dbSearchString", |m, o, a| dl::search_string(m, o, a.v("list"), a.v("tl"), a.v("text"), a.s("p4"), a.s("p5"), a.i("part"), a.s("from_sel"), a.s("all"), a.s("mark")).into()),
        ("SI_dbMFilesDelete", |m, o, a| dm::files_delete(m, o, a.v("info"), a.v("dict"), a.v("list"), a.i("chap"), a.v("tl"), a.s("flag")).into()),
        ("si_dbGetError", |m, _o, _a| dm::get_error(m).into()),
        ("si_dbSetError", |m, _o, a| dm::set_error(m, a.i("code")).into()),
    ])
    .setup(|m| {
        si_mem::init_globals(m, si_db::imglobals::ALL);
        ic::store_strings(m);
        dm::store_strings(m);
        dl::store_strings(m);
    })
    .reset_globals(&[(0x08679660, 0)])
    .run()
}
