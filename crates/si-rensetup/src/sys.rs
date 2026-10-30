// SPDX-License-Identifier: MIT
//! The operating-system services of the render utilities: `renu_mutex.c`, `renu_pipeio.c`,
//! `renu_process.c`, `renu_threads.c`, `renu_dynalink.c`, `renu_memory.c` and
//! `renu_filepath.c`. The mental ray front end uses them to start the tracer as a child
//! process, talk to it through pipes, run the translator in a thread, and load shader
//! libraries.
//!
//! Each original routine is a thin layer over a few calls of the C library. The ports keep
//! that layer (which call, with which arguments, what is tested, what is answered) and make
//! the calls on the trait [`Sys`], whose methods are named after the C library's. Two
//! implementations: [`StdSys`], on Rust's standard library, and a replaying one in the
//! check binary. Finding 0023, section 13, says what each routine did on Linux.

use std::collections::HashMap;

/// `struct sembuf`: semaphore number, operation, flags.
pub type SemBuf = (u16, i16, i16);
/// `semLock` (0x086d34a0, 0x086d35ac): take one, undone at exit (`SEM_UNDO`).
pub const SEM_LOCK: SemBuf = (0, -1, 0x1000);
/// `semUnlock` (0x086d34a6): give one back, `SEM_UNDO | IPC_NOWAIT`.
pub const SEM_UNLOCK: SemBuf = (0, 1, 0x1800);

/// What a signal is set to: the routine's own empty handler, or "ignore".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handler {
    /// `signal_SIGPIPE` (0x0842fd60), which does nothing.
    Empty,
    /// `SIG_IGN`.
    Ignore,
}

/// The C library as these files call it. Results are the C library's: -1 for failure.
pub trait Sys {
    /// `semget(key, count, flags)`.
    fn semget(&mut self, key: i32, count: i32, flags: i32) -> i32;
    /// `semctl(id, number, command, value)`.
    fn semctl(&mut self, id: i32, number: i32, command: i32, value: i32) -> i32;
    /// `semop(id, &op, 1)`.
    fn semop(&mut self, id: i32, op: SemBuf) -> i32;
    /// `*__errno_location()`.
    fn errno(&mut self) -> i32;
    /// `fprintf(stderr, …)` with the text already formatted.
    fn stderr(&mut self, text: &[u8]);
    /// `pipe(fds)`: 0 and the two descriptors, or -1.
    fn pipe(&mut self) -> (i32, [i32; 2]);
    /// `close(fd)`.
    fn close(&mut self, fd: i32) -> i32;
    /// `fcntl(fd, command, value)`.
    fn fcntl(&mut self, fd: i32, command: i32, value: i32) -> i32;
    /// `read(fd, buffer, size)`.
    fn read(&mut self, fd: i32, buffer: &mut [u8]) -> isize;
    /// `write(fd, buffer, size)`.
    fn write(&mut self, fd: i32, buffer: &[u8]) -> isize;
    /// `signal(number, handler)`.
    fn signal(&mut self, number: i32, handler: Handler);
    /// `fdopen(fd, mode)`: a stream, 0 for failure.
    fn fdopen(&mut self, fd: i32, mode: &[u8]) -> u32;
    /// `fclose(stream)`.
    fn fclose(&mut self, stream: u32) -> i32;
    /// `open(path, flags, mode)`.
    fn open(&mut self, path: &[u8], flags: i32, mode: i32) -> i32;
    /// `access(path, mode)`.
    fn access(&mut self, path: &[u8], mode: i32) -> i32;
    /// `getenv(name)`.
    fn getenv(&mut self, name: &[u8]) -> Option<Vec<u8>>;
    /// `fork()` and, in the child, everything `SI_mrprocessCreate` does there: `alarm(0)`;
    /// `stdin` (if above 2) duplicated onto descriptor 0 and `stdout` (if above 2) onto 1
    /// and 2, both then closed; the semaphore `started` removed, which is what wakes the
    /// parent; `execv(argv[0], argv)`; and if that returns, the line `ERROR: Cannot run
    /// executable [<argv[0]>]` written to `stdout` and `exit(-1)`. The answer is `fork`'s
    /// in the parent: the child's process id, or -1.
    fn fork_exec(&mut self, argv: &[Vec<u8>], stdin: i32, stdout: i32, started: i32) -> i32;
    /// `kill(pid, signal)`.
    fn kill(&mut self, pid: i32, signal: i32) -> i32;
    /// `waitpid(pid, &status, options)`: the result alone.
    fn waitpid(&mut self, pid: i32, options: i32) -> i32;
    /// `pthread_create(&thread, NULL, si_mrthreadLinuxKicker, block)` and the wait that
    /// follows it: the new thread stores the process id it has (`getpid()`: on the Linux of
    /// 2002 every thread had one) in the creator's block and then runs `body`, the caller's
    /// function with its argument; the creator calls `sched_yield()` until the id is there.
    /// The answer is `pthread_create`'s result (0 for success) and that id.
    fn pthread_create(&mut self, body: Box<dyn FnOnce() + Send>) -> (i32, i32);
    /// `sched_yield()`.
    fn sched_yield(&mut self);
    /// `getpriority(PRIO_PROCESS, 0)`.
    fn getpriority(&mut self) -> i32;
    /// `setpriority(PRIO_PROCESS, who, priority)`.
    fn setpriority(&mut self, who: i32, priority: i32) -> i32;
    /// `getpgid(pid)`.
    fn getpgid(&mut self, pid: i32) -> i32;
    /// `pthread_exit(NULL)`.
    fn pthread_exit(&mut self);
    /// `dlopen(path, flags)`: a handle, 0 for failure.
    fn dlopen(&mut self, path: &[u8], flags: i32) -> u32;
    /// `dlclose(handle)`.
    fn dlclose(&mut self, handle: u32) -> i32;
    /// `dlsym(handle, name)`.
    fn dlsym(&mut self, handle: u32, name: &[u8]) -> u32;
}

