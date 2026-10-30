// SPDX-License-Identifier: MIT
//! `splitdrv-checkkit <directory of vector files>`: the five drivers of `boolean.c` that call `si_splitFace` (dummyInsert,
//! classifyRegion, si_buildBsp, classifyFaces, xxxx) held against the emulated original, which runs its own `si_splitFace`
//! and the three constructors of `bool_util.c` (vectors: `gen_boolean.py`, labels listed in `self_ported`). The port calls
//! `si_split_face` directly; its calls to the constructors are answered by the Rust below, every other callee by the
//! recorded stand-ins of the vector (the route answers only the four addresses).

use si_boolean::boolean as b;
use si_checkkit::Kit;
use si_mem::{Mem, Outside, Ret, Routine};

/// The outside world the routed routines call among themselves: the same routes (anything else answers 0; the
/// allocator calls it only when it fails).
struct Never;
impl Outside for Never {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> Ret {
        route(m, r, a).unwrap_or(Ret::eax(0))
    }
}

/// The three constructors of `bool_util.c` (another unit) as the check needs them: the original runs its own, the
/// port's calls are answered by these, which only allocate with the port's `si_getMem` and fill the record in.
fn route(m: &mut Mem, r: Routine, args: &[u32]) -> Option<Ret> {
    match r.addr {
        0x0831_70ec => {
            // si_createEdgeNode: 0x14 bytes, visit 0, end 2, edge 0, prev and next itself
            let p = b::si_get_mem(m, &mut Never, 0x14);
            m.set_u32(p, 0);
            m.set_u8(p + 4, 2);
            m.set_u32(p + 8, 0);
            m.set_u32(p + 0x10, p);
            m.set_u32(p + 0xc, p);
            Some(Ret::eax(p))
        }
        0x0831_7118 => {
            // si_createContour(first): 0xc bytes
            let p = b::si_get_mem(m, &mut Never, 0xc);
            m.set_u32(p, args[0]);
            m.set_u32(p + 8, 0);
            m.set_u32(p + 4, 0);
            Some(Ret::eax(p))
        }
        0x0831_7144 => {
            // si_createBoolFace(src): 0x80 bytes, a copy of src or a fresh face
            let p = b::si_get_mem(m, &mut Never, 0x80);
            if args[0] == 0 {
                for k in [0u32, 1, 2, 3, 4, 5, 6, 7, 0x18, 0x14, 0x16, 0x17] {
                    m.set_u32(p + k * 4, 0);
                }
                m.set_u8(p + 0x64, 2);
                m.set_u32(p + 0x70, 0);
                m.set_u32(p + 0x6c, 0);
                m.set_u32(p + 0x68, 0);
                m.set_u8(p + 0x65, 0xff);
                m.set_u32(p + 0x7c, 0xffff_ffff);
            } else {
                m.copy(p, args[0], 0x80);
            }
            m.set_u32(p + 0x78, 0);
            m.set_u32(p + 0x74, 0);
            m.set_u32(p + 0x54, 0);
            Some(Ret::eax(p))
        }
        0x0831_3b74 => {
            // si_splitFace(tree, face, &back, &front), the port's own
            si_boolean::split::si_split_face(m, &mut Never, args[0], args[1], args[2], args[3]);
            Some(Ret::eax(0))
        }
        _ => None,
    }
}

fn main() {
    Kit::new(&[
        ("dummyInsert", |m, o, a| b::dummy_insert(m, o, a.v("tree"), a.v("face"), a.v("which") as u8).into()),
        ("classifyRegion", |m, o, a| b::classify_region(m, o, a.v("regions"), a.v("tree"), a.v("back"), a.v("front")).into()),
        ("si_buildBsp", |m, o, a| b::si_build_bsp(m, o, a.v("tree"), a.v("group"), a.v("status") as u8).into()),
        ("classifyFaces", |m, o, a| b::classify_faces(m, o, a.v("tree"), a.v("group")).into()),
        ("xxxx", |m, o, a| b::xxxx(m, o, a.v("tree"), a.v("group")).into()),
    ])
    .normalise_stack_args()
    .route(route)
    .setup(|m| si_mem::init_globals(m, b::ALL))
    .run()
}
