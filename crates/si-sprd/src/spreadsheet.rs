// SPDX-License-Identifier: MIT
//! `spreadsheet.c`: the data-side routines of the grid widget (finding 0043): the setters of
//! its callbacks, the cell and column data pointers, the selection list's cursor and the
//! check and colour cells. Drawing, events and dialogs are not ported.
//!
//! A sheet (`__SI_StructSpdsheet`): rows (+0x00) and columns (+0x04) as counts, the selection
//! count (+0x18), the row array (+0x2c, pointers to arrays of 0x38 byte cells), the selection
//! list head (+0x30) and first element (+0x34), the cursor (+0x38, +0x3c), the refresh flag
//! (+0x48), the callbacks (+0x60.. +0x78) and the column data (+0x6c). A cell: its kind
//! (+0x00), a word (+0x04), the flags byte (+0x14), editable (+0x16), valid (+0x18), the
//! data pointer (+0x1c), the select, value changed, deselect and double click callbacks
//! (+0x20, +0x24, +0x28, +0x2c), the free function (+0x30) and the edit callback (+0x34).

use si_mem::{clib, Mem, Outside, Routine};

const CELL: u32 = 0x38;

/// `SI_spdDrawCell`, drawn when a check or colour cell changes and the sheet refreshes.
pub const DRAW_CELL: Routine = Routine::new("SI_spdDrawCell", 0x083c_7d0c);
/// The number of colours of the palette (16 bits).
pub const SI_COLOR_NB: u32 = 0x0878_1372;

fn cell(m: &Mem, sh: u32, row: u32, col: u32) -> u32 {
    m.u32(m.u32(sh + 0x2c).wrapping_add(row.wrapping_mul(4))).wrapping_add(col.wrapping_mul(CELL))
}

fn in_range(m: &Mem, sh: u32, row: u32, col: u32) -> bool {
    sh != 0 && row < m.u32(sh) && col < m.u32(sh + 4)
}

/// [soft 0x083c6ec0 SI_spdSetRefresh]
pub fn set_refresh(m: &mut Mem, sh: u32, on: i16) {
    m.set_u16(sh + 0x48, (on != 0) as u16);
}

/// Sets the callback at `off` of one cell, of a row's cells (`col < 0`) or of a column's
/// (`row < 0`); both negative sets nothing. The five setters differ in the offset only.
fn set_fct(m: &mut Mem, sh: u32, row: i32, col: i32, f: u32, off: u32) {
    if row < 0 || col < 0 {
        if row < 0 && col > -1 {
            let mut r = 0u32;
            if m.u32(sh) != 0 {
                loop {
                    let p = m.u32(m.u32(sh + 0x2c).wrapping_add(r * 4)).wrapping_add(off).wrapping_add((col as u32).wrapping_mul(CELL));
                    m.set_u32(p, f);
                    r += 1;
                    if r >= m.u32(sh) {
                        break;
                    }
                }
            }
        } else if row > -1 && col < 0 && m.u32(sh + 4) != 0 {
            let mut c = 0u32;
            let mut at = 0u32;
            loop {
                c += 1;
                let p = m.u32(m.u32(sh + 0x2c).wrapping_add((row as u32).wrapping_mul(4))).wrapping_add(off).wrapping_add(at);
                m.set_u32(p, f);
                at = at.wrapping_add(CELL);
                if c >= m.u32(sh + 4) {
                    break;
                }
            }
        }
    } else {
        let p = m.u32(m.u32(sh + 0x2c).wrapping_add((row as u32).wrapping_mul(4))).wrapping_add(off).wrapping_add((col as u32).wrapping_mul(CELL));
        m.set_u32(p, f);
    }
}

/// [soft 0x083c6edc SI_spdSetDoubleClickFct]
pub fn set_double_click_fct(m: &mut Mem, sh: u32, row: i32, col: i32, f: u32) {
    set_fct(m, sh, row, col, f, 0x2c);
}
/// [soft 0x083c6f70 SI_spdSetValueChangedFct]
pub fn set_value_changed_fct(m: &mut Mem, sh: u32, row: i32, col: i32, f: u32) {
    set_fct(m, sh, row, col, f, 0x24);
}
/// [soft 0x083c7000 SI_spdSetDeselectFct]
pub fn set_deselect_fct(m: &mut Mem, sh: u32, row: i32, col: i32, f: u32) {
    set_fct(m, sh, row, col, f, 0x28);
}
/// [soft 0x083c7090 SI_spdSetSelectFct]
pub fn set_select_fct(m: &mut Mem, sh: u32, row: i32, col: i32, f: u32) {
    set_fct(m, sh, row, col, f, 0x20);
}
/// [soft 0x083c719c SI_spdSetEditFct]
pub fn set_edit_fct(m: &mut Mem, sh: u32, row: i32, col: i32, f: u32) {
    set_fct(m, sh, row, col, f, 0x34);
}

