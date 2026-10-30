// SPDX-License-Identifier: MIT
//! `boolean.c` on the image: the BSP-tree CSG of polygon objects (finding 0037).
//!
//! Records, read from code (every field is named by what the routines here do with it):
//!
//! - a **face** ([`f`]), `csg_face`: the plane `a b c` (three doubles) and `d` at `+0x18`, its box
//!   (`min` at `+0x20`, `max` at `+0x38`, three doubles each), an id at `+0x50`, the result id at
//!   `+0x54`, a count at `+0x5c`, a status byte (`+0x64`: 1 unknown, 2 inside, 3 outside) and a flag
//!   byte (`+0x65`), the list of contours (`+0x68`), the two region records (`+0x6c`, `+0x70`) and the
//!   previous and next face of the list (`+0x74`, `+0x78`);
//! - a **contour** ([`c`]): the first edge node (`+0`) and the next contour (`+8`);
//! - an **edge node** ([`n`]), `edge_list`, in a circular list: a visit stamp (`+0`), which end of the
//!   edge the node starts at (`+4`, a byte), the edge (`+8`), the previous and next node (`+0xc`,
//!   `+0x10`);
//! - an **edge** ([`e`]) of 0x7c bytes: a mark (`+0`), `+4` and `+6` small flags, its two points
//!   (`+8`, `+0xc`), the face on each side (`+0x10`, `+0x14`), four attribute vectors of three
//!   doubles (`+0x18`, `+0x30`, `+0x48`, `+0x60`) and the next edge of the ring (`+0x78`);
//! - a **point** ([`p`]) of 0x5c bytes: `x y z` (doubles), a distance (`+0x48`), the plane it was
//!   last classified against (`+0x50`), a result id (`+0x54`), a counter (`+0x58`), the side
//!   (`+0x5a`) and a flag (`+0x5b`);
//! - a **tree node** ([`t`]), `bsp_tree`: a kind (`+0`, 1 = leaf), the plane (`+4`, four doubles), a
//!   leaf's inside flag (`+8`), the faces on the plane (`+0x24`) and the two subtrees (`+0x28`,
//!   `+0x2c`).
//!
//! The file keeps its own pools (points 25 to a block, edges 100 to a block, the nodes of the
//! intersection lists) in blocks of the allocator `si_getMem`; their roots are the globals below.
//!
//! The routines that run on the stack and read stack words they never wrote (marked where it
//! happens) are written with those words as zeros.

use si_mem::{globals, Mem, Outside, Routine};
use si_x87::{x, Round, F80};

globals! {
    /// 0x086b9800: set by the classification when a face changed.
    CHANGE_STATUS: u8 = 0x086b9800, "changeStatus", init 0;
    CLASSIFYING: u8 = 0x086b9801, "classifying", init 0;
    CUR_NB_EDGES: i32 = 0x086b9804, "curNbEdges", init 0;
    CUR_NB_POINTS: i32 = 0x086b9808, "curNbPoints", init 0;
    HEAD_MEM_BLK: u32 = 0x086b980c, "headMemBlk", init 0;
    TAIL_MEM_BLK: u32 = 0x086b9810, "tailMemBlk", init 0;
    CUR_BLOCK: u32 = 0x086b9814, "curBlock", init 0;
    HEAD_EDGE_TABLE: u32 = 0x086b9818, "headEdgeTable", init 0;
    TAIL_EDGE_TABLE: u32 = 0x086b981c, "tailEdgeTable", init 0;
    LAST_EDGE: i32 = 0x086b9820, "lastEdge", init 0x64;
    FIRST_INTER_LIST: u32 = 0x086b9824, "firstInterList", init 0;
    LAST_INTER_LIST: u32 = 0x086b9828, "lastInterList", init 0;
    HEAD_POINT_TABLE: u32 = 0x086b982c, "headPointTable", init 0;
    TAIL_POINT_TABLE: u32 = 0x086b9830, "tailPointTable", init 0;
    LAST_POINT: i32 = 0x086b9834, "lastPoint", init 0x19;
    MEM_LOC: i32 = 0x086b9838, "memLoc", init 0x20000;
    NB_RES_FACES: i32 = 0x086b983c, "nbResFaces", init 0;
    NB_RES_PTS: i32 = 0x086b9840, "nbResPts", init 0;
    NB_PL: i32 = 0x086b9844, "nbPl", init 0;
    REMOVE_EDGES: u8 = 0x086b9848, "removeEdges", init 0;
    REMOVE_POINTS: u8 = 0x086b9849, "removePoints", init 0;
    TOTAL_SIZE: i32 = 0x086b984c, "totalSize", init 0;
    VISIT_CNT: u16 = 0x086b9850, "visitCnt", init 1;
    COUNT_291: i32 = 0x086b9854, "count.291", init 0x64;
    FID_437: i32 = 0x086b9858, "fid.437", init 1;
    /// The cursor `getInterListNode` keeps between calls (`cur.313`).
    CUR_313: u32 = 0x087ba880, "cur.313";
    /// The `jmp_buf` `fatalError` and `si_getMem` jump through.
    JMP_ENV: [u8; 0x9c] = 0x087ba8a0, "jmpEnv";
    /// The merge distance (squared) of `si_setMergeDist`.
    MERGE_DIST: f64 = 0x087ba940, "mergeDist";
    /// The operation (1 union, 2 intersection, 3 difference).
    OPER: i32 = 0x087ba948, "oper";
    /// The `d` slot of the three template planes `SI_meshCutPlanes` copies from (x, y, z), written before each copy.
    CUT_PLANE_D_X: f64 = 0x086b99b8, "cutPlaneDx";
    CUT_PLANE_D_Y: f64 = 0x086b99d8, "cutPlaneDy";
    CUT_PLANE_D_Z: f64 = 0x086b99f8, "cutPlaneDz";
    /// The planes `classifyPoint` meets on its way down, 4 doubles each (50 at most).
    PL_LIST: [u8; 0x640] = 0x087ba960, "plList";
}

/// `longjmp(jmpEnv, 1)` through the binary's import stub.
pub const LONGJMP: Routine = Routine::new("longjmp", 0x080c_4458);
/// The C library's `fprintf`, through the binary's stub.
pub const FPRINTF: Routine = Routine::new("fprintf", 0x080c_3b88);
/// `stderr` (the pointer variable).
pub const STDERR: u32 = 0x0878_0c88;
/// `"get_mem : No more dynamic memory available !!!\n"`.
pub const NO_MORE_MEMORY: u32 = 0x086b_9860;
/// `si_createEdgeNode (0x083170ec)`: a new edge node.
pub const SI_CREATE_EDGE_NODE: Routine = Routine::new("si_createEdgeNode", 0x0831_70ec);
/// `si_createContour (0x08317118)`.
pub const SI_CREATE_CONTOUR: Routine = Routine::new("si_createContour", 0x0831_7118);
/// `si_createBoolFace (0x08317144)`.
pub const SI_CREATE_BOOL_FACE: Routine = Routine::new("si_createBoolFace", 0x0831_7144);
/// `si_createFaceList (0x08317234)`.
pub const SI_CREATE_FACE_LIST: Routine = Routine::new("si_createFaceList", 0x0831_7234);
/// `si_createRegBspNode (0x08317208)`.
pub const SI_CREATE_REG_BSP_NODE: Routine = Routine::new("si_createRegBspNode", 0x0831_7208);
/// `si_createFaceGroup (0x083170d0)`.
pub const SI_CREATE_FACE_GROUP: Routine = Routine::new("si_createFaceGroup", 0x0831_70d0);
/// `SI_nodeSetUV (0x08453538)`.
pub const SI_NODE_SET_UV: Routine = Routine::new("SI_nodeSetUV", 0x0845_3538);

/// Face fields.
pub mod f {
    pub const D: u32 = 0x18;
    pub const MIN: u32 = 0x20;
    pub const MAX: u32 = 0x38;
    pub const ID: u32 = 0x50;
    pub const RES_ID: u32 = 0x54;
    pub const COUNT: u32 = 0x5c;
    pub const STATUS: u32 = 0x64;
    pub const FLAG: u32 = 0x65;
    pub const CONTOURS: u32 = 0x68;
    pub const REG_A: u32 = 0x6c;
    pub const REG_B: u32 = 0x70;
    pub const PREV: u32 = 0x74;
    pub const NEXT: u32 = 0x78;
}
/// Contour fields.
pub mod c {
    pub const FIRST: u32 = 0;
    pub const NEXT: u32 = 8;
}
/// Edge-node fields.
pub mod n {
    pub const VISIT: u32 = 0;
    pub const END: u32 = 4;
    pub const EDGE: u32 = 8;
    pub const PREV: u32 = 0xc;
    pub const NEXT: u32 = 0x10;
}
/// Edge fields.
pub mod e {
    pub const MARK: u32 = 0;
    pub const PT: u32 = 8;
    pub const FACE: u32 = 0x10;
    pub const ATTR: u32 = 0x18;
    pub const NEXT: u32 = 0x78;
    pub const SIZE: u32 = 0x7c;
}
/// Point fields.
pub mod p {
    pub const DIST: u32 = 0x48;
    pub const PLANE: u32 = 0x50;
    pub const RES_ID: u32 = 0x54;
    pub const CNT: u32 = 0x58;
    pub const SIDE: u32 = 0x5a;
    pub const FLAG: u32 = 0x5b;
    pub const SIZE: u32 = 0x5c;
}
/// Tree-node fields.
pub mod t {
    pub const KIND: u32 = 0;
    pub const PLANE: u32 = 4;
    pub const INSIDE: u32 = 8;
    pub const FACES: u32 = 0x24;
    pub const BACK: u32 = 0x28;
    pub const FRONT: u32 = 0x2c;
}

/// A double as the x87 holds it after a load.
pub(crate) fn d(m: &Mem, a: u32) -> F80 {
    x(m.f64(a))
}

/// `-v` on the bits (what `fchs` and a store do; exact for every value).
pub(crate) fn neg_at(m: &mut Mem, a: u32) {
    let v = m.u64(a) ^ (1 << 63);
    m.set_u64(a, v);
}

/// `visitCnt = visitCnt + 1` (16 bits) and its value.
pub(crate) fn next_visit(m: &mut Mem) -> u32 {
    let v = m.u16(VISIT_CNT.addr()).wrapping_add(1);
    m.set_u16(VISIT_CNT.addr(), v);
    v as u32
}

/// The point at the start of an edge node: `edge->pt[node->end]`.
pub(crate) fn node_pt(m: &Mem, node: u32) -> u32 {
    let edge = m.u32(node + n::EDGE);
    m.u32(edge + e::PT + m.u8(node + n::END) as u32 * 4)
}

/// `fatalError(int, int, sigcontext *)` [soft 0x0830c990 fatalError]: `longjmp(jmpEnv, 1)`.
pub fn fatal_error(m: &mut Mem, o: &mut dyn Outside) {
    o.call(m, LONGJMP, &[JMP_ENV.addr(), 1]);
}

/// `si_getMem(size)` [soft 0x08315180 si_getMem]: memory from the file's own blocks of 0x20000
/// bytes (a bigger request gets a block to itself), rounded up to 8. When the block cannot be
/// had it prints and jumps out; as `longjmp` does not return, a returning outside world gets 0.
pub fn si_get_mem(m: &mut Mem, o: &mut dyn Outside, size: i32) -> u32 {
    let mut size = size;
    if size & 7 != 0 {
        if size < 0 {
            size += 7;
        }
        size = (size & !7) + 8;
    }
    if 0x1ffff < size.wrapping_add(m[MEM_LOC]) {
        let n = if 0x20000 < size { size } else { 0x20000 };
        m[TOTAL_SIZE] = m[TOTAL_SIZE].wrapping_add(n);
        let blk = m.si_malloc(n as u32);
        m[CUR_BLOCK] = blk;
        if blk == 0 {
            let err = m.u32(STDERR);
            o.call(m, FPRINTF, &[err, NO_MORE_MEMORY]);
            o.call(m, LONGJMP, &[JMP_ENV.addr(), 1]);
            return 0;
        }
        m.set_u32(blk, blk);
        m.set_u32(blk + 4, 0);
        m.set_u32(blk + 8, 0);
        let mut head = blk;
        if m[HEAD_MEM_BLK] != 0 {
            let tail = m[TAIL_MEM_BLK];
            m.set_u32(tail + 8, blk);
            m.set_u32(blk + 4, tail);
            head = m[HEAD_MEM_BLK];
        }
        m[HEAD_MEM_BLK] = head;
        m[MEM_LOC] = 0x10;
        m[TAIL_MEM_BLK] = blk;
    }
    let at = m[MEM_LOC];
    m[MEM_LOC] = size.wrapping_add(at);
    (at as u32).wrapping_add(m[CUR_BLOCK])
}

/// `getPoint()` [soft 0x0830c9a8 getPoint]: the next free point of the pool (25 to a block of
/// 0x934 bytes; a new block first closes the box of the last one).
pub fn get_point(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    m[LAST_POINT] += 1;
    if 0x19 < m[LAST_POINT] {
        let blk = si_get_mem(m, o, 0x934);
        m.set_u32(blk + 0x930, 0);
        m.set_u32(blk + 0x92c, 0);
        let tail = m[TAIL_POINT_TABLE];
        if tail != 0 {
            // the box of the block: min at +0, max at +0x18, from its 25 points
            m.copy(tail, tail + 0x30, 0x18);
            m.copy(tail + 0x18, tail + 0x30, 0x18);
            for k in 1..0x19u32 {
                let pt = tail + 0x30 + k * p::SIZE;
                for ax in 0..3u32 {
                    let v = m.f64(pt + ax * 8);
                    if v < m.f64(tail + ax * 8) {
                        m.set_f64(tail + ax * 8, v);
                    } else if m.f64(tail + 0x18 + ax * 8) < v {
                        m.set_f64(tail + 0x18 + ax * 8, v);
                    }
                }
            }
        }
        let mut head = blk;
        if m[HEAD_POINT_TABLE] != 0 {
            m.set_u32(tail + 0x930, blk);
            m.set_u32(blk + 0x92c, tail);
            head = m[HEAD_POINT_TABLE];
        }
        m[HEAD_POINT_TABLE] = head;
        m[TAIL_POINT_TABLE] = blk;
        for k in 0..0x19u32 {
            let pt = blk + 0x30 + k * p::SIZE;
            m.set_u64(pt, 0);
            m.set_u64(pt + 8, 0);
            m.set_u64(pt + 0x10, 0);
            m.set_u64(pt + p::DIST, 0);
            m.set_i32(pt + p::PLANE, -1);
            m.set_u16(pt + p::CNT, 0);
            m.set_u32(pt + p::RES_ID, 0);
            m.set_u8(pt + p::SIDE, 0xff);
            m.set_u8(pt + p::FLAG, 0);
        }
        m[LAST_POINT] = 1;
    }
    m[CUR_NB_POINTS] += 1;
    // tail + lastPoint * 0x5c - 0x2c
    m[TAIL_POINT_TABLE]
        .wrapping_add((m[LAST_POINT] as u32).wrapping_mul(p::SIZE))
        .wrapping_sub(0x2c)
}

/// `findPointId(n)` [soft 0x0830cec4 findPointId]: the n-th point of the pool.
pub fn find_point_id(m: &mut Mem, n: i32) -> u32 {
    let mut blk = m[HEAD_POINT_TABLE];
    let hops = (n / 0x19) as u32;
    if hops != 0 {
        let mut k = 0u32;
        loop {
            blk = m.u32(blk + 0x930);
            k += 1;
            if k >= hops {
                break;
            }
        }
    }
    blk.wrapping_add(0x30).wrapping_add(((n % 0x19) as u32).wrapping_mul(p::SIZE))
}

/// `getEdge()` [soft 0x0830cf10 getEdge]: the next free edge of the pool (100 to a block of 0x3078
/// bytes).
pub fn get_edge(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    m[LAST_EDGE] += 1;
    if 100 < m[LAST_EDGE] {
        let blk = si_get_mem(m, o, 0x3078);
        m.set_u32(blk + 0x3074, 0);
        m.set_u32(blk + 0x3070, 0);
        let mut head = blk;
        if m[HEAD_EDGE_TABLE] != 0 {
            let tail = m[TAIL_EDGE_TABLE];
            m.set_u32(tail + 0x3074, blk);
            m.set_u32(blk + 0x3070, tail);
            head = m[HEAD_EDGE_TABLE];
        }
        m[HEAD_EDGE_TABLE] = head;
        m[TAIL_EDGE_TABLE] = blk;
        for k in 0..100u32 {
            let ed = blk + k * e::SIZE;
            m.set_u8(ed + 6, 0);
            m.set_u16(ed + 4, 0);
            m.set_u32(ed, 0);
            m.set_u32(ed + 8, 0);
            m.set_u32(ed + 0xc, 0);
            m.set_u32(ed + 0x10, 0);
            m.set_u32(ed + 0x14, 0);
            m.set_u32(ed + e::NEXT, ed);
        }
        m[LAST_EDGE] = 1;
    }
    m[CUR_NB_EDGES] += 1;
    m[TAIL_EDGE_TABLE]
        .wrapping_add((m[LAST_EDGE] as u32).wrapping_mul(e::SIZE))
        .wrapping_sub(e::SIZE)
}

/// `findEdgeId(n)` [soft 0x0830cfe4 findEdgeId]: the n-th edge of the pool.
pub fn find_edge_id(m: &mut Mem, n: i32) -> u32 {
    let mut blk = m[HEAD_EDGE_TABLE];
    let hops = (n / 100) as u32;
    if hops != 0 {
        let mut k = 0u32;
        loop {
            blk = m.u32(blk + 0x3074);
            k += 1;
            if k >= hops {
                break;
            }
        }
    }
    blk.wrapping_add(((n % 100) as u32).wrapping_mul(e::SIZE))
}

/// `resetEdgeId()` [soft 0x0830d02c resetEdgeId]: clears the mark of every edge of the pool.
pub fn reset_edge_id(m: &mut Mem) {
    let mut blk = m[HEAD_EDGE_TABLE];
    while blk != 0 {
        for k in 0..100u32 {
            m.set_u32(blk + k * e::SIZE, 0);
        }
        blk = m.u32(blk + 0x3074);
    }
}

