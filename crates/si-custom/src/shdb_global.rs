// SPDX-License-Identifier: MIT
//! `shdb_global.c`: the statics of the shader-database sources. The initial values come from
//! [`crate::shdb_data`]; the addresses of the globals the ported files use are named here.

use crate::shdb_hooks::S_TEMP;
use si_mem::{Mem, Outside, Routine};

pub const SHMGET: Routine = Routine::new("plt:shmget", 0x080c_4198);
pub const SHMAT: Routine = Routine::new("plt:shmat", 0x080c_5138);

/// `s_mem`: the id of the shared segment (`shmget`), and `s_temp` (in [`crate::shdb_hooks::S_TEMP`]) its address.
pub const S_MEM: u32 = 0x0882_4800;
/// The root of the parsed tree (`si_shdbRoot`).
pub const SI_SHDB_ROOT: u32 = 0x086e_56c0;
/// The parser's globals (`.bss`).
pub const SHDB_YYDEBUG: u32 = 0x0882_4860;
pub const SHDB_YYCHAR: u32 = 0x0882_4864;
pub const SHDB_YYLVAL: u32 = 0x0882_4868;
pub const SHDB_YYNERRS: u32 = 0x0882_4878;

/// [soft 0x0854e8a0 __static_initialization_and_destruction_0]: for the initialisation call (`initialize` not zero, `priority`
/// 0xffff) creates the 6-byte shared segment (`shmget(0, 6, 0x380)`, key `IPC_PRIVATE`, mode 0600 and `IPC_CREAT|IPC_EXCL`)
/// into `s_mem` and attaches it (`shmat(id, 0, 0)`) into `s_temp`, the three interrupt flags of the preview. Any other call does nothing.
pub fn static_initialization_and_destruction_0(m: &mut Mem, o: &mut dyn Outside, initialize: u32, priority: u32) {
    if priority == 0xffff && initialize != 0 {
        let id = o.call(m, SHMGET, &[0, 6, 0x380]).eax;
        m.set_u32(S_MEM, id);
        let p = o.call(m, SHMAT, &[id, 0, 0]).eax;
        m.set_u32(S_TEMP, p);
    }
}

/// [soft 0x0854e8e4 _GLOBAL_.I.s_mem]: the file's static constructor, `static_initialization_and_destruction_0(1, 0xffff)`.
pub fn global_i_s_mem(m: &mut Mem, o: &mut dyn Outside) {
    static_initialization_and_destruction_0(m, o, 1, 0xffff);
}
