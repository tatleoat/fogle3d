// SPDX-License-Identifier: MIT
//! The C library's system services, as the layer calls them: one method per imported symbol,
//! with the arguments the original passes.
//!
//! Memory management (`malloc`, `calloc`, `realloc`, `free`, `memset`, `memmove`, `qsort`,
//! the string functions) is not here: those are the program's own bookkeeping and become
//! ordinary Rust. What is here reaches the operating system: files and devices, the process,
//! the clock, System V semaphores and shared memory, and the standard streams.

use crate::rec::{ret_u, SharedLog, Val};

/// Which standard stream a `fprintf` goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CStream {
    Stdout,
    Stderr,
}

/// One variadic argument of a `printf`-style call, as the original pushes it.
#[derive(Clone, Debug, PartialEq)]
pub enum CArg<'a> {
    /// `%d`, `%i`, `%x`, `%u`, `%c`, `%p`: one 32-bit stack word.
    Int(u32),
    /// `%s`: the string pointed to.
    Str(&'a [u8]),
    /// `%f`, `%g`, `%e`: a double (two stack words).
    Double(f64),
}

/// Thrown (as a panic payload) by the recording [`Libc::exit`], which must not return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit(pub i32);

/// The System V `struct sembuf` of `semop` (i386: three shorts).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemBuf {
    pub sem_num: u16,
    pub sem_op: i16,
    pub sem_flg: i16,
}

pub trait Libc {
    // ---- files and devices
    /// `open(path, flags, mode)`
    fn open(&mut self, path: &[u8], flags: i32, mode: u32) -> i32;
    /// `creat(path, mode)`
    fn creat(&mut self, path: &[u8], mode: u32) -> i32;
    fn close(&mut self, fd: i32) -> i32;
    /// `read(fd, buf, buf.len())`; the bytes read are written to the start of `buf`.
    fn read(&mut self, fd: i32, buf: &mut [u8]) -> i32;
    /// `write(fd, buf, buf.len())`
    fn write(&mut self, fd: i32, buf: &[u8]) -> i32;
    fn lseek(&mut self, fd: i32, offset: i32, whence: i32) -> i32;
    /// `ioctl(fd, request, arg)`. Every request the layer issues takes either a null
    /// pointer (`None`) or a pointer to one `int` that the driver reads and may overwrite.
    fn ioctl(&mut self, fd: i32, request: u32, arg: Option<&mut i32>) -> i32;
    /// `__xstat(3, path, &st)`: the result, and `st_size` (the only field the layer reads;
    /// 0 when the call fails).
    fn xstat(&mut self, path: &[u8]) -> (i32, i32);
    /// `flock(fd, operation)`
    fn flock(&mut self, fd: i32, operation: i32) -> i32;

    // ---- process, environment, time
    fn getpid(&mut self) -> i32;
    /// `getenv(name)`: `None` is the null pointer.
    fn getenv(&mut self, name: &[u8]) -> Option<Vec<u8>>;
    /// `exit(status)`: does not return.
    fn exit(&mut self, status: i32) -> !;
    fn usleep(&mut self, usec: u32) -> i32;
    fn sleep(&mut self, sec: u32) -> u32;
    fn sched_yield(&mut self) -> i32;
    /// `time(NULL)`
    fn time(&mut self) -> i32;
    /// `clock()`
    fn clock(&mut self) -> i32;
    /// `*__errno_location()`
    fn errno(&mut self) -> i32;
    /// `strerror(errnum)`
    fn strerror(&mut self, errnum: i32) -> Vec<u8>;

    // ---- System V IPC
    fn ftok(&mut self, path: &[u8], proj: i32) -> i32;
    fn shmget(&mut self, key: i32, size: u32, flags: i32) -> i32;
    /// `shmat(id, NULL, flags)`: the address of the segment (`0xffffffff` on failure).
    fn shmat(&mut self, id: i32, flags: i32) -> u32;
    fn shmdt(&mut self, addr: u32) -> i32;
    /// `shmctl(id, cmd, &ds)`; the layer reads nothing back from `ds`.
    fn shmctl(&mut self, id: i32, cmd: i32) -> i32;
    fn semget(&mut self, key: i32, nsems: i32, flags: i32) -> i32;
    /// `semctl(id, semnum, cmd, arg)` with an integer fourth argument (0 where the original
    /// passes three arguments).
    fn semctl(&mut self, id: i32, semnum: i32, cmd: i32, arg: i32) -> i32;
    fn semop(&mut self, id: i32, ops: &[SemBuf]) -> i32;

