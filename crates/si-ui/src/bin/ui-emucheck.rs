// SPDX-License-Identifier: MIT
//! Run a scenario of calls through the Rust port with the recording stand-ins and print the
//! transcript that `re/tools/si-ui/emu_ui.py` prints for the emulated original: per call the
//! outside calls with their arguments, the return value, and every byte of memory the call
//! changed. `re/tools/si-ui/compare.py` compares the two.
//!
//!   (ulimit -v 2000000; target/si-ui/release/ui-emucheck scenario.scn > transcript.txt)
//!
//! The scenario format is described at the top of `emu_ui.py`.
//!
//! Self-test: with `EMUCHECK_MUTATE=1` in the environment one bit of everything the port
//! produces is flipped before it is printed (the return value, the first byte of every run
//! of changed memory, the last argument digit of the first outside call). Every case that
//! prints anything at all must then differ from the original's.

use si_ui::mem::{Addr, P};
use si_ui::rec::{Answer, SharedLog};
use si_ui::sys::Outside;
use si_ui::Ui;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// What the port stores where the emulated original gets the address of a stand-in for a
/// function that exists only as a pointer.
const FAKE_TRAP: u32 = 0x6000_0ff0;

fn num(t: &str) -> u64 {
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let v = match t.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16),
        None => t.parse::<u64>(),
    }
    .unwrap_or_else(|_| panic!("bad number {t}"));
    if neg {
        v.wrapping_neg()
    } else {
        v
    }
}

fn hex(t: &str) -> Vec<u8> {
    (0..t.len() / 2).map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).expect("bad hex")).collect()
}

fn answer(toks: &[&str]) -> Answer {
    let mut a = Answer::default();
    for t in toks {
        if let Some(rest) = t.strip_prefix("poke:") {
            let (addr, bytes) = rest.split_once(':').expect("poke:ADDR:HEX");
            a.pokes.push((num(addr) as u32, hex(bytes)));
        } else if let Some(bits) = t.strip_prefix("d:") {
            a.vals.push(u64::from_str_radix(bits, 16).expect("bad double"));
        } else {
            a.vals.push(num(t));
        }
    }
    a
}

