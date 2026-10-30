# SPDX-License-Identifier: GPL-3.0-or-later
"""The Rendered viewport of the Softimage 3D engine (our own code, unit viewport1).

Blender calls the engine's `view_update` and `view_draw` for a 3D view whose shading is 'Rendered'; both hand over to
this module.  There is no thread: each picture is made by child programs (blend2db, then render-frames), started and
watched from a bpy timer on Blender's main thread, so the interface never waits for a render.

- Camera.  In camera view: the scene's own camera, exactly as a final render (F12) exports it, at the scene's render
  resolution (smaller only when the camera frame on screen is smaller), so the full-size picture is F12's picture.
  In a free view: a camera made from the view's matrices (perspective or orthographic), at the region's size divided
  by Blender's viewport pixel size.
- Steps.  A first picture of about FIRST_PIXELS pixels (a quarter or an eighth of the size), then the full size, with
  one more step between when the last picture took longer than FULL_GOAL seconds.
- Changes.  While something keeps changing (orbit, an object moved, a setting), a first-step picture is started at
  most every THROTTLE seconds and shown when it is done.  Once nothing changed for DEBOUNCE seconds the steps run for
  the newest state; a step for a state that is no longer current is stopped (its process group, by the PID this module
  started: SIGTERM, SIGKILL after KILL_GRACE seconds) and its folder removed.
- Pixels.  A picture is handed through the engine's own `_deliver` (the F12 code) into a float buffer instead of a
  render result, so the floats are the ones a final render gives Blender, and drawn as a GPU texture.
- Tools only (session.ToolScene): the .dsc scene rendered once at its own camera and size (the frame mapped as F12 maps it),
  drawn fitted into the view; again after any change of the settings.
The viewport picture is a still: motion blur is left out; interlaced fields are woven as F12 weaves them.

The session logic (`Session`) and the camera maths are plain Python, so tests drive them with a fake clock and fake
jobs; bpy, gpu and mathutils are imported only inside the functions that need them."""

import json
import math
import os
import shutil
import signal
import subprocess
import tempfile
import time
import types
import weakref

DEBOUNCE = 0.08         # seconds without a change before the steps run for the newest state
THROTTLE = 0.25         # seconds between the first-step pictures started while things keep changing
TIMER_STEP = 0.02       # seconds between timer calls while there is work
FIRST_PIXELS = 40000    # the first step's picture has at most this many pixels (or the size / MAX_DIVISOR)
MAX_DIVISOR = 8
FULL_GOAL = 0.5         # seconds: the next step is the full size when it is expected to take no longer
KILL_GRACE = 1.0        # seconds between SIGTERM and SIGKILL for a stopped step
MIN_SIDE = 8            # blend2db's smallest picture side
NO_BLUR = ("MOTION_BLUR", "SHUTTER_SPEED", "MIN_MOVEMENT")
ATTR = "_si3d_view"     # the engine attribute that holds a viewport's ViewState

_STATES = {}            # id -> ViewState of every live viewport (shutdown() stops them)
_GPU = {}               # the shader, made once with a GPU context


# ----------------------------------------------------------------------------- camera maths (plain Python)
def matrix_rows(m):
    """A 4x4 matrix (mathutils or rows) as a tuple of 4 row tuples of floats."""
    return tuple(tuple(float(m[i][j]) for j in range(4)) for i in range(4))


def rigid_inverse(v):
    """The inverse of a rotation-and-translation matrix (Blender's view matrix): the view's world matrix, as rows."""
    out = []
    for i in range(3):
        row = [v[0][i], v[1][i], v[2][i]]
        out.append(row + [-(row[0] * v[0][3] + row[1] * v[1][3] + row[2] * v[2][3])])
    out.append([0.0, 0.0, 0.0, 1.0])
    return out


def is_ortho(window):
    return abs(window[3][3]) > 0.5


def view_camera(view, window, width, height, clip=None):
    """The camera (export.scene_to_dict's camera dict) of a free 3D view: its world matrix is the view matrix inverted;
    a perspective window matrix gives the lens on a 36 mm sensor (the wider side: blend2db fits the sensor to it) and the
    clip planes, an orthographic one the ortho scale and the clip planes.  Blender's own viewport lens is in the matrix,
    so its 'View Lens' setting and zoom are carried as the angle they show.  `clip`: the 3D view's own (clip start, clip
    end), used for a perspective view when they agree with the matrix (whose far plane a float32 matrix holds loosely)."""
    p = window
    side = p[0][0] if width >= height else p[1][1]
    cam = {"name": "View", "matrix_world": rigid_inverse(view), "sensor": 36.0}
    if is_ortho(p):
        near, far = (p[2][3] + 1.0) / p[2][2], (p[2][3] - 1.0) / p[2][2]
        # Blender clips an orthographic view from -clip_end / 2; blend2db draws it from 2000 units back
        cam.update(type="ORTHO", lens=50.0, ortho_scale=2.0 / side, clip=[max(near, -1000.0), far])
    else:
        near, far = p[2][3] / (p[2][2] - 1.0), p[2][3] / (p[2][2] + 1.0)
        if clip is not None and abs(clip[0] - near) <= 0.01 * near and abs(clip[1] - far) <= 0.01 * far:
            near, far = float(clip[0]), float(clip[1])
        cam.update(type="PERSP", lens=18.0 * side, ortho_scale=6.0, clip=[near, far])
    return cam


