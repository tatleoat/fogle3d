// SPDX-License-Identifier: MIT
//! Animation files (`.ani`, magic 0x5380f635) and struct key files (`.key`, magic
//! 0x5380f646): finding 0005.
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: the element dictionary's files (item headers, the
//! functions that open the file), the hierarchy animation's file (`si_hieranimBinaryRead`
//! and its helpers), `utilio.c` (the action lists) and the actions' own file.
//!
//! Both kinds of file hold a *hierarchy animation*: the names of the elements of one
//! hierarchy (a model and its descendants, say), then one item per animated attribute, each
//! owned by one of those elements. An item of an animation file is a function curve; an item
//! of a struct key file is a single value. An animation file may end with two lists of
//! *actions* (named spans of time).
//!
//! Field names: the keywords the vendor's converter `bin2ascii` prints
//! (`si_elemdictWriteAscAnimations`, read from its disassembly); the action fields are named
//! after the program's own accessor functions (`SI_actionsElemSetStart`, …).

use crate::fcurve::{convert_angle_unit_to_degree, read_fcurve, read_static_val, write_fcurve, write_static_val, Fcurve, StaticVal};
use crate::io::{fread_i16, fread_u16, End, Reader, Writer};
use si_file::{write_db_header, MAGIC_ANIMATION, MAGIC_STRUCT_KEYS};

/// What an item holds (`ITM_TYPE`).
#[derive(Clone, Debug, PartialEq)]
pub enum ItemData {
    /// Type 0: a function curve.
    Curve(Fcurve),
    /// Type 1: a static value.
    Static(StaticVal),
    /// Any other type: the reader reads nothing and stores a null pointer.
    None,
}

/// One item of an element: an entry in each of the element's six parallel arrays.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// +0x0c[] `ITM_TYPE`: 0 function curve, 1 static value
    pub kind: i16,
    /// +0x10[] `ACT_RELID`: −1 unless the item belongs to an action relation
    pub act_rel_id: i32,
    /// +0x14[] `ACT_OCCID`
    pub act_occ_id: i16,
    /// +0x1c[]: set to 1 when the item is added
    pub w_1c: i16,
    /// +0x20[]: the item header's token 7; 1 unless the file says otherwise (the scene-side
    /// writer takes it from the relation that ties the curve to its element)
    pub w_20: i16,
    /// +0x18[]
    pub data: ItemData,
}

/// One element of the hierarchy (40 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct HierElem {
    /// +0x00 `HIER_NAME`: the element's name in the database
    pub name: Vec<u8>,
    /// +0x04 `HIER_FTH`: the index of the element's parent in this list, −1 for the root
    /// (16 bits in the file)
    pub father: i32,
    /// +0x08 count and the arrays
    pub items: Vec<Item>,
}

/// The record an action element points to when it stands for a path animation
/// (`SI_PathAnimActs`, 28 bytes). The four strings and the curve are named by their file
/// tokens; their meaning has not been read.
#[derive(Clone, Debug, PartialEq)]
pub struct PathAnimActs {
    /// +0x00, +0x04: set to −1 when the record is made
    pub d_00: i32,
    pub d_04: i32,
    /// +0x08: token 0x1a
    pub fcurve: Option<Fcurve>,
    /// +0x0c: token 0x17. The writer saves the record only if this string exists.
    pub s_0c: Option<Vec<u8>>,
    /// +0x10: token 0x16
    pub s_10: Option<Vec<u8>>,
    /// +0x14: token 0x19
    pub s_14: Option<Vec<u8>>,
    /// +0x18: token 0x18
    pub s_18: Option<Vec<u8>>,
}

impl PathAnimActs {
    fn new() -> Self {
        PathAnimActs { d_00: -1, d_04: -1, fcurve: None, s_0c: None, s_10: None, s_14: None, s_18: None }
    }
}

/// One element of an action list (`SI_ActionsElem`, 48 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionElem {
    /// +0x00 the action's name
    pub name: Vec<u8>,
    /// +0x04 a second name (token 0x1b): the program looks it up in the second list
    pub source: Option<Vec<u8>>,
    /// +0x08 start, +0x0c end, in seconds. In memory they are half a frame wider than in
    /// the file on each side.
    pub start: f32,
    pub end: f32,
    /// +0x10, +0x14: tokens 0x1c and 0x1d (the fields `SI_actionsElemSetClipStart` and
    /// `SI_actionsElemSetClipEnd` work on)
    pub clip_start: f32,
    pub clip_end: f32,
    /// +0x18 flags: bit 1 selected, bit 2 set by activation, bit 8
    pub flags: i32,
    /// +0x1c, +0x20: zeroed when the element is made
    pub d_1c: i32,
    pub d_20: i32,
    /// +0x24 the number of cycles the span holds (`SI_actionsElemGetLength` divides by it)
    pub cycles: i32,
    /// +0x28
    pub path: Option<PathAnimActs>,
    /// +0x2c an interpolation code (`SI_actionsElemSetInterp`)
    pub interp: i16,
}

