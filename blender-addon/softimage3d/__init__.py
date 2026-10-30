# SPDX-License-Identifier: GPL-3.0-or-later
"""Fogle3D, a render engine for Blender (our own code).

The open Blender scene is exported to a database (export.py, then the `blend2db` tool), the port's `render-frames`
program (re/tools/si-render/pictures, finding 0012 section 23) renders it, and the .pic files it writes are handed back
through Blender's render result.  A Softimage 3D scene (.dsc) is worked with through File > Import > Softimage 3D scene
only (importer.py); the engine itself renders nothing but the Blender scene.  For tools only (fidelity sweeps, tests)
session.set_tool_scene / env SI3D_TOOL_SCENE render an installed .dsc directly; no UI reaches that.
"""
bl_info = {
    "name": "Fogle3D",
    "author": "fogle3d",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "location": "Render Properties > Render Engine",
    "description": "Render Blender scenes with Fogle3D, an unofficial recompilation. Not affiliated with or endorsed by the makers of Softimage 3D; needs your own licensed copy of Softimage 3D 4.0 and ships none of its assets or code",
    "category": "Render",
}

import os
import bpy
from bpy.props import BoolProperty, EnumProperty, FloatProperty, IntProperty, PointerProperty, StringProperty

ENGINE_ID = "FOGLE3D"          # render engine id; files made before the rename hold LEGACY_ENGINE_ID (see _fix_legacy_engine)
LEGACY_ENGINE_ID = "SOFTIMAGE3D"
DEPTH_BACKGROUND = 1e10        # Blender's Z pass value where nothing was hit (Cycles)
ZPIC_NOTHING = -99999.0        # the renderer's depth file holds -100000 where nothing was hit


class SI3DPreferences(bpy.types.AddonPreferences):
    bl_idname = __package__

    renderer: StringProperty(name="render-frames", subtype="FILE_PATH", default="",
                             description="The renderer program (built with the dist/LTO profile, see the README). "
                                         "Used when the scene's own 'render-frames' setting is empty; env SI3D_RENDER_FRAMES otherwise")
    use_hw_x87: BoolProperty(name="Hardware x87 build", default=False,
                             description="Use this separately built renderer instead (the default build already uses the processor's "
                                         "x87 unit on x86-64; SI_X87=soft forces the software model)")
    renderer_hw: StringProperty(name="render-frames (hardware x87)", subtype="FILE_PATH", default="",
                                description="A second renderer build (e.g. SI_X87=soft)")
    blend2db: StringProperty(name="blend2db", subtype="FILE_PATH", default="",
                             description="The blend2db program (env SI3D_BLEND2DB)")
    import_dump: StringProperty(name="si-import-dump", subtype="FILE_PATH", default="",
                                description="The si-import-dump program (env SI3D_IMPORT_DUMP), for File > Import > "
                                            "Softimage 3D scene (.dsc)")
    install_path: StringProperty(name="Softimage 3D 4.0 install path", subtype="DIR_PATH", default="",
                                 description="The directory your own copy of Softimage|3D 4.0 is installed to (the one holding "
                                             "Creative-Rsrc). Required: the add-on ships none of Softimage's files and reads "
                                             "the resource files and the sample databases from here. Env SI_INSTALL overrides it")
    processes: IntProperty(name="Processes (animation)", default=0, min=0, max=64,
                           description="Renderer processes for an animation render of a scene that carries no state from frame "
                                       "to frame (others run in one process). 0 = all cores but one, at most 16")
    band_threads: IntProperty(name="Band threads", default=0, min=0, max=64,
                              description="Threads inside one frame (RENDER_THREADS; same pictures as one thread). "
                                          "0 = all cores but one, at most 16, for a still image; 1 for animations")
    memory_mb: IntProperty(name="Memory per process (MB)", default=0, min=0, max=256000,
                           description="Address-space cap of each renderer process (ulimit -v; env SI3D_MEM_MB). "
                                       "0 = automatic: 2000 MB plus about 600 MB per million pixels of the picture and 128 MB "
                                       "per band thread (a 4K still gets about 9-12 GB), at most three quarters of the machine's "
                                       "memory. Render Properties > Fogle3D > Memory overrides it for one scene")

    def draw(self, context):
        col = self.layout.column()
        col.label(text="Fogle3D is unofficial: not affiliated with or endorsed by the makers of Softimage 3D.")
        col.label(text="It needs your own licensed copy of Softimage 3D 4.0 and ships none of its assets or code.")
        col.separator()
        col.prop(self, "install_path")
        from . import install
        _, bad = install.check(install.resolve_root("", self.install_path, bpy.path.abspath), install.RENDER)
        for line in bad:
            col.label(text=line, icon="ERROR")
        col.separator()
        col.prop(self, "renderer")
        col.prop(self, "use_hw_x87")
        sub = col.column()
        sub.active = self.use_hw_x87
        sub.prop(self, "renderer_hw")
        col.prop(self, "blend2db")
        col.prop(self, "import_dump")
        col.separator()
        col.prop(self, "processes")
        col.prop(self, "band_threads")
        col.prop(self, "memory_mb")


