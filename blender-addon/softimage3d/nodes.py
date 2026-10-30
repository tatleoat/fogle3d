# SPDX-License-Identifier: GPL-3.0-or-later
"""Softimage nodes for Blender's shader editor (unit nodes1, our own code).

Sockets and the Softimage Material, Texture, 3D Texture and World nodes.  Each node holds the fields of one record of
the program (material, 2D texture, 3D texture, the set-up's ambience, fade and fog), in Softimage's own values (colours as
a material file stores them, no colour management).  export.py reads them through material_nodes.py; what each field
does and what is not carried: docs/design/softimage-nodes.md."""
import bpy
from bpy.props import BoolProperty, EnumProperty, FloatProperty, FloatVectorProperty, IntProperty, PointerProperty

SHADER = "NodeSocketShader"
COLOUR = "SI3DColorSocket"
FACTOR = "SI3DFactorSocket"
VALUE = "SI3DValueSocket"
TEX2D = "SI3DTextureSocket"
TEX3D = "SI3DTexture3DSocket"

# (identifier, label, description, the number the record holds)
SHADING = (
    ("CONSTANT", "Constant", "Flat colour: the diffuse colour, no light (shading model 1)", 1),
    ("LAMBERT", "Lambert", "Ambient and diffuse light, no highlight (shading model 3)", 3),
    ("PHONG", "Phong", "Lambert plus a Phong highlight (shading model 4)", 4),
    ("BLINN", "Blinn", "Lambert plus a Blinn highlight (shading model 5)", 5),
)
METHOD = (
    ("XY", "XY plane", "Projected along Softimage Z, the plane of Blender's front view (X and Z)", 1),
    ("XZ", "XZ plane", "Projected along Softimage Y, the plane of Blender's top view (X and Y)", 2),
    ("YZ", "YZ plane", "Projected along Softimage X, the plane of Blender's side view (Y and Z)", 3),
    ("UV", "UV", "The mesh's active UV map", 4),
    ("CYLINDER", "Cylindrical", "Wrapped round the model's upright axis (Blender Z)", 5),
    ("SPHERE", "Spherical", "Wrapped round the model's centre", 6),
    ("REFLECT_RAY", "Reflection map (rays first)", "Environment seen in the surface; ray-traced reflections where a "
     "reflected ray hits something", 7),
    ("REFLECT", "Reflection map", "Environment seen in the surface, no reflected rays", 8),
)
MASK = (
    ("ALPHA", "Blend by alpha", "The blend is scaled by the picture's alpha", 1),
    ("INTENSITY", "Blend by intensity", "The blend is scaled by the picture's intensity", 2),
    ("BLEND", "Blend", "The blend as set", 3),
    ("MULTIPLY", "Multiply", "The picture multiplies the material's colours", 4),
)
CHANNEL = (
    ("ALPHA", "Alpha", "The picture's alpha drives reflectivity, transparency and bump", 1),
    ("INTENSITY", "Intensity", "The picture's intensity drives reflectivity, transparency and bump", 2),
)
SCOPE = (
    ("LOCAL", "Local (material)", "On the polygons of this material only (relation 401)", 0),
    ("GLOBAL", "Global (model)", "On the whole model; an object whose materials carry different global textures is cut "
     "into one model per texture (relation 400, the route Principled image textures take)", 1),
)
KIND3D = (
    ("MARBLE", "Marble", "Veins", 0),
    ("WOOD", "Wood", "Rings", 1),
    ("CLOUD", "Cloud", "Turbulence", 2),
)
MASK3D = (
    ("ALPHA", "Blend by alpha", "The blend is scaled by the ramp colour's alpha", 1),
    ("INTENSITY", "Blend by intensity", "The blend is scaled by the ramp colour's intensity", 2),
    ("BLEND", "Blend", "The blend as set", 3),
)


def _enum(rows, **kw):
    return EnumProperty(items=[(r[0], r[1], r[2], k) for k, r in enumerate(rows)], update=_changed, **kw)


def value_of(rows, ident):
    """The record's number of an enum identifier."""
    return next(r[3] for r in rows if r[0] == ident)


def ident_of(rows, number, default):
    return next((r[0] for r in rows if r[3] == number), default)


def _changed(self, context):
    from . import material_nodes
    material_nodes.refresh(getattr(self, "id_data", None))


# ---------------------------------------------------------------- sockets

