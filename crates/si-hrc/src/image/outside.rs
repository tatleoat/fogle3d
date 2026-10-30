// SPDX-License-Identifier: MIT
//! What the readers and writers on the image call that is not on the image: the C
//! library's streams and the routines of other source files.
//!
//! Nothing here reproduces a function of the original. A stream is a `FILE *` of the
//! original: an address in `si-mem`'s handle range that only the implementation of
//! [`Outside`] can read from. A routine of another source file is named by a [`Routine`]
//! (its name and its address in the Linux binary) and reached through [`Outside::call`]
//! with the original's arguments as 32-bit stack words in C order, the calling convention
//! of `si_mem::Registry`; each has a typed wrapper below, which is what the ported code
//! calls.
//!
//! [`Files`] is the streams in memory, behaving as the emulation harness's stand-ins do
//! (`re/tools/emu/standins.py`), so that a port and the emulated original see the same
//! bytes arrive, the same partial item at the end of a file and the same handles;
//! [`MemOutside`] is [`Files`] with every other routine answering 0.

use si_mem::{Handles, Kind, Mem};
use std::collections::BTreeMap;

/// A routine of the original outside this crate's source files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

/// Whoever answers for the streams and for the routines that are not on the image.
pub trait Outside {
    /// `fopen (0x080c4e18)`: a stream on the file named by the string at `path`, or null.
    fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32;
    /// `fclose (0x080c4958)`.
    fn fclose(&mut self, m: &mut Mem, file: u32) -> i32;
    /// `fread (0x080c4108)`: up to `size × n` bytes stored at `ptr`; answers the number of
    /// whole items. The bytes of a last, incomplete item are stored too.
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32;
    /// `fgetc (0x080c3828)`: the next byte, or −1 at the end.
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32;
    /// `rewind (0x080c3e78)`.
    fn rewind(&mut self, m: &mut Mem, file: u32);
    /// `cos (0x080c3808)`, the C library's. What it answered on the machines of 2002 in
    /// the last bit is not known; the default is the host's.
    fn cos(&mut self, _m: &mut Mem, x: f64) -> f64 {
        x.cos()
    }
    /// How many bytes the stream has between its position and its end. Not a call of
    /// the original: the bound this project's memory rule needs before an allocation
    /// sized by a count from the file (see `io::alloc_fits`).
    fn bytes_left(&mut self, m: &mut Mem, file: u32) -> u64;
    /// `fwrite (0x080c4f78)`: the `size × n` bytes at `ptr` appended at the stream's
    /// position; answers the number of whole items written. The writers' only way out.
    /// The default writes nothing (an outside world that serves only reading).
    fn fwrite(&mut self, _m: &mut Mem, _ptr: u32, _size: u32, _n: u32, _file: u32) -> u32 {
        0
    }
    /// `fputc (0x080c4708)`: one byte; answers it, or −1.
    fn fputc(&mut self, _m: &mut Mem, _c: i32, _file: u32) -> i32 {
        -1
    }
    /// `fflush (0x080c3be8)`.
    fn fflush(&mut self, _m: &mut Mem, _file: u32) -> i32 {
        0
    }
    /// `ferror (0x080c3878)`: non-zero once a write to the stream failed.
    fn ferror(&mut self, _m: &mut Mem, _file: u32) -> i32 {
        0
    }
    /// `routine(args…)`: what the original would leave in `eax`.
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32;
    /// Whether this outside world answers for `routine` itself although the routine is on
    /// the image. Not a call of the original. The node reader asks before it runs one of
    /// the readers of `binary.c` that were ported after it; only `hrc-emucheck` says yes,
    /// for a vector recorded while those readers were stand-ins of the harness.
    fn stands_in(&mut self, _routine: &'static Routine) -> bool {
        false
    }
}

