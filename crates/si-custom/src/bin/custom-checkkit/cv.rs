// SPDX-License-Identifier: MIT
//! The ports of unit customrt1 (custom value lists, data strings, fcurve names and the rest of the
//! run-time files), kept apart from `main.rs` so the two lists merge cleanly.

use si_checkkit::Port;
use si_custom::custom_enc::*;
use si_custom::custom_val::*;

/// `base` followed by this unit's ports.
pub fn with(base: &[(&'static str, Port)]) -> Vec<(&'static str, Port)> {
    let mut v: Vec<(&'static str, Port)> = base.to_vec();
    v.extend_from_slice(&[
        ("si_customValueAllocate", |m, _o, _a| si_custom_value_allocate(m).into()),
        ("si_customValueInitializeCustVal", |m, o, a| (si_custom_value_initialize_cust_val(m, o, a.v("dst"), a.v("src")) as u32).into()),
        ("si_customValueCopy", |m, o, a| si_custom_value_copy(m, o, a.v("src")).into()),
        ("si_customValueListCopy", |m, o, a| si_custom_value_list_copy(m, o, a.v("list")).into()),
        ("si_customValueDispose", |m, _o, a| (si_custom_value_dispose(m, a.v("v")) as u32).into()),
        ("si_customValueListDispose", |m, _o, a| { si_custom_value_list_dispose(m, a.v("list")); ().into() }),
        ("cusvalIsKeyedValue", |m, _o, a| (cusval_is_keyed_value(m, a.v("v")) as u32).into()),
        ("cusvalGetNextKeyedValue", |m, _o, a| cusval_get_next_keyed_value(m, a.v("v"), a.v("scroll")).into()),
        ("cusvalGetNextKeyableValue", |m, _o, a| cusval_get_next_keyable_value(m, a.v("v"), a.v("scroll")).into()),
        ("findCustomValue", |m, _o, a| find_custom_value(m, a.v("dlg"), a.v("id") as i32).into()),
        ("cusAppendString", |m, _o, a| cus_append_string(m, a.v("s"), a.v("lenp"), a.v("add")).into()),
        ("cusPrintCustomFcurve", |m, o, a| cus_print_custom_fcurve(m, o, a.v("s"), a.v("lenp"), a.v("v"), a.v("count") as i32, a.v("times")).into()),
        ("cusPrintCustomValue", |m, o, a| cus_print_custom_value(m, o, a.v("dlg"), a.v("s"), a.v("lenp"), a.v("v"), a.f("frame"), a.v("count") as i32, a.v("times")).into()),
        ("cusFcvkeyAppendString", |m, o, a| cus_fcvkey_append_string(m, o, a.v("s"), a.v("lenp"), a.v("key")).into()),
        ("cusFcurveAppendString", |m, o, a| cus_fcurve_append_string(m, o, a.v("s"), a.v("lenp"), a.v("name"), a.v("k") as i32, a.v("fc")).into()),
        ("cusKeytimesUpdate", |m, _o, a| { cus_keytimes_update(m, a.v("v"), a.v("countp"), a.v("timesp")); ().into() }),
        ("SI_cusEncodeDataString", |m, o, a| si_cus_encode_data_string(m, o, a.v("dlg"), a.f("frame"), a.v("extra")).into()),
        ("SI_cusDialogWriteData", |m, o, a| { si_cus_dialog_write_data(m, o, a.v("dlg"), a.f("frame"), a.v("fp")); ().into() }),
        ("SI_udfGenerateCcomp", |m, o, a| (si_udf_generate_ccomp(m, o, a.v("a"), a.v("b")) as u32).into()),
        ("SI_udfGenerateCflags", |m, o, a| (si_udf_generate_cflags(m, o, a.v("a"), a.v("b")) as u32).into()),
    ]);
    v
}

/// `SI_fcurveValue(fc, time, 0)` as the generator's stand-in answers it (a significand made from the curve's
/// first word and the time, so every bit of the 80-bit register is used).
pub fn fcurve_value(m: &mut si_mem::Mem, a: &[u32]) -> si_mem::Ret {
    let h = m.u32(a[0]) ^ a[1].wrapping_mul(0x9E37_79B1);
    let mant: u64 = (1u64 << 63) | ((h as u64).wrapping_mul(0x2545_F491_4F6C_DD1D) & ((1u64 << 63) - 1));
    let se: u16 = ((((h >> 5) & 1) as u16) << 15) | (16383 + (h >> 27) as u16 - 8);
    let mut r = [0u8; 10];
    r[..8].copy_from_slice(&mant.to_le_bytes());
    r[8..].copy_from_slice(&se.to_le_bytes());
    si_mem::Ret { eax: 0, edx: 0, st0: Some(r) }
}
