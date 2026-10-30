# SPDX-License-Identifier: GPL-3.0-or-later
"""Release model test (unit pkg1), headless: the add-on needs the user's install path.
Run through re/tools/pkg/run_install_test.sh. Cases: nothing set, wrong path, empty directory, correct path through
the add-on PREFERENCE alone (no scene field, no SI_INSTALL, no SI3D_INSTALLED) -> the render's .pic files are
byte-identical to a direct render-frames run's. Unit rtb (run-time data): an install without the program file
(Creative-Tools/3D/bin/soft) or with another build is refused by name; and a render whose root is not the install (a
render border: a scene overlay) works with render-frames copied outside the repository and Blender's working directory
outside it (no development copy to fall back on), because the add-on passes SI_INSTALL; the same program started
without it exits with status 2 and names SI_INSTALL."""
import os
import shutil
import subprocess
import sys
import tempfile

import addon_utils
import bpy

sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
ok_all = True


def check(name, ok, info=""):
    global ok_all
    ok_all &= bool(ok)
    print("TEST %-52s %s %s" % (name, "ok" if ok else "FAIL", "" if ok else info))


m = addon_utils.enable("softimage3d", default_set=True, persistent=False)
print("ENABLE", m, list(bpy.context.preferences.addons.keys()))
import softimage3d  # noqa: E402
from softimage3d import session  # noqa: E402

pr = bpy.context.preferences.addons["softimage3d"].preferences
sc = bpy.context.scene
sc.render.engine = "FOGLE3D"
sc.si3d.colour = "RAW"
sc.view_settings.view_transform = "Raw"
session.set_tool_scene(sc, os.environ["SI3D_SCENE"])      # tool-only direct .dsc render
sc.si3d.renderer = os.environ["SI3D_RENDER_FRAMES"]
sc.si3d.installed = ""                       # the scene field is empty: only SI_INSTALL or the preference count
good = os.environ["PKG_GOOD_INSTALL"]
for k in ("SI_INSTALL", "SI3D_INSTALLED"):
    os.environ.pop(k, None)


def run():
    try:
        bpy.ops.render.render(write_still=False)
    except RuntimeError as e:
        return str(e)
    return None


pr.install_path = ""
bad = session.problems(sc.si3d, bpy.path.abspath)
check("nothing set: problem names the preference", any("install path" in b and "not set" in b for b in bad), bad)
e = run()
check("nothing set: render stops with that text", e is not None and "not set" in e, repr(e))

pr.install_path = "/nonexistent/softimage"
bad = session.problems(sc.si3d, bpy.path.abspath)
check("wrong path: not a directory", any("not a directory" in b for b in bad), bad)
e = run()
check("wrong path: render stops with that text", e is not None and "not a directory" in e, repr(e))

empty = os.path.join(os.environ["SI3D_OUT"], "empty-install")
os.makedirs(empty, exist_ok=True)
pr.install_path = empty
bad = session.problems(sc.si3d, bpy.path.abspath)
check("empty directory: lists Relations.rsrc and noIcon.pic", any("Relations.rsrc" in b for b in bad) and any("noIcon.pic" in b for b in bad), bad)
e = run()
check("empty directory: render stops listing them", e is not None and "Relations.rsrc" in e, repr(e))

# an install with the two resource files but no program file, then with a file of another size in its place
from softimage3d import install  # noqa: E402
info, _ = install.check(good, install.RENDER)
part = os.path.join(os.environ["SI3D_OUT"], "no-program")
os.makedirs(os.path.join(part, "Creative-Rsrc", "3D", "rsrc"), exist_ok=True)
for n in ("Relations.rsrc", "noIcon.pic"):
    shutil.copy(os.path.join(info["rsrc_dir"], n), os.path.join(part, "Creative-Rsrc", "3D", "rsrc", n))
