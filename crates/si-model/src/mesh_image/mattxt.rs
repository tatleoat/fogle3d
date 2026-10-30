// SPDX-License-Identifier: MIT
//! The materials and 2D textures a conversion to a mesh asks a node for (ninth pass):
//! `SI_findMatInfo` (`grMaterial.c`), `SI_mdlGetMatAndTxt2D` and `SI_matAndTxt2DDestroy`
//! (`t2d_fct.c`). The lists are `genList.c`'s (`si-gen`, finding 0033).
//!
//! The answer of [`mdl_get_mat_and_txt2d`] is a list of 8-byte records, one per owner that
//! has a texture of the asked mapping method: the material (null for the textures of the
//! node itself, which come first) and a list of those textures, in their order.

use si_gen::list;
use si_hrc::image::material::mat;
use si_hrc::image::node::node;
use si_hrc::image::texture::lclmap;
use si_mem::{Mem, Routine};

use crate::model_image::{Callee, Outside};

/// `SI_modelMasterFromInstance` (0x081a7490): the master of an instance node.
pub const SI_MODEL_MASTER_FROM_INSTANCE: Callee = Callee { name: "SI_modelMasterFromInstance", addr: 0x081a7490 };

/// The address of [`mat_and_txt2d_destroy`] in the binary: the destructor the list's users
/// hand to `SI_genListDestroy`.
pub const MAT_AND_TXT2D_DESTROY: u32 = 0x082b_3628;

/// An owner's record in the answer of [`mdl_get_mat_and_txt2d`] (`SI_calloc(1, 8)`).
pub mod rec {
    /// The material, or null for the node's own textures.
    pub const MATERIAL: u32 = 0x0;
    /// The list of its textures.
    pub const TEXTURES: u32 = 0x4;
}

/// The algorithm word (+0x2ec) of an instance node.
const INSTANCE: i32 = 0xf;

/// The materials and the first 2D texture a node draws with [soft 0x085eb634 SI_findMatInfo]:
/// first `*nb = 1`, `*mats` the address of `ptrmat` (a one-entry array holding
/// `SI_defaultMaterial`), `*txt = 0`. Then from `model` up through the fathers: the first
/// node met with a first texture (+0x218) gives `*txt`; the first with a materials array
/// (+0x20c) ends the walk. A node without one that is an instance (+0x2ec = 15) asks for its
/// master (`SI_modelMasterFromInstance`), whose texture counts the same way and whose
/// materials, when it has them, end the walk there; otherwise the walk goes on from the
/// instance's father. The node that ends it gives `*mats` and `*nb`: its material count
/// (+0x206, signed) or 1 when that is 0.
pub fn find_mat_info(m: &mut Mem, o: &mut dyn Outside, model: u32, mats: u32, nb: u32, txt: u32) {
    m.set_i32(nb, 1);
    m.set_u32(mats, super::g::PTRMAT.addr());
    m.set_u32(txt, 0);
    let mut n = model;
    let found = loop {
        if n == 0 {
            return;
        }
        if m.u32(txt) == 0 && m.u32(n + node::TEXTURE_CURRENT) != 0 {
            m.set_u32(txt, m.u32(n + node::TEXTURE_CURRENT));
        }
        if m.u32(n + node::MATERIALS) != 0 {
            break n;
        }
        if m.i32(n + node::ALGORITHM) == INSTANCE {
            let master = o.call(m, &SI_MODEL_MASTER_FROM_INSTANCE, &[n]);
            if master != 0 {
                if m.u32(txt) == 0 && m.u32(master + node::TEXTURE_CURRENT) != 0 {
                    m.set_u32(txt, m.u32(master + node::TEXTURE_CURRENT));
                }
                if m.u32(master + node::MATERIALS) != 0 {
                    break master;
                }
            }
        }
        n = m.u32(n + node::FATHER);
    };
    m.set_u32(mats, m.u32(found + node::MATERIALS));
    let count = m.i16(found + node::NB_MATERIALS);
    m.set_i32(nb, if count == 0 { 1 } else { count as i32 });
}