// ------------------------------------------------------------------ renu_mutex.c

/// A mutex is a System V semaphore of value 1 [soft 0x0842fbd0 SI_mutexCreate]:
/// `semget(IPC_PRIVATE, 1, 0x7ff)`, then `semctl(id, 0, SETVAL, 1)`. The handle is the
/// semaphore's id, -1 for failure; the answer says whether it worked.
pub fn mutex_create(sys: &mut dyn Sys, handle: &mut i32) -> bool {
    let id = sys.semget(0, 1, 0x7ff);
    if id < 0 {
        *handle = -1;
        return false;
    }
    sys.semctl(id, 0, 0x10, 1);
    *handle = id;
    true
}

/// [soft 0x0842fc24 SI_mutexDestroy]: `semctl(id, 1, IPC_RMID, 0)`; the handle becomes -1.
pub fn mutex_destroy(sys: &mut dyn Sys, handle: &mut i32) -> bool {
    if *handle < 0 {
        return false;
    }
    let r = sys.semctl(*handle, 1, 0, 0);
    *handle = -1;
    r > -1
}

/// [soft 0x0842fc5c SI_mutexLock]: one unit taken (`semop` with `SEM_UNDO`); nothing for
/// a handle below 0.
pub fn mutex_lock(sys: &mut dyn Sys, handle: i32) {
    if handle > -1 {
        sys.semop(handle, SEM_LOCK);
    }
}

/// [soft 0x0842fc80 SI_mutexUnlock]: one unit given back.
pub fn mutex_unlock(sys: &mut dyn Sys, handle: i32) {
    if handle > -1 {
        sys.semop(handle, SEM_UNLOCK);
    }
}

// ------------------------------------------------------------------ renu_pipeio.c

/// [soft 0x0842fcb0 SI_pipeioCreate]: a pipe; both ends are -1 when `pipe` fails.
pub fn pipeio_create(sys: &mut dyn Sys, read_end: &mut i32, write_end: &mut i32) -> bool {
    *read_end = -1;
    *write_end = -1;
    let (r, fds) = sys.pipe();
    if r == 0 {
        *read_end = fds[0];
        *write_end = fds[1];
    }
    r == 0
}

/// [soft 0x0842fcfc SI_pipeioDestroy]: closes a descriptor above 2 and makes it -1; the
/// three standard descriptors and -1 are refused.
pub fn pipeio_destroy(sys: &mut dyn Sys, fd: &mut i32) -> bool {
    if *fd < 3 {
        return false;
    }
    sys.close(*fd);
    *fd = -1;
    true
}

/// [soft 0x0842fd30 SI_pipeioResetInheritance]: `fcntl(fd, F_SETFD, FD_CLOEXEC)`: the
/// descriptor is not passed on to a child that is executed.
pub fn pipeio_reset_inheritance(sys: &mut dyn Sys, fd: i32) -> bool {
    fd > -1 && sys.fcntl(fd, 2, 1) > -1
}

