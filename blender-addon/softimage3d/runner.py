# SPDX-License-Identifier: GPL-3.0-or-later
"""Runs render-frames and finds the picture of a frame (our own code)."""
import atexit
import glob
import os
import shutil
import subprocess
import tempfile

_runs = {}      # key -> directory of a finished run
_temps = []


def cleanup():
    for d in _temps:
        shutil.rmtree(d, ignore_errors=True)
    _temps.clear()
    _runs.clear()


atexit.register(cleanup)


def _new_dir(keep):
    if keep:
        os.makedirs(keep, exist_ok=True)
        return tempfile.mkdtemp(prefix="si3d-", dir=keep)
    d = tempfile.mkdtemp(prefix="si3d-")
    _temps.append(d)
    return d


def run_render_frames(st, installed, only, extra_env, out_dir, size=None):
    """One render-frames run; returns (its report path, exit status).  `size` (the picture's width, height)
    chooses the automatic memory cap; a run that ran out of it raises session.SessionError."""
    from . import session
    exe = session.renderer_path(st, bpy_abspath)
    if not exe or not os.path.isfile(exe):
        raise RuntimeError("render-frames program not set (Render > Fogle3D, or env SI3D_RENDER_FRAMES)")
    from . import install
    # `installed` may be a database blend2db built (export.py): the program file is in the user's install
    root = session.install_root(st, bpy_abspath)
    _, bad = install.check(installed, install.RENDER, program_root=root)
    if bad:
        raise RuntimeError("\n".join(bad))
    env = dict(os.environ)
    env.update(install.program_env(root))
    env.update(extra_env)
    thr = session.fit_threads(session.counts(1, False)[1], size, installed, st)
    env["RENSETUP_ONLY"] = only
    env["RENSETUP_PICTURES"] = out_dir
    os.makedirs(out_dir, exist_ok=True)
    log = os.path.join(out_dir, "run.txt")
    # cap the renderer's address space as the corpus runs do (the chosen cap, or the automatic one for the picture and the
    # database); a run that ran out of the automatic cap is tried again with more memory or fewer threads (session.attempts)
    tries = session.attempts(st, size, thr, installed)
    # the time limit grows with the picture (session.run_timeout); a run past session.LONG_JOB_S says so on the console
    limit = session.run_timeout(size)
    for i, (t, mem_kb) in enumerate(tries):
        if t > 1:
            env["RENDER_THREADS"] = str(t)
        elif i:
            env.pop("RENDER_THREADS", None)
        with open(log, "wb") as fh:
            p = subprocess.Popen(["/bin/sh", "-c", 'ulimit -v %d; exec "$0" "$1"' % mem_kb, exe, installed],
                                 env=env, stdout=fh, stderr=subprocess.STDOUT)
            try:
                try:
                    p.wait(timeout=min(limit, session.LONG_JOB_S))
                except subprocess.TimeoutExpired:
                    if limit <= session.LONG_JOB_S:
                        raise
                    print(session.long_job_message(size, t, session.LONG_JOB_S, limit), flush=True)
                    p.wait(timeout=limit - session.LONG_JOB_S)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()
                raise RuntimeError("the renderer ran longer than its time limit of %d s for this picture (env "
                                   "SI3D_RENDER_TIMEOUT sets it; see %s)" % (limit, log))
        if p.returncode == 0 or i + 1 == len(tries) or not session.out_of_memory(_tail(log)):
            break
    if p.returncode != 0:
        session.check_memory(log, mem_kb, size)
    return log, p.returncode


def _tail(path, n=4000):
    try:
        with open(path, "rb") as fh:
            fh.seek(0, 2)
            fh.seek(max(0, fh.tell() - n))
            return fh.read().decode("utf-8", "replace")
    except OSError:
        return ""


def bpy_abspath(p):
    import bpy
    return bpy.path.abspath(p) if p else p


def _pictures(out_dir):
    from . import parallel
    return parallel.pictures(out_dir)


def frame_of(path):
    """NAME.<frame>.pic -> frame."""
    parts = os.path.basename(path).rsplit(".", 2)
    return int(parts[1]) if len(parts) == 3 and parts[1].lstrip("-").isdigit() else None
