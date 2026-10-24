//! JavaScript source compiled to VM bytecode and executed by tasks
//! (clj-quickjs-22). Programs here are JavaScript source: synthetic fixtures
//! (`tests/fixtures/js/`, not light-cljs output) and inline snippets. The
//! bytecode-corruption tests start from compiled code and edit its bytes.

use clj_quickjs::frontend::{SyntaxErrorKind, compile_function, compile_script};
use clj_quickjs::{
    Budget, Executable, Host, HostCall, Opcode, Primitive, RunOutcome, Runtime,
    ValidationErrorKind, ValueView, VmError, VmErrorKind,
};

/// A host for budgeted runs of code that performs no host effects.
struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

fn exe(src: &str) -> Executable {
    compile_script("test", src).unwrap_or_else(|e| panic!("{src:?}: {e}"))
}

fn run(exe: &Executable, args: &[Primitive]) -> RunOutcome {
    let rt = Runtime::new();
    let roots: Vec<_> = args.iter().map(|&p| rt.root(p)).collect();
    let refs: Vec<_> = roots.iter().collect();
    rt.spawn(exe, &refs).unwrap().run().unwrap()
}

fn eval(src: &str) -> ValueView {
    match run(&exe(src), &[]) {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{src:?}: expected Complete, got {other:?}"),
    }
}

fn num(src: &str) -> f64 {
    match eval(src) {
        ValueView::Number(n) => n,
        other => panic!("{src:?}: expected a number, got {other:?}"),
    }
}

fn failure(src: &str) -> VmError {
    match run(&exe(src), &[]) {
        RunOutcome::Failed(e) => e,
        other => panic!("{src:?}: expected Failed, got {other:?}"),
    }
}

/// Kind, message, and the failing source line and column.
fn failure_at(src: &str) -> (VmErrorKind, String, u32, u32) {
    let e = failure(src);
    let at = e
        .diagnostic()
        .source
        .unwrap_or_else(|| panic!("{src:?}: no source location in {e}"));
    (e.kind(), e.message().to_string(), at.line, at.column)
}

fn opcodes(exe: &Executable) -> Vec<Opcode> {
    decode(&exe.to_spec().code)
        .into_iter()
        .map(|(_, op)| op)
        .collect()
}

/// Byte offset and opcode of every instruction in encoded `code`.
fn decode(code: &[u8]) -> Vec<(usize, Opcode)> {
    let mut ops = Vec::new();
    let mut pos = 0;
    while pos < code.len() {
        let op = Opcode::from_byte(code[pos]).unwrap();
        ops.push((pos, op));
        pos += 1 + match op {
            Opcode::Jump | Opcode::JumpIfFalse => 4,
            Opcode::Const
            | Opcode::GetLocal
            | Opcode::SetLocal
            | Opcode::HostEffect
            | Opcode::NewVector
            | Opcode::NewMap
            | Opcode::ThrowError
            | Opcode::Function
            | Opcode::Call
            | Opcode::GetCapture => 2,
            _ => 0,
        };
    }
    ops
}

