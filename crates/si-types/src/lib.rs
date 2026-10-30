// SPDX-License-Identifier: MIT
//! The core in-memory structures of Softimage|3D 4.0 (finding 0010).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `elemdict.c`, `relations.c`, `reldict.c`,
//! `attdict.c`, `enmdict.c`, and the allocators in `model_b.c`, `mesh_b.c`, `obj_global.c`,
//! `spline.c`, `face.c`, `patch.c`, `nrb_b.c` and `graflist.c`, with three small helpers
//! from `dbexternal.c` and `updlist.c`.
//!
//! Two layers:
//!
//! * **Layouts** ([`dict`], [`model`], [`geom`], [`scene`]): `#[repr(C)]` records that match
//!   the original's 32-bit memory layout offset for offset. A pointer is a [`Ptr32`], a
//!   32-bit placeholder that remembers what it points to; it is never dereferenced here.
//!   Every field carries its evidence and a [`Confidence`]; every record asserts its size and
//!   each field's offset at compile time. The same table drives the per-structure
//!   read / named / unknown percentages (`cargo run -p si-types --bin si-types-table`).
//! * **Functions** ([`store`], [`reldict`], [`attdict`], [`enmdict`] and the `init`/`allocate`
//!   methods on the layouts): the allocation, initialisation and look-up routines that were
//!   read in full, each citing its original. Where the original owns a `malloc`ed array the
//!   Rust owns a `Vec` beside the record, and the record's count field is kept equal to the
//!   `Vec`'s length.
//!
//! What the original's allocator leaves behind matters for "reproduce, do not improve":
//! `SI_malloc` (0x080d0ee8) does **not** clear memory. It writes the byte
//! 0x6f (`o`) at the start of the block and 0x6b (`k`) at its end, and nothing else. A field
//! an allocator does not set therefore holds whatever the heap held. Rust cannot reproduce
//! that; such fields are zero here, and each allocator's comment lists them.

pub mod attdict;
pub mod dict;
pub mod enmdict;
pub mod geom;
pub mod model;
pub mod pod;
pub mod reldict;
pub mod scene;
pub mod store;

/// How well a field is established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    /// Code was read that uses the field and shows what it means.
    Read,
    /// The meaning is taken from a function name, a file token, the vendor's attribute table
    /// or a manual; the code confirms offset and width only.
    Named,
    /// Offset and width are seen (or the bytes are never touched); the meaning is not known.
    Unknown,
}

/// One field of a layout.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub offset: usize,
    pub size: usize,
    pub ty: &'static str,
    pub confidence: Confidence,
    pub evidence: &'static str,
}

/// A structure of the original: its name there, its size and its fields in offset order.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub name: &'static str,
    pub original: &'static str,
    pub size: usize,
    /// Where the size comes from.
    pub size_evidence: &'static str,
    pub fields: &'static [Field],
}

impl Confidence {
    /// The weaker of two: a field of an embedded structure is no better established than
    /// the claim that the structure is embedded there.
    pub fn weaker(self, other: Confidence) -> Confidence {
        use Confidence::*;
        match (self, other) {
            (Unknown, _) | (_, Unknown) => Unknown,
            (Named, _) | (_, Named) => Named,
            _ => Read,
        }
    }
}

impl Layout {
    /// Bytes established as read, named, unknown. A field that is itself one of the
    /// crate's structures is counted field by field, each no better than the embedding.
    pub fn bytes(&self) -> (usize, usize, usize) {
        let all = all_layouts();
        let mut b = (0, 0, 0);
        self.count(Confidence::Read, &all, &mut b);
        b
    }

    fn count(&self, outer: Confidence, all: &[Layout], b: &mut (usize, usize, usize)) {
        for f in self.fields {
            let c = outer.weaker(f.confidence);
            match all.iter().find(|l| l.name == f.ty) {
                Some(inner) if c != Confidence::Unknown => inner.count(c, all, b),
                _ => match c {
                    Confidence::Read => b.0 += f.size,
                    Confidence::Named => b.1 += f.size,
                    Confidence::Unknown => b.2 += f.size,
                },
            }
        }
    }

    /// True when the fields tile the structure: no gap, no overlap, ending at its size.
    pub fn is_tiled(&self) -> bool {
        let mut at = 0;
        for f in self.fields {
            if f.offset != at {
                return false;
            }
            at += f.size;
        }
        at == self.size
    }
}

/// A value all of whose bytes are zero: what `calloc` gives, and what this crate uses for
/// fields the original leaves uninitialised.
pub trait Zero {
    fn zero() -> Self;
}

macro_rules! zero_num {
    ($($t:ty),*) => { $( impl Zero for $t { fn zero() -> Self { 0 as $t } } )* };
}
zero_num!(u8, i8, u16, i16, u32, i32, f32);

impl<T: Zero + Copy, const N: usize> Zero for [T; N] {
    fn zero() -> Self {
        [T::zero(); N]
    }
}

/// A pointer of the 32-bit original: four bytes, typed by what it points to. Zero is null.
/// In this crate it is a placeholder: nothing here follows one, and the owned stores keep
/// the pointed-to data in a `Vec` beside the record instead. The type itself is `si-mem`'s
/// (finding 0024), the same in every crate, and that crate's memory image follows it.
pub use si_mem::Ptr32;

impl<T: ?Sized> Zero for Ptr32<T> {
    fn zero() -> Self {
        Self::NULL
    }
}

/// What a `void *` points to, what a function pointer points to, and a zero-terminated
/// string (`char *`): `si-mem`'s, the same in every crate.
pub use si_mem::{CStr32, Code, Opaque};

