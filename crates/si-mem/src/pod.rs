// SPDX-License-Identifier: MIT
//! Which Rust types can lie in the image, and the i386 layout rules that differ from the
//! host's.
//!
//! Nothing here reproduces a function of the original.
//!
//! The original was compiled for i386 by GCC 2.95. Its structures follow the System V i386
//! rules: `char` 1, `short` 2, `int`, `long`, `float` and every pointer 4 bytes aligned to
//! 4, and **`double` and `long long` 8 bytes aligned to 4 inside a structure** (not 8, as
//! on the machine this code is compiled for). A record of this project is a `#[repr(C)]`
//! Rust structure whose fields are chosen so that the host compiler lays it out the same
//! way: pointers are [`crate::Ptr32`], doubles are [`Double`], 64-bit integers
//! [`LongLong`] and [`ULongLong`]. Every record asserts its size and each field's offset,
//! so a wrong choice does not compile.

use crate::Ptr32;

/// A type that can be read from and written to the image in place: every bit pattern is a
/// value, there are no padding bytes, and it is aligned to at most 4 bytes.
///
/// # Safety
/// The implementer guarantees exactly that. For a record: every field is `Pod`, the fields
/// tile the structure (the sum of their sizes is its size), and its alignment is at most 4.
/// [`crate::record!`] asserts all three at compile time and then implements the trait.
pub unsafe trait Pod: Copy + 'static {}

macro_rules! pod {
    ($($t:ty),*) => { $( unsafe impl Pod for $t {} )* };
}
pod!(u8, i8, u16, i16, u32, i32, f32);
unsafe impl<T: Pod, const N: usize> Pod for [T; N] {}
unsafe impl<T: ?Sized + 'static> Pod for Ptr32<T> {}

/// A `double` inside a structure: 8 bytes, aligned to 4.
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Double(pub f64);
unsafe impl Pod for Double {}

/// A `long long` inside a structure: 8 bytes, aligned to 4.
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LongLong(pub i64);
unsafe impl Pod for LongLong {}

/// An `unsigned long long` inside a structure: 8 bytes, aligned to 4.
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ULongLong(pub u64);
unsafe impl Pod for ULongLong {}

/// Used by [`crate::record!`]: compiles only for a `Pod` type.
pub const fn assert_pod<T: Pod>() {}

/// Defines a record of the original: `OFFSET name: type, "what it is";` per field.
///
/// The structure is `#[repr(C)]`; its size, its alignment (at most 4), each field's offset,
/// that the fields tile it without padding and that each field is [`Pod`] are asserted at
/// compile time, and it is `Pod` itself. `SIZE`, `ORIGINAL` (the structure's name in the
/// binary) and `FIELDS` (offset, size, name, description) are associated constants, for
/// tools that tabulate or check layouts.
///
/// ```
/// si_mem::record! {
///     /// A list node.
///     pub struct Node = "SI_Node", size 0x10 {
///         0x00 next: si_mem::Ptr32<Node>, "the next node";
///         0x04 id: i16, "identifier";
///         0x06 flags: [u8; 2], "two flag bytes";
///         0x08 weight: si_mem::Double, "a double at an offset that is a multiple of 4";
///     }
/// }
/// assert_eq!(Node::SIZE, 0x10);
/// ```
#[macro_export]
macro_rules! record {
    (
        $(#[$meta:meta])*
        pub struct $name:ident = $orig:literal, size $size:literal {
            $( $off:literal $field:ident : $ty:ty , $doc:literal ; )*
        }
    ) => {
        $(#[$meta])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            $( #[doc = $doc] pub $field : $ty , )*
        }

        // asserted below: every field is Pod, the fields tile the structure, alignment <= 4
        unsafe impl $crate::Pod for $name {}

        #[allow(dead_code)]
        impl $name {
            /// Size in the original, in bytes.
            pub const SIZE: u32 = $size;
            /// The structure's name in the binary.
            pub const ORIGINAL: &'static str = $orig;
            /// Offset, size, name and description of every field.
            pub const FIELDS: &'static [(u32, u32, &'static str, &'static str)] =
                &[ $( ($off, ::core::mem::size_of::<$ty>() as u32, stringify!($field), $doc) ),* ];
        }

        const _: () = {
            assert!(::core::mem::size_of::<$name>() == $size);
            assert!(::core::mem::align_of::<$name>() <= 4);
            $( assert!(::core::mem::offset_of!($name, $field) == $off); )*
            assert!(0 $( + ::core::mem::size_of::<$ty>() )* == $size);
            $( $crate::assert_pod::<$ty>(); )*
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::record! {
        /// A record with every kind of field.
        pub struct Sample = "SI_Sample", size 0x24 {
            0x00 kind: i32, "type";
            0x04 a: i16, "a short";
            0x06 b: u8, "a byte";
            0x07 c: i8, "a signed byte";
            0x08 next: Ptr32<Sample>, "a pointer to the same record";
            0x0c value: Double, "a double at an offset that is not a multiple of 8";
            0x14 count: LongLong, "a long long";
            0x1c xy: [f32; 2], "two floats";
        }
    }

    #[test]
    fn the_i386_rules() {
        assert_eq!((core::mem::size_of::<Double>(), core::mem::align_of::<Double>()), (8, 4));
        assert_eq!((core::mem::size_of::<LongLong>(), core::mem::align_of::<LongLong>()), (8, 4));
        assert_eq!((core::mem::size_of::<ULongLong>(), core::mem::align_of::<ULongLong>()), (8, 4));
        assert_eq!(core::mem::align_of::<Sample>(), 4);
        assert_eq!(Sample::SIZE, 0x24);
        assert_eq!(Sample::ORIGINAL, "SI_Sample");
        assert_eq!(Sample::FIELDS[5], (0x0c, 8, "value", "a double at an offset that is not a multiple of 8"));
        // the host's own double would not fit at 0x0c
        assert_eq!(core::mem::align_of::<f64>(), 8);
    }
}
