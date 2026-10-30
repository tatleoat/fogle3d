// SPDX-License-Identifier: MIT
//! Records the dictionaries use that `si-types` does not lay out (or lays out under another
//! reading). Each field's evidence is in finding 0026.

use si_mem::{record, CStr32, Opaque, Ptr32};

record! {
    /// What a custom effect tells the relation dictionary about a rule
    /// (`SI_reldictAddRelation` copies it field by field into the rule at +0x20).
    pub struct CusRelInfo = "SI_cusRelInfo", size 0x28 {
        0x00 name: CStr32, "the custom effect's name: to the rule's +0x20";
        0x04 info: [i32; 6], "six words copied to the rule's +0x24 to +0x38";
        0x1c new_src: i16, "to the rule's +0x3c (SI_reldictIsNewSrc reads it there)";
        0x1e max_fanout_dst: i16, "to the rule's +0x3e";
        0x20 max_fanout_src: i16, "to the rule's +0x40";
        0x22 w22: i16, "to the rule's +0x42";
        0x24 w24: i16, "to the rule's +0x44";
        0x26 pad_26: [u8; 2], "not read";
    }
}

record! {
    /// The data of a custom-effect relation (id 20000), as far as the dictionaries read
    /// it: two strings that name the rule.
    pub struct CustomRelData = "SI_CustomRelData", size 0x28 {
        0x00 head: [u32; 8], "not read by the dictionaries";
        0x20 name: CStr32, "compared with a rule's name";
        0x24 effect: CStr32, "compared with a rule's custom-effect name (+0x20)";
    }
}

record! {
    /// One step of an update list (`si_updlistAddNode` 0x08141488 makes it).
    pub struct UpdNode = "SI_UpdElem (list node)", size 0x2c {
        0x00 chapid: i16, "chapter of the element to update; -1 in the end mark";
        0x02 group: i16, "the relation list the step comes from";
        0x04 elemid: i32, "the element's index";
        0x08 data: Ptr32<Opaque>, "the element's object when the node was made";
        0x0c unset_0c: [u32; 2], "not set by si_updlistAddNode";
        0x14 relid: i32, "what to do: a relation id, or -100, -101, -200";
        0x18 nb_args: i16, "number of arguments";
        0x1a pad_1a: [u8; 2], "not set";
        0x1c args: Ptr32<UpdArg>, "the arguments, owned";
        0x20 active: i16, "1, or the active word of the entry whose data the node carries";
        0x22 pad_22: [u8; 2], "not set";
        0x24 rel_data: Ptr32<Opaque>, "the relation's data";
        0x28 extra: Ptr32<Opaque>, "null when made; freed with the node";
    }
}

record! {
    /// One argument of an update step (`si_updlistAddArgument` 0x081413e0 makes it).
    pub struct UpdArg = "SI_UpdElem (argument)", size 0x14 {
        0x00 chapid: i16, "chapter of the element at the other end";
        0x02 group: i16, "the list on that element that holds the matching entry";
        0x04 elemid: i32, "its index";
        0x08 data: Ptr32<Opaque>, "its object";
        0x0c active: i16, "the entry's active word";
        0x0e pad_0e: [u8; 2], "not set";
        0x10 active_data: Ptr32<Opaque>, "the object of the animation element the entry owns";
    }
}

record! {
    /// The condition under which a group of attributes applies (`SI_AttribCond`): an
    /// attribute and the numbers of the enumeration its value must be one of. The size
    /// is what the routines of `attdict.c` read; no allocation of the record is in that
    /// file (the resource reader makes it).
    pub struct AttribCond = "SI_AttribCond", size 0xc {
        0x00 attr: i32, "index of the attribute in the dictionary's array (SI_attdictEvalCond)";
        0x04 values: Ptr32<i32>, "the numbers, owned (attdictAttribGroupDispose frees the array)";
        0x08 nb: i32, "how many";
    }
}

record! {
    /// One entry of the attribute dictionary's index of curve ids (`SI_FcvATD_Index`):
    /// `SI_attdictSortAttribFromFcurveIds` makes the array with `SI_calloc(n, 0xc)`.
    pub struct FcvAtdIndex = "SI_FcvATD_Index", size 0xc {
        0x00 attr: i32, "index of the attribute in the dictionary's array (-1 in a search key)";
        0x04 component: i32, "which of its components (in a search key: the curve id)";
        0x08 flag: i16, "0 when the index is built; SI_attdictSetFcurveFlag sets it";
        0x0a pad_0a: [u8; 2], "not written";
    }
}
