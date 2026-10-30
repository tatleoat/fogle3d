// SPDX-License-Identifier: MIT
//! The C library calls the converters make that `owave2soft.rs` does not declare, and the
//! program's own strings the writers use (finding 0039). Nothing here reproduces a function of
//! the original: the calls are answered by whoever runs the port ([`si_mem::Outside`]).

use si_mem::{outside_calls, Mem, Outside, Routine};

outside_calls! {
    /// Stream and time calls of the C library.
    pub trait StdioCalls {
        /// `fopen(path, mode)` (C library): the stream, or null.
        fn fopen(path: u32, mode: u32) -> u32 = "fopen", 0x080c_4e18;
        /// `fseek(stream, offset, whence)` (C library).
        fn fseek(stream: u32, offset: i32, whence: i32) -> i32 = "fseek", 0x080c_42e8;
        /// `ftell(stream)` (C library).
        fn ftell(stream: u32) -> i32 = "ftell", 0x080c_3e98;
        /// `time(t)` (C library).
        fn time(t: u32) -> u32 = "time", 0x080c_4328;
        /// `ctime(t)` (C library): the text of the time, ending in a newline.
        fn ctime(t: u32) -> u32 = "ctime", 0x080c_4998;
        /// `getenv(name)` (C library): the value, or null.
        fn getenv(name: u32) -> u32 = "getenv", 0x080c_3bb8;
        /// `__errno_location()` (C library): the address of `errno`.
        fn errno_location() -> u32 = "__errno_location", 0x080c_3e38;
        /// `strerror(errnum)` (C library): the text of the error.
        fn strerror(errnum: i32) -> u32 = "strerror", 0x080c_3d28;
        /// `sleep(seconds)` (C library).
        fn sleep(seconds: u32) -> u32 = "sleep", 0x080c_4468;
    }
}

const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
const SPRINTF: Routine = Routine::new("sprintf", 0x080c_4f58);
const SSCANF: Routine = Routine::new("sscanf", 0x080c_4c28);

/// `fprintf (0x080c3b88)` (C library) with the words its format takes after the format (a
/// `double` is two words, low first); the count of characters written.
pub fn fprintf(m: &mut Mem, o: &mut dyn Outside, file: u32, format: u32, args: &[u32]) -> i32 {
    let mut words = vec![file, format];
    words.extend_from_slice(args);
    o.call(m, FPRINTF, &words).eax as i32
}

/// `sprintf (0x080c4f58)` (C library): the characters written, not counting the terminator.
pub fn sprintf(m: &mut Mem, o: &mut dyn Outside, dst: u32, format: u32, args: &[u32]) -> i32 {
    let mut words = vec![dst, format];
    words.extend_from_slice(args);
    o.call(m, SPRINTF, &words).eax as i32
}

/// `sscanf (0x080c4c28)` (C library) with the addresses its format stores through; the number
/// of conversions, or -1 at the end of the string before the first.
pub fn sscanf(m: &mut Mem, o: &mut dyn Outside, text: u32, format: u32, stores: &[u32]) -> i32 {
    let mut words = vec![text, format];
    words.extend_from_slice(stores);
    o.call(m, SSCANF, &words).eax as i32
}

/// The two words of a `double` as pushed.
pub fn dbl(v: f64) -> [u32; 2] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32]
}

/// Strings of the program's `.rodata` the writers and the reader pass by address (a check
/// binary stores them with [`init_rodata`]; the addresses are the original's).
pub const STRINGS: &[(u32, &str)] = &[
    // owave2soft.c: the formats of readMaterialFile
    (0x0869_1e2a, "%s"),
    (0x0869_1e3c, "%s"),
    (0x0869_1e5b, "%f %f %f"),
    (0x0869_1e7f, "%f %f %f"),
    (0x0869_1ea4, "%f %f %f"),
    (0x0869_1ec9, "%d"),
    (0x0869_1f33, "%f"),
    (0x0869_1f52, "%f"),
    // readWavefrontFile
    (0x0869_180f, "%s.obj"),
    (0x0869_1816, "r"),
    (0x0869_1821, "WF_MTL_LIB"),
    (0x0869_182c, "r"),
    (0x0869_186a, "%f %f %f"),
    (0x0869_1894, "%f %f %f"),
    (0x0869_1937, "%s"),
    (0x0869_193a, "<default>"),
    (0x0869_1946, "%s"),
    (0x0869_1949, "off"),
    (0x0869_194d, "0"),
    (0x0869_1989, "%s"),
    (0x0869_198c, "r"),
    (0x0869_19cd, "%s"),
    (0x0869_1a26, "%s"),
    (0x0869_1c41, "<%s>"),
    // osoft2vert.c
    (0x0869_06c0, "No object to convert!"),
    (0x0869_06d6, "Current object: %s"),
    (0x0869_06e9, ".pol"),
    (0x0869_06ee, "Done."),
    (0x0869_06f4, ".pol"),
    (0x0869_06f9, "w"),
    (0x0869_06fb, " (%lf %lf %lf)\n        "),
    (0x0869_0713, " (%lf %lf %lf)"),
    (0x0869_0722, "\n"),
    (0x0869_0724, "# Face %d:\n    face"),
    (0x0869_0738, "\n        hole"),
    (0x0869_0746, " %ld"),
    (0x0869_074b, "\n"),
    (0x0869_074d, "endobject\n"),
    (0x0869_0758, "object polygon\n    vertices"),
];

/// Stores [`STRINGS`] (and the strings of the other modules' tables) in the image's `.rodata`.
pub fn init_rodata(m: &mut Mem) {
    // errno of the emulator's sandbox
    m.set_rom(0x0869_1ff0, &[2, 0, 0, 0]);
    for (addr, s) in STRINGS.iter().chain(crate::osoft2wave::STRINGS.iter()).chain(crate::omovie2soft::STRINGS.iter()).chain(crate::overt2soft::STRINGS.iter()) {
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        m.set_rom(*addr, &b);
    }
}