def full_size(width, height, pixel_size=1):
    ps = max(1, int(pixel_size))
    return max(MIN_SIDE, int(math.ceil(width / ps))), max(MIN_SIDE, int(math.ceil(height / ps)))


def camera_size(rect_w, rect_h, res_w, res_h, pixel_size=1):
    """The picture size in camera view: the scene's render size, smaller in the same proportion when the camera frame on
    screen (rect_w x rect_h pixels) is smaller than it."""
    s = min(rect_w / res_w, rect_h / res_h) / max(1, int(pixel_size))
    if s >= 1.0 - 0.5 / max(res_w, res_h):
        return res_w, res_h
    return max(MIN_SIDE, int(round(res_w * s))), max(MIN_SIDE, int(round(res_h * s)))


def first_divisor(w, h):
    d = 1
    while d < MAX_DIVISOR and (w / d) * (h / d) > FIRST_PIXELS:
        d *= 2
    return d


def step_size(w, h, d):
    if d == 1:
        return w, h
    return max(MIN_SIDE, int(round(w / d))), max(MIN_SIDE, int(round(h / d)))


def predict(times, n):
    """Seconds a picture of n pixels is expected to take, from (pixels, seconds) of the scene's pictures so far: a line
    through the two largest when there are two sizes, else the one time (the start of a process dominates small ones)."""
    if not times:
        return None
    by = {}
    for px, s in times:
        by[px] = s
    sizes = sorted(by)
    if len(sizes) == 1:
        return by[sizes[0]]
    n1, n2 = sizes[-2], sizes[-1]
    b = max(0.0, (by[n2] - by[n1]) / (n2 - n1))
    return by[n2] + b * (n - n2)


def next_divisor(d, times, n_full):
    if d <= 1:
        return None
    if d <= 2:
        return 1
    guess = predict(times, n_full)
    return 1 if guess is not None and guess <= FULL_GOAL else d // 2


def fit_rect(pic_w, pic_h, width, height):
    """The picture fitted into the region, centred, its proportions kept: (x0, y0, x1, y1) in region pixels."""
    s = min(width / pic_w, height / pic_h)
    w, h = pic_w * s, pic_h * s
    x0, y0 = (width - w) / 2.0, (height - h) / 2.0
    return x0, y0, x0 + w, y0 + h


# ----------------------------------------------------------------------------- session (plain Python)
class Target:
    """What the view should show: `key` names it (scene state, camera, size), `plan` is what a job needs, `full` the
    full picture size, `single` a target with one step only (a Softimage scene at its own size)."""

    def __init__(self, key, plan, full, single=False, scene=None):
        self.key, self.plan, self.full, self.single, self.scene = key, plan, tuple(full), single, scene
        self.failed = None


class Picture:
    def __init__(self, pixels, size, key, divisor, started, seconds, live):
        self.pixels, self.size, self.key, self.divisor = pixels, tuple(size), key, divisor
        self.started, self.seconds, self.live = started, seconds, live


