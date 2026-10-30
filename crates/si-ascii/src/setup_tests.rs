// SPDX-License-Identifier: MIT
//! Unit tests of the setup file reader and writer. All data is made up.

use super::check::{self, Change};
use super::*;

/// A setup file with the given header version and body.
fn file(version: &str, body: &str) -> Vec<u8> {
    format!("SSET{version}\n{body}").into_bytes()
}

fn rd(body: &str) -> (Setup, ReadInfo) {
    read(&file("3.71", body)).unwrap()
}

fn words(info: &ReadInfo) -> Vec<String> {
    info.unknown_words.iter().map(|w| String::from_utf8_lossy(w).into_owned()).collect()
}

#[test]
fn tokens_end_at_space_tab_and_newline_only() {
    // a semicolon and a carriage return are ordinary characters for this reader
    let (_, info) = rd("foo;bar\tbaz\r\n qux");
    assert_eq!(words(&info), ["foo;bar", "baz\r", "qux"]);
    assert!(info.all_tokens_consumed);
    assert!(!info.ended_on_eof_word);
    // the last token runs into the end of the file and is a token all the same; then the main
    // loop asks once more
    assert_eq!(info.reads_past_end, 1);
    // a NUL cuts what the comparisons see
    let (_, info) = read(b"SSET1\nab\0cd ef\n").unwrap();
    assert_eq!(words(&info), ["ab", "ef"]);
}

#[test]
fn the_first_token_is_not_compared_with_eof() {
    let (s, info) = rd("EOF SHOW_FLAG SHW_POINTS 1 EOF SHW_LINES 1\n");
    assert_eq!(words(&info), ["EOF"]);
    assert!(info.ended_on_eof_word);
    assert_eq!(s.show.flags[0], 1);
    assert_eq!(s.show.flags[1], 0);
    assert!(!info.all_tokens_consumed);
    assert_eq!(info.reads_past_end, 0);
}

#[test]
fn a_file_without_the_eof_word_costs_two_reads_past_the_end() {
    let (s, info) = rd("SHOW_FLAG\n   SHW_TAG 1 \n   SHW_UNSEL_TAG 3 \n      ");
    assert_eq!(s.show.flags[5], 1);
    assert_eq!(s.show.menu[17], 3);
    assert!(info.all_tokens_consumed && info.unknown_words.is_empty());
    assert_eq!(info.reads_past_end, 2);
    assert_eq!(info.sections, ["SHOW_FLAG"]);
}

#[test]
fn any_header_is_accepted() {
    // another of the seven tags: read as a setup with that version
    let (s, info) = read(b"SDSC2.5\nACTIVE_MODULE 2 EOF").unwrap();
    assert_eq!((s.active_module, info.version), (2, 2.5));
    assert_eq!(info.tag, Some(u32::from_be_bytes(*b"SDSC")));
    // no tag at all: read from the first byte, version -1
    let (s, info) = read(b"ACTIVE_MODULE 3 EOF").unwrap();
    assert_eq!((s.active_module, info.version, info.tag), (3, -1.0, None));
}

#[test]
fn show_flags_under_the_standalone_renderer_skip_the_menu() {
    let mut s = Setup::default();
    let session = Session { standalone_renderer: true, ..Session::default() };
    read_into(&file("3", "SHOW_FLAG SHW_LINES 1 EOF"), &mut s, &session).unwrap();
    assert_eq!((s.show.flags[1], s.show.menu[1]), (1, 0));
    // and the layout is not read at all: its words are skipped one by one
    let info = read_into(&file("3", "WINDOW_LAYOUT WINDOW_TYPE L0_TOP ZOOM YES EOF"), &mut s, &session).unwrap();
    assert_eq!(info.unknown_words.len(), 4);
    assert_eq!(s.layout.windows[WINDOW_TOP].zoom, 0);
}

#[test]
fn preferences() {
    let mut s = Setup::default();
    s.pref.save_mat_txt_not_separately = 1;
    s.pref.save_not_rm_old_versions = 1;
    let body = "PREFERENCE PREF_ROTATION 3 PREF_MODE 7 PREF_TRANSLATION 3 PREF_CLASSICAL_SCALING 5 PREF_INVKIN_AUTO_CNS -2 \
                SAVE_NOT_RM_OLD_VERSIONS 4 EOF";
    read_into(&file("3", body), &mut s, &Session::default()).unwrap();
    assert_eq!((s.pref.rotation, s.pref.translation), (2, 4));
    assert_eq!(s.pref.mode, 4); // the translation line overrides the mode line before it
    assert_eq!((s.pref.classical_scaling, s.pref.classical_scaling_changed), (1, 1));
    assert_eq!(s.pref.invkin_auto_cns, -2);
    // the save option that was not in the section is reset
    assert_eq!((s.pref.save_mat_txt_not_separately, s.pref.save_not_rm_old_versions), (0, 4));

    // a translation other than 3 or 4 gives mode 2; a mode line after it stands
    let (s, _) = rd("PREFERENCE PREF_TRANSLATION 1 EOF");
    assert_eq!((s.pref.translation, s.pref.mode), (1, 2));
    let (s, _) = rd("PREFERENCE PREF_TRANSLATION 1 PREF_MODE 9 EOF");
    assert_eq!(s.pref.mode, 9);

    // a section the file ends in does not store the save options at all
    let mut s = Setup::default();
    s.pref.save_not_rm_old_versions = 1;
    read_into(&file("3", "PREFERENCE SAVE_MAT_TXT_NOT_SEPARATELY 1"), &mut s, &Session::default()).unwrap();
    assert_eq!((s.pref.save_mat_txt_not_separately, s.pref.save_not_rm_old_versions), (0, 1));
}

#[test]
fn active_module_is_renumbered_by_version() {
    let module = |version: &str, n: i32| read(&file(version, &format!("ACTIVE_MODULE {n} EOF"))).unwrap().0.active_module;
    assert_eq!(module("2.61", 4), 4);
    assert_eq!(module("2.613", 4), 4); // as a single, 2.613 is below the double 2.613
    assert_eq!(module("2.62", 4), 5);
    assert_eq!(module("2.62", 3), 3);
    assert_eq!(module("2.64", 4), 3); // as a single, 2.64 is above the double 2.64
    assert_eq!(module("2.65", 3), 3);
    assert_eq!(module("2.65", 6), 5);
    assert_eq!(module("2.99", 4), 3);
    assert_eq!(module("2.99908", 4), 4);
    assert_eq!(module("3.71", 5), 5);
}