/// Which way [`pipeio_read_write`] goes (`_pipeOpr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeOp {
    Read = 1,
    Write = 2,
}

/// One `read` or `write` [soft 0x0842fdb8 pipeioReadWrite]. A write is made with the
/// broken-pipe signal (13) pointed at an empty handler [soft 0x0842fd60 signal_SIGPIPE],
/// and **set to "ignore" afterwards**, whatever it was before. The answer is 0 with the
/// byte count, or -1 with the count left alone.
pub fn pipeio_read_write(sys: &mut dyn Sys, fd: i32, buffer: &mut [u8], count: &mut i32, op: i32) -> i32 {
    let n = match op {
        1 => sys.read(fd, buffer),
        2 => {
            sys.signal(13, Handler::Empty);
            let n = sys.write(fd, buffer);
            sys.signal(13, Handler::Ignore);
            n
        }
        _ => -1,
    };
    if n < 0 {
        return -1;
    }
    *count = n as i32;
    0
}

/// [soft 0x0842fd68 SI_pipeioWrite]
pub fn pipeio_write(sys: &mut dyn Sys, fd: i32, buffer: &[u8], count: &mut i32) -> i32 {
    let mut b = buffer.to_vec();
    pipeio_read_write(sys, fd, &mut b, count, PipeOp::Write as i32)
}

/// [soft 0x0842fd90 SI_pipeioRead]
pub fn pipeio_read(sys: &mut dyn Sys, fd: i32, buffer: &mut [u8], count: &mut i32) -> i32 {
    pipeio_read_write(sys, fd, buffer, count, PipeOp::Read as i32)
}

/// [soft 0x0842fe44 SI_pipeioOpenFILE]: `fdopen`; null for a descriptor below 0.
pub fn pipeio_open_file(sys: &mut dyn Sys, fd: i32, mode: &[u8]) -> u32 {
    if fd < 0 { 0 } else { sys.fdopen(fd, mode) }
}

/// [soft 0x0842fe68 SI_pipeioCloseFILE]: `fclose`; always answers 1.
pub fn pipeio_close_file(sys: &mut dyn Sys, stream: u32) -> bool {
    sys.fclose(stream);
    true
}

/// Opens a file as a descriptor [soft 0x0842fe84 SI_pipeioFileOpen] with mode 0666. The
/// caller's flag bits become `open`'s: 0x80 append (0x400), 0x40 exclusive (0x80), 0x20
/// truncate (0x200), 0x10 create (0x40), 8 read and write (2), 4 write only (1). The bit
/// 0x80 is tested as the sign of the low byte.
pub fn pipeio_file_open(sys: &mut dyn Sys, fd: &mut i32, path: &[u8], flags: i32) -> bool {
    let mut o = if (flags as i8) < 0 { 0x400 } else { 0 };
    for (bit, flag) in [(0x40, 0x80), (0x20, 0x200), (0x10, 0x40), (8, 2), (4, 1)] {
        if flags & bit != 0 {
            o |= flag;
        }
    }
    *fd = -1;
    let r = sys.open(path, o, 0o666);
    if r > -1 {
        *fd = r;
    }
    r > -1
}

// ------------------------------------------------------------------ renu_process.c

/// An "event" is a System V semaphore of value 0 [soft 0x0842fef0 eventCreate]. On failure
/// two lines go to standard error: the `errno`, and for `ENOSPC` (28) the advice to reboot.
fn event_create(sys: &mut dyn Sys, id: &mut i32) -> bool {
    *id = sys.semget(0, 1, 0x7ff);
    if *id < 0 {
        let e = sys.errno();
        sys.stderr(format!("eventCreate(): semget() FAILED errno({e})\n").as_bytes());
        if sys.errno() == 0x1c {
            sys.stderr(b"Could not obtain semaphore key.\nThis may be due to exhausted kernel resources.\nTry rebooting the machine.\n");
        }
        return false;
    }
    sys.semctl(*id, 0, 0x10, 0);
    true
}

/// [soft 0x0842ff68 eventDestroy]: the semaphore is removed; the id becomes -1.
fn event_destroy(sys: &mut dyn Sys, id: &mut i32) -> bool {
    let r = sys.semctl(*id, 1, 0, 0);
    *id = -1;
    r > -1
}

/// [soft 0x0842ff9c eventWait]: tries to take one unit of a semaphore that has none: the
/// caller sleeps until the semaphore is removed.
fn event_wait(sys: &mut dyn Sys, id: i32) {
    sys.semop(id, SEM_LOCK);
}

