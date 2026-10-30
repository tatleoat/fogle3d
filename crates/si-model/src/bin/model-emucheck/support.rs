// SPDX-License-Identifier: MIT
//! Checks of the mesh's support files (vectors of `emu_gen3.py`): `mesh_polygon.rs`,
//! `mesh_util.rs` and the centroid of `obj_global.rs`.
//!
//! Most cases are *chained*: the binary's `SI_meshUpdateNodes` ran on the case's mesh and
//! then, on the same state, the routine under test. The check does the same with the ports
//! and compares everything both left: the whole mesh, its two selection lists, the number
//! of status messages, and the routine's own results.

use si_math::types::{Matrix, Vector};
use si_model::mesh::{ListItem, Mesh, MeshList, PolyNode, Polygon, Vertex};
use si_model::mesh_b::{self, MeshEnv};
use si_model::mesh_polygon as mp;
use si_model::mesh_util as mu;
use si_model::node::Models;
use si_model::transform::Globals;
use si_model::{obj_global, Definition};
use si_x87::vectors::{Case, Check, Field, Verdict, Word};

use crate::world::mutate;

/// The port's words against a field of the vector: the same count, and every word the
/// original wrote the same. With `EMUCHECK_MUTATE` one bit of each of the port's words is
/// flipped first.
pub fn same(c: &Case, name: &str, got: &[u32]) -> Verdict {
    let Some(want) = c.try_output(name) else {
        return Verdict::Different(format!("the vector has no field {name}"));
    };
    same_field(want, name, got)
}

pub fn same_field(want: &Field, name: &str, got: &[u32]) -> Verdict {
    let w = want.written();
    if w.len() != got.len() {
        return Verdict::Different(format!("{name}: {} words against the port's {}", w.len(), got.len()));
    }
    for (i, (a, b)) in w.iter().zip(got).enumerate() {
        let b = *b ^ mutate() as u32;
        if a.is_some_and(|a| a as u32 != b) {
            return Verdict::Different(format!("{name}[{i}]: {:08x} against the port's {b:08x}", a.unwrap() as u32));
        }
    }
    Verdict::Same
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|f| f.to_bits()).collect()
}

fn matrix_bits(m: &Matrix) -> Vec<u32> {
    m.iter().flatten().map(|f| f.to_bits()).collect()
}

/// The six fields of a mesh dump, named `<prefix>hdr` and so on.
pub fn same_mesh(c: &Case, m: &Mesh, prefix: &str, fields: usize) -> Verdict {
    for (name, got) in crate::dump(m).into_iter().take(fields) {
        let v = same(c, &name.replace("m.", prefix), &got);
        if v != Verdict::Same {
            return v;
        }
    }
    Verdict::Same
}

fn item(i: ListItem) -> [u32; 2] {
    match i {
        ListItem::Null => [0, 0],
        ListItem::Polygon(p) => [1, p as u32],
        ListItem::Vertex(v) => [2, v as u32],
    }
}

/// A list as `emu_gen3.py` dumps it.
pub fn dump_list(l: &MeshList, deep: bool) -> Vec<u32> {
    let mut out = vec![l.nodes.len() as u32];
    for n in l.iter() {
        out.extend(item(n.item));
        if deep {
            out.extend(dump_list(&n.sub, false));
            out.extend(dump_list(&n.sub2, false));
            out.extend(matrix_bits(&n.matrix));
        }
    }
    out
}

fn same_lists(c: &Case, m: &Mesh, prefix: &str) -> Verdict {
    same(c, &format!("{prefix}pol"), &dump_list(&m.selected_polygons, true)).and(|| same(c, &format!("{prefix}vrt"), &dump_list(&m.selected_vertices, true)))
}

/// The case's mesh after the port's rebuild, as the generator's chain leaves it.
fn rebuilt(c: &Case) -> (Mesh, MeshEnv, Verdict) {
    let mut m = crate::mesh_of(c);
    let mut env = MeshEnv::default();
    if let Some(Word::Ref { .. }) = c.input("mdl").words().get(1) {
        env.model_name = Some(b"m".to_vec());
    }
    let ok = mesh_b::update_nodes(&mut m, c.input("normals").i16() != 0, &mut env);
    let v = Verdict::from(c.output("ax").u16s() == [ok as u16], || format!("the rebuild's result: port {ok}"));
    (m, env, v)
}

