# SPDX-License-Identifier: GPL-3.0-or-later
"""Headless tests of the Softimage nodes (unit nodes1).  Mode in env SI3D_NODES_TEST:
  ui        registration, node poll and sockets, faults, the Add menu, the panels' drawing, the operators
  identity  the exporter's data for Principled scenes against the exporter before the nodes (env SI3D_OLD_EXPORT), the
            database of blend2db before the nodes (env SI3D_BLEND2DB_OLD) on it, and the database of the same scene after
            "Softimage settings to nodes" against the Principled one
  render    renders: Principled against converted, then a local texture, a 3D texture, fog, fade and a set ambience
  links     links the Principled path carries (Mapping repeats, Image Alpha), against Cycles (CPU) and converted
Run through re/tools/nodes1/run_nodes_test.sh.  Own headless Blender only; Cycles on the CPU (links)."""
import glob
import importlib.util
import json
import math
import os
import shutil
import subprocess
import sys
import types

sys.path.insert(0, os.path.join(os.environ["SI3D_ROOT"], "re/tools/blender3/tests"))
from b3lib import *  # noqa: E402,F401  (b2lib: setup, world, mat, add_mesh, add_light, add_camera, si_render, result)
from softimage3d import export, install  # noqa: E402
from softimage3d import material_nodes as MN  # noqa: E402
from softimage3d import material_panels as MP  # noqa: E402
from softimage3d import node_menu  # noqa: E402
from softimage3d import nodes as N  # noqa: E402

mode = os.environ["SI3D_NODES_TEST"]
FAILS = []


def check(name, ok, detail=""):
    print("TEST %s %s%s" % (name, "ok" if ok else "FAILED", (": " + detail) if detail else ""))
    if not ok:
        FAILS.append(name)


def image(name, base, mark, n=32):
    im = bpy.data.images.new(name, n, n, alpha=False)
    px = np.zeros((n, n, 4), np.float32)
    px[..., 3] = 1
    px[..., :3] = base
    q = n // 4
    px[n - q:n, 0:q, :3] = mark
    px[0:q, n - q:n, :3] = mark
    im.pixels.foreach_set(px.ravel())
    im.update()
    return im


def tex_material(name, img):
    m = bpy.data.materials.new(name)
    nt = m.node_tree
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    b = nt.nodes["Principled BSDF"]
    b.inputs["Roughness"].default_value = 0.8
    nt.links.new(t.outputs["Color"], b.inputs["Base Color"])
    return m


def scene(emission=True):
    """Principled materials of every kind the exporter reads: plain, rough metal, glass, half transparent, emissive,
    image textured; a world colour."""
    sc = setup(240, 180)
    world((0.05, 0.06, 0.08))
    add_mesh("plane", (0, 0, 0), mat("floor", (0.7, 0.7, 0.7), 1.0), size=14)
    add_mesh("sphere", (-2.2, 0.5, 1.0), mat("metal", (0.9, 0.6, 0.3), 0.3, 1.0), radius=1.0)
    add_mesh("sphere", (0.0, 1.5, 1.0), mat_adv("glass", (1, 1, 1), Roughness=0.0, Transmission_Weight=1.0,
                                                 IOR=1.45), radius=0.9)
    add_mesh("cube", (2.2, 0.5, 0.8), tex_material("boxtex", image("chk", (0.8, 0.2, 0.1), (0.1, 0.2, 0.9))), size=1.6)
    add_mesh("cube", (0.0, -1.5, 0.4), mat("half", (0.2, 0.7, 0.3), 0.6, 0.0, 0.5), size=0.8)
    if emission:
        em = mat("glow", (0.3, 0.3, 0.3), 0.5)
        b = em.node_tree.nodes["Principled BSDF"]
        b.inputs["Emission Color"].default_value = (1.0, 0.8, 0.2, 1.0)
        b.inputs["Emission Strength"].default_value = 0.7
        add_mesh("cube", (-1.0, -2.0, 0.3), em, size=0.6)
    add_light("POINT", (3, -4, 7), 1500, (0, 0, 0))
    add_light("SUN", (-4, -3, 6), 3, (0, 0, 0))
    add_camera((0, -9, 5), (0, 0, 0.8), 40)
    return sc


# ---------------------------------------------------------------- databases

