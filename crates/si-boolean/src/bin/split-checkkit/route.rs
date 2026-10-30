// SPDX-License-Identifier: MIT
// included by split-checkkit and ops-checkkit: the stand-ins of the routines the originals ran as their own code
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
        0x0831_70ec => Some(Ret::eax(util::si_create_edge_node(m, &mut Never))),
        0x0831_7118 => Some(Ret::eax(util::si_create_contour(m, &mut Never, args[0]))),
        0x0831_7144 => Some(Ret::eax(util::si_create_bool_face(m, &mut Never, args[0]))),
        0x080d_0ee8 => Some(Ret::eax(m.si_malloc(args[0]))), // SI_malloc: the allocator's own
        0x0831_70d0 => Some(Ret::eax(util::si_create_face_group(m, &mut Never))),
        0x0831_7208 => Some(Ret::eax(util::si_create_reg_bsp_node(m, &mut Never, args[0]))),
        0x0831_7234 => Some(Ret::eax(util::si_create_face_list(m, &mut Never, args[0], args[1]))),
        0x0831_3b74 => {
            // si_splitFace(tree, face, &back, &front), the port's own
            si_boolean::split::si_split_face(m, &mut Never, args[0], args[1], args[2], args[3]);
            Some(Ret::eax(0))
        }
        _ => None,
    }
}

