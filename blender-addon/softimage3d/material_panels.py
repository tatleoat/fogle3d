# SPDX-License-Identifier: GPL-3.0-or-later
"""Material and World tab panels for the Softimage engine (unit nodes1, our own code).  With a Softimage Material on the
output the panel mirrors the node (shading, colours, values, each texture); without one it shows what the Principled
path reads and the buttons that make Softimage nodes.  Blender's material slot and world selector panels are shown for
this engine too."""
import bpy

from . import material_nodes as MN
from . import nodes as N

ENGINE = "FOGLE3D"
BUILTIN = ("EEVEE_MATERIAL_PT_context_material", "WORLD_PT_context_world")
PRINCIPLED_READ = ("Base Color", "Roughness", "Metallic", "Alpha", "IOR", "Transmission Weight", "Emission Color",
                   "Emission Strength")


class _Panel:
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    COMPAT_ENGINES = {ENGINE}


def _socket_row(layout, node, name):
    s = node.inputs.get(name)
    if s is None:
        return
    if s.is_linked:
        src = MN._from(s)
        layout.label(text="%s: linked (%s), value not read" % (name, src.name if src else "?"), icon="ERROR")
    else:
        layout.prop(s, "value", text=name)


def draw_material_node(layout, node):
    MN.draw_faults(layout, MN.faults(node))
    layout.use_property_split = True
    layout.prop(node, "shading")
    col = layout.column()
    for name in ("Ambient", "Diffuse", "Specular", "Specular Decay", "Reflectivity", "Transparency", "Refraction Index"):
        _socket_row(col, node, name)
    layout.prop(node, "blur")
    if node.blur:
        layout.prop(node, "blur_width")
        layout.prop(node, "blur_decay")
    two, t3 = MN.textures_of(node)
    for k, t in enumerate(two):
        header, body = layout.panel("si3d_tex_%d" % k, default_closed=False)
        header.label(text="%s: %s" % (t.name, t.image.name if t.image else "no image"), icon="TEXTURE")
        if body is not None:
            MN.draw_faults(body, MN.faults(t))
            body.template_ID(t, "image", new="image.new", open="image.open")
            for p in ("scope", "method", "mask", "blend"):
                body.prop(t, p)
            N.draw_texture_details(t, body)
    if t3 is not None:
        header, body = layout.panel("si3d_tex3d", default_closed=False)
        header.label(text="%s: %s" % (t3.name, t3.kind.title()), icon="TEXTURE")
        if body is not None:
            MN.draw_faults(body, MN.faults(t3))
            body.prop(t3, "kind")
            col = body.column(align=True)
            for k in range(1, 6):
                row = col.row(align=True)
                row.prop(t3, "colour%d" % k, text="Colour %d" % k)
                if 2 <= k <= 4:
                    row.prop(t3, "stop%d" % k, text="")
            N.draw_texture3d_details(t3, body)
    row = layout.row(align=True)
    row.operator("si3d.add_texture", text="Add texture", icon="TEXTURE").kind = "2D"
    row.operator("si3d.add_texture", text="Add 3D texture", icon="TEXTURE").kind = "3D"


def draw_principled(layout, mat):
    tree = mat.node_tree if MN.uses_nodes(mat) else None
    p = next((n for n in tree.nodes if n.bl_idname == "ShaderNodeBsdfPrincipled"), None) if tree else None
    box = layout.box()
    if p is None:
        box.label(text="No Softimage Material and no Principled BSDF:", icon="INFO")
        box.label(text="the material's own colour, roughness and metallic are rendered")
        box.prop(mat, "diffuse_color")
    else:
        box.label(text="Rendered from the Principled BSDF (%s)" % p.name, icon="INFO")
        col = box.column()
        col.use_property_split = True
        for name in PRINCIPLED_READ:
            s = p.inputs.get(name)
            if s is None:
                continue
            if s.is_linked and name != "Base Color":
                col.label(text="%s: linked, its value is used" % name)
            elif not s.is_linked:
                col.prop(s, "default_value", text=name)
        img = MN.principled_image(mat)
        box.label(text="Texture: %s" % (img.name if img else "none"), icon="TEXTURE")
    col = layout.column(align=True)
    col.operator("si3d.convert_shader", icon="NODETREE")
    col.operator("si3d.new_material", icon="MATERIAL")
    col.operator("si3d.settings_to_nodes", icon="FILE_REFRESH")


class SI3D_PT_material(_Panel, bpy.types.Panel):
    bl_label = "Softimage Material"
    bl_context = "material"

    @classmethod
    def poll(cls, context):
        return context.engine in cls.COMPAT_ENGINES and getattr(context, "material", None) is not None

    def draw(self, context):
        mat = context.material
        node = MN.material_node(mat)
        if node is not None:
            draw_material_node(self.layout, node)
        else:
            draw_principled(self.layout, mat)


class SI3D_PT_world(_Panel, bpy.types.Panel):
    bl_label = "Softimage World"
    bl_context = "world"

    @classmethod
    def poll(cls, context):
        return context.engine in cls.COMPAT_ENGINES and getattr(context, "world", None) is not None

    def draw(self, context):
        world = context.world
        node = MN.world_node(world)
        layout = self.layout
        if node is not None:
            MN.draw_faults(layout, MN.faults(node))
            layout.use_property_split = True
            N.draw_world_details(node, layout)
            return
        box = layout.box()
        box.label(text="The Background's colour times strength lights the scene", icon="INFO")
        box.label(text="(ambience fitted) and fills the background")
        tree = world.node_tree if MN.uses_nodes(world) else None
        bg = next((n for n in tree.nodes if n.bl_idname == "ShaderNodeBackground"), None) if tree else None
        if bg is not None:
            col = box.column()
            col.use_property_split = True
            for name in ("Color", "Strength"):
                s = bg.inputs[name]
                if s.is_linked:
                    col.label(text="%s: linked, not read" % name)
                else:
                    col.prop(s, "default_value", text=name)
        layout.operator("si3d.new_world", icon="WORLD")


CLASSES = (SI3D_PT_material, SI3D_PT_world)


def _builtin():
    return [getattr(bpy.types, n) for n in BUILTIN if hasattr(getattr(bpy.types, n, None), "COMPAT_ENGINES")]


def register():
    for c in CLASSES:
        bpy.utils.register_class(c)
    for p in _builtin():
        p.COMPAT_ENGINES.add(ENGINE)


def unregister():
    for p in _builtin():
        p.COMPAT_ENGINES.discard(ENGINE)
    for c in reversed(CLASSES):
        bpy.utils.unregister_class(c)