/// [soft 0x083c7120 SI_spdSetEditPrologueFct]
pub fn set_edit_prologue_fct(m: &mut Mem, sh: u32, f: u32) {
    m.set_u32(sh + 0x64, f);
}
/// [soft 0x083c7130 SI_spdSetEditCompletedFct]
pub fn set_edit_completed_fct(m: &mut Mem, sh: u32, f: u32) {
    m.set_u32(sh + 0x68, f);
}
/// [soft 0x083c7140 SI_spdSetFreeColDataFct]
pub fn set_free_col_data_fct(m: &mut Mem, sh: u32, f: u32) {
    m.set_u32(sh + 0x70, f);
}
/// The old user data is handed to the free function first (the original passes the function's
/// own address as the argument) [soft 0x083c7150 SI_spdSetUserDataPtr].
pub fn set_user_data_ptr(m: &mut Mem, o: &mut dyn Outside, sh: u32, p: u32) {
    if m.u32(sh + 0x74) != 0 {
        let f = m.u32(sh + 0x78);
        if f != 0 {
            o.call(m, Routine::at(f), &[f]);
        }
    }
    m.set_u32(sh + 0x74, p);
}
/// [soft 0x083c717c SI_spdSetFreeUserDataFct]
pub fn set_free_user_data_fct(m: &mut Mem, sh: u32, f: u32) {
    m.set_u32(sh + 0x78, f);
}
/// [soft 0x083c718c SI_spdSetSelectElementFct]
pub fn set_select_element_fct(m: &mut Mem, sh: u32, f: u32) {
    m.set_u32(sh + 0x60, f);
}
/// [soft 0x083c7ce8 SI_spdSetCellAppDirtyFlag]
pub fn set_cell_app_dirty_flag(m: &mut Mem, sh: u32, row: i32, col: i32) {
    let p = cell(m, sh, row as u32, col as u32).wrapping_add(0x14);
    let v = m.u8(p) | 4;
    m.set_u8(p, v);
}
/// [soft 0x083c86f4 SI_spdGetNbCols]
pub fn get_nb_cols(m: &Mem, sh: u32) -> u32 {
    if sh == 0 { 0 } else { m.u32(sh + 4) }
}
/// [soft 0x083c870c SI_spdGetNbRows]
pub fn get_nb_rows(m: &Mem, sh: u32) -> u32 {
    if sh == 0 { 0 } else { m.u32(sh) }
}
/// [soft 0x083c8728 SI_spdGetCellDataPtr]
pub fn get_cell_data_ptr(m: &Mem, sh: u32, row: u32, col: u32) -> u32 {
    if in_range(m, sh, row, col) { m.u32(cell(m, sh, row, col) + 0x1c) } else { 0 }
}
/// The old data is handed to the cell's free function first
/// [soft 0x083c875c SI_spdSetCellDataPtr].
pub fn set_cell_data_ptr(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, p: u32) -> u32 {
    if !in_range(m, sh, row, col) {
        return 0;
    }
    let c = cell(m, sh, row, col);
    let old = m.u32(c + 0x1c);
    if old != 0 && m.u32(c + 0x30) != 0 {
        let f = m.u32(c + 0x30);
        o.call(m, Routine::at(f), &[old]);
    }
    m.set_u32(c + 0x1c, p);
    1
}
/// [soft 0x083c87b4 SI_spdGetColumnDataPtr]
pub fn get_column_data_ptr(m: &Mem, sh: u32, col: u32) -> u32 {
    m.u32(m.u32(sh + 0x6c).wrapping_add(col.wrapping_mul(4)))
}
/// [soft 0x083c87c8 SI_spdSetColumnDataPtr]
pub fn set_column_data_ptr(m: &mut Mem, o: &mut dyn Outside, sh: u32, col: u32, p: u32) {
    let slot = m.u32(sh + 0x6c).wrapping_add(col.wrapping_mul(4));
    if m.u32(slot) != 0 {
        let f = m.u32(sh + 0x70);
        if f != 0 {
            o.call(m, Routine::at(f), &[slot]);
        }
    }
    let slot = m.u32(sh + 0x6c).wrapping_add(col.wrapping_mul(4));
    m.set_u32(slot, p);
}
/// [soft 0x083c8804 SI_spdSetCellFreeFct]
pub fn set_cell_free_fct(m: &mut Mem, sh: u32, row: u32, col: u32, f: u32) -> u32 {
    if !in_range(m, sh, row, col) {
        return 0;
    }
    let c = cell(m, sh, row, col);
    m.set_u32(c + 0x30, f);
    1
}
/// [soft 0x083c8948 SI_spdGetSelectionFlag]
pub fn get_selection_flag(m: &Mem, sh: u32, row: u32, col: u32) -> u32 {
    if in_range(m, sh, row, col) { (m.u8(cell(m, sh, row, col) + 0x14) & 1) as u32 } else { 0 }
}
/// [soft 0x083c8984 SI_spdGetCurrentSelection]
pub fn get_current_selection(m: &mut Mem, sh: u32, prow: u32, pcol: u32) -> u32 {
    let mut r = 0;
    if m.u32(sh + 0x30) != 0 && m.u32(sh + 0x18) != 0 {
        let n = m.u32(sh + 0x30);
        let (a, b) = (m.u32(n + 0xc), m.u32(n + 0x10));
        m.set_u32(prow, a);
        m.set_u32(pcol, b);
        r = 1;
    }
    let c = m.u32(sh + 0x38);
    m.set_u32(sh + 0x3c, c);
    r
}
/// [soft 0x083c89c0 SI_spdGetFirstSelection]
pub fn get_first_selection(m: &mut Mem, sh: u32, prow: u32, pcol: u32) -> u32 {
    if m.u32(sh + 0x34) != 0 && m.u32(sh + 0x18) != 0 {
        let n = m.u32(m.u32(sh + 0x34));
        let (a, b) = (m.u32(n + 0xc), m.u32(n + 0x10));
        m.set_u32(prow, a);
        m.set_u32(pcol, b);
        1
    } else {
        0
    }
}
/// [soft 0x083c89fc SI_spdGetNextSelection]
pub fn get_next_selection(m: &mut Mem, sh: u32, prow: u32, pcol: u32) -> u32 {
    let cur = m.u32(sh + 0x3c);
    if cur == 0 {
        return 0;
    }
    let next = m.u32(cur + 8);
    m.set_u32(sh + 0x3c, next);
    if next == 0 {
        return 0;
    }
    let n = m.u32(next);
    let (a, b) = (m.u32(n + 0xc), m.u32(n + 0x10));
    m.set_u32(prow, a);
    m.set_u32(pcol, b);
    1
}
/// [soft 0x083c8a3c SI_spdGetNbSelected]
pub fn get_nb_selected(m: &Mem, sh: u32) -> u32 {
    if sh == 0 { 0 } else { m.u32(sh + 0x18) }
}
/// [soft 0x083c9124 SI_spdGetCheckCell]
pub fn get_check_cell(m: &mut Mem, sh: u32, row: u32, col: u32, out: u32) -> u32 {
    if in_range(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        if m.u32(c) == 5 {
            let v = m.u16(c + 4);
            m.set_u16(out, v);
            return 1;
        }
    }
    0
}
/// [soft 0x083c91f4 SI_spdGetColorCell]
pub fn get_color_cell(m: &mut Mem, sh: u32, row: u32, col: u32, out: u32) -> u32 {
    if in_range(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        if m.u32(c) == 6 {
            let v = m.u32(c + 4);
            m.set_u32(out, v);
            return 1;
        }
    }
    0
}
/// [soft 0x083c9830 SI_spdIsCellValid]
pub fn is_cell_valid(m: &Mem, sh: u32, row: i32, col: i32) -> i32 {
    m.i16(cell(m, sh, row as u32, col as u32) + 0x18) as i32
}
/// [soft 0x083c980c SI_spdIsCellEditable]
pub fn is_cell_editable(m: &Mem, sh: u32, row: i32, col: i32) -> i32 {
    m.i16(cell(m, sh, row as u32, col as u32) + 0x16) as i32
}


