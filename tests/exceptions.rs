//! `throw`, `try`/`catch`/`finally` and completion records compiled from
//! JavaScript source (clj-quickjs-11). Programs are JavaScript source:
//! inline synthetic snippets (not light-cljs output) and one adapted QuickJS
//! test (`fixtures/js/quickjs_try_catch.js`, which carries its notice).
//!
//! Effects are observed through a recording host: `__rt.effect(1, x)` logs
//! `x` and returns it.

use clj_quickjs::frontend::{SyntaxErrorKind, compile_script};
use clj_quickjs::{
    Budget, Executable, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView, VmError,
    VmErrorKind,
};

/// Host operation 1 logs its argument and returns it.
#[derive(Default)]
struct Log(Vec<f64>);

impl Host for Log {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => {
                self.0.push(n);
                Ok(call.arg)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

fn exe(src: &str) -> Executable {
    compile_script("test", src).unwrap_or_else(|e| panic!("{src:?}: {e}"))
}

/// Run to the end with an unlimited budget; the outcome and the effect log.
fn run(src: &str) -> (RunOutcome, Vec<f64>) {
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    let mut log = Log::default();
    let outcome = task.run_with(Budget::UNLIMITED, &mut log).unwrap();
    (outcome, log.0)
}

fn view(outcome: RunOutcome, src: &str) -> ValueView {
    match outcome {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{src:?}: expected Complete, got {other:?}"),
    }
}

fn eval(src: &str) -> ValueView {
    view(run(src).0, src)
}

fn num(src: &str) -> f64 {
    match eval(src) {
        ValueView::Number(n) => n,
        other => panic!("{src:?}: expected a number, got {other:?}"),
    }
}

/// The number result and the effect log.
fn num_log(src: &str) -> (f64, Vec<f64>) {
    let (outcome, log) = run(src);
    match view(outcome, src) {
        ValueView::Number(n) => (n, log),
        other => panic!("{src:?}: expected a number, got {other:?}"),
    }
}

fn failure(src: &str) -> (VmError, Vec<f64>) {
    match run(src) {
        (RunOutcome::Failed(e), log) => (e, log),
        (other, _) => panic!("{src:?}: expected Failed, got {other:?}"),
    }
}

/// Kind, message, and the failing source line and column.
fn failure_at(e: &VmError) -> (VmErrorKind, String, u32, u32) {
    let at = e.diagnostic().source.expect("a source location");
    (e.kind(), e.message().to_string(), at.line, at.column)
}

const QUICKJS_TRY_CATCH: &str = include_str!("fixtures/js/quickjs_try_catch.js");

#[test]
fn quickjs_try_catch_cases_pass() {
    // Every case's assert() returned, so the script reached its last line.
    match run(QUICKJS_TRY_CATCH).0 {
        RunOutcome::Complete(v) => {
            assert_eq!(v.view(), ValueView::String);
            assert_eq!(v.string().expect("a string value").to_string(), "ok");
        }
        other => panic!("expected Complete(\"ok\"), got {other:?}"),
    }
}

#[test]
fn quickjs_try_catch_fixture_detects_a_wrong_result() {
    // The same fixture expecting a wrong string fails at its assert(), so
    // the helper is not vacuous.
    assert_eq!(QUICKJS_TRY_CATCH.matches(r#""tfag""#).count(), 1);
    let src = QUICKJS_TRY_CATCH.replace(r#""tfag""#, r#""tfgx""#);
    let (e, _) = failure(&src);
    assert_eq!(e.kind(), VmErrorKind::Thrown, "{e}");
    assert!(
        e.message()
            .contains("assertion failed: got |tfag|, expected |tfgx| (catch)"),
        "{}",
        e.message()
    );
}

#[test]
fn a_thrown_value_is_caught_in_the_same_function() {
    assert_eq!(
        num("let r = 0; try { throw 42; } catch (e) { r = e; } r"),
        42.0
    );
    // Code after the throw does not run; the catch body runs once.
    assert_eq!(
        num_log(
            "let r = 0;
             try { __rt.effect(1, 1); throw 2; __rt.effect(1, 3); }
             catch (e) { r = __rt.effect(1, e * 10); }
             __rt.effect(1, 4);
             r"
        ),
        (20.0, vec![1.0, 20.0, 4.0])
    );
    // No exception: the catch body is skipped.
    assert_eq!(num("let r = 1; try { r = 2; } catch (e) { r = 3; } r"), 2.0);
    // Any value can be thrown, including objects, kept by identity.
    assert_eq!(
        eval("const v = __rt.vector(1); let c; try { throw v; } catch (e) { c = e; } c === v"),
        ValueView::Bool(true)
    );
    for (thrown, expected) in [
        ("undefined", ValueView::Undefined),
        ("null", ValueView::Null),
        ("false", ValueView::Bool(false)),
    ] {
        let src = format!("let r = 1; try {{ throw {thrown}; }} catch (e) {{ r = e; }} r");
        assert_eq!(eval(&src), expected, "{src}");
    }
    // An omitted catch binding.
    assert_eq!(num("let r = 0; try { throw 1; } catch { r = 5; } r"), 5.0);
    // `if` bodies may throw.
    assert_eq!(
        num("let r = 0; try { if (r === 0) throw 3; } catch (e) { r = e; } r"),
        3.0
    );
}

#[test]
fn a_thrown_value_is_caught_in_an_outer_function() {
    assert_eq!(
        num_log(
            "function inner() { __rt.effect(1, 1); throw 7; }
             function middle() { inner(); __rt.effect(1, 2); return 1; }
             function outer() {
               try { return middle(); } catch (e) { return e + 100; }
             }
             const r = outer();
             __rt.effect(1, 3);
             r"
        ),
        (107.0, vec![1.0, 3.0])
    );
    // Unwinding 5,000 frames is a loop over task-owned frames, not Rust
    // recursion, and the task keeps running normally afterwards.
    assert_eq!(
        num(
            "function f(n) { if (n === 0) { throw 99; } return f(n - 1) + 1; }
             let r;
             try { r = f(5000); } catch (e) { r = e; }
             r"
        ),
        99.0
    );
    // An inner handler that does not apply (it was popped when its try
    // block finished) is not reused.
    assert_eq!(
        num("function g() { try { return 1; } catch (e) { return 2; } }
             function h() { g(); throw 3; }
             let r = 0;
             try { h(); } catch (e) { r = e; }
             r"),
        3.0
    );
    // A closure called later throws to whatever handler is live then.
    assert_eq!(
        num("let k;
             try { k = function () { throw 8; }; } catch (e) { k = 0; }
             let r = 0;
             try { k(); } catch (e) { r = e; }
             r"),
        8.0
    );
}

#[test]
fn return_runs_finally_blocks_innermost_first() {
    // test_try_catch6 with effects.
    assert_eq!(
        num_log(
            "function f() {
               try { __rt.effect(1, 1); return 10; __rt.effect(1, 2); }
               finally { __rt.effect(1, 3); }
               __rt.effect(1, 4);
             }
             f()"
        ),
        (10.0, vec![1.0, 3.0])
    );
    assert_eq!(
        num_log(
            "function f() {
               try {
                 try { return 1; } finally { __rt.effect(1, 1); }
               } finally { __rt.effect(1, 2); }
             }
             f()"
        ),
        (1.0, vec![1.0, 2.0])
    );
    // From a catch block, through its finally block.
    assert_eq!(
        num_log(
            "function f() {
               try { throw 5; }
               catch (e) { return e * 2; }
               finally { __rt.effect(1, 9); }
             }
             f()"
        ),
        (10.0, vec![9.0])
    );
    // The return value is evaluated before the finally block runs.
    assert_eq!(
        num_log(
            "function f() {
               let x = 1;
               try { return x; } finally { x = 2; __rt.effect(1, x); }
             }
             f()"
        ),
        (1.0, vec![2.0])
    );
    // A finally block that completes normally keeps a thrown exception too.
    assert_eq!(
        num_log(
            "let r = 0;
             try {
               try { throw 4; } finally { __rt.effect(1, 1); }
             } catch (e) { r = e; }
             r"
        ),
        (4.0, vec![1.0])
    );
}

#[test]
fn finally_overrides_a_prior_return_or_throw() {
    // An abrupt finally completion replaces the pending one.
    assert_eq!(
        num("function f() { try { return 1; } finally { return 2; } } f()"),
        2.0
    );
    assert_eq!(
        num("function f() { try { throw 1; } finally { return 3; } } f()"),
        3.0
    );
    assert_eq!(
        num("function f() { try { return 1; } finally { throw 4; } }
             let r = 0;
             try { f(); } catch (e) { r = e; }
             r"),
        4.0
    );
    assert_eq!(
        num("let r = 0;
             try { try { throw 1; } finally { throw 2; } } catch (e) { r = e; }
             r"),
        2.0
    );
    // A normal finally completion does not.
    assert_eq!(
        num("function f() { try { return 1; } finally { 5; } } f()"),
        1.0
    );
    // A return overriding a pending return still runs the outer finally,
    // and a caught exception inside a finally block keeps its pending
    // completion.
    assert_eq!(
        num_log(
            "function f() {
               try {
                 try { return 1; } finally { return 2; }
               } finally { __rt.effect(1, 7); }
             }
             f()"
        ),
        (2.0, vec![7.0])
    );
    assert_eq!(
        num_log(
            "function f() {
               try { return 1; }
               finally {
                 try { throw 2; } catch (e) { __rt.effect(1, e); }
               }
             }
             f()"
        ),
        (1.0, vec![2.0])
    );
    // A finally block in a frame that is being unwound runs on the way out.
    assert_eq!(
        num_log(
            "function inner() { try { throw 6; } finally { __rt.effect(1, 1); } }
             function outer() { try { inner(); } finally { __rt.effect(1, 2); } }
             let r = 0;
             try { outer(); } catch (e) { r = e; }
             r"
        ),
        (6.0, vec![1.0, 2.0])
    );
}

#[test]
fn catch_blocks_rethrow() {
    assert_eq!(
        num(
            "function f() { try { throw 5; } catch (e) { throw e + 1; } }
             let r = 0;
             try { f(); } catch (e) { r = e; }
             r"
        ),
        6.0
    );
    // The rethrow leaves through the finally block of the same statement.
    assert_eq!(
        num_log(
            "let r = 0;
             try {
               try { throw 1; } catch (e) { throw e * 3; } finally { __rt.effect(1, 9); }
             } catch (e) { r = e; }
             r"
        ),
        (3.0, vec![9.0])
    );
    // An uncaught rethrow reports where it was thrown last.
    let (e, _) = failure("try {\n  throw 1;\n} catch (e) {\n  throw e;\n}");
    assert_eq!(
        failure_at(&e),
        (VmErrorKind::Thrown, "uncaught exception: 1".into(), 4, 3)
    );
}

#[test]
fn catch_parameters_are_block_scoped_bindings() {
    // The parameter shadows an outer binding only inside the catch body.
    assert_eq!(
        num("let e = 1; let r; try { throw 2; } catch (e) { r = e; } r * 10 + e"),
        21.0
    );
    // It can be assigned and captured by a closure.
    assert_eq!(
        num("let k;
             try { throw 1; } catch (e) { k = function () { e = e + 1; return e; }; e = 10; }
             k() + k()"),
        23.0
    );
    // Redeclaring it in the catch body is an early error.
    for src in [
        "try {} catch (e) { let e; }",
        "try {} catch (e) { function e() {} }",
    ] {
        let err = compile_script("t", src).expect_err(src);
        assert_eq!(err.kind, SyntaxErrorKind::Invalid, "{src}: {err}");
    }
    let err = compile_script("t", "try {} catch ({ a }) {}").unwrap_err();
    assert_eq!(err.kind, SyntaxErrorKind::Unsupported, "{err}");
    let err = compile_script("t", "try {}").unwrap_err();
    assert_eq!(err.kind, SyntaxErrorKind::UnexpectedEnd, "{err}");
    let err = compile_script("t", "throw\n1;").unwrap_err();
    assert_eq!(err.kind, SyntaxErrorKind::Invalid, "{err}");
}

#[test]
fn script_completion_values_follow_try_statements() {
    // The block's value, or the catch body's; a normally completing finally
    // block does not change it; an empty body gives `undefined`.
    assert_eq!(num("try { 1; } catch (e) { 2; }"), 1.0);
    assert_eq!(num("try { throw 0; } catch (e) { 5; }"), 5.0);
    assert_eq!(
        eval("1; try { 2; throw 0; } catch (e) {}"),
        ValueView::Undefined
    );
    assert_eq!(num("try { 1; } finally { 2; }"), 1.0);
    assert_eq!(
        num("try { 1; } catch (e) { 2; } finally { 3; if (true) { 4; } }"),
        1.0
    );
    assert_eq!(eval("7; try {} finally { 3; }"), ValueView::Undefined);
}

#[test]
fn javascript_run_time_errors_are_catchable_error_objects() {
    for src in [
        "undeclared;",
        "null();",
        "const c = 1; c = 2;",
        "f(); let x = 1; function f() { return x; }",
        "function f() { return f(); } f();",
    ] {
        let wrapped = format!("let r = 0; try {{ {src} }} catch (e) {{ r = e; }} r");
        assert_eq!(eval(&wrapped), ValueView::Object, "{wrapped}");
    }
    // Each failure is a fresh error object.
    assert_eq!(
        eval(
            "let a; let b;
              try { x; } catch (e) { a = e; }
              try { x; } catch (e) { b = e; }
              a === b"
        ),
        ValueView::Bool(false)
    );
    // Rethrowing an error object reports the original error and position.
    let (e, _) = failure("let y = 0;\ntry {\n  y = z;\n} catch (e) {\n  throw e;\n}");
    assert_eq!(
        failure_at(&e),
        (VmErrorKind::ReferenceError, "z is not defined".into(), 3, 7)
    );
}

#[test]
fn other_failures_end_the_task_without_catch_or_finally() {
    // Object conversion is now supported: this produces NaN and runs finally.
    let (n, log) = num_log(
        "try {
           try { -(function () {}); } catch (e) { __rt.effect(1, 1); }
         } finally { __rt.effect(1, 2); }",
    );
    assert!(n.is_nan());
    assert_eq!(log, [2.0]);
    // Unsupported operations are not JavaScript exceptions: no catch or
    // finally block runs. Host effects require an explicit primitive argument.
    let (e, log) = failure(
        "try {
           try { __rt.effect(1, {}); } catch (e) { __rt.effect(1, 1); }
         } finally { __rt.effect(1, 2); }",
    );
    assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand, "{e}");
    assert_eq!(log, Vec::<f64>::new());
    // Nor are collection contract violations.
    let (e, log) = failure("try { __rt.nth(__rt.vector(), 3); } finally { __rt.effect(1, 2); }");
    assert!(matches!(e.kind(), VmErrorKind::Collection(_)), "{e}");
    assert!(log.is_empty());
}

#[test]
fn unhandled_failures_report_the_source_position_and_call_chain() {
    let src =
        "function inner() {\n  throw 3;\n}\nfunction outer() {\n  return inner();\n}\nouter();";
    let (e, _) = failure(src);
    assert_eq!(
        failure_at(&e),
        (VmErrorKind::Thrown, "uncaught exception: 3".into(), 2, 3)
    );
    let chain: Vec<(String, u32)> = e
        .diagnostic()
        .call_chain
        .iter()
        .map(|site| (site.executable.to_string(), site.source.unwrap().line))
        .collect();
    assert_eq!(chain, [("outer".into(), 5), ("test".into(), 7)]);
    // Leaving through finally blocks keeps the original position and chain.
    let src = "function inner() {\n  try { throw 3; } finally { __rt.effect(1, 1); }\n}\n\
               function outer() {\n  try { return inner(); } finally { __rt.effect(1, 2); }\n}\nouter();";
    let (e2, log) = failure(src);
    assert_eq!(log, [1.0, 2.0]);
    assert_eq!(
        failure_at(&e2),
        (VmErrorKind::Thrown, "uncaught exception: 3".into(), 2, 9)
    );
    assert_eq!(e2.diagnostic().call_chain.len(), 2);
    assert!(e2.to_string().contains("called from outer"), "{e2}");
    // Values are described as JavaScript would print them.
    for (thrown, text) in [
        ("undefined", "undefined"),
        ("null", "null"),
        ("true", "true"),
        ("-1.5", "-1.5"),
        ("1 / 0", "Infinity"),
        ("__rt.vector()", "an object"),
    ] {
        let (e, _) = failure(&format!("throw {thrown};"));
        assert_eq!(e.message(), format!("uncaught exception: {text}"));
    }
}

/// A program whose finally block runs while the only reference to the
/// pending return value (a vector) and to a pending exception (another
/// vector) is the task's completion record.
const PENDING: &str = "
function make() {
  try {
    __rt.effect(1, 1);
    return __rt.vector(1, 2, 3);
  } finally {
    __rt.effect(1, 2);
    let i = 0;
    while (i < 3) { i = i + 1; }
    __rt.effect(1, 3);
  }
}
function fail() {
  try {
    throw __rt.vector(4, 5);
  } finally {
    let i = 0;
    while (i < 3) { i = i + 1; }
    __rt.effect(1, 4);
  }
}
let thrown;
try {
  __rt.effect(1, 0);
  fail();
} catch (e) {
  thrown = e;
} finally {
  __rt.effect(1, 5);
}
__rt.count(make()) * 10 + __rt.count(thrown)
";

#[test]
fn yields_inside_try_catch_and_finally_resume_with_handler_state_intact() {
    let (expected, expected_log) = num_log(PENDING);
    assert_eq!(
        (expected, &expected_log[..]),
        (32.0, &[0.0, 4.0, 5.0, 1.0, 2.0, 3.0][..])
    );
    step_with_collections(PENDING, expected, &expected_log);
}

/// Run `src` one instruction per run, collecting at every yield, and check
/// it completes with `expected` after logging exactly `expected_log`.
fn step_with_collections(src: &str, expected: f64, expected_log: &[f64]) {
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    let mut log = Log::default();
    let mut yields = 0;
    let mut logs_seen = vec![0];
    let result = loop {
        // One instruction per run: the task yields inside every try block,
        // catch block and finally block, and collects at every yield.
        match task.run_with(Budget::instructions(1), &mut log).unwrap() {
            RunOutcome::Yielded => {
                yields += 1;
                if logs_seen.last() != Some(&log.0.len()) {
                    logs_seen.push(log.0.len());
                }
                rt.collect().unwrap();
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(expected));
    // Every effect ran exactly once, in order: no finally block ran because
    // of a yield, and none was replayed after one.
    assert_eq!(log.0, expected_log);
    let one_at_a_time: Vec<usize> = (0..=expected_log.len()).collect();
    assert_eq!(logs_seen, one_at_a_time);
    assert!(yields > 100, "only {yields} yields");
}

#[test]
fn saved_completions_survive_calls_from_finally() {
    // The finally blocks call a JavaScript function (two frames deep) while
    // the only references to the saved return value and the saved exception
    // are the callers' pending completions.
    let src = "
function busy(n) { let i = 0; while (i < n) { i = i + 1; } return i; }
function cleanup(n) { busy(3); __rt.effect(1, n); return busy(2); }
function make() {
  try { return __rt.vector(1, 2, 3); } finally { cleanup(1); }
}
function fail() {
  try { throw __rt.vector(4, 5); } finally { cleanup(2); }
}
let thrown;
try { fail(); } catch (e) { thrown = e; } finally { cleanup(3); }
__rt.count(make()) * 10 + __rt.count(thrown)
";
    let (expected, expected_log) = num_log(src);
    assert_eq!((expected, &expected_log[..]), (32.0, &[2.0, 3.0, 1.0][..]));
    step_with_collections(src, expected, &expected_log);
}

/// `break` and `continue` through finally blocks (clj-quickjs-79).
/// `TEST_TRY_CATCH5` is adapted from `test_try_catch5` in
/// `research/quickjs/tests/test_loop.js` (QuickJS, revision 7d1c13c5d8f8;
/// see `docs/architecture.md` for the attribution): `let` for `var`,
/// `while (true)` for `for(;;)`, and, with no string values, each logged
/// letter is a digit appended with `s = s * 10 + d` ("t" = 1, "b" = 2,
/// "f" = 3), so `"tf"` is 13. The other scripts are written for this
/// project.
///
/// QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
/// released under the MIT license: Permission is hereby granted, free of
/// charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify,
/// merge, publish, distribute, sublicense, and/or sell copies of the
/// Software, and to permit persons to whom the Software is furnished to do
/// so, subject to the following conditions: The above copyright notice and
/// this permission notice shall be included in all copies or substantial
/// portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
/// WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
/// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
/// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
/// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
/// CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH
/// THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
#[test]
fn break_and_continue_run_finally_blocks() {
    const TEST_TRY_CATCH5: &str = "
        let s;
        s = 0;
        while (true) {
            try {
                s = s * 10 + 1;
                break;
                s = s * 10 + 2;
            } finally {
                s = s * 10 + 3;
            }
        }
        s";
    assert_eq!(num(TEST_TRY_CATCH5), 13.0, "\"tf\"");

    // `continue` through a finally block runs it once per iteration, after
    // the protected code and before the test.
    let (i, log) = num_log(
        "let i = 0;
         while (__rt.effect(1, i * 1000) < 3000) {
           i = i + 1;
           try { if (i === 2) continue; __rt.effect(1, i * 10); }
           finally { __rt.effect(1, i); }
           __rt.effect(1, i * 100);
         }
         i",
    );
    assert_eq!(i, 3.0);
    assert_eq!(
        log,
        [
            0.0, 10.0, 1.0, 100.0, 1000.0, 2.0, 2000.0, 30.0, 3.0, 300.0, 3000.0
        ]
    );

    // A `break` through two nested finally blocks (and a catch handler
    // between them) runs the inner one, then the outer one, then leaves the
    // loop; the catch handler it left does not catch the later `throw`.
    let (r, log) = num_log(
        "let r = 0;
         try {
           while (true) {
             try {
               try {
                 try { __rt.effect(1, 1); break; } catch (e) { __rt.effect(1, 91); }
                 __rt.effect(1, 92);
               } finally { __rt.effect(1, 2); }
               __rt.effect(1, 93);
             } finally { __rt.effect(1, 3); }
             __rt.effect(1, 94);
           }
           __rt.effect(1, 4);
           throw 5;
         } catch (e) { r = e; }
         r",
    );
    assert_eq!((r, &log[..]), (5.0, &[1.0, 2.0, 3.0, 4.0][..]));

    // A `continue` from a catch body runs the finally block too.
    let (i, log) = num_log(
        "let i = 0;
         while (i < 2) {
           i = i + 1;
           try { throw i; } catch (e) { __rt.effect(1, e); continue; } finally { __rt.effect(1, 10); }
           __rt.effect(1, 99);
         }
         i",
    );
    assert_eq!((i, &log[..]), (2.0, &[1.0, 10.0, 2.0, 10.0][..]));

    // A `return` inside the finally block overrides the pending `break`.
    let (r, log) = num_log(
        "function f() {
           while (true) {
             try { try { break; } finally { __rt.effect(1, 1); return 5; } }
             finally { __rt.effect(1, 2); }
           }
           return 6;
         }
         f()",
    );
    assert_eq!((r, &log[..]), (5.0, &[1.0, 2.0][..]));

    // A `throw` inside the finally block replaces the pending `break` and is
    // caught by an outer catch block (outer finally blocks run on the way).
    let (r, log) = num_log(
        "let r = 0;
         try {
           while (true) {
             try { try { break; } finally { __rt.effect(1, 1); throw 7; } }
             finally { __rt.effect(1, 2); }
           }
           r = 1;
         } catch (e) { r = e; }
         r",
    );
    assert_eq!((r, &log[..]), (7.0, &[1.0, 2.0][..]));

    // A `break` or `continue` inside the finally block itself discards the
    // pending exception, return or jump.
    assert_eq!(
        num("let r = 0; while (true) { try { throw 7; } finally { r = 1; break; } } r"),
        1.0
    );
    assert_eq!(
        num("let i = 0, r = 0;
             while (i < 3) {
               i = i + 1;
               try { throw i; } catch (e) { r = r + e; throw e; } finally { continue; }
             }
             r"),
        6.0
    );
    assert_eq!(
        num("function f() { while (true) { try { return 1; } finally { break; } } return 2; } f()"),
        2.0
    );
    let (r, log) = num_log(
        "let r = 0;
         while (true) {
           try {
             while (true) { try { break; } finally { __rt.effect(1, 1); break; } }
             __rt.effect(1, 2);
           } finally { __rt.effect(1, 3); }
           r = 4;
           break;
         }
         r",
    );
    assert_eq!((r, &log[..]), (4.0, &[1.0, 2.0, 3.0][..]));

    // A jump that stays inside a finally block keeps that block's pending
    // completion: the pending return still completes.
    assert_eq!(
        num("function f() {
               try { return 1; }
               finally { let i = 0; while (true) { try { i = i + 1; break; } finally {} } }
             }
             f()"),
        1.0
    );

    // Completion values: a finally block that completes normally keeps the
    // value the jump carries; a jump out of a finally block carries the
    // value that block produced.
    assert_eq!(
        num("5; while (true) { try { 6; break; } finally { 7; } }"),
        6.0
    );
    assert_eq!(
        eval("5; while (true) { 1; try { break; } finally { 7; } }"),
        ValueView::Undefined
    );
    assert_eq!(
        num("5; while (true) { try { throw 1; } catch (e) { 4; break; } finally { 7; } }"),
        4.0
    );
    assert_eq!(
        num("while (true) { try { throw 1; } finally { 8; break; } }"),
        8.0
    );
    assert_eq!(
        eval("while (true) { 3; try { throw 1; } finally { break; } }"),
        ValueView::Undefined
    );
}

/// Finally blocks reached by `break` and `continue`, with calls and
/// allocations inside, yield after every two instructions; each run happens
/// on a new thread with a collection in between, and the jumps still
/// complete with the same result and effects, each exactly once.
#[test]
fn a_jump_through_finally_survives_yields_migration_and_collection() {
    let src = "
        function busy(n) { let i = 0; while (i < n) { i = i + 1; } return i; }
        let s = 0, i = 0;
        while (i < 3) {
          i = i + 1;
          try {
            try { if (i === 2) continue; if (i === 3) break; s = s * 10 + 1; }
            finally { s = s * 10 + __rt.effect(1, busy(i)); }
          } finally { s = s * 10 + __rt.effect(1, __rt.count(__rt.vector(i, i))) * 2 + 1; }
        }
        s";
    let (expected, expected_log) = num_log(src);
    assert_eq!(
        (expected, &expected_log[..]),
        (1152535.0, &[1.0, 2.0, 2.0, 2.0, 3.0, 2.0][..])
    );
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    let mut log = Log::default();
    let mut yields = 0;
    let result = loop {
        let (outcome, back) = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let mut log = std::mem::take(&mut log);
                    let outcome = task.run_with(Budget::instructions(2), &mut log).unwrap();
                    (outcome, log)
                })
                .join()
                .unwrap()
        });
        log = back;
        match outcome {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(expected));
    assert_eq!(log.0, expected_log);
    assert!(yields > 50, "only {yields} yields");
}
