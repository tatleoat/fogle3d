# SPDX-License-Identifier: GPL-3.0-or-later
"""Animation and physics for source 'This Blender scene' (unit anim1; our own code).

Blender evaluates everything that moves or deforms (armatures, shape keys, cloth, soft body, rigid body, lattices, hooks,
geometry nodes, particles, fluid caches, hair) and the Softimage renderer draws the result.  The scene's objects fall into
classes, decided by sampling their evaluated geometry (docs/design/blender-animation-and-physics.md):

- static / transform-only: the existing path (export.render_blender_scene): world matrices per frame as function curves.
- deforming, same topology every frame: Softimage shape animation.  Every sampled frame's vertex positions and corner normals
  (world space) go to a side file (format in blend2db's shapes.rs); blend2db makes one shape per frame, a type-8 curve and
  relations 200/15010.  The program blends the shapes at each frame and at each step of its own motion-blur walk, so
  deformation is blurred like motion.
- topology changes (fluid meshes, remeshers, booleans, particles born and dying): a snapshot per frame (one database each).
- instances (particles, geometry-node instances, collection instances): realised as one model each, sampled under a key of
  their own (`instance_key`).
- hair curves (Blender's Curves objects): camera-facing ribbons, one quad strip per curve (`curves_arrays`).

Routes:
- an animation render (Blender's frame loop) with frames 'Scene range' of a scene with deforming objects and no topology change
  samples the range once, builds a few databases (contiguous chunks of the range, each holding the shapes of its frames only)
  and runs the renderer over them, K processes at a time (`chunk_child`); each frame Blender asks for is served from that run.
- everything else renders a frame alone (the existing path); with motion blur, the deforming objects get the shapes of the
  shutter's frames (`blur_shapes`).
"""
import math
import os
import struct
import time

GEOM = {"MESH", "CURVE", "SURFACE", "META", "FONT", "CURVES"}
NOT_HERE = object()           # range_frame: this scene is not for the range route
SHAPE_BUDGET_MB = 700          # shapes per database the renderer has to hold (each process holds one database)
_JOB = [0]                     # render jobs started (bpy.app.handlers.render_init), so a new job never reuses an old run
_DECIDED = {}                  # (job, scene, range) -> route of this job: "range", "window" (topology changes), "plain"
LOG = []                       # (time, text): what the routes did (tests read it)


def _render_init(*_args):
    _JOB[0] += 1


_render_init.__name__ = "_si3d_anim1_render_init"


def ensure_handler():
    import bpy
    hs = bpy.app.handlers.render_init
    for h in list(hs):
        if getattr(h, "__name__", "") == _render_init.__name__ and h is not _render_init:
            hs.remove(h)              # an older copy of this module (add-on reloaded)
    if _render_init not in hs:
        hs.append(_render_init)


def note(text):
    LOG.append((time.time(), text))


def instance_key(inst):
    """A key of an instance (particle, geometry-node instance, collection instance) that stays the same from frame to frame:
    the instancer, the instanced object and the persistent id.  The object's own name would be the source object's, whose
    motion the instance does not have."""
    p = inst.parent
    return "%s>%s#%s" % (p.name if p else "", inst.object.name, ".".join(str(i) for i in inst.persistent_id))


def candidates(depsgraph):
    """Names of the (not instanced) objects whose evaluated geometry can change from frame to frame: meshes with modifiers,
    shape keys or animated mesh data; curves, surfaces, metaballs, text and hair curves."""
    out = set()
    for inst in depsgraph.object_instances:
        if inst.is_instance or not getattr(inst, "show_self", True):
            continue
        ob = inst.object
        if ob.type == "MESH":
            orig = ob.original
            me = orig.data
            sk = getattr(me, "shape_keys", None)
            if len(orig.modifiers) or (sk is not None and len(sk.key_blocks) > 1) or getattr(me, "animation_data", None):
                out.add(ob.name)
        elif ob.type in GEOM:
            out.add(ob.name)
    return out


