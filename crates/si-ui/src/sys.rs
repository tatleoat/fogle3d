// SPDX-License-Identifier: MIT
//! Everything the package's six source files call outside themselves, one trait per
//! subsystem, each carrying exactly the calls the original makes with exactly its arguments.
//!
//! | Trait | The original's source files | Ported? |
//! |---|---|---|
//! | [`Gl`] | the IRIS GL layer (`crates/si-gl`) | yes: [`crate::gl_adapter`] hands each call to `si_gl::IrisGl` |
//! | [`Libc`] | the C library's system services | outside the program |
//! | [`Memory`] | `sys_memory.c` | here: the recorder allocates from the image's heap |
//! | [`Controls`] | `control.c`: the groups and controls of the menu columns | no |
//! | [`Dialogs`] | `dialog.c`, `request.c`, `sys_in_out.c`, `uiLite.c` | no |
//! | [`Menus`] | `menu.c` | no |
//! | [`Status`] | `status.c`: the status line | no |
//! | [`Boxes`] | `colorBox.c`, `playBox.c`, `modeBox.c`, `fslider.c`, `infoLinux.c` | no |
//! | [`Server`] | `server.c`: the canvases, the module switch; the databases' part in the automatic backup | no |
//! | [`Keys`] | `map_suprakey.c`, `quickkeys.c`: swift keys | no |
//! | [`Devices`] | `tablet.c`, `puck.c` | no |
//! | [`Journal`] | `log.c`: the session log | no |
//! | [`Modules`] | `modeler.c`, `scene.c`, `actor.c`, `matter.c`, `tools.c` | no |
//! | [`Motion`] | `motion.c`, `motionDialog.c`, `motionUpdate.c`, `motionUtils.c`, `fastplay.c` | in part, in `si-motion` |
//! | [`Views`] | `draw.c`, `view.c`, `ruler.c`, `canvas.c`, the view kinds, rotoscopy, cameras | no |
//! | [`ImageLib`] | the SGI image library linked into the program | no |
//!
//! Method names are the original's function names, so a method is one call of that function.
//! Every method takes the memory image first: the callee works on the same memory (a menu
//! fills in the event it is handed, a dialog reads its own record). A `&mut` parameter stands
//! for a pointer to a local variable of the caller through which the callee only writes.
//!
//! The declarations below are also read by `re/tools/si-ui/emu_ui.py`, which replaces each of
//! these functions in the emulated original by a stand-in that logs the call the same way
//! and gives the same answer. The parameter types say how: `i16`/`u16` 16 bits of the stack
//! word, `u8` 8 bits, `f32`, `f64`, `&[u8]` a string by content, `&mut T` an output,
//! `InOut<T>` a pointer to a local the callee reads and writes, `&[T; N]` words read through
//! a pointer, `[u32; N]` a structure passed by value, everything else the 32-bit word.

use crate::mem::{Addr, Mem, P};
use crate::rec::{new_log, Arg, InOut, Ret, SharedLog, Val};
use crate::records::{Canvas, Control, Event, Grafport, Group};

