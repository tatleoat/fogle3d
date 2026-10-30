# SPDX-License-Identifier: GPL-3.0-or-later
"""Supervised renderer runs: progress, cancel, region, frame mapping, install checks (our own code).

The renderer (`render-frames`, re/tools/si-render/pictures) is one process per scene and prints
nothing until it ends.  Its only live signal is the picture files: `<out>/<scene>/<name>.<n>.pic`
is created (112 bytes, the header) when frame n starts and holds the whole picture when frame n
ends.  So frame n is complete as soon as a frame > n exists, or the process has exited.  Nothing
in the renderer was changed for this.

Cancel stops the child's whole process group (each renderer process runs in a session of its own:
SIGTERM, then SIGKILL after a grace period).  Animation renders of scenes that carry no state from
frame to frame run on K processes (parallel.py); stills use the renderer's band threads
(RENDER_THREADS, crates/si-render bands.rs; byte-identical to one thread).

Region: the original's render set-up has its own sub-region (`SUBREGION_FRAME l r t b`, fractions
cut off the left, right, top and bottom; switched on by `MOTION_BLUR .. SUBFRAME ACTIVE`).  A
region run therefore renders a patched copy of the scene's database (hard links, only the .sts
is new) and the renderer writes a picture of the region's size.  Its pixels equal the cropped full
picture except for edge pixels (measured in re/out/blenderqol1/region.txt)."""
import glob
import os
import re
import shutil
import signal
import subprocess
import tempfile
import time

from . import parallel

CANCEL_FLAG = False          # set by tests or an operator; engine.test_break() is the user's Esc
LAST_PID = None              # PID of the most recent child (tests)
LOG = []                     # (time, status text, fraction) of every progress update (tests)
HEADER_BYTES = 112           # a picture file that has only its header


class SessionError(RuntimeError):
    pass


# ----------------------------------------------------------------------------- preferences
def prefs():
    """The add-on preferences, or None (add-on registered by hand, as the tests do)."""
    try:
        import bpy
        return bpy.context.preferences.addons[__package__].preferences
    except Exception:
        return None


def _env_int(name):
    try:
        return max(0, int(os.environ.get(name, "0")))
    except ValueError:
        return 0


def bundled(name):
    """A program shipped inside the add-on package (bin/<name>, our own compiled code), or ""."""
    p = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", name)
    if not os.path.isfile(p):
        return ""
    if not os.access(p, os.X_OK):      # unzipping an extension drops the execute bit
        try:
            os.chmod(p, 0o755)
        except OSError:
            pass
    return p


def renderer_path(st, bpy_abspath=lambda p: p):
    """render-frames: the scene's own setting, else the preference (hardware x87 build when chosen). The scene setting's default is env SI3D_RENDER_FRAMES."""
    if st.renderer:
        return bpy_abspath(st.renderer)
    pr = prefs()
    if pr is not None:
        if pr.use_hw_x87 and pr.renderer_hw:
            return bpy_abspath(pr.renderer_hw)
        if pr.renderer:
            return bpy_abspath(pr.renderer)
    return bundled("render-frames")


def install_root(st, bpy_abspath=lambda p: p):
    """The user's Softimage|3D 4.0 install: the scene's own field, else env SI_INSTALL, else the preference
    "Softimage 3D 4.0 install path" (install.py)."""
    from . import install
    pr = prefs()
    return install.resolve_root(st.installed, pr.install_path if pr is not None else "", bpy_abspath)


def blend2db_path(st, bpy_abspath=lambda p: p):
    if st.blend2db:
        return bpy_abspath(st.blend2db)
    pr = prefs()
    if pr is not None and pr.blend2db:
        return bpy_abspath(pr.blend2db)
    return bundled("blend2db")