def camera_info(depsgraph):
    """(position, forward, ortho, tan of half the horizontal field, picture width) of the scene's camera, world space."""
    import numpy as np
    scene = depsgraph.scene
    cam = scene.camera
    r = scene.render
    w = max(1, int(r.resolution_x * r.resolution_percentage / 100))
    if cam is None:
        return None
    ce = cam.evaluated_get(depsgraph)
    m = np.array([list(row) for row in ce.matrix_world], dtype=np.float64)
    c = ce.data
    if c.type == "ORTHO":
        return m[:3, 3], -m[:3, 2], c.ortho_scale, 0.0, w
    sensor = c.sensor_width if c.sensor_fit != "VERTICAL" else c.sensor_height
    return m[:3, 3], -m[:3, 2], 0.0, sensor / (2.0 * c.lens), w


# ----------------------------------------------------------------------------- geometry of one object
def curves_arrays(ob, mw, cam):
    """Hair curves (a Curves object, evaluated) as camera-facing ribbons, object space: dict of numpy arrays verts (N,3),
    quads (Q,4), tris (T,3), tri_poly (T,), corner normals (T,3,3), tri_uv (T,3,2), uv per loop (Q*4,2).  Width: twice the
    point's radius (attribute 'radius', else 0.005 m), at least a pixel at the point's distance.  None if it has no curve."""
    import numpy as np
    cv = ob.data
    npt = len(cv.points)
    nc = len(cv.curves)
    if npt < 2 or nc < 1:
        return None
    pos = np.empty(npt * 3, np.float32)
    cv.attributes["position"].data.foreach_get("vector", pos)
    pos = pos.reshape(-1, 3).astype(np.float64)
    rad = np.full(npt, 0.005)
    ra = cv.attributes.get("radius")
    if ra is not None and ra.domain == "POINT" and len(ra.data) == npt:
        tmp = np.empty(npt, np.float32)
        ra.data.foreach_get("value", tmp)
        rad = tmp.astype(np.float64)
    off = np.empty(nc + 1, np.int32)
    cv.curve_offset_data.foreach_get("value", off)
    M = np.array(mw, dtype=np.float64)
    M3 = M[:3, :3]
    inv = np.linalg.inv(M3) if abs(np.linalg.det(M3)) > 1e-20 else np.eye(3)
    scale = abs(np.linalg.det(M3)) ** (1.0 / 3.0) or 1.0
    curve_of = np.repeat(np.arange(nc), np.diff(off))
    first, last = off[:-1][curve_of], off[1:][curve_of] - 1
    idx = np.arange(npt)
    nxt, prv = np.minimum(idx + 1, last), np.maximum(idx - 1, first)
    tan = pos[nxt] - pos[prv]
    tan /= np.maximum(np.linalg.norm(tan, axis=1, keepdims=True), 1e-12)
    if cam is not None:
        cpos, cfwd, ortho, tanf, width = cam
        if ortho:
            view = np.broadcast_to(inv @ cfwd, pos.shape)
            pix = ortho / width / scale
            dist = np.ones(npt)
        else:
            view = pos - inv @ (cpos - M[:3, 3])
            dist = np.linalg.norm(view, axis=1) * scale
            pix = 2.0 * tanf / width / scale
    else:
        view = np.broadcast_to(np.array([0.0, 1.0, 0.0]), pos.shape)
        dist, pix = np.ones(npt), 0.0
    side = np.cross(tan, view)
    side /= np.maximum(np.linalg.norm(side, axis=1, keepdims=True), 1e-12)
    half = np.maximum(rad, 0.5 * pix * dist)[:, None]
    verts = np.concatenate([pos - side * half, pos + side * half]).astype(np.float32)
    nrm = np.cross(side, tan)
    flip = (nrm * view).sum(axis=1) > 0           # towards the camera
    nrm[flip] *= -1.0
    nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-12)
    seg = idx[idx < last]                          # point i to i+1 of the same curve
    a0, a1, b0, b1 = seg, seg + 1, seg + npt, seg + 1 + npt
    quads = np.stack([a0, a1, b1, b0], axis=1).astype(np.int32)
    tris = np.concatenate([np.stack([a0, a1, b1], 1), np.stack([a0, b1, b0], 1)]).astype(np.int32)
    q = len(seg)
    tri_poly = np.concatenate([np.arange(q), np.arange(q)]).astype(np.int32)
    vn = np.concatenate([nrm, nrm])
    corner_n = vn[tris].astype(np.float32)
    t_along = (idx - first) / np.maximum(last - first, 1)
    vuv = np.concatenate([np.stack([np.zeros(npt), t_along], 1), np.stack([np.ones(npt), t_along], 1)]).astype(np.float32)
    return {"verts": verts, "quads": quads, "tris": tris, "tri_poly": tri_poly, "tri_normals": corner_n,
            "tri_uv": vuv[tris], "uv": vuv[quads.reshape(-1)], "vnormals": vn}


