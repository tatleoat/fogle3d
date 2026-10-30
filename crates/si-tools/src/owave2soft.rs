// SPDX-License-Identifier: MIT
//! `owave2soft.c` on the image (finding 0039): the Wavefront `.obj` reader and the builder
//! that turns what it read into Softimage models.
//!
//! The records, each read from the listings (nothing is declared by the original's headers
//! in what we have):
//!
//! - a **model entry** (`wavefront`, 0x1c bytes, `SI_malloc`): the header it shares
//!   ([`hdr`]), its number in the file's list (from 1), a copy of the header's count, its
//!   kind (0 none, 1 mesh, 2 patch, 3 point, 4 curve-ish, 5 line; set once by `setMdlType`),
//!   its data, a small side record, and the next entry;
//! - the **header** (0x60 bytes, one for the whole list): the file's name, the first entry,
//!   the number of entries and the two vertex lists (positions, normals);
//! - a **vertex list block** (`vl`, 0x3e88 bytes): 1000 vertices of 16 bytes, the count of
//!   the whole list (kept in every block) at +16000 and the next block at +0x3e84;
//! - an **element block** (`element_data`, 0x1f48 bytes): 1000 pairs (vertex number, normal
//!   number) of 8 bytes, the count at +8000 and the next block at +0x1f44;
//! - a **line node** (8 bytes): a vertex number and the next node (kind 5 entries);
//! - a **group name** (`group_name`, 100 bytes) and a **smoothing group** (`smooth_list`,
//!   0x5c bytes): an 80-byte name, a block of entry numbers, their count, and (group names)
//!   the model made for the group and its parent, then the next record;
//! - a **material** (0xdcc bytes) whose name is its first bytes and whose next is at +0x6e4.

use crate::stdio::{self, StdioCalls};
use si_mem::clib;
use si_mem::{outside_calls, Mem, Outside};

outside_calls! {
    /// What `owave2soft.c` calls in other files and nobody has ported here (or that lives in a
    /// crate this one does not depend on).
    pub trait ToolsCalls {
        /// `si_printMessage (0x081b92b4)`, `omovie2soft.c`, a variadic `vsprintf` into a 1,024
        /// byte buffer and `SI_statusDisplayMessage`: (level, format). The arguments the
        /// format takes follow as extra words in [`print_message`].
        fn si_print_message(level: i32, text: u32) = "si_printMessage", 0x081b_92b4;
        /// `SI_materialTransfer (0x08284504)`, `mat_dial.c`: (source, destination, flag).
        fn si_material_transfer(src: u32, dst: u32, flag: i32) = "SI_materialTransfer", 0x0828_4504;
        /// `fgets(buffer, size, stream)` (C library): the buffer, or null at the end.
        fn fgets(buf: u32, size: i32, stream: u32) -> u32 = "fgets", 0x080c_4388;
        /// `fclose(stream)` (C library).
        fn fclose(stream: u32) -> i32 = "fclose", 0x080c_4958;
        /// `feof(stream)` (C library).
        fn feof(stream: u32) -> i32 = "feof", 0x080c_3948;
        /// `si_modelSetDefinition (0x081b8540)`, `omovie2soft.c`: (node, kind, definition).
        fn si_model_set_definition(model: u32, kind: i32, def: u32) = "si_modelSetDefinition", 0x081b_8540;
        /// `SI_meshAllocate (0x08458c20)`.
        fn si_mesh_allocate() -> u32 = "SI_meshAllocate", 0x0845_8c20;
        /// `SI_meshDispose (0x084404a0)`: (pointer to the mesh).
        fn si_mesh_dispose(pmesh: u32) = "SI_meshDispose", 0x0844_04a0;
        /// `SI_vertexAllocate (0x08459d48)`: an array of that many 0x14-byte vertices.
        fn si_vertex_allocate(n: i32) -> u32 = "SI_vertexAllocate", 0x0845_9d48;
        /// `SI_polygonAllocate (0x08459e20)`: an array of that many 0x18-byte polygons.
        fn si_polygon_allocate(n: i32) -> u32 = "SI_polygonAllocate", 0x0845_9e20;
        /// `SI_meshUpdate (0x08458d94)`.
        fn si_mesh_update(mesh: u32) = "SI_meshUpdate", 0x0845_8d94;
        /// `SI_meshInitVertexColors (0x08458bc8)`.
        fn si_mesh_init_vertex_colors(mesh: u32) = "SI_meshInitVertexColors", 0x0845_8bc8;
        /// `SI_orderMesh (0x08479494)`: the mesh with its polygons put in order, as a new mesh.
        fn si_order_mesh(mesh: u32) -> u32 = "SI_orderMesh", 0x0847_9494;
        /// `SI_maxNbPolWarning (0x08448afc)`.
        fn si_max_nb_pol_warning() = "SI_maxNbPolWarning", 0x0844_8afc;
        /// `SI_patchAllocate (0x0847a0d0)`.
        fn si_patch_allocate() -> u32 = "SI_patchAllocate", 0x0847_a0d0;
        /// `SI_patchDispose (0x0847a530)`: (pointer to the patch).
        fn si_patch_dispose(ppatch: u32) = "SI_patchDispose", 0x0847_a530;
        /// `SI_patchUpdateMatrix (0x0847a680)`.
        fn si_patch_update_matrix(patch: u32) = "SI_patchUpdateMatrix", 0x0847_a680;
        /// `SI_splineAllocate (0x0848a990)`.
        fn si_spline_allocate() -> u32 = "SI_splineAllocate", 0x0848_a990;
        /// `SI_splineKeyAllocate (0x0848f8dc)`: an array of that many 0x30-byte keys.
        fn si_spline_key_allocate(n: u32) -> u32 = "SI_splineKeyAllocate", 0x0848_f8dc;
        /// `SI_splineDispose (0x0848ab48)`: (pointer to the spline).
        fn si_spline_dispose(pspline: u32) = "SI_splineDispose", 0x0848_ab48;
        /// `SI_splineUpdateMatrix (0x0848abd0)`.
        fn si_spline_update_matrix(spline: u32) = "SI_splineUpdateMatrix", 0x0848_abd0;
        /// `SI_modelAllocate (0x08477cf4)`, `model_a.c`: a node (name or null).
        fn si_model_allocate(name: u32) -> u32 = "SI_modelAllocate", 0x0847_7cf4;
        /// `SI_modelCreateParentship (0x08477c90)`: (parent node, child node).
        fn si_model_create_parentship(parent: u32, child: u32) = "SI_modelCreateParentship", 0x0847_7c90;
        /// `SI_modelDispose (0x08461920)`: (pointer to the node, flag).
        fn si_model_dispose(pnode: u32, flag: i32) = "SI_modelDispose", 0x0846_1920;
        /// `SI_modelDisposeNode (0x08461a54)`: (pointer to the node).
        fn si_model_dispose_node(pnode: u32) = "SI_modelDisposeNode", 0x0846_1a54;
        /// `SI_modelNextNode (0x08478340)`: the node after this one in the walk of the tree.
        fn si_model_next_node(node: u32) -> u32 = "SI_modelNextNode", 0x0847_8340;
        /// `SI_materialCopy (0x08284888)`: a copy of the material, as a new block.
        fn si_material_copy(mat: u32) -> u32 = "SI_materialCopy", 0x0828_4888;
        /// `SI_materialDispose (0x08284900)`: (node, pointer to the node's material array).
        fn si_material_dispose(node: u32, parr: u32) = "SI_materialDispose", 0x0828_4900;
        /// `SI_meshComputesVertexNormal (0x08459e8c)`, `mesh_b.c`: (mesh, polygon or -1).
        fn si_mesh_computes_vertex_normal(mesh: u32, which: i32) = "SI_meshComputesVertexNormal", 0x0845_9e8c;
    }
}

/// The routine and address of `si_printMessage`, for the variadic form.
const PRINT_MESSAGE: si_mem::Routine = si_mem::Routine::new("si_printMessage", 0x081b_92b4);

/// `si_printMessage (0x081b92b4)` with the words its format takes after the format.
pub fn print_message(m: &mut Mem, o: &mut dyn Outside, level: i32, text: u32, extra: &[u32]) {
    let mut words = vec![level as u32, text];
    words.extend_from_slice(extra);
    o.call(m, PRINT_MESSAGE, &words);
}

/// The routine and address of `fscanf`, for the variadic form.
const FSCANF: si_mem::Routine = si_mem::Routine::new("fscanf", 0x080c_4058);

/// `fscanf (0x080c4058)` (C library) with the addresses its format stores through; what it
/// returned (`-1` at the end of the stream before a conversion).
pub fn fscanf(m: &mut Mem, o: &mut dyn Outside, file: u32, format: u32, stores: &[u32]) -> i32 {
    let mut words = vec![file, format];
    words.extend_from_slice(stores);
    o.call(m, FSCANF, &words).eax as i32
}

/// A model entry.
pub mod wf {
    pub const HDR: u32 = 0x00;
    pub const INDEX: u32 = 0x04;
    pub const COUNT: u32 = 0x08;
    pub const KIND: u32 = 0x0c;
    pub const DATA: u32 = 0x10;
    pub const AUX: u32 = 0x14;
    pub const NEXT: u32 = 0x18;
    pub const SIZE: u32 = 0x1c;
}

/// The header the entries of a file share.
pub mod hdr {
    pub const NAME: u32 = 0x00;
    pub const FIRST: u32 = 0x50;
    pub const COUNT: u32 = 0x54;
    pub const VERTS: u32 = 0x58;
    pub const NORMALS: u32 = 0x5c;
    pub const SIZE: u32 = 0x60;
}

/// Vertex list blocks.
pub mod vl {
    pub const PER_BLOCK: i32 = 1000;
    pub const STRIDE: u32 = 0x10;
    pub const COUNT: u32 = 16000;
    pub const NEXT: u32 = 0x3e84;
    pub const SIZE: u32 = 0x3e88;
}

/// Element blocks.
pub mod elem {
    pub const PER_BLOCK: i32 = 1000;
    pub const STRIDE: u32 = 8;
    pub const COUNT: u32 = 8000;
    pub const NEXT: u32 = 0x1f44;
    pub const SIZE: u32 = 0x1f48;
}

/// Group names and smoothing groups: name (80 bytes), the numbers, their count; then for a
/// group name the model, the parent model and the next (0x58, 0x5c, 0x60); for a smoothing
/// group the next (0x58).
pub mod grp {
    pub const LIST: u32 = 0x50;
    pub const COUNT: u32 = 0x54;
    pub const MODEL: u32 = 0x58;
    pub const PARENT: u32 = 0x5c;
    pub const NEXT: u32 = 0x60;
    pub const SIZE: u32 = 100;
    pub const SMOOTH_NEXT: u32 = 0x58;
    pub const SMOOTH_SIZE: u32 = 0x5c;
}

/// A name list node: an 80-byte name and the next at +0x50.
pub const NAME_NEXT: u32 = 0x50;
/// A material: the next at +0x6e4, 0xdcc bytes, `SI_defaultMaterial` the template.
pub const MAT_NEXT: u32 = 0x6e4;
pub const MAT_SIZE: u32 = 0xdcc;
pub const SI_DEFAULT_MATERIAL: u32 = 0x0869_dde0;

const MSG_OOM_READ_LINE_A: u32 = 0x0869_1c46;
const MSG_OOM_READ_LINE_B: u32 = 0x0869_1c55;
const MSG_OOM_ADD_LIST_DATA: u32 = 0x0869_1dfd;
const MSG_OOM_ADD_VERTEX_A: u32 = 0x0869_1e0c;
const MSG_OOM_ADD_VERTEX_B: u32 = 0x0869_1e1b;
const MSG_OOM_COPY_MATERIAL: u32 = 0x0869_1f9f;
const MSG_NOT_FOUND_MTL: u32 = 0x0869_1f80;
const MSG_OOM_ADD_MATERIAL: u32 = 0x0869_200d;
const MSG_MATERIAL_NOT_FOUND: u32 = 0x0869_1fe0;
const MSG_ASSUMING_DEFAULT_WHITE: u32 = 0x0869_1fbc;
const MSG_OOM_DEFAULT_WF_MATERIAL: u32 = 0x0869_201c;
const MSG_OOM_UPDATE_GROUP_A: u32 = 0x0869_2039;
const MSG_OOM_UPDATE_GROUP_B: u32 = 0x0869_2048;
const MSG_OOM_UPDATE_GROUP_C: u32 = 0x0869_2057;
const MSG_OOM_UPDATE_SMOOTH_A: u32 = 0x0869_20b1;
const MSG_OOM_UPDATE_SMOOTH_B: u32 = 0x0869_20c0;
const MSG_OOM_UPDATE_SMOOTH_C: u32 = 0x0869_20cf;
const MSG_OOM_ADD_NEW_MDL_A: u32 = 0x0869_20de;
const MSG_OOM_ADD_NEW_MDL_B: u32 = 0x0869_20ed;
const MSG_OOM_COPY_LINE_A: u32 = 0x0869_20fc;
const MSG_OOM_COPY_LINE_B: u32 = 0x0869_210b;
const MSG_OOM_POINT: [u32; 2] = [0x0869_1d43, 0x0869_1d52];
const MSG_OOM_PATCH: [u32; 5] = [0x0869_1d61, 0x0869_1d70, 0x0869_1d7f, 0x0869_1d8e, 0x0869_1db2];
const MSG_OOM_LINE: [u32; 2] = [0x0869_1dc1, 0x0869_1dd0];
const MSG_OOM_CURVE: [u32; 2] = [0x0869_1ddf, 0x0869_1dee];
/// `convertMesh`'s messages: out of memory (mesh, polygons, polygon vertices, vertices),
/// "vertex #%d not found.", "vertex normal #%d not found.", "Ordering mesh...".
const MSG_MESH: [u32; 7] = [0x0869_1cc3, 0x0869_1cd2, 0x0869_1ce1, 0x0869_1d23, 0x0869_1cf0, 0x0869_1d06, 0x0869_1d32];
/// `SI_nbPolMax`, a global of the program: the most polygons a mesh may have.
pub const SI_NB_POL_MAX: u32 = 0x086d_3920;
/// `cleanGroupName`'s messages, all "Out of memory.".
const MSG_CGN: [u32; 5] = [0x0869_2066, 0x0869_2075, 0x0869_2084, 0x0869_2093, 0x0869_20a2];
/// `cleanHierarchy`'s name field of a material compared to "default_white".

/// `setMdlType`'s messages by kind, two for each (the data block, the side record).
const MSG_OOM_SET_TYPE: [[u32; 2]; 5] = [
    [0x0869_2138, 0x0869_2147],
    [0x0869_211a, 0x0869_2129],
    [0x0869_2156, 0x0869_2165],
    [0x0869_2174, 0],
    [0x0869_2183, 0x0869_2192],
];
/// "default_white\0", `.rodata` 0x08691fae (also 0x08691cb5, 0x0869202b, 0x086919e7).
pub const DEFAULT_WHITE: &[u8; 14] = b"default_white\0";

/// "Out of memory." and out.
fn oom(m: &mut Mem, o: &mut dyn Outside, msg: u32) {
    o.si_print_message(m, 3, msg);
}

// ---------------------------------------------------------------- disposal