# The renderer's address-space cap (ulimit -v) when nothing sets one: AUTO_BASE_MB for any run, plus
# AUTO_MB_PER_MPIXEL for each million pixels of the picture and AUTO_MB_PER_THREAD for each band thread, at most
# AUTO_RAM_SHARE of the machine's memory.  Measured on 4K renders (unit dof1, re/out/dof1/mem): the peak address
# space grows with the picture (the sample, tag and depth buffers are per pixel) and with each thread (its malloc
# arena and stack): Beauty_shot 3840x3251 5763 MB with 16 threads, 4898 with 2; IK-PageTurn 3840x3840 4090 / 3324;
# OBJECT-FLAMBEAU_MR 3840x2880 2815 / 1375; Beauty_shot 300x254 1500 with 16 threads.  The caps this gives are
# 2.0 to 3.8 times those peaks.
AUTO_BASE_MB = 2000
AUTO_MB_PER_MPIXEL = 600
AUTO_MB_PER_THREAD = 128
AUTO_RAM_SHARE = 0.75
# what a renderer that hit its cap prints (Rust's allocation failure; a thread whose stack could not be mapped)
_OOM_SIGNS = ("memory allocation of ", "failed to spawn thread", "Cannot allocate memory", "out of memory")


def _ram_mb():
    """The machine's memory in MB (/proc/meminfo), or None."""
    try:
        with open("/proc/meminfo") as fh:
            for line in fh:
                if line.startswith("MemTotal:"):
                    return int(line.split()[1]) // 1000
    except (OSError, ValueError, IndexError):
        pass
    return None


def auto_memory_mb(size=None, threads=1):
    """The automatic cap in MB for a picture of `size` (width, height; None = unknown, the base alone)."""
    mb = AUTO_BASE_MB
    if size and size[0] and size[1]:
        mpx = max(0, int(size[0])) * max(0, int(size[1])) / 1e6
        mb += int(mpx * AUTO_MB_PER_MPIXEL + 0.999) + AUTO_MB_PER_THREAD * max(1, int(threads or 1))
    ram = _ram_mb()
    if ram:
        mb = min(mb, max(AUTO_BASE_MB, int(ram * AUTO_RAM_SHARE)))
    return mb


def memory_mb_set(st=None):
    """The cap someone chose, in MB, or 0 (automatic): Render Properties > Fogle3D, then the add-on preference,
    then env SI3D_MEM_MB."""
    mb = getattr(st, "memory_mb", 0) if st is not None else 0
    if not mb:
        pr = prefs()
        mb = getattr(pr, "memory_mb", 0) if pr is not None else 0
    return mb or _env_int("SI3D_MEM_MB")


def memory_kb(st=None, size=None, threads=1):
    """The address-space cap (KB, for ulimit -v) of one renderer process: the chosen one (memory_mb_set), else
    the automatic one for the picture `size` and band `threads` (auto_memory_mb)."""
    return (memory_mb_set(st) or auto_memory_mb(size, threads)) * 1000


