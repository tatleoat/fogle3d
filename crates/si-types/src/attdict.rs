// SPDX-License-Identifier: MIT
//! The dictionary of attributes (`attdict.c`): the tree of attribute groups and the flat
//! array of attributes that the `ELEMENTS_ATTRIBUTES` part of the resource file describes.
//!
//! A group has a name, a condition, the indices of its attributes in the dictionary's one
//! array, sub-groups, and the two functions that read and write its attributes on an
//! element. An attribute is found by a path of group indices (`SI_AttGrpIndex`).

use crate::dict::{AttGrpIndex, AttribCond, AttribGroup, Attribute, AttributeDict};
use crate::{Code, Ptr32, Zero};

/// Address of `SI_dummyAttrGetValue` (0x080fabf0), the function a new group reads
/// attributes with until the resource reader installs the real one. Not read here.
pub const SI_DUMMY_ATTR_GET_VALUE: Ptr32<Code> = Ptr32::new(0x080f_abf0);
/// Address of `SI_dummyAttrSetValue` (0x080fabfc), its counterpart for writing.
pub const SI_DUMMY_ATTR_SET_VALUE: Ptr32<Code> = Ptr32::new(0x080f_abfc);

/// Bits of `Attribute::access`.
pub const ACCESS_WRITE: i16 = 2;
pub const ACCESS_ANIMATE: i16 = 4;

impl Attribute {
    /// [soft 0x0811e7d0 si_attdictInitAttribute]. It leaves `pad_06`, the last entry of
    /// the path, and the other padding as they were.
    pub fn init(&mut self) {
        self.unk_00 = 0;
        self.unk_04 = 0;
        self.index.depth = 0;
        for p in &mut self.index.path[..6] {
            *p = 0;
        }
        self.range_low[0] = 0;
        self.range_high[0] = 0;
        self.index.value_index = 0;
        self.id = 0;
        self.name = Ptr32::NULL;
        self.description = Ptr32::NULL;
        self.access = 0;
        self.scope = 0;
        self.value_type = 1;
        self.enum_type = 0;
        self.range_low[1] = 0;
        self.range_low_flag = 1;
        self.range_high[1] = 0;
        self.range_high_flag = 1;
        self.group = 0;
        self.rank = 0;
        self.visible = 0;
        self.nb_values = 0;
        self.fcv_ids = Ptr32::NULL;
        self.fcv_names = Ptr32::NULL;
        self.unk_60 = 0;
        self.incmode = 3;
    }

    /// [soft 0x081205fc SI_attdictAttributeIsAnimatable]: the animate bit, not normalised.
    pub fn is_animatable(&self) -> u16 {
        self.access as u16 & ACCESS_ANIMATE as u16
    }
}

impl AttribGroup {
    /// [soft 0x0811e890 attdictInitAttribGroup]. It leaves `pad_06` as it was.
    pub fn init(&mut self) {
        self.nb_attribs = 0;
        self.attribs = Ptr32::NULL;
        self.cond = Ptr32::NULL;
        self.nb_subs = 0;
        self.subs = Ptr32::NULL;
        self.name = Ptr32::NULL;
        self.flag = 1;
        self.rel_type = 0;
        self.get_value = SI_DUMMY_ATTR_GET_VALUE;
        self.set_value = SI_DUMMY_ATTR_SET_VALUE;
    }
}

impl AttributeDict {
    /// [soft 0x0811e8e0 SI_attdictInit]. It leaves `pad_1a` as it was.
    pub fn init(&mut self) {
        self.groups = Ptr32::NULL;
        self.nb_groups = 0;
        self.attributes = Ptr32::NULL;
        self.nb_attributes = 0;
        self.fcv_index = Ptr32::NULL;
        self.unk_14 = 0;
        self.dirty = 1;
    }
}

/// An attribute with the arrays and strings its record points to.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeStore {
    pub rec: Attribute,
    pub name: Option<Vec<u8>>,
    pub description: Option<Vec<u8>>,
    /// Parameter id of each component; `rec.nb_values` entries.
    pub fcv_ids: Vec<i16>,
    /// Short name of each component.
    pub fcv_names: Vec<Option<Vec<u8>>>,
}

