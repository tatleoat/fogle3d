// SPDX-License-Identifier: MIT
//! `emergencySave.c`: what the program does when it crashes. A fault signal reaches
//! `SI_serverExit`, which calls the handler set up here: it asks (through its own dialog or
//! the external program `xconfirm`) whether to save, writes the scene into a fresh
//! database in a temporary directory, leaves a note in `~/.siCrashLog`, and offers to
//! start the program again with the same command line. At the next start the note is
//! found and the user is asked whether to load the saved scene.
//!
//! The port starts nothing: sounds, `gclear`, `xconfirm`, `rm` and the restart are command
//! lines handed to [`Sys::system`], [`Sys::popen_line`] and `SI_system`.

use crate::env::{Bytes, Exit, Run, World, P};
use crate::state::{App, CRASH_UI_NONE, CRASH_UI_OWN, CRASH_UI_XCONFIRM};

pub const SFPLAY: &[u8] = b"/usr/sbin/sfplay";
pub const XCONFIRM: &[u8] = b"/usr/bin/X11/xconfirm";
pub const GCLEAR: &[u8] = b"/usr/sbin/gclear";
pub const CRASH_LOG: &[u8] = b".siCrashLog";
pub const SOUND_CRASH: &[u8] = b"/usr/share/data/sounds/prosonus/musictags/belltree_up2.aiff";
pub const SOUND_SAVED: &[u8] = b"/usr/share/data/sounds/prosonus/musictags/harp_glis.Cmj.aiff";
/// `crashInformButtons` (0x086be2d0) and the answer that means no (0x086be2d4).
pub const INFORM_BUTTONS: &[u8] = b"OK  ";
pub const NO_BUTTON: &[u8] = b"No";

/// [soft 0x08392de0 stripTrailSlash]: removes every `/` and `\` at the end of a string
/// (a string of nothing else becomes empty).
pub fn strip_trail_slash(s: &mut Bytes) {
    while let Some(&c) = s.last() {
        if c == b'\\' || c == b'/' {
            s.pop();
        } else {
            break;
        }
    }
}

impl<W: World> App<W> {
    /// [soft 0x08392a80 SI_emergencyCleanup]: nothing in this build.
    pub fn emergency_cleanup(&mut self, _flag: i16) {}

    // adapter: not in place
    /// `SI_replicateCommandLine (0x08392a88)`: the image port
    /// ([`crate::image::batch::replicate_command_line`]) on a scratch image; the string is
    /// kept in the struct form.
    pub fn replicate_command_line(&mut self, args: &[Bytes]) -> &[u8] {
        let mut m = crate::image::adapter::scratch();
        let at: Vec<u32> = args.iter().map(|a| m.place_cstr(a)).collect();
        let argv = m.place(4 * at.len() as u32 + 4, 4);
        for (k, p) in at.iter().enumerate() {
            m.set_u32(argv + 4 * k as u32, *p);
        }
        let line = crate::image::batch::replicate_command_line(&mut m, args.len() as i32, argv);
        self.g.emergency.command_line = m.cstr(line);
        &self.g.emergency.command_line
    }

    /// [soft 0x08392ae0 SI_returnCommandLine].
    pub fn return_command_line(&self) -> &[u8] {
        &self.g.emergency.command_line
    }

    /// [soft 0x08392e50 tempCrashLogName]: `<home>/.siCrashLog`, or `/usr/tmp/.siCrashLog`
    /// when the user has no password entry.
    pub fn temp_crash_log_name(&mut self) -> Bytes {
        match self.w.home_of_user() {
            Some(home) => [&home[..], b"/", CRASH_LOG].concat(),
            None => [b"/usr/tmp".as_slice(), b"/", CRASH_LOG].concat(),
        }
    }