class _ValueSocket:
    def draw(self, context, layout, node, text):
        if self.is_output or self.is_linked:
            layout.label(text=text)
        else:
            layout.prop(self, "value", text=text)


class SI3DColorSocket(_ValueSocket, bpy.types.NodeSocket):
    """A Softimage colour (stored as the material file holds it)"""
    bl_idname = COLOUR
    bl_label = "Softimage colour"
    value: FloatVectorProperty(name="Value", size=3, subtype="COLOR_GAMMA", min=0.0, soft_max=1.0,
                               default=(0.7, 0.7, 0.7), update=_changed)

    def draw_color(self, context, node):
        return (0.85, 0.75, 0.2, 1.0)


class SI3DFactorSocket(_ValueSocket, bpy.types.NodeSocket):
    """A Softimage factor, 0 to 1"""
    bl_idname = FACTOR
    bl_label = "Softimage factor"
    value: FloatProperty(name="Value", min=0.0, max=1.0, default=0.0, subtype="FACTOR", update=_changed)

    def draw_color(self, context, node):
        return (0.6, 0.6, 0.6, 1.0)


class SI3DValueSocket(_ValueSocket, bpy.types.NodeSocket):
    """A Softimage value"""
    bl_idname = VALUE
    bl_label = "Softimage value"
    value: FloatProperty(name="Value", min=0.0, soft_max=500.0, default=1.0, update=_changed)

    def draw_color(self, context, node):
        return (0.45, 0.45, 0.45, 1.0)


class SI3DTextureSocket(bpy.types.NodeSocket):
    """A Softimage 2D texture (from a Softimage Texture node)"""
    bl_idname = TEX2D
    bl_label = "Softimage texture"

    def draw(self, context, layout, node, text):
        layout.label(text=text)

    def draw_color(self, context, node):
        return (0.95, 0.45, 0.15, 1.0)


class SI3DTexture3DSocket(bpy.types.NodeSocket):
    """A Softimage 3D texture (from a Softimage 3D Texture node)"""
    bl_idname = TEX3D
    bl_label = "Softimage 3D texture"

    def draw(self, context, layout, node, text):
        layout.label(text=text)

    def draw_color(self, context, node):
        return (0.75, 0.3, 0.75, 1.0)


# ---------------------------------------------------------------- nodes

class _SINode:
    """Inputs and outputs from the class's tables: (name, socket type, default value or None)."""
    INPUTS = ()
    OUTPUTS = ()

    @classmethod
    def poll(cls, ntree):
        return ntree.bl_idname == "ShaderNodeTree"

    def init(self, context):
        for name, kind, default in self.INPUTS:
            s = self.inputs.new(kind, name)
            if default is not None:
                s.value = default
        for name, kind in self.OUTPUTS:
            self.outputs.new(kind, name)

    def update(self):
        from . import material_nodes
        material_nodes.refresh(self.id_data)

    def draw_faults(self, layout):
        from . import material_nodes
        material_nodes.draw_faults(layout, material_nodes.faults(self))


TEXTURE_INPUTS = ("Texture", "Texture 2", "Texture 3", "Texture 4")


class SI3DMaterialNode(_SINode, bpy.types.Node):
    """A Softimage material: shading model, colours, highlight, reflection, transparency, refraction, textures"""
    bl_idname = "SI3DMaterialNode"
    bl_label = "Softimage Material"
    bl_icon = "MATERIAL"
    bl_width_default = 240
    INPUTS = (("Ambient", COLOUR, (0.5, 0.5, 0.5)), ("Diffuse", COLOUR, (0.7, 0.7, 0.7)),
              ("Specular", COLOUR, (1.0, 1.0, 1.0)), ("Specular Decay", VALUE, 50.0),
              ("Reflectivity", FACTOR, 0.0), ("Transparency", FACTOR, 0.0), ("Refraction Index", VALUE, 1.0)) + \
        tuple((n, TEX2D, None) for n in TEXTURE_INPUTS) + (("3D Texture", TEX3D, None),)
    OUTPUTS = (("Material", SHADER),)

    shading: _enum(SHADING, name="Shading", default="PHONG", description="The shading model")
    blur: BoolProperty(name="Static blur", default=False, update=_changed,
                       description="Blur the material's surface (the material's static blur)")
    blur_width: FloatProperty(name="Blur width", default=1.0, min=0.0, update=_changed)
    blur_decay: FloatProperty(name="Blur decay", default=3.0, min=0.0, update=_changed)
    # The light fit (blend2db) takes a surface's linear colour as Cycles sees it.  A converted material keeps the colour
    # it came from, used while Diffuse still holds the value the conversion wrote (the round trip through the display
    # encoding loses the last bits, which would move the fitted lights).
    fit_colour: FloatVectorProperty(name="Fit colour", size=3, subtype="COLOR", options={"HIDDEN"})
    fit_key: FloatVectorProperty(name="Fit key", size=3, default=(-1.0, -1.0, -1.0), options={"HIDDEN"})

    def draw_buttons(self, context, layout):
        self.draw_faults(layout)
        layout.prop(self, "shading", text="")

    def draw_buttons_ext(self, context, layout):
        self.draw_buttons(context, layout)
        layout.prop(self, "blur")
        if self.blur:
            layout.prop(self, "blur_width")
            layout.prop(self, "blur_decay")


