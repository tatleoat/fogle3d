// SPDX-License-Identifier: MIT
//! What the readers call that is not in this crate: the C library's streams, the graph
//! list, the status line, the custom shader records.
//!
//! Nothing here reproduces a function of the original. A routine is named by a [`Routine`]
//! (its name and its address in the Linux binary; for a routine of the C library the
//! address of the binary's own stub) and reached through [`Outside::call`] with the
//! original's arguments as 32-bit stack words in C order: the calling convention of
//! `si_mem::Registry` and of the `Outside` traits of `si-view`, `si-dict` and `si-rensetup`,
//! so that a wrapper of a few lines makes one world serve them all. [`super::host::Files`]
//! answers the stream calls on byte vectors.

use si_mem::Mem;

/// A routine outside this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: &'static str,
    pub addr: u32,
}

/// Whoever answers for the routines that are not in this crate.
pub trait Outside {
    /// `routine(args…)`: what the original would find in `eax` afterwards. A callee that
    /// stores through a pointer argument stores into the image.
    fn call(&mut self, m: &mut Mem, routine: &'static Routine, args: &[u32]) -> u32;

    /// What the original prints on its standard output with `printf (0x080c48b8)`, already
    /// formatted (the readers print three fixed texts).
    fn print(&mut self, _m: &mut Mem, _text: &[u8]) {}

    /// What the original prints on its standard error with `fprintf (0x080c3b88)`, already
    /// formatted.
    fn print_err(&mut self, _m: &mut Mem, _text: &[u8]) {}

    /// A call through a function pointer the original keeps in a variable: the code at
    /// `addr` with these arguments. The default hands a routine this crate names to
    /// [`Outside::call`] and answers 0 for any other address.
    fn call_addr(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match ALL.iter().find(|r| r.addr == addr) {
            Some(r) => self.call(m, r, args),
            None => 0,
        }
    }

    /// `tan (0x080c4898)`, the C library's. What it answered on the machines of 2002 in
    /// the last bit is not known (finding 0011); the default is the host's.
    fn tan(&mut self, _m: &mut Mem, x: f64) -> f64 {
        x.tan()
    }

    /// **Not a call of the original.** How many bytes the stream has left. Asked only where
    /// the original would allocate for a count the file gives, or read a field of a size
    /// the file gives into a local of its stack, so that the port can stop instead
    /// ([`super::NoReturn`]).
    fn file_left(&mut self, m: &mut Mem, file: u32) -> u32;
}

macro_rules! outside {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this crate reaches through [`Outside`].
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside! {
    // ---- the C library, by the binary's stubs
    /// `fread (0x080c4108)`: `fread(buffer, size, count, stream)`; the bytes go into the
    /// image, the answer is the number of whole items.
    FREAD = "fread", 0x080c4108;
    /// `fgetc (0x080c3828)`: the next byte of a stream, or −1 at its end.
    FGETC = "fgetc", 0x080c3828;
    /// `fopen (0x080c4e18)`: `fopen(path, mode)`, both texts in the image; null when the
    /// file cannot be opened.
    FOPEN = "fopen", 0x080c4e18;
    /// `fclose (0x080c4958)`.
    FCLOSE = "fclose", 0x080c4958;
    /// `rewind (0x080c3e78)`: puts a stream back at its start.
    REWIND = "rewind", 0x080c3e78;
    /// `time (0x080c4328)`: the seconds since 1970, also stored through the argument when
    /// that is not null.
    TIME = "time", 0x080c4328;
    // ---- status.c
    /// `SI_statusDisplayMessage (0x083cca94)`: a text on the status line; the second
    /// argument is its kind (3 for an error).
    SI_STATUS_DISPLAY_MESSAGE = "SI_statusDisplayMessage", 0x083cca94;
    // ---- graflist.c (ported in `si-view`)
    /// `SI_findGlData (0x0832ee68)`: the graph node of a list that holds an object.
    SI_FIND_GL_DATA = "SI_findGlData", 0x0832ee68;
    /// `SI_graflistSetActive (0x0832c488)`: makes a node the list's active one.
    SI_GRAFLIST_SET_ACTIVE = "SI_graflistSetActive", 0x0832c488;
    /// `SI_graflistAddData (0x0832d240)`: gives an object its node in a graph list
    /// (list, object, kind, a word).
    SI_GRAFLIST_ADD_DATA = "SI_graflistAddData", 0x0832d240;
    // ---- model.c, edtools.c
    /// `SI_modelAddILattice (0x08472a0c)`: puts a lattice into a model's list of lattices
    /// (model, node or branch, index, lattice).
    SI_MODEL_ADD_I_LATTICE = "SI_modelAddILattice", 0x08472a0c;
    /// `SI_edAddLattice (0x0810383c)`: gives a model's lattices their element (model, node
    /// or branch).
    SI_ED_ADD_LATTICE = "SI_edAddLattice", 0x0810383c;
    // ---- elemdict.c (ported in `si-dict`)
    /// `SI_elemdictFindListByName (0x081325f4)`: the index of a dictionary's chapter of
    /// that name (dictionary, name).
    SI_ELEMDICT_FIND_LIST_BY_NAME = "SI_elemdictFindListByName", 0x081325f4;
    // ---- actions.c
    /// `SI_actionsGetNewName (0x081e3f0c)`: a name no element of an action list has yet, in
    /// a block of its own (list, a name to start from or null).
    SI_ACTIONS_GET_NEW_NAME = "SI_actionsGetNewName", 0x081e3f0c;
    /// `SI_actionsElemSelect (0x081e46b8)`: selects an element of an action list the way
    /// the user would (list, element, on, a word).
    SI_ACTIONS_ELEM_SELECT = "SI_actionsElemSelect", 0x081e46b8;
    // ---- udf_fct.c
    /// `SI_udfAllocate (0x0854c2f4)`: a new, empty custom shader record; null when there is
    /// no memory.
    SI_UDF_ALLOCATE = "SI_udfAllocate", 0x0854c2f4;
    /// `SI_udfDispose (0x0854c3c0)`: frees a custom shader record (null is accepted).
    SI_UDF_DISPOSE = "SI_udfDispose", 0x0854c3c0;
}

/// `fread(buffer, size, count, stream)`.
pub fn fread(o: &mut dyn Outside, m: &mut Mem, buffer: u32, size: u32, count: u32, stream: u32) -> u32 {
    o.call(m, &FREAD, &[buffer, size, count, stream])
}

/// `fgetc(stream)`.
pub fn fgetc(o: &mut dyn Outside, m: &mut Mem, stream: u32) -> i32 {
    o.call(m, &FGETC, &[stream]) as i32
}