/// Starts a program as a child process [soft 0x0842ffbc SI_mrprocessCreate]: an event is
/// made; the process forks; the child rearranges its standard descriptors, removes the
/// event and executes `argv[0]` ([`Sys::fork_exec`]); the parent waits for the event to go
/// away, removes it again (which fails, harmlessly) and answers the child's process id.
/// -1 when the event or the fork fails. The second argument of the original is not used.
pub fn mrprocess_create(sys: &mut dyn Sys, argv: &[Vec<u8>], stdin: i32, stdout: i32) -> i32 {
    let mut event = 0;
    if !event_create(sys, &mut event) {
        return -1;
    }
    let pid = sys.fork_exec(argv, stdin, stdout, event);
    if pid < 0 {
        event_destroy(sys, &mut event);
        return -1;
    }
    if pid > 0 {
        event_wait(sys, event);
        event_destroy(sys, &mut event);
    }
    pid
}

/// [soft 0x08430138 SI_mrprocessDestroy]: `kill(pid, SIGTERM)`.
pub fn mrprocess_destroy(sys: &mut dyn Sys, pid: i32) {
    sys.kill(pid, 0xf);
}

/// [soft 0x08430154 SI_mrprocessIsAlive]: `waitpid(pid, &status, WNOHANG)` answers 0; a
/// process id below 1 is not alive.
pub fn mrprocess_is_alive(sys: &mut dyn Sys, pid: i32) -> bool {
    pid >= 1 && sys.waitpid(pid, 1) == 0
}

/// The three event routines, which are file-local in the original, for the check binary.
pub mod check {
    use super::Sys;

    pub fn event_create(sys: &mut dyn Sys, id: &mut i32) -> bool {
        super::event_create(sys, id)
    }

    pub fn event_destroy(sys: &mut dyn Sys, id: &mut i32) -> bool {
        super::event_destroy(sys, id)
    }

    pub fn event_wait(sys: &mut dyn Sys, id: i32) {
        super::event_wait(sys, id)
    }
}

// ------------------------------------------------------------------ renu_threads.c

/// Starts a thread [soft 0x084301b8 SI_mrthreadCreate]. The thread's first act
/// [soft 0x08430190 si_mrthreadLinuxKicker] is to store its own process id in the
/// creator's block, then it calls the function with the argument; the creator yields the
/// processor until the id is there. The relative priority (cut to -1 … 1) times ten is
/// added to the creator's own nice value, the sum held to -20 … 20 and given to the thread.
/// The answer is the thread's process id, 0 when it could not be made.
pub fn mrthread_create(sys: &mut dyn Sys, body: Box<dyn FnOnce() + Send>, priority: i32) -> i32 {
    let priority = priority.clamp(-1, 1);
    let (r, id) = sys.pthread_create(body);
    if r != 0 {
        return 0;
    }
    let nice = (sys.getpriority() + priority * 10).clamp(-0x14, 0x14);
    sys.setpriority(id, nice);
    id
}

/// [soft 0x08430274 SI_mrthreadDestroy]: nothing is destroyed; the answer is whether the
/// id is not negative.
pub fn mrthread_destroy(id: i32) -> bool {
    id > -1
}

/// [soft 0x0843028c SI_mrthreadIsAlive]: the thread's process group is the caller's.
pub fn mrthread_is_alive(sys: &mut dyn Sys, id: i32) -> bool {
    if id < 1 {
        return false;
    }
    let mine = sys.getpgid(0);
    mine == sys.getpgid(id)
}

/// [soft 0x084302cc SI_mrthreadExit]: `pthread_exit(NULL)`; the argument is not used.
pub fn mrthread_exit(sys: &mut dyn Sys) {
    sys.pthread_exit();
}

// ------------------------------------------------------------------ renu_dynalink.c

/// [soft 0x08599560 SI_dynlibLoad]: `dlopen(path, RTLD_GLOBAL | RTLD_LAZY)`; the handle is
/// -1 when it fails.
pub fn dynlib_load(sys: &mut dyn Sys, handle: &mut i32, path: &[u8]) -> bool {
    *handle = -1;
    let h = sys.dlopen(path, 0x101);
    if h != 0 {
        *handle = h as i32;
    }
    h != 0
}

/// [soft 0x0859959c SI_dynlibFree]: `dlclose`; the handle becomes -1.
pub fn dynlib_free(sys: &mut dyn Sys, handle: &mut i32) -> bool {
    let r = sys.dlclose(*handle as u32);
    *handle = -1;
    r == 0
}