class SI3DTextureNode(_SINode, bpy.types.Node):
    """A Softimage 2D texture: a picture, how it is projected, what it changes in the material"""
    bl_idname = "SI3DTextureNode"
    bl_label = "Softimage Texture"
    bl_icon = "TEXTURE"
    bl_width_default = 260
    OUTPUTS = (("Texture", TEX2D),)

    image: PointerProperty(name="Image", type=bpy.types.Image, update=_changed)
    scope: _enum(SCOPE, name="Scope", default="LOCAL")
    method: _enum(METHOD, name="Method", default="UV", description="How the picture is put on the surface")
    interpolation: BoolProperty(name="Interpolate", default=True, update=_changed,
                                description="Bilinear interpolation of the picture's pixels (off: nearest pixel)")
    mask: _enum(MASK, name="Mask", default="BLEND", description="What scales the blend")
    blend: FloatProperty(name="Blend", default=1.0, min=0.0, max=1.0, subtype="FACTOR", update=_changed,
                         description="How much of the picture replaces the material's colours")
    channel: _enum(CHANNEL, name="Channel", default="INTENSITY")
    ambient: FloatProperty(name="Ambient", default=0.75, soft_min=-1.0, soft_max=1.0, update=_changed,
                           description="Factor of the picture on the ambient colour (0: not driven)")
    diffuse: FloatProperty(name="Diffuse", default=1.0, soft_min=-1.0, soft_max=1.0, update=_changed,
                           description="Factor of the picture on the diffuse colour (0: not driven)")
    specular: FloatProperty(name="Specular", default=0.0, soft_min=-1.0, soft_max=1.0, update=_changed,
                            description="Factor of the picture on the specular colour (0: not driven)")
    reflectivity: FloatProperty(name="Reflectivity", default=0.0, soft_min=-1.0, soft_max=1.0, update=_changed,
                                description="Factor on reflectivity (negative: inverted channel)")
    transparency: FloatProperty(name="Transparency", default=0.0, soft_min=-1.0, soft_max=1.0, update=_changed,
                                description="Factor on transparency (negative: inverted channel)")
    roughness: FloatProperty(name="Bump", default=0.0, soft_min=-10.0, soft_max=10.0, update=_changed,
                             description="Bump: the channel's slope tilts the normal by this factor (0: no bump)")
    reflection_map: FloatProperty(name="Reflection map", default=1.0, min=0.0, soft_max=1.0, update=_changed,
                                  description="Strength of a reflection map (methods Reflection map)")
    black_white: BoolProperty(name="Key black and white", default=False, update=_changed,
                              description="Fully transparent where the picture is pure black or pure white")
    uscale: FloatProperty(name="U scale", default=1.0, update=_changed, description="Negative mirrors")
    vscale: FloatProperty(name="V scale", default=1.0, update=_changed, description="Negative mirrors")
    uoffset: FloatProperty(name="U offset", default=0.0, update=_changed)
    voffset: FloatProperty(name="V offset", default=0.0, update=_changed)
    urepeat: IntProperty(name="U repeat", default=1, min=1, max=32767, update=_changed)
    vrepeat: IntProperty(name="V repeat", default=1, min=1, max=32767, update=_changed)
    alt_u: BoolProperty(name="Alternate U", default=False, update=_changed, description="Every other repeat mirrored in U")
    alt_v: BoolProperty(name="Alternate V", default=False, update=_changed, description="Every other repeat mirrored in V")
    swap_uv: BoolProperty(name="Swap U and V", default=False, update=_changed)
    # the picture's cropping (unit fidelity2): only the box of pixels x0..x1, y0..y1 of the picture is used
    crop: BoolProperty(name="Crop", default=False, update=_changed,
                       description="Use only a box of the picture's pixels (Softimage's texture cropping)")
    crop_x0: IntProperty(name="Left", default=0, min=-32768, max=32767, update=_changed,
                         description="First pixel column of the box")
    crop_x1: IntProperty(name="Right", default=0, min=-32768, max=32767, update=_changed,
                         description="Last pixel column of the box")
    crop_y0: IntProperty(name="Bottom", default=0, min=-32768, max=32767, update=_changed,
                         description="First pixel row of the box")
    crop_y1: IntProperty(name="Top", default=0, min=-32768, max=32767, update=_changed,
                         description="Last pixel row of the box")
    # a projection's frame (unit imp3): the projected methods take the model's points in its own space through this
    # rotation, and scale the box of them about its centre; the cylinder and sphere move their centre by the
    # translation times the box's half size
    scaling: FloatVectorProperty(name="Projection scale", size=3, default=(1.0, 1.0, 1.0), update=_changed,
                                 description="Projected methods: the box scaled about its centre (Softimage axes: "
                                             "Y is Blender's Z)")
    rotation: FloatVectorProperty(name="Projection rotation", size=3, subtype="EULER", default=(0.0, 0.0, 0.0),
                                  update=_changed, description="Projected methods: the projection turned in the "
                                                               "model's own space (Softimage axes: Y is Blender's Z)")
    translation: FloatVectorProperty(name="Projection translation", size=3, default=(0.0, 0.0, 0.0), update=_changed,
                                     description="Cylindrical and spherical: the centre moved, in half box sizes "
                                                 "(Softimage axes: Y is Blender's Z)")

    def draw_buttons(self, context, layout):
        self.draw_faults(layout)
        layout.template_ID(self, "image", new="image.new", open="image.open")
        layout.prop(self, "scope", text="")
        layout.prop(self, "method", text="")
        layout.prop(self, "mask", text="")
        layout.prop(self, "blend")

    def draw_buttons_ext(self, context, layout):
        self.draw_buttons(context, layout)
        draw_texture_details(self, layout)


