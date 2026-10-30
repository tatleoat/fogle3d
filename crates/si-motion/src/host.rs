// SPDX-License-Identifier: MIT
//! How the ported routines call the parts of the program that are not ported here.
//!
//! Every original function that these source files call and that belongs to another package
//! (the model code, constraints, inverse kinematics, shapes, clusters, expressions, the user
//! interface …) is a method of a host trait, declared with [`host_trait!`]. A method takes
//! the memory image and then exactly the arguments the original passes, in the original's
//! order: pointers as addresses, `float`s as `f32`. Its default body does nothing and
//! returns zero, so a host implements only what it has.
//!
//! The same declaration also implements the trait for [`Trace`], a host that writes down
//! every call with its arguments and answers through a closure. That is what the unit tests
//! use, and what a comparison with the original under emulation needs: the sequence of
//! calls leaving a routine, with their arguments, is part of what the routine does.

use si_mem::Mem;
use si_x87::F80;

/// One argument of a recorded call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg {
    /// An integer or a pointer, as the 32-bit stack slot holds it.
    I(u32),
    /// A `float` argument, by its bits.
    F(u32),
    /// A `double` argument, by its bits.
    D(u64),
}

impl Arg {
    pub fn f(v: f32) -> Arg {
        Arg::F(v.to_bits())
    }
}

macro_rules! arg_int {
    ($($t:ty),*) => { $( impl From<$t> for Arg { fn from(v: $t) -> Arg { Arg::I(v as i32 as u32) } } )* };
}
arg_int!(i8, i16, i32, u8, u16);
impl From<u32> for Arg {
    fn from(v: u32) -> Arg {
        Arg::I(v)
    }
}
impl From<bool> for Arg {
    fn from(v: bool) -> Arg {
        Arg::I(v as u32)
    }
}
impl From<f32> for Arg {
    fn from(v: f32) -> Arg {
        Arg::F(v.to_bits())
    }
}
impl From<f64> for Arg {
    fn from(v: f64) -> Arg {
        Arg::D(v.to_bits())
    }
}

/// What a recorded call answers: the integer register and the top of the x87 stack. A
/// method's return type picks the one it needs.
#[derive(Clone, Copy)]
pub struct Ret {
    pub eax: u32,
    pub st0: F80,
}

impl Default for Ret {
    fn default() -> Self {
        Ret { eax: 0, st0: F80::ZERO }
    }
}

impl Ret {
    pub fn int(v: i32) -> Ret {
        Ret { eax: v as u32, st0: F80::ZERO }
    }
    pub fn ptr(v: u32) -> Ret {
        Ret { eax: v, st0: F80::ZERO }
    }
    pub fn float(v: F80) -> Ret {
        Ret { eax: 0, st0: v }
    }
}

/// Conversion of a recorded answer to a method's return type.
pub trait FromRet {
    fn from_ret(r: Ret) -> Self;
}
impl FromRet for () {
    fn from_ret(_: Ret) {}
}
impl FromRet for u32 {
    fn from_ret(r: Ret) -> u32 {
        r.eax
    }
}
impl FromRet for i32 {
    fn from_ret(r: Ret) -> i32 {
        r.eax as i32
    }
}
impl FromRet for i16 {
    /// A `short` result is the low word of the register.
    fn from_ret(r: Ret) -> i16 {
        r.eax as i16
    }
}
impl FromRet for bool {
    fn from_ret(r: Ret) -> bool {
        r.eax != 0
    }
}
impl FromRet for F80 {
    fn from_ret(r: Ret) -> F80 {
        r.st0
    }
}

/// One recorded call: the method's name (the original function's name in snake case) and
/// its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: &'static str,
    pub args: Vec<Arg>,
}

