//! Behaviour pins for the interpreter-overhead work (lane
//! `claude/interp-overhead`): faster variable tables, frame/key/argument
//! recycling, hot builtin arms moved up, the `length`/`str` shortcut, the
//! in-place record field read, the interpolation parse cache.
//!
//! The big one is the battery: `interp_overhead_battery.qu` exercises
//! aliasing, frame scoping, recursion limits, error traces, `ref`,
//! `global`, shadowing builtins, interpolation and the fast paths, and
//! `interp_overhead_battery.expected` is what the binary printed for it
//! BEFORE any of these optimisations (the v0.4.9 + dict engine at
//! `3bc756e9`). It must print exactly that.

use qu_interp::{Interp, Value};
use std::path::Path;

fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(128 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn run(src: &str) -> Interp {
    let src = src.to_string();
    on_big_stack(move || {
        let mut it = Interp::new();
        it.run(&src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
        it
    })
}

fn strv(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    match it.get(n) {
        Some(Value::Num(x)) => *x,
        Some(Value::Bool(b)) => f64::from(*b),
        other => panic!("`{n}` is {other:?}"),
    }
}

#[test]
fn battery_prints_exactly_what_it_printed_before_the_optimisations() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let src = std::fs::read_to_string(dir.join("interp_overhead_battery.qu")).unwrap();
    let expected = std::fs::read_to_string(dir.join("interp_overhead_battery.expected"))
        .unwrap()
        .replace("\r\n", "\n");
    let got = on_big_stack(move || {
        let mut it = Interp::new();
        // Same name the expected file was produced under, so `e.trace` lines match.
        it.set_script_path(Path::new("interp_overhead_battery.qu"));
        it.run(&src).unwrap_or_else(|e| panic!("battery failed: {e}"));
        match it.get("report") {
            Some(Value::Str(s)) => s.clone(),
            other => panic!("report is {other:?}"),
        }
    });
    let expected = expected.trim_end().to_string();
    let got = got.trim_end().to_string();
    for (i, (a, b)) in expected.lines().zip(got.lines()).enumerate() {
        assert_eq!(a, b, "battery line {} differs", i + 1);
    }
    assert_eq!(expected.lines().count(), got.lines().count(), "battery length differs");
}

#[test]
fn locals_never_survive_into_the_next_call_even_when_frames_are_recycled() {
    let it = run(
        "function setter()\n    leaked = 5\n    return leaked\nend function\n\
         function reader()\n    r = \"ok\"\n    try\n        r = leaked\n    catch e\n        r = \"undefined\"\n    end\n    return r\nend function\n\
         a = setter()\nb = reader()\nc = setter()\nd = reader()\nres = \"{a}{b}{c}{d}\"",
    );
    assert_eq!(strv(&it, "res"), "5undefined5undefined");
}

#[test]
fn a_failed_call_leaves_the_frame_stack_balanced() {
    // Errors at argument binding, in a default expression, in the body and
    // at the recursion limit must each pop exactly what they pushed.
    let it = run(
        "function f(a, b = missing_name)\n    return a\nend function\n\
         function g(x)\n    local_a = x\n    local_b = undefined_thing\nend function\n\
         function deep(n)\n    return deep(n + 1)\nend function\n\
         function ok(x)\n    t = x * 2\n    return t\nend function\n\
         errs = 0\n\
         try\n    f()\ncatch e\n    errs = errs + 1\nend\n\
         try\n    f(1)\ncatch e\n    errs = errs + 1\nend\n\
         try\n    g(1)\ncatch e\n    errs = errs + 1\nend\n\
         try\n    deep(0)\ncatch e\n    errs = errs + 1\nend\n\
         try\n    ok(1, 2, 3)\ncatch e\n    errs = errs + 1\nend\n\
         r = ok(21)\n\
         try\n    leak = local_a\ncatch e\n    errs = errs + 1\nend",
    );
    assert_eq!(num(&it, "errs"), 6.0);
    assert_eq!(num(&it, "r"), 42.0);
}

#[test]
fn recursion_still_stops_at_the_documented_depth_and_recovers() {
    let it = run(
        "function deep(n)\n    return deep(n + 1)\nend function\n\
         function fact(n)\n    if n <= 1\n        return 1\n    end if\n    return n * fact(n - 1)\nend function\n\
         msg = \"\"\ntry\n    deep(0)\ncatch e\n    msg = e.message\nend\n\
         f10 = fact(10)\nf20 = fact(20)",
    );
    assert_eq!(strv(&it, "msg"), "maximum recursion depth exceeded (1000 nested calls)");
    assert_eq!(num(&it, "f10"), 3628800.0);
    assert_eq!(num(&it, "f20"), 2432902008176640000.0);
}

#[test]
fn error_stack_names_the_whole_chain_and_survives_repeated_calls() {
    // The call-stack name buffers are reused across calls; the snapshot an
    // error takes must still be the live chain, not stale names above it.
    let it = run(
        "function inner()\n    y = undefined_zzz\nend function\n\
         function mid()\n    inner()\nend function\n\
         function outer()\n    mid()\nend function\n\
         function other()\n    return 1\nend function\n\
         o1 = other()\no2 = other()\n\
         st1 = \"\"\ntry\n    outer()\ncatch e\n    st1 = e.stack\nend\n\
         o3 = other()\n\
         st2 = \"\"\ntry\n    mid()\ncatch e\n    st2 = e.stack\nend\n\
         st3 = \"\"\ntry\n    inner()\ncatch e\n    st3 = e.stack\nend",
    );
    assert_eq!(strv(&it, "st1"), "outer > mid > inner");
    assert_eq!(strv(&it, "st2"), "mid > inner");
    assert_eq!(strv(&it, "st3"), "inner");
}

#[test]
fn user_functions_still_shadow_the_length_and_str_shortcuts() {
    let it = run(
        "before = length([1, 2, 3])\nb2 = str(5)\n\
         function length(x)\n    return 4242\nend function\n\
         function str(x)\n    return \"S\"\nend function\n\
         after = length([1, 2, 3])\na2 = str(5)",
    );
    assert_eq!(num(&it, "before"), 3.0);
    assert_eq!(strv(&it, "b2"), "5");
    assert_eq!(num(&it, "after"), 4242.0);
    assert_eq!(strv(&it, "a2"), "S");
}

#[test]
fn length_and_str_shortcuts_keep_their_argument_errors() {
    let mut it = Interp::new();
    for (src, needle) in [
        ("x = length()", "at least one argument"),
        ("x = length(1, 2)", "length reads 1 argument"),
        ("x = str()", "at least one argument"),
        ("x = str(1, 2)", "str reads 1 argument"),
    ] {
        let e = it.run(src).expect_err(src).to_string();
        assert!(e.contains(needle), "{src}: {e}");
    }
}

#[test]
fn value_semantics_hold_through_the_in_place_variable_writes() {
    let it = run(
        "a = [1, 2, 3]\nb = a\nfor i = 0 to 2\n    a[i] = a[i] * 10\nend for\n\
         l = (1, 2)\nm = l\nfor i = 1 to 3\n    l = append(l, i)\nend for\n\
         function f(v)\n    w = v\n    w[0] = -1\n    return v[0]\nend function\n\
         keep = f(b)\n\
         r = \"{a[0]},{a[2]},{b[0]},{b[2]},{length(l)},{length(m)},{keep},{b[0]}\"",
    );
    assert_eq!(strv(&it, "r"), "10,30,1,3,5,2,1,1");
}

#[test]
fn ref_aliases_still_see_writes_with_and_without_a_frame() {
    let it = run(
        "x = 1\nref y = x\ny = 5\nx = x + 1\nres1 = \"{x} {y}\"\n\
         function f()\n    p = 1\n    ref q = p\n    q = 9\n    p = p + 1\n    return \"{p} {q}\"\nend function\n\
         res2 = f()\nres3 = f()",
    );
    assert_eq!(strv(&it, "res1"), "6 6");
    assert_eq!(strv(&it, "res2"), "10 10");
    assert_eq!(strv(&it, "res3"), "10 10");
}

#[test]
fn global_declarations_write_through_and_plain_names_stay_local() {
    let it = run(
        "g = 0\nh = 10\n\
         function bump()\n    global g\n    g = g + 1\n    h = 99\n    return g\nend function\n\
         a = bump()\nb = bump()\nres = \"{g} {h} {a} {b}\"",
    );
    assert_eq!(strv(&it, "res"), "2 10 1 2");
}

#[test]
fn record_field_reads_keep_their_errors_and_scoping() {
    let it = run(
        "rec = {a = 1, b = \"two\"}\n\
         function inner()\n    rec = {a = 100}\n    return rec.a\nend function\n\
         v1 = rec.a\nv2 = inner()\nv3 = rec.a\n\
         m1 = \"\"\ntry\n    q = rec.zzz\ncatch e\n    m1 = e.message\nend\n\
         n = 5\nm2 = \"\"\ntry\n    q = n.field\ncatch e\n    m2 = e.message\nend\n\
         m3 = \"\"\ntry\n    q = nope.field\ncatch e\n    m3 = e.message\nend\n\
         rec = {a = 7, b = \"x\"}\nv4 = rec.a",
    );
    assert_eq!(num(&it, "v1"), 1.0);
    assert_eq!(num(&it, "v2"), 100.0);
    assert_eq!(num(&it, "v3"), 1.0);
    assert_eq!(num(&it, "v4"), 7.0);
    assert_eq!(strv(&it, "m1"), "record has no field `zzz` (fields: a, b)");
    assert_eq!(strv(&it, "m2"), "no field `field` on number");
    assert_eq!(strv(&it, "m3"), "`nope` is not defined");
}

#[test]
fn interpolation_cache_is_scoped_by_evaluation_not_by_text() {
    let it = run(
        "function a()\n    n = 11\n    return \"n={n}\"\nend function\n\
         function b()\n    n = 22\n    return \"n={n}\"\nend function\n\
         r = a() + \" \" + b() + \" \" + a()\n\
         bad = 0\nmsg = \"\"\nfor i = 1 to 3\n    try\n        s = \"x{1 +}\"\n    catch e\n        bad = bad + 1\n        msg = e.message\n    end\nend\n\
         esc = \"{{k}} {1 + 2} \\t|\"\n\
         acc = \"\"\nfor i = 1 to 3\n    acc = acc + \"[{i * i}]\"\nend for",
    );
    assert_eq!(strv(&it, "r"), "n=11 n=22 n=11");
    assert_eq!(num(&it, "bad"), 3.0);
    assert!(strv(&it, "msg").starts_with("parse error"), "{}", strv(&it, "msg"));
    assert_eq!(strv(&it, "esc"), "{k} 3 \t|");
    assert_eq!(strv(&it, "acc"), "[1][4][9]");
}

#[test]
fn plain_string_literals_are_unchanged() {
    let it = run("a = \"hello world\"\nb = \"tab\\there\"\nc = \"\"\nd = \"quote \\\" mark\"\ne1 = \"dollar \\$5\"");
    assert_eq!(strv(&it, "a"), "hello world");
    assert_eq!(strv(&it, "b"), "tab\there");
    assert_eq!(strv(&it, "c"), "");
    assert_eq!(strv(&it, "d"), "quote \" mark");
    assert_eq!(strv(&it, "e1"), "dollar $5");
}

#[test]
fn numeric_accumulators_and_string_builders_both_still_work() {
    let it = run(
        "acc = 0\nfor i = 1 to 4\n    acc = acc + i\nend for\n\
         t = \"\"\nfor i = 1 to 3\n    t = t + \"ab\" + str(i)\nend for\n\
         u = 1.5\nu = u + 2 + 3",
    );
    assert_eq!(num(&it, "acc"), 10.0);
    assert_eq!(strv(&it, "t"), "ab1ab2ab3");
    assert_eq!(num(&it, "u"), 6.5);
}

#[test]
fn hot_builtins_moved_to_the_front_behave_the_same() {
    let it = run(
        "d = dict()\nd = set(d, \"a\", 1)\nd = set(d, \"b\", 2)\n\
         k = keys(d)\nv = values(d)\nhk = has_key(d, \"a\")\nhk2 = has_key(d, \"zz\")\ngv = get(d, \"b\")\ngd = get(d, \"nope\", 7)\n\
         n1 = abs(-3)\nn2 = sqrt(16)\nn3 = floor(2.7)\nn4 = ceil(2.1)\n\
         l = (10, 20, 30)\nf1 = first(l)\nl1 = last(l)\nlen1 = length(l)\n\
         s1 = str(12)\ncnt = length(k) + length(v)",
    );
    assert_eq!(num(&it, "hk"), 1.0);
    assert_eq!(num(&it, "hk2"), 0.0);
    assert_eq!(num(&it, "gv"), 2.0);
    assert_eq!(num(&it, "gd"), 7.0);
    assert_eq!(num(&it, "n1"), 3.0);
    assert_eq!(num(&it, "n2"), 4.0);
    assert_eq!(num(&it, "n3"), 2.0);
    assert_eq!(num(&it, "n4"), 3.0);
    assert_eq!(num(&it, "f1"), 10.0);
    assert_eq!(num(&it, "l1"), 30.0);
    assert_eq!(num(&it, "len1"), 3.0);
    assert_eq!(strv(&it, "s1"), "12");
    assert_eq!(num(&it, "cnt"), 4.0);
}