def draw_texture_details(node, layout):
    col = layout.column(align=True)
    col.label(text="Drives")
    for p in ("ambient", "diffuse", "specular", "reflectivity", "transparency", "roughness"):
        col.prop(node, p)
    if node.method in ("REFLECT_RAY", "REFLECT"):
        col.prop(node, "reflection_map")
    layout.prop(node, "channel")
    layout.prop(node, "interpolation")
    layout.prop(node, "black_white")
    col = layout.column(align=True)
    col.label(text="Mapping")
    row = col.row(align=True)
    row.prop(node, "uscale")
    row.prop(node, "vscale")
    row = col.row(align=True)
    row.prop(node, "uoffset")
    row.prop(node, "voffset")
    row = col.row(align=True)
    row.prop(node, "urepeat")
    row.prop(node, "vrepeat")
    row = col.row(align=True)
    row.prop(node, "alt_u")
    row.prop(node, "alt_v")
    col.prop(node, "swap_uv")
    col.prop(node, "crop")
    if node.crop:
        row = col.row(align=True)
        row.prop(node, "crop_x0")
        row.prop(node, "crop_x1")
        row = col.row(align=True)
        row.prop(node, "crop_y0")
        row.prop(node, "crop_y1")
    if node.method in ("XY", "XZ", "YZ", "CYLINDER", "SPHERE"):
        for p in ("scaling", "rotation", "translation"):
            layout.prop(node, p)


