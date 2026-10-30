# SPDX-License-Identifier: GPL-3.0-or-later
"""Softimage node trees (unit nodes1, our own code): which node feeds the output, the node fields as the `si` record the
exporter sends to blend2db, the faults a node draws, and the conversion of a Principled material into Softimage nodes
with the values the exporter would derive from it (blend2db's `material_for` formulas, step for step in single precision,
so the converted material writes the same material file).  Field meanings: docs/design/softimage-nodes.md."""
import math
import struct

from . import nodes as N

MATERIAL = "SI3DMaterialNode"
TEXTURE = "SI3DTextureNode"
TEXTURE3D = "SI3DTexture3DNode"
WORLD = "SI3DWorldNode"
MAX_LOCAL = len(N.TEXTURE_INPUTS)


# ---------------------------------------------------------------- tree walking

def _output(tree, idname):
    """The output node the render uses: the active one of that type (any target), else the first."""
    outs = [n for n in tree.nodes if n.bl_idname == idname]
    return next((n for n in outs if getattr(n, "is_active_output", False)), outs[0] if outs else None)


def _from_socket(socket):
    """The output behind an input (through reroutes, skipping muted links and nodes), or None."""
    seen = 0
    while socket is not None and socket.is_linked and seen < 64:
        link = socket.links[0]
        if getattr(link, "is_muted", False) or not getattr(link, "is_valid", True):
            return None
        node = link.from_node
        if node.bl_idname == "NodeReroute":
            socket = node.inputs[0]
            seen += 1
            continue
        return None if node.mute else link.from_socket
    return None


def _from(socket):
    """The node behind an input (through reroutes, skipping muted links and nodes), or None."""
    out = _from_socket(socket)
    return None if out is None else out.node


def uses_nodes(idblock):
    """Whether the material or world renders from its node tree (Blender 6 drops use_nodes: then always)."""
    return getattr(idblock, "use_nodes", True)


def enable_nodes(idblock):
    if not uses_nodes(idblock):
        idblock.use_nodes = True


def material_node(mat):
    """The Softimage Material that feeds the material's output Surface, or None (then the Principled path renders)."""
    if mat is None or not uses_nodes(mat) or mat.node_tree is None:
        return None
    out = _output(mat.node_tree, "ShaderNodeOutputMaterial")
    if out is None:
        return None
    node = _from(out.inputs.get("Surface"))
    return node if node is not None and node.bl_idname == MATERIAL else None


def world_node(world):
    """The Softimage World that feeds the world's output Surface, or None."""
    if world is None or getattr(world, "node_tree", None) is None:
        return None
    if not uses_nodes(world):
        return None
    out = _output(world.node_tree, "ShaderNodeOutputWorld")
    if out is None:
        return None
    node = _from(out.inputs.get("Surface"))
    return node if node is not None and node.bl_idname == WORLD else None


# ---------------------------------------------------------------- records

def srgb(c):
    """blend2db's encoding of a linear value (build.rs `srgb`), in double precision."""
    c = min(max(c, 0.0), 1.0)
    return c * 12.92 if c <= 0.0031308 else 1.055 * c ** (1.0 / 2.4) - 0.055


def linear(c):
    c = min(max(c, 0.0), 1.0)
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def f32(x):
    """x rounded to single precision (Rust's `as f32`)."""
    return struct.unpack("<f", struct.pack("<f", x))[0]


def texture_record(t):
    """A Softimage Texture node's fields under the names of the 2D texture record (si_hrc's Texture2D)."""
    return {"method": N.value_of(N.METHOD, t.method), "pixel_interp": 1 if t.interpolation else 0,
            "uscale": t.uscale, "vscale": t.vscale, "uoffset": t.uoffset, "voffset": t.voffset,
            "urepeat": t.urepeat, "vrepeat": t.vrepeat, "alt_u": int(t.alt_u), "alt_v": int(t.alt_v),
            "swap_uv": int(t.swap_uv), "mask": N.value_of(N.MASK, t.mask), "blend": t.blend,
            "channel": N.value_of(N.CHANNEL, t.channel), "ambient": t.ambient, "diffuse": t.diffuse,
            "specular": t.specular, "reflectivity": t.reflectivity, "transparency": t.transparency,
            "roughness": t.roughness, "reflection_map": t.reflection_map, "black_white": int(t.black_white),
            "crop_flag": int(t.crop), "crop": [t.crop_x0, t.crop_x1, t.crop_y0, t.crop_y1],
            # the projection's frame (unit imp3; blend2db reads it for the projected methods only)
            "rotation": list(t.rotation), "scaling": list(t.scaling), "translation": list(t.translation)}


