// SPDX-License-Identifier: MIT
//! Compares the render set-up with the emulated original (findings 0011 and 0023).
//!
//! `rensetup-emucheck <dir>`: `<dir>` holds `rules.txt` and one directory per routine with
//! the vector file `re/tools/si-rensetup/emu_gen.py` wrote. A case carries the canonical
//! form (`si_rensetup::canon`) of a made-up world before the call (`desc`) and after it
//! (`dump`), and the calls the routine made into other packages (`log`). The check builds
//! the world, runs the port and compares both. `EMUCHECK_MUTATE=1` flips a bit of what the
//! port produced: every compared case must then differ.

use si_rensetup::canon::{self, Parsed};
use si_rensetup::edlocal::NewName;
use si_rensetup::instance::{self, InstState};
use si_rensetup::outside::{Call, Recorder};
use si_rensetup::world::{Env, Preview};
use si_rensetup::{edlocal, propagate, rdict, rmain, udf, update, util};
use si_types::Ptr32;
use si_scene::dict::{Dict, ElemId, Program};
use si_x87::vectors::{check_dir_with, print_report, Case, Check, Tally, Verdict};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[path = "emucheck_parts/sys.rs"]
mod emucheck_sys;
#[path = "emucheck_parts/boxes.rs"]
mod emucheck_boxes;
#[path = "emucheck_parts/native.rs"]
mod emucheck_native;

static RULES: OnceLock<Vec<Vec<i32>>> = OnceLock::new();
const NIL: u32 = 0xffff_ffff;

fn mutate() -> bool {
    std::env::var_os("EMUCHECK_MUTATE").is_some()
}

/// The program globals as the generator set them: the scene's chapter ids, the relation
/// rules of `rules.txt`, the clock the harness answers `time` with.
fn program() -> Program {
    let mut p = Program::new();
    let mut scratch = Dict::new();
    scratch.ed.load_chapters(&mut p.cid, 1, None);
    for r in RULES.get().unwrap() {
        let i = p.rd.add_relation(b"", r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7] as i16, r[8], r[9], None, r[12] as i16, r[13]);
        p.rd.rules[i as usize].active = r[10] as i16;
        p.rd.rules[i as usize].active_dep = r[11] as i16;
    }
    p.now = 1_000_000_000;
    p
}

struct Ctx {
    dicts: Vec<Dict>,
    roots: Vec<(u32, u32)>,
    env: Env,
    rec: Recorder,
}

fn setup(c: &Case) -> Ctx {
    let Parsed { dicts, roots, heap } = canon::parse(&c.input("desc").u32s());
    let mut env = Env { p: program(), heap, ..Env::default() };
    env.g.database_root = 0x1234;
    Ctx { dicts, roots, env, rec: Recorder::default() }
}

impl Ctx {
    /// The handle of the object at a position among the case's roots; 0 for "none".
    fn root(&self, at: u32) -> u32 {
        if at == NIL { 0 } else { self.roots[at as usize].1 }
    }

    fn pairs(&self, c: &Case, name: &str) -> Vec<(u32, u32)> {
        c.try_input(name).map_or(Vec::new(), |f| f.u32s().chunks(2).map(|p| (self.root(p[0]), self.root(p[1]))).collect())
    }

    fn effects(&mut self, c: &Case) {
        if let Some(f) = c.try_input("effects") {
            self.rec.effects = f.u32s().chunks(3).map(|t| (self.root(t[0]), (t[1] != 0).then_some(t[2] as i32))).collect();
        }
    }
}

fn pack(out: &mut Vec<u32>, s: Option<&[u8]>) {
    match s {
        None => out.push(NIL),
        Some(s) => {
            out.push(s.len() as u32);
            for ch in s.chunks(4) {
                let mut b = [0u8; 4];
                b[..ch.len()].copy_from_slice(ch);
                out.push(u32::from_le_bytes(b));
            }
        }
    }
}

