// SPDX-License-Identifier: MIT
//! The records of this crate as values that can lie in `si-mem`'s memory image
//! (finding 0024): [`si_mem::Pod`] for every layout, so that `mem[ptr]` is the record at a
//! 32-bit address, in place.
//!
//! Nothing here reproduces a function of the original.
//!
//! Why each record qualifies (the trait's three conditions):
//!
//! - *The fields tile the structure and its alignment is at most 4*: asserted below at
//!   compile time, from the same field table the layout macro builds (which already asserts
//!   the size and each field's offset).
//! - *Every field is itself `Pod`*: the layout macro builds a record's zero value from
//!   `Zero::zero()` of every field, so every field type implements [`crate::Zero`]; and
//!   `Zero` is implemented in this crate for exactly the integers and `f32`, arrays of such
//!   types, `Ptr32`, [`crate::Double`] and the layouts themselves. All of those are `Pod`
//!   (the layouts by this file, inductively). Whoever adds a `Zero` implementation for a
//!   type that has an invalid bit pattern or padding (a `bool`, an enum with values, a
//!   plain `f64`) must take the record that uses it out of the list below.
//! - *Every bit pattern is a value*: follows from the fields.

use crate::{dict, geom, model, scene, Layout};
use si_mem::Pod;

// a `double` aligned to 4: eight bytes, any bits

/// True when the fields of a layout follow one another without gap or overlap and end at
/// its size (what `Layout::is_tiled` tests at run time).
const fn tiled(l: &Layout) -> bool {
    let mut at = 0;
    let mut i = 0;
    while i < l.fields.len() {
        if l.fields[i].offset != at {
            return false;
        }
        at += l.fields[i].size;
        i += 1;
    }
    at == l.size
}

macro_rules! pod_records {
    ($($m:ident :: $t:ident),* $(,)?) => {
        $(
            const _: () = {
                assert!(tiled(&$m::$t::LAYOUT));
                assert!(core::mem::align_of::<$m::$t>() <= 4);
                assert!(core::mem::size_of::<$m::$t>() == $m::$t::LAYOUT.size);
            };
            // see the module comment
            unsafe impl Pod for $m::$t {}
        )*
        /// The names of the records that are `Pod`, in the order of [`crate::all_layouts`].
        pub const POD_RECORDS: &[&str] = &[ $( stringify!($t) ),* ];
    };
}

pod_records! {
    dict::Elem, dict::ElemDict, dict::ListOptions, dict::ElemList, dict::DbListNode, dict::Relations,
    dict::RelList, dict::ARelation, dict::RelDict, dict::RelRule, dict::SortTbl, dict::UpdList,
    dict::ChapIds, dict::ChapInfo, dict::ElemIdentity, dict::AttributeValue, dict::AttGrpIndex,
    dict::AttributeDict, dict::AttribGroup, dict::Attribute, dict::EnumValDict, dict::EnumValType,
    dict::EnumVal, model::Model, geom::Vector, scene::GrafNode, scene::GrafList, geom::Mesh,
    geom::Vertex, geom::Polygon, geom::PolyNode, geom::Edge, geom::Face, geom::Spline,
    geom::SplineKey, geom::Patch, geom::NurbsDir, geom::NurbsCrv, geom::NurbsPoint, geom::NurbsSrf,
    scene::Material, scene::LclMap, scene::Texture3D, scene::Camera, scene::Light,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ptr32;
    use si_mem::Mem;

    #[test]
    fn every_layout_is_pod() {
        let names: Vec<&str> = crate::all_layouts().iter().map(|l| l.name).collect();
        assert_eq!(names, POD_RECORDS);
    }

    #[test]
    fn a_model_node_in_the_image() {
        // a node and its son as the original has them: two heap blocks linked by pointers
        let mut m = Mem::new();
        let father: Ptr32<model::Model> = Ptr32::new(m.calloc(1, model::Model::SIZE as u32));
        let son: Ptr32<model::Model> = Ptr32::new(m.calloc(1, model::Model::SIZE as u32));
        m[father].son = son;
        m[son].father = father;
        m[son].kind = model::MSMSH;
        m[son].scal = geom::Vector { x: 1.0, y: 2.0, z: 3.0 };
        let s = m[father].son;
        assert_eq!(m[s].kind, model::MSMSH);
        // the same bytes at the original's offsets
        assert_eq!(m.u32(father.0 + 0x2c4), son.0);
        assert_eq!(m.u32(son.0 + 0x2bc), father.0);
        assert_eq!(m.f32(son.0 + 0x104), 2.0);
        // a pointer to a field, followed
        let scal: Ptr32<geom::Vector> = si_mem::field!(son, model::Model, scal);
        assert_eq!(m[scal].z, 3.0);
        // a null link is a recorded fault, not a panic
        let none = m[son].son;
        assert_eq!(m[none].kind, 0);
        assert!(m.fault().is_some());
    }
}