/// `nbResFeatures(face)` [soft 0x0830d064 nbResFeatures]: numbers the face and the points of its
/// contours (the complement of the running count).
pub fn nb_res_features(m: &mut Mem, face: u32) {
    let id = !m[NB_RES_FACES];
    m[NB_RES_FACES] += 1;
    m.set_i32(face + f::RES_ID, id);
    let mut con = m.u32(face + f::CONTOURS);
    while con != 0 {
        let first = m.u32(con + c::FIRST);
        if first != 0 {
            let mut node = first;
            loop {
                let pt = node_pt(m, node);
                if m.u32(pt + p::RES_ID) == 0 {
                    let id = !m[NB_RES_PTS];
                    m[NB_RES_PTS] += 1;
                    m.set_i32(pt + p::RES_ID, id);
                }
                node = m.u32(node + n::NEXT);
                if node == first {
                    break;
                }
            }
        }
        con = m.u32(con + c::NEXT);
    }
}

/// `extractFace(face)` [soft 0x0830d0d4 extractFace]: cuts the face out of its list; its old next.
pub fn extract_face(m: &mut Mem, face: u32) -> u32 {
    m.set_u32(face + f::PREV, 0);
    let next = m.u32(face + f::NEXT);
    m.set_u32(face + f::NEXT, 0);
    next
}

/// `resetFace(face)` [soft 0x0830d0f4 resetFace]: marks the face outside (status 3), negates its
/// flag and id, and clears its side of every edge it holds.
pub fn reset_face(m: &mut Mem, face: u32) {
    m.set_u8(face + f::STATUS, 3);
    let fl = m.u8(face + f::FLAG);
    m.set_u8(face + f::FLAG, fl.wrapping_neg());
    let id = m.i32(face + f::ID);
    m.set_i32(face + f::ID, id.wrapping_neg());
    let mut con = m.u32(face + f::CONTOURS);
    while con != 0 {
        let first = m.u32(con + c::FIRST);
        let vc = next_visit(m);
        let mut node = first;
        loop {
            m.set_u32(node + n::VISIT, vc);
            let slot = m.u32(node + n::EDGE).wrapping_add(m.u8(node + n::END) as u32 * 4).wrapping_add(0x10);
            if m.u32(slot) == face {
                m.set_u32(slot, 0);
            }
            node = m.u32(node + n::NEXT);
            if node == first || m.u32(node + n::VISIT) == vc {
                break;
            }
        }
        con = m.u32(con + c::NEXT);
    }
}

/// `resetListOfFaces(face)` [soft 0x0830d154 resetListOfFaces].
pub fn reset_list_of_faces(m: &mut Mem, mut face: u32) {
    while face != 0 {
        reset_face(m, face);
        face = m.u32(face + f::NEXT);
    }
}

/// `openObject(face)` [soft 0x0830d17c openObject]: 1 when some edge of the list's faces has a
/// side without a face.
pub fn open_object(m: &mut Mem, mut face: u32) -> u32 {
    loop {
        if face == 0 {
            return 0;
        }
        let first = m.u32(m.u32(face + f::CONTOURS));
        let mut node = first;
        loop {
            let edge = m.u32(node + n::EDGE);
            if m.u32(edge + e::FACE) == 0 || m.u32(edge + e::FACE + 4) == 0 {
                return 1;
            }
            node = m.u32(node + n::NEXT);
            if node == m.u32(m.u32(face + f::CONTOURS)) {
                break;
            }
        }
        face = m.u32(face + f::NEXT);
    }
}

/// `findEdgeNode(face, edge, pcontour)` [soft 0x0830d978 findEdgeNode]: the node of `edge` in
/// the face's contours (stamping the nodes it passes); the contour it was in goes to `*pcontour`
/// unless that is null.
pub fn find_edge_node(m: &mut Mem, face: u32, edge: u32, pcontour: u32) -> u32 {
    if face == 0 {
        return 0;
    }
    let mut con = m.u32(face + f::CONTOURS);
    loop {
        let cur = con;
        if cur == 0 {
            return 0;
        }
        let mut node = m.u32(cur + c::FIRST);
        if node != 0 {
            let vc = next_visit(m);
            loop {
                m.set_u32(node + n::VISIT, vc);
                if m.u32(node + n::EDGE) == edge {
                    if pcontour != 0 {
                        m.set_u32(pcontour, cur);
                    }
                    return node;
                }
                node = m.u32(node + n::NEXT);
                if m.u32(node) == vc {
                    break;
                }
            }
        }
        con = m.u32(cur + c::NEXT);
        if cur == con {
            break;
        }
    }
    // the contour that points to itself ends the search
    let cur = con;
    m.set_u32(cur + c::NEXT, 0);
    0
}

/// `markSelectedFace(face, tree, which)` [soft 0x0830ee8c markSelectedFace].
pub fn mark_selected_face(m: &mut Mem, mut face: u32, tree: u32, which: u8) {
    while face != 0 {
        let slot = if which == 0 { f::REG_A } else { f::REG_B };
        m.set_u32(face + slot, tree);
        m.set_u8(face + f::STATUS, 2);
        face = m.u32(face + f::NEXT);
    }
}

/// `findLastFaceOfList(face)` [soft 0x0830eecc findLastFaceOfList].
pub fn find_last_face_of_list(m: &mut Mem, mut face: u32) -> u32 {
    if face != 0 {
        while m.u32(face + f::NEXT) != 0 {
            face = m.u32(face + f::NEXT);
        }
    }
    face
}

/// `mergeListOfFaces(a, b)` [soft 0x0830eef4 mergeListOfFaces]: `b` after `a`; the head.
pub fn merge_list_of_faces(m: &mut Mem, a: u32, b: u32) -> u32 {
    if a != 0 && b != 0 {
        let last = find_last_face_of_list(m, a);
        m.set_u32(last + f::NEXT, b);
        m.set_u32(b + f::PREV, last);
        return a;
    }
    if a != 0 {
        a
    } else {
        b
    }
}

/// `mergeTwoListsOfFaces(a, a_last, b, b_last, plast)` [soft 0x0830ef2c mergeTwoListsOfFaces]:
/// `b` after `a`, the last face of the result to `*plast`; the head.
pub fn merge_two_lists_of_faces(m: &mut Mem, a: u32, a_last: u32, b: u32, b_last: u32, plast: u32) -> u32 {
    if a == 0 {
        if b == 0 {
            m.set_u32(plast, 0);
            return 0;
        }
        let last = if b_last == 0 { find_last_face_of_list(m, b) } else { b_last };
        m.set_u32(plast, last);
        return b;
    }
    if b == 0 {
        let last = if a_last != 0 { a_last } else { find_last_face_of_list(m, a) };
        m.set_u32(plast, last);
        return a;
    }
    let last = if a_last == 0 { find_last_face_of_list(m, a) } else { a_last };
    m.set_u32(last + f::NEXT, b);
    m.set_u32(b + f::PREV, last);
    let end = if b_last != 0 { b_last } else { find_last_face_of_list(m, b) };
    m.set_u32(plast, end);
    a
}

/// `setStatus(face, status)` [soft 0x0830f20c setStatus]: the status of every face of the list.
pub fn set_status(m: &mut Mem, mut face: u32, status: i8) {
    while face != 0 {
        m.set_i8(face + f::STATUS, status);
        face = m.u32(face + f::NEXT);
    }
}

/// `removeFaceFromList(plist, face)` [soft 0x08310374 removeFaceFromList].
pub fn remove_face_from_list(m: &mut Mem, plist: u32, face: u32) {
    if face == 0 {
        return;
    }
    if m.u32(face + f::PREV) == 0 {
        let next = m.u32(face + f::NEXT);
        if next == 0 {
            return;
        }
        m.set_u32(plist, next);
        m.set_u32(next + f::PREV, 0);
    } else {
        let prev = m.u32(face + f::PREV);
        let next = m.u32(face + f::NEXT);
        m.set_u32(prev + f::NEXT, next);
    }
    if m.u32(face + f::NEXT) != 0 {
        let next = m.u32(face + f::NEXT);
        let prev = m.u32(face + f::PREV);
        m.set_u32(next + f::PREV, prev);
    }
    m.set_u32(face + f::PREV, 0);
    m.set_u32(face + f::NEXT, 0);
}

/// `reverseFace(face)` [soft 0x0830ff44 reverseFace]: turns the face over: for every edge not yet
/// done (mark 0 becomes 1) the four attribute vectors are negated and turned round and the two
/// faces swapped; every node's end and links are swapped; the plane is negated and the regions
/// swapped.
pub fn reverse_face(m: &mut Mem, face: u32) {
    let mut con = m.u32(face + f::CONTOURS);
    while con != 0 {
        let first = m.u32(con + c::FIRST);
        let mut node = first;
        loop {
            let edge = m.u32(node + n::EDGE);
            let pt = node_pt(m, node);
            if m.u32(pt + p::PLANE) != 0 {
                m.set_u32(pt + p::PLANE, 0);
            }
            if m.u32(edge + e::MARK) == 0 {
                m.set_u32(edge + e::MARK, 1);
                // four vectors a b c d, each negated; stored as c d a b
                let mut v = [[0u64; 3]; 4];
                for (k, row) in v.iter_mut().enumerate() {
                    for (ax, w) in row.iter_mut().enumerate() {
                        *w = m.u64(edge + e::ATTR + k as u32 * 0x18 + ax as u32 * 8) ^ (1 << 63);
                    }
                }
                for (k, src) in [2usize, 3, 0, 1].into_iter().enumerate() {
                    for ax in 0..3 {
                        m.set_u64(edge + e::ATTR + k as u32 * 0x18 + ax as u32 * 8, v[src][ax]);
                    }
                }
                let fa = m.u32(edge + e::FACE);
                let fb = m.u32(edge + e::FACE + 4);
                m.set_u32(edge + e::FACE, fb);
                m.set_u32(edge + e::FACE + 4, fa);
            }
            let prev = m.u32(node + n::PREV);
            let end = m.u8(node + n::END);
            m.set_u8(node + n::END, 1u8.wrapping_sub(end));
            let next = m.u32(node + n::NEXT);
            m.set_u32(node + n::PREV, next);
            m.set_u32(node + n::NEXT, prev);
            node = next;
            if node == first {
                break;
            }
        }
        con = m.u32(con + c::NEXT);
    }
    let ra = m.u32(face + f::REG_A);
    let rb = m.u32(face + f::REG_B);
    for off in [0, 8, 0x10] {
        neg_at(m, face + off);
    }
    m.set_u32(face + f::REG_B, ra);
    m.set_u32(face + f::REG_A, rb);
    neg_at(m, face + f::D);
}

/// `reverseTree(tree)` [soft 0x08310100 reverseTree]: turns a tree inside out: a leaf's flag
/// flips; a node reverses its faces, negates its plane, reverses both subtrees and swaps them.
pub fn reverse_tree(m: &mut Mem, tree: u32) {
    if m.u32(tree + t::KIND) == 1 {
        let v = if m.u32(tree + t::INSIDE) == 0 { 1 } else { 0 };
        m.set_u32(tree + t::INSIDE, v);
        return;
    }
    let mut face = m.u32(tree + t::FACES);
    while face != 0 {
        reverse_face(m, face);
        face = m.u32(face + f::NEXT);
    }
    for off in [4, 0xc, 0x14, 0x1c] {
        neg_at(m, tree + off);
    }
    let back = m.u32(tree + t::BACK);
    reverse_tree(m, back);
    let front = m.u32(tree + t::FRONT);
    reverse_tree(m, front);
    let back = m.u32(tree + t::BACK);
    let front = m.u32(tree + t::FRONT);
    m.set_u32(tree + t::BACK, front);
    m.set_u32(tree + t::FRONT, back);
}

/// `si_setMergeDist(dist)` [soft 0x08310894 si_setMergeDist].
pub fn set_merge_dist(m: &mut Mem, dist: f64) {
    m.set_f64(MERGE_DIST.addr(), dist);
}

/// `ptIsOnPlane(tree, pt)` [soft 0x0830e680 ptIsOnPlane]: 1 when the point is a vertex of one of the
/// faces lying on the node's plane (the face list at `+0x24`, linked through `+0x78`), else 0.
pub fn pt_is_on_plane(m: &mut Mem, tree: u32, pt: u32) -> u32 {
    let mut face = m.u32(tree + t::FACES);
    while face != 0 {
        let mut con = m.u32(face + f::CONTOURS);
        while con != 0 {
            let first = m.u32(con + c::FIRST);
            if first != 0 && m.u32(first + n::EDGE) != 0 {
                let mut node = first;
                loop {
                    if node_pt(m, node) == pt {
                        return 1;
                    }
                    node = m.u32(node + n::NEXT);
                    if node == first {
                        break;
                    }
                }
            }
            con = m.u32(con + c::NEXT);
        }
        face = m.u32(face + f::NEXT);
    }
    0
}

/// `getPtPos(tree, pt)` [soft 0x0830e6fc getPtPos]: the side of the node's plane the point is on (1 on it,
/// 2 behind, 3 in front), cached in the point (plane `+0x50`, side `+0x5a`, distance `+0x48`).
/// Order of operations (x87): `((x*a + y*b) + z*c) + d` in 80 bits, compared in 80 bits against 5e-5
/// (above: 3) and -5e-5 (below: 2); the sum is stored to the distance as a double, 0.0 when on the plane.
pub fn get_pt_pos(m: &mut Mem, tree: u32, pt: u32) -> i32 {
    if m.u32(pt + p::PLANE) == tree {
        return m.u8(pt + p::SIDE) as i8 as i32;
    }
    let mut dist = F80::ZERO;
    let mut side = 1;
    if pt_is_on_plane(m, tree, pt) == 0 {
        let pl = tree + t::PLANE;
        let sum = d(m, pt) * d(m, pl) + d(m, pt + 8) * d(m, pl + 8);
        let sum = sum + d(m, pt + 0x10) * d(m, pl + 0x10);
        let sum = sum + d(m, pl + 0x18);
        if sum.gt(x(5e-5)) {
            side = 3;
            dist = sum;
        } else if sum.lt(x(-5e-5)) {
            side = 2;
            dist = sum;
        }
    }
    m.set_f64(pt + p::DIST, dist.to_f64());
    m.set_u32(pt + p::PLANE, tree);
    m.set_u8(pt + p::SIDE, side as u8);
    side
}

/// `testBox(min, max, plane)` [soft 0x0830d3d0 testBox]: where the box lies against the plane `a b c d`:
/// 3 in front (or unordered), 2 behind, 4 cut by the plane. The corner of the first pass takes, per axis,
/// `min` when 5e-5 < the coefficient, else `max`; the second pass takes the other one. A corner is summed
/// `((x*a + y*b) + z*c) + d` in 80 bits (the picked values go through double locals: exact). The first sum
/// gives 3 unless it is below 5e-5; the second gives 2 when it is at most -5e-5 or unordered, else 4.
pub fn test_box(m: &mut Mem, min: u32, max: u32, pl: u32) -> u32 {
    let c5 = x(5e-5);
    let corner = |m: &Mem, second: bool| -> F80 {
        let pick = |ax: u32| -> F80 {
            let pos = c5.lt(d(m, pl + ax * 8));
            let from = if pos == second { max } else { min };
            x(m.f64(from + ax * 8))
        };
        let (cx, cy, cz) = (pick(0), pick(1), pick(2));
        let s = cx * d(m, pl) + cy * d(m, pl + 8);
        let s = s + cz * d(m, pl + 0x10);
        s + d(m, pl + 0x18)
    };
    if !corner(m, false).lt(c5) {
        return 3;
    }
    if corner(m, true).gt(x(-5e-5)) {
        4
    } else {
        2
    }
}

/// `si_whichSide(face, tree)` [soft 0x0830e4f0 si_whichSide]: testBox of the face's box against the node's
/// plane; when that says 4 (cut) the face's first contour is walked and the sides of its points
/// (getPtPos) are merged: 1 ignored, a second different side gives 4; no usable contour gives 5.
pub fn which_side(m: &mut Mem, face: u32, tree: u32) -> u32 {
    let mut r = test_box(m, face + f::MIN, face + f::MAX, tree + t::PLANE);
    if r == 4 {
        let con = m.u32(face + f::CONTOURS);
        let first = m.u32(con + c::FIRST);
        if first == 0 || m.u32(first + n::EDGE) == 0 {
            return 5;
        }
        let mut cur = 1u32;
        let mut node = first;
        loop {
            let pt = node_pt(m, node);
            let s = get_pt_pos(m, tree, pt) as u32;
            if s != cur && s != 1 {
                let was_known = cur != 1;
                cur = s;
                if was_known {
                    return 4;
                }
            }
            node = m.u32(node + n::NEXT);
            if node == first {
                break;
            }
        }
        r = cur;
    }
    r
}

/// `locatePt(v, &index)` [soft 0x0830ccd0 locatePt]: the point of the point pool within the merge distance
/// of `v`, and its index (-1 and 0 when none). Blocks are walked from the head; a block that is not the
/// last is skipped unless `v` lies in its box (per axis `min < v` or `|v - min| < 1e-6`, and
/// `max > v` or `|v - max| < 1e-6`, differences in 80 bits); the last block holds `curNbPoints % 25`
/// points (a signed remainder; 0 means none are searched). A point matches when
/// `(dx*dx + dy*dy) + dz*dz <= mergeDist` with `d = pt - v` formed in 80 bits and not rounded.
pub fn locate_pt(m: &mut Mem, v: [f64; 3]) -> (u32, i32) {
    let eps = x(1e-6);
    let mut blk = m[HEAD_POINT_TABLE];
    let mut base = 0i32;
    let mut count = 0x19u32;
    while blk != 0 {
        let mut inside = true;
        if m.u32(blk + 0x930) == 0 {
            count = (m[CUR_NB_POINTS] % 25) as u32;
        } else {
            for ax in 0..3u32 {
                let a = x(v[ax as usize]);
                let lo = d(m, blk + ax * 8);
                if !(lo.lt(a) || (a - lo).abs().lt(eps)) {
                    inside = false;
                    break;
                }
                let hi = d(m, blk + 0x18 + ax * 8);
                if !(hi.gt(a) || (a - hi).abs().lt(eps)) {
                    inside = false;
                    break;
                }
            }
        }
        if inside {
            for k in 0..count {
                let pt = blk + 0x30 + k * p::SIZE;
                let dx = d(m, pt) - x(v[0]);
                let dy = d(m, pt + 8) - x(v[1]);
                let dz = d(m, pt + 0x10) - x(v[2]);
                let sum = dy * dy + dx * dx;
                let sum = sum + dz * dz;
                if sum.le(d(m, MERGE_DIST.addr())) {
                    return (pt, base + k as i32);
                }
            }
        }
        blk = m.u32(blk + 0x930);
        base += 0x19;
    }
    (0, -1)
}

