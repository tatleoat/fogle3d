// SPDX-License-Identifier: MIT
//! The parts of the program the three envelope source files call that are not in this
//! crate, beyond the routines [`crate::outside`] already names: the geometry packages'
//! copies and disposals, the model tree's update, and (until the second pass) the routines
//! of the three files themselves that are not ported.
//!
//! Nothing here reproduces a function of the original. The routines are reached through the
//! crate's one trait, [`crate::outside::Outside`], in the calling convention described
//! there. The block below is printed by `re/tools/si-deform/env_outside_table.py --write`
//! with every address taken from the binary's symbol table;
//! `re/tools/si-deform/check_outside.py` holds them against it again.

use crate::outside::Routine;

macro_rules! outside_env {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
        /// Every routine this module names.
        pub const ALL: &[&Routine] = &[ $( &$konst ),* ];
    };
}

outside_env! {
    // BEGIN TABLE (env_outside_table.py --write)
    /// `SI_elemdictGetRelationFromIds (0x08134e9c)`: the relation of a type between an element and the element of a chapter and index.
    SI_ELEMDICT_GET_RELATION_FROM_IDS = "SI_elemdictGetRelationFromIds", 0x08134e9c;
    /// `SI_patchCopy (0x0847a2b8)`: a copy of a patch.
    SI_PATCH_COPY = "SI_patchCopy", 0x0847a2b8;
    /// `SI_meshCopy (0x084404e0)`: a copy of a mesh.
    SI_MESH_COPY = "SI_meshCopy", 0x084404e0;
    /// `SI_splineCopy (0x0848aa74)`: a copy of a spline.
    SI_SPLINE_COPY = "SI_splineCopy", 0x0848aa74;
    /// `SI_nurbsCrvCopy (0x084dbbf4)`: a copy of a NURBS curve.
    SI_NURBS_CRV_COPY = "SI_nurbsCrvCopy", 0x084dbbf4;
    /// `SI_nurbsSrfCopy (0x084dc1a8)`: a copy of a NURBS surface.
    SI_NURBS_SRF_COPY = "SI_nurbsSrfCopy", 0x084dc1a8;
    /// `SI_patchDispose (0x0847a530)`: frees a patch and clears the pointer to it.
    SI_PATCH_DISPOSE = "SI_patchDispose", 0x0847a530;
    /// `SI_meshDispose (0x084404a0)`: frees a mesh and clears the pointer to it.
    SI_MESH_DISPOSE = "SI_meshDispose", 0x084404a0;
    /// `SI_splineDispose (0x0848ab48)`: frees a spline and clears the pointer to it.
    SI_SPLINE_DISPOSE = "SI_splineDispose", 0x0848ab48;
    /// `SI_nurbsCrvDispose (0x084db870)`: frees a NURBS curve and clears the pointer to it.
    SI_NURBS_CRV_DISPOSE = "SI_nurbsCrvDispose", 0x084db870;
    /// `SI_nurbsSrfDispose (0x084dba5c)`: frees a NURBS surface and clears the pointer to it.
    SI_NURBS_SRF_DISPOSE = "SI_nurbsSrfDispose", 0x084dba5c;
    /// `SI_modelUpdateSonState (0x084662f8)`: rebuilds a node's matrices from its father's, and its branch's.
    SI_MODEL_UPDATE_SON_STATE = "SI_modelUpdateSonState", 0x084662f8;
    /// `SI_modelSpecialUpdateSonState (0x084ad480)`: the same for a chain, with the inverse kinematics solved.
    SI_MODEL_SPECIAL_UPDATE_SON_STATE = "SI_modelSpecialUpdateSonState", 0x084ad480;
    /// `SI_modelTouchDefinition (0x0846d530)`: tells the display and the dependants that a node's working definition changed.
    SI_MODEL_TOUCH_DEFINITION = "SI_modelTouchDefinition", 0x0846d530;
    /// `SI_skeChainRoot (0x081d20c0)`: the root of the chain a joint or an effector belongs to.
    SI_SKE_CHAIN_ROOT = "SI_skeChainRoot", 0x081d20c0;
    /// `SI_meshPolygonCentroid (0x084467b8)`: the centroid of a polygon of a mesh, stored through the third argument.
    SI_MESH_POLYGON_CENTROID = "SI_meshPolygonCentroid", 0x084467b8;
    /// `SI_findGlData (0x0832ee68)`: the display node of an object in a graph list.
    SI_FIND_GL_DATA = "SI_findGlData", 0x0832ee68;
    /// `SI_modelGlobalState (0x084637dc)`: a node's global scaling, rotation and translation, stored through the three pointers.
    SI_MODEL_GLOBAL_STATE = "SI_modelGlobalState", 0x084637dc;
    /// `SI_modelSetRotation (0x08463f54)`: gives a node new rotation angles (read through the second argument).
    SI_MODEL_SET_ROTATION = "SI_modelSetRotation", 0x08463f54;
    /// `SI_dialogEnvelopInit (0x081d3740)`: asks the user how a skin's points are to be assigned: the method, or 0 for none.
    SI_DIALOG_ENVELOP_INIT = "SI_dialogEnvelopInit", 0x081d3740;
    /// `SI_dialogRequest2 (0x08141d5c)`: asks the user a yes-or-no question (title, question, answers); 0 for no.
    SI_DIALOG_REQUEST2 = "SI_dialogRequest2", 0x08141d5c;
    /// `SI_elemdictRemoveRelationId (0x08133998)`: removes the relation with a given type id between an element and itself.
    SI_ELEMDICT_REMOVE_RELATION_ID = "SI_elemdictRemoveRelationId", 0x08133998;
    /// `SI_meshOptimizeDisplay (0x0845a7c8)`: rebuilds a mesh's display data.
    SI_MESH_OPTIMIZE_DISPLAY = "SI_meshOptimizeDisplay", 0x0845a7c8;
    /// `SI_modelGetVertexPtr (0x0847402c)`: the address of a point of a definition of a kind, by its number; stores a word through the fourth argument.
    SI_MODEL_GET_VERTEX_PTR = "SI_modelGetVertexPtr", 0x0847402c;
    /// `SI_elemGetNbVertices (0x084769b4)`: how many points an element's geometry has in a definition.
    SI_ELEM_GET_NB_VERTICES = "SI_elemGetNbVertices", 0x084769b4;
    // END TABLE
}
