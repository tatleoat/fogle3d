// SPDX-License-Identifier: MIT
//! The driver on made-up cases.

use crate::*;
use si_mem::{Mem, Outside, Routine};
use si_x87::vectors::Case;

/// Reads the word `p` points to, adds one and stores it back; returns the old word.
fn bump(m: &mut Mem, _o: &mut dyn Outside, a: &Args) -> Ret {
    let p = a.v("p");
    let old = m.u32(p);
    m.set_u32(p, old + 1);
    old.into()
}

/// Calls `foo(x)` and the callback `cb` with the answer; returns what `cb` answered.
fn caller(m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Ret {
    let r = o.call(m, Routine::new("foo", 0x6000_0001), &[a.v("x")]).eax;
    o.call(m, Routine::new("", a.v("cb")), &[r]).eax.into()
}

fn kit() -> Kit {
    Kit::new(&[("bump", bump), ("caller", caller), ("none", |_m, _o, _a| Ret::None), ("shape", |m, _o, a| {
        // writes outside the case's buffers
        let p = a.v("p");
        m.set_u32(p + 0x40, 1);
        Ret::None
    })])
}

fn case(line: &str) -> Case {
    Case::parse(line).unwrap().unwrap()
}

fn run_one(kit: &Kit, cfg: &Config, line: &str) -> Outcome {
    check_caught(kit, cfg, &case(line), 0)
}

#[test]
fn identical_when_every_part_agrees() {
    let o = run_one(&kit(), &Config::default(), "bump p_p=&buf buf=00000005 => eax=00000005 buf=00000006");
    assert_eq!(o, Outcome::Same);
}

#[test]
fn a_wrong_return_or_buffer_differs_and_names_it() {
    let k = kit();
    let o = run_one(&k, &Config::default(), "bump p_p=&buf buf=00000005 => eax=00000004 buf=00000006");
    assert!(matches!(&o, Outcome::Different(d) if d.starts_with("eax: original 4, port 5")), "{o:?}");
    let o = run_one(&k, &Config::default(), "bump p_p=&buf buf=00000005 => eax=00000005 buf=00000007");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("buffer `buf`") && d.contains("word 0")), "{o:?}");
}

#[test]
fn an_untouched_buffer_must_stay_untouched() {
    let o = run_one(&kit(), &Config::default(), "bump p_p=&buf buf=00000005 => eax=00000005");
    assert!(matches!(o, Outcome::Different(_)), "{o:?}");
}

#[test]
fn the_memory_switch_makes_an_identical_case_differ() {
    let cfg = Config { mutate: Mutate::Memory, ..Config::default() };
    let o = run_one(&kit(), &cfg, "bump p_p=&buf buf=00000005 => eax=00000005 buf=00000006");
    assert!(matches!(o, Outcome::Different(_)), "{o:?}");
}

#[test]
fn recorded_calls_are_answered_held_and_the_callback_is_found_by_its_argument() {
    let line = "caller v_x=00000003 v_cb=60001ad0 => eax=0000000b @foo(00000003)=00000009 @cb(00000009)=0000000b";
    let k = kit();
    assert_eq!(run_one(&k, &Config::default(), line), Outcome::Same);
    // a wrong argument in the original's record
    let o = run_one(&k, &Config::default(), &line.replace("@foo(00000003)", "@foo(00000004)"));
    assert!(matches!(&o, Outcome::Different(d) if d.contains("call 0 foo: argument 0")), "{o:?}");
    // the calls switch flips the first argument of every call
    let cfg = Config { mutate: Mutate::Calls, ..Config::default() };
    assert!(matches!(run_one(&k, &cfg, line), Outcome::Different(_)));
    // a call the original did not make
    let o = run_one(&k, &Config::default(), "caller v_x=00000003 v_cb=60001ad0 => eax=00000000");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("no further call")), "{o:?}");
}

