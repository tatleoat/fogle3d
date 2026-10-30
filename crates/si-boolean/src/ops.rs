// SPDX-License-Identifier: MIT
//! The callers of the boolean engine outside `boolean.c` (finding 0037, "bool2"): `clipPlane.c`, `subdivide.c`,
//! the engine side of `animBool.c` and the model-material copy of `bool_ui.c`.
//!
//! Callees of other units are answered through [`Routine`]s (hooks of the caller), the file's own functions are
//! called directly.

use crate::boolean::*;
use crate::mesh::{si_build_mesh, si_build_mesh2, si_compute_boolean, si_generate_face_list};
use si_mem::{Mem, Outside, Routine};
use si_x87::{fmov, x, F80};

fn r(m: &Mem, a: u32) -> u32 {
    m.u32(a)
}
fn w(m: &mut Mem, a: u32, v: u32) {
    m.set_u32(a, v)
}

pub const SI_VALID_MESH: Routine = Routine::new("SI_validMesh", 0x0845_25ec);
pub const SI_ORDER_MESH: Routine = Routine::new("SI_orderMesh", 0x0847_9494);
pub const SI_MESH_UPDATE: Routine = Routine::new("SI_meshUpdate", 0x0845_8d94);
pub const SI_SHOW_ACTIVITY: Routine = Routine::new("SI_showActivity", 0x083d_1a60);
pub const SI_MESH_DISPOSE: Routine = Routine::new("SI_meshDispose", 0x0844_04a0);
pub const SI_MESH_COPY: Routine = Routine::new("SI_meshCopy", 0x0844_04e0);
pub const SI_MODEL_CONVERT_TO_MESH: Routine = Routine::new("SI_modelConvertToMesh", 0x0845_87d4);
pub const SI_MODEL_DELETE_DEFINITION: Routine = Routine::new("SI_modelDeleteDefinition", 0x0846_78a8);
pub const SI_MODEL_ADD_DEFINITION: Routine = Routine::new("SI_modelAddDefinition", 0x0846_7038);
pub const SI_MODEL_TOUCH_GEOMETRY: Routine = Routine::new("SI_modelTouchGeometry", 0x0846_d48c);
pub const SI_MODEL_COPY_TRANSFORMATION: Routine = Routine::new("SI_modelCopyTransformation", 0x0846_1f98);
pub const SI_FIND_GL_DATA: Routine = Routine::new("SI_findGlData", 0x0832_ee68);
pub const SI_UPDATE_GN_FUNC: Routine = Routine::new("SI_updateGnFunc", 0x0817_6618);
pub const SI_DB_CONVERT_MODEL_MAT_TXT2DB: Routine = Routine::new("SI_dbConvertModelMatTxt2DB", 0x0810_8490);
pub const SI_MATRIX_INVERSE: Routine = Routine::new("SI_matrixInverse", 0x0851_672c);
pub const SI_MATRIX_MULTIPLY: Routine = Routine::new("SI_matrixMultiply", 0x0851_6244);
pub const SI_MATRIX_TRANSPOSE_INVERSE: Routine = Routine::new("SI_matrixTransposeInverse", 0x0851_8a3c);
pub const SI_STATUS_DISPLAY_MESSAGE: Routine = Routine::new("SI_statusDisplayMessage", 0x083c_ca94);
pub const SI_MATERIAL_COPY: Routine = Routine::new("SI_materialCopy", 0x0828_4888);
pub const SI_ED_DISPOSE_MATERIAL: Routine = Routine::new("SI_edDisposeMaterial", 0x0810_2114);
/// `SI_procData` and `SI_grafLists` (pointer variables).
const SI_PROC_DATA: u32 = 0x0867_9314;
const SI_GRAF_LISTS: u32 = 0x0867_9498;
/// `"SI_BOOL_DOPLC_ENV"` and `"SI_OLD_SUBDIVISION"` (strings of the binary's data).
const ENV_DOPLC: u32 = 0x086b_98e0;
const ENV_OLD_SUBDIVISION: u32 = 0x086b_9a00;
/// `"A fatal error has occurred. Operation aborted !!!"`.
const MSG_FATAL: u32 = 0x086b_9280;
/// The merge distance (squared) both plane cutters set: 2.5e-9.
const PLANE_MERGE_DIST: f64 = 2.5e-9;

