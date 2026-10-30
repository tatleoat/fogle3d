// SPDX-License-Identifier: MIT
use si_mem::{Names, Ptr32};
use si_x87::vectors::{Case, Word};

/// The arguments of a case, by name.
pub struct Args<'a> {
    pub(crate) case: &'a Case,
    pub(crate) names: &'a Names,
}

impl<'a> Args<'a> {
    pub fn new(case: &'a Case, names: &'a Names) -> Args<'a> {
        Args { case, names }
    }

    /// The first word of the input field `name` (a value, or the address of a buffer).
    pub(crate) fn word(&self, name: &str) -> Option<u32> {
        let f = self.case.try_input(name)?;
        match f.words().first() {
            Some(Word::Value { value, .. }) => Some(*value as u32),
            Some(Word::Ref { name, offset }) => {
                let base = self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"));
                Some(base.wrapping_add(*offset))
            }
            _ => panic!("argument `{name}` has no value"),
        }
    }

    /// An argument as the 32-bit stack word (by value, `v_name`, or a pointer, `p_name`).
    pub fn v(&self, name: &str) -> u32 {
        self.word(&format!("v_{name}"))
            .or_else(|| self.word(&format!("p_{name}")))
            .unwrap_or_else(|| panic!("case of {} has no argument `{name}`", self.case.name))
    }

    pub fn i(&self, name: &str) -> i32 {
        self.v(name) as i32
    }

    pub fn s(&self, name: &str) -> i16 {
        self.v(name) as i16
    }

    pub fn f(&self, name: &str) -> f32 {
        f32::from_bits(self.v(name))
    }

    /// A pointer argument.
    pub fn p<T>(&self, name: &str) -> Ptr32<T> {
        Ptr32::new(self.v(name))
    }

    /// Whether the case has the argument.
    pub fn has(&self, name: &str) -> bool {
        self.case.try_input(&format!("v_{name}")).is_some() || self.case.try_input(&format!("p_{name}")).is_some()
    }

    /// The address of a buffer of the case (or a global), by its field name.
    pub fn buffer(&self, name: &str) -> u32 {
        self.names.iter().find(|b| b.0 == name).map(|b| b.1).unwrap_or_else(|| panic!("no buffer `{name}`"))
    }

    /// The case itself, for what the table needs beyond the arguments (its name, its notes).
    pub fn case(&self) -> &Case {
        self.case
    }

    /// The name of the argument (`v_name`) that holds the code address `addr`: how a call
    /// through a function pointer the routine was handed is recorded.
    pub(crate) fn callback(&self, addr: u32) -> Option<String> {
        if addr == 0 {
            return None;
        }
        self.case.inputs.iter().filter_map(|f| f.name.strip_prefix("v_")).find(|n| self.word(&format!("v_{n}")) == Some(addr)).map(str::to_string)
    }
}

/// What a routine returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    None,
    Eax(u32),
}

impl From<()> for Ret {
    fn from(_: ()) -> Ret {
        Ret::None
    }
}
impl From<u32> for Ret {
    fn from(v: u32) -> Ret {
        Ret::Eax(v)
    }
}
impl From<i32> for Ret {
    fn from(v: i32) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl From<i16> for Ret {
    fn from(v: i16) -> Ret {
        Ret::Eax(v as u16 as u32)
    }
}
impl From<u16> for Ret {
    fn from(v: u16) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl From<u8> for Ret {
    fn from(v: u8) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl From<bool> for Ret {
    fn from(v: bool) -> Ret {
        Ret::Eax(v as u32)
    }
}
impl<T> From<Ptr32<T>> for Ret {
    fn from(v: Ptr32<T>) -> Ret {
        Ret::Eax(v.addr())
    }
}