/// A chain walked by `next` offset and freed, the pattern of the five dispose routines of
/// the file: read the next link, free the node, clear the head, take the next as head.
fn dispose_chain(m: &mut Mem, pp: u32, next: u32) {
    let mut p = m.u32(pp);
    while p != 0 {
        let cur = m.u32(pp);
        p = m.u32(cur.wrapping_add(next));
        if cur != 0 {
            m.free(cur);
            m.set_u32(pp, 0);
        }
        m.set_u32(pp, p);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x081c64a8 si_disposeMaterial]: the chain of materials at `*pp` freed.
pub fn si_dispose_material(m: &mut Mem, pp: u32) {
    dispose_chain(m, pp, MAT_NEXT);
}

/// [soft 0x081c63fc si_disposeNameList]: the chain of name nodes at `*pp` freed.
pub fn si_dispose_name_list(m: &mut Mem, pp: u32) {
    dispose_chain(m, pp, NAME_NEXT);
}

/// [soft 0x081c64f0 disposeVertexList]: the blocks of a vertex list freed.
pub fn dispose_vertex_list(m: &mut Mem, pp: u32) {
    dispose_chain(m, pp, vl::NEXT);
}

/// [soft 0x081c6538 disposeData]: the blocks of an element list freed.
pub fn dispose_data(m: &mut Mem, pp: u32) {
    dispose_chain(m, pp, elem::NEXT);
}

/// [soft 0x081c6580 disposeDataLine]: the nodes of a line (kind 5) list freed.
pub fn dispose_data_line(m: &mut Mem, pp: u32) {
    let mut p = m.u32(pp);
    while p != 0 {
        let next = m.u32(p.wrapping_add(4));
        m.free(p);
        p = next;
    }
    m.set_u32(pp, 0);
}

/// [soft 0x081c6384 resetModel]: the data and side record of an entry freed, its kind 0.
pub fn reset_model(m: &mut Mem, w: u32) {
    let line = m.i32(w + wf::KIND) == 5;
    let aux = m.u32(w + wf::AUX);
    if aux != 0 {
        m.free(aux);
        m.set_u32(w + wf::AUX, 0);
    }
    if m.u32(w + wf::DATA) != 0 {
        if line {
            dispose_data_line(m, w + wf::DATA);
        } else {
            dispose_data(m, w + wf::DATA);
        }
    }
    m.set_i32(w + wf::KIND, 0);
}

/// [soft 0x081c6444 disposeGroupName]: the group names at `*pp` freed, each with its block of
/// numbers.
pub fn dispose_group_name(m: &mut Mem, pp: u32) {
    let mut g = m.u32(pp);
    while g != 0 {
        let cur = m.u32(pp);
        g = m.u32(cur.wrapping_add(grp::NEXT));
        let list = m.u32(cur.wrapping_add(grp::LIST));
        if list != 0 {
            m.free(list);
            let cur = m.u32(pp);
            m.set_u32(cur.wrapping_add(grp::LIST), 0);
        }
        let cur = m.u32(pp);
        if cur != 0 {
            m.free(cur);
            m.set_u32(pp, 0);
        }
        m.set_u32(pp, g);
    }
    m.set_u32(pp, 0);
}

/// [soft 0x081c65c0 disposeWFList]: the entries of the list at `*pp` freed with their data;
/// with `all` set also the shared header and the two vertex lists (the header is freed with
/// the first entry), and the walk goes on down the list (without it only the first entry).
pub fn dispose_wf_list(m: &mut Mem, pp: u32, all: i16) {
    let mut first = true;
    if m.u32(pp) != 0 {
        if all != 0 {
            let h = m.u32(m.u32(pp));
            dispose_vertex_list(m, h.wrapping_add(hdr::VERTS));
            let h = m.u32(m.u32(pp));
            dispose_vertex_list(m, h.wrapping_add(hdr::NORMALS));
        }
        let mut cur = m.u32(pp);
        while cur != 0 {
            cur = m.u32(m.u32(pp) + wf::NEXT);
            if first && all != 0 {
                let h = m.u32(m.u32(pp));
                if h != 0 {
                    m.free(h);
                    let w = m.u32(pp);
                    m.set_u32(w, 0);
                }
                first = false;
            }
            let w = m.u32(pp);
            let kind = m.i32(w + wf::KIND);
            let mut aux_too = false;
            if kind == 5 {
                dispose_data_line(m, w + wf::DATA);
                aux_too = true;
            } else if kind != 0 {
                dispose_data(m, w + wf::DATA);
                aux_too = true;
            }
            if aux_too {
                let w = m.u32(pp);
                let aux = m.u32(w + wf::AUX);
                if m.i32(w + wf::KIND) != 0 && aux != 0 {
                    m.free(aux);
                    m.set_u32(w + wf::AUX, 0);
                }
            }
            let w = m.u32(pp);
            if w != 0 {
                m.free(w);
                m.set_u32(pp, 0);
            }
            if all == 0 {
                cur = 0;
            }
            m.set_u32(pp, cur);
        }
    }
    m.set_u32(pp, 0);
}

// ---------------------------------------------------------------- lists

/// [soft 0x081c4894 getListAtPosition]: the pair at position `pos` of an element list, or
/// null when the list is empty or `pos` is beyond its count (a position equal to the count
/// is answered). The original walks `pos / 1000` blocks; a negative position of -1000 or
/// below would count up through the wrap of the 32-bit counter, which is not reproduced
/// (null is answered).
pub fn get_list_at_position(m: &Mem, list: u32, pos: i32) -> u32 {
    if list == 0 || m.i32(list + elem::COUNT) < pos {
        return 0;
    }
    let mut cur = list;
    let blocks = pos / elem::PER_BLOCK;
    if blocks < 0 {
        return 0;
    }
    for _ in 0..blocks {
        cur = m.u32(cur + elem::NEXT);
    }
    cur.wrapping_add(((pos % elem::PER_BLOCK) * elem::STRIDE as i32) as u32)
}

/// [soft 0x081c48ec getVertexAtPosition]: the vertex at position `pos` of a vertex list (as
/// [`get_list_at_position`]).
pub fn get_vertex_at_position(m: &Mem, list: u32, pos: i32) -> u32 {
    if list == 0 || m.i32(list + vl::COUNT) < pos {
        return 0;
    }
    let mut cur = list;
    let blocks = pos / vl::PER_BLOCK;
    if blocks < 0 {
        return 0;
    }
    for _ in 0..blocks {
        cur = m.u32(cur + vl::NEXT);
    }
    cur.wrapping_add(((pos % vl::PER_BLOCK) * vl::STRIDE as i32) as u32)
}

/// [soft 0x081c4940 addListData]: appends the pair (`a`, `b`) to the entry's element list,
/// growing it by a block when the count is a multiple of 1000, and counts it in every
/// block. 0 when the entry is null or memory is out.
pub fn add_list_data(m: &mut Mem, o: &mut dyn Outside, w: u32, a: i32, b: i32) -> u32 {
    if w == 0 {
        return 0;
    }
    let head = m.u32(w + wf::DATA);
    let count = m.i32(head + elem::COUNT);
    let blocks = count / elem::PER_BLOCK;
    let mut cur = head;
    for _ in 0..blocks.max(0) {
        if m.u32(cur + elem::NEXT) == 0 {
            let n = m.si_malloc(elem::SIZE);
            if n == 0 {
                oom(m, o, MSG_OOM_ADD_LIST_DATA);
                return 0;
            }
            let c = m.u32(cur + elem::COUNT);
            m.set_u32(n + elem::COUNT, c);
            m.set_u32(n + elem::NEXT, 0);
            m.set_u32(cur + elem::NEXT, n);
        }
        cur = m.u32(cur + elem::NEXT);
    }
    let off = ((count % elem::PER_BLOCK) * elem::STRIDE as i32) as u32;
    m.set_i32(cur.wrapping_add(off), a);
    m.set_i32(cur.wrapping_add(off).wrapping_add(4), b);
    let mut p = head;
    while p != 0 {
        let c = m.i32(p + elem::COUNT);
        m.set_i32(p + elem::COUNT, c.wrapping_add(1));
        p = m.u32(p + elem::NEXT);
    }
    1
}

/// [soft 0x081c4a34 addVertex]: appends the four words at `v` to the vertex list at `*pl`,
/// making the list's first block when it is empty. 0 when memory is out.
pub fn add_vertex(m: &mut Mem, o: &mut dyn Outside, pl: u32, v: u32) -> u32 {
    if m.u32(pl) == 0 {
        let p = m.si_malloc(vl::SIZE);
        m.set_u32(pl, p);
        if p == 0 {
            oom(m, o, MSG_OOM_ADD_VERTEX_A);
            return 0;
        }
        m.set_u32(p + vl::COUNT, 0);
        m.set_u32(p + vl::NEXT, 0);
    }
    let head = m.u32(pl);
    let count = m.i32(head + vl::COUNT);
    let blocks = count / vl::PER_BLOCK;
    let mut cur = head;
    for _ in 0..blocks.max(0) {
        if m.u32(cur + vl::NEXT) == 0 {
            let n = m.si_malloc(vl::SIZE);
            if n == 0 {
                oom(m, o, MSG_OOM_ADD_VERTEX_B);
                return 0;
            }
            let c = m.u32(cur + vl::COUNT);
            m.set_u32(n + vl::COUNT, c);
            m.set_u32(n + vl::NEXT, 0);
            m.set_u32(cur + vl::NEXT, n);
        }
        cur = m.u32(cur + vl::NEXT);
    }
    let off = ((count % vl::PER_BLOCK) * vl::STRIDE as i32) as u32;
    for k in 0..4 {
        let w = m.u32(v.wrapping_add(4 * k));
        m.set_u32(cur.wrapping_add(off).wrapping_add(4 * k), w);
    }
    let mut p = m.u32(pl);
    while p != 0 {
        let c = m.i32(p + vl::COUNT);
        m.set_i32(p + vl::COUNT, c.wrapping_add(1));
        p = m.u32(p + vl::NEXT);
    }
    1
}

/// [soft 0x081c5bc4 findFather]: the first group name from `g` on whose numbers hold `n`, or null.
pub fn find_father(m: &Mem, n: i32, mut g: u32) -> u32 {
    while g != 0 {
        let count = m.i32(g + grp::COUNT);
        let list = m.u32(g + grp::LIST);
        let mut i = 0;
        while i < count {
            if m.i32(list.wrapping_add(4 * i as u32)) == n {
                return g;
            }
            i += 1;
        }
        g = m.u32(g + grp::NEXT);
    }
    0
}

/// [soft 0x081c5c04 compareInt]: the comparison `qsort` is given, on two ints in memory.
pub fn compare_int(m: &Mem, a: u32, b: u32) -> i32 {
    let (x, y) = (m.i32(a), m.i32(b));
    if y < x {
        1
    } else if x == y {
        0
    } else {
        -1
    }
}

/// The test the original makes through `__ctype_b` on a (signed) character: bit 2 of the table
/// entry, which for the C locale is set for the control characters 0 to 0x1f and 0x7f only
/// (bytes of 0x80 and above are not controls).
fn is_cntrl(c: u8) -> bool {
    c < 0x20 || c == 0x7f
}

/// One token of `readElement`: how many characters from `s` on. `slash_stops_first` is false
/// for the third token, whose first character is taken even when it is a `/`.
fn token_len(m: &Mem, s: u32, first_slash_ends: bool) -> u32 {
    let c = m.u8(s);
    if c == b' ' || c == 0 || (first_slash_ends && c == b'/') || is_cntrl(c) {
        return 0;
    }
    let mut n = 0;
    loop {
        n += 1;
        let c = m.u8(s.wrapping_add(n));
        if c == b' ' || c == 0 || c == b'/' || is_cntrl(c) {
            return n;
        }
    }
}

/// [soft 0x081c4b74 readElement]: reads the `v`, `v/vt` and `v/vt/vn` tokens at the start of
/// `s` one after the other and appends each as the pair (vertex number, normal number) to the
/// entry's element list; a negative number counts back from the end of the vertex (or
/// normal) list. The text read is cut off the front of `s` as it goes (blanks first). Stops at a
/// control character, the end of the text, a backslash (a continued line) or a token whose
/// vertex number is 0. Answers 0 when the text now starts with a backslash, else 1.
/// (The texture number between the slashes is read and dropped.)
pub fn read_element(m: &mut Mem, o: &mut dyn Outside, w: u32, s: u32) -> u32 {
    let buf = m.local(0x100);
    let r = read_element_in(m, o, w, s, buf);
    m.release(0x100);
    r
}

fn read_element_in(m: &mut Mem, o: &mut dyn Outside, w: u32, s: u32, buf: u32) -> u32 {
    loop {
        let mut skip = 0u32;
        // the original's test of ' ' through the table is a constant: blanks are skipped
        // up to the first character that is not one
        if m.u8(s) == b' ' {
            loop {
                skip += 1;
                if m.u8(s.wrapping_add(skip)) != b' ' {
                    break;
                }
            }
        }
        clib::strcpy(m, s, s.wrapping_add(skip));
        let c = m.u8(s);
        if c == b'\\' || c == 0 || is_cntrl(c) {
            return (m.u8(s) != b'\\') as u32;
        }
        let n = token_len(m, s, true);
        clib::strncpy(m, buf, s, n);
        m.set_u8(buf.wrapping_add(n), 0);
        clib::strcpy(m, s, s.wrapping_add(n));
        let mut a = clib::atoi(m, buf);
        if a < 0 {
            let h = m.u32(w);
            let v = m.u32(h + hdr::VERTS);
            a = a.wrapping_add(m.i32(v + vl::COUNT)).wrapping_add(1);
        }
        let mut third = 0;
        if m.u8(s) == b'/' {
            clib::strcpy(m, s, s.wrapping_add(1));
            let n = token_len(m, s, true);
            if n > 0 {
                clib::strncpy(m, buf, s, n);
                m.set_u8(buf.wrapping_add(n), 0);
                clib::strcpy(m, s, s.wrapping_add(n));
                let _ = clib::atoi(m, buf);
            }
            if m.u8(s) == b'/' {
                clib::strcpy(m, s, s.wrapping_add(1));
                let n = token_len(m, s, false);
                clib::strncpy(m, buf, s, n);
                m.set_u8(buf.wrapping_add(n), 0);
                clib::strcpy(m, s, s.wrapping_add(n));
                third = clib::atoi(m, buf);
                if third < 0 {
                    let h = m.u32(w);
                    let nrm = m.u32(h + hdr::NORMALS);
                    third = m.i32(nrm + vl::COUNT).wrapping_add(third).wrapping_add(1);
                }
            }
        }
        add_list_data(m, o, w, a, third);
        let c = m.u8(s);
        if a == 0 || c == b'\\' {
            return (c != b'\\') as u32;
        }
    }
}

// ---------------------------------------------------------------- entries

/// [soft 0x081c5e64 addNewMdl]: a new entry at the end of the list at `*pp`; the first one
/// makes the shared header (empty name, no vertices) and is the list. 0 when memory is out
/// (the entry already made is not freed).
pub fn add_new_mdl(m: &mut Mem, o: &mut dyn Outside, pp: u32) -> u32 {
    let w = m.si_malloc(wf::SIZE);
    if w == 0 {
        oom(m, o, MSG_OOM_ADD_NEW_MDL_A);
        return 0;
    }
    m.set_u32(w + wf::KIND, 0);
    m.set_u32(w + wf::NEXT, 0);
    let first = m.u32(pp);
    if first == 0 {
        m.set_u32(w + wf::INDEX, 1);
        let h = m.si_malloc(hdr::SIZE);
        m.set_u32(w + wf::HDR, h);
        if h == 0 {
            oom(m, o, MSG_OOM_ADD_NEW_MDL_B);
            return 0;
        }
        m.set_u32(h + hdr::NORMALS, 0);
        let h = m.u32(w);
        m.set_u32(h + hdr::VERTS, 0);
        m.set_u8(h, 0);
        m.set_u32(h + hdr::COUNT, 0);
        m.set_u32(h + hdr::FIRST, w);
        m.set_u32(pp, w);
    } else {
        let mut n = 2;
        let mut last = first;
        let mut next = m.u32(first + wf::NEXT);
        while next != 0 {
            last = m.u32(last + wf::NEXT);
            n += 1;
            next = m.u32(last + wf::NEXT);
        }
        let h = m.u32(first);
        m.set_u32(w + wf::HDR, h);
        m.set_u32(h + hdr::FIRST, w);
        m.set_u32(last + wf::NEXT, w);
        m.set_u32(w + wf::INDEX, n);
    }
    1
}

/// [soft 0x081c6110 setMdlType]: fixes the kind of an entry that has none and makes its data
/// block and side record: kinds 1 to 4 an element block (count 0, no next) and a side record
/// of 0x10, 8, 4 bytes or none; kind 5 a two-word line head and a 12-byte record; any other
/// kind leaves the entry as it is (answers 1). The entry's count word is a copy of the
/// header's. 0 when the entry has a kind already or memory is out (the kind stays set).
pub fn set_mdl_type(m: &mut Mem, o: &mut dyn Outside, w: u32, kind: i32) -> u32 {
    if m.i32(w + wf::KIND) != 0 {
        return 0;
    }
    if !(1..=5).contains(&kind) {
        return 1;
    }
    m.set_i32(w + wf::KIND, kind);
    let h = m.u32(w);
    let c = m.u32(h + hdr::COUNT);
    m.set_u32(w + wf::COUNT, c);
    let msgs = MSG_OOM_SET_TYPE[(kind - 1) as usize];
    if kind == 5 {
        let d = m.si_malloc(8);
        m.set_u32(w + wf::DATA, d);
        if d == 0 {
            oom(m, o, msgs[0]);
            return 0;
        }
        let a = m.si_malloc(0xc);
        m.set_u32(w + wf::AUX, a);
        if a == 0 {
            oom(m, o, msgs[1]);
            return 0;
        }
        m.set_u32(m.u32(w + wf::DATA), 0);
        let d = m.u32(w + wf::DATA);
        m.set_u32(d + 4, 0);
        m.set_u32(a, 0);
        m.set_u32(a + 4, 0);
        m.set_u32(a + 8, 0);
        return 1;
    }
    let d = m.si_malloc(elem::SIZE);
    m.set_u32(w + wf::DATA, d);
    if d == 0 {
        oom(m, o, msgs[0]);
        return 0;
    }
    match kind {
        1 | 2 | 3 => {
            let size = [0x10, 8, 4][(kind - 1) as usize];
            let a = m.si_malloc(size);
            m.set_u32(w + wf::AUX, a);
            if a == 0 {
                oom(m, o, msgs[1]);
                return 0;
            }
            if kind == 1 {
                m.set_u32(m.u32(w + wf::DATA) + elem::COUNT, 0);
                let d = m.u32(w + wf::DATA);
                m.set_u32(d + elem::NEXT, 0);
                m.set_u32(m.u32(w + wf::AUX), 0);
            } else if kind == 2 {
                m.set_u32(m.u32(w + wf::DATA) + elem::COUNT, 0);
                let d = m.u32(w + wf::DATA);
                m.set_u32(d + elem::NEXT, 0);
                m.set_u32(m.u32(w + wf::AUX), 0);
                let a = m.u32(w + wf::AUX);
                m.set_u32(a + 4, 0);
            } else {
                m.set_u32(d + elem::COUNT, 0);
                let d = m.u32(w + wf::DATA);
                m.set_u32(d + elem::NEXT, 0);
            }
        }
        _ => {
            // kind 4: no side record
            m.set_u32(w + wf::AUX, 0);
            m.set_u32(d + elem::COUNT, 0);
            let d = m.u32(w + wf::DATA);
            m.set_u32(d + elem::NEXT, 0);
        }
    }
    1
}

/// [soft 0x081c5f30 copyLine]: copies the pairs of `src`'s element list into the line list of
/// `dst`, from the second pair on unless `skip_first` is 0 (then from the first), in reverse
/// order when `dir` is -1. `mode` 1 appends after the last node; any other value pushes each
/// number in front of the nodes made so far and makes the last one made the list's head.
/// `dst`'s side record counts the numbers and keeps the first and the last. 0 when memory
/// is out.
pub fn copy_line(m: &mut Mem, o: &mut dyn Outside, dst: u32, src: u32, dir: i32, mode: i32, skip_first: i16) -> u32 {
    let mut node = m.u32(dst + wf::DATA);
    let start: i32 = if skip_first == 0 { 1 } else { 0 };
    let count0 = m.i32(m.u32(src + wf::DATA) + elem::COUNT);
    if mode == 1 {
        while m.u32(node + 4) != 0 {
            node = m.u32(node + 4);
        }
        let mut i = start;
        if i < count0 {
            loop {
                let k = if dir == -1 { count0 - 1 - i } else { i };
                let e = get_list_at_position(m, m.u32(src + wf::DATA), k);
                if m.u32(node) == 0 {
                    let v = m.u32(e);
                    m.set_u32(node, v);
                    let aux = m.u32(dst + wf::AUX);
                    m.set_u32(aux + 4, v);
                } else {
                    let n = m.si_malloc(8);
                    m.set_u32(node + 4, n);
                    if n == 0 {
                        oom(m, o, MSG_OOM_COPY_LINE_A);
                        return 0;
                    }
                    let v = m.u32(e);
                    m.set_u32(n, v);
                    m.set_u32(m.u32(node + 4) + 4, 0);
                    node = m.u32(node + 4);
                }
                let aux = m.u32(dst + wf::AUX);
                let v = m.u32(e);
                m.set_u32(aux + 8, v);
                let c = m.i32(m.u32(dst + wf::AUX));
                let aux = m.u32(dst + wf::AUX);
                m.set_i32(aux, c.wrapping_add(1));
                i += 1;
                if i >= m.i32(m.u32(src + wf::DATA) + elem::COUNT) {
                    break;
                }
            }
        }
    } else {
        let mut cur = node;
        let mut last = node;
        let mut i = start;
        if i < count0 {
            loop {
                let k = if dir == -1 { count0 - 1 - i } else { i };
                let e = get_list_at_position(m, m.u32(src + wf::DATA), k);
                if m.u32(cur) == 0 {
                    let v = m.u32(e);
                    m.set_u32(cur, v);
                    let aux = m.u32(dst + wf::AUX);
                    m.set_u32(aux + 4, v);
                    m.set_u32(aux + 8, v);
                    last = cur;
                } else {
                    last = m.si_malloc(8);
                    if last == 0 {
                        oom(m, o, MSG_OOM_COPY_LINE_B);
                        return 0;
                    }
                    let v = m.u32(e);
                    m.set_u32(last, v);
                    m.set_u32(last + 4, cur);
                    let aux = m.u32(dst + wf::AUX);
                    m.set_u32(aux + 4, v);
                }
                let aux = m.u32(dst + wf::AUX);
                let c = m.i32(aux);
                m.set_i32(aux, c.wrapping_add(1));
                i += 1;
                cur = last;
                if i >= m.i32(m.u32(src + wf::DATA) + elem::COUNT) {
                    break;
                }
            }
        }
        m.set_u32(dst + wf::DATA, last);
    }
    1
}

// ---------------------------------------------------------------- names and groups

/// Adds `n` to the numbers of a record whose list is at `LIST` and count at `COUNT`:
/// the count is raised first, a new block of one more number made, the old numbers copied,
/// `n` put last and the old block freed. 0 when memory is out (the count stays raised).
fn push_number(m: &mut Mem, o: &mut dyn Outside, r: u32, n: i32, msg: u32) -> bool {
    let old = m.i32(r + grp::COUNT);
    m.set_i32(r + grp::COUNT, old.wrapping_add(1));
    let blk = m.si_malloc((old as u32).wrapping_mul(4).wrapping_add(4));
    if blk == 0 {
        oom(m, o, msg);
        return false;
    }
    let mut i = 0;
    while i < m.i32(r + grp::COUNT) - 1 {
        let v = m.u32(m.u32(r + grp::LIST).wrapping_add(4 * i as u32));
        m.set_u32(blk.wrapping_add(4 * i as u32), v);
        i += 1;
    }
    m.set_i32(blk.wrapping_add(4 * i as u32), n);
    let l = m.u32(r + grp::LIST);
    if l != 0 {
        m.free(l);
        m.set_u32(r + grp::LIST, 0);
    }
    m.set_u32(r + grp::LIST, blk);
    true
}

/// [soft 0x081c56d8 updateGroupName]: when `b < a`, every name of the list `names` is looked up
/// in the group names at `*pg` (by `strcmp` of the leading 80 bytes); a found group gets `a`
/// appended to its numbers, a new one (copy of the name, numbers `[a]`, no model) goes to the
/// end of the list. Answers `b + 1` (or `b` when it was not below `a`), 0 when memory is out.
pub fn update_group_name(m: &mut Mem, o: &mut dyn Outside, pg: u32, names: u32, a: i32, b: i32) -> i32 {
    if b >= a {
        return b;
    }
    let mut nl = names;
    while nl != 0 {
        let mut g = m.u32(pg);
        let mut found = 0;
        while g != 0 {
            if clib::strcmp(m, g, nl) == 0 {
                found = g;
                break;
            }
            g = m.u32(g + grp::NEXT);
        }
        if found != 0 {
            if !push_number(m, o, found, a, MSG_OOM_UPDATE_GROUP_A) {
                return 0;
            }
        } else {
            let g = m.si_malloc(grp::SIZE);
            if g == 0 {
                oom(m, o, MSG_OOM_UPDATE_GROUP_B);
                return 0;
            }
            clib::strcpy(m, g, nl);
            let list = m.si_malloc(4);
            m.set_u32(g + grp::LIST, list);
            if list == 0 {
                oom(m, o, MSG_OOM_UPDATE_GROUP_C);
                return 0;
            }
            m.set_i32(list, a);
            m.set_u32(g + grp::COUNT, 1);
            m.set_u32(g + grp::NEXT, 0);
            let mut t = m.u32(pg);
            if t == 0 {
                m.set_u32(pg, g);
            } else {
                while m.u32(t + grp::NEXT) != 0 {
                    t = m.u32(t + grp::NEXT);
                }
                m.set_u32(t + grp::NEXT, g);
            }
        }
        nl = m.u32(nl + NAME_NEXT);
    }
    b.wrapping_add(1)
}

/// [soft 0x081c5c38 updateSmoothingGroup]: the smoothing group of that name (in the list at
/// `*pl`) gets `a` appended to its numbers; if there is none one is made (name, numbers
/// `[a]`) and put last. 0 when memory is out.
pub fn update_smoothing_group(m: &mut Mem, o: &mut dyn Outside, pl: u32, name: u32, a: i32) -> u32 {
    let mut g = m.u32(pl);
    while g != 0 {
        if clib::strcmp(m, g, name) == 0 {
            return if push_number(m, o, g, a, MSG_OOM_UPDATE_SMOOTH_A) { 1 } else { 0 };
        }
        g = m.u32(g + grp::SMOOTH_NEXT);
    }
    let g = m.si_malloc(grp::SMOOTH_SIZE);
    if g == 0 {
        oom(m, o, MSG_OOM_UPDATE_SMOOTH_B);
        return 0;
    }
    clib::strcpy(m, g, name);
    let list = m.si_malloc(4);
    m.set_u32(g + grp::LIST, list);
    if list == 0 {
        oom(m, o, MSG_OOM_UPDATE_SMOOTH_C);
        return 0;
    }
    m.set_i32(list, a);
    m.set_u32(g + grp::COUNT, 1);
    m.set_u32(g + grp::SMOOTH_NEXT, 0);
    let mut t = m.u32(pl);
    if t == 0 {
        m.set_u32(pl, g);
    } else {
        while m.u32(t + grp::SMOOTH_NEXT) != 0 {
            t = m.u32(t + grp::SMOOTH_NEXT);
        }
        m.set_u32(t + grp::SMOOTH_NEXT, g);
    }
    1
}

// ---------------------------------------------------------------- materials

/// [soft 0x081c53c8 allocateDefaultMaterial]: a material made from the program's default
/// (`SI_materialTransfer` from `SI_defaultMaterial`), its next cleared. Null when memory is out.
pub fn allocate_default_material(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let p = m.si_malloc(MAT_SIZE);
    if p != 0 {
        o.si_material_transfer(m, SI_DEFAULT_MATERIAL, p, 0);
        m.set_u32(p + MAT_NEXT, 0);
    }
    p
}

/// [soft 0x081c5630 allocDefaultWFMaterial]: a default material called "default_white": white
/// ambient, diffuse, specular and (the fourth of each group) 0, shading model 1. Null when
/// memory is out.
pub fn alloc_default_wf_material(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let p = allocate_default_material(m, o);
    if p == 0 {
        oom(m, o, MSG_OOM_DEFAULT_WF_MATERIAL);
        return 0;
    }
    for k in 0..3 {
        let w = u32::from_le_bytes(DEFAULT_WHITE[4 * k..4 * k + 4].try_into().unwrap());
        m.set_u32(p + 4 * k as u32, w);
    }
    for base in [0x34u32, 0x44, 0x54] {
        for k in 0..3 {
            m.set_u32(p + base + 4 * k, 0x3f80_0000);
        }
        m.set_u32(p + base + 12, 0);
    }
    m.set_u16(p + 0x0c, u16::from_le_bytes([DEFAULT_WHITE[12], DEFAULT_WHITE[13]]));
    m.set_u8(p + 0x1e, 1);
    m.set_u8(p + 0x1f, 0);
    p
}

/// The inlined `strcmp` against "default_white" of `addMaterial` and `cleanHierarchy`: 14
/// bytes compared, the terminator included.
fn is_default_white(m: &Mem, s: u32) -> bool {
    (0..14).all(|i| m.u8(s.wrapping_add(i)) == DEFAULT_WHITE[i as usize]) || {
        // `repz cmpsb` stops at the first difference; a string equal in its first bytes up to a
        // difference is not equal, so the only equal string is the literal itself.
        false
    }
}

/// [soft 0x081c5510 addMaterial]: the material called `name` is looked up in the library
/// `lib` (a chain by next), then in the list at `*plist`; if the list has it, its position
/// (from 1) is answered; otherwise a default material is made, filled from the library's by
/// `SI_materialTransfer`, put at the end of the list and its position answered. "default_white"
/// answers 0 at once; a name the library lacks prints a message (level 3) and answers 0.
pub fn add_material(m: &mut Mem, o: &mut dyn Outside, lib: u32, name: u32, plist: u32) -> i32 {
    if is_default_white(m, name) {
        return 0;
    }
    let mut p = lib;
    loop {
        if p == 0 {
            print_message(m, o, 3, MSG_MATERIAL_NOT_FOUND, &[MSG_ASSUMING_DEFAULT_WHITE, name]);
            return 0;
        }
        if clib::strcmp(m, p, name) == 0 {
            break;
        }
        p = m.u32(p + MAT_NEXT);
    }
    let mut q = m.u32(plist);
    let mut pos = 1;
    while q != 0 {
        if clib::strcmp(m, q, name) == 0 {
            return pos;
        }
        q = m.u32(q + MAT_NEXT);
        pos += 1;
    }
    let n = allocate_default_material(m, o);
    if n == 0 {
        oom(m, o, MSG_OOM_ADD_MATERIAL);
        return 0;
    }
    o.si_material_transfer(m, p, n, 0);
    let head = m.u32(plist);
    if head == 0 {
        m.set_u32(plist, n);
        return 1;
    }
    let mut last = head;
    let mut pos = 2;
    while m.u32(last + MAT_NEXT) != 0 {
        last = m.u32(last + MAT_NEXT);
        pos += 1;
    }
    m.set_u32(last + MAT_NEXT, n);
    pos
}

/// [soft 0x081c5408 copyMaterial]: the `idx`-th (from 1) material of the used list `used` is
/// copied to the end of the list at `*plist` unless a material of that name is there; answers
/// the position (from 0) in `*plist` of the copy or of the one found; 0 for a null list or an
/// index of 0; a list too short prints a message (level 2) and answers 0.
pub fn copy_material(m: &mut Mem, o: &mut dyn Outside, idx: i32, used: u32, plist: u32) -> i32 {
    if used == 0 || idx == 0 {
        return 0;
    }
    let mut u = used;
    let mut i = 1;
    if 1 < idx {
        loop {
            u = m.u32(u + MAT_NEXT);
            i += 1;
            if idx <= i || u == 0 {
                break;
            }
        }
    }
    if u == 0 {
        print_message(m, o, 2, MSG_NOT_FOUND_MTL, &[idx as u32]);
        return 0;
    }
    let mut q = m.u32(plist);
    let mut pos = 0;
    while q != 0 {
        if clib::strcmp(m, q, u) == 0 {
            return pos;
        }
        q = m.u32(q + MAT_NEXT);
        pos += 1;
    }
    let n = allocate_default_material(m, o);
    if n == 0 {
        print_message(m, o, 3, MSG_OOM_COPY_MATERIAL, &[]);
        return 0;
    }
    o.si_material_transfer(m, u, n, 0);
    m.set_u32(n + MAT_NEXT, 0);
    let head = m.u32(plist);
    if head == 0 {
        m.set_u32(plist, n);
        return 0;
    }
    let mut last = head;
    let mut pos = 1;
    while m.u32(last + MAT_NEXT) != 0 {
        last = m.u32(last + MAT_NEXT);
        pos += 1;
    }
    m.set_u32(last + MAT_NEXT, n);
    pos
}

// ---------------------------------------------------------------- models

/// Model fields the file reaches: kind, material count and flag, the material array, the
/// father, brother and first son.
pub mod model {
    pub const NAME: u32 = 0x04;
    pub const KIND: u32 = 0x30;
    pub const MAT_COUNT: u32 = 0x206;
    pub const MAT_FLAG: u32 = 0x208;
    pub const MATS: u32 = 0x20c;
    pub const FATHER: u32 = 0x2bc;
    pub const BROTHER: u32 = 0x2c0;
    pub const SON: u32 = 0x2c4;
}

/// [soft 0x081c583c cleanGroupName]: the list of group names rebuilt in ascending order of how
/// many entries they hold (a copy of each, same-count groups in their old order, the old list
/// freed); a group of two or more gets a model of its own (named by a copy of the group's name)
/// and, when `findFather` over the groups after it finds one holding the group's first number
/// that has a model, is made a child of that model. A single group is only given its model. Answers the new
/// list, null for an empty list or when memory ran out (the lists made are then freed; the old
/// one too, except when only the number array could not be had).
/// (The original reads the count array one place past its end while skipping equal counts;
/// that read cannot change the walk and is not made.)
pub fn clean_group_name(m: &mut Mem, o: &mut dyn Outside, first: u32) -> u32 {
    let mut param = first;
    let slot = m.local(4); // the old list's head, as disposeGroupName takes it
    let new_slot = m.local(4);
    m.set_u32(slot, first);
    m.set_u32(new_slot, 0);
    let r = clean_group_name_in(m, o, &mut param, slot, new_slot);
    m.release(4);
    m.release(4);
    r
}

fn model_for(m: &mut Mem, o: &mut dyn Outside, g: u32, msg: u32) -> bool {
    // a count of two or more: a model with a copy of the name
    let model = o.si_model_allocate(m, 0);
    m.set_u32(g + grp::MODEL, model);
    if model == 0 {
        oom(m, o, msg);
        return false;
    }
    let old = m.u32(model + model::NAME);
    if old != 0 {
        m.free(old);
        m.set_u32(model + model::NAME, 0);
    }
    let d = m.strdup(g);
    m.set_u32(model + model::NAME, d);
    true
}

fn clean_group_name_in(m: &mut Mem, o: &mut dyn Outside, param: &mut u32, slot: u32, new_slot: u32) -> u32 {
    let mut count = 0;
    let mut g = *param;
    while g != 0 {
        count += 1;
        g = m.u32(g + grp::NEXT);
    }
    if count < 2 {
        if count == 1 {
            let g = *param;
            if m.i32(g + grp::COUNT) < 2 {
                m.set_u32(g + grp::MODEL, 0);
            } else {
                if !model_for(m, o, g, MSG_CGN[4]) {
                    dispose_group_name(m, slot);
                    return 0;
                }
                m.set_u32(g + grp::PARENT, 0);
                m.set_u32(g + grp::NEXT, 0);
            }
        }
        return *param;
    }
    let base = m.si_malloc(count * 4);
    if base == 0 {
        oom(m, o, MSG_CGN[0]);
        return 0;
    }
    let mut i = 0;
    let mut g = *param;
    while g != 0 {
        let c = m.u32(g + grp::COUNT);
        m.set_u32(base + 4 * i, c);
        i += 1;
        g = m.u32(g + grp::NEXT);
    }
    clib::qsort(m, base, count, 4, &mut |m, a, b| compare_int(m, a, b));
    let mut done = 0;
    while done < count {
        let v = m.i32(base + 4 * done);
        while done < count {
            done += 1;
            if m.i32(base + 4 * done) != v {
                break;
            }
        }
        let mut g = *param;
        while g != 0 {
            if m.i32(g + grp::COUNT) == v {
                let n = m.si_malloc(grp::SIZE);
                let mut fail = 0;
                if n == 0 {
                    fail = 1;
                } else {
                    clib::strcpy(m, n, g);
                    let c = m.i32(g + grp::COUNT);
                    m.set_i32(n + grp::COUNT, c);
                    let list = m.si_malloc((c as u32) << 2);
                    m.set_u32(n + grp::LIST, list);
                    if list == 0 {
                        fail = 2;
                    } else {
                        let mut k = 0;
                        while k < m.i32(g + grp::COUNT) {
                            let x = m.u32(m.u32(g + grp::LIST).wrapping_add(4 * k as u32));
                            m.set_u32(list.wrapping_add(4 * k as u32), x);
                            k += 1;
                        }
                        if m.i32(g + grp::COUNT) < 2 {
                            m.set_u32(n + grp::MODEL, 0);
                        } else if !model_for(m, o, n, MSG_CGN[3]) {
                            fail = 3;
                        }
                    }
                }
                if fail != 0 {
                    if fail != 3 {
                        oom(m, o, MSG_CGN[fail]);
                    }
                    dispose_group_name(m, new_slot);
                    dispose_group_name(m, slot);
                    *param = 0;
                    return 0;
                }
                m.set_u32(n + grp::PARENT, 0);
                m.set_u32(n + grp::NEXT, 0);
                let head = m.u32(new_slot);
                if head == 0 {
                    m.set_u32(new_slot, n);
                } else {
                    let mut t = head;
                    while m.u32(t + grp::NEXT) != 0 {
                        t = m.u32(t + grp::NEXT);
                    }
                    m.set_u32(t + grp::NEXT, n);
                }
            }
            g = m.u32(g + grp::NEXT);
        }
    }
    dispose_group_name(m, slot);
    let new = m.u32(new_slot);
    let mut g = new;
    while g != 0 {
        if m.i32(g + grp::COUNT) > 1 {
            let n0 = m.i32(m.u32(g + grp::LIST));
            let f = find_father(m, n0, m.u32(g + grp::NEXT));
            if f != 0 && m.u32(f + grp::MODEL) != 0 {
                let (pm, cm) = (m.u32(f + grp::MODEL), m.u32(g + grp::MODEL));
                o.si_model_create_parentship(m, pm, cm);
                m.set_u32(g + grp::PARENT, pm);
            }
        }
        g = m.u32(g + grp::NEXT);
    }
    if base != 0 {
        m.free(base);
    }
    new
}

/// The first 14 bytes of the material compared with "default_white" and its terminator.
fn material_is_default_white(m: &Mem, mat: u32) -> bool {
    (0..14).all(|i| m.u8(mat.wrapping_add(i)) == DEFAULT_WHITE[i as usize])
}

/// [soft 0x081c3a70 cleanHierarchy]: the tree under `*pmodel` tidied. A node of no kind that
/// has a father and a son (and the son has no brother) is cut out, the son taking its place;
/// the topmost such node (no father, a single son without brother) is replaced by the son as
/// the root; a node of kind 1 or 4 with a father whose single material is "default_white"
/// loses its material array. At the end a root of no kind gets a one-entry array holding a
/// copy of a default white material.
pub fn clean_hierarchy(m: &mut Mem, o: &mut dyn Outside, pmodel: u32) {
    let cur_slot = m.local(4);
    let mat_slot = m.local(4);
    let first = m.u32(pmodel);
    m.set_u32(cur_slot, first);
    while m.u32(cur_slot) != 0 {
        let cur = m.u32(cur_slot);
        let mut next_node = true;
        let mut check_material = true;
        if m.i32(cur + model::KIND) == 0 {
            let father = m.u32(cur + model::FATHER);
            let son = m.u32(cur + model::SON);
            if father != 0 && son != 0 {
                if m.i32(father + model::KIND) != 0 || m.u32(son + model::BROTHER) != 0 {
                    check_material = false;
                } else {
                    let mut e = m.u32(father + model::SON);
                    if e == cur {
                        m.set_u32(father + model::SON, son);
                    } else {
                        while m.u32(e + model::BROTHER) != cur {
                            e = m.u32(e + model::BROTHER);
                        }
                        m.set_u32(e + model::BROTHER, son);
                    }
                    m.set_u32(son + model::FATHER, father);
                    let b = m.u32(cur + model::BROTHER);
                    m.set_u32(son + model::BROTHER, b);
                    o.si_model_dispose_node(m, cur_slot);
                    m.set_u32(cur_slot, father);
                    next_node = false;
                }
            } else if m.u32(cur + model::FATHER) == 0 && son != 0 && m.u32(cur + model::BROTHER) == 0 {
                if m.u32(son + model::BROTHER) != 0 {
                    check_material = false;
                } else {
                    m.set_u32(son + model::FATHER, 0);
                    m.set_u32(pmodel, son);
                    o.si_model_dispose_node(m, cur_slot);
                    m.set_u32(cur_slot, son);
                    next_node = false;
                }
            }
        }
        if !next_node {
            continue;
        }
        if check_material {
            let kind = m.i32(cur + model::KIND);
            if (kind == 4 || kind == 1) && m.u16(cur + model::MAT_COUNT) == 1 && m.u32(cur + model::FATHER) != 0 {
                let arr = m.u32(cur + model::MATS);
                let first_mat = m.u32(arr);
                if material_is_default_white(m, first_mat) {
                    o.si_material_dispose(m, cur, arr);
                    let a = m.u32(cur + model::MATS);
                    if a != 0 {
                        m.free(a);
                        m.set_u32(cur + model::MATS, 0);
                    }
                    m.set_u16(cur + model::MAT_FLAG, 0);
                    m.set_u16(cur + model::MAT_COUNT, 0);
                }
            }
        }
        let n = o.si_model_next_node(m, cur);
        m.set_u32(cur_slot, n);
    }
    let root = m.u32(pmodel);
    if m.i32(root + model::KIND) == 0 {
        m.set_u16(root + model::MAT_COUNT, 1);
        m.set_u16(root + model::MAT_FLAG, 0);
        let arr = m.si_calloc(1, 4);
        let root = m.u32(pmodel);
        m.set_u32(root + model::MATS, arr);
        if arr == 0 {
            o.si_model_dispose(m, pmodel, 1);
        }
        let dm = alloc_default_wf_material(m, o);
        m.set_u32(mat_slot, dm);
        let c = o.si_material_copy(m, dm);
        let root = m.u32(pmodel);
        let a = m.u32(root + model::MATS);
        m.set_u32(a, c); // (a null array is a null write in the original)
        si_dispose_material(m, mat_slot);
    }
    m.release(4);
    m.release(4);
}

// ---------------------------------------------------------------- conversion

/// What `fld`/`fstp` of a single float does to its bits (the originals copy vertex coordinates
/// through the x87 unit): a signalling NaN comes out quiet, everything else is unchanged.
fn x87_f32_copy(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

fn copy_f32(m: &mut Mem, dst: u32, src: u32) {
    let v = x87_f32_copy(m.u32(src));
    m.set_u32(dst, v);
}

/// [soft 0x081c4138 convertPoint]: a mesh made of the points of an entry (a mesh of vertices
/// only), set as the model's definition (kind 4). The element list's count is the number of
/// vertices; the vertex numbers are those of the pairs. 0 if memory is out or the list is empty
/// (the mesh made is then not freed in the second case, as in the original).
pub fn convert_point(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32) -> u32 {
    let mesh_slot = m.local(4);
    let r = (|| {
        let mesh = o.si_mesh_allocate(m);
        m.set_u32(mesh_slot, mesh);
        if mesh == 0 {
            oom(m, o, MSG_OOM_POINT[0]);
            return 0;
        }
        let count = m.i32(m.u32(w + wf::DATA) + elem::COUNT);
        if count == 0 {
            return 0;
        }
        m.set_i32(mesh + 0x14, count);
        let v = o.si_vertex_allocate(m, count);
        m.set_u32(mesh, v);
        if v == 0 {
            oom(m, o, MSG_OOM_POINT[1]);
            o.si_mesh_dispose(m, mesh_slot);
            return 0;
        }
        let mut i = 0;
        let mut off = 0u32;
        while i < m.i32(mesh + 0x14) {
            let e = get_list_at_position(m, m.u32(w + wf::DATA), i);
            let n = m.i32(e);
            let verts = m.u32(m.u32(w) + hdr::VERTS);
            let p = get_vertex_at_position(m, verts, n.wrapping_sub(1));
            let dst = m.u32(mesh).wrapping_add(off);
            for k in 0..3 {
                copy_f32(m, dst.wrapping_add(4 * k), p.wrapping_add(4 * k));
            }
            i += 1;
            off = off.wrapping_add(0x14);
        }
        o.si_model_set_definition(m, model_node, 4, mesh);
        o.si_mesh_update(m, mesh);
        1
    })();
    m.release(4);
    r
}

/// One key of a spline made from a vertex: the coordinates copied to +0xc and, as the
/// three tangent triples, to +0x18 and +0; flag 0 at +0x2c.
fn fill_key(m: &mut Mem, key: u32, vtx: u32) {
    for k in 0..3 {
        copy_f32(m, key + 0x0c + 4 * k, vtx + 4 * k);
    }
    let (x, y, z) = (m.u32(key + 0x0c), m.u32(key + 0x10), m.u32(key + 0x14));
    m.set_u32(key + 0x1c, y);
    m.set_u32(key + 0x18, x);
    m.set_u32(key + 0x20, z);
    m.set_u32(key, x);
    m.set_u32(key + 4, y);
    m.set_u32(key + 8, z);
    m.set_u16(key + 0x2c, 0);
}

/// [soft 0x081c4538 convertLine]: a spline (kind 6 definition) with a key for each node of the
/// entry's line list; the key count is the low 16 bits of the side record's first word, and a
/// record whose first word is 0 gives 0 (the spline made is not freed).
pub fn convert_line(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32) -> u32 {
    let slot = m.local(4);
    let r = (|| {
        let spl = o.si_spline_allocate(m);
        m.set_u32(slot, spl);
        if spl == 0 {
            oom(m, o, MSG_OOM_LINE[0]);
            return 0;
        }
        m.set_u32(spl + 0x0c, 0);
        let aux = m.u32(w + wf::AUX);
        let n = m.u16(aux);
        m.set_u16(spl + 0x10, n);
        if m.u32(aux) == 0 {
            return 0;
        }
        let keys = o.si_spline_key_allocate(m, m.u16(spl + 0x10) as u32);
        m.set_u32(spl + 0x14, keys);
        if keys == 0 {
            oom(m, o, MSG_OOM_LINE[1]);
            o.si_spline_dispose(m, slot);
            return 0;
        }
        let mut node = m.u32(w + wf::DATA);
        let mut key = keys;
        let mut i = 0u32;
        if m.u16(spl + 0x10) != 0 {
            loop {
                let verts = m.u32(m.u32(w) + hdr::VERTS);
                let v = get_vertex_at_position(m, verts, m.i32(node).wrapping_sub(1));
                fill_key(m, key, v);
                node = m.u32(node + 4);
                i += 1;
                key = key.wrapping_add(0x30);
                if i >= m.u16(spl + 0x10) as u32 {
                    break;
                }
            }
        }
        o.si_spline_update_matrix(m, spl);
        o.si_model_set_definition(m, model_node, 6, spl);
        1
    })();
    m.release(4);
    r
}

/// [soft 0x081c466c convertCurve]: a spline for a curve entry. Its kind (+0xc) is the side
/// record's first word, with 0.5 stored at +0x1c for kind 3; when the list has more than 5
/// pairs and its last three repeat the first three (vertex numbers), the spline is closed
/// (+8 = 1) and has three keys fewer. A key for each remaining pair, as in [`convert_line`].
pub fn convert_curve(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32) -> u32 {
    let slot = m.local(4);
    let r = (|| {
        let spl = o.si_spline_allocate(m);
        m.set_u32(slot, spl);
        if spl == 0 {
            oom(m, o, MSG_OOM_CURVE[0]);
            return 0;
        }
        let kind = m.i32(m.u32(w + wf::AUX));
        m.set_i32(spl + 0x0c, kind);
        if kind == 3 {
            m.set_u32(spl + 0x1c, 0.5f32.to_bits()); // 0.5, at 0x0864c280
        } else {
            m.set_u32(spl + 0x1c, 0);
        }
        m.set_u16(spl + 6, 1);
        let data = m.u32(w + wf::DATA);
        let count = m.i32(data + elem::COUNT);
        if count > 5 {
            let same = |m: &Mem, a: i32, b: i32| {
                let x = get_list_at_position(m, data, a);
                let y = get_list_at_position(m, data, b);
                m.u32(x) == m.u32(y)
            };
            if same(m, 0, count - 3) && same(m, 1, count - 2) && same(m, 2, count - 1) {
                m.set_u16(spl + 8, 1);
                m.set_u16(spl + 0x10, (count - 3) as u16);
            }
        }
        if m.u16(spl + 8) == 0 {
            let c = m.u16(m.u32(w + wf::DATA) + elem::COUNT);
            m.set_u16(spl + 0x10, c);
        }
        let keys = o.si_spline_key_allocate(m, m.u16(spl + 0x10) as u32);
        m.set_u32(spl + 0x14, keys);
        if keys == 0 {
            oom(m, o, MSG_OOM_CURVE[1]);
            o.si_spline_dispose(m, slot);
            return 0;
        }
        let mut pos = 0;
        let mut key = keys;
        if m.u16(spl + 0x10) != 0 {
            let mut i = 0u32;
            loop {
                let e = get_list_at_position(m, m.u32(w + wf::DATA), pos);
                let verts = m.u32(m.u32(w) + hdr::VERTS);
                let v = get_vertex_at_position(m, verts, m.i32(e).wrapping_sub(1));
                fill_key(m, key, v);
                i += 1;
                key = key.wrapping_add(0x30);
                pos += 1;
                if i >= m.u16(spl + 0x10) as u32 {
                    break;
                }
            }
        }
        o.si_spline_update_matrix(m, spl);
        o.si_model_set_definition(m, model_node, 6, spl);
        1
    })();
    m.release(4);
    r
}

/// [soft 0x081c4254 convertPatch]: a patch (kind 1 definition) from an entry: the side record
/// gives the patch's two words at +4 and +8 (its first word), the two 16-bit sizes at +0xc
/// and +0xe, and the floats at +0x14 and +0x18; the element list gives its control points
/// (0x10 bytes each, the fourth float 1.0, in a calloc'd array at +0x1a4, with 16-bit zeros in
/// another at +0x1a8). The first pair, when its vertex number is -1, names a material (its
/// second number) which is then looked up with `copyMaterial` in `used`; otherwise the
/// entry's own material number or a default white. The model's material array becomes one
/// copy of that material unless it is "default_white". 0 (the patch disposed) when memory is out
/// or a vertex number is beyond the list.
pub fn convert_patch(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32, used: u32) -> u32 {
    let slot = m.local(4); // local_8: the patch
    let mat_slot = m.local(4); // local_c: the material list
    let r = convert_patch_in(m, o, model_node, w, used, slot, mat_slot);
    m.release(4);
    m.release(4);
    r
}

fn convert_patch_in(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32, used: u32, slot: u32, mat_slot: u32) -> u32 {
    m.set_u32(mat_slot, 0);
    let patch = o.si_patch_allocate(m);
    m.set_u32(slot, patch);
    if patch == 0 {
        oom(m, o, MSG_OOM_PATCH[0]);
        return 0;
    }
    let aux = m.u32(w + wf::AUX);
    let a0 = m.u32(aux);
    m.set_u32(patch + 8, a0);
    m.set_u32(patch + 4, a0);
    let (cw, ch) = (m.u16(aux + 4), m.u16(aux + 6));
    m.set_u16(patch + 0x0c, cw);
    m.set_u16(patch + 0x0e, ch);
    copy_f32(m, patch + 0x14, aux + 8);
    copy_f32(m, patch + 0x18, aux + 0xc);
    let cnt = (cw as u32).wrapping_mul(ch as u32);
    let pts = m.si_calloc(cnt, 0x10);
    m.set_u32(patch + 0x1a4, pts);
    let fail_msg;
    'build: {
        if pts == 0 {
            fail_msg = MSG_OOM_PATCH[1];
            break 'build;
        }
        let flags = m.si_calloc(cnt, 2);
        m.set_u32(patch + 0x1a8, flags);
        if flags == 0 {
            fail_msg = MSG_OOM_PATCH[2];
            break 'build;
        }
        let mut pt = m.u32(patch + 0x1a4);
        let mut fl = m.u32(patch + 0x1a8);
        let mut pos: i32 = 0;
        let mut rows = 0u32;
        if m.u16(patch + 0x0c) != 0 {
            loop {
                let mut cols = 0u32;
                if m.u16(patch + 0x0e) != 0 {
                    loop {
                        let mut e = get_list_at_position(m, m.u32(w + wf::DATA), pos);
                        if pos == 0 {
                            if m.i32(e) == -1 {
                                let h = m.u32(w);
                                let c1 = m.u32(e + 4);
                                m.set_u32(h + hdr::COUNT, c1);
                                copy_material(m, o, m.i32(m.u32(w) + hdr::COUNT), used, mat_slot);
                                pos = 1;
                                e = get_list_at_position(m, m.u32(w + wf::DATA), 1);
                            } else if m.i32(w + wf::COUNT) == 0 {
                                if m.u32(mat_slot) == 0 {
                                    let d = alloc_default_wf_material(m, o);
                                    m.set_u32(mat_slot, d);
                                }
                            } else {
                                copy_material(m, o, m.i32(w + wf::COUNT), used, mat_slot);
                            }
                        }
                        let verts = m.u32(m.u32(w) + hdr::VERTS);
                        let vnum = m.i32(e);
                        if m.i32(verts + vl::COUNT) < vnum {
                            print_message(m, o, 3, MSG_OOM_PATCH[3], &[vnum as u32]);
                            o.si_patch_dispose(m, slot);
                            return 0;
                        }
                        let v = get_vertex_at_position(m, verts, vnum.wrapping_sub(1));
                        for k in 0..3 {
                            copy_f32(m, pt + 4 * k, v + 4 * k);
                        }
                        m.set_u32(pt + 0xc, 0x3f80_0000);
                        m.set_u16(fl, 0);
                        pos += 1;
                        cols += 1;
                        pt = pt.wrapping_add(0x10);
                        fl = fl.wrapping_add(2);
                        if cols >= m.u16(patch + 0x0e) as u32 {
                            break;
                        }
                    }
                }
                rows += 1;
                if rows >= m.u16(patch + 0x0c) as u32 {
                    break;
                }
            }
        }
        let mat = m.u32(mat_slot);
        if !material_is_default_white(m, mat) {
            m.set_u16(model_node + model::MAT_COUNT, 1);
            m.set_u16(model_node + model::MAT_FLAG, 0);
            let arr = m.si_calloc(1, 4);
            m.set_u32(model_node + model::MATS, arr);
            if arr == 0 {
                fail_msg = MSG_OOM_PATCH[4];
                break 'build;
            }
            let c = o.si_material_copy(m, m.u32(mat_slot));
            m.set_u32(arr, c);
        }
        si_dispose_material(m, mat_slot);
        o.si_patch_update_matrix(m, patch);
        o.si_model_set_definition(m, model_node, 1, patch);
        return 1;
    }
    oom(m, o, fail_msg);
    o.si_patch_dispose(m, slot);
    0
}

/// [soft 0x081c3cac convertMesh]: a mesh (kind 4 definition) from a polygon entry. The side
/// record holds the polygon count (+0) and, as the vertices are numbered in the order the
/// polygons use them, the count of those used (+4). The element list is read as runs of pairs
/// ended by a pair whose first number is 0, one run a polygon; a run led by (-1, n) is a new
/// material (number n, looked up with `copyMaterial`). Each polygon gets a calloc'd array of 0x24
/// byte corners (vertex number, the normal copied to +8 if the pair has one); the mesh's vertex
/// array is filled from the vertices numbered, which are then marked unused again (-1). The
/// model's materials become copies of the material list up to the highest index used. With
/// `order` the mesh is put through `SI_orderMesh`. Ends with `checkSmoothingGroup` over
/// `smooth` and `base`. 0 on a too large mesh, memory or a vertex number beyond the list (the
/// mesh is disposed; lists made are not).
pub fn convert_mesh(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32, used: u32, smooth: u32, order: i16, base: i32) -> u32 {
    let slot = m.local(4); // local_8: the mesh
    let mat_slot = m.local(4); // local_c: the material list
    let r = convert_mesh_in(m, o, model_node, w, used, smooth, order, base, slot, mat_slot);
    m.release(4);
    m.release(4);
    r
}

#[allow(clippy::too_many_arguments)]
fn convert_mesh_in(m: &mut Mem, o: &mut dyn Outside, model_node: u32, w: u32, used: u32, smooth: u32, order: i16, base: i32, slot: u32, mat_slot: u32) -> u32 {
    let mut maxmat: i32 = -1;
    m.set_u32(mat_slot, 0);
    let aux = m.u32(w + wf::AUX);
    m.set_u32(aux + 4, 0);
    if m.i32(SI_NB_POL_MAX) < m.i32(aux) {
        o.si_max_nb_pol_warning(m);
        return 0;
    }
    let mesh = o.si_mesh_allocate(m);
    m.set_u32(slot, mesh);
    if mesh == 0 {
        oom(m, o, MSG_MESH[0]);
        return 0;
    }
    let mut remaining = m.i32(aux);
    m.set_i32(mesh + 0x18, remaining);
    let mut poly = o.si_polygon_allocate(m, remaining);
    m.set_u32(mesh + 4, poly);
    let fail = 'build: {
        if poly == 0 {
            break 'build MSG_MESH[1];
        }
        let data = m.u32(w + wf::DATA);
        let mut p: i32 = 0;
        while remaining != 0 {
            let mut e = get_list_at_position(m, data, p);
            let mat;
            if m.i32(e) == -1 {
                let h = m.u32(w);
                let c1 = m.u32(e + 4);
                m.set_u32(h + hdr::COUNT, c1);
                mat = copy_material(m, o, m.i32(m.u32(w) + hdr::COUNT), used, mat_slot);
                p += 1;
                e = get_list_at_position(m, data, p);
            } else if m.i32(w + wf::COUNT) == 0 {
                mat = 0;
            } else {
                mat = copy_material(m, o, m.i32(w + wf::COUNT), used, mat_slot);
            }
            m.set_u16(poly + 0x12, mat as u16);
            if maxmat < mat {
                maxmat = mat;
            }
            let mut q = p;
            while m.i32(e) != 0 {
                q += 1;
                e = get_list_at_position(m, data, q);
            }
            let nv = q - p;
            m.set_u16(poly + 0x10, nv as u16);
            let corners = m.si_calloc((nv.wrapping_sub((nv.wrapping_sub(1) & 3).wrapping_sub(3))) as u32, 0x24);
            m.set_u32(poly + 0xc, corners);
            if corners == 0 {
                break 'build MSG_MESH[2];
            }
            let mut i = 0;
            let mut off = 0u32;
            while i < nv {
                let e = get_list_at_position(m, data, p);
                let verts = m.u32(m.u32(w) + hdr::VERTS);
                let vnum = m.i32(e);
                if vnum > m.i32(verts + vl::COUNT) {
                    print_message(m, o, 3, MSG_MESH[4], &[vnum as u32]);
                    o.si_mesh_dispose(m, slot);
                    return 0;
                }
                let v = get_vertex_at_position(m, verts, vnum.wrapping_sub(1));
                if m.i32(v + 0xc) == -1 {
                    let a = m.u32(w + wf::AUX);
                    let n = m.u32(a + 4);
                    m.set_u32(v + 0xc, n);
                    m.set_u32(a + 4, n.wrapping_add(1));
                }
                let pc = m.u32(poly + 0xc);
                let idx = m.u32(v + 0xc);
                m.set_u32(pc.wrapping_add(off), idx);
                let nn = m.i32(e + 4);
                if nn > 0 {
                    let norms = m.u32(m.u32(w) + hdr::NORMALS);
                    let nrm = get_vertex_at_position(m, norms, nn - 1);
                    if nn > m.i32(norms + vl::COUNT) {
                        print_message(m, o, 3, MSG_MESH[5], &[nn as u32]);
                    } else {
                        for k in 0..3 {
                            let pc = m.u32(poly + 0xc);
                            copy_f32(m, pc.wrapping_add(off).wrapping_add(8 + 4 * k), nrm.wrapping_add(4 * k));
                        }
                    }
                }
                off = off.wrapping_add(0x24);
                i += 1;
                p += 1;
            }
            poly = poly.wrapping_add(0x18);
            p += 1;
            remaining -= 1;
        }
        let mut nmat = 0;
        if m.u32(mat_slot) != 0 {
            nmat = maxmat.wrapping_add(1);
        }
        let aux = m.u32(w + wf::AUX);
        let nverts = m.u32(aux + 4);
        m.set_u32(mesh + 0x14, nverts);
        let va = o.si_vertex_allocate(m, nverts as i32);
        m.set_u32(mesh, va);
        if va == 0 {
            break 'build MSG_MESH[3];
        }
        let mut j = 0;
        let mut hd = m.u32(w);
        while j < m.i32(m.u32(hd + hdr::VERTS) + vl::COUNT) {
            let v = get_vertex_at_position(m, m.u32(hd + hdr::VERTS), j);
            let id = m.i32(v + 0xc);
            if id != -1 {
                let dst = m.u32(mesh).wrapping_add((id as u32).wrapping_mul(0x14));
                for k in 0..3 {
                    copy_f32(m, dst.wrapping_add(4 * k), v + 4 * k);
                }
                m.set_i32(v + 0xc, -1);
            }
            hd = m.u32(w);
            j += 1;
        }
        m.set_u16(model_node + model::MAT_COUNT, nmat as u16);
        m.set_u16(model_node + model::MAT_FLAG, 0);
        if nmat != 0 {
            let arr = m.si_calloc(m.i16(model_node + model::MAT_COUNT) as i32 as u32, 4);
            m.set_u32(model_node + model::MATS, arr);
            if arr == 0 {
                o.si_mesh_dispose(m, slot);
                return 0;
            }
            let mut mt = m.u32(mat_slot);
            let mut k = 0;
            while k < nmat {
                let c = o.si_material_copy(m, mt);
                let arr = m.u32(model_node + model::MATS);
                m.set_u32(arr.wrapping_add(4 * k as u32), c);
                mt = m.u32(mt + MAT_NEXT);
                k += 1;
            }
        }
        0
    };
    if fail != 0 {
        oom(m, o, fail);
        o.si_mesh_dispose(m, slot);
        return 0;
    }
    si_dispose_material(m, mat_slot);
    let mut result = m.u32(slot);
    if order != 0 {
        print_message(m, o, 1, MSG_MESH[6], &[]);
        result = o.si_order_mesh(m, m.u32(slot));
        o.si_mesh_dispose(m, slot);
    }
    o.si_model_set_definition(m, model_node, 4, result);
    o.si_mesh_update(m, result);
    o.si_mesh_init_vertex_colors(m, result);
    check_smoothing_group(m, o, smooth, result, base);
    1
}