/// The mesh fields the plane cutters carry over to the result (`+0xc`, `+0x10` through the x87, `+0x20` a short).
fn carry_mesh_fields(m: &mut Mem, to: u32, from: u32) {
    let a = fmov(m.f32(from + 0xc));
    m.set_f32(to + 0xc, a);
    let b = fmov(m.f32(from + 0x10));
    m.set_f32(to + 0x10, b);
    let c = m.u16(from + 0x20);
    m.set_u16(to + 0x20, c);
}

/// The last face of a chain linked through `+0x78`.
fn chain_last(m: &Mem, first: u32) -> u32 {
    let mut cur = first;
    loop {
        let nx = r(m, cur + f::NEXT);
        if nx == 0 {
            return cur;
        }
        cur = nx;
    }
}

/// `SI_meshClipPlanes(mesh, n, planes)` [soft 0x083158b0 SI_meshClipPlanes]: keeps what lies behind all `n` planes
/// (`planes` is `n` records of 0x20 bytes, `a b c d` as doubles). The planes go into a tree-node array (plane at
/// `+4`, face list at `+0x24` cleared); every face of the mesh is cut by each plane in turn: a face in front
/// (3) is dropped, a split face keeps its second piece (`*pfront`), the rest goes on; survivors are chained and
/// built into a mesh (`si_buildMesh`), which is disposed again when it answers 0 and the environment variable
/// `SI_BOOL_DOPLC_ENV` is unset. The result takes the source mesh's `+0xc`, `+0x10` and `+0x20`.
/// The two matrices handed to `si_generateFaceList` are uninitialised stack in the original (unused with flag 0).
pub fn si_mesh_clip_planes(m: &mut Mem, o: &mut dyn Outside, mesh: u32, n: i32, planes: u32) -> u32 {
    set_merge_dist(m, PLANE_MERGE_DIST);
    init_boolean(m);
    let tree = si_get_mem(m, o, n.wrapping_mul(0x30));
    for k in 0..n.max(0) as u32 {
        for i in 0..8 {
            let v = r(m, planes + k * 0x20 + i * 4);
            w(m, tree + 4 + k * 0x30 + i * 4, v);
        }
        w(m, tree + k * 0x30 + 0x24, 0);
    }
    set_classifying(m, 0);
    let fr = m.local(0x50);
    let mats = fr;
    let pout = fr + 0x40;
    let pback = fr + 0x44;
    let pfront = fr + 0x48;
    let mut list = si_generate_face_list(m, o, mesh, 0, mats, mats);
    let mut head = 0u32;
    while list != 0 {
        let next = r(m, list + f::NEXT);
        w(m, list + f::PREV, 0);
        w(m, list + f::NEXT, 0);
        let mut cur = list;
        let mut i = 0i32;
        // `cmp ebx, n; jge`: with no plane the face goes straight to the output chain
        while i < n && cur != 0 {
            let node = tree + i as u32 * 0x30;
            let side = which_side(m, cur, node);
            if side == 3 {
                cur = 0;
            } else if side == 4 {
                crate::split::si_split_face(m, o, node, cur, pback, pfront);
                cur = r(m, pfront);
            }
            i += 1;
        }
        if cur != 0 {
            if head != 0 {
                w(m, cur + f::NEXT, head);
                w(m, head + f::PREV, cur);
            }
            w(m, cur + f::PREV, 0);
            head = cur;
        }
        list = next;
    }
    let res = si_build_mesh(m, o, pout, head) as i16;
    if res == 0 && call(m, o, GETENV_ROUTINE, &[ENV_DOPLC]) == 0 {
        call(m, o, SI_MESH_DISPOSE, &[pout]);
        w(m, pout, 0);
    }
    let out = r(m, pout);
    if out != 0 {
        carry_mesh_fields(m, out, mesh);
    }
    si_release_mem(m);
    m.release(0x50);
    out
}

