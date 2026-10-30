# SPDX-License-Identifier: GPL-3.0-or-later
"""Blender scene -> the data a Softimage database needs (our own code).

`scene_to_dict` reads the evaluated scene (meshes, curves/NURBS/text as meshes, lights, the
camera, materials, image textures, world colour, frame range) into plain Python data; `dump`
writes it as JSON.  `render_blender_scene` hands that JSON to the `blend2db` tool
(re/tools/builder2/blend2db), which writes a database the renderer loads, and renders it with
render-frames.  See re/out/lead/blender/design.md and re/tools/builder1/README.txt."""
import json
import math
import os


def _matrix(m):
    return [list(r) for r in m]


def _image_raw(img, texdir):
    """Writes the image's pixels, 8 bit, top row first, to `texdir` (header: b"SIRAW", u32 width, u32 height, u32 planes, then
    the planes' bytes interleaved); returns the path or None.  Byte images give their stored values; float images are encoded."""
    import struct
    import numpy as np
    w, h = img.size
    if w == 0 or h == 0 or not texdir:
        return None
    a = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(a)
    a = a.reshape(h, w, 4)[::-1]
    if img.is_float:
        c = np.clip(a[..., :3], 0.0, 1.0)
        a = a.copy()
        a[..., :3] = np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)
    b = np.rint(np.clip(a, 0.0, 1.0) * 255).astype(np.uint8)
    planes = 4 if (b[..., 3] != 255).any() else 3
    safe = "".join(ch if ch.isalnum() else "_" for ch in img.name) or "image"
    path = os.path.join(texdir, "%s.raw" % safe)
    with open(path, "wb") as fh:
        fh.write(b"SIRAW" + struct.pack("<III", w, h, planes))
        fh.write(b[..., :planes].tobytes())
    return path


def _image_mean(img):
    """The mean linear colour of an image (what its material is lit as when the texture stands in for the diffuse colour)."""
    import numpy as np
    w, h = img.size
    if w == 0 or h == 0:
        return [0.5, 0.5, 0.5]
    a = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(a)
    a = np.clip(a.reshape(-1, 4)[:, :3], 0.0, 1.0)
    if not img.is_float:    # stored display-encoded: linearise
        a = np.where(a <= 0.04045, a / 12.92, ((a + 0.055) / 1.055) ** 2.4)
    return [float(x) for x in a.mean(axis=0)]


def _material(mat, texdir=None):
    d = _material_values(mat, texdir)
    # Material > Settings > Backface Culling (unit finish1): blend2db switches the program's BACK_CULLING on when every
    # material it draws culls.  The key is there only when the switch is on, so other scenes' data stays as it was.
    if getattr(mat, "use_backface_culling", False):
        d = dict(d, backface_culling=True)
    return d


def _material_values(mat, texdir=None):
    from . import material_nodes    # a Softimage Material on the output comes first (unit nodes1)
    si = material_nodes.si_material(mat, texdir, _image_raw, _image_mean)
    if si is not None:
        return si
    d = {"name": mat.name, "diffuse": list(mat.diffuse_color), "specular": mat.specular_intensity,
         "roughness": mat.roughness, "metallic": mat.metallic, "alpha": mat.diffuse_color[3], "textures": []}
    if mat.use_nodes and mat.node_tree:
        for n in mat.node_tree.nodes:
            if n.bl_idname == "ShaderNodeBsdfPrincipled":
                d["diffuse"] = list(n.inputs["Base Color"].default_value)
                d["alpha"] = n.inputs["Alpha"].default_value
                d["roughness"] = n.inputs["Roughness"].default_value
                d["metallic"] = n.inputs["Metallic"].default_value
                d["ior"] = n.inputs["IOR"].default_value
                if "Emission Color" in n.inputs and n.inputs["Emission Strength"].default_value > 0:
                    e = n.inputs["Emission Color"].default_value
                    k = n.inputs["Emission Strength"].default_value
                    if max(e[0], e[1], e[2]) * k > 0.0:
                        d["emission"] = [e[0] * k, e[1] * k, e[2] * k]
                if "Transmission Weight" in n.inputs:      # Blender 4 and later (glass)
                    d["transmission"] = n.inputs["Transmission Weight"].default_value
            elif n.bl_idname == "ShaderNodeTexImage" and n.image:
                d["textures"].append({"image": n.image.filepath, "name": n.image.name, "raw": _image_raw(n.image, texdir),
                                      "mean": _image_mean(n.image)})
                links = material_nodes.image_links(n)    # Alpha and Mapping links the engine can carry (unit nodes1)
                if links:
                    d["textures"][-1]["si"] = links
    return d


