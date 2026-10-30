// SPDX-License-Identifier: MIT
//! `mi2_tools.c`: instance and group lines of the mental ray 2 scene text (package 63, finding 0042). Printed lines are
//! `SI_mrpipeFPRINTF` calls with the format's address and the argument words, as in `mi2_model`.

use crate::mi_mr_main::MRPIPE_FPRINTF;
use si_mem::{Mem, Outside};

const INCREMENTAL: u32 = 0x086e_f9bc;

fn fp(m: &mut Mem, o: &mut dyn Outside, args: &[u32]) {
    o.call(m, MRPIPE_FPRINTF, args);
}

fn dw(m: &Mem, a: u32) -> [u32; 2] {
    let b = (m.f32(a) as f64).to_bits();
    [b as u32, (b >> 32) as u32]
}

/// [soft 0x08597ff0 printTransformMat]: the `transform` block, four rows of three floats (the matrix is 4x4, row
/// 3 is the translation); the fourth element of a row is not printed, the last column is the literal 0.0 / 1.0.
pub fn print_transform_mat(m: &mut Mem, o: &mut dyn Outside, mat: u32) {
    fp(m, o, &[0x086f_5220]);
    for (row, fmt) in [(0u32, 0x086f_522d), (4, 0x086f_523f), (8, 0x086f_5251), (12, 0x086f_5263)] {
        let mut a = vec![fmt];
        for k in 0..3 {
            a.extend(dw(m, mat + 4 * (row + k)));
        }
        fp(m, o, &a);
    }
}

/// [soft 0x085980b8 printInstance]: `instance "<name>" "<name>_master"` (`incremental` first when the file is incremental),
/// its `hide on|off` and transform, `end instance`. The `hide` call's third word is the stale name word of the line before.
pub fn print_instance(m: &mut Mem, o: &mut dyn Outside, name: u32, hide: u32, mat: u32) {
    let fmt = if m.u32(INCREMENTAL) != 0 { 0x086f_5280 } else { 0x086f_52a7 };
    fp(m, o, &[fmt, name, name]);
    let on = if hide as u8 != 0 { 0x086f_52c2 } else { 0x086f_52c5 };
    fp(m, o, &[0x086f_52c9, on, name]);
    print_transform_mat(m, o, mat);
    fp(m, o, &[0x086f_52d3]);
}

/// [soft 0x08598134 printDefaultInstance]: an instance of the name with the identity transform (none written).
pub fn print_default_instance(m: &mut Mem, o: &mut dyn Outside, name: u32) {
    let fmt = if m.u32(INCREMENTAL) != 0 { 0x086f_5300 } else { 0x086f_5327 };
    fp(m, o, &[fmt, name, name]);
    fp(m, o, &[0x086f_5342]);
}

/// [soft 0x08598178 print_group]: the group `group_<name>` of the `count` named instances (and `<name>` itself,
/// shadow too when asked), written unless the file is incremental, and a made copy of the group's name (0 without a
/// name, or with a count and no names). The name buffer is 1024 bytes in the original (longer names overrun it).
pub fn print_group(m: &mut Mem, o: &mut dyn Outside, name: u32, names: u32, count: u32, shadow: u32) -> u32 {
    let count = count as i32;
    if name == 0 || (count > 0 && names == 0) {
        return 0;
    }
    let mut g = b"group_".to_vec();
    g.extend(m.cstr(name));
    let buf = m.local(g.len() as u32 + 1);
    m.put_cstr(buf, &g);
    if m.u32(INCREMENTAL) == 0 {
        fp(m, o, &[0x086f_535a, buf]);
        fp(m, o, &[0x086f_5372, name]);
        if shadow as u16 != 0 {
            fp(m, o, &[0x086f_5378, name]);
        }
        for i in 0..count.max(0) as u32 {
            let n = m.u32(names + 4 * i);
            fp(m, o, &[0x086f_5385, n]);
        }
        fp(m, o, &[0x086f_538c]);
        print_default_instance(m, o, buf);
    }
    let p = m.malloc(g.len() as u32 + 1);
    m.put_cstr(p, &g);
    m.release(g.len() as u32 + 1);
    p
}
