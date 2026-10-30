// SPDX-License-Identifier: MIT
//! Pointers of the 32-bit original.
//!
//! Nothing here reproduces a function of the original. A pointer is four bytes holding an
//! address; the type parameter says what the original keeps there. [`crate::Mem`] follows it.

use core::marker::PhantomData;

/// A pointer of the 32-bit original: four bytes, typed by what it points to. Zero is null.
///
/// It is a plain value: it can be a field of a record, be copied, compared and stepped like
/// a C pointer. It borrows nothing; whether the address is mapped is found out when the
/// image is asked for what is there.
#[repr(transparent)]
pub struct Ptr32<T: ?Sized>(pub u32, PhantomData<fn() -> T>);

impl<T: ?Sized> Ptr32<T> {
    pub const NULL: Self = Ptr32(0, PhantomData);

    pub const fn new(address: u32) -> Self {
        Ptr32(address, PhantomData)
    }

    /// The same as [`Ptr32::new`] (the name the user-interface crate uses).
    pub const fn at(address: u32) -> Self {
        Ptr32(address, PhantomData)
    }

    pub const fn is_null(&self) -> bool {
        self.0 == 0
    }

    pub const fn addr(self) -> u32 {
        self.0
    }

    /// The same address seen as another type (a C cast).
    pub const fn cast<U: ?Sized>(self) -> Ptr32<U> {
        Ptr32(self.0, PhantomData)
    }

    /// The address `offset` bytes further on, as a `U`: a field of a record of which only
    /// the offset is known. For a field of a typed record use [`crate::field!`].
    pub const fn field<U: ?Sized>(self, offset: u32) -> Ptr32<U> {
        Ptr32(self.0.wrapping_add(offset), PhantomData)
    }

    /// The address `n` bytes further on (`(char *)p + n`), same type.
    pub const fn byte_add(self, n: i32) -> Self {
        Ptr32(self.0.wrapping_add(n as u32), PhantomData)
    }
}

impl<T> Ptr32<T> {
    /// The size of what is pointed to, as the 32-bit original counts it.
    pub const SIZE: u32 = core::mem::size_of::<T>() as u32;

    /// The `n`-th element after this one of an array of `T` (`p + n`). Wraps as the
    /// original's 32-bit arithmetic does.
    pub const fn add(self, n: i32) -> Self {
        Ptr32(self.0.wrapping_add((n as u32).wrapping_mul(Self::SIZE)), PhantomData)
    }

    /// `p - n`.
    pub const fn sub(self, n: i32) -> Self {
        Ptr32(self.0.wrapping_sub((n as u32).wrapping_mul(Self::SIZE)), PhantomData)
    }

    /// `p - base` in elements, as the original computes it: the byte difference as a
    /// signed 32-bit number, divided by the element size (truncating; 0 for a size of 0).
    pub const fn offset_from(self, base: Ptr32<T>) -> i32 {
        if Self::SIZE == 0 {
            return 0;
        }
        (self.0.wrapping_sub(base.0) as i32) / Self::SIZE as i32
    }

    /// A pointer to a field at a byte offset, typed by a projection that is never called.
    /// This is what [`crate::field!`] expands to.
    pub fn field_of<U>(self, offset: u32, _project: fn(&T) -> &U) -> Ptr32<U> {
        Ptr32(self.0.wrapping_add(offset), PhantomData)
    }
}

impl<T, const N: usize> Ptr32<[T; N]> {
    /// A pointer to element `i` of the array pointed to (`&(*p)[i]`); `i` is not checked
    /// against the bound, as in C.
    pub const fn elem(self, i: i32) -> Ptr32<T> {
        Ptr32(self.0.wrapping_add((i as u32).wrapping_mul(core::mem::size_of::<T>() as u32)), PhantomData)
    }
}

impl<T: ?Sized> Clone for Ptr32<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Copy for Ptr32<T> {}
impl<T: ?Sized> PartialEq for Ptr32<T> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<T: ?Sized> Eq for Ptr32<T> {}
impl<T: ?Sized> PartialOrd for Ptr32<T> {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl<T: ?Sized> Ord for Ptr32<T> {
    /// Unsigned, as the original compares addresses.
    fn cmp(&self, o: &Self) -> core::cmp::Ordering {
        self.0.cmp(&o.0)
    }
}
impl<T: ?Sized> core::hash::Hash for Ptr32<T> {
    fn hash<H: core::hash::Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}
impl<T: ?Sized> core::fmt::Debug for Ptr32<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}
impl<T: ?Sized> Default for Ptr32<T> {
    fn default() -> Self {
        Self::NULL
    }
}

/// An address of which nothing more is said.
pub type Addr = u32;

/// What a `void *` points to.
pub enum Opaque {}
/// What a function pointer points to: code of the original, at its address in `.text`.
pub enum Code {}
/// A zero-terminated string (`char *`).
pub type CStr32 = Ptr32<u8>;
/// A function pointer as the original stores it in a record or a table: the address of
/// the function's first instruction. [`crate::Registry`] finds the Rust that stands for it.
pub type FnPtr = Ptr32<Code>;

/// A pointer to a named field of a typed record: `field!(p, Grafport, camera)` is
/// `&p->camera`, a `Ptr32` of the field's type. The record type is written out so that the
/// offset is the compiler's (`offset_of!`), which the record's own assertions tie to the
/// original's.
#[macro_export]
macro_rules! field {
    ($p:expr, $T:ty, $f:ident) => {{
        let p: $crate::Ptr32<$T> = $p;
        p.field_of(::core::mem::offset_of!($T, $f) as u32, |r: &$T| &r.$f)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    #[repr(C)]
    struct R {
        a: i32,
        b: [i16; 4],
        c: Ptr32<R>,
    }

    #[test]
    fn arithmetic_is_c_arithmetic() {
        let p: Ptr32<i16> = Ptr32::new(0x1000);
        assert_eq!(p.add(3).addr(), 0x1006);
        assert_eq!(p.add(-1).addr(), 0x0ffe);
        assert_eq!(p.sub(2).addr(), 0x0ffc);
        assert_eq!(p.add(3).offset_from(p), 3);
        assert_eq!(p.sub(2).offset_from(p), -2);
        assert_eq!(Ptr32::<i32>::new(0xffff_fffc).add(1).addr(), 0); // wraps, as 32-bit code does
        let r: Ptr32<R> = p.cast();
        assert_eq!(r.add(1).addr(), 0x1010);
        assert_eq!(r.byte_add(-4).addr(), 0x0ffc);
        assert!(Ptr32::<R>::NULL.is_null() && !r.is_null());
        assert_eq!(format!("{r:?}"), "0x00001000");
        assert!(Ptr32::<u8>::new(0x8000_0000) > Ptr32::<u8>::new(0x7fff_ffff));
    }

    #[test]
    fn pointers_to_fields_and_elements() {
        let r: Ptr32<R> = Ptr32::new(0x2000);
        let b: Ptr32<[i16; 4]> = field!(r, R, b);
        assert_eq!(b.addr(), 0x2004);
        let b2: Ptr32<i16> = b.elem(2);
        assert_eq!(b2.addr(), 0x2008);
        let c: Ptr32<Ptr32<R>> = field!(r, R, c);
        assert_eq!(c.addr(), 0x200c);
        let raw: Ptr32<f32> = r.field(0x40);
        assert_eq!(raw.addr(), 0x2040);
        assert_eq!(core::mem::size_of::<Ptr32<Opaque>>(), 4);
        assert_eq!(core::mem::size_of::<FnPtr>(), 4);
    }
}