#[test]
fn a_lenient_run_answers_unrecorded_callees_zero_and_counts_them() {
    let cfg = Config { lenient: true, ..Config::default() };
    let o = run_one(&kit(), &cfg, "caller v_x=00000003 v_cb=60001ad0 => eax=00000000");
    assert_eq!(o, Outcome::Same);
    let counted = check::UNPORTED.with(|u| u.borrow().get("foo").copied());
    assert!(counted.is_some());
}

#[test]
fn a_stand_in_and_a_route_answer_instead_of_the_record() {
    let k = kit().stand_in("foo", |_m, args| si_mem::Ret::eax(args[0] * 2)).stand_in("cb", |_m, args| si_mem::Ret::eax(args[0] + 1));
    let o = run_one(&k, &Config::default(), "caller v_x=00000003 v_cb=60001ad0 => eax=00000007");
    assert_eq!(o, Outcome::Same);
    let k = kit().route(|_m, r, args| (r.name == "foo").then(|| si_mem::Ret::eax(args[0] + 100)));
    let o = run_one(&k, &Config::default(), "caller v_x=00000003 v_cb=60001ad0 => eax=00000068 @cb(00000067)=00000068");
    assert_eq!(o, Outcome::Same);
}

#[test]
fn the_original_stopped_or_no_port_are_not_compared() {
    let k = kit();
    assert!(matches!(run_one(&k, &Config::default(), "bump p_p=&buf buf=00000005 => eax=00000005 ! STOP:budget"), Outcome::Skip(_)));
    assert!(matches!(run_one(&k, &Config::default(), "nobody p_p=&buf buf=00000005 => eax=00000005"), Outcome::Skip("no port in the table")));
}

#[test]
fn a_write_outside_the_buffers_differs_and_says_where() {
    let o = run_one(&kit(), &Config::default(), "shape p_p=&buf buf=00000005,00000000 =>");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("outside every buffer") && d.contains("`buf`")), "{o:?}");
}

#[test]
fn the_tally_puts_cases_in_the_columns_of_the_older_drivers() {
    let mut t = Tally::default();
    for line in ["none => ", "none => ! UNINIT:eax", "none => ! NA:strtod"] {
        t.count(&case(line), Outcome::Same, 1, line, false);
    }
    assert_eq!((t.identical, t.defined_part, t.given_lib), (1, 1, 1));
}

// ---- out-local arguments, stack-address normalising, scratch globals

/// Hands a local to `fetch(&local, 7)` and returns what was stored there, plus one.
fn out_local(m: &mut Mem, o: &mut dyn Outside, _a: &Args) -> Ret {
    let l = m.local(4);
    o.call(m, Routine::new("fetch", 0x6000_0002), &[l, 7]);
    (m.u32(l) + 1).into()
}

/// The same with a pointer that is no stack address.
fn out_not_local(m: &mut Mem, o: &mut dyn Outside, _a: &Args) -> Ret {
    let l = m.local(4);
    o.call(m, Routine::new("fetch", 0x6000_0002), &[0x0900_0000, 7]);
    m.u32(l).into()
}

/// Hands a stack local to a hook that records a frame address (`sink(&local, 1)`); the record holds the
/// original's address.
fn sink_local(m: &mut Mem, o: &mut dyn Outside, _a: &Args) -> Ret {
    let l = m.local(8);
    o.call(m, Routine::new("sink", 0x6000_0003), &[l, 1]);
    let _ = l;
    Ret::None
}

fn bump_global(m: &mut Mem, _o: &mut dyn Outside, _a: &Args) -> Ret {
    let v = m.u32(0x086d_6960);
    m.set_u32(0x086d_6960, v + 1);
    v.into()
}

fn kit2() -> Kit {
    Kit::new(&[("out_local", out_local), ("out_not_local", out_not_local), ("sink_local", sink_local), ("bump_global", bump_global)])
}

