# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless tests of unit blender2: Z pass (Depth), parallel animation runs, process-group cancel, preferences.
Run through re/tools/blender2/run_blender2.sh. Prints 'TEST <name> PASS|FAIL ...' and 'RESULT passed N of M'."""
import glob
import hashlib
import importlib.util
import os
import struct
import subprocess
import sys
import time

import bpy

sys.stdout.reconfigure(line_buffering=True)
sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
import softimage3d  # noqa: E402
from softimage3d import parallel, session, sipic  # noqa: E402

os.environ["SI3D_KEEP_DELIVERY"] = "1"
softimage3d.register()
OUT = os.environ["SI3D_OUT"]
EXE = os.environ["SI3D_RENDER_FRAMES"]
INST = os.environ["SI3D_INSTALLED"]
os.makedirs(OUT, exist_ok=True)
results = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print("TEST %-40s %s %s" % (name, "PASS" if ok else "FAIL", detail))


def setup(scene_name, w, h):
    sc = bpy.context.scene
    sc.render.engine = "FOGLE3D"
    sc.si3d.colour = "RAW"
    session.set_tool_scene(sc, "/SCENES/%s.dsc" % scene_name)   # tool-only direct .dsc render
    sc.si3d.frame_mode = "RANGE"
    sc.si3d.alpha_pass = False
    sc.view_settings.view_transform = "Raw"
    sc.render.use_border = False
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = w, h, 100
    sc.view_layers[0].use_pass_z = False
    session.shutdown()
    softimage3d._DELIVERED.clear()
    return sc


def direct(only, out, env=None):
    """The renderer run directly (the CLI), one process, whole range or RENDER_FRAME."""
    e = dict(os.environ, RENSETUP_ONLY=only, RENSETUP_PICTURES=out)
    e.update(env or {})
    subprocess.run(["/bin/sh", "-c", 'ulimit -v 2000000; exec "$0" "$1"', EXE, INST], env=e,
                   stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, check=True)


def sha(path):
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()


# --------------------------------------------------------------------------- Z pass
LEAVES = "Leaves.1-0"
sc = setup(LEAVES, 320, 240)
sc.si3d.frame_mode = "ALONE"
sc.view_layers[0].use_pass_z = True
sc.frame_set(1)
ref = os.path.join(OUT, "cli_leaves")
direct("/SCENES/%s.dsc" % LEAVES, ref, {"RENDER_FRAME": "1"})
zref = parallel.pictures(ref, ".Zpic")
check("z: renderer wrote a Zpic beside the picture", len(zref) == 1 and os.path.isfile(zref[0][:-5] + ".pic"), zref)
raw = open(zref[0], "rb").read()
want = struct.unpack(">%df" % (len(raw) // 4), raw)
bpy.ops.render.render(write_still=False)
d = softimage3d._DELIVERED[-1]
check("z: Depth pass registered", "Depth" in d["passes"], d["passes"])
rows = 240
got = d["depth"]
exp = []
for oy in range(240):                        # Blender rows run bottom to top
    sy = 239 - oy
    for ox in range(320):
        z = want[sy * 320 + ox]
        exp.append(1e10 if z <= -99999.0 else -z)
nz = sum(1 for z in got if z < 1e9)
check("z: Depth = -Zpic (float, 1e10 background)", len(got) == 76800 and all(abs(a - b) <= 1e-6 * max(1.0, abs(b)) or a == b for a, b in zip(got, exp)) and nz > 1000,
      "%d pixels, %d with a depth, nearest %.4f" % (len(got), nz, min(got)))
# picture unchanged by the pass
pic = parallel.pictures(ref)[0]
pw, ph, planes = sipic.decode(open(pic, "rb").read())

# a scene that has Z_CHANNEL NO: the add-on switches it on in an overlay copy of the set-up; the picture stays the same
SC = "IK-PageTurn.1-0"
sc = setup(SC, 83, 86)
sc.si3d.frame_mode = "ALONE"
sc.frame_set(173)
sc.view_layers[0].use_pass_z = False
bpy.ops.render.render(write_still=False)
plain = list(softimage3d._DELIVERED[-1]["rgba"])
sc.view_layers[0].use_pass_z = True
softimage3d._DELIVERED.clear()
session.shutdown()
bpy.ops.render.render(write_still=False)
dz = softimage3d._DELIVERED[-1]
nz = sum(1 for z in dz["depth"] if z < 1e9)
check("z: Z_CHANNEL NO scene gets a depth", "Depth" in dz["passes"] and nz > 100, "%d pixels with a depth, nearest %.3f" % (nz, min(dz["depth"])))
check("z: picture identical with the pass on", dz["rgba"] == plain, "")
sc.view_layers[0].use_pass_z = False

# Z pass of a render region (Blender border): the depth file has the full frame's width; coverage must match the full-frame depth
sc = setup("MODEL-Gears.1-0", 320, 240)
sc.si3d.frame_mode = "ALONE"
sc.frame_set(1)
sc.view_layers[0].use_pass_z = True
bpy.ops.render.render(write_still=False)
full = softimage3d._DELIVERED[-1]["depth"]
session.shutdown()
sc.render.use_border = True
sc.render.border_min_x, sc.render.border_max_x, sc.render.border_min_y, sc.render.border_max_y = 0.2, 0.7, 0.3, 0.8
bpy.ops.render.render(write_still=False)
reg = softimage3d._DELIVERED[-1]
bx, by, bw, bh = reg["box"]
bad = 0
for y in range(bh):
    for x in range(bw):
        a = reg["depth"][y * bw + x] if reg["depth"] else None
        b = full[(by + y) * 320 + bx + x]
        bad += (a is None) or ((a < 1e9) != (b < 1e9))
check("z: border region depth coverage = full frame's", reg["depth"] is not None and bad <= 0.02 * bw * bh, "box %s, %d of %d pixels differ in coverage" % (reg["box"], bad, bw * bh))
sc.render.use_border = False
sc.view_layers[0].use_pass_z = False

# --------------------------------------------------------------------------- parallel: byte identity, K=8 against K=1
def run_to_end(child, limit=900):
    t0 = time.time()
    while not child.exited():
        time.sleep(0.05)
        if time.time() - t0 > limit:
            child.cancel()
            raise RuntimeError("timeout")
    return time.time() - t0


def hashes(d):
    out = {}
    for p in parallel.pictures(d) + parallel.pictures(d, ".Zpic"):
        out[os.path.basename(p)] = sha(p)
    return out


SCENES = os.environ.get("SI3D_PAR_SCENES", "MODEL-Gears.1-0,Label-Bottle.1-0,Glue-SplineSurf.1-0").split(",")
CONFIGS = [(1, 1), (8, 1), (1, 8), (8, 2)]
timing = []
for name in SCENES:
    only = "/SCENES/%s.dsc" % name
    dsc = session.find_scene(INST, only)
    info = session.scene_info(dsc)
    frames = session.scene_frames(info)
    why = parallel.stateful_reasons(dsc)
    check("par: %s is splittable" % name, not why, "frames %s, state: %s" % (len(frames), sorted(why)))
    base = None
    for k, t in CONFIGS:
        os.environ["SI3D_PROCESSES"], os.environ["SI3D_BAND_THREADS"] = str(k), str(t)
        out = os.path.join(OUT, "par_%s_k%d_t%d" % (name, k, t))
        child = session.start_run(EXE, INST, only, {}, out, frames, True, dsc, frames)
        kind = type(child).__name__
        wall = run_to_end(child)
        h = hashes(out)
        if base is None:
            # the reference: the plain renderer, one process, one thread (not the add-on)
            ref = os.path.join(OUT, "par_%s_ref" % name)
            t0 = time.time()
            direct(only, ref)
            rwall = time.time() - t0
            base = hashes(ref)
            timing.append((name, "direct", 1, 1, rwall, len(base), True))
        same = h == base
        check("par: %s K=%d T=%d (%s) identical to direct" % (name, k, t, kind), same and child.returncode() == 0,
              "%d files, %.1f s, %s" % (len(h), wall, "identical" if same else "DIFFERENT (%d vs %d files)" % (len(h), len(base))))
        timing.append((name, kind, k, t, wall, len(h), same))
for k in ("SI3D_PROCESSES", "SI3D_BAND_THREADS"):
    os.environ.pop(k, None)
with open(os.path.join(OUT, "timing.tsv"), "w") as fh:
    fh.write("scene\tkind\tK\tT\twall_s\tfiles\tidentical\n")
    for r in timing:
        fh.write("%s\t%s\t%d\t%d\t%.2f\t%d\t%s\n" % r)

# --------------------------------------------------------------------------- stateful scene: not split
st_dsc = session.find_scene(INST, "/SCENES/Leaves.1-0.dsc")
check("par: particle scene is stateful (sequential)", bool(parallel.stateful_reasons(st_dsc)), sorted(parallel.stateful_reasons(st_dsc)))
os.environ["SI3D_PROCESSES"] = "8"
c = session.start_run(EXE, INST, "/SCENES/Leaves.1-0.dsc", {}, os.path.join(OUT, "leaves_seq"), list(range(1, 401)), True, st_dsc, list(range(1, 401)))
check("par: stateful scene -> one process", type(c).__name__ == "Child" and c.sequential, type(c).__name__)
c.cancel()
os.environ.pop("SI3D_PROCESSES", None)

# --------------------------------------------------------------------------- cancel: every process group of a K=8 run
name = "Label-Bottle.1-0"
only = "/SCENES/%s.dsc" % name
dsc = session.find_scene(INST, only)
frames = session.scene_frames(session.scene_info(dsc))
os.environ["SI3D_PROCESSES"] = "4"
child = session.start_run(EXE, INST, only, {}, os.path.join(OUT, "cancel_par"), frames, True, dsc, frames)
t0 = time.time()
while not child.files() and time.time() - t0 < 30:
    time.sleep(0.05)
pids = [p.pid for p in child.run.values()]
alive_before = sum(os.path.exists("/proc/%d" % p) for p in pids)
t0 = time.time()
child.cancel()
dt = time.time() - t0


def group_members(pgids):
    out = []
    for d in os.listdir("/proc"):
        if d.isdigit():
            try:
                f = open("/proc/%s/stat" % d).read().rsplit(")", 1)[1].split()
                if int(f[2]) in pgids and f[0] != "Z":
                    out.append(int(d))
            except (OSError, IndexError, ValueError):
                pass
    return out


left = group_members(set(pids))
n_done = len(child.files())
check("cancel: K=4 run had %d processes" % len(pids), alive_before >= 2, "alive %d" % alive_before)
check("cancel: all process groups gone", not left and dt < 5, "%.2f s, left %s" % (dt, left))
check("cancel: partial run", 0 < n_done < len(frames), "%d of %d pictures" % (n_done, len(frames)))
os.environ.pop("SI3D_PROCESSES", None)

# --------------------------------------------------------------------------- counts
os.environ.pop("SI3D_PROCESSES", None)
os.environ.pop("SI3D_BAND_THREADS", None)
cores = parallel.usable_cores()
k, t = session.counts(100, True)
check("counts: animation default = cores-1 (max 16) processes, 1 thread", k == min(16, max(1, cores - 1)) and t == 1, "cores %d -> %d, %d" % (cores, k, t))
k, t = session.counts(100, False)
check("counts: still default = 1 process, cores-1 threads", k == 1 and t == min(parallel.MAX_BAND_THREADS, max(1, cores - 1)), "%d, %d" % (k, t))
check("counts: processes never above 16", parallel.auto_count(64) == 16 and session.counts(3, True)[0] == 3, "")
check("counts: a still's threads up to cores-1", parallel.auto_count(32, cap=parallel.MAX_BAND_THREADS) == 31, "")

print("RESULT passed %d of %d" % (sum(results), len(results)))
softimage3d.unregister()
sys.exit(0 if all(results) else 1)