def texture3d_record(t):
    return {"kind": N.value_of(N.KIND3D, t.kind), "spacing": t.spacing, "taper": t.taper, "strength": t.strength,
            "iterations": t.iterations, "power": t.power, "mask": N.value_of(N.MASK3D, t.mask),
            "blending": t.blending, "channel": N.value_of(N.CHANNEL, t.channel),
            "colours": [list(getattr(t, "colour%d" % k)) for k in range(1, 6)],
            "factors": [t.stop2, t.stop3, t.stop4], "scaling": list(t.scaling), "rotation": list(t.rotation),
            "translation": list(t.translation), "ambient": t.ambient, "diffuse": t.diffuse, "specular": t.specular,
            "reflection": t.reflection, "transparency": t.transparency, "roughness": t.roughness}


def _value(node, name):
    s = node.inputs.get(name)
    return None if s is None else s.value


def material_record(node):
    """The Softimage Material node's own fields (the `si` record without textures)."""
    return {"shading": N.value_of(N.SHADING, node.shading), "ambient": list(_value(node, "Ambient")),
            "diffuse": list(_value(node, "Diffuse")), "specular": list(_value(node, "Specular")),
            "exponent": _value(node, "Specular Decay"), "reflectivity": _value(node, "Reflectivity"),
            "transparency": _value(node, "Transparency"), "refraction": _value(node, "Refraction Index"),
            "blur": int(node.blur), "blur_width": node.blur_width, "blur_decay": node.blur_decay}


def textures_of(node):
    """(2D texture nodes in input order, the 3D texture node or None) linked into a Softimage Material."""
    two = []
    for name in N.TEXTURE_INPUTS:
        t = _from(node.inputs.get(name))
        if t is not None and t.bl_idname == TEXTURE:
            two.append(t)
    t3 = _from(node.inputs.get("3D Texture"))
    return two, (t3 if t3 is not None and t3.bl_idname == TEXTURE3D else None)


def si_material(mat, texdir, image_raw, image_mean):
    """The exporter's dict for a material whose output is a Softimage Material, or None.  `textures` holds the global
    textures (the old route: relation 400, objects split per texture), `si.textures` the local ones (relation 401);
    `diffuse` is the linear colour the light fit takes as the surface's albedo."""
    node = material_node(mat)
    if node is None:
        return None
    si = material_record(node)
    two, t3 = textures_of(node)
    glob, local = [], []
    for t in two:
        if t.image is None or t.image.size[0] == 0:
            continue
        e = {"image": t.image.filepath, "name": t.image.name, "raw": image_raw(t.image, texdir),
             "mean": image_mean(t.image), "si": texture_record(t)}
        (glob if t.scope == "GLOBAL" else local).append(e)
    si["textures"] = local
    if t3 is not None:
        si["texture3d"] = texture3d_record(t3)
    lit = list(node.fit_colour) if list(node.fit_key) == si["diffuse"] else [linear(c) for c in si["diffuse"]]
    return {"name": mat.name, "diffuse": lit + [1.0], "textures": glob, "si": si}


def world_background(node):
    """The linear colour where nothing is hit; also the world light the lights and the ambience are fitted to (as
    the Background's colour is without a Softimage World)."""
    return list(node.background)


def world_ambience(node):
    """The set-up's AMBIENCE, or None while it is fitted.  A set ambience replaces the fitted one; the lights stay as
    fitted with the background as the world light, so a larger ambience brightens the picture."""
    return None if node.fit_ambience else list(node.ambience)


def world_setup(node):
    """The set-up words of a Softimage World (blend2db replaces the template's lines)."""
    s = {}
    g = "%.9g"
    if node.fade:
        s.update({"FADE": "ACTIVE", "FADE_START": g % node.fade_start, "FADE_END": g % node.fade_end,
                  "FADE_COLOUR": " ".join(g % c for c in node.fade_colour),
                  "FADE_TRANSPARENCY": g % node.fade_transparency})
    if node.fog:
        s.update({"FOG": "ACTIVE", "FOG_START": g % node.fog_start, "FOG_END": g % node.fog_end,
                  "FOG_COLOUR": " ".join(g % c for c in node.fog_colour), "FOG_DENSITY": g % node.fog_density,
                  "FOG_BASE": g % node.fog_base, "FOG_THICKNESS": g % node.fog_thickness})
    return s