#[test]
fn fixtures_compute_through_vm_instructions() {
    assert_eq!(num(include_str!("fixtures/js/arithmetic.js")), 27.5);
    assert_eq!(num(include_str!("fixtures/js/control_flow.js")), 7307.0);
    assert_eq!(num(include_str!("fixtures/js/quickjs_op1_while.js")), 0.0);

    // The fixtures run as ordinary VM code: arithmetic and comparison
    // instructions, a conditional branch and a backward jump.
    let control = exe(include_str!("fixtures/js/control_flow.js"));
    let ops = opcodes(&control);
    for op in [
        Opcode::Rem,
        Opcode::Add,
        Opcode::Lt,
        Opcode::StrictEq,
        Opcode::JumpIfFalse,
        Opcode::Jump,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    // No constant folding: `1 + 2 * 3` is evaluated by the VM.
    let ops = opcodes(&exe("1 + 2 * 3"));
    assert!(
        ops.contains(&Opcode::Mul) && ops.contains(&Opcode::Add),
        "{ops:?}"
    );
}

#[test]
fn loops_yield_and_resume_mid_execution_with_source_positions() {
    let src = "let i = 0;\nlet acc = 0;\nwhile (i < 100) {\n  acc = acc + i;\n  i = i + 1;\n}\nacc";
    let exe = exe(src);
    let rt = Runtime::new();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut yields = 0;
    let mut lines = std::collections::BTreeSet::new();
    let result = loop {
        match task
            .run_with(Budget::instructions(7), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                // The saved frame reports where it will continue.
                let at = task.source_location().expect("compiled code has locations");
                lines.insert(at.line);
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(4950.0));
    assert!(yields > 100, "only {yields} yields");
    // Suspension points fall on the loop's lines (3-5), not only one.
    assert!(lines.is_superset(&[3, 4, 5].into()), "{lines:?}");
}

#[test]
fn numbers_follow_ieee_double_semantics() {
    // NaN and Infinity.
    assert!(num("0 / 0").is_nan());
    assert!(num("NaN + 1").is_nan());
    assert_eq!(num("1 / 0"), f64::INFINITY);
    assert_eq!(num("-1 / 0"), f64::NEG_INFINITY);
    assert_eq!(num("1e308 * 10"), f64::INFINITY);
    assert!(num("Infinity - Infinity").is_nan());
    assert_eq!(eval("NaN === NaN"), ValueView::Bool(false));
    assert_eq!(eval("NaN !== NaN"), ValueView::Bool(true));
    assert_eq!(
        eval("NaN < 1 || NaN >= 1 || NaN <= NaN"),
        ValueView::Bool(false)
    );
    // Negative zero is produced and preserved, but equals zero.
    for src in [
        "-0", "0 * -1", "-0 + -0", "-1 % 1", "-0 % 5", "0 / -3", "-(0)",
    ] {
        let n = num(src);
        assert!(n == 0.0 && n.is_sign_negative(), "{src} gave {n}");
    }
    assert!(num("-0 + 0").is_sign_positive());
    assert_eq!(eval("-0 === 0"), ValueView::Bool(true));
    assert_eq!(num("1 / -0"), f64::NEG_INFINITY);
    assert_eq!(num("1 / (0 * -1)"), f64::NEG_INFINITY);
    // Remainder takes the dividend's sign; NaN and Infinity edges.
    assert_eq!(num("-5 % 3"), -2.0);
    assert_eq!(num("5 % -3"), 2.0);
    assert_eq!(num("5.5 % 2"), 1.5);
    assert!(num("5 % 0").is_nan());
    assert!(num("Infinity % 2").is_nan());
    assert_eq!(num("7 % Infinity"), 7.0);
    // Integers are doubles: exact up to 2^53, then rounding to even.
    assert_eq!(num("9007199254740991 + 1"), 9007199254740992.0);
    assert_eq!(
        eval("9007199254740992 + 1 === 9007199254740992"),
        ValueView::Bool(true)
    );
    assert_eq!(num("9007199254740993"), 9007199254740992.0);
    assert_eq!(eval("0.1 + 0.2 === 0.3"), ValueView::Bool(false));
    assert_eq!(num("0.1 + 0.2"), 0.30000000000000004);
    assert_eq!(num("7 / 2"), 3.5);
    assert_eq!(num("0x10 + 0o10 + 0b10"), 26.0);
    assert_eq!(num("5e-324 / 2"), 0.0);
    // Comparison operators.
    assert_eq!(
        eval("1 <= 1 && 2 >= 1 && 2 > 1 && !(1 > 1)"),
        ValueView::Bool(true)
    );
    assert_eq!(eval("-Infinity < -1e308"), ValueView::Bool(true));
}

#[test]
fn primitives_convert_with_to_number() {
    assert_eq!(num("true + 1"), 2.0);
    assert_eq!(num("null + 1"), 1.0);
    assert!(num("undefined + 1").is_nan());
    assert_eq!(num("+true"), 1.0);
    assert_eq!(num("+false"), 0.0);
    assert!(num("+undefined").is_nan());
    let n = num("-null");
    assert!(n == 0.0 && n.is_sign_negative());
    assert_eq!(num("let t = true; t * 3 - false"), 3.0);
    assert_eq!(eval("null < 1 && false < true"), ValueView::Bool(true));
    assert_eq!(
        eval("undefined < 1 || undefined > 1"),
        ValueView::Bool(false)
    );
    // Conversion happens after both operands are evaluated: the right
    // operand's ReferenceError wins over anything about the left one.
    let (kind, ..) = failure_at("let t = true; t + nope");
    assert_eq!(kind, VmErrorKind::ReferenceError);
}

#[test]
fn truthiness_and_equality_follow_javascript() {
    for falsy in ["0", "-0", "NaN", "null", "undefined", "false"] {
        assert_eq!(eval(&format!("!{falsy}")), ValueView::Bool(true), "{falsy}");
        assert_eq!(num(&format!("{falsy} ? 1 : 2")), 2.0, "{falsy}");
    }
    for truthy in ["1", "-1", "0.5", "Infinity", "-Infinity", "true"] {
        assert_eq!(
            eval(&format!("!{truthy}")),
            ValueView::Bool(false),
            "{truthy}"
        );
        assert_eq!(num(&format!("let n = 0; if ({truthy}) n = 1; n")), 1.0);
    }
    // Logical operators return an operand, not a boolean, and short-circuit
    // (the unknown name on the right is never evaluated).
    assert_eq!(eval("0 || null"), ValueView::Null);
    assert_eq!(num("1 && 2"), 2.0);
    assert_eq!(num("0 && nope"), 0.0);
    assert_eq!(num("3 || nope"), 3.0);
    assert_eq!(eval("null && 1"), ValueView::Null);
    assert_eq!(num("false || 0 || 4"), 4.0);
    // Strict equality.
    let strict = [
        ("1 === 1", true),
        ("1 === true", false),
        ("null === undefined", false),
        ("undefined === undefined", true),
        ("0 !== -0", false),
        ("true !== false", true),
    ];
    // Loose equality: null/undefined only equal each other; booleans compare
    // as numbers.
    let loose = [
        ("null == undefined", true),
        ("undefined == null", true),
        ("null == 0", false),
        ("undefined == 0", false),
        ("null == false", false),
        ("1 == true", true),
        ("0 == false", true),
        ("2 == true", false),
        ("NaN == NaN", false),
        ("0 == -0", true),
        ("1 != true", false),
        ("null != undefined", false),
        ("null != 0", true),
    ];
    for (src, expected) in strict.into_iter().chain(loose) {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }
}

#[test]
fn script_completion_value_follows_javascript() {
    assert_eq!(eval(""), ValueView::Undefined);
    assert_eq!(num("1; ;"), 1.0);
    assert_eq!(num("1; let x = 2;"), 1.0);
    assert_eq!(num("1; {}"), 1.0);
    assert_eq!(eval("1; if (false) 2;"), ValueView::Undefined);
    assert_eq!(eval("1; if (true) {}"), ValueView::Undefined);
    assert_eq!(num("if (true) { 3; let y; }"), 3.0);
    assert_eq!(num("if (false) 1; else 2;"), 2.0);
    assert_eq!(eval("1; while (false) {}"), ValueView::Undefined);
    assert_eq!(num("let i = 0; while (i < 3) { i = i + 1; }"), 3.0);
    // A loop whose body never produces a value completes with undefined,
    // not with the value before the loop.
    assert_eq!(
        eval("let i = 0; 5; while (i < 3) { let j = (i = i + 1); }"),
        ValueView::Undefined
    );
    // An `if` without a value inside a loop body resets it to undefined.
    assert_eq!(
        eval("let i = 0; while (i < 2) { i = i + 1; if (i > 5) { 9; } }"),
        ValueView::Undefined
    );
    // `break`/`continue` (clj-quickjs-78) carry the value their statement
    // list produced so far; an `if` around them makes it undefined.
    assert_eq!(num("5; while (true) { 6; { break; } }"), 6.0);
    assert_eq!(
        eval("let i = 0; while (true) { i = i + 1; if (i > 2) break; }"),
        ValueView::Undefined
    );
    assert_eq!(
        num("let i = 0; while (i < 3) { i = i + 1; continue; 7; }"),
        3.0
    );
    assert_eq!(
        eval("let i = 0; 1; while (i < 2) { i = i + 1; if (i == 2) { continue; } 8; }"),
        ValueView::Undefined
    );
    assert_eq!(
        num("while (true) { 1; try { 2; break; } catch (e) {} }"),
        2.0
    );
    assert_eq!(
        eval("3; while (true) { 4; while (true) { break; } break; }"),
        ValueView::Undefined
    );
}

/// Unlabelled `break` and `continue` in `while` loops (clj-quickjs-78).
/// `TEST_WHILE_BREAK` is adapted from `test_while_break` in
/// `research/quickjs/tests/test_loop.js` (QuickJS, revision 7d1c13c5d8f8;
/// see `docs/architecture.md` for the attribution): `let` for `var`,
/// `x = x + 1` for `x++`, and the asserted condition is the completion
/// value. The other scripts are written for this project.
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
fn while_break_and_continue() {
    const TEST_WHILE_BREAK: &str = "
        let i, c;
        i = 0;
        c = 0;
        while (i < 3) {
            c = c + 1;
            if (i == 1)
                break;
            i = i + 1;
        }
        c === 2 && i === 1";
    assert_eq!(eval(TEST_WHILE_BREAK), ValueView::Bool(true));
    // The same loop under a 3-instruction budget yields inside it and
    // resumes there with the same result.
    let exe = exe(TEST_WHILE_BREAK);
    let task = Runtime::new().spawn(&exe, &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        match task
            .run_with(Budget::instructions(3), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(result, ValueView::Bool(true));
    assert!(yields >= 1, "{yields} yields");

    // `continue` skips the rest of the body: the odd numbers below 10.
    assert_eq!(
        num("let i = 0, sum = 0;
             while (i < 10) {
                 i = i + 1;
                 if (i % 2 === 0) continue;
                 if (i >= 10) continue;
                 sum = sum + i;
             }
             sum"),
        25.0
    );
    // `break` leaves only the innermost loop; `continue` restarts it.
    assert_eq!(
        num("let i = 0, log = 0;
             while (i < 3) {
                 i = i + 1;
                 let j = 0;
                 while (true) {
                     j = j + 1;
                     if (j === 2) continue;
                     if (j > i) break;
                     log = log * 10 + j;
                 }
                 log = log * 10;
             }
             log"),
        // i = 1: j 1, 3 breaks; i = 2: j 1, 3 breaks; i = 3: j 1, 3, 4 breaks.
        1010130.0
    );
    // Loop bodies make new bindings each iteration, also after `continue`.
    assert_eq!(
        num("let fs = 0, i = 0;
             while (i < 3) {
                 i = i + 1;
                 const k = i;
                 if (k === 1) { fs = () => k; continue; }
                 if (k === 2) break;
             }
             fs() * 10 + i"),
        12.0
    );
    // In a function: `break` then `return`; a loop inside a callee.
    assert_eq!(
        num("function find(n) {
                 let i = 0;
                 while (true) { if (i * i >= n) break; i = i + 1; }
                 return i;
             }
             find(10) * 100 + find(0)"),
        400.0
    );

    // Leaving a `try` block pops its catch handler: the `throw` after the
    // loop is not caught by it and escapes. (Each loop runs a bounded number
    // of times even if a stale handler caught the `throw` and re-entered it.)
    let thrown = |src: &str| {
        let (kind, message, _, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::Thrown, "{src}: {message}");
        message
    };
    assert_eq!(
        thrown(
            "let n = 0;\nwhile (n === 0) { try { n = 1; break; } catch (e) { n = 99; } }\nthrow n + 1;"
        ),
        "uncaught exception: 2"
    );
    assert_eq!(
        thrown(
            "let n = 0;\nwhile (n < 3) { try { n = n + 1; continue; } catch (e) { n = 99; } }\nthrow n;"
        ),
        "uncaught exception: 3"
    );
    // ... also two nested try blocks, and a catch body (whose handler is
    // already gone) inside a try block.
    assert_eq!(
        thrown(
            "let n = 0;\nwhile (n === 0) { n = 1; try { try { break; } catch (e) { n = 50; } } catch (e) { n = 60; } }\nthrow n + 3;"
        ),
        "uncaught exception: 4"
    );
    assert_eq!(
        thrown(
            "let n = 0;\nwhile (n === 0) { n = 1; try { try { throw 1; } catch (e) { break; } } catch (e) { n = 60; } }\nthrow n + 4;"
        ),
        "uncaught exception: 5"
    );
    // A handler outside the loop stays: it catches the later `throw`.
    assert_eq!(
        num("let r = 0, n = 0;
             try {
                 while (n === 0) { n = 1; try { break; } catch (e) { r = 1; } }
                 throw 7;
             } catch (e) { r = r * 10 + e; }
             r"),
        7.0
    );
    // A loop inside a `try` with a finally block: the jump stays inside.
    assert_eq!(
        num("let r = 0;
             try { while (true) { r = r + 1; if (r < 3) continue; break; } }
             finally { r = r * 10; }
             r"),
        30.0
    );

    // Completion values (ECMAScript UpdateEmpty).
    assert_eq!(num("5; while (true) { 6; break; }"), 6.0);
    assert_eq!(eval("5; while (true) { break; }"), ValueView::Undefined);

    // Jumps out of a `try` statement with a finally block wait for
    // clj-quickjs-79: rejected at the keyword.
    for (src, column) in [
        ("while (x) { try { break; } finally {} }", 19),
        (
            "while (x) { try {} catch (e) { continue; } finally {} }",
            32,
        ),
        ("while (x) { try {} finally { break; } }", 30),
        (
            "while (x) { try { try { continue; } catch (e) {} } finally {} }",
            25,
        ),
    ] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(
            (e.kind, e.line, e.column),
            (SyntaxErrorKind::Unsupported, 1, column),
            "{src}: {e}"
        );
        assert!(e.message.contains("`finally` block"), "{src}: {e}");
    }
}

#[test]
fn function_entry_points_take_arguments_and_return() {
    let sum = compile_function(
        "function sum(n) {\n  let acc = 0;\n  while (n > 0) { acc = acc + n; n = n - 1; }\n  return acc;\n}",
    )
    .unwrap();
    assert_eq!(sum.name(), "sum");
    let complete = |exe: &Executable, args: &[Primitive]| match run(exe, args) {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        complete(&sum, &[Primitive::Number(10.0)]),
        ValueView::Number(55.0)
    );
    // A missing argument is undefined: `undefined > 0` is false.
    assert_eq!(complete(&sum, &[]), ValueView::Number(0.0));

    // `return` leaves the loop early; falling off the end returns undefined.
    let find = compile_function(
        "function find(limit, step) {
           let i = 0;
           while (true) {
             if (i * step > limit) { return i; }
             i = i + 1;
           }
         }",
    )
    .unwrap();
    let args = [Primitive::Number(50.0), Primitive::Number(7.0)];
    assert_eq!(complete(&find, &args), ValueView::Number(8.0));
    let nothing = compile_function("function f(a, b) { a = b; }").unwrap();
    assert_eq!(complete(&nothing, &[]), ValueView::Undefined);
    let bare = compile_function("function f() { if (true) return; return 1; }").unwrap();
    assert_eq!(complete(&bare, &[]), ValueView::Undefined);

    // Exactly one function declaration is required.
    for src in ["1", "function f() {} function g() {}", ""] {
        let e = compile_function(src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Unsupported, "{src}");
    }
}

#[test]
fn hashbang_script_runs() {
    assert_eq!(num("#!/usr/bin/env qjs\n1 + 1"), 2.0);
    // The hashbang line still counts: the error is reported on line 2.
    let (kind, message, line, column) = failure_at("#!/usr/bin/env qjs\nmissing + 1");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "missing is not defined");
    assert_eq!((line, column), (2, 1));
}

#[test]
fn identifiers_are_not_normalized() {
    // Identifier names are code point sequences, compared without Unicode
    // normalization: `Ãµ` (U+00C3 U+00B5) and `õ` (U+00F5) are different
    // names. The `Ãµ` / `õ` case is adapted from `test_unicode_ident` in
    // `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
    // (`let` for `var`; a ReferenceError for `typeof õ`, as `typeof` is not
    // in the subset); see `docs/architecture.md`, "QuickJS references and
    // provenance".
    //
    // QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
    // released under the MIT license: Permission is hereby granted, free of
    // charge, to any person obtaining a copy of this software and associated
    // documentation files (the "Software"), to deal in the Software without
    // restriction, including without limitation the rights to use, copy,
    // modify, merge, publish, distribute, sublicense, and/or sell copies of
    // the Software, and to permit persons to whom the Software is furnished
    // to do so, subject to the following conditions: The above copyright
    // notice and this permission notice shall be included in all copies or
    // substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
    // WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
    // LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
    // PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
    // HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
    // AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
    // CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
    // SOFTWARE.
    assert_eq!(num("let Ãµ = 3; Ãµ"), 3.0);
    let (kind, message, line, column) = failure_at("let Ãµ = 3; õ");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "õ is not defined");
    assert_eq!((line, column), (1, 13));
    // Canonically equivalent (NFC) and compatibility-equivalent (NFKC)
    // spellings are distinct bindings, not redeclarations.
    assert_eq!(
        num("let \u{E9} = 1, e\u{301} = 2; \u{E9} * 10 + e\u{301}"),
        12.0
    );
    assert_eq!(num("let A = 1, 𝒜 = 2; A * 10 + 𝒜"), 12.0);
    // Escapes are decoded before names are compared.
    assert_eq!(num("let a\\u0062c = 2; abc"), 2.0);
    assert_eq!(num("let abc = 2; a\\u{62}c + \\u0061bc"), 4.0);
    assert_eq!(num("let \\u{C3}\\u{B5} = 5; Ãµ"), 5.0);
    // Diagnostics quote the name as the source spells it.
    let (kind, message, line, column) = failure_at("let \\u{C3}\\u{B5} = 5;\n\\u{F5}");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "\\u{F5} is not defined");
    assert_eq!((line, column), (2, 1));
    // Functions, parameters and closures resolve Unicode names alike.
    assert_eq!(
        num("function π\\u0393(r) { let τ = 2; return function() { return τ * r; }; } πΓ(3)()"),
        6.0
    );
}

#[test]
fn unknown_names_fail_with_reference_errors_at_their_position() {
    let (kind, message, line, column) = failure_at("let a = 1;\na + missing");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "missing is not defined");
    assert_eq!((line, column), (2, 5));
    // Strict-mode assignment to an undeclared name, after evaluating the
    // right-hand side.
    let (kind, message, line, column) = failure_at("let a = 1;\n  b = a;");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "b is not defined");
    assert_eq!((line, column), (2, 3));
    // Only when the code runs: an unreached reference is not an error.
    assert_eq!(num("if (false) { nope; nope = 1; } 1"), 1.0);
    // A binding is visible only in its block.
    let (kind, message, ..) = failure_at("{ let inner = 1; }\ninner");
    assert_eq!(
        (kind, message.as_str()),
        (VmErrorKind::ReferenceError, "inner is not defined")
    );
    let e = failure("x");
    assert!(e.to_string().contains("at source 1:1"), "{e}");
}

#[test]
fn const_reassignment_fails_with_a_type_error() {
    let (kind, message, line, column) = failure_at("const c = 1;\nlet d = 2;\nd = c;\nc = d + 1;");
    assert_eq!(kind, VmErrorKind::TypeError);
    assert_eq!(message, "assignment to constant variable 'c'");
    assert_eq!((line, column), (4, 1));
    // Not reached, not thrown; a shadowing `let` is assignable.
    assert_eq!(num("const c = 1; if (c > 1) { c = 2; } c"), 1.0);
    assert_eq!(num("const c = 1; { let c = 5; c = c + 1; } c"), 1.0);
    // The right-hand side is evaluated first.
    let (kind, ..) = failure_at("const c = 1; c = missing;");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // A const in its temporal dead zone reports the ReferenceError first.
    let (kind, ..) = failure_at("c = 1; const c = 2;");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // Parameters are assignable.
    let f = compile_function("function f(p) { p = p + 1; return p; }").unwrap();
    assert!(
        matches!(run(&f, &[Primitive::Number(1.0)]), RunOutcome::Complete(v) if v.view() == ValueView::Number(2.0))
    );
}

#[test]
fn use_before_initialization_fails_with_reference_errors() {
    let cases = [
        // Before the declaration in the same list.
        ("x;\nlet x = 1;", "x", 1, 1),
        // Inside its own initializer.
        ("let y = 1;\nlet x = y + x;", "x", 2, 13),
        // In a nested block, before the outer declaration.
        ("{\n  { x; }\n  let x = 1;\n}", "x", 2, 5),
        // An inner declaration shadows the outer binding for the whole block.
        ("let x = 1;\n{\n  x;\n  let x = 2;\n}", "x", 3, 3),
        // Assignment before the declaration.
        ("x = 1;\nlet x;", "x", 1, 1),
        // A const too.
        ("let a = k;\nconst k = 1;", "k", 1, 9),
    ];
    for (src, name, line, column) in cases {
        let (kind, message, l, c) = failure_at(src);
        assert_eq!(kind, VmErrorKind::ReferenceError, "{src}");
        assert_eq!(
            message,
            format!("cannot access '{name}' before initialization"),
            "{src}"
        );
        assert_eq!((l, c), (line, column), "{src}");
    }
    // Each loop iteration re-enters the block, so the binding is fresh and
    // uninitialized at the top of every iteration: the second iteration fails.
    let src = "let i = 0;\nwhile (i < 3) {\n  if (i === 1) { z; }\n  let z = i;\n  i = i + 1;\n}";
    let (kind, message, line, column) = failure_at(src);
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "cannot access 'z' before initialization");
    assert_eq!((line, column), (3, 18));
    // Never reached: no error. After the declaration: initialized.
    assert_eq!(num("if (false) { x; } let x = 4; x"), 4.0);
    assert_eq!(eval("let u; u"), ValueView::Undefined);
    // `let x;` re-initializes to undefined on every iteration.
    assert_eq!(
        eval(
            "let i = 0, seen = 0; while (i < 2) { let v; if (v === undefined) seen = seen + 1; v = 1; i = i + 1; } seen"
        ),
        ValueView::Number(2.0)
    );
}

#[test]
fn redeclaration_and_unsupported_forms_are_compile_errors() {
    let compile_err = |src: &str| {
        let e = compile_script("t", src).unwrap_err();
        (e.kind, e.line, e.column)
    };
    use SyntaxErrorKind::{Invalid, Unsupported};
    assert_eq!(compile_err("let a;\nlet a;"), (Invalid, 2, 5));
    assert_eq!(compile_err("const a = 1, a = 2;"), (Invalid, 1, 14));
    assert_eq!(
        compile_err("let a; { const b = 1; let b; }"),
        (Invalid, 1, 27)
    );
    let e = compile_function("function f(a) {\n  let a = 1;\n}").unwrap_err();
    assert_eq!((e.kind, e.line, e.column), (Invalid, 2, 7));
    assert!(e.message.contains("parameter"), "{}", e.message);
    // Shadowing in an inner block is fine.
    assert_eq!(num("let a = 1; { let a = 2; } a"), 1.0);

    // Function declarations take part in redeclaration checks: in a block
    // (lexical) and against `let`/`const` they are JavaScript early errors.
    assert_eq!(compile_err("function f() {}\nlet f;"), (Invalid, 2, 5));
    assert_eq!(
        compile_err("{ function g() {} function g() {} }"),
        (Invalid, 1, 28)
    );

    // Valid syntax that cannot execute yet is rejected where it starts.
    // (Functions and calls execute since clj-quickjs-21.)
    for (src, line, column) in [
        ("let s = 'text';", 1, 9),
        ("let o = 1;\no.p;", 2, 1),
        ("let o = 1;\no.p(1);", 2, 1),
        // Var-scoped redeclaration by a function declaration, which
        // JavaScript allows at the top level of a script or function body.
        ("function f(a) { function a() {} }", 1, 26),
        ("function h() {}\nfunction h() {}", 2, 10),
    ] {
        assert_eq!(compile_err(src), (Unsupported, line, column), "{src}");
    }
    // Syntax errors pass through from the parser.
    assert_eq!(compile_err("let if = 1;").0, SyntaxErrorKind::ReservedWord);
}

#[test]
fn corrupted_compiled_bytecode_is_rejected_by_the_validator() {
    let exe = exe("let i = 0;\nwhile (i < 3) { i = i + 1; }\ni");
    let spec = exe.to_spec();
    // The re-encoded spec loads and runs unchanged.
    let reloaded = Executable::load(spec.clone()).unwrap();
    assert!(
        matches!(run(&reloaded, &[]), RunOutcome::Complete(v) if v.view() == ValueView::Number(3.0))
    );
    assert_eq!(
        reloaded.source_location(0),
        None,
        "byte format carries no locations"
    );
    assert!(exe.source_location(0).is_some());

    let instrs = decode(&spec.code);
    let find = |op: Opcode| instrs.iter().find(|(_, o)| *o == op).unwrap().0;

    // Bad branch target: into the middle of an instruction.
    let mut bad = spec.clone();
    let at = find(Opcode::JumpIfFalse);
    bad.code[at + 1..at + 5].copy_from_slice(&(at as u32 + 2).to_le_bytes());
    let e = Executable::load(bad).unwrap_err();
    assert_eq!(
        e.kind,
        ValidationErrorKind::BadBranchTarget {
            target: at as u32 + 2
        }
    );
    // ... and past the end.
    let mut bad = spec.clone();
    bad.code[at + 1..at + 5].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Executable::load(bad).unwrap_err().kind,
        ValidationErrorKind::BadBranchTarget { .. }
    ));

    // Operand out of range: a local and a constant index.
    let mut bad = spec.clone();
    let at = find(Opcode::GetLocal);
    bad.code[at + 1..at + 3].copy_from_slice(&spec.locals.to_le_bytes());
    let e = Executable::load(bad).unwrap_err();
    assert_eq!(
        e.kind,
        ValidationErrorKind::LocalOutOfRange {
            index: spec.locals,
            count: spec.locals
        }
    );
    let mut bad = spec.clone();
    let at = find(Opcode::Const);
    bad.code[at + 1..at + 3].copy_from_slice(&99u16.to_le_bytes());
    assert!(matches!(
        Executable::load(bad).unwrap_err().kind,
        ValidationErrorKind::ConstantOutOfRange { index: 99, .. }
    ));
    // An unassigned ThrowError kind.
    let mut bad = spec.clone();
    let at = find(Opcode::GetLocal);
    bad.code[at] = Opcode::ThrowError as u8;
    bad.code[at + 1..at + 3].copy_from_slice(&7u16.to_le_bytes());
    assert_eq!(
        Executable::load(bad).unwrap_err().kind,
        ValidationErrorKind::UnknownThrowKind { code: 7 }
    );

    // Stack underflow: the first instruction (`const 0`, pushing the
    // initializer) becomes `pop` + `undefined` + `undefined`, so `pop` runs
    // on an empty stack.
    let mut bad = spec.clone();
    assert_eq!(bad.code[0], Opcode::Const as u8);
    bad.code[0..3].copy_from_slice(&[
        Opcode::Pop as u8,
        Opcode::Undefined as u8,
        Opcode::Undefined as u8,
    ]);
    assert_eq!(
        Executable::load(bad).unwrap_err().kind,
        ValidationErrorKind::StackUnderflow {
            opcode: Opcode::Pop,
            depth: 0
        }
    );
    // Stack-depth mismatch at the loop head: the body's last instruction
    // before the backward jump stores the statement value (`set_local`, net
    // -1); replacing it with `dup undefined pop` (net +1) leaves two extra
    // values on the back edge.
    let mut bad = spec.clone();
    let jump = instrs.iter().position(|(_, o)| *o == Opcode::Jump).unwrap();
    let (at, op) = instrs[jump - 1];
    assert_eq!(op, Opcode::SetLocal);
    bad.code[at..at + 3].copy_from_slice(&[
        Opcode::Dup as u8,
        Opcode::Undefined as u8,
        Opcode::Pop as u8,
    ]);
    assert!(matches!(
        Executable::load(bad).unwrap_err().kind,
        ValidationErrorKind::StackDepthMismatch {
            expected: 0,
            found: 2
        }
    ));
}