    // ---- standard streams
    /// `fprintf(stream, format, args...)` (also what `vfprintf` comes to): the format string
    /// as it is in the binary and the variadic arguments in order.
    fn fprintf(&mut self, stream: CStream, format: &[u8], args: &[CArg]) -> i32;
    /// `vfprintf(stream, format, ap)`: the format string and the caller's variadic
    /// arguments the `va_list` points at.
    /// (Provided: goes to `fprintf` unless an implementation tells the two apart.)
    fn vfprintf(&mut self, stream: CStream, format: &[u8], args: &[CArg]) -> i32 {
        self.fprintf(stream, format, args)
    }
}

/// The recording [`Libc`]: logs each call and answers from the log's configured values.
pub struct RecLibc {
    pub log: SharedLog,
}

impl RecLibc {
    pub fn new(log: SharedLog) -> Self {
        RecLibc { log }
    }
    fn int(&mut self, name: &'static str, args: Vec<Val>, default: u32) -> i32 {
        let a = self.log.borrow_mut().call(name, args);
        ret_u(&a, default) as i32
    }
}

fn printf_vals(format: &[u8], args: &[CArg]) -> Vec<Val> {
    let mut v = vec![Val::S(format.to_vec())];
    for a in args {
        v.push(match a {
            CArg::Int(i) => Val::U(*i),
            CArg::Str(s) => Val::S(s.to_vec()),
            CArg::Double(d) => Val::D(*d),
        });
    }
    v
}