/// A hierarchy animation (`SI_HierAnim`, 28 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HierAnim {
    /// +0x08 is the index of an element list (a chapter) of the element dictionary. The file
    /// names the list (`HIER_CHAPNAME`, for instance `MODELS`); the reader looks the name up
    /// (`SI_elemdictFindListByName`, not ported: it needs the dictionary). `None`: the file
    /// has no name and the reader takes its default list (the global at 0x08781680).
    pub chapter: Option<Vec<u8>>,
    /// +0x00 count, +0x04 elements
    pub elems: Vec<HierElem>,
    /// +0x0c the first action list
    pub actions: Vec<ActionElem>,
    /// +0x14 the second action list
    pub sources: Vec<ActionElem>,
}

impl HierAnim {
    /// An empty hierarchy animation [soft 0x08138b28 SI_hieranimAllocate]
    /// [soft 0x08138aec SI_hieranimInit] [soft 0x081df2f8 SI_actionsInit].
    pub fn new() -> HierAnim {
        HierAnim::default()
    }

    /// Appends an element and returns its index [soft 0x08138cc4 SI_hieranimAddElem].
    pub fn add_elem(&mut self, name: &[u8], father: i32) -> usize {
        self.elems.push(HierElem { name: name.to_vec(), father, items: Vec::new() });
        self.elems.len() - 1
    }

    /// Appends an item to an element [soft 0x08138e0c SI_hieranimAddItem]. An owner index
    /// outside the list drops the item (the original leaks its data) and returns false.
    pub fn add_item(&mut self, owner: i32, kind: i16, act_rel_id: i32, act_occ_id: i16, w_20: i16, data: ItemData) -> bool {
        if owner < 0 || owner as usize >= self.elems.len() {
            return false;
        }
        self.elems[owner as usize].items.push(Item { kind, act_rel_id, act_occ_id, w_1c: 1, w_20, data });
        true
    }
}

/// Reads the element list that opens the file [soft 0x08139094 si_hieranimBinaryRead].
///
/// | token | field |
/// |---|---|
/// | 3 | string: `HIER_CHAPNAME`. Only as the very first token |
/// | 1 | string: `HIER_NAME` of the next element |
/// | 2 | 16-bit: `HIER_FTH`; adds the element with the name read last |
/// | 0 | end of the list |
/// | other | ignored; nothing is read for it |
///
/// This file has its own token and 16-bit variables. A token 2 with no name before it would
/// pass a null pointer to `strdup` in the original; here the element gets an empty name.
pub fn read_hieranim(r: &mut Reader, ha: &mut HierAnim) -> End {
    let mut ok = fread_u16(&mut r.s, &mut r.st.token_hieranim);
    r.note("hierarchy", r.st.token_hieranim, ok);
    let mut chapter = None;
    if r.st.token_hieranim == 3 {
        chapter = Some(r.string());
        ok = fread_u16(&mut r.s, &mut r.st.token_hieranim);
        r.note("hierarchy", r.st.token_hieranim, ok);
    }
    *ha = HierAnim { chapter, ..HierAnim::new() };
    let mut name: Option<Vec<u8>> = None;
    loop {
        let token = r.st.token_hieranim;
        if token == 0 {
            return End::Zero;
        }
        if !ok {
            return End::Eof;
        }
        if token == 1 {
            name = Some(r.string());
        } else if token == 2 {
            fread_i16(&mut r.s, &mut r.st.svalue_hieranim);
            ha.add_elem(&name.take().unwrap_or_default(), r.st.svalue_hieranim as i32);
        }
        ok = fread_u16(&mut r.s, &mut r.st.token_hieranim);
        r.note("hierarchy", r.st.token_hieranim, ok);
    }
}

/// Writes the element list [soft 0x08138fbc si_hieranimBinaryWrite]: token 3 and the
/// chapter's name, then per element token 1 with the name and token 2 with the low 16 bits
/// of the father index, and the zero token.
pub fn write_hieranim(w: &mut Writer, chapter: Option<&[u8]>, ha: &HierAnim) {
    let tok = |w: &mut Writer, t: u16| {
        w.st.token_hieranim = t;
        w.put_u16(t);
    };
    tok(w, 3);
    w.string(chapter);
    for e in &ha.elems {
        tok(w, 1);
        w.string(Some(&e.name));
        tok(w, 2);
        w.st.svalue_hieranim = e.father as i16;
        w.put_u16(e.father as u16);
    }
    tok(w, 0);
}

/// What one call of the item reader gave.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemRead {
    /// False: no item (the file's items are over). The caller stops.
    pub found: bool,
    /// `OWNER_ID`: index of the owning element
    pub owner: i32,
    /// `ITM_TYPE`
    pub kind: i32,
    /// `ACT_RELID` (−1 if the header has none)
    pub act_rel_id: i32,
    /// `ACT_OCCID`, if the header has one (the original leaves the caller's variable alone
    /// otherwise)
    pub act_occ_id: Option<i16>,
    /// Token 7 (1 if the header has none)
    pub w_20: i16,
    pub data: ItemData,
    /// How the item's data ended, if any was read.
    pub end: End,
}