#[test]
fn deep_expressions_compile_within_a_small_stack() {
    // Near the parser's nesting budget: 60 nested parentheses and chains of
    // 240 additions and 240 comma operators. Compiling recurses once per tree
    // level, so it must fit the same 2 MiB thread stack the parser is tested
    // with.
    let nested = format!("{}1{}", "(".repeat(60), ")".repeat(60));
    let chain = vec!["1"; 241].join(" + ");
    let commas = vec!["1"; 241].join(", ");
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("nested", &nested).map(|_| ());
            let b = compile_script("chain", &chain).map(|_| ());
            let c = compile_script("commas", &commas).map(|_| ());
            (a, b, c)
        })
        .unwrap();
    let (a, b, c) = handle.join().unwrap();
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_eq!(num(&vec!["1"; 241].join(" + ")), 241.0);
    assert_eq!(
        num(&(1..=241)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")),
        241.0
    );
}

#[test]
fn entry_function_name_refers_to_the_running_function() {
    let complete = |src: &str, args: &[Primitive]| match run(&compile_function(src).unwrap(), args)
    {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{src}: {other:?}"),
    };
    let four = [Primitive::Number(4.0)];
    // Since clj-quickjs-21 the entry function's own name is the running
    // function object (`current_function`), so it can recurse.
    assert_eq!(
        complete("function f() { return f === f; }", &[]),
        ValueView::Bool(true)
    );
    assert_eq!(
        complete(
            "function fact(n) { return n <= 1 ? 1 : n * fact(n - 1); }",
            &four
        ),
        ValueView::Number(24.0)
    );
    let fact = compile_function("function f(n) { return n ? f(n - 1) : 0; }").unwrap();
    assert!(opcodes(&fact).contains(&Opcode::CurrentFunction));
    // Assigning it would change a binding of a script that does not exist.
    let e = compile_function("function f() { f = 1; }").unwrap_err();
    assert_eq!(
        (e.kind, e.line, e.column),
        (SyntaxErrorKind::Unsupported, 1, 16)
    );
    // Shadowed by a parameter or a body binding, the name is an ordinary local.
    assert_eq!(
        complete("function f(f) { return f * 2; }", &four),
        ValueView::Number(8.0)
    );
    assert_eq!(
        complete("function f() { let f = 3; return f; }", &[]),
        ValueView::Number(3.0)
    );
    assert_eq!(
        complete("function f() { { const f = 5; return f; } }", &[]),
        ValueView::Number(5.0)
    );
}

