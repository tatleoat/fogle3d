// SPDX-License-Identifier: MIT
//! The routines of other source files `collisions.c` calls, as typed calls on
//! [`si_mem::Outside`]. Nothing here reproduces a function of the original.

use si_mem::outside_calls;

outside_calls! {
    /// What `collisions.c` calls outside its file.
    pub trait DynaCalls {
        /// `algebra.c`: the rotation matrix of three angles.
        fn si_matrix_xyz_rotation(angles: u32, out: u32) = "SI_matrixXyzRotation", 0x0851882c;
        /// `model.c`: the bounding box and radius of a model from its geometry.
        fn si_model_compute_bounding(model: u32) = "SI_modelComputeBounding", 0x08467e70;
        /// `dyn_constr.c`: the position of a constraint (model, the body it is taken for, a flag, out vector).
        fn si_constraint_position_get(model: u32, body: u32, flag: i32, out: u32) = "si_constraintPositionGet", 0x08495544;
        /// `dyn_constr.c`: the orientation of a constraint (model, the body, two flags, an axis code, out vector).
        fn si_constraint_orientation_get(model: u32, body: u32, a: i32, b: i32, axis: i32, out: u32) = "si_constraintOrientationGet", 0x08495624;
        /// `obj_global.c`: the centroid of a polygon of a mesh (mesh, polygon, out vector).
        fn si_mesh_polygon_centroid(mesh: u32, poly: i32, out: u32) = "SI_meshPolygonCentroid", 0x084467b8;
        /// `meshUtil.c`: the signed volume and the surface of a polygon (mesh, polygon, scale flag, out volume, out surface).
        fn si_mesh_polygon_volume_surface(mesh: u32, poly: i32, scale: i32, volume: u32, surface: u32) = "SI_meshPolygonVolumeSurface", 0x08444df8;
        /// `settings.c`: the relative to absolute matrices of a dynamics chain.
        fn si_model_dyna_rel2abs_matrix_update(model: u32) = "si_modelDynaRel2AbsMatrixUpdate", 0x0849fe94;
        /// `settings.c`: the absolute to relative matrices of a dynamics chain.
        fn si_model_dyna_abs2rel_matrix_update(model: u32) = "si_modelDynaAbs2RelMatrixUpdate", 0x0849ff5c;
        /// `settings.c`: the joints of a dynamics chain after a step.
        fn si_model_dyna_joints_update(model: u32) = "si_modelDynaJointsUpdate", 0x084a0080;
        /// `dynamic.c`: the slow band inbound pass (model, step, flag).
        fn dyna_slowband_inbound(model: u32, dt: f32, flag: u16) = "dynaSlowbandInbound", 0x0849739c;
        /// `dynamic.c`: the fast band inbound pass.
        fn dyna_fastband_inbound(model: u32) = "dynaFastbandInbound", 0x084981f8;
        /// `dynamic.c`: the fast band outbound pass.
        fn dyna_fastband_outbound(model: u32) = "dynaFastbandOutbound", 0x084988f4;
        /// `ui_misc.c`: the activity indicator.
        fn si_show_activity() = "SI_showActivity", 0x083d1a60;
        /// `tablet.c`: polls the tablet button.
        fn si_tablet_get_button_up_event() = "SI_tabletGetButtonUpEvent", 0x083cec2c;
        /// `gl:event`: whether a button is down (the button code).
        fn getbutton(button: i32) -> i32 = "getbutton", 0x0860e684;
        /// `playback.c`: saves the raw function curve keys of a simulation (model, time).
        fn si_simulation_raw_fcrv_save_keys(model: u32, time: f32) = "SI_simulationRawFcrvSaveKeys", 0x0849e030;
        /// `fcurve.c`: whether a function curve has raw (simulated) keys; a short in ax.
        fn si_fcurve_check_for_raw_keys(fcv: u32) -> u32 = "SI_fcurveCheckForRawKeys", 0x08202e54;
        /// `fcurve.c`: removes the raw keys of a function curve.
        fn si_fcurve_remove_raw_keys(fcv: u32) = "SI_fcurveRemoveRawKeys", 0x08202ea4;
        /// `edanim.c`: the three curves of one kind of an element (element, three curve pointers out, kind, channel).
        fn si_get_fcv_triplet(elem: u32, a: u32, b: u32, c: u32, kind: i32, channel: i32) = "SI_getFcvTriplet", 0x0822efcc;
        /// `time.c`: the frame of a time in seconds.
        fn si_sec_to_frame(sec: f32) -> i32 = "SI_secToFrame", 0x08201568;
        /// `scene.c`: brings the scene to a time.
        fn si_scene_update(sec: f32) = "SI_sceneUpdate", 0x082224b0;
        /// `edtools.c`: lets go of the dynamics of a model (model, root of the chain).
        fn si_ed_element_relax_dynamics(model: u32, root: u32) = "SI_edElementRelaxDynamics", 0x08101788;
        /// `fileswap.c`: writes `count` items of `size` bytes, byte-swapped, to a stream (data, size, count, file).
        fn si_byteswap_fwrite(data: u32, size: i32, count: i32, file: u32) = "SI_byteswap_fwrite", 0x083f9c40;
        /// `fileswap.c`: writes `count` vectors of three floats, byte-swapped (vector, size, count, file).
        fn si_vector_fwrite(vec: u32, size: i32, count: i32, file: u32) = "SI_Vector_fwrite", 0x083f9df8;
        /// `edtools.c`: the animation element of a model (model, out word); its address or 0.
        fn si_ed_get_elem_from_model(model: u32, out: u32) -> u32 = "SI_edGetElemFromModel", 0x080fcf20;
        /// `edanim.c`: the first function curve of the element (element, kind, channel).
        fn si_ed_find_fcv(elem: u32, kind: i32, channel: i32) -> u32 = "SI_edFindFcv", 0x080edb20;
        /// `edanim.c`: the next function curve of the search `SI_edFindFcv` started.
        fn si_ed_find_next_fcv(elem: u32, kind: i32, channel: i32) -> u32 = "SI_edFindNextFcv", 0x080edb74;
        /// `model.c`: the scale, rotation and translation of a model (model, three vectors).
        fn si_model_state(model: u32, scale: u32, rot: u32, trans: u32) = "SI_modelState", 0x08463774;
        /// `model.c`: sets the state of a model from three vectors.
        fn si_model_half_set_state(model: u32, scale: u32, rot: u32, trans: u32) = "SI_modelHalfSetState", 0x084639cc;
        /// `fcurve.c`: the value of a function curve at a time (in the x87 register).
        fn si_fcurve_value(fcv: u32, time: f32, kind: i32) -> si_mem::Ret = "SI_fcurveValue", 0x082002dc;
        /// `matrix3tools.c`: copies the 3 x 3 part of a matrix (destination, source).
        fn si_matrix3_put(dst: u32, src: u32) = "SI_matrix3Put", 0x0851b0ac;
        /// `algebra.c`: the XYZ rotation angles of a matrix (matrix, three floats out).
        fn si_matrix_extract_xyz_rotation(matrix: u32, out: u32) = "SI_matrixExtractXyzRotation", 0x085172b0;
        /// `model.c`: updates the state of the sons of a model (first son, model, flag).
        fn si_model_update_son_state(son: u32, model: u32, flag: i32) = "SI_modelUpdateSonState", 0x084662f8;
        /// `algebra.c`: copies a 4 x 4 matrix (source, destination).
        fn si_matrix_copy(src: u32, dst: u32) = "SI_matrixCopy", 0x08516224;
        /// `model_b.c`: a new model record (name null); its address.
        fn si_model_allocate(name: u32) -> u32 = "SI_modelAllocate", 0x08477cf4;
        /// `primitive.c`: a sphere mesh of the radius with this many bands and meridians.
        fn si_mesh_sphere(radius: f32, bands: i32, meridians: i32) -> u32 = "SI_meshSphere", 0x08486df8;
        /// `primitive.c`: a grid mesh of `nu` x `nv` cells of the given sizes.
        fn si_mesh_grid(nu: i32, nv: i32, du: f32, dv: f32) -> u32 = "SI_meshGrid", 0x08488eec;
        /// `primitive.c`: a cube mesh between two corners (x1, y1, z1, x0, y0, z0).
        fn si_mesh_cube(x1: f32, y1: f32, z1: f32, x0: f32, y0: f32, z0: f32) -> u32 = "SI_meshCube", 0x0848a7c0;
        /// `model.c`: attaches a geometry of the kind to the model.
        fn si_model_add_definition(model: u32, kind: i32, geometry: u32) -> i32 = "SI_modelAddDefinition", 0x08467038;
        /// `matrix3tools.c`: copies a 3 x 3 matrix of floats (source, destination).
        fn si_matrix3_copy(src: u32, dst: u32) = "SI_matrix3Copy", 0x0851b088;
        /// `algebra.c`: the inverse of a 4 x 4 matrix (source, destination).
        fn si_matrix_inverse(src: u32, dst: u32) = "SI_matrixInverse", 0x0851672c;
        /// `algebra.c`: the product of two 4 x 4 matrices (a, b, result).
        fn si_matrix_multiply(a: u32, b: u32, out: u32) = "SI_matrixMultiply", 0x08516244;
        /// `algebra.c`: the transposed inverse of a matrix (source, destination).
        fn si_matrix_transpose_inverse(src: u32, dst: u32) = "SI_matrixTransposeInverse", 0x08518a3c;
        /// `meshUtil.c`: a mesh built from the geometry of the model; its address, or 0.
        fn si_model_convert_to_mesh(model: u32) -> u32 = "SI_modelConvertToMesh", 0x084587d4;
        /// `meshUtil.c`: whether all faces of the mesh are planar (a 16-bit result).
        fn si_mesh_planar(mesh: u32) -> i32 = "SI_meshPlanar", 0x08458848;
        /// `obj_global.c`: splits the faces of the mesh into triangles (mesh, flag).
        fn si_mesh_triangulate(mesh: u32, flag: i32) = "SI_meshTriangulate", 0x08443144;
        /// `boolean.c`: the boolean of two meshes (setup, a, b, result a, result b).
        fn si_compute_boolean(setup: u32, a: u32, b: u32, out_a: u32, out_b: u32) = "SI_computeBoolean", 0x08315348;
        /// `meshUtil.c`: removes the faceted flag of a mesh.
        fn si_remove_faceted(mesh: u32) = "SI_removeFaceted", 0x08458770;
        /// `obj_global.c`: frees the mesh the pointer points to and clears the pointer.
        fn si_mesh_dispose(pp: u32) = "SI_meshDispose", 0x084404a0;
        /// `model.c`: the global scaling, angles and position of a model (model, scaling out, angles out, position out).
        fn si_model_global_state(model: u32, scal: u32, rot: u32, trans: u32) = "SI_modelGlobalState", 0x084637dc;
        /// `meshUtil.c`: the volume and surface of a mesh (mesh, out volume, out surface).
        fn si_mesh_computes_volume_surface(mesh: u32, volume: u32, surface: u32) = "SI_meshComputesVolumeSurface", 0x08445054;
        /// `patch.c`: a mesh from a patch of the model; its address or 0.
        fn si_patch_convert_to_mesh(model: u32, patch: u32) -> u32 = "SI_patchConvertToMesh", 0x0847b69c;
        /// `nurbs.c`: a mesh from a surface of the model; its address or 0.
        fn si_nurbs_convert_to_mesh(model: u32, surface: u32) -> u32 = "SI_nurbsConvertToMesh", 0x084df320;
        /// `model.c`: frees the model the pointer points to (pointer, flag).
        fn si_model_dispose(pp: u32, flag: i32) = "SI_modelDispose", 0x08461920;
        /// `obj_global.c`: the centroid of a mesh (mesh, out vector).
        fn si_mesh_centroid(mesh: u32, out: u32) = "SI_meshCentroid", 0x0844490c;
        /// `dialog:system`: a warning with three lines and a question; the answer in 16 bits.
        fn si_system_warning(a: u32, b: u32, c: u32) -> u32 = "SI_systemWarning", 0x081422d8;
        /// `motionUtils.c`: sets the function curve table (pointer).
        fn si_fcv_table_set(table: u32) = "SI_fcvTableSet", 0x0822b3c4;
        /// `attdict.c`: sets the function curve flag of an attribute (dictionary, attribute, flag).
        fn si_attdict_set_fcurve_flag(dict: u32, attr: u32, flag: i32) = "SI_attdictSetFcurveFlag", 0x0811ff18;
        /// `motionReset.c`: resets the animation of an element (dictionary, element, class, location, flag).
        fn si_elemdict_reset_elem_anim(dict: u32, elem: u32, cid: u32, loc: u32, flag: i32) = "SI_elemdictResetElemAnim", 0x08220f08;
        /// `edtools.c`: selects a dynamics constraint of a model (model, constraint model, kind).
        fn si_ed_select_dyna_constraint(model: u32, constraint: u32, kind: i32) = "SI_edSelectDynaConstraint", 0x0810164c;
        /// `edtools.c`: a new unused model name from a base string.
        fn si_ed_model_get_new_name(base: u32) -> u32 = "SI_edModelGetNewName", 0x080fe894;
        /// `model.c`: renames a model.
        fn si_model_change_name(model: u32, name: u32) = "SI_modelChangeName", 0x0846db14;
        /// `edtools.c`: tells the editor that the constraint object of the kind is no longer used by the model.
        fn si_ed_relax_dyna_constraint(model: u32, obj: u32, kind: i32) = "SI_edRelaxDynaConstraint", 0x081016c8;
        /// `dyn_constr.c`: the model standing for a constraint kind (kind, flag); its address or 0.
        fn si_dyna_constraint_get(kind: i32, flag: i32) -> u32 = "SI_dynaConstraintGet", 0x08495e5c;
        /// `dyn_constr.c`: a constraint record (kind, priority, object); its address or 0.
        fn si_conslist_constr_allocate(kind: i32, prio: i32, obj: u32) -> u32 = "SI_conslistConstrAllocate", 0x084958d0;
        /// `dyn_constr.c`: adds a constraint to the list of a model (model, constraint, flag).
        fn si_conslist_add_constraint(model: u32, c: u32, flag: i32) = "SI_conslistAddConstraint", 0x08495ad8;
        /// `dyn_constr.c`: disposes the constraints of a model (model, all, kind).
        fn si_conslist_dispose(model: u32, all: i32, kind: i32) = "SI_conslistDispose", 0x08495d90;
        /// `playback.c`: disposes the simulation of a model (model, flag).
        fn si_dyna_simulation_dispose(model: u32, flag: i32) = "SI_dynaSimulationDispose", 0x0849d760;
        /// `playback.c`: disposes the transition curves of a model.
        fn si_dyna_transition_crv_dispose(model: u32) = "SI_dynaTransitionCrvDispose", 0x0849d8f8;
        /// `elemdict.c`: the first element related to `elem` (dictionary, element, forward, list, relation, chapter, out chapter, out index).
        fn si_elemdict_find_related_elem(dict: u32, elem: u32, forward: i32, group: i32, relid: i32, chap: u32, out_chap: u32, out_index: u32) -> u32 = "SI_elemdictFindRelatedElem", 0x0813508c;
        /// `mot_plot.c`: plots the trajectory of a model's element (element, model, the element's location word, time).
        fn si_model_anim_plot_trajectory(elem: u32, model: u32, loc: u32, time: f32) = "SI_modelAnimPlotTrajectory", 0x0820d9e0;
        /// `fkey.c`: a new function-curve key (time, value, slope, slope type, interpolation).
        fn si_fkey_create(time: u32, value: u32, slope: u32, slope_type: i32, interp: i32) -> u32 = "SI_fkeyCreate", 0x0820b248;
        /// `fcurve.c`: inserts a key into a curve (curve, key, then the start index, the warn and set-time words).
        fn si_fcurve_add_key_after(curve: u32, key: u32, start: i32, warn: i32, set_time: i32) -> i32 = "SI_fcurveAddKeyAfter", 0x081ff588;
    }
}