/// The recorded calls in the generator's encoding, pointers as object numbers.
fn log_words(calls: &[Call], ids: &HashMap<u32, u32>) -> (Vec<u32>, Vec<u8>) {
    let id = |h: u32| if h == 0 { 0 } else { ids.get(&h).copied().unwrap_or(0x8000_0000) };
    let mut w = Vec::new();
    let mut stdout = Vec::new();
    for call in calls {
        match call {
            Call::Status(text, level) => {
                w.push(1);
                pack(&mut w, Some(text));
                w.push(*level as u32);
            }
            Call::Usleep(_) => {}
            Call::Print(text) => stdout.extend_from_slice(text),
            Call::GrafNodeInfo { render } => w.extend([3, *render as u32]),
            Call::MaterialAllocate(h) => w.extend([4, id(*h)]),
            Call::UpdateSonState { father, model, flag } => w.extend([5, id(*father), id(*model), *flag as u16 as u32]),
            Call::ExpandAll { master, copy } => w.extend([6, id(*master), id(*copy)]),
            Call::ExpandUpdateTransfo { master, instance, copy } => w.extend([7, id(*master), id(*instance), id(*copy)]),
            Call::NbTriangles(m) => w.extend([8, id(*m)]),
            Call::WarnUv(name) => {
                w.push(9);
                pack(&mut w, name.as_deref());
            }
            Call::EffectByIcon(m) => w.extend([10, id(*m)]),
            Call::TopoSort => w.push(11),
            Call::UnToldRelation(a, b, c) => w.extend([12, *a as u16 as u32, *b as u16 as u32, *c as u16 as u32]),
            Call::SceneUpdateItems(bits, camera) => w.extend([13, *bits, id(*camera)]),
            Call::WindowHilite => w.push(14),
            Call::PictureElemRemove(data) => w.extend([15, *data]),
            Call::UdfResetActive => w.push(16),
            Call::UdfIsActive(s) => w.extend([17, *s]),
            Call::UdfSetAsSelected(s) => w.extend([18, *s]),
        }
    }
    (w, stdout)
}

fn first_difference(want: &[Option<u128>], got: &[u32]) -> Option<String> {
    if want.len() != got.len() {
        let at = want.iter().zip(got).position(|(w, g)| w.is_some_and(|w| w as u32 != *g));
        return Some(format!("{} words, the original {}; first difference at {:?}", got.len(), want.len(), at));
    }
    want.iter().zip(got).enumerate().find(|(_, (w, g))| w.is_some_and(|w| w as u32 != **g)).map(|(i, (w, g))| format!("word {i}: port {g:08x}, original {:08x}", w.unwrap() as u32))
}

/// Compares the world and the calls after the port ran with what the original left.
fn finish(c: &Case, ctx: &Ctx, order: &[usize]) -> (Verdict, HashMap<u32, u32>) {
    let dicts: Vec<&Dict> = order.iter().map(|i| &ctx.dicts[*i]).collect();
    let (mut dump, ids) = canon::dump_with_ids(&dicts, &ctx.roots, &ctx.env.heap, &ctx.env.p.cid);
    if mutate() {
        dump[0] ^= 1;
    }
    if let Some(d) = first_difference(&c.output("dump").written(), &dump) {
        return (Verdict::Different(format!("world: {d}")), ids);
    }
    let (log, stdout) = log_words(&ctx.rec.calls, &ids);
    if let Some(d) = first_difference(&c.output("log").written(), &log) {
        return (Verdict::Different(format!("calls: {d} ({:?})", ctx.rec.calls.iter().take(6).collect::<Vec<_>>())), ids);
    }
    // the pauses, which the harness records itself
    let slept: Vec<u128> = c.calls.iter().filter(|k| k.name == "usleep").map(|k| k.arg_values()[0]).collect();
    let sleeps: Vec<u128> = ctx.rec.calls.iter().filter_map(|k| if let Call::Usleep(us) = k { Some(*us as u128) } else { None }).collect();
    if slept != sleeps {
        return (Verdict::Different(format!("pauses: port {sleeps:?}, original {slept:?}")), ids);
    }
    let want = c.try_output("stdout").map(|f| f.to_bytes()).unwrap_or_default();
    (Verdict::from(want == stdout, || format!("standard output: port {:?}, original {:?}", String::from_utf8_lossy(&stdout), String::from_utf8_lossy(&want))), ids)
}

fn word(c: &Case, name: &str, got: u32) -> Verdict {
    let got = if mutate() { got ^ 1 } else { got };
    let want = c.output(name).written();
    Verdict::from(want.first().copied().flatten().is_none_or(|w| w as u32 == got), || format!("{name}: port {got:#x}, original {:?}", want.first()))
}