/// What every chained case ends with: the status messages, the mesh, its lists. A case in
/// which a port had to go on with memory the original never set is not compared.
fn finish(c: &Case, m: &Mesh, env: &MeshEnv, first: Verdict) -> Verdict {
    if si_model::env::stale::count() > 0 {
        return Verdict::Skip;
    }
    first
        .and(|| Verdict::from(env.messages.len() == crate::calls(c, "SI_statusDisplayMessage"), || format!("{} messages", env.messages.len())))
        .and(|| same_mesh(c, m, "m.", 6))
        .and(|| same_lists(c, m, "l."))
}

fn chained(c: &Case, f: impl FnOnce(&mut Mesh, &mut MeshEnv) -> Verdict) -> Verdict {
    si_model::env::stale::reset();
    let (mut m, mut env, first) = rebuilt(c);
    let v = first.and(|| f(&mut m, &mut env));
    finish(c, &m, &env, v)
}

fn args(c: &Case) -> Vec<i32> {
    c.input("arg").i32s()
}

fn update_polygon_lists(c: &Case) -> Verdict {
    chained(c, |_, _| Verdict::Same)
}

fn centroid(c: &Case) -> Verdict {
    chained(c, |m, _| {
        let (out, ok) = obj_global::mesh_polygon_centroid(m, args(c)[0]);
        same(c, "r.ax", &[ok as u32]).and(|| same(c, "r.out", &bits(&out)))
    })
}

fn poly_coord_system(c: &Case) -> Verdict {
    chained(c, |m, _| same(c, "r.out", &matrix_bits(&mp::get_poly_coord_system(m, args(c)[1]))))
}

fn average_coord_system(c: &Case) -> Verdict {
    chained(c, |m, env| {
        let a = args(c);
        let mut l = MeshList::default();
        for &p in &a[1..1 + a[5] as usize] {
            mp::add_list(&mut l, ListItem::Polygon(p));
        }
        same(c, "r.out", &matrix_bits(&mp::get_average_coord_system(&l, m, env)))
    })
}

fn polygon_neighbors(c: &Case) -> Verdict {
    chained(c, |m, _| {
        let a = args(c);
        let l = mp::get_polygon_neighbors(m, a[1], a[6] != 0);
        same(c, "r.list", &dump_list(&l, true))
    })
}

fn connected_polygons(c: &Case) -> Verdict {
    chained(c, |m, _| {
        let a = args(c);
        let mut l = MeshList::default();
        mp::get_connected_polygons(m, Some(a[1]), a[6] != 0, &mut l);
        same(c, "r.list", &dump_list(&l, true))
    })
}

fn sel_pol_info(c: &Case) -> Verdict {
    chained(c, |m, env| {
        let (mut pol, mut vrt) = (MeshList::default(), MeshList::default());
        mp::get_sel_pol_info(&mut pol, &mut vrt, m);
        let first = same(c, "r.pol", &dump_list(&pol, true)).and(|| same(c, "r.vrt", &dump_list(&vrt, true)));
        mp::get_sel_pol_coord_sys(&mut pol, m, env);
        first.and(|| same(c, "r.pol2", &dump_list(&pol, true)))
    })
}

fn pols_to_list(c: &Case) -> Verdict {
    chained(c, |m, _| {
        let (mut found, mut done) = (MeshList::default(), MeshList::default());
        mp::add_pols_to_list(&mut found, &m.selected_vertices, &mut done, m, args(c)[1]);
        same(c, "r.found", &dump_list(&found, true)).and(|| same(c, "r.done", &dump_list(&done, true)))
    })
}

fn copy_polygon_lists(c: &Case) -> Verdict {
    chained(c, |m, env| {
        for (p, f) in m.polygons.iter_mut().zip(c.input("sel").u32s()) {
            p.flag = f as i16;
        }
        mp::copy_polygon_lists(None, m, env);
        Verdict::Same
    })
}

