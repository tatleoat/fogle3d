// SPDX-License-Identifier: MIT
//! The routines of other source files that the browser's source files (`dbicons.c`,
//! `dbmanager.c`, `dbexternal.c`) call, as typed calls on [`si_mem::Outside`] (one line each:
//! arguments, result, name, address).
//!
//! Nothing here reproduces a function of the original. A routine that is ported elsewhere
//! stays a call here until the program wires it (`kit routing-debt`); one that is not ported
//! is answered 0 by the default outside world.

use si_mem::outside_calls;

outside_calls! {
    /// What the browser's source files call outside themselves.
    pub trait DbCalls {
        /// `font.c`: selects the current font (the drawing state; no result).
        fn font(id: i32) = "font", 0x0860ec44;
        /// `font.c`: the width in pixels of a string in the current font.
        fn strwidth(s: u32) -> i32 = "strwidth", 0x0860f084;
        /// `gl` input: the value of a valuator (`getvaluator`).
        fn getvaluator(device: u32) -> i32 = "getvaluator", 0x0860e6a8;
        /// `gl` input: the state of a button (`getbutton`).
        fn getbutton(device: i32) -> i32 = "getbutton", 0x0860e684;
        /// `text.c`: strips the trailing digits of a name in place and returns it.
        fn str_strip_digit(s: u32) -> u32 = "SI_strStripDigit", 0x083cf154;
        /// `geticon.c`: the picture of a chapter's icon file by name (a block the caller frees), or null.
        fn db_chapter_icon_get(name: u32) -> u32 = "SI_dbChapterIconGet", 0x085c478c;
        /// `geticon.c`: the picture of an icon file by file name (a block the caller frees), or null.
        fn db_file_icon_get(name: u32) -> u32 = "SI_dbFileIconGet", 0x085c467c;
        /// `ui_misc.c`: shows the activity indicator.
        fn show_activity() = "SI_showActivity", 0x083d1a60;
        /// `dbexternal.c`: 1 when a database holds no chapter files (a short result).
        fn db_database_is_empty(info: u32) -> i32 = "SI_dbDatabaseIsEmpty", 0x0812711c;
        /// `dbexternal.c`: deletes an external database's directory; 0 when it failed.
        fn db_delete_ext_database(path: u32) -> i32 = "SI_dbDeleteExtDatabase", 0x081270cc;
        /// `elemdict.c`: forgets the origin database of the dictionary's elements by name.
        fn elemdict_reset_origin_db(dict: u32, name: u32) = "si_elemdictResetOriginDB", 0x08137884;
        /// `dbexternal.c`: the index of a database in a directory list, or -1.
        fn db_dir_get_database_index(dir: u32, info: u32) -> i32 = "SI_dbDirGetDatabaseIndex", 0x08121ba4;
        /// `dbexternal.c`: writes the directory list file (a short result, 0x5c when it failed).
        fn db_database_dir_write(dir: u32, path: u32, other: u32, flag: i32) -> i32 = "SI_dbDatabaseDirWrite", 0x08123d30;
        /// `custom`: the suffix of a custom (shader-database) chapter by its dictionary index.
        fn get_custom_suffix(chap: i32) -> u32 = "SI_getCustomSuffix", 0x0854c908;
        /// `dbexternal.c`: the file suffix of a chapter by its dictionary index.
        fn ed_get_suffix_from_chapter(chap: i32) -> u32 = "SI_edGetSuffixFromChapter", 0x08120a48;
        /// `dbexternal.c`: makes a name valid for a file (the answer replaces the string).
        fn dbname_check_validity(s: u32, flag: i32) -> u32 = "SI_dbnameCheckValidity", 0x08121bdc;
        /// `dbexternal.c`: builds `prefix-name` text into a buffer.
        fn db_format_prefix_name(dst: u32, prefix: u32, name: u32) -> u32 = "SI_dbFormatPrefixName", 0x08121ca8;
        /// `ui`: a one-button request dialog with three text lines; returns the answer.
        fn dialog_request(l1: u32, l2: u32, l3: u32, ok: u32) -> i32 = "SI_dialogRequest", 0x08141c00;
        /// `ui`: a two-button request dialog (three lines, two button texts).
        fn dialog_request2(l1: u32, l2: u32, l3: u32, b1: u32, b2: u32) -> i32 = "SI_dialogRequest2", 0x08141d5c;
        /// `ui`: a three-button request dialog.
        fn dialog_request3(l1: u32, l2: u32, l3: u32, b1: u32, b2: u32, b3: u32) -> i32 = "SI_dialogRequest3", 0x08141e84;
        /// `ui`: sets the title text of a dialog item.
        fn set_item_title(dlg: u32, id: i32, text: u32) = "SI_setItemTitle", 0x0838b664;
        /// `ui`: shows a message in the status line.
        fn status_display_message(text: u32, n: i32) = "SI_statusDisplayMessage", 0x083cca94;
        /// `libc`: reads one character from a stream (`_IO_getc`).
        fn io_getc(stream: u32) -> i32 = "_IO_getc", 0x080c4ff8;
        /// `libc`: `stat` (`__xstat(3, path, buf)`); 0 when it worked.
        fn xstat(version: i32, path: u32, buf: u32) -> i32 = "__xstat", 0x080c4238;
        /// `libc`: the process id.
        fn getpid() -> i32 = "getpid", 0x080c39b8;
        /// `custom` (dso code): whether the refresh flag is set (a `char`; only bit 0 is looked at).
        fn cus_dso_get_refresh_flag() -> u32 = "SI_cusDsoGetRefreshFlag", 0x0852e824;
        /// `dbexternal.c`/`dblist`: fills a text list with the elements of an element list.
        fn db_list_prepare_elements_list(a: u32, b: u32, c: i32, d: i32, out: u32, e: i32) = "si_dbListPrepareElementsList", 0x080e2ab0;
        /// `ui`: a scrolling message box with a text list.
        fn scrolling_msg_box_id(list: u32, id: i32, text: u32, title: u32) = "SI_scrollingMsgBoxId", 0x080e6c20;
        /// `ui`: disposes of a text list (the address of the pointer).
        fn text_list_dispose(list: u32) = "SI_textListDispose", 0x0838260c;
        /// `dbexternal.c`: makes a chapter the current one of a database record.
        fn db_set_chapter(info: u32, chap: u32) = "SI_dbSetChapter", 0x08122654;
        /// `dbexternal.c`: builds the file path of the current chapter of a database record.
        fn db_filepath_build(info: u32) = "SI_dbfilepathBuild", 0x081226cc;
        /// `dbexternal.c`: lists the elements of one chapter of a database into an element list.
        fn db_chapter_list_elements(info: u32, list: u32, suffix: u32, idx: i32, one: i32, flag: i32) = "SI_dbChapterListElements", 0x08125378;
        /// `dbexternal.c`: reads the sub-directories of a chapter.
        fn db_chapter_get_sub_dirs(info: u32, chap: u32, flag: i32) = "SI_dbChapterGetSubDirs", 0x0812508c;
        /// `elemlist.c`: initialises an element list record.
        fn elemlist_init(list: u32) = "SI_elemlistInit", 0x08133bc8;
        /// `elemlist.c`: disposes of an element list.
        fn elemlist_dispose(list: u32, flag: i32) = "SI_elemlistDispose", 0x08135fd4;
        /// `dbexternal.c`: scene-reference test of every marked element.
        fn dbfile_check_elements(info: u32, list: u32, out: u32, flag: i32) -> i32 = "SI_dbfileCheckElements", 0x08124c7c;
        /// `dbexternal.c`: scene-reference test of one element name.
        fn dbfile_check_element(info: u32, list: u32, out: u32, name: u32, a: i32, b: i32) -> i32 = "SI_dbfileCheckElement", 0x08124a88;
        /// `elemdict.c`: a list node for a scene file name.
        fn db_scn_get_list_node(name: u32, a: i32, b: i32) -> u32 = "SI_dbScnGetListNode", 0x081319f0;
        /// `elemlist.c`: adds a node to an element list (six more zero words).
        fn elemlist_add_elem(list: u32, node: u32, a: u32, b: u32, c: u32, d: u32, e: u32, f: u32) -> i32 = "SI_elemlistAddElem", 0x081327c4;
        /// `elemlist.c`: the element of an element list by index.
        fn elemlist_get_elem(list: u32, idx: i32) -> u32 = "SI_elemlistGetElem", 0x08132b1c;
        /// `dbexternal.c`: runs a command (name, arguments).
        fn db_system_command(cmd: u32, args: u32) -> i32 = "SI_dbSystemCommand", 0x08126964;
        /// `text.c`: a copy of a name without its suffix (a block the caller frees).
        fn str_strip_suffix(s: u32) -> u32 = "SI_strStripSuffix", 0x083cefd4;
        /// `elemdict.c`: the element list of a dictionary by chapter name.
        fn elemdict_find_list_by_name(dict: u32, name: u32) -> u32 = "SI_elemdictFindListByName", 0x081325f4;
        /// `ed`: marks an element pickable or not in a list.
        fn ed_set_pickable(elem: u32, list: u32, flag: i32) = "SI_edSetPickable", 0x080ff07c;
        /// `ui`: sets the title of a dialog.
        fn set_dialog_title(dlg: u32, text: u32) = "SI_setDialogTitle", 0x0838d048;
        /// `libc`: `access(path, mode)`; 0 when allowed.
        fn access(path: u32, mode: i32) -> i32 = "access", 0x080c3f18;
        /// `dbexternal.c`: whether a database may be written (a short result).
        fn db_database_check_access(info: u32, mode: i32) -> i32 = "SI_dbDatabaseCheckAccess", 0x08120ef4;
        /// `text.c`: strips a trailing number from a name when that is all there is (a short result).
        fn str_check_and_strip_digit_only(s: u32) -> i32 = "SI_strCheckAndStripDigitOnly", 0x083cf1c4;
        /// `dbexternal.c`: deletes an element's files (-1 on a refusal, 0 when it failed).
        fn db_file_delete(info: u32, list: u32, elem: u32, one: i32, strip: i32) -> i32 = "SI_dbfileDelete", 0x08125264;
        /// `dbmanager.c`: moves, copies or deletes scene files (a short result).
        #[allow(clippy::too_many_arguments)]
        fn db_m_scene_files_move_del(a1: u32, a2: u32, a3: u32, a4: u32, a5: u32, a6: u32, a7: u32, a8: u32, a9: i32, a10: i32, a11: i32, a12: i32, a13: i32, a14: i32, a15: i32, a16: u32) -> i32 = "SI_dbMSceneFilesMoveDel", 0x080de788;
    }
}
