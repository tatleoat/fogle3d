// SPDX-License-Identifier: MIT
//! What the expression code asks of the rest of the program (finding 0013).
//!
//! The scanner, the parser and the printer look words up in three dictionaries that the
//! program fills at start-up from `Relations.rsrc` (finding 0006, section 7): enumerations,
//! attributes (which give the function-curve names) and relations. With arguments being
//! validated they also look elements up in the scene. All of that is behind [`Dict`];
//! [`Rsrc`] answers the dictionary questions from a `Relations.rsrc` read by `si-ascii` and
//! has no scene.

// adapter: not in place; the dictionary questions as a trait; on the image `crate::image::io::ImageDict` answers
// them from si-dict's dictionaries.

use si_ascii::srel::Relations;

/// Dictionary and scene look-ups of the scanner, the parser and the printer. Each method
/// stands for one original function. The scene methods have defaults that answer "there is
/// no such thing", which is what the original gets from an empty scene.
pub trait Dict {
    /// The first value, in any enumeration after number `*enum_type` (−1 to start with the
    /// first), whose code is `code`: sets `*enum_type` to its enumeration and returns its id
    /// [soft 0x081388d8 SI_enmdictEnumValGetByAnyCode]. `None` leaves `*enum_type` moved on
    /// by one, as the original does.
    fn enum_val_by_any_code(&self, enum_type: &mut i32, code: &[u8]) -> Option<i32>;

    /// The code of the value `id` of enumeration `enum_type`
    /// [soft 0x08138834 SI_enmdictEnumCodeGetByValue].
    fn enum_code_by_value(&self, enum_type: i32, id: i32) -> Option<Vec<u8>>;

    /// The id of the relation called `name`, −1 if there is none
    /// `SI_reldictGetRelIdByName` (0x0813a5bc).
    fn rel_id_by_name(&self, name: &[u8]) -> i32;

    /// The name of relation `id` `SI_reldictGetRelName` (0x0813a0f4).
    fn rel_name(&self, id: i32) -> Vec<u8>;

    /// The fcurve type called `name`, −1 if there is none
    /// [soft 0x0811ffa8 SI_attdictGetFcurveFromName].
    fn fcurve_from_name(&self, name: &[u8]) -> i32;

    /// The name of fcurve type `fcv` [soft 0x0812012c SI_attdictGetNameFromFcurve].
    fn name_from_fcurve(&self, fcv: i32) -> Vec<u8>;

    /// The index of the element called `name` in `chapter`, −1 if there is none: the
    /// parser's look-up when arguments are validated, by name alone
    /// `SI_elemdictFindElemByNameOnly` (0x08134770) or, when the name holds a dash, by
    /// prefix and name `SI_elemdictFindElemByPrjName` (0x08134678).
    fn find_elem(&self, _chapter: i32, _prefix: Option<&[u8]>, _name: &[u8], _new_only: i16) -> i32 {
        -1
    }

    /// Whether element `elem` of `chapter` exists `SI_elemdictGetElem` (0x08132b50).
    fn elem_exists(&self, _chapter: i32, _elem: i32) -> bool {
        false
    }

    /// The element's name as the dialog shows it: prefix, dash, name
    /// `SI_dbFormatPrefixName` (0x08121ca8) of the element's own record. `None` if the
    /// element or its name is missing.
    fn elem_prefix_name(&self, _chapter: i32, _elem: i32) -> Option<Vec<u8>> {
        None
    }

    /// The name of the cluster with id `cluster` of a model
    /// `SI_edGetClusterFromModel`; `None` if the element or the cluster is missing.
    fn cluster_name(&self, _chapter: i32, _elem: i32, _cluster: i32) -> Option<Vec<u8>> {
        None
    }

    /// A driver and one of its degrees of freedom by their names
    /// `SI_chnMatchNames`: the two indexes, or `None`.
    fn chn_match_names(&self, _driver: &[u8], _dof: &[u8]) -> Option<(i32, i32)> {
        None
    }

    /// The kind of a degree of freedom: 1 input, 2 output `SI_chnGetDofType`.
    fn chn_dof_type(&self, _driver: i32, _dof: i32) -> i32 {
        0
    }

    /// The names of a driver and a degree of freedom `SI_chnGetDriverAndDofNames` (0x0855d76c).
    fn chn_names(&self, _driver: i32, _dof: i32) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (None, None)
    }
}

/// The name [`Dict::name_from_fcurve`] gives for an fcurve type the dictionary does not have
/// (the static `default_name` of the original, which points at this text).
pub const DEFAULT_FCURVE_NAME: &[u8] = b"notf";

/// One entry of the attribute dictionary's fcurve index: attribute, place among the
/// attribute's fcurves, and (looked up through them) id and name.
#[derive(Clone, Debug)]
struct FcvIndex {
    id: i16,
    name: Option<Vec<u8>>,
}

/// The three dictionaries, from a `Relations.rsrc`.
pub struct Rsrc {
    rel: Relations,
    /// The fcurve index, sorted by id [soft 0x0811fc80 SI_attdictSortAttribFromFcurveIds].
    index: Vec<FcvIndex>,
}