fn mesh_copy_lists(c: &Case) -> Verdict {
    chained(c, |m, env| {
        crate::allocate_cosine();
        let copy = obj_global::mesh_copy_with(m, env);
        same_mesh(c, &copy, "c.", 5).and(|| same_lists(c, &copy, "cl."))
    })
}

fn transform_for_extrusion(c: &Case) -> Verdict {
    chained(c, |m, env| {
        let mut l = core::mem::take(&mut m.selected_polygons);
        mp::compute_transform_for_extrusion(&mut l, m, args(c)[6] != 0, env);
        m.selected_polygons = l;
        Verdict::Same
    })
}

fn update_tagged(c: &Case) -> Verdict {
    chained(c, |m, _| {
        for (p, f) in m.polygons.iter_mut().zip(c.input("sel").u32s()) {
            p.flag |= f as i16;
        }
        for (v, p) in m.vertices.iter_mut().zip(c.input("moved").f32s().chunks(3)) {
            v.pos = [p[0], p[1], p[2]];
        }
        mp::update_tagged(m);
        Verdict::Same
    })
}

fn field_matrix(f: &Field, at: usize) -> Matrix {
    let w = f.words();
    let mut m = [[0.0f32; 4]; 4];
    for i in 0..16 {
        m[i / 4][i % 4] = f32::from_bits(w[at + i].value().unwrap_or(0) as u32);
    }
    m
}

const W_GMAT: usize = 0x1c4 / 4;
const W_FATHER: usize = 0x2bc / 4;

/// The nodes of a transformation case: 0 the model, 1 its father's record, 2 the reference
/// object; and the settings.
fn manip(c: &Case) -> (Models, Globals) {
    let mut models = Models::default();
    for name in ["model", "father", "refm"] {
        let id = models.allocate(None);
        models.m_mut(id).gmat = field_matrix(c.input(name), W_GMAT);
    }
    if let Word::Ref { .. } = c.input("model").words()[W_FATHER] {
        models.m_mut(0).father = Models::ptr(1);
    }
    let w = c.input("manip").words();
    let g = Globals { rot_mode: w[2].value().unwrap_or(0) as i32, trans_mode: w[3].value().unwrap_or(0) as i32, ref_model: Some(2), ..Globals::default() };
    (models, g)
}

/// The stand-in of `SI_matrixRotationReferTag`, as the generator's: it records its
/// arguments and answers the identity with the angles in row 3.
#[derive(Default)]
struct Refer {
    calls: Vec<Vec<u32>>,
}

impl mp::Hooks for Refer {
    fn matrix_rotation_refer_tag(&mut self, a: Vector, gmat: &Matrix) -> Matrix {
        let mut w = bits(&[a.x, a.y, a.z]);
        w.extend(matrix_bits(gmat));
        self.calls.push(w);
        [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [a.x, a.y, a.z, 1.0]]
    }
}

impl Refer {
    fn same(&self, c: &Case) -> Verdict {
        let want: Vec<Vec<u32>> = c.calls.iter().filter(|k| k.name == "SI_matrixRotationReferTag").map(|k| k.arg_values().iter().map(|&v| v as u32).collect()).collect();
        let got: Vec<Vec<u32>> = self.calls.iter().map(|w| w.iter().map(|v| v ^ mutate() as u32).collect()).collect();
        Verdict::from(want == got, || format!("{} calls of SI_matrixRotationReferTag against the port's {}, or other arguments", want.len(), got.len()))
    }
}

fn vec3(f: &Field, at: usize) -> Vector {
    let w = f.words();
    let v = |i: usize| f32::from_bits(w[at + i].value().unwrap_or(0) as u32);
    Vector::new(v(0), v(1), v(2))
}

fn transform_selected(c: &Case) -> Verdict {
    let (models, g) = manip(c);
    let mut hooks = Refer::default();
    let t = c.input("targ");
    let v = chained(c, |m, _| {
        mp::transform_selected_polygons(m, &models, 0, t.words()[0].value().unwrap_or(0) as i32, vec3(t, 1), &g, &mut hooks);
        Verdict::Same
    });
    v.and(|| hooks.same(c))
}