    /// [soft 0x08392aec SI_lastCrashLog]: with `remove` 0, the first line of the crash log
    /// without its new line (the directory the last crash saved into), or nothing when
    /// there is no log or it is empty; otherwise the log is deleted and nothing returned.
    pub fn last_crash_log(&mut self, remove: i16) -> Option<Bytes> {
        let name = self.temp_crash_log_name();
        if remove == 0 {
            let content = self.w.read_file(&name)?;
            // fgets(buffer, 0x2000): up to 8,191 bytes or the first new line, which is kept;
            // nothing at the end of the file
            if content.is_empty() {
                return None;
            }
            let mut line: Bytes = content.iter().copied().take(0x1fff).collect();
            if let Some(nl) = line.iter().position(|&c| c == b'\n') {
                line.truncate(nl + 1);
            }
            // what follows works on the C string: up to the first zero byte
            if let Some(z) = line.iter().position(|&c| c == 0) {
                line.truncate(z);
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            Some(line)
        } else {
            self.w.unlink(&name);
            None
        }
    }

    /// [soft 0x08392bf4 SI_emergencySaving]: not 0 while the handler is saving.
    pub fn emergency_saving(&self) -> i32 {
        self.g.emergency.saving as i32
    }

    /// [soft 0x08392c04 SI_emergencySaveEnabled].
    pub fn emergency_save_enabled(&self) -> i32 {
        self.g.emergency.enabled as i32
    }

    /// [soft 0x08392c14 SI_emergencySaveFunc]: the handler, if emergency save is on.
    pub fn emergency_save_func(&self) -> bool {
        self.g.emergency.enabled != 0 && self.g.emergency.handler
    }

    /// [soft 0x08392c30 SI_setupEmergencySave]: decides whether emergency save is on and
    /// where it saves. `mode` 0 switches it off. Otherwise it is off in the batch
    /// renderer, and else follows `SI_EMERGENCY_SAVE`: unset, `1` or `on` → on; `0` or
    /// `off` → off; anything else → on, and if the value names a writable directory, that
    /// is where the scene goes.
    pub fn setup_emergency_save(&mut self, mode: i16) {
        let mut enable = mode;
        let mut dir = None;
        if mode != 0 {
            if self.g.standalone_renderer != 0 {
                enable = 0;
            } else {
                match self.w.getenv(b"SI_EMERGENCY_SAVE") {
                    None => enable = 1,
                    Some(v) => {
                        if v == b"0" || v.eq_ignore_ascii_case(b"off") {
                            enable = 0;
                        } else {
                            enable = 1;
                            if v != b"1" && !v.eq_ignore_ascii_case(b"on") {
                                if let Some(real) = self.w.realpath(&v) {
                                    if self.w.access(&real, 2) == 0 && self.w.stat_is_dir(&real) == Some(true) {
                                        dir = Some(real);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.init_emergency_save(enable, dir);
    }

    /// [soft 0x08392ec0 emergencyDeleteDirectoryPath]: `rm -rf <path>` (in the background
    /// when asked) if the path is writable; 1 when the command was issued.
    pub fn emergency_delete_directory_path(&mut self, path: Option<&[u8]>, background: i16) -> i32 {
        let Some(path) = path else { return 0 };
        if self.w.access(path, 2) != 0 {
            return 0;
        }
        let mut command = [b"rm -rf ".as_slice(), path].concat();
        if background != 0 {
            command.extend_from_slice(b" &");
        }
        self.w.SI_system(&command);
        1
    }

    /// [soft 0x08392f40 checkTools]: which helper programs exist. `sfplay` counts only if
    /// both sound files are readable too.
    pub fn check_tools(&mut self) {
        let saved = self.g.emergency.sound_saved.clone();
        let crash = self.g.emergency.sound_crash.clone();
        let sfplay = self.w.access(SFPLAY, 1) == 0 && self.w.access(&saved, 4) == 0 && self.w.access(&crash, 4) == 0;
        self.g.emergency.have_sfplay = sfplay as i16;
        self.g.emergency.have_xconfirm = (self.w.access(XCONFIRM, 1) == 0) as i16;
        self.g.emergency.have_gclear = (self.w.access(GCLEAR, 1) == 0) as i16;
    }

    /// [soft 0x08392ff4 initEmergencySave]: fills the emergency-save record. Off: the
    /// record is cleared. On: the handler is set, the sound files are chosen
    /// (`SI_SOUND_CRASH_BLAMMO`, `SI_SOUND_CRASH_SAVE_OK`, or the stock ones), the helper
    /// programs are looked for (without `xconfirm` the program's own dialog is used,
    /// without `sfplay` no sounds), and a name for the save directory is made with
    /// `tempnam`: prefix `siKO-`, in `/usr/tmp`, or in the directory given, for which
    /// `TMPDIR` is pointed at that directory for the length of the call. The crash log's
    /// name is fixed and a database record prepared for the directory.
    ///
    /// The strings are 4,095-byte fields in the original; a longer directory name is cut
    /// only for the save directory.
    pub fn init_emergency_save(&mut self, enable: i16, dir: Option<Bytes>) {
        self.g.emergency.save_dir.clear();
        match dir {
            None => self.g.emergency.wanted_dir.clear(),
            Some(mut d) => {
                strip_trail_slash(&mut d);
                self.g.emergency.wanted_dir = d;
            }
        }
        let e = &mut self.g.emergency;
        e.entered = 0;
        e.saving = 0;
        e.enabled = enable;
        e.word503c = 3;
        if enable == 0 {
            e.crash_log.clear();
            e.handler = false;
            e.have_sfplay = 0;
            e.have_xconfirm = 0;
            e.have_gclear = 0;
            return;
        }
        e.handler = true;
        let custom = if self.g.emergency.play_sound_cues != 0 { self.w.getenv(b"SI_SOUND_CRASH_BLAMMO") } else { None };
        self.g.emergency.sound_crash = custom.unwrap_or_else(|| SOUND_CRASH.to_vec());
        let custom = if self.g.emergency.play_sound_cues != 0 { self.w.getenv(b"SI_SOUND_CRASH_SAVE_OK") } else { None };
        self.g.emergency.sound_saved = custom.unwrap_or_else(|| SOUND_SAVED.to_vec());
        self.check_tools();
        let e = &mut self.g.emergency;
        if e.crash_ui_mode == CRASH_UI_XCONFIRM && e.have_xconfirm == 0 {
            e.crash_ui_mode = CRASH_UI_OWN;
        }
        if e.play_sound_cues != 0 && e.have_sfplay == 0 {
            e.play_sound_cues = 0;
        }
        let wanted = e.wanted_dir.clone();
        let name = if wanted.is_empty() {
            self.w.tempnam(b"/usr/tmp", b"siKO-")
        } else {
            let old = self.w.getenv(b"TMPDIR");
            if old.is_some() {
                self.w.putenv(&[b"TMPDIR=".as_slice(), &wanted].concat());
            }
            let name = self.w.tempnam(&wanted, b"siKO-");
            if let Some(old) = old {
                self.w.putenv(&[b"TMPDIR=".as_slice(), &old].concat());
            }
            name
        };
        self.g.emergency.save_dir = match name {
            None => self.w.tmpnam(),
            Some(mut n) => {
                n.truncate(0xfff);
                n
            }
        };
        self.g.emergency.crash_log = self.temp_crash_log_name();
        self.w.SI_dbInfoInit(P);
        self.g.emergency.db_path = Some(self.g.emergency.save_dir.clone());
    }

    /// [soft 0x08393350 playSoundCue]: plays a sound file in the background with `sfplay`,
    /// if sounds are on.
    pub fn play_sound_cue(&mut self, file: Option<&[u8]>) {
        if let Some(file) = file {
            if self.g.emergency.play_sound_cues != 0 {
                self.w.system(&[SFPLAY, b" ", file, b" &"].concat());
            }
        }
    }

    /// [soft 0x08393398 crashWarning]: shows three lines and waits for an answer. With
    /// `inform` not 0 there is one button and the answer is always 1; otherwise the answer
    /// is 1 for yes and 0 for no. `mode` 0 uses the program's own request dialogs (with
    /// the batch-renderer flag cleared for the call), 1 the external `xconfirm` (the answer
    /// is no when its output starts with "No"), anything else asks nothing and answers 1.
    pub fn crash_warning(&mut self, line1: &[u8], line2: &[u8], line3: &[u8], mode: i32, inform: i16) -> i32 {
        let mut result: i16 = 1;
        if mode != CRASH_UI_XCONFIRM {
            if mode != CRASH_UI_OWN {
                return 1;
            }
            let saved = std::mem::replace(&mut self.g.standalone_renderer, 0);
            if inform == 0 {
                result = self.system_warning(line1, line2, line3) as i16;
            } else {
                self.dialog_request(line1, line2, line3, b"OK");
            }
            self.g.standalone_renderer = saved;
            return result as i32;
        }
        let place: &[u8] = if self.g.emergency.have_gclear != 0 && self.g.emergency.pre_clear != 0 { b"-c" } else { b"-geometry +40+40" };
        let mut command = [XCONFIRM, b" ", place, b" -t '", line1, b"' -t '", line2, b"' -t '", line3, b"'"].concat();
        if inform != 0 {
            command.extend_from_slice(&[b" -icon info -header 'SoftImage Fatal Error !' -B ".as_slice(), INFORM_BUTTONS].concat());
        } else {
            command.extend_from_slice(&[b" -icon critical -header 'SoftImage Fatal Error !' -B ".as_slice(), INFORM_BUTTONS, b" -b ", NO_BUTTON].concat());
        }
        let reply = self.w.popen_line(&command).unwrap_or_default();
        if reply.starts_with(NO_BUTTON) {
            result = 0;
        }
        result as i32
    }

    fn offer_restart(&mut self) {
        if self.g.emergency.crash_ui_mode == CRASH_UI_NONE {
            return;
        }
        let line = self.g.emergency.command_line.clone();
        let mode = self.g.emergency.crash_ui_mode;
        if self.crash_warning(b"Shall I try to restart soft, with :", &line, b"as the command ?", mode, 0) as i16 != 0 {
            self.w.err(b"|------- Trying to start a replacement soft....\n");
            self.w.err(b"|------- \n");
            let r = self.w.system(&[b"(sleep 2; ".as_slice(), &line, b") &"].concat());
            if r == -1 {
                // "%d" of errno: the recorder has no errno of its own
                self.w.err(b"|------- Failed start a replacement soft. Error code 0\n");
            }
        }
    }

    /// [soft 0x08393594 emergencySave]: the crash handler. It never returns: every path
    /// ends in `exit` (through `SI_softCleanup`, which exits with 0; the statuses 1, 0 and
    /// 3 written after those calls are never reached).
    ///
    /// - Entered again after it finished its work: `exit(5)`.
    /// - Emergency save off: clean up and exit.
    /// - First entry: the fault signals get their default action again, the crash sound
    ///   plays, the user is asked "Attempt to save backup ?"; on yes the scene is saved
    ///   into a new database in the save directory and the crash log written (the
    ///   directory, the time, "Last SI|3D blammo", "Hello Mum !"). Then the restart
    ///   question, the clean-up, the end.
    /// - Entered while saving (the save itself crashed): the second time "crashed too many
    ///   times", restart question, clean-up; the third time `exit(2)`.
    pub fn emergency_save(&mut self) -> Run<()> {
        if self.g.emergency.immediate_abort != 0 {
            return Err(Exit(5));
        }
        if self.g.emergency.enabled == 0 {
            self.g.emergency.immediate_abort = 1;
            self.soft_cleanup(1)?;
            return Err(Exit(1));
        }
        let before = self.g.emergency.entered;
        self.g.emergency.entered = before.wrapping_add(1);
        if self.g.emergency.saving == 0 && before.wrapping_add(1) <= 1 {
            self.signal_init(0);
            let sound = self.g.emergency.sound_crash.clone();
            self.play_sound_cue(Some(&sound));
            self.g.emergency.saving = 1;
            self.g.standalone_renderer = 1;
            self.w.mem_set_rgb_active(1);
            if self.g.emergency.have_gclear != 0 && self.g.emergency.pre_clear != 0 {
                self.w.system(&[GCLEAR, b" &"].concat());
            }
            self.w.curson();
            let mode = self.g.emergency.crash_ui_mode;
            if self.crash_warning(b"Fatal Error detected", b"Soft will die", b"Attempt to save backup ?", mode, 0) as i16 != 0 {
                self.w.SI_dbCreateExtDatabase(P);
                self.w.SI_dbSaveScene(P, P, 1, 0, None, 0);
                let sound = self.g.emergency.sound_saved.clone();
                self.play_sound_cue(Some(&sound));
                let dir = self.g.emergency.save_dir.clone();
                let mode = self.g.emergency.crash_ui_mode;
                self.crash_warning(b"AutoSave appeared to work OK", &dir, b"See the above directory for database", mode, 1);
                self.w.err(b"|------- Crash scene saved in dummy database\n");
                self.w.err(&[b"|------- Please see directory ".as_slice(), &dir, b"\n"].concat());
                let log = self.g.emergency.crash_log.clone();
                let now = self.w.time();
                let stamp = self.w.ctime(now);
                self.w.write_file(&log, &[&dir[..], b"\n", &stamp, b"Last SI|3D blammo\n", b"Hello Mum !\n"].concat(), false);
            }
            self.offer_restart();
            self.g.emergency.immediate_abort = 1;
            if self.g.emergency.have_gclear != 0 && self.g.emergency.pre_clear == 0 {
                self.w.system(&[GCLEAR, b" &"].concat());
            }
            self.soft_cleanup(1)?;
            return Err(Exit(0));
        }
        if before.wrapping_add(1) > 2 {
            return Err(Exit(2));
        }
        let sound = self.g.emergency.sound_crash.clone();
        self.play_sound_cue(Some(&sound));
        let mode = self.g.emergency.crash_ui_mode;
        self.crash_warning(b"Sorry - I tried, but..", b"..crashed too many times.", b"Will attempt cleanup and exit", mode, 1);
        self.offer_restart();
        self.g.emergency.immediate_abort = 1;
        self.soft_cleanup(1)?;
        Err(Exit(3))
    }
}