#[test]
fn auto_save() {
    // ACTIVE puts the period back to a minute, so the order of the lines matters
    let (s, _) = rd("PREFERENCE_AUTO_SAVE AUTO_SAVE_PERIOD 5 AUTO_SAVE_ACTIVE ACTIVE EOF");
    assert_eq!((s.autosave.active, s.autosave.period), (1, 60));
    let (s, _) = rd("PREFERENCE_AUTO_SAVE AUTO_SAVE_ACTIVE ACTIVE AUTO_SAVE_PERIOD 5 EOF");
    assert_eq!((s.autosave.active, s.autosave.period), (1, 300));
    let (s, info) = rd("PREFERENCE_AUTO_SAVE AUTO_SAVE_ACTIVE PERHAPS AUTO_SAVE_PERIOD 0 AUTO_SAVE_PERIOD 40000 EOF");
    assert_eq!((s.autosave.active, s.autosave.period), (0, 60));
    assert_eq!(info.messages.len(), 3);
    let text = String::from_utf8_lossy(&write(&s, &WriteOptions::default())).into_owned();
    assert!(text.contains("\nPREFERENCE_AUTO_SAVE \n   AUTO_SAVE_ACTIVE INACTIVE \n   AUTO_SAVE_PERIOD 1 \n   \n"));
}

#[test]
fn mouse_values_are_cut_to_shorts() {
    let (s, _) = rd("PREFERENCE_MOUSE DIAL_MOUSE_POS 2 DIAL_MOUSE_WARP 3.9 DIAL_MOUSE_DISP -70000.5 DIAL_MOUSE_BELL 7 EOF");
    assert_eq!((s.mouse.pos, s.mouse.warp, s.mouse.bell), (2, 3, 1));
    assert_eq!(s.mouse.disp, (-70000i32) as i16);
    // without the dialog only the position is kept, and the writer leaves the section out
    let mut s = Setup::default();
    let session = Session { mouse_dialog: false, ..Session::default() };
    read_into(&file("3", "PREFERENCE_MOUSE DIAL_MOUSE_POS 2 DIAL_MOUSE_WARP 3 EOF"), &mut s, &session).unwrap();
    assert_eq!((s.mouse.pos, s.mouse.warp), (2, 0));
    let out = write_with(&s, &WriteOptions::default(), &session);
    assert!(!out.windows(16).any(|w| w == b"PREFERENCE_MOUSE"));
}

#[test]
fn a_value_the_file_ends_before_repeats_the_last_one() {
    let (s, info) = rd("PREFERENCE_INCREMENTATION PREF_SCALING_INCREMENT 1 2.5");
    assert_eq!(s.increments.scaling, [1.0, 2.5, 2.5, 2.5]);
    assert!(info.reads_past_end >= 2);
}

#[test]
fn duplicate_and_schematic_and_dopesheet() {
    let (s, _) = rd("DUPLICATE_SETUP DUP_MATERIAL_DUP 0 DUP_CONNECTION_TO -3 DUP_ALL_TEXTURE3D_ANIM 70000 \
                     SCHEMATIC SCHEM_UPDATE 0 SCHEM_SORT_TYPE 2 DOPESHEET DS_ANCHOR_KEYS 1 EOF");
    assert_eq!((s.duplicate.values[0], s.duplicate.values[23]), (0, -3));
    assert_eq!(s.duplicate.values[29], 70000i32 as i16 as i32);
    assert_eq!((s.schematic.update, s.schematic.update_wanted, s.schematic.sort_type), (0, 0, 2));
    assert_eq!(s.dopesheet.anchor_keys, 1);
    // while the database loader holds the schematic, the flag in force is left alone
    let mut s = Setup::default();
    s.schematic.loading = 1;
    read_into(&file("3", "SCHEMATIC SCHEM_UPDATE 0 EOF"), &mut s, &Session::default()).unwrap();
    assert_eq!((s.schematic.update, s.schematic.update_wanted), (1, 0));
}

#[test]
fn picture_format_line() {
    let (s, info) = rd("PREFERENCE_PICT_FMT 6 1.5 2 3 EOF");
    let cam = s.layout.scene_camera();
    assert_eq!((cam.format, cam.format_aspect, cam.format_width, cam.format_height), (6, 1.5, 2.0, 3.0));
    assert_eq!(cam.aspect, 1.3333);
    assert!(info.deviations.is_empty());
    let (s, info) = rd("PREFERENCE_PICT_FMT 99 1 1 1 EOF");
    assert_eq!(s.layout.scene_camera().format, 99);
    assert_eq!(info.deviations.len(), 1);
}

#[test]
fn quoted_strings() {
    let body = "RENDERING_TYPE WIRE_FRAME OUTPUT_FILE 'a b\nc' PRE_RENDERING '' POST_RENDERING xyz' DITHERING 0 0 1 EOF";
    let (s, info) = rd(body);
    assert_eq!(s.render.output_file.as_deref(), Some(&b"a b\nc"[..]));
    assert_eq!(s.render.pre_rendering.as_deref(), Some(&b""[..])); // an empty string leaves what was there
    assert_eq!(s.render.post_rendering.as_deref(), Some(&b"yz"[..])); // the first byte is taken for the quote
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word);

    // a string longer than the buffer keeps its first 237 characters
    let long: String = (0..300).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let (s, _) = rd(&format!("RENDERING_TYPE WIRE_FRAME OUTPUT_FILE '{long}' DITHERING 0 0 1 EOF"));
    assert_eq!(s.render.output_file.as_deref(), Some(&long.as_bytes()[..237]));

    // no closing quote: the string runs to the end of the file
    let (s, _) = rd("RENDERING_TYPE WIRE_FRAME OUTPUT_FILE 'open");
    assert_eq!(s.render.output_file.as_deref(), Some(&b"open"[..]));
}