impl Default for AttributeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AttributeStore {
    /// A cleared, initialised attribute.
    pub fn new() -> Self {
        let mut rec = Attribute::zero();
        rec.init();
        AttributeStore { rec, name: None, description: None, fcv_ids: Vec::new(), fcv_names: Vec::new() }
    }

    /// Gives the attribute `count` components: ids all -1, names all null
    /// [soft 0x0811e91c si_attdictAllocateFcurveArray].
    pub fn allocate_fcurve_array(&mut self, count: i32) {
        self.rec.nb_values = count as i16;
        self.fcv_ids = vec![-1; count.max(0) as usize];
        self.fcv_names = vec![None; count.max(0) as usize];
    }
}

/// A group with its name, its attribute indices and its sub-groups.
#[derive(Clone, Debug, PartialEq)]
pub struct AttribGroupStore {
    /// `nb_attribs` and `nb_subs` always equal the lengths below.
    pub rec: AttribGroup,
    pub name: Option<Vec<u8>>,
    pub attribs: Vec<i32>,
    pub subs: Vec<AttribGroupStore>,
}

impl AttribGroupStore {
    fn new(name: Option<&[u8]>, cond: Ptr32<AttribCond>) -> Self {
        let mut rec = AttribGroup::zero();
        rec.init();
        rec.cond = cond;
        AttribGroupStore { rec, name: name.map(<[u8]>::to_vec), attribs: Vec::new(), subs: Vec::new() }
    }

    /// Appends a sub-group and returns it [soft 0x0811eae0 si_attdictAddAttribSubGroup].
    /// (The original also accepts a null group and then returns null.)
    pub fn add_sub_group(&mut self, name: Option<&[u8]>, cond: Ptr32<AttribCond>) -> &mut AttribGroupStore {
        self.subs.push(AttribGroupStore::new(name, cond));
        self.rec.nb_subs += 1;
        self.subs.last_mut().unwrap()
    }
}

/// The attribute dictionary with its groups and attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct AttDictStore {
    /// `nb_groups` and `nb_attributes` always equal the lengths below.
    pub rec: AttributeDict,
    pub groups: Vec<AttribGroupStore>,
    pub attributes: Vec<AttributeStore>,
}

impl Default for AttDictStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Which group of the dictionary: a top-level index, then sub-group indices.
pub type GroupPath<'a> = &'a [usize];

impl AttDictStore {
    /// An empty dictionary [soft 0x0811e8e0 SI_attdictInit].
    pub fn new() -> Self {
        let mut rec = AttributeDict::zero();
        rec.init();
        AttDictStore { rec, groups: Vec::new(), attributes: Vec::new() }
    }

    /// Appends a top-level group and returns its index
    /// [soft 0x0811ea5c si_attdictAddAttribGroup]. The first group of a dictionary gets
    /// flag 0, the others 1.
    pub fn add_attrib_group(&mut self, name: Option<&[u8]>, cond: Ptr32<AttribCond>, rel_type: i32) -> usize {
        let index = self.groups.len();
        let mut g = AttribGroupStore::new(name, cond);
        g.rec.rel_type = rel_type;
        if index == 0 {
            g.rec.flag = 0;
        }
        self.groups.push(g);
        self.rec.nb_groups += 1;
        self.rec.dirty = 1;
        index
    }

    /// The group a path leads to.
    pub fn group_mut(&mut self, path: GroupPath) -> Option<&mut AttribGroupStore> {
        let (first, rest) = path.split_first()?;
        let mut g = self.groups.get_mut(*first)?;
        for i in rest {
            g = g.subs.get_mut(*i)?;
        }
        Some(g)
    }

    /// Appends a copy of `attr` to the dictionary and its index to the group at `group`
    /// [soft 0x0811e984 si_attdictAddAttribute]. Returns the attribute's index, or `None`
    /// when the path names no group (the original takes the group by pointer).
    pub fn add_attribute(&mut self, group: GroupPath, attr: &AttributeStore) -> Option<i32> {
        self.group_mut(group)?;
        let index = self.rec.nb_attributes;
        // the original initialises the new slot and then copies all 100 bytes over it
        self.attributes.push(attr.clone());
        self.rec.nb_attributes += 1;
        self.rec.dirty = 1;
        let g = self.group_mut(group).unwrap();
        g.attribs.push(index);
        g.rec.nb_attribs += 1;
        Some(index)
    }