/// Reads one item: its header, then its curve or value
/// [soft 0x08110284 SI_elemdictReadAnimation].
///
/// The header, up to its zero token:
///
/// | token | field |
/// |---|---|
/// | 1 | 16-bit: `OWNER_ID`. An item must have it |
/// | 2 | string, read and dropped |
/// | 3 | 16-bit, read and dropped |
/// | 4 | 16-bit: `ITM_TYPE` |
/// | 5 | 16-bit: `ACT_RELID` |
/// | 6 | 16-bit: `ACT_OCCID` |
/// | 7 | 16-bit: stored with the item (+0x20[]) |
/// | 0, 10, 99 | end of the header. 10 opens the first action list, 99 ends a piped file: with either as the first token there is no item |
/// | other | ignored; nothing is read for it |
///
/// The token variable is set to zero before each read after the first, so the end of the
/// file ends the header like a zero token. After a header with an owner comes, for type 0,
/// a function curve [`read_fcurve`], for type 1 a static value [`read_static_val`], for any
/// other type nothing.
///
/// `degrees`: the file's header was not accepted (an old, header-less file); angle curves
/// are then converted from radians. `default_interpolation`: see [`Fcurve::blank`].
pub fn read_item(r: &mut Reader, degrees: bool, default_interpolation: i16) -> ItemRead {
    let mut it = ItemRead { found: false, owner: 0, kind: 0, act_rel_id: -1, act_occ_id: None, w_20: 1, data: ItemData::None, end: End::Zero };
    let ok = fread_u16(&mut r.s, &mut r.st.token_elemdict);
    r.note("item", r.st.token_elemdict, ok);
    loop {
        let token = r.st.token_elemdict;
        if token == 0 || token == 99 || token == 10 {
            break;
        }
        match token {
            1 => {
                it.owner = r.svalue() as i32;
                it.found = true;
            }
            2 => {
                r.string();
            }
            3 => {
                r.svalue();
            }
            4 => it.kind = r.svalue() as i32,
            5 => it.act_rel_id = r.svalue() as i32,
            6 => it.act_occ_id = Some(r.svalue()),
            7 => it.w_20 = r.svalue(),
            _ => {}
        }
        r.st.token_elemdict = 0;
        let ok = fread_u16(&mut r.s, &mut r.st.token_elemdict);
        r.note("item", r.st.token_elemdict, ok);
    }
    if it.found {
        if it.kind == 1 {
            let (v, end) = read_static_val(r);
            it.data = ItemData::Static(v);
            it.end = end;
        } else if it.kind == 0 {
            let (c, end) = read_fcurve(r, true, default_interpolation);
            let mut c = c.expect("kept");
            if degrees {
                convert_angle_unit_to_degree(&mut c);
            }
            it.data = ItemData::Curve(c);
            it.end = end;
        }
    }
    it
}

/// Writes one item: its header, then its curve or value
/// [soft 0x08110a94 SI_elemdictWriteAnimation].
///
/// The header is token 1 with the owner's index; if the output version is above 2.62 (the
/// double at 0x08649b70) token 4 with the type, tokens 5 and 6 with the two action numbers
/// if the first is not negative, and token 7; then the zero token. For type 1 a static value
/// follows, for type 0 a function curve, for any other type nothing.
///
/// Data that does not match the type (the original casts one pointer) is written as nothing.
pub fn write_item(w: &mut Writer, data: &ItemData, owner: i32, kind: i32, act_rel_id: i32, act_occ_id: i32, w_20: i16) {
    let tok = |w: &mut Writer, t: u16| {
        w.st.token_elemdict = t;
        w.put_u16(t);
    };
    tok(w, 1);
    w.svalue(owner as i16);
    if w.version_output as f64 > 2.62 {
        tok(w, 4);
        w.svalue(kind as i16);
        if act_rel_id >= 0 {
            tok(w, 5);
            w.svalue(act_rel_id as i16);
            tok(w, 6);
            w.svalue(act_occ_id as i16);
        }
        tok(w, 7);
        w.svalue(w_20);
    }
    tok(w, 0);
    match (kind, data) {
        (1, ItemData::Static(v)) => write_static_val(w, Some(v)),
        (1, _) => write_static_val(w, None),
        (0, ItemData::Curve(c)) => write_fcurve(w, c),
        _ => {}
    }
}

/// Half a frame in seconds, as the action readers and writers compute it: zero if the frame
/// rate is zero, else 0.5 divided by the rate. (On the x87 the quotient is rounded to 64
/// bits and then stored as a single; a quotient of two singles cannot come close enough to a
/// rounding boundary for that to differ from dividing in single precision.)
fn half_frame(frame_rate: f32) -> f32 {
    if frame_rate == 0.0 {
        0.0
    } else {
        0.5 / frame_rate
    }
}