def curves_lists(ob, mw, cam):
    """curves_arrays as the lists export._mesh_lists gives a mesh."""
    a = curves_arrays(ob, mw, cam)
    if a is None:
        return None
    q = len(a["quads"])
    return {"verts": a["verts"].tolist(), "polys": a["quads"].tolist(), "poly_material": [0] * q, "smooth": [True] * q,
            "tris": a["tris"].tolist(), "tri_poly": a["tri_poly"].tolist(), "tri_normals": a["tri_normals"].tolist(),
            "tri_uv": a["tri_uv"].tolist(), "uv": [a["uv"].tolist()]}


def grab(ob, mw, cam):
    """One sample of an evaluated object, or None: (signature, local positions, world positions, world corner normals per
    loop, loop vertices, polygon loop starts).  The signature is the polygon topology (vertex and loop counts, a hash of the
    loops' vertices and the polygons' starts): Blender re-triangulates a deformed quad along its shorter diagonal, so the
    triangles are not part of it; the side file's triangles are the still's, their corners looked up by (polygon, vertex).
    Hair (Curves) is curves_arrays' quads."""
    import hashlib
    import numpy as np
    if ob.type == "CURVES":
        a = curves_arrays(ob, mw, cam)
        if a is None:
            return None
        co = a["verts"].astype(np.float64)
        lv = a["quads"].reshape(-1).astype(np.int32)
        lstart = (np.arange(len(a["quads"])) * 4).astype(np.int32)
        cn = a["vnormals"][lv].astype(np.float64)
    else:
        me = ob.to_mesh()
        if me is None:
            return None
        try:
            nv, nl, npl = len(me.vertices), len(me.loops), len(me.polygons)
            co = np.empty(nv * 3, np.float32)
            lv = np.empty(nl, np.int32)
            lstart = np.empty(npl, np.int32)
            lnorm = np.empty(nl * 3, np.float32)
            if nv:
                me.vertices.foreach_get("co", co)
            if nl:
                me.loops.foreach_get("vertex_index", lv)
                me.polygons.foreach_get("loop_start", lstart)
            try:
                if nl:
                    me.corner_normals.foreach_get("vector", lnorm)
            except AttributeError:          # before Blender 4.1
                me.calc_normals_split()
                if nl:
                    me.loops.foreach_get("normal", lnorm)
            co = co.reshape(-1, 3).astype(np.float64)
            cn = lnorm.reshape(-1, 3).astype(np.float64)
        finally:
            ob.to_mesh_clear()
    M = np.array(mw, dtype=np.float64)
    M3 = M[:3, :3]
    world = co @ M3.T + M[:3, 3]
    inv = np.linalg.inv(M3) if abs(np.linalg.det(M3)) > 1e-20 else np.eye(3)
    wn = cn @ inv                                 # (inverse transpose) applied to row vectors
    h = hashlib.blake2b(lv.tobytes() + b"|" + lstart.tobytes(), digest_size=16).hexdigest()
    return (len(co), len(lv), h), co.astype(np.float32), world.astype(np.float32), wn.astype(np.float32), lv, lstart


