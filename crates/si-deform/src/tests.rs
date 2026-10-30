// SPDX-License-Identifier: MIT
//! Unit tests on made-up data (no original file, no vector): that the pieces hold together
//! on the image. The comparison with the original is `deform-emucheck`'s.

use crate::outside::NoOutside;
use crate::{fitting, globals, klusters, lattice, Kluster, Model};
use si_mem::{Mem, Ptr32};

fn image() -> Mem {
    let mut m = Mem::new();
    globals::init(&mut m);
    m
}

fn node(m: &mut Mem) -> Ptr32<Model> {
    let at = m.place(0x430, 4);
    m.set_fill_bytes(at, 0, 0x430);
    Ptr32::new(at)
}

/// A lattice at rest moves nothing, inside the box and (continued in a straight line)
/// outside it. The numbers are chosen so that every step is exact.
#[test]
fn a_lattice_at_rest_is_the_identity() {
    let mut m = image();
    let lat = lattice::lattice_initialize(&mut m, 3, 3, 3, [0, 0, 0], [0.0, 0.0, 0.0], [2.0, 4.0, 8.0]);
    assert_eq!(m[lat].step, [1.0, 2.0, 4.0]);
    assert_eq!(m.heap_blocks().len(), 3);
    let (inp, out) = (m.place(12, 4), m.place(12, 4));
    for p in [[0.5f32, 1.25, 1.75], [2.0, 4.0, 8.0], [2.5, -0.5, 9.0], [-1.0, 5.0, 3.0]] {
        crate::wr3(&mut m, inp, p);
        lattice::lattice_value(&mut m, lat, 0, 0, Ptr32::new(inp), Ptr32::new(out), 2);
        assert_eq!(crate::rd3(&m, out), p);
    }
    // one corner moved: the point in the middle of the first cell follows by an eighth
    let corner = m[lat].keys.addr();
    m.set_f32(corner, 8.0);
    crate::wr3(&mut m, inp, [0.5, 1.0, 2.0]);
    lattice::lattice_value(&mut m, lat, 0, 0, Ptr32::new(inp), Ptr32::new(out), 2);
    assert_eq!(crate::rd3(&m, out), [1.5, 1.0, 2.0]);
    lattice::lattice_reset(&mut m, lat);
    assert_eq!(m.f32(corner), 0.0);
    let slot = m.place(4, 4);
    m.set_u32(slot, lat.addr());
    lattice::lattice_dispose(&mut m, Ptr32::new(slot));
    assert_eq!((m.u32(slot), m.freed().len()), (0, 3));
    assert!(m.fault().is_none());
}

/// Clusters come and go in a node's chain; the states follow the active one.
#[test]
fn clusters_in_a_chain() {
    let mut m = image();
    let mut o = NoOutside::default();
    let model = node(&mut m);
    let names = [m.place_cstr(b"arm"), m.place_cstr(b"leg")];
    let a = klusters::kluster_add_to_list(&mut m, &mut o, model, names[0], 1);
    let b = klusters::kluster_add_to_list(&mut m, &mut o, model, names[1], 0);
    assert_eq!(o.calls.len(), 1);
    assert_eq!(klusters::cluster_count(&m, model), 2);
    assert_eq!(klusters::kluster_get_by_name(&m, model, names[1]), b);
    assert!(klusters::kluster_get_active(&m, model).is_null());
    klusters::kluster_activate(&mut m, &mut o, model, b, 0, 0);
    assert_eq!(klusters::kluster_get_active(&m, model), b);
    klusters::kluster_activate(&mut m, &mut o, model, a, 0, 0);
    assert_eq!((m[a].state, m[b].state), (0, 1));
    let fresh = klusters::kluster_get_distinct_name(&mut m, model);
    assert_eq!(m.cstr(fresh), b"cls0");
    klusters::kluster_dispose(&mut m, &mut o, model, a, 0);
    let head: Ptr32<Kluster> = m[model].klusters.cast();
    assert_eq!((head, klusters::cluster_count(&m, model)), (b, 1));
    assert!(m.fault().is_none());
}

/// A new deformation record has identity matrices, and stays so through an update.
#[test]
fn a_new_fitting_is_the_identity() {
    let mut m = image();
    let identity = m[globals::SI_IDENTITE];
    let f = fitting::fitting_allocate(&mut m);
    fitting::fitting_update_matrix(&mut m, f);
    assert_eq!((m[f].matrix, m[f].normal_matrix), (identity, identity));
    let p = fitting::pfitting_allocate(&mut m);
    fitting::pfitting_update_matrix(&mut m, p);
    assert_eq!((m[p].matrix, m[p].normal_matrix), (identity, identity));
    assert!(m.fault().is_none());
}

/// `SI_postAnimTouch` readjusts the node alone when the `short` is 0, and walks the subtree
/// from a null start for a branch.
#[test]
fn the_post_animation_touch() {
    use crate::deformation::post_anim_touch;
    use crate::outside::{SI_MODEL_NEXT_SON, SI_MODEL_READJUST_DEFINITION};
    let mut m = image();
    let model = node(&mut m);
    let mut o = NoOutside::default();
    post_anim_touch(&mut m, &mut o, model, 0);
    assert_eq!(o.calls, [(&SI_MODEL_READJUST_DEFINITION, vec![model.addr()])]);
    let mut o = NoOutside::default();
    post_anim_touch(&mut m, &mut o, model, 1);
    assert_eq!(o.calls, [(&SI_MODEL_READJUST_DEFINITION, vec![model.addr()]), (&SI_MODEL_NEXT_SON, vec![model.addr(), 0])]);
}
