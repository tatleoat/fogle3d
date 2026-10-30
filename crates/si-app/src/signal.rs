// SPDX-License-Identifier: MIT
//! `sys_signal.c`: which signals end the program through its own exit routine.

use crate::env::{Run, SignalAction, World};
use crate::state::App;

/// The signal numbers [soft 0x080d1038 SI_signalInit] names (Linux): SIGQUIT, SIGBUS,
/// SIGSEGV, SIGFPE, SIGTERM.
pub const SIGQUIT: i32 = 3;
pub const SIGBUS: i32 = 7;
pub const SIGFPE: i32 = 8;
pub const SIGSEGV: i32 = 11;
pub const SIGTERM: i32 = 15;

impl<W: World> App<W> {
    /// [soft 0x080d1038 SI_signalInit]: `coredump` −1 keeps the remembered choice, any
    /// other value becomes it. SIGQUIT always gets the default action. Without core dumps
    /// SIGBUS, SIGSEGV, SIGFPE and SIGTERM go to `SI_serverExit` (which runs the emergency
    /// save); with them the same four get the default action, in the order SIGBUS,
    /// SIGSEGV, SIGTERM, SIGFPE.
    pub fn signal_init(&mut self, coredump: i16) {
        if coredump != -1 {
            self.g.signal_coredump = coredump;
        }
        self.w.signal(SIGQUIT, SignalAction::Default);
        if self.g.signal_coredump == 0 {
            for s in [SIGBUS, SIGSEGV, SIGFPE, SIGTERM] {
                self.w.signal(s, SignalAction::ServerExit);
            }
        } else {
            for s in [SIGBUS, SIGSEGV, SIGTERM, SIGFPE] {
                self.w.signal(s, SignalAction::Default);
            }
        }
    }

    /// [soft 0x080d1020 NTHandler]: a handler that calls `SI_serverExit(signal, 0)`
    /// (nothing installs it in this build).
    pub fn nt_handler(&mut self, signal: i32) -> Run<()> {
        self.server_exit(signal, 0)
    }
}