/// [soft 0x085995c8 SI_dynlibSym]: `dlsym`.
pub fn dynlib_sym(sys: &mut dyn Sys, handle: i32, name: &[u8]) -> u32 {
    sys.dlsym(handle as u32, name)
}

// ------------------------------------------------------------------ renu_memory.c

/// One block a tracker knows: `{pointer, size, next}` (12 bytes in the original).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackNode {
    /// The block's bytes; `None` is a null pointer (a node on the spare list).
    pub data: Option<Vec<u8>>,
    pub size: u32,
}

/// `SI_memtrackParms` (12 bytes): a mutex, the list of live blocks (newest first) and a
/// list of spare nodes. The original's pointers are positions here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemTrack {
    /// +0: the mutex's semaphore id, -1 for none.
    pub mutex: i32,
    /// +4: the nodes of the blocks handed out, newest first.
    pub live: Vec<TrackNode>,
    /// +8: nodes whose block was freed, to be used again, last freed first.
    pub spare: Vec<TrackNode>,
}

/// [soft 0x085995f0 SI_memtrackInitialize]: no mutex, both lists empty.
pub fn memtrack_initialize() -> MemTrack {
    MemTrack { mutex: -1, live: Vec::new(), spare: Vec::new() }
}

/// Hands out a block and remembers it [soft 0x08599610 SI_memtrackAllocate]: under the
/// mutex, a node is taken from the spare list or allocated, put at the head of the live
/// list, and given a new block of `size` bytes. Null for size 0. The block's content is
/// what `malloc` gives; zero here.
pub fn memtrack_allocate(sys: &mut dyn Sys, t: &mut MemTrack, size: u32) -> Option<usize> {
    if size == 0 {
        return None;
    }
    mutex_lock(sys, t.mutex);
    let mut node = if t.spare.is_empty() { TrackNode { data: None, size: 0 } } else { t.spare.remove(0) };
    node.data = Some(vec![0; size as usize]);
    node.size = size;
    t.live.insert(0, node);
    mutex_unlock(sys, t.mutex);
    Some(0)
}

/// [soft 0x085996a4 SI_memtrackCalloc]: the same, cleared.
pub fn memtrack_calloc(sys: &mut dyn Sys, t: &mut MemTrack, count: u32, size: u32) -> Option<usize> {
    let at = memtrack_allocate(sys, t, count.wrapping_mul(size))?;
    t.live[at].data.as_mut().unwrap().fill(0);
    Some(at)
}

/// Gives a block back [soft 0x085996e0 SI_memtrackFree]: the node that holds it leaves the
/// live list for the head of the spare list. `block` is the node's position in the live
/// list (the original searches the list for the pointer); a block the tracker does not
/// know is left alone.
pub fn memtrack_free(sys: &mut dyn Sys, t: &mut MemTrack, block: Option<usize>) {
    let Some(at) = block else { return };
    mutex_lock(sys, t.mutex);
    if at < t.live.len() {
        let mut node = t.live.remove(at);
        node.data = None;
        t.spare.insert(0, node);
    }
    mutex_unlock(sys, t.mutex);
}

/// [soft 0x08599770 SI_memtrackRealloc]: the block of a known node is reallocated; **the
/// node's size word is not updated**. Null for a null block or size 0, and for a block the
/// tracker does not know.
pub fn memtrack_realloc(sys: &mut dyn Sys, t: &mut MemTrack, block: Option<usize>, size: u32) -> Option<usize> {
    let at = block?;
    if size == 0 {
        return None;
    }
    mutex_lock(sys, t.mutex);
    let found = at < t.live.len();
    if found {
        t.live[at].data.as_mut().unwrap().resize(size as usize, 0);
    }
    mutex_unlock(sys, t.mutex);
    found.then_some(at)
}

/// [soft 0x085997e0 SI_memtrackStrdup]: a tracked copy of a string with its terminator.
pub fn memtrack_strdup(sys: &mut dyn Sys, t: &mut MemTrack, s: Option<&[u8]>) -> Option<usize> {
    let s = s?;
    let at = memtrack_allocate(sys, t, s.len() as u32 + 1)?;
    t.live[at].data.as_mut().unwrap()[..s.len()].copy_from_slice(s);
    Some(at)
}

