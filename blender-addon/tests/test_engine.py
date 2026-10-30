# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless test of the Softimage 3D engine (run through re/tools/blender1/run_test.sh).

Registers the add-on, renders one frame and a range of a corpus scene through
bpy.ops.render.render, and compares every pixel with the pictures a direct render-frames run
wrote (decoded by the repository's own re/tools/sipic.py).  Values mode RAW: byte/255 must
come back exactly."""
import importlib.util
import os
import sys

import bpy

addon_parent = os.environ["SI3D_ADDON_PARENT"]
ref_dir = os.environ["SI3D_REF_DIR"]          # pictures of the direct run: <ref>/<scene>/NAME.<n>.pic
sys.path.insert(0, addon_parent)
spec = importlib.util.spec_from_file_location("sipic_ref", os.environ["SI3D_SIPIC_REF"])
sipic_ref = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sipic_ref)
import softimage3d  # noqa: E402

softimage3d.register()
sc = bpy.context.scene
sc.render.engine = "FOGLE3D"
sc.si3d.colour = "RAW"
from softimage3d import session  # noqa: E402
session.set_tool_scene(sc, os.environ["SI3D_SCENE"])      # tool-only direct .dsc render (no UI for it)
sc.view_settings.view_transform = "Raw"
sc.render.dither_intensity = 0.0      # Blender dithers 8-bit output by default

pics = {}
for root, _, files in os.walk(ref_dir):
    for f in files:
        if f.endswith(".pic"):
            n = int(f.rsplit(".", 2)[1])
            info, planes, _ = sipic_ref.decode(open(os.path.join(root, f), "rb").read())
            pics[n] = (info["width"], info["height"], planes)
first, last = min(pics), max(pics)
w, h = pics[first][0], pics[first][1]
sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = w, h, 100


def want_at(frame, x, y):
    _, _, planes = pics[frame]
    j = y * w + x
    return (planes["R"][j], planes["G"][j], planes["B"][j], planes["A"][j] if "A" in planes else 255)


def count_diff(px, frame, opaque_only=False):
    diff = 0
    for y in range(h):
        for x in range(w):
            i = ((h - 1 - y) * w + x) * 4          # Blender rows run bottom to top
            want = want_at(frame, x, y)
            if opaque_only and want[3] != 255:
                continue      # PNG stores straight alpha: Blender un-premultiplies the edge pixels
            got = tuple(int(round(px[i + c] * 255)) for c in range(4))
            if got != want:
                diff += 1
                if diff <= 3 and os.environ.get("SI3D_DEBUG"):
                    print("DIFF frame", frame, "xy", x, y, "want", want, "got", got)
    return diff


def compare(frame, label):
    # background mode keeps no Render Result image: write the frame as a 32-bit float EXR (exact)
    path = os.path.join(os.environ["SI3D_OUT"], "still_%s_%d.exr" % (label.split()[0] + label.split()[1], frame))
    sc.render.image_settings.file_format = "OPEN_EXR"
    sc.render.image_settings.color_depth = "32"
    sc.render.filepath = path
    r = bpy.ops.render.render(write_still=True)
    assert r == {"FINISHED"}, r
    img = bpy.data.images.load(path)
    img.colorspace_settings.name = "Non-Color"
    assert tuple(img.size) == (w, h), (tuple(img.size), w, h)
    d = count_diff(list(img.pixels[:]), frame)
    print("TEST %-30s frame %d: %d pixels, %d different" % (label, frame, w * h, d))
    return d, w * h


total_diff = total_px = 0
modes = [("RANGE", "single frame (range mode)", [last]),
         ("RANGE", "frame loop (range mode)", list(range(first, last + 1))),
         ("ALONE", "single frame (alone mode)", [first])]
for mode, label, frames in modes:
    sc.si3d.frame_mode = mode
    for f in frames:
        sc.frame_set(f)
        d, n = compare(f, label)
        if mode == "RANGE":          # a frame alone may differ from the range run (finding 0012, s.23.1)
            total_diff += d
            total_px += n
# animation through the operator, written as PNG with the Raw view transform
sc.si3d.frame_mode = "RANGE"
sc.frame_start, sc.frame_end = first, last
sc.render.image_settings.file_format = "PNG"
sc.render.image_settings.color_mode = "RGBA"
sc.render.image_settings.color_depth = "8"
sc.render.filepath = os.path.join(os.environ["SI3D_OUT"], "anim_")
bpy.ops.render.render(animation=True)
png_diff = 0
for f in range(first, last + 1):
    im = bpy.data.images.load(os.path.join(os.environ["SI3D_OUT"], "anim_%04d.png" % f))
    im.colorspace_settings.name = "Non-Color"
    png_diff += count_diff(list(im.pixels[:]), f, True)
print("TEST animation PNG files (alpha 255 pixels): %d frames, %d pixels, %d different" % (last - first + 1, (last - first + 1) * w * h, png_diff))
print("RESULT range-mode pixels different: %d of %d; png different: %d" % (total_diff, total_px, png_diff))
softimage3d.unregister()
sys.exit(0 if total_diff == 0 and png_diff == 0 else 1)
