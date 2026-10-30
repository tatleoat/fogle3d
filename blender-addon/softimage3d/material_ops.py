# SPDX-License-Identifier: GPL-3.0-or-later
"""Operators that give materials and worlds Softimage nodes (unit nodes1, our own code): New Softimage material, Convert
Blender shader, Softimage settings to nodes, Softimage world, Add Softimage texture.  The conversions keep the render:
the nodes hold what the exporter derives from the Principled material or the Background now (material_nodes.py)."""
import bpy

from . import material_nodes as MN


class _Op:
    bl_options = {"REGISTER", "UNDO"}


def _material_of(context, create):
    mat = getattr(context, "material", None)
    if mat is None and create:
        mat = bpy.data.materials.new("Softimage material")
        obj = context.object
        if obj is not None and hasattr(obj.data, "materials"):
            if obj.material_slots and obj.material_slots[obj.active_material_index].material is None:
                obj.material_slots[obj.active_material_index].material = mat
            else:
                obj.data.materials.append(mat)
    return mat


class SI3D_OT_new_material(_Op, bpy.types.Operator):
    """Put a Softimage Material on the active material's output (a new material if there is none); the nodes that fed
    the output stay in the tree, unlinked"""
    bl_idname = "si3d.new_material"
    bl_label = "New Softimage material"

    def execute(self, context):
        mat = _material_of(context, True)
        if MN.material_node(mat) is not None:
            self.report({"INFO"}, "%s already has a Softimage Material" % mat.name)
            return {"CANCELLED"}
        MN.new_material_node(mat)
        MN.refresh(mat.node_tree)
        return {"FINISHED"}


class SI3D_OT_convert_shader(_Op, bpy.types.Operator):
    """Make Softimage nodes from the active material's Principled BSDF and first image texture, with the values the
    engine derives from them now (the render does not change); the Principled stays in the tree, unlinked"""
    bl_idname = "si3d.convert_shader"
    bl_label = "Convert Blender shader"

    @classmethod
    def poll(cls, context):
        return getattr(context, "material", None) is not None

    def execute(self, context):
        node, notes = MN.convert_principled(context.material)
        for n in notes:
            self.report({"WARNING"}, n)
        return {"FINISHED"} if node is not None else {"CANCELLED"}


class SI3D_OT_settings_to_nodes(_Op, bpy.types.Operator):
    """Give every material without Softimage nodes, and every world, Softimage nodes holding what the engine reads from
    them now (the render does not change)"""
    bl_idname = "si3d.settings_to_nodes"
    bl_label = "Softimage settings to nodes"

    def execute(self, context):
        m = w = 0
        for mat in list(bpy.data.materials):
            if getattr(mat, "is_grease_pencil", False) or MN.material_node(mat) is not None:
                continue
            node, notes = MN.convert_principled(mat)
            for n in notes:
                self.report({"WARNING"}, n)
            m += node is not None
        for world in list(bpy.data.worlds):
            w += MN.convert_world(world) is not None
        self.report({"INFO"}, "%d materials and %d worlds now use Softimage nodes" % (m, w))
        return {"FINISHED"}


class SI3D_OT_new_world(_Op, bpy.types.Operator):
    """Put a Softimage World on the scene's world output (a new world if there is none), rendering as the world does
    now: its background colour, the ambience fitted from it"""
    bl_idname = "si3d.new_world"
    bl_label = "Softimage world"

    def execute(self, context):
        scene = context.scene
        if scene.world is None:
            scene.world = bpy.data.worlds.new("Softimage world")
        if MN.convert_world(scene.world) is None:
            self.report({"INFO"}, "%s already has a Softimage World" % scene.world.name)
            return {"CANCELLED"}
        return {"FINISHED"}


class SI3D_OT_add_texture(_Op, bpy.types.Operator):
    """Add a Softimage Texture on the next free texture input of the active material's Softimage Material"""
    bl_idname = "si3d.add_texture"
    bl_label = "Add Softimage texture"
    kind: bpy.props.EnumProperty(items=[("2D", "Texture", ""), ("3D", "3D Texture", "")], default="2D")

    @classmethod
    def poll(cls, context):
        return MN.material_node(getattr(context, "material", None)) is not None

    def execute(self, context):
        node = MN.material_node(context.material)
        tree = node.id_data
        if self.kind == "3D":
            s = node.inputs["3D Texture"]
            if s.is_linked:
                self.report({"INFO"}, "The material has a 3D texture")
                return {"CANCELLED"}
            t = tree.nodes.new(MN.TEXTURE3D)
            t.location = (node.location.x - 320.0, node.location.y - 1300.0)
            tree.links.new(t.outputs[0], s)
        else:
            if all(node.inputs[n].is_linked for n in MN.N.TEXTURE_INPUTS):
                self.report({"INFO"}, "All %d texture inputs are taken" % MN.MAX_LOCAL)
                return {"CANCELLED"}
            MN.new_texture_node(node, None)
        MN.refresh(tree)
        return {"FINISHED"}


CLASSES = (SI3D_OT_new_material, SI3D_OT_convert_shader, SI3D_OT_settings_to_nodes, SI3D_OT_new_world,
           SI3D_OT_add_texture)


def register():
    for c in CLASSES:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(CLASSES):
        bpy.utils.unregister_class(c)