class SI3DTexture3DNode(_SINode, bpy.types.Node):
    """A Softimage 3D (solid) texture: marble, wood or cloud through a five-colour ramp"""
    bl_idname = "SI3DTexture3DNode"
    bl_label = "Softimage 3D Texture"
    bl_icon = "TEXTURE"
    bl_width_default = 260
    OUTPUTS = (("3D Texture", TEX3D),)

    kind: _enum(KIND3D, name="Kind", default="MARBLE")
    spacing: FloatProperty(name="Spacing", default=1.0, update=_changed)
    taper: FloatProperty(name="Taper", default=0.0, subtype="ANGLE", update=_changed)
    strength: FloatProperty(name="Strength", default=1.0, update=_changed)
    iterations: IntProperty(name="Iterations", default=1, min=1, max=32, update=_changed,
                            description="Number of noise terms")
    power: FloatProperty(name="Power", default=1.0, update=_changed)
    colour1: FloatVectorProperty(name="Colour 1", size=4, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                 default=(0.0, 0.0, 0.0, 0.0), update=_changed)
    colour2: FloatVectorProperty(name="Colour 2", size=4, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                 default=(0.25, 0.25, 0.25, 0.0), update=_changed)
    colour3: FloatVectorProperty(name="Colour 3", size=4, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                 default=(0.5, 0.5, 0.5, 0.0), update=_changed)
    colour4: FloatVectorProperty(name="Colour 4", size=4, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                 default=(0.75, 0.75, 0.75, 0.0), update=_changed)
    colour5: FloatVectorProperty(name="Colour 5", size=4, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                 default=(1.0, 1.0, 1.0, 0.0), update=_changed)
    stop2: FloatProperty(name="Stop 2", default=0.25, min=0.0, max=1.0, update=_changed)
    stop3: FloatProperty(name="Stop 3", default=0.5, min=0.0, max=1.0, update=_changed)
    stop4: FloatProperty(name="Stop 4", default=0.75, min=0.0, max=1.0, update=_changed)
    mask: _enum(MASK3D, name="Mask", default="BLEND")
    blending: FloatProperty(name="Blend", default=1.0, min=0.0, max=1.0, subtype="FACTOR", update=_changed)
    channel: _enum(CHANNEL, name="Channel", default="ALPHA")
    ambient: FloatProperty(name="Ambient", default=0.75, soft_min=-1.0, soft_max=1.0, update=_changed)
    diffuse: FloatProperty(name="Diffuse", default=1.0, soft_min=-1.0, soft_max=1.0, update=_changed)
    specular: FloatProperty(name="Specular", default=0.0, soft_min=-1.0, soft_max=1.0, update=_changed)
    reflection: FloatProperty(name="Reflectivity", default=0.0, soft_min=-1.0, soft_max=1.0, update=_changed)
    transparency: FloatProperty(name="Transparency", default=0.0, soft_min=-1.0, soft_max=1.0, update=_changed)
    roughness: FloatProperty(name="Bump", default=0.0, soft_min=-10.0, soft_max=10.0, update=_changed)
    scaling: FloatVectorProperty(name="Scale", size=3, default=(1.0, 1.0, 1.0), update=_changed,
                                 description="Softimage axes: Y is Blender's Z")
    rotation: FloatVectorProperty(name="Rotation", size=3, subtype="EULER", default=(0.0, 0.0, 0.0), update=_changed,
                                  description="Softimage axes: Y is Blender's Z")
    translation: FloatVectorProperty(name="Translation", size=3, default=(0.0, 0.0, 0.0), update=_changed,
                                     description="Softimage axes: Y is Blender's Z")

    def draw_buttons(self, context, layout):
        self.draw_faults(layout)
        layout.prop(self, "kind", text="")
        col = layout.column(align=True)
        for k in range(1, 6):
            row = col.row(align=True)
            row.prop(self, "colour%d" % k, text="")
            if 2 <= k <= 4:
                row.prop(self, "stop%d" % k, text="")

    def draw_buttons_ext(self, context, layout):
        self.draw_buttons(context, layout)
        draw_texture3d_details(self, layout)


def draw_texture3d_details(node, layout):
    col = layout.column(align=True)
    for p in ("spacing", "taper", "strength", "iterations", "power"):
        col.prop(node, p)
    layout.prop(node, "mask")
    layout.prop(node, "blending")
    layout.prop(node, "channel")
    col = layout.column(align=True)
    col.label(text="Drives")
    for p in ("ambient", "diffuse", "specular", "reflection", "transparency", "roughness"):
        col.prop(node, p)
    for p in ("scaling", "rotation", "translation"):
        layout.prop(node, p)