impl Libc for RecLibc {
    fn open(&mut self, path: &[u8], flags: i32, mode: u32) -> i32 {
        self.int("open", vec![Val::S(path.to_vec()), Val::U(flags as u32), Val::U(mode)], 3)
    }
    fn creat(&mut self, path: &[u8], mode: u32) -> i32 {
        self.int("creat", vec![Val::S(path.to_vec()), Val::U(mode)], 3)
    }
    fn close(&mut self, fd: i32) -> i32 {
        self.int("close", vec![Val::U(fd as u32)], 0)
    }
    fn read(&mut self, fd: i32, buf: &mut [u8]) -> i32 {
        // answer: [return value, B(bytes)]; without an answer the read returns 0 bytes
        let a = self.log.borrow_mut().call("read", vec![Val::U(fd as u32), Val::Out, Val::U(buf.len() as u32)]);
        if let Some(v) = &a {
            if let Some(b) = v.get(1) {
                let b = b.bytes();
                let n = b.len().min(buf.len());
                buf[..n].copy_from_slice(&b[..n]);
            }
        }
        ret_u(&a, 0) as i32
    }
    fn write(&mut self, fd: i32, buf: &[u8]) -> i32 {
        let n = buf.len() as u32;
        self.int("write", vec![Val::U(fd as u32), Val::B(buf.to_vec()), Val::U(n)], n)
    }
    fn lseek(&mut self, fd: i32, offset: i32, whence: i32) -> i32 {
        self.int("lseek", vec![Val::U(fd as u32), Val::U(offset as u32), Val::U(whence as u32)], 0)
    }
    fn ioctl(&mut self, fd: i32, request: u32, arg: Option<&mut i32>) -> i32 {
        // answer: [return value, U(value written back)]
        let args = match &arg {
            Some(v) => vec![Val::U(fd as u32), Val::U(request), Val::W(vec![**v as u32])],
            None => vec![Val::U(fd as u32), Val::U(request), Val::U(0)],
        };
        let a = self.log.borrow_mut().call("ioctl", args);
        if let (Some(p), Some(v)) = (arg, a.as_ref().and_then(|v| v.get(1))) {
            *p = v.i();
        }
        ret_u(&a, 0) as i32
    }
    fn xstat(&mut self, path: &[u8]) -> (i32, i32) {
        let a = self.log.borrow_mut().call("__xstat", vec![Val::U(3), Val::S(path.to_vec()), Val::Out]);
        (ret_u(&a, 0) as i32, crate::rec::out_u(&a, 1, 0) as i32)
    }
    fn flock(&mut self, fd: i32, operation: i32) -> i32 {
        self.int("flock", vec![Val::U(fd as u32), Val::U(operation as u32)], 0)
    }
    fn getpid(&mut self) -> i32 {
        self.int("getpid", vec![], 1000)
    }
    fn getenv(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        // answer: [S(value)] for a set variable; anything else, or no answer: not set
        let a = self.log.borrow_mut().call("getenv", vec![Val::S(name.to_vec())]);
        match a.as_ref().and_then(|v| v.first()) {
            Some(Val::S(s)) => Some(s.clone()),
            _ => None,
        }
    }
    fn exit(&mut self, status: i32) -> ! {
        self.log.borrow_mut().call("exit", vec![Val::U(status as u32)]);
        std::panic::panic_any(Exit(status))
    }
    fn usleep(&mut self, usec: u32) -> i32 {
        self.int("usleep", vec![Val::U(usec)], 0)
    }
    fn sleep(&mut self, sec: u32) -> u32 {
        self.int("sleep", vec![Val::U(sec)], 0) as u32
    }
    fn sched_yield(&mut self) -> i32 {
        self.int("sched_yield", vec![], 0)
    }
    fn time(&mut self) -> i32 {
        self.int("time", vec![Val::U(0)], 0)
    }
    fn clock(&mut self) -> i32 {
        self.int("clock", vec![], 0)
    }
    fn errno(&mut self) -> i32 {
        self.int("__errno_location", vec![], 0)
    }
    fn strerror(&mut self, errnum: i32) -> Vec<u8> {
        let a = self.log.borrow_mut().call("strerror", vec![Val::U(errnum as u32)]);
        match a.as_ref().and_then(|v| v.first()) {
            Some(Val::S(s)) => s.clone(),
            _ => b"error".to_vec(),
        }
    }
    fn ftok(&mut self, path: &[u8], proj: i32) -> i32 {
        self.int("ftok", vec![Val::S(path.to_vec()), Val::U(proj as u32)], 0x1234)
    }
    fn shmget(&mut self, key: i32, size: u32, flags: i32) -> i32 {
        self.int("shmget", vec![Val::U(key as u32), Val::U(size), Val::U(flags as u32)], 1)
    }
    fn shmat(&mut self, id: i32, flags: i32) -> u32 {
        self.int("shmat", vec![Val::U(id as u32), Val::U(0), Val::U(flags as u32)], 0x6000_0000) as u32
    }
    fn shmdt(&mut self, addr: u32) -> i32 {
        self.int("shmdt", vec![Val::P(addr)], 0)
    }
    fn shmctl(&mut self, id: i32, cmd: i32) -> i32 {
        self.int("shmctl", vec![Val::U(id as u32), Val::U(cmd as u32), Val::Out], 0)
    }
    fn semget(&mut self, key: i32, nsems: i32, flags: i32) -> i32 {
        self.int("semget", vec![Val::U(key as u32), Val::U(nsems as u32), Val::U(flags as u32)], 2)
    }
    fn semctl(&mut self, id: i32, semnum: i32, cmd: i32, arg: i32) -> i32 {
        self.int("semctl", vec![Val::U(id as u32), Val::U(semnum as u32), Val::U(cmd as u32), Val::U(arg as u32)], 0)
    }
    fn semop(&mut self, id: i32, ops: &[SemBuf]) -> i32 {
        let mut b = Vec::new();
        for o in ops {
            b.extend_from_slice(&o.sem_num.to_le_bytes());
            b.extend_from_slice(&o.sem_op.to_le_bytes());
            b.extend_from_slice(&o.sem_flg.to_le_bytes());
        }
        self.int("semop", vec![Val::U(id as u32), Val::B(b), Val::U(ops.len() as u32)], 0)
    }
    fn fprintf(&mut self, stream: CStream, format: &[u8], args: &[CArg]) -> i32 {
        let mut v = vec![Val::U(if stream == CStream::Stderr { 2 } else { 1 })];
        v.extend(printf_vals(format, args));
        self.int("fprintf", v, 0)
    }
    fn vfprintf(&mut self, stream: CStream, format: &[u8], args: &[CArg]) -> i32 {
        let mut v = vec![Val::U(if stream == CStream::Stderr { 2 } else { 1 })];
        v.extend(printf_vals(format, args));
        self.int("vfprintf", v, 0)
    }
}