# ---------------------------------------------------------------- faults

def faults(node):
    """What the exporter cannot use of a Softimage node, as short sentences (empty: the node renders as shown)."""
    out = []
    tree = node.id_data
    kind = node.bl_idname
    in_world = any(n.bl_idname == "ShaderNodeOutputWorld" for n in tree.nodes) and \
        not any(n.bl_idname == "ShaderNodeOutputMaterial" for n in tree.nodes)
    if kind == MATERIAL:
        if in_world:
            out.append("A material node in a world tree: not used")
        else:
            o = _output(tree, "ShaderNodeOutputMaterial")
            if o is None or _from(o.inputs.get("Surface")) != node:
                out.append("Not on the Material Output's Surface: not used")
        for s in node.inputs:
            if s.bl_idname in (N.COLOUR, N.FACTOR, N.VALUE) and s.is_linked:
                out.append("%s: a link here is not read (its value is)" % s.name)
            elif s.is_linked:
                src = _from(s)
                want = TEXTURE3D if s.bl_idname == N.TEX3D else TEXTURE
                if src is not None and src.bl_idname != want:
                    out.append("%s takes a %s node" % (s.name, "Softimage 3D Texture" if want == TEXTURE3D
                                                       else "Softimage Texture"))
    elif kind in (TEXTURE, TEXTURE3D):
        if kind == TEXTURE and node.image is None:
            out.append("No image: the texture is left out")
        elif kind == TEXTURE and node.image.size[0] == 0:
            out.append("The image has no pixels (not found?)")
        targets = [l.to_node for s in node.outputs for l in s.links]
        if not any(t.bl_idname == MATERIAL for t in targets):
            out.append("Not linked to a Softimage Material: not used")
        want = N.TEX3D if kind == TEXTURE3D else N.TEX2D
        for s in node.outputs:
            for l in s.links:
                if l.to_socket.bl_idname != want:
                    out.append("Linked to %s, which does not take it" % l.to_socket.name)
    elif kind == WORLD:
        if not in_world:
            out.append("A world node in a material tree: not used")
        else:
            o = _output(tree, "ShaderNodeOutputWorld")
            if o is None or _from(o.inputs.get("Surface")) != node:
                out.append("Not on the World Output's Surface: not used")
        if node.fade and node.fade_end <= node.fade_start:
            out.append("Fade end is not beyond its start")
    return out


def refresh(tree):
    """Every Softimage node of the tree turns red while it has a fault (drawn on the node)."""
    if tree is None or getattr(tree, "bl_idname", "") != "ShaderNodeTree":
        return
    for n in tree.nodes:
        if n.bl_idname in N.NODE_IDS:
            bad = bool(faults(n))
            if n.use_custom_color != bad:
                n.use_custom_color = bad
            if bad:
                n.color = (0.6, 0.15, 0.15)


def draw_faults(layout, lines):
    if not lines:
        return
    box = layout.box()
    box.alert = True
    for line in lines:
        box.label(text=line, icon="ERROR")


# ---------------------------------------------------------------- building trees

def _place(node, x, y):
    node.location = (x, y)
    return node


def new_material_node(mat, values=None):
    """A Softimage Material on the material's output Surface (an output is made if there is none); the nodes that fed
    the Surface stay in the tree, unlinked.  `values`: a material record (material_record's keys) to set."""
    enable_nodes(mat)
    tree = mat.node_tree
    out = _output(tree, "ShaderNodeOutputMaterial")
    if out is None:
        out = _place(tree.nodes.new("ShaderNodeOutputMaterial"), 300.0, 0.0)
    node = _place(tree.nodes.new(MATERIAL), out.location.x - 320.0, out.location.y)
    if values:
        set_material(node, values)
    tree.links.new(node.outputs["Material"], out.inputs["Surface"])
    return node