/// `findPoint(tree, v)` [soft 0x0830cbac findPoint]: the point at `v`: with no faces on the node,
/// `locatePt` (its index goes to a stack word, dropped here); otherwise the vertex of a face on the
/// node's plane (contours then nodes, the end point of each edge node) whose squared distance
/// `(dx*dx + dy*dy) + dz*dz` (80 bits) is at most 5e-7; 0 when none.
pub fn find_point(m: &mut Mem, tree: u32, v: [f64; 3]) -> u32 {
    let mut face = m.u32(tree + t::FACES);
    if face == 0 {
        return locate_pt(m, v).0;
    }
    let eps = x(5e-7);
    while face != 0 {
        let mut con = m.u32(face + f::CONTOURS);
        while con != 0 {
            let first = m.u32(con + c::FIRST);
            if first != 0 && m.u32(first + n::EDGE) != 0 {
                let mut node = first;
                loop {
                    let pt = node_pt(m, node);
                    let dx = d(m, pt) - x(v[0]);
                    let dy = d(m, pt + 8) - x(v[1]);
                    let dz = d(m, pt + 0x10) - x(v[2]);
                    let sum = dx * dx + dy * dy;
                    let sum = sum + dz * dz;
                    if sum.le(eps) {
                        return pt;
                    }
                    node = m.u32(node + n::NEXT);
                    if node == first {
                        break;
                    }
                }
            }
            con = m.u32(con + c::NEXT);
        }
        face = m.u32(face + f::NEXT);
    }
    0
}

/// `splitEdge(tree, dist, p3, p4)` [soft 0x0830e588 splitEdge]: the point where the plane cuts the edge
/// p3-p4, `p4 - t*(p4 - p3)` with `t = dist / (((b*dy) + (a*dx)) + (c*dz))` (`t` stays in 80 bits; each
/// product is subtracted unrounded and the result rounded to a double). The point is `findPoint`ed or
/// else taken from the pool (flag `+0x5b` = 1); its distance, result id, counter are cleared, side set
/// to 1 and plane to the node. The bytes of a new point beyond x y z come from the stack in the original
/// (here: left as the pool has them; the check's stack holds zeros there).
pub fn split_edge(m: &mut Mem, o: &mut dyn Outside, tree: u32, dist: f64, p3: u32, p4: u32) -> u32 {
    let dx = (d(m, p4) - d(m, p3)).to_f64();
    let dy = (d(m, p4 + 8) - d(m, p3 + 8)).to_f64();
    let dz = (d(m, p4 + 0x10) - d(m, p3 + 0x10)).to_f64();
    let pl = tree + t::PLANE;
    let den = x(m.f64(pl + 8)) * x(dy) + x(m.f64(pl)) * x(dx);
    let den = den + x(m.f64(pl + 0x10)) * x(dz);
    let t1 = x(dist) / den;
    let v = [
        (d(m, p4) - x(dx) * t1).to_f64(),
        (d(m, p4 + 8) - x(dy) * t1).to_f64(),
        (d(m, p4 + 0x10) - t1 * x(dz)).to_f64(),
    ];
    let mut pt = find_point(m, tree, v);
    if pt == 0 {
        pt = get_point(m, o);
        // the original copies 0x5c bytes of a stack temporary: x y z, then stack words (zeros in the check)
        for (k, val) in v.iter().enumerate() {
            m.set_f64(pt + k as u32 * 8, *val);
        }
        m.set_fill_bytes(pt + 0x18, 0, 0x5c - 0x18);
        m.set_u8(pt + p::FLAG, 1);
    }
    m.set_u32(pt + p::DIST, 0);
    m.set_u32(pt + p::DIST + 4, 0);
    m.set_u32(pt + p::RES_ID, 0);
    m.set_u16(pt + p::CNT, 0);
    m.set_u8(pt + p::SIDE, 1);
    m.set_u32(pt + p::PLANE, tree);
    pt
}

/// Inter-list node fields (`inter_node`, 0x44 bytes, made by `getInterListNode`).
pub mod i {
    pub const PT: u32 = 0;
    pub const KEY: u32 = 0x1c;
    pub const TAIL: u32 = 0x2c;
    pub const HEAD: u32 = 0x38;
    pub const PREV: u32 = 0x3c;
    pub const NEXT: u32 = 0x40;
}

/// `addEdgeToInter(a, b, node)` [soft 0x0830e78c addEdgeToInter]: appends the edge node to the circular
/// list kept in `a` (head at `+0x38`) whose last node is `b+0x2c`; `b+0x2c` becomes the new node.
pub fn add_edge_to_inter(m: &mut Mem, a: u32, b: u32, node: u32) {
    if m.u32(a + i::HEAD) == 0 {
        m.set_u32(a + i::HEAD, node);
    } else {
        let tail = m.u32(b + i::TAIL);
        let after = m.u32(tail + n::NEXT);
        m.set_u32(node + n::NEXT, after);
        m.set_u32(node + n::PREV, tail);
        m.set_u32(after + n::PREV, node);
        let prev = m.u32(node + n::PREV);
        m.set_u32(prev + n::NEXT, node);
    }
    m.set_u32(b + i::TAIL, node);
}

/// `addInterNode(pfirst, phead, node, dir)` [soft 0x0830e7c4 addInterNode]: inserts the node into the list
/// sorted by `key` (ascending, `pfirst` = the first node ever, `phead` = the list head). The key is the squared
/// distance of the node's point from the first node's point, negated when the direction (unit vector, after
/// division by the length rounded to f32, when above 1e-4) points against `dir`; the first node with
/// neighbours does that test, a lone first node stores its direction into `dir` instead.
/// x87: `(dx*dx + dy*dy) + dz*dz` in 80 bits, differences unrounded; the key is stored as a double but the
/// insertion compares the 80-bit value.
pub fn add_inter_node(m: &mut Mem, pfirst: u32, phead: u32, node: u32, dir: u32) {
    let first = m.u32(pfirst);
    if first == 0 {
        m.set_u32(pfirst, node);
        m.set_u32(phead, node);
        return;
    }
    let pn = m.u32(node + i::PT);
    let pf = m.u32(first + i::PT);
    let dx = d(m, pn) - d(m, pf);
    let dy = d(m, pn + 8) - d(m, pf + 8);
    let dz = d(m, pn + 0x10) - d(m, pf + 0x10);
    let mut l = dx * dx + dy * dy;
    l = l + dz * dz;
    let s = x(l.sqrt().to_f32());
    let mut v = [dx.to_f64(), dy.to_f64(), dz.to_f64()];
    if s.abs().gt(x(1e-4)) {
        v = [(dx / s).to_f64(), (dy / s).to_f64(), (dz / s).to_f64()];
    }
    if m.u32(first + i::NEXT) == 0 && m.u32(first + i::PREV) == 0 {
        for (k, val) in v.iter().enumerate() {
            m.set_f64(dir + k as u32 * 8, *val);
        }
    } else {
        let dot = d(m, dir) * x(v[0]) + d(m, dir + 8) * x(v[1]);
        let dot = dot + d(m, dir + 0x10) * x(v[2]);
        if F80::ZERO.gt(dot) {
            l = -l;
        }
    }
    m.set_f64(node + i::KEY, l.to_f64());
    let mut prev = 0u32;
    let mut cur = m.u32(phead);
    while cur != 0 && d(m, cur + i::KEY).lt(l) {
        prev = cur;
        cur = m.u32(cur + i::NEXT);
    }
    m.set_u32(node + i::NEXT, cur);
    m.set_u32(node + i::PREV, prev);
    if cur != 0 {
        m.set_u32(cur + i::PREV, node);
    }
    if prev == 0 {
        m.set_u32(phead, node);
    } else {
        m.set_u32(prev + i::NEXT, node);
    }
}

/// `getInterListNode(flag)` [soft 0x0830e8f0 getInterListNode]: the next node of a chain of reusable
/// intersection nodes (0x44 bytes, held in cells of 0x10 bytes: node at `+0`, next cell at `+8`); a set flag
/// byte is cleared and the cursor `cur.313` rewound to `firstInterList`; at the chain's end a node and a cell
/// are made with `si_getMem` (node first). The node is reset.
pub fn get_inter_list_node(m: &mut Mem, o: &mut dyn Outside, flag: u32) -> u32 {
    if m.u8(flag) != 0 {
        m.set_u8(flag, 0);
        m[CUR_313] = m[FIRST_INTER_LIST];
    }
    inter_list_node(m, o)
}

/// `getInterListNode` with a clear flag (the callers that pass the address of a zeroed stack byte).
fn inter_list_node(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    let node;
    if m[CUR_313] == 0 {
        node = si_get_mem(m, o, 0x44);
        let cell = si_get_mem(m, o, 0x10);
        m[CUR_313] = cell;
        m.set_u32(cell + 8, 0);
        m.set_u32(cell, node);
        if m[FIRST_INTER_LIST] == 0 {
            m[FIRST_INTER_LIST] = cell;
        } else {
            let last = m[LAST_INTER_LIST];
            m.set_u32(last + 8, cell);
        }
        m[LAST_INTER_LIST] = cell;
    } else {
        node = m.u32(m[CUR_313]);
    }
    let cell = m[CUR_313];
    m[CUR_313] = m.u32(cell + 8);
    m.set_u32(node + 0x1c, 0);
    m.set_u32(node + 0x20, 0);
    m.set_u32(node, 0);
    m.set_u32(node + 0x2c, 0);
    m.set_i32(node + 0x28, -1);
    m.set_i32(node + 0x24, -1);
    m.set_u32(node + 0x38, 0);
    m.set_i32(node + 0x34, -1);
    m.set_i32(node + 0x30, -1);
    m.set_u32(node + 0x3c, 0);
    m.set_u32(node + 0x40, 0);
    node
}

/// `classifyPoint(tree, v)` [soft 0x0830f0d0 classifyPoint]: 1 unknown, 2 inside, 3 outside. A leaf answers
/// by its inside flag and `oper` (2 or 3: inverted). A node takes `((x*a + y*b) + z*c) + d` in 80 bits: below
/// -5e-5 the front subtree (`+0x2c`) decides, above 5e-5 the back (`+0x28`); otherwise (also unordered) the
/// plane is appended to `plList` (`nbPl` at most 49) and both are asked: differing answers give 1.
pub fn classify_point(m: &mut Mem, tree: u32, v: u32) -> i32 {
    let vec = [m.f64(v), m.f64(v + 8), m.f64(v + 0x10)];
    classify_vec(m, tree, vec)
}

/// `classifyPoint` on a vector held in a local (the callers keep theirs on the stack).
fn classify_vec(m: &mut Mem, tree: u32, v: [f64; 3]) -> i32 {
    if tree == 0 {
        return 1;
    }
    if m.u32(tree + t::KIND) == 1 {
        let inv = (m[OPER] as u32).wrapping_sub(2) < 2;
        let inside = m.u32(tree + t::INSIDE) != 0;
        return if inv == inside { 3 } else { 2 };
    }
    let pl = tree + t::PLANE;
    let s = x(v[0]) * d(m, pl) + x(v[1]) * d(m, pl + 8);
    let s = s + x(v[2]) * d(m, pl + 0x10);
    let s = s + d(m, pl + 0x18);
    if s.lt(x(-5e-5)) {
        let front = m.u32(tree + t::FRONT);
        return classify_vec(m, front, v);
    }
    if s.gt(x(5e-5)) {
        let back = m.u32(tree + t::BACK);
        return classify_vec(m, back, v);
    }
    let nb = m[NB_PL];
    m.copy(PL_LIST.addr().wrapping_add((nb as u32) << 5), pl, 0x20);
    m[NB_PL] = nb + 1;
    if nb + 1 > 0x31 {
        m[NB_PL] = nb;
    }
    let back = m.u32(tree + t::BACK);
    let a = classify_vec(m, back, v);
    let front = m.u32(tree + t::FRONT);
    let b = classify_vec(m, front, v);
    if a != b {
        1
    } else {
        a
    }
}

/// `computeStatus(tree, face)` [soft 0x0831019c computeStatus]: classifies the face against the tree by the
/// average of its first contour's points (each running sum stored as a double, the last sum divided
/// unrounded by the count), then, when that is neither inside nor outside, by the planes met on the way
/// (`plList`, 3 coefficients within 5e-7 of the face's plane: inside) and finally by each vertex (any inside
/// gives inside). The result of the first test is kept in a stack word the original leaves unset when
/// `nbPl` is 0 and the answer was 1 (zero here; the check does not rely on it).
pub fn compute_status(m: &mut Mem, tree: u32, face: u32) -> i32 {
    let con = m.u32(face + f::CONTOURS);
    let first = m.u32(con);
    let mut sum = [0.0f64; 3];
    let mut last = [F80::ZERO; 3];
    let mut cnt = 0i32;
    let mut node = first;
    loop {
        let pt = node_pt(m, node);
        for ax in 0..3u32 {
            last[ax as usize] = x(sum[ax as usize]) + d(m, pt + ax * 8);
            sum[ax as usize] = last[ax as usize].to_f64();
        }
        cnt += 1;
        node = m.u32(node + n::NEXT);
        if node == first {
            break;
        }
    }
    let c = x(cnt);
    let avg = [(last[0] / c).to_f64(), (last[1] / c).to_f64(), (last[2] / c).to_f64()];
    m[NB_PL] = 0;
    let r = classify_vec(m, tree, avg);
    if r == 2 || r == 3 {
        return r;
    }
    let mut res = 0;
    let nb = m[NB_PL];
    let eps = x(5e-7);
    let mut k = 0;
    while k < nb {
        let pl = PL_LIST.addr() + (k as u32) * 0x20;
        let near = |ax: u32| (d(m, pl + ax * 8) - d(m, face + ax * 8)).abs().lt(eps);
        if near(0) && near(1) && near(2) {
            return 2;
        }
        res = 3;
        k += 1;
    }
    if res == 3 {
        let con = m.u32(face + f::CONTOURS);
        let first = m.u32(con);
        let mut node = first;
        loop {
            let pt = node_pt(m, node);
            let v = [m.f64(pt), m.f64(pt + 8), m.f64(pt + 0x10)];
            if classify_vec(m, tree, v) == 2 {
                return 2;
            }
            node = m.u32(node + n::NEXT);
            if node == first {
                break;
            }
        }
    }
    res
}

/// `getNbEdges(node)` [soft 0x08310864 getNbEdges]: stamps and counts a circular list of edge nodes.
pub fn get_nb_edges(m: &mut Mem, mut node: u32) -> i32 {
    let mut cnt = 0;
    if node != 0 {
        let v = next_visit(m);
        loop {
            m.set_u32(node + n::VISIT, v);
            node = m.u32(node + n::NEXT);
            cnt += 1;
            if m.u32(node + n::VISIT) == v {
                break;
            }
        }
    }
    cnt
}

/// `deleteEdge(face, edge)` [soft 0x083107f8 deleteEdge]: unlinks the node of `edge` from the face's
/// contours (the contour's first node becomes the node's predecessor).
pub fn delete_edge(m: &mut Mem, face: u32, edge: u32) {
    let mut con = m.u32(face + f::CONTOURS);
    while con != 0 {
        let first = m.u32(con + c::FIRST);
        if first != 0 {
            let v = next_visit(m);
            let mut node = first;
            loop {
                m.set_u32(node + n::VISIT, v);
                if m.u32(node + n::EDGE) == edge {
                    let (prev, next) = (m.u32(node + n::PREV), m.u32(node + n::NEXT));
                    m.set_u32(prev + n::NEXT, next);
                    m.set_u32(next + n::PREV, prev);
                    m.set_u32(con + c::FIRST, prev);
                    return;
                }
                node = m.u32(node + n::NEXT);
                if node == m.u32(con + c::FIRST) || m.u32(node + n::VISIT) == v {
                    break;
                }
            }
        }
        con = m.u32(con + c::NEXT);
    }
}

/// `si_setClassifying(flag)` [soft 0x083108a4 si_setClassifying].
pub fn set_classifying(m: &mut Mem, flag: u8) {
    m[CLASSIFYING] = flag;
}

/// `si_setCleanup(edges, points)` [soft 0x083108b4 si_setCleanup].
pub fn set_cleanup(m: &mut Mem, edges: u8, points: u8) {
    m[REMOVE_EDGES] = edges;
    m[REMOVE_POINTS] = points;
}

/// `si_initBoolean()` [soft 0x08315298 si_initBoolean]: resets the file's pools and counters.
pub fn init_boolean(m: &mut Mem) {
    m[TOTAL_SIZE] = 0;
    m.set_u16(VISIT_CNT.addr(), 1);
    m[HEAD_MEM_BLK] = 0;
    m[TAIL_MEM_BLK] = 0;
    m[CUR_BLOCK] = 0;
    m[MEM_LOC] = 0x20000;
    m[HEAD_POINT_TABLE] = 0;
    m[TAIL_POINT_TABLE] = 0;
    m[LAST_POINT] = 0x19;
    m[CUR_NB_POINTS] = 0;
    m[CHANGE_STATUS] = 0;
    m[HEAD_EDGE_TABLE] = 0;
    m[TAIL_EDGE_TABLE] = 0;
    m[LAST_EDGE] = 100;
    m[CUR_NB_EDGES] = 0;
    m[FIRST_INTER_LIST] = 0;
    m[LAST_INTER_LIST] = 0;
}