// ---------------------------------------------------------------------------------------
// creation, disposal, texted cells, check and colour cells, selection setters (2026-10-28)

/// Routines the grid calls and this file does not reproduce here: the other drawing
/// routines of the file (they are checked on their own), the toolkit's text and font calls,
/// and the dialog module's calculator.
pub mod hook {
    use si_mem::Routine;
    pub const DRAW_APP_DIRTY_CELLS: Routine = Routine::new("spdDrawAppDirtyCells", 0x083c_7abc);
    pub const GETFONT: Routine = Routine::new("getfont", 0x0860_ec74);
    pub const FONT: Routine = Routine::new("font", 0x0860_ec44);
    pub const STRWIDTH: Routine = Routine::new("strwidth", 0x0860_f084);
    pub const GETHEIGHT: Routine = Routine::new("getheight", 0x0860_eca4);
    pub const TEXTED_PERFORM_CALC: Routine = Routine::new("si_textedPerformCalc", 0x0838_eb68);
}
use hook::*;

/// The application font (16 bits) read by `SI_spdCreate`.
pub const SI_APPF: u32 = 0x0878_1644;

fn rowp(m: &Mem, sh: u32, row: u32) -> u32 {
    m.u32(m.u32(sh + 0x2c).wrapping_add(row.wrapping_mul(4)))
}

fn refresh_draw(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32) {
    if m.u16(sh + 0x48) != 0 {
        o.call(m, DRAW_CELL, &[sh, row, col]);
    }
}

/// What `sprintf(buf, "%ld", v)` writes.
fn sprintf_ld(m: &mut Mem, buf: u32, v: i32) {
    m.put_cstr(buf, v.to_string().as_bytes());
}

/// What `sprintf(buf, "%lf", v)` writes (six decimals; the C library's `inf` and `nan`).
fn sprintf_lf(m: &mut Mem, buf: u32, v: f64) {
    let t = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{:.6}", v)
    };
    m.put_cstr(buf, t.as_bytes());
}

/// `strtod(text, NULL)` for decimal text, "inf", "infinity" and "nan" (case blind), after
/// blanks and a sign. Departure: the hexadecimal float form (`0x1.8p3`) reads as 0, as the
/// text up to the `x` does; the sheet's texts never hold one.
fn strtod_text(t: &[u8]) -> f64 {
    let mut i = t.iter().take_while(|c| matches!(**c, b' ' | b'\t' | b'\n' | 11 | 12 | b'\r')).count();
    let neg = t.get(i) == Some(&b'-');
    if matches!(t.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let rest = &t[i..];
    let low: Vec<u8> = rest.iter().take(8).map(|c| c.to_ascii_lowercase()).collect();
    let sign = |v: f64| if neg { -v } else { v };
    if low.starts_with(b"inf") {
        return sign(f64::INFINITY);
    }
    if low.starts_with(b"nan") {
        return sign(f64::NAN);
    }
    let mut j = 0;
    while rest.get(j).is_some_and(|c| c.is_ascii_digit()) {
        j += 1;
    }
    let mut digits = j;
    if rest.get(j) == Some(&b'.') {
        j += 1;
        let f = j;
        while rest.get(j).is_some_and(|c| c.is_ascii_digit()) {
            j += 1;
        }
        digits += j - f;
    }
    if digits == 0 {
        return 0.0;
    }
    if matches!(rest.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(rest.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        if rest.get(k).is_some_and(|c| c.is_ascii_digit()) {
            while rest.get(k).is_some_and(|c| c.is_ascii_digit()) {
                k += 1;
            }
            j = k;
        }
    }
    let s = std::str::from_utf8(&rest[..j]).unwrap_or("0");
    let s = if s.starts_with('.') { format!("0{s}") } else { s.to_string() };
    let s = s.replace(".e", ".0e").replace(".E", ".0E");
    let s = if s.ends_with('.') { format!("{s}0") } else { s };
    sign(s.parse::<f64>().unwrap_or(0.0))
}

fn in_sheet(m: &Mem, sh: u32, row: u32, col: u32) -> bool {
    sh != 0 && row < m.u32(sh) && col < m.u32(sh + 4)
}

/// [soft 0x083c90b8 SI_spdSetCheckCell]
pub fn set_check_cell(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, v: i16) -> i16 {
    let mut r = 0;
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        if m.u32(c) == 5 {
            m.set_u16(c + 4, v as u16);
            r = 1;
            refresh_draw(m, o, sh, row, col);
        }
    }
    r
}

/// [soft 0x083c9170 SI_spdSetColorCell]
pub fn set_color_cell(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, v: i32) -> i16 {
    let mut r = 0;
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        if m.u32(c) == 6 && v > -2 && v < m.u16(SI_COLOR_NB) as i32 {
            m.set_u32(c + 4, v as u32);
            r = 1;
            refresh_draw(m, o, sh, row, col);
        }
    }
    r
}