/// A routine that writes a matrix the case also supplies (`out`, in and out).
fn direct(c: &Case, f: impl FnOnce(&Models, &Globals, &Matrix, &mut Matrix, &mut Refer) -> Option<Vector>) -> Verdict {
    si_model::env::stale::reset();
    let (models, g) = manip(c);
    let mut hooks = Refer::default();
    let system = field_matrix(c.input("sys"), 0);
    let mut out = field_matrix(c.input("out"), 0);
    let ret = f(&models, &g, &system, &mut out, &mut hooks);
    if si_model::env::stale::count() > 0 {
        return Verdict::Skip;
    }
    same_field(c.after("out"), "out", &matrix_bits(&out))
        .and(|| match ret {
            Some(v) => same(c, "ret", &bits(&[v.x, v.y, v.z])),
            None => Verdict::Same,
        })
        .and(|| hooks.same(c))
}

fn poly_translate(c: &Case) -> Verdict {
    direct(c, |models, g, system, out, _| Some(mp::get_poly_translate(system, models, 0, vec3(c.input("v"), 0), g, out)))
}

fn poly_rotate(c: &Case) -> Verdict {
    direct(c, |models, g, system, out, hooks| {
        *out = mp::get_poly_rotate_matrix(system, models, 0, vec3(c.input("v"), 0), g, hooks);
        None
    })
}

fn poly_scale(c: &Case) -> Verdict {
    direct(c, |_, _, system, out, _| {
        *out = mp::get_poly_scale_matrix(system, vec3(c.input("v"), 0));
        None
    })
}

fn poly_transform(c: &Case) -> Verdict {
    direct(c, |models, g, system, out, hooks| {
        mp::get_poly_transform_matrix(system, models, 0, c.input("kind").i32(), vec3(c.input("v"), 0), g, hooks, out);
        None
    })
}

fn proj_vect(c: &Case) -> Verdict {
    let (d, n) = (c.input("d").f32s(), c.input("n").f32s());
    let (out, ok) = mp::proj_vect([d[0], d[1], d[2]], [n[0], n[1], n[2]]);
    same(c, "ax", &[ok as u32]).and(|| same(c, "out", &bits(&out)))
}

/// The list primitives as a sequence of operations on two lists over ten records.
fn list_script(c: &Case) -> Verdict {
    let recs = crate::values(c.input("polys"));
    let first: Vec<i32> = recs.chunks(7).map(|r| r[0] as i32).collect();
    let mut mesh = Mesh::default();
    for r in recs.chunks(7) {
        mesh.polygons.push(Polygon { normal: [0.0; 3], nodes: Vec::new(), material: 0, flag: r[5] as i16 });
    }
    let mut lists = [MeshList::default(), MeshList::default()];
    let mut res = Vec::new();
    let index = |i: ListItem| match i {
        ListItem::Polygon(p) => 1 + p as u32,
        _ => 0,
    };
    for op in c.input("ops").u32s().chunks(3) {
        let (li, x) = (op[1] as usize, op[2] as i32);
        res.push(match op[0] {
            0 => {
                mp::add_list(&mut lists[li], ListItem::Polygon(x));
                1
            }
            1 => {
                let tail = core::mem::take(&mut lists[1 - li]);
                mp::concat_list(&mut lists[li], tail);
                0
            }
            2 => mp::find_list(&lists[li], ListItem::Polygon(x)).map_or(0, |at| index(lists[li].nodes[at].item)),
            3 => mp::cnt_list(&lists[li]) as u32,
            4 => mp::find_vertex(&lists[li], x % 4, |i| match i {
                ListItem::Polygon(p) => first[p as usize],
                _ => 0,
            })
            .map_or(0, index),
            5 => mp::free_list(&mut lists[li]) as u32,
            _ => {
                mp::clear_tag_mark(&mut mesh, &lists[li]);
                0
            }
        });
    }
    let items = |l: &MeshList| l.iter().map(|n| index(n.item)).collect::<Vec<u32>>();
    let flags: Vec<u32> = mesh.polygons.iter().map(|p| p.flag as u16 as u32).collect();
    same(c, "r.res", &res).and(|| same(c, "r.l0", &items(&lists[0]))).and(|| same(c, "r.l1", &items(&lists[1]))).and(|| same(c, "r.flags", &flags))
}