/// `divideList(list, add, limit, keep, moved)` [soft 0x08310558 divideList]: splits the faces of `*list`
/// by their flag byte (`+0x65`): flag 0 go to `*keep`, others (count += `add`) to `*moved`; a count above
/// `limit` is reset to 0. Then every edge of the `keep` faces that has a flagged face on one side is copied
/// (`getEdge`), the copy takes the flagged face, the original the other, and the node points to the copy.
pub fn divide_list(m: &mut Mem, o: &mut dyn Outside, list: u32, add: i32, limit: i32, keep: u32, moved: u32) {
    let mut last_keep = 0u32;
    let mut last_moved = 0u32;
    let mut face = m.u32(list);
    while face != 0 {
        let next = extract_face(m, face);
        if m.u8(face + f::FLAG) == 0 {
            if m.u32(keep) == 0 {
                m.set_u32(keep, face);
            } else {
                m.set_u32(last_keep + f::NEXT, face);
                m.set_u32(face + f::PREV, last_keep);
            }
            last_keep = face;
        } else {
            m.set_i32(face + f::COUNT, m.i32(face + f::COUNT).wrapping_add(add));
            if m.u32(moved) == 0 {
                m.set_u32(moved, face);
            } else {
                m.set_u32(last_moved + f::NEXT, face);
                m.set_u32(face + f::PREV, last_moved);
            }
            last_moved = face;
        }
        if limit < m.i32(face + f::COUNT) {
            m.set_i32(face + f::COUNT, 0);
        }
        face = next;
    }
    let mut face = m.u32(keep);
    while face != 0 {
        let mut con = m.u32(face + f::CONTOURS);
        while con != 0 {
            let first = m.u32(con);
            let mut node = first;
            loop {
                let edge = m.u32(node + n::EDGE);
                let fa = m.u32(edge + e::FACE);
                let fb = m.u32(edge + e::FACE + 4);
                let mut which = 0;
                if fa != 0 && m.u8(fa + f::FLAG) != 0 {
                    which = 1;
                } else if fb != 0 && m.u8(fb + f::FLAG) != 0 {
                    which = 2;
                }
                if which != 0 {
                    let new = get_edge(m, o);
                    let edge = m.u32(node + n::EDGE);
                    m.copy(new, edge, 0x7c);
                    if which == 1 {
                        m.set_u32(new + e::FACE + 4, 0);
                        m.set_u32(edge + e::FACE, 0);
                    } else {
                        m.set_u32(new + e::FACE, 0);
                        m.set_u32(edge + e::FACE + 4, 0);
                    }
                    m.set_u32(edge + e::NEXT, edge);
                    m.set_u32(node + n::EDGE, new);
                }
                node = m.u32(node + n::NEXT);
                if node == first {
                    break;
                }
            }
            con = m.u32(con + c::NEXT);
        }
        face = m.u32(face + f::NEXT);
    }
}

/// `colinear(a, b)` [soft 0x083106b4 colinear]: 1 when the three points (`a`'s start point, `b`'s start
/// point taken with `a`'s end index, `b`'s other point) have a cross product with every component below 5e-7.
/// x87: differences and products kept unrounded; `u = dyab*dzcb - dzab*dycb`, `w = dxcb*dzab - dxab*dzcb`,
/// `v = dycb*dxab - dyab*dxcb`, tested in the order u, w, v.
pub fn colinear(m: &mut Mem, a: u32, b: u32) -> u32 {
    let ea = m.u8(a + n::END) as u32;
    let pa = m.u32(m.u32(a + n::EDGE) + e::PT + ea * 4);
    let pb = m.u32(m.u32(b + n::EDGE) + e::PT + ea * 4);
    let pc = m.u32(m.u32(b + n::EDGE) + e::PT + (1u32.wrapping_sub(ea)) * 4);
    let eps = x(5e-7);
    let t1 = d(m, pa) - d(m, pb);
    let t2 = d(m, pa + 8) - d(m, pb + 8);
    let t3 = d(m, pa + 0x10) - d(m, pb + 0x10);
    let t4 = d(m, pc) - d(m, pb);
    let t5 = d(m, pc + 8) - d(m, pb + 8);
    let t9 = d(m, pc + 0x10) - d(m, pb + 0x10);
    let u = t2 * t9 - t3 * t5;
    let v = t5 * t1 - t2 * t4;
    let w = t4 * t3 - t1 * t9;
    (u.abs().lt(eps) && w.abs().lt(eps) && v.abs().lt(eps)) as u32
}

/// `computeFaceSize(node, min, max, init)` [soft 0x0830d1bc computeFaceSize]: grows the box (`min`, `max`,
/// three doubles each) by the start points of the circular list of edge nodes; with `init` the box starts as
/// the first node's point and the walk starts at the second. Per axis: above max sets max, else below min sets min.
pub fn compute_face_size(m: &mut Mem, first: u32, min: u32, max: u32, init: bool) {
    let mut node = first;
    if init {
        let pt = node_pt(m, first);
        m.copy(min, pt, 0x18);
        m.copy(max, min, 0x18);
        node = m.u32(first + n::NEXT);
    }
    loop {
        let pt = node_pt(m, node);
        for ax in 0..3u32 {
            let v = m.f64(pt + ax * 8);
            if v > m.f64(max + ax * 8) {
                m.set_f64(max + ax * 8, v);
            } else if v < m.f64(min + ax * 8) {
                m.set_f64(min + ax * 8, v);
            }
        }
        node = m.u32(node + n::NEXT);
        if node == first {
            break;
        }
    }
}

/// `si_computeFaceGroupSize(group)` [soft 0x0830d2d0 si_computeFaceGroupSize]: the box of a face group (min at
/// `+0`, max at `+0x18`) from the boxes of its faces (list from `+0x30`, linked through `+0x78`).
pub fn compute_face_group_size(m: &mut Mem, group: u32) {
    if group == 0 {
        return;
    }
    let first = m.u32(group + 0x30);
    if first == 0 {
        return;
    }
    let mut mn = [m.f64(first + 0x20), m.f64(first + 0x28), m.f64(first + 0x30)];
    let mut mx = [m.f64(first + 0x38), m.f64(first + 0x40), m.f64(first + 0x48)];
    let mut face = m.u32(first + f::NEXT);
    while face != 0 {
        for ax in 0..3u32 {
            let hi = m.f64(face + 0x38 + ax * 8);
            if hi > mx[ax as usize] {
                mx[ax as usize] = hi;
            }
            let lo = m.f64(face + 0x20 + ax * 8);
            if lo < mn[ax as usize] {
                mn[ax as usize] = lo;
            }
        }
        face = m.u32(face + f::NEXT);
    }
    for ax in 0..3u32 {
        m.set_f64(group + ax * 8, mn[ax as usize]);
        m.set_f64(group + 0x18 + ax * 8, mx[ax as usize]);
    }
}

/// `nearPointEdge(edge, eps)` [soft 0x08311ee0 nearPointEdge]: 1 when the edge's end points differ by at most
/// `eps` on each axis and their squared distance `(dy*dy + dx*dx) + dz*dz` is below `eps` (80 bits, unrounded).
pub fn near_point_edge(m: &mut Mem, edge: u32, eps: f64) -> u32 {
    let (a, b) = (m.u32(edge + e::PT), m.u32(edge + e::PT + 4));
    let e5 = x(eps);
    let t1 = d(m, a) - d(m, b);
    if t1.abs().gt(e5) {
        return 0;
    }
    let t2 = d(m, a + 8) - d(m, b + 8);
    if t2.abs().gt(e5) {
        return 0;
    }
    let t3 = d(m, a + 0x10) - d(m, b + 0x10);
    if t3.abs().gt(e5) {
        return 0;
    }
    let s = t2 * t2 + t1 * t1;
    let s = s + t3 * t3;
    s.lt(e5) as u32
}

/// `mergeSimilarEdge(edge, which)` [soft 0x08311f90 mergeSimilarEdge]: when the edge has no face on side
/// `which`, its ring successor has none on the other side and one on side `which`: the edge takes that face
/// and its two attribute vectors of the side, the face's node is pointed at this edge, and the successor
/// is dropped from the ring.
pub fn merge_similar_edge(m: &mut Mem, edge: u32, which: u32) {
    let next = m.u32(edge + e::NEXT);
    let slot = which * 4;
    if m.u32(edge + e::FACE + slot) == 0
        && m.u32(next + e::FACE + (1 - which) * 4) == 0
        && m.u32(next + e::FACE + slot) != 0
    {
        let face = m.u32(next + e::FACE + slot);
        m.set_u32(edge + e::FACE + slot, face);
        m.copy(edge + 0x18 + which * 0x30, next + 0x18 + which * 0x30, 0x18);
        m.copy(edge + 0x30 + which * 0x30, next + 0x30 + which * 0x30, 0x18);
        let nd = find_edge_node(m, face, next, 0);
        if nd != 0 {
            m.set_u32(nd + n::EDGE, edge);
        }
        let nn = m.u32(next + e::NEXT);
        m.set_u32(edge + e::NEXT, nn);
    }
}

/// `computeNormal(face)` [soft 0x08312d08 computeNormal]: the face's plane normal by Newell's method over its
/// contours: per edge (current point to the end index's other point, the next point carried over) each sum is a
/// double local updated as `(f1 * f2) + acc` in 80 bits and stored; the length is `sqrt((B*B + A*A) + C*C)`
/// unrounded, and when above 1e-4 the normal is A/len, B/len, C/len stored in `+0`, `+8`, `+0x10`.
pub fn compute_normal(m: &mut Mem, face: u32) {
    let (mut a, mut b, mut c2) = (0.0f64, 0.0f64, 0.0f64);
    let mut con = m.u32(face + f::CONTOURS);
    while con != 0 && m.u32(con) != 0 {
        let first = m.u32(con);
        let mut node = first;
        let mut cur = {
            let p = node_pt(m, node);
            [m.f64(p), m.f64(p + 8), m.f64(p + 0x10)]
        };
        loop {
            let edge = m.u32(node + n::EDGE);
            let end = m.u8(node + n::END) as u32;
            let p = m.u32(edge + e::PT + (1u32.wrapping_sub(end)) * 4);
            let nx = [m.f64(p), m.f64(p + 8), m.f64(p + 0x10)];
            a = ((x(cur[1]) - x(nx[1])) * (x(cur[2]) + x(nx[2])) + x(a)).to_f64();
            c2 = ((x(nx[1]) + x(cur[1])) * (x(cur[0]) - x(nx[0])) + x(c2)).to_f64();
            b = ((x(cur[2]) - x(nx[2])) * (x(cur[0]) + x(nx[0])) + x(b)).to_f64();
            cur = nx;
            node = m.u32(node + n::NEXT);
            if node == first {
                break;
            }
        }
        if con == m.u32(con + c::NEXT) {
            m.set_u32(con + c::NEXT, 0);
        }
        con = m.u32(con + c::NEXT);
    }
    let len = (x(b) * x(b) + x(a) * x(a) + x(c2) * x(c2)).sqrt();
    if len.gt(x(1e-4)) {
        m.set_f64(face, (x(a) / len).to_f64());
        m.set_f64(face + 8, (x(b) / len).to_f64());
        m.set_f64(face + 0x10, (x(c2) / len).to_f64());
    }
}

/// `computeAveragePt(out, face)` [soft 0x08312e84 computeAveragePt]: the average of the first contour's
/// start points and, for each node, the other end of its edge, into the 24 bytes at `out` (returned): running
/// sums stored as doubles, the last sum divided unrounded by the count (starts at 1, +1 per node).
pub fn compute_average_pt(m: &mut Mem, out: u32, face: u32) -> u32 {
    let con = m.u32(face + f::CONTOURS);
    let first = m.u32(con);
    let p = node_pt(m, first);
    let mut sum = [m.f64(p), m.f64(p + 8), m.f64(p + 0x10)];
    let mut last = [F80::ZERO; 3];
    let mut cnt = 1i32;
    let mut node = first;
    loop {
        let edge = m.u32(node + n::EDGE);
        let end = m.u8(node + n::END) as u32;
        let q = m.u32(edge + e::PT + (1u32.wrapping_sub(end)) * 4);
        for ax in 0..3u32 {
            last[ax as usize] = x(sum[ax as usize]) + d(m, q + ax * 8);
            sum[ax as usize] = last[ax as usize].to_f64();
        }
        cnt += 1;
        node = m.u32(node + n::NEXT);
        if node == first {
            break;
        }
    }
    let c = x(cnt);
    for ax in 0..3u32 {
        m.set_f64(out + ax * 8, (last[ax as usize] / c).to_f64());
    }
    out
}

/// `resetRegion(r)` [soft 0x0830f338 resetRegion]: unlinks a face-list cell. Without a previous cell the
/// next one's face takes the cell's place in its region slots (`+0x6c` or `+0x70`) and its fields are copied
/// into this cell; with one the previous and next cells are joined.
pub fn reset_region(m: &mut Mem, r: u32) {
    let prev = m.u32(r + 8);
    let next = m.u32(r + 0xc);
    if prev == 0 {
        if next != 0 {
            let face = m.u32(next);
            if m.u32(face + f::REG_A) == next {
                m.set_u32(face + f::REG_A, r);
            } else {
                m.set_u32(face + f::REG_B, r);
            }
            m.set_u32(r + 4, m.u32(next + 4));
            m.set_u32(r, face);
            m.set_u32(r + 0xc, m.u32(next + 0xc));
            let nn = m.u32(next + 0xc);
            if nn != 0 {
                m.set_u32(nn + 8, r);
            }
        }
    } else {
        m.set_u32(prev + 0xc, next);
        if next != 0 {
            m.set_u32(next + 8, prev);
        }
    }
}


/// A call to a routine outside the file, and its `eax`.
pub(crate) fn call(m: &mut Mem, o: &mut dyn Outside, r: Routine, args: &[u32]) -> u32 {
    o.call(m, r, args).eax
}

/// `addPtToEdge(edge, pt, face)` [soft 0x0830d50c addPtToEdge]: splits every edge of the ring that starts at
/// `edge` at the point `pt` (which is taken to lie on the segment, at the fraction the squared distances give):
/// the new edge runs from `pt` to the old end, the old edge now ends at `pt`, the attribute vectors of each end
/// that has a face are interpolated, and the other face of the edge gets a node for the new edge after the old
/// one. Answers the first new edge. The six interpolated doubles live in a stack buffer that is not cleared
/// between edges (an end without a face copies the previous edge's value, or stack garbage on the first; the
/// Rust has zeros there).
pub fn add_pt_to_edge(m: &mut Mem, o: &mut dyn Outside, first: u32, pt: u32, face: u32) -> u32 {
    let mut first_new = 0u32;
    let mut head_old = 0u32;
    let mut last_old = 0u32;
    let mut head_new = 0u32;
    let mut prev_new = 0u32;
    let mut tmp = [0u64; 6];
    let mut cur = first;
    loop {
        let next = m.u32(cur + e::NEXT);
        let p0 = m.u32(cur + e::PT);
        let p1 = m.u32(cur + e::PT + 4);
        // the differences are kept unrounded in the register; the stores round copies
        let t1 = d(m, pt) - d(m, p0);
        let t2 = d(m, pt + 8) - d(m, p0 + 8);
        let t3 = d(m, pt + 0x10) - d(m, p0 + 0x10);
        let t4 = d(m, p1) - d(m, p0);
        let t5 = d(m, p1 + 8) - d(m, p0 + 8);
        let t6 = d(m, p1 + 0x10) - d(m, p0 + 0x10);
        let t7 = ((t3 * t3 + (t2 * t2 + t1 * t1)) / (t6 * t6 + (t5 * t5 + t4 * t4))).sqrt();
        for k in 0..2u32 {
            if m.u32(cur + e::FACE + k * 4) != 0 {
                for ax in 0..3u32 {
                    let a = d(m, cur + 0x18 + k * 0x30 + ax * 8);
                    let b = d(m, cur + 0x30 + k * 0x30 + ax * 8);
                    tmp[(k * 3 + ax) as usize] = (a + t7 * (b - a)).to_f64().to_bits();
                }
            }
        }
        let new = get_edge(m, o);
        m.set_u8(new + 6, m.u8(cur + 6));
        m.set_u16(new + 4, m.u16(cur + 4));
        m.set_u32(new + e::PT, pt);
        m.set_u32(new + e::PT + 4, p1);
        m.set_u32(new + e::FACE, m.u32(cur + e::FACE));
        m.set_u32(new + e::FACE + 4, m.u32(cur + e::FACE + 4));
        for i in 0..3u32 {
            m.set_u64(new + 0x18 + i * 8, tmp[i as usize]);
            m.set_u64(new + 0x48 + i * 8, tmp[3 + i as usize]);
        }
        m.copy(new + 0x30, cur + 0x30, 0x18);
        m.copy(new + 0x60, cur + 0x60, 0x18);
        for i in 0..3u32 {
            m.set_u64(cur + 0x30 + i * 8, tmp[i as usize]);
            m.set_u64(cur + 0x60 + i * 8, tmp[3 + i as usize]);
        }
        if first_new == 0 {
            first_new = new;
        }
        if head_new == 0 {
            head_new = new;
        } else {
            m.set_u32(prev_new + e::NEXT, new);
            m.set_u32(new + e::NEXT, head_new);
        }
        prev_new = new;
        m.set_u32(cur + e::PT + 4, pt);
        if head_old == 0 {
            head_old = cur;
        } else {
            m.set_u32(last_old + e::NEXT, cur);
            m.set_u32(cur + e::NEXT, head_old);
        }
        last_old = cur;
        for i in 0..2u32 {
            let fc = m.u32(cur + e::FACE + i * 4);
            if fc != 0 && fc != face {
                let node = find_edge_node(m, fc, cur, 0);
                if node != 0 {
                    let nn = call(m, o, SI_CREATE_EDGE_NODE, &[]);
                    m.set_u8(nn + n::END, m.u8(node + n::END));
                    let after = m.u32(node + n::NEXT);
                    m.set_u32(nn + n::NEXT, after);
                    m.set_u32(nn + n::PREV, node);
                    m.set_u32(after + n::PREV, nn);
                    m.set_u32(node + n::NEXT, nn);
                    if m.u8(node + n::END) == 0 {
                        m.set_u32(nn + n::EDGE, new);
                    } else {
                        m.set_u32(nn + n::EDGE, cur);
                        m.set_u32(node + n::EDGE, new);
                    }
                } else {
                    m.set_u32(new + e::FACE + i * 4, 0);
                    m.set_u32(cur + e::FACE + i * 4, 0);
                }
            }
        }
        cur = next;
        if cur == first {
            break;
        }
    }
    first_new
}