def set_material(node, v):
    node.shading = N.ident_of(N.SHADING, v.get("shading", 4), "PHONG")
    for key, name in (("ambient", "Ambient"), ("diffuse", "Diffuse"), ("specular", "Specular")):
        if key in v:
            node.inputs[name].value = v[key][:3]
    for key, name in (("exponent", "Specular Decay"), ("reflectivity", "Reflectivity"),
                      ("transparency", "Transparency"), ("refraction", "Refraction Index")):
        if key in v:
            node.inputs[name].value = v[key]
    node.blur = bool(v.get("blur", 0))
    node.blur_width = v.get("blur_width", 1.0)
    node.blur_decay = v.get("blur_decay", 3.0)


def set_texture(node, v):
    node.method = N.ident_of(N.METHOD, v.get("method", 4), "UV")
    node.interpolation = bool(v.get("pixel_interp", 1))
    node.mask = N.ident_of(N.MASK, v.get("mask", 3), "BLEND")
    node.channel = N.ident_of(N.CHANNEL, v.get("channel", 2), "INTENSITY")
    for k in ("uscale", "vscale", "uoffset", "voffset", "urepeat", "vrepeat", "blend", "ambient", "diffuse", "specular",
              "reflectivity", "transparency", "roughness", "reflection_map"):
        if k in v:
            setattr(node, k, v[k])
    for k in ("alt_u", "alt_v", "swap_uv", "black_white"):
        if k in v:
            setattr(node, k, bool(v[k]))
    if "crop_flag" in v:
        node.crop = bool(v["crop_flag"])
    for k, x in zip(("crop_x0", "crop_x1", "crop_y0", "crop_y1"), v.get("crop") or ()):
        setattr(node, k, int(x))
    for k in ("rotation", "scaling", "translation"):     # a projection's frame (unit imp3)
        if v.get(k) is not None:
            setattr(node, k, [float(x) for x in v[k][:3]])


def new_texture_node(mat_node, image, values=None, scope="LOCAL", socket=None):
    tree = mat_node.id_data
    used = [s for s in (mat_node.inputs[n] for n in N.TEXTURE_INPUTS) if not s.is_linked]
    target = mat_node.inputs[socket] if socket else (used[0] if used else None)
    k = N.TEXTURE_INPUTS.index(target.name) if target is not None else 0
    t = _place(tree.nodes.new(TEXTURE), mat_node.location.x - 320.0, mat_node.location.y - 60.0 - 300.0 * k)
    t.image = image
    t.scope = scope
    if values:
        set_texture(t, values)
    if target is not None:
        tree.links.new(t.outputs["Texture"], target)
    return t


# The exporter's texture of the Principled path (blend2db: method 4, ambient 1, diffuse 1, specular 0, mask 1, channel 2,
# the rest the record's defaults), as a Softimage Texture's fields.
PRINCIPLED_TEXTURE = {"method": 4, "pixel_interp": 1, "uscale": 1.0, "vscale": 1.0, "uoffset": 0.0, "voffset": 0.0,
                      "urepeat": 1, "vrepeat": 1, "alt_u": 0, "alt_v": 0, "swap_uv": 0, "mask": 1, "blend": 1.0,
                      "channel": 2, "ambient": 1.0, "diffuse": 1.0, "specular": 0.0, "reflectivity": 0.0,
                      "transparency": 0.0, "roughness": 0.0, "reflection_map": 1.0, "black_white": 0}


# blend2db gloss.rs's calibration (unit gloss1): kept equal to its TUNE.
GLOSS = {"k_spec": 0.6, "c_size": 24.0, "r_diel": 0.12, "mirror_lo": 0.1, "mirror_hi": 0.8, "c_tint": 0.45, "c_rough": 0.5,
         "n_max": 1000.0, "glass_o": 0.1, "glass_r": 0.0, "n_scale": 0.35}


def _lobe_energy(n):
    s = 0.0
    for i in range(200):
        vi = (i / 200.0) ** n
        step = 1.0 if i == 199 else ((i + 1) / 200.0) ** n - vi
        s += vi + step / 2.0
    return 2.0 * math.pi * s / 200.0


def _peak(n):
    return (math.pi / 4.0) / _lobe_energy(n)


def _exponent(rough, spread, t):
    r = min(max(rough, 0.02), 1.0)
    a = r * r
    n_r = max(t["n_scale"] * (2.0 / (a * a) - 2.0), 1.0)
    inv = 1.0 / n_r + (spread * spread / t["c_size"] if spread > 0.0 else 0.0)
    return min(max(1.0 / inv, 1.0), t["n_max"])


