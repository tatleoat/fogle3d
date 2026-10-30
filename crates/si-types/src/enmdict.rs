// SPDX-License-Identifier: MIT
//! The dictionary of enumerated types (`enmdict.c`): the `ENUM` blocks of the resource file
//! (chapter types, model types, light types, …), each a list of (number, code, name).

use crate::dict::{EnumVal, EnumValDict, EnumValType};
use crate::Ptr32;

/// One value of an enumerated type, with its strings.
#[derive(Clone, Debug, PartialEq)]
pub struct EnumValStore {
    pub id: i32,
    pub code: Option<Vec<u8>>,
    pub name: Option<Vec<u8>>,
}

impl EnumValStore {
    pub fn record(&self) -> EnumVal {
        EnumVal { id: self.id, code: Ptr32::NULL, name: Ptr32::NULL }
    }
}

/// One enumerated type with its values.
#[derive(Clone, Debug, PartialEq)]
pub struct EnumTypeStore {
    pub name: Option<Vec<u8>>,
    pub values: Vec<EnumValStore>,
}

impl EnumTypeStore {
    pub fn record(&self) -> EnumValType {
        EnumValType { nb_values: self.values.len() as i32, values: Ptr32::NULL, name: Ptr32::NULL }
    }

    /// Appends a value [soft 0x08138520 si_enmdictAddValue]. The original keeps the two
    /// string pointers it is given; here the strings are owned.
    pub fn add_value(&mut self, id: i32, code: Option<&[u8]>, name: Option<&[u8]>) {
        self.values.push(EnumValStore { id, code: code.map(<[u8]>::to_vec), name: name.map(<[u8]>::to_vec) });
    }
}

/// The dictionary with its types.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnumDictStore {
    pub types: Vec<EnumTypeStore>,
}

impl EnumValDict {
    /// [soft 0x081385f4 SI_enmdictInit]
    pub fn init(&mut self) {
        self.nb_types = 0;
        self.types = Ptr32::NULL;
    }
}

impl EnumDictStore {
    /// An empty dictionary [soft 0x081385f4 SI_enmdictInit].
    pub fn new() -> Self {
        EnumDictStore { types: Vec::new() }
    }

    pub fn record(&self) -> EnumValDict {
        EnumValDict { nb_types: self.types.len() as i32, types: Ptr32::NULL }
    }

    /// Appends an empty type and returns its index [soft 0x08138588 si_enmdictAddEnumType]
    /// (the original returns a pointer to the new type).
    pub fn add_enum_type(&mut self, name: Option<&[u8]>) -> usize {
        self.types.push(EnumTypeStore { name: name.map(<[u8]>::to_vec), values: Vec::new() });
        self.types.len() - 1
    }

    /// Frees everything and re-initialises [soft 0x0813860c SI_enmdictDispose].
    pub fn dispose(&mut self) {
        self.types.clear();
    }

    fn type_at(&self, type_index: i32) -> Option<&EnumTypeStore> {
        if type_index < 0 { None } else { self.types.get(type_index as usize) }
    }

    /// The value of a type with this code [soft 0x081387c0 SI_enmdictEnumValGetByCode].
    /// (A value without a code makes the original call `strcmp` on a null pointer; here it
    /// simply does not match.)
    pub fn enum_val_get_by_code(&self, type_index: i32, code: &[u8]) -> Option<&EnumValStore> {
        self.type_at(type_index)?.values.iter().find(|v| v.code.as_deref() == Some(code))
    }

    /// The code of the first value of a type with this number
    /// [soft 0x08138834 SI_enmdictEnumCodeGetByValue].
    pub fn enum_code_get_by_value(&self, type_index: i32, id: i32) -> Option<&[u8]> {
        self.type_at(type_index)?.values.iter().find(|v| v.id == id)?.code.as_deref()
    }

    /// Index of the type with this name, or -1 [soft 0x08138888 SI_enmdictEnumTypeGetName].
    pub fn enum_type_get_name(&self, name: &[u8]) -> i32 {
        self.types.iter().position(|t| t.name.as_deref() == Some(name)).map_or(-1, |i| i as i32)
    }

