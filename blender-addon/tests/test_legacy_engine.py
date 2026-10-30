# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless test (own Blender process): a file saved with the old engine id SOFTIMAGE3D opens with FOGLE3D, and the
stand-in engine of the old id is gone again after the load.  Env SI3D_ADDON_PARENT = folder holding softimage3d/;
optional SI3D_LEGACY_BLEND = a file made by the pre-rename add-on (checked as well)."""
import os
import sys
import tempfile

import bpy

sys.path.insert(0, os.environ["SI3D_ADDON_PARENT"])
import softimage3d  # noqa: E402

softimage3d.register()
ok_all = True


def check(name, ok, info=""):
    global ok_all
    ok_all &= bool(ok)
    print("TEST %-52s %s %s" % (name, "ok" if ok else "FAIL", "" if ok else info))


def selectable(eid):
    """Whether the engine id can be set on a scene (what the Render Engine dropdown offers)."""
    sc = bpy.data.scenes.new("probe")
    try:
        sc.render.engine = eid
        return sc.render.engine == eid
    except TypeError:
        return False
    finally:
        bpy.data.scenes.remove(sc)


def legacy_listed():
    return selectable(softimage3d.LEGACY_ENGINE_ID)


check("engine in the dropdown under the new id", selectable("FOGLE3D"))
check("engine label", softimage3d.SI3DRenderEngine.bl_label == "Fogle3D", softimage3d.SI3DRenderEngine.bl_label)
check("no old-id engine at rest", not legacy_listed())
check("no scene picker properties", not any(n in softimage3d.SI3DSettings.__annotations__ for n in ("source", "scene_filter", "frame_map")))

# a file holding the old id: made with the stand-in engine registered
tmp = os.path.join(tempfile.mkdtemp(prefix="si3d-legacy-"), "old.blend")
softimage3d._legacy_pre()
bpy.context.scene.render.engine = "SOFTIMAGE3D"
bpy.ops.wm.save_as_mainfile(filepath=tmp)
softimage3d._fix_legacy_engine()
check("fix: engine is FOGLE3D after the rewrite", bpy.context.scene.render.engine == "FOGLE3D", bpy.context.scene.render.engine)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.wm.open_mainfile(filepath=tmp)
check("open_mainfile of an old-id file: FOGLE3D", bpy.context.scene.render.engine == "FOGLE3D", bpy.context.scene.render.engine)
check("old-id engine gone after the load", not legacy_listed())

old = os.environ.get("SI3D_LEGACY_BLEND")
if old and os.path.isfile(old):
    bpy.ops.wm.open_mainfile(filepath=old)
    check("file from the old add-on: FOGLE3D", bpy.context.scene.render.engine == "FOGLE3D", bpy.context.scene.render.engine)
    check("its settings survive (ray_depth 3)", bpy.context.scene.si3d.ray_depth == 3, bpy.context.scene.si3d.ray_depth)
    check("old-id engine gone after the load (2)", not legacy_listed())

print("RESULT", "PASS" if ok_all else "FAIL")
softimage3d.unregister()
sys.exit(0 if ok_all else 1)