#[test]
fn expression_evaluation_order_and_conditional_short_circuiting() {
    // Operands are evaluated left to right; assignments have their value.
    assert_eq!(
        num("let x = 0; let y = (x = 1) + (x = 2); y * 10 + x"),
        32.0
    );
    assert_eq!(
        num("let x = 0; let y = (x = 5) - (x = 2); y * 10 + x"),
        32.0
    );
    assert_eq!(num("let x = 1; let y = x * (x = 3); y * 10 + x"), 33.0);
    assert_eq!(num("let a, b; a = b = 4; a + b"), 8.0);
    assert_eq!(num("let x = 0; let t = (x = 1) < (x = 2); t ? x : -1"), 2.0);
    // Only the selected branch of `?:` and `if` runs.
    assert_eq!(num("true ? 7 : missing"), 7.0);
    assert_eq!(num("false ? missing : 8"), 8.0);
    assert_eq!(num("let x = 0; 1 ? (x = 1) : (x = 2); x"), 1.0);
    assert_eq!(num("let x = 0; if (0) x = 1; else x = 2; x"), 2.0);
    // `&&` / `||` evaluate the right side only when needed.
    assert_eq!(
        num("let x = 0; 1 && (x = 3); 0 && (x = 4); 1 || (x = 5); x"),
        3.0
    );
    // A failing operand fails where it is, after the operands before it.
    let (kind, _, line, column) = failure_at("let x = 0;\n(x = missing) + 1");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 2, 6));
}