def _diffuse(mat):
    """`_material(mat)["diffuse"]` alone: the material's colour, overridden by each Principled node's Base Color in node order.
    sample_frame needs only this; going through `_material` also read every image texture's pixels (`_image_mean`), once per
    material and sampled frame: 1.9 s of a 3.5 s frame on a 250-frame scene with a 512x512 texture (unit speed2)."""
    from . import material_nodes
    if material_nodes.material_node(mat) is not None:
        return None                  # a Softimage Material's colours are not sampled per frame
    d = list(mat.diffuse_color)
    if mat.use_nodes and mat.node_tree:
        for n in mat.node_tree.nodes:
            if n.bl_idname == "ShaderNodeBsdfPrincipled":
                d = list(n.inputs["Base Color"].default_value)
    return d


def _world_colour(world):
    """The uniform colour Cycles lights the scene with: the Background node's colour times its strength (Blender 5: the node
    tree always exists; `world.color` is only the viewport colour).  Anything else wired in is not read.  A Softimage World
    on the output gives its background colour (unit nodes1)."""
    from . import material_nodes
    node = material_nodes.world_node(world)
    if node is not None:
        return material_nodes.world_background(node)
    if world.use_nodes and world.node_tree:
        for n in world.node_tree.nodes:
            if n.bl_idname == "ShaderNodeBackground" and n.outputs["Background"].is_linked:
                c = n.inputs["Color"]
                if not c.is_linked:
                    s = n.inputs["Strength"].default_value
                    return [c.default_value[i] * s for i in range(3)]
                return list(world.color)
    return list(world.color)


def _split_by_texture(od, materials):
    """A Softimage 2D texture covers a whole model, a Blender image texture one material.  An object whose material slots carry
    different image textures is therefore cut into one model per texture (triangles keep their vertices, normals and UVs)."""
    def key(slot):
        name = od["materials"][slot] if 0 <= slot < len(od["materials"]) else None
        tex = [t["name"] for t in materials.get(name, {}).get("textures", []) if t.get("raw")] if name else []
        return tex[0] if tex else None
    groups = {}
    for ti, tp in enumerate(od["tri_poly"]):
        slot = od["poly_material"][tp] if tp < len(od["poly_material"]) else 0
        groups.setdefault(key(slot), []).append(ti)
    if len(groups) <= 1:
        return [od]
    parts = []
    for n, (k, tis) in enumerate(sorted(groups.items(), key=lambda kv: kv[1][0])):
        p = dict(od)
        p["name"] = od["name"] if n == 0 else "%s.part%d" % (od["name"], n)
        if n:
            p["source"] = od["name"]        # blend2db: the Blender object a part comes from, for light linking (fidelity2)
        p["anim_name"] = od.get("anim_name", od["name"])        # the model's motion is the object's (or the instance's)
        p["tri_index"] = tis               # which of the object's triangles the part has (unit anim1: shapes)
        for f in ("tris", "tri_poly", "tri_normals", "tri_uv"):
            if od[f]:
                p[f] = [od[f][i] for i in tis]
        parts.append(p)
    return parts


