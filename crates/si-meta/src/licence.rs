// SPDX-License-Identifier: MIT
//! `SI_metaLicenseCheckForAttribute`, `edtools.c`: the last step of [`crate::init::si_metaball_init`] (unit lic1,
//! finding 0038, 2026-10-29).
//!
//! What it does: when the program runs as the full interactive product it asks the licence client for the
//! metaball feature and, granted, changes nothing. In the stand-alone renderer (or with a light or base licence, or
//! when the feature is refused) it takes the write bit (0x2) off the access word of the metaball attributes of the
//! attribute dictionary `SI_ATD`: the curve ids 0x154..=0x15b (`ClayRatio`, `ClayInfZone`, `MetaWire`, `MetaRender`,
//! `MetaSurfDisp`, `MetaNbElem`, `MetaNbVert`, `MetaVolume` in `Relations.rsrc`) and 0xb9..=0xbb (`ClayWeight` and
//! the attribute `ClaySuperExp`, whose third curve 0xbc it shares). The one reader of that bit in the rendering
//! path is `SI_modelSetParam (0x080f3d1c)`, which then refuses an expression's value for a metaball's weight or
//! super-exponents (the f-curve animation of the weight, `animVClayUpdate`, does not read the bit).
//!
//! The licence client (FLEXlm: the decryption of the feature name and the checkout) is not ported and not called.
//! Its answer comes in as a number; [`LICENSED_CHECKOUT`] is the one a licensed interactive run gets. The flags
//! decide before the client is asked, so in the stand-alone renderer, where the program sets
//! `SI_standaloneRenderer` first, the client's answer plays no part and the port is the original's.

use si_dict::attdict::{attribute_get_access_mode, attribute_set_access_mode, get_grp_index_from_fcurve};
use si_dict::globals::SI_ATD;
use si_mem::{Mem, Ptr32};

/// `SI_standaloneRenderer` (16 bits): non-zero in the stand-alone renderer.
pub const SI_STANDALONE_RENDERER: u32 = 0x0867_9400;
/// `SI_lightLicense` (16 bits): 1 for a light licence.
pub const SI_LIGHT_LICENSE: u32 = 0x0867_9320;
/// `SI_baseLicense` (16 bits): 1 for a base licence.
pub const SI_BASE_LICENSE: u32 = 0x0878_1464;

/// The licence client's answer to the checkout of the metaball feature in a licensed run: 0, granted. A stand-in
/// (the client is not ported); see the module text.
pub const LICENSED_CHECKOUT: i32 = 0;

/// The write bit of an attribute's access word, the one taken off.
pub const ACCESS_WRITE: u16 = 0x2;

/// The first range of curve ids, both ends included.
pub const META_FCURVES: (i32, i32) = (0x154, 0x15b);
/// The second range of curve ids, both ends included.
pub const CLAY_FCURVES: (i32, i32) = (0xb9, 0xbb);

/// [soft 0x08107394 SI_metaLicenseCheckForAttribute], with the licence client's answer the one of a licensed run
/// ([`LICENSED_CHECKOUT`]).
pub fn meta_license_check_for_attribute(m: &mut Mem) {
    meta_license_check_for_attribute_with(m, &mut |_| LICENSED_CHECKOUT)
}

/// The same with the licence client's answer given by `checkout` (asked at most once, only when none of the three
/// flags decides; 0 is granted).
pub fn meta_license_check_for_attribute_with(m: &mut Mem, checkout: &mut dyn FnMut(&mut Mem) -> i32) {
    if m.i16(SI_STANDALONE_RENDERER) == 0 && m.i16(SI_LIGHT_LICENSE) != 1 && m.i16(SI_BASE_LICENSE) != 1 && checkout(m) == 0 {
        return;
    }
    // the frame: the path (0x14 bytes) and the two access words, the second range's below the first's
    let index = Ptr32::new(m.local(0x14));
    let modes = m.local(4);
    clear_write_bit(m, META_FCURVES, index, Ptr32::new(modes + 2));
    clear_write_bit(m, CLAY_FCURVES, index, Ptr32::new(modes));
    m.release(4);
    m.release(0x14);
}