const MSG_CONVERT: [u32; 5] = [0x0869_1c64, 0x0869_1c73, 0x0869_1c82, 0x0869_1c8a, 0x0869_1ca1];

/// [soft 0x081c3614 convertWFModel2Soft]: the models for a read file. `groups` is the list of
/// group names (see [`clean_group_name`]), `w` the first entry, `used` the material list,
/// `smooth` the smoothing groups, `order` for `SI_orderMesh`. Each entry becomes a model
/// (named after the group that holds only that entry, else `<file name without directory>_NNN`),
/// built by the `convert*` of its kind (an unknown kind makes an empty model); a model is
/// hung under the model of the group-name record holding its number in a list of two or more
/// (`findFather`), else under the root. With only one entry the model is the result; if no
/// model got a parent through the groups, a lone top group model is the root; the groups'
/// own models are then hung under the root. Ends with [`clean_hierarchy`] and names the root
/// after the header's name. Returns the root, 0 when there is nothing to convert, too many
/// vertices (none), memory is out or a conversion fails (every model made is disposed).
pub fn convert_wf_model2soft(m: &mut Mem, o: &mut dyn Outside, name: u32, groups: u32, w: u32, used: u32, smooth: u32, order: i16) -> u32 {
    let root_slot = m.local(4); // local_104: the root model
    let node_slot = m.local(4); // local_108: the model being made
    let buf = m.local(0x100); // local_100: the model's name
    let r = (|| {
        m.set_u32(root_slot, 0);
        if m.i32(w + wf::KIND) == 0 {
            print_message(m, o, 2, MSG_CONVERT[4], &[]);
            return 0;
        }
        let verts = m.u32(m.u32(w) + hdr::VERTS);
        if m.i32(verts + vl::COUNT) <= 0 {
            return 0;
        }
        m.set_u32(node_slot, 0);
        let mut parented = false;
        let mut counter: i32 = 1;
        // the first group name holding two or more entries
        let mut first_group = groups;
        while first_group != 0 && m.i32(first_group + grp::COUNT) <= 1 {
            first_group = m.u32(first_group + grp::NEXT);
        }
        let root = o.si_model_allocate(m, 0);
        m.set_u32(root_slot, root);
        if root == 0 {
            oom(m, o, MSG_CONVERT[0]);
            return 0;
        }
        let slash = clib::strrchr(m, name, b'/');
        let base = if slash == 0 { name } else { slash + 1 };
        let mut e = w;
        while e != 0 {
            let node = o.si_model_allocate(m, 0);
            m.set_u32(node_slot, node);
            if node == 0 {
                oom(m, o, MSG_CONVERT[1]);
                o.si_model_dispose(m, root_slot, 1);
                return 0;
            }
            let mut g = groups;
            while g != 0 {
                if m.i32(g + grp::COUNT) == 1 && m.i32(m.u32(g + grp::LIST)) == m.i32(e + wf::INDEX) {
                    break;
                }
                g = m.u32(g + grp::NEXT);
            }
            if g != 0 {
                clib::strcpy(m, buf, g);
            } else {
                // sprintf "%s_%03d"; the original's 256-byte buffer is not guarded (a longer
                // name overruns it; not reproduced)
                let mut text = Vec::new();
                let mut p = base;
                while m.u8(p) != 0 {
                    text.push(m.u8(p));
                    p += 1;
                }
                text.extend_from_slice(format!("_{:03}", counter).as_bytes());
                for (k, b) in text.iter().enumerate() {
                    m.set_u8(buf + k as u32, *b);
                }
                m.set_u8(buf + text.len() as u32, 0);
                counter = counter.wrapping_add(1);
            }
            let old = m.u32(node + model::NAME);
            if old != 0 {
                m.free(old);
                m.set_u32(node + model::NAME, 0);
            }
            let nm = m.strdup(buf);
            m.set_u32(node + model::NAME, nm);
            print_message(m, o, 1, MSG_CONVERT[3], &[nm]);
            let ok = match m.i32(e + wf::KIND) {
                2 => convert_mesh(m, o, node, e, used, smooth, order, 0) != 0,
                1 => convert_patch(m, o, node, e, used) != 0,
                4 => convert_point(m, o, node, e) != 0,
                5 => convert_line(m, o, node, e) != 0,
                3 => convert_curve(m, o, node, e) != 0,
                _ => true,
            };
            if !ok {
                o.si_model_dispose(m, root_slot, 1);
                o.si_model_dispose(m, node_slot, 1);
                return 0;
            }
            let first = m.u32(m.u32(e) + hdr::FIRST);
            if m.i32(first + 4) > 1 {
                let f = find_father(m, m.i32(e + wf::INDEX), first_group);
                if f != 0 {
                    let pm = m.u32(f + grp::MODEL);
                    o.si_model_create_parentship(m, pm, node);
                } else {
                    let rt = m.u32(root_slot);
                    o.si_model_create_parentship(m, rt, node);
                    parented = true;
                }
            } else {
                o.si_model_dispose(m, root_slot, 1);
                let n = m.u32(node_slot);
                m.set_u32(root_slot, n);
            }
            e = m.u32(e + wf::NEXT);
        }
        let hd = m.u32(w);
        if m.i32(w + wf::KIND) != 0 && m.u32(hd + hdr::VERTS) != 0 {
            let mut hang = parented;
            if !parented {
                let mut top = 0;
                let mut k = 0;
                let mut c = first_group;
                while c != 0 {
                    if m.u32(c + grp::PARENT) == 0 {
                        top = m.u32(c + grp::MODEL);
                        k += 1;
                    }
                    c = m.u32(c + grp::NEXT);
                }
                if k == 1 {
                    o.si_model_dispose(m, root_slot, 0);
                    m.set_u32(root_slot, top);
                } else {
                    hang = true;
                }
            }
            if hang {
                let mut c = first_group;
                while c != 0 {
                    let gm = m.u32(c + grp::MODEL);
                    if m.u32(c + grp::PARENT) == 0 && gm != 0 {
                        let rt = m.u32(root_slot);
                        o.si_model_create_parentship(m, rt, gm);
                    }
                    c = m.u32(c + grp::NEXT);
                }
            }
        }
        clean_hierarchy(m, o, root_slot);
        let rt = m.u32(root_slot);
        if rt != 0 {
            let old = m.u32(rt + model::NAME);
            if old != 0 {
                m.free(old);
                m.set_u32(rt + model::NAME, 0);
            }
            let hn = m.u32(w);
            let nm = m.strdup(hn);
            m.set_u32(rt + model::NAME, nm);
        }
        m.u32(root_slot)
    })();
    m.release(0x100);
    m.release(4);
    m.release(4);
    r
}