class Session:
    """The pictures of one viewport: which job runs, which picture is shown.  `launch(target, divisor, size, live)`
    starts a job: an object with key, divisor, size, live, started, result, error, poll(now) (None while it runs, else
    "done", "failed" or "cancelled"), cancel(now) and close()."""

    def __init__(self, launch, clock=time.monotonic):
        self.launch, self.clock = launch, clock
        self.target = None
        self.job = None
        self.dying = []             # stopped jobs whose processes have not gone yet
        self.picture = None
        self.version = 0            # +1 for every picture shown
        self.changed_at = -1e9
        self.last_start = -1e9
        self.scene_dirty = True     # the scene changed since the last export (the first one is owed)
        self.scene_seen = None      # the scene state `times` belongs to
        self.last_export = -1e9     # when the scene was last read, and how long that took: while things change the
        self.export_cost = 0.0      # reading takes at most a fifth of the time
        self.times = []             # (pixels, seconds) of this scene state's pictures
        self.message = ""
        self.events = []            # (time, what, key, size, divisor, live, job folder) for the tests

    # changes
    def scene_changed(self, now):
        self.scene_dirty = True
        self.changed_at = now

    def wants_export(self, now):
        """True when the scene should be exported now: it changed and either nothing changed for DEBOUNCE seconds or
        a first-step picture is due (nothing runs and THROTTLE seconds have gone since the last start)."""
        if not self.scene_dirty:
            return False
        return (self.target is None or now - self.changed_at >= DEBOUNCE
                or (self.job is None and now - self.last_start >= THROTTLE
                    and now - self.last_export >= 4.0 * self.export_cost))

    def set_target(self, target, now):
        """The view should show `target` (None: nothing).  Returns True when it is new; a running step for another
        state is stopped at once (a first-step picture started during changes is left to finish)."""
        if target is None:
            if self.job is not None:
                self._cancel(now)
            changed = self.target is not None
            self.target = None
            return changed
        if self.target is not None and target.key == self.target.key:
            self.target.plan = target.plan
            return False
        self.target = target
        self.changed_at = now
        if target.scene != self.scene_seen:
            self.scene_seen, self.times = target.scene, []
        if self.job is not None and not self.job.live and self.job.key != target.key:
            self._cancel(now)
        return True

    # work
    def first_for(self, t):
        return 1 if t.single else first_divisor(*t.full)

    def next_for(self, t):
        """The divisor of the next picture of target t, or None when its full-size picture is shown."""
        p = self.picture
        if p is None or p.key != t.key:
            return self.first_for(t)
        if t.single:
            return None
        return next_divisor(p.divisor, self.times, t.full[0] * t.full[1])

    def busy(self):
        t = self.target
        return bool(self.job is not None or self.dying or self.scene_dirty
                    or (t is not None and t.failed is None and self.next_for(t) is not None))

    def tick(self, now=None):
        """Move the jobs on; returns True when a new picture is to be shown."""
        now = self.clock() if now is None else now
        shown = False
        for j in list(self.dying):
            if j.poll(now) is not None:
                j.close()
                self.dying.remove(j)
        j = self.job
        if j is not None:
            state = j.poll(now)
            if state is not None:
                self.job = None
                if state == "done":
                    secs = now - j.started
                    if j.scene == self.scene_seen:
                        self.times.append((j.size[0] * j.size[1], secs))
                    if self.picture is None or j.started >= self.picture.started:
                        self.picture = Picture(j.result, j.size, j.key, j.divisor, j.started, secs, j.live)
                        self.version += 1
                        shown = True
                        self.events.append((now, "picture", j.key, j.size, j.divisor, j.live, getattr(j, "work", None)))
                    self.message = "%dx%d in %.2f s" % (j.size[0], j.size[1], secs)
                elif state == "failed":
                    if self.target is not None and j.key == self.target.key:
                        self.target.failed = j.error
                    self.message = j.error
                    self.events.append((now, "failed", j.key, j.size, j.divisor, j.live))
                j.close()
        t = self.target
        if t is None or t.failed is not None:
            return shown
        settled = now - self.changed_at >= DEBOUNCE
        j = self.job
        if j is not None:
            if j.key != t.key:
                if settled or not j.live:
                    self._cancel(now)
            elif j.live and settled:
                j.live = False          # the change has stopped: the first-step picture is the first step
        if self.job is None and not self.scene_dirty and len(self.dying) < 2:
            d = self.next_for(t)
            if d is not None:
                if settled:
                    self._start(t, d, False, now)
                elif now - self.last_start >= THROTTLE and (self.picture is None or self.picture.key != t.key):
                    self._start(t, self.first_for(t), True, now)
        return shown

    def _start(self, t, d, live, now):
        size = t.full if t.single else step_size(t.full[0], t.full[1], d)
        job = self.launch(t, d, size, live)
        job.started = now
        job.scene = t.scene
        self.last_start = now
        self.job = job
        self.message = "rendering %dx%d" % size
        self.events.append((now, "start", t.key, size, d, live, getattr(job, "work", None)))

    def _cancel(self, now):
        j, self.job = self.job, None
        j.cancel(now)
        self.dying.append(j)
        self.events.append((now, "cancel", j.key, j.size, j.divisor, j.live, getattr(j, "work", None)))

    def close(self):
        for j in ([self.job] if self.job is not None else []) + self.dying:
            try:
                j.close(kill=True)
            except Exception:
                pass
        self.job, self.dying = None, []


# ----------------------------------------------------------------------------- jobs (child programs)
class Plan:
    """What every job of a target needs (plain values)."""

    def __init__(self, **kw):
        self.__dict__.update(kw)


