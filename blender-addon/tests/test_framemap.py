# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless test (re/tools/framemap1/run_test.sh): Frame map fallback and error text, scene IK-PageTurn (range 173-175).
Blender frame 1 -> scene 173, 3 -> 175 (Same number falls back to Start to first), 4 -> clear error; in-range SAME unchanged."""
import importlib.util
import os
import sys

import bpy

sys.stdout.reconfigure(line_buffering=True)
sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
import softimage3d  # noqa: E402
from softimage3d import session  # noqa: E402

os.environ["SI3D_KEEP_DELIVERY"] = "1"
softimage3d.register()
OUT = os.environ["SI3D_OUT"]
results = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print("TEST %-36s %s %s" % (name, "PASS" if ok else "FAIL", detail))


# pure functions
info = {"first": 173, "last": 175, "step": 1}
m = session.frame_range_message(176, 4, "START", info, 1)
check("message: names setting + valid frames", "Frame map" in m and "1-3" in m and "Start to first" in m, m)
m = session.frame_range_message(4, 4, "SAME", info, 1)
check("message: same number valid frames", "173-175" in m and "Same number" in m, m)

sc = bpy.context.scene
sc.render.engine = "FOGLE3D"
sc.si3d.colour = "RAW"
session.set_tool_scene(sc, "/SCENES/IK-PageTurn.1-0.dsc")   # tool-only direct .dsc render
sc.si3d.frame_mode = "RANGE"
sc.view_settings.view_transform = "Raw"
sc.render.use_border = False
sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = 83, 86, 100
sc.render.image_settings.file_format = "OPEN_EXR"
sc.frame_start, sc.frame_end = 1, 3
session.shutdown()


def still(frame, mode="SAME"):
    session.set_tool_scene(sc, "/SCENES/IK-PageTurn.1-0.dsc", mode)
    sc.frame_set(frame)
    softimage3d._DELIVERED.clear()
    sc.render.filepath = os.path.join(OUT, "fm_%s_%d" % (mode, frame))
    try:
        bpy.ops.render.render(write_still=True)
        return None
    except RuntimeError as e:
        return str(e)


def px(frame, mode="SAME"):
    # a rendered picture's distinguishing content: the stored sum of the delivered Combined values
    d = softimage3d._DELIVERED[-1]
    return d.get("scene_frame")


e1 = still(1)
check("frame 1 SAME: falls back, renders", e1 is None and len(softimage3d._DELIVERED) == 1, e1)
r1 = softimage3d._DELIVERED[-1]["rgba"]
e3 = still(3)
r3 = softimage3d._DELIVERED[-1]["rgba"]
check("frame 3 SAME: falls back, renders", e3 is None and len(softimage3d._DELIVERED) == 1, e3)
e4 = still(4)
check("frame 4 SAME: clear error", e4 is not None and "Frame map" in e4 and "1-3" in e4 and "176" in e4, e4)
e4s = still(4, "START")
check("frame 4 START: clear error", e4s is not None and "Frame map" in e4s and "1-3" in e4s, e4s)
e173 = still(173)
r173 = softimage3d._DELIVERED[-1]["rgba"]
e175 = still(175)
r175 = softimage3d._DELIVERED[-1]["rgba"]
check("frame 173/175 SAME: in range, unchanged", e173 is None and e175 is None, (e173, e175))
check("fallback frame 1 = scene 173, 3 = 175", r1 == r173 and r3 == r175 and r173 != r175, (r1 == r173, r3 == r175, r173 != r175))
print("RESULT passed %d of %d" % (sum(results), len(results)))
softimage3d.unregister()
sys.exit(0 if all(results) else 1)