/// [soft 0x081c34d4 readLine]: the next line of a stream as a malloc'd string, kept whole
/// however long (the buffer grows by 1000 bytes while the text read has neither a newline nor
/// a form feed and the stream is not at its end). 0 when memory is out. As in the original,
/// the piece appended after a growth is whatever `fgets` leaves in its 1000-byte stack
/// buffer: when the stream ends exactly at a piece, `fgets` gives null and the buffer still
/// holds the previous piece (here the stack bytes are zero at first, the original's are not
/// known; not reproduced).
pub fn read_line(m: &mut Mem, o: &mut dyn Outside, file: u32) -> u32 {
    let tmp = m.local(1000);
    let r = (|| {
        let mut buf = m.si_malloc(1000);
        if buf == 0 {
            oom(m, o, MSG_OOM_READ_LINE_A);
            return 0;
        }
        if o.fgets(m, buf, 1000, file) == 0 {
            m.set_u8(buf, 0);
        }
        let mut cap = 2000u32;
        loop {
            if clib::strrchr(m, buf, b'\n') != 0 || clib::strrchr(m, buf, 0x0c) != 0 || o.feof(m, file) != 0 {
                return buf;
            }
            let bigger = m.si_malloc(cap);
            if bigger == 0 {
                oom(m, o, MSG_OOM_READ_LINE_B);
                if buf != 0 {
                    m.free(buf);
                }
                return 0;
            }
            clib::strcpy(m, bigger, buf);
            if buf != 0 {
                m.free(buf);
            }
            o.fgets(m, tmp, 1000, file);
            clib::strcat(m, bigger, tmp);
            buf = bigger;
            cap = cap.wrapping_add(1000);
        }
    })();
    m.release(1000);
    r
}