class Job:
    """One picture.  Source 'Blender scene': blend2db builds the database at the step's size and camera, then
    render-frames renders the frame; tool scene: render-frames on the installed scene.  Each program runs
    in a session of its own (its process group is what a stop ends)."""

    def __init__(self, base, plan, key, divisor, size, live, keep=False):
        self.plan, self.key, self.divisor, self.size, self.live = plan, key, divisor, tuple(size), live
        self.keep = keep
        self.started = self.scene = None
        self.result = None
        self.error = ""
        self.work = tempfile.mkdtemp(prefix="job-", dir=base)
        self.stage = "new"
        self.proc = None            # blend2db
        self.child = None           # session.Child (render-frames)
        self.kill_at = None
        self.closed = False
        self.marks = [("new", time.monotonic())]      # (stage, time) for the tests' timings

    def start(self):
        try:
            if self.plan.kind == "BLENDER":
                w, h = self.size
                # the scene camera at the scene's own size: the final render's resolution words, so the database (and
                # the picture) is the final render's
                res = self.plan.res_list if (self.plan.res_list and self.size == self.plan.res) else [w, h, 100]
                jpath = os.path.join(self.work, "scene.json")
                with open(jpath, "w") as fh:
                    # the scene's JSON text (made once per export) with this job's camera and resolution added
                    fh.write(self.plan.text[:-1])
                    fh.write(', "camera": %s, "resolution": %s}' % (json.dumps(self.plan.camera), json.dumps(res)))
                self._log = open(os.path.join(self.work, "blend2db.txt"), "wb")
                self.proc = subprocess.Popen(
                    [self.plan.blend2db, "build", jpath, os.path.join(self.work, "database"), self.plan.tdb,
                     self.plan.tscene, self.plan.rsrc_dir], stdout=self._log, stderr=subprocess.STDOUT,
                    start_new_session=True)
                self.stage = "build"
                self.marks.append(("build", time.monotonic()))
            else:
                self._render(self.plan.installed, self.plan.only)
        except Exception as e:
            self.error, self.stage = "could not start: %s" % e, "failed"
        return self

    def _render(self, root, only):
        from . import install, session
        # SI_INSTALL: render-frames reads the program file from the user's install (root may be a built database)
        size = getattr(self, "size", None)
        env = dict(install.program_env(getattr(self.plan, "install", "")), RENDER_FRAME=str(self.plan.frame))
        self.child = session.Child(self.plan.exe, root, only, env, os.path.join(self.work, "pictures"), self.plan.threads,
                                   session.memory_kb(None, size, self.plan.threads), size)
        self.stage = "render"
        self.marks.append(("render", time.monotonic()))

    def _pids(self):
        out = []
        if self.proc is not None and self.proc.poll() is None:
            out.append(self.proc.pid)
        if self.child is not None and self.child.proc.poll() is None:
            out.append(self.child.pid)
        return out

    def poll(self, now):
        if self.stage == "stopping":
            pids = self._pids()
            if not pids:
                if self.child is not None:
                    self.child.exited()
                return "cancelled"
            if now >= self.kill_at:
                from . import parallel
                for pid in pids:
                    parallel.killpg(pid, signal.SIGKILL)
            return None
        if self.stage == "failed":
            return "failed"
        if self.stage == "build":
            rc = self.proc.poll()
            if rc is None:
                return None
            self._log.close()
            if rc != 0:
                self.error = "blend2db: " + _tail(os.path.join(self.work, "blend2db.txt"))
                self.stage = "failed"
                return "failed"
            try:
                self._render(os.path.join(self.work, "database"), "SCENES/blend")
            except Exception as e:
                self.error, self.stage = "could not start the renderer: %s" % e, "failed"
                return "failed"
            return None
        if self.stage == "render":
            if not self.child.exited():
                return None
            self.marks.append(("decode", time.monotonic()))
            try:
                self.result = self._decode()
            except Exception as e:
                self.error, self.stage = str(e), "failed"
                return "failed"
            self.stage = "done"
            self.marks.append(("done", time.monotonic()))
            return "done"
        return self.stage if self.stage in ("done", "cancelled") else None

    def _decode(self):
        from . import parallel, sipic
        pics = os.path.join(self.work, "pictures")
        found = sorted(parallel.pictures(pics))
        if not found:
            raise RuntimeError("the renderer wrote no picture: " + self.child.log_tail(300))
        pic = None
        if len(found) == 2:
            # interlaced fields: two half-height pictures, woven into one frame as a final render weaves them
            try:
                a, b = (open(f, "rb").read() for f in found)
                pic = sipic.weave(a, b, int(os.environ.get("SI3D_FIELD_TOP", "2")))
            except sipic.PicError:
                pic = None
        if pic is None:
            with open(found[0], "rb") as fh:
                pic = sipic.decode_np(fh.read())
        w, h, planes = pic
        out = getattr(self.plan, "out_size", None) or (w, h)
        self.size = tuple(out)
        return self.plan.deliver(w, h, planes, out)

    def cancel(self, now):
        from . import parallel
        self.kill_at = now + KILL_GRACE
        if self.stage in ("build", "render"):
            for pid in self._pids():
                parallel.killpg(pid, signal.SIGTERM)
        self.stage = "stopping"

    def close(self, kill=False):
        if self.closed:
            return
        from . import parallel
        for pid in self._pids():
            parallel.killpg(pid, signal.SIGKILL if kill else signal.SIGTERM)
        for p in ([self.proc] if self.proc is not None else []) + ([self.child.proc] if self.child is not None else []):
            try:
                p.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                parallel.killpg(p.pid, signal.SIGKILL)
                try:
                    p.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    pass
        if self.child is not None:
            self.child.exited()
        if self.proc is not None and not self._log.closed:
            self._log.close()
        self.closed = True
        if not self.keep:
            shutil.rmtree(self.work, ignore_errors=True)


