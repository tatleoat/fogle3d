# SPDX-License-Identifier: GPL-3.0-or-later
"""File > Import > Softimage 3D scene (.dsc): a scene of the user's own install as ordinary, editable Blender data.

Named importer.py, not import.py: `import` is a Python keyword, so `from . import import` cannot be written.

The scene is read by the port itself: the program `si-import-dump` (re/tools/import1, our own Rust) loads the scene with
the engine's loader, brings it to each frame with the renderer's own update and writes what the renderer would draw as
JSON (format: docs/design/softimage-import.md).  This module turns that JSON into objects, meshes, curves, materials
(the add-on's Softimage nodes), images, lights, a camera, a world and keyframes.  Pictures stay in the install: a .pic
is converted once to a PNG in a cache folder (cache_dir: Blender's user data folder, never inside the add-on) and the
image keeps the original path as a custom property.  A .dsc dropped on the 3D viewport or the outliner runs the same
import (SI3D_FH_import_dsc); re/tools/import1/convert_scenes.py saves scenes as .blend files.  Our own code; nothing here reproduces a function of the original program.

Softimage is right-handed with Y up and row vectors (p' = p M, translation in the last row); Blender has Z up and
column vectors.  A Softimage point (x, y, z) is the Blender point (x, -z, y): C below.  Units are kept 1:1.
"""
import json
import math
import os
import subprocess
import tempfile

import bpy
from bpy.props import BoolProperty, CollectionProperty, StringProperty
from mathutils import Matrix, Vector

FORMAT = "si3d-import-dump"
VERSION = 1
REPORT_TEXT = "Fogle3D import report"
TEXTURE_DIR = None              # where the PNG copies of the .pic textures go when set (the converter's own folder)
SHAPE_KEY_CAP = 40_000_000      # floats of per-frame shape keys per import (frames x vertices x 3); beyond: first frame only
# a tessellated triangle whose doubled area is at most this times its longest edge squared (narrower than 1e-4 of its
# length) is a sliver, chiefly the pole triangles of a NURBS sphere or revolve: two corners on the pole, apart by
# rounding only (<= 2.5e-5 in 40 dumps; other triangles >= 1e-3 but a few of one deformed sphere).  The renderer leaves
# zero-area triangles undrawn; kept in a mesh, their own normal is rounding noise, so _orient flips some at random,
# and the mesh renderer then refuses those or loses the good triangles beside them (holes at the poles, unit pole1)
SLIVER = 1e-4

# Softimage (x, y, z) -> Blender (x, -z, y)
C3 = Matrix(((1.0, 0.0, 0.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0)))
C4 = C3.to_4x4()
C4T = C4.transposed()

KIND_NAMES = {-1: "dummy root", 0: "null", 1: "patch", 2: "face", 3: "light", 4: "mesh", 5: "skeleton joint", 6: "spline",
              7: "metaball", 8: "metaball", 9: "NURBS curve", 10: "NURBS surface"}

# not carried at all, whatever the scene (the dump bakes kinematics into matrices; these have no Blender counterpart here)
ALWAYS_NOT_CARRIED = (
    "IK chains, constraints and expressions: their effect is baked into per-frame keyframes; the rigs themselves are not built",
    "dynamics, particles and the custom effects (.so shaders): not imported",
    "metaballs and NURBS curves: imported as empties at their place",
    "boolean and other modelling stacks: only the resulting geometry the renderer draws",
    "patches, faces and NURBS surfaces: imported as the renderer's own triangles (no editable surface)",
    "3D (procedural) textures: built as a Softimage 3D Texture node at the first frame's values (an animated 3D texture "
    "keeps those values); a node's own 3D texture goes on a copy of its material",
    "texture band and old-factor fields, picture sequences (first picture only) and light fall-off ranges: "
    "kept as custom properties only",
)


# ------------------------------------------------------------------ the dump program

def dump_program():
    """si-import-dump: env SI3D_IMPORT_DUMP, else the add-on preference, else a copy in the add-on's bin/."""
    p = os.environ.get("SI3D_IMPORT_DUMP", "")
    if p:
        return p
    from . import session
    pr = session.prefs()
    if pr is not None and getattr(pr, "import_dump", ""):
        return bpy.path.abspath(pr.import_dump)
    return session.bundled("si-import-dump")


def install_root(context):
    from . import install, session
    st = getattr(context.scene, "si3d", None)
    pr = session.prefs()
    return install.resolve_root(st.installed if st is not None else "", pr.install_path if pr is not None else "",
                                bpy.path.abspath)


def run_dump(dsc, root, frames="all", deform=True, program=None, memory_kb=None):
    """Runs si-import-dump on `dsc` and returns the parsed dump."""
    from . import install, session
    exe = program or dump_program()
    if not exe or not os.path.isfile(exe):
        raise RuntimeError("The si-import-dump program is not set or not found (Preferences > Add-ons > Fogle3D, "
                           "or env SI3D_IMPORT_DUMP); build it with: cargo build --release --manifest-path "
                           "re/tools/import1/Cargo.toml")
    _, bad = install.check(root, install.RENDER)
    if bad:
        raise RuntimeError("\n".join(bad))
    mem = memory_kb or session.memory_kb()
    with tempfile.TemporaryDirectory(prefix="si3d-import-") as tmp:
        out = os.path.join(tmp, "dump.json")
        args = [exe, "--install", root, "--frames", frames] + (["--deform"] if deform else []) + [dsc, out]
        # the program file is read from the install (si-rt); SI_INSTALL names it when --install is not where it is
        env = dict(os.environ, **install.program_env(root))
        p = subprocess.run(["/bin/sh", "-c", 'ulimit -v %d; exec "$@"' % mem, "sh"] + args, env=env,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=3600)
        if p.returncode != 0 or not os.path.isfile(out):
            text = p.stdout.decode("utf-8", "replace").strip()
            raise RuntimeError("si-import-dump failed (%d): %s" % (p.returncode, text[-2000:] or "no output"))
        with open(out, "r", encoding="utf-8") as fh:
            d = json.load(fh)
    if d.get("format") != FORMAT or d.get("version") != VERSION:
        raise RuntimeError("not a %s version %d file" % (FORMAT, VERSION))
    return d


# ------------------------------------------------------------------ conversions

def _sliver(p):
    """True when triangle p (three points) has 2 * area <= SLIVER * (longest edge)**2 (see SLIVER)."""
    u = [p[1][k] - p[0][k] for k in range(3)]
    w = [p[2][k] - p[0][k] for k in range(3)]
    c = (u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0])
    edge = max(math.dist(p[0], p[1]), math.dist(p[1], p[2]), math.dist(p[2], p[0]))
    return math.sqrt(c[0] * c[0] + c[1] * c[1] + c[2] * c[2]) <= SLIVER * edge * edge


def si_matrix(m16):
    """A Softimage row matrix (16 floats, row by row) as a Blender Matrix in Blender's axes."""
    m = Matrix((m16[0:4], m16[4:8], m16[8:12], m16[12:16])).transposed()
    return C4 @ m @ C4T


def si_point(p):
    return Vector((p[0], -p[2], p[1]))


def _linear(c):
    c = min(max(c, 0.0), 1.0)
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def _name(s, fallback):
    s = (s or "").strip()
    return s if s else fallback


def _texture3d_id(rec):
    """A material's or node's 3D texture id in the dump (older dumps hold a material's presence as a bool)."""
    v = rec.get("texture3d")
    return v if type(v) is int else None


def _props(idblock, prefix, d, keys=None):
    for k, v in d.items():
        if keys is not None and k not in keys:
            continue
        if isinstance(v, bool):
            v = int(v)
        if v is None:
            continue
        if isinstance(v, (dict,)):
            idblock[prefix + k] = json.dumps(v)
        elif isinstance(v, list) and any(isinstance(x, (list, dict)) for x in v):
            idblock[prefix + k] = json.dumps(v)
        else:
            idblock[prefix + k] = v


# ------------------------------------------------------------------ pictures

def cache_dir():
    """Where the PNG copies of the install's .pic textures go.  Blender 5.2 has no 'CACHE' user resource (the brief's
    first choice, tried in case a later Blender adds it); an installed extension gets its own user folder; otherwise
    Blender's user data folder, under softimage3d/textures.  TEXTURE_DIR (set by a caller such as
    re/tools/import1/convert_scenes.py) comes first."""
    if TEXTURE_DIR:
        os.makedirs(TEXTURE_DIR, exist_ok=True)
        return TEXTURE_DIR
    try:
        return bpy.utils.user_resource('CACHE', path="softimage3d/textures", create=True)
    except (TypeError, ValueError):
        pass
    pkg = __package__ or ""
    if pkg.startswith("bl_ext."):
        try:
            return bpy.utils.extension_path_user(pkg, path="textures", create=True)
        except (AttributeError, ValueError, OSError):
            pass
    return bpy.utils.user_resource('DATAFILES', path="softimage3d/textures", create=True)