/// Which list currently holds the selection, as far as one file's reading can tell (the
/// program's global at 0x0878fb10 points at one action list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selected {
    /// Some list outside this file, or none.
    Elsewhere,
    Actions,
    Sources,
}

/// What the element reader collects before the list reader adds the element.
struct ElemRead {
    found: bool,
    name: Option<Vec<u8>>,
    source: Option<Vec<u8>>,
    start: f32,
    end: f32,
    clip_start: f32,
    clip_end: f32,
    cycles: i32,
    flag_10: bool,
    interp: i16,
    path: Option<PathAnimActs>,
    selected: bool,
}

/// Reads the fields of one action element, starting from the token already in the token
/// variable [soft 0x083ff614 SI_actionsElemBinaryRead].
///
/// | token | field |
/// |---|---|
/// | 0x0c | string: the name. An element must have it |
/// | 0x0d | 32-bit integer: the start in frames; divided by the frame rate (30 if the scene's rate is zero) |
/// | 0x0e | 32-bit integer: the end in frames, likewise |
/// | 0x0f | (no field) the element is selected |
/// | 0x10 | (no field) the flag word becomes 2 instead of 10 |
/// | 0x11 | end of the element |
/// | 0x12 | float: the start in seconds |
/// | 0x13 | float: the end in seconds |
/// | 0x14 | 32-bit: the number of cycles; a zero is replaced by 1 |
/// | 0x15 | 16-bit: interpolation code (5 if absent) |
/// | 0x16, 0x17, 0x18, 0x19 | string each: the path animation record's +0x10, +0x0c, +0x18, +0x14 |
/// | 0x1a | function curve: the path animation record's curve |
/// | 0x1b | string: the second name |
/// | 0x1c, 0x1d | float each: clip start and end |
/// | other | ignored; nothing is read for it |
///
/// Before each read after the first the token variable is set to 0x11, so the end of the
/// file ends the element.
fn read_action_elem(r: &mut Reader, default_interpolation: i16) -> ElemRead {
    let mut e = ElemRead {
        found: false,
        name: None,
        source: None,
        start: 0.0,
        end: 0.0,
        clip_start: 0.0,
        clip_end: 0.0,
        cycles: 1,
        flag_10: false,
        interp: 5,
        path: None,
        selected: false,
    };
    let mut path = PathAnimActs::new();
    let mut used = false;
    let rate = if r.st.frame_rate != 0.0 { r.st.frame_rate } else { 30.0 };
    loop {
        let token = r.st.token_utilio;
        if token == 0x11 {
            break;
        }
        match token {
            0x0c => {
                e.name = Some(r.string());
                e.found = true;
            }
            // an integer divided by a single on the x87, stored as a single: the same bits as
            // the division in double precision narrowed (see the finding)
            0x0d => e.start = (r.ivalue() as f64 / rate as f64) as f32,
            0x0e => e.end = (r.ivalue() as f64 / rate as f64) as f32,
            0x0f => e.selected = true,
            0x10 => e.flag_10 = true,
            0x12 => e.start = r.fvalue(),
            0x13 => e.end = r.fvalue(),
            0x14 => {
                e.cycles = r.ivalue();
                if e.cycles == 0 {
                    e.cycles = 1;
                }
            }
            0x15 => e.interp = r.svalue(),
            0x16 => {
                used = true;
                path.s_10 = Some(r.string());
            }
            0x17 => {
                used = true;
                path.s_0c = Some(r.string());
            }
            0x18 => {
                used = true;
                path.s_18 = Some(r.string());
            }
            0x19 => {
                used = true;
                path.s_14 = Some(r.string());
            }
            0x1a => {
                used = true;
                let (c, _) = read_fcurve(r, true, default_interpolation);
                path.fcurve = c;
            }
            0x1b => e.source = Some(r.string()),
            0x1c => e.clip_start = r.fvalue(),
            0x1d => e.clip_end = r.fvalue(),
            _ => {}
        }
        r.st.token_utilio = 0x11;
        let ok = fread_u16(&mut r.s, &mut r.st.token_utilio);
        r.note("action", r.st.token_utilio, ok);
    }
    if used {
        e.path = Some(path);
    }
    e
}

/// Adds an element to a list at its place in time [soft 0x081df3e4 SI_actionsAddElemTime]
/// and returns its index, or `None` if the start is after the end or the cycle count is not
/// positive. The place is before the first element whose start, plus 0.0001, is greater
/// than the new element's *end*. `interp` is the program's default interpolation
/// (`SI_glbAnim` + 0x30). The original makes up a name if given none; that case does not
/// arise from a file and is not offered.
pub fn actions_add_elem_time(list: &mut Vec<ActionElem>, name: &[u8], start: f32, end: f32, cycles: i32, flags: i32, interp: i16) -> Option<usize> {
    if start > end || cycles <= 0 {
        return None;
    }
    let at = list.iter().position(|e| e.start as f64 + 0.0001 > end as f64).unwrap_or(list.len());
    list.insert(
        at,
        ActionElem { name: name.to_vec(), source: None, start, end, clip_start: 0.0, clip_end: 0.0, flags, d_1c: 0, d_20: 0, cycles, path: None, interp },
    );
    Some(at)
}