fn free_text_words(m: &mut Mem, c: u32) {
    if m.u32(c + 8) != 0 {
        let p = m.u32(c + 8);
        m.free(p);
        m.set_u32(c + 8, 0);
    }
    if m.u32(c + 4) != 0 {
        let p = m.u32(c + 4);
        m.free(p);
        m.set_u32(c + 4, 0);
    }
}

/// [soft 0x083c8844 SI_spdSetColType]
pub fn set_col_type(m: &mut Mem, o: &mut dyn Outside, sh: u32, col: u32, kind: i32, text: u32) -> i16 {
    let mut r = 0;
    if sh != 0 && col < m.u32(sh + 4) {
        r = 1;
        let mut row = 0u32;
        if m.u32(sh) != 0 {
            loop {
                let c = col.wrapping_mul(CELL).wrapping_add(rowp(m, sh, row));
                if (m.u32(c).wrapping_sub(5)) > 1 {
                    free_text_words(m, c);
                }
                m.set_u32(c, kind as u32);
                if kind == 5 {
                    m.set_u16(c + 4, 0);
                } else if kind == 6 {
                    m.set_u32(c + 4, 0xFFFF_FFFF);
                } else {
                    m.set_u32(c + 8, 0);
                    if text == 0 {
                        m.set_u32(c + 4, 0);
                    } else {
                        let p = m.strdup(text);
                        m.set_u32(c + 4, p);
                    }
                }
                refresh_draw(m, o, sh, row, col);
                row += 1;
                if row >= m.u32(sh) {
                    break;
                }
            }
        }
    }
    r
}

/// [soft 0x083c8c78 SI_spdGetTextedCell]
pub fn get_texted_cell(m: &mut Mem, sh: u32, row: u32, col: u32, out: u32) -> i16 {
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        if m.u32(c).wrapping_sub(5) > 1 {
            let mut p = m.u32(c + 8);
            if p == 0 {
                p = EMPTY_349;
            }
            let d = m.strdup(p);
            m.set_u32(out, d);
            return 1;
        }
    }
    0
}

const EMPTY_349: u32 = 0x086c_a349;
const EMPTY_34E: u32 = 0x086c_a34e;
const EMPTY_353: u32 = 0x086c_a353;

/// The text of a cell replaced by a copy of `new` (the old one freed).
fn put_text(m: &mut Mem, c: u32, new: u32) {
    if m.u32(c + 8) != 0 {
        let p = m.u32(c + 8);
        m.free(p);
        m.set_u32(c + 8, 0);
    }
    if new == 0 {
        m.set_u32(c + 8, 0);
    } else {
        let p = m.strdup(new);
        m.set_u32(c + 8, p);
    }
}

/// [soft 0x083c8b44 SI_spdSetTextedCell]
pub fn set_texted_cell(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, text: u32) -> i16 {
    let mut r = 0;
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        let kind = m.i32(c);
        if (kind as u32).wrapping_sub(5) > 1 {
            if kind == 0 {
                put_text(m, c, text);
                r = 1;
            } else if kind > -1 && kind < 5 {
                let f = o.call(m, TEXTED_PERFORM_CALC, &[text, (m.i16(c) as i32) as u32, m.u32(c + 4)]).eax;
                if m.u32(c + 8) != 0 {
                    let p = m.u32(c + 8);
                    m.free(p);
                    m.set_u32(c + 8, 0);
                }
                if f == 0 {
                    m.set_u32(c + 8, 0);
                } else {
                    let p = m.strdup(f);
                    m.set_u32(c + 8, p);
                    m.free(f);
                }
            }
            refresh_draw(m, o, sh, row, col);
        }
    }
    r
}

/// [soft 0x083c8cf0 SI_spdSetTextedCellI]
pub fn set_texted_cell_i(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, v: i32) -> i16 {
    let mut r = 0;
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        let kind = m.i32(c);
        if kind == 2 || kind == 4 {
            let buf = m.local(256);
            sprintf_ld(m, buf, v);
            put_text(m, c, buf);
            r = 1;
            refresh_draw(m, o, sh, row, col);
            m.release(256);
        }
    }
    r
}

/// [soft 0x083c8db0 SI_spdGetTextedCellI]
pub fn get_texted_cell_i(m: &mut Mem, sh: u32, row: u32, col: u32, out: u32) -> i16 {
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        let kind = m.i32(c);
        if kind == 2 || kind == 4 {
            let mut p = m.u32(c + 8);
            if p == 0 {
                p = EMPTY_34E;
            }
            let d = m.strdup(p);
            let v = clib::strtol(m, d, 0, 10);
            m.set_u32(out, v as u32);
            if d != 0 {
                m.free(d);
            }
            return 1;
        }
    }
    0
}

/// [soft 0x083c8ed4 SI_spdSetTextedCellF]
pub fn set_texted_cell_f(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, v: f32) -> i16 {
    let mut r = 0;
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        let kind = m.i32(c);
        if kind == 1 || kind == 3 {
            let buf = m.local(256);
            sprintf_lf(m, buf, v as f64);
            put_text(m, c, buf);
            r = 1;
            refresh_draw(m, o, sh, row, col);
            m.release(256);
        }
    }
    r
}

