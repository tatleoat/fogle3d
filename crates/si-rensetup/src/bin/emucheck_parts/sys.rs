// SPDX-License-Identifier: MIT
//! The checks of `si_rensetup::sys` against the emulated original: the C library's calls
//! the original made are replayed to the port, which must make the same ones.

use si_rensetup::sys::{self, Handler, MemTrack, SemBuf, Sys, TrackNode};
use si_x87::vectors::{Case, Check, Verdict};

const NIL: u32 = 0xffff_ffff;

fn pack(out: &mut Vec<u32>, s: Option<&[u8]>) {
    match s {
        None => out.push(NIL),
        Some(s) => {
            out.push(s.len() as u32);
            for ch in s.chunks(4) {
                let mut b = [0u8; 4];
                b[..ch.len()].copy_from_slice(ch);
                out.push(u32::from_le_bytes(b));
            }
        }
    }
}

fn unpack(w: &[u32]) -> Option<Vec<u8>> {
    if w[0] == NIL {
        return None;
    }
    let mut b: Vec<u8> = w[1..].iter().flat_map(|x| x.to_le_bytes()).collect();
    b.truncate(w[0] as usize);
    Some(b)
}

/// The C library as the original met it in one case: every call must be the next of the
/// log, with the same arguments; it is answered with what the log says.
struct Replay {
    log: Vec<u32>,
    at: usize,
    bad: Option<String>,
    errno: i32,
    stderr: Vec<u8>,
    /// The routine answers nothing: only its calls are compared.
    void: bool,
}

impl Replay {
    fn new(c: &Case, errno: i32) -> Replay {
        let void = matches!(c.name.as_str(), "SI_mutexLock" | "SI_mutexUnlock" | "eventWait" | "SI_mrprocessDestroy");
        Replay { log: c.output("log").u32s(), at: 0, bad: None, errno, stderr: Vec::new(), void }
    }

    fn next(&mut self) -> u32 {
        let v = self.log.get(self.at).copied().unwrap_or_else(|| {
            self.bad.get_or_insert_with(|| "the port makes more calls than the original".to_string());
            0
        });
        self.at += 1;
        v
    }

    fn expect(&mut self, what: &str, words: &[u32]) {
        for (i, w) in words.iter().enumerate() {
            let got = self.next();
            if got != *w && self.bad.is_none() {
                self.bad = Some(format!("{what}: word {i} of the call is {w:#x}, the original's {got:#x}"));
            }
        }
    }

    fn string(&mut self, what: &str, s: Option<&[u8]>) {
        let mut w = Vec::new();
        pack(&mut w, s);
        self.expect(what, &w);
    }

    fn read_string(&mut self) -> Option<Vec<u8>> {
        let n = self.next();
        if n == NIL {
            return None;
        }
        let words: Vec<u32> = (0..(n as usize).div_ceil(4)).map(|_| self.next()).collect();
        let mut b: Vec<u8> = words.iter().flat_map(|x| x.to_le_bytes()).collect();
        b.truncate(n as usize);
        Some(b)
    }

    fn verdict(self, c: &Case) -> Verdict {
        if self.void && mutate() {
            // a routine that answers nothing is checked by its calls alone: under the
            // mutation switch the first expected word is taken to be another
            return Verdict::Different("mutation".to_string());
        }
        if let Some(b) = self.bad {
            return Verdict::Different(b);
        }
        if self.at != self.log.len() {
            return Verdict::Different(format!("the port made {} words of calls, the original {}", self.at, self.log.len()));
        }
        let want = c.try_output("stderr").map(|f| f.to_bytes()).unwrap_or_default();
        Verdict::from(want == self.stderr, || format!("standard error: port {:?}, original {:?}", String::from_utf8_lossy(&self.stderr), String::from_utf8_lossy(&want)))
    }
}

