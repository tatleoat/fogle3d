// SPDX-License-Identifier: MIT
//! What the image form of the expression code calls outside what is on the image
//! (finding 0013, section 12).
//!
//! Nothing here reproduces a function of the original. Each line names a routine of the
//! Linux binary by its name and address, with the arguments the original pushes; the call
//! goes to whatever [`si_mem::Outside`] the caller hands in. Routines that are on the image
//! in another crate (`si-dict`'s dictionaries, `si-motion`'s curve look-ups and curve
//! evaluation, `si-view`'s graf lists, `si-math`) are called directly and are not listed.

use si_mem::{outside_calls, CStr32, Opaque, Ptr32, Ret};

outside_calls! {
    /// The callees of `exp_fct.c`, `exp_parsing.c`, `express_io.c` and of the parts of
    /// `edanim.c` and `edparams.c` the evaluator reaches, that are not on the image.
    pub trait ExprCalls {
        /// `status.c`: a message in the status line (kind 2: an error).
        fn status_display_message(text: CStr32, kind: i16) = "SI_statusDisplayMessage", 0x083cca94;
        /// The C library's generator.
        fn rand() -> i32 = "rand", 0x080c5048;
        fn srand(seed: u32) = "srand", 0x080c4a38;
        /// The C library's `rint` on a double (two words); the answer in the register.
        fn rint(x: f64) -> Ret = "rint", 0x080c4188;
        /// `chn_*.c`: a channel's value at a time, and setting it.
        fn chn_get_value(drv: Ptr32<Opaque>, dof: i32, time: f32) -> Ret = "SI_chnGetValue", 0x0855d5d0;
        fn chn_set_value(drv: Ptr32<Opaque>, dof: i32, value: f32) = "SI_chnSetValue", 0x0855d6a8;
        /// `custom_*.c`: a custom effect's parameter by name, and its curve (out).
        fn cus_fetch_custom_value(model: Ptr32<Opaque>, name: CStr32, fcv: Ptr32<u32>) -> u32 = "SI_cusFetchCustomValue", 0x08530e08;
        /// `edparams.c`: a custom parameter's value, read and set.
        fn custom_get_param(val: u32, fcv: u32, out: u32) -> i16 = "si_customGetParam", 0x080f0450;
        fn custom_set_param(val: u32, fcv: u32, value: u32) = "si_customSetParam", 0x080f06a8;
        /// `edparams.c`: the parameter readers of the other chapters.
        fn cam_get_param(fcv_type: i32, obj: u32, fcv: u32, out: u32) -> i16 = "SI_camGetParam", 0x080f1b64;
        fn wave_get_param(fcv_type: i32, obj: u32, fcv: u32, out: u32) -> i16 = "SI_waveGetParam", 0x080f1930;
        fn material_get_param(fcv_type: i32, obj: u32, fcv: u32, out: u32) -> i16 = "SI_materialGetParam", 0x080f2b7c;
        fn text2d_get_param(fcv_type: i32, obj: u32, fcv: u32, out: u32) -> i16 = "SI_text2dGetParam", 0x080f2e30;
        fn text3d_get_param(fcv_type: i32, obj: u32, fcv: u32, out: u32) -> i16 = "SI_text3dGetParam", 0x080f37c4;
        fn light_get_param(fcv_type: i32, obj: u32, fcv: u32, out: u32) -> i16 = "SI_lightGetParam", 0x080f20a8;
        fn scene_get_param(fcv_type: i32, ctrl: u32, fcv: u32, out: u32) -> i16 = "SI_sceneGetParam", 0x080fac08;
        /// … and the setters.
        fn cam_set_param(fcv_type: i32, obj: u32, fcv: u32, value: u32) = "SI_camSetParam", 0x080f4c90;
        fn wave_set_param(fcv_type: i32, obj: u32, fcv: u32, value: u32) = "SI_waveSetParam", 0x080f4b70;
        fn material_set_param(fcv_type: i32, obj: u32, value: u32) = "SI_materialSetParam", 0x080f5964;
        /// `mat_dial.c`: marks a material changed (the port is `elem::material_set_dirty`; listed for the check of
        /// `SI_materialSetParam`, whose vectors hold it as a recorded stand-in).
        fn material_set_dirty(mat: u32, flags: i32) = "SI_materialSetDirty", 0x08281f9c;
        fn text2d_set_param(fcv_type: i32, obj: u32, value: u32) = "SI_text2dSetParam", 0x080f5be8;
        fn text3d_set_param(fcv_type: i32, obj: u32, value: u32) = "SI_text3dSetParam", 0x080f6124;
        fn light_set_param(fcv_type: i32, obj: u32, fcv: u32, value: u32) = "SI_lightSetParam", 0x080f51b8;
        /// `SI_modelGetParam` and `SI_modelSetParam` themselves, for the fcurve types the
        /// image form does not have (everything but scaling, rotation and explicit
        /// translation: [`super::elem`]).
        fn model_get_param(fcv_type: i32, model: u32, fcv: u32, out: u32, coord: i32) -> i16 = "SI_modelGetParam", 0x080f0c3c;
        fn model_set_param(fcv_type: i32, model: u32, fcv: u32, value: u32, coord: i32) -> i32 = "SI_modelSetParam", 0x080f3d1c;
        /// The epilogue of `SI_modelSetParam` and the readers behind its types: fitting (cluster) matrices, the
        /// volume-cube preview primitive, the whole-model update, the dynamics geometry, the quick-stretch record and
        /// the attribute dictionary's access mode. The targets are ported in `si-deform`, `si-render`, `si-model`,
        /// `si-dynamics` (which depends on this crate), `si-kine` and `si-dict`; the hooks stay until routed.
        fn fitting_update_matrix(fit: u32) = "SI_fittingUpdateMatrix", 0x0843a264;
        fn pfitting_update_matrix(fit: u32) = "SI_pfittingUpdateMatrix", 0x0843a484;
        fn vc_create_prim(kind: i32, matrix: u32, out: u32) = "VCCreatePrim", 0x08575dc0;
        fn vc_set_prim_param(prim: i32, which: i32, p: u32) = "VCSetPrimParam", 0x08575dec;
        fn vc_get_prim_param(prim: i32, which: i32, p: u32) = "VCGetPrimParam", 0x08575e0c;
        fn vc_delete_prim(prim: i32) = "VCDeletePrim", 0x08575fa8;
        fn model_update_all(model: u32, a: i16, b: i16) = "SI_modelUpdateAll", 0x0846ef2c;
        fn dyna_geometry_update(model: u32) = "SI_dynaGeometryUpdate", 0x084a0920;
        fn quick_stretch_get_elem(model: u32, index: u32, data: u32) -> i32 = "SI_QuickStretchGetElem", 0x085a2428;
        fn attdict_get_grp_index_from_fcurve(atd: u32, fcv_type: i32, out: u32) -> i16 = "SI_attdictGetGrpIndexFromFcurve", 0x0811fd8c;
        fn attdict_attribute_get_access_mode(atd: u32, index: u32, out: u32) -> i16 = "SI_attdictAttributeGetAccessMode", 0x0811efe0;
        /// `model.c`: setting a node's global scaling, rotation, translation (a vector
        /// passed by address; mode 1, no geometry).
        fn model_set_global_scaling(model: u32, v: u32, mode: i32, geom: i32) = "SI_modelSetGlobalScaling", 0x084642fc;
        fn model_set_global_rotation(model: u32, v: u32, mode: i32, geom: i32) = "SI_modelSetGlobalRotation", 0x08464224;
        fn model_set_global_translation(model: u32, v: u32, mode: i32, geom: i32) = "SI_modelSetGlobalTranslation", 0x0846412c;
        /// `motionUpdate.c`: places an object on its path from the curve's last value (`si-motion` has it; it needs a host).
        fn set_path_translation(rel: u32, chap: u32, obj: u32, curve: u32) = "SI_setPathTranslation", 0x082254e8;
        /// `sys_camera.c`: the camera's format changed (not ported).
        fn change_camera_aspect(cam: u32, mode: i32) = "SI_changeCameraAspect", 0x080d6cc0;
        /// `mat_validation.c`: a value checked and, when valid, stored; 0 when taken.
        fn valid_txt3d_fct1(t: u32, v: f32) -> i32 = "SI_validTxt3dFct1", 0x0828cea8;
        fn valid_txt3d_fct2(t: u32, v: f32) -> i32 = "SI_validTxt3dFct2", 0x0828cefc;
        fn valid_txt3d_fct3(t: u32, v: f32) -> i32 = "SI_validTxt3dFct3", 0x0828cf44;
        fn valid_txt3d_nb_iter(t: u32, v: i32) -> i32 = "SI_validTxt3dNbIter", 0x0828ce7c;
        /// `t3d_dial.c`: a 3-D texture marked changed (not ported); `t3d_fct.c`: its turbulence table updated (`si-render`).
        fn texture3d_set_dirty(t: u32) = "SI_texture3DSetDirty", 0x082b3990;
        fn upd_txt3d_turb(t: u32) = "SI_updTxt3dTurb", 0x082b8d20;
        /// `edtools.c`: a light's graf-list kind (`si-scene`'s; it depends on this crate).
        fn light_get_graf_type(light: u32) -> i32 = "SI_lightGetGrafType", 0x082800fc;
        /// `mi_output.c`: whether the output goes to mental ray (`si-mental`'s; not a dependency of this crate).
        fn get_mi2_output() -> i16 = "SI_getmi2Output", 0x08580cfc;
        /// `mat_validation.c`: a light's values checked after they were stored (0 when valid), and `SI_validLight…` the values
        /// checked and, when valid, stored.
        fn light_validate_area_light(light: u32) -> i32 = "SI_lightValidateAreaLight", 0x0828d0a0;
        fn light_validate_area_type(light: u32) -> i32 = "SI_lightValidateAreaType", 0x0828d0ec;
        fn light_validate_area_rotation(light: u32) -> i32 = "SI_lightValidateAreaRotation", 0x0828d158;
        fn light_validate_area_scaling(light: u32) -> i32 = "SI_lightValidateAreaScaling", 0x0828d1e0;
        fn light_validate_selective(light: u32) -> i32 = "SI_lightValidateSelective", 0x0828d000;
        fn light_sun_validate_date_time(light: u32) -> i32 = "SI_lightSunValidateDateTime", 0x0828d268;
        fn valid_light_shadow_type(light: u32, v: i32) -> i32 = "SI_validLightShadowType", 0x0828cf90;
        fn valid_light_map_res(light: u32, v: i32) -> i32 = "SI_validLightMapRes", 0x0828cfc0;
        fn valid_light_filter_step(light: u32, v: i32) -> i32 = "SI_validLightFilterStep", 0x0828cfe0;
        /// `t2d_*.c`, `mat_validation.c`: the 2-D texture's helpers (`si-hrc`'s scale and effects update are on the image there;
        /// the validators are `matval1`'s). The validators answer 0 when the value was taken.
        fn txt2d_set_scale(t: u32) = "SI_txt2dSetScale", 0x082ada24;
        fn upd_txt2d_effects(t: u32) = "SI_updTxt2dEffects", 0x082b289c;
        fn texture2d_set_dirty(t: u32) = "SI_texture2DSetDirty", 0x082a8fc0;
        fn set_file_name(map: u32, frame: i32) = "SI_setFileName", 0x082b0200;
        fn valid_txt2d_window_scal(t: u32, dst: u32, v: f32) -> i32 = "SI_validTxt2dWindowScal", 0x0828cdf4;
        fn valid_txt2d_window_trans(t: u32, dst: u32, v: f32) -> i32 = "SI_validTxt2dWindowTrans", 0x0828ce28;
        fn valid_txt2d_method(t: u32, v: i32) -> i32 = "SI_validTxt2dMethod", 0x0828cab8;
        fn valid_txt2d_crop_u(t: u32) -> i32 = "SI_validTxt2dCropU", 0x0828cbec;
        fn valid_txt2d_crop_v(t: u32, v: i32, flag: i32) -> i32 = "SI_validTxt2dCropV", 0x0828ccdc;
        fn valid_txt2d_u_repeat(t: u32, v: i32) -> i32 = "SI_validTxt2dURepeat", 0x0828cb8c;
        fn valid_txt2d_v_repeat(t: u32, v: i32) -> i32 = "SI_validTxt2dVRepeat", 0x0828cbbc;
        /// `pictures`: a picture file's dimensions (`si-picture`; it is not a dependency of this crate). `w` and `h` are the
        /// addresses of the shorts to fill (the original passes null for the one it does not want). 0 when read.
        fn picture_get_dimensions(name: u32, w: u32, h: u32) -> i32 = "SI_pictureGetDimensions", 0x085e30c4;
        /// `invkin.c`: a joint's preferred angle (the vector passed by value).
        fn inv_kin_reset_pref_angle(model: u32, axis: i32, v: [u32; 3]) = "SI_invKinResetPrefAngle", 0x084adcec;
        /// `exp_parsing.c`: a text parsed into a tree (the image form of the parser is not
        /// ported; value form in `crate::parse`). `out` receives the expression.
        fn gen_cus_expr_parsing_tree(text: CStr32, vars: u32, nvars: i32, out: Ptr32<u32>, validate: i16, arg_list: u32) = "SI_genCusExprParsingTree", 0x0824b9ac;
        /// `exp_parsing.c`: the pieces of an expression as text (value form in `crate::format`).
        /// Answers the right-hand side; `target`, `nvars`, `vars` are out-values (or null).
        fn get_complete_expr_format(expr: u32, target: Ptr32<u32>, nvars: Ptr32<i32>, vars: Ptr32<u32>, arg_type: i32, var_type: i32) -> u32 = "SI_getCompleteExprFormat", 0x0824db00;
        /// `exp_parsing.c`: whether a text is a valid expression (value form in `crate::parse`);
        /// answers the position of the first error, or null.
        fn valid_custom_express(text: CStr32, vars: u32, nvars: i32, errvar: Ptr32<i32>, a: i32, b: i32) -> u32 = "SI_validCustomExpress", 0x0824b7c8;
        /// `exp_fct.c`: a relation's data freed (`si-scene` has it; it depends on this crate).
        fn expr_rel_data_dispose(data: u32) = "SI_exprRelDataDispose", 0x08249c74;
        /// `exp_parsing.c`: a cluster parameter's cluster (not ported).
        fn exp_find_valid_cluster(kl: u32, name: u32, fcv: i32, index: i32, out: u32, err: u32) = "si_expFindValidCluster", 0x0824e194;
        /// `express_io.c`: an orphan expression's element given up (not ported).
        fn ed_dispose_expression(index: i32) -> i16 = "SI_edDisposeExpression", 0x0825591c;
        /// `exp_parsing.c`: an expression's trees freed (value form only so far; the image form
        /// is not ported).
        fn express_data_dispose(expr: u32) = "SI_expressDataDispose", 0x0824e5b0;
        /// `db*.c`: a new name record, and its file name made (`si-scene` has both on the
        /// image; it depends on this crate, so they are called by address).
        fn db_list_node_allocate() -> u32 = "SI_dbListNodeAllocate", 0x0812250c;
        fn db_format_full_name(node: u32) -> u32 = "SI_dbFormatFullName", 0x081223dc;
        /// `edtools.c`: an element's delete flag set (not ported).
        fn ed_element_set_delete_flag(elem: u32, chap: i32, index: i32, flag: i16, p1: u32, p2: u32, last: i16) = "SI_edElementSetDeleteFlag", 0x080fd910;
        /// `edtools.c`: the first and the next son of an element in the current dictionary
        /// (`si-scene`'s, which depends on this crate).
        fn ed_find_son(elem: u32) -> u32 = "SI_edFindSon", 0x080fcec0;
        fn ed_find_next_son(elem: u32) -> u32 = "SI_edFindNextSon", 0x080fcef0;
        /// `metaball.c`: a metaball touched (a ball, or a system) (not ported).
        fn metaball_touch(ball: u32, system: u32) = "SI_metaballTouch", 0x0845abf8;
        /// The colour index of a wire colour (`si-elemio` has it in value form, not on the
        /// image).
        fn map_wire_to_index_colour(colour: i16, which: i16) -> i16 = "SI_mapWireToIndexColour", 0x083f92bc;
        /// `dbvsfcv.c`: the relation list (group) of a curve id (`si-scene` has it; it depends on this crate).
        fn fcv_get_rel_grp_from_type(id: i32) -> i32 = "SI_fcvGetRelGrpFromType", 0x0810f624;
        /// `fcv_dial.c`: whether a curve id is one an element of a graf class has (not ported).
        fn is_valid_fcv(graf: i32, fcv: i32, elem: i32) -> i16 = "SI_isValidFcv", 0x0820a654;
        /// `sprd_fct.c`: a spreadsheet query's default clause, and a clause's text (not ported).
        fn sprd_create_default_clause() -> u32 = "SI_sprdCreateDefaultClause", 0x085a3b3c;
        fn sprd_gen_clause_str(count: i32, clauses: u32, index: i32) = "SI_sprdGenClauseStr", 0x085a3cb8;
        /// What `printf` prints (the evaluator's two messages); the text is passed by
        /// [`si_mem::Outside::print`], this records the call.
        fn printf(format: CStr32) -> i32 = "printf", 0x080c48b8;
    }
}