/// [soft 0x083c8f98 SI_spdGetTextedCellF]
pub fn get_texted_cell_f(m: &mut Mem, sh: u32, row: u32, col: u32, out: u32) -> i16 {
    if in_sheet(m, sh, row, col) {
        let c = cell(m, sh, row, col);
        let kind = m.i32(c);
        if kind == 1 || kind == 3 {
            let mut p = m.u32(c + 8);
            if p == 0 {
                p = EMPTY_353;
            }
            let d = m.strdup(p);
            let v = strtod_text(&m.cstr(d)) as f32;
            m.set_f32(out, v);
            if d != 0 {
                m.free(d);
            }
            return 1;
        }
    }
    0
}

fn edit_one(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, v: i16) {
    let c = col.wrapping_mul(CELL).wrapping_add(rowp(m, sh, row));
    let e = if m.u32(c) != 6 { v } else { 0 };
    m.set_u16(c + 0x16, e as u16);
    if v != 0 {
        let c = col.wrapping_mul(CELL).wrapping_add(rowp(m, sh, row));
        m.set_u16(c + 0x18, 1);
    }
    refresh_draw(m, o, sh, row, col);
}

fn valid_one(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: u32, col: u32, v: i16) {
    let c = col.wrapping_mul(CELL).wrapping_add(rowp(m, sh, row));
    m.set_u16(c + 0x18, v as u16);
    if v == 0 {
        let c = col.wrapping_mul(CELL).wrapping_add(rowp(m, sh, row));
        m.set_u16(c + 0x16, 0);
    }
    refresh_draw(m, o, sh, row, col);
}

/// The four-way split of `SI_spdSetCellEditable` and `SI_spdSetCellValid`: a column
/// (`row < 0`), a row (`col < 0`), everything, or one cell.
fn each(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: i32, col: i32, v: i16, one: fn(&mut Mem, &mut dyn Outside, u32, u32, u32, i16)) {
    if row < 0 && col >= 0 {
        let mut r = 0u32;
        if m.u32(sh) == 0 {
            return;
        }
        loop {
            one(m, o, sh, r, col as u32, v);
            r += 1;
            if r >= m.u32(sh) {
                return;
            }
        }
    }
    if row < 0 || col < 0 {
        if row >= 0 {
            let mut c = 0u32;
            if m.u32(sh + 4) == 0 {
                return;
            }
            loop {
                one(m, o, sh, row as u32, c, v);
                c += 1;
                if c >= m.u32(sh + 4) {
                    return;
                }
            }
        }
        // both negative
        let mut r = 0u32;
        if m.u32(sh) == 0 {
            return;
        }
        loop {
            if m.u32(sh + 4) != 0 {
                let mut c = 0u32;
                loop {
                    one(m, o, sh, r, c, v);
                    c += 1;
                    if c >= m.u32(sh + 4) {
                        break;
                    }
                }
            }
            r += 1;
            if r >= m.u32(sh) {
                return;
            }
        }
    }
    one(m, o, sh, row as u32, col as u32, v);
}

/// [soft 0x083c9854 SI_spdSetCellEditable]
pub fn set_cell_editable(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: i32, col: i32, v: i16) {
    each(m, o, sh, row, col, v, edit_one);
}

/// [soft 0x083c9a74 SI_spdSetCellValid]
pub fn set_cell_valid(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: i32, col: i32, v: i16) {
    each(m, o, sh, row, col, v, valid_one);
}

/// The selected cells' list emptied and the sheet's selection counters cleared
/// [soft 0x083c9ea4 clearAllSelection].
pub fn clear_all_selection(m: &mut Mem, o: &mut dyn Outside, sh: u32) {
    let mut p = m.u32(sh + 0x18);
    if p != 0 && m.u32(sh + 0x34) != 0 {
        let mut n = 0u16;
        let mut node = m.u32(sh + 0x34);
        loop {
            if p == 0 {
                break;
            }
            let c = m.u32(node);
            let f = m.u8(c + 0x14) & 0xfe;
            m.set_u8(c + 0x14, f);
            let c = m.u32(node);
            let (row, col) = (m.u32(c + 0xc), m.u32(c + 0x10));
            p = m.u32(node + 4);
            if p == 0 {
                if node != 0 {
                    m.free(node);
                }
                p = 0;
            } else if m.u32(p + 8) != 0 {
                let q = m.u32(p + 8);
                m.free(q);
                m.set_u32(p + 8, 0);
            }
            o.call(m, DRAW_CELL, &[sh, row, col]);
            n = n.wrapping_add(1);
            node = p;
            if !((n as u32) < m.u32(sh + 0x18)) {
                break;
            }
        }
        m.set_u32(sh + 0x18, 0);
        m.set_u32(sh + 0x30, 0);
        m.set_u32(sh + 0x34, 0);
        m.set_u32(sh + 0x38, 0);
    }
}