/// One call of a ported routine with the stack words the scenario gives; the value the
/// original leaves in eax, if the routine returns one.
fn dispatch(ui: &mut Ui, name: &str, a: &[u32]) -> Option<u32> {
    let w = |i: usize| a[i] as i32;
    let s = |i: usize| a[i] as i16;
    let p = |i: usize| a[i] as Addr;
    Some(match name {
        // ---- event.c
        "si_actionIsOnEditSRT" => ui.si_action_is_on_edit_srt(p(0)) as u32,
        "SI_DoubleClickStatus" => ui.si_double_click_status() as u32,
        "SI_getCurrentSwiftKey" => ui.si_get_current_swift_key() as u32,
        "SI_changeCurrentSwiftKeyModule" => return none(ui.si_change_current_swift_key_module()),
        "SI_mouseInterrupt" => ui.si_mouse_interrupt() as u32,
        "newEvent" => ui.new_event().addr(),
        "SI_initEvent" => return none(ui.si_init_event()),
        "SI_eventInitPos" => return none(ui.si_event_init_pos()),
        "qdeviceSupraMode" => return none(ui.qdevice_supra_mode()),
        "SI_unqdeviceSupraMode" => return none(ui.si_unqdevice_supra_mode()),
        "SI_getEvent" => ui.si_get_event().addr(),
        "SI_getMainEvent" => ui.si_get_main_event(s(0)).addr(),
        "si_getNextEvent" => ui.si_get_next_event(s(0)).addr(),
        "si_setActiveCanvas" => return none(ui.si_set_active_canvas(w(0))),
        "launchMenu" => ui.launch_menu(w(0), P::at(p(1))) as u32,
        "popQE" => ui.pop_qe().addr(),
        "si_pushQE" => return none(ui.si_push_qe(w(0), s(1), s(2), s(3), s(4), p(5))),
        "SI_fndEvent" => ui.si_fnd_event(w(0), s(1)) as u32,
        "SI_fndCanvasPart" => ui.si_fnd_canvas_part(P::at(p(0)), w(1), w(2)) as u32,
        "SI_mouseDown" => ui.si_mouse_down(w(0)) as u32,
        "SI_getMouse" => {
            let (x, y) = ui.si_get_mouse();
            ui.m.set_i16(p(0), x);
            ui.m.set_i16(p(1), y);
            return None;
        }
        "SI_queueKeyboard" => return none(ui.si_queue_keyboard()),
        "SI_unqueueKeyboard" => return none(ui.si_unqueue_keyboard()),
        "si_resetKeyboard" => return none(ui.si_reset_keyboard()),
        "SI_keyboardToCar" => return none(ui.si_keyboard_to_car(w(0), s(1), s(2), P::at(p(3)))),
        "scanSync" => return none(ui.scan_sync(s(0))),
        "SI_setTimeline" => return none(ui.si_set_timeline(w(0))),
        "SI_activeSupraMode" => return none(ui.si_active_supra_mode(s(0), s(1))),
        "SI_deactiveSupraMode" => return none(ui.si_deactive_supra_mode(s(0), s(1))),
        "findControl" => ui.find_control(w(0), w(1)).addr(),
        "si_disableRepeat" => return none(ui.si_disable_repeat()),
        "si_restoreRepeat" => return none(ui.si_restore_repeat()),
        "transformationBoxHandle" => ui.transformation_box_handle(P::at(p(0)), P::at(p(1))) as u32,
        "activateGroup" => {
            let part = ui.m.i16(p(2));
            ui.activate_group(P::at(p(0)), P::at(p(1)), part, w(3), s(4), s(5), s(6)) as u32
        }
        "si_setMnemonicMode" => return none(ui.si_set_mnemonic_mode(s(0))),
        "puckDown" => ui.puck_down(w(0)) as u32,
        "si_mouseDown" => ui.si_mouse_down_(w(0), s(1), s(2), s(3)) as u32,
        "keyUp" => ui.key_up(w(0)) as u32,
        "keyDown" => ui.key_down(w(0), s(1)) as u32,
        "keyb" => ui.keyb(w(0), s(1)) as u32,
        "checkKeyboard" => ui.check_keyboard() as u32,
        "SI_playBox" => ui.si_play_box(w(0), s(1), s(2), s(3)) as u32,
        "si_getKeySequence" => {
            let mut keys = [0i32; 4];
            ui.si_get_key_sequence(&mut keys);
            for (i, k) in keys.iter().enumerate() {
                ui.m.set_i32(p(0) + 4 * i as u32, *k);
            }
            return None;
        }
        "SI_keyDown" => ui.si_key_down(w(0), s(1)) as u32,
        "SI_keyUp" => ui.si_key_up(w(0)) as u32,
        // ---- window.c
        "si_roto_viewport_clr" => return none(ui.si_roto_viewport_clr(P::at(p(0)))),
        "windowSetGraf" => return none(ui.window_set_graf(P::at(p(0)), s(1), s(2), s(3), s(4))),
        "SI_windowDraw" => return none(ui.si_window_draw(P::at(p(0)))),
        "winCusLayDispArea" => return none(ui.win_cus_lay_disp_area()),
        "si_winCusLayDispMsg" => return none(ui.si_win_cus_lay_disp_msg()),
        "si_winCusLayClear" => return none(ui.si_win_cus_lay_clear()),
        "SI_viewSelect" => return none(ui.si_view_select(P::at(p(0)), P::at(p(1)), s(2), s(3))),
        "winCusLayHandle" => return none(ui.win_cus_lay_handle(P::at(p(0)), s(1), s(2))),
        "winCusLayReshapeHandle" => return none(ui.win_cus_lay_reshape_handle(P::at(p(0)), P::at(p(1)), P::at(p(2)), P::at(p(3)))),
        "si_viewSelectHandle" => return none(ui.si_view_select_handle(P::at(p(0)), s(1), s(2), w(3), P::at(p(4)))),
        "windowToggleSelect" => ui.window_toggle_select(P::at(p(0))) as u32,
        "windowToggleOffRgb" => return none(ui.window_toggle_off_rgb()),
        "si_resetToggleFlag" => return none(ui.si_reset_toggle_flag()),
        "restoreRgb" => return none(ui.restore_rgb()),
        "SI_windowIsActive" => ui.si_window_is_active(w(0)) as u32,
        "windowGetDispTyp" => ui.window_get_disp_typ(w(0)).addr(),
        "SI_windowCamActive" => ui.si_window_cam_active(w(0)).addr(),
        "SI_windowGetHilite" => return none(in_out(ui, p(0), false, |ui, g| ui.si_window_get_hilite(g))),
        "SI_windowViewGetHilite" => return none(in_out(ui, p(0), false, |ui, g| ui.si_window_view_get_hilite(g))),
        "SI_windowGetNext" => in_out(ui, p(0), true, |ui, g| ui.si_window_get_next(g)) as u32,
        "SI_windowGetNextVisible" => in_out(ui, p(0), true, |ui, g| ui.si_window_get_next_visible(g)) as u32,
        "SI_windowSet" => return none(ui.si_window_set(w(0), P::at(p(1)))),
        "SI_windowUnset" => return none(ui.si_window_unset()),
        "SI_getWindowViewport" => return none(ui.si_get_window_viewport(P::at(p(0)))),
        "fndViewPart" => ui.fnd_view_part(P::at(p(0)), w(1), w(2)) as u32,
        "SI_windowDrawAll" => return none(ui.si_window_draw_all()),
        "SI_viewGrafportSelect" => return none(ui.si_view_grafport_select(P::at(p(0)), P::at(p(1)), s(2), s(3))),
        "SI_getNbWindows" => ui.si_get_nb_windows() as u32,
        "getGadgetLabel" => ui.get_gadget_label(w(0)),
        "getGadgetOffset" => ui.get_gadget_offset(w(0)) as u32,
        "getGadgetWidth" => ui.get_gadget_width(w(0)) as u32,
        "locateGadget" => ui.locate_gadget(w(0), w(1)) as u32,
        "voidEvent" => return none(ui.void_event(P::at(p(0)))),
        "setTemplate" => return none(ui.set_template()),
        "windowTitleHandle" => return none(ui.window_title_handle(P::at(p(0)), P::at(p(1)))),
        "SI_setCurrentCam" => return none(ui.si_set_current_cam(P::at(p(0)), w(1))),
        "SI_windowGhostMenu" => return none(ui.si_window_ghost_menu(P::at(p(0)))),
        "windowToggleOffRgbWind" => return none(ui.window_toggle_off_rgb_wind(P::at(p(0)))),
        "restoreRgbWind" => return none(ui.restore_rgb_wind(P::at(p(0)))),
        "changeWindow" => return none(ui.change_window(P::at(p(0)), w(1))),
        "handleChangingDisplays" => return none(ui.handle_changing_displays(P::at(p(0)), w(1))),
        "windowZoomHandle" => return none(ui.window_zoom_handle(P::at(p(0)), P::at(p(1)))),
        "windowToggleHorizontal" => return none(ui.window_toggle_horizontal(P::at(p(0)))),
        "windowToggleVertical" => return none(ui.window_toggle_vertical(P::at(p(0)))),
        "SI_windowSizeSet" => return none(ui.si_window_size_set(P::at(p(0)), w(1))),
        "windowBigToSmall" => return none(ui.window_big_to_small(P::at(p(0)))),
        "windowSmallToBig" => return none(ui.window_small_to_big(P::at(p(0)))),
        "zoomWindow" => return none(ui.zoom_window(P::at(p(0)))),
        "windowSetAllVisible" => return none(ui.window_set_all_visible()),
        "changeWindowAll" => return none(ui.change_window_all(P::at(p(0)), w(1))),
        "winStoreBorder" => return none(ui.win_store_border(P::at(p(0)))),
        "winStoreWinPos" => return none(ui.win_store_win_pos(P::at(p(0)), w(1))),
        "winGetWinPos" => return none(ui.win_get_win_pos(P::at(p(0)), w(1))),
        "windowExpandHoriz" => return none(ui.window_expand_horiz(P::at(p(0)))),
        "windowUnExpandHoriz" => return none(ui.window_un_expand_horiz(P::at(p(0)))),
        "windowDrawRulers" => ui.window_draw_rulers(P::at(p(0))).addr(),
        "si_windowGadjetAll" => return none(ui.si_window_gadjet_all(s(0))),
        "updateCurrentWindow" => ui.update_current_window(P::at(p(0))) as u32,
        "SI_isShadeViewVisible" => ui.si_is_shade_view_visible() as u32,
        "isFcurveView" => in_out(ui, p(0), true, |ui, g| ui.is_fcurve_view(g)) as u32,
        "schemHandleModeMenu" => ui.schem_handle_mode_menu(P::at(p(0)), P::at(p(1))) as u16 as u32,
        "sprdHandleQueriesMenu" => ui.sprd_handle_queries_menu(P::at(p(0)), P::at(p(1))) as u16 as u32,
        // ---- drawWindow.c
        "SI_uiOpenWindow" => ui.si_ui_open_window(p(0), w(1), w(2), p(3), p(4), p(5), p(6)) as u32,
        "SI_uiCloseWindow" => return none(ui.si_ui_close_window(w(0))),
        "SI_uiGetWindowID" => ui.si_ui_get_window_id(w(0)) as u32,
        "SI_uiCloseAllWindow" => return none(ui.si_ui_close_all_window()),
        "si_uiHandleWindowEvent" => ui.si_ui_handle_window_event(p(0), P::at(p(1)), w(2), s(3)) as u32,
        "si_uiPropagateEvent" => return none(ui.si_ui_propagate_event(P::at(p(0)))),
        "si_uiDisableRedraw" => return none(ui.si_ui_disable_redraw()),
        "si_uiEnableRedraw" => return none(ui.si_ui_enable_redraw()),
        "SimpleListPtrC" => ui.simple_list_new(P::at(p(0)), w(1), w(2)).addr(),
        "~SimpleListPtrC" => return none(ui.simple_list_delete(P::at(p(0)), a[1])),
        "add" => ui.simple_list_add(P::at(p(0)), p(1)) as u32,
        "remove" => return none(ui.simple_list_remove(P::at(p(0)), w(1))),
        "ShaderWindowC" => ui.shader_window_new(P::at(p(0)), p(1), w(2), w(3), p(4), p(5), p(6), p(7)).addr(),
        "~ShaderWindowC" => return none(ui.shader_window_delete(P::at(p(0)), a[1])),
        "hit" => ui.shader_window_hit(P::at(p(0)), w(1), w(2)) as u32,
        "redraw" => return none(ui.shader_window_redraw(P::at(p(0)), s(1))),
        // ---- ui_misc.c
        "si_stringReading" => ui.si_string_reading(p(0)),
        "SI_mouseWarpSet" => ui.si_mouse_warp_set(f32::from_bits(a[0]), f32::from_bits(a[1])) as u32,
        "SI_mouseWarpGet" => ui.si_mouse_warp_get(p(0), p(1)) as u32,
        "SI_cursorChange" => return none(ui.si_cursor_change(w(0))),
        "viewWindSetMouse" => return none(ui.view_wind_set_mouse(w(0))),
        "SI_showActivity" => return none(ui.si_show_activity()),
        "SI_stopActivity" => return none(ui.si_stop_activity()),
        "SI_toggleActivity" => return none(ui.si_toggle_activity()),
        // ---- graf.c
        "si_handleStow" => return none(ui.si_handle_stow()),
        "si_handleUnstow" => return none(ui.si_handle_unstow()),
        "si_eventFilter" => ui.si_event_filter(P::at(p(0))) as u32,
        "refreshActiveWind" => return none(ui.refresh_active_wind(P::at(p(0)))),
        "autoSave" => return none(ui.auto_save()),
        // ---- init_graphic.c
        "SI_graphicInit" => return none(ui.si_graphic_init()),
        "SI_colorInit" => return none(ui.si_color_init()),
        "SI_colorMapStore" => ui.si_color_map_store(s(0)) as u32,
        "SI_colorMapRestore" => ui.si_color_map_restore(s(0)) as u32,
        "SI_colorMapSave" => ui.si_color_map_save(p(0)) as u32,
        "SI_colorMapLoad" => ui.si_color_map_load(p(0)) as u32,
        // ---- window.c, second pass
        "SI_windowReadInit" => ui.si_window_read_init(p(0)) as u32,
        "windowReadFile" => ui.window_read_file(p(0)) as u32,
        "windowCamRead" => return none(ui.window_cam_read(p(0), p(1))),
        "windowGridRead" => return none(ui.window_grid_read(p(0), p(1))),
        "windowRulerRead" => return none(ui.window_ruler_read(p(0), p(1))),
        "windowRead" => return none(ui.window_read(p(0), P::at(p(1)))),
        "si_windowPopupSetPos" => return none(ui.si_window_popup_set_pos(P::at(p(0)), p(1))),
        "SI_windowResetAll" => return none(ui.si_window_reset_all()),
        "SI_windowRestoreAll" => return none(ui.si_window_restore_all()),
        "SI_enableSchemCleanAll" => return none(ui.si_enable_schem_clean_all(s(0))),
        "SI_windowSetFcurveActive" => return none(ui.si_window_set_fcurve_active(s(0))),
        "SI_windowSetDopeSheetActive" => return none(ui.si_window_set_dope_sheet_active(s(0))),
        "SI_windowSetPerspectiveActive" => return none(ui.si_window_set_perspective_active()),
        "SI_changeSetOfViews" => return none(ui.si_change_set_of_views()),
        "schemGetModeMenuString" => ui.schem_get_mode_menu_string(p(0)),
        "setVisibleFields" => return none(ui.set_visible_fields()),
        "initGadget" => return none(ui.init_gadget()),
        "drawMenuGadget" => return none(ui.draw_menu_gadget(P::at(p(0)), w(1), w(2), w(3), w(4))),
        "deWindowDraw" => return none(ui.de_window_draw(P::at(p(0)))),
        "SI_windowGadjet" => return none(ui.si_window_gadjet(P::at(p(0)), w(1), s(2))),
        "SI_windowZoomWin" => return none(ui.si_window_zoom_win(P::at(p(0)))),
        "SI_unZoomWindow" => return none(ui.si_un_zoom_window(P::at(p(0)))),
        "SI_unZoomWindowWin" => return none(ui.si_un_zoom_window_win(P::at(p(0)))),
        "windowExpandVert" => return none(ui.window_expand_vert(P::at(p(0)))),
        "windowUnExpandVert" => return none(ui.window_un_expand_vert(P::at(p(0)))),
        "SI_windowLimitsHoriz" => return none(ui.si_window_limits_horiz(P::at(p(0)), p(1))),
        "SI_windowLimitsVert" => return none(ui.si_window_limits_vert(P::at(p(0)), p(1))),
        // ---- scroll.c
        "si_scrollTextViewFrameDraw" => return none(ui.si_scroll_text_view_frame_draw(P::at(p(0)), s(1))),
        "scrollTextViewFrameDrawGL" => return none(ui.scroll_text_view_frame_draw_gl(P::at(p(0)), s(1))),
        "SI_scrollTextViewDraw" => return none(ui.si_scroll_text_view_draw(P::at(p(0)), s(1), s(2))),
        "scrollTextViewDrawGL" => return none(ui.scroll_text_view_draw_gl(P::at(p(0)), s(1), s(2))),
        "scrollSliderDraw" => return none(ui.scroll_slider_draw(P::at(p(0)), s(1), s(2))),
        "scrollSliderDrawGL" => return none(ui.scroll_slider_draw_gl(P::at(p(0)), s(1), s(2))),
        "scrollTuningButtonDraw" => return none(ui.scroll_tuning_button_draw(P::at(p(0)))),
        "scrollTuningButtonDrawGL" => return none(ui.scroll_tuning_button_draw_gl(P::at(p(0)))),
        "SI_scrollSize" => {
            let (a, b, c, d) = ui.si_scroll_size(P::at(p(0)));
            for (i, v) in [a, b, c, d].into_iter().enumerate() {
                ui.m.set_i32(p(1 + i), v);
            }
            return None;
        }
        // ---- scrollbar.c, slider.c
        "SI_scrollbarCreate" => ui
            .si_scrollbar_create(p(0), w(1), w(2), w(3), s(4), a[5], a[6], a[7], a[8], a[9], a[10], a[11], p(12), p(13))
            .addr(),
        "SI_scrollbarSetSlider" => return none(ui.si_scrollbar_set_slider(P::at(p(0)), w(1), s(2))),
        "SI_scrollbarSetIncrements" => return none(ui.si_scrollbar_set_increments(P::at(p(0)), a[1], a[2], a[3])),
        "SI_scrollbarSetDelays" => return none(ui.si_scrollbar_set_delays(P::at(p(0)), a[1] as u16, a[2] as u16)),
        "SI_scrollbarSetUnits" => return none(ui.si_scrollbar_set_units(P::at(p(0)), a[1], a[2], a[3], a[4], s(5))),
        "SI_scrollbarSetUserData" => return none(ui.si_scrollbar_set_user_data(P::at(p(0)), p(1))),
        "SI_scrollbarSetCallback" => return none(ui.si_scrollbar_set_callback(P::at(p(0)), p(1))),
        "SI_scrollbarSetPos" => return none(ui.si_scrollbar_set_pos(P::at(p(0)), w(1), w(2), w(3))),
        "SI_scrollbarIsVertical" => ui.si_scrollbar_is_vertical(P::at(p(0))) as u32,
        "SI_scrollbarMoveUp" => return none(ui.si_scrollbar_move_up(P::at(p(0)), w(1), s(2))),
        "SI_scrollbarMoveDown" => return none(ui.si_scrollbar_move_down(P::at(p(0)), w(1), s(2))),
        "SI_scrollbarMoveLeft" => return none(ui.si_scrollbar_move_left(P::at(p(0)), w(1), s(2))),
        "SI_scrollbarMoveRight" => return none(ui.si_scrollbar_move_right(P::at(p(0)), w(1), s(2))),
        "SI_scrollbarPageUp" => return none(ui.si_scrollbar_page_up(P::at(p(0)), s(1))),
        "SI_scrollbarPageDown" => return none(ui.si_scrollbar_page_down(P::at(p(0)), s(1))),
        "SI_scrollbarPageLeft" => return none(ui.si_scrollbar_page_left(P::at(p(0)), s(1))),
        "SI_scrollbarPageRight" => return none(ui.si_scrollbar_page_right(P::at(p(0)), s(1))),
        "SI_scrollbarDraw" => return none(ui.si_scrollbar_draw(P::at(p(0)), s(1))),
        "scbDraw" => return none(ui.scb_draw(P::at(p(0)))),
        "scbDetectPart" => ui.scb_detect_part(P::at(p(0)), w(1), w(2)) as u32,
        "rulerNew" => ui.ruler_new(P::at(p(0))),
        "si_rulerCreateHoriz" => ui.si_ruler_create_horiz(P::at(p(0))),
        "si_rulerCreateVert" => ui.si_ruler_create_vert(P::at(p(0))),
        "rulerInitialize" => return none(ui.ruler_initialize(p(0))),
        "SI_rulerSetActive" => return none(ui.si_ruler_set_active(p(0), s(1))),
        "SI_rulerDraw" => return none(ui.si_ruler_draw(p(0))),
        "rulerDrawVertDope" => return none(ui.ruler_draw_vert_dope(p(0))),
        "rulerDrawVertSound" => return none(ui.ruler_draw_vert_sound(p(0))),
        "rulerFree" => return none(ui.ruler_free(p(0))),
        "si_rulerTransfer" => return none(ui.si_ruler_transfer(p(0), p(1))),
        "rulerStringFormat" => {
            let t = ui.ruler_string_format(f32::from_bits(a[0]), f32::from_bits(a[1]));
            ui.m.put_cstr(p(2), &t);
            return None;
        }
        "scrollTexedLeftArrow" => return none(ui.scroll_texed_left_arrow(P::at(p(0)), s(1))),
        "scrollTexedLeftArrowGL" => return none(ui.scroll_texed_left_arrow_gl(P::at(p(0)), s(1))),
        "scrollTexedRightArrow" => return none(ui.scroll_texed_right_arrow(P::at(p(0)), s(1))),
        "scrollTexedRightArrowGL" => return none(ui.scroll_texed_right_arrow_gl(P::at(p(0)), s(1))),
        "scrollGrafDraw" => return none(ui.scroll_graf_draw(P::at(p(0)))),
        "scrollGrafDrawGL" => return none(ui.scroll_graf_draw_gl(P::at(p(0)))),
        "scrollHvSliderDraw" => return none(ui.scroll_hv_slider_draw(P::at(p(0)))),
        "scrollHvSliderDrawGL" => return none(ui.scroll_hv_slider_draw_gl(P::at(p(0)))),
        "scrollStaticDraw" => return none(ui.scroll_static_draw(P::at(p(0)))),
        "scrollStaticDrawGL" => return none(ui.scroll_static_draw_gl(P::at(p(0)))),
        "SI_scrollSetInterval" => return none(ui.si_scroll_set_interval(p(0), w(1), f32::from_bits(a[2]), f32::from_bits(a[3]))),
        "SI_SetColSliderRefresh" => return none(ui.si_set_col_slider_refresh(p(0), a[1])),
        "SI_scrollGetDrawFuncPtr" => ui.si_scroll_get_draw_func_ptr(w(0)),
        "rulerDrawHoriz" => none_r(ui.ruler_draw_horiz(p(0)), p(0)),
        "rulerDrawVert" => none_r(ui.ruler_draw_vert(p(0)), p(0)),
        "rulerIncSize" => ui.ruler_inc_size(f32::from_bits(a[0]), f32::from_bits(a[1])).to_f32().to_bits(),
        "SI_rulerVertDopeSetView" => ui.si_ruler_vert_dope_set_view(p(0)) as u32,
        "SI_rulerVertSoundSetView" => ui.si_ruler_vert_sound_set_view(p(0)) as u32,
        "rulerStringOffset" => ui.ruler_string_offset(f32::from_bits(a[0])) as u32,
        "SI_sliderSet" => return none(ui.si_slider_set(p(0), s(1), p(2), s(3))),
        "drawSliderBut" => return none(ui.draw_slider_but(P::at(p(0)), w(1), s(2))),
        "drawSliderButGL" => return none(ui.draw_slider_but_gl(P::at(p(0)), w(1), s(2))),
        name if name.starts_with("in") && name.ends_with("CB") => {
            // a gadget handler: its argument record is in the scenario's arena
            let cb = ui.m[P::<si_ui::records::Callback>::at(p(0))];
            if !ui.call_handler_by_name(name, &cb) {
                panic!("no-port:{name}");
            }
            return None;
        }
        _ => panic!("no-port:{name}"),
    })
}