    /// Frees everything and re-initialises [soft 0x0811ec1c SI_attdictDispose]
    /// [soft 0x0811eb58 attdictAttribGroupDispose].
    pub fn dispose(&mut self) {
        self.attributes.clear();
        self.groups.clear();
        self.rec.init();
    }

    /// The 16-bit value at a byte offset of an index record, as the original's pointer
    /// arithmetic reads it; `None` past the record.
    fn short_at(index: &AttGrpIndex, offset: usize) -> Option<i16> {
        match offset {
            0 => Some(index.depth),
            2 | 4 | 6 | 8 | 10 | 12 | 14 => Some(index.path[(offset - 2) / 2]),
            16 => Some(index.value_index as i16),
            18 => Some((index.value_index >> 16) as i16),
            _ => None,
        }
    }

    /// The index, in `attributes`, of the attribute a path names
    /// [soft 0x0811ef0c SI_attdictAttributeGetByIndex]; `None` where the original returns
    /// null.
    ///
    /// `path[0]` is a top-level group, `path[1]` to `path[depth-2]` sub-groups, and
    /// `path[depth-1]` the position of the attribute in the group reached. With a depth
    /// of 1 the one entry is used both as group and as position. A depth above 8 makes the
    /// original read beyond the path (into the component index, then past the record); the
    /// part past the record is `None` here.
    pub fn attribute_get_by_index(&self, index: &AttGrpIndex) -> Option<usize> {
        let depth = index.depth as i32;
        let first = index.path[0] as i32;
        if !(0 < depth && -1 < first && first < self.rec.nb_groups) {
            return None;
        }
        let mut g = &self.groups[first as usize];
        for level in 1..depth - 1 {
            let s = Self::short_at(index, (level as usize) * 2 + 2)? as i32;
            if s < 0 || g.rec.nb_subs <= s {
                return None;
            }
            g = &g.subs[s as usize];
        }
        let a = Self::short_at(index, depth as usize * 2)? as i32;
        if -1 < a && a < g.rec.nb_attribs {
            Some(g.attribs[a as usize] as usize)
        } else {
            None
        }
    }

    /// The default column width for an attribute, by its value type
    /// [soft 0x081203d0 SI_attdictAttribGetDefWidth]: 10 when the path names nothing or the
    /// type is not 1 to 5.
    pub fn attrib_get_def_width(&self, index: &AttGrpIndex) -> i16 {
        match self.attribute_get_by_index(index).map(|i| self.attributes[i].rec.value_type) {
            Some(1) => 7,
            Some(2) => 3,
            Some(3) => 5,
            Some(4) => 6,
            Some(5) => 15,
            _ => 10,
        }
    }
}

