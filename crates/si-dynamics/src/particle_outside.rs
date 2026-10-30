// SPDX-License-Identifier: MIT
//! The routines of other source files the particle and wave code (`particle_*.c`, `wave_*.c`) calls, as typed calls on
//! [`si_mem::Outside`]. Nothing here reproduces a function of the original. Answered 0 until the file that holds the
//! routine is ported.

use si_mem::outside_calls;

outside_calls! {
    /// What the particle and wave code calls outside its files.
    pub trait ParticleCalls {
        /// `status.c`: shows a message (text, level) in the status line.
        fn si_status_display_message(text: u32, level: i32) = "SI_statusDisplayMessage", 0x083cca94;
        /// `obj_global.c`: a copy of the mesh; its address.
        fn si_mesh_copy(mesh: u32) -> u32 = "SI_meshCopy", 0x084404e0;
        /// `edtools.c`: the animation element of a model's shape (model, out word); its address or 0.
        fn si_ed_get_elem_from_shape(model: u32, out: u32) -> u32 = "SI_edGetElemFromShape", 0x080fd4dc;
        /// `edtools.c`: the animation element of a model (model, out word).
        fn si_ed_get_elem_from_model(model: u32, out: u32) -> u32 = "SI_edGetElemFromModel", 0x080fcf20;
        /// `edtools.c`: the animation element of a material (material, out word).
        fn si_ed_get_elem_from_material(material: u32, out: u32) -> u32 = "SI_edGetElemFromMaterial", 0x080fd288;
        /// `edanim.c`: the function curve of an element by kind and channel (element, kind, channel).
        fn si_ed_find_fcv(elem: u32, kind: i32, channel: i32) -> u32 = "SI_edFindFcv", 0x080edb20;
        /// `edanim.c`: the first function curve of an element whose channel is in a range (element, kind, lo, hi).
        fn si_ed_find_range_fcv(elem: u32, kind: i32, lo: i32, hi: i32) -> u32 = "SI_edFindRangeFcv", 0x080edbc8;
        /// `edanim.c`: the next one of the same search.
        fn si_ed_find_range_next_fcv(elem: u32, kind: i32, lo: i32, hi: i32) -> u32 = "SI_edFindRangeNextFcv", 0x080edc74;
        /// `fkey_fct.c`: a copy of a function-curve key (0x24 bytes); its address.
        fn si_fkey_copy(key: u32) -> u32 = "SI_fkeyCopy", 0x0820bc9c;
        /// `fcurve.c`: adds a key to a function curve (curve, key).
        fn si_fcurve_add_key(curve: u32, key: u32) = "SI_fcurveAddKey", 0x081ff400;
        /// `mesh_b.c`: a new mesh record; its address.
        fn si_mesh_allocate() -> u32 = "SI_meshAllocate", 0x08458c20;
        /// `mesh_b.c`: an array of `n` vertices (0x14 bytes each); its address.
        fn si_vertex_allocate(n: i32) -> u32 = "SI_vertexAllocate", 0x08459d48;
        /// `mesh_b.c`: an array of `n` polygons (0x18 bytes each); its address.
        fn si_polygon_allocate(n: i32) -> u32 = "SI_polygonAllocate", 0x08459e20;
        /// `meshEditFct.c`: allocates `n` corner records (0x24 bytes) into a pointer slot (model, slot, flag, n).
        fn si_nodes_allocate(model: u32, slot: u32, flag: i32, n: u32) = "SI_nodesAllocate", 0x08453b9c;
        /// `meshEditFct.c`: copies the texture coordinates of a corner record (model, model, destination, source).
        fn si_nodes_copy_uv(m1: u32, m2: u32, dst: u32, src: u32) = "SI_nodesCopyUV", 0x08453d90;
        /// `mesh_b.c`: recomputes a mesh's derived data.
        fn si_mesh_update(mesh: u32) = "SI_meshUpdate", 0x08458d94;
        /// `mat_dial.c`: a copy of a material; its address.
        fn si_material_copy(material: u32) -> u32 = "SI_materialCopy", 0x08284888;
        /// `graflist.c`: adds a datum to a graph list (list, data, kind, flag).
        fn si_graflist_add_data(list: u32, data: u32, kind: i32, flag: i32) -> u32 = "SI_graflistAddData", 0x0832d240;
        /// `edtools.c`: gives a model a material (model, material).
        fn si_ed_model_set_material(model: u32, material: u32) = "SI_edModelSetMaterial", 0x08100c34;
        /// `shd_fct.c`: copies the shaders of a material (destination, source).
        fn si_copy_material_shaders(dst: u32, src: u32) = "SI_copyMaterialShaders", 0x082a6808;
        /// `edtools.c`: disposes a 2D texture.
        fn si_ed_dispose_texture2d(t: u32) = "SI_edDisposeTexture2D", 0x0810253c;
        /// `edtools.c`: releases a model's global 3D texture (model, texture).
        fn si_ed_model_relax_gtexture3d(model: u32, t: u32) = "SI_edModelRelaxGTexture3D", 0x08100ef0;
        /// `t3d_dial.c`: whether a 3D texture is still used (a short).
        fn si_texture3d_still_used(t: u32) -> i32 = "SI_texture3DStillUsed", 0x082b56c0;
        /// `edtools.c`: disposes a 3D texture.
        fn si_ed_dispose_texture3d(t: u32) = "SI_edDisposeTexture3D", 0x081028d0;
        /// `t2d_dial.c`: a copy of a 2D texture; its address.
        fn si_txt2d_copy(t: u32) -> u32 = "SI_txt2dCopy", 0x082adfbc;
        /// `t2d_dial.c`: sets the wrap of a 2D texture (model, texture).
        fn si_txt2d_set_wrap(model: u32, t: u32) = "SI_txt2dSetWrap", 0x082ad9f8;
        /// `edtools.c`: gives a model a global 2D texture (model, texture).
        fn si_ed_model_set_gtexture2d(model: u32, t: u32) = "SI_edModelSetGTexture2D", 0x08101130;
        /// `edtools.c`: gives a material a local 2D texture (material, texture).
        fn si_ed_model_set_ltexture2d(material: u32, t: u32) = "SI_edModelSetLTexture2D", 0x0810126c;
        /// `shd_fct.c`: copies the shader of a 2D texture (destination, source shader).
        fn si_copy_t2d_shader(dst: u32, src: u32) = "SI_copyT2DShader", 0x082a6864;
        /// `t3d_dial.c`: a copy of a 3D texture; its address.
        fn si_txt3d_copy(t: u32) -> u32 = "SI_txt3dCopy", 0x082b4008;
        /// `edtools.c`: gives a model a global 3D texture (model, texture).
        fn si_ed_model_set_gtexture3d(model: u32, t: u32) = "SI_edModelSetGTexture3D", 0x08100e4c;
        /// `edtools.c`: gives a material a local 3D texture (material, texture).
        fn si_ed_model_set_ltexture3d(material: u32, t: u32) = "SI_edModelSetLTexture3D", 0x08100f88;
        /// `shd_fct.c`: copies the shader of a 3D texture (destination, source shader).
        fn si_copy_t3d_shader(dst: u32, src: u32) = "SI_copyT3DShader", 0x082a68cc;
        /// `graflist.c`: the next graph node of a list (list, slot holding the current node).
        fn si_next_grafnode(list: u32, slot: u32) = "SI_nextGrafnode", 0x083353d4;
        /// `edtools.c`: disposes the animation element of a wave.
        fn si_ed_dispose_wave(wave: u32) = "SI_edDisposeWave", 0x08103454;
        /// `model.c`: disposes a model through a pointer slot.
        fn si_model_dispose_node(slot: u32) = "SI_modelDisposeNode", 0x08461a54;
        /// `wave_util.c`: attaches a model to a wave (wave, model, short, flag).
        fn si_wave_add_mdl(wave: u32, model: u32, kind: i32, flag: i32) = "SI_waveAddMdl", 0x084c9810;
        /// `wave_init.c`: makes the icon model of a wave.
        fn si_wave_create_icon_mdl(wave: u32) -> u32 = "si_waveCreateIconMdl", 0x084c61b8;
        /// `status.c`: the mouse-button help of the status line (three texts).
        fn si_status_display_mouse(a: u32, b: u32, c: u32) = "SI_statusDisplayMouse", 0x083cc650;
        /// `matrix.c`: extracts scale, rotation and translation from a matrix (matrix, three vectors).
        fn si_matrix_extract_transformations(mat: u32, v1: u32, v2: u32, v3: u32) = "SI_matrixExtractTransformations", 0x08518144;
        /// `model.c`: sets a model's scale, rotation and translation (model, three vectors).
        fn si_model_set_state(model: u32, v1: u32, v2: u32, v3: u32) = "SI_modelSetState", 0x084638a8;
        /// `dictionary.c`: starts a topological sort of an element dictionary (dictionary, flag).
        fn si_rel_topo_sort_init(dict: u32, flag: i32) = "SI_relTopoSortInit", 0x0813ffbc;
        /// `dictionary.c`: sorts an element dictionary into an update list (relation dictionary, dictionary, list, a, b).
        fn si_elemdict_topo_sort(rd: u32, dict: u32, list: u32, a: i32, b: i32) = "SI_elemdictTopoSort", 0x08140828;
        /// `particle_util.c`: makes the animation of a hierarchy of particles (status, argument).
        fn particle_create_hrc_animation(status: u32, arg: u32) = "particleCreateHrcAnimation", 0x084c0e24;
        /// `particle_util.c`: makes the animation of a shape of particles (status, argument).
        fn particle_create_shape_animation(status: u32, arg: u32) = "particleCreateShapeAnimation", 0x084c1de8;
        /// `event.c`: the next input event (its address): +0 the kind, +6 a short code, +0xc the graph port.
        fn si_get_event() -> u32 = "SI_getEvent", 0x08394058;
        /// `graflist.c`: the graph node under the cursor in a graph port (its address or 0).
        fn si_gl_select(port: u32) -> u32 = "SI_gLSelect", 0x0832ef0c;
        /// `object.c`: flashes a graph node (node, times).
        fn si_flash_object(node: u32, times: i32) = "SI_flashObject", 0x0823c9d8;
        /// `edtools.c`: the animation element of a wave (wave, out word); its address or 0.
        fn si_ed_get_elem_from_wave(wave: u32, out: u32) -> u32 = "SI_edGetElemFromWave", 0x080fd448;
        /// `edanim.c`: the next function curve of the search `SI_edFindFcv` started.
        fn si_ed_find_next_fcv(elem: u32, kind: i32, channel: i32) -> u32 = "SI_edFindNextFcv", 0x080edb74;
        /// `edanim.c`: disposes a function curve (owner, word, kind, lo, hi).
        fn si_ed_fcv_dispose(owner: u32, w: u32, kind: i32, lo: i32, hi: i32) = "SI_edFcvDispose", 0x080ee2f0;
        /// `edanim.c`: adds a function curve to an element (curve, element, word, index).
        fn si_ed_fcv_add(fcv: u32, elem: u32, w: u32, index: i32) = "SI_edFcvAdd", 0x080ee650;
        /// `fcurve.c`: a new function curve (channel, owner); its address.
        fn si_fcurve_allocate(channel: i32, owner: u32) -> u32 = "SI_fcurveAllocate", 0x0820a810;
        /// `fkey_fct.c`: a new key at a frame (frame, value, value, flag, mode); its address.
        fn si_fkey_frame_create(frame: i32, v: f32, w: f32, flag: i32, mode: i32) -> u32 = "SI_fkeyFrameCreate", 0x0820b1b0;
        /// `fcurve.c`: finds or makes one curve (element, index, channel, out slot).
        fn si_fcurve_check_add(elem: u32, index: i32, channel: i32, out: u32) = "SI_fcurveCheckAdd", 0x082194b0;
        /// `fcurve.c`: as above for one curve of the first kind.
        fn si_fcurve_check_add1(elem: u32, index: i32, channel: i32, out: u32) = "SI_fcurveCheckAdd1", 0x08219560;
        /// `fcurve.c`: finds or makes three curves (element, index, channel, three out slots).
        fn si_fcurve_check_add3(elem: u32, index: i32, channel: i32, o1: u32, o2: u32, o3: u32) = "SI_fcurveCheckAdd3", 0x082195e8;
        /// `path.c`: a new path (kind); its address.
        fn si_path_new(kind: u32) -> u32 = "SI_pathNew", 0x0821d750;
        /// `path.c`: adds a key to a path from a position (spline, curve, vector, short).
        fn si_path_add_key_pct(spline: u32, fcv: u32, v: u32, kind: i32) = "SI_pathAddKeyPct", 0x0821d7cc;
        /// `model.c`: gives a model a path (model, path).
        fn si_model_set_path(model: u32, path: u32) = "SI_modelSetPath", 0x0821f174;
        /// `path.c`: sets a path element of an owner (owner, kind, path).
        fn si_path_element_set(owner: u32, kind: i32, path: u32) = "SI_pathElementSet", 0x082200c0;
        /// `dialog.c`: a dialogue with a message and a button (text, title, text, button).
        fn si_dialog_request(a: u32, b: u32, c: u32, d: u32) = "SI_dialogRequest", 0x08141c00;
        /// `model.c`: a model's scale, rotation and translation (model, three vectors).
        fn si_model_state(model: u32, scale: u32, rot: u32, trans: u32) = "SI_modelState", 0x08463774;
        /// `edtools.c`: removes a wave from a model's links (model, wave).
        fn si_ed_model_unlink_wave(model: u32, wave: u32) = "SI_edModelUnlinkWave", 0x081014f8;
        /// `matrix3tools.c`: the product of two 3 x 3 matrices (a, b, result).
        fn si_matrix_multiply_sub3(a: u32, b: u32, out: u32) = "SI_matrixMultiplySub3", 0x0851650c;
        /// `algebra.c`: the rotation angles of a matrix (matrix, three floats).
        fn si_matrix_extract_xyz_rotation(mat: u32, out: u32) = "SI_matrixExtractXyzRotation", 0x085172b0;
        /// `model.c`: recomputes a model's matrix.
        fn si_model_update_matrix(model: u32) = "SI_modelUpdateMatrix", 0x0846559c;
        /// `modeldep.c`: updates the dependencies of a model (model, flag).
        fn si_model_update_dependency(model: u32, flag: i32) = "SI_modelUpdateDependency", 0x0846f200;
        /// `obj_global.c`: copies the vertices of a mesh back to its original (mesh, mesh).
        fn si_mesh_transfer_vertices(a: u32, b: u32) = "SI_meshTransferVertices", 0x08446d00;
        /// `patch.c`: as above for a patch.
        fn si_patch_transfer_points(a: u32, b: u32) = "SI_patchTransferPoints", 0x084810cc;
        /// `face.c`: as above for a face.
        fn si_face_transfer_points(a: u32, b: u32) = "SI_faceTransferPoints", 0x0843672c;
        /// `spline.c`: as above for a spline.
        fn si_spline_transfer_points(a: u32, b: u32) = "SI_splineTransferPoints", 0x08490664;
        /// `nurbs_srf.c`: as above for a NURBS surface.
        fn si_nurbs_srf_transfer_points_knots(a: u32, b: u32) = "SI_nurbsSrfTransferPointsKnots", 0x084ea07c;
        /// `edtools.c`: tells the editor about a new wave (0, wave).
        fn si_ed_add_wave(zero: i32, wave: u32) = "SI_edAddWave", 0x081033b4;
        /// `spline.c`: the number of segments of a spline.
        fn si_spline_nb_segment(spline: u32) -> i32 = "SI_splineNbSegment", 0x0848c8dc;
        /// `spline.c`: the point of a spline at a parameter (spline, parameter, three floats out).
        fn si_spline_value(spline: u32, t: f32, out: u32) = "SI_splineValue", 0x0848b1e4;
        /// `refresh.c`: redraws everything (flag).
        fn si_refresh_all(flag: i32) = "SI_refreshAll", 0x08325dc8;
        /// `model.c`: rebuilds a model's definition after its profile changed.
        fn si_model_readjust_definition(model: u32) = "SI_modelReadjustDefinition", 0x0846e340;
        /// `select.c`: puts the selection back (flag, flag).
        fn si_back_selected(a: i32, b: i32) = "SI_backSelected", 0x083260a0;
        /// `savekey.c`: sets a key on a model's parameter (model, group, relid, kind, check, value, keyed, record).
        fn si_savekey_model(model: u32, group: i32, relid: i32, kind: i32, check: i32, value: f32, keyed: i32, rec: u32) = "SI_savekeyModel", 0x08219fe0;
        /// `graflist.c`: the graph node of an object in a list (list, object, kind); its address or 0.
        fn si_find_gl_data(list: u32, model: u32, kind: i32) -> u32 = "SI_findGlData", 0x0832ee68;
        /// `model.c`: brings a model up to a time (model, seconds, graph node).
        fn si_model_update(model: u32, time: f32, node: u32) = "SI_modelUpdate", 0x082222dc;
        /// `model.c`: a copy of a model (model, flag); its address.
        fn si_model_copy_node(model: u32, flag: i32) -> u32 = "SI_modelCopyNode", 0x0846234c;
        /// `mod_global.c`: makes a graph node the son of another (father, son, flag).
        fn si_model_create_parent(parent: u32, child: u32, flag: i32) = "SI_modelCreateParent", 0x0817b500;
        /// `edtools.c`: whether an element has group relations of a kind (element, kind, flag); a short.
        fn si_ed_check_group_relations(elem: u32, kind: i32, flag: i32) -> i16 = "SI_edCheckGroupRelations", 0x08106a50;
        /// `savekey.c`: keys all parameters of a graph node.
        fn si_savekey_object_all(node: u32) = "SI_savekeyObjectAll", 0x08219e04;
        /// `savekey.c`: keys the material parameters of a model.
        fn si_savekey_material(model: u32) = "SI_savekeyMaterial", 0x0821a968;
        /// `mat_dial.c`: copies the values of a material to another (from, to, flag).
        fn si_material_transfer(src: u32, dst: u32, flag: i32) = "SI_materialTransfer", 0x08284504;
        /// `mesh_b.c`: recomputes the polygon normals (mesh, polygon or -1).
        fn si_mesh_computes_polygon_normal(mesh: u32, poly: i32) = "SI_meshComputesPolygonNormal", 0x084599f4;
        /// `mesh_b.c`: recomputes the hard edges (mesh, polygon or -1).
        fn si_mesh_compute_hard_edge(mesh: u32, poly: i32) = "SI_meshComputeHardEdge", 0x0845a6e4;
        /// `mesh_b.c`: recomputes the vertex normals (mesh, polygon or -1).
        fn si_mesh_computes_vertex_normal(mesh: u32, poly: i32) = "SI_meshComputesVertexNormal", 0x08459e8c;
        /// `nurbs_crv.c`: as above for a NURBS curve.
        fn si_nurbs_crv_transfer_points_knots(a: u32, b: u32) = "SI_nurbsCrvTransferPointsKnots", 0x084ea00c;
        /// `wave_dialog.c`: the wave dialog (wave, flag: 1 for a new wave); its result (above 0: accepted).
        fn si_dialog_wave(wave: u32, flag: i32) -> i32 = "si_dialogWave", 0x084ca1f0;
        /// `draw.c`: redraws one system (port, graph list, system number).
        fn si_refresh_system(port: u32, list: u32, system: i32) = "SI_refreshSystem", 0x08323d44;
        /// `counter.c`: refreshes the counter.
        fn si_refresh_counter() = "SI_refreshCounter", 0x080d1d20;
        /// `fcurve.c`: scales a function curve's keys about a time (curve, factor, origin).
        fn si_fcurve_scale_origin(fcv: u32, factor: f32, origin: f32) = "SI_fcurveScaleOrigin", 0x082010b0;
        /// `fcurve.c`: moves a function curve's keys in time (curve, offset).
        fn si_fcurve_offset(fcv: u32, offset: f32) = "SI_fcurveOffset", 0x08200e84;
        /// `toposort.c`: whether the element dictionary has a cycle (relation dictionary, dictionary); a short.
        fn si_elemdict_check_cycle(rd: u32, dict: u32) -> i16 = "SI_elemdictCheckCycle", 0x08140d80;
        /// `particle_dial.c`: writes the particle set-up record as text (stream, status record); ported in `si-ascii` on a typed record.
        fn particle_write_parameters(stream: u32, status: u32) = "particleWriteParameters", 0x084bf540;
        /// `particle_dial.c`: the particle dialog (status record); below 0 when cancelled. Not ported (user interface).
        fn si_dialog_particle(status: u32) -> i32 = "si_dialogParticle", 0x084be3c0;
        /// `particle_util.c`: finds the meshes the set-up uses (status record); below 0 when it cannot.
        fn si_particle_get_meshes(status: u32) -> i32 = "si_particleGetMeshes", 0x084c0c84;
        /// `particle_util.c`: makes the animation of one repetition (status record, repetition); its model or 0.
        fn si_particle_create_animation(status: u32, rep: i32) -> u32 = "si_particleCreateAnimation", 0x084c0d94;
        /// `particle_mn.c`: keys the hide curve of a particle model (status record, model, repetition).
        fn particle_handle_hide_keys(status: u32, model: u32, rep: i32) = "particleHandleHideKeys", 0x084c0ac4;
        /// `metaball.c`: the parent system of a metaball (its address or 0).
        fn si_metaball_get_parent_system(ball: u32) -> u32 = "SI_metaballGetParentSystem", 0x0845cd40;
        /// libc: sleeps for that many microseconds.
        fn usleep(us: u32) -> u32 = "usleep", 0x080c37b8;
    }
}
