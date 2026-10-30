// SPDX-License-Identifier: MIT
//! `renu_filepath.c`, the routine of it that is a path rewrite for the mental ray file
//! (finding 0018, section "Render set-up: the display side"). `SI_mrfileIsValidExec` and
//! `SI_mrfileGetExecFullpath` are in [`crate::sys`] on a trait of their own.
//!
//! **Compared with the emulated original**: run against the original stepped through in
//! the emulation harness (`re/tools/si-rensetup/emu_gen_pass6.py`, `rensetup-emucheck`).

use super::globals::*;
use super::outside::*;
use si_mem::{clib, Mem};

/// The path of a picture or file as the mental ray file names it [soft 0x0842faa8 SI_mrfileConvertToUnix]: `path` is the name in the program, `out` a buffer of `size`
/// bytes it is copied to. Answers 1, or 0 when the database directory knows no match.
///
/// - Without the mental ray 2 output (`SI_getmi2Output`) or without a name server image
///   reference (`SI_spdlUseNamesrvImgRef`; both 16-bit answers): when `out` is `path` itself
///   nothing is done; else the name is copied with `strncpy(out, path, size − 1)` and
///   `out[size − 1]` set to 0. The answer is 1.
/// - Otherwise the database directory `SI_dbdir` is asked (`SI_dbDirSearchDatabaseFromPath`
///   with the path, 3, a local for the record found and a local of 2,048 bytes); an answer
///   other than 100, no record, or a record without a name (+0x8) or a prefix (+0xc) is 0.
///   The text is `{name}/rest` (`{%s}/%s`), the rest being what follows the prefix and the
///   one character after it in `path`; it is copied to `out` as above. The original formats
///   it into a local of 2,048 bytes without a bound; this does not bound it either, and the
///   local here is as large.
pub fn mrfile_convert_to_unix<O: Outside + ?Sized>(o: &mut O, m: &mut Mem, path: u32, out: u32, size: i32) -> u32 {
    let first = o.call(m, SI_GETMI2_OUTPUT, &[]) as u16;
    if first == 0 || (o.call(m, SI_SPDL_USE_NAMESRV_IMG_REF, &[]) as u16) == 0 {
        if out == path {
            return 1;
        }
        copy_out(m, out, path, size);
        return 1;
    }
    // locals: the record found at ebp − 0x808, the buffer handed to the search at ebp − 0x804
    // (2,048 bytes) and the text formatted at ebp − 0x800, which lies inside that buffer
    let frame = m.local(0x810);
    m.set_u32(frame, 0);
    let answer = o.call(m, SI_DB_DIR_SEARCH_DATABASE_FROM_PATH, &[SI_DBDIR, path, 3, frame, frame + 4]);
    let record = m.u32(frame);
    let ok = answer == 100 && record != 0 && m.u32(record + 0xc) != 0 && m.u32(record + 8) != 0;
    let mut result = 0;
    if ok {
        let prefix = clib::strlen(m, m.u32(record + 0xc));
        let mut text = vec![b'{'];
        text.extend(m.cstr(m.u32(record + 8)));
        text.extend_from_slice(b"}/");
        text.extend(m.cstr(path.wrapping_add(prefix).wrapping_add(1)));
        m.put_cstr(frame + 8, &text);
        copy_out(m, out, frame + 8, size);
        result = 1;
    }
    m.release(0x810);
    result
}

/// `strncpy(out, from, size − 1)` and `out[size − 1] = 0`.
fn copy_out(m: &mut Mem, out: u32, from: u32, size: i32) {
    clib::strncpy(m, out, from, (size - 1) as u32);
    m.set_u8(out.wrapping_add((size - 1) as u32), 0);
}