#[test]
fn a_local_marked_57ac57ac_matches_any_stack_address_and_receives_the_recorded_words() {
    let line = "out_local => eax=0000002b @fetch(57ac57ac,00000007)=00000000 @fetch.out(0000002a)=00000000";
    assert_eq!(run_one(&kit2(), &Config::default(), line), Outcome::Same);
    // the words the callee stored are what the port reads: another word, another result
    let o = run_one(&kit2(), &Config::default(), &line.replace("eax=0000002b", "eax=0000002c"));
    assert!(matches!(&o, Outcome::Different(d) if d.starts_with("eax:")), "{o:?}");
    // a pointer that is no stack address is a difference, and says so
    let o = run_one(&kit2(), &Config::default(), "out_not_local => eax=00000000 @fetch(57ac57ac,00000007)=00000000 @fetch.out(0000002a)=00000000");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("argument 0") && d.contains("stack address")), "{o:?}");
    // the other arguments are held as ever
    let o = run_one(&kit2(), &Config::default(), "out_local => eax=0000002b @fetch(57ac57ac,00000008)=00000000 @fetch.out(0000002a)=00000000");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("argument 1")), "{o:?}");
    // the mutation switches still bite: the calls switch flips the first argument
    let cfg = Config { mutate: Mutate::Calls, ..Config::default() };
    assert!(matches!(run_one(&kit2(), &cfg, line), Outcome::Different(_)));
    // a call without the `.out` entries: the local keeps the fill and the case is not equal
    let o = run_one(&kit2(), &Config::default(), "out_local => eax=0000002b @fetch(57ac57ac,00000007)=00000000");
    assert!(matches!(o, Outcome::Different(_) | Outcome::Skip(_)), "{o:?}");
    // an `.out` entry too many
    let o = run_one(&kit2(), &Config::default(), &format!("{line} @fetch.out(00000001)=00000000"));
    assert!(matches!(&o, Outcome::Different(d) if d.contains("more locals")), "{o:?}");
}

#[test]
fn a_recorded_stack_address_is_exact_by_default_and_any_frame_address_when_normalised() {
    let line = "sink_local => @sink(7fee0f00,00000001)=00000000";
    // exact: the original's frame is not the port's
    let o = run_one(&kit2(), &Config::default(), line);
    assert!(matches!(&o, Outcome::Different(d) if d.contains("call 0 sink: argument 0")), "{o:?}");
    // normalised: a stack address within the frame window is the same thing
    let k = kit2().normalise_stack_args();
    // (the local was filled with the pattern, so the result word is the port's; compare calls only)
    let o = run_one(&k, &Config::default(), "sink_local => @sink(7fee0f00,00000001)=00000000");
    assert_eq!(o, Outcome::Same);
    // a heap address where the original had a stack one stays a difference
    let o = run_one(&k, &Config::default(), "out_not_local => @fetch(7fee0f00,00000007)=00000000");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("stack address")), "{o:?}");
    // a window too small for the port's local
    let o = run_one(&kit2().normalise_stack_args_within(0), &Config::default(), "sink_local => @sink(7fee0f00,00000001)=00000000");
    assert!(matches!(o, Outcome::Different(_)), "{o:?}");
    // the words stored through a normalised local reach the port
    let line = "out_local => eax=0000002b @fetch(7fee0f00,00000007)=00000000 @fetch.out(0000002a)=00000000";
    assert_eq!(run_one(&k, &Config::default(), line), Outcome::Same);
    // and exact mode still refuses it
    assert!(matches!(run_one(&kit2(), &Config::default(), line), Outcome::Different(_)));
}

#[test]
fn reset_globals_start_both_sides_alike_and_are_put_back() {
    // without the declaration the bump is a stray write
    let o = run_one(&kit2(), &Config::default(), "bump_global => eax=00000000");
    assert!(matches!(&o, Outcome::Different(d) if d.contains("outside every buffer")), "{o:?}");
    // declared: the port starts from the stated value and its bump is not reported
    let k = kit2().reset_globals(&[(0x086d_6960, 5)]);
    assert_eq!(run_one(&k, &Config::default(), "bump_global => eax=00000005"), Outcome::Same);
    assert!(matches!(run_one(&k, &Config::default(), "bump_global => eax=00000006"), Outcome::Different(_)));
    // a global the case declares itself is the case's
    assert_eq!(run_one(&k, &Config::default(), "bump_global g086d6960=00000009 => eax=00000009 g086d6960=0000000a"), Outcome::Same);
}