/// Reads one action list [soft 0x083ff938 SI_actionsBinaryRead]: `second` false for the
/// first list (closed by token 0x0b), true for the second (closed by 0x1f).
///
/// The first token read here is the list's first element token or its closing token: the
/// token that opens the first list (10) was already consumed by the item reader, and the one
/// that opens the second (0x1e) is read here and passes through the element reader as an
/// unknown token. Elements follow until the closing token, or until an element comes back
/// without a name. The token variable is set to the closing token before each read after
/// an element, so the end of the file closes the list.
///
/// Each element is added at its place in time, half a frame wider on each side than the
/// file says, with flag word 2 or 10. A selected element is then selected the way the
/// program does it (`SI_actionsElemSelect`, 0x081e46b8): if the selection is in another
/// list, every element of that list is deselected first (`SI_actionsUnselectAll`,
/// 0x081e27cc). `other` is this file's other list, for that case, and `selected` says where
/// the selection is. (Those two functions also reset selection state of the action editor
/// and of the audio markers, which is not modelled; they are not counted as ported.)
pub fn read_actions(
    r: &mut Reader,
    second: bool,
    default_interpolation: i16,
    other: &mut [ActionElem],
    selected: &mut Selected,
) -> Vec<ActionElem> {
    let mut list: Vec<ActionElem> = Vec::new();
    let half = half_frame(r.st.frame_rate);
    let close: u16 = if second { 0x1f } else { 0x0b };
    let me = if second { Selected::Sources } else { Selected::Actions };
    let ok = fread_u16(&mut r.s, &mut r.st.token_utilio);
    r.note("action", r.st.token_utilio, ok);
    let mut found = true;
    while r.st.token_utilio != close && found {
        let e = read_action_elem(r, default_interpolation);
        found = e.found;
        if found {
            let flags = if e.flag_10 { 2 } else { 10 };
            let name = e.name.unwrap_or_default();
            if let Some(i) = actions_add_elem_time(&mut list, &name, e.start - half, half + e.end, e.cycles, flags, default_interpolation) {
                let el = &mut list[i];
                el.interp = e.interp;
                el.source = e.source;
                el.path = e.path;
                el.clip_start = e.clip_start;
                el.clip_end = e.clip_end;
                if e.selected {
                    if *selected != me {
                        if *selected != Selected::Elsewhere {
                            for o in other.iter_mut() {
                                o.flags &= !1;
                            }
                        }
                        *selected = me;
                    }
                    list[i].flags |= 1;
                }
            }
        }
        r.st.token_utilio = close;
        let ok = fread_u16(&mut r.s, &mut r.st.token_utilio);
        r.note("action", r.st.token_utilio, ok);
    }
    list
}

/// Writes one action list [soft 0x083ff218 SI_actionsBinaryWrite]: the opening token (10,
/// or 0x1e for the second list); per element the name (0x0c), start plus half a frame
/// (0x12), end minus half a frame (0x13), the interpolation code (0x15), the path animation
/// record if it has its +0x0c string (0x16 to 0x19, and 0x1a if it has a curve); for the
/// first list only, the cycles (0x14), token 0x10 if of flag bits 2 and 8 only 2 is set,
/// token 0x0f if selected, and the second name with the two clip values (0x1b to 0x1d) if
/// there is one; the element's end (0x11); then the closing token (0x0b or 0x1f).
pub fn write_actions(w: &mut Writer, list: &[ActionElem], second: bool) {
    let half = half_frame(w.st.frame_rate);
    w.token_utilio(if second { 0x1e } else { 10 });
    for e in list {
        w.token_utilio(0x0c);
        w.string(Some(&e.name));
        w.token_utilio(0x12);
        w.fvalue(half + e.start);
        w.token_utilio(0x13);
        w.fvalue(e.end - half);
        w.token_utilio(0x15);
        w.svalue(e.interp);
        if let Some(p) = e.path.as_ref().filter(|p| p.s_0c.is_some()) {
            w.token_utilio(0x16);
            w.string(p.s_10.as_deref());
            w.token_utilio(0x17);
            w.string(p.s_0c.as_deref());
            w.token_utilio(0x18);
            w.string(p.s_18.as_deref());
            w.token_utilio(0x19);
            w.string(p.s_14.as_deref());
            if let Some(c) = p.fcurve.as_ref() {
                w.token_utilio(0x1a);
                write_fcurve(w, c);
            }
        }
        if !second {
            w.token_utilio(0x14);
            w.ivalue(e.cycles);
            if e.flags & 10 == 2 {
                w.token_utilio(0x10);
            }
            if e.flags & 1 != 0 {
                w.token_utilio(0x0f);
            }
            if let Some(s) = e.source.as_ref() {
                w.token_utilio(0x1b);
                w.string(Some(s));
                w.token_utilio(0x1c);
                w.fvalue(e.clip_start);
                w.token_utilio(0x1d);
                w.fvalue(e.clip_end);
            }
        }
        w.token_utilio(0x11);
    }
    w.token_utilio(if second { 0x1f } else { 0x0b });
}