fn none_r(_: (), r: u32) -> u32 {
    r
}

fn none(_: ()) -> Option<u32> {
    None
}

/// A routine that takes a pointer to a window pointer: the pointer variable lives in the
/// scenario's arena. `read` says whether the routine reads it before writing.
fn in_out<R>(ui: &mut Ui, at: Addr, read: bool, f: impl FnOnce(&mut Ui, &mut P<si_ui::records::Grafport>) -> R) -> R {
    let mut g = if read { P::at(ui.m.u32(at)) } else { P::NULL };
    let before = g;
    let r = f(ui, &mut g);
    if g != before || !read {
        ui.m.set_u32(at, g.addr());
    }
    r
}

fn fresh() -> (Ui, SharedLog) {
    let (o, log) = Outside::recording();
    let mut ui = Ui::new(o);
    if record_faults() {
        ui.m.on_fault(si_mem::OnFault::Record);
    }
    if let Ok(w) = std::env::var("UI_WATCH") {
        // UI_WATCH=ADDR:LEN[,ADDR:LEN…]: for looking into one case, not part of the check
        for part in w.split(',') {
            let (a, n) = part.split_once(':').expect("UI_WATCH=ADDR:LEN");
            log.borrow_mut().watch.push((num(a) as u32, num(n) as u32));
        }
    }
    (ui, log)
}