def build_db(tag, exe=None, data=None):
    """The add-on's data for the current scene (as render_blender_scene assembles it) through blend2db; returns
    (database dir, data)."""
    work = os.path.join(OUT, tag)
    if os.path.isdir(work):
        shutil.rmtree(work)
    os.makedirs(os.path.join(work, "textures"))
    sc = bpy.context.scene
    if data is None:
        os.makedirs(os.path.join(OUT, "textures"), exist_ok=True)
        data = export.scene_to_dict(bpy.context.evaluated_depsgraph_get(), os.path.join(OUT, "textures"))
        data["frame"] = sc.frame_current
        data["render"] = export.render_options(sc, sc.si3d)
    jpath = os.path.join(work, "scene.json")
    with open(jpath, "w") as fh:
        fh.write(json.dumps(data))
    info, bad = install.check(os.environ["SI3D_INSTALLED"], install.BLEND2DB)
    assert not bad, bad
    tmpl = info["template_sts"]
    db = os.path.join(work, "database")
    p = subprocess.run([exe or os.environ["SI3D_BLEND2DB"], "build", jpath, db, os.path.dirname(os.path.dirname(tmpl)),
                        os.path.basename(tmpl)[:-len(".sts")], os.path.dirname(info["rsrc"])],
                       capture_output=True, text=True, timeout=600)
    with open(os.path.join(work, "blend2db.txt"), "w") as fh:
        fh.write(p.stdout + p.stderr)
    assert p.returncode == 0, p.stderr[-400:]
    return db, data


def files(db):
    return sorted(os.path.relpath(f, db) for f in glob.glob(os.path.join(db, "**", "*"), recursive=True)
                  if os.path.isfile(f))


def compare_dbs(a, b):
    """(same, different, only in a, only in b) by relative path."""
    fa, fb = files(a), files(b)
    same, diff = [], []
    for f in sorted(set(fa) & set(fb)):
        with open(os.path.join(a, f), "rb") as x, open(os.path.join(b, f), "rb") as y:
            (same if x.read() == y.read() else diff).append(f)
    return same, diff, sorted(set(fa) - set(fb)), sorted(set(fb) - set(fa))


def chapter(f):
    """The database directory a file is in (LIGHTS, MATERIALS, ...)."""
    return os.path.basename(os.path.dirname(f))


def text_diff(a, b, f, most=6):
    la = open(os.path.join(a, f), errors="replace").read().splitlines()
    lb = open(os.path.join(b, f), errors="replace").read().splitlines()
    out = ["%s: %r -> %r" % (f, x, y) for x, y in zip(la, lb) if x != y]
    if len(la) != len(lb):
        out.append("%s: %d lines -> %d" % (f, len(la), len(lb)))
    return out[:most]