#[test]
fn layout_window_and_view() {
    let body = "WINDOW_LAYOUT WINDOW_TYPE L0_TOP CAMERA_TYPE RIGHT CAMERA_POSITION 1 2 3 CAMERA_FOV 40 CAMERA_NEAR 0.5 \
                DISPLAY_TYPE 2 ZOOM YES WINDOW_SIZE TALL_SIZE FAST_PLAY YES X_AXIS_LOCK YES Z_AXIS_LOCK YES RULER_VISIBLE YES \
                DELTA 4 5 6 MAGNET 3 CONTROLER ACTIVE CONTROLER_STEP 2.5 DC_DEPTHCUE 1 10 20 3 4 5 DC_RANGE 2 EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word);
    let w = &s.layout.windows[WINDOW_TOP];
    assert_eq!((w.current, w.previous), (SLOT_RIGHT, SLOT_TOP as i16));
    let slot = &w.slots[SLOT_RIGHT];
    assert_eq!(slot.camera.position, [1.0, 2.0, 3.0]);
    assert_eq!((slot.camera.fov, slot.camera.near, w.near), (40.0, 0.5, 0.5));
    assert_eq!(w.display_type, [2, 2]);
    assert_eq!(slot.display_type, [0, 1]); // the slot's own copy is not updated
    assert_eq!((w.zoom, w.size, slot.size), (1, 2, 2));
    assert_eq!(w.fast_play, 0); // read as a number: YES is 0
    assert_eq!(slot.view.axis_lock, [1, 0, 1]);
    assert_eq!((slot.view.ruler_visible, slot.rulers_active), (1, [1, 1]));
    assert_eq!((slot.view.delta, slot.view.magnet), ([4.0, 5.0, 6.0], 3));
    assert_eq!((slot.controller.active, slot.controller.step), (1, 2.5));
    assert_eq!(w.depth_cue, DepthCue { active: 1, range: 2, near: 10, far: 20, colour: [3, 4, 5] });
    // the view the window showed before keeps its own records
    assert_eq!(w.slots[SLOT_TOP].camera.fov, 0.0);
    // the other windows are untouched
    assert_eq!(s.layout.windows[WINDOW_FRONT], Setup::default().layout.windows[WINDOW_FRONT]);
}

#[test]
fn layout_window_type_resets_and_unknown_names() {
    // WINDOW_TYPE clears the axis locks and the display type of the view shown at that moment
    let mut s = Setup::default();
    s.layout.windows[WINDOW_FRONT].slots[SLOT_FRONT].view.axis_lock = [1, 1, 1];
    s.layout.windows[WINDOW_FRONT].display_type = [5, 5];
    // an unknown window name leaves the window chosen earlier in the block selected
    let body = "WINDOW_LAYOUT WINDOW_TYPE L0_FRONT ZOOM YES WINDOW_TYPE L7_NOWHERE GRID_VISIBLE YES EOF";
    read_into(&file("3", body), &mut s, &Session::default()).unwrap();
    let w = &s.layout.windows[WINDOW_FRONT];
    assert_eq!((w.zoom, w.slots[SLOT_FRONT].view.grid_visible), (1, 1));
    assert_eq!((w.slots[SLOT_FRONT].view.axis_lock, w.display_type, w.slots[SLOT_FRONT].display_type), ([0, 0, 0], [0, 0], [0, 0]));
    // but each block starts with no window: here the grid line has nowhere to go
    let body = "WINDOW_LAYOUT WINDOW_TYPE L0_TOP ZOOM YES WINDOW_LAYOUT WINDOW_TYPE L7_NOWHERE GRID_VISIBLE YES EOF";
    let (s, info) = rd(body);
    let w = &s.layout.windows[WINDOW_TOP];
    assert_eq!((w.zoom, w.slots[SLOT_TOP].view.grid_visible), (1, 0));
    assert_eq!(info.sections, ["WINDOW_LAYOUT", "WINDOW_LAYOUT"]);
}

#[test]
fn layout_schematic_views() {
    let (s, _) = rd("WINDOW_LAYOUT WINDOW_TYPE L0_RIGHT CAMERA_TYPE SCHEMATIC_MOTION EOF");
    let w = &s.layout.windows[WINDOW_RIGHT];
    assert_eq!((w.current, w.camera().schematic_modes, w.schematic_menu), (SLOT_SCHEMATIC, 4, [0, 0, 1, 0]));
    let (s, _) = rd("WINDOW_LAYOUT WINDOW_TYPE L0_RIGHT CAMERA_TYPE SCHEMATIC SCHEMATIC_MODES LIGHT MATTER junk PLAIN ZOOM YES EOF");
    let w = &s.layout.windows[WINDOW_RIGHT];
    assert_eq!((w.camera().schematic_modes, w.schematic_menu, w.zoom), (9, [1, 0, 0, 1], 1));
    // written the new way
    let text = String::from_utf8_lossy(&write(&s, &WriteOptions::default())).into_owned();
    assert!(text.contains("\n   CAMERA_TYPE SCHEMATIC \n   SCHEMATIC_MODES LIGHT MATTER PLAIN \n      CAMERA_POSITION "));
    // the list of modes waits for PLAIN and does not notice the end of the file
    let err = read(&file("3", "WINDOW_LAYOUT WINDOW_TYPE L0_RIGHT CAMERA_TYPE SCHEMATIC SCHEMATIC_MODES LIGHT MATTER")).unwrap_err();
    assert_eq!(err, SetupError::SpinsForever { at: "SCHEMATIC_MODES" });
    // a spreadsheet becomes the schematic under the light licence
    let mut s = Setup::default();
    let session = Session { light_license: true, ..Session::default() };
    read_into(&file("3", "WINDOW_LAYOUT WINDOW_TYPE L0_TOP CAMERA_TYPE SPREADSHEET EOF"), &mut s, &session).unwrap();
    assert_eq!(s.layout.windows[WINDOW_TOP].current, SLOT_SCHEMATIC);
    let (s, _) = rd("WINDOW_LAYOUT WINDOW_TYPE L0_TOP CAMERA_TYPE SPREADSHEET SPREADSHEET_SETUP sheet1 EOF");
    assert_eq!(s.layout.windows[WINDOW_TOP].current, SLOT_SPREADSHEET);
    assert_eq!(s.layout.windows[WINDOW_TOP].spreadsheet_wanted.as_deref(), Some(&b"sheet1"[..]));
}

#[test]
fn layout_blocks_of_windows_the_program_does_not_have() {
    // every keyword consumes its values and stores nothing
    let body = "WINDOW_LAYOUT WINDOW_TYPE L1_FRONT CAMERA_TYPE FCURVE CAMERA_POSITION 1 2 3 CAMERA_FOV 9 ZOOM YES \
                DC_DEPTHCUE 0 1.5 2.5 0 0 0 DC_RANGE 2 DO_SEQUENCE YES 1 2 3 4 DO_DRAW_OVER 'x' PICTURE_FRAME 1 2 3 4 5 6 \
                DEPTH_CUE 1 2 3 4 5 6 EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word && info.all_tokens_consumed);
    assert_eq!(s.layout, Setup::default().layout);
    // four keywords do not check for the window
    for (body, keyword) in [
        ("WINDOW_LAYOUT WINDOW_TYPE L1_TOP CONTROLER ACTIVE EOF", "CONTROLER"),
        ("WINDOW_LAYOUT CONTROLER_STEP 1 EOF", "CONTROLER_STEP"),
        ("WINDOW_LAYOUT WINDOW_TYPE L1_TOP TEXTURE YES EOF", "TEXTURE"),
        ("WINDOW_LAYOUT WINDOW_TYPE L1_TOP SPD_UPDATE_SCENE 1 EOF", "SPD_UPDATE_SCENE"),
        ("WINDOW_LAYOUT WINDOW_TYPE L1_TOP MEMO_CAMERA MC_TOP 1 0 ACTIVE EOF", "MEMO_CAMERA"),
    ] {
        assert_eq!(read(&file("3", body)).unwrap_err(), SetupError::NullWindow { keyword }, "{body}");
    }
}