fn args(c: &Case) -> Vec<u32> {
    c.input("args").u32s()
}

fn elem(a: &[u32]) -> ElemId {
    ElemId::new(a[0] as i32, a[1] as i32)
}

/// A routine of one dictionary: `f(dictionary, the rest)`; `after` compares what it
/// answered, given the number each object got in the final world.
fn one_with<R>(c: &Case, f: impl FnOnce(&mut Dict, &mut Ctx2, &[u32]) -> R, after: impl FnOnce(&HashMap<u32, u32>, R) -> Verdict) -> Verdict {
    let mut ctx = setup(c);
    ctx.effects(c);
    let a = c.try_input("args").map(|f| f.u32s()).unwrap_or_default();
    let mut d = std::mem::take(&mut ctx.dicts[0]);
    let r = {
        let mut x = Ctx2 { env: &mut ctx.env, rec: &mut ctx.rec, roots: &ctx.roots };
        f(&mut d, &mut x, &a)
    };
    ctx.dicts[0] = d;
    let order: Vec<usize> = (0..ctx.dicts.len()).collect();
    let (v, ids) = finish(c, &ctx, &order);
    v.and(|| after(&ids, r))
}

fn one(c: &Case, f: impl FnOnce(&mut Dict, &mut Ctx2, &[u32])) -> Verdict {
    one_with(c, f, |_, _| Verdict::Same)
}

struct Ctx2<'a> {
    env: &'a mut Env,
    rec: &'a mut Recorder,
    roots: &'a [(u32, u32)],
}

impl Ctx2<'_> {
    fn root(&self, at: u32) -> u32 {
        if at == NIL { 0 } else { self.roots[at as usize].1 }
    }
}

/// A routine of the scene's dictionary and a render dictionary: `f(scene, render, the rest)`.
fn two_with<R>(c: &Case, f: impl FnOnce(&mut Dict, &mut Dict, &mut Ctx2, &[u32]) -> R, after: impl FnOnce(&HashMap<u32, u32>, R) -> Verdict) -> Verdict {
    let mut ctx = setup(c);
    ctx.effects(c);
    ctx.rec.expansions = ctx.pairs(c, "expansions");
    let a = c.try_input("args").map(|f| f.u32s()).unwrap_or_default();
    let mut ren = ctx.dicts.pop().unwrap();
    let mut scene = ctx.dicts.pop().unwrap();
    let r = {
        let mut x = Ctx2 { env: &mut ctx.env, rec: &mut ctx.rec, roots: &ctx.roots };
        f(&mut scene, &mut ren, &mut x, &a)
    };
    ctx.dicts.push(scene);
    ctx.dicts.push(ren);
    let (v, ids) = finish(c, &ctx, &[0, 1]);
    v.and(|| after(&ids, r))
}

fn two(c: &Case, f: impl FnOnce(&mut Dict, &mut Dict, &mut Ctx2, &[u32])) -> Verdict {
    two_with(c, f, |_, _| Verdict::Same)
}

/// The bytes of a string as the generator packs one: null, or length and bytes.
fn unpack(w: &[u32]) -> Option<Vec<u8>> {
    if w[0] == NIL {
        return None;
    }
    let mut b: Vec<u8> = w[1..].iter().flat_map(|x| x.to_le_bytes()).collect();
    b.truncate(w[0] as usize);
    Some(b)
}

fn packed(s: Option<&[u8]>) -> Vec<u32> {
    let mut w = Vec::new();
    pack(&mut w, s);
    w
}

fn words(c: &Case, name: &str, got: &[u32]) -> Verdict {
    let mut got = got.to_vec();
    if mutate() {
        got[0] ^= 1;
    }
    match first_difference(&c.output(name).written(), &got) {
        None => Verdict::Same,
        Some(d) => Verdict::Different(format!("{name}: {d}")),
    }
}

fn object(ids: &HashMap<u32, u32>, h: u32) -> u32 {
    if h == 0 { 0 } else { ids.get(&h).copied().unwrap_or(0x8000_0000) }
}

