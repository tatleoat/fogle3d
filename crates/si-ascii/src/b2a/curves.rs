// SPDX-License-Identifier: MIT
//! The name of a function curve's type, as the text form prints it after `CURVE_TYPE`: the
//! component name the attribute dictionary (`SI_ATD`) gives the curve id. The dictionary is
//! the one the relations resources build ([`crate::srel`]); here its curve index is built on
//! those values instead of on the memory image (`si-dict` has the image form).

use crate::srel::Relations;

/// The custom-effect curve id, which no attribute answers for (`SI_attdictGetNameFromFcurve`
/// tests it first).
pub const FCV_CUSTOM: i32 = 0x236;

/// What `SI_attdictGetNameFromFcurve` answers when it finds nothing: the static
/// `default_name`, which points at the literal `notfound`.
pub const NOT_FOUND: &[u8] = b"notfound";

/// One entry of the curve index (`FcvAtdIndex`, 12 bytes): the attribute, the component.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry {
    attr: usize,
    component: usize,
}

/// One attribute as the index sees it: its curve ids and component names, `nb_values` of
/// each (+0x56, +0x58, +0x5c of the record).
#[derive(Clone, Debug, PartialEq)]
struct Attr {
    ids: Vec<i16>,
    names: Vec<Option<Vec<u8>>>,
}

/// The attribute dictionary's curve index and the two statics of `SI_attdictFindFcurve`.
#[derive(Clone, Debug)]
pub struct CurveNames {
    attrs: Vec<Attr>,
    /// `fcv_index` (+0x10), `None` when the dictionary has no groups (the sort makes none)
    index: Option<Vec<Entry>>,
    /// `last_sort_ind.327` and `last_fcv_id.328`, both −1 at start.
    last_sort_ind: i32,
    last_fcv_id: i32,
}

impl CurveNames {
    /// The dictionary as the relations reader leaves it, with its curve index built
    /// [soft 0x0811fc80 SI_attdictSortAttribFromFcurveIds]: one entry per component of each
    /// attribute, in the attributes' order ([soft 0x0811fc14 attdictInitFcvIndexes]), sorted
    /// by curve id with `qsort` ([soft 0x0811fbb8 attdictCompareIds]: the 16-bit ids,
    /// signed). glibc's `qsort` is a merge sort while it can allocate, so entries of one id
    /// keep their order; a stable sort does the same here.
    pub fn new(rel: &Relations) -> CurveNames {
        let attrs: Vec<Attr> = rel
            .attributes
            .iter()
            .map(|a| {
                // nb_values is the fcurve count (+0x56); the arrays have that many entries
                let n = a.fcurves.max(0) as usize;
                let mut ids = a.fcurve_ids.clone();
                ids.resize(n.max(ids.len()), -1);
                let mut names = a.fcurve_names.clone();
                names.resize(n.max(names.len()), None);
                Attr { ids: ids[..n].to_vec(), names: names[..n].to_vec() }
            })
            .collect();
        let index = if rel.groups.is_empty() {
            None
        } else {
            let mut v = Vec::new();
            for (i, a) in attrs.iter().enumerate() {
                for k in 0..a.ids.len() {
                    v.push(Entry { attr: i, component: k });
                }
            }
            v.sort_by_key(|e| attrs[e.attr].ids[e.component]);
            Some(v)
        };
        CurveNames { attrs, index, last_sort_ind: -1, last_fcv_id: -1 }
    }

    fn id_of(&self, e: Entry) -> i16 {
        self.attrs[e.attr].ids[e.component]
    }