#[test]
fn layout_older_keywords() {
    let body = "WINDOW_LAYOUT WINDOW_TYPE L0_FRONT PICTURE 'pic' PICTURE_FRAME 1 2 3 4 5 6 DEPTH_CUE 7.9 -8.9 1 2 3 4 \
                MINIMUM_VIEW 1 2 3 MAXIMUM_VIEW 4 5 6 EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty());
    let w = &s.layout.windows[WINDOW_FRONT];
    assert_eq!(w.draw_over.name.as_deref(), Some(&b"pic"[..]));
    assert_eq!((w.draw_over.sequence, w.draw_over.frames), (1, [2, 3, 5, 4, 6]));
    assert_eq!(w.depth_cue, DepthCue { active: 1, range: 0, near: 7, far: -8, colour: [2, 3, 4] });
    assert_eq!(w.camera().view_box, Some(([1.0, 2.0, 3.0], [4.0, 5.0, 6.0])));
}

#[test]
fn layout_draw_over() {
    let body = "WINDOW_LAYOUT WINDOW_TYPE L0_FRONT DO_DRAW_OVER 'pic' DO_SEQUENCE YES 1 2 3 4 DO_MOVIENAME '' DO_USEMOVIE YES DO_OPTION 3 EOF";
    let (s, _) = rd(body);
    let d = &s.layout.windows[WINDOW_FRONT].draw_over;
    assert_eq!((d.name.as_deref(), d.movie_name.as_deref()), (Some(&b"pic"[..]), None));
    assert_eq!((d.sequence, d.frames, d.use_movie, d.option), (1, [1, 2, 3, 0, 4], 1, 3));
    // the missing movie name is written as the C library prints a null pointer
    let out = write(&s, &WriteOptions::default());
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(text.contains("\n   DO_DRAW_OVER 'pic' \n      DO_SEQUENCE YES \n      1 2 3 4 \n      DO_MOVIENAME '(null)' \n      DO_USEMOVIE YES \n      DO_OPTION 3 "));
    // and comes back as that word
    let (again, _) = read(&out).unwrap();
    assert_eq!(again.layout.windows[WINDOW_FRONT].draw_over.movie_name.as_deref(), Some(&b"(null)"[..]));
    // with neither name the block is not written
    let (s, _) = rd("WINDOW_LAYOUT WINDOW_TYPE L0_FRONT DO_DRAW_OVER '' DO_SEQUENCE YES 1 2 3 4 EOF");
    assert!(!String::from_utf8_lossy(&write(&s, &WriteOptions::default())).contains("DO_DRAW_OVER"));
}

#[test]
fn layout_memo_cameras() {
    // the older form, without MC_CAM_CLIP: the token read in its place is the next camera's
    // number, and after the last camera the word that follows
    let old = "WINDOW_LAYOUT WINDOW_TYPE L0_FRONT MEMO_CAMERA MC_TOP 2 \
               0 ACTIVE MC_CAM_POSITION 1 2 3 MC_CAM_INTEREST 4 5 6 MC_CAM_ROLL 7 MC_CAM_FOV 8 MC_CAM_ANGLE 9 \
               1 INACTIVE MC_CAM_POSITION 0 0 1 MC_CAM_INTEREST 0 0 2 MC_CAM_ROLL 3 MC_CAM_FOV 4 MC_CAM_ANGLE 5 \
               MC_PERS 1 0 ACTIVE MC_CAM_POSITION 9 9 9 MC_CAM_INTEREST 8 8 8 MC_CAM_ROLL 7 MC_CAM_FOV 6 MC_CAM_ANGLE 5 \
               ACTIVE_MODULE 2 EOF";
    let (s, info) = rd(old);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word, "{info:?}");
    assert_eq!(s.active_module, 2);
    let w = &s.layout.windows[WINDOW_FRONT];
    let m = &w.slots[SLOT_TOP].memo;
    assert_eq!((m[0].active, m[0].position, m[0].interest, m[0].roll, m[0].fov, m[0].angle), (1, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 7.0, 8.0, 9.0));
    assert_eq!((m[1].active, m[1].position, m[1].angle), (0, [0.0, 0.0, 1.0], 5.0));
    assert_eq!(w.slots[SLOT_PERSPECTIVE].memo[0].fov, 6.0);
    // the newer form
    let new = "WINDOW_LAYOUT WINDOW_TYPE L0_FRONT MEMO_CAMERA MC_DOPE 1 \
               0 ACTIVE MC_CAM_POSITION 1 2 3 MC_CAM_INTEREST 4 5 6 MC_CAM_ROLL 7 MC_CAM_FOV 8 MC_CAM_ANGLE 9 MC_CAM_CLIP -1 1 \
               ACTIVE_MODULE 2 EOF";
    let (s, info) = rd(new);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word);
    assert_eq!(s.layout.windows[WINDOW_FRONT].slots[SLOT_DOPESHEET].memo[0].clip, [-1.0, 1.0]);
    assert_eq!(s.active_module, 2);
    // written: every view with an active memo camera, four cameras each
    let text = String::from_utf8_lossy(&write(&s, &WriteOptions::default())).into_owned();
    assert!(text.contains("\n   MEMO_CAMERA \n      MC_DOPE \n      4 \n      0 \n      ACTIVE \n      MC_CAM_POSITION 1 2 3 "));
    assert!(text.contains("\n      3 \n      INACTIVE \n      MC_CAM_POSITION 0 0 0 "));
    assert_eq!(text.matches("MEMO_CAMERA").count(), 1);
    // a fifth camera has nowhere to go
    let five = "WINDOW_LAYOUT WINDOW_TYPE L0_FRONT MEMO_CAMERA MC_TOP 5 0 A x x x x x x 1 A x x x x x x 2 A x x x x x x 3 A x x x x x x 4 ACTIVE x x x x x x EOF";
    let (_, info) = rd(five);
    assert_eq!(info.deviations, ["more than four memo cameras for a view"]);
}