def still_loops(parts, s):
    """The loops of the still's triangle corners (triangles x 3, the object's triangle order), looked up in a sample by
    (polygon, vertex); None when the still and the sample differ in topology."""
    import numpy as np
    sig, lv, lstart = s[0], s[4], s[5]
    if not parts or len(parts[0]["verts"]) != sig[0] or len(parts[0].get("polys", ())) != len(lstart):
        return None
    nv, nl = sig[0], sig[1]
    totals = np.diff(np.append(lstart, nl))
    if (totals <= 0).any():
        return None
    keys = np.repeat(np.arange(len(lstart), dtype=np.int64), totals) * nv + lv
    order = np.argsort(keys, kind="stable")
    sk = keys[order]
    nt = sum(len(p["tris"]) for p in parts)
    tris = np.full((nt, 3), -1, np.int64)
    poly = np.full(nt, -1, np.int64)
    for p in parts:
        idx = np.asarray(p.get("tri_index") or range(len(p["tris"])), np.int64)
        if len(idx) != len(p["tris"]) or (len(idx) and (idx.max() >= nt or idx.min() < 0)):
            return None
        if len(idx):
            tris[idx] = np.asarray(p["tris"], np.int64).reshape(-1, 3)
            poly[idx] = np.asarray(p["tri_poly"], np.int64)
    if nt == 0 or (tris < 0).any() or (poly < 0).any() or (poly >= len(lstart)).any() or (tris >= nv).any():
        return None
    q = poly[:, None] * nv + tris
    at = np.searchsorted(sk, q)
    at = np.minimum(at, len(sk) - 1)
    if not (sk[at] == q).all():
        return None
    return order[at]


def write_side(path, frames, samples, loops):
    """The side file blend2db's shapes.rs reads: header, frame numbers, then per frame positions and the corner normals of
    the still's triangles (`loops`, from still_loops)."""
    import numpy as np
    sig = samples[0][0]
    with open(path, "wb") as fh:
        fh.write(b"SISHP1" + struct.pack("<3I", len(frames), sig[0], len(loops)))
        fh.write(np.asarray(frames, "<i4").tobytes())
        for s in samples:
            fh.write(np.ascontiguousarray(s[2], "<f4").tobytes())
            fh.write(np.ascontiguousarray(s[3][loops], "<f4").tobytes())


def _classify(ss):
    """'topology', 'deform' or 'rigid' for one object's samples."""
    import numpy as np
    if len({s[0] for s in ss}) > 1:
        return "topology"
    if any(not np.array_equal(ss[0][1], s[1]) for s in ss[1:]):
        return "deform"
    return "rigid"


def _parts(data, name):
    return [o for o in data["objects"] if o.get("anim_name", o["name"]) == name]


def _safe(name):
    return "".join(c if c.isalnum() else "_" for c in name)[:60]


# ----------------------------------------------------------------------------- per-frame databases with blur
def blur_shapes(engine, depsgraph, data, frame, extra, offset, work):
    """Motion blur of a frame rendered alone: the deforming objects' shapes over the shutter (frames frame..frame+extra,
    each sampled `offset` frames off, as sample_range samples the matrices) go to data["shapes"].  Objects whose geometry
    does not change are left alone (their database is the one the existing path builds).  Returns notes."""
    cands = candidates(depsgraph)
    if not cands:
        return []
    frames = list(range(frame, frame + extra + 1))
    samples = {}
    try:
        for f in frames:
            t = f + offset
            whole = math.floor(t)
            engine.frame_set(whole, t - whole)
            cam = camera_info(depsgraph)
            for inst in depsgraph.object_instances:
                if inst.is_instance or inst.object.name not in cands:
                    continue
                g = grab(inst.object, [list(r) for r in inst.matrix_world], cam)
                if g is not None:
                    samples.setdefault(inst.object.name, []).append(g)
    finally:
        engine.frame_set(frame, 0.0)
    return _attach(data, samples, frames, os.path.join(work, "shapes"))


