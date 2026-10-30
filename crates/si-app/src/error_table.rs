// SPDX-License-Identifier: MIT
//! The error service's two tables, `si_errorFormat` and `SI_errorType`. Their contents (the
//! original's message formats) are not in this source: they are read when first used from the
//! user's own Softimage|3D 4.0 install (`si-rt`; release model: none of the original's messages
//! ship), by where the Linux `soft` holds them, and checked against our own checksums
//! (`docs/design/runtime-data.md`). Unit rtdata1 held all 150 entries and the five names against
//! the literals this file carried before (generated then by `re/tools/si-app/gen_tables.py`).

use si_rt::Table;
use std::sync::LazyLock;

/// The number of entries of `si_errorFormat`.
pub const ERROR_COUNT: usize = 150;

/// `si_errorFormat` (0x08686a60): 150 entries of 12 bytes, the code's own number (i32), its type
/// (i32) and a pointer to its `printf` format.
pub static ERROR_FORMAT_TABLE: Table = Table::new("si_app.si_errorFormat", 0x0868_6a60, ERROR_COUNT as u32 * 12, 0xd7ee_744d_99bf_27b5);

/// `SI_errorType` (0x08687198): five pointers to the names of the error types.
pub static ERROR_TYPE_TABLE: Table = Table::new("si_app.SI_errorType", 0x0868_7198, 5 * 4, 0x5465_52e6_d798_cbb8);

fn word(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}

/// For each error code its own number, its type and its `printf` format, as `si_errorFormat`
/// holds them (read once).
pub static ERROR_FORMAT: LazyLock<[(i32, i32, &'static [u8]); ERROR_COUNT]> = LazyLock::new(|| {
    let b = ERROR_FORMAT_TABLE.get();
    std::array::from_fn(|i| (word(b, 3 * i) as i32, word(b, 3 * i + 1) as i32, si_rt::message(word(b, 3 * i + 2))))
});

/// The names of the error types, as `SI_errorType` holds them (read once).
pub static ERROR_TYPE: LazyLock<[&'static [u8]; 5]> = LazyLock::new(|| {
    let b = ERROR_TYPE_TABLE.get();
    std::array::from_fn(|i| si_rt::message(word(b, i)))
});

#[cfg(test)]
mod tests {
    use super::*;

    /// With an install: the codes are their own index, the types are 0 to 4, every format is
    /// text, and the checksums of the two tables hold (`Table::get`).
    #[test]
    fn the_tables_read_from_the_install() {
        si_rt::skip_without_install!();
        for (i, (code, kind, fmt)) in ERROR_FORMAT.iter().enumerate() {
            assert_eq!(*code, i as i32);
            assert!((0..5).contains(kind), "{i}: type {kind}");
            assert!(fmt.iter().all(|&c| c == b'\n' || c == b'\t' || (0x20..0x7f).contains(&c)), "{i}");
        }
        assert!(ERROR_TYPE.iter().all(|t| !t.is_empty() && t.iter().all(u8::is_ascii_uppercase)));
        // our own numbers for the contents (length and FNV-1a of the texts, each followed by a zero)
        let texts: Vec<u8> = ERROR_FORMAT.iter().flat_map(|e| e.2.iter().copied().chain([0])).collect();
        let names: Vec<u8> = ERROR_TYPE.iter().flat_map(|t| t.iter().copied().chain([0])).collect();
        assert_eq!((texts.len(), si_rt::fnv1a64(&texts)), (TEXTS_LEN, TEXTS_SUM));
        assert_eq!((names.len(), si_rt::fnv1a64(&names)), (NAMES_LEN, NAMES_SUM));
    }

    // measured by unit rtdata1 on the literals this file held before (150 formats, five names)
    const TEXTS_LEN: usize = 4763;
    const TEXTS_SUM: u64 = 0xe4d7_9b65_99db_a8b9;
    const NAMES_LEN: usize = 34;
    const NAMES_SUM: u64 = 0xdd30_af67_613f_8fd3;
}