#[test]
fn shade_setup_lines() {
    let body = "WINDOW_LAYOUT WINDOW_TYPE L0_TOP TEXTURE YES AFFECTED_MODEL ALL SHOW_ICON YES OPTIMISATION SHADE HIGHLIGHT_WIDTH 2.5 \
                PSEUDO_WIRE_MODE 3 GHOST_FOLLOW_SELECTION YES ANTIALIAS YES ZOOM YES EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty());
    let w = &s.layout.windows[WINDOW_TOP];
    let p = &w.slots[SLOT_TOP].shade;
    assert_eq!((p.texture, p.affected_model, p.show_icon, p.optimisation, p.highlight_width), (1, 0, 1, 1, 2.5));
    assert_eq!((p.pseudo_wire_mode, p.ghost_follow_selection, p.antialias, w.zoom), (3, 1, 1, 1));
}

#[test]
fn shade_setup_glb() {
    let body = "SHADE_SETUP_GLB BLENDING ALPHA REPEAT GRID LOD 3 TXT_DPY_GLOBAL 70000 TXT_DPY_PREC 4 OPTIMISE_EDITING 2 \
                TEXTURE_PXL_INTERPOLATION_MODE ALWAYS_OFF EOF";
    let (s, _) = rd(body);
    let g = &s.shade_glb;
    assert_eq!((g.blending, g.repeat, g.lod, g.txt_dpy_global, g.txt_dpy_prec, g.optimise_editing, g.texture_pxl_interpolation_mode), (1, 1, 3, 70000i32 as i16, 4, 2, 2));
    let mut s = Setup::default();
    let session = Session { shade_optimize_editing_env: true, ..Session::default() };
    read_into(&file("3", "SHADE_SETUP_GLB OPTIMISE_EDITING 0 EOF"), &mut s, &session).unwrap();
    assert_eq!(s.shade_glb.optimise_editing, 1);
}

#[test]
fn render_section_ends_at_dithering_or_at_an_unknown_word() {
    let (s, info) = rd("RENDERING_TYPE HIDDEN_LINE RENDERING_FRAME 3 9 2 ANTIALIAS FIXED 5 6 0.5 FILTER 7 DITHERING 1 2 0.5 GLOBAL_SWITCHES GLG_SHADOW NO MR_LENS NO EOF");
    assert_eq!((s.render.rendering_type, s.render.frames), (1, [3, 9, 2]));
    assert_eq!((s.render.antialias, s.render.filter, s.render.antialias_level, s.render.antialias_threshold), (0, 7, 6, 0.5));
    assert_eq!((s.render.dithering, s.render.dithering_value), ([1, 2], 0.5));
    assert_eq!((s.render.switches[5], s.render.switches[9], s.render.switches[0]), (0, 0, 1));
    assert_eq!(s.render.has_picture, 0);
    assert!(info.unknown_words.is_empty());

    // an unknown word ends the section; what follows it is skipped word by word
    let (s, info) = rd("RENDERING_TYPE WIRE_FRAME BOGUS 1 FILTER 9 GLOBAL_SWITCHES GLG_SHADOW NO EOF");
    assert_eq!((s.render.rendering_type, s.render.filter, s.render.switches[5]), (0, 2, 0));
    assert_eq!(words(&info), ["BOGUS", "1", "FILTER", "9"]);

    // SUBREGION is the line renderer with the subregion on
    let (s, info) = rd("RENDERING_TYPE SUBREGION DRAWSUB INACTIVE RENDERING_TYPE NONSENSE EOF");
    assert_eq!((s.render.rendering_type, s.render.subregion, s.render.draw_subregion), (4, 1, 0));
    assert_eq!(info.messages, [Message { keyword: "RENDERING_TYPE", token: b"NONSENSE".to_vec() }]);
}

#[test]
fn mental_ray_needs_its_renderer() {
    let (s, _) = rd("RENDERING_TYPE MENTAL_RAY EOF");
    assert_eq!((s.render.rendering_type, s.preview.renderer, s.mi2_output), (10, 3, 1));
    let mut s = Setup::default();
    let session = Session { mental_ray_19: true, ..Session::default() };
    read_into(&file("3", "RENDERING_TYPE MENTAL_RAY PREVIEW 1 1 PRW_GLOBALS PREVIEW_TYPE_MR_2X EOF"), &mut s, &session).unwrap();
    // the preview's choice pulls the rendering type along
    assert_eq!((s.render.rendering_type, s.preview.renderer, s.mi2_output), (10, 3, 1));
    read_into(&file("3", "RENDERING_TYPE MENTAL_RAY_2X PREVIEW 1 1 PRW_GLOBALS PREVIEW_TYPE_MR EOF"), &mut s, &session).unwrap();
    assert_eq!((s.render.rendering_type, s.preview.renderer, s.mi2_output), (9, 2, 0));
}

#[test]
fn render_picture_block() {
    let (s, info) = rd("RENDERING_TYPE WIRE_FRAME PICTURE FORMAT_TYPE PAL ASPECT 1.5 WIDTH 4 HEIGHT 2 PIXEL_RATIO 2 DITHERING 0 0 1 EOF");
    let cam = s.layout.scene_camera();
    assert_eq!((cam.format, cam.format_aspect, cam.format_width, cam.format_height), (7, 1.5, 4.0, 2.0));
    assert_eq!((s.format_aspect[13], s.format_aspect[14]), (1.5, 2.0));
    assert_eq!(cam.aspect, 1.3333); // that of the format, whatever the layout said
    assert_eq!((s.render.has_picture, s.render.pixel_ratio), (1, 2.0));
    assert!(info.unknown_words.is_empty());
    // a width read while the height is still zero does not touch the ratio
    let (s, _) = rd("RENDERING_TYPE WIRE_FRAME PICTURE FORMAT_TYPE CUSTOM_SIZE WIDTH 4 DITHERING 0 0 1 EOF");
    assert_eq!((s.format_aspect[14], s.layout.scene_camera().aspect), (1.0, 1.0));
    // PICTURE followed by something else swallows that word
    let (s, info) = rd("RENDERING_TYPE WIRE_FRAME PICTURE PIXEL_RATIO 2 FILTER 9 GLOBAL_SWITCHES EOF");
    assert_eq!((s.render.pixel_ratio, s.render.filter), (1.0, 2));
    assert_eq!(words(&info), ["2", "FILTER", "9"]);
    // an unknown format name is reported and the format stays
    let (s, info) = rd("RENDERING_TYPE WIRE_FRAME PICTURE FORMAT_TYPE IMAX_3D DITHERING 0 0 1 EOF");
    assert_eq!(s.layout.scene_camera().format, 0);
    assert_eq!(info.messages.len(), 1);
}

