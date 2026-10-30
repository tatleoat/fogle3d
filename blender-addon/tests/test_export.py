# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless test: the default Blender scene (cube, light, camera) plus a NURBS surface, a metaball
and a text object read out through softimage3d.export.scene_to_dict."""
import os, sys
import bpy
sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
import softimage3d
from softimage3d import export
softimage3d.register()
bpy.ops.surface.primitive_nurbs_surface_sphere_add()
bpy.ops.object.metaball_add(type="BALL")
bpy.ops.object.text_add()
bpy.ops.object.light_add(type="SPOT")
dg = bpy.context.evaluated_depsgraph_get()
d = export.scene_to_dict(dg)
kinds = sorted((o["type"], len(o["verts"]), len(o["polys"])) for o in d["objects"])
print("TEST export objects", kinds)
print("TEST export lights", [(l["kind"], l["shadow"]) for l in d["lights"]])
print("TEST export camera", d["camera"]["type"], round(d["camera"]["lens"], 1), "range", d["frame_start"], d["frame_end"], "res", d["resolution"])
print("TEST export materials", list(d["materials"]), "skipped", d["skipped"])
ok = {t for t, _, _ in kinds} >= {"MESH", "SURFACE"} and len(d["lights"]) == 2 and d["camera"]
print("RESULT export", "ok" if ok else "FAILED")
sc = bpy.context.scene; sc.render.engine = "FOGLE3D"
try:
    bpy.ops.render.render(write_still=False)
except Exception as e:
    print("TEST engine BLENDER source:", str(e)[:140])
softimage3d.unregister()
sys.exit(0 if ok else 1)