impl Rsrc {
    /// Builds the fcurve index the original builds after reading the file: one entry per
    /// fcurve of every attribute in file order [soft 0x0811fc14 attdictInitFcvIndexes],
    /// sorted by fcurve id [soft 0x0811fbb8 attdictCompareIds]. The original sorts with the
    /// C library's `qsort`; entries with equal ids keep their file order here (the library's
    /// merge sort does the same; with another `qsort` their order could differ).
    pub fn new(rel: Relations) -> Rsrc {
        let mut index = Vec::new();
        for a in &rel.attributes {
            for k in 0..a.fcurves.max(0) as usize {
                index.push(FcvIndex {
                    id: a.fcurve_ids.get(k).copied().unwrap_or(-1),
                    name: a.fcurve_names.get(k).cloned().flatten(),
                });
            }
        }
        index.sort_by_key(|e| e.id);
        Rsrc { rel, index }
    }

    /// Reads a `Relations.rsrc`.
    pub fn read(data: &[u8]) -> Result<Rsrc, String> {
        let (rel, _) = si_ascii::srel::read(data).map_err(|e| format!("{e:?}"))?;
        Ok(Rsrc::new(rel))
    }

    pub fn relations(&self) -> &Relations {
        &self.rel
    }

    /// The place of fcurve type `fcv` in the index, as the C library's `bsearch` finds it
    /// [soft 0x0811fdfc SI_attdictFindFcurve]. (The original also remembers its last answer
    /// and tries the entry after it first; that shortcut gives the same entry unless two
    /// entries share an id.)
    fn find_fcurve(&self, fcv: i32) -> Option<usize> {
        let (mut lo, mut hi) = (0usize, self.index.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let id = self.index[mid].id as i32;
            if fcv < id {
                hi = mid;
            } else if fcv > id {
                lo = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }

    /// Ids that more than one index entry carries, with the names: where the answer of
    /// [`Dict::name_from_fcurve`] depends on the search.
    pub fn duplicate_ids(&self) -> Vec<(i16, Vec<String>)> {
        let mut out: Vec<(i16, Vec<String>)> = Vec::new();
        for w in self.index.windows(2) {
            if w[0].id == w[1].id {
                let n = |e: &FcvIndex| String::from_utf8_lossy(e.name.as_deref().unwrap_or(b"")).into_owned();
                match out.last_mut() {
                    Some((id, names)) if *id == w[0].id => names.push(n(&w[1])),
                    _ => out.push((w[0].id, vec![n(&w[0]), n(&w[1])])),
                }
            }
        }
        out
    }

    /// Every (id, name) of the index, in its order.
    pub fn fcurves(&self) -> impl Iterator<Item = (i16, Option<&[u8]>)> {
        self.index.iter().map(|e| (e.id, e.name.as_deref()))
    }
}

impl Dict for Rsrc {
    fn enum_val_by_any_code(&self, enum_type: &mut i32, code: &[u8]) -> Option<i32> {
        let n = self.rel.enums.len() as i32;
        if *enum_type >= n {
            return None;
        }
        *enum_type = if *enum_type == -1 { 0 } else { *enum_type + 1 };
        // a start below −1 indexes in front of the array in the original; Rust starts at 0
        let mut i = (*enum_type).max(0);
        while i < n {
            if let Some(v) = self.rel.enums[i as usize].values.iter().find(|v| v.code == code) {
                *enum_type = i;
                return Some(v.id);
            }
            i += 1;
        }
        None
    }

    fn enum_code_by_value(&self, enum_type: i32, id: i32) -> Option<Vec<u8>> {
        if enum_type < 0 {
            return None;
        }
        let e = self.rel.enums.get(enum_type as usize)?;
        e.values.iter().find(|v| v.id == id).map(|v| v.code.clone())
    }

    fn rel_id_by_name(&self, name: &[u8]) -> i32 {
        self.rel.rel_id_by_name(name)
    }

    fn rel_name(&self, id: i32) -> Vec<u8> {
        self.rel.rel_name(id).to_vec()
    }

    fn fcurve_from_name(&self, name: &[u8]) -> i32 {
        for e in &self.index {
            // an fcurve the file gave no name is a null pointer there: `strcmp` on it would
            // fault in the original; none is nameless in the shipped file
            if e.name.as_deref() == Some(name) {
                return e.id as i32;
            }
        }
        // four names the dictionary does not carry; the comparison takes seven bytes, the
        // terminator included
        match name {
            b"hidden" => 0xaf,
            b"camhid" => 0xb1,
            b"lighid" => 0xb0,
            b"t2norm" => 0x2b,
            _ => -1,
        }
    }

    fn name_from_fcurve(&self, fcv: i32) -> Vec<u8> {
        if fcv == crate::tree::FCV_CUSTOM {
            return DEFAULT_FCURVE_NAME.to_vec();
        }
        match self.find_fcurve(fcv) {
            // a nameless fcurve is a null pointer in the original; the callers print it
            // with "%s", which the C library shows as "(null)"
            Some(i) => self.index[i].name.clone().unwrap_or_else(|| b"(null)".to_vec()),
            None => DEFAULT_FCURVE_NAME.to_vec(),
        }
    }
}
