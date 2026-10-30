// SPDX-License-Identifier: MIT
//! The shape file reader of `binary.c` on the image (finding 0005): the shapes of a model
//! (`.shp`), each added to the model's list of definitions, as shape animation then blends
//! between them. `si-elemio`'s `shape` module reads the same files into values.

use super::globals::{FILE_VERSION, SI_OBJECT_PATH, SI_PIPE, SI_PIPED_FILES};
use super::io::{byteswap_fread, file_skip_field, read_bin_header};
use super::mesh::mesh_binary_read;
use super::modelwrite::filename_cleanup;
use super::node::{model_add_definition, node, reader, FACE_BINARY_READ, NURBS_CRV_BINARY_READ, NURBS_SRF_BINARY_READ, PATCH_BINARY_READ, SPLINE_BINARY_READ};
use super::nurbs::{nurbs_crv_binary_read, nurbs_srf_binary_read};
use super::outside::{Outside, Routine};
use super::patch::patch_binary_read;
use super::proto::{prototype_allocate, prototype_binary_read, prototype_dispose, Prototypes};
use super::spline::{face_binary_read, spline_binary_read, spline_copy};
use si_mem::{Mem, Ptr32};

/// `"rb"`, the mode the shape reader opens with.
pub const S_RB_SHP: u32 = 0x086ce797;

/// The node's current definition (`__SI_Model` +0x3c), the one being shown.
pub const CURRENT: u32 = 0x3c;
/// The short a shape file stores after a count of −2 (+0x2e2): whether the blend divides
/// the weights by their sum (see `si-elemio`'s `ShapeModel::weight_mode`).
pub const WEIGHT_MODE: u32 = 0x2e2;
/// The node's file name (+0x2d4), which the shape reader replaces by its own file's.
pub const FILE_NAME: u32 = 0x2d4;