/// The comma operator (clj-quickjs-61): every operand runs, left to right,
/// and the last one is the value.
#[test]
fn comma_operator_evaluates_left_to_right() {
    assert_eq!(num("let a = 1, b = 2; a, b"), 2.0);
    assert_eq!(num("let x = 0; (x = 1, x + 1)"), 2.0);
    assert_eq!(num("let r = (1, 2, 3); r"), 3.0);
    assert_eq!(num("let x = 0; x = 5, x"), 5.0);
    assert_eq!(num("if (0, 1) 7; else 8;"), 7.0);
    // The left operand is evaluated, not skipped.
    let (kind, message, line, column) = failure_at("(nope, 1)");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::ReferenceError, "nope is not defined", 1, 2)
    );
    // Side effects happen in order; dropped values leave no trace.
    assert_eq!(
        num("let x = 1; let y = (x = x * 10, x = x + 2, x * 2); y * 1000 + x"),
        24012.0
    );
    assert_eq!(
        num("let i = 0, n = 0; while (n = n + 1, i < 3) i = i + 1; n"),
        4.0
    );
    assert_eq!(num("function f(a) { return a = a + 1, a * 2; } f(1)"), 4.0);
    assert_eq!(num("let g = (a, b) => (a, b); g(1, 2)"), 2.0);
    assert_eq!(num("() => {}\n, 1"), 1.0);
    // `throw` throws the last operand, after the others ran.
    assert_eq!(
        num("let x = 0; try { throw x = 1, 2; } catch (e) { x = x * 10 + e; } x"),
        12.0
    );
    assert_eq!(num("let h; h = a => a, 9; h(3)"), 3.0);
    // Only the last operand's value is converted by an arithmetic operator.
    assert_eq!(num("(1, null) + 1"), 1.0);
    assert_eq!(num("(null, 1) + 1"), 2.0);
    assert_eq!(num("1 - (true, 2)"), -1.0);
    assert_eq!(eval("1, undefined"), ValueView::Undefined);
    assert_eq!(eval("(1, 2) === 2 ? true : 0"), ValueView::Bool(true));
}