/// One of the two loops: for every curve id of the range whose attribute the dictionary finds and whose access word
/// it answers, that word without the write bit (only the low 16 bits of each answer are tested).
fn clear_write_bit(m: &mut Mem, (first, last): (i32, i32), index: Ptr32<si_types::dict::AttGrpIndex>, mode: Ptr32<i16>) {
    let mut id = first;
    while id <= last {
        if get_grp_index_from_fcurve(m, SI_ATD, id, index) as u16 != 0
            && attribute_get_access_mode(m, SI_ATD, index, mode) as u16 != 0
        {
            let w = (m[mode] as u16 & !ACCESS_WRITE) as i16;
            attribute_set_access_mode(m, SI_ATD, index, w);
        }
        id += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use si_dict::attdict;
    use si_types::dict::{Attribute, AttributeDict};

    /// A dictionary of one group with a few attributes: (attribute id, its curve ids, its access word).
    fn dictionary(m: &mut Mem, attrs: &[(i32, &[i16], i16)]) {
        let atd: Ptr32<AttributeDict> = SI_ATD;
        attdict::init(m, atd);
        let name = m.si_malloc(8);
        m.put_cstr(name, b"Model");
        attdict::add_attrib_group(m, atd, Ptr32::new(name), 0, 0);
        let group = m[atd].groups;
        let src: Ptr32<Attribute> = Ptr32::new(m.local(0x64));
        for &(id, curves, access) in attrs {
            attdict::init_attribute(m, src);
            m[src].id = id;
            m[src].access = access;
            attdict::allocate_fcurve_array(m, src, curves.len() as i32);
            for (k, &c) in curves.iter().enumerate() {
                let ids = m[src].fcv_ids;
                m[ids.add(k as i32)] = c;
            }
            attdict::add_attribute(m, atd, group, src);
        }
        m.release(0x64);
        attdict::update_indexes(m, atd);
        attdict::sort_attrib_from_fcurve_ids(m, atd);
    }

    fn accesses(m: &Mem) -> Vec<i16> {
        let atd: Ptr32<AttributeDict> = SI_ATD;
        (0..m[atd].nb_attributes).map(|i| m[m[atd].attributes.add(i)].access).collect()
    }

    const SAMPLE: &[(i32, &[i16], i16)] = &[
        (1, &[0x10], 7),               // not a metaball attribute
        (0xb9, &[0xb9], 7),            // ClayWeight
        (0xba, &[0xba, 0xbb, 0xbc], 7), // ClaySuperExp
        (0x154, &[0x154], 7),          // ClayRatio
        (0x158, &[0x158], 3),          // MetaSurfDisp
        (0x159, &[0x159], 1),          // MetaNbElem, read only
        (0x15c, &[0x15c], 7),          // just past the range
        (0xbc, &[0xbd], 7),            // the id, not the curve: untouched
    ];

    #[test]
    fn stand_alone_takes_the_write_bit_off_the_metaball_attributes() {
        let mut m = Mem::new();
        dictionary(&mut m, SAMPLE);
        m.set_i16(SI_STANDALONE_RENDERER, 1);
        let mut asked = 0;
        meta_license_check_for_attribute_with(&mut m, &mut |_| {
            asked += 1;
            0
        });
        assert_eq!(asked, 0, "the flag decides before the client is asked");
        assert_eq!(accesses(&m), vec![7, 5, 5, 5, 1, 1, 7, 7]);
    }

    #[test]
    fn licensed_interactive_run_changes_nothing() {
        let mut m = Mem::new();
        dictionary(&mut m, SAMPLE);
        meta_license_check_for_attribute(&mut m);
        assert_eq!(accesses(&m), vec![7, 7, 7, 7, 3, 1, 7, 7]);
    }

    #[test]
    fn refused_checkout_or_light_licence_takes_it_off() {
        for (light, base, answer) in [(0, 0, -1), (1, 0, 0), (0, 1, 0)] {
            let mut m = Mem::new();
            dictionary(&mut m, SAMPLE);
            m.set_i16(SI_LIGHT_LICENSE, light);
            m.set_i16(SI_BASE_LICENSE, base);
            meta_license_check_for_attribute_with(&mut m, &mut |_| answer);
            assert_eq!(accesses(&m), vec![7, 5, 5, 5, 1, 1, 7, 7]);
        }
    }

    #[test]
    fn no_dictionary_no_change() {
        let mut m = Mem::new();
        attdict::init(&mut m, SI_ATD);
        let sp = m.sp();
        m.set_i16(SI_STANDALONE_RENDERER, 1);
        meta_license_check_for_attribute(&mut m);
        assert_eq!(m.sp(), sp);
    }
}