def gloss_values(d, t=GLOSS):
    """blend2db gloss.rs `map` on the exporter's dict of an opaque Principled material: (diffuse, specular, exponent,
    reflectivity), every operation in the same order in double precision, single precision at the end."""
    def num(key, default):
        v = d.get(key)
        return float(default if v is None else v)

    def three(key, default):
        v = d.get(key)
        if not isinstance(v, (list, tuple)):
            return [default] * 3
        return [float(v[i]) if i < len(v) and v[i] is not None else default for i in range(3)]
    spread = max(num("key_spread", 0.0), 0.0)
    tb = [srgb(c) for c in three("diffuse", 0.8)]
    rough = min(max(num("roughness", 0.5), 0.0), 1.0)
    metal = min(max(num("metallic", 0.0), 0.0), 1.0)
    lv = d.get("specular_level")
    lv = d.get("specular") if lv is None else lv
    level = min(max(float(0.5 if lv is None else lv), 0.0), 1.0)
    tint = [min(max(c, 0.0), 1.0) for c in three("specular_tint", 1.0)]
    ior = max(num("ior", 1.5), 1.0)
    q = (ior - 1.0) / (ior + 1.0)
    f0 = min(max(q * q * 2.0 * level, 0.0), 1.0)
    n = _exponent(rough, spread, t)
    pk = _peak(n) * t["k_spec"]
    w = min(max((t["mirror_hi"] - rough) / max(t["mirror_hi"] - t["mirror_lo"], 1e-6), 0.0), 1.0)
    r_d = min(max(t["r_diel"] * w, 0.0), 0.99)
    spec_d = [f0 * k * pk for k in tint]
    tmin = min(1.0, tb[0], tb[1], tb[2])
    r_m = min(max(w * tmin, 0.0), 0.99)
    loc_m = [w * (tb[k] - tmin) * t["c_tint"] + (1.0 - w) * tb[k] * t["c_rough"] for k in range(3)]
    spec_m = [k * pk for k in tb]
    coat = min(max(num("coat", 0.0), 0.0), 1.0)
    n_out, spec_c = n, 0.0
    if coat > 0.0:
        cior = max(num("coat_ior", 1.5), 1.0)
        cq = (cior - 1.0) / (cior + 1.0)
        cf0 = cq * cq
        nc = _exponent(num("coat_roughness", 0.03), spread, t)
        eb = metal * (tb[0] + tb[1] + tb[2]) / 3.0 + (1.0 - metal) * f0
        ec = coat * cf0
        n_out = math.exp((eb * math.log(n) + ec * math.log(nc)) / max(eb + ec, 1e-12))
        spec_c = coat * cf0 * _peak(nc) * t["k_spec"]
    r = metal * r_m + (1.0 - metal) * r_d
    keep = max(1.0 - r, 1e-3)
    diffuse = [f32((metal * loc_m[k] + (1.0 - metal) * (1.0 - r_d) * tb[k]) / keep) for k in range(3)]
    specular = [f32((metal * spec_m[k] + (1.0 - metal) * (1.0 - r_d) * spec_d[k] + spec_c) / keep) for k in range(3)]
    return diffuse, specular, f32(n_out), f32(r)


def glass_values(d, t=GLOSS):
    """blend2db gloss.rs `glass` on the exporter's dict of a clear Principled glass: (diffuse, specular, exponent,
    reflectivity, transparency), or None when the calibration keeps glass as before."""
    if t["glass_o"] <= 0.0:
        return None

    def num(key, default):
        v = d.get(key)
        return float(default if v is None else v)

    def three(key, default):
        v = d.get(key)
        if not isinstance(v, (list, tuple)):
            return [default] * 3
        return [float(v[i]) if i < len(v) and v[i] is not None else default for i in range(3)]
    spread = max(num("key_spread", 0.0), 0.0)
    trans = min(max(num("transmission", 0.0), 0.0), 1.0)
    o = min(max(t["glass_o"] * trans + (1.0 - trans), 1e-3), 1.0)
    lv = d.get("specular_level")
    lv = d.get("specular") if lv is None else lv
    level = min(max(float(0.5 if lv is None else lv), 0.0), 1.0)
    tint = [min(max(c, 0.0), 1.0) for c in three("specular_tint", 1.0)]
    ior = max(num("ior", 1.5), 1.0)
    q = (ior - 1.0) / (ior + 1.0)
    f0 = min(max(q * q * 2.0 * level, 0.0), 1.0)
    n = _exponent(min(max(num("roughness", 0.0), 0.0), 1.0), spread, t)
    r = min(max(t["glass_r"] / o, 0.0), 0.95)
    keep = o * (1.0 - r)
    specular = [f32(f0 * k * _peak(n) * t["k_spec"] / keep) for k in tint]
    return [0.0, 0.0, 0.0], specular, f32(n), f32(r), f32(1.0 - o)