const MSG_MTL_OOM: u32 = 0x0869_1e2d;
const MTL_FMT_TOKEN: u32 = 0x0869_1e2a;
const MTL_FMT_NAME: u32 = 0x0869_1e3c;
const MTL_BAD_NAME: u32 = 0x0869_1e3f;
/// `Ka`, `Kd`, `Ks`: (format, message, offset of the first float).
const MTL_K: [(u32, u32, u32); 3] = [(0x0869_1e5b, 0x0869_1e64, 0x34), (0x0869_1e7f, 0x0869_1e88, 0x44), (0x0869_1ea4, 0x0869_1ead, 0x54)];
const MTL_ILLUM: (u32, u32) = (0x0869_1ec9, 0x0869_1ecc);
const MTL_N: [(u32, u32, u32); 2] = [(0x0869_1f33, 0x0869_1f36, 0x64), (0x0869_1f52, 0x0869_1f55, 0x6dc)];
const MSG_MTL_SHADING: u32 = 0x0869_1f00;

/// [soft 0x081c4ff8 readMaterialFile]: the materials of a `.mtl` stream added to the list
/// `prev`: read token by token with `fscanf("%s")`, the first letter deciding: `n...` (newmtl)
/// a new default material named by the next word; `Ka`, `Kd`, `Ks` three floats (+0x34, +0x44,
/// +0x54); `Ns` a float at +0x64, any other `N...` one at +0x6dc; `i...` an integer shading
/// model (0 gives 1, 1 gives 3, 5 and 7 give 4 with a warning, all others 4) at +0x1e;
/// anything else is skipped with the rest of its line (`fgets`). The new materials form a list
/// to which `prev` is appended at the end of the stream. An unreadable value, or `feof` before
/// it, prints "Invalid material file : %s." (level 3), disposes the new list and returns
/// `prev` (the `i`, `Ns` and `N` errors close the stream first). A null stream gives `prev`;
/// no material read gives 0. The original keeps the last word in its buffer when `%s` finds
/// nothing, and a statement before the first `n...` writes through an uninitialised pointer:
/// both are the original's, the second is not reproduced (the Rust has 0).
pub fn read_material_file(m: &mut Mem, o: &mut dyn Outside, name: u32, file: u32, prev: u32) -> u32 {
    if file == 0 {
        return prev;
    }
    let tok = m.local(1000);
    let head = m.local(4);
    let ival = m.local(4);
    let r = (|| {
        m.set_u32(head, 0);
        let mut cur = 0u32; // ebx: uninitialised in the original until the first `n...`
        let fail = |m: &mut Mem, o: &mut dyn Outside, msg: u32, close: bool| {
            print_message(m, o, 3, msg, &[name]);
            if close {
                o.fclose(m, file);
            }
            si_dispose_material(m, head);
            prev
        };
        loop {
            fscanf(m, o, file, MTL_FMT_TOKEN, &[tok]);
            let c = m.u8(tok);
            let idx = c.wrapping_add(0xb5) as i8 as i32;
            match if idx as u32 > 0x23 { -1 } else { idx } {
                35 => {
                    cur = allocate_default_material(m, o);
                    if cur == 0 {
                        print_message(m, o, 3, MSG_MTL_OOM, &[]);
                        si_dispose_material(m, head);
                        return prev;
                    }
                    if o.feof(m, file) != 0 || fscanf(m, o, file, MTL_FMT_NAME, &[cur]) != 1 {
                        return fail(m, o, MTL_BAD_NAME, false);
                    }
                    let h = m.u32(head);
                    if h == 0 {
                        m.set_u32(head, cur);
                    } else {
                        let mut t = h;
                        while m.u32(t + MAT_NEXT) != 0 {
                            t = m.u32(t + MAT_NEXT);
                        }
                        m.set_u32(t + MAT_NEXT, cur);
                    }
                }
                0 => {
                    let k = match m.u8(tok + 1) {
                        b'a' => MTL_K[0],
                        b'd' => MTL_K[1],
                        _ => MTL_K[2],
                    };
                    if o.feof(m, file) != 0 || fscanf(m, o, file, k.0, &[cur + k.2, cur + k.2 + 4, cur + k.2 + 8]) != 3 {
                        return fail(m, o, k.1, false);
                    }
                }
                30 => {
                    if o.feof(m, file) != 0 || fscanf(m, o, file, MTL_ILLUM.0, &[ival]) != 1 {
                        return fail(m, o, MTL_ILLUM.1, true);
                    }
                    let v = m.i32(ival);
                    let shade = match v {
                        1 => 3,
                        0 => 1,
                        5 | 7 => {
                            print_message(m, o, 2, MSG_MTL_SHADING, &[cur]);
                            4
                        }
                        _ => 4,
                    };
                    m.set_u16(cur + 0x1e, shade);
                }
                3 => {
                    let k = if m.u8(tok + 1) == b's' { MTL_N[0] } else { MTL_N[1] };
                    if o.feof(m, file) != 0 || fscanf(m, o, file, k.0, &[cur + k.2]) != 1 {
                        return fail(m, o, k.1, true);
                    }
                }
                _ => {}
            }
            o.fgets(m, tok, 1000, file);
            if o.feof(m, file) != 0 {
                let h = m.u32(head);
                if h == 0 {
                    return 0;
                }
                let mut t = h;
                while m.u32(t + MAT_NEXT) != 0 {
                    t = m.u32(t + MAT_NEXT);
                }
                m.set_u32(t + MAT_NEXT, prev);
                return h;
            }
        }
    })();
    m.release(4);
    m.release(4);
    m.release(1000);
    r
}