/// `insertEdge(a, b)` [soft 0x0830d834 insertEdge]: swaps the successors of the two edges in their rings (joining
/// the rings), then turns round every other edge of the ring that touches `a`'s ends (the original's test is
/// `e.p1 == a.p0 || e.p0 == a.p1`): the nodes of both faces change end, the attribute vectors swap (`+0x18` with
/// `+0x60`, `+0x30` with `+0x48`), and the points and faces swap.
pub fn insert_edge(m: &mut Mem, a: u32, b: u32) {
    let t = m.u32(a + e::NEXT);
    m.set_u32(a + e::NEXT, m.u32(b + e::NEXT));
    m.set_u32(b + e::NEXT, t);
    let mut cur = m.u32(a + e::NEXT);
    while cur != a {
        if m.u32(cur + e::PT + 4) == m.u32(a + e::PT) || m.u32(cur + e::PT) == m.u32(a + e::PT + 4) {
            for k in 0..2u32 {
                let node = find_edge_node(m, m.u32(cur + e::FACE + k * 4), cur, 0);
                if node != 0 {
                    m.set_u8(node + n::END, 1u8.wrapping_sub(m.u8(node + n::END)));
                }
            }
            let mut v = [[0u8; 24]; 4];
            for (i, vi) in v.iter_mut().enumerate() {
                for (j, byte) in vi.iter_mut().enumerate() {
                    *byte = m.u8(cur + 0x18 + i as u32 * 0x18 + j as u32);
                }
            }
            for (i, vi) in [3usize, 2, 1, 0].iter().enumerate() {
                for j in 0..24u32 {
                    m.set_u8(cur + 0x18 + i as u32 * 0x18 + j, v[*vi][j as usize]);
                }
            }
            let (p0, p1) = (m.u32(cur + e::PT), m.u32(cur + e::PT + 4));
            m.set_u32(cur + e::PT + 4, p0);
            m.set_u32(cur + e::PT, p1);
            let (fa, fb) = (m.u32(cur + e::FACE), m.u32(cur + e::FACE + 4));
            m.set_u32(cur + e::FACE, fb);
            m.set_u32(cur + e::FACE + 4, fa);
        }
        cur = m.u32(cur + e::NEXT);
    }
}

/// `splitOnEdge(a, b, face)` [soft 0x0830ea78 splitOnEdge]: splits the edge behind `a`'s edge node at `b`'s
/// point (`addPtToEdge`), makes an edge node for the new edge and two intersection nodes (one for `b`'s point
/// and key with the new node as head, one for the successor's point and key as tail), and links them after
/// `a`'s successor, whose point becomes `b`'s. Answers the first new intersection node.
pub fn split_on_edge(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, face: u32) -> u32 {
    let head = m.u32(a + 0x38);
    let new_edge = add_pt_to_edge(m, o, m.u32(head + n::EDGE), m.u32(b), face);
    let en = call(m, o, SI_CREATE_EDGE_NODE, &[]);
    m.set_u32(en + n::EDGE, new_edge);
    m.set_u8(en + n::END, m.u8(head + n::END));
    m.set_u32(en + n::PREV, en);
    m.set_u32(en + n::NEXT, en);
    let n1 = inter_list_node(m, o);
    m.set_u32(n1, m.u32(b));
    m.set_u64(n1 + 0x1c, m.u64(b + 0x1c));
    m.set_u32(n1 + 0x38, en);
    m.set_u32(n1 + 0x34, 1);
    m.set_u32(n1 + 0x30, 0);
    let n2 = inter_list_node(m, o);
    let nx = m.u32(a + 0x40);
    m.set_u32(n2, m.u32(nx));
    m.set_u64(n2 + 0x1c, m.u64(nx + 0x1c));
    m.set_u32(n2 + 0x2c, en);
    m.set_u32(n2 + 0x28, 1);
    m.set_u32(n2 + 0x24, 0);
    m.set_u32(nx, m.u32(b));
    m.set_u32(n1 + 0x40, n2);
    m.set_u32(n2 + 0x3c, n1);
    m.set_u32(n1 + 0x3c, nx);
    m.set_u32(n2 + 0x40, m.u32(nx + 0x40));
    m.set_u32(nx + 0x40, n1);
    let after = m.u32(n2 + 0x40);
    if after != 0 {
        m.set_u32(after + 0x3c, n2);
    }
    n1
}

/// `findOnEdge(a, b, plist, face, pout)` [soft 0x0830eb58 findOnEdge]: finds or makes the pair of
/// intersection nodes (sorted by key along the list `*plist`) that carry the edge between the keys of `a` and
/// `b`: splitting an existing run (`splitOnEdge`) where `a` or `b` falls inside one, or else making a new edge
/// with an edge node and two nodes, linked after the last node whose key is below `a`'s. `*pout` gets the node
/// holding `b`'s end; the answer is the edge node (the tail or head of the node found).
pub fn find_on_edge(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, plist: u32, face: u32, pout: u32) -> u32 {
    let mut target = b;
    let mut prevnode = 0u32;
    let mut cur = m.u32(plist);
    // the node after which `splitOnEdge` was done, as `edx` in the original, for the exits to 0830ebd5
    let mut found: Option<u32> = None;
    if cur != 0 {
        let f2 = m.f64(b + 0x1c);
        if !(f2 < m.f64(cur + 0x1c)) {
            let f3 = m.f64(a + 0x1c);
            loop {
                if f3 < m.f64(cur + 0x1c) {
                    target = cur;
                    break;
                }
                if m.u32(a) == m.u32(cur) {
                    let nx = m.u32(cur + 0x40);
                    if f2 < m.f64(nx + 0x1c) {
                        let r = split_on_edge(m, o, cur, b, face);
                        m.set_u32(pout, b);
                        found = Some(m.u32(r + 0x3c));
                    } else {
                        m.set_u32(pout, nx);
                        found = Some(cur);
                    }
                    break;
                }
                let nx = m.u32(cur + 0x40);
                if f3 < m.f64(nx + 0x1c) {
                    let r = split_on_edge(m, o, cur, a, face);
                    let nn = m.u32(r + 0x40);
                    if m.f64(nn + 0x1c) <= f2 {
                        m.set_u32(pout, nn);
                        found = Some(r);
                    } else {
                        let r2 = split_on_edge(m, o, r, b, face);
                        m.set_u32(pout, b);
                        found = Some(m.u32(r2 + 0x3c));
                    }
                    break;
                }
                prevnode = nx;
                let nn = m.u32(nx + 0x40);
                if nn == 0 || f2 < m.f64(nn + 0x1c) {
                    break;
                }
                cur = nn;
            }
        }
    }
    if let Some(nd) = found {
        let t = m.u32(nd + 0x2c);
        return if t != 0 { t } else { m.u32(nd + 0x38) };
    }
    let edge = get_edge(m, o);
    let en = call(m, o, SI_CREATE_EDGE_NODE, &[]);
    m.set_u32(en + n::EDGE, edge);
    m.set_u8(en + n::END, 0);
    m.set_u32(en + n::PREV, en);
    m.set_u32(en + n::NEXT, en);
    // the unordered case leaves the index at 1
    let idx = if m.f64(target + 0x1c) > m.f64(a + 0x1c) { 0usize } else { 1 };
    let mut tmp = [0u32; 2];
    let n1 = inter_list_node(m, o);
    tmp[idx] = n1;
    m.set_u32(n1, m.u32(a));
    m.set_u64(n1 + 0x1c, m.u64(a + 0x1c));
    m.copy(n1 + 4, a + 4, 24);
    m.set_u32(n1 + 0x38, en);
    m.set_u32(n1 + 0x34, 1);
    m.set_u32(n1 + 0x30, 0);
    let n2 = inter_list_node(m, o);
    tmp[1 - idx] = n2;
    m.set_u32(n2, m.u32(target));
    m.set_u64(n2 + 0x1c, m.u64(target + 0x1c));
    m.copy(n2 + 4, target + 4, 24);
    m.set_u32(n2 + 0x2c, en);
    m.set_u32(n2 + 0x28, 1);
    m.set_u32(n2 + 0x24, 0);
    let (lo, hi) = (tmp[idx], tmp[1 - idx]);
    m.set_u32(lo + 0x40, hi);
    m.set_u32(hi + 0x3c, lo);
    if prevnode == 0 {
        let first = m.u32(plist);
        m.set_u32(hi + 0x40, first);
        if first != 0 {
            m.set_u32(first + 0x3c, hi);
        }
        m.set_u32(lo + 0x3c, 0);
        m.set_u32(plist, lo);
    } else {
        let after = m.u32(prevnode + 0x40);
        m.set_u32(hi + 0x40, after);
        if after != 0 {
            m.set_u32(after + 0x3c, hi);
        }
        m.set_u32(lo + 0x3c, prevnode);
        m.set_u32(prevnode + 0x40, lo);
    }
    m.set_u8(edge + 6, 1);
    m.set_u32(edge + e::PT, m.u32(lo));
    m.set_u32(edge + e::PT + 4, m.u32(hi));
    m.copy(edge + 0x18, lo + 4, 24);
    m.copy(edge + 0x48, lo + 4, 24);
    m.copy(edge + 0x30, hi + 4, 24);
    m.copy(edge + 0x60, hi + 4, 24);
    m.set_u32(pout, hi);
    en
}

/// `buildFace(src, list, pstatus)` [soft 0x0830e9c8 buildFace]: a new face (a copy of `src`, via
/// `si_createBoolFace`) with the circular edge-node list `list` as its one contour: every edge of the list
/// gets the face on the side the node says, the first point's side stamp (`+0x5a`) goes to `*pstatus`, the
/// box is computed. Answers the face.
pub fn build_face(m: &mut Mem, o: &mut dyn Outside, src: u32, list: u32, pstatus: u32) -> u32 {
    let face = call(m, o, SI_CREATE_BOOL_FACE, &[src]);
    m.set_u32(face + f::NEXT, 0);
    m.set_u32(face + f::PREV, 0);
    let con = call(m, o, SI_CREATE_CONTOUR, &[list]);
    m.set_u32(face + f::CONTOURS, con);
    m.set_u32(pstatus, 1);
    let vc = next_visit(m);
    let mut node = list;
    loop {
        let edge = m.u32(node + n::EDGE);
        let end = m.u8(node + n::END) as u32;
        m.set_u32(edge + e::FACE + end * 4, face);
        if m.u32(pstatus) == 1 {
            let pt = m.u32(edge + e::PT + end * 4);
            m.set_i32(pstatus, m.u8(pt + p::SIDE) as i8 as i32);
        }
        m.set_u32(node + n::VISIT, vc);
        node = m.u32(node + n::NEXT);
        if m.u32(node + n::VISIT) == vc {
            break;
        }
    }
    let first = m.u32(m.u32(face + f::CONTOURS) + c::FIRST);
    if first != 0 {
        compute_face_size(m, first, face + f::MIN, face + f::MAX, true);
    }
    face
}

/// `si_splitFace (0x08313b74)`: cuts a face by a tree node's plane into two lists (ported in `split.rs`; the five call sites call it directly).
pub const SI_SPLIT_FACE: Routine = Routine::new("si_splitFace", 0x0831_3b74);

/// `cleanUpTree(tree, class_tree)` [soft 0x083103c8 cleanUpTree]: after the classification, settles the status
/// of every face. A leaf of the operation's kind (inside 0 with intersection or difference, inside 1 with union)
/// that holds faces: the first face, if still unknown (1), is classified against `class_tree`, and every later
/// face of the leaf's list takes that status (or its own `computeStatus`, when its flag is 0, which wins when it
/// differs). A node: both subtrees first, then its own faces: unknown ones are classified (a negative id is
/// outside without asking), inside becomes unknown again, outside is reset and unlinked.
pub fn clean_up_tree(m: &mut Mem, tree: u32, class_tree: u32) -> u32 {
    if m.i32(tree + t::KIND) == 1 {
        let inside = m.i32(tree + t::INSIDE);
        let oper = m[OPER];
        let want = (inside == 0 && (oper.wrapping_sub(2) as u32) < 2) || (inside == 1 && oper == 1);
        let list = m.u32(tree + 4);
        if want && list != 0 {
            let face = m.u32(list);
            if m.u8(face + f::STATUS) == 1 {
                let st = compute_status(m, class_tree, face);
                m.set_u8(face + f::STATUS, st as u8);
            }
            let mut cell = m.u32(m.u32(tree + 4) + 0xc);
            while cell != 0 {
                let other = m.u32(cell);
                let st = if m.u8(other + f::FLAG) == 0 {
                    compute_status(m, class_tree, other)
                } else {
                    m.u8(face + f::STATUS) as i8 as i32
                };
                m.set_u8(other + f::STATUS, m.u8(face + f::STATUS));
                if st != m.u8(face + f::STATUS) as i8 as i32 {
                    m.set_u8(other + f::STATUS, st as u8);
                }
                cell = m.u32(cell + 0xc);
            }
        }
    } else {
        let back = m.u32(tree + t::BACK);
        if back != 0 {
            let r = clean_up_tree(m, back, class_tree);
            m.set_u32(tree + t::BACK, r);
        }
        let front = m.u32(tree + t::FRONT);
        if front != 0 {
            let r = clean_up_tree(m, front, class_tree);
            m.set_u32(tree + t::FRONT, r);
        }
        let mut face = m.u32(tree + t::FACES);
        while face != 0 {
            if m.u8(face + f::STATUS) == 1 {
                if m.i32(face + f::ID) < 0 {
                    m.set_u8(face + f::STATUS, 3);
                } else {
                    let st = compute_status(m, class_tree, face);
                    m.set_u8(face + f::STATUS, st as u8);
                }
            }
            let next = m.u32(face + f::NEXT);
            let prev = m.u32(face + f::PREV);
            let st = m.u8(face + f::STATUS);
            if st == 2 {
                m.set_u8(face + f::STATUS, 1);
            } else if st == 3 {
                reset_face(m, face);
                if prev == 0 {
                    m.set_u32(tree + t::FACES, next);
                } else {
                    m.set_u32(prev + f::NEXT, next);
                }
                if next != 0 {
                    m.set_u32(next + f::PREV, prev);
                }
            }
            face = next;
        }
    }
    tree
}

/// `dummyInsert(tree, faces, which)` [soft 0x0830efc4 dummyInsert]: files a list of faces into the tree. At a
/// leaf of the operation's kind the faces are marked (inside, region `which`) and kept; at another leaf they are
/// reset and dropped (0). At a node the first face's side decides: back and front go down that subtree; a cut
/// face is split (`si_splitFace`, which hands back the back and the front part) and both parts go down, front
/// first, the results joined back then front.
pub fn dummy_insert(m: &mut Mem, o: &mut dyn Outside, tree: u32, face: u32, which: u8) -> u32 {
    if face == 0 {
        return 0;
    }
    if m.i32(tree + t::KIND) == 1 {
        let oper = m[OPER];
        let inside = m.i32(tree + t::INSIDE);
        let keep = if (oper.wrapping_sub(2) as u32) < 2 { inside == 0 } else { inside == 1 };
        if keep {
            mark_selected_face(m, face, tree, which);
            return face;
        }
        reset_list_of_faces(m, face);
        return 0;
    }
    match which_side(m, face, tree) {
        3 => dummy_insert(m, o, m.u32(tree + t::BACK), face, which),
        2 => dummy_insert(m, o, m.u32(tree + t::FRONT), face, which),
        4 => {
            let back_out = m.local(4);
            let front_out = m.local(4);
            crate::split::si_split_face(m, o, tree, face, back_out, front_out);
            let front_part = m.u32(front_out);
            let back_part = m.u32(back_out);
            let r_front = dummy_insert(m, o, m.u32(tree + t::FRONT), front_part, which);
            let r_back = dummy_insert(m, o, m.u32(tree + t::BACK), back_part, which);
            m.release(8);
            merge_list_of_faces(m, r_back, r_front)
        }
        _ => 0,
    }
}

/// Splices the chain `head..tail` of region cells in after `cell` (cells are linked through `+8` back and `+0xc`
/// forward).
fn splice_cells_after(m: &mut Mem, head: u32, tail: u32, cell: u32) {
    let nxt = m.u32(cell + 0xc);
    m.set_u32(tail + 0xc, nxt);
    m.set_u32(head + 8, cell);
    let n2 = m.u32(tail + 0xc);
    if n2 != 0 {
        m.set_u32(n2 + 8, tail);
    }
    m.set_u32(cell + 0xc, head);
}

/// `processRegion(r1, r2, r3, r4, faces, plast)` [soft 0x0830f234 processRegion]: two chains of region cells are
/// made for the face list `faces` (`si_createFaceList(r1->bsp, face)` twice a face), the first cell of each face
/// goes to its `+0x6c`, the second to its `+0x70`, and the last face walked to `*plast`. The first chain is then
/// spliced in after `r2` and the second after `r3` when `r4 == r1`, else the first after `r4` and the second after
/// `r2`.
pub fn process_region(m: &mut Mem, o: &mut dyn Outside, r1: u32, r2: u32, r3: u32, r4: u32, mut face: u32, plast: u32) {
    let (mut head_a, mut tail_a, mut head_b, mut tail_b) = (0u32, 0u32, 0u32, 0u32);
    while face != 0 {
        let tree = m.u32(r1 + 4);
        let a = call(m, o, SI_CREATE_FACE_LIST, &[tree, face]);
        if head_a == 0 {
            head_a = a;
        } else {
            m.set_u32(tail_a + 0xc, a);
            m.set_u32(a + 8, tail_a);
        }
        tail_a = a;
        let tree = m.u32(r1 + 4);
        let b = call(m, o, SI_CREATE_FACE_LIST, &[tree, face]);
        if head_b == 0 {
            head_b = b;
        } else {
            m.set_u32(tail_b + 0xc, b);
            m.set_u32(b + 8, tail_b);
        }
        tail_b = b;
        m.set_u32(face + f::REG_A, a);
        m.set_u32(face + f::REG_B, b);
        m.set_u32(plast, face);
        face = m.u32(face + f::NEXT);
    }
    let (after_a, after_b) = if r4 == r1 { (r2, r3) } else { (r4, r2) };
    if tail_a != 0 {
        splice_cells_after(m, head_a, tail_a, after_a);
    }
    if tail_b != 0 {
        splice_cells_after(m, head_b, tail_b, after_b);
    }
}