def _tail(path, n=300):
    try:
        with open(path, "rb") as fh:
            return fh.read()[-n:].decode("utf-8", "replace").strip()
    except OSError:
        return ""


# ----------------------------------------------------------------------------- the picture as floats (F12's code)
class _Capture:
    """What SI3DRenderEngine._deliver calls on an engine, keeping the Combined floats instead of a render result."""

    def __init__(self):
        self.pixels = None

    def _set(self, a):
        self.pixels = a

    def begin_result(self, x, y, w, h):
        rect = types.SimpleNamespace(foreach_set=self._set)
        return types.SimpleNamespace(layers=[types.SimpleNamespace(passes={"Combined": types.SimpleNamespace(rect=rect)})])

    def end_result(self, result):
        pass


def pixels_of(w, h, planes, colour, bg, out=None):
    """The picture's float RGBA, rows bottom to top (Blender's order), (h, w, 4) float32, or resampled to `out` = (width,
    height) as a final render resamples it: exactly the floats a final render hands Blender for the same picture and
    settings."""
    from . import SI3DRenderEngine
    ow, oh = out or (w, h)
    cap = _Capture()
    SI3DRenderEngine._deliver(cap, w, h, planes, ow, oh, colour, None, False, False, bg, None)
    return cap.pixels.reshape(oh, ow, 4)


# ----------------------------------------------------------------------------- one viewport (bpy side)
class ViewState:
    """Everything one viewport engine keeps (engine._si3d_view)."""

    def __init__(self, keep_dir=""):
        base_parent = keep_dir if keep_dir else None
        if base_parent:
            os.makedirs(base_parent, exist_ok=True)
        self.base = tempfile.mkdtemp(prefix="si3d-view-", dir=base_parent)
        self.keep_jobs = bool(os.environ.get("SI3D_VIEW_KEEP"))
        self.session = Session(self._launch)
        self.desc = None            # the scene as exported: Plan-ready values and a signature
        self.desc_version = 0
        self.problem = ""
        self.timer = None
        self.timer_on = False
        self.asked = False          # an export has been asked of Blender (tag_update / tag_redraw)
        self.texture = None
        self.tex_version = -1
        self.rect = None            # where the picture is drawn, region pixels
        self.region = (1, 1)
        self.shown_status = None
        self.closed = False
        self.old_dirs = []          # texture folders of earlier exports, removed when no job reads them
        self.exports = []           # (seconds) of each export (tests)
        _STATES[id(self)] = self

    def _launch(self, target, divisor, size, live):
        return Job(self.base, target.plan, target.key, divisor, size, live, self.keep_jobs).start()

    def reap_dirs(self):
        if not self.old_dirs:
            return
        used = set()
        for j in ([self.session.job] if self.session.job else []) + self.session.dying:
            used.add(getattr(j.plan, "texdir", None))
        for d in list(self.old_dirs):
            if d not in used:
                shutil.rmtree(d, ignore_errors=True)
                self.old_dirs.remove(d)

    def close(self):
        if self.closed:
            return
        self.closed = True
        self.session.close()
        if self.timer is not None:
            try:
                import bpy
                if bpy.app.timers.is_registered(self.timer):
                    bpy.app.timers.unregister(self.timer)
            except Exception:
                pass
        self.texture = None
        if not self.keep_jobs:
            shutil.rmtree(self.base, ignore_errors=True)
        _STATES.pop(id(self), None)


def _abspath(p):
    import bpy
    return bpy.path.abspath(p) if p else p


def _state(engine, scene):
    vp = getattr(engine, ATTR, None)
    if vp is None or vp.closed:
        vp = ViewState(_abspath(scene.si3d.keep_dir))
        setattr(engine, ATTR, vp)
        weakref.finalize(engine, vp.close)      # the engine goes (shading changed, view closed): stop and clean up
    return vp


def render_options(scene, st):
    """export.render_options without motion blur (the viewport shows a still)."""
    from . import export
    opts = export.render_options(scene, st)
    setup = dict(opts.get("setup", {}))
    for k in NO_BLUR:
        setup.pop(k, None)
    return dict(opts, setup=setup)