def _env(name, default=""):
    return os.environ.get(name, default)


class SI3DSettings(bpy.types.PropertyGroup):
    renderer: StringProperty(name="render-frames", subtype="FILE_PATH", default=_env("SI3D_RENDER_FRAMES"),
                             description="The render-frames program (env SI3D_RENDER_FRAMES); empty = the add-on preference")
    blend2db: StringProperty(name="blend2db", subtype="FILE_PATH", default=_env("SI3D_BLEND2DB"),
                             description="The blend2db program (env SI3D_BLEND2DB); empty = the add-on preference")
    installed: StringProperty(name="Install dir", subtype="DIR_PATH", default=_env("SI3D_INSTALLED"),
                              description="Override of the add-on preference \"Softimage 3D 4.0 install path\" for this scene (default: env SI3D_INSTALLED); empty = env SI_INSTALL, then the preference")
    frame_mode: EnumProperty(
        name="Frames", default="RANGE",
        items=[("RANGE", "Scene range", "Render the scene's whole range in one run, as soft -R -s first last does, "
                                       "and serve the frames from it"),
               ("ALONE", "One frame per run", "Render the current frame alone, as soft -R -s n n 1 does")])
    colour: EnumProperty(
        name="Values", default="SRGB",
        items=[("SRGB", "sRGB bytes", "Picture bytes are display values: converted to linear for Blender"),
               ("RAW", "Raw bytes/255", "Hand the bytes over as byte/255 (use the Raw view transform)")])
    alpha_pass: BoolProperty(name="Alpha pass", default=False,
                             description="Offer the picture's alpha plane as a separate 'Alpha' pass")
    fields: EnumProperty(
        name="Fields", default="OFF",
        items=[("OFF", "Off", "Frames, not fields"),
               ("FIRST0", "Field 0 first", "Interlaced field rendering (the program's ON_FIELD YES, FIRST_FIELD 0); Blender has no fields"),
               ("FIRST1", "Field 1 first", "Interlaced field rendering (ON_FIELD YES, FIRST_FIELD 1)")])
    keep_dir: StringProperty(name="Keep pictures in", subtype="DIR_PATH", default="",
                             description="Leave the run's files here (default: a temporary directory)")
    memory_mb: IntProperty(name="Memory (MB)", default=0, min=0, max=256000,
                           description="Address-space cap of each renderer process for this scene (ulimit -v). "
                                       "0 = the add-on preference, else automatic from the picture's size "
                                       "(2000 MB plus about 600 MB per million pixels)")
    # the program's set-up words MAX_RAY_DEPTH and GLOBAL_SWITCHES (an imported scene gets its own; unit fidelity2)
    ray_depth: IntProperty(name="Ray depth", default=0, min=0, max=32,
                           description="The program's MAX_RAY_DEPTH: 1 = no reflected or refracted rays; "
                                       "0 = automatic (4 with a transparent material, else 2)")
    antialias: EnumProperty(
        name="Anti-aliasing", default="DEFAULT",
        items=[("DEFAULT", "Adaptive 2 2", "The program's ANTIALIAS ADAPTIVE, filter 2, level 2, threshold 0.05"),
               ("FIXED", "Fixed", "ANTIALIAS FIXED with the filter, level and threshold below (filter 1, level 1: "
                                  "one sample a pixel)"),
               ("ADAPTIVE", "Adaptive", "ANTIALIAS ADAPTIVE with the filter, level and threshold below")])
    aa_filter: IntProperty(name="Filter", default=2, min=0, max=16, description="ANTIALIAS's filter (also FILTER)")
    aa_level: IntProperty(name="Level", default=2, min=0, max=16, description="ANTIALIAS's level")
    aa_threshold: FloatProperty(name="Threshold", default=0.05, min=0.0, max=1.0, description="ANTIALIAS's threshold")
    use_materials: BoolProperty(name="Materials", default=True, description="The program's GLG_MATERIAL switch")
    use_textures_2d: BoolProperty(name="2D textures", default=True, description="The program's GLG_2D_TXT switch")
    use_textures_3d: BoolProperty(name="3D textures", default=True, description="The program's GLG_3D_TXT switch")
    use_reflection: BoolProperty(name="Reflection", default=True, description="The program's GLG_REFLECT switch")
    use_refraction: BoolProperty(name="Refraction", default=True, description="The program's GLG_REFRACT switch")
    use_shadows: BoolProperty(name="Shadows", default=True, description="The program's GLG_SHADOW switch")