/// `classifyRegion(regions, tree, back, front)` [soft 0x0830f390 classifyRegion]: sorts the region cells of the
/// list `regions` down the tree node: each cell is detached and goes behind `back` or `front` (a cell of the
/// side lists; with the face's status set to the operation's answer when the face's flag is 0 and `changeStatus`
/// is on), a face cut by the plane is split (`si_splitFace`) and both parts get new cells (`processRegion`);
/// the parts replace the face in its list. The statuses are 2/3 by the operation: union (1) back 2 front 3,
/// intersection and difference back 3 front 2; other operations leave the two words uninitialised (zeros here).
pub fn classify_region(m: &mut Mem, o: &mut dyn Outside, mut regions: u32, tree: u32, back: u32, front: u32) {
    let oper = m[OPER];
    let (st_back, st_front): (u8, u8) = match oper {
        1 => (2, 3),
        2 | 3 => (3, 2),
        _ => (0, 0), // the original leaves these two stack words unset
    };
    while regions != 0 {
        let cell = regions;
        regions = m.u32(cell + 0xc);
        let face = m.u32(cell);
        m.set_u32(cell + 0xc, 0);
        m.set_u32(cell + 8, 0);
        match which_side(m, face, tree) {
            3 => {
                let n = m.u32(back + 0xc);
                m.set_u32(cell + 0xc, n);
                m.set_u32(cell + 8, back);
                let n2 = m.u32(back + 0xc);
                if n2 != 0 {
                    m.set_u32(n2 + 8, cell);
                }
                m.set_u32(back + 0xc, cell);
                if m.u8(face + f::FLAG) == 0 && m[CHANGE_STATUS] != 0 {
                    m.set_u8(face + f::STATUS, st_back);
                }
            }
            2 => {
                let n = m.u32(front + 0xc);
                m.set_u32(cell + 0xc, n);
                m.set_u32(cell + 8, front);
                let n2 = m.u32(front + 0xc);
                if n2 != 0 {
                    m.set_u32(n2 + 8, cell);
                }
                m.set_u32(front + 0xc, cell);
                if m.u8(face + f::FLAG) == 0 && m[CHANGE_STATUS] != 0 {
                    m.set_u8(face + f::STATUS, st_front);
                }
            }
            4 => {
                let next = m.u32(face + f::NEXT);
                let prev = m.u32(face + f::PREV);
                let old_b = m.u32(face + f::REG_B);
                let old_a = m.u32(face + f::REG_A);
                m.set_u32(face + f::NEXT, 0);
                m.set_u32(face + f::PREV, 0);
                m.set_u32(face + f::REG_B, 0);
                m.set_u32(face + f::REG_A, 0);
                let back_out = m.local(4);
                let front_out = m.local(4);
                let last_back = m.local(4);
                let last_front = m.local(4);
                let last_all = m.local(4);
                m.set_u32(last_back, 0);
                m.set_u32(last_front, 0);
                crate::split::si_split_face(m, o, tree, face, back_out, front_out);
                let part_back = m.u32(back_out);
                process_region(m, o, cell, back, old_b, old_a, part_back, last_back);
                let part_front = m.u32(front_out);
                process_region(m, o, cell, front, old_b, old_a, part_front, last_front);
                if m.u8(face + f::FLAG) == 0 && m[CHANGE_STATUS] != 0 {
                    set_status(m, m.u32(back_out), st_back as i8);
                    set_status(m, m.u32(front_out), st_front as i8);
                }
                let (pb, pf) = (m.u32(back_out), m.u32(front_out));
                let (lb, lf) = (m.u32(last_back), m.u32(last_front));
                let head = merge_two_lists_of_faces(m, pb, lb, pf, lf, last_all);
                let last = m.u32(last_all);
                if last != 0 {
                    m.set_u32(last + f::NEXT, next);
                    if next != 0 {
                        m.set_u32(next + f::PREV, last);
                    }
                }
                if prev == 0 {
                    let tr = m.u32(cell + 4);
                    m.set_u32(tr + t::FACES, head);
                } else {
                    m.set_u32(prev + f::NEXT, head);
                    if head != 0 {
                        m.set_u32(head + f::PREV, prev);
                    }
                }
                m.set_u32(cell, 0);
                let target = if old_a == cell { old_b } else { old_a };
                reset_region(m, target);
                m.release(20);
            }
            _ => {}
        }
    }
}

/// `classifyCoincident(tree, group)` [soft 0x0830f5e8 classifyCoincident]: the faces of `group` lying in the
/// plane of the node (every plane coefficient within 5e-7 of the face's, compared in 80 bits) are sent down one
/// subtree and then the other, `same` = 1 for those: for union (oper not 2/3) first the front with `same`, then
/// the back with `!same`; for intersection and difference the back with `!same` first and then the front with
/// `same`. Whatever survives gets two region cells each (on the region lists the face's `+0x6c`/`+0x70` leaves
/// hold) and is pushed on the node's own face list.
pub fn classify_coincident(m: &mut Mem, o: &mut dyn Outside, tree: u32, group: u32) {
    let eps = x(5e-7);
    let inter = (m[OPER].wrapping_sub(2) as u32) < 2;
    let mut face = m.u32(group + 0x30);
    while face != 0 {
        let next_face = extract_face(m, face);
        let same = (d(m, tree + 4) - d(m, face)).abs().lt(eps)
            && (d(m, tree + 0xc) - d(m, face + 8)).abs().lt(eps)
            && (d(m, tree + 0x14) - d(m, face + 0x10)).abs().lt(eps);
        let same = same as u8;
        let (t1, w1) = if inter { (m.u32(tree + t::BACK), same ^ 1) } else { (m.u32(tree + t::FRONT), same) };
        let mut list = dummy_insert(m, o, t1, face, w1);
        while list != 0 {
            let next_in = extract_face(m, list);
            let (t2, w2) = if inter { (m.u32(tree + t::FRONT), same) } else { (m.u32(tree + t::BACK), same ^ 1) };
            let kept = dummy_insert(m, o, t2, list, w2);
            if kept != 0 {
                let mut cur = kept;
                let mut last;
                loop {
                    last = cur;
                    let reg_b = m.u32(cur + f::REG_B);
                    let cell = call(m, o, SI_CREATE_FACE_LIST, &[tree, cur]);
                    let head = m.u32(reg_b + 4);
                    m.set_u32(cell + 0xc, head);
                    if m.u32(reg_b + 4) != 0 {
                        let h = m.u32(reg_b + 4);
                        m.set_u32(h + 8, cell);
                    }
                    m.set_u32(reg_b + 4, cell);
                    let reg_a = m.u32(cur + f::REG_A);
                    m.set_u32(cur + f::REG_B, cell);
                    let cell2 = call(m, o, SI_CREATE_FACE_LIST, &[tree, cur]);
                    let head = m.u32(reg_a + 4);
                    m.set_u32(cell2 + 0xc, head);
                    if m.u32(reg_a + 4) != 0 {
                        let h = m.u32(reg_a + 4);
                        m.set_u32(h + 8, cell2);
                    }
                    m.set_u32(reg_a + 4, cell2);
                    m.set_u32(cur + f::REG_A, cell2);
                    cur = m.u32(cur + f::NEXT);
                    if cur == 0 {
                        break;
                    }
                }
                let own = m.u32(tree + t::FACES);
                m.set_u32(last + f::NEXT, own);
                if m.u32(tree + t::FACES) != 0 {
                    let h = m.u32(tree + t::FACES);
                    m.set_u32(h + f::PREV, last);
                }
                m.set_u32(tree + t::FACES, kept);
            }
            list = next_in;
        }
        face = next_face;
    }
}

/// `SI_malloc (0x080d0ee8)`: the program's allocator (another source file); a hook.
pub const SI_MALLOC: Routine = Routine::new("SI_malloc", 0x080d_0ee8);

/// Pushes `face` on the front of the face group `grp` (its first face at `+0x30`), linking through `+0x74` and `+0x78`.
fn group_push_front(m: &mut Mem, grp: u32, face: u32) {
    let head = m.u32(grp + 0x30);
    if head != 0 {
        m.set_u32(face + f::NEXT, head);
        let h = m.u32(grp + 0x30);
        m.set_u32(h + f::PREV, face);
    }
    m.set_u32(grp + 0x30, face);
}

/// A new region cell for `face` at the front of the chain after the cell `*(base + at)`; the cell goes to `*(face + at)`.
fn push_region_cell(m: &mut Mem, o: &mut dyn Outside, node: u32, face: u32, at: u32, lc: u32) {
    let cell = call(m, o, SI_CREATE_FACE_LIST, &[node, face]);
    m.set_u32(face + at, cell);
    let reg = m.u32(lc + at);
    let nx = m.u32(reg + 0xc);
    m.set_u32(cell + 0xc, nx);
    let reg = m.u32(lc + at);
    let nx = m.u32(reg + 0xc);
    if nx != 0 {
        m.set_u32(nx + 8, cell);
    }
    let reg = m.u32(lc + at);
    m.set_u32(cell + 8, reg);
    let reg = m.u32(lc + at);
    m.set_u32(reg + 0xc, cell);
}

/// Pushes the part list `part` (front of the group's list) in front of what the group `grp` holds; `grp.first = part`
/// even when `part` is null (the original's code does that).
fn group_take_part(m: &mut Mem, grp: u32, part: u32) {
    if m.u32(grp + 0x30) != 0 && part != 0 {
        let last = find_last_face_of_list(m, part);
        let head = m.u32(grp + 0x30);
        m.set_u32(last + f::NEXT, head);
        let head = m.u32(grp + 0x30);
        m.set_u32(head + f::PREV, part);
    }
    m.set_u32(grp + 0x30, part);
}

/// `si_buildBsp(tree, group, status)` [soft 0x08313648 si_buildBsp]: builds the BSP tree of the faces of `group`.
/// Two scratch groups (0x34 bytes each, `SI_malloc`) take the faces behind and in front of the node plane. If
/// `tree` is null or a leaf a new node is made from the face of the group with the largest extent (the first of
/// the three axes whose box side exceeds the best so far, 80-bit differences), taken out of the group, its plane
/// copied in, status `status`, with a back and a front leaf (`si_createRegBspNode(1)` and `(0)`) and a region cell
/// for each; otherwise the node is `tree` and its own face list `+0x24` is the one faces on the plane join. Then
/// each face of the group goes by `si_whichSide`: on the plane (1) it joins the node's faces (status `status`, two
/// region cells; the two cells swap when the plane's three coefficients are not all within 5e-7 of the node's
/// first face's), back (3) and front (2) go to the scratch groups, a cut face (4) is split. When `tree` was a leaf
/// the leaf's region list is sorted down the new node (`classifyRegion`); the scratch groups recurse into the two
/// subtrees and are freed. Answers the node (0 when a scratch group could not be had or the group is empty).
pub fn si_build_bsp(m: &mut Mem, o: &mut dyn Outside, tree: u32, group: u32, status: u8) -> u32 {
    let back_grp = call(m, o, SI_MALLOC, &[0x34]);
    let front_grp = call(m, o, SI_MALLOC, &[0x34]);
    if back_grp == 0 || front_grp == 0 || group == 0 || m.u32(group + 0x30) == 0 {
        return 0;
    }
    let first_face = m.u32(group + 0x30);
    let node;
    let mut lc;
    if tree == 0 || m.i32(tree + t::KIND) == 1 {
        let mut best = F80::ZERO;
        let mut pick = 0u32;
        let mut face = first_face;
        loop {
            let mut found = None;
            for ax in 0..3u32 {
                let df = d(m, face + 0x38 + ax * 8) - d(m, face + 0x20 + ax * 8);
                if df.gt(best) {
                    found = Some(df);
                    break;
                }
            }
            if let Some(df) = found {
                best = df;
                pick = face;
            }
            face = m.u32(face + f::NEXT);
            if face == 0 {
                break;
            }
        }
        lc = first_face;
        if pick == 0 {
            m.set_u32(group + 0x30, m.u32(first_face + f::NEXT));
        } else {
            lc = pick;
            let pp = m.u32(pick + f::PREV);
            if pp != 0 {
                m.set_u32(pp + f::NEXT, m.u32(pick + f::NEXT));
            } else {
                m.set_u32(group + 0x30, m.u32(pick + f::NEXT));
            }
            let pn = m.u32(pick + f::NEXT);
            if pn != 0 {
                m.set_u32(pn + f::PREV, m.u32(pick + f::PREV));
            }
        }
        m.set_u32(lc + f::NEXT, 0);
        m.set_u32(lc + f::PREV, 0);
        node = si_get_mem(m, o, 0x30);
        m.set_u32(node, 0);
        m.set_u32(node + t::BACK, 0);
        m.set_u32(node + t::FRONT, 0);
        for i in 0..8 {
            let w = m.u32(lc + i * 4);
            m.set_u32(node + 4 + i * 4, w);
        }
        m.set_u32(node + t::FACES, lc);
        m.set_u8(lc + f::STATUS, status);
        let r = call(m, o, SI_CREATE_REG_BSP_NODE, &[1]);
        m.set_u32(node + t::BACK, r);
        let cell = call(m, o, SI_CREATE_FACE_LIST, &[node, lc]);
        m.set_u32(lc + f::REG_A, cell);
        let b = m.u32(node + t::BACK);
        m.set_u32(b + 4, cell);
        let r = call(m, o, SI_CREATE_REG_BSP_NODE, &[0]);
        m.set_u32(node + t::FRONT, r);
        let cell = call(m, o, SI_CREATE_FACE_LIST, &[node, lc]);
        m.set_u32(lc + f::REG_B, cell);
        let fr = m.u32(node + t::FRONT);
        m.set_u32(fr + 4, cell);
    } else {
        node = tree;
        lc = m.u32(tree + t::FACES);
    }
    m.set_u32(back_grp + 0x30, 0);
    m.set_u32(front_grp + 0x30, 0);
    let mut face = m.u32(group + 0x30);
    let eps = x(5e-7);
    while face != 0 {
        let next = m.u32(face + f::NEXT);
        m.set_u32(face + f::NEXT, 0);
        m.set_u32(face + f::PREV, 0);
        match which_side(m, face, node) {
            1 => {
                let ln = m.u32(lc + f::NEXT);
                if ln != 0 {
                    m.set_u32(ln + f::PREV, face);
                }
                m.set_u32(face + f::PREV, lc);
                m.set_u32(face + f::NEXT, m.u32(lc + f::NEXT));
                m.set_u32(lc + f::NEXT, face);
                m.set_u8(face + f::STATUS, status);
                push_region_cell(m, o, node, face, f::REG_A, lc);
                push_region_cell(m, o, node, face, f::REG_B, lc);
                let same = (d(m, lc) - d(m, face)).abs().lt(eps)
                    && (d(m, lc + 8) - d(m, face + 8)).abs().lt(eps)
                    && (d(m, lc + 0x10) - d(m, face + 0x10)).abs().lt(eps);
                if !same {
                    let a = m.u32(face + f::REG_A);
                    let b = m.u32(face + f::REG_B);
                    m.set_u32(face + f::REG_A, b);
                    m.set_u32(face + f::REG_B, a);
                }
            }
            3 => group_push_front(m, back_grp, face),
            2 => group_push_front(m, front_grp, face),
            4 => {
                let pq = m.local(8);
                crate::split::si_split_face(m, o, node, face, pq, pq + 4);
                let (p, q) = (m.u32(pq), m.u32(pq + 4));
                group_take_part(m, back_grp, p);
                group_take_part(m, front_grp, q);
                m.release(8);
            }
            _ => {}
        }
        face = next;
    }
    if tree != 0 && m.i32(tree + t::KIND) == 1 {
        let b = m.u32(node + t::BACK);
        let fr = m.u32(node + t::FRONT);
        let (b4, f4) = (m.u32(b + 4), m.u32(fr + 4));
        classify_region(m, o, m.u32(tree + 4), node, b4, f4);
    }
    if m.u32(back_grp + 0x30) != 0 {
        let r = si_build_bsp(m, o, m.u32(node + t::BACK), back_grp, status);
        m.set_u32(node + t::BACK, r);
    }
    if m.u32(front_grp + 0x30) != 0 {
        let r = si_build_bsp(m, o, m.u32(node + t::FRONT), front_grp, status);
        m.set_u32(node + t::FRONT, r);
    }
    m.free(front_grp);
    m.free(back_grp);
    node
}

/// Pushes `face` on the front of a face group's list as `classifyFaces` does: next = old head, the old head's prev
/// is the face, the face's prev cleared.
fn cf_push(m: &mut Mem, grp: u32, face: u32) {
    let head = m.u32(grp + 0x30);
    m.set_u32(face + f::NEXT, head);
    if head != 0 {
        m.set_u32(head + f::PREV, face);
    }
    m.set_u32(face + f::PREV, 0);
    m.set_u32(grp + 0x30, face);
}

/// Pushes a whole split-off list (its last face linked to the old head) on the front of a group.
fn cf_push_list(m: &mut Mem, grp: u32, part: u32) {
    if part != 0 {
        let last = find_last_face_of_list(m, part);
        let head = m.u32(grp + 0x30);
        m.set_u32(last + f::NEXT, head);
        if head != 0 {
            m.set_u32(head + f::PREV, last);
        }
        m.set_u32(part + f::PREV, 0);
        m.set_u32(grp + 0x30, part);
    }
}