/// Appends the textures of the chain from `first` (linked through +0x16c) whose mapping
/// method (+0x2a, 16 bits) is `method`, under a record made for `owner` at the first one.
fn gather(m: &mut Mem, out: u32, first: u32, method: i16, owner: impl Fn(&Mem) -> u32) {
    let mut r = 0;
    let mut t = first;
    while t != 0 {
        if m.u16(t + lclmap::METHOD) == method as u16 {
            if r == 0 {
                r = m.si_calloc(1, 8);
                let o = owner(m);
                m.set_u32(r + rec::MATERIAL, o);
                let l = list::create(m);
                m.set_u32(r + rec::TEXTURES, l);
                list::insert_e(m, out, r);
            }
            let l = m.u32(r + rec::TEXTURES);
            list::insert_e(m, l, t);
        }
        t = m.u32(t + lclmap::NEXT);
    }
}

/// The node's 2D textures of one mapping method, by owner [soft 0x082b34d8 SI_mdlGetMatAndTxt2D]:
/// a new list; [`find_mat_info`]; the node's chain of textures from the first one found
/// (the owner null), then each material's chain (+0x6ec) in the array's order. An owner gets
/// its record (`SI_calloc(1, 8)`, the owner stored, a new list of textures, the record at
/// the end of the answer) at its first texture of the method; the textures go at the end of
/// its list. An empty answer is given up (`SI_genListDestroy` with
/// [`mat_and_txt2d_destroy`]) and the answer is null.
pub fn mdl_get_mat_and_txt2d(m: &mut Mem, o: &mut dyn Outside, model: u32, method: i16) -> u32 {
    let out = list::create(m);
    // the frame: the answer at +0 (its address goes to SI_genListDestroy), the materials,
    // their count and the first texture
    let frame = m.local(16);
    let (pout, mats, nb, txt) = (frame, frame + 4, frame + 8, frame + 12);
    m.set_u32(pout, out);
    m.set_i32(nb, 0);
    find_mat_info(m, o, model, mats, nb, txt);
    let first = m.u32(txt);
    gather(m, out, first, method, |_| 0);
    let mut i = 0i32;
    while i < m.i32(nb) {
        let at = m.u32(mats).wrapping_add(4 * i as u32);
        let material = m.u32(at);
        let chain = m.u32(material.wrapping_add(mat::TEXTURES));
        gather(m, out, chain, method, |m| m.u32(at));
        i += 1;
    }
    if list::get_head(m, m.u32(pout)) == 0 {
        list::destroy(m, &mut Callbacks, pout, MAT_AND_TXT2D_DESTROY);
        m.set_u32(pout, 0);
    }
    let answer = m.u32(pout);
    m.release(16);
    answer
}

/// An owner's record given up [soft 0x082b3628 SI_matAndTxt2DDestroy], as `SI_genListDestroy`
/// hands it an element (`pdata` is the element: its first word is the record): the record's
/// list of textures destroyed without a destructor (the textures are the nodes' and the
/// materials'), the record freed (`free`) and the element's word cleared (twice, as in
/// `SI_genListElemDestroy`).
pub fn mat_and_txt2d_destroy(m: &mut Mem, pdata: u32) {
    let r = m.u32(pdata);
    list::destroy(m, &mut Callbacks, r.wrapping_add(rec::TEXTURES), 0);
    let r = m.u32(pdata);
    if r != 0 {
        m.free(r);
        m.set_u32(pdata, 0);
    }
    m.set_u32(pdata, 0);
}

/// What the lists call back here: [`mat_and_txt2d_destroy`], the one destructor this module
/// hands them.
pub(crate) struct Callbacks;

impl si_mem::Outside for Callbacks {
    fn call(&mut self, m: &mut Mem, r: Routine, a: &[u32]) -> si_mem::Ret {
        assert_eq!(r.addr, MAT_AND_TXT2D_DESTROY, "a list of the conversions called back {:08x}", r.addr);
        mat_and_txt2d_destroy(m, a[0]);
        si_mem::Ret::NONE
    }
}