class SI3DWorldNode(_SINode, bpy.types.Node):
    """The Softimage scene's ambience, background, depth fade and layer fog"""
    bl_idname = "SI3DWorldNode"
    bl_label = "Softimage World"
    bl_icon = "WORLD"
    bl_width_default = 240
    OUTPUTS = (("World", SHADER),)

    fit_ambience: BoolProperty(name="Fit ambience", default=False, update=_changed,
                               description="Ambience fitted to light the scene as Cycles does with the background colour "
                               "as the world light (what the engine does without this node); off: the ambience as set")
    ambience: FloatVectorProperty(name="Ambience", size=3, subtype="COLOR_GAMMA", min=0.0, soft_max=1.0,
                                  default=(0.3, 0.3, 0.3), update=_changed,
                                  description="The scene's ambient light (each material's ambient colour times this)")
    background: FloatVectorProperty(name="Background", size=3, subtype="COLOR", min=0.0, soft_max=1.0,
                                    default=(0.0, 0.0, 0.0), update=_changed,
                                    description="The colour where no surface is hit (scene linear, like Blender's "
                                    "world colour)")
    fade: BoolProperty(name="Depth fade", default=False, update=_changed,
                       description="Surfaces fade to the fade colour with their distance from the camera")
    fade_start: FloatProperty(name="Fade start", default=20.0, soft_min=0.0, update=_changed, subtype="DISTANCE",
                              description="Distance from the camera where the fade begins (may be below 0: the scene "
                              "is already partly faded at the camera)")
    fade_end: FloatProperty(name="Fade end", default=100.0, min=0.0, update=_changed, subtype="DISTANCE")
    fade_colour: FloatVectorProperty(name="Fade colour", size=3, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                     default=(0.0, 0.0, 0.0), update=_changed)
    fade_transparency: FloatProperty(name="Fade transparency", default=0.0, min=0.0, max=1.0, subtype="FACTOR",
                                     update=_changed, description="0: fully faded at the end distance")
    fog: BoolProperty(name="Layer fog", default=False, update=_changed,
                      description="A layer of fog between two heights (Blender Z)")
    fog_start: FloatProperty(name="Fog start", default=20.0, min=0.0, update=_changed, subtype="DISTANCE",
                             description="Distance from the camera where the fog begins")
    fog_end: FloatProperty(name="Fog end", default=10000.0, min=0.0, update=_changed, subtype="DISTANCE")
    fog_colour: FloatVectorProperty(name="Fog colour", size=3, subtype="COLOR_GAMMA", min=0.0, max=1.0,
                                    default=(0.7, 0.7, 0.7), update=_changed)
    fog_density: FloatProperty(name="Fog density", default=0.2, min=0.0, update=_changed)
    fog_base: FloatProperty(name="Fog base", default=-1.0, update=_changed, subtype="DISTANCE",
                            description="Height of the fog's bottom (Blender Z)")
    fog_thickness: FloatProperty(name="Fog thickness", default=5.0, min=0.0, update=_changed, subtype="DISTANCE")

    def draw_buttons(self, context, layout):
        self.draw_faults(layout)
        layout.prop(self, "fit_ambience")
        if not self.fit_ambience:
            layout.prop(self, "ambience")
        layout.prop(self, "background")
        layout.prop(self, "fade")
        layout.prop(self, "fog")

    def draw_buttons_ext(self, context, layout):
        self.draw_faults(layout)
        draw_world_details(self, layout)


def draw_world_details(node, layout):
    layout.prop(node, "fit_ambience")
    if not node.fit_ambience:
        layout.prop(node, "ambience")
    layout.prop(node, "background")
    layout.prop(node, "fade")
    if node.fade:
        col = layout.column(align=True)
        for p in ("fade_start", "fade_end", "fade_colour", "fade_transparency"):
            col.prop(node, p)
    layout.prop(node, "fog")
    if node.fog:
        col = layout.column(align=True)
        for p in ("fog_base", "fog_thickness", "fog_density", "fog_colour", "fog_start", "fog_end"):
            col.prop(node, p)


SOCKETS = (SI3DColorSocket, SI3DFactorSocket, SI3DValueSocket, SI3DTextureSocket, SI3DTexture3DSocket)
NODES = (SI3DMaterialNode, SI3DTextureNode, SI3DTexture3DNode, SI3DWorldNode)
NODE_IDS = tuple(c.bl_idname for c in NODES)


def register():
    for c in SOCKETS + NODES:
        bpy.utils.register_class(c)
    from . import node_menu, material_ops, material_panels
    node_menu.register()
    material_ops.register()
    material_panels.register()


def unregister():
    from . import node_menu, material_ops, material_panels
    material_panels.unregister()
    material_ops.unregister()
    node_menu.unregister()
    for c in reversed(SOCKETS + NODES):
        bpy.utils.unregister_class(c)