/// `classifyFaces(tree, group)` [soft 0x0830f794 classifyFaces]: sorts a face group down the BSP tree. A leaf: with union and an inside leaf, or
/// intersection/difference and an outside leaf, the group is built into a subtree (`si_buildBsp`, status 2),
/// otherwise its faces are reset. A node: the group's box decides (2 front, 3 back, else) the faces are sorted by
/// side into three new groups (coincident, back, front; cut faces are split), the coincident ones are classified,
/// the back and front groups recurse into the two subtrees. A group alone in its side takes the parent's box.
/// Answers the (possibly replaced) tree.
pub fn classify_faces(m: &mut Mem, o: &mut dyn Outside, tree: u32, group: u32) -> u32 {
    if tree == 0 {
        return 0;
    }
    if m.i32(tree + t::KIND) == 1 {
        let oper = m[OPER] as i32;
        let inside = m.i32(tree + t::INSIDE);
        if (oper == 1 && inside == 1) || (oper > 0 && oper < 4 && oper != 1 && inside == 0) {
            return si_build_bsp(m, o, tree, group, 2);
        }
        let first = m.u32(group + 0x30);
        reset_list_of_faces(m, first);
        return tree;
    }
    match test_box(m, group, group + 0x18, tree + t::PLANE) {
        2 => {
            let r = classify_faces(m, o, m.u32(tree + t::FRONT), group);
            m.set_u32(tree + t::FRONT, r);
        }
        3 => {
            let r = classify_faces(m, o, m.u32(tree + t::BACK), group);
            m.set_u32(tree + t::BACK, r);
        }
        _ => {
            let coin = call(m, o, SI_CREATE_FACE_GROUP, &[]);
            let back = call(m, o, SI_CREATE_FACE_GROUP, &[]);
            let front = call(m, o, SI_CREATE_FACE_GROUP, &[]);
            let mut face = m.u32(group + 0x30);
            while face != 0 {
                let next = extract_face(m, face);
                match which_side(m, face, tree) {
                    1 => cf_push(m, coin, face),
                    2 => cf_push(m, front, face),
                    3 => cf_push(m, back, face),
                    4 => {
                        let pq = m.local(8);
                        crate::split::si_split_face(m, o, tree, face, pq, pq + 4);
                        let (p, q) = (m.u32(pq), m.u32(pq + 4));
                        cf_push_list(m, back, p);
                        cf_push_list(m, front, q);
                        m.release(8);
                    }
                    _ => {}
                }
                face = next;
            }
            if m.u32(coin + 0x30) != 0 {
                classify_coincident(m, o, tree, coin);
            }
            let sides = [(back, front, t::BACK), (front, back, t::FRONT)];
            for (g, other, slot) in sides {
                if m.u32(g + 0x30) != 0 {
                    if m.u32(coin + 0x30) == 0 && m.u32(other + 0x30) == 0 {
                        for i in 0..6 {
                            let w = m.u32(group + i * 4);
                            m.set_u32(g + i * 4, w);
                        }
                        for i in 0..6 {
                            let w = m.u32(group + 0x18 + i * 4);
                            m.set_u32(g + 0x18 + i * 4, w);
                        }
                    } else {
                        compute_face_group_size(m, g);
                    }
                    let r = classify_faces(m, o, m.u32(tree + slot), g);
                    m.set_u32(tree + slot, r);
                }
            }
        }
    }
    tree
}

/// Hangs the front part `fr` of a split face into the region cell `old` the face stood in (`reg` is the face's
/// field 0x6c or 0x70): the cell now points to the back part `bk`, a new cell (the cell's tree, `fr`) is
/// linked in after it.
fn xx_split_cell(m: &mut Mem, o: &mut dyn Outside, old: u32, bk: u32, fr: u32, reg: u32) {
    let tree = m.u32(old + 4);
    m.set_u32(old, bk);
    m.set_u32(bk + reg, old);
    let cell = call(m, o, SI_CREATE_FACE_LIST, &[tree, fr]);
    m.set_u32(fr + reg, cell);
    let cell = m.u32(fr + reg);
    let nx = m.u32(old + 0xc);
    m.set_u32(cell + 0xc, nx);
    let nx = m.u32(old + 0xc);
    if nx != 0 {
        let c2 = m.u32(fr + reg);
        m.set_u32(nx + 8, c2);
    }
    let c2 = m.u32(fr + reg);
    m.set_u32(old + 0xc, c2);
    let c2 = m.u32(fr + reg);
    m.set_u32(c2 + 8, old);
}

/// `xxxx(tree, group)` [soft 0x0830faec xxxx]: the `classifyFaces` walk that keeps the faces: at a leaf the group's
/// faces get the status 2 (union with an inside leaf, intersection/difference with an outside leaf) or 3. At a node
/// the faces are sorted by side as in `classifyFaces`, but a cut face moves its region cells to the two parts, the
/// coincident ones are classified, the back and front groups recurse, and the three lists are joined (coincident,
/// back, front) into the group's list again.
pub fn xxxx(m: &mut Mem, o: &mut dyn Outside, tree: u32, group: u32) {
    if tree == 0 {
        return;
    }
    if m.i32(tree + t::KIND) == 1 {
        let oper = m[OPER] as i32;
        let inside = m.i32(tree + t::INSIDE);
        let st = if (oper == 1 && inside == 1) || (oper > 0 && oper < 4 && oper != 1 && inside == 0) { 2 } else { 3 };
        let first = m.u32(group + 0x30);
        set_status(m, first, st);
        return;
    }
    match test_box(m, group, group + 0x18, tree + t::PLANE) {
        2 => {
            xxxx(m, o, m.u32(tree + t::FRONT), group);
            return;
        }
        3 => {
            xxxx(m, o, m.u32(tree + t::BACK), group);
            return;
        }
        _ => {}
    }
    let coin = call(m, o, SI_CREATE_FACE_GROUP, &[]);
    let back = call(m, o, SI_CREATE_FACE_GROUP, &[]);
    let front = call(m, o, SI_CREATE_FACE_GROUP, &[]);
    let mut face = m.u32(group + 0x30);
    while face != 0 {
        let next = extract_face(m, face);
        match which_side(m, face, tree) {
            1 => cf_push(m, coin, face),
            2 => cf_push(m, front, face),
            3 => cf_push(m, back, face),
            4 => {
                let rb = m.u32(face + f::REG_B);
                let ra = m.u32(face + f::REG_A);
                m.set_u32(face + f::REG_B, 0);
                m.set_u32(face + f::REG_A, 0);
                let pq = m.local(8);
                crate::split::si_split_face(m, o, tree, face, pq, pq + 4);
                let (bk, fr) = (m.u32(pq), m.u32(pq + 4));
                m.release(8);
                if bk != 0 {
                    if fr != 0 {
                        xx_split_cell(m, o, rb, bk, fr, f::REG_B);
                        xx_split_cell(m, o, ra, bk, fr, f::REG_A);
                    }
                    cf_push_list(m, back, bk);
                }
                cf_push_list(m, front, fr);
            }
            _ => {}
        }
        face = next;
    }
    m.set_u32(group + 0x30, 0);
    if m.u32(coin + 0x30) != 0 {
        classify_coincident(m, o, tree, coin);
    }
    let sides = [(back, front, t::BACK), (front, back, t::FRONT)];
    for (g, other, slot) in sides {
        if m.u32(g + 0x30) != 0 {
            if m.u32(coin + 0x30) == 0 && m.u32(other + 0x30) == 0 {
                for i in 0..6 {
                    let w = m.u32(group + i * 4);
                    m.set_u32(g + i * 4, w);
                }
                for i in 0..6 {
                    let w = m.u32(group + 0x18 + i * 4);
                    m.set_u32(g + 0x18 + i * 4, w);
                }
            } else {
                compute_face_group_size(m, g);
            }
            xxxx(m, o, m.u32(tree + slot), g);
        }
    }
    let mut head = 0u32;
    let mut last = 0u32;
    let c = m.u32(coin + 0x30);
    if c != 0 {
        head = c;
        last = find_last_face_of_list(m, c);
    }
    let b = m.u32(back + 0x30);
    if b != 0 {
        if head != 0 {
            m.set_u32(b + f::PREV, last);
            m.set_u32(last + f::NEXT, b);
        } else {
            head = b;
        }
        last = find_last_face_of_list(m, b);
    }
    let fl = m.u32(front + 0x30);
    if fl != 0 {
        if head != 0 {
            m.set_u32(fl + f::PREV, last);
            m.set_u32(last + f::NEXT, fl);
        } else {
            head = fl;
        }
    }
    m.set_u32(group + 0x30, head);
}

/// `SI_nodeGetUV` (0x084534d8), another unit's: answered by the harness's stand-in.
pub const SI_NODE_GET_UV: Routine = Routine::new("SI_nodeGetUV", 0x0845_34d8);

/// `computeBarycentricUVCoord(poly, which, a, b, c, node)` [soft 0x08311020 computeBarycentricUVCoord]: the UV
/// pair `which` of `node` set to `a*uv0 + b*uv1 + c*uv2` of the polygon's first three corner nodes (36 bytes apart),
/// each sum `((a*x0 + b*x1) + c*x2)` in 80 bits and rounded to float at the end. The reads and the write are
/// `SI_nodeGetUV` and `SI_nodeSetUV` (0x08453538), answered by stand-ins.
pub fn compute_barycentric_uv_coord(m: &mut Mem, o: &mut dyn Outside, poly: u32, which: u32, a: f32, b: f32, c: f32, node: u32) {
    let base = m.local(24);
    let nodes = m.u32(poly + 0xc);
    call(m, o, SI_NODE_GET_UV, &[nodes, which, base + 16, base + 20]);
    call(m, o, SI_NODE_GET_UV, &[nodes + 0x24, which, base + 8, base + 12]);
    call(m, o, SI_NODE_GET_UV, &[nodes + 0x48, which, base, base + 4]);
    let g = |m: &Mem, at: u32| x(m.f32(at));
    let v = ((x(a) * g(m, base + 20) + x(b) * g(m, base + 12)) + x(c) * g(m, base + 4)).to_f32();
    let u = ((x(a) * g(m, base + 16) + x(b) * g(m, base + 8)) + x(c) * g(m, base)).to_f32();
    m.release(24);
    call(m, o, SI_NODE_SET_UV, &[which, u.to_bits(), v.to_bits(), node]);
}

/// `computeBarycentricColor(poly, a, b, c, out)` [soft 0x08310e74 computeBarycentricColor]: the packed colour
/// (R in the low byte, A in the top) of the weights `a`, `b`, `c` over the polygon's first three corner nodes
/// (colour bytes at +0x1c, +0x40, +0x64 of the nodes array, i.e. 0x24 apart). Each channel is
/// `((b0*a + b1*b) + b2*c)` in 80 bits, cut toward zero to an int, only clamped from above to 255.
pub fn compute_barycentric_color(m: &mut Mem, poly: u32, a: f32, b: f32, c: f32, out: u32) {
    let n = m.u32(poly + 0xc);
    let ch = |m: &Mem, k: u32| -> i32 {
        let byte = |off: u32| F80::from_i32(m.u8(n + off + k) as i32);
        let sum = (byte(0x1c) * x(a) + byte(0x40) * x(b)) + byte(0x64) * x(c);
        sum.to_i32(Round::Zero)
    };
    let mut r = [ch(m, 0), ch(m, 1), ch(m, 2), ch(m, 3)];
    for v in r.iter_mut() {
        if *v > 0xff {
            *v = 0xff;
        }
    }
    let packed = r[1].wrapping_mul(0x100).wrapping_add(r[0]).wrapping_add(r[2].wrapping_mul(0x10000)).wrapping_add(r[3].wrapping_mul(0x1000000));
    m.set_i32(out, packed);
}

/// The doubled area of a triangle, as `SI_areaTriangle3D` (0x08517fc8) answers it (the same arithmetic as
/// si-math's port: two of the products of the z component are rounded to float, the rest is 80-bit).
fn area_tri3d(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let cx_ay = (x(c[0]) * a[1]).to_f32();
    let cy_ax = (x(c[1]) * a[0]).to_f32();
    let nx = ((x(b[1]) * c[2] - x(b[2]) * c[1]) + (x(a[1]) * b[2] - x(a[2]) * b[1])) + (x(c[1]) * a[2] - x(c[2]) * a[1]);
    let ny = ((x(b[2]) * c[0] - x(b[0]) * c[2]) + (x(a[2]) * b[0] - x(a[0]) * b[2])) + (x(c[2]) * a[0] - x(c[0]) * a[2]);
    let nz = ((x(b[0]) * c[1] - x(b[1]) * c[0]) + (x(a[0]) * b[1] - x(a[1]) * b[0])) + (x(cx_ay) - cy_ax);
    (nz * nz + (ny * ny + nx * nx)).sqrt().to_f32()
}

/// `computeTriangleBarycentricArea(mesh, n, polys, p, pindex, pa, pb, pc)`
/// [soft 0x08310be8 computeTriangleBarycentricArea]: finds the first of the `n` polygons (24 bytes apart, corner count
/// at +0x10, nodes at +0xc, 36 bytes each with the point index first, points 20 bytes apart at `*mesh`) with three
/// corners whose triangle holds `p`: the weights are the doubled areas of (p,b,c), (a,p,c), (a,b,p) over the
/// triangle's own (rounded to float), held in 80 bits, and the point is in when the sums of two of them are at most
/// 1.000001. Answers 1 with the index and the three weights (as floats), else 0 with zeros.
pub fn compute_triangle_barycentric_area(m: &mut Mem, mesh: u32, n: i32, polys: u32, p: [f32; 3], pindex: u32, pa: u32, pb: u32, pc: u32) -> u32 {
    m.set_f32(pa, 0.0);
    m.set_f32(pb, 0.0);
    m.set_f32(pc, 0.0);
    m.set_u32(pindex, 0);
    let k = x(1.000001f64);
    let mut i = 0i32;
    let mut poly = polys;
    while i < n {
        if m.i16(poly + 0x10) == 3 {
            let nodes = m.u32(poly + 0xc);
            let pts = m.u32(mesh);
            let pt = |m: &Mem, off: u32| -> [f32; 3] {
                let at = pts.wrapping_add(m.u32(nodes + off).wrapping_mul(0x14));
                [m.f32(at), m.f32(at + 4), m.f32(at + 8)]
            };
            let (a, b, c) = (pt(m, 0), pt(m, 0x24), pt(m, 0x48));
            let area0 = x(area_tri3d(a, b, c));
            let alpha = x(area_tri3d(p, b, c)) / area0;
            let beta = x(area_tri3d(a, p, c)) / area0;
            let gamma = x(area_tri3d(a, b, p)) / area0;
            if (alpha + beta).le(k) && (beta + gamma).le(k) && (alpha + gamma).le(k) {
                m.set_f32(pa, alpha.to_f32());
                m.set_f32(pb, beta.to_f32());
                m.set_f32(pc, gamma.to_f32());
                m.set_u32(pindex, i as u32);
                return 1;
            }
        }
        poly += 0x18;
        i += 1;
    }
    0
}

/// `SI_matrixZAlign` (0x08518aa0), `SI_triangulateContour` (0x0851dadc), `SI_polygonAllocate` (0x08459e20),
/// `SI_polygonInitialize` (0x0845a694) and `SI_nodesCopy` (0x08453d44) belong to other units: stand-ins.
pub const SI_MATRIX_Z_ALIGN: Routine = Routine::new("SI_matrixZAlign", 0x0851_8aa0);
pub const SI_TRIANGULATE_CONTOUR: Routine = Routine::new("SI_triangulateContour", 0x0851_dadc);
pub const SI_POLYGON_ALLOCATE: Routine = Routine::new("SI_polygonAllocate", 0x0845_9e20);
pub const SI_POLYGON_INITIALIZE: Routine = Routine::new("SI_polygonInitialize", 0x0845_a694);
pub const SI_NODES_COPY: Routine = Routine::new("SI_nodesCopy", 0x0845_3d44);