/// [soft 0x08599868 SI_memtrackReset]: every block and every node of both lists is freed.
pub fn memtrack_reset(sys: &mut dyn Sys, t: &mut MemTrack) {
    mutex_lock(sys, t.mutex);
    t.live.clear();
    t.spare.clear();
    mutex_unlock(sys, t.mutex);
}

/// Reports the blocks still handed out [soft 0x085998fc SI_memtrackCheck]: one line on
/// standard error per live node with a block (its number counted from 1 along the list,
/// its size in bytes and in kilobytes, its address in decimal and in hexadecimal); the
/// answer is whether there was one. `address` gives a node's block address for the line.
pub fn memtrack_check(sys: &mut dyn Sys, t: &MemTrack, address: &dyn Fn(usize) -> u32) -> bool {
    let mut any = false;
    for (i, n) in t.live.iter().enumerate() {
        if n.data.is_some() {
            let size = n.size as i32;
            let kb = (if size < 0 { size + 0x3ff } else { size }) >> 10;
            let a = address(i);
            sys.stderr(format!("Node: {:05} size: {:>8} [{:>8} Ko] data: {} [0x{:08X}]\n", i + 1, size, kb, a as i32, a).as_bytes());
            any = true;
        }
    }
    any
}

// ------------------------------------------------------------------ renu_filepath.c

/// [soft 0x0842fa88 SI_mrfileIsValidExec]: `access(path, X_OK)` answers 0.
pub fn mrfile_is_valid_exec(sys: &mut dyn Sys, path: &[u8]) -> bool {
    sys.access(path, 1) == 0
}

/// The full path of a program [soft 0x0842f930 SI_mrfileGetExecFullpath]. A name that
/// starts with a slash is taken as it is. Any other is looked for in the directories of
/// `PATH`, or of `/usr/bin:/usr/local/bin:/usr/local/mi/bin:` when `PATH` is not set (split
/// at colons by `strtok_r`, so empty entries are passed over). `None` when no candidate is
/// executable.
///
/// The original means to look in the current directory the program remembers (`SI_cwd`)
/// first: it puts it on the list, and then `SI_strStripString` (0x083cf57c) stores its own
/// list of the `PATH` entries over it. That directory is never searched (and its copy is
/// never freed).
pub fn mrfile_get_exec_fullpath(sys: &mut dyn Sys, name: &[u8]) -> Option<Vec<u8>> {
    if name.first() == Some(&b'/') {
        return mrfile_is_valid_exec(sys, name).then(|| name.to_vec());
    }
    let path = sys.getenv(b"PATH").unwrap_or_else(|| b"/usr/bin:/usr/local/bin:/usr/local/mi/bin:".to_vec());
    for d in path.split(|c| *c == b':').filter(|d| !d.is_empty()) {
        let mut candidate = d.to_vec();
        candidate.push(b'/');
        candidate.extend_from_slice(name);
        if mrfile_is_valid_exec(sys, &candidate) {
            return Some(candidate);
        }
    }
    None
}

// ------------------------------------------------------------------ on the standard library

/// [`Sys`] on Rust's standard library, as far as that goes: pipes, files, child processes
/// and threads are real; a semaphore is a counter (the ports only use one as a lock inside
/// one process, or as the "child has started" signal, which a child made by the standard
/// library needs no help with); shared libraries cannot be loaded and say so.
#[derive(Default)]
pub struct StdSys {
    next: i32,
    readers: HashMap<i32, std::io::PipeReader>,
    writers: HashMap<i32, std::io::PipeWriter>,
    files: HashMap<i32, std::fs::File>,
    children: HashMap<i32, std::process::Child>,
    sems: HashMap<i32, i32>,
    errno: i32,
    /// What went to standard error.
    pub errors: Vec<u8>,
}

impl StdSys {
    pub fn new() -> StdSys {
        StdSys { next: 3, ..StdSys::default() }
    }

    fn fd(&mut self) -> i32 {
        self.next += 1;
        self.next - 1
    }
}

impl Sys for StdSys {
    fn semget(&mut self, _key: i32, _count: i32, _flags: i32) -> i32 {
        let id = self.fd();
        self.sems.insert(id, 0);
        id
    }

    fn semctl(&mut self, id: i32, _number: i32, command: i32, value: i32) -> i32 {
        match command {
            0x10 => self.sems.get_mut(&id).map_or(-1, |v| {
                *v = value;
                0
            }),
            0 => self.sems.remove(&id).map_or(-1, |_| 0),
            _ => -1,
        }
    }

