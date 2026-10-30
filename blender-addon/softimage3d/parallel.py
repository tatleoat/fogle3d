# SPDX-License-Identifier: GPL-3.0-or-later
"""Parallel frame rendering for animation renders (our own code; unit blender2).

The same plan as re/tools/perf1/render-par.py (contract: re/out/lead/blender/parallel-render-contract.md),
but driven by the engine's polling loop instead of a driver process, so progress, Esc and the process
groups stay in one place:

- the scene's frame range is cut into contiguous chunks; up to K `render-frames` processes run at a
  time, each with `RENDER_RANGE=a:b` (every picture it writes is byte-identical to the whole-range
  run's file, except the icon the first picture of a call carries, which is cut off here);
- a finished picture (and its `.Zpic` depth file, if the scene writes one) is moved by os.replace
  from the chunk's directory to `<out>/<scene dir>/`: a file there is complete;
- a scene whose evaluation carries state from frame to frame (expressions, constraints, particles,
  dynamics, ...) is not split: the caller gets a plain session.Child (one process, the whole range);
- cancel signals every process group (each part runs in a session of its own).
"""
import os
import re
import signal
import subprocess
import time

# relation ids whose evaluation can keep state from frame to frame (contract / render-par.py)
STATEFUL = [
    (range(20000, 20100), "expressions"),
    (range(1111, 1118), "constraints"),
    (range(2110, 2113), "motion-capture constraints"),
    (range(116, 129), "relations 116-128"),
    (range(160, 161), "relation 160"),
    (range(238, 240), "relations 238/239"),
    (range(500, 613), "particles / emitters"),
    (range(700, 701), "flock"),
    (range(800, 808), "dynamics"),
]

# processes of an animation render (each holds its own pictures and database copy): at most 16
MAX_THREADS = 16
# band threads of a still (one process; fit_threads lowers them when memory is short): up to all cores but one (unit
# soft4: the still life at 1920x1280 renders in 16.3 s with 16 threads, 14.9 s with 21 on 22 cores)
MAX_BAND_THREADS = 64


def usable_cores():
    try:
        return len(os.sched_getaffinity(0))
    except AttributeError:
        return os.cpu_count() or 1


def auto_count(cores=None, cap=MAX_THREADS):
    """Default process / thread count: all cores but one, never above `cap` (MAX_THREADS, or MAX_BAND_THREADS for a
    still's band threads)."""
    return max(1, min(cap, (cores or usable_cores()) - 1))


def stateful_reasons(dsc):
    """{reason: {relation ids}} for the scene's relations that carry state between frames."""
    try:
        with open(dsc, "rb") as fh:
            t = fh.read()
    except OSError:
        return {"unreadable scene description": set()}
    i = t.find(b"\nRELATIONS")
    j = t.find(b"EndOfRELATIONS")
    why = {}
    if i < 0:
        return why
    for m in re.finditer(rb"^\s+(\d+) (\d+) (\d+)( RELDATA|\s*;)", t[i:j], re.M):
        rid = int(m.group(3))
        for rg, name in STATEFUL:
            if rid in rg:
                why.setdefault(name, set()).add(rid)
    return why


def make_chunks(frames, k, per=3):
    """Contiguous (first, last) ranges of nearly equal frame counts: about `per` per process."""
    n = min(len(frames), max(1, per * k))
    base, rem = divmod(len(frames), n)
    out, at = [], 0
    for i in range(n):
        c = base + (1 if i < rem else 0)
        out.append((frames[at], frames[at + c - 1]))
        at += c
    return out


def strip_icon(path):
    """What a whole-range run does not have: the icon SI_image gives the first picture of each call."""
    with open(path, "rb") as fh:
        data = fh.read()
    if data[-4:] == b"ICON" and len(data) >= 8:
        off = int.from_bytes(data[-8:-4], "big")
        if off < len(data):
            with open(path, "r+b") as fh:
                fh.truncate(off)


def pictures(out_dir, ext=".pic"):
    """<out_dir>/<scene dir>/<name><ext> files, hidden ones too (a scene without an output name writes '.1.pic')."""
    out = []
    try:
        for d in os.scandir(out_dir):
            if d.is_dir() and not d.name.startswith(".parts"):
                for f in os.scandir(d.path):
                    if f.name.endswith(ext) and f.is_file():
                        out.append(f.path)
    except OSError:
        pass
    return out


def killpg(pid, sig):
    try:
        os.killpg(pid, sig)
    except (ProcessLookupError, PermissionError):
        pass


class _Proxy:
    """`.proc.poll()` as session.Child has it (tests ask whether any part is running)."""

    def __init__(self, owner):
        self.owner = owner

    def poll(self):
        return self.owner.returncode()