/// Declares a subsystem's trait and its recording implementation from one list.
macro_rules! outside {
    (
        $(#[$tm:meta])*
        pub trait $tr:ident, recorded by $rec:ident {
            $( $(#[$m:meta])* fn $name:ident ( $($a:ident : $t:ty),* $(,)? ) $(-> $r:ty)? ; )*
            $( manual: $( $(#[$mm:meta])* fn $mname:ident ( $log:ident, $mem:ident ; $($ma:ident : $mt:ty),* $(,)? ) $(-> $mr:ty)? $body:block )* )?
        }
    ) => {
        $(#[$tm])*
        #[allow(non_snake_case)]
        pub trait $tr {
            $( $(#[$m])* fn $name(&mut self, m: &mut Mem, $($a: $t),*) $(-> $r)?; )*
            $( $( $(#[$mm])* fn $mname(&mut self, m: &mut Mem, $($ma: $mt),*) $(-> $mr)?; )* )?
        }

        /// The recording implementation: logs each call and answers from the shared log.
        pub struct $rec {
            pub log: SharedLog,
        }

        #[allow(non_snake_case, unused_variables, unused_mut)]
        impl $tr for $rec {
            $( fn $name(&mut self, m: &mut Mem, $(mut $a: $t),*) $(-> $r)? {
                let ans = self.log.borrow_mut().call(m, stringify!($name), &[$(Arg::val(&$a)),*]);
                let mut next = 1;
                $( Arg::fill(&mut $a, &ans, &mut next); )*
                Ret::ret(&ans)
            } )*
            $( $( fn $mname(&mut self, $mem: &mut Mem, $($ma: $mt),*) $(-> $mr)? {
                let $log = &self.log;
                $body
            } )* )?
        }
    };
}

/// One argument after the format of a `SI_log` call.
#[derive(Clone, Copy, Debug)]
pub enum LogArg {
    /// For a `%d`.
    I(i32),
    /// For a `%s`: the address of the string.
    S(Addr),
}

outside! {
    /// The IRIS GL calls of the two files. Device numbers are 16 bits (`Device`).
    pub trait Gl, recorded by RecGl {
        fn qtest() -> i32;
        fn qread(value: &mut i16) -> i32;
        fn qenter(device: u16, value: i16);
        fn qdevice(device: u16);
        fn unqdevice(device: u16);
        fn tie(button: u16, valuator1: u16, valuator2: u16);
        fn getbutton(device: u16) -> i32;
        fn getvaluator(device: u16) -> i32;
        fn winset(id: i32);
        fn winget() -> i32;
        fn drawmode(mode: i32);
        fn getdrawmode() -> i32;
        fn frontbuffer(enable: i32);
        fn backbuffer(enable: i32);
        fn getbuffer() -> i32;
        fn swapbuffers();
        fn gflush();
        fn gsync();
        fn color(index: u16);
        fn clear();
        fn writemask(mask: u16);
        fn getwritemask() -> i32;
        fn font(index: i16);
        fn getdescender() -> i32;
        fn strwidth(text: &[u8]) -> i32;
        fn charstr(text: &[u8]);
        fn cmov2i(x: i32, y: i32);
        fn recti(x1: i32, y1: i32, x2: i32, y2: i32);
        fn sboxi(x1: i32, y1: i32, x2: i32, y2: i32);
        fn sboxfi(x1: i32, y1: i32, x2: i32, y2: i32);
        fn bgnline();
        fn endline();
        fn bgnclosedline();
        fn endclosedline();
        fn bgnpolygon();
        fn endpolygon();
        fn sboxs(x1: i16, y1: i16, x2: i16, y2: i16);
        fn setpattern(index: i16);
        fn pushviewport();
        fn popviewport();
        fn pushmatrix();
        fn popmatrix();
        /// `ortho2(left, right, bottom, top)`: four floats.
        fn ortho2(left: f32, right: f32, bottom: f32, top: f32);
        /// `viewport(left, right, bottom, top)`: shorts, passed as stack words.
        fn viewport(left: i16, right: i16, bottom: i16, top: i16);
        /// `RGBcolor(red, green, blue)`: the original passes sign-extended 16-bit values.
        fn RGBcolor(red: i16, green: i16, blue: i16);
        fn move2s(x: i16, y: i16);
        fn draw2s(x: i16, y: i16);
        fn setvaluator(device: u16, init: i16, min: i16, max: i16);
        fn deflinestyle(index: i16, pattern: u16);
        /// `mask` is the address of the pattern's 16-bit rows.
        fn defpattern(index: i16, size: i16, mask: Addr);
        /// `glyph` is the address of the cursor's 16-bit rows.
        fn defcursor(index: i16, glyph: Addr);
        fn curorigin(index: i16, x: i16, y: i16);
        fn curstype(kind: i32);
        /// The original passes two more words, both 0 (IRIS GL's colour and write mask).
        fn setcursor(index: i16, colour: i32, mask: i32);
        fn getgdesc(inquiry: i32) -> i32;
        fn mapcolor(index: u16, red: i16, green: i16, blue: i16);
        fn noport();
        fn foreground();
        fn winopen(name: &[u8]) -> i32;
        fn winclose(id: i32);
        fn prefposition(x1: i32, x2: i32, y1: i32, y2: i32);
        fn RGBmode();
        fn gconfig();
        fn winpop();
        manual:
        /// `getsize(&width, &height)`: the current window's size into two 32-bit words of
        /// the image.
        fn getsize(log, m; width: Addr, height: Addr) {
            let ans = log.borrow_mut().call(m, "getsize", &[Val::Out, Val::Out]);
            m.set_i32(width, ans.val(1) as i32);
            m.set_i32(height, ans.val(2) as i32);
        }
        /// `getmcolor(index, &red, &green, &blue)`: a colour-map entry into three 16-bit
        /// words at addresses of the image (globals, or locals on the image's stack).
        fn getmcolor(log, m; index: u16, red: Addr, green: Addr, blue: Addr) {
            let ans = log.borrow_mut().call(m, "getmcolor", &[Val::H(index), Val::Out, Val::Out, Val::Out]);
            for (i, at) in [red, green, blue].into_iter().enumerate() {
                m.set_i16(at, ans.val(1 + i) as i16);
            }
        }
        /// `qcontrol(command, 0, 0, n out, out)` with a local array for the answer.
        fn qcontrol_out(log, m; cmd: i32, out: &mut [i16]) -> i32 {
            let ans = log.borrow_mut().call(m, "qcontrol", &[Val::W(cmd as u32), Val::W(0), Val::W(0), Val::W(out.len() as u32), Val::Out]);
            for (i, v) in out.iter_mut().enumerate() {
                *v = ans.val(1 + i) as i16;
            }
            ans.val(0) as i32
        }
        /// `getdev(n, devices, values)`: the state of `n` devices. `devices` is in the image.
        fn getdev(log, m; n: i32, devices: Addr, values: &mut [i16]) {
            let devs: Vec<u32> = (0..n.max(0) as u32).map(|i| m.u16(devices + 2 * i) as u32).collect();
            let ans = log.borrow_mut().call(m, "getdev", &[Val::W(n as u32), Val::A(devs), Val::Out]);
            for (i, v) in values.iter_mut().enumerate() {
                *v = ans.val(1 + i) as i16;
            }
        }
        /// `qcontrol(command, n in, in, n out, out)`. The input is logged by content (the
        /// original passes a local array); the output pointer is in the image.
        fn qcontrol(log, m; cmd: i32, idata: Option<&[i16]>, ocount: i32, odata: Addr) -> i32 {
            let (n, data) = match idata {
                Some(d) => (d.len() as u32, Val::A(d.iter().map(|v| *v as u16 as u32).collect())),
                None => (0, Val::W(0)),
            };
            let ans = log.borrow_mut().call(m, "qcontrol", &[Val::W(cmd as u32), Val::W(n), data, Val::W(ocount as u32), Val::W(odata)]);
            ans.val(0) as i32
        }
        /// `v2i(vector)`: a vertex of two 32-bit integers.
        fn v2i(log, m; v: [i32; 2]) {
            log.borrow_mut().call(m, "v2i", &[Val::A(vec![v[0] as u32, v[1] as u32])]);
        }
        /// `v2s(vector)`: a vertex of two 16-bit integers.
        fn v2s(log, m; v: [i16; 2]) {
            log.borrow_mut().call(m, "v2s", &[Val::A(vec![v[0] as u16 as u32, v[1] as u16 as u32])]);
        }
    }
}

outside! {
    /// The C library's system services.
    pub trait Libc, recorded by RecLibc {
        fn usleep(microseconds: u32) -> i32;
        fn perror(text: &[u8]);
        /// `time(NULL)`: the argument is the pointer the original passes, 0.
        fn time(tloc: Addr) -> i32;
        fn umask(mask: i32) -> i32;
        /// `__sysconf(name)`: the scroll bars ask for the clock ticks per second (name 2).
        fn __sysconf(name: i32) -> i32;
        /// `atof`'s `__strtod_internal(text, NULL, 0)`: the number in st(0).
        fn __strtod_internal(text: &[u8], endptr: Addr, group: i32) -> f64;
        /// The text for an error number: the address of a string of the library's.
        fn strerror(errnum: i32) -> Addr;
        manual:
        /// `printf(text)` for a text without conversions: the standard output.
        fn printf(log, m; text: &[u8]) {
            log.borrow_mut().call(m, "printf", &[Val::S(text.to_vec())]);
        }
        /// `fprintf(stream, format, …)` with `%s` and `%d` arguments: the program's
        /// messages on the standard error stream.
        fn fprintf(log, m; stream: Addr, format: &[u8], args: &[LogArg]) {
            let mut v = vec![Val::W(stream), Val::S(format.to_vec())];
            for a in args {
                v.push(match a {
                    LogArg::I(i) => Val::W(*i as u32),
                    LogArg::S(0) => Val::W(0),
                    LogArg::S(p) => Val::S(m.cstr(*p)),
                });
            }
            log.borrow_mut().call(m, "fprintf", &v);
        }
        /// `times(&buffer)`: the clock in ticks. The buffer is a local nothing reads.
        fn times(log, m; ) -> i32 {
            log.borrow_mut().call(m, "times", &[Val::Out]).val(0) as i32
        }
        /// `getenv(name)`: the address of the value, null when the variable is not set.
        fn getenv(log, m; name: &[u8]) -> Addr {
            log.borrow_mut().call(m, "getenv", &[Val::S(name.to_vec())]).val(0) as u32
        }
    }
}

outside! {
    /// `sys_memory.c`. The recording implementation hands out zeroed blocks of the image's
    /// heap, at the addresses the check's stand-in gives the original.
    pub trait Memory, recorded by RecMemory {
        manual:
        fn SI_calloc(log, m; count: u32, size: i32) -> Addr {
            log.borrow_mut().call(m, "SI_calloc", &[Val::W(count), Val::W(size as u32)]);
            m.malloc(count.wrapping_mul(size as u32))
        }
        /// The C library's `strdup`: a copy of a string in a new block.
        fn strdup(log, m; text: Addr) -> Addr {
            let s = m.cstr(text);
            log.borrow_mut().call(m, "strdup", &[Val::S(s.clone())]);
            let at = m.malloc(s.len() as u32 + 1);
            if at != 0 {
                m.put_cstr(at, &s);
            }
            at
        }
        /// The C library's `free`. The recorder's blocks are never reused.
        fn free(log, m; block: Addr) {
            log.borrow_mut().call(m, "free", &[Val::W(block)]);
        }
        /// C++ `operator new` (GCC 2.95's `__builtin_new`): a block whose bytes the caller
        /// must set.
        fn __builtin_new(log, m; size: u32) -> Addr {
            log.borrow_mut().call(m, "__builtin_new", &[Val::W(size)]);
            m.malloc(size)
        }
        /// The C library's `malloc` as the program wraps it: a block whose bytes the caller
        /// must set.
        fn SI_malloc(log, m; size: u32) -> Addr {
            log.borrow_mut().call(m, "SI_malloc", &[Val::W(size)]);
            m.malloc(size)
        }
        /// C++ `operator new[]`.
        fn __builtin_vec_new(log, m; size: u32) -> Addr {
            log.borrow_mut().call(m, "__builtin_vec_new", &[Val::W(size)]);
            m.malloc(size)
        }
        /// C++ `operator delete`.
        fn __builtin_delete(log, m; block: Addr) {
            log.borrow_mut().call(m, "__builtin_delete", &[Val::W(block)]);
        }
        /// C++ `operator delete[]`.
        fn __builtin_vec_delete(log, m; block: Addr) {
            log.borrow_mut().call(m, "__builtin_vec_delete", &[Val::W(block)]);
        }
    }
}

outside! {
    /// `control.c`: the groups and controls of the menu columns.
    pub trait Controls, recorded by RecControls {
        fn si_groupDraw(group: P<Group>, how: i16);
        fn si_groupFindIt(canvas: P<Canvas>, column: i16, x: i16, y: i16, group: &mut P<Group>, control: &mut P<Control>, part: &mut i16);
        fn si_groupMnemonicFindIt(canvas: P<Canvas>, letter: u8, mode: i16, group: &mut P<Group>, control: &mut P<Control>, part: &mut i16);
        fn SI_groupSelect(canvas: P<Canvas>, group: P<Group>, control: P<Control>);
        fn SI_groupSelectRightButton(canvas: P<Canvas>, group: P<Group>, control: P<Control>);
        fn si_groupDeactivateCanvas(canvas: P<Canvas>, how: i16);
        fn SI_controlOff(event: P<Event>, how: i16);
        fn SI_controlSetStr(group: i32, control: i32, text: &[u8]);
        fn si_get_ctrl_values(group: i16, control: i16) -> f64;
        fn SI_refreshAllControls();
        fn SI_superModeOff(how: i16);
        /// The function a text-list row names (`ScrollItem::callback`), called through the
        /// pointer after the row is drawn: the items, the row, the text's x and y.
        fn scrollRowCallback(items: Addr, row: i32, x: i32, y: i32);
        /// The function a scroll bar names (`Scrollbar::callback`), called through the pointer:
        /// the bar, its user data and the new position.
        fn scrollbarCallback(bar: Addr, user: Addr, pos: i32);
        /// `dopeSheet.c`: the dope sheet's items drawn in a view.
        fn SI_dopeSheetAllItemDraw(view: Addr);
    }
}

outside! {
    /// `dialog.c`, `request.c`, `sys_in_out.c`, `uiLite.c`: dialogs.
    pub trait Dialogs, recorded by RecDialogs {
        fn SI_dialogStart();
        fn SI_dialogEnd();
        fn si_dialIsOpenWinId(winid: i32) -> i16;
        fn SI_dialogHandleEvent(dialog: Addr) -> P<Event>;
        fn si_dialogHandleKeyDown(dialog: Addr, device: i32, value: i32, event: P<Event>) -> i16;
        fn SI_setTexed(dialog: Addr, item: i16, text: &[u8]);
        fn si_fndDialogActive(winid: i32) -> Addr;
        fn si_calculateTextEdit(dialog: Addr, item: [u32; 10]);
        fn SI_dialogUpdateFcv();
        fn SI_systemWarning(line1: &[u8], line2: &[u8], line3: &[u8]) -> i16;
        fn SI_dialogShowStatus(path: Addr) -> i16;
        fn SI_dialogGet(id: i32) -> Addr;
        fn SI_dialogOpen(id: i32) -> Addr;
        fn SI_dialogDraw(dialog: Addr);
        fn SI_windowGetNextActive(window: InOut<P<Grafport>>) -> i16;
        fn si_dialogsSetCursor(index: i32);
        fn si_statDialHandler(event: P<Event>) -> i32;
        fn SI_dialogRefresh();
        fn SI_getStateItem(dialog: Addr, item: i16, state: &mut i16);
    }
}

outside! {
    /// `menu.c`.
    pub trait Menus, recorded by RecMenus {
        fn si_learnMode() -> i16;
        fn si_menuSelect(menu: Addr, event: P<Event>, x: i16, y: i16);
        fn SI_popupFindLastSelect(menu: Addr, kind: Addr, first: Addr, second: Addr, extra: Addr);
        fn SI_popupList(window: P<Grafport>, menu: Addr) -> i32;
        fn SI_popupListMultiSelection(window: P<Grafport>, menu: Addr, bits: Addr);
        fn SI_setMenuFlag(menu: Addr, item: i32, on: i16);
        /// Read a pop-up menu from a resource file (a `FILE *` of `crate::file`); the menu.
        fn si_popupRead(stream: Addr, a: Addr, b: i32, kind: i16) -> Addr;
    }
}

outside! {
    /// `status.c`: the status line.
    pub trait Status, recorded by RecStatus {
        fn SI_statusDisplayMessage(text: &[u8], kind: i16);
        fn SI_statusDisplayMouse(left: &[u8], middle: &[u8], right: &[u8]);
        fn si_statusDisplayMouseMenuDefault();
        fn SI_statusOpenFile();
        fn si_statusCloseFile();
        fn si_statusFlagSet(flag: i16);
    }
}

outside! {
    /// `colorBox.c`, `playBox.c`, `modeBox.c`, `fslider.c`, `infoLinux.c`: the boxes of the
    /// lower edge of the screen.
    pub trait Boxes, recorded by RecBoxes {
        fn si_colorBoxDisplay();
        fn si_colorBoxFnd(x: i16, y: i16) -> i16;
        fn si_colorBoxSelect(cell: i16, how: i16);
        fn si_colorBoxHide();
        fn SI_playBoxButton(x: i16, y: i16) -> i32;
        fn SI_playBoxHandle(which: i16);
        fn SI_playBoxHilight(button: i32, on: i16);
        fn SI_handleCurrentFrame() -> i32;
        fn SI_modeBoxHandle(x: i32, y: i32);
        fn SI_modeBoxRefresh();
        fn SI_directSrtHandle(group: i32, control: i32, value: f32, relative: u8);
        fn SI_fsliderDraw(frame: i32);
        fn si_fsliderSet(x: i32, first: i32);
        fn SI_infoLinux();
    }
}

outside! {
    /// `server.c`, `ui_misc.c`, `graf.c`: the canvases and the module switch.
    pub trait Server, recorded by RecServer {
        fn SI_isDialogEditor() -> i16;
        fn si_serverMonitor(canvas: P<Canvas>, x: i16, y: i16);
        fn SI_redrawEverything(how: i16);
        fn SI_serverClean();
        fn SI_serverExit(a: i32, b: i32);
        /// `dbase.c`, `dbdir.c`, `dbexternal.c`, `dbio.c`: what the automatic backup asks of
        /// the databases.
        fn SI_dbDirGetBackupDatabase(directory: Addr) -> Addr;
        fn SI_dbSetDbWriteLock(database: Addr, on: i32) -> i16;
        fn SI_dbRenameExtDatabase(database: Addr, name: &[u8]) -> i32;
        fn SI_dbCreateExtDatabase(database: Addr) -> i32;
        fn SI_dbSaveScene(database: Addr, dictionary: Addr, a: i16, b: i16, name: Addr, c: i16);
        fn SI_dbDeleteExtDatabase(name: Addr);
    }
}

outside! {
    /// `map_suprakey.c`, `quickkeys.c`: swift keys.
    pub trait Keys, recorded by RecKeys {
        fn SI_initSupraKeys();
        fn SI_isSupraModeValid(mode: i32) -> i16;
        fn si_deactivateSupraMode(mode: u32) -> i16;
        fn si_leaveSupraMode(mode: i32);
        fn si_enterSupraMode(mode: i32);
        fn si_findCurrentMode(device: u16, keys: &[i32; 4], position: &mut i16) -> i32;
        fn si_findDirectKey(device: u16, keys: &[i32; 4], position: &mut i16, from: i16) -> i16;
        fn SI_getQuickKeySequence(key: i32, index: i32) -> i32;
        fn SI_getQuickModule(key: i32) -> i32;
        fn SI_handleDirectKey(key: i16) -> i16;
        /// The call through the pointer `SI_detectSwiftMouseDSO` (a plug-in's function).
        fn SI_detectSwiftMouseDSO(x: i32, y: i32);
    }
}

outside! {
    /// `tablet.c`, `puck.c`.
    pub trait Devices, recorded by RecDevices {
        fn SI_tabletGetData() -> i32;
        fn SI_tabletGetButtonUpEvent() -> i32;
        fn si_deviceMouse();
    }
}

outside! {
    /// `log.c`: the session log.
    pub trait Journal, recorded by RecJournal {
        fn SI_mouseLog(device: i32, value: i16, x: i16, y: i16);
        fn SI_keyLog(device: i32, value: i16);
        manual:
        /// `SI_log(level, format, …)`: the format with its `%d` and `%s` arguments.
        fn SI_log(log, m; level: i32, format: &[u8], args: &[LogArg]) {
            let mut v = vec![Val::W(level as u32), Val::S(format.to_vec())];
            for a in args {
                v.push(match a {
                    LogArg::I(i) => Val::W(*i as u32),
                    LogArg::S(0) => Val::W(0),
                    LogArg::S(p) => Val::S(m.cstr(*p)),
                });
            }
            log.borrow_mut().call(m, "SI_log", &v);
        }
    }
}

outside! {
    /// `modeler.c`, `scene.c`, `actor.c`, `matter.c`, `tools.c`: the five modules.
    pub trait Modules, recorded by RecModules {
        fn SI_scnGetMode() -> i32;
        fn SI_actorGetMode() -> i16;
        fn SI_mtrGetMode() -> i32;
        fn SI_toolsGetMode() -> i32;
        fn SI_modSetCutPasteKeyMode();
        fn SI_motSetCutPasteKeyMode();
        fn SI_actSetCutPasteKeyMode();
        fn SI_matSetCutPasteKeyMode();
        fn SI_tolSetCutPasteKeyMode();
        fn SI_modSetSlopeManagementMode();
        fn SI_motSetSlopeManagementMode();
        fn SI_actSetSlopeManagementMode();
        fn SI_matSetSlopeManagementMode();
        fn SI_tolSetSlopeManagementMode();
        fn SI_modSetInterpManagementMode();
        fn SI_motSetInterpManagementMode();
        fn SI_actSetInterpManagementMode();
        fn SI_matSetInterpManagementMode();
        fn SI_tolSetInterpManagementMode();
        fn SI_modSetSetKeyMode();
        fn SI_motSetSetKeyMode();
        fn SI_actSetSetKeyMode();
        fn SI_matSetSetKeyMode();
        fn SI_tolSetSetKeyMode();
        fn SI_modSetEditKeyMode();
        fn SI_motSetEditKeyMode();
        fn SI_actSetEditKeyMode();
        fn SI_matSetEditKeyMode();
        fn SI_tolSetEditKeyMode();
        fn SI_modSetEditTagMode();
        fn SI_motSetEditTagMode();
        fn SI_actSetEditTagMode();
        fn SI_matSetEditTagMode();
        fn SI_tolSetEditTagMode();
        fn SI_modSetEditFcvMode();
        fn SI_motSetEditFcvMode();
        fn SI_actSetEditFcvMode();
        fn SI_matSetEditFcvMode();
        fn SI_tolSetEditFcvMode();
    }
}

outside! {
    /// `motion.c`, `motionDialog.c`, `motionUpdate.c`, `motionUtils.c`, `fastplay.c`.
    pub trait Motion, recorded by RecMotion {
        fn SI_playbackRun(mode: i32, right: i16, middle: i16);
        fn SI_sceneDirty() -> i16;
        fn SI_sceneUpdate(seconds: f32);
        fn SI_sceneKeySetColor(seconds: f32, a: i16, b: i16);
        fn SI_fastPlayClearAllView();
    }
}

outside! {
    /// `draw.c` and the code of the views.
    pub trait Views, recorded by RecViews {
        fn SI_refreshFcurve(how: i32);
        fn SI_refreshDopeSheet(how: i32);
        fn SI_refreshSelected(how: i16);
        fn si_inWFcvList(window: P<Grafport>, x: i32, y: i32) -> i16;
        fn SI_spdViewSizeNew(window: P<Grafport>);
        fn SI_refreshSystem(window: P<Grafport>, list: Addr, what: i32);
        fn SI_refreshAll(how: i16);
        fn SI_drawGrid(window: P<Grafport>, how: i16);
        fn SI_canvasDraw(canvas: P<Canvas>, how: i16);
        /// The call through a gadget's handler pointer when it is not one of `window.c`'s
        /// own functions: the argument record (event, x and y, window, part).
        fn gadgetCallback(arguments: &[u32; 4]);
        fn SI_snapshotSelectedFcvs();
        fn SI_swapSelectedFcvsToAux();
        fn SI_setSelectedFcurveInterp(kind: i16);
        fn SI_schematicSort(window: P<Grafport>, dictionary: Addr, how: i16);
        fn SI_schemSetDefaultColors();
        fn SI_schemComputeHierarchiesBBox(dictionary: Addr, a: i16, b: i16);
        fn SI_dopeItemOper(window: P<Grafport>, operation: i32, a: u32, b: u32);
        fn SI_dopeSetupDialogue();
        fn SI_dopeActionsOper(operation: u32);
        fn SI_orthoCamSetInterest(window: P<Grafport>, part: i32);
        fn SI_sprdSetupDialogue(setup: Addr);
        fn SI_sprdQueryDialogue(setup: Addr);
        fn SI_sprdSprdSetupDialogue(setup: Addr, a: Addr, sort: i16);
        fn SI_sprdExec(window: P<Grafport>, dictionary: Addr, how: i16);
        fn si_sprdWriteResultContent(window: P<Grafport>);
        fn SI_sprdSprdSelectElems(window: P<Grafport>, right_button: i16);
        fn SI_sprdSprdDeselectElems(window: P<Grafport>, right_button: i16);
        fn SI_sprdLoadPreDefSetup(window: P<Grafport>, item: i32, how: i16);
        fn SI_doSaveCamPos(window: P<Grafport>);
        fn SI_dovCrop(window: P<Grafport>, left: i32, right: i32, bottom: i32, top: i32, device: i32);
        fn SI_dovSetCamOrtho(window: P<Grafport>, on: i32, device: i32);
        fn SI_dovViewReset(window: P<Grafport>);
        fn SI_displayModeSetup(window: P<Grafport>, event: P<Event>) -> i16;
        fn SI_displayModeHandle(window: P<Grafport>, event: P<Event>) -> i16;
        fn SI_dialogSchematicLayout(window: P<Grafport>, dictionary: Addr);
        fn si_dialogLayout(window: P<Grafport>);
        fn SI_memoCamHandle(window: P<Grafport>, device: i16, x: i16) -> i16;
        fn si_fcvTxtLstHandler(window: P<Grafport>, x: i16, y: i16);
        fn SI_ctrlButHandler(window: P<Grafport>, x: i16, y: i16);
        fn SI_eraseTurnTable(window: P<Grafport>);
        fn SI_camIdConvert(camera_type: i32) -> i32;
        fn SI_prepareViewport(window: P<Grafport>);
        fn SI_dopeFlagsSet(a: i16, b: i16, c: i16, d: i16);
        fn SI_dopeSheetFree();
        fn SI_dovPictSetPos(window: P<Grafport>) -> i32;
        fn SI_dovSetViewport(window: P<Grafport>);
        fn SI_dovPictureOffset(window: P<Grafport>);
        fn SI_drawOver2ShadeDo(window: P<Grafport>) -> i32;
        fn SI_dispModeConvert(window: P<Grafport>, mode: i32) -> i32;
        fn SI_dispModeToggle(window: P<Grafport>, entry: i32) -> i32;
        fn SI_dispModeWindOpen(window: P<Grafport>, how: i32);
        fn SI_displayModeClose(window: P<Grafport>);
        fn SI_txtDisableDirtyReset(on: i16);
        fn SI_resetGL(dictionary: Addr, properties: Addr);
        fn SI_rgbRefreshOne(window: P<Grafport>, how: i16);
        /// What the layout file's reader asks of the cameras, grids, rulers and the rest.
        fn SI_resetCamera(camera: Addr, how: i32);
        fn SI_cameraTransfer(from: Addr, to: Addr);
        fn SI_gridTransfer(from: Addr, to: Addr);
        fn SI_memoCamInit(entry: Addr);
        fn SI_ctrlCamInit(buttons: Addr);
        fn SI_GpropsInit(properties: Addr);
        fn SI_initRotoscopy(window: P<Grafport>);
        fn SI_ghostSetDefault(window: P<Grafport>);
        /// The call through a shader window's callback pointer (`drawWindow.c`): the event,
        /// the IRIS GL window, the window's data word.
        fn shaderWindowCallback(event: P<Event>, window: i32, data: Addr);
        /// `dopeSheet.c`: a click in a dope sheet; non-zero when it changed something.
        fn SI_dopeSheetHandle(event: P<Event>) -> i16;
        /// `gr_global.c`: hand a colour map (501 entries of three 16-bit words) to the
        /// graphic properties.
        fn SI_gpropsSetPaletteEntries(map: Addr, how: i16);
        /// `grTriangle.c`.
        fn SI_cleanUpTriangle(how: i16);
        /// `autozoom.c`: fit a view to the objects of a list.
        fn SI_autozoom(list: Addr, window: P<Grafport>, a: i16, b: i16, c: i16);
        /// `memoCam.c`: draw the stored-camera boxes of a title bar from x, at height y,
        /// leaving `room` pixels for what follows.
        fn SI_memoCamDraw(window: P<Grafport>, x: i32, y: i32, room: i32);
        /// `dov.c`: the rotoscope picture's offset back to none.
        fn SI_dovOffsetReset(window: P<Grafport>);
        /// `dov.c`: the field of view at which the rotoscope picture fills the view, stored
        /// through the pointer (the caller passes the camera's field of view, +0x40).
        fn SI_dovGetDefaultFovCam(window: P<Grafport>, fov: Addr);
        /// `dov.c`: scale the rotoscope picture (`SI_PictureRGBA`) into the window's buffer.
        fn SI_dovPictScale(buffer: Addr, picture: Addr, scale: f32);
        manual:
        /// `SI_camGetDefaultFromId(type, &camera)`: the default camera of a view type, into
        /// a local record of which the caller uses the two words at +0x34 and +0x38 (the
        /// clipping planes).
        fn SI_camGetDefaultFromId(log, m; camera_type: i32, planes: &mut [u32; 2]) -> i16 {
            let ans = log.borrow_mut().call(m, "SI_camGetDefaultFromId", &[Val::W(camera_type as u32), Val::Out]);
            *planes = [ans.val(1) as u32, ans.val(2) as u32];
            ans.val(0) as i16
        }
    }
}

outside! {
    /// The SGI image library that is linked into the program (`image.c` and the files
    /// around it): a colour-map file is one of its images.
    pub trait ImageLib, recorded by RecImageLib {
        /// The image's buffer is full: write it out and store this value first.
        fn iflsbuf(image: Addr, value: i32) -> i32;
        fn iclose(image: Addr) -> i32;
        manual:
        /// `iopen(path, mode)` for reading, `iopen(path, "w", type, dimensions, xsize,
        /// ysize)` for writing: the image record, null when the file cannot be opened.
        fn iopen(log, m; path: Addr, mode: &[u8], shape: Option<[u32; 4]>) -> Addr {
            let mut v = vec![Val::S(m.cstr(path)), Val::S(mode.to_vec())];
            if let Some(shape) = shape {
                v.extend(shape.iter().map(|w| Val::W(*w)));
            }
            log.borrow_mut().call(m, "iopen", &v).val(0) as u32
        }
        /// `getrow(image, buffer, y, z)`: a row of 16-bit values into a buffer of the
        /// caller's (here always four: the images are 4 wide).
        fn getrow(log, m; image: Addr, row: Addr, y: i32, z: i32) -> i32 {
            let ans = log.borrow_mut().call(m, "getrow", &[Val::W(image), Val::Out, Val::W(y as u32), Val::W(z as u32)]);
            for i in 0..4u32 {
                m.set_u16(row + 2 * i, ans.val(1 + i as usize) as u16);
            }
            ans.val(0) as i32
        }
    }
}

/// Everything outside the two files.
pub struct Outside {
    pub gl: Box<dyn Gl>,
    pub c: Box<dyn Libc>,
    pub mem: Box<dyn Memory>,
    pub controls: Box<dyn Controls>,
    pub dialogs: Box<dyn Dialogs>,
    pub menus: Box<dyn Menus>,
    pub status: Box<dyn Status>,
    pub boxes: Box<dyn Boxes>,
    pub server: Box<dyn Server>,
    pub keys: Box<dyn Keys>,
    pub devices: Box<dyn Devices>,
    pub journal: Box<dyn Journal>,
    pub modules: Box<dyn Modules>,
    pub motion: Box<dyn Motion>,
    pub views: Box<dyn Views>,
    pub images: Box<dyn ImageLib>,
}

impl Outside {
    /// The recording stand-ins: every call is appended to the returned log and answered from
    /// the values configured in it.
    pub fn recording() -> (Outside, SharedLog) {
        let log = new_log();
        let o = Outside {
            gl: Box::new(RecGl { log: log.clone() }),
            c: Box::new(RecLibc { log: log.clone() }),
            mem: Box::new(RecMemory { log: log.clone() }),
            controls: Box::new(RecControls { log: log.clone() }),
            dialogs: Box::new(RecDialogs { log: log.clone() }),
            menus: Box::new(RecMenus { log: log.clone() }),
            status: Box::new(RecStatus { log: log.clone() }),
            boxes: Box::new(RecBoxes { log: log.clone() }),
            server: Box::new(RecServer { log: log.clone() }),
            keys: Box::new(RecKeys { log: log.clone() }),
            devices: Box::new(RecDevices { log: log.clone() }),
            journal: Box::new(RecJournal { log: log.clone() }),
            modules: Box::new(RecModules { log: log.clone() }),
            motion: Box::new(RecMotion { log: log.clone() }),
            views: Box::new(RecViews { log: log.clone() }),
            images: Box::new(RecImageLib { log: log.clone() }),
        };
        (o, log)
    }
}