def _srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


_LUT = None


def _lut():
    global _LUT
    if _LUT is None:
        _LUT = [_srgb_to_linear(i / 255.0) for i in range(256)]
    return _LUT


def _byte_values(colour):
    """What each picture byte 0..255 becomes, as float64: the sRGB-to-linear table (colour SRGB) or byte/255."""
    import numpy as np
    if colour == "SRGB":
        return np.array(_lut(), dtype=np.float64)
    return np.arange(256, dtype=np.float64) / 255.0


def _plane(p):
    import numpy as np
    return p if isinstance(p, np.ndarray) else np.frombuffer(p, np.uint8)


def _crop_to(w, h, planes, box, r, depth=None):
    """The region's box (x0, y0 from the bottom, bw, bh) cut out of a picture of the whole frame, when the renderer wrote
    one (the sub-region was not applied): (bw, bh, planes, depth).  A picture of another size is left for _deliver to
    scale (unit finish1)."""
    import numpy as np
    rw = int(r.resolution_x * r.resolution_percentage / 100)
    rh = int(r.resolution_y * r.resolution_percentage / 100)
    if (w, h) != (rw, rh):
        return w, h, planes, depth
    x0, y0, bw, bh = box
    top = h - (y0 + bh)                 # file row 0 is the top row; Blender's y0 counts from the bottom

    def cut(a, dtype):
        a = a if isinstance(a, np.ndarray) else np.frombuffer(a, dtype)
        return np.ascontiguousarray(a.reshape(h, w)[top:top + bh, x0:x0 + bw]).ravel()
    planes = {k: cut(v, np.uint8) for k, v in planes.items()}
    return bw, bh, planes, (cut(depth, np.float32) if depth is not None else None)


_DELIVERED = []     # with env SI3D_KEEP_DELIVERY: what the engine handed to Blender (tests)