def _mesh_lists(me):
    """The mesh's lists for the database: vertices, polygons (vertex indices, material, smooth flag), triangulation as Blender
    does it with each triangle's corner normals and UVs, and the active UV layer.  Read in bulk (foreach_get into numpy, then
    tolist(): the same Python floats and ints as reading the items one by one, 0.15 s less on a 16,000-triangle mesh; unit speed2).
    Before Blender 4.1 (no corner_normals) the triangles' split normals are read one by one."""
    import numpy as np
    me.calc_loop_triangles()
    tris = me.loop_triangles
    nt, nl, nv, npl = len(tris), len(me.loops), len(me.vertices), len(me.polygons)

    def get(coll, attr, n, dtype, width=1):
        a = np.empty(n * width, dtype=dtype)
        if n:
            coll.foreach_get(attr, a)
        return a.reshape(-1, width) if width > 1 else a

    tl = get(tris, "loops", nt, np.int32, 3)
    try:
        cn = me.corner_normals            # Blender 4.1 and later: one normal per loop
    except AttributeError:
        cn = None
    if cn is not None:
        tri_normals = get(cn, "vector", nl, np.float32, 3)[tl].tolist() if nt else []
    else:
        tri_normals = [[list(t.split_normals[k]) for k in range(3)] for t in tris]
    uvl = me.uv_layers.active
    uv = get(uvl.data, "uv", nl, np.float32, 2) if uvl else None
    starts = get(me.polygons, "loop_start", npl, np.int32).tolist()
    totals = get(me.polygons, "loop_total", npl, np.int32).tolist()
    lv = get(me.loops, "vertex_index", nl, np.int32).tolist()
    return {
        "verts": get(me.vertices, "co", nv, np.float32, 3).tolist() if nv else [],
        "polys": [lv[a:a + n] for a, n in zip(starts, totals)],
        "poly_material": get(me.polygons, "material_index", npl, np.int32).tolist(),
        "smooth": get(me.polygons, "use_smooth", npl, bool).tolist(),
        "tris": get(tris, "vertices", nt, np.int32, 3).tolist() if nt else [],
        "tri_poly": get(tris, "polygon_index", nt, np.int32).tolist(),
        "tri_normals": tri_normals, "tri_uv": uv[tl].tolist() if (uvl and nt) else [],
        "uv": [uv.tolist() if nl else []] if uvl else []}


def _softimage_light(ob, frame):
    """The Softimage values of a light File > Import > Softimage 3D scene made, while the light is as imported
    (importer.softimage_light); blend2db then writes them instead of fitting a colour to the Blender light (units
    import2, finish1).  None for every other light, so a Blender scene's lights are fitted as before."""
    try:
        from . import importer
    except ImportError:
        return None
    return importer.softimage_light(ob, frame)


def _light_linking(ob):
    """Blender light linking as Softimage's selective light: ("lit", names) when the light's receiver collection includes
    objects (only those are lit), ("unlit", names) when it only excludes some; None when the light lights everything."""
    ll = getattr(ob, "light_linking", None)
    coll = getattr(ll, "receiver_collection", None) if ll is not None else None
    if coll is None:
        return None
    inc, exc = set(), set()
    # collection_objects / collection_children hold only the link state, in the order of objects / children
    for o, co in zip(coll.objects, getattr(coll, "collection_objects", ())):
        state = getattr(getattr(co, "light_linking", None), "link_state", "INCLUDE")
        (exc if state == "EXCLUDE" else inc).add(o.name)
    for c, ch in zip(coll.children, getattr(coll, "collection_children", ())):
        state = getattr(getattr(ch, "light_linking", None), "link_state", "INCLUDE")
        (exc if state == "EXCLUDE" else inc).update(o.name for o in c.all_objects)
    if inc:
        return "lit", sorted(inc - exc)
    if exc:
        return "unlit", sorted(exc)
    return None