/// Bitwise and shift operators and `~` (clj-quickjs-62). The `test_op1`
/// and numeric `test_cvt` values are adapted from
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8 (each
/// `assert(r, v)` as a Rust assertion on the expression's value; the string
/// operand cases of `test_cvt` wait for StringToNumber); see
/// `docs/architecture.md`, "QuickJS references and provenance".
///
/// QuickJS is Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon,
/// released under the MIT license: Permission is hereby granted, free of
/// charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy,
/// modify, merge, publish, distribute, sublicense, and/or sell copies of
/// the Software, and to permit persons to whom the Software is furnished
/// to do so, subject to the following conditions: The above copyright
/// notice and this permission notice shall be included in all copies or
/// substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS",
/// WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
/// LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
/// PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
/// HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
/// AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
/// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
/// SOFTWARE.
#[test]
fn bitwise_operators_follow_to_int32() {
    // `test_op1`.
    for (src, expected) in [
        ("4 << 2", 16.0),
        ("1 << 0", 1.0),
        ("1 << 31", -2147483648.0),
        ("1 << 32", 1.0),
        ("-4 >> 1", -2.0),
        ("-4 >>> 1", 2147483646.0),
        ("1 & 1", 1.0),
        ("0 | 1", 1.0),
        ("1 ^ 1", 0.0),
        ("~1", -2.0),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
    assert_eq!(eval("(1 << 31) < 0"), ValueView::Bool(true));
    // Numeric groups of `test_cvt`.
    for (src, expected) in [
        ("NaN | 0", 0.0),
        ("Infinity | 0", 0.0),
        ("(-Infinity) | 0", 0.0),
        ("(4294967296 * 3 - 4) | 0", -4.0),
        ("NaN >>> 0", 0.0),
        ("Infinity >>> 0", 0.0),
        ("(-Infinity) >>> 0", 0.0),
        ("(4294967296 * 3 - 4) >>> 0", 4294967292.0),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
    assert_eq!(
        eval("((4294967296 * 3 - 4) >>> 0) === (4294967296 - 4)"),
        ValueView::Bool(true)
    );

    // ToInt32 / ToUint32 edges written for this project.
    for (src, expected) in [
        ("2147483648 | 0", -2147483648.0),
        ("2147483647 | 0", 2147483647.0),
        ("4294967295 | 0", -1.0),
        ("-2147483649 | 0", 2147483647.0),
        ("1e21 | 0", -559939584.0),
        ("-3.9 | 0", -3.0),
        ("1 << -1", -2147483648.0),
        ("1 << 33", 2.0),
        ("8 >> 4294967297", 4.0),
        ("1 >>> 32", 1.0),
        ("-1 >>> 0", 4294967295.0),
        ("-1 >>> 31", 1.0),
        ("-1 >> 31", -1.0),
        ("4294967295 >>> 0", 4294967295.0),
        ("~~3.7", 3.0),
        ("~~-3.7", -3.0),
        ("~NaN", -1.0),
        ("~Infinity", -1.0),
        ("~-1", 0.0),
        ("~2147483647", -2147483648.0),
        ("6 & 3", 2.0),
        ("6 | 3", 7.0),
        ("6 ^ 3", 5.0),
        ("5.9 & 7.9", 5.0),
        // ToNumber of the other primitives.
        ("true | 0", 1.0),
        ("false | 2", 2.0),
        ("null >>> 0", 0.0),
        ("undefined | 0", 0.0),
        ("~undefined", -1.0),
        ("~true", -2.0),
        ("let t = true; t << t", 2.0),
        // Precedence at run time.
        ("1 | 2 ^ 3 & 1", 3.0),
        ("1 << 2 + 1", 8.0),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
    assert_eq!(eval("2 & 1 == 1"), ValueView::Number(0.0));
    assert_eq!(eval("0 < 1 << 2"), ValueView::Bool(true));
    // Results are never -0.
    for src in [
        "-0 | 0", "-0 & -0", "-0 ^ 0", "-0 << 1", "-0 >> 0", "-0 >>> 0", "-0.5 | 0", "~-1", "~~-0",
        "-1 & 0",
    ] {
        let n = num(src);
        assert!(n == 0.0 && n.is_sign_positive(), "{src} gave {n}");
    }

    // Both operands are evaluated, left first, before any conversion.
    let (kind, message, line, column) = failure_at("nope | 1");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::ReferenceError, "nope is not defined", 1, 1)
    );
    assert_eq!(failure_at("1 >>> nope").0, VmErrorKind::ReferenceError);
    assert_eq!(
        failure_at("__rt.vector() & nope").0,
        VmErrorKind::ReferenceError
    );
    assert_eq!(
        num("let x = 0; let y = (x = 6) & (x = 3); y * 10 + x"),
        23.0
    );
    // Objects need ToPrimitive, which is deferred.
    for src in [
        "__rt.vector() | 0",
        "1 << __rt.vector()",
        "~__rt.vector()",
        "~(() => 1)",
    ] {
        assert_eq!(
            failure(src).kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}"
        );
    }

    // Each operator is one VM instruction, with no ToNumber for number
    // operands.
    for (src, op) in [
        ("1 & 2", Opcode::BitAnd),
        ("1 | 2", Opcode::BitOr),
        ("1 ^ 2", Opcode::BitXor),
        ("1 << 2", Opcode::Shl),
        ("1 >> 2", Opcode::Sar),
        ("1 >>> 2", Opcode::UShr),
        ("~1", Opcode::BitNot),
    ] {
        let ops = opcodes(&exe(src));
        assert!(ops.contains(&op), "{src}: {ops:?}");
        assert!(
            !ops.contains(&Opcode::ToNumber) && !ops.contains(&Opcode::ToNumberPair),
            "{src}: {ops:?}"
        );
    }
    assert!(
        opcodes(&exe("~~(1 | 2) << ~3"))
            .iter()
            .all(|op| !matches!(op, Opcode::ToNumber | Opcode::ToNumberPair))
    );
}

#[test]
fn compiler_constant_limit() {
    // 65,536 distinct constants fit the u16 index; the last one is read
    // back correctly (a wrapped index would read constant 0).
    let fits: String = (0..65_536).map(|i| format!("{i};")).collect();
    assert_eq!(num(&fits), 65_535.0);
    let over = format!("{fits}65536;");
    let e = compile_script("t", &over).unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::TooLarge);
    assert_eq!(e.span.text(&over), "65536");
    // Repeated constants share one pool entry.
    let repeated = "1;2;".repeat(40_000);
    assert_eq!(num(&repeated), 2.0);
    assert_eq!(exe(&repeated).to_spec().constants.len(), 2);
}