/// A `double` inside a structure. The i386 ABI aligns it to 4 bytes, not 8, and several
/// records (the NURBS surface among them) place one at an offset that is not a multiple
/// of 8. `si-mem`'s type, the same in every crate.
pub use si_mem::Double;

impl Zero for Double {
    fn zero() -> Self {
        Double(0.0)
    }
}

/// A 4 × 4 matrix of floats, row by row (`float [4][4]`; GCC 2.95 mangles it `PA3_f`).
pub type Matrix = [[f32; 4]; 4];

/// The identity matrix [soft 0x086dc160 SI_identite], which the allocators copy into
/// every matrix field.
pub const IDENTITY: Matrix =
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Defines a `#[repr(C)]` record with the original's offsets, its field table, a zero value,
/// and compile-time assertions of its size and of every field's offset.
macro_rules! layout {
    (
        $(#[$meta:meta])*
        pub struct $name:ident = $orig:literal, size $size:literal, $sev:literal {
            $( $off:literal $field:ident : $ty:ty , $conf:ident , $ev:literal ; )*
        }
    ) => {
        $(#[$meta])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            $( #[doc = $ev] pub $field : $ty , )*
        }

        impl $name {
            /// Size in the original, in bytes.
            pub const SIZE: usize = $size;
            pub const LAYOUT: $crate::Layout = $crate::Layout {
                name: stringify!($name),
                original: $orig,
                size: $size,
                size_evidence: $sev,
                fields: &[ $( $crate::Field {
                    name: stringify!($field),
                    offset: $off,
                    size: ::core::mem::size_of::<$ty>(),
                    ty: stringify!($ty),
                    confidence: $crate::Confidence::$conf,
                    evidence: $ev,
                } ),* ],
            };
        }

        impl $crate::Zero for $name {
            fn zero() -> Self {
                $name { $( $field: <$ty as $crate::Zero>::zero() ),* }
            }
        }

        const _: () = {
            assert!(::core::mem::size_of::<$name>() == $size);
            $( assert!(::core::mem::offset_of!($name, $field) == $off); )*
        };
    };
}
pub(crate) use layout;

/// Every layout of the crate, in the order of the finding.
pub fn all_layouts() -> Vec<Layout> {
    vec![
        dict::Elem::LAYOUT,
        dict::ElemDict::LAYOUT,
        dict::ListOptions::LAYOUT,
        dict::ElemList::LAYOUT,
        dict::DbListNode::LAYOUT,
        dict::Relations::LAYOUT,
        dict::RelList::LAYOUT,
        dict::ARelation::LAYOUT,
        dict::RelDict::LAYOUT,
        dict::RelRule::LAYOUT,
        dict::SortTbl::LAYOUT,
        dict::UpdList::LAYOUT,
        dict::ChapIds::LAYOUT,
        dict::ChapInfo::LAYOUT,
        dict::ElemIdentity::LAYOUT,
        dict::AttributeValue::LAYOUT,
        dict::AttGrpIndex::LAYOUT,
        dict::AttributeDict::LAYOUT,
        dict::AttribGroup::LAYOUT,
        dict::Attribute::LAYOUT,
        dict::EnumValDict::LAYOUT,
        dict::EnumValType::LAYOUT,
        dict::EnumVal::LAYOUT,
        model::Model::LAYOUT,
        geom::Vector::LAYOUT,
        scene::GrafNode::LAYOUT,
        scene::GrafList::LAYOUT,
        geom::Mesh::LAYOUT,
        geom::Vertex::LAYOUT,
        geom::Polygon::LAYOUT,
        geom::PolyNode::LAYOUT,
        geom::Edge::LAYOUT,
        geom::Face::LAYOUT,
        geom::Spline::LAYOUT,
        geom::SplineKey::LAYOUT,
        geom::Patch::LAYOUT,
        geom::NurbsDir::LAYOUT,
        geom::NurbsCrv::LAYOUT,
        geom::NurbsPoint::LAYOUT,
        geom::NurbsSrf::LAYOUT,
        scene::Material::LAYOUT,
        scene::LclMap::LAYOUT,
        scene::Texture3D::LAYOUT,
        scene::Camera::LAYOUT,
        scene::Light::LAYOUT,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_tiles_its_structure() {
        for l in all_layouts() {
            assert!(l.is_tiled(), "{} ({}) has a gap or an overlap", l.name, l.original);
            let (r, n, u) = l.bytes();
            assert_eq!(r + n + u, l.size, "{}", l.name);
        }
    }

    #[test]
    fn an_embedded_structure_is_counted_field_by_field() {
        // a face embeds a spline: the spline's unknown bytes are the face's unknown bytes
        let (_, _, face_unknown) = geom::Face::LAYOUT.bytes();
        let (_, _, spline_unknown) = geom::Spline::LAYOUT.bytes();
        assert!(spline_unknown > 0 && face_unknown >= spline_unknown);
        // an attribute embeds a path it is only inferred to embed: read fields count as named
        let (read, _, _) = dict::AttGrpIndex::LAYOUT.bytes();
        assert_eq!(read, 0x14);
        let attr = dict::Attribute::LAYOUT;
        let own_read: usize = attr.fields.iter().filter(|f| f.confidence == Confidence::Read).map(|f| f.size).sum();
        assert_eq!(attr.bytes().0, own_read);
    }

    #[test]
    fn placeholder_types_have_the_original_sizes() {
        assert_eq!(core::mem::size_of::<Ptr32<Opaque>>(), 4);
        assert_eq!(core::mem::size_of::<Double>(), 8);
        assert_eq!(core::mem::align_of::<Double>(), 4);
        assert_eq!(core::mem::size_of::<Matrix>(), 64);
    }
}