/// What reading an animation or struct key file gives.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimFile {
    /// The header's magic, or `None` if the header was not accepted.
    pub magic: Option<u32>,
    /// The header's version, −1.0 if the header was not accepted.
    pub version: f32,
    pub comment: Vec<u8>,
    pub anim: HierAnim,
    /// Items whose owner index is outside the element list: read and dropped.
    pub dropped_items: usize,
    /// How the element list ended, and whether every item's data ended at its zero token.
    pub hier_end: End,
    pub items_end: End,
    /// Offset of the first byte not read.
    pub pos: usize,
    pub clamped: bool,
}

/// Reads an animation or struct key file [soft 0x08127760 SI_elemdictReadAnimations]: the
/// header, the element list, items until the item reader finds none, then the two action
/// lists. (A struct key file is read by the same function; it simply has no action lists.)
///
/// Here, unlike in the other element readers, `SI_versionInput` is set to −1.0 when the
/// header is not accepted; angle curves of such a file are converted to degrees.
///
/// `ACT_OCCID` of an item whose header has no token 6 is whatever the previous item left in
/// the caller's variable; for the first item the original reads an uninitialised variable,
/// here zero.
///
/// The original would not return from a file that ends inside the element list or inside a
/// curve; here reading stops there (`hier_end` or `items_end` is `Eof`) and the action
/// lists are not attempted.
pub fn read_file(r: &mut Reader, default_interpolation: i16) -> AnimFile {
    let head = si_file::read_bin_header(&mut r.s);
    let (magic, version, comment) = match &head {
        Some(h) => (Some(h.magic), h.version, h.comment_bytes().to_vec()),
        None => (None, -1.0, Vec::new()),
    };
    r.st.version_input = version;
    let mut out = AnimFile {
        magic,
        version,
        comment,
        anim: HierAnim::new(),
        dropped_items: 0,
        hier_end: End::Zero,
        items_end: End::Zero,
        pos: 0,
        clamped: false,
    };
    out.hier_end = read_hieranim(r, &mut out.anim);
    if out.hier_end == End::Zero {
        let mut occ = 0i16;
        loop {
            let it = read_item(r, magic.is_none(), default_interpolation);
            if !it.found {
                break;
            }
            if let Some(o) = it.act_occ_id {
                occ = o;
            }
            if !out.anim.add_item(it.owner, it.kind as i16, it.act_rel_id, occ, it.w_20, it.data) {
                out.dropped_items += 1;
            }
            if it.end == End::Eof {
                out.items_end = End::Eof;
                break;
            }
        }
        if out.items_end == End::Zero {
            let mut selected = Selected::Elsewhere;
            out.anim.actions = read_actions(r, false, default_interpolation, &mut [], &mut selected);
            out.anim.sources = read_actions(r, true, default_interpolation, &mut out.anim.actions, &mut selected);
        }
    }
    out.pos = r.s.tell();
    out.clamped = r.clamped;
    out
}

/// How an item's header fields are chosen when a hierarchy animation is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemHeaders {
    /// As the program's two writers of an existing hierarchy animation do: no action
    /// numbers, token 7 with the value 1, whatever the items hold.
    Program,
    /// From each item's own fields (what the scene-side writer produces from the relations
    /// the items were read into).
    Stored,
}

fn write_items(w: &mut Writer, ha: &HierAnim, headers: ItemHeaders) {
    for (i, e) in ha.elems.iter().enumerate() {
        for it in &e.items {
            match headers {
                ItemHeaders::Program => write_item(w, &it.data, i as i32, it.kind as i32, -1, -1, 1),
                ItemHeaders::Stored => write_item(w, &it.data, i as i32, it.kind as i32, it.act_rel_id, it.act_occ_id as i32, it.w_20),
            }
        }
    }
}

/// An animation file as the program saves an existing hierarchy animation: the branch of
/// `elemdictSaveHierAnimation` (0x0812ca2c) for an element that carries one. (The function's
/// other branch builds the animation from the scene's relations and is not ported, so the
/// function is not counted as ported.) Header, element list, every item of every element in
/// order, the two action lists.
/// `chapter_name` is the name of the element list the animation belongs to (looked up from
/// its list index in the original). With `piped` (the program is writing to its pipe) a
/// token 99 ends the stream.
pub fn write_ani_file(w: &mut Writer, chapter_name: Option<&[u8]>, ha: &HierAnim, headers: ItemHeaders, piped: bool) {
    write_db_header(&mut w.out, w.version_output, MAGIC_ANIMATION);
    write_ani_body(w, chapter_name, ha, headers);
    if piped {
        w.st.token_elemdict = 99;
        w.put_u16(99);
    }
}

/// The part of an animation file after its header.
pub fn write_ani_body(w: &mut Writer, chapter_name: Option<&[u8]>, ha: &HierAnim, headers: ItemHeaders) {
    write_hieranim(w, chapter_name, ha);
    write_items(w, ha, headers);
    write_actions(w, &ha.actions, false);
    write_actions(w, &ha.sources, true);
}