macro_rules! outside {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine the readers reach through [`Outside::call`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside! {
    /// `SI_udfListPointerUpdate (0x085479fc)`, `custom_udf.c`: tells a custom effect's
    /// process where the list of shader references and its length now are.
    SI_UDF_LIST_POINTER_UPDATE = "SI_udfListPointerUpdate", 0x085479fc;
    /// `SI_shaderSetActive (0x085405fc)`, `custom_shader.c`.
    SI_SHADER_SET_ACTIVE = "SI_shaderSetActive", 0x085405fc;
    /// `SI_shaderCopy (0x08540880)`, `custom_shader.c`: a copy of a loaded shader.
    SI_SHADER_COPY = "SI_shaderCopy", 0x08540880;
    /// `si_customValueListCopy (0x08526488)`, `custom_dialog.c`: a copy of a list of a
    /// shader's parameter values.
    SI_CUSTOM_VALUE_LIST_COPY = "si_customValueListCopy", 0x08526488;
    /// `shaderFcurveUpdateOwners (0x0854ca48)`, `udf_fct.c`: points the curves of a shader's
    /// parameters back at the reference that owns them.
    SHADER_FCURVE_UPDATE_OWNERS = "shaderFcurveUpdateOwners", 0x0854ca48;
    /// `si_cusTrace (0x085442f0)`, `custom_trace.c`: a line of the custom effects' trace.
    SI_CUS_TRACE = "si_cusTrace", 0x085442f0;
    /// `SI_shaderDispose (0x085401d0)`, `custom_shader.c`.
    SI_SHADER_DISPOSE = "SI_shaderDispose", 0x085401d0;
    /// `shaderFcurveRemoveFromEditor (0x0854c524)`, `udf_fct.c`: takes a shader's curves out
    /// of the curve editor.
    SHADER_FCURVE_REMOVE_FROM_EDITOR = "shaderFcurveRemoveFromEditor", 0x0854c524;
    /// `si_customValueDispose (0x085262a8)`, `custom_dialog.c`: frees one parameter value.
    SI_CUSTOM_VALUE_DISPOSE = "si_customValueDispose", 0x085262a8;
    /// `SI_sceneversioncheckExec (0x083fb304)`, `headers.c`: the hook that warns of a file
    /// newer than the program.
    SI_SCENEVERSIONCHECK_EXEC = "SI_sceneversioncheckExec", 0x083fb304;
}

pub fn udf_list_pointer_update(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, &SI_UDF_LIST_POINTER_UPDATE, &[]);
}
pub fn shader_set_active(m: &mut Mem, o: &mut dyn Outside, shader: u32, active: i16) {
    o.call(m, &SI_SHADER_SET_ACTIVE, &[shader, active as i32 as u32]);
}
pub fn shader_copy(m: &mut Mem, o: &mut dyn Outside, shader: u32) -> u32 {
    o.call(m, &SI_SHADER_COPY, &[shader])
}
pub fn custom_value_list_copy(m: &mut Mem, o: &mut dyn Outside, list: u32) -> u32 {
    o.call(m, &SI_CUSTOM_VALUE_LIST_COPY, &[list])
}
pub fn shader_fcurve_update_owners(m: &mut Mem, o: &mut dyn Outside, udf: u32) {
    o.call(m, &SHADER_FCURVE_UPDATE_OWNERS, &[udf]);
}
pub fn cus_trace(m: &mut Mem, o: &mut dyn Outside, text: u32) {
    o.call(m, &SI_CUS_TRACE, &[text]);
}
pub fn shader_dispose(m: &mut Mem, o: &mut dyn Outside, shader: u32) {
    o.call(m, &SI_SHADER_DISPOSE, &[shader]);
}
pub fn shader_fcurve_remove_from_editor(m: &mut Mem, o: &mut dyn Outside, udf: u32) {
    o.call(m, &SHADER_FCURVE_REMOVE_FROM_EDITOR, &[udf]);
}
pub fn custom_value_dispose(m: &mut Mem, o: &mut dyn Outside, value: u32) {
    o.call(m, &SI_CUSTOM_VALUE_DISPOSE, &[value]);
}
pub fn sceneversioncheck_exec(m: &mut Mem, o: &mut dyn Outside) -> i32 {
    o.call(m, &SI_SCENEVERSIONCHECK_EXEC, &[]) as i32
}

/// One open stream.
#[derive(Clone, Debug, Default)]
pub struct Stream {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub pos: usize,
    pub eof: bool,
    pub err: bool,
    pub closed: bool,
    /// Opened for writing (`fopen` with a mode without `r`): what is written goes to
    /// `data`, which [`Files`] files under the name when the stream is closed.
    pub writing: bool,
}

impl Stream {
    fn getc(&mut self) -> i32 {
        if self.pos >= self.data.len() {
            self.eof = true;
            return -1;
        }
        self.pos += 1;
        self.data[self.pos - 1] as i32
    }

    /// The harness's `file.state` word: end reached, error, closed.
    pub fn state(&self) -> u8 {
        self.eof as u8 | (self.err as u8) << 1 | (self.closed as u8) << 2
    }
}

/// Streams in memory over a small file system, as the harness's stand-ins keep them:
/// handles 0x100 apart from 0x6f000000, the first three being the standard streams, so the
/// first file opened is 0x6f000300 here as there.
pub struct Files {
    named: BTreeMap<Vec<u8>, Vec<u8>>,
    open: Handles<Stream>,
    last: u32,
}

impl Default for Files {
    fn default() -> Self {
        Files::new()
    }
}

impl Files {
    pub fn new() -> Files {
        let mut open = Handles::new(Kind::File);
        for name in [&b"stdin"[..], b"stdout", b"stderr"] {
            open.open(Stream { name: name.to_vec(), ..Stream::default() });
        }
        Files { named: BTreeMap::new(), open, last: 0 }
    }

    /// Puts a file into the file system: `fopen` of that path will find it.
    pub fn add(&mut self, path: &[u8], data: Vec<u8>) {
        self.named.insert(path.to_vec(), data);
    }

    /// A stream on bytes, as a caller that already holds a `FILE *` has it.
    pub fn open_bytes(&mut self, name: &[u8], data: Vec<u8>) -> u32 {
        self.last = self.open.open(Stream { name: name.to_vec(), data, ..Stream::default() });
        self.last
    }

    /// The stream opened last (by [`Files::open_bytes`] or `fopen`), closed or not; 0 if none.
    pub fn last(&self) -> u32 {
        self.last
    }

    pub fn stream(&self, file: u32) -> Option<&Stream> {
        self.open.get(file)
    }

    /// How many streams were opened, the three standard ones not counted.
    pub fn opened(&self) -> usize {
        self.open.len() - 3
    }

    /// What the file system holds under `path`: a file put there, or one a stream wrote
    /// and closed.
    pub fn file(&self, path: &[u8]) -> Option<&Vec<u8>> {
        self.named.get(path)
    }

    pub fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        let path = m.cstr(path);
        let mode = m.cstr(mode);
        // writing ("w", "wb"): a new, empty file, filed when the stream is closed
        if !mode.contains(&b'r') {
            self.last = self.open.open(Stream { name: path, writing: true, ..Stream::default() });
            return self.last;
        }
        match self.named.get(&path) {
            Some(data) => {
                let data = data.clone();
                self.last = self.open.open(Stream { name: path, data, ..Stream::default() });
                self.last
            }
            None => 0,
        }
    }

    pub fn fclose(&mut self, file: u32) -> i32 {
        if let Some(s) = self.open.get_mut(file) {
            s.closed = true;
            if s.writing {
                let (name, data) = (s.name.clone(), s.data.clone());
                self.named.insert(name, data);
            }
        }
        0
    }

    /// A stream to write to that is not in the file system, as a caller that already
    /// holds a `FILE *` has it; [`Files::stream`] shows what was written.
    pub fn open_for_writing(&mut self, name: &[u8]) -> u32 {
        self.last = self.open.open(Stream { name: name.to_vec(), writing: true, ..Stream::default() });
        self.last
    }

    pub fn fwrite(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        let Some(s) = self.open.get_mut(file) else {
            m.record_fault(file, 1, si_mem::Access::Write);
            return 0;
        };
        let total = size as u64 * n as u64;
        if !s.writing || s.closed || total > (1 << 30) {
            s.err = true;
            return 0;
        }
        let bytes = m.bytes(ptr, total as u32).to_vec();
        let end = s.pos + bytes.len();
        if s.data.len() < end {
            s.data.resize(end, 0);
        }
        s.data[s.pos..end].copy_from_slice(&bytes);
        s.pos = end;
        if size == 0 {
            0
        } else {
            n
        }
    }

    pub fn fputc(&mut self, m: &mut Mem, c: i32, file: u32) -> i32 {
        let Some(s) = self.open.get_mut(file) else {
            m.record_fault(file, 1, si_mem::Access::Write);
            return -1;
        };
        if !s.writing || s.closed {
            s.err = true;
            return -1;
        }
        let b = c as u8;
        if s.pos < s.data.len() {
            s.data[s.pos] = b;
        } else {
            s.data.push(b);
        }
        s.pos += 1;
        b as i32
    }

    pub fn ferror(&self, file: u32) -> i32 {
        self.open.get(file).map_or(0, |s| s.err as i32)
    }

    pub fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        let Some(s) = self.open.get_mut(file) else {
            m.record_fault(file, 1, si_mem::Access::Read);
            return 0;
        };
        let want = (size as u64 * n as u64).min(usize::MAX as u64) as usize;
        let have = s.data.len().saturating_sub(s.pos);
        let got = want.min(have);
        if got < want {
            s.eof = true;
        }
        let from = s.pos;
        s.pos += got;
        if got > 0 {
            // one store of everything that arrived, the bytes of an incomplete item included
            let bytes = s.data[from..from + got].to_vec();
            m.set_bytes(ptr, &bytes);
        }
        if size == 0 {
            0
        } else {
            (got / size as usize) as u32
        }
    }

    pub fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        match self.open.get_mut(file) {
            Some(s) => s.getc(),
            None => {
                m.record_fault(file, 1, si_mem::Access::Read);
                -1
            }
        }
    }

    pub fn bytes_left(&self, file: u32) -> u64 {
        self.open.get(file).map_or(0, |s| s.data.len().saturating_sub(s.pos) as u64)
    }

    pub fn rewind(&mut self, file: u32) {
        if let Some(s) = self.open.get_mut(file) {
            s.pos = 0;
            s.eof = false;
            s.err = false;
        }
    }
}