    fn semop(&mut self, id: i32, op: SemBuf) -> i32 {
        // one thread of control: a lock that is free is taken, a wait for a removed
        // semaphore returns at once
        match self.sems.get_mut(&id) {
            Some(v) => {
                *v += op.1 as i32;
                0
            }
            None => -1,
        }
    }

    fn errno(&mut self) -> i32 {
        self.errno
    }

    fn stderr(&mut self, text: &[u8]) {
        self.errors.extend_from_slice(text);
    }

    fn pipe(&mut self) -> (i32, [i32; 2]) {
        match std::io::pipe() {
            Ok((r, w)) => {
                let (a, b) = (self.fd(), self.fd());
                self.readers.insert(a, r);
                self.writers.insert(b, w);
                (0, [a, b])
            }
            Err(_) => (-1, [0, 0]),
        }
    }

    fn close(&mut self, fd: i32) -> i32 {
        if self.readers.remove(&fd).is_some() || self.writers.remove(&fd).is_some() || self.files.remove(&fd).is_some() { 0 } else { -1 }
    }

    fn fcntl(&mut self, fd: i32, _command: i32, _value: i32) -> i32 {
        // descriptors made by the standard library are not inherited to begin with
        if self.readers.contains_key(&fd) || self.writers.contains_key(&fd) || self.files.contains_key(&fd) { 0 } else { -1 }
    }

    fn read(&mut self, fd: i32, buffer: &mut [u8]) -> isize {
        use std::io::Read;
        let r = match (self.readers.get_mut(&fd), self.files.get_mut(&fd)) {
            (Some(p), _) => p.read(buffer),
            (_, Some(f)) => f.read(buffer),
            _ => return -1,
        };
        r.map_or(-1, |n| n as isize)
    }

    fn write(&mut self, fd: i32, buffer: &[u8]) -> isize {
        use std::io::Write;
        let r = match (self.writers.get_mut(&fd), self.files.get_mut(&fd)) {
            (Some(p), _) => p.write(buffer),
            (_, Some(f)) => f.write(buffer),
            _ => return -1,
        };
        r.map_or(-1, |n| n as isize)
    }

    fn signal(&mut self, _number: i32, _handler: Handler) {
        // a Rust program has the broken-pipe signal ignored from the start
    }

    fn fdopen(&mut self, fd: i32, _mode: &[u8]) -> u32 {
        fd as u32
    }

    fn fclose(&mut self, stream: u32) -> i32 {
        self.close(stream as i32)
    }

    fn open(&mut self, path: &[u8], flags: i32, _mode: i32) -> i32 {
        use std::os::unix::ffi::OsStrExt;
        let mut o = std::fs::OpenOptions::new();
        o.read(flags & 3 != 1).write(flags & 3 != 0).append(flags & 0x400 != 0).truncate(flags & 0x200 != 0);
        if flags & 0x80 != 0 {
            o.create_new(true);
        } else {
            o.create(flags & 0x40 != 0);
        }
        match o.open(std::ffi::OsStr::from_bytes(path)) {
            Ok(f) => {
                let fd = self.fd();
                self.files.insert(fd, f);
                fd
            }
            Err(_) => -1,
        }
    }

    fn access(&mut self, path: &[u8], mode: i32) -> i32 {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(std::ffi::OsStr::from_bytes(path)) {
            Ok(m) if mode & 1 == 0 || (m.is_file() && m.permissions().mode() & 0o111 != 0) => 0,
            _ => -1,
        }
    }

    fn getenv(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        std::env::var_os(std::ffi::OsStr::from_bytes(name)).map(|v| v.into_vec())
    }

    fn fork_exec(&mut self, argv: &[Vec<u8>], stdin: i32, stdout: i32, started: i32) -> i32 {
        use std::os::unix::ffi::OsStrExt;
        use std::process::{Command, Stdio};
        let Some(program) = argv.first() else { return -1 };
        let mut c = Command::new(std::ffi::OsStr::from_bytes(program));
        c.args(argv[1..].iter().map(|a| std::ffi::OsStr::from_bytes(a)));
        if stdin > 2 {
            match self.readers.remove(&stdin) {
                Some(r) => c.stdin(Stdio::from(r)),
                None => return -1,
            };
        }
        if stdout > 2 {
            match self.writers.remove(&stdout) {
                Some(w) => match w.try_clone() {
                    Ok(w2) => c.stdout(Stdio::from(w)).stderr(Stdio::from(w2)),
                    Err(_) => return -1,
                },
                None => return -1,
            };
        }
        // the child has "removed the event" as soon as it exists
        self.sems.remove(&started);
        match c.spawn() {
            Ok(child) => {
                let pid = child.id() as i32;
                self.children.insert(pid, child);
                pid
            }
            // the original's child writes "ERROR: Cannot run executable" to the pipe and the
            // parent still gets a process id; here the failure is known at once
            Err(_) => -1,
        }
    }