def scene_to_dict(depsgraph, texdir=None):
    import bpy
    scene = depsgraph.scene
    out = {"frame": scene.frame_current, "frame_start": scene.frame_start, "frame_end": scene.frame_end, "fps": scene.render.fps,
           "resolution": [scene.render.resolution_x, scene.render.resolution_y,
                          scene.render.resolution_percentage],
           "camera": None, "lights": [], "objects": [], "materials": {}, "world": None,
           "skipped": []}
    if scene.world:
        out["world"] = _world_colour(scene.world)
    cam = scene.camera
    if cam is not None:
        c = cam.data
        out["camera"] = {"name": cam.name, "matrix_world": _matrix(cam.matrix_world), "type": c.type,
                         "lens": c.lens, "sensor": c.sensor_width, "ortho_scale": c.ortho_scale,
                         "clip": [c.clip_start, c.clip_end]}
        if c.dof.use_dof:
            out["camera"]["dof"] = {"focus": (c.dof.focus_object.matrix_world.translation - cam.matrix_world.translation).length
                                    if c.dof.focus_object else c.dof.focus_distance, "fstop": c.dof.aperture_fstop}
        elif c.get("si3d_dof") and c.get("si3d_dof_values") is not None:
            # an imported camera's own depth of field (the scene's mode and values), when Blender's is off
            out["camera"]["si_dof"] = {"mode": int(c["si3d_dof"]), "values": [float(v) for v in c["si3d_dof_values"]]}
    for inst in depsgraph.object_instances:
        ob = inst.object
        mw = _matrix(inst.matrix_world)
        if ob.type == "LIGHT":
            li = ob.data
            e = {"name": ob.name, "kind": li.type, "color": list(li.color), "energy": li.energy,
                 "matrix_world": mw, "shadow": getattr(li, "use_shadow", True)}
            if li.type == "SPOT":
                e["spot_size"], e["spot_blend"] = li.spot_size, li.spot_blend
            if li.type == "AREA":
                e["area_shape"], e["area_size"] = li.shape, [li.size, li.size_y]
            si = _softimage_light(ob, scene.frame_current)
            if si is not None:
                e["si"] = si
            sel = _light_linking(ob)
            if sel is not None:
                e[sel[0]] = sel[1]          # blend2db: a selective light (unit fidelity1)
            out["lights"].append(e)
        elif ob.type in {"MESH", "CURVE", "SURFACE", "META", "FONT", "CURVES"}:
            if not inst.is_instance and not getattr(inst, "show_self", True):
                continue                # an instancer hidden in renders (particle emitter, 'Show Instancer' off; unit anim1)
            if ob.type == "CURVES":    # hair curves: camera-facing ribbons (unit anim1)
                from . import animexport
                me = animexport.curves_lists(ob, mw, animexport.camera_info(depsgraph))
            else:
                me = ob.to_mesh()   # evaluated: modifiers applied; curves/NURBS/metaballs tessellated by Blender
            if me is None:
                out["skipped"].append(ob.name)
                continue
            mats = []
            for m in ob.material_slots:
                if m.material:
                    out["materials"].setdefault(m.material.name, _material(m.material, texdir))
                mats.append(m.material.name if m.material else None)
            od = {"name": ob.name, "type": ob.type, "matrix_world": mw, "materials": mats}
            if inst.is_instance:       # an instance moves as itself, not as the object it shows (unit anim1)
                from . import animexport
                od["anim_name"] = animexport.instance_key(inst)
            od.update(me if ob.type == "CURVES" else _mesh_lists(me))
            out["objects"].extend(_split_by_texture(od, out["materials"]))
            if not inst.is_instance:   # particle-system hair is drawn by Blender's renderers only (unit anim1)
                for ps in getattr(ob, "particle_systems", ()):
                    if ps.settings.type == "HAIR" and ps.settings.render_type == "PATH":
                        out["skipped"].append("%s hair '%s' (particle hair: convert it to a Curves object)" % (ob.name, ps.name))
            if ob.type != "CURVES":
                ob.to_mesh_clear()
        elif ob.type not in {"CAMERA", "EMPTY", "ARMATURE"}:
            out["skipped"].append("%s (%s)" % (ob.name, ob.type))
    return out