fn order_mesh(c: &Case) -> Verdict {
    if std::env::var_os("ORDER_IMAGE").is_some() {
        return order_mesh_image(c);
    }
    crate::allocate_cosine();
    let m = si_model::order::order_mesh(&crate::mesh_of(c));
    let uv: Vec<u32> = m.polygons.iter().flat_map(|p| p.nodes.iter().flat_map(|n| [n.uv[0].to_bits(), n.uv[1].to_bits(), n.color])).collect();
    same_mesh(c, &m, "m.", 4).and(|| same(c, "r.uv", &uv))
}

/// The same vector through the image-based form of `order.c` (finding 0024;
/// `ORDER_IMAGE=1`). The case's buffers go into `si-mem`'s arena as they are, the port runs
/// on them, and the mesh it leaves in the heap is walked as the generator's `post` walks
/// the emulator's memory: the same four dump fields and `r.uv` as the form above is held
/// to. With `ORDER_IMAGE=2` two things the form above cannot be asked are compared as
/// well: the address of the new mesh (`eax`) and the order in which blocks were freed
/// (`freed`), which agree only when every allocation of the original was made, in order
/// and with its size.
fn order_mesh_image(c: &Case) -> Verdict {
    use si_mem::{Mem, Ptr32, Word as W};
    use si_types::geom;
    let words = |f: &Field| -> Option<Vec<W>> {
        let convert = |w: &Word| match w {
            Word::Value { bytes, value } => W::Value { bytes: *bytes, value: *value },
            Word::Unwritten { bytes } => W::Unwritten { bytes: *bytes },
            Word::Ref { name, offset } => W::Ref { name: name.clone(), offset: *offset },
        };
        f.words.as_ref().map(|ws| ws.iter().map(convert).collect())
    };
    let mut m = Mem::new();
    let owned: Vec<(&str, Option<Vec<W>>)> = c.inputs.iter().map(|f| (f.name.as_str(), words(f))).collect();
    let fields: Vec<(&str, Option<&[W]>)> = owned.iter().map(|(n, w)| (*n, w.as_deref())).collect();
    let names = match si_mem::load_case(&mut m, &fields) {
        Ok(n) => n,
        Err(e) => return Verdict::Different(e),
    };
    let mesh: Ptr32<geom::Mesh> = Ptr32::new(names.addr(&m, "mesh").unwrap());
    let out = si_model::order_image::order_mesh(&mut m, mesh);
    if let Some(f) = m.fault() {
        return Verdict::Different(format!("the port faulted: {f:?}"));
    }
    // the generator's walk of the result
    let h = m[out];
    let hdr = vec![h.angle.to_bits(), h.cos_angle.to_bits(), h.nb_vertices as u32, h.nb_polygons as u32, h.nb_edges as u32, h.flags as u32, h.nb_display as u32];
    let (mut vpos, mut stars, mut polys, mut uv) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..h.nb_vertices {
        let v = h.vertices.add(i);
        vpos.extend((0..3).map(|w| m.u32(v.addr() + 4 * w)));
        vpos.push(m[v].flag as u16 as u32);
        stars.push(m[v].nb_vectors as u32);
        let star: Ptr32<[u32; 3]> = m[v].vectors.cast();
        for s in 0..m[v].nb_vectors as i32 {
            stars.extend(m[star.add(s)]);
        }
    }
    for i in 0..h.nb_polygons {
        let q = h.polygons.add(i);
        polys.extend((0..3).map(|w| m.u32(q.addr() + 4 * w)));
        polys.extend([m[q].nb_nodes as u32, m[q].material as u16 as u32, m[q].flag as u16 as u32]);
        for k in 0..m[q].nb_nodes as i32 {
            let n = m[q].nodes.add(k).addr();
            polys.extend((0..5).map(|w| m.u32(n + 4 * w)));
            uv.extend([m.u32(n + 0x14), m.u32(n + 0x18), m.u32(n + 0x1c)]);
        }
    }
    let v = same(c, "m.hdr", &hdr).and(|| same(c, "m.vpos", &vpos)).and(|| same(c, "m.stars", &stars)).and(|| same(c, "m.polys", &polys)).and(|| same(c, "r.uv", &uv));
    if v != Verdict::Same || std::env::var("ORDER_IMAGE").as_deref() != Ok("2") {
        return v;
    }
    let block = |a: u32| m.heap_block_of(a).map_or("?".to_string(), |(i, off)| format!("heap{i}+{off:#x}"));
    let eax = c.output("eax").u32();
    if eax != out.addr() ^ mutate() as u32 {
        return Verdict::Different(format!("eax: {eax:08x} against the port's {:08x} ({})", out.addr(), block(out.addr())));
    }
    let freed: Vec<String> = m.freed().iter().map(|&a| names.name_of(&m, a).map_or("?".to_string(), |(n, off)| if off == 0 { n } else { format!("{n}+{off:#x}") })).collect();
    let want: Vec<String> = c.output("freed").words().iter().map(|w| match w {
        Word::Ref { name, offset: 0 } => name.clone(),
        other => format!("{other:?}"),
    }).collect();
    if mutate() || freed != want {
        let at = freed.iter().zip(&want).position(|(a, b)| a != b);
        return Verdict::Different(format!("freed: {} blocks against the port's {}, first difference at {at:?}", want.len(), freed.len()));
    }
    Verdict::Same
}