    fn kill(&mut self, pid: i32, _signal: i32) -> i32 {
        // the standard library can only end a child outright, where the original asks it to stop
        self.children.get_mut(&pid).map_or(-1, |c| c.kill().map_or(-1, |_| 0))
    }

    fn waitpid(&mut self, pid: i32, _options: i32) -> i32 {
        match self.children.get_mut(&pid).map(|c| c.try_wait()) {
            Some(Ok(None)) => 0,
            Some(Ok(Some(_))) => pid,
            _ => -1,
        }
    }

    fn pthread_create(&mut self, body: Box<dyn FnOnce() + Send>) -> (i32, i32) {
        // threads of one process share its id nowadays; each gets a number of its own here
        match std::thread::Builder::new().spawn(body) {
            Ok(_) => (0, self.fd() + 100_000),
            Err(_) => (-1, -1),
        }
    }

    fn sched_yield(&mut self) {
        std::thread::yield_now();
    }

    fn getpriority(&mut self) -> i32 {
        0
    }

    fn setpriority(&mut self, _who: i32, _priority: i32) -> i32 {
        0
    }

    fn getpgid(&mut self, _pid: i32) -> i32 {
        0
    }

    fn pthread_exit(&mut self) {}

    fn dlopen(&mut self, _path: &[u8], _flags: i32) -> u32 {
        0
    }

    fn dlclose(&mut self, _handle: u32) -> i32 {
        -1
    }

    fn dlsym(&mut self, _handle: u32, _name: &[u8]) -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_talks_through_a_pipe() {
        let mut sys = StdSys::new();
        let (mut r, mut w) = (0, 0);
        assert!(pipeio_create(&mut sys, &mut r, &mut w));
        let Some(echo) = mrfile_get_exec_fullpath(&mut sys, b"echo") else { return };
        let pid = mrprocess_create(&mut sys, &[echo, b"render".to_vec()], 0, w);
        assert!(pid > 0);
        let mut buf = [0u8; 16];
        let mut n = 0;
        assert_eq!(pipeio_read(&mut sys, r, &mut buf, &mut n), 0);
        assert_eq!(&buf[..n as usize], b"render\n");
        assert!(pipeio_destroy(&mut sys, &mut r));
        assert_eq!(r, -1);
        assert!(!pipeio_destroy(&mut sys, &mut r));
    }

    #[test]
    fn a_thread_runs_its_body() {
        let mut sys = StdSys::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let id = mrthread_create(&mut sys, Box::new(move || tx.send(42).unwrap()), 5);
        assert!(id > 0 && mrthread_destroy(id));
        assert_eq!(rx.recv().unwrap(), 42);
        assert!(mrthread_is_alive(&mut sys, id));
        assert!(!mrthread_is_alive(&mut sys, 0));
    }

    #[test]
    fn a_tracker_reuses_its_nodes() {
        let mut sys = StdSys::new();
        let mut t = memtrack_initialize();
        assert!(mutex_create(&mut sys, &mut t.mutex));
        assert_eq!(memtrack_allocate(&mut sys, &mut t, 0), None);
        memtrack_allocate(&mut sys, &mut t, 8);
        memtrack_strdup(&mut sys, &mut t, Some(b"abc"));
        assert_eq!(t.live[0].data.as_deref(), Some(&b"abc\0"[..]));
        memtrack_free(&mut sys, &mut t, Some(1));
        assert_eq!((t.live.len(), t.spare.len()), (1, 1));
        memtrack_realloc(&mut sys, &mut t, Some(0), 100);
        assert_eq!((t.live[0].size, t.live[0].data.as_ref().unwrap().len()), (4, 100));
        assert!(memtrack_check(&mut sys, &t, &|_| 0x1000));
        assert_eq!(sys.errors, b"Node: 00001 size:        4 [       0 Ko] data: 4096 [0x00001000]\n");
        memtrack_reset(&mut sys, &mut t);
        assert!(!memtrack_check(&mut sys, &t, &|_| 0));
        assert!(mutex_destroy(&mut sys, &mut t.mutex));
        assert_eq!(t.mutex, -1);
    }
}