def sample_frame(depsgraph):
    """What can move in one evaluated frame, by object name: world matrices, light colour and energy, camera lens,
    material base colours.  (Mesh deformation is not sampled.)"""
    scene = depsgraph.scene
    out = {"frame": scene.frame_current, "objects": {}, "lights": {}, "camera": None, "materials": {}}
    for inst in depsgraph.object_instances:
        ob = inst.object
        if inst.is_instance:
            if ob.type in {"MESH", "CURVE", "SURFACE", "META", "FONT", "CURVES"}:
                from . import animexport        # instances under keys of their own (unit anim1)
                out["objects"][animexport.instance_key(inst)] = _matrix(inst.matrix_world)
            continue
        if ob.type == "LIGHT":
            out["lights"][ob.name] = {"matrix_world": _matrix(inst.matrix_world), "color": list(ob.data.color), "energy": ob.data.energy}
            si = _softimage_light(ob, scene.frame_current)
            if si is not None:
                out["lights"][ob.name]["si"] = si
        elif ob.type in {"MESH", "CURVE", "SURFACE", "META", "FONT", "CURVES"}:
            out["objects"][ob.name] = _matrix(inst.matrix_world)
            for slot in ob.material_slots:
                if slot.material:
                    out["materials"][slot.material.name] = _diffuse(slot.material)
    cam = scene.camera
    if cam is not None:
        out["camera"] = {"matrix_world": _matrix(cam.matrix_world), "lens": cam.data.lens}
    return out


def sample_range(scene, first=None, last=None, offset=0.0):
    """The frames first..last (default the scene's range), one `sample_frame` each, taken `offset` frames later (negative: earlier)
    than their number; the scene's current frame is put back."""
    import bpy
    cur = scene.frame_current
    frames = []
    first = scene.frame_start if first is None else first
    last = scene.frame_end if last is None else last
    try:
        for f in range(first, last + 1):
            t = f + offset
            whole = math.floor(t)
            scene.frame_set(whole, subframe=t - whole)
            frames.append(sample_frame(bpy.context.evaluated_depsgraph_get()))
    finally:
        scene.frame_set(cur)
    return frames


def freeze_camera(frames, index):
    """The program's blur walk (si_blurGeometry) moves models only, never the camera.  A camera that moves while the shutter is
    open is turned into the same picture with a still camera: every sample's camera is the sample `index`'s, and objects and
    lights carry the inverse of the camera's move (M' = C_index * C_s^-1 * M).  Returns True when something was changed."""
    from mathutils import Matrix
    cams = [f.get("camera") for f in frames]
    if not all(cams):
        return False
    base = Matrix(cams[index]["matrix_world"])
    moved = False
    for f, c in zip(frames, cams):
        cur = Matrix(c["matrix_world"])
        if max(abs(a - b) for ra, rb in zip(cur, base) for a, b in zip(ra, rb)) < 1e-6:
            continue
        moved = True
        t = base @ cur.inverted_safe()
        for group in (f["objects"], ):
            for k, m in group.items():
                group[k] = _matrix(t @ Matrix(m))
        for lg in f["lights"].values():
            lg["matrix_world"] = _matrix(t @ Matrix(lg["matrix_world"]))
        c["matrix_world"] = _matrix(base)
    return moved


def blur_shutter(scene):
    """(shutter in frames, offset in frames) of Blender's motion blur, or None when it is off.  The program exposes from the frame's
    time to time + SHUTTER_SPEED frames (si_blurGeometry); Blender's shutter position moves that window: START as is, CENTER
    half a shutter earlier, END a whole shutter earlier.  The offset is applied to the sampled animation (sample_range)."""
    r = scene.render
    if not getattr(r, "use_motion_blur", False):
        return None
    shutter = max(0.0, float(r.motion_blur_shutter))
    if shutter <= 0.0:
        return None
    pos = getattr(r, "motion_blur_position", "CENTER")
    return shutter, {"START": 0.0, "CENTER": -shutter / 2.0, "END": -shutter}.get(pos, -shutter / 2.0)