#[test]
fn a_function_curve_in_place_of_a_value_is_read_and_lost() {
    let body = "RENDERING_TYPE WIRE_FRAME FADE_START CURVE_TYPE whatever INTERPOLATION LINEAR POST_EXTRAPOLATION 2 KEYS 2 \
                TIME 1 VALUE 2 LEFT_SLOPE 0.5 FLAG 0 TIME 3 VALUE 4 INTERPOLATION SPLINE FLAG 1 \
                FADE_END 5 AMBIENCE 0.1 0.2 0.3 DITHERING 0 0 1 EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word, "{info:?}");
    assert_eq!(s.render.fade_start, 20.0); // untouched
    assert_eq!((s.render.fade_end, s.render.ambience), (5.0, [0.1, 0.2, 0.3]));
    assert_eq!(info.lost_fcurves.len(), 1);
    let c = &info.lost_fcurves[0];
    assert_eq!((c.interpolation, c.post_extrapolation, c.keys.len()), (1, 2, 2));
    assert_eq!((c.keys[0].time, c.keys[0].value, c.keys[0].left_slope, c.keys[0].flag), (1.0, 2.0, 0.5, 0));
    assert_eq!((c.keys[1].time, c.keys[1].interpolation, c.keys[1].flag, c.keys[1].spline_index), (3.0, 2, 1, -1));
    // a new curve starts with the preferred interpolation; three curves for a colour
    let body = "PREFERENCE PREF_FCV_INTERP_TYPE 0 RENDERING_TYPE WIRE_FRAME FOG_COLOUR CURVE_TYPE a KEYS 0 CURVE_TYPE b KEYS 0 CURVE_TYPE c KEYS 1 VALUE 1 FLAG 0 DITHERING 0 0 1 EOF";
    let (s, info) = rd(body);
    assert_eq!(info.lost_fcurves.len(), 3);
    assert_eq!(info.lost_fcurves[0].interpolation, 0);
    assert_eq!(s.render.fog_colour, [0.7, 0.7, 0.7]);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word);
    // a key count the file cannot hold
    let err = read(&file("3", "RENDERING_TYPE WIRE_FRAME FADE_END CURVE_TYPE a KEYS 2000000000 TIME 1")).unwrap_err();
    assert_eq!(err, SetupError::CountTooLarge { keyword: "KEYS", count: 2000000000 });
}

#[test]
fn mental_ray_lines() {
    let body = "RENDERING_TYPE WIRE_FRAME MR_FILENAME '' MR_TIME_CONTRAST 0.5 2 -1 1 MR_FACE MR_BACK MR_FILE_OUTPUT WHATEVER \
                MR_LINK_STRING 'one' MR_LINK_STRING 'two' MR_PH_MAP_FILE '' MR_CS_ACC 123456789 MR_USE_FG_RAD_MAX 1 MR_FG_RAD_MAX 2.5 \
                MR_THRESHOLD 0.1 0.2 0.3 MR_BLUR_TYPE MR_BLUR_MVONLY DITHERING 0 0 1 EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty() && info.messages.is_empty());
    let m = &s.render.mental_ray;
    assert_eq!(m.filename.as_deref(), Some(&b"out.mi"[..]));
    assert_eq!(m.time_contrast, [0.5, 0.1, 0.1, 1.0]);
    assert_eq!((m.face, m.file_output, m.blur_type), (2, 0, 2));
    assert_eq!(m.link_string.as_deref(), Some(&b"one\ntwo"[..]));
    assert_eq!(m.ph_map_file, None);
    assert_eq!((m.photon_ints[0], m.photon_flags[9], m.photon_reals[4], m.threshold), (123456789, 1, 2.5, [0.1, 0.2, 0.3]));
    let text = String::from_utf8_lossy(&write(&s, &WriteOptions::default())).into_owned();
    assert!(text.contains("\n   MR_LINK_STRING 'one' \n   MR_LINK_STRING 'two' "));
    assert!(text.contains("MR_PH_MAP_FILE '(null)' "));
    // the int goes out through %.7g and comes back as 1
    assert!(text.contains("MR_CS_ACC 1.234568e+08 "));
    let (again, _) = read(text.as_bytes()).unwrap();
    assert_eq!(again.render.mental_ray.photon_ints[0], 1);
    // a render section starts with no link strings
    let mut t = s.clone();
    read_into(&file("3", "RENDERING_TYPE WIRE_FRAME EOF"), &mut t, &Session::default()).unwrap();
    assert_eq!(t.render.mental_ray.link_string, None);
}

#[test]
fn mental_ray_shaders() {
    let body = "RENDERING_TYPE WIRE_FRAME MR_ATMOS_SHADER UDF_NAME 'fog' UDF_ACTIVE ACTIVE MR_OUTPUT_NUM 2 \
                MR_RENDER_SHADER UDF_NAME 'b' UDF_INTERNAL INTERNAL UDF_ORDER 2 MR_RENDER_SHADER UDF_NAME 'a' UDF_ORDER 1 \
                MR_LENS_NUM 1 MR_LENS_SHADER UDF_NAME 'l' MR_LENS_SHADER UDF_NAME 'm' UDF_ORDER 3 DITHERING 0 0 1 EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word, "{info:?}");
    let m = &s.render.mental_ray;
    assert_eq!(m.atmos_shader, Some(Udf { name: Some(b"fog".to_vec()), active: 1, ..Udf::default() }));
    assert_eq!(m.output_shaders[0].as_ref().unwrap().name.as_deref(), Some(&b"a"[..]));
    assert_eq!(m.output_shaders[1], Some(Udf { name: Some(b"b".to_vec()), internal: 1, order: 2, ..Udf::default() }));
    assert_eq!(m.lens_shaders[0].as_ref().unwrap().name.as_deref(), Some(&b"l"[..])); // no order: the first place
    assert_eq!(info.deviations, ["shader order outside the shader array"]);
    // the order is written only for shaders of kinds 6 and 10
    let mut t = s.clone();
    t.render.mental_ray.output_shaders[1].as_mut().unwrap().kind = 10;
    let text = String::from_utf8_lossy(&write(&t, &WriteOptions::default())).into_owned();
    assert_eq!(text.matches("UDF_ORDER").count(), 1);
    assert!(text.contains("MR_RENDER_SHADER \n   UDF_NAME 'b' \n   UDF_ACTIVE INACTIVE \n   UDF_INTERNAL INTERNAL \n   UDF_ORDER 2 "));
    let err = read(&file("3", "RENDERING_TYPE WIRE_FRAME MR_OUTPUT_NUM 2000000000 EOF")).unwrap_err();
    assert_eq!(err, SetupError::CountTooLarge { keyword: "MR_OUTPUT_NUM", count: 2000000000 });
}