/// [`Files`] as the outside world: the streams are served, every other routine answers 0
/// and is remembered.
#[derive(Default)]
pub struct MemOutside {
    pub files: Files,
    pub calls: Vec<(&'static Routine, Vec<u32>)>,
}

impl Outside for MemOutside {
    fn fopen(&mut self, m: &mut Mem, path: u32, mode: u32) -> u32 {
        self.files.fopen(m, path, mode)
    }
    fn fclose(&mut self, _m: &mut Mem, file: u32) -> i32 {
        self.files.fclose(file)
    }
    fn fread(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        self.files.fread(m, ptr, size, n, file)
    }
    fn fgetc(&mut self, m: &mut Mem, file: u32) -> i32 {
        self.files.fgetc(m, file)
    }
    fn rewind(&mut self, _m: &mut Mem, file: u32) {
        self.files.rewind(file)
    }
    fn bytes_left(&mut self, _m: &mut Mem, file: u32) -> u64 {
        self.files.bytes_left(file)
    }
    fn fwrite(&mut self, m: &mut Mem, ptr: u32, size: u32, n: u32, file: u32) -> u32 {
        self.files.fwrite(m, ptr, size, n, file)
    }
    fn fputc(&mut self, m: &mut Mem, c: i32, file: u32) -> i32 {
        self.files.fputc(m, c, file)
    }
    fn ferror(&mut self, _m: &mut Mem, file: u32) -> i32 {
        self.files.ferror(file)
    }
    fn call(&mut self, _m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32 {
        self.calls.push((routine, args.to_vec()));
        0
    }
}
