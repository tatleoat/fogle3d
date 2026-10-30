# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless tests of the engine's quality-of-life features (run through re/tools/blenderqol1/run_qol.sh):
progress, cancel by PID, region, Alpha pass, multi-frame arrival, error panel, per-scene settings.
Prints 'TEST <name> PASS|FAIL ...' lines and 'RESULT passed N of M'."""
import glob
import importlib.util
import os
import subprocess
import sys
import threading
import time

import bpy

sys.stdout.reconfigure(line_buffering=True)

sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
spec = importlib.util.spec_from_file_location("sipic_ref", os.environ["SI3D_SIPIC_REF"])
sipic_ref = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sipic_ref)
import softimage3d  # noqa: E402
from softimage3d import session  # noqa: E402

os.environ["SI3D_KEEP_DELIVERY"] = "1"
softimage3d.register()
OUT = os.environ["SI3D_OUT"]
os.makedirs(OUT, exist_ok=True)
results = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print("TEST %-36s %s %s" % (name, "PASS" if ok else "FAIL", detail))


def setup(scene_name, w, h):
    sc = bpy.context.scene
    sc.render.engine = "FOGLE3D"
    sc.si3d.colour = "RAW"
    session.set_tool_scene(sc, "/SCENES/%s.dsc" % scene_name)   # tool-only direct .dsc render
    sc.si3d.frame_mode = "RANGE"
    sc.si3d.alpha_pass = False
    sc.view_settings.view_transform = "Raw"
    sc.render.dither_intensity = 0.0
    sc.render.use_border = False
    sc.render.use_crop_to_border = False
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = w, h, 100
    session.shutdown()
    softimage3d._DELIVERED.clear()
    session.LOG.clear()
    session.CANCEL_FLAG = False
    return sc


def ref_pic(path):
    info, planes, _ = sipic_ref.decode(open(path, "rb").read())
    return info["width"], info["height"], planes


def exr_write(sc, name, multilayer=False):
    sc.render.image_settings.file_format = "OPEN_EXR_MULTILAYER" if multilayer else "OPEN_EXR"
    sc.render.image_settings.color_depth = "32"
    sc.render.filepath = os.path.join(OUT, name)
    return bpy.ops.render.render(write_still=True)


def pixels(path):
    img = bpy.data.images.load(path)
    img.colorspace_settings.name = "Non-Color"
    return tuple(img.size), list(img.pixels[:])


def direct_pics(only, frame_env=None):
    """The renderer run directly (the CLI): {frame: (w, h, planes)}."""
    d = os.path.join(OUT, "cli_" + only.strip("/").replace("/", "_") + "_" + "_".join((frame_env or {}).values()))
    env = dict(os.environ, RENSETUP_ONLY=only, RENSETUP_PICTURES=d)
    env.update(frame_env or {})
    subprocess.run(["/bin/sh", "-c", 'ulimit -v 2000000; exec "$0" "$1"', os.environ["SI3D_RENDER_FRAMES"], os.environ["SI3D_INSTALLED"]],
                   env=env, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, check=True)
    out = {}
    for p in glob.glob(os.path.join(d, "*", "*.pic")):
        out[int(os.path.basename(p).rsplit(".", 2)[1])] = ref_pic(p)
    return out


def diff_count(px, ref):
    w, h, planes = ref
    d = 0
    for y in range(h):
        for x in range(w):
            i = ((h - 1 - y) * w + x) * 4
            j = y * w + x
            want = (planes["R"][j], planes["G"][j], planes["B"][j], planes["A"][j] if "A" in planes else 255)
            got = tuple(int(round(px[i + c] * 255)) for c in range(4))
            d += got != want
    return d


# ------------------------------------------------------------------ progress + multi-frame arrival
SC = "IK-PageTurn.1-0"
sc = setup(SC, 83, 86)                      # this scene has its own 83x86 sub-region
cli = direct_pics("/SCENES/%s.dsc" % SC)
first, last = min(cli), max(cli)
n = last - first + 1
session.set_tool_scene(sc, "/SCENES/%s.dsc" % SC, "START")
sc.frame_start, sc.frame_end = 1, n
sc.render.image_settings.file_format = "OPEN_EXR"
sc.render.image_settings.color_depth = "32"
sc.render.filepath = os.path.join(OUT, "anim_")
bpy.ops.render.render(animation=True)
dl = list(softimage3d._DELIVERED)
check("multi-frame: frames delivered", len(dl) == n, "%d of %d" % (len(dl), n))
tot = 0
for k in range(n):
    size, px = pixels(os.path.join(OUT, "anim_%04d.exr" % (k + 1)))
    tot += diff_count(px, cli[first + k])
check("multi-frame: pixels vs CLI run", tot == 0, "%d pixels differ over %d frames (Blender 1..%d = scene %d..%d)" % (tot, n, n, first, last))
check("multi-frame: first arrived while child ran", dl and dl[0]["child_running"], "child_running=%s" % (dl[0]["child_running"] if dl else None))
fr = [f for (_, _, f) in session.LOG]
txt = {t for (_, t, _) in session.LOG}
check("progress: updates, fraction rises", len(fr) >= 2 and max(fr) > min(fr), "%d updates, fractions %s" % (len(fr), ["%.2f" % x for x in fr[:8]]))
check("progress: 'frame k/N' text", any("frame" in t and "/%d" % n in t for t in txt), sorted(txt)[:3])

# ------------------------------------------------------------------ alpha pass
sc = setup(SC, 83, 86)
session.set_tool_scene(sc, "/SCENES/%s.dsc" % SC, "START")
sc.frame_start = sc.frame_end = 1
sc.frame_set(1)
sc.si3d.alpha_pass = True
exr_write(sc, "alpha.exr")
d = softimage3d._DELIVERED[-1]
w, h, planes = cli[first]
want = [planes["A"][(h - 1 - y) * w + x] / 255.0 for y in range(h) for x in range(w)]
check("alpha pass: values = picture A", d["alpha"] == want and "Alpha" in d["passes"], "passes %s" % d["passes"])

sc.si3d.alpha_pass = False
exr_write(sc, "noalpha.exr")
check("alpha pass: off -> Combined only", softimage3d._DELIVERED[-1]["passes"] == ["Combined"], softimage3d._DELIVERED[-1]["passes"])

# ------------------------------------------------------------------ region
TP = "ZIP_PATCH-teapot.1-0"
sc = setup(TP, 500, 424)
sc.frame_start = sc.frame_end = 1
sc.frame_set(1)
full = direct_pics("/SCENES/%s.dsc" % TP, {"RENDER_FRAME": "1"})[1]
sc.render.use_border = True
sc.render.use_crop_to_border = True
sc.render.border_min_x, sc.render.border_max_x = 0.2, 0.7
sc.render.border_min_y, sc.render.border_max_y = 0.4, 0.9
sc.si3d.frame_mode = "ALONE"
exr_write(sc, "region.exr")
size, px = pixels(os.path.join(OUT, "region.exr"))
box = softimage3d._DELIVERED[-1]["box"]
check("region: result size = border box", tuple(size) == (250, 212) and tuple(box[2:]) == (250, 212), "image %s box %s" % (tuple(size), box))
W, H, fp = full
x0, y0, bw, bh = box
top = H - (y0 + bh)
best = None
for dy in (-1, 0, 1):
    n_diff = 0
    for yy in range(bh):
        for xx in range(bw):
            i = ((bh - 1 - yy) * bw + xx) * 4
            j = (top + dy + yy) * W + x0 + xx
            want = (fp["R"][j], fp["G"][j], fp["B"][j], fp["A"][j])
            got = tuple(int(round(px[i + c] * 255)) for c in range(4))
            n_diff += got != want
    print("  region offset dy=%d: %d of %d pixels differ" % (dy, n_diff, bw * bh))
    best = n_diff if best is None else min(best, n_diff)
check("region: pixels vs cropped full run", best <= 0.02 * bw * bh, "%d of %d differ (%.2f%%; the original's own edge pixels)" % (best, bw * bh, 100.0 * best / (bw * bh)))
sc.render.use_crop_to_border = False
exr_write(sc, "region_full.exr")
size2, _ = pixels(os.path.join(OUT, "region_full.exr"))
check("region: uncropped keeps frame size", tuple(size2) == (500, 424), "image %s" % (tuple(size2),))

# ------------------------------------------------------------------ cancel by PID
sc = setup(TP, 500, 424)
sc.frame_start, sc.frame_end = 1, 100
sc.frame_set(100)                            # the child must render 1..100 before it has frame 100
os.environ["SI3D_PROCESSES"] = "1"           # one process for the whole range (unit blender2: K processes are tested in test_par.py)
threading.Timer(3.0, lambda: setattr(session, "CANCEL_FLAG", True)).start()
t0 = time.time()
sc.render.image_settings.file_format = "OPEN_EXR"
sc.render.filepath = os.path.join(OUT, "cancel_")
bpy.ops.render.render(animation=True)
del os.environ["SI3D_PROCESSES"]
dt = time.time() - t0
pid = session.LAST_PID
gone = (not os.path.exists("/proc/%d" % pid)) or open("/proc/%d/stat" % pid).read().split()[2] == "Z"
run = list(session._runs.values())[0]
nfiles = len(run.child.files())
check("cancel: returned quickly", dt < 8.0, "%.1f s" % dt)
check("cancel: child stopped by PID", gone and run.child.cancelled and run.child.proc.poll() is not None, "pid %d, exit %s" % (pid, run.child.proc.poll()))
check("cancel: partial run", 1 <= nfiles < 100, "%d of 100 pictures written" % nfiles)
check("cancel: nothing delivered after Esc", len(softimage3d._DELIVERED) <= nfiles, "%d delivered before it, %d pictures written" % (len(softimage3d._DELIVERED), nfiles))
session.CANCEL_FLAG = False

# ------------------------------------------------------------------ error panel + per-scene settings
sc = setup(SC, 83, 86)
good = sc.si3d.renderer
sc.si3d.renderer = "/nonexistent/render-frames"
bad = session.problems(sc.si3d, bpy.path.abspath)
check("errors: missing renderer text", len(bad) == 1 and "not found" in bad[0], bad)
err = None
try:
    bpy.ops.render.render(write_still=False)
except RuntimeError as e:
    err = str(e)
check("errors: render reports it", err is not None and "not found" in err, repr(err))
sc.si3d.renderer = good
inst = sc.si3d.installed
sc.si3d.installed = "/nonexistent/installed"
check("errors: missing install text", any("not a directory" in b for b in session.problems(sc.si3d, bpy.path.abspath)), "")
sc.si3d.installed = inst


class FakeLayout:
    texts = []
    alert = False

    def __getattr__(self, name):
        if name in ("box", "column", "row", "grid_flow"):
            return lambda *a, **k: self
        if name == "label":
            return lambda *a, **k: FakeLayout.texts.append(k.get("text"))
        return lambda *a, **k: None


sc.si3d.renderer = ""
lay = FakeLayout()


class Ctx:
    scene = sc


class P:
    layout = lay


softimage3d.SI3D_PT_settings.draw(P, Ctx)
check("errors: panel shows a box", any(t and "cannot render" in t for t in FakeLayout.texts) and lay.alert, FakeLayout.texts[:3])
sc.si3d.renderer = good
sc2 = bpy.data.scenes.new("other")
sc2.si3d.ray_depth = 5
sc.si3d.ray_depth = 7
sc.si3d.alpha_pass = True
path = os.path.join(OUT, "settings.blend")
bpy.ops.wm.save_as_mainfile(filepath=path)
bpy.ops.wm.open_mainfile(filepath=path)
a, b = bpy.data.scenes["Scene"].si3d, bpy.data.scenes["other"].si3d
check("settings: per scene, kept in .blend", (a.ray_depth, b.ray_depth, a.alpha_pass, b.alpha_pass) == (7, 5, True, False),
      (a.ray_depth, b.ray_depth, a.alpha_pass, b.alpha_pass))

print("RESULT passed %d of %d" % (sum(results), len(results)))
softimage3d.unregister()
sys.exit(0 if all(results) else 1)