#[test]
fn preview_section() {
    let body = "PREVIEW 10 20 PRW_GLOBALS PRW_REFLECT NO SAMPLE_SIZE 9 MAX_FILTER 0 MIN_SAMPLES -7 MAX_SAMPLES 5 \
                DIV_THRESHOLD 2 0.5 -1 7 ADAPTIVE PERHAPS WIDTH -1 XY_PIXEL_RATIO 1.5 FILTER_TYPE 8 PRW_VOLUME NO EOF";
    let (s, info) = rd(body);
    let p = &s.preview;
    assert_eq!((p.size, p.switches), ([10, 20], [1, 1, 1, 0, 1, 1]));
    assert_eq!((p.sample_size, p.max_filter), (2, 2));
    assert_eq!((p.min_samples, p.max_samples), (3, 5)); // -7 becomes -2; then 5 pulls the minimum up
    assert_eq!(p.div_threshold, [0.2, 0.5, 0.2, 0.05]);
    assert_eq!((p.adaptive, p.width, p.xy_pixel_ratio, p.filter_type, p.volume), (1, 2.0, 1.5, 8, 0));
    assert_eq!(info.messages.len(), 5);
    assert!(info.ended_on_eof_word && info.unknown_words.is_empty());
    // without PRW_GLOBALS the section is just the two numbers
    let (s, info) = rd("PREVIEW 1 2 EOF");
    assert_eq!(s.preview.size, [1, 2]);
    assert!(info.ended_on_eof_word);
    // a minimum above the maximum pulls the maximum
    let (s, _) = rd("PREVIEW 1 2 PRW_GLOBALS MIN_SAMPLES 4 EOF");
    assert_eq!((s.preview.min_samples, s.preview.max_samples), (4, 6));
}

#[test]
fn audio_section() {
    let body = "AUDIOCONTAINER VOLUME 9 AUDIOTRACK 'voice' '/a b/voice.aiff' ACTIVE STARTOFFSET 1.5 3 PREPOSTROLL 4 5 \
                WAVEDISPLAYQUALITY MEDIUM 7 PBBUFFERING LOW 8 TRACKHEIGHT TINY DISPLAYMARKERFCV NONE VOLUME 0.5 \
                AUDIOMARKER 'm 1' 0.25 0.75 ACTIVE AUDIOTRACK 'two' 'x' INACTIVE EOF";
    let (s, info) = rd(body);
    assert!(info.unknown_words.is_empty() && info.ended_on_eof_word);
    assert_eq!(s.audio.tracks.len(), 2);
    let t = &s.audio.tracks[0];
    assert_eq!((t.name.as_deref(), t.path.as_deref(), t.active), (Some(&b"voice"[..]), Some(&b"/a b/voice.aiff"[..]), true));
    assert_eq!((t.start_offset, t.start_offset_frame, t.pre_roll, t.post_roll), (1.5, 3, 4, 5));
    assert_eq!((t.wave_display_quality, t.wave_display_value, t.pb_buffering, t.pb_buffering_value), (Level::Medium, 7, Level::Low, 8));
    assert_eq!((t.track_height, t.display_marker_fcv, t.volume), (Level::Unset, Level::Low, 0.5));
    assert_eq!(t.markers, [AudioMarker { name: Some(b"m 1".to_vec()), start: 0.25, end: 0.75, active: true }]);
    let out = write(&s, &WriteOptions::default());
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(text.contains("\n\nAUDIOCONTAINER \n   AUDIOTRACK 'voice' '/a b/voice.aiff' ACTIVE \n      STARTOFFSET 1.5 3 \n      PREPOSTROLL 4 5 \n      WAVEDISPLAYQUALITY MEDIUM 7 \n      PBBUFFERING LOW 8 \n      TRACKHEIGHT HIGH \n      DISPLAYMARKERFCV NONE \n      VOLUME 0.5 \n      AUDIOMARKER 'm 1' 0.25 0.75 ACTIVE "));
    let (again, _) = read(&out).unwrap();
    assert_eq!(again.audio.tracks[0].markers, t.markers);
    assert_eq!(again.audio.tracks[1].active, false);
    // no tracks, no section
    assert!(!String::from_utf8_lossy(&write(&Setup::default(), &WriteOptions::default())).contains("AUDIO"));
}

#[test]
fn writer_layout_and_indent() {
    let s = Setup::default();
    let first = write(&s, &WriteOptions::default());
    let later = write(&s, &WriteOptions { indent: 1, ..WriteOptions::default() });
    assert!(first.starts_with(b"SSET4\nSHOW_FLAG \n\n   SHW_POINTS 0 \n   SHW_LINES 0 "));
    assert!(later.starts_with(b"SSET4\nSHOW_FLAG \n   \n      SHW_POINTS 0 \n      SHW_LINES 0 "));
    // the picture-format line takes the level down to zero whatever it was: from there on the
    // two files are the same. The preview raises the level by one for good, so the dope sheet
    // is one level in, and so is everything in the next file written.
    let from = |out: &[u8], word: &[u8]| out.windows(word.len()).position(|w| w == word).unwrap();
    assert_eq!(&first[from(&first, b"PREFERENCE_PICT_FMT")..], &later[from(&later, b"PREFERENCE_PICT_FMT")..]);
    assert!(first.ends_with(b"\nDOPESHEET \n      DS_RIPPLING 0 \n      DS_DISPLAY_ACTIONS 0 \n      DS_ANCHOR_KEYS 0 \n      DS_AFFECT_KEYS 0 \n      \nEOF "));
    let text = String::from_utf8_lossy(&first).into_owned();
    assert!(text.contains("\nPREFERENCE_PICT_FMT 0 0 0 0 \n\nSHADE_SETUP_GLB \n   BLENDING NONE \n   REPEAT NONE "));
    assert!(text.contains("\n\nACTIVE_MODULE 0 \nWINDOW_LAYOUT \n   WINDOW_TYPE L0_FRONT \n   CAMERA_TYPE FRONT \n      CAMERA_POSITION \n         0 0 0 \n      CAMERA_INTEREST \n         0 0 0 \n      CAMERA_FOV 0 "));
    // the scene camera's position is not written
    assert!(text.contains("\n   WINDOW_TYPE L0_PERSPECTIVE \n   CAMERA_TYPE PERSPECTIVE \n      CAMERA_FOV 0 "));
    assert!(text.contains("\nRENDERING_TYPE RENDERING_LINE \n   RENDERING_FRAME 1 100 1 \n   DRAW_SUBR_ON_RENDER YES \n   FRAME_BUFFER ACTIVE \n   OUTPUT_FILE 'default' \n   RESOLUTION 500 424 \n   PICTURE \n      FORMAT_TYPE 35MM_137 \n   PIXEL_RATIO 1 "));
    assert!(text.contains("\n   MOTION_BLUR 0 SUBFRAME INACTIVE DRAWSUB ACTIVE \n"));
    assert!(text.contains("\n   DITHERING 0 0 10 \n\nGLOBAL_SWITCHES \n   GLG_MATERIAL YES "));
    assert_eq!(text.matches("BACK_CULLING ACTIVE").count(), 2);
    // under the stand-alone renderer neither show flags nor windows, unless the save is an emergency one
    let alone = Session { standalone_renderer: true, ..Session::default() };
    let text = String::from_utf8_lossy(&write_with(&s, &WriteOptions::default(), &alone)).into_owned();
    assert!(text.starts_with("SSET4\nPREFERENCE \n   PREF_BUSY_CURSOR 1 ") && !text.contains("WINDOW_LAYOUT") && !text.contains("ACTIVE_MODULE"));
    let emergency = Session { standalone_renderer: true, emergency_saving: true, ..Session::default() };
    assert_eq!(write_with(&s, &WriteOptions::default(), &emergency), first);
}

