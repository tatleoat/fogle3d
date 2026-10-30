// SPDX-License-Identifier: MIT
//! `SI_AttributeValue` as the expression code uses it (finding 0013).
//!
//! The original record is 12 bytes: a type at +0 and two 4-byte words at +4 and +8. The
//! evaluator fills one per operand and one per function argument. The types it produces or
//! tests are 1 (float at +4), 2 (16-bit integer at +4), 3 (enumeration: number at +4, value
//! at +8), 4 (integer at +4), 5 (string pointer at +4), 9 (float at +8) and 10 (a function
//! argument passed unevaluated: an `expr_node` pointer at +4).

use crate::tree::ExprNode;
use si_x87::{Round, F80};

#[derive(Clone, Debug, PartialEq)]
pub enum AttrValue<'a> {
    /// Any type whose words are plain numbers: the type, the word at +4, the word at +8.
    Raw { kind: i32, w1: u32, w2: u32 },
    /// Type 5: a C string; `None` is a null pointer.
    Str(Option<Vec<u8>>),
    /// Type 10: an argument that the function receives as a tree, not as a value.
    Node(&'a ExprNode),
}

impl<'a> AttrValue<'a> {
    pub const fn float(v: f32) -> Self {
        AttrValue::Raw { kind: 1, w1: v.to_bits(), w2: 0 }
    }

    pub const fn int(v: i32) -> Self {
        AttrValue::Raw { kind: 4, w1: v as u32, w2: 0 }
    }

    /// The type word at +0.
    pub fn kind(&self) -> i32 {
        match self {
            AttrValue::Raw { kind, .. } => *kind,
            AttrValue::Str(_) => 5,
            AttrValue::Node(_) => 10,
        }
    }

    /// The value as a real number, in the register [soft 0x080f0b18 SI_getAttrValInFloat]
    /// [soft 0x080f0ac8 SI_getAttrValInDouble]: type 1 the float at +4, type 2 the low 16
    /// bits of +4 as a signed integer, type 4 the integer at +4, type 9 the float at +8,
    /// anything else 0. The two original functions are the same code.
    pub fn in_float(&self) -> F80 {
        match *self {
            AttrValue::Raw { kind: 1, w1, .. } => F80::from_f32(f32::from_bits(w1)),
            AttrValue::Raw { kind: 2, w1, .. } => F80::from_i32(w1 as u16 as i16 as i32),
            AttrValue::Raw { kind: 4, w1, .. } => F80::from_i32(w1 as i32),
            AttrValue::Raw { kind: 9, w2, .. } => F80::from_f32(f32::from_bits(w2)),
            _ => F80::ZERO,
        }
    }

    /// The value as an integer [soft 0x080f0b68 SI_getAttrValInInt]: as [`Self::in_float`],
    /// the two float cases cut toward zero (a C cast: `fistp` under the truncating control
    /// word; a NaN or a value out of range gives 0x80000000).
    pub fn in_int(&self) -> i32 {
        match *self {
            AttrValue::Raw { kind: 1, w1, .. } => F80::from_f32(f32::from_bits(w1)).to_i32(Round::Zero),
            AttrValue::Raw { kind: 2, w1, .. } => w1 as u16 as i16 as i32,
            AttrValue::Raw { kind: 4, w1, .. } => w1 as i32,
            AttrValue::Raw { kind: 9, w2, .. } => F80::from_f32(f32::from_bits(w2)).to_i32(Round::Zero),
            _ => 0,
        }
    }
}