macro_rules! routines {
    ( $( $(#[$meta:meta])* $konst:ident = $name:literal, $addr:literal; )* ) => {
        $( $(#[$meta])* pub const $konst: Routine = Routine { name: $name, addr: $addr }; )*
    };
}

routines! {
    /// `SI_patchUpdateSurface (0x0847cee4)`, `patch.c`: the surface of a patch from its
    /// control points.
    SI_PATCH_UPDATE_SURFACE = "SI_patchUpdateSurface", 0x0847cee4;
    /// `SI_patchCopy (0x0847a2b8)`, `patch.c`.
    SI_PATCH_COPY = "SI_patchCopy", 0x0847a2b8;
    /// `SI_faceCopy (0x08434990)`, `face.c`.
    SI_FACE_COPY = "SI_faceCopy", 0x08434990;
    /// `SI_meshCopy (0x084404e0)`, `obj_global.c`.
    SI_MESH_COPY = "SI_meshCopy", 0x084404e0;
    /// `SI_nurbsCrvCopy (0x084dbbf4)`, `nrb_b.c`.
    SI_NURBS_CRV_COPY = "SI_nurbsCrvCopy", 0x084dbbf4;
    /// `SI_nurbsSrfCopy (0x084dc1a8)`, `nrb_b.c`.
    SI_NURBS_SRF_COPY = "SI_nurbsSrfCopy", 0x084dc1a8;
}

/// The tag the list of definitions must carry for a node of `kind`; `None` for a kind
/// without one.
fn tag_of(kind: i32) -> Option<u32> {
    Some(match kind {
        1 => 0x5054_4348,  // PTCH
        2 => 0x4641_4345,  // FACE
        4 => 0x4d45_5348,  // MESH
        6 => 0x5350_4c4e,  // SPLN
        9 => 0x4e43_5256,  // NCRV
        10 => 0x4e53_5246, // NSRF
        _ => return None,
    })
}

/// One record of the node's kind (1, 2, 4, 6, 9 or 10) from the stream.
fn read_one(m: &mut Mem, o: &mut dyn Outside, file: u32, protos: Ptr32<Prototypes>, model: u32, kind: i32) -> u32 {
    let table = protos.addr();
    match kind {
        1 => reader(m, o, &PATCH_BINARY_READ, &[file, table], |m, o| patch_binary_read(m, o, file, protos).addr()),
        2 => reader(m, o, &FACE_BINARY_READ, &[file, table], |m, o| face_binary_read(m, o, file, protos).addr()),
        4 => mesh_binary_read(m, o, file, protos, model, 0, 0).addr(),
        6 => reader(m, o, &SPLINE_BINARY_READ, &[file, table], |m, o| spline_binary_read(m, o, file, protos).addr()),
        9 => reader(m, o, &NURBS_CRV_BINARY_READ, &[file, table], |m, o| nurbs_crv_binary_read(m, o, file, protos).addr()),
        10 => reader(m, o, &NURBS_SRF_BINARY_READ, &[file, table], |m, o| nurbs_srf_binary_read(m, o, file, protos).addr()),
        _ => 0,
    }
}

/// [soft 0x083f1798 SI_shapeBinaryRead]: the shape file `name` + `.shp` read onto the node
/// `model`. Answers 1, or 0 when there is no stream or when the list of definitions carries
/// another tag than the node's kind has.
///
/// The path is printed into 256 bytes of stack; the file is opened unless files come from
/// a pipe (`SI_pipedFiles`, then `SI_pipe` is the stream). The header is read and its
/// answer not looked at: its version becomes `binary.c`'s `fileVersion` (through the x87
/// unit, which makes a signalling NaN quiet), also when the header was refused and the
/// version is what the stack held. A tag; if it is `PROT`, the prototype block and another
/// tag; a 16-bit count. A negative count brings the current definition (+0x3c): after a
/// count of −2 a short for +0x2e2, then one record of the node's kind (`patchBinaryRead`,
/// `faceBinaryRead`, `meshBinaryRead(stream, table, node, null, 0)`, `splineBinaryRead`,
/// `nurbsCrvBinaryRead`, `nurbsSrfBinaryRead`; for any other kind one field is skipped),
/// stored when it is not null (a patch is also handed to `SI_patchUpdateSurface`), and the
/// count again.
///
/// Then, for a kind with a tag, if the tag is the kind's, `count` records of the kind are
/// read, each one that is not null added with `SI_modelAddDefinition(node, kind, record)`;
/// a node still without a current definition gets a copy of the list's second entry
/// (`SI_patchCopy` and its like; the entry is taken without a look at the list's length).
/// Another tag: nothing is read and the answer is 0. A kind without a tag: one field is
/// skipped. Last, the node's file name (+0x2d4) is freed and replaced by `name` without
/// `SI_objectPath` in front (`si_filenameCleanup`), the stream closed unless it is the pipe,
/// and the prototype table freed.
///
/// A path of 256 bytes or more runs over the buffer into the saved registers and the
/// frame: the port records a fault and answers 0.
pub fn shape_binary_read(m: &mut Mem, o: &mut dyn Outside, name: u32, model: u32) -> i16 {
    const FRAME: u32 = 0x110;
    let frame = m.local(FRAME);
    let (protos_at, count_at, tag_at, version_at, path) = (frame, frame + 4, frame + 8, frame + 12, frame + 16);
    let mut text = m.cstr(name);
    text.extend_from_slice(b".shp");
    if text.len() >= 0x100 {
        m.record_fault(path + 0x100, text.len() as u32 - 0xff, si_mem::Access::Write);
        m.release(FRAME);
        return 0;
    }
    m.put_cstr(path, &text);
    let mut file = m[SI_PIPE];
    if m[SI_PIPED_FILES] == 0 {
        file = o.fopen(m, path, S_RB_SHP);
    }
    if file == 0 {
        m.release(FRAME);
        return 0;
    }
    read_bin_header(m, o, file, version_at, 0);
    byteswap_fread(m, o, tag_at, 4, 1, file);
    // fld / fstp: a signalling NaN comes out quiet
    let mut bits = m.u32(version_at);
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x003f_ffff != 0 {
        bits |= 0x0040_0000;
    }
    m[FILE_VERSION] = f32::from_bits(bits);
    let protos = prototype_allocate(m);
    m.set_u32(protos_at, protos.addr());
    if m.u32(tag_at) == 0x5052_4f54 {
        prototype_binary_read(m, o, file, protos);
        byteswap_fread(m, o, tag_at, 4, 1, file);
    }
    byteswap_fread(m, o, count_at, 2, 1, file);
    if m.i16(count_at) < 0 {
        if m.i16(count_at) == -2 {
            byteswap_fread(m, o, count_at, 2, 1, file);
            let v = m.u16(count_at);
            m.set_u16(model + WEIGHT_MODE, v);
        }
        let kind = m.i32(model + node::TYPE);
        match kind {
            1 => {
                let g = read_one(m, o, file, protos, model, 1);
                if g != 0 {
                    m.set_u32(model + CURRENT, g);
                    o.call(m, &SI_PATCH_UPDATE_SURFACE, &[g]);
                }
            }
            2 | 4 | 6 | 9 | 10 => {
                let g = read_one(m, o, file, protos, model, kind);
                if g != 0 {
                    m.set_u32(model + CURRENT, g);
                }
            }
            _ => {
                file_skip_field(m, o, file, -1);
            }
        }
        byteswap_fread(m, o, count_at, 2, 1, file);
    }
    let kind = m.i32(model + node::TYPE);
    let accepted = match tag_of(kind) {
        Some(tag) if m.u32(tag_at) == tag => {
            if 0 < m.i16(count_at) {
                let mut i: i16 = 0;
                loop {
                    let g = read_one(m, o, file, protos, model, kind);
                    if g != 0 {
                        model_add_definition(m, o, model, m.u32(model + node::TYPE), g);
                    }
                    i = i.wrapping_add(1);
                    if i >= m.i16(count_at) {
                        break;
                    }
                }
            }
            if m.u32(model + CURRENT) == 0 {
                let second = m.u32(m.u32(model + node::DEFINITIONS).wrapping_add(4));
                let copy = match kind {
                    1 => o.call(m, &SI_PATCH_COPY, &[second]),
                    2 => o.call(m, &SI_FACE_COPY, &[second]),
                    4 => o.call(m, &SI_MESH_COPY, &[second]),
                    6 => spline_copy(m, second).addr(),
                    9 => o.call(m, &SI_NURBS_CRV_COPY, &[second]),
                    _ => o.call(m, &SI_NURBS_SRF_COPY, &[second]),
                };
                m.set_u32(model + CURRENT, copy);
            }
            1
        }
        Some(_) => 0,
        None => {
            file_skip_field(m, o, file, -1);
            1
        }
    };
    let old = m.u32(model + FILE_NAME);
    if old != 0 {
        m.free(old);
        m.set_u32(model + FILE_NAME, 0);
    }
    let base = m[SI_OBJECT_PATH];
    let clean = filename_cleanup(m, name, base);
    m.set_u32(model + FILE_NAME, clean);
    if m[SI_PIPED_FILES] == 0 {
        o.fclose(m, file);
    }
    prototype_dispose(m, Ptr32::new(protos_at));
    m.release(FRAME);
    accepted
}