class SI3DRenderEngine(bpy.types.RenderEngine):
    bl_idname = ENGINE_ID
    bl_label = "Fogle3D"
    bl_use_preview = False
    bl_use_postprocess = True
    bl_use_shading_nodes_custom = False   # the shader editor stays open: Softimage nodes live there (unit nodes1)

    def update_render_passes(self, scene=None, renderlayer=None):
        # Combined (RGBA) is always there.  Depth: the renderer's depth file (Z_CHANNEL YES, <name>.<frame>.Zpic,
        # finding 0012 section 31) is read back as Blender's Z pass when the view layer asks for it.
        if scene is not None and scene.si3d.alpha_pass:
            self.register_pass(scene, renderlayer, "Alpha", 1, "A", "VALUE")
        if renderlayer is not None and renderlayer.use_pass_z:
            self.register_pass(scene, renderlayer, "Depth", 1, "Z", "VALUE")

    def render(self, depsgraph):
        from . import runner, sipic, session
        scene = depsgraph.scene
        st = scene.si3d
        frame = scene.frame_current
        if session.cancel_requested(self):       # cancelled already (Esc in an animation): no new run for the next frames
            session.cancel_runs()                # and the range run stops (the cancel can come between two frames)
            self._cancelled(scene, None)
            return
        bad = session.problems(st, bpy.path.abspath)
        if bad:
            self.error_set("Fogle3D: " + bad[0])
            self.report({"ERROR"}, "Fogle3D: " + " ".join(bad))
            return
        region = None
        try:
            tool = session.tool_scene(st)
            if tool is None:
                from . import export
                saved = runner.run_render_frames
                runner.run_render_frames = session.supervised_run_render_frames(self)
                # the render region: the generated set-up gets the original's sub-region, as the tools' direct scene
                # render does (export.patch_generated_setup), and Blender takes a picture of the box's size (unit finish1)
                region = export.region_of(scene)[1]
                # a depth file of a sub-region keeps the frame's line width (sipic.decode_zpic): (full width, first column)
                self._zgeom = (int(scene.render.resolution_x * scene.render.resolution_percentage / 100), region[0]) if region else (None, 0)
                try:
                    pic = export.render_blender_scene(self, depsgraph, st, frame)
                finally:
                    runner.run_render_frames = saved
            else:
                pic, region = self._render_scene(scene, st, tool, frame, depsgraph.view_layer.use_pass_z)
            if pic is None:                       # cancelled by the user
                self._cancelled(scene, region)
                return
            if isinstance(pic, tuple):                 # already decoded (interlaced fields woven into one frame)
                w, h, planes = pic
            else:
                with open(pic, "rb") as fh:
                    w, h, planes = sipic.decode_np(fh.read())
            depth = None
            src = pic if isinstance(pic, str) else getattr(pic, "source", None)   # mrfx.Picture: the sky put behind a .pic
            if depsgraph.view_layer.use_pass_z and src:
                zp = src[:-len(".pic")] + ".Zpic"
                if os.path.isfile(zp):
                    with open(zp, "rb") as fh:
                        fw, x0 = getattr(self, "_zgeom", (None, 0))
                        depth = sipic.decode_zpic(fh.read(), w, h, fw, x0)
                if depth is None:
                    self.report({"WARNING"}, "Fogle3D: no depth file of the picture's size (%s); the Depth pass is empty" % os.path.basename(zp))
            if region is not None and (w, h) != tuple(region[2:]):
                w, h, planes, depth = _crop_to(w, h, planes, region, scene.render, depth)
        except Exception as e:  # shown in Blender's status bar, render ends
            if str(e) == "cancelled":
                self._cancelled(scene, None)
                return
            self.error_set("Fogle3D: %s" % e)
            self.report({"ERROR"}, "Fogle3D: %s" % e)
            return
        r = scene.render
        rw = int(r.resolution_x * r.resolution_percentage / 100)
        rh = int(r.resolution_y * r.resolution_percentage / 100)
        self.update_progress(1.0)
        bg = None
        if session.tool_scene(st) is None and scene.world and not r.film_transparent:
            from . import export
            bg = export._world_colour(scene.world)   # Cycles shows the world colour where nothing is hit
        self._deliver(w, h, planes, rw, rh, st.colour, region, r.use_crop_to_border, st.alpha_pass, bg, depth)

    def view_update(self, context, depsgraph):
        # Rendered viewport (viewport.py): the scene changed, or the viewport started
        from . import viewport
        viewport.view_update(self, context, depsgraph)

    def view_draw(self, context, depsgraph):
        from . import viewport
        viewport.view_draw(self, context, depsgraph)

    def _cancelled(self, scene, region):
        self.update_stats("Fogle3D", "cancelled")
        if not self.test_break():
            # cancelled through session.CANCEL_FLAG, not Blender's Esc: Blender has not been told to
            # stop and would write a missing result, so hand it an empty one of the right size
            r = scene.render
            if region is None and r.use_border:      # the result is the region's box (unit finish1)
                from . import export
                region = export.region_of(scene)[1]
            bw, bh = (region[2], region[3]) if region else (int(r.resolution_x * r.resolution_percentage / 100),
                                                          int(r.resolution_y * r.resolution_percentage / 100))
            result = self.begin_result(0, 0, bw, bh)
            import numpy as np
            result.layers[0].passes["Combined"].rect.foreach_set(np.zeros(bw * bh * 4, np.float32))
            self.end_result(result)

    def _render_scene(self, scene, st, tool, frame, want_z=False):
        """Tools only (session.ToolScene): the installed .dsc rendered directly: (picture path, region box in Blender pixels
        or None), (None, None) on Esc."""
        from . import session
        installed = session.install_root(st, bpy.path.abspath)
        exe = session.renderer_path(st, bpy.path.abspath)
        dsc = session.find_scene(installed, tool.filter)
        info = session.scene_info(dsc)
        frames = session.scene_frames(info)
        fmode = tool.frame_map
        sf = session.map_frame(frame, fmode, info, scene.frame_start)
        if frames is not None and sf not in frames and fmode == "SAME" and info["first"] is not None:
            alt = session.map_frame(frame, "START", info, scene.frame_start)
            fmode, sf = "START", alt    # friendly default (when neither fits, the error names these frames)
            if alt in frames:
                self.report({"INFO"}, "Fogle3D: Blender frame %d is outside the scene's range %d-%d, so 'Start to first' is used "
                            "(Blender frame %d = scene frame %d); set the tool's frame_map to choose" % (
                                frame, info["first"], info["last"], scene.frame_start, info["first"]))
        if frames is not None and sf not in frames:
            raise session.SessionError(session.frame_range_message(sf, frame, fmode, info, scene.frame_start))
        r = scene.render
        rw = int(r.resolution_x * r.resolution_percentage / 100)
        rh = int(r.resolution_y * r.resolution_percentage / 100)
        frac = box = None
        if r.use_border:
            frac, box = session.region_fractions((r.border_min_x, r.border_max_x, r.border_min_y, r.border_max_y), rw, rh)
        alone = st.frame_mode == "ALONE"
        # depth file geometry when the picture is a sub-region: (full frame width, first column)
        left = frac[0] if frac else session.own_region_left(dsc)
        self._zgeom = (info["width"], int(round(left * info["width"]))) if (left is not None and info["width"]) else (None, 0)
        animation = bool(self.is_animation) and not alone
        # the frames to render: an animation render needs Blender's frame range only (mapped), a still the one frame
        if alone:
            want = [sf]
        elif frames is None:
            want = None
        elif animation:
            lo = session.map_frame(scene.frame_start, fmode, info, scene.frame_start)
            hi = session.map_frame(scene.frame_end, fmode, info, scene.frame_start)
            want = [f for f in frames if lo <= f <= hi] or frames
            if sf not in want:
                want = frames
        else:
            want = [sf]
        key = (dsc, installed, exe, frac, want_z, tuple(want[:1] + want[-1:]) if want else None, animation)
        z_patch = want_z and not session.has_z_channel(dsc)

        def make():
            work = session.new_dir(bpy.path.abspath(st.keep_dir))
            root, only = installed, tool.filter
            if frac is not None or z_patch:
                root = session.make_overlay(installed, dsc, os.path.join(work, "overlay"), region=frac, z_channel=z_patch)
                only = "/SCENES/" + os.path.basename(dsc)
            self.update_stats("Fogle3D", "starting the renderer")
            from . import install       # SI_INSTALL: the program file is read from the install (root may be an overlay)
            env = dict(install.program_env(installed), **({"RENDER_FRAME": str(sf)} if alone else {}))
            child = session.start_run(exe, root, only, env,
                                      os.path.join(work, "pictures"), want, animation, dsc, frames,
                                      st=st, size=(info["width"], info["height"]))
            return session.RangeRun(child, child.frames, work)

        run = session.get_run(self, key, make)
        return session.wait_for(self, run.child, sf, run.frames), box

    def _deliver(self, w, h, planes, rw, rh, colour, box, crop, alpha_pass, bg=None, depth=None):
        """Hands the picture (planes of w x h bytes, file row 0 at the top) to Blender's result: the border box (x0, y0, bw, bh)
        or the whole rw x rh frame, nearest-neighbour resampled when the sizes differ.  Bulk numpy, one foreach_set per pass;
        every value is computed as the per-pixel form computed it (byte -> float64 table, the same float64 operations for the
        world colour under a pixel that is not opaque, Python's float -> float32 rounding), so the floats Blender gets are
        the same bits."""
        import numpy as np
        x0, y0, bw, bh = box if box else (0, 0, rw, rh)
        pr, pg, pb = _plane(planes["R"]), _plane(planes["G"]), _plane(planes["B"])
        pa = _plane(planes["A"]) if "A" in planes else None
        # Blender rows run bottom to top; the file's row 0 is the top row.  Source pixel of output (ox, oy):
        # row (bh - 1 - oy) * h // bh, column ox * w // bw.
        if bw == w and bh == h:
            def pick(p):
                return p.reshape(h, w)[::-1]
        else:
            sy = (bh - 1 - np.arange(bh, dtype=np.int64)) * h // bh
            sx = np.arange(bw, dtype=np.int64) * w // bw
            idx = (sy * w)[:, None] + sx[None, :]

            def pick(p):
                return p[idx]
        r8, g8, b8 = pick(pr), pick(pg), pick(pb)
        a8 = pick(pa) if pa is not None else None
        keep = bool(os.environ.get("SI3D_KEEP_DELIVERY"))
        val = _byte_values(colour)                                    # float64 per byte value
        a64 = np.arange(256, dtype=np.float64) / 255.0               # a = A / 255.0
        # per channel: a float64 table and the index into it of each pixel (the float64 values are the per-pixel form's;
        # Blender gets them as float32, the rounding foreach_set made of Python floats)
        if bg is not None and a8 is not None:
            # the picture's colour is premultiplied: over the world colour where a < 1 (A < 255): v + (1 - a) * bg, a = 1
            rest = 1.0 - a64[:255]
            a16 = a8.astype(np.intp) << 8
            chans = []
            for c, p in enumerate((r8, g8, b8)):
                tab = np.empty((256, 256), np.float64)
                tab[:255] = val[None, :] + (rest * float(bg[c]))[:, None]
                tab[255] = val
                chans.append((tab.ravel(), a16 | p))
            ach = None                                                # alpha 1 everywhere
        else:
            chans = [(val, r8), (val, g8), (val, b8)]
            ach = (a64, a8) if a8 is not None else None
        rgba = np.empty((bh, bw, 4), np.float32)
        for c, (tab, ix) in enumerate(chans):
            rgba[..., c] = tab.astype(np.float32)[ix]
        rgba[..., 3] = ach[0].astype(np.float32)[ach[1]] if ach is not None else 1.0
        alpha = a64.astype(np.float32)[a8] if a8 is not None else np.ones((bh, bw), np.float32)
        zout = z64 = None
        if depth is not None:
            z = pick(np.frombuffer(depth, np.float32)).astype(np.float64)
            # camera looks down -z: distance = -z; nothing hit: Blender's background depth
            z64 = np.where(z <= ZPIC_NOTHING, DEPTH_BACKGROUND, -z)
            zout = z64.astype(np.float32)
        # Blender's result rectangle is the border box itself (it places it in the full frame when
        # 'Crop to Border' is off), so the tile always starts at 0, 0.
        result = self.begin_result(0, 0, bw, bh)
        layer = result.layers[0]
        layer.passes["Combined"].rect.foreach_set(rgba.ravel())
        if alpha_pass and "Alpha" in layer.passes:
            layer.passes["Alpha"].rect.foreach_set(np.ascontiguousarray(alpha).ravel())
        if zout is not None and "Depth" in layer.passes:
            layer.passes["Depth"].rect.foreach_set(np.ascontiguousarray(zout).ravel())
        names = list(layer.passes.keys())
        self.end_result(result)
        from . import session
        if keep:
            # the test copy holds the values as the per-pixel form listed them (Python floats, before the float32 rounding)
            v = np.empty((bh, bw, 4), np.float64)
            for c, (tab, ix) in enumerate(chans):
                v[..., c] = tab[ix]
            v[..., 3] = ach[0][ach[1]] if ach is not None else 1.0
            _DELIVERED.append({"box": (x0, y0, bw, bh), "crop": crop, "pic": (w, h), "rgba": v.ravel().tolist(),
                               "alpha": a64[a8].ravel().tolist() if a8 is not None else [1.0] * (bw * bh),
                               "depth": z64.ravel().tolist() if z64 is not None else None,
                               "passes": names, "time": __import__("time").time(),
                               "child_running": any(r.child.proc.poll() is None for r in session._runs.values())})