/// One cell selected (`on == 1`, when it is editable and not yet selected) or deselected
/// (`on == 0`), with its select or deselect callback [soft 0x083c9f68 cellSetSelectFlag].
pub fn cell_set_select_flag(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: i32, col: i32, on: i16) {
    m.set_u32(sh + 0x3c, 0);
    let (ru, cu) = (row as u32, col as u32);
    let c = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
    let cb;
    if on == 1 && (m.u16(c + 0x14) as u8 ^ 1) & 1 != 0 && m.i16(c + 0x16) == 1 {
        let node = m.si_calloc(1, 0xc);
        let cellp = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
        m.set_u32(node, cellp);
        let tail = m.u32(sh + 0x38);
        m.set_u32(node + 8, tail);
        m.set_u32(node + 4, 0);
        if m.u32(sh + 0x18) == 0 {
            m.set_u32(sh + 0x38, node);
            m.set_u32(sh + 0x34, node);
        } else {
            let t = m.u32(sh + 0x38);
            m.set_u32(t + 4, node);
            m.set_u32(sh + 0x38, node);
        }
        let c1 = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
        let f = m.u8(c1 + 0x14) | 1;
        m.set_u8(c1 + 0x14, f);
        let c2 = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
        let f = m.u8(c2 + 0x14) | 4;
        m.set_u8(c2 + 0x14, f);
        let n = m.u32(sh + 0x18).wrapping_add(1);
        m.set_u32(sh + 0x18, n);
        let first = m.u32(node);
        m.set_u32(sh + 0x30, first);
        let c3 = rowp(m, sh, ru);
        cb = m.u32(c3.wrapping_add(cu.wrapping_mul(CELL)).wrapping_add(0x20));
    } else {
        if on != 0 {
            o.call(m, DRAW_APP_DIRTY_CELLS, &[sh]);
            return;
        }
        let c = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
        let f = m.u16(c + 0x14);
        if f & 1 == 0 {
            o.call(m, DRAW_APP_DIRTY_CELLS, &[sh]);
            return;
        }
        let mut found = false;
        m.set_u16(c + 0x14, f & 0xfffe);
        let mut i = 0u32;
        let c = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
        let f = m.u8(c + 0x14) | 4;
        m.set_u8(c + 0x14, f);
        let mut node = m.u32(sh + 0x38);
        if m.u32(sh + 0x18) != 0 {
            loop {
                let target = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
                if m.u32(node) == target {
                    found = true;
                    let (prev, next) = (m.u32(node + 8), m.u32(node + 4));
                    if prev == 0 {
                        m.set_u32(sh + 0x34, next);
                    } else {
                        m.set_u32(prev + 4, next);
                    }
                    if next == 0 {
                        m.set_u32(sh + 0x38, prev);
                    } else {
                        m.set_u32(next + 8, prev);
                    }
                    if node != 0 {
                        m.free(node);
                        node = 0;
                    }
                    let n = m.u32(sh + 0x18).wrapping_sub(1);
                    m.set_u32(sh + 0x18, n);
                } else {
                    node = m.u32(node + 8);
                }
                i += 1;
                if !(i < m.u32(sh + 0x18) && !found) {
                    break;
                }
            }
        }
        let here = cu.wrapping_mul(CELL).wrapping_add(rowp(m, sh, ru));
        if m.u32(sh + 0x30) == here {
            let tail = m.u32(sh + 0x38);
            if m.u32(sh + 0x18) == 0 || tail == 0 {
                m.set_u32(sh + 0x30, 0);
            } else {
                let first = m.u32(tail);
                m.set_u32(sh + 0x30, first);
            }
        }
        let r = rowp(m, sh, ru);
        cb = m.u32(r.wrapping_add(cu.wrapping_mul(CELL)).wrapping_add(0x28));
    }
    if cb != 0 {
        o.call(m, Routine::at(cb), &[sh, ru, cu]);
    }
    o.call(m, DRAW_APP_DIRTY_CELLS, &[sh]);
}

/// [soft 0x083c8a58 SI_spdSetSelectionFlag]
pub fn set_selection_flag(m: &mut Mem, o: &mut dyn Outside, sh: u32, row: i32, col: i32, on: i16, keep: i16) -> i16 {
    let mut r = 1;
    if (row < 0 && col < 0) || m.u32(sh) < row as u32 || m.u32(sh + 4) < col as u32 {
        r = 0;
    } else {
        if keep == 0 {
            clear_all_selection(m, o, sh);
        }
        if row < 0 || col < 0 {
            if row < 0 && col > -1 {
                let mut i = 0u32;
                if m.u32(sh) != 0 {
                    loop {
                        cell_set_select_flag(m, o, sh, i as i32, col, on);
                        i += 1;
                        if i >= m.u32(sh) {
                            break;
                        }
                    }
                }
            } else if col < 0 && row > -1 {
                let mut i = 0u32;
                if m.u32(sh + 4) != 0 {
                    loop {
                        cell_set_select_flag(m, o, sh, row, i as i32, on);
                        i += 1;
                        if i >= m.u32(sh + 4) {
                            break;
                        }
                    }
                }
            } else {
                r = 0;
            }
        } else {
            cell_set_select_flag(m, o, sh, row, col, on);
        }
    }
    r
}

/// A row of cells made for one row of a new sheet; 0 when a text could not be copied
/// [soft 0x083c94c8 spdAllocateRow].
pub fn spd_allocate_row(m: &mut Mem, row: u32, cols: u32, types: u32, strs: u32) -> u32 {
    let base = m.si_calloc(cols, CELL);
    if base == 0 {
        return 0;
    }
    let (mut c, mut t, mut s) = (base, types, strs);
    let mut i = 0u32;
    if cols != 0 {
        loop {
            m.set_u32(c + 0xc, row);
            m.set_u32(c + 0x10, i);
            let kind = m.u32(t);
            m.set_u32(c, kind);
            m.set_u16(c + 0x16, (kind != 6) as u16);
            m.set_u16(c + 0x18, 1);
            if m.u32(t) != 5 {
                if m.u32(t) == 6 {
                    m.set_u32(c + 4, 0xFFFF_FFFF);
                } else if m.u32(s) != 0 {
                    let src = m.u32(s);
                    let p = m.strdup(src);
                    m.set_u32(c + 4, p);
                    if p == 0 {
                        return 0;
                    }
                }
            }
            i += 1;
            c = c.wrapping_add(CELL);
            t = t.wrapping_add(4);
            s = s.wrapping_add(4);
            if i >= cols {
                break;
            }
        }
    }
    base
}