#[test]
fn what_is_written_reads_back_and_settles_after_two_generations() {
    // a state with every section touched
    let mut s = Setup::default();
    s.show.menu = [1; 18];
    s.show.flags = [1; 18];
    s.pref.rotation = 1;
    s.increments.rotation = [0.5, 0.25, 1e-7, 123456.7];
    s.duplicate.values[23] = 2;
    s.active_module = 3;
    s.layout.windows[WINDOW_TOP].set_current(SLOT_SCHEMATIC);
    s.layout.windows[WINDOW_TOP].slots[SLOT_SCHEMATIC].camera.schematic_modes = 6;
    s.layout.windows[WINDOW_TOP].schematic_menu = [0, 1, 1, 0];
    s.layout.windows[WINDOW_TOP].schematic_menu_last = 2;
    s.layout.windows[WINDOW_RIGHT].slots[SLOT_FCURVE].memo[2].active = 1;
    s.layout.windows[WINDOW_RIGHT].slots[SLOT_FCURVE].memo[2].clip = [-3.5, 8.0];
    s.layout.windows[WINDOW_FRONT].draw_over.name = Some(b"over".to_vec());
    s.layout.windows[WINDOW_FRONT].draw_over.movie_name = Some(b"movie".to_vec());
    s.render.subregion = 1;
    s.render.subregion_frame = [0.1, 0.2, 0.3, 0.4];
    s.render.mental_ray.ph_map_file = Some(b"map".to_vec());
    s.preview.renderer = 3;
    s.mi2_output = 1;
    s.render.rendering_type = 10;
    s.dopesheet.rippling = 1;
    let out = write(&s, &WriteOptions::default());
    let (back, info) = read(&out).unwrap();
    assert!(info.all_tokens_consumed && info.unknown_words.is_empty() && info.messages.is_empty() && info.ended_on_eof_word);
    assert_eq!(info.reads_past_end, 0);
    // what the reader makes of the file differs from the state written in the places the
    // original itself does not carry through
    let mut expect = s.clone();
    expect.schematic.update_wanted = 1; // the reader stores the flag in both places
    for (i, w) in expect.layout.windows.iter_mut().enumerate() {
        // WINDOW_TYPE clears the second display type, of the window and of the view shown
        // then, which in a fresh program is view i of window i; CAMERA_TYPE records it as the
        // view shown before
        w.display_type = [0, 0];
        w.slots[i].display_type = [0, 0];
        w.previous = i as i16;
    }
    // the PICTURE block gives the scene camera the aspect ratio of the picture format
    expect.layout.scene_camera_mut().aspect = 1.37;
    // the view the window changes to brings its own draw-over record; none of that here
    assert_eq!(check::structure_differences(&back, &expect), Vec::<String>::new());
    assert_eq!(back, expect);
    // and that state is stable: written and read once more it is the same
    let out2 = write(&back, &WriteOptions::default());
    assert_eq!(read(&out2).unwrap().0, back);
    assert_eq!(write(&read(&out2).unwrap().0, &WriteOptions::default()), out2);

    // from the start-up state, the empty photon map name is lost, then becomes "(null)"
    let s1 = Setup::default();
    let f1 = write(&s1, &WriteOptions::default());
    let s2 = read(&f1).unwrap().0;
    assert_eq!(s2.render.mental_ray.ph_map_file, None);
    let f2 = write(&s2, &WriteOptions::default());
    let s3 = read(&f2).unwrap().0;
    assert_eq!(s3.render.mental_ray.ph_map_file.as_deref(), Some(&b"(null)"[..]));
    let f3 = write(&s3, &WriteOptions::default());
    assert_eq!(f3, f2);
    assert_eq!(read(&f3).unwrap().0, s3);
}

#[test]
fn x87_int_conversion() {
    assert_eq!(c_float_to_int(3.9), 3);
    assert_eq!(c_float_to_int(-3.9), -3);
    assert_eq!(c_float_to_int(3e9), i32::MIN);
    assert_eq!(c_float_to_int(f32::NAN), i32::MIN);
    assert_eq!(c_float_to_int(-2147483648.0), i32::MIN);
}

#[test]
fn check_classifies_differences() {
    let old = b"SSET3\n   MOTION_BLUR 0 \n   A 1 B 2 \n   X 1e-007 \n   P '' \n   KEPT 1 \n   GONE 5 \n   T 1 \n";
    let new = b"SSET4\n   MOTION_BLUR 0 SUBFRAME ACTIVE \n   A 1 \n   B 2 \n   X 1e-07 \n   P '(null)' \n   KEPT 1 \n   T 2 \n   NEW 3 \n";
    let d = check::diff(old, new);
    assert!(d.header);
    assert_eq!(d.changed.get(&("MOTION_BLUR".to_string(), Change::Longer)), Some(&1));
    assert_eq!(d.changed.get(&("A".to_string(), Change::Shorter)), Some(&1));
    assert_eq!(d.changed.get(&("X".to_string(), Change::NumberFormat)), Some(&1));
    assert_eq!(d.changed.get(&("P".to_string(), Change::EmptyString)), Some(&1));
    assert_eq!(d.changed.get(&("T".to_string(), Change::Value)), Some(&1));
    assert_eq!(d.removed.keys().collect::<Vec<_>>(), ["GONE"]);
    assert_eq!(d.added.keys().collect::<Vec<_>>(), ["B", "NEW"]);
    assert!(check::diff(old, old).is_empty());
    // a whole made-up file through the three checks
    let data = file("3.5", "SHOW_FLAG SHW_POINTS 1 PREFERENCE PREF_MODE 4 EOF ");
    let r = check::check_file(&data, &Session::default());
    assert!(r.complete && r.error.is_none());
    assert!(!r.identical && r.diff.header);
    assert!(r.third_write_identical);
}