def render_options(scene, st):
    """Set-up words the program's render options take from the Blender scene: `setup` maps a word of the set-up file to its
    value (blend2db replaces the template's line).  Fields are the add-on's own setting (Blender has no interlacing)."""
    setup = {}
    if getattr(st, "fields", "OFF") != "OFF":
        setup["ON_FIELD"] = "YES"
        setup["FIRST_FIELD"] = "0" if st.fields == "FIRST0" else "1"
    blur = blur_shutter(scene)
    if blur:
        # MOTION_BLUR 1 / SHUTTER_SPEED (frames) / MIN_MOVEMENT (pixels a model must move to be blurred)
        setup["MOTION_BLUR"] = "1"
        setup["SHUTTER_SPEED"] = "%.6g" % blur[0]
        setup["MIN_MOVEMENT"] = "0.5"
    # GLOBAL_SWITCHES: only the switches turned off are written (the template's are all YES) (unit fidelity2)
    for word, prop in (("GLG_MATERIAL", "use_materials"), ("GLG_2D_TXT", "use_textures_2d"),
                       ("GLG_3D_TXT", "use_textures_3d"), ("GLG_REFLECT", "use_reflection"),
                       ("GLG_REFRACT", "use_refraction"), ("GLG_SHADOW", "use_shadows")):
        if not getattr(st, prop, True):
            setup[word] = "NO"
    aa = getattr(st, "antialias", "DEFAULT")
    if aa in ("FIXED", "ADAPTIVE"):  # ANTIALIAS mode filter level threshold; FILTER is the same filter
        setup["ANTIALIAS"] = "%s %d %d %.6g" % (aa, st.aa_filter, st.aa_level, st.aa_threshold)
        setup["FILTER"] = "%d" % st.aa_filter
    r = scene.render
    pax, pay = float(r.pixel_aspect_x), float(r.pixel_aspect_y)
    if pax != pay and pax > 0 and pay > 0:
        # pixels that are not square (an imported NTSC set-up: PIXEL_RATIO 0.9): the renderer widens the camera's
        # view by the set-up's pixel ratio, width over height of a pixel (unit fidelity2)
        setup["PIXEL_RATIO"] = "%.9g" % (pax / pay)
    out = {"setup": setup}
    depth = int(getattr(st, "ray_depth", 0) or 0)
    if depth > 0:                    # MAX_RAY_DEPTH as set; 0 leaves blend2db's own rule
        out["ray_depth"] = depth
    from . import material_nodes
    wn = material_nodes.world_node(scene.world)
    if wn is not None:               # a Softimage World: fade, fog and a set ambience (unit nodes1)
        setup.update(material_nodes.world_setup(wn))
        amb = material_nodes.world_ambience(wn)
        if amb is not None:
            out["ambience"] = amb
    return out


def dump(depsgraph, path):
    with open(path, "w") as fh:
        json.dump(scene_to_dict(depsgraph), fh)


def _find(root, pattern, prefer=""):
    import glob
    hits = sorted(glob.glob(os.path.join(root, "**", pattern), recursive=True))
    pref = [h for h in hits if prefer in h]
    return (pref or hits or [None])[0]


def region_of(scene):
    """The render region (Output > Render Region) as the original's sub-region: (fractions l, r, t, b cut off each side,
    box x0, y0, w, h in Blender pixels), or (None, None) when it is off (unit finish1)."""
    r = scene.render
    if not r.use_border:
        return None, None
    from . import session
    rw = int(r.resolution_x * r.resolution_percentage / 100)
    rh = int(r.resolution_y * r.resolution_percentage / 100)
    return session.region_fractions((r.border_min_x, r.border_max_x, r.border_min_y, r.border_max_y), rw, rh)


def _run_tail(pics, n=400):
    """': ' and the end of render-frames' log in `pics` (its reason when it stopped before rendering: the program
    file not found, another build), or ''."""
    try:
        with open(os.path.join(pics, "run.txt"), "rb") as fh:
            t = fh.read().decode("utf-8", "replace").strip()
    except OSError:
        return ""
    return (": " + t[-n:]) if t else ""


