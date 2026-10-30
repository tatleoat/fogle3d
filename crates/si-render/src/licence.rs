// SPDX-License-Identifier: MIT
//! The renderer's licence question (`render.c`): `getRenderLicense`, which both frame loops
//! (`SI_sample`, `SI_supersample`) ask first, and the word `SI_rendererLicenseFailed` that
//! it leaves for `SI_image` (finding 0038, lic2).
//!
//! The routine around the question is ported; the licence client it asks (FLEXlm: the
//! heartbeat, the decryption of the feature's name and version, the checkout, the vendor
//! version string it saves and puts back) is not ported and is never called. Where the
//! original takes the client's checkout answer, the port takes [`LICENSED_CHECKOUT`], the
//! answer of a granted checkout, or the answer its caller gives
//! ([`get_render_license_with`]). The client's own memory is left alone.

use crate::data::text;
use crate::img::{f, g, Outside};
use si_mem::Mem;

/// `SI_rendererLicenseFailed` (0x0869c020, 16 bits, 0 in the binary's data):
/// `getRenderLicense` clears it, and sets it to 1 when no checkout succeeded; `SI_image`
/// clears it before the frame, leaves through `longjmp` after the frame when it is set, and
/// shows a dialogue on the jump path when it is set (not in the stand-alone renderer).
pub const SI_RENDERER_LICENSE_FAILED: u32 = 0x0869_c020;

/// `checkForExit` (0x0825b800, `render.c`), called by address on the image (its port is
/// [`crate::render::check_for_exit`]).
pub const CHECK_FOR_EXIT: u32 = 0x0825_b800;

/// The licence client's answer to the checkout of the renderer's feature that the port
/// stands in: 0, a granted checkout. A stand-in, not observed: the client is never called
/// and its answer in the original's runs is not recorded (licence code is not traced).
/// That the original's batch renders got 0 is what their pictures show: `SI_sample` and
/// `SI_supersample` render nothing unless the routine answers 1, which needs a checkout
/// answer of 0.
pub const LICENSED_CHECKOUT: i32 = 0;

/// Asks for the renderer's licence [soft 0x0825b868 getRenderLicense]:
/// `getRenderLicense()`, with the client's checkout answer [`LICENSED_CHECKOUT`]. See
/// [`get_render_license_with`].
pub fn get_render_license(m: &mut Mem, env: &mut dyn Outside) -> u32 {
    get_render_license_with(m, env, &mut || LICENSED_CHECKOUT)
}