/// True when two paths have the same depth, the same component index and the same
/// `depth` entries [soft 0x08120610 SI_attdictCompareGrpIndex]. (The original also
/// answers false for a null pointer. A depth above 7 makes it compare past the path.)
pub fn compare_grp_index(a: &AttGrpIndex, b: &AttGrpIndex) -> bool {
    if a.depth != b.depth || a.value_index != b.value_index {
        return false;
    }
    (0..a.depth.max(0) as usize).all(|i| {
        let off = 2 + 2 * i;
        AttDictStore::short_at(a, off) == AttDictStore::short_at(b, off)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &[u8], value_type: i32, ids: &[i16]) -> AttributeStore {
        let mut a = AttributeStore::new();
        a.name = Some(name.to_vec());
        a.rec.value_type = value_type;
        a.allocate_fcurve_array(ids.len() as i32);
        a.fcv_ids.copy_from_slice(ids);
        a
    }

    fn index(path: &[i16], value_index: i32) -> AttGrpIndex {
        let mut i = AttGrpIndex::zero();
        i.depth = path.len() as i16;
        i.path[..path.len()].copy_from_slice(path);
        i.value_index = value_index;
        i
    }

    #[test]
    fn initial_values() {
        let a = AttributeStore::new();
        assert_eq!((a.rec.value_type, a.rec.incmode, a.rec.range_low_flag, a.rec.range_high_flag), (1, 3, 1, 1));
        assert_eq!((a.rec.id, a.rec.nb_values, a.rec.access), (0, 0, 0));
        let d = AttDictStore::new();
        assert_eq!((d.rec.nb_groups, d.rec.nb_attributes, d.rec.dirty), (0, 0, 1));
        let mut g = AttribGroup::zero();
        g.init();
        assert_eq!((g.flag, g.get_value, g.set_value), (1, SI_DUMMY_ATTR_GET_VALUE, SI_DUMMY_ATTR_SET_VALUE));
        let mut b = attr(b"x", 1, &[]);
        b.allocate_fcurve_array(3);
        assert_eq!((b.rec.nb_values, &b.fcv_ids[..], b.fcv_names.len()), (3, &[-1i16, -1, -1][..], 3));
    }

    #[test]
    fn attributes_are_found_by_path() {
        let mut d = AttDictStore::new();
        let general = d.add_attrib_group(Some(b"General_Element"), Ptr32::NULL, 0);
        let other = d.add_attrib_group(Some(b"Other"), Ptr32::NULL, 5);
        assert_eq!((d.groups[general].rec.flag, d.groups[other].rec.flag, d.groups[other].rec.rel_type), (0, 1, 5));
        d.add_attribute(&[general], &attr(b"Chapter", 4, &[260])).unwrap();
        d.add_attribute(&[general], &attr(b"Name", 2, &[262])).unwrap();
        d.groups[general].add_sub_group(Some(b"MODELS"), Ptr32::NULL);
        d.groups[general].subs[0].add_sub_group(Some(b"Geometry"), Ptr32::NULL);
        let scal = d.add_attribute(&[general, 0], &attr(b"Scaling", 5, &[1, 2, 3])).unwrap();
        let nbv = d.add_attribute(&[general, 0, 0], &attr(b"NbVertices", 3, &[280])).unwrap();
        assert_eq!((scal, nbv, d.rec.nb_attributes), (2, 3, 4));
        assert_eq!(d.add_attribute(&[7], &attr(b"x", 1, &[])), None);

        // group 0, attribute 1
        assert_eq!(d.attribute_get_by_index(&index(&[0, 1], 0)), Some(1));
        // group 0, sub-group 0, attribute 0
        assert_eq!(d.attribute_get_by_index(&index(&[0, 0, 0], 2)), Some(2));
        assert_eq!(d.attribute_get_by_index(&index(&[0, 0, 0, 0], 0)), Some(3));
        // out of range at each level
        assert_eq!(d.attribute_get_by_index(&index(&[2, 0], 0)), None);
        assert_eq!(d.attribute_get_by_index(&index(&[0, 1, 0], 0)), None);
        assert_eq!(d.attribute_get_by_index(&index(&[0, 5], 0)), None);
        assert_eq!(d.attribute_get_by_index(&index(&[], 0)), None);
        // depth 1: the entry is both the group and the position in it
        assert_eq!(d.attribute_get_by_index(&index(&[0], 0)), Some(0));
        assert_eq!(d.attribute_get_by_index(&index(&[1], 0)), None);

        assert_eq!(d.attrib_get_def_width(&index(&[0, 0], 0)), 6); // type 4
        assert_eq!(d.attrib_get_def_width(&index(&[0, 1], 0)), 3); // type 2
        assert_eq!(d.attrib_get_def_width(&index(&[0, 0, 0], 0)), 15); // type 5
        assert_eq!(d.attrib_get_def_width(&index(&[1, 0], 0)), 10);

        assert!(compare_grp_index(&index(&[0, 0, 0], 2), &index(&[0, 0, 0], 2)));
        assert!(!compare_grp_index(&index(&[0, 0, 0], 2), &index(&[0, 0, 0], 1)));
        assert!(!compare_grp_index(&index(&[0, 0, 0], 2), &index(&[0, 0], 2)));
        assert!(!compare_grp_index(&index(&[0, 0, 1], 2), &index(&[0, 0, 0], 2)));

        let mut w = attr(b"w", 1, &[]);
        w.rec.access = ACCESS_WRITE | ACCESS_ANIMATE | 1;
        assert_eq!((w.rec.is_animatable(), attr(b"r", 1, &[]).rec.is_animatable()), (4, 0));

        d.dispose();
        assert_eq!((d.rec.nb_groups, d.rec.nb_attributes, d.rec.dirty, d.groups.len()), (0, 0, 1, 0));
    }
}