def patch_generated_setup(dbdir, z_channel=False, region=None):
    """The set-up blend2db wrote, patched as the 'Softimage scene' source patches a scene's (session.patch_sts): the
    depth file switched on (Z_CHANNEL YES, finding 0012 section 31), and the render region as the original's own
    sub-region (SUBREGION_FRAME; the renderer then writes a picture of the region's size).  Nothing is rewritten when
    neither is asked for, so the database stays byte for byte what blend2db wrote.  blend2db writes the database's
    chapters under <dbdir>/db/ (before unit finish1 the patch looked in <dbdir>/SETUP_SOFT, found nothing, and the
    Depth pass of this source stayed empty)."""
    if not z_channel and region is None:
        return
    import glob
    from . import session
    found = glob.glob(os.path.join(dbdir, "SETUP_SOFT", "*.sts")) + glob.glob(os.path.join(dbdir, "*", "SETUP_SOFT", "*.sts"))
    if not found:
        raise RuntimeError("the generated database has no render set-up (SETUP_SOFT) under " + dbdir)
    for sts in found:
        with open(sts, "r", errors="surrogateescape") as fh:
            t = fh.read()
        with open(sts, "w", errors="surrogateescape") as fh:
            fh.write(session.patch_sts(t, region=region, z_channel=z_channel))