// ---- the replay mode

mod replay_mode {
    use crate::{ReplayCase, ReplayKit, ReplayOutcome, ReplayVerdict};
    use si_mem::{Mem, Routine, Outside};

    fn kit() -> ReplayKit {
        ReplayKit::new(
            |_m, _c| {},
            |m, o, c, names| {
                let a = |m: &Mem, n: &str| c.arg(m, names, n);
                Ok(match c.name.as_str() {
                    "inc" => ReplayOutcome::Returned(Some(a(m, "x") + 1)),
                    "fetch" => {
                        let l = m.local(4);
                        o.call(m, Routine::new("get", 1), &[l, a(m, "x")]);
                        ReplayOutcome::Returned(Some(m.u32(l)))
                    }
                    "leave" => ReplayOutcome::Exited(a(m, "code") as i32),
                    "say" => {
                        o.print(1, b"hi");
                        ReplayOutcome::Returned(None)
                    }
                    other => return Err(format!("no port for `{other}`")),
                })
            },
        )
    }

    fn check(line: &str, mutate: &str) -> ReplayVerdict {
        kit().check(&ReplayCase::parse(line).unwrap().unwrap(), mutate)
    }

    #[test]
    fn results_calls_out_stores_exits_and_text_are_held() {
        assert_eq!(check("inc v_x=00000004 => eax=00000005", ""), ReplayVerdict::Same);
        assert!(matches!(check("inc v_x=00000004 => eax=00000006", ""), ReplayVerdict::Differ(_)));
        let f = "fetch v_x=00000003 => eax=0000002a @get(57ac57ac,00000003)=00000000 @get.out(0000002a)=00000000";
        assert_eq!(check(f, ""), ReplayVerdict::Same);
        assert!(matches!(check(&f.replace("@get(57ac57ac,00000003)", "@get(57ac57ac,00000004)"), ""), ReplayVerdict::Differ(d) if d.contains("argument 1")));
        assert_eq!(check("leave v_code=00000002 => ! STOP:exit:2", ""), ReplayVerdict::Same);
        assert!(matches!(check("leave v_code=00000002 => ! STOP:exit:3", ""), ReplayVerdict::Differ(_)));
        assert_eq!(check("say => stdout=\"hi\"", ""), ReplayVerdict::Same);
        assert!(matches!(check("nobody => ", ""), ReplayVerdict::Differ(d) if d.contains("no port")));
        assert_eq!(check("inc v_x=00000001 => eax=00000002 ! UNINIT:eax", ""), ReplayVerdict::NotCompared);
        assert_eq!(check("inc v_x=00000001 => eax=00000002 ! STOP:budget", "").clone(), ReplayVerdict::Differ("the original stopped (STOP:budget), the port did not".into()));
    }

    #[test]
    fn both_mutation_switches_make_a_compared_case_differ() {
        assert!(matches!(check("inc v_x=00000004 => eax=00000005", "1"), ReplayVerdict::Differ(_)));
        let f = "fetch v_x=00000003 => eax=0000002a @get(57ac57ac,00000003)=00000000 @get.out(0000002a)=00000000";
        assert!(matches!(check(f, "calls"), ReplayVerdict::Differ(_)));
    }

    #[test]
    fn the_report_has_one_row_per_routine() {
        let dir = std::env::temp_dir().join(format!("si-checkkit-replay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.vec"), "# made up\ninc v_x=00000001 => eax=00000002\ninc v_x=00000001 => eax=00000003\nleave v_code=00000000 => ! STOP:exit:0\n").unwrap();
        let (text, ok) = kit().report(&dir, None, "", false);
        std::fs::remove_dir_all(&dir).ok();
        assert!(!ok);
        let inc = text.lines().find(|l| l.starts_with("inc")).unwrap();
        assert_eq!(inc.split_whitespace().collect::<Vec<_>>(), ["inc", "2", "1", "1", "0", "0"]);
        assert!(text.lines().any(|l| l.starts_with("total") && l.contains('3')));
    }
}