def _attach(data, samples, frames, sdir):
    notes = []
    for name, ss in sorted(samples.items()):
        if len(ss) != len(frames):
            continue
        kind = _classify(ss)
        if kind == "rigid":
            continue
        if kind == "topology":
            notes.append("%s changes topology over the shutter: its deformation is not blurred" % name)
            continue
        loops = still_loops(_parts(data, name), ss[0])
        if loops is None:
            notes.append("%s: the frame's mesh and the shutter's samples differ in topology: not blurred" % name)
            continue
        os.makedirs(sdir, exist_ok=True)
        path = os.path.join(sdir, "%s.sishp" % _safe(name))
        write_side(path, frames, ss, loops)
        data.setdefault("shapes", {})[name] = {"path": path}
    if data.get("shapes"):
        note("blur shapes: %s" % ", ".join(sorted(data["shapes"])))
    return notes


# ----------------------------------------------------------------------------- the range route
_CHUNKS = []


def chunk_child(exe, roots, chunks, only, out_dir, k, threads, mem_kb, extra_env=None):
    """K render-frames processes over chunks of the range, each chunk a database of its own: parallel.ParChild with one root
    per chunk (the interface of session.Child)."""
    from . import parallel
    if not _CHUNKS:
        class Chunks(parallel.ParChild):
            def pump(self):
                if getattr(self, "_go", False):
                    parallel.ParChild.pump(self)

            def _launch(self, i):
                self.root = self.roots[i]
                parallel.ParChild._launch(self, i)
        _CHUNKS.append(Chunks)
    frames = [f for a, b in chunks for f in range(a, b + 1)]
    c = _CHUNKS[0](exe, roots[0], only, dict(extra_env or {}), out_dir, frames, k, threads, mem_kb)
    c.roots, c.chunks, c.todo, c._go = list(roots), list(chunks), list(range(len(chunks))), True
    c.pump()
    return c


def _camera_moves(anim):
    cams = [a.get("camera") for a in anim]
    if not all(cams):
        return False
    m0 = cams[0]["matrix_world"]
    return any(max(abs(x - y) for ra, rb in zip(c["matrix_world"], m0) for x, y in zip(ra, rb)) > 1e-6 for c in cams[1:])


def sample(depsgraph):
    """export.sample_frame of the render depsgraph (moved with engine.frame_set): the camera read from its evaluated copy,
    since the original object's matrix follows the viewport only."""
    from . import export
    a = export.sample_frame(depsgraph)
    cam = depsgraph.scene.camera
    if cam is not None:
        ce = cam.evaluated_get(depsgraph)
        a["camera"] = {"matrix_world": export._matrix(ce.matrix_world), "lens": ce.data.lens}
    return a


def _route_key(scene, st, depsgraph):
    r = scene.render
    return (_JOB[0], scene.name, scene.frame_start, scene.frame_end, scene.frame_step, r.resolution_x, r.resolution_y,
            r.resolution_percentage, bool(depsgraph.view_layer.use_pass_z))


def route(engine, depsgraph, st):
    """The route already decided for this render job ("range", "window", "plain"), or None."""
    return _DECIDED.get(_route_key(depsgraph.scene, st, depsgraph))


def range_frame(engine, depsgraph, st, frame):
    """A frame of an animation render with frames 'Scene range': the picture's path from the range run, None when the user
    cancelled, or NOT_HERE when the scene is not for this route (the caller renders the frame alone; `route` says why)."""
    import bpy
    from . import session, export
    ensure_handler()
    scene = depsgraph.scene
    if not getattr(engine, "is_animation", False) or st.frame_mode != "RANGE" or scene.frame_end <= scene.frame_start:
        return NOT_HERE
    if os.environ.get("SI3D_ANIM_RANGE", "1") == "0" or getattr(st, "fields", "OFF") != "OFF":
        return NOT_HERE
    key = _route_key(scene, st, depsgraph)
    if _DECIDED.get(key) not in (None, "range"):
        return NOT_HERE
    if key not in _DECIDED and not candidates(depsgraph):
        _DECIDED[key] = "plain"
        return NOT_HERE
    rkey = ("anim1-range",) + key

    def make():
        run = _make_range_run(engine, depsgraph, st, scene, frame)
        if run is None:
            raise _NotHere()
        return run

    try:
        run = session.get_run(engine, rkey, make)
    except _NotHere:
        return NOT_HERE
    _DECIDED[key] = "range"
    if frame not in run.frames:
        return NOT_HERE
    return session.wait_for(engine, run.child, frame, run.frames)