/// One row of cells taken apart and freed; the slot cleared [soft 0x083c96ec spdDisposeRow].
pub fn spd_dispose_row(m: &mut Mem, o: &mut dyn Outside, pp: u32, cols: u32) {
    let base = m.u32(pp);
    if base != 0 {
        let mut c = base;
        let mut i = 0u32;
        if cols != 0 {
            loop {
                if m.u32(c).wrapping_sub(5) > 1 {
                    free_text_words(m, c);
                }
                if m.u32(c + 0x1c) != 0 && m.u32(c + 0x30) != 0 {
                    let (f, d) = (m.u32(c + 0x30), m.u32(c + 0x1c));
                    o.call(m, Routine::at(f), &[d]);
                }
                i += 1;
                c = c.wrapping_add(CELL);
                if i >= cols {
                    break;
                }
            }
        }
        m.free(base);
        m.set_u32(pp, 0);
    }
}

/// A sheet taken apart; `pp` is the address of the sheet pointer, cleared
/// [soft 0x083c956c spdDispose].
pub fn spd_dispose(m: &mut Mem, o: &mut dyn Outside, pp: u32) {
    let sh = m.u32(pp);
    if sh == 0 {
        return;
    }
    let mut p = m.u32(sh + 0x1c);
    if p != 0 {
        let total = (m.u16(sh + 0x26) as u32).wrapping_mul(m.u32(sh + 4));
        let mut i = 0u32;
        if total != 0 {
            loop {
                let q = m.u32(m.u32(sh + 0x1c).wrapping_add(i.wrapping_mul(4)));
                if q != 0 {
                    m.free(q);
                    let slot = m.u32(sh + 0x1c).wrapping_add(i.wrapping_mul(4));
                    m.set_u32(slot, 0);
                }
                p = m.u32(sh + 0x1c);
                i += 1;
                if i >= (m.u16(sh + 0x26) as u32).wrapping_mul(m.u32(sh + 4)) {
                    break;
                }
            }
        }
        if p != 0 {
            m.free(p);
            m.set_u32(sh + 0x1c, 0);
        }
    }
    if m.u32(sh + 0x6c) != 0 {
        if m.u32(sh + 0x70) != 0 && m.u32(sh + 4) != 0 {
            let mut i = 0u32;
            loop {
                let (f, a) = (m.u32(sh + 0x70), m.u32(sh + 0x6c).wrapping_add(i.wrapping_mul(4)));
                o.call(m, Routine::at(f), &[a]);
                i += 1;
                if i >= m.u32(sh + 4) {
                    break;
                }
            }
        }
        if m.u32(sh + 0x6c) != 0 {
            let p = m.u32(sh + 0x6c);
            m.free(p);
            m.set_u32(sh + 0x6c, 0);
        }
    }
    if m.u32(sh + 0x20) != 0 {
        let p = m.u32(sh + 0x20);
        m.free(p);
        m.set_u32(sh + 0x20, 0);
    }
    if m.u32(sh + 0x5c) != 0 {
        let p = m.u32(sh + 0x5c);
        m.free(p);
        m.set_u32(sh + 0x5c, 0);
    }
    if m.u32(sh + 0x2c) != 0 {
        if m.u32(sh) != 0 {
            let mut i = 0u32;
            loop {
                let slot = m.u32(sh + 0x2c).wrapping_add(i.wrapping_mul(4));
                let cols = m.u32(sh + 4);
                spd_dispose_row(m, o, slot, cols);
                i += 1;
                if i >= m.u32(sh) {
                    break;
                }
            }
        }
        if m.u32(sh + 0x2c) != 0 {
            let p = m.u32(sh + 0x2c);
            m.free(p);
            m.set_u32(sh + 0x2c, 0);
        }
    }
    if m.u32(sh + 0x74) != 0 && m.u32(sh + 0x78) != 0 {
        let (f, d) = (m.u32(sh + 0x78), m.u32(sh + 0x74));
        o.call(m, Routine::at(f), &[d]);
        m.set_u32(sh + 0x74, 0);
        m.set_u32(sh + 0x78, 0);
    }
    m.free(sh);
    m.set_u32(pp, 0);
}

/// [soft 0x083c86dc SI_spdDelete]
pub fn spd_delete(m: &mut Mem, o: &mut dyn Outside, sh: u32) {
    let pp = m.local(4);
    m.set_u32(pp, sh);
    spd_dispose(m, o, pp);
    m.release(4);
}

/// A new sheet of `rows` by `cols` cells (the types are an array of 32-bit words, one a column); 0 when an allocation failed
/// [soft 0x083c9384 spdAllocate].
pub fn spd_allocate(m: &mut Mem, o: &mut dyn Outside, rows: u32, cols: u32, hdr: u16, types: u32, strs: u32) -> u32 {
    let sh = m.si_calloc(1, 0x7c);
    if sh == 0 {
        return 0;
    }
    m.set_u32(sh, rows);
    m.set_u32(sh + 4, cols);
    m.set_u16(sh + 0x26, hdr);
    let a = m.si_calloc((hdr as u32).wrapping_mul(cols), 4);
    m.set_u32(sh + 0x1c, a);
    if a != 0 {
        let b = m.si_calloc(cols, 2);
        m.set_u32(sh + 0x20, b);
        if b != 0 {
            let c = m.si_calloc(cols, 4);
            m.set_u32(sh + 0x6c, c);
            if c != 0 {
                if rows == 0 {
                    return sh;
                }
                let r5c = m.si_calloc(rows, 4);
                m.set_u32(sh + 0x5c, r5c);
                let r2c = m.si_calloc(rows, 4);
                m.set_u32(sh + 0x2c, r2c);
                if m.u32(sh + 0x2c) != 0 && m.u32(sh + 0x5c) != 0 {
                    let mut i = 0u32;
                    loop {
                        let r = spd_allocate_row(m, i, cols, types, strs);
                        let slot = m.u32(sh + 0x2c).wrapping_add(i.wrapping_mul(4));
                        m.set_u32(slot, r);
                        if r == 0 {
                            break;
                        }
                        i += 1;
                        if rows <= i {
                            return sh;
                        }
                    }
                }
            }
        }
    }
    let pp = m.local(4);
    m.set_u32(pp, sh);
    spd_dispose(m, o, pp);
    m.release(4);
    0
}