def _describe_blender(vp, scene, depsgraph, st):
    from . import export, install, session
    installed = session.install_root(st, _abspath)
    info, bad = install.check(installed, install.BLEND2DB)
    if bad:
        raise session.SessionError(bad[0])
    texdir = tempfile.mkdtemp(prefix="tex-", dir=vp.base)
    try:
        data = export.scene_to_dict(depsgraph, texdir)
        data["frame"] = scene.frame_current
        data["render"] = render_options(scene, st)
        camera, res_list = data.pop("camera"), data.pop("resolution")
        text = json.dumps(data)     # once per export: the jobs add their camera and resolution to this text
    except Exception:
        shutil.rmtree(texdir, ignore_errors=True)
        raise
    r = scene.render
    bg = export._world_colour(scene.world) if (scene.world and not r.film_transparent) else None
    exe, b2 = session.renderer_path(st, _abspath), session.blend2db_path(st, _abspath)
    tmpl = info["template_sts"]
    sig = (text.replace(texdir, ""), json.dumps(camera), res_list, st.colour, bg, installed, exe, b2, tmpl)
    return {"kind": "BLENDER", "sig": sig, "texdir": texdir, "text": text, "camera": camera, "res_list": res_list,
            "frame": data["frame"], "bg": bg, "colour": st.colour, "install": installed,
            "exe": exe, "blend2db": b2, "rsrc_dir": os.path.dirname(info["rsrc"]),
            "tdb": os.path.dirname(os.path.dirname(tmpl)), "tscene": os.path.basename(tmpl)[:-len(".sts")],
            "res": (_b2d_side(r.resolution_x, r.resolution_percentage), _b2d_side(r.resolution_y, r.resolution_percentage)),
            "has_camera": camera is not None, "threads": session.counts(1, False)[1]}


def _b2d_side(n, pct):
    """A picture side as blend2db makes it from Blender's resolution and percentage (rounded, at least MIN_SIDE)."""
    return max(MIN_SIDE, int(math.floor(n * pct / 100.0 + 0.5)))


def scene_frame(st, info, frame, blender_start):
    """(scene frame, None) for Blender frame `frame` as a final render maps it, or (None, message)."""
    from . import session
    frames = session.scene_frames(info)
    mode = session.tool_scene(st).frame_map
    sf = session.map_frame(frame, mode, info, blender_start)
    if frames is not None and sf not in frames and mode == "SAME" and info["first"] is not None:
        mode, sf = "START", session.map_frame(frame, "START", info, blender_start)
    if frames is not None and sf not in frames:
        return None, session.frame_range_message(sf, frame, mode, info, blender_start)
    return sf, None


def _describe_softimage(scene, st):
    from . import session
    installed = session.install_root(st, _abspath)
    exe = session.renderer_path(st, _abspath)
    tool = session.tool_scene(st)
    dsc = session.find_scene(installed, tool.filter)
    info = session.scene_info(dsc)
    sf, bad = scene_frame(st, info, scene.frame_current, scene.frame_start)
    if bad:
        raise session.SessionError(bad)
    # the picture is handed over as a final render hands it: resampled to Blender's render size
    r = scene.render
    size = (max(1, int(r.resolution_x * r.resolution_percentage / 100)), max(1, int(r.resolution_y * r.resolution_percentage / 100)))
    sig = ("SCENE", dsc, installed, exe, tool.filter, sf, st.colour, size)
    return {"kind": "SCENE", "sig": sig, "texdir": None, "exe": exe, "installed": installed, "only": tool.filter,
            "frame": sf, "colour": st.colour, "bg": None, "size": size, "threads": session.counts(1, False)[1]}


def export_scene(engine, vp, scene, depsgraph, now):
    """Read the scene (main thread): a new description when anything a picture depends on changed."""
    from . import session
    st = scene.si3d
    vp.session.scene_dirty = False
    vp.asked = False
    t0 = time.monotonic()
    bad = session.problems(st, _abspath)
    if bad:
        vp.problem, vp.desc = bad[0], None
        vp.session.set_target(None, now)
        return
    try:
        desc = _describe_blender(vp, scene, depsgraph, st) if session.tool_scene(st) is None else _describe_softimage(scene, st)
    except Exception as e:
        vp.problem, vp.desc = str(e), None
        vp.session.set_target(None, now)
        return
    vp.problem = ""
    vp.exports.append(time.monotonic() - t0)
    vp.session.last_export, vp.session.export_cost = now, vp.exports[-1]
    if vp.desc is not None and vp.desc["sig"] == desc["sig"]:
        if desc["texdir"]:
            shutil.rmtree(desc["texdir"], ignore_errors=True)
        return
    if vp.desc is not None and vp.desc.get("texdir"):
        vp.old_dirs.append(vp.desc["texdir"])
    vp.desc_version += 1
    desc["version"] = vp.desc_version
    vp.desc = desc