/// `polygonTesselate(mesh, poly, pcount)` [soft 0x083108cc polygonTesselate]: cuts one polygon of a mesh into
/// triangles. Copies the polygon's corner point indices (first word of each 36-byte node) and the mesh's points
/// (20 bytes apart, 3 floats kept) into scratch blocks, moves the points by the matrix `SI_matrixZAlign` makes of the
/// polygon's normal (each coordinate `((x*a + y*b) + z*c) + d` in 80 bits, rounded to float), has
/// `SI_triangulateContour` triangulate, and builds one triangle polygon per answer (`SI_polygonAllocate`,
/// `SI_polygonInitialize`; a node copy `SI_nodesCopy` for every corner of the old polygon that is a corner of the
/// triangle). Answers the new polygons (0 when the polygon or the mesh is empty or the triangulation fails; then
/// `*pcount` is 0 only in the empty case).
pub fn polygon_tesselate(m: &mut Mem, o: &mut dyn Outside, mesh: u32, poly: u32, pcount: u32) -> u32 {
    let ncorner = m.u16(poly + 0x10) as u32;
    if ncorner == 0 || m.i32(mesh + 0x14) == 0 {
        m.set_u32(pcount, 0);
        return 0;
    }
    let idx = call(m, o, SI_MALLOC, &[ncorner << 2]);
    let nv = m.i32(mesh + 0x14);
    let pts = call(m, o, SI_MALLOC, &[(nv as u32).wrapping_mul(12)]);
    let nodes = m.u32(poly + 0xc);
    let mut i = 0u32;
    loop {
        let w = m.u32(nodes + i * 0x24);
        m.set_u32(idx + i * 4, w);
        i += 1;
        if i >= m.u16(poly + 0x10) as u32 {
            break;
        }
    }
    let src = m.u32(mesh);
    let mut v = 0i32;
    while v < m.i32(mesh + 0x14) {
        for k in 0..3 {
            let w = m.u32(src + v as u32 * 0x14 + k * 4);
            m.set_u32(pts + v as u32 * 12 + k * 4, w);
        }
        v += 1;
    }
    let mat = m.local(64);
    let ptris = m.local(4);
    let pcnt = m.local(4);
    m.set_u32(ptris, 0);
    m.set_u32(pcnt, 0);
    call(m, o, SI_MATRIX_Z_ALIGN, &[poly, mat]);
    let e = |m: &Mem, k: u32| x(m.f32(mat + k * 4));
    let mut v = 0i32;
    while v < m.i32(mesh + 0x14) {
        let at = pts + v as u32 * 12;
        let (px, py, pz) = (x(m.f32(at)), x(m.f32(at + 4)), x(m.f32(at + 8)));
        let nx = (((px * e(m, 0) + py * e(m, 4)) + pz * e(m, 8)) + e(m, 12)).to_f32();
        let ny = (((px * e(m, 1) + py * e(m, 5)) + pz * e(m, 9)) + e(m, 13)).to_f32();
        let nz = (((px * e(m, 2) + py * e(m, 6)) + pz * e(m, 10)) + e(m, 14)).to_f32();
        m.set_f32(at, nx);
        m.set_f32(at + 4, ny);
        m.set_f32(at + 8, nz);
        v += 1;
    }
    let nv = m.u32(mesh + 0x14);
    let ok = call(m, o, SI_TRIANGULATE_CONTOUR, &[nv, pts, m.u16(poly + 0x10) as u32, idx, pcnt, ptris, 1]) as u16;
    if ok == 0 {
        if idx != 0 {
            m.free(idx);
        }
        if pts != 0 {
            m.free(pts);
        }
        m.release(72);
        return 0;
    }
    let count = m.i32(pcnt);
    let out = call(m, o, SI_POLYGON_ALLOCATE, &[count as u32]);
    m.set_i32(pcount, count);
    let mut t = 0i32;
    while t < count {
        let tris = m.u32(ptris);
        let (a, b, c) = (m.u32(tris + t as u32 * 12), m.u32(tris + t as u32 * 12 + 4), m.u32(tris + t as u32 * 12 + 8));
        call(m, o, SI_POLYGON_INITIALIZE, &[out + t as u32 * 0x18, 3, a, b, c]);
        t += 1;
        let mut j = 0u32;
        while j < m.u16(poly + 0x10) as u32 {
            let first = m.u32(m.u32(poly + 0xc) + j * 0x24);
            let tris = m.u32(ptris);
            let dst_nodes = m.u32(out + (t as u32 - 1) * 0x18 + 0xc);
            let dst = if first == m.u32(tris + (t as u32 - 1) * 12) {
                Some(dst_nodes)
            } else if first == m.u32(tris + (t as u32 - 1) * 12 + 4) {
                Some(dst_nodes + 0x24)
            } else if first == m.u32(tris + (t as u32 - 1) * 12 + 8) {
                Some(dst_nodes + 0x48)
            } else {
                None
            };
            if let Some(dst) = dst {
                let srcn = m.u32(poly + 0xc) + j * 0x24;
                call(m, o, SI_NODES_COPY, &[0, 0, dst, srcn]);
            }
            j += 1;
        }
    }
    if idx != 0 {
        m.free(idx);
    }
    if pts != 0 {
        m.free(pts);
    }
    let tris = m.u32(ptris);
    if tris != 0 {
        m.free(tris);
    }
    m.release(72);
    out
}

/// `si_getResultingFaces(tree, pfirst, plast)` [soft 0x08313ab0 si_getResultingFaces]: gathers the faces kept in the
/// tree into one list (first face in `*pfirst`, last in `*plast`): the back subtree's, the front subtree's, then the
/// node's own list (whose slot is cleared). A leaf or no tree gives an empty list.
pub fn si_get_resulting_faces(m: &mut Mem, tree: u32, pfirst: u32, plast: u32) {
    m.set_u32(pfirst, 0);
    m.set_u32(plast, 0);
    if tree == 0 || m.i32(tree + t::KIND) == 1 {
        return;
    }
    for slot in [t::BACK, t::FRONT] {
        let sub = m.u32(tree + slot);
        if sub != 0 {
            let l = m.local(8);
            si_get_resulting_faces(m, sub, l, l + 4);
            let (first, last) = (m.u32(l), m.u32(l + 4));
            let (a, alast) = (m.u32(pfirst), m.u32(plast));
            let r = merge_two_lists_of_faces(m, a, alast, first, last, plast);
            m.set_u32(pfirst, r);
            m.release(8);
        }
    }
    let own = m.u32(tree + t::FACES);
    if own != 0 {
        let (a, alast) = (m.u32(pfirst), m.u32(plast));
        let r = merge_two_lists_of_faces(m, a, alast, own, 0, plast);
        m.set_u32(pfirst, r);
        m.set_u32(tree + t::FACES, 0);
    }
}

/// `si_releaseMem()` [soft 0x08315260 si_releaseMem]: frees every block of the allocator's list (`headMemBlk`, next
/// at +8). The head is not cleared.
pub fn si_release_mem(m: &mut Mem) {
    let mut b = m[HEAD_MEM_BLK];
    while b != 0 {
        let cur = b;
        b = m.u32(b + 8);
        m.free(cur);
    }
}

/// The flag bits `fnstsw` leaves after `fcom st0 = a` against `b` and `and ah, 0x45`: 0 greater, 1 less,
/// 0x40 equal, 0x45 unordered.
pub(crate) fn fcom(a: F80, b: F80) -> u8 {
    match a.compare(b) {
        Some(core::cmp::Ordering::Greater) => 0,
        Some(core::cmp::Ordering::Less) => 1,
        Some(core::cmp::Ordering::Equal) => 0x40,
        None => 0x45,
    }
}

/// `verifyEdge(tree, edge)` [soft 0x0830d9f4 verifyEdge]: finds where the other edges of the tree's face list
/// touch `edge` and splits `edge` there. `count.291` grows by 2 per call (back to 100 above 64000). The
/// direction of the edge is its difference vector divided by the 80-bit root of the sum of squares (the first
/// square is stored as a double, the first sum too; the length is stored as a double, the unit vector as
/// doubles). Each point of each edge node of each contour of each face is visited once (its `+0x50` is set to
/// `count`): its distance to the face planes of `edge` (the two stored as doubles, the second a copy of the first
/// when both faces have the same id) must lie within 5e-5, and its projection on the line (a signed distance
/// along the unit vector, negative when the dot product is) within 5e-7 per axis of the point; then the point's
/// distance (`+0x48`) is stored and its `+0x50` is `count + 1`. Every edge with such a point (or two, the span
/// ordered) is kept in a list sorted by its low end, an entry merged into an old one when the old span is
/// shorter than 1e-6 and an end within 1e-6 (nodes of 0x1c bytes from `si_getMem`: edge, low, high, prev,
/// next). Then each listed edge is clipped to `edge` with `addPtToEdge`/`insertEdge`, depending on which of
/// its ends lie on the span `[0, length]`; `edge` may become one of the new edges, or 0 (the walk stops).
pub fn verify_edge(m: &mut Mem, o: &mut dyn Outside, tree: u32, mut edge: u32) {
    let mut count = m[COUNT_291].wrapping_add(2);
    if 64000 < count {
        count = 100;
    }
    m[COUNT_291] = count;
    let faces = [m.u32(edge + e::FACE), m.u32(edge + e::FACE + 4)];
    let same = faces[0] != 0 && faces[1] != 0 && m.i32(faces[0] + f::ID) == m.i32(faces[1] + f::ID);
    let mut planes = [[0f64; 4]; 2];
    for i in 0..2 {
        if faces[i] != 0 {
            for j in 0..4 {
                planes[i][j] = m.f64(faces[i] + 8 * j as u32);
            }
        }
    }
    let ends = [m.u32(edge + e::PT), m.u32(edge + e::PT + 4)];
    let o0 = [d(m, ends[0]), d(m, ends[0] + 8), d(m, ends[0] + 0x10)];
    let t = [d(m, ends[1]) - o0[0], d(m, ends[1] + 8) - o0[1], d(m, ends[1] + 0x10) - o0[2]];
    let sum1 = x((t[1] * t[1] + x((t[0] * t[0]).to_f64())).to_f64());
    let root = (t[2] * t[2] + sum1).sqrt();
    let dir = [x((t[0] / root).to_f64()), x((t[1] / root).to_f64()), x((t[2] / root).to_f64())];
    let len = x(root.to_f64());
    let (e5, e7, e6) = (x(5e-5), x(5e-7), x(1e-6));
    let mut head = 0u32;
    let mut face = m.u32(tree + 0x24);
    while face != 0 {
        let mut con = m.u32(face + f::CONTOURS);
        while con != 0 {
            let mut node = m.u32(con + c::FIRST);
            loop {
                let ed = m.u32(node + n::EDGE);
                let pts = [m.u32(ed + e::PT), m.u32(ed + e::PT + 4)];
                for &pt in &pts {
                    let p = [d(m, pt), d(m, pt + 8), d(m, pt + 0x10)];
                    let cnt = m[COUNT_291];
                    let id = m.i32(pt + p::PLANE);
                    if id == cnt || id == cnt.wrapping_add(1) {
                        continue;
                    }
                    m.set_i32(pt + p::PLANE, cnt);
                    let mut out = [0f64; 2];
                    for i in 0..2 {
                        if faces[i] == 0 {
                            out[i] = 0.0;
                        } else if i == 0 || !same {
                            let pl = &planes[i];
                            let s = p[0] * x(pl[0]) + p[1] * x(pl[1]);
                            let s = s + p[2] * x(pl[2]);
                            out[i] = (s + x(pl[3])).to_f64();
                        } else {
                            out[1] = out[0];
                        }
                    }
                    let (o_a, o_b) = (x(out[0]), x(out[1]));
                    if !(o_a.lt(e5) && o_a.gt(-e5) && o_b.lt(e5) && o_b.gt(-e5)) {
                        continue;
                    }
                    let b = [p[0] - o0[0], p[1] - o0[1], p[2] - o0[2]];
                    let sq = b[1] * b[1] + b[0] * b[0];
                    let mut dist = (sq + b[2] * b[2]).sqrt();
                    let dot = (b[1] * dir[1] + b[0] * dir[0]) + b[2] * dir[2];
                    if x(0.0).gt(dot) {
                        dist = -dist;
                    }
                    let r = [dist * dir[0] + o0[0], dist * dir[1] + o0[1], dist * dir[2] + o0[2]];
                    if (p[0] - r[0]).abs().lt(e7) && (p[1] - r[1]).abs().lt(e7) && (p[2] - r[2]).abs().lt(e7) {
                        m.set_f64(pt + p::DIST, dist.to_f64());
                        m.set_i32(pt + p::PLANE, cnt.wrapping_add(1));
                    }
                }
                let cnt = m[COUNT_291];
                let k = cnt.wrapping_add(1);
                let (id0, id1) = (m.i32(pts[0] + p::PLANE), m.i32(pts[1] + p::PLANE));
                let (d0, d1) = (d(m, pts[0] + p::DIST), d(m, pts[1] + p::DIST));
                let zero = x(0.0);
                let a_hit = id0 == k && d0.gt(zero) && d0.lt(len);
                let b_hit = id1 == k && d1.gt(zero) && d1.lt(len);
                let mut insert = a_hit || b_hit;
                if !insert && id0 == k && id1 == id0 {
                    let le = |f: u8| f == 1 || f == 0x40;
                    let to_b = if le(fcom(d0, zero)) { fcom(zero, d1) & 5 != 0 } else { true };
                    if to_b {
                        insert = if fcom(d0, len) & 5 != 0 { true } else { !le(fcom(len, d1)) };
                    }
                }
                if insert {
                    let (lo, hi) = if id0 == k {
                        if id1 == id0 {
                            if fcom(d1, d0) == 1 { (d1, d0) } else { (d0, d1) }
                        } else {
                            (d0, d0)
                        }
                    } else {
                        (d1, d1)
                    };
                    let mut prev = 0u32;
                    let mut cur = head;
                    while cur != 0 {
                        let f = fcom(lo, d(m, cur + 4));
                        if f == 1 || f == 0x40 {
                            break;
                        }
                        prev = cur;
                        cur = m.u32(cur + 0x18);
                    }
                    let mut skip = false;
                    let mut update = false;
                    if cur != 0 {
                        let (cl, ch) = (d(m, cur + 4), d(m, cur + 0xc));
                        if (cl - ch).abs().lt(e6) {
                            if (lo - cl).abs().lt(e6) || (hi - cl).abs().lt(e6) {
                                update = true;
                            }
                        } else if fcom(lo, hi) == 0x40 || (fcom(lo, cl) == 0x40 && fcom(ch, hi) == 0x40) {
                            skip = true;
                        }
                    }
                    if update {
                        m.set_f64(cur + 4, lo.to_f64());
                        m.set_f64(cur + 0xc, hi.to_f64());
                        m.set_u32(cur, ed);
                    } else if !skip {
                        let nn = si_get_mem(m, o, 0x1c);
                        m.set_u32(nn, ed);
                        m.set_f64(nn + 4, lo.to_f64());
                        m.set_f64(nn + 0xc, hi.to_f64());
                        m.set_u32(nn + 0x18, cur);
                        m.set_u32(nn + 0x14, prev);
                        if prev != 0 {
                            m.set_u32(prev + 0x18, nn);
                        } else {
                            head = nn;
                        }
                        if cur != 0 {
                            m.set_u32(cur + 0x14, nn);
                        }
                    }
                }
                node = m.u32(node + n::NEXT);
                if node == m.u32(con + c::FIRST) {
                    break;
                }
            }
            con = m.u32(con + c::NEXT);
        }
        face = m.u32(face + f::NEXT);
    }
    let zero = x(0.0);
    let mut cur = head;
    while cur != 0 && edge != 0 {
        let e2 = m.u32(cur);
        let (pa, pb) = (m.u32(e2 + e::PT), m.u32(e2 + e::PT + 4));
        let k = m[COUNT_291].wrapping_add(1);
        let (w0, w1) = (m.i32(pa + p::PLANE) == k, m.i32(pb + p::PLANE) == k);
        let (da, db) = (d(m, pa + p::DIST), d(m, pb + p::DIST));
        let fa = zero.le(da) && da.le(len);
        let fb = zero.le(db) && db.le(len);
        // a point not yet an end of the (current) edge
        let fresh = |m: &Mem, pe: u32, pt: u32| pt != m.u32(pe + e::PT) && pt != m.u32(pe + e::PT + 4);
        if !w0 {
            if w1 && fb {
                let pt = m.u32(e2 + e::PT + 4);
                if fresh(m, edge, pt) {
                    edge = add_pt_to_edge(m, o, edge, pt, 0);
                }
            }
        } else if !w1 {
            if fa {
                let pt = m.u32(e2 + e::PT);
                if fresh(m, edge, pt) {
                    edge = add_pt_to_edge(m, o, edge, pt, 0);
                }
            }
        } else if fa && fb {
            let mut nw = 0u32;
            let pt = m.u32(e2 + e::PT);
            if fresh(m, edge, pt) {
                let r = add_pt_to_edge(m, o, edge, pt, 0);
                if db.lt(da) {
                    nw = r;
                } else {
                    edge = r;
                }
            }
            let pt = m.u32(e2 + e::PT + 4);
            if fresh(m, edge, pt) {
                let r = add_pt_to_edge(m, o, edge, pt, 0);
                if da.lt(db) {
                    nw = r;
                } else {
                    edge = r;
                }
            }
            insert_edge(m, e2, edge);
            edge = nw;
        } else if fa {
            let pt = m.u32(e2 + e::PT);
            let (esi, mut nw) = if fresh(m, edge, pt) {
                let r = add_pt_to_edge(m, o, edge, pt, 0);
                (r, r)
            } else {
                (edge, 0)
            };
            if db.lt(zero) && m.u32(e2 + e::PT) != m.u32(edge + e::PT) {
                let q = m.u32(edge + e::PT);
                add_pt_to_edge(m, o, e2, q, 0);
                insert_edge(m, e2, edge);
            } else if db.gt(len) && m.u32(e2 + e::PT + 4) != m.u32(esi + e::PT + 4) {
                let q = m.u32(esi + e::PT + 4);
                add_pt_to_edge(m, o, e2, q, 0);
                insert_edge(m, e2, esi);
                nw = 0;
            }
            edge = nw;
        } else if fb {
            let pt = m.u32(e2 + e::PT + 4);
            let (esi, mut nw) = if fresh(m, edge, pt) {
                let r = add_pt_to_edge(m, o, edge, pt, 0);
                (r, r)
            } else {
                (edge, 0)
            };
            if da.lt(zero) && m.u32(e2 + e::PT + 4) != m.u32(edge + e::PT) {
                let q = m.u32(edge + e::PT);
                let nb = add_pt_to_edge(m, o, e2, q, 0);
                insert_edge(m, nb, edge);
            } else if len.lt(db) && m.u32(e2 + e::PT + 4) != m.u32(esi + e::PT + 4) {
                let q = m.u32(esi + e::PT + 4);
                let nb = add_pt_to_edge(m, o, e2, q, 0);
                insert_edge(m, nb, esi);
                nw = 0;
            }
            edge = nw;
        } else {
            let q0 = m.u32(edge + e::PT);
            let r1 = add_pt_to_edge(m, o, e2, q0, 0);
            let q1 = m.u32(edge + e::PT + 4);
            let nb = if zero.gt(da) {
                add_pt_to_edge(m, o, r1, q1, 0);
                r1
            } else {
                add_pt_to_edge(m, o, e2, q1, 0)
            };
            insert_edge(m, nb, edge);
        }
        cur = m.u32(cur + 0x18);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Mem {
        let mut m = Mem::new();
        si_mem::init_globals(&mut m, ALL);
        m
    }

    #[test]
    fn merge_dist_roundtrip() {
        let mut m = mem();
        set_merge_dist(&mut m, 0.125);
        assert_eq!(m.f64(MERGE_DIST.addr()), 0.125);
    }

    #[test]
    fn nb_res_features_numbers_faces_downwards() {
        let mut m = mem();
        let face = 0x087b_a000; // inside the image, past the globals
        nb_res_features(&mut m, face);
        nb_res_features(&mut m, face);
        assert_eq!(m[NB_RES_FACES], 2);
        assert_eq!(m.i32(face + f::RES_ID), !1);
    }
}
