# SPDX-License-Identifier: GPL-3.0-or-later
"""Add > Softimage in the shader editor (unit nodes1, our own code): the Softimage nodes, added with Blender's own
add-node operator so the node follows the pointer.  Shown only while the engine is Fogle3D and the editor holds a
shader tree; the material nodes in a material tree, the World node in a world tree."""
import bpy

ENGINE = "FOGLE3D"
MATERIAL_ROWS = (("SI3DMaterialNode", "Material"), ("SI3DTextureNode", "Texture"), ("SI3DTexture3DNode", "3D Texture"))
WORLD_ROWS = (("SI3DWorldNode", "World"),)


def shown(context):
    space = getattr(context, "space_data", None)
    return context.engine == ENGINE and getattr(space, "tree_type", "ShaderNodeTree") == "ShaderNodeTree"


def rows_for(context):
    space = getattr(context, "space_data", None)
    if getattr(space, "shader_type", "OBJECT") == "WORLD":
        return WORLD_ROWS
    return MATERIAL_ROWS


class NODE_MT_si3d_add(bpy.types.Menu):
    bl_idname = "NODE_MT_si3d_add"
    bl_label = "Softimage"

    def draw(self, context):
        layout = self.layout
        for idname, text in rows_for(context):
            op = layout.operator("node.add_node", text=text)
            op.type = idname
            op.use_transform = True
        layout.separator()
        if rows_for(context) is WORLD_ROWS:
            layout.operator("si3d.new_world", icon="WORLD")
        else:
            layout.operator("si3d.new_material", icon="MATERIAL")
            layout.operator("si3d.convert_shader", icon="NODETREE")


def node_add_menu(self, context):
    if shown(context):
        self.layout.menu(NODE_MT_si3d_add.bl_idname)


def register():
    bpy.utils.register_class(NODE_MT_si3d_add)
    bpy.types.NODE_MT_add.append(node_add_menu)


def unregister():
    bpy.types.NODE_MT_add.remove(node_add_menu)
    bpy.utils.unregister_class(NODE_MT_si3d_add)