def _deliver_for(desc):
    colour, bg = desc["colour"], desc["bg"]
    return lambda w, h, planes, out=None: pixels_of(w, h, planes, colour, bg, out)


def _target(vp, context, scene, engine):
    """The target of the current view and scene description, and the picture's place in the region."""
    desc = vp.desc
    region, rv3d = context.region, context.region_data
    width, height = int(region.width), int(region.height)
    vp.region = (width, height)
    if desc is None or rv3d is None:
        return None
    ps = engine.get_preview_pixel_size(scene)
    if desc["kind"] == "SCENE":
        w, h = desc["size"]
        plan = Plan(kind="SCENE", exe=desc["exe"], installed=desc["installed"], install=desc["installed"], only=desc["only"], frame=desc["frame"],
                    threads=desc["threads"], deliver=_deliver_for(desc), texdir=None, out_size=(w, h))
        vp.rect = fit_rect(w, h, width, height)
        return Target(("SCENE", desc["version"]), plan, (w, h), single=True, scene=desc["version"])
    view, window = matrix_rows(rv3d.view_matrix), matrix_rows(rv3d.window_matrix)
    if rv3d.view_perspective == "CAMERA" and desc["has_camera"] and scene.camera is not None:
        camera = desc["camera"]
        x0, y0, x1, y1 = camera_rect(scene, view, window, width, height)
        vp.rect = (x0, y0, x1, y1)
        rw, rh = desc["res"]
        full = camera_size(x1 - x0, y1 - y0, rw, rh, ps)
        res_list = desc["res_list"]
    else:
        res_list = None
        space = getattr(context, "space_data", None)
        clip = (space.clip_start, space.clip_end) if hasattr(space, "clip_start") else None
        camera = view_camera(view, window, width, height, clip)
        vp.rect = (0.0, 0.0, float(width), float(height))
        full = full_size(width, height, ps)
    plan = Plan(kind="BLENDER", text=desc["text"], camera=camera, res=tuple(desc["res"]), res_list=res_list, blend2db=desc["blend2db"], tdb=desc["tdb"],
                tscene=desc["tscene"], rsrc_dir=desc["rsrc_dir"], exe=desc["exe"], frame=desc["frame"], install=desc.get("install", ""),
                threads=desc["threads"], deliver=_deliver_for(desc), texdir=desc["texdir"])
    key = ("BLENDER", desc["version"], json.dumps(camera, sort_keys=True), full)
    return Target(key, plan, full, scene=desc["version"])


def camera_rect(scene, view, window, width, height):
    """The scene camera's frame in the region (x0, y0, x1, y1 pixels): its corners through the view's matrices."""
    from mathutils import Matrix, Vector
    cam = scene.camera
    m = Matrix(window) @ Matrix(view) @ cam.matrix_world
    xs, ys = [], []
    for v in cam.data.view_frame(scene=scene):
        p = m @ Vector((v.x, v.y, v.z, 1.0))
        if abs(p.w) < 1e-12:
            continue
        xs.append((p.x / p.w * 0.5 + 0.5) * width)
        ys.append((p.y / p.w * 0.5 + 0.5) * height)
    if not xs:
        return 0.0, 0.0, float(width), float(height)
    return min(xs), min(ys), max(xs), max(ys)


def _refresh(engine, vp, context, depsgraph, now):
    scene = depsgraph.scene if depsgraph is not None else context.scene
    if vp.session.wants_export(now):
        export_scene(engine, vp, scene, depsgraph, now)
        now = time.monotonic()
    if not vp.problem:
        t = _target(vp, context, scene, engine)
        if vp.session.set_target(t, now) and vp.session.job is None:
            vp.session.tick(now)        # start at once (the timer moves it on)
    _ensure_timer(engine, vp)
    _show_status(engine, vp)


def status_text(vp):
    if vp.problem:
        return "Fogle3D: " + vp.problem
    s = vp.session
    if s.job is not None:
        return "Fogle3D: rendering %dx%d" % s.job.size
    if s.target is not None and s.target.failed:
        return "Fogle3D: " + s.target.failed
    if s.picture is not None:
        return "Fogle3D: " + s.message
    return "Fogle3D: starting"


def _show_status(engine, vp):
    text = status_text(vp)
    if text != vp.shown_status:
        vp.shown_status = text
        try:
            engine.update_stats("", text)
        except Exception:
            pass


def _register_timer(fn, interval):
    import bpy
    bpy.app.timers.register(fn, first_interval=interval)


register_timer = _register_timer      # tests put their own