const GETENV_ROUTINE: Routine = Routine::new("getenv", 0x080c_3bb8);

/// `SI_meshCutPlanes(mesh, min, max, nx, ny, nz)` [soft 0x08316ba0 SI_meshCutPlanes]: cuts the mesh with `nx`, `ny`
/// and `nz` planes evenly spread between `min` and `max` on the three axes (counts are shorts; `min` and `max`
/// are three floats each). The mesh is first ordered (`SI_orderMesh`, `SI_meshUpdate`) unless `SI_validMesh`
/// accepts it. Each axis walks its planes in turn: a face in front (3) of a plane goes on to the next plane (on
/// the last plane to the output chain), a face on or behind (1, 2) goes to the output chain, a face on the
/// outside (0) is dropped, and a split face sends its first piece (`*pback`) on (or, on the last plane, to the
/// output) and its second (`*pfront`) to the output. The chains are then built into a mesh, `si_buildMesh2` unless
/// `SI_OLD_SUBDIVISION` is set. The offset of plane k is computed in the x87 in 80 bits
/// (`pos += (max - min) / (n + 1)`, stored negated as a double in the template's `d` slot); an axis with a
/// negative count makes no planes, and a total of zero or less allocates none (the original would then read an
/// unset pointer if another axis had planes: here 0).
pub fn si_mesh_cut_planes(m: &mut Mem, o: &mut dyn Outside, mesh: u32, min: [f32; 3], max: [f32; 3], counts: [i16; 3]) -> u32 {
    let work;
    if call(m, o, SI_VALID_MESH, &[mesh, 0]) & 0xffff == 0 {
        work = call(m, o, SI_ORDER_MESH, &[mesh]);
        if work == 0 {
            return 0;
        }
        call(m, o, SI_MESH_UPDATE, &[work]);
    } else {
        work = mesh;
    }
    let cnt = [counts[0] as i32, counts[1] as i32, counts[2] as i32];
    let total = cnt[1] + cnt[0] + cnt[2];
    set_merge_dist(m, PLANE_MERGE_DIST);
    init_boolean(m);
    let fr = m.local(0x80);
    let mats = fr; // 0x40 bytes, uninitialised in the original
    let pout = fr + 0x40;
    let pback = fr + 0x44;
    let pfront = fr + 0x48;
    let wmesh = fr + 0x4c; // the working mesh as a variable (SI_meshDispose takes its address)
    w(m, wmesh, work);
    let mut planes = 0u32;
    if total > 0 {
        planes = si_get_mem(m, o, total.wrapping_mul(0x30));
        let mut k = 0u32;
        // axis x: template (1,0,0,d), y: (0,1,0,d), z: (0,0,1,d); the d slot is a global written before each copy
        let axes: [(u32, [f64; 3]); 3] = [
            (CUT_PLANE_D_X.addr(), [1.0, 0.0, 0.0]),
            (CUT_PLANE_D_Y.addr(), [0.0, 1.0, 0.0]),
            (CUT_PLANE_D_Z.addr(), [0.0, 0.0, 1.0]),
        ];
        for (a, (dslot, nrm)) in axes.iter().enumerate() {
            let n = cnt[a];
            let lo = x(min[a]);
            let hi = x(max[a]);
            let step = (hi - lo) / F80::from_i32(n.wrapping_add(1));
            let mut pos = lo;
            for _ in 0..n.max(0) {
                pos = pos + step;
                m.set_f64(*dslot, (-pos).to_f64());
                let at = planes + k * 0x30 + 4;
                for (j, v) in nrm.iter().enumerate() {
                    m.set_f64(at + j as u32 * 8, *v);
                }
                let dv = m.f64(*dslot);
                m.set_f64(at + 0x18, dv);
                w(m, planes + k * 0x30 + 0x24, 0);
                k += 1;
            }
        }
    }
    set_classifying(m, 0);
    let mut list = si_generate_face_list(m, o, work, 0, mats, mats);
    let mut kplane = 0u32;
    for a in 0..3usize {
        let n = cnt[a];
        let mut head_a = 0u32;
        let mut tail_a = 0u32;
        let mut i = 0i32;
        if n > 0 {
            while i < n {
                let mut head_b = 0u32;
                let mut tail_b = 0u32;
                call(m, o, SI_SHOW_ACTIVITY, &[]);
                let last = i == n - 1;
                let mut face = list;
                while face != 0 {
                    let next = r(m, face + f::NEXT);
                    w(m, face + f::NEXT, 0);
                    w(m, face + f::PREV, 0);
                    let node = planes.wrapping_add(kplane * 0x30);
                    let side = which_side(m, face, node);
                    // append to the output chain / to the next-plane chain
                    macro_rules! to_a {
                        ($first:expr, $lastf:expr) => {{
                            if head_a == 0 {
                                head_a = $first;
                            } else {
                                w(m, tail_a + f::NEXT, $first);
                                w(m, $first + f::PREV, tail_a);
                            }
                            tail_a = $lastf;
                        }};
                    }
                    macro_rules! to_b {
                        ($first:expr, $lastf:expr) => {{
                            if head_b == 0 {
                                head_b = $first;
                            } else {
                                w(m, tail_b + f::NEXT, $first);
                                w(m, $first + f::PREV, tail_b);
                            }
                            tail_b = $lastf;
                        }};
                    }
                    if side == 3 {
                        if last {
                            to_a!(face, face);
                        } else {
                            to_b!(face, face);
                        }
                    } else if side == 4 {
                        crate::split::si_split_face(m, o, node, face, pback, pfront);
                        let o1 = r(m, pback);
                        if o1 != 0 {
                            let l1 = chain_last(m, o1);
                            if last {
                                to_a!(o1, l1);
                            } else {
                                to_b!(o1, l1);
                            }
                        }
                        let o2 = r(m, pfront);
                        if o2 != 0 {
                            let l2 = chain_last(m, o2);
                            to_a!(o2, l2);
                        }
                    } else if side >= 1 && side < 3 {
                        to_a!(face, face);
                    }
                    face = next;
                }
                list = head_b;
                i += 1;
                kplane += 1;
            }
            list = head_a;
        }
    }
    if call(m, o, GETENV_ROUTINE, &[ENV_OLD_SUBDIVISION]) != 0 {
        si_build_mesh(m, o, pout, list);
    } else {
        si_build_mesh2(m, o, work, pout, list);
    }
    let out = r(m, pout);
    if out != 0 {
        carry_mesh_fields(m, out, mesh);
    }
    si_release_mem(m);
    if work != mesh {
        call(m, o, SI_MESH_DISPOSE, &[wmesh]);
    }
    m.release(0x80);
    out
}