def principled_values(d):
    """blend2db's `material_for` on the exporter's dict of a Principled material: the material record it writes.
    Double precision where the Rust code is, single precision where it is (every value ends as f32)."""
    def num(key, default):
        v = d.get(key, default)
        return float(default if v is None else v)
    base = list(d.get("diffuse", [0.8, 0.8, 0.8, 1.0]))[:3]
    diffuse = [f32(srgb(float(c))) for c in base]
    rough = min(max(num("roughness", 0.5), 0.05), 1.0)
    spec_i = min(max(num("specular", 0.5), 0.0), 1.0)
    metal = min(max(num("metallic", 0.0), 0.0), 1.0)
    alpha = min(max(num("alpha", 1.0), 0.0), 1.0)
    exponent = f32(min(max(2.0 / (rough * rough) - 2.0, 1.0), 500.0))
    s = f32(min(max(spec_i * (1.0 - rough) * 0.5, 0.0), 1.0))
    m32 = f32(metal)
    one_m = f32(1.0 - m32)
    spec = [f32(f32(s * one_m) + f32(diffuse[i] * m32)) for i in range(3)]
    trans = min(max(num("transmission", 0.0), 0.0), 1.0)
    transp = 1.0 - alpha * (1.0 - trans)
    v = {"shading": 4, "ambient": list(diffuse), "diffuse": list(diffuse), "specular": spec, "exponent": exponent,
         "reflectivity": m32, "transparency": f32(transp), "refraction": 1.0, "blur": 0, "blur_width": 1.0,
         "blur_decay": 3.0}
    e = d.get("emission")
    if isinstance(e, (list, tuple)):
        ec = [f32(min(max(float(e[i]) if i < len(e) else 0.0, 0.0), 1.0)) for i in range(3)]
        if max(ec) > 0.0:
            v["shading"] = 1
            v["diffuse"] = list(ec)
            v["ambient"] = list(ec)
    if transp <= 0.0 and e is None:     # an opaque surface: blend2db gloss.rs (unit gloss1)
        g = gloss_values(d)
        v.update({"ambient": list(g[0]), "diffuse": list(g[0]), "specular": list(g[1]), "exponent": g[2],
                  "reflectivity": g[3], "transparency": 0.0})
    elif trans > 0.0 and alpha >= 1.0 and e is None:    # a clear glass: gloss.rs `glass`
        g = glass_values(d)
        if g is not None:
            v.update({"ambient": list(g[0]), "diffuse": list(g[0]), "specular": list(g[1]), "exponent": g[2],
                      "reflectivity": g[3], "transparency": g[4]})
    if d.get("ior") is not None and transp > 0.0:
        v["refraction"] = f32(float(d["ior"]))
    return v


def _to_inputs(socket, seen=0):
    """The (node, input) pairs an output feeds, through reroutes, skipping muted links and nodes."""
    out = []
    for link in socket.links:
        if getattr(link, "is_muted", False) or not getattr(link, "is_valid", True):
            continue
        node = link.to_node
        if node.bl_idname == "NodeReroute" and seen < 64:
            out += _to_inputs(node.outputs[0], seen + 1)
        elif not node.mute:
            out.append((node, link.to_socket))
    return out


def _whole(v):
    return abs(v - round(v)) < 1e-6 and 1 <= round(v) <= 32767