class ParChild:
    """K render-frames processes over chunks of a frame range; the interface of session.Child."""

    def __init__(self, exe, root, only, extra_env, out_dir, frames, k, threads=1, mem_kb=2000000, per=3):
        self.exe, self.root, self.only = exe, root, only
        self.extra_env = dict(extra_env)
        self.out_dir = out_dir
        self.frames = list(frames)
        self.first = self.frames[0]
        self.k = max(1, k)
        self.threads = max(1, threads)
        self.mem_kb = mem_kb
        self.chunks = make_chunks(self.frames, self.k, per)
        self.todo = list(range(len(self.chunks)))
        self.run = {}                    # part -> Popen
        self.status = {}                 # part -> exit status
        self.merged_final = set()
        self.cancelled = False
        self.t0 = time.time()
        self.starts = {}
        self.pid = None
        self.proc = _Proxy(self)
        os.makedirs(os.path.join(out_dir, ".parts"), exist_ok=True)
        self.log_path = os.path.join(out_dir, "run.txt")
        self._logs = {}
        self.pump()

    # -- processes
    def _launch(self, i):
        lo, hi = self.chunks[i]
        pdir = os.path.join(self.out_dir, ".parts", "p%d" % i)
        os.makedirs(pdir, exist_ok=True)
        env = dict(os.environ)
        env.update(self.extra_env)
        env["RENSETUP_ONLY"] = self.only
        env["RENSETUP_PICTURES"] = pdir
        env["RENDER_RANGE"] = "%d:%d" % (lo, hi)
        if self.threads > 1:
            env["RENDER_THREADS"] = str(self.threads)
        else:
            env.pop("RENDER_THREADS", None)
        log = open(os.path.join(self.out_dir, ".parts", "%d.log" % i), "wb")
        self._logs[i] = log
        cmd = ["/bin/sh", "-c", 'ulimit -v %d; exec "$0" "$1"' % self.mem_kb, self.exe, self.root]
        p = subprocess.Popen(cmd, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        if self.pid is None:
            self.pid = p.pid
        self.run[i] = p

    def pump(self):
        """Reap finished parts, start the next chunks, move complete pictures to the result directory."""
        if self.cancelled:
            return
        for i, p in list(self.run.items()):
            rc = p.poll()
            if rc is not None:
                self.status[i] = rc
                del self.run[i]
                self._logs[i].close()
        while self.todo and len(self.run) < self.k and not any(s != 0 for s in self.status.values()):
            self._launch(self.todo.pop(0))
        for i in range(len(self.chunks)):
            if i in self.merged_final:
                continue
            final = i in self.status
            self._merge(i, final)
            if final:
                self.merged_final.add(i)
        if self.todo and any(s != 0 for s in self.status.values()):
            self.todo = []              # a part failed: do not start the rest

    def _merge(self, i, final):
        pdir = os.path.join(self.out_dir, ".parts", "p%d" % i)
        if not os.path.isdir(pdir):
            return
        lo = self.chunks[i][0]
        for dirpath, _dirs, files in os.walk(pdir):
            rel = os.path.relpath(dirpath, pdir)
            pics = {}
            for f in files:
                if f == "index.tsv":
                    if final and i == 0:
                        os.replace(os.path.join(dirpath, f), os.path.join(self.out_dir, "index.tsv"))
                    continue
                m = re.search(r"\.(-?\d+)\.pic$", f)
                if m:
                    pics[int(m[1])] = f
            top = max(pics) if pics else None
            short = final and top is not None and self._out_of_memory(i)
            for n, f in sorted(pics.items()):
                if n == top and (not final or short):
                    continue                    # may still be written (or was cut short when the part ran out of memory)
                src = os.path.join(dirpath, f)
                dst = os.path.join(self.out_dir, rel, f)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                if n == lo and n != self.first:
                    strip_icon(src)
                z = src[:-len(".pic")] + ".Zpic"      # the depth file first: the picture's arrival says both are there
                if os.path.exists(z):
                    os.replace(z, dst[:-len(".pic")] + ".Zpic")
                os.replace(src, dst)
                self.starts.setdefault(n, time.time())

    def _out_of_memory(self, i):
        """True when part i ended with an error after running out of its memory cap."""
        if self.status.get(i, 0) == 0:
            return False
        from . import session
        try:
            with open(os.path.join(self.out_dir, ".parts", "%d.log" % i), "rb") as fh:
                return session.out_of_memory(fh.read()[-4000:].decode("utf-8", "replace"))
        except OSError:
            return False

    # -- the Child interface
    def exited(self):
        self.pump()
        return self.cancelled or (not self.todo and not self.run)

    def returncode(self):
        if not self.exited():
            return None
        if self.cancelled:
            return -signal.SIGTERM
        bad = [s for s in self.status.values() if s != 0]
        return bad[0] if bad else 0

    def files(self):
        self.pump()
        out = {}
        for p in pictures(self.out_dir):
            parts = os.path.basename(p).rsplit(".", 2)
            if len(parts) == 3 and parts[1].lstrip("-").isdigit():
                out[int(parts[1])] = p
                self.starts.setdefault(int(parts[1]), time.time())
        return out

    def ready(self):
        return self.files()                     # a file in the result directory is complete

    def cancel(self, grace=3.0):
        self.cancelled = True
        procs = list(self.run.values())
        for p in procs:
            killpg(p.pid, signal.SIGTERM)
        t = time.time()
        for p in procs:
            try:
                p.wait(timeout=max(0.05, grace - (time.time() - t)))
            except subprocess.TimeoutExpired:
                killpg(p.pid, signal.SIGKILL)
                try:
                    p.wait(timeout=grace)
                except subprocess.TimeoutExpired:
                    pass
        for i, p in list(self.run.items()):
            self.status[i] = p.poll() if p.poll() is not None else -9
            self._logs[i].close()
        self.run.clear()
        self.todo = []

    def log_tail(self, n=400):
        out = []
        for i in sorted(self.status):
            if self.status[i] != 0:
                try:
                    with open(os.path.join(self.out_dir, ".parts", "%d.log" % i), "rb") as fh:
                        out.append("part %d (exit %s): %s" % (i, self.status[i], fh.read()[-n:].decode("utf-8", "replace").strip()))
                except OSError:
                    pass
        return "\n".join(out)

    def summary(self):
        return {"k": self.k, "threads": self.threads, "chunks": self.chunks, "status": dict(self.status)}