def _cache_name(path):
    import hashlib
    st = os.stat(path)
    key = ("%s|%d|%d" % (os.path.abspath(path), st.st_size, int(st.st_mtime))).encode("utf-8", "surrogateescape")
    base = "".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in os.path.basename(path)[:-4] or "picture")
    return "%s-%s.png" % (base, hashlib.sha1(key).hexdigest()[:12])


def picture_image(pic, notes):
    """A Blender image of a .pic file of the install, through a PNG in the cache.  None when it cannot be read."""
    for img in bpy.data.images:
        if img.get("si3d_picture") == pic:
            return img
    try:
        png = os.path.join(cache_dir(), _cache_name(pic))
    except OSError as e:
        notes.append("picture %s: %s" % (pic, e))
        return None
    name = os.path.basename(pic)
    if not os.path.isfile(png):
        from . import sipic
        import numpy as np
        try:
            with open(pic, "rb") as fh:
                w, h, planes = sipic.decode_np(fh.read())
        except Exception as e:  # noqa: BLE001 - any unreadable picture is reported and skipped
            notes.append("picture %s not read: %s" % (pic, e))
            return None
        px = np.empty((h, w, 4), np.float32)
        for k, nm in enumerate("RGB"):
            p = planes.get(nm)
            px[..., k] = (p.reshape(h, w).astype(np.float32) / 255.0) if p is not None else 0.0
        a = planes.get("A")
        # a picture without alpha: opaque here; the exporter then writes no alpha plane and the renderer reads its
        # default alpha (0xad) for it, as for the original file
        px[..., 3] = (a.reshape(h, w).astype(np.float32) / 255.0) if a is not None else 1.0
        img = bpy.data.images.new(name, w, h, alpha=True)
        img.pixels.foreach_set(px[::-1].ravel())         # file row 0 is the top row; Blender's row 0 the bottom one
        img.filepath_raw = png
        img.file_format = "PNG"
        try:
            img.save()
        except RuntimeError as e:
            notes.append("picture %s: PNG not written: %s" % (pic, e))
        img.filepath = png
    else:
        img = bpy.data.images.load(png, check_existing=True)
        img.name = name
    img["si3d_picture"] = pic
    return img


# ------------------------------------------------------------------ materials

def _material_values(m):
    return {"shading": m["shading"], "ambient": m["ambient"], "diffuse": m["diffuse"], "specular": m["specular"],
            "exponent": m["exponent"], "reflectivity": m["reflectivity"], "transparency": m["transparency"],
            "refraction": m["refraction"], "blur": m.get("static_blur", 0), "blur_width": m.get("blur_width", 1.0)}