pr.install_path = part
bad = session.problems(sc.si3d, bpy.path.abspath)
check("no program file: problem names Creative-Tools/3D/bin/soft", any("Creative-Tools/3D/bin/soft" in b for b in bad), bad)
e = run()
check("no program file: render stops naming it", e is not None and "Creative-Tools/3D/bin/soft" in e, repr(e))
os.makedirs(os.path.join(part, "Creative-Tools", "3D", "bin"), exist_ok=True)
with open(os.path.join(part, "Creative-Tools", "3D", "bin", "soft"), "wb") as fh:
    fh.write(b"another build" * 100)
bad = session.problems(sc.si3d, bpy.path.abspath)
check("another build: problem says so", any("not the Softimage|3D 4.0 Linux build" in b and "expected 8472833" in b for b in bad), bad)

pr.install_path = good
check("correct path: no install problem", not [b for b in session.problems(sc.si3d, bpy.path.abspath) if "install" in b.lower()], "")
keep = os.path.join(os.environ["SI3D_OUT"], "kept")
os.makedirs(keep, exist_ok=True)
sc.si3d.keep_dir = keep
sc.si3d.frame_mode = "RANGE"
sc.frame_set(174)
sc.render.image_settings.file_format = "OPEN_EXR"
sc.render.filepath = os.path.join(os.environ["SI3D_OUT"], "inst_still.exr")
e = run()
check("correct path: render runs", e is None, e)
for r in list(session._runs.values()):       # the range run (173..175) goes on after frame 174 is delivered: let it end
    r.child.proc.wait(timeout=120)
ref = {}
for r, _, fs in os.walk(os.environ["SI3D_REF_DIR"]):
    for f in fs:
        if f.endswith(".pic"):
            ref[f] = open(os.path.join(r, f), "rb").read()
got = {}
for r, _, fs in os.walk(keep):
    for f in fs:
        if f.endswith(".pic"):
            got[f] = open(os.path.join(r, f), "rb").read()
same = sum(1 for k, v in ref.items() if got.get(k) == v)
check("correct path: pictures byte-identical to direct run", ref and same == len(ref) == len(got), "%d of %d identical, %d produced" % (same, len(ref), len(got)))
print("RESULT pictures identical %d of %d" % (same, len(ref)))

# the program file through SI_INSTALL: render-frames and the working directory outside the repository, a render border
# (the root is then a scene overlay without the program file)
outside = tempfile.mkdtemp(prefix="si3d-rtb-", dir=os.environ.get("SI3D_OUTSIDE") or None)
try:
    exe = os.path.join(outside, "render-frames")
    shutil.copy2(os.environ["SI3D_RENDER_FRAMES"], exe)
    here = os.getcwd()
    os.chdir(outside)
    sc.si3d.renderer = exe
    sc.si3d.frame_mode = "ALONE"
    sc.render.use_border, sc.render.use_crop_to_border = True, False
    sc.render.border_min_x, sc.render.border_max_x, sc.render.border_min_y, sc.render.border_max_y = 0.25, 0.75, 0.25, 0.75
    session.shutdown()
    e = run()
    check("program file through SI_INSTALL: border render (overlay root) runs", e is None, e)
    ov = [os.path.join(r, d) for r, ds, _ in os.walk(keep) for d in ds if d == "overlay"]
    if ov:
        env = dict(os.environ, RENSETUP_ONLY="/SCENES/", RENSETUP_PICTURES=os.path.join(outside, "pics"), RENDER_FIRST_ONLY="1")
        env.pop("SI_INSTALL", None)
        p = subprocess.run(["/bin/sh", "-c", 'ulimit -v 2000000; exec "$0" "$1"', exe, ov[0]], env=env, cwd=outside,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=600)
        text = p.stdout.decode("utf-8", "replace")
        check("without SI_INSTALL: the same program on the overlay exits 2 naming SI_INSTALL", p.returncode == 2 and "SI_INSTALL" in text,
              "%d %s" % (p.returncode, text[-300:]))
    else:
        check("an overlay was made for the border render", False, "no overlay directory under %s" % keep)
    os.chdir(here)
finally:
    session.shutdown()
    shutil.rmtree(outside, ignore_errors=True)       # our own temporary directory (absolute path from mkdtemp)
sys.exit(0 if ok_all else 1)