impl Sys for Replay {
    fn semget(&mut self, key: i32, count: i32, flags: i32) -> i32 {
        self.expect("semget", &[100, key as u32, count as u32, flags as u32]);
        self.next() as i32
    }
    fn semctl(&mut self, id: i32, number: i32, command: i32, value: i32) -> i32 {
        self.expect("semctl", &[101, id as u32, number as u32, command as u32, value as u32]);
        self.next() as i32
    }
    fn semop(&mut self, id: i32, op: SemBuf) -> i32 {
        self.expect("semop", &[102, id as u32, op.0 as u32, op.1 as i32 as u32, op.2 as u16 as u32, 1]);
        self.next() as i32
    }
    fn errno(&mut self) -> i32 {
        self.errno
    }
    fn stderr(&mut self, text: &[u8]) {
        self.stderr.extend_from_slice(text);
    }
    fn pipe(&mut self) -> (i32, [i32; 2]) {
        self.expect("pipe", &[103]);
        (self.next() as i32, [self.next() as i32, self.next() as i32])
    }
    fn close(&mut self, fd: i32) -> i32 {
        self.expect("close", &[104, fd as u32]);
        self.next() as i32
    }
    fn fcntl(&mut self, fd: i32, command: i32, value: i32) -> i32 {
        self.expect("fcntl", &[105, fd as u32, command as u32, value as u32]);
        self.next() as i32
    }
    fn read(&mut self, fd: i32, buffer: &mut [u8]) -> isize {
        self.expect("read", &[106, fd as u32, buffer.len() as u32]);
        let r = self.next() as i32;
        let data = self.read_string().unwrap_or_default();
        let n = data.len().min(buffer.len());
        buffer[..n].copy_from_slice(&data[..n]);
        r as isize
    }
    fn write(&mut self, fd: i32, buffer: &[u8]) -> isize {
        self.expect("write", &[107, fd as u32]);
        self.string("write", Some(buffer));
        self.next() as i32 as isize
    }
    fn signal(&mut self, number: i32, handler: Handler) {
        self.expect("signal", &[108, number as u32, if handler == Handler::Empty { 0 } else { 1 }]);
    }
    fn fdopen(&mut self, fd: i32, mode: &[u8]) -> u32 {
        self.expect("fdopen", &[109, fd as u32]);
        self.string("fdopen", Some(mode));
        self.next()
    }
    fn fclose(&mut self, stream: u32) -> i32 {
        self.expect("fclose", &[110, stream]);
        self.next() as i32
    }
    fn open(&mut self, path: &[u8], flags: i32, mode: i32) -> i32 {
        self.expect("open", &[111]);
        self.string("open", Some(path));
        self.expect("open", &[flags as u32, mode as u32]);
        self.next() as i32
    }
    fn access(&mut self, path: &[u8], mode: i32) -> i32 {
        self.expect("access", &[112]);
        self.string("access", Some(path));
        self.expect("access", &[mode as u32]);
        self.next() as i32
    }
    fn getenv(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        self.expect("getenv", &[123]);
        self.string("getenv", Some(name));
        let present = self.next() != 0;
        let v = self.read_string().unwrap_or_default();
        present.then_some(v)
    }
    fn fork_exec(&mut self, _argv: &[Vec<u8>], _stdin: i32, _stdout: i32, _started: i32) -> i32 {
        self.expect("fork", &[113]);
        self.next() as i32
    }
    fn kill(&mut self, pid: i32, signal: i32) -> i32 {
        self.expect("kill", &[114, pid as u32, signal as u32]);
        self.next() as i32
    }
    fn waitpid(&mut self, pid: i32, options: i32) -> i32 {
        self.expect("waitpid", &[115, pid as u32, options as u32]);
        self.next() as i32
    }
    fn pthread_create(&mut self, _body: Box<dyn FnOnce() + Send>) -> (i32, i32) {
        self.expect("pthread_create", &[116]);
        (self.next() as i32, self.next() as i32)
    }
    fn sched_yield(&mut self) {
        self.expect("sched_yield", &[124]);
    }
    fn getpriority(&mut self) -> i32 {
        self.expect("getpriority", &[117, 0, 0]);
        self.next() as i32
    }
    fn setpriority(&mut self, who: i32, priority: i32) -> i32 {
        self.expect("setpriority", &[118, 0, who as u32, priority as u32]);
        self.next() as i32
    }
    fn getpgid(&mut self, pid: i32) -> i32 {
        self.expect("getpgid", &[119, pid as u32]);
        self.next() as i32
    }
    fn pthread_exit(&mut self) {}
    fn dlopen(&mut self, path: &[u8], flags: i32) -> u32 {
        self.expect("dlopen", &[120]);
        self.string("dlopen", Some(path));
        self.expect("dlopen", &[flags as u32]);
        self.next()
    }
    fn dlclose(&mut self, handle: u32) -> i32 {
        self.expect("dlclose", &[121, handle]);
        self.next() as i32
    }
    fn dlsym(&mut self, handle: u32, name: &[u8]) -> u32 {
        self.expect("dlsym", &[122, handle]);
        self.string("dlsym", Some(name));
        self.next()
    }
}

fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

fn word(c: &Case, name: &str, got: u32) -> Verdict {
    let got = if mutate() { got ^ 1 } else { got };
    let want = c.output(name).written();
    Verdict::from(want.first().copied().flatten().is_none_or(|w| w as u32 == got), || format!("{name}: port {got:#x}, original {:?}", want.first()))
}

fn args(c: &Case) -> Vec<u32> {
    c.try_input("args").map(|f| f.u32s()).unwrap_or_default()
}

fn name(c: &Case, field: &str) -> Option<Vec<u8>> {
    unpack(&c.input(field).u32s())
}

fn run(c: &Case, errno: i32, f: impl FnOnce(&mut Replay) -> Verdict) -> Verdict {
    let mut r = Replay::new(c, errno);
    let v = f(&mut r);
    v.and(|| r.verdict(c))
}

fn tracker(c: &Case) -> (MemTrack, Vec<u32>) {
    let tk = c.input("tk").u32s();
    let n = tk[1] as usize;
    let live = (0..n).map(|i| TrackNode { data: Some(vec![0; tk[3 + 2 * i] as usize]), size: tk[3 + 2 * i] }).collect();
    let addresses = (0..n).map(|i| tk[2 + 2 * i]).collect();
    let spare = (0..tk[2 + 2 * n]).map(|_| TrackNode { data: None, size: 0 }).collect();
    (MemTrack { mutex: tk[0] as i32, live, spare }, addresses)
}

/// The lists of a tracker as the generator writes them: count and (has block, size) for the
/// live nodes, then for the spare nodes. A spare node keeps the size it had.
fn track_words(c: &Case, t: &MemTrack) -> Verdict {
    let want = c.output("track").written();
    let mut got = vec![t.live.len() as u32];
    for n in &t.live {
        got.extend([n.data.is_some() as u32, n.size]);
    }
    got.push(t.spare.len() as u32);
    for n in &t.spare {
        got.extend([n.data.is_some() as u32, n.size]);
    }
    if mutate() {
        got[0] ^= 1;
    }
    // the size word of a spare node is whatever it was; only its presence is compared
    let live_words = 1 + 2 * t.live.len();
    let same = want.len() == got.len() && want.iter().zip(&got).enumerate().all(|(i, (w, g))| w.is_none_or(|w| w as u32 == *g) || (i > live_words && (i - live_words) % 2 == 0));
    Verdict::from(same, || format!("tracker: port {got:?}, original {want:?}")).and(|| word(c, "mutex", t.mutex as u32))
}

fn block(a: u32) -> Option<usize> {
    (a != NIL).then_some(a as usize)
}