/// `getRenderLicense()` with the client's checkout answer from `checkout`, asked once a try.
///
/// At most 60 tries (the counter starts at 0x3c and goes down by one for each refused
/// checkout); before each the client is given its heartbeat and its vendor version string
/// is saved and replaced by the decrypted one (client: not ported). On a granted checkout
/// (answer 0) the answer becomes 1, and in the stand-alone renderer (`SI_standaloneRenderer`
/// 0x08679400, 16 bits, non-zero) **after at least one refusal** (counter at most 0x3b) two
/// warnings go out: `SI_warning("License connection reestablished.", " ", 2)` and
/// `SI_warning("Resuming rendering...", " ", 2)`. On a refusal: in the stand-alone renderer
/// `SI_warning("Lost connection to license server.", " ", 2)` and `sleep(60)`; otherwise up
/// to twelve rounds of `SI_cusCheckRenderAbort()`, then `checkForExit()` (a non-zero low
/// word ends the rounds), then `sleep(5)`. The tries go on while the counter is non-zero and
/// nothing was granted.
///
/// Then `SI_rendererLicenseFailed` = 0, and when nothing was granted it is 1 and, in the
/// stand-alone renderer, `SI_warning("Failed to reestablish connection to license server.",
/// " ", 2)` and `SI_warning("Rendering aborted.", " ", 2)` go out. The vendor version string
/// is put back (client). Answers 1 when a checkout was granted, else 0.
///
/// With the granted answer, the first try succeeds with the counter at 0x3c: no warning, no
/// other call, the word cleared, the answer 1. Class: copy.
pub fn get_render_license_with(m: &mut Mem, env: &mut dyn Outside, checkout: &mut dyn FnMut() -> i32) -> u32 {
    let mut granted = 0u32;
    let mut tries: i32 = 0x3c;
    loop {
        // (client: lc_heartbeat, the vendor version saved and replaced, the feature's name
        // decrypted; not ported)
        if checkout() == 0 {
            granted = 1;
            if m.i16(g::SI_STANDALONE_RENDERER) != 0 && tries <= 0x3b {
                env.call(m, f::SI_WARNING, &[text::LICENSE_REESTABLISHED, text::BLANK_REESTABLISHED, 2]);
                env.call(m, f::SI_WARNING, &[text::RESUMING_RENDERING, text::BLANK_RESUMING, 2]);
            }
        } else {
            tries -= 1;
            if m.i16(g::SI_STANDALONE_RENDERER) != 0 {
                env.call(m, f::SI_WARNING, &[text::LOST_CONNECTION, text::BLANK_LOST, 2]);
                env.call(m, f::SLEEP, &[0x3c]);
            } else {
                let mut rounds: i32 = 0xc;
                loop {
                    env.call(m, f::SI_CUS_CHECK_RENDER_ABORT, &[]);
                    if env.call(m, CHECK_FOR_EXIT, &[]) as u16 != 0 {
                        break;
                    }
                    env.call(m, f::SLEEP, &[5]);
                    rounds -= 1;
                    if rounds <= 0 {
                        break;
                    }
                }
            }
        }
        if tries == 0 || granted != 0 {
            break;
        }
    }
    m.set_u16(SI_RENDERER_LICENSE_FAILED, 0);
    if granted == 0 {
        m.set_u16(SI_RENDERER_LICENSE_FAILED, 1);
        if m.i16(g::SI_STANDALONE_RENDERER) != 0 {
            env.call(m, f::SI_WARNING, &[text::LICENSE_FAILED, text::BLANK_FAILED, 2]);
            env.call(m, f::SI_WARNING, &[text::RENDERING_ABORTED, text::BLANK_ABORTED, 2]);
        }
    }
    // (client: the vendor version string put back; not ported)
    granted
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every call; `checkForExit` answers from `exit`.
    #[derive(Default)]
    struct Calls {
        calls: Vec<(u32, Vec<u32>)>,
        exit: u32,
    }

    impl Outside for Calls {
        fn call(&mut self, _m: &mut Mem, f: u32, args: &[u32]) -> u32 {
            self.calls.push((f, args.to_vec()));
            if f == CHECK_FOR_EXIT {
                self.exit
            } else {
                0
            }
        }
    }

    fn image(standalone: i16) -> Mem {
        let mut m = Mem::new();
        crate::data::place_texts(&mut m);
        m.set_i16(g::SI_STANDALONE_RENDERER, standalone);
        m.set_u16(SI_RENDERER_LICENSE_FAILED, 0x5a5a);
        m
    }

    fn texts(m: &Mem, c: &Calls) -> Vec<String> {
        c.calls
            .iter()
            .map(|(f, a)| match *f {
                f::SI_WARNING => format!("warning {}|{}|{}", String::from_utf8_lossy(&m.cstr(a[0])), String::from_utf8_lossy(&m.cstr(a[1])), a[2]),
                f::SLEEP => format!("sleep {}", a[0]),
                f::SI_CUS_CHECK_RENDER_ABORT => "abort?".into(),
                CHECK_FOR_EXIT => "exit?".into(),
                other => format!("{other:#x}"),
            })
            .collect()
    }

    #[test]
    fn a_granted_licence_is_asked_once_and_says_nothing() {
        si_rt::skip_without_install!();
        for standalone in [0, 1] {
            let mut m = image(standalone);
            let mut c = Calls::default();
            assert_eq!(get_render_license(&mut m, &mut c), 1);
            assert!(c.calls.is_empty());
            assert_eq!(m.u16(SI_RENDERER_LICENSE_FAILED), 0);
        }
    }

    #[test]
    fn the_stand_alone_renderer_waits_a_minute_a_refusal_and_says_when_it_is_back() {
        si_rt::skip_without_install!();
        let mut m = image(1);
        let mut c = Calls::default();
        let mut answers = [7, 7, 0].into_iter();
        assert_eq!(get_render_license_with(&mut m, &mut c, &mut || answers.next().unwrap()), 1);
        let lost = "warning Lost connection to license server.| |2";
        assert_eq!(
            texts(&m, &c),
            [lost, "sleep 60", lost, "sleep 60", "warning License connection reestablished.| |2", "warning Resuming rendering...| |2"]
        );
        assert_eq!(m.u16(SI_RENDERER_LICENSE_FAILED), 0);
    }

    #[test]
    fn sixty_refusals_fail_the_render() {
        si_rt::skip_without_install!();
        let mut m = image(1);
        let mut c = Calls::default();
        let mut asked = 0;
        assert_eq!(
            get_render_license_with(&mut m, &mut c, &mut || {
                asked += 1;
                1
            }),
            0
        );
        assert_eq!(asked, 60);
        let t = texts(&m, &c);
        assert_eq!(t.len(), 122);
        assert_eq!(t[120..], ["warning Failed to reestablish connection to license server.| |2", "warning Rendering aborted.| |2"]);
        assert_eq!(m.u16(SI_RENDERER_LICENSE_FAILED), 1);
    }

    #[test]
    fn the_interactive_program_polls_twelve_times_or_until_the_user_stops() {
        si_rt::skip_without_install!();
        // twelve rounds of five seconds, then the second try is granted (no warning: not
        // the stand-alone renderer)
        let mut m = image(0);
        let mut c = Calls::default();
        let mut answers = [3, 0].into_iter();
        assert_eq!(get_render_license_with(&mut m, &mut c, &mut || answers.next().unwrap()), 1);
        let t = texts(&m, &c);
        assert_eq!(t.len(), 36);
        assert!(t.chunks(3).all(|r| r == ["abort?", "exit?", "sleep 5"]));
        // a stop from the user ends the rounds at once, and the routine asks again
        let mut m = image(0);
        let mut c = Calls { exit: 0x1_0000, ..Calls::default() };
        let mut answers = [3, 0].into_iter();
        get_render_license_with(&mut m, &mut c, &mut || answers.next().unwrap());
        assert_eq!(texts(&m, &c).len(), 36, "only the low word of checkForExit's answer counts");
        let mut m = image(0);
        let mut c = Calls { exit: 1, ..Calls::default() };
        let mut answers = [3, 3, 0].into_iter();
        assert_eq!(get_render_license_with(&mut m, &mut c, &mut || answers.next().unwrap()), 1);
        assert_eq!(texts(&m, &c), ["abort?", "exit?", "abort?", "exit?"]);
        // all sixty refused: the word set, no warning
        let mut m = image(0);
        let mut c = Calls { exit: 1, ..Calls::default() };
        assert_eq!(get_render_license_with(&mut m, &mut c, &mut || -1), 0);
        assert_eq!(c.calls.len(), 120);
        assert_eq!(m.u16(SI_RENDERER_LICENSE_FAILED), 1);
    }
}