/// The closure a [`Trace`] answers through: it may read and write the image (to play the
/// callee's side effects) and returns the callee's result.
pub type Responder = Box<dyn FnMut(&mut Mem, &'static str, &[Arg]) -> Ret>;

/// A host that records every call. Implements every host trait declared with
/// [`host_trait!`].
pub struct Trace {
    pub calls: Vec<Call>,
    pub respond: Responder,
}

impl Default for Trace {
    fn default() -> Self {
        Trace::new()
    }
}

impl Trace {
    /// Records, and answers zero to everything.
    pub fn new() -> Trace {
        Trace { calls: Vec::new(), respond: Box::new(|_, _, _| Ret::default()) }
    }

    pub fn with(respond: impl FnMut(&mut Mem, &'static str, &[Arg]) -> Ret + 'static) -> Trace {
        Trace { calls: Vec::new(), respond: Box::new(respond) }
    }

    pub fn call(&mut self, mem: &mut Mem, name: &'static str, args: &[Arg]) -> Ret {
        self.calls.push(Call { name, args: args.to_vec() });
        (self.respond)(mem, name, args)
    }

    /// The names of the recorded calls, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.calls.iter().map(|c| c.name).collect()
    }
}

/// Declares a host trait: one method per original function called from the ported code.
///
/// ```ignore
/// host_trait! {
///     /// What `motionXxx.c` calls outside this package.
///     pub trait XxxHost {
///         /// `SI_modelUpdateSonState(father, model, flag)` (model.c).
///         fn model_update_son_state(father: Addr, model: Addr, flag: i32);
///         /// `SI_edIsItemConstrained(dict, object, chapter, relation, flag, time)` (edtools.c).
///         fn ed_is_item_constrained(dict: Addr, obj: Addr, chap: i32, rel: i32, flag: i32, time: f32) -> i16;
///     }
/// }
/// ```
///
/// Every method gets `&mut self, mem: &mut Mem` in front of its arguments and a default
/// body that returns the zero of its return type. Supertraits may be listed after the name.
#[macro_export]
macro_rules! host_trait {
    ($(#[$m:meta])* pub trait $T:ident $(: $($sup:path),+)? {
        $( $(#[$fm:meta])* fn $f:ident ( $($a:ident : $t:ty),* $(,)? ) $(-> $r:ty)? ; )*
    }) => {
        $(#[$m])*
        pub trait $T $(: $($sup +)+)? {
            $(
                $(#[$fm])*
                #[allow(unused_variables, clippy::too_many_arguments)]
                fn $f(&mut self, mem: &mut $crate::Mem, $($a: $t),*) $(-> $r)? {
                    <_ as $crate::host::FromRet>::from_ret($crate::host::Ret::default())
                }
            )*
        }
        impl $T for $crate::host::Trace {
            $(
                #[allow(clippy::too_many_arguments)]
                fn $f(&mut self, mem: &mut $crate::Mem, $($a: $t),*) $(-> $r)? {
                    let r = self.call(mem, stringify!($f), &[$($crate::host::Arg::from($a)),*]);
                    <_ as $crate::host::FromRet>::from_ret(r)
                }
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_mem::Addr;

    host_trait! {
        /// A made-up trait to exercise the macro.
        pub trait DemoHost {
            fn touch(model: Addr, flag: i32);
            fn is_constrained(obj: Addr, time: f32) -> i16;
            fn length(obj: Addr) -> F80;
        }
    }

    struct Nothing;
    impl DemoHost for Nothing {}

    fn routine<H: DemoHost + ?Sized>(m: &mut Mem, h: &mut H, obj: Addr) -> f32 {
        h.touch(m, obj, 3);
        if h.is_constrained(m, obj, 0.5) != 0 {
            return -1.0;
        }
        (h.length(m, obj) * 2.0f32).to_f32()
    }

    #[test]
    fn default_bodies_answer_zero() {
        let mut m = Mem::new();
        assert_eq!(routine(&mut m, &mut Nothing, 0x10), 0.0);
    }

    #[test]
    fn trace_records_and_answers() {
        let mut m = Mem::new();
        let mut t = Trace::with(|_, name, _| match name {
            "length" => Ret::float(F80::from_f32(1.25)),
            _ => Ret::default(),
        });
        assert_eq!(routine(&mut m, &mut t, 0x10), 2.5);
        assert_eq!(t.names(), ["touch", "is_constrained", "length"]);
        assert_eq!(t.calls[1].args, [Arg::I(0x10), Arg::f(0.5)]);
    }
}