#[test]
fn compiler_local_limit() {
    // A script uses local 0 for its completion value, so 65,534 bindings
    // can be live at once and the 65,535th is rejected.
    let names = |n: usize, p: &str| {
        (0..n)
            .map(|i| format!("{p}{i} = {i}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let fits = format!("let {}; a65533", names(65_534, "a"));
    assert_eq!(exe(&fits).to_spec().locals, u16::MAX);
    assert_eq!(num(&fits), 65_533.0);
    let over = format!("let {};", names(65_535, "a"));
    let e = compile_script("t", &over).unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::TooLarge);
    assert_eq!(e.span.text(&over), "a65534");
    // Sibling blocks reuse slots: two blocks of 40,000 bindings each fit.
    let siblings = format!(
        "{{ let {}; }} {{ let {}; b39999 }}",
        names(40_000, "a"),
        names(40_000, "b")
    );
    assert_eq!(exe(&siblings).to_spec().locals, 40_001);
    assert_eq!(num(&siblings), 39_999.0);
}

#[test]
fn parenthesized_binding_diagnostics_name_the_identifier() {
    let cases = [
        ("(missing)", "missing is not defined", 1, 2),
        ("( /* x */\n  missing )", "missing is not defined", 2, 3),
        (
            "(early); let early = 1;",
            "cannot access 'early' before initialization",
            1,
            2,
        ),
        (
            "const c = 1;\n((c)) = 2;",
            "assignment to constant variable 'c'",
            2,
            3,
        ),
        ("( /* gone */ gone ) = 1;", "gone is not defined", 1, 14),
    ];
    for (src, message, line, column) in cases {
        let (_, m, l, c) = failure_at(src);
        assert_eq!((m.as_str(), l, c), (message, line, column), "{src}");
    }
}