// ---------------------------------------------------------------- smoothing

/// [soft 0x081c5d80 checkSmoothingGroup]: for each edge record of the mesh (from the second,
/// 0x14 bytes each in blocks of 1024 behind the table at +8; the count at +0x1c) whose two
/// vertex numbers (+8, +0xc) are not negative, shifted by `base`: if exactly one of the
/// smoothing groups (taken in order, stopping at the first that has any) holds one of the
/// two numbers once, the edge gets flag 0x18 (+0x12). Then the mesh's vertex normals are
/// recomputed (`SI_meshComputesVertexNormal`, answered by the hook).
pub fn check_smoothing_group(m: &mut Mem, o: &mut dyn Outside, lists: u32, mesh: u32, base: i32) {
    let mut i: u32 = 1;
    if 1 < m.i32(mesh + 0x1c) {
        loop {
            let blk = (i as i32) >> 10;
            let at = i & 0x3ff;
            i += 1;
            let table = m.u32(mesh + 8);
            let e = m.u32(table.wrapping_add(4 * blk as u32)).wrapping_add(at * 0x14);
            if m.i32(e + 8) >= 0 && m.i32(e + 0xc) >= 0 {
                let a = m.i32(e + 8).wrapping_add(base);
                let b = m.i32(e + 0xc).wrapping_add(base);
                let mut s = lists;
                if lists != 0 {
                    loop {
                        let mut hits = 0;
                        let mut k: i32 = 0;
                        if 0 < m.i32(s + grp::COUNT) {
                            let arr = m.u32(s + grp::LIST);
                            let mut v = m.i32(arr);
                            while v <= a || v <= b {
                                let x = m.i32(arr.wrapping_add(4 * k as u32));
                                if x == a || x == b {
                                    hits += 1;
                                }
                                k += 1;
                                if m.i32(s + grp::COUNT) <= k {
                                    break;
                                }
                                v = m.i32(arr.wrapping_add(4 * k as u32));
                            }
                        }
                        if hits == 1 {
                            m.set_u16(e + 0x12, 0x18);
                        }
                        s = m.u32(s + grp::SMOOTH_NEXT);
                        if s == 0 || hits >= 1 {
                            break;
                        }
                    }
                }
            }
            if i as i32 >= m.i32(mesh + 0x1c) {
                break;
            }
        }
    }
    o.si_mesh_computes_vertex_normal(m, mesh, -1);
}