def _wrap(text, n):
    import textwrap
    return textwrap.wrap(text, n) or [""]


class SI3D_PT_settings(bpy.types.Panel):
    bl_label = "Fogle3D"
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    bl_context = "render"
    COMPAT_ENGINES = {ENGINE_ID}

    @classmethod
    def poll(cls, context):
        return context.engine in cls.COMPAT_ENGINES

    def draw(self, context):
        from . import session
        st = context.scene.si3d
        bad = session.problems(st, bpy.path.abspath)
        if bad:
            box = self.layout.box()
            box.alert = True
            box.label(text="Fogle3D cannot render yet", icon="ERROR")
            for line in bad:
                for part in _wrap(line, 52):
                    box.label(text=part)
        col = self.layout.column()
        col.prop(st, "renderer")
        col.prop(st, "installed")
        col.prop(st, "blend2db")
        col.prop(st, "colour")
        col.prop(st, "alpha_pass")
        col.prop(st, "fields")
        col.prop(st, "ray_depth")
        col.prop(st, "antialias")
        if st.antialias != "DEFAULT":
            row = col.row(align=True)
            for q in ("aa_filter", "aa_level", "aa_threshold"):
                row.prop(st, q)
        grid = col.grid_flow(columns=3, even_columns=True)
        for p in ("use_materials", "use_textures_2d", "use_textures_3d", "use_reflection", "use_refraction",
                  "use_shadows"):
            grid.prop(st, p)
        col.prop(st, "keep_dir")
        row = col.row()
        row.prop(st, "memory_mb")
        if not session.memory_mb_set(st) and session.tool_scene(st) is None:
            r = context.scene.render
            size = (int(r.resolution_x * r.resolution_percentage / 100), int(r.resolution_y * r.resolution_percentage / 100))
            row.label(text="automatic: %d MB" % session.auto_memory_mb(size, session.counts(1, False)[1]))