// ---- meshUtil.c

fn remove_faceted(c: &Case) -> Verdict {
    let mut m = crate::mesh_of(c);
    mu::remove_faceted(&mut m);
    same_mesh(c, &m, "m.", 6)
}

fn init_vertex_colors(c: &Case) -> Verdict {
    let mut m = crate::mesh_of(c);
    mu::mesh_init_vertex_colors(&mut m);
    let colors: Vec<u32> = m.polygons.iter().flat_map(|p| p.nodes.iter().map(|n| n.color)).collect();
    same(c, "r.col", &colors)
}

fn planar(c: &Case) -> Verdict {
    chained(c, |m, _| same(c, "r.ax", &[mu::mesh_planar(m) as u32]))
}

fn vertex_normal(c: &Case) -> Verdict {
    chained(c, |m, _| same(c, "r.out", &bits(&mu::compute_mesh_vertex_normal(m, args(c)[0]))))
}

/// One of the two meshes of a `meshCopyAttributes` case, with the further texture
/// coordinates its corners point to.
fn mesh_from(c: &Case, pre: &str) -> Mesh {
    let head = crate::values(c.input(&format!("{pre}mesh")));
    let mut m = Mesh::default();
    m.flags = head[8] as u16;
    for v in crate::values(c.input(&format!("{pre}verts"))).chunks(6) {
        m.vertices.push(Vertex { pos: [f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2])], star: Vec::new(), flag: v[5] as i16 });
    }
    let count = head[6] as usize;
    for (k, p) in crate::values(c.input(&format!("{pre}polys"))).chunks(8).enumerate().take(count) {
        let f = c.input(&format!("{pre}n{k}"));
        let w = crate::values(f);
        let mut nodes = Vec::new();
        for (i, n) in w.chunks(9).take(p[4] as usize).enumerate() {
            let extra_uv = match &f.words()[9 * i + 8] {
                Word::Ref { name, offset } => c.input(name).f32s()[*offset as usize / 4..][..14].chunks(2).map(|q| [q[0], q[1]]).collect(),
                _ => Vec::new(),
            };
            nodes.push(PolyNode {
                vertex: n[0] as i32,
                edge: n[1] as i32,
                normal: [f32::from_bits(n[2]), f32::from_bits(n[3]), f32::from_bits(n[4])],
                uv: [f32::from_bits(n[5]), f32::from_bits(n[6])],
                color: n[7],
                extra_uv,
            });
        }
        m.polygons.push(Polygon { normal: [0.0; 3], nodes, material: p[5] as i16, flag: p[6] as i16 });
    }
    m
}

