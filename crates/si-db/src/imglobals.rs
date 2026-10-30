// SPDX-License-Identifier: MIT
//! Global variables of the browser's source files, at their addresses in the binary
//! (held against the binary's symbol table by `re/tools/si-mem/check_globals.py`).

si_mem::globals! {
    /// `glbDbIcon` (dbicons.c): the database icon record (12 bytes), made on first use.
    GLB_DB_ICON: u32 = 0x08680ee0, "glbDbIcon", init 0;
    /// `glbDirIcon` (dbicons.c): the directory icon record.
    GLB_DIR_ICON: u32 = 0x08680ee4, "glbDirIcon", init 0;
    /// `glbNoIcon` (dbicons.c): the "no icon" record.
    GLB_NO_ICON: u32 = 0x08680ee8, "glbNoIcon", init 0;
    /// `workStr`: the scratch text buffer the browser's drawing code formats into.
    WORK_STR: [u8; 512] = 0x087859a0, "workStr";
    /// `dbErrFlag` (dbmanager.c): the browser's last error code.
    DB_ERR_FLAG: i32 = 0x0867de00, "dbErrFlag", init 0;
    /// `SI_dBWorkFname` (dbmanager.c): the scratch file-name buffer.
    DB_WORK_FNAME: [u8; 256] = 0x08788140, "SI_dBWorkFname";
    /// `SI_procData`: the pointer to the process data (its first word is the current dictionary index).
    SI_PROC_DATA: u32 = 0x08679314, "SI_procData", init 0x08782108;
    /// `SI_elemDicts`: the pointer to the table of element dictionaries (0x70 bytes each).
    SI_ELEM_DICTS: u32 = 0x0867931c, "SI_elemDicts", init 0;
    /// `SI_appf` (font ids): the application font.
    SI_APPF: i16 = 0x08781644, "SI_appf";
    /// `SI_irisf`: the IRIS font.
    SI_IRISF: i16 = 0x08781790, "SI_irisf";
    /// `SI_coordx` (window code): the valuator number of the pointer's x.
    SI_COORDX: u16 = 0x086792be, "SI_coordx", init 0x010a;
    /// `SI_coordy`: the valuator number of the pointer's y.
    SI_COORDY: u16 = 0x086792c0, "SI_coordy", init 0x010b;
    /// `workStr` of `dbmanager.c`: the manager's scratch text buffer (another static of the same name).
    DBM_WORK_STR: [u8; 512] = 0x08783bc0, "workStr";
    /// `stdin`: the C library's standard input stream.
    STDIN: u32 = 0x08780e90, "stdin@@GLIBC_2.0";
    /// `SI_cid` (elemdict.c): the scene dictionary's chapter indices (the words the icon names are chosen by).
    SI_CID: [i32; 36] = 0x08781660, "SI_cid";
}

/// The offsets in `SI_cid` of the chapter indices `dbGetChapIconName` tests.
pub mod cid {
    pub const ANI: u32 = 0x00;
    pub const CAM: u32 = 0x04;
    pub const CLS: u32 = 0x08;
    pub const DSC: u32 = 0x0c;
    pub const LAT: u32 = 0x14;
    pub const LIG: u32 = 0x18;
    pub const MAT: u32 = 0x1c;
    pub const MODEL: u32 = 0x20;
    pub const PAL: u32 = 0x28;
    pub const SHP: u32 = 0x54;
    pub const STS: u32 = 0x58;
    pub const STC: u32 = 0x5c;
    pub const SFK: u32 = 0x60;
    pub const SKY: u32 = 0x64;
    pub const T2D: u32 = 0x68;
    pub const T3D: u32 = 0x6c;
    pub const WAV: u32 = 0x70;
    pub const EXP: u32 = 0x74;
    pub const SPD: u32 = 0x80;
}