/// `si_copyModelMat(model, a, b)` [soft 0x0830bc30 si_copyModelMat]: replaces the materials of `model` (count at
/// `+0x206`, array at `+0x20c`) by copies of those of `a` followed by those of `b` (either may be 0); the copies'
/// four words at `+0xdb0` are cleared. The old materials are disposed first (`SI_edDisposeMaterial(m, 1)`, the
/// array freed). The count is a short that wraps on the sum.
pub fn si_copy_model_mat(m: &mut Mem, o: &mut dyn Outside, model: u32, a: u32, b: u32) {
    let cnt = |m: &Mem, md: u32| m.u16(md + 0x206) as i16 as i32;
    if cnt(m, model) > 0 {
        let arr = r(m, model + 0x20c);
        let mut i = 1i32;
        while i < cnt(m, model) {
            let mat = r(m, r(m, model + 0x20c) + i as u32 * 4);
            call(m, o, SI_ED_DISPOSE_MATERIAL, &[mat, 1]);
            i += 1;
        }
        let first = r(m, arr);
        call(m, o, SI_ED_DISPOSE_MATERIAL, &[first, 1]);
        let arr = r(m, model + 0x20c);
        if arr != 0 {
            m.free(arr);
            w(m, model + 0x20c, 0);
        }
        w(m, model + 0x20c, 0);
        m.set_u16(model + 0x206, 0);
    }
    for src in [a, b] {
        if src != 0 {
            let s = m.u16(model + 0x206).wrapping_add(m.u16(src + 0x206));
            m.set_u16(model + 0x206, s);
        }
    }
    if cnt(m, model) > 0 {
        let mut base = 0u32;
        let arr = m.si_calloc(cnt(m, model) as u32, 4);
        w(m, model + 0x20c, arr);
        let copy_all = |m: &mut Mem, o: &mut dyn Outside, src: u32, base: u32| -> u32 {
            let n = cnt(m, src) as u32; // the loop compares the unsigned count
            let mut i = 0u32;
            while i < n && m.fault().is_none() {
                let mat = call(m, o, SI_MATERIAL_COPY, &[r(m, r(m, src + 0x20c) + i * 4)]);
                let slot = r(m, model + 0x20c) + (base + i) * 4;
                w(m, slot, mat);
                if mat != 0 {
                    for off in [0xdb0, 0xdb4, 0xdb8, 0xdbc] {
                        w(m, mat + off, 0);
                    }
                }
                i += 1;
            }
            n
        };
        if a != 0 {
            if m.u16(a + 0x206) != 0 {
                base = copy_all(m, o, a, 0);
            } else {
                base = cnt(m, a) as u32;
            }
        }
        if b != 0 && m.u16(b + 0x206) != 0 {
            copy_all(m, o, b, base);
        }
    }
}