def render_blender_scene(engine, depsgraph, st, frame):
    """Source 'This Blender scene': scene_to_dict -> JSON -> `blend2db build` -> render-frames -> the frame's .pic.

    Needs the blend2db program (setting, env SI3D_BLEND2DB), render-frames and the install dir (it holds
    Relations.rsrc and the template set-up file the render options are patched into)."""
    import subprocess
    import bpy
    from . import runner
    from . import session, install
    installed = session.install_root(st, runner.bpy_abspath)
    exe = session.blend2db_path(st, runner.bpy_abspath)
    if not exe or not os.path.isfile(exe):
        raise RuntimeError("blend2db program not set (Render > Fogle3D, or env SI3D_BLEND2DB)")
    info, bad = install.check(installed, install.BLEND2DB)
    if bad:
        raise RuntimeError("\n".join(bad))
    rsrc, tmpl = info["rsrc"], info["template_sts"]
    from . import animexport
    # an animation render of a scene with deforming objects: one run over the range (unit anim1)
    pic = animexport.range_frame(engine, depsgraph, st, frame)
    from . import mrfx      # mental ray shader stand-ins (unit sky1): the sky put behind the frame, when the scene has one
    if pic is not animexport.NOT_HERE:
        return mrfx.render_composite(depsgraph, pic, frame) or pic
    work = runner._new_dir(runner.bpy_abspath(st.keep_dir))
    os.makedirs(os.path.join(work, "textures"), exist_ok=True)
    data = scene_to_dict(depsgraph, os.path.join(work, "textures"))
    data["frame"] = frame
    mrfx.render_textures(depsgraph.scene, os.path.join(work, "textures"), frame)
    data["render"] = render_options(depsgraph.scene, st)
    sc = bpy.context.scene
    blur = blur_shutter(depsgraph.scene)
    # (a scene whose topology changes renders every frame from a database of the frame alone: no range sampled)
    ranged = st.frame_mode == "RANGE" and sc.frame_end > sc.frame_start and animexport.route(engine, depsgraph, st) != "window"
    if blur or ranged:
        # with blur the database must carry the animation to the end of the shutter: frames past the range's last (or past the
        # still) are sampled too; the program interpolates linearly between the samples
        extra = int(math.ceil(blur[0])) + 1 if blur else 0
        first, last = (sc.frame_start, sc.frame_end) if ranged else (frame, frame)
        last += extra
        try:
            frames = sample_range(sc, first, last, blur[1] if blur else 0.0)
            if blur and freeze_camera(frames[frame - first:], 0):
                if data.get("camera"):
                    data["camera"]["matrix_world"] = frames[frame - first]["camera"]["matrix_world"]
                engine.report({"INFO"}, "Fogle3D: camera motion in the shutter is baked into the objects (the blur only moves models)")
            data["anim"] = {"first": first, "last": last, "frames": frames}
            if blur:    # deforming objects: shapes over the shutter, so the program's blur blurs the deformation (unit anim1)
                for n in animexport.blur_shapes(engine, depsgraph, data, frame, extra, blur[1], work):
                    engine.report({"WARNING"}, "Fogle3D: " + n)
        except Exception as e:   # frame changes are refused in some contexts: render the still
            engine.report({"WARNING"}, "animation not sampled: %s" % e)
    jpath = os.path.join(work, "scene.json")
    with open(jpath, "w") as fh:
        # json.dumps (C encoder, one string) writes the same text as json.dump (pure-Python encoder, ~850k small writes for a
        # subdivided mesh: 0.35 s against 0.1 s; unit speed2)
        fh.write(json.dumps(data))
    dbdir = os.path.join(work, "database")
    tdb = os.path.dirname(os.path.dirname(tmpl))
    tscene = os.path.basename(tmpl)[:-len(".sts")]
    engine.update_stats("", "Fogle3D: building the database")
    p = subprocess.run([exe, "build", jpath, dbdir, tdb, tscene, os.path.dirname(rsrc)],
                       capture_output=True, text=True, timeout=600)
    with open(os.path.join(work, "blend2db.txt"), "w") as fh:
        fh.write(p.stdout + p.stderr)
    if p.returncode != 0:
        why = (p.stderr.strip() or p.stdout.strip())
        if "the scene has no mesh object to render" in why:
            # nothing the renderer draws (curves, nulls, an empty scene): the Softimage renderer gives a picture with
            # nothing on it, so does this source: an empty frame (the world colour is put under it when the film is
            # not transparent), not an error (unit fidelity1: 24 converted corpus scenes)
            import numpy as np
            r = depsgraph.scene.render
            w = int(r.resolution_x * r.resolution_percentage / 100)
            h = int(r.resolution_y * r.resolution_percentage / 100)
            engine.report({"INFO"}, "Fogle3D: the scene has no surface the renderer draws (meshes, patches, NURBS "
                                    "surfaces): an empty picture")
            return w, h, {k: np.zeros(w * h, np.uint8) for k in ("R", "G", "B", "A")}
        raise RuntimeError("blend2db: " + why[:300])
    patch_generated_setup(dbdir, depsgraph.view_layer.use_pass_z, region_of(depsgraph.scene)[0])
    for line in p.stdout.splitlines():
        if line.startswith("not carried"):
            engine.report({"WARNING"}, line)
    pics = os.path.join(work, "pictures")
    engine.update_stats("", "Fogle3D: rendering")
    r = depsgraph.scene.render
    size = (int(r.resolution_x * r.resolution_percentage / 100), int(r.resolution_y * r.resolution_percentage / 100))
    # SI_INSTALL: render-frames reads the program file from the user's install, not from the database
    runner.run_render_frames(st, dbdir, "SCENES/blend", dict(install.program_env(installed), RENDER_FRAME=str(frame)), pics,
                             size=size)
    found = runner._pictures(pics)
    if not found:
        raise RuntimeError("the renderer wrote no picture (see %s)%s" % (os.path.join(pics, "run.txt"), _run_tail(pics)))
    if len(found) == 2:
        # field rendering writes two half-height pictures per frame (NAME.<frame>.1.pic, .2.pic): weave them into one frame
        from . import sipic
        try:
            a, b = (open(f, "rb").read() for f in sorted(found))
            woven = sipic.weave(a, b, int(os.environ.get("SI3D_FIELD_TOP", "2")))
            return mrfx.render_composite(depsgraph, woven, frame) or woven
        except sipic.PicError:
            pass
    return mrfx.render_composite(depsgraph, found[0], frame) or found[0]