/// A sheet built and its columns measured; `err` gets -1 when it could not be made
/// [soft 0x083c8494 SI_spdCreate]. Arguments in the original's order: the four view box
/// words, rows, columns, header lines, header texts, cell types, minimum widths in
/// characters, cell texts, column data, a flag, the callback and the error word.
#[allow(clippy::too_many_arguments)]
pub fn spd_create(
    m: &mut Mem, o: &mut dyn Outside, a1: i32, a2: i32, a3: i32, a4: i32, rows: u32, cols: u32, hdr: u16, mut heads: u32, types: u32,
    mut minw: u32, texts: u32, coldata: u32, flag: i16, cbp: u32, err: u32,
) -> u32 {
    let sh = spd_allocate(m, o, rows, cols, hdr, types, texts);
    if sh == 0 {
        m.set_i32(err, -1);
        return 0;
    }
    let saved = o.call(m, GETFONT, &[]).eax;
    let appf = m.i16(SI_APPF) as i32 as u32;
    o.call(m, FONT, &[appf]);
    m.set_u32(sh + 0x50, 0);
    m.set_u32(sh + 0x4c, 0);
    let w = o.call(m, STRWIDTH, &[0x086c_a345]).eax as i32;
    let mut c = 0u32;
    let nh = hdr as u32 as i32;
    if cols != 0 {
        loop {
            let mut width = (m.u16(minw) as i32).wrapping_mul(w);
            minw = minw.wrapping_add(2);
            let mut j = 0i32;
            if nh > 0 {
                loop {
                    if m.u32(heads) != 0 {
                        let src = m.u32(heads);
                        let p = m.strdup(src);
                        let slot = m.u32(sh + 0x1c).wrapping_add(c.wrapping_mul(hdr as u32).wrapping_add(j as u32).wrapping_mul(4));
                        m.set_u32(slot, p);
                        let s1 = o.call(m, STRWIDTH, &[src]).eax as i32;
                        if !(width > s1) {
                            width = o.call(m, STRWIDTH, &[src]).eax as i32;
                        }
                    }
                    j += 1;
                    heads = heads.wrapping_add(4);
                    if j >= nh {
                        break;
                    }
                }
            }
            let ws = m.u32(sh + 0x20).wrapping_add(c.wrapping_mul(2));
            m.set_u16(ws, (width.wrapping_add(8)) as u16);
            let wv = m.u16(m.u32(sh + 0x20).wrapping_add(c.wrapping_mul(2))) as u32;
            let t = m.u32(sh + 0x4c).wrapping_add(wv);
            m.set_u32(sh + 0x4c, t);
            c += 1;
            if c >= cols {
                break;
            }
        }
    }
    c = 0;
    while c < cols {
        let v = m.u32(coldata.wrapping_add(c * 4));
        let slot = m.u32(sh + 0x6c).wrapping_add(c * 4);
        m.set_u32(slot, v);
        c += 1;
    }
    m.set_u32(sh + 0x78, 0);
    m.set_u32(sh + 0x74, 0);
    m.set_u16(sh + 0x40, a1 as u16);
    m.set_u16(sh + 0x46, a2 as u16);
    m.set_u16(sh + 0x42, a3 as u16);
    m.set_u16(sh + 0x44, a4 as u16);
    let v24: u16 = if m.u32(sh) == 0 {
        0
    } else {
        // x87: log10 of the unsigned row count (stored as a double), then truncated: the
        // rounding control is set to "toward zero" for the conversion
        let l = si_x87::trig::log10(m.u32(sh) as f64).to_f64();
        let sw = o.call(m, STRWIDTH, &[0x086c_a347]).eax as i32;
        ((l as i32).wrapping_add(1).wrapping_mul(sw).wrapping_add(8)) as u16
    };
    m.set_u16(sh + 0x24, v24);
    let h = o.call(m, GETHEIGHT, &[]).eax as i32;
    m.set_u16(sh + 0x28, (h.wrapping_add(8).wrapping_mul(hdr as i32).wrapping_add(8)) as u16);
    m.set_u16(sh + 0x48, 1);
    if flag == 0 {
        m.set_u32(sh + 0x58, cbp);
    } else {
        m.set_u32(sh + 0x54, cbp);
    }
    o.call(m, FONT, &[saved as i16 as i32 as u32]);
    sh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_flags_on_made_up_sheet() {
        let mut m = Mem::new();
        let sh = m.si_calloc(1, 0x80);
        let rows = m.si_calloc(1, 4);
        let row = m.si_calloc(2, CELL);
        m.set_u32(sh, 1);
        m.set_u32(sh + 4, 2);
        m.set_u32(sh + 0x2c, rows);
        m.set_u32(rows, row);
        m.set_u8(row + CELL + 0x14, 3);
        assert_eq!((get_nb_rows(&m, sh), get_nb_cols(&m, sh), get_nb_rows(&m, 0)), (1, 2, 0));
        assert_eq!(get_selection_flag(&m, sh, 0, 1), 1);
        assert_eq!(get_selection_flag(&m, sh, 1, 0), 0);
        set_cell_app_dirty_flag(&mut m, sh, 0, 0);
        assert_eq!(m.u8(row + 0x14), 4);
    }
}