_classes = (SI3DPreferences, SI3DSettings, SI3DRenderEngine, SI3D_PT_settings)


# Files made before the rename hold the engine id SOFTIMAGE3D.  Blender reads an engine id that no registered engine has
# as the first engine (EEVEE) and loses the text, so while a file loads a stand-in engine of the old id is registered
# (load_pre), the id is rewritten to FOGLE3D (load_post) and the stand-in is removed again: it is never in the menu
# for longer than the load.
class SI3DLegacyEngine(bpy.types.RenderEngine):     # its own class: unregistering a subclass of the real engine breaks it
    bl_idname = LEGACY_ENGINE_ID
    bl_label = "Fogle3D (old id)"


_legacy_registered = False


def _legacy_pre(*_args):
    global _legacy_registered
    if not _legacy_registered:
        try:
            bpy.utils.register_class(SI3DLegacyEngine)
            _legacy_registered = True
        except Exception:
            pass


def _fix_legacy_engine(*_args):
    """load_post: every scene whose engine is the old id gets FOGLE3D; then the stand-in engine goes."""
    global _legacy_registered
    for sc in bpy.data.scenes:
        if sc.render.engine == LEGACY_ENGINE_ID:
            sc.render.engine = ENGINE_ID
    if _legacy_registered:
        try:
            bpy.utils.unregister_class(SI3DLegacyEngine)
        except Exception:
            pass
        _legacy_registered = False


def _handlers():
    from bpy.app.handlers import persistent
    return persistent(_legacy_pre), persistent(_fix_legacy_engine)


def register():
    for c in _classes:
        bpy.utils.register_class(c)
    bpy.types.Scene.si3d = PointerProperty(type=SI3DSettings)
    global _pre_h, _post_h
    _pre_h, _post_h = _handlers()
    bpy.app.handlers.load_pre.append(_pre_h)
    bpy.app.handlers.load_post.append(_post_h)
    from . import nodes
    nodes.register()
    from . import importer
    importer.register()


_pre_h = _post_h = None


def unregister():
    for lst, h in ((bpy.app.handlers.load_pre, _pre_h), (bpy.app.handlers.load_post, _post_h)):
        if h in lst:
            lst.remove(h)
    from . import importer
    importer.unregister()
    from . import nodes
    nodes.unregister()
    del bpy.types.Scene.si3d
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
    from . import runner, session, viewport
    viewport.shutdown()
    session.shutdown()
    runner.cleanup()