    /// [soft 0x0811fdfc SI_attdictFindFcurve]: the position in the curve index of a curve
    /// id, or −1.
    ///
    /// The last answer is remembered: the same id answers at once, and the id one above
    /// answers with the next entry when that entry is the next component of the same
    /// attribute and has this id. Otherwise the index is searched with glibc's `bsearch`
    /// (bisection on `(l + u) / 2`; [soft 0x0811fd10 attdictBSearchCompareIds] compares the
    /// key's full id with an entry's 16-bit one), so of several entries with one id the one
    /// the bisection meets first answers.
    pub fn find_fcurve(&mut self, id: i32) -> i32 {
        if self.attrs.is_empty() {
            return -1;
        }
        let Some(index) = self.index.as_ref() else { return -1 };
        let n = index.len() as i32;
        let last = self.last_sort_ind;
        if last != -1 && last < n {
            let e = index[last as usize];
            if id == self.last_fcv_id {
                return last;
            }
            if self.last_fcv_id.wrapping_add(1) == id {
                let a = &self.attrs[e.attr];
                let next = e.component + 1;
                if next < a.ids.len() && a.ids[next] as i32 == id && last + 1 < n && index[(last + 1) as usize].component == next {
                    self.last_sort_ind = last + 1;
                    self.last_fcv_id = id;
                    return last + 1;
                }
            }
        }
        let (mut l, mut u) = (0usize, index.len());
        let mut found = None;
        while l < u {
            let idx = (l + u) / 2;
            let v = self.id_of(index[idx]) as i32;
            if id < v {
                u = idx;
            } else if id > v {
                l = idx + 1;
            } else {
                found = Some(idx);
                break;
            }
        }
        let Some(at) = found else { return -1 };
        self.last_fcv_id = id;
        self.last_sort_ind = at as i32;
        at as i32
    }

    /// [soft 0x0810f640 SI_fcvGetNameFromType] → [soft 0x0812012c SI_attdictGetNameFromFcurve]:
    /// the component name of a curve id. `None` is a null name pointer (an attribute that
    /// gave fewer names than components), which `printf("%s")` prints as `(null)`.
    pub fn name(&mut self, id: i32) -> Option<Vec<u8>> {
        if id == FCV_CUSTOM || self.attrs.is_empty() || self.index.is_none() {
            return Some(NOT_FOUND.to_vec());
        }
        let at = self.find_fcurve(id);
        if at == -1 {
            return Some(NOT_FOUND.to_vec());
        }
        let e = self.index.as_ref().unwrap()[at as usize];
        self.attrs[e.attr].names[e.component].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srel::{AttribGroup, Attribute};

    fn attr(ids: &[i16], names: &[&str]) -> Attribute {
        let mut a = Attribute::init();
        a.fcurves = ids.len() as i16;
        a.fcurve_ids = ids.to_vec();
        a.fcurve_names = names.iter().map(|n| Some(n.as_bytes().to_vec())).collect();
        a
    }

    #[test]
    fn names_by_id_with_duplicates_and_the_cache() {
        let mut rel = Relations::new();
        rel.groups.push(AttribGroup { name: b"g".to_vec(), ..Default::default() });
        rel.attributes.push(attr(&[30, 31, 32], &["x", "y", "z"]));
        rel.attributes.push(attr(&[10], &["first"]));
        rel.attributes.push(attr(&[10], &["second"]));
        let mut c = CurveNames::new(&rel);
        // sorted: 10 first, 10 second, 30, 31, 32: the bisection of five meets index 2 first,
        // then index 1, the second of the two entries of id 10
        assert_eq!(c.name(10), Some(b"second".to_vec()));
        assert_eq!(c.name(31), Some(b"y".to_vec()));
        assert_eq!(c.name(32), Some(b"z".to_vec())); // through the cache
        assert_eq!(c.name(11), Some(NOT_FOUND.to_vec()));
        assert_eq!(c.name(FCV_CUSTOM), Some(NOT_FOUND.to_vec()));
        // no groups: no index
        let mut rel = Relations::new();
        rel.attributes.push(attr(&[1], &["a"]));
        assert_eq!(CurveNames::new(&rel).name(1), Some(NOT_FOUND.to_vec()));
    }
}
