// SPDX-License-Identifier: MIT
//! `grTriangle.c` on the image: what the renderer and the display built of a node given up
//! when its geometry changes. The touch ([`super::touch::touch_definition`]) and the
//! readjusting ([`super::readjust`]) call these through [`Outside`]; whoever wires the
//! program answers those calls with these functions.

use si_mem::{Mem, Ptr32};
use si_types::model::Model;

use super::{callee, Outside};

/// Gives up a node's render data [soft 0x085efc38 SI_removeTriangle]: nothing for a null
/// node; otherwise the render vertices (+0x22c, their count at +0x228 cleared first), the
/// block at +0x42c, the render triangles (+0x234, their count at +0x230 cleared first) and
/// the extra texture coordinates (+0x330) are freed, each pointer cleared after its block;
/// then the node's triangle mesh ([`mdl_remove_tmesh`]). When `sons` (16 bits) is not 0,
/// every son, first (+0x2c4) to last through the brothers (+0x2c0), is given up the same
/// way with its own sons. The recursion is Rust recursion.
pub fn remove_triangle(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>, sons: i16) {
    if node.is_null() {
        return;
    }
    let block = m[node].ren_vertices.addr();
    m[node].ren_count0 = 0;
    if block != 0 {
        m.free(block);
        m[node].ren_vertices = Ptr32::NULL;
    }
    let block = m[node].unk_42c.addr();
    if block != 0 {
        m.free(block);
        m[node].unk_42c = Ptr32::NULL;
    }
    let block = m[node].ren_triangles.addr();
    m[node].ren_count1 = 0;
    if block != 0 {
        m.free(block);
        m[node].ren_triangles = Ptr32::NULL;
    }
    let block = m[node].extra_tcoords.addr();
    if block != 0 {
        m.free(block);
        m[node].extra_tcoords = Ptr32::NULL;
    }
    mdl_remove_tmesh(m, o, node);
    if sons == 0 {
        return;
    }
    let mut son = m[node].son;
    while !son.is_null() {
        remove_triangle(m, o, son, sons);
        son = m[son].brother;
    }
}

/// Gives up a node's triangle mesh for the display [soft 0x085f1438 SI_mdlRemoveTmesh]: when
/// the node has one (+0x3f0), its display list goes back to GL (`SI_glDeleteDispList(node)`),
/// the mesh's block at +4 is freed and cleared, then the mesh itself (the pointer read
/// again); the node's pointer is cleared once more after that, as the original does.
pub fn mdl_remove_tmesh(m: &mut Mem, o: &mut dyn Outside, node: Ptr32<Model>) {
    let tmesh = m[node].gl_list.addr();
    if tmesh == 0 {
        return;
    }
    o.call(m, &callee::SI_GL_DELETE_DISP_LIST, &[node.addr()]);
    let block = m.u32(tmesh.wrapping_add(4));
    if block != 0 {
        m.free(block);
        m.set_u32(tmesh.wrapping_add(4), 0);
    }
    let t = m[node].gl_list.addr();
    if t != 0 {
        m.free(t);
        m[node].gl_list = Ptr32::NULL;
    }
    m[node].gl_list = Ptr32::NULL;
}
