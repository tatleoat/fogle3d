// SPDX-License-Identifier: MIT
//! `headers.c`: the 88-byte header every database file starts with (finding 0002).

use super::byteswap::file_chunk_fread;
use super::globals::{SCENE_VERSION_CHECK, SI_VERSION_INPUT};
use super::off::file_chunk;
use super::outside::{Outside, REWIND};
use super::fmov;
use si_mem::Mem;

/// The float a version starts as (0x0865c588): −1.
pub const NO_VERSION_BITS: u32 = 0xbf80_0000;
/// The double above which a file's version makes the header reader call the scene version
/// check (0x0865c590).
pub const VERSION_CHECK_ABOVE: f64 = 4.0001;
/// The first of the nineteen magic numbers a header may start with.
pub const FIRST_MAGIC: u32 = 0x5380_f634;

/// [soft 0x083fb2f0 SI_sceneversioncheckExists]
/// Whether a scene version check is installed (`scene_version_check` is not null).
pub fn sceneversioncheck_exists(m: &Mem) -> u32 {
    (m[SCENE_VERSION_CHECK] != 0) as u32
}

/// [soft 0x083fb304 SI_sceneversioncheckExec]
/// Calls the installed scene version check and answers its 16-bit result; 0 without one.
pub fn sceneversioncheck_exec(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    if sceneversioncheck_exists(m) & 0xffff == 0 {
        return 0;
    }
    let check = m[SCENE_VERSION_CHECK];
    o.call_addr(m, check, &[]) as i16 as i32
}

/// [soft 0x083fb6fc SI_dbfileReadBinHeader]
/// Reads the 88 bytes of a file header. `*version` starts as −1. When the first word is
/// one of the nineteen magic numbers (0x5380f634 … 0x5380f646) the version is stored
/// through `version` and in `SI_versionInput`, the scene version check is called for a
/// version above 4.0001, the comment (80 bytes) is copied to `comment` when that is not
/// null, and the answer is the magic number. Otherwise the stream is put back at its start
/// and the answer is −1.
///
/// The header lies on the original's stack and its read is not looked at: a file shorter
/// than a header leaves what bytes there are, not swapped, in front of whatever the stack
/// held. Here that is what the image's stack holds.
pub fn dbfile_read_bin_header(m: &mut Mem, o: &mut dyn Outside, stream: u32, version: u32, comment: u32) -> i32 {
    let sp = m.sp();
    let chunk = m.local(file_chunk::SIZE);
    let answer = read_bin_header_at(m, o, stream, version, comment, chunk);
    m.set_sp(sp);
    answer
}

/// The header reader with its 88 bytes of stack at an address the caller chose (the
/// lattice file's reader needs to know where they lay: see
/// [`super::elemdict::elemdict_read_lattice`]).
pub(crate) fn read_bin_header_at(m: &mut Mem, o: &mut dyn Outside, stream: u32, version: u32, comment: u32, chunk: u32) -> i32 {
    m.set_u32(version, NO_VERSION_BITS);
    file_chunk_fread(m, o, chunk, file_chunk::SIZE, 1, stream);
    let magic = m.u32(chunk + file_chunk::MAGIC);
    if magic.wrapping_sub(FIRST_MAGIC) <= 0x12 {
        let v = fmov(m.u32(chunk + file_chunk::VERSION));
        m.set_u32(version, v);
        m[SI_VERSION_INPUT] = v;
        if f32::from_bits(m.u32(version)) as f64 > VERSION_CHECK_ABOVE {
            sceneversioncheck_exec(m, o);
        }
        if comment != 0 {
            si_mem::clib::strncpy(m, comment, chunk + file_chunk::COMMENT, 0x50);
        }
        magic as i32
    } else {
        o.call(m, &REWIND, &[stream]);
        -1
    }
}