def _ensure_timer(engine, vp):
    if vp.timer_on or vp.closed:
        return
    vp.timer_on = True
    ref = weakref.ref(engine)

    def step():
        target = ref()
        if target is None:
            return None
        return timer_step(target)

    vp.timer = step
    register_timer(step, TIMER_STEP)


def timer_step(engine):
    """One timer call: move the jobs on, ask Blender for a redraw when a picture is new, and for an update when the
    scene is to be read again.  Returns the seconds to the next call, or None when there is nothing to do."""
    vp = getattr(engine, ATTR, None)
    if vp is None or vp.closed:
        return None
    now = time.monotonic()
    try:
        if vp.session.tick(now):
            engine.tag_redraw()
        if vp.session.wants_export(now) and not vp.asked:
            vp.asked = True
            engine.tag_update()         # Blender calls view_update (and the draw), where the scene is read
            engine.tag_redraw()
        vp.reap_dirs()
        _show_status(engine, vp)
    except ReferenceError:              # the engine has gone
        vp.close()
        return None
    s = vp.session
    if s.job is not None or s.dying:
        return TIMER_STEP
    if s.busy():
        return TIMER_STEP if not vp.asked else 0.1     # (waiting for Blender to call view_update / view_draw)
    vp.timer_on = False
    return None


def view_update(engine, context, depsgraph):
    """Blender's view_update: the scene changed (or the first call)."""
    vp = _state(engine, depsgraph.scene)
    now = time.monotonic()
    if vp.desc is None or any(True for _ in depsgraph.updates):
        vp.session.scene_changed(now)
    _refresh(engine, vp, context, depsgraph, now)


def view_draw(engine, context, depsgraph):
    """Blender's view_draw: notice a view change, then draw the newest picture (no other work)."""
    vp = _state(engine, depsgraph.scene)
    _refresh(engine, vp, context, depsgraph, time.monotonic())
    if vp.session.picture is None or vp.rect is None:
        return
    import bpy
    if bpy.app.background:
        return
    draw(vp)


def _shader():
    if "shader" not in _GPU:
        import gpu
        info = gpu.types.GPUShaderCreateInfo()
        info.sampler(0, "FLOAT_2D", "image")
        info.vertex_in(0, "VEC2", "pos")
        info.vertex_in(1, "VEC2", "texCoord")
        iface = gpu.types.GPUStageInterfaceInfo("si3d_view_iface")
        iface.smooth("VEC2", "uv")
        info.vertex_out(iface)
        info.fragment_out(0, "VEC4", "FragColor")
        info.vertex_source("void main() { uv = texCoord; gl_Position = vec4(pos, 0.0, 1.0); }")
        # nearest pixel: a smaller step's picture (or a pixel size above 1) shows as blocks, the full one pixel for pixel
        info.fragment_source(
            "void main() {"
            "  ivec2 size = textureSize(image, 0);"
            "  ivec2 p = clamp(ivec2(floor(uv * vec2(size))), ivec2(0), size - 1);"
            "  FragColor = texelFetch(image, p, 0);"
            "}")
        _GPU["shader"] = gpu.shader.create_from_info(info)
    return _GPU["shader"]


def draw(vp):
    """The picture as a texture over its place in the region (scene-linear floats; Blender's view transform follows)."""
    import gpu
    from gpu_extras.batch import batch_for_shader
    from mathutils import Matrix
    pic = vp.session.picture
    if vp.texture is None or vp.tex_version != vp.session.version:
        h, w = pic.pixels.shape[:2]
        buf = gpu.types.Buffer("FLOAT", w * h * 4, pic.pixels.reshape(-1))
        vp.texture = gpu.types.GPUTexture((w, h), format="RGBA32F", data=buf)
        vp.tex_version = vp.session.version
    width, height = vp.region
    x0, y0, x1, y1 = vp.rect
    a, b = 2.0 * x0 / width - 1.0, 2.0 * y0 / height - 1.0
    c, d = 2.0 * x1 / width - 1.0, 2.0 * y1 / height - 1.0
    shader = _shader()
    batch = batch_for_shader(shader, "TRI_FAN", {"pos": ((a, b), (c, b), (c, d), (a, d)),
                                                 "texCoord": ((0, 0), (1, 0), (1, 1), (0, 1))})
    gpu.state.depth_test_set("NONE")
    gpu.state.blend_set("ALPHA_PREMULT")
    with gpu.matrix.push_pop():
        gpu.matrix.load_matrix(Matrix.Identity(4))
        with gpu.matrix.push_pop_projection():
            gpu.matrix.load_projection_matrix(Matrix.Identity(4))
            shader.bind()
            shader.uniform_sampler("image", vp.texture)
            batch.draw(shader)
    gpu.state.blend_set("NONE")


def shutdown():
    """Stop every viewport's programs (by PID) and remove their folders (add-on unregistered)."""
    for vp in list(_STATES.values()):
        vp.close()
    _GPU.clear()