/// Whoever stands for the routines of [`ExprCalls`]: answers zeros and counts by name,
/// for runs where nothing else is wired.
#[derive(Default)]
pub struct Counting {
    pub calls: std::collections::BTreeMap<&'static str, usize>,
    pub printed: Vec<u8>,
}

impl si_mem::Outside for Counting {
    fn call(&mut self, _m: &mut si_mem::Mem, routine: si_mem::Routine, _args: &[u32]) -> Ret {
        *self.calls.entry(routine.name).or_default() += 1;
        Ret::NONE
    }
    fn print(&mut self, _stream: u32, text: &[u8]) {
        self.printed.extend_from_slice(text);
    }
}

/// An outside world that is the C library for what the expression code asks of it (the
/// generator `rand`/`srand`, one state, started as after `srand(1)`; `getenv` answering
/// null; what is printed kept) and answers zeros to the rest, counting by name.
#[derive(Default)]
pub struct Clib {
    pub crand: crate::rand::CRand,
    pub calls: std::collections::BTreeMap<String, usize>,
    pub printed: Vec<u8>,
}

impl si_mem::Outside for Clib {
    fn call(&mut self, _m: &mut si_mem::Mem, routine: si_mem::Routine, args: &[u32]) -> Ret {
        match routine.name {
            "rand" => Ret::int(self.crand.rand()),
            "srand" => {
                self.crand.srand(args.first().copied().unwrap_or(1));
                Ret::NONE
            }
            "getenv" | "printf" => Ret::NONE,
            n => {
                let name = if n.is_empty() { format!("{:#x}", routine.addr) } else { n.to_string() };
                *self.calls.entry(name).or_default() += 1;
                Ret::NONE
            }
        }
    }
    fn print(&mut self, _stream: u32, text: &[u8]) {
        self.printed.extend_from_slice(text);
    }
}

/// `si-dict`'s outside world, handed on to ours: every routine the dictionaries call that
/// is not on the image goes to the same [`si_mem::Outside`].
pub struct DictOutside<'a>(pub &'a mut dyn si_mem::Outside);

impl si_dict::outside::Outside for DictOutside<'_> {
    fn call(&mut self, m: &mut si_mem::Mem, routine: &'static si_dict::outside::Routine, args: &[u32]) -> u32 {
        self.0.call(m, si_mem::Routine::new(routine.name, routine.addr), args).eax
    }
    fn call_addr(&mut self, m: &mut si_mem::Mem, addr: u32, args: &[u32]) -> u32 {
        self.0.call(m, si_mem::Routine::at(addr), args).eax
    }
    fn stdout(&mut self, text: &[u8]) {
        self.0.print(1, text)
    }
    fn stderr(&mut self, text: &[u8]) {
        self.0.print(2, text)
    }
}