// ---------------------------------------------------------------- the reader

/// The two stream variables the original keeps in its data: `inf` (the `.obj` stream) and `f`
/// (the material file's).
const WF_INF: u32 = 0x0878_8c04;
const WF_MTLF: u32 = 0x0878_8c08;

const S_OOM: u32 = 0x0869_1800;
const S_FMT_OBJ: u32 = 0x0869_180f;
const S_R_OBJ: u32 = 0x0869_1816;
const S_OPEN_ERR: u32 = 0x0869_1818;
const S_ENV: u32 = 0x0869_1821;
const S_R_ENV: u32 = 0x0869_182c;
const S_NO_DEFMTL: u32 = 0x0869_1840;
const S_F3_VN: u32 = 0x0869_186a;
const S_BAD_NORMAL: u32 = 0x0869_1873;
const S_F3_V: u32 = 0x0869_1894;
const S_BAD_VERTEX: u32 = 0x0869_189d;
const S_POLY3_A: u32 = 0x0869_18c0;
const S_POLY3_B: u32 = 0x0869_1900;
const S_PCT_S: u32 = 0x0869_1937;
const S_S_FMT: u32 = 0x0869_1946;
const S_BAD_SMOOTH: u32 = 0x0869_1960;
const S_MTLLIB_FMT: u32 = 0x0869_1989;
const S_R_MTL: u32 = 0x0869_198c;
const S_MTL_NOT_FOUND: u32 = 0x0869_19a0;
const S_USEMTL_FMT: u32 = 0x0869_19cd;
const S_BAD_FILE: u32 = 0x0869_19d0;
const S_SKIP_RES: u32 = 0x0869_1a00;
const S_O_FMT: u32 = 0x0869_1a26;
const S_LINE2: u32 = 0x0869_1a40;
const S_VNF: u32 = 0x0869_1a6a;
const S_PATCH16_A: u32 = 0x0869_1aa0;
const S_SPLINE_NONE: u32 = 0x0869_1ae0;
const S_PATCH16_B: u32 = 0x0869_1b20;
const S_SPLINE4: u32 = 0x0869_1b60;
const S_VNF_PATCH: u32 = 0x0869_1ba0;
const S_VNF_SPLINE: u32 = 0x0869_1be0;
const S_NO_POINT: u32 = 0x0869_1c18;
const S_VNF_POINT: u32 = 0x0869_1c2b;
const S_NAME_FMT: u32 = 0x0869_1c41;
const F32_HALF: u32 = 0x3f00_0000;

/// Whether the token at `t` is the string `s`, as the original's inlined `strcmp` over the
/// literal and its terminator says.
fn tok_is(m: &Mem, t: u32, s: &[u8]) -> bool {
    for (i, b) in s.iter().enumerate() {
        if m.u8(t.wrapping_add(i as u32)) != *b {
            return false;
        }
    }
    m.u8(t.wrapping_add(s.len() as u32)) == 0
}

/// The state `readWavefrontFile` carries between lines.
struct Rd {
    /// the entry being filled
    w: u32,
    /// the polygon / group counters and the pending material
    group_no: i32,
    poly_no: i32,
    line_no: i32,
    pending: i32,
    /// a group began since the entry was made (the word at ebp-0x41e)
    flag: bool,
    names: u32,
    lib: u32,
    inf: u32,
    cur: u32,
    smooth: u32,
}

/// [soft 0x081c1e04 readWavefrontFile]: reads `<path>.obj` into entries. `pgroups` is the
/// pointer to the group-name list, `pwf` to the entry list, `plib` to the materials used and
/// `psmooth` to the smoothing groups. Lines are read whole (`readLine`) and cut into a token
/// (up to a blank or control character) and the rest, which stays in the line buffer; the
/// token's first letter chooses: `#` a comment, `v` (`v`, `vn`; `vt` and `vp` ignored)
/// vertices and normals, `f`/`fo` a polygon (a new mesh entry unless the current one is a
/// mesh), `g` group names (a pending new entry), `s` smoothing group (`off`/`0` clears),
/// `mtllib` material files (names relative to the `.obj`'s directory), `usemtl` a material
/// by name (`default_white` when none), `o` the object name, `l` a line (joined to the last
/// one when they share an end), `p` points, `r` a skipped resolution line (2 s sleep), and
/// `cdp bsp bzp cdc` patches and splines. 1 when read to the end; 0 when memory is out, the
/// file cannot be opened or a vertex, normal or file line is invalid. See the finding.
pub fn read_wavefront_file(m: &mut Mem, o: &mut dyn Outside, path: u32, pgroups: u32, pwf: u32, plib: u32, psmooth: u32) -> u32 {
    let sp0 = m.sp();
    let r = read_wavefront_in(m, o, path, pgroups, pwf, plib, psmooth);
    m.set_sp(sp0);
    r
}