def old_export():
    path = os.environ.get("SI3D_OLD_EXPORT", "")
    if not path or not os.path.isfile(path):
        return None
    spec = importlib.util.spec_from_file_location("si3d_old_export", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# ---------------------------------------------------------------- ui

class FakeLayout:
    """Stands in for a UI layout: records calls, checks that every prop names a real property and every operator id
    exists."""
    calls = []

    def __init__(self):
        self.use_property_split = False
        self.alert = False

    def __iter__(self):
        return iter((FakeLayout(), FakeLayout()))

    def prop(self, data, name, **kw):
        ok = name in data.bl_rna.properties
        FakeLayout.calls.append(("prop", type(data).__name__, name, ok))
        return FakeLayout()

    def operator(self, idname, **kw):
        cat, op = idname.split(".")
        ok = hasattr(getattr(bpy.ops, cat), op) and getattr(getattr(bpy.ops, cat), op).idname() != ""
        FakeLayout.calls.append(("operator", idname, ok))
        return types.SimpleNamespace()

    def panel(self, idname, **kw):
        return FakeLayout(), FakeLayout()

    def __getattr__(self, name):
        if name.startswith("__"):
            raise AttributeError(name)
        return lambda *a, **k: FakeLayout()


def drawn(fn, *args):
    FakeLayout.calls = []
    fn(*args)
    bad = [c for c in FakeLayout.calls if not c[-1]]
    return len(FakeLayout.calls), bad


def ui_test():
    sc = setup()
    check("engine keeps the shader editor", softimage3d.SI3DRenderEngine.bl_use_shading_nodes_custom is False)
    check("registered", all(c.is_registered for c in N.NODES + N.SOCKETS) and
          hasattr(bpy.types, "NODE_MT_si3d_add") and hasattr(bpy.types, "SI3D_PT_material"))
    for name in MP.BUILTIN:
        check("%s shown for the engine" % name, "FOGLE3D" in getattr(bpy.types, name).COMPAT_ENGINES)
    m = bpy.data.materials.new("m")
    geo = bpy.data.node_groups.new("g", "GeometryNodeTree")
    comp = bpy.data.node_groups.new("c", "CompositorNodeTree")
    check("poll: shader trees only", all(c.poll(m.node_tree) and not c.poll(geo) and not c.poll(comp) for c in N.NODES))
    # sockets and defaults
    node = MN.new_material_node(m)
    names = [s.name for s in node.inputs]
    check("material sockets", names == ["Ambient", "Diffuse", "Specular", "Specular Decay", "Reflectivity",
                                        "Transparency", "Refraction Index", "Texture", "Texture 2", "Texture 3",
                                        "Texture 4", "3D Texture"], str(names))
    check("material on the output", MN.material_node(m) == node)
    check("enum record numbers", N.value_of(N.SHADING, "CONSTANT") == 1 and N.value_of(N.SHADING, "BLINN") == 5 and
          N.value_of(N.METHOD, "UV") == 4 and N.value_of(N.CHANNEL, "INTENSITY") == 2)
    # faults
    t = MN.new_texture_node(node, None)
    check("fault: texture without image", MN.faults(t) == ["No image: the texture is left out"], str(MN.faults(t)))
    MN.refresh(m.node_tree)
    check("fault shown on the node", t.use_custom_color and not node.use_custom_color)
    t.image = image("img", (0.5, 0.5, 0.5), (1, 1, 1))
    MN.refresh(m.node_tree)
    check("fault cleared", MN.faults(t) == [] and not t.use_custom_color, str(MN.faults(t)))
    v = m.node_tree.nodes.new("ShaderNodeValue")
    m.node_tree.links.new(v.outputs[0], node.inputs["Diffuse"])
    check("fault: link on a value input", any("Diffuse" in f for f in MN.faults(node)), str(MN.faults(node)))
    m.node_tree.links.remove(node.inputs["Diffuse"].links[0])
    w_in_m = m.node_tree.nodes.new(MN.WORLD)
    check("fault: world node in a material tree", MN.faults(w_in_m) == ["A world node in a material tree: not used"])
    m.node_tree.nodes.remove(w_in_m)
    p = m.node_tree.nodes.get("Principled BSDF")
    out = MN._output(m.node_tree, "ShaderNodeOutputMaterial")
    m.node_tree.links.new(p.outputs[0], out.inputs["Surface"])
    check("fault: material node off the output", MN.material_node(m) is None and
          any("Surface" in f for f in MN.faults(node)), str(MN.faults(node)))
    m.node_tree.links.new(node.outputs[0], out.inputs["Surface"])
    rr = m.node_tree.nodes.new("NodeReroute")
    m.node_tree.links.new(node.outputs[0], rr.inputs[0])
    m.node_tree.links.new(rr.outputs[0], out.inputs["Surface"])
    check("reroute followed", MN.material_node(m) == node)
    node.mute = True
    check("muted node not used", MN.material_node(m) is None)
    node.mute = False
    # node drawing and panels
    ctx = types.SimpleNamespace(engine="FOGLE3D", material=m, world=None, space_data=None)
    for n in (node, t):
        for fn in (n.draw_buttons, n.draw_buttons_ext):
            k, bad = drawn(fn, ctx, FakeLayout())
            check("draw %s.%s" % (n.bl_idname, fn.__name__), not bad and k > 0, str(bad))
    t3 = m.node_tree.nodes.new(MN.TEXTURE3D)
    m.node_tree.links.new(t3.outputs[0], node.inputs["3D Texture"])
    for fn in (t3.draw_buttons, t3.draw_buttons_ext):
        k, bad = drawn(fn, ctx, FakeLayout())
        check("draw %s.%s" % (t3.bl_idname, fn.__name__), not bad and k > 0, str(bad))
    k, bad = drawn(MP.draw_material_node, FakeLayout(), node)
    check("material panel, Softimage node", not bad and k > 20, "%d calls %s" % (k, bad))
    m2 = mat("plain", (0.2, 0.3, 0.4))
    k, bad = drawn(MP.draw_principled, FakeLayout(), m2)
    check("material panel, Principled", not bad and k > 5, "%d calls %s" % (k, bad))
    check("panel poll", MP.SI3D_PT_material.poll(ctx) and
          not MP.SI3D_PT_material.poll(types.SimpleNamespace(engine="CYCLES", material=m)))
    w = world((0.1, 0.1, 0.1))
    wn = MN.convert_world(w)
    wctx = types.SimpleNamespace(engine="FOGLE3D", world=w)
    for fn in (wn.draw_buttons, wn.draw_buttons_ext):
        k, bad = drawn(fn, wctx, FakeLayout())
        check("draw world.%s" % fn.__name__, not bad and k > 0, str(bad))
    wn.fade = wn.fog = True
    k, bad = drawn(MP.SI3D_PT_world.draw, types.SimpleNamespace(layout=FakeLayout()), wctx)
    check("world panel", not bad and k > 10, "%d calls %s" % (k, bad))
    check("world on the output", MN.world_node(w) == wn and MN.faults(wn) == [], str(MN.faults(wn)))
    wn.fade_end = wn.fade_start
    check("fault: fade end", MN.faults(wn) == ["Fade end is not beyond its start"])
    # the Add menu
    mspace = types.SimpleNamespace(tree_type="ShaderNodeTree", shader_type="OBJECT")
    wspace = types.SimpleNamespace(tree_type="ShaderNodeTree", shader_type="WORLD")
    gspace = types.SimpleNamespace(tree_type="GeometryNodeTree", shader_type="OBJECT")
    check("menu shown", node_menu.shown(types.SimpleNamespace(engine="FOGLE3D", space_data=mspace)) and
          not node_menu.shown(types.SimpleNamespace(engine="CYCLES", space_data=mspace)) and
          not node_menu.shown(types.SimpleNamespace(engine="FOGLE3D", space_data=gspace)))
    check("menu rows", node_menu.rows_for(types.SimpleNamespace(space_data=mspace)) == node_menu.MATERIAL_ROWS and
          node_menu.rows_for(types.SimpleNamespace(space_data=wspace)) == node_menu.WORLD_ROWS)
    for space in (mspace, wspace):
        k, bad = drawn(node_menu.NODE_MT_si3d_add.draw, types.SimpleNamespace(layout=FakeLayout()),
                       types.SimpleNamespace(engine="FOGLE3D", space_data=space))
        check("menu draw %s" % space.shader_type, not bad and k >= 2, str(bad))
    # operators
    ob = add_mesh("cube", (0, 0, 0))
    with bpy.context.temp_override(object=ob, active_object=ob):
        r = bpy.ops.si3d.new_material()
    nm = ob.active_material
    check("op new material", r == {"FINISHED"} and nm is not None and MN.material_node(nm) is not None)
    with bpy.context.temp_override(object=ob, material=nm):
        for _ in range(4):
            bpy.ops.si3d.add_texture(kind="2D")
        r5 = bpy.ops.si3d.add_texture(kind="2D")
        r3 = bpy.ops.si3d.add_texture(kind="3D")
        r33 = bpy.ops.si3d.add_texture(kind="3D")
    two, t3n = MN.textures_of(MN.material_node(nm))
    check("op add texture", len(two) == 4 and r5 == {"CANCELLED"} and r3 == {"FINISHED"} and r33 == {"CANCELLED"}
          and t3n is not None)
    pm = mat("toconvert", (0.6, 0.2, 0.2), 0.4, 0.5)
    ob.data.materials.append(pm)
    want = MN.principled_values(export._material(pm, None))
    with bpy.context.temp_override(object=ob, material=pm):
        r = bpy.ops.si3d.convert_shader()
    pn = MN.material_node(pm)
    check("op convert shader", r == {"FINISHED"} and pn is not None and
          pm.node_tree.nodes.get("Principled BSDF") is not None)
    got = MN.material_record(pn)
    check("convert keeps the values", all(got[k] == want[k] for k in want), "%s against %s" % (got, want))
    for i in range(3):
        mat("more%d" % i, (0.1 * i, 0.2, 0.3))
    bpy.data.worlds.new("other")
    r = bpy.ops.si3d.settings_to_nodes()
    check("op settings to nodes", r == {"FINISHED"} and
          all(MN.material_node(x) is not None for x in bpy.data.materials) and
          all(MN.world_node(x) is not None for x in bpy.data.worlds))
    sc.world = None
    r = bpy.ops.si3d.new_world()
    check("op new world", r == {"FINISHED"} and sc.world is not None and MN.world_node(sc.world) is not None)
    softimage3d.unregister()
    check("unregistered", not any(c.is_registered for c in N.NODES + N.SOCKETS) and not MP.SI3D_PT_material.is_registered
          and all("FOGLE3D" not in getattr(bpy.types, n).COMPAT_ENGINES for n in MP.BUILTIN))
    softimage3d.register()
    check("registered again", all(c.is_registered for c in N.NODES + N.SOCKETS) and MP.SI3D_PT_material.is_registered)
    result("ui", not FAILS)


# ---------------------------------------------------------------- identity

def _as_before(d, old_form):
    """The exporter's data in the form of the exporter before the nodes (unit mem1 made the meshes compact: shortest-decimal
    float32 numbers, a corner table with `corner_x` for the triangle corners that do not use their vertex's own entry, no
    `polys` and `uv`): the corner normals and UVs per triangle, the mesh floats as float32 bits, `polys` and `uv` left out."""
    import struct
    import copy
    d = copy.deepcopy(d)
    f32 = lambda v: struct.unpack("<f", struct.pack("<f", v))[0]
    for o in d["objects"]:
        if "corner_normals" in o:
            tris = o["tris"]
            cx = {x: c for x, c in o.get("corner_x", [])}
            ci = lambda ti, k: cx.get(ti * 3 + k, tris[ti][k])
            o["tri_normals"] = [[o["corner_normals"][ci(ti, k)] for k in range(3)] for ti in range(len(tris))]
            o["tri_uv"] = [[o["corner_uv"][ci(ti, k)] for k in range(3)] for ti in range(len(tris))] if o["corner_uv"] else []
            for k in ("corner_normals", "corner_uv", "corner_x"):
                del o[k]
        o.pop("polys", None)
        o.pop("uv", None)
        o["verts"] = [[f32(x) for x in v] for v in o["verts"]]
        o["tri_normals"] = [[[f32(x) for x in n] for n in t] for t in o["tri_normals"]]
        o["tri_uv"] = [[[f32(x) for x in n] for n in t] for t in o["tri_uv"]]
    return d


# Keys the exporter gained after the nodes' baseline (SI3D_OLD_EXPORT, commit 5bf576e) for features of their own, left
# out of the "as before the nodes" comparison (unit softspeed1; the baseline stays the export before the nodes):
# soft shadows (unit soft1: a light's radius or angle, the Soft shadow lights setting), glossy highlights (unit gloss1:
# the Principled node's highlight inputs and the key light's spread).
LATER_LIGHT_KEYS = ("radius", "angle")
LATER_MATERIAL_KEYS = ("specular_level", "specular_tint", "coat", "coat_roughness", "coat_ior", "key_spread")
LATER_RENDER_KEYS = ("soft_lights",)


def without_later_keys(d, render=False):
    """A copy of the exporter's scene data (or, with render, its render options) without LATER_*_KEYS."""
    if render:
        return {k: v for k, v in d.items() if k not in LATER_RENDER_KEYS}
    d = dict(d)
    d["lights"] = [{k: v for k, v in e.items() if k not in LATER_LIGHT_KEYS} for e in d.get("lights", [])]
    d["materials"] = {n: {k: v for k, v in m.items() if k not in LATER_MATERIAL_KEYS}
                      for n, m in d.get("materials", {}).items()}
    return d


def identity_test():
    old = old_export()
    for emission in (False, True):
        tag = "emission" if emission else "plain"
        scene(emission)
        sc = bpy.context.scene
        dg = bpy.context.evaluated_depsgraph_get()
        texdir = os.path.join(OUT, "textures")
        os.makedirs(texdir, exist_ok=True)
        d_new = export.scene_to_dict(dg, texdir)
        if old is not None:
            d_old = old.scene_to_dict(dg, texdir)
            check("%s: exporter data as before the nodes" % tag,
                  json.dumps(without_later_keys(_as_before(d_old, True))) == json.dumps(without_later_keys(_as_before(d_new, False))))
            check("%s: render options as before" % tag,
                  json.dumps(without_later_keys(old.render_options(sc, sc.si3d), True))
                  == json.dumps(without_later_keys(export.render_options(sc, sc.si3d), True)))
        else:
            print("TEST %s: exporter before the nodes not given (SI3D_OLD_EXPORT), not compared" % tag)
        db_p, data_p = build_db(tag + "-principled")
        exe_old = os.environ.get("SI3D_BLEND2DB_OLD", "")
        if exe_old and os.path.isfile(exe_old):
            db_o, _ = build_db(tag + "-principled-oldtool", exe_old, data_p)
            same, diff, oa, ob = compare_dbs(db_o, db_p)
            check("%s: blend2db before and after the nodes, %d files" % (tag, len(same)),
                  not diff and not oa and not ob, "different %s, only old %s, only new %s" % (diff, oa, ob))
        else:
            print("TEST %s: blend2db before the nodes not given (SI3D_BLEND2DB_OLD), not compared" % tag)
        r = bpy.ops.si3d.settings_to_nodes()
        check("%s: converted" % tag, r == {"FINISHED"} and
              all(MN.material_node(m) is not None for m in bpy.data.materials))
        db_n, data_n = build_db(tag + "-nodes")
        same, diff, oa, ob = compare_dbs(db_p, db_n)
        print("TEST %s: Principled against nodes: %d files identical, %d different %s" % (tag, len(same), len(diff), diff))
        for f in diff:
            if f.endswith((".sts", ".dsc")):
                for line in text_diff(db_p, db_n, f):
                    print("TEST   ", line)
        must = [f for f in diff if chapter(f) not in ("LIGHTS", "SETUP_SOFT")]
        light_only = all(chapter(f) in ("LIGHTS", "SETUP_SOFT") for f in diff)
        check("%s: materials, models, textures, pictures, scene identical" % tag, not must and not oa and not ob,
              "different %s, only Principled %s, only nodes %s" % (must, oa, ob))
        check("%s: lights and set-up identical too" % tag, not diff, "%s%s" % (diff, " (lights and set-up only)"
                                                                             if diff and light_only else ""))
    result("identity", not FAILS)


# ---------------------------------------------------------------- render

def render(tag):
    sc = bpy.context.scene
    keep = os.path.join(OUT, "r-" + tag)
    if os.path.isdir(keep):
        shutil.rmtree(keep)
    sc.si3d.keep_dir = keep
    a = si_render(os.path.join(OUT, tag + ".png"))
    dbs = glob.glob(os.path.join(keep, "*", "database"))
    return a, (dbs[0] if dbs else None)


def dsc_text(db):
    f = glob.glob(os.path.join(db, "**", "SCENES", "*.dsc"), recursive=True)
    return open(f[0], errors="replace").read() if f else ""


def has_rel(dsc, kind):
    import re
    return re.search(r"\b\d+\s+\d+\s+%d\s*;" % kind, dsc) is not None


def sts_text(db):
    f = glob.glob(os.path.join(db, "**", "SETUP_SOFT", "*.sts"), recursive=True)
    return open(f[0], errors="replace").read() if f else ""


def region(a, box):
    h, w = a.shape[:2]
    x0, y0, x1, y1 = box
    return a[int(h * y0):int(h * y1), int(w * x0):int(w * x1), :3]


def render_test():
    scene(True)
    sc = bpy.context.scene
    ref, db_p = render("principled")
    bpy.ops.si3d.settings_to_nodes()
    conv, db_n = render("converted")
    d = np.abs(ref[..., :3] - conv[..., :3])
    check("render: converted against Principled", d.max() <= 2.5 / 255,
          "max %.1f/255, mean %.4f/255, %d pixels differ" % (d.max() * 255, d.mean() * 255, int((d > 0.25 / 255).any(-1).sum())))
    # a local texture (relation 401) on the floor: a repeated checker
    floor = MN.material_node(bpy.data.materials["floor"])
    t = MN.new_texture_node(floor, image("floorimg", (0.9, 0.9, 0.9), (0.05, 0.05, 0.4), 16))
    t.urepeat = t.vrepeat = 6
    t.mask = "BLEND"
    loc, db = render("local-texture")
    dt = dsc_text(db)
    floor_box = (0.05, 0.85, 0.95, 0.99)
    ch = np.abs(region(loc, floor_box) - region(conv, floor_box)).mean()
    check("render: local texture changes the floor", ch > 0.02 and has_rel(dt, 401),
          "mean change %.3f, relation 401 in the scene: %s" % (ch, has_rel(dt, 401)))
    # a 3D texture (relation 501) on the half transparent cube: marble
    half = MN.material_node(bpy.data.materials["half"])
    t3 = bpy.data.materials["half"].node_tree.nodes.new(MN.TEXTURE3D)
    bpy.data.materials["half"].node_tree.links.new(t3.outputs[0], half.inputs["3D Texture"])
    t3.kind = "MARBLE"
    t3.mask = "BLEND"
    t3.scaling = (0.3, 0.3, 0.3)
    t3.colour1 = (0.0, 0.0, 0.0, 0.0)
    t3.colour5 = (0.1, 0.9, 0.1, 0.0)
    t3d, db = render("texture3d")
    dt = dsc_text(db)
    cube_box = (0.44, 0.57, 0.56, 0.76)
    ch = np.abs(region(t3d, cube_box) - region(loc, cube_box)).mean()
    n3 = len(glob.glob(os.path.join(db, "**", "TEXTURES3D", "*"), recursive=True))
    check("render: 3D texture changes the cube", ch > 0.01 and has_rel(dt, 501) and n3 == 1,
          "mean change %.3f, relation 501 in the scene: %s, %d 3D texture files" % (ch, has_rel(dt, 501), n3))
    # the world: fog, fade, a set ambience
    w = sc.world
    wn = MN.world_node(w)
    wn.fog = True
    wn.fog_base, wn.fog_thickness, wn.fog_density, wn.fog_start, wn.fog_end = 0.0, 3.0, 1.0, 0.0, 10000.0
    wn.fog_colour = (0.9, 0.9, 0.95)
    fog, db = render("fog")
    st = sts_text(db)
    ch = np.abs(fog[..., :3] - t3d[..., :3]).mean()
    check("render: fog", ch > 0.01 and "FOG ACTIVE" in " ".join(st.split()),
          "mean change %.3f, FOG ACTIVE in set-up: %s" % (ch, "FOG ACTIVE" in " ".join(st.split())))
    wn.fog = False
    wn.fade = True
    wn.fade_start, wn.fade_end, wn.fade_colour = 6.0, 16.0, (0.5, 0.0, 0.0)
    fade, db = render("fade")
    st = sts_text(db)
    far = (0.3, 0.42, 0.7, 0.5)
    ch = np.abs(region(fade, far) - region(t3d, far)).mean()
    check("render: fade", ch > 0.01 and "FADE ACTIVE" in " ".join(st.split()),
          "mean change %.3f in the far floor" % ch)
    wn.fade = False
    wn.fit_ambience = False
    wn.ambience = (0.6, 0.6, 0.6)
    amb, db = render("ambience")
    st = " ".join(sts_text(db).split())
    box = (0.62, 0.32, 0.86, 0.62)     # the image textured cube (the floor is white already)
    ch = luma(region(amb, box)).mean() - luma(region(t3d, box)).mean()
    check("render: set ambience brightens", ch > 0.04 and "AMBIENCE 0.6 0.6 0.6" in st,
          "mean luma change %.3f on the textured cube" % ch)
    wn.background = (0.0, 0.0, 0.4)
    bg, _ = render("background")
    corner = bg[2:10, 2:10, :3].mean(axis=(0, 1))
    check("render: background colour", corner[2] > corner[0] + 0.2, "corner %s" % np.round(corner, 3))
    result("render", not FAILS)


# ---------------------------------------------------------------- links on the Principled path

def checker(name, n=8, alpha_left=False):
    im = bpy.data.images.new(name, n, n, alpha=alpha_left)
    px = np.zeros((n, n, 4), np.float32)
    yy, xx = np.mgrid[0:n, 0:n]
    on = ((xx // (n // 4) + yy // (n // 4)) % 2) == 0
    px[..., :3] = np.where(on[..., None], (0.9, 0.9, 0.9), (0.1, 0.2, 0.8))
    px[..., 3] = 1.0
    if alpha_left:
        px[:, : n // 2, 3] = 0.0
    im.pixels.foreach_set(px.ravel())
    im.update()
    return im


def wall(m, loc=(0, 0, 0), size=3.0):
    ob = add_mesh("plane", loc, m, size=size)
    ob.rotation_euler = (math.pi / 2, 0, 0)
    return ob


def linked_material(name, img, mapping=None, alpha=False):
    m = bpy.data.materials.new(name)
    nt = m.node_tree
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    t.interpolation = "Closest"
    b = nt.nodes["Principled BSDF"]
    b.inputs["Roughness"].default_value = 1.0
    b.inputs["Specular IOR Level"].default_value = 0.0
    nt.links.new(t.outputs["Color"], b.inputs["Base Color"])
    if mapping:
        tc = nt.nodes.new("ShaderNodeTexCoord")
        mp = nt.nodes.new("ShaderNodeMapping")
        mp.inputs["Scale"].default_value = mapping
        nt.links.new(tc.outputs["UV"], mp.inputs["Vector"])
        nt.links.new(mp.outputs["Vector"], t.inputs["Vector"])
    if alpha:
        nt.links.new(t.outputs["Alpha"], b.inputs["Alpha"])
    return m, t


def links_scene():
    sc = setup(160, 120)
    world((0.02, 0.02, 0.02))
    add_light("SUN", (0, -5, 3), 4.0, (0, 0, 0))
    add_camera((0, -7, 0), (0, 0, 0), 50)
    return sc


def links_test():
    # a Mapping with whole scales: the repeats
    links_scene()
    m, t = linked_material("mapped", checker("chk", 64), mapping=(3.0, 2.0, 1.0))
    wall(m)
    d = export._material(m, None)
    check("links: Mapping read as repeats", d["textures"][0].get("si") == {"urepeat": 3, "vrepeat": 2},
          str(d["textures"][0].get("si")))
    ref = cycles_ref(os.path.join(OUT, "mapping_ref.png"), 16)
    ours, _ = render("mapping")
    m.node_tree.links.remove(t.inputs["Vector"].links[0])
    check("links: no Mapping, no override", "si" not in export._material(m, None)["textures"][0])
    single, _ = render("mapping-unlinked")
    a, b = rmse(ref, ours), rmse(ref, single)
    check("links: repeats closer to Cycles", a < 0.5 * b, "rmse %.4f with the repeats, %.4f without" % (a, b))
    # Image Alpha -> Alpha: the picture's alpha is the opacity
    links_scene()
    add_mesh("cube", (-0.6, 2.0, 0.0), mat("red", (0.8, 0.05, 0.05), 1.0), size=1.6)
    m, t = linked_material("cutout", checker("chka", alpha_left=True), alpha=True)
    wall(m)
    d = export._material(m, None)
    check("links: Alpha read as the opacity", d["textures"][0].get("si", {}).get("transparency") == -1.0,
          str(d["textures"][0].get("si")))
    bpy.context.scene.render.film_transparent = False
    ref = cycles_ref(os.path.join(OUT, "alpha_ref.png"), 16)
    ours, db = render("alpha")
    st = " ".join(sts_text(db).split())
    m.node_tree.links.remove(m.node_tree.nodes["Principled BSDF"].inputs["Alpha"].links[0])
    opaque, _ = render("alpha-unlinked")
    a, b = rmse(ref, ours), rmse(ref, opaque)
    left = (0.3, 0.3, 0.48, 0.7)
    red = region(ours, left)
    red_seen = float((red[..., 0] > red[..., 2] + 0.2).mean())
    check("links: cut-out closer to Cycles", a < 0.6 * b and red_seen > 0.3,
          "rmse %.4f with the alpha, %.4f without; red seen through the cut-out half: %.2f of the box" % (a, b, red_seen))
    check("links: ray depth raised for the cut-out", "MAX_RAY_DEPTH 4" in st,
          str([w.strip() for w in sts_text(db).splitlines() if "MAX_RAY_DEPTH" in w]))
    # both convert to Softimage nodes with the same database
    for name, kw in (("mapped", {"mapping": (3.0, 2.0, 1.0)}), ("cutout", {"alpha": True})):
        links_scene()
        add_mesh("cube", (-0.6, 2.0, 0.0), mat("red", (0.8, 0.05, 0.05), 1.0), size=1.6)
        m, t = linked_material(name, checker("c" + name, alpha_left=(name == "cutout")), **kw)
        wall(m)
        db_p, _ = build_db("links-%s-principled" % name)
        bpy.ops.si3d.settings_to_nodes()
        db_n, _ = build_db("links-%s-nodes" % name)
        same, diff, oa, ob = compare_dbs(db_p, db_n)
        check("links: %s converts to the same database, %d files" % (name, len(same)), not diff and not oa and not ob,
              "different %s" % diff)
    result("links", not FAILS)


MODES = {"ui": ui_test, "identity": identity_test, "render": render_test, "links": links_test}
if mode in MODES:       # any other mode: imported as a library
    MODES[mode]()