    /// Searches every type after `*type_index` (or from the first when it is -1) for a
    /// value with this code [soft 0x081388d8 SI_enmdictEnumValGetByAnyCode]. On success
    /// `*type_index` is the type found. On failure it is left one past where it started,
    /// as the original leaves it; when it starts at or past the number of types nothing
    /// changes. A start below -1 makes the original index before the array; here that
    /// finds nothing.
    pub fn enum_val_get_by_any_code(&self, type_index: &mut i32, code: Option<&[u8]>) -> Option<&EnumValStore> {
        let nb = self.types.len() as i32;
        let code = code?;
        if *type_index >= nb {
            return None;
        }
        *type_index = if *type_index == -1 { 0 } else { *type_index + 1 };
        let start = *type_index;
        if start < 0 {
            return None;
        }
        for t in start..nb {
            if let Some(v) = self.types[t as usize].values.iter().find(|v| v.code.as_deref() == Some(code)) {
                *type_index = t;
                return Some(v);
            }
        }
        None
    }

    /// The name of the first value of a type with this number
    /// [soft 0x081389a4 SI_enmdictEnumValGetNameById].
    pub fn enum_val_get_name_by_id(&self, type_index: i32, id: i32) -> Option<&[u8]> {
        self.type_at(type_index)?.values.iter().find(|v| v.id == id)?.name.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> EnumDictStore {
        let mut d = EnumDictStore::new();
        let wir = d.add_enum_type(Some(b"WirType"));
        for (id, code, name) in [(0, &b"HID"[..], &b"Hidden"[..]), (1, b"GHST", b"Ghosted"), (2, b"FULL", b"Visible")] {
            d.types[wir].add_value(id, Some(code), Some(name));
        }
        let sel = d.add_enum_type(Some(b"SelType"));
        for (id, code, name) in [(0, &b"UNSL"[..], &b"Unselected"[..]), (1, b"NODE", b"NodeSelected"), (3, b"BRCH", b"BranchSelected")] {
            d.types[sel].add_value(id, Some(code), Some(name));
        }
        d
    }

    #[test]
    fn look_ups() {
        let d = dict();
        assert_eq!(d.record().nb_types, 2);
        assert_eq!(d.types[0].record().nb_values, 3);
        assert_eq!(d.enum_type_get_name(b"SelType"), 1);
        assert_eq!(d.enum_type_get_name(b"MdlType"), -1);
        assert_eq!(d.enum_val_get_by_code(0, b"GHST").map(|v| v.id), Some(1));
        assert_eq!(d.enum_val_get_by_code(0, b"NODE"), None);
        assert_eq!(d.enum_val_get_by_code(2, b"GHST"), None);
        assert_eq!(d.enum_val_get_by_code(-1, b"GHST"), None);
        assert_eq!(d.enum_code_get_by_value(1, 3), Some(&b"BRCH"[..]));
        assert_eq!(d.enum_code_get_by_value(1, 2), None);
        assert_eq!(d.enum_val_get_name_by_id(0, 2), Some(&b"Visible"[..]));
    }

    #[test]
    fn search_in_any_type_moves_the_cursor_as_the_original_does() {
        let d = dict();
        let mut t = -1;
        assert_eq!(d.enum_val_get_by_any_code(&mut t, Some(b"NODE")).map(|v| v.id), Some(1));
        assert_eq!(t, 1);
        // continuing after type 1: nothing left, the cursor ends one past
        assert_eq!(d.enum_val_get_by_any_code(&mut t, Some(b"NODE")), None);
        assert_eq!(t, 2);
        // at the number of types nothing changes
        assert_eq!(d.enum_val_get_by_any_code(&mut t, Some(b"NODE")), None);
        assert_eq!(t, 2);
        let mut t = -1;
        assert_eq!(d.enum_val_get_by_any_code(&mut t, Some(b"NOPE")), None);
        assert_eq!(t, 0);
        let mut t = -1;
        assert_eq!(d.enum_val_get_by_any_code(&mut t, None), None);
        assert_eq!(t, -1);
    }
}