def fit_processes(k, mem_kb):
    """`k` renderer processes, fewer when the processes count is automatic and k caps of `mem_kb` each would pass
    AUTO_RAM_SHARE of the machine's memory (a 4K animation: each process holds its own picture buffers)."""
    pr = prefs()
    if (pr.processes if pr is not None else 0) or _env_int("SI3D_PROCESSES"):
        return k
    ram = _ram_mb()
    if not ram or not mem_kb:
        return k
    return max(1, min(k, int(ram * AUTO_RAM_SHARE * 1000) // mem_kb))


def out_of_memory(text):
    """True when a renderer log shows it ran out of its memory cap."""
    return bool(text) and any(s in text for s in _OOM_SIGNS)


def memory_message(mem_kb, size=None):
    """The error shown when the renderer ran out of memory."""
    pic = " for a %dx%d picture" % (size[0], size[1]) if size and size[0] and size[1] else ""
    return ("the renderer ran out of memory: it may use at most %d MB%s. Raise 'Memory (MB)' in Render Properties > "
            "Fogle3D (or 'Memory per process' in the add-on preferences, or env SI3D_MEM_MB)" % (mem_kb // 1000, pic))


def check_memory(log_path, mem_kb, size=None):
    """Raise SessionError(memory_message) when the run whose log is `log_path` ran out of memory."""
    try:
        with open(log_path, "rb") as fh:
            fh.seek(0, 2)
            n = fh.tell()
            fh.seek(max(0, n - 4000))
            tail = fh.read().decode("utf-8", "replace")
    except OSError:
        return
    if out_of_memory(tail):
        raise SessionError(memory_message(mem_kb, size))


def counts(n_frames, animation):
    """(processes, band threads) for a render. Preference value, else env (SI3D_PROCESSES, SI3D_BAND_THREADS),
    else the default: cores - 1 capped at 16. Animations spread over processes (one band thread each); a
    still is one process with band threads."""
    pr = prefs()
    procs = (pr.processes if pr is not None else 0) or _env_int("SI3D_PROCESSES")
    thr = (pr.band_threads if pr is not None else 0) or _env_int("SI3D_BAND_THREADS")
    auto = parallel.auto_count()
    cap = parallel.MAX_THREADS
    if animation:
        k = max(1, min(procs or auto, cap, max(1, n_frames)))
        return k, max(1, min(thr or 1, cap))
    return 1, max(1, min(thr or auto, cap))


# ----------------------------------------------------------------------------- tool-only scene source
# The engine renders the Blender scene.  File > Import > Softimage 3D scene is the only way a user works with a .dsc.
# For the tools (fidelity sweeps, regression suites, tests) the engine can still render an installed .dsc directly,
# to compare with its Blender-scene render.  That path is reachable only from code: set_tool_scene() from a tool script,
# or the environment variable SI3D_TOOL_SCENE (text of the scene's path; SI3D_TOOL_FRAME_MAP = SAME or START).
# No panel, menu, operator or property shows it.
class ToolScene:
    def __init__(self, filter, frame_map="SAME"):
        self.filter, self.frame_map = filter, frame_map


_tool_scenes = {}


def set_tool_scene(scene, text, frame_map="SAME"):
    """Tools: render the installed .dsc whose path holds `text` for this Blender scene (None / "" = the Blender scene)."""
    if text:
        _tool_scenes[scene.name_full] = ToolScene(text, frame_map)
    else:
        _tool_scenes.pop(scene.name_full, None)


def tool_scene(st):
    """The ToolScene of the scene `st` (its si3d settings) belongs to, or None (the normal case: the Blender scene renders)."""
    owner = getattr(st, "id_data", None)
    t = _tool_scenes.get(owner.name_full) if owner is not None else None
    if t is None and os.environ.get("SI3D_TOOL_SCENE"):
        t = ToolScene(os.environ["SI3D_TOOL_SCENE"], os.environ.get("SI3D_TOOL_FRAME_MAP", "SAME"))
    return t


# ----------------------------------------------------------------------------- install checks
def problems(st, bpy_abspath=lambda p: p):
    """Everything that is missing for a render, as readable lines (empty list = ready)."""
    out = []
    exe = renderer_path(st, bpy_abspath)
    if not exe:
        out.append("The renderer program (render-frames) is not set. Set 'render-frames' below "
                   "or the environment variable SI3D_RENDER_FRAMES.")
    elif not os.path.isfile(exe):
        out.append("The renderer program was not found: %s" % exe)
    elif not os.access(exe, os.X_OK):
        out.append("The renderer program is not executable: %s" % exe)
    from . import install
    tool = tool_scene(st)
    _, bad = install.check(install_root(st, bpy_abspath), install.RENDER if tool else install.BLEND2DB)
    out.extend(bad)
    if not tool:
        b2 = blend2db_path(st, bpy_abspath)
        if not b2 or not os.path.isfile(b2):
            out.append("The blend2db program is not set or not found (needed to render this Blender scene).")
    return out


def find_rsrc(installed):
    """The install's Relations.rsrc (a path under /linux/ first): install.find, whose answer is kept while the file is there."""
    from . import install
    return install.find(installed, "Relations.rsrc", "/linux/")


_dsc_cache = {}


def find_scene(installed, text):
    """The one .dsc whose path holds `text` (the renderer renders every match, so more than one is refused)."""
    key = installed
    if key not in _dsc_cache:
        _dsc_cache[key] = sorted(glob.glob(os.path.join(installed, "**", "SCENES", "*.dsc"), recursive=True))
    hits = [p for p in _dsc_cache[key] if text in p]
    if not hits:
        raise SessionError("no scene of %s has %r in its path" % (installed, text))
    if len(hits) > 1:
        raise SessionError("%r matches %d scenes (%s ...); make it longer" % (text, len(hits), os.path.basename(hits[0])))
    return hits[0]


def sts_path(dsc):
    """The scene's render set-up file: the element its description lists in the SETUP_SOFT chapter
    (usually the scene's own name, not always: Leaves.1-0 names fast-Leaves.3-0)."""
    name = os.path.basename(dsc)[:-4]
    try:
        with open(dsc, "r", errors="replace") as fh:
            m = re.search(r"CHAPTER\s+SETUP_SOFT\s+NBELEM\s+\d+\s+(\S+)", fh.read())
        if m:
            name = m.group(1)
    except OSError:
        pass
    return os.path.join(os.path.dirname(os.path.dirname(dsc)), "SETUP_SOFT", name + ".sts")


def scene_info(dsc):
    """{'first','last','step','width','height'} from the scene's render set-up (None where absent)."""
    info = {"first": None, "last": None, "step": 1, "width": None, "height": None}
    try:
        with open(sts_path(dsc), "r", errors="replace") as fh:
            text = fh.read()
    except OSError:
        return info
    m = re.search(r"^\s*RENDERING_FRAME\s+(-?\d+)\s+(-?\d+)\s+(-?\d+)", text, re.M)
    if m:
        info["first"], info["last"], info["step"] = int(m.group(1)), int(m.group(2)), max(1, int(m.group(3)))
    m = re.search(r"^\s*RESOLUTION\s+(\d+)\s+(\d+)", text, re.M)
    if m:
        info["width"], info["height"] = int(m.group(1)), int(m.group(2))
    return info


def own_region_left(dsc):
    """Left cut-off fraction of the set-up's own sub-region (SUBREGION_FRAME l r t b ..., switched on by
    MOTION_BLUR .. SUBFRAME ACTIVE), or None when it is off."""
    try:
        with open(sts_path(dsc), "r", errors="replace") as fh:
            t = fh.read()
    except OSError:
        return None
    m = re.search(r"^\s*SUBREGION_FRAME\s+(\S+)", t, re.M)
    on = re.search(r"MOTION_BLUR\s+\S+\s+SUBFRAME\s+ACTIVE", t)
    if m and on:
        try:
            return float(m.group(1))
        except ValueError:
            return None
    return None


def scene_frames(info):
    if info["first"] is None:
        return None
    return list(range(info["first"], info["last"] + 1, info["step"]))


def map_frame(blender_frame, mode, info, blender_start):
    """Blender frame -> Softimage frame.  SAME: n is n.  START: Blender's first frame is the scene's first."""
    if mode == "START" and info["first"] is not None:
        return info["first"] + (blender_frame - blender_start)
    return blender_frame


def frame_range_message(sf, blender_frame, mode, info, blender_start):
    """Error text for a frame outside the scene's range: names the setting and the valid Blender frames."""
    first, last, step = info["first"], info["last"], info["step"]
    b0 = blender_start if mode == "START" else first
    b1 = b0 + (last - first)
    return ("Blender frame %d maps to scene frame %d, outside the scene's range %d-%d step %d. "
            "Valid Blender frames with Frame map '%s': %d-%d (tool setting frame_map, "
            "or move the Blender frame)" % (blender_frame, sf, first, last, step,
                                            "Start to first" if mode == "START" else "Same number", b0, b1))


# ----------------------------------------------------------------------------- region overlay
def has_z_channel(dsc):
    try:
        with open(sts_path(dsc), "r", errors="replace") as fh:
            return re.search(r"^\s*Z_CHANNEL\s+YES", fh.read(), re.M) is not None
    except OSError:
        return False


def patch_sts(text, region=None, frame_range=None, z_channel=False):
    """The render set-up with a sub-region box (fractions l, r, t, b cut off each side) switched on,
    and optionally another RENDERING_FRAME range (first, last, step)."""
    if frame_range is not None:
        text, n = re.subn(r"^(\s*)RENDERING_FRAME\s+-?\d+\s+-?\d+\s+-?\d+[ \t]*$",
                          r"\g<1>RENDERING_FRAME %d %d %d" % frame_range, text, count=1, flags=re.M)
        if n != 1:
            raise SessionError("the scene's set-up has no RENDERING_FRAME line")
    if z_channel:
        # the depth file <name>.<frame>.Zpic is written beside the picture (finding 0012 section 31)
        text, n = re.subn(r"^(\s*)Z_CHANNEL\s+(?:NO|YES)", r"\g<1>Z_CHANNEL YES", text, count=1, flags=re.M)
        if n != 1:
            raise SessionError("the scene's set-up has no Z_CHANNEL line")
    if region is not None:
        l, r, t, b = region
        text = re.sub(r"^\s*SUBREGION_FRAME .*\n", "", text, flags=re.M)
        text, n = re.subn(r"^(\s*)(RENDERING_FRAME .*)$",
                          lambda m: "%s%s\n%sSUBREGION_FRAME %.9g %.9g %.9g %.9g 0 0" % (m.group(1), m.group(2), m.group(1), l, r, t, b),
                          text, count=1, flags=re.M)
        if n != 1:
            raise SessionError("the scene's set-up has no RENDERING_FRAME line")
        # the later MOTION_BLUR line sets the switch again (SUBFRAME), so it must say ACTIVE too
        text = re.sub(r"(MOTION_BLUR\s+\S+\s+SUBFRAME\s+)(?:INACTIVE|ACTIVE)", r"\g<1>ACTIVE", text)
    return text


def make_overlay(installed, dsc, dest, region=None, frame_range=None, z_channel=False):
    """A small install tree for one scene: rsrc/ (Relations.rsrc, noIcon.pic) and db/ (the scene's
    database, files hard-linked or copied, its .sts rewritten).  Returns the tree's root."""
    rsrc = find_rsrc(installed)
    db = os.path.dirname(os.path.dirname(dsc))
    os.makedirs(os.path.join(dest, "rsrc"))
    for n in ("Relations.rsrc", "noIcon.pic"):
        src = os.path.join(os.path.dirname(rsrc), n)
        if os.path.isfile(src):
            shutil.copy(src, os.path.join(dest, "rsrc", n))
    top = os.path.join(dest, "db")
    for r, _dirs, files in os.walk(db):
        rel = os.path.relpath(r, db)
        os.makedirs(os.path.join(top, rel), exist_ok=True)
        for f in files:
            s, d = os.path.join(r, f), os.path.join(top, rel, f)
            try:
                os.link(s, d)
            except OSError:
                shutil.copy2(s, d)
    p = os.path.join(top, "SETUP_SOFT", os.path.basename(sts_path(dsc)))
    with open(sts_path(dsc), "r", errors="surrogateescape") as fh:
        text = fh.read()
    os.unlink(p)                       # never write through the hard link into the install
    with open(p, "w", errors="surrogateescape") as fh:
        fh.write(patch_sts(text, region, frame_range, z_channel))
    return dest


def region_fractions(border, rw, rh):
    """Blender border (min_x, max_x, min_y, max_y as 0..1, y from the bottom) -> the original's
    (left, right, top, bottom) cut-offs, and the box in Blender pixels (x0, y0, w, h)."""
    mnx, mxx, mny, mxy = border
    x0, x1 = int(round(mnx * rw)), int(round(mxx * rw))
    y0, y1 = int(round(mny * rh)), int(round(mxy * rh))
    x1, y1 = max(x1, x0 + 1), max(y1, y0 + 1)
    frac = (x0 / rw, (rw - x1) / rw, (rh - y1) / rh, y0 / rh)
    return frac, (x0, y0, x1 - x0, y1 - y0)


# ----------------------------------------------------------------------------- the child
def build_command(exe, installed, mem_kb=None):
    # cap the renderer's address space as the corpus runs do (memory_kb: the chosen cap or the automatic one)
    return ["/bin/sh", "-c", 'ulimit -v %d; exec "$0" "$1"' % (mem_kb or memory_kb()), exe, installed]


def start_run(exe, root, only, extra_env, out_dir, frames, animation, dsc=None, full=None, st=None, size=None):
    """The renderer for `frames` (scene frames to render, a contiguous slice of the scene's range `full`; None
    when the range is unknown), as a Child-like object (its `.frames` says which frames it will write): K
    processes for an animation of a scene with no frame-to-frame state (parallel.ParChild), else one process
    (Child) with band threads. A scene that carries state between frames always renders its whole range in
    one process (the pictures of a frame alone can differ). `only` must pick one scene.  `st` and `size`
    (the picture's width, height) choose the memory cap (memory_kb)."""
    why = {}
    alone = "RENDER_FRAME" in extra_env
    if frames is not None and not alone:
        dsc = dsc or find_scene(root, only)
        why = parallel.stateful_reasons(dsc)
    k, thr = counts(len(frames) if frames else 1, animation)
    k = fit_processes(k, memory_kb(st, size, thr))
    if frames is not None and not why and not alone and len(frames) > 1 and k > 1 and animation:
        c = parallel.ParChild(exe, root, only, extra_env, out_dir, frames, k, thr, memory_kb(st, size, thr))
        c.size = size
        return c
    env = dict(extra_env)
    if frames is not None and not why and not alone and len(frames) >= 1 and full is not None and list(frames) != list(full):
        env["RENDER_RANGE"] = "%d:%d" % (frames[0], frames[-1])      # a slice of the range, same bytes as in-range
    c = Child(exe, root, only, env, out_dir, thr, memory_kb(st, size, thr), size)
    c.sequential = sorted(why) if why else None
    c.frames = (full if why else frames) if frames is not None else None
    return c


class Child:
    """One renderer process, watched through its picture files."""

    def __init__(self, exe, installed, only, extra_env, out_dir, threads=1, mem_kb=None, size=None):
        global LAST_PID
        self.mem_kb = mem_kb or memory_kb(None, size, threads)
        self.size = size
        env = dict(os.environ)
        env.update(extra_env)
        env["RENSETUP_ONLY"] = only
        env["RENSETUP_PICTURES"] = out_dir
        if threads > 1:
            env["RENDER_THREADS"] = str(threads)
        os.makedirs(out_dir, exist_ok=True)
        self.out_dir = out_dir
        self.log_path = os.path.join(out_dir, "run.txt")
        self._log = open(self.log_path, "wb")
        # a session of its own: the process group is what Esc stops
        self.proc = subprocess.Popen(build_command(exe, installed, self.mem_kb), env=env, stdout=self._log,
                                     stderr=subprocess.STDOUT, start_new_session=True)
        self.pid = self.proc.pid
        LAST_PID = self.pid
        self._pidfd = _pidfd(self.pid)    # readable when the process ends (opened before anything can reap it)
        self.t0 = time.time()
        self.cancelled = False
        self.starts = {}                  # frame -> time its file first showed

    def exited(self):
        rc = self.proc.poll()
        if rc is not None:
            if not self._log.closed:
                self._log.close()
            if self._pidfd is not None:
                os.close(self._pidfd)
                self._pidfd = None
        return rc is not None

    def nap(self, seconds):
        """Sleep up to `seconds`, waking at once when the process ends."""
        _nap(self._pidfd, seconds, self.proc)

    def returncode(self):
        return self.proc.poll()

    def files(self):
        """{frame: path} of the picture files that exist now."""
        out = {}
        for p in parallel.pictures(self.out_dir):
            parts = os.path.basename(p).rsplit(".", 2)
            if len(parts) == 3 and parts[1].lstrip("-").isdigit():
                fr = int(parts[1])
                out[fr] = p
                self.starts.setdefault(fr, time.time())
        return out

    def ready(self):
        """{frame: path} of complete pictures: a later frame exists, or the process has ended."""
        f = self.files()
        if not f:
            return {}
        top = max(f)
        if self.exited():
            if self.returncode() != 0 and out_of_memory(self.log_tail(4000)):
                f.pop(top)                # the frame it was writing when it ran out of memory is cut short
            return {k: v for k, v in f.items() if os.path.getsize(v) > HEADER_BYTES}
        return {k: v for k, v in f.items() if k < top}

    def cancel(self, grace=3.0):
        """Stop the child's process group: SIGTERM, then SIGKILL if it is still there after `grace` seconds."""
        self.cancelled = True
        if self.proc.poll() is None:
            try:
                parallel.killpg(self.pid, signal.SIGTERM)
                self.proc.wait(timeout=grace)
            except subprocess.TimeoutExpired:
                parallel.killpg(self.pid, signal.SIGKILL)
                self.proc.wait(timeout=grace)
        self.exited()

    def log_tail(self, n=400):
        try:
            with open(self.log_path, "rb") as fh:
                return fh.read()[-n:].decode("utf-8", "replace").strip()
        except OSError:
            return ""


def _pidfd(pid):
    """A pidfd of the child (Linux 5.3+: readable once the process has ended), or None.  Blender's own Python is built
    without os.pidfd_open, so the system call is made through ctypes (number 434 on x86-64 and arm64)."""
    f = getattr(os, "pidfd_open", None)
    try:
        if f is not None:
            return f(pid)
        import platform
        if not os.uname().sysname == "Linux" or platform.machine() not in ("x86_64", "aarch64", "arm64"):
            return None
        import ctypes
        libc = ctypes.CDLL(None, use_errno=True)
        fd = libc.syscall(ctypes.c_long(434), ctypes.c_int(pid), ctypes.c_uint(0))
        return fd if fd >= 0 else None
    except (AttributeError, OSError, ValueError):
        return None


def _nap(fd, seconds, proc=None):
    """Sleep up to `seconds`; with a pidfd, wake when the process ends.  Without one, Popen.wait's own short sleeps
    (0.5 ms doubling up to 50 ms) still see the end sooner than one plain sleep."""
    if fd is not None:
        try:
            import select
            p = select.poll()
            p.register(fd, select.POLLIN)
            p.poll(max(0, int(seconds * 1000)))
            return
        except (OSError, ValueError):
            pass
    if proc is not None:
        try:
            proc.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            pass
        return
    time.sleep(seconds)


def nap(child, seconds):
    """The poll loops' pause: up to `seconds`, cut short when a Child's process ends (other children: a sleep)."""
    f = getattr(child, "nap", None)
    if f is not None:
        f(seconds)
    else:
        time.sleep(seconds)


def cancel_requested(engine):
    global CANCEL_FLAG
    if CANCEL_FLAG:
        return True
    try:
        return bool(engine.test_break())
    except Exception:
        return False


def progress_text(child, frames, done_n, current):
    """('Frame 3/100', fraction) from what the picture files show."""
    total = len(frames) if frames else 0
    elapsed = time.time() - child.t0
    if hasattr(child, "chunks") and total:
        return "%d/%d frames done (%d processes), %.1f s" % (done_n, total, len(child.run), elapsed), min(1.0, done_n / total)
    if not total:
        return "%d frames done, %.0f s" % (done_n, elapsed), 0.0
    inside = 0.0
    if done_n and current in child.starts:
        avg = (time.time() - child.t0) / max(1, done_n)
        inside = min(0.95, (time.time() - child.starts[current]) / avg) if avg > 0 else 0.0
    return "frame %d/%d, %.1f s" % (min(done_n + 1, total), total, elapsed), min(1.0, (done_n + inside) / total)


def wait_for(engine, child, want, frames=None, poll=0.05):
    """Block until the picture of frame `want` is complete (path), the user cancels (None) or the
    child ends without it (SessionError).  Reports progress and status while it waits."""
    last = 0
    while True:
        rdy = child.ready()
        if want in rdy:
            return rdy[want]
        if child.exited():
            rdy = child.ready()
            if want in rdy:
                return rdy[want]
            if child.cancelled:
                return None
            have = sorted(child.files())
            if out_of_memory(child.log_tail(4000)):
                raise SessionError(memory_message(getattr(child, "mem_kb", 0) or memory_kb(), getattr(child, "size", None)))
            raise SessionError("the renderer ended (exit %s) without a picture for frame %d (it wrote %s)%s" % (
                child.returncode(), want, have or "none", ("\n" + child.log_tail()) if child.log_tail() else ""))
        if cancel_requested(engine):
            child.cancel()
            return None
        files = child.files()
        text, frac = progress_text(child, frames, len(rdy), max(files) if files else None)
        if time.time() - last > 0.1:
            engine.update_stats("Fogle3D", text)
            engine.update_progress(frac)
            LOG.append((time.time(), text, frac))
            last = time.time()
        nap(child, poll)


# ----------------------------------------------------------------------------- compatible blocking run
def supervised_run_render_frames(engine):
    """A drop-in for runner.run_render_frames(st, installed, only, extra_env, out_dir) that polls the
    child, shows progress and honours Esc.  Used for the 'This Blender scene' source, whose code
    (export.py) calls runner.run_render_frames."""
    def run(st, installed, only, extra_env, out_dir, size=None):
        import bpy
        exe = renderer_path(st, bpy.path.abspath)
        bad = problems(st, bpy.path.abspath)
        if bad:
            raise SessionError(bad[0])
        thr = counts(1, False)[1]
        child = Child(exe, installed, only, extra_env, out_dir, thr, memory_kb(st, size, thr), size)
        while not child.exited():
            if cancel_requested(engine):
                child.cancel()
                raise SessionError("cancelled")
            f = child.files()
            engine.update_stats("Fogle3D", "rendering, %d pictures, %.1f s" % (len(f), time.time() - child.t0))
            child.nap(0.05)
        if child.returncode() != 0:
            check_memory(child.log_path, child.mem_kb, size)
        return child.log_path, child.returncode()
    return run


# ----------------------------------------------------------------------------- range runs
_runs = {}          # key -> RangeRun (one live at a time)
_temps = []


def new_dir(keep=""):
    if keep:
        os.makedirs(keep, exist_ok=True)
        return tempfile.mkdtemp(prefix="si3d-", dir=keep)
    d = tempfile.mkdtemp(prefix="si3d-")
    _temps.append(d)
    return d


def shutdown():
    """Stop every child still running (by PID) and remove temporary directories."""
    for run in list(_runs.values()):
        run.child.cancel(grace=1.0)
    _runs.clear()
    for d in _temps:
        shutil.rmtree(d, ignore_errors=True)
    _temps.clear()


class RangeRun:
    def __init__(self, child, frames, work):
        self.child, self.frames, self.work = child, frames, work


def cancel_runs():
    """Stop the live range run's child (process group, by PID).  A cancel that comes between two frames of an
    animation finds no wait_for running: the next frame's render sees it and stops the child here."""
    for run in list(_runs.values()):
        if not run.child.cancelled:
            run.child.cancel()


def get_run(engine, key, make):
    """The live run for `key`; a run for another key is stopped first (by PID)."""
    run = _runs.get(key)
    if run is not None and not run.child.cancelled and (not run.child.exited() or run.child.ready()):
        return run
    for k, old in list(_runs.items()):
        old.child.cancel(grace=1.0)
        shutil.rmtree(old.work, ignore_errors=True) if old.work in _temps else None
        _runs.pop(k)
    run = make()
    _runs[key] = run
    return run