/// A struct key file written from an existing hierarchy animation
/// [soft 0x0812cf54 SI_elemdictSaveExistingHierAnim]: header with the struct key magic,
/// element list, every item; no action lists. (The dialog's own writer,
/// `SI_elemdictSaveHierStructKey`, produces the same layout from the scene's elements and
/// is not ported.)
pub fn write_key_file(w: &mut Writer, chapter_name: Option<&[u8]>, ha: &HierAnim, headers: ItemHeaders) {
    write_db_header(&mut w.out, w.version_output, MAGIC_STRUCT_KEYS);
    write_key_body(w, chapter_name, ha, headers);
}

/// The part of a struct key file after its header.
pub fn write_key_body(w: &mut Writer, chapter_name: Option<&[u8]>, ha: &HierAnim, headers: ItemHeaders) {
    write_hieranim(w, chapter_name, ha);
    write_items(w, ha, headers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcurve::Key;

    fn curve(t: i32) -> Fcurve {
        Fcurve {
            curve_type: t,
            count: 2,
            keys: vec![Key { time: 0.0, value: 1.0, interpolation: 2, ..Key::new() }, Key { time: 1.0, value: 2.0, interpolation: 2, ..Key::new() }],
            pre_extrapolation: 0,
            post_extrapolation: 0,
            flags: 0,
            interpolation: 2,
        }
    }

    fn sample() -> HierAnim {
        let mut ha = HierAnim::new();
        ha.chapter = Some(b"MODELS".to_vec());
        ha.add_elem(b"root.1-0", -1);
        ha.add_elem(b"arm.1-0", 0);
        assert!(ha.add_item(0, 0, -1, 0, 1, ItemData::Curve(curve(7))));
        assert!(ha.add_item(1, 0, -1, 0, 1, ItemData::Curve(curve(8))));
        assert!(ha.add_item(1, 1, -1, 0, 1, ItemData::Static(StaticVal { curve_type: 9, value: 0.5 })));
        assert!(!ha.add_item(2, 0, -1, 0, 1, ItemData::None));
        ha
    }

    #[test]
    fn ani_round_trip() {
        let ha = sample();
        let mut w = Writer::new();
        write_ani_file(&mut w, ha.chapter.as_deref(), &ha, ItemHeaders::Program, false);
        let mut r = Reader::new(&w.out);
        let f = read_file(&mut r, 2);
        assert_eq!((f.magic, f.version, f.pos), (Some(MAGIC_ANIMATION), 4.0, w.out.len()));
        assert_eq!((f.hier_end, f.items_end, f.dropped_items), (End::Zero, End::Zero, 0));
        assert_eq!(f.anim, ha);
        // the file ends with the two empty lists: 10, 0x0b, 0x1e, 0x1f
        assert_eq!(&w.out[w.out.len() - 8..], &[0, 10, 0, 0x0b, 0, 0x1e, 0, 0x1f]);
        // and as a key file: no lists
        let mut k = Writer::new();
        write_key_file(&mut k, ha.chapter.as_deref(), &ha, ItemHeaders::Program);
        assert_eq!(k.out[88..], w.out[88..w.out.len() - 8]);
        let f = read_file(&mut Reader::new(&k.out), 2);
        assert_eq!((f.magic, f.pos), (Some(MAGIC_STRUCT_KEYS), k.out.len()));
        assert_eq!(f.anim, ha);
    }

    #[test]
    fn item_header_forms() {
        let mut ha = sample();
        ha.elems[1].items[0].act_rel_id = 0x4e2a;
        ha.elems[1].items[0].act_occ_id = 3;
        ha.elems[1].items[1].w_20 = 0;
        let mut w = Writer::new();
        write_ani_body(&mut w, ha.chapter.as_deref(), &ha, ItemHeaders::Stored);
        let mut r = Reader::new(&w.out);
        let mut got = HierAnim::new();
        assert_eq!(read_hieranim(&mut r, &mut got), End::Zero);
        let a = read_item(&mut r, false, 2);
        assert_eq!((a.found, a.owner, a.kind, a.act_rel_id, a.act_occ_id, a.w_20), (true, 0, 0, -1, None, 1));
        let b = read_item(&mut r, false, 2);
        assert_eq!((b.owner, b.act_rel_id, b.act_occ_id), (1, 0x4e2a, Some(3)));
        let c = read_item(&mut r, false, 2);
        assert_eq!((c.kind, c.w_20, c.data), (1, 0, ItemData::Static(StaticVal { curve_type: 9, value: 0.5 })));
        // the next token is 10: no item
        assert!(!read_item(&mut r, false, 2).found);
        // up to version 2.62 the header is the owner alone
        let mut w = Writer::with_version(2.62);
        write_item(&mut w, &ItemData::None, 5, 3, 1, 2, 1);
        assert_eq!(w.out, [0, 1, 0, 5, 0, 0]);
    }

    #[test]
    fn actions_round_trip_and_order() {
        let mut ha = sample();
        let el = |name: &[u8], start: f32, end: f32| ActionElem {
            name: name.to_vec(),
            source: None,
            start,
            end,
            clip_start: 0.0,
            clip_end: 0.0,
            flags: 10,
            d_1c: 0,
            d_20: 0,
            cycles: 1,
            path: None,
            interp: 5,
        };
        ha.actions = vec![el(b"walk", 0.0, 1.0), el(b"run", 2.0, 3.0)];
        ha.actions[0].flags = 2 | 1;
        ha.actions[1].source = Some(b"clip".to_vec());
        ha.actions[1].clip_start = 0.25;
        ha.actions[1].clip_end = 0.75;
        ha.actions[1].cycles = 2;
        ha.sources = vec![el(b"clip", 0.0, 4.0)];
        ha.sources[0].path = Some(PathAnimActs { fcurve: Some(curve(1)), s_0c: Some(b"a".to_vec()), s_10: Some(b"b".to_vec()), s_14: None, s_18: Some(b"c".to_vec()), ..PathAnimActs::new() });
        ha.sources[0].path.as_mut().unwrap().s_14 = Some(Vec::new());
        // the second list's elements are read back with one cycle and flag word 10
        let mut w = Writer::new();
        write_ani_file(&mut w, ha.chapter.as_deref(), &ha, ItemHeaders::Program, false);
        let f = read_file(&mut Reader::new(&w.out), 5);
        assert_eq!(f.pos, w.out.len());
        assert_eq!(f.anim, ha);
        // with a frame rate the stored times are half a frame inside the spans
        let mut w30 = Writer::new();
        w30.st.frame_rate = 30.0;
        write_actions(&mut w30, &ha.actions[..1], false);
        let stored = f32::from_bits(u32::from_be_bytes([w30.out[11], w30.out[12], w30.out[13], w30.out[14]]));
        assert_eq!(stored, 0.5 / 30.0);
        let mut r = Reader::new(&w30.out[2..]);
        r.st.frame_rate = 30.0;
        let mut sel = Selected::Elsewhere;
        let got = read_actions(&mut r, false, 5, &mut [], &mut sel);
        assert_eq!((got[0].start, got[0].flags, sel), (0.0, 3, Selected::Actions));
        // elements are placed by time, not by file order
        let mut list = Vec::new();
        actions_add_elem_time(&mut list, b"b", 5.0, 6.0, 1, 10, 2);
        actions_add_elem_time(&mut list, b"a", 1.0, 2.0, 1, 10, 2);
        actions_add_elem_time(&mut list, b"c", 5.0, 9.0, 1, 10, 2);
        assert_eq!(list.iter().map(|e| e.name[0]).collect::<Vec<_>>(), b"abc");
        assert_eq!(actions_add_elem_time(&mut list, b"x", 3.0, 2.0, 1, 10, 2), None);
        assert_eq!(actions_add_elem_time(&mut list, b"x", 1.0, 2.0, 0, 10, 2), None);
    }

    #[test]
    fn a_selection_in_the_second_list_clears_the_first() {
        let name = |n: u8| [0, 0x0c, n, 0];
        let mut b = Vec::new();
        b.extend_from_slice(&name(b'a'));
        b.extend_from_slice(&[0, 0x0f, 0, 0x11, 0, 0x0b]); // selected, end, close
        b.extend_from_slice(&[0, 0x1e]);
        b.extend_from_slice(&name(b'b'));
        b.extend_from_slice(&[0, 0x0f, 0, 0x11, 0, 0x1f]);
        let mut r = Reader::new(&b);
        let mut sel = Selected::Elsewhere;
        let mut first = read_actions(&mut r, false, 2, &mut [], &mut sel);
        assert_eq!(first[0].flags, 11);
        let second = read_actions(&mut r, true, 2, &mut first, &mut sel);
        assert_eq!((first[0].flags, second[0].flags, sel), (10, 11, Selected::Sources));
        assert_eq!(r.s.tell(), b.len());
    }

    #[test]
    fn files_without_lists_and_old_frame_times() {
        // items, then the end of the file: both lists come out empty
        let ha = sample();
        let mut w = Writer::new();
        write_key_body(&mut w, ha.chapter.as_deref(), &ha, ItemHeaders::Program);
        let mut r = Reader::new(&w.out);
        let f = read_file(&mut r, 2);
        assert_eq!(f.magic, None); // no header: read from the first byte all the same
        assert_eq!((f.anim, f.pos), (ha, w.out.len()));
        // integer frame times
        let b = [0, 0x0c, b'a', 0, 0, 0x0d, 0, 0, 0, 30, 0, 0x0e, 0, 0, 0, 60, 0, 0x11, 0, 0x0b];
        let mut sel = Selected::Elsewhere;
        let got = read_actions(&mut Reader::new(&b), false, 2, &mut [], &mut sel);
        assert_eq!((got[0].start, got[0].end), (1.0, 2.0));
    }
}
