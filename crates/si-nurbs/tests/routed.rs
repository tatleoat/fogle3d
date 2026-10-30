// SPDX-License-Identifier: MIT

//! Dispatch tests of `si_nurbs::routed` (an integration test: the module is not in the crate's own test build, which
//! cannot include the `inter` family).
use si_mem::{Mem, Outside, Ret, Routine, Trace};
use si_nurbs::routed::{Routed, ROUTED};


    /// With the switch off every call goes to the wrapped world and no port answers.
    #[test]
    fn switch_off_leaves_calls_to_the_wrapped_world() {
        let mut m = Mem::new();
        let mut inner = Trace::with(|_m, _r, _a| Ret::eax(7));
        let mut routed = Routed::new(&mut inner);
        routed.enabled = false;
        let r = routed.call(&mut m, Routine::new("SI_nurbsSrfSwapUV", 0x084d_c5f4), &[1]);
        assert_eq!(r.eax, 7);
        assert!(routed.counts.is_empty());
        assert!(routed.route(&mut m, 0x0123_4567, &[]).is_none());
    }

    /// The calls that a port answers on a zero surface/curve (null or empty records, which the ports leave alone) never
    /// reach the wrapped world, and the counts say each was answered once.
    #[test]
    fn null_record_calls_reach_their_port() {
        let mut m = Mem::new();
        let mut inner = Trace::with(|_m, r, _a| panic!("{} ({:08x}) went to the wrapped world", r.name, r.addr));
        let mut routed = Routed::new(&mut inner);
        routed.enabled = true;
        for (name, addr) in [
            ("SI_nurbsSrfSwapUV", 0x084d_c5f4u32),
            ("si_nurbsInterComputeRayT", 0x0850_cc48),
            ("si_nurbsCurveInterPointOnLine", 0x0850_9a44),
            ("si_nurbsCurveInterUpdateBbox", 0x0850_8250),
        ] {
            let s = m.si_malloc(0x400);
            let words = [s, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            routed.call(&mut m, Routine::new(name, addr), &words);
            assert_eq!(routed.counts.get(name), Some(&1), "{name}");
        }
        for a in [0x084d_4130u32, 0x084d_caf8, 0x0850_2904, 0x084c_b560, 0x084c_e028, 0x0850_dc64] {
            assert!(ROUTED.contains(&a));
        }
    }