def _texture_values(t, as_uv):
    f = t.get("factors") or [1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
    v = {"method": 4 if as_uv else t["method"], "pixel_interp": t["pixel_interp"], "mask": t["mask"],
         "channel": t["channel"], "uscale": t["uscale"], "vscale": t["vscale"], "uoffset": t["uoffset"],
         "voffset": t["voffset"], "urepeat": max(1, t["urepeat"]), "vrepeat": max(1, t["vrepeat"]),
         "blend": t["blend"], "ambient": f[0], "diffuse": f[1], "specular": f[2], "reflectivity": f[3],
         "transparency": f[4], "roughness": f[6], "reflection_map": f[7], "alt_u": t["alt_u"], "alt_v": t["alt_v"],
         "swap_uv": t["swap_uv"], "black_white": t["black_white"]}
    crop = t.get("crop") or []
    if len(crop) == 5:      # the dump's [switch, x0, x1, y0, y1] (the record's +0x2e, +0x30 to +0x36)
        v["crop_flag"], v["crop"] = crop[0], list(crop[1:])
    pr = t.get("projection") or {}
    if not as_uv:           # a projection's frame (unit imp3): rotation +0x94, scaling +0xac, translation +0xa0
        for k in ("rotation", "scaling", "translation"):
            if pr.get(k) is not None:
                v[k] = list(pr[k])
    return v


PROJECTED = (1, 2, 3, 5, 6)


def _xyz_rotation(rot):
    """si-math's matrix_xyz_rotation (radians), the rows of the row-vector matrix (as blend2db's simat.rs has it)."""
    sx, cx = math.sin(rot[0]), math.cos(rot[0])
    sy, cy = math.sin(rot[1]), math.cos(rot[1])
    sz, cz = math.sin(rot[2]), math.cos(rot[2])
    return ((cy * cz, cy * sz, -sy),
            (sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy),
            (sx * sz + cx * sy * cz, sz * cx * sy - cz * sx, cx * cy))


def _plane_in_object_space(g16, rot):
    """Whether a plane projection (XY, XZ, YZ) with the rotation `rot` of a model with the global matrix `g16`
    (Softimage, row vectors) has an exact form for blend2db, which writes the vertices in world space: the columns of
    G3^-1 r orthogonal (blend2db simat::texture2d_object_space; a mirror is taken by the axis the plane does not read)."""
    g = Matrix((g16[0:3], g16[4:7], g16[8:11]))
    try:
        gi = g.inverted()
    except ValueError:
        return False
    k = gi @ Matrix(_xyz_rotation(rot))
    cols = [Vector((k[0][j], k[1][j], k[2][j])) for j in range(3)]
    if any(c.length == 0.0 for c in cols):
        return False
    cols = [c.normalized() for c in cols]
    return all(abs(cols[i].dot(cols[j])) <= 1e-6 for i, j in ((0, 1), (0, 2), (1, 2)))


# ------------------------------------------------------------------ lights: the Softimage values kept for the engine

SI_LIGHT = "si3d_light"                 # JSON: the Softimage light record's values (SI_LIGHT_KEYS)
SI_LIGHT_KEY = "si3d_light_key"         # JSON: the Blender values the import made of them (_light_key)
SI_LIGHT_FRAMES = "si3d_light_frames"   # JSON, animated colour only: {frame: {"color": Softimage, "key": Blender}}
SI_LIGHT_KEYS = ("type", "color", "cone", "spread", "falloff", "falloff_start", "falloff_end", "shadow", "umbra",
                 "shadow_map")


def _light_key(li):
    """The Blender light values an import sets (type, colour, energy, spot size and blend, shadow switch)."""
    return [li.type] + [float(c) for c in li.color] + [float(li.energy), float(getattr(li, "spot_size", 0.0)),
                                                       float(getattr(li, "spot_blend", 0.0)), bool(li.use_shadow)]


def _same_key(a, b):
    if len(a) != len(b) or a[0] != b[0]:
        return False
    return all((abs(x - y) <= 1e-5 * (1.0 + abs(y))) if isinstance(y, float) else x == y for x, y in zip(a[1:], b[1:]))


def softimage_light(ob, frame=None):
    """The Softimage values of a light this importer made (dict of SI_LIGHT_KEYS: type 1 point, 2 spot, 3 infinite,
    colour as the scene's record has it, cone and spread in radians, fall-off, shadow type), or None when the object is
    not such a light or its type, colour, energy, spot size or blend or shadow switch were changed since the import.
    `frame`: the frame whose values are asked for (an animated colour); the light's current values are compared.

    For the engine's "This Blender scene" source: blend2db fits a Softimage colour to Blender's light so that the
    engine matches Cycles, which makes an imported scene brighter than the Softimage scene; with these values the
    exporter hands the scene's own light over instead (export._softimage_light, the light's "si" entry; blend2db
    writes it as it is: units import2, finish1)."""
    li = getattr(ob, "data", None)
    if getattr(ob, "type", None) != "LIGHT" or li is None:
        return None
    li = getattr(li, "original", li)
    try:
        values = json.loads(li[SI_LIGHT])
        key = json.loads(li[SI_LIGHT_KEY])
        frames = json.loads(li[SI_LIGHT_FRAMES]) if SI_LIGHT_FRAMES in li else None
    except (KeyError, TypeError, ValueError):
        return None
    now = _light_key(getattr(ob, "data", li))
    if frames is not None and frame is not None and str(frame) in frames:
        f = frames[str(frame)]
        if not _same_key(now, f["key"]):
            return None
        values["color"] = f["color"]
        return values
    return values if _same_key(now, key) else None


class Builder:
    def __init__(self, context, d, opts):
        self.context = context
        self.d = d
        self.o = opts
        self.notes = []
        self.counts = {"objects": 0, "meshes": 0, "faces": 0, "curves": 0, "empties": 0, "lights": 0, "cameras": 0,
                       "materials": 0, "images": 0, "keyframed": 0, "shape_keyed": 0}
        self.models = d["models"]
        self.by_id = {m["id"]: m for m in self.models}
        self.textures = {t["id"]: t for t in d.get("textures", [])}
        self.textures3d = {t["id"]: t for t in d.get("textures3d", [])}
        self._uv_same = {}
        self.materials = {m["id"]: m for m in d.get("materials", [])}
        self.objects = {}           # model id -> object
        self.meshes = {}            # model id -> mesh data (for geometry_of)
        self.blender_materials = {}
        self.images = {}
        self.uv_texture = {}        # material id -> texture id that the mesh's render UV layer carries (first texture)
        try:
            from . import material_nodes
            self.mn = material_nodes
        except ImportError:
            self.mn = None
        self.shape_floats = 0

    # -------------------------------------------------------------- collection
    def run(self):
        sc = self.d["scene"]
        name = _name(sc.get("name"), "Softimage scene")
        self.col = bpy.data.collections.new(name)
        self.parts = {}
        self.context.scene.collection.children.link(self.col)
        self.col["si3d_scene"] = sc.get("path", "")
        self.col["si3d_database"] = sc.get("database", "")
        self.frames = self.d.get("frames") or []
        self.frame_index = {f["frame"]: k for k, f in enumerate(self.frames)}
        for m in self.models:
            self.model(m)
        self.parenting()
        if self.o.get("animation", True):
            self.animate()
        self.camera()
        self.lights()
        if self.o.get("scene_settings", True):
            self.scene_settings()
        # the mental ray shaders' stand-ins (sky, water; unit sky1): nothing happens for a scene without them
        from . import mrfx
        self.notes += mrfx.import_shaders(self)
        return self.col

    def link(self, ob, part=None):
        """Links `ob` into the scene's collection, or into its child collection `part` ("camera", "lights")."""
        col = self.col
        if part is not None:
            col = self.parts.get(part)
            if col is None:
                col = self.parts[part] = bpy.data.collections.new("%s %s" % (self.col.name, part))
                self.col.children.link(col)
        col.objects.link(ob)
        self.counts["objects"] += 1
        return ob

    # -------------------------------------------------------------- models
    def geometry_matrix(self, m):
        """Blender matrix taking the dump's vertices of `m` to the object's local frame (None: they are local)."""
        lat = m.get("latdep")
        if lat is None or lat not in self.by_id:
            return None
        own = si_matrix(m["global_matrix"])
        geo = si_matrix(self.by_id[lat]["global_matrix"])
        try:
            return own.inverted() @ geo
        except ValueError:
            self.notes.append("%s: its matrix has no inverse; lattice-dependent geometry left in place" % m.get("name"))
            return None

    def model(self, m):
        if m["dummy"]:
            return
        name = _name(m["name"], "%s.%d" % (KIND_NAMES.get(m["kind"], "node"), m["id"]))
        g = m.get("geometry")
        data = None
        if m.get("geometry_of") is not None and m["geometry_of"] in self.meshes and m.get("latdep") is None:
            data = self.meshes[m["geometry_of"]]
        elif g is not None:
            t = g.get("type")
            if t == "mesh":
                data = self.mesh_from_polygons(name, m, g)
            elif t == "triangles":
                data = self.mesh_from_triangles(name, m, g)
            elif t == "spline":
                data = self.curve_from_spline(name, g)
        if data is not None and isinstance(data, bpy.types.Mesh):
            self.meshes[m["id"]] = data
        ob = bpy.data.objects.new(name, data)
        if data is None:
            ob.empty_display_type = "SPHERE" if m["kind"] == 5 else "PLAIN_AXES"
            ob.empty_display_size = 0.5 if m["kind"] == 5 else 1.0
            self.counts["empties"] += 1
        self.link(ob)
        if data is not None and m.get("geometry_of") is not None and isinstance(data, bpy.types.Mesh):
            self._own_texture3d_slots(ob, m)
        ob.matrix_world = si_matrix(m["global_matrix"])
        ob["si3d_id"] = m["id"]
        ob["si3d_kind"] = m["kind"]
        ob["si3d_kind_name"] = KIND_NAMES.get(m["kind"], "kind %d" % m["kind"])
        ob["si3d_algorithm"] = m.get("algorithm", 0)
        _props(ob, "si3d_", m, ("scaling", "rotation", "translation", "normals_flipped", "visible", "texture3d"))
        if m.get("visible", 1) == 0 or m.get("algorithm") == 13:
            ob.hide_render = True
        self.objects[m["id"]] = ob

    def _materials_of(self, m, me, own):
        ids = m.get("materials") or []
        for mid in ids:
            me.materials.append(self.material(mid, own, _texture3d_id(m)))
        return ids

    def _own_texture3d_slots(self, ob, m):
        """A node that shares another node's mesh but has another 3D texture of its own: its slots hold its own
        materials (linked to the object, not to the shared mesh)."""
        owner = self.by_id.get(m["geometry_of"]) or {}
        if _texture3d_id(owner) == _texture3d_id(m):
            return
        own = list(((owner.get("geometry") or {}).get("own_textures")) or [])
        for slot, mid in zip(ob.material_slots, m.get("materials") or []):
            slot.link = "OBJECT"
            slot.material = self.material(mid, own, _texture3d_id(m))

    def _orient(self, pts, normals):
        """True when the polygon's winding (Newell normal of pts) disagrees with the sum of its corner normals."""
        nx = ny = nz = 0.0
        n = len(pts)
        for i in range(n):
            a, b = pts[i], pts[(i + 1) % n]
            nx += (a[1] - b[1]) * (a[2] + b[2])
            ny += (a[2] - b[2]) * (a[0] + b[0])
            nz += (a[0] - b[0]) * (a[1] + b[1])
        sx = sum(c[0] for c in normals)
        sy = sum(c[1] for c in normals)
        sz = sum(c[2] for c in normals)
        return nx * sx + ny * sy + nz * sz < 0.0

    def _finish_mesh(self, me, verts, faces, loop_normals, face_mat, uv_layers, render_uv, name):
        """verts: Blender local points; faces: lists of vertex indices; loop_normals: per face per corner; uv_layers:
        {texture id: per face per corner (u, v)}; render_uv: per face per corner, the UV the renderer reads."""
        import numpy as np
        nv, nf = len(verts), len(faces)
        me.vertices.add(nv)
        me.vertices.foreach_set("co", np.asarray(verts, np.float32).ravel() if nv else np.zeros(0, np.float32))
        sizes = np.fromiter((len(f) for f in faces), np.int32, nf)
        starts = np.zeros(nf, np.int32)
        if nf:
            starts[1:] = np.cumsum(sizes)[:-1]
        nl = int(sizes.sum())
        me.loops.add(nl)
        me.polygons.add(nf)
        flat = np.fromiter((v for f in faces for v in f), np.int32, nl)
        me.loops.foreach_set("vertex_index", flat)
        me.polygons.foreach_set("loop_start", starts)
        me.polygons.foreach_set("material_index", np.asarray(face_mat, np.int32) if nf else np.zeros(0, np.int32))
        me.update(calc_edges=True)

        def put(layer_name, per_face):
            # the dump holds the renderer's texture coordinates (u, v); a mesh's UV (what the exporter hands back, and
            # what Blender's engines read) is (u, 1 - v): the renderer keeps (1 - u, v) of a mesh's pair when it makes
            # its vertices (SI_meshTriangles, crates/si-render softvertex.rs) and its UV method then takes one minus
            # each (SI_textureVerts, softtriangle.rs), so the scene file's own pairs come back. (It was one minus
            # both, which mirrored every texture left to right on the way back through the exporter: unit extra2,
            # MODEL-TVset, Label-Bottle, Puzzle-PuzzleGame.)
            lay = me.uv_layers.new(name=layer_name)
            a = np.fromiter((c for f in per_face for uv in f for c in uv), np.float64, 2 * nl)
            a[1::2] = 1.0 - a[1::2]
            lay.data.foreach_set("uv", a.astype(np.float32))
            return lay

        if render_uv is not None:
            lay = put("UVMap", render_uv)
            me.uv_layers.active = lay
            lay.active_render = True
        for tid in sorted(uv_layers):
            put("SI texture %d" % tid, uv_layers[tid])
        if nf:
            # a face none of whose corners has a normal (a faceted mesh: the dump's corner normals are zero, the
            # renderer shades it with the polygon's own normal; or normals the scene holds as NaN) is flat; a zero
            # custom normal is Blender's "the face's own" (it was +Z, which lit every such face alike: unit fidelity1)
            smooth = np.fromiter((any(Vector(n).length > 0 for n in f) for f in loop_normals), bool, nf)
            me.polygons.foreach_set("use_smooth", smooth)
            ln = [Vector(n) for f in loop_normals for n in f]
            try:
                me.normals_split_custom_set([tuple(n.normalized()) if n.length > 0 else (0.0, 0.0, 0.0) for n in ln])
            except (AttributeError, RuntimeError, ValueError) as e:
                self.notes.append("%s: custom normals not set: %s" % (name, e))
        me.update()
        self.counts["meshes"] += 1
        self.counts["faces"] += nf

    def mesh_from_polygons(self, name, m, g):
        from mathutils.geometry import tessellate_polygon
        me = bpy.data.meshes.new(name)
        gm = self.geometry_matrix(m)
        nm3 = gm.to_3x3().inverted_safe().transposed() if gm is not None else None
        sign = -1.0 if m.get("normals_flipped") else 1.0
        verts = []
        for p in g["vertices"]:
            v = si_point(p)
            verts.append(tuple(gm @ v) if gm is not None else tuple(v))
        own = list(g.get("own_textures") or [])
        mats = self._materials_of(m, me, own)
        hidden = set(g.get("hidden_polygons") or [])
        uvs = {int(k): v for k, v in (g.get("uv") or {}).items()}
        faces, normals, fmat, per_tex, render_uv = [], [], [], {t: [] for t in uvs}, []
        dropped = 0
        # a faceted mesh (mesh flag 0x2) renders every polygon with its own normal, whatever normals its corners
        # store (the renderer marks every edge and gives each corner the polygon's normal): its faces are made flat
        # (unit imp3; PICTURE-SKELETON's head, cube105, stores smooth ones). That normal follows the polygon's
        # winding, which is kept, as for a faceted mesh whose stored normals are zero.
        faceted = bool(g.get("faceted"))
        for pi, poly in enumerate(g["polygons"]):
            if pi in hidden:
                dropped += 1
                continue
            cn = g["corner_normals"][pi]
            local = g["polygon_material"][pi] if pi < len(g["polygon_material"]) else 0
            mid = mats[local] if local < len(mats) else None
            first_tex = self._first_uv_texture(mid, uvs, own)
            # contours: the outer one and holes, split by -1
            contours, cur = [], []
            for ci, v in enumerate(poly):
                if v < 0:
                    if cur:
                        contours.append(cur)
                    cur = []
                else:
                    cur.append(ci)
            if cur:
                contours.append(cur)
            contours = [self._clean(c, poly) for c in contours]
            contours = [c for c in contours if len(c) >= 3]
            if not contours:
                dropped += 1
                continue
            if len(contours) == 1:
                pieces = [contours[0]]
            else:
                lines = [[Vector(verts[poly[ci]]) for ci in c] for c in contours]
                flat = [ci for c in contours for ci in c]
                pieces = [[flat[a], flat[b], flat[c]] for a, b, c in tessellate_polygon(lines)]
            for piece in pieces:
                pn = []
                for ci in piece:
                    # a normal the scene has as NaN (written null): none, the face's own is used
                    n = Vector(cn[ci]) if ci < len(cn) and None not in cn[ci] else Vector((0.0, 0.0, 0.0))
                    n = C3 @ (n * sign)
                    if nm3 is not None:
                        n = nm3 @ n
                    pn.append(n)
                if not faceted and self._orient([verts[poly[ci]] for ci in piece], pn):
                    piece = piece[::-1]
                    pn = pn[::-1]
                faces.append([poly[ci] for ci in piece])
                normals.append([Vector((0.0, 0.0, 0.0))] * len(pn) if faceted else pn)
                fmat.append(local)
                for t, lay in uvs.items():
                    row = lay[pi]
                    per_tex[t].append([tuple(row[ci]) if ci < len(row) else (0.0, 0.0) for ci in piece])
                if first_tex is not None:
                    row = uvs[first_tex][pi]
                    render_uv.append([tuple(row[ci]) if ci < len(row) else (0.0, 0.0) for ci in piece])
                else:
                    render_uv.append([(0.0, 0.0)] * len(piece))
        if dropped:
            self.notes.append("%s: %d polygon(s) hidden or degenerate, left out" % (name, dropped))
        self._finish_mesh(me, verts, faces, normals, fmat, per_tex, render_uv if uvs else None, name)
        me["si3d_faceted"] = int(bool(g.get("faceted")))
        me["si3d_discontinuity_angle"] = g.get("discontinuity_angle", 0.0)
        return me

    @staticmethod
    def _clean(c, poly):
        """A contour without repeated consecutive vertices (Blender refuses a face that uses a vertex twice in a row)."""
        out = []
        for ci in c:
            if out and poly[out[-1]] == poly[ci]:
                continue
            out.append(ci)
        while len(out) > 1 and poly[out[0]] == poly[out[-1]]:
            out.pop()
        seen, uniq = set(), []
        for ci in out:
            if poly[ci] in seen:
                return out          # a vertex used twice (not in a row): kept, Blender tolerates it
            seen.add(poly[ci])
            uniq.append(ci)
        return uniq

    def _same_uv(self, a, b):
        """Whether textures a and b have the same coordinates (within 1e-4) on every model that has both, and at least
        one model has both."""
        key = (a, b)
        if key not in self._uv_same:
            seen = False
            same = True
            for m in self.d.get("models", []):
                uv = (m.get("geometry") or {}).get("uv") or {}
                la, lb = uv.get(str(a)), uv.get(str(b))
                if la is None or lb is None:
                    continue
                seen = True
                if len(la) != len(lb) or any(len(pa) != len(pb) or any(
                        abs(ca[0] - cb[0]) > 1e-4 or abs(ca[1] - cb[1]) > 1e-4 for ca, cb in zip(pa, pb))
                        for pa, pb in zip(la, lb)):
                    same = False
                    break
            self._uv_same[key] = seen and same
        return self._uv_same[key]

    def _keeps_projection(self, t):
        """Whether a projected texture stays a projection instead of UV (unit imp3): it bumps (the record's bump flag
        +0x120, or a bump factor beyond 0.0001), its method is a plane (XY, XZ, YZ: the renderer bumps those along the
        plane's axes, the others along the coordinates as for UV), and on every model that carries it the projection
        has an exact form in the object's space with vertices written in world space (_plane_in_object_space); a model
        whose vertices come from another's frame (lattice) keeps UV."""
        if t["method"] not in (1, 2, 3):
            return False
        f = t.get("factors") or []
        if not (t.get("roughness") or (len(f) > 6 and abs(f[6]) > 1e-4)):
            return False
        key = ("projection", t["id"])
        if key not in self._uv_same:
            rot = ((t.get("projection") or {}).get("rotation")) or [0.0, 0.0, 0.0]
            seen, ok = False, True
            for m in self.models:
                g = m.get("geometry") or {}
                ids = [x for mid in (m.get("materials") or []) for x in (self.materials.get(mid) or {}).get("textures", [])]
                ids += list(g.get("own_textures") or [])
                if t["id"] not in ids:
                    continue
                seen = True
                if m.get("latdep") is not None or not _plane_in_object_space(m["global_matrix"], rot):
                    ok = False
                    break
            self._uv_same[key] = seen and ok
        return self._uv_same[key]

    def _first_uv_texture(self, mid, uvs, own):
        """The first texture of a polygon (the material's, then the node's own: the renderer's order) that has
        coordinates: the one the render UV layer carries."""
        texs = list(self.materials[mid]["textures"]) if mid in self.materials else []
        for t in texs + own:
            if t in uvs:
                return t
        return None

    def mesh_from_triangles(self, name, m, g):
        me = bpy.data.meshes.new(name)
        gm = self.geometry_matrix(m)
        nm3 = gm.to_3x3().inverted_safe().transposed() if gm is not None else None
        sign = -1.0 if m.get("normals_flipped") else 1.0
        verts = []
        for p in g["vertices"]:
            v = si_point(p)
            verts.append(tuple(gm @ v) if gm is not None else tuple(v))
        vn = []
        for n in g["normals"]:
            n = C3 @ (Vector(n) * sign)
            vn.append(nm3 @ n if nm3 is not None else n)
        own = list(g.get("own_textures") or [])
        mats = self._materials_of(m, me, own)
        uvs = {int(k): v for k, v in (g.get("uv") or {}).items()}
        faces, normals, fmat, per_tex, render_uv = [], [], [], {t: [] for t in uvs}, []
        bad = 0
        for ti, tri in enumerate(g["triangles"]):
            if len(tri) != 3 or min(tri) < 0 or len(set(tri)) < 3 or _sliver([verts[i] for i in tri]):
                bad += 1
                continue
            order = [0, 1, 2]
            pn = [vn[i] for i in tri]
            if self._orient([verts[i] for i in tri], pn):
                order = [2, 1, 0]
            faces.append([tri[k] for k in order])
            normals.append([pn[k] for k in order])
            local = g["triangle_material"][ti] if ti < len(g["triangle_material"]) else 0
            fmat.append(local)
            mid = mats[local] if local < len(mats) else None
            for t, lay in uvs.items():
                per_tex[t].append([tuple(lay[ti][k]) for k in order])
            ft = self._first_uv_texture(mid, uvs, own)
            render_uv.append([tuple(uvs[ft][ti][k]) for k in order] if ft is not None else [(0.0, 0.0)] * 3)
        if bad:
            self.notes.append("%s: %d degenerate triangle(s) left out" % (name, bad))
        self._finish_mesh(me, verts, faces, normals, fmat, per_tex, render_uv if uvs else None, name)
        me["si3d_source"] = g.get("source", "")
        me["si3d_tessellation"] = g.get("tessellation", "")
        return me

    def curve_from_spline(self, name, g):
        cu = bpy.data.curves.new(name, "CURVE")
        cu.dimensions = "3D"
        keys = g.get("keys") or []
        if not keys:
            return cu
        kind = g.get("spline_kind", 0)
        if kind == 1 and all("prev" in k and "next" in k for k in keys):
            sp = cu.splines.new("BEZIER")
            sp.bezier_points.add(len(keys) - 1)
            for bp, k in zip(sp.bezier_points, keys):
                bp.co = si_point(k["pos"])
                bp.handle_left_type = bp.handle_right_type = "FREE"
                bp.handle_left = si_point(k["prev"])
                bp.handle_right = si_point(k["next"])
        else:
            sp = cu.splines.new("NURBS" if kind == 2 else "POLY")
            sp.points.add(len(keys) - 1)
            for pt, k in zip(sp.points, keys):
                p = si_point(k["pos"])
                pt.co = (p.x, p.y, p.z, 1.0)
            if kind == 2:
                sp.order_u = min(4, len(keys))
                sp.use_endpoint_u = True
        sp.use_cyclic_u = bool(g.get("closed"))
        cu["si3d_spline_kind"] = kind
        self.counts["curves"] += 1
        return cu

    def material(self, mid, own=(), own3d=None):
        """The Blender material of a Softimage material; a node with textures of its own (global textures) gets a
        copy that carries them too.  `own3d`: the node's own 3D texture, which the renderer applies where the
        material has none: such a node gets a copy of the material that carries it."""
        m = self.materials[mid]
        t3 = _texture3d_id(m)
        from_node = t3 is None and own3d is not None
        if from_node:
            t3 = own3d
        key = (mid, tuple(own), t3 if from_node else None)
        if key in self.blender_materials:
            return self.blender_materials[key]
        nm = _name(m.get("name"), "material.%d" % mid)
        if own:
            nm += " +texture %s" % ",".join(map(str, own))
        if from_node:
            nm += " +3D texture %s" % _name((self.textures3d.get(t3) or {}).get("name"), str(t3))
        mat = bpy.data.materials.new(nm)
        mat["si3d_id"] = mid
        _props(mat, "si3d_", m, ("shading", "ambient", "diffuse", "specular", "exponent", "reflectivity", "transparency",
                                  "refraction", "static_blur", "blur_width", "texture3d"))
        mat.diffuse_color = [_linear(c) for c in m["diffuse"][:3]] + [1.0 - min(max(m["transparency"], 0.0), 1.0)]
        # the set-up's back-face culling (on in most scenes) is the renderer's, for every surface; Blender holds it per
        # material (EEVEE and the viewport honour it, Cycles does not)
        mat.use_backface_culling = bool(self.d["settings"].get("back_culling", False))
        texs = [(self.textures[t], "LOCAL") for t in m.get("textures", []) if t in self.textures]
        texs += [(self.textures[t], "GLOBAL") for t in own if t in self.textures]
        rm = m.get("reflection_map")
        if rm is not None and rm in self.textures and rm not in (m.get("textures") or []) and rm not in own:
            # the material's reflection map (+0x6f4) the renderer took from a map outside the material's list: a node's
            # own reflection map becomes its material's, for every node of that material (SI_materials). Carried as a
            # texture of the material: a Reflection map method makes it the material's again when the scene is loaded,
            # and shading skips it as a 2D texture (unit fidelity2: Env_Sunset, Env_Moonscape)
            texs.append((self.textures[rm], "LOCAL"))
        if self.mn is not None:
            node = self.mn.new_material_node(mat, _material_values(m))
            first = None
            for t, scope in texs[:4]:
                img = self.image(t)
                if img is None:
                    continue
                # the first texture is the render UV layer's; a later projected one joins it when the renderer gives it
                # the same coordinates on every model (unit extra2: Torch_Fire's flame grid, flamme and fire_shade2,
                # both YZ; the second, the flame's textured shadow, was left a projection the engine does not carry)
                # a bump-mapped plane projection keeps its projection when blend2db can carry it exactly: made UV it
                # bumps along the UV frame, not the plane's axes (unit imp3: MATTER-displacement, Missile, Sand_Gust)
                as_uv = bool(self.o.get("projected_as_uv", True) and t["method"] in PROJECTED
                             and (first is None or self._same_uv(first, t["id"]))
                             and not self._keeps_projection(t))
                tn = self.mn.new_texture_node(node, img, _texture_values(t, as_uv), scope=scope)
                tn["si3d_texture_id"] = t["id"]
                tn["si3d_method"] = t["method"]
                _props(tn, "si3d_", t, ("crop", "band_u", "band_v", "repeat", "sequence", "projection", "picture"))
                if first is None:
                    first = t["id"]
            if len(texs) > 4:
                self.notes.append("material %s: %d textures, the node takes 4" % (mat.name, len(texs)))
            if t3 is not None and t3 in self.textures3d:
                self.texture3d_node(node, self.textures3d[t3])
            try:
                self.mn.refresh(mat.node_tree)
            except Exception:   # noqa: BLE001 - only the node's fault display
                pass
        else:
            self._principled(mat, m, [t for t, _ in texs])
        if t3 is not None and (self.mn is None or t3 not in self.textures3d):
            self.notes.append("material %s: a 3D (procedural) texture, not built" % mat.name)
        self.blender_materials[key] = mat
        self.counts["materials"] += 1
        return mat

    def texture3d_node(self, mat_node, t):
        """A Softimage 3D Texture node on the material node's 3D Texture input, with the record's values."""
        N = self.mn.N
        tree = mat_node.id_data
        tn = tree.nodes.new(self.mn.TEXTURE3D)
        tn.location = (mat_node.location.x - 320.0, mat_node.location.y - 1300.0)
        tn.kind = N.ident_of(N.KIND3D, t.get("kind", 0), "MARBLE")
        tn.mask = N.ident_of(N.MASK3D, t.get("mask", 3), "BLEND")
        # the renderer reads channel 2 as the intensity and anything else as the alpha
        tn.channel = "INTENSITY" if t.get("channel") == 2 else "ALPHA"
        for k in ("spacing", "taper", "strength", "power", "blending", "ambient", "diffuse", "specular", "reflection",
                  "transparency", "roughness"):
            if t.get(k) is not None:
                setattr(tn, k, t[k])
        tn.iterations = min(max(int(t.get("iterations", 1)), 1), 32)
        for k, c in enumerate((t.get("colours") or [])[:5]):
            setattr(tn, "colour%d" % (k + 1), [min(max(float(v), 0.0), 1.0) for v in (list(c) + [0.0] * 4)[:4]])
        for k, v in enumerate((t.get("factors") or [])[:3]):
            setattr(tn, "stop%d" % (k + 2), min(max(float(v), 0.0), 1.0))
        for k in ("scaling", "rotation", "translation"):
            if t.get(k) is not None:
                setattr(tn, k, t[k][:3])
        tn["si3d_texture3d_id"] = t["id"]
        tn.name = tn.label = "3D texture %s" % _name(t.get("name"), str(t["id"]))
        tree.links.new(tn.outputs[0], mat_node.inputs["3D Texture"])
        return tn

    def _principled(self, mat, m, texs):
        """Without the Softimage nodes (an older add-on): a Principled BSDF and the first picture."""
        try:
            mat.use_nodes = True
        except AttributeError:
            pass
        tree = mat.node_tree
        p = next((n for n in tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if p is None:
            return
        p.inputs["Base Color"].default_value = [_linear(c) for c in m["diffuse"][:3]] + [1.0]
        p.inputs["Alpha"].default_value = 1.0 - min(max(m["transparency"], 0.0), 1.0)
        for t in texs:
            img = self.image(t)
            if img is None:
                continue
            tn = tree.nodes.new("ShaderNodeTexImage")
            tn.image = img
            tn.location = (p.location.x - 320, p.location.y)
            tree.links.new(tn.outputs["Color"], p.inputs["Base Color"])
            break

    def image(self, t):
        if t["id"] in self.images:
            return self.images[t["id"]]
        img = None
        if not t.get("found"):
            self.notes.append("texture %d: picture %s not found in the install" % (t["id"], t.get("picture")))
        else:
            img = picture_image(t["file"], self.notes)
            if img is not None:
                self.counts["images"] += 1
        if img is None and self.d.get("default_picture"):
            # the renderer reads its default picture (noIcon.pic of the resource directory) for a picture it cannot read
            img = picture_image(self.d["default_picture"], self.notes)
            if img is not None:
                self.notes.append("texture %d: the renderer's default picture in its place, as the renderer does"
                                  % t["id"])
        if t.get("sequence"):
            self.notes.append("texture %d: a picture sequence; its first picture only" % t["id"])
        self.images[t["id"]] = img
        return img

    # -------------------------------------------------------------- hierarchy and animation
    def parent_of(self, m):
        f = m.get("father")
        while f is not None and f in self.by_id and self.by_id[f]["dummy"]:
            f = self.by_id[f].get("father")
        return f if f in self.objects else None

    def parenting(self):
        if not self.o.get("hierarchy", True):
            return
        for m in self.models:
            ob = self.objects.get(m["id"])
            f = self.parent_of(m)
            if ob is None or f is None:
                continue
            w = ob.matrix_world.copy()
            ob.parent = self.objects[f]
            ob.matrix_parent_inverse = Matrix.Identity(4)
            pw = si_matrix(self.by_id[f]["global_matrix"])
            ob.matrix_basis = pw.inverted_safe() @ w
            # A child under a parent scaled unevenly can have a sheared local matrix, which Blender's
            # location/rotation/scale cannot hold; the rest goes into the parent inverse so that the
            # object still sits at the program's own global matrix.
            rest = pw.inverted_safe() @ w @ ob.matrix_basis.inverted_safe()
            if max(abs(rest[i][j] - (i == j)) for i in range(4) for j in range(4)) > 1e-5:
                ob.matrix_parent_inverse = rest

    def _world_at(self, mid, k):
        mat = self.frames[k]["matrices"].get(str(mid)) if self.frames else None
        return si_matrix(mat if mat is not None else self.by_id[mid]["global_matrix"])

    def animate(self):
        if len(self.frames) < 2:
            return
        animated = set(self.d.get("animated_models") or [])
        hier = self.o.get("hierarchy", True)
        for mid in sorted(animated):
            ob = self.objects.get(mid)
            if ob is None:
                continue
            m = self.by_id[mid]
            f = self.parent_of(m) if hier else None
            basis = []
            for k in range(len(self.frames)):
                w = self._world_at(mid, k)
                basis.append((self._world_at(f, k) @ ob.matrix_parent_inverse).inverted_safe() @ w
                             if f is not None else w)
            self.key_matrices(ob, basis)
            self.counts["keyframed"] += 1
        self.shape_keys()

    def key_matrices(self, ob, mats):
        ob.rotation_mode = "XYZ"
        locs, rots, scls = [], [], []
        prev = None
        for mtx in mats:
            loc, q, s = mtx.decompose()
            e = q.to_euler("XYZ", prev) if prev is not None else q.to_euler("XYZ")
            prev = e
            locs.append(tuple(loc))
            rots.append(tuple(e))
            scls.append(tuple(s))
        frames = [f["frame"] for f in self.frames]
        ob.matrix_basis = mats[0]
        put_keys(ob, "location", frames, locs)
        put_keys(ob, "rotation_euler", frames, rots)
        put_keys(ob, "scale", frames, scls)

    def shape_keys(self):
        deformed = self.d.get("deformed_models") or []
        if not deformed or len(self.frames) < 2:
            return
        for mid in deformed:
            ob = self.objects.get(mid)
            m = self.by_id[mid]
            if ob is None or ob.type != "MESH" or (m.get("geometry") or {}).get("type") != "mesh":
                continue
            me = ob.data
            if me.users > 1:
                ob.data = me = me.copy()
            nv = len(me.vertices)
            per = [f.get("vertices", {}).get(str(mid)) for f in self.frames]
            if any(p is None or len(p) != nv for p in per):
                self.notes.append("%s: per-frame vertices do not match the mesh; deformation not carried" % ob.name)
                continue
            if self.shape_floats + nv * 3 * len(per) > SHAPE_KEY_CAP:
                self.notes.append("%s: deformation over the shape-key budget; first frame only" % ob.name)
                continue
            self.shape_floats += nv * 3 * len(per)
            import numpy as np
            gm = self.geometry_matrix(m)
            basis = ob.shape_key_add(name="Basis", from_mix=False)
            basis.interpolation = "KEY_LINEAR"
            frames = [f["frame"] for f in self.frames]
            for k, pts in enumerate(per):
                a = np.asarray(pts, np.float64)
                b = np.empty_like(a)
                b[:, 0], b[:, 1], b[:, 2] = a[:, 0], -a[:, 2], a[:, 1]
                if gm is not None:
                    g = np.array(gm, np.float64)
                    b = b @ g[:3, :3].T + g[:3, 3]
                key = ob.shape_key_add(name="frame %d" % frames[k], from_mix=False)
                key.data.foreach_set("co", b.astype(np.float32).ravel())
                key.value = 0.0
            # each frame's key is 1 at its frame and 0 at the neighbours (linear in between)
            sk = me.shape_keys
            sk.use_relative = True
            for k, kb in enumerate(sk.key_blocks[1:]):
                pts = []
                if k > 0:
                    pts.append((frames[k - 1], 0.0))
                pts.append((frames[k], 1.0))
                if k + 1 < len(frames):
                    pts.append((frames[k + 1], 0.0))
                put_keys(sk, 'key_blocks["%s"].value' % kb.name, [p[0] for p in pts], [(p[1],) for p in pts])
            self.counts["shape_keyed"] += 1

    # -------------------------------------------------------------- camera, lights, scene
    def camera(self):
        c = self.d.get("camera")
        if not c:
            return
        s = self.d["settings"]
        cam = bpy.data.cameras.new("Camera")
        ob = self.link(bpy.data.objects.new("Camera", cam), "camera")
        self.counts["cameras"] += 1
        w, h = s["width"], s["height"]
        aspect = w / h if h else 1.0

        def lens_of(fov_v):
            cam.sensor_fit = "AUTO"
            cam.sensor_width = 36.0
            fov_big = 2.0 * math.atan(math.tan(fov_v / 2.0) * aspect) if aspect >= 1.0 else fov_v
            return 36.0 / (2.0 * math.tan(fov_big / 2.0))

        cam.lens = lens_of(c["fov"])
        cam.clip_start = max(c["near"], 1e-6)
        cam.clip_end = c["far"]
        ob.matrix_world = C4 @ Matrix([c["camera_matrix"][i:i + 4] for i in (0, 4, 8, 12)]).transposed()
        _props(cam, "si3d_", c, ("fov", "roll", "position", "interest", "near", "far", "dof", "dof_values", "window"))
        self.context.scene.camera = ob
        if self.o.get("animation", True) and len(self.frames) > 1:
            cams = [f.get("camera") or c for f in self.frames]
            mats = [C4 @ Matrix([x["camera_matrix"][i:i + 4] for i in (0, 4, 8, 12)]).transposed() for x in cams]
            if any(mats[k] != mats[0] for k in range(len(mats))):
                self.key_matrices(ob, mats)
                self.counts["keyframed"] += 1
            lenses = [lens_of(x["fov"]) for x in cams]
            if any(abs(v - lenses[0]) > 1e-6 for v in lenses):
                put_keys(cam, "lens", [f["frame"] for f in self.frames], [(v,) for v in lenses])
        if c.get("dof"):
            self.notes.append("camera depth of field (mode %d): kept for the Softimage renderer, not set up for EEVEE or "
                              "Cycles" % c["dof"])

    def _scene_centre(self):
        pts = []
        for ob in self.objects.values():
            if ob.type == "MESH" and not ob.hide_render and len(ob.data.vertices):
                pts += [ob.matrix_world @ Vector(b) for b in ob.bound_box]
        if not pts:
            return Vector((0.0, 0.0, 0.0))
        lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
        hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
        return (lo + hi) / 2.0

    def _light_values(self, l, centre):
        """(Blender type, colour, energy, location, rotation quaternion) of a Softimage light record."""
        kind = {1: "POINT", 2: "SPOT", 3: "SUN", 4: "SUN"}.get(l["type"], "POINT")
        lin = [_linear(c) for c in l["color"][:3]]
        peak = max(lin) if max(lin) > 0 else 0.0
        col = [c / peak for c in lin] if peak > 0 else [1.0, 1.0, 1.0]
        pos = si_point(l["position"])
        if kind == "SUN":
            d = -pos if pos.length > 0 else Vector((0.0, 0.0, -1.0))
            loc = centre + pos.normalized() * 10.0 if pos.length > 0 else centre
            energy = math.pi * peak
        else:
            d = si_point(l["interest"]) - pos
            loc = pos
            dist = max((pos - centre).length, 1.0)
            energy = 4.0 * math.pi * math.pi * dist * dist * peak
        if d.length == 0:
            d = Vector((0.0, 0.0, -1.0))
        q = d.to_track_quat("-Z", "Y")
        return kind, col, energy, loc, q

    def _selective(self, ob, l):
        """A selective light lights (and its shadows fall from) only the models the dump lists ("lit_models"): Blender
        light and shadow linking, one collection of those objects as the light's receivers and blockers."""
        ids = l.get("lit_models")
        if not isinstance(ids, list) or not hasattr(ob, "light_linking"):
            self.notes.append("light %s: selective (lights only some models): it lights everything here" % ob.name)
            return
        coll = bpy.data.collections.new("%s lit" % ob.name)
        n = 0
        for i in ids:
            o = self.objects.get(i)
            if o is not None and o.name not in coll.objects:
                coll.objects.link(o)
                n += 1
        ob.light_linking.receiver_collection = coll
        ob.light_linking.blocker_collection = coll
        self.notes.append("light %s: selective, lights %d model(s) (light and shadow linking, collection '%s')"
                          % (ob.name, n, coll.name))

    def lights(self):
        lights = self.d.get("lights") or []
        if not lights:
            return
        self.context.view_layer.update()
        centre = self._scene_centre()
        only_default = all(l.get("default") for l in lights)
        for k, l in enumerate(lights):
            if l.get("default") and not self.o.get("default_light", True):
                self.context.scene["si3d_default_light"] = 1
                continue
            kind, col, energy, loc, q = self._light_values(l, centre)
            li = bpy.data.lights.new(_name(l.get("name"), "light.%d" % k), kind)
            li.color = col
            li.energy = energy
            li.use_shadow = l.get("shadow", 1) in (2, 3, 4)
            if kind == "SPOT":
                size = min(max(l["cone"] + l["spread"], 0.01), math.pi)
                li.spot_size = size
                cos_o, cos_i = math.cos(size / 2.0), math.cos(l["cone"] / 2.0)
                li.spot_blend = min(max((cos_i - cos_o) / (1.0 - cos_o), 0.0), 1.0) if cos_o < 1.0 else 0.0
            _props(li, "si3d_", l, ("type", "color", "position", "interest", "cone", "spread", "falloff",
                                    "falloff_start", "falloff_end", "shadow", "umbra", "selective", "default"))
            # the Softimage values for the engine's "This Blender scene" source, with the Blender values they were
            # made into (softimage_light below hands them out only while the light is left as imported)
            li[SI_LIGHT] = json.dumps({k: l.get(k) for k in SI_LIGHT_KEYS})
            li[SI_LIGHT_KEY] = json.dumps(_light_key(li))
            ob = self.link(bpy.data.objects.new(li.name, li), "lights")
            ob.location = loc
            ob.rotation_mode = "QUATERNION"
            ob.rotation_quaternion = q
            ob.hide_render = not l.get("on", True)
            self.counts["lights"] += 1
            if l.get("selective"):
                self._selective(ob, l)
            if self.o.get("animation", True) and len(self.frames) > 1:
                per = [(f.get("lights") or [])[k] if k < len(f.get("lights") or []) else l for f in self.frames]
                vals = [self._light_values(x, centre) for x in per]
                frames = [f["frame"] for f in self.frames]
                if any(v[3] != vals[0][3] for v in vals):
                    put_keys(ob, "location", frames, [tuple(v[3]) for v in vals])
                if any(v[4] != vals[0][4] for v in vals):
                    qs, prev = [], None
                    for v in vals:
                        qq = v[4].copy()
                        if prev is not None and prev.dot(qq) < 0:
                            qq.negate()
                        qs.append(tuple(qq))
                        prev = qq
                    put_keys(ob, "rotation_quaternion", frames, qs)
                if any(v[1] != vals[0][1] for v in vals):
                    put_keys(li, "color", frames, [tuple(v[1]) for v in vals])
                if any(abs(v[2] - vals[0][2]) > 1e-9 for v in vals):
                    put_keys(li, "energy", frames, [(v[2],) for v in vals])
                if any(x.get("color") != l.get("color") for x in per):
                    # an animated colour: the Softimage colour and the Blender key of every frame
                    li[SI_LIGHT_FRAMES] = json.dumps({str(fr): {"color": x.get("color"), "key": [li.type] + list(v[1]) +
                                                      [v[2]] + _light_key(li)[5:]} for fr, x, v in zip(frames, per, vals)})
        if only_default:
            self.notes.append("the scene has no light of its own: Softimage's default light is %s" %
                              ("created as a point light" if self.o.get("default_light", True) else "not created"))

    def scene_settings(self):
        s = self.d["settings"]
        sc = self.context.scene
        sc.frame_start = s["frame_first"]
        sc.frame_end = max(s["frame_last"], s["frame_first"])
        sc.frame_step = max(1, s.get("frame_step", 1))
        rate = s.get("frame_rate") or 30.0
        fps = max(1, int(round(rate)))
        sc.render.fps = fps
        sc.render.fps_base = fps / rate
        sc.render.resolution_x = max(4, s["width"])
        sc.render.resolution_y = max(4, s["height"])
        sc.render.resolution_percentage = 100
        pr = s.get("pixel_ratio") or 1.0
        sc.render.pixel_aspect_x, sc.render.pixel_aspect_y = (pr, 1.0) if pr >= 1.0 else (1.0, 1.0 / pr)
        # a Softimage picture is transparent (alpha 0) where nothing is hit; without this the engine's "This Blender
        # scene" source fills the background with the world colour at alpha 1
        sc.render.film_transparent = True
        # the set-up's sub-region (the renderer draws only that rectangle of the picture) becomes Blender's render
        # region box, left switched off; ticked, both of the engine's sources render the box (the "This Blender
        # scene" source since unit finish1: the generated set-up gets the sub-region)
        crop = s.get("crop")
        r = sc.render
        r.use_border = False
        if crop and s["width"] > 0 and s["height"] > 0:
            W, H = float(s["width"]), float(s["height"])
            r.use_crop_to_border = False
            r.border_min_x = min(max(crop["left"] / W, 0.0), 1.0)
            r.border_max_x = min(max(1.0 - crop["right"] / W, 0.0), 1.0)
            r.border_min_y = min(max(crop["bottom"] / H, 0.0), 1.0)
            r.border_max_y = min(max(1.0 - crop["top"] / H, 0.0), 1.0)
            sc["si3d_crop"] = json.dumps(crop)
            self.notes.append("the scene's set-up renders a sub-region (%d x %d pixels): it is the Output > Render "
                              "Region box, switched off; tick it for the sub-region"
                              % (crop.get("width", 0), crop.get("height", 0)))
        sc.frame_set(s["frame_first"])
        sc["si3d_back_culling"] = bool(s.get("back_culling", False))
        self.world()
        # the engine's pictures are display values (sRGB bytes): shown as they are, without a look or dither
        sc.view_settings.view_transform = "Standard"
        sc.view_settings.look = "None"
        sc.render.dither_intensity = 0.0
        st = getattr(sc, "si3d", None)
        if st is not None:
            st.frame_mode = "ALONE"             # F12 renders the current frame alone (not the whole range first)
            # the scene's own MAX_RAY_DEPTH and GLOBAL_SWITCHES (unit fidelity2): without them the generated set-up
            # traces reflections a set-up of depth 1 never shows
            if s.get("ray_depth"):
                st.ray_depth = min(max(int(s["ray_depth"]), 1), 32)
            aa = s.get("antialias")
            if isinstance(aa, dict) and aa.get("mode") in ("FIXED", "ADAPTIVE"):
                st.antialias = aa["mode"]
                st.aa_filter = min(max(int(aa.get("filter", 2)), 0), 16)
                st.aa_level = min(max(int(aa.get("level", 2)), 0), 16)
                st.aa_threshold = min(max(float(aa.get("threshold", 0.05)), 0.0), 1.0)
            sw = s.get("switches") or {}
            for k, prop in (("material", "use_materials"), ("textures_2d", "use_textures_2d"),
                            ("textures_3d", "use_textures_3d"), ("reflection", "use_reflection"),
                            ("refraction", "use_refraction"), ("shadows", "use_shadows")):
                if k in sw:
                    setattr(st, prop, bool(sw[k]))
        try:
            sc.render.engine = "FOGLE3D"
        except TypeError:
            pass

    def world(self):
        env = self.d["settings"].get("environment") or {}
        sc = self.context.scene
        w = bpy.data.worlds.new(_name(self.d["scene"].get("name"), "Softimage") + " world")
        sc.world = w
        amb = env.get("ambience") or [0.0, 0.0, 0.0]
        w["si3d_ambience"] = list(amb)
        w.color = (0.0, 0.0, 0.0)
        fog = env.get("fog") or {}
        if self.mn is None:
            return
        self.mn.enable_nodes(w)
        tree = w.node_tree
        out = next((n for n in tree.nodes if n.bl_idname == "ShaderNodeOutputWorld"), None)
        if out is None:
            out = tree.nodes.new("ShaderNodeOutputWorld")
        node = tree.nodes.new(self.mn.WORLD)
        node.location = (out.location.x - 300.0, out.location.y)
        node.fit_ambience = False
        node.ambience = [min(max(c, 0.0), 1.0) for c in amb[:3]]
        node.background = (0.0, 0.0, 0.0)
        fade = env.get("fade") or {}
        if fade.get("on"):
            # the depth fade: surfaces go to the fade colour between the two distances, and what nothing hits is the
            # fade colour at alpha 1 - transparency (the renderer's background; unit fidelity1: 18 corpus scenes)
            node.fade = True
            node.fade_start, node.fade_end = fade.get("start", 20.0), fade.get("end", 100.0)
            node.fade_colour = [min(max(c, 0.0), 1.0) for c in fade.get("color", [0.0, 0.0, 0.0])[:3]]
            node.fade_transparency = min(max(fade.get("transparency", 0.0), 0.0), 1.0)
        if fog.get("on"):
            node.fog = True
            node.fog_start, node.fog_end = fog.get("start", 20.0), fog.get("end", 10000.0)
            node.fog_colour = [min(max(c, 0.0), 1.0) for c in fog.get("color", [1.0, 1.0, 1.0])[:3]]
            node.fog_density = fog.get("density", 0.2)
            node.fog_base, node.fog_thickness = fog.get("base", -1.0), fog.get("thickness", 5.0)
        tree.links.new(node.outputs["World"], out.inputs["Surface"])
        try:
            self.mn.refresh(tree)
        except Exception:   # noqa: BLE001
            pass

    # -------------------------------------------------------------- report
    def report(self):
        d = self.d
        lines = ["Fogle3D import: %s" % d["scene"].get("path", ""), ""]
        lines.append("Made: " + ", ".join("%s %d" % (k, v) for k, v in self.counts.items()))
        c = d.get("counts", {})
        lines.append("Scene: " + ", ".join("%s %s" % (k, v) for k, v in c.items()))
        if d.get("not_carried"):
            lines += ["", "Not carried or carried differently in this scene:"]
            lines += ["  %d x %s" % (n["count"], n["what"]) for n in d["not_carried"]]
        if d.get("notes") or self.notes:
            lines += ["", "Notes:"] + ["  " + n for n in d.get("notes", []) + self.notes]
        lines += ["", "Never carried by the importer:"] + ["  " + n for n in ALWAYS_NOT_CARRIED]
        return "\n".join(lines)


def _fcurves_of(idblock):
    ad = idblock.animation_data
    act = ad.action if ad is not None else None
    if act is None:
        return None
    try:
        return act.fcurves                           # before Blender 4.4 (and the legacy proxy up to 4.5)
    except AttributeError:
        pass
    from bpy_extras import anim_utils
    cb = anim_utils.action_get_channelbag_for_slot(act, ad.action_slot)
    return cb.fcurves if cb is not None else None


def put_keys(idblock, path, frames, values):
    """Keyframes of `path` (one tuple per frame, one value per array index), linear, written in bulk."""
    import numpy as np
    n_idx = len(values[0])
    for i in range(n_idx):
        if n_idx == 1:
            idblock.keyframe_insert(path, frame=frames[0])
        else:
            idblock.keyframe_insert(path, index=i, frame=frames[0])
    fcs = _fcurves_of(idblock)
    if fcs is None:
        return
    for i in range(n_idx):
        fc = fcs.find(path, index=i)
        if fc is None:
            continue
        kp = fc.keyframe_points
        if len(kp) < len(frames):
            kp.add(len(frames) - len(kp))
        co = np.empty(2 * len(frames), np.float32)
        co[0::2] = frames
        co[1::2] = [v[i] for v in values]
        kp.foreach_set("co", co)
        kp.foreach_set("interpolation", np.full(len(frames), _LINEAR, np.int32))
        fc.update()


_LINEAR = bpy.types.Keyframe.bl_rna.properties["interpolation"].enum_items["LINEAR"].value


def import_dsc(context, filepath, dump=None, **opts):
    """Imports one scene; returns (collection, report text).  `dump`: an already made dump (tests), else the program
    runs on the install the add-on is set to."""
    if dump is None:
        root = opts.pop("install_root", None) or install_root(context)
        frames = "all" if opts.get("animation", True) else "first"
        dump = run_dump(filepath, root, frames=frames, deform=opts.get("deform", True), program=opts.pop("program", None))
    b = Builder(context, dump, opts)
    col = b.run()
    text = b.report()
    t = bpy.data.texts.get(REPORT_TEXT) or bpy.data.texts.new(REPORT_TEXT)
    t.clear()
    t.write(text + "\n")
    col["si3d_counts"] = json.dumps(b.counts)
    return col, text, b


# ------------------------------------------------------------------ operator and menu

class SI3D_OT_import_dsc(bpy.types.Operator):
    """Import a Softimage|3D 4.0 scene (.dsc) of your install as Blender objects, materials, lights, camera and animation"""
    bl_idname = "import_scene.softimage3d_dsc"
    bl_label = "Import Softimage 3D scene"
    bl_options = {"REGISTER", "UNDO", "PRESET"}

    filepath: StringProperty(subtype="FILE_PATH")
    # a drop on the 3D viewport or the outliner (SI3D_FH_import_dsc) fills these: the folder and the files' names
    directory: StringProperty(subtype="DIR_PATH", options={"HIDDEN", "SKIP_SAVE"})
    files: CollectionProperty(type=bpy.types.OperatorFileListElement, options={"HIDDEN", "SKIP_SAVE"})
    filter_glob: StringProperty(default="*.dsc", options={"HIDDEN"})
    animation: BoolProperty(name="Animation", default=True,
                            description="Bake every frame of the scene's range into keyframes (transforms, camera, lights)")
    deform: BoolProperty(name="Mesh deformation", default=True,
                         description="Shape-animated and enveloped meshes: one shape key per frame")
    hierarchy: BoolProperty(name="Hierarchy", default=True, description="Parent objects as the scene's tree does")
    default_light: BoolProperty(name="Default light", default=True,
                                description="A scene without lights of its own is lit by Softimage's default light "
                                            "(white, at 100, 100, 100): create it as a Blender light (the exporter "
                                            "gives a scene without lights a black light, so the engine would not add it)")
    projected_as_uv: BoolProperty(name="Projections as UV", default=True,
                                  description="A material's first texture projected by the renderer (planar, "
                                              "cylindrical, spherical) becomes a UV texture with the renderer's own "
                                              "coordinates, so it stays put when objects move or are edited")
    scene_settings: BoolProperty(name="Scene settings", default=True,
                                 description="Frame range, frame rate, resolution, world and render engine from the scene")

    def invoke(self, context, event):
        if self.filepath or len(self.files):
            return self.execute(context)        # a dropped file: no file browser
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def _paths(self):
        if len(self.files) and self.directory:
            return [os.path.join(self.directory, f.name) for f in self.files if f.name]
        return [self.filepath] if self.filepath else []

    def execute(self, context):
        paths = [bpy.path.abspath(p) for p in self._paths()]
        if not paths or not all(p.lower().endswith(".dsc") for p in paths):
            self.report({"ERROR"}, "Choose a Softimage scene file (.dsc)")
            return {"CANCELLED"}
        opts = {k: getattr(self, k) for k in ("animation", "deform", "hierarchy", "default_light", "projected_as_uv",
                                              "scene_settings")}
        done = 0
        for path in paths:
            try:
                col, text, b = import_dsc(context, path, **dict(opts))
            except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired) as e:
                self.report({"ERROR"}, "Fogle3D import of %s: %s" % (os.path.basename(path), e))
                continue
            done += 1
            self.report({"INFO"}, "Imported %s: %d objects (report in the text '%s')" % (col.name, b.counts["objects"],
                                                                                        REPORT_TEXT))
        return {"FINISHED"} if done else {"CANCELLED"}


def menu_func(self, context):
    self.layout.operator(SI3D_OT_import_dsc.bl_idname, text="Softimage 3D scene (.dsc)")


_FileHandler = getattr(bpy.types, "FileHandler", None)     # Blender 4.1 and later
if _FileHandler is not None:
    class SI3D_FH_import_dsc(_FileHandler):
        """A .dsc file dropped on the 3D viewport or the outliner runs the import (the operator's options as last used)."""
        bl_idname = "SI3D_FH_import_dsc"
        bl_label = "Softimage 3D scene"
        bl_import_operator = SI3D_OT_import_dsc.bl_idname
        bl_file_extensions = ".dsc"

        @classmethod
        def poll_drop(cls, context):
            return context.area is not None and context.area.type in {"VIEW_3D", "OUTLINER"}
    CLASSES = (SI3D_OT_import_dsc, SI3D_FH_import_dsc)
else:
    CLASSES = (SI3D_OT_import_dsc,)


# ------------------------------------------------------------------ files saved by convert_scenes.py open in Rendered

SI_OPEN_RENDERED = "si3d_open_rendered"     # scene flag set by re/tools/import1/convert_scenes.py


def open_rendered_views():
    """Every 3D view of the open file goes to Rendered shading (drawn by the Softimage 3D engine) when a scene of the
    file carries SI_OPEN_RENDERED.  Blender sets Rendered back to Solid whenever it reads a file, so the saved shading
    alone does not survive.  Returns the number of views set (0 when the engine offers no Rendered shading)."""
    if not any(s.get(SI_OPEN_RENDERED) for s in bpy.data.scenes):
        return 0
    n = 0
    for screen in bpy.data.screens:
        for area in screen.areas:
            for space in area.spaces:
                if space.type != "VIEW_3D":
                    continue
                try:
                    space.shading.type = "RENDERED"
                except TypeError:               # the engine has no view_draw: Blender does not offer Rendered
                    return n
                n += 1
    return n


@bpy.app.handlers.persistent
def _open_rendered(*_args):
    if not bpy.app.background:              # no viewport drawing in a background Blender
        open_rendered_views()


def register():
    for c in CLASSES:
        bpy.utils.register_class(c)
    bpy.types.TOPBAR_MT_file_import.append(menu_func)
    if _open_rendered not in bpy.app.handlers.load_post:
        bpy.app.handlers.load_post.append(_open_rendered)


def unregister():
    if _open_rendered in bpy.app.handlers.load_post:
        bpy.app.handlers.load_post.remove(_open_rendered)
    bpy.types.TOPBAR_MT_file_import.remove(menu_func)
    for c in reversed(CLASSES):
        bpy.utils.unregister_class(c)