/// `UI_FAULT_RECORD=1`: a measurement for finding 0024, not the check. A fault is then only
/// recorded (reads answer zeros, writes are dropped) and the routine goes on, as in the
/// image's default mode; a call during which one was recorded is printed as stopped by a
/// fault after its outside calls. Compared with the original's transcripts this shows how
/// many faulting cases still agree when the port does not stop at the fault.
fn record_faults() -> bool {
    std::env::var_os("UI_FAULT_RECORD").is_some()
}

/// `ui-emucheck --init-image`: every byte the crate's start-up writes into the image, as
/// `ADDRESS HEX` lines, for `re/tools/si-ui/check_init.py` to hold against the binary.
fn print_init_image() {
    let m0 = si_ui::mem::image();
    let before = m0.snapshot();
    let (ui, _) = fresh();
    for (addr, bytes) in ui.m.changed(&before) {
        let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        println!("{:08x} {}", addr, hex);
    }
}

fn main() {
    // the original's tables come from the user's install (unit rta)
    si_rt::check_or_exit("ui-emucheck");
    if std::env::args().nth(1).as_deref() == Some("--init-image") {
        return print_init_image();
    }
    let path = std::env::args().nth(1).expect("usage: ui-emucheck scenario.scn");
    let text = std::fs::read_to_string(&path).expect("cannot read the scenario");
    std::panic::set_hook(Box::new(|_| {}));
    let mutate = std::env::var("EMUCHECK_MUTATE").is_ok();
    let (mut ui, mut log) = fresh();
    let mut out = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        match toks[0] {
            "reset" => (ui, log) = fresh(),
            "mem" => {
                let data = if toks[2].starts_with("trap:") { FAKE_TRAP.to_le_bytes().to_vec() } else { hex(toks[2]) };
                ui.m.set_bytes(num(toks[1]) as u32, &data);
            }
            "file" => ui.files.add(toks[1].as_bytes(), toks.get(2).map_or(Vec::new(), |h| hex(h))),
            "open" => {
                ui.files.open_named(toks[1].as_bytes());
            }
            "answer" => log.borrow_mut().answer(toks[1], answer(&toks[2..])),
            "always" => log.borrow_mut().always(toks[1], answer(&toks[2..])),
            "call" => {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
                let mut rest = &toks[2..];
                let mut ret = None;
                if let Some(last) = rest.last() {
                    if let Some(kind) = last.strip_prefix("ret:") {
                        ret = Some(kind);
                        rest = &rest[..rest.len() - 1];
                    }
                }
                let args: Vec<u32> = rest.iter().map(|t| num(t) as u32).collect();
                log.borrow_mut().lines.clear();
                let before = ui.m.snapshot();
                ui.m.clear_fault();
                let result = catch_unwind(AssertUnwindSafe(|| dispatch(&mut ui, toks[1], &args)));
                let lines = std::mem::take(&mut log.borrow_mut().lines);
                match result {
                    Err(e) => {
                        let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                        // a fault of the image unwinds with a `Stop` payload (si-mem)
                        let kind = if e.downcast_ref::<si_mem::Stop>().is_some() {
                            "fault".to_string()
                        } else if msg.starts_with("log-overflow") {
                            "log-overflow".to_string()
                        } else {
                            format!("panic:{}", msg.split_whitespace().next().unwrap_or(""))
                        };
                        for l in lines.iter().take(50) {
                            out.push_str(l);
                            out.push('\n');
                        }
                        out.push_str(&format!("= stop {kind}\n"));
                    }
                    Ok(_) if record_faults() && ui.m.fault().is_some() => {
                        for l in lines.iter().take(50) {
                            out.push_str(l);
                            out.push('\n');
                        }
                        out.push_str("= stop fault\n");
                    }
                    Ok(eax) => {
                        for (i, l) in lines.iter().enumerate() {
                            if mutate && i == 0 {
                                // "(…x)" becomes "(…y)": one argument digit off
                                let cut = l.len() - 1;
                                let flipped = if l.ends_with("()") { "~)".to_string() } else { format!("{})", (l.as_bytes()[cut - 1] ^ 1) as char) };
                                out.push_str(&l[..if l.ends_with("()") { cut } else { cut - 1 }]);
                                out.push_str(&flipped);
                            } else {
                                out.push_str(l);
                            }
                            out.push('\n');
                        }
                        let eax = eax.map(|v| if mutate { v ^ 1 } else { v });
                        match (ret, eax) {
                            (Some("eax"), Some(v)) => out.push_str(&format!("= ret {:#x}\n", v)),
                            (Some("ax"), Some(v)) => out.push_str(&format!("= ret {:#x}\n", v & 0xffff)),
                            // st0 results are floats here: the f32 bits come back in eax
                            (Some("st0"), Some(v)) => {
                                let (m, e) = si_x87::F80::from_f32(f32::from_bits(v)).to_bits();
                                out.push_str(&format!("= ret st0 {:x}:{:x}\n", m, e))
                            }
                            _ => {}
                        }
                        for (addr, mut bytes) in ui.m.changed(&before) {
                            if mutate {
                                bytes[0] ^= 1;
                            }
                            out.push_str(&format!("= mem {:08x} ", addr));
                            for b in bytes {
                                out.push_str(&format!("{:02x}", b));
                            }
                            out.push('\n');
                        }
                        for (handle, stream) in ui.files.opened() {
                            out.push_str(&format!("= file {:08x} pos {} eof {} closed {}\n", handle, stream.position(), stream.eof as u8, stream.closed as u8));
                        }
                    }
                }
            }
            other => panic!("bad scenario line: {other}"),
        }
    }
    print!("{out}");
}