class _NotHere(Exception):
    pass


def _make_range_run(engine, depsgraph, st, scene, frame):
    """Samples the range once, decides the route (stored in _DECIDED) and, for 'range', builds the chunk databases and starts
    the renderer over them: a session.RangeRun, else None."""
    import json
    import subprocess
    import bpy
    import numpy as np
    from . import session, export, runner, install
    key = _route_key(scene, st, depsgraph)
    t0 = time.time()
    first, last, step = scene.frame_start, scene.frame_end, scene.frame_step
    want = list(range(first, last + 1, step))
    blur = export.blur_shutter(scene)
    extra = int(math.ceil(blur[0])) + 1 if blur else 0
    offset = blur[1] if blur else 0.0
    cands = candidates(depsgraph)
    frames = list(range(first, last + extra + 1))
    anim, samples, inst_sets = [], {}, set()
    engine.update_stats("Fogle3D", "sampling the animation")
    try:
        for f in frames:
            if session.cancel_requested(engine):
                raise RuntimeError("cancelled")
            t = f + offset
            whole = math.floor(t)
            engine.frame_set(whole, t - whole)
            anim.append(sample(depsgraph))
            cam = camera_info(depsgraph)
            keys = []
            for inst in depsgraph.object_instances:
                if inst.is_instance:
                    if inst.object.type in GEOM:
                        keys.append(instance_key(inst))
                    continue
                if inst.object.name in cands:
                    g = grab(inst.object, [list(r) for r in inst.matrix_world], cam)
                    if g is not None:
                        samples.setdefault(inst.object.name, []).append(g)
            inst_sets.add(tuple(sorted(keys)))
        engine.frame_set(first, 0.0)
        kinds = {n: _classify(ss) for n, ss in samples.items() if len(ss) == len(frames)}
        lost = [n for n, ss in samples.items() if len(ss) != len(frames)]
        why = None
        if any(k == "topology" for k in kinds.values()) or lost:
            why = "topology changes (%s)" % ", ".join(sorted([n for n, k in kinds.items() if k == "topology"] + lost))
        elif len(inst_sets) > 1:
            why = "the instances change from frame to frame (particles born or dying)"
        elif not any(k == "deform" for k in kinds.values()):
            _DECIDED[key] = "plain"
            note("range route: no deforming object, existing path")
            return None
        elif blur and _camera_moves(anim):
            why = "the camera moves and motion blur is on (the blur moves models only: frames rendered alone)"
        if why:
            _DECIDED[key] = "window"
            note("range route: per-frame databases, " + why)
            engine.report({"INFO"}, "Fogle3D: %s: every frame is a database of its own" % why)
            return None
        # the still: the first frame, evaluated by the render depsgraph
        work = session.new_dir(bpy.path.abspath(st.keep_dir))
        os.makedirs(os.path.join(work, "textures"), exist_ok=True)
        still = export.scene_to_dict(depsgraph, os.path.join(work, "textures"))
    finally:
        engine.frame_set(frame, 0.0)
    still["frame"] = first
    if still.get("camera") and anim[0].get("camera"):
        still["camera"]["matrix_world"] = anim[0]["camera"]["matrix_world"]
    still["render"] = export.render_options(scene, st)
    deform = sorted(n for n, k in kinds.items() if k == "deform")
    loops = {}
    for n in deform:
        loops[n] = still_loops(_parts(still, n), samples[n][0])
        if loops[n] is None:
            _DECIDED[key] = "window"
            note("range route: %s differs from its samples in topology, per-frame databases" % n)
            return None
    # chunks: about 3 per process, fewer frames when a chunk's shapes would pass the budget
    k, thr = session.counts(len(want), True)
    per_frame = sum(int(samples[n][0][0][0]) * 0x14 + len(loops[n]) * (0x84 + 0x3c) for n in deform)
    r = scene.render
    mem_kb = session.memory_kb(st, (int(r.resolution_x * r.resolution_percentage / 100),
                                    int(r.resolution_y * r.resolution_percentage / 100)), thr)
    k = session.fit_processes(k, mem_kb)
    budget = min(SHAPE_BUDGET_MB << 20, mem_kb * 1024 // 2)
    max_len = max(1, budget // max(1, per_frame) - extra)
    from . import parallel
    n_chunks = max(min(len(want), 3 * max(1, k)), -(-len(want) // max_len))
    if step != 1:
        n_chunks = len(want)          # one chunk per wanted frame
    chunks = parallel.make_chunks(want, n_chunks, 1) if step == 1 else [(f, f) for f in want]
    installed = session.install_root(st, runner.bpy_abspath)
    exe = session.blend2db_path(st, runner.bpy_abspath)
    info, bad = install.check(installed, install.BLEND2DB)
    if bad:
        raise RuntimeError("\n".join(bad))
    rsrc, tmpl = info["rsrc"], info["template_sts"]
    tdb = os.path.dirname(os.path.dirname(tmpl))
    tscene = os.path.basename(tmpl)[:-len(".sts")]
    head = json.dumps(still)
    assert head.endswith("}")
    roots, procs = [], []
    engine.update_stats("Fogle3D", "building %d databases" % len(chunks))
    for ci, (a, b) in enumerate(chunks):
        cdir = os.path.join(work, "chunk%03d" % ci)
        os.makedirs(cdir, exist_ok=True)
        lo, hi = a - first, b + extra - first
        if hi == lo:          # a database's animation needs two frames (blend2db takes one entry as a still)
            hi, lo = (hi + 1, lo) if hi + 1 < len(frames) else (hi, lo - 1)
        cframes = frames[lo:hi + 1]
        shapes = {}
        for n in deform:
            p = os.path.join(cdir, "%s.sishp" % _safe(n))
            write_side(p, cframes, samples[n][lo:hi + 1], loops[n])
            shapes[n] = {"path": p}
        jpath = os.path.join(cdir, "scene.json")
        with open(jpath, "w") as fh:
            fh.write(head[:-1] + ', "anim": ' + json.dumps({"first": cframes[0], "last": cframes[-1], "frames": anim[lo:hi + 1]})
                     + ', "shapes": ' + json.dumps(shapes) + "}")
        dbdir = os.path.join(cdir, "database")
        roots.append(dbdir)
        procs.append((subprocess.Popen([exe, "build", jpath, dbdir, tdb, tscene, os.path.dirname(rsrc)],
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT), cdir))
        while len([p for p, _ in procs if p.poll() is None]) >= max(1, k):
            time.sleep(0.01)
    for p, cdir in procs:
        out, _ = p.communicate(timeout=600)
        with open(os.path.join(cdir, "blend2db.txt"), "wb") as fh:
            fh.write(out)
        if p.returncode != 0:
            raise RuntimeError("blend2db: " + out.decode("utf-8", "replace").strip()[-300:])
    region = export.region_of(scene)[0]       # the render region as the original's sub-region (unit finish1)
    for dbdir in roots:
        export.patch_generated_setup(dbdir, depsgraph.view_layer.use_pass_z, region)
    t1 = time.time()
    rexe = session.renderer_path(st, bpy.path.abspath)
    child = chunk_child(rexe, roots, chunks, "SCENES/blend", os.path.join(work, "pictures"), k, thr, mem_kb,
                        install.program_env(installed))
    child.size = (int(r.resolution_x * r.resolution_percentage / 100), int(r.resolution_y * r.resolution_percentage / 100))
    note("range route: %d frames sampled, %d deforming (%s), %d databases in %.2f s, %d processes x %d threads" % (
        len(frames), len(deform), ", ".join(deform), len(chunks), t1 - t0, k, thr))
    engine.report({"INFO"}, "Fogle3D: %d deforming objects as shape animation, %d databases, one run" % (len(deform), len(chunks)))
    return session.RangeRun(child, want, work)