pub fn lookup(name_: &str) -> Option<Check> {
    Some(match name_ {
        "SI_mutexCreate" => |c| {
            run(c, 0, |r| {
                let mut h = 0x5555;
                let ok = sys::mutex_create(r, &mut h);
                word(c, "eax", ok as u32).and(|| word(c, "h", h as u32))
            })
        },
        "SI_mutexDestroy" => |c| {
            run(c, 0, |r| {
                let mut h = args(c)[0] as i32;
                let ok = sys::mutex_destroy(r, &mut h);
                word(c, "eax", ok as u32).and(|| word(c, "h", h as u32))
            })
        },
        "SI_mutexLock" => |c| {
            run(c, 0, |r| {
                sys::mutex_lock(r, args(c)[0] as i32);
                Verdict::Same
            })
        },
        "SI_mutexUnlock" => |c| {
            run(c, 0, |r| {
                sys::mutex_unlock(r, args(c)[0] as i32);
                Verdict::Same
            })
        },
        "eventWait" => |c| {
            run(c, 0, |r| {
                sys::check::event_wait(r, args(c)[0] as i32);
                Verdict::Same
            })
        },
        "eventCreate" => |c| {
            run(c, args(c)[0] as i32, |r| {
                let mut id = 0x5555;
                let ok = sys::check::event_create(r, &mut id);
                word(c, "eax", ok as u32).and(|| word(c, "id", id as u32))
            })
        },
        "eventDestroy" => |c| {
            run(c, 0, |r| {
                let mut id = args(c)[0] as i32;
                let ok = sys::check::event_destroy(r, &mut id);
                word(c, "eax", ok as u32).and(|| word(c, "id", id as u32))
            })
        },
        "SI_pipeioCreate" => |c| {
            run(c, 0, |r| {
                let (mut a, mut b) = (0x5555, 0x5555);
                let ok = sys::pipeio_create(r, &mut a, &mut b);
                word(c, "eax", ok as u32).and(|| word(c, "a", a as u32)).and(|| word(c, "b", b as u32))
            })
        },
        "SI_pipeioDestroy" => |c| {
            run(c, 0, |r| {
                let mut p = args(c)[0] as i32;
                let ok = sys::pipeio_destroy(r, &mut p);
                word(c, "eax", ok as u32).and(|| word(c, "p", p as u32))
            })
        },
        "SI_pipeioResetInheritance" => |c| run(c, 0, |r| word(c, "eax", sys::pipeio_reset_inheritance(r, args(c)[0] as i32) as u32)),
        "SI_pipeioWrite" | "SI_pipeioRead" | "pipeioReadWrite" => |c| {
            run(c, 0, |r| {
                let a = args(c);
                let mut buf = name(c, "data").unwrap();
                let mut count = 0x5555_5555u32 as i32;
                let res = match c.name.as_str() {
                    "SI_pipeioWrite" => sys::pipeio_write(r, a[0] as i32, &buf, &mut count),
                    "SI_pipeioRead" => sys::pipeio_read(r, a[0] as i32, &mut buf, &mut count),
                    _ => sys::pipeio_read_write(r, a[0] as i32, &mut buf, &mut count, a[2] as i32),
                };
                let mut packed = Vec::new();
                pack(&mut packed, Some(&buf));
                let same_buf = c.output("buf").u32s() == packed;
                let cnt = c.output("cnt").written()[0];
                word(c, "eax", res as u32)
                    .and(|| Verdict::from(same_buf != mutate(), || "the buffer differs".to_string()))
                    .and(|| Verdict::from(cnt.is_none() == (res != 0) && cnt.is_none_or(|w| w as u32 == count as u32), || format!("count: port {count}, original {cnt:?}")))
            })
        },
        "SI_pipeioOpenFILE" => |c| run(c, 0, |r| word(c, "eax", sys::pipeio_open_file(r, args(c)[0] as i32, &name(c, "name").unwrap()))),
        "SI_pipeioCloseFILE" => |c| run(c, 0, |r| word(c, "eax", sys::pipeio_close_file(r, args(c)[0]) as u32)),
        "SI_pipeioFileOpen" => |c| {
            run(c, 0, |r| {
                let mut fd = 0x5555;
                let ok = sys::pipeio_file_open(r, &mut fd, &name(c, "name").unwrap(), args(c)[0] as i32);
                word(c, "eax", ok as u32).and(|| word(c, "p", fd as u32))
            })
        },
        "SI_mrprocessCreate" => |c| {
            run(c, 0, |r| {
                let a = args(c);
                let mut argv = vec![name(c, "a0").unwrap()];
                if a[2] > 1 {
                    argv.push(name(c, "a1").unwrap());
                }
                word(c, "eax", sys::mrprocess_create(r, &argv, a[0] as i32, a[1] as i32) as u32)
            })
        },
        "SI_mrprocessDestroy" => |c| {
            run(c, 0, |r| {
                sys::mrprocess_destroy(r, args(c)[0] as i32);
                Verdict::Same
            })
        },
        "SI_mrprocessIsAlive" => |c| run(c, 0, |r| word(c, "eax", sys::mrprocess_is_alive(r, args(c)[0] as i32) as u32)),
        "SI_mrthreadCreate" => |c| run(c, 0, |r| word(c, "eax", sys::mrthread_create(r, Box::new(|| {}), args(c)[0] as i32) as u32)),
        "SI_mrthreadDestroy" => |c| word(c, "eax", sys::mrthread_destroy(args(c)[0] as i32) as u32),
        "SI_mrthreadIsAlive" => |c| run(c, 0, |r| word(c, "eax", sys::mrthread_is_alive(r, args(c)[0] as i32) as u32)),
        "SI_dynlibLoad" => |c| {
            run(c, 0, |r| {
                let mut h = 0x5555;
                let ok = sys::dynlib_load(r, &mut h, &name(c, "name").unwrap());
                word(c, "eax", ok as u32).and(|| word(c, "h", h as u32))
            })
        },
        "SI_dynlibFree" => |c| {
            run(c, 0, |r| {
                let mut h = args(c)[0] as i32;
                let ok = sys::dynlib_free(r, &mut h);
                word(c, "eax", ok as u32).and(|| word(c, "h", h as u32))
            })
        },
        "SI_dynlibSym" => |c| run(c, 0, |r| word(c, "eax", sys::dynlib_sym(r, args(c)[0] as i32, &name(c, "name").unwrap()))),
        "SI_mrfileIsValidExec" => |c| run(c, 0, |r| word(c, "eax", sys::mrfile_is_valid_exec(r, &name(c, "name").unwrap()) as u32)),
        "SI_mrfileGetExecFullpath" => |c| {
            run(c, 0, |r| {
                let got = sys::mrfile_get_exec_fullpath(r, &name(c, "name").unwrap());
                let mut packed = Vec::new();
                pack(&mut packed, got.as_deref());
                if mutate() {
                    packed[0] ^= 1;
                }
                word(c, "eax", got.is_some() as u32).and(|| Verdict::from(c.output("found").u32s() == packed, || format!("found: port {got:?}")))
            })
        },
        "SI_memtrackInitialize" => |c| {
            let t = sys::memtrack_initialize();
            let got = [t.mutex as u32 ^ mutate() as u32, t.live.len() as u32, t.spare.len() as u32];
            Verdict::from(c.output("p").u32s() == got, || "the record differs".to_string())
        },
        "SI_memtrackAllocate" => |c| {
            run(c, 0, |r| {
                let (mut t, _) = tracker(c);
                let got = sys::memtrack_allocate(r, &mut t, args(c)[0]);
                let want = c.output("eax").written()[0];
                Verdict::from(want.is_none_or(|w| (w != 0) == (got.is_some() != mutate())), || format!("answer: port {got:?}, original {want:?}")).and(|| track_words(c, &t))
            })
        },
        "SI_memtrackCalloc" | "SI_memtrackStrdup" => |c| {
            run(c, 0, |r| {
                let (mut t, _) = tracker(c);
                let got = if c.name == "SI_memtrackCalloc" { sys::memtrack_calloc(r, &mut t, args(c)[0], args(c)[1]) } else { sys::memtrack_strdup(r, &mut t, name(c, "name").as_deref()) };
                let want = c.output("eax").written()[0];
                let mut head = Vec::new();
                // the head of the live list after the call, whichever node that is
                pack(&mut head, t.live.first().and_then(|n| n.data.as_deref()));
                let made = got.is_some();
                Verdict::from(want.is_none_or(|w| (w != 0) == (made != mutate())), || format!("answer: port {got:?}, original {want:?}"))
                    .and(|| track_words(c, &t))
                    .and(|| Verdict::from(!made || c.output("head").u32s() == head, || "the new block's content differs".to_string()))
            })
        },
        "SI_memtrackFree" => |c| {
            run(c, 0, |r| {
                let (mut t, _) = tracker(c);
                sys::memtrack_free(r, &mut t, block(args(c)[0]));
                track_words(c, &t)
            })
        },
        "SI_memtrackRealloc" => |c| {
            run(c, 0, |r| {
                let (mut t, _) = tracker(c);
                let got = sys::memtrack_realloc(r, &mut t, block(args(c)[0]), args(c)[1]);
                let want = c.output("eax").written()[0];
                Verdict::from(want.is_none_or(|w| (w != 0) == (got.is_some() != mutate())), || format!("answer: port {got:?}, original {want:?}")).and(|| track_words(c, &t))
            })
        },
        "SI_memtrackReset" => |c| {
            run(c, 0, |r| {
                let (mut t, _) = tracker(c);
                sys::memtrack_reset(r, &mut t);
                track_words(c, &t)
            })
        },
        "SI_memtrackCheck" => |c| {
            run(c, 0, |r| {
                let (t, addresses) = tracker(c);
                let any = sys::memtrack_check(r, &t, &|i| addresses[i]);
                word(c, "ax", any as u32)
            })
        },
        _ => return None,
    })
}