/// `SI_animBooleanUpdate(node, time)` [soft 0x0830a7b0 SI_animBooleanUpdate]: the update function of an animated
/// boolean node. `node+8` is the result model, `node+0x1c` the update node holding the two operand models (`+8`,
/// `+0x30`), `node+0x24` the parameters (short enable; `+4` operation 1 union, 2 intersection, 3 difference;
/// `+8` material mode 0..3; `+0xc`, `+0xe` the cleanup shorts; `+0x10` copy-materials flag). Answers 0 when any
/// of the four is missing, else 1. With the enable off nothing happens. Otherwise the result model's old
/// definition is dropped (remembering whether its first definition was not the copy kept at `+0x48`), the
/// materials are rebuilt (`si_copyModelMat` by mode, then `SI_dbConvertModelMatTxt2DB`), both operands become
/// meshes (`SI_modelConvertToMesh`) and: a missing/empty first operand with a non-empty second and operation 1
/// copies the second; a non-empty first alone with operation 1 or 3 copies the first; two non-empty ones run
/// `SI_computeBoolean` (setup: operation, output mode 0, the parameters' shorts, the product of the second's
/// matrix with the first's inverse and its transpose-inverse, and the material-count window by mode; a
/// non-planar operand is triangulated and forces the cleanup shorts to 1); anything else gives nothing. The
/// result is added as definition 4 (and copied to `+0x48` when it was remembered), the geometry touched, the
/// transformation of the first operand copied over, and the graf node updated. A failed boolean (-1) prints
/// "A fatal error has occurred. Operation aborted !!!". In the original the setup's `+8` word and `+0x10` word are
/// stack leftovers (+8 holds the material mode), the window words stay unset for a mode over 3 (here 0).
pub fn si_anim_boolean_update(m: &mut Mem, o: &mut dyn Outside, node: u32) -> u32 {
    let target = r(m, node + 8);
    let upd = r(m, node + 0x1c);
    let a = r(m, upd + 8);
    let b = r(m, upd + 0x30);
    let params = r(m, node + 0x24);
    if target == 0 || a == 0 || b == 0 || params == 0 {
        return 0;
    }
    if m.u16(params) == 0 {
        return 1;
    }
    let mut changed = false;
    let defs = r(m, target + 0x38);
    if defs != 0 {
        changed = r(m, defs + m.u16(target + 0x36) as u32 * 4) != r(m, target + 0x48);
        call(m, o, SI_MODEL_DELETE_DEFINITION, &[target, 0]);
        let gl = r(m, SI_GRAF_LISTS) + r(m, r(m, SI_PROC_DATA) + 4).wrapping_mul(0x24);
        let g = call(m, o, SI_FIND_GL_DATA, &[gl, target, 2]);
        call(m, o, SI_UPDATE_GN_FUNC, &[g]);
    }
    let mode = r(m, params + 8);
    let cnt = |m: &Mem, md: u32| m.u16(md + 0x206) as i16 as i32;
    let (add, limit) = match mode {
        1 => (0, cnt(m, a)),
        2 => (0, cnt(m, b)),
        3 => (cnt(m, a), cnt(m, a) + cnt(m, b)),
        _ => (0, 0),
    };
    if m.u16(params + 0x10) != 0 {
        match mode {
            0 => si_copy_model_mat(m, o, target, 0, 0),
            1 => si_copy_model_mat(m, o, target, a, 0),
            2 => si_copy_model_mat(m, o, target, b, 0),
            3 => si_copy_model_mat(m, o, target, a, b),
            _ => {}
        }
        call(m, o, SI_DB_CONVERT_MODEL_MAT_TXT2DB, &[target, 0, 0, 0]);
    }
    let fr = m.local(0x10 + 0x9c + 0x40);
    let cell_a = fr;
    let cell_b = fr + 4;
    let cell_out_b = fr + 8;
    let cell_res = fr + 0xc;
    let setup = fr + 0x10;
    let inv = fr + 0x10 + 0x9c;
    w(m, cell_res, 0);
    w(m, cell_out_b, 0);
    let ma = call(m, o, SI_MODEL_CONVERT_TO_MESH, &[a]);
    w(m, cell_a, ma);
    let mb = call(m, o, SI_MODEL_CONVERT_TO_MESH, &[b]);
    w(m, cell_b, mb);
    let polys = |m: &Mem, mesh: u32| mesh != 0 && r(m, mesh + 0x18) != 0;
    let dispose = |m: &mut Mem, o: &mut dyn Outside, cell: u32| {
        call(m, o, SI_MESH_DISPOSE, &[cell]);
    };
    let op = r(m, params + 4);
    let mut done = false;
    if !polys(m, ma) && !polys(m, mb) {
        // both empty: dispose what exists, answer 0
        if ma != 0 {
            dispose(m, o, cell_a);
        }
        if mb != 0 {
            dispose(m, o, cell_b);
        }
        m.release(0x10 + 0x9c + 0x40);
        return 0;
    }
    if !polys(m, ma) && polys(m, mb) && op == 1 {
        let c = call(m, o, SI_MESH_COPY, &[mb]);
        w(m, cell_res, c);
        if ma != 0 {
            dispose(m, o, cell_a);
        }
        dispose(m, o, cell_b);
        done = true;
    } else if ma == 0 {
        if mb != 0 {
            dispose(m, o, cell_b);
        }
        m.release(0x10 + 0x9c + 0x40);
        return 0;
    } else if polys(m, ma) && !polys(m, mb) && (op == 1 || op == 3) {
        let c = call(m, o, SI_MESH_COPY, &[ma]);
        w(m, cell_res, c);
        dispose(m, o, cell_a);
        if mb != 0 {
            dispose(m, o, cell_b);
        }
        done = true;
    }
    if !done {
        if !polys(m, ma) || !polys(m, mb) {
            dispose(m, o, cell_a);
            if mb != 0 {
                dispose(m, o, cell_b);
            }
            m.release(0x10 + 0x9c + 0x40);
            return 0;
        }
        w(m, setup, op);
        w(m, setup + 4, 0);
        w(m, setup + 8, mode);
        let c1 = m.u16(params + 0xc);
        let c2 = m.u16(params + 0xe);
        m.set_u16(setup + 0xc, c1);
        m.set_u16(setup + 0xe, c2);
        m.set_u16(setup + 0x10, 0);
        m.set_i32(setup + 0x94, add);
        m.set_i32(setup + 0x98, limit);
        let ma_mat = r(m, a + 0x44) + 0x1c4;
        let mb_mat = r(m, b + 0x44) + 0x1c4;
        call(m, o, SI_MATRIX_INVERSE, &[ma_mat, inv]);
        call(m, o, SI_MATRIX_MULTIPLY, &[mb_mat, inv, setup + 0x14]);
        call(m, o, SI_MATRIX_TRANSPOSE_INVERSE, &[setup + 0x14, setup + 0x54]);
        let planar = |m: &mut Mem, o: &mut dyn Outside, mesh: u32| {
            if call(m, o, crate::mesh::SI_MESH_PLANAR, &[mesh]) & 0xffff == 0 {
                call(m, o, crate::mesh::SI_MESH_TRIANGULATE, &[mesh, 0]);
                m.set_u16(setup + 0xc, 1);
                m.set_u16(setup + 0xe, 1);
            }
        };
        planar(m, o, ma);
        planar(m, o, mb);
        let rc = si_compute_boolean(m, o, setup, ma, mb, cell_res, cell_out_b);
        if rc == 0xffff_ffff {
            call(m, o, SI_STATUS_DISPLAY_MESSAGE, &[MSG_FATAL, 2]);
        }
        dispose(m, o, cell_a);
        dispose(m, o, cell_b);
        if r(m, cell_out_b) != 0 {
            dispose(m, o, cell_out_b);
        }
    }
    let res = r(m, cell_res);
    if res != 0 {
        call(m, o, SI_MODEL_ADD_DEFINITION, &[target, 4, res]);
        if changed {
            let c = call(m, o, SI_MESH_COPY, &[res]);
            w(m, target + 0x48, c);
        }
    }
    call(m, o, SI_MODEL_TOUCH_GEOMETRY, &[target]);
    let amat = r(m, a + 0x44);
    call(m, o, SI_MODEL_COPY_TRANSFORMATION, &[amat, target]);
    let gl = r(m, SI_GRAF_LISTS) + r(m, r(m, SI_PROC_DATA) + 4).wrapping_mul(0x24);
    let g = call(m, o, SI_FIND_GL_DATA, &[gl, target, 2]);
    call(m, o, SI_UPDATE_GN_FUNC, &[g]);
    m.release(0x10 + 0x9c + 0x40);
    1
}

/// The six constructors of `bool_util.c` (0x083170d0 .. 0x08317234), ported in `util.rs`, as the hook the checks and
/// the render host use: `addr` selects the constructor. `None` for any other address.
pub fn bool_util_stand_in(m: &mut Mem, addr: u32, args: &[u32]) -> Option<u32> {
    struct Alloc;
    impl Outside for Alloc {
        fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> si_mem::Ret {
            si_mem::Ret::eax(if r.addr == 0x080d_0ee8 { m.si_malloc(a[0]) } else { 0 })
        }
    }
    use crate::util as u;
    let a = |k: usize| args.get(k).copied().unwrap_or(0);
    Some(match addr {
        0x0831_70ec => u::si_create_edge_node(m, &mut Alloc),
        0x0831_7118 => u::si_create_contour(m, &mut Alloc, a(0)),
        0x0831_7144 => u::si_create_bool_face(m, &mut Alloc, a(0)),
        0x0831_70d0 => u::si_create_face_group(m, &mut Alloc),
        0x0831_7208 => u::si_create_reg_bsp_node(m, &mut Alloc, a(0)),
        0x0831_7234 => u::si_create_face_list(m, &mut Alloc, a(0), a(1)),
        _ => return None,
    })
}