fn copy_attributes(c: &Case) -> Verdict {
    let (a, mut b) = (mesh_from(c, "a"), mesh_from(c, "b"));
    let nb_uv = (!c.input("model").is_null()).then(|| c.input("model").i32s()[265]);
    let ok = mu::mesh_copy_attributes(nb_uv, &a, &mut b, c.input("what").i32());
    let corners = |m: &Mesh| {
        let mut w = Vec::new();
        for n in m.polygons.iter().flat_map(|p| p.nodes.iter()) {
            w.extend([n.uv[0].to_bits(), n.uv[1].to_bits(), n.color]);
            w.push(2 * n.extra_uv.len() as u32);
            w.extend(n.extra_uv.iter().flatten().map(|v| v.to_bits()));
        }
        w
    };
    same(c, "ax", &[ok as u32]).and(|| same(c, "r.a", &corners(&a))).and(|| same(c, "r.b", &corners(&b)))
}

#[derive(Default)]
struct Converted(Option<&'static str>);

impl mu::ConvertHooks for Converted {
    fn patch_convert_to_mesh(&mut self, _: &Models, _: usize) -> Option<Mesh> {
        self.0 = Some("SI_patchConvertToMesh");
        None
    }
    fn meta_to_mesh(&mut self, _: &Models, _: usize) -> Option<Mesh> {
        self.0 = Some("SI_meta2Mesh");
        None
    }
    fn nurbs_convert_to_mesh(&mut self, _: &Models, _: usize) -> Option<Mesh> {
        self.0 = Some("SI_nurbsConvertToMesh");
        None
    }
    fn face_convert_to_mesh(&mut self, _: &Models, _: usize) -> Option<Mesh> {
        self.0 = Some("SI_faceConvertToMesh");
        None
    }
}

/// Which conversion a node's kind leads to; for a mesh the port copies it where the
/// original calls `SI_meshCopy`.
fn convert_to_mesh(c: &Case) -> Verdict {
    let mut models = Models::default();
    let node = (!c.input("model").is_null()).then(|| {
        let id = models.allocate(None);
        models.m_mut(id).kind = c.input("model").i32s()[12];
        models.data[id].defs.push(Definition::Mesh(Mesh::default()));
        models.m_mut(id).nb_defs = 1;
        models.data[id].current = si_model::node::Current::Def(0);
        id
    });
    let mut hooks = Converted::default();
    let got = mu::model_convert_to_mesh(&models, node, &mut MeshEnv::default(), &mut hooks);
    let called = if got.is_some() { Some("SI_meshCopy") } else { hooks.0 };
    let want = c.calls.first().map(|k| k.name.as_str());
    Verdict::from(want == called.filter(|_| !mutate()), || format!("the original calls {want:?}, the port {called:?}"))
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_meshUpdatePolygonLists" => update_polygon_lists,
        "SI_meshPolygonCentroid" => centroid,
        "getPolyCoordSystem" => poly_coord_system,
        "getAverageCoordSystem" => average_coord_system,
        "getPolygonNeighbors" => polygon_neighbors,
        "getConnectedPolygons" => connected_polygons,
        "SI_meshGetSelPolInfo" => sel_pol_info,
        "SI_meshCopyPolygonLists" => copy_polygon_lists,
        "addPolsToList" => pols_to_list,
        "SI_meshCopy#lists" => mesh_copy_lists,
        "computeTransformForExtrusion" => transform_for_extrusion,
        "SI_meshUpdateTagged" => update_tagged,
        "SI_meshTransformSelectedPolygons" => transform_selected,
        "getPolyTranslate" => poly_translate,
        "getPolyRotateMatrix" => poly_rotate,
        "getPolyScaleMatrix" => poly_scale,
        "getPolyTransformMatrix" => poly_transform,
        "projVect" => proj_vect,
        "SI_meshAddList#script" => list_script,
        "SI_orderMesh" => order_mesh,
        "SI_removeFaceted" => remove_faceted,
        "SI_meshInitVertexColors" => init_vertex_colors,
        "SI_meshPlanar" => planar,
        "SI_computeMeshVertexNormal" => vertex_normal,
        "meshCopyAttributes" => copy_attributes,
        "SI_modelConvertToMesh" => convert_to_mesh,
        _ => return None,
    })
}
