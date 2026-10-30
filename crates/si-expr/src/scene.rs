// SPDX-License-Identifier: MIT
//! What evaluation asks of the rest of the program (finding 0013).
//!
//! An expression reads other elements' parameters, writes its target, calls the C library's
//! random generator and, in a few functions, asks for positions, function curves and
//! relations. None of that program state is ported yet, so the evaluator and the built-in
//! functions take it through [`Scene`]. Each method names the original functions it stands
//! for. The defaults answer what the original gets from a scene with nothing in it.

// adapter: not in place; what the value-form evaluator asks of the program; on the image the same questions go
// to the dictionaries, the curves and the models in `crate::image::elem`.

use crate::rand::CRand;
use crate::tree::ArgNode;
use crate::value::AttrValue;
use si_x87::F80;

/// Where evaluating has no defined result in the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalUndefined {
    /// A null pointer is followed: a node without its leaf record or operand, a string
    /// comparison with a null string, a random record without its data.
    NullPointer(u8),
    /// The remainder operator divides by an integer zero (an operand between −1 and 1) or
    /// takes the remainder of the smallest integer by −1: the processor traps.
    IntegerDivide,
    /// A function index the table does not have: for a negative one the original returns
    /// without writing a result, for one past the end it reads behind the table.
    UnknownFunction(i32),
    /// A value is taken from the table of pre-computed random numbers outside its bounds.
    RandomTable,
    /// A function is given an operator node where it expects a reference, and the node's
    /// left operand happens to carry the number the function tests for.
    NodeMisread,
    /// A function reads more arguments than it was given.
    MissingArgument,
}

/// What [`Scene::elem_info`] tells `has_relation` about an element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElemInfo {
    /// The element record's word at +0x54: 1 for an element that is not a root of its file.
    pub not_root: i32,
    /// Whether the element has data (+4 not null).
    pub has_data: bool,
    /// The word at +0xc of that data (for an animation element: its number of curves).
    pub data_count: i32,
}

pub trait Scene {
    /// `SI_frameStart` (0x08782050).
    fn frame_start(&self) -> i32 {
        0
    }
    /// `SI_frameEnd` (0x08782044).
    fn frame_end(&self) -> i32 {
        0
    }
    /// `SI_frameCurrent` (0x08782040).
    fn frame_current(&self) -> i32 {
        0
    }
    /// `SI_queryMode` (0x08699ec0): set while the spreadsheet evaluates a query.
    fn query_mode(&self) -> i16 {
        0
    }
    /// The C library's `rand`.
    fn rand(&mut self) -> i32;
    /// The C library's `srand`.
    fn srand(&mut self, seed: u32);
    /// What the original prints on standard output.
    fn print(&mut self, _text: &str) {}

    /// Whether the element a reference names exists `SI_elemdictGetElem` (0x08132b50).
    fn elem_exists(&self, _chapter: i32, _elem: i32) -> bool {
        false
    }
    /// The current value of a parameter, into `value`
    /// `SI_edElementStaticValueGet`; the original passes the element, the chapter, the
    /// fcurve type, the reference's access information and, for a custom parameter, its
    /// name. The value keeps what it held if the parameter gives nothing.
    fn static_value_get(&mut self, _a: &ArgNode, _custom: Option<&[u8]>, _value: &mut AttrValue<'static>) {}
    /// The same through the attribute dictionary, for the spreadsheet's queries and for the
    /// old value of an assignment's target `SI_attdictGetGrpIndexFromFcurve`
    /// `SI_edElementAttValueGet`.
    fn att_value_get(&mut self, _a: &ArgNode, _value: &mut AttrValue<'static>) {}
    /// Writes a parameter `SI_edElementStaticValueSet` (0x080f06e4). `kind` is the type
    /// word of the value record: 1, or `None` for a custom parameter, where the original
    /// leaves that word unset.
    fn static_value_set(&mut self, _a: &ArgNode, _kind: Option<i32>, _v: f32, _custom: Option<&[u8]>) {}
    /// A channel's value at a time `SI_chnGetValue`.
    fn chn_get_value(&mut self, _a: &ArgNode, _time: f32) -> F80 {
        F80::ZERO
    }
    /// Sets a channel's value `SI_chnSetValue` (0x0855d6a8).
    fn chn_set_value(&mut self, _a: &ArgNode, _v: f32) {}

    /// The value at `time` of the function curve behind a reference: the curve from
    /// `SI_edElementStaticFcvGet` (or `SI_cusFetchCustomValue` for a custom
    /// parameter), evaluated with `SI_fcurveValue` (0x082002dc) and its range check on.
    /// `None` if the element or the curve is missing.
    fn fcurve_value(&mut self, _a: &ArgNode, _custom: bool, _time: f32) -> Option<F80> {
        None
    }
    /// The same at a frame, with `SI_fcurveFrameValue` (0x08200258).
    fn fcurve_frame_value(&mut self, _a: &ArgNode, _custom: bool, _frame: i32) -> Option<F80> {
        None
    }
    /// The position of an element's centre `SI_constrLocation` (0x08222538), after the
    /// original's detour for the interest of a camera or light (`SI_dbChapter2GrafType`,
    /// `SI_edGetCamFromCamInt`, `SI_edGetLigFromLigInt`). `None` if the element
    /// or its data is missing.
    fn location(&mut self, _a: &ArgNode) -> Option<[f32; 3]> {
        None
    }
    /// The current camera's position (floats at +0x10 of the record `SI_procData` + 8
    /// points at).
    fn camera_position(&self) -> [f32; 3] {
        [0.0; 3]
    }
    /// The current camera's interest (floats at +0x1c of the same record).
    fn camera_interest(&self) -> [f32; 3] {
        [0.0; 3]
    }
    /// The solid noise of the texture code at a point `SI_ttnGetNoise` (0x083172bc)
    /// (after `SI_ttnInitNoise` (0x08317270) on the first call of the program).
    fn noise(&mut self, _x: f32, _y: f32, _z: f32) -> F80 {
        F80::ZERO
    }
    /// The element record's fields `has_relation` looks at; `None` if there is no element.
    fn elem_info(&self, _a: &ArgNode) -> Option<ElemInfo> {
        None
    }
    /// Whether the element has a relation with an id from `low` to `high` in one of its
    /// six relation groups `SI_elemdictFindRangeRelatedElem`.
    fn related_in_range(&mut self, _a: &ArgNode, _direction: i16, _group: i32, _low: i32, _high: i32, _chapter: i32) -> bool {
        false
    }
    /// Whether the element has relation `relid` in the given direction
    /// `SI_elemdictRelationCheck`.
    fn relation_check(&mut self, _a: &ArgNode, _direction: i16, _relid: i32) -> bool {
        false
    }
}

/// A scene with nothing in it and the C library's generator in its start-up state.
#[derive(Clone, Debug, Default)]
pub struct EmptyScene {
    pub crand: CRand,
    pub frame_start: i32,
    pub frame_end: i32,
    pub frame_current: i32,
    pub printed: String,
}

impl Scene for EmptyScene {
    fn frame_start(&self) -> i32 {
        self.frame_start
    }
    fn frame_end(&self) -> i32 {
        self.frame_end
    }
    fn frame_current(&self) -> i32 {
        self.frame_current
    }
    fn rand(&mut self) -> i32 {
        self.crand.rand()
    }
    fn srand(&mut self, seed: u32) {
        self.crand.srand(seed)
    }
    fn print(&mut self, text: &str) {
        self.printed.push_str(text);
    }
}