fn read_wavefront_in(m: &mut Mem, o: &mut dyn Outside, path: u32, pgroups: u32, pwf: u32, plib: u32, psmooth: u32) -> u32 {
    // the original's frame: the cells that are passed by address, then the buffers
    let names_cell = m.local(4);
    let lib_cell = m.local(4);
    let src_cell = m.local(4);
    let buf_a = m.local(0x100);
    let buf_b = m.local(0x100);
    let smooth = m.local(0x100);
    let tok = m.local(0x100);
    let vtx = m.local(16);
    m.set_u32(names_cell, 0);
    m.set_u32(smooth, 0);
    m.set_u8(smooth, 0);
    add_new_mdl(m, o, pwf);
    let w = m.u32(pwf);
    let n = clib::strlen(m, path);
    let name = m.si_malloc(n.wrapping_add(5));
    if name == 0 {
        print_message(m, o, 3, S_OOM, &[]);
        return 0;
    }
    stdio::sprintf(m, o, name, S_FMT_OBJ, &[path]);
    let inf = o.fopen(m, name, S_R_OBJ);
    m.set_u32(WF_INF, inf);
    if inf == 0 {
        let e = o.errno_location(m);
        let en = m.u32(e);
        let text = o.strerror(m, en as i32);
        print_message(m, o, 3, S_OPEN_ERR, &[name, text]);
        m.free(name);
        return 0;
    }
    m.free(name);
    let lib0 = alloc_default_wf_material(m, o);
    m.set_u32(lib_cell, lib0);
    let env = o.getenv(m, S_ENV);
    if env != 0 {
        let f = o.fopen(m, env, S_R_ENV);
        m.set_u32(WF_MTLF, f);
        if f == 0 {
            print_message(m, o, 3, S_NO_DEFMTL, &[env]);
        } else {
            let l = m.u32(lib_cell);
            let l = read_material_file(m, o, env, f, l);
            m.set_u32(lib_cell, l);
            o.fclose(m, f);
        }
    }
    let mut st = Rd { w, group_no: 0, poly_no: 0, line_no: 0, pending: -1, flag: false, names: 0, lib: 0, inf, cur: 0, smooth };
    let _ = &st.lib;
    // the exits that close the stream and drop what was read
    macro_rules! bail {
        () => {{
            o.fclose(m, st.inf);
            m.set_u32(names_cell, st.names);
            si_dispose_name_list(m, names_cell);
            si_dispose_material(m, lib_cell);
            return 0;
        }};
    }
    // the copy of the names in the cell is the truth while a call needs its address
    macro_rules! names_cell_sync {
        () => {
            m.set_u32(names_cell, st.names)
        };
    }
    loop {
        st.line_no += 1;
        if st.cur != 0 {
            m.free(st.cur);
        }
        st.cur = read_line(m, o, st.inf);
        // the token
        let mut p = st.cur;
        while m.u8(p) == b' ' {
            p = p.wrapping_add(1);
        }
        let c = m.u8(p);
        if c == 0 || is_cntrl(c) {
            m.set_u8(tok, 0);
        } else {
            let mut i = 0u32;
            loop {
                let b = m.u8(p.wrapping_add(i));
                m.set_u8(tok.wrapping_add(i), b);
                i += 1;
                let c = m.u8(p.wrapping_add(i));
                if c == b' ' || c == 0 || is_cntrl(c) {
                    break;
                }
            }
            m.set_u8(tok.wrapping_add(i), 0);
            clib::strcpy(m, st.cur, p.wrapping_add(i));
        }
        let first = m.u8(tok);
        match first {
            b'#' => {
                let mut i = 0u32;
                while m.u8(st.cur.wrapping_add(i)) == b' ' {
                    i += 1;
                }
                let c = m.u8(st.cur.wrapping_add(i));
                if c != 0 && !is_cntrl(c) {
                    clib::strcpy(m, st.cur, st.cur.wrapping_add(i));
                    let mut i = 0u32;
                    if m.u8(st.cur) != 0 {
                        loop {
                            if is_cntrl(m.u8(st.cur.wrapping_add(i))) {
                                break;
                            }
                            i += 1;
                            if m.u8(st.cur.wrapping_add(i)) == 0 {
                                break;
                            }
                        }
                    }
                    m.set_u8(st.cur.wrapping_add(i), 0);
                }
            }
            b'b' | b'c' => {
                if !(tok_is(m, tok, b"cdp") || tok_is(m, tok, b"bsp") || tok_is(m, tok, b"bzp") || tok_is(m, tok, b"cdc")) {
                    // not a patch word
                } else {
                    let kind = if tok_is(m, tok, b"cdp") || tok_is(m, tok, b"bsp") || tok_is(m, tok, b"bzp") { 1 } else { 3 };
                    let r = set_mdl_type(m, o, st.w, kind);
                    if r == 0 || st.flag {
                        names_cell_sync!();
                        st.group_no = update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
                        add_new_mdl(m, o, pwf);
                        st.w = m.u32(m.u32(m.u32(pwf)) + hdr::FIRST);
                        set_mdl_type(m, o, st.w, kind);
                        st.flag = false;
                    }
                    while read_element(m, o, st.w, st.cur) == 0 {
                        if st.cur != 0 {
                            m.free(st.cur);
                        }
                        st.cur = read_line(m, o, st.inf);
                        st.line_no += 1;
                    }
                    let data = m.u32(st.w + wf::DATA);
                    let cnt = m.i32(data + elem::COUNT);
                    if cnt == 0 {
                        let msg = if kind == 1 { S_PATCH16_A } else { S_SPLINE_NONE };
                        print_message(m, o, 2, msg, &[st.line_no as u32]);
                        reset_model(m, st.w);
                    } else if cnt == 16 || kind != 1 {
                        if cnt <= 3 && kind == 3 {
                            print_message(m, o, 2, S_SPLINE4, &[st.line_no as u32]);
                            reset_model(m, st.w);
                        } else {
                            let mut i: i32 = 0;
                            if i < cnt {
                                loop {
                                    let d = m.u32(st.w + wf::DATA);
                                    let pv = get_list_at_position(m, d, i);
                                    let v = m.i32(pv);
                                    let hd = m.u32(st.w);
                                    let verts = m.u32(hd + hdr::VERTS);
                                    if v > m.i32(verts + vl::COUNT) {
                                        let msg = if kind == 1 { S_VNF_PATCH } else { S_VNF_SPLINE };
                                        print_message(m, o, 3, msg, &[v as u32]);
                                        reset_model(m, st.w);
                                        break;
                                    }
                                    i += 1;
                                    let d = m.u32(st.w + wf::DATA);
                                    if i >= m.i32(d + elem::COUNT) {
                                        break;
                                    }
                                }
                            }
                            let d = m.u32(st.w + wf::DATA);
                            if i == m.i32(d + elem::COUNT) {
                                let aux = m.u32(st.w + wf::AUX);
                                if kind == 1 {
                                    m.set_u16(aux + 4, 4);
                                    m.set_u16(m.u32(st.w + wf::AUX) + 6, 4);
                                    m.set_u32(m.u32(st.w + wf::AUX) + 8, 0);
                                    m.set_u32(m.u32(st.w + wf::AUX) + 0xc, 0);
                                    if tok_is(m, tok, b"bzp") {
                                        m.set_u32(m.u32(st.w + wf::AUX), 1);
                                    } else if tok_is(m, tok, b"bsp") {
                                        m.set_u32(m.u32(st.w + wf::AUX), 2);
                                    } else if tok_is(m, tok, b"cdp") {
                                        let a = m.u32(st.w + wf::AUX);
                                        m.set_u32(a, 3);
                                        m.set_u32(a + 8, F32_HALF);
                                        m.set_u32(a + 0xc, F32_HALF);
                                    }
                                } else if tok_is(m, tok, b"cdc") {
                                    m.set_u32(aux, 3);
                                }
                            }
                        }
                    } else {
                        print_message(m, o, 2, S_PATCH16_B, &[st.line_no as u32]);
                        reset_model(m, st.w);
                    }
                }
            }
            b'f' => {
                if tok_is(m, tok, b"f") || tok_is(m, tok, b"fo") {
                    let r = set_mdl_type(m, o, st.w, 2);
                    if (r == 0 && m.i32(st.w + wf::KIND) != 2) || st.flag {
                        names_cell_sync!();
                        st.group_no = update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
                        add_new_mdl(m, o, pwf);
                        st.w = m.u32(m.u32(m.u32(pwf)) + hdr::FIRST);
                        set_mdl_type(m, o, st.w, 2);
                        st.flag = false;
                        if st.pending != -1 {
                            add_list_data(m, o, st.w, -1, st.pending);
                            st.pending = -1;
                        }
                    }
                    if m.u8(st.smooth) != 0 {
                        update_smoothing_group(m, o, psmooth, st.smooth, st.poly_no);
                    }
                    st.poly_no += 1;
                    loop {
                        let r = read_element(m, o, st.w, st.cur);
                        if r == 0 {
                            if st.cur != 0 {
                                m.free(st.cur);
                            }
                            st.cur = read_line(m, o, st.inf);
                            st.line_no += 1;
                        } else {
                            add_list_data(m, o, st.w, 0, 0);
                            let aux = m.u32(st.w + wf::AUX);
                            let v = m.i32(aux).wrapping_add(1);
                            m.set_i32(aux, v);
                        }
                        if r != 0 {
                            break;
                        }
                    }
                    let d = m.u32(st.w + wf::DATA);
                    if m.i32(d + elem::COUNT) <= 3 {
                        print_message(m, o, 2, S_POLY3_A, &[st.line_no as u32]);
                        m.set_i32(m.u32(st.w + wf::DATA) + elem::COUNT, 0);
                        let aux = m.u32(st.w + wf::AUX);
                        let v = m.i32(aux).wrapping_sub(1);
                        m.set_i32(aux, v);
                    } else {
                        let mut k: i32 = 2;
                        loop {
                            let d = m.u32(st.w + wf::DATA);
                            let c = m.i32(d + elem::COUNT);
                            let pe = get_list_at_position(m, d, c.wrapping_sub(k));
                            if m.i32(pe) == 0 {
                                print_message(m, o, 2, S_POLY3_B, &[st.line_no as u32]);
                                let mut blk = m.u32(st.w + wf::DATA);
                                while blk != 0 {
                                    let first = m.u32(st.w + wf::DATA);
                                    let c = m.i32(first + elem::COUNT);
                                    m.set_i32(blk + elem::COUNT, c.wrapping_sub(k).wrapping_add(1));
                                    blk = m.u32(blk + elem::NEXT);
                                }
                                let aux = m.u32(st.w + wf::AUX);
                                let v = m.i32(aux).wrapping_sub(1);
                                m.set_i32(aux, v);
                                break;
                            }
                            k += 1;
                            if k > 4 {
                                break;
                            }
                        }
                    }
                }
            }
            b'g' => {
                if tok_is(m, tok, b"g") {
                    if m.i32(st.w + wf::KIND) != 0 {
                        names_cell_sync!();
                        st.group_no = update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
                        st.flag = true;
                    }
                    m.set_u32(names_cell, st.names);
                    si_dispose_name_list(m, names_cell);
                    st.names = m.u32(names_cell);
                    let mut s = st.cur;
                    let mut got;
                    loop {
                        while m.u8(s) == b' ' {
                            s = s.wrapping_add(1);
                        }
                        let c = m.u8(s);
                        if c == 0 || is_cntrl(c) {
                            got = 0;
                        } else {
                            let node = m.si_malloc(0x54);
                            if node == 0 {
                                bail!();
                            }
                            m.set_u32(node + NAME_NEXT, 0);
                            if st.names == 0 {
                                st.names = node;
                            } else {
                                let mut t = st.names;
                                while m.u32(t + NAME_NEXT) != 0 {
                                    t = m.u32(t + NAME_NEXT);
                                }
                                m.set_u32(t + NAME_NEXT, node);
                            }
                            got = stdio::sscanf(m, o, s, S_PCT_S, &[node]);
                        }
                        loop {
                            let c = m.u8(s);
                            if c == b' ' || c == 0 {
                                break;
                            }
                            s = s.wrapping_add(1);
                        }
                        if got != 1 {
                            break;
                        }
                    }
                    let _ = &mut got;
                    if st.names == 0 {
                        let node = m.si_malloc(0x54);
                        st.names = node;
                        if node == 0 {
                            bail!();
                        }
                        for (i, b) in b"<default>\0".iter().enumerate() {
                            m.set_u8(node.wrapping_add(i as u32), *b);
                        }
                        m.set_u32(node + NAME_NEXT, 0);
                    }
                }
            }
            b'l' => {
                if tok_is(m, tok, b"l") {
                    m.set_u32(src_cell, 0);
                    let skip: i32;
                    let mut dir: i32 = 0;
                    let mut mode: i32 = 0;
                    let r = set_mdl_type(m, o, st.w, 5);
                    if (r == 0 && m.i32(st.w + wf::KIND) != 5) || st.flag {
                        names_cell_sync!();
                        st.group_no = update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
                        add_new_mdl(m, o, pwf);
                        st.w = m.u32(m.u32(m.u32(pwf)) + hdr::FIRST);
                        set_mdl_type(m, o, st.w, 5);
                        st.flag = false;
                    }
                    add_new_mdl(m, o, src_cell);
                    set_mdl_type(m, o, m.u32(src_cell), 2);
                    loop {
                        let r = read_element(m, o, m.u32(src_cell), st.cur);
                        if r == 0 {
                            if st.cur != 0 {
                                m.free(st.cur);
                            }
                            st.cur = read_line(m, o, st.inf);
                            st.line_no += 1;
                        }
                        if r != 0 {
                            break;
                        }
                    }
                    let sd = m.u32(m.u32(src_cell) + wf::DATA);
                    let cnt = m.i32(sd + elem::COUNT);
                    if cnt <= 1 {
                        print_message(m, o, 2, S_LINE2, &[st.line_no as u32]);
                        if m.i32(m.u32(st.w + wf::AUX)) == 0 {
                            reset_model(m, st.w);
                        }
                        dispose_wf_list(m, src_cell, 1);
                    } else {
                        let mut i: i32 = 0;
                        if 0 < cnt {
                            loop {
                                let sd = m.u32(m.u32(src_cell) + wf::DATA);
                                let pv = get_list_at_position(m, sd, i);
                                let v = m.i32(pv);
                                let hd = m.u32(st.w);
                                let verts = m.u32(hd + hdr::VERTS);
                                if v > m.i32(verts + vl::COUNT) {
                                    print_message(m, o, 3, S_VNF, &[v as u32]);
                                    if m.i32(m.u32(st.w + wf::AUX)) == 0 {
                                        reset_model(m, st.w);
                                    }
                                    dispose_wf_list(m, src_cell, 1);
                                    break;
                                }
                                i += 1;
                                let sd = m.u32(m.u32(src_cell) + wf::DATA);
                                if i >= m.i32(sd + elem::COUNT) {
                                    break;
                                }
                            }
                        }
                        if m.u32(src_cell) != 0 {
                            let aux = m.u32(st.w + wf::AUX);
                            let mut go_new = false;
                            if m.i32(aux) == 0 {
                                dir = 1;
                                mode = 1;
                                skip = 1;
                                go_new = true;
                            } else {
                                let ebx = m.i32(aux + 4);
                                let edi = m.i32(aux + 8);
                                let sd = m.u32(m.u32(src_cell) + wf::DATA);
                                let p0 = get_list_at_position(m, sd, 0);
                                let sd2 = m.u32(m.u32(src_cell) + wf::DATA);
                                let last = m.i32(sd2 + elem::COUNT).wrapping_sub(1);
                                let pn = get_list_at_position(m, sd2, last);
                                let first = m.i32(p0);
                                let lastv = m.i32(pn);
                                if first == edi {
                                    dir = 1;
                                    mode = 1;
                                    skip = 0;
                                } else if lastv == edi {
                                    dir = -1;
                                    mode = 1;
                                    skip = 0;
                                } else if first == ebx {
                                    dir = 1;
                                    mode = -1;
                                    skip = 0;
                                } else {
                                    if lastv == ebx {
                                        mode = -1;
                                        dir = -1;
                                    }
                                    skip = (lastv != ebx) as i32;
                                    if skip != 0 {
                                        go_new = true;
                                    }
                                }
                            }
                            if go_new && m.i32(m.u32(st.w + wf::AUX)) != 0 {
                                names_cell_sync!();
                                st.group_no = update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
                                add_new_mdl(m, o, pwf);
                                st.w = m.u32(m.u32(m.u32(pwf)) + hdr::FIRST);
                                set_mdl_type(m, o, st.w, 5);
                                st.flag = false;
                                mode = 1;
                                dir = 1;
                            }
                            copy_line(m, o, st.w, m.u32(src_cell), dir, mode, skip as i16);
                            dispose_wf_list(m, src_cell, 1);
                        }
                    }
                }
            }
            b'm' => {
                if tok_is(m, tok, b"mtllib") {
                    let mut s = st.cur;
                    loop {
                        while m.u8(s) == b' ' {
                            s = s.wrapping_add(1);
                        }
                        let c = m.u8(s);
                        let ok;
                        if c == 0 || is_cntrl(c) {
                            ok = false;
                        } else {
                            clib::strcpy(m, buf_a, path);
                            let r = clib::strrchr(m, buf_a, b'/');
                            m.set_u8(r.wrapping_add(1), 0);
                            clib::strcpy(m, buf_b, buf_a);
                            let n = stdio::sscanf(m, o, s, S_MTLLIB_FMT, &[buf_a]);
                            ok = n == 1;
                            if ok {
                                clib::strcat(m, buf_b, buf_a);
                            }
                            loop {
                                let c = m.u8(s);
                                if c == b' ' || c == 0 {
                                    break;
                                }
                                s = s.wrapping_add(1);
                            }
                        }
                        if !ok {
                            break;
                        }
                        let f = o.fopen(m, buf_b, S_R_MTL);
                        m.set_u32(WF_MTLF, f);
                        if f == 0 {
                            print_message(m, o, 2, S_MTL_NOT_FOUND, &[st.line_no as u32, buf_b]);
                        } else {
                            let l = m.u32(lib_cell);
                            let l = read_material_file(m, o, buf_b, f, l);
                            m.set_u32(lib_cell, l);
                            o.fclose(m, f);
                        }
                    }
                }
            }
            b'o' => {
                if tok_is(m, tok, b"o") {
                    let hd = m.u32(st.w);
                    stdio::sscanf(m, o, st.cur, S_O_FMT, &[hd]);
                }
            }
            b'p' => {
                if tok_is(m, tok, b"p") {
                    let r = set_mdl_type(m, o, st.w, 4);
                    if (r == 0 && m.i32(st.w + wf::KIND) != 4) || st.flag {
                        names_cell_sync!();
                        st.group_no = update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
                        add_new_mdl(m, o, pwf);
                        st.w = m.u32(m.u32(m.u32(pwf)) + hdr::FIRST);
                        set_mdl_type(m, o, st.w, 4);
                        st.flag = false;
                    }
                    let start = m.i32(m.u32(st.w + wf::DATA) + elem::COUNT);
                    loop {
                        let r = read_element(m, o, st.w, st.cur);
                        if r == 0 {
                            if st.cur != 0 {
                                m.free(st.cur);
                            }
                            st.cur = read_line(m, o, st.inf);
                            st.line_no += 1;
                        }
                        if r != 0 {
                            break;
                        }
                    }
                    let d = m.u32(st.w + wf::DATA);
                    if m.i32(d + elem::COUNT) == 0 {
                        print_message(m, o, 2, S_NO_POINT, &[st.line_no as u32]);
                        reset_model(m, st.w);
                    } else {
                        let mut i = start;
                        loop {
                            let d = m.u32(st.w + wf::DATA);
                            if i >= m.i32(d + elem::COUNT) {
                                break;
                            }
                            let pv = get_list_at_position(m, d, i);
                            let v = m.i32(pv);
                            let hd = m.u32(st.w);
                            let verts = m.u32(hd + hdr::VERTS);
                            if v > m.i32(verts + vl::COUNT) {
                                print_message(m, o, 3, S_VNF_POINT, &[v as u32]);
                                reset_model(m, st.w);
                                break;
                            }
                            i += 1;
                        }
                    }
                }
            }
            b'r' => {
                print_message(m, o, 2, S_SKIP_RES, &[st.line_no as u32, st.cur]);
                o.sleep(m, 2);
            }
            b's' => {
                if tok_is(m, tok, b"s") {
                    let n = stdio::sscanf(m, o, st.cur, S_S_FMT, &[smooth]);
                    if n == 1 {
                        if tok_is(m, smooth, b"off") || tok_is(m, smooth, b"0") {
                            m.set_u8(smooth, 0);
                        }
                    } else {
                        print_message(m, o, 3, S_BAD_SMOOTH, &[st.line_no as u32]);
                    }
                }
            }
            b'u' => {
                if tok_is(m, tok, b"usemtl") {
                    let n = stdio::sscanf(m, o, st.cur, S_USEMTL_FMT, &[buf_a]);
                    if n > 1 {
                        print_message(m, o, 3, S_BAD_FILE, &[st.line_no as u32]);
                        bail!();
                    }
                    if n < 1 {
                        for (i, b) in DEFAULT_WHITE.iter().enumerate() {
                            m.set_u8(buf_a.wrapping_add(i as u32), *b);
                        }
                    }
                    let lib = m.u32(lib_cell);
                    let id = add_material(m, o, lib, buf_a, plib);
                    let kind = m.i32(st.w + wf::KIND);
                    st.pending = id;
                    if kind != 0 && kind != 3 && kind != 4 && kind != 5 && 0 < id {
                        add_list_data(m, o, st.w, -1, id);
                        st.pending = -1;
                    }
                    let hd = m.u32(st.w);
                    m.set_i32(hd + 0x54, id);
                }
            }
            b'v' => {
                if tok_is(m, tok, b"vn") {
                    let n = stdio::sscanf(m, o, st.cur, S_F3_VN, &[vtx, vtx + 4, vtx + 8]);
                    if n != 3 {
                        print_message(m, o, 3, S_BAD_NORMAL, &[st.line_no as u32]);
                        bail!();
                    }
                    let hd = m.u32(st.w);
                    add_vertex(m, o, hd + hdr::NORMALS, vtx);
                } else if tok_is(m, tok, b"vt") || tok_is(m, tok, b"vp") {
                } else if tok_is(m, tok, b"v") {
                    let n = stdio::sscanf(m, o, st.cur, S_F3_V, &[vtx, vtx + 4, vtx + 8]);
                    if n != 3 {
                        print_message(m, o, 3, S_BAD_VERTEX, &[st.line_no as u32]);
                        bail!();
                    }
                    m.set_u32(vtx + 12, 0xffff_ffff);
                    let hd = m.u32(st.w);
                    add_vertex(m, o, hd + hdr::VERTS, vtx);
                }
            }
            _ => {}
        }
        if o.feof(m, st.inf) != 0 {
            break;
        }
    }
    if st.cur != 0 {
        m.free(st.cur);
    }
    names_cell_sync!();
    update_group_name(m, o, pgroups, st.names, m.i32(st.w + wf::INDEX), st.group_no);
    o.fclose(m, st.inf);
    m.set_u32(names_cell, st.names);
    si_dispose_name_list(m, names_cell);
    si_dispose_material(m, lib_cell);
    let hd = m.u32(m.u32(pwf));
    if m.u8(hd) == 0 {
        let r = clib::strrchr(m, path, b'/');
        stdio::sprintf(m, o, hd, S_NAME_FMT, &[r.wrapping_add(1)]);
    }
    1
}

/// [soft 0x081c1d40 si_objectReadWavefront]: reads the `.obj` of that name (`readWavefrontFile`)
/// and, when it gave an entry list whose header has a name, cleans the group names and
/// converts the entries to models (`convertWFModel2Soft`, with the given order); then drops
/// the materials, group names and entry list. What the conversion answered (0 when nothing
/// was converted).
pub fn si_object_read_wavefront(m: &mut Mem, o: &mut dyn Outside, path: u32, order: i16) -> u32 {
    let groups = m.local(4);
    let wfc = m.local(4);
    let lib = m.local(4);
    let smooth = m.local(4);
    m.set_u32(groups, 0);
    m.set_u32(lib, 0);
    m.set_u32(smooth, 0);
    m.set_u32(wfc, 0);
    let mut result = 0;
    let ok = read_wavefront_file(m, o, path, groups, wfc, lib, smooth);
    if ok != 0 {
        let hd = m.u32(m.u32(wfc));
        if m.u8(hd) != 0 {
            let g = clean_group_name(m, o, m.u32(groups));
            m.set_u32(groups, g);
            result = convert_wf_model2soft(m, o, path, g, m.u32(wfc), m.u32(lib), m.u32(smooth), order);
        }
    }
    si_dispose_material(m, lib);
    dispose_group_name(m, groups);
    dispose_wf_list(m, wfc, 1);
    m.release(4);
    m.release(4);
    m.release(4);
    m.release(4);
    result
}