def image_links(img_node):
    """What the engine carries of the links around a Principled material's Image Texture beyond Color -> Base Color,
    as overrides of its texture record (blend2db's global texture; empty when there is no such link, and the material
    exports as it did before these were read):
      Alpha -> the Principled BSDF's Alpha: the picture's alpha is the opacity (mask Blend, transparency factor -1 read
        from the alpha channel: opacity = alpha), the colour is the picture's as in Blender;
      a Mapping (Point) from the UV coordinates into Vector, with whole positive X and Y scales, no move, no turn, the
        image repeating: the repeats (urepeat, vrepeat).
    Other links (roughness, normal or bump maps, other mappings) are not carried."""
    o = {}
    if any(n.bl_idname == "ShaderNodeBsdfPrincipled" and s.name == "Alpha"
           for n, s in _to_inputs(img_node.outputs["Alpha"])):
        o.update({"mask": 3, "blend": 1.0, "transparency": -1.0, "channel": 1})
    mp = _from(img_node.inputs.get("Vector"))
    if mp is not None and mp.bl_idname == "ShaderNodeMapping" and mp.vector_type == "POINT" and \
            getattr(img_node, "extension", "REPEAT") == "REPEAT":
        out = _from_socket(mp.inputs["Vector"])
        uv = out is not None and (out.node.bl_idname == "ShaderNodeUVMap" or (
            out.node.bl_idname == "ShaderNodeTexCoord" and out.name == "UV"))
        loc, rot, sca = (mp.inputs[k] for k in ("Location", "Rotation", "Scale"))
        if uv and not (loc.is_linked or rot.is_linked or sca.is_linked) and \
                max(abs(c) for c in loc.default_value) < 1e-9 and max(abs(c) for c in rot.default_value) < 1e-9 and \
                _whole(sca.default_value[0]) and _whole(sca.default_value[1]):
            o.update({"urepeat": int(round(sca.default_value[0])), "vrepeat": int(round(sca.default_value[1]))})
    return o


def principled_image(mat):
    """The image the Principled path puts on the model: the first Image Texture node with pixels, in node order."""
    tree = mat.node_tree if uses_nodes(mat) else None
    for n in (tree.nodes if tree else ()):
        if n.bl_idname == "ShaderNodeTexImage" and n.image is not None and n.image.size[0] > 0:
            return n.image
    return None


def convert_principled(mat):
    """Softimage nodes holding what the exporter derives from this material now (its Principled BSDF, or its own colour
    without one, and the first image texture as a global texture).  Returns (material node, notes); notes name inputs
    whose links the Principled path does not read either.  A material that already renders through a Softimage
    Material is left as is (returns (None, [...]))."""
    from . import export
    if material_node(mat) is not None:
        return None, ["%s already has a Softimage Material" % mat.name]
    d = export._material(mat, None)
    if "si" not in d:       # the highlight's size, as the exporter gives it (unit gloss1)
        import bpy
        d["key_spread"] = export.key_spreads(bpy.context.evaluated_depsgraph_get()).get(mat.name, 0.0)
    notes = []
    tree = mat.node_tree if uses_nodes(mat) else None
    p = next((n for n in tree.nodes if n.bl_idname == "ShaderNodeBsdfPrincipled"), None) if tree else None
    if p is not None:
        linked = [s.name for s in p.inputs if s.is_linked and s.name != "Base Color"]
        if linked:
            notes.append("%s: linked inputs carried as their values: %s" % (mat.name, ", ".join(linked)))
    v = principled_values(d)
    node = new_material_node(mat, v)
    node.fit_colour = d["diffuse"][:3]
    node.fit_key = node.inputs["Diffuse"].value
    img = principled_image(mat)
    if img is not None:
        e = next((t for t in d["textures"] if t["name"] == img.name), {})
        new_texture_node(node, img, dict(PRINCIPLED_TEXTURE, **e.get("si", {})), scope="GLOBAL")
    refresh(mat.node_tree)
    return node, notes


def convert_world(world):
    """A Softimage World on the world's output that renders as the world does now: the Background's colour (times its
    strength) as the background, ambience fitted from it.  Returns the node, or None when the world has one."""
    from . import export
    enable_nodes(world)
    tree = world.node_tree
    if world_node(world) is not None:
        return None
    col = export._world_colour(world)
    out = _output(tree, "ShaderNodeOutputWorld")
    if out is None:
        out = _place(tree.nodes.new("ShaderNodeOutputWorld"), 300.0, 0.0)
    node = _place(tree.nodes.new(WORLD), out.location.x - 300.0, out.location.y)
    node.fit_ambience = True
    node.background = [max(c, 0.0) for c in col[:3]]
    node.ambience = [f32(srgb(c)) for c in col[:3]]
    tree.links.new(node.outputs["World"], out.inputs["Surface"])
    refresh(tree)
    return node
