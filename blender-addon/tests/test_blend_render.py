# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless test of source BLENDER (unit builder1): a scene built here (a cube, a point light, a camera,
and with SI3D_TEST=full a floor, a sphere, a spot and a sun) is exported, turned into a database by blend2db and
rendered by render-frames through bpy.ops.render.render.  Writes <SI3D_OUT>/<name>_si.png (the Softimage render) and
<SI3D_OUT>/<name>_ref.png (Blender's Workbench render of the same scene, for a human to compare) and prints
TEST/RESULT lines.  Run through re/tools/builder1/run_blender_test.sh."""
import math
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
import softimage3d  # noqa: E402

out = os.environ["SI3D_OUT"]
mode = os.environ.get("SI3D_TEST", "cube")
os.makedirs(out, exist_ok=True)
softimage3d.register()
sc = bpy.context.scene
sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = 320, 240, 100
sc.view_settings.view_transform = "Standard"
sc.render.dither_intensity = 0.0
sc.render.image_settings.file_format = "PNG"

def aim(ob, target):
    d = Vector(target) - ob.location
    ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()

def mat(name, rgb, rough=0.5, metal=0.0, alpha=1.0):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (*rgb, 1)
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    b.inputs["Alpha"].default_value = alpha
    return m

cube = bpy.data.objects["Cube"]
cam = bpy.data.objects["Camera"]
lamp = bpy.data.objects["Light"]
if mode == "cube":
    cube.data.materials.clear()
    cube.data.materials.append(mat("red", (0.8, 0.1, 0.1)))
    cube.rotation_euler = (0.3, 0.2, 0.5)
elif mode == "full":
    cube.location = (-1.5, 0, 1)
    cube.data.materials.clear()
    cube.data.materials.append(mat("red", (0.8, 0.1, 0.1), 0.3))
    bpy.ops.mesh.primitive_uv_sphere_add(radius=1, location=(1.5, 0.5, 1), segments=24, ring_count=12)
    bpy.ops.object.shade_smooth()
    bpy.context.object.data.materials.append(mat("green", (0.1, 0.7, 0.2), 0.6))
    bpy.ops.mesh.primitive_plane_add(size=10, location=(0, 0, 0))
    bpy.context.object.data.materials.append(mat("grey", (0.5, 0.5, 0.5), 0.9))
    # an asymmetric marker (an L of two boxes) to see handedness
    for loc, sc3 in (((-3.5, -2, 0.25), (0.5, 0.5, 0.25)), ((-3.5, -1.2, 0.25), (0.5, 0.3, 0.25)), ((-2.7, -2, 0.25), (0.3, 0.5, 0.25))):
        bpy.ops.mesh.primitive_cube_add(size=1, location=loc, scale=sc3)
        bpy.context.object.data.materials.append(mat("blue", (0.1, 0.2, 0.9)))
    lamp.location = (3, -3, 5)
    bpy.ops.object.light_add(type="SPOT", location=(-3, 3, 5))
    sp = bpy.context.object
    sp.data.energy = 1500
    sp.data.spot_size = math.radians(40)
    aim(sp, (-1.5, 0, 1))
    bpy.ops.object.light_add(type="SUN", location=(0, 0, 5))
    sp = bpy.context.object
    sp.data.energy = 0.6
    sp.rotation_euler = (math.radians(50), 0, math.radians(30))
cam.location = (7, -7, 5) if mode == "cube" else (6, -9, 6)
aim(cam, (0, 0, 0.5) if mode == "full" else (0, 0, 0))
lamp.data.energy = 1000

st = sc.si3d
st.colour = "SRGB"
st.keep_dir = os.path.join(out, "work%d" % os.getpid())
# reference: Blender's own render of the scene (Cycles on the CPU, a few samples; SI3D_REF=WORKBENCH for flat shading)
if os.environ.get("SI3D_REF") == "WORKBENCH":
    sc.render.engine = "BLENDER_WORKBENCH"
else:
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = 24
    sc.cycles.use_denoising = False
sc.render.filepath = os.path.join(out, mode + "_ref.png")
bpy.ops.render.render(write_still=True)
# the Softimage render
sc.render.engine = "FOGLE3D"
sc.render.filepath = os.path.join(out, mode + "_si.png")
try:
    bpy.ops.render.render(write_still=True)
except Exception as e:
    print("TEST render failed:", e)
    print("RESULT", mode, "FAILED")
    sys.exit(1)
img = bpy.data.images.load(sc.render.filepath)
px = list(img.pixels)
w, h = img.size
n = w * h
nonblank = sum(1 for i in range(n) if px[4 * i + 3] > 0.5 and (px[4 * i] + px[4 * i + 1] + px[4 * i + 2]) > 0.02)
cols = {tuple(round(px[4 * i + c], 1) for c in range(3)) for i in range(n)}
xs = [i % w for i in range(n) if px[4 * i + 3] > 0.5 and (px[4 * i] + px[4 * i + 1] + px[4 * i + 2]) > 0.02]
ys = [i // w for i in range(n) if px[4 * i + 3] > 0.5 and (px[4 * i] + px[4 * i + 1] + px[4 * i + 2]) > 0.02]
print("TEST size", w, h, "non-blank pixels", nonblank, "of", n, "distinct colours", len(cols))
if xs:
    print("TEST bbox x", min(xs), max(xs), "y", min(ys), max(ys))
ok = nonblank > 200 and len(cols) > 3
print("RESULT", mode, "ok" if ok else "FAILED")
softimage3d.unregister()
sys.exit(0 if ok else 1)