/// Sets the camera globals of a case: scene camera, the default perspective window's, the
/// highlighted window's, the stand-alone switch, the picture width.
fn cameras(x: &mut Ctx2, a: &[u32]) {
    x.env.g.scene_camera = x.root(a[0]);
    x.env.g.pers_camera = x.root(a[1]);
    x.rec.hilite_camera = (a[3] != 0).then_some(x.root(a[2]));
    x.env.g.standalone_renderer = a[4] != 0;
    x.env.g.picture_width = a[5] as i16;
}

/// The render controls of a shader scan case.
fn scan_ctl(c: &Case) -> udf::ScanControl {
    let w = c.input("ctlv").u32s();
    udf::ScanControl { w25c: w[0] as i16, atmosphere: w[1] as i16, textures_2d: w[2] as i16, textures_3d: w[3] as i16, reflection: w[4] as i16 }
}

/// A routine of the shader scan: the recorder is told which shader records are switched on.
fn scan(c: &Case, f: impl FnOnce(&mut Dict, &Env, &mut Recorder, &udf::ScanControl, &[u32])) -> Verdict {
    one(c, |d, x, a| {
        x.rec.active = c.input("active").u32s().chunks(2).map(|p| (p[0], p[1] as i16)).collect();
        f(d, x.env, x.rec, &scan_ctl(c), a)
    })
}

fn preview_of(x: &Ctx2, a: &[u32]) -> Preview {
    Preview { kind: a[0] as i32, model: x.root(a[1]), material: x.root(a[2]), tex2d: x.root(a[3]), tex3d: x.root(a[4]), replaced: x.root(a[5]) }
}

fn lookup(name: &str) -> Option<Check> {
    // the native hand-over on the memory image (finding 0023, section 17): its cases are
    // named `<routine>#img` and never fall through to a first-pass check of the same name
    if name.ends_with("#img") {
        return Some(emucheck_native::lookup(name).unwrap_or(|_| Verdict::Skip));
    }
    Some(match name {
        "SI_propagateHiddenFlag" => |c| one(c, |d, x, a| propagate::propagate_hidden_flag(d, x.env, elem(a))),
        "SI_propagateSelectFlag" => |c| one(c, |d, x, a| propagate::propagate_select_flag(d, x.env, elem(a))),
        "SI_propagateTransform" => |c| one(c, |d, x, a| propagate::propagate_transform(d, x.env, x.rec, elem(a))),
        "si_renderAddDefaultMaterial" => |c| {
            let mut index = 0;
            one(c, |d, x, a| {
                x.rec.materials = vec![x.root(a[0])];
                index = propagate::render_add_default_material(d, x.env, &mut NewName::default(), x.rec).1;
            })
            .and(|| word(c, "out", index as u32))
        },
        "SI_propagateMaterial" => |c| one(c, |d, x, a| {
            propagate::propagate_material(d, x.env, x.rec, a[0] as i32);
        }),
        "SI_dictCopyMaterialRel" => |c| one(c, |d, x, a| rdict::copy_material_rel(d, x.env, elem(a), elem(&a[2..]))),
        "SI_dictCopyTexture2dRel" => |c| one(c, |d, x, a| rdict::copy_texture2d_rel(d, x.env, elem(a), elem(&a[2..]))),
        "SI_dictCopyTexture3dRel" => |c| one(c, |d, x, a| rdict::copy_texture3d_rel(d, x.env, elem(a), elem(&a[2..]))),
        "SI_dictAddElemData" => |c| {
            let mut index = 0;
            one(c, |d, x, a| {
                let data = if a[1] == NIL { a[2] } else { x.root(a[1]) };
                index = rdict::add_elem_data(d, x.env, a[0] as i32, data);
            })
            .and(|| word(c, "eax", index as u32))
        },
        "SI_dictAttachMaterialToSelectedModels" => |c| {
            one(c, |d, x, a| {
                let p = preview_of(x, a);
                rdict::attach_material_to_selected_models(d, x.env, x.rec, &p)
            })
        },
        "SI_dictAttachT2dGlobal" => |c| one(c, |d, x, a| rdict::attach_t2d_global(d, x.env, x.root(a[0]), x.root(a[1]))),
        "SI_dictAttachT2dLocal" => |c| one(c, |d, x, a| rdict::attach_t2d_local(d, x.env, x.root(a[0]), x.root(a[1]))),
        "SI_dictAddT2dGlbRelations" => |c| one(c, |d, x, a| rdict::add_t2d_glb_relations(d, x.env, a[0] as i32, x.root(a[1]))),
        "SI_dictAddT2dLclRelations" => |c| one(c, |d, x, a| rdict::add_t2d_lcl_relations(d, x.env, a[0] as i32, x.root(a[1]))),
        "SI_dictAddT3dGlbRelation" => |c| one(c, |d, x, a| rdict::add_t3d_glb_relation(d, x.env, a[0] as i32, x.root(a[1]))),
        "SI_dictAddT3dLclRelation" => |c| one(c, |d, x, a| rdict::add_t3d_lcl_relation(d, x.env, a[0] as i32, x.root(a[1]))),
        "SI_dictRemoveMatRelations" => |c| one(c, |d, x, a| rdict::remove_mat_relations(d, x.env, a[0] as i32)),
        "SI_dictRemoveT3dGlbRelation" => |c| one(c, |d, x, a| rdict::remove_t3d_glb_relation(d, x.env, a[0] as i32)),
        "SI_dictRemoveT3dLclRelation" => |c| one(c, |d, x, a| rdict::remove_t3d_lcl_relation(d, x.env, a[0] as i32)),
        "SI_dictRemoveT2dGlobalRelations" => |c| one(c, |d, x, a| rdict::remove_t2d_global_relations(d, x.env, Some(elem(a)))),
        "SI_dictRemoveT2dLocalRelations" => |c| one(c, |d, x, a| rdict::remove_t2d_local_relations(d, x.env, Some(elem(a)))),
        "SI_dictReplaceT3dGlb" => |c| one(c, |d, x, a| rdict::replace_t3d_glb(d, x.env, a[0] as i32, a[1] as i32)),
        "SI_dictReplaceT3dLcl" => |c| one(c, |d, x, a| rdict::replace_t3d_lcl(d, x.env, a[0] as i32, a[1] as i32)),
        "SI_dictUnselectAllChapterElems" => |c| one(c, |d, _, a| rdict::unselect_all_chapter_elems(d, a[0] as i32)),
        "SI_deleteImageMapElems" => |c| one(c, |d, x, _| rdict::delete_image_map_elems(d, x.env, x.rec)),
        "SI_dictAddMaterialShaderRelations" => |c| one(c, |d, x, a| rdict::add_material_shader_relations(d, x.env, a[0] as i32, [a[1], a[2], a[3], a[4]])),
        "SI_dictAddT2dShaderRelation" => |c| one(c, |d, x, a| rdict::add_t2d_shader_relation(d, x.env, a[0] as i32, a[1])),
        "SI_dictAddT3dShaderRelation" => |c| one(c, |d, x, a| rdict::add_t3d_shader_relation(d, x.env, a[0] as i32, a[1])),
        "updateSelectiveLights" => |c| one(c, |d, x, _| rmain::update_selective_lights(d, x.env)),
        "resetTextureElemMaps" => |c| one(c, |d, x, _| rmain::reset_texture_elem_maps(d, x.env)),
        "freeDictChapterNames" => |c| one(c, |d, _, _| rmain::free_dict_chapter_names(d)),
        "InitSceneTransmitFlags" => |c| one(c, |d, _, _| util::init_scene_transmit_flags(d)),
        "addPreviewRelations" => |c| {
            one(c, |d, x, a| {
                if a[6] != 0 {
                    let p = preview_of(x, a);
                    rmain::add_preview_relations(d, x.env, x.rec, &p)
                }
            })
        },
        "SI_checkAllModels" => |c| {
            let mut answer = 0;
            one(c, |d, x, a| {
                x.rec.triangles = c.input("tri").u32s().chunks(2).map(|p| (x.root(p[0]), p[1] as i32)).collect();
                answer = util::check_all_models(d, x.env, x.rec, a[0] as i16);
            })
            .and(|| word(c, "eax", answer as u32))
        },
        "PropagateShadeTreeFlag" => |c| one(c, |d, _, a| util::propagate_shade_tree_flag(d, elem(a))),
        "setHierarchyTransmitFlags" => |c| one(c, |d, x, a| util::set_hierarchy_transmit_flags(d, x.env, elem(a), a[2] as i32, 0)),
        "SI_SetSceneTransmitFlags" => |c| {
            two(c, |scene, ren, x, a| {
                x.env.g.textures_2d_on = a[0] as i16;
                util::set_scene_transmit_flags(ren, scene, x.env, x.rec)
            })
        },
        "SetElemTransmitFlags" => |c| {
            let mut answer = false;
            two(c, |scene, ren, x, a| {
                x.env.g.textures_2d_on = a[0] as i16;
                answer = util::set_elem_transmit_flags(ren, scene, x.env, x.rec, elem(&a[1..]), a[3] as i32, a[1] as i32);
            })
            .and(|| word(c, "ax", answer as u32))
        },
        "SI_instanceElem" => |c| {
            two_with(c, |scene, ren, x, a| instance::instance_elem(a[0] as i32, scene, ren, x.env), |_, answer| {
                let a = args(c);
                word(c, "eax", answer.map_or(0, |e| a[1].wrapping_add(e.index as u32 * 0x84)))
            })
        },
        "SI_deleteInstanceMaterialArrays" => |c| two(c, |scene, ren, x, _| instance::delete_instance_material_arrays(scene, ren, x.env)),
        "SI_instanceElementCleanup" => |c| two(c, |scene, ren, x, _| instance::instance_element_cleanup(scene, ren, x.env)),
        "SI_transferInstLocalTransforms" => |c| two(c, |scene, ren, x, _| instance::transfer_inst_local_transforms(scene, ren, x.env)),
        "SI_updateInstTransforms" => |c| two(c, |scene, ren, x, _| instance::update_inst_transforms(scene, ren, x.env, x.rec)),
        "copySchinfo" => |c| two(c, |scene, ren, x, _| update::copy_schinfo(scene, ren, x.env)),
        "si_renderUpdateInstanceHiddenAndSelect" => |c| two(c, |scene, ren, x, _| update::render_update_instance_hidden_and_select(scene, ren, x.env)),
        "adjustInstMaterialArray" => |c| two(c, |scene, ren, x, _| instance::adjust_inst_material_array(scene, ren, x.env)),
        "addInstanceMatTexToDictionary" => |c| two(c, |scene, ren, x, _| instance::add_instance_mat_tex_to_dictionary(scene, ren, x.env)),
        "SI_renderSceneUpdate" => |c| two(c, |scene, ren, x, a| update::render_scene_update(f32::from_bits(a[0]), a[1], scene, ren, x.env, x.rec)),
        "SI_expandInstances" => |c| two(c, |scene, ren, x, _| instance::expand_instances(scene, ren, x.env, x.rec, &mut InstState::default())),
        "SI_elemdictCopyElements" => |c| two(c, |scene, ren, x, _| edlocal::copy_elements(scene, ren, &x.env.p)),
        "SI_elemdictCopyRenderRelations" => |c| {
            two(c, |scene, ren, x, _| {
                edlocal::copy_render_relations(scene, ren, &mut x.env.p);
            })
        },
        "SI_elemdictGetElemFromData" => |c| {
            one_with(c, |d, x, a| edlocal::get_elem_from_data(d, a[0] as i32, x.root(a[1])), |_, (e, i)| {
                let a = args(c);
                word(c, "out", i as u32).and(|| word(c, "eax", e.map_or(0, |e| a[2].wrapping_add(e.index as u32 * 0x84))))
            })
        },
        "SI_elemdictGetNewNameId" => |c| {
            one_with(
                c,
                |d, _, a| edlocal::get_new_name_id(d, &mut NewName::default(), a[0] as i32, unpack(&c.input("name").u32s()).as_deref(), a[1] != 0),
                |_, (name, num)| word(c, "num", num as u32).and(|| words(c, "new", &packed(Some(&name)))),
            )
        },
        "SI_elemdictAddModel" => |c| {
            one(c, |d, x, a| {
                x.env.p.add_in_dictionary = a[2] as i16;
                edlocal::add_model(d, x.env, &mut NewName::default(), x.root(a[0]), Ptr32::new(a[3]), a[1] != 0, &mut Default::default());
            })
        },
        "SI_elemdictReplaceModel" => |c| one(c, |d, x, a| edlocal::replace_model(d, x.env, &mut NewName::default(), a[0] as i32, x.root(a[1]), a[2] != 0)),
        "SI_elemdictModelMasterFromInstance" => |c| {
            one_with(c, |d, x, a| edlocal::model_master_from_instance(d, &x.env.p, x.root(a[0])), |ids, m| word(c, "master", object(ids, m)))
        },
        "SI_instanceMaterialShare" => |c| {
            one_with(
                c,
                |_, x, a| {
                    x.env.g.reg_inst_mat_arrays = a[2] as i16;
                    edlocal::instance_material_share(x.env, x.root(a[0]), x.root(a[1]));
                    x.env.g.mem_list.len() as u32
                },
                |_, n| word(c, "nreg", n),
            )
        },
        "SI_instanceTextureShare" => |c| {
            one(c, |_, x, a| {
                let (from, to) = (x.root(a[0]), x.root(a[1]));
                edlocal::instance_texture_share(&mut x.env.heap, from, to)
            })
        },
        "SI_modelChangeName" => |c| {
            one(c, |_, x, a| {
                let m = x.root(a[0]);
                edlocal::model_change_name(&mut x.env.heap, m, &unpack(&c.input("name").u32s()).unwrap())
            })
        },
        "SI_dbListNodeCopyInfo" => |c| {
            one(c, |d, x, a| {
                let src = d.node_of(a[0] as i32, a[1] as i32).unwrap().clone();
                let mut dst = d.node_of(a[2] as i32, a[3] as i32).unwrap().clone();
                edlocal::list_node_copy_info((a[4] != 0).then_some(&*d), &x.env.p, &src, &mut dst);
                *d.node_of_mut(a[2] as i32, a[3] as i32).unwrap() = dst;
            })
        },
        "SI_deleteRenderDictionary" => |c| {
            two(c, |scene, ren, x, a| {
                let e = (a[0] != NIL).then(|| ElemId::new(x.env.p.cid.mat, a[0] as i32));
                rmain::delete_render_dictionary(scene, ren, x.env, e, a[1] != 0)
            })
        },
        "SI_getRenderCamera" => |c| {
            one_with(
                c,
                |_, x, a| {
                    cameras(x, a);
                    util::get_render_camera(x.env, x.rec, a[6] != 0)
                },
                |ids, cam| word(c, "camera", object(ids, cam)),
            )
        },
        "SI_clampMaxcoc" => |c| {
            one_with(
                c,
                |_, x, a| {
                    cameras(x, a);
                    util::clamp_maxcoc(x.env, x.rec, a[6] != 0)
                },
                |_, v| match v {
                    Some(v) => word(c, "out", v.to_bits()),
                    None => Verdict::from(c.output("out").written()[0].is_none(), || "the original wrote an answer".to_string()),
                },
            )
        },
        "SI_assignMaxcoc" => |c| {
            one(c, |_, x, a| {
                cameras(x, a);
                util::assign_maxcoc(x.env, x.rec, a[6] != 0, f32::from_bits(a[7]))
            })
        },
        "SI_emptyString" => |c| word(c, "eax", util::empty_string(unpack(&c.input("name").u32s()).as_deref()) as u32),
        "SI_buildPicFileName" => |c| {
            let a = args(c);
            let mut env = Env::default();
            env.g.picture_path = unpack(&c.input("path").u32s()).unwrap();
            let got = util::build_pic_file_name(&env, &unpack(&c.input("name").u32s()).unwrap(), a[0] as i32, a[1] != 0, a[2] as i32, a[3] != 0);
            words(c, "file", &packed(Some(&got)))
        },
        "SI_perspectiveNotBView" => |c| {
            one_with(
                c,
                |_, x, _| {
                    x.rec.windows = c.input("wins").u32s().chunks(2).map(|p| (p[0] as i16, x.root(p[1]))).collect();
                    util::perspective_not_b_view(x.env, x.rec)
                },
                |_, r| word(c, "eax", r as u32),
            )
        },
        "isADKitEffect" => |c| {
            one_with(
                c,
                |_, x, a| {
                    let m = x.root(a[0]);
                    util::is_adkit_effect(x.env, x.rec, m)
                },
                |_, r| word(c, "eax", r as u32),
            )
        },
        "initialiseRenderParms" => |c| {
            let mut got = rmain::initialise_render_parms().bytes;
            if mutate() {
                got[0] ^= 1;
            }
            Verdict::from(c.output("p").to_bytes() == got, || "the record differs".to_string())
        },
        "SI_initializePreviewStruct" => |c| {
            let mut p = Preview { kind: 1, model: 2, material: 3, tex2d: 4, tex3d: 5, replaced: 6 };
            rmain::initialize_preview_struct(&mut p);
            let got = if mutate() { 1 } else { 0 };
            Verdict::from(p == Preview::default() && c.output("p").to_bytes().iter().all(|b| *b == got), || "the structure is not cleared".to_string())
        },
        "SI_mentalShaderScan" => |c| scan(c, |d, env, rec, ctl, a| udf::mental_shader_scan(d, env, rec, ctl, a[0] != 0)),
        "si_mentalScanAllModels" => |c| scan(c, |d, env, rec, ctl, a| udf::mental_scan_all_models(d, env, rec, ctl, a[0] != 0)),
        "si_mentalScanAllLights" => |c| scan(c, |d, env, rec, _, _| udf::mental_scan_all_lights(d, env, rec)),
        "si_mentalScanCamera" => |c| scan(c, |d, env, rec, _, _| udf::mental_scan_camera(d, env, rec)),
        "si_mentalScanOutput" => |c| scan(c, |d, env, rec, _, _| udf::mental_scan_output(d, env, rec)),
        "si_mentalScanAtmosphere" => |c| scan(c, |d, env, rec, _, _| udf::mental_scan_atmosphere(d, env, rec)),
        "mentalScanMulti" => |c| scan(c, |d, env, rec, _, a| udf::mental_scan_multi(d, env, rec, a[0] as i32)),
        "mentalScanModel" => |c| scan(c, |d, env, rec, ctl, a| udf::mental_scan_model(d, env, rec, ctl, elem(a), a[2] != 0)),
        "mentalScanMaterial" => |c| scan(c, |d, env, rec, ctl, a| udf::mental_scan_material(d, env, rec, ctl, elem(a))),
        "mentalScanTexture2D" => |c| scan(c, |d, env, rec, _, a| udf::mental_scan_texture2d(d, env, rec, elem(a), a[2] != 0)),
        "mentalScanTexture3D" => |c| scan(c, |d, env, rec, _, a| udf::mental_scan_texture3d(d, env, rec, elem(a), a[2] != 0)),
        "SI_createRenderDictionary" => |c| {
            let mut answer = None;
            two(c, |scene, ren, x, a| {
                x.rec.materials = vec![x.root(a[2])];
                x.env.g.textures_2d_on = a[3] as i16;
                let pc = c.input("prevc").u32s();
                x.env.g.prev = preview_of(x, &pc);
                answer = rmain::create_render_dictionary(scene, ren, x.env, x.rec, &mut rmain::RenState::default(), a[0] != 0, a[1] != 0);
            })
            .and(|| {
                let want = c.output("eax").written()[0];
                Verdict::from(want.is_none_or(|w| (w != 0) == (answer.is_some() != mutate())), || format!("answer: port {answer:?}, original {want:?}"))
            })
            .and(|| word(c, "dbroot", 0))
        },
        _ => return emucheck_sys::lookup(name).or_else(|| emucheck_boxes::lookup(name)),
    })
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: rensetup-emucheck <dir>");
    let dir = Path::new(&dir);
    let rules = std::fs::read_to_string(dir.join("rules.txt")).expect("rules.txt (written by emu_gen.py)");
    RULES.set(rules.lines().map(|l| l.split_whitespace().map(|x| x.parse().unwrap()).collect()).collect()).unwrap();
    let mut subs: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
    subs.sort();
    let mut rows: Vec<(String, Tally)> = Vec::new();
    for sub in subs {
        rows.extend(check_dir_with(&sub, &lookup, true).unwrap());
    }
    let ok = print_report(&rows);
    if mutate() {
        // every compared case must differ
        let live = rows.iter().all(|(_, t)| t.identical == 0 && t.identical_given_calls == 0);
        println!("mutation: {}", if live { "every compared case differs" } else { "SOME CASES STILL AGREE" });
        std::process::exit(if live { 0 } else { 1 });
    }
    std::process::exit(if ok { 0 } else { 1 });
}
